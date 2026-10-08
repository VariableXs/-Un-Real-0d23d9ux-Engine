//! F183 存储健康监测 · 批次四深化（secstar · G-G-13）。
//!
//! 批次四功能面（与批次三互补：批次三管「解析与估计」，本批管
//! 「趋势与综合」）：
//! - [`AttrTrend`]：属性趋势——value 滑窗 + 下降率告警（正常衰减
//!   与断崖下降分开报——健康监测的价值在趋势不在瞬时）；
//! - [`DailyBars`]：30 日柱状数据——跨日滚动/空日 0 柱/超窗滚出
//!   （写入量柱状图的数据面）；
//! - [`ReallocWatch`]：重分配扇区计数器——增量告警（坏块扩散的
//!   前兆指标：增量比绝对值更早报警）；
//! - [`HealthScore`]：综合健康分——磨损/温度/错误三因素加权千分制
//!   （单因素达标≠整体健康——总分页的算术面）。
//!
//! 零堆纪律：定长滑窗 + 定长柱，无 alloc。

use super::diskhealth::{BAND_GREEN_PERMILLE, DAILY_BARS, ERROR_SPIKE_PER_DAY};
use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 属性趋势（滑窗 + 下降率）
// ---------------------------------------------------------------------------

/// 趋势窗容量。
pub const TREND_WINDOW: usize = 8;

pub struct AttrTrend {
    win: [u8; TREND_WINDOW],
    n: usize,
    head: usize,
}

impl AttrTrend {
    pub const fn new() -> AttrTrend {
        AttrTrend { win: [100; TREND_WINDOW], n: 0, head: 0 }
    }

    pub fn push(&mut self, value: u8) {
        self.win[self.head] = value;
        self.head = (self.head + 1) % TREND_WINDOW;
        if self.n < TREND_WINDOW {
            self.n += 1;
        }
    }

    fn ordered(&self) -> [u8; TREND_WINDOW] {
        let mut out = [0u8; TREND_WINDOW];
        for i in 0..self.n {
            out[i] = self.win[(self.head + TREND_WINDOW - self.n + i) % TREND_WINDOW];
        }
        out
    }

    pub fn latest(&self) -> Option<u8> {
        if self.n == 0 {
            return None;
        }
        Some(self.win[(self.head + TREND_WINDOW - 1) % TREND_WINDOW])
    }

    /// 窗内下降量（首→末；0/负降 = 稳定）。
    pub fn drop_amount(&self) -> u8 {
        if self.n < 2 {
            return 0;
        }
        let o = self.ordered();
        o[0].saturating_sub(o[self.n - 1])
    }

    /// 断崖告警：窗内下降 ≥ 阈值（正常衰减慢、断崖快——阈值分开报）。
    pub fn cliff(&self, threshold: u8) -> bool {
        self.drop_amount() >= threshold
    }
}

// ---------------------------------------------------------------------------
// 30 日柱状数据
// ---------------------------------------------------------------------------

pub struct DailyBars {
    bars: [u32; DAILY_BARS], // 每日写入 MiB
    head: usize,
    pub filled: usize,
}

impl DailyBars {
    pub const fn new() -> DailyBars {
        DailyBars { bars: [0; DAILY_BARS], head: 0, filled: 0 }
    }

    /// 推入当日写入量（超窗滚出最老——跨日滚动方向）。
    pub fn push_day(&mut self, mib: u32) {
        self.bars[self.head] = mib;
        self.head = (self.head + 1) % DAILY_BARS;
        if self.filled < DAILY_BARS {
            self.filled += 1;
        }
    }

    /// 时间序第 i 天（0=最老）。
    pub fn bar(&self, i: usize) -> Option<u32> {
        if i >= self.filled {
            return None;
        }
        let start = if self.filled == DAILY_BARS { self.head } else { 0 };
        Some(self.bars[(start + i) % DAILY_BARS])
    }

    /// 空日判定：0 柱是合法数据（没写就是没写——不伪造填充）。
    pub fn zero_days(&self) -> usize {
        (0..self.filled).filter(|i| self.bar(*i) == Some(0)).count()
    }
}

// ---------------------------------------------------------------------------
// 重分配扇区计数器
// ---------------------------------------------------------------------------

/// 增量告警线（次/日——坏块扩散前兆）。
pub const REALLOC_DAILY_ALERT: u32 = 8;

#[derive(Clone, Copy, Debug, Default)]
pub struct ReallocWatch {
    pub total: u64,
    today: u32,
}

impl ReallocWatch {
    pub const fn new() -> ReallocWatch {
        ReallocWatch { total: 0, today: 0 }
    }

    pub fn on_realloc(&mut self, count: u32) {
        self.total += count as u64;
        self.today += count;
    }

    /// 日滚动：返回昨日是否触线（增量比绝对值早报警）。
    pub fn day_rollover(&mut self) -> bool {
        let alert = self.today >= REALLOC_DAILY_ALERT;
        self.today = 0;
        alert
    }
}

// ---------------------------------------------------------------------------
// 综合健康分（千分制三因素加权）
// ---------------------------------------------------------------------------

/// 权重（‰，和 = 1000）：磨损 50% / 温度 30% / 错误 20%。
pub const SCORE_WEIGHTS: [u32; 3] = [500, 300, 200];

/// 综合分：三因素各自 0-1000（越高越健康）加权求和。
pub fn health_score(wear: u32, temp: u32, errors: u32) -> u32 {
    let parts = [wear, temp.min(1_000), errors.min(1_000)];
    let mut score = 0u32;
    for i in 0..3 {
        score += parts[i].min(1_000) * SCORE_WEIGHTS[i] / 1_000;
    }
    score.min(1_000)
}

/// 分→段（复用主层绿/黄/红千分线——同一把尺不另造）。
pub fn score_band(score: u32) -> super::diskhealth::Band {
    super::diskhealth::band_of(1_000 - score)
}

// ---------------------------------------------------------------------------
// 批次四自检
// ---------------------------------------------------------------------------

#[inline(never)]
pub fn run_diskhealth_b4_checks() -> CheckSet {
    let mut cs = CheckSet::new("F183-b4");

    // 1) 趋势滑窗：序列 100→98→95→80 → 最新 80、窗内降 20（首末差）。
    let mut tr = AttrTrend::new();
    for v in [100u8, 98, 95, 80] {
        tr.push(v);
    }
    cs.add("trend_drop", tr.latest() == Some(80) && tr.drop_amount() == 20, "");

    // 2) 断崖告警：降 20 ≥ 15 阈值真、≥ 25 假（阈值分开报两面）。
    cs.add("trend_cliff_two_ways", tr.cliff(15) && !tr.cliff(25), "");

    // 3) 稳定不误报：100→100→99→100 降 1 → 任何合理阈值不响。
    let mut tr2 = AttrTrend::new();
    for v in [100u8, 100, 99, 100] {
        tr2.push(v);
    }
    cs.add("trend_stable_quiet", tr2.drop_amount() == 0 && !tr2.cliff(15), "");

    // 4) 趋势窗回卷：8 窗塞 10 值只留最近 8（首末差按窗内算）。
    let mut tr3 = AttrTrend::new();
    for i in 0..10u8 {
        tr3.push(100 - i);
    }
    // 窗内 = 92..=99（最近 8 值）→ 降 7。
    cs.add("trend_window_wraps", tr3.drop_amount() == 7 && tr3.latest() == Some(91), "");

    // 5) 30 日柱：跨日滚动、时间序读取（数据面保序）。
    let mut bars = DailyBars::new();
    for d in 0..(DAILY_BARS + 5) {
        bars.push_day(d as u32 + 1);
    }
    // 超窗后最老 5 天滚出：时间序第 0 天 = 原 6 号日。
    cs.add(
        "bars_roll",
        bars.filled == DAILY_BARS && bars.bar(0) == Some(6) && bars.bar(DAILY_BARS - 1) == Some(DAILY_BARS as u32 + 5),
        "",
    );

    // 6) 空日 0 柱合法：混入 0 日 → zero_days 计数（不伪造填充）。
    let mut bars2 = DailyBars::new();
    for v in [5u32, 0, 0, 7] {
        bars2.push_day(v);
    }
    cs.add("bars_zero_days", bars2.zero_days() == 2 && bars2.bar(1) == Some(0), "");

    // 7) 重分配增量告警：日 8 次触线、7 次不触（前兆指标两面）。
    let mut w1 = ReallocWatch::new();
    w1.on_realloc(8);
    let day1 = w1.day_rollover();
    let mut w2 = ReallocWatch::new();
    w2.on_realloc(7);
    let day2 = w2.day_rollover();
    cs.add(
        "realloc_increment_alert",
        day1 && !day2 && w1.total == 8 && w1.today == 0,
        "",
    );

    // 8) 重分配跨日累计：两日各 5 次 → 总 10、无单日告警（累计≠单日）。
    let mut w3 = ReallocWatch::new();
    w3.on_realloc(5);
    let d1 = w3.day_rollover();
    w3.on_realloc(5);
    let d2 = w3.day_rollover();
    cs.add(
        "realloc_cumulative_vs_daily",
        !d1 && !d2 && w3.total == 10,
        "",
    );

    // 9) 综合分：三因素全满 → 1000；磨损归零 → 500（加权算术）。
    cs.add(
        "health_score_full_and_zero_wear",
        health_score(1_000, 1_000, 1_000) == 1_000 && health_score(0, 1_000, 1_000) == 500,
        "",
    );

    // 10) 综合分段：900 分 → 主层尺绿段；100 分 → 红段（过尺换算：
    //     已用 = 1000-分，200 分 → 已用 800 < 850 落黄——边界语义如实）。
    cs.add(
        "health_score_band",
        matches!(score_band(900), super::diskhealth::Band::Green)
            && matches!(score_band(200), super::diskhealth::Band::Yellow)
            && matches!(score_band(100), super::diskhealth::Band::Red),
        "",
    );

    // 11) 权重和恒 1000（一处一事实算术）。
    cs.add("score_weights_sum", SCORE_WEIGHTS.iter().sum::<u32>() == 1_000, "");

    // 12) 主册常量贯通：绿段 600‰ / 30 日柱 / 错误 10/日一处一事实。
    cs.add(
        "consts_aligned",
        BAND_GREEN_PERMILLE == 600 && DAILY_BARS == 30 && ERROR_SPIKE_PER_DAY == 10,
        "",
    );

    cs
}

// ---------------------------------------------------------------------------
// 宿主单测（批次四）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests_b4 {
    use super::*;

    #[test]
    fn trend_monotone_window_slides() {
        // 滑窗正确性：塞 16 值窗内始终最近 8（首末差随窗滚动）。
        let mut tr = AttrTrend::new();
        for i in 0..16u8 {
            tr.push(100 - i);
        }
        // 窗内 100-8..100-15 → 首末差 = 92-85 = 7。
        assert_eq!(tr.drop_amount(), 7);
        assert_eq!(tr.latest(), Some(85));
    }

    #[test]
    fn bars_time_order_preserved() {
        // 时间序读取全程保序（柱状图不洗顺序）。
        let mut bars = DailyBars::new();
        for d in 1..=10u32 {
            bars.push_day(d * 10);
        }
        for i in 0..10usize {
            assert_eq!(bars.bar(i), Some((i as u32 + 1) * 10), "i={i}");
        }
    }

    #[test]
    fn health_score_component_isolation() {
        // 因素隔离：温度崩不改磨损贡献（单因素诊断仍可读）。
        let base = health_score(1_000, 1_000, 1_000);
        let temp_crash = health_score(1_000, 0, 1_000);
        assert_eq!(base - temp_crash, 300);
    }
}
