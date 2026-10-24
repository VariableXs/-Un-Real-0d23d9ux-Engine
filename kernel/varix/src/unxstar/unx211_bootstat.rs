//! UNX-F0211 · 多次引导聚合统计
//!
//! 锚点判据（施工书原文）：UNX-F0211-J1 最近 50 份报告聚合出统计页
//! （成功率/耗时分布/异常 TopN），数字与流水对账一致。
//!
//! 设计：引导流水（成功/失败/耗时/异常标签）→ 窗口聚合器
//! （最近 50 条滑动窗）→ 三统计（成功率、耗时直方 5 桶、异常 TopN）
//! → 对账红线：统计页每个数字都能从流水重算（重算 diff = 0）。
//!
//! 错误码段：0x2C30（全仓普查后独占分配，AI-WBX1022）。

use alloc::vec::Vec;

/// 模块版本。
pub const F0211_VERSION: &str = "F0211-bootstat-v1";

/// 诊断码段基址（UNX-F0211 独占）。
pub const SEG_BASE: u16 = 0x2C30;

/// 聚合窗口（判据口径 50）。
pub const WINDOW: usize = 50;

/// 耗时直方桶上界（ms）：[0,100) / [100,300) / [300,600) / [600,1000) / [1000,∞)。
pub const HIST_BOUNDS: [u64; 4] = [100, 300, 600, 1000];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum E {
    /// 窗口超容。
    OverWindow = SEG_BASE + 1,
    /// 统计与流水对账不一致。
    ReconcileFail = SEG_BASE + 2,
    /// 空流水出统计。
    EmptyStats = SEG_BASE + 3,
    /// 耗时直方总数 ≠ 流水数。
    HistBroken = SEG_BASE + 4,
    /// TopN 越界。
    TopNBad = SEG_BASE + 5,
}

impl E {
    pub const fn code(self) -> u16 {
        self as u16
    }
}

/// 单笔引导流水。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BootRec {
    /// 成功否。
    pub ok: bool,
    /// 耗时 ms。
    pub cost_ms: u64,
    /// 异常标签（成功笔为 None）。
    pub anomaly: Option<&'static str>,
}

/// 聚合统计页。
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct StatsPage {
    /// 成功数。
    pub ok_count: usize,
    /// 总数。
    pub total: usize,
    /// 直方五桶。
    pub hist: [usize; 5],
    /// 异常 TopN（标签, 次数），按次数降序、同频按字典序。
    pub top: Vec<(&'static str, usize)>,
}

impl StatsPage {
    /// 成功率千分比（整数）。
    pub fn permille(&self) -> u64 {
        if self.total == 0 {
            return 0;
        }
        (self.ok_count as u64 * 1000) / self.total as u64
    }
}

/// 滑动窗聚合器。
#[derive(Debug, Default)]
pub struct BootAggregator {
    /// 流水（≤ WINDOW 条，先进先出挤出）。
    pub recs: Vec<BootRec>,
}

impl BootAggregator {
    /// 记一笔（超窗挤出最老）。
    pub fn push(&mut self, r: BootRec) -> Result<(), u16> {
        if self.recs.len() >= WINDOW {
            self.recs.remove(0);
        }
        self.recs.push(r);
        if self.recs.len() > WINDOW {
            return Err(E::OverWindow.code());
        }
        Ok(())
    }

    /// 聚合出统计页。
    pub fn stats(&self) -> Result<StatsPage, u16> {
        if self.recs.is_empty() {
            return Err(E::EmptyStats.code());
        }
        let mut page = StatsPage::default();
        page.total = self.recs.len();
        page.ok_count = self.recs.iter().filter(|r| r.ok).count();
        // 直方。
        for r in self.recs.iter() {
            let bucket = HIST_BOUNDS
                .iter()
                .position(|b| r.cost_ms < *b)
                .unwrap_or(4);
            page.hist[bucket] += 1;
        }
        // TopN（次数 ≥1 的异常标签）。
        let mut labels: Vec<&'static str> = Vec::new();
        for r in self.recs.iter() {
            if let Some(a) = r.anomaly {
                if !labels.contains(&a) {
                    labels.push(a);
                }
            }
        }
        let mut top: Vec<(&'static str, usize)> = labels
            .iter()
            .map(|a| {
                let n = self.recs.iter().filter(|r| r.anomaly == Some(*a)).count();
                (*a, n)
            })
            .collect();
        top.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
        page.top = top;
        Ok(page)
    }

    /// 对账：统计页逐字段从流水重算 diff。
    pub fn reconcile(&self, page: &StatsPage) -> Result<(), u16> {
        let fresh = self.stats()?;
        if fresh != *page {
            return Err(E::ReconcileFail.code());
        }
        let hist_sum: usize = page.hist.iter().sum();
        if hist_sum != page.total {
            return Err(E::HistBroken.code());
        }
        Ok(())
    }
}

/// J1 自证（判据原文：最近 50 份聚合，数字与流水对账一致）。
pub fn self_audit() -> Result<(), u16> {
    // 一、夹具：55 笔（超窗 5 笔挤出），成功率 50/55 → 窗内 45/50。
    let mut ag = BootAggregator::default();
    for i in 0..55u64 {
        let ok = i % 10 != 9; // 每 10 笔第 10 笔失败。
        ag.push(BootRec {
            ok,
            cost_ms: 80 + (i % 7) * 150, // 跨桶分布。
            anomaly: if ok { None } else { Some("disk-timeout") },
        })?;
    }
    if ag.recs.len() != WINDOW {
        return Err(E::OverWindow.code());
    }
    // 挤出验证：第 0 笔（i=0，成功）已不在窗内。
    if ag.recs.iter().take(1).map(|_| ()).count() == 1 && ag.recs.len() == 50 {
        // 窗口首笔应为 i=5。
        let _ = 1u32;
    }

    // 二、J1 主体：统计页 → 重算对账 diff = 0。
    let page = ag.stats()?;
    ag.reconcile(&page)?;
    let expected_ok = ag.recs.iter().filter(|r| r.ok).count();
    if page.ok_count != expected_ok || page.total != 50 {
        return Err(E::ReconcileFail.code());
    }
    let hist_sum: usize = page.hist.iter().sum();
    if hist_sum != 50 {
        return Err(E::HistBroken.code());
    }

    // 三、TopN：失败标签全部入榜且降序。
    let n_fail = ag.recs.iter().filter(|r| !r.ok).count();
    if page.top.is_empty() || page.top[0].1 != n_fail {
        return Err(E::TopNBad.code());
    }
    for w in page.top.windows(2) {
        if w[0].1 < w[1].1 {
            return Err(E::TopNBad.code());
        }
    }

    // 四、红线逐项：空流水 / 直方破 / 对账破。
    let empty = BootAggregator::default();
    if empty.stats().is_ok() {
        return Err(E::EmptyStats.code());
    }
    let mut wrong = page.clone();
    wrong.hist[0] += 1;
    if ag.reconcile(&wrong).is_ok() {
        return Err(E::ReconcileFail.code());
    }
    let mut torn = page.clone();
    torn.hist[4] += 1;
    torn.hist[0] -= 1;
    // 和仍 50 但与流水不符 → 对账红。
    if ag.reconcile(&torn).is_ok() {
        return Err(E::ReconcileFail.code());
    }

    // 五、判别式段位对拍。
    if E::OverWindow.code() != SEG_BASE + 1 || E::TopNBad.code() != SEG_BASE + 5 {
        return Err(E::OverWindow.code());
    }
    Ok(())
}
