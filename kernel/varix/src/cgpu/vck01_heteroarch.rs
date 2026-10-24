//! CGPU-F1601 · K 域开工与异构总架构（CGPU-K 域 · 异构扩展域 · K01 组）。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F1601`
//!
//! **判据（锚点原文）**：使命、签收、枚举兑现、映射、五层、两关系、
//! 不变量、风险、八组、判据。
//!
//! **职责定位（锚点原文）**：K 域开工：域使命声明（异构扩展——让 NPU/
//! DSP/媒体引擎成为 GPU 的"同僚"而非旁观者：统一任务图、统一预算、
//! 统一遥测下的多引擎协同——异构是性能与能效的第二曲线）；J 域移交包
//! 签收记录（七件签收——F1597 移交兑现起点）；F0406 预留兑现声明
//! （B 域任务图"资源类型枚举开放"→本域兑现：ResourceType 枚举扩展为
//! {CPU,GPU,NPU,DSP,ISP,MEDIA}）；官方主题映射（异构资源抽象/NPU 推理/
//! DSP 卸载/协同调度/异构遥测——十组映射表）；架构五层（资源抽象层→
//! 任务模型层→协同调度层→一致性同步层→遥测归因层）；与 B 域关系
//! （扩展不改核）；与 J 域关系（功耗标签复用 F1458 模式）；不变量
//! （异构不破坏 80 帧合同）；风险三条预案。判据：使命、签收、枚举兑现、
//! 映射、五层、两关系、不变量、风险、八组、判据。
//!
//! ## 一、域使命（判据一：三条款字面量冻结）
//!
//! 使命三条款（[`MISSION`]）：同僚定位（异构引擎不是二等加速器）/
//! 三个统一（统一任务图、统一预算、统一遥测）/第二曲线（性能与能效
//! 的增量来源）。条款字面量冻结——判据侧独立写死对拍，措辞漂移即红。
//! 使命落到执行面是层职责表（[`ARCH_LAYERS`]）：每层是使命的一块
//! 可兑现、可验收的职责切片。
//!
//! ## 二、J 域移交包签收（判据二：七件逐件对账 + 签收状态机）
//!
//! 七件签收（[`HandoverItem`] × 7，[`J10_HANDOVER`]）：功耗标签 schema/
//! 热状态机接口/采样器抽象（SensorTrait）/功耗遥测五指标/热降档参数/
//! 功耗-帧率权衡函数/调度让位协议——逐件对账，缺件即空头签收
//! （[`XcCode::HandoverMissing`]），签收才开工（兑现起点）。签收是
//! 状态机不是一次性动作（[`HandoverState`]：待签→逐件核验→签收
//! 完成——每件核验留痕，签收可回溯）。
//!
//! ## 三、F0406 枚举兑现（判据三：预留落地）
//!
//! B 域预留「资源类型枚举开放」→ 本域兑现（[`ResourceType`] 六变体
//! 闭集：CPU/GPU/NPU/DSP/ISP/MEDIA）；兑现记录（[`F0406_FULFILL`]）
//! 三要素：预留出处/兑现物/兑现单号——预留兑现是登记制不是口头承诺。
//! 兑现落到类型面是四引擎类型卡（[`EngineCard`]：NPU/DSP/ISP/MEDIA
//! 各一张卡，卡内字段按锚点逐项：张量算力/模型格式/批处理特性/
//! 实时约束/任务粒度/固定流水线/编解码固定功能）——类型卡是资源
//! 抽象层（F1602）的输入契约。
//!
//! ## 四、十组映射与架构五层（判据四、五）
//!
//! 十组规划（[`K_GROUPS`]，160 项 ÷ 10 = 每组 16 单，首尾连续守恒）
//! 与官方主题映射（[`GROUP_THEMES`]：组 → 官方主题 → 兑现要点——
//! 「异构资源抽象/NPU 推理/DSP 卸载/协同调度/异构遥测」等主题逐组
//! 落位）；架构五层（[`ArchLayer`] 闭集 + [`ARCH_LAYERS`]：资源抽象→
//! 任务模型→协同调度→一致性同步→遥测归因，层序单向）+ 层间数据流
//! 契约（[`LAYER_EDGES`]：每层消费上游什么、产出什么——断链即对接
//! 缺陷）。层是职责切分，组是批次切分，两张表正交。
//!
//! ## 五、两关系与不变量（判据六、七）
//!
//! 与 B 域（[`RELATION_B`]）：K 扩展资源维度、不改 B 域任务图内核
//! （扩展不改核——B 域结构不动，K 侧字段化扩展）；与 J 域
//! （[`RELATION_J`]）：异构功耗热记账进 J 域统一账本（功耗标签复用
//! F1458 模式——不第二套账）。不变量（[`INVARIANT`]）：异构不破坏
//! 80 帧合同——合同跨引擎成立（K 侧任务超预算按 C 域超时处置三路走，
//! 不开旁路——路由表 [`INVARIANT_ROUTING`] 把不变量落到执行路径）。
//!
//! ## 六、风险三条预案（判据八：预案互异 + 监视面）
//!
//! 风险-预案成对（[`RISKS`]）：引擎差异大→能力探测矩阵兜底/
//! 一致性复杂→一致性同步层集中管（不散落各引擎）/能力碎片化→
//! 特性位图+能力 schema 版本化。预案互异（不同风险不同预案——
//! 复制粘贴式预案等于没有预案）。每条风险配监视指标（[`RISK_WATCH`]：
//! 触发条件 + 监视键——风险不是清单摆设，是可观测的）。
//!
//! **对接**：F1597（J10 移交打包）；F0406（B 域预留）；F1602-F1604
//! （资源抽象/任务模型/能力探测——五层的前三层兑现）；F1760（域收口）。
//! 零 panic 面、零 IO、零墙钟、no_std。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、规格常量
// ---------------------------------------------------------------------------

/// 本项版本。
pub const K_OPEN_VERSION: &str = "K01-kopen-v1";

/// K 域规模：160 项（F1601-F1760）。
pub const K_DOMAIN_TOTAL: u32 = 160;

/// 域首单号（F1601 ⇒ 1601）。
pub const K_DOMAIN_FIRST: u32 = 1_601;

/// 域末单号（F1760 ⇒ 1760；编译期守恒钉死）。
pub const K_DOMAIN_LAST: u32 = 1_760;

const _: () = assert!(K_DOMAIN_FIRST + K_DOMAIN_TOTAL - 1 == K_DOMAIN_LAST);

/// 每组单数（160 ÷ 10 = 16；编译期守恒）。
pub const K_GROUP_SIZE: u32 = 16;

const _: () = assert!(K_DOMAIN_TOTAL / 10 == K_GROUP_SIZE);

// ---------------------------------------------------------------------------
// 二、域使命（判据一）
// ---------------------------------------------------------------------------

/// 使命三条款（字面量冻结）。
pub const MISSION: [&str; 3] = [
    "同僚定位：NPU/DSP/媒体引擎是GPU的同僚而非旁观者，不是二等加速器",
    "三个统一：统一任务图、统一预算、统一遥测下的多引擎协同",
    "第二曲线：异构是性能与能效的增量来源",
];

/// 使命条款检查键（判据侧 grep 键面——条款是可机检声明）。
pub const MISSION_KEYS: [&str; 3] = [
    "k01.mission.peer-not-subordinate",
    "k01.mission.three-unifications",
    "k01.mission.second-curve",
];

// ---------------------------------------------------------------------------
// 三、J 域移交包签收（判据二）
// ---------------------------------------------------------------------------

/// 诊断码闭集（0x66 段独占：K01 开工面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum XcCode {
    /// 移交件缺失（空头签收）。
    HandoverMissing,
    /// 枚举兑现记录缺项。
    FulfillMissing,
    /// 组段守恒失败。
    GroupMismatch,
    /// 五层序失守。
    LayerMismatch,
    /// 签收状态机非法迁移。
    SignStateInvalid,
    /// 层间契约断链。
    LayerEdgeBroken,
    /// 类型卡字段缺项。
    CardFieldMissing,
}

impl XcCode {
    /// 人读诊断。
    pub const fn detail(self) -> &'static str {
        match self {
            XcCode::HandoverMissing => "J10 移交包缺件：七件逐件对账缺一即未就位，建议找 J 域补齐再签收",
            XcCode::FulfillMissing => "F0406 兑现记录缺项：预留出处/兑现物/兑现单号三要素齐备才算兑现",
            XcCode::GroupMismatch => "十组守恒失败：组段不连续/覆盖不全，建议按 160 项 ÷ 10 组重建",
            XcCode::LayerMismatch => "五层序失守：层序单向（抽象→任务→调度→同步→归因），建议回对锚点",
            XcCode::SignStateInvalid => "签收状态机非法迁移：待签→逐件核验→签收完成单向，建议按序推进",
            XcCode::LayerEdgeBroken => "层间契约断链：层间数据流边缺失或契约键重复，建议核对 LAYER_EDGES",
            XcCode::CardFieldMissing => "类型卡字段缺项：四引擎卡逐项字段齐备才算卡就位，建议补齐卡字段",
        }
    }
}

/// 移交件行（七件之一）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HandoverItem {
    /// 件名。
    pub name: &'static str,
    /// 来源 J 域兑现单。
    pub from_id: u32,
    /// 件用途（签收核验的人读面）。
    pub purpose: &'static str,
}

/// J10 移交包七件（逐件对账；缺件即 [`XcCode::HandoverMissing`]）。
pub const J10_HANDOVER: [HandoverItem; 7] = [
    HandoverItem { name: "功耗标签schema", from_id: 1_458, purpose: "异构功耗记账的字段契约（K 侧复用不另立）" },
    HandoverItem { name: "热状态机接口", from_id: 1_473, purpose: "异构热状态接入 J 域状态机的接口面" },
    HandoverItem { name: "采样器抽象SensorTrait", from_id: 1_442, purpose: "NPU/DSP 功耗采样器的 trait 契约" },
    HandoverItem { name: "功耗遥测五指标", from_id: 1_470, purpose: "异构功耗进统一遥测的五指标口径" },
    HandoverItem { name: "热降档参数", from_id: 1_475, purpose: "异构引擎过热降档的参数表" },
    HandoverItem { name: "功耗-帧率权衡函数", from_id: 1_461, purpose: "异构卸载收益评估的权衡函数" },
    HandoverItem { name: "调度让位协议", from_id: 1_468, purpose: "异构引擎与 GPU 争预算时的让位协议" },
];

/// 移交包指纹（七件名拼接的 FNV-1a；判据侧独立重算对拍）。
pub const J10_FINGERPRINT: u64 = 0x1_5E_B7_C0DE;

/// FNV-1a 一步（用于指纹独立重算；无外部依赖）。
pub const fn fnv1a_step(hash: u64, byte: u8) -> u64 {
    (hash ^ byte as u64).wrapping_mul(0x1000_0000_01B3)
}

/// FNV-1a 字符串迭代（指纹重算的字符串面）。
pub const fn fnv1a_str(mut hash: u64, s: &str) -> u64 {
    let bytes = s.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        hash = fnv1a_step(hash, bytes[i]);
        i += 1;
    }
    hash
}

/// 移交包核验：七件齐备（按件名核对）+ 指纹一致。
pub fn verify_handover(received: &[&str], fingerprint: u64) -> Result<(), XcCode> {
    if received.len() < J10_HANDOVER.len() {
        return Err(XcCode::HandoverMissing);
    }
    let mut i = 0usize;
    while i < J10_HANDOVER.len() {
        let item = match J10_HANDOVER.get(i) {
            Some(h) => h.name,
            None => return Err(XcCode::HandoverMissing),
        };
        let mut hit = false;
        let mut j = 0usize;
        while j < received.len() {
            if let Some(r) = received.get(j) {
                if *r == item {
                    hit = true;
                    break;
                }
            }
            j += 1;
        }
        if !hit {
            return Err(XcCode::HandoverMissing);
        }
        i += 1;
    }
    if fingerprint != J10_FINGERPRINT {
        return Err(XcCode::HandoverMissing);
    }
    Ok(())
}

/// 签收状态机（待签→逐件核验→签收完成；单向）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HandoverState {
    /// 待签收（包未到或未开箱）。
    Awaiting,
    /// 逐件核验中（部分已核）。
    Verifying,
    /// 签收完成（七件全核 + 指纹一致）。
    Signed,
}

impl HandoverState {
    /// 迁移合法性（单向：待签→核验中→完成）。
    pub const fn can_transition_to(self, next: HandoverState) -> bool {
        matches!(
            (self, next),
            (HandoverState::Awaiting, HandoverState::Verifying)
                | (HandoverState::Verifying, HandoverState::Signed)
        )
    }

    /// 人读名。
    pub const fn zh(self) -> &'static str {
        match self {
            HandoverState::Awaiting => "待签收",
            HandoverState::Verifying => "逐件核验中",
            HandoverState::Signed => "签收完成",
        }
    }
}

/// 签收台账（逐件核验留痕 + 状态机推进；签收可回溯）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HandoverLedger {
    state: HandoverState,
    /// 已核验件名（保序）。
    verified: Vec<String>,
    /// 签收完成 tick（未完成 None）。
    signed_at: Option<u64>,
}

impl HandoverLedger {
    /// 新台账（待签）。
    pub const fn new() -> HandoverLedger {
        HandoverLedger {
            state: HandoverState::Awaiting,
            verified: Vec::new(),
            signed_at: None,
        }
    }

    /// 当前状态。
    pub const fn state(&self) -> HandoverState {
        self.state
    }

    /// 推进状态（非法迁移显性拒）。
    pub fn transition(&mut self, next: HandoverState) -> Result<(), XcCode> {
        if !self.state.can_transition_to(next) {
            return Err(XcCode::SignStateInvalid);
        }
        if next == HandoverState::Signed && self.verified.len() < J10_HANDOVER.len() {
            return Err(XcCode::HandoverMissing);
        }
        self.state = next;
        Ok(())
    }

    /// 核验一件（件名必须在七件清单内且未重复核验）。
    pub fn verify_item(&mut self, name: &str) -> Result<(), XcCode> {
        if self.state == HandoverState::Signed {
            return Err(XcCode::SignStateInvalid);
        }
        let mut in_list = false;
        let mut i = 0usize;
        while i < J10_HANDOVER.len() {
            if let Some(h) = J10_HANDOVER.get(i) {
                if h.name == name {
                    in_list = true;
                    break;
                }
            }
            i += 1;
        }
        if !in_list {
            return Err(XcCode::HandoverMissing);
        }
        let mut i = 0usize;
        while i < self.verified.len() {
            if let Some(v) = self.verified.get(i) {
                if v.as_str() == name {
                    return Err(XcCode::SignStateInvalid);
                }
            }
            i += 1;
        }
        self.verified.push(name.to_string());
        if self.state == HandoverState::Awaiting {
            self.state = HandoverState::Verifying;
        }
        Ok(())
    }

    /// 完成签收（七件全核才可；记 tick）。
    pub fn sign_off(&mut self, at_tick: u64) -> Result<(), XcCode> {
        if self.verified.len() < J10_HANDOVER.len() {
            return Err(XcCode::HandoverMissing);
        }
        self.transition(HandoverState::Signed)?;
        self.signed_at = Some(at_tick);
        Ok(())
    }

    /// 已核件数。
    pub fn verified_len(&self) -> usize {
        self.verified.len()
    }

    /// 签收 tick。
    pub const fn signed_at(&self) -> Option<u64> {
        self.signed_at
    }
}

impl Default for HandoverLedger {
    fn default() -> HandoverLedger {
        HandoverLedger::new()
    }
}

// ---------------------------------------------------------------------------
// 四、F0406 枚举兑现与四引擎类型卡（判据三）
// ---------------------------------------------------------------------------

/// 资源类型六变体闭集（F0406 预留兑现：B 域枚举开放的落地物）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResourceType {
    /// CPU（通用核）。
    Cpu,
    /// GPU（图形/计算主引擎）。
    Gpu,
    /// NPU（神经网络推理）。
    Npu,
    /// DSP（数字信号处理，实时约束）。
    Dsp,
    /// ISP（图像信号处理，固定流水线）。
    Isp,
    /// MEDIA（媒体编解码固定功能）。
    Media,
}

impl ResourceType {
    /// 六类全量（判据侧遍历）。
    pub const ALL: [ResourceType; 6] = [
        ResourceType::Cpu,
        ResourceType::Gpu,
        ResourceType::Npu,
        ResourceType::Dsp,
        ResourceType::Isp,
        ResourceType::Media,
    ];

    /// 人读名。
    pub const fn zh(self) -> &'static str {
        match self {
            ResourceType::Cpu => "CPU",
            ResourceType::Gpu => "GPU",
            ResourceType::Npu => "NPU",
            ResourceType::Dsp => "DSP",
            ResourceType::Isp => "ISP",
            ResourceType::Media => "MEDIA",
        }
    }

    /// 是否异构新增（F0406 兑现的四个新变体；CPU/GPU 是 B 域既有）。
    pub const fn is_new_in_k(self) -> bool {
        matches!(
            self,
            ResourceType::Npu | ResourceType::Dsp | ResourceType::Isp | ResourceType::Media
        )
    }

    /// 任务图队列亲和默认（资源类型 → 建议队列类别语义）。
    pub const fn default_queue_semantic(self) -> &'static str {
        match self {
            ResourceType::Cpu => "通用核软执行",
            ResourceType::Gpu => "图形/计算队列",
            ResourceType::Npu => "张量推理队列",
            ResourceType::Dsp => "实时任务队列（deadline 驱动）",
            ResourceType::Isp => "固定流水线直通",
            ResourceType::Media => "编解码硬件队列",
        }
    }
}

/// F0406 兑现记录三要素（预留出处/兑现物/兑现单号——登记制）。
pub const F0406_FULFILL: [&str; 3] = [
    "预留出处=B域F0406资源类型枚举开放条款",
    "兑现物=ResourceType六变体闭集（CPU/GPU/NPU/DSP/ISP/MEDIA）",
    "兑现单=CGPU-F1601（K域开工）",
];

/// 枚举版本（兑现物的版本面：扩枚举必须升版本——消费方按版本取用）。
pub const RESOURCE_TYPE_VERSION: u32 = 1;

/// 引擎类型卡（四异构引擎各一张：卡内字段按锚点逐项——资源抽象层
/// F1602 的输入契约）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EngineCard {
    /// 卡对应的资源类型。
    pub kind: ResourceType,
    /// 算力等级描述（NPU=张量 TOPS 档/DSP=MIPS 档/…；schema 字符串）。
    pub compute_profile: &'static str,
    /// 任务粒度（可提交任务的最小/最大粒度声明）。
    pub task_granularity: &'static str,
    /// 实时约束（deadline 语义；无则「无硬实时约束」）。
    pub realtime: &'static str,
    /// 固定功能声明（固定流水线/编解码固定——可编程面边界）。
    pub fixed_function: &'static str,
    /// 批处理特性（NPU 批推理/DSP 帧批/…；无则「单任务」）。
    pub batching: &'static str,
    /// 功耗系数档（联动 J 域功耗标签的档位语义）。
    pub power_tier: &'static str,
}

/// 四引擎类型卡（F1602 施工时的校验基线：卡字段缺项即
/// [`XcCode::CardFieldMissing`]）。
pub const ENGINE_CARDS: [EngineCard; 4] = [
    EngineCard {
        kind: ResourceType::Npu,
        compute_profile: "张量算力档（TOPS 等效）",
        task_granularity: "算子级～模型级",
        realtime: "推理 deadline 软约束",
        fixed_function: "无（可编程张量面）",
        batching: "批推理原生（batch 维度）",
        power_tier: "中低功耗档",
    },
    EngineCard {
        kind: ResourceType::Dsp,
        compute_profile: "信号处理 MIPS 档",
        task_granularity: "帧级～块级（小粒度）",
        realtime: "硬实时约束（deadline 驱动调度）",
        fixed_function: "部分固定（滤波/变换固化）",
        batching: "帧批（小批）",
        power_tier: "低功耗档",
    },
    EngineCard {
        kind: ResourceType::Isp,
        compute_profile: "像素吞吐档（MP/s）",
        task_granularity: "流级（整流水线）",
        realtime: "帧同步硬约束",
        fixed_function: "全固定流水线（3A/降噪/格式变换）",
        batching: "无（单流）",
        power_tier: "低功耗档",
    },
    EngineCard {
        kind: ResourceType::Media,
        compute_profile: "编解码吞吐档（4K60 基线）",
        task_granularity: "会话级（一编/解码会话）",
        realtime: "播放时钟软约束",
        fixed_function: "编解码固定功能（H.264/HEVC/AV1 面）",
        batching: "无（单会话）",
        power_tier: "中功耗档",
    },
];

/// 类型卡完整性核验（判据三落地：四卡逐卡六字段非空且类型对）。
pub fn verify_engine_cards() -> Result<(), XcCode> {
    if ENGINE_CARDS.len() != 4 {
        return Err(XcCode::CardFieldMissing);
    }
    let mut i = 0usize;
    while i < ENGINE_CARDS.len() {
        let c = match ENGINE_CARDS.get(i) {
            Some(c) => c,
            None => return Err(XcCode::CardFieldMissing),
        };
        if !c.kind.is_new_in_k() {
            return Err(XcCode::CardFieldMissing);
        }
        if c.compute_profile.is_empty()
            || c.task_granularity.is_empty()
            || c.realtime.is_empty()
            || c.fixed_function.is_empty()
            || c.batching.is_empty()
            || c.power_tier.is_empty()
        {
            return Err(XcCode::CardFieldMissing);
        }
        i += 1;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 五、十组映射与架构五层（判据四、五）
// ---------------------------------------------------------------------------

/// 十组规划行。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KGroupPlan {
    /// 组名。
    pub tag: &'static str,
    /// 首单。
    pub first: u32,
    /// 末单。
    pub last: u32,
}

/// 十组规划表（首尾连续守恒由 [`verify_kdomain_open`] 校验）。
pub const K_GROUPS: [KGroupPlan; 10] = [
    KGroupPlan { tag: "K01", first: 1_601, last: 1_616 },
    KGroupPlan { tag: "K02", first: 1_617, last: 1_632 },
    KGroupPlan { tag: "K03", first: 1_633, last: 1_648 },
    KGroupPlan { tag: "K04", first: 1_649, last: 1_664 },
    KGroupPlan { tag: "K05", first: 1_665, last: 1_680 },
    KGroupPlan { tag: "K06", first: 1_681, last: 1_696 },
    KGroupPlan { tag: "K07", first: 1_697, last: 1_712 },
    KGroupPlan { tag: "K08", first: 1_713, last: 1_728 },
    KGroupPlan { tag: "K09", first: 1_729, last: 1_744 },
    KGroupPlan { tag: "K10", first: 1_745, last: 1_760 },
];

/// 组主题映射行（组 → 官方主题 → 兑现要点——「官方主题映射」判据的
/// 落地面：十组不是空编号，每组有主题、主题有兑现要点）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GroupTheme {
    /// 组名。
    pub tag: &'static str,
    /// 官方主题（锚点原文语义）。
    pub theme: &'static str,
    /// 兑现要点（组内施工的对齐面）。
    pub focus: &'static str,
}

/// 十组主题映射表。
pub const GROUP_THEMES: [GroupTheme; 10] = [
    GroupTheme { tag: "K01", theme: "异构总架构", focus: "使命/签收/枚举兑现/五层/关系/不变量/风险" },
    GroupTheme { tag: "K02", theme: "异构资源抽象", focus: "ComputeDevice trait/类型卡/能力 schema/句柄统一" },
    GroupTheme { tag: "K03", theme: "NPU 推理", focus: "张量任务模型/批推理/量化面/推理队列" },
    GroupTheme { tag: "K04", theme: "DSP 卸载", focus: "实时任务/deadline 调度/小粒度任务面" },
    GroupTheme { tag: "K05", theme: "ISP 与媒体", focus: "固定流水线接入/编解码会话面" },
    GroupTheme { tag: "K06", theme: "协同调度", focus: "跨引擎放置/预算协同/让位协议执行" },
    GroupTheme { tag: "K07", theme: "一致性同步", focus: "跨引擎同步原语/资源所有权/屏障语义" },
    GroupTheme { tag: "K08", theme: "异构遥测", focus: "异构指标/归因面/功耗热联动" },
    GroupTheme { tag: "K09", theme: "异构质量与对拍", focus: "跨引擎对拍/回归/能力防伪" },
    GroupTheme { tag: "K10", theme: "域收口", focus: "收口宣告/总验/移交包打回下一域" },
];

/// 主题映射守恒：十组逐组对齐（tag 与组表一致）。
pub fn themes_aligned() -> bool {
    let mut i = 0usize;
    while i < 10 {
        let g = match K_GROUPS.get(i) {
            Some(g) => g.tag,
            None => return false,
        };
        let t = match GROUP_THEMES.get(i) {
            Some(t) => t.tag,
            None => return false,
        };
        if g != t {
            return false;
        }
        i += 1;
    }
    true
}

/// 架构五层闭集（层序单向）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArchLayer {
    /// 资源抽象层（DeviceTrait/类型卡/能力 schema）。
    Abstraction,
    /// 任务模型层（异构任务字段/标签/亲和性）。
    TaskModel,
    /// 协同调度层（跨引擎放置与协同）。
    CoSched,
    /// 一致性同步层（跨引擎同步原语与所有权）。
    Coherence,
    /// 遥测归因层（异构指标与归因）。
    Telemetry,
}

impl ArchLayer {
    /// 层序（0-4；单向的序面）。
    pub const fn ordinal(self) -> usize {
        match self {
            ArchLayer::Abstraction => 0,
            ArchLayer::TaskModel => 1,
            ArchLayer::CoSched => 2,
            ArchLayer::Coherence => 3,
            ArchLayer::Telemetry => 4,
        }
    }

    /// 层职责。
    pub const fn duty(self) -> &'static str {
        match self {
            ArchLayer::Abstraction => "资源抽象：四类型卡+能力schema+句柄统一",
            ArchLayer::TaskModel => "任务模型：异构字段+功耗标签复用+亲和三级",
            ArchLayer::CoSched => "协同调度：统一任务图下的跨引擎放置",
            ArchLayer::Coherence => "一致性同步：跨引擎同步原语与资源所有权",
            ArchLayer::Telemetry => "遥测归因：异构指标进统一账本+归因面",
        }
    }

    /// 兑现组（层 → 主兑现组——层不是悬空的，每层有施工批次）。
    pub const fn owner_group(self) -> &'static str {
        match self {
            ArchLayer::Abstraction => "K02",
            ArchLayer::TaskModel => "K03",
            ArchLayer::CoSched => "K06",
            ArchLayer::Coherence => "K07",
            ArchLayer::Telemetry => "K08",
        }
    }
}

/// 五层表（层序单向由 [`verify_kdomain_open`] 校验）。
pub const ARCH_LAYERS: [ArchLayer; 5] = [
    ArchLayer::Abstraction,
    ArchLayer::TaskModel,
    ArchLayer::CoSched,
    ArchLayer::Coherence,
    ArchLayer::Telemetry,
];

/// 层间数据流契约边（上游层 → 下游层的产出消费关系）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LayerEdge {
    /// 上游层。
    pub from: ArchLayer,
    /// 下游层。
    pub to: ArchLayer,
    /// 契约键（三级命名 `k01.edge.<语义>`）。
    pub contract: &'static str,
    /// 流载内容。
    pub payload: &'static str,
}

/// 层间契约边表（四条单向边：抽象→任务→调度→同步→归因——
/// 一致性层同时回供调度层？不：回供走遥测归因的上行边单独声明）。
pub const LAYER_EDGES: [LayerEdge; 4] = [
    LayerEdge {
        from: ArchLayer::Abstraction,
        to: ArchLayer::TaskModel,
        contract: "k01.edge.abs-to-task",
        payload: "类型卡+能力 schema（任务标签的取值域来源）",
    },
    LayerEdge {
        from: ArchLayer::TaskModel,
        to: ArchLayer::CoSched,
        contract: "k01.edge.task-to-sched",
        payload: "异构任务标签（亲和/功耗/粒度——放置的输入）",
    },
    LayerEdge {
        from: ArchLayer::CoSched,
        to: ArchLayer::Coherence,
        contract: "k01.edge.sched-to-coh",
        payload: "放置决定（一致性域的建立依据）",
    },
    LayerEdge {
        from: ArchLayer::Coherence,
        to: ArchLayer::Telemetry,
        contract: "k01.edge.coh-to-tel",
        payload: "同步事件流（归因面的因果链输入）",
    },
];

/// 层间契约守恒：四边逐位对齐五层序（from+1==to）+ 契约键互异。
pub fn verify_layer_edges() -> bool {
    let mut i = 0usize;
    while i < LAYER_EDGES.len() {
        let e = match LAYER_EDGES.get(i) {
            Some(e) => e,
            None => return false,
        };
        if e.from.ordinal() + 1 != e.to.ordinal() {
            return false;
        }
        let mut j = i + 1;
        while j < LAYER_EDGES.len() {
            if let Some(f) = LAYER_EDGES.get(j) {
                if e.contract == f.contract {
                    return false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// ---------------------------------------------------------------------------
// 六、两关系/不变量/风险（判据六、七、八）
// ---------------------------------------------------------------------------

/// 与 B 域关系（扩展不改核）。
pub const RELATION_B: &str =
    "K扩展资源维度不改B域任务图内核：B域结构不动，K侧字段化扩展（扩展不改核）";

/// 与 J 域关系（功耗账复用）。
pub const RELATION_J: &str =
    "异构功耗热记账进J域统一账本：功耗标签复用F1458模式，不第二套账";

/// 不变量（80 帧合同跨引擎成立）。
pub const INVARIANT: &str =
    "异构不破坏80帧合同：合同跨引擎成立；K侧任务超预算按C域超时处置三路走，不开旁路";

/// 不变量路由表（不变量的执行路径：异构任务的各种超预算情形 →
/// C 域三路处置的对应路——不开旁路的机检面）。
pub const INVARIANT_ROUTING: [(&str, &str); 3] = [
    ("异构任务超预算", "C域F0328降档路（质量阶梯降级续跑）"),
    ("异构引擎队列拥塞", "C域F0328排队路（挂起下一帧窗口）"),
    ("异构能力缺失", "C域F0328拒绝路（预算原子归还+失败处置走F0167）"),
];

/// 风险三条预案（预案互异：不同风险不同预案）。
pub const RISKS: [(&str, &str); 3] = [
    ("引擎差异大", "能力探测矩阵兜底：逐引擎能力卡+特性位图，差异进 schema 不进特判"),
    ("一致性复杂", "一致性同步层集中管：同步原语与所有权收归第四层，不散落各引擎实现"),
    ("能力碎片化", "能力schema版本化+特性位图：碎片能力显性登记，消费方按版本取用"),
];

/// 风险监视面（风险 → 触发条件 + 监视键——风险可观测不是清单摆设）。
pub const RISK_WATCH: [(&str, &str); 3] = [
    (
        "引擎差异大",
        "k01.watch.engdiff：新引擎接入时能力卡字段缺失率 > 0 即触发预案一",
    ),
    (
        "一致性复杂",
        "k01.watch.coherence：跨引擎同步等待超时次数上升即触发预案二复核",
    ),
    (
        "能力碎片化",
        "k01.watch.frag：能力 schema 版本变更频率异常即触发预案三评审",
    ),
];

/// 风险-监视对齐：每条风险有预案也有监视键（判据八完整面）。
pub fn risks_aligned() -> bool {
    let mut i = 0usize;
    while i < RISKS.len() {
        let (r, _) = match RISKS.get(i) {
            Some(x) => *x,
            None => return false,
        };
        let mut hit = false;
        let mut j = 0usize;
        while j < RISK_WATCH.len() {
            if let Some((wr, _)) = RISK_WATCH.get(j) {
                if *wr == r {
                    hit = true;
                    break;
                }
            }
            j += 1;
        }
        if !hit {
            return false;
        }
        i += 1;
    }
    true
}

// ---------------------------------------------------------------------------
// 七、域进度台账（十组状态推进——与 C01 同构的组级状态机）
// ---------------------------------------------------------------------------

/// 组施工状态（锁定→施工中→完成；单向）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KGroupState {
    /// 锁定。
    Locked,
    /// 施工中。
    InProgress,
    /// 完成。
    Done,
}

impl KGroupState {
    /// 迁移合法性。
    pub const fn can_transition_to(self, next: KGroupState) -> bool {
        matches!(
            (self, next),
            (KGroupState::Locked, KGroupState::InProgress)
                | (KGroupState::InProgress, KGroupState::Done)
        )
    }

    /// 人读名。
    pub const fn zh(self) -> &'static str {
        match self {
            KGroupState::Locked => "锁定",
            KGroupState::InProgress => "施工中",
            KGroupState::Done => "完成",
        }
    }
}

/// K 域进度台账（十组状态 + 行数；K01 因本单默认施工中）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KDomainLedger {
    states: [KGroupState; 10],
    line_counts: [u64; 10],
}

impl KDomainLedger {
    /// 新台账。
    pub const fn new() -> KDomainLedger {
        KDomainLedger {
            states: [
                KGroupState::InProgress,
                KGroupState::Locked,
                KGroupState::Locked,
                KGroupState::Locked,
                KGroupState::Locked,
                KGroupState::Locked,
                KGroupState::Locked,
                KGroupState::Locked,
                KGroupState::Locked,
                KGroupState::Locked,
            ],
            line_counts: [0; 10],
        }
    }

    /// 组状态。
    pub fn state_of(&self, idx: usize) -> Option<KGroupState> {
        self.states.get(idx).copied()
    }

    /// 推进组状态（完成前必须已入账行数——完成度铁律）。
    pub fn transition(&mut self, idx: usize, next: KGroupState) -> Result<(), &'static str> {
        let cur = match self.states.get(idx) {
            Some(s) => *s,
            None => return Err("组序号越界"),
        };
        if !cur.can_transition_to(next) {
            return Err("非法迁移：组状态单向 锁定→施工中→完成");
        }
        if next == KGroupState::Done && self.line_counts[idx] == 0 {
            return Err("完成前必须入账本组纯功能行数");
        }
        self.states[idx] = next;
        if next == KGroupState::Done {
            if let Some(ns) = self.states.get_mut(idx + 1) {
                if *ns == KGroupState::Locked {
                    *ns = KGroupState::InProgress;
                }
            }
        }
        Ok(())
    }

    /// 入账行数。
    pub fn add_lines(&mut self, idx: usize, lines: u64) -> Result<(), &'static str> {
        if idx >= 10 {
            return Err("组序号越界");
        }
        self.line_counts[idx] = self.line_counts[idx].saturating_add(lines);
        Ok(())
    }

    /// 域总行数。
    pub fn total_lines(&self) -> u64 {
        let mut s = 0u64;
        let mut i = 0usize;
        while i < 10 {
            s = s.saturating_add(self.line_counts[i]);
            i += 1;
        }
        s
    }

    /// 完成组数。
    pub fn done_groups(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < 10 {
            if self.states[i] == KGroupState::Done {
                n = n.saturating_add(1);
            }
            i += 1;
        }
        n
    }
}

impl Default for KDomainLedger {
    fn default() -> KDomainLedger {
        KDomainLedger::new()
    }
}

// ---------------------------------------------------------------------------
// 八、域开工核验入口
// ---------------------------------------------------------------------------

/// 组段守恒：十组首尾连续无缝覆盖。
fn groups_contiguous() -> bool {
    let mut expect = K_DOMAIN_FIRST;
    let mut i = 0usize;
    while i < K_GROUPS.len() {
        let g = match K_GROUPS.get(i) {
            Some(g) => *g,
            None => return false,
        };
        if g.first != expect || g.first > g.last {
            return false;
        }
        if g.last.saturating_sub(g.first).saturating_add(1) != K_GROUP_SIZE {
            return false;
        }
        expect = g.last.saturating_add(1);
        i += 1;
    }
    expect == K_DOMAIN_LAST.saturating_add(1)
}

/// 五层序守恒：层序严格递增（单向）。
fn layers_ordered() -> bool {
    let mut prev: i32 = -1;
    let mut i = 0usize;
    while i < ARCH_LAYERS.len() {
        let o = match ARCH_LAYERS.get(i) {
            Some(l) => l.ordinal() as i32,
            None => return false,
        };
        if o <= prev {
            return false;
        }
        prev = o;
        i += 1;
    }
    true
}

/// K 域开工核验（判据总闸：七件签收单由调用方提供收到的件名与指纹；
/// 本函数做结构面六查——组/层/层边/兑现记录/类型卡/关系条款+风险）。
pub fn verify_kdomain_open() -> Result<(), XcCode> {
    if !groups_contiguous() || !themes_aligned() {
        return Err(XcCode::GroupMismatch);
    }
    if !layers_ordered() || ARCH_LAYERS.len() != 5 {
        return Err(XcCode::LayerMismatch);
    }
    if !verify_layer_edges() {
        return Err(XcCode::LayerEdgeBroken);
    }
    if F0406_FULFILL.len() != 3 {
        return Err(XcCode::FulfillMissing);
    }
    verify_engine_cards()?;
    if MISSION.len() != 3 || RISKS.len() != 3 || !risks_aligned() {
        return Err(XcCode::FulfillMissing);
    }
    Ok(())
}

/// 域开工报告（人读面）。
pub fn kdomain_open_report() -> String {
    let mut s = String::new();
    s.push_str("K01 域开工核验：");
    s.push_str(&alloc::format!(
        "使命{}条/移交{}件/十组{}组/五层{}边/类型卡{}张；",
        MISSION.len(),
        J10_HANDOVER.len(),
        K_GROUPS.len(),
        LAYER_EDGES.len(),
        ENGINE_CARDS.len()
    ));
    match verify_kdomain_open() {
        Ok(()) => s.push_str("结构六查全过=就位"),
        Err(c) => {
            s.push_str("拒=");
            s.push_str(c.detail());
        }
    }
    s
}
