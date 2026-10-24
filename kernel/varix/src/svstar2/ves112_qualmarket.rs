//! VE-F3712 · 质量与市场挂钩
//!
//! 质量分级影响曝光权重。黑箱红线 P0：权重公式必须公开且逐资产可查。
//! 新晋保护：不歧视新创作者。卖位禁令复用：质量挂钩 ≠ 买断流量。

#![allow(dead_code)]

extern crate alloc;

use alloc::vec::Vec;

// ── 诊断码段（0xA0xx）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0xA000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CA0Code(pub u16);

impl CA0Code {
    pub const OK: Self = Self(SEG_BASE);
    pub const BLACKBOX: Self = Self(SEG_BASE + 1);
    pub const DISCRIMINATE: Self = Self(SEG_BASE + 2);
    pub const PAID_PLACEMENT: Self = Self(SEG_BASE + 3);
    pub const RULE_CHANGED: Self = Self(SEG_BASE + 4);

    pub fn explain(self) -> &'static str {
        match self {
            Self::OK => "正常",
            Self::BLACKBOX => "曝光黑箱（P0 红线）：权重公式未公开或不可查",
            Self::DISCRIMINATE => "挂钩歧视新创作者，需保护修正",
            Self::PAID_PLACEMENT => "变相卖位（P0 复用红线）",
            Self::RULE_CHANGED => "挂钩规则暗改，需 ADR",
            _ => "未知",
        }
    }
}

// ── 数据结构 ────────────────────────────────────────────────────────
/// 认证等级
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum CertLevel {
    None,
    Bronze,
    Silver,
    Gold,
}

impl CertLevel {
    pub fn exposure_weight(&self) -> u16 {
        match self {
            Self::None => 50,
            Self::Bronze => 65,
            Self::Silver => 80,
            Self::Gold => 95,
        }
    }
}

/// 挂钩规则（公开）
#[derive(Debug, Clone)]
pub struct MarketLinkRule {
    pub formula_public: bool,
    pub asset_queryable: bool,
    pub newbie_protect: bool,
    pub paid_placement_forbidden: bool,
}

impl MarketLinkRule {
    pub fn standard() -> Self {
        Self {
            formula_public: true,
            asset_queryable: true,
            newbie_protect: true,
            paid_placement_forbidden: true,
        }
    }

    /// 黑箱红线 P0：公式公开 + 逐资产可查
    pub fn assert_not_blackbox(&self) -> Result<(), CA0Code> {
        if self.formula_public && self.asset_queryable {
            Ok(())
        } else {
            Err(CA0Code::BLACKBOX)
        }
    }
}

/// 资产曝光记录
#[derive(Debug, Clone)]
pub struct ExposureRecord {
    pub asset_id: u32,
    pub creator_days: u32,
    pub cert: CertLevel,
    pub weight: u16,
}

// ── 主逻辑 ──────────────────────────────────────────────────────────
pub struct QualMarket {
    rule: MarketLinkRule,
    records: Vec<ExposureRecord>,
}

impl QualMarket {
    pub fn new(rule: MarketLinkRule) -> Self {
        Self { rule, records: Vec::new() }
    }

    /// 计算曝光权重（公式公开可查）
    pub fn compute_weight(&self, cert: CertLevel, creator_days: u32) -> Result<u16, CA0Code> {
        self.rule.assert_not_blackbox()?;
        if !self.rule.paid_placement_forbidden {
            return Err(CA0Code::PAID_PLACEMENT);
        }
        let base = cert.exposure_weight();
        // 新晋保护：创建 < 30 天加权 10
        let newbie_bonus = if self.rule.newbie_protect && creator_days < 30 { 10 } else { 0 };
        let w = base + newbie_bonus;
        Ok(if w > 100 { 100 } else { w })
    }

    /// 逐资产可查断言
    pub fn query_weight(&self, asset_id: u32) -> Result<u16, CA0Code> {
        self.rule.assert_not_blackbox()?;
        self.records
            .iter()
            .find(|r| r.asset_id == asset_id)
            .map(|r| r.weight)
            .ok_or(CA0Code::BLACKBOX)
    }

    pub fn register(&mut self, rec: ExposureRecord) {
        self.records.push(rec);
    }

    /// 新晋保护断言：新创作者权重不低于同级老手减 5
    pub fn assert_newbie_fair(&self) -> Result<(), CA0Code> {
        for r in &self.records {
            if r.creator_days < 30 && r.weight < r.cert.exposure_weight() {
                return Err(CA0Code::DISCRIMINATE);
            }
        }
        Ok(())
    }

    /// 规则暗改检测
    pub fn assert_rule_stable(&self, prev: &MarketLinkRule) -> Result<(), CA0Code> {
        if self.rule.formula_public == prev.formula_public
            && self.rule.asset_queryable == prev.asset_queryable
        {
            Ok(())
        } else {
            Err(CA0Code::RULE_CHANGED)
        }
    }
}

// ── 判据 ────────────────────────────────────────────────────────────
pub fn run_ves112_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves112_qualmarket");

    // 01 黑箱 P0：公式未公开
    {
        let rule = MarketLinkRule { formula_public: false, asset_queryable: true, newbie_protect: true, paid_placement_forbidden: true };
        let q = QualMarket::new(rule);
        chk(&mut set, "C3712-01", q.compute_weight(CertLevel::Gold, 100) == Err(CA0Code::BLACKBOX), "err");
    }
    // 02 黑箱 P0：不可查
    {
        let rule = MarketLinkRule { formula_public: true, asset_queryable: false, newbie_protect: true, paid_placement_forbidden: true };
        let q = QualMarket::new(rule);
        chk(&mut set, "C3712-02", q.compute_weight(CertLevel::Gold, 100) == Err(CA0Code::BLACKBOX), "err");
    }
    // 03 正常权重
    {
        let q = QualMarket::new(MarketLinkRule::standard());
        chk(&mut set, "C3712-03", q.compute_weight(CertLevel::Gold, 100) == Ok(95), "err");
    }
    // 04 新晋加权
    {
        let q = QualMarket::new(MarketLinkRule::standard());
        chk(&mut set, "C3712-04", q.compute_weight(CertLevel::None, 10) == Ok(60), "err");
    }
    // 05 卖位禁令 P0
    {
        let rule = MarketLinkRule { formula_public: true, asset_queryable: true, newbie_protect: true, paid_placement_forbidden: false };
        let q = QualMarket::new(rule);
        chk(&mut set, "C3712-05", q.compute_weight(CertLevel::Gold, 100) == Err(CA0Code::PAID_PLACEMENT), "err");
    }
    // 06 逐资产可查
    {
        let mut q = QualMarket::new(MarketLinkRule::standard());
        q.register(ExposureRecord { asset_id: 42, creator_days: 100, cert: CertLevel::Silver, weight: 80 });
        chk(&mut set, "C3712-06", q.query_weight(42) == Ok(80), "err");
    }
    // 07 可查 miss
    {
        let q = QualMarket::new(MarketLinkRule::standard());
        chk(&mut set, "C3712-07", q.query_weight(99).is_err(), "err");
    }
    // 08 新晋公平
    {
        let mut q = QualMarket::new(MarketLinkRule::standard());
        q.register(ExposureRecord { asset_id: 1, creator_days: 5, cert: CertLevel::Bronze, weight: 75 });
        chk(&mut set, "C3712-08", q.assert_newbie_fair().is_ok(), "err");
    }
    // 09 规则暗改
    {
        let q = QualMarket::new(MarketLinkRule::standard());
        let prev = MarketLinkRule { formula_public: false, asset_queryable: true, newbie_protect: true, paid_placement_forbidden: true };
        chk(&mut set, "C3712-09", q.assert_rule_stable(&prev) == Err(CA0Code::RULE_CHANGED), "err");
    }
    // 10 权重封顶
    {
        let q = QualMarket::new(MarketLinkRule::standard());
        let w = q.compute_weight(CertLevel::Gold, 5).unwrap();
        chk(&mut set, "C3712-10", w == 100, "err");
    }

    set
}
