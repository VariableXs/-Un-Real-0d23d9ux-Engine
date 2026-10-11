//! CGPU-F3681 · X 域开工与十年硬件演进总架构（CGPU-X 域 · 十年硬件演进 · 批次 X01）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F3681`
//!
//! 锚点原文：「域开工：X 域开工（签收 W10 移交包（演进衔接包签收——衔接确认）；
//! 演进定位（十年硬件浪潮下的前瞻设计——定位声明；四段架构（预测/规划/预留/
//! 风险——四段（W10 预告兑现（文档章节演进预留——兑现确认；测试（签收/定位/
//! 四段/兑现四组）。判据：签收、定位声明、四段、兑现确认、四组、判据。」
//!
//! 判据逐条落位：
//! - **签收**：[`sign_off`]——W10 移交包七件（[`HANDOVER_SEVEN`]，封闭）逐一
//!   在场才收讫，缺件 [`E_X810_MISSING_ITEM`]；演进衔接包必须衔接确认
//!   （[`LINK_PACKAGE`] 两件在场），未确认 [`E_X810_LINK_UNCONFIRMED`]。
//! - **定位声明**：[`POSITIONING`]——「十年硬件浪潮下的前瞻设计」；声明失守
//!   （丢「前瞻」）即 [`E_X810_POSITIONING_FAULT`]：演进域没有前瞻就只剩
//!   追赶，追赶永远慢一个硬件世代。
//! - **四段**：[`EvolveSegment`] 四值封闭（预测/规划/预留/风险）+
//!   [`SEGMENT_CHARTER`] 平行章程；残缺 [`E_X810_SEGMENT_INCOMPLETE`]；
//!   风险段必须带「可撤销」（预留若无退出路径就是负资产），缺席
//!   [`E_X810_REVERSIBILITY_MISSING`]。
//! - **兑现确认**：[`verify_w10_fulfilled`]——W10 预告的文档章节演进预留
//!   （[`W10_FULFILLMENT`]）必须非空在场，空欠 [`E_X810_W10_UNFULFILLED`]。
//! - **四组**：判据侧 [`super::vcx81_hwevolve_checks`] 签收/定位/四段/兑现
//!   四组全量断言。
//!
//! **为什么「可撤销」是风险段的铁律**：硬件预测注定部分落空——预测-规划的
//! 价值不在全对，而在落空的部分**能干净退出**。不可撤销的预留把每一次
//! 预测错误都变成永久负担，十年尺度下复利成灾。
//!
//! **零静默纪律**：所有缺件/未确认/失守/残缺/空欠都产出诊断码（0x99C 段，
//! X 域独占细分），由调用方聚合上报；开工总账（[`x_domain_total`]）
//! 可 grep 可对账。
//!
//! 确定性：全部封闭常量表 + 纯函数，时间戳由调用方注入（本模块不读时钟）。

use alloc::string::String;

// ---------------------------------------------------------------------------
// 一、诊断码（X 域独占 0x99C 段）
// ---------------------------------------------------------------------------

/// 移交包缺件（七件未齐即拒收——缺一件等于移交未完成）。
pub const E_X810_MISSING_ITEM: u16 = 0x99C0;
/// 衔接未确认（演进衔接包在场但衔接确认缺席）。
pub const E_X810_LINK_UNCONFIRMED: u16 = 0x99C1;
/// 定位失守（定位声明丢「前瞻」——演进域退化为追赶）。
pub const E_X810_POSITIONING_FAULT: u16 = 0x99C2;
/// 四段残缺（预测/规划/预留/风险任一段章程缺席）。
pub const E_X810_SEGMENT_INCOMPLETE: u16 = 0x99C3;
/// 风险段缺可撤销（章程未声明「可撤销」——预留无退出路径）。
pub const E_X810_REVERSIBILITY_MISSING: u16 = 0x99C4;
/// W10 兑现空欠（文档章节演进预留兑现物空缺——预告不兑现）。
pub const E_X810_W10_UNFULFILLED: u16 = 0x99C5;
/// 版本失配（批次版本与 X01 冻结版不符）。
pub const E_X810_VERSION_MISMATCH: u16 = 0x99C6;

/// 域批次版本。
pub const VCX81_VERSION: &str = "CX81-hwevolve-v1";

/// 版本校验（双向）：X01 冻结版放行，其余一律 [`E_X810_VERSION_MISMATCH`]。
pub fn check_version(v: &str) -> Result<(), u16> {
    if v == VCX81_VERSION {
        Ok(())
    } else {
        Err(E_X810_VERSION_MISMATCH)
    }
}

// ---------------------------------------------------------------------------
// 二、签收：W10 移交包七件（封闭清单）+ 衔接确认
// ---------------------------------------------------------------------------

/// W10 移交包七件（封闭——CGPU-F3680 产出；次序即签收点验序）。
pub const HANDOVER_SEVEN: [&str; 7] = [
    "接口冻结",
    "契约",
    "资产",
    "基线",
    "遗留",
    "演进衔接包",
    "教训十条",
];

/// 演进衔接包内两件（W10 预告兑现的名目——兑现确认的对照基准）。
pub const LINK_PACKAGE: [&str; 2] = ["文档章节演进预留", "硬件演进观察口径"];

/// 签收回执（七件收讫 + 衔接确认；版本随 X 域开工冻结）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HandoverReceipt {
    /// 实收件数（收讫时恒为 7）。
    pub items_signed: usize,
    /// 衔接确认（演进衔接包已点验）。
    pub link_confirmed: bool,
    /// 签收批次版本。
    pub version: String,
}

/// 签收：七件逐一在场 + 衔接确认在场，二者齐备才收讫。
///
/// 缺件（任一位 false）返回 [`E_X810_MISSING_ITEM`]；七件齐但衔接确认缺席
/// 返回 [`E_X810_LINK_UNCONFIRMED`]——衔接包是七件里唯一承载 W10 预告兑现的
/// 一件，只点验不确认等于没接。
pub fn sign_off(items_present: [bool; 7], link_confirmed: bool) -> Result<HandoverReceipt, u16> {
    let mut signed: usize = 0;
    for present in items_present {
        if present {
            signed = signed + 1;
        }
    }
    if signed < 7 {
        return Err(E_X810_MISSING_ITEM);
    }
    if !link_confirmed {
        return Err(E_X810_LINK_UNCONFIRMED);
    }
    Ok(HandoverReceipt {
        items_signed: 7,
        link_confirmed: true,
        version: String::from(VCX81_VERSION),
    })
}

// ---------------------------------------------------------------------------
// 三、定位声明：十年硬件浪潮下的前瞻设计
// ---------------------------------------------------------------------------

/// 演进定位声明（锚点原文——「前瞻」为不可删字，删即定位失守）。
pub const POSITIONING: &str = "十年硬件浪潮下的前瞻设计";

/// 演进纪律声明（预测不押注单点——浪潮是分布不是事件）。
pub const EVOLVE_DISCIPLINE: &str = "预测给区间不押单点，预留给接口不给实现";

/// 定位声明校验：声明必须落在「前瞻」上，纯追赶式声明即失守。
pub fn verify_positioning(declared: &str) -> Result<(), u16> {
    if declared.contains("前瞻") {
        Ok(())
    } else {
        Err(E_X810_POSITIONING_FAULT)
    }
}

// ---------------------------------------------------------------------------
// 四、四段架构：预测 / 规划 / 预留 / 风险（封闭四值 + 平行章程）
// ---------------------------------------------------------------------------

/// 演进四段架构（封闭——锚点「预测/规划/预留/风险」；次序即开工点名序）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EvolveSegment {
    /// 预测：硬件技术趋势的区间化研判。
    Forecast,
    /// 规划：预测到架构路线的转化。
    Roadmap,
    /// 预留：架构面的前瞻适配接口。
    Reserve,
    /// 风险：预测落空的显性账本与退出路径。
    Risk,
}

/// 四段封闭表（次序固定：风险居第 3 位——先有预留再谈退出）。
pub const SEGMENTS: [EvolveSegment; 4] = [
    EvolveSegment::Forecast,
    EvolveSegment::Roadmap,
    EvolveSegment::Reserve,
    EvolveSegment::Risk,
];

/// 四段平行章程（与 [`SEGMENTS`] 按位对齐——每段一句可 grep 的开工声明）。
pub const SEGMENT_CHARTER: [&str; 4] = [
    "预测：硬件趋势给区间不押单点——浪潮是分布不是事件",
    "规划：预测到路线的转化——每条路线标注依据与置信度",
    "预留：架构面只留接口不留实现——实现随硬件落地",
    "风险：预测落空显性记账——每条预留可撤销可退出",
];

/// 四段章程校验：每段章程非空（残缺即 [`E_X810_SEGMENT_INCOMPLETE`]），
/// 且风险段（位 3）章程必须声明「可撤销」（缺席即
/// [`E_X810_REVERSIBILITY_MISSING`]）。
pub fn verify_segment_charters(charters: [&str; 4]) -> Result<(), u16> {
    for charter in charters {
        if charter.is_empty() {
            return Err(E_X810_SEGMENT_INCOMPLETE);
        }
    }
    if !charters[3].contains("可撤销") {
        return Err(E_X810_REVERSIBILITY_MISSING);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 五、W10 预告兑现：文档章节演进预留
// ---------------------------------------------------------------------------

/// 文档章节演进预留（W10 预告兑现物——封闭三条：挂载点/更新规则/退场规则）。
pub const W10_FULFILLMENT: [&str; 3] = [
    "章节挂载点：演进观察与预测的文档章节位与更新节奏",
    "更新规则：硬件节点事件触发的章节更新与复述规则",
    "退场规则：预测证伪后的章节归档与预留撤销记录规则",
];

/// W10 兑现回执（条数 + 兑现确认）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct W10Receipt {
    /// 兑现物条目数。
    pub items: usize,
    /// 兑现确认（兑现物非空在场）。
    pub confirmed: bool,
}

/// 兑现确认：W10 预告的文档章节演进预留必须非空在场，
/// 空欠即 [`E_X810_W10_UNFULFILLED`]——预告不兑现，开工不算数。
pub fn verify_w10_fulfilled() -> Result<W10Receipt, u16> {
    if W10_FULFILLMENT.is_empty() {
        return Err(E_X810_W10_UNFULFILLED);
    }
    Ok(W10Receipt {
        items: W10_FULFILLMENT.len(),
        confirmed: true,
    })
}

// ---------------------------------------------------------------------------
// 六、开工宣告：X 域号段与总账
// ---------------------------------------------------------------------------

/// X 域号段（CGPU-F3681-F3840，十年硬件演进）。
pub const X_DOMAIN_RANGE: (u32, u32) = (3681, 3840);

/// X 域总项数（160——开工总账，可 grep 可对账）。
pub fn x_domain_total() -> u32 {
    X_DOMAIN_RANGE.1 - X_DOMAIN_RANGE.0 + 1
}
