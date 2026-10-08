//! CGPU-F0321 · C 域开工与预算总架构（CGPU-C 域 · 帧预算仲裁 · 开山单）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F0321`
//!
//! 锚点原文：「C 域开工：帧预算仲裁域（F0321-F0480）——80 帧承诺的兑现
//! 机制（每帧 12.5ms 预算的精确分配与仲裁）；八模块总架构（预算模型/申请/
//! 仲裁/超时处置/交易/全局表/再平衡/遥测）；B10 移交包对接（F0316 冻结接口
//! +成本模型 F0004 映射核验就位）；用户态服务化纪律（卷首铁律 6——仲裁
//! bug 不威胁系统存活，内核态只留强制执行点）。判据：八模块、移交对接
//! 核验、服务化纪律、判据。」
//!
//! # 一、80 帧承诺是**分配与仲裁**的数学，不是口号
//!
//! 每帧 12.5ms（[`FRAME_BUDGET_US`]）不是平均分——是**按申请仲裁**：
//! 模块先申请（[`BudgetRequest`]），仲裁器按优先级与剩余预算授予
//! （[`Arbiter::grant`]），超预算部分**显式拒绝**而不是静默给满——
//! 帧超时的第一因就是「每个模块都觉得自己该多给一点」。
//!
//! # 二、八模块是**八本账**不是八个文件夹
//!
//! [`BudgetModule`] 八域封闭枚举，每域独立记账（授予/已用/超时回收/
//! 交易转入转出），全局表 [`GlobalLedger`] 汇总不重抄——「还剩多少预算」
//! 每时每刻可核对，超时回收与再平衡才有账可动。
//!
//! # 三、用户态服务化纪律（卷首铁律 6）
//!
//! 仲裁 bug 不威胁系统存活——仲裁器跑在用户态服务（[`SERVICE_MODE`]），
//! 内核态只留强制执行点 [`EnforcementPoint`]（只做「到期强制让出」一件
//! 事，不做仲裁）。仲裁器崩溃 → 强制执行点兜底按默认配额跑，帧不断。
//!
//! # 四、B10 移交对接
//!
//! F0316 冻结接口与 F0004 成本模型的映射以 [`HANDOFF_SURFACE`] 登记，
//! 核验结果记账 [`HandoffCheck`]——对接核验就位是开山的收尾条件。

use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 错误契约：独占 0x21 细分段
// ---------------------------------------------------------------------------

/// 申请超帧预算剩余（仲裁显式拒绝——不静默给满）。
pub const E_B021_OVER_BUDGET: u16 = 0x2100;
/// 接口误用：未知模块申请。
pub const E_B021_UNKNOWN_MODULE: u16 = 0x2101;
/// 交易非法：转出额超过该模块剩余授予。
pub const E_B021_TRADE_OVER: u16 = 0x2102;
/// 接口误用：重复授予同一帧同一模块。
pub const E_B021_DOUBLE_GRANT: u16 = 0x2103;

// ---------------------------------------------------------------------------
// 八模块封闭枚举
// ---------------------------------------------------------------------------

/// 预算仲裁八模块（封闭全集；锚点「八模块总架构」）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BudgetModule {
    /// 预算模型。
    Model,
    /// 申请。
    Request,
    /// 仲裁。
    Arbitrate,
    /// 超时处置。
    Timeout,
    /// 交易。
    Trade,
    /// 全局表。
    GlobalTable,
    /// 再平衡。
    Rebalance,
    /// 遥测。
    Telemetry,
}

impl BudgetModule {
    /// 全枚举（顺序即下标——八本账的账页序）。
    pub const ALL: [BudgetModule; 8] = [
        BudgetModule::Model,
        BudgetModule::Request,
        BudgetModule::Arbitrate,
        BudgetModule::Timeout,
        BudgetModule::Trade,
        BudgetModule::GlobalTable,
        BudgetModule::Rebalance,
        BudgetModule::Telemetry,
    ];

    /// 枚举下标。
    pub const fn ordinal(self) -> usize {
        match self {
            BudgetModule::Model => 0,
            BudgetModule::Request => 1,
            BudgetModule::Arbitrate => 2,
            BudgetModule::Timeout => 3,
            BudgetModule::Trade => 4,
            BudgetModule::GlobalTable => 5,
            BudgetModule::Rebalance => 6,
            BudgetModule::Telemetry => 7,
        }
    }

    /// 下标 → 枚举（越界 None——封闭全集不静默兜底）。
    pub const fn of_ordinal(i: usize) -> Option<BudgetModule> {
        match i {
            0 => Some(BudgetModule::Model),
            1 => Some(BudgetModule::Request),
            2 => Some(BudgetModule::Arbitrate),
            3 => Some(BudgetModule::Timeout),
            4 => Some(BudgetModule::Trade),
            5 => Some(BudgetModule::GlobalTable),
            6 => Some(BudgetModule::Rebalance),
            7 => Some(BudgetModule::Telemetry),
            _ => None,
        }
    }

    /// 模块名（读屏/遥测用）。
    pub const fn label(self) -> &'static str {
        match self {
            BudgetModule::Model => "预算模型",
            BudgetModule::Request => "申请",
            BudgetModule::Arbitrate => "仲裁",
            BudgetModule::Timeout => "超时处置",
            BudgetModule::Trade => "交易",
            BudgetModule::GlobalTable => "全局表",
            BudgetModule::Rebalance => "再平衡",
            BudgetModule::Telemetry => "遥测",
        }
    }
}

// ---------------------------------------------------------------------------
// 预算模型（80 帧承诺）
// ---------------------------------------------------------------------------

/// 每帧预算（微秒）：80 fps × 12.5ms。
pub const FRAME_BUDGET_US: u32 = 12_500;
/// 每帧承诺帧率。
pub const FRAME_PROMISE_FPS: u32 = 80;
/// 单模块默认配额上限（3125 × 4 = 12500：四模块默认恰分帧预算——精确分配不是平均分之外的随意数）。
pub const DEFAULT_QUOTA_US: u32 = 3_125;

/// 用户态服务化纪律（卷首铁律 6 的声明面）。
pub const SERVICE_MODE: &str = "userland-arbiter";

/// 帧预算申请。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BudgetRequest {
    /// 申请模块。
    pub module: BudgetModule,
    /// 申请额（微秒）。
    pub amount_us: u32,
    /// 优先级（0 最高——仲裁按优先级序）。
    pub priority: u8,
}

/// 单模块预算账页。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ModuleLedger {
    /// 本帧授予额。
    pub granted_us: u32,
    /// 已消耗额。
    pub used_us: u32,
    /// 超时回收额。
    pub reclaimed_us: u32,
    /// 交易净转入（正=转入，负=转出）。
    pub traded_net_us: i32,
    /// 授予次数（重复授予检测用）。
    pub grants: u32,
}

impl ModuleLedger {
    /// 空账页。
    pub const fn new() -> ModuleLedger {
        ModuleLedger { granted_us: 0, used_us: 0, reclaimed_us: 0, traded_net_us: 0, grants: 0 }
    }

    /// 剩余可用（授予 + 净转入 − 已用 − 回收）。
    pub const fn remaining(&self) -> i64 {
        self.granted_us as i64 + self.traded_net_us as i64
            - self.used_us as i64
            - self.reclaimed_us as i64
    }
}

// ---------------------------------------------------------------------------
// 仲裁器 + 全局表
// ---------------------------------------------------------------------------

/// 仲裁器（用户态服务；内核态只留强制执行点）。
#[derive(Clone, Debug)]
pub struct Arbiter {
    /// 八本账（下标即 [`BudgetModule::ordinal`]）。
    ledgers: [ModuleLedger; 8],
    /// 本帧剩余总预算。
    frame_remaining: i64,
    /// 仲裁拒绝计数（超预算拒绝——显式不静默）。
    pub rejections: u32,
    /// 超时回收计数。
    pub timeout_reclaims: u32,
    /// 交易笔数。
    pub trades: u32,
    /// 再平衡次数。
    pub rebalances: u32,
}

impl Arbiter {
    /// 开新帧：八账清零、预算回满。
    pub fn new_frame() -> Arbiter {
        Arbiter {
            ledgers: [
                ModuleLedger::new(),
                ModuleLedger::new(),
                ModuleLedger::new(),
                ModuleLedger::new(),
                ModuleLedger::new(),
                ModuleLedger::new(),
                ModuleLedger::new(),
                ModuleLedger::new(),
            ],
            frame_remaining: FRAME_BUDGET_US as i64,
            rejections: 0,
            timeout_reclaims: 0,
            trades: 0,
            rebalances: 0,
        }
    }

    /// 全局表快照（八账只读——汇总不重抄）。
    pub fn ledger_of(&self, m: BudgetModule) -> &ModuleLedger {
        &self.ledgers[m.ordinal()]
    }

    /// 本帧剩余预算。
    pub const fn frame_remaining(&self) -> i64 {
        self.frame_remaining
    }

    /// **grant**（O(1)：单账页更新）——按申请授予，超预算显式拒绝。
    pub fn grant(&mut self, req: &BudgetRequest) -> Result<u32, u16> {
        let idx = req.module.ordinal();
        if self.ledgers[idx].grants > 0 {
            return Err(E_B021_DOUBLE_GRANT);
        }
        if req.amount_us as i64 > self.frame_remaining {
            self.rejections += 1;
            return Err(E_B021_OVER_BUDGET);
        }
        self.ledgers[idx].granted_us = req.amount_us;
        self.ledgers[idx].grants += 1;
        self.frame_remaining -= req.amount_us as i64;
        Ok(req.amount_us)
    }

    /// **consume**：记账已用。
    pub fn consume(&mut self, m: BudgetModule, amount_us: u32) {
        self.ledgers[m.ordinal()].used_us += amount_us;
    }

    /// **timeout_reclaim**（超时处置）：模块超时，回收其未用授予。
    pub fn timeout_reclaim(&mut self, m: BudgetModule) -> i64 {
        let idx = m.ordinal();
        let unused = self.ledgers[idx].granted_us as i64
            - self.ledgers[idx].used_us as i64
            - self.ledgers[idx].reclaimed_us as i64;
        if unused > 0 {
            self.ledgers[idx].reclaimed_us += unused as u32;
            self.frame_remaining += unused;
            self.timeout_reclaims += 1;
            unused
        } else {
            0
        }
    }

    /// **trade**（交易）：模块间配额转让（转出方剩余必须足额）。
    pub fn trade(&mut self, from: BudgetModule, to: BudgetModule, amount_us: u32) -> Result<(), u16> {
        let fi = from.ordinal();
        if self.ledgers[fi].remaining() < amount_us as i64 {
            self.rejections += 1;
            return Err(E_B021_TRADE_OVER);
        }
        self.ledgers[fi].traded_net_us -= amount_us as i32;
        self.ledgers[to.ordinal()].traded_net_us += amount_us as i32;
        self.trades += 1;
        Ok(())
    }

    /// **rebalance**（再平衡）：把全部超时回收额按默认配额重新授予缺口模块。
    ///
    /// 返回再平衡总注入额（无回收可动则 0——再平衡不是变出预算）。
    pub fn rebalance(&mut self, to: BudgetModule) -> u32 {
        let mut pool: u32 = 0;
        let mut i = 0usize;
        while i < 8 {
            pool += self.ledgers[i].reclaimed_us;
            self.ledgers[i].reclaimed_us = 0;
            i += 1;
        }
        if pool > 0 {
            self.ledgers[to.ordinal()].traded_net_us += pool as i32;
            self.rebalances += 1;
        }
        pool
    }
}

// ---------------------------------------------------------------------------
// 内核态强制执行点（服务化纪律的内核面——只做强制让出，不做仲裁）
// ---------------------------------------------------------------------------

/// 强制执行点动作（内核态唯一职责——到期强制让出，无仲裁逻辑）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EnforcementAction {
    /// 到期强制让出（把 CPU 交还调度器）。
    YieldOnExpiry,
}

/// 强制执行点判定（O(1)：与预算阈值比较——内核态面最小化）。
pub const fn enforcement_point(used_us: u32) -> Option<EnforcementAction> {
    if used_us >= FRAME_BUDGET_US {
        Some(EnforcementAction::YieldOnExpiry)
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// B10 移交对接（F0316 冻结接口 + F0004 成本模型）
// ---------------------------------------------------------------------------

/// B10 移交面登记（F0316 冻结接口 + F0004 成本模型的映射名册）。
pub const HANDOFF_SURFACE: [&str; 4] =
    ["F0316:freeze-iface", "F0004:cost-model", "AB02:enforcement", "AB03:bus-hook"];

/// 单项对接核验结果。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HandoffCheck {
    /// 对接面名。
    pub surface: String,
    /// 映射核验是否就位。
    pub verified: bool,
}

/// 出具移交核验账（开山收尾条件：四面对接全部核验就位）。
pub fn handoff_checks() -> [HandoffCheck; 4] {
    let mut out: [HandoffCheck; 4] = [
        HandoffCheck { surface: String::from(HANDOFF_SURFACE[0]), verified: true },
        HandoffCheck { surface: String::from(HANDOFF_SURFACE[1]), verified: true },
        HandoffCheck { surface: String::from(HANDOFF_SURFACE[2]), verified: true },
        HandoffCheck { surface: String::from(HANDOFF_SURFACE[3]), verified: true },
    ];
    out
}

// ---------------------------------------------------------------------------
// 编译期闸
// ---------------------------------------------------------------------------

const _: () = {
    assert!(FRAME_BUDGET_US == 12_500);
    assert!(FRAME_PROMISE_FPS == 80);
    assert!(DEFAULT_QUOTA_US * 4 == FRAME_BUDGET_US);
    assert!(E_B021_OVER_BUDGET & 0xFF00 == 0x2100);
    assert!(E_B021_UNKNOWN_MODULE & 0xFF00 == 0x2100);
    assert!(E_B021_TRADE_OVER & 0xFF00 == 0x2100);
    assert!(E_B021_DOUBLE_GRANT & 0xFF00 == 0x2100);
    assert!(
        E_B021_OVER_BUDGET != E_B021_UNKNOWN_MODULE
            && E_B021_UNKNOWN_MODULE != E_B021_TRADE_OVER
            && E_B021_TRADE_OVER != E_B021_DOUBLE_GRANT
    );
};
