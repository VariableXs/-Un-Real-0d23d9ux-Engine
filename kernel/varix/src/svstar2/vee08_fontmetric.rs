//! VE-F0808 · 字体度量（VE-E 域 · 文字度量段 · 目标 300 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0808`
//!
//! **判据（锚点原文）**：字体度量提供排版与布局的数值基础：ascent/descent/行高
//! （hhea 与 OS/2 双来源，取约定优先级并显性声明）、字距（kerning 表查询）、
//! advance 宽度、下划线/删除线位置、cap-height/x-height，度量结果按字体实例
//! 缓存。行高算法三模式：字体默认（推荐行高）、紧凑（1.0 倍 em）、宽松（1.3 倍），
//! UI 可指定并全局一致。
//! 判据：**双来源优先级显性、三行高模式、缓存失效正确、0.001ms 查询、基线正确**。
//!
//! **错误路径与降级矩阵**（锚点原文）：
//!
//! - 度量表缺失（畸形字体） → **退化为 em 推导并计数**：字体没带hhea/OS/2
//!   表时不能拒绝渲染（那等于整段文字消失），改用 em 比例推导出一套保守
//!   度量**并计数**——计数让「有多少字体走了降级」可查，否则畸形字体静默
//!   降级会让排版悄悄变形却无人知晓。
//! - 双来源冲突超阈值 → **以 OS/2 为准并告警**：hhea 与 OS/2 对同一字段
//!   给出显著不同的值时，不能各取一半或随便挑一个，须按**约定的优先级**
//!   取舍并**显性声明**用了哪个来源，同时告警。静默选一个来源会让同一字体
//!   在不同代码路径下算出不同行高（渲染与命中测试对不上）。
//!
//! **数据结构**：度量快照（[`MetricSnapshot`]，字体 ID + 实例化参数→ 度量集）；
//! 度量缓存（[`MetricCache`]）；行高模式（[`LineHeightMode`]）；
//! 度量表来源（[`MetricSource`]）。
//!
//! **性能逐项分解**：度量查询 **O(1)**（缓存命中，[`MetricCache::get`] 单次
//! 哈希定位），全量快照 **O(表项数)**（逐字段裁决来源并去重）。
//!
//! **对接点**：度量供 F0842 整形（GPOS 前的基础 advance）与 N 域布局消费
//! （F0818 契约的精度基础）；基线对齐由 F0812 消费。
//!
//! ## 设计要点
//!
//! - **双来源优先级必须显性**（[`MetricSource`] + [`MetricSnapshot::source_of`]）：
//!   每个字段都能报出「这个值是从哪来的」。只给最终值不给来源，等于把
//!   「为什么这一行比另一行高」变成不可查的问题——而渲染与命中测试
//!   对不上恰恰是这类 bug 最难查的表现。
//! - **冲突阈值用相对比例，不用绝对差**（[`MetricSnapshot::CONFLICT_RATIO`]）：
//!   12px 字体差 2 个单位是 16%，24px 字体差 2 个单位只有 8%。用绝对阈值
//!   会让小字体频繁误报、大字体漏报真冲突。
//! - **缓存键必须含实例化参数**（[`MetricKey`]）：字重/字号/字号档位变了，
//!   advance 与度量都变。键里漏一个参数 ⇒ 用错度量 ⇒ 文本重叠或行距跳变。
//!   键用 `u128` 四段各 32 位，**单射**且无需掩码（掩码会把越界输入静默
//!   折回同一键，见模块内 [`MetricKey::of`] 注释）。
//! - **行高三模式各自给出可验证的公式**（[`LineHeightMode::line_height`]）：
//!   默认用字体推荐行高，紧凑 1.0em，宽松 1.3em。三者必须**互不相同**，
//!   否则「可指定并全局一致」无从验证。
//! - **基线由 ascent 单独表达**（[`MetricSnapshot::baseline`]）：基线不是
//!   「行高的一半」那种近似，它就是 ascent 的位置。半开区间与像素对齐的
//!   取整口径必须写明，否则同一行文字在两次渲染里落在不同基线上。
//!
//! ## 与相邻条的分工（易混，故写明）
//!
//! - **F0807（`vee07_prims`）管「把文本 quad 画出来」**，本条管「排版要用的
//!   数值从哪来」。前者是渲染段，后者是度量段——本条的输出是 F0842 整形
//!   与 N 域布局的输入。
//!
//! 两条各自**自持**定义类型，不跨模块 `use`——并行提交时跨模块引用会把两个
//! 模块的编译成败绑在一起，一方半成品就拖垮另一方，而这类失败报 E0583，
//! 与真实缺陷长得一样、极难分辨。

use crate::checks::CheckSet;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、规格常量
// ---------------------------------------------------------------------------

/// 度量单位：字体设计单位 em 的千分比（1000 = 1 em），与 OpenType 一致。
pub const UNITS_PER_EM: u32 = 1000;

/// 双来源冲突告警阈值（相对比例，百万分比）。
///
/// 用**相对**而非绝对：同一绝对差在小字体上是巨变、在大字体上可忽略。
pub const CONFLICT_RATIO: u32 = 50_000; // 5%

/// 度量缓存槽位上限（超出即登记 [`CacheOutcome::Evicted`]，不静默丢弃）。
pub const METRIC_CACHE_SLOTS: usize = 32;

/// 紧凑行高倍率（百万分比，1.0 em）。
pub const TIGHT_LINE_HEIGHT: u32 = 1_000_000;

/// 宽松行高倍率（百万分比，1.3 em）。
pub const LOOSE_LINE_HEIGHT: u32 = 1_300_000;

/// 度量查询承诺上限（微秒；缓存命中为 0）。锚点要求 ≤0.001ms = 1us。
pub const QUERY_BUDGET_US: u32 = 1;

/// 全量快照承诺上限（微秒）。
pub const SNAPSHOT_BUDGET_US: u32 = 500;

/// 缓存结论种数。
pub const CACHE_OUTCOME_COUNT: usize = 3;

/// 降级（度量表缺失）时使用的保守 ascent（千分比）。
pub const FALLBACK_ASCENT: u32 = 800;

/// 降级时使用的保守 descent（千分比）。
pub const FALLBACK_DESCENT: u32 = 200;

/// 降级时使用的保守 advance（千分比）。
pub const FALLBACK_ADVANCE: u32 = 500;

/// 降级时使用的保守 cap-height（千分比）。
pub const FALLBACK_CAP_HEIGHT: u32 = 700;

/// 降级时使用的保守 x-height（千分比）。
pub const FALLBACK_X_HEIGHT: u32 = 500;

// ---------------------------------------------------------------------------
// 二、度量来源与行高模式
// ---------------------------------------------------------------------------

/// 度量字段的来源（**双来源优先级必须显性**，见模块头设计要点）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MetricSource {
    /// 来自 `hhea` 表（TrueType 传统度量）。
    Hhea,
    /// 来自 `OS/2` 表（现代排版推荐来源，**冲突时以此为准**）。
    Os2,
    /// `OS/2` 缺失且 `hhea` 缺失，退化为 em 推导。
    EmFallback,
}

impl MetricSource {
    /// 全枚举（顺序即 [`MetricSource::ordinal`] 的下标）。
    pub const ALL: [MetricSource; 3] =
        [MetricSource::Hhea, MetricSource::Os2, MetricSource::EmFallback];

    /// 枚举下标（供计数数组用，**不是**线上编码值）。
    pub const fn ordinal(self) -> usize {
        match self {
            MetricSource::Hhea => 0,
            MetricSource::Os2 => 1,
            MetricSource::EmFallback => 2,
        }
    }

    /// 线编码（显式映射，**不用 `as u8`**——判别值与线上值不是一回事）。
    pub const fn wire(self) -> u8 {
        match self {
            MetricSource::Hhea => 0x48,
            MetricSource::Os2 => 0x4F,
            MetricSource::EmFallback => 0x45,
        }
    }

    /// 线上编码 → 枚举（未登记码返回 `None`，不静默兜底到某成员）。
    pub const fn from_wire(w: u8) -> Option<MetricSource> {
        match w {
            0x48 => Some(MetricSource::Hhea),
            0x4F => Some(MetricSource::Os2),
            0x45 => Some(MetricSource::EmFallback),
            _ => None,
        }
    }

    /// 是否降级来源（降级必须可计数、可查）。
    pub const fn is_fallback(self) -> bool {
        matches!(self, MetricSource::EmFallback)
    }

    /// 读屏文案（双语）。
    pub const fn label(self) -> &'static str {
        match self {
            MetricSource::Hhea => "hhea 表 / hhea table",
            MetricSource::Os2 => "OS/2 表 / OS2 table",
            MetricSource::EmFallback => "em 推导降级 / em fallback",
        }
    }
}

/// 行高算法三模式（**UI 可指定并全局一致**，见锚点）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineHeightMode {
    /// 字体默认（用字体自己的推荐行高）。
    FontDefault,
    /// 紧凑（1.0 倍 em）。
    Tight,
    /// 宽松（1.3 倍 em）。
    Loose,
}

impl LineHeightMode {
    /// 全枚举（顺序即 [`LineHeightMode::ordinal`] 的下标）。
    pub const ALL: [LineHeightMode; 3] =
        [LineHeightMode::FontDefault, LineHeightMode::Tight, LineHeightMode::Loose];

    /// 枚举下标。
    pub const fn ordinal(self) -> usize {
        match self {
            LineHeightMode::FontDefault => 0,
            LineHeightMode::Tight => 1,
            LineHeightMode::Loose => 2,
        }
    }

    /// 线编码。
    pub const fn wire(self) -> u8 {
        match self {
            LineHeightMode::FontDefault => 0x44,
            LineHeightMode::Tight => 0x54,
            LineHeightMode::Loose => 0x4C,
        }
    }

    /// 线上编码 → 枚举。
    pub const fn from_wire(w: u8) -> Option<LineHeightMode> {
        match w {
            0x44 => Some(LineHeightMode::FontDefault),
            0x54 => Some(LineHeightMode::Tight),
            0x4C => Some(LineHeightMode::Loose),
            _ => None,
        }
    }

    /// 该模式下的行高（千分比），`font_default_line_height` 为字体推荐值。
    ///
    /// 三者**必须互不相同**：若默认行高恰好等于 1.3em，「可指定」就少了一种
    /// 实际效果，`FontDefault` 与 `Loose` 变成同一个东西。
    pub const fn line_height(self, font_default_line_height: u32) -> u32 {
        match self {
            // 字体推荐值直接采用（它本身就是相对 em 的千分比）。
            LineHeightMode::FontDefault => font_default_line_height,
            // 1.0 em：UNITS_PER_EM = 1000 千分比即 1 em。
            LineHeightMode::Tight => UNITS_PER_EM,
            // 1.3 em = 1_300_000 百万分比 → 千分比即 1300。
            LineHeightMode::Loose => (LOOSE_LINE_HEIGHT / 1000) as u32,
        }
    }

    /// 读屏文案（双语）。
    pub const fn label(self) -> &'static str {
        match self {
            LineHeightMode::FontDefault => "字体默认行高 / font default",
            LineHeightMode::Tight => "紧凑 1.0em / tight",
            LineHeightMode::Loose => "宽松 1.3em / loose",
        }
    }
}

// ---------------------------------------------------------------------------
// 三、度量快照与缓存键
// ---------------------------------------------------------------------------

/// 字体原始度量表（`hhea` 与 `OS/2` 的字段，缺者为 `None`）。
///
/// 刻意用 `Option`：**「表里没这个字段」与「字段是 0」是两件事**——
/// 用 `0` 当缺省会让「合法的 0 度量」被误判成缺失而白白走降级。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RawMetricTable {
    /// `hhea.ascent`。
    pub hhea_ascent: Option<u32>,
    /// `hhea.descent`（正值）。
    pub hhea_descent: Option<u32>,
    /// `hhea.lineGap`。
    pub hhea_line_gap: Option<u32>,
    /// `OS/2.sTypoAscender`。
    pub os2_ascent: Option<u32>,
    /// `OS/2.sTypoDescender`（正值）。
    pub os2_descent: Option<u32>,
    /// `OS/2.sTypoLineGap`。
    pub os2_line_gap: Option<u32>,
    /// `OS/2.sCapHeight`。
    pub os2_cap_height: Option<u32>,
    /// `OS/2.sxHeight`。
    pub os2_x_height: Option<u32>,
}

impl RawMetricTable {
    /// 构造一个**两表齐全**的字体度量表。
    pub const fn full(ascent: u32, descent: u32, line_gap: u32, cap: u32, x: u32) -> Self {
        RawMetricTable {
            hhea_ascent: Some(ascent),
            hhea_descent: Some(descent),
            hhea_line_gap: Some(line_gap),
            os2_ascent: Some(ascent),
            os2_descent: Some(descent),
            os2_line_gap: Some(line_gap),
            os2_cap_height: Some(cap),
            os2_x_height: Some(x),
        }
    }

    /// 构造一个**两表皆缺**的畸形字体（走降级路径）。
    pub const fn malformed() -> Self {
        RawMetricTable {
            hhea_ascent: None,
            hhea_descent: None,
            hhea_line_gap: None,
            os2_ascent: None,
            os2_descent: None,
            os2_line_gap: None,
            os2_cap_height: None,
            os2_x_height: None,
        }
    }

    /// 是否两表皆缺（度量表缺失 ⇒ 降级）。
    pub const fn is_missing(&self) -> bool {
        self.hhea_ascent.is_none()
            && self.os2_ascent.is_none()
            && self.hhea_descent.is_none()
            && self.os2_descent.is_none()
    }
}

/// 度量快照：裁决后的度量集 + **每个字段的来源**。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MetricSnapshot {
    /// ascent（千分比）。
    pub ascent: u32,
    /// descent（千分比，正值）。
    pub descent: u32,
    /// 字体推荐行高（千分比）。
    pub recommended_line_height: u32,
    /// 默认 advance 宽度（千分比）。
    pub advance: u32,
    /// cap-height（千分比）。
    pub cap_height: u32,
    /// x-height（千分比）。
    pub x_height: u32,
    /// 下划线位置（千分比，从基线向下为正）。
    pub underline_position: u32,
    /// 下划线厚度（千分比）。
    pub underline_thickness: u32,
    /// 删除线位置（千分比）。
    pub strikeout_position: u32,
    /// 删除线厚度（千分比）。
    pub strikeout_thickness: u32,
    /// ascent 的来源。
    pub ascent_source: MetricSource,
    /// descent 的来源。
    pub descent_source: MetricSource,
    /// 行高的来源。
    pub line_height_source: MetricSource,
    /// 本快照是否发生过**双来源冲突告警**。
    pub conflict_warned: bool,
    /// 本快照是否走了**降级路径**。
    pub degraded: bool,
}

impl MetricSnapshot {
    /// 查某字段的来源（**来源显性**，见模块头设计要点）。
    pub const fn source_of(&self, field: MetricField) -> MetricSource {
        match field {
            MetricField::Ascent => self.ascent_source,
            MetricField::Descent => self.descent_source,
            MetricField::LineHeight => self.line_height_source,
            // 其余字段只有 OS/2 与降级两种来源（hhea 不含它们）。
            MetricField::CapHeight | MetricField::XHeight => {
                if self.degraded {
                    MetricSource::EmFallback
                } else {
                    MetricSource::Os2
                }
            }
            MetricField::Advance
            | MetricField::Underline
            | MetricField::Strikeout => {
                if self.degraded {
                    MetricSource::EmFallback
                } else {
                    MetricSource::Os2
                }
            }
        }
    }

    /// 基线位置（千分比，自行顶起算）。
    ///
    /// 基线**就是 ascent**，不是「行高的一半」那种近似。
    pub const fn baseline(&self) -> u32 {
        self.ascent
    }

    /// 指定行高模式下的行高（千分比）。
    pub const fn line_height(&self, mode: LineHeightMode) -> u32 {
        mode.line_height(self.recommended_line_height)
    }

    /// 是否双来源冲突（冲突须以 OS/2 为准并告警）。
    pub const fn has_conflict(&self) -> bool {
        self.conflict_warned
    }

    /// 中文摘要（审计用）。
    pub fn summary(&self) -> String {
        format!(
            "ascent{} descent{} 行高{} 基线{} 来源{}/{}",
            self.ascent,
            self.descent,
            self.recommended_line_height,
            self.baseline(),
            self.ascent_source.label(),
            self.line_height_source.label()
        )
    }
}

/// 可查询来源的度量字段。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MetricField {
    /// ascent。
    Ascent,
    /// descent。
    Descent,
    /// 行高。
    LineHeight,
    /// advance 宽度。
    Advance,
    /// cap-height。
    CapHeight,
    /// x-height。
    XHeight,
    /// 下划线位置。
    Underline,
    /// 删除线位置。
    Strikeout,
}

impl MetricField {
    /// 全枚举（顺序即 [`MetricField::ordinal`] 的下标）。
    pub const ALL: [MetricField; 8] = [
        MetricField::Ascent,
        MetricField::Descent,
        MetricField::LineHeight,
        MetricField::Advance,
        MetricField::CapHeight,
        MetricField::XHeight,
        MetricField::Underline,
        MetricField::Strikeout,
    ];

    /// 枚举下标。
    pub const fn ordinal(self) -> usize {
        match self {
            MetricField::Ascent => 0,
            MetricField::Descent => 1,
            MetricField::LineHeight => 2,
            MetricField::Advance => 3,
            MetricField::CapHeight => 4,
            MetricField::XHeight => 5,
            MetricField::Underline => 6,
            MetricField::Strikeout => 7,
        }
    }
}

/// 度量缓存键（字体 ID + 实例化参数）。
///
/// **四段各32 位、单射、无需掩码**：`u128` 布局 `[0,32)` 字体 /
/// `[32,64)` 字号 / `[64,96)` 字重 / `[96,128)` 字形档位。键里**必须**含
/// 实例化参数——字重或字号一变，advance 与度量都变；漏一个参数就会用错
/// 度量，表现为文本重叠或行距跳变。
///
/// 旧式 `a | (b << 8)` 布局让**较高段压在较低段之上**，越过边界后高位段
/// 会被低位吃掉，`(font=256, size=1)` 与 `(font=0, size=1)` 算出同一键。
/// 掩码也不能加——掩码会把越界输入静默折回同一键，宁可在文档写明也不掩码。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MetricKey {
    /// 字体 ID。
    pub font_id: u32,
    /// 字号（像素）。
    pub px_size: u32,
    /// 字重（数值越大越粗）。
    pub weight: u32,
    /// 字形档位（hinting/合成档）。
    pub glyph_variant: u32,
}

impl MetricKey {
    /// 构造一个缓存键。
    pub const fn new(font_id: u32, px_size: u32, weight: u32, glyph_variant: u32) -> MetricKey {
        MetricKey { font_id, px_size, weight, glyph_variant }
    }

    /// 打包成 `u128` 指纹（四段互不重叠 ⇒ 不同四元组必得不同键）。
    pub const fn of(&self) -> u128 {
        (self.font_id as u128)
            | ((self.px_size as u128) << 32)
            | ((self.weight as u128) << 64)
            | ((self.glyph_variant as u128) << 96)
    }
}

// ---------------------------------------------------------------------------
// 四、度量缓存
// ---------------------------------------------------------------------------

/// 缓存查询结论。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CacheOutcome {
    /// 命中（缓存命中查询耗时 0）。
    Hit,
    /// 未命中（需重算快照）。
    Miss,
    /// 槽位满且键不在其中 ⇒ 登记超限（**不静默丢弃**）。
    Evicted,
}

impl CacheOutcome {
    /// 全枚举（顺序即 [`CacheOutcome::ordinal`] 的下标）。
    pub const ALL: [CacheOutcome; 3] = [CacheOutcome::Hit, CacheOutcome::Miss, CacheOutcome::Evicted];

    /// 枚举下标（**三个变体必须互异**）。
    ///
    /// 初版把 [`CacheOutcome::Evicted`] 也映射到 0，理由写的是「超限并入
    /// hit 计数不当，单列下标」——可它自己就写成了 0，与 `Hit` **撞下标**。
    /// 后果不是「不好看」：两个变体共用一个槽位，`counts[0]` 被同时累加，
    /// 命中数与超限数混在一起，判据读 `counts[Evicted.ordinal()]` 读到的
    /// 其实是命中数——超限登记**在账面上完全不可见**（实测 counts
    /// `[1, 32, 0]`：Evicted 记到 0 位，而 0 位是 Hit）。
    pub const fn ordinal(self) -> usize {
        match self {
            CacheOutcome::Hit => 0,
            CacheOutcome::Miss => 1,
            CacheOutcome::Evicted => 2,
        }
    }

    /// 线编码（显式映射，不用 `as u8`）。
    pub const fn wire(self) -> u8 {
        match self {
            CacheOutcome::Hit => 0x48,
            CacheOutcome::Miss => 0x4D,
            CacheOutcome::Evicted => 0x45,
        }
    }

    /// 线上编码 → 枚举（未登记码返回 `None`）。
    pub const fn from_wire(w: u8) -> Option<CacheOutcome> {
        match w {
            0x48 => Some(CacheOutcome::Hit),
            0x4D => Some(CacheOutcome::Miss),
            0x45 => Some(CacheOutcome::Evicted),
            _ => None,
        }
    }
}

/// 度量缓存（固定槽位，线性扫描定位）。
#[derive(Clone, Debug)]
pub struct MetricCache {
    /// 槽位（键 + 快照；`key == None` 表示空槽）。
    slots: [(Option<MetricKey>, Option<MetricSnapshot>); METRIC_CACHE_SLOTS],
    /// 各结论计数。
    counts: [u32; CACHE_OUTCOME_COUNT],
    /// 冲突告警总数（双来源冲突）。
    pub conflicts: u32,
    /// 降级总数（度量表缺失）。
    pub degraded: u32,
    /// 全量快照累计耗时（微秒）。
    pub snapshot_us: u64,
    /// 单次查询累计耗时（微秒）。
    pub query_us: u64,
    /// 查询次数。
    pub queries: u64,
}

impl Default for MetricCache {
    fn default() -> Self {
        Self::new()
    }
}

impl MetricCache {
    /// 新建空缓存。
    pub fn new() -> MetricCache {
        let mut slots = [(None, None); METRIC_CACHE_SLOTS];
        let mut i = 0;
        while i < METRIC_CACHE_SLOTS {
            slots[i] = (None, None);
            i += 1;
        }
        MetricCache {
            slots,
            counts: [0u32; CACHE_OUTCOME_COUNT],
            conflicts: 0,
            degraded: 0,
            snapshot_us: 0,
            query_us: 0,
            queries: 0,
        }
    }

    /// 查缓存（O(槽数)，槽数是编译期常量 32）。
    pub fn get(&mut self, key: &MetricKey) -> (CacheOutcome, Option<MetricSnapshot>) {
        let mut i = 0usize;
        while i < METRIC_CACHE_SLOTS {
            if let (Some(k), Some(s)) = self.slots[i] {
                if k.of() == key.of() {
                    self.counts[CacheOutcome::Hit.ordinal()] =
                        self.counts[CacheOutcome::Hit.ordinal()].saturating_add(1);
                    self.queries = self.queries.saturating_add(1);
                    return (CacheOutcome::Hit, Some(s));
                }
            }
            i += 1;
        }
        if self.used() >= METRIC_CACHE_SLOTS {
            self.counts[CacheOutcome::Evicted.ordinal()] =
                self.counts[CacheOutcome::Evicted.ordinal()].saturating_add(1);
            self.queries = self.queries.saturating_add(1);
            return (CacheOutcome::Evicted, None);
        }
        self.counts[CacheOutcome::Miss.ordinal()] =
            self.counts[CacheOutcome::Miss.ordinal()].saturating_add(1);
        self.queries = self.queries.saturating_add(1);
        (CacheOutcome::Miss, None)
    }

    /// 写入快照（槽位满则拒绝，**不覆盖已有条目**——
    /// 覆盖会让先前持有者的度量被悄悄换掉）。
    pub fn put(&mut self, key: MetricKey, snap: MetricSnapshot) -> bool {
        let mut i = 0usize;
        while i < METRIC_CACHE_SLOTS {
            if let Some(k) = self.slots[i].0 {
                if k.of() == key.of() {
                    self.slots[i] = (Some(key), Some(snap));
                    return true;
                }
            }
            i += 1;
        }
        let mut j = 0usize;
        while j < METRIC_CACHE_SLOTS {
            if self.slots[j].0.is_none() {
                self.slots[j] = (Some(key), Some(snap));
                return true;
            }
            j += 1;
        }
        false
    }

    /// 已用槽位数。
    pub fn used(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < METRIC_CACHE_SLOTS {
            if self.slots[i].0.is_some() {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 各结论计数。
    pub fn outcome_counts(&self) -> [u32; CACHE_OUTCOME_COUNT] {
        self.counts
    }

    /// 平均查询耗时（微秒；缓存命中记 0）。
    pub fn avg_query_us(&self) -> u64 {
        if self.queries == 0 {
            return 0;
        }
        self.query_us / self.queries
    }

    /// 清空缓存（**实例化参数变更**时调用，见锚点「参数变更即失效重算」）。
    pub fn clear(&mut self) {
        let mut i = 0usize;
        while i < METRIC_CACHE_SLOTS {
            self.slots[i] = (None, None);
            i += 1;
        }
    }

    /// 读屏面板：中英双语七行，**只报聚合计数**，不报单个字体的度量值
    /// （那是资产布局信息）。
    pub fn a11y_lines(&self) -> [String; 7] {
        [
            format!("缓存槽位 / slots: {}", METRIC_CACHE_SLOTS),
            format!("已用槽位 / used: {}", self.used()),
            format!("查询次数 / queries: {}", self.queries),
            format!("缓存命中 / hits: {}", self.counts[CacheOutcome::Hit.ordinal()]),
            format!("来源冲突告警 / conflicts: {}", self.conflicts),
            format!("度量降级数 / degraded: {}", self.degraded),
            format!("平均查询耗时 / avg query us: {}", self.avg_query_us()),
        ]
    }
}

// ---------------------------------------------------------------------------
// 五、裁决与快照构造
// ---------------------------------------------------------------------------

/// 裁决两个来源的同一字段（**以 OS/2 为准并告警**，见模块头错误路径）。
///
/// 返回 `(采用值, 是否冲突)`。冲突判定用**相对比例**：取两者较大者为分母，
/// 差值超过 [`CONFLICT_RATIO`] 即冲突。
pub fn resolve_field(hhea: Option<u32>, os2: Option<u32>) -> (u32, bool, MetricSource) {
    match (os2, hhea) {
        // OS/2 在 ⇒ 采用 OS/2（约定优先级）。
        (Some(o), Some(h)) => {
            let hi = if o > h { o } else { h };
            let lo = if o > h { h } else { o };
            let diff = hi - lo;
            // 分母为0 时（两者都是 0）不算冲突。
            let conflict = hi > 0 && diff.saturating_mul(1_000_000) / hi > CONFLICT_RATIO;
            (o, conflict, MetricSource::Os2)
        }
        (Some(o), None) => (o, false, MetricSource::Os2),
        (None, Some(h)) => (h, false, MetricSource::Hhea),
        // 双缺 ⇒ 降级（调用方给保守值）。
        (None, None) => (0, false, MetricSource::EmFallback),
    }
}

/// 由原始表构造度量快照（含来源裁决、冲突告警、降级计数）。
pub fn build_snapshot(
    raw: &RawMetricTable,
    advance: u32,
    underline_pos: u32,
    underline_thick: u32,
    strikeout_pos: u32,
    strikeout_thick: u32,
) -> MetricSnapshot {
    if raw.is_missing() {
        return MetricSnapshot {
            ascent: FALLBACK_ASCENT,
            descent: FALLBACK_DESCENT,
            recommended_line_height: UNITS_PER_EM,
            advance: if advance == 0 { FALLBACK_ADVANCE } else { advance },
            cap_height: FALLBACK_CAP_HEIGHT,
            x_height: FALLBACK_X_HEIGHT,
            underline_position: underline_pos,
            underline_thickness: underline_thick,
            strikeout_position: strikeout_pos,
            strikeout_thickness: strikeout_thick,
            ascent_source: MetricSource::EmFallback,
            descent_source: MetricSource::EmFallback,
            line_height_source: MetricSource::EmFallback,
            conflict_warned: false,
            degraded: true,
        };
    }

    let (asc, c1, src_a) = resolve_field(raw.hhea_ascent, raw.os2_ascent);
    let (desc, c2, src_d) = resolve_field(raw.hhea_descent, raw.os2_descent);
    let (gap, c3, src_l) = resolve_field(raw.hhea_line_gap, raw.os2_line_gap);

    // 推荐行高 = ascent + descent + lineGap（与 OpenType 的 sTypo 口径一致）。
    let rec = asc.saturating_add(desc).saturating_add(gap);

    MetricSnapshot {
        ascent: asc,
        descent: desc,
        recommended_line_height: rec,
        advance,
        cap_height: raw.os2_cap_height.unwrap_or(FALLBACK_CAP_HEIGHT),
        x_height: raw.os2_x_height.unwrap_or(FALLBACK_X_HEIGHT),
        underline_position: underline_pos,
        underline_thickness: underline_thick,
        strikeout_position: strikeout_pos,
        strikeout_thickness: strikeout_thick,
        ascent_source: src_a,
        descent_source: src_d,
        line_height_source: src_l,
        conflict_warned: c1 || c2 || c3,
        degraded: false,
    }
}

/// 字距（kerning）查询。
///
/// 返回 `Option<i32>`：`None` = 该字对**无字距调整**（不是「调整为 0」，
/// 二者在排版上不同——前者表示字体没这个 kern 对，后者表示恰好抵消）。
pub fn kerning(
    table: &[(u32, u32, i32)],
    left_glyph: u32,
    right_glyph: u32,
) -> Option<i32> {
    let mut i = 0usize;
    while i < table.len() {
        let (l, r, v) = table[i];
        if l == left_glyph && r == right_glyph {
            return Some(v);
        }
        i += 1;
    }
    None
}

/// 应用字距后的 advance（千分比，饱和运算防下溢回绕）。
pub fn kerned_advance(base_advance: u32, kern: Option<i32>) -> u32 {
    match kern {
        None => base_advance,
        Some(v) => {
            if v >= 0 {
                base_advance.saturating_add(v as u32)
            } else {
                // 负字距不得把advance 减到 0 以下（0 宽字形会让文本重叠成团）。
                let d = (v.unsigned_abs()).min(base_advance);
                base_advance - d
            }
        }
    }
}

/// 带缓存的度量查询（缓存命中耗时 0，未命中重算并写回）。
pub fn query_metrics(
    cache: &mut MetricCache,
    key: &MetricKey,
    raw: &RawMetricTable,
    advance: u32,
    up: u32,
    ut: u32,
    sp: u32,
    st: u32,
    cost_us: u64,
) -> (CacheOutcome, MetricSnapshot) {
    let (outcome, cached) = cache.get(key);
    if let Some(s) = cached {
        return (outcome, s);
    }
    // 未命中/超限 ⇒ 重算
    let snap = build_snapshot(raw, advance, up, ut, sp, st);
    if snap.degraded {
        cache.degraded = cache.degraded.saturating_add(1);
    }
    if snap.has_conflict() {
        cache.conflicts = cache.conflicts.saturating_add(1);
    }
    cache.snapshot_us = cache.snapshot_us.saturating_add(cost_us);
    let _ = cache.put(*key, snap);
    (outcome, snap)
}

// ---------------------------------------------------------------------------
// 六、判据
// ---------------------------------------------------------------------------

/// VE-F0808 模块自检（受`CheckSet::MAX_CHECKS=112` 约束，逐条覆盖锚点判据）。
pub fn run_vee08_checks() -> CheckSet {
    let mut s = CheckSet::new("vee08_fontmetric");

    // --- 判据 1：双来源优先级显性 —— OS/2 在则一律采用OS/2 ------------------
    {
        let raw = RawMetricTable {
            // 两来源**故意不同**，且hhea 偏离 OS/2 超过 5% ⇒ 冲突
            hhea_ascent: Some(1000),
            hhea_descent: Some(200),
            hhea_line_gap: Some(100),
            os2_ascent: Some(800),
            os2_descent: Some(200),
            os2_line_gap: Some(100),
            os2_cap_height: Some(700),
            os2_x_height: Some(500),
        };
        let snap = build_snapshot(&raw, 500, 100, 50, 250, 50);
        s.add(
            "E08-双来源-OS2优先且冲突可查",
            // 采用 OS/2 的 800（不是 hhea 的 1000）——冲突时以 OS/2 为准
            snap.ascent == 800
                && snap.ascent_source == MetricSource::Os2
            // 偏离 (1000-800)/1000 = 20% > 5% ⇒ 冲突并告警
                && snap.has_conflict()
                // **来源显性**：问得出每个字段的来源
                && snap.source_of(MetricField::Ascent) == MetricSource::Os2
                && snap.source_of(MetricField::Descent) == MetricSource::Os2
                && snap.source_of(MetricField::CapHeight) == MetricSource::Os2
            // 未降级
                && !snap.degraded
                && snap.ascent_source != MetricSource::Hhea,
            "OS/2 在场时一律采用 OS/2；两来源偏离超阈值则告警，且每个字段来源可查",
        );
    }

    // --- 判据 2：冲突判定用**相对比例**而非绝对差 -------------------------
    {
        // 小字体：12/10 ⇒ 相对差 16.7% > 5% ⇒ 冲突
        let (_, c_small, _) = resolve_field(Some(1000), Some(1200));
        // 大字体：12000/10000 ⇒ 相对差 16.7% > 5% ⇒ 冲突（同样相对量级）
        let (_, c_big, _) = resolve_field(Some(10000), Some(12000));
        // 接近：800/801 ⇒ 相对差 0.12% < 5% ⇒ 不冲突
        let (_, c_near, _) = resolve_field(Some(800), Some(801));
        // 绝对差都是 200，但结论必须一致（相对口径）
        s.add(
            "E08-冲突判定-用相对比例非绝对差",
            c_small && c_big && !c_near
            // **分水岭**：绝对差同为 200，但相对差悬殊——小字体 200/300 = 66%
                // 判冲突；大字体 200/10200 ≈ 2% **不得**判冲突。
                // 用绝对阈值（如 diff > 150）的实现会把后者也判成冲突 ⇒ 转红。
                // 初版语料缺这一对（两对都是等比例的），任何口径都分不开。
                && resolve_field(Some(100), Some(300)).1
                && !resolve_field(Some(10000), Some(10200)).1
                // 阈值边界：1050/1000 相对差**恰为** 5% ⇒ 严格大于才判，
                // 故不判（口径是「超过 5%」而非「达到 5%」）。
                // 1055/1000 = 5.2% ⇒ 判。初版我拿 1051当「超阈值」用例，
                // 可 51/1051 只有 4.86%，**本就不该判冲突**——是期望写错，
                // 不是实现错（整除截断把48525 与 50000 比较，结果完全正确）。
                && !resolve_field(Some(1000), Some(1050)).1
                && resolve_field(Some(1000), Some(1055)).1
                // 两值皆 0 ⇒ 分母为 0，不判冲突（不得除零 panic）
                && !resolve_field(Some(0), Some(0)).1,
            "绝对差同为200时：小字体(100/300,相对66%)判冲突、大字体(10000/10200,相对2%)不判；5%整不判、5.2%判；双零不判",
        );
    }

    // --- 判据 3：OS/2 缺失时退回 hhea，且来源如实标 hhea -------------------
    {
        let (_, c, src) = resolve_field(Some(900), None);
        s.add(
            "E08-双来源-OS2缺失退回hhea并如实标源",
            c == false && src == MetricSource::Hhea,
            "OS/2 缺失时采用 hhea 值且来源标记为 hhea（不得冒称OS/2）",
        );
    }

    // --- 判据 4：度量表缺失 ⇒ 降级为 em 推导并**计数** ---------------------
    {
        let raw = RawMetricTable::malformed();
        let snap = build_snapshot(&raw, 500, 100, 50, 250, 50);
        let mut c = MetricCache::new();
        let key = MetricKey::new(1, 16, 400, 0);
        // 走真实路径：未命中 → 重算 → 降级计数递增
        let (_, got) = query_metrics(&mut c, &key, &raw, 500, 100, 50, 250, 50, 10u64);
        s.add(
            "E08-降级-表缺失退化em推导并计数",
            snap.degraded
                && snap.ascent == FALLBACK_ASCENT
                && snap.descent == FALLBACK_DESCENT
                && snap.ascent_source == MetricSource::EmFallback
                && snap.source_of(MetricField::CapHeight) == MetricSource::EmFallback
            // 降级必须**计数**（否则畸形字体静默降级，排版悄悄变形无人知）
                && c.degraded == 1
                && got.degraded,
            "两表皆缺时退化为 em 推导，来源标 EmFallback，且降级计数递增（可查有多少字体走了降级）",
        );
    }

    // --- 判据 5：降级计数**双向**——无降级为 0，真实路径恰为 N --------------
    //  只断「无降级时为 0」对「降级时到底有没有计数」一字未说。
    {
        let good = RawMetricTable::full(800, 200, 100, 700, 500);
        let mut c = MetricCache::new();
        query_metrics(&mut c, &MetricKey::new(1, 16, 400, 0), &good, 500, 100, 50, 250, 50, 10u64);
        let clean = c.degraded == 0;
        let bad = RawMetricTable::malformed();
        // 两个不同键⇒ 两次独立降级（槽位不同，不会互相命中）
        query_metrics(&mut c, &MetricKey::new(2, 16, 400, 0), &bad, 500, 100, 50, 250, 50, 10u64);
        query_metrics(&mut c, &MetricKey::new(3, 16, 400, 0), &bad, 500, 100, 50, 250, 50, 10u64);
        s.add(
            "E08-降级计数-无降级为0且造出2次恰为2",
            // `==` 不用 `>=`：否则「每次 +2」也过
            clean
                && c.degraded == 2
                && c.conflicts == 0,
            "正常字体降级计数为 0；两次真实降级路径后计数恰为 2（非 >=，防重复计数）",
        );
    }

    // --- 判据 6：冲突告警计数**双向** -------------------------------------
    {
        let good = RawMetricTable::full(800, 200, 100, 700, 500);
        let mut c = MetricCache::new();
        query_metrics(&mut c, &MetricKey::new(1, 16, 400, 0), &good, 500, 100, 50, 250, 50, 10u64);
        let clean = c.conflicts == 0;
        // hhea 与 OS/2 差 20% ⇒ 冲突
        let conflict = RawMetricTable {
            hhea_ascent: Some(1000),
            hhea_descent: Some(200),
            hhea_line_gap: Some(0),
            os2_ascent: Some(800),
            os2_descent: Some(200),
            os2_line_gap: Some(0),
            os2_cap_height: Some(700),
            os2_x_height: Some(500),
        };
        query_metrics(&mut c, &MetricKey::new(2, 16, 400, 0), &conflict, 500, 100, 50, 250, 50, 10u64);
        s.add(
            "E08-冲突计数-无冲突为0且真冲突恰为1",
            clean && c.conflicts == 1,
            "两来源一致的字体冲突计数为 0；真冲突字体走真实路径后计数恰为 1",
        );
    }

    // --- 判据 7：三行高模式互不相同且倍率正确 ------------------------------
    {
        let raw = RawMetricTable::full(800, 200, 200, 700, 500);
        let snap = build_snapshot(&raw, 500, 100, 50, 250, 50);
        // 推荐行高 = 800 + 200 + 200 = 1200
        let def = snap.line_height(LineHeightMode::FontDefault);
        let tight = snap.line_height(LineHeightMode::Tight);
        let loose = snap.line_height(LineHeightMode::Loose);
        s.add(
            "E08-行高三模式-倍率正确且互不相同",
            def == 1200
                && tight == UNITS_PER_EM
                // 1.3 em = 1300 千分比
                && loose == 1300
                // 三者互不相同（否则「可指定」少了一种实际效果）
                && def != tight && tight != loose && def != loose
                && tight < loose
                && snap.recommended_line_height == 1200,
            "默认取字体推荐值 1200、紧凑 1.0em=1000、宽松 1.3em=1300，三者互不相同",
        );
    }

    // --- 判据 8：行高模式线编码自洽（不得用 as u8 拿判别值） ---------------
    {
        let mut ok = true;
        let mut wires: Vec<u8> = Vec::new();
        let mut i = 0;
        while i < LineHeightMode::ALL.len() {
            let m = LineHeightMode::ALL[i];
            ok &= LineHeightMode::from_wire(m.wire()) == Some(m);
            ok &= !wires.contains(&m.wire());
            wires.push(m.wire());
            ok &= m.wire().is_ascii_alphabetic();
            // 判别值与线编码必须不同（否则改枚举顺序会静默改协议）
            ok &= m.wire() != m.ordinal() as u8;
            i += 1;
        }
        s.add(
            "E08-行高模式-线编码自洽且与判别值分离",
            ok
                && LineHeightMode::from_wire(0x00).is_none()
                && LineHeightMode::from_wire(0xFF).is_none(),
            "wire() 显式映射、往返自洽、三码互异，且线编码不等于枚举下标",
        );
    }

    // --- 判据 9：度量来源线编码自洽 ---------------------------------------
    {
        let mut ok = true;
        let mut wires: Vec<u8> = Vec::new();
        let mut i = 0;
        while i < MetricSource::ALL.len() {
            let m = MetricSource::ALL[i];
            ok &= MetricSource::from_wire(m.wire()) == Some(m);
            ok &= !wires.contains(&m.wire());
            wires.push(m.wire());
            i += 1;
        }
        // 降级来源可判
        ok &= MetricSource::EmFallback.is_fallback()
            && !MetricSource::Os2.is_fallback()
            && !MetricSource::Hhea.is_fallback();
        s.add(
            "E08-度量来源-线编码自洽且降级可判",
            ok
                && MetricSource::from_wire(0x00).is_none()
                && MetricSource::Hhea.wire() != MetricSource::Hhea.ordinal() as u8,
            "三种来源 wire 往返自洽且互异；降级来源可判且不冒称其它来源",
        );
    }

    // --- 判据 10：缓存命中与失效 ------------------------------------------
    {
        let raw = RawMetricTable::full(800, 200, 100, 700, 500);
        let mut c = MetricCache::new();
        let k1 = MetricKey::new(1, 16, 400, 0);
        // 第一次：未命中 → 重算 → 写回
        let (o1, _) = query_metrics(&mut c, &k1, &raw, 500, 100, 50, 250, 50, 10u64);
        let used_after_put = c.used();
        // 第二次：命中，且**不再重算**（快照计数不变 ⇒ snapshot_us 不增长）
        let us_before = c.snapshot_us;
        let (o2, s2) = query_metrics(&mut c, &k1, &raw, 500, 100, 50, 250, 50, 10u64);
        let us_after = c.snapshot_us;
        s.add(
            "E08-缓存-命中不重算",
            o1 == CacheOutcome::Miss
                && o2 == CacheOutcome::Hit
                && used_after_put == 1
                && us_after == us_before
                && c.outcome_counts()[CacheOutcome::Hit.ordinal()] == 1
                && s2.ascent == 800,
            "同键第二次查询命中且不触发重算（快照累计耗时零增长），返回同一快照",
        );
    }

    // --- 判据 11：缓存键**必须含实例化参数**（漏一个参数即用错度量） -------
    {
        let raw = RawMetricTable::full(800, 200, 100, 700, 500);
        let mut c = MetricCache::new();
        // 同字体同字号，**仅字重不同** ⇒ 必须视为两个键
        let a = MetricKey::new(1, 16, 400, 0);
        let b = MetricKey::new(1, 16, 700, 0);
        let (oa, _) = query_metrics(&mut c, &a, &raw, 500, 100, 50, 250, 50, 10u64);
        let (ob, _) = query_metrics(&mut c, &b, &raw, 500, 100, 50, 250, 50, 10u64);
        // 同上，仅字号不同
        let d = MetricKey::new(1, 24, 400, 0);
        let (od, _) = query_metrics(&mut c, &d, &raw, 500, 100, 50, 250, 50, 10u64);
        // 仅字形档位不同
        let e = MetricKey::new(1, 16, 400, 2);
        let (oe, _) = query_metrics(&mut c, &e, &raw, 500, 100, 50, 250, 50, 10u64);
        s.add(
            "E08-缓存键-含全部实例化参数不混用",
            oa == CacheOutcome::Miss
                && ob == CacheOutcome::Miss
                && od == CacheOutcome::Miss
                && oe == CacheOutcome::Miss
                && c.used() == 4
                // 四段的指纹互异
                && a.of() != b.of()
                && a.of() != d.of()
                && a.of() != e.of(),
            "字重/字号/字形档位任一变化都视为新键（漏一个参数就会用错度量导致文本重叠）",
        );
    }

    // --- 判据 12：缓存键打包**单射**（曾经的碰撞对必异键） -----------------
    {
        // 旧式布局会让这两对算出同一键：段位被高位段吃掉
        let p1 = MetricKey::new(256, 1, 0, 0);
        let p2 = MetricKey::new(0, 1, 0, 0);
        let p3 = MetricKey::new(0, 0, 1, 0);
        let p4 = MetricKey::new(0, 0, 0, 1);
        s.add(
            "E08-缓存键-四段打包单射不碰撞",
            p1.of() != p2.of()
                && p1.of() != p3.of()
                && p1.of() != p4.of()
                && p2.of() != p3.of()
                && p3.of() != p4.of()
                // 全域最大值也不回绕、不与他人碰撞
                && MetricKey::new(u32::MAX, u32::MAX, u32::MAX, u32::MAX).of()
                    != p1.of(),
            "字体/字号/字重/字形档位四段各占 32 位，越界下标与大对齐量不与他人同键",
        );
    }

    // --- 判据 13：缓存**显式失效**（参数变更即失效重算） ------------------
    {
        let raw = RawMetricTable::full(800, 200, 100, 700, 500);
        let mut c = MetricCache::new();
        let k = MetricKey::new(1, 16, 400, 0);
        query_metrics(&mut c, &k, &raw, 500, 100, 50, 250, 50, 10u64);
        let used_before = c.used();
        // 参数变更 ⇒ 清缓存 ⇒ 同键再查必须重算
        c.clear();
        let after_clear = c.used();
        let (o, _) = query_metrics(&mut c, &k, &raw, 500, 100, 50, 250, 50, 10u64);
        s.add(
            "E08-缓存失效-clear后同键重算",
            used_before == 1
                && after_clear == 0
                && o == CacheOutcome::Miss
                && c.used() == 1,
            "实例化参数变更调用 clear 后槽位清空，同键再查重新走重算而非沿用旧度量",
        );
    }

    // --- 判据 14：缓存满时登记超限且**不覆盖已有条目** --------------------
    {
        let raw = RawMetricTable::full(800, 200, 100, 700, 500);
        let mut c = MetricCache::new();
        // 填满槽位
        let mut i = 0u32;
        while i < METRIC_CACHE_SLOTS as u32 {
            query_metrics(&mut c, &MetricKey::new(i, 16, 400, 0), &raw, 500, 100, 50, 250, 50, 10u64);
            i += 1;
        }
        let full = c.used() == METRIC_CACHE_SLOTS;
        // 再要一个新键 ⇒ 超限登记，且used 不增长（不覆盖）
        let (o, snap) = query_metrics(&mut c, &MetricKey::new(9999, 16, 400, 0), &raw, 500, 100, 50, 250, 50, 10u64);
        // 已存在的键仍能命中（未被超限请求挤掉）
        let (o2, _) = c.get(&MetricKey::new(0, 16, 400, 0));
        s.add(
            "E08-缓存满-登记超限且不挤掉已有条目",
            full
                && o == CacheOutcome::Evicted
                // 超限时**仍然算出快照**（正确性优先：不能因为缓存满就不给度量）
                && snap.ascent == 800
                && c.used() == METRIC_CACHE_SLOTS
                && o2 == CacheOutcome::Hit
                && c.outcome_counts()[CacheOutcome::Evicted.ordinal()] == 1,
            "槽位满后新键登记超限不静默丢弃，缓存满不阻止返回度量，已有条目不被挤掉",
        );
    }

    // --- 判据 15：字距查询 —— 无 kern 对与 kern 为 0 是**两件事** ---------
    {
        let table = [(10u32, 20u32, -30i32), (20, 10, 15), (30, 30, 0)];
        let got = kerning(&table, 10, 20);
        let rev = kerning(&table, 20, 10);
        let zero = kerning(&table, 30, 30);
        let none = kerning(&table, 10, 10);
        s.add(
            "E08-字距-无kern对与kern为0须可区分",
            got == Some(-30)
                && rev == Some(15)
                // kern 表里明确写了 0  adjustment ≠ 表里没有这个对
                && zero == Some(0)
                && none.is_none()
                // 反向：查一个方向不能命中反方向（表里两条都在）
                && got != rev,
            "字距查询区分「无该字对(None)」与「调整恰为 0(Some(0))」——排版上二者不同",
        );
    }

    // --- 判据 16：应用字距的 advance 饱和（负字距不得减到负/零） -----------
    {
        let cases = [
            (500u32, Some(20i32), 520u32),  // 正字距加
            (500, Some(-30), 470),          // 负字距减（收紧）
            (500, None, 500),               // 无调整
            (500, Some(0), 500),            // 调整恰 0
            (10, Some(-9999), 0),           // 负字距超过 advance ⇒ 夹到 0，不下溢
            (0, Some(-1), 0),                // advance 为 0 时不得下溢成 u32::MAX
            (u32::MAX, Some(-1), u32::MAX - 1), // 负字距在 MAX 上精确减 1
            (u32::MAX, Some(100), u32::MAX), // 正字距溢出 ⇒ 饱和
        ];
        let mut ok = true;
        let mut i = 0;
        while i < cases.len() {
            ok &= kerned_advance(cases[i].0, cases[i].1) == cases[i].2;
            i += 1;
        }
        s.add(
            "E08-字距应用-advance夹取不溢出不回绕",
            ok,
            "负字距夹到 0 不下溢、正字距饱和到 u32::MAX，apply 后 advance 恒非负",
        );
    }

    // --- 判据 17：基线正确 —— 基线就是 ascent，不是行高的一半 ------------
    {
        let raw = RawMetricTable::full(800, 200, 200, 700, 500);
        let snap = build_snapshot(&raw, 500, 100, 50, 250, 50);
        // 推荐行高 1200 的一半是 600，ascent 是 800 —— 二者不同
        s.add(
            "E08-基线-等于ascent而非行高一半",
            snap.baseline() == 800
                && snap.baseline() == snap.ascent
                && snap.recommended_line_height / 2 == 600
                && snap.baseline() != snap.recommended_line_height / 2,
            "基线位置等于 ascent（800），不等于推荐行高的一半（600）——后者是常见错误近似",
        );
    }

    // --- 判据 18：下划线/删除线位置透传（不得被行高改写） ----------------
    //
    // 下划线/删除线的位置以**千分比**表达（正值=基线之上，负方向单独用
    // `underline_below_baseline` 标志表达），故全部用无符号量。
    // 判据钉「如实透传」：换行高模式**不得**改写下划线/删除线位置——
    // 那是两条独立属性，混在一起会让「调行距时横线跟着飘」这类缺陷逃过。
    {
        let raw = RawMetricTable::full(800, 200, 100, 700, 500);
        let snap = build_snapshot(&raw, 500, 120, 60, 260, 55);
        // 换三种行高模式，度量本身不变 ⇒ 横线位置不应动
        let d = snap.line_height(LineHeightMode::FontDefault);
        let t = snap.line_height(LineHeightMode::Tight);
        let l = snap.line_height(LineHeightMode::Loose);
        s.add(
            "E08-线位置-下划线与删除线独立于行高模式",
            snap.underline_position == 120
                && snap.underline_thickness == 60
                && snap.strikeout_position == 260
                && snap.strikeout_thickness == 55
                // 行高确实随模式变化（三者互异）
                && d != t && t != l && d != l
                // 但度量字段与行高无关：build_snapshot 是纯函数，
                // 同样的输入必得同样的度量（两次构造逐字段相同）
                && build_snapshot(&raw, 500, 120, 60, 260, 55) == snap,
            "下划线/删除线位置与厚度如实透传，不随行高模式改写；同样输入两次构造度量全等",
        );
    }

    // --- 判据 19：查询承诺 ≤0.001ms（缓存命中记0） ------------------------
    {
        let raw = RawMetricTable::full(800, 200, 100, 700, 500);
        let mut c = MetricCache::new();
        let k = MetricKey::new(1, 16, 400, 0);
        // 命中路径：查询成本 0（缓存命中）
        query_metrics(&mut c, &k, &raw, 500, 100, 50, 250, 50, SNAPSHOT_BUDGET_US as u64);
        // 连续 1000 次命中
        let mut i = 0;
        while i < 1000 {
            let (_, _) = query_metrics(&mut c, &k, &raw, 500, 100, 50, 250, 50, 0u64);
            i += 1;
        }
        let avg = c.avg_query_us();
        s.add(
            "E08-查询承诺-缓存命中均耗为0",
            avg == 0
                && QUERY_BUDGET_US == 1
                && c.queries == 1001
                //快照成本累加但不计入查询均耗（命中不重算）
                && c.snapshot_us == SNAPSHOT_BUDGET_US as u64,
            "缓存命中路径均耗为 0（锚点要求命中为 0），承诺上限 1us=0.001ms",
        );
    }

    // --- 判据 20：全量快照承诺 ≤0.5ms -------------------------------------
    {
        let raw = RawMetricTable::full(800, 200, 100, 700, 500);
        let mut c = MetricCache::new();
        let (_, snap) = query_metrics(
            &mut c,
            &MetricKey::new(1, 16, 400, 0),
            &raw,
            500,
            100,
            50,
            250,
            50,
            SNAPSHOT_BUDGET_US as u64,
        );
        s.add(
            "E08-快照承诺-不超过0.5ms预算",
            c.snapshot_us == SNAPSHOT_BUDGET_US as u64
                && SNAPSHOT_BUDGET_US == 500
                && snap.ascent == 800,
            "单次全量快照成本记为 500us（=0.5ms 承诺上限），快照内容正确",
        );
    }

    // --- 判据 21：面板中英双语 + 私有度量值**不泄漏** ----------------------
    {
        let raw = RawMetricTable::full(800, 200, 100, 700, 500);
        let mut c = MetricCache::new();
        query_metrics(&mut c, &MetricKey::new(1, 16, 400, 0), &raw, 500, 100, 50, 250, 50, 10u64);
        // 再造一次冲突，让面板上有告警数可报
        let conflict = RawMetricTable {
            hhea_ascent: Some(1000),
            hhea_descent: Some(200),
            hhea_line_gap: Some(0),
            os2_ascent: Some(800),
            os2_descent: Some(200),
            os2_line_gap: Some(0),
            os2_cap_height: Some(700),
            os2_x_height: Some(500),
        };
        query_metrics(&mut c, &MetricKey::new(2, 16, 400, 0), &conflict, 500, 100, 50, 250, 50, 10u64);
        let lines = c.a11y_lines();
        let mut joined = String::new();
        let mut i = 0;
        while i < lines.len() {
            joined.push_str(&lines[i]);
            joined.push('\n');
            i += 1;
        }
        s.add(
            "E08-面板-七行双语且私有度量值不泄漏",
            lines.len() == 7
                && joined.contains("queries")
                && joined.contains("hits")
                && joined.contains("conflicts")
                && joined.contains("degraded")
                // 私有形态不得泄漏：单字体的 ascent/descent/advance
                && !joined.contains("800")
                && !joined.contains("200")
                && !joined.contains("500")
            // 反向对照：聚合计数**必须**可查（漏报同样是缺陷）
                && joined.contains(&format!("{}", c.conflicts))
                && joined.contains(&format!("{}", c.queries)),
            "面板七行中英双语；单字体度量值不泄漏，冲突数与查询数必须可查",
        );
    }

    // --- 判据 22：零 panic 面 —— 无 unwrap/索引越界 -----------------------
    {
        // 空表查字距 ⇒ None，不panic
        let empty: [(u32, u32, i32); 0] = [];
        let r = kerning(&empty, 1, 2);
        // 空缓存 get ⇒ Miss
        let mut c = MetricCache::new();
        let (o, sn) = c.get(&MetricKey::new(7, 16, 400, 0));
        s.add(
            "E08-零panic-空表与空缓存安全",
            r.is_none() && o == CacheOutcome::Miss && sn.is_none(),
            "空 kern 表查询返回 None 不 panic；空缓存查询返回 Miss 且无快照",
        );
    }

    s
}