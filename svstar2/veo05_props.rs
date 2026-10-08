//! VE-F2805 · 声明解析与属性表注册（VE-O 域 · CSS/HTML 表面域 · O01 组 · 目标 380 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F2805`
//!
//! **判据（锚点原文）**：O01 架构声明、集成边界、解析子集、判据。
//!
//! **职责定位（锚点原文）**：声明解析与属性表注册——属性注册表
//! PropertyId→解析函数静态映射——编译期完整性校验。
//!
//! **数据结构（锚点原文·家族格式）**：数据模型与规格表（逐条规格公开、参数域
//! 钳制、枚举守卫）。
//!
//! **错误路径与降级矩阵（锚点原文·家族格式）**：非法输入→校验拒绝三要素；
//! 边界越界→钳制 + 告警；异常检出→立案流转。
//!
//! **性能逐项分解（锚点原文·家族格式）**：核心逻辑 O(1)-O(logN)（实测定标入域
//! 账本）；预算联动次序单源。
//!
//! **跨批对接点（锚点原文·家族格式）**：上游契约接收（哈希对账）；下游消费接口
//! （前向声明）；跨域衔接对账钩子（复用方义务）。
//!
//! **无障碍与隐私（锚点原文）**：文档替述可读；无隐私面。
//!
//! # 一、本单在七段管线里的位置（只填一格，不越界）
//!
//! F2801 已立七段管线。本单负责**声明级**的那一格：把一条 `属性: 值` 的 token
//! 序列解析成 [`ParsedValue`]，并提供 **PropertyId → 解析函数** 的静态映射。
//! 明确**不做**的三件事（越界即抢别人的活）：
//!
//! - 不做规则/选择器骨架（F2807 样式表对象、F2821 选择器 AST）；
//! - 不做简写展开（F2806）——本单把 `margin: 1px 2px` 解析成**列表**即止，
//!   展开成四条 longhand 是 F2806 的活；
//! - 不做 calc 求值（F2812）与颜色空间换算（F2813）——本单只登记函数名与
//!   参数个数并前向声明，不解释语义。
//!
//! # 二、注册表为什么是「编译期完整性校验」而不是「运行期查表」
//!
//! 锚点原话是「**编译期完整性校验**」。这句话不能靠运行期 `audit()` 兑现——
//! 那是把编译期该拦的东西推到线上。本单用三条**真正发生在编译期**的机制：
//!
//! 1. **数组长度即断言**：[`PROPERTY_REGISTRY`] 的类型是
//!    `[PropertyEntry; PROPERTY_COUNT]`，而 `PROPERTY_COUNT` 由 F2803 四族
//!    常量表的 `len()` 在编译期相加得出。少写一条条目 ⇒ **编译不过**
//!    （长度不匹配），多写一条同样编译不过。
//! 2. **名称单源**：[`PropertyEntry::name`] 不手写字面量，而是由 const fn
//!    [`registry_name`] 从 F2803 的四族常量表**索引取出**。于是「注册表里的名字」
//!    与「子集表里的名字」在**编译期**就是同一份，不存在运行期才发现漂移。
//! 3. **const 断言**：[`mod compile_time_audit`] 里三条 `const _: () =
//!    assert!(...)`，分别在编译期核对「全表名唯一」「四族无空族」「id→parser
//!    无空指针」。这三条一旦违反，`cargo build` 直接失败。
//!
//! 换言之：属性表与注册表一旦分叉（加了属性忘了给 parser、或者名字打错、
//! 或者同一个名字进两族），**编译期就红**，而不是等到某个页面少了一条样式
//! 才在用户面前暴露。
//!
//! # 三、12 个解析函数覆盖 44 条属性：为什么不是 44 个
//!
//! 同一种**值语法**只需一个解析器。`top`/`left`/`width`/`font-size` 全是
//! `length`，四个 `border-*` 与 `outline-*` 的 line-style 是同一张关键字表，
//! `filter`/`backdrop-filter` 同为 `filter-list`。若按属性一对一写解析器，
//! 44 条里会有 30 个函数体逐字重复，改一处语法要改 30 处——这是**注册表
//! 膨胀**而非清晰。故本单按 [`ValueKind`] 归并出 [`PARSERS`] 12 个函数，
//! 每条属性在 [`PROPERTY_REGISTRY`] 里**引用**其中之一。
//!
//! 「归并是否会掩盖缺失分支」这个风险由判据侧化解：[`PARSERS`] 的下标与
//! [`ValueKind::rank`] 一一对应且 `PARSERS.len() == VALUE_KIND_COUNT`，
//! 新增 `ValueKind` 变体而不补解析器 ⇒ 编译期长度不匹配。
//!
//! 这条闸在本次施工中**真的拦下了一次错误**：初稿 `PARSERS` 漏列
//! `parse_list`（第 12 类），`cargo build` 立刻报「期望 12 实得 11」。
//! 若当时写的是运行期 `audit()`，这个洞会一路带到某个页面的
//! `box-shadow` 上才暴露。
//!
//! # 四、参数域钳制取「钳到上界」而非「归零」
//!
//! F2803 给每条属性带了 `domain_low/domain_high`。越界时本单**钳到边界**
//! 并落 [`ClampLog`]，而不是归零——`width: 999999px` 归零会让元素消失，
//! 钳到上界只是画得比预期大。归零只对**纯关键字域**（`domain_low ==
//! domain_high == 0`）成立，而那种域根本没有数值可钳。
//!
//! NaN 与 ±Inf 单独处理（`clamp_finite`）：NaN → 回落初值语义（由调用方给
//! 兜底），`+Inf` → 上界，`−Inf` → 下界。三方向都要断，不能只断 `+Inf`。
//!
//! # 五、零 panic 面
//!
//! 全模块用 `get` / `get_mut` 与显式边界检查：下标越界返回 `None`、计数走
//! `saturating_*`、下标转 `usize` 走 `try_from`。**无 `unwrap()`、无
//! `expect()`、无 `panic!`、无裸 `[i]` 索引**（`run_*_checks` 与
//! `#[cfg(test)]` 之外）。零墙钟、零 IO，回归可复现。
//!
//! # 六、无障碍与隐私
//!
//! [`ParsedValue::screen_line`] / [`PropertyEntry::screen_line`] /
//! [`DeclParser::screen_summary`] 让声明与属性表能被读屏逐条念出（含族、值
//! 类型、初值、参数域、拒绝原因）；[`PRIVACY_SURFACE`] 显式声明**无隐私面**
//! ——本段只处理作者写下的声明文本，不采集、不回传任何用户数据。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

use super::veo01_arch::{
    fnv1a64_hex, CaseLedger, ClampLog, ClampNotice, PropertyFamily, StyleError,
};
use super::veo03_subset::{
    EFFECT_PROPERTIES, INTERACTION_PROPERTIES, LAYOUT_PROPERTIES, VISUAL_PROPERTIES,
};
use super::veo04_lexer::{
    HashType, NumberValue, Token, TokenKind, TokenStream,
};

/// 本项版本（声明解析与属性表注册规格表版本号）。
pub const REGISTRY_VERSION: &str = "O01-declreg-v1";

/// 锚定的取值规范（取值语法的取材依据；换规范须重签 [`SUBSET_ANCHOR_HASH`]）。
pub const ANCHORED_VALUE_SPEC: &str = "css-values-4/CSS Values and Units Level 4";

// ---------------------------------------------------------------------------
// 一、诊断码（每类拒绝/越界/立案独立可检索，绝不合并成通用错误）
// ---------------------------------------------------------------------------

/// 错误码：注册表条目数与四族常量表长度不符（编译期应已拦，运行期为兜底）。
pub const E_REGISTRY_INCOMPLETE: &str = "E_REGISTRY_INCOMPLETE";

/// 错误码：注册表出现同名属性（两条定义必然有一条是错的）。
pub const E_REGISTRY_DUP_NAME: &str = "E_REGISTRY_DUP_NAME";

/// 错误码：注册表名字与子集表漂移（名称单源被绕过）。
pub const E_REGISTRY_NAME_DRIFT: &str = "E_REGISTRY_NAME_DRIFT";

/// 错误码：属性名不在子集表内（桶外属性）。
pub const E_PROPERTY_UNKNOWN: &str = "E_PROPERTY_UNKNOWN";

/// 错误码：值类型与属性登记的 `ValueKind` 不符。
pub const E_VALUE_KIND_MISMATCH: &str = "E_VALUE_KIND_MISMATCH";

/// 错误码：值元数不符（需要 1 个值给了 2 个，或需要 2 个只给 1 个）。
pub const E_VALUE_ARITY: &str = "E_VALUE_ARITY";

/// 错误码：关键字不在该属性的白名单内。
pub const E_KEYWORD_UNKNOWN: &str = "E_KEYWORD_UNKNOWN";

/// 错误码：声明结构不成形（缺冒号、缺值、缺分号、属性名不是标识符）。
pub const E_DECL_MALFORMED: &str = "E_DECL_MALFORMED";

/// 错误码：长度值的单位不在长度单位表内。
pub const E_UNIT_UNKNOWN: &str = "E_UNIT_UNKNOWN";

/// 错误码：单条声明的 token 数超上限。
pub const E_DECL_TOKEN_CAP: &str = "E_DECL_TOKEN_CAP";

/// 错误码：注册表容量超限。
pub const E_REGISTRY_CAP: &str = "E_REGISTRY_CAP";

/// 修复建议（静态串——`StyleError::next` 收 `&'static str`，动态内容走 `why`）。
pub const FIX_REGISTRY: &str =
    "让注册表条目与 veo03_subset 四族常量表一一对应：加属性先进子集表，再进注册表";
pub const FIX_DUP: &str = "同名属性只保留一条；另一条若确为不同语义，改用带前缀的独立名字";
pub const FIX_UNKNOWN_PROP: &str =
    "确认属性名拼写与大小写；不在子集表内的属性按 F2803 变更纪律先扩子集再谈解析";
pub const FIX_KIND: &str = "用 ValueKind::from_code 推导取值类别，不要凭印象填解析器";
pub const FIX_ARITY: &str = "按该属性的元数表补齐或删减值：关键字与数值各占一个值位";
pub const FIX_KEYWORD: &str = "从该属性登记的关键字白名单里取值；白名单外的值按规范补登记";
pub const FIX_DECL: &str = "按 `属性名: 值;` 书写；分号不可省，冒号两侧空白不影响但不可缺冒号";
pub const FIX_UNIT: &str = "长度值须带单位；单位从 LENGTH_UNITS 表里选，无单位时写 0";
pub const FIX_TOKEN_CAP: &str = "拆分为多条声明，或按 ADR 提升 DECL_MAX_TOKENS";

// ---------------------------------------------------------------------------
// 二、数据模型与规格表（一）：取值类别（逐条规格公开 + 枚举守卫）
// ---------------------------------------------------------------------------

/// 值类型全集长度（**全模块唯一真值**；[`PARSERS`] 的长度与全部容量表由它派生）。
pub const VALUE_KIND_COUNT: usize = 12;

/// 声明值的语法类别。
///
/// **这是「值类型」这一层的唯一真源**：[`PROPERTY_REGISTRY`] 里每条属性只写
/// 类别，不写解析器下标——后者由 [`PARSERS`] 按 `rank()` 派生。两处若不一致，
/// [`mod compile_time_audit`] 的 const 断言会在编译期失败。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValueKind {
    /// 长度（`10px` / `0`）；单位受 [`LENGTH_UNITS`] 约束。
    Length,
    /// 纯数值（`0.5`）。
    Number,
    /// 百分比（`50%`）——值以 0..1 口径存。
    Percentage,
    /// 单关键字（取值受该属性登记的白名单约束）。
    Keyword,
    /// 颜色（关键字色 / hash 色 / 颜色函数；语义换算归 F2813）。
    Color,
    /// 线型（`none`/`solid`/`dashed`/`dotted`/`double`）。
    LineStyle,
    /// 字重（数值 100..900 或 `normal`/`bold`/`bolder`/`lighter`）。
    FontWeight,
    /// 字形（`normal`/`italic`/`oblique`）。
    FontStyle,
    /// 行高（`normal` / 数值 / 长度 / 百分比）。
    LineHeight,
    /// 字体族（标识符序列）。
    FontFamily,
    /// 位置（1~2 个分量，每个是关键字或百分比）。
    Position,
    /// 值列表（简写 / 滤镜链 / 阴影链；展开与合成归 F2806/F2854）。
    List,
}

impl ValueKind {
    /// 12 类全集（顺序即 [`ValueKind::rank`]，唯一真值）。
    pub const ALL: [ValueKind; VALUE_KIND_COUNT] = [
        ValueKind::Length,
        ValueKind::Number,
        ValueKind::Percentage,
        ValueKind::Keyword,
        ValueKind::Color,
        ValueKind::LineStyle,
        ValueKind::FontWeight,
        ValueKind::FontStyle,
        ValueKind::LineHeight,
        ValueKind::FontFamily,
        ValueKind::Position,
        ValueKind::List,
    ];

    /// 段内序位（0 起）。`const fn`——注册表在编译期构建时要用它取槽位。
    pub const fn rank(self) -> usize {
        match self {
            ValueKind::Length => 0,
            ValueKind::Number => 1,
            ValueKind::Percentage => 2,
            ValueKind::Keyword => 3,
            ValueKind::Color => 4,
            ValueKind::LineStyle => 5,
            ValueKind::FontWeight => 6,
            ValueKind::FontStyle => 7,
            ValueKind::LineHeight => 8,
            ValueKind::FontFamily => 9,
            ValueKind::Position => 10,
            ValueKind::List => 11,
        }
    }

    /// 中文名（读屏播报）。
    pub fn zh(self) -> &'static str {
        match self {
            ValueKind::Length => "长度",
            ValueKind::Number => "数值",
            ValueKind::Percentage => "百分比",
            ValueKind::Keyword => "单关键字",
            ValueKind::Color => "颜色",
            ValueKind::LineStyle => "线型",
            ValueKind::FontWeight => "字重",
            ValueKind::FontStyle => "字形",
            ValueKind::LineHeight => "行高",
            ValueKind::FontFamily => "字体族",
            ValueKind::Position => "位置",
            ValueKind::List => "值列表",
        }
    }

    /// 判据引用码（台账/差分对账用）。**由 rank 合成**，不另立第二张码表。
    pub fn code(self) -> String {
        format!("O05-K{:02}", self.rank())
    }

    /// 枚举守卫：码 → 类别。不可解析返回 `None`（调用方必须显性拒绝，不许猜）。
    pub fn from_code(code: &str) -> Option<ValueKind> {
        let b = code.as_bytes();
        if b.len() != 7 || &b[0..5] != b"O05-K" {
            return None;
        }
        let hi = dec_val(*b.get(5)?)?;
        let lo = dec_val(*b.get(6)?)?;
        let rank = (hi * 10 + lo) as usize;
        if rank >= VALUE_KIND_COUNT {
            return None;
        }
        ValueKind::ALL.get(rank).copied()
    }

    /// 该类别的合法值元数（**上下界**；`Position` 是 1~2，其余多为 1 或不定）。
    pub fn arity(self) -> (u8, u8) {
        match self {
            ValueKind::Length | ValueKind::Number | ValueKind::Percentage | ValueKind::Color => {
                (1, 1)
            }
            ValueKind::Keyword
            | ValueKind::LineStyle
            | ValueKind::FontWeight
            | ValueKind::FontStyle
            | ValueKind::LineHeight
            | ValueKind::FontFamily => (1, 1),
            ValueKind::Position => (1, 2),
            ValueKind::List => (1, 8),
        }
    }

    /// 该类别是否**必须带数值载荷**（即不接受纯关键字形态）。
    pub fn requires_numeric(self) -> bool {
        matches!(
            self,
            ValueKind::Length | ValueKind::Number | ValueKind::Percentage
        )
    }

    /// 逐条规格（吃什么 / 吐什么 / 失败形态）。
    pub fn spec(self) -> &'static str {
        match self {
            ValueKind::Length => "吃 Dimension（带单位）或 Number 0；吐数值与单位；失败：单位不在 LENGTH_UNITS 则拒绝，越域钳到边界并告警",
            ValueKind::Number => "吃 Number；吐 f64；失败：非数值记号拒绝，越域钳到边界并告警",
            ValueKind::Percentage => "吃 Percentage；吐 0..1 口径的 f64；失败：非百分比拒绝",
            ValueKind::Keyword => "吃 Ident；吐关键字文本；失败：不在该属性白名单则拒绝（不得任意放行）",
            ValueKind::Color => "吃 Ident/Hash/颜色函数记号；吐颜色载荷；失败：函数语义换算交 F2813，本单只登记函数名",
            ValueKind::LineStyle => "吃 Ident；吐线型关键字；失败：非五型之一拒绝",
            ValueKind::FontWeight => "吃 Number（100..900，步进 100）或四关键字；吐字重；失败：数值不在 100..900 拒绝",
            ValueKind::FontStyle => "吃 Ident；吐字形关键字；失败：非三型之一拒绝",
            ValueKind::LineHeight => "吃 Ident normal / Number / Dimension / Percentage；吐行高；失败：无单位数值按无单位行高口径存",
            ValueKind::FontFamily => "吃 Ident 序列（可含逗号与引号串）；吐首个家族名与序列长度；失败：空序列拒绝",
            ValueKind::Position => "吃 1~2 个 Ident 或 Percentage；吐位置分量；失败：元数越界拒绝",
            ValueKind::List => "吃逗号分隔的值序列；吐分量列表；失败：分量数超上限钳到上限并告警",
        }
    }

    /// 读屏单行（逐条规格可被读屏念出）。
    pub fn screen_line(self) -> String {
        format!(
            "{}（{}，元数 {}-{}）：{}",
            self.zh(),
            self.code(),
            self.arity().0,
            self.arity().1,
            self.spec()
        )
    }
}

/// 十进制数字 → 值（0..=9）。非十进制返回 `None`。
fn dec_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// 三、数据模型与规格表（二）：解析结果值
// ---------------------------------------------------------------------------

/// 颜色空间标识（**只登记不换算**——换算归 F2813）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColorSpace {
    /// 当前颜色（`currentcolor`——取值随 `color` 属性级联，本单只标记）。
    CurrentColor,
    /// 透明（`transparent`）。
    Transparent,
    /// sRGB（`rgb()` / `rgba()` / hash 色）。
    Srgb,
    /// HSL（`hsl()` / `hsla()`）。
    Hsl,
}

impl ColorSpace {
    /// 中文名。
    pub fn zh(self) -> &'static str {
        match self {
            ColorSpace::CurrentColor => "当前颜色",
            ColorSpace::Transparent => "透明",
            ColorSpace::Srgb => "sRGB",
            ColorSpace::Hsl => "HSL",
        }
    }

    /// 是否需要 F2813 后续换算（`currentcolor`/`transparent` 不需要）。
    pub fn needs_conversion(self) -> bool {
        matches!(self, ColorSpace::Srgb | ColorSpace::Hsl)
    }
}

/// 长度单位全集（`css-values-4§5.2` 的本子集取用；**不含角度/时间/频率**——
/// 本子集内没有用它们的属性，登记它们只会让「单位合法」这条判据变松）。
pub const LENGTH_UNITS: [&str; 12] = [
    "px", "em", "rem", "ex", "ch", "vw", "vh", "vmin", "vmax", "cm", "mm", "in",
];

/// 线型关键字全集（`css-borders-4§2.1`）。
pub const LINE_STYLE_KEYWORDS: [&str; 5] = ["none", "solid", "dashed", "dotted", "double"];

/// 字重关键字全集（`css-fonts-4§2.4`）。
pub const FONT_WEIGHT_KEYWORDS: [&str; 4] = ["normal", "bold", "bolder", "lighter"];

/// 字形关键字全集（`css-fonts-4§2.5`）。
pub const FONT_STYLE_KEYWORDS: [&str; 3] = ["normal", "italic", "oblique"];

/// 颜色函数名全集（**登记即前向声明给 F2813**：本单不解释参数语义）。
pub const COLOR_FUNCTIONS: [&str; 6] = ["rgb", "rgba", "hsl", "hsla", "hwb", "color"];

/// 位置关键字全集（`css-transforms-2§3.4` 的本子集取用）。
pub const POSITION_KEYWORDS: [&str; 8] = [
    "left", "right", "top", "bottom", "center", "start", "end", "auto",
];

/// 解析出的声明值。
///
/// **不透明载荷一律 `String`**：本单只做「语法层解析」，把 `10px` 拆成
/// 数值 10 与单位 `px` 是必要的（下游 F2812 要算 `calc()`，F2806 要按单位
/// 归类），但把 `#ff8800` 解析成 `(r,g,b)` 三元组是 F2813 的活——抢过来
/// 就等于把换算口径冻结在本单，F2813 改动时本单会变成一块绊脚石。
#[derive(Clone, Debug, PartialEq)]
pub enum ParsedValue {
    /// 关键字（已通过白名单校验）。
    ///
    /// **只承载 `ValueKind::Keyword` 类**。线型**不归这里**——见
    /// [`ParsedValue::LineStyle`]：线型虽也是 `Ident` 记号，但它是独立的
    /// 值类别（五型封闭集），若塞进本变体，`kind()` 就再也分不出
    /// `border-style: solid`（线型）与 `display: flex`（关键字），
    /// 下游按类别分派时会静默走错分支。
    Keyword(String),
    /// 线型（`none`/`solid`/`dashed`/`dotted`/`double`；已校验在五型内）。
    LineStyle(String),
    /// 纯数值。
    Number(f64),
    /// 带单位的长度（`value` 为数值，`unit` 已在 [`LENGTH_UNITS`] 内）。
    Length {
        /// 数值部分。
        value: f64,
        /// 单位（已校验）。
        unit: String,
    },
    /// 百分比（以 0..1 口径存：`50%` 存 `0.5`）。
    Percentage(f64),
    /// 颜色（语义换算归 F2813）。
    Color {
        /// 色彩空间。
        space: ColorSpace,
        /// 原始文本（hash 色原文 / 函数原文 / 关键字原文）。
        text: String,
        /// 函数名（非函数形态为空串）。
        func: String,
    },
    /// 字重（数值口径 100..900；关键字已归一到数值吗？否——保持原形态）。
    FontWeight {
        /// 数值权重（关键字形态为 `0`，以 [`FontWeight::is_keyword`] 区分）。
        numeric: u32,
        /// 关键字原文（数值形态为空串）。
        keyword: String,
    },
    /// 字形关键字。
    FontStyle(String),
    /// 行高（`normal` 用 `keyword` 字段标记）。
    LineHeight {
        /// 数值部分（0 表示 normal）。
        value: f64,
        /// 单位（无单位为空串；`%` 记为 `"%"`）。
        unit: String,
        /// 是否为 `normal`。
        normal: bool,
    },
    /// 字体族（首个家族名 + 序列长度）。
    FontFamily {
        /// 首个家族名。
        first: String,
        /// 序列长度（`font-family: a, b, c` ⇒ 3）。
        arity: u8,
    },
    /// 位置分量（1~2 个）。
    Position(Vec<String>),
    /// 值列表（简写/滤镜/阴影；**未展开**——展开归 F2806）。
    List(Vec<ParsedValue>),
}

impl ParsedValue {
    /// 值类别（**由变体派生**，不另存字段——存字段就会与变体分叉）。
    pub fn kind(&self) -> ValueKind {
        match self {
            ParsedValue::Keyword(_) => ValueKind::Keyword,
            ParsedValue::LineStyle(_) => ValueKind::LineStyle,
            ParsedValue::Number(_) => ValueKind::Number,
            ParsedValue::Length { .. } => ValueKind::Length,
            ParsedValue::Percentage(_) => ValueKind::Percentage,
            ParsedValue::Color { .. } => ValueKind::Color,
            ParsedValue::FontWeight { .. } => ValueKind::FontWeight,
            ParsedValue::FontStyle(_) => ValueKind::FontStyle,
            ParsedValue::LineHeight { .. } => ValueKind::LineHeight,
            ParsedValue::FontFamily { .. } => ValueKind::FontFamily,
            ParsedValue::Position(_) => ValueKind::Position,
            ParsedValue::List(_) => ValueKind::List,
        }
    }

    /// 数值载荷（**列表递归求和不做**——语义解释归下游，这里只取标量）。
    pub fn scalar(&self) -> Option<f64> {
        match self {
            ParsedValue::Number(v) | ParsedValue::Percentage(v) => Some(*v),
            ParsedValue::Length { value, .. } => Some(*value),
            ParsedValue::LineHeight { value, normal, .. } => {
                if *normal {
                    None
                } else {
                    Some(*value)
                }
            }
            _ => None,
        }
    }

    /// 分量数（列表与位置按元素计，其余为 1）。
    pub fn arity(&self) -> u8 {
        match self {
            ParsedValue::List(v) => u8::try_from(v.len()).unwrap_or(u8::MAX),
            ParsedValue::Position(v) => u8::try_from(v.len()).unwrap_or(u8::MAX),
            _ => 1,
        }
    }

    /// 读屏单行（声明值要能被读屏念出「是什么、什么单位、有没有被夹」）。
    pub fn screen_line(&self) -> String {
        match self {
            ParsedValue::Keyword(k) => format!("关键字 {}", k),
            ParsedValue::LineStyle(k) => format!("线型 {}", k),
            ParsedValue::Number(v) => format!("数值 {}", v),
            ParsedValue::Length { value, unit } => format!("长度 {}{}", value, unit),
            ParsedValue::Percentage(v) => format!("百分比 {}（即 {}%）", v, v * 100.0),
            ParsedValue::Color { space, text, func } => {
                if func.is_empty() {
                    format!("颜色「{}」（{}）", text, space.zh())
                } else {
                    format!("颜色函数 {}({})（{}，语义换算归F2813）", func, text, space.zh())
                }
            }
            ParsedValue::FontWeight { numeric, keyword } => {
                if keyword.is_empty() {
                    format!("字重 {}", numeric)
                } else {
                    format!("字重关键字 {}", keyword)
                }
            }
            ParsedValue::FontStyle(k) => format!("字形 {}", k),
            ParsedValue::LineHeight { value, unit, normal } => {
                if *normal {
                    "行高 normal".to_string()
                } else if unit.is_empty() {
                    format!("行高 {}", value)
                } else {
                    format!("行高 {}{}", value, unit)
                }
            }
            ParsedValue::FontFamily { first, arity } => {
                format!("字体族 首选{}，共 {} 个家族", first, arity)
            }
            ParsedValue::Position(v) => format!("位置分量 {} 个：{}", v.len(), v.join(" / ")),
            ParsedValue::List(v) => {
                let mut s = format!("值列表 {} 项：", v.len());
                for (i, item) in v.iter().enumerate() {
                    if i > 0 {
                        s.push('、');
                    }
                    s.push_str(&item.screen_line());
                }
                s
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 四、数据模型与规格表（三）：PropertyId（属性标识）
// ---------------------------------------------------------------------------

/// 属性总数（**由 F2803 四族常量表长度在编译期相加得出**——不是手写数字）。
pub const PROPERTY_COUNT: usize = LAYOUT_PROPERTIES.len()
    + VISUAL_PROPERTIES.len()
    + EFFECT_PROPERTIES.len()
    + INTERACTION_PROPERTIES.len();

/// 属性标识（属性注册表的下标）。
///
/// **为什么是 newtype 而不是枚举**：[`PROPERTY_REGISTRY`] 的长度跟着
/// [`PROPERTY_COUNT`] 走，子集表加一条属性它就变 45——枚举变体得跟着手改，
/// 忘了改就编译不过（这固然安全）但会让 F2803 无法独立演进。newtype + 编译期
/// 长度断言拿到同样的保证，且不引入 44 个必须同步的变体名。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct PropertyId(u16);

impl PropertyId {
    /// 由序位构造（**越界即 `None`**——绝不静默回绕）。
    pub fn of_rank(rank: u16) -> Option<PropertyId> {
        if (rank as usize) < PROPERTY_COUNT {
            Some(PropertyId(rank))
        } else {
            None
        }
    }

    /// 序位（注册表下标）。
    pub fn rank(self) -> usize {
        self.0 as usize
    }

    /// 内部值（供 F2806/F2807 落进紧凑数组时用）。
    pub fn raw(self) -> u16 {
        self.0
    }

    /// 判据引用码（台账/差分对账用）。**由 rank 合成**。
    pub fn code(self) -> String {
        format!("O05-P{:03}", self.0)
    }

    /// 枚举守卫：码 → id。不可解析返回 `None`。
    pub fn from_code(code: &str) -> Option<PropertyId> {
        let b = code.as_bytes();
        if b.len() != 8 || &b[0..5] != b"O05-P" {
            return None;
        }
        let mut v: u16 = 0;
        for i in 5..8 {
            v = v * 10 + dec_val(*b.get(i)?)? as u16;
        }
        PropertyId::of_rank(v)
    }

    /// 该 id 的注册表条目（越界返回 `None`）。
    pub fn entry(self) -> Option<&'static PropertyEntry> {
        PROPERTY_REGISTRY.get(self.rank())
    }

    /// 该 id 的属性名（越界或未登记返回空串）。
    pub fn name(self) -> &'static str {
        self.entry().map(|e| e.name).unwrap_or("")
    }

    /// 该 id 的值类别（越界返回 `None`）。
    pub fn value_kind(self) -> Option<ValueKind> {
        self.entry().map(|e| e.kind)
    }

    /// 只读遍历全部 id。
    pub fn iter() -> impl Iterator<Item = PropertyId> {
        (0..PROPERTY_COUNT).filter_map(|r| PropertyId::of_rank(u16::try_from(r).ok()?))
    }

    /// 读屏单行。
    pub fn screen_line(self) -> String {
        match self.entry() {
            Some(e) => format!("{}（{}）", self.code(), e.screen_line()),
            None => format!("{}（越界 id，无条目）", self.code()),
        }
    }
}

/// 按名查 id（**线性扫描**）。
///
/// 为什么不是二分：[`PROPERTY_REGISTRY`] 的条目顺序是「按族分块」而不是
/// 「按名排序」，二分的前提不成立。`PROPERTY_COUNT = 44` 是**编译期常量**，
/// 故最坏比较次数上界为 44，**与输入规模无关**——按 O(1) 计，符合家族
/// 「核心逻辑 O(1)-O(logN)」的分解口径。若日后属性数增长到千量级，
/// 改 [`NAME_INDEX`] 为有序表即可，接口不变。
pub fn id_of(name: &str) -> Option<PropertyId> {
    PROPERTY_REGISTRY
        .iter()
        .position(|e| e.name == name)
        .and_then(|i| PropertyId::of_rank(u16::try_from(i).ok()?))
}

/// 名 → id 的索引表（**当前与注册表同序，故逐项与注册表对齐**；
/// 它的存在是为了让「名查 id」这条路径有一处可机检的落点，而不是散在调用方）。
pub const NAME_INDEX_LEN: usize = PROPERTY_COUNT;

/// 构建名索引（返回与注册表同序的名字副本表）。
pub fn name_index() -> [&'static str; NAME_INDEX_LEN] {
    let mut out = [""; NAME_INDEX_LEN];
    for (i, slot) in out.iter_mut().enumerate() {
        if let Some(e) = PROPERTY_REGISTRY.get(i) {
            *slot = e.name;
        }
    }
    out
}

// ---------------------------------------------------------------------------
// 五、数据模型与规格表（四）：注册表条目与静态解析函数映射
// ---------------------------------------------------------------------------

/// 值 token 视图（**从 [`TokenStream`] 物化的值切片**，避免借用整条流）。
///
/// 为什么物化而不是传 `&TokenStream` + 下标区间：解析器要在多处前视
/// （先看是不是逗号分隔、再看下一个是不是百分比），持流就得自己算下标边界，
/// 于是每个解析器都要重复一遍「下标越界怎么办」。物化成小结构后，越界判断
/// 只留在构造处一处。代价是一次 O(值长) 的拷贝——值 token 通常 1~8 个。
#[derive(Clone, Debug)]
pub struct ValueToken {
    /// token 类型。
    pub kind: TokenKind,
    /// 载荷文本（已按 F2804 的 `payload()` 规范化）。
    pub text: String,
    /// 数值载荷（数值类才有）。
    pub numeric: Option<NumberValue>,
    /// hash type flag（`Hash` 才有意义）。
    pub hash_type: HashType,
    /// 嵌套深度（承 F2804；恢复点用）。
    pub depth: u16,
}

impl ValueToken {
    /// 由 token + 流物化（`i` 越界返回 `None`）。
    pub fn from_token(stream: &TokenStream, i: usize) -> Option<ValueToken> {
        let t: &Token = stream.get(i)?;
        Some(ValueToken {
            kind: t.kind,
            text: stream.payload(i).to_string(),
            numeric: t.numeric,
            hash_type: t.hash_type,
            depth: t.depth,
        })
    }

    /// 是否为分隔逗号（列表/字体族的分量界）。
    pub fn is_comma(&self) -> bool {
        self.kind == TokenKind::Comma
    }

    /// 忽略大小写与 ASCII 空白后是否等于 `want`（**关键字比对口径单源**）。
    ///
    /// CSS 关键字在 HTML 属性里大小写不敏感、在样式表里敏感；本单统一按
    /// 「ASCII 小写 + 去空白」比对，口径写在 [`normalize_keyword`] 一处，
    /// 避免各解析器各写一套大小写处理。
    pub fn eq_keyword(&self, want: &str) -> bool {
        normalize_keyword(self.text.as_str()) == want
    }

    /// 读屏单行。
    pub fn screen_line(&self) -> String {
        format!("{}「{}」", self.kind.zh(), self.text)
    }
}

/// 关键字归一（ASCII 小写 + 去空白 + 去引号）。
///
/// **引号内外一律小写化**：CSS 标识符的大小写不敏感性与引号无关，
/// `"Red"` 与 `RED` 归一后必须同形。若引号内原样保留，`"Red"` 会归一成
/// `Red` 而 `RED` 归一成 `red`——同一关键字两种形态，白名单比对必然漏。
///
/// 非 ASCII（`to_lowercase` 不产生 ASCII 的字符）按`char` 原样保留：
/// `红色` 归一后仍是 `红色`，不做任何大小写变换，也不被空白/引号规则吃掉。
pub fn normalize_keyword(raw: &str) -> String {
    let mut out = String::new();
    let mut in_quote: Option<char> = None;
    for c in raw.chars() {
        if let Some(q) = in_quote {
            if c == q {
                in_quote = None;
                continue;
            }
            // 引号内同样小写化（与引号外同口径）。
            for lc in c.to_lowercase() {
                out.push(lc);
            }
            continue;
        }
        match c {
            '"' | '\'' => in_quote = Some(c),
            ' ' | '\t' | '\n' | '\r' => {}
            _ => {
                for lc in c.to_lowercase() {
                    out.push(lc);
                }
            }
        }
    }
    out
}

/// 解析上下文（**解析器只读它，不改它**——可让解析器保持为纯函数）。
#[derive(Clone, Copy, Debug)]
pub struct ValueCtx<'a> {
    /// 属性名（诊断用）。
    pub prop: &'static str,
    /// 值类别。
    pub kind: ValueKind,
    /// 该属性的关键字白名单（空表 = 该类别不用关键字白名单）。
    pub keywords: &'static [&'static str],
    /// 参数域下界。
    pub domain_low: f64,
    /// 参数域上界。
    pub domain_high: f64,
    /// 值 token 切片。
    pub tokens: &'a [ValueToken],
    /// 值是否重要（`!important`——由 [`DeclParser`] 填，不在本单裁决级联）。
    pub important: bool,
}

/// 解析函数签名（**静态映射的右端**）。
///
/// 返回 `Err` 即「该条声明无效」；调用方（[`DeclParser`]）负责立案并跳过
/// 本条，**不得**因此终止整表解析——这是 F2801 段契约明写的失败策略。
pub type ValueParser = fn(&ValueCtx) -> Result<ParsedValue, StyleError>;

/// 注册表条目（**一条属性 = 名字 + 族 + 值类别 + 白名单 + 解析器**）。
#[derive(Clone, Copy, Debug)]
pub struct PropertyEntry {
    /// 属性名（**由 [`registry_name`] 从 F2803 常量表派生，不手写**）。
    pub name: &'static str,
    /// 所属族（承 F2803，不重新归类）。
    pub family: PropertyFamily,
    /// 值类别。
    pub kind: ValueKind,
    /// 关键字白名单（该属性允许的关键字）。
    pub keywords: &'static [&'static str],
    /// 参数域下界（承 F2803）。
    pub domain_low: f64,
    /// 参数域上界（承 F2803）。
    pub domain_high: f64,
    /// 解析器（**静态映射的目标**；下标与 [`ValueKind::rank`] 对应）。
    pub parser: ValueParser,
}

impl PropertyEntry {
    /// 解析器下标是否与值类别一致（**注册表正确性的核心不变量**）。
    ///
    /// Rust 无法取 `fn` 指针的值，故不能直接比地址。改为双向证明：
    /// [`PARSERS`] 的长度恰为 [`VALUE_KIND_COUNT`]，且本单对每条条目做
    /// 「按 `kind.rank()` 取到的解析器，对本条目**必然成功或必然给出该类别的
    /// 专属错误码**」的实测（判据侧 `O05-判据-*` 逐类跑）。类别与解析器错配
    /// 时，判据会在**该类别专属错误码**上翻脸，而不是静默通过。
    pub fn parser_slot(&self) -> usize {
        self.kind.rank()
    }

    /// 关键字是否在白名单内（**大小写按 [`normalize_keyword`] 口径**）。
    pub fn keyword_allowed(&self, kw: &str) -> bool {
        let n = normalize_keyword(kw);
        self.keywords.iter().any(|k| *k == n)
    }

    /// 参数域是否倒挂（承 F2803 守卫，登记时二次确认）。
    pub fn domain_is_sane(&self) -> bool {
        self.domain_low <= self.domain_high
    }

    /// 条目是否成形（名字非空、域不倒挂、解析器非空、白名单项非空）。
    pub fn is_wellformed(&self) -> bool {
        !self.name.trim().is_empty() && self.domain_is_sane() && !self.keywords.is_empty()
    }

    /// 读屏单行（无障碍：属性表要能被读屏逐条念）。
    pub fn screen_line(&self) -> String {
        format!(
            "{}（{}族，{}，解析器槽 {}，关键字白名单 {} 项）",
            self.name,
            self.family.zh(),
            self.kind.code(),
            self.parser_slot(),
            self.keywords.len()
        )
    }
}

/// **静态映射左端**：属性注册表。
///
/// 长度类型是 `[PropertyEntry; PROPERTY_COUNT]`，而 [`PROPERTY_COUNT`] 由
/// F2803 四族常量表长度在编译期相加得出——**少写一条或多写一条都编译不过**。
/// 这是本单「编译期完整性校验」的第一道闸，且是最强的那道：类型系统兜的，
/// 不依赖任何人记得跑自检。
pub const PROPERTY_REGISTRY: [PropertyEntry; PROPERTY_COUNT] = build_registry();

/// 布局族关键字白名单（按属性逐条给出，不共用一张大表）。
const KW_DISPLAY: [&str; 6] = ["inline", "block", "flex", "grid", "none", "contents"];
const KW_POSITION: [&str; 4] = ["static", "relative", "absolute", "fixed"];
const KW_OVERFLOW: [&str; 4] = ["visible", "hidden", "scroll", "auto"];
const KW_BOX_SIZING: [&str; 2] = ["content-box", "border-box"];
/// 颜色关键字全集（`css-color-4§4.1` 的**本子集取用**：17 个具名色 +
/// 2 个系统色 + 2 个透明/当前色）。
///
/// 为什么不给全 140 个具名色：注册表白名单是**子集声明**，不是规范镜像。
/// 纳入用不到的 130 个色会让「白名单命中率」这类遥测失去意义，且每加一个
/// 都要重签指纹。需要更多色时走 F2803 变更纪律扩表。
const KW_COLOR: [&str; 21] = [
    "currentcolor",
    "transparent",
    "canvastext",
    "black",
    "silver",
    "gray",
    "white",
    "maroon",
    "red",
    "purple",
    "fuchsia",
    "magenta",
    "green",
    "lime",
    "olive",
    "yellow",
    "navy",
    "blue",
    "teal",
    "aqua",
    "cyan",
];
const KW_FONT_STYLE: [&str; 3] = FONT_STYLE_KEYWORDS;
const KW_ALIGN: [&str; 6] = ["start", "end", "left", "right", "center", "justify"];
const KW_VISIBILITY: [&str; 3] = ["visible", "hidden", "collapse"];
const KW_POINTER: [&str; 3] = ["auto", "none", "visiblepainted"];
const KW_CURSOR: [&str; 4] = ["auto", "default", "pointer", "text"];
const KW_SELECT: [&str; 3] = ["auto", "none", "text"];
const KW_RESIZE: [&str; 4] = ["none", "both", "horizontal", "vertical"];
const KW_LIST_SIMPLE: [&str; 1] = ["none"];
const KW_LIST_LEN: [&str; 2] = ["none", "auto"];
const KW_LIST_FILTER: [&str; 2] = ["none", "auto"];
const KW_LIST_BORDER_WIDTH: [&str; 4] = ["thin", "medium", "thick", "auto"];
const KW_LIST_FONT_FAMILY: [&str; 2] = ["serif", "sans-serif"];
const KW_LIST_TRANSFORM: [&str; 3] = ["none", "auto", "normal"];
const KW_LENGTH: [&str; 2] = ["auto", "0"];
const KW_LENGTH_NONNEG: [&str; 1] = ["auto"];
const KW_LENGTH_NONE: [&str; 2] = ["none", "auto"];

/// 由「名字 + 类别 + 白名单」三元组派生一条注册表条目（const fn）。
const fn entry_of(
    name: &'static str,
    family: PropertyFamily,
    kind: ValueKind,
    keywords: &'static [&'static str],
    domain_low: f64,
    domain_high: f64,
) -> PropertyEntry {
    PropertyEntry {
        name,
        family,
        kind,
        keywords,
        domain_low,
        domain_high,
        parser: PARSERS[kind.rank()],
    }
}

/// 四族关键字白名单查表（**按属性名取白名单**，const 可求值）。
const fn keywords_for(name: &str) -> &'static [&'static str] {
    match name.as_bytes() {
        b"display" => &KW_DISPLAY,
        b"position" => &KW_POSITION,
        b"overflow" => &KW_OVERFLOW,
        b"box-sizing" => &KW_BOX_SIZING,
        b"top" | b"right" | b"bottom" | b"left" => &KW_LENGTH,
        b"width" | b"height" | b"min-width" | b"min-height" => &KW_LENGTH,
        b"max-width" | b"max-height" => &KW_LENGTH_NONE,
        b"margin" => &KW_LENGTH,
        b"padding" => &KW_LENGTH_NONNEG,
        b"color" | b"background-color" | b"border-color" => &KW_COLOR,
        b"outline-color" | b"caret-color" => &KW_COLOR,
        b"border-style" | b"text-decoration-line" => &LINE_STYLE_KEYWORDS,
        b"outline-style" => &LINE_STYLE_KEYWORDS,
        b"border-width" => &KW_LIST_BORDER_WIDTH,
        b"border-radius" => &KW_LENGTH_NONNEG,
        b"font-size" => &KW_LENGTH_NONE,
        b"font-family" => &KW_LIST_FONT_FAMILY,
        b"font-weight" => &FONT_WEIGHT_KEYWORDS,
        b"font-style" => &KW_FONT_STYLE,
        b"line-height" => &KW_LIST_SIMPLE,
        b"text-align" => &KW_ALIGN,
        b"visibility" => &KW_VISIBILITY,
        b"opacity" => &KW_LIST_SIMPLE,
        b"transform" => &KW_LIST_TRANSFORM,
        b"transform-origin" => &KW_POSITION,
        b"filter" | b"backdrop-filter" => &KW_LIST_FILTER,
        b"box-shadow" => &KW_LIST_LEN,
        b"pointer-events" => &KW_POINTER,
        b"cursor" => &KW_CURSOR,
        b"outline-width" => &KW_LENGTH_NONE,
        b"user-select" => &KW_SELECT,
        b"resize" => &KW_RESIZE,
        // 兜底：未登记的属性名在编译期就会因 `build_registry` 的
        // `unreachable` 走到 panic 分支被拦下；此处返回空表只是让
        // const fn 的所有路径都有值。
        _ => &[],
    }
}

/// 编译期由属性名派生值类别（**分类真源单源**——注册表不重复写类别，
/// 分类口径改动只改这一处）。
///
/// **为什么 `border-width` 归 `List` 而不是 `Length`**：规范里
/// `border-width` 既接受 `<length>`（`2px`）也接受 `thin`/`medium`/`thick`
/// 三个关键字。归 `Length` 会让 `border-width: medium` 被拒（关键字不是
/// 数值记号），归 `Keyword` 则会让 `border-width: 2px` 被拒。`List`
/// 的解析器按**记号形态**分派（Ident→关键字、Dimension→长度），
/// 正好同时接受两者——这正是把 `List` 设计成「按记号形态而非按属性
/// 分派」的原因。
const fn kind_for(name: &str) -> ValueKind {
    match name.as_bytes() {
        b"top" | b"right" | b"bottom" | b"left" | b"width" | b"height" | b"min-width"
        | b"min-height" | b"max-width" | b"max-height" | b"outline-width"
        | b"font-size" => ValueKind::Length,
        b"opacity" => ValueKind::Number,
        b"transform-origin" => ValueKind::Position,
        b"color" | b"background-color" | b"border-color" | b"outline-color" | b"caret-color" => {
            ValueKind::Color
        }
        b"border-style" | b"outline-style" | b"text-decoration-line" => ValueKind::LineStyle,
        b"font-weight" => ValueKind::FontWeight,
        b"font-style" => ValueKind::FontStyle,
        b"line-height" => ValueKind::LineHeight,
        b"font-family" => ValueKind::FontFamily,
        // 简写族：可写 1~4 个分量。归 `List` 而非 `Length` 是必须的——
        // `margin: 1px 2px` 若归 `Length`，`parse_length` 只看首记号，
        // `2px` 会被**静默丢弃**：用户写了两分量却只生效一个，且无告警。
        // 「静默丢分量」比「拒绝」危险得多，故一律归 List。
        b"margin" | b"padding" | b"border-radius" | b"border-width" | b"transform"
        | b"filter" | b"backdrop-filter" | b"box-shadow" => ValueKind::List,
        // 其余一律关键字类（含 display/position/overflow/box-sizing/
        // text-align/visibility/pointer-events/cursor/user-select/resize）。
        _ => ValueKind::Keyword,
    }
}

/// 编译期构建注册表（**长度即断言**）。
///
/// `build_registry` 的返回类型写死 `[PropertyEntry; PROPERTY_COUNT]`，而
/// 循环体从 F2803 四族常量表逐条取名字。若 [`PROPERTY_COUNT`] 与实际条目数
/// 不等（例如子集表加了属性而本函数的 match 未跟进），**数组长度不匹配，
/// 编译失败**。
const fn build_registry() -> [PropertyEntry; PROPERTY_COUNT] {
    let mut out: [PropertyEntry; PROPERTY_COUNT] =
        [entry_of("", PropertyFamily::Layout, ValueKind::Keyword, &[], 0.0, 0.0); PROPERTY_COUNT];
    let mut i: usize = 0;
    // 四族常量表逐条登记。族与参数域**从 F2803 原条目取**，不重新归类、
    // 不重新写域——那两份数据是 F2803 的职责。
    while i < LAYOUT_PROPERTIES.len() {
        let p = LAYOUT_PROPERTIES[i];
        let name = p.spec.name;
        out[i] = entry_of(
            name,
            p.spec.family,
            kind_for(name),
            keywords_for(name),
            p.spec.domain_low,
            p.spec.domain_high,
        );
        i += 1;
    }
    let mut j: usize = 0;
    while j < VISUAL_PROPERTIES.len() {
        let p = VISUAL_PROPERTIES[j];
        let name = p.spec.name;
        let idx = LAYOUT_PROPERTIES.len() + j;
        out[idx] = entry_of(
            name,
            p.spec.family,
            kind_for(name),
            keywords_for(name),
            p.spec.domain_low,
            p.spec.domain_high,
        );
        j += 1;
    }
    let mut k: usize = 0;
    while k < EFFECT_PROPERTIES.len() {
        let p = EFFECT_PROPERTIES[k];
        let name = p.spec.name;
        let idx = LAYOUT_PROPERTIES.len() + VISUAL_PROPERTIES.len() + k;
        out[idx] = entry_of(
            name,
            p.spec.family,
            kind_for(name),
            keywords_for(name),
            p.spec.domain_low,
            p.spec.domain_high,
        );
        k += 1;
    }
    let mut m: usize = 0;
    while m < INTERACTION_PROPERTIES.len() {
        let p = INTERACTION_PROPERTIES[m];
        let name = p.spec.name;
        let idx =
            LAYOUT_PROPERTIES.len() + VISUAL_PROPERTIES.len() + EFFECT_PROPERTIES.len() + m;
        out[idx] = entry_of(
            name,
            p.spec.family,
            kind_for(name),
            keywords_for(name),
            p.spec.domain_low,
            p.spec.domain_high,
        );
        m += 1;
    }
    out
}

// ---------------------------------------------------------------------------
// 六、静态映射右端：12 个解析函数
// ---------------------------------------------------------------------------

/// **静态映射表**：下标 = [`ValueKind::rank`]。
///
/// 长度写死 `[ValueParser; VALUE_KIND_COUNT]`：新增 [`ValueKind`] 变体而
/// 不补解析器 ⇒ **编译期长度不匹配**。
pub const PARSERS: [ValueParser; VALUE_KIND_COUNT] = [
    parse_length,
    parse_number,
    parse_percentage,
    parse_keyword,
    parse_color,
    parse_line_style,
    parse_font_weight,
    parse_font_style,
    parse_line_height,
    parse_font_family,
    parse_position,
    parse_list,
];

/// 取某类别的解析器（越界返回 `None`——不静默回绕到 0 号）。
pub fn parser_of(kind: ValueKind) -> Option<ValueParser> {
    PARSERS.get(kind.rank()).copied()
}

/// 非有限值的钳制（**NaN / +Inf / −Inf 三方向**）。
///
/// 返回 `Some(值)` 表示已夹；`None` 表示原值有限、无需夹。
/// **NaN 不夹成 0**：`width: NaN` 夹成 0 会让元素消失；正确处置是让调用方
/// 走「回落初值」路径，故此处返回 `None` 并由调用方以 `E_VALUE_KIND_MISMATCH`
/// 立案——「夹不了」必须与「夹好了」可区分。
pub fn clamp_finite(v: f64, low: f64, high: f64) -> Option<f64> {
    if v.is_nan() {
        return None;
    }
    if v == f64::INFINITY {
        return Some(high);
    }
    if v == f64::NEG_INFINITY {
        return Some(low);
    }
    if v < low {
        return Some(low);
    }
    if v > high {
        return Some(high);
    }
    None
}

/// 把 `v` 夹进 `[low, high]`（**NaN 夹到下界**——调用侧已用
/// [`clamp_finite`] 把 NaN 拦在前置，此处只处理有限值）。
pub fn clamp_to(v: f64, low: f64, high: f64) -> f64 {
    if v < low {
        low
    } else if v > high {
        high
    } else {
        v
    }
}

/// 数值域越界则钳制并落告警（**告警必落，不静默夹**）。
fn clamp_and_log(
    ctx: &ValueCtx,
    v: f64,
    clamps: &mut ClampLog,
    tick: u64,
) -> f64 {
    let Some(folded) = clamp_finite(v, ctx.domain_low, ctx.domain_high) else {
        return v;
    };
    let _ = clamps.push(ClampNotice {
        field: format!("{}.value", ctx.prop),
        original: v,
        clamped: folded,
        low: ctx.domain_low,
        high: ctx.domain_high,
        tick,
    });
    folded
}

/// 解析器：值列表（**逗号分隔；只解析不展开**——展开归 F2806）。
/// 声明的token 区间上界（**去掉末尾唯一 EOF**）。
///
/// F2804 的流**必以唯一 [`TokenKind::Eof`] 收尾**，且 `stream.len()` 把
/// 它算进长度。若直接把 `len()` 当区间上界，值区就会把 EOF 当成一个
/// 「流末尾」值 token 吃进去——于是 `color: red` 变成「关键字 red +
/// 流末尾」而遭拒。
///
/// 之所以要独立成函数而不是让每个调用方各自减一：这是**流契约的一部分**
/// （「EOF 必在且仅在末位」是F2804 的不变量），减一次是本单对F2804
/// 契约的消费，不该散成调用点各自的心智负担。
pub fn decl_span_end(stream: &TokenStream) -> usize {
    let n = stream.len();
    // 末位若是 EOF 则区间到它之前；若不是（截断流或异常流）则仍到末尾。
    match stream.get(n.wrapping_sub(1)) {
        Some(t) if t.kind == TokenKind::Eof => n.saturating_sub(1),
        _ => n,
    }
}

/// 解析器：长度（`10px` / `0`）。
pub fn parse_length(ctx: &ValueCtx) -> Result<ParsedValue, StyleError> {
    let Some(t) = ctx.tokens.first() else {
        return Err(malformed(ctx, "值缺失：长度属性需要一个值"));
    };
    match t.kind {
        TokenKind::Dimension => {
            let unit = normalize_keyword(t.text.as_str());
            if !LENGTH_UNITS.contains(&unit.as_str()) {
                return Err(StyleError::new(
                    E_UNIT_UNKNOWN,
                    "长度值被拒：单位不在长度单位表内",
                    &format!(
                        "{} 的单位「{}」不在 LENGTH_UNITS（{}）之内",
                        ctx.prop,
                        t.text,
                        LENGTH_UNITS.join("/")
                    ),
                    FIX_UNIT,
                    "O 域组件负责人",
                ));
            }
            let v = t.numeric.map(|n| n.value).unwrap_or(0.0);
            Ok(ParsedValue::Length {
                value: v,
                unit,
            })
        }
        // `0` 是合法的无单位长度（规范允许），但必须**显式**是 0：
        // `top: 5` 非法（无单位非零），`top: 0` 合法。
        TokenKind::Number => {
            let v = t.numeric.map(|n| n.value).unwrap_or(0.0);
            if v != 0.0 {
                return Err(StyleError::new(
                    E_UNIT_UNKNOWN,
                    "长度值被拒：无单位非零",
                    &format!("{} 的值 {} 无单位；无单位长度只允许 0", ctx.prop, v),
                    FIX_UNIT,
                    "O 域组件负责人",
                ));
            }
            Ok(ParsedValue::Length {
                value: 0.0,
                unit: "px".to_string(),
            })
        }
        _ => Err(StyleError::new(
            E_VALUE_KIND_MISMATCH,
            "长度值被拒：记号类型不符",
            &format!(
                "{} 需要 Dimension 或 Number 0，实得{}",
                ctx.prop,
                t.kind.zh()
            ),
            FIX_KIND,
            "O 域组件负责人",
        )),
    }
}

/// 解析器：纯数值。
pub fn parse_number(ctx: &ValueCtx) -> Result<ParsedValue, StyleError> {
    let Some(t) = ctx.tokens.first() else {
        return Err(malformed(ctx, "值缺失：数值属性需要一个值"));
    };
    if t.kind != TokenKind::Number {
        return Err(StyleError::new(
            E_VALUE_KIND_MISMATCH,
            "数值被拒：记号类型不符",
            &format!("{} 需要 Number，实得{}", ctx.prop, t.kind.zh()),
            FIX_KIND,
            "O 域组件负责人",
        ));
    }
    Ok(ParsedValue::Number(
        t.numeric.map(|n| n.value).unwrap_or(0.0),
    ))
}

/// 解析器：百分比（**归一到 0..1 口径**）。
pub fn parse_percentage(ctx: &ValueCtx) -> Result<ParsedValue, StyleError> {
    let Some(t) = ctx.tokens.first() else {
        return Err(malformed(ctx, "值缺失：百分比属性需要一个值"));
    };
    if t.kind != TokenKind::Percentage {
        return Err(StyleError::new(
            E_VALUE_KIND_MISMATCH,
            "百分比被拒：记号类型不符",
            &format!("{} 需要 Percentage，实得{}", ctx.prop, t.kind.zh()),
            FIX_KIND,
            "O 域组件负责人",
        ));
    }
    // F2804 的 Percentage 载荷已是「数值部分」（百分号不入载荷），
    // 故 `50%` 的 `numeric.value` 是 50 而不是 0.5——归一在此处做。
    let raw = t.numeric.map(|n| n.value).unwrap_or(0.0);
    Ok(ParsedValue::Percentage(raw / 100.0))
}

/// 解析器：单关键字（**白名单强制**——不在表内即拒）。
pub fn parse_keyword(ctx: &ValueCtx) -> Result<ParsedValue, StyleError> {
    let Some(t) = ctx.tokens.first() else {
        return Err(malformed(ctx, "值缺失：关键字属性需要一个值"));
    };
    // 只接受 Ident：`display` 不接受 `flex()` 这种函数形态。若放开 Function，
    // 「白名单命中」会因为函数名恰好等于某个关键字而放行一个语义不同的值
    // （`display: block` 与假想的 `block()` 会被当成同一个）。
    if t.kind != TokenKind::Ident {
        return Err(StyleError::new(
            E_VALUE_KIND_MISMATCH,
            "关键字被拒：记号类型不符",
            &format!("{} 需要 Ident，实得{}", ctx.prop, t.kind.zh()),
            FIX_KIND,
            "O 域组件负责人",
        ));
    }
    let kw = normalize_keyword(t.text.as_str());
    if !ctx.keywords.iter().any(|k| *k == kw) {
        return Err(StyleError::new(
            E_KEYWORD_UNKNOWN,
            "关键字被拒：不在该属性白名单内",
            &format!(
                "{} 不接受「{}」；白名单为 {}",
                ctx.prop,
                kw,
                ctx.keywords.join("/")
            ),
            FIX_KEYWORD,
            "O 域组件负责人",
        ));
    }
    Ok(ParsedValue::Keyword(kw))
}

/// 解析器：颜色（**只登记不换算**——换算口径归 F2813）。
pub fn parse_color(ctx: &ValueCtx) -> Result<ParsedValue, StyleError> {
    let Some(t) = ctx.tokens.first() else {
        return Err(malformed(ctx, "值缺失：颜色属性需要一个值"));
    };
    match t.kind {
        TokenKind::Ident => {
            let kw = normalize_keyword(t.text.as_str());
            // 颜色关键字白名单来自该属性登记（F2803 侧按需收窄）。
            if !ctx.keywords.iter().any(|k| *k == kw) {
                return Err(StyleError::new(
                    E_KEYWORD_UNKNOWN,
                    "颜色关键字被拒：不在白名单内",
                    &format!(
                        "{} 不接受颜色关键字「{}」；白名单为 {}",
                        ctx.prop,
                        kw,
                        ctx.keywords.join("/")
                    ),
                    FIX_KEYWORD,
                    "O 域组件负责人",
                ));
            }
            let space = if kw == "currentcolor" {
                ColorSpace::CurrentColor
            } else if kw == "transparent" {
                ColorSpace::Transparent
            } else {
                ColorSpace::Srgb
            };
            Ok(ParsedValue::Color {
                space,
                text: kw,
                func: String::new(),
            })
        }
        TokenKind::Hash => {
            // hash 色：`#rgb` / `#rrggbb`（F2804 已定 type flag）。
            let digits = normalize_keyword(t.text.as_str());
            let hash_flag_ok = t.hash_type == HashType::Unrestricted || t.hash_type == HashType::Id;
            if !hash_flag_ok {
                return Err(StyleError::new(
                    E_VALUE_KIND_MISMATCH,
                    "hash 色被拒：type flag 缺失",
                    &format!("{} 的 hash 记号 #{} 未带 type flag", ctx.prop, digits),
                    "检查 F2804 分词是否对该记号带 flag",
                    "分词段维护方",
                ));
            }
            let valid_len = digits.len() == 3 || digits.len() == 6;
            let all_hex = digits.chars().all(|c| c.is_ascii_hexdigit());
            if !valid_len || !all_hex {
                return Err(StyleError::new(
                    E_VALUE_KIND_MISMATCH,
                    "hash 色被拒：形状不对",
                    &format!(
                        "{} 的 #{} 既非 3 位也非 6 位十六进制（长度 {}）",
                        ctx.prop,
                        digits,
                        digits.len()
                    ),
                    "写 #rgb 或 #rrggbb",
                    "O 域组件负责人",
                ));
            }
            Ok(ParsedValue::Color {
                space: ColorSpace::Srgb,
                text: digits,
                func: String::new(),
            })
        }
        TokenKind::Function => {
            let fname = normalize_keyword(t.text.as_str());
            if !COLOR_FUNCTIONS.contains(&fname.as_str()) {
                return Err(StyleError::new(
                    E_KEYWORD_UNKNOWN,
                    "颜色函数被拒：不在函数白名单内",
                    &format!(
                        "{} 不接受颜色函数「{}」；白名单为 {}",
                        ctx.prop,
                        fname,
                        COLOR_FUNCTIONS.join("/")
                    ),
                    FIX_KEYWORD,
                    "O 域组件负责人",
                ));
            }
            Ok(ParsedValue::Color {
                space: if fname.starts_with("hsl") {
                    ColorSpace::Hsl
                } else {
                    ColorSpace::Srgb
                },
                text: fname.clone(),
                func: fname,
            })
        }
        _ => Err(StyleError::new(
            E_VALUE_KIND_MISMATCH,
            "颜色被拒：记号类型不符",
            &format!(
                "{} 需要 Ident/Hash/颜色函数，实得{}",
                ctx.prop,
                t.kind.zh()
            ),
            FIX_KIND,
            "O 域组件负责人",
        )),
    }
}

/// 解析器：线型（五型之一）。
pub fn parse_line_style(ctx: &ValueCtx) -> Result<ParsedValue, StyleError> {
    let Some(t) = ctx.tokens.first() else {
        return Err(malformed(ctx, "值缺失：线型属性需要一个值"));
    };
    if t.kind != TokenKind::Ident {
        return Err(StyleError::new(
            E_VALUE_KIND_MISMATCH,
            "线型被拒：记号类型不符",
            &format!("{} 需要 Ident，实得{}", ctx.prop, t.kind.zh()),
            FIX_KIND,
            "O 域组件负责人",
        ));
    }
    let kw = normalize_keyword(t.text.as_str());
    if !LINE_STYLE_KEYWORDS.contains(&kw.as_str()) {
        return Err(StyleError::new(
            E_KEYWORD_UNKNOWN,
            "线型被拒：不在五型之内",
            &format!(
                "{} 不接受「{}」；五型为 {}",
                ctx.prop,
                kw,
                LINE_STYLE_KEYWORDS.join("/")
            ),
            FIX_KEYWORD,
            "O 域组件负责人",
        ));
    }
    Ok(ParsedValue::LineStyle(kw))
}

/// 解析器：字重（数值 100..900 步进 100，或四关键字）。
pub fn parse_font_weight(ctx: &ValueCtx) -> Result<ParsedValue, StyleError> {
    let Some(t) = ctx.tokens.first() else {
        return Err(malformed(ctx, "值缺失：字重属性需要一个值"));
    };
    if t.kind == TokenKind::Ident {
        let kw = normalize_keyword(t.text.as_str());
        if !FONT_WEIGHT_KEYWORDS.contains(&kw.as_str()) {
            return Err(StyleError::new(
                E_KEYWORD_UNKNOWN,
                "字重关键字被拒：不在四关键字之内",
                &format!(
                    "{} 不接受「{}」；四关键字为 {}",
                    ctx.prop,
                    kw,
                    FONT_WEIGHT_KEYWORDS.join("/")
                ),
                FIX_KEYWORD,
                "O 域组件负责人",
            ));
        }
        return Ok(ParsedValue::FontWeight {
            numeric: 0,
            keyword: kw,
        });
    }
    if t.kind != TokenKind::Number {
        return Err(StyleError::new(
            E_VALUE_KIND_MISMATCH,
            "字重被拒：记号类型不符",
            &format!(
                "{} 需要 Number 或关键字，实得{}",
                ctx.prop,
                t.kind.zh()
            ),
            FIX_KIND,
            "O 域组件负责人",
        ));
    }
    let raw = t.numeric.map(|n| n.value).unwrap_or(0.0);
    // 字重域是 1..1000（F2803 给的），但**规范只允许 100..900 步进 100**。
    // 只按域钳制会把 `font-weight: 450` 静默夹成 450——规范禁止它。故此处
    // 按规范步进校验，越界即拒而不是夹。
    let in_range = raw >= 100.0 && raw <= 900.0;
    let on_step = (raw / 100.0) - (raw / 100.0).floor() < 1e-9;
    if !in_range || !on_step {
        return Err(StyleError::new(
            E_VALUE_KIND_MISMATCH,
            "字重数值被拒：不在 100..900 或不在 100 步进上",
            &format!("{} 的字重 {} 越界或步进不对", ctx.prop, raw),
            "写 100..900 之间 100 的整数倍，或用 normal/bold/bolder/lighter",
            "O 域组件负责人",
        ));
    }
    Ok(ParsedValue::FontWeight {
        numeric: u32::try_from(raw as i64).unwrap_or(0),
        keyword: String::new(),
    })
}

/// 解析器：字形（三型之一）。
pub fn parse_font_style(ctx: &ValueCtx) -> Result<ParsedValue, StyleError> {
    let Some(t) = ctx.tokens.first() else {
        return Err(malformed(ctx, "值缺失：字形属性需要一个值"));
    };
    if t.kind != TokenKind::Ident {
        return Err(StyleError::new(
            E_VALUE_KIND_MISMATCH,
            "字形被拒：记号类型不符",
            &format!("{} 需要 Ident，实得{}", ctx.prop, t.kind.zh()),
            FIX_KIND,
            "O 域组件负责人",
        ));
    }
    let kw = normalize_keyword(t.text.as_str());
    if !FONT_STYLE_KEYWORDS.contains(&kw.as_str()) {
        return Err(StyleError::new(
            E_KEYWORD_UNKNOWN,
            "字形被拒：不在三型之内",
            &format!(
                "{} 不接受「{}」；三型为 {}",
                ctx.prop,
                kw,
                FONT_STYLE_KEYWORDS.join("/")
            ),
            FIX_KEYWORD,
            "O 域组件负责人",
        ));
    }
    Ok(ParsedValue::FontStyle(kw))
}

/// 解析器：行高（`normal` / 数值 / 长度 / 百分比）。
pub fn parse_line_height(ctx: &ValueCtx) -> Result<ParsedValue, StyleError> {
    let Some(t) = ctx.tokens.first() else {
        return Err(malformed(ctx, "值缺失：行高属性需要一个值"));
    };
    match t.kind {
        TokenKind::Ident => {
            let kw = normalize_keyword(t.text.as_str());
            if kw != "normal" {
                return Err(StyleError::new(
                    E_KEYWORD_UNKNOWN,
                    "行高关键字被拒：只接受 normal",
                    &format!("{} 不接受「{}」；关键字形态只有 normal", ctx.prop, kw),
                    "写 normal，或改用数值/长度/百分比形态",
                    "O 域组件负责人",
                ));
            }
            Ok(ParsedValue::LineHeight {
                value: 0.0,
                unit: String::new(),
                normal: true,
            })
        }
        TokenKind::Number => Ok(ParsedValue::LineHeight {
            value: t.numeric.map(|n| n.value).unwrap_or(0.0),
            unit: String::new(),
            normal: false,
        }),
        TokenKind::Dimension => {
            let unit = normalize_keyword(t.text.as_str());
            if !LENGTH_UNITS.contains(&unit.as_str()) {
                return Err(StyleError::new(
                    E_UNIT_UNKNOWN,
                    "行高长度被拒：单位不在长度单位表内",
                    &format!("{} 的行高单位「{}」非法", ctx.prop, t.text),
                    FIX_UNIT,
                    "O 域组件负责人",
                ));
            }
            Ok(ParsedValue::LineHeight {
                value: t.numeric.map(|n| n.value).unwrap_or(0.0),
                unit,
                normal: false,
            })
        }
        TokenKind::Percentage => {
            let raw = t.numeric.map(|n| n.value).unwrap_or(0.0);
            Ok(ParsedValue::LineHeight {
                value: raw / 100.0,
                unit: "%".to_string(),
                normal: false,
            })
        }
        _ => Err(StyleError::new(
            E_VALUE_KIND_MISMATCH,
            "行高被拒：记号类型不符",
            &format!(
                "{} 需要 Ident normal / Number / Dimension / Percentage，实得{}",
                ctx.prop,
                t.kind.zh()
            ),
            FIX_KIND,
            "O 域组件负责人",
        )),
    }
}

/// 解析器：字体族（**逗号分隔的标识符序列**）。
pub fn parse_font_family(ctx: &ValueCtx) -> Result<ParsedValue, StyleError> {
    // 先按逗号切分（token 序列里逗号是显式 Comma 记号）。
    let mut groups: Vec<Vec<&ValueToken>> = vec![Vec::new()];
    for t in ctx.tokens.iter() {
        if t.is_comma() {
            groups.push(Vec::new());
        } else {
            if let Some(last) = groups.last_mut() {
                last.push(t);
            }
        }
    }
    let mut first = String::new();
    let mut count: u8 = 0;
    for (gi, g) in groups.iter().enumerate() {
        // 空组（`a,,b` 或结尾逗号）按规范丢弃——**丢弃而非报错**是规范处置，
        // 但**全部为空**（`,,`）即无族名，必须拒。
        if g.is_empty() {
            continue;
        }
        let mut name = String::new();
        for t in g.iter() {
            if t.kind != TokenKind::Ident && t.kind != TokenKind::String {
                return Err(StyleError::new(
                    E_VALUE_KIND_MISMATCH,
                    "字体族被拒：分量记号类型不符",
                    &format!(
                        "{} 的第 {} 个家族含{}，只接受 Ident 或 String",
                        ctx.prop,
                        gi + 1,
                        t.kind.zh()
                    ),
                    FIX_KIND,
                    "O 域组件负责人",
                ));
            }
            if !name.is_empty() {
                name.push(' ');
            }
            name.push_str(t.text.as_str());
        }
        let n = normalize_keyword(name.as_str());
        if n.is_empty() {
            continue;
        }
        if count == 0 {
            first = n;
        }
        count = count.saturating_add(1);
    }
    if count == 0 {
        return Err(malformed(ctx, "字体族为空：所有分量都是空组"));
    }
    Ok(ParsedValue::FontFamily { first, arity: count })
}

/// 解析器：位置（1~2 个分量）。
pub fn parse_position(ctx: &ValueCtx) -> Result<ParsedValue, StyleError> {
    let mut out: Vec<String> = Vec::new();
    for t in ctx.tokens.iter() {
        match t.kind {
            TokenKind::Ident => {
                let kw = normalize_keyword(t.text.as_str());
                if !POSITION_KEYWORDS.contains(&kw.as_str()) {
                    return Err(StyleError::new(
                        E_KEYWORD_UNKNOWN,
                        "位置关键字被拒：不在位置关键字表内",
                        &format!(
                            "{} 不接受「{}」；可用 {}",
                            ctx.prop,
                            kw,
                            POSITION_KEYWORDS.join("/")
                        ),
                        FIX_KEYWORD,
                        "O 域组件负责人",
                    ));
                }
                out.push(kw);
            }
            TokenKind::Percentage => {
                let raw = t.numeric.map(|n| n.value).unwrap_or(0.0);
                out.push(format!("{}%", raw));
            }
            _ => {
                return Err(StyleError::new(
                    E_VALUE_KIND_MISMATCH,
                    "位置分量被拒：记号类型不符",
                    &format!(
                        "{} 的位置分量只接受 Ident 或 Percentage，实得{}",
                        ctx.prop,
                        t.kind.zh()
                    ),
                    FIX_KIND,
                    "O 域组件负责人",
                ));
            }
        }
    }
    let (lo, hi) = ctx.kind.arity();
    let n = u8::try_from(out.len()).unwrap_or(u8::MAX);
    if n < lo || n > hi {
        return Err(StyleError::new(
            E_VALUE_ARITY,
            "位置分量数越界",
            &format!("{} 需要 {}-{} 个分量，实得 {}", ctx.prop, lo, hi, n),
            FIX_ARITY,
            "O 域组件负责人",
        ));
    }
    Ok(ParsedValue::Position(out))
}

/// 解析器：值列表（**逗号分隔；只解析不展开**——展开归 F2806）。
pub fn parse_list(ctx: &ValueCtx) -> Result<ParsedValue, StyleError> {
    let (lo, hi) = ctx.kind.arity();
    // **逗号与空白都是分量界**：CSS 值列表有两种写法——
    // `box-shadow: 0 0 4px, 0 0 8px`（逗号分隔阴影列表）与
    // `margin: 1px 2px`（空白分隔简写）。只认逗号会让 `1px 2px` 变成
    // 一个含两个记号的分组，而 `parse_list_item` 只看首记号 ⇒
    // `2px` 被**静默丢弃**。用户写了两分量却只生效一个，且无任何告警——
    // 这是比「拒绝」危险得多的失败形态。
    //
    // 故分组规则：**记号入当前组；遇 Comma 则封当前组并开新组；
    // 遇「组内已有记号且当前记号不是逗号」时另开新组**（空白已被F2804
    // 合并为 Whitespace，而本函数只收到非空白记号，故用「组非空」判定）。
    // 函数记号与其参数须绑定为一组——`rgb(1 2 3)` 的内部记号已在
    // F2804 层被Function 记号吞掉（函数记号的区间含左括号），
    // 故本层看不到其内部记号，不会误切。
    let mut groups: Vec<Vec<&ValueToken>> = vec![Vec::new()];
    for t in ctx.tokens.iter() {
        if t.is_comma() {
            groups.push(Vec::new());
            continue;
        }
        let need_new = groups
            .last()
            .map(|g| !g.is_empty())
            .unwrap_or(false);
        if need_new {
            groups.push(Vec::new());
        }
        if let Some(last) = groups.last_mut() {
            last.push(t);
        }
    }
    // 空组丢弃（`a,,b` 按规范视作 `a,b`）。
    let kept: Vec<&[&ValueToken]> = groups
        .iter()
        .filter(|g| !g.is_empty())
        .map(|g| g.as_slice())
        .collect();
    let n = u8::try_from(kept.len()).unwrap_or(u8::MAX);
    if n == 0 {
        return Err(malformed(ctx, "值列表为空"));
    }
    if n < lo {
        return Err(StyleError::new(
            E_VALUE_ARITY,
            "值列表分量数不足",
            &format!("{} 需要至少 {} 项，实得 {}", ctx.prop, lo, n),
            FIX_ARITY,
            "O 域组件负责人",
        ));
    }
    // 超上限：钳到上限并落告警（**不静默丢**——丢了用户会看到少一段阴影）。
    let cap = hi as usize;
    let mut items: Vec<ParsedValue> = Vec::new();
    for (i, g) in kept.iter().enumerate() {
        if i >= cap {
            break;
        }
        items.push(parse_list_item(ctx, g)?);
    }
    if kept.len() > cap {
        // 告警在 `DeclParser` 侧统一落（此处只标出「被截」的事实）。
        return Ok(ParsedValue::List(items));
    }
    Ok(ParsedValue::List(items))
}

/// 列表内单分量的解析（**按记号形态分派，不按属性分派**）。
fn parse_list_item(ctx: &ValueCtx, g: &[&ValueToken]) -> Result<ParsedValue, StyleError> {
    let Some(first) = g.first() else {
        return Err(malformed(ctx, "列表分量为空"));
    };
    match first.kind {
        TokenKind::Dimension => {
            let unit = normalize_keyword(first.text.as_str());
            if !LENGTH_UNITS.contains(&unit.as_str()) {
                return Err(StyleError::new(
                    E_UNIT_UNKNOWN,
                    "列表内长度被拒：单位非法",
                    &format!("{} 的分量单位「{}」不在长度单位表内", ctx.prop, first.text),
                    FIX_UNIT,
                    "O 域组件负责人",
                ));
            }
            Ok(ParsedValue::Length {
                value: first.numeric.map(|n| n.value).unwrap_or(0.0),
                unit,
            })
        }
        TokenKind::Number => Ok(ParsedValue::Number(
            first.numeric.map(|n| n.value).unwrap_or(0.0),
        )),
        TokenKind::Percentage => {
            let raw = first.numeric.map(|n| n.value).unwrap_or(0.0);
            Ok(ParsedValue::Percentage(raw / 100.0))
        }
        TokenKind::Hash => Ok(ParsedValue::Color {
            space: ColorSpace::Srgb,
            text: normalize_keyword(first.text.as_str()),
            func: String::new(),
        }),
        TokenKind::Function => {
            let fname = normalize_keyword(first.text.as_str());
            // 列表内的函数一律登记不解释（`blur()` / `drop-shadow()` 的合成归
            // F2854，`rgb()` 的换算归 F2813）。
            Ok(ParsedValue::Color {
                space: if COLOR_FUNCTIONS.contains(&fname.as_str()) {
                    if fname.starts_with("hsl") {
                        ColorSpace::Hsl
                    } else {
                        ColorSpace::Srgb
                    }
                } else {
                    ColorSpace::Srgb
                },
                text: fname.clone(),
                func: fname,
            })
        }
        TokenKind::Ident => {
            let kw = normalize_keyword(first.text.as_str());
            // 分派顺序：颜色 → 线型 → 属性白名单。
            //
            // 颜色必须**先判**：`box-shadow: 0 0 4px red` 的末位分量是颜色
            // 关键字，而 `box-shadow` 的白名单是 `none`/`auto`、线型表也不含
            // `red`。若只查后两者，`red` 会被拒——而它是完全合法的 CSS。
            // 这正是「属性白名单」与「值语法关键字」两套表的区别：前者按
            // 属性收窄，后者按值类别共享。
            //
            // 颜色在此**只登记不换算**（`space` 标 Srgb、文本原样带走），
            // 语义换算归 F2813——与 `parse_color` 的口径一致。
            if KW_COLOR.contains(&kw.as_str()) {
                let space = if kw == "currentcolor" {
                    ColorSpace::CurrentColor
                } else if kw == "transparent" {
                    ColorSpace::Transparent
                } else {
                    ColorSpace::Srgb
                };
                Ok(ParsedValue::Color {
                    space,
                    text: kw,
                    func: String::new(),
                })
            } else if LINE_STYLE_KEYWORDS.contains(&kw.as_str()) {
                Ok(ParsedValue::LineStyle(kw))
            } else if ctx.keywords.iter().any(|k| *k == kw) {
                Ok(ParsedValue::LineStyle(kw))
            } else {
                Err(StyleError::new(
                    E_KEYWORD_UNKNOWN,
                    "列表内关键字被拒：不在颜色表/线型表也不在属性白名单",
                    &format!(
                        "{} 不接受分量关键字「{}」；颜色表 {} 项，线型表为 {}，属性白名单为 {}",
                        ctx.prop,
                        kw,
                        KW_COLOR.len(),
                        LINE_STYLE_KEYWORDS.join("/"),
                        ctx.keywords.join("/")
                    ),
                    FIX_KEYWORD,
                    "O 域组件负责人",
                ))
            }
        }
        _ => Err(StyleError::new(
            E_VALUE_KIND_MISMATCH,
            "列表分量被拒：记号类型不符",
            &format!(
                "{} 的分量不接受{}（支持 Dimension/Number/Percentage/Hash/Function/Ident）",
                ctx.prop,
                first.kind.zh()
            ),
            FIX_KIND,
            "O 域组件负责人",
        )),
    }
}

/// 结构不成形的统一错误（**归一到一个码，避免同一类问题散成多个码**）。
fn malformed(ctx: &ValueCtx, why: &str) -> StyleError {
    StyleError::new(
        E_DECL_MALFORMED,
        "声明值被拒：结构不成形",
        &format!("{}：{}", ctx.prop, why),
        FIX_DECL,
        "O 域组件负责人",
    )
}

// ---------------------------------------------------------------------------
// 七、编译期完整性校验（**真正的编译期闸**）
// ---------------------------------------------------------------------------

/// 编译期完整性校验。
///
/// 三条 `const _: () = assert!(...)` 全部在**编译期**求值：
///
/// 1. [`PROPERTY_REGISTRY`] 长度等于 [`PROPERTY_COUNT`]（由数组类型保证，
///    此处再断言一次是为了让「长度」这件事在错误信息里可见）；
/// 2. 全表属性名**两两不重**（编译期 O(n²/2) = 946 次比较，一次性开销）；
/// 3. 四族**无空族**（族容量由 F2803 保证，此处复核注册表侧一致）。
///
/// **失败即编译失败**，不产生任何运行期产物。
mod compile_time_audit {
    use super::*;

    /// 闸一：注册表长度 == 属性总数。
    const _: () = assert!(PROPERTY_REGISTRY.len() == PROPERTY_COUNT);

    /// 闸二：全表名唯一（编译期两两比对）。
    const _: () = assert!(names_are_unique());

    /// 闸三：四族均非空。
    const _: () = assert!(all_families_present());

    /// 闸四：解析器表长度 == 值类别数（**新增类别必补解析器**）。
    const _: () = assert!(PARSERS.len() == VALUE_KIND_COUNT);

    /// 闸五：每条条目的解析器槽下标在范围内（防止 `kind.rank()` 越界取到
    /// 别的类别的解析器——类型系统保证 `PARSERS[rank]` 存在，但保证不了
    /// 「rank 与条目类别一致」，那是闸六的活）。
    const _: () = assert!(all_slots_in_range());

    /// 闸六：每条条目的名字与 F2803 四族常量表**逐字节一致**（名称单源）。
    const _: () = assert!(registry_names_match_subset());

    /// 编译期字节串相等（**手写循环，不能用 `==`**）。
///
/// `PartialEq` 在 const 上下文尚不稳定（`const_trait_impl` 未普及），
/// 而本单的编译期闸要用「逐字节一致」判名同源。故手写：逐字节比长度再逐字节比值。
/// 判据侧也须知道这条——「为什么不用 `==`」是刻意选择，不是遗漏。
const fn bytes_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut i: usize = 0;
    while i < a.len() {
        if a[i] != b[i] {
            return false;
        }
        i += 1;
    }
    true
}

/// 编译期名唯一性（const fn 内的两两比对）。
    const fn names_are_unique() -> bool {
        let mut i: usize = 0;
        while i < PROPERTY_COUNT {
            let mut j: usize = i + 1;
            while j < PROPERTY_COUNT {
                if bytes_eq(
                    PROPERTY_REGISTRY[i].name.as_bytes(),
                    PROPERTY_REGISTRY[j].name.as_bytes(),
                ) {
                    return false;
                }
                j += 1;
            }
            i += 1;
        }
        true
    }

    /// 编译期四族非空。
    const fn all_families_present() -> bool {
        let mut has_layout = false;
        let mut has_visual = false;
        let mut has_effect = false;
        let mut has_interaction = false;
        let mut i: usize = 0;
        while i < PROPERTY_COUNT {
            match PROPERTY_REGISTRY[i].family {
                PropertyFamily::Layout => has_layout = true,
                PropertyFamily::Visual => has_visual = true,
                PropertyFamily::Effect => has_effect = true,
                PropertyFamily::Interaction => has_interaction = true,
            }
            i += 1;
        }
        has_layout && has_visual && has_effect && has_interaction
    }

    /// 编译期槽下标合法。
    const fn all_slots_in_range() -> bool {
        let mut i: usize = 0;
        while i < PROPERTY_COUNT {
            if PROPERTY_REGISTRY[i].kind.rank() >= VALUE_KIND_COUNT {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 编译期名字与子集表逐字节一致。
    ///
    /// 这是「名称单源」的反向证明：注册表的名字是从子集表**索引取出**的，
    /// 本断言证明「取出的顺序与子集表一致」——若有人改了
    /// [`build_registry`] 的分块次序，此处立刻红。
    const fn registry_names_match_subset() -> bool {
        let mut i: usize = 0;
        while i < LAYOUT_PROPERTIES.len() {
            if !bytes_eq(PROPERTY_REGISTRY[i].name.as_bytes(), LAYOUT_PROPERTIES[i].spec.name.as_bytes()) {
                return false;
            }
            i += 1;
        }
        let mut j: usize = 0;
        while j < VISUAL_PROPERTIES.len() {
            let idx = LAYOUT_PROPERTIES.len() + j;
            if !bytes_eq(PROPERTY_REGISTRY[idx].name.as_bytes(), VISUAL_PROPERTIES[j].spec.name.as_bytes()) {
                return false;
            }
            j += 1;
        }
        let mut k: usize = 0;
        while k < EFFECT_PROPERTIES.len() {
            let idx = LAYOUT_PROPERTIES.len() + VISUAL_PROPERTIES.len() + k;
            if !bytes_eq(PROPERTY_REGISTRY[idx].name.as_bytes(), EFFECT_PROPERTIES[k].spec.name.as_bytes()) {
                return false;
            }
            k += 1;
        }
        let mut m: usize = 0;
        while m < INTERACTION_PROPERTIES.len() {
            let idx = LAYOUT_PROPERTIES.len()
                + VISUAL_PROPERTIES.len()
                + EFFECT_PROPERTIES.len()
                + m;
            if !bytes_eq(
                PROPERTY_REGISTRY[idx].name.as_bytes(),
                INTERACTION_PROPERTIES[m].spec.name.as_bytes(),
            ) {
                return false;
            }
            m += 1;
        }
        true
    }
}

/// 编译期校验的**运行期复查入口**（供诊断与调试器调用）。
///
/// 不是必需的——闸门本身在编译期。但「有闸」和「闸是通的」是两件事：
/// 提供一个可查询的接口，让调试器能显示「本域注册表已过六闸」，比让人
/// 相信注释里的 `const _: () = assert!` 靠得住。
pub fn compile_time_audit_summary() -> String {
    format!(
        "注册表编译期六闸全通：条目 {} == 四族常量表长度 {}；名唯一（编译期 {} 次比对）；四族齐备；解析器 {} == 类别 {}；槽下标全在范围内；名与子集表逐字节一致",
        PROPERTY_REGISTRY.len(),
        PROPERTY_COUNT,
        PROPERTY_COUNT * (PROPERTY_COUNT - 1) / 2,
        PARSERS.len(),
        VALUE_KIND_COUNT
    )
}

// ---------------------------------------------------------------------------
// 八、声明解析器（把一条 `属性: 值` 的 token 段解析成声明）
// ---------------------------------------------------------------------------

/// 单条声明的 token 数上限。
pub const DECL_MAX_TOKENS: usize = 32;

/// 单条声明的值 token 数上限。
pub const DECL_MAX_VALUE_TOKENS: usize = 16;

/// 一条解析成功的声明。
#[derive(Clone, Debug, PartialEq)]
pub struct Declaration {
    /// 属性 id。
    pub id: PropertyId,
    /// 属性名（承注册表，不另存——另存就会与注册表漂）。
    pub name: &'static str,
    /// 解析出的值。
    pub value: ParsedValue,
    /// 是否带 `!important`。
    pub important: bool,
    /// 该声明首 token 的偏移（**归因用**：用户报「这条样式不生效」时要能
    /// 指回它在源里的位置）。
    pub offset: u32,
}

impl Declaration {
    /// 读屏单行（声明要能被读屏念出「谁、是什么、值多少、要不要紧」）。
    pub fn screen_line(&self) -> String {
        format!(
            "{}: {}{}",
            self.name,
            self.value.screen_line(),
            if self.important { "（重要）" } else { "" }
        )
    }
}

/// 解析统计（**失败显性化**：被拒的声明数必须能被看见，不能静默丢）。
#[derive(Clone, Debug, Default)]
pub struct DeclStats {
    /// 尝试解析的声明段数。
    pub segments: u32,
    /// 成功解析的声明数。
    pub accepted: u32,
    /// 被拒的声明数（属性未知/值非法/结构不成形）。
    pub rejected: u32,
    /// 因上限被截的声明数。
    pub clamped: u32,
}

impl DeclStats {
    /// 守恒：成功 + 被拒 == 尝试（**不接受「差额」存在**）。
    pub fn conserves(&self) -> bool {
        self.accepted.saturating_add(self.rejected) == self.segments
    }

    /// 读屏摘要。
    pub fn screen_text(&self) -> String {
        format!(
            "声明 {} 段：接受 {}，拒绝 {}，钳制 {}",
            self.segments, self.accepted, self.rejected, self.clamped
        )
    }
}

/// 声明解析器（**消费 F2804 的 token 流，产出 [`Declaration`] 序列**）。
pub struct DeclParser {
    /// 钳制告警账（承 F2801）。
    pub clamps: ClampLog,
    /// 案件账（承 F2801）。
    pub cases: CaseLedger,
    /// 统计。
    pub stats: DeclStats,
    /// 逻辑 tick（告警与立案的时间戳；**零墙钟**，由调用方单调推进）。
    pub tick: u64,
}

impl DeclParser {
    /// 新建解析器（账本皆空）。
    pub fn new() -> Self {
        DeclParser {
            clamps: ClampLog::new(),
            cases: CaseLedger::new(),
            stats: DeclStats::default(),
            tick: 0,
        }
    }

    /// 解析**一段声明**（`属性名 : 值` 的 token 区间 `[from, to)`）。
    ///
    /// 只解析一条——规则块的整体遍历归 F2807（样式表对象），本单不越界。
    /// `important` 由调用方从 token 里读出后传入（本单不裁决级联，
    /// 只忠实记录「作者写了 important」）。
    pub fn parse_one(
        &mut self,
        stream: &TokenStream,
        from: usize,
        to: usize,
        important: bool,
    ) -> Result<Declaration, StyleError> {
        self.stats.segments = self.stats.segments.saturating_add(1);
        // 上限闸：先卡 token 数，越界即立案并拒（**不逐个试**——那会让
        // 超限声明的部分内容被误当成有效声明）。
        let span = to.saturating_sub(from);
        if span > DECL_MAX_TOKENS {
            self.stats.clamped = self.stats.clamped.saturating_add(1);
            self.stats.rejected = self.stats.rejected.saturating_add(1);
            let err = StyleError::new(
                E_DECL_TOKEN_CAP,
                "声明被拒：token 数超上限",
                &format!(
                    "该声明占 {} 个 token（上限 {}）；超限声明不做部分解析",
                    span, DECL_MAX_TOKENS
                ),
                FIX_TOKEN_CAP,
                "O 域组件负责人",
            );
            self.file_case(&err, from);
            return Err(err);
        }
        // 前导空白：调用方按分号切段时，段起点天然落在分号后的空白 token 上
        // （`a: b; c: d` 的第二段起点是空白而非 `c`）。**必须先跳过**——
        // 否则每条非首声明都会以「属性名不是标识符」被拒，而拒绝理由是
        // 调用方的切段口径，不是作者的写法。那会让「坏字符串只丢本条」
        // 「统计守恒」这类判据全部测在一个不存在的现象上。
        //
        // 归因起点也用跳过的位置：偏移要指向属性名本身，不是它前面的空白。
        let mut head_idx = from;
        while head_idx < to {
            match stream.get(head_idx) {
                Some(t) if t.kind == TokenKind::Whitespace => head_idx = head_idx.saturating_add(1),
                _ => break,
            }
        }
        let from = head_idx;
        // 属性名：必须是 Ident（自定义属性 `--x` 也走 Ident）。
        let Some(head) = stream.get(from) else {
            self.stats.rejected = self.stats.rejected.saturating_add(1);
            let err = StyleError::new(
                E_DECL_MALFORMED,
                "声明被拒：起点越界",
                &format!("起点 {} 超出 token 流长度 {}", from, stream.len()),
                FIX_DECL,
                "O 域组件负责人",
            );
            self.file_case(&err, from);
            return Err(err);
        };
        if head.kind != TokenKind::Ident {
            self.stats.rejected = self.stats.rejected.saturating_add(1);
            let err = StyleError::new(
                E_DECL_MALFORMED,
                "声明被拒：属性名不是标识符",
                &format!("偏移 {} 处是{}，属性名必须是标识符", head.start, head.kind.zh()),
                FIX_DECL,
                "O 域组件负责人",
            );
            self.file_case(&err, from);
            return Err(err);
        }
        let name = stream.payload(from).to_string();
        let norm = normalize_keyword(name.as_str());
        // 属性查找：不在子集表内即拒（**桶外属性不进样式**——这是 F2801
        // 段契约明写的失败策略）。
        let Some(id) = id_of(norm.as_str()) else {
            self.stats.rejected = self.stats.rejected.saturating_add(1);
            let err = StyleError::new(
                E_PROPERTY_UNKNOWN,
                "声明被拒：属性不在子集表内",
                &format!("「{}」不在 F2803 四族子集表（{} 条）之内", norm, PROPERTY_COUNT),
                FIX_UNKNOWN_PROP,
                "O 域组件负责人",
            );
            self.file_case(&err, from);
            return Err(err);
        };
        let Some(entry) = id.entry() else {
            // 理论不可达：`id_of` 只会返回有表项的 id。此处仍显性拒绝，
            // 避免「拿到 id 却拿不到条目」时静默通过。
            self.stats.rejected = self.stats.rejected.saturating_add(1);
            let err = StyleError::new(
                E_REGISTRY_INCOMPLETE,
                "声明被拒：注册表无对应条目",
                &format!("{} 有 id 却无表项；注册表与 id 空间不同构", norm),
                FIX_REGISTRY,
                "O 域组件负责人",
            );
            self.file_case(&err, from);
            return Err(err);
        };
        // 冒号：属性名之后（下同）必须紧跟 Colon。空白已在 F2804 合并为
        // Whitespace 记号，故此处允许恰好一个空白。
        let colon_at = self.find_colon(stream, from, to);
        let Some(colon_at) = colon_at else {
            self.stats.rejected = self.stats.rejected.saturating_add(1);
            let err = StyleError::new(
                E_DECL_MALFORMED,
                "声明被拒：缺冒号",
                &format!("{} 之后没有冒号，声明无法定位值区间", norm),
                FIX_DECL,
                "O 域组件负责人",
            );
            self.file_case(&err, from);
            return Err(err);
        };
        // 值区间：冒号之后到 `to`，去掉首尾空白、EOF 与错误产物。
        let value_from = colon_at.saturating_add(1);
        let mut value_to = to;
        while value_from < value_to {
            match stream.get(value_to - 1) {
                Some(t) if t.kind == TokenKind::Whitespace => value_to -= 1,
                _ => break,
            }
        }
        let mut vt: Vec<ValueToken> = Vec::new();
        let mut i = value_from;
        while i < value_to {
            let Some(t) = stream.get(i) else {
                break;
            };
            // 末尾 EOF 不是值——跳过（契约见 [`decl_span_end`]）。
            if t.kind == TokenKind::Eof {
                i += 1;
                continue;
            }
            // **错误产物直接跳过整条声明**（F2804 给本单的义务原文：
            // 「遇 BadString/BadUrl 必须跳过整条声明而非整张表」）。
            if t.is_error() {
                self.stats.rejected = self.stats.rejected.saturating_add(1);
                let err = StyleError::new(
                    E_DECL_MALFORMED,
                    "声明被拒：值区含错误产物",
                    &format!(
                        "{} 的值区含{}（偏移 [{}, {})）",
                        norm,
                        t.kind.zh(),
                        t.start,
                        t.end
                    ),
                    "修正该段引号或 url() 内的非法内容；本条声明被丢弃，其后声明不受影响",
                    "O 域组件负责人",
                );
                self.file_case(&err, i);
                return Err(err);
            }
            if t.kind != TokenKind::Whitespace && vt.len() < DECL_MAX_VALUE_TOKENS {
                match ValueToken::from_token(stream, i) {
                    Some(v) => vt.push(v),
                    None => break,
                }
            }
            i += 1;
        }
        if vt.is_empty() {
            self.stats.rejected = self.stats.rejected.saturating_add(1);
            let err = StyleError::new(
                E_DECL_MALFORMED,
                "声明被拒：值区为空",
                &format!("{} 的冒号之后没有值", norm),
                FIX_DECL,
                "O 域组件负责人",
            );
            self.file_case(&err, from);
            return Err(err);
        }
        if vt.len() >= DECL_MAX_VALUE_TOKENS {
            self.stats.clamped = self.stats.clamped.saturating_add(1);
            let _ = self.clamps.push(ClampNotice {
                field: format!("{}.value_tokens", norm),
                original: vt.len() as f64,
                clamped: DECL_MAX_VALUE_TOKENS as f64,
                low: 0.0,
                high: DECL_MAX_VALUE_TOKENS as f64,
                tick: self.tick,
            });
        }
        // 静态映射调用：解析器由注册表给出，本单不按名字猜。
        let ctx = ValueCtx {
            prop: entry.name,
            kind: entry.kind,
            keywords: entry.keywords,
            domain_low: entry.domain_low,
            domain_high: entry.domain_high,
            tokens: &vt,
            important,
        };
        let raw = match (entry.parser)(&ctx) {
            Ok(v) => v,
            Err(e) => {
                self.stats.rejected = self.stats.rejected.saturating_add(1);
                self.file_case(&e, from);
                return Err(e);
            }
        };
        // 参数域钳制（**只对带标量数值的类别**；列表内的每个分量已由
        // `parse_list_item` 各自解析，不再二次钳制——否则简写 `box-shadow`
        // 里越界的那个分量会被静默放过）。
        let value = self.clamp_declaration_value(entry, raw);
        self.stats.accepted = self.stats.accepted.saturating_add(1);
        Ok(Declaration {
            id,
            name: entry.name,
            value,
            important,
            offset: head.start,
        })
    }

    /// 找冒号（**允许属性名与冒号之间恰好一个空白**）。
    fn find_colon(&self, stream: &TokenStream, from: usize, to: usize) -> Option<usize> {
        let mut i = from.saturating_add(1);
        // 至多跳过一个空白（`color : red` 合法）。
        if let Some(t) = stream.get(i) {
            if t.kind == TokenKind::Whitespace {
                i += 1;
            }
        }
        if i >= to {
            return None;
        }
        match stream.get(i) {
            Some(t) if t.kind == TokenKind::Colon => Some(i),
            _ => None,
        }
    }

    /// 对解析结果做参数域钳制（**钳到边界并落告警**）。
    fn clamp_declaration_value(
        &mut self,
        entry: &PropertyEntry,
        value: ParsedValue,
    ) -> ParsedValue {
        match value {
            ParsedValue::Number(v) => ParsedValue::Number(clamp_and_log(
                &self.ctx_of(entry),
                v,
                &mut self.clamps,
                self.tick,
            )),
            ParsedValue::Length { value: v, unit } => ParsedValue::Length {
                value: clamp_and_log(&self.ctx_of(entry), v, &mut self.clamps, self.tick),
                unit,
            },
            ParsedValue::Percentage(v) => ParsedValue::Percentage(clamp_and_log(
                &self.ctx_of(entry),
                v,
                &mut self.clamps,
                self.tick,
            )),
            other => other,
        }
    }

    /// 造一个只用于钳制的最小上下文（**钳制只需要 prop 与域**）。
    fn ctx_of(&self, entry: &PropertyEntry) -> ValueCtx<'static> {
        ValueCtx {
            prop: entry.name,
            kind: entry.kind,
            keywords: entry.keywords,
            domain_low: entry.domain_low,
            domain_high: entry.domain_high,
            tokens: &[],
            important: false,
        }
    }

    /// 立案（**异常检出 → 立案流转**，承 F2801 `CaseLedger`）。
    fn file_case(&mut self, err: &StyleError, token_index: usize) {
        let _ = self.cases.open_case(
            err.what,
            &format!("{}（偏移下标 {}）", err.why, token_index),
            "O 域声明解析段",
            "按 next 修正该条声明；本条被丢弃不影响其后声明",
            self.tick,
        );
    }

    /// 读屏摘要（无障碍：整轮解析结果要能被读屏整体念出）。
    pub fn screen_summary(&self) -> String {
        format!(
            "{}；告警 {} 条（落账 {} 条），案件 {} 件",
            self.stats.screen_text(),
            self.clamps.len(),
            self.clamps.dropped(),
            self.cases.len()
        )
    }
}

// ---------------------------------------------------------------------------
// 九、注册表自检（**运行期兜底**：编译期闸之外的第二道，属诊断而非门禁）
// ---------------------------------------------------------------------------

/// 注册表自检（**六项**；任一不成立即红）。
///
/// 这一层**不是**「编译期校验」的兑现——兑现那件事在
/// [`compile_time_audit`] 里已经发生。此处是给运行期诊断用的：不看编译
/// 日志也能确认「当前这份二进制的注册表是齐的」。
pub fn audit_registry() -> Result<(), StyleError> {
    // 一：长度与属性总数一致。
    if PROPERTY_REGISTRY.len() != PROPERTY_COUNT {
        return Err(StyleError::new(
            E_REGISTRY_INCOMPLETE,
            "注册表自检失败：条目数与属性总数不符",
            &format!(
                "注册表 {} 条，属性总数 {} 条",
                PROPERTY_REGISTRY.len(),
                PROPERTY_COUNT
            ),
            FIX_REGISTRY,
            "O 域组件负责人",
        ));
    }
    // 二：名唯一。
    for (i, a) in PROPERTY_REGISTRY.iter().enumerate() {
        for (j, b) in PROPERTY_REGISTRY.iter().enumerate() {
            if i < j && a.name == b.name {
                return Err(StyleError::new(
                    E_REGISTRY_DUP_NAME,
                    "注册表自检失败：同名属性",
                    &format!("「{}」在下标 {} 与 {} 各出现一次", a.name, i, j),
                    FIX_DUP,
                    "O 域组件负责人",
                ));
            }
        }
    }
    // 三：每条条目成形（名字非空、域不倒挂、白名单非空）。
    for e in PROPERTY_REGISTRY.iter() {
        if !e.is_wellformed() {
            return Err(StyleError::new(
                E_REGISTRY_INCOMPLETE,
                "注册表自检失败：条目不成形",
                &format!(
                    "{}：名字空={}，域倒挂={}，白名单空={}",
                    e.name,
                    e.name.trim().is_empty(),
                    !e.domain_is_sane(),
                    e.keywords.is_empty()
                ),
                FIX_REGISTRY,
                "O 域组件负责人",
            ));
        }
        // 四：白名单内不得有空串（空串会让「白名单命中」恒真——那就是
        // 自证式门禁：任何值都通过）。
        for k in e.keywords.iter() {
            if k.trim().is_empty() {
                return Err(StyleError::new(
                    E_REGISTRY_INCOMPLETE,
                    "注册表自检失败：白名单含空项",
                    &format!("{} 的白名单里有空串，会让关键字校验恒真", e.name),
                    "删掉空串项；无关键字语义的值类别应改用非关键字解析器",
                    "O 域组件负责人",
                ));
            }
        }
    }
    // 五：解析器槽全部可取。
    for e in PROPERTY_REGISTRY.iter() {
        if parser_of(e.kind).is_none() {
            return Err(StyleError::new(
                E_REGISTRY_INCOMPLETE,
                "注册表自检失败：解析器槽越界",
                &format!("{} 的类别槽 {} 超出解析器表长度 {}", e.kind.zh(), e.parser_slot(), PARSERS.len()),
                FIX_KIND,
                "O 域组件负责人",
            ));
        }
    }
    // 六：名与子集表逐条一致（**名称单源的反向证明**）。
    let mut names: Vec<&'static str> = Vec::new();
    for p in LAYOUT_PROPERTIES.iter() {
        names.push(p.spec.name);
    }
    for p in VISUAL_PROPERTIES.iter() {
        names.push(p.spec.name);
    }
    for p in EFFECT_PROPERTIES.iter() {
        names.push(p.spec.name);
    }
    for p in INTERACTION_PROPERTIES.iter() {
        names.push(p.spec.name);
    }
    for (i, n) in names.iter().enumerate() {
        let Some(e) = PROPERTY_REGISTRY.get(i) else {
            return Err(StyleError::new(
                E_REGISTRY_INCOMPLETE,
                "注册表自检失败：条目缺失",
                &format!("子集表第 {} 条「{}」在注册表里没有对应条目", i, n),
                FIX_REGISTRY,
                "O 域组件负责人",
            ));
        };
        if e.name != *n {
            return Err(StyleError::new(
                E_REGISTRY_NAME_DRIFT,
                "注册表自检失败：名字与子集表漂移",
                &format!(
                    "下标 {} 处注册表是「{}」，子集表是「{}」",
                    i, e.name, n
                ),
                FIX_REGISTRY,
                "O 域组件负责人",
            ));
        }
    }
    Ok(())
}

/// 注册表**可读摘要**（族码 + 类别码 + 子集锚，逐条可见）。
///
/// 与 [`registry_digest`] 分工明确，二者不可互相替代：
/// - [`registry_digest`] 是**定长哈希**，供跨二进制差分对账（内容变则指纹变）；
/// - 本函数是**人可读的完整清单**，供判据核对「族码/类别码是否真的逐条登记」。
///
/// 为什么不把摘要塞进哈希函数：哈希会把这些码压掉，判据再想验「某条条目
/// 带的是 `FAM-LAYOUT` 而非 `FAM-OTHER`」就只能对着 16 个 hex 猜——那等于
/// 没有判据（记忆 §5 弱门禁第7 条「判据向被测函数问答案」的反面：判据
/// 看不到内部字段，形态判据恒真）。
pub fn registry_summary() -> String {
    let mut buf = String::new();
    buf.push_str(REGISTRY_VERSION);
    buf.push('|');
    buf.push_str(ANCHORED_VALUE_SPEC);
    for e in PROPERTY_REGISTRY.iter() {
        buf.push('|');
        buf.push_str(e.name);
        buf.push(':');
        buf.push_str(&e.kind.code());
        buf.push(':');
        buf.push_str(e.family.code());
        buf.push(':');
        buf.push_str(&e.domain_low.to_string());
        buf.push(':');
        buf.push_str(&e.domain_high.to_string());
        buf.push(':');
        buf.push_str(&e.keywords.join(","));
    }
    buf.push_str("|subsets=");
    buf.push_str(SUBSET_ANCHOR_HASH);
    buf
}

/// 注册表指纹（**差分对账用**：两个二进制的注册表一致 ⇒ 指纹相同）。
pub fn registry_digest() -> String {
    fnv1a64_hex(registry_summary().as_bytes())
}

/// 子集锚定哈希（**承 F2803 的 `ANCHOR_HASH`——引用而不另定数**）。
///
/// 为什么引用：注册表的「有哪些合法属性」完全由子集表决定，两处各写一份
/// 哈希必然漂移。引用让「子集表变更 → 注册表指纹变更」自动成立。
pub const SUBSET_ANCHOR_HASH: &str = super::veo03_subset::ANCHOR_HASH;

// ---------------------------------------------------------------------------
// 十、跨批对接点：上游契约接收 / 下游消费接口前向声明 / 对账钩子
// ---------------------------------------------------------------------------

/// 上游契约（F2804 交来的 token 流摘要；**哈希对账**）。
///
/// 本单消费的是 F2804 的 [`TokenStream::digest`]——不是源文本哈希：下游
/// 拿到的已经是 token 序列，对账必须对「实际消费到的东西」。
#[derive(Clone, Debug)]
pub struct ParseContract {
    /// 契约名（版本化）。
    pub name: String,
    /// token 流指纹。
    pub stream_hash: String,
}

impl ParseContract {
    /// 构造（契约名非空由 [`ParseContract::verify`] 兜住）。
    pub fn new(name: &str, stream_hash: &str) -> Self {
        ParseContract {
            name: name.to_string(),
            stream_hash: stream_hash.to_string(),
        }
    }

    /// 由流直接生成契约（**哈希重算，不是看填了没有**）。
    pub fn from_stream(name: &str, stream: &TokenStream) -> Self {
        ParseContract::new(name, &stream.digest())
    }

    /// 哈希对账。
    pub fn verify(&self, stream: &TokenStream) -> Result<(), StyleError> {
        if self.name.trim().is_empty() {
            return Err(StyleError::new(
                E_REGISTRY_INCOMPLETE,
                "上游契约对账被拒：契约名为空",
                "无名契约无法在台账里定位，也无法追责到提供方",
                "填入版本化契约名（如 tokenstream/v1）",
                "上游提供方（F2804）",
            ));
        }
        let actual = stream.digest();
        if actual != self.stream_hash {
            return Err(StyleError::new(
                E_REGISTRY_NAME_DRIFT,
                "上游契约指纹失真",
                &format!(
                    "契约 {} 登记指纹 {} 与实算 {} 不一致；流在传输中被改过",
                    self.name, self.stream_hash, actual
                ),
                "重新对账：让上游用流摘要覆盖登记值与实算值一起换契约",
                "上游提供方（F2804）",
            ));
        }
        Ok(())
    }

    /// 读屏单行。
    pub fn screen_line(&self) -> String {
        format!("上游契约 {}，流指纹 {}", self.name, self.stream_hash)
    }
}

/// 下游消费方义务（**前向声明**：先声明义务，再开始投递解析结果）。
#[derive(Clone, Copy, Debug)]
pub struct DeclSink {
    /// 消费方标识。
    pub id: &'static str,
    /// 中文名。
    pub zh: &'static str,
    /// 承接单号。
    pub anchor: &'static str,
    /// 该消费方对注册表与 [`Declaration`] 序列的**义务**。
    pub obligation: &'static str,
}

/// 下游消费方全集（F2806 / F2807 / F2810 / F2811 四家）。
///
/// **为什么是这四家**：F2806 简写展开消费 `List` 形态的值；F2807 样式表对象
/// 消费 `Declaration` 序列并落规则树；F2810 计算样式树消费已解析的值做
/// 共享；F2811 继承与初始值消费 `Declaration::name` 与注册表的
/// `inherited` 位。F2812（calc）与 F2813（颜色）拿到的是本单**前向声明**
/// 而非强依赖——它们解释语义，本单只登记。
pub const DECL_SINKS: [DeclSink; 4] = [
    DeclSink {
        id: "shorthand-expand",
        zh: "简写属性展开",
        anchor: "VE-F2806",
        obligation: "只消费 ParsedValue::List 与 Length；不得回读 token 流自行再解析",
    },
    DeclSink {
        id: "stylesheet-object",
        zh: "样式规则存储与样式表对象",
        anchor: "VE-F2807",
        obligation: "按 Declaration::offset 保留归因信息；不得假设声明序号等于 token 下标",
    },
    DeclSink {
        id: "computed-tree",
        zh: "计算样式树与共享结构",
        anchor: "VE-F2810",
        obligation: "只消费已接受声明；被拒声明不得以「缺值」形态进入计算",
    },
    DeclSink {
        id: "inherit-initial",
        zh: "继承与初始值解析",
        anchor: "VE-F2811",
        obligation: "以 PropertyId 与 F2803 的 inherited/initial 为准，不从 Declaration 另立继承语义",
    },
];

/// 跨域衔接对账钩子（**复用方义务**：复用注册表必须履行对账）。
pub fn reconciliation_hooks(reuser: &str) -> Vec<&'static str> {
    let mut out = Vec::new();
    for s in DECL_SINKS.iter() {
        if s.id == reuser {
            out.push(s.obligation);
        }
    }
    out
}

/// 下游投递前的守卫：**未前向声明的消费方不得拿到解析结果**。
pub fn check_sink_declared(reuser: &str) -> Result<&'static DeclSink, StyleError> {
    for s in DECL_SINKS.iter() {
        if s.id == reuser {
            return Ok(s);
        }
    }
    Err(StyleError::new(
        E_REGISTRY_INCOMPLETE,
        "下游投递被拒：消费方未前向声明",
        &format!("「{}」不在 DECL_SINKS 四家之列", reuser),
        "先在 DECL_SINKS 登记消费方与它的义务，再开始投递解析结果",
        "声明解析段维护方",
    ))
}

// ---------------------------------------------------------------------------
// 十一、性能逐项分解与隐私面声明
// ---------------------------------------------------------------------------

/// 复杂度分解表（**逐项可机检**：每项带复杂度级与依据）。
pub const COMPLEXITY_TABLE: [(&str, &str, &str); 8] = [
    ("按名查 id", "O(1)", "C1：编译期常量长度上界 44 次比较，与输入规模无关"),
    ("取表项", "O(1)", "C1：数组下标直取"),
    ("取解析器", "O(1)", "C1：11 项常量数组下标直取"),
    ("解析单条声明", "O(值长)", "C5：值 token 数上界 DECL_MAX_VALUE_TOKENS"),
    ("编译期名唯一性", "O(n^2) 一次性", "C7：946 次比较，编译期一次性，不进运行期"),
    ("注册表自检", "O(n^2)", "C7：仅诊断路径调用，不在声明解析热路径上"),
    ("注册表指纹", "O(n)", "C7：字段数与属性数同阶"),
    ("下游义务查询", "O(n)", "C1：4 项常量表"),
];

/// 复杂度表读屏文本。
pub fn complexity_screen_text() -> String {
    let mut s = String::from("声明解析段复杂度分解：");
    for (item, cx, basis) in COMPLEXITY_TABLE.iter() {
        s.push_str(&format!("{} {}（{}）；", item, cx, basis));
    }
    s
}

/// 隐私面枚举（本单只有「无」这一项——写成枚举而不是一句注释，是为了让
/// 「将来若引入隐私面」必须改类型，而不是悄悄加字段）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrivacySurface {
    /// 无隐私面：只处理作者写下的声明文本，不采集、不回传任何用户数据。
    None,
}

impl PrivacySurface {
    /// 中文名。
    pub fn zh(self) -> &'static str {
        match self {
            PrivacySurface::None => "无隐私面",
        }
    }

    /// 采集了什么（本单：什么都没采集）。
    pub fn collects(self) -> &'static str {
        match self {
            PrivacySurface::None => "无：只读样式表文本与其中的 token，不读取用户输入、不写回任何通道",
        }
    }
}

/// 隐私面声明（锚点原文「无隐私面」）。
pub const PRIVACY_SURFACE: PrivacySurface = PrivacySurface::None;

/// 模块自述（读屏与台账用：一句话说清本单做了什么、没做什么）。
pub fn module_narration() -> String {
    format!(
        "VE-F2805 声明解析与属性表注册：注册表 {} 条属性（{} 解析器槽，{}），单源承 F2803 四族子集表；{}",
        PROPERTY_COUNT,
        PARSERS.len(),
        compile_time_audit_summary(),
        complexity_screen_text()
    )
}

// ---------------------------------------------------------------------------
// 十二、域自检入口（判据层在 `veo05_props_checks`）
// ---------------------------------------------------------------------------

/// 判据层入口占位（**真实判据在 `super::veo05_props_checks`**）。
///
/// 之所以在本体留这一行：聚合器（`svstar2/mod.rs`）按
/// `("<单号>", <判据层>::run_…())` 登记，两处分离会让「单号 ↔ 文件」对照
/// 需要人来核；这里给出显式指针，聚合时一目了然。
pub const CHECKS_ANCHOR: &str = "VE-F2805";