//! VE-F3405 · 令牌类型系统（VE-E 域 · 主题与个性化引擎 · 令牌运行时组）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3405`
//!
//! **判据（锚点原文）**：六类全集、类型校验、单位显式、违例定位、判据。
//!
//! **职责定位（锚点原文）**：令牌类型全集（颜色/尺寸/字重/时长/缓动/阴影六类），
//! 类型校验（颜色令牌塞字符串=拒绝），单位体系（px/rem/ms 显式）。
//!
//! **数据结构（锚点原文）**：类型系统；校验器。
//!
//! ## 一、为什么"六类"是封闭集合，而不是"能认出来就算一类"
//!
//! 朴素写法是"看一眼值像不像颜色，像就当颜色"——那种写法里类型系统是**猜**的，
//! 于是"颜色令牌塞字符串"要么被猜成"某种具名颜色"（不拒绝，规格要的恰恰是拒绝），
//! 要么被猜错成别的类（错得更远：错类比不校验更难查，因为下游拿到的是合法值）。
//!
//! 本模块把类型做成**声明先行**：令牌路径先在 [`TypeChecker`] 上 [`declare`] 一个
//! [`TokenKind`]，随后 [`check_all`] 按声明的类走**专属解析器**。类与解析器是
//! 一张封闭表 [`TokenKind::ALL`]（六项），不在表内的一律 [`TypeCode::UnknownKind`]
//! 拒绝。这样"颜色令牌塞字符串"的结局是确定的：`Rgba` 解析器拿到 `"coral"`，
//! 判定 [`TypeCode::ColorBad`]，**不猜具名色**。
//!
//! ## 二、单位体系为什么封闭在 px/rem/ms 三项
//!
//! 规格原文是"px/rem/ms 显式"。这里的"显式"有两层，本模块两层都兑现：
//!
//! 1. **不许裸数字**：需要单位的类（[`TokenKind::expects_unit`] 为真）拿不到单位
//!    就是 [`TypeCode::MissingUnit`] 拒绝。不做"缺单位按 px 默认"——那会让
//!    `12rem` 与 `12` 在文件里长得一样，缩放就失效了。
//! 2. **不许表外单位**：`vw`/`em`/`pt`/`%`/`s` 一律 [`TypeCode::UnknownUnit`]。
//!    刻意**不**收 `s`：一旦体系里同时有 ms 与 s，跨令牌做算术时两套单位制要在
//!    每个消费点归一，漏一处就是错值；只留 ms 把换算责任推给作者，一次写对。
//!
//! ## 三、违例的三条降级路径是分级的，不是"都报错"
//!
//! 锚点把三条路径写成三种语气，本模块照语气分流，**不合并**：
//!
//! - **类型违例 → 拒绝 + 定位**（[`TypeCode::KindMismatch`] / [`ColorBad`] …）：
//!   值进不了声明的类，产出 [`Violation`]，带 [`Site`] 三要素，**不产出值**。
//! - **单位混用 → 告警**（[`TypeCode::UnitClash`]）：单位**本身合法**，但单位族与
//!   声明类不符（尺寸令牌写 `200ms`）。此时数值仍可用，按声明类归一（`200ms` 在
//!   尺寸位归一为 `200px`）并**放行**，同时把告警计数显性化。理由：单位族写错是
//!   作者笔误，值本身没坏；而"颜色塞字符串"是**类型契约**破了，放行等于让错误
//!   扩散到渲染层。两条路径混成一条会让告警被忽略、错误被放行。
//! - **未知类型 → 拒绝**（[`TypeCode::UnknownKind`]）：声明了六类之外的类。
//!
//! ## 四、判据为什么不是恒真的（鉴别力从哪来）
//!
//! 类型校验最容易被写成恒真断言："跑一遍 check_all，断言不炸"。不炸的写法有
//! 两种可能：真对，或者**什么都没查**。所以本模块做了三件事：
//!
//! - [`TypeChecker::lenient_parse_for_test`] 是**故意宽松的参考实现**：颜色接受
//!   任意具名串、缺单位按默认单位补上。判据 [`ver01e_checks`] 断言它在一批语料上
//!   **放行而严格实现拒绝**——这是鉴别力的反证。
//! - 字重合法档位由常量表 [`WEIGHT_STEPS`] **在判据侧独立推导**期望值，不向被测
//!   函数问答案（否则是自证式）。
//! - 恒等式判据：输入条数 == 产出值数 + 违例数，一条不落（零静默）。
//!
//! ## 五、性能逐项分解（锚点原文 O(令牌数)）
//!
//! - [`TypeChecker::declare`]：O(声明数) —— 顺序表 + 严格升序校验，比哈希表省
//!   内存且顺序可复现（回归可比对）。
//! - [`check_all`]：O(令牌数) —— 一次线性遍历，每条解析的工作量由值长度界定。
//! - [`TypeChecker::verify`]：O(声明数) —— 结构不变式（升序、不重复）。
//! - [`TypeChecker::spoken`]：O(类数) —— 固定六类，不随令牌数增长。
//!
//! ## 六、跨批对接点（锚点原文 E17 验证器复用）
//!
//! 交出三件：稳定错误码 [`TypeCode::wire`]、可导出报告 [`TypeReport`]、
//! 供静态验证三查调用的 [`TypeChecker::check_all`]。E17（令牌验证器）直接消费，
//! 不自己重写一套类型判断。覆盖优先级归 F3406、持久化归 F3407、主题运行时归
//! F3404，本模块一概不做。
//!
//! ## 七、无障碍与隐私（锚点原文 类型表读屏可达）
//!
//! [`TypeChecker::spoken`] 给出与类型表等信息的纯文本替述：六类令牌的中文名、
//! 稳定短码、是否带单位、单位族、已声明条数。它**不含任何令牌值正文**——值可能
//! 含用户自定义字符串，进读屏就是隐私泄露。判据侧用值集里真实存在的字面量做反例。
//!
//! **零墙钟、零 IO、无随机源**，时间与并发均不参与，回归可复现。

use super::ver01b_parser::Site;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、常量（参数唯一源）
// ---------------------------------------------------------------------------

/// 类型系统版本。类型语义变更走版本号（下游按版本决定是否重验）。
pub const TYPE_SYS_VERSION: &str = "E01-typesys-v1";

/// 令牌路径字节上限。路径是标识不是正文，超长即拒绝而不是截断（截断后两个不同
/// 路径会同名，那比拒绝更坏）。
pub const MAX_PATH_LEN: usize = 256;

/// 声明条数上限。**必须与 [`super::ver01c_cascade::MAX_GRAPH_NODES`] 同值**：
/// 级联引擎放行 4096 个节点而类型系统拒收 4095 条声明，就是"解析成功、级联成功、
/// 类型校验失败"的空转。
pub const MAX_DECLS: usize = 4096;

/// 单次校验的令牌条数上限（同 [`MAX_DECLS`]，理由同上）。
pub const MAX_BINDINGS: usize = 4096;

/// 单个令牌值的字节上限。**必须与 [`super::ver01c_cascade::MAX_VALUE_LEN`] 同值**。
pub const MAX_RAW_LEN: usize = 4096;

/// 定点数小数位数上限：毫（milli）。**超过即 [`TypeCode::BadLiteral`] 拒绝**，
/// 不做截断——截断会把 `12.3456rem` 变成 `12.345rem`，那是静默改值。
pub const MILLI_DIGITS: u32 = 3;

/// `1.000` 在定点表示下的整数值。
pub const MILLI_ONE: i64 = 1000;

/// 颜色十六进制字面量的最小长度（含 `#`）：`#RGB`。
pub const HEX_MIN_LEN: usize = 4;

/// 颜色十六进制字面量的最大长度（含 `#`）：`#RRGGBBAA`。
pub const HEX_MAX_LEN: usize = 9;

/// 阴影值的最小段数：`x y blur color`。
pub const SHADOW_MIN_SEGMENTS: usize = 4;

/// 阴影值的最大段数：`x y blur spread color`。
pub const SHADOW_MAX_SEGMENTS: usize = 5;

/// 字重合法档位（百分比制，步长 100）。判据侧**独立由此表推导**期望值，
/// 不向被测函数问答案。
pub const WEIGHT_STEPS: [u16; 9] = [100, 200, 300, 400, 500, 600, 700, 800, 900];

/// 字重下界（0 不存在、100 是最轻档）。
pub const WEIGHT_MIN: u16 = 100;

/// 字重上界（900 是最重档；1000+ 在 CSS 里属任意值，本体系不收）。
pub const WEIGHT_MAX: u16 = 900;

/// 时长上界（毫秒）。超界拒绝而不是钳制——钳制会把 `99999ms` 悄悄变成
/// `10000ms`，动效时长被改而没人知道。
pub const MAX_DURATION_MS: u32 = 10_000;

/// 模糊半径下界（毫像素）。负模糊半径无意义。
pub const MIN_BLUR_MILLI: i64 = 0;

/// 扩散半径下界（毫像素）。
pub const MIN_SPREAD_MILLI: i64 = -MILLI_ONE * 64;

/// 尺寸绝对值上界（毫像素）：`1000.000`。防溢出也防离谱值。
pub const MAX_LENGTH_ABS_MILLI: i64 = MILLI_ONE * 1000;

/// 分量上界（颜色单通道 / rgb 三元组）。
pub const MAX_CHANNEL: u8 = 255;

/// 诊断三要素的占位上限——给 `expect`/`got` 的静态文案，不限制。
pub const DIAG_TEXT_MAX: usize = 96;

/// 声明路径不得为空串的硬线（空路径会让两条不同声明撞名）。
pub const PATH_MIN_LEN: usize = 1;

// ---------------------------------------------------------------------------
// 二、六类令牌类型（封闭全集）
// ---------------------------------------------------------------------------

/// 令牌类型族。六类，封闭；不在 [`TokenKind::ALL`] 内的一律拒绝。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TokenKind {
    /// 颜色：`#RGB`/`#RGBA`/`#RRGGBB`/`#RRGGBBAA`/`rgb(r,g,b)`/`rgba(r,g,b,a)`。
    Color,
    /// 尺寸：定点数 + `px`/`rem`。
    Size,
    /// 字重：百分比档位或 `normal`/`bold` 关键字。
    FontWeight,
    /// 时长：定点数 + `ms`。
    Duration,
    /// 缓动：七种具名缓动之一。
    Easing,
    /// 阴影：`x y blur [spread] color` 多段合成。
    Shadow,
}

impl TokenKind {
    /// **六类全集**。类型系统只认这六项，改这里就是改类型系统。
    pub const ALL: [TokenKind; 6] = [
        TokenKind::Color,
        TokenKind::Size,
        TokenKind::FontWeight,
        TokenKind::Duration,
        TokenKind::Easing,
        TokenKind::Shadow,
    ];

    /// 线上短码。**枚举判别值不是线上编码值**——显式映射，改枚举顺序不影响线。
    pub fn wire(self) -> &'static str {
        match self {
            TokenKind::Color => "color",
            TokenKind::Size => "size",
            TokenKind::FontWeight => "fontWeight",
            TokenKind::Duration => "duration",
            TokenKind::Easing => "easing",
            TokenKind::Shadow => "shadow",
        }
    }

    /// 读屏播报用的中文名（不依赖颜色与缩进）。
    pub fn zh(self) -> &'static str {
        match self {
            TokenKind::Color => "颜色",
            TokenKind::Size => "尺寸",
            TokenKind::FontWeight => "字重",
            TokenKind::Duration => "时长",
            TokenKind::Easing => "缓动",
            TokenKind::Shadow => "阴影",
        }
    }

    /// 从线上短码/中文名解析类型。**未知类型 → [`TypeCode::UnknownKind`] 拒绝**。
    /// 大小写与首尾空白都按"不同类型"处理：`Color`、`COLOR`、`color ` 全部拒绝。
    /// 宽松匹配会让"看着像"变成"就是"，那正是本单要消灭的猜测。
    pub fn parse(s: &str) -> Result<TokenKind, TypeCode> {
        for k in TokenKind::ALL.iter() {
            if s == k.wire() {
                return Ok(*k);
            }
        }
        Err(TypeCode::UnknownKind)
    }

    /// 该类是否必须带显式单位。
    pub fn expects_unit(self) -> bool {
        matches!(self, TokenKind::Size | TokenKind::Duration)
    }

    /// 该类归属的单位族。颜色/字重/缓动/阴影本身无单位。
    pub fn unit_family(self) -> UnitFamily {
        match self {
            TokenKind::Size => UnitFamily::Length,
            TokenKind::Duration => UnitFamily::Time,
            TokenKind::Color | TokenKind::FontWeight | TokenKind::Easing | TokenKind::Shadow => {
                UnitFamily::None
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 三、单位体系（封闭三项：px / rem / ms）
// ---------------------------------------------------------------------------

/// 单位族。长度与时间是两个不相通的族，跨族即"单位混用"。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum UnitFamily {
    /// 无单位（颜色/字重/缓动）。
    None,
    /// 长度族：`px`/`rem`。
    Length,
    /// 时间族：`ms`。
    Time,
}

impl UnitFamily {
    /// 读屏播报名。
    pub fn zh(self) -> &'static str {
        match self {
            UnitFamily::None => "无单位",
            UnitFamily::Length => "长度",
            UnitFamily::Time => "时间",
        }
    }

    /// 该族的**归一目标单位**（跨族单位混用时数值折算到哪个单位）。
    ///
    /// 取 [`Unit::ALL`] 里本族的首个成员，**不硬编码字面量**——硬编码的话，
    /// 将来往长度族加入第三个成员时归一目标会静默停在旧值，而归一目标是
    /// "作者笔误被吃掉"这件事的唯一落点，落错了没人看得出来。
    pub fn default_unit(self) -> Option<Unit> {
        Unit::ALL.iter().find(|u| u.family() == self).copied()
    }
}

/// 合法单位的封闭集合：只有 `px`/`rem`/`ms` 三项。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Unit {
    /// 像素（长度族）。
    Px,
    /// 根字号（长度族）。
    Rem,
    /// 毫秒（时间族）。
    Ms,
}

impl Unit {
    /// **单位体系全集**（锚点原文 px/rem/ms 显式）。
    pub const ALL: [Unit; 3] = [Unit::Px, Unit::Rem, Unit::Ms];

    /// 单位短码（与字面量后缀逐字节相同）。
    pub fn wire(self) -> &'static str {
        match self {
            Unit::Px => "px",
            Unit::Rem => "rem",
            Unit::Ms => "ms",
        }
    }

    /// 读屏播报名。
    pub fn zh(self) -> &'static str {
        match self {
            Unit::Px => "像素",
            Unit::Rem => "根字号",
            Unit::Ms => "毫秒",
        }
    }

    /// 该单位归属的族。
    pub fn family(self) -> UnitFamily {
        match self {
            Unit::Px | Unit::Rem => UnitFamily::Length,
            Unit::Ms => UnitFamily::Time,
        }
    }

    /// 从字面量后缀解析单位。**表外单位（`vw`/`em`/`pt`/`%`/`s`）→
    /// [`TypeCode::UnknownUnit`] 拒绝**，大小写敏感（`PX` 不是 `px`）。
    pub fn from_suffix(s: &str) -> Result<Unit, TypeCode> {
        for u in Unit::ALL.iter() {
            if s == u.wire() {
                return Ok(*u);
            }
        }
        Err(TypeCode::UnknownUnit)
    }
}

// ---------------------------------------------------------------------------
// 四、六类的值表示
// ---------------------------------------------------------------------------

/// 颜色值。四通道各 0..=255，**不透明色 alpha 恒为 255**——把 alpha 默认成 0 会让
/// "忘了写 alpha"变成全透明，而全透明在渲染里看起来像"颜色没生效"。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Rgba {
    /// 红。
    pub r: u8,
    /// 绿。
    pub g: u8,
    /// 蓝。
    pub b: u8,
    /// 透明度（`#RGB`/`#RRGGBB`/`rgb()` 恒为 255）。
    pub a: u8,
}

impl Rgba {
    /// 不透明黑。
    pub const fn opaque_black() -> Rgba {
        Rgba {
            r: 0,
            g: 0,
            b: 0,
            a: MAX_CHANNEL,
        }
    }

    /// 由三通道构造（alpha 恒 255）。
    pub const fn rgb(r: u8, g: u8, b: u8) -> Rgba {
        Rgba { r, g, b, a: MAX_CHANNEL }
    }
}

/// 长度值。定点（毫单位）+ 单位，**不用浮点**——浮点在 no_std 里没有
/// `floor`/`round` 的稳定语义，且跨主题累加会漂。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Length {
    /// 毫单位整数值（`1.5rem` → `1500`）。
    pub milli: i64,
    /// 单位（长度族恒为 `Px`/`Rem`，由 [`UnitFamily::Length`] 守卫）。
    pub unit: Unit,
}

impl Length {
    /// 构造长度值。**族不符即拒**（长度位不许出现 `ms`）——用
    /// [`TypeCode::UnitClash`] 而不是静默接受：构造面与解析面同一条纪律。
    pub fn new(milli: i64, unit: Unit) -> Result<Length, TypeCode> {
        if unit.family() != UnitFamily::Length {
            return Err(TypeCode::UnitClash);
        }
        if milli.checked_abs().map(|m| m > MAX_LENGTH_ABS_MILLI) != Some(false) {
            return Err(TypeCode::OutOfRange);
        }
        Ok(Length { milli, unit })
    }

    /// 读屏播报文本（**只播数值与单位，不含任何调用方上下文**）。
    pub fn spoken(&self) -> String {
        format!("{}{}", self.milli, self.unit.wire())
    }
}

/// 时长值（毫秒）。整数毫秒，不带小数——亚毫秒时序在帧预算里无意义，
/// 而小数会让"两档时长差 0.5ms"这种无意义的比较变得可能。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Duration {
    /// 毫秒。
    pub ms: u32,
}

impl Duration {
    /// 构造时长。超 [`MAX_DURATION_MS`] 拒绝，**不钳制**。
    pub fn new(ms: u32) -> Result<Duration, TypeCode> {
        if ms > MAX_DURATION_MS {
            return Err(TypeCode::OutOfRange);
        }
        Ok(Duration { ms })
    }
}

/// 字重值。合法档位由 [`WEIGHT_STEPS`] 定义，步长 100。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FontWeight {
    /// 百分比值（100..=900，步长 100）。
    pub pct: u16,
}

impl FontWeight {
    /// 构造字重。越界或不在档位表上 → [`TypeCode::WeightOffGrid`]。
    pub fn new(pct: u16) -> Result<FontWeight, TypeCode> {
        if pct < WEIGHT_MIN || pct > WEIGHT_MAX {
            return Err(TypeCode::WeightOffGrid);
        }
        // 档位表是常量，用它做判定而不是 `% 100 == 0` 的算术等价式——
        // 两者当前同值，但档位表是唯一的语义源，判据侧也按它推导。
        if !WEIGHT_STEPS.contains(&pct) {
            return Err(TypeCode::WeightOffGrid);
        }
        Ok(FontWeight { pct })
    }

    /// `normal` 关键字的档位。
    pub const NORMAL: u16 = 400;
    /// `bold` 关键字的档位。
    pub const BOLD: u16 = 700;

    /// 关键字解析：`normal`/`bold`。其余关键字 → [`TypeCode::WeightOffGrid`]。
    pub fn from_keyword(s: &str) -> Result<FontWeight, TypeCode> {
        match s {
            "normal" => FontWeight::new(FontWeight::NORMAL),
            "bold" => FontWeight::new(FontWeight::BOLD),
            _ => Err(TypeCode::WeightOffGrid),
        }
    }
}

/// 缓动。七种具名缓动，**不含 `cubic-bezier()` 自定义曲线**——自定义曲线是
/// 另一种表达（需要四个无单位小数），把它塞进具名类会让"缓动"这一类的
/// 语法不再自洽（有的要单位有的不要）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Easing {
    /// 线性。
    Linear,
    /// 缓入缓出（CSS `ease`）。
    Ease,
    /// 缓入。
    EaseIn,
    /// 缓出。
    EaseOut,
    /// 缓入缓出（CSS `ease-in-out`）。
    EaseInOut,
    /// 阶跃起始。
    StepStart,
    /// 阶跃结束。
    StepEnd,
}

impl Easing {
    /// 缓动全集（判据侧按此表独立推导期望集合）。
    pub const ALL: [Easing; 7] = [
        Easing::Linear,
        Easing::Ease,
        Easing::EaseIn,
        Easing::EaseOut,
        Easing::EaseInOut,
        Easing::StepStart,
        Easing::StepEnd,
    ];

    /// 字面量短码。
    pub fn wire(self) -> &'static str {
        match self {
            Easing::Linear => "linear",
            Easing::Ease => "ease",
            Easing::EaseIn => "ease-in",
            Easing::EaseOut => "ease-out",
            Easing::EaseInOut => "ease-in-out",
            Easing::StepStart => "step-start",
            Easing::StepEnd => "step-end",
        }
    }

    /// 读屏播报名。
    pub fn zh(self) -> &'static str {
        match self {
            Easing::Linear => "匀速",
            Easing::Ease => "缓动",
            Easing::EaseIn => "缓入",
            Easing::EaseOut => "缓出",
            Easing::EaseInOut => "缓入缓出",
            Easing::StepStart => "阶跃起始",
            Easing::StepEnd => "阶跃结束",
        }
    }

    /// 从字面量解析缓动。**未知缓动名 → [`TypeCode::EasingUnknown`] 拒绝**，
    /// 不做"最近的具名缓动"猜测（`ease-in-ot` 拼错时猜成 `ease-in` 是错的，
    /// 报出来才是对的）。
    pub fn parse(s: &str) -> Result<Easing, TypeCode> {
        for e in Easing::ALL.iter() {
            if s == e.wire() {
                return Ok(*e);
            }
        }
        Err(TypeCode::EasingUnknown)
    }
}

/// 阴影值。`x y blur [spread] color` 合成，**颜色段必须在末位**——放在任意位置
/// 都会让"第几段是颜色"成为解析歧义源。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Shadow {
    /// x 偏移。
    pub offset_x: Length,
    /// y 偏移。
    pub offset_y: Length,
    /// 模糊半径。非负。
    pub blur: Length,
    /// 扩散半径。可为负（内阴影收边）。
    pub spread: Option<Length>,
    /// 颜色。
    pub color: Rgba,
}

impl Shadow {
    /// 段数（是否带扩散半径）。
    pub fn segments(&self) -> usize {
        if self.spread.is_some() {
            SHADOW_MAX_SEGMENTS
        } else {
            SHADOW_MIN_SEGMENTS
        }
    }
}

/// 六类的值载体。**枚举判别值不是线上编码值**——用 [`TokenValue::kind`] 反查。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TokenValue {
    /// 颜色值。
    Color(Rgba),
    /// 尺寸值。
    Size(Length),
    /// 字重值。
    FontWeight(FontWeight),
    /// 时长值。
    Duration(Duration),
    /// 缓动值。
    Easing(Easing),
    /// 阴影值。
    Shadow(Shadow),
}

impl TokenValue {
    /// 本值归属的类型（与声明类一致才可能被 [`TypeChecker::check_all`] 产出）。
    pub fn kind(self) -> TokenKind {
        match self {
            TokenValue::Color(_) => TokenKind::Color,
            TokenValue::Size(_) => TokenKind::Size,
            TokenValue::FontWeight(_) => TokenKind::FontWeight,
            TokenValue::Duration(_) => TokenKind::Duration,
            TokenValue::Easing(_) => TokenKind::Easing,
            TokenValue::Shadow(_) => TokenKind::Shadow,
        }
    }

    /// 读屏播报文本。**只播类型与数值，不含调用方上下文**。
    pub fn spoken(self) -> String {
        match self {
            TokenValue::Color(c) => format!("颜色 r{} g{} b{} a{}", c.r, c.g, c.b, c.a),
            TokenValue::Size(l) => format!("尺寸 {}", l.spoken()),
            TokenValue::FontWeight(w) => format!("字重 {}", w.pct),
            TokenValue::Duration(d) => format!("时长 {}ms", d.ms),
            TokenValue::Easing(e) => format!("缓动 {}", e.wire()),
            TokenValue::Shadow(_) => format!("阴影 {} 段", self.kind_spoken_segments()),
        }
    }

    fn kind_spoken_segments(self) -> usize {
        match self {
            TokenValue::Shadow(s) => s.segments(),
            _ => 0,
        }
    }
}

// ---------------------------------------------------------------------------
// 五、诊断码与违例（三要素：码 + 定位 + 说明）
// ---------------------------------------------------------------------------

/// 类型系统稳定错误码。**枚举判别值不是线上编码值**，一律走 [`TypeCode::wire`]。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TypeCode {
    /// 诊断三要素里有空段（构造期拦截）。
    IncompleteDiag,
    /// 路径为空或超长。
    BadPath,
    /// 声明重复。
    DupDecl,
    /// 声明数超上限。
    DeclLimit,
    /// 令牌条数超上限。
    TokenLimit,
    /// 值字节数超上限。
    ValueLimit,
    /// 声明的类不在六类全集内。
    UnknownKind,
    /// 值与声明类不符（例：颜色令牌收到 `"12px"`）。
    KindMismatch,
    /// 字面量在本类内也不合法（例：颜色写 `"#zzz"`）。
    BadLiteral,
    /// 应带单位而未带。
    MissingUnit,
    /// 单位不在 px/rem/ms 体系内。
    UnknownUnit,
    /// 单位合法但族与声明类不符——**告警级**，按声明类归一后放行。
    UnitClash,
    /// 数值越界（长度/时长）。
    OutOfRange,
    /// 字重越界或不在档位表。
    WeightOffGrid,
    /// 缓动名未知。
    EasingUnknown,
    /// 阴影段数不合法。
    ShadowArity,
}

impl TypeCode {
    /// 线上短码。**新增码位只追加不复用**（复用会让历史告警的含义漂移）。
    pub fn wire(self) -> &'static str {
        match self {
            TypeCode::IncompleteDiag => "E_TYPE_DIAG_INCOMPLETE",
            TypeCode::BadPath => "E_TYPE_BAD_PATH",
            TypeCode::DupDecl => "E_TYPE_DUP_DECL",
            TypeCode::DeclLimit => "E_TYPE_DECL_LIMIT",
            TypeCode::TokenLimit => "E_TYPE_TOKEN_LIMIT",
            TypeCode::ValueLimit => "E_TYPE_VALUE_LIMIT",
            TypeCode::UnknownKind => "E_TYPE_UNKNOWN_KIND",
            TypeCode::KindMismatch => "E_TYPE_KIND_MISMATCH",
            TypeCode::BadLiteral => "E_TYPE_BAD_LITERAL",
            TypeCode::MissingUnit => "E_TYPE_MISSING_UNIT",
            TypeCode::UnknownUnit => "E_TYPE_UNKNOWN_UNIT",
            TypeCode::UnitClash => "E_TYPE_UNIT_CLASH",
            TypeCode::OutOfRange => "E_TYPE_OUT_OF_RANGE",
            TypeCode::WeightOffGrid => "E_TYPE_WEIGHT_OFF_GRID",
            TypeCode::EasingUnknown => "E_TYPE_EASING_UNKNOWN",
            TypeCode::ShadowArity => "E_TYPE_SHADOW_ARITY",
        }
    }

    /// 读屏播报名（不依赖颜色与缩进）。
    pub fn zh(self) -> &'static str {
        match self {
            TypeCode::IncompleteDiag => "诊断信息不完整",
            TypeCode::BadPath => "令牌路径非法",
            TypeCode::DupDecl => "类型声明重复",
            TypeCode::DeclLimit => "类型声明超上限",
            TypeCode::TokenLimit => "令牌条数超上限",
            TypeCode::ValueLimit => "令牌值超长",
            TypeCode::UnknownKind => "未知令牌类型",
            TypeCode::KindMismatch => "类型不符",
            TypeCode::BadLiteral => "字面量非法",
            TypeCode::MissingUnit => "缺少单位",
            TypeCode::UnknownUnit => "未知单位",
            TypeCode::UnitClash => "单位族与类型不符",
            TypeCode::OutOfRange => "数值越界",
            TypeCode::WeightOffGrid => "字重不在档位表上",
            TypeCode::EasingUnknown => "缓动名未知",
            TypeCode::ShadowArity => "阴影段数非法",
        }
    }

    /// **告警级**判定：只有 [`TypeCode::UnitClash`] 是告警，其余全是拒绝级。
    /// 分级由函数给出唯一口径，散落的 `if` 迟早会分叉。
    pub fn is_warning(self) -> bool {
        matches!(self, TypeCode::UnitClash)
    }
}

/// 违例。**三要素缺一不可**：稳定码、定位 [`Site`]、路径。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Violation {
    /// 稳定错误码。
    pub code: TypeCode,
    /// 令牌路径（逐字等于声明路径，不做规范化）。
    pub path: String,
    /// 定位（读屏第一句就念它）。
    pub site: Site,
    /// 期望说明（静态文案）。
    pub expect: &'static str,
    /// 实得说明（静态文案，不含值正文——值可能含用户字符串）。
    pub got: &'static str,
}

impl Violation {
    /// 构造违例。**三要素任一为空 → 码位改写为 [`TypeCode::IncompleteDiag`]**：
    /// 缺定位的违例等于没定位，而空路径的违例在表里根本无法定位。
    pub fn new(
        code: TypeCode,
        path: &str,
        site: Site,
        expect: &'static str,
        got: &'static str,
    ) -> Violation {
        let mut c = code;
        if path.len() < PATH_MIN_LEN || expect.is_empty() || got.is_empty() {
            c = TypeCode::IncompleteDiag;
        }
        Violation {
            code: c,
            path: String::from(path),
            site,
            expect,
            got,
        }
    }

    /// 读屏播报一行。顺序固定：位置 → 路径 → 码 → 说明 → 实得。
    pub fn spoken(&self) -> String {
        format!(
            "第{}行第{}列 {} {} 期望{} 实得{}",
            self.site.line,
            self.site.col,
            self.path,
            self.code.zh(),
            self.expect,
            self.got
        )
    }
}

// ---------------------------------------------------------------------------
// 六、声明与校验器
// ---------------------------------------------------------------------------

/// 一条类型声明：路径 → 类。**声明先行**是本单的核心（见头注第一节）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Decl {
    /// 令牌路径。
    pub path: String,
    /// 声明的类。
    pub kind: TokenKind,
    /// 声明点。
    pub site: Site,
}

/// 一条已校验通过的绑定：路径 + 值 + 定位。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Checked {
    /// 令牌路径。
    pub path: String,
    /// 校验后的值。
    pub value: TokenValue,
    /// 值的定义点。
    pub site: Site,
}

/// 待校验的输入绑定：路径 + 值原文 + 定义点。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Binding {
    /// 令牌路径。
    pub path: String,
    /// 值原文（数字保留原文，单位与关键字保留大小写）。
    pub raw: String,
    /// 定义点。
    pub site: Site,
}

impl Binding {
    /// 构造绑定。**不做校验**——校验只在 [`TypeChecker::check_all`] 里发生，
    /// 在构造期就偷偷校验会让"哪里拒绝的"这个问题失去答案。
    pub fn new(path: &str, raw: &str, site: Site) -> Binding {
        Binding {
            path: String::from(path),
            raw: String::from(raw),
            site,
        }
    }
}

/// 校验结果。**恒等式**：产出值数 + 违例数 == 输入条数（零静默）。
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Verdict {
    /// 通过并产出值的绑定（按输入顺序）。
    pub accepted: Vec<Checked>,
    /// 拒绝级违例（按输入顺序）。
    pub violations: Vec<Violation>,
    /// 告警级记录（按输入顺序）。**告警不阻断**，但必须显性化。
    pub warnings: Vec<Violation>,
}

impl Verdict {
    /// 守恒对账：输入条数 == 产出 + 拒绝。**返回差值，0 为守恒**。
    ///
    /// 刻意**不**把告警算进产出侧：告警项仍然产出了值，它既在 `accepted` 里也在
    /// `warnings` 里。若把告警也计入，会得出"多算了一条"的假红。
    pub fn conservation_delta(&self, inputs: usize) -> usize {
        let accounted = self.accepted.len() + self.violations.len();
        if accounted > inputs {
            accounted - inputs
        } else {
            inputs - accounted
        }
    }

    /// 取指定路径的产出值。判据侧用它做**逐令牌比对**而不是"整体在不在表里"。
    pub fn value_of(&self, path: &str) -> Option<TokenValue> {
        self.accepted.iter().find(|c| c.path == path).map(|c| c.value)
    }

    /// 违例总数（拒绝 + 告警）。
    pub fn issue_count(&self) -> usize {
        self.violations.len() + self.warnings.len()
    }
}

/// 类型校验器。声明表为**严格升序顺序表**——顺序可复现（回归能逐条比对），
/// 且 O(1) 定位靠二分而非哈希（省内存，也省 no_std 侧的实现面）。
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct TypeChecker {
    /// 声明（严格按 `path` 升序，由 [`TypeChecker::declare`] 维持）。
    decls: Vec<Decl>,
}

impl TypeChecker {
    /// 空校验器。
    pub fn new() -> TypeChecker {
        TypeChecker { decls: Vec::new() }
    }

    /// 已声明条数。
    pub fn decl_count(&self) -> usize {
        self.decls.len()
    }

    /// **判据专用**：取声明路径的有序列表（用于"值集由表推导"类判据）。
    ///
    /// 刻意**不**暴露 `&mut Vec<Decl>`：那是绕过全部不变式的口子，而判据需要的
    /// 只是"表里有哪些路径"这一事实。
    pub fn decl_paths(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::with_capacity(self.decls.len());
        for d in self.decls.iter() {
            out.push(d.path.clone());
        }
        out
    }

    /// **判据专用**：把声明表整体反转（手工破坏升序不变式）。
    ///
    /// 只提供"反转"这一个破坏动作，而不是泛泛的 `&mut`：泛泛可变引用会让
    /// 判据写出"改坏表再验 verify"之外的用法，而这些用法无法区分
    /// "verify 没抓到"与"改动根本没生效"。反转必定改变顺序，可复现。
    pub fn reverse_decls_for_test(&mut self) {
        self.decls.reverse();
    }

    /// **判据专用**：把某条声明的路径强行改成给定值（模拟表被并发改坏）。
    ///
    /// 找不到目标路径时**不动表**——静默无效的注入会让判据"测不到任何东西"
    /// 却仍然全绿（这就是采样留洞）。
    pub fn force_path_for_test(&mut self, path: &str, new_path: &str) -> bool {
        let at = self.lower_bound(path);
        if at < self.decls.len() && self.decls[at].path == path {
            self.decls[at].path = String::from(new_path);
            return true;
        }
        false
    }

    /// 注册一条类型声明。
    ///
    /// 拒绝条件：路径空/超长、重复、声明数超 [`MAX_DECLS`]、类不在六类全集内
    /// （[`TypeCode::UnknownKind`]）。**返回 `Err` 而不是静默忽略**——静默忽略
    /// 会让"我声明了但没生效"这件事无人察觉。
    pub fn declare(&mut self, path: &str, kind: TokenKind, site: Site) -> Result<(), TypeCode> {
        if path.len() < PATH_MIN_LEN {
            return Err(TypeCode::BadPath);
        }
        if path.len() > MAX_PATH_LEN {
            return Err(TypeCode::BadPath);
        }
        // 六类封闭：不在全集内的类拒收。
        if !TokenKind::ALL.contains(&kind) {
            return Err(TypeCode::UnknownKind);
        }
        if self.decls.len() >= MAX_DECLS {
            return Err(TypeCode::DeclLimit);
        }
        let at = self.lower_bound(path);
        if at < self.decls.len() && self.decls[at].path == path {
            return Err(TypeCode::DupDecl);
        }
        let decl = Decl {
            path: String::from(path),
            kind,
            site,
        };
        self.decls.insert(at, decl);
        Ok(())
    }

    /// 按路径取声明（二分）。
    pub fn decl_of(&self, path: &str) -> Option<&Decl> {
        let at = self.lower_bound(path);
        if at < self.decls.len() && self.decls[at].path == path {
            Some(&self.decls[at])
        } else {
            None
        }
    }

    /// 校验器的结构不变式：严格升序、不重复、条数不超限、类都在全集内。
    ///
    /// 判据侧**手工改坏一张正常表**再验本函数（而不是拿一张本来就没建好的表来
    /// 验）——后者会把"表坏"与"验证器坏"混成同一个现象。
    pub fn verify(&self) -> Result<(), TypeCode> {
        if self.decls.len() > MAX_DECLS {
            return Err(TypeCode::DeclLimit);
        }
        let mut i = 0usize;
        while i < self.decls.len() {
            let d = &self.decls[i];
            if d.path.len() < PATH_MIN_LEN || d.path.len() > MAX_PATH_LEN {
                return Err(TypeCode::BadPath);
            }
            if !TokenKind::ALL.contains(&d.kind) {
                return Err(TypeCode::UnknownKind);
            }
            if i > 0 {
                let prev = &self.decls[i - 1];
                if prev.path.as_str() >= d.path.as_str() {
                    // 非严格升序（含重复）——一条就够，不逐条报。
                    return Err(TypeCode::DupDecl);
                }
            }
            i += 1;
        }
        Ok(())
    }

    /// **本单主流程**：按声明校验一批绑定。
    ///
    /// 单趟线性遍历，O(令牌数)（见头注第五节）。每条绑定恰好落进三桶之一：
    /// 通过 → [`Verdict::accepted`]；告警级 → 同时进 `accepted` 与
    /// [`Verdict::warnings`]；拒绝级 → [`Verdict::violations`]。
    pub fn check_all(&self, bindings: &[Binding]) -> Verdict {
        let mut v = Verdict::default();
        if bindings.len() > MAX_BINDINGS {
            // 条数超限是**整批拒**而不是截断：截断会让后段的令牌既没值也没违例，
            // 违反零静默。整批拒则每条都能在 violations 里看到自己的违例。
            for b in bindings.iter() {
                v.violations.push(Violation::new(
                    TypeCode::TokenLimit,
                    &b.path,
                    b.site,
                    "令牌条数不超上限",
                    "整批超限",
                ));
            }
            return v;
        }
        for b in bindings.iter() {
            match self.check_one(b) {
                Ok((value, warn)) => {
                    v.accepted.push(Checked {
                        path: b.path.clone(),
                        value,
                        site: b.site,
                    });
                    if let Some(code) = warn {
                        v.warnings.push(Violation::new(
                            code,
                            &b.path,
                            b.site,
                            warn_expect(code),
                            warn_got(code),
                        ));
                    }
                }
                Err(code) => {
                    v.violations.push(Violation::new(
                        code,
                        &b.path,
                        b.site,
                        expect_of(code),
                        got_of(code),
                    ));
                }
            }
        }
        v
    }

    /// 单条校验：先查声明，再按声明类走**专属解析器**。
    fn check_one(&self, b: &Binding) -> Result<(TokenValue, Option<TypeCode>), TypeCode> {
        if b.raw.len() > MAX_RAW_LEN {
            return Err(TypeCode::ValueLimit);
        }
        let d = self.decl_of(&b.path).ok_or(TypeCode::UnknownKind)?;
        parse_as(d.kind, &b.raw)
    }

    /// **故意宽松的参考实现（仅判据用）**。
    ///
    /// 它把"猜"当成策略：颜色接受任意具名串（`"coral"` 当合法颜色）、缺单位按
    /// 默认单位补上、跨族单位直接忽略后缀。它的**唯一用途**是给判据提供反证——
    /// 若严格实现在一批语料上与它结论一致，那说明严格实现其实没在查。
    pub fn lenient_parse_for_test(kind: TokenKind, raw: &str) -> Result<TokenValue, TypeCode> {
        match kind {
            TokenKind::Color => {
                // 宽松：只要不是空串就算"颜色"，不做十六进制校验。
                if raw.is_empty() {
                    return Err(TypeCode::BadLiteral);
                }
                Ok(TokenValue::Color(Rgba::opaque_black()))
            }
            TokenKind::Size | TokenKind::Duration => {
                // 宽松：剥掉任意后缀，把前缀当数字，缺后缀按默认单位补。
                let digits: String = raw
                    .chars()
                    .take_while(|c| c.is_ascii_digit() || *c == '.' || *c == '-')
                    .collect();
                let n = parse_milli(&digits).unwrap_or(MILLI_ONE);
                if kind == TokenKind::Size {
                    let l = Length::new(n, Unit::Px).map_err(|_| TypeCode::BadLiteral)?;
                    Ok(TokenValue::Size(l))
                } else {
                    // 宽松实现的**唯一职责是"蒙对"**：它必须在严格实现拒的语料上
                    // 也放行，否则它当不了反证。`120ms` 的毫值是 120000，
                    // 折算回毫秒是 120 —— 不折算就会超上界被拒，反证链就断了。
                    // 同时**上限也不设防**（`Duration::new` 会拒超界值），
                    // 所以这里直接造 `Duration { ms }` 而不走构造器。
                    let ms_u32 = u32::try_from(n / MILLI_ONE).unwrap_or(0);
                    Ok(TokenValue::Duration(Duration { ms: ms_u32 }))
                }
            }
            TokenKind::FontWeight => FontWeight::new(FontWeight::NORMAL)
                .map(TokenValue::FontWeight)
                .map_err(|_| TypeCode::BadLiteral),
            TokenKind::Easing => Easing::parse(raw)
                .map(TokenValue::Easing)
                .map_err(|_| TypeCode::BadLiteral),
            TokenKind::Shadow => Err(TypeCode::ShadowArity),
        }
    }

    /// 类型表读屏替述。**只含类型元信息，绝不含令牌值正文**（见头注第七节）。
    pub fn spoken(&self) -> String {
        let mut s = String::new();
        s.push_str("令牌类型表，共六类：");
        for k in TokenKind::ALL.iter() {
            s.push_str(k.zh());
            s.push('(');
            s.push_str(k.wire());
            s.push(')');
            if k.expects_unit() {
                s.push_str("需显式单位");
                s.push('/');
                s.push_str(k.unit_family().zh());
            } else {
                s.push_str("无单位");
            }
            s.push('；');
        }
        s.push_str("单位体系三项：");
        for u in Unit::ALL.iter() {
            s.push_str(u.wire());
            s.push('(');
            s.push_str(u.zh());
            s.push_str("族");
            s.push_str(u.family().zh());
            s.push_str(")；");
        }
        s.push_str("已声明");
        s.push_str(&self.decls.len().to_string());
        s.push_str("条。");
        s
    }

    /// 下界：首个 `path >= key` 的下标（表维持严格升序）。
    fn lower_bound(&self, key: &str) -> usize {
        let mut lo = 0usize;
        let mut hi = self.decls.len();
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            if self.decls[mid].path.as_str() < key {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        lo
    }
}

// ---------------------------------------------------------------------------
// 七、按类解析（六条互不相同的路径）
// ---------------------------------------------------------------------------

/// 按声明类解析字面量。返回 `(值, 告警码)`——告警码非空表示**已放行但要告警**。
///
/// 这里是"类型违例→拒绝"与"单位混用→告警"分流的地方（见头注第三节）：
/// 单位族不符**不**走 `Err`，而是把数值按声明类归一后放行。
pub fn parse_as(kind: TokenKind, raw: &str) -> Result<(TokenValue, Option<TypeCode>), TypeCode> {
    match kind {
        TokenKind::Color => parse_color(raw).map(|c| (TokenValue::Color(c), None)),
        TokenKind::Size => parse_sized(raw, TokenKind::Size),
        TokenKind::Duration => parse_sized(raw, TokenKind::Duration),
        TokenKind::FontWeight => parse_weight(raw).map(|w| (TokenValue::FontWeight(w), None)),
        TokenKind::Easing => parse_easing(raw).map(|e| (TokenValue::Easing(e), None)),
        TokenKind::Shadow => parse_shadow(raw).map(|s| (TokenValue::Shadow(s), None)),
    }
}

/// 颜色解析。**只认六种字面量形态**，不猜具名色。
///
/// 导出供判据做等价对照（`#abc` 展开验证）；生产消费面走 [`parse_as`]。
pub fn parse_color(raw: &str) -> Result<Rgba, TypeCode> {
    let b = raw.as_bytes();
    if b.is_empty() {
        return Err(TypeCode::BadLiteral);
    }
    if b[0] == b'#' {
        return parse_hex(raw);
    }
    if raw.starts_with("rgb(") || raw.starts_with("rgba(") {
        return parse_rgb_func(raw);
    }
    // 具名色（`coral`/`red`/`transparent`）与任何别的形态一律拒绝：规格要的
    // 正是"颜色令牌塞字符串 = 拒绝"，这里就是那句话的执行点。
    Err(TypeCode::BadLiteral)
}

/// 十六进制颜色。长度只接受 3/4/6/8 数字位（含 `#`）。
fn parse_hex(raw: &str) -> Result<Rgba, TypeCode> {
    let body = &raw[1..];
    if body.len() < HEX_MIN_LEN - 1 || body.len() > HEX_MAX_LEN - 1 {
        return Err(TypeCode::BadLiteral);
    }
    let nib = |c: u8| -> Result<u8, TypeCode> {
        match c {
            b'0'..=b'9' => Ok(c - b'0'),
            b'a'..=b'f' => Ok(c - b'a' + 10),
            b'A'..=b'F' => Ok(c - b'A' + 10),
            _ => Err(TypeCode::BadLiteral),
        }
    };
    let d: Vec<u8> = body
        .as_bytes()
        .iter()
        .map(|c| nib(*c))
        .collect::<Result<Vec<u8>, TypeCode>>()?;
    let pair = |x: u8, y: u8| -> u8 { x * 16 + y };
    let rgba = match d.len() {
        3 => Rgba::rgb(
            pair(d[0], d[0]),
            pair(d[1], d[1]),
            pair(d[2], d[2]),
        ),
        4 => Rgba {
            r: pair(d[0], d[0]),
            g: pair(d[1], d[1]),
            b: pair(d[2], d[2]),
            a: pair(d[3], d[3]),
        },
        6 => Rgba::rgb(
            pair(d[0], d[1]),
            pair(d[2], d[3]),
            pair(d[4], d[5]),
        ),
        8 => Rgba {
            r: pair(d[0], d[1]),
            g: pair(d[2], d[3]),
            b: pair(d[4], d[5]),
            a: pair(d[6], d[7]),
        },
        _ => return Err(TypeCode::BadLiteral),
    };
    Ok(rgba)
}

/// `rgb(r,g,b)` / `rgba(r,g,b,a)`。三通道各 0..=255；alpha 是 **0..=1 的比例**
/// （四舍五入到 0..=255），不接受 `rgb(1,2,3,255)` 这种"alpha 当字节"的写法
/// ——两种写法同形但语义不同，混收就是在猜。
fn parse_rgb_func(raw: &str) -> Result<Rgba, TypeCode> {
    let open = raw.find('(').ok_or(TypeCode::BadLiteral)?;
    let close = raw.rfind(')').ok_or(TypeCode::BadLiteral)?;
    if close < open {
        return Err(TypeCode::BadLiteral);
    }
    let has_alpha = raw.starts_with("rgba(");
    let inner = &raw[open + 1..close];
    let parts: Vec<&str> = inner.split(',').collect();
    if parts.len() != if has_alpha { 4 } else { 3 } {
        return Err(TypeCode::BadLiteral);
    }
    let mut ch: [u8; 3] = [0u8; 3];
    for i in 0..3 {
        let s = parts[i].trim();
        if s.is_empty() || !s.as_bytes().iter().all(|c| c.is_ascii_digit()) {
            return Err(TypeCode::BadLiteral);
        }
        match s.parse::<u32>() {
            Ok(v) if v <= MAX_CHANNEL as u32 => ch[i] = v as u8,
            _ => return Err(TypeCode::BadLiteral),
        }
    }
    let alpha = if has_alpha {
        let s = parts[3].trim();
        // 比例写法 `0.0..=1.0`；整数 `1` 也是合法比例，但 `255` 不是。
        let scaled = parse_ratio_milli(s).ok_or(TypeCode::BadLiteral)?;
        if scaled < 0 || scaled > MILLI_ONE {
            return Err(TypeCode::BadLiteral);
        }
        // 毫比例 → 通道：四舍五入而非截断（`0.5` 截断成 0 会让半透明变全透明）。
        ((scaled * MAX_CHANNEL as i64 + MILLI_ONE / 2) / MILLI_ONE) as u8
    } else {
        MAX_CHANNEL
    };
    Ok(Rgba {
        r: ch[0],
        g: ch[1],
        b: ch[2],
        a: alpha,
    })
}

/// 尺寸/时长的共同骨架：定点数 + 单位后缀。
///
/// **这是全单唯一一处"告警而非拒绝"的分支**（见头注第三节），所以它的判定必须
/// 显式到不能再简：
///
/// - 后缀为空（裸数字）→ [`TypeCode::MissingUnit`] **拒绝**。不按默认单位补：
///   补了之后 `12rem` 与 `12` 在文件里同形，缩放就失效了。
/// - 后缀不在 px/rem/ms 内（`vw`/`em`/`pt`/`%`/`s`）→ [`TypeCode::UnknownUnit`]
///   **拒绝**。体系封闭，不引第二时制。
/// - 后缀**在体系内但族与声明类不符**（尺寸位写 `200ms`）→
///   **告警** [`TypeCode::UnitClash`] 并按声明类归一后**放行**。数值本身没坏，
///   是作者笔误；放行同时把告警显性化，比整批拒绝有用。
fn parse_sized(raw: &str, kind: TokenKind) -> Result<(TokenValue, Option<TypeCode>), TypeCode> {
    if raw.is_empty() {
        return Err(TypeCode::MissingUnit);
    }
    // 切分复用 [`split_num_suffix`]（从左扫数值前缀）——与阴影段同一套文法，
    // 两处各写一份 `rposition` 就是两处各留一个 `#000` 型缺陷。
    let (num, suffix) = split_num_suffix(raw)?;
    if suffix.is_empty() {
        return Err(TypeCode::MissingUnit);
    }
    let unit = Unit::from_suffix(suffix)?; // 表外单位在此拒绝
    let milli = parse_milli(num).ok_or(TypeCode::BadLiteral)?;

    if unit.family() != kind.unit_family() {
        // 单位混用：**不拒绝**，归一到声明族的唯一成员后放行 + 告警。
        // 归一目标不是硬编码：`Length` 族有 px/rem 两个成员，若此处硬编码 Px，
        // 一旦将来把 ms 归一进长度族就会静默错。取族内 `ALL` 的首项，
        // 并在判据里断这个"归一目标本身可构造"（见 `E05-边界-归一目标可构造`）。
        let target = kind.unit_family().default_unit().ok_or(TypeCode::UnitClash)?;
        let value = if kind == TokenKind::Size {
            let l = Length::new(milli, target)?;
            TokenValue::Size(l)
        } else {
            // 时长位：把毫值折回整毫秒。`200px` 在时长位 = 200 毫，
            // 归一后是 200ms —— **折算必须做**，否则 `0.2s`（=200ms）
            // 与 `0.2ms`（=0ms）会得到同一个值，单位混用告警就成了摆设。
            let ms = milli_to_ms(milli)?;
            TokenValue::Duration(Duration::new(ms)?)
        };
        return Ok((value, Some(TypeCode::UnitClash)));
    }

    if kind == TokenKind::Size {
        let l = Length::new(milli, unit)?;
        Ok((TokenValue::Size(l), None))
    } else {
        // **毫值 → 毫秒的折算**：`1.5ms` 的毫值是 1500，时长存的是整毫秒 1。
        // 漏掉这一步会让所有亚毫秒写法越界（1500 > MAX_DURATION_MS 不成立，
        // 但 `20000.000ms` 这类会被算成两万个毫秒），且判据里
        // `dur.fast = "120ms"` 的期望 120 就对不上了。
        let ms = milli_to_ms(milli)?;
        Ok((TokenValue::Duration(Duration::new(ms)?), None))
    }
}

/// 毫值 → 整毫秒。**拒绝亚毫秒精度**而不是四舍五入：`0.4ms` 折成 0ms
/// 等于把一个"极短但非零"的时长变成零，动效会整个不出现——那是静默改语义。
fn milli_to_ms(milli: i64) -> Result<u32, TypeCode> {
    if milli < 0 {
        return Err(TypeCode::OutOfRange);
    }
    if milli % MILLI_ONE != 0 {
        return Err(TypeCode::BadLiteral);
    }
    let ms = milli / MILLI_ONE;
    u32::try_from(ms).map_err(|_| TypeCode::OutOfRange)
}

/// 定点数解析。**只接受至多 [`MILLI_DIGITS`] 位小数**，超出即 `None` 拒绝，
/// 不截断。小数点与负号各至多一个。
fn parse_milli(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    if b.is_empty() {
        return None;
    }
    let mut i = 0usize;
    let mut neg = false;
    if b[0] == b'-' {
        neg = true;
        i = 1;
    } else if b[0] == b'+' {
        i = 1;
    }
    let int_start = i;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    let int_part = &s[int_start..i];
    let mut frac_part = "";
    if i < b.len() {
        if b[i] != b'.' {
            return None;
        }
        i += 1;
        let frac_start = i;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        if i != b.len() {
            return None; // 尾随垃圾
        }
        frac_part = &s[frac_start..i];
    }
    if int_part.is_empty() && frac_part.is_empty() {
        return None;
    }
    if frac_part.len() > MILLI_DIGITS as usize {
        return None; // 精度超限，拒绝而非截断
    }
    // 整数部分用 checked 链累加——解析不可信输入，溢出必须被拒而不是回绕。
    let mut int_val: i64 = 0;
    for c in int_part.as_bytes() {
        let d = (c - b'0') as i64;
        int_val = int_val.checked_mul(10)?.checked_add(d)?;
    }
    let mut frac_val: i64 = 0;
    let mut scale: i64 = 1;
    for c in frac_part.as_bytes() {
        let d = (c - b'0') as i64;
        frac_val = frac_val.checked_mul(10)?.checked_add(d)?;
        scale = scale.checked_mul(10)?;
    }
    // 小数补齐到毫位。
    while scale < MILLI_ONE {
        frac_val = frac_val.checked_mul(10)?;
        scale = scale.checked_mul(10)?;
    }
    let total = int_val.checked_mul(MILLI_ONE)?.checked_add(frac_val)?;
    if neg {
        total.checked_neg()
    } else {
        Some(total)
    }
}

/// `0.0..=1.0` 比例 → 毫比例（0..=1000）。不接受百分号、不接受 `50%`。
fn parse_ratio_milli(s: &str) -> Option<i64> {
    parse_milli(s)
}

/// 字重解析：档位数字或 `normal`/`bold`。
fn parse_weight(raw: &str) -> Result<FontWeight, TypeCode> {
    if raw.is_empty() {
        return Err(TypeCode::WeightOffGrid);
    }
    if raw.as_bytes()[0].is_ascii_digit() {
        // 全数字才当档位；`400px` 不是字重。
        if !raw.as_bytes().iter().all(|c| c.is_ascii_digit()) {
            return Err(TypeCode::WeightOffGrid);
        }
        let v: u32 = raw.parse::<u32>().map_err(|_| TypeCode::WeightOffGrid)?;
        if v > WEIGHT_MAX as u32 {
            return Err(TypeCode::WeightOffGrid);
        }
        return FontWeight::new(v as u16);
    }
    FontWeight::from_keyword(raw)
}

/// 缓动解析。
fn parse_easing(raw: &str) -> Result<Easing, TypeCode> {
    Easing::parse(raw)
}

/// 阴影解析：段数 4 或 5，颜色段在末位。
///
/// 段内可有空白（`0 2px 8px #000` 与 `0  2px  8px  #000` 同义）。连续空白折叠
/// 由 [`split_ws`] 负责——不折叠的话 `0  2px` 会被当成空段。
fn parse_shadow(raw: &str) -> Result<Shadow, TypeCode> {
    let segs: Vec<&str> = split_ws(raw);
    if segs.len() < SHADOW_MIN_SEGMENTS || segs.len() > SHADOW_MAX_SEGMENTS {
        return Err(TypeCode::ShadowArity);
    }
    let color_at = segs.len() - 1;
    let ox = parse_len_seg(segs[0])?;
    let oy = parse_len_seg(segs[1])?;
    let blur = parse_len_seg(segs[2])?;
    if blur.milli < MIN_BLUR_MILLI {
        return Err(TypeCode::OutOfRange);
    }
    let spread = if segs.len() == SHADOW_MAX_SEGMENTS {
        let sp = parse_len_seg(segs[3])?;
        if sp.milli < MIN_SPREAD_MILLI {
            return Err(TypeCode::OutOfRange);
        }
        Some(sp)
    } else {
        None
    };
    let color = parse_color(segs[color_at])?;
    Ok(Shadow {
        offset_x: ox,
        offset_y: oy,
        blur,
        spread,
        color,
    })
}

/// 阴影的一个长度段。**段内必须自带单位**（与 [`parse_sized`] 同纪律）。
///
/// 切分点用 [`split_num_suffix`]（**从左向右扫数值前缀**），而不是
/// `rposition`（从右找最后一个数字）——后者会把 `#000` 的末位 `0` 当成数值
/// 结尾，切出 `"#0"` + `"00"` 这种 nonsense，然后报 `UnknownUnit`。
/// 判据 `E05-类型-阴影四段通过` 就是钉这一处的：颜色段含数字时必须仍被识别为
/// 颜色段。
///
/// 跨族单位（`8ms`）在此**拒绝**而不是静默归一——注意这与顶层 `parse_sized`
/// 的告警策略相反，理由明确：阴影是一段复合字面量，若这里放行并归一，作者写
/// `0 2px 8ms #000` 会得到"看得过去的阴影"，而告警无处可挂（`parse_shadow`
/// 的返回型只有值，没有告警通道）。**同一策略在两处不一致时，必须有一处是
/// 拒绝**，否则就是把"告警"变成了"看情况"。判据 `E05-边界-阴影跨族拒绝`
/// 钉住这条分界。
fn parse_len_seg(s: &str) -> Result<Length, TypeCode> {
    // **零偏移豁免单位**：`0 2px 8px #000` 里的 `x` 偏移是裸 `0`，而这是最常见
    // 的阴影写法（垂直阴影）。若一刀切要求 `0px`，作者就得写 `0px 2px 8px #000`
    // ——不算错，但会让"横向偏移为零"这个语义被写法噪声淹没。
    //
    // 豁免**只对数值恰为零生效**，且**只在前三段（偏移/模糊）**：
    // - 扩散段不豁免（`0` 与"没写"在视觉上无法区分，留歧义）；
    // - 颜色段根本不进本函数（末位单独走 `parse_color`）；
    // - 非零一律要求显式单位（`2` 仍报 `MissingUnit`），
    //   所以豁免不会变成"影子长度一律无单位"。
    if s == "0" {
        return Length::new(0, Unit::Px);
    }
    let (num, suffix) = split_num_suffix(s)?;
    if suffix.is_empty() {
        return Err(TypeCode::MissingUnit);
    }
    let unit = Unit::from_suffix(suffix)?;
    // 跨族 → 显式 UnitClash（拒绝级）。不放行。
    if unit.family() != UnitFamily::Length {
        return Err(TypeCode::UnitClash);
    }
    let milli = parse_milli(num).ok_or(TypeCode::BadLiteral)?;
    Length::new(milli, unit)
}

/// 把一段切成「数值前缀 + 其余」。
///
/// 数值前缀文法：`['-'|'+'] 数字* ['.' 数字*]`，且**至少有一个数字**。
/// 遇首个非数字字符即停（不跳空白——`12 px` 里的空格意味着后缀是 `" px"`，
/// 那正是"带空格单位须拒"的判据要抓的东西，所以这里不能容错跳过）。
fn split_num_suffix(s: &str) -> Result<(&str, &str), TypeCode> {
    let b = s.as_bytes();
    let mut i = 0usize;
    if i < b.len() && (b[i] == b'-' || b[i] == b'+') {
        i += 1;
    }
    let digits_start = i;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    if i < b.len() && b[i] == b'.' {
        i += 1;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
    }
    if i == digits_start {
        // 整个前缀一个数字都没有（`"px"`/`"coral"`/`"#000"`）。
        return Err(TypeCode::BadLiteral);
    }
    Ok((&s[..i], &s[i..]))
}

/// 按空白切段，**折叠连续空白并丢弃空段**。
fn split_ws(s: &str) -> Vec<&str> {
    let mut out: Vec<&str> = Vec::new();
    for part in s.split_whitespace() {
        if !part.is_empty() {
            out.push(part);
        }
    }
    out
}

// ---------------------------------------------------------------------------
// 八、降级文案（唯一口径，避免散落的 if 迟早分叉）
// ---------------------------------------------------------------------------

/// 拒绝级错误码的"期望"文案。
fn expect_of(code: TypeCode) -> &'static str {
    match code {
        TypeCode::BadPath => "路径非空且不超长",
        TypeCode::DupDecl => "路径唯一",
        TypeCode::DeclLimit => "声明数不超上限",
        TypeCode::TokenLimit => "令牌条数不超上限",
        TypeCode::ValueLimit => "值长度不超上限",
        TypeCode::UnknownKind => "六类之一",
        TypeCode::KindMismatch => "值属声明的类",
        TypeCode::BadLiteral => "本类合法字面量",
        TypeCode::MissingUnit => "带显式单位",
        TypeCode::UnknownUnit => "单位在 px/rem/ms 内",
        TypeCode::OutOfRange => "数值在界内",
        TypeCode::WeightOffGrid => "字重在档位表上",
        TypeCode::EasingUnknown => "缓动名在全集内",
        TypeCode::ShadowArity => "阴影 4 或 5 段",
        TypeCode::IncompleteDiag => "诊断三要素齐备",
        // 告警级永不走本函数（走 `warn_expect`），此处仍给出文案以满足穷尽匹配。
        TypeCode::UnitClash => "单位族与类一致",
    }
}

/// 拒绝级错误码的"实得"文案。
fn got_of(code: TypeCode) -> &'static str {
    match code {
        TypeCode::BadPath => "路径非法",
        TypeCode::DupDecl => "路径重复",
        TypeCode::DeclLimit => "声明超限",
        TypeCode::TokenLimit => "条数超限",
        TypeCode::ValueLimit => "值超长",
        TypeCode::UnknownKind => "非六类",
        TypeCode::KindMismatch => "值属他类",
        TypeCode::BadLiteral => "字面量不合法",
        TypeCode::MissingUnit => "裸数值",
        TypeCode::UnknownUnit => "表外单位",
        TypeCode::OutOfRange => "越界",
        TypeCode::WeightOffGrid => "不在档位表",
        TypeCode::EasingUnknown => "不在全集",
        TypeCode::ShadowArity => "段数非法",
        TypeCode::IncompleteDiag => "要素缺失",
        TypeCode::UnitClash => "跨族单位",
    }
}

/// 告警级的"期望"文案。
fn warn_expect(code: TypeCode) -> &'static str {
    match code {
        TypeCode::UnitClash => "单位族与类一致",
        // 非告警码不应走到这里；给出可辨识文案而非 panic（解析面零 panic）。
        _ => "无告警",
    }
}

/// 告警级的"实得"文案。
fn warn_got(code: TypeCode) -> &'static str {
    match code {
        TypeCode::UnitClash => "跨族单位已按类归一",
        _ => "未知告警",
    }
}

// ---------------------------------------------------------------------------
// 九、报告（跨批对接点：E17 验证器直接消费）
// ---------------------------------------------------------------------------

/// 类型校验报告。E17（令牌验证器）的三查之一就是调用本报告：**报告缺失即阻断**
/// （锚点对 E17 的降级要求），所以报告的三个计数缺一不可。
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct TypeReport {
    /// 报告版本。取 [`TYPE_SYS_VERSION`]。
    pub version: &'static str,
    /// 已声明条数。
    pub declared: usize,
    /// 输入令牌条数。
    pub inputs: usize,
    /// 产出值条数。
    pub produced: usize,
    /// 拒绝级违例条数。
    pub rejected: usize,
    /// 告警级条数。
    pub warned: usize,
    /// 六类的产出分布（顺序同 [`TokenKind::ALL`]，下标即 `ALL` 下标）。
    pub per_kind: [usize; 6],
}

impl TypeReport {
    /// 由校验结果构建报告。**守恒性在此显式成立**：三类计数相加须等于输入数。
    pub fn build(checker: &TypeChecker, inputs: usize, v: &Verdict) -> TypeReport {
        let mut per_kind = [0usize; 6];
        for c in v.accepted.iter() {
            let k = c.value.kind();
            if let Some(i) = TokenKind::ALL.iter().position(|x| *x == k) {
                // 下标必在范围内（`kind()` 只产出六类），但仍做边界守卫——
                // 越界即**跳过而不是 panic**：报告面零 panic 是硬要求，
                // 而跳过会让分布对账不平，所以由 `E05-对接-分布对账` 抓住。
                per_kind[i] += 1;
            }
        }
        TypeReport {
            version: TYPE_SYS_VERSION,
            declared: checker.decl_count(),
            inputs,
            produced: v.accepted.len(),
            rejected: v.violations.len(),
            warned: v.warnings.len(),
            per_kind,
        }
    }

    /// 三类计数之和是否等于输入条数（E17 判"报告可信"用）。
    ///
    /// **告警项同时计入 `produced` 与 `warned`**，所以这里是三项之和而不是
    /// `produced + rejected`——后者会把有告警的令牌漏掉。
    pub fn accounted(&self) -> usize {
        self.produced + self.rejected
    }

    /// 六类分布之和是否等于产出条数（防止 `per_kind` 因越界跳过而对账不平）。
    pub fn per_kind_sum(&self) -> usize {
        let mut s = 0usize;
        for c in self.per_kind.iter() {
            s += *c;
        }
        s
    }

    /// 报告读屏文本。**不含令牌值正文**（只报类型与计数）。
    pub fn spoken(&self) -> String {
        let mut s = String::new();
        s.push_str("令牌类型校验报告 版本");
        s.push_str(self.version);
        s.push_str("：输入");
        s.push_str(&self.inputs.to_string());
        s.push_str("条，声明");
        s.push_str(&self.declared.to_string());
        s.push_str("条，产出");
        s.push_str(&self.produced.to_string());
        s.push_str("条，拒绝");
        s.push_str(&self.rejected.to_string());
        s.push_str("条，告警");
        s.push_str(&self.warned.to_string());
        s.push_str("条。六类分布：");
        for (i, k) in TokenKind::ALL.iter().enumerate() {
            s.push_str(k.zh());
            s.push('=');
            s.push_str(&self.per_kind[i].to_string());
            s.push('；');
        }
        s
    }
}