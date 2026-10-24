// SPDX-License-Identifier: Apache-2.0
// Varix Kernel — SVSTAR2 · ves104_a11yscan
// VE-F3704 · 主题无障碍扫描（R06 组 · 主题质量与一致性组）
//
// 锚点：VE-F3704
// 判据原文：阻断 P0、三单源复用、全对覆盖、分级报告、判据。
//
// 难点分析：
//   1. 阻断级（不可辨）问题容许即 P0——不可辨主题不可发布。
//   2. 对比度全组合扫描：前景×背景全对不能漏，漏扫组合即覆盖断言失败。
//   3. 三级分级（阻断/严重/建议）与处置力度对齐——阻断必须进发布门禁。

#![allow(dead_code)]

extern crate alloc;

use alloc::vec;
use alloc::vec::Vec;
use alloc::string::{String, ToString};

use crate::checks::CheckSet;

// ── 诊断码（0x9800 起）────────────────────────────────────────────

/// 诊断码段基址。
pub const SEG_BASE: u16 = 0x9800;

/// 诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct C98Code(pub u16);

impl C98Code {
    pub const BLOCKER_TOLERATED: Self = Self(SEG_BASE);   // 阻断级容许 P0
    pub const CONTRAST_FAIL: Self = Self(SEG_BASE + 1);   // 对比度不足
    pub const COLORBLIND_FAIL: Self = Self(SEG_BASE + 2); // 色弱不可辨
    pub const FOCUS_MISSING: Self = Self(SEG_BASE + 3);   // 焦点环缺失
    pub const COVER_SHORT: Self = Self(SEG_BASE + 4);     // 全对覆盖不足
    pub const A11Y_PASS: Self = Self(SEG_BASE + 5);       // 无障碍通过
    pub const GRADED: Self = Self(SEG_BASE + 6);          // 分级完成

    pub fn code(self) -> u16 {
        self.0
    }

    pub fn explain(self) -> &'static str {
        match self {
            Self::BLOCKER_TOLERATED => "阻断级问题容许——不可辨主题不可发布 P0",
            Self::CONTRAST_FAIL => "对比度不足——低对比（严重级）",
            Self::COLORBLIND_FAIL => "色弱模拟不可辨——阻断级",
            Self::FOCUS_MISSING => "焦点环缺失——焦点红线",
            Self::COVER_SHORT => "全对扫描覆盖不足——覆盖断言",
            Self::A11Y_PASS => "无障碍扫描通过",
            Self::GRADED => "问题分级完成",
            _ => "未知诊断",
        }
    }
}

// ── 问题分级 ──────────────────────────────────────────────────────

/// 无障碍问题级别。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Severity {
    /// 建议级。
    Suggestion,
    /// 严重级（低对比）。
    Serious,
    /// 阻断级（不可辨）。
    Blocker,
}

impl Severity {
    pub fn name(self) -> &'static str {
        match self {
            Self::Suggestion => "建议",
            Self::Serious => "严重",
            Self::Blocker => "阻断",
        }
    }

    /// 是否阻断发布。
    pub fn blocks_release(self) -> bool {
        matches!(self, Self::Blocker)
    }
}

/// 无障碍问题。
pub struct A11yIssue {
    pub severity: Severity,
    /// 问题指纹。
    pub fp: u64,
}

impl A11yIssue {
    pub fn new(severity: Severity, fp: u64) -> Self {
        Self { severity, fp }
    }
}

/// 分级报告。
pub struct A11yReport {
    issues: Vec<A11yIssue>,
    /// 阻断级容许次数（必须为 0）。
    blockers_tolerated: u32,
}

impl A11yReport {
    pub fn new() -> Self {
        Self { issues: Vec::new(), blockers_tolerated: 0 }
    }

    /// 登记问题。
    pub fn add(&mut self, issue: A11yIssue) {
        self.issues.push(issue);
    }

    /// 发布门：阻断级存在即拒绝。
    pub fn gate_release(&mut self) -> Result<(), C98Code> {
        for i in &self.issues {
            if i.severity.blocks_release() {
                self.blockers_tolerated = self.blockers_tolerated.saturating_add(1);
                return Err(C98Code::BLOCKER_TOLERATED);
            }
        }
        Ok(())
    }

    /// 按级计数。
    pub fn count_by(&self, s: Severity) -> usize {
        self.issues.iter().filter(|i| i.severity == s).count()
    }

    pub fn total(&self) -> usize {
        self.issues.len()
    }

    pub fn blockers_tolerated(&self) -> u32 {
        self.blockers_tolerated
    }
}

impl Default for A11yReport {
    fn default() -> Self {
        Self::new()
    }
}

// ── 对比度全对扫描 ────────────────────────────────────────────────

/// 对比度要求（WCAG 风格基点，10000=21:1）。
pub const CONTRAST_AA_BP: u16 = 4500; // ≈4.5:1

/// 颜色对。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ColorPair {
    pub fg: u16,
    pub bg: u16,
}

impl ColorPair {
    pub fn new(fg: u16, bg: u16) -> Self {
        Self { fg, bg }
    }
}

/// 全对扫描器。
pub struct ContrastScanner {
    /// 已扫对数。
    scanned: usize,
    /// 期望全对数。
    expected_pairs: usize,
    /// 不足对比对数。
    failures: u32,
}

impl ContrastScanner {
    /// 构造：n 色板 → n*(n-1) 有序对（fg≠bg）。
    pub fn new(palette_size: usize) -> Self {
        let expected = palette_size.saturating_mul(palette_size.saturating_sub(1));
        Self { scanned: 0, expected_pairs: expected, failures: 0 }
    }

    /// 扫一对。contrast_bp 为实测对比度。
    pub fn scan_pair(&mut self, _pair: ColorPair, contrast_bp: u16) -> Result<(), C98Code> {
        self.scanned = self.scanned.saturating_add(1);
        if contrast_bp < CONTRAST_AA_BP {
            self.failures = self.failures.saturating_add(1);
            return Err(C98Code::CONTRAST_FAIL);
        }
        Ok(())
    }

    /// 覆盖检查：已扫 == 期望。
    pub fn check_coverage(&self) -> Result<(), C98Code> {
        if self.scanned >= self.expected_pairs {
            Ok(())
        } else {
            Err(C98Code::COVER_SHORT)
        }
    }

    pub fn failures(&self) -> u32 {
        self.failures
    }

    pub fn scanned(&self) -> usize {
        self.scanned
    }
}

// ── 色弱模拟 ──────────────────────────────────────────────────────

/// 色弱类型。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ColorBlindness {
    /// 红色盲。
    Protanopia,
    /// 绿色盲。
    Deuteranopia,
    /// 蓝色盲。
    Tritanopia,
}

impl ColorBlindness {
    pub fn name(self) -> &'static str {
        match self {
            Self::Protanopia => "红色盲",
            Self::Deuteranopia => "绿色盲",
            Self::Tritanopia => "蓝色盲",
        }
    }

    pub fn all() -> [ColorBlindness; 3] {
        [Self::Protanopia, Self::Deuteranopia, Self::Tritanopia]
    }
}

/// 色弱可辨性检查器。
pub struct ColorBlindSim {
    /// 不可辨视图数。
    undistinguishable: u32,
}

impl ColorBlindSim {
    pub fn new() -> Self {
        Self { undistinguishable: 0 }
    }

    /// 模拟某色弱下两色是否可辨（简化：亮度差阈值）。
    pub fn check_pair(&mut self, _cb: ColorBlindness, lum_a: u16, lum_b: u16) -> Result<(), C98Code> {
        let diff = if lum_a > lum_b { lum_a - lum_b } else { lum_b - lum_a };
        if diff < 500 {
            self.undistinguishable = self.undistinguishable.saturating_add(1);
            return Err(C98Code::COLORBLIND_FAIL);
        }
        Ok(())
    }

    pub fn undistinguishable(&self) -> u32 {
        self.undistinguishable
    }
}

impl Default for ColorBlindSim {
    fn default() -> Self {
        Self::new()
    }
}

// ── 焦点可见 ──────────────────────────────────────────────────────

/// 焦点环检查器。
pub struct FocusChecker {
    missing: u32,
}

impl FocusChecker {
    pub fn new() -> Self {
        Self { missing: 0 }
    }

    /// 检查焦点环存在且可见（宽度>0，对比>阈值）。
    pub fn check(&mut self, ring_width: u16, ring_contrast_bp: u16) -> Result<(), C98Code> {
        if ring_width == 0 || ring_contrast_bp < 3000 {
            self.missing = self.missing.saturating_add(1);
            return Err(C98Code::FOCUS_MISSING);
        }
        Ok(())
    }

    pub fn missing(&self) -> u32 {
        self.missing
    }
}

impl Default for FocusChecker {
    fn default() -> Self {
        Self::new()
    }
}

// ── 判据 ──────────────────────────────────────────────────────────

pub fn run_ves104_checks() -> CheckSet {
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves104_a11yscan");
    {
        // 01 阻断级容许即 P0
        let mut r = A11yReport::new();
        r.add(A11yIssue::new(Severity::Blocker, 1));
        let gate = r.gate_release();
        chk(
            &mut set,
            "C398-01",
            gate.err() == Some(C98Code::BLOCKER_TOLERATED) && r.blockers_tolerated() == 1,
            "阻断级问题容许→P0（不可辨主题不可发布）",
        );
    }
    {
        // 02 严重级不阻断发布
        let mut r = A11yReport::new();
        r.add(A11yIssue::new(Severity::Serious, 2));
        chk(&mut set, "C398-02", r.gate_release().is_ok(), "严重级不阻断发布");
    }
    {
        // 03 分级计数
        let mut r = A11yReport::new();
        r.add(A11yIssue::new(Severity::Blocker, 1));
        r.add(A11yIssue::new(Severity::Serious, 2));
        r.add(A11yIssue::new(Severity::Serious, 3));
        r.add(A11yIssue::new(Severity::Suggestion, 4));
        chk(
            &mut set,
            "C398-03",
            r.count_by(Severity::Blocker) == 1 && r.count_by(Severity::Serious) == 2 && r.total() == 4,
            "三级分级计数正确",
        );
    }
    {
        // 04 对比度不足被拒
        let mut sc = ContrastScanner::new(3);
        let p = ColorPair::new(1, 2);
        let r = sc.scan_pair(p, 2000); // < 4500
        chk(
            &mut set,
            "C398-04",
            r.err() == Some(C98Code::CONTRAST_FAIL) && sc.failures() == 1,
            "对比度 <4.5:1 即不足",
        );
    }
    {
        // 05 对比度合规
        let mut sc = ContrastScanner::new(3);
        let p = ColorPair::new(1, 2);
        chk(&mut set, "C398-05", sc.scan_pair(p, 7000).is_ok(), "对比度 ≥4.5:1 通过");
    }
    {
        // 06 全对覆盖不足
        let sc = ContrastScanner::new(3); // 期望 6 对
        chk(
            &mut set,
            "C398-06",
            sc.check_coverage().err() == Some(C98Code::COVER_SHORT),
            "0/6 对→覆盖不足",
        );
    }
    {
        // 07 全对覆盖达标
        let mut sc = ContrastScanner::new(2); // 期望 2 对
        assert!(sc.scan_pair(ColorPair::new(1, 2), 5000).is_ok());
        assert!(sc.scan_pair(ColorPair::new(2, 1), 5000).is_ok());
        chk(&mut set, "C398-07", sc.check_coverage().is_ok(), "2/2 对覆盖达标");
    }
    {
        // 08 色弱不可辨被拒
        let mut sim = ColorBlindSim::new();
        let r = sim.check_pair(ColorBlindness::Protanopia, 5000, 5200); // 差 200
        chk(
            &mut set,
            "C398-08",
            r.err() == Some(C98Code::COLORBLIND_FAIL),
            "色弱下亮度差<500 即不可辨",
        );
    }
    {
        // 09 色弱可辨通过
        let mut sim = ColorBlindSim::new();
        let r = sim.check_pair(ColorBlindness::Deuteranopia, 2000, 8000);
        chk(&mut set, "C398-09", r.is_ok(), "亮度差 6000 可辨");
    }
    {
        // 10 焦点环缺失
        let mut fc = FocusChecker::new();
        let r = fc.check(0, 5000); // 宽度 0
        chk(
            &mut set,
            "C398-10",
            r.err() == Some(C98Code::FOCUS_MISSING) && fc.missing() == 1,
            "焦点环宽度 0 即缺失",
        );
    }
    {
        // 11 焦点环合规
        let mut fc = FocusChecker::new();
        chk(&mut set, "C398-11", fc.check(2, 5000).is_ok(), "2px 宽度+5:1 对比合规");
    }
    {
        // 12 三级全量
        chk(
            &mut set,
            "C398-12",
            Severity::Blocker.blocks_release() && !Severity::Serious.blocks_release() && !Severity::Suggestion.blocks_release(),
            "仅阻断级发布阻断",
        );
    }
    set
}
