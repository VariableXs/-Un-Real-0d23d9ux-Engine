//! VE-F3775 · 真实走查预演（R09 自查组 · 目标 340 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3775`
//!
//! **判据（锚点原文）**：双走查+五关、工具先行、不留尾巴、复用复刻、
//! 顺序闸、判据。
//!
//! **本条是什么**：正式走查前的**彩排**——把 F3375 双走查模式与五关
//! 制度完整复刻为可执行预演：预演清单 10 面 × 五关 × 双走查；
//! 工具先行（辅具预通）；不留尾巴（发现即清零或立案）。
//!
//! ## 预演清单（锚点数据结构：预演清单 10+5+双）
//!
//! 10 交互面（创作画布/工作流面板/编辑器/验证器/预览器/发布向导/
//! 个性化面板/市场页/运行时 HUD/设置页）× 五关（可达/可操作/可理解/
//! 稳健/辅具）× 双走查（创作者走查 + 终端用户走查）= 100 预演格。
//!
//! **顺序闸**：工具先行 → 10 面 → 五关 → 双走查 → 修正清零——
//! 跳步即拦截（复述顺序闸红线）。
//!
//! 诊断码段：独占 0xA6xx。

#![allow(dead_code)]

extern crate alloc;

use alloc::vec::Vec;

// ─────────────────────────────────────────────────────────────
// 诊断码（0xA6xx）
// ─────────────────────────────────────────────────────────────

/// F3775 诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct A6Code(pub u16);

impl A6Code {
    /// 工具未预通——工具先行红线。
    pub const TOOL_NOT_READY: A6Code = A6Code(0xA601);
    /// 预演缺项——补。
    pub const ITEM_MISS: A6Code = A6Code(0xA602);
    /// 辅具预通失败——修复。
    pub const ASSIST_FAIL: A6Code = A6Code(0xA603);
    /// 尾巴未清零——阻断。
    pub const TAIL_OPEN: A6Code = A6Code(0xA604);
    /// 顺序闸拦截。
    pub const ORDER_BLOCK: A6Code = A6Code(0xA605);
    /// 双走查缺一——不预演完。
    pub const DUAL_WALK_MISS: A6Code = A6Code(0xA606);
    /// 五关缺关。
    pub const GATE_MISS: A6Code = A6Code(0xA607);

    /// 诊断说明。
    pub fn explain(self) -> &'static str {
        match self.0 {
            0xA601 => "走查工具未预通，工具先行红线不通过。",
            0xA602 => "预演清单缺项，需补齐后重演。",
            0xA603 => "辅具预通失败，需修复后重演。",
            0xA604 => "修正尾巴未清零，预演不收口。",
            0xA605 => "顺序闸拦截，禁止跳步预演。",
            0xA606 => "双走查缺一，预演不完整。",
            0xA607 => "五关缺关，关卡覆盖不足。",
            _ => "未知诊断码。",
        }
    }

    /// 数值。
    pub fn raw(self) -> u16 {
        self.0
    }
}

// ─────────────────────────────────────────────────────────────
// 预演清单（10 面 × 五关 × 双走查）
// ─────────────────────────────────────────────────────────────

/// 交互面（10 面闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Surface {
    /// 创作画布。
    Canvas,
    /// 工作流面板。
    Workflow,
    /// 编辑器。
    Editor,
    /// 验证器。
    Validator,
    /// 预览器。
    Preview,
    /// 发布向导。
    Publish,
    /// 个性化面板。
    Persona,
    /// 市场页。
    Market,
    /// 运行时 HUD。
    RuntimeHud,
    /// 设置页。
    Settings,
}

impl Surface {
    /// 全集。
    pub const ALL: [Surface; 10] = [
        Surface::Canvas,
        Surface::Workflow,
        Surface::Editor,
        Surface::Validator,
        Surface::Preview,
        Surface::Publish,
        Surface::Persona,
        Surface::Market,
        Surface::RuntimeHud,
        Surface::Settings,
    ];

    /// 序号。
    pub fn ordinal(self) -> usize {
        match self {
            Surface::Canvas => 0,
            Surface::Workflow => 1,
            Surface::Editor => 2,
            Surface::Validator => 3,
            Surface::Preview => 4,
            Surface::Publish => 5,
            Surface::Persona => 6,
            Surface::Market => 7,
            Surface::RuntimeHud => 8,
            Surface::Settings => 9,
        }
    }

    /// 名称。
    pub fn name(self) -> &'static str {
        match self {
            Surface::Canvas => "创作画布",
            Surface::Workflow => "工作流面板",
            Surface::Editor => "编辑器",
            Surface::Validator => "验证器",
            Surface::Preview => "预览器",
            Surface::Publish => "发布向导",
            Surface::Persona => "个性化面板",
            Surface::Market => "市场页",
            Surface::RuntimeHud => "运行时HUD",
            Surface::Settings => "设置页",
        }
    }
}

/// 五关（可达/可操作/可理解/稳健/辅具）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WalkGate {
    /// 可达。
    Reachable,
    /// 可操作。
    Operable,
    /// 可理解。
    Understandable,
    /// 稳健。
    Robust,
    /// 辅具。
    Assistive,
}

impl WalkGate {
    /// 全集。
    pub const ALL: [WalkGate; 5] = [
        WalkGate::Reachable,
        WalkGate::Operable,
        WalkGate::Understandable,
        WalkGate::Robust,
        WalkGate::Assistive,
    ];

    /// 序号。
    pub fn ordinal(self) -> usize {
        match self {
            WalkGate::Reachable => 0,
            WalkGate::Operable => 1,
            WalkGate::Understandable => 2,
            WalkGate::Robust => 3,
            WalkGate::Assistive => 4,
        }
    }

    /// 名称。
    pub fn name(self) -> &'static str {
        match self {
            WalkGate::Reachable => "可达",
            WalkGate::Operable => "可操作",
            WalkGate::Understandable => "可理解",
            WalkGate::Robust => "稳健",
            WalkGate::Assistive => "辅具",
        }
    }
}

/// 双走查画像。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Walker {
    /// 创作者走查。
    Creator,
    /// 终端用户走查。
    EndUser,
}

impl Walker {
    /// 全集。
    pub const ALL: [Walker; 2] = [Walker::Creator, Walker::EndUser];

    /// 序号。
    pub fn ordinal(self) -> usize {
        match self {
            Walker::Creator => 0,
            Walker::EndUser => 1,
        }
    }

    /// 名称。
    pub fn name(self) -> &'static str {
        match self {
            Walker::Creator => "创作者走查",
            Walker::EndUser => "终端用户走查",
        }
    }
}

/// 预演阶段（顺序闸五段）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Phase {
    /// 工具先行。
    ToolFirst,
    /// 十面预演。
    Surfaces,
    /// 五关预演。
    Gates,
    /// 双走查预演。
    DualWalk,
    /// 修正清零。
    ClearTail,
}

impl Phase {
    /// 全序。
    pub const ORDER: [Phase; 5] = [
        Phase::ToolFirst,
        Phase::Surfaces,
        Phase::Gates,
        Phase::DualWalk,
        Phase::ClearTail,
    ];

    /// 序号。
    pub fn ordinal(self) -> usize {
        match self {
            Phase::ToolFirst => 0,
            Phase::Surfaces => 1,
            Phase::Gates => 2,
            Phase::DualWalk => 3,
            Phase::ClearTail => 4,
        }
    }

    /// 名称。
    pub fn name(self) -> &'static str {
        match self {
            Phase::ToolFirst => "工具先行",
            Phase::Surfaces => "十面预演",
            Phase::Gates => "五关预演",
            Phase::DualWalk => "双走查预演",
            Phase::ClearTail => "修正清零",
        }
    }
}

/// 单格预演结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CellResult {
    /// 面序号。
    pub surface: u8,
    /// 关序号。
    pub gate: u8,
    /// 画像序号。
    pub walker: u8,
    /// 通过。
    pub passed: bool,
    /// 发现数。
    pub findings: u8,
}

impl CellResult {
    /// 构造。
    pub const fn new(surface: u8, gate: u8, walker: u8, passed: bool, findings: u8) -> Self {
        CellResult {
            surface,
            gate,
            walker,
            passed,
            findings,
        }
    }

    /// 格键（面×关×画像唯一）。
    pub fn key(self) -> u16 {
        (self.surface as u16) * 50 + (self.gate as u16) * 2 + self.walker as u16
    }
}

/// 预演清单状态机。
#[derive(Clone, Debug)]
pub struct RehearsalList {
    /// 100 格结果。
    cells: Vec<CellResult>,
    /// 当前阶段。
    phase: usize,
    /// 工具预通完成。
    tool_ready: bool,
    /// 辅具预通完成。
    assist_ready: bool,
    /// 修正尾巴条数。
    open_tail: u32,
    /// 累计发现。
    total_findings: u32,
}

impl RehearsalList {
    /// 新建。
    pub fn new() -> Self {
        RehearsalList {
            cells: Vec::new(),
            phase: 0,
            tool_ready: false,
            assist_ready: false,
            open_tail: 0,
            total_findings: 0,
        }
    }

    /// 当前阶段。
    pub fn phase(&self) -> Phase {
        Phase::ORDER[self.phase.min(4)]
    }

    /// 推进阶段（顺序闸：前置未完则拒）。
    pub fn advance(&mut self) -> bool {
        match self.phase {
            0 => {
                if self.tool_ready && self.assist_ready {
                    self.phase = 1;
                    true
                } else {
                    false
                }
            }
            1 => {
                if self.surface_coverage() == 10 {
                    self.phase = 2;
                    true
                } else {
                    false
                }
            }
            2 => {
                if self.gate_coverage() == 5 {
                    self.phase = 3;
                    true
                } else {
                    false
                }
            }
            3 => {
                if self.dual_complete() {
                    self.phase = 4;
                    true
                } else {
                    false
                }
            }
            4 => {
                if self.open_tail == 0 {
                    self.phase = 5;
                    true
                } else {
                    false
                }
            }
            _ => false,
        }
    }

    /// 工具预通。
    pub fn mark_tool_ready(&mut self) {
        self.tool_ready = true;
    }

    /// 辅具预通。
    pub fn mark_assist_ready(&mut self, ok: bool) {
        self.assist_ready = ok;
        if !ok {
            self.open_tail += 1;
        }
    }

    /// 记一格。
    pub fn record(&mut self, cell: CellResult) -> bool {
        if self.phase != 1 && self.phase != 2 && self.phase != 3 {
            return false;
        }
        self.total_findings += cell.findings as u32;
        self.open_tail += cell.findings as u32;
        self.cells.push(cell);
        true
    }

    /// 清一条尾巴。
    pub fn clear_one_tail(&mut self) -> bool {
        if self.open_tail == 0 {
            return false;
        }
        self.open_tail -= 1;
        true
    }

    /// 面覆盖数。
    pub fn surface_coverage(&self) -> usize {
        let mut seen = [false; 10];
        for c in &self.cells {
            if (c.surface as usize) < 10 {
                seen[c.surface as usize] = true;
            }
        }
        seen.iter().filter(|s| **s).count()
    }

    /// 关覆盖数。
    pub fn gate_coverage(&self) -> usize {
        let mut seen = [false; 5];
        for c in &self.cells {
            if (c.gate as usize) < 5 {
                seen[c.gate as usize] = true;
            }
        }
        seen.iter().filter(|s| **s).count()
    }

    /// 双走查是否都出现。
    pub fn dual_complete(&self) -> bool {
        let mut seen = [false; 2];
        for c in &self.cells {
            if (c.walker as usize) < 2 {
                seen[c.walker as usize] = true;
            }
        }
        seen[0] && seen[1]
    }

    /// 开放尾巴数。
    pub fn open_tail(&self) -> u32 {
        self.open_tail
    }

    /// 累计发现。
    pub fn total_findings(&self) -> u32 {
        self.total_findings
    }

    /// 格数。
    pub fn cell_count(&self) -> usize {
        self.cells.len()
    }

    /// 是否可收口（五段走完且无尾巴）。
    pub fn closable(&self) -> bool {
        self.phase == 5 && self.open_tail == 0
    }
}

impl Default for RehearsalList {
    fn default() -> Self {
        RehearsalList::new()
    }
}

// ─────────────────────────────────────────────────────────────
// 修正清单（锚点：修正清单）
// ─────────────────────────────────────────────────────────────

/// 修正项。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FixItem {
    /// 来源格键。
    pub from_cell: u16,
    /// 严重度（0=提示 1=P1 2=P0）。
    pub severity: u8,
    /// 已修复。
    pub fixed: bool,
}

impl FixItem {
    /// 构造。
    pub const fn new(from_cell: u16, severity: u8) -> Self {
        FixItem {
            from_cell,
            severity,
            fixed: false,
        }
    }

    /// 红线项（P0）。
    pub fn is_p0(self) -> bool {
        self.severity == 2
    }
}

/// 修正清单。
#[derive(Clone, Debug)]
pub struct FixLedger {
    items: Vec<FixItem>,
}

impl FixLedger {
    /// 新建。
    pub fn new() -> Self {
        FixLedger { items: Vec::new() }
    }

    /// 登记。
    pub fn push(&mut self, item: FixItem) {
        self.items.push(item);
    }

    /// 修复一条。
    pub fn fix(&mut self, cell: u16) -> bool {
        for it in self.items.iter_mut() {
            if it.from_cell == cell && !it.fixed {
                it.fixed = true;
                return true;
            }
        }
        false
    }

    /// 未修数。
    pub fn open_count(&self) -> usize {
        self.items.iter().filter(|i| !i.fixed).count()
    }

    /// 未修 P0 数。
    pub fn open_p0(&self) -> usize {
        self.items.iter().filter(|i| !i.fixed && i.is_p0()).count()
    }

    /// 总数。
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

impl Default for FixLedger {
    fn default() -> Self {
        FixLedger::new()
    }
}

// ─────────────────────────────────────────────────────────────
// 判据集
// ─────────────────────────────────────────────────────────────

use crate::checks::CheckSet;

/// F3775 判据集。
pub fn run_vr09o_checks() -> CheckSet {
    let mut set = CheckSet::new("VE-F3775");

    // 0xA601/0xA603 工具先行
    {
        let mut r = RehearsalList::new();
        set.add("0xA601-未预通不可推进", !r.advance(), "");
        r.mark_tool_ready();
        set.add("0xA601-仅工具不可推进", !r.advance(), "");
        r.mark_assist_ready(true);
        set.add("0xA601-双预通过闸", r.advance(), "");
    }

    // 0xA603 辅具失败记尾巴
    {
        let mut r = RehearsalList::new();
        r.mark_tool_ready();
        r.mark_assist_ready(false);
        set.add("0xA603-辅具失败记尾巴", r.open_tail() == 1, "");
        set.add("0xA603-失败不推进", !r.advance(), "");
    }

    // 0xA602 面覆盖
    {
        let mut r = RehearsalList::new();
        r.mark_tool_ready();
        r.mark_assist_ready(true);
        r.advance();
        for s in 0..10 {
            r.record(CellResult::new(s, 0, 0, true, 0));
        }
        set.add("0xA602-十面覆盖", r.surface_coverage() == 10, "");
        set.add("0xA602-过面闸", r.advance(), "");
    }

    // 0xA607 五关覆盖
    {
        let mut r = RehearsalList::new();
        r.mark_tool_ready();
        r.mark_assist_ready(true);
        r.advance();
        for s in 0..10 {
            r.record(CellResult::new(s, 0, 0, true, 0));
        }
        r.advance();
        for g in 0..5 {
            r.record(CellResult::new(0, g, 0, true, 0));
        }
        set.add("0xA607-五关覆盖", r.gate_coverage() == 5, "");
        set.add("0xA607-过关闸", r.advance(), "");
    }

    // 0xA606 双走查
    {
        let mut r = RehearsalList::new();
        r.mark_tool_ready();
        r.mark_assist_ready(true);
        r.advance();
        for s in 0..10 {
            r.record(CellResult::new(s, 0, 0, true, 0));
        }
        r.advance();
        for g in 0..5 {
            r.record(CellResult::new(0, g, 0, true, 0));
        }
        r.advance();
        r.record(CellResult::new(0, 0, 0, true, 0));
        set.add("0xA606-单走查不齐", !r.dual_complete(), "");
        r.record(CellResult::new(0, 0, 1, true, 0));
        set.add("0xA606-双走查齐", r.dual_complete(), "");
    }

    // 0xA604 尾巴清零
    {
        let mut r = RehearsalList::new();
        r.mark_tool_ready();
        r.mark_assist_ready(true);
        r.advance();
        r.record(CellResult::new(0, 0, 0, true, 3));
        set.add("0xA604-发现记尾巴", r.open_tail() == 3, "");
        set.add("0xA604-清三回", r.clear_one_tail() && r.clear_one_tail() && r.clear_one_tail(), "");
        set.add("0xA604-空清拒", !r.clear_one_tail(), "");
    }

    // 0xA605 顺序闸
    {
        let mut r = RehearsalList::new();
        set.add("0xA605-跳步拒推进", !r.advance(), "");
        r.mark_tool_ready();
        r.mark_assist_ready(true);
        set.add("0xA605-阶段名", r.phase() == Phase::ToolFirst, "");
    }

    // 修正清单
    {
        let mut f = FixLedger::new();
        set.add("0xA604-空清单", f.is_empty() && f.open_count() == 0, "");
        f.push(FixItem::new(1, 2));
        f.push(FixItem::new(2, 1));
        set.add("0xA604-两未修", f.open_count() == 2, "");
        set.add("0xA604-P0计数", f.open_p0() == 1, "");
        f.fix(1);
        set.add("0xA604-修一剩一", f.open_count() == 1 && f.open_p0() == 0, "");
    }

    // 全集序号自洽
    {
        let mut ok = true;
        for (i, s) in Surface::ALL.iter().enumerate() {
            if s.ordinal() != i {
                ok = false;
            }
        }
        set.add("0xA602-面序自洽", ok, "");
        let mut ok_g = true;
        for (i, g) in WalkGate::ALL.iter().enumerate() {
            if g.ordinal() != i {
                ok_g = false;
            }
        }
        set.add("0xA607-关序自洽", ok_g, "");
    }

    set
}
