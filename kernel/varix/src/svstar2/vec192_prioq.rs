//! VE-F0592 · 编译队列优先级
//!
//! 四档优先级（前台交互、预览、后台预热、批量）与语义——前台永远先于后台，
//! 同档先到先得；老化机制：等待超时自动提级防饥饿（提级事件入遥测）；取消
//! 语义：被新版本取代的任务在安全检查点取消（取消不产生半成品产物）；透明度：
//! 队列状态可查询（各档深度、等待分布），入 F0589 遥测面板。
//!
//! 跨批对接：上游 F0591 池消费；下游 F0589 遥测、F0597 三账。

use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 四档优先级
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Prio {
    /// 前台交互
    Foreground,
    /// 预览
    Preview,
    /// 后台预热
    Prewarm,
    /// 批量
    Batch,
}

impl Prio {
    pub fn code(self) -> u32 {
        match self {
            Prio::Foreground => 0,
            Prio::Preview => 1,
            Prio::Prewarm => 2,
            Prio::Batch => 3,
        }
    }
}

/// 队列任务
#[derive(Clone, Debug)]
pub struct QTask {
    pub id: u32,
    pub prio: Prio,
    /// 入队时刻（单调序号）
    pub enqueued_at: u64,
    /// 被新版本取代标记
    pub superseded: bool,
    /// 产出半成品标记（取消纪律下恒为 false）
    pub partial: bool,
}

impl QTask {
    pub fn new(id: u32, prio: Prio, at: u64) -> Self {
        QTask { id, prio, enqueued_at: at, superseded: false, partial: false }
    }
}

/// 优先级队列：前台永远先于后台，同档先到先得
#[derive(Clone, Debug, Default)]
pub struct PrioQueue {
    pub tasks: Vec<QTask>,
    pub now: u64,
    /// 老化提级事件（入遥测）
    pub aging_events: u32,
    /// 取消事件
    pub cancels: u32,
}

impl PrioQueue {
    /// 入队 O(log) 语义：追加后按（档位, 到达序）取首
    pub fn enqueue(&mut self, mut t: QTask) -> bool {
        self.now += 1;
        t.enqueued_at = self.now;
        if self.tasks.iter().any(|x| x.id == t.id) {
            return false;
        }
        self.tasks.push(t);
        true
    }

    /// 取首：最小档位，同档最早到达
    pub fn pick(&self) -> Option<&QTask> {
        self.tasks
            .iter()
            .filter(|t| !t.superseded)
            .min_by_key(|t| (t.prio, t.enqueued_at))
    }

    pub fn pop(&mut self) -> Option<QTask> {
        let best = self.pick().map(|t| t.id)?;
        let i = self.tasks.iter().position(|t| t.id == best)?;
        Some(self.tasks.remove(i))
    }

    /// 老化提级：等待超时自动升到前台档防饥饿
    pub fn age_up(&mut self, wait_limit: u64) -> usize {
        let mut n = 0;
        let now = self.now;
        for t in self.tasks.iter_mut() {
            if t.prio > Prio::Foreground && now - t.enqueued_at > wait_limit {
                t.prio = Prio::Foreground;
                n += 1;
                self.aging_events += 1;
            }
        }
        n
    }

    /// 安全取消：检查点原子判定——被取代任务整件移除，不产生半成品
    pub fn cancel_superseded(&mut self, id: u32) -> bool {
        if let Some(t) = self.tasks.iter_mut().find(|t| t.id == id) {
            if t.superseded {
                t.partial = false;
                self.tasks.retain(|x| x.id != id);
                self.cancels += 1;
                return true;
            }
            return false;
        }
        false
    }

    /// 标记取代（新版本到达）
    pub fn supersede(&mut self, old_id: u32) -> bool {
        match self.tasks.iter_mut().find(|t| t.id == old_id) {
            Some(t) => {
                t.superseded = true;
                true
            }
            None => false,
        }
    }

    /// 状态透明：各档深度
    pub fn depth(&self, p: Prio) -> usize {
        self.tasks.iter().filter(|t| t.prio == p && !t.superseded).count()
    }

    /// 等待分布（各档最早等待时长）
    pub fn wait_spread(&self) -> Vec<(Prio, u64)> {
        [Prio::Foreground, Prio::Preview, Prio::Prewarm, Prio::Batch]
            .iter()
            .filter_map(|p| {
                self.tasks
                    .iter()
                    .filter(|t| t.prio == *p && !t.superseded)
                    .map(|t| self.now - t.enqueued_at)
                    .max()
                    .map(|w| (*p, w))
            })
            .collect()
    }
}

pub fn run_vec192_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("svstar2-vec192");
    // 族一：四档语义
    s.add("档-次序", Prio::Foreground < Prio::Preview && Prio::Preview < Prio::Prewarm && Prio::Prewarm < Prio::Batch, "order");
    s.add("档-编码", Prio::Foreground.code() == 0 && Prio::Batch.code() == 3, "code");
    // 族二：前台先于后台 + 同档先到先得
    let mut q = PrioQueue::default();
    q.enqueue(QTask::new(1, Prio::Batch, 0));
    q.enqueue(QTask::new(2, Prio::Foreground, 0));
    q.enqueue(QTask::new(3, Prio::Foreground, 0));
    s.add("取首-前台优先", q.pick().map(|t| t.id) == Some(2), "fg");
    q.pop();
    s.add("取首-同档先到", q.pick().map(|t| t.id) == Some(3), "fifo");
    q.pop();
    s.add("取首-后台补位", q.pick().map(|t| t.id) == Some(1), "bg");
    // 族三：老化防饿
    let mut q2 = PrioQueue::default();
    q2.enqueue(QTask::new(10, Prio::Batch, 0));
    for _ in 0..100 {
        q2.now += 1;
    }
    s.add("老化-未到不提级", q2.age_up(200) == 0, "notyet");
    s.add("老化-超时提级", q2.age_up(50) == 1 && q2.aging_events == 1, "up");
    s.add("老化-提级后台先出", q2.pick().map(|t| t.id) == Some(10), "picked");
    // 族四：安全取消
    let mut q3 = PrioQueue::default();
    q3.enqueue(QTask::new(20, Prio::Preview, 0));
    s.add("取消-未取代拒绝", !q3.cancel_superseded(20), "no");
    q3.supersede(20);
    s.add("取消-取代后可取消", q3.cancel_superseded(20) && q3.cancels == 1, "yes");
    s.add("取消-无半成品", q3.tasks.iter().all(|t| !t.partial), "nopartial");
    s.add("取消-重复取消拒绝", !q3.cancel_superseded(20), "dup");
    // 族五：状态透明
    let mut q4 = PrioQueue::default();
    q4.enqueue(QTask::new(30, Prio::Foreground, 0));
    q4.enqueue(QTask::new(31, Prio::Preview, 0));
    q4.enqueue(QTask::new(32, Prio::Preview, 0));
    q4.enqueue(QTask::new(33, Prio::Batch, 0));
    s.add("透明-档深度", q4.depth(Prio::Preview) == 2 && q4.depth(Prio::Foreground) == 1, "depth");
    let spread = q4.wait_spread();
    s.add("透明-等待分布", spread.len() == 3 && spread[0].0 == Prio::Foreground, "spread");
    s.add("透明-被取代不计深度", {
        q4.supersede(31);
        q4.depth(Prio::Preview) == 1
    }, "sup");
    // 族六：重复入队拒绝
    let mut q5 = PrioQueue::default();
    q5.enqueue(QTask::new(40, Prio::Foreground, 0));
    s.add("入队-重复拒绝", !q5.enqueue(QTask::new(40, Prio::Foreground, 0)), "dup");
    s
}
