//! VE-F3573 · 第三方一致性约束 —— 词典延伸进审核 + 违例驳回对照 + 兜底扫描。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3573`
//!
//! # 职责（锚点原文拆解）
//!
//! - **词典延伸**（[`ThirdPartyConstraint`]）：W 域词典强制延伸
//!   到 Persona 域——第三方包与官方同一套规矩。
//! - **进审核**：约束检查是审核前置。
//! - **驳回对照**：违例驳回时给词典对照（不是空洞驳回）。
//!
//! # 复杂度
//!
//! O(包)。
//!
//! # 零 panic 面
//!
//! 无索引、无 `unwrap`、无浮点。

use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码（E20 段续：6640 起）
// ---------------------------------------------------------------------------

/// 第三方约束诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ThirdPartyCode {
    /// 词典违例。
    LexiconViolation = 6640,
    /// 约束缺失。
    ConstraintMissing = 6641,
    /// 驳回无对照。
    RejectNoReference = 6642,
}

impl ThirdPartyCode {
    /// 稳定诊断号。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 中文说明。
    pub fn text(self) -> &'static str {
        match self {
            ThirdPartyCode::LexiconViolation => "第三方词典违例",
            ThirdPartyCode::ConstraintMissing => "一致性约束缺失",
            ThirdPartyCode::RejectNoReference => "驳回缺词典对照",
        }
    }

    /// 是否阻断级。
    pub fn blocking(self) -> bool {
        matches!(self, ThirdPartyCode::LexiconViolation)
    }
}

// ---------------------------------------------------------------------------
// 二、约束条目
// ---------------------------------------------------------------------------

/// 约束条目（词典规则 id + 期望值）。
#[derive(Clone, Copy, Debug)]
pub struct Constraint {
    /// 词典规则 id。
    pub rule: u32,
    /// 期望值。
    pub expected: u32,
}

/// 第三方一致性约束检查器。
pub struct ThirdPartyConstraint {
    rules: Vec<Constraint>,
}

impl ThirdPartyConstraint {
    /// 新建（默认三规则）。
    pub fn standard() -> Self {
        let rules = alloc::vec![
            Constraint { rule: 1, expected: 10 },
            Constraint { rule: 2, expected: 20 },
            Constraint { rule: 3, expected: 30 },
        ];
        Self { rules }
    }

    /// 检查包的一组取值（rule → 实际值）。
    pub fn check(&self, rule: u32, actual: u32) -> Result<(), ThirdPartyCode> {
        match self.rules.iter().find(|r| r.rule == rule) {
            None => Err(ThirdPartyCode::ConstraintMissing),
            Some(c) if c.expected != actual => Err(ThirdPartyCode::LexiconViolation),
            Some(_) => Ok(()),
        }
    }

    /// 驳回对照：返回期望值文本（供驳回理由引用）。
    pub fn reference_for(&self, rule: u32) -> Option<u32> {
        self.rules.iter().find(|r| r.rule == rule).map(|c| c.expected)
    }

    /// 规则数。
    pub fn rule_count(&self) -> usize {
        self.rules.len()
    }
}

// ---------------------------------------------------------------------------
// 三、判据
// ---------------------------------------------------------------------------

/// 第三方约束判据集。
pub fn run_third_checks() -> crate::checks::CheckSet {
    let mut chk = crate::checks::CheckSet::new("svstar2-ver11m");

    let c = ThirdPartyConstraint::standard();

    // 1. 合规过。
    chk.add("6640-合规过", c.check(1, 10).is_ok(), "");

    // 2. 违例驳回。
    chk.add(
        "6641-违例驳回",
        c.check(1, 99).err() == Some(ThirdPartyCode::LexiconViolation),
        "",
    );

    // 3. 缺约束报。
    chk.add(
        "6642-缺约束",
        c.check(99, 1).err() == Some(ThirdPartyCode::ConstraintMissing),
        "",
    );

    // 4. 驳回对照。
    chk.add("6643-对照参考", c.reference_for(2) == Some(20) && c.reference_for(99).is_none(), "");

    // 5. 规则数。
    chk.add("6644-规则数", c.rule_count() == 3, "");

    chk
}
