//! VE-F3962 · S01-S03 组自查核验
//!
//! 三组逐项核验：S01 无障碍渲染管线六断言复测（层级/裁剪/缓存/降级/回退/
//! 审计）；S02 高对比引擎四色复测（正常色/高对比/色盲模拟/暗色）+ 对比度
//! 阈值 4.5:1 复测；S03 读屏协同语义树快照复测（焦点序/朗读序/快捷键序
//! 三序一致）——证据引用制执行（每项挂色标或录证指针）。
//! 错误路径：断言不过→缺陷立案；证据缺→回补；三序不一致→管线回退定位。
//! 性能：复测 O(60 项)；定位 O(层)；快照比对 O(节点)。
//! 跨批对接：S01-S03 档案单源；F3965 组间对账上游；F3966 API 清点。
//! 无障碍与隐私：核验即无障碍实测（域本色）；无隐私面。

#![allow(dead_code)]
use alloc::vec;

extern crate alloc;

use alloc::vec::Vec;
use alloc::string::String;

// ── 诊断码段（0x2F80）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0x2F80;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct S9BCode(pub u16);

impl S9BCode {
    pub const OK: Self = Self(SEG_BASE);
    pub const ASSERT_FAIL: Self = Self(SEG_BASE + 1);
    pub const EVIDENCE_GAP: Self = Self(SEG_BASE + 2);
    pub const ORDER_MISMATCH: Self = Self(SEG_BASE + 3);
    pub const CONTRAST_LOW: Self = Self(SEG_BASE + 4);

    pub fn explain(self) -> &'static str {
        match self {
            Self::OK => "正常",
            Self::ASSERT_FAIL => "断言不过→缺陷立案",
            Self::EVIDENCE_GAP => "证据缺→回补",
            Self::ORDER_MISMATCH => "三序不一致→管线回退定位",
            Self::CONTRAST_LOW => "对比度 <4.5:1→立案",
            _ => "未知",
        }
    }
}

// ── S01 六断言复测 ───────────────────────────────────────────────────

/// S01 渲染管线六断言。
pub const S01_ASSERTS: [&str; 6] = ["层级", "裁剪", "缓存", "降级", "回退", "审计"];

/// 断言复测表：断言名 → 通过 + 证据指针。
#[derive(Debug, Clone)]
pub struct AssertRow {
    pub name: &'static str,
    pub pass: bool,
    /// 证据指针（空 = 缺证据）。
    pub evidence: Option<&'static str>,
}

/// S01 六断言复测器。
#[derive(Debug, Clone)]
pub struct S01Retest {
    pub rows: Vec<AssertRow>,
}

impl S01Retest {
    pub fn new() -> Self {
        Self { rows: Vec::new() }
    }

    pub fn submit(&mut self, name: &'static str, pass: bool, evidence: Option<&'static str>) -> S9BCode {
        if !S01_ASSERTS.contains(&name) {
            return S9BCode::ASSERT_FAIL;
        }
        self.rows.push(AssertRow { name, pass, evidence });
        S9BCode::OK
    }

    /// 断言不过清单（立案名单）。
    pub fn failed(&self) -> Vec<&'static str> {
        self.rows.iter().filter(|r| !r.pass).map(|r| r.name).collect()
    }

    /// 证据缺清单（回补名单）。
    pub fn no_evidence(&self) -> Vec<&'static str> {
        self.rows.iter().filter(|r| r.evidence.is_none()).map(|r| r.name).collect()
    }

    pub fn all_green(&self) -> bool {
        self.rows.len() == 6 && self.failed().is_empty() && self.no_evidence().is_empty()
    }
}

impl Default for S01Retest {
    fn default() -> Self {
        Self::new()
    }
}

// ── S02 四色复测 + 4.5:1 阈值 ────────────────────────────────────────

/// S02 四色模式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorMode {
    Normal,
    HighContrast,
    ColorBlind,
    Dark,
}

pub const COLOR_MODES: [ColorMode; 4] = [
    ColorMode::Normal,
    ColorMode::HighContrast,
    ColorMode::ColorBlind,
    ColorMode::Dark,
];

/// 对比度万分比整型（45_000 = 4.5:1）。
pub const CONTRAST_THRESHOLD_PPM: u32 = 45_000;

/// S02 复测器：四色逐档对比度实测。
#[derive(Debug, Clone)]
pub struct S02Retest {
    /// 每档对比度（万分比）。
    pub contrast: [u32; 4],
}

impl S02Retest {
    pub fn new(contrast: [u32; 4]) -> Self {
        Self { contrast }
    }

    /// 4.5:1 复测：四档全部达阈。
    pub fn all_pass(&self) -> bool {
        self.contrast.iter().all(|c| *c >= CONTRAST_THRESHOLD_PPM)
    }

    /// 低档清单。
    pub fn low_modes(&self) -> Vec<ColorMode> {
        COLOR_MODES
            .iter()
            .filter(|m| self.contrast[**m as usize] < CONTRAST_THRESHOLD_PPM)
            .map(|m| *m)
            .collect()
    }

    /// 最小余量（万分比）。
    pub fn min_margin(&self) -> u32 {
        self.contrast.iter().map(|c| c.saturating_sub(CONTRAST_THRESHOLD_PPM)).min().unwrap_or(0)
    }
}

// ── S03 三序一致快照复测 ─────────────────────────────────────────────

/// 语义树快照：三序（焦点/朗读/快捷键）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TripleOrder {
    pub focus: Vec<&'static str>,
    pub speak: Vec<&'static str>,
    pub hotkey: Vec<&'static str>,
}

impl TripleOrder {
    /// 三序一致断言：逐位相同。
    pub fn consistent(&self) -> bool {
        self.focus == self.speak && self.speak == self.hotkey
    }

    /// 首分歧序：不一致时返回首个分歧位（定位）。
    pub fn first_diverge(&self) -> Option<(usize, usize)> {
        let seqs = [&self.focus, &self.speak, &self.hotkey];
        let n = seqs.iter().map(|s| s.len()).max().unwrap_or(0);
        for pos in 0..n {
            for i in 1..3 {
                if seqs[i].get(pos) != seqs[0].get(pos) {
                    return Some((i, pos));
                }
            }
        }
        None
    }

    /// 管线回退定位：分歧序号 → 所属管线层。
    pub fn layer_of(seq: usize) -> &'static str {
        match seq {
            1 => "朗读管线",
            2 => "快捷键管线",
            _ => "焦点管线",
        }
    }
}

// ── 判据 ─────────────────────────────────────────────────────────────

pub fn run_ves216_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves216_s09s0103");

    // 01 六断言清单守恒
    {
        chk(&mut set, "C3962-01", S01_ASSERTS.len() == 6 && S01_ASSERTS[0] == "层级" && S01_ASSERTS[5] == "审计", "err");
    }
    // 02 六断言全绿 + 证据齐
    {
        let mut r = S01Retest::new();
        for a in S01_ASSERTS.iter() {
            r.submit(a, true, Some("色标"));
        }
        chk(&mut set, "C3962-02", r.all_green(), "err");
    }
    // 03 断言不过→立案名单
    {
        let mut r = S01Retest::new();
        r.submit("层级", true, Some("录证"));
        r.submit("降级", false, Some("色标"));
        chk(&mut set, "C3962-03", r.failed() == vec!["降级"] && !r.all_green(), "err");
    }
    // 04 证据缺→回补名单
    {
        let mut r = S01Retest::new();
        r.submit("裁剪", true, None);
        chk(&mut set, "C3962-04", r.no_evidence() == vec!["裁剪"] && !r.all_green(), "err");
    }
    // 05 非法断言名拒收
    {
        let mut r = S01Retest::new();
        chk(&mut set, "C3962-05", r.submit("幻断言", true, Some("e")) == S9BCode::ASSERT_FAIL, "err");
    }
    // 06 四色清单守恒
    {
        chk(&mut set, "C3962-06", COLOR_MODES.len() == 4 && ColorMode::Normal as usize == 0 && ColorMode::Dark as usize == 3, "err");
    }
    // 07 4.5:1 全过
    {
        let r = S02Retest::new([50_000, 70_000, 46_000, 60_000]);
        chk(&mut set, "C3962-07", r.all_pass() && r.low_modes().is_empty() && r.min_margin() == 1_000, "err");
    }
    // 08 低对比档检出→立案
    {
        let r = S02Retest::new([50_000, 44_999, 60_000, 45_000]);
        chk(&mut set, "C3962-08", r.low_modes() == vec![ColorMode::HighContrast] && !r.all_pass() && r.min_margin() == 0, "err");
    }
    // 09 阈值边界精确（45_000 = 4.5:1 达标）
    {
        let r = S02Retest::new([45_000; 4]);
        chk(&mut set, "C3962-09", r.all_pass(), "err");
    }
    // 10 三序一致
    {
        let t = TripleOrder { focus: vec!["A", "B", "C"], speak: vec!["A", "B", "C"], hotkey: vec!["A", "B", "C"] };
        chk(&mut set, "C3962-10", t.consistent() && t.first_diverge().is_none(), "err");
    }
    // 11 三序分歧→首分歧位定位
    {
        let t = TripleOrder { focus: vec!["A", "B", "C"], speak: vec!["A", "X", "C"], hotkey: vec!["A", "B", "C"] };
        chk(&mut set, "C3962-11", !t.consistent() && t.first_diverge() == Some((1, 1)) && TripleOrder::layer_of(1) == "朗读管线", "err");
    }
    // 12 长度失配也判分歧
    {
        let t = TripleOrder { focus: vec!["A", "B"], speak: vec!["A", "B", "C"], hotkey: vec!["A", "B"] };
        chk(&mut set, "C3962-12", !t.consistent() && t.first_diverge() == Some((1, 2)), "err");
    }
    // 13 诊断码窗界自守
    {
        chk(&mut set, "C3962-13", S9BCode::CONTRAST_LOW.0 < SEG_BASE + 16, "err");
    }
    // 14 错误码解释覆盖
    {
        let all = [S9BCode::OK, S9BCode::ASSERT_FAIL, S9BCode::EVIDENCE_GAP, S9BCode::ORDER_MISMATCH, S9BCode::CONTRAST_LOW];
        chk(&mut set, "C3962-14", all.iter().all(|c| c.explain() != "未知"), "err");
    }

    set
}

// ── 测试支撑 ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_s09_s0103_all_green() {
        let s = run_ves216_checks();
        assert!(s.all_passed(), "ves216 red: {}", s.red_items_text());
    }

    #[test]
    fn test_s01_full_cycle() {
        let mut r = S01Retest::new();
        for a in S01_ASSERTS.iter() {
            r.submit(a, true, Some("证据")).ok();
        }
        assert!(r.all_green());
        r.rows[2].pass = false;
        assert_eq!(r.failed(), vec!["缓存"]);
    }

    #[test]
    fn test_triple_order_layer_mapping() {
        assert_eq!(TripleOrder::layer_of(0), "焦点管线");
        assert_eq!(TripleOrder::layer_of(1), "朗读管线");
        assert_eq!(TripleOrder::layer_of(2), "快捷键管线");
    }
}
