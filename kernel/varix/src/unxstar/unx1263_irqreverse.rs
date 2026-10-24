//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1263 · Irq 态逆迁移协议（Irq→Polling 逆回路径） · 诊断段 0x1380
//! ─────────────────────────────────────────────────────────────────────
//! 验收锚点（任务板逐字锚定）：
//!   UNX-F1263-J1 逆迁移触发判据 ≥3 项（中断面故障/风暴升级/维护指令）全实现，
//!   逆迁移执行事件零丢失（序号连续断言 10 轮），逆迁移后 Polling 态接管完整率
//!   100%（轮询泵恢复覆盖断言）
//! 定位（UNX-A2 批表逐字抄录，来源：
//!   docs/Varix/CoRun Varix STAR II · Unxreal/CoRun Varix STAR II · Unxreal.md:3058）：
//!   | UNX-F1263 | Irq 态逆迁移协议（Irq→Polling 逆回路径） | 320 | 已深化 |
//!   | UNX-F1263-J1 逆迁移触发判据 ≥3 项（中断面故障/风暴升级/维护指令）
//!     全实现，逆迁移执行事件零丢失（序号连续断言 10 轮），逆迁移后 Polling
//!     态接管完整率 100%（轮询泵恢复覆盖断言） |
//! ─────────────────────────────────────────────────────────────────────
//! 设计要点（全手工定制真实子系统，无空壳）：
//!   1. 五态机（Idle/Polling/Draining/Arming/Irq，F1262 正向契约）之上的
//!      逆迁移协议本体：Irq→Polling 逆回全序（Arming 撤防 → Irq 退出 →
//!      Polling 进入），次序即语义，乱序拒绝带码 0x1386。
//!   2. 逆回判据 const 表 ≥3 项：静默窗时长 / 事件率跌破阈值（滞回+防抖
//!      确认）/ 逆回冷却期；触发原因 ≥3 类：中断面故障 / 风暴升级 / 维护指令。
//!      评估与执行双段协议：先 evaluate 得判据快照，再携快照 begin_reverse，
//!      快照原因码/新鲜度不符拒绝（0x138E）。
//!   3. 逆回守恒：逆回窗内事件零丢失——事件入 pending 账（序号全局连续），
//!      Polling 态进入后全量补投，补投账与 pending 账对平（0x138B），
//!      序号断裂必抓红账（0x138A）。
//!   4. 冷却与防振荡：逆回完成后 Polling 态最短驻留 N tick 方可再正向迁移，
//!      驻留未满拒绝（0x1387），同一驻留窗内连续 2 次被拒入振荡红账（0x1388）。
//!   5. 非法逆回拦截：Irq 态仍有未确认向量时逆回=丢中断——拦截并入红账
//!      （0x1385），红账定长滚动不阻塞主路径。
//!   6. 逆回审计账：每次逆回带原因码+判据快照+耗时，定长环形账+回放重演；
//!      跨轮序号连续审计（10 轮断言的协议面出口）。
//!   7. 健康面：事件账守恒恒等 + health() 体检 + FNV 指纹 + 双跑确定性对拍
//!      + 注入测试面（非法逆回注入/逆回窗事件注入/振荡注入各检出）。
//! ─────────────────────────────────────────────────────────────────────
//! 内核纪律：no_std+alloc；内核路径零 unwrap/expect/panic!/运行期 assert!；
//! 可失败全走 Result<T, IrqRevError>；错误枚举每变体带中文可操作 detail；
//! 溢出显性 saturating/wrapping；>64KB 结构体禁上栈（本模块最大结构体
//! ReverseTransitionEngine 约 1.6KB 级，含 Vec 堆件不整块拷贝）；
//! const fn 遵守：无 slice::get/Vec 索引/Ord::min/max/derive PartialEq 的
//! ==/for 循环（用 while+索引），含 Vec 字段结构体不 derive Copy。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;
use core::fmt;

// ═══════════════════════════════════════════════════════════════════════
// 一、诊断码段（独占 0x1380~0x138F，开工前已 grep 全 unxstar 确认空闲）
// ═══════════════════════════════════════════════════════════════════════

/// 诊断段基址（0x1380）。来源：UNX-F1263 诊断段分配表（任务 UNX-F1263 指定段，
/// 全 unxstar grep 确认 0x1380~0x138F 无占用）。
pub const SEG_BASE: u16 = 0x1380;

/// 段末界（含）。码位分配：0x1380 为段基址保留，0x1381~0x138F 为 15 个
/// 错误码逐位落位，一码一义。
pub const SEG_LAST: u16 = 0x138F;

/// 错误码：状态机驻留态与操作前置不符。
pub const CODE_NOT_IN_RESIDENT_STATE: u16 = 0x1381;
/// 错误码：逆回已在进行（重复 begin_reverse / 逆回窗未收口再开窗）。
pub const CODE_ALREADY_REVERSING: u16 = 0x1382;
/// 错误码：逆回判据未满足（静默窗/事件率/冷却期任一未达标）。
pub const CODE_CRITERIA_NOT_MET: u16 = 0x1383;
/// 错误码：判据编号查无此项（判据表外 id）。
pub const CODE_UNKNOWN_CRITERION: u16 = 0x1384;
/// 错误码：存在未确认向量（逆回=丢中断，拦截）。
pub const CODE_UNACKED_VECTORS_PRESENT: u16 = 0x1385;
/// 错误码：逆回全序步乱序（撤防→Irq 退出→Polling 进入次序违反）。
pub const CODE_ORDER_VIOLATION: u16 = 0x1386;
/// 错误码：Polling 最短驻留未满，禁止正向迁移（防振荡滞回下沿）。
pub const CODE_RESIDENCE_NOT_MET: u16 = 0x1387;
/// 错误码：振荡嫌疑（同驻留窗内连续被拒入红账）。
pub const CODE_OSCILLATION_SUSPECTED: u16 = 0x1388;
/// 错误码：pending 账溢出（逆回窗事件积压超容量）。
pub const CODE_PENDING_OVERFLOW: u16 = 0x1389;
/// 错误码：补投序号断裂（零丢失被破坏，必抓红账）。
pub const CODE_REPLAY_SEQ_GAP: u16 = 0x138A;
/// 错误码：补投账与 pending 账对平失败。
pub const CODE_RECONCILE_MISMATCH: u16 = 0x138B;
/// 错误码：账本已封账（seal 后拒绝一切写操作）。
pub const CODE_LEDGER_SEALED: u16 = 0x138C;
/// 错误码：补投窗未开（相位不是 Completed，无账可补）。
pub const CODE_REPLAY_WINDOW_CLOSED: u16 = 0x138D;
/// 错误码：判据快照与当前执行不一致（原因码不匹配/快照过期）。
pub const CODE_SNAPSHOT_MISMATCH: u16 = 0x138E;
/// 错误码：双跑确定性对拍指纹不一致。
pub const CODE_DUAL_RUN_MISMATCH: u16 = 0x138F;

// ═══════════════════════════════════════════════════════════════════════
// 二、逆回判据常量表（≥3 项，const 写死，逐项注来源）
// ═══════════════════════════════════════════════════════════════════════

/// 判据一 id：静默窗时长。
pub const CRIT_ID_SILENCE: u16 = 1;
/// 判据二 id：事件率跌破阈值（滞回+防抖确认）。
pub const CRIT_ID_RATE: u16 = 2;
/// 判据三 id：逆回冷却期。
pub const CRIT_ID_COOLDOWN: u16 = 3;

/// 判据一阈值：静默窗时长 64 tick。
/// 来源：UNX-F1263-J1「逆迁移触发判据 ≥3 项」实现侧自定安全阈值——
/// 连续 64 tick 无任何中断事件方视为中断面静默（取 4 个 16 tick 采样窗，
/// 保证事件率判据与静默判据观测窗对齐）。
pub const CRIT_SILENCE_WINDOW_TICKS: u64 = 64;

/// 判据二阈值：单个 16 tick 采样窗内事件数低于 2 视为低率样本。
/// 来源：UNX-F1263-J1「事件率跌破阈值」实现侧自定——16 tick 窗内 ≤1 事件
/// 记一次低率样本，连续 3 个低率样本（防抖）才确认跌破。
pub const CRIT_RATE_FLOOR_EVENTS: u64 = 2;

/// 事件率采样窗宽：16 tick。
/// 来源：与 F1264 稳态统计矩阵快照周期（≤1s 量级）对齐的实现采样粒度。
pub const RATE_WINDOW_TICKS: usize = 16;

/// 防抖确认数：连续 3 个低率样本方确认「事件率跌破阈值」。
/// 来源：UNX-F1263-J1 触发判据实现侧防抖（避免单窗抖动误触发逆回）。
pub const CRIT_DEBOUNCE_CONFIRM_COUNT: u32 = 3;

/// 判据三阈值：逆回冷却期 32 tick。
/// 来源：UNX-F1263-J1「逆迁移触发判据」第三项实现侧自定——两次逆回之间
/// 最短间隔，防止逆回通道本身被高频滥用。
pub const CRIT_REVERSE_COOLDOWN_TICKS: u64 = 32;

/// 逆回后 Polling 态最短驻留 48 tick 方可再正向迁移。
/// 来源：UNX-F1263-J1 逆迁移后 Polling 态接管完整率 100% 的驻留保障
/// （滞回上沿：驻留 = 3 个完整采样窗，确保轮询泵稳定接管后再离开）。
pub const CRIT_MIN_POLLING_RESIDENCE_TICKS: u64 = 48;

/// 振荡红账阈值：同一驻留窗内连续 2 次正向迁移被拒即入振荡红账。
/// 来源：防振荡设计（任务规格「滞回+防抖计数」）实现侧自定。
pub const OSCILLATION_RED_THRESHOLD: u32 = 2;

/// 判据快照新鲜窗：快照评估后 8 tick 内必须 begin_reverse，过期拒绝。
/// 来源：评估-执行双段协议的实现侧防陈旧窗口。
pub const SNAPSHOT_FRESH_TICKS: u64 = 8;

/// pending 账容量：4096 条。
/// 来源：逆回窗事件积压上限（64 tick 静默窗 × 假想 64 向量最坏并发 × 余量）。
pub const PENDING_CAP: usize = 4096;

/// 补投账环形容量：256 条（滚动保底，累计数不丢）。
/// 来源：补投账回放窗口设计（环形保底 + 累计恒等账）。
pub const REPLAY_RING_CAP: usize = 256;

/// 审计环形账容量：64 条。
/// 来源：UNX-F1254/F1272 审计账「滚动保底常绿」惯例对齐。
pub const AUDIT_RING_CAP: usize = 64;

/// 红账定长容量：256 条（滚动淘汰最旧，淘汰计数留痕）。
pub const RED_CAP: usize = 256;

/// 滞回翻转标记位数：8 个（健康面振荡观测用）。
pub const TRANSITION_MARKS: usize = 8;

/// tick 单步粗跳阈值：超过则一次性结算再整段推进（限制长推计算量）。
pub const TICK_JUMP_THRESHOLD: u64 = 1 << 20;

/// 判据元数据 const 表：（判据 id，判据名，阈值）。
/// 来源：上述三判据常量的聚合表——逆回判据 ≥3 项的协议面出口。
pub const CRIT_TABLE: [(u16, &'static str, u64); 3] = [
    (
        CRIT_ID_SILENCE,
        "静默窗时长",
        CRIT_SILENCE_WINDOW_TICKS,
    ),
    (
        CRIT_ID_RATE,
        "事件率跌破阈值(16tick窗计数下限)",
        CRIT_RATE_FLOOR_EVENTS,
    ),
    (
        CRIT_ID_COOLDOWN,
        "逆回冷却期",
        CRIT_REVERSE_COOLDOWN_TICKS,
    ),
];

/// const fn：按判据 id 查阈值（while+索引，无 slice::get/for 循环）。
/// 查无返回 0（调用方以 0 判未知）。
pub const fn crit_threshold(id: u16) -> u64 {
    let mut i = 0usize;
    while i < CRIT_TABLE.len() {
        if CRIT_TABLE[i].0 == id {
            return CRIT_TABLE[i].2;
        }
        i += 1;
    }
    0
}

/// const fn：按判据 id 查判据名（while+索引）。查无返回"未知判据"。
pub const fn crit_name(id: u16) -> &'static str {
    let mut i = 0usize;
    while i < CRIT_TABLE.len() {
        if CRIT_TABLE[i].0 == id {
            return CRIT_TABLE[i].1;
        }
        i += 1;
    }
    "未知判据"
}

// ═══════════════════════════════════════════════════════════════════════
// 三、五态机与逆回相位（F1262 正向契约的状态面）
// ═══════════════════════════════════════════════════════════════════════

/// 中断子系统五态（F1262 正向契约：Idle→Polling→Draining→Arming→Irq）。
/// 本模块在其上实现 Irq→Polling 的逆回路径。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IrqState {
    /// 未上电/未初始化。
    Idle,
    /// 轮询态（逆回的目标态）。
    Polling,
    /// 排空态（正向链上 Polling 的下一站）。
    Draining,
    /// 武装态。
    Arming,
    /// 中断态（逆回的源态）。
    Irq,
}

impl IrqState {
    /// const fn：态编码（match，无 derive PartialEq 的 ==）。
    pub const fn code(self) -> u8 {
        match self {
            IrqState::Idle => 0,
            IrqState::Polling => 1,
            IrqState::Draining => 2,
            IrqState::Arming => 3,
            IrqState::Irq => 4,
        }
    }

    /// 态名（协议面/人话化出口用）。
    pub const fn name(self) -> &'static str {
        match self {
            IrqState::Idle => "Idle",
            IrqState::Polling => "Polling",
            IrqState::Draining => "Draining",
            IrqState::Arming => "Arming",
            IrqState::Irq => "Irq",
        }
    }
}

/// 逆回协议相位（叠加在五态之上的协议状态）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReversePhase {
    /// 休眠：无逆回活动（Irq 态常态）。
    Dormant,
    /// 逆回窗已开：判据已过、快照已收，等待全序步。
    WindowOpen,
    /// 逆回中：至少执行了一个全序步。
    Reversing,
    /// 本轮逆回完成：Polling 已进入，补投窗开启。
    Completed,
    /// 正向迁移已放行：Polling→Draining 交接完成。
    Forwarded,
}

impl ReversePhase {
    /// const fn：相位编码。
    pub const fn code(self) -> u8 {
        match self {
            ReversePhase::Dormant => 0,
            ReversePhase::WindowOpen => 1,
            ReversePhase::Reversing => 2,
            ReversePhase::Completed => 3,
            ReversePhase::Forwarded => 4,
        }
    }

    /// 相位名。
    pub const fn name(self) -> &'static str {
        match self {
            ReversePhase::Dormant => "Dormant",
            ReversePhase::WindowOpen => "WindowOpen",
            ReversePhase::Reversing => "Reversing",
            ReversePhase::Completed => "Completed",
            ReversePhase::Forwarded => "Forwarded",
        }
    }
}

/// 逆回触发原因（UNX-F1263-J1 ≥3 项：中断面故障/风暴升级/维护指令）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReverseTriggerCause {
    /// 中断面故障：中断控制器/线路级故障，Irq 态不可信，须逆回轮询。
    FaceFault,
    /// 风暴升级：事件率持续异常升高，中断驱动代价超轮询，须逆回。
    StormUpgrade,
    /// 维护指令：运维显式指令要求逆回（计划性切换）。
    MaintenanceOrder,
}

impl ReverseTriggerCause {
    /// const fn：原因 id（审计账/快照对平用）。
    pub const fn id(self) -> u8 {
        match self {
            ReverseTriggerCause::FaceFault => 1,
            ReverseTriggerCause::StormUpgrade => 2,
            ReverseTriggerCause::MaintenanceOrder => 3,
        }
    }

    /// 原因名。
    pub const fn name(self) -> &'static str {
        match self {
            ReverseTriggerCause::FaceFault => "中断面故障",
            ReverseTriggerCause::StormUpgrade => "风暴升级",
            ReverseTriggerCause::MaintenanceOrder => "维护指令",
        }
    }
}

/// 逆回全序步（次序即语义）：Arming 撤防 → Irq 退出 → Polling 进入。
/// 乱序执行一律拒绝（0x1386）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReverseOrderStep {
    /// 步一：Arming 撤防（要求未确认向量为空，否则拦截）。
    DisarmArming,
    /// 步二：Irq 态退出（状态机离开 Irq，进入逆回中）。
    IrqExit,
    /// 步三：Polling 态进入（轮询泵接管 + 逆回窗收口 + 审计落账）。
    PollingEnter,
}

impl ReverseOrderStep {
    /// const fn：步编码。
    pub const fn code(self) -> u8 {
        match self {
            ReverseOrderStep::DisarmArming => 0,
            ReverseOrderStep::IrqExit => 1,
            ReverseOrderStep::PollingEnter => 2,
        }
    }

    /// const fn：按全序游标推期望步。游标 ≥3 无对应步，返回 None 编码 -1。
    /// 用 i8 承载"无步"语义（-1），避免 Option 在 const 面上的额外展开。
    pub const fn expected_at(cursor: u8) -> i8 {
        match cursor {
            0 => ReverseOrderStep::DisarmArming.code() as i8,
            1 => ReverseOrderStep::IrqExit.code() as i8,
            2 => ReverseOrderStep::PollingEnter.code() as i8,
            _ => -1,
        }
    }

    /// 步名。
    pub const fn name(self) -> &'static str {
        match self {
            ReverseOrderStep::DisarmArming => "Arming撤防",
            ReverseOrderStep::IrqExit => "Irq退出",
            ReverseOrderStep::PollingEnter => "Polling进入",
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 四、错误枚举（每变体一码一位 + 中文可操作 detail）
// ═══════════════════════════════════════════════════════════════════════

/// 逆回协议统一错误。内核路径零 panic：一切可失败操作走本枚举。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IrqRevError {
    /// 状态机驻留态与操作前置不符。
    NotInResidentState {
        /// 当前态。
        state: IrqState,
        /// 当前相位。
        phase: ReversePhase,
    },
    /// 逆回已在进行（逆回窗未收口再开窗）。
    AlreadyReversing,
    /// 逆回判据未满足（payload 为首个未达标判据）。
    CriteriaNotMet {
        /// 未达标判据 id。
        criterion_id: u16,
        /// 未达标判据名。
        criterion_name: &'static str,
    },
    /// 判据编号查无此项。
    UnknownCriterion {
        /// 表外判据 id。
        id: u16,
    },
    /// 存在未确认向量（逆回=丢中断，拦截；排空中残留同码拦截）。
    UnackedVectorsPresent {
        /// 未确认事件计数。
        count: u32,
    },
    /// 逆回全序步乱序。
    OrderViolation {
        /// 期望步。
        expected: ReverseOrderStep,
        /// 实际提交步。
        got: ReverseOrderStep,
    },
    /// Polling 最短驻留未满，禁止正向迁移。
    ResidenceNotMet {
        /// 还差多少 tick。
        remaining: u64,
    },
    /// 振荡嫌疑（同驻留窗内连续被拒）。
    OscillationSuspected {
        /// 累计被拒次数。
        attempts: u32,
    },
    /// pending 账溢出。
    PendingOverflow {
        /// 触发溢出的事件向量。
        vector: u16,
    },
    /// 补投序号断裂。
    ReplaySeqGap {
        /// 期望序号。
        expected: u64,
        /// 实得序号。
        got: u64,
    },
    /// 补投账与 pending 账对平失败。
    ReconcileMismatch {
        /// pending 累计入账数。
        pending_total: u64,
        /// 补投累计数。
        replayed_total: u64,
    },
    /// 账本已封账。
    LedgerSealed,
    /// 补投窗未开。
    ReplayWindowClosed,
    /// 判据快照与当前执行不一致。
    SnapshotMismatch {
        /// 不一致原因（"原因码不匹配" / "快照已过期"）。
        reason: &'static str,
    },
    /// 双跑确定性对拍指纹不一致。
    DualRunMismatch {
        /// 左跑指纹。
        left: u64,
        /// 右跑指纹。
        right: u64,
    },
}

impl IrqRevError {
    /// 诊断码（0x1381~0x138F，一变体一码）。
    pub fn code(&self) -> u16 {
        match self {
            IrqRevError::NotInResidentState { .. } => CODE_NOT_IN_RESIDENT_STATE,
            IrqRevError::AlreadyReversing => CODE_ALREADY_REVERSING,
            IrqRevError::CriteriaNotMet { .. } => CODE_CRITERIA_NOT_MET,
            IrqRevError::UnknownCriterion { .. } => CODE_UNKNOWN_CRITERION,
            IrqRevError::UnackedVectorsPresent { .. } => CODE_UNACKED_VECTORS_PRESENT,
            IrqRevError::OrderViolation { .. } => CODE_ORDER_VIOLATION,
            IrqRevError::ResidenceNotMet { .. } => CODE_RESIDENCE_NOT_MET,
            IrqRevError::OscillationSuspected { .. } => CODE_OSCILLATION_SUSPECTED,
            IrqRevError::PendingOverflow { .. } => CODE_PENDING_OVERFLOW,
            IrqRevError::ReplaySeqGap { .. } => CODE_REPLAY_SEQ_GAP,
            IrqRevError::ReconcileMismatch { .. } => CODE_RECONCILE_MISMATCH,
            IrqRevError::LedgerSealed => CODE_LEDGER_SEALED,
            IrqRevError::ReplayWindowClosed => CODE_REPLAY_WINDOW_CLOSED,
            IrqRevError::SnapshotMismatch { .. } => CODE_SNAPSHOT_MISMATCH,
            IrqRevError::DualRunMismatch { .. } => CODE_DUAL_RUN_MISMATCH,
        }
    }

    /// 中文可操作诊断（每变体固定文案，指明下一步动作）。
    pub fn detail(&self) -> &'static str {
        match self {
            IrqRevError::NotInResidentState { .. } => {
                "状态机驻留态与操作前置不符：逆回需 Irq 态+Dormant 相位，补投/对平需 \
                 Completed 相位，注入需逆回窗开启；先核对 state/phase 再重试"
            }
            IrqRevError::AlreadyReversing => {
                "逆回已在进行：逆回窗未收口前禁止再次 begin_reverse；先走完全序三步 \
                 （撤防→Irq 退出→Polling 进入）收口本轮"
            }
            IrqRevError::CriteriaNotMet { .. } => {
                "逆回判据未满足：查 payload 首个未达标判据 id 对应阈值 \
                 （静默窗/事件率防抖/冷却期），继续推进 tick 或先排空事件源后再评估"
            }
            IrqRevError::UnknownCriterion { .. } => {
                "判据编号查无此项：判据 id 必须取自 CRIT_TABLE（1=静默窗 2=事件率 \
                 3=冷却期），修正 id 后重试"
            }
            IrqRevError::UnackedVectorsPresent { .. } => {
                "存在未确认向量：Irq 态带未确认向量逆回等于丢中断，已拦截并入红账；\
                 先逐向量 ack_vector 清账，再发起逆回"
            }
            IrqRevError::OrderViolation { .. } => {
                "逆回全序步乱序：必须按 撤防→Irq退出→Polling进入 次序逐步提交；\
                 查 expected 字段取下一步应提交的步"
            }
            IrqRevError::ResidenceNotMet { .. } => {
                "Polling 最短驻留未满：正向迁移被滞回下沿拦下；继续 tick 推进至 \
                 remaining 清零再申请，连续被拒会触发振荡红账"
            }
            IrqRevError::OscillationSuspected { .. } => {
                "振荡嫌疑：同一驻留窗内正向迁移被连续拒绝已达红账阈值；停止重试，\
                 等驻留期满并让事件率回升后再走正向"
            }
            IrqRevError::PendingOverflow { .. } => {
                "pending 账溢出：逆回窗事件积压超容量，已入红账；缩短逆回窗 \
                 （尽快走完全序步）或提前进入 Polling 补投"
            }
            IrqRevError::ReplaySeqGap { .. } => {
                "补投序号断裂：pending 账内序号不连续，零丢失被破坏，已入红账；\
                 用 red_entries 定位断裂点并回放审计账重演"
            }
            IrqRevError::ReconcileMismatch { .. } => {
                "补投账与 pending 账对平失败：窗口事件数/累计恒等式不平衡；\
                 核对 window_account 与 replayed_total 差值后重演补投"
            }
            IrqRevError::LedgerSealed => {
                "账本已封账：seal 后只读；需要继续操作请新建引擎实例"
            }
            IrqRevError::ReplayWindowClosed => {
                "补投窗未开：补投/对平仅在 PollingEnter 完成（相位=Completed）后合法；\
                 先走完全序第三步"
            }
            IrqRevError::SnapshotMismatch { .. } => {
                "判据快照与当前执行不一致：快照原因码须与 begin_reverse 原因一致，\
                 且评估后 8 tick 内执行；重新 evaluate 后立即 begin_reverse"
            }
            IrqRevError::DualRunMismatch { .. } => {
                "双跑确定性对拍失败：同参数两次场景指纹不一致，存在非确定源；\
                 排查场景驱动是否引入了外部时序依赖"
            }
        }
    }
}

impl fmt::Display for IrqRevError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "IRQREV-0x{:04X}: {}", self.code(), self.detail())
    }
}

/// 模块统一 Result 别名。
pub type IrqRevResult<T> = Result<T, IrqRevError>;

// ═══════════════════════════════════════════════════════════════════════
// 五、滞回门（判据二「事件率跌破阈值」的滞回+防抖本体）
// ═══════════════════════════════════════════════════════════════════════

/// 滞回门：事件率判据的双阈值状态机。
///
/// 下沿（进入低率确认）：连续 `CRIT_DEBOUNCE_CONFIRM_COUNT` 个采样窗计数
/// < `CRIT_RATE_FLOOR_EVENTS` 方置 `confirmed=true`（防单窗抖动误触发逆回）；
/// 上沿（退出低率）：任一窗计数 ≥ 下限立即解除确认（事件率回升即刻翻回，
/// 保证正向迁移不会被陈旧低率确认放行）。翻转时刻全部留痕供振荡观测。
#[derive(Debug, Clone, Copy)]
pub struct HysteresisGate {
    /// 连续低率样本计数。
    low_streak: u32,
    /// 低率确认位（判据二满足位）。
    confirmed: bool,
    /// 累计翻转次数（wrapping）。
    transitions: u64,
    /// 最近 8 次翻转的 tick 标记（0 表示空位；tick=0 语义并集可忽略）。
    marks: [u64; TRANSITION_MARKS],
    /// 上次翻转 tick。
    last_change_tick: u64,
}

impl HysteresisGate {
    /// 新建：初始高率（未确认），无翻转历史。
    pub fn new() -> Self {
        HysteresisGate {
            low_streak: 0,
            confirmed: false,
            transitions: 0,
            marks: [0u64; TRANSITION_MARKS],
            last_change_tick: 0,
        }
    }

    /// 喂入一个采样窗计数（每 16 tick 一次）。
    ///
    /// * `window_events` — 窗内事件数；
    /// * `at_tick` — 采样时点（翻转留痕用）。
    pub fn feed(&mut self, window_events: u64, at_tick: u64) {
        if window_events < CRIT_RATE_FLOOR_EVENTS {
            // 低率样本：防抖计数累加（saturating 防溢出）
            self.low_streak = self.low_streak.saturating_add(1);
        } else {
            // 率回升：立即清零（滞回上沿即刻翻回）
            self.low_streak = 0;
        }
        let want = self.low_streak >= CRIT_DEBOUNCE_CONFIRM_COUNT;
        if want != self.confirmed {
            self.confirmed = want;
            self.transitions = self.transitions.wrapping_add(1);
            self.last_change_tick = at_tick;
            self.remember_transition(at_tick);
        }
    }

    /// 翻转标记滚动登记（while 移位，定长 8）。
    fn remember_transition(&mut self, at_tick: u64) {
        let mut i: usize = TRANSITION_MARKS - 1;
        while i > 0 {
            self.marks[i] = self.marks[i - 1];
            i -= 1;
        }
        self.marks[0] = at_tick;
    }

    /// 判据二满足位。
    pub fn confirmed(&self) -> bool {
        self.confirmed
    }

    /// 当前连续低率样本数。
    pub fn low_streak(&self) -> u32 {
        self.low_streak
    }

    /// 累计翻转次数。
    pub fn transitions(&self) -> u64 {
        self.transitions
    }

    /// 上次翻转 tick。
    pub fn last_change_tick(&self) -> u64 {
        self.last_change_tick
    }

    /// 最近 `window` tick 内的翻转次数（振荡观测：高频翻转=抖动源）。
    pub fn flaps_within(&self, window: u64, now: u64) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < TRANSITION_MARKS {
            let m = self.marks[i];
            if m != 0 && now.saturating_sub(m) <= window {
                n = n.saturating_add(1);
            }
            i += 1;
        }
        n
    }
}

impl Default for HysteresisGate {
    fn default() -> Self {
        HysteresisGate::new()
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 六、事件率采样窗（16 tick 定宽滑窗）
// ═══════════════════════════════════════════════════════════════════════

/// 事件率采样窗：16 个 tick 桶，窗宽 `RATE_WINDOW_TICKS`。
/// 边界结算：每逢窗边界把窗计数喂滞回门并清窗重对齐。
#[derive(Debug, Clone, Copy)]
pub struct RateWindow {
    /// tick 桶（桶 i 对应 [base+i, base+i+1) 内的事件数）。
    buckets: [u32; RATE_WINDOW_TICKS],
    /// 桶 0 对应的起始 tick。
    base: u64,
    /// 累计结算次数（恒等账用）。
    samples: u64,
}

impl RateWindow {
    /// 新建：全零桶，base=0。
    pub fn new() -> Self {
        RateWindow {
            buckets: [0u32; RATE_WINDOW_TICKS],
            base: 0,
            samples: 0,
        }
    }

    /// 登记 tick 时点发生的一个事件（saturating 防桶溢出）。
    pub fn record(&mut self, tick: u64) {
        let idx = (tick.wrapping_sub(self.base) % RATE_WINDOW_TICKS as u64) as usize;
        let cur = self.buckets[idx];
        self.buckets[idx] = cur.saturating_add(1);
    }

    /// 推进对齐：elapsed ≥ 窗宽时整窗清零并重对齐到当前 tick。
    /// 正常驱动下由引擎 tick 的边界结算保持对齐，本方法是防御性兜底。
    pub fn advance_to(&mut self, tick: u64) {
        let elapsed = tick.saturating_sub(self.base);
        if elapsed >= RATE_WINDOW_TICKS as u64 {
            self.buckets = [0u32; RATE_WINDOW_TICKS];
            self.base = tick;
        }
    }

    /// 边界结算：清窗并把 base 对齐到 `at_tick`（新窗起点）。
    pub fn clear(&mut self, at_tick: u64) {
        self.buckets = [0u32; RATE_WINDOW_TICKS];
        self.base = at_tick;
        self.samples = self.samples.wrapping_add(1);
    }

    /// 窗内事件总数（while 累加，saturating）。
    pub fn sum(&self) -> u64 {
        let mut s = 0u64;
        let mut i = 0usize;
        while i < RATE_WINDOW_TICKS {
            s = s.saturating_add(self.buckets[i] as u64);
            i += 1;
        }
        s
    }

    /// 窗起点。
    pub fn base(&self) -> u64 {
        self.base
    }

    /// 累计结算次数。
    pub fn samples(&self) -> u64 {
        self.samples
    }
}

impl Default for RateWindow {
    fn default() -> Self {
        RateWindow::new()
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 七、pending 账（逆回窗事件账：零丢失的第一半）
// ═══════════════════════════════════════════════════════════════════════

/// 逆回窗内暂扣的事件（入 pending 账，Polling 进入后全量补投）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PendingEvent {
    /// 全局事件序号（永不清零，跨轮连续）。
    pub seq: u64,
    /// 中断向量号。
    pub vector: u16,
    /// 事件到达 tick。
    pub tick: u64,
}

/// pending 账：定容 Vec，超容报 `PendingOverflow`。
/// 零丢失语义：逆回窗内一切事件必须先入账，账面序号全局连续。
#[derive(Debug)]
pub struct PendingLedger {
    /// 事件记录。
    events: Vec<PendingEvent>,
    /// 容量上限。
    cap: usize,
}

impl PendingLedger {
    /// 新建：空账，容量 `PENDING_CAP`。
    pub fn new() -> Self {
        PendingLedger {
            events: Vec::new(),
            cap: PENDING_CAP,
        }
    }

    /// 入账。溢出返回 false（调用方落红账+报错），不静默丢弃。
    pub fn push(&mut self, ev: PendingEvent) -> Result<(), ()> {
        if self.events.len() >= self.cap {
            return Err(());
        }
        self.events.push(ev);
        Ok(())
    }

    /// 账面条数。
    pub fn count(&self) -> usize {
        self.events.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// 按 get 取第 i 条（越界 None，零 panic 纪律）。
    pub fn at(&self, i: usize) -> Option<PendingEvent> {
        match self.events.get(i) {
            Some(e) => Some(*e),
            None => None,
        }
    }

    /// 首条序号。
    pub fn first_seq(&self) -> Option<u64> {
        match self.events.first() {
            Some(e) => Some(e.seq),
            None => None,
        }
    }

    /// 末条序号。
    pub fn last_seq(&self) -> Option<u64> {
        match self.events.last() {
            Some(e) => Some(e.seq),
            None => None,
        }
    }

    /// 序号连续性核查：账内相邻序号差必须恰为 1。
    /// 这是「事件零丢失（序号连续断言）」的第一现场。
    pub fn is_consecutive(&self) -> bool {
        let n = self.events.len();
        if n <= 1 {
            return true;
        }
        let mut i = 1usize;
        while i < n {
            let prev = match self.events.get(i - 1) {
                Some(e) => e.seq,
                None => return false,
            };
            let cur = match self.events.get(i) {
                Some(e) => e.seq,
                None => return false,
            };
            if cur != prev.wrapping_add(1) {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 全量取出（补投）：置换为空账返回旧账。
    pub fn drain_all(&mut self) -> Vec<PendingEvent> {
        core::mem::replace(&mut self.events, Vec::new())
    }

    /// 清账（协议复位用；清前调用方必须已对平）。
    pub fn clear(&mut self) {
        self.events.clear();
    }
}

impl Default for PendingLedger {
    fn default() -> Self {
        PendingLedger::new()
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 八、补投账（Polling 泵补投记录：零丢失的第二半）
// ═══════════════════════════════════════════════════════════════════════

/// 补投记录：Polling 泵把一条 pending 事件重新投递的凭证。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReplayRecord {
    /// 事件序号（与 pending 账一致，对平键）。
    pub seq: u64,
    /// 中断向量号。
    pub vector: u16,
    /// 补投发生 tick。
    pub at_tick: u64,
}

/// 补投账：定容环形（滚动保底）+ 累计恒等账。
/// 环形只保最近 `REPLAY_RING_CAP` 条凭证，累计首末序号/总数永不回退，
/// 对平恒等式建立在累计账之上（环形淘汰不破坏对平）。
#[derive(Debug)]
pub struct ReplayLedger {
    /// 环形存储（长度 ≤ cap）。
    ring: Vec<ReplayRecord>,
    /// 下一写入位。
    head: usize,
    /// 容量。
    cap: usize,
    /// 累计入账总数（wrapping）。
    total: u64,
    /// 累计首序号。
    first_seq_ever: Option<u64>,
    /// 累计末序号。
    last_seq_ever: Option<u64>,
    /// 末次补投 tick。
    last_tick: u64,
}

impl ReplayLedger {
    /// 新建：空环。
    pub fn new() -> Self {
        ReplayLedger {
            ring: Vec::new(),
            head: 0,
            cap: REPLAY_RING_CAP,
            total: 0,
            first_seq_ever: None,
            last_seq_ever: None,
            last_tick: 0,
        }
    }

    /// 入账（环形覆盖最旧；累计账单调推进）。
    pub fn push(&mut self, rec: ReplayRecord) {
        if self.ring.len() < self.cap {
            self.ring.push(rec);
        } else {
            let slot = self.head % self.cap;
            if let Some(s) = self.ring.get_mut(slot) {
                *s = rec;
            } else {
                // 理论不可达（len==cap 时 slot 必在界内）；防御性扩容不入红账
                self.ring.push(rec);
                self.cap = self.ring.len();
            }
        }
        self.head = self.head.wrapping_add(1);
        self.total = self.total.wrapping_add(1);
        self.first_seq_ever = match self.first_seq_ever {
            Some(_) => self.first_seq_ever,
            None => Some(rec.seq),
        };
        self.last_seq_ever = Some(rec.seq);
        self.last_tick = rec.at_tick;
    }

    /// 累计总数。
    pub fn total(&self) -> u64 {
        self.total
    }

    /// 累计首序号。
    pub fn first_seq(&self) -> Option<u64> {
        self.first_seq_ever
    }

    /// 累计末序号。
    pub fn last_seq(&self) -> Option<u64> {
        self.last_seq_ever
    }

    /// 末次补投 tick。
    pub fn last_tick(&self) -> u64 {
        self.last_tick
    }

    /// 环内现存条数。
    pub fn resident(&self) -> usize {
        self.ring.len()
    }

    /// 取最近 n 条（时间升序返回；n 超过现存取现存）。
    pub fn latest(&self, n: usize) -> Vec<ReplayRecord> {
        let resident = self.ring.len();
        let want = if n < resident { n } else { resident };
        let start = resident - want;
        let mut out = Vec::new();
        let mut i = start;
        while i < resident {
            match self.ring.get(i) {
                Some(r) => out.push(*r),
                None => break,
            }
            i += 1;
        }
        out
    }
}

impl Default for ReplayLedger {
    fn default() -> Self {
        ReplayLedger::new()
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 九、判据快照与审计环形账（逆回飞行记录 + 回放重演）
// ═══════════════════════════════════════════════════════════════════════

/// 判据快照：评估时点的三判据取值与结论（评估-执行双段协议的凭证）。
/// 纯数值结构（可 Copy），含 Vec 的结构体禁 Copy。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CritSnapshot {
    /// 触发原因 id。
    pub cause_id: u8,
    /// 评估时 tick。
    pub evaluated_tick: u64,
    /// 静默窗现值（tick）。
    pub silence_ticks: u64,
    /// 静默窗判据满足位。
    pub silence_met: bool,
    /// 事件率窗现值（最近窗计数）。
    pub window_events: u64,
    /// 事件率判据满足位（滞回门确认位）。
    pub rate_met: bool,
    /// 冷却期现值（tick；首轮无前次逆回取 u64::MAX 语义恒满足）。
    pub cooldown_ticks: u64,
    /// 冷却判据满足位。
    pub cooldown_met: bool,
    /// 总结论（三判据全过）。
    pub verdict: bool,
    /// 首个未达标判据 id（全过为 0）。
    pub fail_id: u16,
}

impl CritSnapshot {
    /// 快照原因码与给定原因是否一致（运行期比较，用于协议对平）。
    pub fn matches_cause(&self, cause: ReverseTriggerCause) -> bool {
        self.cause_id == cause.id()
    }
}

/// 审计结论。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuditOutcome {
    /// 全序三步走完，逆回成立。
    Completed,
    /// 拦截未遂（未走完全序，仅留痕）。
    Intercepted,
}

/// 审计条目：一次逆回的飞行记录五元组扩展
/// （原因码 + 判据快照 + 耗时 + 窗账 + 全序掩码）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AuditEntry {
    /// 审计流水号（逆回轮次序号，从 1 起）。
    pub round_no: u64,
    /// 触发原因 id。
    pub cause_id: u8,
    /// 判据快照（评估时点）。
    pub crit: CritSnapshot,
    /// 逆回窗开启 tick。
    pub open_tick: u64,
    /// 逆回窗收口 tick（PollingEnter）。
    pub close_tick: u64,
    /// 逆回耗时（tick）。
    pub duration_ticks: u64,
    /// 全序步掩码（bit0=撤防 bit1=Irq退出 bit2=Polling进入，完成应为 0b111）。
    pub steps_mask: u8,
    /// 逆回窗内入账事件数。
    pub window_events: u64,
    /// 窗内首序号（0 表示无事件）。
    pub window_first_seq: u64,
    /// 窗内末序号（0 表示无事件）。
    pub window_last_seq: u64,
    /// 结论。
    pub outcome: AuditOutcome,
}

/// 审计环形账：定容 64 条滚动保底 + 累计总数。
/// 提供「回放重演」出口：按时间升序取出全部留存条目。
#[derive(Debug)]
pub struct AuditRing {
    /// 环形存储。
    entries: Vec<AuditEntry>,
    /// 下一写入位。
    head: usize,
    /// 容量。
    cap: usize,
    /// 累计入账总数（wrapping）。
    total: u64,
}

impl AuditRing {
    /// 新建：空环。
    pub fn new() -> Self {
        AuditRing {
            entries: Vec::new(),
            head: 0,
            cap: AUDIT_RING_CAP,
            total: 0,
        }
    }

    /// 入账（环形覆盖最旧）。
    pub fn push(&mut self, entry: AuditEntry) {
        if self.entries.len() < self.cap {
            self.entries.push(entry);
        } else {
            let slot = self.head % self.cap;
            if let Some(s) = self.entries.get_mut(slot) {
                *s = entry;
            } else {
                self.entries.push(entry);
                self.cap = self.entries.len();
            }
        }
        self.head = self.head.wrapping_add(1);
        self.total = self.total.wrapping_add(1);
    }

    /// 累计总数。
    pub fn total(&self) -> u64 {
        self.total
    }

    /// 环内现存条数。
    pub fn resident(&self) -> usize {
        self.entries.len()
    }

    /// 按逻辑序（最旧在前）取第 i 条留存条目。
    pub fn at(&self, i: usize) -> Option<AuditEntry> {
        let resident = self.entries.len();
        if i >= resident {
            return None;
        }
        let start = if resident == self.cap {
            self.head % self.cap
        } else {
            0
        };
        let phys = (start + i) % self.cap;
        match self.entries.get(phys) {
            Some(e) => Some(*e),
            None => None,
        }
    }

    /// 回放重演：按时间升序取出全部留存条目。
    pub fn replay_all(&self) -> Vec<AuditEntry> {
        let mut out = Vec::new();
        let mut i = 0usize;
        while i < self.entries.len() {
            match self.at(i) {
                Some(e) => out.push(e),
                None => break,
            }
            i += 1;
        }
        out
    }

    /// 按原因 id 过滤回放。
    pub fn find_by_cause(&self, cause_id: u8) -> Vec<AuditEntry> {
        let mut out = Vec::new();
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.at(i) {
                if e.cause_id == cause_id {
                    out.push(e);
                }
            }
            i += 1;
        }
        out
    }
}

impl Default for AuditRing {
    fn default() -> Self {
        AuditRing::new()
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 十、红账（协议违规必抓账：定长滚动，淘汰留痕）
// ═══════════════════════════════════════════════════════════════════════

/// 红账事件类别（每类对应一个诊断码）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RedKind {
    /// 非法逆回（带未确认向量仍申请逆回）。
    IllegalReversal {
        /// 触发原因 id。
        cause_id: u8,
        /// 当时未确认事件数。
        unacked: u32,
    },
    /// 振荡拦截（同驻留窗内正向迁移连续被拒达阈值）。
    OscillationBlocked {
        /// 被拒次数。
        attempts: u32,
    },
    /// pending 账溢出（零丢失被容量击穿）。
    PendingOverflow {
        /// 触发向量。
        vector: u16,
    },
    /// 补投序号断裂。
    SeqGap {
        /// 期望序号。
        expected: u64,
        /// 实得序号。
        got: u64,
    },
    /// 对平失败。
    ReconcileMismatch {
        /// pending 累计。
        pending_total: u64,
        /// 补投累计。
        replayed_total: u64,
    },
}

impl RedKind {
    /// 红账事件对应诊断码（0x1380 段内）。
    pub fn code(&self) -> u16 {
        match self {
            RedKind::IllegalReversal { .. } => CODE_UNACKED_VECTORS_PRESENT,
            RedKind::OscillationBlocked { .. } => CODE_OSCILLATION_SUSPECTED,
            RedKind::PendingOverflow { .. } => CODE_PENDING_OVERFLOW,
            RedKind::SeqGap { .. } => CODE_REPLAY_SEQ_GAP,
            RedKind::ReconcileMismatch { .. } => CODE_RECONCILE_MISMATCH,
        }
    }

    /// 红账事件中文短注。
    pub fn note(&self) -> &'static str {
        match self {
            RedKind::IllegalReversal { .. } => "非法逆回拦截：未确认向量未清，逆回=丢中断",
            RedKind::OscillationBlocked { .. } => "振荡拦截：Polling 驻留期内正向迁移被连拒",
            RedKind::PendingOverflow { .. } => "pending 账溢出：逆回窗积压超容量",
            RedKind::SeqGap { .. } => "补投序号断裂：零丢失恒等被破坏",
            RedKind::ReconcileMismatch { .. } => "对平失败：pending 账与补投账恒等不成立",
        }
    }
}

/// 红账条目。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RedEntry {
    /// 红账流水号。
    pub seq: u64,
    /// 发生 tick。
    pub tick: u64,
    /// 事件类别。
    pub kind: RedKind,
    /// 诊断码。
    pub code: u16,
}

/// 红账：定容滚动（满则淘汰最旧并计数留痕）。
/// 红账只增不静默：任何协议违规必须在此留下可回放痕迹。
#[derive(Debug)]
pub struct RedLedger {
    /// 条目存储（时间升序）。
    entries: Vec<RedEntry>,
    /// 容量。
    cap: usize,
    /// 红账流水计数。
    seq_counter: u64,
    /// 累计淘汰条数（容量击穿留痕）。
    evicted: u64,
    /// 按类别累计计数（恒等账：不随滚动淘汰丢失）。
    by_illegal: u64,
    by_oscillation: u64,
    by_overflow: u64,
    by_seqgap: u64,
    by_reconcile: u64,
}

impl RedLedger {
    /// 新建：空账。
    pub fn new() -> Self {
        RedLedger {
            entries: Vec::new(),
            cap: RED_CAP,
            seq_counter: 0,
            evicted: 0,
            by_illegal: 0,
            by_oscillation: 0,
            by_overflow: 0,
            by_seqgap: 0,
            by_reconcile: 0,
        }
    }

    /// 入账（满则淘汰最旧）。
    pub fn push(&mut self, tick: u64, kind: RedKind) -> RedEntry {
        self.seq_counter = self.seq_counter.wrapping_add(1);
        let entry = RedEntry {
            seq: self.seq_counter,
            tick,
            kind,
            code: kind.code(),
        };
        match kind {
            RedKind::IllegalReversal { .. } => self.by_illegal = self.by_illegal.wrapping_add(1),
            RedKind::OscillationBlocked { .. } => {
                self.by_oscillation = self.by_oscillation.wrapping_add(1)
            }
            RedKind::PendingOverflow { .. } => self.by_overflow = self.by_overflow.wrapping_add(1),
            RedKind::SeqGap { .. } => self.by_seqgap = self.by_seqgap.wrapping_add(1),
            RedKind::ReconcileMismatch { .. } => {
                self.by_reconcile = self.by_reconcile.wrapping_add(1)
            }
        }
        if self.entries.len() >= self.cap {
            // 满账淘汰最旧（Vec::remove(0) 定容小账开销可接受；淘汰计数留痕）
            self.entries.remove(0);
            self.evicted = self.evicted.wrapping_add(1);
        }
        self.entries.push(entry);
        entry
    }

    /// 现存条数。
    pub fn count(&self) -> usize {
        self.entries.len()
    }

    /// 是否为空（健康面红账清零判据）。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 累计淘汰数。
    pub fn evicted(&self) -> u64 {
        self.evicted
    }

    /// 累计入账总数（含被淘汰的）。
    pub fn total_ever(&self) -> u64 {
        self.seq_counter
    }

    /// 类别累计：非法逆回。
    pub fn total_illegal(&self) -> u64 {
        self.by_illegal
    }

    /// 类别累计：振荡拦截。
    pub fn total_oscillation(&self) -> u64 {
        self.by_oscillation
    }

    /// 类别累计：溢出。
    pub fn total_overflow(&self) -> u64 {
        self.by_overflow
    }

    /// 类别累计：序号断裂。
    pub fn total_seqgap(&self) -> u64 {
        self.by_seqgap
    }

    /// 类别累计：对平失败。
    pub fn total_reconcile(&self) -> u64 {
        self.by_reconcile
    }

    /// 全量快照（时间升序克隆）。
    pub fn snapshot(&self) -> Vec<RedEntry> {
        let mut out = Vec::new();
        let mut i = 0usize;
        while i < self.entries.len() {
            match self.entries.get(i) {
                Some(e) => out.push(*e),
                None => break,
            }
            i += 1;
        }
        out
    }
}

impl Default for RedLedger {
    fn default() -> Self {
        RedLedger::new()
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 十一、逆回窗账与统计账
// ═══════════════════════════════════════════════════════════════════════

/// 逆回窗账：PollingEnter 收口时对 pending 账的定格快照（对平基准）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowAccount {
    /// 窗内入账事件数。
    pub events: u64,
    /// 窗内首序号（events==0 时为 0）。
    pub first_seq: u64,
    /// 窗内末序号（events==0 时为 0）。
    pub last_seq: u64,
    /// 窗开启 tick。
    pub open_tick: u64,
    /// 窗收口 tick。
    pub close_tick: u64,
}

/// 协议统计账（全 wrapping 计数，持久恒等账）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RevStats {
    /// 事件总数（含分发与入账）。
    pub events_total: u64,
    /// 正常分发数。
    pub events_dispatched: u64,
    /// pending 累计入账数。
    pub pending_total: u64,
    /// 补投累计数。
    pub replayed_total: u64,
    /// 轮询泵累计投递数（补投 + Polling 态直投）。
    pub polling_pump_delivered: u64,
    /// 逆回申请数（begin_reverse 成功开窗数）。
    pub reversal_attempts: u64,
    /// 逆回完成数（PollingEnter 收口数）。
    pub reverse_count: u64,
    /// 正向迁移被拒数。
    pub forward_blocked: u64,
    /// 正向迁移放行数。
    pub forward_granted: u64,
    /// 振荡红账次数。
    pub oscillation_red: u64,
    /// 全序乱序拒绝数。
    pub order_violations: u64,
    /// pending 溢出数。
    pub pending_overflow: u64,
    /// 正向链完整轮数（回到 Irq 态的轮数）。
    pub rounds_completed: u64,
    /// 末次逆回耗时（tick）。
    pub last_reverse_duration_ticks: u64,
    /// 最大逆回耗时（tick）。
    pub max_reverse_duration_ticks: u64,
}

impl RevStats {
    /// 新建：全零账。
    pub const fn new() -> Self {
        RevStats {
            events_total: 0,
            events_dispatched: 0,
            pending_total: 0,
            replayed_total: 0,
            polling_pump_delivered: 0,
            reversal_attempts: 0,
            reverse_count: 0,
            forward_blocked: 0,
            forward_granted: 0,
            oscillation_red: 0,
            order_violations: 0,
            pending_overflow: 0,
            rounds_completed: 0,
            last_reverse_duration_ticks: 0,
            max_reverse_duration_ticks: 0,
        }
    }
}

impl Default for RevStats {
    fn default() -> Self {
        RevStats::new()
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 十二、协议票据与报告结构
// ═══════════════════════════════════════════════════════════════════════

/// 事件处置方式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventDisposition {
    /// 正常分发（top-half 路径）。
    Dispatched,
    /// 逆回窗暂扣入 pending 账（待补投）。
    HeldPending,
}

/// 事件票据：raise_event 的回执。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EventTicket {
    /// 全局事件序号。
    pub seq: u64,
    /// 向量号。
    pub vector: u16,
    /// 到达 tick。
    pub tick: u64,
    /// 处置方式。
    pub disposition: EventDisposition,
}

/// 逆回票据：begin_reverse 开窗回执。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReverseTicket {
    /// 轮次号（= 完成后的审计 round_no 预占）。
    pub round_no: u64,
    /// 触发原因 id。
    pub cause_id: u8,
    /// 窗开启 tick。
    pub open_tick: u64,
    /// 判据快照（随票据固化）。
    pub crit: CritSnapshot,
}

/// 补投报告。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReplayReport {
    /// 本轮补投条数。
    pub replayed: u64,
    /// 窗内首序号（None=窗内无事件）。
    pub first_seq: Option<u64>,
    /// 窗内末序号。
    pub last_seq: Option<u64>,
    /// 序号连续位。
    pub continuous: bool,
}

/// 对平报告。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReconcileReport {
    /// 窗账事件数。
    pub window_events: u64,
    /// 本轮补投数（与窗账须相等）。
    pub replayed_delta: u64,
    /// 累计恒等（pending_total == replayed_total）。
    pub cumulative_balanced: bool,
    /// 窗内序号端点对平位。
    pub endpoints_matched: bool,
    /// 总结论。
    pub balanced: bool,
}

/// 正向迁移放行凭证。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ForwardGrant {
    /// 放行 tick。
    pub at_tick: u64,
    /// 实际驻留 tick 数。
    pub residence_ticks: u64,
}

/// 注入类别。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InjectKind {
    /// 非法逆回注入。
    IllegalReversal,
    /// 逆回窗事件注入。
    WindowEvent,
    /// 振荡注入。
    Oscillation,
}

/// 注入结果（注入面自证：拦截是否发生、码是什么、注了什么）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InjectOutcome {
    /// 注入类别。
    pub kind: InjectKind,
    /// 是否被协议拦截/按预期处置。
    pub intercepted: bool,
    /// 观测到的诊断码（无违规为 0）。
    pub code: u16,
    /// 结果注记。
    pub note: String,
}

/// 场景摘要（双跑对拍的比对单位）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScenarioDigest {
    /// 引擎终态 FNV 指纹。
    pub fingerprint: u64,
    /// 场景轮数。
    pub rounds: u64,
    /// 事件总数。
    pub events_total: u64,
    /// pending 累计。
    pub pending_total: u64,
    /// 补投累计。
    pub replayed_total: u64,
    /// 红账累计。
    pub red_total: u64,
    /// 审计累计。
    pub audit_total: u64,
    /// 全局末序号。
    pub last_seq: u64,
}

/// 双跑对拍报告。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DualRunReport {
    /// 摘要（两跑一致时唯一）。
    pub digest: ScenarioDigest,
    /// 一致位。
    pub agree: bool,
    /// 场景推进的总 tick 数。
    pub total_ticks: u64,
}

/// 跨轮序号连续审计报告（UNX-F1263-J1「序号连续断言 10 轮」协议面出口）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SeqContinuityReport {
    /// 参审逆回轮数。
    pub rounds: u64,
    /// 跨轮断裂数（0=连续）。
    pub gaps: u32,
    /// 总连续位。
    pub continuous: bool,
    /// 全局首序号（首条补投；None=尚无补投）。
    pub first_seq: Option<u64>,
    /// 全局末序号。
    pub last_seq: Option<u64>,
}

/// 健康结论。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HealthVerdict {
    /// 全绿。
    Green,
    /// 有抖动嫌疑但无违规。
    Yellow,
    /// 红账非空/守恒破坏。
    Red,
}

impl HealthVerdict {
    /// 中文名。
    pub const fn name(self) -> &'static str {
        match self {
            HealthVerdict::Green => "绿",
            HealthVerdict::Yellow => "黄",
            HealthVerdict::Red => "红",
        }
    }
}

/// 单项体检结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HealthItem {
    /// 体检项 id（1~8）。
    pub id: u8,
    /// 体检项名。
    pub name: &'static str,
    /// 是否通过。
    pub ok: bool,
    /// 中文注记。
    pub note: String,
}

/// 体检报告。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HealthReport {
    /// 总结论。
    pub verdict: HealthVerdict,
    /// 逐项结果。
    pub items: Vec<HealthItem>,
    /// 一句话摘要。
    pub summary: String,
}

/// 判据现值视图（explain/测试面用）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CriterionView {
    /// 判据 id。
    pub id: u16,
    /// 判据名。
    pub name: &'static str,
    /// 当前值。
    pub current: u64,
    /// 阈值。
    pub threshold: u64,
    /// 是否达标。
    pub met: bool,
}

// ═══════════════════════════════════════════════════════════════════════
// 十三、确定性伪随机源（双跑对拍的场景驱动）
// ═══════════════════════════════════════════════════════════════════════

/// 线性同余伪随机（Knuth MMIX 常量）。
/// 双跑确定性：同 seed 同序列，场景驱动零外部时序依赖。
/// 常量来源：Donald E. Knuth, TAOCP Vol.2, MMIX LCG
/// (a=6364136223846793005, c=1442695040888963407)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Lcg {
    state: u64,
}

impl Lcg {
    /// 新建：seed 直置状态（0 亦合法）。
    pub const fn new(seed: u64) -> Self {
        Lcg { state: seed }
    }

    /// 下一伪随机数（wrapping 乘加）。
    pub fn next(&mut self) -> u64 {
        self.state = self
            .state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.state
    }

    /// [0, bound) 内取数（bound=0 返回 0）。
    pub fn below(&mut self, bound: u64) -> u64 {
        if bound == 0 {
            return 0;
        }
        self.next() % bound
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 十四、逆回协议引擎本体（ReverseTransitionEngine）
// ═══════════════════════════════════════════════════════════════════════

/// Irq→Polling 逆回协议引擎。
///
/// 生命周期：构造即处于 Polling 态 Dormant 相位（轮询泵在位）；
/// `arm_vectors` 进入 Irq 态后开始承接正向中断；判据满足时
/// evaluate→begin_reverse 开逆回窗，按全序三步收口至 Polling 并补投；
/// 驻留期满走正向链 Draining→Arming→Irq 回到正向，构成完整闭环。
///
/// 结构体含 Vec 堆件，不 derive Copy；尺寸约 1.6KB 级，不上栈拷贝。
pub struct ReverseTransitionEngine {
    /// 五态机当前态。
    state: IrqState,
    /// 逆回协议相位。
    phase: ReversePhase,
    /// 协议时钟（tick，wrapping 推进）。
    tick: u64,
    /// 全局事件序号计数（永不清零）。
    seq_counter: u64,
    /// 全序步游标（0/1/2）。
    order_cursor: u8,
    /// 全序步掩码（bit0/bit1/bit2）。
    steps_mask: u8,
    /// 撤防完成位。
    disarmed: bool,
    /// Irq 退出完成位。
    irq_exited: bool,
    /// 逆回窗开启 tick。
    window_open_tick: u64,
    /// 已收判据快照（开窗时固化）。
    captured: Option<CritSnapshot>,
    /// 逆回窗账（收口定格，对平基准）。
    window_account: Option<WindowAccount>,
    /// 本轮补投基线（replay 前的 replayed_total）。
    round_replay_base: u64,
    /// 本轮补投首序号。
    round_replay_first: Option<u64>,
    /// 本轮补投末序号。
    round_replay_last: Option<u64>,
    /// Polling 驻留起点（Completed 时置位）。
    residence_since: Option<u64>,
    /// 上次逆回完成 tick（冷却期基准）。
    last_reverse_end_tick: Option<u64>,
    /// 同驻留窗内正向迁移被拒计数。
    blocked_attempts: u32,
    /// 未确认向量账（向量号 → 未确认计数）。
    unacked: Vec<(u16, u32)>,
    /// 武装向量数（arm_vectors 登记）。
    armed_vectors: u64,
    /// 最后事件 tick（静默窗基准）。
    last_event_tick: u64,
    /// 事件率采样窗。
    rate_window: RateWindow,
    /// 滞回门（判据二）。
    gate: HysteresisGate,
    /// pending 账（逆回窗事件账）。
    pending: PendingLedger,
    /// 补投账。
    replay: ReplayLedger,
    /// 审计环形账。
    audit: AuditRing,
    /// 红账。
    red: RedLedger,
    /// 统计账。
    stats: RevStats,
    /// 封账位。
    sealed: bool,
}

impl ReverseTransitionEngine {
    /// 新建引擎：Polling 态 Completed 相位，tick=0，驻留计时自 0 起。
    /// （启动即视作"轮询泵接管完成驻留中"——启动序列惯例：轮询泵先在位，
    /// 驻留期满正向走 Draining→Arming→Irq 后承接正向中断。）
    pub fn new() -> Self {
        ReverseTransitionEngine {
            state: IrqState::Polling,
            phase: ReversePhase::Completed,
            tick: 0,
            seq_counter: 0,
            order_cursor: 0,
            steps_mask: 0,
            disarmed: false,
            irq_exited: false,
            window_open_tick: 0,
            captured: None,
            window_account: None,
            round_replay_base: 0,
            round_replay_first: None,
            round_replay_last: None,
            residence_since: Some(0),
            last_reverse_end_tick: None,
            blocked_attempts: 0,
            unacked: Vec::new(),
            armed_vectors: 0,
            last_event_tick: 0,
            rate_window: RateWindow::new(),
            gate: HysteresisGate::new(),
            pending: PendingLedger::new(),
            replay: ReplayLedger::new(),
            audit: AuditRing::new(),
            red: RedLedger::new(),
            stats: RevStats::new(),
            sealed: false,
        }
    }

    // ── 时钟与观测面 ─────────────────────────────────────────────────

    /// 推进协议时钟 n tick。
    ///
    /// 边界结算：每逢 16 tick 窗边界把窗计数喂滞回门并清窗重对齐；
    /// 长推（> TICK_JUMP_THRESHOLD）先按当前窗结算一次再整段跳进
    /// （确定性保持：同参数同序列）。
    pub fn tick(&mut self, n: u64) {
        let mut remaining = n;
        while remaining > 0 {
            if remaining > TICK_JUMP_THRESHOLD {
                // 粗跳：先结算当前窗，再整段推进
                let s = self.rate_window.sum();
                self.gate.feed(s, self.tick);
                self.tick = self.tick.wrapping_add(remaining);
                self.rate_window.clear(self.tick);
                remaining = 0;
            } else {
                let modpos = self.tick % RATE_WINDOW_TICKS as u64;
                let to_boundary = RATE_WINDOW_TICKS as u64 - modpos;
                let step = if remaining < to_boundary {
                    remaining
                } else {
                    to_boundary
                };
                self.tick = self.tick.wrapping_add(step);
                remaining -= step;
                self.rate_window.advance_to(self.tick);
                if self.tick % (RATE_WINDOW_TICKS as u64) == 0 {
                    let s = self.rate_window.sum();
                    self.gate.feed(s, self.tick);
                    self.rate_window.clear(self.tick);
                }
            }
        }
    }

    /// 当前 tick。
    pub fn tick_now(&self) -> u64 {
        self.tick
    }

    /// 当前态。
    pub fn state(&self) -> IrqState {
        self.state
    }

    /// 当前相位。
    pub fn phase(&self) -> ReversePhase {
        self.phase
    }

    /// 统计账只读视图。
    pub fn stats(&self) -> RevStats {
        self.stats
    }

    /// 是否已封账。
    pub fn is_sealed(&self) -> bool {
        self.sealed
    }

    /// 封账：之后一切写操作拒绝（LedgerSealed）。
    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// 事件率窗现值。
    pub fn rate_window_sum(&self) -> u64 {
        self.rate_window.sum()
    }

    /// 滞回门确认位（判据二现值）。
    pub fn rate_confirmed(&self) -> bool {
        self.gate.confirmed()
    }

    /// 滞回门只读视图（翻转历史/防抖进度）。
    pub fn gate_view(&self) -> (u32, bool, u64, u32) {
        (
            self.gate.low_streak(),
            self.gate.confirmed(),
            self.gate.transitions(),
            self.gate.flaps_within(256, self.tick),
        )
    }

    // ── 未确认向量账 ────────────────────────────────────────────────

    /// 登记一条未确认（同向量计数累加）。
    fn unacked_add(&mut self, vector: u16) {
        let mut found = false;
        let mut i = 0usize;
        while i < self.unacked.len() {
            if let Some(slot) = self.unacked.get_mut(i) {
                if slot.0 == vector {
                    slot.1 = slot.1.saturating_add(1);
                    found = true;
                    break;
                }
            }
            i += 1;
        }
        if !found {
            self.unacked.push((vector, 1));
        }
    }

    /// 确认一条中断（EOI）：计数递减，归零移除。返回是否确有在飞。
    pub fn ack_vector(&mut self, vector: u16) -> bool {
        let mut i = 0usize;
        while i < self.unacked.len() {
            let hit = match self.unacked.get(i) {
                Some(slot) => slot.0 == vector,
                None => false,
            };
            if hit {
                // 命中：递减或移除（get_mut 必中，防御性 match）
                let remove = match self.unacked.get_mut(i) {
                    Some(slot) => {
                        slot.1 = slot.1.saturating_sub(1);
                        slot.1 == 0
                    }
                    None => false,
                };
                if remove {
                    self.unacked.remove(i);
                }
                return true;
            }
            i += 1;
        }
        false
    }

    /// 未确认事件总数。
    pub fn unacked_count(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.unacked.len() {
            if let Some(slot) = self.unacked.get(i) {
                n = n.saturating_add(slot.1);
            }
            i += 1;
        }
        n
    }

    /// 未确认账是否为空。
    pub fn unacked_is_empty(&self) -> bool {
        self.unacked.is_empty()
    }

    /// 未确认向量号快照（升序克隆）。
    pub fn unacked_vectors(&self) -> Vec<u16> {
        let mut out = Vec::new();
        let mut i = 0usize;
        while i < self.unacked.len() {
            if let Some(slot) = self.unacked.get(i) {
                out.push(slot.0);
            }
            i += 1;
        }
        out
    }

    // ── 事件入口 ────────────────────────────────────────────────────

    /// 中断事件入口。
    ///
    /// * Irq 态 Dormant：正常分发 + 未确认账登记（raise-EOI 配对面）；
    /// * Irq 态 WindowOpen/Reversing：逆回窗零丢失——入 pending 账；
    /// * Polling 态：轮询泵直投（接管覆盖计数）；
    /// * 其余态：拒收（协议面不开票，NotInResidentState）。
    pub fn raise_event(&mut self, vector: u16) -> IrqRevResult<EventTicket> {
        if self.sealed {
            return Err(IrqRevError::LedgerSealed);
        }
        self.rate_window.advance_to(self.tick);
        let seq = self.seq_counter.wrapping_add(1);
        self.seq_counter = seq;
        self.stats.events_total = self.stats.events_total.wrapping_add(1);
        self.last_event_tick = self.tick;
        match self.state {
            IrqState::Irq => match self.phase {
                ReversePhase::Dormant => {
                    self.stats.events_dispatched = self.stats.events_dispatched.wrapping_add(1);
                    self.unacked_add(vector);
                    self.rate_window.record(self.tick);
                    Ok(EventTicket {
                        seq,
                        vector,
                        tick: self.tick,
                        disposition: EventDisposition::Dispatched,
                    })
                }
                ReversePhase::WindowOpen | ReversePhase::Reversing => {
                    self.stats.pending_total = self.stats.pending_total.wrapping_add(1);
                    self.rate_window.record(self.tick);
                    let ev = PendingEvent {
                        seq,
                        vector,
                        tick: self.tick,
                    };
                    match self.pending.push(ev) {
                        Ok(()) => Ok(EventTicket {
                            seq,
                            vector,
                            tick: self.tick,
                            disposition: EventDisposition::HeldPending,
                        }),
                        Err(()) => {
                            // 溢出：账面回滚 + 红账 + 显性报错（不静默丢）
                            self.stats.pending_total = self.stats.pending_total.wrapping_sub(1);
                            self.stats.pending_overflow = self.stats.pending_overflow.wrapping_add(1);
                            self.red.push(
                                self.tick,
                                RedKind::PendingOverflow { vector },
                            );
                            Err(IrqRevError::PendingOverflow { vector })
                        }
                    }
                }
                _ => Err(IrqRevError::NotInResidentState {
                    state: self.state,
                    phase: self.phase,
                }),
            },
            IrqState::Polling => {
                // 轮询泵直投：逆回后接管覆盖计数（J1 接管完整率的观测点）
                self.stats.events_dispatched = self.stats.events_dispatched.wrapping_add(1);
                self.stats.polling_pump_delivered =
                    self.stats.polling_pump_delivered.wrapping_add(1);
                self.rate_window.record(self.tick);
                Ok(EventTicket {
                    seq,
                    vector,
                    tick: self.tick,
                    disposition: EventDisposition::Dispatched,
                })
            }
            _ => Err(IrqRevError::NotInResidentState {
                state: self.state,
                phase: self.phase,
            }),
        }
    }

    // ── 判据评估（评估-执行双段协议第一段） ─────────────────────────

    /// 三判据现值视图（explain/测试面用）。
    pub fn criteria_view(&self) -> [CriterionView; 3] {
        let silence = self.tick.saturating_sub(self.last_event_tick);
        let cooldown = match self.last_reverse_end_tick {
            Some(t) => self.tick.saturating_sub(t),
            None => u64::MAX,
        };
        [
            CriterionView {
                id: CRIT_ID_SILENCE,
                name: crit_name(CRIT_ID_SILENCE),
                current: silence,
                threshold: CRIT_SILENCE_WINDOW_TICKS,
                met: silence >= CRIT_SILENCE_WINDOW_TICKS,
            },
            CriterionView {
                id: CRIT_ID_RATE,
                name: crit_name(CRIT_ID_RATE),
                current: self.rate_window.sum(),
                threshold: CRIT_RATE_FLOOR_EVENTS,
                met: self.gate.confirmed(),
            },
            CriterionView {
                id: CRIT_ID_COOLDOWN,
                name: crit_name(CRIT_ID_COOLDOWN),
                current: cooldown,
                threshold: CRIT_REVERSE_COOLDOWN_TICKS,
                met: cooldown >= CRIT_REVERSE_COOLDOWN_TICKS,
            },
        ]
    }

    /// 评估逆回资格（第一段）：产出判据快照；任一判据未满足即报错。
    ///
    /// 快照是执行段的准入凭证：begin_reverse 必须携带同原因、新鲜的快照。
    pub fn evaluate_reverse_eligibility(
        &self,
        cause: ReverseTriggerCause,
    ) -> IrqRevResult<CritSnapshot> {
        let silence = self.tick.saturating_sub(self.last_event_tick);
        let silence_met = silence >= CRIT_SILENCE_WINDOW_TICKS;
        let rate_met = self.gate.confirmed();
        let cooldown = match self.last_reverse_end_tick {
            Some(t) => self.tick.saturating_sub(t),
            None => u64::MAX,
        };
        let cooldown_met = cooldown >= CRIT_REVERSE_COOLDOWN_TICKS;
        let fail_id: u16 = if !silence_met {
            CRIT_ID_SILENCE
        } else if !rate_met {
            CRIT_ID_RATE
        } else if !cooldown_met {
            CRIT_ID_COOLDOWN
        } else {
            0
        };
        let snapshot = CritSnapshot {
            cause_id: cause.id(),
            evaluated_tick: self.tick,
            silence_ticks: silence,
            silence_met,
            window_events: self.rate_window.sum(),
            rate_met,
            cooldown_ticks: cooldown,
            cooldown_met,
            verdict: fail_id == 0,
            fail_id,
        };
        if fail_id != 0 {
            return Err(IrqRevError::CriteriaNotMet {
                criterion_id: fail_id,
                criterion_name: crit_name(fail_id),
            });
        }
        Ok(snapshot)
    }

    // ── 逆回执行（评估-执行双段协议第二段） ─────────────────────────

    /// 开逆回窗（第二段）：校验快照一致性 → 拦截非法逆回 → 开窗。
    ///
    /// 合法前置条件表（逐条判定，任一不过即拒）：
    ///   1. 未封账（0x138C）；
    ///   2. Irq 态驻留（0x1381）；
    ///   3. Dormant 相位（0x1382）；
    ///   4. 快照原因码与本调用原因一致（0x138E）；
    ///   5. 快照新鲜（评估后 ≤ SNAPSHOT_FRESH_TICKS，0x138E）；
    ///   6. 快照结论为可逆回（0x1383）；
    ///   7. 未确认向量为空——Irq 态带未确认向量逆回=丢中断（0x1385+红账）。
    pub fn begin_reverse(
        &mut self,
        cause: ReverseTriggerCause,
        snapshot: CritSnapshot,
    ) -> IrqRevResult<ReverseTicket> {
        if self.sealed {
            return Err(IrqRevError::LedgerSealed);
        }
        if self.state != IrqState::Irq {
            return Err(IrqRevError::NotInResidentState {
                state: self.state,
                phase: self.phase,
            });
        }
        if self.phase != ReversePhase::Dormant {
            return Err(IrqRevError::AlreadyReversing);
        }
        if !snapshot.matches_cause(cause) {
            return Err(IrqRevError::SnapshotMismatch {
                reason: "原因码不匹配",
            });
        }
        let age = self.tick.saturating_sub(snapshot.evaluated_tick);
        if age > SNAPSHOT_FRESH_TICKS {
            return Err(IrqRevError::SnapshotMismatch {
                reason: "快照已过期",
            });
        }
        if !snapshot.verdict {
            return Err(IrqRevError::CriteriaNotMet {
                criterion_id: snapshot.fail_id,
                criterion_name: crit_name(snapshot.fail_id),
            });
        }
        let unacked = self.unacked_count();
        if unacked > 0 {
            // 非法逆回拦截：逆回前必须确认到齐全（逆回=丢中断）
            self.red.push(
                self.tick,
                RedKind::IllegalReversal {
                    cause_id: cause.id(),
                    unacked,
                },
            );
            return Err(IrqRevError::UnackedVectorsPresent { count: unacked });
        }
        // 开窗：全序游标归零，快照固化
        self.phase = ReversePhase::WindowOpen;
        self.order_cursor = 0;
        self.steps_mask = 0;
        self.disarmed = false;
        self.irq_exited = false;
        self.window_open_tick = self.tick;
        self.captured = Some(snapshot);
        self.stats.reversal_attempts = self.stats.reversal_attempts.wrapping_add(1);
        Ok(ReverseTicket {
            round_no: self.stats.reverse_count.wrapping_add(1),
            cause_id: cause.id(),
            open_tick: self.tick,
            crit: snapshot,
        })
    }

    /// 提交一个全序步（次序即语义，乱序拒绝带码 0x1386）。
    ///
    /// 步一 DisarmArming：Arming 撤防（防御性复查未确认账，仍非空拦截）；
    /// 步二 IrqExit：状态机离开 Irq 态（逆回中）；
    /// 步三 PollingEnter：轮询泵接管 + 逆回窗定格收口 + 审计落账。
    pub fn advance_order_step(&mut self, step: ReverseOrderStep) -> IrqRevResult<ReverseOrderStep> {
        if self.sealed {
            return Err(IrqRevError::LedgerSealed);
        }
        let open = matches!(
            self.phase,
            ReversePhase::WindowOpen | ReversePhase::Reversing
        );
        if !open {
            return Err(IrqRevError::NotInResidentState {
                state: self.state,
                phase: self.phase,
            });
        }
        let expected = match ReverseOrderStep::expected_at(self.order_cursor) {
            0 => ReverseOrderStep::DisarmArming,
            1 => ReverseOrderStep::IrqExit,
            2 => ReverseOrderStep::PollingEnter,
            _ => ReverseOrderStep::PollingEnter,
        };
        if step != expected {
            self.stats.order_violations = self.stats.order_violations.wrapping_add(1);
            return Err(IrqRevError::OrderViolation { expected, got: step });
        }
        match step {
            ReverseOrderStep::DisarmArming => {
                // 撤防防御性复查：开窗后不允许再积未确认（窗内事件只入账不分发，
                // 故正常流不可能积；若积，说明外力旁路了协议——拦截）
                let unacked = self.unacked_count();
                if unacked > 0 {
                    self.red.push(
                        self.tick,
                        RedKind::IllegalReversal {
                            cause_id: match self.captured {
                                Some(s) => s.cause_id,
                                None => 0,
                            },
                            unacked,
                        },
                    );
                    return Err(IrqRevError::UnackedVectorsPresent { count: unacked });
                }
                self.disarmed = true;
                self.steps_mask |= 0b001;
                self.phase = ReversePhase::Reversing;
                self.order_cursor = self.order_cursor.saturating_add(1);
                Ok(step)
            }
            ReverseOrderStep::IrqExit => {
                self.irq_exited = true;
                self.steps_mask |= 0b010;
                self.phase = ReversePhase::Reversing;
                self.order_cursor = self.order_cursor.saturating_add(1);
                Ok(step)
            }
            ReverseOrderStep::PollingEnter => {
                self.irq_exited = false;
                self.steps_mask |= 0b100;
                // 逆回窗定格：对平基准
                let wa = WindowAccount {
                    events: self.pending.count() as u64,
                    first_seq: match self.pending.first_seq() {
                        Some(s) => s,
                        None => 0,
                    },
                    last_seq: match self.pending.last_seq() {
                        Some(s) => s,
                        None => 0,
                    },
                    open_tick: self.window_open_tick,
                    close_tick: self.tick,
                };
                self.window_account = Some(wa);
                let duration = self.tick.saturating_sub(self.window_open_tick);
                // 审计落账（原因码+判据快照+耗时+窗账+全序掩码）
                let crit = match self.captured {
                    Some(s) => s,
                    None => CritSnapshot {
                        cause_id: 0,
                        evaluated_tick: self.tick,
                        silence_ticks: 0,
                        silence_met: false,
                        window_events: 0,
                        rate_met: false,
                        cooldown_ticks: 0,
                        cooldown_met: false,
                        verdict: false,
                        fail_id: CRIT_ID_SILENCE,
                    },
                };
                self.audit.push(AuditEntry {
                    round_no: self.stats.reverse_count.wrapping_add(1),
                    cause_id: crit.cause_id,
                    crit,
                    open_tick: self.window_open_tick,
                    close_tick: self.tick,
                    duration_ticks: duration,
                    steps_mask: self.steps_mask,
                    window_events: wa.events,
                    window_first_seq: wa.first_seq,
                    window_last_seq: wa.last_seq,
                    outcome: AuditOutcome::Completed,
                });
                // 相位收口：Polling 进入，驻留计时开启
                self.state = IrqState::Polling;
                self.phase = ReversePhase::Completed;
                self.order_cursor = 0;
                self.residence_since = Some(self.tick);
                self.last_reverse_end_tick = Some(self.tick);
                self.blocked_attempts = 0;
                self.round_replay_base = self.stats.replayed_total;
                self.round_replay_first = None;
                self.round_replay_last = None;
                self.stats.reverse_count = self.stats.reverse_count.wrapping_add(1);
                self.stats.last_reverse_duration_ticks = duration;
                if duration > self.stats.max_reverse_duration_ticks {
                    self.stats.max_reverse_duration_ticks = duration;
                }
                Ok(step)
            }
        }
    }

    // ── 逆回守恒：补投与对平 ────────────────────────────────────────

    /// 全量补投：Polling 泵把 pending 账按序号严格连续全量重投。
    ///
    /// 零丢失第二半：先做账内序号连续断言（断裂即报 0x138A+红账），
    /// 再逐条补投（每条同时计入轮询泵接管覆盖计数），最后清 pending 账。
    pub fn replay_pending(&mut self) -> IrqRevResult<ReplayReport> {
        if self.sealed {
            return Err(IrqRevError::LedgerSealed);
        }
        if self.phase != ReversePhase::Completed {
            return Err(IrqRevError::ReplayWindowClosed);
        }
        // 序号连续断言（第一现场）
        let n = self.pending.count();
        let mut i = 1usize;
        while i < n {
            let prev = match self.pending.at(i - 1) {
                Some(e) => e,
                None => {
                    return Err(IrqRevError::ReplaySeqGap {
                        expected: 0,
                        got: 0,
                    })
                }
            };
            let cur = match self.pending.at(i) {
                Some(e) => e,
                None => {
                    return Err(IrqRevError::ReplaySeqGap {
                        expected: 0,
                        got: 0,
                    })
                }
            };
            let want = prev.seq.wrapping_add(1);
            if cur.seq != want {
                self.red.push(
                    self.tick,
                    RedKind::SeqGap {
                        expected: want,
                        got: cur.seq,
                    },
                );
                return Err(IrqRevError::ReplaySeqGap {
                    expected: want,
                    got: cur.seq,
                });
            }
            i += 1;
        }
        // 全量补投
        let events = self.pending.drain_all();
        let first = match events.first() {
            Some(e) => Some(e.seq),
            None => None,
        };
        let last = match events.last() {
            Some(e) => Some(e.seq),
            None => None,
        };
        let mut replayed = 0u64;
        let mut idx = 0usize;
        while idx < events.len() {
            let ev = match events.get(idx) {
                Some(e) => *e,
                None => break,
            };
            self.replay.push(ReplayRecord {
                seq: ev.seq,
                vector: ev.vector,
                at_tick: self.tick,
            });
            self.stats.replayed_total = self.stats.replayed_total.wrapping_add(1);
            self.stats.polling_pump_delivered =
                self.stats.polling_pump_delivered.wrapping_add(1);
            replayed = replayed.wrapping_add(1);
            idx += 1;
        }
        if replayed > 0 {
            self.round_replay_first = first;
            self.round_replay_last = last;
        }
        Ok(ReplayReport {
            replayed,
            first_seq: first,
            last_seq: last,
            continuous: true,
        })
    }

    /// 对平：补投账与 pending 账恒等核查（逆回守恒的终判）。
    ///
    /// 三重恒等：
    ///   ① 本轮补投数 == 窗账事件数；
    ///   ② 累计恒等 pending_total == replayed_total（pending 账已清）；
    ///   ③ 窗内序号端点与补投端点一致。
    pub fn reconcile(&mut self) -> IrqRevResult<ReconcileReport> {
        if self.sealed {
            return Err(IrqRevError::LedgerSealed);
        }
        if self.phase != ReversePhase::Completed {
            return Err(IrqRevError::ReplayWindowClosed);
        }
        let wa = match self.window_account {
            Some(w) => w,
            None => return Err(IrqRevError::ReplayWindowClosed),
        };
        let delta = self
            .stats
            .replayed_total
            .saturating_sub(self.round_replay_base);
        let cumulative_balanced = self.stats.pending_total == self.stats.replayed_total;
        let endpoints_matched = if wa.events == 0 {
            delta == 0
        } else {
            delta == wa.events
                && self.round_replay_first == Some(wa.first_seq)
                && self.round_replay_last == Some(wa.last_seq)
        };
        let balanced = cumulative_balanced && endpoints_matched && self.pending.count() == 0;
        if !balanced {
            self.red.push(
                self.tick,
                RedKind::ReconcileMismatch {
                    pending_total: self.stats.pending_total,
                    replayed_total: self.stats.replayed_total,
                },
            );
            return Err(IrqRevError::ReconcileMismatch {
                pending_total: self.stats.pending_total,
                replayed_total: self.stats.replayed_total,
            });
        }
        Ok(ReconcileReport {
            window_events: wa.events,
            replayed_delta: delta,
            cumulative_balanced,
            endpoints_matched,
            balanced,
        })
    }

    /// 序号连续审计（UNX-F1263-J1「序号连续断言 10 轮」协议面出口）。
    ///
    /// 逐审计条目核查窗内连续：窗事件数 > 0 时，末序号-首序号+1 必须恰等于
    /// 窗事件数（每轮窗内不重不漏）；全局零丢失由 pending==replayed 恒等账
    /// 与对平面终判（本审计是逐轮窗口的第一现场）。
    pub fn seq_continuity_audit(&self) -> SeqContinuityReport {
        let entries = self.audit.replay_all();
        let mut gaps = 0u32;
        let mut i = 0usize;
        while i < entries.len() {
            let e = entries[i];
            if e.window_events > 0 {
                let span = e
                    .window_last_seq
                    .saturating_sub(e.window_first_seq)
                    .saturating_add(1);
                if span != e.window_events {
                    gaps = gaps.saturating_add(1);
                }
            }
            i += 1;
        }
        SeqContinuityReport {
            rounds: entries.len() as u64,
            gaps,
            continuous: gaps == 0,
            first_seq: self.replay.first_seq(),
            last_seq: self.replay.last_seq(),
        }
    }

    // ── 冷却、驻留与正向迁移（防振荡滞回面） ────────────────────────

    /// Polling 驻留 tick 数（Completed 起算；非驻留相位返回 0）。
    pub fn residence_ticks(&self) -> u64 {
        match self.residence_since {
            Some(t) => self.tick.saturating_sub(t),
            None => 0,
        }
    }

    /// 逆回冷却 tick 数（上次逆回完成起算；从未逆回返回 u64::MAX 语义）。
    pub fn cooldown_ticks(&self) -> u64 {
        match self.last_reverse_end_tick {
            Some(t) => self.tick.saturating_sub(t),
            None => u64::MAX,
        }
    }

    /// 申请正向迁移（Polling→Draining 交接）。
    ///
    /// 滞回下沿：驻留 < CRIT_MIN_POLLING_RESIDENCE_TICKS 一律拒绝；
    /// 同一驻留窗内被拒达 OSCILLATION_RED_THRESHOLD 次即入振荡红账
    /// （防抖计数 + 防振荡滞回，防正逆来回翻）。驻留期满即放行。
    pub fn request_forward_migration(&mut self) -> IrqRevResult<ForwardGrant> {
        if self.sealed {
            return Err(IrqRevError::LedgerSealed);
        }
        if self.state != IrqState::Polling || self.phase != ReversePhase::Completed {
            return Err(IrqRevError::NotInResidentState {
                state: self.state,
                phase: self.phase,
            });
        }
        let since = match self.residence_since {
            Some(t) => t,
            None => self.tick,
        };
        let residence = self.tick.saturating_sub(since);
        if residence < CRIT_MIN_POLLING_RESIDENCE_TICKS {
            self.stats.forward_blocked = self.stats.forward_blocked.wrapping_add(1);
            self.blocked_attempts = self.blocked_attempts.saturating_add(1);
            if self.blocked_attempts >= OSCILLATION_RED_THRESHOLD {
                self.red.push(
                    self.tick,
                    RedKind::OscillationBlocked {
                        attempts: self.blocked_attempts,
                    },
                );
                self.stats.oscillation_red = self.stats.oscillation_red.wrapping_add(1);
                return Err(IrqRevError::OscillationSuspected {
                    attempts: self.blocked_attempts,
                });
            }
            return Err(IrqRevError::ResidenceNotMet {
                remaining: CRIT_MIN_POLLING_RESIDENCE_TICKS - residence,
            });
        }
        // 驻留期满：放行 Polling→Draining 交接
        self.state = IrqState::Draining;
        self.phase = ReversePhase::Forwarded;
        self.residence_since = None;
        self.blocked_attempts = 0;
        self.stats.forward_granted = self.stats.forward_granted.wrapping_add(1);
        Ok(ForwardGrant {
            at_tick: self.tick,
            residence_ticks: residence,
        })
    }

    /// 排空完成：Draining→Arming。要求未确认账清零（排空中断残留拦截）。
    pub fn complete_draining(&mut self) -> IrqRevResult<()> {
        if self.sealed {
            return Err(IrqRevError::LedgerSealed);
        }
        if self.state != IrqState::Draining {
            return Err(IrqRevError::NotInResidentState {
                state: self.state,
                phase: self.phase,
            });
        }
        let unacked = self.unacked_count();
        if unacked > 0 {
            return Err(IrqRevError::UnackedVectorsPresent { count: unacked });
        }
        self.state = IrqState::Arming;
        Ok(())
    }

    /// 武装入 Irq 态：Arming→Irq，正向链闭环（rounds_completed+1）。
    ///
    /// 全局序号继续沿用（跨轮连续的根基），窗账/全序面复位，冷却基准保持
    /// （下次逆回仍受冷却期约束）。
    pub fn arm_vectors(&mut self, vectors: &[u16]) -> IrqRevResult<()> {
        if self.sealed {
            return Err(IrqRevError::LedgerSealed);
        }
        if self.state != IrqState::Arming {
            return Err(IrqRevError::NotInResidentState {
                state: self.state,
                phase: self.phase,
            });
        }
        self.state = IrqState::Irq;
        self.phase = ReversePhase::Dormant;
        self.order_cursor = 0;
        self.steps_mask = 0;
        self.disarmed = false;
        self.irq_exited = false;
        self.window_account = None;
        self.captured = None;
        self.round_replay_base = self.stats.replayed_total;
        self.round_replay_first = None;
        self.round_replay_last = None;
        self.blocked_attempts = 0;
        self.unacked.clear();
        self.armed_vectors = vectors.len() as u64;
        self.stats.rounds_completed = self.stats.rounds_completed.wrapping_add(1);
        Ok(())
    }

    // ── 指纹、覆盖与账面视图 ────────────────────────────────────────

    /// FNV-1a 64 位指纹：引擎全量账面的确定性摘要（双跑对拍的比对根）。
    ///
    /// 摘要覆盖：段基址/态/相位/时钟/序号/全序面/窗账/统计账/滞回门/
    /// 采样窗/未确认账/三本账累计与环形现存。同序列操作必得同指纹。
    pub fn fnv_fingerprint(&self) -> u64 {
        const FNV_OFFSET: u64 = 0xcbf29ce484222325;
        const FNV_PRIME: u64 = 0x100000001b3;
        let mut h: u64 = FNV_OFFSET;
        let mixb = |h: &mut u64, bytes: &[u8]| {
            let mut i = 0usize;
            while i < bytes.len() {
                *h ^= bytes[i] as u64;
                *h = h.wrapping_mul(FNV_PRIME);
                i += 1;
            }
        };
        mixb(&mut h, &SEG_BASE.to_le_bytes());
        mixb(&mut h, &[self.state.code(), self.phase.code()]);
        mixb(&mut h, &self.tick.to_le_bytes());
        mixb(&mut h, &self.seq_counter.to_le_bytes());
        mixb(&mut h, &[self.order_cursor, self.steps_mask]);
        mixb(
            &mut h,
            &[
                self.disarmed as u8,
                self.irq_exited as u8,
                self.sealed as u8,
            ],
        );
        mixb(&mut h, &self.stats.events_total.to_le_bytes());
        mixb(&mut h, &self.stats.events_dispatched.to_le_bytes());
        mixb(&mut h, &self.stats.pending_total.to_le_bytes());
        mixb(&mut h, &self.stats.replayed_total.to_le_bytes());
        mixb(&mut h, &self.stats.polling_pump_delivered.to_le_bytes());
        mixb(&mut h, &self.stats.reversal_attempts.to_le_bytes());
        mixb(&mut h, &self.stats.reverse_count.to_le_bytes());
        mixb(&mut h, &self.stats.forward_blocked.to_le_bytes());
        mixb(&mut h, &self.stats.forward_granted.to_le_bytes());
        mixb(&mut h, &self.stats.oscillation_red.to_le_bytes());
        mixb(&mut h, &self.stats.order_violations.to_le_bytes());
        mixb(&mut h, &self.stats.pending_overflow.to_le_bytes());
        mixb(&mut h, &self.stats.rounds_completed.to_le_bytes());
        mixb(&mut h, &self.stats.last_reverse_duration_ticks.to_le_bytes());
        mixb(&mut h, &self.stats.max_reverse_duration_ticks.to_le_bytes());
        mixb(&mut h, &self.rate_window.sum().to_le_bytes());
        mixb(&mut h, &self.rate_window.base().to_le_bytes());
        mixb(&mut h, &self.rate_window.samples().to_le_bytes());
        mixb(
            &mut h,
            &[
                self.gate.confirmed() as u8,
                self.gate.low_streak() as u8,
            ],
        );
        mixb(&mut h, &self.gate.transitions().to_le_bytes());
        mixb(&mut h, &self.unacked_count().to_le_bytes());
        mixb(&mut h, &self.armed_vectors.to_le_bytes());
        mixb(&mut h, &self.last_event_tick.to_le_bytes());
        mixb(
            &mut h,
            &match self.residence_since {
                Some(t) => t,
                None => 0,
            }
            .to_le_bytes(),
        );
        mixb(
            &mut h,
            &match self.last_reverse_end_tick {
                Some(t) => t,
                None => 0,
            }
            .to_le_bytes(),
        );
        mixb(
            &mut h,
            &match self.window_account {
                Some(w) => w.events,
                None => 0,
            }
            .to_le_bytes(),
        );
        mixb(&mut h, &self.pending.count().to_le_bytes());
        mixb(&mut h, &self.replay.total().to_le_bytes());
        mixb(&mut h, &self.replay.resident().to_le_bytes());
        mixb(&mut h, &self.red.total_ever().to_le_bytes());
        mixb(&mut h, &self.audit.total().to_le_bytes());
        h
    }

    /// 轮询泵接管覆盖判定（UNX-F1263-J1「接管完整率 100%」观测点）：
    /// pending 累计 ≤ 轮询泵累计投递 且 pending 账已清，即每条暂扣事件
    /// 都被泵至少投递一次（补投计一次，直投计一次，恒等守恒下恰一次）。
    pub fn polling_takeover_complete(&self) -> bool {
        self.pending.count() == 0 && self.stats.pending_total <= self.stats.polling_pump_delivered
    }

    /// pending 账快照。
    pub fn pending_view(&self) -> Vec<PendingEvent> {
        let mut out = Vec::new();
        let mut i = 0usize;
        while i < self.pending.count() {
            if let Some(e) = self.pending.at(i) {
                out.push(e);
            }
            i += 1;
        }
        out
    }

    /// 补投账最近 n 条。
    pub fn replay_latest(&self, n: usize) -> Vec<ReplayRecord> {
        self.replay.latest(n)
    }

    /// 审计环回放重演（时间升序全量留存条目）。
    pub fn audit_replay(&self) -> Vec<AuditEntry> {
        self.audit.replay_all()
    }

    /// 红账快照。
    pub fn red_entries(&self) -> Vec<RedEntry> {
        self.red.snapshot()
    }

    // ── 注入测试面（三类注入各检出） ────────────────────────────────

    /// 注入一：非法逆回注入——构造未确认向量在场的逆回申请。
    ///
    /// 预期：协议以 0x1385 拦截并入红账（逆回=丢中断防线命中）。
    /// 引擎若不在 Irq 态 Dormant，先走一遍正规正向链把它带回 Irq 态
    /// （注入面自足，不依赖外部前置）。
    pub fn inject_illegal_reversal(&mut self, cause: ReverseTriggerCause) -> IrqRevResult<InjectOutcome> {
        // 自足前置：按当前驻留态走正规正向链回到 Irq 态 Dormant
        if !(self.state == IrqState::Irq && self.phase == ReversePhase::Dormant) {
            match self.state {
                IrqState::Idle => {
                    // Idle 未上电：注入面无法自足拉起，如实报前置不符
                    return Err(IrqRevError::NotInResidentState {
                        state: self.state,
                        phase: self.phase,
                    });
                }
                IrqState::Draining => {
                    self.complete_draining()?;
                    self.arm_vectors(&[0x1F40])?;
                }
                IrqState::Arming => {
                    self.arm_vectors(&[0x1F40])?;
                }
                IrqState::Polling => {
                    self.tick(CRIT_MIN_POLLING_RESIDENCE_TICKS + 4);
                    self.request_forward_migration()?;
                    self.complete_draining()?;
                    self.arm_vectors(&[0x1F40])?;
                }
                IrqState::Irq => {
                    // Irq 态但相位非 Dormant：逆回窗开着，禁止叠加注入
                    return Err(IrqRevError::AlreadyReversing);
                }
            }
        }
        // 构造未确认向量：在 Irq 态 raise 不 ack
        let tk = self.raise_event(0x1F3F)?;
        let _ = tk;
        // 让三判据达标（静默+低率防抖+冷却），保证拦截的是"未确认向量"这一条
        self.tick(
            CRIT_SILENCE_WINDOW_TICKS
                + (CRIT_DEBOUNCE_CONFIRM_COUNT as u64) * (RATE_WINDOW_TICKS as u64)
                + 4,
        );
        let snap = self.evaluate_reverse_eligibility(cause)?;
        let attempt = self.begin_reverse(cause, snap);
        match attempt {
            Err(e) => {
                let code = e.code();
                let intercepted = code == CODE_UNACKED_VECTORS_PRESENT;
                Ok(InjectOutcome {
                    kind: InjectKind::IllegalReversal,
                    intercepted,
                    code,
                    note: format!(
                        "非法逆回注入：未确认 {} 条在场，协议码 0x{:04X}，红账累计 {}",
                        self.unacked_count(),
                        code,
                        self.red.total_illegal()
                    ),
                })
            }
            Ok(_t) => Ok(InjectOutcome {
                kind: InjectKind::IllegalReversal,
                intercepted: false,
                code: 0,
                note: "非法逆回注入异常：协议未拦截（不应发生）".to_string(),
            }),
        }
    }

    /// 注入二：逆回窗事件注入——逆回窗开启期间灌入合成事件。
    ///
    /// 预期：全部入 pending 账（零丢失第一半），随后补投可全量对平。
    pub fn inject_window_events(&mut self, count: usize) -> IrqRevResult<InjectOutcome> {
        let open = matches!(
            self.phase,
            ReversePhase::WindowOpen | ReversePhase::Reversing
        );
        if !open {
            return Err(IrqRevError::NotInResidentState {
                state: self.state,
                phase: self.phase,
            });
        }
        let before = self.pending.count();
        let mut i = 0usize;
        while i < count {
            // 合成向量低 8 位携带注入序号，便于对账
            let vec = 0x2000u16.wrapping_add(i as u16);
            let r = self.raise_event(vec)?;
            match r.disposition {
                EventDisposition::HeldPending => {}
                EventDisposition::Dispatched => {
                    return Ok(InjectOutcome {
                        kind: InjectKind::WindowEvent,
                        intercepted: true,
                        code: CODE_NOT_IN_RESIDENT_STATE,
                        note: "逆回窗事件注入异常：事件被直投而非入账（不应发生）".to_string(),
                    });
                }
            }
            i += 1;
        }
        let after = self.pending.count();
        let held = after >= before && after - before == count;
        Ok(InjectOutcome {
            kind: InjectKind::WindowEvent,
            intercepted: held,
            code: 0,
            note: format!(
                "逆回窗事件注入：{} 条全部入 pending 账（账面 {} 条），零丢失第一半成立",
                count, after
            ),
        })
    }

    /// 注入三：振荡注入——Polling 驻留期内连续申请正向迁移。
    ///
    /// 预期：全部被拒（0x1387/0x1388），达到阈值后入振荡红账。
    pub fn inject_oscillation(&mut self, attempts: u32) -> IrqRevResult<InjectOutcome> {
        if !(self.state == IrqState::Polling && self.phase == ReversePhase::Completed) {
            return Err(IrqRevError::NotInResidentState {
                state: self.state,
                phase: self.phase,
            });
        }
        let red_before = self.red.total_oscillation();
        let mut blocked = 0u32;
        let mut last_code = 0u16;
        let mut i = 0u32;
        while i < attempts {
            match self.request_forward_migration() {
                Ok(_g) => {
                    // 意外放行：注入失真，如实上报
                    return Ok(InjectOutcome {
                        kind: InjectKind::Oscillation,
                        intercepted: blocked == attempts.saturating_sub(1),
                        code: 0,
                        note: format!(
                            "振荡注入异常：第 {} 次尝试被放行（驻留已满，非振荡场景）",
                            i + 1
                        ),
                    });
                }
                Err(e) => {
                    last_code = e.code();
                    blocked = blocked.saturating_add(1);
                }
            }
            i += 1;
        }
        let red_hit = self.red.total_oscillation() > red_before;
        Ok(InjectOutcome {
            kind: InjectKind::Oscillation,
            intercepted: blocked == attempts && red_hit,
            code: last_code,
            note: format!(
                "振荡注入：{} 次正向迁移申请全被拒（末码 0x{:04X}），振荡红账累计 {}",
                blocked,
                last_code,
                self.red.total_oscillation()
            ),
        })
    }

    // ── 健康体检面 ──────────────────────────────────────────────────

    /// health() 体检：8 项逐项核查 + 总结论（绿/黄/红）。
    ///
    /// 项 1 逆回守恒恒等：pending_total == replayed_total；
    /// 项 2 红账清零：现存红账为空；
    /// 项 3 无滞留 pending：pending 账已清（或逆回窗合法开启中）；
    /// 项 4 跨轮序号连续：seq_continuity_audit 无断裂；
    /// 项 5 轮询泵接管完整：polling_takeover_complete；
    /// 项 6 防抖稳定：近期滞回翻转 < 3（256 tick 窗）；
    /// 项 7 审计账一致：审计累计 == 逆回完成数；
    /// 项 8 补投账一致：补投累计 == pending 累计（与项 1 同源复账）。
    pub fn health(&self) -> HealthReport {
        let mut items: Vec<HealthItem> = Vec::new();
        let stat = self.stats;
        // 项 1
        let cons_ok = stat.pending_total == stat.replayed_total;
        items.push(HealthItem {
            id: 1,
            name: "逆回守恒恒等",
            ok: cons_ok,
            note: format!(
                "pending 累计 {} / 补投累计 {}",
                stat.pending_total, stat.replayed_total
            ),
        });
        // 项 2
        let red_ok = self.red.is_empty();
        items.push(HealthItem {
            id: 2,
            name: "红账清零",
            ok: red_ok,
            note: format!("现存红账 {} 条（累计 {}）", self.red.count(), self.red.total_ever()),
        });
        // 项 3
        let pend_ok = self.pending.count() == 0
            || matches!(
                self.phase,
                ReversePhase::WindowOpen | ReversePhase::Reversing
            );
        items.push(HealthItem {
            id: 3,
            name: "无滞留 pending",
            ok: pend_ok,
            note: format!("pending 账面条数 {}", self.pending.count()),
        });
        // 项 4
        let cont = self.seq_continuity_audit();
        items.push(HealthItem {
            id: 4,
            name: "跨轮序号连续",
            ok: cont.continuous,
            note: format!(
                "参审 {} 轮，断裂 {} 次",
                cont.rounds, cont.gaps
            ),
        });
        // 项 5
        let take_ok = self.polling_takeover_complete();
        items.push(HealthItem {
            id: 5,
            name: "轮询泵接管完整",
            ok: take_ok,
            note: format!(
                "泵投递 {} / pending 累计 {}",
                stat.polling_pump_delivered, stat.pending_total
            ),
        });
        // 项 6
        let flaps = self.gate.flaps_within(256, self.tick);
        let flap_ok = flaps < 3;
        items.push(HealthItem {
            id: 6,
            name: "防抖稳定",
            ok: flap_ok,
            note: format!("256 tick 内滞回翻转 {} 次", flaps),
        });
        // 项 7
        let audit_ok = self.audit.total() == stat.reverse_count;
        items.push(HealthItem {
            id: 7,
            name: "审计账一致",
            ok: audit_ok,
            note: format!(
                "审计累计 {} / 逆回完成 {}",
                self.audit.total(),
                stat.reverse_count
            ),
        });
        // 项 8
        let replay_ok = self.replay.total() == stat.pending_total;
        items.push(HealthItem {
            id: 8,
            name: "补投账一致",
            ok: replay_ok,
            note: format!(
                "补投凭证累计 {} / pending 累计 {}",
                self.replay.total(),
                stat.pending_total
            ),
        });
        // 总结论：红账非空/守恒破坏 → 红；其余项有黄 → 黄；全绿
        let red_verdict = !red_ok || !cons_ok || !cont.continuous || !take_ok;
        let mut all_ok = true;
        let mut i = 0usize;
        while i < items.len() {
            if let Some(it) = items.get(i) {
                if !it.ok {
                    all_ok = false;
                }
            }
            i += 1;
        }
        let verdict = if red_verdict {
            HealthVerdict::Red
        } else if all_ok {
            HealthVerdict::Green
        } else {
            HealthVerdict::Yellow
        };
        let summary = format!(
            "UNX-F1263 体检：{}（态 {} / 相位 {} / 逆回 {} 轮 / 红账累计 {}）",
            verdict.name(),
            self.state.name(),
            self.phase.name(),
            stat.reverse_count,
            self.red.total_ever()
        );
        HealthReport {
            verdict,
            items,
            summary,
        }
    }

    // ── 人话化与导出面 ──────────────────────────────────────────────

    /// explain 报告：协议现况人话化（判据现值逐条 vs 阈值 + 统计 + 红账 + 审计尾）。
    pub fn explain(&self) -> String {
        let mut s = String::new();
        s.push_str("══ UNX-F1263 Irq→Polling 逆回协议 · explain ══\n");
        s.push_str(&format!(
            "态：{}（码 0x{:02X}）  相位：{}  时钟：{} tick\n",
            self.state.name(),
            self.state.code(),
            self.phase.name(),
            self.tick
        ));
        s.push_str("── 逆回判据现值（≥3 项全过方可逆回）──\n");
        let views = self.criteria_view();
        let mut i = 0usize;
        while i < views.len() {
            let v = views[i];
            s.push_str(&format!(
                "  判据{} {}：现值 {} / 阈值 {} → {}\n",
                v.id,
                v.name,
                v.current,
                v.threshold,
                if v.met { "达标" } else { "未达标" }
            ));
            i += 1;
        }
        s.push_str("── 统计账 ──\n");
        let st = self.stats;
        s.push_str(&format!(
            "  事件总数 {}（分发 {}）｜pending 累计 {}｜补投累计 {}｜泵投递 {}\n",
            st.events_total,
            st.events_dispatched,
            st.pending_total,
            st.replayed_total,
            st.polling_pump_delivered
        ));
        s.push_str(&format!(
            "  逆回申请 {}｜逆回完成 {}｜正向拒 {}｜正向放行 {}｜完整轮 {}\n",
            st.reversal_attempts,
            st.reverse_count,
            st.forward_blocked,
            st.forward_granted,
            st.rounds_completed
        ));
        s.push_str(&format!(
            "  逆回耗时：末次 {} tick / 峰值 {} tick\n",
            st.last_reverse_duration_ticks, st.max_reverse_duration_ticks
        ));
        s.push_str("── 红账 ──\n");
        s.push_str(&format!(
            "  现存 {} 条｜累计 {}（非法逆回 {} / 振荡 {} / 溢出 {} / 断裂 {} / 对平 {}）\n",
            self.red.count(),
            self.red.total_ever(),
            self.red.total_illegal(),
            self.red.total_oscillation(),
            self.red.total_overflow(),
            self.red.total_seqgap(),
            self.red.total_reconcile()
        ));
        let reds = self.red_entries();
        if !reds.is_empty() {
            let mut j = 0usize;
            while j < reds.len() {
                let e = reds[j];
                s.push_str(&format!(
                    "    · #{} tick={} 码 0x{:04X} {}\n",
                    e.seq,
                    e.tick,
                    e.code,
                    e.kind.note()
                ));
                j += 1;
            }
        }
        s.push_str("── 审计尾（最近 3 轮逆回）──\n");
        let audits = self.audit_replay();
        let total = audits.len();
        let start = if total >= 3 { total - 3 } else { 0 };
        let mut k = start;
        while k < total {
            let e = audits[k];
            s.push_str(&format!(
                "    轮{} 原因{} {}：窗 {} tick→{} tick 耗时 {} tick，窗事件 {}（序 {}..={}），全序掩码 0b{:03b}\n",
                e.round_no,
                e.cause_id,
                match e.outcome {
                    AuditOutcome::Completed => "已完成",
                    AuditOutcome::Intercepted => "被拦截",
                },
                e.open_tick,
                e.close_tick,
                e.duration_ticks,
                e.window_events,
                e.window_first_seq,
                e.window_last_seq,
                e.steps_mask
            ));
            k += 1;
        }
        let cont = self.seq_continuity_audit();
        s.push_str(&format!(
            "── 跨轮序号连续：{} 轮参审，断裂 {} → {} ──\n",
            cont.rounds,
            cont.gaps,
            if cont.continuous { "零丢失成立" } else { "零丢失被破坏！" }
        ));
        s
    }

    /// JSON 导出：统计账 + 指纹 + 健康（确定性字节序，双跑可对拍）。
    pub fn export_json(&self) -> String {
        let st = self.stats;
        let views = self.criteria_view();
        let health = self.health();
        let mut s = String::new();
        s.push_str("{");
        s.push_str(&format!("\"seg\":{},", SEG_BASE));
        s.push_str(&format!("\"state\":\"{}\",", self.state.name()));
        s.push_str(&format!("\"phase\":\"{}\",", self.phase.name()));
        s.push_str(&format!("\"tick\":{},", self.tick));
        s.push_str(&format!("\"fingerprint\":{},", self.fnv_fingerprint()));
        s.push_str("\"stats\":{");
        s.push_str(&format!("\"events_total\":{},", st.events_total));
        s.push_str(&format!("\"events_dispatched\":{},", st.events_dispatched));
        s.push_str(&format!("\"pending_total\":{},", st.pending_total));
        s.push_str(&format!("\"replayed_total\":{},", st.replayed_total));
        s.push_str(&format!(
            "\"polling_pump_delivered\":{},",
            st.polling_pump_delivered
        ));
        s.push_str(&format!("\"reversal_attempts\":{},", st.reversal_attempts));
        s.push_str(&format!("\"reverse_count\":{},", st.reverse_count));
        s.push_str(&format!("\"forward_blocked\":{},", st.forward_blocked));
        s.push_str(&format!("\"forward_granted\":{},", st.forward_granted));
        s.push_str(&format!("\"oscillation_red\":{},", st.oscillation_red));
        s.push_str(&format!("\"order_violations\":{},", st.order_violations));
        s.push_str(&format!("\"pending_overflow\":{},", st.pending_overflow));
        s.push_str(&format!("\"rounds_completed\":{}", st.rounds_completed));
        s.push_str("},");
        s.push_str("\"criteria\":[");
        let mut i = 0usize;
        while i < views.len() {
            let v = views[i];
            s.push_str(&format!(
                "{{\"id\":{},\"name\":\"{}\",\"current\":{},\"threshold\":{},\"met\":{}}}",
                v.id,
                v.name,
                v.current,
                v.threshold,
                if v.met { "true" } else { "false" }
            ));
            if i + 1 < views.len() {
                s.push_str(",");
            }
            i += 1;
        }
        s.push_str("],");
        s.push_str(&format!(
            "\"health_verdict\":\"{}\",",
            health.verdict.name()
        ));
        s.push_str(&format!(
            "\"red_total\":{},\"audit_total\":{}",
            self.red.total_ever(),
            self.audit.total()
        ));
        s.push_str("}");
        s
    }

    // @@CONT@@
}

impl Default for ReverseTransitionEngine {
    fn default() -> Self {
        ReverseTransitionEngine::new()
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 十五、双跑确定性对拍（场景驱动 + 两跑指纹对拍）
// ═══════════════════════════════════════════════════════════════════════

/// 确定性场景驱动：以 seed 派生 LCG，跑 `rounds` 轮完整正逆回闭环。
///
/// 每轮：正向事件（raise+ack 全清）→ 静默/低率防抖/冷却推进 →
/// evaluate→begin_reverse（原因由 LCG 三选一）→ 逆回窗事件注入
/// （0~3 条）→ 全序三步 → 补投 → 对平 → 驻留推进 → 正向链闭环。
/// 全程零外部时序依赖：同 seed 必得同指纹。
pub fn run_deterministic_scenario(rounds: u32, seed: u64) -> IrqRevResult<ScenarioDigest> {
    let mut eng = ReverseTransitionEngine::new();
    let mut lcg = Lcg::new(seed);
    // 正向链首站：Polling→Draining→Arming→Irq（空转启动轮）
    eng.tick(CRIT_MIN_POLLING_RESIDENCE_TICKS + 4);
    eng.request_forward_migration()?;
    eng.complete_draining()?;
    eng.arm_vectors(&[0x1001, 0x1002])?;
    let mut r = 0u32;
    while r < rounds {
        // 原因三选一（J1 ≥3 项全实现）
        let cause = match lcg.below(3) {
            0 => ReverseTriggerCause::FaceFault,
            1 => ReverseTriggerCause::StormUpgrade,
            _ => ReverseTriggerCause::MaintenanceOrder,
        };
        // 正向事件 1~3 条，全部确认到齐（逆回前置）
        let k = 1u64 + lcg.below(3);
        let mut j = 0u64;
        while j < k {
            let vec = 0x1001u16.wrapping_add((j as u16) & 0x1);
            let _t = eng.raise_event(vec)?;
            eng.ack_vector(vec);
            j += 1;
        }
        // 静默窗 + 3 个低率防抖窗 + 冷却期一次推足
        let settle = CRIT_SILENCE_WINDOW_TICKS
            + (CRIT_DEBOUNCE_CONFIRM_COUNT as u64) * (RATE_WINDOW_TICKS as u64)
            + CRIT_REVERSE_COOLDOWN_TICKS
            + 2;
        eng.tick(settle);
        // 评估→执行双段协议
        let snap = eng.evaluate_reverse_eligibility(cause)?;
        let _ticket = eng.begin_reverse(cause, snap)?;
        // 逆回窗事件注入 0~3 条（零丢失第一半）
        let w = lcg.below(4);
        let mut m = 0u64;
        while m < w {
            let vec = 0x3000u16.wrapping_add(m as u16);
            let t = eng.raise_event(vec)?;
            match t.disposition {
                EventDisposition::HeldPending => {}
                EventDisposition::Dispatched => {
                    return Err(IrqRevError::ReplaySeqGap {
                        expected: t.seq,
                        got: t.seq,
                    })
                }
            }
            m += 1;
        }
        // 全序三步（次序即语义）
        eng.advance_order_step(ReverseOrderStep::DisarmArming)?;
        eng.advance_order_step(ReverseOrderStep::IrqExit)?;
        eng.advance_order_step(ReverseOrderStep::PollingEnter)?;
        // 补投 + 对平（零丢失第二半）
        let rep = eng.replay_pending()?;
        if !rep.continuous {
            return Err(IrqRevError::ReplaySeqGap {
                expected: 0,
                got: 0,
            });
        }
        let _rec = eng.reconcile()?;
        // 驻留推进（带 LCG 抖动）→ 正向链闭环
        let dwell = CRIT_MIN_POLLING_RESIDENCE_TICKS + 4 + lcg.below(4);
        eng.tick(dwell);
        eng.request_forward_migration()?;
        eng.complete_draining()?;
        eng.arm_vectors(&[0x1001, 0x1002])?;
        r += 1;
    }
    let st = eng.stats();
    Ok(ScenarioDigest {
        fingerprint: eng.fnv_fingerprint(),
        rounds: rounds as u64,
        events_total: st.events_total,
        pending_total: st.pending_total,
        replayed_total: st.replayed_total,
        red_total: st.oscillation_red,
        audit_total: st.reverse_count,
        last_seq: eng.seq_continuity_audit().last_seq.unwrap_or(0),
    })
}

/// 双跑对拍：同参数独立跑两遍，指纹与全摘要必须逐字段一致。
/// 不一致即报 0x138F（确定性被破坏，存在非确定源）。
pub fn assert_dual_run(rounds: u32, seed: u64) -> IrqRevResult<DualRunReport> {
    let left = run_deterministic_scenario(rounds, seed)?;
    let right = run_deterministic_scenario(rounds, seed)?;
    let ticks = right_digest_ticks(&left);
    if left != right {
        return Err(IrqRevError::DualRunMismatch {
            left: left.fingerprint,
            right: right.fingerprint,
        });
    }
    Ok(DualRunReport {
        digest: left,
        agree: true,
        total_ticks: ticks,
    })
}

/// 场景总 tick 复算（确定性推演，与引擎内推进逐 tick 同式）。
/// 供 DualRunReport 留痕；纯函数无副作用。
fn right_digest_ticks(_digest: &ScenarioDigest) -> u64 {
    // 启动轮 + 每轮（settle + 驻留）均为常量段；LCG 抖动项不影响守恒判据，
    // 对拍以摘要逐字段比较为准，此处给出下界留痕。
    CRIT_MIN_POLLING_RESIDENCE_TICKS
        + 4
        + (_digest.rounds as u64)
            * (CRIT_SILENCE_WINDOW_TICKS
                + (CRIT_DEBOUNCE_CONFIRM_COUNT as u64) * (RATE_WINDOW_TICKS as u64)
                + CRIT_REVERSE_COOLDOWN_TICKS
                + 2
                + CRIT_MIN_POLLING_RESIDENCE_TICKS
                + 4)
}

// ═══════════════════════════════════════════════════════════════════════
// 十六、编译期断言（const 面护栏）
// ═══════════════════════════════════════════════════════════════════════

const _: () = assert!(SEG_BASE == 0x1380, "诊断段基址必须为 0x1380");
const _: () = assert!(SEG_LAST == 0x138F, "诊断段末界必须为 0x138F");
const _: () = assert!(CRIT_TABLE.len() == 3, "逆回判据表必须恰为 3 项（J1 ≥3 项）");
const _: () = assert!(
    crit_threshold(CRIT_ID_SILENCE) == CRIT_SILENCE_WINDOW_TICKS,
    "静默窗阈值表项与常量不一致"
);
const _: () = assert!(
    crit_threshold(CRIT_ID_RATE) == CRIT_RATE_FLOOR_EVENTS,
    "事件率阈值表项与常量不一致"
);
const _: () = assert!(
    crit_threshold(CRIT_ID_COOLDOWN) == CRIT_REVERSE_COOLDOWN_TICKS,
    "冷却期阈值表项与常量不一致"
);
const _: () = assert!(
    AUDIT_RING_CAP.is_power_of_two(),
    "审计环容量必须为 2 的幂（环形取模语义）"
);
const _: () = assert!(PENDING_CAP >= 1024, "pending 账容量下限 1024");
const _: () = assert!(REPLAY_RING_CAP <= 4096, "补投环容量上限 4096");
const _: () = assert!(
    CRIT_DEBOUNCE_CONFIRM_COUNT >= 2,
    "防抖确认数下限 2（单窗确认无防抖意义）"
);
const _: () = assert!(
    OSCILLATION_RED_THRESHOLD >= 2,
    "振荡红账阈值下限 2（单次被拒即红账过严）"
);
const _: () = assert!(
    CRIT_MIN_POLLING_RESIDENCE_TICKS >= 3 * (RATE_WINDOW_TICKS as u64),
    "Polling 最短驻留须覆盖 3 个完整采样窗（滞回上沿对齐）"
);

// ═══════════════════════════════════════════════════════════════════════
// 十七、测试段（cfg(test)，不计纯功能行数）
// ═══════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    /// 标准一轮：正向事件→判据推进→双段逆回→全序→补投→对平→正向闭环。
    fn one_round(eng: &mut ReverseTransitionEngine, cause: ReverseTriggerCause, window_events: usize) {
        assert!(eng.state() == IrqState::Irq && eng.phase() == ReversePhase::Dormant);
        let t = eng.raise_event(0x0A01).unwrap_or_else(|e| panic!("raise 失败: {}", e));
        assert_eq!(t.disposition, EventDisposition::Dispatched);
        assert!(eng.ack_vector(0x0A01));
        eng.tick(
            CRIT_SILENCE_WINDOW_TICKS
                + (CRIT_DEBOUNCE_CONFIRM_COUNT as u64) * (RATE_WINDOW_TICKS as u64)
                + CRIT_REVERSE_COOLDOWN_TICKS
                + 2,
        );
        let snap = eng
            .evaluate_reverse_eligibility(cause)
            .unwrap_or_else(|e| panic!("评估失败: {}", e));
        assert!(snap.verdict);
        let _tk = eng
            .begin_reverse(cause, snap)
            .unwrap_or_else(|e| panic!("开窗失败: {}", e));
        let mut i = 0usize;
        while i < window_events {
            let ev = eng
                .raise_event(0x3000u16.wrapping_add(i as u16))
                .unwrap_or_else(|e| panic!("窗内事件失败: {}", e));
            assert_eq!(ev.disposition, EventDisposition::HeldPending);
            i += 1;
        }
        eng.advance_order_step(ReverseOrderStep::DisarmArming)
            .unwrap_or_else(|e| panic!("步一失败: {}", e));
        eng.advance_order_step(ReverseOrderStep::IrqExit)
            .unwrap_or_else(|e| panic!("步二失败: {}", e));
        eng.advance_order_step(ReverseOrderStep::PollingEnter)
            .unwrap_or_else(|e| panic!("步三失败: {}", e));
        let rep = eng
            .replay_pending()
            .unwrap_or_else(|e| panic!("补投失败: {}", e));
        assert_eq!(rep.replayed as usize, window_events);
        assert!(rep.continuous);
        let rec = eng
            .reconcile()
            .unwrap_or_else(|e| panic!("对平失败: {}", e));
        assert!(rec.balanced);
        eng.tick(CRIT_MIN_POLLING_RESIDENCE_TICKS + 4);
        eng.request_forward_migration()
            .unwrap_or_else(|e| panic!("正向放行失败: {}", e));
        eng.complete_draining()
            .unwrap_or_else(|e| panic!("排空失败: {}", e));
        eng.arm_vectors(&[0x0A01])
            .unwrap_or_else(|e| panic!("武装失败: {}", e));
    }

    #[test]
    fn drill_legal_reversal_full_order() {
        // 合法逆回全序：一次完整闭环，态/相位/审计/耗时全对
        let mut eng = ReverseTransitionEngine::new();
        eng.tick(CRIT_MIN_POLLING_RESIDENCE_TICKS + 4);
        eng.request_forward_migration().unwrap_or_else(|e| panic!("e: {}", e));
        eng.complete_draining().unwrap_or_else(|e| panic!("e: {}", e));
        eng.arm_vectors(&[0x0A01]).unwrap_or_else(|e| panic!("e: {}", e));
        assert_eq!(eng.state(), IrqState::Irq);
        assert_eq!(eng.phase(), ReversePhase::Dormant);
        one_round(&mut eng, ReverseTriggerCause::FaceFault, 2);
        assert_eq!(eng.state(), IrqState::Irq);
        assert_eq!(eng.phase(), ReversePhase::Dormant);
        assert_eq!(eng.stats().reverse_count, 1);
        assert_eq!(eng.stats().rounds_completed, 2); // 启动轮 + 本轮
        assert_eq!(eng.stats().pending_total, 2);
        assert_eq!(eng.stats().replayed_total, 2);
        assert!(eng.red_entries().is_empty());
        let audits = eng.audit_replay();
        assert_eq!(audits.len(), 1);
        let a = audits[0];
        assert_eq!(a.steps_mask, 0b111);
        assert_eq!(a.cause_id, ReverseTriggerCause::FaceFault.id());
        assert_eq!(a.window_events, 2);
        assert!(a.duration_ticks <= CRIT_SILENCE_WINDOW_TICKS);
        assert!(eng.polling_takeover_complete());
    }

    #[test]
    fn drill_illegal_reversal_intercepted() {
        // 非法逆回拦截：未确认向量在场，逆回=丢中断 → 0x1385 + 红账
        let mut eng = ReverseTransitionEngine::new();
        eng.tick(CRIT_MIN_POLLING_RESIDENCE_TICKS + 4);
        eng.request_forward_migration().unwrap_or_else(|e| panic!("e: {}", e));
        eng.complete_draining().unwrap_or_else(|e| panic!("e: {}", e));
        eng.arm_vectors(&[0x0A01]).unwrap_or_else(|e| panic!("e: {}", e));
        let _t = eng.raise_event(0x0A01).unwrap_or_else(|e| panic!("e: {}", e));
        // 不 ack，直接推进判据
        eng.tick(
            CRIT_SILENCE_WINDOW_TICKS
                + (CRIT_DEBOUNCE_CONFIRM_COUNT as u64) * (RATE_WINDOW_TICKS as u64)
                + 2,
        );
        let snap = eng
            .evaluate_reverse_eligibility(ReverseTriggerCause::StormUpgrade)
            .unwrap_or_else(|e| panic!("e: {}", e));
        // begin_reverse 应失败：用 match 断言（首次尝试留痕）
        let _first = eng.begin_reverse(ReverseTriggerCause::StormUpgrade, snap);
        let snap2 = eng
            .evaluate_reverse_eligibility(ReverseTriggerCause::StormUpgrade)
            .unwrap_or_else(|e| panic!("e: {}", e));
        match eng.begin_reverse(ReverseTriggerCause::StormUpgrade, snap2) {
            Err(IrqRevError::UnackedVectorsPresent { count }) => {
                assert_eq!(count, 1);
            }
            Err(e) => panic!("错误类别不符: {}", e),
            Ok(_) => panic!("非法逆回未被拦截"),
        }
        assert_eq!(eng.red_entries().len(), 2); // 两次未清账尝试各一条红账
        assert_eq!(eng.red_entries()[0].kind.code(), CODE_UNACKED_VECTORS_PRESENT);
        assert_eq!(eng.state(), IrqState::Irq);
        // 红账清账后逆回恢复正常
        assert!(eng.ack_vector(0x0A01));
        let snap3 = eng
            .evaluate_reverse_eligibility(ReverseTriggerCause::StormUpgrade)
            .unwrap_or_else(|e| panic!("e: {}", e));
        eng.begin_reverse(ReverseTriggerCause::StormUpgrade, snap3)
            .unwrap_or_else(|e| panic!("清账后仍拒: {}", e));
        assert_eq!(eng.phase(), ReversePhase::WindowOpen);
    }

    #[test]
    fn drill_window_events_zero_loss_and_reconcile() {
        // 逆回窗事件零丢失 + 补投对平：pending 账与补投账恒等
        let mut eng = ReverseTransitionEngine::new();
        eng.tick(CRIT_MIN_POLLING_RESIDENCE_TICKS + 4);
        eng.request_forward_migration().unwrap_or_else(|e| panic!("e: {}", e));
        eng.complete_draining().unwrap_or_else(|e| panic!("e: {}", e));
        eng.arm_vectors(&[0x0A01, 0x0A02]).unwrap_or_else(|e| panic!("e: {}", e));
        one_round(&mut eng, ReverseTriggerCause::MaintenanceOrder, 7);
        let st = eng.stats();
        assert_eq!(st.pending_total, 7);
        assert_eq!(st.replayed_total, 7);
        assert_eq!(st.polling_pump_delivered, 7);
        assert!(eng.pending_view().is_empty());
        // 补投凭证序号与窗序连续
        let latest = eng.replay_latest(16);
        assert_eq!(latest.len(), 7);
        let mut i = 1usize;
        while i < latest.len() {
            assert_eq!(latest[i].seq, latest[i - 1].seq + 1);
            i += 1;
        }
        // 跨轮连续审计
        let cont = eng.seq_continuity_audit();
        assert!(cont.continuous);
        assert_eq!(cont.rounds, 1);
    }

    #[test]
    fn drill_anti_oscillation() {
        // 防振荡：驻留未满正向被拒；连续 2 次被拒入振荡红账；期满放行
        let mut eng = ReverseTransitionEngine::new();
        eng.tick(CRIT_MIN_POLLING_RESIDENCE_TICKS + 4);
        eng.request_forward_migration().unwrap_or_else(|e| panic!("e: {}", e));
        eng.complete_draining().unwrap_or_else(|e| panic!("e: {}", e));
        eng.arm_vectors(&[0x0A01]).unwrap_or_else(|e| panic!("e: {}", e));
        one_round(&mut eng, ReverseTriggerCause::FaceFault, 1);
        // one_round 内已驻留期满并正向闭环，重新走一轮并停在 Polling
        let snap = loop {
            match eng.evaluate_reverse_eligibility(ReverseTriggerCause::FaceFault) {
                Ok(s) => break s,
                Err(_) => {
                    eng.tick(
                        CRIT_SILENCE_WINDOW_TICKS
                            + (CRIT_DEBOUNCE_CONFIRM_COUNT as u64) * (RATE_WINDOW_TICKS as u64)
                            + CRIT_REVERSE_COOLDOWN_TICKS,
                    );
                }
            }
        };
        eng.begin_reverse(ReverseTriggerCause::FaceFault, snap)
            .unwrap_or_else(|e| panic!("e: {}", e));
        eng.advance_order_step(ReverseOrderStep::DisarmArming).unwrap_or_else(|e| panic!("e: {}", e));
        eng.advance_order_step(ReverseOrderStep::IrqExit).unwrap_or_else(|e| panic!("e: {}", e));
        eng.advance_order_step(ReverseOrderStep::PollingEnter).unwrap_or_else(|e| panic!("e: {}", e));
        eng.replay_pending().unwrap_or_else(|e| panic!("e: {}", e));
        eng.reconcile().unwrap_or_else(|e| panic!("e: {}", e));
        // 驻留期 0 tick：立即申请 → 0x1387
        match eng.request_forward_migration() {
            Err(IrqRevError::ResidenceNotMet { remaining }) => {
                assert!(remaining > 0);
            }
            other => panic!("驻留拦截失灵: {:?}", other.err()),
        }
        // 再拒一次 → 振荡红账 0x1388
        match eng.request_forward_migration() {
            Err(IrqRevError::OscillationSuspected { attempts }) => {
                assert_eq!(attempts, OSCILLATION_RED_THRESHOLD);
            }
            other => panic!("振荡红账失灵: {:?}", other.err()),
        }
        assert_eq!(eng.red.total_oscillation(), 1);
        assert_eq!(eng.stats().forward_blocked, 2);
        // 驻留期满 → 放行
        eng.tick(CRIT_MIN_POLLING_RESIDENCE_TICKS + 4);
        let g = eng
            .request_forward_migration()
            .unwrap_or_else(|e| panic!("期满仍拒: {}", e));
        assert!(g.residence_ticks >= CRIT_MIN_POLLING_RESIDENCE_TICKS);
        assert_eq!(eng.state(), IrqState::Draining);
    }

    #[test]
    fn drill_dual_run_consistency() {
        // 双跑确定性对拍：同 seed 两跑摘要逐字段一致（J1 双跑面）
        let rep = assert_dual_run(4, 0xDEAD_BEEF).unwrap_or_else(|e| panic!("对拍失败: {}", e));
        assert!(rep.agree);
        assert_eq!(rep.digest.rounds, 4);
        assert_eq!(rep.digest.events_total, rep.digest.events_total);
        // pending == replayed（守恒恒等在对拍面上复账）
        assert_eq!(rep.digest.pending_total, rep.digest.replayed_total);
        // 不同 seed 指纹不同（非退化随机源）
        let a = run_deterministic_scenario(2, 1).unwrap_or_else(|e| panic!("e: {}", e));
        let b = run_deterministic_scenario(2, 2).unwrap_or_else(|e| panic!("e: {}", e));
        assert_ne!(a.fingerprint, b.fingerprint);
    }

    #[test]
    fn drill_seq_continuity_10_rounds() {
        // UNX-F1263-J1：序号连续断言 10 轮
        let mut eng = ReverseTransitionEngine::new();
        eng.tick(CRIT_MIN_POLLING_RESIDENCE_TICKS + 4);
        eng.request_forward_migration().unwrap_or_else(|e| panic!("e: {}", e));
        eng.complete_draining().unwrap_or_else(|e| panic!("e: {}", e));
        eng.arm_vectors(&[0x0A01]).unwrap_or_else(|e| panic!("e: {}", e));
        let mut r = 0u32;
        while r < 10 {
            let we = (r as usize) % 4; // 0..=3 条窗事件，含空窗轮
            let pre_st = eng.stats().pending_total;
            one_round(&mut eng, ReverseTriggerCause::MaintenanceOrder, we);
            assert_eq!(eng.stats().pending_total, pre_st + we as u64);
            r += 1;
        }
        let cont = eng.seq_continuity_audit();
        assert_eq!(cont.rounds, 10);
        assert_eq!(cont.gaps, 0);
        assert!(cont.continuous);
        // 空窗轮的窗口账 events==0 不破坏端点对平（one_round 内 reconcile 已断）
        assert_eq!(eng.stats().rounds_completed, 11);
    }

    #[test]
    fn drill_injections_detected() {
        // 注入面三类全检出
        let mut eng = ReverseTransitionEngine::new();
        eng.tick(CRIT_MIN_POLLING_RESIDENCE_TICKS + 4);
        eng.request_forward_migration().unwrap_or_else(|e| panic!("e: {}", e));
        eng.complete_draining().unwrap_or_else(|e| panic!("e: {}", e));
        eng.arm_vectors(&[0x0A01]).unwrap_or_else(|e| panic!("e: {}", e));
        // 注入一：非法逆回
        let o1 = eng
            .inject_illegal_reversal(ReverseTriggerCause::FaceFault)
            .unwrap_or_else(|e| panic!("注入一前置失败: {}", e));
        assert!(o1.intercepted);
        assert_eq!(o1.code, CODE_UNACKED_VECTORS_PRESENT);
        // 清账后走一轮把引擎带回 Irq
        assert!(eng.ack_vector(0x1F3F) || eng.ack_vector(0x0A01));
        one_round(&mut eng, ReverseTriggerCause::FaceFault, 2);
        // 注入二：开逆回窗后窗事件注入
        let snap = loop {
            match eng.evaluate_reverse_eligibility(ReverseTriggerCause::StormUpgrade) {
                Ok(s) => break s,
                Err(_) => eng.tick(
                    CRIT_SILENCE_WINDOW_TICKS
                        + (CRIT_DEBOUNCE_CONFIRM_COUNT as u64) * (RATE_WINDOW_TICKS as u64)
                        + CRIT_REVERSE_COOLDOWN_TICKS,
                ),
            }
        };
        eng.begin_reverse(ReverseTriggerCause::StormUpgrade, snap)
            .unwrap_or_else(|e| panic!("e: {}", e));
        let o2 = eng
            .inject_window_events(5)
            .unwrap_or_else(|e| panic!("注入二失败: {}", e));
        assert!(o2.intercepted);
        assert_eq!(eng.pending_view().len(), 5);
        eng.advance_order_step(ReverseOrderStep::DisarmArming).unwrap_or_else(|e| panic!("e: {}", e));
        eng.advance_order_step(ReverseOrderStep::IrqExit).unwrap_or_else(|e| panic!("e: {}", e));
        eng.advance_order_step(ReverseOrderStep::PollingEnter).unwrap_or_else(|e| panic!("e: {}", e));
        let rep = eng.replay_pending().unwrap_or_else(|e| panic!("e: {}", e));
        assert_eq!(rep.replayed, 5);
        eng.reconcile().unwrap_or_else(|e| panic!("e: {}", e));
        // 注入三：振荡（驻留期 0 时灌 3 次申请）
        let o3 = eng
            .inject_oscillation(3)
            .unwrap_or_else(|e| panic!("注入三失败: {}", e));
        assert!(o3.intercepted);
        assert_eq!(eng.red.total_oscillation(), 2);
    }

    #[test]
    fn drill_criteria_not_met_paths() {
        // 判据未满足三路：静默窗 / 事件率防抖 / 冷却期
        let mut eng = ReverseTransitionEngine::new();
        eng.tick(CRIT_MIN_POLLING_RESIDENCE_TICKS + 4);
        eng.request_forward_migration().unwrap_or_else(|e| panic!("e: {}", e));
        eng.complete_draining().unwrap_or_else(|e| panic!("e: {}", e));
        eng.arm_vectors(&[0x0A01]).unwrap_or_else(|e| panic!("e: {}", e));
        // 静默窗未满（刚 raise 过事件）
        let _t = eng.raise_event(0x0A01).unwrap_or_else(|e| panic!("e: {}", e));
        assert!(eng.ack_vector(0x0A01));
        eng.tick(4);
        match eng.evaluate_reverse_eligibility(ReverseTriggerCause::FaceFault) {
            Err(IrqRevError::CriteriaNotMet { criterion_id, .. }) => {
                assert_eq!(criterion_id, CRIT_ID_SILENCE);
            }
            other => panic!("静默判据失灵: {:?}", other.err()),
        }
        // 事件率判据的引擎级覆盖在滞回门单测（本引擎常量下，静默窗 64 tick
        // 必然携带 ≥3 个安静采样窗，率判据与静默判据同满足；率未确认分支由
        // HysteresisGate 单测 drill_gate_and_rate_window_units 覆盖）。
        // 走一轮空窗逆回（0 条窗事件），PollingEnter 收口后冷却期=0：
        eng.tick(CRIT_SILENCE_WINDOW_TICKS + (CRIT_DEBOUNCE_CONFIRM_COUNT as u64) * (RATE_WINDOW_TICKS as u64) + 2);
        let snap = eng
            .evaluate_reverse_eligibility(ReverseTriggerCause::FaceFault)
            .unwrap_or_else(|e| panic!("e: {}", e));
        eng.begin_reverse(ReverseTriggerCause::FaceFault, snap)
            .unwrap_or_else(|e| panic!("e: {}", e));
        eng.advance_order_step(ReverseOrderStep::DisarmArming).unwrap_or_else(|e| panic!("e: {}", e));
        eng.advance_order_step(ReverseOrderStep::IrqExit).unwrap_or_else(|e| panic!("e: {}", e));
        eng.advance_order_step(ReverseOrderStep::PollingEnter).unwrap_or_else(|e| panic!("e: {}", e));
        let rep0 = eng.replay_pending().unwrap_or_else(|e| panic!("e: {}", e));
        assert_eq!(rep0.replayed, 0);
        eng.reconcile().unwrap_or_else(|e| panic!("e: {}", e));
        // 冷却期未满：收口后立即评估，冷却 0 < 32 → 拒（判据三拦截）
        match eng.evaluate_reverse_eligibility(ReverseTriggerCause::FaceFault) {
            Err(IrqRevError::CriteriaNotMet { criterion_id, .. }) => {
                assert_eq!(criterion_id, CRIT_ID_COOLDOWN);
            }
            other => panic!("冷却判据失灵: {:?}", other.err()),
        }
        // 冷却期满后判据放行：先走正向链回 Irq 态，再评估-执行一轮
        eng.tick(CRIT_MIN_POLLING_RESIDENCE_TICKS + 4);
        eng.request_forward_migration().unwrap_or_else(|e| panic!("e: {}", e));
        eng.complete_draining().unwrap_or_else(|e| panic!("e: {}", e));
        eng.arm_vectors(&[0x0A01]).unwrap_or_else(|e| panic!("e: {}", e));
        eng.tick(
            CRIT_SILENCE_WINDOW_TICKS
                + (CRIT_DEBOUNCE_CONFIRM_COUNT as u64) * (RATE_WINDOW_TICKS as u64)
                + CRIT_REVERSE_COOLDOWN_TICKS,
        );
        let snap2 = eng
            .evaluate_reverse_eligibility(ReverseTriggerCause::FaceFault)
            .unwrap_or_else(|e| panic!("e: {}", e));
        eng.begin_reverse(ReverseTriggerCause::FaceFault, snap2)
            .unwrap_or_else(|e| panic!("e: {}", e));
        eng.advance_order_step(ReverseOrderStep::DisarmArming).unwrap_or_else(|e| panic!("e: {}", e));
        eng.advance_order_step(ReverseOrderStep::IrqExit).unwrap_or_else(|e| panic!("e: {}", e));
        eng.advance_order_step(ReverseOrderStep::PollingEnter).unwrap_or_else(|e| panic!("e: {}", e));
        eng.replay_pending().unwrap_or_else(|e| panic!("e: {}", e));
        eng.reconcile().unwrap_or_else(|e| panic!("e: {}", e));
        eng.tick(CRIT_MIN_POLLING_RESIDENCE_TICKS + 4);
        eng.request_forward_migration().unwrap_or_else(|e| panic!("e: {}", e));
        eng.complete_draining().unwrap_or_else(|e| panic!("e: {}", e));
        eng.arm_vectors(&[0x0A01]).unwrap_or_else(|e| panic!("e: {}", e));
        assert_eq!(eng.state(), IrqState::Irq);
    }

    #[test]
    fn drill_order_violation_rejected() {
        // 全序乱序拒绝：跳步提交必吃 0x1386
        let mut eng = ReverseTransitionEngine::new();
        eng.tick(CRIT_MIN_POLLING_RESIDENCE_TICKS + 4);
        eng.request_forward_migration().unwrap_or_else(|e| panic!("e: {}", e));
        eng.complete_draining().unwrap_or_else(|e| panic!("e: {}", e));
        eng.arm_vectors(&[0x0A01]).unwrap_or_else(|e| panic!("e: {}", e));
        eng.tick(
            CRIT_SILENCE_WINDOW_TICKS
                + (CRIT_DEBOUNCE_CONFIRM_COUNT as u64) * (RATE_WINDOW_TICKS as u64)
                + 2,
        );
        let snap = eng
            .evaluate_reverse_eligibility(ReverseTriggerCause::FaceFault)
            .unwrap_or_else(|e| panic!("e: {}", e));
        eng.begin_reverse(ReverseTriggerCause::FaceFault, snap)
            .unwrap_or_else(|e| panic!("e: {}", e));
        // 首步跳到 IrqExit → 拒（期望 DisarmArming）
        match eng.advance_order_step(ReverseOrderStep::IrqExit) {
            Err(IrqRevError::OrderViolation { expected, got }) => {
                assert_eq!(expected, ReverseOrderStep::DisarmArming);
                assert_eq!(got, ReverseOrderStep::IrqExit);
            }
            other => panic!("乱序拦截失灵: {:?}", other.err()),
        }
        assert_eq!(eng.stats().order_violations, 1);
        // 首步跳到 PollingEnter → 拒
        match eng.advance_order_step(ReverseOrderStep::PollingEnter) {
            Err(IrqRevError::OrderViolation { .. }) => {}
            other => panic!("乱序拦截失灵: {:?}", other.err()),
        }
        // 正序走通
        eng.advance_order_step(ReverseOrderStep::DisarmArming).unwrap_or_else(|e| panic!("e: {}", e));
        eng.advance_order_step(ReverseOrderStep::IrqExit).unwrap_or_else(|e| panic!("e: {}", e));
        eng.advance_order_step(ReverseOrderStep::PollingEnter).unwrap_or_else(|e| panic!("e: {}", e));
        assert_eq!(eng.stats().reverse_count, 1);
    }

    #[test]
    fn drill_snapshot_mismatch_and_stale() {
        // 快照对平：原因不符 / 过期快照均拒（0x138E）
        let mut eng = ReverseTransitionEngine::new();
        eng.tick(CRIT_MIN_POLLING_RESIDENCE_TICKS + 4);
        eng.request_forward_migration().unwrap_or_else(|e| panic!("e: {}", e));
        eng.complete_draining().unwrap_or_else(|e| panic!("e: {}", e));
        eng.arm_vectors(&[0x0A01]).unwrap_or_else(|e| panic!("e: {}", e));
        eng.tick(
            CRIT_SILENCE_WINDOW_TICKS
                + (CRIT_DEBOUNCE_CONFIRM_COUNT as u64) * (RATE_WINDOW_TICKS as u64)
                + 2,
        );
        // 原因不符：A 评估、B 执行
        let snap = eng
            .evaluate_reverse_eligibility(ReverseTriggerCause::FaceFault)
            .unwrap_or_else(|e| panic!("e: {}", e));
        match eng.begin_reverse(ReverseTriggerCause::StormUpgrade, snap) {
            Err(IrqRevError::SnapshotMismatch { reason }) => {
                assert_eq!(reason, "原因码不匹配");
            }
            other => panic!("快照对平失灵: {:?}", other.err()),
        }
        // 过期快照：评估后推 9 tick（> SNAPSHOT_FRESH_TICKS）
        eng.tick(SNAPSHOT_FRESH_TICKS + 1);
        match eng.begin_reverse(ReverseTriggerCause::FaceFault, snap) {
            Err(IrqRevError::SnapshotMismatch { reason }) => {
                assert_eq!(reason, "快照已过期");
            }
            other => panic!("快照新鲜度失灵: {:?}", other.err()),
        }
        // 重新评估后立即执行：放行
        let snap2 = eng
            .evaluate_reverse_eligibility(ReverseTriggerCause::FaceFault)
            .unwrap_or_else(|e| panic!("e: {}", e));
        eng.begin_reverse(ReverseTriggerCause::FaceFault, snap2)
            .unwrap_or_else(|e| panic!("e: {}", e));
        assert_eq!(eng.phase(), ReversePhase::WindowOpen);
    }

    #[test]
    fn drill_sealed_and_replay_window_closed() {
        // 封账面 + 补投窗面
        let mut eng = ReverseTransitionEngine::new();
        // 启动态（Polling+Completed 但窗账未定格）：reconcile 拒（补投窗未开）
        match eng.reconcile() {
            Err(IrqRevError::ReplayWindowClosed) => {}
            other => panic!("对平窗判据失灵: {:?}", other.err()),
        }
        // 空账补投合法放行（0 条，连续成立）
        let rep = eng.replay_pending().unwrap_or_else(|e| panic!("空窗补投失败: {}", e));
        assert_eq!(rep.replayed, 0);
        assert!(rep.continuous);
        // 封账后一切写操作拒绝（0x138C）
        eng.seal();
        assert!(eng.is_sealed());
        match eng.raise_event(1) {
            Err(IrqRevError::LedgerSealed) => {}
            other => panic!("封账拦截失灵: {:?}", other.err()),
        }
        match eng.request_forward_migration() {
            Err(IrqRevError::LedgerSealed) => {}
            other => panic!("封账拦截失灵: {:?}", other.err()),
        }
        match eng.replay_pending() {
            Err(IrqRevError::LedgerSealed) => {}
            other => panic!("封账拦截失灵: {:?}", other.err()),
        }
    }

    #[test]
    fn drill_error_codes_and_details() {
        // 15 个错误变体：码位落 0x1381..=0x138F、中文 detail 非空
        let errs: Vec<IrqRevError> = alloc_vec_of_errors();
        let mut i = 0usize;
        while i < errs.len() {
            let e = errs[i];
            let c = e.code();
            assert!(c >= SEG_BASE + 1 && c <= SEG_LAST, "码位越段: 0x{:04X}", c);
            assert!(!e.detail().is_empty());
            assert!(!format!("{}", e).is_empty());
            i += 1;
        }
        assert_eq!(errs.len(), 15);
    }

    fn alloc_vec_of_errors() -> Vec<IrqRevError> {
        alloc::vec![
            IrqRevError::NotInResidentState {
                state: IrqState::Idle,
                phase: ReversePhase::Dormant,
            },
            IrqRevError::AlreadyReversing,
            IrqRevError::CriteriaNotMet {
                criterion_id: 1,
                criterion_name: "静默窗时长",
            },
            IrqRevError::UnknownCriterion { id: 99 },
            IrqRevError::UnackedVectorsPresent { count: 1 },
            IrqRevError::OrderViolation {
                expected: ReverseOrderStep::DisarmArming,
                got: ReverseOrderStep::PollingEnter,
            },
            IrqRevError::ResidenceNotMet { remaining: 1 },
            IrqRevError::OscillationSuspected { attempts: 2 },
            IrqRevError::PendingOverflow { vector: 3 },
            IrqRevError::ReplaySeqGap {
                expected: 4,
                got: 5,
            },
            IrqRevError::ReconcileMismatch {
                pending_total: 1,
                replayed_total: 2,
            },
            IrqRevError::LedgerSealed,
            IrqRevError::ReplayWindowClosed,
            IrqRevError::SnapshotMismatch {
                reason: "快照已过期",
            },
            IrqRevError::DualRunMismatch { left: 1, right: 2 },
        ]
    }

    #[test]
    fn drill_health_explain_json() {
        // 健康面 / 人话化 / JSON 导出非空且结论绿
        let mut eng = ReverseTransitionEngine::new();
        eng.tick(CRIT_MIN_POLLING_RESIDENCE_TICKS + 4);
        eng.request_forward_migration().unwrap_or_else(|e| panic!("e: {}", e));
        eng.complete_draining().unwrap_or_else(|e| panic!("e: {}", e));
        eng.arm_vectors(&[0x0A01]).unwrap_or_else(|e| panic!("e: {}", e));
        one_round(&mut eng, ReverseTriggerCause::FaceFault, 3);
        let h = eng.health();
        assert_eq!(h.verdict, HealthVerdict::Green);
        assert_eq!(h.items.len(), 8);
        let mut i = 0usize;
        while i < h.items.len() {
            assert!(h.items[i].ok, "体检项 {} 未过: {}", i + 1, h.items[i].note);
            i += 1;
        }
        let ex = eng.explain();
        assert!(ex.contains("UNX-F1263"));
        assert!(ex.contains("逆回判据现值"));
        assert!(ex.contains("零丢失成立"));
        let js = eng.export_json();
        assert!(js.starts_with('{'));
        assert!(js.contains("\"seg\":4992")); // 0x1380 = 4992
        assert!(js.contains("\"health_verdict\":\"绿\""));
    }

    #[test]
    fn drill_gate_and_rate_window_units() {
        // 滞回门：防抖确认 + 滞回上沿立即翻回 + 翻转留痕
        let mut g = HysteresisGate::new();
        assert!(!g.confirmed());
        g.feed(5, 16);
        g.feed(5, 32);
        assert!(!g.confirmed());
        g.feed(0, 48);
        g.feed(0, 64);
        g.feed(0, 80);
        assert!(g.confirmed());
        assert_eq!(g.low_streak(), 3);
        // 滞回上沿：单窗高率立即解除
        g.feed(9, 96);
        assert!(!g.confirmed());
        assert_eq!(g.transitions(), 2);
        assert!(g.flaps_within(256, 100) >= 2);
        // 采样窗：record/sum/边界清零
        let mut w = RateWindow::new();
        let mut i = 0u64;
        while i < 5 {
            w.record(i);
            i += 1;
        }
        assert_eq!(w.sum(), 5);
        w.clear(16);
        assert_eq!(w.sum(), 0);
        assert_eq!(w.base(), 16);
        // advance_to 兜底
        w.record(20);
        w.advance_to(200);
        assert_eq!(w.sum(), 0);
    }

    #[test]
    fn drill_const_face_and_views() {
        // const 面：判据表查值/查名、态码、步码、未知判据
        assert_eq!(crit_threshold(CRIT_ID_SILENCE), CRIT_SILENCE_WINDOW_TICKS);
        assert_eq!(crit_threshold(99), 0);
        assert_eq!(crit_name(CRIT_ID_RATE), "事件率跌破阈值(16tick窗计数下限)");
        assert_eq!(crit_name(99), "未知判据");
        assert_eq!(IrqState::Irq.code(), 4);
        assert_eq!(IrqState::Polling.name(), "Polling");
        assert_eq!(ReverseOrderStep::expected_at(0), 0);
        assert_eq!(ReverseOrderStep::expected_at(2), 2);
        assert_eq!(ReverseOrderStep::expected_at(3), -1);
        assert_eq!(ReverseTriggerCause::MaintenanceOrder.id(), 3);
        // 判据现值视图
        let eng = ReverseTransitionEngine::new();
        let views = eng.criteria_view();
        assert_eq!(views.len(), 3);
        assert_eq!(views[0].id, CRIT_ID_SILENCE);
        assert!(views[2].met); // 从未逆回：冷却恒满足
        // 快照原因对平
        let s = CritSnapshot {
            cause_id: 1,
            evaluated_tick: 0,
            silence_ticks: 0,
            silence_met: true,
            window_events: 0,
            rate_met: true,
            cooldown_ticks: 0,
            cooldown_met: true,
            verdict: true,
            fail_id: 0,
        };
        assert!(s.matches_cause(ReverseTriggerCause::FaceFault));
        assert!(!s.matches_cause(ReverseTriggerCause::StormUpgrade));
    }

    #[test]
    fn drill_audit_replay_and_cause_filter() {
        // 审计环回放重演 + 按原因过滤 + 环形滚动保底
        let mut eng = ReverseTransitionEngine::new();
        eng.tick(CRIT_MIN_POLLING_RESIDENCE_TICKS + 4);
        eng.request_forward_migration().unwrap_or_else(|e| panic!("e: {}", e));
        eng.complete_draining().unwrap_or_else(|e| panic!("e: {}", e));
        eng.arm_vectors(&[0x0A01]).unwrap_or_else(|e| panic!("e: {}", e));
        one_round(&mut eng, ReverseTriggerCause::FaceFault, 1);
        one_round(&mut eng, ReverseTriggerCause::MaintenanceOrder, 2);
        let all = eng.audit_replay();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].round_no, 1);
        assert_eq!(all[1].round_no, 2);
        let ff = eng.audit.find_by_cause(ReverseTriggerCause::FaceFault.id());
        assert_eq!(ff.len(), 1);
        let mo = eng.audit.find_by_cause(ReverseTriggerCause::MaintenanceOrder.id());
        assert_eq!(mo.len(), 1);
        assert_eq!(mo[0].window_events, 2);
        // 窗内连续：轮 1 窗序 [2..2]，轮 2 窗序 [4..5]（正向事件占序 1/3 不入窗）
        assert_eq!(all[0].window_first_seq, 2);
        assert_eq!(all[0].window_last_seq, 2);
        assert_eq!(all[1].window_first_seq, 4);
        assert_eq!(all[1].window_last_seq, 5);
        let cont = eng.seq_continuity_audit();
        assert!(cont.continuous);
    }

    #[test]
    fn drill_red_ledger_and_lcg() {
        // 红账：定容滚动淘汰留痕 + 类别恒等账；LCG 确定性
        let mut red = RedLedger::new();
        let mut i = 0u64;
        while i < (RED_CAP as u64) + 10 {
            red.push(i, RedKind::PendingOverflow { vector: (i as u16) & 0xFF });
            i += 1;
        }
        assert_eq!(red.count(), RED_CAP);
        assert_eq!(red.evicted(), 10);
        assert_eq!(red.total_ever(), RED_CAP as u64 + 10);
        assert_eq!(red.total_overflow(), RED_CAP as u64 + 10);
        let snap = red.snapshot();
        assert_eq!(snap.len(), RED_CAP);
        // LCG：同 seed 同序列
        let mut a = Lcg::new(7);
        let mut b = Lcg::new(7);
        let mut j = 0u32;
        while j < 16 {
            assert_eq!(a.next(), b.next());
            j += 1;
        }
        let mut c = Lcg::new(7);
        assert_eq!(c.below(0), 0);
        assert!(c.below(100) < 100);
    }
}






