//! VE-F3826 · 对比度实时引擎
//!
//! 运行时对比度动态保障：增量计算（变更时算）、动态修复（描边/提亮/加影）。
//! 还原断言 P1：自动修复必须可一键还原。意图保持红线：自动修复不改设计意图。
//! 实时监控：对比度跌破门→告警+修复。

#![allow(dead_code)]

use alloc::vec;
extern crate alloc;

use alloc::vec::Vec;

// ── 诊断码段（0xAExx）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0xAE00;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CAECode(pub u16);

impl CAECode {
    pub const OK: Self = Self(SEG_BASE);
    pub const IRREVERSIBLE: Self = Self(SEG_BASE + 1);
    pub const INTENT_BROKEN: Self = Self(SEG_BASE + 2);
    pub const FULL_RECALC: Self = Self(SEG_BASE + 3);
    pub const ALERT_MISSING: Self = Self(SEG_BASE + 4);

    pub fn explain(self) -> &'static str {
        match self {
            Self::OK => "正常",
            Self::IRREVERSIBLE => "修复不可还原（P1 红线实测）",
            Self::INTENT_BROKEN => "修复破坏设计意图（红线实测）",
            Self::FULL_RECALC => "全量计算，需增量修正",
            Self::ALERT_MISSING => "告警缺失，需补",
            _ => "未知",
        }
    }
}

// ── 数据结构 ────────────────────────────────────────────────────────
/// 修复策略
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FixStrategy {
    Outline,
    Brighten,
    Shadow,
}

/// 修复方式表（公开）
#[derive(Debug, Clone)]
pub struct FixMethodTable {
    pub published: bool,
    pub strategies: Vec<FixStrategy>,
}

impl FixMethodTable {
    pub fn standard() -> Self {
        Self { published: true, strategies: alloc::vec![FixStrategy::Outline, FixStrategy::Brighten, FixStrategy::Shadow] }
    }

    pub fn assert_public(&self) -> Result<(), CAECode> {
        if self.published {
            Ok(())
        } else {
            Err(CAECode::INTENT_BROKEN)
        }
    }
}

/// 修复记录（含还原信息）
#[derive(Debug, Clone)]
pub struct FixRecord {
    pub id: u32,
    pub strategy: FixStrategy,
    pub original: u32,
    pub fixed: u32,
    pub reversible: bool,
    pub intent_preserved: bool,
}

/// 对比度对
#[derive(Debug, Clone, Copy)]
pub struct ContrastPair {
    pub id: u32,
    pub ratio: u16,
    pub threshold: u16,
}

// ── 主逻辑 ──────────────────────────────────────────────────────────
pub struct ContrastEngine {
    table: FixMethodTable,
    fixes: Vec<FixRecord>,
    alerts: Vec<u32>,
}

impl ContrastEngine {
    pub fn new(table: FixMethodTable) -> Self {
        Self { table, fixes: Vec::new(), alerts: Vec::new() }
    }

    /// 增量计算：仅变更对
    pub fn compute_delta(&self, changed: &[ContrastPair]) -> u32 {
        changed.iter().filter(|p| p.ratio < p.threshold).count() as u32
    }

    /// 自动修复
    pub fn auto_fix(&mut self, pair: &ContrastPair) -> Result<u32, CAECode> {
        self.table.assert_public()?;
        if pair.ratio >= pair.threshold {
            return Ok(pair.id);
        }
        let strategy = self.table.strategies.first().copied().ok_or(CAECode::INTENT_BROKEN)?;
        let rec = FixRecord {
            id: pair.id,
            strategy,
            original: pair.ratio as u32,
            fixed: (pair.threshold + 10) as u32,
            reversible: true,
            intent_preserved: true,
        };
        self.fixes.push(rec);
        Ok(pair.id)
    }

    /// 还原断言 P1
    pub fn assert_reversible(&self) -> Result<(), CAECode> {
        if self.fixes.iter().all(|f| f.reversible) {
            Ok(())
        } else {
            Err(CAECode::IRREVERSIBLE)
        }
    }

    /// 意图保持红线
    pub fn assert_intent(&self) -> Result<(), CAECode> {
        if self.fixes.iter().all(|f| f.intent_preserved) {
            Ok(())
        } else {
            Err(CAECode::INTENT_BROKEN)
        }
    }

    /// 告警
    pub fn alert(&mut self, pair_id: u32) {
        self.alerts.push(pair_id);
    }

    pub fn assert_alerted(&self, pair_id: u32) -> Result<(), CAECode> {
        if self.alerts.contains(&pair_id) {
            Ok(())
        } else {
            Err(CAECode::ALERT_MISSING)
        }
    }

    /// 一键还原
    pub fn revert(&mut self, id: u32) -> bool {
        if let Some(pos) = self.fixes.iter().position(|f| f.id == id && f.reversible) {
            self.fixes.remove(pos);
            true
        } else {
            false
        }
    }

    pub fn fix_count(&self) -> usize {
        self.fixes.len()
    }
}

// ── 判据 ────────────────────────────────────────────────────────────
pub fn run_ves126_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves126_contrast");

    // 01 还原断言 P1
    {
        let mut e = ContrastEngine::new(FixMethodTable::standard());
        e.fixes.push(FixRecord { id: 1, strategy: FixStrategy::Outline, original: 100, fixed: 500, reversible: false, intent_preserved: true });
        chk(&mut set, "C3826-01", e.assert_reversible() == Err(CAECode::IRREVERSIBLE), "err");
    }
    // 02 意图保持
    {
        let mut e = ContrastEngine::new(FixMethodTable::standard());
        e.fixes.push(FixRecord { id: 1, strategy: FixStrategy::Brighten, original: 100, fixed: 500, reversible: true, intent_preserved: false });
        chk(&mut set, "C3826-02", e.assert_intent() == Err(CAECode::INTENT_BROKEN), "err");
    }
    // 03 自动修复
    {
        let mut e = ContrastEngine::new(FixMethodTable::standard());
        let pair = ContrastPair { id: 1, ratio: 100, threshold: 450 };
        chk(&mut set, "C3826-03", e.auto_fix(&pair).is_ok() && e.fix_count() == 1, "err");
    }
    // 04 达标不修
    {
        let mut e = ContrastEngine::new(FixMethodTable::standard());
        let pair = ContrastPair { id: 1, ratio: 700, threshold: 450 };
        e.auto_fix(&pair).unwrap();
        chk(&mut set, "C3826-04", e.fix_count() == 0, "err");
    }
    // 05 还原
    {
        let mut e = ContrastEngine::new(FixMethodTable::standard());
        let pair = ContrastPair { id: 1, ratio: 100, threshold: 450 };
        e.auto_fix(&pair).unwrap();
        chk(&mut set, "C3826-05", e.revert(1) && e.fix_count() == 0, "err");
    }
    // 06 告警
    {
        let mut e = ContrastEngine::new(FixMethodTable::standard());
        e.alert(42);
        chk(&mut set, "C3826-06", e.assert_alerted(42).is_ok(), "err");
    }
    // 07 告警缺失
    {
        let e = ContrastEngine::new(FixMethodTable::standard());
        chk(&mut set, "C3826-07", e.assert_alerted(99) == Err(CAECode::ALERT_MISSING), "err");
    }
    // 08 增量计算
    {
        let e = ContrastEngine::new(FixMethodTable::standard());
        let pairs = [
            ContrastPair { id: 1, ratio: 100, threshold: 450 },
            ContrastPair { id: 2, ratio: 700, threshold: 450 },
        ];
        chk(&mut set, "C3826-08", e.compute_delta(&pairs) == 1, "err");
    }
    // 09 修复方式表公开
    {
        let t = FixMethodTable::standard();
        chk(&mut set, "C3826-09", t.assert_public().is_ok(), "err");
    }
    // 10 解释非空
    {
        chk(&mut set, "C3826-10", !CAECode::IRREVERSIBLE.explain().is_empty(), "err");
    }

    set
}
