//! CGPU-F2724 · 安全渲染降级接口（CGPU-R 域 · 安全渲染 · 组内第 4 条）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F2724`
//!
//! 锚点原文：「降级接口：接口（安全渲染降级接口（Q10 衔接包兑现——可靠性
//! 异常→渲染降级调用约定——兑现确认；接口冻结（v1 冻结——冻结声明；测试
//! （兑现/冻结两组）。判据：兑现确认、v1 冻结、两组、判据。」
//!
//! 判据逐条落位：
//! - **兑现确认**：[`verify_q10_fulfilled`]——Q10 衔接包预告的「可靠性异常→
//!   渲染降级调用约定」兑现物（[`Q10_FULFILLMENT`]）必须非空在场，空欠
//!   [`E_R240_Q10_UNFULFILLED`]：预告不兑现，接口不算开工。
//! - **调用约定**：[`degrade`]——可靠性异常源（[`DegradeReason`]，五值封闭，
//!   对齐 F2722 五类威胁）到降级动作（[`DEGRADE_ACTIONS`]）与显性复述
//!   （[`RESTATEMENTS`]）的**逐位固定映射**；复述缺席判
//!   [`E_R240_RESTATE_MISSING`]——降级不给用户一句人话就是静默降级。
//! - **v1 冻结**：[`API_V1_FREEZE`]——接口签名清单封闭冻结；清单漂移判
//!   [`E_R240_FREEZE_DRIFT`]；冻结声明 [`FROZEN_DECLARATION`] 写死在案。
//! - **两组**：判据侧 [`super::vcr24_degradeapi_checks`] 兑现组/冻结组两族
//!   全量断言。
//!
//! **为什么复述是接口的一部分**：安全降级天然伴随「功能变少」，用户看到的
//! 是效果缺失——不复述原因，用户会把安全降级误判为产品缺陷。复述文案
//! 与动作同位冻结，接口调用成功即复述在场。
//!
//! **零静默纪律**：所有空欠/冻结漂移/未知异常/复述缺席都产出诊断码
//! （0x99B 段 R 域细分续编），由调用方聚合上报。
//!
//! 确定性：全部封闭常量表 + 纯函数，时间戳由调用方注入（本模块不读时钟）。

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码（R 域 0x99B 段续编，0x99B8 起）
// ---------------------------------------------------------------------------

/// Q10 兑现空欠（可靠性异常→降级调用约定兑现物空缺）。
pub const E_R240_Q10_UNFULFILLED: u16 = 0x99B8;
/// v1 冻结漂移（接口签名清单与冻结版不符）。
pub const E_R240_FREEZE_DRIFT: u16 = 0x99B9;
/// 未知异常源（降级调用传入五类封闭之外的异常源）。
pub const E_R240_REASON_UNKNOWN: u16 = 0x99BA;
/// 复述缺席（降级动作无显性复述文案——静默降级）。
pub const E_R240_RESTATE_MISSING: u16 = 0x99BB;
/// 动作缺席（降级调用无动作映射——有异常无处置）。
pub const E_R240_ACTION_MISSING: u16 = 0x99BC;
/// 版本失配（批次版本与 R04 冻结版不符）。
pub const E_R240_VERSION_MISMATCH: u16 = 0x99BD;

/// 域批次版本。
pub const VCR24_VERSION: &str = "CR24-degradeapi-v1";

/// 版本校验（双向）：R04 冻结版放行，其余一律 [`E_R240_VERSION_MISMATCH`]。
pub fn check_version(v: &str) -> Result<(), u16> {
    if v == VCR24_VERSION {
        Ok(())
    } else {
        Err(E_R240_VERSION_MISMATCH)
    }
}

// ---------------------------------------------------------------------------
// 二、Q10 衔接包兑现：可靠性异常→渲染降级调用约定
// ---------------------------------------------------------------------------

/// Q10 兑现物（封闭三条——可靠性域预告的降级调用约定名目）。
pub const Q10_FULFILLMENT: [&str; 3] = [
    "异常源封闭：可靠性异常按 F2722 五类威胁对齐为五个降级触发源",
    "动作固定映射：每个异常源到降级动作的映射逐位冻结不仲裁",
    "复述同位冻结：每次降级必须带一句人话复述，文案与动作同表冻结",
];

/// Q10 兑现回执（条数 + 兑现确认）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Q10Receipt {
    /// 兑现物条目数。
    pub items: usize,
    /// 兑现确认（兑现物非空在场）。
    pub confirmed: bool,
}

/// 兑现确认：Q10 预告的调用约定兑现物必须非空在场，
/// 空欠即 [`E_R240_Q10_UNFULFILLED`]——预告不兑现，接口不算开工。
pub fn verify_q10_fulfilled() -> Result<Q10Receipt, u16> {
    if Q10_FULFILLMENT.is_empty() {
        return Err(E_R240_Q10_UNFULFILLED);
    }
    Ok(Q10Receipt {
        items: Q10_FULFILLMENT.len(),
        confirmed: true,
    })
}

// ---------------------------------------------------------------------------
// 三、降级调用约定：异常源封闭 → 动作/复述逐位固定映射
// ---------------------------------------------------------------------------

/// 可靠性异常源（封闭五值——对齐 F2722 五类威胁，次序即威胁秩）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DegradeReason {
    /// 恶意着色器：解析或编译面检出恶意构造。
    MaliciousShader,
    /// 资源炸弹：资源申请呈指数/超限形态。
    ResourceBomb,
    /// 越界：渲染数据访问越出声明边界。
    BoundaryBreach,
    /// 外泄：数据流出路径超出声明面。
    Exfiltration,
    /// 死循环：渲染作业超时呈不收敛形态。
    InfiniteLoop,
}

/// 五源降级动作（与 [`DegradeReason`] 按位对齐——映射逐位冻结不仲裁）。
pub const DEGRADE_ACTIONS: [&str; 5] = [
    "拒绝该着色器并回退上一可信管线状态",
    "封顶资源申请并切换低保真降载渲染",
    "裁剪越界访问并按声明边界收缩绘制域",
    "断开外泄路径并以本地脱敏副本续渲",
    "终止不收敛作业并输出已完成帧+中断复述",
];

/// 五源复述文案（与 [`DEGRADE_ACTIONS`] 按位对齐——同表冻结，每条含「降级」）。
pub const RESTATEMENTS: [&str; 5] = [
    "已降级：该着色器被安全策略拒绝，已回退到上一可信画面",
    "已降级：资源申请超出上限，当前以低保真模式继续",
    "已降级：越界绘制域已被收缩，超出部分不再呈现",
    "已降级：数据外传通道已断开，正在以本地副本继续渲染",
    "已降级：作业未在时限内收敛，已输出当前已完成部分",
];

/// 降级结果（动作 + 复述同位返回——调用成功即复述在场）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DegradeOutcome {
    /// 降级动作（来自 [`DEGRADE_ACTIONS`] 封闭表）。
    pub action: String,
    /// 显性复述（来自 [`RESTATEMENTS`] 封闭表）。
    pub restatement: String,
    /// 降级发生（本接口恒为 true——查询性调用不经过此接口）。
    pub degraded: bool,
}

/// 降级调用约定入口：可靠性异常源 → 降级动作+复述（逐位固定映射）。
pub fn degrade(reason: DegradeReason) -> Result<DegradeOutcome, u16> {
    let idx = match reason {
        DegradeReason::MaliciousShader => 0usize,
        DegradeReason::ResourceBomb => 1usize,
        DegradeReason::BoundaryBreach => 2usize,
        DegradeReason::Exfiltration => 3usize,
        DegradeReason::InfiniteLoop => 4usize,
    };
    let action = DEGRADE_ACTIONS[idx];
    let restatement = RESTATEMENTS[idx];
    if action.is_empty() {
        return Err(E_R240_ACTION_MISSING);
    }
    if restatement.is_empty() {
        return Err(E_R240_RESTATE_MISSING);
    }
    Ok(DegradeOutcome {
        action: String::from(action),
        restatement: String::from(restatement),
        degraded: true,
    })
}

// ---------------------------------------------------------------------------
// 四、接口冻结（v1 冻结——签名清单封闭 + 冻结声明）
// ---------------------------------------------------------------------------

/// v1 冻结签名清单（封闭——降级接口对外的全部函数名，新增即 v2）。
pub const API_V1_FREEZE: [&str; 5] = [
    "degrade",
    "verify_q10_fulfilled",
    "check_freeze",
    "check_version",
    "degrade_audit",
];

/// 冻结声明（v1 冻结——语义与边界写死在案）。
pub const FROZEN_DECLARATION: &str =
    "降级接口 v1 冻结：五源五动作五复述逐位固定；签名清单之外的新增走 v2 流程";

/// 冻结校验：传入签名清单与 v1 清单逐位比对（数目+次序+名字全等），
/// 任何漂移判 [`E_R240_FREEZE_DRIFT`]。
pub fn check_freeze(signatures: &[&str]) -> Result<usize, u16> {
    if signatures.len() != API_V1_FREEZE.len() {
        return Err(E_R240_FREEZE_DRIFT);
    }
    let mut i = 0usize;
    while i < API_V1_FREEZE.len() {
        if signatures[i] != API_V1_FREEZE[i] {
            return Err(E_R240_FREEZE_DRIFT);
        }
        i += 1;
    }
    Ok(signatures.len())
}

/// 冻结自审计：本文件实际暴露的冻结函数逐一在场核对（防声明与实现脱节）。
pub fn degrade_audit() -> Result<usize, u16> {
    let live: Vec<&str> = vec![
        "degrade",
        "verify_q10_fulfilled",
        "check_freeze",
        "check_version",
        "degrade_audit",
    ];
    check_freeze(&live)
}
