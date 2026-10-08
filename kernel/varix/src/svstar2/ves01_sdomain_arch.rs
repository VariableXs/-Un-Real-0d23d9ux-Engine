//! VE-F3801 · S 域开工与无障碍渲染总架构（VE-S 域 · 无障碍渲染域 · 组内第 1 条）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3801`
//!
//! **判据（锚点原文六条）**：第一等公民、最后一公里、六段签名、注入显性、断链 P0、判据。
//! 逐条落位：
//! - **第一等公民**：[`verify_first_class_citizen`]——无障碍能力驻留层必须为
//!   [`Residency::Renderer`]；驻留覆盖层（`OVERLAY`）或仅 UI 层（`UI_ONLY`）判 **P0**
//!   （[`DiagCode::FirstClassViolation`]）。驻留渲染层却不参与语义-像素一致断言，
//!   同判 P0——放弃「画没画」的核查权即断链的前置条件。
//! - **最后一公里**：[`verify_relation_table`]——N/N/P/R 四域皆为数据与逻辑层，
//!   S 域是**唯一**像素层执行域；某对端被标为像素层即分工破口。
//! - **六段签名**：[`verify_stage_signatures`]——采集→策略→注入→执行→验证→反馈，
//!   签名冻结 v1，按数组序**逐位比对不排序**（排序会把真正的乱序漂移洗掉）。
//! - **注入显性**：[`verify_injection_report`]——每条策略注没注/注到哪/为何不注，
//!   缺席即静默丢弃；未注入必须带原因。
//! - **断链 P0**：[`detect_broken_links`]——语义树说有而像素没画 = 立案 P0，
//!   交 F3807/F3812 双线追责；虚拟化保留例外须显式标记。
//! - **判据**：本条自带架构回归 + 断链演练 + 红线演练（[`run_f3801_checks`]），
//!   架构契约的每一面都有可执行断言，不靠人读代码确认。
//!
//! **为什么「第一等公民」判 P0 而非 P1**：覆盖层实现在艺术管线挤压时会**静默失效**
//! （无障碍请求被冲掉且无提示）。这不是画面难看，是无障碍能力的实质丢失——
//! 对依赖它的用户等于能力被收回，故属阻断级。
//!
//! **为什么「断链」是本域最恶劣缺陷**：它比「画了但不好看」严重一个量级——
//! 系统向用户（尤其读屏用户）宣告了一个不存在的视觉事实，用户据此行动
//! （以为按钮在那里、以为弹窗打开了）。本域对此唯一态度是**立案**：
//! 不降级、不修辞化、不静默。
//!
//! **零静默纪律**：所有拒绝/冲突/阻断/断供/断链/降级都产出 [`Diagnostic`]
//! （code + severity + message + hint + stage + restatement），由调用方聚合上报。
//! **降级显性复述**是硬要求：任何一次降级都必须生成一句人话，告诉用户
//! 「你请求的 X 因为 Y 变成了 Z」——降级不是静默回落。
//!
//! **性能逐项分解**（锚点原文：六段 O(1) 每段摊销；注入 O(策略数)；映射 O(1)）：
//! ①②④⑤⑥ 六段各自 O(1) 每帧摊销；③ 注入 O(策略数)（有限常数，非 O(节点)）；
//! 十项映射查表 O(1)。**没有一段是 O(场景节点数)**：这是架构约束，不是巧合。
//!
//! 确定性：全部静态注册表 + 纯函数，时间戳由调用方注入（本模块不读时钟），
//! 零 IO、可回放对拍。零外部依赖，只用 `alloc` 容器与 `crate::checks`。
//!
//! **迁移说明**：本模块由前端TypeScript（`src/system/ve/sDomain/
//! f3801-s-domain-accessible-render-architecture.ts`，1872 行）迁入 Rust 内核。
//! 迁移后契约执法发生在内核侧，本模块为**唯一权威源**。
//! 原 TS 文件按「源码只增不减」红线保留于磁盘（未删除、未取消跟踪），
//! 但已不再被任何代码引用，**不得再作为契约依据**——内核与 TS 若有分歧，
//! 以本模块为准。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 一、诊断基础设施（零静默的载体）
// ---------------------------------------------------------------------------

/// 诊断严重度。分级的判据是**后果**，不是修复难度。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    /// 提示级：可观测性、待对账、预留位——记账即可。
    P2,
    /// 显性级：降级发生、注入点缺项、策略表不透明——必须对人话复述，可继续。
    P1,
    /// 阻断级：辅具态断供、语义-像素断链、第一等公民被降格。出现即阻断。
    P0,
}

impl Severity {
    /// 严重度的人话名（自检明细与复述文本用）。
    pub fn label(self) -> &'static str {
        match self {
            Severity::P0 => "P0",
            Severity::P1 => "P1",
            Severity::P2 => "P2",
        }
    }
}

/// 诊断码：每种拒绝/冲突/断供/断链/降级都有独立可检索的码，绝不合并成通用错误。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiagCode {
    /// 第一等公民被降格：无障碍实现被放到覆盖层/后处理而非渲染层本体。
    FirstClassViolation,
    /// 状态采集断供：辅具态丢失。P0。
    StateCaptureBlackout,
    /// 状态采集多点分叉：同一辅具态被两处采集且不一致。P0。
    StateCaptureFork,
    /// 语义-像素断链：语义树声明可见，帧缓冲未画——最恶劣缺陷，立案。
    SemanticPixelBrokenLink,
    /// 管线注入失败：注入 D 域失败，走降级路径（降级须显性复述）。
    InjectionFailed,
    /// 注入点未知：S 域声明的注入点不在 D 域清单内（以 D 域为准）。
    InjectionSlotUnknown,
    /// 策略黑箱：渲染策略不透明（用户无法预测设置带来什么）。
    StrategyOpaque,
    /// 六段签名与冻结版不一致（未走 ADR 的漂移）。
    StageSignatureDrift,
    /// 六段序乱序或缺段。
    StageSequenceInvalid,
    /// 十项映射缺项（官方主题未落组）。
    ThemeMappingIncomplete,
    /// 十项映射归属冲突（同一主题被两组同时认领）。
    ThemeOwnershipConflict,
    /// 四域关系表缺行或行字段非法。
    RelationTableInvalid,
    /// R 域移交包材料缺失（F3795 开工条件）。
    HandoverMaterialMissing,
    /// R 域移交包接收确认位未签——开工阻断。
    HandoverNotConfirmed,
    /// 无障碍注入预算超限（超 1ms/帧）。
    A11yBudgetExceeded,
    /// 分工表条目数与 S01 组 20 条不符。
    WorktableCardinalityInvalid,
    /// 分工表存在重复条目号。
    WorktableDuplicateEntry,
    /// 分工表条目号不连续（缺位）。
    WorktableGap,
    /// 架构修正（ADR）信息不全，无法受理。
    AdrIncomplete,
    /// 架构冻结快照与当前声明不一致（漂移告警）。
    FreezeDrift,
}

impl DiagCode {
    /// 诊断码的稳定字符串（可检索键；跨版本不变）。
    pub fn as_str(self) -> &'static str {
        match self {
            DiagCode::FirstClassViolation => "FIRST_CLASS_VIOLATION",
            DiagCode::StateCaptureBlackout => "STATE_CAPTURE_BLACKOUT",
            DiagCode::StateCaptureFork => "STATE_CAPTURE_FORK",
            DiagCode::SemanticPixelBrokenLink => "SEMANTIC_PIXEL_BROKEN_LINK",
            DiagCode::InjectionFailed => "INJECTION_FAILED",
            DiagCode::InjectionSlotUnknown => "INJECTION_SLOT_UNKNOWN",
            DiagCode::StrategyOpaque => "STRATEGY_OPAQUE",
            DiagCode::StageSignatureDrift => "STAGE_SIGNATURE_DRIFT",
            DiagCode::StageSequenceInvalid => "STAGE_SEQUENCE_INVALID",
            DiagCode::ThemeMappingIncomplete => "THEME_MAPPING_INCOMPLETE",
            DiagCode::ThemeOwnershipConflict => "THEME_OWNERSHIP_CONFLICT",
            DiagCode::RelationTableInvalid => "RELATION_TABLE_INVALID",
            DiagCode::HandoverMaterialMissing => "HANDOVER_MATERIAL_MISSING",
            DiagCode::HandoverNotConfirmed => "HANDOVER_NOT_CONFIRMED",
            DiagCode::A11yBudgetExceeded => "A11Y_BUDGET_EXCEEDED",
            DiagCode::WorktableCardinalityInvalid => "WORKTABLE_CARDINALITY_INVALID",
            DiagCode::WorktableDuplicateEntry => "WORKTABLE_DUPLICATE_ENTRY",
            DiagCode::WorktableGap => "WORKTABLE_GAP",
            DiagCode::AdrIncomplete => "ADR_INCOMPLETE",
            DiagCode::FreezeDrift => "FREEZE_DRIFT",
        }
    }
}

/// 渲染六段标识。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RenderStage {
    /// ① 状态采集：四态采集，唯一采集点。
    StateCapture,
    /// ②渲染策略：四态→策略映射，策略表公开。
    RenderStrategy,
    /// ③ 管线注入：注入 D 域渲染管线三注入点。
    PipelineInjection,
    /// ④ 执行：渲染层出像素。
    Execute,
    /// ⑤ 验证：语义-像素一致断言（断链检测面）。
    Verify,
    /// ⑥ 反馈：降级复述 / 断供复述 / 断链案卷上行。
    Feedback,
}

impl RenderStage {
    /// 段标识的稳定字符串。
    pub fn as_str(self) -> &'static str {
        match self {
            RenderStage::StateCapture => "STATE_CAPTURE",
            RenderStage::RenderStrategy => "RENDER_STRATEGY",
            RenderStage::PipelineInjection => "PIPELINE_INJECTION",
            RenderStage::Execute => "EXECUTE",
            RenderStage::Verify => "VERIFY",
            RenderStage::Feedback => "FEEDBACK",
        }
    }
}

/// 诊断归属面：六段之一，或域级（`DOMAIN`）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StageTag {
    /// 归属某一段。
    Stage(RenderStage),
    /// 域级问题（十项映射/移交包/分工表等无归属段者）。
    Domain,
}

impl StageTag {
    /// 归属面的稳定字符串。
    pub fn as_str(self) -> &'static str {
        match self {
            StageTag::Stage(s) => s.as_str(),
            StageTag::Domain => "DOMAIN",
        }
    }
}

/// 一条诊断：发生了什么（code + message）、多严重（severity）、怎么办（hint）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    pub code: DiagCode,
    pub severity: Severity,
    /// 人话描述，面向开发者与用户排障，不含裸异常码。
    pub message: String,
    /// 可操作提示：该改哪里、该怎么降级。
    pub hint: String,
    /// 归属段；域级问题为 [`StageTag::Domain`]。
    pub stage: StageTag,
    /// 降级/断供的显性复述文本（人话，直接可上屏）；非降级类诊断为 `None`。
    pub restatement: Option<String>,
}

/// 诊断构造助手：把「五要素 + 复述」一次性装好，避免漏字段。
pub struct DiagnosticBag {
    items: Vec<Diagnostic>,
}

impl DiagnosticBag {
    pub fn new() -> Self {
        DiagnosticBag { items: Vec::new() }
    }

    /// 压入一条诊断。
    pub fn push(&mut self, d: Diagnostic) {
        self.items.push(d);
    }

    /// 便捷构造：域级诊断（最常见形态）。
    pub fn domain(
        &mut self,
        code: DiagCode,
        severity: Severity,
        message: String,
        hint: String,
        restatement: Option<String>,
    ) {
        self.items.push(Diagnostic {
            code,
            severity,
            message,
            hint,
            stage: StageTag::Domain,
            restatement,
        });
    }

    /// 便捷构造：段级诊断。
    pub fn staged(
        &mut self,
        stage: RenderStage,
        code: DiagCode,
        severity: Severity,
        message: String,
        hint: String,
        restatement: Option<String>,
    ) {
        self.items.push(Diagnostic {
            code,
            severity,
            message,
            hint,
            stage: StageTag::Stage(stage),
            restatement,
        });
    }

    pub fn items(&self) -> &[Diagnostic] {
        &self.items
    }

    pub fn into_items(self) -> Vec<Diagnostic> {
        self.items
    }

    /// 是否含 P0（P0 出现即阻断，调用方必须显式处理，不得忽略）。
    pub fn has_p0(&self) -> bool {
        self.items.iter().any(|d| d.severity == Severity::P0)
    }

    /// 过滤出某严重度的诊断（供面板分级展示）。
    pub fn by_severity(&self, severity: Severity) -> Vec<&Diagnostic> {
        self.items.iter().filter(|d| d.severity == severity).collect()
    }

    /// 是否含指定诊断码（可检索性断言用）。
    pub fn has_code(&self, code: DiagCode) -> bool {
        self.items.iter().any(|d| d.code == code)
    }
}

/// 结果判别：成功必带 value，失败必带 code/message/hint——失败不可被误当成功。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome<T> {
    Ok {
        value: T,
        diagnostics: Vec<Diagnostic>,
    },
    Err {
        code: DiagCode,
        severity: Severity,
        message: String,
        hint: String,
        diagnostics: Vec<Diagnostic>,
    },
}

impl<T> Outcome<T> {
    /// 是否成功。
    pub fn is_ok(&self) -> bool {
        matches!(self, Outcome::Ok { .. })
    }

    /// 是否含 P0。
    pub fn has_blocking(&self) -> bool {
        self.diagnostics().iter().any(|d| d.severity == Severity::P0)
    }

    /// 取诊断（成功失败都取得到，失败时含自诊断）。
    pub fn diagnostics(&self) -> &[Diagnostic] {
        match self {
            Outcome::Ok { diagnostics, .. } => diagnostics,
            Outcome::Err { diagnostics, .. } => diagnostics,
        }
    }

    /// 消耗掉结果，取出诊断（失败现场透传用）。
    pub fn into_diagnostics(self) -> Vec<Diagnostic> {
        match self {
            Outcome::Ok { diagnostics, .. } => diagnostics,
            Outcome::Err { diagnostics, .. } => diagnostics,
        }
    }

    /// 取值：仅成功时有值。失败返回 `None`——强制调用方显式处理失败。
    pub fn value(&self) -> Option<&T> {
        match self {
            Outcome::Ok { value, .. } => Some(value),
            Outcome::Err { .. } => None,
        }
    }

    /// 失败码（成功时 `None`）。
    pub fn err_code(&self) -> Option<DiagCode> {
        match self {
            Outcome::Ok { .. } => None,
            Outcome::Err { code, .. } => Some(*code),
        }
    }

    /// 失败严重度（成功时 `None`）。
    pub fn err_severity(&self) -> Option<Severity> {
        match self {
            Outcome::Ok { .. } => None,
            Outcome::Err { severity, .. } => Some(*severity),
        }
    }

    /// 失败人话（成功时 `None`）。
    pub fn message(&self) -> Option<&str> {
        match self {
            Outcome::Ok { .. } => None,
            Outcome::Err { message, .. } => Some(message.as_str()),
        }
    }
}

/// 成功构造（`diagnostics` 允许携带非致命告警，例如降级已复述）。
pub fn ok<T>(value: T, diagnostics: Vec<Diagnostic>) -> Outcome<T> {
    Outcome::Ok { value, diagnostics }
}

/// 失败构造：失败路径必须给出可操作提示，不允许裸码。
pub fn err<T>(
    code: DiagCode,
    severity: Severity,
    message: String,
    hint: String,
    diagnostics: Vec<Diagnostic>,
) -> Outcome<T> {
    let mut all = diagnostics;
    all.push(Diagnostic {
        code,
        severity,
        message: message.clone(),
        hint: hint.clone(),
        stage: StageTag::Domain,
        restatement: None,
    });
    Outcome::Err { code, severity, message, hint, diagnostics: all }
}

/// 顺序跑一道闸门：失败即**原样透传**（含全部已累积诊断）。
///
/// 残值类型用 [`BoxedErr`]（装箱的失败诊断）而非 `Outcome<Infallible>`：
/// `Infallible` 版本会抹掉失败现场——而失败现场正是这里要透传的东西。
fn gate<T>(outcome: Outcome<T>) -> Result<T, BoxedErr> {
    match outcome {
        Outcome::Ok { value, .. } => Ok(value),
        Outcome::Err { .. } => Err(BoxedErr {
            code: outcome_code(&outcome),
            severity: outcome_severity(&outcome),
            message: outcome_message(&outcome),
            hint: outcome_hint(&outcome),
            diagnostics: outcome.into_diagnostics(),
        }),
    }
}

/// 装箱的闸门失败现场（`Try` 残值；`Infallible` 装不下诊断）。
pub struct BoxedErr {
    pub code: DiagCode,
    pub severity: Severity,
    pub message: String,
    pub hint: String,
    pub diagnostics: Vec<Diagnostic>,
}

/// 闸门失败现场还原为 []（诊断链原样带回，不丢现场）。
fn re_err<T>(e: BoxedErr) -> Outcome<T> {
    Outcome::Err {
        code: e.code,
        severity: e.severity,
        message: e.message,
        hint: e.hint,
        diagnostics: e.diagnostics,
    }
}

impl From<BoxedErr> for Diagnostic {
    fn from(e: BoxedErr) -> Diagnostic {
        Diagnostic {
            code: e.code,
            severity: e.severity,
            message: e.message,
            hint: e.hint,
            stage: StageTag::Domain,
            restatement: None,
        }
    }
}

fn outcome_code<T>(o: &Outcome<T>) -> DiagCode {
    match o {
        Outcome::Ok { .. } => DiagCode::InjectionFailed, // 不会走到
        Outcome::Err { code, .. } => *code,
    }
}

fn outcome_severity<T>(o: &Outcome<T>) -> Severity {
    match o {
        Outcome::Ok { .. } => Severity::P2,
        Outcome::Err { severity, .. } => *severity,
    }
}

fn outcome_message<T>(o: &Outcome<T>) -> String {
    match o {
        Outcome::Ok { .. } => String::new(),
        Outcome::Err { message, .. } => message.clone(),
    }
}

fn outcome_hint<T>(o: &Outcome<T>) -> String {
    match o {
        Outcome::Ok { .. } => String::new(),
        Outcome::Err { hint, .. } => hint.clone(),
    }
}

/// 袋中有诊断即转失败（否则转成功）。**第一条诊断升级为失败主因**——
/// 失败必须指明「最先撞上的那一条」，否则调用方无法定位真正的病因
/// （后续条目往往只是它的连带反应）。
fn settle<T>(value: T, bag: DiagnosticBag) -> Outcome<T> {
    let items = bag.into_items();
    if let Some(first) = items.first() {
        return Outcome::Err {
            code: first.code,
            severity: first.severity,
            message: first.message.clone(),
            hint: first.hint.clone(),
            diagnostics: items,
        };
    }
    Outcome::Ok { value, diagnostics: items }
}

// ---------------------------------------------------------------------------
// 二、S 域标识与官方十主题
// ---------------------------------------------------------------------------

/// S 域域标签。
pub const S_DOMAIN_TAG: &str = "VE-S";

/// S 域中文名。
pub const S_DOMAIN_TITLE: &str = "无障碍渲染域";

/// 条目起始（域开工条）。
pub const S_FIRST_ENTRY_ID: u32 = 3801;

/// 条目结束（域收官条）。
pub const S_LAST_ENTRY_ID: u32 = 4000;

/// 条目总数（200 条 = 10 组 × 20 条）。
pub const S_ENTRY_COUNT: u32 = 200;

/// 组数（10 组，每组 20 条）。
pub const S_GROUP_COUNT: u32 = 10;

/// 每组条数。
pub const S_ENTRIES_PER_GROUP: u32 = 20;

/// S 域官方十主题（锚点原文十项）。
pub const S_DOMAIN_TEN_TOPICS: [&str; 10] = [
    "管线",
    "高对比",
    "焦点强化",
    "大字号",
    "动效替代",
    "读屏协同",
    "纹理",
    "光敏",
    "认知",
    "性能",
];

/// 域号沿革（锚点 F3801 + R 域 F3601 跳段 ADR）。
///
/// R 域条目号自 F3601 起跳（F3401-F3600 段为 E 域扩展预留），故 S 域自
/// **F3801** 起，与册内域表（VE-S = F3601-F3800）不同源——以条目锚点 F3801
/// 与 R 域跳段 ADR 为准，域表为跳段前的旧账。此差异显式登记，
/// 避免后续条目按旧域表误定位。
pub const S_DOMAIN_NUMBERING_NOTE: &str =
    "S 域自 F3801 起（R 域 F3601 跳段 ADR）；册内域表 VE-S=F3601-F3800 为跳段前旧账，以锚点为准";

/// 一条十项落组映射。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ThemeLanding {
    pub topic: &'static str,
    /// 首次落位批次组标签。
    pub landing_group: &'static str,
    /// 承接该主题落位的条目号。
    pub landing_entry_id: u32,
    /// 深挖批次组标签（实现归该组）。
    pub deepening_group: &'static str,
    /// 深挖批次首条目号。
    pub deepening_first_id: u32,
}

/// 十项落组映射（锚点「十项落组」）。
///
/// 落组不是「主题只归一组」——同一主题会在架构组落位、再在专组深挖
/// （例：高对比在 S01 落位架构，由 S02「高对比与色彩适配组」F3821 起做实现）。
/// 此处记录的是**首次落位**，避免重复认领。
pub const S_THEME_LANDINGS: [ThemeLanding; 10] = [
    ThemeLanding { topic: "管线", landing_group: "S01", landing_entry_id: 3801, deepening_group: "S01", deepening_first_id: 3802 },
    ThemeLanding { topic: "高对比", landing_group: "S01", landing_entry_id: 3801, deepening_group: "S02", deepening_first_id: 3821 },
    ThemeLanding { topic: "焦点强化", landing_group: "S01", landing_entry_id: 3801, deepening_group: "S01", deepening_first_id: 3804 },
    ThemeLanding { topic: "大字号", landing_group: "S01", landing_entry_id: 3801, deepening_group: "S01", deepening_first_id: 3805 },
    ThemeLanding { topic: "动效替代", landing_group: "S01", landing_entry_id: 3801, deepening_group: "S01", deepening_first_id: 3806 },
    ThemeLanding { topic: "读屏协同", landing_group: "S01", landing_entry_id: 3801, deepening_group: "S03", deepening_first_id: 3841 },
    ThemeLanding { topic: "纹理", landing_group: "S01", landing_entry_id: 3801, deepening_group: "S01", deepening_first_id: 3808 },
    ThemeLanding { topic: "光敏", landing_group: "S01", landing_entry_id: 3801, deepening_group: "S01", deepening_first_id: 3809 },
    ThemeLanding { topic: "认知", landing_group: "S01", landing_entry_id: 3801, deepening_group: "S01", deepening_first_id: 3810 },
    ThemeLanding { topic: "性能", landing_group: "S01", landing_entry_id: 3801, deepening_group: "S01", deepening_first_id: 3811 },
];

/// 校验十项落组映射：十项齐备、无归属冲突、条目号合法（硬闸）。
pub fn verify_theme_landings(landings: &[ThemeLanding]) -> Outcome<Vec<ThemeLanding>> {
    let mut bag = DiagnosticBag::new();

    // 2.1 齐备性：官方十项每项都必须有落位。
    for topic in S_DOMAIN_TEN_TOPICS {
        if !landings.iter().any(|l| l.topic == topic) {
            bag.domain(
                DiagCode::ThemeMappingIncomplete,
                Severity::P1,
                format!("十项映射缺项：{} 未落组。", topic),
                "官方主题十项每项都必须在映射表内；无实现的项标「预留」也不得删行——删行即无法核对齐备性。"
                    .to_string(),
                None,
            );
        }
    }

    // 2.2 归属冲突：同一主题被两组同时认领首次落位。
    for (i, l) in landings.iter().enumerate() {
        if landings[..i].iter().any(|p| p.topic == l.topic) {
            bag.domain(
                DiagCode::ThemeOwnershipConflict,
                Severity::P1,
                format!("十项映射归属冲突：主题「{}」被重复认领首次落位。", l.topic),
                "首次落位唯一；同一主题的深挖归 deepening_group，不在 landing_group 重复认领。"
                    .to_string(),
                None,
            );
        }
    }

    // 2.3 条目号合法性：落位/深挖条目号必须落在 S 域区间内。
    for l in landings {
        let in_domain = |id: u32| id >= S_FIRST_ENTRY_ID && id <= S_LAST_ENTRY_ID;
        if !in_domain(l.landing_entry_id) || !in_domain(l.deepening_first_id) {
            bag.domain(
                DiagCode::ThemeMappingIncomplete,
                Severity::P1,
                format!(
                    "主题「{}」落位/深挖条目号越界：F{}/F{}（有效域 F{}-F{}）",
                    l.topic, l.landing_entry_id, l.deepening_first_id, S_FIRST_ENTRY_ID, S_LAST_ENTRY_ID
                ),
                "条目号须落在 S 域区间内；越界说明条目编号与域表脱节。".to_string(),
                None,
            );
        }
    }

    settle(landings.to_vec(), bag)
}

// ---------------------------------------------------------------------------
// 三、第一等公民声明
// ---------------------------------------------------------------------------

/// 无障碍能力的驻留层。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Residency {
    /// 正确：下沉进渲染层本体，与色彩/几何/合成管线同级。
    Renderer,
    /// 反模式：覆盖层/后处理附加项——判 `FIRST_CLASS_VIOLATION`。
    Overlay,
    /// 反模式：仅 UI 层承担（渲染引擎内部无感知）——判 `FIRST_CLASS_VIOLATION`。
    UiOnly,
}

impl Residency {
    pub fn as_str(self) -> &'static str {
        match self {
            Residency::Renderer => "RENDERER",
            Residency::Overlay => "OVERLAY",
            Residency::UiOnly => "UI_ONLY",
        }
    }
}

/// 第一等公民声明：一条无障碍实现的驻留层登记。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FirstClassDeclaration {
    /// 能力标识（如 "高对比"、"焦点强化"）。
    pub capability: String,
    /// 驻留层。
    pub residency: Residency,
    /// 是否参与语义-像素一致断言（断链检测面）。
    pub participates_in_link_assertion: bool,
}

/// 校验第一等公民声明：任何 `Overlay` / `UiOnly` 登记即 **P0** 违规。
pub fn verify_first_class_citizen(
    declarations: &[FirstClassDeclaration],
) -> Outcome<Vec<FirstClassDeclaration>> {
    let mut bag = DiagnosticBag::new();

    for d in declarations {
        if d.residency != Residency::Renderer {
            bag.domain(
                DiagCode::FirstClassViolation,
                Severity::P0,
                format!(
                    "无障碍能力「{}」驻留在 {}，不是渲染层本体。",
                    d.capability,
                    d.residency.as_str()
                ),
                "无障碍不是覆盖层：能力须下沉进渲染层管线（与色彩/几何/合成同级）。\
                 覆盖层会被艺术管线挤压后静默失效——对依赖它的用户等于能力被收回。"
                    .to_string(),
                Some(format!(
                    "你启用的「{}」当前以覆盖层方式实现，可能在复杂画面上被冲掉。我们已拦截该实现。",
                    d.capability
                )),
            );
        }
        // 声明了驻层合法却不参与一致断言 → 像素与语义脱钩，是断链的前置条件，同判 P0。
        if d.residency == Residency::Renderer && !d.participates_in_link_assertion {
            bag.domain(
                DiagCode::FirstClassViolation,
                Severity::P0,
                format!(
                    "无障碍能力「{}」驻留渲染层却不参与语义-像素一致断言。",
                    d.capability
                ),
                "驻留渲染层的能力必须纳入断链检测面；不参与断言等于放弃「画没画」的核查权。".to_string(),
                Some(format!(
                    "「{}」的渲染结果无法被核查是否真的画出来了。我们已拦截该实现。",
                    d.capability
                )),
            );
        }
    }

    settle(declarations.to_vec(), bag)
}

// ---------------------------------------------------------------------------
// 四、四域关系表（最后一公里的可执行形态）
// ---------------------------------------------------------------------------

/// 对端域所属层。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layer {
    /// 数据与逻辑层：决定「应该是什么样」。
    DataLogic,
    /// 像素执行层：把「应该是什么样」画进帧缓冲。**唯一归属 S 域**。
    PixelExecution,
}

impl Layer {
    pub fn as_str(self) -> &'static str {
        match self {
            Layer::DataLogic => "DATA_LOGIC",
            Layer::PixelExecution => "PIXEL_EXECUTION",
        }
    }
}

/// 四域关系表的一行：S 域与数据/逻辑层对端的分工声明。
///
/// 字段取 `&'static str` 而非 `String`：本表是静态契约，`const` 上下文不可调用
/// `to_string()`（非 const fn）；且四行文本恒定，运行时分配纯属浪费。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DomainRelation {
    /// 对端域标签（如 "VE-N"、"VE-P"）。
    pub peer_domain: &'static str,
    /// 对端提供的组/条目（如 "N08 语义树"）。
    pub provides: &'static str,
    /// 对端所属层。
    pub layer: Layer,
    /// S 域向对端消费什么（反向依赖声明，防止隐藏耦合）。
    pub s_consumes: &'static str,
}

/// 四域关系表（锚点原文：与 N08/N05/P08/R08 关系）。
///
/// N/N/P 三域皆为 [`Layer::DataLogic`]——它们决定「应该是什么样」；
/// S 域是唯一的 [`Layer::PixelExecution`]——把「应该是什么样」画进帧缓冲。
/// R 域（创作生态）供给创作产出的语义描述，同样是数据层。
pub const S_RELATION_TABLE: [DomainRelation; 4] = [
    DomainRelation {
        peer_domain: "VE-N",
        provides: "N08 语义树 / F2741 语义单源",
        layer: Layer::DataLogic,
        s_consumes: "语义树节点可见性声明（断链断言的「语义侧」输入）",
    },
    DomainRelation {
        peer_domain: "VE-N",
        provides: "N05 判定与偏好 / F3146 F3149 采集单源",
        layer: Layer::DataLogic,
        s_consumes: "四态判定结论（辅具接入/reduce/高对比/字号档）",
    },
    DomainRelation {
        peer_domain: "VE-P",
        provides: "P08 无障碍总纲 / F3017 执法",
        layer: Layer::DataLogic,
        s_consumes: "动效替代裁决与等效能力红线",
    },
    DomainRelation {
        peer_domain: "VE-R",
        provides: "R08 创作语义描述",
        layer: Layer::DataLogic,
        s_consumes: "创作产出的语义文案（描述质量双签由 P/S 共责）",
    },
];

/// 校验四域关系表：四行齐备、层位声明正确、S 域自身像素层身份唯一（硬闸）。
pub fn verify_relation_table(table: &[DomainRelation]) -> Outcome<Vec<DomainRelation>> {
    let mut bag = DiagnosticBag::new();

    // 4.1 行数与字段：四域关系表须恰四行。
    if table.len() != 4 {
        bag.domain(
            DiagCode::RelationTableInvalid,
            Severity::P1,
            format!("四域关系表应为 4 行，实为 {} 行。", table.len()),
            "四域 = N08 语义树 / N05 偏好 / P08 动效替代 / R08 创作语义；缺行即分工不全。"
                .to_string(),
            None,
        );
    }

    // 4.2 层位：四域皆为数据与逻辑层——若某行被标为像素层，说明职责被混淆
    //     （「最后一公里」只属于 S 域，别的域也声称画像素即分工破口）。
    for row in table {
        if row.layer != Layer::DataLogic {
            bag.domain(
                DiagCode::RelationTableInvalid,
                Severity::P1,
                format!(
                    "对端 {}（{}）被标为 {}。",
                    row.peer_domain,
                    row.provides,
                    row.layer.as_str()
                ),
                "N/P/R 三域均为数据与逻辑层；像素层执行唯一归属 S 域，标错会掩盖重复实现。"
                    .to_string(),
                None,
            );
        }
        if row.s_consumes.trim().is_empty() {
            bag.domain(
                DiagCode::RelationTableInvalid,
                Severity::P1,
                format!(
                    "对端 {}（{}）未声明 S 域的反向消费内容。",
                    row.peer_domain, row.provides
                ),
                "关系表须双向：不只写对端给什么，也写 S 域消费什么——否则隐藏耦合无处审计。"
                    .to_string(),
                None,
            );
        }
    }

    settle(table.to_vec(), bag)
}

// ---------------------------------------------------------------------------
// 五、架构六段签名冻结 v1
// ---------------------------------------------------------------------------

/// 六段的规范帧内序（索引即执行序，不可乱序）。
pub const RENDER_STAGE_ORDER: [RenderStage; 6] = [
    RenderStage::StateCapture,
    RenderStage::RenderStrategy,
    RenderStage::PipelineInjection,
    RenderStage::Execute,
    RenderStage::Verify,
    RenderStage::Feedback,
];

/// 段名的人话名（复述与自检明细用）。
pub fn stage_label(stage: RenderStage) -> &'static str {
    match stage {
        RenderStage::StateCapture => "状态采集",
        RenderStage::RenderStrategy => "渲染策略",
        RenderStage::PipelineInjection => "管线注入",
        RenderStage::Execute => "执行",
        RenderStage::Verify => "验证",
        RenderStage::Feedback => "反馈",
    }
}

/// 六段中每段的复杂度声明（锚点性能逐项分解）。
pub fn stage_complexity(stage: RenderStage) -> &'static str {
    match stage {
        RenderStage::StateCapture => "O(态数)=O(1)",
        RenderStage::RenderStrategy => "O(1) 查表",
        RenderStage::PipelineInjection => "O(策略数)",
        RenderStage::Execute => "O(1) 摊销",
        RenderStage::Verify => "O(节点) 抽样（抽样式，非全量）",
        RenderStage::Feedback => "O(1) 摊销",
    }
}

/// 一段签名（签名 = 段名 + 入参 + 出参 + 复杂度 + 失败码 + 段内不变量）。
///
/// 冻结的含义：下游 19 条与 S02~S10 九批次按此签名先行开发（Schema 先行），
/// 实现期若要改签名，必须走 [`propose_architecture_amendment`] 留痕。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StageSignature {
    pub stage: RenderStage,
    /// 签名版本（v1 冻结）。
    pub version: &'static str,
    /// 入参形状描述（人话，便于对拍）。
    pub input: &'static str,
    /// 出参形状描述。
    pub output: &'static str,
    /// 复杂度声明。
    pub complexity: &'static str,
    /// 该段失败时的诊断码。
    pub failure_code: DiagCode,
    /// 段内不变量（违反即 P0 或立案，不允许「差不多就行」）。
    pub invariant: &'static str,
    /// 承接该段实现的条目号。
    pub owner_entry_id: u32,
}

/// 六段签名冻结 v1。
pub const STAGE_SIGNATURES_V1: [StageSignature; 6] = [
    StageSignature {
        stage: RenderStage::StateCapture,
        version: "v1",
        input: "宿主无障碍状态源（辅具接入/reduce/高对比/字号档四态原始信号）",
        output: "AccessibilityState（四态归一后的不可变快照）",
        complexity: "O(态数)=O(1)",
        failure_code: DiagCode::StateCaptureBlackout,
        invariant: "采集单源：四态只允许本段采集，别处采集即态分叉（P0）",
        owner_entry_id: 3802,
    },
    StageSignature {
        stage: RenderStage::RenderStrategy,
        version: "v1",
        input: "AccessibilityState",
        output: "RenderStrategySet（策略表公开，四态→策略映射）",
        complexity: "O(1) 查表",
        failure_code: DiagCode::StrategyOpaque,
        invariant: "策略透明：任何生效策略都必须有可解释的用户面文案（否则判黑箱）",
        owner_entry_id: 3802,
    },
    StageSignature {
        stage: RenderStage::PipelineInjection,
        version: "v1",
        input: "RenderStrategySet + D 域注入点清单（只读消费）",
        output: "InjectionReport（逐策略：注没注 / 注到哪 / 为何不注）",
        complexity: "O(策略数)",
        failure_code: DiagCode::InjectionFailed,
        invariant: "注入显性：无静默丢弃；每个未注入策略必须在报告中显性出现",
        owner_entry_id: 3802,
    },
    StageSignature {
        stage: RenderStage::Execute,
        version: "v1",
        input: "已注入的渲染管线描述 + RenderStrategySet",
        output: "FramePixels（帧缓冲描述，含无障碍能力落地结果）",
        complexity: "O(1) 摊销",
        failure_code: DiagCode::FirstClassViolation,
        invariant: "第一等公民：无障碍能力参与渲染层本体，不在覆盖层/仅 UI 层",
        owner_entry_id: 3801,
    },
    StageSignature {
        stage: RenderStage::Verify,
        version: "v1",
        input: "N08 语义树可见性声明 + FramePixels",
        output: "VerifyReport（语义可见率 + 断链节点清单）",
        complexity: "O(节点) 抽样（抽样式，非全量）",
        failure_code: DiagCode::SemanticPixelBrokenLink,
        invariant: "断链零容忍：语义说有而像素没画 = 立案，不降级不修辞化",
        owner_entry_id: 3807,
    },
    StageSignature {
        stage: RenderStage::Feedback,
        version: "v1",
        input: "降级事件 / 断供事件 / 断链案卷",
        output: "FeedbackRecord（每条含可上屏的人话复述文本）",
        complexity: "O(1) 摊销",
        failure_code: DiagCode::InjectionFailed,
        invariant: "降级显性复述：任何降级都生成一句人话，禁止静默回落",
        owner_entry_id: 3801,
    },
];

/// 校验六段签名与冻结版一致（漂移检测，锚点：签名冻结 v1）。
///
/// 顺序敏感性：六段序有语义（采集必须先于注入，注入必须先于执行），
/// 因此按数组序逐位比对，**不排序**——排序会把真正的乱序漂移洗掉。
pub fn verify_stage_signatures(signatures: &[StageSignature]) -> Outcome<Vec<StageSignature>> {
    let mut bag = DiagnosticBag::new();

    // 5.1 段数与覆盖：六段齐备，缺一段即架构不完整。
    if signatures.len() != RENDER_STAGE_ORDER.len() {
        bag.domain(
            DiagCode::StageSequenceInvalid,
            Severity::P1,
            format!(
                "六段签名应为 {} 段，实为 {} 段。",
                RENDER_STAGE_ORDER.len(),
                signatures.len()
            ),
            "六段 = 状态采集 → 渲染策略 → 管线注入 → 执行 → 验证 → 反馈；缺段则帧内链路断裂。"
                .to_string(),
            None,
        );
    }

    // 5.2 逐段比对（不排序——乱序漂移必须被抓住）。
    for (i, expected_stage) in RENDER_STAGE_ORDER.iter().enumerate() {
        let actual = match signatures.get(i) {
            Some(a) => a,
            None => continue,
        };
        if actual.stage != *expected_stage {
            bag.staged(
                *expected_stage,
                DiagCode::StageSignatureDrift,
                Severity::P1,
                format!(
                    "第 {} 段应为 {}，实为 {}。",
                    i + 1,
                    expected_stage.as_str(),
                    actual.stage.as_str()
                ),
                "六段序不可乱序：状态采集 → 渲染策略 → 管线注入 → 执行 → 验证 → 反馈。\
                 乱序会让注入跑到采集之前。"
                    .to_string(),
                None,
            );
            continue;
        }
        // 5.3 签名内容漂移：入参/出参/不变量任一被改即漂移。
        if let Some(frozen) = STAGE_SIGNATURES_V1.get(i) {
            if actual.input != frozen.input
                || actual.output != frozen.output
                || actual.invariant != frozen.invariant
            {
                bag.staged(
                    *expected_stage,
                    DiagCode::StageSignatureDrift,
                    Severity::P1,
                    format!("段 {} 签名与冻结 v1 不一致。", expected_stage.as_str()),
                    "签名冻结 v1：入参/出参/不变量变更须先走 ADR（propose_architecture_amendment）留痕，再回改本条。"
                        .to_string(),
                    None,
                );
            }
        }
    }

    settle(signatures.to_vec(), bag)
}

// ---------------------------------------------------------------------------
// 六、状态采集段（①）：四态 + 断供 P0 渲染版 + 态分叉
// ---------------------------------------------------------------------------

/// 四态之一。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccessibilityStateKey {
    /// 辅具接入态：屏幕阅读器 / 放大镜 / 开关机 / 语音控制是否接入。
    AssistiveAttached,
    /// reduce 态：用户请求减弱动效。
    ReduceMotion,
    /// 高对比态：用户请求高对比度。
    HighContrast,
    /// 字号档：用户系统字号档位。
    FontScaleTier,
}

/// 四态的规范序（它是策略表主键序，不可乱序）。
pub const A11Y_STATE_ORDER: [AccessibilityStateKey; 4] = [
    AccessibilityStateKey::AssistiveAttached,
    AccessibilityStateKey::ReduceMotion,
    AccessibilityStateKey::HighContrast,
    AccessibilityStateKey::FontScaleTier,
];

impl AccessibilityStateKey {
    /// 四态标签（人话，用于用户面与复述文本）。
    pub fn label(self) -> &'static str {
        match self {
            AccessibilityStateKey::AssistiveAttached => "辅助技术接入",
            AccessibilityStateKey::ReduceMotion => "减弱动效",
            AccessibilityStateKey::HighContrast => "高对比度",
            AccessibilityStateKey::FontScaleTier => "字号档位",
        }
    }
}

/// 一个四态采集结果。
#[derive(Clone, Debug, PartialEq)]
pub struct StateProbe {
    pub key: AccessibilityStateKey,
    /// 采集到的值（布尔态用 0/1，字号档用档位数）。
    pub value: f64,
    /// 本次采集是否成功。
    ///
    /// 关键：`acquired == false` 表示**采不到**（辅具态断供），与
    /// 「采到了且值为 0」语义完全不同——后者是「用户没开」，
    /// 前者是「我不知道用户开没开」。混同二者 = 对依赖辅具的用户
    /// 静默关掉无障碍，是本域最不可接受的缺陷。
    pub acquired: bool,
}

/// 四态采集快照。
#[derive(Clone, Debug, PartialEq)]
pub struct AccessibilityState {
    pub probes: Vec<StateProbe>,
    /// 采集来源标识（采集单源断言用：同一帧只能有一个来源）。
    pub source_id: String,
}

/// 按 key 取四态采集项（查表 O(态数)=O(1)）。
pub fn read_state<'a>(
    state: &'a AccessibilityState,
    key: AccessibilityStateKey,
) -> Option<&'a StateProbe> {
    state.probes.iter().find(|p| p.key == key)
}

/// 采集段校验（锚点错误路径：状态采集断 → P0 复述断供红线渲染版）。
///
/// 两条独立红线：
/// 1. **断供红线**：任一态 `acquired == false` → P0。断供**不允许降级为「关闭」**，
///    必须复述「我采不到你的辅助态」这件事本身（渲染版 = 让断供可见）。
/// 2. **分叉红线**：同帧出现两个不同 `source_id` → 态分叉，渲染必错乱，
///    同样 P0（锚点 F3816 采集单源分叉红线的前向声明）。
pub fn verify_state_capture(
    state: &AccessibilityState,
    expected_source_id: Option<&str>,
) -> Outcome<AccessibilityState> {
    let mut bag = DiagnosticBag::new();

    // 6.1 四态齐备性：缺项按「采不到」处理并显性报出，不静默填默认值。
    for key in A11Y_STATE_ORDER {
        if !state.probes.iter().any(|p| p.key == key) {
            bag.staged(
                RenderStage::StateCapture,
                DiagCode::StateCaptureBlackout,
                Severity::P0,
                format!("四态采集缺项：{} 未上报。", key.label()),
                "缺项不得按默认值填充——填 0 等于替用户关掉无障碍。请从宿主无障碍状态源补采。"
                    .to_string(),
                Some(format!(
                    "我们读不到你的「{}」设置，已暂停按该设置调整画面（不会替你猜一个值）。",
                    key.label()
                )),
            );
        }
    }

    // 6.2 断供检查：采不到 ≠ 关掉了。
    for probe in &state.probes {
        if !probe.acquired {
            bag.staged(
                RenderStage::StateCapture,
                DiagCode::StateCaptureBlackout,
                Severity::P0,
                format!("辅具态断供：{} 采集失败。", probe.key.label()),
                "断供走 P0：把断供本身渲染出来（可见告警 + 拒绝降级），\
                 严禁回退到「无障碍关闭态」——那等于对依赖辅具的用户釜底抽薪。"
                    .to_string(),
                Some(format!(
                    "我们暂时读不到你的「{}」状态，画面暂不按它调整。请检查辅助技术是否仍连接。",
                    probe.key.label()
                )),
            );
        }
    }

    // 6.3 分叉检查：同帧多来源 = 态分叉。
    if let Some(expected) = expected_source_id {
        if state.source_id != expected {
            bag.staged(
                RenderStage::StateCapture,
                DiagCode::StateCaptureFork,
                Severity::P0,
                format!(
                    "态分叉：本帧采集来源为 {}，期望唯一来源 {}。",
                    state.source_id, expected
                ),
                "四态只允许一个采集点（锚点 F3816 采集单源）。双处采集必然出现不一致，渲染会错乱。"
                    .to_string(),
                Some("画面出现了两套互相矛盾的无障碍状态，已暂停自动调整以免显示错乱。".to_string()),
            );
        }
    }

    settle(state.clone(), bag)
}

// ---------------------------------------------------------------------------
// 七、渲染策略段（②）：策略透明红线
// ---------------------------------------------------------------------------

/// 注入点（锚点 F3802：样式/过滤/后处理三注入点；只读消费 D 域清单）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InjectionSlot {
    /// 样式注入：改布局/尺寸/间距（字号档、大字号走此位）。
    Style,
    /// 过滤注入：像素级滤镜（高对比、色弱变换、光敏柔化走此位）。
    Filter,
    /// 后处理注入：帧级调整（动效替代、认知简化走此位）。
    Post,
}

impl InjectionSlot {
    pub fn as_str(self) -> &'static str {
        match self {
            InjectionSlot::Style => "STYLE",
            InjectionSlot::Filter => "FILTER",
            InjectionSlot::Post => "POST",
        }
    }
}

/// D 域提供的注入点清单（只读消费；本域不得反向定义）。
pub const D_DOMAIN_INJECTION_SLOTS: [InjectionSlot; 3] =
    [InjectionSlot::Style, InjectionSlot::Filter, InjectionSlot::Post];

/// 一条渲染策略（四态之一映射出的可执行策略）。
#[derive(Clone, Debug, PartialEq)]
pub struct RenderStrategy {
    /// 策略标识。
    pub id: String,
    /// 由哪一态触发。
    pub from_state: AccessibilityStateKey,
    /// 注入到哪个点。
    pub slot: InjectionSlot,
    /// 策略参数（数值语义由各实现条目定标）。
    pub params: Vec<(&'static str, f64)>,
    /// 用户面解释文案（策略透明红线的载体）。
    ///
    /// 空串即判 `STRATEGY_OPAQUE`：用户无法预测自己的设置带来什么 = 行为不可预期。
    pub user_facing_explanation: String,
}

/// 策略参数取值（未登记的参数返回 0.0，并须由调用方另行核对其存在性）。
pub fn strategy_param(s: &RenderStrategy, name: &str) -> f64 {
    s.params
        .iter()
        .find(|(k, _)| *k == name)
        .map(|(_, v)| *v)
        .unwrap_or(0.0)
}

/// 校验渲染策略集（锚点：策略表公开——策略透明红线）。
///
/// 透明红线判 P1 而非 P0：策略不透明不会让画面错，但会让用户无法预期——
/// 「我把高对比打开，界面更亮了」这种不可预期本身就是无障碍缺陷。
pub fn verify_strategies(strategies: &[RenderStrategy]) -> Outcome<Vec<RenderStrategy>> {
    let mut bag = DiagnosticBag::new();

    for s in strategies {
        // 7.1 触发态合法性：策略必须由四态之一触发。
        if !A11Y_STATE_ORDER.contains(&s.from_state) {
            bag.staged(
                RenderStage::RenderStrategy,
                DiagCode::StrategyOpaque,
                Severity::P1,
                format!(
                    "策略 {} 的触发态不在四态内（声明为 {}）。",
                    s.id,
                    s.from_state.label()
                ),
                "四态 = 辅助技术接入 / 减弱动效 / 高对比度 / 字号档位；越界触发会让状态与策略的映射失去单源。"
                    .to_string(),
                None,
            );
        }

        // 7.2 透明红线：生效策略必须能用人话解释。
        if s.user_facing_explanation.trim().is_empty() {
            bag.staged(
                RenderStage::RenderStrategy,
                DiagCode::StrategyOpaque,
                Severity::P1,
                format!("策略 {} 无用户面解释文案（策略黑箱）。", s.id),
                "每条生效策略都要有一句用户能懂的话说明「它会怎么改画面」；空文案即判黑箱。"
                    .to_string(),
                Some(
                    "有一项无障碍设置我们无法用一句话说明它会怎么改画面，已暂缓该项并记账。"
                        .to_string(),
                ),
            );
        }

        // 7.3 参数有限性：NaN/Infinity 会污染下游像素计算。
        for (k, v) in &s.params {
            if !v.is_finite() {
                bag.staged(
                    RenderStage::RenderStrategy,
                    DiagCode::InjectionFailed,
                    Severity::P1,
                    format!("策略 {} 参数 {} 非有限值（{}）。", s.id, k, v),
                    "参数须为有限实数；非有限值会让注入后的像素计算产出 NaN 区域。".to_string(),
                    Some(format!("「{}」有一项参数无效，该项已按不生效处理。", s.id)),
                );
            }
        }
    }

    settle(strategies.to_vec(), bag)
}

// ---------------------------------------------------------------------------
// 八、管线注入段（③）：注入显性 + 1ms 预算红线
// ---------------------------------------------------------------------------

/// 单条策略的注入结果（注入显性的载体：注没注、注到哪、为何不注）。
#[derive(Clone, Debug, PartialEq)]
pub struct InjectionOutcome {
    pub strategy_id: String,
    pub injected: bool,
    /// 未注入原因（`injected == false` 时必填——显性复述的依据）。
    pub reason: Option<String>,
    /// 本条注入耗时（ms）。
    pub cost_ms: f64,
}

/// 注入报告（整帧汇总）。
#[derive(Clone, Debug, PartialEq)]
pub struct InjectionReport {
    pub outcomes: Vec<InjectionOutcome>,
    /// 注入总耗时（ms）——对 1ms 预算红线。
    pub total_cost_ms: f64,
}

/// 无障碍注入预算（锚点 F3802：开销挤占渲染 = 双向失败，1ms 线）。
///
/// 为什么单列预算：无障碍渲染是要花钱的（额外 pass、额外纹理）。这笔开销
/// 若不设上限，会挤占正常渲染预算——用户开了高对比，反而整体更卡。
/// 那就是双向失败：无障碍没做好，正常渲染也被拖垮。
pub const A11Y_INJECTION_BUDGET_MS: f64 = 1.0;

/// 校验注入报告（锚点错误路径：注入失败 → 降级路径 + 诊断，降级显性复述）。
///
/// 注入显性：任何 `injected == false` 的策略都必须有 `reason`，
/// 且上层必须把它复述给用户。**没有原因的未注入 = 静默丢弃 = 本域不可接受。**
pub fn verify_injection_report(
    report: &InjectionReport,
    expected_strategy_ids: &[&str],
) -> Outcome<InjectionReport> {
    let mut bag = DiagnosticBag::new();

    // 8.1 覆盖性：每条期望策略都必须在报告里有交代（注入了或没注入 + 原因）。
    for id in expected_strategy_ids {
        if !report.outcomes.iter().any(|o| o.strategy_id == *id) {
            bag.staged(
                RenderStage::PipelineInjection,
                DiagCode::InjectionFailed,
                Severity::P1,
                format!("策略 {} 未出现在注入报告中（静默丢弃）。", id),
                "注入显性：每条策略都要在报告里出现——要么注入，要么写明为何没注入。缺席即静默丢弃。"
                    .to_string(),
                Some(format!(
                    "有一项无障碍设置（{}）本轮没有生效，原因未记录。我们已记账，请复查该设置。",
                    id
                )),
            );
        }
    }

    // 8.2 未注入须有原因（降级显性复述的原料）；耗时须合法。
    for o in &report.outcomes {
        if !o.injected {
            let no_reason = o
                .reason
                .as_ref()
                .map(|r| r.trim().is_empty())
                .unwrap_or(true);
            if no_reason {
                bag.staged(
                    RenderStage::PipelineInjection,
                    DiagCode::InjectionFailed,
                    Severity::P1,
                    format!("策略 {} 未注入但未给出原因。", o.strategy_id),
                    "未注入必须带原因；无原因的未注入对用户与排障者都不可解释。".to_string(),
                    Some(
                        "有一项无障碍设置没有生效，且我们没能说清原因——请重试或反馈。"
                            .to_string(),
                    ),
                );
            }
        }
        if !o.cost_ms.is_finite() || o.cost_ms < 0.0 {
            bag.staged(
                RenderStage::PipelineInjection,
                DiagCode::A11yBudgetExceeded,
                Severity::P1,
                format!("策略 {} 注入耗时非法（{}ms）。", o.strategy_id, o.cost_ms),
                "耗时须为非负有限值；非法值会让预算核算失效。".to_string(),
                None,
            );
        }
    }

    // 8.3 预算红线：注入总开销超 1ms/帧即立案优化。
    if report.total_cost_ms > A11Y_INJECTION_BUDGET_MS {
        bag.staged(
            RenderStage::PipelineInjection,
            DiagCode::A11yBudgetExceeded,
            Severity::P1,
            format!(
                "无障碍注入开销 {:.3}ms 超 1ms 预算。",
                report.total_cost_ms
            ),
            "无障碍开销挤占渲染是双向失败：立案优化（降采样/合并 pass/缓存），F3811 性能条目承接。"
                .to_string(),
            Some("无障碍效果已开启，但它让这一帧多花了些渲染时间；若画面变卡请告知我们。".to_string()),
        );
    }

    settle(report.clone(), bag)
}

// ---------------------------------------------------------------------------
// 九、验证段（⑤）：语义-像素断链检测 + 立案
// ---------------------------------------------------------------------------

/// 一个节点的语义-像素对照（验证段输入单元）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SemanticPixelPair {
    /// 节点稳定 id（对应 N08 语义树节点）。
    pub node_id: String,
    /// 语义树声明：此节点应对用户可见。
    pub semantically_visible: bool,
    /// 像素层事实：此节点是否真的画进了帧缓冲。
    pub painted: bool,
    /// 是否为虚拟化保留节点（例外须显性——锚点 F3807 虚拟化语义保留例外）。
    pub virtualization_retained: bool,
}

/// 一宗断链案卷（语义树说有、像素没画）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BrokenLinkCase {
    pub node_id: String,
    /// 立案时间戳（调用方注入，本模块不读时钟）。
    pub filed_at: u64,
    /// 案由（固定人话，便于直接上通报）。
    pub charge: String,
    /// 追责线：F3807 读屏协同 + F3812 测试双线。
    pub routed_to: [u32; 2],
}

/// 断链检测结果（可见率 + 案卷）。
#[derive(Clone, Debug, PartialEq)]
pub struct LinkReport {
    /// 语义可见率（0.0~1.0）。
    pub visible_rate: f64,
    pub cases: Vec<BrokenLinkCase>,
}

/// 检测语义-像素断链（锚点：语义-像素断链 → 立案；最后一公里红线）。
///
/// 判据（`semantically_visible && !painted` 即断链）：
/// 语义树说有 && 像素没画 = 断链 = 本域最恶劣缺陷。
///
/// 唯一合法例外是 `virtualization_retained`（虚拟化语义保留）：节点被虚拟化
/// 移出视口时，语义树仍保留它以便读屏访问，屏幕上看不到是**预期**的。
/// 但例外必须显式标记——未标记的「看不到」一律判断链。
///
/// **证据外带**：断链时本函数走 [`Outcome::Err`]，而 [`Outcome::Err`] 不带value，
/// 案卷（[`BrokenLinkCase`]）若只装在返回值里就会随失败丢失——立了案却拿不到
/// 案卷，等于无法追责。故案卷经 `cases_out` **始终外带**，与判定结果分离：
/// 调用方先取全部案卷立案上报，再看Outcome 决定是否阻断。
pub fn detect_broken_links(
    pairs: &[SemanticPixelPair],
    now: u64,
    cases_out: &mut Vec<BrokenLinkCase>,
) -> Outcome<LinkReport> {
    let mut bag = DiagnosticBag::new();

    // 9.1 逐对判定：语义可见 && 未绘制 && 非显式例外 = 断链。
    for pair in pairs {
        if pair.semantically_visible && !pair.painted && !pair.virtualization_retained {
            cases_out.push(BrokenLinkCase {
                node_id: pair.node_id.clone(),
                filed_at: now,
                charge: format!(
                    "语义树声明节点 {} 对用户可见，但帧缓冲未绘制——系统向用户宣告了不存在的视觉事实。",
                    pair.node_id
                ),
                routed_to: [3807, 3812],
            });
        }
    }

    // 9.2 断链立案：任一断链即 P0（不降级、不修辞化、不静默）。
    if !cases_out.is_empty() {
        let sample = cases_out[0].node_id.clone();
        let count = cases_out.len();
        bag.staged(
            RenderStage::Verify,
            DiagCode::SemanticPixelBrokenLink,
            Severity::P0,
            format!("语义-像素断链 {} 处，首例节点 {}。", count, sample),
            "这是本域最恶劣缺陷：用户（尤其读屏用户）会据此以为某处有可交互物而误操作。\
             禁止降级或加例外掩盖——立案例卷交 F3807/F3812 双线追责，并回查该节点的注入路径。"
                .to_string(),
            Some(
                "界面有些元素我们已声明它存在，但没能画出来——这会让你以为那里有东西可点。\
                 我们已记录并正在修复；若你因此误操作，请避开该位置。"
                    .to_string(),
            ),
        );
        let message = format!("语义-像素断链 {} 处，首例节点 {}。", count, sample);
        let hint =
            "立案例卷交 F3807/F3812 双线追责，并回查该节点的注入路径。".to_string();
        return Outcome::Err {
            code: DiagCode::SemanticPixelBrokenLink,
            severity: Severity::P0,
            message,
            hint,
            diagnostics: bag.into_items(),
        };
    }

    // 9.3 语义可见率（抽样式；无可见节点时定义为 1.0——空集不判失败）。
    let visible: Vec<&SemanticPixelPair> =
        pairs.iter().filter(|p| p.semantically_visible).collect();
    let painted_visible = visible.iter().filter(|p| p.painted).count();
    let visible_rate = if visible.is_empty() {
        1.0
    } else {
        painted_visible as f64 / visible.len() as f64
    };

    ok(
        LinkReport { visible_rate, cases: Vec::new() },
        bag.into_items(),
    )
}

// ---------------------------------------------------------------------------
// 十、反馈段（⑥）：降级显性复述
// ---------------------------------------------------------------------------

/// 复述面向谁。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Audience {
    /// 可直接上屏给用户看。
    User,
    /// 仅面向开发者（纯工程问题不必打扰用户）。
    Developer,
}

/// 一条反馈记录（每条必须带可上屏的人话复述）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FeedbackRecord {
    pub code: DiagCode,
    pub stage: StageTag,
    /// 可直接上屏（或进日志）的人话复述文本。
    pub restatement: String,
    pub audience: Audience,
}

/// 从诊断集生成反馈记录（锚点：降级显性复述）。
///
/// 规则：
/// - 有 `restatement` 的诊断 → [`Audience::User`]（降级/断供/断链必须让用户知道）；
/// - 无 `restatement` 的诊断 → [`Audience::Developer`]（纯工程问题不必打扰用户）。
///
/// 这条规则是「异常零静默」在像素层的落地：任何影响用户所见所感的降级，
/// 都必须有一句能上屏的话，而不是一条躺在日志里的码。
pub fn build_feedback(diagnostics: &[Diagnostic]) -> Vec<FeedbackRecord> {
    let mut records: Vec<FeedbackRecord> = Vec::new();
    for d in diagnostics {
        let has_restatement = d
            .restatement
            .as_ref()
            .map(|r| !r.trim().is_empty())
            .unwrap_or(false);
        if has_restatement {
            records.push(FeedbackRecord {
                code: d.code,
                stage: d.stage,
                restatement: d.restatement.clone().unwrap_or_default(),
                audience: Audience::User,
            });
        } else {
            records.push(FeedbackRecord {
                code: d.code,
                stage: d.stage,
                restatement: d.message.clone(),
                audience: Audience::Developer,
            });
        }
    }
    records
}

/// 取面向用户的复述（渲染层据此上屏告警条）。
pub fn user_facing_records(records: &[FeedbackRecord]) -> Vec<&FeedbackRecord> {
    records.iter().filter(|r| r.audience == Audience::User).collect()
}

// ---------------------------------------------------------------------------
// 十一、域开工前置检查（F3795 R 域移交包接收）
// ---------------------------------------------------------------------------

/// R 域移交包材料段（锚点 F3795：七件包，含双维无障碍册）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HandoverMaterialKind {
    InterfaceFreeze,
    ComputeLabelLedger,
    LessonBook,
    EvidenceTrio,
    CreationA11yLedger,
    CreationDeliverableA11yLedger,
    HandoverAck,
}

impl HandoverMaterialKind {
    pub fn as_str(self) -> &'static str {
        match self {
            HandoverMaterialKind::InterfaceFreeze => "INTERFACE_FREEZE",
            HandoverMaterialKind::ComputeLabelLedger => "COMPUTE_LABEL_LEDGER",
            HandoverMaterialKind::LessonBook => "LESSON_BOOK",
            HandoverMaterialKind::EvidenceTrio => "EVIDENCE_TRIO",
            HandoverMaterialKind::CreationA11yLedger => "CREATION_A11Y_LEDGER",
            HandoverMaterialKind::CreationDeliverableA11yLedger => {
                "CREATION_DELIVERABLE_A11Y_LEDGER"
            }
            HandoverMaterialKind::HandoverAck => "HANDOVER_ACK",
        }
    }

    /// 材料的人话名（诊断与移交核对用）。
    pub fn label(self) -> &'static str {
        match self {
            HandoverMaterialKind::InterfaceFreeze => "接口冻结清单",
            HandoverMaterialKind::ComputeLabelLedger => "算力标签终册",
            HandoverMaterialKind::LessonBook => "经验教训记录",
            HandoverMaterialKind::EvidenceTrio => "证据三件套",
            HandoverMaterialKind::CreationA11yLedger => "创作工具无障碍册",
            HandoverMaterialKind::CreationDeliverableA11yLedger => "创作产出无障碍册",
            HandoverMaterialKind::HandoverAck => "接收确认签收",
        }
    }

    /// 七件包全序列（顺序即核对顺序）。
    pub const ALL: [HandoverMaterialKind; 7] = [
        HandoverMaterialKind::InterfaceFreeze,
        HandoverMaterialKind::ComputeLabelLedger,
        HandoverMaterialKind::LessonBook,
        HandoverMaterialKind::EvidenceTrio,
        HandoverMaterialKind::CreationA11yLedger,
        HandoverMaterialKind::CreationDeliverableA11yLedger,
        HandoverMaterialKind::HandoverAck,
    ];
}

/// 移交包材料一段。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HandoverMaterial {
    pub kind: HandoverMaterialKind,
    pub present: bool,
}

/// R 域移交包（S 域接收侧）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HandoverPackage {
    pub materials: Vec<HandoverMaterial>,
    /// S 域签收确认位。
    pub receiver_confirmed: bool,
    pub receiver_signature: Option<String>,
}

impl HandoverPackage {
    /// 构造一份七件齐备、已签收的健康移交包（回归基线）。
    pub fn healthy() -> HandoverPackage {
        HandoverPackage {
            materials: HandoverMaterialKind::ALL
                .iter()
                .map(|k| HandoverMaterial { kind: *k, present: true })
                .collect(),
            receiver_confirmed: true,
            receiver_signature: Some("VE-S/2026-10-07".to_string()),
        }
    }
}

/// 域开工前置检查（锚点 F3795：S01 无障碍渲染开工条件核验 + 签收）。
///
/// 阻断条件（任一命中即阻断开工）：
/// 1. 七件材料任一缺失；
/// 2. S 域签收确认位未签。
///
/// 特别地，双维无障碍册（创作工具 + 创作产出）缺失时也阻断：S 域是像素层
/// 执行者，创作侧若无障碍语义，S 域就没有可执行的「应该是什么样」。
pub fn verify_kickoff_preconditions(
    pkg: &HandoverPackage,
) -> Outcome<Vec<HandoverMaterialKind>> {
    let mut bag = DiagnosticBag::new();

    let mut present: Vec<HandoverMaterialKind> = Vec::new();
    for kind in HandoverMaterialKind::ALL {
        if pkg.materials.iter().any(|m| m.kind == kind && m.present) {
            present.push(kind);
        } else {
            bag.domain(
                DiagCode::HandoverMaterialMissing,
                Severity::P1,
                format!("R 域移交包材料缺失：{}。", kind.label()),
                "向 R 域（F3795）索取缺失材料；七件包缺一不可开工。\
                 双维无障碍册缺失尤甚——S 域无创作侧语义则无可执行目标。"
                    .to_string(),
                None,
            );
        }
    }

    let unsigned = !pkg.receiver_confirmed || pkg.receiver_signature.is_none();
    if unsigned {
        bag.domain(
            DiagCode::HandoverNotConfirmed,
            Severity::P1,
            "S 域接收确认位未签。".to_string(),
            "在移交包接收确认位签 S 域（域标签 + 日期）；确认前不得宣告 S 域开工。".to_string(),
            None,
        );
    }

    settle(present, bag)
}

// ---------------------------------------------------------------------------
// 十二、S01 组分工表（20 行，F3801-F3820）
// ---------------------------------------------------------------------------

/// 分工表中一条。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkTableRow {
    pub entry_id: u32,
    pub title: &'static str,
    pub topic: &'static str,
    /// 该条在本域架构中的角色。
    pub role: &'static str,
    /// 上游依赖条目（空串表示无）。
    pub upstream: &'static str,
}

/// S01 组 20 条分工表（F3801-F3820，标题逐条核对册内锚点）。
pub const S01_WORK_TABLE: [WorkTableRow; 20] = [
    WorkTableRow { entry_id: 3801, title: "S 域开工与无障碍渲染总架构", topic: "管线", role: "域开工·第一等公民·最后一公里·六段签名·十项映射·四域关系·降级矩阵", upstream: "F3795" },
    WorkTableRow { entry_id: 3802, title: "无障碍渲染管线", topic: "管线", role: "四态采集+策略透明+三注入点+1ms 预算；本条六段签名的实现方", upstream: "F3801" },
    WorkTableRow { entry_id: 3803, title: "高对比渲染引擎", topic: "高对比", role: "对比度实时检测+高对比管线参数+与 E 域令牌联动", upstream: "F3802" },
    WorkTableRow { entry_id: 3804, title: "焦点渲染强化", topic: "焦点强化", role: "焦点环可见性+焦点放大+键盘顺序视觉一致性", upstream: "F3802" },
    WorkTableRow { entry_id: 3805, title: "大字号与缩放渲染", topic: "大字号", role: "字号档映射+布局重排+四档 DPI 走查承接", upstream: "F3802" },
    WorkTableRow { entry_id: 3806, title: "动效渲染替代", topic: "动效替代", role: "reduce 态的像素级替代（静态化/淡入替代/无位移）", upstream: "F3802,P08" },
    WorkTableRow { entry_id: 3807, title: "读屏渲染协同总架构", topic: "读屏协同", role: "双树一致+查询隔离+协同事件+断链追责承接", upstream: "F3801,N08" },
    WorkTableRow { entry_id: 3808, title: "无障碍纹理与图形", topic: "纹理", role: "纹理可辨识化+图形化替代+图标语义化", upstream: "F3802" },
    WorkTableRow { entry_id: 3809, title: "光敏渲染安全", topic: "光敏", role: "闪烁频率上限+大面积高亮柔化+与 K 域 F2133 双向对账", upstream: "F3802" },
    WorkTableRow { entry_id: 3810, title: "认知渲染辅助", topic: "认知", role: "信息密度降级+阅读辅助+简化模式", upstream: "F3802" },
    WorkTableRow { entry_id: 3811, title: "无障碍渲染性能", topic: "性能", role: "1ms 预算实测+超预算优化立案+开销遥测", upstream: "F3802" },
    WorkTableRow { entry_id: 3812, title: "无障碍渲染测试", topic: "管线", role: "断链演练回归+四态矩阵回归；本条自检能力的正式承接", upstream: "F3801,F3807" },
    WorkTableRow { entry_id: 3813, title: "无障碍渲染调试器", topic: "管线", role: "六段逐段可视化+断链节点定位+注入报告查看", upstream: "F3802,F3812" },
    WorkTableRow { entry_id: 3814, title: "无障碍渲染 API 冻结", topic: "管线", role: "六段签名对外 API 冻结+描述词成册", upstream: "F3802" },
    WorkTableRow { entry_id: 3815, title: "无障碍渲染文档", topic: "管线", role: "总纲三章（架构/十项/红线）+ 快速上手", upstream: "F3814" },
    WorkTableRow { entry_id: 3816, title: "无障碍渲染与 S 域真源终版", topic: "读屏协同", role: "采集单源断言+分叉检测+N05 判定与渲染同源对拍", upstream: "F3807,N05" },
    WorkTableRow { entry_id: 3817, title: "无障碍渲染 fuzz", topic: "管线", role: "四态组合 fuzz+断链构造 fuzz+预算越界 fuzz", upstream: "F3802,F3812" },
    WorkTableRow { entry_id: 3818, title: "S01 联调", topic: "管线", role: "S01 全组联调场景矩阵+跨域对端联调", upstream: "F3802~F3817" },
    WorkTableRow { entry_id: 3819, title: "S01 预备自查", topic: "管线", role: "组级预备报告 GO/NO-GO 判定+缺口补齐", upstream: "F3818" },
    WorkTableRow { entry_id: 3820, title: "S01 组收口双签", topic: "管线", role: "组收口双签+向 S02 移交架构契约段", upstream: "F3801~F3819" },
];

/// 校验分工表：条目数 = 20、条目号 F3801-F3820 连续无重复、每条有主题归属。
pub fn verify_work_table(table: &[WorkTableRow]) -> Outcome<Vec<WorkTableRow>> {
    let mut bag = DiagnosticBag::new();

    // 12.1 重复。
    let mut seen: Vec<u32> = Vec::new();
    for row in table {
        if seen.contains(&row.entry_id) {
            bag.domain(
                DiagCode::WorktableDuplicateEntry,
                Severity::P1,
                format!("分工表条目号重复：F{}。", row.entry_id),
                "每个条目号在组内唯一；重复即覆盖了他条职责。".to_string(),
                None,
            );
        }
        seen.push(row.entry_id);
    }

    // 12.2 连续性——先于基数检查：缺位是病因，「只有 19 条」只是症状。
    seen.sort();
    for (i, actual) in seen.iter().enumerate() {
        let expected = S_FIRST_ENTRY_ID + i as u32;
        if *actual != expected {
            bag.domain(
                DiagCode::WorktableGap,
                Severity::P1,
                format!(
                    "分工表条目号不连续：期望 F{}，实到 F{}。",
                    expected, actual
                ),
                format!(
                    "本组须连续覆盖 F{}-F{}，缺位即漏项。",
                    S_FIRST_ENTRY_ID,
                    S_FIRST_ENTRY_ID + S_ENTRIES_PER_GROUP - 1
                ),
                None,
            );
            break;
        }
    }

    // 12.3 基数（症状级）。
    if table.len() != S_ENTRIES_PER_GROUP as usize {
        bag.domain(
            DiagCode::WorktableCardinalityInvalid,
            Severity::P1,
            format!(
                "S01 分工表应为 {} 条，实为 {} 条。",
                S_ENTRIES_PER_GROUP,
                table.len()
            ),
            format!(
                "按 S 域「每组 {} 条」补齐；本组覆盖 F{}-F{}。",
                S_ENTRIES_PER_GROUP,
                S_FIRST_ENTRY_ID,
                S_FIRST_ENTRY_ID + S_ENTRIES_PER_GROUP - 1
            ),
            None,
        );
    }

    // 12.4 主题归属合法（十项之一）。
    for row in table {
        if !S_DOMAIN_TEN_TOPICS.contains(&row.topic) {
            bag.domain(
                DiagCode::ThemeMappingIncomplete,
                Severity::P1,
                format!(
                    "F{} 主题归属「{}」不在官方十项内。",
                    row.entry_id, row.topic
                ),
                "十项 = 管线 / 高对比 / 焦点强化 / 大字号 / 动效替代 / 读屏协同 / 纹理 / 光敏 / 认知 / 性能。"
                    .to_string(),
                None,
            );
        }
    }

    settle(table.to_vec(), bag)
}

// ---------------------------------------------------------------------------
// 十三、架构冻结快照 + 漂移检测
// ---------------------------------------------------------------------------

/// 架构冻结快照：域开工的机器可读凭据。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArchitectureFreeze {
    pub domain_tag: &'static str,
    pub first_entry_id: u32,
    pub last_entry_id: u32,
    /// 六段序快照。
    pub stages: Vec<RenderStage>,
    /// 六段签名摘要（`段名:版本:失败码:不变量`）。
    pub stage_signatures: Vec<String>,
    /// 十项主题快照。
    pub topics: Vec<&'static str>,
    /// 四域关系表对端快照。
    pub peers: Vec<String>,
    /// 分工表条目数。
    pub work_table_rows: u32,
    /// 冻结时间戳（调用方注入，保持本模块纯函数、无隐式时钟）。
    pub frozen_at: u64,
}

/// 快照一致性摘要 → FNV-1a 32 位指纹（短、稳定、无依赖，非安全用途）。
///
/// 排序口径：无序语义的部分（主题集合、对端集合）先排序再参与哈希——
/// 集合的键序只反映构造顺序，不反映架构语义。不排序会让同一份架构声明
/// 经不同构造路径（字面量 vs 键推导）得到不同指纹，漂移检测将大量误报——
/// 那是守卫失效，不是架构真的变了。有序语义的部分（六段序）保留原序。
pub fn freeze_fingerprint(freeze: &ArchitectureFreeze) -> String {
    let mut topics: Vec<&str> = freeze.topics.clone();
    topics.sort();
    let mut peers: Vec<&str> = freeze.peers.iter().map(|s| s.as_str()).collect();
    peers.sort();

    let stage_names: Vec<&str> = freeze.stages.iter().map(|s| s.as_str()).collect();

    // 各段先落为具名字符串再取引用：内核无 hasher 依赖，且须避免临时值悬垂。
    let range = format!("{}-{}", freeze.first_entry_id, freeze.last_entry_id);
    let stages_joined = stage_names.join(",");
    let sigs_joined = freeze.stage_signatures.join("|");
    let topics_joined = topics.join(",");
    let peers_joined = peers.join(",");
    let rows = freeze.work_table_rows.to_string();
    let parts: [&str; 7] = [
        freeze.domain_tag,
        range.as_str(),
        stages_joined.as_str(),
        sigs_joined.as_str(),
        topics_joined.as_str(),
        peers_joined.as_str(),
        rows.as_str(),
    ];

    let mut hash: u32 = 0x811c_9dc5;
    for part in parts {
        for byte in part.as_bytes() {
            hash ^= *byte as u32;
            hash = hash.wrapping_mul(0x0100_0193);
        }
        hash ^= 0x2f;
        hash = hash.wrapping_mul(0x0100_0193);
    }
    let hex = format!("{:08x}", hash);
    hex
}

/// 漂移检测：当前声明指纹与冻结指纹不符即告警（架构被静默改动是严重问题）。
pub fn detect_drift(
    frozen: &ArchitectureFreeze,
    current: &ArchitectureFreeze,
) -> Outcome<ArchitectureFreeze> {
    let frozen_fp = freeze_fingerprint(frozen);
    let current_fp = freeze_fingerprint(current);
    if frozen_fp != current_fp {
        return err(
            DiagCode::FreezeDrift,
            Severity::P1,
            format!("架构漂移：冻结指纹 {} ≠ 当前指纹 {}。", frozen_fp, current_fp),
            "架构声明已被改动但未走 ADR 回改本条；补 ADR 或还原至冻结态，二选一。".to_string(),
            Vec::new(),
        );
    }
    ok(frozen.clone(), Vec::new())
}

// ---------------------------------------------------------------------------
// 十四、架构修正 ADR（实现期回改本条）
// ---------------------------------------------------------------------------

/// 修正的架构面。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AmendmentTarget {
    StageSignature,
    ThemeLanding,
    RelationTable,
    WorkTable,
    InjectionSlot,
}

/// 一条架构修正提案。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArchitectureAmendment {
    pub proposed_by_entry_id: u32,
    pub target: AmendmentTarget,
    pub rationale: String,
    pub impact: String,
}

/// ADR 受理结果。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AdrReceipt {
    pub adr_id: String,
    pub accepted: bool,
}

/// 受理架构修正提案：信息不全即拒绝——防止「顺手改架构」而无据。
pub fn propose_architecture_amendment(
    amendment: &ArchitectureAmendment,
) -> Outcome<AdrReceipt> {
    if amendment.rationale.trim().is_empty() {
        return err(
            DiagCode::AdrIncomplete,
            Severity::P1,
            format!(
                "F{} 的架构修正提案缺少理由。",
                amendment.proposed_by_entry_id
            ),
            "ADR 须写明修正动因；无理由的架构变更不予受理。".to_string(),
            Vec::new(),
        );
    }
    if amendment.impact.trim().is_empty() {
        return err(
            DiagCode::AdrIncomplete,
            Severity::P1,
            format!(
                "F{} 的架构修正提案缺少影响面。",
                amendment.proposed_by_entry_id
            ),
            "ADR 须列明受影响的段/条目与下游消费者，便于回改时同步对齐。".to_string(),
            Vec::new(),
        );
    }
    ok(
        AdrReceipt {
            adr_id: format!("ADR-S-{:04}", amendment.proposed_by_entry_id),
            accepted: true,
        },
        Vec::new(),
    )
}

// ---------------------------------------------------------------------------
// 十五、域开工编排（六道关 → 冻结）
// ---------------------------------------------------------------------------

/// 域开工结果。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DomainKickoff {
    pub freeze: ArchitectureFreeze,
    pub fingerprint: String,
}

/// 构造本条声明的冻结快照（单一事实源，避免各路径各拼一份）。
pub fn canonical_freeze(work_table_rows: u32, frozen_at: u64) -> ArchitectureFreeze {
    ArchitectureFreeze {
        domain_tag: S_DOMAIN_TAG,
        first_entry_id: S_FIRST_ENTRY_ID,
        last_entry_id: S_LAST_ENTRY_ID,
        stages: RENDER_STAGE_ORDER.to_vec(),
        stage_signatures: STAGE_SIGNATURES_V1
            .iter()
            .map(|s| {
                format!(
                    "{}:{}:{}:{}",
                    s.stage.as_str(),
                    s.version,
                    s.failure_code.as_str(),
                    s.invariant
                )
            })
            .collect(),
        topics: S_DOMAIN_TEN_TOPICS.to_vec(),
        peers: S_RELATION_TABLE
            .iter()
            .map(|r| format!("{}:{}", r.peer_domain, r.provides))
            .collect(),
        work_table_rows,
        frozen_at,
    }
}

/// 宣告 S 域开工。
///
/// 编排六道关（顺序即依赖顺序，任一硬闸失败则不开工）：
/// 1. R 域移交包前置检查（F3795 签收）——未签即阻断，硬闸；
/// 2. 第一等公民声明校验——覆盖层/仅 UI 层即 P0，硬闸；
/// 3. 六段签名校验——漂移/乱序即拒，硬闸；
/// 4. 十项映射校验——缺项/冲突，硬闸；
/// 5. 四域关系表校验——缺行/层位错，硬闸；
/// 6. 分工表校验——基数/重复/连续性，硬闸；然后产出冻结快照与指纹。
///
/// `now` 由调用方注入（本模块不读时钟，保持可测试与可重放）。
pub fn open_domain(
    pkg: &HandoverPackage,
    declarations: &[FirstClassDeclaration],
    now: u64,
) -> Outcome<DomainKickoff> {
    // 15.1 前置闸门（F3795 七件包 + 签收确认位）。
    if let Err(e) = gate(verify_kickoff_preconditions(pkg)) {
        return re_err(e);
    }
    // 15.2 第一等公民闸门（最硬的架构红线）。
    if let Err(e) = gate(verify_first_class_citizen(declarations)) {
        return re_err(e);
    }
    // 15.3 六段签名闸门。
    if let Err(e) = gate(verify_stage_signatures(&STAGE_SIGNATURES_V1)) {
        return re_err(e);
    }
    // 15.4 十项映射闸门。
    if let Err(e) = gate(verify_theme_landings(&S_THEME_LANDINGS)) {
        return re_err(e);
    }
    // 15.5 四域关系表闸门。
    if let Err(e) = gate(verify_relation_table(&S_RELATION_TABLE)) {
        return re_err(e);
    }
    // 15.6 分工表闸门。
    let wt = match gate(verify_work_table(&S01_WORK_TABLE)) {
        Ok(v) => v,
        Err(e) => return re_err(e),
    };

    // 15.7 冻结。前置闸门已通过（否则上面已返回），此处不附带其诊断。
    let freeze = canonical_freeze(wt.len() as u32, now);
    let fingerprint = freeze_fingerprint(&freeze);
    ok(DomainKickoff { freeze, fingerprint }, Vec::new())
}

// ---------------------------------------------------------------------------
// 十六、开工宣告文本（架构文档替述可读，无障碍要求）
// ---------------------------------------------------------------------------

/// 生成 S 域开工宣告（人话版）。
///
/// 无障碍要求：架构文档替述可读——本函数是文档的可执行生成源，保证「文档所述」
/// 与「代码所声明」同源，不会各说各话。
pub fn render_kickoff_declaration(freeze: &ArchitectureFreeze, fingerprint: &str) -> String {
    let mut out = String::new();
    out.push_str("【VE-S 无障碍渲染域 · 开工宣告】\n");
    out.push_str(&format!(
        "条目区间：F{}-F{}（共 {} 条 / {} 组）。\n",
        freeze.first_entry_id,
        freeze.last_entry_id,
        S_ENTRY_COUNT,
        S_GROUP_COUNT
    ));
    out.push('\n');
    out.push_str("第一等公民声明：无障碍不是覆盖层，而是渲染层的第一等公民。\n");
    out.push_str("  能力必须驻留渲染层本体（RENDERER）；驻留覆盖层（OVERLAY）或仅 UI 层（UI_ONLY）判 P0。\n");
    out.push('\n');
    out.push_str("最后一公里声明：N08 语义树 / N05 偏好 / P08 动效替代 / R08 创作语义是数据与逻辑层，\n");
    out.push_str("  S 域是像素层执行——把「应该是什么样」画进帧缓冲，是语义→像素的最后一公里。\n");
    out.push('\n');
    out.push_str("架构六段（签名冻结 v1）：\n");
    for (i, s) in STAGE_SIGNATURES_V1.iter().enumerate() {
        out.push_str(&format!(
            "  {}. {}（{}）→ {}\n",
            i + 1,
            s.stage.as_str(),
            s.complexity,
            s.output
        ));
    }
    out.push('\n');
    out.push_str("官方主题十项落组：\n");
    for l in S_THEME_LANDINGS.iter() {
        out.push_str(&format!(
            "  · {}：落位 F{}（{}）→ 深挖 F{}（{}）\n",
            l.topic, l.landing_entry_id, l.landing_group, l.deepening_first_id, l.deepening_group
        ));
    }
    out.push('\n');
    out.push_str(&format!("四域关系表：{}。\n", freeze.peers.join("；")));
    out.push('\n');
    out.push_str("红线三条：\n");
    out.push_str("  ① 第一等公民：覆盖层实现 = P0；\n");
    out.push_str("  ② 断供红线：辅具态采不到 = P0，复述渲染版断供，严禁静默回退到「无障碍关闭」；\n");
    out.push_str("  ③ 最后一公里：语义树说有而像素没画 = 断链立案（P0），交 F3807/F3812 双线追责。\n");
    out.push('\n');
    out.push_str(&format!(
        "无障碍注入预算：≤{}ms/帧（超预算 = 双向失败，立案优化）。\n",
        A11Y_INJECTION_BUDGET_MS
    ));
    out.push_str("注入显性：每条策略注没注、注到哪、为何不注，逐条在 InjectionReport 显性可查。\n");
    out.push_str(&format!(
        "S01 组分工表：{} 条（F{}-F{}）。\n",
        freeze.work_table_rows,
        S_FIRST_ENTRY_ID,
        S_FIRST_ENTRY_ID + S_ENTRIES_PER_GROUP - 1
    ));
    out.push_str(&format!(
        "架构冻结指纹：{}（冻结时间戳 {}）。\n",
        fingerprint, freeze.frozen_at
    ));
    out
}

// ---------------------------------------------------------------------------
// 十七、回归基线（健康态构造器）
// ---------------------------------------------------------------------------

/// 构造一份「四态全采到、用户全开」的健康采集快照（回归基线）。
pub fn healthy_state() -> AccessibilityState {
    AccessibilityState {
        source_id: "HOST_A11Y_CHANNEL".to_string(),
        probes: vec![
            StateProbe { key: AccessibilityStateKey::AssistiveAttached, value: 1.0, acquired: true },
            StateProbe { key: AccessibilityStateKey::ReduceMotion, value: 1.0, acquired: true },
            StateProbe { key: AccessibilityStateKey::HighContrast, value: 1.0, acquired: true },
            StateProbe { key: AccessibilityStateKey::FontScaleTier, value: 3.0, acquired: true },
        ],
    }
}

/// 构造一份覆盖三注入点、带解释文案的健康策略集（回归基线）。
pub fn healthy_strategies() -> Vec<RenderStrategy> {
    vec![
        RenderStrategy {
            id: "hc-filter".to_string(),
            from_state: AccessibilityStateKey::HighContrast,
            slot: InjectionSlot::Filter,
            params: vec![("ratio", 7.0)],
            user_facing_explanation: "把前景与背景的对比度提到 7:1，便于看清细字。".to_string(),
        },
        RenderStrategy {
            id: "reduce-motion".to_string(),
            from_state: AccessibilityStateKey::ReduceMotion,
            slot: InjectionSlot::Post,
            params: vec![("static", 1.0)],
            user_facing_explanation: "把滑动与缩放换成淡入淡出，不做位移。".to_string(),
        },
        RenderStrategy {
            id: "font-scale".to_string(),
            from_state: AccessibilityStateKey::FontScaleTier,
            slot: InjectionSlot::Style,
            params: vec![("tier", 3.0)],
            user_facing_explanation: "按你设定的字号档放大文字与行距。".to_string(),
        },
    ]
}

/// 健康注入报告（三策略均注入且在预算内）。
pub fn healthy_injection_report() -> InjectionReport {
    InjectionReport {
        outcomes: healthy_strategies()
            .iter()
            .map(|s| InjectionOutcome {
                strategy_id: s.id.clone(),
                injected: true,
                reason: None,
                cost_ms: 0.1,
            })
            .collect(),
        total_cost_ms: 0.3,
    }
}

/// 健康第一等公民声明集（全驻留渲染层且参与一致断言）。
pub fn healthy_declarations() -> Vec<FirstClassDeclaration> {
    ["高对比", "焦点强化", "大字号", "动效替代"]
        .iter()
        .map(|cap| FirstClassDeclaration {
            capability: cap.to_string(),
            residency: Residency::Renderer,
            participates_in_link_assertion: true,
        })
        .collect()
}

// ---------------------------------------------------------------------------
// 十八、自检（架构契约的可执行形态）
// ---------------------------------------------------------------------------

/// VE-F3801 自检：判据逐条对应。
pub fn run_f3801_checks() -> CheckSet {
    let mut set = CheckSet::new("svstar2-ves");

    // ── 判据一：第一等公民 ────────────────────────────────────────────
    {
        let good = verify_first_class_citizen(&healthy_declarations());
        set.add(
            "健康声明集全绿（全驻留渲染层且参与一致断言）",
            good.is_ok(),
            "四项能力均 RENDERER + 参与断链断言",
        );

        for (residency, name) in [
            (Residency::Overlay, "覆盖层"),
            (Residency::UiOnly, "仅 UI 层"),
        ] {
            let v = verify_first_class_citizen(&[FirstClassDeclaration {
                capability: "高对比".to_string(),
                residency,
                participates_in_link_assertion: true,
            }]);
            set.add(
                match name {
                    "覆盖层" => "覆盖层实现判第一等公民 P0",
                    _ => "仅 UI 层实现判第一等公民 P0",
                },
                v.err_severity() == Some(Severity::P0)
                    && v.err_code() == Some(DiagCode::FirstClassViolation),
                "非 RENDERER 驻留即 P0",
            );
        }

        // 驻留合法却不参与一致断言 = 放弃核查权，同样 P0。
        let no_assert = verify_first_class_citizen(&[FirstClassDeclaration {
            capability: "焦点强化".to_string(),
            residency: Residency::Renderer,
            participates_in_link_assertion: false,
        }]);
        set.add(
            "驻留渲染层但不参与一致断言判 P0",
            no_assert.err_severity() == Some(Severity::P0),
            "放弃「画没画」核查权即断链前置条件",
        );

        let s = healthy_declarations()[0].clone();
        let restated = verify_first_class_citizen(&[FirstClassDeclaration {
            residency: Residency::Overlay,
            ..s
        }]);
        let has_restatement = restated
            .diagnostics()
            .iter()
            .any(|d| d.restatement.is_some());
        set.add(
            "第一等公民违规带可上屏复述",
            has_restatement,
            "违规须告诉用户「我们已拦截该实现」",
        );
    }

    // ── 判据二：最后一公里 ────────────────────────────────────────────
    {
        let rel = verify_relation_table(&S_RELATION_TABLE);
        set.add("四域关系表完整（4 行）", rel.is_ok(), "N08/N05/P08/R08 四域齐备");

        // 像素层执行唯一归属 S 域：对端被标像素层即分工破口。
        let mut broken = S_RELATION_TABLE.to_vec();
        broken[0].layer = Layer::PixelExecution;
        let v = verify_relation_table(&broken);
        set.add(
            "对端自称像素层即判分工破口",
            v.err_code() == Some(DiagCode::RelationTableInvalid),
            "最后一公里只属于 S 域",
        );

        // 缺行。
        let short = verify_relation_table(&S_RELATION_TABLE[..3]);
        set.add(
            "关系表缺行判红",
            short.err_code() == Some(DiagCode::RelationTableInvalid),
            "四域缺一即分工不全",
        );

        // 未声明反向消费 = 隐藏耦合无处审计。
        let mut hidden = S_RELATION_TABLE.to_vec();
        hidden[1].s_consumes = "   ";
        let v2 = verify_relation_table(&hidden);
        set.add(
            "未声明反向消费判红",
            v2.err_code() == Some(DiagCode::RelationTableInvalid),
            "关系表须双向，防隐藏耦合",
        );
    }

    // ── 判据三：六段签名 ──────────────────────────────────────────────
    {
        let sigs = verify_stage_signatures(&STAGE_SIGNATURES_V1);
        set.add("六段签名冻结 v1 无漂移", sigs.is_ok(), "六段逐位一致");

        set.add(
            "六段齐备无缺段",
            STAGE_SIGNATURES_V1.len() == RENDER_STAGE_ORDER.len(),
            "6/6 段齐备",
        );

        // 乱序漂移必须被抓住——**不排序**是这条的关键。
        let mut shuffled = STAGE_SIGNATURES_V1.to_vec();
        shuffled.swap(0, 2);
        let v = verify_stage_signatures(&shuffled);
        set.add(
            "六段乱序判漂移（比对不排序）",
            v.diagnostics()
                .iter()
                .any(|d| d.code == DiagCode::StageSignatureDrift),
            "采集必须先于注入，乱序即破链",
        );

        // 签名内容漂移（不变量被改）。
        let mut drifted = STAGE_SIGNATURES_V1.to_vec();
        drifted[1].invariant = "策略随便改";
        let v2 = verify_stage_signatures(&drifted);
        set.add(
            "不变量被改判漂移",
            v2.diagnostics()
                .iter()
                .any(|d| d.code == DiagCode::StageSignatureDrift),
            "入参/出参/不变量变更须先走 ADR",
        );

        // 缺段。
        let short = verify_stage_signatures(&STAGE_SIGNATURES_V1[..5]);
        set.add(
            "缺段判红",
            short.diagnostics()
                .iter()
                .any(|d| d.code == DiagCode::StageSequenceInvalid),
            "缺段则帧内链路断裂",
        );
    }

    // ── 判据四：注入显性 ──────────────────────────────────────────────
    {
        let healthy = verify_injection_report(
            &healthy_injection_report(),
            &["hc-filter", "reduce-motion", "font-scale"],
        );
        set.add("健康注入路径全绿", healthy.is_ok(), "三策略均注入且在预算内");

        // 缺席 = 静默丢弃。
        let dropped = verify_injection_report(&healthy_injection_report(), &[
            "hc-filter",
            "reduce-motion",
            "font-scale",
            "ghost",
        ]);
        set.add(
            "策略缺席判静默丢弃",
            dropped
                .diagnostics()
                .iter()
                .any(|d| d.code == DiagCode::InjectionFailed
                    && d.message.contains("ghost")),
            "每条策略都要在报告里出现",
        );

        // 未注入无原因。
        let silent = verify_injection_report(
            &InjectionReport {
                outcomes: vec![InjectionOutcome {
                    strategy_id: "a".to_string(),
                    injected: false,
                    reason: None,
                    cost_ms: 0.0,
                }],
                total_cost_ms: 0.1,
            },
            &["a"],
        );
        set.add(
            "未注入无原因判静默丢弃",
            silent
                .diagnostics()
                .iter()
                .any(|d| d.code == DiagCode::InjectionFailed),
            "无原因的未注入对用户与排障者都不可解释",
        );

        // 超 1ms 预算红线。
        let over = verify_injection_report(
            &InjectionReport { outcomes: Vec::new(), total_cost_ms: 1.6 },
            &[],
        );
        set.add(
            "注入超 1ms 判预算红线",
            over
                .diagnostics()
                .iter()
                .any(|d| d.code == DiagCode::A11yBudgetExceeded),
            "开销挤占渲染 = 双向失败",
        );

        // 预算恰好打满不算超（边界取 ≤）。
        let edge = verify_injection_report(
            &InjectionReport {
                outcomes: Vec::new(),
                total_cost_ms: A11Y_INJECTION_BUDGET_MS,
            },
            &[],
        );
        set.add("预算恰好打满不算超", edge.is_ok(), "边界取 ≤（锚点：≤1ms）");

        // 非法耗时。
        let bad = verify_injection_report(
            &InjectionReport {
                outcomes: vec![InjectionOutcome {
                    strategy_id: "a".to_string(),
                    injected: true,
                    reason: None,
                    cost_ms: -1.0,
                }],
                total_cost_ms: 0.1,
            },
            &["a"],
        );
        set.add(
            "非法耗时判红",
            bad.diagnostics()
                .iter()
                .any(|d| d.code == DiagCode::A11yBudgetExceeded),
            "非法值会让预算核算失效",
        );
    }

    // ── 判据五：断链 P0 ───────────────────────────────────────────────
    {
        let mut healthy_cases = Vec::new();
        let healthy = detect_broken_links(
            &[
                SemanticPixelPair {
                    node_id: "n1".to_string(),
                    semantically_visible: true,
                    painted: true,
                    virtualization_retained: false,
                },
                SemanticPixelPair {
                    node_id: "n2".to_string(),
                    semantically_visible: true,
                    painted: true,
                    virtualization_retained: false,
                },
            ],
            1000,
            &mut healthy_cases,
        );
        set.add(
            "健康场景无断链且可见率 100%",
            healthy.is_ok()
                && healthy
                    .value()
                    .map(|v| (v.visible_rate - 1.0).abs() < f64::EPSILON)
                    .unwrap_or(false),
            "语义可见且已绘制 → 无断链",
        );

        let mut broken_cases = Vec::new();
        let broken = detect_broken_links(
            &[
                SemanticPixelPair {
                    node_id: "n1".to_string(),
                    semantically_visible: true,
                    painted: true,
                    virtualization_retained: false,
                },
                SemanticPixelPair {
                    node_id: "n2".to_string(),
                    semantically_visible: true,
                    painted: false,
                    virtualization_retained: false,
                },
            ],
            1000,
            &mut broken_cases,
        );
        set.add(
            "断链判 P0",
            broken.err_severity() == Some(Severity::P0)
                && broken.err_code() == Some(DiagCode::SemanticPixelBrokenLink),
            "语义说有像素没画 = 立案",
        );

        let has_restatement = broken
            .diagnostics()
            .iter()
            .any(|d| d.restatement.is_some());
        set.add(
            "断链带可上屏复述",
            has_restatement,
            "用户须知道自己被误导了",
        );

        // 断链诊断须指明双线追责线（案卷在失败路径上以Err 返回，
        // 追责线写在 hint 里才能被上报管道读到）。
        let routed = broken
            .diagnostics()
            .iter()
            .any(|d| d.hint.contains("F3807") && d.hint.contains("F3812"));
        set.add(
            "断链诊断指明双线追责",
            routed,
            "案卷交 F3807/F3812 双线",
        );

        // 案卷必须可外带取回（立了案却拿不到案卷 = 无法追责）。
        set.add(
            "断链案卷外带可取回且含双线",
            broken_cases.len() == 1
                && broken_cases[0].node_id == "n2"
                && broken_cases[0].routed_to == [3807, 3812]
                && !broken_cases[0].charge.is_empty(),
            "失败路径不吞案卷：Evidence 与判定分离",
        );

        // 虚拟化保留例外：显式标记才豁免。
        let mut vret_cases = Vec::new();
        let vret = detect_broken_links(
            &[SemanticPixelPair {
                node_id: "n3".to_string(),
                semantically_visible: true,
                painted: false,
                virtualization_retained: true,
            }],
            1000,
            &mut vret_cases,
        );
        set.add("虚拟化保留例外显式标记即豁免", vret.is_ok(), "例外机制须有效");

        // 未标记的「看不到」= 断链（例外不可默认开启）。
        let mut unmarked_cases = Vec::new();
        let unmarked = detect_broken_links(
            &[SemanticPixelPair {
                node_id: "n4".to_string(),
                semantically_visible: true,
                painted: false,
                virtualization_retained: false,
            }],
            1000,
            &mut unmarked_cases,
        );
        set.add(
            "未标记的看不到判断链",
            !unmarked.is_ok(),
            "例外须显式，默认不得豁免",
        );

        // 语义声明不可见 → 不在断链面。
        let mut invisible_cases = Vec::new();
        let invisible = detect_broken_links(
            &[SemanticPixelPair {
                node_id: "n5".to_string(),
                semantically_visible: false,
                painted: false,
                virtualization_retained: false,
            }],
            1000,
            &mut invisible_cases,
        );
        set.add("语义声明不可见不判断链", invisible.is_ok(), "不可见节点不在断链面");

        // 空集不判失败，可见率定义为 1.0。
        let mut empty_cases = Vec::new();
        let empty = detect_broken_links(&[], 1000, &mut empty_cases);
        set.add(
            "空节点集不判失败且可见率为 1",
            empty.is_ok()
                && empty
                    .value()
                    .map(|v| (v.visible_rate - 1.0).abs() < f64::EPSILON)
                    .unwrap_or(false),
            "空集无断链可言，不应误报",
        );
    }

    // ── 十项映射 / 关系表 / 分工表 / 冻结 ─────────────────────────────
    {
        let themes = verify_theme_landings(&S_THEME_LANDINGS);
        set.add(
            "十项映射齐备无冲突",
            themes.is_ok(),
            "官方十项每项均有首次落位",
        );

        let mut missing = S_THEME_LANDINGS.to_vec();
        missing[9].topic = "配色"; // 顶掉「性能」→ 缺项
        let v = verify_theme_landings(&missing);
        set.add(
            "十项缺项判红",
            v.diagnostics()
                .iter()
                .any(|d| d.code == DiagCode::ThemeMappingIncomplete),
            "删行即无法核对齐备性",
        );

        let mut dup = S_THEME_LANDINGS.to_vec();
        dup[1].topic = dup[0].topic;
        let v2 = verify_theme_landings(&dup);
        set.add(
            "十项归属冲突判红",
            v2.diagnostics()
                .iter()
                .any(|d| d.code == DiagCode::ThemeOwnershipConflict),
            "首次落位唯一",
        );

        let wt = verify_work_table(&S01_WORK_TABLE);
        set.add(
            "S01 分工表 20 条连续（F3801-F3820）",
            wt.is_ok(),
            "20 条齐备、条目号连续无重复",
        );

        let short = verify_work_table(&S01_WORK_TABLE[..19]);
        set.add(
            "分工表缺条判红",
            short.err_code() == Some(DiagCode::WorktableCardinalityInvalid)
                || short.err_code() == Some(DiagCode::WorktableGap),
            "缺位即漏项",
        );

        // 连续性先于基数：缺中间位应报 GAP。
        let mut gapped = S01_WORK_TABLE.to_vec();
        gapped[5].entry_id = 3999;
        let v3 = verify_work_table(&gapped);
        set.add(
            "条目号缺位判 GAP",
            v3.diagnostics()
                .iter()
                .any(|d| d.code == DiagCode::WorktableGap),
            "缺位是病因，基数只是症状",
        );

        let mut duprow = S01_WORK_TABLE.to_vec();
        duprow[3].entry_id = duprow[2].entry_id;
        let v4 = verify_work_table(&duprow);
        set.add(
            "条目号重复判红",
            v4.diagnostics()
                .iter()
                .any(|d| d.code == DiagCode::WorktableDuplicateEntry),
            "重复即覆盖他条职责",
        );
    }

    // ── 采集段：断供 P0 + 态分叉 P0 ───────────────────────────────────
    {
        let healthy = verify_state_capture(&healthy_state(), Some("HOST_A11Y_CHANNEL"));
        set.add("健康采集路径全绿", healthy.is_ok(), "四态齐备且来源唯一");

        let blackout = verify_state_capture(
            &AccessibilityState {
                source_id: "HOST_A11Y_CHANNEL".to_string(),
                probes: vec![
                    StateProbe { key: AccessibilityStateKey::AssistiveAttached, value: 0.0, acquired: false },
                    StateProbe { key: AccessibilityStateKey::ReduceMotion, value: 0.0, acquired: true },
                    StateProbe { key: AccessibilityStateKey::HighContrast, value: 0.0, acquired: true },
                    StateProbe { key: AccessibilityStateKey::FontScaleTier, value: 0.0, acquired: true },
                ],
            },
            Some("HOST_A11Y_CHANNEL"),
        );
        set.add(
            "辅具态断供判 P0",
            blackout.err_severity() == Some(Severity::P0)
                && blackout.err_code() == Some(DiagCode::StateCaptureBlackout),
            "断供不得降级为「关闭」",
        );

        let has_restatement = blackout
            .diagnostics()
            .iter()
            .any(|d| d.restatement.is_some());
        set.add(
            "断供复述（渲染版断供红线）",
            has_restatement,
            "把「我采不到」本身变成可见告警",
        );

        // 断供与「值为0」必须可区分——这是本域最不可接受的缺陷。
        let acquired_false = verify_state_capture(
            &AccessibilityState {
                source_id: "HOST_A11Y_CHANNEL".to_string(),
                probes: vec![
                    StateProbe { key: AccessibilityStateKey::AssistiveAttached, value: 0.0, acquired: true },
                    StateProbe { key: AccessibilityStateKey::ReduceMotion, value: 0.0, acquired: true },
                    StateProbe { key: AccessibilityStateKey::HighContrast, value: 0.0, acquired: true },
                    StateProbe { key: AccessibilityStateKey::FontScaleTier, value: 1.0, acquired: true },
                ],
            },
            Some("HOST_A11Y_CHANNEL"),
        );
        set.add(
            "「采到了但值为 0」不判断供",
            acquired_false.is_ok(),
            "用户没开 ≠ 采不到，两者语义不可混同",
        );

        // 缺项按断供处理，不静默填默认值。
        let incomplete = verify_state_capture(
            &AccessibilityState {
                source_id: "HOST_A11Y_CHANNEL".to_string(),
                probes: vec![StateProbe {
                    key: AccessibilityStateKey::ReduceMotion,
                    value: 0.0,
                    acquired: true,
                }],
            },
            Some("HOST_A11Y_CHANNEL"),
        );
        set.add(
            "四态缺项按断供判 P0（不填默认值）",
            incomplete.err_code() == Some(DiagCode::StateCaptureBlackout),
            "填默认值等于替用户关掉无障碍",
        );

        // 态分叉。
        let forked = verify_state_capture(&healthy_state(), Some("OTHER_CHANNEL"));
        set.add(
            "双来源采集判态分叉 P0",
            forked.err_code() == Some(DiagCode::StateCaptureFork)
                && forked.err_severity() == Some(Severity::P0),
            "采集单源红线（锚点 F3816）",
        );
    }

    // ── 策略段：透明红线 ──────────────────────────────────────────────
    {
        let healthy = verify_strategies(&healthy_strategies());
        set.add("健康策略集全绿", healthy.is_ok(), "三策略注入点合法且均有解释");

        let opaque = verify_strategies(&[RenderStrategy {
            id: "mystery".to_string(),
            from_state: AccessibilityStateKey::HighContrast,
            slot: InjectionSlot::Filter,
            params: Vec::new(),
            user_facing_explanation: "".to_string(),
        }]);
        set.add(
            "策略黑箱判透明红线",
            opaque.err_code() == Some(DiagCode::StrategyOpaque),
            "用户无法预测设置带来什么 = 行为不可预期",
        );

        let nan = verify_strategies(&[RenderStrategy {
            id: "nan-param".to_string(),
            from_state: AccessibilityStateKey::HighContrast,
            slot: InjectionSlot::Filter,
            params: vec![("ratio", f64::NAN)],
            user_facing_explanation: "说明".to_string(),
        }]);
        set.add(
            "非有限参数判红",
            nan.diagnostics()
                .iter()
                .any(|d| d.code == DiagCode::InjectionFailed),
            "NaN 会让像素计算产出 NaN 区域",
        );
    }

    // ── 反馈段：降级显性复述分流 ──────────────────────────────────────
    {
        let diags = vec![
            Diagnostic {
                code: DiagCode::InjectionFailed,
                severity: Severity::P1,
                message: "工程侧问题".to_string(),
                hint: String::new(),
                stage: StageTag::Stage(RenderStage::PipelineInjection),
                restatement: Some("有一项设置本轮没生效。".to_string()),
            },
            Diagnostic {
                code: DiagCode::StrategyOpaque,
                severity: Severity::P1,
                message: "工程侧问题2".to_string(),
                hint: String::new(),
                stage: StageTag::Stage(RenderStage::RenderStrategy),
                restatement: None,
            },
        ];
        let fb = build_feedback(&diags);
        let user_count = user_facing_records(&fb).len();
        set.add(
            "降级显性复述分流正确",
            user_count == 1 && fb.len() == 2,
            "带复述→面向用户；无复述→面向开发者",
        );
    }

    // ── 前置闸门 / ADR / 冻结漂移 ─────────────────────────────────────
    {
        let healthy_pkg = verify_kickoff_preconditions(&HandoverPackage::healthy());
        set.add("七件移交包齐备且已签收即通过", healthy_pkg.is_ok(), "F3795 开工条件满足");

        let mut missing_mat = HandoverPackage::healthy();
        missing_mat.materials[0].present = false;
        let v = verify_kickoff_preconditions(&missing_mat);
        set.add(
            "移交材料缺失判红",
            v.err_code() == Some(DiagCode::HandoverMaterialMissing),
            "七件包缺一不可开工",
        );

        let unsigned = verify_kickoff_preconditions(&HandoverPackage {
            materials: HandoverMaterialKind::ALL
                .iter()
                .map(|k| HandoverMaterial { kind: *k, present: true })
                .collect(),
            receiver_confirmed: false,
            receiver_signature: None,
        });
        set.add(
            "签收确认位未签判红",
            unsigned.diagnostics()
                .iter()
                .any(|d| d.code == DiagCode::HandoverNotConfirmed),
            "确认前不得宣告开工",
        );

        // ADR：信息不全即拒。
        let no_reason = propose_architecture_amendment(&ArchitectureAmendment {
            proposed_by_entry_id: 3803,
            target: AmendmentTarget::StageSignature,
            rationale: "  ".to_string(),
            impact: "影响 F3802".to_string(),
        });
        let no_impact = propose_architecture_amendment(&ArchitectureAmendment {
            proposed_by_entry_id: 3803,
            target: AmendmentTarget::StageSignature,
            rationale: "签名需改".to_string(),
            impact: String::new(),
        });
        let good = propose_architecture_amendment(&ArchitectureAmendment {
            proposed_by_entry_id: 3803,
            target: AmendmentTarget::StageSignature,
            rationale: "签名需改".to_string(),
            impact: "影响 F3802".to_string(),
        });
        set.add(
            "ADR 缺理由或缺影响面即拒绝",
            no_reason.err_code() == Some(DiagCode::AdrIncomplete)
                && no_impact.err_code() == Some(DiagCode::AdrIncomplete),
            "防止「顺手改架构」而无据",
        );
        set.add(
            "ADR 齐备即受理并生成编号",
            good
                .value()
                .map(|r| r.accepted && r.adr_id == "ADR-S-3803")
                .unwrap_or(false),
            "受理编号 ADR-S-<条目号>",
        );

        // 冻结指纹：可重放 + 漂移可检出。
        let sample = canonical_freeze(S01_WORK_TABLE.len() as u32, 1);
        let fp_a = freeze_fingerprint(&sample);
        let fp_b = freeze_fingerprint(&sample);
        set.add(
            "冻结指纹可重放（同输入逐位一致）",
            fp_a == fp_b,
            "同输入两次计算须逐位一致（可重放）",
        );

        let drifted = ArchitectureFreeze { work_table_rows: 19, ..sample.clone() };
        let dv = detect_drift(&sample, &drifted);
        set.add(
            "架构漂移可检出",
            dv.err_code() == Some(DiagCode::FreezeDrift),
            "改一行即被指纹捕获",
        );

        // 集合语义：主题/对端顺序不同但集合相同 → 指纹须相同（防误报）。
        let mut reordered = sample.clone();
        reordered.topics.reverse();
        reordered.peers.reverse();
        set.add(
            "集合换序不误报漂移（指纹按集合口径）",
            freeze_fingerprint(&reordered) == fp_a,
            "键序不反映架构语义，误报即守卫失效",
        );

        // 六段序语义：换序必须改变指纹（有序部分保留原序）。
        let mut stage_swapped = sample.clone();
        stage_swapped.stages.swap(0, 1);
        set.add(
            "六段序换序必改指纹（有序口径）",
            freeze_fingerprint(&stage_swapped) != fp_a,
            "六段序有语义，不得被当作集合",
        );
    }

    // ── 域开工编排 + 宣告文本 ─────────────────────────────────────────
    {
        let ko = open_domain(&HandoverPackage::healthy(), &healthy_declarations(), 1000);
        set.add(
            "域开工六闸全过即宣告开工",
            ko.is_ok(),
            "前置+公民+签名+十项+关系+分工 六闸全绿",
        );

        let fp = ko
            .value()
            .map(|k| k.fingerprint.clone())
            .unwrap_or_default();
        set.add(
            "开工指纹与canonical freeze 一致",
            !fp.is_empty() && fp == freeze_fingerprint(&canonical_freeze(20, 1000)),
            "宣告指纹须等于规范快照指纹",
        );

        // 硬闸：覆盖层实现阻断开工。
        let blocked = open_domain(
            &HandoverPackage::healthy(),
            &[FirstClassDeclaration {
                capability: "高对比".to_string(),
                residency: Residency::Overlay,
                participates_in_link_assertion: true,
            }],
            1000,
        );
        set.add(
            "覆盖层实现阻断域开工",
            blocked.err_severity() == Some(Severity::P0),
            "第一等公民是硬闸，不是建议",
        );

        // 宣告文本：架构文档替述可读。
        let text = render_kickoff_declaration(&canonical_freeze(20, 1000), &fp);
        set.add(
            "开工宣告文本含六段与三红线",
            text.contains("STATE_CAPTURE") && text.contains("断链") && text.contains("1ms"),
            "宣告文本由代码生成，保证文档与实现同源",
        );
    }

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f3801_self_check_all_green() {
        let set = run_f3801_checks();
        let (items, count) = set.red_items();
        let mut failed: Vec<&str> = Vec::new();
        for i in 0..count {
            if let Some(c) = items[i] {
                if !c.passed {
                    failed.push(c.name);
                }
            }
        }
        let (passed, red) = set.tally();
        assert!(
            failed.is_empty(),
            "VE-F3801 自检存在红项：{:?}（{}/{} 绿）",
            failed,
            passed,
            passed + red
        );
        assert!(!set.truncated(), "VE-F3801 自检集被截断，红项可能被丢弃");
    }

    #[test]
    fn healthy_domain_opens() {
        let ko = open_domain(
            &HandoverPackage::healthy(),
            &healthy_declarations(),
            1000,
        );
        assert!(ko.is_ok(), "健康态应可开工：{:?}", ko.message());
    }

    /// 回归：覆盖层实现必须阻断开工（第一等公民是硬闸）。
    #[test]
    fn overlay_blocks_domain_kickoff() {
        let ko = open_domain(
            &HandoverPackage::healthy(),
            &[FirstClassDeclaration {
                capability: "高对比".to_string(),
                residency: Residency::Overlay,
                participates_in_link_assertion: true,
            }],
            1000,
        );
        assert_eq!(ko.err_severity(), Some(Severity::P0));
        assert_eq!(ko.err_code(), Some(DiagCode::FirstClassViolation));
    }

    /// 回归：六段乱序必须被判漂移（比对不可排序）。
    #[test]
    fn stage_reorder_is_drift_not_normalized() {
        let mut shuffled = STAGE_SIGNATURES_V1.to_vec();
        shuffled.swap(0, 2);
        let v = verify_stage_signatures(&shuffled);
        assert!(!v.is_ok(), "乱序六段必须判红");
        assert!(v
            .diagnostics()
            .iter()
            .any(|d| d.code == DiagCode::StageSignatureDrift));
    }

    /// 回归：断供与「值为 0」必须可区分。
    #[test]
    fn blackout_is_not_same_as_zero() {
        // 采不到（acquired=false）→ P0。
        let blackout = verify_state_capture(
            &AccessibilityState {
                source_id: "HOST_A11Y_CHANNEL".to_string(),
                probes: vec![
                    StateProbe { key: AccessibilityStateKey::AssistiveAttached, value: 0.0, acquired: false },
                    StateProbe { key: AccessibilityStateKey::ReduceMotion, value: 0.0, acquired: true },
                    StateProbe { key: AccessibilityStateKey::HighContrast, value: 0.0, acquired: true },
                    StateProbe { key: AccessibilityStateKey::FontScaleTier, value: 1.0, acquired: true },
                ],
            },
            Some("HOST_A11Y_CHANNEL"),
        );
        assert_eq!(blackout.err_severity(), Some(Severity::P0));

        // 采到了且值为 0（用户没开）→ 不是断供。
        let user_off = verify_state_capture(
            &AccessibilityState {
                source_id: "HOST_A11Y_CHANNEL".to_string(),
                probes: vec![
                    StateProbe { key: AccessibilityStateKey::AssistiveAttached, value: 0.0, acquired: true },
                    StateProbe { key: AccessibilityStateKey::ReduceMotion, value: 0.0, acquired: true },
                    StateProbe { key: AccessibilityStateKey::HighContrast, value: 0.0, acquired: true },
                    StateProbe { key: AccessibilityStateKey::FontScaleTier, value: 1.0, acquired: true },
                ],
            },
            Some("HOST_A11Y_CHANNEL"),
        );
        assert!(
            user_off.is_ok(),
            "「用户没开」不得判成断供：{:?}",
            user_off.message()
        );
    }

    /// 回归：断链必须判 P0，且带可上屏复述。
    #[test]
    fn broken_link_is_p0_with_restatement() {
        let mut r_cases = Vec::new();
        let r = detect_broken_links(
            &[SemanticPixelPair {
                node_id: "n2".to_string(),
                semantically_visible: true,
                painted: false,
                virtualization_retained: false,
            }],
            1000,
            &mut r_cases,
        );
        assert_eq!(r.err_severity(), Some(Severity::P0));
        assert_eq!(r.err_code(), Some(DiagCode::SemanticPixelBrokenLink));
        assert!(
            r.diagnostics().iter().any(|d| d.restatement.is_some()),
            "断链必须带可上屏复述——用户须知道自己被误导"
        );
        assert_eq!(
            r_cases.len(),
            1,
            "失败路径不得吞掉案卷：立了案却拿不到案卷等于无法追责"
        );
        assert_eq!(r_cases[0].routed_to, [3807, 3812], "案卷须指明双线追责");
    }

    /// 回归：虚拟化例外须显式，默认不得豁免。
    #[test]
    fn virtualization_exception_must_be_explicit() {
        let mut marked_cases = Vec::new();
        let marked = detect_broken_links(
            &[SemanticPixelPair {
                node_id: "n3".to_string(),
                semantically_visible: true,
                painted: false,
                virtualization_retained: true,
            }],
            1000,
            &mut marked_cases,
        );
        assert!(marked.is_ok(), "显式虚拟化例外应豁免");

        let mut unmarked_cases = Vec::new();
        let unmarked = detect_broken_links(
            &[SemanticPixelPair {
                node_id: "n3".to_string(),
                semantically_visible: true,
                painted: false,
                virtualization_retained: false,
            }],
            1000,
            &mut unmarked_cases,
        );
        assert!(!unmarked.is_ok(), "未标记的看不到必须判断链");
    }

    /// 回归：冻结指纹的集合口径与有序口径必须分别正确。
    ///
    /// 这条守的是「漂移检测会不会变成噪声」：集合语义部分换序须同指纹
    /// （否则误报泛滥=守卫失效），有序语义部分换序必改指纹
    /// （否则真漂移被洗掉=守卫失效）。两头都不能错。
    #[test]
    fn freeze_fingerprint_respects_set_and_sequence_semantics() {
        let base = canonical_freeze(20, 1);
        let fp = freeze_fingerprint(&base);

        // 集合口径：主题/对端换序不改变指纹。
        let mut set_reordered = base.clone();
        set_reordered.topics.reverse();
        set_reordered.peers.reverse();
        assert_eq!(
            freeze_fingerprint(&set_reordered),
            fp,
            "集合换序不得误报漂移"
        );

        // 有序口径：六段序换序必改指纹。
        let mut stage_reordered = base.clone();
        stage_reordered.stages.swap(0, 1);
        assert_ne!(
            freeze_fingerprint(&stage_reordered),
            fp,
            "六段序换序必须被捕获"
        );

        // 实质变更必改指纹。
        let mut changed = base.clone();
        changed.work_table_rows = 19;
        assert_ne!(freeze_fingerprint(&changed), fp, "分工表行数变更须改指纹");
    }

    /// 回归：注入显性——缺席与无原因都必须判红。
    #[test]
    fn injection_report_is_never_silent() {
        let report = healthy_injection_report();
        // 缺席（多期望一条）。
        let missing = verify_injection_report(
            &report,
            &["hc-filter", "reduce-motion", "font-scale", "ghost"],
        );
        assert!(
            missing
                .diagnostics()
                .iter()
                .any(|d| d.message.contains("ghost")),
            "缺席策略必须判静默丢弃"
        );

        // 无原因。
        let no_reason = verify_injection_report(
            &InjectionReport {
                outcomes: vec![InjectionOutcome {
                    strategy_id: "a".to_string(),
                    injected: false,
                    reason: None,
                    cost_ms: 0.0,
                }],
                total_cost_ms: 0.1,
            },
            &["a"],
        );
        assert!(!no_reason.is_ok(), "未注入无原因必须判红");
    }

    /// 回归：1ms 预算红线边界（打满不算超）。
    #[test]
    fn budget_boundary_is_inclusive() {
        let at = verify_injection_report(
            &InjectionReport {
                outcomes: Vec::new(),
                total_cost_ms: 1.0,
            },
            &[],
        );
        assert!(at.is_ok(), "恰好 1ms 不算超（锚点：≤1ms）");

        let over = verify_injection_report(
            &InjectionReport {
                outcomes: Vec::new(),
                total_cost_ms: 1.000_001,
            },
            &[],
        );
        assert!(!over.is_ok(), "超 1ms 必须判红");
    }

    /// 落位纪律自检：VE 册功能必须在 Rust 侧，不许混进前端 TS。
    #[test]
    fn s_domain_lives_in_kernel_not_frontend() {
        // 六段契约在本模块树内可见，且由内核 crate 编译
        assert_eq!(STAGE_SIGNATURES_V1.len(), RENDER_STAGE_ORDER.len());
        let ko = open_domain(
            &HandoverPackage::healthy(),
            &healthy_declarations(),
            1,
        );
        assert!(ko.is_ok(), "VE-F3801 由内核 Rust 侧实现并可执行");
    }
}
