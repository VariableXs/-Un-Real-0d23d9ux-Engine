//! VE-F2806 · 简写属性展开（shorthand→longhand 展开器）
//!
//! 承接 VE-F2805（声明解析与属性表注册）。F2805 把 `margin: 1px 2px` 解析成
//! `ParsedValue::List([Length(1px), Length(2px)])` 并**明确不展开**（展开归本单），
//! 本单把一条简写声明展开成 1~4 条 longhand 声明。
//!
//! # 一、职责与边界
//!
//! 职责定位（锚点原文）：**简写属性展开——shorthand→longhand 展开器——
//! 子集内全部简写逐条实现**。
//!
//! 边界声明（**不越权**，逐条写明「归谁」）：
//! - **级联与优先级裁决不归本单**——本单只做「一条声明 ⇒ N 条 longhand」的
//!   纯函数展开，不看文档顺序、不算权重、不做 `!important` 的胜负判定。
//!   那是 F2807（样式规则存储）与级联层的活。
//! - **longhand 的值重新解析不归本单**——F2805 已把每个分量解析成
//!   `ParsedValue`，本单只做**分量到边角的分配**，不重新走一遍解析器。
//! - **展开结果的最终布局不归本单**——`margin-top` 具体怎么影响盒模型，
//!   归 F2811/F2813 的计算层。
//! - **`border-radius` 的 `/` 斜杠双轴语法归本单**（它是简写语法的一部分），
//!   但椭圆→圆的曲线求解不归本单（本单只保留两轴半径两个分量）。
//! - **`font` / `background` 等多族混合简写不归本单**——它们不在 F2803 的
//!   44 条子集内（子集只有 `font-size`/`font-family`/`font-weight`/`font-style`
//!   四个 font 分项，没有 `font` 本身），见 [`SHORTHAND_TABLE`] 的登记。
//!
//! # 二、核心逻辑：1~4 分量的边角分配
//!
//! CSS 的「1~4 值 = 上右下左」是**幂等可递归**的分配规则（CSSOM 定义的
//! `assign` 语义）：
//!
//! | 分量数 | 上 | 右 | 下 | 左 |
//! |---|---|---|---|---|
//! | 1 | a | a | a | a |
//! | 2 | a | b | a | b |
//! | 3 | a | b | c | b |
//! | 4 | a | b | c | d |
//!
//! 规范等价定义（**本单实现的就是它**）：
//! `top = v[0]`；`right = v[1]` 若存在否则 `v[0]`；`bottom = v[2]` 若存在否则
//! `top`；`left = v[3]` 若存在否则（`v[1]` 若存在否则 `v[0]`）。
//!
//! 为什么用「逐边 fallback 到已有分量」而不是「按 arity 查表」：查表法在
//! arity=3 时要特判（CSS 的 3 值不是 `a b a c` 而是 `a b c b`——**第 2 个分量
//! 同时给左右**），特判写漏一处就是静默错值；而 fallback 规则是**一条公式**
//! 对 1/2/3/4 全部成立，无特判。判据侧据此独立重算期望值。
//!
//! # 三、border-radius 的双轴（`/` 分隔）
//!
//! `border-radius: <h1..h4> / <v1..v4>`。纵向分量缺省时**逐个**取对应横向
//! 分量（不是「整体取第一个」）——`border-radius: 1px 2px` 的两角横向是
//! 1px/2px，纵向也分别是 1px/2px，不是都 1px。这条最容易被实现成
//! 「纵向整体复制第一个横向分量」，判据专设一项钉死。
//!
//! # 四、数据结构与规格表
//!
//! [`SHORTHAND_TABLE`]：子集内全部简写的规格表，**逐条公开**（`pub const`，
//! 外部可机检）。每条规格给：
//! - 简写名（必须能在 F2805 的注册表里查到，`id_of` 返回 `Some`）；
//! - 四条 longhand 的**按序**名字（`Longhand` 静态表下标，四元组齐）；
//! - 该简写的分量数上界（`max_components`）；
//! - 该简写是否走双轴（`slash_axes`，只有 `border-radius` 为真）。
//!
//! **编译期完整性校验五闸**（`const _: () = assert!(…)`，写错即编译不过，
//! 不是运行期才发现）：
//! 1. 规格条数 == [`SHORTHAND_COUNT`]（数组长度即断言）；
//! 2. 每条的四个 longhand 名**都非空**（空名会让展开产出无名声明）；
//! 3. 每条的四个 longhand 名**两两不同**（重名会让同一个 longhand 被写两次，
//!    后写覆盖先写 ⇒ 静默丢值）；
//! 4. 每条的简写名**逐字节互不相同**（同名两条 ⇒ 谁生效取决于表序）；
//! 5. 每条 `max_components` ∈ [1,4]（越界即分配公式失效）；
//! 再加两条跨表闸：
//! 6. 每条简写名**能在 F2805 注册表查到**（查不到 ⇒ 展开器服务一个不存在的属性）；
//! 7. 每条简写名在 F2805 注册表里的 `ValueKind` **与规格表登记的
//!    `registry_kind` 逐位一致**（规格表登记 F2805 的真实归类：
//!    margin/padding/border-width/border-radius 为 `List`、border-style 为
//!    `LineStyle`、border-color 为 `Color`——不是一律 `List`，因为
//!    F2805 的 `kind_for` 按取值语法归类别。这条闸拦住「规格表与注册表
//!    漂移」：F2805 改了归类而本单没跟，审计即红）。
//!
//! # 五、错误路径与降级矩阵
//!
//! - **非法输入 → 校验拒绝三要素**：分量数为 0、超过 `max_components`、
//!   双轴简写给了 `/` 但两侧都空、简写名不在表内 —— 各自有**专属错误码**
//!   （[`E_SHORTHAND_ARITY`] / [`E_SHORTHAND_UNKNOWN`] /
//!   [`E_SHORTHAND_AXES_EMPTY`]），拒绝时给 `next`（下一步该做什么）与
//!   `who`（责任方），**零静默**。
//! - **边界越界 → 钳制 + 告警**：展开条数有上界 [`MAX_LONGHAND_PER_DECL`]，
//!   超了**不静默截断**——记账本 [`ClampLog`] 并立案。
//! - **异常检出 → 立案流转**：任何一次拒绝都经 [`CaseLedger::open_case`]
//!   立案（现象/影响/定位/处置四要素齐），可查回。
//!
//! # 六、性能分解
//!
//! - 分配本身 **O(1)**：分量数上界 4（[`MAX_COMPONENTS`] 是编译期常量），
//!   边角数固定 4，与输入规模无关。
//! - `longhand` 名查表 **O(1)**：四元组是静态数组，按下标取，不做名查找。
//! - 简写表本身 **O(1)**：[`SHORTHAND_COUNT`] 条，线性扫描上界是
//!   [`SHORTHAND_COUNT`]（编译期常量，见 [`shorthand_count`]），与属性总数无关。
//! - 预算联动**次序单源**：展开顺序恒为「上右下左」，唯一真值在
//!   [`EDGE_ORDER`]，下游（层叠、增量重算）据此定序，本单不第二处排序。
//!
//! # 七、跨批对接点
//!
//! - **上游契约接收（哈希对账）**：[`UPSTREAM_CONTRACT`] 钉住 F2805 的
//!   `registry_digest()` 指纹。F2805 的属性表改了而本单没跟着改时，
//!   [`audit_upstream`] 会**报错而非通过**——这是「哈希对账」家族的标准做法。
//! - **下游消费接口（前向声明）**：[`DOWNSTREAM_DECL`] 前向声明本单产出的
//!   longhand 声明供F2807 样式表对象消费；本单**只声明不实现**消费方逻辑。
//! - **跨域衔接对账钩子（复用方义务）**：[`reconcile_longhand`] 让复用方
//!   自查「我拿到的是不是本单的展开产物」，本单提供判据、义务在复用方。
//!
//! # 八、无障碍与隐私
//!
//! [`Longhand::screen_line`] / [`Expansion::screen_text`] 让展开结果能被
//! 读屏念出「哪个 longhand 得到了什么值」。**无隐私面**：本单不触碰任何
//! 作者身份、文档内容、用户数据，只做属性名与数值的重排。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use super::veo01_arch::{CaseLedger, ClampLog, StyleError};
use super::veo05_props::{self, PropertyId, ValueKind};

// ---------------------------------------------------------------------------
// 一、版本与规格锚点
// ---------------------------------------------------------------------------

/// 本单规格版本（**改动规格即改此串**，便于下游对账）。
pub const SHORTHAND_VERSION: &str = "O01-shorthand-v1";

/// 上游契约锚点（F2805 声明解析与属性表注册）。
pub const UPSTREAM_ANCHOR: &str = "css-values-4/CSS Values and Units Level 4";

/// 下游契约锚点（F2807 样式规则存储与样式表对象）。
pub const DOWNSTREAM_ANCHOR: &str = "css-cascade-5/CSS Cascading and Inheritance Level 5";

/// 上游指纹：F2805 注册表的 `registry_digest()`。
///
/// **这不是抄一个常量**——[`audit_upstream`] 在运行期**自己算**一遍
/// F2805 的摘要并与本单记录的基线比对（见 [`UPSTREAM_DIGEST`]）。
/// 这样 F2805 改了属性表，本单在下一次审计时就会发现对不上。
pub const UPSTREAM_DIGEST: &str = "o05-registry-digest-v1";

/// 上游契约接收记录（[`audit_upstream`] 的返回凭据）。
#[derive(Clone, Debug, PartialEq)]
pub struct UpstreamContract {
    /// 上游模块标识。
    pub peer: &'static str,
    /// 锚点。
    pub anchor: &'static str,
    /// 上游注册表摘要（运行期现算）。
    pub digest: String,
    /// 本单记录的基线摘要。
    pub baseline: String,
    /// 是否对账通过。
    pub matched: bool,
    /// 上游属性总数（运行期现算）。
    pub upstream_properties: u32,
}

impl UpstreamContract {
    /// 读屏单行。
    pub fn screen_line(&self) -> String {
        format!(
            "上游 {}｜锚点 {}｜上游属性 {} 条｜摘要 {}｜对账 {}",
            self.peer,
            self.anchor,
            self.upstream_properties,
            if self.matched { "一致" } else { "不一致" },
            if self.matched { "通过" } else { "拒绝" }
        )
    }
}

/// 下游消费接口（前向声明）——**只声明，不实现**。
///
/// 本单产出的 [`Expansion`] 交给 F2807 消费。**声明而不实现**是刻意的：
/// 现在就把F2807 的存储布局写进来，等F2807 落定时必然要改，而那时改的是
/// 本单（别人的单已经收工）。只暴露「下游需要什么」这一面。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DownstreamDecl {
    /// 下游模块标识。
    pub peer: &'static str,
    /// 下游需要的字段名（按序；F2807 按这些字段取）。
    pub fields: &'static [&'static str],
}

impl DownstreamDecl {
    /// 读屏单行。
    pub fn screen_line(&self) -> String {
        format!("下游 {}｜需要字段 {} 项", self.peer, self.fields.len())
    }
}

/// 下游消费接口的正式声明。
pub const DOWNSTREAM_DECL: DownstreamDecl = DownstreamDecl {
    peer: "VE-F2807",
    fields: &["longhand_name", "value", "important", "offset"],
};

// ---------------------------------------------------------------------------
// 二、诊断码（**F2806 独占码段：0x2Cxx**）
// ---------------------------------------------------------------------------
//
// 码段纪律：F2807=0x2Axx、F2808=0x2Bxx 已占；**本单独占 0x2Cxx**。
// 自建码而非复用下游封闭枚举——下游要加变体时不会因为本单的码而编译失败，
// 反过来本单也不必等下游先裁决。

/// 分量数非法（0 个，或超过该简写的上界）。
pub const E_SHORTHAND_ARITY: &str = "E_SHORTHAND_ARITY";

/// 简写名不在规格表内。
pub const E_SHORTHAND_UNKNOWN: &str = "E_SHORTHAND_UNKNOWN";

/// 双轴简写（`border-radius`）的 `/` 两侧都空。
pub const E_SHORTHAND_AXES_EMPTY: &str = "E_SHORTHAND_AXES_EMPTY";

/// 展开条数越界（超 [`MAX_LONGHAND_PER_DECL`]）。
pub const E_LONGHAND_CAP: &str = "E_LONGHAND_CAP";

/// 上游契约对账失败（F2805 注册表已变而本单未跟）。
pub const E_UPSTREAM_DRIFT: &str = "E_UPSTREAM_DRIFT";

/// 上游属性名不可解析（复用方义务未履行）。
pub const E_LONGHAND_NAME: &str = "E_LONGHAND_NAME";

/// 处置建议：分量数不符。
pub const FIX_ARITY: &str =
    "简写值写1~4 个分量（1 个给全边、2 个给上下左右、3 个给上左右下、4 个给上右下左）";

/// 处置建议：简写名不在表内。
pub const FIX_UNKNOWN: &str =
    "该属性名不在本单简写规格表内；非简写属性不需要展开，直接由F2805 解析即可";

/// 处置建议：双轴两侧皆空。
pub const FIX_AXES_EMPTY: &str = "border-radius 的斜杠两侧至少一侧要写横向半径；只写一侧时另一侧逐个等于对应横向值";

/// 处置建议：展开条数越界。
pub const FIX_LONGHAND_CAP: &str =
    "拆分声明或降低单条简写的分量数；一条简写最多展开为 4 条 longhand";

/// 处置建议：上游漂移。
pub const FIX_UPSTREAM: &str =
    "F2805 属性注册表已改动；同步本单规格表的简写名单与 max_components 后重跑审计";

/// 处置建议：longhand 名不可解析。
pub const FIX_LONGHAND_NAME: &str = "longhand 名必须逐字节取自本单 SHORTHAND_TABLE 的四元组，不得自行拼接";

// ---------------------------------------------------------------------------
// 三、边与长写名（**唯一的边序真值**）
// ---------------------------------------------------------------------------

/// 边的枚举（**顺序即展开产出的下标顺序**）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edge {
    /// 上。
    Top,
    /// 右。
    Right,
    /// 下。
    Bottom,
    /// 左。
    Left,
}

/// 边数（固定 4——`border-radius` 也是四边/四角）。
pub const EDGE_COUNT: usize = 4;

/// 边的展开次序（**次序单源**：下游层叠与增量重算据此定序）。
///
/// 恒为上→右→下→左。任何别处出现第二种边序即为漂移。
///
/// **与 [`EDGE_RANKS`] 的对应关系由运行期闸核对**（[`audit_spec_table`]
/// 的闸 8）：`Edge` 是无数据枚举，但要在 `const` 里从秩**造**出枚举需要
/// `const fn` 构造器而 `Edge` 的判别式又不能 const 化，所以这里只能
/// 「枚举数组 + 秩数组」并存、再由运行期闸证明两者逐位一致。
/// 编译期已精确断死秩必须是 0/1/2/3，运行期闸再断枚举的秩与之一致——
/// 两层合起来，错序（`[Top, Bottom, Right, Left]`）无处可藏。
pub const EDGE_ORDER: [Edge; EDGE_COUNT] = [
    Edge::Top,
    Edge::Right,
    Edge::Bottom,
    Edge::Left,
];

impl Edge {
    /// 中文名（读屏用）。
    pub fn zh(self) -> &'static str {
        match self {
            Edge::Top => "上",
            Edge::Right => "右",
            Edge::Bottom => "下",
            Edge::Left => "左",
        }
    }

    /// 由展开下标取边（**越界返回 `None`，绝不静默回绕到上边**）。
    pub fn of_index(i: usize) -> Option<Edge> {
        EDGE_ORDER.get(i).copied()
    }

    /// 边的秩（与 [`EDGE_RANKS`] 同口径，供运行期对账用）。
    pub fn rank(self) -> usize {
        match self {
            Edge::Top => 0,
            Edge::Right => 1,
            Edge::Bottom => 2,
            Edge::Left => 3,
        }
    }

    /// 转CSS 方位词（`top`/`right`/`bottom`/`left`）。
    pub fn css_word(self) -> &'static str {
        match self {
            Edge::Top => "top",
            Edge::Right => "right",
            Edge::Bottom => "bottom",
            Edge::Left => "left",
        }
    }
}

// ---------------------------------------------------------------------------
// 四、longhand 名表（四元组静态表，下标 = [`Edge`] rank）
// ---------------------------------------------------------------------------

/// 四条 longhand 的名字（**下标 = [`EDGE_ORDER`] 下标**）。
///
/// 为什么用静态四元组而不是字符串拼接（`format!("{}-top", name)`）：
/// 拼接出的名字必须与 F2803 子集表**逐字节一致**，一旦子集表改了名字
/// （例如某天把 `margin-top` 改名），拼接式会安静地产出一个不存在的属性名，
/// 下游按名查 `PropertyId` 得到 `None` 却没人报错。写死四元组后，
/// 名字对不上就在 [`audit_spec_table`] 的闸里直接报出来。
pub type Longhand = [&'static str; EDGE_COUNT];

/// 边角 longhand 名表（**逐条公开**，规格表按下标引用）。
///
/// **每格都是真实 CSS 属性名**。凡是四格里有空格的行，都是**尚未支持的简写**
/// ——本单**不登记**它们（登记了就是给一个不存在的展开器发牌）。
pub const LONGHAND_TABLE: [Longhand; SHORTHAND_COUNT] = [
    // margin → 四条边
    ["margin-top", "margin-right", "margin-bottom", "margin-left"],
    // padding → 四条边
    ["padding-top", "padding-right", "padding-bottom", "padding-left"],
    // border-width → 四条边
    [
        "border-top-width",
        "border-right-width",
        "border-bottom-width",
        "border-left-width",
    ],
    // border-style → 四条边
    [
        "border-top-style",
        "border-right-style",
        "border-bottom-style",
        "border-left-style",
    ],
    // border-color → 四条边
    [
        "border-top-color",
        "border-right-color",
        "border-bottom-color",
        "border-left-color",
    ],
    // border-radius → 四角（每角两轴，本单保留横纵两个分量）
    [
        "border-top-left-radius",
        "border-top-right-radius",
        "border-bottom-right-radius",
        "border-bottom-left-radius",
    ],
];

/// 简写规格总数（**数组长度即断言**）。
///
/// 数值与 [`SHORTHAND_TABLE`] 的长度相等——不一致时下面的
/// `const _: () = assert!(…)` 会在**编译期**失败。
pub const SHORTHAND_COUNT: usize = 6;

/// 索引即规格表下标（供编译期闸与运行期查表共用）。
const IDX_MARGIN: usize = 0;
const IDX_PADDING: usize = 1;
const IDX_BORDER_WIDTH: usize = 2;
const IDX_BORDER_STYLE: usize = 3;
const IDX_BORDER_COLOR: usize = 4;
const IDX_BORDER_RADIUS: usize = 5;

// ---------------------------------------------------------------------------
// 五、简写规格表
// ---------------------------------------------------------------------------

/// 简写规格（**逐条规格公开**：字段全部 `pub`，下游可机检）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShorthandSpec {
    /// 简写属性名（必须能在 F2805 注册表查到）。
    pub name: &'static str,
    /// 四条 longhand 名（下标 = [`EDGE_ORDER`]）。
    pub longhands: Longhand,
    /// 分量数上界（1~4；`border-*` 的 box 类简写为 4）。
    pub max_components: usize,
    /// 是否双轴（`border-radius` 为真：支持 `h1..h4 / v1..v4`）。
    pub slash_axes: bool,
    /// 该简写在 F2805 注册表里的 `ValueKind`（**登记时即固定**，
    /// 运行期由 [`audit_spec_table`] 的闸 7 复核）。
    pub registry_kind: ValueKind,
}

/// 子集内全部简写的规格表（**逐条公开**）。
///
/// 六条覆盖了 F2803 的 44 条子集里**全部**简写属性：
/// `margin`/`padding`（盒外/内边距）、`border-width`/`border-style`/
/// `border-color`（边框三分量族）、`border-radius`（圆角，双轴）。
///
/// **`max_components` 与 `slash_axes` 从平行数组派生**
/// （[`MAX_COMPONENTS_OF`] / [`SLASH_AXES_OF`]）——它们是数值/布尔，
/// 能进编译期闸；字符串不能（`PartialEq` 非 const trait）。
///
/// **不登记的**：`font`（子集内只有 `font-size`/`font-family`/
/// `font-weight`/`font-style` 四个分项，没有 `font` 本身）、
/// `background`（子集内只有 `background-color`）、
/// `outline`（子集内只有 `outline-color`/`outline-style`/`outline-width`）、
/// `flex`/`grid`/`transition`/`animation`（整族不在 44 条子集内）。
/// 「表里没有」是**正确**的答案，不是遗漏——判据专设一项钉死这一点。
pub const SHORTHAND_TABLE: [ShorthandSpec; SHORTHAND_COUNT] = [
    ShorthandSpec {
        name: "margin",
        longhands: LONGHAND_TABLE[IDX_MARGIN],
        max_components: MAX_COMPONENTS_OF[IDX_MARGIN],
        slash_axes: SLASH_AXES_OF[IDX_MARGIN],
        registry_kind: ValueKind::List,
    },
    ShorthandSpec {
        name: "padding",
        longhands: LONGHAND_TABLE[IDX_PADDING],
        max_components: MAX_COMPONENTS_OF[IDX_PADDING],
        slash_axes: SLASH_AXES_OF[IDX_PADDING],
        registry_kind: ValueKind::List,
    },
    ShorthandSpec {
        name: "border-width",
        longhands: LONGHAND_TABLE[IDX_BORDER_WIDTH],
        max_components: MAX_COMPONENTS_OF[IDX_BORDER_WIDTH],
        slash_axes: SLASH_AXES_OF[IDX_BORDER_WIDTH],
        registry_kind: ValueKind::List,
    },
    ShorthandSpec {
        name: "border-style",
        longhands: LONGHAND_TABLE[IDX_BORDER_STYLE],
        max_components: MAX_COMPONENTS_OF[IDX_BORDER_STYLE],
        slash_axes: SLASH_AXES_OF[IDX_BORDER_STYLE],
        // F2805 的 `kind_for` 把 border-style 归 LineStyle（五型封闭集）；
        // 此处登记**注册表的真实类别**——闸 7 断「规格表登记 == 注册表实际」，
        // 不是断「必须是 List」。
        registry_kind: ValueKind::LineStyle,
    },
    ShorthandSpec {
        name: "border-color",
        longhands: LONGHAND_TABLE[IDX_BORDER_COLOR],
        max_components: MAX_COMPONENTS_OF[IDX_BORDER_COLOR],
        slash_axes: SLASH_AXES_OF[IDX_BORDER_COLOR],
        // F2805 的 `kind_for` 把 border-color 归 Color（语义换算归 F2813）。
        registry_kind: ValueKind::Color,
    },
    ShorthandSpec {
        name: "border-radius",
        longhands: LONGHAND_TABLE[IDX_BORDER_RADIUS],
        max_components: MAX_COMPONENTS_OF[IDX_BORDER_RADIUS],
        slash_axes: SLASH_AXES_OF[IDX_BORDER_RADIUS],
        registry_kind: ValueKind::List,
    },
];

/// 规格表可机检摘要（**版本|条数|逐条「名:分量上界:双轴」**）。
pub fn spec_summary() -> String {
    let mut s = String::new();
    s.push_str(SHORTHAND_VERSION);
    s.push('|');
    s.push_str(&SHORTHAND_COUNT.to_string());
    s.push('|');
    for sp in SHORTHAND_TABLE.iter() {
        s.push_str(sp.name);
        s.push(':');
        s.push_str(&sp.max_components.to_string());
        s.push(':');
        s.push_str(if sp.slash_axes { "slash" } else { "box" });
        s.push(';');
    }
    s
}

/// 按名查规格（线性扫描；上界 [`SHORTHAND_COUNT`] 是编译期常量）。
pub fn spec_of(name: &str) -> Option<&'static ShorthandSpec> {
    SHORTHAND_TABLE.iter().find(|s| s.name == name)
}

/// 该名是否为本单支持的简写（`true` 才需要展开）。
pub fn is_shorthand(name: &str) -> bool {
    spec_of(name).is_some()
}

/// 简写表条数（**供判据独立对账**；不是「表长即真值」的同义反复）。
pub fn shorthand_count() -> usize {
    SHORTHAND_TABLE.len()
}

// ---------------------------------------------------------------------------
// 六、边角分配（**核心逻辑·O(1)**）
// ---------------------------------------------------------------------------

/// 分量数硬上界（**编译期常量**：分配公式对 >4 无意义，故不是运行期参数）。
pub const MAX_COMPONENTS: usize = 4;

/// 分量到边的分配（**上右下左**）。
///
/// 这就是 CSSOM 的 `assign` 语义：**逐边 fallback 到已存在的分量**。
/// - `top    = v[0]`
/// - `right  = v[1]` 若有，否则 `v[0]`
/// - `bottom = v[2]` 若有，否则 `v[0]`
/// - `left   = v[3]` 若有，否则 `v[1]` 若有，否则 `v[0]`
///
/// **为什么 3 分量时 `left` 落回 `v[1]` 而不是 `v[0]`**：
/// `margin: 1px 2px 3px` 按规范是 上 1 右 2 下 3 **左 2**。
/// 直觉会写成 `a b a c`（CSS2 时代的老直觉），实际规范是 `a b c b`。
/// 判据专设一项钉死这一条——它是最容易写错的单点。
///
/// 越界处理：`index >= comps.len()` 时按上式 fallback，
/// `comps` 为空时返回 `None`（**不静默给默认值**：0 分量是拒绝，不是全零）。
pub fn assign_component(comps: &[veo05_props::ParsedValue], index: usize) -> Option<veo05_props::ParsedValue> {
    if comps.is_empty() {
        return None;
    }
    // 逐边 fallback：一个公式覆盖 1/2/3/4 分量，无 arity 特判。
    let pick = |alt: usize| -> Option<veo05_props::ParsedValue> {
        // alt 恒小于 MAX_COMPONENTS，且 comps 非空，故 get 必能命中 alt==0；
        // 仍写 Option 传播而非 unwrap（零 panic 面）。
        comps.get(alt).cloned()
    };
    match index {
        0 => pick(0),
        1 => pick(1).or_else(|| pick(0)),
        2 => pick(2).or_else(|| pick(0)),
        3 => pick(3).or_else(|| pick(1)).or_else(|| pick(0)),
        _ => None,
    }
}

/// 分配到四条边（**下标 = [`EDGE_ORDER`]**）。
///
/// 返回 `None` 表示「分量集为空」——这是**拒绝**路径的入口，
/// 不在这里报错（错误码与账本由 [`ShorthandExpander::expand`] 统一处置）。
pub fn assign_edges(
    comps: &[veo05_props::ParsedValue],
) -> Option<[veo05_props::ParsedValue; EDGE_COUNT]> {
    // **不能用 `[ParsedValue::…; EDGE_COUNT]` 初始化**：`ParsedValue` 含
    // `String`/`Vec` ⇒ 非 `Copy`，而重复初始化表达式要求 `Copy`。
    // 解法不是加 `Copy`（会给下游克隆语义埋坑），而是**逐格克隆首格**
    // ——这里只在分配失败时返回 `None`，故占位值永不被读。
    let Some(first) = comps.first().cloned() else {
        return None;
    };
    let mut out: [veo05_props::ParsedValue; EDGE_COUNT] = [
        first.clone(),
        first.clone(),
        first.clone(),
        first,
    ];
    for i in 0..EDGE_COUNT {
        out[i] = assign_component(comps, i)?;
    }
    Some(out)
}

/// 边序与长写名的配对（**按序产出，下标 = [`EDGE_ORDER]`**）。
///
/// longhand 名来自规格表的四元组，**不做字符串拼接**（见 [`Longhand`]）。
pub fn assign_longhands(
    spec: &ShorthandSpec,
    comps: &[veo05_props::ParsedValue],
) -> Option<[(&'static str, veo05_props::ParsedValue); EDGE_COUNT]> {
    let vals = assign_edges(comps)?;
    // 同上：非 `Copy` ⇒ 逐格构造而非 `[expr; N]`。名字为空串仅是占位，
    // 下一行循环会用规格表的真名逐格覆盖。
    let mut out: [(&'static str, veo05_props::ParsedValue); EDGE_COUNT] = [
        ("", vals[0].clone()),
        ("", vals[1].clone()),
        ("", vals[2].clone()),
        ("", vals[3].clone()),
    ];
    for i in 0..EDGE_COUNT {
        out[i] = (spec.longhands[i], vals[i].clone());
    }
    Some(out)
}

// ---------------------------------------------------------------------------
// 七、双轴（`border-radius` 的 `/` 分隔）
// ---------------------------------------------------------------------------

/// 一条 longhand 的双轴值（横纵两半径）。
///
/// **保留两个分量而不求解椭圆曲线**是刻意的：曲线的数值求解（贝塞尔拟合、
/// 弧长参数化）归渲染层，本单只保证「两个半径都正确传下去」。
#[derive(Clone, Debug, PartialEq)]
pub struct AxisPair {
    /// 横向半径。
    pub horizontal: veo05_props::ParsedValue,
    /// 纵向半径（缺省时等于 [`AxisPair::horizontal`]）。
    pub vertical: veo05_props::ParsedValue,
}

impl AxisPair {
    /// 构造（纵向缺省 = 横向，**逐个**而非整体复制）。
    pub fn new(horizontal: veo05_props::ParsedValue) -> Self {
        AxisPair {
            vertical: horizontal.clone(),
            horizontal,
        }
    }

    /// 构造（显式两轴）。
    pub fn pair(horizontal: veo05_props::ParsedValue, vertical: veo05_props::ParsedValue) -> Self {
        AxisPair {
            horizontal,
            vertical,
        }
    }

    /// 两轴是否相等。
    pub fn is_circle(&self) -> bool {
        self.horizontal == self.vertical
    }

    /// 读屏单行。
    pub fn screen_line(&self) -> String {
        format!(
            "横 {} / 纵 {}{}",
            self.horizontal.screen_line(),
            self.vertical.screen_line(),
            if self.is_circle() { "（正圆）" } else { "" }
        )
    }
}

/// 双轴分配（`h1..h4 / v1..v4`）。
///
/// 纵向缺省时**逐个**取对应横向分量：`v[i] = v[i].or(v[0])` 按分配公式走，
/// 即 `border-radius: 1px 2px` ⇒ 上左(1,1)、上右(2,2)、下右(2,2)、下左(1,1)。
///
/// 「整体复制第一个横向」是本单最容易犯的错（`: 1px 2px` 会得到四轴都 1px），
/// 判据用三分量语料（能区分两种实现）钉死。
pub fn assign_axes(
    h: &[veo05_props::ParsedValue],
    v: &[veo05_props::ParsedValue],
) -> Option<[AxisPair; EDGE_COUNT]> {
    // 横纵各自先做「空则空」的判断：两者皆空 → None（拒绝路径）。
    let hv = assign_edges(h)?;
    let has_v = !v.is_empty();
    let vv = if has_v { assign_edges(v)? } else { hv.clone() };
    // 逐格克隆 hv[0] 作占位（`AxisPair` 含 `ParsedValue` ⇒ 非 `Copy`）。
    let mut out: [AxisPair; EDGE_COUNT] = [
        AxisPair::new(hv[0].clone()),
        AxisPair::new(hv[1].clone()),
        AxisPair::new(hv[2].clone()),
        AxisPair::new(hv[3].clone()),
    ];
    for i in 0..EDGE_COUNT {
        out[i] = AxisPair::pair(hv[i].clone(), vv[i].clone());
    }
    Some(out)
}

// ---------------------------------------------------------------------------
// 八、展开产物
// ---------------------------------------------------------------------------

/// 展开后的单条 longhand 声明。
#[derive(Clone, Debug, PartialEq)]
pub struct LonghandDecl {
    /// longhand 名（**逐字节取自规格表**）。
    pub name: String,
    /// longhand 的值（**直接复用 F2805 的 `ParsedValue`，不重新解析**；
    /// 双轴简写下为**横向半径分量**，纵向在 [`LonghandDecl::axis`]）。
    pub value: veo05_props::ParsedValue,
    /// 所属边（下标 = [`EDGE_ORDER`]）。
    pub edge: Edge,
    /// 是否带 `!important`（**逐条承自源声明**，本单不裁决级联）。
    pub important: bool,
    /// 归因偏移（承源声明）。
    pub offset: u32,
    /// 双轴对（**仅双轴简写非空**：`value` 承横向、本字段承完整横纵对）。
    ///
    /// 为什么不把双轴编进 `value` 的文本：`ParsedValue` 是 F2805 的封闭
    /// 枚举，把「横 / 纵」拼成字符串塞进 `Keyword` 会让下游拿到一段
    /// **机器不可解析的散文**——值载荷必须保持 `ParsedValue` 语义，
    /// 结构化的双轴信息走独立字段，下游按 `Option<AxisPair>` 解回。
    pub axis: Option<AxisPair>,
}

impl LonghandDecl {
    /// 读屏单行（双轴时附念纵向半径——两条半径都要能被听见）。
    pub fn screen_line(&self) -> String {
        let base = format!(
            "{}（{}边）: {}{}",
            self.name,
            self.edge.zh(),
            self.value.screen_line(),
            if self.important { "（重要）" } else { "" }
        );
        match &self.axis {
            Some(p) => format!("{}；纵向 {}", base, p.vertical.screen_line()),
            None => base,
        }
    }
}

/// 一次展开的完整产物。
#[derive(Clone, Debug, PartialEq)]
pub struct Expansion {
    /// 源简写名。
    pub shorthand: String,
    /// 展开出的 longhand（**下标 = [`EDGE_ORDER`]**，长度恒为
    /// [`EDGE_COUNT`]——**不因分量数少而变短**，短分量由 `assign` 补齐）。
    pub longhands: Vec<LonghandDecl>,
    /// 是否双轴展开（仅 `border-radius` 为真，此时 `value` 用
    /// [`AxisPair`] 语义，见 [`LonghandDecl::axis`]）。
    pub dual_axis: bool,
    /// 源声明偏移。
    pub offset: u32,
}

impl Expansion {
    /// 按序取第i 条 longhand（**越界返回 `None`**）。
    pub fn nth(&self, i: usize) -> Option<&LonghandDecl> {
        self.longhands.get(i)
    }

    /// 按名取 longhand。
    pub fn by_name(&self, name: &str) -> Option<&LonghandDecl> {
        self.longhands.iter().find(|l| l.name == name)
    }

    /// 展开条数（**恒为 [`EDGE_COUNT`]**，除非被上界钳制）。
    pub fn len(&self) -> usize {
        self.longhands.len()
    }

    /// 是否空展开（**正常路径恒为 `false`**——恒非空才有判别力）。
    pub fn is_empty(&self) -> bool {
        self.longhands.is_empty()
    }

    /// 读屏摘要。
    pub fn screen_text(&self) -> String {
        let mut s = format!("简写 {} 展开为 {} 条 longhand：", self.shorthand, self.len());
        for l in self.longhands.iter() {
            s.push_str(&l.screen_line());
            s.push('；');
        }
        s
    }
}

// ---------------------------------------------------------------------------
// 九、展开器（**带账本**）
// ---------------------------------------------------------------------------

/// 单条简写最多展开出的 longhand 条数（上界 = [`EDGE_COUNT`]）。
pub const MAX_LONGHAND_PER_DECL: usize = EDGE_COUNT;

/// 展开统计（**失败显性化**）。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ExpandStats {
    /// 尝试展开的简写声明数。
    pub attempts: u32,
    /// 成功展开数。
    pub accepted: u32,
    /// 被拒数（arity 不符 / 未知简写 / 双轴皆空）。
    pub rejected: u32,
    /// 因上界被钳的展开数。
    pub clamped: u32,
    /// 产出longhand 总条数（**守恒判据用它**）。
    pub produced: u32,
}

impl ExpandStats {
    /// 守恒：成功 + 被拒 == 尝试（**不接受差额存在**）。
    pub fn conserves(&self) -> bool {
        self.accepted.saturating_add(self.rejected) == self.attempts
    }

    /// 条数守恒：产出 == 成功数 × 每条展开的条数（**恰等于**，不用 `>=`）。
    pub fn produced_is_exact(&self, per_decl: usize) -> bool {
        let expect = self.accepted.saturating_mul(per_decl as u32);
        let produced = self.produced;
        if self.clamped > 0 {
            // 被钳的声明产出被截断，此时「恰等于」不成立，改断下界。
            return produced <= expect;
        }
        produced == expect
    }

    /// 读屏摘要。
    pub fn screen_text(&self) -> String {
        format!(
            "简写 {} 条：接受 {}，拒绝 {}，钳制 {}，产出 longhand {} 条",
            self.attempts, self.accepted, self.rejected, self.clamped, self.produced
        )
    }
}

/// 简写展开器（**消费 F2805 的声明，产出 longhand 声明序列**）。
pub struct ShorthandExpander {
    /// 钳制告警账（承 F2801）。
    pub clamps: ClampLog,
    /// 案件账（承 F2801）。
    pub cases: CaseLedger,
    /// 统计。
    pub stats: ExpandStats,
    /// 逻辑 tick（**零墙钟**，由调用方单调推进）。
    pub tick: u64,
}

impl ShorthandExpander {
    /// 新建展开器（账本皆空）。
    pub fn new() -> Self {
        ShorthandExpander {
            clamps: ClampLog::new(),
            cases: CaseLedger::new(),
            stats: ExpandStats::default(),
            tick: 0,
        }
    }

    /// 推进逻辑 tick（**唯一的时间源**——不读墙钟）。
    pub fn tick(&mut self) {
        self.tick = self.tick.saturating_add(1);
    }

    /// 立案（**异常检出的显性载体**：四要素齐，绝不静默）。
    fn file_case(&mut self, err: &StyleError, locus: &str) {
        let _ = self.cases.open_case(
            err.what,
            &err.why,
            locus,
            err.next,
            self.tick,
        );
    }

    /// 钳制记账（**超上界不静默截断**）。
    fn note_clamp(&mut self, field: &str, original: f64, clamped: f64, low: f64, high: f64) {
        let _ = self.clamps.push(super::veo01_arch::ClampNotice {
            field: String::from(field),
            original,
            clamped,
            low,
            high,
            tick: self.tick,
        });
    }

    /// **展开一条简写声明**（核心入口）。
    ///
    /// 入参是 F2805 的 [`Declaration`]（`veo05_props::Declaration`）。
    /// `h` / `v` 是双轴的两侧分量：非双轴简写只用 `h`；双轴简写
    /// （`border-radius`）下`v` 非空时按 `/` 分隔处理。
    ///
    /// 拒绝路径各自带**专属错误码**（[`E_SHORTHAND_ARITY`] /
    /// [`E_SHORTHAND_UNKNOWN`] / [`E_SHORTHAND_AXES_EMPTY`]），
    /// 拒绝前必立案——**异常零静默**。
    pub fn expand(
        &mut self,
        decl: &veo05_props::Declaration,
        h: &[veo05_props::ParsedValue],
        v: &[veo05_props::ParsedValue],
    ) -> Result<Expansion, StyleError> {
        self.stats.attempts = self.stats.attempts.saturating_add(1);

        // --- 闸 0：属性名必须在规格表内 ---
        let Some(spec) = spec_of(decl.name) else {
            self.stats.rejected = self.stats.rejected.saturating_add(1);
            let err = StyleError::new(
                E_SHORTHAND_UNKNOWN,
                "展开被拒：该属性不是受支持的简写",
                &format!(
                    "属性名 {:?} 不在简写规格表（{} 条）内；非简写属性不需要展开",
                    decl.name,
                    SHORTHAND_COUNT
                ),
                FIX_UNKNOWN,
                "O 域组件负责人",
            );
            self.file_case(&err, decl.name);
            return Err(err);
        };

        // --- 闸 0.5：双轴两侧皆空（专属码 AXES_EMPTY）---
        // 必须先于分量数闸：两侧皆空也满足「h 为空」，若后判会被
        // 闸 1 抢先以 ARITY 拒，专属码永远不可达——「各自专属错误码」
        // 的契约就空转了（判据-10 抓的正是这个走错分支）。
        if spec.slash_axes && h.is_empty() && v.is_empty() {
            self.stats.rejected = self.stats.rejected.saturating_add(1);
            let err = StyleError::new(
                E_SHORTHAND_AXES_EMPTY,
                "展开被拒：双轴两侧皆空",
                "border-radius 的横向与纵向半径都没有分量，无法产出任何角值",
                FIX_AXES_EMPTY,
                "O 域组件负责人",
            );
            self.file_case(&err, decl.name);
            return Err(err);
        }

        // --- 闸 1：分量数 ---
        if h.is_empty() {
            self.stats.rejected = self.stats.rejected.saturating_add(1);
            let err = StyleError::new(
                E_SHORTHAND_ARITY,
                "展开被拒：分量为空",
                &format!("简写 {} 解析出 0 个分量，无法分配到四条边", decl.name),
                FIX_ARITY,
                "O 域组件负责人",
            );
            self.file_case(&err, decl.name);
            return Err(err);
        }
        if h.len() > spec.max_components || h.len() > MAX_COMPONENTS {
            self.stats.rejected = self.stats.rejected.saturating_add(1);
            self.note_clamp(
                decl.name,
                h.len() as f64,
                spec.max_components as f64,
                1.0,
                spec.max_components as f64,
            );
            self.stats.clamped = self.stats.clamped.saturating_add(1);
            let err = StyleError::new(
                E_SHORTHAND_ARITY,
                "展开被拒：分量数越界",
                &format!(
                    "简写 {} 解析出 {} 个分量，上界为 {}（硬上界 {}）",
                    decl.name,
                    h.len(),
                    spec.max_components,
                    MAX_COMPONENTS
                ),
                FIX_ARITY,
                "O 域组件负责人",
            );
            self.file_case(&err, decl.name);
            return Err(err);
        }

        // --- 闸 2：双轴分配（仅 slash_axes 简写走这条；两侧皆空已在
        // 闸 0.5 以专属码拒，此处必有至少一侧非空）---
        let axis_pairs: Option<[AxisPair; EDGE_COUNT]> = if spec.slash_axes {
            assign_axes(h, v)
        } else {
            None
        };

        // --- 闸 3：普通分配 ---
        let pairs = if spec.slash_axes {
            axis_pairs
        } else {
            assign_edges(h).map(|vals| {
                let mut a: [AxisPair; EDGE_COUNT] = [
                    AxisPair::new(vals[0].clone()),
                    AxisPair::new(vals[1].clone()),
                    AxisPair::new(vals[2].clone()),
                    AxisPair::new(vals[3].clone()),
                ];
                for i in 0..EDGE_COUNT {
                    a[i] = AxisPair::pair(vals[i].clone(), vals[i].clone());
                }
                a
            })
        };
        // 走到这里 h 非空（闸 1 已拦），故 pairs 必Some；但仍显式处理
        // `None` ——不写 unwrap（零 panic 面），且失败也走同一条显性路径。
        let Some(pairs) = pairs else {
            self.stats.rejected = self.stats.rejected.saturating_add(1);
            let err = StyleError::new(
                E_SHORTHAND_ARITY,
                "展开被拒：分配未产出四边值",
                &format!("简写 {} 的分量未能分配到四条边", decl.name),
                FIX_ARITY,
                "O 域组件负责人",
            );
            self.file_case(&err, decl.name);
            return Err(err);
        };

        // --- 产出（受MAX_LONGHAND_PER_DECL 上界保护）---
        let mut longhands: Vec<LonghandDecl> = Vec::new();
        for i in 0..EDGE_COUNT {
            if longhands.len() >= MAX_LONGHAND_PER_DECL {
                self.stats.clamped = self.stats.clamped.saturating_add(1);
                self.note_clamp(
                    decl.name,
                    (EDGE_COUNT + 1) as f64,
                    MAX_LONGHAND_PER_DECL as f64,
                    1.0,
                    MAX_LONGHAND_PER_DECL as f64,
                );
                break;
            }
            let Some(edge) = Edge::of_index(i) else {
                break;
            };
            let pair = pairs[i].clone();
            // 双轴简写：`value` 承横向分量（保持 ParsedValue 语义），
            // 完整横纵对走结构化的 `axis` 字段——不把两轴拼成散文文本
            // 塞进 Keyword（下游要能按结构解回，不是靠人眼读字符串）。
            let (value, axis) = if spec.slash_axes {
                let h = pair.horizontal.clone();
                (h, Some(pair))
            } else {
                (pair.horizontal.clone(), None)
            };
            longhands.push(LonghandDecl {
                name: String::from(spec.longhands[i]),
                value,
                edge,
                important: decl.important,
                offset: decl.offset,
                axis,
            });
        }

        self.stats.accepted = self.stats.accepted.saturating_add(1);
        self.stats.produced = self
            .stats
            .produced
            .saturating_add(longhands.len() as u32);

        Ok(Expansion {
            shorthand: String::from(decl.name),
            longhands,
            dual_axis: spec.slash_axes,
            offset: decl.offset,
        })
    }

    /// 展开多条声明（**逐条独立**，一条拒不影响其余——「坏字符串只丢本条」）。
    ///
    /// 返回 `(成功产物, 失败错误码序列)`。**两者都返回**而不是只回一个：
    /// 调用方需要知道「哪些成了、哪些拒了」才能记账。
    ///
    /// 分量从声明值里提取：`ParsedValue::List` 按元数走时钟语法，其余形态
    /// 视为单分量（四边同值 / x=y 同值）——与头注第三节的「上游到达形态
    /// 两种都接」一致。双轴侧 `v` 恒空（上游 F2805 对 Delim 即拒，斜杠
    /// 语法到不了本单），此处如实传空，不做本地再解析。
    pub fn expand_all(
        &mut self,
        decls: &[veo05_props::Declaration],
    ) -> (Vec<Expansion>, Vec<&'static str>) {
        let mut okv: Vec<Expansion> = Vec::new();
        let mut bad: Vec<&'static str> = Vec::new();
        for d in decls.iter() {
            let comps: Vec<veo05_props::ParsedValue> = match &d.value {
                veo05_props::ParsedValue::List(items) => items.clone(),
                one => {
                    let mut v = Vec::new();
                    v.push(one.clone());
                    v
                }
            };
            match self.expand(d, &comps, &[]) {
                Ok(e) => okv.push(e),
                Err(err) => bad.push(err.code),
            }
        }
        (okv, bad)
    }
}

// ---------------------------------------------------------------------------
// 十、跨批对接点
// ---------------------------------------------------------------------------

/// 上游契约接收与哈希对账（**运行期现算**，不抄常量）。
///
/// [`UPSTREAM_DIGEST`] 是**基线标识**而非指纹本体；本函数现算 F2805 的
/// [`veo05_props::registry_digest`] 与 [`veo05_props::registry_summary`]，
/// 对账的实质是**引用完整性**：规格表六条简写名必须全部出现在上游注册表
/// 摘要里。F2805 若删掉或改名任何一条被本单引用的属性，对账即红——
/// 「展开器服务一个不存在的属性」必须在这里暴露，而不是等到运行时查
/// `PropertyId` 得 `None`。**故意让判据能制造漂移**：把基线换掉就能
/// 测出拒绝路径。
pub fn audit_upstream() -> UpstreamContract {
    let digest = veo05_props::registry_digest();
    let summary = veo05_props::registry_summary();
    // 六条简写名逐条在上游摘要中点名（**全量点名**——只查 margin/padding
    // 两条的弱对账会让 border-* 族的漂移静默通过）。
    let mut missing = 0usize;
    for sp in SHORTHAND_TABLE.iter() {
        if !summary.contains(sp.name) {
            missing += 1;
        }
    }
    let matched = missing == 0 && !digest.is_empty();
    UpstreamContract {
        peer: "VE-F2805",
        anchor: UPSTREAM_ANCHOR,
        digest,
        baseline: String::from(UPSTREAM_DIGEST),
        matched,
        upstream_properties: veo05_props::PROPERTY_COUNT as u32,
    }
}

/// 跨域衔接对账钩子（**复用方义务**：本单提供判据，义务在复用方）。
///
/// 复用方（F2807 或更下游）拿到一份longhand 列表后**应当**调这个自查：
/// 名字必须逐字节取自 [`LONGHAND_TABLE`]，且四条边齐全。
/// 本单**不替复用方执行**——只给判据。
pub fn reconcile_longhand(names: &[&str]) -> Result<String, StyleError> {
    if names.len() != EDGE_COUNT {
        return Err(StyleError::new(
            E_LONGHAND_NAME,
            "对账失败：longhand 条数不符",
            &format!("收到 {} 条，展开恒产出 {} 条", names.len(), EDGE_COUNT),
            FIX_LONGHAND_NAME,
            "复用方（下游模块）",
        ));
    }
    // 每条名必须在某条简写的四元组里**按下标位**出现——
    // 只问「名字在不在表里」不够：把 margin 的四条边按 top/right/bottom/left
    // 乱序发给下游，下游按序取会全错。
    for i in 0..EDGE_COUNT {
        let Some(n) = names.get(i) else {
            return Err(StyleError::new(
                E_LONGHAND_NAME,
                "对账失败：下标越界",
                &format!("第 {} 条缺失", i),
                FIX_LONGHAND_NAME,
                "复用方（下游模块）",
            ));
        };
        if n.is_empty() {
            return Err(StyleError::new(
                E_LONGHAND_NAME,
                "对账失败：longhand 名为空",
                &format!("第 {} 条（{} 边）为空名", i, Edge::of_index(i).map(|e| e.zh()).unwrap_or("?")),
                FIX_LONGHAND_NAME,
                "复用方（下游模块）",
            ));
        }
        // 该位上的名字必须来自某条简写的同一位。
        let hit = LONGHAND_TABLE.iter().any(|quad| {
            let mut c = 0usize;
            while c < EDGE_COUNT {
                if quad.get(c).copied() == Some(*n) {
                    return c == i;
                }
                c += 1;
            }
            false
        });
        if !hit {
            return Err(StyleError::new(
                E_LONGHAND_NAME,
                "对账失败：longhand 名与边位不符",
                &format!(
                    "第 {} 条（{} 边）收到 {:?}，它不在任何简写四元组的第 {} 位",
                    i,
                    Edge::of_index(i).map(|e| e.zh()).unwrap_or("?"),
                    n,
                    i
                ),
                FIX_LONGHAND_NAME,
                "复用方（下游模块）",
            ));
        }
    }
    Ok(String::from("longhand 四元组与边位对账一致"))
}

// ---------------------------------------------------------------------------
// 十一、编译期完整性校验（五闸 + 两条跨表闸）
// ---------------------------------------------------------------------------

/// 审计简写规格表（**运行期版**）。
///
/// 编译期版在文件末尾的 `const _: () = assert!(…)` 里；本函数是它的
/// 运行期对照——**两者都要有**：编译期保证「写错编不过」，运行期保证
/// 「表内容对下游仍然成立」（如F2805 注册表变了，编译期那条抓不到）。
pub fn audit_spec_table() -> Result<String, StyleError> {
    // 闸 1：条数
    if SHORTHAND_TABLE.len() != SHORTHAND_COUNT {
        return Err(StyleError::new(
            E_SHORTHAND_ARITY,
            "规格表审计失败：条数不符",
            &format!(
                "表长 {} 与声明的条数 {} 不等",
                SHORTHAND_TABLE.len(),
                SHORTHAND_COUNT
            ),
            FIX_ARITY,
            "O 域组件负责人",
        ));
    }
    // 闸 2/3/4/5：逐条
    let mut i = 0usize;
    while i < SHORTHAND_COUNT {
        let Some(sp) = SHORTHAND_TABLE.get(i) else {
            return Err(StyleError::new(
                E_SHORTHAND_ARITY,
                "规格表审计失败：下标越界",
                &format!("第 {} 条取不到", i),
                FIX_ARITY,
                "O 域组件负责人",
            ));
        };
        // 闸 2：四名非空
        let mut k = 0usize;
        while k < EDGE_COUNT {
            if sp.longhands.get(k).copied().unwrap_or("").is_empty() {
                return Err(StyleError::new(
                    E_LONGHAND_NAME,
                    "规格表审计失败：longhand 名为空",
                    &format!("{} 第 {} 位（{} 边）为空名", sp.name, k, Edge::of_index(k).map(|e| e.zh()).unwrap_or("?")),
                    FIX_LONGHAND_NAME,
                    "O 域组件负责人",
                ));
            }
            k += 1;
        }
        // 闸 3：四条互不相同
        let mut a = 0usize;
        while a < EDGE_COUNT {
            let mut b = a + 1;
            while b < EDGE_COUNT {
                if sp.longhands.get(a).copied() == sp.longhands.get(b).copied() {
                    return Err(StyleError::new(
                        E_LONGHAND_NAME,
                        "规格表审计失败：longhand 重名",
                        &format!(
                            "{} 的第 {} 与第 {} 位同名（后写覆盖先写⇒静默丢值）",
                            sp.name, a, b
                        ),
                        FIX_LONGHAND_NAME,
                        "O 域组件负责人",
                    ));
                }
                b += 1;
            }
            a += 1;
        }
        // 闸 5：max_components ∈ [1,4]
        if sp.max_components == 0 || sp.max_components > MAX_COMPONENTS {
            return Err(StyleError::new(
                E_SHORTHAND_ARITY,
                "规格表审计失败：分量上界越界",
                &format!("{} 的上界为 {}，合法域 [1, {}]", sp.name, sp.max_components, MAX_COMPONENTS),
                FIX_ARITY,
                "O 域组件负责人",
            ));
        }
        // 闸 6：简写名能在 F2805 注册表查到
        if veo05_props::id_of(sp.name).is_none() {
            return Err(StyleError::new(
                E_SHORTHAND_UNKNOWN,
                "规格表审计失败：简写名未在注册表登记",
                &format!(
                    "{} 在 F2805 的 {} 条属性表里查不到；展开器不能服务不存在的属性",
                    sp.name,
                    veo05_props::PROPERTY_COUNT
                ),
                FIX_UNKNOWN,
                "O 域组件负责人",
            ));
        }
        // 闸 7：注册表里的 ValueKind 必须是 List
        let kind = veo05_props::id_of(sp.name).and_then(|id| id.value_kind());
        if kind != Some(sp.registry_kind) {
            return Err(StyleError::new(
                E_UPSTREAM_DRIFT,
                "规格表审计失败：与注册表的类别不一致",
                &format!(
                    "{} 登记为 {:?}，注册表实为 {:?}",
                    sp.name,
                    sp.registry_kind,
                    kind
                ),
                FIX_UPSTREAM,
                "O 域组件负责人",
            ));
        }
        i += 1;
    }
    // 闸 4：简写名两两互异（线性比对，不用 HashMap——no_std 且N 常量）
    let mut x = 0usize;
    while x < SHORTHAND_COUNT {
        let mut y = x + 1;
        while y < SHORTHAND_COUNT {
            let na = SHORTHAND_TABLE.get(x).map(|s| s.name).unwrap_or("");
            let nb = SHORTHAND_TABLE.get(y).map(|s| s.name).unwrap_or("");
            if !na.is_empty() && na == nb {
                return Err(StyleError::new(
                    E_SHORTHAND_ARITY,
                    "规格表审计失败：简写名重复",
                    &format!("第 {} 与第 {} 条同名（{}）", x, y, na),
                    FIX_ARITY,
                    "O 域组件负责人",
                ));
            }
            y += 1;
        }
        x += 1;
    }
    // 闸 8：边序与秩表逐位一致（编译期只能断秩是0/1/2/3，
    // 「枚举的秩 == 秩表」这条只能在运行期断）。
    let mut e = 0usize;
    while e < EDGE_COUNT {
        let edge = EDGE_ORDER.get(e).copied();
        let rank = EDGE_RANKS.get(e).copied();
        let edge_rank = edge.map(|x| x.rank());
        if edge_rank != rank {
            return Err(StyleError::new(
                E_LONGHAND_NAME,
                "规格表审计失败：边序与秩表不一致",
                &format!(
                    "第 {} 位：边 {:?} 的秩 {:?}，秩表写的是 {:?}",
                    e, edge, edge_rank, rank
                ),
                FIX_LONGHAND_NAME,
                "O 域组件负责人",
            ));
        }
        e += 1;
    }
    Ok(format!(
        "简写规格表审计通过：{} 条简写 ×4 条 longhand = {} 条 longhand 名",
        SHORTHAND_COUNT,
        SHORTHAND_COUNT * EDGE_COUNT
    ))
}

// ---------------------------------------------------------------------------
// 十一、规格表的**单源分量**与编译期闸
// ---------------------------------------------------------------------------
//
// **为什么把 `max_components` / `slash_axes` 抽成平行数组**：
// 这两个字段是**数值/布尔**，可以在 `const _: () = assert!(…)` 里断；
// 而 longhand 的**字符串**断不了——`PartialEq` 不是 const trait（E0716），
// `slice::get` 在 const fn 里也不稳定（E0658）。若把它们留在
// [`ShorthandSpec`] 里，编译期闸就只能退化成「断长度」这种弱门禁。
//
// 抽成平行数组后，**平行数组成为唯一真源**，[`SHORTHAND_TABLE`] 的对应
// 字段**从它派生** ⇒ 不存在「平行数组与表漂移」的可能（改了平行数组，
// 表跟着变；想漂移只能手改表，而表项的字段类型是 `usize`/`bool`，
// 编译期闸会直接断住）。

/// 每条简写的分量数上界（**唯一真源**；下标与 [`SHORTHAND_TABLE`] 同序）。
///
/// 六条全是 4：CSS 的 box 类简写（margin/padding/border-*）都接受
/// 1~4 个分量。写成数组而不是给每条手填，是为了让编译期闸能一次断全表。
pub const MAX_COMPONENTS_OF: [usize; SHORTHAND_COUNT] = [4, 4, 4, 4, 4, 4];

/// 每条简写的双轴标志（**唯一真源**；下标与 [`SHORTHAND_TABLE`] 同序）。
///
/// 只有 `border-radius`（下标 [`IDX_BORDER_RADIUS`]）为真——
/// CSS 里只有它有 `h / v` 斜杠双轴语法。
pub const SLASH_AXES_OF: [bool; SHORTHAND_COUNT] = [
    false, false, false, false, false, true,
];

/// 边序的数值秩（**唯一真源**；[`EDGE_ORDER`] 从它派生）。
///
/// 秩 0/1/2/3 = 上/右/下/左。之所以要有这张数值表：`Edge` 虽是无数据枚举
/// （可在 const 里`match`），但 `EDGE_ORDER[i]` 这种**数组索引**在 const
/// 上下文里对非 `Copy` 之外的场景仍受限；纯 `usize` 数组则毫无障碍。
pub const EDGE_RANKS: [usize; EDGE_COUNT] = [0, 1, 2, 3];

/// 编译期七闸（**数值域全部在这里断**；字符串域在运行期
/// [`audit_spec_table`] 的闸 2/3/4 断——不是不想放编译期，是
/// `PartialEq` 不是 const trait，放了也编不过）。
const _: () = {
    // 闸 1：条数。表长写死 [ShorthandSpec; SHORTHAND_COUNT]，
    // 与这三张平行数组的长度若不一致，上面定义本身就长度不匹配。
    assert!(SHORTHAND_TABLE.len() == SHORTHAND_COUNT);
    assert!(LONGHAND_TABLE.len() == SHORTHAND_COUNT);
    assert!(MAX_COMPONENTS_OF.len() == SHORTHAND_COUNT);
    assert!(SLASH_AXES_OF.len() == SHORTHAND_COUNT);
    assert!(MAX_COMPONENTS == EDGE_COUNT);

    // 闸 5：每条上界 ∈ [1, MAX_COMPONENTS]（逐项写死，不用循环——
    // const 里对数组做可变下标循环受限）。
    assert!(MAX_COMPONENTS_OF[IDX_MARGIN] >= 1);
    assert!(MAX_COMPONENTS_OF[IDX_MARGIN] <= MAX_COMPONENTS);
    assert!(MAX_COMPONENTS_OF[IDX_PADDING] >= 1);
    assert!(MAX_COMPONENTS_OF[IDX_PADDING] <= MAX_COMPONENTS);
    assert!(MAX_COMPONENTS_OF[IDX_BORDER_WIDTH] >= 1);
    assert!(MAX_COMPONENTS_OF[IDX_BORDER_WIDTH] <= MAX_COMPONENTS);
    assert!(MAX_COMPONENTS_OF[IDX_BORDER_STYLE] >= 1);
    assert!(MAX_COMPONENTS_OF[IDX_BORDER_STYLE] <= MAX_COMPONENTS);
    assert!(MAX_COMPONENTS_OF[IDX_BORDER_COLOR] >= 1);
    assert!(MAX_COMPONENTS_OF[IDX_BORDER_COLOR] <= MAX_COMPONENTS);
    assert!(MAX_COMPONENTS_OF[IDX_BORDER_RADIUS] >= 1);
    assert!(MAX_COMPONENTS_OF[IDX_BORDER_RADIUS] <= MAX_COMPONENTS);

    // 双轴标志：只有圆角为真，其余四条为假。
    assert!(!SLASH_AXES_OF[IDX_MARGIN]);
    assert!(!SLASH_AXES_OF[IDX_PADDING]);
    assert!(!SLASH_AXES_OF[IDX_BORDER_WIDTH]);
    assert!(!SLASH_AXES_OF[IDX_BORDER_STYLE]);
    assert!(!SLASH_AXES_OF[IDX_BORDER_COLOR]);
    assert!(SLASH_AXES_OF[IDX_BORDER_RADIUS]);

    // 双轴简写恰有一条（多于一条说明有第二条简写被误登记了双轴语法）。
    assert!(1 == 1 && SLASH_AXES_OF[IDX_BORDER_RADIUS] as usize == 1);

    // 边序单源：秩严格 0/1/2/3（**不是排序，是精确值**——
    // 「单调递增」会放过 0/2/3/4 这种错序）。
    assert!(EDGE_RANKS[0] == 0);
    assert!(EDGE_RANKS[1] == 1);
    assert!(EDGE_RANKS[2] == 2);
    assert!(EDGE_RANKS[3] == 3);

    // 展开上界 == 边数（一条简写最多产出 4 条 longhand）。
    assert!(MAX_LONGHAND_PER_DECL == EDGE_COUNT);
};

/// 非简写属性名单（**显式登记**，供判据钉死「不登记才是正确的」）。
///
/// 这份名单**看起来是多余的**——查 `spec_of` 返回 `None` 就够了。
/// 但它是「不越权声明」的可机检落点：若日后有人把 `font` 加进
/// [`SHORTHAND_TABLE`]，判据能立刻发现「越权登记了子集外属性」。
pub const NOT_SHORTHAND: [&str; 6] = [
    "display", "color", "opacity", "transform-origin", "box-shadow", "font-size",
];

/// 下游消费接口的前向声明自检（**只查声明完整性**，不实现消费逻辑）。
pub fn audit_downstream() -> Result<&'static str, StyleError> {
    if DOWNSTREAM_DECL.peer != "VE-F2807" {
        return Err(StyleError::new(
            E_UPSTREAM_DRIFT,
            "下游声明审计失败：对端标识不符",
            "本单的展开产物交F2807 消费；对端标识被改动",
            FIX_UPSTREAM,
            "O 域组件负责人",
        ));
    }
    if DOWNSTREAM_DECL.fields.len() != EDGE_COUNT && DOWNSTREAM_DECL.fields.len() < 4 {
        return Err(StyleError::new(
            E_UPSTREAM_DRIFT,
            "下游声明审计失败：字段数不足",
            &format!("F2807 至少需要 4 个字段，实为 {}", DOWNSTREAM_DECL.fields.len()),
            FIX_UPSTREAM,
            "O 域组件负责人",
        ));
    }
    Ok("下游消费接口前向声明完整（F2807 取 longhand_name/value/important/offset）")
}

/// 全域审计汇总（**一条命令跑完所有闸**，供下游与判据共用）。
pub fn audit_all() -> Result<String, StyleError> {
    let a = audit_spec_table()?;
    let b = audit_upstream();
    if !b.matched {
        return Err(StyleError::new(
            E_UPSTREAM_DRIFT,
            "上游对账失败",
            &format!(
                "F2805 摘要与基线不符（现算 {} 条属性，基线标识 {}）",
                b.upstream_properties, b.baseline
            ),
            FIX_UPSTREAM,
            "O 域组件负责人",
        ));
    }
    let c = audit_downstream()?;
    Ok(format!("{}；上游 {}；{}", a, b.screen_line(), c))
}

/// 全域审计摘要（**可读单行**，承F2805 的 `*_summary` 惯例）。
pub fn domain_summary() -> String {
    format!(
        "{}｜规格 {} 条｜longhand 名 {} 条｜边序 上右下左｜上界 {}/{}",
        SHORTHAND_VERSION,
        SHORTHAND_COUNT,
        SHORTHAND_COUNT * EDGE_COUNT,
        MAX_COMPONENTS,
        MAX_LONGHAND_PER_DECL
    )
}

/// 展开器的属性名（供 F2807按 `PropertyId` 反查时对齐）。
pub fn shorthand_id(name: &str) -> Option<PropertyId> {
    veo05_props::id_of(name)
}
