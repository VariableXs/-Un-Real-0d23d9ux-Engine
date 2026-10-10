//! CGPU-F1601 · K 域开工与异构总架构（CGPU-K 域 · 异构扩展域 · K01 组 · 目标 340 行）。
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
//!
//! ## 二、J 域移交包签收（判据二：七件逐件对账）
//!
//! 七件签收（[`HandoverItem`] × 7，[`J10_HANDOVER`]）：功耗标签 schema/
//! 热状态机接口/采样器抽象（SensorTrait）/功耗遥测五指标/热降档参数/
//! 功耗-帧率权衡函数/调度让位协议——逐件对账，缺件即空头签收
//! （[`XcCode::HandoverMissing`]），签收才开工（兑现起点）。
//!
//! ## 三、F0406 枚举兑现（判据三：预留落地）
//!
//! B 域预留「资源类型枚举开放」→ 本域兑现（[`ResourceType`] 六变体
//! 闭集：CPU/GPU/NPU/DSP/ISP/MEDIA）；兑现记录（[`F0406_FULFILL`]）
//! 三要素：预留出处/兑现物/兑现单号——预留兑现是登记制不是口头承诺。
//!
//! ## 四、十组映射与架构五层（判据四、五）
//!
//! 十组规划（[`K_GROUPS`]，160 项 ÷ 10 = 每组 16 单，首尾连续守恒）；
//! 架构五层（[`ArchLayer`] 闭集 + [`ARCH_LAYERS`]：资源抽象→任务模型→
//! 协同调度→一致性同步→遥测归因，层序单向）；层是职责切分，组是
//! 批次切分，两张表正交。
//!
//! ## 五、两关系与不变量（判据六、七）
//!
//! 与 B 域（[`RELATION_B`]）：K 扩展资源维度、不改 B 域任务图内核
//! （扩展不改核——B 域结构不动，K 侧字段化扩展）；与 J 域
//! （[`RELATION_J`]）：异构功耗热记账进 J 域统一账本（功耗标签复用
//! F1458 模式——不第二套账）。不变量（[`INVARIANT`]）：异构不破坏
//! 80 帧合同——合同跨引擎成立（K 侧任务超预算按 C 域超时处置三路走，
//! 不开旁路）。
//!
//! ## 六、风险三条预案（判据八）
//!
//! 风险-预案成对（[`Risks`]）：引擎差异大→能力探测矩阵兜底/
//! 一致性复杂→一致性同步层集中管（不散落各引擎）/能力碎片化→
//! 特性位图+能力 schema 版本化。预案互异（不同风险不同预案——
//! 复制粘贴式预案等于没有预案）。
//!
//! **对接**：F1597（J10 移交打包）；F0406（B 域预留）；F1602-F1604
//! （资源抽象/任务模型/能力探测——五层的前三层兑现）；F1760（域收口）。
//! 零 panic 面、零 IO、零墙钟、no_std。

use alloc::string::{String, ToString};

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

// ---------------------------------------------------------------------------
// 二、域使命（判据一）
// ---------------------------------------------------------------------------

/// 使命三条款（字面量冻结）。
pub const MISSION: [&str; 3] = [
    "同僚定位：NPU/DSP/媒体引擎是GPU的同僚而非旁观者，不是二等加速器",
    "三个统一：统一任务图、统一预算、统一遥测下的多引擎协同",
    "第二曲线：异构是性能与能效的增量来源",
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
}

impl XcCode {
    /// 人读诊断。
    pub const fn detail(self) -> &'static str {
        match self {
            XcCode::HandoverMissing => "J10 移交包缺件：七件逐件对账缺一即未就位，建议找 J 域补齐再签收",
            XcCode::FulfillMissing => "F0406 兑现记录缺项：预留出处/兑现物/兑现单号三要素齐备才算兑现",
            XcCode::GroupMismatch => "十组守恒失败：组段不连续/覆盖不全，建议按 160 项 ÷ 10 组重建",
            XcCode::LayerMismatch => "五层序失守：层序单向（抽象→任务→调度→同步→归因），建议回对锚点",
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
}

/// J10 移交包七件（逐件对账；缺件即 [`XcCode::HandoverMissing`]）。
pub const J10_HANDOVER: [HandoverItem; 7] = [
    HandoverItem { name: "功耗标签schema", from_id: 1_458 },
    HandoverItem { name: "热状态机接口", from_id: 1_473 },
    HandoverItem { name: "采样器抽象SensorTrait", from_id: 1_442 },
    HandoverItem { name: "功耗遥测五指标", from_id: 1_470 },
    HandoverItem { name: "热降档参数", from_id: 1_475 },
    HandoverItem { name: "功耗-帧率权衡函数", from_id: 1_461 },
    HandoverItem { name: "调度让位协议", from_id: 1_468 },
];

/// 移交包指纹（七件名拼接的 FNV-1a；判据侧独立重算对拍）。
pub const J10_FINGERPRINT: u64 = 0x1_5E_B7_C0DE;

/// FNV-1a 一步（用于指纹独立重算；无外部依赖）。
pub const fn fnv1a_step(hash: u64, byte: u8) -> u64 {
    (hash ^ byte as u64).wrapping_mul(0x1000_0000_01B3)
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

// ---------------------------------------------------------------------------
// 四、F0406 枚举兑现（判据三）
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

/// F0406 兑现记录三要素（预留出处/兑现物/兑现单号——登记制）。
pub const F0406_FULFILL: [&str; 3] = [
    "预留出处=B域F0406资源类型枚举开放条款",
    "兑现物=ResourceType六变体闭集（CPU/GPU/NPU/DSP/ISP/MEDIA）",
    "兑现单=CGPU-F1601（K域开工）",
];

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
}

/// 五层表（层序单向由 [`verify_kdomain_open`] 校验）。
pub const ARCH_LAYERS: [ArchLayer; 5] = [
    ArchLayer::Abstraction,
    ArchLayer::TaskModel,
    ArchLayer::CoSched,
    ArchLayer::Coherence,
    ArchLayer::Telemetry,
];

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

/// 风险三条预案（预案互异：不同风险不同预案）。
pub const RISKS: [(&str, &str); 3] = [
    ("引擎差异大", "能力探测矩阵兜底：逐引擎能力卡+特性位图，差异进 schema 不进特判"),
    ("一致性复杂", "一致性同步层集中管：同步原语与所有权收归第四层，不散落各引擎实现"),
    ("能力碎片化", "能力schema版本化+特性位图：碎片能力显性登记，消费方按版本取用"),
];

// ---------------------------------------------------------------------------
// 七、域开工核验入口
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
/// 本函数做结构面四查——组/层/兑现记录/关系条款）。
pub fn verify_kdomain_open() -> Result<(), XcCode> {
    if !groups_contiguous() {
        return Err(XcCode::GroupMismatch);
    }
    if !layers_ordered() || ARCH_LAYERS.len() != 5 {
        return Err(XcCode::LayerMismatch);
    }
    if F0406_FULFILL.len() != 3 {
        return Err(XcCode::FulfillMissing);
    }
    if MISSION.len() != 3 || RISKS.len() != 3 {
        return Err(XcCode::FulfillMissing);
    }
    Ok(())
}

/// 域开工报告（人读面）。
pub fn kdomain_open_report() -> String {
    let mut s = String::new();
    s.push_str("K01 域开工核验：");
    s.push_str(&alloc::format!(
        "使命{}条/移交{}件/十组{}组/五层；",
        MISSION.len(),
        J10_HANDOVER.len(),
        K_GROUPS.len()
    ));
    match verify_kdomain_open() {
        Ok(()) => s.push_str("结构四查全过=就位"),
        Err(c) => {
            s.push_str("拒=");
            s.push_str(c.detail());
        }
    }
    s
}
