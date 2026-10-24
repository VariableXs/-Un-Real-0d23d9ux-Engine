// SPDX-License-Identifier: Apache-2.0
// Varix Kernel — SVSTAR2 · ves99_ecodocs
// VE-F3699 · 生态文档（R05 组 · 生态运营组）
//
// 锚点：VE-F3699
// 判据原文：三 SOP、规则公开、义务前置、2 跳复用、家族管道、判据。
//
// 难点分析：
//   1. 三 SOP（活动/处罚/结算）必须可执行——SOP 不可执行=流程空文，演练即验。
//   2. 规则暗改是治理红线——规则公开册变更走 ADR，暗改即立案。
//   3. 创作者手册义务前置：义务条款必须排在权利条款之前，不能只写权利不写义务。

#![allow(dead_code)]

extern crate alloc;

use alloc::vec;
use alloc::vec::Vec;
use alloc::string::{String, ToString};

use crate::checks::CheckSet;

// ── 诊断码（0x9300 起）────────────────────────────────────────────

/// 诊断码段基址。
pub const SEG_BASE: u16 = 0x9300;

/// 诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct C93Code(pub u16);

impl C93Code {
    pub const SOP_UNRUNNABLE: Self = Self(SEG_BASE);   // SOP 不可执行
    pub const RULE_TAMPER: Self = Self(SEG_BASE + 1);  // 规则暗改
    pub const OBLIGATION_LATE: Self = Self(SEG_BASE + 2); // 义务未前置
    pub const NAV_TOO_DEEP: Self = Self(SEG_BASE + 3); // 超 2 跳
    pub const DRIFT_DETECTED: Self = Self(SEG_BASE + 4); // 文档漂移
    pub const SOP_DRILLED: Self = Self(SEG_BASE + 5);  // SOP 演练通过
    pub const RULE_PUBLIC: Self = Self(SEG_BASE + 6);  // 规则公开合规

    pub fn code(self) -> u16 {
        self.0
    }

    pub fn explain(self) -> &'static str {
        match self {
            Self::SOP_UNRUNNABLE => "SOP 不可执行——演练修正",
            Self::RULE_TAMPER => "规则暗改检出——ADR 立案",
            Self::OBLIGATION_LATE => "义务条款未前置——义务前置红线",
            Self::NAV_TOO_DEEP => "导航超 2 跳——2 跳红线",
            Self::DRIFT_DETECTED => "文档漂移——对账修正",
            Self::SOP_DRILLED => "SOP 演练通过",
            Self::RULE_PUBLIC => "规则公开册合规",
            _ => "未知诊断",
        }
    }
}

// ── SOP ───────────────────────────────────────────────────────────

/// SOP 类别（三 SOP 全量）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SopKind {
    /// 活动 SOP。
    Activity,
    /// 处罚 SOP。
    Penalty,
    /// 结算 SOP。
    Settle,
}

impl SopKind {
    pub fn name(self) -> &'static str {
        match self {
            Self::Activity => "活动",
            Self::Penalty => "处罚",
            Self::Settle => "结算",
        }
    }

    pub fn all() -> [SopKind; 3] {
        [Self::Activity, Self::Penalty, Self::Settle]
    }
}

/// SOP 步骤。
pub struct SopStep {
    /// 步骤号。
    pub no: u16,
    /// 是否可执行（有明确动作+判据）。
    pub runnable: bool,
}

impl SopStep {
    pub fn new(no: u16, runnable: bool) -> Self {
        Self { no, runnable }
    }
}

/// SOP 文档。
pub struct SopDoc {
    pub kind: SopKind,
    pub steps: Vec<SopStep>,
    /// 演练是否通过。
    drilled: bool,
}

impl SopDoc {
    pub fn new(kind: SopKind) -> Self {
        Self { kind, steps: Vec::new(), drilled: false }
    }

    /// 追加步骤。
    pub fn push(&mut self, step: SopStep) {
        self.steps.push(step);
    }

    /// 演练：步骤号连续 + 每步可执行。
    pub fn drill(&mut self) -> Result<(), C93Code> {
        if self.steps.is_empty() {
            return Err(C93Code::SOP_UNRUNNABLE);
        }
        for (i, s) in self.steps.iter().enumerate() {
            if !s.runnable || s.no != (i as u16 + 1) {
                return Err(C93Code::SOP_UNRUNNABLE);
            }
        }
        self.drilled = true;
        Ok(())
    }

    pub fn is_drilled(&self) -> bool {
        self.drilled
    }

    pub fn step_count(&self) -> usize {
        self.steps.len()
    }
}

// ── 规则公开册 ────────────────────────────────────────────────────

/// 规则版本记录。
pub struct RuleVersion {
    pub version: u32,
    /// 变更摘要指纹。
    pub digest: u64,
    /// 是否经 ADR（Architecture Decision Record）。
    pub via_adr: bool,
}

impl RuleVersion {
    pub fn new(version: u32, digest: u64, via_adr: bool) -> Self {
        Self { version, digest, via_adr }
    }
}

/// 规则公开册——暗改检测。
pub struct RuleBook {
    versions: Vec<RuleVersion>,
    tamper_hits: u32,
}

impl RuleBook {
    pub fn new() -> Self {
        Self { versions: Vec::new(), tamper_hits: 0 }
    }

    /// 首版登记。
    pub fn bootstrap(&mut self, version: u32, digest: u64) {
        self.versions.push(RuleVersion::new(version, digest, true));
    }

    /// 发布新版本。必须经 ADR，否则暗改。
    pub fn publish(&mut self, version: u32, digest: u64, via_adr: bool) -> Result<(), C93Code> {
        if !via_adr {
            self.tamper_hits = self.tamper_hits.saturating_add(1);
            return Err(C93Code::RULE_TAMPER);
        }
        self.versions.push(RuleVersion::new(version, digest, true));
        Ok(())
    }

    /// 当前版本数。
    pub fn version_count(&self) -> usize {
        self.versions.len()
    }

    pub fn tamper_hits(&self) -> u32 {
        self.tamper_hits
    }
}

impl Default for RuleBook {
    fn default() -> Self {
        Self::new()
    }
}

// ── 创作者手册 ────────────────────────────────────────────────────

/// 手册条目类型。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ClauseKind {
    /// 义务条款。
    Obligation,
    /// 权利条款。
    Right,
}

impl ClauseKind {
    pub fn name(self) -> &'static str {
        match self {
            Self::Obligation => "义务",
            Self::Right => "权利",
        }
    }
}

/// 手册条目。
pub struct ManualClause {
    pub kind: ClauseKind,
    pub no: u16,
}

impl ManualClause {
    pub fn new(kind: ClauseKind, no: u16) -> Self {
        Self { kind, no }
    }
}

/// 创作者手册——义务前置校验。
pub struct CreatorManual {
    clauses: Vec<ManualClause>,
}

impl CreatorManual {
    pub fn new() -> Self {
        Self { clauses: Vec::new() }
    }

    /// 追加条目。
    pub fn push(&mut self, c: ManualClause) {
        self.clauses.push(c);
    }

    /// 义务前置：第一条必须是义务，且第一个权利之前至少有一条义务。
    pub fn assert_obligation_first(&self) -> Result<(), C93Code> {
        if self.clauses.is_empty() {
            return Err(C93Code::OBLIGATION_LATE);
        }
        if self.clauses[0].kind != ClauseKind::Obligation {
            return Err(C93Code::OBLIGATION_LATE);
        }
        let mut seen_obligation = false;
        for c in &self.clauses {
            match c.kind {
                ClauseKind::Obligation => seen_obligation = true,
                ClauseKind::Right => {
                    if !seen_obligation {
                        return Err(C93Code::OBLIGATION_LATE);
                    }
                }
            }
        }
        Ok(())
    }

    pub fn clause_count(&self) -> usize {
        self.clauses.len()
    }
}

impl Default for CreatorManual {
    fn default() -> Self {
        Self::new()
    }
}

// ── 2 跳导航 ──────────────────────────────────────────────────────

/// 导航链深度检查器。
pub struct NavDepthGuard {
    /// 允许最大跳数。
    pub max_hops: usize,
}

impl NavDepthGuard {
    pub fn standard() -> Self {
        Self { max_hops: 2 }
    }

    /// 检查导航链深度（节点数-1=跳数）。
    pub fn check(&self, chain_len: usize) -> Result<(), C93Code> {
        let hops = chain_len.saturating_sub(1);
        if hops > self.max_hops {
            return Err(C93Code::NAV_TOO_DEEP);
        }
        Ok(())
    }
}

// ── 文档对账 ──────────────────────────────────────────────────────

/// 文档对账器——清单与实现的漂移检测。
pub struct DocReconciler {
    /// 清单声明的条目指纹。
    declared: Vec<u64>,
    /// 实际存在的条目指纹。
    actual: Vec<u64>,
    drift_hits: u32,
}

impl DocReconciler {
    pub fn new() -> Self {
        Self { declared: Vec::new(), actual: Vec::new(), drift_hits: 0 }
    }

    /// 登记声明。
    pub fn declare(&mut self, fp: u64) {
        self.declared.push(fp);
    }

    /// 登记实际。
    pub fn observe(&mut self, fp: u64) {
        self.actual.push(fp);
    }

    /// 对账：声明集 == 实际集。
    pub fn reconcile(&mut self) -> Result<(), C93Code> {
        let mut d = self.declared.clone();
        let mut a = self.actual.clone();
        d.sort_unstable();
        a.sort_unstable();
        if d == a {
            Ok(())
        } else {
            self.drift_hits = self.drift_hits.saturating_add(1);
            Err(C93Code::DRIFT_DETECTED)
        }
    }

    pub fn drift_hits(&self) -> u32 {
        self.drift_hits
    }
}

impl Default for DocReconciler {
    fn default() -> Self {
        Self::new()
    }
}

// ── 判据 ──────────────────────────────────────────────────────────

pub fn run_ves99_checks() -> CheckSet {
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves99_ecodocs");
    {
        // 01 三 SOP 全量
        chk(&mut set, "C393-01", SopKind::all().len() == 3, "活动/处罚/结算三 SOP");
    }
    {
        // 02 SOP 演练通过
        let mut d = SopDoc::new(SopKind::Activity);
        d.push(SopStep::new(1, true));
        d.push(SopStep::new(2, true));
        let r = d.drill();
        chk(&mut set, "C393-02", r.is_ok() && d.is_drilled(), "步骤连续可执行→演练通过");
    }
    {
        // 03 不可执行 SOP 被拒
        let mut d = SopDoc::new(SopKind::Penalty);
        d.push(SopStep::new(1, true));
        d.push(SopStep::new(2, false)); // 不可执行
        let r = d.drill();
        chk(
            &mut set,
            "C393-03",
            r.err() == Some(C93Code::SOP_UNRUNNABLE),
            "含不可执行步骤即 SOP_UNRUNNABLE",
        );
    }
    {
        // 04 步骤号跳号被拒
        let mut d = SopDoc::new(SopKind::Settle);
        d.push(SopStep::new(1, true));
        d.push(SopStep::new(3, true)); // 跳号
        let r = d.drill();
        chk(&mut set, "C393-04", r.err() == Some(C93Code::SOP_UNRUNNABLE), "步骤号跳号即不可执行");
    }
    {
        // 05 规则暗改检出
        let mut rb = RuleBook::new();
        rb.bootstrap(1, 0xAA);
        let r = rb.publish(2, 0xBB, false); // 无 ADR
        chk(
            &mut set,
            "C393-05",
            r.err() == Some(C93Code::RULE_TAMPER) && rb.tamper_hits() == 1,
            "规则变更不经 ADR 即暗改（规则公开红线）",
        );
    }
    {
        // 06 ADR 变更合规
        let mut rb = RuleBook::new();
        rb.bootstrap(1, 0xAA);
        let r = rb.publish(2, 0xBB, true);
        chk(&mut set, "C393-06", r.is_ok() && rb.version_count() == 2, "ADR 变更正常登记");
    }
    {
        // 07 义务前置合规
        let mut m = CreatorManual::new();
        m.push(ManualClause::new(ClauseKind::Obligation, 1));
        m.push(ManualClause::new(ClauseKind::Obligation, 2));
        m.push(ManualClause::new(ClauseKind::Right, 3));
        chk(&mut set, "C393-07", m.assert_obligation_first().is_ok(), "义务在前权利在后→合规");
    }
    {
        // 08 权利开头被拒
        let mut m = CreatorManual::new();
        m.push(ManualClause::new(ClauseKind::Right, 1));
        let r = m.assert_obligation_first();
        chk(
            &mut set,
            "C393-08",
            r.err() == Some(C93Code::OBLIGATION_LATE),
            "手册第一条不是义务→义务前置红线",
        );
    }
    {
        // 09 2 跳导航合规
        let g = NavDepthGuard::standard();
        chk(
            &mut set,
            "C393-09",
            g.check(3).is_ok() && g.check(1).is_ok(),
            "≤2 跳（3 节点）导航合规",
        );
    }
    {
        // 10 超 2 跳拒绝
        let g = NavDepthGuard::standard();
        chk(
            &mut set,
            "C393-10",
            g.check(4).err() == Some(C93Code::NAV_TOO_DEEP),
            "4 节点=3 跳超 2 跳红线",
        );
    }
    {
        // 11 对账一致
        let mut dr = DocReconciler::new();
        dr.declare(1);
        dr.declare(2);
        dr.observe(1);
        dr.observe(2);
        chk(&mut set, "C393-11", dr.reconcile().is_ok(), "声明集==实际集对账通过");
    }
    {
        // 12 漂移检出
        let mut dr = DocReconciler::new();
        dr.declare(1);
        dr.declare(2);
        dr.observe(1); // 缺 2
        let r = dr.reconcile();
        chk(
            &mut set,
            "C393-12",
            r.err() == Some(C93Code::DRIFT_DETECTED) && dr.drift_hits() == 1,
            "文档漂移检出→对账修正",
        );
    }
    set
}
