//! VE-F3827 · 文字对比强化
//!
//! 低对比文字强化四策略：描边/加影/提亮/底衬。字形保真红线（轮廓偏移≤1px）。
//! 可读性验证复用对比度门。批量渲染预算分项。

#![allow(dead_code)]

use alloc::vec;
extern crate alloc;

use alloc::vec::Vec;

// ── 诊断码段（0xAFxx）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0xAF00;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CAFCode(pub u16);

impl CAFCode {
    pub const OK: Self = Self(SEG_BASE);
    pub const GLYPH_BROKEN: Self = Self(SEG_BASE + 1);
    pub const BOOST_INEFFECTIVE: Self = Self(SEG_BASE + 2);
    pub const STRATEGY_HIDDEN: Self = Self(SEG_BASE + 3);
    pub const BUDGET_EXCEEDED: Self = Self(SEG_BASE + 4);

    pub fn explain(self) -> &'static str {
        match self {
            Self::OK => "正常",
            Self::GLYPH_BROKEN => "字形破坏（P1 红线实测）：轮廓差分 >1px",
            Self::BOOST_INEFFECTIVE => "强化无效，策略修正",
            Self::STRATEGY_HIDDEN => "策略黑箱，需公开",
            Self::BUDGET_EXCEEDED => "超预算，需分项",
            _ => "未知",
        }
    }
}

// ── 数据结构 ────────────────────────────────────────────────────────
/// 强化策略
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoostStrategy {
    Outline,
    Shadow,
    Brighten,
    Backdrop,
}

impl BoostStrategy {
    pub const ALL: [BoostStrategy; 4] = [
        Self::Outline, Self::Shadow, Self::Brighten, Self::Backdrop,
    ];
}

/// 策略选择表（公开）
#[derive(Debug, Clone)]
pub struct StrategyTable {
    pub published: bool,
    pub mapping: Vec<(u8, BoostStrategy)>,
}

impl StrategyTable {
    pub fn standard() -> Self {
        Self {
            published: true,
            mapping: alloc::vec![
                (0, BoostStrategy::Outline),
                (1, BoostStrategy::Shadow),
                (2, BoostStrategy::Brighten),
                (3, BoostStrategy::Backdrop),
            ],
        }
    }

    pub fn select(&self, bg_complexity: u8) -> Option<BoostStrategy> {
        self.mapping.iter().find(|(c, _)| *c == bg_complexity).map(|(_, s)| *s)
    }

    pub fn assert_public(&self) -> Result<(), CAFCode> {
        if self.published {
            Ok(())
        } else {
            Err(CAFCode::STRATEGY_HIDDEN)
        }
    }
}

/// 强化结果
#[derive(Debug, Clone, Copy)]
pub struct BoostResult {
    pub strategy: BoostStrategy,
    pub glyph_offset: u16,
    pub readability_gain: i16,
    pub batch_ms: u16,
}

// ── 主逻辑 ──────────────────────────────────────────────────────────
pub struct TextBoost {
    table: StrategyTable,
    results: Vec<BoostResult>,
    budget_ms: u16,
}

impl TextBoost {
    pub fn new(table: StrategyTable, budget_ms: u16) -> Self {
        Self { table, results: Vec::new(), budget_ms }
    }

    /// 字形保真红线 P1：轮廓偏移 ≤1px
    pub fn assert_glyph_fidelity(&self) -> Result<(), CAFCode> {
        if self.results.iter().any(|r| r.glyph_offset > 1) {
            Err(CAFCode::GLYPH_BROKEN)
        } else {
            Ok(())
        }
    }

    /// 可读性提升断言
    pub fn assert_readability(&self) -> Result<(), CAFCode> {
        if self.results.iter().any(|r| r.readability_gain <= 0) {
            Err(CAFCode::BOOST_INEFFECTIVE)
        } else {
            Ok(())
        }
    }

    /// 批量分项预算
    pub fn assert_budget(&self) -> Result<(), CAFCode> {
        let total: u32 = self.results.iter().map(|r| r.batch_ms as u32).sum();
        if total > self.budget_ms as u32 {
            Err(CAFCode::BUDGET_EXCEEDED)
        } else {
            Ok(())
        }
    }

    pub fn apply(&mut self, bg_complexity: u8, glyph_offset: u16, gain: i16, ms: u16) -> Result<(), CAFCode> {
        self.table.assert_public()?;
        let strategy = self.table.select(bg_complexity).ok_or(CAFCode::STRATEGY_HIDDEN)?;
        self.results.push(BoostResult {
            strategy,
            glyph_offset,
            readability_gain: gain,
            batch_ms: ms,
        });
        Ok(())
    }

    /// 四策略覆盖
    pub fn assert_four_strategies(&self) -> Result<(), CAFCode> {
        for s in BoostStrategy::ALL {
            if !self.results.iter().any(|r| r.strategy == s) {
                return Err(CAFCode::BOOST_INEFFECTIVE);
            }
        }
        Ok(())
    }

    pub fn result_count(&self) -> usize {
        self.results.len()
    }
}

// ── 判据 ────────────────────────────────────────────────────────────
pub fn run_ves127_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves127_textboost");

    // 01 字形破坏 P1
    {
        let mut t = TextBoost::new(StrategyTable::standard(), 100);
        t.apply(0, 3, 10, 5).unwrap();
        chk(&mut set, "C3827-01", t.assert_glyph_fidelity() == Err(CAFCode::GLYPH_BROKEN), "err");
    }
    // 02 字形保真
    {
        let mut t = TextBoost::new(StrategyTable::standard(), 100);
        t.apply(0, 1, 10, 5).unwrap();
        chk(&mut set, "C3827-02", t.assert_glyph_fidelity().is_ok(), "err");
    }
    // 03 强化无效
    {
        let mut t = TextBoost::new(StrategyTable::standard(), 100);
        t.apply(0, 1, -5, 5).unwrap();
        chk(&mut set, "C3827-03", t.assert_readability() == Err(CAFCode::BOOST_INEFFECTIVE), "err");
    }
    // 04 可读提升
    {
        let mut t = TextBoost::new(StrategyTable::standard(), 100);
        t.apply(0, 1, 20, 5).unwrap();
        chk(&mut set, "C3827-04", t.assert_readability().is_ok(), "err");
    }
    // 05 超预算
    {
        let mut t = TextBoost::new(StrategyTable::standard(), 10);
        t.apply(0, 1, 10, 15).unwrap();
        chk(&mut set, "C3827-05", t.assert_budget() == Err(CAFCode::BUDGET_EXCEEDED), "err");
    }
    // 06 预算内
    {
        let mut t = TextBoost::new(StrategyTable::standard(), 100);
        t.apply(0, 1, 10, 5).unwrap();
        chk(&mut set, "C3827-06", t.assert_budget().is_ok(), "err");
    }
    // 07 四策略覆盖
    {
        let mut t = TextBoost::new(StrategyTable::standard(), 100);
        t.apply(0, 1, 10, 1).unwrap();
        t.apply(1, 1, 10, 1).unwrap();
        t.apply(2, 1, 10, 1).unwrap();
        t.apply(3, 1, 10, 1).unwrap();
        chk(&mut set, "C3827-07", t.assert_four_strategies().is_ok(), "err");
    }
    // 08 策略选择
    {
        let table = StrategyTable::standard();
        chk(&mut set, "C3827-08", table.select(2) == Some(BoostStrategy::Brighten), "err");
    }
    // 09 策略公开
    {
        let table = StrategyTable::standard();
        chk(&mut set, "C3827-09", table.assert_public().is_ok(), "err");
    }
    // 10 解释非空
    {
        chk(&mut set, "C3827-10", !CAFCode::GLYPH_BROKEN.explain().is_empty(), "err");
    }

    set
}
