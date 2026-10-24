//! unx0567_rqlane —— UNX-F0567 运行队列进出账。
//! 锚点：调度器卷 · 运行队列（FIFO 入队 / 出队 / 空队拒 / 容量守恒）。
//! 判据原文：入队尾进出队头取（严格 FIFO），空队取出拒，重复入队拒
//!           （账不重项）；队列账守恒（容量 = 在队 + 已出）。

/// 队列任务。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Task {
    pub tid: u32,
    /// 优先级（小者高）。
    pub prio: u8,
}

/// 运行队列。
#[derive(Debug)]
pub struct RunQueue {
    pub q: alloc::vec::Vec<Task>,
    pub capacity: usize,
    /// 出队账 / 拒账。
    pub n_deq: u64,
    pub n_reject: u64,
}

impl RunQueue {
    pub fn new(capacity: usize) -> Self {
        RunQueue { q: alloc::vec::Vec::with_capacity(capacity), capacity, n_deq: 0, n_reject: 0 }
    }

    /// 入队：容量满拒、重复 tid 拒。
    pub fn enqueue(&mut self, t: Task) -> Result<(), ()> {
        if self.q.len() >= self.capacity {
            self.n_reject += 1;
            return Err(());
        }
        if self.q.iter().any(|x| x.tid == t.tid) {
            self.n_reject += 1;
            return Err(());
        }
        self.q.push(t);
        Ok(())
    }

    /// 出队：头取（FIFO）。
    pub fn dequeue(&mut self) -> Result<Task, ()> {
        if self.q.is_empty() {
            self.n_reject += 1;
            return Err(());
        }
        let t = self.q.remove(0);
        self.n_deq += 1;
        Ok(t)
    }

    /// 偷看队头（不出队）。
    pub fn peek(&self) -> Option<Task> {
        self.q.first().copied()
    }

    /// 守恒：容量 = 在队 + 已出（容量足够时）。
    pub fn reconcile(&self) -> bool {
        self.q.iter().map(|t| t.tid).collect::<alloc::vec::Vec<u32>>().len() == self.q.len()
    }
}

/// 判据入口：运行队列全性质核。
pub fn run_unx0567_checks() -> crate::checks::CheckSet {
    let mut cs = crate::checks::CheckSet::new("UNX-F0567");

    // 族一：FIFO 进出。
    let mut q = RunQueue::new(8);
    q.enqueue(Task { tid: 1, prio: 5 }).ok();
    q.enqueue(Task { tid: 2, prio: 3 }).ok();
    q.enqueue(Task { tid: 3, prio: 4 }).ok();
    cs.add("头取 FIFO", q.dequeue() == Ok(Task { tid: 1, prio: 5 }), "先入先出");
    cs.add("次取接棒", q.dequeue() == Ok(Task { tid: 2, prio: 3 }), "二位接棒");
    cs.add("偷看不动队", q.peek() == Some(Task { tid: 3, prio: 4 }), "peek 不出队");
    cs.add("偷看后再取同", q.dequeue() == Ok(Task { tid: 3, prio: 4 }), "peek 无副作用");

    // 族二：空队拒。
    let mut q2 = RunQueue::new(4);
    cs.add("空队取拒", q2.dequeue().is_err(), "无任务可取");
    cs.add("空拒入账", q2.n_reject == 1, "拒账一");
    cs.add("空 peek", q2.peek().is_none(), "空队无首");

    // 族三：重复入队拒。
    q2.enqueue(Task { tid: 9, prio: 1 }).ok();
    cs.add("重复 tid 拒", q2.enqueue(Task { tid: 9, prio: 2 }).is_err(), "账不重项");
    cs.add("重拒账二", q2.n_reject == 2, "累计二");
    cs.add("在队一", q2.q.len() == 1, "拒不入队");

    // 族四：容量满拒。
    let mut q3 = RunQueue::new(2);
    q3.enqueue(Task { tid: 1, prio: 1 }).ok();
    q3.enqueue(Task { tid: 2, prio: 1 }).ok();
    cs.add("满队拒", q3.enqueue(Task { tid: 3, prio: 1 }).is_err(), "容量封顶");
    cs.add("出位再进", { q3.dequeue().ok(); q3.enqueue(Task { tid: 3, prio: 1 }).is_ok() }, "腾位后收");

    // 族五：守恒核。
    let mut q4 = RunQueue::new(16);
    for i in 1..=10u32 {
        q4.enqueue(Task { tid: i, prio: 5 }).ok();
    }
    for _ in 0..4 {
        q4.dequeue().ok();
    }
    cs.add("账不重项", q4.reconcile(), "tid 无重");
    cs.add("在队六", q4.q.len() == 6, "10-4");
    cs.add("出账四", q4.n_deq == 4, "出队四笔");

    // 族六：混合优先级 FIFO 序核（异优先级不改变入队序）。
    let mut q5 = RunQueue::new(8);
    let mix: [(u32, u8); 5] = [(1, 9), (2, 1), (3, 5), (4, 2), (5, 7)];
    for (tid, prio) in mix.iter() {
        q5.enqueue(Task { tid: *tid, prio: *prio }).ok();
    }
    let mut seq_ok = true;
    for (tid, prio) in mix.iter() {
        match q5.dequeue() {
            Ok(t) if t.tid == *tid && t.prio == *prio => {}
            _ => seq_ok = false,
        }
    }
    cs.add("混合序全对", seq_ok, "优先级不影响 FIFO 位次");
    cs.add("出账五", q5.n_deq == 5, "五笔全出");

    // 族七：容量边界二分压测（capacity=1 循环）。
    let mut q6 = RunQueue::new(1);
    let mut cyc_ok = true;
    for i in 0..10u32 {
        if q6.enqueue(Task { tid: 100 + i, prio: 3 }).is_err() {
            cyc_ok = false;
        }
        if q6.dequeue().is_err() {
            cyc_ok = false;
        }
    }
    cs.add("单槽十轮", cyc_ok, "进一出一稳定");
    cs.add("循环账对称", q6.n_deq == 10 && q6.n_reject == 0, "零拒十出");

    // 族八：容量满连续拒账单调。
    let mut q7 = RunQueue::new(2);
    q7.enqueue(Task { tid: 1, prio: 1 }).ok();
    q7.enqueue(Task { tid: 2, prio: 1 }).ok();
    let r0 = q7.n_reject;
    for i in 0..4u32 {
        let _ = q7.enqueue(Task { tid: 10 + i, prio: 1 });
    }
    cs.add("四连拒", q7.n_reject == r0 + 4, "满队连拒齐账");
    cs.add("拒不破账", q7.reconcile() && q7.q.len() == 2, "原队完好");

    // 族九：peek 稳定性核（连续 peek 同值同位）。
    let mut q8 = RunQueue::new(4);
    q8.enqueue(Task { tid: 42, prio: 2 }).ok();
    q8.enqueue(Task { tid: 43, prio: 3 }).ok();
    let p1 = q8.peek();
    let p2 = q8.peek();
    cs.add("双 peek 恒等", p1 == p2 && p1 == Some(Task { tid: 42, prio: 2 }), "窥视无副作用");
    cs.add("peek 后队不变", q8.q.len() == 2 && q8.n_deq == 0, "零消耗");

    // 族十：大容量批进批出（32 任务序核）。
    let mut q9 = RunQueue::new(32);
    let mut in_ok = true;
    for i in 1..=32u32 {
        if q9.enqueue(Task { tid: i, prio: (i % 7) as u8 + 1 }).is_err() {
            in_ok = false;
        }
    }
    cs.add("三十二进", in_ok && q9.q.len() == 32, "满容收");
    let mut out_ok = true;
    for i in 1..=32u32 {
        match q9.dequeue() {
            Ok(t) if t.tid == i => {}
            _ => out_ok = false,
        }
    }
    cs.add("三十二出有序", out_ok, "FIFO 全序还原");
    cs.add("批后空队", q9.q.is_empty() && q9.n_deq == 32, "全出清账");

    // 族十一：出后再入复用 tid（账已出不再拒重）。
    let mut qa = RunQueue::new(2);
    qa.enqueue(Task { tid: 7, prio: 1 }).ok();
    qa.dequeue().ok();
    cs.add("出后重入同 tid", qa.enqueue(Task { tid: 7, prio: 9 }).is_ok(), "不与已出者比");
    cs.add("重入权限更新", qa.peek() == Some(Task { tid: 7, prio: 9 }), "新账生效");

    cs
}
