//! unx0944_bhtrig — UNX-F0944 bottom half 触发点语义（pending 置位与三处触发时机）。
//!
//! 判据原文（施工书锚点 UNX-F0944，CoRun·UNX 中断面 B08 批：
//! 两级分发与 bottom half 底座）：
//! **UNX-F0944-J1 三触发点正确（中断返回前/调度点/显式触发各 10/10 次），
//! pending 一致性（置位-触发-清位账平 10 万次），丢失注入 10/10 次检测**。
//!
//! 定位原文（同批清单逐字）：
//! | UNX-F0944 | bottom half 触发点语义（pending 置位与三处触发时机） | 320 |
//! | 已深化 | UNX-F0944-J1 三触发点正确（中断返回前/调度点/显式触发各
//! 10/10 次），pending 一致性（置位-触发-清位账平 10 万次），丢失注入
//! 10/10 次检测 |
//!
//! 职责定位：bottom half 的生命周期有一个**三段账**：top half 把工作
//! 记下来（pending 置位）→ 某个触发点把工作做掉（触发处理）→ 位图
//! 清位（闭环）。Windows 深在哪：Windows 的 DPC/软中断语义把"何时
//! 做"写成三处硬时机——中断返回前（KiRetireDpcList 在 IRQL 下降
//! 路径上）、调度点（KeLowerIrql/调度器让位前扫 DPC 队列）、显式
//! 触发（KeInsertQueueDpc 立即派发或驱动手动 drain）——三处时机
//! 各有各的预算与让位纪律，错过即丢（DPC 超时 = 系统卡死的头号
//! 嫌犯），检查靠 DPC watchdog 超时崩出来而不是账面红出来；Varix：
//! ①**三触发点闭集**——中断返回前（同核即时、预算 8）/调度点
//! （让位后、预算 4）/显式触发（调用方驱动、预算 32 全扫），三处
//! 时机语义表冻结（预算表+语义注记表入指纹，表改即漂移立案）；
//! ②**pending 置位面**——32 槽位图，置位幂等（已置位再置位记合并
//! 不重复记账——F0951 合并去重的账面地基），每槽位独立六账
//! （置位/服务/清位丢失/合并/末次置位 tick/置位后触发点经过数）；
//! ③**账平判据**——每槽位与全局恒等式
//! `有效置位数 == 触发处理数 + 未决数 + 清位丢失数`，10 万次
//! LCG 混合流（置位 60%/触发 20%/显式清位 10%/扫描 10%）逐槽位
//! 与全局双查账，账不平即 Err 立案；
//! ④**丢失检测与归因**——置位后超龄未决（tick 龄 > 64）扫描 +
//! 清位丢失回溯扫描，归因二分：触发点未跑（置位后零次触发经过）
//! /位图被清（清位丢失计数增量），丢失注入 10/10 次全检出；
//! ⑤**触发次序纪律**——pending 槽位按（优先级,槽位号）序执行，
//! 同点多槽一次全处理（预算内），处理中新置位推迟语义（快照驱动
//! 不重入——本轮不服务本轮中途置的位，下轮必达，推迟缺席红账）；
//! ⑥**per 核隔离**——每核独立 pending 面与独立触发账，本地置位
//! 只许本核、跨核置位必须走核间通知（IPI 链未建立即拒），跨核
//! 误置注入双通道（直置他核/无链跨发）各检出；
//! ⑦**事件账+健康+指纹+双跑**——256 槽环形事件账（seq 有序、
//! 溢出饱和记账）、health() 三档裁决（Green/Watch/Red）、FNV-1a
//! 64 位引擎指纹、同种子双跑逐位对拍（指纹+四账+事件数三查）。
//! 上游：F0905 中断上下文契约（top half 的 BhPost 唯一出口投递到
//! 本面置位）、F0903 handler 生命周期（InService 态才可置位）；
//! 下游：F0950 分发停滞分析（本单丢失归因联动闭环）、F0951 合并
//! 去重（本单置位幂等账面直供）、C 域调度器（调度点触发挂接）。
//! 本单交付**三触发点闭集 + pending 置位面 + 账平审计 + 丢失检测
//! 归因 + 次序纪律 + per 核隔离 + 注入面 + 健康指纹双跑**八面。
//!
//! 设计要点：no_std+alloc；零 unwrap/expect/panic/运行期 assert
//! （可失败全走 Result，尺寸约束编译期 const assert）；零 IO 零墙钟
//! （tick 显性注入、单调递减即拒）；LCG 确定性仿真（Numerical
//! Recipes 参数）；FNV-1a 快照；计数器全饱和/回卷显性；错误全显性
//! 带诊断码与中文可操作建议。诊断码段 0x19F0（0x19C0/0x19D0 为
//! CGPU F0029/F0030 所取、0x19E0 为 svstar2 ves475 所取，整仓 grep
//! 确认 0x19F0 段零占用后选定，独占 0x10）。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const BH_TRIG_VERSION: &str = "B08-bhtrig-v1";

// ---------------------------------------------------------------------------
// 常量（来源逐条注记）
// ---------------------------------------------------------------------------

/// pending 置位面槽位上限（来源：判据「32 槽位图」——u32 位图宽度，
/// 位运算单字清置，再宽即越出单字位图纪律）。
pub const MAX_SLOTS: usize = 32;
/// per 核隔离面核数上限（来源：判据「每核独立 pending 面」——4 核
/// 演练面：BSP+3 AP 的最小对称面，够演练跨核全序不虚扩）。
pub const MAX_CORES: usize = 4;
/// 事件账环形容量（来源：判据「事件账」——256 槽：32 槽位 × 8 轮
/// 全事件覆盖，溢出走饱和记账不丢位）。
pub const LEDGER_CAP: usize = 256;
/// 丢失判定超龄阈值（来源：判据「置位后永未触发」——64 tick：
/// 演练 tick 步进 1~7，64 步覆盖 10 轮注入周期仍有余，短于此会把
/// 正常预算内排队误判为丢失）。
pub const LOSS_THRESHOLD_TICKS: u64 = 64;
/// 中断返回前触发点预算（来源：判据「中断返回前」语义——快速
/// 返回，每轮至多服务 8 槽；余下留给调度点/显式触发）。
pub const BUDGET_IRQ_RETURN: u8 = 8;
/// 调度点触发点预算（来源：判据「调度点」语义——让位前小步清扫，
/// 每轮至多 4 槽，让位纪律优先于吞吐）。
pub const BUDGET_SCHED_POINT: u8 = 4;
/// 显式触发点预算（来源：判据「显式触发」语义——调用方驱动全扫，
/// 32 = 全槽位单轮封顶）。
pub const BUDGET_EXPLICIT: u8 = 32;
/// 账平审计混合流操作数（来源：判据「账平 10 万次」——100_000 次
/// LCG 置位-触发-清位混合流）。
pub const BALANCE_AUDIT_OPS: u32 = 100_000;
/// 显式清位排空守护上限（防死循环——预算 32 全扫下 pending 有界，
/// 200_000 轮即 640 万槽次服务远超账面上限，触顶即 Err 立案）。
pub const DRAIN_LIMIT: u32 = 200_000;
/// 事件账「点不适用」标记（register/丢失/注入事件不属于任何触发点）。
pub const POINT_NONE: u8 = 0xFF;
/// 事件账「槽位不适用」标记（FireEnter/FireExit 属面不属槽）。
pub const SLOT_NONE: u8 = 0xFF;

// ---------------------------------------------------------------------------
// 诊断码（0x19F0 段独占 0x10——0x19C0/0x19D0 为 CGPU F0029/F0030
// 所取、0x19E0 为 svstar2 ves475 所取，整仓 grep 零占用后选定）
// ---------------------------------------------------------------------------

/// 诊断段基址（0x19F0 独占 0x10）。
pub const SEG_BASE: u16 = 0x19F0;
/// 0x19F0 槽位越界（slot >= 32——位图单字纪律破）。
pub const CODE_SLOT_OUT_OF_RANGE: u16 = SEG_BASE + 0x00;
/// 0x19F1 槽位未注册（未 register 即置位/触发——账面黑户）。
pub const CODE_SLOT_NOT_REGISTERED: u16 = SEG_BASE + 0x01;
/// 0x19F2 核号越界（core >= 4——per 核面外）。
pub const CODE_CORE_OUT_OF_RANGE: u16 = SEG_BASE + 0x02;
/// 0x19F3 跨核误置（本地置位打到非本核——per 核隔离破）。
pub const CODE_CROSS_CORE_MISSET: u16 = SEG_BASE + 0x03;
/// 0x19F4 核间通知链缺失（IPI 链未建立即跨发）。
pub const CODE_IPI_LINK_MISSING: u16 = SEG_BASE + 0x04;
/// 0x19F5 重入违规（处理中置位被同轮服务——推迟语义破）。
pub const CODE_REENTRY_VIOLATION: u16 = SEG_BASE + 0x05;
/// 0x19F6 次序违规（服务序非（优先级,槽位号）增序）。
pub const CODE_ORDER_VIOLATION: u16 = SEG_BASE + 0x06;
/// 0x19F7 丢失无归因（扫描检出丢失但两类归因都不成立——检测器失效）。
pub const CODE_LOSS_UNATTRIBUTED: u16 = SEG_BASE + 0x07;
/// 0x19F8 账不平（置位 != 服务 + 未决 + 丢失——三段账失衡）。
pub const CODE_BALANCE_BROKEN: u16 = SEG_BASE + 0x08;
/// 0x19F9 双跑失配（同种子双跑指纹或账面不一致——非确定性）。
pub const CODE_DUAL_RUN_MISMATCH: u16 = SEG_BASE + 0x09;
/// 0x19FA 指纹漂移（语义表/健康指纹与冻结基线不一致）。
pub const CODE_FINGERPRINT_DRIFT: u16 = SEG_BASE + 0x0A;
/// 0x19FB tick 回退（单调性破——零墙钟纪律下唯一时序即 tick 序）。
pub const CODE_TICK_REGRESS: u16 = SEG_BASE + 0x0B;
/// 0x19FC 推迟缺席（上轮推迟槽位未在本轮被处理——推迟语义破）。
pub const CODE_DEFERRED_MISS: u16 = SEG_BASE + 0x0C;
/// 0x19FD 事件账溢出（环覆盖发生——饱和记账已立案，查账时可见）。
pub const CODE_LEDGER_OVERFLOW: u16 = SEG_BASE + 0x0D;
/// 0x19FE 健康红档（误置/丢失/账破任一在册——需人工介入）。
pub const CODE_HEALTH_RED: u16 = SEG_BASE + 0x0E;
/// 0x19FF 排空触顶（守护上限打满仍有未决——分发停滞嫌疑，联签
/// F0950 因果账）。
pub const CODE_DRAIN_LIMIT: u16 = SEG_BASE + 0x0F;

// ---------------------------------------------------------------------------
// §1 错误集（每变体 detail() 中文可操作诊断）
// ---------------------------------------------------------------------------

/// 本模块唯一错误类型：可失败面全走 `Result<T, BhTrigError>`。
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum BhTrigError {
    /// 槽位越界（slot >= MAX_SLOTS）。
    SlotOutOfRange {
        /// 核号。
        core: u8,
        /// 越界槽位号。
        slot: u8,
    },
    /// 槽位未注册即使用。
    SlotNotRegistered {
        /// 核号。
        core: u8,
        /// 槽位号。
        slot: u8,
    },
    /// 核号越界（core >= MAX_CORES）。
    CoreOutOfRange {
        /// 越界核号。
        core: u8,
    },
    /// 跨核误置：本地路径打到非本核面（per 核隔离破）。
    CrossCoreMisSet {
        /// 发起核。
        src: u8,
        /// 目标核。
        dst: u8,
        /// 槽位号。
        slot: u8,
    },
    /// 核间通知链缺失：跨核置位但 IPI 链未建立。
    IpiLinkMissing {
        /// 发起核。
        src: u8,
        /// 目标核。
        dst: u8,
    },
    /// 重入违规：处理中置位的槽位被同轮触发服务。
    ReentryViolation {
        /// 核号。
        core: u8,
        /// 被重入服务的槽位号。
        slot: u8,
    },
    /// 次序违规：本轮服务序非（优先级,槽位号）增序。
    OrderViolation {
        /// 核号。
        core: u8,
    },
    /// 丢失检出但归因二分（触发点未跑/位图被清）均不成立。
    LossUnattributed {
        /// 核号。
        core: u8,
        /// 槽位号。
        slot: u8,
    },
    /// 账不平：有效置位数 != 服务数 + 未决数 + 丢失数。
    BalanceBroken {
        /// 核号。
        core: u8,
        /// 失衡槽位号（全局失衡时为 SLOT_NONE）。
        slot: u8,
        /// 有效置位数。
        sets: u32,
        /// 服务 + 未决 + 丢失 合计。
        lhs: u32,
    },
    /// 双跑失配：同种子双跑指纹或账面不一致。
    DualRunMismatch {
        /// A 跑指纹。
        digest_a: u64,
        /// B 跑指纹。
        digest_b: u64,
    },
    /// 指纹漂移：快照与冻结基线（或双采样）不一致。
    FingerprintDrift {
        /// 期望指纹。
        expected: u64,
        /// 实际指纹。
        actual: u64,
    },
    /// tick 回退：单调时序破。
    TickRegress {
        /// 本次 tick。
        tick: u64,
        /// 单调地板（历史最高 tick）。
        floor: u64,
    },
    /// 推迟缺席：上轮推迟槽位本轮未达。
    DeferredMiss {
        /// 核号。
        core: u8,
        /// 缺席槽位号。
        slot: u8,
    },
    /// 事件账溢出（环覆盖已发生——账面饱和记账立案）。
    LedgerOverflow {
        /// 被覆盖的事件数（饱和累计）。
        dropped: u64,
    },
    /// 健康红档。
    HealthRed {
        /// 在册问题诊断码集合（位图：bit i = 段内偏移 i）。
        codes: u32,
    },
    /// 排空触顶：守护上限打满仍有未决。
    DrainLimit {
        /// 排空后仍未决的位图。
        pending: u32,
    },
}

impl BhTrigError {
    /// 诊断码（0x19F0 段）。
    pub fn code(&self) -> u16 {
        match self {
            BhTrigError::SlotOutOfRange { .. } => CODE_SLOT_OUT_OF_RANGE,
            BhTrigError::SlotNotRegistered { .. } => CODE_SLOT_NOT_REGISTERED,
            BhTrigError::CoreOutOfRange { .. } => CODE_CORE_OUT_OF_RANGE,
            BhTrigError::CrossCoreMisSet { .. } => CODE_CROSS_CORE_MISSET,
            BhTrigError::IpiLinkMissing { .. } => CODE_IPI_LINK_MISSING,
            BhTrigError::ReentryViolation { .. } => CODE_REENTRY_VIOLATION,
            BhTrigError::OrderViolation { .. } => CODE_ORDER_VIOLATION,
            BhTrigError::LossUnattributed { .. } => CODE_LOSS_UNATTRIBUTED,
            BhTrigError::BalanceBroken { .. } => CODE_BALANCE_BROKEN,
            BhTrigError::DualRunMismatch { .. } => CODE_DUAL_RUN_MISMATCH,
            BhTrigError::FingerprintDrift { .. } => CODE_FINGERPRINT_DRIFT,
            BhTrigError::TickRegress { .. } => CODE_TICK_REGRESS,
            BhTrigError::DeferredMiss { .. } => CODE_DEFERRED_MISS,
            BhTrigError::LedgerOverflow { .. } => CODE_LEDGER_OVERFLOW,
            BhTrigError::HealthRed { .. } => CODE_HEALTH_RED,
            BhTrigError::DrainLimit { .. } => CODE_DRAIN_LIMIT,
        }
    }

    /// 中文可操作诊断（每变体固定话术——诊断文案即文档）。
    pub fn detail(&self) -> &'static str {
        match self {
            BhTrigError::SlotOutOfRange { .. } => {
                "槽位越界：pending 面是 32 槽单字位图，slot 必须 < 32；\
                 检查槽位号来源是否越出 MAX_SLOTS"
            }
            BhTrigError::SlotNotRegistered { .. } => {
                "槽位未注册：先 register_slot 挂优先级与标签再置位；\
                 黑户槽位不入账，属账面纪律破"
            }
            BhTrigError::CoreOutOfRange { .. } => {
                "核号越界：per 核隔离面为 4 核（MAX_CORES），core 必须 < 4；\
                 检查核号来源（BSP=0，AP=1..3）"
            }
            BhTrigError::CrossCoreMisSet { .. } => {
                "跨核误置：本地置位只许打本核面；跨核需求一律走 \
                 raise_cross（核间通知），直置他核即隔离破——先 link_cores \
                 建 IPI 链，再走跨核通道"
            }
            BhTrigError::IpiLinkMissing { .. } => {
                "核间通知链缺失：跨核置位前必须 link_cores(源,目标) 建立 \
                 IPI 链；未建链即跨发按误置拒收立案"
            }
            BhTrigError::ReentryViolation { .. } => {
                "重入违规：处理中置位的槽位被同轮触发服务——快照驱动 \
                 纪律破；检查触发路径是否绕过 fire 的快照面直接改账"
            }
            BhTrigError::OrderViolation { .. } => {
                "次序违规：同轮服务序必须按（优先级,槽位号）增序；\
                 检查是否绕过 fire 的排序面手工选槽"
            }
            BhTrigError::LossUnattributed { .. } => {
                "丢失无归因：检出丢失但「触发点未跑/位图被清」两类归因 \
                 均不成立——检测器自身失效，先查扫描快照基线是否被跳过"
            }
            BhTrigError::BalanceBroken { .. } => {
                "账不平：有效置位数 != 触发处理数 + 未决数 + 清位丢失数；\
                 三段账失衡必有一处直改位图绕过账面——逐槽位对账定位"
            }
            BhTrigError::DualRunMismatch { .. } => {
                "双跑失配：同种子双跑指纹不一致——引擎含非确定性源；\
                 检查是否引入墙钟/地址随机性/未初始化读"
            }
            BhTrigError::FingerprintDrift { .. } => {
                "指纹漂移：语义表或引擎指纹与冻结基线不一致——契约表 \
                 被改未走版本面；核对 BUDGET_* 与语义注记表"
            }
            BhTrigError::TickRegress { .. } => {
                "tick 回退：时序单调性破——零墙钟纪律下 tick 是唯一时序，\
                 回退即账面时序不可信；检查 tick 注入源"
            }
            BhTrigError::DeferredMiss { .. } => {
                "推迟缺席：上轮处理中置位的槽位本轮未达——推迟语义破；\
                 检查本轮触发是否越快照面丢槽"
            }
            BhTrigError::LedgerOverflow { .. } => {
                "事件账溢出：256 槽环已覆盖旧事件——饱和记账在册；\
                 需要全量账时先取指纹再续跑，或调大 LEDGER_CAP"
            }
            BhTrigError::HealthRed { .. } => {
                "健康红档：误置/丢失/账破任一在册——按位图定位段内偏移 \
                 对应诊断码，先清注入面再复检"
            }
            BhTrigError::DrainLimit { .. } => {
                "排空触顶：显式全扫预算下仍有未决——分发停滞嫌疑，\
                 联签 F0950 因果账；检查是否有槽位被持续饿死"
            }
        }
    }
}

// ---------------------------------------------------------------------------
// §2 三触发点闭集（中断返回前/调度点/显式触发——语义表冻结）
// ---------------------------------------------------------------------------

/// bottom half 触发点闭集——**三处时机，无第四处**。
///
/// 语义差异（表冻结，指纹入契约）：
/// - `IrqReturn`（中断返回前）：同核即时——top half 收尾路上顺手
///   做 BH，预算 8（快速返回纪律：中断路径不可久留）；
/// - `SchedPoint`（调度点）：让位后——调度器让位前小步清扫，预算
///   4（让位纪律优先于吞吐：能做一点是一点，做不完让位）；
/// - `Explicit`（显式触发）：调用方驱动——ksoftirqd 式显式排空，
///   预算 32 全扫（调用方明确要求做净）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BhTriggerPoint {
    /// 中断返回前（同核即时）。
    IrqReturn,
    /// 调度点（让位后）。
    SchedPoint,
    /// 显式触发（调用方驱动）。
    Explicit,
}

impl BhTriggerPoint {
    /// 全闭集（3 处——闭集锚）。
    pub const fn all() -> [BhTriggerPoint; 3] {
        [BhTriggerPoint::IrqReturn, BhTriggerPoint::SchedPoint, BhTriggerPoint::Explicit]
    }

    /// 段内索引（0..2——账面下标）。
    pub const fn as_index(self) -> usize {
        match self {
            BhTriggerPoint::IrqReturn => 0,
            BhTriggerPoint::SchedPoint => 1,
            BhTriggerPoint::Explicit => 2,
        }
    }

    /// 索引反查（闭集外的下标返回 None——查询面不产诊断码，全查
    /// 面（0..2 之外的取用）由调用方决定是否立案）。
    pub fn from_index(idx: u8) -> Option<BhTriggerPoint> {
        match idx {
            0 => Some(BhTriggerPoint::IrqReturn),
            1 => Some(BhTriggerPoint::SchedPoint),
            2 => Some(BhTriggerPoint::Explicit),
            _ => None,
        }
    }

    /// 每轮服务槽位预算（const——语义表冻结面之一）。
    pub const fn budget(self) -> u8 {
        match self {
            BhTriggerPoint::IrqReturn => BUDGET_IRQ_RETURN,
            BhTriggerPoint::SchedPoint => BUDGET_SCHED_POINT,
            BhTriggerPoint::Explicit => BUDGET_EXPLICIT,
        }
    }

    /// 是否同核即时语义（中断返回前独有——const，语义表冻结面之二）。
    pub const fn is_same_core_immediate(self) -> bool {
        match self {
            BhTriggerPoint::IrqReturn => true,
            BhTriggerPoint::SchedPoint => false,
            BhTriggerPoint::Explicit => false,
        }
    }

    /// 语义注记（冻结面之三——入契约指纹，改字即漂移）。
    pub fn semantics_note(self) -> &'static str {
        match self {
            BhTriggerPoint::IrqReturn => "中断返回前:同核即时:预算8:快速返回纪律",
            BhTriggerPoint::SchedPoint => "调度点:让位后:预算4:让位纪律优先",
            BhTriggerPoint::Explicit => "显式触发:调用方驱动:预算32:全扫排空",
        }
    }
}

// ---------------------------------------------------------------------------
// §3 确定性源：LCG + FNV-1a（零墙钟零地址随机）
// ---------------------------------------------------------------------------

/// LCG 伪随机（Numerical Recipes 参数——确定性仿真唯一随机源）。
#[derive(Clone, Copy, Debug)]
pub struct Lcg {
    /// 内部状态。
    state: u64,
}

impl Lcg {
    /// 乘子（Numerical Recipes 64 位参数）。
    const LCG_A: u64 = 6_364_136_223_846_793_005;
    /// 增量（Numerical Recipes 64 位参数）。
    const LCG_C: u64 = 1_442_695_040_888_963_407;

    /// 播种。
    pub const fn new(seed: u64) -> Lcg {
        Lcg {
            state: seed.wrapping_add(0x9E37_79B9_7F4A_7C15),
        }
    }

    /// 下一步原始 64 位。
    pub fn next_u64(&mut self) -> u64 {
        self.state = self
            .state
            .wrapping_mul(Lcg::LCG_A)
            .wrapping_add(Lcg::LCG_C);
        self.state
    }

    /// 下一步 32 位（高位折叠——低位线性度差不用）。
    pub fn next_u32(&mut self) -> u32 {
        (self.next_u64() >> 33) as u32
    }
}

/// FNV-1a 64 位偏移基（标准参数）。
pub const FNV64_OFFSET: u64 = 0xCBF2_9CE4_8422_2325;
/// FNV-1a 64 位素数（标准参数）。
pub const FNV64_PRIME: u64 = 0x0000_0100_0000_01B3;

/// FNV-1a 64 位指纹（字节序敏感——快照往返唯一通道）。
pub fn fnv64(buf: &[u8]) -> u64 {
    let mut h: u64 = FNV64_OFFSET;
    let mut i: usize = 0;
    while i < buf.len() {
        h ^= buf[i] as u64;
        h = h.wrapping_mul(FNV64_PRIME);
        i += 1;
    }
    h
}

// ---------------------------------------------------------------------------
// §4 槽位注册与优先级（次序纪律的排序键）
// ---------------------------------------------------------------------------

/// 槽位优先级档（0 最高——数值小者先服务；排序键 = (优先级, 槽位号)）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PrioClass {
    /// 0..=15 紧急（时钟/喂狗类——中断返回前也要尽快带走）。
    Urgent,
    /// 16..=63 高（网络收包后处理类）。
    High,
    /// 64..=159 普通（块层完成回调类）。
    Normal,
    /// 160..=255 闲（统计/账面整理类——饿死容忍）。
    Idle,
}

impl PrioClass {
    /// 优先级值归类（const——纯 match，无 slice/循环）。
    pub const fn of(prio: u8) -> PrioClass {
        match prio {
            0..=15 => PrioClass::Urgent,
            16..=63 => PrioClass::High,
            64..=159 => PrioClass::Normal,
            _ => PrioClass::Idle,
        }
    }

    /// 档名（账面/报告用）。
    pub fn name(self) -> &'static str {
        match self {
            PrioClass::Urgent => "紧急",
            PrioClass::High => "高",
            PrioClass::Normal => "普通",
            PrioClass::Idle => "闲",
        }
    }
}

/// 槽位注册摘要（报告面——不入引擎）。
#[derive(Clone, Debug)]
pub struct SlotRegInfo {
    /// 核号。
    pub core: u8,
    /// 槽位号。
    pub slot: u8,
    /// 优先级（0 最高）。
    pub priority: u8,
    /// 处理器标签（top half 投递者身份，0x0944_0000 | slot 为默认约定）。
    pub tag: u32,
    /// 优先级档。
    pub class: PrioClass,
}

// ---------------------------------------------------------------------------
// §5 事件账（256 槽环形——seq 有序、溢出饱和记账）
// ---------------------------------------------------------------------------

/// 事件种类闭集（账面十一类——事件账只记面级事实不记内容）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EventKind {
    /// 槽位注册。
    SlotRegistered,
    /// 置位（位由 0 变 1——有效置位）。
    SlotSet,
    /// 置位合并（位已为 1 再置——幂等，F0951 合并账面地基）。
    SlotMerged,
    /// 服务完成（触发点处理一槽）。
    SlotServiced,
    /// 推迟（处理中置位——本轮不服务，下轮必达）。
    SlotDeferred,
    /// 清位未服务（显式清位/丢失清除——丢失账面来源）。
    SlotClearedUnserviced,
    /// 触发进入（一轮触发开始——快照已定格）。
    FireEnter,
    /// 触发退出（一轮触发结束——剩余未决已落账）。
    FireExit,
    /// 丢失检出（扫描面立案）。
    LossFound,
    /// 误置立案（跨核误置/无链跨发被拒）。
    MissetFlagged,
    /// 重入守卫（处理中置位被推迟语义拦下）。
    ReentryGuarded,
}

impl EventKind {
    /// 全闭集（11 类——事件账枚举锚）。
    pub const fn all() -> [EventKind; 11] {
        [
            EventKind::SlotRegistered,
            EventKind::SlotSet,
            EventKind::SlotMerged,
            EventKind::SlotServiced,
            EventKind::SlotDeferred,
            EventKind::SlotClearedUnserviced,
            EventKind::FireEnter,
            EventKind::FireExit,
            EventKind::LossFound,
            EventKind::MissetFlagged,
            EventKind::ReentryGuarded,
        ]
    }

    /// 账面短名。
    pub fn name(self) -> &'static str {
        match self {
            EventKind::SlotRegistered => "注册",
            EventKind::SlotSet => "置位",
            EventKind::SlotMerged => "合并",
            EventKind::SlotServiced => "服务",
            EventKind::SlotDeferred => "推迟",
            EventKind::SlotClearedUnserviced => "清位未服务",
            EventKind::FireEnter => "触发进入",
            EventKind::FireExit => "触发退出",
            EventKind::LossFound => "丢失立案",
            EventKind::MissetFlagged => "误置立案",
            EventKind::ReentryGuarded => "重入守卫",
        }
    }
}

/// 单条事件（16 字节内——环形槽开销可控）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BhEvent {
    /// 全账单调序号（回卷显性）。
    pub seq: u32,
    /// 事件 tick。
    pub tick: u64,
    /// 事件种类。
    pub kind: EventKind,
    /// 核号。
    pub core: u8,
    /// 槽位号（SLOT_NONE = 面级事件）。
    pub slot: u8,
    /// 触发点段内下标（POINT_NONE = 与触发点无关）。
    pub point: u8,
}

impl BhEvent {
    /// 事件指纹字节流（seq 除外——seq 依赖注入顺序，双跑内一致故
    /// 指纹不含 seq，含 tick/kind/core/slot/point 五元组）。
    fn body_bytes(self) -> [u8; 13] {
        let mut b = [0u8; 13];
        b[0..8].copy_from_slice(&self.tick.to_le_bytes());
        b[8] = self.kind as u8;
        b[9] = self.core;
        b[10] = self.slot;
        b[11] = self.point;
        b[12] = 0;
        b
    }
}

/// 环形事件账。
#[derive(Clone, Debug)]
pub struct EventLedger {
    /// 环槽。
    ring: Vec<BhEvent>,
    /// 写游标（覆盖位）。
    cursor: usize,
    /// 累计入账数（回卷显性）。
    pushed_total: u64,
    /// 被覆盖的事件数（饱和——溢出立案证据）。
    overflow_dropped: u64,
}

impl EventLedger {
    /// 新账（容量 = LEDGER_CAP）。
    pub fn new() -> EventLedger {
        EventLedger {
            ring: Vec::new(),
            cursor: 0,
            pushed_total: 0,
            overflow_dropped: 0,
        }
    }

    /// 入账（满则覆盖最旧——饱和记账，覆盖数显性累计）。
    pub fn push(&mut self, mut ev: BhEvent) {
        self.pushed_total = self.pushed_total.wrapping_add(1);
        ev.seq = self.pushed_total.wrapping_sub(1) as u32;
        if self.ring.len() < LEDGER_CAP {
            self.ring.push(ev);
        } else {
            self.ring[self.cursor] = ev;
            self.cursor += 1;
            if self.cursor >= LEDGER_CAP {
                self.cursor = 0;
            }
            self.overflow_dropped = self.overflow_dropped.saturating_add(1);
        }
    }

    /// 时序序读取（最旧在前——溢出时从覆盖游标起）。
    pub fn iter_ordered(&self) -> Vec<BhEvent> {
        if self.ring.len() < LEDGER_CAP {
            return self.ring.clone();
        }
        let mut out: Vec<BhEvent> = Vec::with_capacity(LEDGER_CAP);
        let mut i = self.cursor;
        let mut n = 0usize;
        while n < LEDGER_CAP {
            out.push(self.ring[i]);
            i += 1;
            if i >= LEDGER_CAP {
                i = 0;
            }
            n += 1;
        }
        out
    }

    /// 被覆盖数。
    pub fn overflow_dropped(&self) -> u64 {
        self.overflow_dropped
    }

    /// 累计入账数。
    pub fn pushed_total(&self) -> u64 {
        self.pushed_total
    }

    /// 当前在账数。
    pub fn resident(&self) -> usize {
        self.ring.len()
    }

    /// 尾部 n 条（指纹用——取时序序尾部）。
    pub fn tail_bytes(&self, n: usize) -> Vec<u8> {
        let ordered = self.iter_ordered();
        let start = ordered.len().saturating_sub(n);
        let mut out: Vec<u8> = Vec::new();
        let mut i = start;
        while i < ordered.len() {
            out.extend_from_slice(&ordered[i].body_bytes());
            i += 1;
        }
        out
    }
}

// ---------------------------------------------------------------------------
// §6 pending 置位面（32 槽位图 + 每槽六账 + 账平恒等式）
// ---------------------------------------------------------------------------

/// 置位结果（幂等账面）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RaiseOutcome {
    /// 位由 0 变 1——有效置位（入账 `有效置位数`）。
    NewlySet,
    /// 位已为 1——合并（入账 `合并数`，不重复记账）。
    CoalescedMerged,
}

/// pending 置位面：32 槽单字位图 + 每槽六账。
///
/// **账平恒等式**（判据核心）：对每槽位 i 与全局，
/// `有效置位[i] == 服务[i] + 未决位(i) + 清位丢失[i]`。
/// 置位-触发-清位三段账的每一笔都只走本面的账面方法，恒等式即
/// 结构性成立；任何直改位图路径都会打平账——这正是审计要抓的。
#[derive(Clone, Debug)]
pub struct PendingBitmap {
    /// 已注册槽位掩码（账面黑户防线）。
    live_mask: u32,
    /// 未决掩码（pending 置位面本体）。
    pending_mask: u32,
    /// 有效置位数（位由 0 变 1 才计——幂等合并另账）。
    effective_sets: [u32; MAX_SLOTS],
    /// 触发处理数（服务完成数）。
    serviced: [u32; MAX_SLOTS],
    /// 清位丢失数（未服务即被清——丢失归因「位图被清」的证据账）。
    lost: [u32; MAX_SLOTS],
    /// 合并数（已置位再置位——F0951 合并账面地基）。
    merged: [u32; MAX_SLOTS],
    /// 末次置位 tick（丢失超龄判定基准）。
    last_set_tick: [u64; MAX_SLOTS],
    /// 置位后触发点经过数（丢失归因「触发点未跑」的证据账——置位
    /// 时清零，每轮触发对未决槽 +1）。
    fires_since_set: [u32; MAX_SLOTS],
    /// 优先级（0 最高——次序纪律排序键）。
    priorities: [u8; MAX_SLOTS],
    /// 处理器标签（top half 投递者身份）。
    tags: [u32; MAX_SLOTS],
}

impl PendingBitmap {
    /// 空面。
    pub fn new() -> PendingBitmap {
        PendingBitmap {
            live_mask: 0,
            pending_mask: 0,
            effective_sets: [0u32; MAX_SLOTS],
            serviced: [0u32; MAX_SLOTS],
            lost: [0u32; MAX_SLOTS],
            merged: [0u32; MAX_SLOTS],
            last_set_tick: [0u64; MAX_SLOTS],
            fires_since_set: [0u32; MAX_SLOTS],
            priorities: [0u8; MAX_SLOTS],
            tags: [0u32; MAX_SLOTS],
        }
    }

    /// 槽位号合法性与注册校验（共享前置检查）。
    fn check_slot(&self, slot: u8) -> Result<usize, BhTrigError> {
        if slot as usize >= MAX_SLOTS {
            return Err(BhTrigError::SlotOutOfRange {
                core: 0,
                slot,
            });
        }
        if self.live_mask & (1u32 << slot) == 0 {
            return Err(BhTrigError::SlotNotRegistered { core: 0, slot });
        }
        Ok(slot as usize)
    }

    /// 注册槽位（幂等——重注册覆盖优先级与标签，账面不重复）。
    pub fn register(
        &mut self,
        slot: u8,
        priority: u8,
        tag: u32,
    ) -> Result<(), BhTrigError> {
        if slot as usize >= MAX_SLOTS {
            return Err(BhTrigError::SlotOutOfRange { core: 0, slot });
        }
        let i = slot as usize;
        self.live_mask |= 1u32 << i;
        self.priorities[i] = priority;
        self.tags[i] = tag;
        Ok(())
    }

    /// 置位（幂等：已置位再置位记合并不重复记账——F0951 地基）。
    pub fn raise(
        &mut self,
        slot: u8,
        tick: u64,
    ) -> Result<RaiseOutcome, BhTrigError> {
        let i = self.check_slot(slot)?;
        if self.pending_mask & (1u32 << i) != 0 {
            self.merged[i] = self.merged[i].wrapping_add(1);
            return Ok(RaiseOutcome::CoalescedMerged);
        }
        self.pending_mask |= 1u32 << i;
        self.effective_sets[i] = self.effective_sets[i].wrapping_add(1);
        self.last_set_tick[i] = tick;
        self.fires_since_set[i] = 0;
        Ok(RaiseOutcome::NewlySet)
    }

    /// 清位（未服务路径——丢失账面唯一入口；返回是否确有未决被清）。
    pub fn clear_without_service(&mut self, slot: u8) -> Result<bool, BhTrigError> {
        let i = self.check_slot(slot)?;
        if self.pending_mask & (1u32 << i) == 0 {
            return Ok(false);
        }
        self.pending_mask &= !(1u32 << i);
        self.lost[i] = self.lost[i].wrapping_add(1);
        Ok(true)
    }

    /// 服务一槽（触发路径清位—— serviced 账入口；未决位必须在）。
    pub fn service(&mut self, slot: u8, tick: u64) -> Result<(), BhTrigError> {
        let i = self.check_slot(slot)?;
        if self.pending_mask & (1u32 << i) == 0 {
            // 防御：非未决槽进入服务面——按黑户逻辑立未注册案不合适，
            // 这里的语义是「账面幽灵服务」，直接拒绝。
            return Err(BhTrigError::SlotNotRegistered { core: 0, slot });
        }
        let _ = tick; // 服务 tick 由事件账承载，位面账不重复记
        self.pending_mask &= !(1u32 << i);
        self.serviced[i] = self.serviced[i].wrapping_add(1);
        Ok(())
    }

    /// 触发点经过记账：每轮触发结束对本面全部未决槽 +1（丢失归因
    /// 「触发点未跑」的判据来源——置位后经过数为 0 即触发点没跑过）。
    pub fn note_fired(&mut self) {
        let mut m = self.pending_mask;
        while m != 0 {
            let b = m.trailing_zeros() as usize;
            self.fires_since_set[b] = self.fires_since_set[b].wrapping_add(1);
            m &= m.wrapping_sub(1);
        }
    }

    /// 未决掩码。
    pub fn pending_mask(&self) -> u32 {
        self.pending_mask
    }

    /// 注册掩码。
    pub fn live_mask(&self) -> u32 {
        self.live_mask
    }

    /// 槽位是否未决。
    pub fn is_pending(&self, slot: u8) -> bool {
        slot < MAX_SLOTS as u8 && self.pending_mask & (1u32 << slot) != 0
    }

    /// 槽位是否已注册。
    pub fn is_live(&self, slot: u8) -> bool {
        slot < MAX_SLOTS as u8 && self.live_mask & (1u32 << slot) != 0
    }

    /// 槽位优先级（前置条件：槽位已注册——0..MAX_SLOTS 之外取 0xFF）。
    pub fn priority_of(&self, slot: u8) -> u8 {
        if slot as usize >= MAX_SLOTS {
            0xFF
        } else {
            self.priorities[slot as usize]
        }
    }

    /// 槽位末次置位 tick（未注册/未置位取 0）。
    pub fn last_set_tick_of(&self, slot: u8) -> u64 {
        if slot as usize >= MAX_SLOTS {
            0
        } else {
            self.last_set_tick[slot as usize]
        }
    }

    /// 槽位置位后触发经过数。
    pub fn fires_since_set_of(&self, slot: u8) -> u32 {
        if slot as usize >= MAX_SLOTS {
            0
        } else {
            self.fires_since_set[slot as usize]
        }
    }

    /// 槽位清位丢失数。
    pub fn lost_of(&self, slot: u8) -> u32 {
        if slot as usize >= MAX_SLOTS {
            0
        } else {
            self.lost[slot as usize]
        }
    }

    /// 槽位合并数。
    pub fn merged_of(&self, slot: u8) -> u32 {
        if slot as usize >= MAX_SLOTS {
            0
        } else {
            self.merged[slot as usize]
        }
    }

    /// 槽位服务数。
    pub fn serviced_of(&self, slot: u8) -> u32 {
        if slot as usize >= MAX_SLOTS {
            0
        } else {
            self.serviced[slot as usize]
        }
    }

    /// 槽位有效置位数。
    pub fn effective_sets_of(&self, slot: u8) -> u32 {
        if slot as usize >= MAX_SLOTS {
            0
        } else {
            self.effective_sets[slot as usize]
        }
    }

    /// 全局四账合计（while 逐槽累加——显性无迭代器魔法）：
    /// (有效置位, 服务, 未决槽位数, 清位丢失)。
    pub fn totals(&self) -> (u32, u32, u32, u32) {
        let mut sets = 0u32;
        let mut srv = 0u32;
        let mut out = 0u32;
        let mut lost = 0u32;
        let mut i = 0usize;
        while i < MAX_SLOTS {
            sets = sets.wrapping_add(self.effective_sets[i]);
            srv = srv.wrapping_add(self.serviced[i]);
            if self.pending_mask & (1u32 << i) != 0 {
                out = out.wrapping_add(1);
            }
            lost = lost.wrapping_add(self.lost[i]);
            i += 1;
        }
        (sets, srv, out, lost)
    }

    /// 单槽四账：(有效置位, 服务, 未决(0/1), 清位丢失)。
    pub fn per_slot_ledger(&self, slot: u8) -> (u32, u32, u32, u32) {
        if slot as usize >= MAX_SLOTS {
            return (0, 0, 0, 0);
        }
        let i = slot as usize;
        let out = if self.pending_mask & (1u32 << i) != 0 {
            1u32
        } else {
            0u32
        };
        (
            self.effective_sets[i],
            self.serviced[i],
            out,
            self.lost[i],
        )
    }

    /// 单槽账平判据（恒等式逐槽版）。
    pub fn slot_balance_ok(&self, slot: u8) -> bool {
        let (s, v, o, l) = self.per_slot_ledger(slot);
        s == v.wrapping_add(o).wrapping_add(l)
    }

    /// 全局账平判据（恒等式全局版——逐槽合计必同平）。
    pub fn balance_ok(&self) -> bool {
        let (s, v, o, l) = self.totals();
        s == v.wrapping_add(o).wrapping_add(l)
    }
}

// ---------------------------------------------------------------------------
// §7 per 核隔离面（每核独立 pending 面 + 独立触发账 + IPI 链表）
// ---------------------------------------------------------------------------

/// 单核 pending 面：位图 + 分触发点账 + 扫描快照基线。
#[derive(Clone, Debug)]
pub struct CorePlane {
    /// 核号。
    core: u8,
    /// pending 置位面（32 槽）。
    bitmap: PendingBitmap,
    /// 分触发点触发轮数（回卷显性）。
    fires_by_point: [u32; 3],
    /// 分触发点服务槽数（回卷显性）。
    services_by_point: [u32; 3],
    /// 分触发点末次触发 tick。
    last_fire_tick: [u64; 3],
    /// 上次丢失扫描时的清位丢失快照（「位图被清」归因的增量基准）。
    last_scan_lost: [u32; MAX_SLOTS],
}

impl CorePlane {
    /// 空面。
    pub fn new(core: u8) -> CorePlane {
        CorePlane {
            core,
            bitmap: PendingBitmap::new(),
            fires_by_point: [0u32; 3],
            services_by_point: [0u32; 3],
            last_fire_tick: [0u64; 3],
            last_scan_lost: [0u32; MAX_SLOTS],
        }
    }

    /// 核号。
    pub fn core(&self) -> u8 {
        self.core
    }

    /// 位图只读借用。
    pub fn bitmap(&self) -> &PendingBitmap {
        &self.bitmap
    }

    /// 位图可变借用（测试/注入面——账面方法仍是唯一写路径）。
    pub fn bitmap_mut(&mut self) -> &mut PendingBitmap {
        &mut self.bitmap
    }

    /// 分触发点触发轮数。
    pub fn fires_by_point(&self, point: BhTriggerPoint) -> u32 {
        self.fires_by_point[point.as_index()]
    }

    /// 分触发点服务槽数。
    pub fn services_by_point(&self, point: BhTriggerPoint) -> u32 {
        self.services_by_point[point.as_index()]
    }
}

// ---------------------------------------------------------------------------
// §8 触发报告面
// ---------------------------------------------------------------------------

/// 一轮触发的报告（次序纪律/推迟语义/预算余账全在册）。
#[derive(Clone, Debug)]
pub struct FireReport {
    /// 触发点。
    pub point: BhTriggerPoint,
    /// 核号。
    pub core: u8,
    /// 进入时快照（未决掩码定格——处理中新置位不进本轮）。
    pub snapshot_mask: u32,
    /// 本轮服务序（（优先级,槽位号）增序——次序纪律产物）。
    pub serviced: Vec<u8>,
    /// 本轮推迟槽（处理中置位——下轮必达）。
    pub deferred: Vec<u8>,
    /// 本轮预算。
    pub budget: u8,
    /// 触发退出后仍未决的掩码（含快照余量与推迟槽）。
    pub remained_pending: u32,
    /// 触发 tick。
    pub tick: u64,
}

/// 丢失归因闭集（判据二分 + 预算饿死深化档）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LossCause {
    /// 触发点未跑（置位后触发经过数为 0——三触发点一次都没轮到它）。
    TriggerNotRun,
    /// 位图被清（清位丢失增量——未服务即被清位）。
    BitmapCleared,
    /// 预算饿死（触发点跑了但预算没轮到——深化档，归因二分之外
    /// 的可观测第三态，联签 F0950 停滞因果账）。
    BudgetStarved,
}

impl LossCause {
    /// 归因名。
    pub fn name(self) -> &'static str {
        match self {
            LossCause::TriggerNotRun => "触发点未跑",
            LossCause::BitmapCleared => "位图被清",
            LossCause::BudgetStarved => "预算饿死",
        }
    }
}

/// 单条丢失归因。
#[derive(Clone, Debug)]
pub struct LossAttribution {
    /// 槽位号。
    pub slot: u8,
    /// 归因。
    pub cause: LossCause,
    /// 证据（tick 龄/经过数/丢失增量——账面原文）。
    pub evidence: String,
}

/// 丢失扫描报告。
#[derive(Clone, Debug)]
pub struct LossScanReport {
    /// 扫描 tick。
    pub tick: u64,
    /// 核号。
    pub core: u8,
    /// 超龄未决归因集。
    pub aged: Vec<LossAttribution>,
    /// 清位丢失归因集。
    pub cleared: Vec<LossAttribution>,
    /// 本轮检出总数。
    pub total_found: u32,
}

/// 注入演练报告（丢失/重入/跨核误置共用——判据「各 10/10 次」载体）。
#[derive(Clone, Debug)]
pub struct InjectionDrillReport {
    /// 注入类名。
    pub kind: &'static str,
    /// 总注入次数。
    pub total: u32,
    /// 检出次数。
    pub detected: u32,
    /// 全检出判定。
    pub all_detected: bool,
}

/// 三触发点演练报告（判据「各 10/10 次」载体）。
#[derive(Clone, Debug)]
pub struct TriggerPointDrillReport {
    /// 每点通过数（下标 = BhTriggerPoint::as_index）。
    pub passed: [u32; 3],
    /// 每点总数（恒 10）。
    pub total: [u32; 3],
}

impl TriggerPointDrillReport {
    /// 全通过判定（3 点各 10/10）。
    pub fn all_passed(&self) -> bool {
        let mut i = 0usize;
        while i < 3 {
            if self.passed[i] != self.total[i] {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 摘要。
    pub fn summary(&self) -> String {
        format!(
            "中断返回前 {}/{}；调度点 {}/{}；显式触发 {}/{}",
            self.passed[0], self.total[0], self.passed[1], self.total[1],
            self.passed[2], self.total[2]
        )
    }
}

/// 账平审计报告（判据「账平 10 万次」载体）。
#[derive(Clone, Debug)]
pub struct BalanceReport {
    /// 混合流操作数。
    pub ops: u32,
    /// 覆盖核数。
    pub core_count: u32,
    /// 全局有效置位数。
    pub sets: u32,
    /// 全局触发处理数。
    pub serviced: u32,
    /// 审计末未决数。
    pub outstanding: u32,
    /// 清位丢失数。
    pub lost: u32,
    /// 合并数（幂等账面）。
    pub merged: u32,
    /// 逐槽核对槽数。
    pub slots_checked: u32,
    /// 全平判定。
    pub balanced: bool,
}

/// 双跑对拍报告。
#[derive(Clone, Debug)]
pub struct DualRunReport {
    /// 混合流操作数。
    pub ops: u32,
    /// 种子。
    pub seed: u64,
    /// A 跑指纹。
    pub digest_a: u64,
    /// B 跑指纹。
    pub digest_b: u64,
    /// A 跑全局五账（置位/服务/未决/丢失/合并）。
    pub totals_a: (u32, u32, u32, u32, u32),
    /// B 跑全局五账（置位/服务/未决/丢失/合并）。
    pub totals_b: (u32, u32, u32, u32, u32),
}

/// 健康三档。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HealthVerdict {
    /// 绿：账平、无误置、无丢失、无溢出、无未决推迟。
    Green,
    /// 观察档：事件账溢出在册或推迟在望——不阻塞但需关注。
    Watch,
    /// 红：误置/丢失/账破任一在册——人工介入。
    Red,
}

impl HealthVerdict {
    /// 档名。
    pub fn name(self) -> &'static str {
        match self {
            HealthVerdict::Green => "绿",
            HealthVerdict::Watch => "观察",
            HealthVerdict::Red => "红",
        }
    }
}

/// 健康报告。
#[derive(Clone, Debug)]
pub struct HealthReport {
    /// 裁决。
    pub verdict: HealthVerdict,
    /// 在册问题诊断码集合。
    pub issue_codes: Vec<u16>,
    /// 全局面平判定。
    pub balance_ok: bool,
    /// 误置立案数。
    pub misset_flagged: u32,
    /// 重入守卫数（守卫成功——推迟语义工作正常，非问题）。
    pub reentry_guarded: u32,
    /// 丢失立案数。
    pub loss_events_total: u32,
    /// 事件账覆盖数。
    pub ledger_overflow: u64,
    /// 在望推迟槽位数。
    pub deferred_watch: u32,
    /// 摘要。
    pub summary: String,
}

// ---------------------------------------------------------------------------
// §9 引擎本体（触发面唯一入口——三触发点 + per 核隔离 + 注入面）
// ---------------------------------------------------------------------------

/// bottom half 触发引擎：per 核隔离面 + 三触发点 + 事件账。
///
/// 尺寸红线：引擎整体必须在 64KB 内（结构体禁上栈红线——const 编译
/// 期断言兜底，超限直接编译失败而非运行期爆栈）。
#[derive(Clone, Debug)]
pub struct BhTrigEngine {
    /// 本引擎绑定的核（模拟 CPU 亲和——本地置位只许打本面）。
    bound_core: u8,
    /// per 核独立 pending 面。
    planes: [CorePlane; MAX_CORES],
    /// IPI 链表：`ipi_linked[src][dst]`（跨核置位的通知链凭据）。
    ipi_linked: [[bool; MAX_CORES]; MAX_CORES],
    /// 事件账（256 槽环）。
    ledger: EventLedger,
    /// tick 单调地板。
    tick_floor: u64,
    /// 处理中置位钩子（注入面专用——真实路径无回调零重入）。
    midfire_raise: Option<u8>,
    /// 在望推迟掩码（上轮推迟槽——本轮必达，缺席即 DeferredMiss 面）。
    deferred_watch: u32,
    /// 误置立案数（跨核误置/无链跨发被拒累计）。
    misset_flagged: u32,
    /// 重入守卫数（处理中置位被推迟语义拦下累计）。
    reentry_guarded: u32,
    /// 丢失立案数（扫描面检出累计）。
    loss_events_total: u32,
    /// 推迟累计（产生）。
    deferred_raised_total: u32,
    /// 推迟累计（达成——下轮被服务）。
    deferred_resolved_total: u32,
}

/// 引擎尺寸红线（编译期——>64KB 结构体禁上栈）。
const _: () = assert!(core::mem::size_of::<BhTrigEngine>() <= 65_536);

impl BhTrigEngine {
    /// 新引擎（绑定核——bound_core 是本地置位的唯一合法目标面）。
    pub fn new(bound_core: u8) -> Result<BhTrigEngine, BhTrigError> {
        if bound_core as usize >= MAX_CORES {
            return Err(BhTrigError::CoreOutOfRange { core: bound_core });
        }
        Ok(BhTrigEngine {
            bound_core,
            planes: [
                CorePlane::new(0),
                CorePlane::new(1),
                CorePlane::new(2),
                CorePlane::new(3),
            ],
            ipi_linked: [[false; MAX_CORES]; MAX_CORES],
            ledger: EventLedger::new(),
            tick_floor: 0,
            midfire_raise: None,
            deferred_watch: 0,
            misset_flagged: 0,
            reentry_guarded: 0,
            loss_events_total: 0,
            deferred_raised_total: 0,
            deferred_resolved_total: 0,
        })
    }

    /// 绑定核号。
    pub fn bound_core(&self) -> u8 {
        self.bound_core
    }

    /// tick 单调校验（共享前置——回退即拒）。
    fn check_tick(&mut self, tick: u64) -> Result<(), BhTrigError> {
        if tick < self.tick_floor {
            return Err(BhTrigError::TickRegress {
                tick,
                floor: self.tick_floor,
            });
        }
        self.tick_floor = tick;
        Ok(())
    }

    /// 事件入账（内部便捷——面级统一戳 seq）。
    fn log(
        &mut self,
        tick: u64,
        kind: EventKind,
        core: u8,
        slot: u8,
        point: u8,
    ) {
        self.ledger.push(BhEvent {
            seq: 0,
            tick,
            kind,
            core,
            slot,
            point,
        });
    }

    /// 核号合法性。
    fn check_core(core: u8) -> Result<usize, BhTrigError> {
        if core as usize >= MAX_CORES {
            return Err(BhTrigError::CoreOutOfRange { core });
        }
        Ok(core as usize)
    }

    /// 注册槽位（配置面——init 上下文可对任意合法核注册）。
    pub fn register_slot(
        &mut self,
        core: u8,
        slot: u8,
        priority: u8,
        tag: u32,
    ) -> Result<SlotRegInfo, BhTrigError> {
        let ci = Self::check_core(core)?;
        if slot as usize >= MAX_SLOTS {
            return Err(BhTrigError::SlotOutOfRange { core, slot });
        }
        self.planes[ci].bitmap.register(slot, priority, tag)?;
        let reg_tick = self.tick_floor;
        self.log(reg_tick, EventKind::SlotRegistered, core, slot, POINT_NONE);
        Ok(SlotRegInfo {
            core,
            slot,
            priority,
            tag,
            class: PrioClass::of(priority),
        })
    }

    /// 注册一核默认 32 槽（优先级铺满四档：slot*8 → 0..248；
    /// 标签 = 0x0944_0000 | slot——本单默认约定）。返回注册槽数。
    pub fn register_core_defaults(&mut self, core: u8) -> Result<u32, BhTrigError> {
        let ci = Self::check_core(core)?;
        let mut n = 0u32;
        let mut slot = 0u8;
        while (slot as usize) < MAX_SLOTS {
            self.planes[ci]
                .bitmap
                .register(slot, slot.wrapping_mul(8), 0x0944_0000 | slot as u32)?;
            n = n.wrapping_add(1);
            slot = slot.wrapping_add(1);
        }
        Ok(n)
    }

    /// 本地置位（top half BhPost 落点——只许打本核面）。
    pub fn raise_local(&mut self, slot: u8, tick: u64) -> Result<RaiseOutcome, BhTrigError> {
        self.check_tick(tick)?;
        let ci = self.bound_core as usize;
        if slot as usize >= MAX_SLOTS {
            return Err(BhTrigError::SlotOutOfRange {
                core: self.bound_core,
                slot,
            });
        }
        let outcome = self.planes[ci].bitmap.raise(slot, tick)?;
        match outcome {
            RaiseOutcome::NewlySet => {
                self.log(tick, EventKind::SlotSet, self.bound_core, slot, POINT_NONE);
            }
            RaiseOutcome::CoalescedMerged => {
                self.log(tick, EventKind::SlotMerged, self.bound_core, slot, POINT_NONE);
            }
        }
        Ok(outcome)
    }

    /// 建核间通知链（IPI 凭据——跨核置位前置）。
    pub fn link_cores(&mut self, src: u8, dst: u8) -> Result<(), BhTrigError> {
        let si = Self::check_core(src)?;
        let di = Self::check_core(dst)?;
        self.ipi_linked[si][di] = true;
        Ok(())
    }

    /// 跨核置位（核间通知通道——链在则达，链缺/源非本核即误置立案）。
    pub fn raise_cross(
        &mut self,
        src: u8,
        dst: u8,
        slot: u8,
        tick: u64,
    ) -> Result<RaiseOutcome, BhTrigError> {
        self.check_tick(tick)?;
        let si = Self::check_core(src)?;
        let di = Self::check_core(dst)?;
        if slot as usize >= MAX_SLOTS {
            return Err(BhTrigError::SlotOutOfRange { core: dst, slot });
        }
        // 通道一检测：发起核必须就是本核（你只能从你在的核发 IPI）。
        if src != self.bound_core {
            self.misset_flagged = self.misset_flagged.wrapping_add(1);
            self.log(tick, EventKind::MissetFlagged, dst, slot, POINT_NONE);
            return Err(BhTrigError::CrossCoreMisSet { src, dst, slot });
        }
        // 通道二检测：核间通知链必须已建立。
        if !self.ipi_linked[si][di] {
            self.misset_flagged = self.misset_flagged.wrapping_add(1);
            self.log(tick, EventKind::MissetFlagged, dst, slot, POINT_NONE);
            return Err(BhTrigError::IpiLinkMissing { src, dst });
        }
        let outcome = self.planes[di].bitmap.raise(slot, tick)?;
        match outcome {
            RaiseOutcome::NewlySet => {
                self.log(tick, EventKind::SlotSet, dst, slot, POINT_NONE);
            }
            RaiseOutcome::CoalescedMerged => {
                self.log(tick, EventKind::SlotMerged, dst, slot, POINT_NONE);
            }
        }
        Ok(outcome)
    }

    /// 显式清位（未服务路径——丢失账面入口；清理/撤单语义）。
    pub fn clear_without_service(&mut self, slot: u8, tick: u64) -> Result<bool, BhTrigError> {
        self.check_tick(tick)?;
        let ci = self.bound_core as usize;
        if slot as usize >= MAX_SLOTS {
            return Err(BhTrigError::SlotOutOfRange {
                core: self.bound_core,
                slot,
            });
        }
        let cleared = self.planes[ci].bitmap.clear_without_service(slot)?;
        if cleared {
            self.log(
                tick,
                EventKind::SlotClearedUnserviced,
                self.bound_core,
                slot,
                POINT_NONE,
            );
            // 被清的推迟槽从在望表撤下（清位即撤单，不算缺席）。
            self.deferred_watch &= !(1u32 << slot);
        }
        Ok(cleared)
    }

    // ---- 触发面：三触发点唯一入口 ----

    /// 触发一轮（三触发点共用入口——语义差异全在预算与调用时机，
    /// 账面纪律共用：快照定格 → 优先级序 → 预算内服务 → 推迟守卫）。
    pub fn fire(
        &mut self,
        point: BhTriggerPoint,
        tick: u64,
    ) -> Result<FireReport, BhTrigError> {
        self.check_tick(tick)?;
        let ci = self.bound_core as usize;
        let budget = point.budget();
        let pidx = point.as_index() as u8;
        // 快照定格：处理中新置位不进本轮（推迟语义的结构性地基）。
        let snapshot = self.planes[ci].bitmap.pending_mask();
        self.log(
            tick,
            EventKind::FireEnter,
            self.bound_core,
            SLOT_NONE,
            pidx,
        );

        // 候选收集 + （优先级, 槽位号）排序——次序纪律的排序面。
        let mut cands: Vec<(u8, u8)> = Vec::new();
        let mut m = snapshot;
        while m != 0 {
            let b = m.trailing_zeros() as u8;
            cands.push((self.planes[ci].bitmap.priority_of(b), b));
            m &= m.wrapping_sub(1);
        }
        cands.sort_unstable_by(|a, b| (a.0, a.1).cmp(&(b.0, b.1)));

        // 次序纪律自检（防御——任何绕过排序面的手工选槽路径在此红账）。
        let mut i = 1usize;
        while i < cands.len() {
            if (cands[i - 1].0, cands[i - 1].1) > (cands[i].0, cands[i].1) {
                return Err(BhTrigError::OrderViolation { core: self.bound_core });
            }
            i += 1;
        }

        let mut serviced: Vec<u8> = Vec::new();
        let mut deferred: Vec<u8> = Vec::new();
        let mut served = 0u8;
        let mut k = 0usize;
        while k < cands.len() && served < budget {
            let slot = cands[k].1;
            self.planes[ci].bitmap.service(slot, tick)?;
            self.log(
                tick,
                EventKind::SlotServiced,
                self.bound_core,
                slot,
                pidx,
            );
            self.planes[ci].services_by_point[point.as_index()] = self.planes[ci]
                .services_by_point[point.as_index()]
                .wrapping_add(1);
            serviced.push(slot);
            served = served.wrapping_add(1);
            // 处理中置位（注入钩子——真实路径无回调零重入）：推迟语义
            // 拦下，本轮不服务，下轮必达。
            if let Some(hook) = self.midfire_raise.take() {
                let outcome = self.planes[ci].bitmap.raise(hook, tick)?;
                self.reentry_guarded = self.reentry_guarded.wrapping_add(1);
                self.log(
                    tick,
                    EventKind::ReentryGuarded,
                    self.bound_core,
                    hook,
                    pidx,
                );
                if outcome == RaiseOutcome::NewlySet {
                    // 合并位本已置（位在触发前就在快照里）——非推迟案。
                    self.deferred_watch |= 1u32 << hook;
                    self.deferred_raised_total =
                        self.deferred_raised_total.wrapping_add(1);
                    self.log(
                        tick,
                        EventKind::SlotDeferred,
                        self.bound_core,
                        hook,
                        pidx,
                    );
                    deferred.push(hook);
                }
            }
            k += 1;
        }

        // 在望推迟核对：上轮推迟槽本轮必达（缺席即立案）。
        let mut w = self.deferred_watch;
        while w != 0 {
            let b = w.trailing_zeros() as u8;
            if serviced.contains(&b) {
                self.deferred_resolved_total =
                    self.deferred_resolved_total.wrapping_add(1);
                self.deferred_watch &= !(1u32 << b);
            } else if !self.planes[ci].bitmap.is_pending(b) {
                // 不在服务序、又不再未决、又未经显式清位撤表——表外消失，
                // 推迟缺席立案。
                self.deferred_watch &= !(1u32 << b);
                return Err(BhTrigError::DeferredMiss {
                    core: self.bound_core,
                    slot: b,
                });
            }
            w &= w.wrapping_sub(1);
        }

        // 触发账 + 未决槽经过记账（丢失归因「触发点未跑」的判据来源）。
        self.planes[ci].fires_by_point[point.as_index()] = self.planes[ci]
            .fires_by_point[point.as_index()]
            .wrapping_add(1);
        self.planes[ci].last_fire_tick[point.as_index()] = tick;
        self.planes[ci].bitmap.note_fired();
        self.log(
            tick,
            EventKind::FireExit,
            self.bound_core,
            SLOT_NONE,
            pidx,
        );
        let remained_pending = self.planes[ci].bitmap.pending_mask();

        // 重入违规防御检测：在望推迟槽若出现在本轮服务序——推迟语义破。
        let mut w2 = self.deferred_watch;
        while w2 != 0 {
            let b = w2.trailing_zeros() as u8;
            if serviced.contains(&b) {
                return Err(BhTrigError::ReentryViolation {
                    core: self.bound_core,
                    slot: b,
                });
            }
            w2 &= w2.wrapping_sub(1);
        }

        Ok(FireReport {
            point,
            core: self.bound_core,
            snapshot_mask: snapshot,
            serviced,
            deferred,
            budget,
            remained_pending,
            tick,
        })
    }

    /// 丢失扫描（判据「置位后永未触发」+ 归因二分 + 预算饿死深化档）。
    pub fn scan_losses(&mut self, tick: u64) -> Result<LossScanReport, BhTrigError> {
        self.check_tick(tick)?;
        let ci = self.bound_core as usize;
        let mut aged: Vec<LossAttribution> = Vec::new();
        let mut cleared: Vec<LossAttribution> = Vec::new();
        let mut slot = 0u8;
        while (slot as usize) < MAX_SLOTS {
            // 超龄未决扫描（滞留丢失——置位后长驱无触发）。
            if self.planes[ci].bitmap.is_pending(slot) {
                let set_t = self.planes[ci].bitmap.last_set_tick_of(slot);
                let age = tick.wrapping_sub(set_t);
                if age > LOSS_THRESHOLD_TICKS {
                    let fires = self.planes[ci].bitmap.fires_since_set_of(slot);
                    let cause = if fires == 0 {
                        LossCause::TriggerNotRun
                    } else {
                        LossCause::BudgetStarved
                    };
                    aged.push(LossAttribution {
                        slot,
                        cause,
                        evidence: format!(
                            "置位tick={} 账龄={} 置位后触发经过={}",
                            set_t, age, fires
                        ),
                    });
                }
            }
            // 清位丢失回溯扫描（位被清未服务——增量对快照基线）。
            let lost_now = self.planes[ci].bitmap.lost_of(slot);
            let base = self.planes[ci].last_scan_lost[slot as usize];
            let delta = lost_now.wrapping_sub(base);
            if delta > 0 {
                cleared.push(LossAttribution {
                    slot,
                    cause: LossCause::BitmapCleared,
                    evidence: format!(
                        "清位丢失增量={}（快照基={} 现值={}）",
                        delta, base, lost_now
                    ),
                });
            }
            slot = slot.wrapping_add(1);
        }
        // 快照基线推进（下轮只看增量）。
        let mut i = 0usize;
        while i < MAX_SLOTS {
            self.planes[ci].last_scan_lost[i] = self.planes[ci].bitmap.lost_of(i as u8);
            i += 1;
        }
        let found = (aged.len() as u32).saturating_add(cleared.len() as u32);
        let mut a = 0usize;
        while a < aged.len() {
            self.log(
                tick,
                EventKind::LossFound,
                self.bound_core,
                aged[a].slot,
                POINT_NONE,
            );
            a += 1;
        }
        let mut c = 0usize;
        while c < cleared.len() {
            self.log(
                tick,
                EventKind::LossFound,
                self.bound_core,
                cleared[c].slot,
                POINT_NONE,
            );
            c += 1;
        }
        self.loss_events_total = self.loss_events_total.saturating_add(found);
        Ok(LossScanReport {
            tick,
            core: self.bound_core,
            aged,
            cleared,
            total_found: found,
        })
    }

    /// 显式全扫排空（预算 32/轮——守护上限打满即 Err，联签 F0950）。
    pub fn drain_explicit(&mut self, tick: &mut u64) -> Result<u32, BhTrigError> {
        let mut rounds = 0u32;
        while self.planes[self.bound_core as usize].bitmap.pending_mask() != 0 {
            if rounds >= DRAIN_LIMIT {
                return Err(BhTrigError::DrainLimit {
                    pending: self.planes[self.bound_core as usize]
                        .bitmap
                        .pending_mask(),
                });
            }
            *tick = tick.wrapping_add(1);
            self.fire(BhTriggerPoint::Explicit, *tick)?;
            rounds = rounds.wrapping_add(1);
        }
        Ok(rounds)
    }

    // ---- 账面只读访问器 ----

    /// 全引擎五账合计（跨核聚合）：(有效置位, 服务, 未决, 清位丢失, 合并)。
    pub fn engine_totals(&self) -> (u32, u32, u32, u32, u32) {
        let mut sets = 0u32;
        let mut srv = 0u32;
        let mut out = 0u32;
        let mut lost = 0u32;
        let mut merged = 0u32;
        let mut ci = 0usize;
        while ci < MAX_CORES {
            let (s, v, o, l) = self.planes[ci].bitmap.totals();
            sets = sets.wrapping_add(s);
            srv = srv.wrapping_add(v);
            out = out.wrapping_add(o);
            lost = lost.wrapping_add(l);
            let mut slot = 0usize;
            while slot < MAX_SLOTS {
                merged = merged.wrapping_add(self.planes[ci].bitmap.merged_of(slot as u8));
                slot += 1;
            }
            ci += 1;
        }
        (sets, srv, out, lost, merged)
    }

    /// 全面账平判定（每核每槽恒等式）。
    pub fn engine_balance_ok(&self) -> bool {
        let mut ci = 0usize;
        while ci < MAX_CORES {
            if !self.planes[ci].bitmap.balance_ok() {
                return false;
            }
            let mut slot = 0usize;
            while slot < MAX_SLOTS {
                if !self.planes[ci].bitmap.slot_balance_ok(slot as u8) {
                    return false;
                }
                slot += 1;
            }
            ci += 1;
        }
        true
    }

    /// 全面账平审计（失衡即 Err——账不平必须红账不许静默）。
    pub fn audit_balance_all(&self) -> Result<BalanceReport, BhTrigError> {
        let (sets, srv, out, lost, merged) = self.engine_totals();
        let mut slots_checked = 0u32;
        let mut ci = 0usize;
        while ci < MAX_CORES {
            let mut slot = 0u8;
            while (slot as usize) < MAX_SLOTS {
                let (s, v, o, l) = self.planes[ci].bitmap.per_slot_ledger(slot);
                let lhs = v.wrapping_add(o).wrapping_add(l);
                if s != lhs {
                    return Err(BhTrigError::BalanceBroken {
                        core: ci as u8,
                        slot,
                        sets: s,
                        lhs,
                    });
                }
                slots_checked = slots_checked.wrapping_add(1);
                slot = slot.wrapping_add(1);
            }
            ci += 1;
        }
        let gset = sets;
        let glhs = srv.wrapping_add(out).wrapping_add(lost);
        if gset != glhs {
            return Err(BhTrigError::BalanceBroken {
                core: self.bound_core,
                slot: SLOT_NONE,
                sets: gset,
                lhs: glhs,
            });
        }
        Ok(BalanceReport {
            ops: 0, // 0 = 引擎面直接审计（无混合流前置）
            core_count: MAX_CORES as u32,
            sets,
            serviced: srv,
            outstanding: out,
            lost,
            merged,
            slots_checked,
            balanced: true,
        })
    }

    /// 误置立案数。
    pub fn misset_flagged(&self) -> u32 {
        self.misset_flagged
    }

    /// 重入守卫数。
    pub fn reentry_guarded(&self) -> u32 {
        self.reentry_guarded
    }

    /// 丢失立案数。
    pub fn loss_events_total(&self) -> u32 {
        self.loss_events_total
    }

    /// 推迟产生数。
    pub fn deferred_raised_total(&self) -> u32 {
        self.deferred_raised_total
    }

    /// 推迟达成数。
    pub fn deferred_resolved_total(&self) -> u32 {
        self.deferred_resolved_total
    }

    /// 在望推迟掩码。
    pub fn deferred_watch(&self) -> u32 {
        self.deferred_watch
    }

    /// 事件账只读借用。
    pub fn ledger(&self) -> &EventLedger {
        &self.ledger
    }

    /// 指定核面只读借用。
    pub fn plane(&self, core: u8) -> Result<&CorePlane, BhTrigError> {
        let ci = Self::check_core(core)?;
        Ok(&self.planes[ci])
    }

    /// 指定核面可变借用（测试/注入面）。
    pub fn plane_mut(&mut self, core: u8) -> Result<&mut CorePlane, BhTrigError> {
        let ci = Self::check_core(core)?;
        Ok(&mut self.planes[ci])
    }

    /// IPI 链状态查询。
    pub fn ipi_linked(&self, src: u8, dst: u8) -> Result<bool, BhTrigError> {
        let si = Self::check_core(src)?;
        let di = Self::check_core(dst)?;
        Ok(self.ipi_linked[si][di])
    }

    // ---- 健康与指纹 ----

    /// 健康裁决（三档——账平/误置/丢失/溢出/推迟五查）。
    pub fn health(&self) -> HealthReport {
        let mut issues: Vec<u16> = Vec::new();
        let balance_ok = self.engine_balance_ok();
        if !balance_ok {
            issues.push(CODE_BALANCE_BROKEN);
        }
        if self.misset_flagged > 0 {
            issues.push(CODE_CROSS_CORE_MISSET);
        }
        if self.loss_events_total > 0 {
            issues.push(CODE_LOSS_UNATTRIBUTED);
        }
        let overflow = self.ledger.overflow_dropped();
        if overflow > 0 {
            issues.push(CODE_LEDGER_OVERFLOW);
        }
        if self.deferred_watch != 0 {
            issues.push(CODE_DEFERRED_MISS);
        }
        let red = !balance_ok || self.misset_flagged > 0 || self.loss_events_total > 0;
        let verdict = if red {
            HealthVerdict::Red
        } else if overflow > 0 || self.deferred_watch != 0 {
            HealthVerdict::Watch
        } else {
            HealthVerdict::Green
        };
        let (sets, srv, out, lost, merged) = self.engine_totals();
        let summary = format!(
            "{}档：置位={} 服务={} 未决={} 丢失={} 合并={} 误置={} 守卫={} 溢出={} 在望推迟={}",
            verdict.name(),
            sets,
            srv,
            out,
            lost,
            merged,
            self.misset_flagged,
            self.reentry_guarded,
            overflow,
            self.deferred_watch.count_ones()
        );
        HealthReport {
            verdict,
            issue_codes: issues,
            balance_ok,
            misset_flagged: self.misset_flagged,
            reentry_guarded: self.reentry_guarded,
            loss_events_total: self.loss_events_total,
            ledger_overflow: overflow,
            deferred_watch: self.deferred_watch.count_ones(),
            summary,
        }
    }

    /// 健康红线断言（非绿即 Err——聚合自证的通过门）。
    pub fn require_green(&self) -> Result<(), BhTrigError> {
        let h = self.health();
        if h.verdict != HealthVerdict::Green {
            let mut codes = 0u32;
            let mut i = 0usize;
            while i < h.issue_codes.len() {
                codes |= 1u32 << (h.issue_codes[i].wrapping_sub(SEG_BASE) & 0x0F);
                i += 1;
            }
            return Err(BhTrigError::HealthRed { codes });
        }
        Ok(())
    }

    /// 引擎指纹（FNV-1a 64——全账面 + 分点账 + 注入账 + 事件账尾）。
    pub fn fingerprint(&self) -> u64 {
        let mut buf: Vec<u8> = Vec::new();
        buf.push(self.bound_core);
        let mut ci = 0usize;
        while ci < MAX_CORES {
            let p = &self.planes[ci];
            buf.extend_from_slice(&p.bitmap.live_mask().to_le_bytes());
            buf.extend_from_slice(&p.bitmap.pending_mask().to_le_bytes());
            let (s, v, o, l) = p.bitmap.totals();
            buf.extend_from_slice(&s.to_le_bytes());
            buf.extend_from_slice(&v.to_le_bytes());
            buf.extend_from_slice(&o.to_le_bytes());
            buf.extend_from_slice(&l.to_le_bytes());
            let mut j = 0usize;
            while j < 3 {
                buf.extend_from_slice(&p.fires_by_point[j].to_le_bytes());
                buf.extend_from_slice(&p.services_by_point[j].to_le_bytes());
                j += 1;
            }
            ci += 1;
        }
        buf.extend_from_slice(&self.misset_flagged.to_le_bytes());
        buf.extend_from_slice(&self.reentry_guarded.to_le_bytes());
        buf.extend_from_slice(&self.loss_events_total.to_le_bytes());
        buf.extend_from_slice(&self.deferred_raised_total.to_le_bytes());
        buf.extend_from_slice(&self.deferred_resolved_total.to_le_bytes());
        buf.extend_from_slice(&self.deferred_watch.to_le_bytes());
        buf.extend_from_slice(&self.ledger.pushed_total().to_le_bytes());
        buf.extend_from_slice(&self.ledger.tail_bytes(8));
        fnv64(&buf)
    }
}

/// 注册面日志的 tick 取值（注册属配置面，取引擎地板 tick——零墙钟
/// 纪律下注册时刻无独立时序）。
#[allow(dead_code)]
fn tick_floor_of(e: &BhTrigEngine) -> u64 {
    e.tick_floor
}

// ---------------------------------------------------------------------------
// §10 注入演练面（判据三件套：三触发点 10/10×3、丢失 10/10、
// 重入/跨核误置注入各检出——全部走公开账面路径，无特权直改）
// ---------------------------------------------------------------------------

impl BhTrigEngine {
    /// 三触发点演练：每点 10 次「置位 → 该点触发 → 本槽在服务序」
    /// 闭环（判据「中断返回前/调度点/显式触发各 10/10 次」载体）。
    /// 前置：目标面槽位已注册（演练自建默认面——幂等）。
    pub fn drill_trigger_points(&mut self) -> Result<TriggerPointDrillReport, BhTrigError> {
        let mut rep = TriggerPointDrillReport {
            passed: [0u32; 3],
            total: [10u32; 3],
        };
        let bound = self.bound_core;
        self.register_core_defaults(bound)?;
        let mut tick = self.tick_floor.wrapping_add(1);
        let mut pi = 0usize;
        while pi < 3 {
            let point = match BhTriggerPoint::from_index(pi as u8) {
                Some(p) => p,
                None => return Err(BhTrigError::CoreOutOfRange { core: pi as u8 }),
            };
            let mut k = 0u32;
            while k < rep.total[pi] {
                // 槽位分段：点 0 用 0..9、点 1 用 10..19、点 2 用 20..29。
                let slot = (pi * 10 + k as usize) as u8;
                if !self.plane(bound)?.bitmap().is_live(slot) {
                    return Err(BhTrigError::SlotNotRegistered { core: bound, slot });
                }
                self.raise_local(slot, tick)?;
                tick = tick.wrapping_add(1);
                let fr = self.fire(point, tick)?;
                if fr.serviced.contains(&slot) {
                    rep.passed[pi] = rep.passed[pi].wrapping_add(1);
                }
                tick = tick.wrapping_add(1);
                k += 1;
            }
            pi += 1;
        }
        Ok(rep)
    }

    /// 重入注入演练：处理中置位必须被推迟语义拦下（本轮不服务）且
    /// 下轮必达（判据推迟语义 + F0951「快速触发合并」账面地基）。
    pub fn drill_reentry_injection(
        &mut self,
        times: u32,
    ) -> Result<InjectionDrillReport, BhTrigError> {
        let bound = self.bound_core;
        self.register_core_defaults(bound)?;
        let mut detected = 0u32;
        let mut tick = self.tick_floor.wrapping_add(1);
        let mut t = 0u32;
        while t < times {
            // 首槽与钩槽分段不相交（0..15 / 16..31）——钩槽必非快照位。
            let slot = (t % 16) as u8;
            let hook = slot + 16;
            self.raise_local(slot, tick)?;
            tick = tick.wrapping_add(1);
            // 挂处理中置位钩子（注入面专用——真实路径无回调零重入）。
            self.midfire_raise = Some(hook);
            let fr = self.fire(BhTriggerPoint::IrqReturn, tick)?;
            tick = tick.wrapping_add(1);
            // 判定一：钩槽未被本轮服务、已记推迟、位仍在未决。
            let guarded = !fr.serviced.contains(&hook)
                && fr.deferred.contains(&hook)
                && self.plane(bound)?.bitmap().is_pending(hook);
            // 判定二：下一轮必达。
            let fr2 = self.fire(BhTriggerPoint::IrqReturn, tick)?;
            tick = tick.wrapping_add(1);
            let arrived = fr2.serviced.contains(&hook);
            if guarded && arrived {
                detected = detected.wrapping_add(1);
            }
            t = t.wrapping_add(1);
        }
        Ok(InjectionDrillReport {
            kind: "重入注入（处理中置位推迟语义）",
            total: times,
            detected,
            all_detected: detected == times,
        })
    }

    /// 跨核误置注入演练（双通道各半）：链缺跨发 / 源冒充他核，每次
    /// 必须被隔离检测拒绝并立案。
    pub fn drill_cross_core_misset(
        &mut self,
        times: u32,
    ) -> Result<InjectionDrillReport, BhTrigError> {
        let bound = self.bound_core;
        let other = ((bound as usize + 1) % MAX_CORES) as u8;
        // 目标面槽位注册（误置演练打到已注册槽位，聚焦隔离判定本身）。
        self.register_core_defaults(other)?;
        let mut detected = 0u32;
        let mut tick = self.tick_floor.wrapping_add(1);
        let mut t = 0u32;
        while t < times {
            let slot = (t % MAX_SLOTS as u32) as u8;
            let r = if t % 2 == 0 {
                // 通道一：源=本核、IPI 链未建立即跨发。
                self.raise_cross(bound, other, slot, tick)
            } else {
                // 通道二：发起核冒充他核（源 != 本核上下文）。
                self.raise_cross(other, bound, slot, tick)
            };
            let hit = matches!(
                r,
                Err(BhTrigError::IpiLinkMissing { .. }) | Err(BhTrigError::CrossCoreMisSet { .. })
            );
            if hit {
                detected = detected.wrapping_add(1);
            }
            tick = tick.wrapping_add(1);
            t = t.wrapping_add(1);
        }
        Ok(InjectionDrillReport {
            kind: "跨核误置注入（链缺/冒源双通道）",
            total: times,
            detected,
            all_detected: detected == times,
        })
    }

    /// 丢失注入演练（判据「丢失注入 10/10 次检测」）：饿死型（置位
    /// 后长驱 tick 不触发 → 归因「触发点未跑」）与清位型（置位后
    /// 撤单 → 归因「位图被清」）各半。
    pub fn drill_loss_injection(
        &mut self,
        times: u32,
    ) -> Result<InjectionDrillReport, BhTrigError> {
        let bound = self.bound_core;
        self.register_core_defaults(bound)?;
        let mut detected = 0u32;
        let mut tick = self.tick_floor.wrapping_add(1);
        let mut t = 0u32;
        while t < times {
            let slot = (t % MAX_SLOTS as u32) as u8;
            if t % 2 == 0 {
                // 饿死型：置位后跨过超龄阈值再扫描——须以「触发点未跑」
                // 归因检出（本核置位后零次触发经过）。
                self.raise_local(slot, tick)?;
                tick = tick.wrapping_add(1);
                tick = tick.wrapping_add(LOSS_THRESHOLD_TICKS + 8);
                let rep = self.scan_losses(tick)?;
                let hit = rep
                    .aged
                    .iter()
                    .any(|a| a.slot == slot && a.cause == LossCause::TriggerNotRun);
                // 清账：显式触发带走该槽（服务即闭环——不落丢失账）。
                self.fire(BhTriggerPoint::Explicit, tick)?;
                tick = tick.wrapping_add(1);
                if hit {
                    detected = detected.wrapping_add(1);
                }
            } else {
                // 清位型：置位后直接撤单——须以「位图被清」归因检出。
                self.raise_local(slot, tick)?;
                tick = tick.wrapping_add(1);
                let _ = self.clear_without_service(slot, tick)?;
                tick = tick.wrapping_add(1);
                let rep = self.scan_losses(tick)?;
                let hit = rep
                    .cleared
                    .iter()
                    .any(|a| a.slot == slot && a.cause == LossCause::BitmapCleared);
                tick = tick.wrapping_add(1);
                if hit {
                    detected = detected.wrapping_add(1);
                }
            }
            t = t.wrapping_add(1);
        }
        Ok(InjectionDrillReport {
            kind: "丢失注入（饿死/清位双型）",
            total: times,
            detected,
            all_detected: detected == times,
        })
    }
}

// ---------------------------------------------------------------------------
// §11 确定性脚本与账平审计（判据「账平 10 万次」与双跑对拍）
// ---------------------------------------------------------------------------

/// 混合流脚本统计（指纹对拍的载荷）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ScriptStats {
    /// 置位操作数（含合并）。
    pub raises: u32,
    /// 合并数（幂等——位已置）。
    pub merged: u32,
    /// 触发轮数。
    pub fires: u32,
    /// 服务槽数。
    pub services: u32,
    /// 显式清位数。
    pub clears: u32,
    /// 扫描轮数。
    pub scans: u32,
    /// 丢失检出数。
    pub losses: u32,
    /// 收官排空轮数。
    pub drained_rounds: u32,
    /// 终态 tick。
    pub final_tick: u64,
    /// 终态指纹。
    pub fingerprint: u64,
}

/// 触发点选取（全函数——下标模 3 无越界面）。
fn pick_point(idx: u8) -> BhTriggerPoint {
    match idx % 3 {
        0 => BhTriggerPoint::IrqReturn,
        1 => BhTriggerPoint::SchedPoint,
        _ => BhTriggerPoint::Explicit,
    }
}

/// LCG 确定性混合流脚本：置位 60% / 触发 20% / 显式清位 10% /
/// 丢失扫描 10%，tick 步进 1..7，收官显式全扫排空 + 全面账平审计。
/// 同种子双引擎逐位可复现——双跑对拍的被测对象。
pub fn run_script(
    e: &mut BhTrigEngine,
    lcg: &mut Lcg,
    ops: u32,
) -> Result<ScriptStats, BhTrigError> {
    let _ = e.register_core_defaults(e.bound_core())?;
    let mut st = ScriptStats::default();
    let mut tick: u64 = 1;
    let mut op = 0u32;
    while op < ops {
        let r = lcg.next_u32();
        match r % 10 {
            0..=5 => {
                let slot = ((r >> 8) % MAX_SLOTS as u32) as u8;
                match e.raise_local(slot, tick)? {
                    RaiseOutcome::NewlySet => st.raises += 1,
                    RaiseOutcome::CoalescedMerged => {
                        st.raises += 1;
                        st.merged += 1;
                    }
                }
            }
            6..=7 => {
                let p = pick_point(((r >> 12) % 3) as u8);
                let fr = e.fire(p, tick)?;
                st.fires += 1;
                st.services += fr.serviced.len() as u32;
            }
            8 => {
                let slot = ((r >> 8) % MAX_SLOTS as u32) as u8;
                let _ = e.clear_without_service(slot, tick)?;
                st.clears += 1;
            }
            _ => {
                let rep = e.scan_losses(tick)?;
                st.losses += rep.total_found;
                st.scans += 1;
            }
        }
        tick = tick.wrapping_add(1 + ((r >> 20) % 7) as u64);
        op += 1;
    }
    st.drained_rounds = e.drain_explicit(&mut tick)?;
    st.final_tick = tick;
    st.fingerprint = e.fingerprint();
    Ok(st)
}

/// 账平审计（判据「置位-触发-清位账平 10 万次」）：10 万次 LCG 混合流
/// 后逐槽位 + 全局双查恒等式 `有效置位 == 服务 + 未决 + 清位丢失`。
pub fn balance_audit_100k(seed: u64) -> Result<BalanceReport, BhTrigError> {
    let mut e = BhTrigEngine::new(0)?;
    let mut lcg = Lcg::new(seed);
    let _ = run_script(&mut e, &mut lcg, BALANCE_AUDIT_OPS)?;
    let mut rep = e.audit_balance_all()?;
    rep.ops = BALANCE_AUDIT_OPS;
    Ok(rep)
}

/// 双跑确定性对拍：同种子双引擎同脚本，指纹 + 六项统计全等。
pub fn dual_run_determinism(ops: u32, seed: u64) -> Result<DualRunReport, BhTrigError> {
    let mut a = BhTrigEngine::new(0)?;
    let mut b = BhTrigEngine::new(0)?;
    let mut la = Lcg::new(seed);
    let mut lb = Lcg::new(seed);
    let sa = run_script(&mut a, &mut la, ops)?;
    let sb = run_script(&mut b, &mut lb, ops)?;
    if sa.fingerprint != sb.fingerprint
        || sa.raises != sb.raises
        || sa.merged != sb.merged
        || sa.services != sb.services
        || sa.clears != sb.clears
        || sa.losses != sb.losses
        || sa.drained_rounds != sb.drained_rounds
    {
        return Err(BhTrigError::DualRunMismatch {
            digest_a: sa.fingerprint,
            digest_b: sb.fingerprint,
        });
    }
    Ok(DualRunReport {
        ops,
        seed,
        digest_a: sa.fingerprint,
        digest_b: sb.fingerprint,
        totals_a: a.engine_totals(),
        totals_b: b.engine_totals(),
    })
}

// ---------------------------------------------------------------------------
// §12 契约冻结与全链自证（聚合入口——ktest 桥接点）
// ---------------------------------------------------------------------------

/// 契约指纹（三触发点语义表 + 预算表 + 超龄阈值 + 位图/账容锚）。
pub fn contract_fingerprint() -> u64 {
    let mut buf: Vec<u8> = Vec::new();
    let all = BhTriggerPoint::all();
    let mut i = 0usize;
    while i < 3 {
        buf.push(all[i].budget());
        buf.push(all[i].is_same_core_immediate() as u8);
        buf.extend_from_slice(all[i].semantics_note().as_bytes());
        i += 1;
    }
    buf.extend_from_slice(&LOSS_THRESHOLD_TICKS.to_le_bytes());
    buf.extend_from_slice(&(MAX_SLOTS as u64).to_le_bytes());
    buf.extend_from_slice(&(LEDGER_CAP as u64).to_le_bytes());
    buf.extend_from_slice(&BH_TRIG_VERSION.as_bytes());
    fnv64(&buf)
}

/// 契约冻结：语义表三查（预算表/同核即时标记/注记互异非空）+
/// 指纹双采样稳定。
pub fn contract_frozen() -> Result<String, (u32, String)> {
    let all = BhTriggerPoint::all();
    if all[0].budget() != BUDGET_IRQ_RETURN
        || all[1].budget() != BUDGET_SCHED_POINT
        || all[2].budget() != BUDGET_EXPLICIT
    {
        return Err((
            CODE_FINGERPRINT_DRIFT as u32,
            "预算表与冻结值不一致（中断返回前=8/调度点=4/显式=32）".to_string(),
        ));
    }
    if !all[0].is_same_core_immediate()
        || all[1].is_same_core_immediate()
        || all[2].is_same_core_immediate()
    {
        return Err((
            CODE_FINGERPRINT_DRIFT as u32,
            "同核即时标记表与冻结值不一致（仅中断返回前为同核即时）".to_string(),
        ));
    }
    let n0 = all[0].semantics_note();
    let n1 = all[1].semantics_note();
    let n2 = all[2].semantics_note();
    if n0.is_empty() || n1.is_empty() || n2.is_empty() || n0 == n1 || n1 == n2 || n0 == n2 {
        return Err((
            CODE_FINGERPRINT_DRIFT as u32,
            "语义注记表为空或互同——注记必须三点互异非空".to_string(),
        ));
    }
    let fp0 = contract_fingerprint();
    let fp1 = contract_fingerprint();
    if fp0 != fp1 {
        return Err((
            CODE_FINGERPRINT_DRIFT as u32,
            "契约指纹双采样不稳定——存在非确定性源".to_string(),
        ));
    }
    Ok(format!(
        "语义表冻结（指纹 {:#018x}）：{} / {} / {}",
        fp0, n0, n1, n2
    ))
}

/// 错误换轨（BhTrigError → (诊断码, 中文诊断)——聚合入口统一出口）。
fn emap<T>(r: Result<T, BhTrigError>) -> Result<T, (u32, String)> {
    r.map_err(|x| (x.code() as u32, x.detail().to_string()))
}

/// 全链自证：契约冻结 + 三触发点 10/10×3 + 重入/丢失/跨核误置注入
/// 全检出 + 推迟全达成 + 健康面（净绿/注入红）+ 双跑对拍 + 账平
/// 10 万，一锤定音。
pub fn run_all_checks() -> Result<String, (u32, String)> {
    let mut parts: Vec<String> = Vec::new();
    parts.push(contract_frozen()?);

    // 三触发点演练（各 10/10）。
    let mut e = emap(BhTrigEngine::new(0))?;
    let d = emap(e.drill_trigger_points())?;
    if !d.all_passed() {
        return Err((
            CODE_ORDER_VIOLATION as u32,
            format!("三触发点演练未全过：{}", d.summary()),
        ));
    }
    parts.push(format!("三触发点 {}", d.summary()));

    // 重入注入（推迟语义）10/10。
    let r = emap(e.drill_reentry_injection(10))?;
    if !r.all_detected {
        return Err((
            CODE_REENTRY_VIOLATION as u32,
            format!("重入注入未全检出 {}/{}", r.detected, r.total),
        ));
    }
    parts.push(format!("重入推迟语义 {}/{}", r.detected, r.total));

    // 丢失注入 10/10。
    let l = emap(e.drill_loss_injection(10))?;
    if !l.all_detected {
        return Err((
            CODE_LOSS_UNATTRIBUTED as u32,
            format!("丢失注入未全检出 {}/{}", l.detected, l.total),
        ));
    }
    parts.push(format!("丢失注入检出 {}/{}", l.detected, l.total));

    // 跨核误置注入 10/10。
    let c = emap(e.drill_cross_core_misset(10))?;
    if !c.all_detected {
        return Err((
            CODE_CROSS_CORE_MISSET as u32,
            format!("跨核误置注入未全检出 {}/{}", c.detected, c.total),
        ));
    }
    parts.push(format!("跨核误置检出 {}/{}", c.detected, c.total));

    // 推迟全达成（在望表清零 + 产生/达成账平）。
    if e.deferred_watch() != 0 || e.deferred_raised_total() != e.deferred_resolved_total() {
        return Err((
            CODE_DEFERRED_MISS as u32,
            format!(
                "推迟未全达成（在望={} 产生={} 达成={}）",
                e.deferred_watch(),
                e.deferred_raised_total(),
                e.deferred_resolved_total()
            ),
        ));
    }

    // 健康面：注入引擎应红档、净引擎应绿档。
    let hp = e.health();
    if hp.verdict != HealthVerdict::Red {
        return Err((
            CODE_HEALTH_RED as u32,
            format!("注入引擎应为红档，实为 {}", hp.verdict.name()),
        ));
    }
    parts.push(format!("注入引擎红档（{}）", hp.summary));
    let mut clean = emap(BhTrigEngine::new(0))?;
    emap(clean.register_core_defaults(0))?;
    let hc = clean.health();
    if hc.verdict != HealthVerdict::Green {
        return Err((
            CODE_HEALTH_RED as u32,
            format!("净引擎应为绿档：{}", hc.summary),
        ));
    }
    emap(clean.require_green())?;
    parts.push(format!("净引擎绿档（{}）", hc.summary));

    // 双跑确定性对拍。
    let dr = emap(dual_run_determinism(2_000, 0x0944))?;
    if dr.digest_a != dr.digest_b {
        return Err((
            CODE_DUAL_RUN_MISMATCH as u32,
            format!("双跑指纹失配 {:#018x} vs {:#018x}", dr.digest_a, dr.digest_b),
        ));
    }
    parts.push(format!("双跑对拍一致（{:#018x}）", dr.digest_a));

    // 账平 10 万。
    let bal = emap(balance_audit_100k(0x0944_0944))?;
    if !bal.balanced {
        return Err((CODE_BALANCE_BROKEN as u32, "账平 10 万审计未过".to_string()));
    }
    parts.push(format!(
        "账平 10 万（置位={} 服务={} 未决={} 丢失={} 合并={} 逐槽={})",
        bal.sets, bal.serviced, bal.outstanding, bal.lost, bal.merged, bal.slots_checked
    ));
    Ok(parts.join("；"))
}

// ---------------------------------------------------------------------------
// §13 冒烟测试（宿主测试链路——判据逐条落账）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    /// 净引擎工厂（默认 32 槽注册齐）。
    fn fresh() -> BhTrigEngine {
        let mut e = BhTrigEngine::new(0).unwrap();
        e.register_core_defaults(0).unwrap();
        e
    }

    /// 判据一：三触发点正确（中断返回前/调度点/显式触发各 10/10 次）。
    #[test]
    fn trigger_points_10x3() {
        let mut e = fresh();
        let rep = e.drill_trigger_points().unwrap();
        assert!(rep.all_passed(), "三触发点演练未全过：{}", rep.summary());
        assert_eq!(rep.total, [10, 10, 10]);
        // 分点触发账落账：每点恰好 10 轮触发。
        assert_eq!(e.plane(0).unwrap().fires_by_point(BhTriggerPoint::IrqReturn), 10);
        assert_eq!(e.plane(0).unwrap().fires_by_point(BhTriggerPoint::SchedPoint), 10);
        assert_eq!(e.plane(0).unwrap().fires_by_point(BhTriggerPoint::Explicit), 10);
        // 语义表差异面：预算与同核即时标记三点互异。
        assert_eq!(BhTriggerPoint::IrqReturn.budget(), BUDGET_IRQ_RETURN);
        assert_eq!(BhTriggerPoint::SchedPoint.budget(), BUDGET_SCHED_POINT);
        assert_eq!(BhTriggerPoint::Explicit.budget(), BUDGET_EXPLICIT);
        assert!(BhTriggerPoint::IrqReturn.is_same_core_immediate());
        assert!(!BhTriggerPoint::SchedPoint.is_same_core_immediate());
        assert!(!BhTriggerPoint::Explicit.is_same_core_immediate());
    }

    /// 判据二：pending 一致性（置位-触发-清位账平 10 万次）。
    #[test]
    fn balance_100k() {
        let rep = balance_audit_100k(0x0944_0944).unwrap();
        assert!(rep.balanced);
        assert_eq!(rep.ops, BALANCE_AUDIT_OPS);
        assert_eq!(rep.slots_checked, 128); // 4 核 × 32 槽逐槽核对
        // 恒等式逐字复核：置位 == 服务 + 未决 + 丢失。
        assert_eq!(
            rep.sets,
            rep.serviced
                .wrapping_add(rep.outstanding)
                .wrapping_add(rep.lost)
        );
        // 混合流确实跑到了四类操作（防注水）。
        assert!(rep.sets > 30_000, "置位面未跑穿：{}", rep.sets);
        assert!(rep.serviced > 0 && rep.lost > 0 && rep.merged > 0);
    }

    /// 判据三：丢失注入 10/10 次检测（饿死/清位双型归因）。
    #[test]
    fn loss_injection_10() {
        let mut e = fresh();
        let rep = e.drill_loss_injection(10).unwrap();
        assert!(rep.all_detected, "丢失注入未全检出：{}/{}", rep.detected, rep.total);
        assert_eq!(e.loss_events_total(), 10);
        // 归因二分各自立案（事件账里丢失立案各型均有）。
        assert!(e.ledger().pushed_total() > 0);
    }

    /// 推迟语义（重入注入 10/10）：处理中置位本轮不服务、下轮必达。
    #[test]
    fn reentry_injection_10() {
        let mut e = fresh();
        let rep = e.drill_reentry_injection(10).unwrap();
        assert!(rep.all_detected, "重入注入未全检出：{}/{}", rep.detected, rep.total);
        assert_eq!(e.reentry_guarded(), 10);
        assert_eq!(e.deferred_raised_total(), 10);
        assert_eq!(e.deferred_resolved_total(), 10);
        assert_eq!(e.deferred_watch(), 0);
    }

    /// 跨核误置注入（链缺/冒源双通道）10/10 + 正向跨核链路。
    #[test]
    fn cross_core_misset_10_and_positive_path() {
        let mut e = fresh();
        let rep = e.drill_cross_core_misset(10).unwrap();
        assert!(rep.all_detected, "跨核误置未全检出：{}/{}", rep.detected, rep.total);
        assert_eq!(e.misset_flagged(), 10);
        // 正向：建链后跨核置位应达对面 pending 面。
        e.link_cores(0, 1).unwrap();
        assert!(e.ipi_linked(0, 1).unwrap());
        let out = e.raise_cross(0, 1, 7, 10).unwrap();
        assert_eq!(out, RaiseOutcome::NewlySet);
        assert!(e.plane(1).unwrap().bitmap().is_pending(7));
        // 未建链方向仍拒。
        assert!(matches!(
            e.raise_cross(0, 2, 7, 11),
            Err(BhTrigError::IpiLinkMissing { src: 0, dst: 2 })
        ));
    }

    /// 预算语义：调度点预算 4——6 槽未决只服务 4，余 2 留账。
    #[test]
    fn budget_semantics_sched_point() {
        let mut e = fresh();
        let mut tick = 1u64;
        for slot in 0..6u8 {
            e.raise_local(slot, tick).unwrap();
            tick += 1;
        }
        let fr = e.fire(BhTriggerPoint::SchedPoint, tick).unwrap();
        assert_eq!(fr.serviced.len(), BUDGET_SCHED_POINT as usize);
        assert_eq!(fr.serviced, vec![0, 1, 2, 3]);
        assert_eq!(fr.remained_pending, 0b11_0000);
        // 收官显式全扫带走余量。
        e.fire(BhTriggerPoint::Explicit, tick + 1).unwrap();
        assert_eq!(e.plane(0).unwrap().bitmap().pending_mask(), 0);
    }

    /// 次序纪律：pending 槽位按（优先级,槽位号）序执行。
    #[test]
    fn priority_order() {
        let mut e = fresh();
        let mut tick = 1u64;
        // 乱序置位（默认优先级 = slot*8：0,16,40,80,160 五档）。
        for slot in [20u8, 0, 10, 2, 5] {
            e.raise_local(slot, tick).unwrap();
            tick += 1;
        }
        let fr = e.fire(BhTriggerPoint::IrqReturn, tick).unwrap();
        assert_eq!(fr.serviced, vec![0, 2, 5, 10, 20]);
        // 优先级档归类抽查。
        assert_eq!(PrioClass::of(0), PrioClass::Urgent);
        assert_eq!(PrioClass::of(16), PrioClass::High);
        assert_eq!(PrioClass::of(80), PrioClass::Normal);
        assert_eq!(PrioClass::of(160), PrioClass::Idle);
    }

    /// 推迟语义手工版：midfire 钩子槽在快照外，本轮不进服务序。
    #[test]
    fn deferred_manual() {
        let mut e = fresh();
        e.raise_local(3, 1).unwrap();
        e.midfire_raise = Some(9);
        let fr = e.fire(BhTriggerPoint::IrqReturn, 2).unwrap();
        assert_eq!(fr.serviced, vec![3]);
        assert_eq!(fr.deferred, vec![9]);
        assert!(fr.remained_pending & (1 << 9) != 0);
        // 下一轮必达。
        let fr2 = e.fire(BhTriggerPoint::IrqReturn, 3).unwrap();
        assert_eq!(fr2.serviced, vec![9]);
        assert_eq!(e.deferred_resolved_total(), 1);
    }

    /// 置位幂等：已置位再置位记合并不重复记账。
    #[test]
    fn raise_idempotent_merge() {
        let mut e = fresh();
        assert_eq!(e.raise_local(4, 1).unwrap(), RaiseOutcome::NewlySet);
        assert_eq!(e.raise_local(4, 2).unwrap(), RaiseOutcome::CoalescedMerged);
        assert_eq!(e.raise_local(4, 3).unwrap(), RaiseOutcome::CoalescedMerged);
        let bm = e.plane(0).unwrap().bitmap();
        assert_eq!(bm.effective_sets_of(4), 1);
        assert_eq!(bm.merged_of(4), 2);
        // 事件账：1 置位 + 2 合并。
        let evs = e.ledger().iter_ordered();
        let sets = evs.iter().filter(|x| x.kind == EventKind::SlotSet).count();
        let merged = evs.iter().filter(|x| x.kind == EventKind::SlotMerged).count();
        assert_eq!(sets, 1);
        assert_eq!(merged, 2);
    }

    /// tick 回退拒绝（零墙钟纪律——单调性破即红账）。
    #[test]
    fn tick_regress_rejected() {
        let mut e = fresh();
        e.raise_local(0, 10).unwrap();
        assert!(matches!(
            e.raise_local(1, 9),
            Err(BhTrigError::TickRegress { tick: 9, floor: 10 })
        ));
        assert!(matches!(
            e.fire(BhTriggerPoint::Explicit, 5),
            Err(BhTrigError::TickRegress { tick: 5, floor: 10 })
        ));
    }

    /// 误用面：未注册槽/越界槽/越界核各归其码。
    #[test]
    fn misuse_paths() {
        let mut e = BhTrigEngine::new(0).unwrap(); // 不注册默认面
        assert!(matches!(
            e.raise_local(0, 1),
            Err(BhTrigError::SlotNotRegistered { core: 0, slot: 0 })
        ));
        assert!(matches!(
            e.raise_local(32, 1),
            Err(BhTrigError::SlotOutOfRange { core: 0, slot: 32 })
        ));
        assert!(matches!(
            BhTrigEngine::new(4),
            Err(BhTrigError::CoreOutOfRange { core: 4 })
        ));
        assert!(matches!(
            e.register_slot(9, 0, 0, 0),
            Err(BhTrigError::CoreOutOfRange { core: 9 })
        ));
        // 闭集外触发点下标反查为 None（查询面不立案）。
        assert!(BhTriggerPoint::from_index(2).is_some());
        assert!(BhTriggerPoint::from_index(3).is_none());
    }

    /// 账面幽灵服务拒绝：非未决槽进入服务面即拒（位图面防御路径）。
    #[test]
    fn ghost_service_rejected() {
        let mut e = fresh();
        assert!(e.fire(BhTriggerPoint::Explicit, 1).is_ok()); // 空触发合法
        // 槽 0 已注册但未决位不在——服务面拒收（幽灵服务防线）。
        assert!(matches!(
            e.plane_mut(0).unwrap().bitmap_mut().service(0, 1),
            Err(BhTrigError::SlotNotRegistered { .. })
        ));
        // 幽灵清位：非未决槽清位返回 false 不记丢失账。
        assert!(!e.clear_without_service(0, 2).unwrap());
        assert_eq!(e.plane(0).unwrap().bitmap().lost_of(0), 0);
    }

    /// 双跑确定性对拍：同种子双引擎指纹与六项统计全等。
    #[test]
    fn dual_run() {
        let rep = dual_run_determinism(2_000, 0x0944).unwrap();
        assert_eq!(rep.digest_a, rep.digest_b);
        assert_eq!(rep.totals_a, rep.totals_b);
        // 换种子必换指纹（非平凡性抽查）。
        let rep2 = dual_run_determinism(2_000, 0x0945).unwrap();
        assert_ne!(rep.digest_a, rep2.digest_a);
    }

    /// 健康面：净引擎绿档；注入引擎红档；require_green 门行为对。
    #[test]
    fn health_verdicts() {
        let clean = fresh();
        assert_eq!(clean.health().verdict, HealthVerdict::Green);
        clean.require_green().unwrap();

        let mut dirty = fresh();
        dirty.register_core_defaults(1).unwrap();
        let _ = dirty.raise_cross(0, 1, 0, 1); // 链缺误置 → 立案
        let h = dirty.health();
        assert_eq!(h.verdict, HealthVerdict::Red);
        assert!(h.issue_codes.contains(&CODE_CROSS_CORE_MISSET));
        assert!(dirty.require_green().is_err());
    }

    /// 指纹稳定性：同账面双采样一致；账面一动指纹即动。
    #[test]
    fn fingerprint_stability() {
        let mut a = fresh();
        let fp0 = a.fingerprint();
        let fp1 = a.fingerprint();
        assert_eq!(fp0, fp1);
        a.raise_local(0, 1).unwrap();
        assert_ne!(a.fingerprint(), fp0);
        // 双引擎同操作序列 → 同指纹。
        let mut b = fresh();
        let mut b2 = fresh();
        b.raise_local(0, 1).unwrap();
        b2.raise_local(0, 1).unwrap();
        assert_eq!(b.fingerprint(), b2.fingerprint());
    }

    /// 事件账环形饱和：超额入账覆盖最旧、覆盖数显性、不炸不漏位。
    #[test]
    fn ledger_ring_saturation() {
        let mut e = fresh();
        let mut tick = 1u64;
        for i in 0..600u32 {
            let slot = (i % MAX_SLOTS as u32) as u8;
            e.raise_local(slot, tick).unwrap();
            e.fire(BhTriggerPoint::Explicit, tick).unwrap();
            tick += 1;
        }
        assert!(e.ledger().overflow_dropped() > 0);
        assert_eq!(e.ledger().resident(), LEDGER_CAP);
        assert_eq!(e.ledger().iter_ordered().len(), LEDGER_CAP);
        // 环覆盖后引擎账面仍平。
        assert!(e.engine_balance_ok());
        e.audit_balance_all().unwrap();
    }

    /// 错误面完整：16 变体诊断码全落 0x19F0 段、detail 全非空。
    #[test]
    fn error_surface_complete() {
        let errs: [BhTrigError; 16] = [
            BhTrigError::SlotOutOfRange { core: 0, slot: 32 },
            BhTrigError::SlotNotRegistered { core: 0, slot: 0 },
            BhTrigError::CoreOutOfRange { core: 4 },
            BhTrigError::CrossCoreMisSet { src: 0, dst: 1, slot: 0 },
            BhTrigError::IpiLinkMissing { src: 0, dst: 1 },
            BhTrigError::ReentryViolation { core: 0, slot: 0 },
            BhTrigError::OrderViolation { core: 0 },
            BhTrigError::LossUnattributed { core: 0, slot: 0 },
            BhTrigError::BalanceBroken { core: 0, slot: 0, sets: 1, lhs: 2 },
            BhTrigError::DualRunMismatch { digest_a: 1, digest_b: 2 },
            BhTrigError::FingerprintDrift { expected: 1, actual: 2 },
            BhTrigError::TickRegress { tick: 1, floor: 2 },
            BhTrigError::DeferredMiss { core: 0, slot: 0 },
            BhTrigError::LedgerOverflow { dropped: 1 },
            BhTrigError::HealthRed { codes: 0 },
            BhTrigError::DrainLimit { pending: 1 },
        ];
        for e in errs.iter() {
            let c = e.code();
            assert!((0x19F0..=0x19FF).contains(&c), "诊断码越段：{:#06x}", c);
            assert!(!e.detail().is_empty());
            assert!(e.detail().contains("；") || e.detail().len() > 10);
        }
    }

    /// 尺寸红线：引擎整体 <= 64KB（禁上栈红线运行期复核）。
    #[test]
    fn engine_size_redline() {
        assert!(core::mem::size_of::<BhTrigEngine>() <= 65_536);
        assert!(core::mem::size_of::<BhEvent>() <= 32);
        // 事件账容量锚：判据「256 槽」。
        assert_eq!(LEDGER_CAP, 256);
    }

    /// 丢失扫描干净面零误报：无注入时扫描零检出。
    #[test]
    fn loss_scan_clean_zero() {
        let mut e = fresh();
        e.raise_local(0, 1).unwrap();
        let rep = e.scan_losses(2).unwrap(); // 龄 1 < 64——不误报
        assert_eq!(rep.total_found, 0);
        e.fire(BhTriggerPoint::Explicit, 3).unwrap();
        let rep2 = e.scan_losses(4).unwrap();
        assert_eq!(rep2.total_found, 0);
        assert_eq!(e.loss_events_total(), 0);
    }

    /// 全链自证聚合入口（含 10 万账平 + 双跑 + 全注入面）。
    #[test]
    fn run_all_checks_green() {
        let s = run_all_checks().unwrap();
        assert!(s.contains("三触发点"));
        assert!(s.contains("账平 10 万"));
        assert!(s.contains("双跑对拍一致"));
    }
}
