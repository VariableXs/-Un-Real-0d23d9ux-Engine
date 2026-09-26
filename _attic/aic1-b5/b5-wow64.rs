
// ---------------------------------------------------------------------------
// F004 · 深化批次五：诚实卡片样本账本聚合面（10/10 判据的记账核算）
//
// 主册依据（G-A-04【验收判据】）：「32 位样本集 10 枚 100% 触发诚实卡片
// （零静默失败/零崩溃）」——判据是记账题：10 个样本 × (出卡? 崩溃? 静默?)
// → 账本聚合出率（本面是核算核，样本注入走既有 detect_machine/honest_card）。
// ---------------------------------------------------------------------------

/// 单样本判账（一枚样本的三问）。
#[derive(Clone, Copy, Debug)]
pub struct SampleVerdict {
    pub refused: bool,
    pub card_shown: bool,
    pub crashed: bool,
    pub silent_fail: bool,
}

/// 样本账本聚合（10 枚判据线的核算核）。
pub struct RefusalStats {
    pub samples: [Option<SampleVerdict>; 10],
    pub n: usize,
}

impl RefusalStats {
    pub const fn new() -> RefusalStats {
        RefusalStats { samples: [None; 10], n: 0 }
    }

    pub fn record(&mut self, v: SampleVerdict) -> bool {
        if self.n >= self.samples.len() {
            return false;
        }
        self.samples[self.n] = Some(v);
        self.n += 1;
        true
    }

    /// 出卡率（permille；零样本 → None 不猜）。
    pub fn card_rate_permille(&self) -> Option<u32> {
        if self.n == 0 {
            return None;
        }
        let shown = self.samples[..self.n]
            .iter()
            .flatten()
            .filter(|v| v.card_shown && !v.crashed && !v.silent_fail)
            .count();
        Some((shown * 1000 / self.n) as u32)
    }

    /// 判据：10/10 全出卡且零崩溃零静默。
    pub fn meets_criterion(&self) -> bool {
        self.n == self.samples.len() && self.card_rate_permille() == Some(1000)
    }
}

/// F004 深化批次五自检。
pub fn run_wow64_deep4_checks() -> CheckSet {
    let mut cs = CheckSet::new("F004-wow64-deep4");
    // 1) 满账本判据：10 枚全出卡（零崩溃零静默）→ 1000‰ 达线。
    let mut st = RefusalStats::new();
    for _ in 0..10 {
        st.record(SampleVerdict { refused: true, card_shown: true, crashed: false, silent_fail: false });
    }
    cs.add(
        "refusal_stats_full_criterion",
        st.n == 10 && st.card_rate_permille() == Some(1000) && st.meets_criterion(),
        "",
    );
    // 2) 一枚静默失败 → 率 900‰，判据如实红（不粉饰）。
    let mut st2 = RefusalStats::new();
    for i in 0..10u32 {
        st2.record(SampleVerdict {
            refused: true,
            card_shown: i != 9,
            crashed: false,
            silent_fail: i == 9,
        });
    }
    cs.add(
        "refusal_stats_silent_fail_visible",
        st2.card_rate_permille() == Some(900) && !st2.meets_criterion(),
        "",
    );
    // 3) 零样本如实 None；满容如实拒（账本纪律）。
    let empty = RefusalStats::new();
    let mut full = RefusalStats::new();
    for _ in 0..12 {
        full.record(SampleVerdict { refused: true, card_shown: true, crashed: false, silent_fail: false });
    }
    cs.add(
        "refusal_stats_bounds_honest",
        empty.card_rate_permille().is_none() && full.n == 10,
        "",
    );
    cs
}
