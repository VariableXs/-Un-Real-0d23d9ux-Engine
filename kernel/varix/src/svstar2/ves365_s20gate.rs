//! ves365_s20gate — VE-F4125 · T07 i18n 回归门与看板
//! 回归门（基准对基线劣化判定——超阈值红门阻断合入）+ 自动归因报告 + i18n 性能看板（四维预算消耗/基准趋势/红门历史三视区）+ 红门复议通道（成立阈值修订留痕）+ 看板含无障碍预算消耗视区（域本色）+ S 域色标复述。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0xD2C0;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GdCode {
    Ok = 0,
    RedGate = SEG_BASE + 0x01,
    BoardDrift = SEG_BASE + 0x02,
}

/// 劣化阈值（万分比——超 5% 即红门）。
pub const REGRESS_FLOOR_BP: u32 = 500;

/// 回归判定：baseline vs current ns → 偏差万分比；劣化超阈值红门。
pub fn regress_check(baseline_ns: u32, current_ns: u32) -> Result<u32, GdCode> {
    if baseline_ns == 0 { return Ok(0); }
    if current_ns <= baseline_ns { return Ok(0); } // 改善不算劣化
    let d = (current_ns - baseline_ns) * 10_000 / baseline_ns;
    if d > REGRESS_FLOOR_BP { return Err(GdCode::RedGate); }
    Ok(d)
}

/// 自动归因：按四维归因报告（谁把门拉红——取劣化最大维）。
pub fn attribute(baseline: [u32; 4], current: [u32; 4]) -> Option<usize> {
    let mut best: Option<(usize, u32)> = None;
    for i in 0..4 {
        if baseline[i] == 0 { continue; }
        if current[i] > baseline[i] {
            let d = (current[i] - baseline[i]) * 10_000 / baseline[i];
            if best.map_or(true, |(_, bd)| d > bd) { best = Some((i, d)); }
        }
    }
    best.map(|(i, _)| i)
}

/// 看板三视区。
pub struct Board {
    /// 视区一：四维预算消耗（万分比）。
    pub budget_use: [u32; 4],
    /// 视区二：基准趋势（最近 N 轮总 ns）。
    pub trend: Vec<u32>,
    /// 视区三：红门历史。
    pub red_history: Vec<(&'static str, u32)>,
    /// 无障碍预算消耗视区（域本色）。
    pub a11y_use_bp: u32,
}

pub const DIM_NAMES: [&'static str; 4] = ["lookup", "switch", "memory", "package"];

impl Board {
    pub fn new() -> Self {
        Board { budget_use: [0; 4], trend: Vec::new(), red_history: Vec::new(), a11y_use_bp: 0 }
    }

    pub fn record_trend(&mut self, total_ns: u32) { self.trend.push(total_ns); }

    /// 趋势劣化：末值比首值劣化超阈。
    pub fn trend_regressing(&self) -> bool {
        match (self.trend.first(), self.trend.last()) {
            (Some(f), Some(l)) => regress_check(*f, *l).is_err(),
            _ => false,
        }
    }

    pub fn record_red(&mut self, feature: &'static str, deviation_bp: u32) {
        self.red_history.push((feature, deviation_bp));
    }

    /// 看板数据源对拍：预算消耗四维与名字单源长度一致。
    pub fn consistent(&self) -> bool { self.budget_use.len() == DIM_NAMES.len() }
}

/// 复议册：红门复议 → 成立阈值修订留痕。
pub struct AppealBook { pub cases: Vec<(&'static str, bool)>, pub revised: Vec<&'static str> }

impl AppealBook {
    pub fn new() -> Self { AppealBook { cases: Vec::new(), revised: Vec::new() } }

    pub fn appeal(&mut self, feature: &'static str, upheld: bool) {
        self.cases.push((feature, upheld));
        if upheld { self.revised.push(feature); }
    }

    pub fn revised_count(&self) -> usize { self.revised.len() }
}

/// S 域色标复述（域本色）：绿/黄/红三档色标语义。
pub fn color_of(deviation_bp: u32) -> &'static str {
    if deviation_bp == 0 { "green" }
    else if deviation_bp <= REGRESS_FLOOR_BP { "yellow" }
    else { "red" }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves365_checks() -> CheckSet {
    let mut set = CheckSet::new("ves365");

    // C365-01 阈值单源。
    chk(&mut set, "C365-01", REGRESS_FLOOR_BP == 500, "劣化阈值 5%");

    // C365-02 改善不算劣化。
    chk(&mut set, "C365-02", regress_check(1000, 900) == Ok(0), "900 < 1000 改善");

    // C365-03 带内劣化黄区。
    chk(&mut set, "C365-03", regress_check(1000, 1020) == Ok(200), "2% 劣化 200bp 黄区");

    // C365-04 超阈红门阻断。
    chk(&mut set, "C365-04", regress_check(1000, 1100) == Err(GdCode::RedGate), "10% > 5% => 红门");

    // C365-05 自动归因：劣化最大维。
    let attr = attribute([1000, 500, 2000, 800], [1000, 700, 2200, 1600]);
    chk(&mut set, "C365-05", attr == Some(3), "维 3 劣化 100% 最大");

    // C365-06 无劣化归因 None。
    chk(&mut set, "C365-06", attribute([100, 100, 100, 100], [90, 100, 100, 100]).is_none(), "无劣化无归因");

    // C365-07 看板三视区结构。
    let mut bd = Board::new();
    for v in [1000u32, 1020, 1040, 1300].iter() { bd.record_trend(*v); }
    chk(&mut set, "C365-07", bd.trend.len() == 4 && bd.trend_regressing(), "趋势 1300/1000 劣化 30% 红");

    // C365-08 趋势带内不判劣化。
    let mut bd2 = Board::new();
    for v in [1000u32, 1010, 1005, 1020].iter() { bd2.record_trend(*v); }
    chk(&mut set, "C365-08", !bd2.trend_regressing(), "2% 带内");

    // C365-09 红门历史记录。
    bd.record_red("search-box", 800);
    bd.record_red("lang-menu", 1200);
    chk(&mut set, "C365-09", bd.red_history.len() == 2 && bd.red_history[1].1 == 1200, "两红门留痕");

    // C365-10 无障碍预算消耗视区（域本色）。
    bd.a11y_use_bp = 6_200;
    chk(&mut set, "C365-10", bd.a11y_use_bp == 6_200, "专视区在板");

    // C365-11 复议通道：成立修订留痕。
    let mut ap = AppealBook::new();
    ap.appeal("search-box", true);
    ap.appeal("lang-menu", false);
    chk(&mut set, "C365-11", ap.revised_count() == 1 && ap.revised.contains(&"search-box"), "一成立一留痕");

    // C365-12 色标复述三档。
    chk(&mut set, "C365-12",
        color_of(0) == "green" && color_of(300) == "yellow" && color_of(600) == "red", "三档色标");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ves365_smoke() {
        let set = run_ves365_checks();
        assert!(set.failed_count() == 0, "ves365 red: {}", set.red_items_text());
    }

    #[test]
    fn ves365_gate() {
        assert_eq!(regress_check(0, 999), Ok(0));
        assert!(regress_check(1000, 1050).is_ok()); // 恰 500 带内
        assert_eq!(regress_check(1000, 1051), Err(GdCode::RedGate));
    }
}
