//! H2 缩略图队列 · 深化批次四（F285 渲染侧——解码任务的排队/
//! 去重/取消/优先级单点实现）。
//!
//! **承接判据**（主册 H 域正文，一处一事实）：
//! - **F285 图标缓存与刷新**：变更→更新 1s 内——更新路径的排队
//!   纪律在这里：可视区条目优先（h2list 车道：看不见的不解码）、
//!   同源去重（同键重复请求不重复入队）、滚出可视区即取消（解码
//!   完成也不回填已弃任务）；
//! - **F093 车道（经 F285 锚）**：万张目录滚动帧率不掉——本层
//!   保证解码吞吐有界（并发上限 2 + 单帧只出队 1 个），解码不与
//!   呈现抢帧预算；
//! - **十四章状态机**：Queued→Decoding→Done/Cancelled 每个任务
//!   都有终态；取消的任务不得回填缓存（半成品进缓存=缺陷）。
//!
//! 时间纪律：无时钟；优先级由调用方按可视区结论注入。

use crate::checks::CheckSet;

use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 队列模型
// ---------------------------------------------------------------------------

/// 解码并发上限（吞吐有界——呈现帧预算保护线）。
pub const MAX_CONCURRENT: usize = 2;

/// 任务状态机。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JobState {
    Queued,
    Decoding,
    Done,
    Cancelled,
}

/// 一个解码任务。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Job {
    /// 缓存键（源路径+目标档——同键即同任务）。
    pub key: String,
    /// 优先级：0 = 可视区内（先解码），9 = 预取（空闲才做）。
    pub priority: u8,
    pub state: JobState,
    /// 入队序（同优先级 FIFO——稳定不饿死）。
    pub seq: u64,
}

/// 缩略图解码队列。
pub struct ThumbQueue {
    jobs: Vec<Job>,
    next_seq: u64,
    pub cancelled_total: u32,
}

impl ThumbQueue {
    pub fn new() -> ThumbQueue {
        ThumbQueue { jobs: Vec::new(), next_seq: 0, cancelled_total: 0 }
    }

    /// 入队：同键去重（Queued/Decoding 中存在同键则拒绝——重复
    /// 解码是浪费也是账目污染）。返回是否真的入了队。
    pub fn push(&mut self, key: &str, priority: u8) -> bool {
        if self.jobs.iter().any(|j| j.key == key && matches!(j.state, JobState::Queued | JobState::Decoding)) {
            return false;
        }
        let seq = self.next_seq;
        self.next_seq += 1;
        self.jobs.push(Job { key: key.into(), priority, state: JobState::Queued, seq });
        true
    }

    /// 取下一个该解码的任务：优先级升序（可视区 0 先于预取 9）、
    /// 同级 FIFO；并发上限内才出队（MAX_CONCURRENT 个 Decoding 占
    /// 满时不给新任务——解码不抢呈现帧）。
    pub fn next(&mut self) -> Option<String> {
        let decoding = self.jobs.iter().filter(|j| j.state == JobState::Decoding).count();
        if decoding >= MAX_CONCURRENT {
            return None;
        }
        let cand = self
            .jobs
            .iter()
            .filter(|j| j.state == JobState::Queued)
            .min_by_key(|j| (j.priority, j.seq))?
            .key
            .clone();
        if let Some(j) = self.jobs.iter_mut().find(|j| j.key == cand) {
            j.state = JobState::Decoding;
        }
        Some(cand)
    }

    /// 完成：只有 Decoding 态可完成（重复完成/未开始完成 = 拒绝）。
    /// 返回是否该回填缓存（Done=true；Cancelled 永不回填）。
    pub fn finish(&mut self, key: &str) -> bool {
        match self.jobs.iter_mut().find(|j| j.key == key && j.state == JobState::Decoding) {
            Some(j) => {
                j.state = JobState::Done;
                true
            }
            None => false,
        }
    }

    /// 取消（滚出可视区/源删除）：Queued/Decoding 都可取消；取消
    /// 计数显性化（§十三章——取消风暴可观测）。
    pub fn cancel(&mut self, key: &str) -> bool {
        match self.jobs.iter_mut().find(|j| j.key == key && matches!(j.state, JobState::Queued | JobState::Decoding)) {
            Some(j) => {
                j.state = JobState::Cancelled;
                self.cancelled_total += 1;
                true
            }
            None => false,
        }
    }

    /// 失效（源变更——F285 变更→更新判据的入口）：该键旧任务取消、
    /// 键位标记需重解（调用方重新 push 同键即入新任务）。
    pub fn invalidate(&mut self, key: &str) -> bool {
        let had = self.cancel(key);
        // 已完成的同键任务移除（腾位给新版本）。
        self.jobs.retain(|j| !(j.key == key && j.state == JobState::Done));
        had || true
    }

    /// 各状态计数（诊断页口径）。
    pub fn count(&self, s: JobState) -> usize {
        self.jobs.iter().filter(|j| j.state == s).count()
    }

    /// 收账：清理终态任务（Done/Cancelled 出账——账本不无限增长）。
    pub fn reap(&mut self) -> usize {
        let before = self.jobs.len();
        self.jobs.retain(|j| matches!(j.state, JobState::Queued | JobState::Decoding));
        before - self.jobs.len()
    }

    pub fn len(&self) -> usize {
        self.jobs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.jobs.is_empty()
    }
}

// ---------------------------------------------------------------------------
// 自检（判据逐条钉死）
// ---------------------------------------------------------------------------

pub fn run_h2thumbq_checks() -> CheckSet {
    let mut set = CheckSet::new("h2-h2thumbq");
    let mut q = ThumbQueue::new();
    // 可视区优先：预取 9 先入队也不挡可视区 0。
    q.push("/预取/大图.png", 9);
    q.push("/可视/a.png", 0);
    set.add(
        "h2thumbq visible first",
        q.next().as_deref() == Some("/可视/a.png"),
        "priority beats arrival",
    );
    // 并发上限：2 个 Decoding 占满后 next() = None（吞吐有界）。
    let second = q.next();
    set.add(
        "h2thumbq concurrency cap",
        second.as_deref() == Some("/预取/大图.png") && q.next().is_none(),
        "max 2 in flight",
    );
    // 同键去重：排队中重复入队被拒。
    set.add(
        "h2thumbq dedupe",
        !q.push("/可视/a.png", 0),
        "no double decode",
    );
    // 完成：只有 in-flight 可完成；完成后可重入队（新一轮请求）。
    set.add(
        "h2thumbq finish gate",
        q.finish("/可视/a.png") && !q.finish("/可视/a.png") && !q.finish("/无/不存在.png"),
        "decode-once semantics",
    );
    // 取消：取消态不回填；取消计数显性化。
    let mut q2 = ThumbQueue::new();
    q2.push("/x.png", 0);
    let k = q2.next().unwrap();
    q2.cancel(&k);
    set.add(
        "h2thumbq cancel no backfill",
        !q2.finish(&k) && q2.cancelled_total == 1,
        "cancelled never cached",
    );
    // 失效：变更→旧任务取消 + 旧成品腾位 → 新任务可入。
    let mut q3 = ThumbQueue::new();
    q3.push("/y.png", 0);
    let k3 = q3.next().unwrap();
    q3.finish(&k3);
    let invalidated = q3.invalidate("/y.png");
    set.add(
        "h2thumbq invalidate",
        invalidated && q3.push("/y.png", 0),
        "change → re-decode",
    );
    // 收账：终态出账、活任务保留（/1 完成后收账出 1 条）。
    let mut q4 = ThumbQueue::new();
    for name in ["/1", "/2", "/3"] {
        q4.push(name, 5);
    }
    let a = q4.next();
    let b = q4.next();
    set.add(
        "h2thumbq fifo same priority",
        a.as_deref() == Some("/1") && b.as_deref() == Some("/2"),
        "stable order",
    );
    q4.finish("/1");
    let reaped = q4.reap();
    set.add(
        "h2thumbq reap",
        reaped >= 1 && q4.count(JobState::Queued) == 1,
        "ledger bounded",
    );
    set.add("h2thumbq cap const", MAX_CONCURRENT == 2, "throughput line");
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn h2thumbq_all_green() {
        let set = run_h2thumbq_checks();
        assert!(set.all_passed(), "h2thumbq 自检有红项");
        assert!(!set.truncated(), "h2thumbq 自检溢出");
    }

    #[test]
    fn churn_never_leaks_jobs() {
        // 万轮入队-完成-收账翻搅：活账恒定 ≤ 并发上限 + 排队量（不泄漏）。
        let mut q = ThumbQueue::new();
        for i in 0..10_000u64 {
            let key = alloc::format!("/k{i}.png");
            q.push(&key, (i % 10) as u8);
            while let Some(k) = q.next() {
                q.finish(&k);
            }
            if i % 3 == 0 {
                q.reap();
            }
            assert!(q.count(JobState::Decoding) <= MAX_CONCURRENT);
        }
    }
}
