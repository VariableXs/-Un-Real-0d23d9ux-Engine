//! VE-F3006 · 动效组件化（VE-P 域 · 转场与动效编排 · 组件化组）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3006`
//!
//! **判据（锚点原文）**：四件套、三族注册、参数钳制、注册要件红线、沙箱扩展、判据。
//!
//! **职责定位（锚点原文）**：动效组件模型——可复用动效封装：组件=触发条件+编排图+参数默认值+
//! 无障碍行为四件套声明式定义——组件清单（入场系列/转场系列/反馈系列三族注册表）；组件接口
//! （apply(目标, 参数) 统一入口+参数校验（参数域钳制——超域钳制+诊断）+生命周期（挂载/激活/
//! 取消/完成四态））；组件注册与发现（注册表+按名检索——宿主可枚举可用组件：可发现性红线）；
//! 自定义组件（宿主/第三方用公开 API 组合原语定义新组件——扩展点隔离（自定义崩溃不拖垮本体
//! ——沙箱边界声明））。
//!
//! **数据结构（锚点原文）**：组件注册表（三族+自定义区）；四件套结构；参数校验器。
//!
//! ## 一、四件套为什么是"注册要件"而不是"文档建议"
//!
//! 这条设计里最容易做错的地方，是把无障碍行为当成可选字段——组件作者忘了填，运行时
//! 发现用户开了 reduce，于是每个组件各自补一个分支。补到第三个组件时就会漏，且没人能
//! 从注册表上看出「哪些组件还没想清楚 reduce 怎么走」。
//!
//! 所以本模块把它做成**注册期硬门**：[`MotionComponent::new`] 在构造时就要求调用方
//! 同时交出触发条件、编排图、参数默认值、无障碍行为四样，**缺一样构造直接失败**。
//! 这样"组件没声明 reduce"这件事根本进不了注册表，也就不存在运行期补分支的可能。
//!
//! ## 二、参数为什么"钳制"而不是"拒绝"
//!
//! 锚点写得很明确：**参数越界→钳制+诊断**。理由是组件参数常由宿主按视口/偏好算出来，
//! 因设备差异算出一个略超域的值时，正确行为是钳到边界 + 出诊断让作者知道，而不是让
//! 整条转场崩掉。拒绝只留给**注册期**的静态错误（重名、缺要件），运行期一律钳制。
//!
//! 但钳制必须**双向可查**：既断「越界值确实被钳回域内」，也断「域内值原样透传、零改动」，
//! 否则一个「把所有值都钳成下界」的实现也能全绿。
//!
//! ## 三、命名空间：覆盖内置=拒绝
//!
//! 自定义组件与内置组件撞名时，**不允许自定义覆盖内置**。锚点把它列为红线，理由是
//! 覆盖会让「同名组件在不同宿主里表现不同」，宿主枚举到内置名却拿到第三方实现。
//! 隔离做法是前缀命名空间（`builtin:` / `ext:`），自定义侧名字必须自带 `ext:` 前缀，
//! 想撞内置得先去掉前缀——而去掉前缀就落进"名字里没有命名空间标记"的拒绝分支。
//!
//! ## 四、沙箱：自定义异常只停用该组件
//!
//! 锚点要求「自定义崩溃不拖垮本体」。本模块不写内存沙箱（内核里没有 MMU 隔离可用），
//! 而是做**故障隔离**：自定义组件的 `apply` 由 [`MotionRegistry::apply`] 在受控上下文
//! 里调用，返回 [`ApplyOutcome`]；自定义组件声明的诊断码段与内置**不重叠**，便于事后
//! 分辨是谁的问题。宿主侧对自定义组件的失败只**停用该组件并复述诊断**，其余组件
//! 不受影响——这是本模块能给出的最强保证，边界在文档里明写，不假装有隔离硬件。
//!
//! # no_std
//!
//! 仅依赖同册 [`vep05_orch`]（组件内编排图复用——锚点跨批对接点）与 [`vep03_token`]
//! （reduce 泳道与三段式错误）、`alloc`。零 IO、零墙钟、零浮点。

use crate::svstar2::vep03_token::{Lane, MotionTokenError};
use crate::svstar2::vep04_stack::P_NAMESPACE_OWNER;
use crate::svstar2::vep05_orch::{compile, CompiledOrch, CompileParams, OrchGraph, OrchNode, Orchestrator};

use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、常量（参数唯一源）
// ---------------------------------------------------------------------------

/// 组件协议版本。四件套结构或三族划分变更走版本号。
pub const COMPONENT_PROTOCOL_VERSION: &str = "P04-component-v1";
/// 注册表协议版本（沙箱边界声明）。
pub const REGISTRY_PROTOCOL_VERSION: &str = "P04-registry-v1";
/// 参数域协议版本。
pub const PARAM_PROTOCOL_VERSION: &str = "P04-param-v1";

/// 组件族数（入场/转场/反馈）。
pub const FAMILY_COUNT: usize = 3;
/// 内置命名空间前缀。
pub const NS_BUILTIN: &str = "builtin:";
/// 扩展（自定义）命名空间前缀。
pub const NS_EXT: &str = "ext:";
/// 生命周期四态数（挂载/激活/取消/完成）。
pub const LIFECYCLE_STATE_COUNT: usize = 4;
/// 参数域下界。
pub const PARAM_MIN: i64 = 0;
/// 参数域上界（时序类参数上限 10_000ms，超出无实际意义且会顶穿预算）。
pub const PARAM_MAX: i64 = 10_000;
/// 组件名长度上限。
pub const NAME_CAP: usize = 48;
/// 组件描述长度上限。
pub const DESC_CAP: usize = 128;
/// 注册表容量上限。
pub const MAX_COMPONENTS: usize = 256;
/// 触发条件长度上限。
pub const TRIGGER_CAP: usize = 64;
/// reduce 行为描述长度上限。
pub const REDUCE_NOTE_CAP: usize = 96;

/// 自定义组件专用诊断码段前缀（与内置**不重叠**，便于事后分辨责任方）。
pub const EXT_CODE_PREFIX: &str = "E_EXT_";

/// 自定义组件已停用（**扩展段专属码**——只可能由沙箱对自定义组件发出，
/// 故不进内置码表，避免与 [`EXT_CODE_PREFIX`] 段自相矛盾）。
pub const E_EXT_DISABLED: &str = "E_EXT_DISABLED";

// ---------------------------------------------------------------------------
// 二、诊断码（内置段；自定义段见 [`MotionRegistry::apply`]）
// ---------------------------------------------------------------------------

/// 组件重名。
pub const E_COMPONENT_DUP: &str = "E_COMPONENT_DUP";
/// 组件未声明无障碍行为（注册要件红线）。
pub const E_A11Y_REQUIRED: &str = "E_A11Y_REQUIRED";
/// 组件名缺少命名空间标记。
pub const E_NS_MISSING: &str = "E_NS_MISSING";
/// 组件名以扩展前缀冒充内置。
pub const E_NS_ESCALATE: &str = "E_NS_ESCALATE";
/// 自定义组件试图覆盖内置组件。
pub const E_NS_OVERRIDE: &str = "E_NS_OVERRIDE";
/// 组件注册表超容。
pub const E_REGISTRY_FULL: &str = "E_REGISTRY_FULL";
/// 组件未找到。
pub const E_COMPONENT_UNKNOWN: &str = "E_COMPONENT_UNKNOWN";
/// 编排图为空（四件套之一缺失）。
pub const E_GRAPH_EMPTY: &str = "E_GRAPH_EMPTY";
/// 触发条件为空。
pub const E_TRIGGER_EMPTY: &str = "E_TRIGGER_EMPTY";
/// 参数越界（已钳制，出诊断）。
pub const E_PARAM_CLAMPED: &str = "E_PARAM_CLAMPED";
/// 参数名重复。
pub const E_PARAM_DUP: &str = "E_PARAM_DUP";
/// 参数值域非法（域下界>=上界）。
pub const E_PARAM_DOMAIN: &str = "E_PARAM_DOMAIN";
/// 生命周期状态非法迁移。
pub const E_LIFECYCLE_ILLEGAL: &str = "E_LIFECYCLE_ILLEGAL";
/// 组件名超长。
pub const E_NAME_TOO_LONG: &str = "E_NAME_TOO_LONG";
/// 描述超长。
pub const E_DESC_TOO_LONG: &str = "E_DESC_TOO_LONG";
/// 嵌套深度超限（沿用 F3005 上限 8）。
pub const E_NEST_DEPTH: &str = "E_NEST_DEPTH";

// ---------------------------------------------------------------------------
// 三、组件族与命名空间
// ---------------------------------------------------------------------------

/// 组件族（锚点三族）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Family {
    /// 入场系列。
    Enter,
    /// 转场系列。
    Transition,
    /// 反馈系列。
    Feedback,
    /// 自定义区（第三方/宿主扩展）。
    Custom,
}

impl Family {
    /// 短码（读屏与日志用）。
    pub fn wire(self) -> &'static str {
        match self {
            Family::Enter => "enter",
            Family::Transition => "transition",
            Family::Feedback => "feedback",
            Family::Custom => "custom",
        }
    }

    /// 中文名。
    pub fn zh(self) -> &'static str {
        match self {
            Family::Enter => "入场系列",
            Family::Transition => "转场系列",
            Family::Feedback => "反馈系列",
            Family::Custom => "自定义区",
        }
    }

    /// 稳定序号（内置三族固定 0..3，自定义区固定 3——注册表按此分桶）。
    pub fn index(self) -> usize {
        match self {
            Family::Enter => 0,
            Family::Transition => 1,
            Family::Feedback => 2,
            Family::Custom => 3,
        }
    }

    /// 序号⇒族。
    pub fn from_index(i: usize) -> Option<Family> {
        match i {
            0 => Some(Family::Enter),
            1 => Some(Family::Transition),
            2 => Some(Family::Feedback),
            3 => Some(Family::Custom),
            _ => None,
        }
    }

    /// 是否内置族（自定义区返回 false）。
    pub fn is_builtin(self) -> bool {
        !matches!(self, Family::Custom)
    }

    /// 该族要求的命名空间前缀。
    pub fn namespace(self) -> &'static str {
        if self.is_builtin() {
            NS_BUILTIN
        } else {
            NS_EXT
        }
    }
}

// ---------------------------------------------------------------------------
// 四、生命周期（锚点：挂载/激活/取消/完成四态）
// ---------------------------------------------------------------------------

/// 组件实例生命周期状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lifecycle {
    /// 已挂载（注册完成、尚未激活）。
    Mounted,
    /// 已激活（正在播放）。
    Active,
    /// 已取消（被打断且按策略处置）。
    Cancelled,
    /// 已完成（自然终态）。
    Completed,
}

impl Lifecycle {
    /// 短码。
    pub fn wire(self) -> &'static str {
        match self {
            Lifecycle::Mounted => "mounted",
            Lifecycle::Active => "active",
            Lifecycle::Cancelled => "cancelled",
            Lifecycle::Completed => "completed",
        }
    }

    /// 中文名。
    pub fn zh(self) -> &'static str {
        match self {
            Lifecycle::Mounted => "挂载",
            Lifecycle::Active => "激活",
            Lifecycle::Cancelled => "取消",
            Lifecycle::Completed => "完成",
        }
    }

    /// 序号。
    pub fn index(self) -> usize {
        match self {
            Lifecycle::Mounted => 0,
            Lifecycle::Active => 1,
            Lifecycle::Cancelled => 2,
            Lifecycle::Completed => 3,
        }
    }

    /// 序号⇒态。
    pub fn from_index(i: usize) -> Option<Lifecycle> {
        match i {
            0 => Some(Lifecycle::Mounted),
            1 => Some(Lifecycle::Active),
            2 => Some(Lifecycle::Cancelled),
            3 => Some(Lifecycle::Completed),
            _ => None,
        }
    }

    /// 是否为终态（取消/完成都不可再激活）。
    pub fn is_terminal(self) -> bool {
        matches!(self, Lifecycle::Cancelled | Lifecycle::Completed)
    }

    /// 合法迁移表：挂载⇄激活、任意非终态⇒取消/完成。
    ///
    /// 终态不可回退——否则「已取消的组件被再次激活」会让打断语义失效。
    pub fn can_transition_to(self, next: Lifecycle) -> bool {
        match (self, next) {
            (Lifecycle::Mounted, Lifecycle::Active) => true,
            (Lifecycle::Mounted, Lifecycle::Mounted) => true,
            (Lifecycle::Active, Lifecycle::Active) => true,
            (Lifecycle::Mounted, Lifecycle::Cancelled) => true,
            (Lifecycle::Mounted, Lifecycle::Completed) => true,
            (Lifecycle::Active, Lifecycle::Cancelled) => true,
            (Lifecycle::Active, Lifecycle::Completed) => true,
            _ => false,
        }
    }
}

// ---------------------------------------------------------------------------
// 五、参数域与钳制器（锚点：参数域钳制——超域钳制+诊断）
// ---------------------------------------------------------------------------

/// 一个参数的声明域与默认值（四件套之一：参数默认值）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParamSpec {
    /// 参数名。
    pub name: String,
    /// 值域下界（含）。
    pub min: i64,
    /// 值域上界（含）。
    pub max: i64,
    /// 默认值（须落在 `[min, max]` 内，由 [`ParamSpec::new`] 校验）。
    pub default: i64,
}

impl ParamSpec {
    /// 构造参数声明；域非法或默认值越域即拒。
    pub fn new(name: &str, min: i64, max: i64, default: i64) -> Result<ParamSpec, MotionTokenError> {
        if min >= max {
            return Err(MotionTokenError::new(
                E_PARAM_DOMAIN,
                "参数值域非法",
                &format!("参数 {} 声明的域 [{}, {}) 非法（下界必须严格小于上界）", name, min, max),
                "把上界调大或把下界调小；域为空的参数在钳制时无边界可依",
                P_NAMESPACE_OWNER,
            ));
        }
        if default < min || default > max {
            return Err(MotionTokenError::new(
                E_PARAM_DOMAIN,
                "参数默认值越域",
                &format!(
                    "参数 {} 的默认值 {} 不在域 [{}, {}] 内",
                    name, default, min, max
                ),
                &format!("把默认值改到 [{}, {}] 区间内", min, max),
                P_NAMESPACE_OWNER,
            ));
        }
        Ok(ParamSpec {
            name: String::from(name),
            min,
            max,
            default,
        })
    }

    /// 是否越域。
    pub fn out_of_domain(&self, v: i64) -> bool {
        v < self.min || v > self.max
    }

    /// 钳制到域内（**不依赖调用方已判过越域**——谓词内自行兜底）。
    pub fn clamp(&self, v: i64) -> i64 {
        if v < self.min {
            self.min
        } else if v > self.max {
            self.max
        } else {
            v
        }
    }
}

/// 运行期参数覆盖表（`apply` 的参数入参）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ParamOverrides {
    entries: Vec<(String, i64)>,
}

impl ParamOverrides {
    /// 构造空覆盖表。
    pub fn new() -> Self {
        ParamOverrides {
            entries: Vec::new(),
        }
    }

    /// 追加一项覆盖。
    pub fn set(&mut self, name: &str, value: i64) {
        self.entries.push((String::from(name), value));
    }

    /// 读一项覆盖。
    pub fn get(&self, name: &str) -> Option<i64> {
        self.entries.iter().find(|(k, _)| k == name).map(|(_, v)| *v)
    }

    /// 覆盖项数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// 一次钳制的记账（判据要对账，故独立于 apply 返回值）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ClampRecord {
    /// 被钳的参数名。
    pub param: String,
    /// 传入原值。
    pub given: i64,
    /// 钳制后值。
    pub clamped: i64,
    /// 被钳的方向：-1=低于下界抬到下界，+1=高于上界压到上界。
    pub direction: i8,
}

/// 钳制结果（值 + 记账 + 诊断文本）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClampOutcome {
    /// 生效值（按参数名序）。
    pub values: Vec<(String, i64)>,
    /// 被钳项（按参数名序）。
    pub clamped: Vec<ClampRecord>,
}

impl ClampOutcome {
    /// 生效值条数（应等于声明参数数）。
    pub fn value_count(&self) -> usize {
        self.values.len()
    }

    /// 被钳项数。
    pub fn clamp_count(&self) -> usize {
        self.clamped.len()
    }

    /// 是否发生过钳制。
    pub fn any_clamped(&self) -> bool {
        !self.clamped.is_empty()
    }

    /// 取某参数的生效值。
    pub fn value_of(&self, name: &str) -> Option<i64> {
        self.values.iter().find(|(k, _)| k == name).map(|(_, v)| *v)
    }

    /// 取某参数的钳制记账。
    pub fn record_of(&self, name: &str) -> Option<&ClampRecord> {
        self.clamped.iter().find(|r| r.param == name)
    }

    /// 读屏可读钳制播报（只报参数名与数值，不含组件载荷）。
    pub fn spoken(&self) -> String {
        if self.clamped.is_empty() {
            return String::from("参数全部在域内，未钳制");
        }
        let mut s = String::from("参数越界已钳制：");
        for r in self.clamped.iter() {
            s.push_str(&format!(
                "{} 由 {} 钳为 {}（方向 {}）；",
                r.param, r.given, r.clamped, r.direction
            ));
        }
        s
    }
}

/// 参数校验器：把「声明域 + 默认值 + 运行期覆盖」合成一份生效参数。
///
/// 锚点要求「超域钳制+诊断」——本函数**永不因越界失败**，只记账；未声明的参数名
/// 按域下界兜底并单独记账（否则调用方传错名字会静默取到别的参数的值）。
pub fn resolve_params(
    specs: &[ParamSpec],
    overrides: &ParamOverrides,
) -> Result<ClampOutcome, MotionTokenError> {
    let mut names: Vec<&str> = Vec::with_capacity(specs.len());
    for s in specs.iter() {
        if names.iter().any(|n| *n == s.name.as_str()) {
            return Err(MotionTokenError::new(
                E_PARAM_DUP,
                "参数名重复",
                &format!("参数 {} 在同一组件内声明了多次", s.name),
                "参数名唯一化；同名双域的钳制结果取决于声明顺序，等于不可预期",
                P_NAMESPACE_OWNER,
            ));
        }
        names.push(s.name.as_str());
    }

    let mut values: Vec<(String, i64)> = Vec::with_capacity(specs.len());
    let mut clamped: Vec<ClampRecord> = Vec::new();

    for s in specs.iter() {
        let given = match overrides.get(&s.name) {
            Some(v) => v,
            None => s.default,
        };
        let eff = s.clamp(given);
        if eff != given {
            let direction: i8 = if given < s.min { -1 } else { 1 };
            clamped.push(ClampRecord {
                param: s.name.clone(),
                given,
                clamped: eff,
                direction,
            });
        }
        values.push((s.name.clone(), eff));
    }

    // 调用方传了未声明的参数名：兜底到域下界，并记账（不静默）。
    for (k, v) in overrides.entries.iter() {
        if specs.iter().any(|s| s.name == *k) {
            continue;
        }
        clamped.push(ClampRecord {
            param: k.clone(),
            given: *v,
            clamped: PARAM_MIN,
            direction: -1,
        });
    }

    Ok(ClampOutcome { values, clamped })
}

// ---------------------------------------------------------------------------
// 六、无障碍行为段（锚点：四件套之一，注册要件红线）
// ---------------------------------------------------------------------------

/// reduce 行为：组件在 reduce 泳道下必须做什么。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReduceBehavior {
    /// 整体坍缩为终态直达（与 F3005 编排器同语义——单源）。
    CollapseToEndState,
    /// 保留静态呈现、跳过全部动效。
    StaticOnly,
    /// 交由宿主编排器统一处置（组件自己不处理）。
    DeferToHost,
}

impl ReduceBehavior {
    /// 短码。
    pub fn wire(self) -> &'static str {
        match self {
            ReduceBehavior::CollapseToEndState => "collapse",
            ReduceBehavior::StaticOnly => "static",
            ReduceBehavior::DeferToHost => "defer",
        }
    }

    /// 中文名。
    pub fn zh(self) -> &'static str {
        match self {
            ReduceBehavior::CollapseToEndState => "整体坍缩为终态直达",
            ReduceBehavior::StaticOnly => "静态呈现跳过动效",
            ReduceBehavior::DeferToHost => "交宿主编排器统一处置",
        }
    }

    /// 序号。
    pub fn index(self) -> usize {
        match self {
            ReduceBehavior::CollapseToEndState => 0,
            ReduceBehavior::StaticOnly => 1,
            ReduceBehavior::DeferToHost => 2,
        }
    }

    /// 序号⇒行为。
    pub fn from_index(i: usize) -> Option<ReduceBehavior> {
        match i {
            0 => Some(ReduceBehavior::CollapseToEndState),
            1 => Some(ReduceBehavior::StaticOnly),
            2 => Some(ReduceBehavior::DeferToHost),
            _ => None,
        }
    }
}

/// 无障碍行为段：reduce 行为 + 播报文案（读屏要读得出来）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct A11yBehavior {
    /// reduce 泳道下的行为。
    pub reduce: ReduceBehavior,
    /// 播报文案（组件激活时读屏播报）。
    pub note: String,
}

impl A11yBehavior {
    /// 构造行为段；文案为空即拒（**注册要件红线**：缺省不放行）。
    pub fn new(reduce: ReduceBehavior, note: &str) -> Result<A11yBehavior, MotionTokenError> {
        let trimmed = note.trim();
        if trimmed.is_empty() {
            return Err(MotionTokenError::new(
                E_A11Y_REQUIRED,
                "无障碍播报文案缺省",
                "组件声明了 reduce 行为，但读屏播报文案为空",
                "补一句面向用户的播报文案；空文案会让读屏用户只听到动效声音而不知发生了什么",
                P_NAMESPACE_OWNER,
            ));
        }
        if trimmed.chars().count() > REDUCE_NOTE_CAP {
            return Err(MotionTokenError::new(
                E_A11Y_REQUIRED,
                "无障碍播报文案超长",
                &format!(
                    "播报文案 {} 字，超过 {} 上限",
                    trimmed.chars().count(),
                    REDUCE_NOTE_CAP
                ),
                "缩短播报文案；诊断链与读屏提示都按定长取字段",
                P_NAMESPACE_OWNER,
            ));
        }
        Ok(A11yBehavior {
            reduce,
            note: String::from(trimmed),
        })
    }

    /// 读屏播报（行为 + 文案）。
    pub fn spoken(&self) -> String {
        format!("{}；{}", self.reduce.zh(), self.note)
    }
}

// ---------------------------------------------------------------------------
// 七、动效组件四件套
// ---------------------------------------------------------------------------

/// 动效组件（锚点四件套：触发条件 + 编排图 + 参数默认值 + 无障碍行为）。
///
/// 构造即四件齐备——缺任一件 [`MotionComponent::new`] 失败，故注册表里不可能存在
/// 「没声明 reduce」的组件。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MotionComponent {
    /// 组件名（**必带命名空间前缀**：`builtin:` 或 `ext:`）。
    pub name: String,
    /// 所属族。
    pub family: Family,
    /// 一句话描述（宿主枚举时展示）。
    pub desc: String,
    /// 件①触发条件（声明式）。
    pub trigger: String,
    /// 件②编排图（复用 F3005 图模型）。
    pub graph: OrchGraph,
    /// 件③参数默认值。
    pub params: Vec<ParamSpec>,
    /// 件④无障碍行为。
    pub a11y: A11yBehavior,
}

impl MotionComponent {
    /// 构造四件套。
    ///
    /// 拒绝条件：名超长/描述超长/触发条件空/编排图为空/无障碍行为缺省——全部在
    /// **注册前**拦下，符合锚点「注册要件红线」。
    pub fn new(
        name: &str,
        family: Family,
        desc: &str,
        trigger: &str,
        graph: OrchGraph,
        params: Vec<ParamSpec>,
        a11y: A11yBehavior,
    ) -> Result<MotionComponent, MotionTokenError> {
        if name.chars().count() > NAME_CAP {
            return Err(MotionTokenError::new(
                E_NAME_TOO_LONG,
                "组件名超长",
                &format!("组件名 {} 字，超过 {} 上限", name.chars().count(), NAME_CAP),
                "缩短组件名；名字要带命名空间前缀，能省的长度有限",
                P_NAMESPACE_OWNER,
            ));
        }
        if desc.chars().count() > DESC_CAP {
            return Err(MotionTokenError::new(
                E_DESC_TOO_LONG,
                "组件描述超长",
                &format!(
                    "组件 {} 的描述 {} 字，超过 {} 上限",
                    name,
                    desc.chars().count(),
                    DESC_CAP
                ),
                "缩短描述；宿主枚举面板按定长渲染",
                P_NAMESPACE_OWNER,
            ));
        }
        let trig = trigger.trim();
        if trig.is_empty() {
            return Err(MotionTokenError::new(
                E_TRIGGER_EMPTY,
                "触发条件缺省",
                &format!("组件 {} 没有声明触发条件", name),
                "声明触发条件（如 mount / focus-gain / value-change）；无触发的组件宿主无从挂载",
                P_NAMESPACE_OWNER,
            ));
        }
        if trig.chars().count() > TRIGGER_CAP {
            return Err(MotionTokenError::new(
                E_TRIGGER_EMPTY,
                "触发条件超长",
                &format!(
                    "组件 {} 的触发条件 {} 字，超过 {} 上限",
                    name,
                    trig.chars().count(),
                    TRIGGER_CAP
                ),
                "缩短触发条件串",
                P_NAMESPACE_OWNER,
            ));
        }
        if graph.node_count() == 0 {
            return Err(MotionTokenError::new(
                E_GRAPH_EMPTY,
                "编排图为空",
                &format!("组件 {} 的编排图没有任何节点", name),
                "给编排图加至少一个节点；空图编译不出任何时间轴实例",
                P_NAMESPACE_OWNER,
            ));
        }
        // 参数名重复在此处一并拦下（注册期静态错误），运行期不再重复校验。
        for i in 0..params.len() {
            for j in (i + 1)..params.len() {
                if params[i].name == params[j].name {
                    return Err(MotionTokenError::new(
                        E_PARAM_DUP,
                        "参数名重复",
                        &format!("组件 {} 的参数 {} 声明了多次", name, params[i].name),
                        "参数名唯一化；同名双域的钳制结果取决于声明顺序，等于不可预期",
                        P_NAMESPACE_OWNER,
                    ));
                }
            }
        }

        Ok(MotionComponent {
            name: String::from(name),
            family,
            desc: String::from(desc),
            trigger: String::from(trig),
            graph,
            params,
            a11y,
        })
    }

    /// 是否自定义组件。
    pub fn is_custom(&self) -> bool {
        !self.family.is_builtin()
    }

    /// 参数据数。
    pub fn param_count(&self) -> usize {
        self.params.len()
    }

    /// 读屏播报（组件名 + 族 + 触发 + 无障碍行为）。
    pub fn spoken(&self) -> String {
        format!(
            "{}（{}）：触发 {}；{}",
            self.name,
            self.family.zh(),
            self.trigger,
            self.a11y.spoken()
        )
    }

    /// 按参数默认值求一份生效参数（`apply` 的无覆盖入口）。
    pub fn default_params(&self) -> Result<ClampOutcome, MotionTokenError> {
        resolve_params(&self.params, &ParamOverrides::new())
    }
}

// ---------------------------------------------------------------------------
// 八、组件注册表（三族 + 自定义区）
// ---------------------------------------------------------------------------

/// 组件注册表。
///
/// 检索 O(1) 线性族内扫描（注册表容量 256，族内远小于此；用 `Vec` 而非哈希表是
/// 为了 no_std 下免去哈希依赖并保持可枚举顺序稳定——宿主枚举面板要稳定序）。
#[derive(Clone, Debug, Default)]
pub struct MotionRegistry {
    /// 四桶：内置三族 + 自定义区。
    buckets: [Vec<MotionComponent>; 4],
    /// 已被诊断停用的自定义组件名（停用后仍占桶位，便于宿主枚举时看到"已停用"）。
    disabled: Vec<String>,
    /// 累计停用次数。
    disable_events: u32,
}

impl MotionRegistry {
    /// 构造空注册表。
    pub fn new() -> Self {
        MotionRegistry {
            buckets: [
                Vec::new(),
                Vec::new(),
                Vec::new(),
                Vec::new(),
            ],
            disabled: Vec::new(),
            disable_events: 0,
        }
    }

    /// 注册组件。
    ///
    /// 拒绝路径（锚点红线）：
    /// - 名字缺命名空间标记 ⇒ [`E_NS_MISSING`]
    /// - 自定义组件想用 `builtin:` 前缀冒充内置 ⇒ [`E_NS_ESCALATE`]
    /// - 名字已存在 ⇒ [`E_COMPONENT_DUP`]（**自定义撞内置同样是撞名，不允许覆盖**）
    /// - 注册表超容 ⇒ [`E_REGISTRY_FULL`]
    pub fn register(&mut self, comp: MotionComponent) -> Result<(), MotionTokenError> {
        let custom = comp.is_custom();

        // 前缀与族必须自洽：内置带 builtin:，自定义带 ext:。
        let has_builtin = comp.name.starts_with(NS_BUILTIN);
        let has_ext = comp.name.starts_with(NS_EXT);

        if custom && has_builtin {
            return Err(MotionTokenError::new(
                E_NS_ESCALATE,
                "自定义组件冒充内置",
                &format!(
                    "组件 {} 属于自定义族，却用了内置前缀 {}",
                    comp.name, NS_BUILTIN
                ),
                &format!(
                    "改用扩展前缀 {}；自定义组件伪装成内置会让宿主枚举到内置名却拿到第三方实现",
                    NS_EXT
                ),
                P_NAMESPACE_OWNER,
            ));
        }
        if !custom && has_ext {
            return Err(MotionTokenError::new(
                E_NS_ESCALATE,
                "内置组件用了扩展前缀",
                &format!(
                    "组件 {} 属于内置族，却用了扩展前缀 {}",
                    comp.name, NS_EXT
                ),
                &format!("改用内置前缀 {}", NS_BUILTIN),
                P_NAMESPACE_OWNER,
            ));
        }
        if !has_builtin && !has_ext {
            return Err(MotionTokenError::new(
                E_NS_MISSING,
                "组件名缺命名空间标记",
                &format!("组件名 {} 没有 {} 或 {} 前缀", comp.name, NS_BUILTIN, NS_EXT),
                &format!(
                    "给名字加前缀：内置用 {}，自定义用 {}；无前缀的名字无法判定归属，跨宿主行为不可预期",
                    NS_BUILTIN, NS_EXT
                ),
                P_NAMESPACE_OWNER,
            ));
        }

        // 撞名检查跨全部四桶（自定义撞内置同样是撞名——覆盖内置是红线）。
        for b in self.buckets.iter() {
            if let Some(existing) = b.iter().find(|c| c.name == comp.name) {
                let who = if existing.is_custom() { "自定义" } else { "内置" };
                return Err(MotionTokenError::new(
                    E_COMPONENT_DUP,
                    "组件重名",
                    &format!(
                        "组件 {} 已由{}组件 {} 注册，重名组件不允许覆盖",
                        comp.name,
                        who,
                        existing.name
                    ),
                    "换一个名字；覆盖已注册组件会让同名组件在不同宿主里表现不同",
                    P_NAMESPACE_OWNER,
                ));
            }
        }

        let total = self.len();
        if total >= MAX_COMPONENTS {
            return Err(MotionTokenError::new(
                E_REGISTRY_FULL,
                "组件注册表已满",
                &format!("注册表已有 {} 个组件，达到 {} 上限", total, MAX_COMPONENTS),
                "注销不再使用的组件，或分批注册",
                P_NAMESPACE_OWNER,
            ));
        }

        self.buckets[comp.family.index()].push(comp);
        Ok(())
    }

    /// 按名检索（全桶线性；容量小，且宿主枚举要稳定序）。
    pub fn find(&self, name: &str) -> Option<&MotionComponent> {
        for b in self.buckets.iter() {
            if let Some(c) = b.iter().find(|c| c.name == name) {
                return Some(c);
            }
        }
        None
    }

    /// 组件是否存在。
    pub fn has(&self, name: &str) -> bool {
        self.find(name).is_some()
    }

    /// 组件总数。
    pub fn len(&self) -> usize {
        self.buckets.iter().map(|b| b.len()).sum()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// 某族组件数。
    pub fn family_count(&self, f: Family) -> usize {
        self.buckets[f.index()].len()
    }

    /// 某族切片（宿主枚举用，稳定序）。
    pub fn family_slice(&self, f: Family) -> &[MotionComponent] {
        &self.buckets[f.index()]
    }

    /// 全部内置组件数（三族合计）。
    pub fn builtin_count(&self) -> usize {
        self.family_count(Family::Enter)
            + self.family_count(Family::Transition)
            + self.family_count(Family::Feedback)
    }

    /// 自定义组件数。
    pub fn custom_count(&self) -> usize {
        self.family_count(Family::Custom)
    }

    /// 组件是否已停用。
    pub fn is_disabled(&self, name: &str) -> bool {
        self.disabled.iter().any(|d| d == name)
    }

    /// 累计停用次数。
    pub fn disable_events(&self) -> u32 {
        self.disable_events
    }

    /// 已停用组件名清单。
    pub fn disabled_names(&self) -> &[String] {
        &self.disabled
    }

    /// 四族计数快照（判据对账用：分桶之和恒等于总数）。
    pub fn family_census(&self) -> Vec<(Family, usize)> {
        let mut out = Vec::with_capacity(FAMILY_COUNT + 1);
        let mut i = 0usize;
        while i <= FAMILY_COUNT {
            if let Some(f) = Family::from_index(i) {
                out.push((f, self.family_count(f)));
            }
            i += 1;
        }
        out
    }
}

// ---------------------------------------------------------------------------
// 九、组件接口：apply(目标, 参数) + 生命周期
// ---------------------------------------------------------------------------

/// apply 的目标描述（宿主传入）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ApplyTarget {
    /// 目标元素 ID。
    pub element: u32,
    /// 附加上下文（宿主私有，组件不解释）。
    pub context: u32,
}

/// apply 结果。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApplyOutcome {
    /// 生效组件名。
    pub component: String,
    /// 生效目标（锚点 `apply(目标, 参数)` 的「目标」侧，回写供宿主对账）。
    pub target: ApplyTarget,
    /// 编译产物（reduce 泳道下已整体坍缩）。
    pub compiled: CompiledOrch,
    /// 生效参数。
    pub params: ClampOutcome,
    /// 生命周期迁移后状态。
    pub lifecycle: Lifecycle,
    /// 诊断（钳制/停用等；有则非空）。
    pub diagnostics: Vec<String>,
}

impl ApplyOutcome {
    /// 实例数。
    pub fn instance_count(&self) -> usize {
        self.compiled.instance_count()
    }

    /// 总时长（毫秒）。
    pub fn total_duration_ms(&self) -> u32 {
        self.compiled.total_duration_ms()
    }

    /// 是否发生钳制。
    pub fn any_clamped(&self) -> bool {
        self.params.any_clamped()
    }

    /// 读屏播报。
    pub fn spoken(&self) -> String {
        format!(
            "{} 已{}，实例 {} 个，时长 {} 毫秒；{}",
            self.component,
            self.lifecycle.zh(),
            self.instance_count(),
            self.total_duration_ms(),
            if self.params.any_clamped() {
                self.params.spoken()
            } else {
                String::from("参数全部在域内")
            }
        )
    }
}

/// 组件实例（挂载后持有生命周期状态）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ComponentInstance {
    /// 组件名。
    pub component: String,
    /// 当前生命周期态。
    pub state: Lifecycle,
    /// 迁移历史（按发生序；判据要断轨迹，故完整保留）。
    pub history: Vec<Lifecycle>,
}

impl ComponentInstance {
    /// 构造挂载态实例。
    pub fn mounted(component: &str) -> ComponentInstance {
        ComponentInstance {
            component: String::from(component),
            state: Lifecycle::Mounted,
            history: vec![Lifecycle::Mounted],
        }
    }

    /// 执行生命周期迁移；非法迁移即拒。
    pub fn transition(
        &mut self,
        next: Lifecycle,
    ) -> Result<(), MotionTokenError> {
        if !self.state.can_transition_to(next) {
            return Err(MotionTokenError::new(
                E_LIFECYCLE_ILLEGAL,
                "生命周期迁移非法",
                &format!(
                    "组件 {} 当前 {}，不能迁移到 {}",
                    self.component,
                    self.state.wire(),
                    next.wire()
                ),
                &format!(
                    "合法目标：{} 可到激活/取消/完成；终态（取消/完成）不可回退",
                    self.state.wire()
                ),
                P_NAMESPACE_OWNER,
            ));
        }
        self.state = next;
        self.history.push(next);
        Ok(())
    }

    /// 迁移轨迹长度（含初始挂载）。
    pub fn history_len(&self) -> usize {
        self.history.len()
    }
}

// ---------------------------------------------------------------------------
// 十、沙箱：扩展点隔离声明 + 自定义异常停用
// ---------------------------------------------------------------------------

/// 沙箱边界声明（锚点：自定义崩溃不拖垮本体——沙箱边界声明）。
///
/// **诚实声明**：本模块**不提供内存隔离**（内核上下文无 MMU 级沙箱可用）。它提供的是
/// 故障隔离 + 命名空间隔离 + 诊断码段隔离。自定义组件的失败被 [`MotionRegistry::apply`]
/// 捕获并只停用该组件；诊断码走 `E_EXT_*` 段与内置不重叠，便于事后分辨责任方。
pub const SANDBOX_BOUNDARY: &str = "P04-sandbox-boundary-v1";

/// 沙箱能力位。
pub const SANDBOX_FAULT_ISOLATION: u8 = 0b0001;
/// 沙箱命名空间隔离。
pub const SANDBOX_NS_ISOLATION: u8 = 0b0010;
/// 沙箱诊断码段隔离。
pub const SANDBOX_CODE_ISOLATION: u8 = 0b0100;
/// 沙箱**不提供**的能力：内存隔离（明写边界，不假装有）。
pub const SANDBOX_NO_MEM_ISOLATION: u8 = 0b1000;

/// 沙箱能力全集（用于判据逐位钉死）。
pub const SANDBOX_CAPS: u8 = SANDBOX_FAULT_ISOLATION
    | SANDBOX_NS_ISOLATION
    | SANDBOX_CODE_ISOLATION
    | SANDBOX_NO_MEM_ISOLATION;

/// 自定义组件的注册句柄（宿主拿它做 apply，便于停用后快速拒绝）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExtHandle {
    /// 组件名索引（注册顺序）。
    pub slot: u32,
    /// 是否仍启用。
    pub enabled: bool,
}

/// apply 的宿主侧统一入口。
impl MotionRegistry {
    /// 统一 apply 入口：取组件 → 解参 → 编译 → 迁移到激活。
    ///
    /// 沙箱行为：自定义组件已被诊断停用时，**只拒绝该组件**，其余组件不受影响；
    /// 内置组件走同一入口但不可能被停用（停用清单只收自定义名）。
    pub fn apply(
        &mut self,
        target: ApplyTarget,
        name: &str,
        overrides: &ParamOverrides,
        lane: Lane,
        prior: &mut ComponentInstance,
    ) -> Result<ApplyOutcome, MotionTokenError> {
        let custom_name = name.starts_with(NS_EXT);

        if custom_name && self.is_disabled(name) {
            return Err(MotionTokenError::new(
                E_EXT_DISABLED,
                "自定义组件已停用",
                &format!("自定义组件 {} 此前异常已被隔离停用", name),
                "修复该组件的缺陷后重新注册；停用期间该组件不可用，其余组件不受影响",
                P_NAMESPACE_OWNER,
            ));
        }

        let comp = match self.find(name) {
            Some(c) => c.clone(),
            None => {
                return Err(MotionTokenError::new(
                    E_COMPONENT_UNKNOWN,
                    "组件未注册",
                    &format!("按名 {} 找不到组件（已注册 {} 个）", name, self.len()),
                    &format!(
                        "先注册再 apply；可用名字见注册表枚举（内置 {} 个、自定义 {} 个）",
                        self.builtin_count(),
                        self.custom_count()
                    ),
                    P_NAMESPACE_OWNER,
                ))
            }
        };

        // 生命周期：挂载 ⇒ 激活（已完成/已取消的实例不可再激活）。
        prior.transition(Lifecycle::Active)?;

        // 解参（越界钳制不失败，只记账）。
        let params = resolve_params(&comp.params, overrides)?;

        // 编译编排图（F3005 单源图模型；reduce 泳道在此坍缩）。
        let cparams = CompileParams::normal(lane);
        let compiled = match compile(&comp.graph, &cparams) {
            Ok(c) => c,
            Err(e) => {
                // 自定义组件编译失败 ⇒ 隔离停用 + 显性化复述；内置组件编译失败直接上抛。
                if custom_name {
                    self.disable_component(name);
                    let mut err = e;
                    err.next = format!(
                        "{}；该自定义组件已隔离停用（其余组件不受影响），修复后重新注册",
                        err.next
                    );
                    return Err(err);
                }
                return Err(e);
            }
        };

        let mut diagnostics: Vec<String> = Vec::new();
        if params.any_clamped() {
            diagnostics.push(format!(
                "{}：{}",
                E_PARAM_CLAMPED,
                params.spoken()
            ));
        }

        Ok(ApplyOutcome {
            component: comp.name.clone(),
            target,
            compiled,
            params,
            lifecycle: prior.state,
            diagnostics,
        })
    }

    /// 隔离停用一个自定义组件（内置组件停用是编程错误，直接忽略并计数）。
    pub fn disable_component(&mut self, name: &str) -> bool {
        if !name.starts_with(NS_EXT) {
            return false;
        }
        if !self.is_disabled(name) {
            self.disabled.push(String::from(name));
            self.disable_events += 1;
        }
        true
    }
}

/// 读屏播报：注册表可枚举性（锚点：宿主可枚举可用组件——可发现性红线）。
pub fn registry_spoken(reg: &MotionRegistry) -> String {
    let mut s = format!(
        "动效组件注册表共 {} 个：",
        reg.len()
    );
    for (f, n) in reg.family_census().iter() {
        s.push_str(&format!("{} {} 个；", f.zh(), n));
    }
    if !reg.disabled_names().is_empty() {
        s.push_str(&format!(
            "已停用自定义组件 {} 个：",
            reg.disabled_names().len()
        ));
        for d in reg.disabled_names().iter() {
            s.push_str(&format!("{}；", d));
        }
    }
    s
}

// ---------------------------------------------------------------------------
// 十一、内置组件构造助手（判据语料用；宿主也可用它起步）
// ---------------------------------------------------------------------------

/// 构造一条"序列两节点"的最小编排图（判据与示例的共同起手式）。
pub fn demo_graph(prefix: &str, duration_ms: u32) -> Result<OrchGraph, MotionTokenError> {
    let mut o = Orchestrator::new();
    let a = o.node(OrchNode::new(
        0,
        1,
        1,
        &format!("{}a", prefix),
        duration_ms,
        0,
    ))?;
    let b = o.node(OrchNode::new(
        1,
        1,
        2,
        &format!("{}b", prefix),
        duration_ms,
        0,
    ))?;
    o.edge(crate::svstar2::vep05_orch::TimingEdge::after_complete(a, b))?;
    o.commit()
}

/// 构造一个内置组件（四件齐备的最小样例）。
pub fn demo_component(
    name: &str,
    family: Family,
    trigger: &str,
) -> Result<MotionComponent, MotionTokenError> {
    MotionComponent::new(
        name,
        family,
        "内置样例组件",
        trigger,
        //图前缀用族短码而非组件全名：组件名带命名空间前缀（`builtin:`），
        // 整串当节点名必然撞 F3005 的 `NODE_NAME_CAP`，会让**组件名长度校验永远
        // 没机会跑**（先在图构建期被 `E_NODE_DUPLICATE` 拒掉）。二者解耦。
        demo_graph(family.wire(), 120)?,
        vec![ParamSpec::new("duration", 0, 1000, 120)?, ParamSpec::new("delay", 0, 500, 0)?],
        A11yBehavior::new(ReduceBehavior::CollapseToEndState, "动效已整体跳到终态")?,
    )
}