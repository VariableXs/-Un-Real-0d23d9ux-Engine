//! unx0571_waitq —— UNX-F0571 等待队列账。
//! 锚点：调度器卷 · 阻塞与唤醒（等事件挂起 / 唤醒入就绪 / 双等拒）。
//! 判据原文：阻塞者离就绪队进等待队（事件键分桶），唤醒须有等待者
//!           （空唤拒）；同任务双等拒（单挂）；唤醒入就绪队尾。

/// 等待队列管理器。
#[derive(Debug, Default)]
pub struct WaitQ {
    /// 就绪队。
    pub ready: alloc::vec::Vec<u32>,
    /// 等待桶（事件键 → 等待者）。
    pub buckets: alloc::vec::Vec<(u32, alloc::vec::Vec<u32>)>,
    /// 拒账（双等 / 空唤）。
    pub n_dup_wait: u64,
    pub n_empty_wake: u64,
    /// 唤醒账。
    pub n_wake: u64,
}

impl WaitQ {
    /// 阻塞：任务挂到事件桶（须在就绪队中，且未在等）。
    pub fn block(&mut self, tid: u32, ev: u32) -> Result<(), ()> {
        let ready_pos = self.ready.iter().position(|t| *t == tid).ok_or(())?;
        if self.buckets.iter().any(|b| b.1.contains(&tid)) {
            self.n_dup_wait += 1;
            return Err(()); // 双等拒
        }
        self.ready.remove(ready_pos);
        match self.buckets.iter_mut().find(|b| b.0 == ev) {
            Some(b) => b.1.push(tid),
            None => self.buckets.push((ev, alloc::vec![tid])),
        }
        Ok(())
    }

    /// 唤醒：事件桶头出，回就绪队尾。
    pub fn wake(&mut self, ev: u32) -> Result<u32, ()> {
        let b = self.buckets.iter_mut().find(|b| b.0 == ev).ok_or(())?;
        if b.1.is_empty() {
            self.n_empty_wake += 1;
            return Err(());
        }
        let tid = b.1.remove(0);
        self.ready.push(tid);
        self.n_wake += 1;
        Ok(tid)
    }

    /// 桶内等待者数。
    pub fn waiting_of(&self, ev: u32) -> usize {
        self.buckets.iter().find(|b| b.0 == ev).map(|b| b.1.len()).unwrap_or(0)
    }
}

/// 判据入口：等待队列全性质核。
pub fn run_unx0571_checks() -> crate::checks::CheckSet {
    let mut cs = crate::checks::CheckSet::new("UNX-F0571");

    // 族一：阻塞离队进桶。
    let mut w = WaitQ { ready: alloc::vec![1, 2, 3], buckets: alloc::vec::Vec::new(), n_dup_wait: 0, n_empty_wake: 0, n_wake: 0 };
    w.block(2, 0x10).ok();
    cs.add("阻塞离就绪", w.ready == alloc::vec![1u32, 3], "2 号出队");
    cs.add("进事件桶", w.waiting_of(0x10) == 1, "桶 0x10 一人");

    // 族二：空唤拒。
    let mut w2 = WaitQ::default();
    cs.add("空桶唤醒拒", w2.wake(0x20).is_err(), "无等待者");
    cs.add("空唤入账", w2.n_empty_wake == 1, "拒账一");

    // 族三：双等拒。
    let mut w3 = WaitQ { ready: alloc::vec![9], buckets: alloc::vec::Vec::new(), n_dup_wait: 0, n_empty_wake: 0, n_wake: 0 };
    w3.block(9, 0x30).ok();
    // 9 已不在就绪队 → 直接拒（未就绪）。
    cs.add("未就绪阻塞拒", w3.block(9, 0x31).is_err(), "不在就绪队");
    cs.add("桶单挂", w3.waiting_of(0x30) == 1 && w3.waiting_of(0x31) == 0, "只进首桶");

    // 族四：唤醒回就绪队尾。
    let mut w4 = WaitQ { ready: alloc::vec![1], buckets: alloc::vec::Vec::new(), n_dup_wait: 0, n_empty_wake: 0, n_wake: 0 };
    w4.block(1, 0x40).ok();
    w4.ready.push(7); // 就绪队有 7 号
    cs.add("唤醒回尾", w4.wake(0x40) == Ok(1), "桶头出");
    cs.add("就绪序尾接", w4.ready == alloc::vec![7u32, 1], "7 先 1 后");
    cs.add("桶清空", w4.waiting_of(0x40) == 0, "唤完桶空");

    // 族五：多等待者 FIFO 唤醒。
    let mut w5 = WaitQ { ready: alloc::vec![], buckets: alloc::vec::Vec::new(), n_dup_wait: 0, n_empty_wake: 0, n_wake: 0 };
    w5.ready = alloc::vec![1, 2, 3];
    w5.block(1, 0x50).ok();
    w5.block(2, 0x50).ok();
    w5.block(3, 0x50).ok();
    cs.add("桶三人", w5.waiting_of(0x50) == 3, "同事件三等");
    cs.add("FIFO 唤一", w5.wake(0x50) == Ok(1), "首等先醒");
    cs.add("FIFO 唤二", w5.wake(0x50) == Ok(2), "次等接棒");
    cs.add("桶剩一", w5.waiting_of(0x50) == 1, "3 号仍在等");

    // 族六：异事件桶分立。
    cs.add("异桶不串", w5.wake(0x51).is_err(), "0x51 无等者");
    cs.add("原桶不损", w5.waiting_of(0x50) == 1, "误唤不伤");
    cs.add("唤醒账二", w5.n_wake == 2, "两唤两账");

    // 族七：多桶并行等待与定向唤醒。
    let mut w6 = WaitQ::default();
    w6.ready = alloc::vec![1, 2, 3, 4];
    w6.block(1, 0xA0).ok();
    w6.block(2, 0xA0).ok();
    w6.block(3, 0xB0).ok();
    w6.block(4, 0xB0).ok();
    cs.add("双桶二二", w6.waiting_of(0xA0) == 2 && w6.waiting_of(0xB0) == 2, "分桶各二");
    cs.add("定向唤 A", w6.wake(0xA0) == Ok(1), "A 桶头出");
    cs.add("定向唤 B", w6.wake(0xB0) == Ok(3), "B 桶头出");
    cs.add("A 剩一 B 剩一", w6.waiting_of(0xA0) == 1 && w6.waiting_of(0xB0) == 1, "各余一");
    cs.add("唤醒账二", w6.n_wake == 2, "两唤两账");

    // 族八：唤醒后重阻塞循环（就绪→等→就绪→等）。
    let mut w7 = WaitQ::default();
    w7.ready = alloc::vec![5];
    let mut loop_ok = true;
    for round in 0..3u32 {
        if w7.block(5, 0xC0 + round).is_err() {
            loop_ok = false;
        }
        if w7.wake(0xC0 + round) != Ok(5) {
            loop_ok = false;
        }
    }
    cs.add("三循环挂醒", loop_ok, "每轮换事件键");
    cs.add("循环后净", w7.ready == alloc::vec![5u32] && w7.n_wake == 3, "账归位");

    // 族九：唤醒序与就绪队尾汇流核。
    let mut w8 = WaitQ::default();
    w8.ready = alloc::vec![9];
    w8.block(9, 0xD0).ok();
    w8.ready.push(1); // 就绪队已有 1
    w8.ready.push(2);
    w8.wake(0xD0).ok();
    cs.add("汇流序", w8.ready == alloc::vec![1u32, 2, 9], "唤醒者排尾");

    // 族十：桶内全唤重建序核（依次唤醒顺序与入等序一致）。
    let mut w9 = WaitQ::default();
    w9.ready = alloc::vec![10, 11, 12, 13];
    for tid in [10u32, 11, 12, 13] {
        w9.block(tid, 0xE0).ok();
    }
    let mut wake_seq = alloc::vec::Vec::new();
    while w9.waiting_of(0xE0) > 0 {
        if let Ok(tid) = w9.wake(0xE0) {
            wake_seq.push(tid);
        } else {
            break;
        }
    }
    cs.add("四唤全序", wake_seq == alloc::vec![10u32, 11, 12, 13], "入等序=唤醒序");
    cs.add("全唤归就绪", w9.ready == alloc::vec![10u32, 11, 12, 13], "全员回队");
    cs.add("唤醒账四", w9.n_wake == 4, "四唤四账");

    // 族十一：桶残尾拒再唤空（唤尽后第五次拒）。
    let mut w10 = WaitQ::default();
    w10.ready = alloc::vec![20, 21];
    w10.block(20, 0xF0).ok();
    w10.block(21, 0xF0).ok();
    w10.wake(0xF0).ok();
    w10.wake(0xF0).ok();
    cs.add("唤尽再唤拒", w10.wake(0xF0).is_err(), "桶空");
    cs.add("空唤入账", w10.n_empty_wake == 1, "拒账一");

    // 族十二：跨桶唤醒互不消费。
    let mut w11 = WaitQ::default();
    w11.ready = alloc::vec![30, 31];
    w11.block(30, 0x11).ok();
    w11.block(31, 0x22).ok();
    w11.wake(0x11).ok();
    cs.add("A 唤 B 不动", w11.waiting_of(0x22) == 1, "31 仍等 0x22");
    cs.add("B 独唤", w11.wake(0x22) == Ok(31), "31 由己桶醒");
    cs.add("双桶清", w11.waiting_of(0x11) == 0 && w11.waiting_of(0x22) == 0, "全醒净");

    cs
}
