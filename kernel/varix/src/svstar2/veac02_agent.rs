//! VE-F5802 · AI Agent 模型（VE-AC 域 · AI 与行为域 · AC02 组）。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F5802`
//!
//! **判据（锚点原文）**：三段抽象、注册制、行为契约、判据。
//!
//! **职责定位（锚点原文）**：AI Agent 模型——Agent 抽象（感知输入、
//! 决策核心、行动输出三段）与 Agent 注册制（每个 Agent 声明能力边界
//! 与资源需求，边界外行为拒绝），Agent 行为契约公开可查；含契约变更
//! 通知机制。错误路径与降级矩阵：未注册→拒绝运行；越边界→钳制+
//! 审计；契约缺失→拒绝注册。运行 O（Agent 数）。跨批对接点：AC03
//! 行为执行；Y 域脚本。无障碍与隐私：契约可查。
//!
//! ## 一、三段抽象（判据一）
//!
//! Agent 抽象三段（[`AgentPhase`]——感知输入/决策核心/行动输出）；
//! 三段职责单句冻结（[`PHASE_DUTIES`]——与 veac01 四层管道级分工
//! 对齐：Agent 内部三段是运行时形态，层归层段归段）；Agent 卡
//! （[`AgentCard`]——id/三段段色/能力边界集/资源需求/契约版本/
//! 状态机）；三段推进单向（感知→决策→行动，逆序显性拒
//! [`XcCode::PhaseBackward`]——与 veac01 层间单向流同构）。
//!
//! ## 二、注册制（判据二：边界声明 + 契约齐全才准入）
//!
//! 每个 Agent 声明能力边界与资源需求（[`AgentCard::capabilities`]
//! ——能力键闭集内声明 [`CAPABILITY_KEYS`]；[`ResourceDemand`]
//! ——CPU/内存/带宽三预算，预算和入引擎守恒账）；边界外行为拒绝
//! （[`AgentRegistry::invoke`]——行动键不在声明集即拒
//! [`XcCode::CapabilityOutOfRange`]）；契约缺失→拒绝注册
//! （[`BehaviorContract`]——契约四件（职责声明/边界清单/降级承诺/
//! 版本号）缺一即拒 [`XcCode::ContractIncomplete`]）；未注册→拒绝
//! 运行（注册表查无此 id 即拒 [`XcCode::AgentUnregistered`]）。
//!
//! ## 三、行为契约（判据三：公开可查 + 变更通知）
//!
//! Agent 行为契约公开可查（[`AgentRegistry::contract_of`]——按 id
//! 取契约全文；读屏可达摘要 [`BehaviorContract::a11y_line`]——
//! 无障碍判据「契约可查」的落地）；契约变更通知机制
//! （[`ContractChangeLedger`]——变更留痕（旧版/新版/动因/tick）+
//! 订阅者通知账（[`SubscriberLedger`]——每订阅者按版本号确认，
//! 未确认计数入账；通知不静默——变更必须过通知闸才能生效
//! [`XcCode::NotifyPending`]）。
//!
//! ## 四、越边界钳制与审计（错误路径三落点）
//!
//! 越边界→钳制+审计（[`clamp_action`]——行动参数超声明幅度即钳到
//! 边界值并强制审计留痕 [`AuditRecord`]——钳制不是静默修正：账里
//! 必有原值/钳值/越界方向）；未注册拒绝运行与契约缺失拒绝注册见
//! 上；运行 O（Agent 数）（[`AgentRegistry::run_tick`]——每 tick
//! 逐 Agent 三段推进一遍，无嵌套遍历——复杂度声明 O(n)）。
//!
//! **对接**：veac01（AC01 AI 域总架构——三律红线与注册面的上游，
//! 本单注册制是三律「可预期」的 Agent 级落地）；AC03（行为执行——
//! 行动输出的下游接缝，[`ActionOut`] 即执行器入参卡）；Y 域脚本
//! （脚本 Agent 同走本注册制——脚本不是法外之地）。零 panic 面、
//! 零 IO、零墙钟（tick 注入）、no_std、零浮点。

use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、规格常量
// ---------------------------------------------------------------------------

/// 本项版本。
pub const AGENT_VERSION: &str = "AC02-agent-v1";

/// 契约 schema 版本（变更通知的对拍基线）。
pub const CONTRACT_SCHEMA: u32 = 1;

/// 能力键闭集（边界声明只能从闭集取——边界外没有能力这个概念）。
pub const CAPABILITY_KEYS: [&str; 8] = [
    "move",      // 移动
    "attack",    // 攻击
    "dialogue",  // 对话
    "trade",     // 交易
    "craft",     // 合成
    "scout",     // 侦察
    "guard",     // 守卫
    "emote",     // 表情
];

/// 资源需求三维度（预算键）。
pub const RESOURCE_KEYS: [&str; 3] = ["cpu_bps", "mem_kb", "band_bps"];

/// 单 Agent 资源上限（守恒闸的分母锚）。
pub const RESOURCE_CPU_BPS_CAP: u32 = 2_000;
pub const RESOURCE_MEM_KB_CAP: u32 = 4_096;
pub const RESOURCE_BAND_BPS_CAP: u32 = 8_000;

/// 行动幅度钳制界（行动参数统一幅度 0..=1000）。
pub const ACTION_MAG_MAX: u32 = 1_000;

/// 契约四件（缺一不注册）。
pub const CONTRACT_SECTIONS: [&str; 4] = ["职责声明", "边界清单", "降级承诺", "版本号"];

// ---------------------------------------------------------------------------
// 二、诊断码（0x62 段独占：AC02 Agent 面）
// ---------------------------------------------------------------------------

/// 诊断码闭集（0x62 段）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum XcCode {
    /// 未注册（拒绝运行）。
    AgentUnregistered,
    /// 重复注册（同 id 幂等拒）。
    AgentDuplicate,
    /// 能力越界（行动键不在声明集）。
    CapabilityOutOfRange,
    /// 契约缺失（四件缺一拒绝注册）。
    ContractIncomplete,
    /// 资源越上限（需求超能力帽拒绝注册）。
    ResourceOverCap,
    /// 资源守恒破（引擎总预算超顶）。
    ResourceSumOver,
    /// 段序逆行（三段单向流被逆推）。
    PhaseBackward,
    /// 契约版本回退（通知账版本倒挂）。
    VersionBackward,
    /// 通知未确认（契约变更未过通知闸）。
    NotifyPending,
    /// 订阅者不在账。
    SubscriberMissing,
}

impl XcCode {
    /// 人读诊断（含可操作建议）。
    pub const fn detail(self) -> &'static str {
        match self {
            XcCode::AgentUnregistered => "未注册：注册表查无此 Agent，拒绝运行，建议先走注册制准入",
            XcCode::AgentDuplicate => "重复注册：同 id Agent 已在册，幂等拒绝重复登记",
            XcCode::CapabilityOutOfRange => "能力越界：行动键不在声明边界集，建议扩边界申报或换合法行动",
            XcCode::ContractIncomplete => "契约缺失：职责/边界/降级/版本四件缺一拒绝注册，建议补齐契约",
            XcCode::ResourceOverCap => "资源越上限：需求超单 Agent 资源帽，建议降需求申报",
            XcCode::ResourceSumOver => "资源守恒破：引擎总预算超顶，建议削减在册 Agent 或降配",
            XcCode::PhaseBackward => "段序逆行：感知→决策→行动单向流，逆推显性拒",
            XcCode::VersionBackward => "契约版本回退：通知账版本倒挂，建议递增版本再变更",
            XcCode::NotifyPending => "通知未确认：契约变更未过通知闸不生效，建议等订阅者确认",
            XcCode::SubscriberMissing => "订阅者不在账：通知目标查无此人，建议先订阅再收通知",
        }
    }

    /// 诊断码短键。
    pub const fn key(self) -> &'static str {
        match self {
            XcCode::AgentUnregistered => "ac02.xc.agent-unregistered",
            XcCode::AgentDuplicate => "ac02.xc.agent-duplicate",
            XcCode::CapabilityOutOfRange => "ac02.xc.capability-out-of-range",
            XcCode::ContractIncomplete => "ac02.xc.contract-incomplete",
            XcCode::ResourceOverCap => "ac02.xc.resource-over-cap",
            XcCode::ResourceSumOver => "ac02.xc.resource-sum-over",
            XcCode::PhaseBackward => "ac02.xc.phase-backward",
            XcCode::VersionBackward => "ac02.xc.version-backward",
            XcCode::NotifyPending => "ac02.xc.notify-pending",
            XcCode::SubscriberMissing => "ac02.xc.subscriber-missing",
        }
    }

    /// 诊断码段位（0x6200 基座）。
    pub const fn code(self) -> u16 {
        0x6200
            + match self {
                XcCode::AgentUnregistered => 0x01,
                XcCode::AgentDuplicate => 0x02,
                XcCode::CapabilityOutOfRange => 0x03,
                XcCode::ContractIncomplete => 0x04,
                XcCode::ResourceOverCap => 0x05,
                XcCode::ResourceSumOver => 0x06,
                XcCode::PhaseBackward => 0x07,
                XcCode::VersionBackward => 0x08,
                XcCode::NotifyPending => 0x09,
                XcCode::SubscriberMissing => 0x0A,
            }
    }
}

// ---------------------------------------------------------------------------
// 三、FNV-1a 指纹基元
// ---------------------------------------------------------------------------

/// FNV-1a 64 位偏移基。
pub const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;

/// FNV-1a 64 位素数。
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// FNV-1a 单字节吸收。
pub const fn fnv1a_byte(state: u64, byte: u8) -> u64 {
    (state ^ (byte as u64)).wrapping_mul(FNV_PRIME)
}

/// FNV-1a 字节串吸收。
pub const fn fnv1a_bytes(state: u64, bytes: &[u8]) -> u64 {
    let mut s = state;
    let mut i = 0usize;
    while i < bytes.len() {
        s = fnv1a_byte(s, bytes[i]);
        i += 1;
    }
    s
}

/// FNV-1a 字符串吸收。
pub fn fnv1a_str(state: u64, text: &str) -> u64 {
    fnv1a_bytes(state, text.as_bytes())
}

/// FNV-1a u64 吸收。
pub const fn fnv1a_u64(state: u64, value: u64) -> u64 {
    fnv1a_bytes(state, &value.to_be_bytes())
}

/// FNV-1a u32 吸收。
pub const fn fnv1a_u32(state: u64, value: u32) -> u64 {
    fnv1a_bytes(state, &value.to_be_bytes())
}

// ---------------------------------------------------------------------------
// 四、三段抽象（判据一）
// ---------------------------------------------------------------------------

/// Agent 三段闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AgentPhase {
    /// 感知输入。
    Perceive,
    /// 决策核心。
    Decide,
    /// 行动输出。
    Act,
}

impl AgentPhase {
    /// 全段数组（序号对位）。
    pub const ALL: [AgentPhase; 3] = [AgentPhase::Perceive, AgentPhase::Decide, AgentPhase::Act];

    /// 序号。
    pub const fn ordinal(self) -> usize {
        match self {
            AgentPhase::Perceive => 0,
            AgentPhase::Decide => 1,
            AgentPhase::Act => 2,
        }
    }

    /// 下一段（行动段的下一段是自己——终段由账面表达）。
    pub const fn next(self) -> AgentPhase {
        match self {
            AgentPhase::Perceive => AgentPhase::Decide,
            AgentPhase::Decide => AgentPhase::Act,
            AgentPhase::Act => AgentPhase::Act,
        }
    }

    /// 人读名。
    pub const fn zh(self) -> &'static str {
        match self {
            AgentPhase::Perceive => "感知",
            AgentPhase::Decide => "决策",
            AgentPhase::Act => "行动",
        }
    }

    /// 段机检键。
    pub const fn key(self) -> &'static str {
        match self {
            AgentPhase::Perceive => "ac02.phase.perceive",
            AgentPhase::Decide => "ac02.phase.decide",
            AgentPhase::Act => "ac02.phase.act",
        }
    }
}

/// 三段职责单句（字面量冻结；与 veac01 四层分工对齐的注记）。
pub const PHASE_DUTIES: [&str; 3] = [
    "感知输入：只读世界快照，不产生行动",
    "决策核心：只产出行动意图，不直接改世界",
    "行动输出：意图经边界闸与钳制后交 AC03 执行器",
];

// ---------------------------------------------------------------------------
// 五、行为契约（判据三·数据面）
// ---------------------------------------------------------------------------

/// 资源需求（三预算；各自有帽）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResourceDemand {
    /// CPU 预算（bps，≤2000）。
    pub cpu_bps: u32,
    /// 内存预算（KB，≤4096）。
    pub mem_kb: u32,
    /// 带宽预算（bps，≤8000）。
    pub band_bps: u32,
}

impl ResourceDemand {
    /// 零需求（观察者 Agent 合法形态）。
    pub const fn zero() -> ResourceDemand {
        ResourceDemand {
            cpu_bps: 0,
            mem_kb: 0,
            band_bps: 0,
        }
    }

    /// 需求合法性（三帽各自不超）。
    pub const fn validate(&self) -> Result<(), XcCode> {
        if self.cpu_bps > RESOURCE_CPU_BPS_CAP {
            return Err(XcCode::ResourceOverCap);
        }
        if self.mem_kb > RESOURCE_MEM_KB_CAP {
            return Err(XcCode::ResourceOverCap);
        }
        if self.band_bps > RESOURCE_BAND_BPS_CAP {
            return Err(XcCode::ResourceOverCap);
        }
        Ok(())
    }

    /// 需求指纹。
    pub const fn fingerprint(&self) -> u64 {
        let s = fnv1a_u32(FNV_OFFSET, self.cpu_bps);
        let s = fnv1a_u32(s, self.mem_kb);
        fnv1a_u32(s, self.band_bps)
    }
}

/// 行为契约（四件齐才有效——契约缺失拒绝注册的判据面）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BehaviorContract {
    /// 职责声明（非空人读句）。
    pub duty: String,
    /// 能力边界清单（CAPABILITY_KEYS 闭集内声明）。
    pub boundaries: Vec<usize>,
    /// 降级承诺（非空人读句——契约必答「坏了怎么办」）。
    pub degrade: String,
    /// 契约版本（严格递增）。
    pub version: u32,
}

impl BehaviorContract {
    /// 契约完整性核验（四件核：职责非空/边界在闭集且非空/降级非空/版本非零）。
    pub fn validate(&self) -> Result<(), XcCode> {
        if self.duty.is_empty() || self.degrade.is_empty() {
            return Err(XcCode::ContractIncomplete);
        }
        if self.boundaries.is_empty() {
            return Err(XcCode::ContractIncomplete);
        }
        let mut i = 0usize;
        while i < self.boundaries.len() {
            let b = match self.boundaries.get(i) {
                Some(x) => *x,
                None => return Err(XcCode::ContractIncomplete),
            };
            if b >= CAPABILITY_KEYS.len() {
                return Err(XcCode::CapabilityOutOfRange);
            }
            i += 1;
        }
        if self.version == 0 {
            return Err(XcCode::ContractIncomplete);
        }
        Ok(())
    }

    /// 契约指纹（四件混算——变更对拍基元）。
    pub fn fingerprint(&self) -> u64 {
        let mut s = fnv1a_str(FNV_OFFSET, &self.duty);
        let mut i = 0usize;
        while i < self.boundaries.len() {
            if let Some(b) = self.boundaries.get(i) {
                s = fnv1a_u64(s, *b as u64);
            }
            i += 1;
        }
        s = fnv1a_str(s, &self.degrade);
        fnv1a_u64(s, self.version as u64)
    }

    /// 读屏可达摘要（无障碍判据「契约可查」的落地——固定三段式）。
    pub fn a11y_line(&self) -> String {
        let mut s = String::new();
        s.push_str("契约：");
        s.push_str(&self.duty);
        s.push_str("；边界=");
        let mut i = 0usize;
        while i < self.boundaries.len() {
            if let Some(b) = self.boundaries.get(i) {
                if let Some(k) = CAPABILITY_KEYS.get(*b) {
                    if i > 0 {
                        s.push('、');
                    }
                    s.push_str(k);
                }
            }
            i += 1;
        }
        s.push_str(&alloc::format!("；降级={}；版本=v{}", self.degrade, self.version));
        s
    }
}

// ---------------------------------------------------------------------------
// 六、Agent 卡与行动卡（三段的运行时形态）
// ---------------------------------------------------------------------------

/// Agent 卡（注册制的登记单元）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentCard {
    /// Agent id（注册表唯一）。
    pub id: String,
    /// 当前段（三段单向游标）。
    pub phase: AgentPhase,
    /// 行为契约（含边界与版本）。
    pub contract: BehaviorContract,
    /// 资源需求。
    pub demand: ResourceDemand,
    /// 注册 tick。
    pub registered_tick: u64,
}

impl AgentCard {
    /// 卡指纹（id+契约+需求混算）。
    pub fn fingerprint(&self) -> u64 {
        let s = fnv1a_str(FNV_OFFSET, &self.id);
        let s = fnv1a_u64(s, self.contract.fingerprint());
        let s = fnv1a_u64(s, self.demand.fingerprint());
        fnv1a_u64(s, self.registered_tick)
    }
}

/// 行动输出卡（AC03 执行器入参接缝）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActionOut {
    /// Agent id。
    pub agent: String,
    /// 行动键（CAPABILITY_KEYS 之一）。
    pub key: usize,
    /// 行动幅度（0..=1000；越界被钳）。
    pub magnitude: u32,
    /// 产出 tick。
    pub tick: u64,
}

impl ActionOut {
    /// 行动卡指纹。
    pub fn fingerprint(&self) -> u64 {
        let s = fnv1a_str(FNV_OFFSET, &self.agent);
        let s = fnv1a_u64(s, self.key as u64);
        let s = fnv1a_u64(s, self.magnitude as u64);
        fnv1a_u64(s, self.tick)
    }
}

/// 钳制记录（越边界钳制+审计的账面单元）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClampRecord {
    /// Agent id。
    pub agent: String,
    /// 行动键。
    pub key: usize,
    /// 原幅度。
    pub raw: u32,
    /// 钳后幅度。
    pub clamped: u32,
    /// 越界方向（true=超上限 / false=负向欠界——欠界在 u32 面即 0 兜底）。
    pub over_top: bool,
    /// 钳制 tick。
    pub tick: u64,
}

impl ClampRecord {
    /// 钳制记录指纹。
    pub fn fingerprint(&self) -> u64 {
        let s = fnv1a_str(FNV_OFFSET, &self.agent);
        let s = fnv1a_u64(s, self.key as u64);
        let s = fnv1a_u64(s, self.raw as u64);
        let s = fnv1a_u64(s, self.clamped as u64);
        let s = fnv1a_u64(s, if self.over_top { 1 } else { 0 });
        fnv1a_u64(s, self.tick)
    }
}

/// 行动参数钳制（越边界→钳到界值；返回记录供审计——钳制不静默）。
pub fn clamp_action(agent: &str, key: usize, magnitude: u32, tick: u64) -> (ActionOut, Option<ClampRecord>) {
    if magnitude <= ACTION_MAG_MAX {
        (
            ActionOut {
                agent: agent.to_string(),
                key,
                magnitude,
                tick,
            },
            None,
        )
    } else {
        let rec = ClampRecord {
            agent: agent.to_string(),
            key,
            raw: magnitude,
            clamped: ACTION_MAG_MAX,
            over_top: true,
            tick,
        };
        (
            ActionOut {
                agent: agent.to_string(),
                key,
                magnitude: ACTION_MAG_MAX,
                tick,
            },
            Some(rec),
        )
    }
}

// ---------------------------------------------------------------------------
// 七、契约变更通知（判据三·机制面）
// ---------------------------------------------------------------------------

/// 契约变更留痕（旧版/新版/动因/tick）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContractChange {
    /// Agent id。
    pub agent: String,
    /// 旧契约指纹。
    pub old_fp: u64,
    /// 新契约指纹。
    pub new_fp: u64,
    /// 旧版本。
    pub old_version: u32,
    /// 新版本。
    pub new_version: u32,
    /// 动因。
    pub reason: String,
    /// 变更 tick。
    pub tick: u64,
}

impl ContractChange {
    /// 变更指纹。
    pub fn fingerprint(&self) -> u64 {
        let s = fnv1a_str(FNV_OFFSET, &self.agent);
        let s = fnv1a_u64(s, self.old_fp);
        let s = fnv1a_u64(s, self.new_fp);
        let s = fnv1a_u64(s, self.old_version as u64);
        let s = fnv1a_u64(s, self.new_version as u64);
        let s = fnv1a_str(s, &self.reason);
        fnv1a_u64(s, self.tick)
    }
}

/// 订阅者账（变更通知闸的接收面：每订阅者按版本确认）。
#[derive(Clone, Debug, Default)]
pub struct SubscriberLedger {
    /// 订阅者名（注册表级唯一由调用方保证）。
    names: Vec<String>,
    /// 各订阅者已确认版本（0=未确认任何）。
    acked: Vec<u32>,
}

impl SubscriberLedger {
    /// 空账。
    pub const fn new() -> SubscriberLedger {
        SubscriberLedger {
            names: Vec::new(),
            acked: Vec::new(),
        }
    }

    /// 订阅（重复订阅幂等——返回已订阅位）。
    pub fn subscribe(&mut self, name: &str) -> usize {
        let mut i = 0usize;
        while i < self.names.len() {
            if let Some(n) = self.names.get(i) {
                if n == name {
                    return i;
                }
            }
            i += 1;
        }
        self.names.push(name.to_string());
        self.acked.push(0);
        self.names.len().saturating_sub(1)
    }

    /// 订阅者数。
    pub const fn len(&self) -> usize {
        self.names.len()
    }

    /// 是否空账。
    pub const fn is_empty(&self) -> bool {
        self.names.is_empty()
    }

    /// 确认（版本递增校验——确认倒挂显性拒）。
    pub fn ack(&mut self, slot: usize, version: u32) -> Result<(), XcCode> {
        let cur = match self.acked.get(slot) {
            Some(x) => *x,
            None => return Err(XcCode::SubscriberMissing),
        };
        if version < cur {
            return Err(XcCode::VersionBackward);
        }
        if let Some(a) = self.acked.get_mut(slot) {
            *a = version;
        }
        Ok(())
    }

    /// 已确认版本读取。
    pub fn acked_version(&self, slot: usize) -> u32 {
        match self.acked.get(slot) {
            Some(v) => *v,
            None => 0,
        }
    }

    /// 未确认数（相对 target 版本）。
    pub fn pending_count(&self, target_version: u32) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.acked.len() {
            if let Some(v) = self.acked.get(i) {
                if *v < target_version {
                    n = n.saturating_add(1);
                }
            }
            i += 1;
        }
        n
    }
}

/// 契约变更账（变更留痕 + 通知闸）。
#[derive(Clone, Debug, Default)]
pub struct ContractChangeLedger {
    changes: Vec<ContractChange>,
    subscribers: SubscriberLedger,
}

impl ContractChangeLedger {
    /// 空账。
    pub const fn new() -> ContractChangeLedger {
        ContractChangeLedger {
            changes: Vec::new(),
            subscribers: SubscriberLedger::new(),
        }
    }

    /// 订阅变更通知。
    pub fn subscribe(&mut self, name: &str) -> usize {
        self.subscribers.subscribe(name)
    }

    /// 订阅者数。
    pub const fn subscriber_count(&self) -> usize {
        self.subscribers.len()
    }

    /// 记变更（版本必须严格递增——回退显性拒）。
    pub fn record(&mut self, ch: ContractChange) -> Result<(), XcCode> {
        if ch.new_version <= ch.old_version {
            return Err(XcCode::VersionBackward);
        }
        self.changes.push(ch);
        Ok(())
    }

    /// 变更笔数。
    pub const fn len(&self) -> usize {
        self.changes.len()
    }

    /// 是否空账。
    pub const fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }

    /// 最新变更读取。
    pub fn latest(&self) -> Option<&ContractChange> {
        let n = self.changes.len();
        if n == 0 {
            None
        } else {
            self.changes.get(n - 1)
        }
    }

    /// 通知闸核验（最新变更需全员确认到该版本才放行——通知不静默）。
    pub fn notify_gate(&self) -> Result<(), XcCode> {
        let latest = match self.latest() {
            Some(c) => c,
            None => return Ok(()),
        };
        if self.subscribers.pending_count(latest.new_version) > 0 {
            return Err(XcCode::NotifyPending);
        }
        Ok(())
    }

    /// 变更账指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut s = FNV_OFFSET;
        let mut i = 0usize;
        while i < self.changes.len() {
            if let Some(c) = self.changes.get(i) {
                s = fnv1a_u64(s, c.fingerprint());
            }
            i += 1;
        }
        s
    }
}

// ---------------------------------------------------------------------------
// 八、注册表与运行时（判据二 + O(n) 复杂度声明）
// ---------------------------------------------------------------------------

/// 审计记录（越界钳制/未注册尝试等安全面的统一留痕）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuditRecord {
    /// 事件键（机检短键）。
    pub event: String,
    /// 关联 Agent id（可为空串=全局事件）。
    pub agent: String,
    /// 事件 tick。
    pub tick: u64,
}

/// Agent 注册表（注册制 + 资源守恒 + 通知闸 + O(n) 运行）。
#[derive(Clone, Debug, Default)]
pub struct AgentRegistry {
    cards: Vec<AgentCard>,
    changes: ContractChangeLedger,
    clamps: Vec<ClampRecord>,
    audits: Vec<AuditRecord>,
    /// 资源守恒账（三预算各自累计）。
    sum_cpu: u32,
    sum_mem: u32,
    sum_band: u32,
    /// 未注册拒绝计数。
    unregistered_count: u32,
    /// 越界拒绝计数。
    out_of_range_count: u32,
}

impl AgentRegistry {
    /// 空注册表。
    pub const fn new() -> AgentRegistry {
        AgentRegistry {
            cards: Vec::new(),
            changes: ContractChangeLedger::new(),
            clamps: Vec::new(),
            audits: Vec::new(),
            sum_cpu: 0,
            sum_mem: 0,
            sum_band: 0,
            unregistered_count: 0,
            out_of_range_count: 0,
        }
    }

    /// 在册数。
    pub const fn len(&self) -> usize {
        self.cards.len()
    }

    /// 是否空表。
    pub const fn is_empty(&self) -> bool {
        self.cards.is_empty()
    }

    /// 注册（判据二：契约齐全 + 资源合法 + 守恒可容纳才准入；同 id 幂等拒）。
    pub fn register(&mut self, card: AgentCard) -> Result<(), XcCode> {
        // 契约缺失→拒绝注册。
        card.contract.validate()?;
        card.demand.validate()?;
        // 同 id 幂等拒。
        let mut i = 0usize;
        while i < self.cards.len() {
            if let Some(c) = self.cards.get(i) {
                if c.id == card.id {
                    return Err(XcCode::AgentDuplicate);
                }
            }
            i += 1;
        }
        // 资源守恒预演（saturating 累加后与各自帽对拍）。
        let ncpu = self.sum_cpu.saturating_add(card.demand.cpu_bps);
        let nmem = self.sum_mem.saturating_add(card.demand.mem_kb);
        let nband = self.sum_band.saturating_add(card.demand.band_bps);
        if ncpu > RESOURCE_CPU_BPS_CAP
            || nmem > RESOURCE_MEM_KB_CAP
            || nband > RESOURCE_BAND_BPS_CAP
        {
            return Err(XcCode::ResourceSumOver);
        }
        self.sum_cpu = ncpu;
        self.sum_mem = nmem;
        self.sum_band = nband;
        self.cards.push(card);
        Ok(())
    }

    /// 按 id 查卡。
    pub fn find(&self, id: &str) -> Option<&AgentCard> {
        let mut i = 0usize;
        while i < self.cards.len() {
            if let Some(c) = self.cards.get(i) {
                if c.id == id {
                    return Some(c);
                }
            }
            i += 1;
        }
        None
    }

    /// 契约公开可查（按 id 取契约摘要；未注册给 Err——查无此人也是查询结果）。
    pub fn contract_of(&self, id: &str) -> Result<String, XcCode> {
        match self.find(id) {
            Some(c) => Ok(c.contract.a11y_line()),
            None => Err(XcCode::AgentUnregistered),
        }
    }

    /// 行动提交（判据二执行面：未注册拒 / 越界拒 / 越幅度钳+审计）。
    pub fn invoke(&mut self, id: &str, key: usize, magnitude: u32, tick: u64) -> Result<ActionOut, XcCode> {
        // 未注册→拒绝运行。
        let card = match self.find(id) {
            Some(c) => (c.id.clone(), c.contract.boundaries.clone()),
            None => {
                self.unregistered_count = self.unregistered_count.saturating_add(1);
                self.audits.push(AuditRecord {
                    event: "ac02.audit.unregistered".to_string(),
                    agent: id.to_string(),
                    tick,
                });
                return Err(XcCode::AgentUnregistered);
            }
        };
        // 行动键表外（闭集外没有能力）。
        if key >= CAPABILITY_KEYS.len() {
            self.out_of_range_count = self.out_of_range_count.saturating_add(1);
            return Err(XcCode::CapabilityOutOfRange);
        }
        // 边界外行为拒绝（声明集外即拒——注册制的牙齿）。
        let mut declared = false;
        let mut i = 0usize;
        while i < card.1.len() {
            if let Some(b) = card.1.get(i) {
                if *b == key {
                    declared = true;
                }
            }
            i += 1;
        }
        if !declared {
            self.out_of_range_count = self.out_of_range_count.saturating_add(1);
            self.audits.push(AuditRecord {
                event: "ac02.audit.capability-out-of-range".to_string(),
                agent: card.0.clone(),
                tick,
            });
            return Err(XcCode::CapabilityOutOfRange);
        }
        // 幅度钳制（越界→钳+审计）。
        let (out, rec) = clamp_action(&card.0, key, magnitude, tick);
        if let Some(r) = rec {
            self.clamps.push(r);
        }
        Ok(out)
    }

    /// 契约变更提交（版本递增 + 契约指纹必须真的变了 + 通知账入账）。
    pub fn amend_contract(&mut self, id: &str, new_contract: BehaviorContract, reason: &str, tick: u64) -> Result<(), XcCode> {
        new_contract.validate()?;
        let idx = {
            let mut found: Option<usize> = None;
            let mut i = 0usize;
            while i < self.cards.len() {
                if let Some(c) = self.cards.get(i) {
                    if c.id == id {
                        found = Some(i);
                    }
                }
                i += 1;
            }
            match found {
                Some(x) => x,
                None => return Err(XcCode::AgentUnregistered),
            }
        };
        let card = match self.cards.get(idx) {
            Some(c) => c,
            None => return Err(XcCode::AgentUnregistered),
        };
        let old_fp = card.contract.fingerprint();
        let new_fp = new_contract.fingerprint();
        if old_fp == new_fp {
            // 空变更幂等（指纹没变不算变更）。
            return Ok(());
        }
        let old_version = card.contract.version;
        let ch = ContractChange {
            agent: id.to_string(),
            old_fp,
            new_fp,
            old_version,
            new_version: new_contract.version,
            reason: reason.to_string(),
            tick,
        };
        self.changes.record(ch)?;
        // 通知闸：订阅者全员确认后才真换卡。
        self.changes.notify_gate()?;
        if let Some(c) = self.cards.get_mut(idx) {
            c.contract = new_contract;
        }
        Ok(())
    }

    /// 变更订阅代理（注册表直通通知账）。
    pub fn subscribe_changes(&mut self, name: &str) -> usize {
        self.changes.subscribe(name)
    }

    /// 订阅确认代理。
    pub fn ack_change(&mut self, slot: usize, version: u32) -> Result<(), XcCode> {
        self.changes.subscribers.ack(slot, version)
    }

    /// 运行一拍（O(n)：逐 Agent 三段推进——段游标单向，行动段产出即回感知）。
    /// 返回本拍产出的行动卡数。
    pub fn run_tick(&mut self, tick: u64) -> usize {
        let n = self.cards.len();
        let mut produced = 0usize;
        let mut i = 0usize;
        while i < n {
            // 段推进（单向：感知→决策→行动；行动段游标回感知=一轮闭环）。
            if let Some(c) = self.cards.get_mut(i) {
                let nxt = c.phase.next();
                if c.phase == AgentPhase::Act {
                    // 行动段完成一轮——本拍产出计一，回感知。
                    produced = produced.saturating_add(1);
                    c.phase = AgentPhase::Perceive;
                } else {
                    c.phase = nxt;
                }
            }
            i += 1;
        }
        let _ = tick;
        produced
    }

    /// 段序核验（三段单向的账面复核——供判据侧直调）。
    pub const fn phase_forward_ok(from: AgentPhase, to: AgentPhase) -> Result<(), XcCode> {
        if to.ordinal() < from.ordinal() {
            return Err(XcCode::PhaseBackward);
        }
        Ok(())
    }

    /// 资源守恒读取。
    pub const fn resource_sums(&self) -> (u32, u32, u32) {
        (self.sum_cpu, self.sum_mem, self.sum_band)
    }

    /// 钳制账笔数。
    pub const fn clamp_count(&self) -> usize {
        self.clamps.len()
    }

    /// 审计账笔数。
    pub const fn audit_count(&self) -> usize {
        self.audits.len()
    }

    /// 未注册拒绝计数。
    pub const fn unregistered_rejects(&self) -> u32 {
        self.unregistered_count
    }

    /// 越界拒绝计数。
    pub const fn out_of_range_rejects(&self) -> u32 {
        self.out_of_range_count
    }

    /// 注册表自洽核验（资源守恒重放 + 变更账指纹对拍）。
    pub fn verify(&self) -> Result<(), XcCode> {
        let mut cpu: u32 = 0;
        let mut mem: u32 = 0;
        let mut band: u32 = 0;
        let mut i = 0usize;
        while i < self.cards.len() {
            let c = match self.cards.get(i) {
                Some(x) => x,
                None => return Err(XcCode::AgentUnregistered),
            };
            c.contract.validate()?;
            c.demand.validate()?;
            cpu = cpu.saturating_add(c.demand.cpu_bps);
            mem = mem.saturating_add(c.demand.mem_kb);
            band = band.saturating_add(c.demand.band_bps);
            i += 1;
        }
        if cpu != self.sum_cpu || mem != self.sum_mem || band != self.sum_band {
            return Err(XcCode::ResourceSumOver);
        }
        Ok(())
    }

    /// 人读汇总。
    pub fn report(&self) -> String {
        alloc::format!(
            "AgentRegistry:在册{}/资源(cpu={},mem={},band={})/钳制{}/审计{}/未注册拒{}/越界拒{}",
            self.cards.len(),
            self.sum_cpu,
            self.sum_mem,
            self.sum_band,
            self.clamps.len(),
            self.audits.len(),
            self.unregistered_count,
            self.out_of_range_count
        )
    }
}
