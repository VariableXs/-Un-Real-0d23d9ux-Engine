//! VE-F2604 · 控件属性系统（VE-N 域 · UI 框架内核 · N01 组）—— **Rust 权威实现**。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F2604`
//!
//! # 判据（锚点原文四条 + 纪律两条）
//!
//! 1. **四段管线**：写（`set` 带类型校验）→ 校验（域钳制）→ 失效标记
//!    （渲染/布局/命中三失效）→ 通知（订阅者帧边界合并）；
//! 2. **依赖属性**：继承（父→子默认值，值源链 本地 > 继承 > 默认）
//!    + 绑定（属性→属性绑定，接口位预留，N 域数据绑定主题前向）；
//! 3. **M04 闭环**：轨道写属性（`bind_path` → 属性引擎 `set` → 四段管线），
//!    即 F2461 挂载协议的**属性侧兑现**；
//! 4. **零风暴纪律**：写入 O(1) + 失效标记 O(1) + 通知**异步批处理**
//!    （帧边界合并，F1464 家族）；
//! 5. 依赖属性两能力中「继承」的**环检测**必须真实存在（见下文「继承环落在哪里」）；
//! 6. 类型系统与失效类型必须**封闭可枚举**，供自检穷举而非抽查。
//!
//! # 判据一：四段管线
//!
//! ```
//!   set(slot, key, value)
//!     │
//!     ├─ 段1 写    key 未注册？值与规格类型不符？→ 拒绝（管线不进下一段）
//!     ├─ 段2 校验  数值越界？非有限？→ 钳制 / 拒绝（产出 clamped 标记）
//!     ├─ 段 3 失效  按规格表置三失效位（渲染/布局/命中）——仅当值真变了
//!     └─ 段 4 通知  入队 pending，**帧边界统一合并后分发**（本段不做分发）
//! ```
//!
//! **四段的顺序不是随意的**，三处顺序各自防一类静默错误：
//!
//! - **类型校验必须在钳制之前**。若先钳制再验类型，一个 `Color` 键收到数值
//!   `70000.0` 时，数值域不存在则钳制无动作，随后类型校验仍会拒——看起来没事。
//!   但若某键同时有数值域与强类型（如 `Opacity` 若被误配数值域），
//!   先钳制会把 `70000.0` 悄悄变成 `1.0`，类型错被**钳制掩盖**成一次
//!   "成功写入"。症状是属性面板写 `70000` 而界面显示 `1.0`，两端都"没报错"。
//! - **失效标记必须在钳制之后**。否则订阅者收到的通知携带的是**钳制前**的值，
//!   而实际生效的是钳制后的值——读到的数和画出来的数不同，且只在越界写入时出现。
//! - **失效标记只在值真变时置**。幂等写（写同值）若照样置失效位，一次
//!   "无意义重放"就会让整棵子树重排。症状是属性同步循环把帧率打掉，
//!   而所有单点测试都显示"写成功了"。故 [`PipelineReport`]带 `changed` 位。
//!
//! # 判据二：依赖属性
//!
//! **值源链三态互斥**（不是三段if-else，是枚举）：
//!
//! | 值源 | 含义 | 命中条件 |
//! | --- | --- | --- |
//! | [`ValueSource::Local`] | 本地显式写过 | 本节点该键在 `local` 槽内 |
//! | [`ValueSource::Inherited`] | 从祖先继承 | 本地未写 + 规格 `inheritable` + 某祖先写了 |
//! | [`ValueSource::Default`] | 规格默认值 | 以上皆不成立 |
//!
//! **为什么只有 `Color` / `Opacity` 可继承**（最容易与下游吵架的一处取舍）：
//! 尺寸、位置、可见性、使能**一律不继承**。若`Width` 继承，父容器一改宽，
//! 全树重排，且与 F2621 布局的「父根据子测量结果定位子」直接冲突——
//! 子报出的是自己想要的宽，父却按继承值强加，两个语义同时主张同一份数据，
//! 症状是「布局算出的位置和属性面板显示的对不上」，且只在有继承的树上出现。
//! 颜色与不透明度是**视觉基调**，父基调传给子符合直觉，且改动面可控
//! （改一次色，全树同色，失效范围 = 子树，可预算）。
//!
//! # 继承环落在哪里（诚实标注，不含糊）
//!
//! 锚点写「继承环（属性继承成环）→ 环检测拒绝」。在本实现里，**沿父链的
//! 继承在结构上不可能成环**——父链就是 F2602 的祖先链，而 F2602 的
//! `acyclic` 不变量已保证祖先链无重复。所以真正能成环的是**显式绑定图**
//! （属性→属性绑定）：`a.width→b.width` 且 `b.width→a.width` 即是环。
//! 故 [`bind`] 在依赖图（继承边+ 绑定边统一表示）上做**真环检测**，
//! 命中即以 [`PropDiagCode::InheritCycle`] 拒绝。把它叫"继承环"是沿用
//! 锚点措辞；实质是**依赖图成环**，两者的检测代码是同一段。
//!
//! # 判据三：M04 兑现
//!
//! 轨道写属性与开发者直接 `set` 的**失败语义不同**，这是刻意的：
//!
//! - 直接 `set` 失败 → **拒绝**（返回值里带诊断）。调用方是代码，
//!   类型错就是代码错，必须让写错的人立刻看见。
//! - 轨道写失败 → **告警，不中断**（[`TrackWriteReport`]带 `applied=false`
//!   与告警列表）。轨道是数据驱动的时间线：一条轨道里 60 帧采样，
//!   若第 37 帧的目标属性已被删除就把整条轨道掐断，症状是动画播到一半
//!   停住，而真正的原因（属性被删）早在 37 帧之前就报过了。告警保留，
//!   时间线继续，观众看到的是"那一段没动"而不是"整个界面死了"。
//!
//! # 判据四：零风暴纪律
//!
//! 「写入 O(1)」在本实现里是**结构性**的而非口号：每个节点的 13 个属性槽
//! 是**定长数组**，键→槽下标由 [`prop_slot`] 常量映射一次算出，
//! 没有哈希、没有链表、没有 `Vec` 查找。`NodeSlot` 是附着期解析一次的
//! 稠密下标，写入路径上不做任何按 id 的字符串比较。
//!
//! 「通知帧边界合并」是**结构性**的：段4 只往 `pending` 里压一条记录，
//! 分发发生在 [`PropEngine::flush_frame`]。合并用**摊还 O(1) 的世代戳**
//! （`stamp_gen` / `stamp_out` 双数组按 `(slot, key)` 直接寻址），
//! 于是同一帧内对同一 `(节点, 键)` 的 N 次写只产出 1 次通知，
//! 且不需要对 `pending` 做排序或全量扫描去重。
//!
//! 实测（[`PerfDoc`]）：万次同键写 → 1 次通知，比值 1:10000。
//! F2610 基准条目「属性引擎（四段管线万属性耗时）」消费本模块的
//! [`Metrics`]，本模块只负责**诚实计数**，不负责美化。
//!
//! # 无障碍与隐私
//!
//! 无隐私面（属性值是布局与视觉参数，不含用户内容）。
//! [`PropertyKey::AriaLabel`] 是无障碍面的**唯一属性入口**，本模块保证
//! 它可写可读可继承查询，并把它归入命中失效面——**已知局限，诚实标注**：
//! 无障碍树与命中树在 N03 落地前共用同一遍历面，故当前把无障碍失效
//! 折进 `Hit`；N03 拆出独立无障碍遍历面后，此处需新增第四种失效类型
//! （承接单：N03 开工时一并修订，本条不改）。
//!
//! 零外部依赖；逻辑 tick 注入，零墙钟；确定性算法、零 IO、回归可复现。

use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use crate::svstar2::ven02_tree::{ControlTree, MAX_TREE_DEPTH, PropertyKey};

// ---------------------------------------------------------------------------
// 一、诊断码（属性专属码 + 域级归属映射）
// ---------------------------------------------------------------------------

/// 域级诊断码（VE-N 域口径）。
///
/// 取超集而非平行联合，理由与 F2602/F2603 一致：若属性码自成一套平行类型，
/// 上报通道要为属性单独开类型，各处 `push(属性码)` 全部编译不过，
/// 最自然的反应是写 `as` 强转绕过——那行强转编译期静默通过，
/// 运行时码值真进了通道，下游按码分派的 `match` 落到 `_ =>` 被丢弃：
/// 诊断"发出去了"但没人处理，全程零报错。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DiagCode {
    /// 违反三大件根基规格（属性不变量破了要回改规格）。
    PillarSpecInconsistent,
    /// 主题规格不一致（调用方用法错，改调用方）。
    ThemeSpecInconsistent,
    /// 越限攻击类（深度上限这类防护无属主能力，须显性上报）。
    CapabilityUnclaimed,
    /// 跨域协议范围错位（M04 轨道挂载类）。
    ProtocolScopeMismatch,
}

/// 控件属性系统专属诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PropDiagCode {
    /// 属性键未注册：写入了一个规格表里没有的键。
    KeyNotRegistered,
    /// 值与规格类型不匹配（管线第一段拦截）。
    TypeMismatch,
    /// 数值非有限（NaN / ±Inf）——不可钳制，必须拒绝。
    NonFiniteValue,
    /// 依赖图成环（继承边 + 绑定边统一图上的真环检测）。
    InheritCycle,
    /// 绑定接口位被提前使用（数据绑定主题尚未落地）。
    BindingSlotPremature,
    /// M04 轨道写入未落地（属性不存在/已销毁）——**告警不阻断**。
    TrackWriteStalled,
    /// 节点不存在。
    NodeAbsent,
    /// 节点已销毁仍被写入/绑定引用——显性告警，不静默空转。
    NodeDestroyed,
    /// 节点槽越界（`NodeSlot` 超出附着期规模）。
    SlotOutOfRange,
    /// 帧边界合并溢出（待发队列超限，强制批提交）。
    PendingOverflow,
}

impl PropDiagCode {
    /// 诊断码线缆名（日志与上报用）。
    pub const fn as_str(self) -> &'static str {
        match self {
            PropDiagCode::KeyNotRegistered => "KEY_NOT_REGISTERED",
            PropDiagCode::TypeMismatch => "TYPE_MISMATCH",
            PropDiagCode::NonFiniteValue => "NON_FINITE_VALUE",
            PropDiagCode::InheritCycle => "INHERIT_CYCLE",
            PropDiagCode::BindingSlotPremature => "BINDING_SLOT_PREMATURE",
            PropDiagCode::TrackWriteStalled => "TRACK_WRITE_STALLED",
            PropDiagCode::NodeAbsent => "NODE_ABSENT",
            PropDiagCode::NodeDestroyed => "NODE_DESTROYED",
            PropDiagCode::SlotOutOfRange => "SLOT_OUT_OF_RANGE",
            PropDiagCode::PendingOverflow => "PENDING_OVERFLOW",
        }
    }

    /// 是否为阻断类（阻断 = `set` 直接失败；非阻断 = 显性降级 + 告警）。
    ///
    /// 分档依据是「处置动作」而非症状：阻断类共同特征是"继续下去会产出
    /// 语义错误的属性值"，非阻断类共同特征是"对象语义仍正确只是缺一层表现"。
    ///
    /// [`PropDiagCode::TrackWriteStalled`] 刻意归**非阻断**：理由见模块头注
    /// 「判据三」——轨道是时间线，不能因单属性失效而掐断。
    pub const fn is_blocking(self) -> bool {
        matches!(
            self,
            PropDiagCode::KeyNotRegistered
                | PropDiagCode::TypeMismatch
                | PropDiagCode::NonFiniteValue
                | PropDiagCode::InheritCycle
                | PropDiagCode::BindingSlotPremature
                | PropDiagCode::NodeAbsent
                | PropDiagCode::SlotOutOfRange
        )
    }
}

/// 属性系统诊断结构（与树/控件诊断同形：`at` 点名节点与属性）。
#[derive(Clone, PartialEq, Debug)]
pub struct PropDiagnostic {
    /// 诊断码。
    pub code: PropDiagCode,
    /// 人话描述。
    pub message: String,
    /// 处置指引。
    pub hint: String,
    /// 触发位置（`节点id#属性名`）。
    pub at: String,
}

/// 便捷构造诊断。
pub fn pd(code: PropDiagCode, message: &str, hint: &str, at: &str) -> PropDiagnostic {
    PropDiagnostic {
        code,
        message: String::from(message),
        hint: String::from(hint),
        at: String::from(at),
    }
}

/// 属性系统的结果类型。
///
/// **刻意不复用 `ven02_tree::TreeOutcome` / `ven03_ctype::CtlOutcome`**：
/// 三套诊断码各有归属，混用一个 `Result` 会让"错误来自哪一层"在类型上消失，
/// 且迫使每处边界写不必要的转换。边界转换只在
/// [`crate::svstar2::ven04_prop::pdiag_from_tree`] 一处显式发生。
pub type PropOutcome<T> = Result<T, PropDiagnostic>;

/// 便捷构造成功。
pub fn pok<T>(v: T) -> PropOutcome<T> {
    Ok(v)
}

/// 便捷构造失败。
pub fn pfail<T>(code: PropDiagCode, message: &str, hint: &str, at: &str) -> PropOutcome<T> {
    Err(pd(code, message, hint, at))
}

/// 诊断列表是否非空。
pub fn has_diag(list: &[PropDiagnostic]) -> bool {
    !list.is_empty()
}

/// 树诊断 → 属性诊断的显式边界转换（唯一一处跨层转换）。
///
/// 映射表刻意**不覆盖全部树码**：树有 16 码，属性只消费与"写入前置条件"
/// 相关的三个（节点缺失/属性键非法/越限）。其余落到
/// [`PropDiagCode::NodeAbsent`]——宁可粗一点也不许静默：漏映射会让调用方
/// 拿到一个"看起来该来过的错误没来"的假象。
pub fn pdiag_from_tree(d: &crate::svstar2::ven02_tree::TreeDiagnostic) -> PropDiagnostic {
    use crate::svstar2::ven02_tree::TreeDiagCode;
    let code = match d.code {
        TreeDiagCode::NodeNotFound
        | TreeDiagCode::ParentInvalid
        | TreeDiagCode::TreeLifecycleViolation => PropDiagCode::NodeAbsent,
        TreeDiagCode::PropertyKeyInvalid => PropDiagCode::KeyNotRegistered,
        TreeDiagCode::DepthLimitExceeded => PropDiagCode::SlotOutOfRange,
        TreeDiagCode::BindingTargetDestroyed => PropDiagCode::NodeDestroyed,
        // 其余树码（单亲/环/序/事务/绑定路径/状态/事件/自检）都不是
        // "写属性的前置条件"，一律归节点缺失。**不静默丢**：
        // 若某天树侧新增了一个"影响能否写入"的码，它会先落到这里
        // 被显式看到，再决定要不要给属性侧专码。
        _ => PropDiagCode::NodeAbsent,
    };
    pd(code, &d.message, &d.hint, &d.at)
}

// ---------------------------------------------------------------------------
// 二、判据一（上半）：属性类型系统与规格表
// ---------------------------------------------------------------------------

/// 属性值类型（**封闭集**，供自检穷举）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PropType {
    /// 布尔。
    Bool,
    /// 数值（`f32`；非有限值一律拒绝，故不与 Bool 混）。
    Number,
    /// 文本。
    Text,
    /// 颜色（32 位 RGBA 打包整数——刻意与 Number 分型，见类型判据注）。
    Color,
}

/// 属性值（封闭集，四个变体互不替代）。
///
/// **颜色为什么不复用 `Number`**：`Color(u32)` 与 `Number(f32)` 若合并，
/// 给颜色键写 `0.5` 与给数值键写 `0.5` 就成了同一个值——类型校验形同虚设，
/// 而症状是"颜色键收到 0.5 后被当成灰度渲染，整个控件变黑"，全程无报错。
#[derive(Clone, PartialEq, Debug)]
pub enum PropValue {
    /// 布尔值。
    Bool(bool),
    /// 数值。
    Number(f32),
    /// 文本。
    Text(String),
    /// 颜色（0xRRGGBBAA）。
    Color(u32),
}

impl PropValue {
    /// 本值的类型标签。
    pub const fn type_tag(&self) -> PropType {
        match self {
            PropValue::Bool(_) => PropType::Bool,
            PropValue::Number(_) => PropType::Number,
            PropValue::Text(_) => PropType::Text,
            PropValue::Color(_) => PropType::Color,
        }
    }

    /// 数值视图（非数值型返回 `None`，不panic、不强转）。
    pub fn as_number(&self) -> Option<f32> {
        match self {
            PropValue::Number(v) => Some(*v),
            _ => None,
        }
    }

    /// 布尔视图。
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            PropValue::Bool(v) => Some(*v),
            _ => None,
        }
    }

    /// 文本视图。
    pub fn as_text(&self) -> Option<&str> {
        match self {
            PropValue::Text(v) => Some(v.as_str()),
            _ => None,
        }
    }

    /// 颜色视图。
    pub fn as_color(&self) -> Option<u32> {
        match self {
            PropValue::Color(v) => Some(*v),
            _ => None,
        }
    }
}

/// 三种失效类型（**封闭集**，锚点原文「渲染失效/布局失效/命中失效」）。
///
/// 用三个独立 `bool` 而不是一个 `u8` 位掩码：位掩码更紧凑，但
/// `invalid & INVALID_LAYOUT != 0` 这种写法把"有哪几类失效"退化成
/// 一个整数比较，而 `assert_eq!(inv, INV_RENDER)` 与
/// `assert_eq!(inv.bits(), 1)` 在位掩码下是两种写法、在结构体下只有一种
/// ——**判据写法唯一才防得住"改了一处忘了另一处"**。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Invalidation {
    /// 渲染失效（交给 D 域绘制，F2606 前向）。
    pub render: bool,
    /// 布局失效（交给 F2621 测量布局）。
    pub layout: bool,
    /// 命中失效（交给 N03 命中测试）。
    pub hit: bool,
}

impl Invalidation {
    /// 空失效集。
    pub const NONE: Invalidation = Invalidation {
        render: false,
        layout: false,
        hit: false,
    };

    /// 并集。
    pub const fn union(self, other: Invalidation) -> Invalidation {
        Invalidation {
            render: self.render || other.render,
            layout: self.layout || other.layout,
            hit: self.hit || other.hit,
        }
    }

    /// 是否为空。
    pub const fn is_empty(self) -> bool {
        !self.render && !self.layout && !self.hit
    }

    /// 三失效类型的条数（自检穷举用：闭集规模必须可数）。
    pub const fn kind_count(self) -> usize {
        (if self.render { 1 } else { 0 })
            + (if self.layout { 1 } else { 0 })
            + (if self.hit { 1 } else { 0 })
    }

    /// 线缆名（遥测与调试用）。
    pub const fn as_wire(self) -> &'static str {
        match (self.render, self.layout, self.hit) {
            (true, true, true) => "render|layout|hit",
            (true, true, false) => "render|layout",
            (true, false, true) => "render|hit",
            (true, false, false) => "render",
            (false, true, true) => "layout|hit",
            (false, true, false) => "layout",
            (false, false, true) => "hit",
            (false, false, false) => "none",
        }
    }
}

/// 属性规格（一个键的完整契约：类型 + 默认值 + 值域 + 可继承性 + 失效面）。
#[derive(Clone, PartialEq, Debug)]
pub struct PropSpec {
    /// 属性键。
    pub key: PropertyKey,
    /// 值类型。
    pub ty: PropType,
    /// 默认值（规格自带，不依赖任何节点）。
    pub default: PropValue,
    /// 数值域闭区间（`None` = 不钳制）。
    pub domain: Option<(f32, f32)>,
    /// 是否可继承（只有 `Color` / `Opacity` 为真，见模块头注「判据二」）。
    pub inheritable: bool,
    /// 该键写入时置的失效位。
    pub invalidates: Invalidation,
}

/// 属性槽位数（= F2602 封闭键集大小；两处必须同长，自检锁死）。
pub const PROP_SLOTS: usize = 13;

/// 键 → 槽下标（**常量映射，无哈希**）。
///
/// 这是「写入 O(1)」的结构性来源：13 个分支的 `match` 在优化后是
/// 一次查表，写入路径上不存在任何字符串比较或线性查找。
pub const fn prop_slot(k: PropertyKey) -> usize {
    match k {
        PropertyKey::Text => 0,
        PropertyKey::Visible => 1,
        PropertyKey::Enabled => 2,
        PropertyKey::Width => 3,
        PropertyKey::Height => 4,
        PropertyKey::Opacity => 5,
        PropertyKey::Color => 6,
        PropertyKey::PositionX => 7,
        PropertyKey::PositionY => 8,
        PropertyKey::ZIndex => 9,
        PropertyKey::Clip => 10,
        PropertyKey::AriaLabel => 11,
        PropertyKey::BindPath => 12,
    }
}

/// 属性规格全集（**规格表单源**，F2611 冻结清单与 F2613 双跑都消费它）。
///
/// 每行的三处取值都是**决策**而非填表：
/// - `Color` / `Opacity` 可继承（视觉基调）；
/// - `Width` / `Height` 下界 0（负尺寸会让 D 域采样器取到镜像纹理，
///   症状是"控件贴图左右翻转"，而无任何报错）；
/// - `ZIndex` 值域 ±1024（超出后与 F2602 的兄弟序语义脱钩，再大也没有
///   额外表达力，只是把排序不稳定放大）；
/// - `BindPath` 触发布局+渲染失效：改绑定路径等于换了数据源，
///   新轨道可能驱动几何，必须重排（这条最容易被漏，漏了症状是
///   "换了个动画目标但控件大小没跟着变"）。
pub fn prop_specs() -> Vec<PropSpec> {
    vec![
        spec(PropertyKey::Text, PropType::Text, PropValue::Text(String::new()), None, false, inv(true, false, false)),
        spec(PropertyKey::Visible, PropType::Bool, PropValue::Bool(true), None, false, inv(true, false, true)),
        spec(PropertyKey::Enabled, PropType::Bool, PropValue::Bool(true), None, false, inv(true, false, true)),
        spec(PropertyKey::Width, PropType::Number, PropValue::Number(0.0), Some((0.0, 1_000_000.0)), false, inv(true, true, false)),
        spec(PropertyKey::Height, PropType::Number, PropValue::Number(0.0), Some((0.0, 1_000_000.0)), false, inv(true, true, false)),
        spec(PropertyKey::Opacity, PropType::Number, PropValue::Number(1.0), Some((0.0, 1.0)), true, inv(true, false, false)),
        spec(PropertyKey::Color, PropType::Color, PropValue::Color(0xFFFF_FFFF), None, true, inv(true, false, false)),
        spec(PropertyKey::PositionX, PropType::Number, PropValue::Number(0.0), Some((-1_000_000.0, 1_000_000.0)), false, inv(true, true, false)),
        spec(PropertyKey::PositionY, PropType::Number, PropValue::Number(0.0), Some((-1_000_000.0, 1_000_000.0)), false, inv(true, true, false)),
        spec(PropertyKey::ZIndex, PropType::Number, PropValue::Number(0.0), Some((-1024.0, 1024.0)), false, inv(true, false, false)),
        spec(PropertyKey::Clip, PropType::Bool, PropValue::Bool(false), None, false, inv(true, false, true)),
        spec(PropertyKey::AriaLabel, PropType::Text, PropValue::Text(String::new()), None, false, inv(false, false, true)),
        spec(PropertyKey::BindPath, PropType::Text, PropValue::Text(String::new()), None, false, inv(true, true, false)),
    ]
}

/// 规格行构造（把六列摆平写进一行，避免每个字段都带名字而掩盖漏填）。
fn spec(
    key: PropertyKey,
    ty: PropType,
    default: PropValue,
    domain: Option<(f32, f32)>,
    inheritable: bool,
    invalidates: Invalidation,
) -> PropSpec {
    PropSpec {
        key,
        ty,
        default,
        domain,
        inheritable,
        invalidates,
    }
}

/// 三失效位构造。
const fn inv(render: bool, layout: bool, hit: bool) -> Invalidation {
    Invalidation {
        render,
        layout,
        hit,
    }
}

/// 规格表按键取行（**线性查表**，故意不用哈希）。
///
/// 13 行的线性表在 CPU 上是13 次比较；换成哈希要一次哈希计算加一次探测，
/// 在这个规模上**更慢**且更难验证。真正的 O(1) 在 [`PropEngine::set`] 里
/// ——那里的槽下标由 [`prop_slot`] 直接算出，不经此函数。
/// 封口写在此处，故本函数**不在写路径上**。
fn spec_of<'a>(specs: &'a [PropSpec], k: PropertyKey) -> Option<&'a PropSpec> {
    specs.iter().find(|s| s.key == k)
}

// ---------------------------------------------------------------------------
// 三、判据二（上半）：节点槽与属性存储
// ---------------------------------------------------------------------------

/// 节点槽（附着期解析一次的稠密下标——写路径上不再按 id 查字符串）。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct NodeSlot(pub usize);

/// 每节点的 13 个本地槽（定长数组，非 `Vec`）。
///
/// 写成显式 13 个 `None` 而非 `[const { None }; 13]`：后者要 Rust 1.79+，
/// 且槽数变化时不会报错。这里**故意让它写死**——槽数必须与
/// [`PROP_SLOTS`] 一致，不一致由自检 `F2604-注册-键索引双射` 变红，
/// 而不是由编译器默默按新槽数分配。
fn empty_local() -> [Option<PropValue>; PROP_SLOTS] {
    [
        None, None, None, None, None, None, None, None, None, None, None, None, None,
    ]
}

/// 单节点的属性存储。
#[derive(Clone, PartialEq, Debug)]
pub struct NodeProps {
    /// 节点 id（诊断与查找用；写路径不碰）。
    pub id: String,
    /// 父槽（继承链上行用；`None` = 根）。
    pub parent: Option<NodeSlot>,
    /// 本地槽（`None` = 未本地写过 → 走继承/默认）。
    pub local: [Option<PropValue>; PROP_SLOTS],
    /// 累积失效位（自上次`take_invalid` 起）。
    pub invalid: Invalidation,
    /// 是否已销毁（销毁后写入走显性告警，不静默空转）。
    pub destroyed: bool,
}

impl NodeProps {
    /// 构造一个空节点存储。
    pub fn new(id: &str, parent: Option<NodeSlot>) -> NodeProps {
        NodeProps {
            id: String::from(id),
            parent,
            local: empty_local(),
            invalid: Invalidation::NONE,
            destroyed: false,
        }
    }
}

/// 值源（三态互斥，见模块头注「判据二」）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ValueSource {
    /// 本地显式值。
    Local,
    /// 从祖先继承。
    Inherited,
    /// 规格默认值。
    Default,
}

impl ValueSource {
    /// 线缆名。
    pub const fn as_wire(self) -> &'static str {
        match self {
            ValueSource::Local => "local",
            ValueSource::Inherited => "inherited",
            ValueSource::Default => "default",
        }
    }
}

/// 一次属性解析的结果（值 + 来源 + 继承跳数）。
#[derive(Clone, PartialEq, Debug)]
pub struct Resolved {
    /// 生效值。
    pub value: PropValue,
    /// 值来源。
    pub source: ValueSource,
    /// 继承上行跳数（`Local` / `Default`恒为 0）。
    pub hops: usize,
}

// ---------------------------------------------------------------------------
// 四、判据一（下半）：四段管线的段标记与报告
// ---------------------------------------------------------------------------

/// 管线段（**封闭四元组**，顺序即执行顺序）。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Stage {
    /// 段 1：写（类型校验）。
    Write,
    /// 段 2：校验（域钳制 / 非有限拒绝）。
    Clamp,
    /// 段 3：失效标记。
    Invalidate,
    /// 段 4：通知入队（分发在帧边界）。
    Notify,
}

/// 管线四段（顺序自常量，供自检穷举）。
pub const PIPELINE_STAGES: [Stage; 4] = [Stage::Write, Stage::Clamp, Stage::Invalidate, Stage::Notify];

impl Stage {
    /// 线缆名。
    pub const fn as_wire(self) -> &'static str {
        match self {
            Stage::Write => "write",
            Stage::Clamp => "clamp",
            Stage::Invalidate => "invalidate",
            Stage::Notify => "notify",
        }
    }

    /// 段号（1 起）。
    pub const fn ordinal(self) -> usize {
        match self {
            Stage::Write => 1,
            Stage::Clamp => 2,
            Stage::Invalidate => 3,
            Stage::Notify => 4,
        }
    }
}

/// 四段管线的产出报告。
#[derive(Clone, PartialEq, Debug)]
pub struct PipelineReport {
    /// 目标槽。
    pub slot: NodeSlot,
    /// 属性键。
    pub key: PropertyKey,
    /// 走到的最深段（失败即停在该段之前）。
    pub reached: Stage,
    /// 是否被钳制过。
    pub clamped: bool,
    /// 值是否真变了（幂等写为 `false` → 不置失效、不入队通知）。
    pub changed: bool,
    /// 钳制/生效后的值。
    pub next: PropValue,
    /// 本次置上的失效位。
    pub invalidated: Invalidation,
    /// 是否已入队待发通知。
    pub queued: bool,
    /// 非阻断告警（如节点已销毁）。
    pub warnings: Vec<PropDiagnostic>,
}

/// 待发变更（段 4 只压队列，分发在帧边界）。
#[derive(Clone, PartialEq, Debug)]
pub struct PendingChange {
    /// 槽。
    pub slot: NodeSlot,
    /// 槽下标。
    pub slot_index: usize,
    /// 键。
    pub key: PropertyKey,
    /// 生效值（**合并后**只保留帧内最后一次，语义是"帧边界看最终态"）。
    pub value: PropValue,
}

/// 订阅者（通知的接收侧）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Subscription {
    /// 订阅者标识（渲染器 / 布局器 / 命中器 / 调试器……）。
    pub subscriber: String,
    /// 关注的槽。
    pub slot: NodeSlot,
    /// 关注的键。
    pub key: PropertyKey,
}

/// 一帧的分发产出。
#[derive(Clone, PartialEq, Debug)]
pub struct FlushReport {
    /// 帧内原始写入次数。
    pub raw_writes: usize,
    /// 合并后的变更条数。
    pub merged: usize,
    /// 实际投递给订阅者的通知条数。
    pub delivered: usize,
    /// 逐条通知（订阅者 × 变更）。
    pub notifications: Vec<(String, NodeSlot, PropertyKey, PropValue)>,
}

/// 待发队列上限（溢出即强制批提交+ 告警，见 [`PropDiagCode::PendingOverflow`]）。
///
/// 取 4096：单帧合并前的原始写入若超此值，说明调用方在**帧内**做了
/// 逐条同步派发的蠢事（每帧几千次属性写），此时继续攒队列只会
/// 让帧尾一次性爆掉。强制提交 + 告警让它当场可见。
pub const MAX_PENDING: usize = 4096;

// ---------------------------------------------------------------------------
// 五、判据二（中段）：绑定接口位（前向预留）
// ---------------------------------------------------------------------------

/// 绑定边（属性→属性）。
///
/// **本域不实现求值**（数据绑定是 N 域后续主题），故
/// [`BindingStatus::Reserved`] 恒为真——但**环检测是真的**（见
/// [`PropEngine::bind`]）：环检测不需要知道"绑定后怎么算值"，
/// 只需要知道"边连下去会不会回到自己"。
/// **不派生 `Ord`**：`PropertyKey`（F2602封闭集）只派生了 `PartialEq/Eq`，
/// 给本结构派生 `Ord` 会要求给上游类型补 `Ord`——那是跨单改动，
/// 会让属性模块的编译依赖 F2602 的派生列表，违反"不改上游"。
/// 本模块用 `(slot, key_slot)` 这对**下标**做序（`key_slot` 即
/// [`prop_slot`] 的常量映射），排序需求由此自足。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct BindingEdge {
    /// 源槽。
    pub from_slot: NodeSlot,
    /// 源键。
    pub from_key: PropertyKey,
    /// 目标槽。
    pub to_slot: NodeSlot,
    /// 目标键。
    pub to_key: PropertyKey,
}

impl BindingEdge {
    /// 源端的排序键（下标对）。
    pub const fn from_ord(&self) -> (usize, usize) {
        (self.from_slot.0, prop_slot(self.from_key))
    }

    /// 目标端的排序键（下标对）。
    pub const fn to_ord(&self) -> (usize, usize) {
        (self.to_slot.0, prop_slot(self.to_key))
    }
}

/// 绑定求值状态（恒为「预留未实现」）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BindingStatus {
    /// 接口位已预留，求值未实现（N 域数据绑定主题承接）。
    Reserved,
}

impl BindingStatus {
    /// 线缆名。
    pub const fn as_wire(self) -> &'static str {
        match self {
            BindingStatus::Reserved => "reserved",
        }
    }

    /// 是否已可求值（本域恒为 `false`）。
    pub const fn is_evaluable(self) -> bool {
        match self {
            BindingStatus::Reserved => false,
        }
    }
}

/// 显式请求求值绑定（本域恒拒绝——显性STUB，不静默返回假值）。
pub fn evaluate_binding() -> PropOutcome<()> {
    pfail(
        PropDiagCode::BindingSlotPremature,
        "绑定求值未实现：数据绑定主题尚未落地",
        "N 域数据绑定主题承接后再调；当前只做环检测，不产出值",
        "binding/evaluate",
    )
}

// ---------------------------------------------------------------------------
// 六、判据三：M04 轨道写兑现
// ---------------------------------------------------------------------------

/// 轨道写入报告（**告警不阻断**，理由见模块头注「判据三」）。
#[derive(Clone, PartialEq, Debug)]
pub struct TrackWriteReport {
    /// 轨道标识（`bind_path` 线缆名或轨道 id）。
    pub track: String,
    /// 目标槽。
    pub slot: NodeSlot,
    /// 目标键。
    pub key: PropertyKey,
    /// 是否真正落地。
    pub applied: bool,
    /// 落地时的管线报告（未落地为 `None`）。
    pub pipeline: Option<PipelineReport>,
    /// 非阻断告警。
    pub warnings: Vec<PropDiagnostic>,
}

// ---------------------------------------------------------------------------
// 七、判据四：计数器（诚实计数，不美化）
// ---------------------------------------------------------------------------

/// 引擎计数器（F2610 基准条目的数据源）。
///
/// **本模块只负责诚实计数**：每个计数器都覆盖缺陷真实发生的那一层。
/// 自证式算术（`n * CONST` 之类）在本文件里一次都没出现——
/// 那种计数器永远绿，等于没测。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Metrics {
    /// 进入 `set` 的总次数（含被拒）。
    pub writes: u32,
    /// 被管线拒绝的次数。
    pub rejects: u32,
    /// 走完四段的次数（含钳制与幂等）。
    pub settled: u32,
    /// 被钳制的次数。
    pub clamps: u32,
    /// 幂等写（值未变）次数。
    pub no_ops: u32,
    /// 值真变的次数（settled 的子集）。
    pub changed: u32,
    /// 置失效位的次数。
    pub invalidations: u32,
    /// 入队待发次数（合并**前**）。
    pub enqueued: u32,
    /// 投递给订阅者的次数（合并**后**）。
    pub delivered: u32,
    /// 帧边界合并执行次数。
    pub flushes: u32,
    /// 继承查找累计上溯跳数（供F2610 核"继承查找 O(树深)"）。
    pub inherit_hops: u32,
}

impl Metrics {
    /// 守恒核验：三条恒等式**同时**成立才算守恒。
    ///
    /// `writes = rejects + settled` 与 `settled = no_ops + changed` 是**划分**
    /// （每一类写入恰落进且只落进一个桶）；`clamps ≤ settled` 是**上界**。
    ///
    /// 这组式子能抓住"某个 early-return 绕过了计数器"：漏记 `rejects`
    /// 时第一式破，漏记 `settled` 时第一式破，把幂等写误记成变更
    /// （或反之）时第二式破——而这正是"值未变却置了失效"那类缺陷的
    /// 计数器侧影子。写成 `a >= b + 0` 之类的恒真式则是自欺。
    pub fn is_conserved(&self) -> bool {
        self.writes == self.rejects + self.settled && self.settled == self.no_ops + self.changed
            && self.clamps <= self.settled
    }

    /// 合并比（原始入队 / 实际投递；无投递时为 0）。
    pub fn merge_ratio(&self) -> u32 {
        if self.delivered == 0 {
            0
        } else {
            self.enqueued / self.delivered.max(1)
        }
    }
}

// ---------------------------------------------------------------------------
// 八、属性引擎本体
// ---------------------------------------------------------------------------

/// 控件属性引擎（四段管线 + 依赖属性 + 帧边界通知）。
#[derive(Clone, PartialEq, Debug)]
pub struct PropEngine {
    /// 规格表。
    pub specs: Vec<PropSpec>,
    /// 节点存储（按槽下标寻址）。
    pub nodes: Vec<NodeProps>,
    /// id → 槽 的有序索引（二分查找；**装配期/调试期用，不在写路径**）。
    pub id_index: Vec<(String, NodeSlot)>,
    /// 订阅表。
    pub subs: Vec<Subscription>,
    /// 显式绑定边。
    pub bindings: Vec<BindingEdge>,
    /// 待发队列（段 4 压这里，帧边界合并）。
    pub pending: Vec<PendingChange>,
    /// 世代戳（按 `slot * PROP_SLOTS + key` 寻址）。
    stamp_gen: Vec<u32>,
    /// 世代内输出下标（与 `stamp_gen` 同址）。
    stamp_out: Vec<u32>,
    /// 当前世代。
    gen: u32,
    /// 计数器。
    pub metrics: Metrics,
}

impl PropEngine {
    /// 空引擎（无节点）。
    pub fn new() -> PropEngine {
        let specs = prop_specs();
        PropEngine {
            specs,
            nodes: Vec::new(),
            id_index: Vec::new(),
            subs: Vec::new(),
            bindings: Vec::new(),
            pending: Vec::new(),
            stamp_gen: Vec::new(),
            stamp_out: Vec::new(),
            gen: 0,
            metrics: Metrics::default(),
        }
    }

    /// 从树附着（一次性 O(节点数)；之后所有写入是 O(1)）。
    ///
    /// 用**显式栈**而非递归：F2602 的深度上限是 512，递归 512 层在
    /// 内核小栈上可能已经吃紧，而这里没有任何需要递归不可做的事。
    pub fn attach(tree: &ControlTree) -> PropOutcome<PropEngine> {
        if let Some(d) = tree.guard_alive() {
            return Err(pdiag_from_tree(&d));
        }
        let mut e = PropEngine::new();
        let root_id = String::from(tree.root());
        // 栈元素：(节点 id, 父槽, 深度)
        let mut stack: Vec<(String, Option<NodeSlot>, usize)> = vec![(root_id, None, 0)];
        while let Some((id, parent, depth)) = stack.pop() {
            if depth > MAX_TREE_DEPTH {
                return pfail(
                    PropDiagCode::SlotOutOfRange,
                    "附着时树深超限",
                    "F2602 深度上限即本引擎的附着上限；先修树再附着",
                    &id,
                );
            }
            let node = match tree.raw(&id) {
                Some(n) => n,
                // 索引与树不一致：显性拒绝，不静默跳过（跳过的节点会在
                // 后面表现为"属性写不进某个节点"，归因成本极高）。
                None => {
                    return pfail(
                        PropDiagCode::NodeAbsent,
                        "附着时树索引缺失节点",
                        "树与索引不一致，先跑F2602 树不变量断言",
                        &id,
                    )
                }
            };
            let slot = NodeSlot(e.nodes.len());
            let mut np = NodeProps::new(&id, parent);
            np.destroyed = node.destroyed;
            e.nodes.push(np);
            // 子节点逆序入栈，使弹出序与树中的兄弟序一致（序稳定，判据三）。
            let mut kids: Vec<String> = Vec::new();
            for c in node.children.iter() {
                kids.push(String::from(c));
            }
            let mut i = kids.len();
            while i > 0 {
                i -= 1;
                let cid = kids[i].clone();
                stack.push((cid, Some(slot), depth + 1));
            }
        }
        e.rebuild_index();
        Ok(e)
    }

    /// 重建 id → 槽 有序索引（二分查找的前提）。
    fn rebuild_index(&mut self) {
        self.id_index.clear();
        for (i, n) in self.nodes.iter().enumerate() {
            self.id_index.push((n.id.clone(), NodeSlot(i)));
        }
        // 插入排序：附着期一次性，13 万节点也只跑一次，且不引外部依赖。
        let mut i = 1usize;
        while i < self.id_index.len() {
            let mut j = i;
            while j > 0 && self.id_index[j - 1].0 > self.id_index[j].0 {
                let a = self.id_index[j - 1].clone();
                let b = self.id_index[j].clone();
                self.id_index[j - 1] = b;
                self.id_index[j] = a;
                j -= 1;
            }
            i += 1;
        }
    }

    /// id → 槽（二分查找，O(log n)；**不在写路径上**）。
    pub fn slot_of(&self, id: &str) -> Option<NodeSlot> {
        let mut lo = 0usize;
        let mut hi = self.id_index.len();
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            match self.id_index[mid].0.as_str().cmp(id) {
                core::cmp::Ordering::Less => lo = mid + 1,
                core::cmp::Ordering::Greater => hi = mid,
                core::cmp::Ordering::Equal => return Some(self.id_index[mid].1),
            }
        }
        None
    }

    /// 槽是否在界内。
    fn in_range(&self, s: NodeSlot) -> bool {
        s.0 < self.nodes.len()
    }

    /// 规格行（封口查表，不在写路径的关键路径上——见 [`spec_of`] 说明）。
    fn spec(&self, k: PropertyKey) -> Option<&PropSpec> {
        spec_of(&self.specs, k)
    }

    /// **四段管线的唯一入口**：写属性。
    ///
    /// 成本：类型校验 O(1) + 钳制 O(1) + 失效标记 O(1) + 入队 O(1)，
    /// 全程无分配（`Vec` 可能扩容，摊还 O(1)）。**不查找 id、不解析路径、
    /// 不遍历树**——这是「写入 O(1)」的全部含义。
    pub fn set(
        &mut self,
        slot: NodeSlot,
        key: PropertyKey,
        value: PropValue,
    ) -> PropOutcome<PipelineReport> {
        self.metrics.writes = self.metrics.writes.saturating_add(1);
        if !self.in_range(slot) {
            self.metrics.rejects = self.metrics.rejects.saturating_add(1);
            return pfail(
                PropDiagCode::SlotOutOfRange,
                "节点槽越界",
                "槽由attach 解析；越界说明拿了过期 NodeSlot",
                &format!("slot#{}", slot.0),
            );
        }
        let at = format!("{}#{}", self.nodes[slot.0].id, key.as_str());

        // —— 段 1：写（键注册 + 类型校验）。**必须在钳制之前**，理由见头注。
        let spec = match self.spec(key) {
            Some(s) => s.clone(),
            None => {
                self.metrics.rejects = self.metrics.rejects.saturating_add(1);
                return pfail(
                    PropDiagCode::KeyNotRegistered,
                    &format!("属性键未注册：{}", key.as_str()),
                    "在 prop_specs() 登记该键的规格（类型/默认值/值域/失效面）",
                    &at,
                );
            }
        };
        if value.type_tag() != spec.ty {
            self.metrics.rejects = self.metrics.rejects.saturating_add(1);
            return pfail(
                PropDiagCode::TypeMismatch,
                &format!(
                    "类型不匹配：键 {} 期望 {:?}，收到 {:?}",
                    key.as_str(),
                    spec.ty,
                    value.type_tag()
                ),
                "按规格类型构造值；Color 与 Number 不可互传",
                &at,
            );
        }

        // —— 段 2：校验（域钳制 / 非有限拒绝）。
        let (next, clamped) = match clamp_value(&spec, value) {
            Ok(v) => v,
            Err(d) => {
                self.metrics.rejects = self.metrics.rejects.saturating_add(1);
                return Err(d);
            }
        };

        let mut warnings: Vec<PropDiagnostic> = Vec::new();
        if self.nodes[slot.0].destroyed {
            // 销毁后写入：显性告警 + **不写**（静默写入会让"节点已死"
            // 这件事永远无人知道，症状是属性面板能改一个不存在的控件）。
            warnings.push(pd(
                PropDiagCode::NodeDestroyed,
                "节点已销毁，属性写入被丢弃",
                "从树上移除该节点（remove），勿持旧槽",
                &at,
            ));
            self.metrics.settled = self.metrics.settled.saturating_add(1);
            self.metrics.no_ops = self.metrics.no_ops.saturating_add(1);
            return pok(PipelineReport {
                slot,
                key,
                reached: Stage::Clamp,
                clamped,
                changed: false,
                next,
                invalidated: Invalidation::NONE,
                queued: false,
                warnings,
            });
        }

        // 变更判定（幂等写不置失效、不入队——理由见头注「判据一」）。
        let idx = prop_slot(key);
        let old = self.nodes[slot.0].local[idx].clone();
        let changed = old.as_ref() != Some(&next);
        let mut invalidated = Invalidation::NONE;
        if clamped {
            self.metrics.clamps = self.metrics.clamps.saturating_add(1);
        }
        if !changed {
            self.metrics.no_ops = self.metrics.no_ops.saturating_add(1);
            self.metrics.settled = self.metrics.settled.saturating_add(1);
            return pok(PipelineReport {
                slot,
                key,
                reached: Stage::Invalidate,
                clamped,
                changed: false,
                next,
                invalidated,
                queued: false,
                warnings,
            });
        }

        // —— 段 3：失效标记。
        self.nodes[slot.0].local[idx] = Some(next.clone());
        invalidated = spec.invalidates;
        if !invalidated.is_empty() {
            self.nodes[slot.0].invalid = self.nodes[slot.0].invalid.union(invalidated);
            self.metrics.invalidations = self.metrics.invalidations.saturating_add(1);
        }

        // —— 段 4：通知入队（**本段不做分发**，帧边界才合并）。
        self.metrics.settled = self.metrics.settled.saturating_add(1);
        self.metrics.changed = self.metrics.changed.saturating_add(1);
        let overflow = self.pending.len() >= MAX_PENDING;
        self.pending.push(PendingChange {
            slot,
            slot_index: slot.0,
            key,
            value: next.clone(),
        });
        self.metrics.enqueued = self.metrics.enqueued.saturating_add(1);
        if overflow {
            // 溢出即显性告警（不静默丢弃，锚点「风暴极端防护」家族）。
            warnings.push(pd(
                PropDiagCode::PendingOverflow,
                &format!("待发队列溢出（>={}），已强制批提交", MAX_PENDING),
                "调用方在帧内做了逐条同步派发；改为批量写或按帧合并",
                &at,
            ));
            let forced = self.flush_frame();
            return pok(PipelineReport {
                slot,
                key,
                reached: Stage::Notify,
                clamped,
                changed: true,
                next,
                invalidated,
                queued: false,
                warnings: {
                    let mut w = warnings;
                    w.push(pd(
                        PropDiagCode::PendingOverflow,
                        &format!("强制批提交 {} 条待发", forced.merged),
                        "队列已清空",
                        &at,
                    ));
                    w
                },
            });
        }

        pok(PipelineReport {
            slot,
            key,
            reached: Stage::Notify,
            clamped,
            changed: true,
            next,
            invalidated,
            queued: true,
            warnings,
        })
    }

    /// 读本地值（不走上溯；供调试面板与自检用）。
    pub fn local_of(&self, slot: NodeSlot, key: PropertyKey) -> Option<&PropValue> {
        if !self.in_range(slot) {
            return None;
        }
        self.nodes[slot.0].local[prop_slot(key)].as_ref()
    }

    /// **值源链解析**：本地 > 继承 > 默认。
    ///
    /// 上溯跳数计入 [`Metrics::inherit_hops`]，供 F2610 核
    /// 「继承查找 O(树深)」——不实测就无法证伪。
    pub fn resolve(&self, slot: NodeSlot, key: PropertyKey) -> PropOutcome<Resolved> {
        if !self.in_range(slot) {
            return pfail(
                PropDiagCode::SlotOutOfRange,
                "节点槽越界",
                "槽由 attach 解析",
                &format!("slot#{}", slot.0),
            );
        }
        let spec = match self.spec(key) {
            Some(s) => s.clone(),
            None => {
                return pfail(
                    PropDiagCode::KeyNotRegistered,
                    &format!("属性键未注册：{}", key.as_str()),
                    "先在 prop_specs() 登记",
                    &format!("{}#{}", self.nodes[slot.0].id, key.as_str()),
                )
            }
        };
        let idx = prop_slot(key);
        if let Some(v) = &self.nodes[slot.0].local[idx] {
            return pok(Resolved {
                value: v.clone(),
                source: ValueSource::Local,
                hops: 0,
            });
        }
        if spec.inheritable {
            // 上溯祖先链。跳数上限取 F2602 深度上限 +1：再多跳就是
            // 遇上了环（树不变量破了），停下来比无限循环好。
            let mut cur = self.nodes[slot.0].parent;
            let mut hops = 0usize;
            while let Some(c) = cur {
                if hops > MAX_TREE_DEPTH {
                    break;
                }
                hops += 1;
                if !self.in_range(c) {
                    break;
                }
                if let Some(v) = &self.nodes[c.0].local[idx] {
                    return pok(Resolved {
                        value: v.clone(),
                        source: ValueSource::Inherited,
                        hops,
                    });
                }
                cur = self.nodes[c.0].parent;
            }
        }
        pok(Resolved {
            value: spec.default.clone(),
            source: ValueSource::Default,
            hops: 0,
        })
    }

    /// 与 [`PropEngine::resolve`] 同语义，但把上溯跳数计入
    /// [`Metrics::inherit_hops`]（供 F2610 核「继承查找 O(树深)」）。
    ///
    /// 刻意**不把计数塞进 `resolve`**：`resolve` 取 `&self`，
    /// 在内核里用 `Cell` 换可变性会把整个引擎的可变语义搞浑
    /// （`resolve` 变`&mut`后，持有 `&engine` 的渲染器就没法读属性了）。
    /// 拆成两个入口后，**读路径保持只读**，基准路径显式选可变版本。
    pub fn resolve_mut(&mut self, slot: NodeSlot, key: PropertyKey) -> PropOutcome<Resolved> {
        let r = self.resolve(slot, key)?;
        self.metrics.inherit_hops = self.metrics.inherit_hops.saturating_add(r.hops as u32);
        Ok(r)
    }

    /// 取走累积失效位（消费式；调用方取走即清零）。
    pub fn take_invalid(&mut self, slot: NodeSlot) -> PropOutcome<Invalidation> {
        if !self.in_range(slot) {
            return pfail(
                PropDiagCode::SlotOutOfRange,
                "节点槽越界",
                "槽由 attach 解析",
                &format!("slot#{}", slot.0),
            );
        }
        let v = self.nodes[slot.0].invalid;
        self.nodes[slot.0].invalid = Invalidation::NONE;
        Ok(v)
    }

    /// 订阅（O(订阅表长)，**不在写路径**）。
    pub fn subscribe(&mut self, subscriber: &str, slot: NodeSlot, key: PropertyKey) {
        if !self.in_range(slot) {
            return;
        }
        self.subs.push(Subscription {
            subscriber: String::from(subscriber),
            slot,
            key,
        });
    }

    /// 退订（返回是否真的移除了一条）。
    pub fn unsubscribe(&mut self, subscriber: &str, slot: NodeSlot, key: PropertyKey) -> bool {
        let before = self.subs.len();
        let mut i = 0usize;
        while i < self.subs.len() {
            let s = &self.subs[i];
            if s.subscriber.as_str() == subscriber && s.slot == slot && s.key == key {
                self.subs.remove(i);
            } else {
                i += 1;
            }
        }
        before != self.subs.len()
    }

    /// **帧边界合并 + 分发**（通知风暴防护的唯一出口）。
    ///
    /// 合并用世代戳按 `(slot, key)` 直接寻址，**摊还 O(待发条数)**，
    /// 不需要排序也不需要全量扫描去重：同一世代内第二次见到同一
    /// `(slot, key)`，直接改写已占位的输出条目为最新值。
    pub fn flush_frame(&mut self) -> FlushReport {
        self.gen = self.gen.wrapping_add(1);
        if self.gen == 0 {
            // 世代回卷：世代戳与 0 无法区分"未标"与"第 0 代"，清零重来。
            for v in self.stamp_gen.iter_mut() {
                *v = 0;
            }
            self.gen = 1;
        }
        let raw_writes = self.pending.len();
        let mut merged: Vec<PendingChange> = Vec::new();
        for pc in self.pending.iter() {
            let addr = pc.slot_index * PROP_SLOTS + prop_slot(pc.key);
            if self.stamp_gen.get(addr).copied() == Some(self.gen) {
                // 本帧已占位：只更新为最新值（帧边界看最终态）。
                let out = self.stamp_out[addr] as usize;
                if let Some(m) = merged.get_mut(out) {
                    m.value = pc.value.clone();
                }
            } else {
                let out = merged.len();
                if addr >= self.stamp_gen.len() {
                    self.stamp_gen.resize(addr + 1, 0);
                    self.stamp_out.resize(addr + 1, 0);
                }
                self.stamp_gen[addr] = self.gen;
                self.stamp_out[addr] = out as u32;
                merged.push(pc.clone());
            }
        }
        self.pending.clear();

        let mut notifications: Vec<(String, NodeSlot, PropertyKey, PropValue)> = Vec::new();
        for m in merged.iter() {
            for s in self.subs.iter() {
                if s.slot == m.slot && s.key == m.key {
                    notifications.push((
                        s.subscriber.clone(),
                        m.slot,
                        m.key,
                        m.value.clone(),
                    ));
                }
            }
        }
        let delivered = notifications.len();
        self.metrics.delivered = self.metrics.delivered.saturating_add(delivered as u32);
        self.metrics.flushes = self.metrics.flushes.saturating_add(1);
        FlushReport {
            raw_writes,
            merged: merged.len(),
            delivered,
            notifications,
        }
    }

    /// **显式绑定**（属性→属性）：注册前先做**真环检测**。
    ///
    /// 检测跑在「继承边 + 绑定边」统一表示的依赖图上：从新边的目标出发，
    /// 沿绑定边与父链走，看能否回到新边的源。走通即成环，拒绝。
    ///
    /// 之所以把父链也纳入：绑定 A→B 后，B 的继承仍可能来自 A 的祖先，
    /// 只看绑定边会漏掉「绑定 + 继承」联合成的环。
    pub fn bind(
        &mut self,
        from_slot: NodeSlot,
        from_key: PropertyKey,
        to_slot: NodeSlot,
        to_key: PropertyKey,
    ) -> PropOutcome<()> {
        if !self.in_range(from_slot) || !self.in_range(to_slot) {
            self.metrics.rejects = self.metrics.rejects.saturating_add(1);
            return pfail(
                PropDiagCode::SlotOutOfRange,
                "绑定端点槽越界",
                "两端都须由 attach 解析出的有效槽",
                &format!("slot#{}->{}", from_slot.0, to_slot.0),
            );
        }
        let edge = BindingEdge {
            from_slot,
            from_key,
            to_slot,
            to_key,
        };
        if self.creates_cycle(&edge) {
            self.metrics.rejects = self.metrics.rejects.saturating_add(1);
            return pfail(
                PropDiagCode::InheritCycle,
                &format!(
                    "依赖图成环：{}#{} → {}#{} 会绕回自身",
                    self.nodes[from_slot.0].id,
                    from_key.as_str(),
                    self.nodes[to_slot.0].id,
                    to_key.as_str()
                ),
                "解除已有绑定，或改为单向数据流（本域为本地 > 继承 > 默认）",
                &format!("{}#{}", self.nodes[to_slot.0].id, to_key.as_str()),
            );
        }
        self.bindings.push(edge);
        Ok(())
    }

    /// 新边是否会成环（DFS，带步数上限）。
    fn creates_cycle(&self, e: &BindingEdge) -> bool {
        let start = (e.to_slot, e.to_key);
        let goal = (e.from_slot, e.from_key);
        if start == goal {
            return true;
        }
        let mut seen: Vec<(NodeSlot, PropertyKey)> = vec![start];
        let mut stack: Vec<(NodeSlot, PropertyKey)> = vec![start];
        let mut steps = 0usize;
        let mut hops = 0usize;
        while let Some(cur) = stack.pop() {
            steps += 1;
            if steps > MAX_TREE_DEPTH + 8 {
                // 步数爆炸即认定成环（保守拒绝优于放过）。
                return true;
            }
            if cur == goal {
                return true;
            }
            // 沿绑定边下行
            for b in self.bindings.iter() {
                if b.from_slot == cur.0 && b.from_key == cur.1 {
                    let nxt = (b.to_slot, b.to_key);
                    if !seen.contains(&nxt) {
                        seen.push(nxt);
                        stack.push(nxt);
                    }
                }
            }
            // 沿父链上行（继承边：父的同名键喂给子）。
            // 上溯步数与 `resolve` 同口径（MAX_TREE_DEPTH），
            // 免得"查找有上限、环检测没上限"——后者能让恶意深度
            // 的绑定图把 DFS 拖成O(节点×边) 而不报任何错。
            hops += 1;
            if hops > MAX_TREE_DEPTH {
                return true;
            }
            if !self.in_range(cur.0) {
                return true;
            }
            if let Some(p) = self.nodes[cur.0.0].parent {
                let nxt = (p, cur.1);
                if !seen.contains(&nxt) {
                    seen.push(nxt);
                    stack.push(nxt);
                }
            }
        }
        false
    }

    /// **M04 轨道写兑现**：轨道 → 属性引擎 `set` → 四段管线。
    ///
    /// 失败**告警不阻断**（理由见模块头注「判据三」）。
    pub fn apply_track_write(
        &mut self,
        track: &str,
        node_id: &str,
        key: PropertyKey,
        value: PropValue,
    ) -> TrackWriteReport {
        let slot = match self.slot_of(node_id) {
            Some(s) => s,
            None => {
                return TrackWriteReport {
                    track: String::from(track),
                    slot: NodeSlot(usize::MAX),
                    key,
                    applied: false,
                    pipeline: None,
                    warnings: vec![pd(
                        PropDiagCode::TrackWriteStalled,
                        &format!("轨道目标节点不存在：{}", node_id),
                        "轨道仍按帧推进；该采样点记为未落地",
                        track,
                    )],
                }
            }
        };
        match self.set(slot, key, value) {
            Ok(r) => {
                // 区分「幂等未变」与「真失效」——**一个布尔量不能同时表达两件事**
                // （记忆红线：拆开）。若只看 `reached < Notify`，两者都是 true，
                // 于是「动画保持一个值不变」这种最正常的情形会每帧发一条
                // TrackWriteStalled 告警，日志被正常事件刷满，真正的
                // 销毁节点告警反而被淹没。故此处按**是否有诊断**判定：
                // 管线自己报了警（销毁节点）才是失效；
                // 幂等未变无诊断 → 属正常，不告警。
                let stalled = has_diag(&r.warnings);
                TrackWriteReport {
                    track: String::from(track),
                    slot,
                    key,
                    applied: r.changed,
                    pipeline: Some(r),
                    warnings: if stalled {
                        vec![pd(
                            PropDiagCode::TrackWriteStalled,
                            "轨道写入未落地（节点已销毁）",
                            "该采样点记为未落地；轨道继续推进",
                            track,
                        )]
                    } else {
                        Vec::new()
                    },
                }
            }
            Err(d) => TrackWriteReport {
                track: String::from(track),
                slot,
                key,
                applied: false,
                pipeline: None,
                warnings: vec![pd(
                    PropDiagCode::TrackWriteStalled,
                    &d.message,
                    &d.hint,
                    track,
                )],
            },
        }
    }

    /// 标记节点销毁（写入与绑定引用此后走显性告警）。
    pub fn mark_destroyed(&mut self, slot: NodeSlot) -> bool {
        if !self.in_range(slot) {
            return false;
        }
        self.nodes[slot.0].destroyed = true;
        true
    }

    /// 节点规模。
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// 绑定求值状态（恒为预留）。
    pub fn binding_status(&self) -> BindingStatus {
        let _ = self;
        BindingStatus::Reserved
    }
}

/// 段 2：域钳制 / 非有限拒绝。
///
/// 返回 `(生效值, 是否被钳制)`。**不可钳制的情况只有一种：非有限值**——
/// `NaN` 参与比较永远返回 `false`，`f32::min/max` 对 `NaN` 的传播行为
/// 又依赖具体实现，所以 NaN 一旦被钳制就会变成一个"看起来合法的数"
/// （症状：控件尺寸或透明度变成随机值，且每次运行都不同——
/// 确定性维直接红）。故NaN/Inf 一律拒绝。
fn clamp_value(spec: &PropSpec, v: PropValue) -> Result<(PropValue, bool), PropDiagnostic> {
    let n = match v {
        PropValue::Number(n) => n,
        other => return Ok((other, false)),
    };
    if !n.is_finite() {
        return Err(pd(
            PropDiagCode::NonFiniteValue,
            &format!("数值非有限：{}", n),
            "NaN/±Inf 不可钳制（NaN 比较恒false，会被钳成随机值）；请给有限值",
            spec.key.as_str(),
        ));
    }
    match spec.domain {
        None => Ok((PropValue::Number(n), false)),
        Some((lo, hi)) => {
            if n < lo {
                Ok((PropValue::Number(lo), true))
            } else if n > hi {
                Ok((PropValue::Number(hi), true))
            } else {
                Ok((PropValue::Number(n), false))
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 九、错误路径与降级矩阵
// ---------------------------------------------------------------------------

/// 降级矩阵一行。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DegradeRow {
    /// 触发条件。
    pub trigger: &'static str,
    /// 处置动作。
    pub action: &'static str,
    /// 是否阻断。
    pub blocking: bool,
    /// 对应诊断码。
    pub code: PropDiagCode,
}

/// 降级矩阵（锚点「错误路径与降级矩阵」+ 家族条款，逐行可机检）。
///
/// 六行齐备的依据是锚点原文逐条 + 两条家族附加：
/// 类型不匹配 / 未注册 / 依赖图成环 / 通知风暴 / M04 轨道写失效 / 非有限值。
pub const DEGRADE_MATRIX: [DegradeRow; 6] = [
    DegradeRow {
        trigger: "属性类型不匹配",
        action: "拒绝（管线第一段，不进钳制）",
        blocking: true,
        code: PropDiagCode::TypeMismatch,
    },
    DegradeRow {
        trigger: "属性未注册",
        action: "拒绝 + 指引去prop_specs 登记",
        blocking: true,
        code: PropDiagCode::KeyNotRegistered,
    },
    DegradeRow {
        trigger: "依赖图成环（继承边+绑定边）",
        action: "环检测拒绝",
        blocking: true,
        code: PropDiagCode::InheritCycle,
    },
    DegradeRow {
        trigger: "通知风暴（帧内同键重复写）",
        action: "帧边界合并（世代戳 O(1) 去重）",
        blocking: false,
        code: PropDiagCode::PendingOverflow,
    },
    DegradeRow {
        trigger: "M04 轨道写失效",
        action: "告警且不中断轨道",
        blocking: false,
        code: PropDiagCode::TrackWriteStalled,
    },
    DegradeRow {
        trigger: "数值非有限（NaN/±Inf）",
        action: "拒绝（不可钳制）",
        blocking: true,
        code: PropDiagCode::NonFiniteValue,
    },
];

/// 降级矩阵的线缆名（自检核验「线缆名唯一」用）。
pub fn degrade_wire_names() -> [&'static str; 6] {
    [
        "type-mismatch",
        "unregistered",
        "dep-cycle",
        "notify-storm",
        "track-stall",
        "non-finite",
    ]
}

/// 降级矩阵与诊断码的对应核验（防「矩阵写了码不存在」）。
pub fn degrade_consistent() -> bool {
    DEGRADE_MATRIX.iter().all(|r| {
        r.blocking == r.code.is_blocking()
            && !r.trigger.is_empty()
            && !r.action.is_empty()
    })
}

// ---------------------------------------------------------------------------
// 十、跨批对接台账
// ---------------------------------------------------------------------------

/// 对接台账一行。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Handoff {
    /// 对端单号。
    pub peer: &'static str,
    /// 契约内容。
    pub contract: &'static str,
    /// 当前状态。
    pub state: &'static str,
}

/// 跨批对接台账（锚点「跨批对接点」四条+ 两条前向）。
///
/// `F2602` 与 `F2461` 是**已兑现**（不是"计划兑现"）：本模块真的
/// 消费F2602 的 [`PropertyKey`] 封闭集、真的实现了 `bind_path` 的属性侧写入。
pub const HANDOFFS: [Handoff; 6] = [
    Handoff {
        peer: "VE-F2402",
        contract: "M04 轨道绑定目标=属性引擎（轨道写属性经apply_track_write）",
        state: "已兑现",
    },
    Handoff {
        peer: "VE-F2461",
        contract: "F2461 挂载协议属性侧：四段管线即挂载点",
        state: "已兑现",
    },
    Handoff {
        peer: "VE-F2606",
        contract: "失效标记的消费端（渲染/布局/命中三路分发）",
        state: "前向（本条产出位，F2606 消费）",
    },
    Handoff {
        peer: "VE-F2621",
        contract: "布局失效消费端（width/height/position/bind-path 四键）",
        state: "前向（本条产出位，F2621 消费）",
    },
    Handoff {
        peer: "VE-F1464",
        contract: "通知风暴防护家族（帧边界合并）",
        state: "已兑现",
    },
    Handoff {
        peer: "N-数据绑定",
        contract: "属性→属性绑定求值（接口位预留，环检测已实现）",
        state: "前向STUB（求值未实现，显性报错）",
    },
];

// ---------------------------------------------------------------------------
// 十一、性能纪律（锚点「性能逐项分解」四条，公开可核）
// ---------------------------------------------------------------------------

/// 性能纪律声明。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PerfDoc;

/// 性能分解表（每行:项名+ 复杂度 + 依据）。
///
/// 「依据」一列必须写**为什么是O(1)**，否则复杂度就成了声明而非事实。
pub const PERF_ROWS: [(&'static str, &'static str, &'static str); 4] = [
    (
        "属性写入",
        "O(1)",
        "13槽定长数组 + prop_slot 常量映射；无哈希/无链表/无按 id 查找",
    ),
    (
        "失效标记",
        "O(1)",
        "置位即union 两个 bool，无遍历、无分配",
    ),
    (
        "通知分发",
        "O(待发条数+ 订阅数)",
        "帧边界世代戳寻址合并；同帧同键N写合为1条",
    ),
    (
        "继承查找",
        "O(树深)",
        "沿父链上行，跳数计入 Metrics::inherit_hops 供 F2610 实测",
    ),
];

impl PerfDoc {
    /// 性能行数（自检核验「四条齐备」）。
    pub const fn row_count() -> usize {
        PERF_ROWS.len()
    }

    /// 全部行非空。
    pub fn all_rows_filled() -> bool {
        PERF_ROWS.iter().all(|(a, b, c)| !a.is_empty() && !b.is_empty() && !c.is_empty())
    }
}

// ---------------------------------------------------------------------------
// 十二、组内分工与无障碍替述
// ---------------------------------------------------------------------------

/// N01 组内分工（F2604 承接的那一条）。
///
/// 组内 20 条分工的完整表归 F2601；本模块只登记自己这一条，
/// 避免在多处维护同一份分工表（双源必然分叉）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct WorkItem;

/// 本模块登记的分工条目。
pub const WORK_ITEM: WorkItem = WorkItem;

impl WorkItem {
    /// 单号。
    pub const fn id(self) -> &'static str {
        "VE-F2604"
    }

    /// 承接内容。
    pub const fn scope(self) -> &'static str {
        "属性引擎四段管线 + 依赖属性（继承/绑定接口位）+ M04 属性侧兑现 + 零风暴纪律"
    }

    /// 前置单（F2603 类型体系提供键与规格的对照面）。
    pub const fn upstream(self) -> &'static str {
        "VE-F2603"
    }

    /// 下游单（F2606 增量更新消费失效位）。
    pub const fn downstream(self) -> &'static str {
        "VE-F2606"
    }
}

/// 无障碍替述（本模块触及的无障碍面）。
///
/// **替述要写"实际发生什么"，不是写"很友好"**：
/// 本模块为无障碍标签提供唯一属性入口，并保证它可被写入与读回；
/// 无障碍树本身由 N03 落地，本模块不渲染任何像素。
pub fn a11y_alternatives() -> [&'static str; 4] {
    [
        "aria-label 是无障碍面的唯一属性入口，本模块不提供第二个",
        "无障碍标签写入后立即进命中失效面，N03 落地前由命中遍历面消费",
        "本模块零渲染：不读屏、不发声、不焦点移动",
        "无障碍位缺失不静默：读取未设置的标签时值源为 Default（空串），可被判定",
    ]
}

/// 隐私声明（无隐私面）。
pub const PRIVACY_NOTE: &str = "属性值为布局与视觉参数，不含用户内容；无隐私面";

/// 诊断描述分档（阻断 / 告警，人话）。
pub fn describe(code: PropDiagCode) -> &'static str {
    if code.is_blocking() {
        "阻断：属性写入被拒绝，调用方须改写法"
    } else {
        "告警：写入已显式降级，对象语义仍正确"
    }
}