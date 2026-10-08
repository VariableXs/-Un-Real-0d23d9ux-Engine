//! VE-F5801 · AI 域总架构（VE-AI 域 · AC01 · 目标 340 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F5801`
//!
//! **判据（锚点原文）**：四层架构、AI 三律、可预期可解释可关闭、判据。
//!
//! **职责定位（锚点原文）**：四层架构（感知层、决策层、执行层、协作层）
//! 与三律（**可预期**：AI 行为对玩家透明一致、**可解释**：每个决策可回答
//! 为什么、**可关闭**：任何 AI 能力可降级可关闭），域色聪明得有分寸——
//! AI 惊艳玩家但不欺骗玩家；承接 AB 域移交包（F5793 十件）。
//!
//! # 一、四层架构是流向表不是口头约定
//!
//! 感知→决策→执行是主干下行；协作层横跨（接收执行反馈、向决策注入
//! 协作意图）。流向表**冻结**（[`legal_flow`]）：任何不在此表的层间调用
//! = 越权，显性拒绝（[`E_AI_TRESPASS`]）——「架构」只有能机检越权时
//! 才是架构，否则只是目录结构。
//!
//! # 二、三律是三条红线，逐条可机检
//!
//! - **可预期**：AI 行为必须在行为册登记后才可激活——未登记行为对玩家
//!   表现不一致（同输入不同输出且无声明），拒绝注册（[`LAW_VIOLATION`]）；
//! - **可解释**：每个决策必须带 reason，无理由的决策在裁决时显性违例
//!   ——「为什么」答不出来，玩家面对的就是黑箱；
//! - **可关闭**：每个 AI 能力带显性开关，关闭后调用返回显性拒绝码而
//!   不是静默空转——降级是状态不是失踪。
//!
//! 三律违例是**红线**（[`E_AI_LAW`]）：不降级、不静默、注册即拒。
//!
//! # 三、承接缺件→向 AB 追补
//!
//! F5793 十件移交清单**名字冻结**（判据对账），逐件核验；缺件不静默
//! 跳过——生成追补报告（缺哪件、向谁追补），追补完成前该件对应的
//! AI 能力不开放（承接完整性先于功能进度）。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、常量与错误码
// ---------------------------------------------------------------------------

/// 本项版本。
pub const AIARCH_PROTOCOL_VERSION: &str = "AC01-aiarch-v1";

/// 四层闭集长度。
pub const LAYER_COUNT: usize = 4;

/// 三律闭集长度。
pub const LAW_COUNT: usize = 3;

/// F5793 十件承接闭集长度。
pub const HANDOVER_COUNT: usize = 10;

/// 层间越权（流向表外调用）。
pub const E_AI_TRESPASS: &str = "E_AI_TRESPASS";

/// 三律违例（红线：不降级不静默）。
pub const E_AI_LAW: &str = "E_AI_LAW";

/// 承接缺件（向 AB 追补）。
pub const E_AI_HANDOVER: &str = "E_AI_HANDOVER";

// ---------------------------------------------------------------------------
// 二、四层架构（闭集 + 冻结流向表）
// ---------------------------------------------------------------------------

/// AI 四层。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AiLayer {
    /// 感知层：把世界转成 AI 可消费的信号。
    Perceive,
    /// 决策层：信号→意图（三律的主战场）。
    Decide,
    /// 执行层：意图→世界中的动作。
    Act,
    /// 协作层：多 Agent 协调与执行反馈回流。
    Coordinate,
}

impl AiLayer {
    /// 层名（冻结）。
    pub const fn name(self) -> &'static str {
        match self {
            AiLayer::Perceive => "perceive",
            AiLayer::Decide => "decide",
            AiLayer::Act => "act",
            AiLayer::Coordinate => "coordinate",
        }
    }

    /// 中文。
    pub const fn zh(self) -> &'static str {
        match self {
            AiLayer::Perceive => "感知层",
            AiLayer::Decide => "决策层",
            AiLayer::Act => "执行层",
            AiLayer::Coordinate => "协作层",
        }
    }

    /// 序号。
    pub const fn ordinal(self) -> usize {
        match self {
            AiLayer::Perceive => 0,
            AiLayer::Decide => 1,
            AiLayer::Act => 2,
            AiLayer::Coordinate => 3,
        }
    }

    /// 四层全集（顺序即序号）。
    pub fn all() -> [AiLayer; LAYER_COUNT] {
        [AiLayer::Perceive, AiLayer::Decide, AiLayer::Act, AiLayer::Coordinate]
    }
}

/// 合法层间调用表（冻结）：感知→决策、决策→执行、执行→协作（反馈）、
/// 协作→决策（协作意图注入）。表外皆越权。
pub fn legal_flow(from: AiLayer, to: AiLayer) -> bool {
    matches!(
        (from, to),
        (AiLayer::Perceive, AiLayer::Decide)
            | (AiLayer::Decide, AiLayer::Act)
            | (AiLayer::Act, AiLayer::Coordinate)
            | (AiLayer::Coordinate, AiLayer::Decide)
    )
}

/// 合法流向对账表（判据侧独立枚举全 16 组合重算——不共享上面的实现）。
pub const FLOW_TABLE_EXPECTED: [[bool; LAYER_COUNT]; LAYER_COUNT] = [
    // from=Perceive: 只到 Decide
    [false, true, false, false],
    // from=Decide: 只到 Act
    [false, false, true, false],
    // from=Act: 只到 Coordinate
    [false, false, false, true],
    // from=Coordinate: 只到 Decide
    [false, true, false, false],
];

/// 越权裁决：合法放行、越权返回显性错误码。
pub fn check_flow(from: AiLayer, to: AiLayer) -> Result<(), String> {
    if legal_flow(from, to) {
        Ok(())
    } else {
        Err(format!(
            "{}：{} → {} 不在冻结流向表",
            E_AI_TRESPASS,
            from.name(),
            to.name()
        ))
    }
}

// ---------------------------------------------------------------------------
// 三、AI 三律（可预期 / 可解释 / 可关闭）
// ---------------------------------------------------------------------------

/// AI 三律。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AiLaw {
    /// 可预期：AI 行为对玩家透明一致。
    Predictable,
    /// 可解释：每个决策可回答为什么。
    Explainable,
    /// 可关闭：任何 AI 能力可降级可关闭。
    Closable,
}

impl AiLaw {
    /// 律名（冻结）。
    pub const fn name(self) -> &'static str {
        match self {
            AiLaw::Predictable => "predictable",
            AiLaw::Explainable => "explainable",
            AiLaw::Closable => "closable",
        }
    }

    /// 中文。
    pub const fn zh(self) -> &'static str {
        match self {
            AiLaw::Predictable => "可预期",
            AiLaw::Explainable => "可解释",
            AiLaw::Closable => "可关闭",
        }
    }

    /// 三律全集。
    pub fn all() -> [AiLaw; LAW_COUNT] {
        [AiLaw::Predictable, AiLaw::Explainable, AiLaw::Closable]
    }
}

/// AI 行为登记项（可预期的载体：登记在册才可激活）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AiBehavior {
    /// 行为名。
    pub name: String,
    /// 所属层。
    pub layer: AiLayer,
    /// 一句话对玩家的声明（透明一致的可读面）。
    pub declaration: String,
}

/// AI 决策（可解释的载体：reason 必带）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AiDecision {
    /// 行为名（须已登记）。
    pub behavior: String,
    /// 决策理由（**必非空**——答不出的决策是黑箱）。
    pub reason: String,
}

/// AI 能力开关（可关闭的载体：关闭后显性拒）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AiCapability {
    /// 能力名。
    pub name: String,
    /// 开关。
    pub enabled: bool,
    /// 关闭时的降级声明（读屏可达）。
    pub degrade_note: String,
}

/// 三律裁决器：一次裁决覆盖三律，违例返回**具体违反哪条**（红线：注册/激活即拒）。
pub fn law_check(
    behavior: Option<&AiBehavior>,
    decision: Option<&AiDecision>,
    capability: Option<&AiCapability>,
) -> Result<(), String> {
    // 可预期：行为必须在册。
    match behavior {
        Some(_) => {}
        None => {
            return Err(format!(
                "{}：违反 {}——行为未登记，对玩家不透明一致",
                E_AI_LAW,
                AiLaw::Predictable.name()
            ))
        }
    }
    // 可解释：决策 reason 必非空白。
    match decision {
        Some(d) if !d.reason.trim().is_empty() => {}
        Some(_) => {
            return Err(format!(
                "{}：违反 {}——决策理由为空（黑箱决策）",
                E_AI_LAW,
                AiLaw::Explainable.name()
            ))
        }
        None => {
            return Err(format!(
                "{}：违反 {}——决策缺 reason 字段",
                E_AI_LAW,
                AiLaw::Explainable.name()
            ))
        }
    }
    // 可关闭：能力关闭时必须带降级声明（关得掉且关得明白）。
    match capability {
        Some(c) if !c.enabled => {
            if c.degrade_note.trim().is_empty() {
                return Err(format!(
                    "{}：违反 {}——能力关闭但无降级声明（失踪而非关闭）",
                    E_AI_LAW,
                    AiLaw::Closable.name()
                ));
            }
        }
        Some(_) => {}
        None => {
            return Err(format!(
                "{}：违反 {}——能力缺开关（关不掉）",
                E_AI_LAW,
                AiLaw::Closable.name()
            ))
        }
    }
    Ok(())
}

/// 三律声明单行（读屏可达——不含内部状态细节）。
pub fn laws_screen_line() -> String {
    format!(
        "AI 三律：{} / {} / {}——AI 惊艳玩家但不欺骗玩家",
        AiLaw::Predictable.zh(),
        AiLaw::Explainable.zh(),
        AiLaw::Closable.zh()
    )
}

// ---------------------------------------------------------------------------
// 四、F5793 十件承接清单（名字冻结 + 缺件追补）
// ---------------------------------------------------------------------------

/// F5793 十件名字（冻结——判据逐字对账，顺序即编号）。
pub const HANDOVER_ITEMS: [&str; HANDOVER_COUNT] = [
    "感知消费音频事件接口",
    "听者数据供AI定位",
    "语音字幕供AI对话",
    "预算口径",
    "遥测规范",
    "沙箱三律参照",
    "文档总纲",
    "测试资产",
    "fuzz语料",
    "知识库",
];

/// 承接件状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HandoverState {
    /// 已核验通过。
    Verified,
    /// 待核验。
    Pending,
}

/// 承接清单（十件逐件核验）。
#[derive(Clone, Debug)]
pub struct HandoverLedger {
    /// 逐件状态（下标 = [`HANDOVER_ITEMS`] 下标）。
    pub states: [HandoverState; HANDOVER_COUNT],
}

impl HandoverLedger {
    /// 新清单（全部待核验）。
    pub fn new() -> HandoverLedger {
        HandoverLedger { states: [HandoverState::Pending; HANDOVER_COUNT] }
    }

    /// 核验一件（下标越界返回 Err——不静默钳制）。
    pub fn verify(&mut self, ordinal: usize) -> Result<(), String> {
        match self.states.get_mut(ordinal) {
            Some(st) => {
                *st = HandoverState::Verified;
                Ok(())
            }
            None => Err(format!("{}：件号 {} 越界", E_AI_HANDOVER, ordinal)),
        }
    }

    /// 已核验件数。
    pub fn verified_count(&self) -> usize {
        self.states.iter().filter(|s| **s == HandoverState::Verified).count()
    }

    /// 缺件追补报告（缺哪件、序号多少——空 = 十件齐）。
    pub fn missing_report(&self) -> Vec<(usize, &'static str)> {
        let mut out: Vec<(usize, &'static str)> = Vec::new();
        for (i, st) in self.states.iter().enumerate() {
            if *st == HandoverState::Pending {
                if let Some(name) = HANDOVER_ITEMS.get(i) {
                    out.push((i, name));
                }
            }
        }
        out
    }
}

/// 版本指纹（FNV-1a 运行期重算对账——防手抄漂移）。
pub const fn version_fingerprint() -> u64 {
    let bytes = AIARCH_PROTOCOL_VERSION.as_bytes();
    let mut h: u64 = 0xcbf29ce484222325;
    let mut i = 0;
    while i < bytes.len() {
        h ^= bytes[i] as u64;
        h = h.wrapping_mul(0x100000001b3);
        i += 1;
    }
    h
}
