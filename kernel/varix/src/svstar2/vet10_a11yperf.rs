//! VE-F3811 · 无障碍渲染性能（VE-T 域 · 无障碍渲染 · 目标 360 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3811`
//!
//! **判据（锚点原文）**：1ms 分项、归因分离、消失 P0、可达保底、预算执法、判据。
//!
//! **本条是什么**：无障碍渲染开销治理——总预算 1ms + 分项预算
//! （高对比 0.3/焦点 0.2/替代 0.3/其他 0.2）；开销与主渲染归因分离
//! （混账 = 优化失焦）；降档不得降可达性（**消失禁令 P0**：可以丑
//! 但不可以消失）。
//!
//! 诊断码段：独占 0xC4xx。

#![allow(dead_code)]

extern crate alloc;

use alloc::vec::Vec;

// ─────────────────────────────────────────────────────────────
// 诊断码（0xC4xx）
// ─────────────────────────────────────────────────────────────

/// F3811 诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct C4Code(pub u16);

impl C4Code {
    /// 总预算超——分项定位。
    pub const BUDGET_OVER: C4Code = C4Code(0xC401);
    /// 混账检出——分离修正。
    pub const MIXED_BOOK: C4Code = C4Code(0xC402);
    /// 降档消失——P0。
    pub const VANISH_P0: C4Code = C4Code(0xC403);
    /// 分项超——组内优化。
    pub const SUB_OVER: C4Code = C4Code(0xC404);
    /// 分项表缺。
    pub const TABLE_MISS: C4Code = C4Code(0xC405);
    /// 可达保底破——P0。
    pub const REACH_BREAK: C4Code = C4Code(0xC406);

    /// 诊断说明。
    pub fn explain(self) -> &'static str {
        match self.0 {
            0xC401 => "无障碍渲染总预算超 1ms，需分项定位。",
            0xC402 => "开销混入主渲染账，归因分离红线。",
            0xC403 => "降档导致渲染消失，P0——可达性消失禁令。",
            0xC404 => "分项预算超，需组内优化。",
            0xC405 => "分项预算表缺失。",
            0xC406 => "可达保底被破坏，P0。",
            _ => "未知诊断码。",
        }
    }

    /// 数值。
    pub fn raw(self) -> u16 {
        self.0
    }
}

// ─────────────────────────────────────────────────────────────
// 分项预算（总 1ms）
// ─────────────────────────────────────────────────────────────

/// 预算分项（四项）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BudgetItem {
    /// 高对比 0.3ms。
    HighContrast,
    /// 焦点 0.2ms。
    Focus,
    /// 替代 0.3ms。
    Substitute,
    /// 其他 0.2ms。
    Other,
}

impl BudgetItem {
    /// 全集。
    pub const ALL: [BudgetItem; 4] = [
        BudgetItem::HighContrast,
        BudgetItem::Focus,
        BudgetItem::Substitute,
        BudgetItem::Other,
    ];

    /// 序号。
    pub fn ordinal(self) -> usize {
        match self {
            BudgetItem::HighContrast => 0,
            BudgetItem::Focus => 1,
            BudgetItem::Substitute => 2,
            BudgetItem::Other => 3,
        }
    }

    /// 名称。
    pub fn name(self) -> &'static str {
        match self {
            BudgetItem::HighContrast => "高对比",
            BudgetItem::Focus => "焦点",
            BudgetItem::Substitute => "替代",
            BudgetItem::Other => "其他",
        }
    }

    /// 预算微秒（1ms 总 = 1000us：300/200/300/200）。
    pub fn budget_us(self) -> u32 {
        match self {
            BudgetItem::HighContrast => 300,
            BudgetItem::Focus => 200,
            BudgetItem::Substitute => 300,
            BudgetItem::Other => 200,
        }
    }
}

/// 预算执法器。
#[derive(Clone, Debug)]
pub struct BudgetEnforcer {
    /// 分项实测微秒。
    used: [u32; 4],
}

impl BudgetEnforcer {
    /// 新建。
    pub const fn new() -> Self {
        BudgetEnforcer { used: [0; 4] }
    }

    /// 记开销。
    pub fn charge(&mut self, item: BudgetItem, us: u32) {
        self.used[item.ordinal()] += us;
    }

    /// 某项超支。
    pub fn sub_over(&self, item: BudgetItem) -> bool {
        self.used[item.ordinal()] > item.budget_us()
    }

    /// 总开销。
    pub fn total_us(&self) -> u32 {
        self.used.iter().sum()
    }

    /// 总预算超（>1000us）。
    pub fn total_over(&self) -> bool {
        self.total_us() > 1_000
    }

    /// 超支项名。
    pub fn over_items(&self) -> Vec<&'static str> {
        let mut out = Vec::new();
        for it in BudgetItem::ALL.iter() {
            if self.sub_over(*it) {
                out.push(it.name());
            }
        }
        out
    }

    /// 分项表完整（四项都有记录路径）。
    pub fn table_complete(&self) -> bool {
        true
    }
}

// ─────────────────────────────────────────────────────────────
// 归因分离
// ─────────────────────────────────────────────────────────────

/// 开销账本（两本：无障碍 vs 主渲染）。
#[derive(Clone, Debug)]
pub struct CostBooks {
    /// 无障碍账（微秒）。
    pub a11y_us: u32,
    /// 主渲染账（微秒）。
    pub main_us: u32,
    /// 混账检出次数。
    pub mixed: u32,
}

impl CostBooks {
    /// 新建。
    pub const fn new() -> Self {
        CostBooks {
            a11y_us: 0,
            main_us: 0,
            mixed: 0,
        }
    }

    /// 记无障碍开销。
    pub fn charge_a11y(&mut self, us: u32) {
        self.a11y_us += us;
    }

    /// 记主渲染开销。
    pub fn charge_main(&mut self, us: u32) {
        self.main_us += us;
    }

    /// 混账记（违规：把无障碍开销记进主账）。
    pub fn mix_violation(&mut self) {
        self.mixed += 1;
    }

    /// 分离干净。
    pub fn separated(&self) -> bool {
        self.mixed == 0
    }
}

// ─────────────────────────────────────────────────────────────
// 降档保底（消失禁令 P0）
// ─────────────────────────────────────────────────────────────

/// 降档策略。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Degrade {
    /// 不降。
    None,
    /// 降视觉品质。
    Quality,
    /// 降刷新率。
    Fps,
    /// 极端降档（仍须保可达）。
    Extreme,
}

/// 降档后状态。
#[derive(Clone, Copy, Debug)]
pub struct DegradedState {
    /// 内容仍可见/可读。
    pub reachable: bool,
    /// 内容仍可操作。
    pub operable: bool,
}

impl DegradedState {
    /// 构造。
    pub const fn new(reachable: bool, operable: bool) -> Self {
        DegradedState {
            reachable,
            operable,
        }
    }

    /// 消失禁令合规（可达且可操作）。
    pub fn no_vanish(self) -> bool {
        self.reachable && self.operable
    }
}

/// 降档器。
#[derive(Clone, Debug)]
pub struct DegradeEngine {
    /// 当前档。
    pub level: Degrade,
    /// 消失事件数（P0）。
    pub vanish: u32,
    /// 保底破坏数。
    pub reach_break: u32,
}

impl DegradeEngine {
    /// 新建。
    pub const fn new() -> Self {
        DegradeEngine {
            level: Degrade::None,
            vanish: 0,
            reach_break: 0,
        }
    }

    /// 执行降档并评估保底。
    pub fn degrade(&mut self, level: Degrade, after: DegradedState) -> bool {
        self.level = level;
        if !after.reachable {
            self.vanish += 1;
            return false;
        }
        if !after.operable {
            self.reach_break += 1;
            return false;
        }
        true
    }

    /// 零消失。
    pub fn zero_vanish(&self) -> bool {
        self.vanish == 0
    }
}

// ─────────────────────────────────────────────────────────────
// 判据集
// ─────────────────────────────────────────────────────────────

use crate::checks::CheckSet;

/// F3811 判据集。
pub fn run_f3811_checks() -> CheckSet {
    let mut set = CheckSet::new("VE-F3811");

    // 0xC405 分项表
    {
        set.add(
            "0xC405-分项和=1ms",
            BudgetItem::HighContrast.budget_us()
                + BudgetItem::Focus.budget_us()
                + BudgetItem::Substitute.budget_us()
                + BudgetItem::Other.budget_us()
                == 1_000,
            "",
        );
    }

    // 0xC401 总预算
    {
        let mut b = BudgetEnforcer::new();
        b.charge(BudgetItem::HighContrast, 250);
        b.charge(BudgetItem::Focus, 150);
        b.charge(BudgetItem::Substitute, 250);
        b.charge(BudgetItem::Other, 150);
        set.add("0xC401-800us不超", !b.total_over() && b.total_us() == 800, "");
        b.charge(BudgetItem::Other, 300);
        set.add("0xC401-1100超", b.total_over(), "");
    }

    // 0xC404 分项超
    {
        let mut b = BudgetEnforcer::new();
        b.charge(BudgetItem::Focus, 250);
        set.add("0xC404-焦点超检出", b.sub_over(BudgetItem::Focus), "");
        set.add("0xC404-超项名单", b.over_items().len() == 1, "");
        set.add("0xC404-其他未超", !b.sub_over(BudgetItem::Other), "");
    }

    // 0xC402 归因分离
    {
        let mut c = CostBooks::new();
        c.charge_a11y(100);
        c.charge_main(200);
        set.add("0xC402-分离干净", c.separated() && c.a11y_us == 100, "");
        c.mix_violation();
        set.add("0xC402-混账检出", !c.separated() && c.mixed == 1, "");
    }

    // 0xC403/0xC406 消失禁令
    {
        let mut d = DegradeEngine::new();
        let ok = DegradedState::new(true, true);
        set.add("0xC403-保底过", d.degrade(Degrade::Quality, ok) && d.zero_vanish(), "");
        let vanish = DegradedState::new(false, false);
        set.add("0xC403-消失P0", !d.degrade(Degrade::Extreme, vanish) && !d.zero_vanish(), "");
        let reach = DegradedState::new(true, false);
        set.add("0xC406-可操作破", !d.degrade(Degrade::Fps, reach) && d.reach_break == 1, "");
    }

    // 项序自洽
    {
        let mut ok = true;
        for (i, it) in BudgetItem::ALL.iter().enumerate() {
            if it.ordinal() != i {
                ok = false;
            }
        }
        set.add("0xC405-项序自洽", ok, "");
    }

    set
}
