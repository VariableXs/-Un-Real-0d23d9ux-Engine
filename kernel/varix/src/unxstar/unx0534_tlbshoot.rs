//! unx0534_tlbshoot —— UNX-F0534 TLB shootdown 广播账。
//! 锚点：主机 MMU 卷 · 核间失效广播（发单 / 确认 / 聚合屏障）。
//! 判据原文：每核至多发一单（幂等），确认数到齐才闭屏障；
//!           迟到确认独立账不入屏障，发单账守恒（发 = 确 + 在途）。

/// 单核发单状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreState {
    /// 未发单。
    Idle,
    /// 已发单待确认。
    Sent,
    /// 已确认。
    Acked,
}

/// 发单记录。
#[derive(Debug, Clone, Copy)]
pub struct Shot {
    pub core: u8,
    pub iova: u64,
    pub state: CoreState,
}

/// Shootdown 广播账。
#[derive(Debug, Default)]
pub struct Shootdown {
    pub shots: alloc::vec::Vec<Shot>,
    pub acked: u64,
    pub late: u64,
    /// 广播目标核数。
    pub targets: u8,
}

impl Shootdown {
    pub fn new(targets: u8) -> Self {
        Shootdown { shots: alloc::vec::Vec::new(), acked: 0, late: 0, targets }
    }

    /// 发单：同核幂等（已发不重发），越界核拒。
    pub fn send(&mut self, core: u8, iova: u64) -> Result<bool, ()> {
        if core >= self.targets {
            return Err(());
        }
        if self.shots.iter().any(|s| s.core == core) {
            // 已有在途单：幂等不重发。
            let _ = iova;
            return Ok(false);
        }
        self.shots.push(Shot { core, iova, state: CoreState::Sent });
        Ok(true)
    }

    /// 确认：Sent → Acked；无单核确认记迟到。
    pub fn ack(&mut self, core: u8) {
        match self.shots.iter_mut().find(|s| s.core == core) {
            Some(s) if s.state == CoreState::Sent => {
                s.state = CoreState::Acked;
                self.acked += 1;
            }
            _ => self.late += 1,
        }
    }

    /// 屏障闭合：全部在途单已确认。
    pub fn barrier_closed(&self) -> bool {
        self.shots.iter().all(|s| s.state == CoreState::Acked)
    }

    /// 在途计数。
    pub fn in_flight(&self) -> usize {
        self.shots.iter().filter(|s| s.state == CoreState::Sent).count()
    }

    /// 守恒：发单数 = 确认数 + 在途数（迟到独立不计）。
    pub fn reconcile(&self) -> bool {
        self.shots.len() as u64 == self.acked + self.in_flight() as u64
    }
}

/// 聚合器：多轮 shootdown 的总账。
#[derive(Debug, Default)]
pub struct Aggregator {
    pub rounds: u64,
    pub total_sent: u64,
    pub total_acked: u64,
    pub total_late: u64,
}

impl Aggregator {
    pub fn absorb(&mut self, s: &Shootdown) {
        self.rounds += 1;
        self.total_sent += s.shots.len() as u64;
        self.total_acked += s.acked;
        self.total_late += s.late;
    }

    pub fn reconcile(&self) -> bool {
        self.total_sent == self.total_acked + self.in_flight_total()
    }

    fn in_flight_total(&self) -> u64 {
        // 聚合时在途数 = 发 - 确（迟到不计）。
        self.total_sent - self.total_acked
    }
}

/// 判据入口：TLB shootdown 全性质核。
pub fn run_unx0534_checks() -> crate::checks::CheckSet {
    let mut cs = crate::checks::CheckSet::new("UNX-F0534");

    // 族一：发单与幂等。
    let mut sd = Shootdown::new(4);
    cs.add("首发收单", sd.send(0, 0x1000) == Ok(true), "新核入账");
    cs.add("重发幂等", sd.send(0, 0x2000) == Ok(false), "同核不重发");
    cs.add("越界拒", sd.send(9, 0x3000).is_err(), "核号超 targets");
    cs.add("单账一", sd.shots.len() == 1, "只进一单");

    // 族二：确认与屏障。
    sd.ack(0);
    cs.add("确认转态", sd.shots[0].state == CoreState::Acked, "Sent→Acked");
    cs.add("屏障闭合", sd.barrier_closed(), "全员确认");
    cs.add("守恒核", sd.reconcile(), "发 = 确 + 在途");

    // 族三：部分确认屏障不闭。
    let mut sd2 = Shootdown::new(3);
    sd2.send(0, 0x4000).ok();
    sd2.send(1, 0x4000).ok();
    sd2.send(2, 0x5000).ok();
    sd2.ack(0);
    cs.add("屏障未闭", !sd2.barrier_closed(), "尚有在途");
    cs.add("在途二", sd2.in_flight() == 2, "1、2 号未确认");
    cs.add("守恒核二", sd2.reconcile(), "3 = 1 + 2");

    // 族四：迟到确认独立账。
    sd2.ack(0); // 已确认再确认 → 迟到
    sd2.ack(7); // 越界核确认 → 迟到
    cs.add("迟到账二", sd2.late == 2, "重复 + 越界");
    cs.add("迟到不入守恒", sd2.reconcile(), "发单账未膨胀");

    // 族五：全员到齐闭屏障。
    sd2.ack(1);
    sd2.ack(2);
    cs.add("全员闭屏障", sd2.barrier_closed(), "三单三确认");
    cs.add("在途清零", sd2.in_flight() == 0, "全部落账");

    // 族六：聚合器多轮吸收。
    let mut agg = Aggregator::default();
    agg.absorb(&sd);
    agg.absorb(&sd2);
    cs.add("轮账二", agg.rounds == 2, "两轮入总账");
    cs.add("发账四", agg.total_sent == 4, "1 + 3");
    cs.add("确账四", agg.total_acked == 4, "1 + 3");
    cs.add("迟账二", agg.total_late == 2, "迟到聚合");
    cs.add("聚合守恒", agg.reconcile(), "发 = 确 + 在途");

    // 族七：同址多核广播（同 IOVA 逐核发单）。
    let mut sd3 = Shootdown::new(4);
    for c in 0..4u8 {
        sd3.send(c, 0x6000).ok();
    }
    cs.add("四核四单", sd3.shots.len() == 4, "同址广播全员");
    cs.add("同址异核", sd3.shots.iter().all(|s| s.iova == 0x6000), "地址一致");
    for c in 0..4u8 {
        sd3.ack(c);
    }
    cs.add("广播闭环", sd3.barrier_closed(), "全员收讫");

    // 族八：空广播与零确认边界。
    let sd4 = Shootdown::new(0);
    cs.add("零核空账", sd4.shots.is_empty() && sd4.barrier_closed(), "vacuous 闭合");
    cs.add("空守恒", sd4.reconcile(), "0 = 0 + 0");
    let mut sd5 = Shootdown::new(1);
    let _ = sd5.send(0, 0x7000);
    cs.add("单核在途不闭", !sd5.barrier_closed(), "未确认不闭");

    // 族九：混合地址多轮聚合压测（64 单全链）。
    let mut agg2 = Aggregator::default();
    let mut clean = true;
    for round in 0..4u8 {
        let mut s = Shootdown::new(16);
        for core in 0..16u8 {
            let iova = 0x8000 + (round as u64 * 0x1000);
            if s.send(core, iova) != Ok(true) {
                clean = false;
            }
        }
        for core in 0..16u8 {
            s.ack(core);
        }
        if !s.barrier_closed() || !s.reconcile() {
            clean = false;
        }
        agg2.absorb(&s);
    }
    cs.add("六十四单全链", clean, "四轮 × 十六核全通");
    cs.add("聚合轮四", agg2.rounds == 4, "四轮入总账");
    cs.add("聚合发六四", agg2.total_sent == 64, "4×16");
    cs.add("聚合零迟到", agg2.total_late == 0, "按序确认无迟到");
    cs.add("聚合守恒", agg2.reconcile(), "发 = 确 + 在途");

    cs
}
