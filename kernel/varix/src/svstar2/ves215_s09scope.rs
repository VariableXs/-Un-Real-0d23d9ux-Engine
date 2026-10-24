//! VE-F3961 · S09 开工与自查范围
//!
//! S 域自查组开工：O09/P09/Q09/R09 模板第八用声明（家族治理流程复用）。
//! 范围声明：S01-S08 八组 × 20 项 = 160 + 组间 28 对对账 + 七维汇总
//! （API/测试/性能/安全/无障碍/文档/预算）。三增维复用声明（工具无障碍/
//! 产出无障碍/心流——增维登记红线）。证据引用制：无障碍证据双源（自动化
//! 色标 + 用户测试录证，录证脱敏）。
//! 错误路径：证据缺→阻断+回补；过期→增量复查；增维无标准→先立标准。
//! 性能：计划 O(组×维)；核验 O(项)；引用 O(1)。
//! 跨批对接：八组档案；O09-P09-Q09-R09 模板单源；S10 衔接。无隐私面。

#![allow(dead_code)]
use alloc::vec;

extern crate alloc;

use alloc::vec::Vec;
use alloc::string::String;

// ── 诊断码段（0x2F40）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0x2F40;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct S9ACode(pub u16);

impl S9ACode {
    pub const OK: Self = Self(SEG_BASE);
    pub const EVIDENCE_MISSING: Self = Self(SEG_BASE + 1);
    pub const EVIDENCE_STALE: Self = Self(SEG_BASE + 2);
    pub const NOSTANDARD: Self = Self(SEG_BASE + 3);
    pub const SCOPE_OVERFLOW: Self = Self(SEG_BASE + 4);

    pub fn explain(self) -> &'static str {
        match self {
            Self::OK => "正常",
            Self::EVIDENCE_MISSING => "证据缺→阻断+回补",
            Self::EVIDENCE_STALE => "过期→增量复查",
            Self::NOSTANDARD => "增维无标准→先立标准",
            Self::SCOPE_OVERFLOW => "范围越界→拒收",
            _ => "未知",
        }
    }
}

// ── 家族模板第八用声明 ───────────────────────────────────────────────

/// 家族治理模板使用计数（O09/P09/Q09/R09 → S09 第八用）。
pub const FAMILY_TEMPLATE_USES: u8 = 8;

/// 模板用声明账。
#[derive(Debug, Clone)]
pub struct TemplateUses {
    pub declared: u8,
}

impl TemplateUses {
    pub fn new() -> Self {
        Self { declared: 0 }
    }

    pub fn declare_use(&mut self) -> u8 {
        self.declared += 1;
        self.declared
    }

    pub fn is_eighth(&self) -> bool {
        self.declared == FAMILY_TEMPLATE_USES
    }
}

impl Default for TemplateUses {
    fn default() -> Self {
        Self::new()
    }
}

// ── 自查范围声明（160 + 28 + 七维）───────────────────────────────────

pub const GROUP_COUNT: usize = 8;
pub const ITEMS_PER_GROUP: usize = 20;
pub const PAIR_COUNT: usize = 28;
pub const SEVEN_DIMS: [&str; 7] = ["API", "测试", "性能", "安全", "无障碍", "文档", "预算"];

/// 自查计划：组 × 维矩阵。
#[derive(Debug, Clone)]
pub struct SelfAuditPlan {
    /// 项完成状态：组×项。
    pub items: [[bool; ITEMS_PER_GROUP]; GROUP_COUNT],
    /// 28 对对账完成状态。
    pub pairs: [bool; PAIR_COUNT],
    /// 七维汇总完成状态。
    pub dims: [bool; 7],
}

impl SelfAuditPlan {
    pub fn new() -> Self {
        Self { items: [[false; ITEMS_PER_GROUP]; GROUP_COUNT], pairs: [false; PAIR_COUNT], dims: [false; 7] }
    }

    /// 登记项核验（组 1-8、项 1-20）。
    pub fn check_item(&mut self, group: usize, item: usize) -> S9ACode {
        if group < 1 || group > GROUP_COUNT || item < 1 || item > ITEMS_PER_GROUP {
            return S9ACode::SCOPE_OVERFLOW;
        }
        self.items[group - 1][item - 1] = true;
        S9ACode::OK
    }

    /// 核验总数。
    pub fn checked_items(&self) -> usize {
        self.items.iter().flatten().filter(|c| **c).count()
    }

    /// 160 项硬门。
    pub fn items_full(&self) -> bool {
        self.checked_items() == GROUP_COUNT * ITEMS_PER_GROUP
    }

    /// 对账对完成（对 0-27）。
    pub fn close_pair(&mut self, pair: usize) -> S9ACode {
        if pair >= PAIR_COUNT {
            return S9ACode::SCOPE_OVERFLOW;
        }
        self.pairs[pair] = true;
        S9ACode::OK
    }

    pub fn pairs_closed(&self) -> usize {
        self.pairs.iter().filter(|p| **p).count()
    }

    /// 七维汇总登记。
    pub fn close_dim(&mut self, dim: usize) -> S9ACode {
        if dim >= 7 {
            return S9ACode::SCOPE_OVERFLOW;
        }
        self.dims[dim] = true;
        S9ACode::OK
    }

    pub fn dims_closed(&self) -> usize {
        self.dims.iter().filter(|d| **d).count()
    }

    /// 自查完备：三面全绿。
    pub fn complete(&self) -> bool {
        self.items_full() && self.pairs_closed() == PAIR_COUNT && self.dims_closed() == 7
    }
}

impl Default for SelfAuditPlan {
    fn default() -> Self {
        Self::new()
    }
}

// ── 三增维声明 ───────────────────────────────────────────────────────

/// S 域三增维。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExtraDim {
    /// 工具无障碍。
    ToolA11y,
    /// 产出无障碍。
    OutputA11y,
    /// 心流。
    Flow,
}

pub const EXTRA_DIMS: [ExtraDim; 3] = [ExtraDim::ToolA11y, ExtraDim::OutputA11y, ExtraDim::Flow];

/// 增维标准登记：无标准 → 先立标准（红线）。
#[derive(Debug, Clone)]
pub struct DimStandards {
    /// 每增维 → 是否已立标准。
    pub standing: [bool; 3],
}

impl DimStandards {
    pub fn new() -> Self {
        Self { standing: [false; 3] }
    }

    /// 增维登记前置断言：未立标准即拒。
    pub fn allow_entry(&self, d: ExtraDim) -> S9ACode {
        if self.standing[d as usize] {
            S9ACode::OK
        } else {
            S9ACode::NOSTANDARD
        }
    }

    pub fn establish(&mut self, d: ExtraDim) {
        self.standing[d as usize] = true;
    }

    pub fn all_standing(&self) -> bool {
        self.standing.iter().all(|s| *s)
    }
}

impl Default for DimStandards {
    fn default() -> Self {
        Self::new()
    }
}

// ── 证据引用制（双源）────────────────────────────────────────────────

/// 无障碍证据双源。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvidenceSource {
    /// 自动化色标。
    ColorMark,
    /// 用户测试录证（脱敏）。
    UserRecording,
}

/// 证据指针：双源引用 + 采集周期（时效）。
#[derive(Debug, Clone)]
pub struct EvidenceRef {
    pub item: &'static str,
    pub source: EvidenceSource,
    /// 采集周期号。
    pub cycle: u32,
}

/// 证据引用账：每项必须挂至少一源指针。
#[derive(Debug, Clone)]
pub struct EvidenceBook {
    pub refs: Vec<EvidenceRef>,
    /// 当前周期。
    pub cycle_now: u32,
    /// 时效窗（超过即过期）。
    pub fresh_window: u32,
}

impl EvidenceBook {
    pub fn new(cycle_now: u32, fresh_window: u32) -> Self {
        Self { refs: Vec::new(), cycle_now, fresh_window }
    }

    pub fn attach(&mut self, r: EvidenceRef) {
        self.refs.push(r);
    }

    /// 证据缺检出：项名无指针。
    pub fn missing(&self, item: &str) -> bool {
        !self.refs.iter().any(|r| r.item == item)
    }

    /// 过期检出：周期差超窗。
    pub fn stale(&self, item: &str) -> bool {
        self.refs.iter().filter(|r| r.item == item)
            .any(|r| self.cycle_now.saturating_sub(r.cycle) > self.fresh_window)
    }

    /// 双源断言：项同时有色标与录证才算双源齐。
    pub fn dual_source(&self, item: &str) -> bool {
        self.refs.iter().any(|r| r.item == item && r.source == EvidenceSource::ColorMark)
            && self.refs.iter().any(|r| r.item == item && r.source == EvidenceSource::UserRecording)
    }

    /// 增量复查：过期项原位刷新周期（安全实现：仅改 Copy 字段）。
    pub fn refresh(&mut self, item: &str, _source: EvidenceSource) -> S9ACode {
        if self.missing(item) {
            return S9ACode::EVIDENCE_MISSING;
        }
        let now = self.cycle_now;
        for r in self.refs.iter_mut() {
            if r.item == item {
                r.cycle = now;
            }
        }
        S9ACode::OK
    }
}

// ── 判据 ─────────────────────────────────────────────────────────────

pub fn run_ves215_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves215_s09scope");

    // 01 模板第八用声明
    {
        let mut t = TemplateUses::new();
        for _ in 0..7 {
            t.declare_use();
        }
        chk(&mut set, "C3961-01", !t.is_eighth() && t.declare_use() == 8 && t.is_eighth(), "err");
    }
    // 02 范围守恒：160 + 28 + 七维
    {
        chk(&mut set, "C3961-02", GROUP_COUNT * ITEMS_PER_GROUP == 160 && PAIR_COUNT == 28 && SEVEN_DIMS.len() == 7 && SEVEN_DIMS[4] == "无障碍", "err");
    }
    // 03 项核验越界拒收
    {
        let mut p = SelfAuditPlan::new();
        chk(&mut set, "C3961-03", p.check_item(0, 1) == S9ACode::SCOPE_OVERFLOW && p.check_item(9, 1) == S9ACode::SCOPE_OVERFLOW && p.check_item(1, 21) == S9ACode::SCOPE_OVERFLOW, "err");
    }
    // 04 160 项硬门（抽样不满不绿）
    {
        let mut p = SelfAuditPlan::new();
        for g in 1..=8 {
            for i in 1..=20 {
                let _ = p.check_item(g, i);
            }
        }
        chk(&mut set, "C3961-04", p.items_full() && p.checked_items() == 160, "err");
    }
    // 05 28 对对账
    {
        let mut p = SelfAuditPlan::new();
        for i in 0..28 {
            let _ = p.close_pair(i);
        }
        chk(&mut set, "C3961-05", p.pairs_closed() == 28 && p.close_pair(28) == S9ACode::SCOPE_OVERFLOW, "err");
    }
    // 06 七维汇总
    {
        let mut p = SelfAuditPlan::new();
        for d in 0..7 {
            let _ = p.close_dim(d);
        }
        chk(&mut set, "C3961-06", p.dims_closed() == 7 && p.complete(), "err");
    }
    // 07 三增维清单守恒
    {
        chk(&mut set, "C3961-07", EXTRA_DIMS.len() == 3 && ExtraDim::ToolA11y as usize == 0 && ExtraDim::Flow as usize == 2, "err");
    }
    // 08 增维无标准→先立标准实测
    {
        let mut s = DimStandards::new();
        chk(&mut set, "C3961-08", s.allow_entry(ExtraDim::Flow) == S9ACode::NOSTANDARD && { s.establish(ExtraDim::Flow); s.allow_entry(ExtraDim::Flow) == S9ACode::OK }, "err");
    }
    // 09 证据双源断言
    {
        let mut b = EvidenceBook::new(10, 3);
        b.attach(EvidenceRef { item: "对比度", source: EvidenceSource::ColorMark, cycle: 9 });
        chk(&mut set, "C3961-09", !b.dual_source("对比度") && { b.attach(EvidenceRef { item: "对比度", source: EvidenceSource::UserRecording, cycle: 9 }); b.dual_source("对比度") }, "err");
    }
    // 10 证据缺→阻断检出
    {
        let b = EvidenceBook::new(10, 3);
        chk(&mut set, "C3961-10", b.missing("焦点序") && b.stale("焦点序") == false, "err");
    }
    // 11 证据过期→增量复查实测
    {
        let mut b = EvidenceBook::new(20, 3);
        b.attach(EvidenceRef { item: "色标", source: EvidenceSource::ColorMark, cycle: 5 });
        chk(&mut set, "C3961-11", b.stale("色标") && b.refresh("色标", EvidenceSource::ColorMark) == S9ACode::OK, "err");
    }
    // 12 刷新不存在项拒收
    {
        let mut b = EvidenceBook::new(20, 3);
        chk(&mut set, "C3961-12", b.refresh("幽灵", EvidenceSource::ColorMark) == S9ACode::EVIDENCE_MISSING, "err");
    }
    // 13 诊断码窗界自守
    {
        chk(&mut set, "C3961-13", S9ACode::SCOPE_OVERFLOW.0 < SEG_BASE + 16, "err");
    }
    // 14 错误码解释覆盖
    {
        let all = [S9ACode::OK, S9ACode::EVIDENCE_MISSING, S9ACode::EVIDENCE_STALE, S9ACode::NOSTANDARD, S9ACode::SCOPE_OVERFLOW];
        chk(&mut set, "C3961-14", all.iter().all(|c| c.explain() != "未知"), "err");
    }

    set
}

// ── 测试支撑 ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_s09_scope_all_green() {
        let s = run_ves215_checks();
        assert!(s.all_passed(), "ves215 red: {}", s.red_items_text());
    }

    #[test]
    fn test_plan_complete_gate() {
        let mut p = SelfAuditPlan::new();
        assert!(!p.complete());
        for g in 1..=8 {
            for i in 1..=20 {
                let _ = p.check_item(g, i);
            }
        }
        for i in 0..28 {
            let _ = p.close_pair(i);
        }
        for d in 0..7 {
            let _ = p.close_dim(d);
        }
        assert!(p.complete());
    }

    #[test]
    fn test_evidence_dual_source() {
        let mut b = EvidenceBook::new(5, 2);
        b.attach(EvidenceRef { item: "x", source: EvidenceSource::ColorMark, cycle: 5 });
        b.attach(EvidenceRef { item: "x", source: EvidenceSource::UserRecording, cycle: 5 });
        assert!(b.dual_source("x"));
        assert!(!b.stale("x"));
    }
}
