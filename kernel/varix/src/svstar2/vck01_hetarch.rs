//! CGPU-F1601 · K 域开工与异构总架构（CGPU-K 域 · 异构扩展 · 开工单 340 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F1601`
//!
//! 锚点原文：「K 域开工：域使命声明（异构扩展——让 NPU/DSP/媒体引擎成为 GPU 的
//! "同僚"而非旁观者：统一任务图、统一预算、统一遥测下的多引擎协同——异构是
//! 性能与能效的第二曲线）；J 域移交包签收记录（七件签收——F1597 移交兑现起点）；
//! **F0406 预留兑现声明**（B 域任务图"资源类型枚举开放（NPU/DSP 任务预算标签
//! 扩展——资源类型枚举开放）"→本域兑现：ResourceType 枚举扩展为
//! {CPU,GPU,NPU,DSP,ISP,MEDIA}——枚举开放设计落地——兑现记录）；官方主题映射
//! （异构资源抽象/NPU 推理/DSP 卸载/协同调度/异构遥测——十组映射表）；架构五层
//! （资源抽象层→任务模型层→协同调度层→一致性同步层→遥测归因层）；与 B 域关系
//! （任务图管全引擎正确性与依赖——K 扩展资源维度不改 B 内核——扩展不改核声明）；
//! 与 J 域关系（异构功耗热记账进 J 域统一账本——功耗标签复用 F1458 模式）；不变量
//! （异构不破坏 80 帧合同——合同跨引擎成立）；风险（引擎差异大/一致性复杂/能力
//! 碎片化——抽象层+能力探测预案）；测试（使命/签收/枚举兑现/映射/五层/两关系/
//! 不变量/风险八组）。判据：使命、签收、枚举兑现、映射、五层、B/J 关系、不变量、
//! 风险、八组、判据。」
//!
//! # 一、使命是**声明**不是口号：异构是第二曲线
//!
//! [`MISSION`] 把域使命原句承载为常量：NPU/DSP/媒体引擎成为 GPU 的**同僚**——
//! 统一任务图（B 域）、统一预算（C 域）、统一遥测（J 域）下的多引擎协同。「同僚
//! 而非旁观者」是可 grep 的判据词，不是愿景板上的贴纸。
//!
//! # 二、签收是**状态机**不是会议纪要：七件逐一签、缺件不兑付
//!
//! J 域移交包（F1597）七件名册 [`HANDOFF_SEVEN`] 落成签收状态机
//! [`HandoffLedger`]：逐件签收（[`HandoffLedger::sign`]）、重复签显性拒、越界拒、
//! 全签才 [`HandoffLedger::ready_to_extend`]——「移交兑现起点」由此可机检：七件
//! 没签齐之前，K 域扩展不动工。
//!
//! # 三、预留兑现是**类型落地**不是备忘录：ResourceType 六值封闭
//!
//! F0406 预留「资源类型枚举开放」在本域兑现：[`ResourceType`] 从 {CPU,GPU}
//! 扩展为六值封闭枚举 {CPU,GPU,NPU,DSP,ISP,MEDIA}，兑现记录
//! [`PROMISE_F0406`] 把预留原文与兑现单号写进类型——枚举开放设计落地是
//! **有账可查的动作**。
//!
//! # 四、不变量跨引擎成立：异构不破坏 80 帧合同
//!
//! [`invariant_contract_holds`] 把域不变量落成判定：每引擎各自满足帧预算
//! **且**归并结果确定性成立，合同才算跨引擎成立——任一为假即不变量破缺
//! （显性返回假，不静默通过）。

use alloc::string::String;

// ---------------------------------------------------------------------------
// 一、诊断码（vck01 独占段 0x6B..）
// ---------------------------------------------------------------------------

/// 签收越界（名册只有七件——第八件不存在）。
pub const E_K601_SIGN_RANGE: u16 = 0x6B00;
/// 重复签收（签过的件再签——显性拒，不静默去重）。
pub const E_K601_DUP_SIGN: u16 = 0x6B01;
/// 签收未齐即开工（七件没签齐 K 域扩展不动工）。
pub const E_K601_NOT_READY: u16 = 0x6B02;
/// 组映射越界（承接主题号超出 0..=5）。
pub const E_K601_THEME_RANGE: u16 = 0x6B03;
/// 层序越界（架构只有五层——第六层不存在）。
pub const E_K601_LAYER_RANGE: u16 = 0x6B04;
/// 引擎位越界（不变量判定的引擎表外引用）。
pub const E_K601_ENGINE_RANGE: u16 = 0x6B05;

/// 域版本。
pub const VCK01_VERSION: &str = "CK01-hetarch-v1";

// ---------------------------------------------------------------------------
// 二、域使命声明（可 grep 的判据词）
// ---------------------------------------------------------------------------

/// 域使命（锚点「域使命声明」原句承载——「同僚」「第二曲线」是判据词）。
pub const MISSION: &str =
    "让 NPU/DSP/媒体引擎成为 GPU 的同僚而非旁观者：统一任务图、统一预算、\
统一遥测下的多引擎协同——异构是性能与能效的第二曲线";

// ---------------------------------------------------------------------------
// 三、J 域移交包七件签收（F1597 移交兑现起点）
// ---------------------------------------------------------------------------

/// J 域移交包七件名册（F1597 锚点「包内容七件」——签收对象规格）。
pub const HANDOFF_SEVEN: [&str; 7] = [
    "接口冻结清单（107 函数 v1）",
    "契约清单（八域契约四要素）",
    "资产清单（测试/性能精度/安全/长稳/文档五账）",
    "基线快照（开销收益总册+精度表）",
    "遗留移交清单（智能排程/统计模型/PDF 报告+处置建议）",
    "异构衔接包（F0406 资源类型枚举开放设计移交+功耗标签复用指南）",
    "经验教训十条",
];

/// 七件签收账（逐件签、重复拒、缺件不兑付——签收是状态机不是纪要）。
#[derive(Clone, Debug)]
pub struct HandoffLedger {
    signed: [bool; 7],
    /// 签收动作计数（签收历史的可审计面）。
    pub sign_actions: u32,
}

impl HandoffLedger {
    /// 新账（七件全未签）。
    pub fn new() -> HandoffLedger {
        HandoffLedger { signed: [false; 7], sign_actions: 0 }
    }

    /// **sign**：逐件签收。越界拒 [`E_K601_SIGN_RANGE`]；重复签拒
    /// [`E_K601_DUP_SIGN`]（签收历史不可被静默改写）。
    pub fn sign(&mut self, item: usize) -> Result<(), u16> {
        if item >= 7 {
            return Err(E_K601_SIGN_RANGE);
        }
        if self.signed[item] {
            return Err(E_K601_DUP_SIGN);
        }
        self.signed[item] = true;
        self.sign_actions += 1;
        Ok(())
    }

    /// 某件是否已签。
    pub fn is_signed(&self, item: usize) -> Result<bool, u16> {
        if item >= 7 {
            return Err(E_K601_SIGN_RANGE);
        }
        Ok(self.signed[item])
    }

    /// 已签件数。
    pub fn signed_count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < 7 {
            if self.signed[i] {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// **ready_to_extend**：七件全签才许 K 域扩展动工（移交兑现起点）。
    pub fn ready_to_extend(&self) -> bool {
        self.signed_count() == 7
    }
}

// ---------------------------------------------------------------------------
// 四、F0406 预留兑现声明：ResourceType 六值封闭枚举
// ---------------------------------------------------------------------------

/// F0406 预留兑现记录（预留原文→兑现单号——枚举开放设计落地有账可查）。
pub struct PromiseRecord {
    /// 预留出处（B 域任务图预留原文要点）。
    pub promise: &'static str,
    /// 预留来源单号。
    pub source: &'static str,
    /// 兑现单号（本域开工单）。
    pub fulfilled_by: &'static str,
    /// 兑现前枚举值数（原 {CPU,GPU}）。
    pub before_values: usize,
    /// 兑现后枚举值数（六值封闭）。
    pub after_values: usize,
}

/// F0406 兑现声明常量。
pub const PROMISE_F0406: PromiseRecord = PromiseRecord {
    promise: "B 域任务图资源类型枚举开放（NPU/DSP 任务预算标签扩展）",
    source: "CGPU-F0406",
    fulfilled_by: "CGPU-F1601",
    before_values: 2,
    after_values: 6,
};

/// 任务图引擎资源类型（F0406 预留兑现：{CPU,GPU} → 六值封闭全集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResourceType {
    /// CPU 通用算力。
    Cpu,
    /// GPU 图形与并行算力。
    Gpu,
    /// NPU 神经网络推理。
    Npu,
    /// DSP 数字信号处理。
    Dsp,
    /// ISP 图像信号流水线。
    Isp,
    /// MEDIA 专用编解码引擎。
    Media,
}

impl ResourceType {
    /// 全枚举（顺序即下标）。
    pub const ALL: [ResourceType; 6] = [
        ResourceType::Cpu,
        ResourceType::Gpu,
        ResourceType::Npu,
        ResourceType::Dsp,
        ResourceType::Isp,
        ResourceType::Media,
    ];

    /// 枚举下标。
    pub const fn ordinal(self) -> usize {
        match self {
            ResourceType::Cpu => 0,
            ResourceType::Gpu => 1,
            ResourceType::Npu => 2,
            ResourceType::Dsp => 3,
            ResourceType::Isp => 4,
            ResourceType::Media => 5,
        }
    }

    /// 下标 → 枚举（越界 None）。
    pub const fn of_ordinal(i: usize) -> Option<ResourceType> {
        match i {
            0 => Some(ResourceType::Cpu),
            1 => Some(ResourceType::Gpu),
            2 => Some(ResourceType::Npu),
            3 => Some(ResourceType::Dsp),
            4 => Some(ResourceType::Isp),
            5 => Some(ResourceType::Media),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// 五、官方主题映射（五主题封闭 + 十组映射表）
// ---------------------------------------------------------------------------

/// K 域官方主题（封闭五主题——锚点「官方主题映射」）。
pub const THEMES: [&str; 5] =
    ["异构资源抽象", "NPU 推理", "DSP 卸载", "协同调度", "异构遥测"];

/// 域任务段（F1601-F1760，160 单）。
pub const DOMAIN_RANGE: (u32, u32) = (1601, 1760);

/// 组规划条目（十组映射表）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Group {
    /// 组号（1..=10，对应 K01..K10）。
    pub gid: u8,
    /// 组名。
    pub name: &'static str,
    /// 承接主题号（1..=5；开工组/收口组可空段用 0——0 仅限 K01/K10）。
    pub theme: u8,
}

/// 十组封闭表（锚点「十组映射表」——组段 F01..F10 连续无洞）。
pub const GROUPS: [Group; 10] = [
    Group { gid: 1, name: "K01 开工与总架构", theme: 0 },
    Group { gid: 2, name: "K02 资源抽象", theme: 1 },
    Group { gid: 3, name: "K03 NPU 推理", theme: 2 },
    Group { gid: 4, name: "K04 DSP 卸载", theme: 3 },
    Group { gid: 5, name: "K05 协同调度", theme: 4 },
    Group { gid: 6, name: "K06 异构遥测", theme: 5 },
    Group { gid: 7, name: "K07 协同深化", theme: 4 },
    Group { gid: 8, name: "K08 遥测深化", theme: 5 },
    Group { gid: 9, name: "K09 场景扩展", theme: 1 },
    Group { gid: 10, name: "K10 域收口", theme: 0 },
];

/// 组号连续性核对（K01..K10 无洞无重——判据侧独立重算同一结果）。
pub fn groups_contiguous() -> bool {
    let mut ok = true;
    let mut expect = 1u8;
    let mut i = 0usize;
    while i < GROUPS.len() {
        if GROUPS[i].gid != expect {
            ok = false;
        }
        expect += 1;
        i += 1;
    }
    ok && expect == 11
}

/// 组-主题映射合法（承接主题号在 0..=5；0 仅限开工/收口组 K01/K10）。
pub fn group_themes_valid() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < GROUPS.len() {
        let t = GROUPS[i].theme;
        if t > 5 {
            ok = false;
        }
        if t == 0 && GROUPS[i].gid != 1 && GROUPS[i].gid != 10 {
            ok = false;
        }
        i += 1;
    }
    ok
}

/// 域段单数守恒（F1601-F1760 恰 160 单——判据侧独立重算）。
pub const fn domain_size() -> usize {
    (DOMAIN_RANGE.1 - DOMAIN_RANGE.0 + 1) as usize
}

// ---------------------------------------------------------------------------
// 六、架构五层（顺序即秩）
// ---------------------------------------------------------------------------

/// 架构五层封闭表（锚点「架构五层」——资源抽象→任务模型→协同调度→一致性同步→遥测归因）。
pub const ARCH_LAYERS: [&str; 5] =
    ["资源抽象层", "任务模型层", "协同调度层", "一致性同步层", "遥测归因层"];

/// 层名查询（越界 None——第六层不存在）。
pub fn layer_of(i: usize) -> Result<&'static str, u16> {
    if i >= ARCH_LAYERS.len() {
        return Err(E_K601_LAYER_RANGE);
    }
    Ok(ARCH_LAYERS[i])
}

// ---------------------------------------------------------------------------
// 七、两域关系与不变量
// ---------------------------------------------------------------------------

/// 与 B 域关系（锚点「扩展不改核声明」——K 扩展资源维度不改 B 内核）。
pub const B_RELATION: &str = "任务图管全引擎正确性与依赖——K 扩展资源维度不改 B 内核";

/// 与 J 域关系（锚点「功耗标签复用 F1458 模式」——异构功耗热记账进 J 域统一账本）。
pub const J_RELATION: &str = "异构功耗热记账进 J 域统一账本——功耗标签复用 F1458 模式";

/// 域不变量（锚点「异构不破坏 80 帧合同——合同跨引擎成立」）。
pub const INVARIANT_80FPS: &str = "异构不破坏 80 帧合同——合同跨引擎成立";

/// **不变量判定**：每引擎各自满足帧预算**且**归并结果确定性成立，合同才跨引擎成立。
///
/// 任一为假即不变量破缺——显性返回假，不静默通过。
pub const fn invariant_contract_holds(per_engine_budget_ok: bool, merge_deterministic: bool) -> bool {
    per_engine_budget_ok && merge_deterministic
}

/// 域风险三条（锚点「风险」——各配预案，风险不裸奔）。
pub const RISKS: [(&str, &str); 3] = [
    ("引擎差异大", "抽象层统一接口——差异收敛在类型卡"),
    ("一致性复杂", "一致性同步层集中处理——不散落各消费方"),
    ("能力碎片化", "能力探测预案——按实探能力分派不假设"),
];

/// 摘要行（面板/日志共用）。
pub fn screen_line() -> String {
    let mut s = String::new();
    s.push_str(VCK01_VERSION);
    s.push_str(" themes=");
    s.push_str(&alloc::format!("{}", THEMES.len()));
    s.push_str(" groups=10 range=1601-1760 layers=5 resources=6");
    s
}

// ---------------------------------------------------------------------------
// 编译期闸
// ---------------------------------------------------------------------------

const _: () = {
    assert!(HANDOFF_SEVEN.len() == 7);
    assert!(THEMES.len() == 5);
    assert!(GROUPS.len() == 10);
    assert!(ARCH_LAYERS.len() == 5);
    assert!(RISKS.len() == 3);
    assert!(ResourceType::ALL.len() == 6);
    assert!(PROMISE_F0406.before_values == 2 && PROMISE_F0406.after_values == 6);
    assert!(DOMAIN_RANGE.0 == 1601 && DOMAIN_RANGE.1 == 1760);
    assert!((DOMAIN_RANGE.1 - DOMAIN_RANGE.0 + 1) as usize == 160);
    assert!(E_K601_SIGN_RANGE & 0xFF00 == 0x6B00);
    assert!(E_K601_DUP_SIGN & 0xFF00 == 0x6B00);
    assert!(E_K601_NOT_READY & 0xFF00 == 0x6B00);
    assert!(E_K601_THEME_RANGE & 0xFF00 == 0x6B00);
    assert!(E_K601_LAYER_RANGE & 0xFF00 == 0x6B00);
    assert!(E_K601_ENGINE_RANGE & 0xFF00 == 0x6B00);
    assert!(
        E_K601_SIGN_RANGE != E_K601_DUP_SIGN
            && E_K601_DUP_SIGN != E_K601_NOT_READY
            && E_K601_NOT_READY != E_K601_THEME_RANGE
            && E_K601_THEME_RANGE != E_K601_LAYER_RANGE
            && E_K601_LAYER_RANGE != E_K601_ENGINE_RANGE
    );
};
