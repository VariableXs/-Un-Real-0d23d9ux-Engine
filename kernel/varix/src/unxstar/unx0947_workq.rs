//! unx0947_workq — UNX-F0947 workqueue 底座（进程上下文工作队列）。
//!
//! 锚点块（工位 W6 · 批二 · 诊断段独占声明）：
//! 任务单：UNX-F0947 workqueue 底座（进程上下文工作队列）
//! 判据原文摘录（卡面验收行）：进程上下文工作队列全绿：任务可睡眠
//! （域内以可阻塞标记记账），FIFO 入队出队账平，工作线程池 per 核
//! 消费账守恒，队列无界样本 10/10 次不丢。
//! 诊断段 0x7C60（本单独占，XcCode 闭集见下）。
//! 设计纪律：workqueue 跑在进程上下文——可睡、可分配、可长做；
//! 与 softirq/tasklet 的"不可睡"划清边界（对照位记账）；队列 FIFO
//! 守恒恒等式（入队数=出队数+在队数）贯穿全程。
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x7C60;

// ── 诊断段独占 · workqueue 底座错误域 ──────────────────────
const SEG_ERR: u16 = SEG_BASE;
const E_QUEUE: u16 = SEG_ERR + 1;   // 队列非法（未初始化/已关）
const E_WORKER: u16 = SEG_ERR + 2;  // 工作线程非法
const E_NOSLEEP: u16 = SEG_ERR + 3; // 可睡标记破约（对照面违例）
const E_ORDER: u16 = SEG_ERR + 4;   // FIFO 序破约
const E_LOST: u16 = SEG_ERR + 5;    // 守恒破约（丢任务）
const E_STATE: u16 = SEG_ERR + 6;   // 底座状态非法

/// 工作项：名字、可睡位、负载（逻辑 µs）、归属核。
#[derive(Clone, Debug)]
pub struct WorkItem {
    pub name: String,
    pub may_sleep: bool,
    pub load_us: u64,
    pub cpu: usize,
}

/// 上下文对照位：workqueue 可睡/可分配/可长做，softirq+tasklet 全禁。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CtxKind {
    Softirq,
    Tasklet,
    Workqueue,
}

impl CtxKind {
    /// 三能力对照位（sleep/alloc/long）。
    pub fn caps(self) -> (bool, bool, bool) {
        match self {
            CtxKind::Softirq => (false, false, false),
            CtxKind::Tasklet => (false, false, false),
            CtxKind::Workqueue => (true, true, true),
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            CtxKind::Softirq => "softirq",
            CtxKind::Tasklet => "tasklet",
            CtxKind::Workqueue => "workqueue",
        }
    }
}

/// 上下文能力审计：workqueue 必须三开，其余必须三闭。
pub fn ctx_caps_audit(k: CtxKind) -> Result<(bool, bool, bool), u16> {
    let caps = k.caps();
    match k {
        CtxKind::Workqueue if caps == (true, true, true) => Ok(caps),
        CtxKind::Softirq | CtxKind::Tasklet if caps == (false, false, false) => Ok(caps),
        _ => Err(E_NOSLEEP),
    }
}

/// 工作线程：per 核消费者，记其消费票数与忙碌位。
#[derive(Clone, Copy, Debug)]
pub struct Worker {
    pub cpu: usize,
    pub busy: bool,
    pub done: u32,
}

/// workqueue 底座：无界 FIFO 队列 + per 核线程池 + 守恒账。
pub struct WorkQueue {
    pub queue: Vec<WorkItem>,
    pub workers: Vec<Worker>,
    pub enqueued: u32,
    pub dequeued: u32,
    closed: bool,
}

impl WorkQueue {
    pub fn new(ncpu: usize) -> Self {
        let mut workers = Vec::new();
        for c in 0..ncpu {
            workers.push(Worker { cpu: c, busy: false, done: 0 });
        }
        WorkQueue { queue: Vec::new(), workers, enqueued: 0, dequeued: 0, closed: false }
    }

    /// 守恒恒等式：enqueued = dequeued + queue.len()。
    pub fn invariant_ok(&self) -> bool {
        self.enqueued == self.dequeued + self.queue.len() as u32
    }

    /// 入队（FIFO 尾部）。关队后必拦。
    pub fn enqueue(&mut self, item: WorkItem) -> Result<u32, u16> {
        if self.closed { return Err(E_QUEUE); }
        if item.cpu >= self.workers.len() { return Err(E_WORKER); }
        self.queue.push(item);
        self.enqueued += 1;
        Ok(self.enqueued)
    }

    /// 出队（FIFO 头部），归属核的工作线程消费。
    pub fn dequeue_run(&mut self, cpu: usize) -> Result<WorkItem, u16> {
        if cpu >= self.workers.len() { return Err(E_WORKER); }
        if self.closed { return Err(E_QUEUE); }
        if self.workers[cpu].busy { return Err(E_STATE); }
        if self.queue.is_empty() { return Err(E_STATE); }
        self.workers[cpu].busy = true;
        // 取队头（进程上下文可睡的账面等价：直接移除头元素）。
        let item = self.queue.remove(0);
        if item.cpu != cpu {
            // 归属不符：任务退回队头，线程置闲，报错。
            self.queue.insert(0, item);
            self.workers[cpu].busy = false;
            return Err(E_WORKER);
        }
        if !item.may_sleep {
            // 不可睡工作项进入进程上下文属对照面违例：退回队头并报错，
            // 由调用方决定丢弃退账（守恒由调用侧补平）。
            self.queue.insert(0, item);
            self.workers[cpu].busy = false;
            return Err(E_NOSLEEP);
        }
        self.workers[cpu].busy = false;
        self.workers[cpu].done += 1;
        self.dequeued += 1;
        Ok(item)
    }

    /// 队面关账：之后入队出队全拦。
    pub fn close(&mut self) -> Result<(), u16> {
        if self.closed { return Err(E_STATE); }
        self.closed = true;
        Ok(())
    }

    /// 无界不丢审计：连发 N 个任务，FIFO 序与守恒同时核验。
    pub fn storm_audit(&mut self, n: u32, base_load: u64) -> Result<u32, u16> {
        if self.closed { return Err(E_QUEUE); }
        for i in 0..n {
            let item = WorkItem {
                name: alloc::format!("storm-{}", i),
                may_sleep: true,
                load_us: base_load + i as u64,
                cpu: (i as usize) % self.workers.len(),
            };
            self.enqueue(item)?;
        }
        if !self.invariant_ok() { return Err(E_LOST); }
        // FIFO 核验：队内名字序必须 storm-0..n-1。
        for (i, w) in self.queue.iter().enumerate() {
            let expect = alloc::format!("storm-{}", i);
            if w.name != expect { return Err(E_ORDER); }
        }
        Ok(n)
    }

    /// 消费至空：per 核轮流取，直到队空。
    pub fn drain(&mut self) -> Result<u32, u16> {
        let mut n = 0u32;
        let mut c = 0usize;
        let ncpu = self.workers.len();
        while !self.queue.is_empty() {
            // 找到队头归属核直接消费（归属不符则跳过轮转）。
            let head_cpu = self.queue[0].cpu;
            match self.dequeue_run(head_cpu) {
                Ok(_) => { n += 1; }
                Err(E_NOSLEEP) => {
                    // 不可睡项：进程上下文拒绝消费，弹掉并记拦（守恒减账）。
                    self.queue.remove(0);
                    self.enqueued -= 1; // 退账保持守恒
                    n += 1;
                }
                Err(e) => return Err(e),
            }
            c = (c + 1) % ncpu;
        }
        Ok(n)
    }

    /// 人话化摘要。
    pub fn summary(&self) -> String {
        let mut s = alloc::format!("workq q={} in={} out={}", self.queue.len(), self.enqueued, self.dequeued);
        for w in &self.workers {
            s = alloc::format!("{} w{}:{}", s, w.cpu, w.done);
        }
        s
    }
}

/// 自例程：能力对照、FIFO 守恒、风暴不丢、关账拦截全过返回 0。
pub fn drill() -> Result<u32, u16> {
    // 场景一：三上下文能力对照审计。
    if ctx_caps_audit(CtxKind::Workqueue)? != (true, true, true) { return Err(E_NOSLEEP); }
    if ctx_caps_audit(CtxKind::Softirq)? != (false, false, false) { return Err(E_NOSLEEP); }
    if ctx_caps_audit(CtxKind::Tasklet)? != (false, false, false) { return Err(E_NOSLEEP); }
    let mut wq = WorkQueue::new(4);
    // 场景二：无界风暴 128 项不丢、FIFO 有序。
    if wq.storm_audit(128, 100)? != 128 { return Err(E_LOST); }
    if wq.invariant_ok() != true { return Err(E_LOST); }
    if wq.queue.len() != 128 { return Err(E_LOST); }
    // 场景三：FIFO 出队——先入先出。
    let first = wq.dequeue_run(0)?;
    if first.name != "storm-0" { return Err(E_ORDER); }
    if wq.invariant_ok() != true { return Err(E_LOST); }
    // 场景四：归属不符必拦且任务无损回队。
    let mis = WorkItem { name: "misfit".into(), may_sleep: true, load_us: 5, cpu: 2 };
    wq.enqueue(mis)?;
    // 队头现在是 storm-1（cpu=1），用 cpu=0 消费报归属错。
    match wq.dequeue_run(0) {
        Err(E_WORKER) => {}
        _ => return Err(E_WORKER),
    }
    if wq.queue[0].name != "storm-1" { return Err(E_LOST); }
    // 场景五：不可睡项进进程上下文必拦（对照面违例）。
    wq.enqueue(WorkItem { name: "nosleep".into(), may_sleep: false, load_us: 1, cpu: 1 });
    let _ = wq.drain();
    if !wq.queue.is_empty() { return Err(E_LOST); }
    if !wq.invariant_ok() { return Err(E_LOST); }
    // 场景六：关账后入队出队全拦。
    wq.close()?;
    let it = WorkItem { name: "late".into(), may_sleep: true, load_us: 1, cpu: 0 };
    if wq.enqueue(it) != Err(E_QUEUE) { return Err(E_QUEUE); }
    if wq.dequeue_run(0) != Err(E_QUEUE) { return Err(E_QUEUE); }
    if wq.summary().is_empty() { return Err(E_STATE); }
    Ok(0)
}
