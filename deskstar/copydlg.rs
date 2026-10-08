//! F086 复制/移动进度对话框 · 完整设计（STAR I 主册 G-C-16）。
//!
//! **判据（主册）**：40GB 拷贝全程对话框不卡（UI 线程隔离 B-18xx
//! 判据）；暂停-恢复-续传实测；冲突面板三选全对。
//!
//! **设计要点（主册）**：
//! - 文件操作进度对话框：总进度+当前文件双进度条、速度曲线、剩余
//!   时间、暂停/取消；冲突合并处理（F087）；「后台化」按钮——关
//!   对话框任务继续，托盘小图标续命；
//! - 对话框 480×280px：文件名行（当前文件）/双进度条（总 8px+当前
//!   4px）/速度+剩余时间行/三钮（暂停/取消/后台化）；取消=确认
//!   （已拷部分保留+说明，不静默丢）；冲突弹 F087 面板（应用到此
//!   文件，全选应用）；
//! - 任务队列持久化（重启后未完成任务询问续传）；速度采样 1s 粒度；
//! - 源文件中途消失 → 该文件跳过+汇总报告；目标盘满 → 暂停+三选
//!   （清理/换目标/取消）；U 盘拔出 → 任务失败如实+已拷清单；多
//!   任务并发排队（上限 3 并行，其余队列）；
//! - 剩余时间用滑动窗口均速（防抖动数字跳）；速度曲线图 120px 宽
//!   迷你图；后台化后托盘图标带迷你进度环；对话框位置记忆；多冲突
//!   批量面板单屏最多 20 条（超出分页）；完成音效（F079 回收站族旁
//!   「任务完成」事件）。
//!
//! 实装口径：任务队列账（3 并行上限）+ 进度/速度/ETA 账（dbase
//! SlidingRate 同源）+ 暂停/取消/后台化状态机 + 磁盘满三选账 + UI
//! 线程隔离账（进度更新节拍与传输解耦）。时间注入式。

use crate::checks::CheckSet;

use crate::deskstar::dbase::{budget_ok, SlidingRate};
use alloc::string::String;
use alloc::vec::Vec;
use alloc::{vec, format};

// ---------------------------------------------------------------------------
// 规格常量（参数唯一源——主册交互设计/状态与异常/设计细节）
// ---------------------------------------------------------------------------

/// 对话框宽（px）。
pub const DLG_W_PX: i32 = 480;

/// 对话框高（px）。
pub const DLG_H_PX: i32 = 280;

/// 速度曲线迷你图宽（px）。
pub const SPARK_W_PX: i32 = 120;

/// 速度采样粒度（s）。
pub const SAMPLE_SECS: u64 = 1;

/// 并行任务上限。
pub const PARALLEL_CAP: usize = 3;

/// UI 线程节流（ms——进度刷新与传输解耦：间隔内不唤醒 UI）。
pub const UI_THROTTLE_MS: u64 = 100;

// ---------------------------------------------------------------------------
// 模型
// ---------------------------------------------------------------------------

/// 任务状态机。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskState {
    Queued,
    Running,
    Paused,
    /// 磁盘满暂停（三选待决）。
    StalledFull,
    Failed,
    Done,
    Cancelled,
}

/// 磁盘满三选。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StallChoice {
    /// 清理目标盘后继续。
    Cleanup,
    /// 换目标。
    Retarget,
    /// 取消。
    Cancel,
}

/// 文件操作任务。
pub struct FileTask {
    pub id: u64,
    pub kind: OpKind,
    pub state: TaskState,
    /// 文件清单（字节大小；源文件中途消失 → 跳过+汇总）。
    pub files: Vec<(String, u64)>,
    /// 当前进度（字节）。
    pub copied: u64,
    /// 当前文件下标。
    pub cur_file: usize,
    /// 当前文件内进度。
    pub cur_copied: u64,
    /// 跳过清单（源消失）。
    pub skipped: Vec<String>,
    /// 已拷清单（U 盘拔出失败如实呈现）。
    pub copied_list: Vec<String>,
    /// 速度账（1s 粒度，滑动窗口均速）。
    rate: SlidingRate,
    /// 后台化标志（对话框关了任务继续）。
    pub backgrounded: bool,
    /// 对话框位置记忆。
    pub dlg_pos: (i32, i32),
    /// 冲突待决清单（F087 面板接缝：决策回填前的挂起）。
    pub conflicts_pending: Vec<String>,
    /// 冲突决策表（可回看——完成摘要数据源）。
    pub decisions: Vec<(String, &'static str)>,
}

/// 操作类型。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpKind {
    Copy,
    Move,
}

impl OpKind {
    pub fn name(self) -> &'static str {
        match self {
            OpKind::Copy => "复制",
            OpKind::Move => "移动",
        }
    }
}

impl FileTask {
    fn new(id: u64, kind: OpKind, files: Vec<(String, u64)>) -> FileTask {
        FileTask {
            id,
            kind,
            state: TaskState::Queued,
            files,
            copied: 0,
            cur_file: 0,
            cur_copied: 0,
            skipped: Vec::new(),
            copied_list: Vec::new(),
            rate: SlidingRate::new(5),
            backgrounded: false,
            dlg_pos: (0, 0),
            conflicts_pending: Vec::new(),
            decisions: Vec::new(),
        }
    }

    fn total_bytes(&self) -> u64 {
        self.files.iter().map(|(_, s)| *s).sum()
    }

    fn remain_bytes(&self) -> u64 {
        self.total_bytes() - self.copied.min(self.total_bytes())
    }

    /// 速度回执（1s 粒度入滑动窗）。
    fn report_rate(&mut self, sec: u64, bytes_per_s: u64) {
        self.rate.sample(sec, bytes_per_s);
    }

    /// 剩余时间（滑动均速估算；速率 0 → None 诚实不编）。
    fn eta_secs(&self) -> Option<u64> {
        self.rate.eta_secs(self.remain_bytes())
    }

    /// 总进度（千分比）。
    fn total_permille(&self) -> u16 {
        let t = self.total_bytes();
        if t == 0 {
            return 1000;
        }
        ((self.copied.min(t)) * 1000 / t) as u16
    }

    /// 当前文件进度（千分比）。
    fn cur_permille(&self) -> u16 {
        let Some((_, s)) = self.files.get(self.cur_file) else {
            return 1000;
        };
        if *s == 0 {
            return 1000;
        }
        ((self.cur_copied.min(*s)) * 1000 / s) as u16
    }

    fn current_name(&self) -> Option<&str> {
        self.files.get(self.cur_file).map(|(n, _)| n.as_str())
    }
}

// ---------------------------------------------------------------------------
// 任务队列管理器
// ---------------------------------------------------------------------------

/// 复制/移动任务队列。
pub struct CopyMgr {
    tasks: Vec<FileTask>,
    next_id: u64,
    now_ms: u64,
    /// UI 唤醒节拍账（节流内不唤醒——40GB 不卡判据的记账面）。
    last_ui_wake: u64,
    pub ui_wakes: u64,
    /// 完成音效事件账（F079「任务完成」）。
    pub completion_events: u64,
}

impl CopyMgr {
    pub fn new() -> CopyMgr {
        CopyMgr {
            tasks: Vec::new(),
            next_id: 1,
            now_ms: 0,
            last_ui_wake: 0,
            ui_wakes: 0,
            completion_events: 0,
        }
    }

    /// 提交任务（超并行上限入队列）。
    pub fn submit(&mut self, kind: OpKind, files: Vec<(String, u64)>) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        let mut t = FileTask::new(id, kind, files);
        let running = self.running_count() < PARALLEL_CAP;
        t.state = if running { TaskState::Running } else { TaskState::Queued };
        self.tasks.push(t);
        id
    }

    fn running_count(&self) -> usize {
        self.tasks
            .iter()
            .filter(|t| t.state == TaskState::Running)
            .count()
    }

    pub fn task(&self, id: u64) -> Option<&FileTask> {
        self.tasks.iter().find(|t| t.id == id)
    }

    fn task_mut(&mut self, id: u64) -> Option<&mut FileTask> {
        self.tasks.iter_mut().find(|t| t.id == id)
    }

    /// 传输进度回执（传输线程序调用；UI 节流记账在此处）。
    pub fn progress(&mut self, id: u64, bytes_delta: u64, sec: u64, now_ms: u64) {
        self.now_ms = now_ms;
        // UI 节流：间隔内不唤醒（进度账照记——隔离语义）。
        if now_ms.saturating_sub(self.last_ui_wake) >= UI_THROTTLE_MS {
            self.ui_wakes += 1;
            self.last_ui_wake = now_ms;
        }
        if let Some(t) = self.task_mut(id) {
            if t.state != TaskState::Running {
                return;
            }
            let (_, size) = t.files[t.cur_file];
            t.cur_copied = (t.cur_copied + bytes_delta).min(size);
            t.copied += bytes_delta;
            t.report_rate(sec, bytes_delta.max(1));
            if t.cur_copied >= size {
                let name = t.files[t.cur_file].0.clone();
                t.copied_list.push(name);
                t.cur_file += 1;
                t.cur_copied = 0;
                if t.cur_file >= t.files.len() {
                    t.state = TaskState::Done;
                }
            }
        }
    }

    /// 完成事件回收（Done 状态被驱动确认后触发完成音效账）。
    pub fn collect_completions(&mut self) -> u64 {
        let done = self
            .tasks
            .iter()
            .filter(|t| t.state == TaskState::Done && !t.backgrounded)
            .count() as u64;
        self.completion_events += done;
        done
    }

    /// 暂停/恢复。
    pub fn pause(&mut self, id: u64) -> bool {
        match self.task_mut(id) {
            Some(t) if t.state == TaskState::Running => {
                t.state = TaskState::Paused;
                true
            }
            _ => false,
        }
    }

    pub fn resume(&mut self, id: u64) -> bool {
        match self.task_mut(id) {
            Some(t) if matches!(t.state, TaskState::Paused | TaskState::StalledFull) => {
                t.state = TaskState::Running;
                true
            }
            _ => false,
        }
    }

    /// 队列驱动：有空位则让队首 Queued 任务接棒（任务完成/取消后调用）。
    pub fn pump(&mut self) -> usize {
        let mut promoted = 0;
        while self.running_count() < PARALLEL_CAP {
            let next = self
                .tasks
                .iter_mut()
                .find(|t| t.state == TaskState::Queued)
                .map(|t| t.id);
            match next {
                Some(id) => {
                    if let Some(t) = self.task_mut(id) {
                        t.state = TaskState::Running;
                    }
                    promoted += 1;
                }
                None => break,
            }
        }
        promoted
    }

    /// 取消 = 确认（已拷部分保留 + 说明——不静默丢）。
    pub fn cancel(&mut self, id: u64) -> Option<(u64, usize)> {
        match self.task_mut(id) {
            Some(t) if matches!(
                t.state,
                TaskState::Running | TaskState::Paused | TaskState::StalledFull
            ) =>
            {
                t.state = TaskState::Cancelled;
                let partial = t.copied;
                let kept = t.copied_list.len();
                Some((partial, kept))
            }
            _ => None,
        }
    }

    /// 后台化（对话框关，任务继续——托盘迷你进度环数据源不变）。
    pub fn background(&mut self, id: u64) -> bool {
        match self.task_mut(id) {
            Some(t) if matches!(t.state, TaskState::Running | TaskState::Paused) => {
                t.backgrounded = true;
                true
            }
            _ => false,
        }
    }

    /// 源文件中途消失（跳过 + 汇总报告）。
    pub fn source_vanished(&mut self, id: u64, name: &str) -> bool {
        match self.task_mut(id) {
            Some(t) => {
                if let Some(pos) = t.files.iter().position(|(n, _)| n == name) {
                    t.skipped.push(String::from(name));
                    // 跳过 = 从清单摘除并把游标保持（总账相应减）。
                    let (_, size) = t.files.remove(pos);
                    let _ = size;
                    if t.cur_file >= t.files.len() && !t.files.is_empty() {
                        t.cur_file = t.files.len() - 1;
                    }
                    if t.files.is_empty() {
                        t.state = TaskState::Done;
                    }
                    true
                } else {
                    false
                }
            }
            None => false,
        }
    }

    /// 目标盘满 → 暂停 + 三选。
    pub fn disk_full(&mut self, id: u64) -> bool {
        match self.task_mut(id) {
            Some(t) if t.state == TaskState::Running => {
                t.state = TaskState::StalledFull;
                true
            }
            _ => false,
        }
    }

    /// 三选执行。
    pub fn stall_choice(&mut self, id: u64, choice: StallChoice) -> bool {
        match choice {
            StallChoice::Cleanup | StallChoice::Retarget => self.resume(id),
            StallChoice::Cancel => self.cancel(id).is_some(),
        }
    }

    /// U 盘拔出 → 任务失败如实 + 已拷清单。
    pub fn device_yanked(&mut self, id: u64) -> Option<Vec<String>> {
        match self.task_mut(id) {
            Some(t)
                if matches!(
                    t.state,
                    TaskState::Running | TaskState::Paused | TaskState::StalledFull
                ) =>
            {
                t.state = TaskState::Failed;
                Some(t.copied_list.clone())
            }
            _ => None,
        }
    }

    /// 冲突上抛（F087 面板接缝——决策回填前挂起该文件）。
    pub fn raise_conflict(&mut self, id: u64, name: &str) -> bool {
        match self.task_mut(id) {
            Some(t) => {
                t.conflicts_pending.push(String::from(name));
                true
            }
            None => false,
        }
    }

    /// 冲突决策回填（决策表可回看——完成摘要数据源）。
    pub fn resolve_conflict(&mut self, id: u64, name: &str, decision: &'static str) -> bool {
        match self.task_mut(id) {
            Some(t) => {
                let before = t.conflicts_pending.len();
                t.conflicts_pending.retain(|n| n != name);
                if t.conflicts_pending.len() < before {
                    t.decisions.push((String::from(name), decision));
                    true
                } else {
                    false
                }
            }
            None => false,
        }
    }

    /// 批量冲突分页账：单屏最多 20 条（超出分页）。
    pub fn conflict_pages(n_conflicts: usize) -> usize {
        (n_conflicts + 19) / 20
    }

    /// 对话框位置记忆。
    pub fn remember_pos(&mut self, id: u64, x: i32, y: i32) -> bool {
        match self.task_mut(id) {
            Some(t) => {
                t.dlg_pos = (x, y);
                true
            }
            None => false,
        }
    }

    /// 重启续传询问面（任务队列持久化标志——序列化由存储层接手，
    /// 本账保证未完成任务可被枚举）。
    pub fn resumable_tasks(&self) -> Vec<u64> {
        self.tasks
            .iter()
            .filter(|t| matches!(t.state, TaskState::Paused | TaskState::StalledFull | TaskState::Cancelled))
            .map(|t| t.id)
            .collect()
    }
}

// ---------------------------------------------------------------------------
// 自检（判据唯一源：主册 G-C-16 验收判据）
// ---------------------------------------------------------------------------

/// F086 自检：暂停-恢复-续传、取消保留已拷、冲突三选回填、源消失
/// 跳过、盘满三选、U 盘拔出如实、3 并行上限、UI 节流、后台化、
/// 40GB 账、分页、位置记忆、完成音效。
pub fn run_copydlg_checks() -> CheckSet {
    let mut set = CheckSet::new("deskstar-F086");
    let mut m = CopyMgr::new();
    // 1. 40GB 任务：传输侧高频回报，UI 侧节流唤醒（隔离判据的记账面）。
    let big: Vec<(String, u64)> = vec![(String::from("素材.pack"), 40 * 1024 * 1024 * 1024u64)];
    let id = m.submit(OpKind::Copy, big);
    let t = m.task(id).unwrap();
    set.add(
        "dlg-geometry",
        DLG_W_PX == 480 && DLG_H_PX == 280 && t.total_bytes() == 40 * 1024 * 1024 * 1024,
        "480×280 / 40GB",
    );
    let mut wakes0 = m.ui_wakes;
    let steps = 4000u64;
    let chunk = (40u64 * 1024 * 1024 * 1024) / steps;
    for i in 0..steps {
        m.progress(id, chunk, i, i * 10); // 传输 100ms 级 4000 次回报
    }
    // 尾差补齐（40GB 非整除 4000 份——最后一份补足）。
    let tail = 40u64 * 1024 * 1024 * 1024 - chunk * steps;
    m.progress(id, tail, steps, steps * 10);
    wakes0 = m.ui_wakes - wakes0;
    set.add(
        "ui-isolated",
        wakes0 <= (steps * 10) / UI_THROTTLE_MS + 2 && m.task(id).unwrap().total_permille() == 1000,
        "throttled UI wakes",
    );
    // 2. 暂停-恢复-续传。
    let mut m2 = CopyMgr::new();
    let id2 = m2.submit(
        OpKind::Copy,
        vec![(String::from("a"), 1_000), (String::from("b"), 1_000)],
    );
    m2.progress(id2, 400, 0, 0);
    let cur_ok = {
        let t = m2.task(id2).unwrap();
        t.cur_permille() == 400 && t.current_name() == Some("a")
    };
    let p = m2.pause(id2);
    m2.progress(id2, 400, 1, 100); // 暂停期回报被拒
    let frozen = m2.task(id2).unwrap().copied == 400;
    let r = m2.resume(id2);
    m2.progress(id2, 600, 2, 200);
    m2.progress(id2, 1000, 3, 300);
    let done = m2.task(id2).unwrap().state == TaskState::Done
        && m2.task(id2).unwrap().total_permille() == 1000;
    set.add("pause-resume", p && frozen && r && done && cur_ok, "pause/resume keep");
    // 3. 取消 = 确认（已拷部分保留 + 已拷清单）。
    let mut m3 = CopyMgr::new();
    let id3 = m3.submit(OpKind::Move, vec![(String::from("x"), 800)]);
    m3.progress(id3, 500, 0, 0);
    let cancel = m3.cancel(id3);
    let t3 = m3.task(id3).unwrap();
    set.add(
        "cancel-keep",
        cancel == Some((500, 0)) && t3.state == TaskState::Cancelled && t3.copied == 500,
        "partial kept, honest",
    );
    // 4. 源消失跳过 + 汇总。
    let mut m4 = CopyMgr::new();
    let id4 = m4.submit(
        OpKind::Copy,
        vec![(String::from("gone"), 100), (String::from("ok"), 100)],
    );
    m4.source_vanished(id4, "gone");
    m4.progress(id4, 100, 0, 0);
    let t4 = m4.task(id4).unwrap();
    set.add(
        "vanish-skip",
        t4.skipped == vec![String::from("gone")] && t4.state == TaskState::Done,
        "skip + summary",
    );
    // 5. 盘满三选。
    let mut m5 = CopyMgr::new();
    let id5 = m5.submit(OpKind::Copy, vec![(String::from("z"), 100)]);
    m5.disk_full(id5);
    let stalled = m5.task(id5).unwrap().state == TaskState::StalledFull;
    let resumed = m5.stall_choice(id5, StallChoice::Cleanup);
    set.add("stall-3choice", stalled && resumed, "cleanup resumes");
    // 6. U 盘拔出：失败如实 + 已拷清单。
    let mut m6 = CopyMgr::new();
    let id6 = m6.submit(
        OpKind::Copy,
        vec![(String::from("p1"), 100), (String::from("p2"), 100)],
    );
    m6.progress(id6, 100, 0, 0); // p1 完成
    m6.progress(id6, 50, 1, 10); // p2 部分
    let yanked = m6.device_yanked(id6);
    let t6 = m6.task(id6).unwrap();
    set.add(
        "yank-honest",
        yanked.map(|l| l.len()) == Some(1) && t6.state == TaskState::Failed,
        "fail + copied list",
    );
    // 7. 并行上限 3（前 3 Running、其余 Queued；取消后 pump 接棒）。
    let mut m7 = CopyMgr::new();
    let mut ids7 = Vec::new();
    for i in 0..5u64 {
        ids7.push(m7.submit(OpKind::Copy, vec![(format!("f{i}"), 10)]));
    }
    let running = ids7
        .iter()
        .filter(|id| m7.task(**id).unwrap().state == TaskState::Running)
        .count();
    let queued = ids7
        .iter()
        .filter(|id| m7.task(**id).unwrap().state == TaskState::Queued)
        .count();
    m7.cancel(ids7[0]);
    let promoted = m7.pump();
    set.add(
        "parallel-cap",
        running == 3 && queued == 2 && promoted == 1,
        "3 parallel, rest queued",
    );
    // 8. 冲突三选回填 + 决策表回看 + 分页。
    let mut m8 = CopyMgr::new();
    let id8 = m8.submit(OpKind::Copy, vec![(String::from("c"), 10)]);
    m8.raise_conflict(id8, "c");
    let resolved = m8.resolve_conflict(id8, "c", "保留两者");
    let t8 = m8.task(id8).unwrap();
    set.add(
        "conflict-f087",
        resolved
            && t8.decisions == vec![(String::from("c"), "保留两者")]
            && CopyMgr::conflict_pages(45) == 3,
        "decisions + paging 20",
    );
    // 9. 后台化 + 位置记忆 + 完成音效。
    let mut m9 = CopyMgr::new();
    let id9 = m9.submit(OpKind::Copy, vec![(String::from("w"), 100)]);
    m9.remember_pos(id9, 120, 90);
    m9.background(id9);
    m9.progress(id9, 100, 0, 0);
    let t9 = m9.task(id9).unwrap();
    let bg = t9.backgrounded && t9.dlg_pos == (120, 90) && t9.state == TaskState::Done;
    let ev = m9.collect_completions();
    set.add(
        "bg-position-sfx",
        bg && ev == 0 && m9.completion_events == 0,
        "bg keeps running, no toast spam",
    );
    // 10. ETA 滑动均速（不跳数字）。
    let mut m10 = CopyMgr::new();
    let id10 = m10.submit(OpKind::Copy, vec![(String::from("e"), 10_000)]);
    m10.progress(id10, 1000, 1, 0);
    m10.progress(id10, 1000, 2, 10);
    let t10 = m10.task(id10).unwrap();
    let eta = t10.eta_secs();
    set.add(
        "eta-stable",
        eta.is_some() && budget_ok(eta.unwrap(), 9_000),
        "sliding rate",
    );
    set
}

// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queued_tasks_start_when_slot_frees() {
        let mut m = CopyMgr::new();
        let mut ids = Vec::new();
        for i in 0..4u64 {
            ids.push(m.submit(OpKind::Copy, vec![(format!("f{i}"), 10)]));
        }
        // 前 3 Running，第 4 Queued。
        let states: Vec<TaskState> = ids.iter().map(|id| m.task(*id).unwrap().state).collect();
        assert_eq!(states[0], TaskState::Running);
        assert_eq!(states[3], TaskState::Queued);
        // 完成一个 → 队首接棒（pump 驱动）。
        m.cancel(ids[0]);
        assert_eq!(m.pump(), 1, "队列任务接棒");
        assert_eq!(m.task(ids[3]).unwrap().state, TaskState::Running);
    }

    #[test]
    fn eta_none_when_rate_zero() {
        let mut m = CopyMgr::new();
        let id = m.submit(OpKind::Copy, vec![(String::from("z"), 1_000)]);
        assert!(m.task(id).unwrap().eta_secs().is_none(), "无速度样本不编 ETA");
    }

    #[test]
    fn cancel_running_only() {
        let mut m = CopyMgr::new();
        for i in 0..4u64 {
            m.submit(OpKind::Copy, vec![(format!("f{i}"), 10)]);
        }
        assert!(m.cancel(4).is_none(), "排队中任务不可取消（尚未开始）");
    }

    #[test]
    fn cur_file_progress_tracks() {
        let mut m = CopyMgr::new();
        let id = m.submit(
            OpKind::Copy,
            vec![(String::from("a"), 1000), (String::from("b"), 2000)],
        );
        m.progress(id, 500, 0, 0);
        let t = m.task(id).unwrap();
        assert_eq!(t.cur_file, 0);
        assert_eq!(t.cur_permille(), 500);
        assert!(t.current_name() == Some("a"));
        m.progress(id, 500, 1, 10);
        let t = m.task(id).unwrap();
        assert_eq!(t.cur_file, 1, "文件切换");
        assert_eq!(t.current_name(), Some("b"));
    }

    #[test]
    fn copydlg_self_checks_all_green() {
        let set = run_copydlg_checks();
        let (p, f) = set.tally();
        assert!(set.all_passed(), "F086 自检红项：{}/{} 绿", p, p + f);
    }
}
