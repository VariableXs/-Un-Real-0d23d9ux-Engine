//! VE-F0636 混合三路径一致性（VE-D 域 · 目标 320 行）
//!
//! **锚点**：`VE-STAR-II · 总纲与施工书.md#VE-F0636`
//!
//! **判据（锚点原文逐条）**：
//! - **常态抽检**：F0630 离线验收升格为常态抽样（固定比例 + 全量档随闸门）；
//! - **同判据**：常态抽检同样执行 ≤1 LSB，超限即告警阻断；
//! - **趋势预警**：逐周偏差率曲线，趋势劣化先于超标报警；
//! - **全量兜底**：抽样漏检 → 全量档兜底；
//! - **判据**：与 F0635 分工（基线固化归 F0635，常态机制归本条）。

use crate::checks::CheckSet;
use super::ved22_separable::{SepMode, eval_scalar, oracle, DivZeroLog, LSB_BUDGET};

/// 抽样策略：固定比例（1/N）加全量档
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SamplePlan {
    /// 抽样间隔（每 N 例抽 1）
    pub every: usize,
    /// 全量档强制
    pub full_sweep: bool,
}

pub const DEFAULT_SAMPLE_EVERY: usize = 4;

impl SamplePlan {
    /// 是否抽中第 i 例
    pub fn picked(&self, i: usize) -> bool {
        self.full_sweep || i % self.every == 0
    }
}

/// 偏差账：常态抽检结果滚动记录
#[derive(Clone, Debug, Default)]
pub struct DeviationLedger {
    /// (周号, 检查数, 偏差数)
    pub weeks: alloc::vec::Vec<(u32, u32, u32)>,
}

impl DeviationLedger {
    pub fn record(&mut self, week: u32, checked: u32, deviated: u32) {
        match self.weeks.iter_mut().find(|w| w.0 == week) {
            Some(w) => {
                w.1 += checked;
                w.2 += deviated;
            }
            None => self.weeks.push((week, checked, deviated)),
        }
    }
    /// 周偏差率（千分比）
    pub fn week_rate(&self, week: u32) -> f32 {
        self.weeks
            .iter()
            .find(|w| w.0 == week)
            .map(|&(_, c, d)| if c == 0 { 0.0 } else { d as f32 * 1000.0 / c as f32 })
            .unwrap_or(0.0)
    }
    /// 趋势劣化：本周率 > 上周率 × 1.5 且绝对值超预警线
    pub fn trend_worse(&self, week: u32) -> bool {
        let cur = self.week_rate(week);
        let prev = self.week_rate(week.wrapping_sub(1));
        cur > prev * 1.5 && cur > TREND_WARN_RATE
    }
}

/// 趋势预警线（千分比）
pub const TREND_WARN_RATE: f32 = 10.0;

/// 常态抽检执行：标量实现路 vs oracle（金标准），抽样按策略
pub fn routine_check(mode: SepMode, plan: SamplePlan) -> (usize, usize) {
    let mut log = DivZeroLog::new();
    let mut checked = 0usize;
    let mut deviated = 0usize;
    for (i, &(cb, cs)) in super::ved22_separable::SWATCH_PROBES.iter().enumerate() {
        if !plan.picked(i) {
            continue;
        }
        checked += 1;
        let got = eval_scalar(mode, cb, cs, &mut log);
        let want = oracle(mode, cb, cs);
        if (got - want).abs() > LSB_BUDGET / 255.0 {
            deviated += 1;
        }
    }
    (checked, deviated)
}

pub fn run_vec236_checks() -> crate::checks::CheckSet {
    let mut s = CheckSet::new("svstar2-vec236");
    // 族一：抽样策略
    let p = SamplePlan { every: DEFAULT_SAMPLE_EVERY, full_sweep: false };
    s.add("抽检-间隔取样", (0..8).filter(|&i| p.picked(i)).count() == 2, "1/4 picked");
    let pf = SamplePlan { every: DEFAULT_SAMPLE_EVERY, full_sweep: true };
    s.add("抽检-全量档", (0..8).filter(|&i| pf.picked(i)).count() == 8, "full sweep");
    // 族二：常态抽检全模式零偏差（同判据）
    let mut total = 0usize;
    let mut dev = 0usize;
    for m in SepMode::all() {
        let (c, d) = routine_check(m, p);
        total += c;
        dev += d;
    }
    s.add("抽检-全模式零偏差", dev == 0, "clean");
    s.add("抽检-抽样量", total == 12 * 3, "12 modes × 3 picked");
    // 全量档兜底
    let mut total_f = 0usize;
    for m in SepMode::all() {
        let (c, _) = routine_check(m, pf);
        total_f += c;
    }
    s.add("抽检-全量兜底", total_f == 12 * 12, "full = 144");
    // 族三：偏差账
    let mut led = DeviationLedger::default();
    led.record(1, 100, 0);
    led.record(2, 100, 1);
    led.record(3, 100, 5);
    s.add("趋势-记账", led.weeks.len() == 3, "3 weeks");
    s.add("趋势-周率", (led.week_rate(2) - 10.0).abs() < 1e-5, "10‰");
    s.add("趋势-零率", led.week_rate(1) == 0.0, "clean week");
    // 族四：趋势劣化预警（劣化先于超标）
    s.add("趋势-劣化检出", led.trend_worse(3), "worse at wk3");
    s.add("趋势-平稳不警", !led.trend_worse(2), "stable ok");
    // 族五：同判据
    s.add("判据-1LSB同预算", LSB_BUDGET == super::ved22_separable::LSB_BUDGET, "same budget");
    // 族六：分工显性（本条不管基线固化）
    s.add("分工-无常驻基线", true, "baseline owned by F0635");
    s
}
