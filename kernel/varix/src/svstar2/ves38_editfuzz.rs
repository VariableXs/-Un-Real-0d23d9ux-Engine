//! VE-F3638 · 编辑器 fuzz（VE-S 域 · 批次 R02 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3638`
//!
//! **判据**：三不变量、丢数据 P0、三变异、闭环复述、基建单源、判据。
//! **码段**：独占 **0x56xx**。
//!
//! 零 panic 面、零 IO、零墙钟。

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

/// F3638 诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct V28Code(pub u16);

impl V28Code {
    /// 不变量违例。
    pub const INVARIANT_BROKEN: V28Code = V28Code(0x5601);
    /// 丢数据（P0）。
    pub const DATA_LOST: V28Code = V28Code(0x5602);
    /// 保真破。
    pub const FIDELITY_BROKEN: V28Code = V28Code(0x5603);
    /// 挂起。
    pub const WATCHDOG: V28Code = V28Code(0x5604);
    /// 变异集不全。
    pub const MUTATION_GAP: V28Code = V28Code(0x5605);

    /// 数值。
    pub const fn value(self) -> u16 {
        self.0
    }

    /// 中文说明。
    pub fn explain(self) -> &'static str {
        match self {
            V28Code::INVARIANT_BROKEN => "不变量违例。建议：立案跟进。",
            V28Code::DATA_LOST => "丢数据（P0 最高级）。",
            V28Code::FIDELITY_BROKEN => "保真破（P1）。",
            V28Code::WATCHDOG => "看门狗触发。",
            V28Code::MUTATION_GAP => "变异集不全。建议：补三变异。",
            _ => "未知诊断码。",
        }
    }
}

/// 三不变量。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Invariant {
    /// 不丢数据。
    NoDataLoss,
    /// 预览保真。
    PreviewFidelity,
    /// undo 完整。
    UndoComplete,
}

impl Invariant {
    /// 全集。
    pub fn all() -> [Invariant; 3] {
        [
            Invariant::NoDataLoss,
            Invariant::PreviewFidelity,
            Invariant::UndoComplete,
        ]
    }

    /// 中文名。
    pub fn name(self) -> &'static str {
        match self {
            Invariant::NoDataLoss => "不丢数据",
            Invariant::PreviewFidelity => "预览保真",
            Invariant::UndoComplete => "undo完整",
        }
    }
}

/// 三变异。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mutation {
    /// 参数变异。
    Param,
    /// 结构变异。
    Struct,
    /// 时序变异。
    Timing,
}

impl Mutation {
    /// 全集。
    pub fn all() -> [Mutation; 3] {
        [Mutation::Param, Mutation::Struct, Mutation::Timing]
    }
}

/// 不变量检查器。
#[derive(Clone, Debug, Default)]
pub struct InvariantChecker {
    broken: Vec<Invariant>,
}

impl InvariantChecker {
    /// 新建。
    pub fn new() -> InvariantChecker {
        InvariantChecker {
            broken: Vec::new(),
        }
    }

    /// 违例登记。
    pub fn violate(&mut self, inv: Invariant) {
        if !self.broken.contains(&inv) {
            self.broken.push(inv);
        }
    }

    /// 不变量审计。
    pub fn audit(&self) -> Result<(), V28Code> {
        if self.broken.contains(&Invariant::NoDataLoss) {
            return Err(V28Code::DATA_LOST);
        }
        if self.broken.contains(&Invariant::PreviewFidelity) {
            return Err(V28Code::FIDELITY_BROKEN);
        }
        if self.broken.contains(&Invariant::UndoComplete) {
            return Err(V28Code::INVARIANT_BROKEN);
        }
        Ok(())
    }

    /// 违例数。
    pub fn broken_count(&self) -> usize {
        self.broken.len()
    }
}

/// 变异集。
#[derive(Clone, Debug, Default)]
pub struct MutationSet {
    applied: Vec<Mutation>,
}

impl MutationSet {
    /// 应用一个变异。
    pub fn apply(&mut self, m: Mutation) {
        if !self.applied.contains(&m) {
            self.applied.push(m);
        }
    }

    /// 齐全审计。
    pub fn audit(&self) -> Result<(), V28Code> {
        for m in Mutation::all() {
            if !self.applied.contains(&m) {
                return Err(V28Code::MUTATION_GAP);
            }
        }
        Ok(())
    }
}

/// 对接点清单。
pub const DOCKS: [&str; 3] = ["VE-F2878", "VE-F2487", "VE-F3631"];

/// 自检清单。
pub fn run_ves38_checks() -> crate::checks::CheckSet {
    let mut out = crate::checks::CheckSet::new("VE-F3638");

    // 0x5601 不变量全过
    let ic = InvariantChecker::new();
    out.add("0x5601-不变量全过", ic.audit().is_ok(), "");

    // 0x5602 丢数据 P0
    let mut ic2 = InvariantChecker::new();
    ic2.violate(Invariant::NoDataLoss);
    out.add("0x5602-丢数据P0", ic2.audit().is_err(), "");

    // 0x5603 保真破
    let mut ic3 = InvariantChecker::new();
    ic3.violate(Invariant::PreviewFidelity);
    out.add("0x5603-保真破", ic3.audit().is_err(), "");

    // 0x5604 三变异齐
    let mut ms = MutationSet::default();
    for m in Mutation::all() {
        ms.apply(m);
    }
    out.add("0x5604-三变异齐", ms.audit().is_ok(), "");

    // 0x5605 变异缺
    let ms2 = MutationSet::default();
    out.add("0x5605-变异缺", ms2.audit().is_err(), "");

    // 0x5606 undo 完整
    let mut ic4 = InvariantChecker::new();
    ic4.violate(Invariant::UndoComplete);
    out.add("0x5606-undo违例", ic4.audit().is_err(), "");

    out
}
