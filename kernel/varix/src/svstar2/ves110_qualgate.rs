// SPDX-License-Identifier: Apache-2.0
// Varix Kernel — SVSTAR2 · ves110_qualgate
// VE-F3709 · 质量门禁（R06 组 · 主题质量与一致性组）
//
// 锚点：VE-F3709
// 判据原文：质量门禁——阻断/放行/条件放行三级门、门禁规则公开、门禁全覆盖。
//
// 难点分析：
//   1. 质量门禁是发布最后防线：阻断级问题存在时不得发布。
//   2. 条件放行（warning→可带条件发）必须显式记录条件，暗门=红线旁路。
//   3. 门禁规则必须公开——黑箱门禁=不可信。

#![allow(dead_code)]

use alloc::vec;
extern crate alloc;

use alloc::vec::Vec;
use alloc::string::{String, ToString};

use crate::checks::CheckSet;

// ── 诊断码（0x9E00 起）────────────────────────────────────────────

/// 诊断码段基址。
pub const SEG_BASE: u16 = 0x9E00;

/// 诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct C9ECode(pub u16);

impl C9ECode {
    pub const BLOCKED: Self = Self(SEG_BASE);          // 阻断
    pub const CONDITION_REQUIRED: Self = Self(SEG_BASE + 1); // 需条件
    pub const RULE_BLACKBOX: Self = Self(SEG_BASE + 2); // 规则黑箱
    pub const RELEASE_OK: Self = Self(SEG_BASE + 3);   // 放行
    pub const CONDITIONAL_OK: Self = Self(SEG_BASE + 4); // 条件放行
    pub const RULE_PUBLIC: Self = Self(SEG_BASE + 5);  // 规则公开
    pub const GATE_COMPLETE: Self = Self(SEG_BASE + 6); // 门禁齐备

    pub fn code(self) -> u16 {
        self.0
    }

    pub fn explain(self) -> &'static str {
        match self {
            Self::BLOCKED => "质量门禁阻断——不得发布",
            Self::CONDITION_REQUIRED => "需条件放行——显式记录条件",
            Self::RULE_BLACKBOX => "门禁规则黑箱——不可信",
            Self::RELEASE_OK => "质量门禁放行",
            Self::CONDITIONAL_OK => "条件放行合规",
            Self::RULE_PUBLIC => "门禁规则公开",
            Self::GATE_COMPLETE => "门禁全覆盖",
            _ => "未知诊断",
        }
    }
}

// ── 门禁决策 ──────────────────────────────────────────────────────

/// 门禁决策。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GateDecision {
    /// 阻断发布。
    Block,
    /// 条件放行（须显式条件）。
    Conditional,
    /// 无条件放行。
    Pass,
}

impl GateDecision {
    pub fn name(self) -> &'static str {
        match self {
            Self::Block => "阻断",
            Self::Conditional => "条件放行",
            Self::Pass => "放行",
        }
    }
}

/// 门禁规则条目。
pub struct GateRule {
    /// 规则 ID。
    pub id: u16,
    /// 是否公开。
    pub published: bool,
    /// 阻断阈值（基点）。
    pub threshold_bp: u16,
}

impl GateRule {
    pub fn new(id: u16, threshold_bp: u16, published: bool) -> Self {
        Self { id, threshold_bp, published }
    }
}

// ── 质量门禁 ──────────────────────────────────────────────────────

/// 质量门禁。
pub struct QualGate {
    /// 门禁规则。
    rules: Vec<GateRule>,
    /// 条件放行记录：(rule_id, condition)。
    conditionals: Vec<(u16, &'static str)>,
}

impl QualGate {
    pub fn new() -> Self {
        Self { rules: Vec::new(), conditionals: Vec::new() }
    }

    /// 加规则。
    pub fn add_rule(&mut self, r: GateRule) {
        self.rules.push(r);
    }

    /// 规则公开审计。
    pub fn audit_transparency(&self) -> Result<(), C9ECode> {
        for r in &self.rules {
            if !r.published {
                return Err(C9ECode::RULE_BLACKBOX);
            }
        }
        Ok(())
    }

    /// 评估一个指标值 vs 规则。低于阈值→阻断。
    pub fn evaluate(&mut self, rule_id: u16, actual_bp: u16) -> Result<GateDecision, C9ECode> {
        for r in &self.rules {
            if r.id == rule_id {
                if actual_bp >= r.threshold_bp {
                    return Ok(GateDecision::Pass);
                }
                return Err(C9ECode::BLOCKED);
            }
        }
        // 未知规则：条件放行须显式条件
        Err(C9ECode::CONDITION_REQUIRED)
    }

    /// 条件放行登记。
    pub fn conditional_pass(&mut self, rule_id: u16, condition: &'static str) -> Result<(), C9ECode> {
        if condition.is_empty() {
            return Err(C9ECode::CONDITION_REQUIRED);
        }
        self.conditionals.push((rule_id, condition));
        Ok(())
    }

    /// 全规则已公开？
    pub fn all_published(&self) -> bool {
        self.rules.iter().all(|r| r.published)
    }

    pub fn rule_count(&self) -> usize {
        self.rules.len()
    }

    pub fn conditional_count(&self) -> usize {
        self.conditionals.len()
    }
}

impl Default for QualGate {
    fn default() -> Self {
        Self::new()
    }
}

// ── 门禁覆盖 ──────────────────────────────────────────────────────

/// 门禁覆盖矩阵（维度×规则）。
pub struct GateCoverage {
    /// 每维是否有对应规则。
    covered: Vec<bool>,
}

impl GateCoverage {
    pub fn new(dim_count: usize) -> Self {
        Self { covered: alloc::vec![false; dim_count] }
    }

    /// 标记维度被门禁覆盖。
    pub fn cover(&mut self, dim_idx: usize) {
        if dim_idx < self.covered.len() {
            self.covered[dim_idx] = true;
        }
    }

    /// 全覆盖？
    pub fn is_full(&self) -> bool {
        self.covered.iter().all(|&b| b)
    }

    /// 缺口数。
    pub fn gaps(&self) -> usize {
        self.covered.iter().filter(|&&b| !b).count()
    }
}

// ── 判据 ──────────────────────────────────────────────────────────

pub fn run_ves110_checks() -> CheckSet {
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves110_qualgate");
    {
        // 01 规则公开合规
        let mut g = QualGate::new();
        g.add_rule(GateRule::new(1, 5000, true));
        chk(&mut set, "C39E-01", g.audit_transparency().is_ok(), "规则公开审计通过");
    }
    {
        // 02 规则黑箱被拒
        let mut g = QualGate::new();
        g.add_rule(GateRule::new(1, 5000, false));
        chk(
            &mut set,
            "C39E-02",
            g.audit_transparency().err() == Some(C9ECode::RULE_BLACKBOX),
            "未公开规则=黑箱门禁",
        );
    }
    {
        // 03 达标放行
        let mut g = QualGate::new();
        g.add_rule(GateRule::new(1, 5000, true));
        let d = g.evaluate(1, 8000);
        chk(&mut set, "C39E-03", d == Ok(GateDecision::Pass), "8000≥5000 达标放行");
    }
    {
        // 04 不达标阻断
        let mut g = QualGate::new();
        g.add_rule(GateRule::new(1, 5000, true));
        let d = g.evaluate(1, 3000);
        chk(
            &mut set,
            "C39E-04",
            d.err() == Some(C9ECode::BLOCKED),
            "3000<5000 阻断发布",
        );
    }
    {
        // 05 未知规则需条件
        let mut g = QualGate::new();
        let d = g.evaluate(99, 8000);
        chk(
            &mut set,
            "C39E-05",
            d.err() == Some(C9ECode::CONDITION_REQUIRED),
            "未知规则→条件放行须显式条件",
        );
    }
    {
        // 06 条件放行合规
        let mut g = QualGate::new();
        let r = g.conditional_pass(1, "外观问题不阻断功能");
        chk(
            &mut set,
            "C39E-06",
            r.is_ok() && g.conditional_count() == 1,
            "显式条件放行合规",
        );
    }
    {
        // 07 空条件被拒
        let mut g = QualGate::new();
        let r = g.conditional_pass(1, "");
        chk(
            &mut set,
            "C39E-07",
            r.err() == Some(C9ECode::CONDITION_REQUIRED),
            "空条件=暗门→拒绝",
        );
    }
    {
        // 08 全规则公开判定
        let mut g = QualGate::new();
        g.add_rule(GateRule::new(1, 5000, true));
        g.add_rule(GateRule::new(2, 3000, true));
        chk(&mut set, "C39E-08", g.all_published() && g.rule_count() == 2, "全规则公开");
    }
    {
        // 09 门禁覆盖缺口
        let gc = GateCoverage::new(3);
        chk(&mut set, "C39E-09", gc.gaps() == 3 && !gc.is_full(), "3 维全缺");
    }
    {
        // 10 门禁全覆盖
        let mut gc = GateCoverage::new(3);
        gc.cover(0);
        gc.cover(1);
        gc.cover(2);
        chk(&mut set, "C39E-10", gc.is_full() && gc.gaps() == 0, "3 维全覆盖");
    }
    {
        // 11 边界值恰好等于阈值
        let mut g = QualGate::new();
        g.add_rule(GateRule::new(1, 5000, true));
        let d = g.evaluate(1, 5000);
        chk(&mut set, "C39E-11", d == Ok(GateDecision::Pass), "恰好阈值放行");
    }
    {
        // 12 空门禁无规则
        let g = QualGate::new();
        chk(&mut set, "C39E-12", g.all_published() && g.rule_count() == 0, "空门禁无黑箱");
    }
    set
}
