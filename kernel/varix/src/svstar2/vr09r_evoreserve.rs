//! VE-F3778 · R 域演进预留（R09 自查组 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3778`
//!
//! **判据（锚点原文）**：三段预留、成本评估、扩展红线、不承诺、复用七用、
//! 判据。
//!
//! **本条是什么**：R 域八组预留盘点——新创作类型/新工坊/协作实现/
//! 推荐模型化（可解释前提）/经济模型演进/质量标准演进/心流指标细化/
//! 辅具精度提升。每项走**三段**（提案→评估→冻结），带成本评估；
//! **扩展不进化**红线：预留是登记不是承诺，虚假承诺须撤回。
//!
//! 诊断码段：独占 0xA9xx。

#![allow(dead_code)]

extern crate alloc;

use alloc::vec::Vec;

// ─────────────────────────────────────────────────────────────
// 诊断码（0xA9xx）
// ─────────────────────────────────────────────────────────────

/// F3778 诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct A9Code(pub u16);

impl A9Code {
    /// 三段缺——补。
    pub const STAGE_MISS: A9Code = A9Code(0xA901);
    /// 破冻结——重设。
    pub const FREEZE_BREAK: A9Code = A9Code(0xA902);
    /// 未评估——拦截。
    pub const NO_EVAL: A9Code = A9Code(0xA903);
    /// 虚假承诺——撤回。
    pub const FALSE_PROMISE: A9Code = A9Code(0xA904);
    /// 八组缺组。
    pub const GROUP_MISS: A9Code = A9Code(0xA905);
    /// 成本缺失。
    pub const COST_MISS: A9Code = A9Code(0xA906);

    /// 诊断说明。
    pub fn explain(self) -> &'static str {
        match self.0 {
            0xA901 => "三段阶段缺失，预留不完整。",
            0xA902 => "冻结被破坏，需重设预留。",
            0xA903 => "未经评估即冻结，拦截。",
            0xA904 => "虚假承诺登记，须撤回（红线）。",
            0xA905 => "八组预留缺组。",
            0xA906 => "成本评估缺失。",
            _ => "未知诊断码。",
        }
    }

    /// 数值。
    pub fn raw(self) -> u16 {
        self.0
    }
}

// ─────────────────────────────────────────────────────────────
// 八组预留
// ─────────────────────────────────────────────────────────────

/// 预留组（八组 R 版闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ReserveGroup {
    /// 新创作类型。
    NewCreationType,
    /// 新工坊。
    NewWorkshop,
    /// 协作实现。
    CollabImpl,
    /// 推荐模型化（可解释前提）。
    RecModel,
    /// 经济模型演进。
    EconomyEvolve,
    /// 质量标准演进。
    QualityEvolve,
    /// 心流指标细化。
    FlowMetric,
    /// 辅具精度提升。
    AssistPrecision,
}

impl ReserveGroup {
    /// 全集。
    pub const ALL: [ReserveGroup; 8] = [
        ReserveGroup::NewCreationType,
        ReserveGroup::NewWorkshop,
        ReserveGroup::CollabImpl,
        ReserveGroup::RecModel,
        ReserveGroup::EconomyEvolve,
        ReserveGroup::QualityEvolve,
        ReserveGroup::FlowMetric,
        ReserveGroup::AssistPrecision,
    ];

    /// 序号。
    pub fn ordinal(self) -> usize {
        match self {
            ReserveGroup::NewCreationType => 0,
            ReserveGroup::NewWorkshop => 1,
            ReserveGroup::CollabImpl => 2,
            ReserveGroup::RecModel => 3,
            ReserveGroup::EconomyEvolve => 4,
            ReserveGroup::QualityEvolve => 5,
            ReserveGroup::FlowMetric => 6,
            ReserveGroup::AssistPrecision => 7,
        }
    }

    /// 名称。
    pub fn name(self) -> &'static str {
        match self {
            ReserveGroup::NewCreationType => "新创作类型",
            ReserveGroup::NewWorkshop => "新工坊",
            ReserveGroup::CollabImpl => "协作实现",
            ReserveGroup::RecModel => "推荐模型化",
            ReserveGroup::EconomyEvolve => "经济模型演进",
            ReserveGroup::QualityEvolve => "质量标准演进",
            ReserveGroup::FlowMetric => "心流指标细化",
            ReserveGroup::AssistPrecision => "辅具精度提升",
        }
    }
}

/// 预留三段。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stage {
    /// 提案。
    Proposal,
    /// 评估。
    Evaluation,
    /// 冻结。
    Frozen,
}

/// 单条预留。
#[derive(Clone, Copy, Debug)]
pub struct ReserveItem {
    /// 组。
    pub group: u8,
    /// 当前段。
    pub stage: Stage,
    /// 成本分（0=未评估）。
    pub cost: u16,
    /// 已评估。
    pub evaluated: bool,
    /// 是对外承诺（预留默认不是承诺）。
    pub promised: bool,
    /// 已撤回。
    pub retracted: bool,
}

impl ReserveItem {
    /// 新提案。
    pub const fn propose(group: u8) -> Self {
        ReserveItem {
            group,
            stage: Stage::Proposal,
            cost: 0,
            evaluated: false,
            promised: false,
            retracted: false,
        }
    }

    /// 评估（记录成本分）。
    pub fn evaluate(&mut self, cost: u16) -> bool {
        if self.stage != Stage::Proposal {
            return false;
        }
        self.cost = cost;
        self.evaluated = true;
        self.stage = Stage::Evaluation;
        true
    }

    /// 冻结（须已评估）。
    pub fn freeze(&mut self) -> bool {
        if self.stage == Stage::Evaluation && self.evaluated {
            self.stage = Stage::Frozen;
            true
        } else {
            false
        }
    }

    /// 破冻结（回评估段）。
    pub fn break_freeze(&mut self) -> bool {
        if self.stage == Stage::Frozen {
            self.stage = Stage::Evaluation;
            true
        } else {
            false
        }
    }

    /// 登记为承诺（预留登记不是承诺——须显式）。
    pub fn mark_promise(&mut self) {
        self.promised = true;
    }

    /// 撤回虚假承诺。
    pub fn retract(&mut self) -> bool {
        if self.promised && !self.retracted {
            self.retracted = true;
            self.promised = false;
            true
        } else {
            false
        }
    }
}

/// 预留清单。
#[derive(Clone, Debug)]
pub struct ReserveLedger {
    items: Vec<ReserveItem>,
}

impl ReserveLedger {
    /// 新建。
    pub fn new() -> Self {
        ReserveLedger { items: Vec::new() }
    }

    /// 提案。
    pub fn propose(&mut self, group: ReserveGroup) -> usize {
        self.items.push(ReserveItem::propose(group.ordinal() as u8));
        self.items.len() - 1
    }

    /// 按索引取可变引用。
    pub fn get_mut(&mut self, idx: usize) -> Option<&mut ReserveItem> {
        self.items.get_mut(idx)
    }

    /// 按索引取只读引用。
    pub fn get(&self, idx: usize) -> Option<&ReserveItem> {
        self.items.get(idx)
    }

    /// 组覆盖数。
    pub fn group_coverage(&self) -> usize {
        let mut seen = [false; 8];
        for it in self.items.iter() {
            let g = it.group as usize;
            if g < 8 {
                seen[g] = true;
            }
        }
        seen.iter().filter(|s| **s).count()
    }

    /// 未评估数。
    pub fn unevaluated(&self) -> usize {
        self.items.iter().filter(|i| !i.evaluated).count()
    }

    /// 成本缺失数（已冻结但 cost==0 且未评估标记丢失——防御）。
    pub fn cost_missing(&self) -> usize {
        self.items
            .iter()
            .filter(|i| i.stage == Stage::Frozen && !i.evaluated)
            .count()
    }

    /// 活跃虚假承诺数（promised 且未撤回）。
    pub fn live_promises(&self) -> usize {
        self.items.iter().filter(|i| i.promised && !i.retracted).count()
    }

    /// 总数。
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// 空。
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

impl Default for ReserveLedger {
    fn default() -> Self {
        ReserveLedger::new()
    }
}

// ─────────────────────────────────────────────────────────────
// 不承诺册 + 路线图
// ─────────────────────────────────────────────────────────────

/// 不承诺声明（预留登记不是承诺）。
#[derive(Clone, Copy, Debug)]
pub struct NoPromise {
    /// 声明在册。
    pub declared: bool,
    /// 路线图在册（双维：工具+产出）。
    pub roadmap_tool: bool,
    pub roadmap_output: bool,
}

impl NoPromise {
    /// 新建（缺）。
    pub const fn new() -> Self {
        NoPromise {
            declared: false,
            roadmap_tool: false,
            roadmap_output: false,
        }
    }

    /// 双维路线图齐。
    pub fn roadmap_ok(&self) -> bool {
        self.roadmap_tool && self.roadmap_output
    }

    /// 三件齐（声明+双路线图）。
    pub fn complete(&self) -> bool {
        self.declared && self.roadmap_ok()
    }
}

// ─────────────────────────────────────────────────────────────
// 判据集
// ─────────────────────────────────────────────────────────────

use crate::checks::CheckSet;

/// F3778 判据集。
pub fn run_vr09r_checks() -> CheckSet {
    let mut set = CheckSet::new("VE-F3778");

    // 0xA905 八组覆盖
    {
        let mut l = ReserveLedger::new();
        set.add("0xA905-初始零组", l.group_coverage() == 0, "");
        for g in ReserveGroup::ALL.iter() {
            l.propose(*g);
        }
        set.add("0xA905-八组齐", l.group_coverage() == 8, "");
        set.add("0xA905-八项在册", l.len() == 8, "");
    }

    // 0xA901 三段推进
    {
        let mut l = ReserveLedger::new();
        let i = l.propose(ReserveGroup::NewCreationType);
        if let Some(it) = l.get_mut(i) {
            set.add("0xA901-提案段", it.stage == Stage::Proposal, "");
            set.add("0xA901-未评估", !it.evaluated, "");
        }
        // 未评估冻结拒
        {
            let it = match l.get_mut(i) {
                Some(x) => x,
                None => {
                    set.add("0xA901-索引有效", false, "");
                    return set;
                }
            };
            set.add("0xA901-未评估冻结拒", !it.freeze(), "");
            set.add("0xA901-评估进", it.evaluate(50), "");
            set.add("0xA901-评估段", it.stage == Stage::Evaluation, "");
            set.add("0xA901-冻结进", it.freeze(), "");
            set.add("0xA901-冻结段", it.stage == Stage::Frozen, "");
        }
    }

    // 0xA902 破冻结
    {
        let mut l = ReserveLedger::new();
        let i = l.propose(ReserveGroup::RecModel);
        if let Some(it) = l.get_mut(i) {
            it.evaluate(30);
            it.freeze();
            set.add("0xA902-破冻结进", it.break_freeze(), "");
            set.add("0xA902-回评估段", it.stage == Stage::Evaluation, "");
            set.add("0xA902-非冻破拒", !it.break_freeze(), "");
        }
    }

    // 0xA903 未评估拦截（清单级）
    {
        let mut l = ReserveLedger::new();
        for g in ReserveGroup::ALL.iter() {
            l.propose(*g);
        }
        set.add("0xA903-全未评估", l.unevaluated() == 8, "");
        if let Some(it) = l.get_mut(0) {
            it.evaluate(10);
        }
        set.add("0xA903-评一剩七", l.unevaluated() == 7, "");
        set.add("0xA903-成本缺检出", l.cost_missing() == 0, "");
        if let Some(it) = l.get_mut(0) {
            it.evaluated = false;
            it.stage = Stage::Frozen;
        }
        set.add("0xA906-伪冻结成本缺", l.cost_missing() == 1, "");
    }

    // 0xA904 虚假承诺撤回
    {
        let mut l = ReserveLedger::new();
        let i = l.propose(ReserveGroup::EconomyEvolve);
        if let Some(it) = l.get_mut(i) {
            it.mark_promise();
        }
        set.add("0xA904-承诺计数", l.live_promises() == 1, "");
        if let Some(it) = l.get_mut(i) {
            set.add("0xA904-撤回进", it.retract(), "");
        }
        set.add("0xA904-撤回后零", l.live_promises() == 0, "");
        if let Some(it) = l.get_mut(i) {
            set.add("0xA904-重复撤拒", !it.retract(), "");
        }
    }

    // 不承诺册
    {
        let mut n = NoPromise::new();
        set.add("0xA904-初始不完整", !n.complete(), "");
        n.declared = true;
        set.add("0xA904-仅声明不完整", !n.complete(), "");
        n.roadmap_tool = true;
        n.roadmap_output = true;
        set.add("0xA904-三件齐", n.complete() && n.roadmap_ok(), "");
    }

    // 组序自洽
    {
        let mut ok = true;
        for (i, g) in ReserveGroup::ALL.iter().enumerate() {
            if g.ordinal() != i {
                ok = false;
            }
        }
        set.add("0xA905-组序自洽", ok, "");
    }

    set
}
