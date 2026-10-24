//! VE-F3798 · R 域演进预留（R10 收口组 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3798`
//!
//! **判据（锚点原文）**：双维路线图、不承诺声明、成本评估、预留登记、判据。
//!
//! **本条是什么**：R 域演进预留终版——双维路线图（工具+产出）；
//! 不承诺声明册；预留登记成本评估；虚假承诺撤回红线。
//!
//! 诊断码段：独占 0xBDxx。

#![allow(dead_code)]

extern crate alloc;

use alloc::vec::Vec;

// ─────────────────────────────────────────────────────────────
// 诊断码（0xBDxx）
// ─────────────────────────────────────────────────────────────

/// F3798 诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BDCode(pub u16);

impl BDCode {
    /// 路线图缺维——补。
    pub const ROADMAP_MISS: BDCode = BDCode(0xBD01);
    /// 虚假承诺——撤回。
    pub const FALSE_PROMISE: BDCode = BDCode(0xBD02);
    /// 成本缺失——补评。
    pub const COST_MISS: BDCode = BDCode(0xBD03);
    /// 声明缺——补。
    pub const DECL_MISS: BDCode = BDCode(0xBD04);
    /// 预留缺登记。
    pub const RSV_MISS: BDCode = BDCode(0xBD05);
    /// 双维不齐。
    pub const DUAL_MISS: BDCode = BDCode(0xBD06);

    /// 诊断说明。
    pub fn explain(self) -> &'static str {
        match self.0 {
            0xBD01 => "双维路线图缺维，需补齐。",
            0xBD02 => "虚假承诺登记，须撤回。",
            0xBD03 => "预留成本未评估，需补评。",
            0xBD04 => "不承诺声明缺失，需补。",
            0xBD05 => "预留登记缺失。",
            0xBD06 => "双维不齐。",
            _ => "未知诊断码。",
        }
    }

    /// 数值。
    pub fn raw(self) -> u16 {
        self.0
    }
}

// ─────────────────────────────────────────────────────────────
// 双维路线图 + 不承诺声明
// ─────────────────────────────────────────────────────────────

/// 路线图维。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RoadmapDim {
    /// 工具维。
    Tool,
    /// 产出维。
    Output,
}

impl RoadmapDim {
    /// 全集。
    pub const ALL: [RoadmapDim; 2] = [RoadmapDim::Tool, RoadmapDim::Output];

    /// 序号。
    pub fn ordinal(self) -> usize {
        match self {
            RoadmapDim::Tool => 0,
            RoadmapDim::Output => 1,
        }
    }

    /// 名称。
    pub fn name(self) -> &'static str {
        match self {
            RoadmapDim::Tool => "工具维",
            RoadmapDim::Output => "产出维",
        }
    }
}

/// 路线图。
#[derive(Clone, Debug)]
pub struct Roadmap {
    present: [bool; 2],
    /// 不承诺声明在册。
    pub no_promise: bool,
    /// 每维条目数。
    entries: [u16; 2],
}

impl Roadmap {
    /// 新建。
    pub const fn new() -> Self {
        Roadmap {
            present: [false; 2],
            no_promise: false,
            entries: [0; 2],
        }
    }

    /// 登记一维。
    pub fn enroll(&mut self, d: RoadmapDim, count: u16) -> bool {
        let i = d.ordinal();
        self.present[i] = true;
        self.entries[i] = count;
        true
    }

    /// 双维齐。
    pub fn dual_ok(&self) -> bool {
        self.present[0] && self.present[1]
    }

    /// 缺维数。
    pub fn missing(&self) -> usize {
        self.present.iter().filter(|p| !**p).count()
    }

    /// 总条目。
    pub fn total_entries(&self) -> u32 {
        (self.entries[0] as u32) + (self.entries[1] as u32)
    }

    /// 完整（双维+声明+有条目）。
    pub fn complete(&self) -> bool {
        self.dual_ok() && self.no_promise && self.total_entries() > 0
    }
}

// ─────────────────────────────────────────────────────────────
// 预留登记 + 成本评估
// ─────────────────────────────────────────────────────────────

/// 预留项。
#[derive(Clone, Copy, Debug)]
pub struct RsvItem {
    /// 成本分（0=未评）。
    pub cost: u16,
    /// 已承诺（预留默认非承诺）。
    pub promised: bool,
    /// 已撤回。
    pub retracted: bool,
    /// 冻结。
    pub frozen: bool,
}

impl RsvItem {
    /// 新建。
    pub const fn new() -> Self {
        RsvItem {
            cost: 0,
            promised: false,
            retracted: false,
            frozen: false,
        }
    }

    /// 评估成本。
    pub fn evaluate(&mut self, cost: u16) -> bool {
        if cost == 0 {
            return false;
        }
        self.cost = cost;
        true
    }

    /// 冻结（须已评估）。
    pub fn freeze(&mut self) -> bool {
        if self.cost > 0 {
            self.frozen = true;
            true
        } else {
            false
        }
    }

    /// 撤回承诺。
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

/// 预留册。
#[derive(Clone, Debug)]
pub struct RsvLedger {
    items: Vec<RsvItem>,
}

impl RsvLedger {
    /// 新建。
    pub fn new() -> Self {
        RsvLedger { items: Vec::new() }
    }

    /// 收一项。
    pub fn push(&mut self) -> usize {
        self.items.push(RsvItem::new());
        self.items.len() - 1
    }

    /// 可变取。
    pub fn get_mut(&mut self, i: usize) -> Option<&mut RsvItem> {
        self.items.get_mut(i)
    }

    /// 未评成本数。
    pub fn unevaluated(&self) -> usize {
        self.items.iter().filter(|i| i.cost == 0).count()
    }

    /// 活跃虚假承诺数。
    pub fn live_promises(&self) -> usize {
        self.items
            .iter()
            .filter(|i| i.promised && !i.retracted)
            .count()
    }

    /// 冻结数。
    pub fn frozen_count(&self) -> usize {
        self.items.iter().filter(|i| i.frozen).count()
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

impl Default for RsvLedger {
    fn default() -> Self {
        RsvLedger::new()
    }
}

// ─────────────────────────────────────────────────────────────
// 判据集
// ─────────────────────────────────────────────────────────────

use crate::checks::CheckSet;

/// F3798 判据集。
pub fn run_vr10r_checks() -> CheckSet {
    let mut set = CheckSet::new("VE-F3798");

    // 0xBD01/0xBD06 双维路线图
    {
        let mut r = Roadmap::new();
        set.add("0xBD06-初始缺双", r.missing() == 2, "");
        r.enroll(RoadmapDim::Tool, 5);
        set.add("0xBD06-单维不齐", !r.dual_ok(), "");
        r.enroll(RoadmapDim::Output, 5);
        set.add("0xBD06-双维齐", r.dual_ok() && r.total_entries() == 10, "");
    }

    // 0xBD04 不承诺声明
    {
        let mut r = Roadmap::new();
        r.enroll(RoadmapDim::Tool, 1);
        r.enroll(RoadmapDim::Output, 1);
        set.add("0xBD04-无声明不完整", !r.complete(), "");
        r.no_promise = true;
        set.add("0xBD04-有声明完整", r.complete(), "");
    }

    // 0xBD03 成本评估
    {
        let mut l = RsvLedger::new();
        let i = l.push();
        set.add("0xBD03-初始未评", l.unevaluated() == 1, "");
        if let Some(it) = l.get_mut(i) {
            set.add("0xBD03-零成本拒评", !it.evaluate(0), "");
            it.evaluate(50);
            set.add("0xBD03-评后清", l.unevaluated() == 0, "");
        }
    }

    // 冻结须评估
    {
        let mut l = RsvLedger::new();
        let i = l.push();
        if let Some(it) = l.get_mut(i) {
            set.add("0xBD03-未评不冻", !it.freeze(), "");
            it.evaluate(30);
            set.add("0xBD03-评后可冻", it.freeze(), "");
            set.add("0xBD03-冻计数", l.frozen_count() == 1, "");
        }
    }

    // 0xBD02 虚假承诺撤回
    {
        let mut l = RsvLedger::new();
        let i = l.push();
        if let Some(it) = l.get_mut(i) {
            it.promised = true;
        }
        set.add("0xBD02-承诺计数", l.live_promises() == 1, "");
        if let Some(it) = l.get_mut(i) {
            set.add("0xBD02-撤回进", it.retract(), "");
        }
        set.add("0xBD02-撤后清", l.live_promises() == 0, "");
        if let Some(it) = l.get_mut(i) {
            set.add("0xBD02-重复撤拒", !it.retract(), "");
        }
    }

    // 维序自洽
    {
        let mut ok = true;
        for (i, d) in RoadmapDim::ALL.iter().enumerate() {
            if d.ordinal() != i {
                ok = false;
            }
        }
        set.add("0xBD01-维序自洽", ok, "");
    }

    set
}
