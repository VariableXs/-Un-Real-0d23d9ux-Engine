//! unx0547_genbar —— UNX-F0547 世代屏障账。
//! 锚点：SMP 卷 · 多核屏障（计数到齐翻代 / 提前到拒 / 世代防重入）。
//! 判据原文：参与数到齐才翻代（代号单调），早代到达者不得携旧代号
//!           重入新代；重入账与迟到账分立。

/// 世代屏障。
#[derive(Debug)]
pub struct GenBarrier {
    /// 参与核数。
    pub parties: u8,
    /// 当前代号。
    pub gen: u64,
    /// 本代已到计数。
    pub arrived: u8,
    /// 已翻代次数。
    pub n_release: u64,
    /// 迟到账（翻代后仍带旧代号报到）。
    pub n_late: u64,
    /// 到达者代号账（防重入）。
    pub arrivals: alloc::vec::Vec<(u8, u64)>,
}

impl GenBarrier {
    pub fn new(parties: u8) -> Self {
        GenBarrier { parties, gen: 0, arrived: 0, n_release: 0, n_late: 0, arrivals: alloc::vec::Vec::new() }
    }

    /// 到达：携带自报代号；代号落后于当前代记迟到并拒。
    pub fn arrive(&mut self, core: u8, claimed_gen: u64) -> Result<bool, ()> {
        if claimed_gen != self.gen {
            self.n_late += 1;
            return Err(());
        }
        if self.arrivals.iter().any(|a| a.0 == core && a.1 == self.gen) {
            return Err(()); // 同代重复到
        }
        self.arrivals.push((core, self.gen));
        self.arrived += 1;
        if self.arrived == self.parties {
            self.gen += 1;
            self.arrived = 0;
            self.arrivals.clear();
            self.n_release += 1;
            Ok(true) // 翻代
        } else {
            Ok(false)
        }
    }

    /// 当前代号快照。
    pub fn cur_gen(&self) -> u64 {
        self.gen
    }

    /// 本代余位。
    pub fn slots_left(&self) -> u8 {
        self.parties - self.arrived
    }
}

/// 判据入口：世代屏障全性质核。
pub fn run_unx0547_checks() -> crate::checks::CheckSet {
    let mut cs = crate::checks::CheckSet::new("UNX-F0547");

    // 族一：部分到达不翻代。
    let mut b = GenBarrier::new(4);
    cs.add("一至未翻", b.arrive(0, 0) == Ok(false), "1/4");
    cs.add("二至未翻", b.arrive(1, 0) == Ok(false), "2/4");
    cs.add("余位二", b.slots_left() == 2, "4-2");
    cs.add("代零未翻", b.cur_gen() == 0 && b.n_release == 0, "代不动");

    // 族二：到齐翻代。
    cs.add("三至未翻", b.arrive(2, 0) == Ok(false), "3/4");
    cs.add("四至翻代", b.arrive(3, 0) == Ok(true), "4/4 翻");
    cs.add("代进一", b.cur_gen() == 1 && b.n_release == 1, "代号单调");
    cs.add("账清零", b.arrived == 0 && b.arrivals.is_empty(), "新代净账");

    // 族三：旧代号迟到拒。
    cs.add("旧代号拒", b.arrive(0, 0).is_err(), "带代 0 报代 1");
    cs.add("迟到账一", b.n_late == 1, "迟到入账");
    cs.add("迟到不占位", b.arrived == 0, "拒到不计");

    // 族四：同代重复到拒。
    b.arrive(0, 1).ok();
    cs.add("重入拒", b.arrive(0, 1).is_err(), "同代二报");
    cs.add("重入不占位", b.arrived == 1, "仍一席");

    // 族五：新代号继续到齐翻代。
    b.arrive(1, 1).ok();
    b.arrive(2, 1).ok();
    cs.add("二代翻", b.arrive(3, 1) == Ok(true), "代 1 齐");
    cs.add("双翻账", b.n_release == 2 && b.cur_gen() == 2, "两代齐翻");

    // 族六：三核屏障全链压测（三代循环）。
    let mut b2 = GenBarrier::new(3);
    let mut clean = true;
    for round in 0..3u64 {
        for core in 0..3u8 {
            if b2.arrive(core, round) != Ok(core == 2) {
                clean = false;
            }
        }
        if b2.cur_gen() != round + 1 {
            clean = false;
        }
    }
    cs.add("三代全翻", clean && b2.n_release == 3, "3 核 × 3 代");
    cs.add("压测零迟到", b2.n_late == 0, "自报代号全程吻合");

    // 族七：双核屏障快速往返（代号严格交替核）。
    let mut b3 = GenBarrier::new(2);
    let mut seq_ok = true;
    for g in 0..8u64 {
        if b3.arrive(0, g) != Ok(false) || b3.arrive(1, g) != Ok(true) {
            seq_ok = false;
        }
    }
    cs.add("八代交替", seq_ok && b3.n_release == 8, "2 核 × 8 代全通");
    cs.add("终代八", b3.cur_gen() == 8, "代号推进到底");

    // 族八：迟到者搅局不破代（迟到账独立、翻代不受扰）。
    let mut b4 = GenBarrier::new(2);
    b4.arrive(0, 0).ok();
    b4.arrive(1, 0).ok(); // 代 0 翻 → 代 1
    cs.add("旧代号迟到", b4.arrive(0, 0).is_err(), "代 0 已封");
    b4.arrive(0, 1).ok();
    b4.arrive(1, 1).ok(); // 代 1 翻
    cs.add("迟到不阻翻代", b4.cur_gen() == 2 && b4.n_release == 2, "两代照翻");
    cs.add("迟到账独立", b4.n_late == 1 && b4.n_release == 2, "不混主流账");

    // 族九：八核大屏障两代压测。
    let mut b5 = GenBarrier::new(8);
    let mut big_ok = true;
    for round in 0..2u64 {
        for core in 0..8u8 {
            if b5.arrive(core, round) != Ok(core == 7) {
                big_ok = false;
            }
        }
    }
    cs.add("八核两代", big_ok && b5.n_release == 2 && b5.cur_gen() == 2, "16 到 2 翻");
    cs.add("大屏零迟到", b5.n_late == 0, "自报全程吻合");

    // 族十：翻代清账核（arrivals 与 arrived 均随翻代归零）。
    let mut b6 = GenBarrier::new(3);
    b6.arrive(0, 0).ok();
    b6.arrive(1, 0).ok();
    cs.add("中途账二", b6.arrived == 2, "2/3");
    b6.arrive(2, 0).ok(); // 翻代
    cs.add("翻代清位", b6.arrived == 0 && b6.arrivals.is_empty(), "代内账归零");
    cs.add("余位重满", b6.slots_left() == 3, "新代全位可用");
    // 同核新代可再报（跨代重入合法）。
    cs.add("跨代重入合法", b6.arrive(0, 1) == Ok(false), "新代号新账");

    cs
}
