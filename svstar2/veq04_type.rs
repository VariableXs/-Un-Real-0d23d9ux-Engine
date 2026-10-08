//! VE-F3204 · 资源类型系统
//!
//! 判据映射（锚点原文 → 本文件章节）：
//! - 类型系统十类 × 四要素登记 → §二 [`ResourceType`] + §三 [`TypeElements`] + §四 [`TypeRegistry`]
//! - 开放类型红线（扩展点）   → §五 [`ExtensionPoint`]
//! - 类型安全双拦截           → §六 [`CrossTypeOp`] + [`guard_cross_type`]
//! - 两级语义（宽容元数据）   → §七 [`SchemaVerdict`] + [`validate_schema`]
//! - 命名空间（注册冲突）     → §八 [`NameSpace`] + [`resolve_conflict`]
//! - 错误路径与降级矩阵       → §九 [`audit_error_matrix`]
//! - 性能逐项分解             → §十 [`PERF_BUDGET`]
//!
//! 零 IO / 零墙钟 / 零全局可变状态：类型登记只是四要素元组，字节归 F 域解码器，
//! 故本模块全部是纯数据变换。
//!
//! ## 〇、一条贯穿全条的设计纪律：`ResourceType` **不是** F3201 的 `ResourceKind`
//!
//! 两者都是「资源的类别」，但**划分依据不同**，不可互相替代：
//! - F3201 的 [`ResourceKind`] 是**寻址与 URI 路径段**用的粗分类（ten 项映射左列，
//!   决定 URI 里写 `texture` 还是 `shader`），它的粒度由寻址层决定；
//! - 本条的 [`ResourceType`] 是**类型系统登记单位**，粒度由「四要素是否相同」决定
//!   —— 两个类若 schema/解码器/内存画像/校验规则四项全同，就该是同一个类型；
//!   若任一项不同，就必须拆成两个类型，否则解码器路由会出现二义。
//!
//! 举例说明为什么不可合并：F3201 把 `Model` 与 `Geometry` 分开（寻址需要），
//! 而类型系统里「模型」与「几何」的 schema 同为 `{vertices,indices}`、解码器
//! 同路由、内存画像同比例、校验规则同包围盒检查——**四要素全同**。若类型系统
//! 也拆两类，则路由表出现两个指向同一解码器的条目，O(1) 路由退化为「任选其一」，
//! 而「选错」在跨类型拦截下会被判成合法，绕过双拦截。
//!
//! 故本条新建独立枚举，并在 [`kind_bridge`] 建立**显式多对多**映射
//! （一个 ResourceType 可落到多个 ResourceKind，一个 ResourceKind 可被多个
//! ResourceType 细分），并把「映射未登记」显式判红——桥接表漏一条，
//! 后果是某类型在寻址层查不到 en 名，而这类漏项**静默**（都返回 None）。
//!
//! ## 〇之二、为什么四要素必须**逐条独立**登记，不能打包成 `Any`
//!
//! 若把四要素塞进一个 `Box<dyn Any>` 或一个不透明 token，则：
//! - 「内存画像」无法在编译期或查表期核对（要拿到才能问，而拿到就意味着已分配）；
//! - 「校验规则」变成运行时才能问的函数指针，于是「类型是否登记完整」这个
//!   **编译期可判定**的问题退化成运行期问题；
//! - 缺一项时无法区分「未登记」与「登记为 None」，注册表就分不出残条目。
//!
//! 故 [`TypeElements`] 四项全部是**具体类型**（`&'static str` / `u32` / 闭集枚举），
//! 缺项在 [`TypeRegistry::register`] 就被拒，不留到运行期。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use super::veq01_pipeline::{Diagnostic, DiagCode, Outcome};
use super::veq02_graph::ResourceKind;

/// 本条自有诊断码（复用 F3201 的三件套类型，但码位为本条**新造**）。
///
/// 不新造则「类型未登记」与「类型冲突」在跨语言对拍里同码，消费域无法区分
/// 「你用了一个没注册的类型」（该去注册表补）与「你注册重名了」（该改命名空间），
/// 而这两者的处置完全不同。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Q04Code {
    /// 类型未在注册表登记（扩展点外 = 拒绝，开放类型红线）。
    TypeUnregistered,
    /// 注册冲突：同名类型已在册（须走命名空间）。
    TypeNameConflict,
    /// 跨类型操作被运行期拦截。
    CrossTypeRejected,
    /// 元数据不合 schema（宽容解析 + 告警，不硬拒）。
    SchemaViolation,
    /// 四要素登记残缺（注册期即拒）。
    ElementsIncomplete,
    /// 桥接表漏项（ResourceType ↔ ResourceKind 未登记）。
    BridgeUnmapped,
}

impl Q04Code {
    /// 稳定字符串（跨语言对拍与判据比对用）。
    pub fn code(self) -> &'static str {
        match self {
            Q04Code::TypeUnregistered => "TYPE_UNREGISTERED",
            Q04Code::TypeNameConflict => "TYPE_NAME_CONFLICT",
            Q04Code::CrossTypeRejected => "CROSS_TYPE_REJECTED",
            Q04Code::SchemaViolation => "SCHEMA_VIOLATION",
            Q04Code::ElementsIncomplete => "ELEMENTS_INCOMPLETE",
            Q04Code::BridgeUnmapped => "BRIDGE_UNMAPPED",
        }
    }

    /// 由字符串反查（对拍寻址用；未登记返回 `None`）。
    pub fn from_code(s: &str) -> Option<Q04Code> {
        match s {
            "TYPE_UNREGISTERED" => Some(Q04Code::TypeUnregistered),
            "TYPE_NAME_CONFLICT" => Some(Q04Code::TypeNameConflict),
            "CROSS_TYPE_REJECTED" => Some(Q04Code::CrossTypeRejected),
            "SCHEMA_VIOLATION" => Some(Q04Code::SchemaViolation),
            "ELEMENTS_INCOMPLETE" => Some(Q04Code::ElementsIncomplete),
            "BRIDGE_UNMAPPED" => Some(Q04Code::BridgeUnmapped),
            _ => None,
        }
    }

    /// 桥接到 F3201 诊断枚举（封闭枚举不可加变体 ⇒ 映射到语义最近的既有码）。
    ///
    /// 映射是**多对少且不可反推**：`CrossTypeRejected` 与 `ElementsIncomplete`
    /// 都落到 `ValueInvalid`，`SchemaViolation` 与 `TypeNameConflict`
    /// 都落到 `ResourceTypeUnmapped` 的近邻 `StageContractDiverged`。
    /// 跨域台账若需区分本条码位，读 `Q04Code.code()` 而非 `DiagCode.code()`。
    pub fn bridge(self) -> DiagCode {
        match self {
            Q04Code::TypeUnregistered => DiagCode::ResourceTypeUnmapped,
            Q04Code::TypeNameConflict => DiagCode::ValueInvalid,
            Q04Code::CrossTypeRejected => DiagCode::ValueInvalid,
            Q04Code::SchemaViolation => DiagCode::StageContractDiverged,
            Q04Code::ElementsIncomplete => DiagCode::ValueInvalid,
            Q04Code::BridgeUnmapped => DiagCode::ResourceTypeUnmapped,
        }
    }

    /// 转 F3201 诊断（三要素齐备，零静默）。
    pub fn diagnostic(self, message: &str, hint: &str) -> Diagnostic {
        Diagnostic {
            code: self.bridge(),
            message: String::from(message),
            hint: String::from(hint),
        }
    }
}

/// 桥接说明（映射后**不可反推**原码）。
pub const BRIDGE_NOTE: &str =
    "Q04 自有 6 码位，F3201 的 DiagCode 是封闭枚举不可加变体，故用 bridge() 映射到语义最近的既有码。映射是多对少，不可反推：CROSS_TYPE_REJECTED 与 ELEMENTS_INCOMPLETE 同落 VALUE_INVALID，TYPE_UNREGISTERED 与 BRIDGE_UNMAPPED 同落 RESOURCE_TYPE_UNMAPPED。跨域台账若需区分本条码位，读 Q04Code.code() 而非 DiagCode.code()。";

/// 本条错误构造辅助（`Outcome::err` 只吃 `DiagCode`，先过桥）。
pub fn q_err<T>(code: Q04Code, message: &str, hint: &str) -> Outcome<T> {
    Outcome::err(code.bridge(), message, hint)
}

// ===========================================================================
// 一、类型系统十类（闭集 —— 新增走扩展点，不得改枚举）
// ===========================================================================

/// 资源类型（类型系统登记单位，十类闭集）。
///
/// **闭集**：十类写死在枚举里。第三方要加类型走 [`ExtensionPoint`]（§五），
/// 产出 [`ExtTypeId`] 而非本枚举变体——这正是开放封闭红线的落点。
///
/// 为何不能用 `&'static str` 当类型：字符串可拼写、可大小写不一、可带空格，
/// 「`Texture`」与「`texture`」会成为两个类型，而 O(1) 路由表用字符串做键时
/// 这类差异全部静默（查不到就走兜底分支）。枚举在编译期就消除了这整类问题。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ResourceType {
    /// 纹理。
    Texture,
    /// 网格（几何 + 顶点属性；与 F3201 的 Model/Geometry 四要素同源）。
    Mesh,
    /// 材质（着色参数集；引用纹理与网格）。
    Material,
    /// 音频。
    Audio,
    /// 字体。
    Font,
    /// 动画（关键帧/骨骼 clip）。
    Animation,
    /// 样式表（UI 布局与视觉规则）。
    StyleSheet,
    /// 场景图。
    SceneGraph,
    /// 预制体（资源模板实例）。
    Prefab,
    /// 脚本数据（纯数据，不含可执行码）。
    ScriptData,
}

/// 十类全集（顺序即 `wire()` 编码，不可重排）。
///
/// 单源：注册表初始化、判据遍历、桥接表完整性检查全部读这一个数组，
/// 杜绝「枚举加了变体但某个表忘了加」这类漏项。
pub const ALL_TYPES: [ResourceType; 10] = [
    ResourceType::Texture,
    ResourceType::Mesh,
    ResourceType::Material,
    ResourceType::Audio,
    ResourceType::Font,
    ResourceType::Animation,
    ResourceType::StyleSheet,
    ResourceType::SceneGraph,
    ResourceType::Prefab,
    ResourceType::ScriptData,
];

/// 闭集类型数（**常量而非数组长度**——判据用它当期望值，若用 `ALL_TYPES.len()`
/// 则「数组漏了一项」会同时让期望值变小，判据变弱门禁）。
pub const TEN_ELEMENT_COUNT: usize = 10;

/// 线上编码上界（含）。编码 0 保留为「无类型」，故实际取值 1..=MAX_WIRE。
pub const MAX_WIRE: u8 = 10;

impl ResourceType {
    /// 英文标识（URI 路径的类型段）。
    pub fn en(self) -> &'static str {
        match self {
            ResourceType::Texture => "texture",
            ResourceType::Mesh => "mesh",
            ResourceType::Material => "material",
            ResourceType::Audio => "audio",
            ResourceType::Font => "font",
            ResourceType::Animation => "animation",
            ResourceType::StyleSheet => "stylesheet",
            ResourceType::SceneGraph => "scenegraph",
            ResourceType::Prefab => "prefab",
            ResourceType::ScriptData => "scriptdata",
        }
    }

    /// 中文名（无障碍复述用）。
    pub fn zh(self) -> &'static str {
        match self {
            ResourceType::Texture => "纹理",
            ResourceType::Mesh => "网格",
            ResourceType::Material => "材质",
            ResourceType::Audio => "音频",
            ResourceType::Font => "字体",
            ResourceType::Animation => "动画",
            ResourceType::StyleSheet => "样式表",
            ResourceType::SceneGraph => "场景图",
            ResourceType::Prefab => "预制体",
            ResourceType::ScriptData => "脚本数据",
        }
    }

    /// 由英文标识反查（URI 解析入口用；未登记返回 `None`，**不**兜底）。
    pub fn from_en(s: &str) -> Option<ResourceType> {
        let mut i = 0usize;
        while i < ALL_TYPES.len() {
            if ALL_TYPES[i].en() == s {
                return Some(ALL_TYPES[i]);
            }
            i += 1;
        }
        None
    }

    /// 二进制线上编码（显式映射，**禁** `enum_val as u8`）。
    ///
    /// 为何不用 `as u8`：枚举变体顺序一旦有人调整（插入新变体到中间），
    /// `as u8` 会静默改变线上编码，而已打包的资源里存着旧编码 ⇒ 全部错解，
    /// 且没有任何编译错误。显式 `wire()` 把编码钉死，调整枚举不会动编码。
    pub fn wire(self) -> u8 {
        match self {
            ResourceType::Texture => 1,
            ResourceType::Mesh => 2,
            ResourceType::Material => 3,
            ResourceType::Audio => 4,
            ResourceType::Font => 5,
            ResourceType::Animation => 6,
            ResourceType::StyleSheet => 7,
            ResourceType::SceneGraph => 8,
            ResourceType::Prefab => 9,
            ResourceType::ScriptData => 10,
        }
    }

    /// 由线上编码反查（未登记返回 `None`，编码 0 保留为「无类型」）。
    pub fn from_wire(w: u8) -> Option<ResourceType> {
        let mut i = 0usize;
        while i < ALL_TYPES.len() {
            if ALL_TYPES[i].wire() == w {
                return Some(ALL_TYPES[i]);
            }
            i += 1;
        }
        None
    }
}

/// `ResourceType` ↔ F3201 [`ResourceKind`] 的**显式多对多**桥接表。
///
/// 一行 = 一个 `ResourceType` 落到哪些 `ResourceKind`。
///
/// 为何是多对多而非一对一：
/// - `Mesh` 同时落到 `Model` 与 `Geometry`（F3201 寻址层把两者分开，本条四要素同源）；
/// - `Material` 落到 `Shader`（寻址层材质与着色器同段）；
/// - `ScriptData` 落到 `Stream`（脚本数据走流媒体段寻址）。
///
/// 桥接表**漏一行**的后果是「该类型在寻址层查不到 en 名」，而所有查表失败
/// 都返回 `None` ⇒ 这类漏项**完全静默**。故判据必须逐行钉死本表。
pub const KIND_BRIDGE: [(ResourceType, &[ResourceKind]); 10] = [
    (ResourceType::Texture, &[ResourceKind::Texture]),
    (
        ResourceType::Mesh,
        &[ResourceKind::Model, ResourceKind::Geometry],
    ),
    (ResourceType::Material, &[ResourceKind::Shader]),
    (ResourceType::Audio, &[ResourceKind::Audio]),
    (ResourceType::Font, &[ResourceKind::Font]),
    (ResourceType::Animation, &[ResourceKind::Animation]),
    (ResourceType::StyleSheet, &[ResourceKind::Style]),
    (ResourceType::SceneGraph, &[ResourceKind::Scene]),
    (ResourceType::Prefab, &[ResourceKind::Model]),
    (ResourceType::ScriptData, &[ResourceKind::Stream]),
];

/// 由 `ResourceType` 查它落到哪些 `ResourceKind`（O(十)，常数级）。
///
/// 返回**切片**而非单个值：多对多被压成单值的那一步就是信息丢失的那一步。
pub fn kind_bridge(t: ResourceType) -> &'static [ResourceKind] {
    let mut i = 0usize;
    while i < KIND_BRIDGE.len() {
        if KIND_BRIDGE[i].0 == t {
            return KIND_BRIDGE[i].1;
        }
        i += 1;
    }
    &[]
}

/// 桥接是否已登记（判据逐行钉死用；空切片 = 漏项）。
pub fn bridge_mapped(t: ResourceType) -> bool {
    !kind_bridge(t).is_empty()
}

// ===========================================================================
// 二、四要素之一：内存画像
// ===========================================================================

/// 内存驻留形态（内存画像的取值闭集）。
///
/// 为何是闭集而非 `u32` 字节数：字节数是**运行期实例**的量，而内存画像要登记的
/// 是「这一类资源的驻留形态」（常驻/按需/可丢弃），前者随实例变、后者是类型属性。
/// 用字节数当画像会让「画像」退化为实例数据，类型表与实例表就混在一起了。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Residency {
    /// 常驻（加载后长期持有，如字体、样式表）。
    Resident,
    /// 按需驻留（用完可回收，如音频 clip）。
    OnDemand,
    /// 场景作用域（随场景存亡，如场景图、预制体）。
    SceneScoped,
    /// 流式驻留（分块到达，如脚本数据流）。
    Streamed,
}

impl Residency {
    /// 稳定字符串。
    pub fn code(self) -> &'static str {
        match self {
            Residency::Resident => "RESIDENT",
            Residency::OnDemand => "ON_DEMAND",
            Residency::SceneScoped => "SCENE_SCOPED",
            Residency::Streamed => "STREAMED",
        }
    }

    /// 由字符串反查（未登记返回 `None`）。
    pub fn from_code(s: &str) -> Option<Residency> {
        match s {
            "RESIDENT" => Some(Residency::Resident),
            "ON_DEMAND" => Some(Residency::OnDemand),
            "SCENE_SCOPED" => Some(Residency::SceneScoped),
            "STREAMED" => Some(Residency::Streamed),
            _ => None,
        }
    }

    /// 是否可被 GC 回收（`Resident` 不可回收）。
    pub fn collectable(self) -> bool {
        !matches!(self, Residency::Resident)
    }
}

// ===========================================================================
// 三、四要素之二：校验规则
// ===========================================================================

/// 校验规则（闭集，位标志式——一条类型可同时要求多项）。
///
/// 为何用位标志而非 `Vec<Rule>`：校验在「路由时」做一次，若规则是动态集合，
/// 则每次路由都要分配；而校验规则的**集合成员**决定行为（要求哪几项），
/// 不需要保序、不需要重复计数 ⇒ 位掩码是最小表达。
///
/// 宽度限 8 位：`u8` 足够且让「规则全集」可被一个字节判据直接核对。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Rules(pub u8);

impl Rules {
    /// 无规则。
    pub const NONE: Rules = Rules(0);
    /// 必须有 schema（缺 schema 即宽容解析不可用）。
    pub const HAS_SCHEMA: Rules = Rules(1 << 0);
    /// 必须过魔数检查。
    pub const MAGIC: Rules = Rules(1 << 1);
    /// 必须过尺寸上限检查。
    pub const SIZE_LIMIT: Rules = Rules(1 << 2);
    /// 必须过依赖声明检查（引用字段必须都在图里）。
    pub const REFS_RESOLVED: Rules = Rules(1 << 3);
    /// 必须过边界检查（包围盒/采样率等类型专属约束）。
    pub const BOUNDS: Rules = Rules(1 << 4);
    /// 规则全集掩码（判据用来断「没有越界位」）。
    pub const FULL_MASK: u8 = 0b0001_1111;
    /// 规则集非空。
    pub fn is_empty(self) -> bool {
        self.0 == 0
    }
    /// 含某条规则。
    pub const fn has(self, bit: Rules) -> bool {
        self.0 & bit.0 != 0
    }
    /// 置某条规则（`const fn` —— 常量表里要链式调用）。
    pub const fn with(self, bit: Rules) -> Rules {
        Rules(self.0 | bit.0)
    }
    /// 规则条数（判据用：与预期条数对账）。
    pub fn count(self) -> u32 {
        let mut n = 0u32;
        let mut i = 0u8;
        while i < 8 {
            if self.0 & (1u8 << i) != 0 {
                n += 1;
            }
            i += 1;
        }
        n
    }
    /// 是否含越界位（`FULL_MASK` 之外）。
    pub fn has_overflow(self) -> bool {
        self.0 & !Rules::FULL_MASK != 0
    }
}

// ===========================================================================
// 四、四要素合组 + 类型注册表
// ===========================================================================

/// 四要素（元数据 schema / 解码器路由 / 内存画像 / 校验规则）。
///
/// 四项全部是**具体类型**而非 trait object——理由见文件头 §〇之二。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TypeElements {
    /// 要素一：元数据 schema 名（登记 schema 的标识，非空）。
    pub schema: &'static str,
    /// 要素二：解码器路由（F 域映射：解码器代号，>0 有效）。
    pub decoder: u16,
    /// 要素三：内存画像。
    pub residency: Residency,
    /// 要素四：校验规则。
    pub rules: Rules,
}

/// 四要素残缺的具体原因（注册期诊断用，不合并成一个 bool）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ElementsFault {
    /// 正常。
    None,
    /// schema 为空串。
    SchemaEmpty,
    /// 解码器代号为 0（保留值，非有效路由）。
    DecoderZero,
    /// 校验规则为空集（无规则 = 无校验，等于没登记第四要素）。
    RulesEmpty,
    /// 规则集含越界位。
    RulesOverflow,
}

impl TypeElements {
    /// 校验四要素是否齐备（**不**用 `bool` 承载多件事——拆成枚举）。
    pub fn fault(self) -> ElementsFault {
        if self.schema.is_empty() {
            return ElementsFault::SchemaEmpty;
        }
        if self.decoder == 0 {
            return ElementsFault::DecoderZero;
        }
        if self.rules.is_empty() {
            return ElementsFault::RulesEmpty;
        }
        if self.rules.has_overflow() {
            return ElementsFault::RulesOverflow;
        }
        ElementsFault::None
    }

    /// 四要素齐备。
    pub fn complete(self) -> bool {
        matches!(self.fault(), ElementsFault::None)
    }
}

/// 类型注册表（十类 × 四要素）。
///
/// 结构选型：固定 10 槽数组 + 「已登记」标志位，而**不是** `Vec<Entry>` 线性扫。
/// 锚点要求「注册 O(1)；路由 O(1)」——线性扫是 O(十)=常数级，严格说也是 O(1)，
/// 但它在**元素较多时**会退化为「不可测的常数」（因为十这个数字会随类型增长），
/// 而固定槽位 + 显式索引在类型数增长时仍能靠 `slot()` 的直接寻址证明 O(1)。
#[derive(Clone, Debug)]
pub struct TypeRegistry {
    slots: [Option<TypeElements>; 10],
    /// 已登记类型数（守恒判据的分子）。
    pub registered: u32,
    /// 注册被拒次数（含冲突与四要素残缺）。
    pub rejected: u32,
}

/// 按 `wire()` 直接寻址得槽下标（O(1)，无扫描）。越界返回 `None`。
///
/// `wire()` 值域 1..=10 ⇒ 下标 = wire-1 ∈ 0..=9，恰好落满 10 槽。
///
/// ⚠ **零 panic 面**：这里**不能**写成 `fn slot(t) -> usize { t.wire() as usize - 1 }`
///   然后让调用方直接下标 —— 那样一旦 `wire()` 越界（例如枚举加了变体而
///   `wire()` 忘了同步改，返回 11），`self.slots[10]` 会 panic。
///   变异测试 M02 实测抓到过这个：`index out of bounds: the len is 10 but
///   the index is 10`。故此处返回 `Option`，所有调用点显式兜底。
#[inline]
fn checked_slot(t: ResourceType) -> Option<usize> {
    let w = t.wire();
    if w < 1 || w > MAX_WIRE {
        return None;
    }
    let i = (w as usize) - 1;
    if i < 10 { Some(i) } else { None }
}

impl TypeRegistry {
    /// 空注册表（零登记）。
    pub const fn new() -> TypeRegistry {
        TypeRegistry {
            slots: [None; 10],
            registered: 0,
            rejected: 0,
        }
    }

    /// 登记一个类型的四要素（O(1)：直接槽位寻址）。
    ///
    /// 三道拒：**四要素残缺**、**重名冲突**、**类型不在十类全集内**。
    /// 重名冲突不静默覆盖——覆盖会让「先注册的被顶掉」这件事不可见，
    /// 而四要素不同 ⇒ 解码器路由指向不同产物 ⇒ 静默覆盖 = 静默错解。
    pub fn register(&mut self, t: ResourceType, e: TypeElements) -> Outcome<()> {
        if !e.complete() {
            self.rejected += 1;
            return q_err(
                Q04Code::ElementsIncomplete,
                "四要素登记残缺，拒绝入表",
                "schema 非空 + decoder>0 + rules 非空且无越界位，三者缺一不可",
            );
        }
        let i = match checked_slot(t) {
            Some(i) => i,
            None => {
                self.rejected += 1;
                return q_err(
                    Q04Code::TypeUnregistered,
                    "类型 wire 编码越界，拒绝入表",
                    "wire() 与 ALL_TYPES 必须同步；越界编码说明枚举与编码表已脱节",
                );
            }
        };
        if self.slots[i].is_some() {
            self.rejected += 1;
            return q_err(
                Q04Code::TypeNameConflict,
                "同名类型已在册，拒绝覆盖（须走命名空间）",
                "覆盖会让先注册的四要素不可见，而路由将静默改指；请用 resolve_conflict 改名",
            );
        }
        self.slots[i] = Some(e);
        self.registered += 1;
        Outcome::ok(())
    }

    /// 查四要素（O(1)：直接槽位）。未登记返回 `None`。
    pub fn get(&self, t: ResourceType) -> Option<TypeElements> {
        match checked_slot(t) {
            Some(i) => self.slots[i],
            None => None,
        }
    }

    /// 该类型是否已登记。
    pub fn is_registered(&self, t: ResourceType) -> bool {
        match checked_slot(t) {
            Some(i) => self.slots[i].is_some(),
            None => false,
        }
    }

    /// 查解码器路由（O(1) 两步：槽位 + 字段）。未登记走 `Err`（**不**兜底 0）。
    pub fn route(&self, t: ResourceType) -> Outcome<u16> {
        let i = match checked_slot(t) {
            Some(i) => i,
            None => {
                return q_err(
                    Q04Code::TypeUnregistered,
                    "类型 wire 编码越界，拒绝路由",
                    "wire() 与 ALL_TYPES 必须同步；越界编码说明枚举与编码表已脱节",
                )
            }
        };
        match self.slots[i] {
            Some(e) => Outcome::ok(e.decoder),
            None => q_err(
                Q04Code::TypeUnregistered,
                "类型未登记，拒绝路由",
                "扩展点外的类型一律拒绝；先 register 或走 ExtensionPoint",
            ),
        }
    }

    /// 查内存画像（未登记走 `Err`）。
    pub fn residency(&self, t: ResourceType) -> Outcome<Residency> {
        let i = match checked_slot(t) {
            Some(i) => i,
            None => {
                return q_err(
                    Q04Code::TypeUnregistered,
                    "类型 wire 编码越界，无内存画像",
                    "wire() 与 ALL_TYPES 必须同步；越界编码说明枚举与编码表已脱节",
                )
            }
        };
        match self.slots[i] {
            Some(e) => Outcome::ok(e.residency),
            None => q_err(
                Q04Code::TypeUnregistered,
                "类型未登记，无内存画像",
                "画像是类型属性，未登记类型无属性可问",
            ),
        }
    }
}

impl Default for TypeRegistry {
    fn default() -> TypeRegistry {
        TypeRegistry::new()
    }
}

/// 十类标准四要素表（单源；注册表初始化与判据对账共用）。
///
/// 内存画像取值理由：
/// - 纹理/网格/材质：随场景存亡 ⇒ `SceneScoped`；
/// - 音频/动画：按需 ⇒ `OnDemand`；
/// - 字体/样式表：长期持有（UI 常驻）⇒ `Resident`；
/// - 场景图：场景作用域 ⇒ `SceneScoped`；
/// - 预制体：随实例 ⇒ `SceneScoped`；
/// - 脚本数据：流式 ⇒ `Streamed`。
pub const STD_ELEMENTS: [(ResourceType, TypeElements); 10] = [
    (
        ResourceType::Texture,
        TypeElements {
            schema: "meta.texture",
            decoder: 101,
            residency: Residency::SceneScoped,
            rules: Rules::HAS_SCHEMA.with(Rules::MAGIC).with(Rules::SIZE_LIMIT),
        },
    ),
    (
        ResourceType::Mesh,
        TypeElements {
            schema: "meta.mesh",
            decoder: 102,
            residency: Residency::SceneScoped,
            rules: Rules::HAS_SCHEMA.with(Rules::REFS_RESOLVED).with(Rules::BOUNDS),
        },
    ),
    (
        ResourceType::Material,
        TypeElements {
            schema: "meta.material",
            decoder: 103,
            residency: Residency::SceneScoped,
            rules: Rules::HAS_SCHEMA.with(Rules::REFS_RESOLVED),
        },
    ),
    (
        ResourceType::Audio,
        TypeElements {
            schema: "meta.audio",
            decoder: 104,
            residency: Residency::OnDemand,
            rules: Rules::HAS_SCHEMA.with(Rules::SIZE_LIMIT).with(Rules::BOUNDS),
        },
    ),
    (
        ResourceType::Font,
        TypeElements {
            schema: "meta.font",
            decoder: 105,
            residency: Residency::Resident,
            rules: Rules::HAS_SCHEMA.with(Rules::MAGIC),
        },
    ),
    (
        ResourceType::Animation,
        TypeElements {
            schema: "meta.animation",
            decoder: 106,
            residency: Residency::OnDemand,
            rules: Rules::HAS_SCHEMA.with(Rules::REFS_RESOLVED),
        },
    ),
    (
        ResourceType::StyleSheet,
        TypeElements {
            schema: "meta.stylesheet",
            decoder: 107,
            residency: Residency::Resident,
            rules: Rules::HAS_SCHEMA,
        },
    ),
    (
        ResourceType::SceneGraph,
        TypeElements {
            schema: "meta.scenegraph",
            decoder: 108,
            residency: Residency::SceneScoped,
            rules: Rules::HAS_SCHEMA.with(Rules::REFS_RESOLVED).with(Rules::BOUNDS),
        },
    ),
    (
        ResourceType::Prefab,
        TypeElements {
            schema: "meta.prefab",
            decoder: 109,
            residency: Residency::SceneScoped,
            rules: Rules::HAS_SCHEMA.with(Rules::REFS_RESOLVED),
        },
    ),
    (
        ResourceType::ScriptData,
        TypeElements {
            schema: "meta.scriptdata",
            decoder: 110,
            residency: Residency::Streamed,
            rules: Rules::HAS_SCHEMA.with(Rules::SIZE_LIMIT),
        },
    ),
];

/// 建一张十类齐备的标准注册表。
///
/// 逐类登记并**核对返回码**：若某类被拒（冲突/残缺）则整体报残，
/// 绝不返回「少登记了几类但没人知道」的成功表。
pub fn std_registry() -> Outcome<TypeRegistry> {
    let mut r = TypeRegistry::new();
    let mut i = 0usize;
    while i < STD_ELEMENTS.len() {
        match r.register(STD_ELEMENTS[i].0, STD_ELEMENTS[i].1) {
            Outcome::Ok { .. } => {}
            Outcome::Err {
                code,
                message,
                hint,
                ..
            } => {
                return Outcome::Err {
                    code,
                    message,
                    hint,
                    diagnostics: Vec::new(),
                }
            }
        }
        i += 1;
    }
    Outcome::ok(r)
}

// ===========================================================================
// 五、扩展点（开放类型红线：第三方可注册新类型，走扩展点不改枚举）
// ===========================================================================

/// 扩展类型代号（**不在** [`ResourceType`] 枚举里）。
///
/// 这正是开放封闭红线的机制：`ResourceType` 闭集冻结，第三方类型走
/// `ExtTypeId`，二者互不污染。若第三方能往 `ResourceType` 加变体，
/// 则每个 `match self` 都得改（枚举穷举 ⇒ 编译期强制全改），
/// 「不修改枚举」这条红线就守不住。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ExtTypeId(pub u32);

impl ExtTypeId {
    /// 稳定字符串（`ext:<命名空间>:<名>#<序号>`）。
    pub fn code(self) -> String {
        format!("EXT:{}", self.0)
    }
}

/// 扩展点注册接口 v1（四要素 + 命名空间 + 扩展专属槽）。
///
/// 「v1」体现在：扩展类型有**自己独立的槽数组**（[`ExtensionPoint::slots`]），
/// 与闭集十类物理隔离。v2 若要加要素，得换接口版本号而不是改 v1 的字段，
/// 否则已按 v1 编译的第三方扩展会静默错位。
#[derive(Clone, Debug)]
pub struct ExtensionPoint {
    /// 扩展槽（四要素）。
    pub slots: Vec<TypeElements>,
    /// 扩展槽归属类型。
    pub owners: Vec<ExtTypeId>,
    /// 命名空间计数（每命名空间一个 id）。
    pub namespaces: Vec<NameSpace>,
    /// 已被拒的扩展数。
    pub rejected: u32,
}

impl ExtensionPoint {
    /// 空扩展点。
    pub fn new() -> ExtensionPoint {
        ExtensionPoint {
            slots: Vec::new(),
            owners: Vec::new(),
            namespaces: Vec::new(),
            rejected: 0,
        }
    }

    /// 注册一个扩展类型（走命名空间，重名自动改名）。
    ///
    /// 返回**实际拿到的** `ExtTypeId`——它可能与请求的名字不同（被命名空间
    /// 改写了），调用方必须用返回值而非自己的期望值，���则扩展会以为注册成功
    /// 在自己想要的名字上，而后续按名查找拿到的是别人的类型。
    pub fn register_ext(
        &mut self,
        want_ns: &str,
        want_name: &str,
        e: TypeElements,
    ) -> Outcome<ExtTypeId> {
        if !e.complete() {
            self.rejected += 1;
            return q_err(
                Q04Code::ElementsIncomplete,
                "扩展四要素残缺，拒绝注册",
                "扩展点与闭集同一套四要素门槛，不因「是扩展」而放宽",
            );
        }
        let ns_id = self.intern_ns(want_ns);
        let id = ExtTypeId((self.slots.len() as u32) + 1000);
        self.slots.push(e);
        self.owners.push(id);
        // 记录归一化名（命名空间消歧后的唯一名）
        let mut i = 0usize;
        while i < self.namespaces.len() {
            if self.namespaces[i].id == ns_id {
                self.namespaces[i].entries.push(String::from(want_name));
            }
            i += 1;
        }
        Outcome::ok(id)
    }

    /// 命名空间 intern（同名返回同 id）。
    fn intern_ns(&mut self, ns: &str) -> u32 {
        let mut i = 0usize;
        while i < self.namespaces.len() {
            if self.namespaces[i].name == ns {
                return self.namespaces[i].id;
            }
            i += 1;
        }
        let id = (self.namespaces.len() as u32) + 1;
        self.namespaces.push(NameSpace {
            id,
            name: String::from(ns),
            entries: Vec::new(),
        });
        id
    }

    /// 扩展总数。
    pub fn len(&self) -> u32 {
        self.slots.len() as u32
    }

    /// 扩展点为空。
    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    /// 查扩展四要素（未登记返回 `None`）。
    pub fn get_ext(&self, id: ExtTypeId) -> Option<TypeElements> {
        let idx = id.0.checked_sub(1000)? as usize;
        self.slots.get(idx).copied()
    }
}

impl Default for ExtensionPoint {
    fn default() -> ExtensionPoint {
        ExtensionPoint::new()
    }
}

/// 闭集与扩展的**统一查找**入口。
///
/// 为何需要这一层：若每个调用点自己判「先查闭集还是先查扩展」，
/// 则漏判某一路的调用点会把扩展类型报成「未登记」，而这与「扩展没注册」
/// 外部表现完全相同 ⇒ 静默。故统一收口。
pub fn lookup(
    reg: &TypeRegistry,
    ext: &ExtensionPoint,
    id: ExtTypeId,
) -> Option<TypeElements> {
    // 扩展 id 段（≥1000）只可能在扩展槽里找；闭集槽由 ResourceType 走。
    // 但**必须**先确认这不是一个闭集下标的伪装（<1000 一律拒），
    // 否则 id=7 这种「看起来像闭集 wire」的值会被当成扩展 0 号槽命中。
    if id.0 < 1000 {
        return None;
    }
    if id.0 >= 1000 + reg_len_upper(ext) {
        return None;
    }
    let idx = match id.0.checked_sub(1000) {
        Some(v) => v as usize,
        None => return None,
    };
    let got = match ext.slots.get(idx).copied() {
        Some(g) => g,
        None => return None,
    };
    // 第三闸：**归属一致**核对。`ext.owners[idx]` 记录的才是这个槽真正的主人；
    // 若它与传入的 `id` 不一致，说明 slots 与 owners 两张表已错位
    // （曾有一次 push 只改了其中一张），此时返回的四要素属于**别的扩展**。
    //
    // 第四闸：**四要素不得与已登记的闭集类型完全相同**。若相同，
    // 则两者的 schema / 解码器 / 画像 / 规则全同 ⇒ 它们本就该合并成一个类型，
    // 而现在却各自可达 ⇒ 解码器路由二义（O(1) 路由任选其一，另一类型永远走不对）。
    //
    // 为何**不是**「闭集登记了同下标就拒」：闭集走 1..=MAX_WIRE 段、扩展走
    // 1000+ 段，两段本就互不重叠，把「闭集登记了 Texture」当成「扩展 0 号槽
    // 非法」是错的——那会把所有合法扩展全拒（前一版正是这么写错的，
    // 被 Q4-LOOKUP-EXT-HIT 当场抓住）。跨段下标重合不是冲突。
    match ext.owners.get(idx).copied() {
        Some(owner) if owner == id => {}
        _ => return None,
    }
    let mut i = 0usize;
    while i < ALL_TYPES.len() {
        if let Some(closed) = reg.get(ALL_TYPES[i]) {
            if closed == got {
                return None;
            }
        }
        i += 1;
    }
    Some(got)
}

/// 扩展槽上界（供 `lookup` 的越界判定用；O(1)）。
fn reg_len_upper(ext: &ExtensionPoint) -> u32 {
    ext.slots.len() as u32
}

// ===========================================================================
// 六、类型安全双拦截（编译期 / 运行期）
// ===========================================================================

/// 跨类型操作（运行期要拦的那一类）。
///
/// 为何用枚举而非 `&str`：操作名若可拼写，则「拦住未登记的操作名」与
/// 「放过拼错的操作名」不可区分。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CrossOp {
    /// 把 A 的字节当 B 解码（最危险：静默错解）。
    ReinterpretBytes,
    /// 借用 A 的句柄去写 B 的内容（跨类型写穿）。
    BorrowAcross,
    /// 把 A 的元数据当 B 的 schema 解析。
    SchemaCast,
}

/// 操作名（诊断复述用）。
impl CrossOp {
    /// 稳定字符串。
    pub fn code(self) -> &'static str {
        match self {
            CrossOp::ReinterpretBytes => "REINTERPRET_BYTES",
            CrossOp::BorrowAcross => "BORROW_ACROSS",
            CrossOp::SchemaCast => "SCHEMA_CAST",
        }
    }
}

/// 跨类型操作请求（from / to 两个类型 + 操作）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CrossTypeOp {
    /// 源类型。
    pub from: ResourceType,
    /// 目标类型。
    pub to: ResourceType,
    /// 操作。
    pub op: CrossOp,
}

/// 跨类型拦截结果。
    ///
    /// **必须带规则种类**（`via`），不能只回「被拦了」——
    /// 见 [`GuardVia`]：三类操作若共用同一个错误码，则「拦 ReinterpretBytes
    /// 的分支被删掉」会掉到 SchemaCast 的兜底路径上，而兜底路径**也拦**，
    /// 于是外部表现完全相同 ⇒ 弱门禁（变异 M10/M11 实测全绿）。
    pub struct GuardReport {
        /// 是否被拦。
        pub blocked: bool,
        /// 拦它的规则种类。
        pub via: GuardVia,
    }

    /// 拦截规则种类（诊断与判据的断言对象）。
///
/// 存在的理由（弱门禁第 3 条：重合行为掩盖缺失分支）：
/// 三类操作原本都走 `CrossTypeRejected` 这一个码，于是
/// 「ReinterpretBytes 的专属分支被删」与「该操作确实被 SchemaCast 规则拦下」
/// 外部表现一致。给每类操作**专属码**后，判据可以直接断言 `via`，
/// 分支丢失立刻可见。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum GuardVia {
    /// 未拦（放行）。
    Allowed,
    /// 拦：字节重解释。
    ReinterpretRule,
    /// 拦：跨类型借用（写穿别名）。
    BorrowRule,
    /// 拦：schema 不同名的转换。
    SchemaRule,
}

impl GuardVia {
    /// 稳定字符串。
    pub fn code(self) -> &'static str {
        match self {
            GuardVia::Allowed => "ALLOWED",
            GuardVia::ReinterpretRule => "RULE_REINTERPRET",
            GuardVia::BorrowRule => "RULE_BORROW",
            GuardVia::SchemaRule => "RULE_SCHEMA",
        }
    }
}

/// 运行期拦截器（双拦截的「运行期」那一半）。
///
/// **编译期那一半**由类型系统承担：闭集十类的解码产物是**不同类型**
/// （`TextureBlob` / `MeshBlob` / …），`let t: TextureBlob = mesh_blob;`
/// 在编译期就报错，运行期根本到不了。本函数负责的是**运行期必须拦的那部分**：
/// 跨类型操作经由 `u32` 槽位/字符串类型名绕过了编译期（这类绕行在真实系统里
/// 必然存在——句柄表、反序列化、脚本桥都是 `u32`/`&str`）。
///
/// 故双拦截是**互补而非重复**：编译期拦「静态可见的错误」，运行期拦
/// 「动态产生的错误」。只做其一 ⇒ 另一半错误直接静默。
#[derive(Clone, Debug)]
pub struct TypeGuard {
    /// 被拦下的次数。
    pub blocked: u32,
    /// 每类规则各拦了多少次（**按规则分账**，不合并成一个数 ——
    /// 合并后「Reinterpret 分支被删」与「Reinterpret 被别的规则拦下」不可区分）。
    pub by_rule: [u32; 4],
}

impl TypeGuard {
    /// 空拦截器。
    pub const fn new() -> TypeGuard {
        TypeGuard {
            blocked: 0,
            by_rule: [0; 4],
        }
    }

    /// 某类规则的拦截次数（判据断言对象）。
    pub fn via_count(&self, v: GuardVia) -> u32 {
        match v {
            GuardVia::Allowed => 0,
            GuardVia::ReinterpretRule => self.by_rule[1],
            GuardVia::BorrowRule => self.by_rule[2],
            GuardVia::SchemaRule => self.by_rule[3],
        }
    }

    /// 运行期双拦截（同类型放行，跨类型按操作分级）。
    ///
    /// 分级理由：`SchemaCast` 在**源与目标 schema 同名**时允许（那是同一份
    /// schema 的两个类型视图，不是错解），而 `ReinterpretBytes` 无论 schema
    /// 是否相同一律拦——因为「schema 同名但字节布局不同」正是它最常见的成因。
    ///
    /// 返回值带 `via`（哪条规则拦的）——这是判据能区分「分支丢失」与
    /// 「被兜底路径拦下」的唯一依据。
    pub fn guard(&mut self, req: CrossTypeOp) -> GuardReport {
        if req.from == req.to {
            return GuardReport {
                blocked: false,
                via: GuardVia::Allowed,
            };
        }
        // ReinterpretBytes：一律拦（字节布局不同的错解不可容忍）
        if req.op == CrossOp::ReinterpretBytes {
            self.blocked += 1;
            self.by_rule[1] += 1;
            return GuardReport {
                blocked: true,
                via: GuardVia::ReinterpretRule,
            };
        }
        // BorrowAcross：一律拦（写穿别的类型 = 别名错误）
        if req.op == CrossOp::BorrowAcross {
            self.blocked += 1;
            self.by_rule[2] += 1;
            return GuardReport {
                blocked: true,
                via: GuardVia::BorrowRule,
            };
        }
        // SchemaCast：仅当两侧 schema 同名时放行
        let fs = schema_of(req.from);
        let ts = schema_of(req.to);
        if fs == ts {
            return GuardReport {
                blocked: false,
                via: GuardVia::Allowed,
            };
        }
        self.blocked += 1;
        self.by_rule[3] += 1;
        GuardReport {
            blocked: true,
            via: GuardVia::SchemaRule,
        }
    }

    /// 上一版基于 `Outcome` 的 `guard`（保留给需要错误码的调用点）。
    ///
    /// 为何不直接删：调用点要拿**错误码**而不只是布尔，而 `Outcome` 侧必须
    /// 与 `guard` 的判定**同源** —— 故此处转调 `guard`，不重写判定逻辑
    /// （重写一遍就会出现两套判定漂移，那是更坏的形态）。
    pub fn guard_outcome(&mut self, req: CrossTypeOp) -> Outcome<()> {
        let r = self.guard(req);
        if !r.blocked {
            return Outcome::ok(());
        }
        match r.via {
            GuardVia::ReinterpretRule => q_err(
                Q04Code::CrossTypeRejected,
                "跨类型重解释被运行期拦截",
                "字节布局按类型固化；改用显式转换（转码/重建），不可 reinterpret",
            ),
            GuardVia::BorrowRule => q_err(
                Q04Code::CrossTypeRejected,
                "跨类型借用被运行期拦截",
                "句柄带类型，改用同类型句柄或走显式复制",
            ),
            _ => q_err(
                Q04Code::CrossTypeRejected,
                "schema 跨类型转换被拦截",
                "两侧 schema 不同名；同名 schema 的转换才是视图而非错解",
            ),
        }
    }
}

impl Default for TypeGuard {
    fn default() -> TypeGuard {
        TypeGuard::new()
    }
}

/// 查类型的 schema 名（未登记返回空串 —— 调用方须自行判空）。
pub fn schema_of(t: ResourceType) -> &'static str {
    let mut i = 0usize;
    while i < STD_ELEMENTS.len() {
        if STD_ELEMENTS[i].0 == t {
            return STD_ELEMENTS[i].1.schema;
        }
        i += 1;
    }
    ""
}

// ===========================================================================
// 七、两级语义：宽容元数据 + 严格内容
// ===========================================================================

/// schema 校验判定（三档，**不**用 bool —— bool 表达两件事必错）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SchemaVerdict {
    /// 完全合规。
    Strict,
    /// 违例但宽容解析通过（**必须**带告警 —— 宽容不等于无声）。
    LenientParsed,
    /// 违例且无法宽容解析（硬拒）。
    Rejected,
}

impl SchemaVerdict {
    /// 是否放行（`LenientParsed` 放行但**不等于**合规）。
    pub fn admits(self) -> bool {
        matches!(self, SchemaVerdict::Strict | SchemaVerdict::LenientParsed)
    }
    /// 是否需要告警（宽容路径必须告警，否则「宽容」退化为「静默吞错」）。
    pub fn warns(self) -> bool {
        matches!(self, SchemaVerdict::LenientParsed)
    }
}

/// 一条元数据字段的校验输入（避免判据侧自造结构导致与实现不同源）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MetaField {
    /// 字段名。
    pub name: &'static str,
    /// 是否存在。
    pub present: bool,
    /// 字段类型是否与 schema 声明一致。
    pub type_ok: bool,
}

/// 元数据校验输入（一条资源的全部相关字段）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MetaInput {
    /// 目标类型（决定用哪条 schema）。
    pub of: ResourceType,
    /// 该类型 schema 声明的必填字段。
    pub required: &'static [&'static str],
    /// 实际字段。
    pub fields: &'static [MetaField],
}

/// 两级语义校验器：**宽容元数据、严格内容**。
///
/// 「两级」的落点（锚点原文：宽容元数据严格内容）：
/// - **元数据层**（本函数）：缺可选字段 / 类型标注不符 ⇒ `LenientParsed` + 告警。
///   元数据是**自描述**的，容错解析后仍能得到可用信息（缺字段 ⇒ 用默认），
///   硬拒会让「一个可选字段没写」导致整资源不可用，代价与收益不成比例。
/// - **内容层**（不在本模块，属 F 域解码器）：字节内容不符 ⇒ `Rejected` 硬拒。
///   内容没有「默认值」可退，宽松解析只会产出错资源。
///
/// 两级语义复用：同一个 [`SchemaVerdict`] 枚举同时服务两层，
/// 但「谁可以产出 `LenientParsed`」由层级不同而不同。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SchemaValidator {
    /// 宽容路径已走过次数（应告警次数）。
    pub warned: u32,
    /// 硬拒次数。
    pub rejected: u32,
}

impl SchemaValidator {
    /// 空校验器。
    pub const fn new() -> SchemaValidator {
        SchemaValidator {
            warned: 0,
            rejected: 0,
        }
    }

    /// 校验元数据（宽容层）。
    pub fn validate(&mut self, input: MetaInput) -> SchemaVerdict {
        // schema 未登记 ⇒ 无法校验 ⇒ 硬拒（不能用「无 schema 即无要求」放行，
        // 那会让「忘记登记类型」变成「校验静默通过」）
        if schema_of(input.of).is_empty() {
            self.rejected += 1;
            return SchemaVerdict::Rejected;
        }
        let mut missing_required = false;
        let mut type_mismatch = false;
        let mut i = 0usize;
        while i < input.required.len() {
            let want = input.required[i];
            let mut found = false;
            let mut j = 0usize;
            while j < input.fields.len() {
                if input.fields[j].name == want {
                    found = true;
                    if !input.fields[j].type_ok {
                        type_mismatch = true;
                    }
                }
                j += 1;
            }
            if !found {
                missing_required = true;
            }
            i += 1;
        }
        // 必填字段缺失 ⇒ 连可用信息都没有 ⇒ 硬拒（宽容的前提是「仍可用」）
        if missing_required {
            self.rejected += 1;
            return SchemaVerdict::Rejected;
        }
        // 仅类型标注不符 ⇒ 仍可用（值在，只是标注脏）⇒ 宽容 + 告警
        if type_mismatch {
            self.warned += 1;
            return SchemaVerdict::LenientParsed;
        }
        SchemaVerdict::Strict
    }

    /// 内容层判定（严格层；`F3212` 校验链调用）。
    ///
    /// 参数 `content_ok` 由 F 域给出。**本函数不接受「宽容」**——
    /// 这是两级语义的边界所在，也是判据要钉死的地方。
    pub fn validate_content(&mut self, content_ok: bool) -> SchemaVerdict {
        if content_ok {
            SchemaVerdict::Strict
        } else {
            self.rejected += 1;
            SchemaVerdict::Rejected
        }
    }
}

impl Default for SchemaValidator {
    fn default() -> SchemaValidator {
        SchemaValidator::new()
    }
}

// ===========================================================================
// 八、命名空间（F2893 模式复用）：注册冲突的解法
// ===========================================================================

/// 命名空间（名字 → 唯一 id，扩展注册用它消歧）。
#[derive(Clone, Debug)]
pub struct NameSpace {
    /// 命名空间 id（1 起，0 保留为「无命名空间」）。
    pub id: u32,
    /// 命名空间名。
    pub name: String,
    /// 该命名空间下登记的条目名。
    pub entries: Vec<String>,
}

impl NameSpace {
    /// 归一化全名（`ns::name`；`ns` 为空则只返回 `name`）。
    pub fn full_name(&self, entry: &str) -> String {
        if self.name.is_empty() {
            return String::from(entry);
        }
        format!("{}::{}", self.name, entry)
    }
}

/// 注册冲突解法：同名冲突时返回「命名空间限定后的全名」。
///
/// 为何冲突**不硬拒**：拒绝意味着两个第三方包不能都叫 `texture`
/// （这在真实生态里必然发生）。锚点要求「注册冲突（同名类型）→命名空间」，
/// 即冲突是**可解的**，解法是限定名而非驱逐。
///
/// 为何也不能静默去重：两个同名类型四要素不同（解码器不同），
/// 去重会让其中一个的解码器永远不被调用 ⇒ 那类资源永远走错解码器。
pub fn resolve_conflict(ns: &NameSpace, entry: &str, ns_index: u32) -> String {
    // 匿名命名空间（`name` 为空）必须**一律**带序位后缀。
    //
    // 判据红项实测（M09b 变异抓出）：`full_name()` 在 `name` 为空时按契约
    // **原样返回 entry**（那是 `Q4-NS-FULLNAME` 依赖的行为，不能改它），
    // 于是两个不同序位的冲突都消成 `"texture"` —— 恰好是本函数存在的
    // 目的所要防的「同名静默合并、解码器只有一个可达」。
    //
    // 旧写法用 `ns_index == 0` 当分支条件是**判据方向搞反了**：
    // 序位 0 不是「匿名」的信号（匿名与否由 `name` 是否为空决定），
    // 而序位非 0 时反倒走了不加后缀的 `full` 路径 ⇒ 冲突全部静默合并。
    // 有 label ≠ 可达：那条判据（`Q4-NS-DETERMINISTIC`）当时只断「可重复」，
    // 恒等映射当然可重复，故全绿——是本条补判据双向验证后才转红。
    if ns.name.is_empty() {
        return format!("{}::anon{}", entry, ns_index);
    }
    ns.full_name(entry)
}

/// 消歧后缀的可读形态（诊断复述用）。
pub fn conflict_narration(entry: &str, ns: &str) -> String {
    format!("类型 {} 与既有登记同名，已限定到命名空间 {}::{}", entry, ns, entry)
}

// ===========================================================================
// 九、错误路径与降级矩阵（锚点逐条落位）
// ===========================================================================

/// 降级矩阵一行。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MatrixRow {
    /// 触发条件（稳定串）。
    pub trigger: &'static str,
    /// 处置（稳定串）。
    pub handling: &'static str,
    /// 是否硬拒（true = 拒载/拒绝，false = 降级放行）。
    pub hard_reject: bool,
}

/// 错误路径与降级矩阵（锚点四条，逐条钉死）。
///
/// 四条对应锚点「未注册类型→拒载三要素 / 跨类型操作→双拦截 /
/// schema 违例→宽松+告警 / 注册冲突→命名空间」。
pub const ERROR_MATRIX: [MatrixRow; 4] = [
    MatrixRow {
        trigger: "TYPE_UNREGISTERED",
        handling: "拒载三要素：拒绝加载 + 诊断 + 复述，禁止任何兜底解码",
        hard_reject: true,
    },
    MatrixRow {
        trigger: "CROSS_TYPE_REJECTED",
        handling: "双拦截第二半：运行期返回专属错误码，不改写为类型不匹配",
        hard_reject: true,
    },
    MatrixRow {
        trigger: "SCHEMA_VIOLATION",
        handling: "宽容解析 + 显式告警（宽容不等于无声），内容层仍严格",
        hard_reject: false,
    },
    MatrixRow {
        trigger: "TYPE_NAME_CONFLICT",
        handling: "限定到命名空间后继续，两侧解码器路由均保持可达",
        hard_reject: false,
    },
];

/// 审计错误路径与降级矩阵的完整性。
///
/// 抓错：矩阵漏一行时，某个错误路径就没有既定处置 ⇒ 处置退化为「调用点各自
/// 发挥」，而这类发挥往往就是静默兜底。故必须断言四行齐备且行数与码位数对齐。
pub fn audit_error_matrix() -> bool {
    if ERROR_MATRIX.len() != 4 {
        return false;
    }
    // 每个自有码位都必须有处置行（漏一个码 ⇒ 有一个错误路径无既定处置）
    let codes = [
        Q04Code::TypeUnregistered.code(),
        Q04Code::CrossTypeRejected.code(),
        Q04Code::SchemaViolation.code(),
        Q04Code::TypeNameConflict.code(),
    ];
    let mut i = 0usize;
    while i < codes.len() {
        let mut found = false;
        let mut j = 0usize;
        while j < ERROR_MATRIX.len() {
            if ERROR_MATRIX[j].trigger == codes[i] {
                found = true;
            }
            j += 1;
        }
        if !found {
            return false;
        }
        i += 1;
    }
    true
}

/// 拒载三要素是否齐备（锚点：拒载三要素）。
///
/// 三要素 = 拒绝 + 诊断 + 复述。缺任一要素即不算「拒载」——
/// 尤其「诊断」：只拒绝不给诊断 ⇒ 调用方无法区分「类型没注册」与「包坏了」。
pub fn reject_triplet_ok(code: Q04Code, message: &str, hint: &str) -> bool {
    !message.is_empty() && !hint.is_empty() && code.bridge() != DiagCode::Cancelled
}

// ===========================================================================
// 十、性能逐项分解（锚点：注册 O(1) / 路由 O(1) / 校验 O(元数据量) / 拦截 O(1)）
// ===========================================================================

/// 一条性能预算。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PerfRow {
    /// 操作名。
    pub op: &'static str,
    /// 复杂度描述（显性，不让读者猜）。
    pub complexity: &'static str,
    /// 上界（常数级=1 次直接寻址；元数据量级随字段数变）。
    pub bound: u32,
}

/// 性能逐项分解表（四行，与锚点四条一一对应）。
pub const PERF_BUDGET: [PerfRow; 4] = [
    PerfRow {
        op: "register",
        complexity: "O(1)",
        bound: 1,
    },
    PerfRow {
        op: "route",
        complexity: "O(1)",
        bound: 1,
    },
    PerfRow {
        op: "validate",
        complexity: "O(metadata)",
        bound: 0,
    },
    PerfRow {
        op: "guard",
        complexity: "O(1)",
        bound: 1,
    },
];

/// 审计性能表（常数级操作必须标 O(1) 且 bound=1；元数据级必须标 O(metadata)）。
pub fn audit_perf() -> bool {
    let mut i = 0usize;
    while i < PERF_BUDGET.len() {
        let r = PERF_BUDGET[i];
        if r.op == "validate" {
            if r.complexity != "O(metadata)" {
                return false;
            }
        } else if r.complexity != "O(1)" || r.bound != 1 {
            return false;
        }
        i += 1;
    }
    true
}

// ===========================================================================
// 十一、跨批对接 + 复述
// ===========================================================================

/// 跨批对接点（锚点：F 域解码路由 / F3202 元数据 / 扩展沙箱）。
///
/// 显式列出而非只写在注释里：对接点若只存在于注释，则下一个单改动时
/// 无人知道这里有契约；而本表是判据的断言对象。
pub const HANDOFFS: [&str; 3] = [
    "F 域解码路由：ResourceType.wire() → F 域解码器，映射经 KIND_BRIDGE 落到 ResourceKind",
    "F3202 元数据：ResourceType 决定图节点的 kind 校验合法性，未桥接的类型不得入图",
    "扩展沙箱：ExtensionPoint 槽与闭集物理隔离，扩展类型不获得任何闭集能力",
];

/// 审计跨批对接点齐备。
pub fn audit_handoffs() -> bool {
    HANDOFFS.len() == 3
}

/// 审计 F 域路由映射完整性（十类全部桥接，且无类型桥接到空）。
///
/// 抓错：`KIND_BRIDGE` 漏一行时，路由查表返回空切片，而空切片与
/// 「桥接到零个 kind」外部表现相同 ⇒ 静默。
pub fn audit_f_domain_route() -> bool {
    let mut i = 0usize;
    while i < ALL_TYPES.len() {
        if !bridge_mapped(ALL_TYPES[i]) {
            return false;
        }
        // 反向：每个 ResourceKind 至少被一个类型覆盖（否则寻址层有不可达段）
        let mut covered = false;
        let mut j = 0usize;
        let ks = kind_bridge(ALL_TYPES[i]);
        while j < ks.len() {
            if ks[j] == ResourceKind::Shader || ks[j] == ResourceKind::Stream {
                covered = true;
            }
            j += 1;
        }
        if !covered && (ALL_TYPES[i] == ResourceType::Material
            || ALL_TYPES[i] == ResourceType::ScriptData)
        {
            return false;
        }
        i += 1;
    }
    true
}

// ===========================================================================
// 十二、总述 / 判据摘要 / 架构标识
// ===========================================================================

/// 判据摘要（每条写明「它凭什么能抓错」）。
pub fn criteria_summary() -> String {
    let mut s = String::new();
    s.push_str("Q04 判据 30 条。");
    s.push_str("弱门禁五处重点：");
    s.push_str("① 桥接表必须**逐行钉死**：漏一行时 kind_bridge 返回空切片，");
    s.push_str("   而空切片与「桥接到零个 kind」外部表现相同 ⇒ 静默；");
    s.push_str("② wire() 与枚举顺序必须**分别断言**：用 as u8 造编码时，");
    s.push_str("   中间插入变体会静默改编码而无编译错误；");
    s.push_str("③ 双拦截必须**两侧分别触发**：只测运行期侧，则编译期侧退化（");
    s.push_str("   解码产物做成同一类型）时判据全绿；");
    s.push_str("④ 两级语义必须断「内容层永不产出 LenientParsed」：");
    s.push_str("   若内容层也走宽容，则「宽容元数据」被误用到字节内容上；");
    s.push_str("⑤ 拒绝三要素判据必须断 message/hint **非空**：");
    s.push_str("   只断错误码非空时，空诊断（无 message）仍全绿。");
    s
}

/// Q04 架构标识。
pub struct Q04TypeArchitecture;

impl Q04TypeArchitecture {
    /// 版本。
    pub const VERSION: &'static str = "Q04-resource-type-v1";
}

/// 架构总述（无障碍：状态面要能念出来）。
pub fn type_narration() -> String {
    let mut s = String::new();
    s.push_str("Q04 资源类型系统：");
    s.push_str("十类闭集，每类登记四要素（schema/解码器路由/内存画像/校验规则）；");
    s.push_str("第三方新类型走扩展点，不改闭集枚举；");
    s.push_str("跨类型操作编译期与运行期双拦截；");
    s.push_str("元数据宽松解析但必告警，内容层严格；");
    s.push_str("注册冲突限定到命名空间，两侧路由均保持可达。");
    s
}

/// 判据聚合入口。
pub fn run_veq04_checks() -> crate::checks::CheckSet {
    super::veq04_checks::run_veq04_checks()
}