//! CGPU-F2881 · S 域开工与多用户总架构（CGPU-S 域 · 多用户与虚拟化 · 批次 S01）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F2881`
//!
//! 锚点原文：「域开工：S 域开工（签收 R10 移交包（多用户安全衔接包签收——衔接确认）；
//! 多用户定位（一机多租·公平共享 GPU——定位声明；四段架构（模型/隔离/调度/配额——
//! 四段（R10 预告兑现（每用户威胁模型与配额建议——兑现确认；测试（签收/定位/四段/兑现
//! 四组）。判据：签收、定位声明、四段、兑现确认、四组、判据。」
//!
//! 判据逐条落位：
//! - **签收**：[`sign_off`]——R10 移交包七件（[`HANDOVER_SEVEN`]，封闭）逐一在场
//!   才收讫，缺件 [`E_S810_MISSING_ITEM`]；多用户安全衔接包必须衔接确认
//!   （[`LINK_PACKAGE`] 两件在场），未确认 [`E_S810_LINK_UNCONFIRMED`]。
//! - **定位声明**：[`POSITIONING`]——「一机多租·公平共享 GPU」；声明失守
//!   （丢「公平」）即 [`E_S810_POSITIONING_FAULT`]：一机多租不容独占，
//!   定位不是口号，是后续每条 S 域判据的仲裁原点。
//! - **四段**：[`Segment`] 四值封闭（模型/隔离/调度/配额）+ [`SEGMENT_CHARTER`]
//!   平行章程；残缺 [`E_S810_SEGMENT_INCOMPLETE`]；隔离段必须带公平铁律
//!   （「不互窃」——R 域隔离失败🔴纪律的会话版承接），缺席
//!   [`E_S810_FAIRNESS_MISSING`]。
//! - **兑现确认**：[`verify_r10_fulfilled`]——R 域预告的每用户威胁模型
//!   （[`THREAT_MODEL`]）与配额建议（[`QUOTA_ADVICE`]）两件兑现物必须非空在场，
//!   空欠 [`E_S810_R10_UNFULFILLED`]：预告不兑现即空欠，开工不算数。
//! - **四组**：判据侧 [`super::vcs81_sdomain_arch_checks`] 签收/定位/四段/兑现四组
//!   全量断言。
//!
//! **为什么「公平」是定位的不可删字**：一机多租把 GPU 从独占变成共享，
//! 共享秩序只有一条底线——任何用户不得借结构侵蚀他人份额。定位声明里
//! 删掉「公平」，四段里的隔离与配额就失去仲裁原点，沦为纯技术配置。
//!
//! **零静默纪律**：所有缺件/未确认/失守/残缺/空欠都产出诊断码
//! （0x9B 段，S 域独占细分），由调用方聚合上报；开工宣告本身也是显性事件
//! （[`s_domain_total`] 可 grep 可对账）。
//!
//! 确定性：全部封闭常量表 + 纯函数，时间戳由调用方注入（本模块不读时钟）。

use alloc::string::String;

// ---------------------------------------------------------------------------
// 一、诊断码（S 域独占 0x9B 段，细分 0x9B0x）
// ---------------------------------------------------------------------------

/// 移交包缺件（七件未齐即拒收——缺一件等于移交未完成）。
pub const E_S810_MISSING_ITEM: u16 = 0x9B00;
/// 衔接未确认（多用户安全衔接包在场但衔接确认缺席）。
pub const E_S810_LINK_UNCONFIRMED: u16 = 0x9B01;
/// 定位失守（定位声明丢「公平」——一机多租不容独占）。
pub const E_S810_POSITIONING_FAULT: u16 = 0x9B02;
/// 四段残缺（模型/隔离/调度/配额任一段章程缺席）。
pub const E_S810_SEGMENT_INCOMPLETE: u16 = 0x9B03;
/// 隔离段缺公平铁律（章程未声明「不互窃」——隔离失败🔴纪律的会话版承接缺席）。
pub const E_S810_FAIRNESS_MISSING: u16 = 0x9B04;
/// R10 兑现空欠（威胁模型或配额建议兑现物空缺——预告不兑现）。
pub const E_S810_R10_UNFULFILLED: u16 = 0x9B05;
/// 版本失配（批次版本与开工冻结版不符）。
pub const E_S810_VERSION_MISMATCH: u16 = 0x9B06;

/// 域批次版本。
pub const VCS81_VERSION: &str = "CS81-sdomain-v1";

/// 版本校验（双向）：开工冻结版放行，其余一律 [`E_S810_VERSION_MISMATCH`]。
pub fn check_version(v: &str) -> Result<(), u16> {
    if v == VCS81_VERSION {
        Ok(())
    } else {
        Err(E_S810_VERSION_MISMATCH)
    }
}

// ---------------------------------------------------------------------------
// 二、签收：R10 移交包七件（封闭清单）+ 衔接确认
// ---------------------------------------------------------------------------

/// R10 移交包七件（封闭——CGPU-F2877 产出；次序即签收点验序）。
pub const HANDOVER_SEVEN: [&str; 7] = [
    "接口冻结",
    "契约",
    "资产",
    "基线",
    "遗留",
    "多用户安全衔接包",
    "教训十条",
];

/// 多用户安全衔接包内两件（R10 预告兑现的名目——兑现确认的对照基准）。
pub const LINK_PACKAGE: [&str; 2] = ["每用户威胁模型", "每用户配额建议"];

/// 签收回执（七件收讫 + 衔接确认；版本随 S 域开工冻结）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HandoverReceipt {
    /// 实收件数（收讫时恒为 7）。
    pub items_signed: usize,
    /// 衔接确认（多用户安全衔接包已点验）。
    pub link_confirmed: bool,
    /// 签收批次版本。
    pub version: String,
}

/// 签收：七件逐一在场 + 衔接确认在场，二者齐备才收讫。
///
/// 缺件（任一位 false）返回 [`E_S810_MISSING_ITEM`]；七件齐但衔接确认缺席
/// 返回 [`E_S810_LINK_UNCONFIRMED`]——衔接包是七件里唯一承载 R10 预告兑现的
/// 一件，只点验不确认等于没接。
pub fn sign_off(items_present: [bool; 7], link_confirmed: bool) -> Result<HandoverReceipt, u16> {
    let mut signed: usize = 0;
    for present in items_present {
        if present {
            signed = signed + 1;
        }
    }
    if signed < 7 {
        return Err(E_S810_MISSING_ITEM);
    }
    if !link_confirmed {
        return Err(E_S810_LINK_UNCONFIRMED);
    }
    Ok(HandoverReceipt {
        items_signed: 7,
        link_confirmed: true,
        version: String::from(VCS81_VERSION),
    })
}

// ---------------------------------------------------------------------------
// 三、定位声明：一机多租·公平共享 GPU
// ---------------------------------------------------------------------------

/// 多用户定位声明（锚点原文——「公平」为不可删字，删即定位失守）。
pub const POSITIONING: &str = "一机多租·公平共享 GPU";

/// 公平铁律（会话版）——隔离段章程的仲裁原点（R 域隔离失败🔴纪律承接）。
pub const FAIRNESS_IRONLAW: &str = "用户间不互窃——公平铁律的会话版";

/// 定位声明校验：声明必须落在「公平共享」上，独占式声明即失守。
pub fn verify_positioning(declared: &str) -> Result<(), u16> {
    if declared.contains("公平") {
        Ok(())
    } else {
        Err(E_S810_POSITIONING_FAULT)
    }
}

// ---------------------------------------------------------------------------
// 四、四段架构：模型 / 隔离 / 调度 / 配额（封闭四值 + 平行章程）
// ---------------------------------------------------------------------------

/// 四段架构（封闭——锚点「模型/隔离/调度/配额」；次序即开工点名序）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Segment {
    /// 模型：多用户会话与角色的承载结构。
    Model,
    /// 隔离：用户间不互窃——公平铁律的会话版。
    Isolation,
    /// 调度：多会话公平共享 GPU 时隙。
    Scheduling,
    /// 配额：每用户预算与用量的秩序。
    Quota,
}

/// 四段封闭表（次序固定：隔离居第 1 位——公平铁律必须先于调度与配额被声明）。
pub const SEGMENTS: [Segment; 4] = [
    Segment::Model,
    Segment::Isolation,
    Segment::Scheduling,
    Segment::Quota,
];

/// 四段平行章程（与 [`SEGMENTS`] 按位对齐——每段一句可 grep 的开工声明）。
pub const SEGMENT_CHARTER: [&str; 4] = [
    "模型：多用户会话与角色的承载结构",
    "隔离：用户间不互窃——公平铁律的会话版",
    "调度：多会话公平共享 GPU 时隙",
    "配额：每用户预算与用量的秩序",
];

/// 四段章程校验：每段章程非空（残缺即 [`E_S810_SEGMENT_INCOMPLETE`]），
/// 且隔离段（位 1）章程必须声明「不互窃」（缺席即 [`E_S810_FAIRNESS_MISSING`]）。
pub fn verify_four_segment_charters(charters: [&str; 4]) -> Result<(), u16> {
    for charter in charters {
        if charter.is_empty() {
            return Err(E_S810_SEGMENT_INCOMPLETE);
        }
    }
    if !charters[1].contains("不互窃") {
        return Err(E_S810_FAIRNESS_MISSING);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 五、R10 预告兑现：每用户威胁模型 + 每用户配额建议
// ---------------------------------------------------------------------------

/// 每用户威胁模型（R10 预告兑现物——封闭三条，越权/窃配额/侧信道）。
pub const THREAT_MODEL: [&str; 3] = [
    "越权会话：用户 A 不得触及用户 B 的渲染上下文",
    "配额窃取：用户不得透支他人预算",
    "侧信道：跨用户时序观测必须显性披露",
];

/// 每用户配额建议（R10 预告兑现物——封闭三条，保底/弹性/超额显性）。
pub const QUOTA_ADVICE: [&str; 3] = [
    "基础份额：每用户保底 1/N 算力时隙",
    "弹性上限：空闲回收，忙时不满发",
    "超额显性：超限请求拒绝并复述原因",
];

/// R10 兑现回执（两件兑现物的条数 + 兑现确认）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct R10Fulfillment {
    /// 威胁模型条目数。
    pub threats: usize,
    /// 配额建议条目数。
    pub quotas: usize,
    /// 兑现确认（两件兑现物均非空在场）。
    pub confirmed: bool,
}

/// 兑现确认：R10 预告的两件（威胁模型/配额建议）兑现物必须非空在场，
/// 任一件空欠即 [`E_S810_R10_UNFULFILLED`]——预告不兑现，开工不算数。
pub fn verify_r10_fulfilled() -> Result<R10Fulfillment, u16> {
    if THREAT_MODEL.is_empty() || QUOTA_ADVICE.is_empty() {
        return Err(E_S810_R10_UNFULFILLED);
    }
    Ok(R10Fulfillment {
        threats: THREAT_MODEL.len(),
        quotas: QUOTA_ADVICE.len(),
        confirmed: true,
    })
}

// ---------------------------------------------------------------------------
// 六、开工宣告：S 域号段与总账
// ---------------------------------------------------------------------------

/// S 域号段（CGPU-F2881-F3040，多用户与虚拟化）。
pub const S_DOMAIN_RANGE: (u32, u32) = (2881, 3040);

/// S 域总项数（160——开工总账，可 grep 可对账）。
pub fn s_domain_total() -> u32 {
    S_DOMAIN_RANGE.1 - S_DOMAIN_RANGE.0 + 1
}
