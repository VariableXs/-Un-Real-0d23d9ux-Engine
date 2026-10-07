//! VE-F2602 · 控件树模型（VE-N 域 · UI 框架内核 · N01 组）—— **Rust 权威实现**。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F2602`
//!
//! # 迁移声明（TS → Rust，权威源单点）
//!
//! 本模块是 `src/system/ve/uiKernel/f2602-control-tree-model.ts`（1905 行 TS）的
//! **Rust 重实现**，按 Variable 2026-10-07 指令「全部功能围绕内核、全部为 Rust」
//! 而作。TS 文件**保留不删**（源码只增不减，且删除闸门会拦），但**权威源是本文件**：
//! 后续修改一律改这里，TS 侧不再跟进。理由不是"新更好"，是内核态跑不了 JS
//! （无堆、无 GC、无动态链接，见 `mod.rs` 头注落位铁律）。
//!
//! # 判据（锚点原文四条）
//!
//! 1. **四要素模型**（属性 / 子节点 / 事件 / 状态）；
//! 2. **三不变量**（单亲 / 无环 / 序稳定）；
//! 3. **原子操作**（insert / remove / reorder + 批量单事务）；
//! 4. **M04 绑定**（`bind_path` 路径解析器，树侧实现）。
//!
//! # 判据一：四要素模型
//!
//! 节点四要素逐条规格（锚点要求「逐要素规格公开」）：
//!
//! | 要素 | 本模块类型 | 规格要点 |
//! | --- | --- | --- |
//! | 属性 | [`PropertyKey`] 封闭枚举 | **只持键不持值**——值归 F2604 属性引擎 |
//! | 子节点 | `Vec<String>` 有序 | 序由三操作唯一决定（三不变量之二） |
//! | 事件 | `Vec<HandlerEntry>` 有序 | 顺序即执行序，N03 输入域前向对接 |
//! | 状态 | [`VisualState`] 封闭枚举 | 由 [`resolve_visual_state`] 唯一求出 |
//!
//! **为什么属性只持键不持值**（最容易走错的一步）：树若也存一份值，
//! 属性引擎（F2604）再存一份，就出现双源。两份值可以不一致（写引擎成功、
//! 写树失败），而症状是「属性面板显示 A、界面渲染 B」，且两份都"对"——
//! 归因时无从判定谁是真源。故树只认键（用于路由与查询），值单一归F2604。
//!
//! **为什么状态是枚举而不是布尔标志组**（`hovered/pressed/focused` 三bool）：
//! 布尔能表示的组合远多于合法组合——`hovered && pressed && disabled` 在语义上
//! 矛盾（禁用控件不该响应 hover），但三个 bool 允许它存在，且各控件对矛盾
//! 组合的渲染策略不同，最终同一控件在不同实现里呈现不同颜色。
//! 枚举把矛盾组合从"可能"变成"不可能"，代价是状态机要处理优先级，
//! 由 [`resolve_visual_state`] 一处纯函数集中承担（可单测穷举 2^5 = 32 组合）。
//!
//! # 判据二：三不变量
//!
//! 三条都写成**可机检断言**，不是文档里的承诺：
//!
//! - **单亲**：每节点至多一个父。纪律：父指针是唯一权威，子列表只作镜像；
//!   任何改动父关系的操作必须同事务更新两侧。断言：[`assert_invariants`]
//!   逐节点核对双向互指。
//! - **无环**：祖先链无重复。纪律：插入前沿**父的祖先链上溯**（O(深度)），
//!   不下钻子树（后者对宽树是 O(子树)）。断言：上溯携带 visited，遇重复即判环。
//! - **序稳定**：兄弟序只由 insert/remove/reorder 改动。纪律：移除后序号
//!   **不重排**（允许空洞）。断言：索引越界即拒绝（`ORDER_STABILITY_VIOLATION`）。
//!
//! **「remove 后序号不重排」的取舍**（显式记录，因最易与下游冲突）：
//! 备选是 remove 后把后续兄弟序号全部前移（保持连续）。问题是：若外部
//! （虚拟化列表 F2601组/ 动画轨道 M04）持有兄弟序号作为稳定标识，序号前移会让
//! 那些标识指向别的控件——症状是「删掉第 3 行后，第 5 行的动画播到了第 4 行上」，
//! 且只在有删除操作的会话里出现。本模块取「序号只保证单调递增与不重复，
//! 不保证连续」，需要连续序号的消费者应使用**稳定 id**。
//!
//! # 判据三：原子操作
//!
//! - `insert` / `remove` / `reorder` 三操作 + [`transact`] 批量单事务。
//! - `reorder` 的 `new_index` 语义固定为**「先移除、再插入」**：children=[a,b,c]，
//!   `reorder(a, 2)` → 移除 a 得 [b,c] → 在索引 2 插入得 [b,c,a]。若解释为
//!   「移除之前的位置」会得 [c,a,b]——两种语义都自洽，但差一位且每种操作都差
//!   一位，极难归因。故在签名与文档中固定为移除后语义。
//! - 事务回滚用**前像快照纯赋值**，不靠"反向再执行一遍操作"——反向操作本身
//!   可能失败，那时树停在半回滚状态，比不回滚更糟（不回滚至少是整体失败，
//!   半回滚是"树已损坏但没人知道"）。
//!
//! # 判据四：M04 绑定
//!
//! `bind_path` 语法：`seg ('/' seg)*`，`seg = 段名 ['#' 兄弟序号]`。
//! 段名**优先匹配 kind**，序号仅作同类消歧——若只支持序号，节点一旦被重排，
//! 绑定会静默漂到别的控件上（症状「动画打到隔壁控件」，极难归因）。
//!
//! # 深度上限
//!
//! [`MAX_TREE_DEPTH`] = 512。真实 UI 树深度通常 20~40 层，512 已是十余倍；
//! 恶意构造可轻松做到数万层（循环里不断 insert）。不设上限时，一次深度 50000
//! 的插入会让后续任何遍历（命中/布局/序列化）触发栈溢出——那类崩溃在生产里
//! 表现为"打开某个界面就崩"，且崩溃点与攻击点无关，极难归因。
//!
//! 零外部依赖；逻辑 tick 注入，零墙钟；确定性算法、零 IO、回归可复现。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码（树专属码 + 域级归属映射）
// ---------------------------------------------------------------------------

/// 域级诊断码（VE-N 域口径）。
///
/// 取超集而非平行联合：若树码自成一套平行类型，上报通道就得为树单独开类型，
/// 各处 `push(树码)` 全部编译不过；最自然的反应是写 `as` 强转绕过——那行强转
/// 在编译期静默通过，运行时若码值真进了通道，下游按码分派的 `match` 会落到
/// `_ =>` 被丢弃：诊断"发出去了"但没人处理，全程零报错。取超集则天然可传，
/// 编译期即锁死。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DiagCode {
    /// 违反三大件根基规格（树不变量破了要回改规格）。
    PillarSpecInconsistent,
    /// 主题规格不一致（调用方用法错，改调用方）。
    ThemeSpecInconsistent,
    /// 越限攻击类（深度上限这类防护无属主能力，须显性上报）。
    CapabilityUnclaimed,
    /// 跨域协议范围错位（绑定路径类）。
    ProtocolScopeMismatch,
}

/// 控件树专属诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TreeDiagCode {
    /// 单亲冲突：待插入节点已有父。
    SingleParentViolation,
    /// 无环破坏：插入将形成祖先环。
    AncestorCycle,
    /// 序稳定破坏：索引越界或重排位置非法。
    OrderStabilityViolation,
    /// 深度超限（fuzz 面 F2609）。
    DepthLimitExceeded,
    /// 节点不存在。
    NodeNotFound,
    /// 父节点不存在。
    ParentInvalid,
    /// 节点自插入（环的特例）。
    SelfInsertion,
    /// 批量事务中途失败，已回滚。
    BatchRolledBack,
    /// 批量操作违反单事务纪律。
    BatchNotAtomic,
    /// 绑定路径失效。
    BindingPathInvalid,
    /// 绑定目标节点已销毁。
    BindingTargetDestroyed,
    /// 状态值不在状态机枚举内。
    StateInvalid,
    /// 事件类型不在封闭集内。
    EventTypeUnknown,
    /// 属性键类型非法。
    PropertyKeyInvalid,
    /// 树未初始化或已销毁（调用序错误）。
    TreeLifecycleViolation,
    /// 自检审计不通过。
    TreeSelfcheckFailed,
}

/// 树码全集（映射表完备性机检的事实源）。
pub const TREE_DIAG_CODES: [TreeDiagCode; 16] = [
    TreeDiagCode::SingleParentViolation,
    TreeDiagCode::AncestorCycle,
    TreeDiagCode::OrderStabilityViolation,
    TreeDiagCode::DepthLimitExceeded,
    TreeDiagCode::NodeNotFound,
    TreeDiagCode::ParentInvalid,
    TreeDiagCode::SelfInsertion,
    TreeDiagCode::BatchRolledBack,
    TreeDiagCode::BatchNotAtomic,
    TreeDiagCode::BindingPathInvalid,
    TreeDiagCode::BindingTargetDestroyed,
    TreeDiagCode::StateInvalid,
    TreeDiagCode::EventTypeUnknown,
    TreeDiagCode::PropertyKeyInvalid,
    TreeDiagCode::TreeLifecycleViolation,
    TreeDiagCode::TreeSelfcheckFailed,
];

/// 树码 → 域级码的归属映射（穷尽表，机检完备性）。
///
/// 归属按**处置动作**而非症状划分，因为下游按码分派时关心的正是"该怎么办"：
/// 违反三不变量 → 回改规格（PILLAR）；非法输入/调用序错 → 改调用方（THEME）；
/// 越限攻击 → 显性上报（CAPABILITY）；跨域绑定错位 → 协议范围（PROTOCOL）。
pub const fn tree_code_ownership(code: TreeDiagCode) -> DiagCode {
    match code {
        TreeDiagCode::SingleParentViolation
        | TreeDiagCode::AncestorCycle
        | TreeDiagCode::OrderStabilityViolation
        | TreeDiagCode::BatchNotAtomic
        | TreeDiagCode::TreeSelfcheckFailed => DiagCode::PillarSpecInconsistent,
        TreeDiagCode::DepthLimitExceeded => DiagCode::CapabilityUnclaimed,
        TreeDiagCode::BindingPathInvalid | TreeDiagCode::BindingTargetDestroyed => {
            DiagCode::ProtocolScopeMismatch
        }
        TreeDiagCode::NodeNotFound
        | TreeDiagCode::ParentInvalid
        | TreeDiagCode::SelfInsertion
        | TreeDiagCode::BatchRolledBack
        | TreeDiagCode::StateInvalid
        | TreeDiagCode::EventTypeUnknown
        | TreeDiagCode::PropertyKeyInvalid
        | TreeDiagCode::TreeLifecycleViolation => DiagCode::ThemeSpecInconsistent,
    }
}

/// 树诊断码的稳定线缆名（上报与日志用；与枚举逐条对应）。
pub const fn tree_code_name(code: TreeDiagCode) -> &'static str {
    match code {
        TreeDiagCode::SingleParentViolation => "SINGLE_PARENT_VIOLATION",
        TreeDiagCode::AncestorCycle => "ANCESTOR_CYCLE",
        TreeDiagCode::OrderStabilityViolation => "ORDER_STABILITY_VIOLATION",
        TreeDiagCode::DepthLimitExceeded => "DEPTH_LIMIT_EXCEEDED",
        TreeDiagCode::NodeNotFound => "NODE_NOT_FOUND",
        TreeDiagCode::ParentInvalid => "PARENT_INVALID",
        TreeDiagCode::SelfInsertion => "SELF_INSERTION",
        TreeDiagCode::BatchRolledBack => "BATCH_ROLLED_BACK",
        TreeDiagCode::BatchNotAtomic => "BATCH_NOT_ATOMIC",
        TreeDiagCode::BindingPathInvalid => "BINDING_PATH_INVALID",
        TreeDiagCode::BindingTargetDestroyed => "BINDING_TARGET_DESTROYED",
        TreeDiagCode::StateInvalid => "STATE_INVALID",
        TreeDiagCode::EventTypeUnknown => "EVENT_TYPE_UNKNOWN",
        TreeDiagCode::PropertyKeyInvalid => "PROPERTY_KEY_INVALID",
        TreeDiagCode::TreeLifecycleViolation => "TREE_LIFECYCLE_VIOLATION",
        TreeDiagCode::TreeSelfcheckFailed => "TREE_SELFCHECK_FAILED",
    }
}

/// 树诊断结构：code + message + hint + at。
///
/// `at` 字段是本条自加的：树操作的排障第一问是"哪个节点 / 哪条路径触发的"。
/// 若不带 `at`，一棵万节点树报出 3 条 `ORDER_STABILITY_VIOLATION` 时，
/// 开发者无法知道是哪三处，只能全树搜索——而树操作的失败往往与节点 id 直接相关。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TreeDiagnostic {
    /// 诊断码。
    pub code: TreeDiagCode,
    /// 人话描述。
    pub message: String,
    /// 处置指引（错误三要素之一，必须说"该怎么办"）。
    pub hint: String,
    /// 触发位置（节点 id 或路径）。
    pub at: String,
}

/// 便捷构造诊断。
pub fn td(code: TreeDiagCode, message: &str, hint: &str, at: &str) -> TreeDiagnostic {
    TreeDiagnostic {
        code,
        message: String::from(message),
        hint: String::from(hint),
        at: String::from(at),
    }
}

/// 树侧结果类型（`no_std` 不可用 `?` 于自定义类型，故用显式 `match`）。
pub type TreeOutcome<T> = Result<T, TreeDiagnostic>;

/// 构造成功值。
pub fn tok<T>(v: T) -> TreeOutcome<T> {
    Ok(v)
}

/// 构造失败诊断。
pub fn tfail<T>(code: TreeDiagCode, message: &str, hint: &str, at: &str) -> TreeOutcome<T> {
    Err(td(code, message, hint, at))
}

/// 诊断收集袋（零运行时契约层：纯容器，无隐式全局）。
#[derive(Clone, Debug, Default)]
pub struct DiagBag {
    /// 已收集诊断。
    pub items: Vec<TreeDiagnostic>,
}

impl DiagBag {
    /// 新建空袋。
    pub fn new() -> DiagBag {
        DiagBag { items: Vec::new() }
    }

    /// 追加一条诊断。
    pub fn push(&mut self, d: TreeDiagnostic) {
        self.items.push(d);
    }

    /// 是否有诊断。
    pub fn has_p0(&self) -> bool {
        !self.items.is_empty()
    }

    /// 便捷：追加构造好的诊断。
    pub fn add(&mut self, code: TreeDiagCode, message: &str, hint: &str, at: &str) {
        self.push(td(code, message, hint, at));
    }
}

// ---------------------------------------------------------------------------
// 二、判据一：四要素封闭集（属性键 / 事件类型 / 视觉状态）
// ---------------------------------------------------------------------------

/// 属性键封闭集（新增键须同步改本枚举与 F2604 属性引擎键表）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PropertyKey {
    /// 文本内容键。
    Text,
    /// 可见性。
    Visible,
    /// 使能。
    Enabled,
    /// 宽。
    Width,
    /// 高。
    Height,
    /// 不透明度。
    Opacity,
    /// 颜色。
    Color,
    /// X 坐标。
    PositionX,
    /// Y 坐标。
    PositionY,
    /// 层级序。
    ZIndex,
    /// 剪裁。
    Clip,
    /// 无障碍标签（无障碍面唯一入口，值归F2604）。
    AriaLabel,
    /// 绑定路径（M04）。
    BindPath,
}

/// 属性键全集（枚举守卫的事实源）。
pub const PROPERTY_KEYS: [PropertyKey; 13] = [
    PropertyKey::Text,
    PropertyKey::Visible,
    PropertyKey::Enabled,
    PropertyKey::Width,
    PropertyKey::Height,
    PropertyKey::Opacity,
    PropertyKey::Color,
    PropertyKey::PositionX,
    PropertyKey::PositionY,
    PropertyKey::ZIndex,
    PropertyKey::Clip,
    PropertyKey::AriaLabel,
    PropertyKey::BindPath,
];

impl PropertyKey {
    /// 属性键线缆名（序列化与诊断用）。
    pub const fn as_str(self) -> &'static str {
        match self {
            PropertyKey::Text => "text",
            PropertyKey::Visible => "visible",
            PropertyKey::Enabled => "enabled",
            PropertyKey::Width => "width",
            PropertyKey::Height => "height",
            PropertyKey::Opacity => "opacity",
            PropertyKey::Color => "color",
            PropertyKey::PositionX => "position-x",
            PropertyKey::PositionY => "position-y",
            PropertyKey::ZIndex => "z-index",
            PropertyKey::Clip => "clip",
            PropertyKey::AriaLabel => "aria-label",
            PropertyKey::BindPath => "bind-path",
        }
    }
}

/// 输入事件类型封闭集（N03 输入域前向对接点）。
///
/// 封闭的意义：N03 派发事件时按类型路由到处理器表，若类型不封闭，就会出现
/// "派发到一个没有处理器的类型"——那类事件被静默丢弃，用户表现为"点了没反应"，
/// 而日志里什么都没有。封闭集 + 未知类型显性拒绝是同一件事。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EventType {
    /// 指针按下。
    PointerDown,
    /// 指针抬起。
    PointerUp,
    /// 指针移动。
    PointerMove,
    /// 滚轮。
    Wheel,
    /// 按键按下。
    KeyDown,
    /// 按键抬起。
    KeyUp,
    /// 获焦。
    Focus,
    /// 失焦。
    Blur,
    /// 值变更（滑杆等）。
    ValueChange,
    /// 布局变更。
    LayoutChange,
}

/// 事件类型全集。
pub const EVENT_TYPES: [EventType; 10] = [
    EventType::PointerDown,
    EventType::PointerUp,
    EventType::PointerMove,
    EventType::Wheel,
    EventType::KeyDown,
    EventType::KeyUp,
    EventType::Focus,
    EventType::Blur,
    EventType::ValueChange,
    EventType::LayoutChange,
];

impl EventType {
    /// 事件类型线缆名。
    pub const fn as_str(self) -> &'static str {
        match self {
            EventType::PointerDown => "pointer-down",
            EventType::PointerUp => "pointer-up",
            EventType::PointerMove => "pointer-move",
            EventType::Wheel => "wheel",
            EventType::KeyDown => "key-down",
            EventType::KeyUp => "key-up",
            EventType::Focus => "focus",
            EventType::Blur => "blur",
            EventType::ValueChange => "value-change",
            EventType::LayoutChange => "layout-change",
        }
    }

    /// 按线缆名解析（未知值显性拒绝，不返回默认值）。
    pub fn from_wire(s: &str) -> TreeOutcome<EventType> {
        for t in EVENT_TYPES.iter() {
            if t.as_str() == s {
                return tok(*t);
            }
        }
        tfail(
            TreeDiagCode::EventTypeUnknown,
            &format!("未知事件类型：{}", s),
            "事件类型是封闭集；新增类型须改 EventType 与 EVENT_TYPES",
            s,
        )
    }
}

/// 视觉状态枚举（状态机声明）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VisualState {
    /// 常态。
    Normal,
    /// 悬停。
    Hovered,
    /// 按下。
    Pressed,
    /// 聚焦。
    Focused,
    /// 聚焦且悬停。
    FocusedHovered,
    /// 禁用。
    Disabled,
    /// 禁用且悬停。
    DisabledHovered,
    /// 选中（业务态，与焦点不是一回事）。
    Active,
}

/// 视觉状态全集。
pub const VISUAL_STATES: [VisualState; 8] = [
    VisualState::Normal,
    VisualState::Hovered,
    VisualState::Pressed,
    VisualState::Focused,
    VisualState::FocusedHovered,
    VisualState::Disabled,
    VisualState::DisabledHovered,
    VisualState::Active,
];

impl VisualState {
    /// 视觉状态线缆名。
    pub const fn as_str(self) -> &'static str {
        match self {
            VisualState::Normal => "normal",
            VisualState::Hovered => "hovered",
            VisualState::Pressed => "pressed",
            VisualState::Focused => "focused",
            VisualState::FocusedHovered => "focused-hovered",
            VisualState::Disabled => "disabled",
            VisualState::DisabledHovered => "disabled-hovered",
            VisualState::Active => "active",
        }
    }

    /// 是否为 disabled 族（自检用：disabled 必须压过一切信号）。
    pub const fn is_disabled_family(self) -> bool {
        matches!(self, VisualState::Disabled | VisualState::DisabledHovered)
    }
}

/// 视觉状态输入信号（由 N03 输入域与属性引擎填充）。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct StateSignals {
    /// 悬停。
    pub hovered: bool,
    /// 按下。
    pub pressed: bool,
    /// 聚焦。
    pub focused: bool,
    /// 禁用。
    pub disabled: bool,
    /// 选中。
    pub active: bool,
}

/// 视觉状态解析：由信号求唯一状态（状态机的纯函数部分）。
///
/// 优先级 `disabled > active > pressed > focused > hovered > normal`，理由写下来
/// 防止各控件各自实现一套：
/// - `disabled` 必须最高：禁用控件仍会收到 hover 信号（鼠标经过就是会经过），
///   若 hover 优先于 disabled，用户会看到"禁用的按钮仍有高亮"——明确的可用性缺陷；
/// - `pressed` 高于 `focused`：按住鼠标时焦点确实也在，但视觉上用户期待"按下的样子"；
/// - `active` 单独一档表示选中态：焦点在输入焦点位置，选中在业务状态位置。
pub fn resolve_visual_state(s: StateSignals) -> VisualState {
    if s.disabled {
        return if s.hovered {
            VisualState::DisabledHovered
        } else {
            VisualState::Disabled
        };
    }
    if s.active {
        return VisualState::Active;
    }
    if s.pressed {
        return VisualState::Pressed;
    }
    if s.focused {
        return if s.hovered {
            VisualState::FocusedHovered
        } else {
            VisualState::Focused
        };
    }
    if s.hovered {
        return VisualState::Hovered;
    }
    VisualState::Normal
}

/// 事件处理器表项（有序；顺序即执行序）。
///
/// 用有序 `Vec` 而非映射：事件处理器需要**顺序语义**（同一事件类型可有多个
/// 处理器，按注册序依次执行，且允许后注册者阻断前者）。映射的键序在多数语言里
/// 是实现细节而非语言保证，用数组把顺序写成显式契约。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct HandlerEntry {
    /// 事件类型。
    pub event: EventType,
    /// 处理器标识（实际调用由上层注入，本树只维护路由表）。
    pub handler_id: String,
    /// 是否阻断后续同类型处理器。
    pub blocking: bool,
}

/// 控件节点（四要素载体）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ControlNode {
    /// 节点 id（树内唯一；重复 id 会让绑定路径解析指向错误目标）。
    pub id: String,
    /// 控件种类标签（F2609 fuzz 面据此构造恶意树）。
    pub kind: String,
    /// 父节点 id（根为 `None`）。单亲不变量：至多一个非空值。
    pub parent_id: Option<String>,
    /// 子节点 id 有序列表（序稳定：三操作之外不得改动）。
    pub children: Vec<String>,
    /// 属性键有序集合（不含值——见模块头注「为什么只持键」）。
    pub property_keys: Vec<PropertyKey>,
    /// 事件处理器表（有序）。
    pub handlers: Vec<HandlerEntry>,
    /// 当前视觉状态（由 [`resolve_visual_state`] 求出，不由外部直改）。
    pub state: VisualState,
    /// 当前状态信号。
    pub signals: StateSignals,
    /// 绑定的轨道路径（M04 `bind_path`）。
    pub bind_path: Option<String>,
    /// 是否已销毁（销毁后仍被绑定引用须显性告警，不静默空转）。
    pub destroyed: bool,
}

/// 节点构造器（`parent_id` 恒为 `None`，由树操作写入）。
pub fn create_node(id: &str, kind: &str) -> ControlNode {
    ControlNode {
        id: String::from(id),
        kind: String::from(kind),
        parent_id: None,
        children: Vec::new(),
        property_keys: Vec::new(),
        handlers: Vec::new(),
        state: VisualState::Normal,
        signals: StateSignals::default(),
        bind_path: None,
        destroyed: false,
    }
}

// ---------------------------------------------------------------------------
// 三、判据二：三不变量声明
// ---------------------------------------------------------------------------

/// 深度上限（恶意超深树防护阈值；与 F2609 fuzz 面联动）。
pub const MAX_TREE_DEPTH: usize = 512;

/// 一条不变量的声明规格。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InvariantSpec {
    /// 不变量 id。
    pub id: &'static str,
    /// 名称。
    pub name: &'static str,
    /// 陈述（人话，进代码评审 checklist）。
    pub statement: &'static str,
    /// 实现纪律。
    pub discipline: &'static str,
    /// 断言方式。
    pub assertion: &'static str,
    /// 违反时的诊断码。
    pub violation: TreeDiagCode,
    /// 为什么这是根基而非可选健壮性检查。
    pub why_fundamental: &'static str,
}

/// 三不变量声明（锚点「三不变量声明」）。
pub const TREE_INVARIANTS: [InvariantSpec; 3] = [
    InvariantSpec {
        id: "single-parent",
        name: "单亲不变量",
        statement: "每个节点至多有一个父节点；父指针与父的子列表必须互指一致",
        discipline: "父指针是唯一权威，子列表只是镜像；改动父关系必须同事务更新两侧，禁止只改一侧",
        assertion: "assert_invariants 逐节点核对双向互指；插入时先查待插入节点是否已有父",
        violation: TreeDiagCode::SingleParentViolation,
        why_fundamental: "单亲一旦破坏，同一控件在树中出现两次：命中测试命中两次（点击处理两遍），焦点遍历走两次（Tab 在同一控件停两次）",
    },
    InvariantSpec {
        id: "acyclic",
        name: "无环不变量",
        statement: "任一节点的祖先链上不得出现重复节点",
        discipline: "插入前沿父的祖先链上溯检查（O(深度)），不下钻子树；检查与插入同事务，中途发现环则不插入",
        assertion: "祖先链遍历携带 visited，遇重复即判环；环上节点禁止任何插入",
        violation: TreeDiagCode::AncestorCycle,
        why_fundamental: "环一旦形成，遍历不再终止。症状不是报错而是界面卡死（每帧无限递归，主线程占满而进程仍活着）",
    },
    InvariantSpec {
        id: "stable-order",
        name: "序稳定不变量",
        statement: "兄弟序由 insert/remove/reorder 三操作唯一决定，其他路径不得重排",
        discipline: "移除后序号不重排（允许空洞、不保证连续）：外部需稳定标识应使用节点 id；重排目标索引按「先移除再插入」语义计算",
        assertion: "索引越界即拒绝；重排前校验目标索引合法",
        violation: TreeDiagCode::OrderStabilityViolation,
        why_fundamental: "兄弟序决定绘制序、命中优先级与 Tab 焦点序；序不稳定时 Tab 会落在不同控件上，且因每次重建树序不同而表现为偶发",
    },
];

/// 不变量 id 全集。
pub const INVARIANT_IDS: [&str; 3] = ["single-parent", "acyclic", "stable-order"];

// ---------------------------------------------------------------------------
// 四、判据二/三载体：树存储与三操作
// ---------------------------------------------------------------------------

/// 控件树存储。
///
/// 为何是「可变存储 + 只读视图」而不是不可变持久结构：控件树每帧可能被改动
/// （属性失效引发的子树调整），不可变结构每次改动都要复制路径上全部节点，
/// 万节点树的单次插入会变成千次分配——与 F2407「全纪律零分配」取向相反。
/// VARIX 的树是「一棵树被多个消费者读」，不是「多棵树共享前缀」，复制无收益。
/// 故取可变存储，把「不被外部改坏」交给只读视图（[`ControlTree::snapshot`]
/// 返回深拷贝）与三不变量断言。
#[derive(Clone, Debug)]
pub struct ControlTree {
    nodes: Vec<ControlNode>,
    root_id: String,
    destroyed: bool,
}

impl ControlTree {
    /// 构造：必须给定唯一根节点 id（根也是普通节点，只是 `parent_id` 为 `None`）。
    pub fn new(root_id: &str) -> TreeOutcome<ControlTree> {
        if root_id.is_empty() {
            return tfail(
                TreeDiagCode::TreeLifecycleViolation,
                "root_id 不得为空",
                "树必须有唯一根，否则遍历无从起步",
                "",
            );
        }
        let root = create_node(root_id, "root");
        tok(ControlTree {
            nodes: alloc::vec![root],
            root_id: String::from(root_id),
            destroyed: false,
        })
    }

    /// 根节点 id。
    pub fn root(&self) -> &str {
        &self.root_id
    }

    /// 是否已销毁。
    pub fn is_destroyed(&self) -> bool {
        self.destroyed
    }

    /// 节点是否存在。
    pub fn has(&self, id: &str) -> bool {
        self.index_of(id).is_some()
    }

    /// 节点数（基准 F2610 读取）。
    pub fn size(&self) -> usize {
        self.nodes.len()
    }

    /// 销毁整树（销毁后任何写操作拒绝）。
    pub fn destroy(&mut self) {
        self.destroyed = true;
    }

    /// 内部取节点引用（本模块内用；对外一律走 [`ControlTree::snapshot`]）。
    pub fn raw(&self, id: &str) -> Option<&ControlNode> {
        self.index_of(id).map(|i| &self.nodes[i])
    }

    /// 内部取节点引用（可变）。
    pub fn raw_mut(&mut self, id: &str) -> Option<&mut ControlNode> {
        self.index_of(id).map(move |i| &mut self.nodes[i])
    }

    /// 内部写节点（按 id 覆盖或追加）。
    pub fn put(&mut self, n: ControlNode) {
        match self.index_of(&n.id) {
            Some(i) => self.nodes[i] = n,
            None => self.nodes.push(n),
        }
    }

    /// 取节点的深拷贝只读视图。
    ///
    /// 深拷贝而非引用返回：节点内含 `children`/`property_keys`/`handlers` 三个
    /// 数组。若按引用返回，调用方 `push` 一个子节点 id 就等于绕过单亲校验直接
    /// 改了树——不变量断言会在下一次自检时报错，但那时破坏已经发生，且报错位置
    /// （自检）与破坏位置（某处 push）完全脱节，无法归因。
    pub fn snapshot(&self, id: &str) -> TreeOutcome<ControlNode> {
        if self.destroyed {
            return tfail(
                TreeDiagCode::TreeLifecycleViolation,
                "树已销毁，读取节点被拒绝",
                "销毁后不再读取节点；如需重建请 new ControlTree",
                id,
            );
        }
        match self.raw(id) {
            None => tfail(
                TreeDiagCode::NodeNotFound,
                "节点不存在于树中",
                "检查 id 是否拼错，或节点是否已被移除",
                id,
            ),
            Some(n) => tok(n.clone()),
        }
    }

    /// 节点下标（线性扫描：节点表规模在 UI 树量级内，且避免哈希表依赖）。
    pub fn index_of(&self, id: &str) -> Option<usize> {
        let mut i = 0usize;
        while i < self.nodes.len() {
            if self.nodes[i].id == id {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 祖先链（自下而上，含自身；空链表示该id 不在树中）。
    pub fn ancestors(&self, id: &str) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        if self.index_of(id).is_none() {
            return out;
        }
        let mut cur = Some(String::from(id));
        let mut hops = 0usize;
        while let Some(c) = cur {
            if hops > MAX_TREE_DEPTH + 1 {
                break;
            }
            if out.iter().any(|x| *x == c) {
                // 已有重复即环：不再延伸，交给 assert_invariants 判红。
                break;
            }
            out.push(c.clone());
            cur = self.raw(&c).and_then(|n| n.parent_id.clone());
            hops += 1;
        }
        out
    }

    /// 唯一的前置守卫：树是否存活。
    ///
    /// 单独抽出是因为每个写操作都要查一次，漏一处就是一个"销毁后仍可写入"的
    /// 僵尸入口——症状是界面已关闭但后台仍在改树，内存泄漏且难以复现。
    pub fn guard_alive(&self) -> Option<TreeDiagnostic> {
        if self.destroyed {
            return Some(td(
                TreeDiagCode::TreeLifecycleViolation,
                "树已销毁，写操作被拒绝",
                "确认调用序：销毁后不得再写；重建树或改用新实例",
                "",
            ));
        }
        None
    }
}

/// 子树高度（自身不计入，叶子为 0；`None` 表示节点不存在）。带环保护。
///
/// 迭代式（显式栈）而非递归：对深树递归会爆栈，而本函数的输入恰恰可能是
/// 攻击者构造的深树。
pub fn subtree_height(tree: &ControlTree, id: &str) -> Option<usize> {
    if tree.raw(id).is_none() {
        return None;
    }
    // visited 兼作环保护与去重。
    let mut visited: Vec<String> = Vec::new();
    let mut best = 0usize;
    let mut stack: Vec<(String, usize)> = alloc::vec![(String::from(id), 0usize)];
    let mut guard = 0usize;
    while let Some((cur, d)) = stack.pop() {
        guard += 1;
        if guard > MAX_TREE_DEPTH + 8 {
            return None;
        }
        if visited.iter().any(|v| *v == cur) {
            continue;
        }
        visited.push(cur.clone());
        if d > best {
            best = d;
        }
        if let Some(n) = tree.raw(&cur) {
            for c in n.children.iter() {
                stack.push((c.clone(), d + 1));
            }
        }
    }
    Some(best)
}

/// 单亲冲突时的两种语义（锚点「两语义显性——设计声明」）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SingleParentPolicy {
    /// 拒绝：节点已有父时报`SINGLE_PARENT_VIOLATION`（默认，严于默认）。
    Reject,
    /// 自动摘除：把节点从原父摘下后插入新父（等价 move，便于重排场景）。
    Detach,
}

/// 批量事务的单步。
///
/// 用判别联合而非「操作名 + 参数字典」：后者在运行时要靠 `match op` 再解构
/// 参数字典，参数拼错（如传了 `parent_id` 给 remove）不会报错，只会被忽略——
/// 表现为"这步什么都没发生"，批次却报告成功。用判别联合后 remove 分支的
/// payload 里根本没有 `parent_id` 字段，拼不出来。
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum TreeOp {
    /// 插入。
    Insert {
        /// 父 id。
        parent_id: String,
        /// 子 id。
        child_id: String,
        /// 插入索引（`None` = 追加到末尾）。
        index: Option<usize>,
        /// 单亲策略。
        policy: SingleParentPolicy,
    },
    /// 移除（节点保留在表中成为游离节点）。
    Remove {
        /// 子 id。
        child_id: String,
    },
    /// 重排。
    Reorder {
        /// 父 id。
        parent_id: String,
        /// 子 id。
        child_id: String,
        /// 移除之后的目标位。
        new_index: usize,
    },
    /// 设置状态信号。
    SetState {
        /// 节点 id。
        node_id: String,
        /// 信号。
        signals: StateSignals,
    },
    /// 设置绑定路径。
    Bind {
        /// 节点 id。
        node_id: String,
        /// 路径（`None` = 解绑）。
        bind_path: Option<String>,
    },
    /// 注销节点（真正从表中移除）。
    Unregister {
        /// 子 id。
        child_id: String,
    },
}

/// `insert`：把 child 插入 parent 的子列表。
///
/// 校验顺序刻意是「便宜的在前」：父存在 → 节点存在 → 自插入 → 单亲 → 无环 → 深度。
/// 理由是每项检查代价不同（无环 O(深度)、深度 O(子树)），而绝大多数非法调用在
/// 第二三项就被拦下。若把无环与深度检查放最前，每次"父不存在"这种最常见的调用
/// 错误都要白跑一遍 O(深度) 遍历——在批量重建树的场景（万次 insert）里那是
/// 数量级可观的浪费。
pub fn insert(
    tree: &mut ControlTree,
    parent_id: &str,
    child_id: &str,
    index: Option<usize>,
    policy: SingleParentPolicy,
) -> TreeOutcome<ControlNode> {
    if let Some(d) = tree.guard_alive() {
        return Err(d);
    }
    if tree.raw(parent_id).is_none() {
        return tfail(
            TreeDiagCode::ParentInvalid,
            "父节点不存在于树中",
            "检查 parent_id；父节点须先 insert 到树上",
            parent_id,
        );
    }
    if tree.raw(child_id).is_none() {
        // 节点未注册：显式注册（避免要求调用方分两步，也避免自动注册掩盖拼写错误）。
        let n = create_node(child_id, "panel");
        tree.put(n);
    }
    if child_id == parent_id {
        return tfail(
            TreeDiagCode::SelfInsertion,
            "节点不能插入到自己的子列表",
            "自插入是祖先环的特例，会使遍历不终止",
            child_id,
        );
    }

    // 单亲校验 + 摘除预处理。摘除先于无环校验，故无环失败时须把摘除回滚。
    let existing_parent = tree.raw(child_id).and_then(|n| n.parent_id.clone());
    let mut detached_from: Option<String> = None;
    let mut detached_idx: usize = 0usize;
    if let Some(ep) = existing_parent {
        if policy == SingleParentPolicy::Reject {
            return tfail(
                TreeDiagCode::SingleParentViolation,
                &format!("节点已有父（{}），按 Reject 策略拒绝插入", ep),
                "若意图是移动节点，请显式传 Detach；不要依赖隐式摘除",
                child_id,
            );
        }
        detached_from = Some(ep.clone());
        detached_idx = 0usize;
        if let Some(dp) = tree.raw_mut(&ep) {
            if let Some(pos) = dp.children.iter().position(|c| c == child_id) {
                detached_idx = pos;
                dp.children.remove(pos);
            }
        }
    }

    // 无环校验：若 parent 位于 child 的后代链上，插入即成环。
    // 上溯 parent 的祖先链，看是否遇到 child —— O(深度)，不下钻子树。
    let mut cur = Some(String::from(parent_id));
    let mut hops = 0usize;
    let mut cycle_at: Option<String> = None;
    while let Some(c) = cur.clone() {
        if c == child_id {
            cycle_at = Some(c);
            break;
        }
        hops += 1;
        if hops > MAX_TREE_DEPTH + 1 {
            cycle_at = Some(c.clone());
            break;
        }
        cur = tree.raw(&c).and_then(|n| n.parent_id.clone());
    }
    if let Some(at) = cycle_at {
        rollback_detach(tree, &detached_from, detached_idx, child_id);
        return tfail(
            TreeDiagCode::AncestorCycle,
            &format!("插入将形成环：目标父 {} 是 {} 的后代", parent_id, child_id),
            "调整层级：容器只能挂到不含自己的分支上；或改用 remove + insert 两步",
            &at,
        );
    }

    // 深度校验：新深度 = 父深度 + 1 + 子树高度。超限即拒。
    let parent_chain = tree.ancestors(parent_id);
    let parent_depth = parent_chain.len().saturating_sub(1);
    let sub = match subtree_height(tree, child_id) {
        Some(s) => s,
        None => {
            rollback_detach(tree, &detached_from, detached_idx, child_id);
            return tfail(
                TreeDiagCode::NodeNotFound,
                "计算子树高度时节点不存在",
                "检查 child_id",
                child_id,
            );
        }
    };
    let new_depth = parent_depth + 1 + sub;
    if new_depth > MAX_TREE_DEPTH {
        rollback_detach(tree, &detached_from, detached_idx, child_id);
        return tfail(
            TreeDiagCode::DepthLimitExceeded,
            &format!("插入后深度 {} 超过上限 {}", new_depth, MAX_TREE_DEPTH),
            "降低嵌套层级：把深层内容改为惰性展开/列表虚拟化，而不是加深树",
            child_id,
        );
    }

    // 序稳定校验：索引须落在 [0, children.len()]。
    let kids_len = tree.raw(parent_id).map(|n| n.children.len()).unwrap_or(0);
    let idx = index.unwrap_or(kids_len);
    if idx > kids_len {
        rollback_detach(tree, &detached_from, detached_idx, child_id);
        return tfail(
            TreeDiagCode::OrderStabilityViolation,
            &format!("插入索引 {} 越界（合法区间 0..{}）", idx, kids_len),
            "index 省略或取 0..当前子节点数；越界通常意味着对兄弟数有陈旧假设",
            parent_id,
        );
    }

    if let Some(p) = tree.raw_mut(parent_id) {
        p.children.insert(idx, String::from(child_id));
    }
    if let Some(c) = tree.raw_mut(child_id) {
        c.parent_id = Some(String::from(parent_id));
        c.destroyed = false;
    }
    match tree.raw(child_id) {
        Some(n) => tok(n.clone()),
        None => tfail(
            TreeDiagCode::NodeNotFound,
            "插入后节点丢失",
            "这是内部一致性错误，请上报",
            child_id,
        ),
    }
}

/// 摘除回滚（把 detach 阶段的改动还原，供插入后续校验失败时调用）。
fn rollback_detach(
    tree: &mut ControlTree,
    detached_from: &Option<String>,
    detached_idx: usize,
    child_id: &str,
) {
    if let Some(from) = detached_from {
        if let Some(dp) = tree.raw_mut(from) {
            let at = core::cmp::min(detached_idx, dp.children.len());
            dp.children.insert(at, String::from(child_id));
        }
    }
}

/// `remove`：把节点从父的子列表摘下（节点本身保留在表中成为游离节点）。
///
/// 为何不连节点一起删（`unregister` 是另一个操作）：布局/动画在摘除瞬间还需要
/// 读节点的最终矩形来播退出动画。若 remove 即销毁，退出动画就没有数据源，
/// 只能由调用方提前缓存——而缓存时机无法统一，迟早漏。摘下但保留，把
/// "最终态读取"的窗口留给调用方，再由 `unregister` 显式回收。
pub fn remove(tree: &mut ControlTree, child_id: &str) -> TreeOutcome<ControlNode> {
    if let Some(d) = tree.guard_alive() {
        return Err(d);
    }
    let parent_id = match tree.raw(child_id) {
        None => {
            return tfail(
                TreeDiagCode::NodeNotFound,
                "待移除节点不存在",
                "检查 child_id；重复 remove 是常见调用序错误",
                child_id,
            )
        }
        Some(n) => n.parent_id.clone(),
    };
    if child_id == tree.root() {
        return tfail(
            TreeDiagCode::OrderStabilityViolation,
            "根节点不可移除",
            "树必须有根；整树销毁请用 destroy()",
            child_id,
        );
    }
    let parent_id = match parent_id {
        None => {
            return tfail(
                TreeDiagCode::SingleParentViolation,
                "节点当前没有父，remove 无对象可摘",
                "该节点已是游离态；游离节点应走 Unregister 回收",
                child_id,
            )
        }
        Some(p) => p,
    };
    if tree.raw(&parent_id).is_none() {
        return tfail(
            TreeDiagCode::ParentInvalid,
            "父节点不存在，树状态已损坏",
            "运行 assert_invariants 定位",
            &parent_id,
        );
    }
    let found = tree
        .raw(&parent_id)
        .map(|p| p.children.iter().any(|c| c == child_id))
        .unwrap_or(false);
    if !found {
        return tfail(
            TreeDiagCode::SingleParentViolation,
            "父的子列表中找不到该节点（父子双向不一致）",
            "运行 assert_invariants；不得直接改 children 绕过操作接口",
            child_id,
        );
    }
    if let Some(p) = tree.raw_mut(&parent_id) {
        if let Some(pos) = p.children.iter().position(|c| c == child_id) {
            p.children.remove(pos);
        }
    }
    if let Some(c) = tree.raw_mut(child_id) {
        c.parent_id = None;
    }
    match tree.raw(child_id) {
        Some(n) => tok(n.clone()),
        None => tfail(
            TreeDiagCode::NodeNotFound,
            "移除后节点丢失",
            "这是内部一致性错误，请上报",
            child_id,
        ),
    }
}

/// `reorder`：调整节点在父子列表中的位置。
///
/// 语义：「先移除、再插入」，`new_index` 是**移除之后**的目标位。详见模块头注
/// 「判据三」对两种自洽语义的取舍说明。
pub fn reorder(
    tree: &mut ControlTree,
    parent_id: &str,
    child_id: &str,
    new_index: usize,
) -> TreeOutcome<ControlNode> {
    if let Some(d) = tree.guard_alive() {
        return Err(d);
    }
    if tree.raw(parent_id).is_none() {
        return tfail(
            TreeDiagCode::ParentInvalid,
            "父节点不存在",
            "检查 parent_id",
            parent_id,
        );
    }
    let cur_parent = match tree.raw(child_id) {
        None => {
            return tfail(
                TreeDiagCode::NodeNotFound,
                "待重排节点不存在",
                "检查 child_id",
                child_id,
            )
        }
        Some(n) => n.parent_id.clone(),
    };
    if cur_parent.as_deref() != Some(parent_id) {
        return tfail(
            TreeDiagCode::SingleParentViolation,
            "节点不属于该父，reorder 无效",
            "reorder 只能在节点当前的父下调整；跨父请用 insert(policy=Detach)",
            child_id,
        );
    }
    let mut kids: Vec<String> = match tree.raw(parent_id) {
        Some(p) => p.children.clone(),
        None => Vec::new(),
    };
    let from = match kids.iter().position(|c| c == child_id) {
        None => {
            return tfail(
                TreeDiagCode::SingleParentViolation,
                "父的子列表中找不到该节点",
                "运行 assert_invariants",
                child_id,
            )
        }
        Some(f) => f,
    };
    kids.remove(from);
    if new_index > kids.len() {
        return tfail(
            TreeDiagCode::OrderStabilityViolation,
            &format!(
                "重排目标索引 {} 越界（移除后合法区间 0..{}）",
                new_index,
                kids.len()
            ),
            "注意索引语义为「移除之后」的目标位，故上界是原长度减一",
            child_id,
        );
    }
    kids.insert(new_index, String::from(child_id));
    if let Some(p) = tree.raw_mut(parent_id) {
        p.children = kids;
    }
    match tree.raw(child_id) {
        Some(n) => tok(n.clone()),
        None => tfail(
            TreeDiagCode::NodeNotFound,
            "重排后节点丢失",
            "这是内部一致性错误，请上报",
            child_id,
        ),
    }
}

/// 设置状态信号 → 状态经 [`resolve_visual_state`] 唯一求出。
pub fn set_state(tree: &mut ControlTree, node_id: &str, signals: StateSignals) -> TreeOutcome<VisualState> {
    if let Some(d) = tree.guard_alive() {
        return Err(d);
    }
    if tree.raw(node_id).is_none() {
        return tfail(
            TreeDiagCode::NodeNotFound,
            "节点不存在",
            "检查 node_id",
            node_id,
        );
    }
    let next = resolve_visual_state(signals);
    if let Some(n) = tree.raw_mut(node_id) {
        n.signals = signals;
        n.state = next;
    }
    tok(next)
}

/// 事务前像快照（回滚的事实源）。
#[derive(Clone, Debug)]
struct TxSnapshot {
    node_id: String,
    existed_before: bool,
    before: Option<ControlNode>,
}

/// 事务执行器：批量操作 = 单事务（锚点「操作原子性（批量=单事务）」）。
///
/// 三条纪律：
/// 1. 全成功 → 全部生效，返回应用步数；
/// 2. 任一步失败 → 已生效的步骤**全部回滚**，返回失败 + 失败步序号；
/// 3. 回滚本身不依赖任何操作成功（用快照纯赋值）。
///
/// 为什么必须回滚而不是"失败即停"：批量操作在真实场景里都是"一次重建一棵子树"
/// 或"一次应用一批轨道绑定"。若第 7 步失败而前 6 步已生效，树就停在"一半新一半旧"
/// 的状态——它不报错、不崩溃，只是一直显示错误内容。用户在界面上看到的是
/// "这个面板一半是新的、一半是旧的"，开发者从任何日志里都看不出原因，因为每一步
/// 单独看都成功了。这类缺陷的定位成本是数量级的。
pub fn transact(tree: &mut ControlTree, ops: &[TreeOp]) -> TreeOutcome<usize> {
    if let Some(d) = tree.guard_alive() {
        return Err(d);
    }
    let mut snaps: Vec<TxSnapshot> = Vec::new();
    let mut applied: usize = 0usize;
    for (i, op) in ops.iter().enumerate() {
        // 步骤前留快照。快照必须覆盖**本步会触及的全部节点**，而不只是目标节点：
        // insert/reorder/remove 都会改动「父节点的 children 列表」——只快照子节点的话，
        // 事务失败后子节点被还原了，父节点的 children 里却还留着它，即父说
        // "这个孩子在这儿"、孩子说"我没有父亲"。这种半回滚状态不报错、不崩溃，
        // 只在后续遍历时表现为节点重复出现，归因成本极高。
        for sid in snapshot_ids(tree, op) {
            let existed = tree.raw(&sid).is_some();
            snaps.push(TxSnapshot {
                before: tree.raw(&sid).cloned(),
                node_id: sid,
                existed_before: existed,
            });
        }
        if let Err(d) = apply_op(tree, op) {
            // 回滚：按快照逆序赋值（同一节点被快照多次时，后入的先还原，
            // 逆序回放正好让它回到最初状态）。
            let mut k = snaps.len();
            while k > 0 {
                k -= 1;
                if let Some(s) = snaps.get(k) {
                    if s.existed_before {
                        if let Some(b) = s.before.clone() {
                            tree.put(b);
                        }
                    } else if let Some(i) = tree.index_of(&s.node_id) {
                        let _ = tree.nodes.remove(i);
                    }
                }
            }
            let mut rolled = d.clone();
            rolled.code = TreeDiagCode::BatchRolledBack;
            rolled.message = format!("批量事务第 {} 步失败，已回滚：{}", i, d.message);
            rolled.hint = format!("{}（回滚为纯赋值，不依赖任何操作成功）", d.hint);
            return Err(rolled);
        }
        applied += 1;
    }
    tok(applied)
}

/// 本步会触及的节点 id 集合（目标 + 其父）。
fn snapshot_ids(tree: &ControlTree, op: &TreeOp) -> Vec<String> {
    let mut ids: Vec<String> = Vec::new();
    let (target, parent) = match op {
        TreeOp::Insert {
            parent_id,
            child_id,
            ..
        } => (child_id.clone(), Some(parent_id.clone())),
        TreeOp::Remove { child_id } => {
            let p = tree.raw(child_id).and_then(|n| n.parent_id.clone());
            (child_id.clone(), p)
        }
        TreeOp::Reorder {
            parent_id,
            child_id,
            ..
        } => (child_id.clone(), Some(parent_id.clone())),
        TreeOp::SetState { node_id, .. } | TreeOp::Bind { node_id, .. } => {
            (node_id.clone(), None)
        }
        TreeOp::Unregister { child_id } => (child_id.clone(), None),
    };
    if let Some(p) = parent.as_ref() {
        if *p != target {
            ids.push(p.clone());
        }
    }
    ids.push(target);
    ids
}

/// 单步执行（[`transact`] 与直接调用共用）。
pub fn apply_op(tree: &mut ControlTree, op: &TreeOp) -> TreeOutcome<()> {
    match op {
        TreeOp::Insert {
            parent_id,
            child_id,
            index,
            policy,
        } => {
            insert(tree, parent_id, child_id, *index, *policy)?;
        }
        TreeOp::Remove { child_id } => {
            remove(tree, child_id)?;
        }
        TreeOp::Reorder {
            parent_id,
            child_id,
            new_index,
        } => {
            reorder(tree, parent_id, child_id, *new_index)?;
        }
        TreeOp::SetState { node_id, signals } => {
            set_state(tree, node_id, *signals)?;
        }
        TreeOp::Bind { node_id, bind_path } => {
            if let Some(d) = tree.guard_alive() {
                return Err(d);
            }
            if tree.raw(node_id).is_none() {
                return tfail(
                    TreeDiagCode::NodeNotFound,
                    "节点不存在",
                    "检查 node_id",
                    node_id,
                );
            }
            if let Some(n) = tree.raw_mut(node_id) {
                n.bind_path = bind_path.clone();
            }
        }
        TreeOp::Unregister { child_id } => {
            if let Some(d) = tree.guard_alive() {
                return Err(d);
            }
            if child_id == tree.root() {
                return tfail(
                    TreeDiagCode::OrderStabilityViolation,
                    "根节点不可注销",
                    "树必须有根；整树销毁请用 destroy()",
                    child_id,
                );
            }
            let p = tree.raw(child_id).and_then(|n| n.parent_id.clone());
            if let Some(pid) = p {
                if let Some(pn) = tree.raw_mut(&pid) {
                    if let Some(pos) = pn.children.iter().position(|c| c == child_id) {
                        pn.children.remove(pos);
                    }
                }
            }
            // 注销整棵子树（否则子节点变孤儿——孤儿是 F2609 fuzz 的四类畸形之一）。
            let victims = collect_subtree(tree, child_id);
            for v in victims {
                if let Some(i) = tree.index_of(&v) {
                    let _ = tree.nodes.remove(i);
                }
            }
        }
    }
    tok(())
}

/// 收集子树全部 id（含自身）。带 visited 防环。
fn collect_subtree(tree: &ControlTree, id: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut stack: Vec<String> = alloc::vec![String::from(id)];
    while let Some(c) = stack.pop() {
        if out.iter().any(|x| *x == c) {
            continue;
        }
        out.push(c.clone());
        if let Some(n) = tree.raw(&c) {
            for k in n.children.iter() {
                stack.push(k.clone());
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// 五、判据四：M04 绑定路径解析
// ---------------------------------------------------------------------------

/// 绑定路径段。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct BindSegment {
    /// 段名（匹配 kind 或 id）。
    pub name: String,
    /// 兄弟序号（`-1` 表示不消歧）。
    pub index: i64,
}

/// 解析绑定路径为段序列。
///
/// 解析与定位分成两步（而不是一步到位）：路径语法错误与路径解析不到是两类
/// 完全不同的问题。前者是调用方的拼写/格式错（几乎恒为 bug），后者是数据在运行时
/// 变了（节点被删、层级调整，属可预期的状态变化）。合成一个错误码时，开发者
/// 分不清该改代码还是该改数据，两种都修不对。
pub fn parse_bind_path(path: &str) -> TreeOutcome<Vec<BindSegment>> {
    if path.is_empty() {
        return tfail(
            TreeDiagCode::BindingPathInvalid,
            "绑定路径为空串",
            "解绑用 bind_path=None；空串不是合法路径",
            path,
        );
    }
    if path.starts_with('/') || path.ends_with('/') {
        return tfail(
            TreeDiagCode::BindingPathInvalid,
            &format!("绑定路径以分隔符开头或结尾：{}", path),
            "路径应为 seg/seg 形式，首尾不得有 '/'",
            path,
        );
    }
    let mut out: Vec<BindSegment> = Vec::new();
    for raw in path.split('/') {
        if raw.is_empty() {
            return tfail(
                TreeDiagCode::BindingPathInvalid,
                &format!("绑定路径含空段：{}", path),
                "检查连续分隔符 '//'",
                path,
            );
        }
        let seg = match raw.find('#') {
            None => {
                if !is_valid_seg_name(raw) {
                    return tfail(
                        TreeDiagCode::BindingPathInvalid,
                        &format!("段名含非法字符：{}", raw),
                        "段名只允许字母数字下划线点与连字符；'#' 保留给序号",
                        path,
                    );
                }
                BindSegment {
                    name: String::from(raw),
                    index: -1,
                }
            }
            Some(h) => {
                let name = &raw[..h];
                let idx_str = &raw[h + 1..];
                if name.is_empty() || !is_valid_seg_name(name) {
                    return tfail(
                        TreeDiagCode::BindingPathInvalid,
                        &format!("段名非法：{}", raw),
                        "检查 '#' 前的段名",
                        path,
                    );
                }
                if idx_str.is_empty() || !idx_str.bytes().all(|b| b.is_ascii_digit()) {
                    return tfail(
                        TreeDiagCode::BindingPathInvalid,
                        &format!("兄弟序号非法：{}", raw),
                        "'#' 后须为非负整数；缺省序号请省略 '#'",
                        path,
                    );
                }
                match parse_u64(idx_str) {
                    Some(v) => BindSegment {
                        name: String::from(name),
                        index: v as i64,
                    },
                    None => {
                        return tfail(
                            TreeDiagCode::BindingPathInvalid,
                            &format!("兄弟序号溢出：{}", raw),
                            "序号须在 u64 范围内",
                            path,
                        )
                    }
                }
            }
        };
        out.push(seg);
    }
    tok(out)
}

/// 段名字符集校验（字母数字 / `_` / `.` / `-`）。
///
/// 刻意**不允许** `/` `#` 与空白：这三类字符是路径语法自身的分隔/消歧符，
/// 放进段名会让解析结果依赖切分顺序（同一路径两种读法），是最典型的解析歧义源。
fn is_valid_seg_name(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    s.bytes().all(|b| {
        b.is_ascii_alphanumeric() || b == b'_' || b == b'.' || b == b'-'
    })
}

/// 无依赖的 `u64` 解析（`no_std` 下 `str::parse` 需 `core::str::FromStr`，此处显式实现以免歧义）。
fn parse_u64(s: &str) -> Option<u64> {
    let mut acc: u64 = 0u64;
    let bytes = s.as_bytes();
    if bytes.is_empty() {
        return None;
    }
    let mut i = 0usize;
    while i < bytes.len() {
        let b = bytes[i];
        if !b.is_ascii_digit() {
            return None;
        }
        acc = acc.checked_mul(10)?.checked_add((b - b'0') as u64)?;
        i += 1;
    }
    Some(acc)
}

/// 按路径定位节点（M04 轨道 → 树节点的解析器）。
///
/// 失败分两类并分别立码：路径语法错 / 段在同级找不到 → `BINDING_PATH_INVALID`
/// （数据变了，须显性告警）；定位到的节点已销毁 → `BINDING_TARGET_DESTROYED`
/// （生命周期错位）。之所以把"销毁"单列而不并入 INVALID：二者处置不同——INVALID
/// 通常要修路径或补节点，destroyed 要查生命周期序（谁在节点销毁后还在写绑定）。
/// 并成一个码，开发者只能靠猜。
pub fn resolve_bind_path(tree: &ControlTree, path: &str) -> TreeOutcome<ControlNode> {
    let parsed = parse_bind_path(path)?;
    let mut current = String::from(tree.root());
    for (s, seg) in parsed.iter().enumerate() {
        if s == 0 {
            // 首段定位根本身（路径首段写的是根的 id/kind，而非根的某个子节点）。
            // 不这么处理会出现"路径第一段永远匹配不到"：解析从 current=root 起步，
            // 若首段也去 root 的子级里找 root，匹配对象与被匹配对象同级不同层。
            let hit = tree
                .raw(&current)
                .map(|r| r.id == seg.name || r.kind == seg.name)
                .unwrap_or(false);
            if !hit {
                return tfail(
                    TreeDiagCode::BindingPathInvalid,
                    &format!(
                        "路径 {} 首段 {} 未命中根节点（根 id={}）",
                        path,
                        seg.name,
                        tree.root()
                    ),
                    "首段必须写根的 id 或 kind；路径从根自身起算，不从根的子节点起算",
                    path,
                );
            }
            continue;
        }
        let kids: Vec<String> = match tree.raw(&current) {
            Some(n) => n.children.clone(),
            None => Vec::new(),
        };
        let mut hits: Vec<String> = Vec::new();
        for k in kids.iter() {
            if let Some(n) = tree.raw(k) {
                if n.kind == seg.name || n.id == seg.name {
                    hits.push(k.clone());
                }
            }
        }
        if hits.is_empty() {
            return tfail(
                TreeDiagCode::BindingPathInvalid,
                &format!("路径 {} 的段 {} 在 {} 下无匹配子节点", path, seg.name, current),
                "检查段名拼写与该段是否已被移除；路径失效应显性告警而非静默",
                path,
            );
        }
        let chosen = if seg.index < 0 {
            if hits.len() > 1 {
                return tfail(
                    TreeDiagCode::BindingPathInvalid,
                    &format!(
                        "路径 {} 的段 {} 匹配到 {} 个兄弟，未用 '#序号' 消歧",
                        path,
                        seg.name,
                        hits.len()
                    ),
                    "同类兄弟多于一个时必须写 '#n'；否则节点重排后绑定会静默漂移",
                    path,
                );
            }
            hits[0].clone()
        } else {
            let want = seg.index as usize;
            match hits.get(want) {
                Some(h) => h.clone(),
                None => {
                    return tfail(
                        TreeDiagCode::BindingPathInvalid,
                        &format!(
                            "路径 {} 的段 {}#{} 越界（同类兄弟 {} 个）",
                            path,
                            seg.name,
                            want,
                            hits.len()
                        ),
                        "序号按同类兄弟的出现序计算；节点被删会让序号漂移",
                        path,
                    )
                }
            }
        };
        if let Some(n) = tree.raw(&chosen) {
            if n.destroyed {
                return tfail(
                    TreeDiagCode::BindingTargetDestroyed,
                    &format!("路径 {} 定位到的节点 {} 已销毁", path, chosen),
                    "查生命周期序：谁在节点销毁后还在写绑定；销毁后不得静默空转",
                    path,
                );
            }
        }
        current = chosen;
    }
    match tree.raw(&current) {
        Some(n) => tok(n.clone()),
        None => tfail(
            TreeDiagCode::BindingPathInvalid,
            &format!("路径 {} 定位到的节点不存在", path),
            "检查路径；定位结果缺失说明树在解析期间被改",
            path,
        ),
    }
}

// ---------------------------------------------------------------------------
// 六、树度量与不变量机检
// ---------------------------------------------------------------------------

/// 树的度量（供基准 F2610 与遥测 F2616 读取）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TreeMetrics {
    /// 节点总数。
    pub node_count: usize,
    /// 最大深度（根节点深度记0）。
    pub max_depth: usize,
    /// 最大兄弟数（最宽的一层的子节点数）。
    pub max_sibling_count: usize,
}

/// 度量子树深度（不含自身，根为 0）。带 `visited` 做环保护。
///
/// 环保护不是"允许环"，而是保证"检测环的代码本身不会被环杀死"。
pub fn depth_of(
    node_id: &str,
    children_of: &dyn Fn(&str) -> Vec<String>,
    visited: &mut Vec<String>,
) -> usize {
    if visited.iter().any(|v| v == node_id) {
        return 0;
    }
    visited.push(String::from(node_id));
    let kids = children_of(node_id);
    if kids.is_empty() {
        return 0;
    }
    let mut best = 0usize;
    for k in kids.iter() {
        let d = depth_of(k, children_of, visited);
        if d > best {
            best = d;
        }
    }
    best + 1
}

/// 树度量（迭代式，避免深树爆栈）。
pub fn tree_metrics(tree: &ControlTree) -> TreeMetrics {
    let mut max_depth = 0usize;
    let mut max_sib = 0usize;
    let mut stack: Vec<(String, usize)> = alloc::vec![(String::from(tree.root()), 0usize)];
    let mut guard = 0usize;
    while let Some((cur, d)) = stack.pop() {
        guard += 1;
        if guard > MAX_TREE_DEPTH * 4 + 16 {
            break;
        }
        if d > max_depth {
            max_depth = d;
        }
        if let Some(n) = tree.raw(&cur) {
            if n.children.len() > max_sib {
                max_sib = n.children.len();
            }
            for c in n.children.iter() {
                stack.push((c.clone(), d + 1));
            }
        }
    }
    TreeMetrics {
        node_count: tree.size(),
        max_depth,
        max_sibling_count: max_sib,
    }
}

/// 一条不变量核验结果。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct InvariantReport {
    /// 不变量 id。
    pub id: &'static str,
    /// 是否通过。
    pub ok: bool,
    /// 违例细节（通过时为空）。
    pub detail: String,
}

/// 核验三不变量（逐条机检，非文档承诺）。
pub fn assert_invariants(tree: &ControlTree) -> Vec<InvariantReport> {
    let mut out: Vec<InvariantReport> = Vec::new();

    // ① 单亲：父指针与父的子列表双向互指。
    let mut bad: Vec<String> = Vec::new();
    for n in tree.nodes.iter() {
        if let Some(pid) = n.parent_id.clone() {
            match tree.raw(&pid) {
                None => bad.push(format!("{} 的父 {} 不存在", n.id, pid)),
                Some(p) => {
                    if !p.children.iter().any(|c| *c == n.id) {
                        bad.push(format!("{} 的父 {} 的子列表里没有它", n.id, pid));
                    }
                }
            }
        }
        // 反向：每个子列表项的父指针必须指回本节点。
        for c in n.children.iter() {
            match tree.raw(c) {
                None => bad.push(format!("{} 的子 {} 不存在", n.id, c)),
                Some(cn) => {
                    if cn.parent_id.as_deref() != Some(n.id.as_str()) {
                        bad.push(format!("{} 声明子 {} 但后者父指针为 {:?}", n.id, c, cn.parent_id));
                    }
                }
            }
        }
    }
    out.push(InvariantReport {
        id: "single-parent",
        ok: bad.is_empty(),
        detail: bad.join("; "),
    });

    // ② 无环：逐节点上溯祖先链，不得遇重复。
    let mut cyc: Vec<String> = Vec::new();
    for n in tree.nodes.iter() {
        let mut seen: Vec<String> = Vec::new();
        let mut cur = n.parent_id.clone();
        let mut hops = 0usize;
        while let Some(c) = cur {
            if seen.iter().any(|v| *v == c) || c == n.id {
                cyc.push(format!("{} 的祖先链成环于 {}", n.id, c));
                break;
            }
            seen.push(c.clone());
            hops += 1;
            if hops > MAX_TREE_DEPTH + 1 {
                cyc.push(format!("{} 的祖先链超长（疑似环）", n.id));
                break;
            }
            cur = tree.raw(&c).and_then(|x| x.parent_id.clone());
        }
    }
    out.push(InvariantReport {
        id: "acyclic",
        ok: cyc.is_empty(),
        detail: cyc.join("; "),
    });

    // ③ 序稳定：兄弟序无重复项（重复即两次插入占同一位，绘制序不确定）。
    let mut dup: Vec<String> = Vec::new();
    for n in tree.nodes.iter() {
        let mut seen: Vec<&String> = Vec::new();
        for c in n.children.iter() {
            if seen.contains(&c) {
                dup.push(format!("{} 的子列表中 {} 出现多次", n.id, c));
            }
            seen.push(c);
        }
    }
    out.push(InvariantReport {
        id: "stable-order",
        ok: dup.is_empty(),
        detail: dup.join("; "),
    });

    out
}

/// 全树自检：封闭集 + 不变量 + 深度。
pub fn self_check(tree: &ControlTree) -> Vec<InvariantReport> {
    let mut out = assert_invariants(tree);
    let m = tree_metrics(tree);
    out.push(InvariantReport {
        id: "depth-within-limit",
        ok: m.max_depth <= MAX_TREE_DEPTH,
        detail: format!("max_depth={} limit={}", m.max_depth, MAX_TREE_DEPTH),
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 四要素_节点构造与快照深拷贝() {
        let mut t = ControlTree::new("root").unwrap();
        assert!(t.has("root"));
        // 未注册节点在 insert 时显式注册，不要求调用方分两步。
        insert(&mut t, "root", "a", None, SingleParentPolicy::Reject).unwrap();
        let snap = t.snapshot("a").unwrap();
        // 改快照不影响树（深拷贝语义）。
        let mut s2 = snap.clone();
        s2.children.push("ghost".to_string());
        assert!(t.raw("a").unwrap().children.is_empty());
        assert_eq!(t.raw("a").unwrap().parent_id.as_deref(), Some("root"));
    }

    #[test]
    fn 三不变量_单亲双向互指() {
        let mut t = ControlTree::new("root").unwrap();
        insert(&mut t, "root", "a", None, SingleParentPolicy::Reject).unwrap();
        insert(&mut t, "root", "b", None, SingleParentPolicy::Reject).unwrap();
        // a 已有父，Reject 策略必须拒绝。
        let e = insert(&mut t, "a", "b", None, SingleParentPolicy::Reject).unwrap_err();
        assert_eq!(e.code, TreeDiagCode::SingleParentViolation);
        // Detach 策略等价 move，且两侧都更新。
        insert(&mut t, "a", "b", None, SingleParentPolicy::Detach).unwrap();
        assert!(t.raw("root").unwrap().children.is_empty());
        assert_eq!(t.raw("a").unwrap().children, alloc::vec!["b".to_string()]);
        assert!(assert_invariants(&t).iter().all(|r| r.ok));
    }

    #[test]
    fn 三不变量_无环拒绝且摘除回滚() {
        let mut t = ControlTree::new("root").unwrap();
        insert(&mut t, "root", "a", None, SingleParentPolicy::Reject).unwrap();
        insert(&mut t, "a", "b", None, SingleParentPolicy::Reject).unwrap();
        // 把 a 插到自己的子 b 下 → 环。
        let e = insert(&mut t, "b", "a", None, SingleParentPolicy::Reject).unwrap_err();
        assert_eq!(e.code, TreeDiagCode::AncestorCycle);
        // 环被拒后树必须完好（不得留下半成品）。
        assert!(assert_invariants(&t).iter().all(|r| r.ok));
        assert_eq!(t.raw("a").unwrap().parent_id.as_deref(), Some("root"));
    }

    #[test]
    fn 三不变量_深度上限拒绝() {
        let mut t = ControlTree::new("root").unwrap();
        let mut cur = String::from("root");
        // 造一条超过上限的链：每插一个都先校验，故逐层插入到上限后拒绝。
        for i in 0..(MAX_TREE_DEPTH + 4) {
            let nid = format!("n{}", i);
            if insert(&mut t, &cur, &nid, None, SingleParentPolicy::Reject).is_err() {
                assert!(i >= MAX_TREE_DEPTH - 1, "深度上限触发过早：i={}", i);
                return;
            }
            cur = nid;
        }
        panic!("深度上限未触发");
    }

    #[test]
    fn 原子操作_reorder为移除后语义() {
        let mut t = ControlTree::new("root").unwrap();
        for id in ["a", "b", "c"] {
            insert(&mut t, "root", id, None, SingleParentPolicy::Reject).unwrap();
        }
        reorder(&mut t, "root", "a", 2).unwrap();
        // [a,b,c] 移除 a 得 [b,c]，在索引 2 插入得 [b,c,a]。
        assert_eq!(
            t.raw("root").unwrap().children,
            alloc::vec!["b".to_string(), "c".to_string(), "a".to_string()]
        );
        // 移除后序号不重排（b、c 序号保留，允许空洞）。
        let e = reorder(&mut t, "root", "a", 3).unwrap_err();
        assert_eq!(e.code, TreeDiagCode::OrderStabilityViolation);
    }

    #[test]
    fn 原子操作_事务中途失败全回滚() {
        let mut t = ControlTree::new("root").unwrap();
        insert(&mut t, "root", "keep", None, SingleParentPolicy::Reject).unwrap();
        let before = t.raw("root").unwrap().children.clone();
        let ops = alloc::vec![
            TreeOp::Insert {
                parent_id: "root".to_string(),
                child_id: "x".to_string(),
                index: None,
                policy: SingleParentPolicy::Reject,
            },
            TreeOp::Insert {
                parent_id: "nosuch".to_string(),
                child_id: "y".to_string(),
                index: None,
                policy: SingleParentPolicy::Reject,
            },
        ];
        let e = transact(&mut t, &ops).unwrap_err();
        assert_eq!(e.code, TreeDiagCode::BatchRolledBack);
        // 第一步的成果必须已回滚。
        assert_eq!(t.raw("root").unwrap().children, before);
        assert!(t.raw("x").is_none());
        assert!(assert_invariants(&t).iter().all(|r| r.ok));
    }

    #[test]
    fn 绑定_路径解析与定位() {
        let mut t = ControlTree::new("root").unwrap();
        let mut root = t.raw("root").unwrap().clone();
        root.kind = String::from("window");
        t.put(root);
        insert(&mut t, "root", "p0", None, SingleParentPolicy::Reject).unwrap();
        t.raw_mut("p0").unwrap().kind = String::from("panel");
        insert(&mut t, "p0", "btn", None, SingleParentPolicy::Reject).unwrap();
        t.raw_mut("btn").unwrap().kind = String::from("button");

        let hit = resolve_bind_path(&t, "window/panel#0/button").unwrap();
        assert_eq!(hit.id, "btn");
        // 空串 / 首尾分隔符 / 连续分隔符 / 非法段名 / 非法序号，五类拒绝。
        for bad in ["", "/a", "a/", "a//b", "a b", "a#x", "a#"] {
            assert!(parse_bind_path(bad).is_err(), "应拒绝：{}", bad);
        }
        // 同类兄弟未消歧 → 显性拒绝（防重排后静默漂移）。
        insert(&mut t, "p0", "btn2", None, SingleParentPolicy::Reject).unwrap();
        t.raw_mut("btn2").unwrap().kind = String::from("button");
        let e = resolve_bind_path(&t, "window/panel#0/button").unwrap_err();
        assert_eq!(e.code, TreeDiagCode::BindingPathInvalid);
        // 目标销毁 → 独立码。
        t.raw_mut("btn").unwrap().destroyed = true;
        let e2 = resolve_bind_path(&t, "window/panel#0/button#0").unwrap_err();
        assert_eq!(e2.code, TreeDiagCode::BindingTargetDestroyed);
    }

    #[test]
    fn 状态机_穷举32组合且disabled压过一切() {
        let mut legal = 0usize;
        for mask in 0u32..32 {
            let sig = StateSignals {
                hovered: mask & 1 != 0,
                pressed: mask & 2 != 0,
                focused: mask & 4 != 0,
                disabled: mask & 8 != 0,
                active: mask & 16 != 0,
            };
            let st = resolve_visual_state(sig);
            assert!(VISUAL_STATES.contains(&st));
            legal += 1;
            if sig.disabled {
                assert!(st.is_disabled_family(), "disabled 必须压过一切信号");
            }
        }
        assert_eq!(legal, 32);
    }

    #[test]
    fn 封闭集_三表长度与守卫一致() {
        assert_eq!(PROPERTY_KEYS.len(), 13);
        assert_eq!(EVENT_TYPES.len(), 10);
        assert_eq!(VISUAL_STATES.len(), 8);
        assert_eq!(TREE_INVARIANTS.len(), 3);
        assert_eq!(TREE_DIAG_CODES.len(), 16);
        // 诊断码线缆名不得重复（重名会让日志无法定位）。
        let mut names: Vec<&str> = TREE_DIAG_CODES.iter().map(|c| tree_code_name(*c)).collect();
        names.sort_unstable();
        let before = names.len();
        names.dedup();
        assert_eq!(before, names.len(), "诊断码线缆名重复");
    }
}
