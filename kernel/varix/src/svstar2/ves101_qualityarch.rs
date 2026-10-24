// SPDX-License-Identifier: Apache-2.0
// Varix Kernel — SVSTAR2 · ves101_qualityarch
// VE-F3701 · 主题质量总纲（R06 组 · 主题质量与一致性组）
//
// 锚点：VE-F3701
// 判据原文：五维模型、主客分流、透明断言、改进导向、判据。
//
// 难点分析：
//   1. 美观维是主观维——完全量化主观=伪科学，必须主客分流：客观代理指标+人工评审。
//   2. 改进导向红线：扫描目的是改进不是打分，只罚不改=质量体系失败。
//   3. 评分规则全公开——黑箱评分即 P1 红线。

#![allow(dead_code)]

extern crate alloc;

use alloc::vec;
use alloc::vec::Vec;
use alloc::string::{String, ToString};

use crate::checks::CheckSet;

// ── 诊断码（0x9500 起）────────────────────────────────────────────

/// 诊断码段基址。
pub const SEG_BASE: u16 = 0x9500;

/// 诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct C95Code(pub u16);

impl C95Code {
    pub const SUBJECTIVE_OVER_QUANT: Self = Self(SEG_BASE);   // 主观全量化
    pub const RULE_BLACKBOX: Self = Self(SEG_BASE + 1);       // 规则黑箱 P1
    pub const PENALTY_ONLY: Self = Self(SEG_BASE + 2);        // 罚而不导
    pub const DIM_MISSING: Self = Self(SEG_BASE + 3);         // 维度缺失
    pub const FIVE_DIM_OK: Self = Self(SEG_BASE + 4);         // 五维齐备
    pub const SPLIT_OK: Self = Self(SEG_BASE + 5);            // 主客分流合规
    pub const GUIDE_OK: Self = Self(SEG_BASE + 6);            // 改进导向合规

    pub fn code(self) -> u16 {
        self.0
    }

    pub fn explain(self) -> &'static str {
        match self {
            Self::SUBJECTIVE_OVER_QUANT => "主观维全量化——伪科学红线，须走人工评审",
            Self::RULE_BLACKBOX => "评分规则黑箱——透明红线 P1",
            Self::PENALTY_ONLY => "罚而不导——改进导向红线",
            Self::DIM_MISSING => "质量维度缺失——五维模型不完整",
            Self::FIVE_DIM_OK => "五维质量模型齐备",
            Self::SPLIT_OK => "主客分流合规",
            Self::GUIDE_OK => "改进导向合规",
            _ => "未知诊断",
        }
    }
}

// ── 五维质量模型 ──────────────────────────────────────────────────

/// 质量维度（五维全量）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum QualityDim {
    /// 一致性。
    Consistency,
    /// 无障碍。
    Accessibility,
    /// 性能。
    Performance,
    /// 兼容。
    Compatibility,
    /// 美观（主观维）。
    Aesthetics,
}

impl QualityDim {
    pub fn name(self) -> &'static str {
        match self {
            Self::Consistency => "一致性",
            Self::Accessibility => "无障碍",
            Self::Performance => "性能",
            Self::Compatibility => "兼容",
            Self::Aesthetics => "美观",
        }
    }

    /// 是否主观维。
    pub fn is_subjective(self) -> bool {
        matches!(self, Self::Aesthetics)
    }

    pub fn all() -> [QualityDim; 5] {
        [Self::Consistency, Self::Accessibility, Self::Performance, Self::Compatibility, Self::Aesthetics]
    }
}

/// 维度评分来源。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ScoreSource {
    /// 客观算法。
    Algorithm,
    /// 人工评审。
    Human,
    /// 代理指标（客观近似）。
    Proxy,
}

impl ScoreSource {
    /// 主观维必须 Human 或 Proxy，不得纯 Algorithm。
    pub fn acceptable_for(self, dim: QualityDim) -> bool {
        if dim.is_subjective() {
            matches!(self, Self::Human | Self::Proxy)
        } else {
            true
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Algorithm => "客观算法",
            Self::Human => "人工评审",
            Self::Proxy => "代理指标",
        }
    }
}

/// 五维模型。
pub struct QualityModel {
    /// 每维是否已定义。
    dims: [bool; 5],
    /// 每维评分来源。
    sources: [Option<ScoreSource>; 5],
}

impl QualityModel {
    pub fn new() -> Self {
        Self { dims: [false; 5], sources: [None; 5] }
    }

    fn idx(d: QualityDim) -> usize {
        match d {
            QualityDim::Consistency => 0,
            QualityDim::Accessibility => 1,
            QualityDim::Performance => 2,
            QualityDim::Compatibility => 3,
            QualityDim::Aesthetics => 4,
        }
    }

    /// 定义维度。
    pub fn define(&mut self, d: QualityDim, src: ScoreSource) -> Result<(), C95Code> {
        if !src.acceptable_for(d) {
            return Err(C95Code::SUBJECTIVE_OVER_QUANT);
        }
        let i = Self::idx(d);
        self.dims[i] = true;
        self.sources[i] = Some(src);
        Ok(())
    }

    /// 五维齐备？
    pub fn all_defined(&self) -> bool {
        self.dims.iter().all(|&b| b)
    }

    /// 缺维数。
    pub fn missing_count(&self) -> usize {
        self.dims.iter().filter(|&&b| !b).count()
    }

    /// 完整性检查。
    pub fn check_complete(&self) -> Result<(), C95Code> {
        if self.all_defined() {
            Ok(())
        } else {
            Err(C95Code::DIM_MISSING)
        }
    }

    /// 主客分流检查：美观维来源不得是纯算法。
    pub fn check_split(&self) -> Result<(), C95Code> {
        let i = Self::idx(QualityDim::Aesthetics);
        match self.sources[i] {
            Some(src) if src.acceptable_for(QualityDim::Aesthetics) => Ok(()),
            Some(_) => Err(C95Code::SUBJECTIVE_OVER_QUANT),
            None => Err(C95Code::DIM_MISSING),
        }
    }
}

impl Default for QualityModel {
    fn default() -> Self {
        Self::new()
    }
}

// ── 透明断言 ──────────────────────────────────────────────────────

/// 评分规则条目。
pub struct ScoringRule {
    /// 规则 ID。
    pub id: u16,
    /// 权重（基点）。
    pub weight_bp: u16,
    /// 是否公开。
    pub published: bool,
}

impl ScoringRule {
    pub fn new(id: u16, weight_bp: u16, published: bool) -> Self {
        Self { id, weight_bp, published }
    }
}

/// 规则透明审计器。
pub struct TransparencyAuditor {
    rules: Vec<ScoringRule>,
    blackbox_hits: u32,
}

impl TransparencyAuditor {
    pub fn new() -> Self {
        Self { rules: Vec::new(), blackbox_hits: 0 }
    }

    pub fn add(&mut self, r: ScoringRule) {
        self.rules.push(r);
    }

    /// 全量审计：所有规则必须公开。未公开即黑箱 P1。
    pub fn audit(&mut self) -> Result<(), C95Code> {
        self.blackbox_hits = 0;
        for r in &self.rules {
            if !r.published {
                self.blackbox_hits = self.blackbox_hits.saturating_add(1);
            }
        }
        if self.blackbox_hits > 0 {
            return Err(C95Code::RULE_BLACKBOX);
        }
        Ok(())
    }

    pub fn blackbox_hits(&self) -> u32 {
        self.blackbox_hits
    }

    /// 权重总和是否 10000。
    pub fn weights_balanced(&self) -> bool {
        let sum: u32 = self.rules.iter().map(|r| r.weight_bp as u32).sum();
        sum == 10000
    }
}

impl Default for TransparencyAuditor {
    fn default() -> Self {
        Self::new()
    }
}

// ── 改进导向 ──────────────────────────────────────────────────────

/// 处置动作类型。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DispositionKind {
    /// 仅惩罚。
    PenaltyOnly,
    /// 惩罚+改进建议。
    PenaltyWithGuide,
    /// 仅改进建议。
    GuideOnly,
}

impl DispositionKind {
    /// 是否合规（必须含改进导向）。
    pub fn compliant(self) -> bool {
        matches!(self, Self::PenaltyWithGuide | Self::GuideOnly)
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::PenaltyOnly => "仅惩罚",
            Self::PenaltyWithGuide => "惩罚+改进",
            Self::GuideOnly => "仅改进",
        }
    }
}

/// 改进导向门。
pub struct GuidanceGate {
    /// 罚而不导次数。
    penalty_only_hits: u32,
}

impl GuidanceGate {
    pub fn new() -> Self {
        Self { penalty_only_hits: 0 }
    }

    /// 审查处置动作。
    pub fn review(&mut self, d: DispositionKind) -> Result<(), C95Code> {
        if !d.compliant() {
            self.penalty_only_hits = self.penalty_only_hits.saturating_add(1);
            return Err(C95Code::PENALTY_ONLY);
        }
        Ok(())
    }

    pub fn penalty_only_hits(&self) -> u32 {
        self.penalty_only_hits
    }
}

impl Default for GuidanceGate {
    fn default() -> Self {
        Self::new()
    }
}

// ── 质量总纲 ──────────────────────────────────────────────────────

/// R06 质量总纲。
pub struct QualityArch {
    pub model: QualityModel,
    pub transparency: TransparencyAuditor,
    pub guidance: GuidanceGate,
}

impl QualityArch {
    pub fn new() -> Self {
        Self {
            model: QualityModel::new(),
            transparency: TransparencyAuditor::new(),
            guidance: GuidanceGate::new(),
        }
    }

    /// 全绿：五维齐备+规则全公开+零罚而不导。
    pub fn all_green(&self) -> bool {
        self.model.all_defined()
            && self.transparency.blackbox_hits() == 0
            && self.guidance.penalty_only_hits() == 0
    }
}

impl Default for QualityArch {
    fn default() -> Self {
        Self::new()
    }
}

// ── 判据 ──────────────────────────────────────────────────────────

pub fn run_ves101_checks() -> CheckSet {
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves101_qualityarch");
    {
        // 01 五维全量
        chk(&mut set, "C395-01", QualityDim::all().len() == 5, "一致性/无障碍/性能/兼容/美观五维");
    }
    {
        // 02 主观维拒绝纯算法
        let mut m = QualityModel::new();
        let r = m.define(QualityDim::Aesthetics, ScoreSource::Algorithm);
        chk(
            &mut set,
            "C395-02",
            r.err() == Some(C95Code::SUBJECTIVE_OVER_QUANT),
            "美观维纯算法=主观全量化红线",
        );
    }
    {
        // 03 主观维代理指标可
        let mut m = QualityModel::new();
        let r = m.define(QualityDim::Aesthetics, ScoreSource::Proxy);
        chk(&mut set, "C395-03", r.is_ok(), "主观维代理指标合规（主客分流）");
    }
    {
        // 04 主观维人工评审可
        let mut m = QualityModel::new();
        let r = m.define(QualityDim::Aesthetics, ScoreSource::Human);
        chk(&mut set, "C395-04", r.is_ok(), "主观维人工评审合规");
    }
    {
        // 05 五维齐备检查
        let mut m = QualityModel::new();
        for d in QualityDim::all() {
            let src = if d.is_subjective() { ScoreSource::Proxy } else { ScoreSource::Algorithm };
            assert!(m.define(d, src).is_ok());
        }
        chk(&mut set, "C395-05", m.all_defined() && m.check_complete().is_ok(), "五维齐备");
    }
    {
        // 06 缺维被拒
        let mut m = QualityModel::new();
        m.define(QualityDim::Consistency, ScoreSource::Algorithm).unwrap();
        let r = m.check_complete();
        chk(
            &mut set,
            "C395-06",
            r.err() == Some(C95Code::DIM_MISSING) && m.missing_count() == 4,
            "缺 4 维即 DIM_MISSING",
        );
    }
    {
        // 07 规则黑箱 P1
        let mut ta = TransparencyAuditor::new();
        ta.add(ScoringRule::new(1, 5000, true));
        ta.add(ScoringRule::new(2, 5000, false)); // 未公开
        let r = ta.audit();
        chk(
            &mut set,
            "C395-07",
            r.err() == Some(C95Code::RULE_BLACKBOX) && ta.blackbox_hits() == 1,
            "规则未公开即黑箱 P1（透明红线）",
        );
    }
    {
        // 08 全公开合规
        let mut ta = TransparencyAuditor::new();
        ta.add(ScoringRule::new(1, 5000, true));
        ta.add(ScoringRule::new(2, 5000, true));
        let r = ta.audit();
        chk(&mut set, "C395-08", r.is_ok() && ta.weights_balanced(), "全公开+权重平衡");
    }
    {
        // 09 罚而不导被拒
        let mut g = GuidanceGate::new();
        let r = g.review(DispositionKind::PenaltyOnly);
        chk(
            &mut set,
            "C395-09",
            r.err() == Some(C95Code::PENALTY_ONLY) && g.penalty_only_hits() == 1,
            "仅惩罚不给建议=改进导向红线",
        );
    }
    {
        // 10 惩罚+改进合规
        let mut g = GuidanceGate::new();
        let r = g.review(DispositionKind::PenaltyWithGuide);
        chk(&mut set, "C395-10", r.is_ok(), "惩罚+改进建议合规");
    }
    {
        // 11 仅改进也合规
        let mut g = GuidanceGate::new();
        let r = g.review(DispositionKind::GuideOnly);
        chk(&mut set, "C395-11", r.is_ok(), "纯改进导向合规");
    }
    {
        // 12 总纲全绿
        let mut qa = QualityArch::new();
        for d in QualityDim::all() {
            let src = if d.is_subjective() { ScoreSource::Proxy } else { ScoreSource::Algorithm };
            assert!(qa.model.define(d, src).is_ok());
        }
        qa.transparency.add(ScoringRule::new(1, 10000, true));
        assert!(qa.transparency.audit().is_ok());
        assert!(qa.guidance.review(DispositionKind::GuideOnly).is_ok());
        chk(&mut set, "C395-12", qa.all_green(), "五维+透明+导向全绿");
    }
    set
}
