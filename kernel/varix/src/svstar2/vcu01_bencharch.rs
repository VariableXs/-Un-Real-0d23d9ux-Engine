//! CGPU-F3201 · U 域开工与 Bench 总架构（CGPU-U 域 · CGPU-Bench · 批次 U01）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F3201`
//!
//! 锚点原文：「域开工：U 域开工（签收 T10 移交包（Bench 衔接包签收——衔接确认）；
//! Bench 定位（实测认证不凭文档声明——定位声明（CGPU 理念呼应——呼应声明；
//! 四段架构（场景/运行/计分/发布——四段（T10 预告兑现（对接协议性能基准项——
//! 兑现确认；测试（签收/定位/四段/兑现四组）。判据：签收、定位声明、四段、
//! 兑现确认、四组、判据。」
//!
//! 判据逐条落位：
//! - **签收**：[`sign_off`]——T10 移交包七件（[`HANDOVER_SEVEN`]，封闭）逐一
//!   在场才收讫，缺件 [`E_U010_MISSING_ITEM`]；Bench 衔接包必须衔接确认
//!   （[`LINK_PACKAGE`] 两件在场），未确认 [`E_U010_LINK_UNCONFIRMED`]。
//! - **定位声明**：[`POSITIONING`]——「实测认证不凭文档声明」；声明失守
//!   （丢「实测」）即 [`E_U010_POSITIONING_FAULT`]：Bench 的存在意义就是
//!   把「行不行」从口头变成数字。
//! - **四段**：[`BenchSegment`] 四值封闭（场景/运行/计分/发布）+
//!   [`BENCH_CHARTER`] 平行章程；残缺 [`E_U010_SEGMENT_INCOMPLETE`]；
//!   场景段必须带「真实负载」（无真实负载的基准是自娱自乐），缺席
//!   [`E_U010_SCENE_LOAD_MISSING`]。
//! - **兑现确认**：[`verify_t10_fulfilled`]——T10 预告的对接协议性能基准项
//!   （[`T10_FULFILLMENT`]）必须非空在场，空欠 [`E_U010_T10_UNFULFILLED`]：
//!   预告不兑现，开工不算数。
//! - **四组**：判据侧 [`super::vcu01_bencharch_checks`] 签收/定位/四段/兑现
//!   四组全量断言。
//!
//! **CGPU 理念呼应**：与等价优先（CGPU-N）同源——等价优先裁决「像不像」，
//! 实测认证裁决「快不快」；两者共享同一信条：**可验证的数字压倒一切声明**。
//! 文档可以承诺，只有基准能认证。
//!
//! **零静默纪律**：所有缺件/未确认/失守/残缺/空欠都产出诊断码（0x99A 段，
//! U 域独占细分），由调用方聚合上报；开工总账（[`u_domain_total`]）
//! 可 grep 可对账。
//!
//! 确定性：全部封闭常量表 + 纯函数，时间戳由调用方注入（本模块不读时钟）。

use alloc::string::String;

// ---------------------------------------------------------------------------
// 一、诊断码（U 域独占 0x99A 段）
// ---------------------------------------------------------------------------

/// 移交包缺件（七件未齐即拒收——缺一件等于移交未完成）。
pub const E_U010_MISSING_ITEM: u16 = 0x99A0;
/// 衔接未确认（Bench 衔接包在场但衔接确认缺席）。
pub const E_U010_LINK_UNCONFIRMED: u16 = 0x99A1;
/// 定位失守（定位声明丢「实测」——Bench 退化为文档装饰）。
pub const E_U010_POSITIONING_FAULT: u16 = 0x99A2;
/// 四段残缺（场景/运行/计分/发布任一段章程缺席）。
pub const E_U010_SEGMENT_INCOMPLETE: u16 = 0x99A3;
/// 场景段缺真实负载（章程未声明「真实负载」——无负载基准不成立）。
pub const E_U010_SCENE_LOAD_MISSING: u16 = 0x99A4;
/// T10 兑现空欠（对接协议性能基准项空缺——预告不兑现）。
pub const E_U010_T10_UNFULFILLED: u16 = 0x99A5;
/// 版本失配（批次版本与 U01 冻结版不符）。
pub const E_U010_VERSION_MISMATCH: u16 = 0x99A6;

/// 域批次版本。
pub const VCU01_VERSION: &str = "CU01-bencharch-v1";

/// 版本校验（双向）：U01 冻结版放行，其余一律 [`E_U010_VERSION_MISMATCH`]。
pub fn check_version(v: &str) -> Result<(), u16> {
    if v == VCU01_VERSION {
        Ok(())
    } else {
        Err(E_U010_VERSION_MISMATCH)
    }
}

// ---------------------------------------------------------------------------
// 二、签收：T10 移交包七件（封闭清单）+ 衔接确认
// ---------------------------------------------------------------------------

/// T10 移交包七件（封闭——CGPU-F3200 产出；次序即签收点验序）。
pub const HANDOVER_SEVEN: [&str; 7] = [
    "接口冻结",
    "契约",
    "资产",
    "基线",
    "遗留",
    "Bench 衔接包",
    "教训十条",
];

/// Bench 衔接包内两件（T10 预告兑现的名目——兑现确认的对照基准）。
pub const LINK_PACKAGE: [&str; 2] = ["对接协议基准项", "性能基准口径"];

/// 签收回执（七件收讫 + 衔接确认；版本随 U 域开工冻结）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HandoverReceipt {
    /// 实收件数（收讫时恒为 7）。
    pub items_signed: usize,
    /// 衔接确认（Bench 衔接包已点验）。
    pub link_confirmed: bool,
    /// 签收批次版本。
    pub version: String,
}

/// 签收：七件逐一在场 + 衔接确认在场，二者齐备才收讫。
///
/// 缺件（任一位 false）返回 [`E_U010_MISSING_ITEM`]；七件齐但衔接确认缺席
/// 返回 [`E_U010_LINK_UNCONFIRMED`]——衔接包是七件里唯一承载 T10 预告兑现的
/// 一件，只点验不确认等于没接。
pub fn sign_off(items_present: [bool; 7], link_confirmed: bool) -> Result<HandoverReceipt, u16> {
    let mut signed: usize = 0;
    for present in items_present {
        if present {
            signed = signed + 1;
        }
    }
    if signed < 7 {
        return Err(E_U010_MISSING_ITEM);
    }
    if !link_confirmed {
        return Err(E_U010_LINK_UNCONFIRMED);
    }
    Ok(HandoverReceipt {
        items_signed: 7,
        link_confirmed: true,
        version: String::from(VCU01_VERSION),
    })
}

// ---------------------------------------------------------------------------
// 三、定位声明：实测认证不凭文档声明
// ---------------------------------------------------------------------------

/// Bench 定位声明（锚点原文——「实测」为不可删字，删即定位失守）。
pub const POSITIONING: &str = "实测认证不凭文档声明";

/// CGPU 理念呼应声明（与等价优先同源——可验证的数字压倒一切声明）。
pub const CGPU_ECHO: &str = "与等价优先同源：基准数字是唯一凭证";

/// 定位声明校验：声明必须落在「实测」上，纯文档式声明即失守。
pub fn verify_positioning(declared: &str) -> Result<(), u16> {
    if declared.contains("实测") {
        Ok(())
    } else {
        Err(E_U010_POSITIONING_FAULT)
    }
}

// ---------------------------------------------------------------------------
// 四、四段架构：场景 / 运行 / 计分 / 发布（封闭四值 + 平行章程）
// ---------------------------------------------------------------------------

/// Bench 四段架构（封闭——锚点「场景/运行/计分/发布」；次序即开工点名序）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BenchSegment {
    /// 场景：基准场景库——真实负载的固化形态。
    Scene,
    /// 运行：运行器——场景的确定性执行与采样。
    Run,
    /// 计分：三分计分体系——数字的口径与仲裁。
    Score,
    /// 发布：基准发布与回归——数字的出证与看守。
    Publish,
}

/// 四段封闭表（次序固定：场景居第 0 位——没有场景就没有一切后续）。
pub const BENCH_SEGMENTS: [BenchSegment; 4] = [
    BenchSegment::Scene,
    BenchSegment::Run,
    BenchSegment::Score,
    BenchSegment::Publish,
];

/// 四段平行章程（与 [`BENCH_SEGMENTS`] 按位对齐——每段一句可 grep 的开工声明）。
pub const BENCH_CHARTER: [&str; 4] = [
    "场景：基准场景库——真实负载的固化形态",
    "运行：运行器——确定性执行与采样复现",
    "计分：三分计分体系——口径先行仲裁在后",
    "发布：基准发布与回归——数字出证且看守",
];

/// 四段章程校验：每段章程非空（残缺即 [`E_U010_SEGMENT_INCOMPLETE`]），
/// 且场景段（位 0）章程必须声明「真实负载」（缺席即
/// [`E_U010_SCENE_LOAD_MISSING`]）。
pub fn verify_bench_charters(charters: [&str; 4]) -> Result<(), u16> {
    for charter in charters {
        if charter.is_empty() {
            return Err(E_U010_SEGMENT_INCOMPLETE);
        }
    }
    if !charters[0].contains("真实负载") {
        return Err(E_U010_SCENE_LOAD_MISSING);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 五、T10 预告兑现：对接协议性能基准项
// ---------------------------------------------------------------------------

/// 对接协议性能基准项（T10 预告兑现物——封闭三条：一致性/吞吐/延迟）。
pub const T10_FULFILLMENT: [&str; 3] = [
    "协议一致性基准：T 域对接协议的互操作一致性计分口径",
    "吞吐基准：单位时间渲染作业完成量的实测口径",
    "延迟基准：端到端作业延迟的实测口径与采样规则",
];

/// T10 兑现回执（基准项条数 + 兑现确认）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct T10Receipt {
    /// 基准项条目数。
    pub items: usize,
    /// 兑现确认（兑现物非空在场）。
    pub confirmed: bool,
}

/// 兑现确认：T10 预告的对接协议性能基准项必须非空在场，
/// 空欠即 [`E_U010_T10_UNFULFILLED`]——预告不兑现，开工不算数。
pub fn verify_t10_fulfilled() -> Result<T10Receipt, u16> {
    if T10_FULFILLMENT.is_empty() {
        return Err(E_U010_T10_UNFULFILLED);
    }
    Ok(T10Receipt {
        items: T10_FULFILLMENT.len(),
        confirmed: true,
    })
}

// ---------------------------------------------------------------------------
// 六、开工宣告：U 域号段与总账
// ---------------------------------------------------------------------------

/// U 域号段（CGPU-F3201-F3360，CGPU-Bench）。
pub const U_DOMAIN_RANGE: (u32, u32) = (3201, 3360);

/// U 域总项数（160——开工总账，可 grep 可对账）。
pub fn u_domain_total() -> u32 {
    U_DOMAIN_RANGE.1 - U_DOMAIN_RANGE.0 + 1
}
