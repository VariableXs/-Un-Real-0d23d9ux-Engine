//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1063 · nohz 进入与退出协议（停发/恢复全序） · 诊断段 0x1470
//! ─────────────────────────────────────────────────────────────────────
//! 锚点判据原文（任务板逐字抄录，来源：
//!   docs/Varix/CoRun Varix STAR II · Unxreal/CoRun Varix STAR II · Unxreal.md:2779）：
//!   | UNX-F1063 | nohz 进入与退出协议（停发/恢复全序） | 320 | 已深化 |
//!   | UNX-F1063-J1 协议正确（进入/退出全序 10/10 次），退出补拍正确
//!   （停发期理论拍数补投断言 10/10 次——补拍语义固化），协议时长
//!   <20µs 断言 |
//! ─────────────────────────────────────────────────────────────────────
//! 设计要点（全手工定制真实子系统，无空壳）：
//!   1. 六态全序状态机 Periodic→EnterReq→TicksHalted→ExitReq→Catchup→
//!      Periodic；非法转移一律乱序拒绝并带 0x1470 段诊断码立案；
//!   2. 进入前置判据 const 门表（单任务 / 无 pending 回调 / 空闲预算 /
//!      非最后一核 / 时基同步 / 无悬空窗），逐位验收，任一失格回退；
//!   3. 停发期理论拍数整数推演：span = exit_tsc − enter_tsc，
//!      theory = span / period（下取整）+ 半周期进位（余数 ≥ period/2
//!      记进位旗），余数入推演账，全程无浮点；
//!   4. 退出补拍（Catchup）：按理论拍数逐拍补投 tick。补拍语义固化：
//!      补拍只推进 tick 账（tick_account += period），墙钟 wall_ns
//!      冻结不动（杜绝墙钟跳变），会话闭合时 tick 账增量与理论账
//!      严格对平（不平即 CatchupMismatch 红案）；
//!   5. 协议时长 <20µs 硬线：步数化预算——每协议步 / 每补拍拍按
//!      const 单步成本累加，越线即 BudgetBreach 立案；
//!   6. 多核协同：每 CPU 独立 nohz 账 + 全局"至少一核在拍"保底——
//!      最后一个在拍核请求停发被 LastCoreGuard 拒绝并回退；
//!   7. 事件账（环形 256 槽）+ health() 三色 + FNV-1a 64 指纹 +
//!      双跑确定性对拍（同 seed 双跑指纹恒等）+ 注入面（乱序注入 /
//!      补拍对不平注入 / 全核停发注入，逐项检出）。
//! ─────────────────────────────────────────────────────────────────────
//! 常量来源（写死，逐条注明）：
//!   - TICK_PERIOD_NS = 4_000_000：Linux x86_64 默认 CONFIG_HZ=250，
//!     1e9 ns / 250 = 4_000_000 ns/拍；
//!   - PROTOCOL_BUDGET_NS = 20_000：判据原文硬线 "<20µs"；
//!   - FNV-1a 64 位参数 offset=0xcbf29ce484222325、prime=0x100000001b3
//!     （Landon Curt Noll 公布参数）；
//!   - MAX_CATCHUP_TICKS = 46：预算 20_000 ns 扣除五协议步固定开销
//!     （5 × 1_200 = 6_000 ns）后 14_000 ns ÷ 每拍补投成本 300 ns = 46。
//! 诊断段核验记录（落点前 grep 实测）：
//!   - 0x1420 被 svstar2/ves442_s24flow.rs SEG_BASE 占用；
//!   - 0x1440 被 svstar2/ves443_s24graph.rs SEG_BASE 占用；
//!   - 0x1460 被 svstar2/ves444_s24lexin.rs SEG_BASE 占用；
//!   - 0x1480 被 svstar2/ves445_s24token.rs SEG_BASE 占用；
//!   - 0x1454 被 cgpu/vck12_hetsim.rs F1454_EXT_ANCHOR 占用；
//!   - 0x1470~0x147F 全仓无诊断码占用（unx561_fbmode.rs 的 "MODE147"
//!     为显示模式名字符串，非诊断码），故取 0x1470 段。
//! ═══════════════════════════════════════════════════════════════════════

use core::fmt::Write as _;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ─────────────────────────────────────────────────────────────────────────
// §0  段常量与时基参数
// ─────────────────────────────────────────────────────────────────────────

/// 诊断段基址（0x1470，核验记录见头注释）。
pub const SEG_BASE: u16 = 0x1470;

/// tick 周期（ns）。来源：CONFIG_HZ=250 → 1e9/250 = 4_000_000。
pub const TICK_PERIOD_NS: u64 = 4_000_000;

/// 协议时长硬线（ns）。来源：判据原文 "<20µs"。
pub const PROTOCOL_BUDGET_NS: u64 = 20_000;

/// 单协议步模拟成本（ns）。五步固定开销 5×1_200 = 6_000 ns，
/// 占预算 30%，为补拍留 14_000 ns 空间（预算推演见头注释）。
pub const STEP_COST_NS: u64 = 1_200;

/// 单补拍拍投递成本（ns）。46 拍 × 300 = 13_800 ns，紧贴预算余量。
pub const CATCHUP_TICK_COST_NS: u64 = 300;

/// 补拍拍数上限。来源：20_000 − 6_000（五步）= 14_000；14_000/300 = 46。
pub const MAX_CATCHUP_TICKS: u64 = 46;

/// 最小停发驻留（ns）。低于 2 拍的停发无节能收益（门 G2 判据）：
/// 收益模型 = 停发拍数 × 单拍节能 − 进出协议开销，2 拍为盈亏平衡点。
pub const MIN_RESIDENCE_NS: u64 = 2 * TICK_PERIOD_NS;

/// 半周期进位阈值（ns）。余数 ≥ period/2 时理论拍数进一（四舍五入语义）。
pub const HALF_PERIOD_NS: u64 = TICK_PERIOD_NS / 2;

/// 参与协同的核数上限（vCPU 池规格，协同账固定槽位避免堆分配）。
pub const MAX_CPUS: usize = 4;

/// 事件账容量（环形槽位；2 的幂便于取模——纪律允许，此处用 while 遍历）。
pub const EVENTS_CAP: usize = 256;

/// 判据点名演练轮数（10/10）。
pub const DRILL_ROUNDS: usize = 10;

/// FNV-1a 64 位偏移基（Landon Curt Noll 公布参数）。
const FNV_OFFSET: u64 = 0xcbf29ce484222325;

/// FNV-1a 64 位素数（Landon Curt Noll 公布参数）。
const FNV_PRIME: u64 = 0x100000001b3;

/// 编译期静态断言：预算推演自洽（补拍上限不越 20µs 硬线）。
const _: () = {
    // 五协议步固定开销
    const STEPS_FIXED: u64 = 5 * STEP_COST_NS; // 6_000
    // 上限补拍的投递开销
    const CATCHUP_MAX_COST: u64 = MAX_CATCHUP_TICKS * CATCHUP_TICK_COST_NS; // 13_800
    // 两者之和必须低于硬线（留 200 ns 余量）
    assert!(STEPS_FIXED + CATCHUP_MAX_COST < PROTOCOL_BUDGET_NS);
    // 周期必须为偶数，半周期阈值才有意义
    assert!(TICK_PERIOD_NS % 2 == 0);
    // 事件槽位必须是 2 的幂（环形取模）
    assert!(EVENTS_CAP & (EVENTS_CAP - 1) == 0);
    // 协同核数必须落在 1..=8 的硬件池规格内
    assert!(MAX_CPUS >= 1 && MAX_CPUS <= 8);
};

// ─────────────────────────────────────────────────────────────────────────
// §1  六态全序状态机
// ─────────────────────────────────────────────────────────────────────────

/// nohz 协议六态。
///
/// 全序（唯一合法环路）：
///   Periodic → EnterReq → TicksHalted → ExitReq → Catchup → Periodic
/// 另有两条合法回退边：
///   EnterReq → Periodic（进入门失格回退）
///   其余任何转移均属乱序，拒绝并立案。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NohzPhase {
    /// 周期在拍：tick 正常发放。
    Periodic,
    /// 进入请求：进入门验收中，tick 仍发放。
    EnterReq,
    /// 停发中：tick 停发，墙钟冻结。
    TicksHalted,
    /// 退出请求：仍停发，等待理论拍数推演。
    ExitReq,
    /// 补拍中：按理论拍数逐拍补投 tick（tick 账推进、墙钟不动）。
    Catchup,
    /// 闭合回拍（别名态，与 Periodic 同义；保留显式变体便于账面区分）。
    Closed,
}

impl NohzPhase {
    /// 中文短句（账面/摘要用）。
    pub const fn text(self) -> &'static str {
        match self {
            NohzPhase::Periodic => "周期在拍",
            NohzPhase::EnterReq => "进入请求",
            NohzPhase::TicksHalted => "停发中",
            NohzPhase::ExitReq => "退出请求",
            NohzPhase::Catchup => "补拍中",
            NohzPhase::Closed => "闭合回拍",
        }
    }

    /// 该态下 tick 是否停发（全局保底统计口径）。
    ///
    /// 口径：TicksHalted 与 ExitReq 期间 tick 全停；Catchup 阶段虽在
    /// 补投，但那是账面追平动作，不是实时 tick 源，亦计入停发口径。
    pub const fn is_halted(self) -> bool {
        match self {
            NohzPhase::TicksHalted => true,
            NohzPhase::ExitReq => true,
            NohzPhase::Catchup => true,
            NohzPhase::Periodic => false,
            NohzPhase::EnterReq => false,
            NohzPhase::Closed => false,
        }
    }

    /// 态序号（0..6），供 const 转移表索引。
    pub const fn ordinal(self) -> usize {
        match self {
            NohzPhase::Periodic => 0,
            NohzPhase::EnterReq => 1,
            NohzPhase::TicksHalted => 2,
            NohzPhase::ExitReq => 3,
            NohzPhase::Catchup => 4,
            NohzPhase::Closed => 5,
        }
    }

    /// 由序号还态（const 可用；越界回 Periodic）。
    pub const fn from_ordinal(o: usize) -> NohzPhase {
        match o {
            0 => NohzPhase::Periodic,
            1 => NohzPhase::EnterReq,
            2 => NohzPhase::TicksHalted,
            3 => NohzPhase::ExitReq,
            4 => NohzPhase::Catchup,
            _ => NohzPhase::Closed,
        }
    }
}

/// 全序合法转移表（const，含两条回退边，共 7 条）。
///
/// 写死依据：状态机设计一节（头注释要点 1）。任何不在此表的
/// (from, to) 组合都是乱序。
const ALLOWED_TRANSITIONS: [(usize, usize); 7] = [
    // (from.ordinal(), to.ordinal())
    (0, 1), // Periodic   → EnterReq   请求进入
    (1, 2), // EnterReq   → TicksHalted 过门停发
    (1, 0), // EnterReq   → Periodic   门失格回退
    (2, 3), // TicksHalted→ ExitReq    请求退出
    (3, 4), // ExitReq    → Catchup    转入补拍
    (4, 5), // Catchup    → Closed     补拍闭合
    (5, 0), // Closed     → Periodic   回到周期在拍
];

/// const 转移合法性判定（while + 索引，规避 const fn 禁坑）。
const fn transition_allowed(from: NohzPhase, to: NohzPhase) -> bool {
    let f = from.ordinal();
    let t = to.ordinal();
    let mut i = 0;
    while i < ALLOWED_TRANSITIONS.len() {
        let (a, b) = ALLOWED_TRANSITIONS[i];
        if a == f && b == t {
            return true;
        }
        i += 1;
    }
    false
}

/// 编译期断言：转移表自洽（每条边的 from/to 序号都落在六态域内）。
const _: () = {
    let mut i = 0;
    while i < ALLOWED_TRANSITIONS.len() {
        let (a, b) = ALLOWED_TRANSITIONS[i];
        assert!(a < 6);
        assert!(b < 6);
        i += 1;
    }
};

// ─────────────────────────────────────────────────────────────────────────
// §2  进入前置判据 const 门表
// ─────────────────────────────────────────────────────────────────────────

/// 进入门位掩码（G0..G5，逐位验收）。
pub const GATE_SINGLE_TASK: u32 = 1 << 0; // G0 单任务持有
pub const GATE_NO_PENDING: u32 = 1 << 1; // G1 无 pending 回调
pub const GATE_IDLE_BUDGET: u32 = 1 << 2; // G2 空闲预算充足
pub const GATE_NOT_LAST_CORE: u32 = 1 << 3; // G3 非最后一核
pub const GATE_CLOCK_STABLE: u32 = 1 << 4; // G4 时基同步
pub const GATE_NO_OPEN_WINDOW: u32 = 1 << 5; // G5 无悬空协议窗

/// 全门齐备掩码（六位全一）。
pub const GATES_ALL: u32 = GATE_SINGLE_TASK
    | GATE_NO_PENDING
    | GATE_IDLE_BUDGET
    | GATE_NOT_LAST_CORE
    | GATE_CLOCK_STABLE
    | GATE_NO_OPEN_WINDOW;

/// 门判据 const 表条目。
pub struct GateEntry {
    /// 掩码位。
    pub mask: u32,
    /// 门名（账面）。
    pub name: &'static str,
    /// 失格时的中文诊断短句。
    pub deny_text: &'static str,
}

/// 进入前置判据 const 表（写死六门，逐条可验收）。
///
/// 写死依据：真实 nohz 进入路径三前提——单任务运行（无 rq 竞争）、
/// 定时器回调队列清空、距下次事件足够远（节能收益覆盖协议开销）；
/// 加本内核多核保底与时基一致性两条件，共六门。
pub const ENTER_GATES: [GateEntry; 6] = [
    GateEntry {
        mask: GATE_SINGLE_TASK,
        name: "G0单任务",
        deny_text: "运行队列存在竞争任务，不宜停发",
    },
    GateEntry {
        mask: GATE_NO_PENDING,
        name: "G1无pending回调",
        deny_text: "定时器回调队列非空，停发将拖欠回调",
    },
    GateEntry {
        mask: GATE_IDLE_BUDGET,
        name: "G2空闲预算",
        deny_text: "距下次事件不足最小驻留，节能不抵开销",
    },
    GateEntry {
        mask: GATE_NOT_LAST_CORE,
        name: "G3非末核",
        deny_text: "本核是最后一个在拍核，停发违反全局保底",
    },
    GateEntry {
        mask: GATE_CLOCK_STABLE,
        name: "G4时基同步",
        deny_text: "clocksource 未同步，停发期时戳不可信",
    },
    GateEntry {
        mask: GATE_NO_OPEN_WINDOW,
        name: "G5无悬空窗",
        deny_text: "存在未闭合协议窗，禁止叠加进入",
    },
];

/// 验收掩码：返回 (全部通过, 首个失格门序号; 全过则 0)。
///
/// const fn：while + 索引遍历 const 表（规避 const fn 禁坑）。
const fn gates_evaluate(mask: u32) -> (bool, usize) {
    let mut i = 0;
    while i < ENTER_GATES.len() {
        if mask & ENTER_GATES[i].mask == 0 {
            return (false, i);
        }
        i += 1;
    }
    (true, 0)
}

/// 编译期断言：全门掩码必须覆盖 const 表中全部六位。
const _: () = {
    let mut acc: u32 = 0;
    let mut i = 0;
    while i < ENTER_GATES.len() {
        acc |= ENTER_GATES[i].mask;
        i += 1;
    }
    assert!(acc == GATES_ALL);
    // 全掩码必须一次过门
    assert!(gates_evaluate(GATES_ALL).0);
    // 零掩码必须首门即拒
    assert!(!gates_evaluate(0).0);
};

// ─────────────────────────────────────────────────────────────────────────
// §3  错误枚举（每变体 detail() 中文可操作诊断）
// ─────────────────────────────────────────────────────────────────────────

/// nohz 协议错误集（每变体映射 0x1470 段诊断码 + 中文可操作诊断）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NohzError {
    /// 0x1470 乱序转移：不在全序转移表内的 (from, to)。
    OutOfOrder(NohzPhase, NohzPhase),
    /// 0x1471 进入门失格：掩码缺位（携带首失格门序号）。
    GateRefused(usize),
    /// 0x1472 最后一核保底：全核停发拦截。
    LastCoreGuard,
    /// 0x1473 时钟倒退：退出时戳早于进入时戳。
    ClockBackward(u64),
    /// 0x1474 补拍超限：理论拍数越 MAX_CATCHUP_TICKS（携带理论值）。
    CatchupOver(u64),
    /// 0x1475 补拍对不平：闭合时补拍账 ≠ 理论账（携带 (已投, 理论)）。
    CatchupMismatch(u64, u64),
    /// 0x1476 预算越线：会话累计开销越过 20µs 硬线（携带开销）。
    BudgetBreach(u64),
    /// 0x1477 核号越界：cpu ≥ MAX_CPUS。
    BadCpu(usize),
    /// 0x1478 会话悬空：闭合时报账缺失（携带核号）。
    SessionOpen(usize),
    /// 0x1479 驻留过短：停发跨度不足最小驻留（携带跨度）。
    TooShort(u64),
    /// 0x147A 事件账满：环形槽全部占用且被拒写入（携带拒写事件码）。
    LogFull(u16),
    /// 0x147B 账面缺段：被请求的核从未建立 nohz 账。
    LedgerMissing(usize),
}

impl NohzError {
    /// 诊断码（0x1470 段 + 变体序号）。
    pub const fn code(&self) -> u16 {
        match self {
            NohzError::OutOfOrder(_, _) => SEG_BASE + 0x0,
            NohzError::GateRefused(_) => SEG_BASE + 0x1,
            NohzError::LastCoreGuard => SEG_BASE + 0x2,
            NohzError::ClockBackward(_) => SEG_BASE + 0x3,
            NohzError::CatchupOver(_) => SEG_BASE + 0x4,
            NohzError::CatchupMismatch(_, _) => SEG_BASE + 0x5,
            NohzError::BudgetBreach(_) => SEG_BASE + 0x6,
            NohzError::BadCpu(_) => SEG_BASE + 0x7,
            NohzError::SessionOpen(_) => SEG_BASE + 0x8,
            NohzError::TooShort(_) => SEG_BASE + 0x9,
            NohzError::LogFull(_) => SEG_BASE + 0xA,
            NohzError::LedgerMissing(_) => SEG_BASE + 0xB,
        }
    }

    /// 中文可操作诊断（每变体给"哪里坏 + 下一步怎么办"）。
    pub fn detail(&self) -> &'static str {
        match self {
            NohzError::OutOfOrder(_, _) => {
                "转移不在全序表内：按 Periodic→EnterReq→TicksHalted→ExitReq→Catchup→Periodic 顺序重走，或走 EnterReq→Periodic 回退边"
            }
            NohzError::GateRefused(_) => {
                "进入门失格：先补齐失格位（掩码见 ENTER_GATES 表），再重新 request_enter"
            }
            NohzError::LastCoreGuard => {
                "最后一核保底触发：至少保留一核在拍；先让其余核恢复在拍，再请求本核停发"
            }
            NohzError::ClockBackward(_) => {
                "退出时戳早于进入时戳：核对 clocksource 单调性（门 G4），必要时先做时基重同步"
            }
            NohzError::CatchupOver(_) => {
                "理论拍数越补拍上限：缩短停发跨度或提高 tick 周期，避免补拍成本越 20µs 预算"
            }
            NohzError::CatchupMismatch(_, _) => {
                "补拍账与理论账不平：逐拍核对 serve_catchup_tick 调用次数与推演账 theory_ticks"
            }
            NohzError::BudgetBreach(_) => {
                "协议开销越 20µs 硬线：已立案并回退；压缩补拍拍数或单步成本后重试"
            }
            NohzError::BadCpu(_) => {
                "核号越界：cpu 必须 < MAX_CPUS（协同池规格），先核对核号来源"
            }
            NohzError::SessionOpen(_) => {
                "会话悬空：闭合前必须先走完全序五步；用 health() 定位悬空核"
            }
            NohzError::TooShort(_) => {
                "停发跨度不足最小驻留：跨度至少 2 拍（节能盈亏平衡点），否则直接保持 Periodic"
            }
            NohzError::LogFull(_) => {
                "事件账已满：环形槽 256 全占用；先导出指纹与摘要，再 clear 重启账本"
            }
            NohzError::LedgerMissing(_) => {
                "该核无 nohz 账：先为该核建账（协议构造时统一建立），再发起协议动作"
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────
// §4  事件账（环形 + FNV-1a 64 指纹）
// ─────────────────────────────────────────────────────────────────────────

/// 事件种类。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventKind {
    /// 进入请求受理。
    EnterRequested,
    /// 进入门失格（携带首失格门序号）。
    GateDenied(usize),
    /// 停发生效（携带进入时戳）。
    HaltArmed(u64),
    /// 末核保底拦截。
    LastCoreBlocked,
    /// 退出请求受理（携带退出时戳）。
    ExitRequested(u64),
    /// 理论拍数推演落账（携带理论拍数）。
    TheoryComputed(u64),
    /// 单拍补投（携带第几拍，从 1 计）。
    CatchupTick(u64),
    /// 补拍闭合对平（携带 (已投, 理论)）。
    CatchupReconciled(u64, u64),
    /// 补拍对不平（携带 (已投, 理论)）。
    CatchupBroken(u64, u64),
    /// 会话闭合回拍（携带会话净开销 ns）。
    SessionClosed(u64),
    /// 乱序拒绝（携带 from/to 序号）。
    OutOfOrderBlocked(u8, u8),
    /// 预算越线立案（携带累计 ns）。
    BudgetBreachFiled(u64),
    /// 墙钟校准落账（携带校准差值 ns，有符号按补码存）。
    WallCalibrated(u64),
    /// 注入检出（携带注入种类序号）。
    InjectCaught(u8),
    /// 驻留过短拒绝（携带实际跨度 ns）。
    ShortStayRejected(u64),
    /// 会话中止回退（携带中止时核的态序号）。
    SessionAborted(u8),
    /// 账面软复位（审计/演练收尾）。
    LedgerReset,
}

impl EventKind {
    /// 事件中文短句。
    pub fn text(&self) -> String {
        match self {
            EventKind::EnterRequested => "进入请求受理".to_string(),
            EventKind::GateDenied(g) => format!("进入门失格（{}号门）", g),
            EventKind::HaltArmed(t) => format!("停发生效（进入时戳={}ns）", t),
            EventKind::LastCoreBlocked => "末核保底拦截".to_string(),
            EventKind::ExitRequested(t) => format!("退出请求受理（退出时戳={}ns）", t),
            EventKind::TheoryComputed(n) => format!("理论拍数推演落账（{}拍）", n),
            EventKind::CatchupTick(n) => format!("补投第{}拍", n),
            EventKind::CatchupReconciled(a, b) => format!("补拍对平（已投{}/理论{}）", a, b),
            EventKind::CatchupBroken(a, b) => format!("补拍对不平（已投{}/理论{}）", a, b),
            EventKind::SessionClosed(ns) => format!("会话闭合（净开销{}ns）", ns),
            EventKind::OutOfOrderBlocked(f, t) => format!(
                "乱序拒绝（{}→{}）",
                NohzPhase::from_ordinal(*f as usize).text(),
                NohzPhase::from_ordinal(*t as usize).text()
            ),
            EventKind::BudgetBreachFiled(ns) => format!("预算越线立案（累计{}ns）", ns),
            EventKind::WallCalibrated(d) => format!("墙钟校准（差值{}ns）", d),
            EventKind::InjectCaught(k) => format!("注入检出（种类{}）", k),
            EventKind::ShortStayRejected(ns) => format!("驻留过短拒绝（跨度{}ns）", ns),
            EventKind::SessionAborted(o) => format!(
                "会话中止回退（中止时态={}）",
                NohzPhase::from_ordinal(*o as usize).text()
            ),
            EventKind::LedgerReset => "账面软复位".to_string(),
        }
    }

    /// 事件严重级：0=信息、1=黄、2=红。
    pub const fn severity(&self) -> u8 {
        match self {
            EventKind::CatchupBroken(_, _) => 2,
            EventKind::BudgetBreachFiled(_) => 2,
            EventKind::OutOfOrderBlocked(_, _) => 2,
            EventKind::LastCoreBlocked => 1,
            EventKind::GateDenied(_) => 1,
            EventKind::InjectCaught(_) => 1,
            EventKind::ShortStayRejected(_) => 1,
            EventKind::SessionAborted(_) => 1,
            _ => 0,
        }
    }
}

/// 单条事件记录（标量账，可安全入栈）。
#[derive(Clone, Copy, Debug)]
pub struct EventRecord {
    /// 事件序号（账内单调递增，wrapping 防护）。
    pub seq: u64,
    /// 事件种类。
    pub kind: EventKind,
    /// 虚拟时戳（ns）。
    pub at_ns: u64,
    /// 事件所属核号（255 = 全局）。
    pub cpu: u8,
}

/// 环形事件账：固定 256 槽 + 写指针 + 总计数 + FNV 指纹。
///
/// 指纹规则：每条事件按 (seq, at_ns, cpu, 严重级, 载荷) 逐字节喂入
/// FNV-1a 64；账清空时指纹复位。双跑对拍即比指纹。
pub struct EventLog {
    /// 环形槽（堆上，容量 EVENTS_CAP）。
    slots: Vec<EventRecord>,
    /// 已写事件总数（含被环形覆盖的历史）。
    total: u64,
    /// 下一个写槽位（total & (CAP-1)）。
    head: usize,
    /// 红级事件计数。
    red_count: u64,
    /// 黄级事件计数。
    amber_count: u64,
    /// FNV-1a 64 运行指纹。
    fingerprint: u64,
}

impl EventLog {
    /// 建账（容量写死 EVENTS_CAP = 256）。
    pub fn new() -> EventLog {
        EventLog {
            slots: Vec::new(),
            total: 0,
            head: 0,
            red_count: 0,
            amber_count: 0,
            fingerprint: FNV_OFFSET,
        }
    }

    /// 记一条事件（满槽覆盖最旧并立案返回错误）。
    pub fn push(&mut self, kind: EventKind, at_ns: u64, cpu: u8) -> Result<(), NohzError> {
        let rec = EventRecord {
            seq: self.total,
            kind,
            at_ns,
            cpu,
        };
        // 指纹：逐字节喂 seq / at_ns / cpu / severity / 载荷摘要
        self.fingerprint = fnv_byte(self.fingerprint, (rec.seq & 0xFF) as u8);
        self.fingerprint = fnv_byte(self.fingerprint, ((rec.seq >> 8) & 0xFF) as u8);
        self.fingerprint = fnv_byte(self.fingerprint, (rec.at_ns & 0xFF) as u8);
        self.fingerprint = fnv_byte(self.fingerprint, ((rec.at_ns >> 8) & 0xFF) as u8);
        self.fingerprint = fnv_byte(self.fingerprint, ((rec.at_ns >> 16) & 0xFF) as u8);
        self.fingerprint = fnv_byte(self.fingerprint, ((rec.at_ns >> 24) & 0xFF) as u8);
        self.fingerprint = fnv_byte(self.fingerprint, rec.cpu);
        self.fingerprint = fnv_byte(self.fingerprint, kind.severity());
        self.fingerprint = fnv_byte(self.fingerprint, kind.severity().wrapping_mul(7));
        // 严重级计数（saturating 防溢出）
        match kind.severity() {
            2 => self.red_count = self.red_count.saturating_add(1),
            1 => self.amber_count = self.amber_count.saturating_add(1),
            _ => {}
        }
        if self.slots.len() < EVENTS_CAP {
            self.slots.push(rec);
        } else {
            // 环形覆盖最旧槽；覆盖本身不立案（正常环形行为），
            // 但 total 达 u64 饱和即异常（实际不可达，防御性立案）
            let slot = self.head;
            self.slots[slot] = rec;
            if self.total == u64::MAX {
                return Err(NohzError::LogFull(SEG_BASE));
            }
        }
        self.head = (self.head + 1) % EVENTS_CAP;
        self.total = self.total.wrapping_add(1);
        Ok(())
    }

    /// 账内现存事件数。
    pub fn len(&self) -> usize {
        self.slots.len()
    }

    /// 账是否为空。
    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    /// 红级事件计数。
    pub fn red_count(&self) -> u64 {
        self.red_count
    }

    /// 黄级事件计数。
    pub fn amber_count(&self) -> u64 {
        self.amber_count
    }

    /// 运行指纹。
    pub fn fingerprint(&self) -> u64 {
        self.fingerprint
    }

    /// 最近 n 条事件快照（按 seq 降序，新→旧）。
    pub fn recent(&self, n: usize) -> Vec<EventRecord> {
        let mut out: Vec<EventRecord> = Vec::new();
        let have = self.slots.len();
        let take = if n > have { have } else { n };
        let mut i = 0;
        while i < take {
            // 环形倒序：head-1-i 取模回绕
            let idx = (self.head + EVENTS_CAP - 1 - i) % EVENTS_CAP;
            out.push(self.slots[idx]);
            i += 1;
        }
        out
    }

    /// 按种类前缀检索（核对注入检出等场景用）。
    pub fn count_kind(&self, want: fn(&EventKind) -> bool) -> usize {
        let mut n = 0;
        let mut i = 0;
        while i < self.slots.len() {
            if want(&self.slots[i].kind) {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 账面软复位（指纹一并复位，复位本身留痕）。
    pub fn clear(&mut self) {
        self.slots.clear();
        self.total = 0;
        self.head = 0;
        self.red_count = 0;
        self.amber_count = 0;
        self.fingerprint = FNV_OFFSET;
        let _ = self.push(EventKind::LedgerReset, 0, 255);
    }
}

/// FNV-1a 单字节搅动。
const fn fnv_byte(h: u64, b: u8) -> u64 {
    (h ^ (b as u64)).wrapping_mul(FNV_PRIME)
}

// ─────────────────────────────────────────────────────────────────────────
// §5  停发期理论拍数推演（整数推演 + 推演账）
// ─────────────────────────────────────────────────────────────────────────

/// 理论拍数推演账（全程无浮点，每一步可复核）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HalspanAccount {
    /// 进入时戳（ns，停发生效瞬间）。
    pub enter_ns: u64,
    /// 退出时戳（ns，退出请求瞬间）。
    pub exit_ns: u64,
    /// 停发跨度（ns，exit − enter）。
    pub span_ns: u64,
    /// tick 周期（ns）。
    pub period_ns: u64,
    /// 整除商（下取整拍数）。
    pub quotient: u64,
    /// 整除余数（ns）。
    pub remainder_ns: u64,
    /// 半周期进位旗（余数 ≥ period/2 时理论拍数进一）。
    pub rounded_up: bool,
    /// 理论拍数（最终值，quotient + 进位）。
    pub theory_ticks: u64,
    /// 半周期阈值（ns，记账复核用）。
    pub half_period_ns: u64,
}

impl HalspanAccount {
    /// 整数推演：由进入/退出时戳与 tick 周期推理论拍数。
    ///
    /// 推演步（全部整数运算）：
    ///   1) span = exit − enter（exit < enter → ClockBackward）；
    ///   2) quotient = span / period；remainder = span % period；
    ///   3) rounded_up = remainder ≥ period/2；
    ///   4) theory = quotient + (rounded_up ? 1 : 0)。
    pub fn derive(enter_ns: u64, exit_ns: u64) -> Result<HalspanAccount, NohzError> {
        if exit_ns < enter_ns {
            return Err(NohzError::ClockBackward(enter_ns.wrapping_sub(exit_ns)));
        }
        let span_ns = exit_ns - enter_ns;
        let period_ns = TICK_PERIOD_NS;
        let quotient = span_ns / period_ns;
        let remainder_ns = span_ns % period_ns;
        let rounded_up = remainder_ns >= HALF_PERIOD_NS;
        // 进位用 saturating_add 显性防溢出
        let theory_ticks = if rounded_up {
            quotient.saturating_add(1)
        } else {
            quotient
        };
        Ok(HalspanAccount {
            enter_ns,
            exit_ns,
            span_ns,
            period_ns,
            quotient,
            remainder_ns,
            rounded_up,
            theory_ticks,
            half_period_ns: HALF_PERIOD_NS,
        })
    }

    /// 反向复核账：quotient×period + remainder 必须 === span（恒等式）。
    ///
    /// 这是推演账的自证：任何一步算错都会打破该恒等式。
    pub const fn reconciles(&self) -> bool {
        let rebuilt = self
            .quotient
            .wrapping_mul(self.period_ns)
            .wrapping_add(self.remainder_ns);
        rebuilt == self.span_ns
    }

    /// 推演账中文摘要。
    pub fn summarize(&self) -> String {
        format!(
            "推演账：span={}ns÷period={}ns={}拍余{}ns（半周期阈值{}ns{}进位）→理论{}拍；恒等式复核={}",
            self.span_ns,
            self.period_ns,
            self.quotient,
            self.remainder_ns,
            self.half_period_ns,
            if self.rounded_up { "已" } else { "未" },
            self.theory_ticks,
            self.reconciles()
        )
    }
}

// ─────────────────────────────────────────────────────────────────────────
// §6  每 CPU nohz 账（标量账，可安全入栈）
// ─────────────────────────────────────────────────────────────────────────

/// 每核 nohz 账：六态机现状 + 时戳 + 补拍账 + 墙钟/tick 双账。
#[derive(Clone, Copy, Debug)]
pub struct CpuNohzLedger {
    /// 核号。
    pub cpu: usize,
    /// 当前态。
    pub phase: NohzPhase,
    /// 停发生效时戳（ns；仅在 TicksHalted 之后有效）。
    pub enter_ns: u64,
    /// 退出请求时戳（ns；仅在 ExitReq 之后有效）。
    pub exit_ns: u64,
    /// 理论拍数（推演账落定后有效）。
    pub theory_ticks: u64,
    /// 已补投拍数。
    pub catchup_served: u64,
    /// 墙钟（ns）。补拍语义：补拍不推进墙钟，只在闭合时校准一次。
    pub wall_ns: u64,
    /// 墙钟冻结点（停发生效瞬间的墙钟值，校准基线）。
    pub wall_frozen_ns: u64,
    /// tick 账（拍数，模拟 jiffies）。补拍逐拍推进。
    pub tick_account: u64,
    /// 停发前 tick 账快照（对平基线：停发前账 + 理论 = 闭合时账）。
    pub tick_pre_halt: u64,
    /// 会话累计开销（ns，步数化预算账）。
    pub spent_ns: u64,
    /// 会话步数（协议步 + 补拍拍）。
    pub session_steps: u64,
    /// 会话是否闭合。
    pub session_closed: bool,
    /// 闭合时墙钟校准差值（闭合后有效）。
    pub calibrate_delta_ns: i64,
    /// 历史停发轮数（成功闭合计）。
    pub halt_rounds: u64,
    /// 理论拍数推演账（最近一次；None = 尚未推演）。
    pub halspan: Option<HalspanAccount>,
}

impl CpuNohzLedger {
    /// 建账（初始 Periodic，tick 账零起点）。
    pub const fn new(cpu: usize) -> CpuNohzLedger {
        CpuNohzLedger {
            cpu,
            phase: NohzPhase::Periodic,
            enter_ns: 0,
            exit_ns: 0,
            theory_ticks: 0,
            catchup_served: 0,
            wall_ns: 0,
            wall_frozen_ns: 0,
            tick_account: 0,
            tick_pre_halt: 0,
            spent_ns: 0,
            session_steps: 0,
            session_closed: true,
            calibrate_delta_ns: 0,
            halt_rounds: 0,
            halspan: None,
        }
    }

    /// 是否处于停发口径（含退出请求与补拍期）。
    pub const fn is_halted(&self) -> bool {
        self.phase.is_halted()
    }

    /// 补拍对平判定：已投 == 理论 且 tick 账增量 == 理论。
    pub const fn catchup_reconciled(&self) -> bool {
        self.theory_ticks == self.catchup_served
            && self.tick_account.wrapping_sub(self.tick_pre_halt) == self.theory_ticks
    }

    /// 会话累计开销是否在预算内。
    pub const fn within_budget(&self) -> bool {
        self.spent_ns < PROTOCOL_BUDGET_NS
    }
}

// ─────────────────────────────────────────────────────────────────────────
// §7  NohzProtocol：进入/退出全序协议协调器
// ─────────────────────────────────────────────────────────────────────────
//
// 职责：
//   a) 持有 MAX_CPUS 份每核账 + 全局事件账；
//   b) 执行六态全序转移（const 转移表裁决，乱序拒绝带码）；
//   c) 进入前独立复跑末核保底（G3 门位之外的第二道防线）；
//   d) 步数化预算：每步/每补拍按 const 单步成本累加，越 20µs 硬线
//      即立案并强制中止会话；
//   e) 补拍语义固化：补拍只推进 tick 账，墙钟冻结，闭合时一次校准。
//
pub struct NohzProtocol {
    /// 每核 nohz 账（固定槽位，不上堆）。
    cores: [CpuNohzLedger; MAX_CPUS],
    /// 全局事件账。
    log: EventLog,
    /// 成功闭合的全序会话计数（判据口径：进入/退出全序）。
    full_orders_ok: u64,
    /// 补拍对平成功计数（判据口径：理论拍数补投对平）。
    catchups_ok: u64,
    /// 末核保底拦截计数。
    last_core_blocks: u64,
    /// 乱序拒绝计数。
    out_of_order_blocks: u64,
    /// 预算越线立案计数。
    budget_breaches: u64,
    /// 历史最重单会话开销（ns）。
    worst_session_ns: u64,
    /// 会话开启总数。
    sessions_opened: u64,
    /// 会话正常闭合总数（含红案闭合）。
    sessions_closed: u64,
    /// 会话中止总数。
    sessions_aborted: u64,
    /// 补拍总投递拍数（跨会话累计）。
    catchup_ticks_total: u64,
    /// 推演进位轮数（半周期进位发生的次数，账面统计）。
    rounding_rounds: u64,
}

impl NohzProtocol {
    /// 建协议（MAX_CPUS 份核账全部就位）。
    pub fn new() -> NohzProtocol {
        NohzProtocol {
            cores: [
                CpuNohzLedger::new(0),
                CpuNohzLedger::new(1),
                CpuNohzLedger::new(2),
                CpuNohzLedger::new(3),
            ],
            log: EventLog::new(),
            full_orders_ok: 0,
            catchups_ok: 0,
            last_core_blocks: 0,
            out_of_order_blocks: 0,
            budget_breaches: 0,
            worst_session_ns: 0,
            sessions_opened: 0,
            sessions_closed: 0,
            sessions_aborted: 0,
            catchup_ticks_total: 0,
            rounding_rounds: 0,
        }
    }

    /// 只读借核账。
    pub fn core(&self, cpu: usize) -> Result<&CpuNohzLedger, NohzError> {
        if cpu >= MAX_CPUS {
            return Err(NohzError::BadCpu(cpu));
        }
        Ok(&self.cores[cpu])
    }

    /// 事件账只读借。
    pub fn log(&self) -> &EventLog {
        &self.log
    }

    /// 成功全序会话计数。
    pub fn full_orders_ok(&self) -> u64 {
        self.full_orders_ok
    }

    /// 补拍对平成功计数。
    pub fn catchups_ok(&self) -> u64 {
        self.catchups_ok
    }

    /// 末核保底拦截计数。
    pub fn last_core_blocks(&self) -> u64 {
        self.last_core_blocks
    }

    /// 乱序拒绝计数。
    pub fn out_of_order_blocks(&self) -> u64 {
        self.out_of_order_blocks
    }

    /// 预算越线立案计数。
    pub fn budget_breaches(&self) -> u64 {
        self.budget_breaches
    }

    /// 全局 FNV 指纹：事件账指纹再搅入各核账关键标量。
    pub fn fingerprint(&self) -> u64 {
        let mut h = self.log.fingerprint();
        let mut i = 0;
        while i < MAX_CPUS {
            let c = &self.cores[i];
            h = fnv_byte(h, (c.tick_account & 0xFF) as u8);
            h = fnv_byte(h, ((c.tick_account >> 8) & 0xFF) as u8);
            h = fnv_byte(h, (c.catchup_served & 0xFF) as u8);
            h = fnv_byte(h, (c.theory_ticks & 0xFF) as u8);
            h = fnv_byte(h, (c.halt_rounds & 0xFF) as u8);
            h = fnv_byte(h, (c.spent_ns & 0xFF) as u8);
            h = fnv_byte(h, c.phase.ordinal() as u8);
            i += 1;
        }
        h
    }

    /// 墙钟同步（仅 Periodic 态）：停发前由 clocksource 域对一次墙钟，
    /// 使停发冻结点有真实基线。非 Periodic 态调用属乱序（停发期内
    /// 禁止改写墙钟——补拍语义红线的另一道锁）。
    pub fn sync_wall(&mut self, cpu: usize, now_ns: u64) -> Result<(), NohzError> {
        self.check_cpu(cpu)?;
        if self.cores[cpu].phase != NohzPhase::Periodic {
            return Err(NohzError::OutOfOrder(self.cores[cpu].phase, NohzPhase::Periodic));
        }
        self.cores[cpu].wall_ns = now_ns;
        Ok(())
    }

    /// 会话中止总数只读。
    pub fn sessions_aborted(&self) -> u64 {
        self.sessions_aborted
    }

    // ── 内部：步数化预算记账 ─────────────────────────────────────────

    /// 记一步成本（saturating 累加）；越 20µs 硬线 → 立案 + 强制中止，
    /// 返回 Err(BudgetBreach)。
    fn charge(&mut self, cpu: usize, cost: u64, now_ns: u64) -> Result<(), NohzError> {
        let before = self.cores[cpu].spent_ns;
        let after = before.saturating_add(cost);
        self.cores[cpu].spent_ns = after;
        self.cores[cpu].session_steps = self.cores[cpu].session_steps.saturating_add(1);
        if after >= PROTOCOL_BUDGET_NS {
            self.budget_breaches = self.budget_breaches.saturating_add(1);
            let _ = self.log.push(EventKind::BudgetBreachFiled(after), now_ns, cpu as u8);
            // 强制中止（中止本身不再计费——立案优先，避免连环越线）
            self.abort_session_inner(cpu, now_ns);
            return Err(NohzError::BudgetBreach(after));
        }
        if after > self.worst_session_ns {
            self.worst_session_ns = after;
        }
        Ok(())
    }

    /// 会话中止（内部版）：从任意非 Periodic 态强制回拍。
    /// 语义对应真实内核 tick_nohz 重入失败回退周期模式。
    /// 预算账随会话清零（20µs 硬线是单会话口径）。
    fn abort_session_inner(&mut self, cpu: usize, now_ns: u64) {
        let from = self.cores[cpu].phase.ordinal();
        self.cores[cpu].phase = NohzPhase::Periodic;
        self.cores[cpu].session_closed = true;
        self.cores[cpu].spent_ns = 0;
        self.cores[cpu].session_steps = 0;
        self.sessions_aborted = self.sessions_aborted.saturating_add(1);
        let _ = self
            .log
            .push(EventKind::SessionAborted(from as u8), now_ns, cpu as u8);
    }

    /// 会话中止（对外）：核账悬空时的恢复路径，可从
    /// EnterReq/TicksHalted/ExitReq/Catchup/Closed 任一态回 Periodic。
    /// Periodic 态调用返回乱序错误（无事可中止）。
    pub fn abort_session(&mut self, cpu: usize, now_ns: u64) -> Result<(), NohzError> {
        self.check_cpu(cpu)?;
        if self.cores[cpu].phase == NohzPhase::Periodic {
            // 无事可中止：按乱序口径立案留痕（与其他越序拒绝同权）
            self.out_of_order_blocks = self.out_of_order_blocks.saturating_add(1);
            let _ = self.log.push(
                EventKind::OutOfOrderBlocked(
                    NohzPhase::Periodic.ordinal() as u8,
                    NohzPhase::Periodic.ordinal() as u8,
                ),
                now_ns,
                cpu as u8,
            );
            return Err(NohzError::OutOfOrder(
                NohzPhase::Periodic,
                NohzPhase::Periodic,
            ));
        }
        // 留痕后再清账：中止带琥珀级事件
        let from = self.cores[cpu].phase.ordinal();
        self.abort_session_inner(cpu, now_ns);
        // 从 Closed 中止的属正常收尾路径，不计中止数——上面 inner 已计，
        // 此处按来源回拨（Closed→Periodic 本就是合法边，撤回中止计数）
        if from == NohzPhase::Closed.ordinal() {
            self.sessions_aborted = self.sessions_aborted.saturating_sub(1);
        }
        Ok(())
    }

    fn check_cpu(&self, cpu: usize) -> Result<(), NohzError> {
        if cpu >= MAX_CPUS {
            return Err(NohzError::BadCpu(cpu));
        }
        Ok(())
    }

    /// const 转移裁决：合法则落态，否则乱序立案。
    fn transit(&mut self, cpu: usize, to: NohzPhase, now_ns: u64) -> Result<NohzPhase, NohzError> {
        let from = self.cores[cpu].phase;
        if !transition_allowed(from, to) {
            self.out_of_order_blocks = self.out_of_order_blocks.saturating_add(1);
            let _ = self.log.push(
                EventKind::OutOfOrderBlocked(from.ordinal() as u8, to.ordinal() as u8),
                now_ns,
                cpu as u8,
            );
            return Err(NohzError::OutOfOrder(from, to));
        }
        self.cores[cpu].phase = to;
        Ok(from)
    }

    // ── 全序五步对外 API ─────────────────────────────────────────────

    /// 第 1 步：进入请求（Periodic → EnterReq）。
    ///
    /// 流程：核号核验 → 转移裁决 → 计一步 → 六门 const 表逐位验收；
    /// 失格即走 EnterReq→Periodic 合法回退边并带首失格门序号拒绝。
    pub fn request_enter(
        &mut self,
        cpu: usize,
        gates: u32,
        now_ns: u64,
    ) -> Result<(), NohzError> {
        self.check_cpu(cpu)?;
        self.transit(cpu, NohzPhase::EnterReq, now_ns)?;
        self.charge(cpu, STEP_COST_NS, now_ns)?;
        let (pass, gate_idx) = gates_evaluate(gates);
        if pass {
            self.sessions_opened = self.sessions_opened.saturating_add(1);
            self.cores[cpu].session_closed = false;
            let _ = self.log.push(EventKind::EnterRequested, now_ns, cpu as u8);
            return Ok(());
        }
        // 门失格：走合法回退边 EnterReq→Periodic（回退不再计费）
        self.cores[cpu].phase = NohzPhase::Periodic;
        self.cores[cpu].session_closed = true;
        let _ = self.log.push(EventKind::GateDenied(gate_idx), now_ns, cpu as u8);
        Err(NohzError::GateRefused(gate_idx))
    }

    /// 第 2 步：停发生效（EnterReq → TicksHalted）。
    ///
    /// 流程：核号核验 → 独立末核保底复跑（其余核全部停发即拒）→
    /// 转移裁决 → 计一步 → 落停发时戳与冻结墙钟、快照 tick 账基线。
    pub fn confirm_halt(&mut self, cpu: usize, now_ns: u64) -> Result<(), NohzError> {
        self.check_cpu(cpu)?;
        // 独立末核保底：本核转入停发后是否再无在拍核？
        // 口径：is_halted() == false 的核算在拍（Periodic/EnterReq——
        // 两态下 tick 仍发放，保底只认"能发放 tick 的核"）。
        let mut others_halted = true;
        let mut i = 0;
        while i < MAX_CPUS {
            if i != cpu && !self.cores[i].is_halted() {
                // 在拍核存在（含协议中未停发核）
                others_halted = false;
            }
            i += 1;
        }
        if others_halted {
            self.last_core_blocks = self.last_core_blocks.saturating_add(1);
            let _ = self
                .log
                .push(EventKind::LastCoreBlocked, now_ns, cpu as u8);
            // 走合法回退边 EnterReq→Periodic
            self.cores[cpu].phase = NohzPhase::Periodic;
            self.cores[cpu].session_closed = true;
            return Err(NohzError::LastCoreGuard);
        }
        self.transit(cpu, NohzPhase::TicksHalted, now_ns)?;
        self.charge(cpu, STEP_COST_NS, now_ns)?;
        let c = &mut self.cores[cpu];
        c.enter_ns = now_ns;
        c.wall_frozen_ns = c.wall_ns;
        c.tick_pre_halt = c.tick_account;
        c.catchup_served = 0;
        c.theory_ticks = 0;
        let _ = self.log.push(EventKind::HaltArmed(now_ns), now_ns, cpu as u8);
        Ok(())
    }

    /// 第 3 步：退出请求（TicksHalted → ExitReq）。
    ///
    /// 流程：核号核验 → 态预检（非 TicksHalted 直接乱序拒绝，先于
    /// 驻留检查——全序裁决优先）→ 停发跨度预检（不足最小驻留直接
    /// 拒绝，不落态）→ 转移裁决 → 计一步 → 落退出时戳。
    pub fn request_exit(&mut self, cpu: usize, now_ns: u64) -> Result<(), NohzError> {
        self.check_cpu(cpu)?;
        if self.cores[cpu].phase != NohzPhase::TicksHalted {
            let from = self.cores[cpu].phase;
            self.out_of_order_blocks = self.out_of_order_blocks.saturating_add(1);
            let _ = self.log.push(
                EventKind::OutOfOrderBlocked(
                    from.ordinal() as u8,
                    NohzPhase::ExitReq.ordinal() as u8,
                ),
                now_ns,
                cpu as u8,
            );
            return Err(NohzError::OutOfOrder(from, NohzPhase::ExitReq));
        }
        let enter_ns = self.cores[cpu].enter_ns;
        let span = now_ns.saturating_sub(enter_ns);
        if span < MIN_RESIDENCE_NS {
            let _ = self
                .log
                .push(EventKind::ShortStayRejected(span), now_ns, cpu as u8);
            return Err(NohzError::TooShort(span));
        }
        self.transit(cpu, NohzPhase::ExitReq, now_ns)?;
        self.charge(cpu, STEP_COST_NS, now_ns)?;
        self.cores[cpu].exit_ns = now_ns;
        let _ = self.log.push(EventKind::ExitRequested(now_ns), now_ns, cpu as u8);
        Ok(())
    }

    /// 第 4 步：转入补拍（ExitReq → Catchup）+ 理论拍数整数推演。
    ///
    /// 流程：核号核验 → 推演（exit<enter → ClockBackward）→ 越限预检
    /// （theory > MAX_CATCHUP_TICKS → CatchupOver，不落态）→ 转移裁决
    /// → 计一步 → 推演账落核账并记事件。
    pub fn begin_catchup(&mut self, cpu: usize, now_ns: u64) -> Result<u64, NohzError> {
        self.check_cpu(cpu)?;
        let enter_ns = self.cores[cpu].enter_ns;
        let exit_ns = self.cores[cpu].exit_ns;
        let acct = HalspanAccount::derive(enter_ns, exit_ns)?;
        if acct.theory_ticks > MAX_CATCHUP_TICKS {
            return Err(NohzError::CatchupOver(acct.theory_ticks));
        }
        self.transit(cpu, NohzPhase::Catchup, now_ns)?;
        self.charge(cpu, STEP_COST_NS, now_ns)?;
        if acct.rounded_up {
            self.rounding_rounds = self.rounding_rounds.saturating_add(1);
        }
        let theory = acct.theory_ticks;
        self.cores[cpu].theory_ticks = theory;
        self.cores[cpu].halspan = Some(acct);
        let _ = self
            .log
            .push(EventKind::TheoryComputed(theory), now_ns, cpu as u8);
        Ok(theory)
    }

    /// 第 4.5 步：单拍补投（仅 Catchup 态）。
    ///
    /// 补拍语义固化（判据点名）：
    ///   - 只推进 tick 账（tick_account += 1 拍），墙钟 wall_ns 纹丝不动
    ///     （杜绝墙钟跳变）；
    ///   - 每拍计补投成本入会话预算账；
    ///   - 投满理论拍数后再投 → CatchupMismatch（超投同样是账面错）。
    pub fn serve_catchup_tick(&mut self, cpu: usize, now_ns: u64) -> Result<u64, NohzError> {
        self.check_cpu(cpu)?;
        if self.cores[cpu].phase != NohzPhase::Catchup {
            self.out_of_order_blocks = self.out_of_order_blocks.saturating_add(1);
            let _ = self.log.push(
                EventKind::OutOfOrderBlocked(
                    self.cores[cpu].phase.ordinal() as u8,
                    NohzPhase::Catchup.ordinal() as u8,
                ),
                now_ns,
                cpu as u8,
            );
            return Err(NohzError::OutOfOrder(self.cores[cpu].phase, NohzPhase::Catchup));
        }
        let theory = self.cores[cpu].theory_ticks;
        let served = self.cores[cpu].catchup_served;
        if served >= theory {
            // 超投立案：已投(+1) 对 理论
            let _ = self.log.push(
                EventKind::CatchupBroken(served.wrapping_add(1), theory),
                now_ns,
                cpu as u8,
            );
            return Err(NohzError::CatchupMismatch(served.wrapping_add(1), theory));
        }
        self.charge(cpu, CATCHUP_TICK_COST_NS, now_ns)?;
        let c = &mut self.cores[cpu];
        c.catchup_served = c.catchup_served.saturating_add(1);
        c.tick_account = c.tick_account.saturating_add(1);
        self.catchup_ticks_total = self.catchup_ticks_total.saturating_add(1);
        let served_now = c.catchup_served;
        let _ = self
            .log
            .push(EventKind::CatchupTick(served_now), now_ns, cpu as u8);
        Ok(served_now)
    }

    /// 第 5 步：补拍闭合对平（Catchup → Closed）。
    ///
    /// 流程：核号核验 → 转移裁决 → 计一步 → 对平断言（已投 == 理论 且
    /// tick 账增量 == 理论）；不平即红案 CatchupBroken，但会话仍闭合
    /// （账面留红，health 转红——真实内核对应 nohz 退出时的对账告警）。
    pub fn complete_catchup(&mut self, cpu: usize, now_ns: u64) -> Result<(), NohzError> {
        self.check_cpu(cpu)?;
        self.transit(cpu, NohzPhase::Closed, now_ns)?;
        self.charge(cpu, STEP_COST_NS, now_ns)?;
        let c = &self.cores[cpu];
        let served = c.catchup_served;
        let theory = c.theory_ticks;
        if c.catchup_reconciled() {
            self.catchups_ok = self.catchups_ok.saturating_add(1);
            let _ = self.log.push(
                EventKind::CatchupReconciled(served, theory),
                now_ns,
                cpu as u8,
            );
            return Ok(());
        }
        let _ = self
            .log
            .push(EventKind::CatchupBroken(served, theory), now_ns, cpu as u8);
        Err(NohzError::CatchupMismatch(served, theory))
    }

    /// 第 6 步：回拍（Closed → Periodic）+ 墙钟校准 + 会话收账。
    ///
    /// 墙钟校准语义：补拍全程墙钟冻结在停发点（wall_frozen_ns），
    /// 回拍瞬间用调用方提供的真实时戳一次校准；校准差值 = 真实时戳 −
    /// 冻结点。补拍本身从未触碰墙钟，故校准是唯一一次墙钟写入，
    /// "补拍不触发墙钟跳变"由构造保证。
    pub fn close_session(&mut self, cpu: usize, now_ns: u64) -> Result<(), NohzError> {
        self.check_cpu(cpu)?;
        self.transit(cpu, NohzPhase::Periodic, now_ns)?;
        let c = &mut self.cores[cpu];
        let frozen = c.wall_frozen_ns;
        // 校准差值：有符号存账（真实时戳理论上 ≥ 冻结点；倒退即负值留痕）
        let delta = (now_ns as i64).wrapping_sub(frozen as i64);
        c.calibrate_delta_ns = delta;
        c.wall_ns = now_ns;
        c.session_closed = true;
        c.halt_rounds = c.halt_rounds.saturating_add(1);
        // 会话收账：本会话开销先定格，再清预算账（硬线是单会话口径，
        // 清账让下一会话从零起算；worst_session_ns 留全历史最重值）
        let spent = c.spent_ns;
        let reconciled = c.catchup_reconciled();
        c.spent_ns = 0;
        c.session_steps = 0;
        if spent > self.worst_session_ns {
            self.worst_session_ns = spent;
        }
        self.sessions_closed = self.sessions_closed.saturating_add(1);
        if reconciled && spent < PROTOCOL_BUDGET_NS {
            self.full_orders_ok = self.full_orders_ok.saturating_add(1);
        }
        let _ = self.log.push(EventKind::SessionClosed(spent), now_ns, cpu as u8);
        let _ = self
            .log
            .push(EventKind::WallCalibrated(delta as u64), now_ns, cpu as u8);
        Ok(())
    }
}

// ─────────────────────────────────────────────────────────────────────────
// §8  预算稽核（步数化预算复核子系统）
// ─────────────────────────────────────────────────────────────────────────

/// 单核预算稽核结果。
#[derive(Clone, Debug)]
pub struct BudgetAuditRow {
    /// 核号。
    pub cpu: usize,
    /// 会话累计开销（ns）。
    pub spent_ns: u64,
    /// 会话步数（协议步 + 补拍拍）。
    pub steps: u64,
    /// 是否在预算内。
    pub within: bool,
    /// 余量（ns，saturating）。
    pub headroom_ns: u64,
    /// 中文一句话。
    pub note: String,
}

/// 全局预算稽核账。
#[derive(Clone, Debug)]
pub struct BudgetAudit {
    /// 逐核稽核行。
    pub rows: Vec<BudgetAuditRow>,
    /// 历史最重单会话开销（ns）。
    pub worst_ns: u64,
    /// 预算越线立案总数。
    pub breaches: u64,
    /// 预算硬线（ns，复读常量便于对账）。
    pub budget_ns: u64,
    /// 单步成本（ns）。
    pub step_cost_ns: u64,
    /// 单补拍拍成本（ns）。
    pub catchup_cost_ns: u64,
}

impl BudgetAudit {
    /// 对协议账做一次全核预算稽核。
    pub fn audit(p: &NohzProtocol) -> BudgetAudit {
        let mut rows: Vec<BudgetAuditRow> = Vec::new();
        let mut i = 0;
        while i < MAX_CPUS {
            let c = &p.cores[i];
            let within = c.within_budget();
            let headroom_ns = PROTOCOL_BUDGET_NS.saturating_sub(c.spent_ns);
            let note = if within {
                format!(
                    "核{}开销{}ns/{}步，余量{}ns，在线",
                    i, c.spent_ns, c.session_steps, headroom_ns
                )
            } else {
                format!(
                    "核{}开销{}ns越{}ns硬线，已立案",
                    i, c.spent_ns, PROTOCOL_BUDGET_NS
                )
            };
            rows.push(BudgetAuditRow {
                cpu: i,
                spent_ns: c.spent_ns,
                steps: c.session_steps,
                within,
                headroom_ns,
                note,
            });
            i += 1;
        }
        BudgetAudit {
            rows,
            worst_ns: p.worst_session_ns,
            breaches: p.budget_breaches,
            budget_ns: PROTOCOL_BUDGET_NS,
            step_cost_ns: STEP_COST_NS,
            catchup_cost_ns: CATCHUP_TICK_COST_NS,
        }
    }

    /// 稽核是否全绿（无越线行且无立案）。
    pub fn all_clean(&self) -> bool {
        let mut clean = self.breaches == 0;
        let mut i = 0;
        while i < self.rows.len() {
            if !self.rows[i].within {
                clean = false;
            }
            i += 1;
        }
        clean
    }

    /// 稽核摘要（逐行中文）。
    pub fn summarize(&self) -> String {
        let mut s = format!(
            "预算稽核：硬线{}ns，最重会话{}ns，越线立案{}起",
            self.budget_ns, self.worst_ns, self.breaches
        );
        let mut i = 0;
        while i < self.rows.len() {
            let _ = write!(s, "\n  {}", self.rows[i].note);
            i += 1;
        }
        s
    }
}

// ─────────────────────────────────────────────────────────────────────────
// §9  health() 三色体检 + 全局摘要
// ─────────────────────────────────────────────────────────────────────────

/// 健康级（0=绿 1=黄 2=红）。
pub const HEALTH_GREEN: u8 = 0;
/// 健康级：黄（有琥珀事件但无红案）。
pub const HEALTH_AMBER: u8 = 1;
/// 健康级：红（存在红案或悬空会话）。
pub const HEALTH_RED: u8 = 2;

/// 健康体检结论。
#[derive(Clone, Debug)]
pub struct HealthReport {
    /// 级别（0/1/2）。
    pub level: u8,
    /// 级别中文。
    pub level_text: &'static str,
    /// 红案数。
    pub red: u64,
    /// 琥珀数。
    pub amber: u64,
    /// 悬空会话数。
    pub open_sessions: usize,
    /// 停发口径核数。
    pub halted_cores: usize,
    /// 全序成功数。
    pub full_orders_ok: u64,
    /// 补拍对平成功数。
    pub catchups_ok: u64,
    /// 补拍总拍数。
    pub catchup_ticks_total: u64,
    /// 推演进位轮数。
    pub rounding_rounds: u64,
    /// 全局指纹（hex）。
    pub fingerprint: String,
    /// 中文摘要（多行）。
    pub summary: String,
}

impl NohzProtocol {
    /// 三色体检：绿=无红无黄无悬空；黄=仅琥珀；红=有红案或悬空会话。
    pub fn health(&self) -> HealthReport {
        // 悬空会话：session_closed == false 的核
        let mut open_sessions = 0usize;
        let mut halted_cores = 0usize;
        let mut i = 0;
        while i < MAX_CPUS {
            if !self.cores[i].session_closed {
                open_sessions += 1;
            }
            if self.cores[i].is_halted() {
                halted_cores += 1;
            }
            i += 1;
        }
        let red = self.log.red_count();
        let amber = self.log.amber_count();
        let level = if red > 0 || open_sessions > 0 {
            HEALTH_RED
        } else if amber > 0 {
            HEALTH_AMBER
        } else {
            HEALTH_GREEN
        };
        let level_text = match level {
            HEALTH_GREEN => "绿",
            HEALTH_AMBER => "黄",
            _ => "红",
        };
        let summary = format!(
            "nohz协议体检[{}]：红{}起/黄{}起，悬空会话{}，停发口径核{}，\
             全序{}次/补拍对平{}次/补拍总拍{}，推演进位{}轮，指纹{:016x}",
            level_text,
            red,
            amber,
            open_sessions,
            halted_cores,
            self.full_orders_ok,
            self.catchups_ok,
            self.catchup_ticks_total,
            self.rounding_rounds,
            self.fingerprint()
        );
        HealthReport {
            level,
            level_text,
            red,
            amber,
            open_sessions,
            halted_cores,
            full_orders_ok: self.full_orders_ok,
            catchups_ok: self.catchups_ok,
            catchup_ticks_total: self.catchup_ticks_total,
            rounding_rounds: self.rounding_rounds,
            fingerprint: format!("{:016x}", self.fingerprint()),
            summary,
        }
    }

    /// 全局指纹只读。
    pub fn fp(&self) -> u64 {
        self.fingerprint()
    }

    /// 历史最重单会话开销只读（<20µs 断言用）。
    pub fn worst_session_ns(&self) -> u64 {
        self.worst_session_ns
    }

    /// 会话闭合总数只读。
    pub fn sessions_closed(&self) -> u64 {
        self.sessions_closed
    }

    /// 推演账只读借（最近一次推演）。
    pub fn halspan_of(&self, cpu: usize) -> Result<Option<HalspanAccount>, NohzError> {
        self.check_cpu(cpu)?;
        Ok(self.cores[cpu].halspan)
    }
}

// ─────────────────────────────────────────────────────────────────────────
// §10  双跑确定性演练（判据口径：全序 10/10、补拍对平 10/10）
// ─────────────────────────────────────────────────────────────────────────

/// LCG 常量（Knuth MMIX：multiplier 与 increment 的公设参数）。
const LCG_MULT: u64 = 6364136223846793005;
/// LCG 增量（Knuth MMIX 公设参数）。
const LCG_INC: u64 = 1442695040888963407;

/// 下一随机数（wrapping 显性）。
const fn lcg_next(x: u64) -> u64 {
    x.wrapping_mul(LCG_MULT).wrapping_add(LCG_INC)
}

/// 双跑演练报告。
#[derive(Clone, Debug)]
pub struct DrillReport {
    /// 演练轮数（判据点名 10）。
    pub rounds: usize,
    /// 全序成功轮数（进入/退出全序走通）。
    pub full_order_ok: usize,
    /// 补拍对平成功轮数（补拍账 == 理论账）。
    pub catchup_ok: usize,
    /// 跨轮补拍总拍数。
    pub theory_ticks_total: u64,
    /// 跨轮推演进位轮数（半周期进位覆盖次数）。
    pub rounding_rounds: u64,
    /// 预算越线立案数。
    pub budget_breaches: u64,
    /// 末核保底拦截数（演练主路径应为 0）。
    pub last_core_blocks: u64,
    /// 乱序拒绝数（演练主路径应为 0）。
    pub out_of_order_blocks: u64,
    /// 预算稽核是否全清。
    pub budget_clean: bool,
    /// 健康级（0=绿）。
    pub health_level: u8,
    /// 全局指纹（hex，双跑对拍键）。
    pub fingerprint: String,
    /// 中文结论摘要。
    pub summary: String,
}

impl DrillReport {
    /// 判据 J1 三断言一次复核：全序 10/10、补拍对平 10/10、预算内。
    pub const fn passes_j1(&self) -> bool {
        self.full_order_ok == DRILL_ROUNDS
            && self.catchup_ok == DRILL_ROUNDS
            && self.budget_breaches == 0
            && self.last_core_blocks == 0
            && self.out_of_order_blocks == 0
            && self.budget_clean
            && self.health_level == HEALTH_GREEN
    }
}

/// 最小全序会话执行器（演练与注入共用的"走全序五步+闭合"底座）。
///
/// 时戳模型：t0 为停发起点，退出时戳 = t0 + span_ns；会话闭合用
/// t0 + span_ns + CLOSE_GAP_NS（闭合与退出之间留真实时间流逝，
/// 供墙钟校准差值非零——体现"补拍不推进墙钟、闭合一次校准"）。
/// 闭合间隔写死 1_000 ns（远小于 tick 周期，不影响拍数推演）。
const CLOSE_GAP_NS: u64 = 1_000;

fn run_minimal_session(
    p: &mut NohzProtocol,
    cpu: usize,
    t0: u64,
    span_ns: u64,
) -> Result<u64, NohzError> {
    p.sync_wall(cpu, t0)?; // 停发前墙钟对基线（冻结点 = t0）
    p.request_enter(cpu, GATES_ALL, t0)?;
    p.confirm_halt(cpu, t0)?;
    let t_exit = t0 + span_ns;
    p.request_exit(cpu, t_exit)?;
    let theory = p.begin_catchup(cpu, t_exit)?;
    let mut served = 0;
    while served < theory {
        served = p.serve_catchup_tick(cpu, t_exit + served)?;
    }
    p.complete_catchup(cpu, t_exit + served)?;
    p.close_session(cpu, t_exit + served + CLOSE_GAP_NS)?;
    Ok(theory)
}

/// 双跑确定性演练：单实例内连跑 DRILL_ROUNDS 轮完整协议。
///
/// 轮次剧本（LCG 驱动，同 seed 必然同剧本——确定性根基）：
///   - 轮 r 停发核 = r % 2（在 0/1 间交替；核 2/3 全程在拍，
///     保底永不触发——保底走注入面专测）；
///   - 轮 r 停发跨度 = period × (3 + r%6) + (r%2) × 半周期，
///     理论拍数落在 3..=8，其中偶数轮触发半周期进位（覆盖舍入语义）；
///   - 时戳随轮单调递增（虚拟时钟无倒退）。
pub fn run_full_drill(seed: u64) -> DrillReport {
    let mut p = NohzProtocol::new();
    let mut rng = seed ^ 0x9E37_79B9_7F4A_7C15; // 黄金比例常数作种子混淆
    let mut t: u64 = 0;
    let mut full_order_ok = 0usize;
    let mut catchup_ok = 0usize;
    let mut theory_ticks_total: u64 = 0;
    let mut r = 0usize;
    while r < DRILL_ROUNDS {
        rng = lcg_next(rng);
        let cpu = r % 2;
        let periods = 3 + (r % 6) as u64;
        let span_ns = TICK_PERIOD_NS
            .saturating_mul(periods)
            .saturating_add(if r % 2 == 0 { HALF_PERIOD_NS } else { 0 });
        // 时间推进：会话间留 3 拍真实在拍期（tick 账由调用方语义维护，
        // 此处只推协议时戳，不喂账）
        t = t.saturating_add(TICK_PERIOD_NS.saturating_mul(3));
        match run_minimal_session(&mut p, cpu, t, span_ns) {
            Ok(theory) => {
                full_order_ok += 1;
                theory_ticks_total = theory_ticks_total.saturating_add(theory);
                if p.core(cpu).map(|c| c.catchup_reconciled()).unwrap_or(false) {
                    catchup_ok += 1;
                }
            }
            Err(_) => { // 单轮失败不炸演练：计入缺口，报告层判定
            }
        }
        r += 1;
    }
    let audit = BudgetAudit::audit(&p);
    let hp = p.health();
    let summary = format!(
        "双跑演练 seed={:#x}：全序{}/{}，补拍对平{}/{}，补拍总拍{}，\
         预算稽核{}，体检[{}]",
        seed,
        full_order_ok,
        DRILL_ROUNDS,
        catchup_ok,
        DRILL_ROUNDS,
        theory_ticks_total,
        if audit.all_clean() { "全清" } else { "有案" },
        hp.level_text
    );
    DrillReport {
        rounds: DRILL_ROUNDS,
        full_order_ok,
        catchup_ok,
        theory_ticks_total,
        rounding_rounds: hp.rounding_rounds,
        budget_breaches: p.budget_breaches(),
        last_core_blocks: p.last_core_blocks(),
        out_of_order_blocks: p.out_of_order_blocks(),
        budget_clean: audit.all_clean(),
        health_level: hp.level,
        fingerprint: hp.fingerprint,
        summary,
    }
}

/// 确定性对拍：同 seed 双跑，指纹必须逐位恒等（FNV-1a 64）。
pub fn verify_determinism(seed: u64) -> bool {
    let a = run_full_drill(seed);
    let b = run_full_drill(seed);
    a.fingerprint == b.fingerprint
        && a.full_order_ok == b.full_order_ok
        && a.catchup_ok == b.catchup_ok
        && a.theory_ticks_total == b.theory_ticks_total
}

// ─────────────────────────────────────────────────────────────────────────
// §11  注入面（乱序 / 补拍对不平 / 全核停发 / 门失格，逐项检出）
// ─────────────────────────────────────────────────────────────────────────

/// 注入种类。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InjectKind {
    /// I0 乱序注入：Periodic 态直接请求退出。
    OutOfOrder,
    /// I1 补拍对不平注入：故意少投一拍就闭合。
    CatchupMismatch,
    /// I2 全核停发注入：其余核全部停发后末核请求停发。
    AllCoreHalt,
    /// I3 门失格注入：掩码缺 G1 无 pending 回调位。
    GateRefuse,
    /// I4 预算洪水注入：反复门失格尝试耗尽 20µs 预算窗。
    BudgetFlood,
}

impl InjectKind {
    /// 种类序号（事件落账用）。
    pub const fn ordinal(self) -> u8 {
        match self {
            InjectKind::OutOfOrder => 0,
            InjectKind::CatchupMismatch => 1,
            InjectKind::AllCoreHalt => 2,
            InjectKind::GateRefuse => 3,
            InjectKind::BudgetFlood => 4,
        }
    }

    /// 中文名。
    pub const fn text(self) -> &'static str {
        match self {
            InjectKind::OutOfOrder => "乱序注入",
            InjectKind::CatchupMismatch => "补拍对不平注入",
            InjectKind::AllCoreHalt => "全核停发注入",
            InjectKind::GateRefuse => "门失格注入",
            InjectKind::BudgetFlood => "预算洪水注入",
        }
    }
}

/// 注入检出报告。
#[derive(Clone, Debug)]
pub struct InjectReport {
    /// 注入种类。
    pub kind: InjectKind,
    /// 是否按预期检出（检出 = 返回预期错误码 + 事件账留痕）。
    pub caught: bool,
    /// 观测到的诊断码（未检出为 0）。
    pub observed_code: u16,
    /// 中文结论。
    pub note: String,
}

/// 注入执行器：每类注入在全新协议实例上复现一次攻击路径，
/// 并断言防线按码检出。报告 caught=false 即防线失守。
pub fn run_injection(kind: InjectKind) -> InjectReport {
    match kind {
        InjectKind::OutOfOrder => {
            let mut p = NohzProtocol::new();
            // 攻击：Periodic 态直接 request_exit（全序上无此边）
            let res = p.request_exit(0, 1_000);
            let caught = match &res {
                Err(e) => {
                    e.code() == NohzError::OutOfOrder(NohzPhase::Periodic, NohzPhase::ExitReq)
                        .code()
                        && p.out_of_order_blocks() == 1
                        && p.log().count_kind(|k| {
                            matches!(k, EventKind::OutOfOrderBlocked(_, _))
                        }) == 1
                }
                Ok(()) => false,
            };
            let code = match &res {
                Err(e) => e.code(),
                Ok(()) => 0,
            };
            InjectReport {
                kind,
                caught,
                observed_code: code,
                note: format!(
                    "乱序注入：Periodic→ExitReq 被拒，诊断码 {:#06x}，事件账留痕",
                    code
                ),
            }
        }
        InjectKind::CatchupMismatch => {
            let mut p = NohzProtocol::new();
            // 攻击：正常进入停发，推演出理论拍数后故意少投一拍就闭合
            let t0: u64 = TICK_PERIOD_NS * 3;
            let span = TICK_PERIOD_NS * 5;
            let res = (|| -> Result<u64, NohzError> {
                p.request_enter(0, GATES_ALL, t0)?;
                p.confirm_halt(0, t0)?;
                p.request_exit(0, t0 + span)?;
                let theory = p.begin_catchup(0, t0 + span)?;
                // 少投一拍：theory ≥ 1 恒成立（span ≥ 最小驻留 ≥ 2 拍）
                let mut served = 0;
                while served + 1 < theory {
                    served = p.serve_catchup_tick(0, t0 + span + served)?;
                }
                p.complete_catchup(0, t0 + span + served)?;
                Ok(theory)
            })();
            let caught = match &res {
                Err(e) => {
                    e.code() == NohzError::CatchupMismatch(0, 0).code()
                        && p.log().count_kind(|k| matches!(k, EventKind::CatchupBroken(_, _))) >= 1
                }
                Ok(_) => false,
            };
            let code = match &res {
                Err(e) => e.code(),
                Ok(_) => 0,
            };
            InjectReport {
                kind,
                caught,
                observed_code: code,
                note: format!(
                    "补拍对不平注入：少投一拍闭合被红案拦截，诊断码 {:#06x}",
                    code
                ),
            }
        }
        InjectKind::AllCoreHalt => {
            let mut p = NohzProtocol::new();
            // 攻击：核 1/2/3 依次真实进入停发（留停在 TicksHalted），
            // 末核 0 再请求停发 → 全核停发，保底必须拦截
            let mut setup_ok = true;
            let mut c = 1;
            while c < MAX_CPUS {
                let t0 = TICK_PERIOD_NS.saturating_mul(3 + c as u64);
                if p.request_enter(c, GATES_ALL, t0).is_err()
                    || p.confirm_halt(c, t0).is_err()
                {
                    setup_ok = false;
                }
                c += 1;
            }
            // 末核 0：掩码带全门（含 G3 位——伪造门承诺），
            // 协议在 confirm_halt 处独立复跑保底，不受门位伪造影响
            let res = (|| -> Result<(), NohzError> {
                p.request_enter(0, GATES_ALL, TICK_PERIOD_NS)?;
                p.confirm_halt(0, TICK_PERIOD_NS)
            })();
            let caught = setup_ok
                && match &res {
                    Err(e) => {
                        e.code() == NohzError::LastCoreGuard.code()
                            && p.last_core_blocks() == 1
                            && p.log()
                                .count_kind(|k| matches!(k, EventKind::LastCoreBlocked))
                                == 1
                    }
                    Ok(()) => false,
                };
            let code = match &res {
                Err(e) => e.code(),
                Ok(()) => 0,
            };
            InjectReport {
                kind,
                caught,
                observed_code: code,
                note: format!(
                    "全核停发注入：末核停发被独立保底拦截（门位伪造无效），诊断码 {:#06x}",
                    code
                ),
            }
        }
        InjectKind::GateRefuse => {
            let mut p = NohzProtocol::new();
            // 攻击：掩码缺 G1（无 pending 回调）位
            let gates = GATES_ALL & !GATE_NO_PENDING;
            let res = p.request_enter(0, gates, 1_000);
            let caught = match &res {
                Err(e) => {
                    e.code() == NohzError::GateRefused(1).code()
                        && p.log()
                            .count_kind(|k| matches!(k, EventKind::GateDenied(1)))
                            == 1
                }
                Ok(()) => false,
            };
            let code = match &res {
                Err(e) => e.code(),
                Ok(()) => 0,
            };
            InjectReport {
                kind,
                caught,
                observed_code: code,
                note: format!("门失格注入：缺 G1 位被首失格门拒绝，诊断码 {:#06x}", code),
            }
        }
        InjectKind::BudgetFlood => {
            let mut p = NohzProtocol::new();
            // 攻击：每次尝试都门失格（缺 G0 位），单步成本持续入账；
            // 预算窗内未闭合的尝试开销不清零，累计 17 步（17×1200=20400ns）
            // 必越 20µs 硬线 → BudgetBreach 立案 + 会话强制中止回拍。
            let gates = GATES_ALL & !GATE_SINGLE_TASK;
            let mut attempts = 0usize;
            let mut breached = false;
            let mut code = 0u16;
            while attempts < 64 {
                attempts += 1;
                match p.request_enter(0, gates, attempts as u64 * 10) {
                    Err(NohzError::BudgetBreach(spent)) => {
                        breached = true;
                        code = NohzError::BudgetBreach(spent).code();
                        break;
                    }
                    _ => {}
                }
            }
            let caught = breached
                && p.budget_breaches() == 1
                && p.core(0).map(|c| c.phase == NohzPhase::Periodic).unwrap_or(false)
                && p.core(0).map(|c| c.spent_ns == 0).unwrap_or(false)
                && p.log()
                    .count_kind(|k| matches!(k, EventKind::BudgetBreachFiled(_)))
                    == 1;
            InjectReport {
                kind,
                caught,
                observed_code: code,
                note: format!(
                    "预算洪水注入：{}次尝试耗尽预算窗，越线立案 {:#06x}，会话已强制回拍",
                    attempts, code
                ),
            }
        }
    }
}

/// 全注入面扫描：五类注入逐一执行，全部检出才真。
pub fn injection_sweep() -> [InjectReport; 5] {
    [
        run_injection(InjectKind::OutOfOrder),
        run_injection(InjectKind::CatchupMismatch),
        run_injection(InjectKind::AllCoreHalt),
        run_injection(InjectKind::GateRefuse),
        run_injection(InjectKind::BudgetFlood),
    ]
}

// ─────────────────────────────────────────────────────────────────────────
// §12  测试（仅 #[cfg(test)] 链入 std；内核路径保持零 unwrap/panic）
// ─────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── J1 判据三断言 ────────────────────────────────────────────────

    /// J1-①：进入/退出全序 10/10 次。
    #[test]
    fn j1_full_order_10_of_10() {
        let rep = run_full_drill(0x4E0B_7F10);
        assert_eq!(rep.rounds, DRILL_ROUNDS);
        assert_eq!(rep.full_order_ok, DRILL_ROUNDS, "全序轮数必须 10/10");
        assert!(rep.passes_j1(), "J1 综合断言失败：{}", rep.summary);
    }

    /// J1-②：退出补拍正确——停发期理论拍数补投断言 10/10 次
    /// （补拍语义固化：补拍账 == 理论账逐轮对平）。
    #[test]
    fn j1_catchup_reconcile_10_of_10() {
        let rep = run_full_drill(0x4E0B_7F10);
        assert_eq!(rep.catchup_ok, DRILL_ROUNDS, "补拍对平必须 10/10");
        assert!(rep.theory_ticks_total > 0);
    }

    /// J1-③：协议时长 <20µs 断言——逐会话闭合开销均低于硬线，
    /// 历史最重会话同样低于硬线（含上限补拍 46 拍的极端会话）。
    #[test]
    fn j1_protocol_budget_under_20us() {
        // 常规 10 轮演练
        let mut p = NohzProtocol::new();
        let mut t: u64 = 0;
        let mut r = 0;
        while r < DRILL_ROUNDS {
            let cpu = r % 2;
            let periods = 3 + (r % 6) as u64;
            let span = TICK_PERIOD_NS * periods
                + if r % 2 == 0 { HALF_PERIOD_NS } else { 0 };
            t += TICK_PERIOD_NS * 3;
            run_minimal_session(&mut p, cpu, t, span).unwrap();
            r += 1;
        }
        assert!(
            p.worst_session_ns() < PROTOCOL_BUDGET_NS,
            "最重会话 {}ns 必须小于 {}ns",
            p.worst_session_ns(),
            PROTOCOL_BUDGET_NS
        );
        // 极端会话：46 拍补拍（预算推演的满载点）
        let mut p2 = NohzProtocol::new();
        let span46 = TICK_PERIOD_NS * 46; // 理论 46 拍（上限）
        run_minimal_session(&mut p2, 0, TICK_PERIOD_NS * 3, span46).unwrap();
        assert!(p2.worst_session_ns() < PROTOCOL_BUDGET_NS);
        // 闭合事件落账的净开销同样逐一验线
        let closed_events = p2.log().count_kind(|k| matches!(k, EventKind::SessionClosed(_)));
        assert_eq!(closed_events, 1);
    }

    // ── 理论拍数整数推演 ─────────────────────────────────────────────

    /// 推演账：整除/舍入/恒等式/倒退四类用例。
    #[test]
    fn halspan_integer_derivation() {
        // 整除：span = 5 拍整 → theory = 5，无进位，余数 0
        let a = HalspanAccount::derive(0, TICK_PERIOD_NS * 5).unwrap();
        assert_eq!(a.quotient, 5);
        assert_eq!(a.remainder_ns, 0);
        assert!(!a.rounded_up);
        assert_eq!(a.theory_ticks, 5);
        assert!(a.reconciles(), "恒等式必须自证成立");

        // 半周期进位：余数恰等于 period/2 → 进一
        let b = HalspanAccount::derive(0, TICK_PERIOD_NS * 5 + HALF_PERIOD_NS).unwrap();
        assert_eq!(b.quotient, 5);
        assert!(b.rounded_up);
        assert_eq!(b.theory_ticks, 6);

        // 余数差 1ns 不到半周期 → 不进位
        let c = HalspanAccount::derive(0, TICK_PERIOD_NS * 5 + HALF_PERIOD_NS - 1).unwrap();
        assert!(!c.rounded_up);
        assert_eq!(c.theory_ticks, 5);

        // 余数差 1ns 过半周期 → 进位
        let d = HalspanAccount::derive(0, TICK_PERIOD_NS * 5 + HALF_PERIOD_NS + 1).unwrap();
        assert!(d.rounded_up);
        assert_eq!(d.theory_ticks, 6);

        // 非零起点同样成立
        let e = HalspanAccount::derive(1_000_000, 1_000_000 + TICK_PERIOD_NS * 3).unwrap();
        assert_eq!(e.theory_ticks, 3);
        assert!(e.reconciles());

        // 时钟倒退 → ClockBackward，回溯差值正确
        match HalspanAccount::derive(100, 50) {
            Err(NohzError::ClockBackward(delta)) => assert_eq!(delta, 50),
            other => panic!("应报 ClockBackward，实际 {:?}", other),
        }
    }

    /// 推演账摘要非空且含关键数字。
    #[test]
    fn halspan_summary_readable() {
        let a = HalspanAccount::derive(0, TICK_PERIOD_NS * 5 + HALF_PERIOD_NS).unwrap();
        let s = a.summarize();
        assert!(s.contains("理论6拍"), "摘要应含理论拍数：{}", s);
        assert!(s.contains("复核=true"), "摘要应含恒等式复核：{}", s);
    }

    // ── 六态全序与乱序拒绝 ───────────────────────────────────────────

    /// 全序合法环路逐边验证（const 转移表）。
    #[test]
    fn transition_table_complete() {
        assert!(transition_allowed(NohzPhase::Periodic, NohzPhase::EnterReq));
        assert!(transition_allowed(NohzPhase::EnterReq, NohzPhase::TicksHalted));
        assert!(transition_allowed(NohzPhase::EnterReq, NohzPhase::Periodic)); // 回退边
        assert!(transition_allowed(NohzPhase::TicksHalted, NohzPhase::ExitReq));
        assert!(transition_allowed(NohzPhase::ExitReq, NohzPhase::Catchup));
        assert!(transition_allowed(NohzPhase::Catchup, NohzPhase::Closed));
        assert!(transition_allowed(NohzPhase::Closed, NohzPhase::Periodic));
        // 乱序边一票否决
        assert!(!transition_allowed(NohzPhase::Periodic, NohzPhase::ExitReq));
        assert!(!transition_allowed(NohzPhase::TicksHalted, NohzPhase::Periodic));
        assert!(!transition_allowed(NohzPhase::Closed, NohzPhase::EnterReq));
        assert!(!transition_allowed(NohzPhase::Catchup, NohzPhase::TicksHalted));
    }

    /// 乱序注入路径：五种越序尝试全部带码拒绝并立案。
    #[test]
    fn out_of_order_rejected_with_code() {
        let mut p = NohzProtocol::new();
        // Periodic → ExitReq
        match p.request_exit(0, 1_000) {
            Err(NohzError::OutOfOrder(f, to)) => {
                assert_eq!((f, to), (NohzPhase::Periodic, NohzPhase::ExitReq));
                assert_eq!(
                    NohzError::OutOfOrder(f, to).code(),
                    SEG_BASE,
                    "乱序码 = 段基 + 0"
                );
            }
            other => panic!("应拒乱序，实际 {:?}", other),
        }
        // Periodic → TicksHalted（跳过 EnterReq）
        match p.confirm_halt(0, 2_000) {
            Err(NohzError::OutOfOrder(_, _)) => {}
            other => panic!("应拒跳步，实际 {:?}", other),
        }
        // Periodic → Catchup（serve 越序）
        match p.serve_catchup_tick(0, 3_000) {
            Err(NohzError::OutOfOrder(_, _)) => {}
            other => panic!("应拒越序补拍，实际 {:?}", other),
        }
        // Periodic → Closed（close 越序）
        match p.close_session(0, 4_000) {
            Err(NohzError::OutOfOrder(_, _)) => {}
            other => panic!("应拒越序闭合，实际 {:?}", other),
        }
        // Periodic 态中止（无事可中止）
        match p.abort_session(0, 5_000) {
            Err(NohzError::OutOfOrder(_, _)) => {}
            other => panic!("应拒空中止，实际 {:?}", other),
        }
        assert_eq!(p.out_of_order_blocks(), 5, "五次乱序全部立案");
        // 每次都有 OutOfOrderBlocked 留痕
        assert_eq!(
            p.log().count_kind(|k| matches!(k, EventKind::OutOfOrderBlocked(_, _))),
            5
        );
        // 账面态未受污染
        assert_eq!(p.core(0).unwrap().phase, NohzPhase::Periodic);
    }

    /// 正常会话的六态足迹：逐态断言时戳/账面推进。
    #[test]
    fn happy_path_state_footprint() {
        let mut p = NohzProtocol::new();
        let t0 = TICK_PERIOD_NS * 4;
        let span = TICK_PERIOD_NS * 5 + HALF_PERIOD_NS; // 理论 6 拍
        p.sync_wall(0, t0).unwrap();
        p.request_enter(0, GATES_ALL, t0).unwrap();
        assert_eq!(p.core(0).unwrap().phase, NohzPhase::EnterReq);
        p.confirm_halt(0, t0).unwrap();
        assert_eq!(p.core(0).unwrap().phase, NohzPhase::TicksHalted);
        assert_eq!(p.core(0).unwrap().enter_ns, t0);
        assert!(p.core(0).unwrap().is_halted());
        let t_exit = t0 + span;
        p.request_exit(0, t_exit).unwrap();
        assert_eq!(p.core(0).unwrap().phase, NohzPhase::ExitReq);
        assert_eq!(p.core(0).unwrap().exit_ns, t_exit);
        let theory = p.begin_catchup(0, t_exit).unwrap();
        assert_eq!(theory, 6, "5 拍整 + 半周期 → 6 拍");
        assert_eq!(p.core(0).unwrap().phase, NohzPhase::Catchup);
        // 逐拍补投，tick 账推进、墙钟冻结在 t0
        let wall_frozen = p.core(0).unwrap().wall_ns;
        assert_eq!(wall_frozen, t0, "冻结点应为停发前同步的墙钟基线");
        let mut served = 0;
        while served < theory {
            served = p.serve_catchup_tick(0, t_exit + served).unwrap();
            let c = p.core(0).unwrap();
            assert_eq!(c.wall_ns, wall_frozen, "补拍不得触碰墙钟");
            assert_eq!(c.tick_account, served, "tick 账逐拍推进");
        }
        assert_eq!(served, theory);
        p.complete_catchup(0, t_exit + served).unwrap();
        assert_eq!(p.core(0).unwrap().phase, NohzPhase::Closed);
        assert!(p.core(0).unwrap().catchup_reconciled());
        p.close_session(0, t_exit + served + CLOSE_GAP_NS).unwrap();
        let c = p.core(0).unwrap();
        assert_eq!(c.phase, NohzPhase::Periodic);
        assert!(c.session_closed);
        // 墙钟校准：闭合时一次校准，差值 = 停发跨度 + 补拍账面时移 + 闭合间隔
        assert_eq!(
            c.calibrate_delta_ns,
            (span + served + CLOSE_GAP_NS) as i64,
            "校准差值 = 真实闭合时戳 − 冻结基线"
        );
        assert_eq!(c.wall_ns, t_exit + served + CLOSE_GAP_NS);
        assert_eq!(c.halt_rounds, 1);
        assert_eq!(p.full_orders_ok(), 1);
        assert_eq!(p.catchups_ok(), 1);
    }

    // ── 进入门与保底 ─────────────────────────────────────────────────

    /// 门表逐门失格：缺哪门拒哪门（首失格门序号回传）。
    #[test]
    fn gate_table_denies_each_bit() {
        let mut i = 0;
        while i < ENTER_GATES.len() {
            let mut p = NohzProtocol::new();
            let mask = GATES_ALL & !ENTER_GATES[i].mask;
            match p.request_enter(0, mask, 1_000) {
                Err(NohzError::GateRefused(idx)) => {
                    assert_eq!(idx, i, "首失格门应为 {} 号", i);
                }
                other => panic!("门 {} 应失格，实际 {:?}", i, other),
            }
            // 回退边已走：态归 Periodic
            assert_eq!(p.core(0).unwrap().phase, NohzPhase::Periodic);
            i += 1;
        }
    }

    /// 门表 const 契约：掩码名与 deny 文案非空、掩码位两两独立。
    #[test]
    fn gate_table_const_contract() {
        let mut i = 0;
        while i < ENTER_GATES.len() {
            assert!(!ENTER_GATES[i].name.is_empty());
            assert!(!ENTER_GATES[i].deny_text.is_empty());
            assert!(ENTER_GATES[i].mask.count_ones() == 1);
            // 单位掩码逐位验收：只有全掩码过门
            let (pass, _) = gates_evaluate(ENTER_GATES[i].mask);
            assert!(!pass, "单门位不得过门");
            i += 1;
        }
        let (pass, idx) = gates_evaluate(GATES_ALL);
        assert!(pass);
        assert_eq!(idx, 0);
    }

    /// 末核保底（独立于 G3 门位）：全核停发请求被拒并自动回退。
    #[test]
    fn last_core_guard_blocks_and_rolls_back() {
        let mut p = NohzProtocol::new();
        // 核 1/2/3 依次真实停发（各自留停在 TicksHalted）
        let mut c = 1;
        while c < MAX_CPUS {
            let t0 = TICK_PERIOD_NS * (3 + c as u64);
            p.request_enter(c, GATES_ALL, t0).unwrap();
            p.confirm_halt(c, t0).unwrap();
            c += 1;
        }
        // 末核 0：掩码伪造全门（含 G3 位），协议独立复跑保底
        p.request_enter(0, GATES_ALL, TICK_PERIOD_NS).unwrap();
        match p.confirm_halt(0, TICK_PERIOD_NS) {
            Err(NohzError::LastCoreGuard) => {}
            other => panic!("末核停发应被保底拦截，实际 {:?}", other),
        }
        // 拦截后自动走 EnterReq→Periodic 回退边，会话闭合无悬空
        let c0 = p.core(0).unwrap();
        assert_eq!(c0.phase, NohzPhase::Periodic);
        assert!(c0.session_closed);
        assert_eq!(p.last_core_blocks(), 1);
        assert_eq!(
            p.log().count_kind(|k| matches!(k, EventKind::LastCoreBlocked)),
            1
        );
        // 恢复一核后末核即可停发（保底解除）
        p.abort_session(1, TICK_PERIOD_NS * 10).unwrap();
        p.request_enter(0, GATES_ALL, TICK_PERIOD_NS * 11).unwrap();
        p.confirm_halt(0, TICK_PERIOD_NS * 11).unwrap();
        assert_eq!(p.core(0).unwrap().phase, NohzPhase::TicksHalted);
    }

    /// 多核独立账：两核并发停发互不串账（各自推演/补拍/指纹分量）。
    #[test]
    fn multicore_ledgers_independent() {
        let mut p = NohzProtocol::new();
        let t = TICK_PERIOD_NS * 3;
        // 核 0：5 拍整停发
        run_minimal_session(&mut p, 0, t, TICK_PERIOD_NS * 5).unwrap();
        // 核 1：3 拍 + 半周期停发（理论 4 拍）
        run_minimal_session(&mut p, 1, t, TICK_PERIOD_NS * 3 + HALF_PERIOD_NS).unwrap();
        let c0 = p.core(0).unwrap();
        let c1 = p.core(1).unwrap();
        assert_eq!(c0.tick_account, 5);
        assert_eq!(c1.tick_account, 4);
        assert_eq!(c0.theory_ticks, 5);
        assert_eq!(c1.theory_ticks, 4);
        assert!(c0.catchup_reconciled());
        assert!(c1.catchup_reconciled());
        assert_eq!(c0.halt_rounds, 1);
        assert_eq!(c1.halt_rounds, 1);
        assert_eq!(p.full_orders_ok(), 2);
        assert_eq!(p.catchups_ok(), 2);
    }

    // ── 停发跨度与补拍上限 ───────────────────────────────────────────

    /// 驻留过短：退出请求被拒且态保持停发，随后可中止回拍。
    #[test]
    fn short_stay_rejected() {
        let mut p = NohzProtocol::new();
        let t0 = TICK_PERIOD_NS * 4;
        p.request_enter(0, GATES_ALL, t0).unwrap();
        p.confirm_halt(0, t0).unwrap();
        // 停发仅 1.5 拍（6ms < 最小驻留 8ms）
        match p.request_exit(0, t0 + TICK_PERIOD_NS + HALF_PERIOD_NS) {
            Err(NohzError::TooShort(span)) => {
                assert_eq!(span, TICK_PERIOD_NS + HALF_PERIOD_NS);
            }
            other => panic!("驻留过短应被拒，实际 {:?}", other),
        }
        assert_eq!(p.core(0).unwrap().phase, NohzPhase::TicksHalted);
        assert_eq!(
            p.log()
                .count_kind(|k| matches!(k, EventKind::ShortStayRejected(_))),
            1
        );
        p.abort_session(0, t0).unwrap();
        assert_eq!(p.core(0).unwrap().phase, NohzPhase::Periodic);
        assert_eq!(p.sessions_aborted(), 1);
    }

    /// 补拍超限：停发 100 拍 → 理论 100 拍 > 上限 46 → CatchupOver 拒绝。
    #[test]
    fn catchup_over_rejected() {
        let mut p = NohzProtocol::new();
        let t0 = TICK_PERIOD_NS * 4;
        let span = TICK_PERIOD_NS * 100;
        p.request_enter(0, GATES_ALL, t0).unwrap();
        p.confirm_halt(0, t0).unwrap();
        p.request_exit(0, t0 + span).unwrap();
        match p.begin_catchup(0, t0 + span) {
            Err(NohzError::CatchupOver(n)) => assert_eq!(n, 100),
            other => panic!("应报补拍超限，实际 {:?}", other),
        }
        // 态保持 ExitReq，可中止回拍
        assert_eq!(p.core(0).unwrap().phase, NohzPhase::ExitReq);
        p.abort_session(0, t0 + span).unwrap();
        assert_eq!(p.core(0).unwrap().phase, NohzPhase::Periodic);
    }

    // ── 预算立案 ─────────────────────────────────────────────────────

    /// 预算洪水：门失格尝试的步开销累计，第 17 步越 20µs 硬线立案。
    #[test]
    fn budget_breach_filed_and_aborts() {
        let rep = run_injection(InjectKind::BudgetFlood);
        assert!(rep.caught, "预算洪水必须被检出：{}", rep.note);
        assert_eq!(rep.observed_code, NohzError::BudgetBreach(0).code());
        // 立案后预算账清零、态回 Periodic（中止收尾）
        // （详细账面断言在注入器内部完成，此处复核稽核行）
        assert!(rep.note.contains("强制回拍"));
    }

    /// 预算稽核行：正常会话后逐核行全绿、余量正确。
    #[test]
    fn budget_audit_rows_clean() {
        let mut p = NohzProtocol::new();
        run_minimal_session(&mut p, 0, TICK_PERIOD_NS * 3, TICK_PERIOD_NS * 5).unwrap();
        let audit = BudgetAudit::audit(&p);
        assert!(audit.all_clean());
        assert_eq!(audit.budget_ns, PROTOCOL_BUDGET_NS);
        assert_eq!(audit.rows.len(), MAX_CPUS);
        // 会话闭合后预算账已清零，余量 = 全额
        assert_eq!(audit.rows[0].spent_ns, 0);
        assert_eq!(audit.rows[0].headroom_ns, PROTOCOL_BUDGET_NS);
        assert!(audit.rows[1].within);
        let s = audit.summarize();
        assert!(s.contains("最重会话"), "稽核摘要应含最重会话：{}", s);
        assert!(p.worst_session_ns() < PROTOCOL_BUDGET_NS);
    }

    // ── 事件账与指纹 ─────────────────────────────────────────────────

    /// 环形账：256 槽覆盖最旧，指纹随内容变化，recent 倒序取数。
    #[test]
    fn event_log_ring_and_fingerprint() {
        let mut log = EventLog::new();
        let mut i = 0u64;
        while i < (EVENTS_CAP as u64) + 44 {
            log.push(EventKind::CatchupTick(i), i * 10, 0).unwrap();
            i += 1;
        }
        assert_eq!(log.len(), EVENTS_CAP, "环形覆盖后槽位恒为 256");
        let fp_a = log.fingerprint();
        // 覆盖后再推一条：指纹必须改变（内容敏感）
        log.push(EventKind::CatchupTick(999_999), 9_999_999, 1).unwrap();
        assert_ne!(log.fingerprint(), fp_a);
        // recent 倒序：最新在前
        let recent = log.recent(3);
        assert_eq!(recent.len(), 3);
        assert!(matches!(recent[0].kind, EventKind::CatchupTick(999_999)));
        assert!(recent[0].seq > recent[1].seq);
        // 软复位：清账留痕，指纹复位为新账起点
        log.clear();
        assert_eq!(log.len(), 1);
        assert!(matches!(log.recent(1)[0].kind, EventKind::LedgerReset));
    }

    /// 严重级映射：红案/琥珀/信息三级各就各位。
    #[test]
    fn event_severity_mapping() {
        assert_eq!(EventKind::CatchupBroken(1, 2).severity(), 2);
        assert_eq!(EventKind::BudgetBreachFiled(1).severity(), 2);
        assert_eq!(EventKind::OutOfOrderBlocked(0, 1).severity(), 2);
        assert_eq!(EventKind::LastCoreBlocked.severity(), 1);
        assert_eq!(EventKind::GateDenied(0).severity(), 1);
        assert_eq!(EventKind::InjectCaught(0).severity(), 1);
        assert_eq!(EventKind::CatchupTick(1).severity(), 0);
        assert_eq!(EventKind::CatchupReconciled(1, 1).severity(), 0);
    }

    // ── health 三色 ──────────────────────────────────────────────────

    /// 三色体检：绿（干净）→ 红（悬空会话）→ 绿（闭合）→ 黄（琥珀）→ 红（红案）。
    #[test]
    fn health_three_colors() {
        let mut p = NohzProtocol::new();
        // 绿
        let hp = p.health();
        assert_eq!(hp.level, HEALTH_GREEN);
        assert_eq!(hp.level_text, "绿");
        // 悬空会话 → 红
        p.request_enter(0, GATES_ALL, 1_000).unwrap();
        let hp = p.health();
        assert_eq!(hp.level, HEALTH_RED);
        assert_eq!(hp.open_sessions, 1);
        assert!(hp.summary.contains("悬空会话1"));
        // 中止回退 → 琥珀（SessionAborted 为琥珀级留痕）
        p.abort_session(0, 2_000).unwrap();
        assert_eq!(p.health().level, HEALTH_AMBER);
        // 门失格（琥珀）→ 仍黄
        let _ = p.request_enter(0, GATES_ALL & !GATE_CLOCK_STABLE, 3_000);
        let hp = p.health();
        assert_eq!(hp.level, HEALTH_AMBER);
        assert_eq!(hp.level_text, "黄");
        assert!(hp.amber >= 2);
        // 红案（乱序）→ 红
        let _ = p.close_session(0, 4_000);
        let hp = p.health();
        assert_eq!(hp.level, HEALTH_RED);
        assert!(hp.red >= 1);
        assert!(hp.fingerprint.len() == 16, "指纹应为 16 位 hex");
    }

    // ── 注入面 ───────────────────────────────────────────────────────

    /// 注入面全扫描：五类注入全部按码检出。
    #[test]
    fn injection_sweep_all_caught() {
        let sweep = injection_sweep();
        assert_eq!(sweep.len(), 5);
        let mut i = 0;
        while i < sweep.len() {
            assert!(sweep[i].caught, "注入 {} 未检出：{}", sweep[i].kind.text(), sweep[i].note);
            assert!(
                sweep[i].observed_code >= SEG_BASE && sweep[i].observed_code <= SEG_BASE + 0xB,
                "检出码应落在 0x1470 段"
            );
            i += 1;
        }
        // 逐类复核码值
        assert_eq!(
            run_injection(InjectKind::OutOfOrder).observed_code,
            SEG_BASE
        );
        assert_eq!(
            run_injection(InjectKind::CatchupMismatch).observed_code,
            SEG_BASE + 0x5
        );
        assert_eq!(
            run_injection(InjectKind::AllCoreHalt).observed_code,
            SEG_BASE + 0x2
        );
        assert_eq!(
            run_injection(InjectKind::GateRefuse).observed_code,
            SEG_BASE + 0x1
        );
        assert_eq!(
            run_injection(InjectKind::BudgetFlood).observed_code,
            SEG_BASE + 0x6
        );
    }

    /// 补拍对不平的两个方向：超投（Catchup 态内投满再投）与
    /// 欠投（少投一拍就闭合）都被红案拦截。
    #[test]
    fn catchup_mismatch_injection_detail() {
        // Part A：超投——投满理论拍后再投第 6 拍
        let mut p = NohzProtocol::new();
        let t0 = TICK_PERIOD_NS * 3;
        let span = TICK_PERIOD_NS * 5;
        p.request_enter(0, GATES_ALL, t0).unwrap();
        p.confirm_halt(0, t0).unwrap();
        p.request_exit(0, t0 + span).unwrap();
        let theory = p.begin_catchup(0, t0 + span).unwrap();
        assert_eq!(theory, 5);
        let mut served = 0;
        while served < theory {
            served = p.serve_catchup_tick(0, t0 + span + served).unwrap();
        }
        assert_eq!(served, 5);
        match p.serve_catchup_tick(0, t0 + span + served) {
            Err(NohzError::CatchupMismatch(a, b)) => assert_eq!((a, b), (6, 5), "超投 6 对理论 5"),
            other => panic!("超投应拦截，实际 {:?}", other),
        }
        // 超投被拒即无账面污染：仍 5 拍，闭合对平照常通过
        assert_eq!(p.core(0).unwrap().catchup_served, 5);
        p.complete_catchup(0, t0 + span + served).unwrap();
        assert!(p.core(0).unwrap().catchup_reconciled());
        p.close_session(0, t0 + span + served + CLOSE_GAP_NS).unwrap();
        // Part B：欠投——核 1 少投一拍就闭合
        let t1 = t0 + TICK_PERIOD_NS * 10;
        p.sync_wall(1, t1).unwrap();
        p.request_enter(1, GATES_ALL, t1).unwrap();
        p.confirm_halt(1, t1).unwrap();
        p.request_exit(1, t1 + span).unwrap();
        let theory1 = p.begin_catchup(1, t1 + span).unwrap();
        assert_eq!(theory1, 5);
        let mut served1 = 0;
        while served1 + 1 < theory1 {
            served1 = p.serve_catchup_tick(1, t1 + span + served1).unwrap();
        }
        assert_eq!(served1, 4);
        match p.complete_catchup(1, t1 + span + served1) {
            Err(NohzError::CatchupMismatch(a, b)) => {
                assert_eq!((a, b), (4, 5), "已投 4 对理论 5");
            }
            other => panic!("对不平应红案，实际 {:?}", other),
        }
        assert_eq!(
            p.log()
                .count_kind(|k| matches!(k, EventKind::CatchupBroken(4, 5))),
            1
        );
        // health 转红
        assert_eq!(p.health().level, HEALTH_RED);
    }

    // ── 双跑确定性 ───────────────────────────────────────────────────

    /// 双跑确定性对拍：多种子同跑双遍，指纹与账面逐项恒等。
    #[test]
    fn deterministic_double_run() {
        let seeds = [1u64, 0xDEAD_BEEF, 0x4E0B_7F10, u64::MAX, 42];
        let mut i = 0;
        while i < seeds.len() {
            assert!(
                verify_determinism(seeds[i]),
                "seed {:#x} 双跑指纹必须恒等",
                seeds[i]
            );
            i += 1;
        }
    }

    /// 演练报告字段完备性：轮数/预算/健康级/摘要全就位。
    #[test]
    fn drill_report_fields() {
        let rep = run_full_drill(0xC0FF_EE01);
        assert_eq!(rep.rounds, DRILL_ROUNDS);
        assert!(rep.budget_clean);
        assert_eq!(rep.budget_breaches, 0);
        assert_eq!(rep.last_core_blocks, 0);
        assert_eq!(rep.out_of_order_blocks, 0);
        assert_eq!(rep.health_level, HEALTH_GREEN);
        assert_eq!(rep.fingerprint.len(), 16);
        assert!(rep.summary.contains("双跑演练"));
        assert!(rep.summary.contains("10/10"));
    }

    /// 推演进位覆盖：演练剧本中偶数轮触发半周期进位（轮账可复核）。
    #[test]
    fn rounding_rounds_covered() {
        let rep = run_full_drill(0x4E0B_7F10);
        // 10 轮中偶数轮（0/2/4/6/8）都带半周期余数 → 至少 5 轮进位
        assert!(
            rep.rounding_rounds >= 5,
            "进位轮数应 ≥5，实际 {}",
            rep.rounding_rounds
        );
    }

    // ── 错误集契约 ───────────────────────────────────────────────────

    /// 错误集契约：十二变体码落 0x1470 段、detail 中文非空、码值互异。
    #[test]
    fn error_contract() {
        let errs: [NohzError; 12] = [
            NohzError::OutOfOrder(NohzPhase::Periodic, NohzPhase::ExitReq),
            NohzError::GateRefused(0),
            NohzError::LastCoreGuard,
            NohzError::ClockBackward(1),
            NohzError::CatchupOver(1),
            NohzError::CatchupMismatch(1, 2),
            NohzError::BudgetBreach(1),
            NohzError::BadCpu(9),
            NohzError::SessionOpen(0),
            NohzError::TooShort(1),
            NohzError::LogFull(1),
            NohzError::LedgerMissing(0),
        ];
        let mut i = 0;
        while i < errs.len() {
            let code = errs[i].code();
            assert!(code >= SEG_BASE && code <= SEG_BASE + 0xB, "码 {:#06x} 越段", code);
            let d = errs[i].detail();
            assert!(!d.is_empty(), "变体 {} detail 为空", i);
            assert!(d.len() > 10, "变体 {} detail 过短，须可操作", i);
            // 码值互异
            let mut j = 0;
            while j < i {
                assert_ne!(errs[j].code(), code, "变体 {} 与 {} 码值冲突", j, i);
                j += 1;
            }
            i += 1;
        }
    }

    /// 核号越界：所有 API 对 cpu ≥ MAX_CPUS 统一 BadCpu 拒绝。
    #[test]
    fn bad_cpu_rejected_everywhere() {
        let mut p = NohzProtocol::new();
        let bad = MAX_CPUS + 7;
        match p.request_enter(bad, GATES_ALL, 0) {
            Err(NohzError::BadCpu(c)) => assert_eq!(c, bad),
            other => panic!("应拒越界核号，实际 {:?}", other),
        }
        match p.confirm_halt(bad, 0) {
            Err(NohzError::BadCpu(_)) => {}
            other => panic!("应拒越界核号，实际 {:?}", other),
        }
        match p.request_exit(bad, 0) {
            Err(NohzError::BadCpu(_)) => {}
            other => panic!("应拒越界核号，实际 {:?}", other),
        }
        match p.begin_catchup(bad, 0) {
            Err(NohzError::BadCpu(_)) => {}
            other => panic!("应拒越界核号，实际 {:?}", other),
        }
        match p.serve_catchup_tick(bad, 0) {
            Err(NohzError::BadCpu(_)) => {}
            other => panic!("应拒越界核号，实际 {:?}", other),
        }
        match p.complete_catchup(bad, 0) {
            Err(NohzError::BadCpu(_)) => {}
            other => panic!("应拒越界核号，实际 {:?}", other),
        }
        match p.close_session(bad, 0) {
            Err(NohzError::BadCpu(_)) => {}
            other => panic!("应拒越界核号，实际 {:?}", other),
        }
        match p.abort_session(bad, 0) {
            Err(NohzError::BadCpu(_)) => {}
            other => panic!("应拒越界核号，实际 {:?}", other),
        }
        match p.core(bad) {
            Err(NohzError::BadCpu(_)) => {}
            other => panic!("应拒越界核号，实际 {:?}", other),
        }
        match p.halspan_of(bad) {
            Err(NohzError::BadCpu(_)) => {}
            other => panic!("应拒越界核号，实际 {:?}", other),
        }
    }

    /// 六态中文短句与停发口径抽查。
    #[test]
    fn phase_text_and_halt_scope() {
        assert_eq!(NohzPhase::Periodic.text(), "周期在拍");
        assert_eq!(NohzPhase::EnterReq.text(), "进入请求");
        assert_eq!(NohzPhase::TicksHalted.text(), "停发中");
        assert_eq!(NohzPhase::ExitReq.text(), "退出请求");
        assert_eq!(NohzPhase::Catchup.text(), "补拍中");
        assert_eq!(NohzPhase::Closed.text(), "闭合回拍");
        // 停发口径：TicksHalted/ExitReq/Catchup 计停发，其余不计
        assert!(!NohzPhase::Periodic.is_halted());
        assert!(!NohzPhase::EnterReq.is_halted());
        assert!(NohzPhase::TicksHalted.is_halted());
        assert!(NohzPhase::ExitReq.is_halted());
        assert!(NohzPhase::Catchup.is_halted());
        assert!(!NohzPhase::Closed.is_halted());
        // 序号可逆
        let mut o = 0;
        while o < 6 {
            assert_eq!(NohzPhase::from_ordinal(o).ordinal(), o);
            o += 1;
        }
    }

    /// 会话步数账：正常会话 5 协议步 + 理论拍数步（闭合后清零，
    /// 但闭合事件携净开销）。
    #[test]
    fn session_step_accounting() {
        let mut p = NohzProtocol::new();
        let t0 = TICK_PERIOD_NS * 3;
        let span = TICK_PERIOD_NS * 5; // 理论 5 拍
        p.request_enter(0, GATES_ALL, t0).unwrap();
        p.confirm_halt(0, t0).unwrap();
        p.request_exit(0, t0 + span).unwrap();
        let theory = p.begin_catchup(0, t0 + span).unwrap();
        let mut served = 0;
        while served < theory {
            served = p.serve_catchup_tick(0, t0 + span + served).unwrap();
        }
        p.complete_catchup(0, t0 + span + served).unwrap();
        // 闭合前账面：5 步 + 5 拍 = 10 步，开销 5×1200 + 5×300 = 7500ns
        let before = p.core(0).unwrap();
        assert_eq!(before.session_steps, 10);
        assert_eq!(before.spent_ns, 7_500);
        p.close_session(0, t0 + span + served + CLOSE_GAP_NS).unwrap();
        // 闭合后预算账清零（单会话口径）
        let after = p.core(0).unwrap();
        assert_eq!(after.spent_ns, 0);
        assert_eq!(after.session_steps, 0);
        assert!(p.worst_session_ns() >= 7_500);
    }

    /// 会话计数账：开启/闭合/中止三账分立且逐笔对应。
    #[test]
    fn session_counters() {
        let mut p = NohzProtocol::new();
        // 1 笔正常闭合
        run_minimal_session(&mut p, 0, TICK_PERIOD_NS * 3, TICK_PERIOD_NS * 4).unwrap();
        assert_eq!(p.sessions_closed(), 1);
        assert_eq!(p.full_orders_ok(), 1);
        // 1 笔中止（CatchupOver 路径）
        let t0 = TICK_PERIOD_NS * 20;
        p.request_enter(1, GATES_ALL, t0).unwrap();
        p.confirm_halt(1, t0).unwrap();
        p.request_exit(1, t0 + TICK_PERIOD_NS * 100).unwrap();
        assert!(p.begin_catchup(1, t0 + TICK_PERIOD_NS * 100).is_err());
        p.abort_session(1, t0).unwrap();
        assert_eq!(p.sessions_closed(), 1);
        // 1 笔门失格（未开账即拒，不计开启）
        let _ = p.request_enter(2, GATES_ALL & !GATE_IDLE_BUDGET, t0);
        assert_eq!(p.sessions_closed(), 1);
        assert_eq!(p.full_orders_ok(), 1);
    }
}
