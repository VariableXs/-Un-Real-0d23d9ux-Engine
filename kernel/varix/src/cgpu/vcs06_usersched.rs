//! CGPU-F2886 · 多用户优先级调度（CGPU-S 域 · 多用户组 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F2886`
//!
//! **判据（锚点原文）**：前后台、复用声明、两组、判据。
//!
//! **职责定位（锚点原文摘录）**：调度（多用户优先级调度（前台优先/后台让路——调度实现（C 域预算复用——复用声明；测试（优先级/复用两组）。
//!
//! # 深度展开说明（本文件为锚点判据的深度展开版）
//!
//! 锚点判据四词逐条兑现为真实子系统（不是注释声明，是可运行账本）：
//!
//! 一、**前后台**——四级用户类 [`UserClass`]（Interactive / Foreground /
//!     Background / Idle）配 [`ClassPolicy`] const 策略表（保底份额 ppm /
//!     DRR 配额 / 突发上限 / 让度规则闭集）。前台就绪即插队（相位序：交互
//!     →前台→后台→闲时），但**后台保底红线永不破**——后台保底单位在每个
//!     tick 的第一相位先行实授（先扣先授，前台 burst 摸不到这笔预算）。
//! 二、**复用声明**——份额切分口径**复用 C 域 F0322 预算池账户切分口径**
//!     （见 [`REUSE_DECL_F0322`]）：预算池总额恒 1e6 ppm，注册/退休/调额
//!     全部零和搬运（未分配池 ↔ 会话账户），编译期对默认切分表做总和
//!     恰 1e6 断言，运行期每次账目变动后再校验（[`UserScheduler::pool_audit`]）。
//! 三、**调度实现**——同级公平走赤字轮转 DRR（[`UserScheduler::tick`]
//!     相位 3~6），每 tick 配额注入+赤字结转（结转上限=类突发上限）；
//!     饥饿防线（starving 计数超阈值 → 强制注入保底 quantum + 告警码
//!     0x1318 入账）；优先级跃迁（[`UserScheduler::promote`] 即时升档 /
//!     [`UserScheduler::demote`] 三拍确认防抖——理由见方法文档）；每 tick
//!     记账守恒（Σ实授 + 未分配 ≡ 10000 单位刻度）+ 裁决留痕
//!     （[`TickLedger`] 定长环，每笔裁决带 [`Verdict`] 理由码）。
//! 四、**测试（优先级/复用两组）**——见文件尾 `#[cfg(test)]`：优先级组
//!     （抢占/保底红线/滞回/DRR 公平/饥饿注入）与复用组（池零和/份额
//!     对齐/守恒/墓碑不复用）各成一组，另附确定性双跑对拍与体检组。
//!
//! # 隔离与纪律
//!
//! - 完全自包含：仅 `use core::…` / `use alloc::…`，**零 `use crate::…`**；
//! - no_std + alloc 内核路径零 unwrap / expect / panic! / 运行期 assert!，
//!   可失败一律 `Result<T, UserSchedError>`（诊断码段 0x1310~0x131F 独占，
//!   每变体 `detail()` 中文可操作诊断）；
//! - 溢出全部显性：份额/预算 saturating，tick/sid wrapping；
//! - >64KB 结构（账本环 ≈30KB）只驻堆（Vec），不上栈；
//! - 常量数值来源逐条注释（预算口径 F0322、阈值来源 tick 帧窗口）。

use alloc::vec;
use alloc::vec::Vec;

// ═══════════════════════════════════════════════════════════════════
// §0 模块常量（数值来源逐条注明）
// ═══════════════════════════════════════════════════════════════════

/// 模块版本（VCS06 用户调度 · 深度展开版）。
pub const USERSCHED_VERSION: &str = "S06-usersched-v1";

/// 预算总口径：1e6 ppm = 100%（复用 C 域 F0322 预算池账户切分口径）。
pub const PPM_TOTAL: u64 = 1_000_000;

/// 每 tick GPU 时间片总单位（万分刻度：1 tick = 10000 单位，1 单位 =
/// 100 ppm；口径来源 F0322「份额=账户切分、tick=万分刻度」）。
pub const TICK_UNITS: u64 = 10_000;

/// 会话表容量上限（来源：多用户组规格 F2886 上游 vcs02_multiuser
/// 的单机组并发上限 64；超出即拒，不静默扩表）。
pub const MAX_SESSIONS: usize = 64;

/// 账本环容量（96 tick ≈ 1.6 秒@60Hz 溯源窗口；96×TickRecord≈41KB
/// ——Option 判别标签使行宽 424B；只驻堆（Vec），不上栈，低于 64KB
/// 上栈禁线）。
pub const LEDGER_CAP: usize = 96;

/// 单 tick 裁决留痕条数上限（超出折入 folded 计数——留痕是采样不是
/// 全量日志，环账本语义）。
pub const DECISIONS_CAP: usize = 24;

/// 告警账容量（0x1318 饥饿注入等告警码入账上限，满则挤最旧）。
pub const WARN_CAP: usize = 64;

/// 饥饿阈值（来源：16 tick ≈ 60Hz 下一帧交互窗口 267ms；后台连续
/// 16 tick 未达应得即触发强制注入）。
pub const STARVE_THRESHOLD: u32 = 16;

/// 饥饿急限（体检失败线：注入机制每 16 tick 应复位一次，若仍累积到
/// 64 tick 说明注入失灵——账本损坏级，体检必须报 0x1318）。
pub const STARVE_EMERGENCY: u32 = 64;

/// 降档确认拍数（滞回防抖：降档需连续 3 拍申请；来源见 `demote` 文档）。
pub const DEMOTE_CONFIRM_TICKS: u32 = 3;

/// 最小注册份额（1% = 10000 ppm；低于此的切分粒度无调度意义且放大
/// 账本行数，拒收）。
pub const MIN_SHARE: u64 = 10_000;

/// 份额对齐粒度：100 ppm（1 单位 = 100 ppm，未对齐则 due 除不尽、
/// 守恒式出现碎账——对齐是「对齐镜像」的字面义务）。
pub const SHARE_ALIGN: u64 = 100;

/// 复用声明（判据「C 域预算复用——复用声明」锚定正文）：
/// 本模块份额切分 = C 域 F0322 预算池账户切分口径的对齐镜像——
/// 总池 1e6 ppm、单位 100ppm、零和搬运、变动后再校验，四条不另立。
pub const REUSE_DECL_F0322: &str =
    "复用 C 域 F0322 口径：份额=预算池账户切分对齐镜像（1e6 ppm 总池 / 100 ppm 单位 / 零和搬运 / 变动再校验）";

/// 诊断码段下界（本模块独占 0x1310~0x131F，开工前已 grep 全 cgpu 确认空闲）。
pub const DIAG_SEG_LO: u16 = 0x1310;
/// 诊断码段上界。
pub const DIAG_SEG_HI: u16 = 0x131F;

// ── 编译期断言（const 域，禁运行期 assert 的镜像面） ──────────────

/// const 表求和（const fn 纪律：while+索引，禁 for/迭代器/Ord::min）。
const fn ppm_sum(table: &[u64]) -> u64 {
    let mut i = 0usize;
    let mut s = 0u64;
    while i < table.len() {
        s = s.wrapping_add(table[i]);
        i += 1;
    }
    s
}

// 判据「份额切分 ppm 和恰 1e6」的编译期面：默认切分表总和恰 1e6。
const DEFAULT_SHARE_SPLIT: [u64; 4] = [400_000, 300_000, 200_000, 100_000];
const _: () = assert!(ppm_sum(&DEFAULT_SHARE_SPLIT) == PPM_TOTAL);

// tick 刻度与池口径的整除镜像：10000 单位 × 100 ppm = 1e6 ppm。
const _: () = assert!(TICK_UNITS * SHARE_ALIGN == PPM_TOTAL);

// 会话容量与多用户组规格对齐。
const _: () = assert!(MAX_SESSIONS <= 64 && MAX_SESSIONS > 0);

// 账本环驻堆规模红线（≈41KB < 64KB 上栈禁线；环本体只经 Vec 驻堆）。
const _: () = assert!(LEDGER_CAP * core::mem::size_of::<TickRecord>() < 64 * 1024);

// 诊断码段独占校验（段宽 16，错误枚举 16 变体恰好铺满）。
const _: () = assert!(DIAG_SEG_HI - DIAG_SEG_LO == 0x0F);

// ═══════════════════════════════════════════════════════════════════
// §1 诊断错误（0x1310~0x131F 独占码段 · 中文可操作诊断）
// ═══════════════════════════════════════════════════════════════════

/// 用户调度诊断错误闭集（16 变体铺满 0x1310~0x131F 段，不留空洞——
/// 新增诊断走新段，不挤本段，段内地址即账）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum UserSchedError {
    /// 会话不存在（查无此 sid 或已墓碑）。
    NoSid,
    /// 墓碑 sid 复用企图（sid 只发一次，永不复用）。
    SidTomb,
    /// 会话表容量满。
    Capacity,
    /// 重复注册（同 sid 存活账上已有）。
    DupSid,
    /// 预算池总和失衡或资金不足（零和被破坏/出资方余额不够）。
    ShareSum,
    /// 份额非法（未按 100ppm 对齐 / 低于最小份额）。
    ShareRange,
    /// 状态机错序（操作与当前状态不容）。
    State,
    /// 保底红线/可行性破坏（份额低于类保底或出资后击穿出资方保底）。
    Floor,
    /// 饥饿升级超急限（注入防线失灵，账本损坏级）。
    StarveEsc,
    /// 类越界（顶类再升 / 底类再降）。
    BadClass,
    /// tick 守恒式破坏（Σ实授+未分配 ≠ 10000 单位）。
    Ledger,
    /// 参数非法（sid=0、delta=0、自转账等）。
    Arg,
    /// 赤字越界（负或超突发上限——账本损坏级）。
    Deficit,
    /// 双跑指纹不一致（确定性被破坏）。
    Fprint,
    /// 预算池未分配余额不足以注册（需指定 donor 出资）。
    PoolDrained,
    /// 内部不变量破坏（不该发生；出现即账本损坏，立即上浮）。
    Internal,
}

impl UserSchedError {
    /// 诊断码（0x1310~0x131F 段内独占）。
    pub const fn code(self) -> u16 {
        match self {
            UserSchedError::NoSid => 0x1310,
            UserSchedError::SidTomb => 0x1311,
            UserSchedError::Capacity => 0x1312,
            UserSchedError::DupSid => 0x1313,
            UserSchedError::ShareSum => 0x1314,
            UserSchedError::ShareRange => 0x1315,
            UserSchedError::State => 0x1316,
            UserSchedError::Floor => 0x1317,
            UserSchedError::StarveEsc => 0x1318,
            UserSchedError::BadClass => 0x1319,
            UserSchedError::Ledger => 0x131A,
            UserSchedError::Arg => 0x131B,
            UserSchedError::Deficit => 0x131C,
            UserSchedError::Fprint => 0x131D,
            UserSchedError::PoolDrained => 0x131E,
            UserSchedError::Internal => 0x131F,
        }
    }

    /// 中文可操作诊断（逐码给出「为什么+怎么办」，不留裸码）。
    pub const fn detail(self) -> &'static str {
        match self {
            UserSchedError::NoSid => "会话不存在：sid 查无或已墓碑——先用 session_view 核对 sid 是否存活（墓碑不可查）",
            UserSchedError::SidTomb => "墓碑 sid 复用企图：sid 只发一次永不复用——改用 register_auto 由单调计数器发新号",
            UserSchedError::Capacity => "会话表容量已满（上限 64）：先 retire 退休僵尸会话再注册新会话",
            UserSchedError::DupSid => "重复注册：该 sid 在存活账上已有——同一用户重复上线应复用原会话而非重注册",
            UserSchedError::ShareSum => "预算池零和失衡或资金不足：出资方余额不够或池总和≠1e6——先 set_share_pair 腾挪或改从未分配池出资",
            UserSchedError::ShareRange => "份额非法：未按 100ppm 对齐或低于最小份额 10000ppm——份额须为 100 的整数倍且≥1%",
            UserSchedError::State => "状态机错序：操作与当前状态不容（如对墓碑 resume、对暂停会话再暂停）——先核对 session_view().state",
            UserSchedError::Floor => "保底红线破坏：目标份额低于类保底或出资后击穿出资方类保底——先降档/先调份额再操作，红线不可协商",
            UserSchedError::StarveEsc => "饥饿超急限：注入防线未按期复位 starving——属账本损坏级，停机体检（health()）并回放账本环定位",
            UserSchedError::BadClass => "类越界：Interactive 不可再升、Idle 不可再降——升降档前先判类边界",
            UserSchedError::Ledger => "tick 守恒式破坏：Σ实授+未分配≠10000 单位——调度账本损坏，立即停机回放最近 96 tick 环账",
            UserSchedError::Arg => "参数非法：sid=0 / delta=0 / 自转账等——0 号 sid 保留、零和搬运不允许零元与自环",
            UserSchedError::Deficit => "赤字越界：DRR 赤字为负或超类突发上限——账本损坏，停机体检并用指纹对拍定位首坏 tick",
            UserSchedError::Fprint => "双跑指纹不一致：同输入同调度序被破坏——检查是否存在非确定性外部输入混入会话序列",
            UserSchedError::PoolDrained => "预算池未分配余额不足：注册需从池出资但池空——指定 donor 出资或先退休让额",
            UserSchedError::Internal => "内部不变量破坏：游标越界等不应发生——立即停机，本码出现即调度账本已损坏",
        }
    }
}

// ═══════════════════════════════════════════════════════════════════
// §2 用户类四级 + ClassPolicy const 策略表
// ═══════════════════════════════════════════════════════════════════

/// 用户类四级闭集（判据「优先级四级」；ordinal 越小优先级越高）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum UserClass {
    /// 交互类（前台光标/输入跟随，最高优先级，不让度）。
    Interactive,
    /// 前台类（可见窗口主渲染，让位于交互类）。
    Foreground,
    /// 后台类（离屏/预取，让位于全部前台类，享有保底红线）。
    Background,
    /// 闲时类（纯填充，无保底，让位于一切）。
    Idle,
}

impl UserClass {
    /// 四级全闭集（序 = 优先级序，ordinal 对齐）。
    pub const ALL: [UserClass; 4] = [
        UserClass::Interactive,
        UserClass::Foreground,
        UserClass::Background,
        UserClass::Idle,
    ];

    /// 类序（0 最高 3 最低；const fn，match 不走 Ord）。
    pub const fn ordinal(self) -> usize {
        match self {
            UserClass::Interactive => 0,
            UserClass::Foreground => 1,
            UserClass::Background => 2,
            UserClass::Idle => 3,
        }
    }

    /// 中文类名（诊断与留痕用）。
    pub const fn zh(self) -> &'static str {
        match self {
            UserClass::Interactive => "交互",
            UserClass::Foreground => "前台",
            UserClass::Background => "后台",
            UserClass::Idle => "闲时",
        }
    }

    /// 升一档（ordinal 减一）；已在顶类返回 None。
    pub const fn promote_of(self) -> Option<UserClass> {
        match self {
            UserClass::Foreground => Some(UserClass::Interactive),
            UserClass::Background => Some(UserClass::Foreground),
            UserClass::Idle => Some(UserClass::Background),
            UserClass::Interactive => None,
        }
    }

    /// 降一档（ordinal 加一）；已在底类返回 None。
    pub const fn demote_of(self) -> Option<UserClass> {
        match self {
            UserClass::Interactive => Some(UserClass::Foreground),
            UserClass::Foreground => Some(UserClass::Background),
            UserClass::Background => Some(UserClass::Idle),
            UserClass::Idle => None,
        }
    }

    /// 类策略（const 表直取）。
    pub const fn policy(self) -> ClassPolicy {
        CLASS_POLICY[self.ordinal()]
    }
}

/// 让度规则闭集（谁可以让给谁——策略表的一列，不散落硬编码）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum YieldRule {
    /// 不让度（交互类：输入跟随不可让）。
    Never,
    /// 仅让位于更高类（前台让交互）。
    ToHigherOnly,
    /// 让位于全部前台类（后台让交互+前台）。
    ToAllUpper,
    /// 让位于一切（闲时类：纯填充语义）。
    Always,
}

impl YieldRule {
    /// 中文说明。
    pub const fn zh(self) -> &'static str {
        match self {
            YieldRule::Never => "不让度（交互跟随不可让）",
            YieldRule::ToHigherOnly => "仅让位于交互类",
            YieldRule::ToAllUpper => "让位于交互+前台",
            YieldRule::Always => "让位于一切（纯填充）",
        }
    }
}

/// 类策略行（保底份额 ppm / DRR 配额 / 突发上限 / 让度规则）。
///
/// 保底份额语义：该类单个会话每 tick 的硬保底（先扣先授或可行性
/// 不变量保护），**份额 < 保底的注册直接拒**（份额是自报口径，
/// 保底是底线口径，自报不得穿底线）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ClassPolicy {
    /// 每 tick 硬保底（ppm；×100→单位。交互/前台由「先服务+可行性
    /// 不变量」保护，后台由「第一相位先扣先授」硬保护，闲时无保底）。
    pub floor_ppm: u64,
    /// DRR 每 tick 配额注入（单位刻度；同级公平的轮转步长）。
    pub quantum: u64,
    /// 赤字结转上限（×quantum = 单会话最大积欠补偿；防囤积）。
    pub max_burst_quanta: u64,
    /// 让度规则闭集。
    pub yield_rule: YieldRule,
}

/// 类策略 const 表（判据「const 表」本体；数值来源注释逐行给出）。
pub const CLASS_POLICY: [ClassPolicy; 4] = [
    // 交互类：保底 20%（输入跟随低于 20% 即可感知卡顿——UX 口径）；
    // 配额 2000 单位（=20% tick，一拍内交互帧足量）；突发 4 拍（滚轮
    // 惯性约 66ms）；不让度。
    ClassPolicy { floor_ppm: 200_000, quantum: 2_000, max_burst_quanta: 4, yield_rule: YieldRule::Never },
    // 前台类：保底 10%（可见窗口最低流畅线）；配额 2000 单位；突发
    // 4 拍；仅让交互。
    ClassPolicy { floor_ppm: 100_000, quantum: 2_000, max_burst_quanta: 4, yield_rule: YieldRule::ToHigherOnly },
    // 后台类：保底 2%（保底红线：前台全载时后台单会话仍得 200 单位）；
    // 配额 1000 单位（半拍步长，减少与前台的大步碰撞）；突发 4 拍；
    // 让全部前台类。
    ClassPolicy { floor_ppm: 20_000, quantum: 1_000, max_burst_quanta: 4, yield_rule: YieldRule::ToAllUpper },
    // 闲时类：无保底（0——闲时本就是让出来的）；配额 400 单位（小步
    // 填充，降低对真实负载的脉冲扰动）；突发 4 拍；让一切。
    ClassPolicy { floor_ppm: 0, quantum: 400, max_burst_quanta: 4, yield_rule: YieldRule::Always },
];

// 策略表编译期校验：保底随优先级单调不增；闲时无保底；配额正。
const _: () = assert!(CLASS_POLICY[0].floor_ppm >= CLASS_POLICY[1].floor_ppm);
const _: () = assert!(CLASS_POLICY[1].floor_ppm >= CLASS_POLICY[2].floor_ppm);
const _: () = assert!(CLASS_POLICY[2].floor_ppm >= CLASS_POLICY[3].floor_ppm);
const _: () = assert!(CLASS_POLICY[3].floor_ppm == 0);
const _: () = assert!(CLASS_POLICY[0].quantum > 0 && CLASS_POLICY[3].quantum > 0);

// ═══════════════════════════════════════════════════════════════════
// §3 会话状态机（Active → Paused → Tombstone · 墓碑 sid 不复用）
// ═══════════════════════════════════════════════════════════════════

/// 会话状态三闭集（判据「Active→Paused→Tombstone」）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SessionState {
    /// 在册可调度。
    Active,
    /// 暂停（保留份额账户，不参与轮转——配额为暂停用户保留不回收）。
    Paused,
    /// 墓碑（sid 永不复用；份额已清零并归还池/承继方）。
    Tombstone,
}

impl SessionState {
    /// 中文状态名。
    pub const fn zh(self) -> &'static str {
        match self {
            SessionState::Active => "在役",
            SessionState::Paused => "暂停",
            SessionState::Tombstone => "墓碑",
        }
    }

    /// 序（指纹/留痕用稳定编码）。
    pub const fn ordinal(self) -> u64 {
        match self {
            SessionState::Active => 0,
            SessionState::Paused => 1,
            SessionState::Tombstone => 2,
        }
    }
}

// ═══════════════════════════════════════════════════════════════════
// §4 裁决理由闭集（决策留痕：每笔裁决带理由）
// ═══════════════════════════════════════════════════════════════════

/// 裁决理由闭集（TickLedger 每笔 Decision 必带；字符串码独立命名空间
/// V_US_*，数字段 0x1310~0x131F 专属于错误闭集，两Namespace不混用）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Verdict {
    /// 后台保底先扣先授（红线相位）。
    FloorGuard,
    /// 同级 DRR 轮转常规配额。
    Drr,
    /// 被前台插队（后台/闲时相位让位后的残额轮转）。
    Preempt,
    /// 饥饿强制注入（starving 超阈值）。
    StarveInject,
    /// 收尾均填（残额逐单位轮填给未达应得者）。
    Fill,
    /// 闲时填充（Idle 类相位）。
    IdleCredit,
}

impl Verdict {
    /// 字符串理由码（V_US_* 闭集）。
    pub const fn code(self) -> &'static str {
        match self {
            Verdict::FloorGuard => "V_US_FLOOR",
            Verdict::Drr => "V_US_DRR",
            Verdict::Preempt => "V_US_PREEMPT",
            Verdict::StarveInject => "V_US_STARVE",
            Verdict::Fill => "V_US_FILL",
            Verdict::IdleCredit => "V_US_IDLE",
        }
    }

    /// 中文理由。
    pub const fn zh(self) -> &'static str {
        match self {
            Verdict::FloorGuard => "后台保底先扣先授（红线相位）",
            Verdict::Drr => "同级 DRR 轮转配额",
            Verdict::Preempt => "前台就绪插队，让位残额轮转",
            Verdict::StarveInject => "饥饿超阈值强制注入",
            Verdict::Fill => "残额逐单位收尾均填",
            Verdict::IdleCredit => "闲时类填充",
        }
    }
}

/// 单笔裁决（谁、得多少、为什么）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Decision {
    /// 会话 sid。
    pub sid: u32,
    /// 本笔实授单位数。
    pub units: u64,
    /// 裁决理由。
    pub reason: Verdict,
}

// ═══════════════════════════════════════════════════════════════════
// §5 账本环（TickLedger · 定长环 + 守恒自检）
// ═══════════════════════════════════════════════════════════════════

/// 单 tick 记账行（环账本的一格）。
#[derive(Clone, Debug)]
pub struct TickRecord {
    /// tick 序号（wrapping 单调）。
    pub tick: u64,
    /// 本 tick 全体实授合计（单位）。
    pub granted_total: u64,
    /// 本 tick 未分配（=预算池未分配份额的 tick 镜像 + 残额退回）。
    pub unassigned: u64,
    /// 本 tick 裁决留痕（超 DECISIONS_CAP 折入 folded）。
    pub decisions: [Option<Decision>; DECISIONS_CAP],
    /// 折叠计数（留痕溢出量——采样语义，不是丢账）。
    pub folded: u32,
    /// 强制注入笔数（饥饿防线动作量）。
    pub injections: u32,
    /// 守恒自检结果（Σ实授+未分配 ≡ 10000 且未透支保底预留）。
    pub conservation_ok: bool,
}

impl TickRecord {
    /// 空行（全部 None 的定长决策槽）。
    const EMPTY: TickRecord = TickRecord {
        tick: 0,
        granted_total: 0,
        unassigned: 0,
        decisions: [None; DECISIONS_CAP],
        folded: 0,
        injections: 0,
        conservation_ok: false,
    };

    /// 非空裁决条数（枚举定长槽，不用迭代器过滤计数以便 const 友好阅读）。
    pub fn decision_count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < DECISIONS_CAP {
            if self.decisions[i].is_some() {
                n += 1;
            }
            i += 1;
        }
        n
    }
}

/// tick 账本环（定长 FIFO 覆写，写满回卷挤最旧——溯源窗口恒定）。
#[derive(Debug)]
pub struct TickLedger {
    ring: Vec<TickRecord>,
    head: usize,
    filled: usize,
}

impl TickLedger {
    /// 新环（预分配 LEDGER_CAP 格）。
    pub fn new() -> Self {
        let mut ring = Vec::new();
        let mut i = 0usize;
        while i < LEDGER_CAP {
            ring.push(TickRecord::EMPTY);
            i += 1;
        }
        TickLedger { ring, head: 0, filled: 0 }
    }

    /// 推入一行（写满回卷覆写最旧）。
    pub fn push(&mut self, rec: TickRecord) {
        self.ring[self.head] = rec;
        self.head = (self.head + 1) % LEDGER_CAP;
        if self.filled < LEDGER_CAP {
            self.filled += 1;
        }
    }

    /// 已存行数。
    pub fn len(&self) -> usize {
        self.filled
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.filled == 0
    }

    /// 按最旧→最新序取第 k 行。
    pub fn get(&self, k: usize) -> Option<&TickRecord> {
        if k >= self.filled {
            return None;
        }
        let start = if self.filled < LEDGER_CAP { 0 } else { self.head };
        Some(&self.ring[(start + k) % LEDGER_CAP])
    }

    /// 全量守恒自检（环内每一行 conservation_ok 必须为真）。
    pub fn conservation_all_ok(&self) -> bool {
        let mut k = 0usize;
        while k < self.filled {
            match self.get(k) {
                Some(r) => {
                    if !r.conservation_ok {
                        return false;
                    }
                }
                None => return false,
            }
            k += 1;
        }
        true
    }

    /// 环内决策总笔数（含折叠）。
    pub fn total_decisions(&self) -> u64 {
        let mut n = 0u64;
        let mut k = 0usize;
        while k < self.filled {
            if let Some(r) = self.get(k) {
                n = n.wrapping_add(r.decision_count() as u64);
                n = n.wrapping_add(r.folded as u64);
            }
            k += 1;
        }
        n
    }

    /// 环内注入总笔数（饥饿防线动作量）。
    pub fn total_injections(&self) -> u64 {
        let mut n = 0u64;
        let mut k = 0usize;
        while k < self.filled {
            if let Some(r) = self.get(k) {
                n = n.wrapping_add(r.injections as u64);
            }
            k += 1;
        }
        n
    }
}

// ═══════════════════════════════════════════════════════════════════
// §6 会话账（UserSession · 份额/已用/欠账/饥饿/防抖计数）
// ═══════════════════════════════════════════════════════════════════

/// 会话账（判据「sid/类/份额/已用/欠账/饥饿计数/状态机」逐字段兑现）。
#[derive(Clone, Debug)]
pub struct UsSession {
    /// 会话号（单调发放，墓碑不复用；0 号保留禁用）。
    pub sid: u32,
    /// 用户类（四级；promote/demote 可变）。
    pub class: UserClass,
    /// 状态机（Active/Paused/Tombstone）。
    pub state: SessionState,
    /// 份额（ppm，100 对齐；≥类保底；池内零和持有）。
    pub share_ppm: u64,
    /// 累计实授（单位刻度；含保底/注入/填额——全部入账）。
    pub used: u64,
    /// 欠账（i64 有符号：正=欠它，负=它多拿；saturating 累积）。
    pub debt: i64,
    /// DRR 赤字（i64：≥0 常态，fill 可小幅透支至 -quantum）。
    pub deficit: i64,
    /// 饥饿计数（连续 tick 实授<应得；注入后复位；saturating）。
    pub starving: u32,
    /// 降档防抖计数（连续 demote 申请数；升档即清零）。
    pub demote_pending: u32,
    /// 注册 tick。
    pub born_tick: u64,
    /// 末次实授 tick（0=从未实授）。
    pub last_grant_tick: u64,
}

impl UsSession {
    /// 每 tick 应得（due = share/100 单位；share 100 对齐故整除精确）。
    pub fn due_units(&self) -> u64 {
        self.share_ppm / SHARE_ALIGN
    }

    /// 类保底（单位刻度）。
    pub fn floor_units(&self) -> u64 {
        CLASS_POLICY[self.class.ordinal()].floor_ppm / SHARE_ALIGN
    }

    /// 赤字结转上限（i64；= quantum × burst）。
    pub fn deficit_cap(&self) -> i64 {
        let p = CLASS_POLICY[self.class.ordinal()];
        (p.quantum.wrapping_mul(p.max_burst_quanta)) as i64
    }

    /// 赤字注入配额（每 tick 一次的 DRR top-up）。
    pub fn quantum_units(&self) -> u64 {
        CLASS_POLICY[self.class.ordinal()].quantum
    }
}

/// 会话外部快照（只读视图，供审计/展示，不暴露内部账字段别名）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SessionView {
    /// 会话号。
    pub sid: u32,
    /// 用户类。
    pub class: UserClass,
    /// 状态。
    pub state: SessionState,
    /// 份额 ppm。
    pub share_ppm: u64,
    /// 累计实授（单位）。
    pub used: u64,
    /// 欠账。
    pub debt: i64,
    /// 饥饿计数。
    pub starving: u32,
    /// 降档防抖计数。
    pub demote_pending: u32,
}

// ═══════════════════════════════════════════════════════════════════
// §7 告警账 / 聚合 / 体检 / 统计 视图结构
// ═══════════════════════════════════════════════════════════════════

/// 告警事件（0x1318 等告警码入账行）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct WarnEvent {
    /// 发生 tick。
    pub tick: u64,
    /// 告警码（0x1310~0x131F 段内）。
    pub code: u16,
    /// 关联 sid。
    pub sid: u32,
}

/// 饥饿注入告警码（复用错误闭集 StarveEsc 的段内码——告警与急限
/// 同源同码，账面唯一）。
pub const WARN_STARVE_INJECT: u16 = 0x1318;

/// 会话聚合行（实得 ppm vs 应得 ppm 的偏差审计——判据「会话聚合」）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SessionAggregate {
    /// 会话号。
    pub sid: u32,
    /// 环窗口内实授合计（单位）。
    pub granted_units: u64,
    /// 环窗口内出现在裁决中的 tick 数。
    pub active_ticks: u64,
    /// 环窗口内实得折算 ppm（granted/总量×1e6；总量 0 时记 0）。
    pub actual_ppm: u64,
    /// 应得 ppm（=当前份额）。
    pub due_ppm: u64,
    /// 偏差（actual - due；i64 可负——负即被让路，正即被补足）。
    pub deviation_ppm: i64,
}

/// 体检失败项（码 + 中文注）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct HealthFail {
    /// 诊断码（0x1310~0x131F 段内）。
    pub code: u16,
    /// 中文注（直接可操作）。
    pub note: &'static str,
}

/// 体检报告（8 项检查闭集；clean 即可继续服役）。
#[derive(Clone, Debug)]
pub struct HealthReport {
    /// 检查项总数（恒 8，扩展须同步文档）。
    pub checks_total: u32,
    /// 失败项（空 = 全绿）。
    pub failures: Vec<HealthFail>,
}

impl HealthReport {
    /// 全绿否。
    pub fn is_clean(&self) -> bool {
        self.failures.is_empty()
    }
}

/// 调度统计（生命周期累计，非环窗口）。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct SchedStats {
    /// 已走 tick 数。
    pub ticks: u64,
    /// 累计裁决笔数（含折叠）。
    pub decisions: u64,
    /// 累计饥饿注入笔数。
    pub injections: u64,
    /// 累计告警入账笔数。
    pub warnings: u64,
    /// 累计实授单位。
    pub granted_units: u64,
    /// 累计未分配单位。
    pub unassigned_units: u64,
}

/// tick 摘要（tick() 返回值：本 tick 关键量）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TickSummary {
    /// tick 序号。
    pub tick: u64,
    /// 本 tick 实授合计。
    pub granted_total: u64,
    /// 本 tick 未分配。
    pub unassigned: u64,
    /// 本 tick 留痕笔数（含折叠）。
    pub decisions: usize,
    /// 本 tick 注入笔数。
    pub injections: u32,
}

// ═══════════════════════════════════════════════════════════════════
// §8 FNV-1a 指纹（确定性对拍的账面基石）
// ═══════════════════════════════════════════════════════════════════

/// FNV-1a 64 位偏移基（标准参数）。
const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV-1a 64 位素数（标准参数）。
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// 单字混入（小端 8 字节逐字节 xor-mul）。
fn fnv_word(h: u64, v: u64) -> u64 {
    let mut x = h;
    let mut i = 0u32;
    while i < 8 {
        let byte = ((v >> (i * 8)) & 0xFF) as u64;
        x = (x ^ byte).wrapping_mul(FNV_PRIME);
        i += 1;
    }
    x
}

/// 字节串混入（版本串等）。
fn fnv_bytes(h: u64, b: &[u8]) -> u64 {
    let mut x = h;
    let mut i = 0usize;
    while i < b.len() {
        x = (x ^ (b[i] as u64)).wrapping_mul(FNV_PRIME);
        i += 1;
    }
    x
}

// ═══════════════════════════════════════════════════════════════════
// §9 调度器本体（UserScheduler）
// ═══════════════════════════════════════════════════════════════════

/// 多用户 GPU 时间片调度器（判据「调度实现」本体）。
///
/// 账面结构：
/// - `sessions`：会话账（注册序稳定；只增不删，墓碑原位冻结——
///   下标即轮转游标的稳定锚，删除会扰动轮转序破坏确定性）；
/// - `retired`：墓碑名单（sid 复用的封禁账）；
/// - `pool_unassigned`：预算池未分配余额（1e6 - Σ存活份额，恒非负）；
/// - `cursor[4]`：每类 DRR 轮转游标（存 sessions 下标，稳定序）。
pub struct UserScheduler {
    sessions: Vec<UsSession>,
    retired: Vec<u32>,
    pool_unassigned: u64,
    cursor: [usize; 4],
    ledger: TickLedger,
    warnings: Vec<WarnEvent>,
    tick_no: u64,
    next_sid: u32,
    stats: SchedStats,
}

impl UserScheduler {
    /// 新调度器（池满额未分配）。
    pub fn new() -> Self {
        UserScheduler {
            sessions: Vec::new(),
            retired: Vec::new(),
            pool_unassigned: PPM_TOTAL,
            cursor: [0usize; 4],
            ledger: TickLedger::new(),
            warnings: Vec::new(),
            tick_no: 0,
            next_sid: 1,
            stats: SchedStats::default(),
        }
    }

    // ── 查询面 ────────────────────────────────────────────────────

    /// tick 计数。
    pub fn tick_count(&self) -> u64 {
        self.tick_no
    }

    /// 生命周期统计。
    pub fn stats(&self) -> SchedStats {
        self.stats
    }

    /// 账本环引用。
    pub fn ledger(&self) -> &TickLedger {
        &self.ledger
    }

    /// 告警账切片。
    pub fn warnings(&self) -> &[WarnEvent] {
        &self.warnings
    }

    /// 存活会话数（不含墓碑）。
    pub fn live_count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.sessions.len() {
            if self.sessions[i].state != SessionState::Tombstone {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 墓碑数。
    pub fn tomb_count(&self) -> usize {
        self.retired.len()
    }

    /// 预算池未分配余额（ppm）。
    pub fn pool_free(&self) -> u64 {
        self.pool_unassigned
    }

    /// 会话快照（存活账含墓碑原位；查无返回 None）。
    pub fn session_view(&self, sid: u32) -> Option<SessionView> {
        // 墓碑不可查（存在性不泄露——墓碑就是不存在）。
        let mut i = 0usize;
        while i < self.sessions.len() {
            if self.sessions[i].sid == sid && self.sessions[i].state != SessionState::Tombstone {
                let s = &self.sessions[i];
                return Some(SessionView {
                    sid: s.sid,
                    class: s.class,
                    state: s.state,
                    share_ppm: s.share_ppm,
                    used: s.used,
                    debt: s.debt,
                    starving: s.starving,
                    demote_pending: s.demote_pending,
                });
            }
            i += 1;
        }
        None
    }

    // ── 内部定位 ──────────────────────────────────────────────────

    /// 按 sid 定位存活账下标（墓碑不计——查墓碑一律 NoSid，
    /// 存在性不泄露的镜像面：墓碑就是不存在）。
    fn find_live(&self, sid: u32) -> Result<usize, UserSchedError> {
        let mut i = 0usize;
        while i < self.sessions.len() {
            if self.sessions[i].sid == sid && self.sessions[i].state != SessionState::Tombstone {
                return Ok(i);
            }
            i += 1;
        }
        Err(UserSchedError::NoSid)
    }

    /// 池零和再校验（每次账目变动后调用——复用声明「变动再校验」）。
    fn pool_audit_inner(&self) -> Result<(), UserSchedError> {
        let mut sum = self.pool_unassigned;
        let mut i = 0usize;
        while i < self.sessions.len() {
            let s = &self.sessions[i];
            if s.state != SessionState::Tombstone {
                // 防饱和搬运引入的假和：任何一条 saturating 溢出都会
                // 让总和越界，这里统一抓出。
                sum = match sum.checked_add(s.share_ppm) {
                    Some(v) => v,
                    None => return Err(UserSchedError::ShareSum),
                };
                // 对齐与下限（份额镜像口径的两条硬性）。
                if s.share_ppm % SHARE_ALIGN != 0 || s.share_ppm < MIN_SHARE {
                    return Err(UserSchedError::ShareRange);
                }
                // 自报不穿底线：份额 ≥ 类保底。
                if s.share_ppm < CLASS_POLICY[s.class.ordinal()].floor_ppm {
                    return Err(UserSchedError::Floor);
                }
            }
            i += 1;
        }
        if sum != PPM_TOTAL {
            return Err(UserSchedError::ShareSum);
        }
        Ok(())
    }

    /// 公开池审计（复用声明的运行期再校验入口）。
    pub fn pool_audit(&self) -> Result<(), UserSchedError> {
        self.pool_audit_inner()
    }

    /// 保底可行性审计：Σ类保底（存活账）≤ 1e6 ppm（份额≥保底已保证，
    /// 此处按防线纵深原则独立复算）。
    pub fn floor_feasible(&self) -> bool {
        let mut need = 0u64;
        let mut i = 0usize;
        while i < self.sessions.len() {
            let s = &self.sessions[i];
            if s.state != SessionState::Tombstone {
                need = need.saturating_add(CLASS_POLICY[s.class.ordinal()].floor_ppm);
            }
            i += 1;
        }
        need <= PPM_TOTAL
    }

    // ── 注册 / 退休（墓碑纪律） ───────────────────────────────────

    /// 注册会话（显式 sid；墓碑 sid 拒绝复用）。
    ///
    /// 出资顺序：先从未分配池出，池不足则必须指定 donor 零和腾挪
    /// （复用 F0322 账户切分口径：没有凭空的份额）。
    pub fn register(
        &mut self,
        sid: u32,
        class: UserClass,
        share_ppm: u64,
        donor: Option<u32>,
    ) -> Result<(), UserSchedError> {
        if sid == 0 {
            return Err(UserSchedError::Arg);
        }
        if self.sessions.len() >= MAX_SESSIONS {
            return Err(UserSchedError::Capacity);
        }
        // 墓碑封禁（判据「墓碑 sid 不复用」的硬面）。
        let mut r = 0usize;
        while r < self.retired.len() {
            if self.retired[r] == sid {
                return Err(UserSchedError::SidTomb);
            }
            r += 1;
        }
        // 存活重复拒绝。
        let mut i = 0usize;
        while i < self.sessions.len() {
            if self.sessions[i].sid == sid && self.sessions[i].state != SessionState::Tombstone {
                return Err(UserSchedError::DupSid);
            }
            i += 1;
        }
        // 份额合法性：对齐 + 下限 + 自报不穿保底。
        if share_ppm % SHARE_ALIGN != 0 || share_ppm < MIN_SHARE {
            return Err(UserSchedError::ShareRange);
        }
        if share_ppm < CLASS_POLICY[class.ordinal()].floor_ppm {
            return Err(UserSchedError::Floor);
        }
        if share_ppm > PPM_TOTAL {
            return Err(UserSchedError::ShareRange);
        }
        // 出资：池优先，池不足必须 donor 零和。
        if self.pool_unassigned >= share_ppm {
            self.pool_unassigned -= share_ppm;
        } else {
            let d = match donor {
                Some(d) => d,
                None => return Err(UserSchedError::PoolDrained),
            };
            if d == sid {
                return Err(UserSchedError::Arg);
            }
            let di = self.find_live(d)?;
            let dfloor = CLASS_POLICY[self.sessions[di].class.ordinal()].floor_ppm;
            if self.sessions[di].share_ppm < share_ppm {
                return Err(UserSchedError::ShareSum);
            }
            if self.sessions[di].share_ppm - share_ppm < dfloor {
                return Err(UserSchedError::Floor);
            }
            self.sessions[di].share_ppm -= share_ppm;
        }
        self.sessions.push(UsSession {
            sid,
            class,
            state: SessionState::Active,
            share_ppm,
            used: 0,
            debt: 0,
            deficit: 0,
            starving: 0,
            demote_pending: 0,
            born_tick: self.tick_no,
            last_grant_tick: 0,
        });
        self.pool_audit_inner()?;
        Ok(())
    }

    /// 自动发号注册（单调计数器，绕过墓碑名单天然成立）。
    pub fn register_auto(
        &mut self,
        class: UserClass,
        share_ppm: u64,
        donor: Option<u32>,
    ) -> Result<u32, UserSchedError> {
        let sid = self.next_sid;
        if sid == u32::MAX {
            return Err(UserSchedError::Capacity);
        }
        self.register(sid, class, share_ppm, donor)?;
        self.next_sid = self.next_sid.wrapping_add(1);
        Ok(sid)
    }

    /// 退休会话（Active/Paused → Tombstone；份额零和归还池或承继方）。
    pub fn retire(&mut self, sid: u32, heir: Option<u32>) -> Result<(), UserSchedError> {
        let idx = self.find_live(sid)?;
        let share = self.sessions[idx].share_ppm;
        if share > 0 {
            match heir {
                Some(h) => {
                    if h == sid {
                        return Err(UserSchedError::Arg);
                    }
                    let hi = self.find_live(h)?;
                    self.sessions[hi].share_ppm = self.sessions[hi]
                        .share_ppm
                        .saturating_add(share);
                }
                None => {
                    self.pool_unassigned = self.pool_unassigned.saturating_add(share);
                }
            }
        }
        let s = &mut self.sessions[idx];
        s.state = SessionState::Tombstone;
        s.share_ppm = 0;
        s.deficit = 0;
        s.debt = 0;
        s.starving = 0;
        s.demote_pending = 0;
        self.retired.push(sid);
        self.pool_audit_inner()?;
        Ok(())
    }

    // ── 状态机（Active ↔ Paused） ─────────────────────────────────

    /// 暂停（Active → Paused；份额保留，饥饿计数清零——无需求不算饿）。
    pub fn pause(&mut self, sid: u32) -> Result<(), UserSchedError> {
        let idx = self.find_live(sid)?;
        if self.sessions[idx].state != SessionState::Active {
            return Err(UserSchedError::State);
        }
        self.sessions[idx].state = SessionState::Paused;
        self.sessions[idx].starving = 0;
        Ok(())
    }

    /// 恢复（Paused → Active）。
    pub fn resume(&mut self, sid: u32) -> Result<(), UserSchedError> {
        let idx = self.find_live(sid)?;
        if self.sessions[idx].state != SessionState::Paused {
            return Err(UserSchedError::State);
        }
        self.sessions[idx].state = SessionState::Active;
        Ok(())
    }

    // ── 优先级跃迁（滞回防抖） ────────────────────────────────────

    /// 即时升档（判据「用户切前台→即时提升」）。
    ///
    /// 滞回理由：升档是用户显式动作（点了窗口/开始交互），晚一拍就
    /// 是可感知卡顿，故**即时生效**；同时清零降档防抖计数——一拍
    /// 前台证据即推翻全部降档候选（升档误判代价低、降档误判代价高，
    /// 证据权重不对称）。已在顶类返回 Ok(false)（幂等，不报错）。
    pub fn promote(&mut self, sid: u32) -> Result<bool, UserSchedError> {
        let idx = self.find_live(sid)?;
        let cur = self.sessions[idx].class;
        let up = match cur.promote_of() {
            Some(c) => c,
            None => {
                // 顶类幂等；顶类上的 promote 请求仍是前台证据——
                // 照样清零降档防抖计数（证据权重不对称的补全面）。
                self.sessions[idx].demote_pending = 0;
                return Ok(false);
            }
        };
        // 目标类保底必须被当前份额承载（升档抬底线）。
        if self.sessions[idx].share_ppm < CLASS_POLICY[up.ordinal()].floor_ppm {
            return Err(UserSchedError::Floor);
        }
        self.sessions[idx].class = up;
        self.sessions[idx].demote_pending = 0;
        Ok(true)
    }

    /// 降档（三拍确认防抖——判据「提升有滞回防抖」的降档面）。
    ///
    /// 防抖理由：失焦/滚动停止/短暂切走常是单拍抖动，立即降档会让
    /// 交互回卡；连续 3 拍（DEMOTE_CONFIRM_TICKS）仍申请才真降。
    /// 第 3 拍到点即降并清零计数；底类（Idle）不可再降，报 BadClass。
    pub fn demote(&mut self, sid: u32) -> Result<bool, UserSchedError> {
        let idx = self.find_live(sid)?;
        let cur = self.sessions[idx].class;
        let down = match cur.demote_of() {
            Some(c) => c,
            None => return Err(UserSchedError::BadClass),
        };
        self.sessions[idx].demote_pending = self.sessions[idx].demote_pending.saturating_add(1);
        if self.sessions[idx].demote_pending >= DEMOTE_CONFIRM_TICKS {
            // 降档落底线：目标类保底 ≤ 当前类保底 ≤ 份额，恒可承载。
            self.sessions[idx].class = down;
            self.sessions[idx].demote_pending = 0;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    // ── 份额调账（零和搬运） ──────────────────────────────────────

    /// 份额零和搬运（donor → receiver，delta ppm；双方保底各自守住）。
    pub fn set_share_pair(
        &mut self,
        donor: u32,
        receiver: u32,
        delta_ppm: u64,
    ) -> Result<(), UserSchedError> {
        if delta_ppm == 0 || delta_ppm % SHARE_ALIGN != 0 {
            return Err(UserSchedError::ShareRange);
        }
        if donor == receiver {
            return Err(UserSchedError::Arg);
        }
        let di = self.find_live(donor)?;
        let ri = self.find_live(receiver)?;
        let dfloor = CLASS_POLICY[self.sessions[di].class.ordinal()].floor_ppm;
        if self.sessions[di].share_ppm < delta_ppm
            || self.sessions[di].share_ppm - delta_ppm < dfloor
        {
            return Err(UserSchedError::Floor);
        }
        self.sessions[di].share_ppm -= delta_ppm;
        self.sessions[ri].share_ppm = self.sessions[ri].share_ppm.saturating_add(delta_ppm);
        self.pool_audit_inner()?;
        Ok(())
    }

    // ── DRR 内核件 ────────────────────────────────────────────────

    /// 记账式实授（累计实授 + 末授 tick；charge=true 时扣赤字——
    /// 保底/注入是天赋权利不动赤字，DRR/fill 是信用额度照扣）。
    /// 赤字下限 -quantum（fill 慈善透支的封底，下一拍 quantum 先补洞）。
    fn grant_at(&mut self, idx: usize, units: u64, charge: bool) {
        let t = self.tick_no;
        let q = CLASS_POLICY[self.sessions[idx].class.ordinal()].quantum as i64;
        let s = &mut self.sessions[idx];
        if charge {
            let nd = s.deficit.saturating_sub(units as i64);
            s.deficit = if nd < -q { -q } else { nd };
        }
        s.used = s.used.saturating_add(units);
        s.last_grant_tick = t;
    }

    /// 挑轮转起点（members 升序；找首个 ≥ 游标的位次，无则回卷 0）。
    fn pick_start(&self, ci: usize, members: &[usize]) -> usize {
        let cur = self.cursor[ci];
        let mut p = 0usize;
        while p < members.len() {
            if members[p] >= cur {
                return p;
            }
            p += 1;
        }
        0
    }

    /// 单类 DRR 相位（判据「同级多会话轮转 DRR」）。
    ///
    /// 每 tick 每会话一次配额注入：deficit += quantum（封顶 burst），
    /// 实授 = min(赤字, 应得余量, 预算余量)，赤字即扣。轮转从类游标
    /// 起、注册序推进，授毕游标停在末位次+1（下一 tick 接力——同级
    /// 公平与轮转序稳定的双重保证）。
    fn drr_phase(
        &mut self,
        ci: usize,
        members: &[usize],
        due: &[u64],
        granted: &mut [u64],
        budget: &mut u64,
        reason: Verdict,
        decisions: &mut Vec<Decision>,
        folded: &mut u32,
    ) {
        if members.is_empty() {
            return;
        }
        let quantum = CLASS_POLICY[ci].quantum;
        let start = self.pick_start(ci, members);
        let mut k = 0usize;
        let mut last_pos: Option<usize> = None;
        while k < members.len() {
            let pos = (start + k) % members.len();
            let idx = members[pos];
            let due_left = due[idx] - granted[idx];
            if due_left > 0 && *budget > 0 {
                // 赤字注入（封顶 burst——防囤积霸榜）。
                let cap = (quantum.wrapping_mul(CLASS_POLICY[ci].max_burst_quanta)) as i64;
                let s = &mut self.sessions[idx];
                let nd = s.deficit.saturating_add(quantum as i64);
                s.deficit = if nd > cap { cap } else { nd };
                let want = min3_u64(s.deficit as u64, due_left, *budget);
                if want > 0 {
                    granted[idx] += want;
                    *budget -= want;
                    self.grant_at(idx, want, true);
                    if decisions.len() < DECISIONS_CAP {
                        decisions.push(Decision { sid: self.sessions[idx].sid, units: want, reason });
                    } else {
                        *folded = folded.wrapping_add(1);
                    }
                }
                last_pos = Some(pos);
            }
            k += 1;
            if *budget == 0 {
                break;
            }
        }
        // 游标接力（稳定轮转序：下轮从本次末位次之后起）。
        if let Some(lp) = last_pos {
            let nx = (lp + 1) % members.len();
            self.cursor[ci] = members[nx];
        }
    }

    // ── tick 主循环（六相位裁决） ─────────────────────────────────

    /// 走一拍（判据「每 tick 记账+守恒」本体）。
    ///
    /// 相位序（前台优先/后台让路的机制化）：
    /// P0 收集在役会话并算应得 →
    /// P1 后台保底先扣先授（红线相位——前台 burst 摸不到这笔）→
    /// P2 饥饿强制注入（starving 超阈值者，告警 0x1318 入账）→
    /// P3 交互类 DRR → P4 前台类 DRR →
    /// P5 后台类 DRR（前台就绪时理由记 Preempt——插队留痕）→
    /// P6 闲时类 DRR →
    /// P7 残额收尾均填（让路规则：交互/前台随时可填满应得；后台/闲时
    /// 仅当无任何前台类在役才可捡漏——残额让给前台突发窗口）→
    /// P8 记账守恒自检 + 环入账 + 饥饿/欠账结转。
    pub fn tick(&mut self) -> Result<TickSummary, UserSchedError> {
        self.tick_no = self.tick_no.wrapping_add(1);
        let n = self.sessions.len();

        // P0：在役收集（注册序稳定）与应得计算。
        let mut runnable: Vec<usize> = Vec::new();
        let mut by_class: [Vec<usize>; 4] = [Vec::new(), Vec::new(), Vec::new(), Vec::new()];
        let mut due: Vec<u64> = vec![0u64; n];
        let mut granted: Vec<u64> = vec![0u64; n];
        let mut i = 0usize;
        while i < n {
            if self.sessions[i].state == SessionState::Active {
                by_class[self.sessions[i].class.ordinal()].push(i);
                runnable.push(i);
                due[i] = self.sessions[i].due_units();
            }
            i += 1;
        }

        let mut budget = TICK_UNITS;
        let mut decisions: Vec<Decision> = Vec::new();
        let mut folded: u32 = 0;
        let mut injections: u32 = 0;

        // P1：后台保底先扣先授（红线相位；判据「保底红线永不破」）。
        // 前台类保底由「先服务 + 池可行性不变量（Σ保底≤1e6）」保护；
        // 后台保底在此硬性落地——第一笔预算先归后台。
        let bg_floor = CLASS_POLICY[UserClass::Background.ordinal()].floor_ppm / SHARE_ALIGN;
        let mut p1 = 0usize;
        while p1 < by_class[UserClass::Background.ordinal()].len() {
            let idx = by_class[UserClass::Background.ordinal()][p1];
            let room = min3_u64(bg_floor, due[idx] - granted[idx], budget);
            if room > 0 {
                granted[idx] += room;
                budget -= room;
                self.grant_at(idx, room, false);
                if decisions.len() < DECISIONS_CAP {
                    decisions.push(Decision {
                        sid: self.sessions[idx].sid,
                        units: room,
                        reason: Verdict::FloorGuard,
                    });
                } else {
                    folded = folded.wrapping_add(1);
                }
            }
            p1 += 1;
        }

        // P2：饥饿强制注入（判据「超阈值强制注入」；告警 0x1318 入账）。
        let mut ci = UserClass::Background.ordinal();
        while ci <= UserClass::Idle.ordinal() {
            let quantum = CLASS_POLICY[ci].quantum;
            let mut p2 = 0usize;
            while p2 < by_class[ci].len() {
                let idx = by_class[ci][p2];
                if self.sessions[idx].starving >= STARVE_THRESHOLD {
                    let room = min3_u64(quantum, due[idx] - granted[idx], budget);
                    if room > 0 {
                        granted[idx] += room;
                        budget -= room;
                        self.grant_at(idx, room, false);
                        injections = injections.wrapping_add(1);
                        if decisions.len() < DECISIONS_CAP {
                            decisions.push(Decision {
                                sid: self.sessions[idx].sid,
                                units: room,
                                reason: Verdict::StarveInject,
                            });
                        } else {
                            folded = folded.wrapping_add(1);
                        }
                        if self.warnings.len() >= WARN_CAP {
                            self.warnings.remove(0);
                        }
                        self.warnings.push(WarnEvent {
                            tick: self.tick_no,
                            code: WARN_STARVE_INJECT,
                            sid: self.sessions[idx].sid,
                        });
                        // 注入即复位：本轮饿意已偿，重新计数（防重复
                        // 注入同一次饿意——阈值语义是「连续未达应得」）。
                        self.sessions[idx].starving = 0;
                    }
                }
                p2 += 1;
            }
            ci += 1;
        }

        // P3/P4：交互、前台 DRR（前台就绪即插队——相位序先于后台残额）。
        let mut fg_consumed: u64 = 0;
        let mut pc = 0usize;
        while pc <= UserClass::Foreground.ordinal() {
            let members_snapshot: Vec<usize> = by_class[pc].clone();
            let before = granted_sum_of(&members_snapshot, &granted);
            self.drr_phase(
                pc,
                &members_snapshot,
                &due,
                &mut granted,
                &mut budget,
                Verdict::Drr,
                &mut decisions,
                &mut folded,
            );
            fg_consumed += granted_sum_of(&members_snapshot, &granted) - before;
            pc += 1;
        }

        // P5：后台 DRR（前台就绪且确实吃走了预算 → 理由记 Preempt）。
        let bg_reason = if fg_consumed > 0 { Verdict::Preempt } else { Verdict::Drr };
        let bg_members: Vec<usize> = by_class[UserClass::Background.ordinal()].clone();
        self.drr_phase(
            UserClass::Background.ordinal(),
            &bg_members,
            &due,
            &mut granted,
            &mut budget,
            bg_reason,
            &mut decisions,
            &mut folded,
        );

        // P6：闲时 DRR（纯填充）。
        let idle_members: Vec<usize> = by_class[UserClass::Idle.ordinal()].clone();
        self.drr_phase(
            UserClass::Idle.ordinal(),
            &idle_members,
            &due,
            &mut granted,
            &mut budget,
            Verdict::IdleCredit,
            &mut decisions,
            &mut folded,
        );

        // P7：残额收尾均填（让路规则：交互/前台随时可填满应得；后台/
        // 闲时仅当**无任何前台类在役**时才可捡漏——前台在役时残额宁可
        // 入未分配也不喂后台，这正是判据「后台让路」的机制化：后台
        // 高于保底的部分只走 DRR 限速流，残额让给前台突发窗口）。
        // 填额是残值分配不是 DRR 信用：**不动赤字**（charge=false——
        // 否则本拍捡漏会吃掉下拍配额，让路流被历史残值卡死）。
        // 逐单位挑**缺口最大**者（同类并列取注册序——逐单位轮转语义，
        // 同级公平不因捡漏顺序破功）。
        let fg_present = !by_class[UserClass::Interactive.ordinal()].is_empty()
            || !by_class[UserClass::Foreground.ordinal()].is_empty();
        let mut fill_units: Vec<u64> = vec![0u64; n];
        let mut guard = 0u64;
        while budget > 0 && guard <= TICK_UNITS * 2 {
            guard = guard.wrapping_add(1);
            let mut best: Option<usize> = None;
            let mut best_short: u64 = 0;
            let mut ord = 0usize;
            while ord < 4 {
                let eligible = if ord <= UserClass::Foreground.ordinal() {
                    true
                } else {
                    !fg_present
                };
                if eligible {
                    let mut q = 0usize;
                    while q < by_class[ord].len() {
                        let idx = by_class[ord][q];
                        let short = due[idx] - granted[idx];
                        // 严格大于才替换：同类并列时先到者（注册序）赢一拍，
                        // 下一拍其缺口变小、对手接棒——天然轮转。
                        if short > 0 && short > best_short {
                            best = Some(idx);
                            best_short = short;
                        }
                        q += 1;
                    }
                }
                ord += 1;
            }
            match best {
                Some(idx) => {
                    granted[idx] += 1;
                    budget -= 1;
                    fill_units[idx] += 1;
                    self.grant_at(idx, 1, false);
                }
                None => break, // 全员应得已满（或后台让路中），残额入未分配
            }
        }
        // 收尾填额入留痕（每会话一笔 Fill 汇总）。
        let mut fi = 0usize;
        while fi < n {
            if fill_units[fi] > 0 {
                if decisions.len() < DECISIONS_CAP {
                    decisions.push(Decision {
                        sid: self.sessions[fi].sid,
                        units: fill_units[fi],
                        reason: Verdict::Fill,
                    });
                } else {
                    folded = folded.wrapping_add(1);
                }
            }
            fi += 1;
        }

        // P8：守恒自检（判据「Σ会话份额 ≡ 10000ppm」的 tick 面）：
        // ① Σ实授 + 残额 ≡ 10000（预算不蒸发）；
        // ② 残额 ≥ 未分配池的 tick 镜像（不透支保留份额）；
        // ③ 单会话实授 ≤ 应得（不超领；fill 只补到 due 为止）。
        let granted_total = TICK_UNITS - budget;
        let unassigned_floor = self.pool_unassigned / SHARE_ALIGN;
        let mut conservation = granted_total + budget == TICK_UNITS;
        conservation = conservation && budget >= unassigned_floor;
        let mut vi = 0usize;
        while vi < n {
            if granted[vi] > due[vi] {
                conservation = false;
            }
            vi += 1;
        }
        if !conservation {
            return Err(UserSchedError::Ledger);
        }

        // 饥饿/欠账结转（在役者按应得达成度记账；非在役清零饿意）。
        let mut si = 0usize;
        while si < n {
            let s = &mut self.sessions[si];
            if s.state == SessionState::Active {
                if granted[si] < due[si] {
                    s.starving = s.starving.saturating_add(1);
                } else {
                    s.starving = 0;
                }
                let delta = (due[si] as i64).saturating_sub(granted[si] as i64);
                s.debt = s.debt.saturating_add(delta);
            } else {
                s.starving = 0;
            }
            si += 1;
        }

        // 环入账 + 生命周期统计。
        let mut slots = [None; DECISIONS_CAP];
        let mut di = 0usize;
        while di < decisions.len() && di < DECISIONS_CAP {
            slots[di] = Some(decisions[di]);
            di += 1;
        }
        self.ledger.push(TickRecord {
            tick: self.tick_no,
            granted_total,
            unassigned: budget,
            decisions: slots,
            folded,
            injections,
            conservation_ok: true,
        });
        self.stats.ticks = self.stats.ticks.wrapping_add(1);
        self.stats.decisions = self
            .stats
            .decisions
            .wrapping_add(decisions.len() as u64 + folded as u64);
        self.stats.injections = self.stats.injections.wrapping_add(injections as u64);
        self.stats.warnings = self.stats.warnings.wrapping_add(injections as u64);
        self.stats.granted_units = self.stats.granted_units.wrapping_add(granted_total);
        self.stats.unassigned_units = self.stats.unassigned_units.wrapping_add(budget);

        Ok(TickSummary {
            tick: self.tick_no,
            granted_total,
            unassigned: budget,
            decisions: decisions.len() + folded as usize,
            injections,
        })
    }

    // ── 聚合（实得 vs 应得偏差） ──────────────────────────────────

    /// 环窗口聚合（判据「各会话实得 ppm vs 应得偏差」）。
    pub fn aggregate(&self) -> Vec<SessionAggregate> {
        let mut total: u64 = 0;
        // sid → (units, ticks)；按注册序输出（不重排——确定性）。
        let mut units: Vec<u64> = Vec::new();
        let mut hits: Vec<u64> = Vec::new();
        let mut sids: Vec<u32> = Vec::new();
        let mut i = 0usize;
        while i < self.sessions.len() {
            if self.sessions[i].state != SessionState::Tombstone {
                sids.push(self.sessions[i].sid);
                units.push(0);
                hits.push(0);
            }
            i += 1;
        }
        let mut k = 0usize;
        while k < self.ledger.len() {
            if let Some(rec) = self.ledger.get(k) {
                total = total.wrapping_add(rec.granted_total);
                let mut d = 0usize;
                while d < DECISIONS_CAP {
                    if let Some(dec) = rec.decisions[d] {
                        let mut j = 0usize;
                        while j < sids.len() {
                            if sids[j] == dec.sid {
                                units[j] = units[j].wrapping_add(dec.units);
                                hits[j] = hits[j].wrapping_add(1);
                                break;
                            }
                            j += 1;
                        }
                    }
                    d += 1;
                }
            }
            k += 1;
        }
        let mut out = Vec::new();
        let mut j = 0usize;
        while j < sids.len() {
            let due_ppm = match self.session_view(sids[j]) {
                Some(v) => v.share_ppm,
                None => 0,
            };
            let actual = if total > 0 {
                units[j].saturating_mul(PPM_TOTAL) / total
            } else {
                0
            };
            out.push(SessionAggregate {
                sid: sids[j],
                granted_units: units[j],
                active_ticks: hits[j],
                actual_ppm: actual,
                due_ppm,
                deviation_ppm: actual as i64 - due_ppm as i64,
            });
            j += 1;
        }
        out
    }

    // ── 体检（8 项闭集） ──────────────────────────────────────────

    /// 全账体检（判据「体检」；clean 才可继续服役）。
    ///
    /// 8 项：①池零和 ②份额对齐+自报不穿保底 ③墓碑 sid 不复用
    /// ④环账守恒全绿 ⑤赤字有界 ⑥饥饿未破急限 ⑦sid 单调不回退
    /// ⑧轮转游标在界内。
    pub fn health(&self) -> HealthReport {
        let mut failures: Vec<HealthFail> = Vec::new();
        // ①②池零和 + 份额合法性（复用 pool_audit 逐条归码）。
        match self.pool_audit_inner() {
            Ok(()) => {}
            Err(e) => failures.push(HealthFail { code: e.code(), note: e.detail() }),
        }
        // ③墓碑不复用：retired 名单与存活账交集必空。
        let mut r = 0usize;
        while r < self.retired.len() {
            let mut i = 0usize;
            while i < self.sessions.len() {
                if self.sessions[i].sid == self.retired[r]
                    && self.sessions[i].state != SessionState::Tombstone
                {
                    failures.push(HealthFail {
                        code: UserSchedError::SidTomb.code(),
                        note: "墓碑 sid 出现在存活账——sid 复用防线被穿",
                    });
                }
                i += 1;
            }
            r += 1;
        }
        // ④环账守恒。
        if !self.ledger.conservation_all_ok() {
            failures.push(HealthFail {
                code: UserSchedError::Ledger.code(),
                note: "环账中存在守恒破行——回放定位首坏 tick",
            });
        }
        // ⑤赤字有界（0 ≤ d ≤ cap；fill 允许的负透支 ≥ -quantum）。
        let mut i = 0usize;
        while i < self.sessions.len() {
            let s = &self.sessions[i];
            if s.state != SessionState::Tombstone {
                let floor_neg = -(s.quantum_units() as i64);
                if s.deficit > s.deficit_cap() || s.deficit < floor_neg {
                    failures.push(HealthFail {
                        code: UserSchedError::Deficit.code(),
                        note: "赤字越界（超突发上限或低于 fill 透支下限）",
                    });
                }
            }
            i += 1;
        }
        // ⑥饥饿急限（注入防线每 16 tick 复位，破 64 即防线失灵）。
        let mut i = 0usize;
        while i < self.sessions.len() {
            if self.sessions[i].starving > STARVE_EMERGENCY {
                failures.push(HealthFail {
                    code: UserSchedError::StarveEsc.code(),
                    note: "饥饿超急限——注入防线失灵，停机回放",
                });
            }
            i += 1;
        }
        // ⑦sid 单调：next_sid > 一切已发 sid（墓碑含内）。
        let mut i = 0usize;
        while i < self.sessions.len() {
            if self.sessions[i].sid >= self.next_sid {
                failures.push(HealthFail {
                    code: UserSchedError::Internal.code(),
                    note: "sid 单调性破坏——发号器回退",
                });
            }
            i += 1;
        }
        // ⑧游标在界内。
        let mut ci = 0usize;
        while ci < 4 {
            if self.cursor[ci] > self.sessions.len() {
                failures.push(HealthFail {
                    code: UserSchedError::Internal.code(),
                    note: "轮转游标越界——账本损坏",
                });
            }
            ci += 1;
        }
        HealthReport { checks_total: 8, failures }
    }

    // ── 指纹与确定性对拍 ──────────────────────────────────────────

    /// FNV-1a 64 位全账指纹（确定性双跑对拍的账面基石）。
    ///
    /// 覆盖：版本串、tick、发号器、池余、全部会话账字段（含赤字与
    /// 欠账的位级）、四类游标、环账全部行、告警账。同输入同序必同值。
    pub fn fingerprint(&self) -> u64 {
        let mut h = fnv_bytes(FNV_OFFSET, USERSCHED_VERSION.as_bytes());
        h = fnv_word(h, self.tick_no);
        h = fnv_word(h, self.next_sid as u64);
        h = fnv_word(h, self.pool_unassigned);
        let mut i = 0usize;
        while i < self.sessions.len() {
            let s = &self.sessions[i];
            h = fnv_word(h, s.sid as u64);
            h = fnv_word(h, s.class.ordinal() as u64);
            h = fnv_word(h, s.state.ordinal());
            h = fnv_word(h, s.share_ppm);
            h = fnv_word(h, s.used);
            h = fnv_word(h, s.debt as u64);
            h = fnv_word(h, s.deficit as u64);
            h = fnv_word(h, s.starving as u64);
            h = fnv_word(h, s.demote_pending as u64);
            h = fnv_word(h, s.born_tick);
            h = fnv_word(h, s.last_grant_tick);
            i += 1;
        }
        let mut ci = 0usize;
        while ci < 4 {
            h = fnv_word(h, self.cursor[ci] as u64);
            ci += 1;
        }
        let mut k = 0usize;
        while k < self.ledger.len() {
            if let Some(rec) = self.ledger.get(k) {
                h = fnv_word(h, rec.tick);
                h = fnv_word(h, rec.granted_total);
                h = fnv_word(h, rec.unassigned);
                h = fnv_word(h, rec.folded as u64);
                h = fnv_word(h, rec.injections as u64);
                h = fnv_word(h, rec.conservation_ok as u64);
                let mut d = 0usize;
                while d < DECISIONS_CAP {
                    if let Some(dec) = rec.decisions[d] {
                        h = fnv_word(h, dec.sid as u64);
                        h = fnv_word(h, dec.units);
                        h = fnv_word(h, verdict_code_ord(dec.reason));
                    }
                    d += 1;
                }
            }
            k += 1;
        }
        let mut w = 0usize;
        while w < self.warnings.len() {
            h = fnv_word(h, self.warnings[w].tick);
            h = fnv_word(h, self.warnings[w].code as u64);
            h = fnv_word(h, self.warnings[w].sid as u64);
            w += 1;
        }
        h
    }

    /// 双跑对拍（判据「确定性双跑对拍」的账面入口）。
    ///
    /// 同会话序列喂两个调度器，任一时点指纹必须相等；不等即确定性
    /// 被破坏，报 Fprint（指纹即证据，不猜哪一拍坏——回放环账定位）。
    pub fn verify_pair(&self, other: &UserScheduler) -> Result<(), UserSchedError> {
        if self.fingerprint() == other.fingerprint() {
            Ok(())
        } else {
            Err(UserSchedError::Fprint)
        }
    }
}

// ── 内部小件 ─────────────────────────────────────────────────────

/// 三值最小（运行期；const 域另有 while 版纪律，不共用）。
fn min3_u64(a: u64, b: u64, c: u64) -> u64 {
    let m = if a < b { a } else { b };
    if c < m { c } else { m }
}

/// 成员集已授合计（相位前后的差量统计用）。
fn granted_sum_of(members: &[usize], granted: &[u64]) -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < members.len() {
        s += granted[members[i]];
        i += 1;
    }
    s
}

/// Verdict 稳定序编码（指纹用；字符串码不进指纹——序即账）。
fn verdict_code_ord(v: Verdict) -> u64 {
    match v {
        Verdict::FloorGuard => 0,
        Verdict::Drr => 1,
        Verdict::Preempt => 2,
        Verdict::StarveInject => 3,
        Verdict::Fill => 4,
        Verdict::IdleCredit => 5,
    }
}

// ═══════════════════════════════════════════════════════════════════
// §10 测试（判据「测试（优先级/复用两组）」——两组分立，另附确定性
//      与体检组；断言只在 cfg(test)，内核路径零 assert/panic）
// ═══════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    /// 测试前置（Result → 值；失败即用例失败，诊断码+中文进失败信息）。
    fn must<T>(r: Result<T, UserSchedError>) -> T {
        match r {
            Ok(v) => v,
            Err(e) => core::panic!("前置失败: 0x{:04X} {}", e.code(), e.detail()),
        }
    }

    /// 组一 · 优先级组 T1：前台抢占——后台独享时满应得，前台就绪后
    /// 后台回落到保底+DRR 限速流，前台拿满应得（判据「前台就绪即插队
    /// +后台让路」）。
    #[test]
    fn t1_fg_preemption() {
        let mut s = UserScheduler::new();
        let bg = must(s.register_auto(UserClass::Background, 400_000, None));
        // 前台未就绪：后台可捡漏填满应得（due 4000 单位），残额入未分配。
        let t = must(s.tick());
        assert_eq!(t.granted_total, 4_000);
        assert_eq!(t.unassigned, 6_000);
        // 前台就绪（从池出资注册，池余 600000 ≥ 500000）。
        let fg = must(s.register_auto(UserClass::Foreground, 500_000, None));
        let t = must(s.tick());
        // 让路规则生效：前台满应得 5000；后台只剩保底 200 + DRR 1000。
        assert_eq!(t.granted_total, 5_000 + 200 + 1_000);
        let agg = s.aggregate();
        let mut fg_units = 0u64;
        let mut bg_units = 0u64;
        let mut i = 0usize;
        while i < agg.len() {
            if agg[i].sid == fg {
                fg_units = agg[i].granted_units;
            }
            if agg[i].sid == bg {
                bg_units = agg[i].granted_units;
            }
            i += 1;
        }
        // 两拍窗口：前台应得 5000/拍满额；后台首拍满额 4000 + 次拍让路 1200。
        assert!(fg_units >= 5_000, "前台两拍应至少一拍满额 5000, got {}", fg_units);
        assert!(bg_units >= 4_000 + 200, "后台两拍合计≥首拍满额+次拍保底, got {}", bg_units);
    }

    /// 组一 · 优先级组 T2：保底红线——前台满载连跑多拍，后台每拍实授
    /// 恒 ≥ 保底 200 单位（红线永不破的机制面）。
    #[test]
    fn t2_floor_redline() {
        let mut s = UserScheduler::new();
        let fg = must(s.register_auto(UserClass::Foreground, 500_000, None));
        let bg = must(s.register_auto(UserClass::Background, 300_000, None));
        let mut rounds = 0u32;
        while rounds < 12 {
            let t = must(s.tick());
            assert_eq!(t.granted_total + t.unassigned, TICK_UNITS, "守恒式破坏");
            // 后台本拍实授：从裁决留痕里取（FloorGuard/DRR/Preempt/Fill 合计）。
            let rec = s.ledger().get(s.ledger().len() - 1);
            let mut bg_units = 0u64;
            if let Some(r) = rec {
                let mut d = 0usize;
                while d < DECISIONS_CAP {
                    if let Some(dec) = r.decisions[d] {
                        if dec.sid == bg {
                            bg_units += dec.units;
                        }
                    }
                    d += 1;
                }
            }
            assert!(
                bg_units >= 200,
                "后台保底红线被破: tick={} bg_units={}",
                rounds + 1,
                bg_units
            );
            let _ = fg;
            rounds += 1;
        }
    }

    /// 组一 · 优先级组 T3：饥饿注入——后台份额远低于前台负载，starving
    /// 累积 16 拍后强制注入触发，告警 0x1318 入账，注入拍后台实授
    /// 超出纯保底。
    #[test]
    fn t3_starve_injection() {
        let mut s = UserScheduler::new();
        let fg = must(s.register_auto(UserClass::Foreground, 700_000, None));
        let bg = must(s.register_auto(UserClass::Background, 200_000, None));
        let _ = fg;
        // 前 16 拍：后台只有保底 200 + DRR 1000 < 应得 2000 → starving 逐拍累积。
        let mut i = 0u32;
        while i < STARVE_THRESHOLD {
            let _ = must(s.tick());
            i += 1;
        }
        // 第 17 拍相位 P2 注入触发（结算时 starving 恰达阈值）。
        let t = must(s.tick());
        assert!(t.injections >= 1, "注入未触发: injections={}", t.injections);
        assert!(
            s.warnings().iter().any(|w| w.code == WARN_STARVE_INJECT && w.sid == bg),
            "0x1318 告警未入账"
        );
        // 注入拍后台本拍实授 > 纯保底（含注入 quantum 1000 中的一部分）。
        let rec = s.ledger().get(s.ledger().len() - 1);
        let mut bg_units = 0u64;
        if let Some(r) = rec {
            let mut d = 0usize;
            while d < DECISIONS_CAP {
                if let Some(dec) = r.decisions[d] {
                    if dec.sid == bg {
                        bg_units += dec.units;
                    }
                }
                d += 1;
            }
        }
        assert!(bg_units > 200, "注入拍后台实授应超纯保底, got {}", bg_units);
        // 注入后 starving 复位（重新计数，不重复偿同一次饿意）。
        let v = s.session_view(bg);
        assert!(v.is_some(), "bg 会话应存活");
        assert_eq!(v.map(|x| x.starving), Some(0));
    }

    /// 组一 · 优先级组 T4：滞回防抖——降档需连续 3 拍确认，前 2 拍不
    /// 生效；升档即时生效且清零防抖计数（判据「提升有滞回防抖」）。
    #[test]
    fn t4_hysteresis() {
        let mut s = UserScheduler::new();
        let sid = must(s.register_auto(UserClass::Foreground, 400_000, None));
        // 升档即时。
        assert!(must(s.promote(sid)), "首次升档应生效");
        assert_eq!(s.session_view(sid).map(|v| v.class), Some(UserClass::Interactive));
        // 顶类再升：幂等 Ok(false)。
        assert!(!must(s.promote(sid)));
        // 降档 1、2 拍：不生效，计数累积。
        assert!(!must(s.demote(sid)));
        assert!(!must(s.demote(sid)));
        assert_eq!(s.session_view(sid).map(|v| v.class), Some(UserClass::Interactive));
        assert_eq!(s.session_view(sid).map(|v| v.demote_pending), Some(2));
        // 升档证据清零防抖（Interactive 顶类 promote 幂等也算证据）。
        let _ = must(s.promote(sid));
        assert_eq!(s.session_view(sid).map(|v| v.demote_pending), Some(0));
        // 重新数满 3 拍：第 3 拍真降。
        assert!(!must(s.demote(sid)));
        assert!(!must(s.demote(sid)));
        assert!(must(s.demote(sid)), "第 3 拍确认应真降");
        assert_eq!(s.session_view(sid).map(|v| v.class), Some(UserClass::Foreground));
        // 底类再降：BadClass。
        let lo = must(s.register_auto(UserClass::Idle, 100_000, None));
        assert_eq!(s.demote(lo).unwrap_err().code(), UserSchedError::BadClass.code());
    }

    /// 组一 · 优先级组 T5：DRR 同级公平——两个同份额后台会话，多拍
    /// 窗口内实授差 ≤ 一拍配额量级（轮转+赤字结转的公平面）。
    #[test]
    fn t5_drr_fairness() {
        let mut s = UserScheduler::new();
        let a = must(s.register_auto(UserClass::Background, 200_000, None));
        let b = must(s.register_auto(UserClass::Background, 200_000, None));
        let mut i = 0u32;
        while i < 20 {
            let _ = must(s.tick());
            i += 1;
        }
        let agg = s.aggregate();
        let mut ua = 0u64;
        let mut ub = 0u64;
        let mut j = 0usize;
        while j < agg.len() {
            if agg[j].sid == a {
                ua = agg[j].granted_units;
            }
            if agg[j].sid == b {
                ub = agg[j].granted_units;
            }
            j += 1;
        }
        // 20 拍窗口：各应得 2000×20=40000；|差| ≤ 2×quantum=2000 容差。
        let diff = if ua > ub { ua - ub } else { ub - ua };
        assert!(diff <= 2_000, "同级公平被破: a={} b={} diff={}", ua, ub, diff);
    }

    /// 组二 · 复用组 T6：池零和与守恒——任意操作序列后池总和恒 1e6，
    /// 每 tick Σ实授+未分配 ≡ 10000（判据「份额切分 ppm 和恰 1e6 +
    /// 每 tick 守恒」运行期面）。
    #[test]
    fn t6_pool_conservation() {
        let mut s = UserScheduler::new();
        assert_eq!(s.pool_free(), PPM_TOTAL);
        let a = must(s.register_auto(UserClass::Interactive, 400_000, None));
        let b = must(s.register_auto(UserClass::Foreground, 300_000, None));
        let c = must(s.register_auto(UserClass::Background, 200_000, None));
        // 池余 100000 再注册 100000（无 donor）。
        let d = must(s.register_auto(UserClass::Idle, 100_000, None));
        assert_eq!(s.pool_free(), 0);
        // 池空后再注册必须 donor：池出不出 → PoolDrained。
        assert_eq!(
            s.register_auto(UserClass::Idle, 10_000, None).unwrap_err().code(),
            UserSchedError::PoolDrained.code()
        );
        // 零和腾挪后成功。
        must(s.set_share_pair(a, b, 100_000));
        let e = must(s.register_auto(UserClass::Idle, 10_000, Some(b)));
        assert!(s.pool_audit().is_ok());
        // 混合操作 30 拍，每拍守恒 + 池零和。
        must(s.pause(c));
        let mut i = 0u32;
        while i < 10 {
            let t = must(s.tick());
            assert_eq!(t.granted_total + t.unassigned, TICK_UNITS);
            assert!(s.pool_audit().is_ok());
            i += 1;
        }
        must(s.resume(c));
        must(s.retire(d, Some(a)));
        while i < 30 {
            let t = must(s.tick());
            assert_eq!(t.granted_total + t.unassigned, TICK_UNITS);
            assert!(s.pool_audit().is_ok());
            i += 1;
        }
        let _ = e;
        // 聚合偏差有界（环窗口内 |偏差| ≤ 1e6 量级 sanity）。
        let agg = s.aggregate();
        assert_eq!(agg.len(), 4); // a b c e（d 已墓碑不聚合）
    }

    /// 组二 · 复用组 T7：份额对齐与保底约束——未对齐/低于下限/穿保底
    /// 全拒，错误码各就各位（对齐镜像的字面义务）。
    #[test]
    fn t7_share_alignment() {
        let mut s = UserScheduler::new();
        // 未对齐（10150 % 100 ≠ 0）。
        assert_eq!(
            s.register_auto(UserClass::Background, 10_150, None).unwrap_err().code(),
            UserSchedError::ShareRange.code()
        );
        // 低于最小份额。
        assert_eq!(
            s.register_auto(UserClass::Background, 5_000, None).unwrap_err().code(),
            UserSchedError::ShareRange.code()
        );
        // 自报穿保底（后台保底 20000，报 10000）。
        assert_eq!(
            s.register_auto(UserClass::Background, 10_000, None).unwrap_err().code(),
            UserSchedError::Floor.code()
        );
        // 出资击穿 donor 保底（先清池逼走 donor 路径：交互保底 200000，
        // 让 200000 后剩 100000 < 保底 → Floor 拒，红线不因出资方自愿而破）。
        let a = must(s.register_auto(UserClass::Interactive, 300_000, None));
        let b = must(s.register_auto(UserClass::Foreground, 700_000, None));
        assert_eq!(s.pool_free(), 0);
        assert_eq!(
            s.register_auto(UserClass::Background, 200_000, Some(a)).unwrap_err().code(),
            UserSchedError::Floor.code()
        );
        // 零 delta / 自转账拒绝。
        assert_eq!(
            s.set_share_pair(a, b, 0).unwrap_err().code(),
            UserSchedError::ShareRange.code()
        );
        assert_eq!(
            s.set_share_pair(a, a, 1_000).unwrap_err().code(),
            UserSchedError::Arg.code()
        );
        // 合法腾挪后审计通过（donor 保底仍守住）。
        must(s.set_share_pair(a, b, 50_000));
        assert!(s.pool_audit().is_ok());
    }

    /// 组二 · 复用组 T8：墓碑纪律——退休后 sid 永不复用，自动发号
    /// 单调不回退（判据「墓碑 sid 不复用」）。
    #[test]
    fn t8_tombstone() {
        let mut s = UserScheduler::new();
        let a = must(s.register_auto(UserClass::Background, 300_000, None));
        must(s.retire(a, None));
        // 墓碑查询不存在（存在性不泄露）。
        assert!(s.session_view(a).is_none());
        // 显式复用墓碑 sid → SidTomb。
        assert_eq!(
            s.register(a, UserClass::Idle, 10_000, None).unwrap_err().code(),
            UserSchedError::SidTomb.code()
        );
        // 自动发号跳过：新号 > 墓碑号。
        let b = must(s.register_auto(UserClass::Idle, 10_000, None));
        assert!(b > a);
        // 墓碑份额已归还池。
        assert_eq!(s.pool_free(), PPM_TOTAL - 10_000);
        // 对墓碑 pause/resume → NoSid（不是 State——墓碑就是不存在）。
        assert_eq!(s.pause(a).unwrap_err().code(), UserSchedError::NoSid.code());
        assert_eq!(s.resume(a).unwrap_err().code(), UserSchedError::NoSid.code());
        // 双重退休 → NoSid。
        assert_eq!(s.retire(a, None).unwrap_err().code(), UserSchedError::NoSid.code());
    }

    /// 组三 · 确定性组 T9：双跑对拍——同会话序列喂两个调度器，各阶段
    /// 指纹一致；故意分歧后 verify_pair 报 Fprint（判据「确定性双跑
    /// 对拍（同会话序列同调度序）」）。
    #[test]
    fn t9_double_run() {
        // 跑道 A：完整脚本。
        let mut a = UserScheduler::new();
        let x = must(a.register_auto(UserClass::Foreground, 500_000, None));
        let y = must(a.register_auto(UserClass::Background, 200_000, None));
        let _ = must(a.promote(x));
        let mut i = 0u32;
        while i < 20 {
            let _ = must(a.tick());
            i += 1;
        }
        let _ = must(a.demote(y));
        let _ = must(a.tick());
        // 跑道 B：同脚本同序。
        let mut b = UserScheduler::new();
        let x2 = must(b.register_auto(UserClass::Foreground, 500_000, None));
        let y2 = must(b.register_auto(UserClass::Background, 200_000, None));
        let _ = must(b.promote(x2));
        let mut i = 0u32;
        while i < 20 {
            let _ = must(b.tick());
            i += 1;
        }
        let _ = must(b.demote(y2));
        let _ = must(b.tick());
        assert!(a.verify_pair(&b).is_ok(), "同序双跑指纹必须一致");
        // 故意分歧：B 多走一拍。
        let _ = must(b.tick());
        assert_eq!(
            a.verify_pair(&b).unwrap_err().code(),
            UserSchedError::Fprint.code()
        );
        let _ = (x, y);
    }

    /// 组三 · 体检组 T10：体检全绿 + 诊断码段闭集自检 + 环回卷。
    #[test]
    fn t10_health_and_ring() {
        let mut s = UserScheduler::new();
        let _ = must(s.register_auto(UserClass::Interactive, 400_000, None));
        let _ = must(s.register_auto(UserClass::Background, 200_000, None));
        // 40 拍 > LEDGER_CAP=96？不超；再压 60 拍共 100 > 96 触发回卷。
        let mut i = 0u32;
        while i < 100 {
            let _ = must(s.tick());
            i += 1;
        }
        assert_eq!(s.ledger().len(), LEDGER_CAP, "环必须回卷封顶");
        assert!(s.ledger().conservation_all_ok());
        let rep = s.health();
        assert!(rep.is_clean(), "体检应全绿: {:?}", rep.failures);
        assert_eq!(rep.checks_total, 8);
        // 诊断码段闭集：16 变体码全落 0x1310~0x131F 且互异。
        let all = [
            UserSchedError::NoSid,
            UserSchedError::SidTomb,
            UserSchedError::Capacity,
            UserSchedError::DupSid,
            UserSchedError::ShareSum,
            UserSchedError::ShareRange,
            UserSchedError::State,
            UserSchedError::Floor,
            UserSchedError::StarveEsc,
            UserSchedError::BadClass,
            UserSchedError::Ledger,
            UserSchedError::Arg,
            UserSchedError::Deficit,
            UserSchedError::Fprint,
            UserSchedError::PoolDrained,
            UserSchedError::Internal,
        ];
        let mut k = 0usize;
        while k < all.len() {
            assert!(all[k].code() >= DIAG_SEG_LO && all[k].code() <= DIAG_SEG_HI);
            let mut j = k + 1;
            while j < all.len() {
                assert_ne!(all[k].code(), all[j].code(), "诊断码撞码");
                assert_ne!(all[k].detail(), all[j].detail(), "诊断文重复");
                j += 1;
            }
            k += 1;
        }
    }

    /// 组三 · 状态机组 T11：暂停保留份额、恢复回归轮转；暂停者应得
    /// 转入未分配（守恒仍闭）。
    #[test]
    fn t11_pause_resume() {
        let mut s = UserScheduler::new();
        let a = must(s.register_auto(UserClass::Foreground, 600_000, None));
        let b = must(s.register_auto(UserClass::Background, 400_000, None));
        let t1 = must(s.tick());
        assert_eq!(t1.granted_total, 6_000 + 200 + 1_000); // 前台满额+后台让路流
        must(s.pause(b));
        let t2 = must(s.tick());
        // b 应得 4000 冻结入未分配；a 应得 6000 满额。
        assert_eq!(t2.unassigned, 4_000);
        assert_eq!(t2.granted_total, 6_000);
        must(s.resume(b));
        let t3 = must(s.tick());
        assert_eq!(t3.granted_total, 6_000 + 200 + 1_000); // 恢复后回到让路稳态
        let _ = a;
    }

    /// 组三 · 状态机组 T12：tick 守恒在长程混合负载下持续闭合
    /// （120 拍 > 环容量，覆盖回卷后的守恒与聚合一致性）。
    #[test]
    fn t12_long_run_conservation() {
        let mut s = UserScheduler::new();
        let i1 = must(s.register_auto(UserClass::Interactive, 300_000, None));
        let f1 = must(s.register_auto(UserClass::Foreground, 300_000, None));
        let b1 = must(s.register_auto(UserClass::Background, 200_000, None));
        let i2 = must(s.register_auto(UserClass::Idle, 200_000, None));
        let mut i = 0u32;
        while i < 120 {
            let t = must(s.tick());
            assert_eq!(t.granted_total + t.unassigned, TICK_UNITS, "tick {}", i + 1);
            if i == 60 {
                must(s.pause(f1));
            }
            if i == 90 {
                must(s.resume(f1));
                let _ = must(s.promote(i1));
                must(s.demote(b1));
            }
            i += 1;
        }
        assert!(s.health().is_clean());
        let agg = s.aggregate();
        assert_eq!(agg.len(), 4);
        let mut j = 0usize;
        while j < agg.len() {
            assert!(agg[j].due_ppm > 0);
            j += 1;
        }
        let _ = i2;
    }
}
