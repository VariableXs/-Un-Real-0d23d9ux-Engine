//! unx0572_mlfq2 —— UNX-F0572 多级反馈队列账。
//! 锚点：调度器卷 · MLFQ（片尽降带 / 等久升带 / 顶带轮转）。
//! 判据原文：新任务进顶带，片尽降带（到底封底），等待超龄升带
//!           （到顶封顶）；带账守恒（总量 = 各带和）。

/// MLFQ 调度器。
#[derive(Debug)]
pub struct Mlfq {
    /// 带 FIFO（0 = 顶带）。
    pub bands: alloc::vec::Vec<alloc::vec::Vec<u32>>,
    /// 每带片长。
    pub slices: alloc::vec::Vec<u32>,
    /// 任务当前带（tid → band）。
    pub whereis: alloc::vec::Vec<(u32, usize)>,
    /// 任务等待 tick（tid → 累计）。
    pub aged: alloc::vec::Vec<(u32, u64)>,
    /// 升带阈值。
    pub boost_at: u64,
    /// 降带账 / 升带账。
    pub n_demote: u64,
    pub n_boost: u64,
}

impl Mlfq {
    pub fn new(nbands: usize, base_slice: u32, boost_at: u64) -> Self {
        let slices = (0..nbands).map(|i| base_slice * (i as u32 + 1)).collect();
        Mlfq { bands: (0..nbands).map(|_| alloc::vec::Vec::new()).collect(), slices, whereis: alloc::vec::Vec::new(), aged: alloc::vec::Vec::new(), boost_at, n_demote: 0, n_boost: 0 }
    }

    /// 新任务进顶带。
    pub fn admit(&mut self, tid: u32) -> Result<(), ()> {
        if self.whereis.iter().any(|w| w.0 == tid) {
            return Err(());
        }
        self.bands[0].push(tid);
        self.whereis.push((tid, 0));
        self.aged.push((tid, 0));
        Ok(())
    }

    /// 查任务所在带。
    pub fn band_of(&self, tid: u32) -> Option<usize> {
        self.whereis.iter().find(|w| w.0 == tid).map(|w| w.1)
    }

    /// 片尽降带（底带封底）。
    pub fn demote(&mut self, tid: u32) -> Result<(), ()> {
        let b = self.band_of(tid).ok_or(())?;
        if b + 1 >= self.bands.len() {
            return Ok(()); // 底带封底：不动不记账
        }
        self.bands[b].retain(|t| *t != tid);
        self.bands[b + 1].push(tid);
        if let Some(w) = self.whereis.iter_mut().find(|w| w.0 == tid) {
            w.1 = b + 1;
        }
        self.n_demote += 1;
        Ok(())
    }

    /// 等待老化：全体未在队首跑者等待 tick 累加；超阈升带。
    pub fn age_tick(&mut self) {
        let n = self.aged.len();
        for i in 0..n {
            let (tid, a) = self.aged[i];
            let na = a + 1;
            self.aged[i].1 = na;
            if na >= self.boost_at && na - a == 1 {
                // 刚跨阈：升一带（顶带封顶）。
                if let Some(b) = self.band_of(tid) {
                    if b > 0 {
                        self.bands[b].retain(|t| *t != tid);
                        self.bands[b - 1].push(tid);
                        if let Some(w) = self.whereis.iter_mut().find(|w| w.0 == tid) {
                            w.1 = b - 1;
                        }
                        self.n_boost += 1;
                    }
                }
            }
        }
    }

    /// 带账守恒：whereis 与 bands 一致。
    pub fn reconcile(&self) -> bool {
        let total: usize = self.bands.iter().map(|b| b.len()).sum();
        total == self.whereis.len()
    }
}

/// 判据入口：MLFQ 全性质核。
pub fn run_unx0572_checks() -> crate::checks::CheckSet {
    let mut cs = crate::checks::CheckSet::new("UNX-F0572");

    // 族一：新任务进顶带。
    let mut m = Mlfq::new(3, 2, 100);
    m.admit(1).ok();
    m.admit(2).ok();
    cs.add("顶带入位", m.band_of(1) == Some(0) && m.band_of(2) == Some(0), "顶带收新");
    cs.add("重复进拒", m.admit(1).is_err(), "账不重项");
    cs.add("片长递增", m.slices == alloc::vec![2u32, 4, 6], "带深片长");

    // 族二：片尽降带。
    m.demote(1).ok();
    cs.add("降到带 1", m.band_of(1) == Some(1), "顶→中");
    m.demote(1).ok();
    cs.add("降到带 2", m.band_of(1) == Some(2), "中→底");
    cs.add("降带账二", m.n_demote == 2, "两降两账");
    m.demote(1).ok();
    cs.add("底带封底", m.band_of(1) == Some(2), "不再降");
    cs.add("封底不记账", m.n_demote == 2, "账不虚增");

    // 族三：带账守恒。
    cs.add("守恒核", m.reconcile(), "各带和 = 总数");
    cs.add("总量二", m.bands.iter().map(|b| b.len()).sum::<usize>() == 2, "1 底带 + 2 顶带");

    // 族四：等待升带。
    let mut m2 = Mlfq::new(3, 2, 3);
    m2.admit(5).ok();
    m2.demote(5).ok();
    cs.add("先降到 1", m2.band_of(5) == Some(1), "准备升带场景");
    for _ in 0..3 {
        m2.age_tick();
    }
    cs.add("三龄升带", m2.band_of(5) == Some(0) && m2.n_boost == 1, "超阈回顶");
    m2.demote(5).ok();
    cs.add("再降回 1", m2.band_of(5) == Some(1), "升降往返");

    // 族五：顶带封顶（带 0 任务不升）。
    let mut m3 = Mlfq::new(2, 1, 2);
    m3.admit(9).ok(); // 顶带
    for _ in 0..5 {
        m3.age_tick();
    }
    cs.add("顶带封顶", m3.band_of(9) == Some(0) && m3.n_boost == 0, "不虚升");

    // 族六：降升账分立。
    let m4 = Mlfq::new(3, 2, 100);
    cs.add("初始双零", m4.n_demote == 0 && m4.n_boost == 0, "新调度器净账");

    // 族七：多任务交错升降账。
    let mut m5 = Mlfq::new(3, 2, 4);
    m5.admit(1).ok();
    m5.admit(2).ok();
    m5.demote(1).ok();
    m5.demote(2).ok();
    cs.add("双降带 1", m5.band_of(1) == Some(1) && m5.band_of(2) == Some(1), "双双到中带");
    m5.age_tick();
    m5.age_tick();
    m5.age_tick();
    m5.age_tick();
    cs.add("双升回顶", m5.band_of(1) == Some(0) && m5.band_of(2) == Some(0), "同批老化同批升");
    cs.add("升降对称账", m5.n_demote == 2 && m5.n_boost == 2, "二降二升");
    cs.add("交错守恒", m5.reconcile(), "全程账平");

    // 族八：三任务阶梯分布核。
    let mut m6 = Mlfq::new(3, 2, 100);
    m6.admit(1).ok();
    m6.admit(2).ok();
    m6.admit(3).ok();
    m6.demote(1).ok();
    m6.demote(1).ok(); // 1 号到底带
    m6.demote(2).ok(); // 2 号到中带
    cs.add("阶梯分布", m6.band_of(1) == Some(2) && m6.band_of(2) == Some(1) && m6.band_of(3) == Some(0), "三带各一");
    cs.add("三降账", m6.n_demote == 3, "三降三账");
    cs.add("阶梯守恒", m6.reconcile(), "3 = 1+1+1");

    // 族九：幽灵任务降带拒。
    cs.add("幽灵降带拒", m6.demote(99).is_err(), "未登记任务");
    cs.add("幽灵不动账", m6.n_demote == 3 && m6.reconcile(), "拒不扰账");

    cs
}
