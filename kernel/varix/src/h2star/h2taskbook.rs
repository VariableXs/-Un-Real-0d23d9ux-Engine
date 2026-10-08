//! H2 域传输任务簿 · 深化批次二（服务层纵深——长复制的全生命周期）。
//!
//! **承接判据**（主册 H 域正文，一处一事实）：
//! - **F269 长复制暂停与恢复**：暂停即时生效（当前 4MB 块写完即停
//!   <1s）、恢复从断点续传（已复制部分校验后跳过）、暂停期间源占用
//!   释放、关机时未完成任务在下次开机询问「继续吗」——本引擎给出
//!   任务簿全状态机（Running/Paused/Done/Failed/Aborted）与关机
//!   询问清单，断点账委托 [`h2ledger::ResumeLedger`]（一处一事实）；
//! - **F270 文件操作错误重试**：批量第 37 个失败不中断——三选一
//!   （重试该批/跳过继续/全部中止）、中止时已完成项保留并列明
//!   「完成 36/100」、失败清单可导出文本（文件名+人话原因，给排障）。
//!
//! 时间纪律：时刻由调用方注入；块大小来自 h2knob `h2.f269.chunk_mb`
//! （默认 4MB）——本引擎按块推进，不持有真实 IO。

use crate::checks::CheckSet;
use crate::h2star::h2ledger::ResumeLedger;

use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// F269 任务簿
// ---------------------------------------------------------------------------

/// 传输任务状态机。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum XferState {
    Running,
    Paused,
    Done,
    Failed,
    /// 用户在关机询问时选了「不继续」——已完成的保留，任务封账。
    Abandoned,
}

impl XferState {
    pub fn terminal(self) -> bool {
        matches!(self, XferState::Done | XferState::Failed | XferState::Abandoned)
    }
}

/// 一个传输任务。
pub struct XferTask {
    pub src: String,
    pub dst: String,
    pub total_bytes: u64,
    /// 已确认落盘字节（块边界对齐——暂停语义的计量面）。
    pub done_bytes: u64,
    pub state: XferState,
    /// 断点账（块校验和——恢复时好块跳过、坏块重传）。
    pub ledger: ResumeLedger,
}

/// 块大小（字节；h2knob `h2.f269.chunk_mb` 默认 4MB 的字节值）。
pub const CHUNK_BYTES: u64 = 4 * 1024 * 1024;

/// 传输任务簿（顺序执行——并发度由 F057 IO 分级管，不在本引擎）。
pub struct TaskBook {
    tasks: Vec<XferTask>,
    /// 当前活动任务下标（None=空闲）。
    pub active: Option<usize>,
}

impl TaskBook {
    pub fn new() -> TaskBook {
        TaskBook { tasks: Vec::new(), active: None }
    }

    pub fn submit(&mut self, src: &str, dst: &str, total_bytes: u64) -> usize {
        let id = self.tasks.len();
        self.tasks.push(XferTask {
            src: String::from(src),
            dst: String::from(dst),
            total_bytes,
            done_bytes: 0,
            state: XferState::Paused, // 提交后待启动——启动权在调度面。
            ledger: ResumeLedger::new(&alloc::format!("{src}→{dst}"), total_bytes, CHUNK_BYTES),
        });
        id
    }

    pub fn task(&self, id: usize) -> Option<&XferTask> {
        self.tasks.get(id)
    }

    pub fn task_mut(&mut self, id: usize) -> Option<&mut XferTask> {
        self.tasks.get_mut(id)
    }

    /// 启动/恢复（Paused → Running；终态拒绝——非法迁移诚实失败）。
    pub fn start(&mut self, id: usize, now_min: u64) -> bool {
        match self.tasks.get_mut(id) {
            Some(t) if t.state == XferState::Paused => {
                t.state = XferState::Running;
                t.ledger.pause(now_min); // 断点时刻=最近一次启停点。
                self.active = Some(id);
                true
            }
            _ => false,
        }
    }

    /// 暂停（Running → Paused）：进度收口到块边界——「当前块写完
    /// 即停 <1s」的计量保证（进度永远不是块中间态）。
    pub fn pause(&mut self, id: usize) -> bool {
        match self.tasks.get_mut(id) {
            Some(t) if t.state == XferState::Running => {
                let whole_blocks = t.done_bytes / CHUNK_BYTES * CHUNK_BYTES;
                t.done_bytes = whole_blocks;
                t.state = XferState::Paused;
                if self.active == Some(id) {
                    self.active = None;
                }
                true
            }
            _ => false,
        }
    }

    /// 推进（调度面每写完一块调用）：块校验和入账；写满即 Done。
    pub fn advance_block(&mut self, id: usize, content: &[u8]) -> bool {
        let (block_idx, ok) = match self.tasks.get_mut(id) {
            Some(t) if t.state == XferState::Running => {
                let idx = (t.done_bytes / CHUNK_BYTES) as usize;
                let marked = t.ledger.mark_done(idx, content);
                t.done_bytes += content.len() as u64;
                if t.done_bytes >= t.total_bytes {
                    t.state = XferState::Done;
                    if self.active == Some(id) {
                        self.active = None;
                    }
                }
                (idx, marked)
            }
            _ => return false,
        };
        let _ = ok;
        true
    }

    /// 关机询问清单（F269 判据「关机时未完成的复制任务下次开机询问
    /// 『继续吗』」）：所有 Running/Paused 任务按序返回。
    pub fn shutdown_pending(&self) -> Vec<usize> {
        self.tasks
            .iter()
            .enumerate()
            .filter(|(_, t)| matches!(t.state, XferState::Running | XferState::Paused))
            .map(|(i, _)| i)
            .collect()
    }

    /// 关机询问的选择落账：继续（保持 Paused 待下次启动）或放弃
    /// （Abandoned 封账——已完成部分保留）。
    pub fn resolve_shutdown(&mut self, id: usize, keep: bool) -> bool {
        match self.tasks.get_mut(id) {
            Some(t) if !t.state.terminal() && t.state != XferState::Running => {
                if !keep {
                    t.state = XferState::Abandoned;
                }
                true
            }
            _ => false,
        }
    }

    pub fn len(&self) -> usize {
        self.tasks.len()
    }
}

// ---------------------------------------------------------------------------
// F270 批量操作簿
// ---------------------------------------------------------------------------

/// 批量操作的一个条目。
pub struct OpItem {
    pub path: String,
    /// 人话失败原因（None=未失败）。
    pub failure: Option<&'static str>,
    pub done: bool,
}

/// 三选一（判据原文：重试该批/跳过继续/全部中止）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BatchChoice {
    RetryFailed,
    SkipContinue,
    AbortAll,
}

/// 批量操作簿：错误不中断整体，账目到条目级。
pub struct BatchOp {
    pub items: Vec<OpItem>,
    pub aborted: bool,
}

impl BatchOp {
    pub fn new(paths: &[&str]) -> BatchOp {
        BatchOp {
            items: paths
                .iter()
                .map(|p| OpItem { path: String::from(*p), failure: None, done: false })
                .collect(),
            aborted: false,
        }
    }

    /// 单条失败落账（不影响其余——判据「一个坏文件不毁一批工作」）。
    pub fn fail_one(&mut self, idx: usize, reason: &'static str) -> bool {
        match self.items.get_mut(idx) {
            Some(it) if !it.done => {
                it.failure = Some(reason);
                true
            }
            _ => false,
        }
    }

    /// 单条成功落账。
    pub fn finish_one(&mut self, idx: usize) -> bool {
        match self.items.get_mut(idx) {
            Some(it) => {
                it.done = true;
                it.failure = None;
                true
            }
            _ => false,
        }
    }

    /// 失败清单（当前未完成的失败项）。
    pub fn failed(&self) -> Vec<usize> {
        self.items
            .iter()
            .enumerate()
            .filter(|(_, it)| it.failure.is_some() && !it.done)
            .map(|(i, _)| i)
            .collect()
    }

    /// 三选一执行。
    /// - RetryFailed：失败项重新入队（failure 清位待重试）；
    /// - SkipContinue：失败项记「跳过」，其余继续（清单保留供导出）；
    /// - AbortAll：整体中止，已完成项保留（判据「完成 36/100」）。
    pub fn resolve(&mut self, choice: BatchChoice) -> (usize, usize) {
        let failed = self.failed();
        match choice {
            BatchChoice::RetryFailed => {
                let n = failed.len();
                for i in failed {
                    self.items[i].failure = None;
                }
                (n, 0)
            }
            BatchChoice::SkipContinue => (0, failed.len()),
            BatchChoice::AbortAll => {
                self.aborted = true;
                (0, failed.len())
            }
        }
    }

    /// 完成计数（「完成 36/100」人话账）。
    pub fn done_count(&self) -> usize {
        self.items.iter().filter(|it| it.done).count()
    }

    /// 导出文本（失败清单可导出——文件名+人话原因逐行；中止时带
    /// 完成计数头）。格式稳定可 grep——排障脚本直读。
    pub fn export_report(&self) -> String {
        let mut out = String::new();
        if self.aborted {
            out.push_str(&alloc::format!(
                "批量操作已中止——完成 {}/{}：\n",
                self.done_count(),
                self.items.len()
            ));
        }
        let failed = self.failed();
        if failed.is_empty() {
            out.push_str("无失败项\n");
            return out;
        }
        out.push_str(&alloc::format!("失败 {} 项：\n", failed.len()));
        for i in failed {
            let it = &self.items[i];
            out.push_str(&alloc::format!(
                "- {} — {}\n",
                it.path,
                it.failure.unwrap_or("原因未登记")
            ));
        }
        out
    }
}

// ---------------------------------------------------------------------------
// 自检（判据逐条钉死）
// ---------------------------------------------------------------------------

pub fn run_h2taskbook_checks() -> CheckSet {
    let mut set = CheckSet::new("h2-h2taskbook");
    // --- F269 状态机与块边界暂停。 ---
    let mut book = TaskBook::new();
    let total = CHUNK_BYTES * 5 + 100; // 5 块 + 尾。
    let id = book.submit("S:\\影片.mkv", "D:\\备份\\影片.mkv", total);
    set.add("h2taskbook submit paused", book.task(id).unwrap().state == XferState::Paused, "await start");
    assert!(book.start(id, 10));
    // 推进 2 块 + 半块后暂停 → 收口到 2 块整。
    let half = (CHUNK_BYTES / 2) as usize;
    let _ = book.advance_block(id, &alloc::vec![0u8; CHUNK_BYTES as usize]);
    let _ = book.advance_block(id, &alloc::vec![0u8; CHUNK_BYTES as usize]);
    let _ = book.advance_block(id, &alloc::vec![0u8; half]);
    set.add(
        "h2taskbook mid-block",
        book.task(id).unwrap().done_bytes == CHUNK_BYTES * 2 + half as u64,
        "running keeps raw",
    );
    let _ = book.pause(id);
    set.add(
        "h2taskbook pause block-boundary",
        book.task(id).unwrap().done_bytes == CHUNK_BYTES * 2
            && book.task(id).unwrap().state == XferState::Paused,
        "<1s stop",
    );
    // 恢复 + 写满 → Done。
    assert!(book.start(id, 20));
    for i in 0..4u64 {
        let left = total - book.task(id).unwrap().done_bytes;
        let n = left.min(CHUNK_BYTES) as usize;
        let _ = book.advance_block(id, &alloc::vec![1u8; n]);
    }
    set.add(
        "h2taskbook done",
        book.task(id).unwrap().state == XferState::Done && book.task(id).unwrap().done_bytes == total,
        "full transfer",
    );
    // --- F269 断点续传：好块跳过、坏块重传（委托 h2ledger 校验）。 ---
    let mut book2 = TaskBook::new();
    let id2 = book2.submit("a", "b", CHUNK_BYTES * 3);
    assert!(book2.start(id2, 0));
    let good: Vec<u8> = (0..CHUNK_BYTES as usize).map(|i| (i % 251) as u8).collect();
    let _ = book2.advance_block(id2, &good);
    let _ = book2.advance_block(id2, &good);
    let _ = book2.pause(id2);
    // 人为截断：第 1 块（中间块）内容变了 → 计划恢复时坏块重传。
    // 源数据拼装 = 好 + 坏 + 好（标记的是块 0/1 的 good——坏块在第 1 位）。
    let mut corrupted = good.clone();
    corrupted[777] = corrupted[777].wrapping_add(1);
    let mut src = good.clone();
    src.extend_from_slice(&corrupted);
    src.extend_from_slice(&good);
    let (skip, resend) = book2.task(id2).unwrap().ledger.plan_resume(&src);
    set.add(
        "h2taskbook resume plan",
        skip == 1 && resend == alloc::vec![1, 2],
        "skip good resend bad",
    );
    // --- F269 关机询问。 ---
    let mut book3 = TaskBook::new();
    let t1 = book3.submit("a", "b", 100);
    let t2 = book3.submit("c", "d", 100);
    let _ = book3.resolve_shutdown(t2, false); // 未启动也可放弃。
    set.add(
        "h2taskbook shutdown ask",
        book3.shutdown_pending() == alloc::vec![t1] && book3.task(t2).unwrap().state == XferState::Abandoned,
        "pending only",
    );
    // --- F270 批量三选一 + 导出。 ---
    let mut batch = BatchOp::new(&["f1", "f2", "f3", "f4", "f5"]);
    for i in 0..3 {
        let _ = batch.finish_one(i);
    }
    let _ = batch.fail_one(3, "文件被另一个程序占用——关闭占用它的程序后重试");
    let _ = batch.fail_one(4, "目标磁盘已满——清理空间或换目标");
    set.add(
        "h2taskbook fail no interrupt",
        batch.done_count() == 3 && batch.failed() == alloc::vec![3, 4],
        "37th dies, 38th lives",
    );
    let _ = batch.resolve(BatchChoice::RetryFailed);
    set.add(
        "h2taskbook retry clears",
        batch.failed().is_empty() && batch.done_count() == 3,
        "back in queue",
    );
    // 重试后 3/4 两项再次失败（真实重试常常再挂），跳过继续 → 清单留底。
    let _ = batch.fail_one(3, "文件被另一个程序占用——关闭占用它的程序后重试");
    let _ = batch.fail_one(4, "目标磁盘已满——清理空间或换目标");
    let _ = batch.resolve(BatchChoice::SkipContinue);
    let report = batch.export_report();
    set.add(
        "h2taskbook export",
        report.contains("f4") && report.contains("f5") && report.contains("目标磁盘已满"),
        "names+reasons",
    );
    let mut batch2 = BatchOp::new(&["x1", "x2"]);
    let _ = batch2.finish_one(0);
    let _ = batch2.fail_one(1, "路径不存在");
    let _ = batch2.resolve(BatchChoice::AbortAll);
    let rep2 = batch2.export_report();
    set.add(
        "h2taskbook abort keeps",
        batch2.aborted && rep2.contains("完成 1/2") && rep2.contains("x1") == false,
        "36/100 account",
    );
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn h2taskbook_all_green() {
        let set = run_h2taskbook_checks();
        let (p, f) = set.tally();
        assert!(set.all_passed(), "h2taskbook 自检红 {f}/{p}");
    }

    #[test]
    fn terminal_states_locked() {
        let mut book = TaskBook::new();
        let id = book.submit("a", "b", CHUNK_BYTES);
        assert!(book.start(id, 0));
        let chunk = alloc::vec![0u8; CHUNK_BYTES as usize];
        let _ = book.advance_block(id, &chunk);
        assert!(book.task(id).unwrap().state == XferState::Done);
        assert!(!book.pause(id), "终态不可暂停");
        assert!(!book.start(id, 5), "终态不可重启");
    }

    #[test]
    fn fail_unknown_index_honest() {
        let mut b = BatchOp::new(&["a"]);
        assert!(!b.fail_one(99, "x"), "越界失败落账拒绝");
        assert!(!b.finish_one(99), "越界成功落账拒绝");
    }
}
