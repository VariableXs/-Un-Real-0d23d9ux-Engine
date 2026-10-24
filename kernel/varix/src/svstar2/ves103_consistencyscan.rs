// SPDX-License-Identifier: Apache-2.0
// Varix Kernel — SVSTAR2 · ves103_consistencyscan
// VE-F3703 · 主题一致性扫描（R06 组 · 主题质量与一致性组）
//
// 锚点：VE-F3703
// 判据原文：语义唯一、魔法数扫描、离散度门、白名单误报、判据。
//
// 难点分析：
//   1. 同语义色一致是红线——错误色全主题唯一，同语义多色=混乱（语义唯一断言）。
//   2. 魔法数=不专业——偏离令牌系统的 magic number 必须检出。
//   3. 误报处理走白名单——合理例外不能阻塞整个扫描流水线。

#![allow(dead_code)]

use alloc::vec;
extern crate alloc;

use alloc::vec::Vec;
use alloc::string::{String, ToString};

use crate::checks::CheckSet;

// ── 诊断码（0x9700 起）────────────────────────────────────────────

/// 诊断码段基址。
pub const SEG_BASE: u16 = 0x9700;

/// 诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct C97Code(pub u16);

impl C97Code {
    pub const SEMANTIC_MULTI: Self = Self(SEG_BASE);   // 同语义多色 P1
    pub const MAGIC_FOUND: Self = Self(SEG_BASE + 1);  // 魔法数检出
    pub const DISCRETE_SPREAD: Self = Self(SEG_BASE + 2); // 离散超标
    pub const WHITELISTED: Self = Self(SEG_BASE + 3);  // 白名单放行
    pub const CONSISTENT: Self = Self(SEG_BASE + 4);   // 一致性通过
    pub const SEMANTIC_UNIQUE: Self = Self(SEG_BASE + 5); // 语义唯一合规
    pub const TOKEN_ALIGNED: Self = Self(SEG_BASE + 6); // 令牌对齐

    pub fn code(self) -> u16 {
        self.0
    }

    pub fn explain(self) -> &'static str {
        match self {
            Self::SEMANTIC_MULTI => "同语义多色——语义一致红线 P1",
            Self::MAGIC_FOUND => "魔法数检出——魔法数红线",
            Self::DISCRETE_SPREAD => "间距令牌离散超标——建议收敛",
            Self::WHITELISTED => "白名单放行——误报容差",
            Self::CONSISTENT => "一致性扫描通过",
            Self::SEMANTIC_UNIQUE => "语义色唯一合规",
            Self::TOKEN_ALIGNED => "令牌系统对齐",
            _ => "未知诊断",
        }
    }
}

// ── 语义色唯一 ────────────────────────────────────────────────────

/// 语义色角色。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ColorRole {
    /// 错误色。
    Error,
    /// 成功色。
    Success,
    /// 警告色。
    Warning,
    /// 信息色。
    Info,
}

impl ColorRole {
    pub fn name(self) -> &'static str {
        match self {
            Self::Error => "错误色",
            Self::Success => "成功色",
            Self::Warning => "警告色",
            Self::Info => "信息色",
        }
    }

    pub fn all() -> [ColorRole; 4] {
        [Self::Error, Self::Success, Self::Warning, Self::Info]
    }
}

/// 语义色登记表。
pub struct SemanticPalette {
    /// role → 色值列表（应恰好 1 个）。
    entries: Vec<(ColorRole, u16)>,
    multi_hits: u32,
}

impl SemanticPalette {
    pub fn new() -> Self {
        Self { entries: Vec::new(), multi_hits: 0 }
    }

    /// 登记用色。
    pub fn use_color(&mut self, role: ColorRole, color: u16) {
        self.entries.push((role, color));
    }

    /// 语义唯一断言：每个角色的所有用色必须同一色值。
    pub fn assert_unique(&mut self) -> Result<(), C97Code> {
        self.multi_hits = 0;
        for role in ColorRole::all() {
            let mut colors: Vec<u16> = Vec::new();
            for &(r, c) in &self.entries {
                if r == role {
                    if !colors.contains(&c) {
                        colors.push(c);
                    }
                }
            }
            if colors.len() > 1 {
                self.multi_hits = self.multi_hits.saturating_add(1);
            }
        }
        if self.multi_hits > 0 {
            return Err(C97Code::SEMANTIC_MULTI);
        }
        Ok(())
    }

    /// 某角色使用的色值数。
    pub fn distinct_colors(&self, role: ColorRole) -> usize {
        let mut colors: Vec<u16> = Vec::new();
        for &(r, c) in &self.entries {
            if r == role && !colors.contains(&c) {
                colors.push(c);
            }
        }
        colors.len()
    }

    pub fn multi_hits(&self) -> u32 {
        self.multi_hits
    }
}

impl Default for SemanticPalette {
    fn default() -> Self {
        Self::new()
    }
}

// ── 魔法数扫描 ────────────────────────────────────────────────────

/// 间距令牌值（离散阶）。
pub const SPACING_TOKENS: [u16; 6] = [2, 4, 8, 12, 16, 24];

/// 魔法数扫描器。
pub struct MagicScanner {
    /// 白名单值（合理例外）。
    whitelist: Vec<u16>,
    /// 检出数。
    found: u32,
}

impl MagicScanner {
    pub fn new() -> Self {
        Self { whitelist: Vec::new(), found: 0 }
    }

    /// 加白名单。
    pub fn whitelist(&mut self, v: u16) {
        if !self.whitelist.contains(&v) {
            self.whitelist.push(v);
        }
    }

    /// 扫描一个间距值：非令牌且非白名单即魔法数。
    pub fn scan(&mut self, v: u16) -> Result<(), C97Code> {
        if SPACING_TOKENS.contains(&v) {
            return Ok(());
        }
        if self.whitelist.contains(&v) {
            return Err(C97Code::WHITELISTED); // 白名单但仍是检出（标记为容差）
        }
        self.found = self.found.saturating_add(1);
        Err(C97Code::MAGIC_FOUND)
    }

    pub fn found(&self) -> u32 {
        self.found
    }
}

impl Default for MagicScanner {
    fn default() -> Self {
        Self::new()
    }
}

// ── 离散度门 ──────────────────────────────────────────────────────

/// 间距离散度检测。
pub struct DiscreteGate {
    /// 允许最大唯一间距数。
    pub max_distinct: usize,
}

impl DiscreteGate {
    pub fn standard() -> Self {
        Self { max_distinct: 4 }
    }

    /// 检查间距使用离散度：唯一值数超标即建议收敛。
    pub fn check(&self, values: &[u16]) -> Result<(), C97Code> {
        let mut distinct: Vec<u16> = Vec::new();
        for &v in values {
            if !distinct.contains(&v) {
                distinct.push(v);
            }
        }
        if distinct.len() > self.max_distinct {
            return Err(C97Code::DISCRETE_SPREAD);
        }
        Ok(())
    }
}

// ── 一致性扫描器 ──────────────────────────────────────────────────

/// 一致性扫描报告。
pub struct ConsistencyReport {
    /// 语义违规数。
    pub semantic_violations: u32,
    /// 魔法数数。
    pub magic_count: u32,
    /// 离散超标。
    pub discrete_spread: bool,
}

impl ConsistencyReport {
    pub fn is_clean(&self) -> bool {
        self.semantic_violations == 0 && self.magic_count == 0 && !self.discrete_spread
    }
}

/// 一致性扫描器。
pub struct ConsistencyScanner {
    pub palette: SemanticPalette,
    pub magic: MagicScanner,
    pub discrete: DiscreteGate,
}

impl ConsistencyScanner {
    pub fn new() -> Self {
        Self {
            palette: SemanticPalette::new(),
            magic: MagicScanner::new(),
            discrete: DiscreteGate::standard(),
        }
    }

    /// 全量扫描。
    pub fn scan(&mut self, spacing_values: &[u16]) -> ConsistencyReport {
        let sem = self.palette.assert_unique().is_err();
        // 魔法数扫描已在单点进行，这里只聚合
        let magic_count = self.magic.found();
        let disc = self.discrete.check(spacing_values).is_err();
        ConsistencyReport {
            semantic_violations: self.palette.multi_hits(),
            magic_count,
            discrete_spread: disc,
        }
        .tap_sem(sem)
    }
}

// 辅助：把语义结果灌进报告
trait TapSem {
    fn tap_sem(self, sem: bool) -> Self;
}

impl TapSem for ConsistencyReport {
    fn tap_sem(mut self, sem: bool) -> Self {
        let _ = sem;
        self
    }
}

impl Default for ConsistencyScanner {
    fn default() -> Self {
        Self::new()
    }
}

// ── 判据 ──────────────────────────────────────────────────────────

pub fn run_ves103_checks() -> CheckSet {
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves103_consistencyscan");
    {
        // 01 语义唯一合规
        let mut p = SemanticPalette::new();
        p.use_color(ColorRole::Error, 0xFF00);
        p.use_color(ColorRole::Error, 0xFF00);
        let r = p.assert_unique();
        chk(&mut set, "C397-01", r.is_ok(), "同语义同色值合规");
    }
    {
        // 02 同语义多色被拒
        let mut p = SemanticPalette::new();
        p.use_color(ColorRole::Error, 0xFF00);
        p.use_color(ColorRole::Error, 0xCC00); // 第二色
        let r = p.assert_unique();
        chk(
            &mut set,
            "C397-02",
            r.err() == Some(C97Code::SEMANTIC_MULTI) && p.multi_hits() == 1,
            "错误色全主题唯一——多色即混乱 P1",
        );
    }
    {
        // 03 各角色独立
        let mut p = SemanticPalette::new();
        p.use_color(ColorRole::Error, 0xFF00);
        p.use_color(ColorRole::Success, 0x00FF);
        p.use_color(ColorRole::Warning, 0xFF80);
        chk(&mut set, "C397-03", p.assert_unique().is_ok(), "不同语义不同色不冲突");
    }
    {
        // 04 魔法数检出
        let mut ms = MagicScanner::new();
        let r = ms.scan(7); // 7 不在 [2,4,8,12,16,24]
        chk(
            &mut set,
            "C397-04",
            r.err() == Some(C97Code::MAGIC_FOUND) && ms.found() == 1,
            "偏离令牌的间距=魔法数",
        );
    }
    {
        // 05 令牌值放行
        let mut ms = MagicScanner::new();
        let r = ms.scan(8); // 8 是令牌
        chk(&mut set, "C397-05", r.is_ok(), "令牌值合规放行");
    }
    {
        // 06 白名单容差
        let mut ms = MagicScanner::new();
        ms.whitelist(7);
        let r = ms.scan(7);
        chk(
            &mut set,
            "C397-06",
            r.err() == Some(C97Code::WHITELISTED) && ms.found() == 0,
            "白名单值不计魔法数（误报容差）",
        );
    }
    {
        // 07 离散度合规
        let g = DiscreteGate::standard();
        let vals = alloc::vec![4u16, 4, 8, 8, 12, 4];
        chk(&mut set, "C397-07", g.check(&vals).is_ok(), "3 种唯一间距≤4 门槛");
    }
    {
        // 08 离散超标
        let g = DiscreteGate::standard();
        let vals = alloc::vec![2u16, 3, 5, 7, 11, 13]; // 6 种唯一
        chk(
            &mut set,
            "C397-08",
            g.check(&vals).err() == Some(C97Code::DISCRETE_SPREAD),
            "6 种唯一间距>4 门槛→离散超标",
        );
    }
    {
        // 09 扫描报告干净
        let mut sc = ConsistencyScanner::new();
        sc.palette.use_color(ColorRole::Error, 1);
        sc.palette.use_color(ColorRole::Success, 2);
        let report = sc.scan(&[4, 8, 4, 8]);
        chk(&mut set, "C397-09", report.is_clean(), "全绿报告");
    }
    {
        // 10 扫描报告检出语义
        let mut sc = ConsistencyScanner::new();
        sc.palette.use_color(ColorRole::Error, 1);
        sc.palette.use_color(ColorRole::Error, 2);
        let report = sc.scan(&[4, 8]);
        chk(
            &mut set,
            "C397-10",
            report.semantic_violations == 1 && !report.is_clean(),
            "语义违规进报告",
        );
    }
    {
        // 11 空调色板不误报
        let mut p = SemanticPalette::new();
        chk(&mut set, "C397-11", p.assert_unique().is_ok(), "空色板语义唯一合规");
    }
    {
        // 12 去重唯一色计数
        let mut p = SemanticPalette::new();
        p.use_color(ColorRole::Info, 10);
        p.use_color(ColorRole::Info, 10);
        p.use_color(ColorRole::Info, 10);
        chk(&mut set, "C397-12", p.distinct_colors(ColorRole::Info) == 1, "3 次同色计 1 种唯一");
    }
    set
}
