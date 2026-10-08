//! VE-F0048 · 垂直同步与邮箱模式（VE-A 域 · A03 同步与呈现组 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0048`
//!
//! **职责定位（锚点原文）**：垂直同步与邮箱模式——VSync 的模式管理
//! （开启/关闭/邮箱模式三档），撕裂与延迟的取舍显式（关同步省延迟但
//! 撕裂——用户自己选），模式切换原子；含模式与全屏独占的联动约束。
//! 数据结构：模式管理。
//!
//! **错误路径与降级矩阵（锚点原文，逐条落实）**：
//!
//! - 切换失败 → **回退**（[`ModeTransition`] 的回退相：任一失败都不改
//!   现役模式，旧模式继续生效，失败留痕可查）；
//! - 撕裂 → **检测 + 告知**（撕裂账逐次计数且逐次告知，不静默；关同步
//!   下撕裂是**预期**而非异常，但预期不等于免告知）；
//! - 延迟劣化 → **归因**（延迟代价按现役模式记账，归因可查）。
//!
//! **性能逐项分解**：O(1)——模式切换是常数步原子事务，撕裂账与归因账
//! 都是定容计数，无任何随帧数增长的状态。
//!
//! **跨批对接点**：V02 刷新率联动——刷新率合法性先于模式有效性裁决
//! （非法刷新一律先拒）；A48 的 [`SyncDemand`] 与 F0047 的消费口径对齐
//! （本条自持常量，不跨模块 use，见文末自持说明）。
//!
//! **无障碍与隐私**：模式状态读屏可达（[`VsyncManager::a11y_lines`]）——
//! 中英双语七行，只报模式事实与聚合计数，不报窗口标题、不泄漏单帧延迟。
//!
//! ## 设计要点（为什么这样写）
//!
//! - **三档是封闭全集**（[`SyncMode`]）：开启/关闭/邮箱三档穷举，
//!   wire 编码显式映射（`from_wire` 出 `Option`，禁 `as u8` 直转）。
//!   拍成 bool「开/关」会把邮箱档整个丢掉——邮箱不是「聪明的开」，
//!   它的延迟与撕裂特征独立成档。
//! - **取舍是表数据不是注释**（[`MODE_TRADEOFF`]）：每档的延迟代价与
//!   撕裂风险是定容表逐格写死的结论，判据侧独立重算逐格对账。「关
//!   同步省延迟但撕裂」因此可对账：唯一零排队格 = Off、唯一「撕裂
//!   预期」格 = Off，两格重合即取舍显式成立。
//! - **用户选择是选择不是错误**：手选 Off 即生效，不拦、不降级、
//!   不替用户「聪明」——但撕裂告知计数必须同步建立（[`TearLedger`]），
//!   取舍显式的另一半是「用户知情」。
//! - **模式切换是原子事务**（[`ModeTransition`]）：校验→应用→确认，
//!   应用段注入故障即回退，现役模式不变；同模式切换是 no-op 不制造
//!   假事务；非帧边界强应用被闸拒绝（[`CODE_NOT_QUIESCENT`]）。
//! - **全屏独占 × 邮箱是硬约束**：邮箱依赖合成器协调呈现，全屏独占
//!   绕过合成器，二者不可同时成立——自动退回 On 并计数告知，不静默
//!   按邮箱确认（静默会让调用方按邮箱延迟预算排帧而实际拿到 On 行为）。
//! - **零 panic 面**：wire 解码走 `Option`、计数全 `saturating_add`、
//!   表下标由枚举 `ordinal()` 生成（定容数组内）、无 unwrap/expect。
//!
//! ## 与相邻条的分工（易混，故写明）
//!
//! - **F0047** 管缓冲数怎么选；本条管呈现同步模式怎么选。F0047 消费
//!   的 [`SyncDemand`] 由本条的模式事实导出（本条是源头、F0047 是
//!   消费方），两处各持一份常量并对齐判据，不做跨模块 use。
//! - **F0051** 管全屏独占的进入/退出；本条只消费「当前是否独占」这一
//!   位，不裁决独占本身。
//! - **F0055** 管撕裂防护与审核；本条只做撕裂的检测计数与告知，
//!   防护策略不在本条。
//!
//! ## 自持说明
//!
//! [`MAILBOX_SEMANTIC_BUFFERS`] 与 F0046/F0047 的同名语义对齐（邮箱 =
//! 三缓冲），但刻意不跨模块 use：呈现组的跨条契约以「各条自持常量 +
//! 判据独立钉住」的方式防漂移，一处改档时对齐判据必红。

use crate::checks::CheckSet;

use alloc::format;
use alloc::string::String;

// ---------------------------------------------------------------------------
// 一、常量与诊断码（新域独占码段 0x48xx）
// ---------------------------------------------------------------------------

/// 三缓冲语义下界（与 F0046/F0047 的邮箱语义对齐，自持防漂移）。
pub const MAILBOX_SEMANTIC_BUFFERS: u8 = 3;

/// 撕裂账定容（环形告知窗口；计数累计不减，越界取码返 None）。
pub const TEAR_LEDGER_CAP: usize = 32;

/// 面板行数（形状漂移即红）。
pub const PANEL_LINES: usize = 7;

/// 刷新率合法下界（Hz；V02 联动）。
pub const REFRESH_MIN_HZ: u16 = 10;

/// 刷新率合法上界（Hz；V02 联动）。
pub const REFRESH_MAX_HZ: u16 = 1000;

/// 非法模式值（wire 解码失败）。
pub const CODE_BAD_MODE: PolicyCode = 0x4801;

/// 应用段注入故障（演练注入常态化：切换失败是常态不是异常）。
pub const CODE_MODE_FAULT: PolicyCode = 0x4802;

/// 非静默点（非帧边界）强应用被闸拒绝。
pub const CODE_NOT_QUIESCENT: PolicyCode = 0x4803;

/// 全屏独占 × 邮箱硬约束冲突（退回 On 的专属码，不与其他失败混同）。
pub const CODE_EXCLUSIVE_CONFLICT: PolicyCode = 0x4804;

/// 刷新率非法（V02 联动前置裁决）。
pub const CODE_BAD_REFRESH: PolicyCode = 0x4805;

/// On 档报告撕裂属异常事件（同步开启时撕裂不应发生）专属码。
pub const CODE_TEAR_ON_SYNC: PolicyCode = 0x4806;

pub type PolicyCode = u16;

// ---------------------------------------------------------------------------
// 二、三档模式（封闭全集）
// ---------------------------------------------------------------------------

/// VSync 模式三档：开启 / 关闭 / 邮箱。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyncMode {
    /// 垂直同步开启：合成器排队，零撕裂，延迟最高。
    On,
    /// 垂直同步关闭：零排队延迟最低，撕裂预期（用户自己选）。
    Off,
    /// 邮箱模式：完整帧可替换，不撕裂且延迟低于 On（依赖合成器协调）。
    Mailbox,
}

impl SyncMode {
    pub const ALL_LEN: usize = 3;

    pub const ALL: [SyncMode; 3] = [SyncMode::On, SyncMode::Off, SyncMode::Mailbox];

    pub const fn ordinal(self) -> usize {
        match self {
            SyncMode::On => 0,
            SyncMode::Off => 1,
            SyncMode::Mailbox => 2,
        }
    }

    pub const fn wire(self) -> u8 {
        match self {
            SyncMode::On => 0x01,
            SyncMode::Off => 0x02,
            SyncMode::Mailbox => 0x03,
        }
    }

    /// 线上解码显式映射（禁 `as u8` 直转，未知值出 `None` 不猜测）。
    pub const fn from_wire(w: u8) -> Option<SyncMode> {
        match w {
            0x01 => Some(SyncMode::On),
            0x02 => Some(SyncMode::Off),
            0x03 => Some(SyncMode::Mailbox),
            _ => None,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            SyncMode::On => "垂直同步开 / vsync on",
            SyncMode::Off => "垂直同步关 / vsync off",
            SyncMode::Mailbox => "邮箱模式 / mailbox",
        }
    }
}

// ---------------------------------------------------------------------------
// 三、取舍表（取舍显式：结论是数据，不是注释）
// ---------------------------------------------------------------------------

/// 撕裂风险三值（预期 > 可能 > 无——预期档的用户告知义务最重）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TearRisk {
    /// 同步开启：撕裂不应发生。
    None,
    /// 邮箱：呈现完整帧替换，常规下不撕裂；掉出邮箱窗时有撕裂可能。
    Possible,
    /// 关同步：撕裂是预期行为（每次发生都要检测 + 告知）。
    Expected,
}

impl TearRisk {
    pub const fn ordinal(self) -> usize {
        match self {
            TearRisk::None => 0,
            TearRisk::Possible => 1,
            TearRisk::Expected => 2,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            TearRisk::None => "无撕裂 / no tear",
            TearRisk::Possible => "可能撕裂 / possible tear",
            TearRisk::Expected => "撕裂预期 / tear expected",
        }
    }
}

/// 取舍表一格：延迟代价（合成器排队折算，微秒当量/帧）+ 撕裂风险 + 理由。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ModeTradeoff {
    /// 延迟代价当量（On = 满一帧排队；Mailbox = 半帧当量；Off = 零排队）。
    pub latency_us_per_frame: u32,
    /// 撕裂风险。
    pub tear_risk: TearRisk,
    /// 理由码（可查可对账，不是 bool）。
    pub reason: Reason,
}

/// 取舍理由（八值封闭集；归因与建议逐值对应）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reason {
    /// 开启同步：零撕裂优先，付满一帧排队。
    OnZeroTear,
    /// 关闭同步：零排队优先，撕裂预期（用户自己选）。
    OffZeroQueue,
    /// 邮箱：不撕裂且延迟低于 On（完整帧可替换）。
    MailboxBalanced,
    /// 用户手选覆盖自动推荐。
    UserOverride,
    /// 独占×邮箱硬约束退回 On。
    ExclusiveFallback,
}

impl Reason {
    pub const ALL_LEN: usize = 5;

    pub const fn ordinal(self) -> usize {
        match self {
            Reason::OnZeroTear => 0,
            Reason::OffZeroQueue => 1,
            Reason::MailboxBalanced => 2,
            Reason::UserOverride => 3,
            Reason::ExclusiveFallback => 4,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Reason::OnZeroTear => "开同步零撕裂 / on: zero tear, full queue",
            Reason::OffZeroQueue => "关同步零排队 / off: zero queue, tear expected",
            Reason::MailboxBalanced => "邮箱平衡 / mailbox: no tear, low latency",
            Reason::UserOverride => "用户手选 / user override",
            Reason::ExclusiveFallback => "独占退回 / exclusive fallback",
        }
    }
}

/// 取舍表本体：`[SyncMode::ordinal()]` 定容三格，**表驱动不是 if 链**。
///
/// - On：满一帧排队当量（16667/60Hz 折算口径由调用方换算），撕裂无。
/// - Off：零排队，撕裂预期——唯一「省延迟」格与唯一「撕裂预期」格重合，
///   即锚点「关同步省延迟但撕裂——用户自己选」的可对账形式。
/// - Mailbox：半帧当量，常规不撕裂（掉出邮箱窗降为可能）。
pub const MODE_TRADEOFF: [ModeTradeoff; 3] = [
    ModeTradeoff {
        latency_us_per_frame: 16_667,
        tear_risk: TearRisk::None,
        reason: Reason::OnZeroTear,
    },
    ModeTradeoff {
        latency_us_per_frame: 0,
        tear_risk: TearRisk::Expected,
        reason: Reason::OffZeroQueue,
    },
    ModeTradeoff {
        latency_us_per_frame: 8_334,
        tear_risk: TearRisk::Possible,
        reason: Reason::MailboxBalanced,
    },
];

/// 查表（O(1)：下标由枚举 `ordinal()` 生成，恒在定容数组内，零 panic 面）。
pub const fn mode_tradeoff(m: SyncMode) -> ModeTradeoff {
    MODE_TRADEOFF[m.ordinal()]
}

// ---------------------------------------------------------------------------
// 四、全屏独占联动约束（消费 F0051 的一位，不裁决独占本身）
// ---------------------------------------------------------------------------

/// 全屏独占状态（本条只消费，不裁决）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Exclusivity {
    /// 非独占（合成器在路径上，三档皆可用）。
    None,
    /// 全屏独占（绕过合成器：邮箱不可用，退 On）。
    Exclusive,
}

impl Exclusivity {
    pub const fn ordinal(self) -> usize {
        match self {
            Exclusivity::None => 0,
            Exclusivity::Exclusive => 1,
        }
    }
}

// ---------------------------------------------------------------------------
// 五、模式切换原子事务（校验→应用→确认；失败回退，现役不变）
// ---------------------------------------------------------------------------

/// 切换相。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransitionPhase {
    /// 校验中（约束与刷新率裁决在此相完成）。
    Assess,
    /// 已应用（新模式生效，一步原子替换）。
    Applied,
    /// 已回退（旧模式继续生效；失败留痕）。
    RolledBack,
}

impl TransitionPhase {
    pub const fn ordinal(self) -> usize {
        match self {
            TransitionPhase::Assess => 0,
            TransitionPhase::Applied => 1,
            TransitionPhase::RolledBack => 2,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            TransitionPhase::Assess => "校验 / assess",
            TransitionPhase::Applied => "已应用 / applied",
            TransitionPhase::RolledBack => "已回退 / rolled back",
        }
    }
}

/// 原子切换事务：从 [`VsyncManager::plan_switch`] 取出，逐步推进。
pub struct ModeTransition {
    from: SyncMode,
    to: SyncMode,
    phase: TransitionPhase,
    at_boundary: bool,
    fault_injected: bool,
    rollback_code: Option<PolicyCode>,
}

impl ModeTransition {
    /// 计划一次切换（校验相前置：约束冲突在此即判，不出半成品事务）。
    ///
    /// - 同模式：`Err(CODE_NOT_QUIESCENT)` 语义不符，故返回 `Ok(None)`
    ///   表示 no-op（调用方不制造假事务）；
    /// - 独占 × 邮箱：`Err(CODE_EXCLUSIVE_CONFLICT)`（裁决退回由管理器
    ///   显式走 fallback 路径，不藏在 plan 里）。
    pub fn plan(
        from: SyncMode,
        to: SyncMode,
        exclusivity: Exclusivity,
        at_boundary: bool,
    ) -> Result<Option<ModeTransition>, PolicyCode> {
        if from == to {
            return Ok(None);
        }
        if exclusivity == Exclusivity::Exclusive && to == SyncMode::Mailbox {
            return Err(CODE_EXCLUSIVE_CONFLICT);
        }
        Ok(Some(ModeTransition {
            from,
            to,
            phase: TransitionPhase::Assess,
            at_boundary,
            fault_injected: false,
            rollback_code: None,
        }))
    }

    pub fn phase(&self) -> TransitionPhase {
        self.phase
    }

    pub fn from(&self) -> SyncMode {
        self.from
    }

    pub fn to(&self) -> SyncMode {
        self.to
    }

    pub fn rollback_code(&self) -> Option<PolicyCode> {
        self.rollback_code
    }

    /// 演练注入：应用段故障（注入必须可见，判据钉「注入即留痕」）。
    pub fn inject_apply_fault(&mut self) {
        self.fault_injected = true;
    }

    /// 推进一步。返回新相。
    ///
    /// - Assess → Applied：仅在帧边界（静默点）允许；非边界强应用被闸
    ///   拒绝（现役不变，不产生半事务）；
    /// - Assess → RolledBack：注入故障或非边界，原因入 `rollback_code`；
    /// - Applied/RolledBack 为终相，step 恒定。
    pub fn step(&mut self) -> TransitionPhase {
        match self.phase {
            TransitionPhase::Assess => {
                if self.fault_injected {
                    self.phase = TransitionPhase::RolledBack;
                    self.rollback_code = Some(CODE_MODE_FAULT);
                } else if !self.at_boundary {
                    self.phase = TransitionPhase::RolledBack;
                    self.rollback_code = Some(CODE_NOT_QUIESCENT);
                } else {
                    self.phase = TransitionPhase::Applied;
                }
                self.phase
            }
            other => other,
        }
    }
}

// ---------------------------------------------------------------------------
// 六、撕裂账（检测 + 告知；关同步下撕裂是预期但免告知不成立）
// ---------------------------------------------------------------------------

/// 撕裂账：逐次计数、逐次告知，环形定容窗口。
pub struct TearLedger {
    detected_total: u32,
    reported_total: u32,
    /// 环形告知窗（最近 TEAR_LEDGER_CAP 条：帧号回执）。
    window: [Option<u64>; TEAR_LEDGER_CAP],
    head: usize,
    /// On 档撕裂异常事件计数（同步开启撕裂不应发生，单独归因）。
    on_sync_anomalies: u32,
}

impl TearLedger {
    pub const fn new() -> TearLedger {
        TearLedger {
            detected_total: 0,
            reported_total: 0,
            window: [None; TEAR_LEDGER_CAP],
            head: 0,
            on_sync_anomalies: 0,
        }
    }

    /// 报告一次撕裂（携带帧号回执）。返回告知是否成立（恒 true：检测
    /// 即告知，检测与告知 1:1，不静默吞）。
    pub fn report(&mut self, mode: SyncMode, frame: u64) -> Result<(), PolicyCode> {
        self.detected_total = self.detected_total.saturating_add(1);
        self.reported_total = self.reported_total.saturating_add(1);
        self.window[self.head] = Some(frame);
        self.head = (self.head + 1) % TEAR_LEDGER_CAP;
        if mode == SyncMode::On {
            // 同步开启下撕裂不应发生：照常告知（检测即告知），并单列
            // 异常计数供归因——不是把异常混进常规账里。
            self.on_sync_anomalies = self.on_sync_anomalies.saturating_add(1);
            return Err(CODE_TEAR_ON_SYNC);
        }
        Ok(())
    }

    pub fn detected_total(&self) -> u32 {
        self.detected_total
    }

    pub fn reported_total(&self) -> u32 {
        self.reported_total
    }

    pub fn on_sync_anomalies(&self) -> u32 {
        self.on_sync_anomalies
    }

    /// 窗口内最近一条回执（越界取码返回 None；窗口只留最近痕迹但计数
    /// 累计不减）。
    pub fn last_receipt(&self) -> Option<u64> {
        let idx = if self.head == 0 {
            TEAR_LEDGER_CAP - 1
        } else {
            self.head - 1
        };
        self.window[idx]
    }
}

// ---------------------------------------------------------------------------
// 七、模式管理器（现役模式 + 切换 + 归因账 + 读屏面板）
// ---------------------------------------------------------------------------

/// VSync 模式管理器。
pub struct VsyncManager {
    mode: SyncMode,
    refresh_hz: u16,
    exclusive: Exclusivity,
    ledger: TearLedger,
    switches_applied: u32,
    switches_rolled_back: u32,
    exclusive_fallbacks: u32,
    off_informed: u32,
    latency_attributed: [u32; 3],
    last_reason: Option<Reason>,
}

impl VsyncManager {
    pub const fn new() -> VsyncManager {
        VsyncManager {
            mode: SyncMode::On,
            refresh_hz: 60,
            exclusive: Exclusivity::None,
            ledger: TearLedger::new(),
            switches_applied: 0,
            switches_rolled_back: 0,
            exclusive_fallbacks: 0,
            off_informed: 0,
            latency_attributed: [0; 3],
            last_reason: None,
        }
    }

    pub fn mode(&self) -> SyncMode {
        self.mode
    }

    pub fn last_reason(&self) -> Option<Reason> {
        self.last_reason
    }

    pub fn switches_applied(&self) -> u32 {
        self.switches_applied
    }

    pub fn switches_rolled_back(&self) -> u32 {
        self.switches_rolled_back
    }

    pub fn exclusive_fallbacks(&self) -> u32 {
        self.exclusive_fallbacks
    }

    pub fn off_informed(&self) -> u32 {
        self.off_informed
    }

    pub fn ledger(&self) -> &TearLedger {
        &self.ledger
    }

    /// V02 联动：刷新率更新（合法性前置裁决，非法即拒不改值）。
    pub fn set_refresh_hz(&mut self, hz: u16) -> Result<(), PolicyCode> {
        if hz < REFRESH_MIN_HZ || hz > REFRESH_MAX_HZ {
            return Err(CODE_BAD_REFRESH);
        }
        self.refresh_hz = hz;
        Ok(())
    }

    pub fn refresh_hz(&self) -> u16 {
        self.refresh_hz
    }

    /// F0051 联动：独占状态更新（本条只消费这一位）。
    pub fn set_exclusivity(&mut self, e: Exclusivity) {
        self.exclusive = e;
    }

    /// 发起一次模式切换（原子事务入口）。
    ///
    /// - 同模式 no-op：返回 `Ok(None)`，不制造假事务；
    /// - 独占 × 邮箱：裁决**退回 On**并计数告知（专属码），不静默按
    ///   邮箱确认——调用方按邮箱延迟预算排帧而实际拿到 On 行为是最
    ///   隐蔽的一类失配；
    /// - 事务由调用方 `step()` 推进，`record_transition` 回填现役档。
    pub fn plan_switch(
        &mut self,
        to: SyncMode,
        at_boundary: bool,
    ) -> Result<Option<ModeTransition>, PolicyCode> {
        let t = ModeTransition::plan(self.mode, to, self.exclusive, at_boundary)?;
        if t.is_none() {
            return Ok(None);
        }
        let t = t.unwrap_or_else(|| ModeTransition {
            from: self.mode,
            to,
            phase: TransitionPhase::Assess,
            at_boundary,
            fault_injected: false,
            rollback_code: None,
        });
        if self.exclusive == Exclusivity::Exclusive && to == SyncMode::Off {
            // 独占下关同步合法（合成器已绕过）。
        }
        if to == SyncMode::Off {
            // 关同步的撕裂告知义务在任何路径上都同步建立（含独占）——
            // 用户知情不因独占豁免。
            self.off_informed = self.off_informed.saturating_add(1);
        }
        Ok(Some(t))
    }

    /// 独占 × 邮箱的显式 fallback：退 On、计数、理由可查。
    pub fn exclusive_mailbox_fallback(&mut self) -> ModeTransition {
        self.exclusive_fallbacks = self.exclusive_fallbacks.saturating_add(1);
        ModeTransition {
            from: SyncMode::Mailbox,
            to: SyncMode::On,
            phase: TransitionPhase::Assess,
            at_boundary: true,
            fault_injected: false,
            rollback_code: Some(CODE_EXCLUSIVE_CONFLICT),
        }
    }

    /// 事务结果回填（applied 才改现役模式；回退留痕）。
    pub fn record_transition(&mut self, applied: bool, to: SyncMode) {
        if applied {
            self.switches_applied = self.switches_applied.saturating_add(1);
            self.mode = to;
            self.latency_attributed[to.ordinal()] =
                self.latency_attributed[to.ordinal()].saturating_add(1);
            self.last_reason = Some(mode_tradeoff(to).reason);
        } else {
            self.switches_rolled_back = self.switches_rolled_back.saturating_add(1);
        }
    }

    /// 延迟劣化归因：当前现役模式的延迟当量（查表 O(1)）。
    pub fn latency_us(&self) -> u32 {
        mode_tradeoff(self.mode).latency_us_per_frame
    }

    /// 撕裂报告入口（转发撕裂账；关同步下逐次告知）。
    pub fn report_tear(&mut self, frame: u64) -> Result<(), PolicyCode> {
        self.ledger.report(self.mode, frame)
    }

    /// 读屏面板（七行双语，只报模式事实与聚合计数）。
    pub fn a11y_lines(&self) -> [String; PANEL_LINES] {
        let t = mode_tradeoff(self.mode);
        [
            format!("现役模式 / active mode: {}", self.mode.label()),
            format!("延迟当量 / latency equivalent: {} us", t.latency_us_per_frame),
            format!("撕裂风险 / tear risk: {}", t.tear_risk.label()),
            format!(
                "切换计数 / switches: 成功 {}，回退 {}",
                self.switches_applied, self.switches_rolled_back
            ),
            format!(
                "撕裂账 / tears: 检测 {}，告知 {}，On档异常 {}",
                self.ledger.detected_total(),
                self.ledger.reported_total(),
                self.ledger.on_sync_anomalies()
            ),
            format!(
                "独占退回与关同步告知 / exclusive fallbacks & off notices: {} 与 {}",
                self.exclusive_fallbacks, self.off_informed
            ),
            format!(
                "刷新率 / refresh: {} Hz（合法 {}..={}）",
                self.refresh_hz, REFRESH_MIN_HZ, REFRESH_MAX_HZ
            ),
        ]
    }
}

// ---------------------------------------------------------------------------
// 八、域自检（判据逐条映射锚点：三档模式/取舍显式/用户选择/原子切换/判据）
// ---------------------------------------------------------------------------

/// F0048 域自检入口（聚合器调用；零 panic 面，失败逐条红不炸域）。
pub fn run_vea48_checks() -> CheckSet {
    let mut s = CheckSet::new("vea48_vsync");

    // --- 判据 1：三档封闭全集——wire 往返 + label 非空 + 未知值拒绝 ---
    {
        let mut ok = true;
        let mut i = 0usize;
        while i < SyncMode::ALL_LEN {
            let m = SyncMode::ALL[i];
            if SyncMode::from_wire(m.wire()) != Some(m) {
                ok = false;
            }
            if m.label().is_empty() {
                ok = false;
            }
            i += 1;
        }
        s.add(
            "A48-三档-wire往返且label非空且未知值拒",
            ok
                && SyncMode::from_wire(0).is_none()
                && SyncMode::from_wire(0x04).is_none()
                && SyncMode::from_wire(0xFF).is_none()
                && SyncMode::ALL_LEN == 3,
            "三档 wire 编码显式映射往返一致；0x00/0x04/0xFF 全部解码为 None（禁 as u8 直转）",
        );
    }

    // --- 判据 2：取舍表显式——判据侧独立重算逐格对账 + 唯一格断言 ---
    {
        // 独立重算期望表（不读 MODE_TRADEOFF，逐格字面写死）。
        let expect_lat: [u32; 3] = [16_667, 0, 8_334];
        let expect_risk: [usize; 3] = [0, 2, 1]; // None / Expected / Possible
        let mut ok = true;
        let mut i = 0usize;
        while i < SyncMode::ALL_LEN {
            let e = mode_tradeoff(SyncMode::ALL[i]);
            if e.latency_us_per_frame != expect_lat[i]
                || e.tear_risk.ordinal() != expect_risk[i]
            {
                ok = false;
            }
            i += 1;
        }
        // 取舍显式的可对账形式：唯一零排队格 = Off，唯一撕裂预期格 = Off，
        // 两格必须重合（重合即「关同步省延迟但撕裂」成立且可审计）。
        let zero_queue = MODE_TRADEOFF[SyncMode::Off.ordinal()].latency_us_per_frame == 0;
        let zero_queue_unique = MODE_TRADEOFF[SyncMode::On.ordinal()].latency_us_per_frame > 0
            && MODE_TRADEOFF[SyncMode::Mailbox.ordinal()].latency_us_per_frame > 0;
        let expected_unique = MODE_TRADEOFF[SyncMode::On.ordinal()].tear_risk.ordinal()
            != TearRisk::Expected.ordinal()
            && MODE_TRADEOFF[SyncMode::Mailbox.ordinal()].tear_risk.ordinal()
                != TearRisk::Expected.ordinal();
        s.add(
            "A48-取舍表-独立重算对账且唯一格重合",
            ok && zero_queue && zero_queue_unique && expected_unique,
            "三档延迟当量与撕裂风险逐格与独立重算一致；唯一零排队格与唯一撕裂预期格都=Off（取舍显式可对账）",
        );
    }

    // --- 判据 3：用户选择——手选 Off 即生效且撕裂告知义务同步建立 ---
    {
        let mut a = VsyncManager::new();
        let t = a.plan_switch(SyncMode::Off, true);
        let applied = match t {
            Ok(Some(mut x)) => {
                let p1 = x.step();
                let done = p1 == TransitionPhase::Applied;
                a.record_transition(done, x.to());
                done
            }
            _ => false,
        };
        s.add(
            "A48-用户选择-关同步生效且告知义务建立",
            applied
                && a.mode() == SyncMode::Off
                && a.off_informed() == 1
                && a.last_reason().map(|r| r.ordinal()) == Some(Reason::OffZeroQueue.ordinal())
                && mode_tradeoff(SyncMode::Off).latency_us_per_frame == 0,
            "手选关同步即生效（不拦不降级），撕裂告知计数同步建立（用户知情），理由=零排队撕裂预期",
        );
    }

    // --- 判据 4：原子切换——计划→应用→回填现役；同模式 no-op ---
    {
        let mut a = VsyncManager::new();
        let noop = matches!(a.plan_switch(SyncMode::On, true), Ok(None));
        let ok = match a.plan_switch(SyncMode::Mailbox, true) {
            Ok(Some(mut x)) => {
                let p = x.step();
                x.from() == SyncMode::On
                    && x.to() == SyncMode::Mailbox
                    && p == TransitionPhase::Applied
                    && x.phase() == TransitionPhase::Applied
                    && x.rollback_code().is_none()
            }
            _ => false,
        };
        a.record_transition(true, SyncMode::Mailbox);
        s.add(
            "A48-原子切换-一步应用且同模式no-op",
            noop && ok && a.mode() == SyncMode::Mailbox && a.switches_applied() == 1,
            "同模式计划返回 no-op 不制造假事务；On→邮箱一步到 Applied（原子），回填后现役=邮箱、成功计数 1",
        );
    }

    // --- 判据 5：切换失败→回退——注入故障回退且现役模式不变 ---
    {
        let mut a = VsyncManager::new();
        let rolled = match a.plan_switch(SyncMode::Off, true) {
            Ok(Some(mut x)) => {
                x.inject_apply_fault();
                let p = x.step();
                p == TransitionPhase::RolledBack && x.rollback_code() == Some(CODE_MODE_FAULT)
            }
            _ => false,
        };
        a.record_transition(false, SyncMode::Off);
        s.add(
            "A48-回退-注入故障回退且现役不变",
            rolled
                && a.mode() == SyncMode::On
                && a.switches_rolled_back() == 1
                && a.switches_applied() == 0,
            "应用段注入故障 ⇒ RolledBack 且回滚码=MODE_FAULT；回填后现役仍是 On、回退计数 1（切换失败→回退）",
        );
    }

    // --- 判据 6：非静默点强应用被闸拒绝 ---
    {
        let mut a = VsyncManager::new();
        let rejected = match a.plan_switch(SyncMode::Off, false) {
            Ok(Some(mut x)) => {
                let p = x.step();
                p == TransitionPhase::RolledBack && x.rollback_code() == Some(CODE_NOT_QUIESCENT)
            }
            _ => false,
        };
        s.add(
            "A48-静默点闸-非帧边界强应用被拒",
            rejected,
            "非帧边界（非静默点）应用被闸拒绝并回退（原因=NOT_QUIESCENT），不产生半事务",
        );
    }

    // --- 判据 7：独占 × 邮箱硬约束——计划拒绝 + 显式退回计数 ---
    {
        let mut a = VsyncManager::new();
        a.set_exclusivity(Exclusivity::Exclusive);
        // 独占下计划邮箱（On→邮箱）：plan 直接拒（专属码，约束检查
        // 先于应用，从任意现役档发起都必须被拒）。
        let plan_rejected = matches!(
            a.plan_switch(SyncMode::Mailbox, true),
            Err(CODE_EXCLUSIVE_CONFLICT)
        );
        // 显式 fallback：退 On、计数、留痕。
        let mut fb = a.exclusive_mailbox_fallback();
        let p = fb.step();
        let fallback_ok = p == TransitionPhase::Applied;
        a.record_transition(fallback_ok, SyncMode::On);
        s.add(
            "A48-独占约束-邮箱被拒且退回留痕",
            plan_rejected
                && fallback_ok
                && a.mode() == SyncMode::On
                && a.exclusive_fallbacks() == 1
                && CODE_EXCLUSIVE_CONFLICT != CODE_BAD_MODE,
            "全屏独占×邮箱计划即拒（专属码）；显式 fallback 退 On 并计数留痕（不静默按邮箱确认）",
        );
    }

    // --- 判据 8：撕裂→检测+告知——1:1 且 On 档异常单列 ---
    {
        let mut a = VsyncManager::new();
        // Off 档：撕裂预期，逐次检测逐次告知。
        a.record_transition(true, SyncMode::Off);
        let r1 = a.report_tear(100);
        let r2 = a.report_tear(101);
        // On 档：撕裂不应发生，照常告知但异常单列 + 专属码。
        a.record_transition(true, SyncMode::On);
        let r3 = a.report_tear(102);
        let last = a.ledger().last_receipt();
        s.add(
            "A48-撕裂账-检测告知一一对应且On异常单列",
            r1.is_ok() && r2.is_ok() && r3 == Err(CODE_TEAR_ON_SYNC)
                && a.ledger().detected_total() == 3
                && a.ledger().reported_total() == 3
                && a.ledger().on_sync_anomalies() == 1
                && last == Some(102),
            "检测与告知 1:1（3 检测 3 告知，不静默）；On 档撕裂异常单列计数并出专属码；窗口回执留最近帧号",
        );
    }

    // --- 判据 9：延迟劣化→归因——按模式记账可查 ---
    {
        let a = VsyncManager::new();
        let lat_on = a.latency_us();
        // 归因表逐模式字面重算（判据侧写死，不读表自证）。
        let expect: [u32; 3] = [16_667, 0, 8_334];
        let mut i = 0usize;
        let mut table_ok = true;
        while i < SyncMode::ALL_LEN {
            if mode_tradeoff(SyncMode::ALL[i]).latency_us_per_frame != expect[i] {
                table_ok = false;
            }
            i += 1;
        }
        s.add(
            "A48-延迟归因-按现役模式记账",
            lat_on == 16_667 && table_ok,
            "现役模式的延迟当量即归因结论（On=满帧排队）；归因表判据侧逐格字面重算对账",
        );
    }

    // --- 判据 10：V02 刷新率联动——合法性前置裁决 ---
    {
        let mut a = VsyncManager::new();
        let bad_low = a.set_refresh_hz(REFRESH_MIN_HZ - 1) == Err(CODE_BAD_REFRESH);
        let bad_high = a.set_refresh_hz(REFRESH_MAX_HZ + 1) == Err(CODE_BAD_REFRESH);
        let edge_low = a.set_refresh_hz(REFRESH_MIN_HZ).is_ok();
        let edge_high = a.set_refresh_hz(REFRESH_MAX_HZ).is_ok();
        let bad_mid = a.set_refresh_hz(0) == Err(CODE_BAD_REFRESH);
        s.add(
            "A48-刷新率联动-边界即合法且非法先拒",
            bad_low && bad_high && edge_low && edge_high && bad_mid
                && REFRESH_MIN_HZ == 10 && REFRESH_MAX_HZ == 1000,
            "恰在下界/上界即合法（贴线不误拒），出界与 0 一律先拒（非法刷新先于模式有效性裁决）",
        );
    }

    // --- 判据 11：读屏面板——七行双语逐行绑定聚合量 ---
    {
        let mut a = VsyncManager::new();
        a.record_transition(true, SyncMode::Off);
        let _ = a.report_tear(7);
        let lines = a.a11y_lines();
        let all_nonempty = lines.iter().all(|l| !l.is_empty());
        s.add(
            "A48-面板-七行双语且逐行绑定聚合量",
            lines.len() == PANEL_LINES
                && all_nonempty
                && lines[0].contains("vsync off")
                && lines[1].contains("0 us")
                && lines[2].contains("tear expected")
                && lines[3].contains("成功 1")
                && lines[4].contains("检测 1")
                && lines[5].contains("0")
                && lines[6].contains("60 Hz")
                && lines.iter().all(|l| l.chars().any(|c| c.is_ascii_alphabetic())),
            "面板七行逐行绑定：现役 off / 延迟 0 / 撕裂预期 / 切换成功 1 / 撕裂检测 1 / 退回与告知 / 刷新率 60，每行双语",
        );
    }

    // --- 判据 12：接缝——完整链（计划→应用→回填→撕裂→面板一致） ---
    {
        let mut a = VsyncManager::new();
        let chain = match a.plan_switch(SyncMode::Mailbox, true) {
            Ok(Some(mut x)) => {
                let p1 = x.step();
                a.record_transition(p1 == TransitionPhase::Applied, x.to());
                p1 == TransitionPhase::Applied
                    && a.mode() == SyncMode::Mailbox
                    && a.latency_us() == 8_334
            }
            _ => false,
        };
        // 接缝后模式事实与 F0047 消费口径对齐：邮箱 ⇒ 三缓冲语义下界。
        let semantic_ok = a.mode() == SyncMode::Mailbox && MAILBOX_SEMANTIC_BUFFERS == 3;
        let lines = a.a11y_lines();
        s.add(
            "A48-接缝-决策经过渡真更新现役且语义对齐",
            chain && semantic_ok && lines[0].contains("mailbox"),
            "完整链：计划→应用→回填后现役=邮箱、延迟当量=8334（查表）；邮箱模式与三缓冲语义下界（F0046/F0047 口径）对齐",
        );
    }

    s
}
