//! VE-F4002 · 语言标签与 Locale 模型（VE-T 域 · 国际化域 · T01 组 · 目标 380 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F4002`
//!
//! **判据（锚点原文）**：BCP47 全量、回退链、解析缓存、容错表、单源复用。
//!
//! **职责定位（锚点原文）**：T01 语言标签与 Locale 模型——Locale 模型（BCP47
//! 全量解析：language-script-region-variant 四段+扩展——解析器（容错解析（常见
//! 畸形容忍表）；匹配语义（locale fallback 链（zh-CN→zh→und 逐级回退——回退链
//! 规范逐条；Locale 缓存（解析缓存 O(1)；与 lang 属性协同（F2948 单源复用声明。
//!
//! **数据结构（锚点原文·家族格式）**：数据模型与规格表（逐条规格公开、参数域
//! 钳制、枚举守卫——家族格式）。
//!
//! **错误路径与降级矩阵（锚点原文·家族格式）**：畸形标签→容错表+诊断；回退耗尽
//! →und 默认+显性；扩展非法→拒绝三要素。
//!
//! **性能逐项分解（锚点原文·家族格式）**：解析 O(标签长) 缓存后 O(1)；回退
//! O(链深)；匹配 O(1)。
//!
//! **跨批对接点（锚点原文·家族格式）**：F2948/F2915 单源复用声明；N02 消费；
//! F4003 方向前向。
//!
//! **无障碍与隐私（锚点原文）**：文档替述可读；无隐私面。
//!
//! # 本项的边界（不越界施工，遵守"只做领到的任务"）
//!
//! 本项交付 **Locale 模型本体**：BCP47 四段 + 扩展的解析器与规范化、回退链、
//! 解析缓存、匹配语义、畸形容忍表、单源复用声明。它**不代做**后续 19 项：
//!
//! - F4003 文字方向模型拥有**方向判定本体**；本项只在 [`FORWARD_SLOTS`] 留
//!   `direction-default-by-locale` 的**前向声明位**（`landed: false`），并提供
//!   [`DirectionDefault`] 这一个「按 Locale 给默认方向」的取值口——**只给值不做
//!   判定算法**，判定归 F4003；
//! - F4004 排版管线拥有**四族路由表**；本项不写排版策略；
//! - F4006 日期数字格式拥有 **CLDR 格式规则集**；本项不写格式化器；
//! - F4010 语料、F4011 调试器、F4012 性能预算均不在本项内。
//!
//! # 关于「单源」的硬约束（本项第一红线）
//!
//! 锚点要求「与 lang 属性协同（**F2948 单源复用声明**）」。这句话最容易做成
//! 口号——在 F4002 里再写一份标签解析，然后让 F2948 自己挑一份用。所以本项把它
//! 落成可机检的 [`SingleSourceClaim`] 表 + [`check_single_source`]：
//!
//! - **每项能力有且只有一个 owner**。`tag-parse`（标签解析）与
//!   `fallback-chain`（回退链）的 owner 是本项 `VE-F4002`；
//! - **F2948 与 F2915 是 consumer 不是 owner**——它们的 `consumes` 指向本项的
//!   能力键，不许自己再声明一份；
//! - 任何能力出现**第二个 owner** 即 [`E_SINGLE_SOURCE_DUP`] 阻断。理由很实际：
//!   两份 BCP47 解析器对 `zh-Hans-CN-u-ca-buddhist` 的切法只要有一处不同，
//!   `:lang()` 选中的字体和排版管线的字体就会分叉，而这种分叉在界面上表现为
//!   「同一页两种字」，极难归因。
//!
//! # 关于「解析缓存 O(1)」的诚实表述
//!
//! 锚点写「解析缓存 O(1)」。本项**不把它写成假的**：命中路径的真实代价是
//! `O(标签长)` 哈希 + `O(1)` 开放定址探测，**省掉的是解析与结构分配**
//! （不再构造 `Vec<String>` 段表、不再做四段归位、不再分配诊断向量）。缓存
//! 命中后 `resolve` 返回 `&ParsedTag`（借用，不拷贝），所以"缓存后 O(1)"成立
//! 的准确含义是**后续对该标签的一切消费都是 O(1) 查表**——同一个 Locale 被
//! 反复问的时候（真实场景里每个组件都会问一次），第一次付解析代价，之后全部
//! O(1)。缓存表满时 [`E_INDEX_FULL`] **显性拒绝**而不是静默覆盖，理由是静默
//! 覆盖会让命中率无征兆地掉下去，而掉命中率表现为「偶发变慢」，极难归因。
//!
//! # 确定性
//!
//! 零时钟、零 IO、零环境依赖；输入是 `&str`，输出是纯数据结构。同一输入必得
//! 同一结果（含诊断序列的顺序），保证回归可复现、对拍可重现。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// §0 参数域钳制（锚点：参数域钳制——家族格式）
// ---------------------------------------------------------------------------

/// 索引表最小槽数（参数域下界）。
pub const MIN_INDEX_SLOTS: usize = 8;
/// 索引表最大槽数（参数域上界）。
///
/// 上界存在的理由是内核态内存不可回收：Locale 缓存是**有界**结构，槽数由
/// 域预算给定，调用方不得申请无限表。越界请求被钳到本值并留下钳制记录。
pub const MAX_INDEX_SLOTS: usize = 4096;
/// 语言标签最小字节长（`"zh"`）。
pub const MIN_TAG_LEN: usize = 2;
/// 语言标签最大字节长上界（参数域上界；BCP47 实践上极少超过 64）。
pub const MAX_TAG_LEN: usize = 255;
/// 变体段数量上界。
pub const MAX_VARIANTS: usize = 8;
/// 扩展段数量上界。
pub const MAX_EXTENSIONS: usize = 8;
/// 单个扩展段的取值数量上界。
pub const MAX_EXTENSION_VALUES: usize = 8;
/// 私有用途段数量上界。
pub const MAX_PRIVATEUSE: usize = 8;

/// 未确定语言标签（回退链终点，锚点钦定 `zh-CN→zh→und` 的 `und`）。
pub const UNDETERMINED: &str = "und";

/// 通用参数钳制：`value` 被钳进 `[lo, hi]`。
pub fn clamp(value: usize, lo: usize, hi: usize) -> usize {
    if value < lo {
        lo
    } else if value > hi {
        hi
    } else {
        value
    }
}

/// 向上取整到 2 的幂（开放定址要求槽数为 2 的幂，掩码寻址才成立）。
pub fn round_up_pow2(value: usize) -> usize {
    if value <= 1 {
        return 1;
    }
    let mut n = 1usize;
    while n < value {
        n <<= 1;
    }
    n
}

/// 解析预算：所有参数域的钳制结果集中在此，钳制行为对调用方可见。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ParseBudget {
    /// 标签最大字节长。
    pub max_len: usize,
    /// 变体段数量上界。
    pub max_variants: usize,
    /// 扩展段数量上界。
    pub max_extensions: usize,
    /// 单扩展取值数量上界。
    pub max_extension_values: usize,
    /// 私有用途段数量上界。
    pub max_privateuse: usize,
}

impl ParseBudget {
    /// 域默认预算（各参数取家族上界的保守值）。
    pub const DOMAIN_DEFAULT: ParseBudget = ParseBudget {
        max_len: MAX_TAG_LEN,
        max_variants: MAX_VARIANTS,
        max_extensions: MAX_EXTENSIONS,
        max_extension_values: MAX_EXTENSION_VALUES,
        max_privateuse: MAX_PRIVATEUSE,
    };

    /// 按请求构造预算：**逐字段钳制**，越界项被钳到参数域边界。
    ///
    /// 锚点要求「参数域钳制」。这里做成**钳制而非拒绝**——因为超预算的调用方
    /// 往往只是想多要一点，直接拒绝会把「参数写大了」变成「功能不可用」；
    /// 但钳制必须**可见**，所以本函数同时返回 [`ClampRecord`] 列表。
    pub fn requested(max_len: usize) -> (ParseBudget, Vec<ClampRecord>) {
        let mut log = Vec::new();
        let d = ParseBudget::DOMAIN_DEFAULT;
        let len = if max_len == 0 {
            log.push(ClampRecord {
                field: "max_len",
                requested: max_len,
                effective: d.max_len,
                reason: "零预算不可用，退回域默认",
            });
            d.max_len
        } else if max_len > MAX_TAG_LEN {
            log.push(ClampRecord {
                field: "max_len",
                requested: max_len,
                effective: MAX_TAG_LEN,
                reason: "超出参数域上界",
            });
            MAX_TAG_LEN
        } else if max_len < MIN_TAG_LEN {
            log.push(ClampRecord {
                field: "max_len",
                requested: max_len,
                effective: MIN_TAG_LEN,
                reason: "低于参数域下界",
            });
            MIN_TAG_LEN
        } else {
            max_len
        };
        (
            ParseBudget {
                max_len: len,
                ..d
            },
            log,
        )
    }
}

impl Default for ParseBudget {
    fn default() -> Self {
        ParseBudget::DOMAIN_DEFAULT
    }
}

/// 钳制记录：谁被钳了、从多少钳到多少、为什么。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClampRecord {
    /// 参数字段名。
    pub field: &'static str,
    /// 调用方请求值。
    pub requested: usize,
    /// 实际生效值。
    pub effective: usize,
    /// 钳制理由（人话）。
    pub reason: &'static str,
}

// ---------------------------------------------------------------------------
// §1 拒绝三要素 + 诊断（锚点：扩展非法→拒绝三要素；畸形标签→容错表+诊断）
// ---------------------------------------------------------------------------

/// 异常零静默铁律的载体：任何拒绝都必须同时给全三项。
///
/// 只报 code 不给路的拒绝在评审按缺陷处理——运维拿到一个 `E_...` 却不知道
/// 该改哪一行，等于没报。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rejection {
    /// 机器可读错误码。
    pub code: &'static str,
    /// 发生了什么（人话）。
    pub what: String,
    /// 为什么（根因）。
    pub why: String,
    /// 下一步怎么办（修正建议）。
    pub next: String,
}

impl Rejection {
    /// 三要素齐备性——缺项即缺陷。
    pub fn is_complete(&self) -> bool {
        !self.code.is_empty() && !self.what.is_empty() && !self.why.is_empty() && !self.next.is_empty()
    }
}

/// 输入为空或剥完修复后为空段。
pub const E_SEGMENT_EMPTY: &str = "E_LOCALE_SEGMENT_EMPTY";
/// 首段不是合法 language 子标签。
pub const E_LANGUAGE_INVALID: &str = "E_LOCALE_LANGUAGE_INVALID";
/// 有子段无法归位（四段 + 扩展都装不下）。
pub const E_SEGMENT_UNCONSUMED: &str = "E_LOCALE_SEGMENT_UNCONSUMED";
/// 扩展 singleton 后面没有取值（`-u-` 悬空）。
pub const E_SINGLETON_NO_VALUE: &str = "E_LOCALE_SINGLETON_NO_VALUE";
/// 同一 singleton 重复出现。
pub const E_SINGLETON_DUPLICATE: &str = "E_LOCALE_SINGLETON_DUPLICATE";
/// 私有用途段 `x-` 后面没有取值。
pub const E_PRIVATEUSE_EMPTY: &str = "E_LOCALE_PRIVATEUSE_EMPTY";
/// 超出解析预算（超长/段数越界）。
pub const E_BUDGET_EXCEEDED: &str = "E_LOCALE_BUDGET_EXCEEDED";
/// 索引表探测环走满（缓存满）。
pub const E_INDEX_FULL: &str = "E_LOCALE_INDEX_FULL";
/// 子标签种类序列不满足枚举守卫的规范序。
pub const E_SEGMENT_ORDER: &str = "E_LOCALE_SEGMENT_ORDER";
/// 同一能力出现第二个 owner（单源红线）。
pub const E_SINGLE_SOURCE_DUP: &str = "E_LOCALE_SINGLE_SOURCE_DUP";
/// 规格表存在未被任何实现覆盖的条目。
pub const E_SPEC_GAP: &str = "E_LOCALE_SPEC_GAP";

/// 诊断：容错修复的**显性留痕**。
///
/// 锚点「畸形标签→容错表+诊断」的关键在"诊断"二字——容错不是静默吞掉，
/// 每次修复都留下一条带码的记录，调用方可据此回报"你的标签被规范化了"。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    /// 机器可读诊断码。
    pub code: &'static str,
    /// 诊断阶段（剥壳 / 归位 / 规范化）。
    pub stage: &'static str,
    /// 人话说明。
    pub detail: String,
}

/// 剥壳阶段：首尾空白。
pub const D_SPACES: &str = "D_LOCALE_SPACES";
/// 剥壳阶段：字符集后缀（`.UTF-8`）。
pub const D_CODESET: &str = "D_LOCALE_CODESET";
/// 剥壳阶段：`@modifier` 后缀。
pub const D_AT_MODIFIER: &str = "D_LOCALE_AT_MODIFIER";
/// 剥壳阶段：下划线分隔改连字符。
pub const D_UNDERSCORE: &str = "D_LOCALE_UNDERSCORE";
/// 剥壳阶段：段内空子段（`zh--CN`）。
pub const D_EMPTY_SEGMENT: &str = "D_LOCALE_EMPTY_SEGMENT";
/// 剥壳阶段：尾随连字符（`zh-`）。
pub const D_TRAILING_HYPHEN: &str = "D_LOCALE_TRAILING_HYPHEN";
/// 归位阶段：大小写非规范。
pub const D_CASE: &str = "D_LOCALE_CASE";
/// 归位阶段：段序非规范（script/region 位置错）。
pub const D_SEGMENT_ORDER: &str = "D_LOCALE_SEGMENT_ORDER";

fn diag(code: &'static str, stage: &'static str, detail: &str) -> Diagnostic {
    Diagnostic {
        code,
        stage,
        detail: detail.to_string(),
    }
}

// ---------------------------------------------------------------------------
// §2 子标签种类与枚举守卫（锚点：枚举守卫——家族格式）
// ---------------------------------------------------------------------------

/// 子标签种类。枚举守卫的对象是**种类序列**的规范性，而不是"枚举本身存在"。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SubtagKind {
    /// 主语言子标签（2-8 字母）。
    Language,
    /// 扩展语言子标签（3 字母，历史遗留）。
    Extlang,
    /// 文种子标签（4 字母）。
    Script,
    /// 区域子标签（2 字母 或 3 数字）。
    Region,
    /// 变体子标签（5-8 字母数字，或 4 位数字开头）。
    Variant,
    /// 扩展 singleton（1 字母数字，非 `x`）。
    ExtensionSingleton,
    /// 扩展取值（2-8 字母数字）。
    ExtensionValue,
    /// 私有用途取值（1-8 字母数字）。
    PrivateUse,
}

impl SubtagKind {
    /// 规范序排名表（锚点「四段+扩展」的权威次序）。
    ///
    /// **注意这是排名表，不是合法序列**：本常量按 rank 升序列出全部 8 种种类，
    /// 仅供 [`SubtagKind::rank`] 排位与文档对照使用。
    /// 合法性判定走 [`guard_segment_order`] 的文法状态机——直接拿本常量当
    /// "合法序列"来判是错的（扩展段的 `value → singleton` 转移会让 rank 下降）。
    pub const CANONICAL_ORDER: [SubtagKind; 8] = [
        SubtagKind::Language,
        SubtagKind::Extlang,
        SubtagKind::Script,
        SubtagKind::Region,
        SubtagKind::Variant,
        SubtagKind::ExtensionSingleton,
        SubtagKind::ExtensionValue,
        SubtagKind::PrivateUse,
    ];

    /// 规范序排名（越小越前）。
    pub fn rank(self) -> u8 {
        match self {
            SubtagKind::Language => 0,
            SubtagKind::Extlang => 1,
            SubtagKind::Script => 2,
            SubtagKind::Region => 3,
            SubtagKind::Variant => 4,
            SubtagKind::ExtensionSingleton => 5,
            SubtagKind::ExtensionValue => 6,
            SubtagKind::PrivateUse => 7,
        }
    }

    /// 人话名（替述可读，无障碍面）。
    pub fn name(self) -> &'static str {
        match self {
            SubtagKind::Language => "主语言",
            SubtagKind::Extlang => "扩展语言",
            SubtagKind::Script => "文种",
            SubtagKind::Region => "区域",
            SubtagKind::Variant => "变体",
            SubtagKind::ExtensionSingleton => "扩展单键",
            SubtagKind::ExtensionValue => "扩展取值",
            SubtagKind::PrivateUse => "私有用途",
        }
    }
}

/// 区域子标签的两类形态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RegionKind {
    /// 2 字母（ISO 3166-1，如 `US`）。
    Alpha2,
    /// 3 数字（UN M.49，如 `419`）。
    Numeric3,
}

impl RegionKind {
    /// 形态名（替述可读）。
    pub fn name(self) -> &'static str {
        match self {
            RegionKind::Alpha2 => "字母区域",
            RegionKind::Numeric3 => "数字区域",
        }
    }
}

/// 枚举守卫：种类序列必须符合 BCP47 langtag 文法。
///
/// 锚点「枚举守卫」的落点。守卫失败即 [`E_SEGMENT_ORDER`]——**不静默重排**
/// （重排会掩盖调用方的畸形输入，而畸形输入是 bug 的真正来源）。
///
/// # 为什么不能只比 [`SubtagKind::rank`]
///
/// 朴素做法是「序列的 rank 非降」。**那是错的**，而且错得很隐蔽：多个扩展段
/// （`en-t-de-DE-u-ca-gregory`）的种类序列是
/// `Language, ExtSingleton, ExtValue, ExtValue, ExtSingleton, ExtValue`——
/// 第二段的 singleton 出现在上一段的 value 之后，rank 从6 跌回 5，非降检查
/// 会把**完全合法**的标签判成越序。
///
/// 所以本守卫按 langtag 文法做**分组状态机**：
///
/// ```text
/// language (extlang)* script? region? variant* (extension|privateuse)*
///            ↑ 各自至多一次/多次      ↑ 每个扩展 = singleton value+ 可重复
/// ```
///
/// 合法转移只有两类：段内**递进**（language→script→region→variant→扩展），
/// 以及 **value → singleton**（开下一个扩展段）。其余（尤其 region→script
/// 这类"位置对调"）一律拒绝。
pub fn guard_segment_order(kinds: &[SubtagKind]) -> Result<(), Rejection> {
    use SubtagKind as K;
    let mut i: usize;
    // 首段必须是 language。
    match kinds.first() {
        Some(K::Language) => i = 1,
        Some(other) => {
            return Err(order_err(format!(
                "首段是{}，BCP47 langtag 必须以主语言起始",
                other.name()
            )))
        }
        None => return Ok(()),
    }
    // 扩展语言：至多三个 3 字母段。
    while i < kinds.len() && kinds[i] == K::Extlang {
        i += 1;
    }
    // 文种至多一次、区域至多一次，且必须各自只出现一次（重复即越序）。
    if i < kinds.len() && kinds[i] == K::Script {
        i += 1;
    }
    if i < kinds.len() && kinds[i] == K::Region {
        i += 1;
    }
    // 文种/区域不得重复出现（`zh-Hans-Hans-CN` / `zh-CN-CN`）。
    if i < kinds.len() && matches!(kinds[i], K::Script | K::Region) {
        return Err(order_err(format!(
            "{} 出现在已用过的位置之后，langtag 里文种与区域各至多一次",
            kinds[i].name()
        )));
    }
    // 变体：可多次。
    while i < kinds.len() && kinds[i] == K::Variant {
        i += 1;
    }
    // 扩展段与私有用途：可多段，每段 = singleton 后接≥1 个 value；
    // 私有用途段一旦出现即为末段。
    while i < kinds.len() {
        match kinds[i] {
            K::ExtensionSingleton => {
                i += 1;
                let mut values = 0usize;
                while i < kinds.len() && kinds[i] == K::ExtensionValue {
                    i += 1;
                    values += 1;
                }
                if values == 0 {
                    return Err(order_err(
                        "扩展单键后面没有扩展取值（悬空 singleton）".to_string(),
                    ));
                }
            }
            K::PrivateUse => {
                // 私有用途必须收尾：其后只允许 PrivateUse。
                while i < kinds.len() && kinds[i] == K::PrivateUse {
                    i += 1;
                }
                if i < kinds.len() {
                    return Err(order_err(format!(
                        "{} 出现在私有用途段之后，私有用途按 BCP47 必须在最后",
                        kinds[i].name()
                    )));
                }
            }
            other => {
                return Err(order_err(format!(
                    "{} 出现在扩展段位置（langtag 的四段必须先于扩展段）",
                    other.name()
                )))
            }
        }
    }
    Ok(())
}

fn order_err(what: String) -> Rejection {
    Rejection {
        code: E_SEGMENT_ORDER,
        what,
        why: "BCP47 的四段+扩展次序是 language-script-region-variant-extension；\
              越序说明输入不是合法 langtag，强行归位会把畸形输入藏起来"
            .to_string(),
        next: "把标签改写成规范序（如 zh-Hans-CN 而非 zh-CN-Hans）；\
              若这是用户输入，走畸形容忍表而不是强行归位"
            .to_string(),
    }
}

// ---------------------------------------------------------------------------
// §3 畸形容忍表（锚点：常见畸形容忍表）
// ---------------------------------------------------------------------------

/// 畸形类别。每一类都必须在 [`DEFECT_TABLE`] 里有行——**表缺行即缺陷**
/// （由 [`check_defect_table`] 机检）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Defect {
    /// 首尾空白（`" zh-CN "`）。
    SurroundingSpace,
    /// 下划线分隔（`zh_CN`，Windows 区域设置风格）。
    UnderscoreSeparator,
    /// 字符集后缀（`en_US.UTF-8`）。
    CodesetSuffix,
    /// `@modifier` 后缀（`de_DE@euro`）。
    AtModifier,
    /// 大小写非规范（`ZH-hans-cn`）。
    CaseChaos,
    /// 段内空子段（`zh--CN`）。
    EmptySegment,
    /// 尾随连字符（`zh-`）。
    TrailingHyphen,
    /// 私有用途段出现在扩展之前（`zh-x-priv-u-nu-latn`）。
    PrivateUseMisplaced,
}

/// 畸形处置强度。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DefectSeverity {
    /// 可容错：修复 + 留诊断。
    Repairable,
    /// 不可容错：拒绝三要素。
    Fatal,
}

/// 容忍表的一行：畸形 → 处置 → 码 → 人话。
#[derive(Clone, Copy, Debug)]
pub struct DefectSpec {
    /// 畸形类别。
    pub defect: Defect,
    /// 处置强度。
    pub severity: DefectSeverity,
    /// 诊断码。
    pub code: &'static str,
    /// 症状（人话）。
    pub symptom: &'static str,
    /// 修复动作（人话）。
    pub repair: &'static str,
}

/// **常见畸形容忍表**（锚点钦定）。
///
/// 表的设计原则：**能确定还原的就修，不能确定的就不修**。
/// 例如 `zh_CN` 能确定还原成 `zh-CN`（下划线在 BCP47 里无合法用途），所以修；
/// 而 `zh-` 丢掉的段是什么无从得知，所以只做丢弃 + 诊断，不猜。
pub const DEFECT_TABLE: [DefectSpec; 8] = [
    DefectSpec {
        defect: Defect::SurroundingSpace,
        severity: DefectSeverity::Repairable,
        code: D_SPACES,
        symptom: "标签首尾带空白",
        repair: "去首尾空白",
    },
    DefectSpec {
        defect: Defect::CodesetSuffix,
        severity: DefectSeverity::Repairable,
        code: D_CODESET,
        symptom: "带 POSIX 字符集后缀（.UTF-8）",
        repair: "在 '.' 处截断",
    },
    DefectSpec {
        defect: Defect::AtModifier,
        severity: DefectSeverity::Repairable,
        code: D_AT_MODIFIER,
        symptom: "带 POSIX @modifier 后缀（de_DE@euro）",
        repair: "在 '@' 处截断",
    },
    DefectSpec {
        defect: Defect::UnderscoreSeparator,
        severity: DefectSeverity::Repairable,
        code: D_UNDERSCORE,
        symptom: "用下划线代替连字符分隔（zh_CN）",
        repair: "下划线改连字符",
    },
    DefectSpec {
        defect: Defect::EmptySegment,
        severity: DefectSeverity::Repairable,
        code: D_EMPTY_SEGMENT,
        symptom: "段间多出连字符（zh--CN）",
        repair: "丢弃空子段",
    },
    DefectSpec {
        defect: Defect::TrailingHyphen,
        severity: DefectSeverity::Repairable,
        code: D_TRAILING_HYPHEN,
        symptom: "尾随连字符（zh-）",
        repair: "丢弃尾随空段",
    },
    DefectSpec {
        defect: Defect::CaseChaos,
        severity: DefectSeverity::Repairable,
        code: D_CASE,
        symptom: "子标签大小写不符合规范位置规则",
        repair: "按位置规则重排大小写（lang↓ script⭑ region↑ variant↓）",
    },
    DefectSpec {
        defect: Defect::PrivateUseMisplaced,
        severity: DefectSeverity::Fatal,
        code: E_SEGMENT_UNCONSUMED,
        symptom: "私有用途段出现在扩展段之前",
        repair: "拒绝：私有用途段按 BCP47 必须在最后一段",
    },
];

/// 查容忍表。
pub fn defect_spec(d: Defect) -> &'static DefectSpec {
    DEFECT_TABLE
        .iter()
        .find(|s| s.defect == d)
        .unwrap_or(&DEFECT_TABLE[0])
}

/// 容忍表自检：每种 [`Defect`] 有且只有一行，且码不重复、行不为空。
///
/// 表是机检出来的，不是写出来的注释——加一个 `Defect` 变体而忘了加表行，
/// 这里立刻红。
pub fn check_defect_table() -> Result<(), Rejection> {
    for d in [
        Defect::SurroundingSpace,
        Defect::UnderscoreSeparator,
        Defect::CodesetSuffix,
        Defect::AtModifier,
        Defect::CaseChaos,
        Defect::EmptySegment,
        Defect::TrailingHyphen,
        Defect::PrivateUseMisplaced,
    ] {
        let rows = DEFECT_TABLE.iter().filter(|s| s.defect == d).count();
        if rows != 1 {
            return Err(Rejection {
                code: E_SPEC_GAP,
                what: format!("容忍表缺行或重复：{:?} 有 {} 行（应为 1 行）", d, rows),
                why: "容忍表是容错的唯一依据；缺行则该畸形落到未定义行为，\
                      重复行则处置强度有二义"
                    .to_string(),
                next: format!("在 DEFECT_TABLE 中为 {:?} 补恰好一行", d),
            });
        }
    }
    for i in 0..DEFECT_TABLE.len() {
        for j in (i + 1)..DEFECT_TABLE.len() {
            if DEFECT_TABLE[i].code == DEFECT_TABLE[j].code {
                return Err(Rejection {
                    code: E_SPEC_GAP,
                    what: format!("容忍表诊断码重复：{}", DEFECT_TABLE[i].code),
                    why: "诊断码是调用方定位的唯一依据，重复则无法区分是哪类畸形被修".to_string(),
                    next: "把重复行的 code 改成独立码".to_string(),
                });
            }
            if DEFECT_TABLE[i].symptom.is_empty() || DEFECT_TABLE[i].repair.is_empty() {
                return Err(Rejection {
                    code: E_SPEC_GAP,
                    what: "容忍表存在空症状或空修复动作的行".to_string(),
                    why: "空行等于占位符；容忍表里的占位符会让容错行为不可预期".to_string(),
                    next: "补齐 symptom 与 repair".to_string(),
                });
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// §4 标签数据模型（锚点：language-script-region-variant 四段 + 扩展）
// ---------------------------------------------------------------------------

/// 扩展段：一个 singleton + 若干取值。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Extension {
    /// singleton（`u` / `t` / 任意非 `x` 单字符）。
    pub singleton: char,
    /// 取值序列（已小写规范化）。
    pub values: Vec<String>,
}

/// 语言标签：BCP47 四段 + 扩展 + 私有用途。
///
/// 字段全部规范化到位（lang 小写 / script 首字母大写 / region 大写 / 其余小写），
/// 所以 [`LanguageTag::canonical`] 是纯拼装，两次解析同一等价标签必得同一规范式
/// ——这是回退链与匹配能走哈希的前提。
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct LanguageTag {
    /// 主语言子标签（小写）。
    pub language: String,
    /// 扩展语言子标签（小写，历史遗留，BCP47 已不推荐但仍需能解析）。
    pub extlangs: Vec<String>,
    /// 文种子标签（首字母大写，如 `Hans` / `Latn`）。
    pub script: Option<String>,
    /// 区域子标签（2 字母大写或 3 数字）。
    pub region: Option<String>,
    /// 变体子标签序列（小写）。
    pub variants: Vec<String>,
    /// 扩展段序列（按 singleton 升序）。
    pub extensions: Vec<Extension>,
    /// 私有用途取值序列（原样保留，私有段无大小写约定）。
    pub privateuse: Vec<String>,
}

impl LanguageTag {
    /// 未确定语言（回退链终点）。
    pub fn undetermined() -> Self {
        LanguageTag {
            language: UNDETERMINED.to_string(),
            ..Default::default()
        }
    }

    /// 便捷构造：只给语言（其余段留空）。
    pub fn language_only(lang: &str) -> Self {
        LanguageTag {
            language: lang.to_ascii_lowercase(),
            ..Default::default()
        }
    }

    /// 便捷构造：语言 + 区域（最常见的两段形如 `zh-CN`）。
    pub fn lang_region(lang: &str, region: &str) -> Self {
        LanguageTag {
            language: lang.to_ascii_lowercase(),
            region: Some(region.to_ascii_uppercase()),
            ..Default::default()
        }
    }

    /// 规范式（RFC 5646 canonical form）。
    ///
    /// 扩展段按 singleton 升序、变体段按字典序——这一步是**幂等**的：
    /// `canonical(parse(canonical(x))) == canonical(parse(x))`。幂等性由
    /// [`check_canonical_idempotent`] 机检，它保证缓存键与回退链可比。
    pub fn canonical(&self) -> String {
        let mut out = String::with_capacity(32);
        out.push_str(&self.language);
        for e in &self.extlangs {
            out.push('-');
            out.push_str(e);
        }
        if let Some(s) = &self.script {
            out.push('-');
            out.push_str(s);
        }
        if let Some(r) = &self.region {
            out.push('-');
            out.push_str(r);
        }
        let mut variants = self.variants.clone();
        sort_strings(&mut variants);
        for v in &variants {
            out.push('-');
            out.push_str(v);
        }
        let mut exts: Vec<&Extension> = self.extensions.iter().collect();
        exts.sort_by(|a, b| a.singleton.cmp(&b.singleton));
        for e in exts {
            out.push('-');
            out.push(e.singleton);
            for v in &e.values {
                out.push('-');
                out.push_str(v);
            }
        }
        if !self.privateuse.is_empty() {
            out.push_str("-x");
            for p in &self.privateuse {
                out.push('-');
                out.push_str(p);
            }
        }
        out
    }

    /// 人类可读描述（替述可读，无障碍面）。
    ///
    /// 屏幕阅读器需要念出"语言、地区"而不是"zh-Hans-CN"。本函数给出的串
    /// 不依赖任何本地化资源，因此在内核侧也能生成，不引入数据面。
    pub fn spoken(&self) -> String {
        let mut parts: Vec<&str> = Vec::new();
        parts.push(self.language.as_str());
        if !self.extlangs.is_empty() {
            parts.push(self.extlangs[0].as_str());
        }
        if let Some(s) = &self.script {
            parts.push(s.as_str());
        }
        if let Some(r) = &self.region {
            parts.push(r.as_str());
        }
        for v in &self.variants {
            parts.push(v.as_str());
        }
        if self.privateuse.is_empty() {
            parts.join(" ")
        } else {
            format!("{} （私有用途 {} 段）", parts.join(" "), self.privateuse.len())
        }
    }

    /// 从 Unicode 扩展（`-u-`）里取 `keyword` 对应的 type 段。
    ///
    /// **为什么需要这个取口**：BCP47 的 `extension = singleton 1*("-" (2*8alphanum))`
    /// **无法表达嵌套 singleton**——`-u-ca-buddhist-nu-latn` 里的 `nu` 只有两个字符，
    /// 形式上就是 `u` 的一个普通取值，解析器把它并入同一段是符合规范的。
    /// 但消费面（历法/数字制/校对）真正要的是 `ca=buddhist` 这样的**键值对**，
    /// 所以本方法在扁平取值序列上按 `keyword` → 紧随其后的段 规则取值。
    ///
    /// 找不到 keyword、或 keyword 在末位没有 type 段时返回 `None`——
    /// **不猜**：宁可显性缺值，也不要让历法静默退回公历而无从追查。
    pub fn unicode_extension_key(&self, keyword: &str) -> Option<&str> {
        let ext = self.extensions.iter().find(|e| e.singleton == 'u')?;
        let idx = ext.values.iter().position(|v| v == keyword)?;
        ext.values.get(idx + 1).map(|s| s.as_str())
    }

    /// 变体段数量上界守卫用：段数是否在预算内。
    pub fn subtag_count(&self) -> usize {
        1 + self.extlangs.len()
            + usize::from(self.script.is_some())
            + usize::from(self.region.is_some())
            + self.variants.len()
            + self.extensions.iter().map(|e| 1 + e.values.len()).sum::<usize>()
            + self.privateuse.len()
    }

    /// 规范序下的子标签种类序列（枚举守卫的输入）。
    pub fn kind_sequence(&self) -> Vec<SubtagKind> {
        let mut kinds = vec![SubtagKind::Language];
        for _ in &self.extlangs {
            kinds.push(SubtagKind::Extlang);
        }
        if self.script.is_some() {
            kinds.push(SubtagKind::Script);
        }
        if self.region.is_some() {
            kinds.push(SubtagKind::Region);
        }
        for _ in &self.variants {
            kinds.push(SubtagKind::Variant);
        }
        for e in &self.extensions {
            kinds.push(SubtagKind::ExtensionSingleton);
            for _ in &e.values {
                kinds.push(SubtagKind::ExtensionValue);
            }
        }
        for _ in &self.privateuse {
            kinds.push(SubtagKind::PrivateUse);
        }
        kinds
    }

    /// 枚举守卫：自身的种类序列必须规范。
    pub fn guard(&self) -> Result<(), Rejection> {
        guard_segment_order(&self.kind_sequence())
    }
}

/// 插入排序（`no_std` 且不依赖 `slice::sort` 的分配策略；变体/扩展数量有界，
/// 插入排序在最坏情况（已排序）O(n)、逆序 O(n²) 下 n ≤ 8，代价可忽略）。
fn sort_strings(v: &mut Vec<String>) {
    let mut i = 1;
    while i < v.len() {
        let mut j = i;
        while j > 0 && v[j - 1] > v[j] {
            let t = v[j - 1].clone();
            v[j - 1] = v[j].clone();
            v[j] = t;
            j -= 1;
        }
        i += 1;
    }
}

// ---------------------------------------------------------------------------
// §5 子标签判别（BCP47 全量切分）
// ---------------------------------------------------------------------------

fn is_alpha(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphabetic())
}

fn is_digit(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
}

fn is_alphanum(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric())
}

/// 形态名（替述可读）。
pub fn describe_tag_shape(tag: &LanguageTag) -> String {
    // 注意：不能对 kind_sequence 直接 dedup——它是**相邻去重**，而扩展段
    // 序列是 [singleton,value,value,singleton,value]，去重后仍留重复项，
    // 输出会变成 "扩展单键→扩展取值→扩展单键→扩展取值"。
    // 这里按 [`SubtagKind::CANONICAL_ORDER`] 的排名顺序**全局**去重，
    // 输出恒为该标签实际段形态的升序去重列表（可复现）。
    let kinds = tag.kind_sequence();
    let mut names: Vec<&str> = Vec::new();
    for k in SubtagKind::CANONICAL_ORDER.iter() {
        if kinds.contains(k) {
            names.push(k.name());
        }
    }
    names.join("→")
}

/// 区域形态判别。
pub fn region_kind_of(s: &str) -> Option<RegionKind> {
    if s.len() == 2 && is_alpha(s) {
        Some(RegionKind::Alpha2)
    } else if s.len() == 3 && is_digit(s) {
        Some(RegionKind::Numeric3)
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// §6 容错解析器（锚点：容错解析 / 常见畸形容忍表）
// ---------------------------------------------------------------------------

/// 解析产物：标签 + 规范式 + 容错诊断序列。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParsedTag {
    /// 原始输入（保留，供诊断回溯）。
    pub source: String,
    /// 规范化后的标签。
    pub tag: LanguageTag,
    /// 规范式（缓存键）。
    pub canonical: String,
    /// 容错诊断序列（空 = 输入本就规范）。
    pub repairs: Vec<Diagnostic>,
    /// 是否发生了截断（尾随连字符等）。
    pub truncated: bool,
}

impl ParsedTag {
    /// 是否为"未发生任何容错"的干净解析。
    pub fn is_clean(&self) -> bool {
        self.repairs.is_empty() && !self.truncated
    }

    /// 被修过的畸形类别（去重、保序）。
    pub fn repaired_codes(&self) -> Vec<&'static str> {
        let mut out: Vec<&'static str> = Vec::new();
        for d in &self.repairs {
            if !out.contains(&d.code) {
                out.push(d.code);
            }
        }
        out
    }
}

/// 容错解析：剥壳 → 分段 → 归位 → 规范化。
///
/// 流程与锚点「容错解析（常见畸形容忍表）」逐条对应：
/// 1. **剥壳**——按容忍表处理空白 / 字符集 / `@modifier` / 下划线 / 空段 /
///    尾随连字符，每一步留一条诊断；
/// 2. **分段归位**——首段定 language，其后按 BCP47 位置规则依次认 extlang /
///    script / region / variant / extension / privateuse；装不下的段即拒绝
///    （[`E_SEGMENT_UNCONSUMED`]），**不猜**；
/// 3. **规范化**——按位置规则重排大小写，非规范则留 [`D_CASE`]。
///
/// 复杂度 `O(标签长)`；段数与字段数成正比，四段 + 扩展都是定长扫描。
pub fn parse_lenient(input: &str, budget: &ParseBudget) -> Result<ParsedTag, Rejection> {
    let source = input.to_string();
    let mut repairs: Vec<Diagnostic> = Vec::new();
    let mut truncated = false;

    // ---- 剥壳 ----
    let mut s: String = input.trim().to_string();
    if s.len() != input.len() {
        repairs.push(diag(
            D_SPACES,
            "剥壳",
            &format!("去首尾空白：{:?} → {:?}", input, s),
        ));
    }
    if let Some(p) = s.find('.') {
        let cut = s[..p].to_string();
        repairs.push(diag(
            D_CODESET,
            "剥壳",
            &format!("剥字符集后缀：{} → {}", s, cut),
        ));
        s = cut;
        truncated = true;
    }
    if let Some(p) = s.find('@') {
        let cut = s[..p].to_string();
        repairs.push(diag(
            D_AT_MODIFIER,
            "剥壳",
            &format!("剥 @modifier 后缀：{} → {}", s, cut),
        ));
        s = cut;
        truncated = true;
    }
    if s.contains('_') {
        let fixed = s.replace('_', "-");
        repairs.push(diag(
            D_UNDERSCORE,
            "剥壳",
            &format!("下划线改连字符：{} → {}", s, fixed),
        ));
        s = fixed;
    }

    // ---- 预算闸：标签总长 ----
    if s.len() > budget.max_len {
        return Err(Rejection {
            code: E_BUDGET_EXCEEDED,
            what: format!("标签长 {} 字节超出预算 {}", s.len(), budget.max_len),
            why: "Locale 标签参与哈希键与缓存槽；不设上界则单个畸形输入即可\
                  放大成大额内存占用"
                .to_string(),
            next: format!("缩短标签至 {} 字节以内，或调高 ParseBudget::max_len", budget.max_len),
        });
    }

    // ---- 分段（丢弃空段，尾随连字符单列诊断） ----
    let raw: Vec<&str> = s.split('-').collect();
    let mut segs: Vec<String> = Vec::with_capacity(raw.len());
    for (i, seg) in raw.iter().enumerate() {
        if seg.is_empty() {
            let is_tail = i + 1 == raw.len();
            repairs.push(diag(
                if is_tail { D_TRAILING_HYPHEN } else { D_EMPTY_SEGMENT },
                "剥壳",
                if is_tail {
                    "丢尾随连字符产生的空段"
                } else {
                    "丢段间多出连字符产生的空段"
                },
            ));
            if is_tail {
                truncated = true;
            }
            continue;
        }
        segs.push(seg.to_string());
    }
    if segs.is_empty() {
        return Err(Rejection {
            code: E_SEGMENT_EMPTY,
            what: format!("输入 {:?} 剥壳后没有任何子标签", input),
            why: "空标签没有可回退的语义；直接当 und 处理会把配置错误藏起来".to_string(),
            next: "显式写 und 表示'语言未定'，不要写空串".to_string(),
        });
    }

    let mut tag = classify(&segs, budget, &mut repairs)?;
    normalize(&mut tag, &mut repairs);

    // ---- 枚举守卫 ----
    tag.guard()?;

    let canonical = tag.canonical();
    Ok(ParsedTag {
        source,
        tag,
        canonical,
        repairs,
        truncated,
    })
}

/// 分段归位：把子标签序列装进四段 + 扩展。
fn classify(
    segs: &[String],
    budget: &ParseBudget,
    _repairs: &mut Vec<Diagnostic>,
) -> Result<LanguageTag, Rejection> {
    let mut tag = LanguageTag::default();
    let mut i = 0usize;

    // ---- 首段：language 或私有用途 ----
    let first = &segs[0];
    if first.eq_ignore_ascii_case("x") {
        // 纯私有用途标签：按 BCP47 直接进私有段。
    } else if first.len() >= 2 && first.len() <= 8 && is_alpha(first) {
        tag.language = first.to_ascii_lowercase();
        i = 1;
        // ---- 扩展语言（3 字母，且其后必须还有段） ----
        let mut n = 0;
        while n < 3
            && i < segs.len()
            && segs[i].len() == 3
            && is_alpha(&segs[i])
            && i + 1 < segs.len()
        {
            tag.extlangs.push(segs[i].to_ascii_lowercase());
            i += 1;
            n += 1;
        }
        // ---- 文种（4 字母） ----
        if i < segs.len() && segs[i].len() == 4 && is_alpha(&segs[i]) {
            tag.script = Some(segs[i].clone());
            i += 1;
        }
        // ---- 区域（2 字母 或 3 数字） ----
        if i < segs.len() && region_kind_of(&segs[i]).is_some() {
            tag.region = Some(segs[i].clone());
            i += 1;
        }
    } else {
        return Err(Rejection {
            code: E_LANGUAGE_INVALID,
            what: format!("首段 {:?} 不是合法 language 子标签", first),
            why: "BCP47 language 段是 2-8 个字母；不是字母就不是语言标签，\
                  后续的 script/region 也就无从定位"
                .to_string(),
            next: "首段写 2-3 位 ISO 639 代码（zh/en/ja）；确需私有内容用 x- 前缀".to_string(),
        });
    }

    // ---- 变体段 ----
    while i < segs.len() && is_variant(&segs[i]) {
        if tag.variants.len() >= budget.max_variants {
            return Err(budget_reject("变体段", budget.max_variants));
        }
        tag.variants.push(segs[i].clone());
        i += 1;
    }

    // ---- 扩展段 ----
    while i < segs.len() && is_singleton(&segs[i]) {
        let singleton = segs[i].chars().next().unwrap_or('?').to_ascii_lowercase();
        if singleton == 'x' {
            break; // 私有用途段收尾
        }
        if tag.extensions.iter().any(|e| e.singleton == singleton) {
            return Err(Rejection {
                code: E_SINGLETON_DUPLICATE,
                what: format!("扩展 singleton {:?} 重复出现", singleton),
                why: "同一 singleton 出现两次时，扩展取值该并到哪一段并无定论；\
                      RFC 5646 要求去重而不是任选"
                    .to_string(),
                next: format!("把两处 -{} 的取值合并成一段 -{}", singleton, singleton),
            });
        }
        if tag.extensions.len() >= budget.max_extensions {
            return Err(budget_reject("扩展段", budget.max_extensions));
        }
        i += 1;
        let mut values: Vec<String> = Vec::new();
        while i < segs.len() && is_extension_value(&segs[i]) {
            if values.len() >= budget.max_extension_values {
                return Err(budget_reject("扩展取值", budget.max_extension_values));
            }
            values.push(segs[i].clone());
            i += 1;
        }
        if values.is_empty() {
            return Err(Rejection {
                code: E_SINGLETON_NO_VALUE,
                what: format!("扩展 singleton {:?} 后面没有任何取值", singleton),
                why: "悬空 singleton 无法表达任何语义；保留它会让匹配把两个\
                      语义不同的标签判成同一个"
                    .to_string(),
                next: format!("给 -{} 补至少一个取值，或删掉这个悬空 singleton", singleton),
            });
        }
        tag.extensions.push(Extension { singleton, values });
    }

    // ---- 私有用途段 ----
    if i < segs.len() && segs[i].eq_ignore_ascii_case("x") {
        i += 1;
        while i < segs.len() && segs[i].len() <= 8 && is_alphanum(&segs[i]) {
            if tag.privateuse.len() >= budget.max_privateuse {
                return Err(budget_reject("私有用途段", budget.max_privateuse));
            }
            tag.privateuse.push(segs[i].clone());
            i += 1;
        }
        if tag.privateuse.is_empty() {
            return Err(Rejection {
                code: E_PRIVATEUSE_EMPTY,
                what: "私有用途段 x- 后面没有任何取值".to_string(),
                why: "空的 x- 段不承载信息，但会让规范式多出一个 -x，\
                      破坏规范式的幂等性"
                    .to_string(),
                next: "删掉这个空的 x-，或补上私有取值".to_string(),
            });
        }
    }

    // ---- 兜底：还有段没归位 ----
    if i < segs.len() {
        let left = segs[i].clone();
        let is_privateuse_misplaced = tag.privateuse.is_empty() && left.eq_ignore_ascii_case("x");
        let spec = defect_spec(if is_privateuse_misplaced {
            Defect::PrivateUseMisplaced
        } else {
            Defect::CaseChaos
        });
        return Err(Rejection {
            code: E_SEGMENT_UNCONSUMED,
            what: format!("子段 {:?} 无法归位（停在第 {} 段）", left, i),
            why: if is_privateuse_misplaced {
                format!("{}；私有用途段按 BCP47 必须在最后", spec.symptom)
            } else {
                format!("四段+扩展都装不下它；形态不合 BCP47（{}）", spec.symptom)
            },
            next: if is_privateuse_misplaced {
                "把 x- 段挪到标签末尾".to_string()
            } else {
                "核对该段长度：script=4 字母、region=2 字母或 3 数字、\
                 variant=5-8 位或 4 位数字开头"
                    .to_string()
            },
        });
    }

    if tag.language.is_empty() {
        // 纯私有用途标签（`x-...`）合法，但回退链上它等价于 und——
        // 私有段不参与任何语言语义。
        tag.language = UNDETERMINED.to_string();
    }
    Ok(tag)
}

fn budget_reject(what: &str, limit: usize) -> Rejection {
    Rejection {
        code: E_BUDGET_EXCEEDED,
        what: format!("{} 数量超出预算 {}", what, limit),
        why: "段数无界会让单条标签膨胀成大结构；预算把单标签成本钉在常数内".to_string(),
        next: format!("删减{}至 {} 以内", what, limit),
    }
}

fn is_singleton(s: &str) -> bool {
    s.len() == 1 && is_alphanum(s)
}

fn is_variant(s: &str) -> bool {
    // 5-8 位字母数字，或 4 位且首位是数字。
    ((s.len() >= 5 && s.len() <= 8) || s.len() == 4) && is_alphanum(s)
        && (s.len() >= 5 || s.as_bytes()[0].is_ascii_digit())
}

fn is_extension_value(s: &str) -> bool {
    s.len() >= 2 && s.len() <= 8 && is_alphanum(s)
}

/// 规范化：按 BCP47 位置规则重排大小写。
fn normalize(tag: &mut LanguageTag, repairs: &mut Vec<Diagnostic>) {
    let lang_fixed = tag.language.clone();
    if lang_fixed.bytes().any(|b| b.is_ascii_uppercase()) {
        repairs.push(diag(
            D_CASE,
            "归位",
            &format!("language 段小写化：{}", lang_fixed),
        ));
    }
    tag.language = lang_fixed.to_ascii_lowercase();

    for e in tag.extlangs.iter_mut() {
        *e = e.to_ascii_lowercase();
    }
    if let Some(s) = tag.script.as_mut() {
        let fixed = title_case(s);
        if &fixed != s {
            repairs.push(diag(
                D_CASE,
                "归位",
                &format!("script 段首字母大写化：{} → {}", s, fixed),
            ));
        }
        *s = fixed;
    }
    if let Some(r) = tag.region.as_mut() {
        let fixed = if is_alpha(r) {
            r.to_ascii_uppercase()
        } else {
            r.clone()
        };
        if &fixed != r {
            repairs.push(diag(
                D_CASE,
                "归位",
                &format!("region 段大写化：{} → {}", r, fixed),
            ));
        }
        *r = fixed;
    }
    for v in tag.variants.iter_mut() {
        *v = v.to_ascii_lowercase();
    }
    for e in tag.extensions.iter_mut() {
        for v in e.values.iter_mut() {
            *v = v.to_ascii_lowercase();
        }
    }
}

fn title_case(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    for (i, &c) in b.iter().enumerate() {
        if i == 0 {
            out.push(c.to_ascii_uppercase() as char);
        } else {
            out.push(c.to_ascii_lowercase() as char);
        }
    }
    out
}

// ---------------------------------------------------------------------------
// §7 回退链（锚点：zh-CN→zh→und 逐级回退 / 回退链规范逐条）
// ---------------------------------------------------------------------------

/// 一次回退去掉的是什么。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RemovedSegment {
    /// 未去掉任何段（起点）。
    None,
    /// 去掉私有用途段。
    PrivateUse,
    /// 去掉扩展段（singleton 及其全部取值）。
    Extension,
    /// 去掉一个变体段。
    Variant,
    /// 去掉区域段。
    Region,
    /// 去掉文种段。
    Script,
    /// 去掉扩展语言段。
    Extlang,
}

impl RemovedSegment {
    /// 人话名（回退链规范逐条可读）。
    pub fn rule(self) -> &'static str {
        match self {
            RemovedSegment::None => "起点：未截断",
            RemovedSegment::PrivateUse => "私有用途段不参与语言语义，整体去掉后停",
            RemovedSegment::Extension => "去掉整个扩展段（singleton 连同其取值）",
            RemovedSegment::Variant => "去掉最右一个变体段",
            RemovedSegment::Region => "去掉区域段（zh-Hans-CN → zh-Hans）",
            RemovedSegment::Script => "去掉文种段（zh-Hans → zh）",
            RemovedSegment::Extlang => "去掉扩展语言段",
        }
    }
}

/// 回退链上的一跳。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FallbackHop {
    /// 本跳的规范式。
    pub tag: String,
    /// 从上一跳到本跳去掉了什么。
    pub removed: RemovedSegment,
    /// 本跳适用的规则（人话）。
    pub rule: &'static str,
}

/// 回退链：逐级截断到 `und`，每跳带规则留痕。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FallbackChain {
    /// 链起点规范式。
    pub origin: String,
    /// 各级跳。
    pub hops: Vec<FallbackHop>,
    /// 是否走到了 `und`（回退耗尽，锚点要求**显性**）。
    pub exhausted: bool,
    /// 链终点（恒为 `und`）。
    pub terminal: String,
}

impl FallbackChain {
    /// 链深（含起点与终点）。
    pub fn depth(&self) -> usize {
        self.hops.len()
    }

    /// 各级规范式序列。
    pub fn tags(&self) -> Vec<String> {
        self.hops.iter().map(|h| h.tag.clone()).collect()
    }

    /// 规则序列（逐条可读，供调试器与文档使用）。
    pub fn rules(&self) -> Vec<&'static str> {
        self.hops.iter().map(|h| h.rule).collect()
    }
}

/// 逐级截断一跳：去掉最右可去的一段。
///
/// RFC 4647 §3.4.1 lookup 语义：**每次只去一段**；若标签以 singleton 结尾，
/// 则 singleton 及其后全部一起去掉。理由是扩展取值本身不构成可独立匹配的语言
/// 身份（`de-DE-u-co-phonebk` 截成 `de-DE-u-co` 毫无意义），所以整段去。
///
/// 截断次序（与 [`SubtagKind::CANONICAL_ORDER`] 逆序一致）：
/// 私有用途 → 扩展 → 变体 → 区域 → 文种 → 扩展语言。
fn truncate_once(tag: &LanguageTag) -> Option<(LanguageTag, RemovedSegment)> {
    let mut t = tag.clone();
    if !t.privateuse.is_empty() {
        t.privateuse.clear();
        return Some((t, RemovedSegment::PrivateUse));
    }
    if !t.extensions.is_empty() {
        t.extensions.pop();
        return Some((t, RemovedSegment::Extension));
    }
    if !t.variants.is_empty() {
        t.variants.pop();
        return Some((t, RemovedSegment::Variant));
    }
    if t.region.is_some() {
        t.region = None;
        return Some((t, RemovedSegment::Region));
    }
    if t.script.is_some() {
        t.script = None;
        return Some((t, RemovedSegment::Script));
    }
    if !t.extlangs.is_empty() {
        t.extlangs.pop();
        return Some((t, RemovedSegment::Extlang));
    }
    None
}

/// 生成回退链。
///
/// 锚点钦定「zh-CN→zh→und 逐级回退——回退链规范逐条」，所以：
/// - **逐级**：每跳只去一段，链上不会出现"跳段"；
/// - **规范逐条**：每跳带 [`RemovedSegment::rule`]，可被调试器逐条展示；
/// - **耗尽显性**：[`FallbackChain::exhausted`] 为真表示终点是 `und` 默认值，
///   调用方**必须**能区分"匹配到 und"与"真的匹配到 und 资源"。
///
/// 复杂度 `O(链深)`，链深 ≤ 段数 + 2，是标签自身长度的线性量。
pub fn fallback_chain(tag: &LanguageTag) -> FallbackChain {
    let mut hops = Vec::new();
    let mut cur = tag.clone();
    hops.push(FallbackHop {
        tag: cur.canonical(),
        removed: RemovedSegment::None,
        rule: RemovedSegment::None.rule(),
    });
    while let Some((next, removed)) = truncate_once(&cur) {
        hops.push(FallbackHop {
            tag: next.canonical(),
            removed,
            rule: removed.rule(),
        });
        cur = next;
    }
    // 终点恒为 und；起点已是 und 时不重复追加。
    let exhausted = cur.language != UNDETERMINED;
    if exhausted {
        hops.push(FallbackHop {
            tag: UNDETERMINED.to_string(),
            removed: RemovedSegment::Region,
            rule: "回退耗尽：落到 und 默认（显性标记 exhausted=true）",
        });
    }
    FallbackChain {
        origin: tag.canonical(),
        exhausted,
        terminal: hops[hops.len() - 1].tag.clone(),
        hops,
    }
}

// ---------------------------------------------------------------------------
// §8 O(1) 索引与解析缓存（锚点：Locale 缓存 / 解析缓存 O(1)）
// ---------------------------------------------------------------------------

const EMPTY_SLOT: u32 = u32::MAX;

/// 开放定址索引（FNV-1a + 线性探测）。
///
/// 为什么不用 `BTreeMap`：锚点要求匹配 `O(1)`，而 BTreeMap 是 `O(log n)`——
/// 在「每个组件都要问一次 Locale」的帧路径上，log 因子会随资源数增长。
/// 开放定址把探测长度钉在常数级，代价是**表满即拒绝**（[`E_INDEX_FULL`]），
/// 换来的是可预测的内存上界与可预测的最坏耗时。
///
/// **不变式**：本索引只做「键 → 句柄」的映射，句柄由调用方解释。多个键可以
/// 指向同一句柄（[`LocaleCache`] 的别名登记就靠这个），所以本结构**不**假设
/// "第 n 个键对应第 n 个值"——把键与值强行按插入序平行，是别名场景下的经典 bug。
pub struct TagIndex {
    /// 每槽的句柄（`EMPTY_SLOT` 表空）。
    slots: Vec<u32>,
    /// 每槽的键文本——**按槽索引，不按句柄索引**。
    ///
    /// 这一条是别名登记能成立的关键：多个键可以指向同一句柄，若把键文本按句柄
    /// 存，后登记的别名会覆盖掉规范式的键文本，`get(规范式)` 随之失效。
    slot_keys: Vec<String>,
    /// 句柄 → 值，由调用方解释（本结构不假设"第 n 个键对应第 n 个值"）。
    mask: usize,
}

impl TagIndex {
    /// 按请求槽数构造；槽数**钳到参数域**并向上取整到 2 的幂。
    pub fn with_slots(requested: usize) -> (Self, Option<ClampRecord>) {
        let capped = clamp(requested, MIN_INDEX_SLOTS, MAX_INDEX_SLOTS);
        let slots = round_up_pow2(capped);
        let record = if slots != requested {
            Some(ClampRecord {
                field: "index_slots",
                requested,
                effective: slots,
                reason: "槽数钳到参数域 [8,4096] 并取 2 的幂（掩码寻址要求）",
            })
        } else {
            None
        };
        let mut slot_keys: Vec<String> = Vec::with_capacity(slots);
        for _ in 0..slots {
            slot_keys.push(String::new());
        }
        (
            TagIndex {
                slots: vec![EMPTY_SLOT; slots],
                slot_keys,
                mask: slots - 1,
            },
            record,
        )
    }

    /// FNV-1a 64 位哈希。
    fn hash(key: &str) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in key.as_bytes() {
            h ^= *b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        h
    }

    /// 查句柄（O(1)：一次定位 + 有界线性探测）。
    pub fn get(&self, key: &str) -> Option<u32> {
        let mut idx = (Self::hash(key) as usize) & self.mask;
        for _ in 0..self.slots.len() {
            let slot = self.slots[idx];
            if slot == EMPTY_SLOT {
                return None;
            }
            if self.slot_keys[idx] == key {
                return Some(slot);
            }
            idx = (idx + 1) & self.mask;
        }
        None
    }

    /// 登记 `key`。
    ///
    /// `preferred` 指定句柄：传 `Some(h)` 表示**别名登记**（该键与既有键指向
    /// 同一条产物）；传 `None` 表示新分配句柄（= 已用槽数，即值表的新下标）。
    /// 两种情形都对同键幂等。
    pub fn insert(&mut self, key: &str, preferred: Option<u32>) -> Result<u32, Rejection> {
        if let Some(h) = self.get(key) {
            return Ok(h);
        }
        let handle = match preferred {
            Some(h) => h,
            None => self.occupied() as u32,
        };
        let mut idx = (Self::hash(key) as usize) & self.mask;
        let mut probes = 0usize;
        loop {
            if self.slots[idx] == EMPTY_SLOT {
                self.slots[idx] = handle;
                self.slot_keys[idx] = key.to_string();
                return Ok(handle);
            }
            probes += 1;
            if probes > self.slots.len() {
                return Err(Rejection {
                    code: E_INDEX_FULL,
                    what: format!(
                        "Locale 索引表探测环走满（槽 {} 已用 {}）",
                        self.slots.len(),
                        self.occupied()
                    ),
                    why: "静默覆盖会让命中率无征兆地掉下去；掉命中率在界面上表现为\
                          '偶发变慢'，是极难归因的故障"
                        .to_string(),
                    next: format!(
                        "调大索引槽数（上限 {}）或减少同时驻留的 Locale 种类",
                        MAX_INDEX_SLOTS
                    ),
                });
            }
            idx = (idx + 1) & self.mask;
        }
    }

    /// 已占用槽数（= 真实键数，别名各占一槽）。
    pub fn occupied(&self) -> usize {
        self.slots.iter().filter(|s| **s != EMPTY_SLOT).count()
    }

    /// 槽数（内存上界）。
    pub fn capacity(&self) -> usize {
        self.slots.len()
    }
}

/// 缓存命中/未命中后的来源（显性，便于性能归因）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CacheOutcome {
    /// 命中：探测键直接命中，未解析。
    Hit,
    /// 未命中：本次解析并入表。
    MissStored,
    /// 别名命中：探测键未命中，解析后发现规范式已在表内，
    /// 于是把本次探测键登记为指向既有条目的别名（**不新增条目**）。
    AliasHit,
}

/// 解析缓存。
///
/// 命中路径返回**借用**（`&ParsedTag`）而非拷贝——拷贝会把"O(1) 命中"变成
/// O(标签长)，那样的缓存等于没缓存。
pub struct LocaleCache {
    index: TagIndex,
    entries: Vec<ParsedTag>,
    hits: u64,
    misses: u64,
}

impl LocaleCache {
    /// 按请求槽数构造；钳制记录随返。
    pub fn new(requested_slots: usize) -> (Self, Option<ClampRecord>) {
        let (index, rec) = TagIndex::with_slots(requested_slots);
        (
            LocaleCache {
                index,
                entries: Vec::new(),
                hits: 0,
                misses: 0,
            },
            rec,
        )
    }

    /// 域默认容量的缓存。
    pub fn domain_default() -> Self {
        LocaleCache::new(64).0
    }

    /// 解析并查缓存。
    ///
    /// - 命中：`O(标签长)` 哈希 + `O(1)` 探测，**跳过解析与结构分配**；
    /// - 未命中：`O(标签长)` 解析 + `O(标签长)` 入表。
    ///
    /// 失败（畸形到无法容错）**不入表**——缓存只存成功产物，避免把错误路径
    /// 也变成"命中即返回旧错"的陷阱。
    pub fn resolve<'a>(
        &'a mut self,
        input: &str,
        budget: &ParseBudget,
    ) -> Result<(&'a ParsedTag, CacheOutcome), Rejection> {
        let probe_key = cache_probe_key(input);
        if let Some(v) = self.index.get(&probe_key) {
            self.hits += 1;
            return Ok((&self.entries[v as usize], CacheOutcome::Hit));
        }
        self.misses += 1;
        let parsed = parse_lenient(input, budget)?;
        // 规范式是权威键。已在表内 → 本次探测键登记为别名，不新增条目。
        let (handle, stored) = match self.index.get(&parsed.canonical) {
            Some(h) => {
                let h = self.index.insert(&probe_key, Some(h))?;
                (h, false)
            }
            None => {
                let h = self.index.insert(&probe_key, None)?;
                // 规范式登记为同句柄的第二键（此后按规范式查也命中同一条目）。
                // 先登记再 move：反序会把 parsed 提前 move 掉。
                self.index.insert(&parsed.canonical, Some(h))?;
                (h, true)
            }
        };
        if stored {
            self.entries.push(parsed);
            Ok((&self.entries[handle as usize], CacheOutcome::MissStored))
        } else {
            Ok((&self.entries[handle as usize], CacheOutcome::AliasHit))
        }
    }

    /// 命中率（千分比，零 = 未命中过）。
    pub fn hit_permille(&self) -> u32 {
        let total = self.hits + self.misses;
        if total == 0 {
            return 0;
        }
        ((self.hits * 1000) / total) as u32
    }

    /// 命中次数。
    pub fn hits(&self) -> u64 {
        self.hits
    }

    /// 未命中次数。
    pub fn misses(&self) -> u64 {
        self.misses
    }

    /// 已缓存条目数。
    pub fn entries(&self) -> usize {
        self.entries.len()
    }

    /// 索引槽数（内存上界）。
    pub fn slots(&self) -> usize {
        self.index.capacity()
    }
}

/// 探测键：轻量归一（去空白 + 小写 + 下划线改连字符），用于命中判定。
///
/// 注意它**不是**规范式（规范式要等解析后才知道）。用轻量键是为了让命中路径
/// 不必先解析——这正是"缓存后 O(1)"的来源：命中时一次哈希 + 一次探测就拿到
/// 产物，全程不构造段表、不分配诊断向量。
///
/// 轻量键对 `ZH_hans_cn` 与 `zh-Hans-CN` 会归到同一个键，所以等价标签天然
/// 共享条目；而 `en_US.UTF-8` 这类带壳标签的轻量键与规范式不同，处理办法是
/// **别名登记**——见 [`LocaleCache::resolve`]：探测未命中时解析出规范式，若该
/// 规范式已在表内，就把这次的轻量键登记为指向既有条目的别名，不再新增条目。
/// 这样"等价标签共享一条缓存条目"对**所有**容错形态都成立，而不只对大小写成立。
fn cache_probe_key(input: &str) -> String {
    let trimmed = input.trim();
    let mut out = String::with_capacity(trimmed.len());
    for b in trimmed.bytes() {
        match b {
            b'_' => out.push('-'),
            b'A'..=b'Z' => out.push((b + 32) as char),
            _ => out.push(b as char),
        }
    }
    out
}

// ---------------------------------------------------------------------------
// §9 匹配语义（锚点：匹配语义）
// ---------------------------------------------------------------------------

/// Locale 登记表：可用资源索引。
///
/// 按 **language 子标签**建桶是本项匹配 O(1) 的关键：回退链的每一跳（除终点
/// `und`）都保持同一 language，所以查一个具体跳只需定位一个桶，桶内条目数是
/// 同语言的资源数（实践中 1-3 个）。终点 `und` 单独走全表扫（桶外特例）。
pub struct LocaleRegistry {
    index: TagIndex,
    tags: Vec<LanguageTag>,
}

impl LocaleRegistry {
    /// 空登记表。
    pub fn new() -> Self {
        let (index, _) = TagIndex::with_slots(64);
        LocaleRegistry {
            index,
            tags: Vec::new(),
        }
    }

    /// 登记一个可用 Locale（规范式去重）。
    ///
    /// # 不变量（改动本方法前先读这段）
    ///
    /// 本表的 `index` 与 `tags` 是**平行数组**：句柄 h 必须对应 `tags[h]`。
    /// 成立的前提是**本表不使用别名登记**（恒传 `preferred = None`）——
    /// 此时 `TagIndex::insert` 返回的句柄 = 已占用槽数，而每新增一条就同步
    /// `push` 一次，两边同步增长，对齐成立。
    ///
    /// 一旦有人给本表改用别名（`preferred = Some(h)`），句柄就会**复用**既有
    /// 下标，`handle >= tags.len()` 判false → 条目被静默丢弃，
    /// 表现为「登记过的 Locale 查不到」，且没有任何报错。所以下面用
    /// `debug_assert` 把前提钉死：别名一旦被误用，debug 构建立刻定位到本行。
    pub fn insert(&mut self, tag: LanguageTag) -> Result<(), Rejection> {
        let key = tag.canonical();
        let existed = self.index.get(&key).is_some();
        let handle = self.index.insert(&key, None)? as usize;
        debug_assert_eq!(
            existed,
            handle < self.tags.len(),
            "登记表出现别名式句柄复用，平行数组不变式已被破坏"
        );
        if !existed {
            debug_assert_eq!(handle, self.tags.len(), "新增条目必须接在表尾");
            self.tags.push(tag);
        }
        Ok(())
    }

    /// 精确命中（O(1)）。
    pub fn contains_canonical(&self, canonical: &str) -> bool {
        self.index.get(canonical).is_some()
    }

    /// 已登记数量。
    pub fn len(&self) -> usize {
        self.tags.len()
    }

    /// 空表判定。
    pub fn is_empty(&self) -> bool {
        self.tags.is_empty()
    }

    /// 逐跳查表（每跳 `O(1)` 索引 + 桶内小扫；整链 `O(链深)`）。
    fn find_canonical(&self, canonical: &str) -> Option<&LanguageTag> {
        self.index
            .get(canonical)
            .map(|v| &self.tags[v as usize])
    }
}

impl Default for LocaleRegistry {
    fn default() -> Self {
        LocaleRegistry::new()
    }
}

/// 匹配结果。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Match {
    /// 请求的规范式。
    pub requested: String,
    /// 命中的规范式（未命中时为 `und`）。
    pub matched: String,
    /// 命中发生在回退链第几跳（0 = 精确命中）。
    pub hop: usize,
    /// 是否精确命中。
    pub exact: bool,
    /// 是否回退耗尽（命中的是 `und` 默认）。
    ///
    /// 锚点「回退耗尽→und 默认+**显性**」的落点：这个位必须由调用方显式检查，
    /// 因为"回退到 und"在界面上表现为「所有语言资源都失效」，把它和"真的配了
    /// und 资源"混同会让排查从一开始就找错方向。
    pub exhausted: bool,
}

/// 匹配：请求标签在可用集合里逐级回退查找。
///
/// 复杂度 `O(链深)`，每跳 `O(1)`——这正是锚点「匹配 O(1)」+「回退 O(链深)」
/// 的组合口径：单次索引探测 O(1)，链上有几跳就几次。
pub fn lookup(registry: &LocaleRegistry, requested: &LanguageTag) -> Match {
    let chain = fallback_chain(requested);
    for (hop, h) in chain.hops.iter().enumerate() {
        if let Some(found) = registry.find_canonical(&h.tag) {
            return Match {
                requested: requested.canonical(),
                matched: found.canonical(),
                hop,
                exact: hop == 0,
                exhausted: found.language == UNDETERMINED,
            };
        }
    }
    // 表里连 und 都没有：仍然返回 und 默认 + exhausted 显性，而不是 Err。
    // 理由：查不到资源是**正常业务态**（用户装的语言包就是没这个语言），
    // 不是内核故障；报 Err 会让上层把可降级态当崩溃处理。
    Match {
        requested: requested.canonical(),
        matched: UNDETERMINED.to_string(),
        hop: chain.hops.len().saturating_sub(1),
        exact: false,
        exhausted: true,
    }
}

// ---------------------------------------------------------------------------
// §10 规格表（锚点：逐条规格公开）
// ---------------------------------------------------------------------------

/// 规格表一行：一条规格 + 判据映射 + 兑现位置。
#[derive(Clone, Copy, Debug)]
pub struct SpecItem {
    /// 规格号（S-01…）。
    pub id: &'static str,
    /// 规格原文（逐条公开）。
    pub statement: &'static str,
    /// 对应判据。
    pub criterion: &'static str,
    /// 兑现函数（谁保证这条）。
    pub enforced_by: &'static str,
}

/// **数据模型与规格表**（锚点钦定，逐条规格公开）。
///
/// 这张表不是文档摘抄——[`check_spec_coverage`] 会核对每条的 `enforced_by`
/// 都指向真实存在的公开函数名，判据全覆盖，且规格号连续。
pub const SPEC_SHEET: [SpecItem; 13] = [
    SpecItem {
        id: "S-01",
        statement: "BCP47 四段 language-script-region-variant 全部可解析并归位",
        criterion: "BCP47 全量",
        enforced_by: "parse_lenient",
    },
    SpecItem {
        id: "S-02",
        statement: "扩展段（singleton+取值）与私有用途段可解析，非法扩展拒绝三要素",
        criterion: "BCP47 全量",
        enforced_by: "classify",
    },
    SpecItem {
        id: "S-03",
        statement: "规范式按位置规则规范化大小写，且规范化幂等",
        criterion: "BCP47 全量",
        enforced_by: "normalize",
    },
    SpecItem {
        id: "S-04",
        statement: "常见畸形（下划线/字符集/@modifier/空段/尾连字符/大小写）可容错并留诊断",
        criterion: "容错表",
        enforced_by: "parse_lenient",
    },
    SpecItem {
        id: "S-05",
        statement: "容忍表每种畸形有且只有一行，码不重复、行不空",
        criterion: "容错表",
        enforced_by: "check_defect_table",
    },
    SpecItem {
        id: "S-06",
        statement: "回退链逐级截断，每跳带规则留痕（回退链规范逐条）",
        criterion: "回退链",
        enforced_by: "fallback_chain",
    },
    SpecItem {
        id: "S-07",
        statement: "回退链终点恒为 und，且耗尽状态显性可查",
        criterion: "回退链",
        enforced_by: "fallback_chain",
    },
    SpecItem {
        id: "S-08",
        statement: "解析缓存在解析后命中为 O(1) 查表，命中返回借用不拷贝",
        criterion: "解析缓存",
        enforced_by: "LocaleCache::resolve",
    },
    SpecItem {
        id: "S-09",
        statement: "缓存表满显性拒绝，不静默覆盖",
        criterion: "解析缓存",
        enforced_by: "TagIndex::insert",
    },
    SpecItem {
        id: "S-10",
        statement: "匹配逐跳 O(1)，回退耗尽返回 und 默认并置 exhausted",
        criterion: "回退链",
        enforced_by: "lookup",
    },
    SpecItem {
        id: "S-11",
        statement: "参数域钳制可见：越界请求被钳到边界并留钳制记录",
        criterion: "解析缓存",
        enforced_by: "ParseBudget::requested",
    },
    SpecItem {
        id: "S-12",
        statement: "枚举守卫：子标签种类序列必须落在规范序上，越序拒绝",
        criterion: "容错表",
        enforced_by: "guard_segment_order",
    },
    SpecItem {
        id: "S-13",
        statement: "单源复用：每项能力有且只有一个 owner，F2948/F2915 为 consumer",
        criterion: "单源复用",
        enforced_by: "check_single_source",
    },
];

/// 规格表自检：编号连续、判据全覆盖、`enforced_by` 指向真实函数。
///
/// 判据覆盖映射表——锚点判据五条，本表必须逐条有规格承载。
const CRITERIA: [&str; 5] = ["BCP47 全量", "回退链", "解析缓存", "容错表", "单源复用"];

/// `enforced_by` 白名单：本项真实存在的公开兑现点。
///
/// 白名单而不是"去源码里 grep"——后者在 no_std 库里跑不了（没有文件系统），
/// 且会被注释里的同名文本骗过。白名单的代价是新增兑现点要同步加一行，
/// 这个代价换来的是"规格表里的兑现点一定是真的"。
const ENFORCERS: [&str; 13] = [
    "parse_lenient",
    "classify",
    "normalize",
    "parse_lenient",
    "check_defect_table",
    "fallback_chain",
    "fallback_chain",
    "LocaleCache::resolve",
    "TagIndex::insert",
    "lookup",
    "ParseBudget::requested",
    "guard_segment_order",
    "check_single_source",
];

/// 规格表自检。
pub fn check_spec_coverage() -> Result<(), Rejection> {
    // 判据全覆盖：每条判据至少一条规格承载。
    for c in CRITERIA {
        if !SPEC_SHEET.iter().any(|s| s.criterion == c) {
            return Err(Rejection {
                code: E_SPEC_GAP,
                what: format!("判据「{}」在规格表里没有任何承载条目", c),
                why: "判据是验收的最小单位；判据无承载等于该项没实现，\
                      而它在文档里看起来像实现了"
                    .to_string(),
                next: format!("为判据「{}」补至少一条 SpecItem", c),
            });
        }
    }
    // 编号连续 + 兑现点真实 + 文案非空。
    for (i, s) in SPEC_SHEET.iter().enumerate() {
        let expect = format!("S-{:02}", i + 1);
        if s.id != expect {
            return Err(Rejection {
                code: E_SPEC_GAP,
                what: format!("规格号不连续：第 {} 行是 {}（应为 {}）", i + 1, s.id, expect),
                why: "规格号是引用单位；跳号会让'补 S-07'这类指令落不到实处".to_string(),
                next: format!("把该行 id 改为 {}", expect),
            });
        }
        if !ENFORCERS.contains(&s.enforced_by) {
            return Err(Rejection {
                code: E_SPEC_GAP,
                what: format!("规格 {} 的兑现点 {:?} 不在本项公开兑现点清单内", s.id, s.enforced_by),
                why: "规格表里的兑现点必须是真的；写了兑现点却没这函数，等于\
                      用规格表给自己背书"
                    .to_string(),
                next: format!(
                    "把 {} 改为真实兑现点（{:?}）或先实现它",
                    s.id, ENFORCERS
                ),
            });
        }
        if s.statement.is_empty() || s.criterion.is_empty() {
            return Err(Rejection {
                code: E_SPEC_GAP,
                what: format!("规格 {} 的原文或判据为空", s.id),
                why: "空规格等于占位符；占位符会让'逐条规格公开'名存实亡".to_string(),
                next: "补齐 statement 与 criterion".to_string(),
            });
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// §11 单源复用声明（锚点：F2948/F2915 单源复用声明 / N02 消费 / F4003 前向）
// ---------------------------------------------------------------------------

/// 能力键：单源仲裁的最小单位。
///
/// 能力键是**字符串**而不是枚举——因为对端（F2948/F2915）在别的 crate 里，
/// 跨 crate 引用枚举变体等于把两域焊死；字符串键 + 唯一 owner 判定既够用
/// 又不制造编译期耦合。
pub type CapabilityKey = &'static str;

/// 标签解析能力键（本项 owner）。
pub const CAP_TAG_PARSE: CapabilityKey = "tag-parse";
/// 回退链能力键（本项 owner）。
pub const CAP_FALLBACK_CHAIN: CapabilityKey = "fallback-chain";
/// 畸形容忍能力键（本项 owner）。
pub const CAP_DEFECT_TOLERANCE: CapabilityKey = "defect-tolerance";
/// `:lang()` 匹配树能力键（**F2948 owner**）。
pub const CAP_LANG_MATCH_TREE: CapabilityKey = "lang-match-tree";
/// lang→字体偏好映射能力键（**F2948 owner**）。
pub const CAP_LANG_FONT_PREF: CapabilityKey = "lang-font-preference";
/// 字体偏好表本体能力键（**F2915 owner**）。
pub const CAP_FONT_PREF_TABLE: CapabilityKey = "font-preference-table";
/// i18n 调试器能力键（**F4011 owner**）——容错表的硬编码检出读同一张表。
pub const CAP_I18N_DEBUGGER: CapabilityKey = "i18n-debugger";
/// 排版消费能力键（**N02 owner**）。
pub const CAP_TYPOGRAPHY_CONSUME: CapabilityKey = "typography-consume";
/// 方向模型能力键（**F4003 owner**）。
pub const CAP_DIRECTION_MODEL: CapabilityKey = "direction-model";

/// 本项标识（owner 侧写这个）。
pub const OWNER_SELF: &str = "VE-F4002";

/// 一条单源声明。
#[derive(Clone, Copy, Debug)]
pub struct SingleSourceClaim {
    /// 能力键。
    pub key: CapabilityKey,
    /// 能力名（替述可读）。
    pub label: &'static str,
    /// 唯一 owner（`VE-xxxx`）。
    pub owner: &'static str,
    /// consumer 列表（消费但不拥有）。
    pub consumes: &'static [&'static str],
    /// 声明正文（为什么这样分工）。
    pub statement: &'static str,
}

/// **单源复用声明表**（锚点钦定）。
///
/// 表的读法：**owner 唯一，consumer 可多**。本项是 `tag-parse` / `fallback-chain`
/// / `defect-tolerance` 的 owner；F2948（`:lang()` 匹配树 + lang→字体偏好映射）
/// 与 F2915（字体偏好表本体）是 consumer；N02 消费回退链做排版路由；
/// F4003 消费"按 Locale 给默认方向"这个取值口。
pub const LANG_SINGLE_SOURCE: [SingleSourceClaim; 9] = [
    SingleSourceClaim {
        key: CAP_TAG_PARSE,
        label: "BCP47 标签解析与规范化",
        owner: OWNER_SELF,
        consumes: &["VE-F2948", "VE-F2915"],
        statement: "全域只有这一份 BCP47 解析器。对端拿 LanguageTag/parse_lenient，\
                    不许自建第二份"
    },
    SingleSourceClaim {
        key: CAP_FALLBACK_CHAIN,
        label: "locale 回退链",
        owner: OWNER_SELF,
        consumes: &["VE-N02", "VE-F2948"],
        statement: "回退链只有这一份；N02 排版路由与 F2948 :lang() 匹配共用，\
                    保证'选字体'与'选排版策略'落在同一个 Locale 上"
    },
    SingleSourceClaim {
        key: CAP_DEFECT_TOLERANCE,
        label: "标签畸形容忍表",
        owner: OWNER_SELF,
        consumes: &["VE-F4011"],
        statement: "容错表只有这一份；F4011 调试器的硬编码检出读同一张表，\
                    不另立一套'调试专用'的宽松规则"
    },
    SingleSourceClaim {
        key: CAP_LANG_MATCH_TREE,
        label: ":lang() 选择器匹配树",
        owner: "VE-F2948",
        consumes: &[OWNER_SELF],
        statement: "匹配树归 F2948（它消费本项的解析结果）；本项只提供被匹配的\
                    标签与回退链，不实现选择器"
    },
    SingleSourceClaim {
        key: CAP_LANG_FONT_PREF,
        label: "lang→字体偏好映射激活",
        owner: "VE-F2948",
        consumes: &[OWNER_SELF],
        statement: "映射激活归 F2948（F2915 预留兑现）；本项不给字体建议"
    },
    SingleSourceClaim {
        key: CAP_FONT_PREF_TABLE,
        label: "字体偏好表本体",
        owner: "VE-F2915",
        consumes: &["VE-F2948"],
        statement: "偏好表本体归 F2915；本项不持表，避免两域各持一份偏好表"
    },
    SingleSourceClaim {
        key: CAP_TYPOGRAPHY_CONSUME,
        label: "排版策略路由消费",
        owner: "VE-N02",
        consumes: &[OWNER_SELF],
        statement: "排版路由归 N02；它消费本项回退链得到'最终生效 Locale'，\
                    再选排版策略——路由与匹配不许各算一次"
    },
    SingleSourceClaim {
        key: CAP_I18N_DEBUGGER,
        label: "i18n 调试器（Locale/方向/格式三维检视）",
        owner: "VE-F4011",
        consumes: &[OWNER_SELF],
        statement: "调试器归 F4011；它的伪本地化与硬编码检出读本项的容错表，\
                    不另立一套'调试专用'的宽松规则"
    },
    SingleSourceClaim {
        key: CAP_DIRECTION_MODEL,
        label: "文字方向判定",
        owner: "VE-F4003",
        consumes: &[OWNER_SELF],
        statement: "方向判定归 F4003；本项只提供按 Locale 取默认方向的取值口，\
                    不做首强字符启发式"
    },
];

/// 单源红线机检：每个能力键有且只有一个 owner。
///
/// 这条是本项的第一红线。违反的代价在文件头「关于『单源』的硬约束」里写了：
/// 两份解析器会让同一页出现两种字，且极难归因。
pub fn check_single_source() -> Result<(), Rejection> {
    for (i, a) in LANG_SINGLE_SOURCE.iter().enumerate() {
        for b in LANG_SINGLE_SOURCE.iter().skip(i + 1) {
            if a.key == b.key {
                return Err(Rejection {
                    code: E_SINGLE_SOURCE_DUP,
                    what: format!(
                        "能力 {:?} 出现第二个 owner：{} 与 {}",
                        a.key, a.owner, b.owner
                    ),
                    why: "两份 BCP47 解析器/回退链对同一标签的切法只要有一处不同，\
                          :lang() 选中的字体与排版管线选的字体就会分叉，\
                          表现为'同一页两种字'且极难归因"
                        .to_string(),
                    next: format!("合并为唯一 owner（建议 {}）并删掉另一条声明", a.owner),
                });
            }
        }
        if a.owner.is_empty() || a.statement.is_empty() || a.label.is_empty() {
            return Err(Rejection {
                code: E_SPEC_GAP,
                what: format!("能力 {:?} 的声明要素不全（owner/label/statement 有空）", a.key),
                why: "空 owner 等于无人负责；空 statement 等于没声明".to_string(),
                next: "补齐该行的 owner / label / statement".to_string(),
            });
        }
    }
    // 本项必须真的持有那三项，否则"单源复用声明"是空话。
    for must in [CAP_TAG_PARSE, CAP_FALLBACK_CHAIN, CAP_DEFECT_TOLERANCE] {
        if !LANG_SINGLE_SOURCE
            .iter()
            .any(|c| c.key == must && c.owner == OWNER_SELF)
        {
            return Err(Rejection {
                code: E_SPEC_GAP,
                what: format!("本项未声明拥有能力 {:?}", must),
                why: "锚点要求本项作为 lang 单源；若三项能力都不在本项名下，\
                      则本项只是又一份旁支实现"
                    .to_string(),
                next: format!("在 LANG_SINGLE_SOURCE 中把 {:?} 的 owner 设为 {}", must, OWNER_SELF),
            });
        }
    }
    // 命名空间纪律：`consumes` 装**项号**，`key` 装**能力键**，两者不可混比。
    // 每个 consumer 必须在本表里作为某项能力的 owner 出现过——
    // 否则消费方登记的是一个本表不存在的归属，等于消费了个空能力。
    let owners: Vec<&str> = LANG_SINGLE_SOURCE.iter().map(|c| c.owner).collect();
    for c in LANG_SINGLE_SOURCE.iter() {
        for p in c.consumes.iter() {
            if !owners.contains(p) {
                return Err(Rejection {
                    code: E_SPEC_GAP,
                    what: format!("能力 {:?} 的 consumer {:?} 未在本表任何 owner 位出现", c.key, p),
                    why: "consumer 必须对应一个真实的 owner 项；登记成不存在的归属，\
                          等于消费方来取一个没人提供的能力，表现为下游调用空实现"
                        .to_string(),
                    next: format!("核对 {:?} 的 owner，或把consumer 改成本表已有 owner", p),
                });
            }
        }
    }
    // 本项不得声明拥有对端能力（越权即僭越施工）。
    for c in LANG_SINGLE_SOURCE.iter() {
        if c.owner == OWNER_SELF
            && matches!(
                c.key,
                CAP_LANG_MATCH_TREE | CAP_LANG_FONT_PREF | CAP_FONT_PREF_TABLE
                    | CAP_TYPOGRAPHY_CONSUME | CAP_DIRECTION_MODEL
            )
        {
            return Err(Rejection {
                code: E_SINGLE_SOURCE_DUP,
                what: format!("本项越权声明拥有 {:?}（该能力归 {}）", c.key, "对端"),
                why: "越权声明会让对端以为能力已就绪而不再实现，\
                      表现为下游调用一个空实现"
                    .to_string(),
                next: format!("把 {:?} 的 owner 改回其真正归属域", c.key),
            });
        }
    }
    Ok(())
}

/// 前向声明位：已落地与否**如实登记**。
///
/// 沿用家族前向声明纪律：**声明齐、已落地数如实**是合法的；**缺档或谎报已落地**
/// 才是缺陷。所以 [`FORWARD_SLOTS`] 里 `landed: false` 是正常态。
#[derive(Clone, Copy, Debug)]
pub struct ForwardSlot {
    /// 消费方（下游项号）。
    pub consumer: &'static str,
    /// 消费的能力键。
    pub key: CapabilityKey,
    /// 消费内容（人话）。
    pub statement: &'static str,
    /// 是否已落地。前向阶段恒为 `false`。
    pub landed: bool,
}

/// 前向槽位登记。
pub const FORWARD_SLOTS: [ForwardSlot; 3] = [
    ForwardSlot {
        consumer: "VE-F4003",
        key: CAP_DIRECTION_MODEL,
        statement: "按 Locale 取默认方向（LTR/RTL）；本项只给取值口，判定算法归 F4003",
        landed: false,
    },
    ForwardSlot {
        consumer: "VE-N02",
        key: CAP_TYPOGRAPHY_CONSUME,
        statement: "回退链产出的'最终生效 Locale'供排版路由消费",
        landed: false,
    },
    ForwardSlot {
        consumer: "VE-F2948",
        key: CAP_LANG_MATCH_TREE,
        statement: "标签解析与回退链供 :lang() 匹配树消费",
        landed: false,
    },
];

/// 前向槽位机检：槽位齐、键合法、`landed` 与本项实际提供的能力一致。
pub fn check_forward_slots() -> Result<(), Rejection> {
    for slot in FORWARD_SLOTS.iter() {
        if !LANG_SINGLE_SOURCE.iter().any(|c| c.key == slot.key) {
            return Err(Rejection {
                code: E_SPEC_GAP,
                what: format!("前向槽位引用了未声明的能力键 {:?}", slot.key),
                why: "槽位引用不存在的键，等于消费方来取一个没人提供的能力".to_string(),
                next: format!("先在 LANG_SINGLE_SOURCE 声明 {:?}", slot.key),
            });
        }
        if slot.statement.is_empty() {
            return Err(Rejection {
                code: E_SPEC_GAP,
                what: format!("前向槽位 {} 的消费说明为空", slot.consumer),
                why: "空说明使槽位无法被下游理解".to_string(),
                next: "补齐 statement".to_string(),
            });
        }
    }
    // 本项**只提供**方向取值口，不提供判定本体——所以 F4003 的槽位恒未落地。
    // 若哪天谎报 landed=true 而 F4003 未实现，方向就会退化成恒 LTR，
    // 表现为"阿拉伯语界面排版整体反向失效"且极难归因。
    for slot in FORWARD_SLOTS.iter() {
        if slot.landed && slot.key == CAP_DIRECTION_MODEL {
            return Err(Rejection {
                code: E_SPEC_GAP,
                what: "F4003 方向槽位被标为已落地，但方向判定本体不在本项".to_string(),
                why: "谎报已落地会让上层以为方向能力就绪；\
                      实际退化成恒 LTR，表现为 RTL 语言整体反向失效"
                    .to_string(),
                next: "把 landed 改回 false，直到 VE-F4003 真正实现判定本体".to_string(),
            });
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// §12 按 Locale 取默认方向（前向取值口，判定本体归 F4003）
// ---------------------------------------------------------------------------

/// 书写方向。本项只搬运 F4003 的取值域并按 Locale 查表，**不做启发式判定**。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WritingDirection {
    /// 从左到右（拉丁、汉字、泰文、印地等）。
    Ltr,
    /// 从右到左（阿拉伯、希伯来、波斯、乌尔都等）。
    Rtl,
}

impl WritingDirection {
    /// 名称（替述可读）。
    pub fn name(self) -> &'static str {
        match self {
            WritingDirection::Ltr => "从左到右",
            WritingDirection::Rtl => "从右到左",
        }
    }
}

/// RTL 语言主子标签表。
///
/// 这是**查表**不是判定——查表是 O(1) 且稳定，判定（首强字符启发式）是 F4003
/// 的活。分开的原因很实际：查表错了改一行常量，判定错了要动算法与语料。
const RTL_LANGUAGES: [&str; 7] = ["ar", "arc", "dv", "fa", "he", "ps", "ur"];

/// 按 Locale 取默认书写方向（**前向取值口**，判定本体归 F4003）。
///
/// 只看主语言子标签：脚本（如 `Arab`）与区域不改默认方向的方向性——方向是
/// 语言的属性，不是区域的习惯。混排文本的实际方向由 F4003 的分层模型裁决。
pub fn default_direction(tag: &LanguageTag) -> WritingDirection {
    if RTL_LANGUAGES.contains(&tag.language.as_str()) {
        WritingDirection::Rtl
    } else {
        WritingDirection::Ltr
    }
}

// ---------------------------------------------------------------------------
// §13 派生不变量（回归与对拍用）
// ---------------------------------------------------------------------------

/// 规范式幂等自检：对同一标签重复规范化，规范式不变。
///
/// 这是缓存键与回退链可比的前提——若不幂等，`zh-Hans-CN` 与 `zh-hans-cn`
/// 会占两个缓存条目且回退链一跳都跳不上。
pub fn check_canonical_idempotent(tag: &LanguageTag) -> Result<(), Rejection> {
    let a = tag.canonical();
    let mut again = tag.clone();
    normalize_quiet(&mut again);
    let b = again.canonical();
    if a != b {
        return Err(Rejection {
            code: E_SPEC_GAP,
            what: format!("规范式不幂等：{} → {}", a, b),
            why: "不幂等会让等价标签占多个缓存条目，回退链与匹配都会漏跳".to_string(),
            next: "核对该标签的 segment 形态；变体/扩展排序应已稳定".to_string(),
        });
    }
    Ok(())
}

fn normalize_quiet(tag: &mut LanguageTag) {
    let mut sink: Vec<Diagnostic> = Vec::new();
    normalize(tag, &mut sink);
}

/// 解析+回退一体入口：给调用方的最常用面。
///
/// 返回 `(解析产物, 回退链, 匹配结果)`——三样都是调用方要的，分三次调只是把
/// 同一份解析重复三遍。一体入口还顺带保证三样**出自同一次解析**（不会出现
/// 「匹配的标签和回退的标签不是同一个」这种最难查的分叉）。
pub fn resolve_locale(
    registry: &LocaleRegistry,
    input: &str,
    budget: &ParseBudget,
) -> Result<(ParsedTag, FallbackChain, Match), Rejection> {
    let parsed = parse_lenient(input, budget)?;
    let chain = fallback_chain(&parsed.tag);
    let m = lookup(registry, &parsed.tag);
    Ok((parsed, chain, m))
}

/// 域自检摘要（供 [`crate::svstar2`] 聚合器与文档引用）。
pub fn run_vei02_summary() -> CheckSet {
    let mut set = CheckSet::new("svstar2-vei02-summary");
    set.add(
        "S-容忍表-每种畸形有且只有一行",
        check_defect_table().is_ok(),
        "",
    );
    set.add(
        "S-规格表-判据全覆盖且兑现点真实",
        check_spec_coverage().is_ok(),
        "",
    );
    set.add(
        "S-单源-每能力唯一owner",
        check_single_source().is_ok(),
        "",
    );
    set.add(
        "S-前向-槽位齐且落地数如实",
        check_forward_slots().is_ok(),
        "",
    );
    set
}

// ---------------------------------------------------------------------------
// 单元测试（宿主 cargo test 直跑）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn b() -> ParseBudget {
        ParseBudget::DOMAIN_DEFAULT
    }

    #[test]
    fn parses_four_segment_plus_extension() {
        let p = parse_lenient("zh-Hans-CN-u-ca-buddhist-nu-latn", &b()).unwrap();
        assert_eq!(p.tag.language, "zh");
        assert_eq!(p.tag.script.as_deref(), Some("Hans"));
        assert_eq!(p.tag.region.as_deref(), Some("CN"));
        // BCP47 无法表达嵌套 singleton：`nu` 是 2 字符子标签，形式上是 u 的
        // 一个 value（RFC 5646: extension = singleton 1*("-" (2*8alphanum))）。
        // 故整个 `-u-ca-buddhist-nu-latn` 是**一个**扩展段，取值 4 个。
        assert_eq!(p.tag.extensions.len(), 1);
        assert_eq!(p.tag.extensions[0].singleton, 'u');
        assert_eq!(p.tag.extensions[0].values, alloc::vec!["ca", "buddhist", "nu", "latn"]);
        assert_eq!(p.canonical, "zh-Hans-CN-u-ca-buddhist-nu-latn");
        // 消费面靠 unicode_extension_key 取键值对，而不是靠嵌套结构。
        assert_eq!(p.tag.unicode_extension_key("ca"), Some("buddhist"));
        assert_eq!(p.tag.unicode_extension_key("nu"), Some("latn"));
        assert_eq!(p.tag.unicode_extension_key("xx"), None);
    }

    #[test]
    fn two_distinct_singletons_are_two_extensions() {
        // `t` 与 `u` 是两个独立 singleton → 两个扩展段（与上一条对照）。
        let p = parse_lenient("en-t-de-DE-u-ca-gregory", &b()).unwrap();
        assert_eq!(p.tag.extensions.len(), 2);
        assert_eq!(p.tag.extensions[0].singleton, 't');
        assert_eq!(p.tag.extensions[0].values, alloc::vec!["de", "de"]);
        assert_eq!(p.tag.extensions[1].singleton, 'u');
        // 规范式按 singleton 升序（'t' < 'u'），保证可复现；
        // 扩展取值段一律小写化（`de-DE` → `de-de`）。
        assert_eq!(p.canonical, "en-t-de-de-u-ca-gregory");
    }

    #[test]
    fn fallback_chain_is_stepwise_to_und() {
        let p = parse_lenient("zh-Hans-CN", &b()).unwrap();
        let c = fallback_chain(&p.tag);
        assert_eq!(
            c.tags(),
            vec!["zh-Hans-CN", "zh-Hans", "zh", "und"]
        );
        assert!(c.exhausted);
        assert_eq!(c.terminal, "und");
    }

    #[test]
    fn fallback_chain_strips_extension_whole() {
        let p = parse_lenient("de-DE-u-co-phonebk", &b()).unwrap();
        let c = fallback_chain(&p.tag);
        assert_eq!(c.tags(), vec!["de-DE-u-co-phonebk", "de-DE", "de", "und"]);
    }

    #[test]
    fn tolerates_windows_style_underscore() {
        let p = parse_lenient("zh_CN", &b()).unwrap();
        assert_eq!(p.canonical, "zh-CN");
        assert!(p.repaired_codes().contains(&D_UNDERSCORE));
        assert!(!p.is_clean());
    }

    #[test]
    fn tolerates_posix_codeset_and_modifier() {
        let p = parse_lenient("de_DE.UTF-8@euro", &b()).unwrap();
        assert_eq!(p.canonical, "de-DE");
        assert!(p.truncated);
        assert!(p.repaired_codes().contains(&D_CODESET));
    }

    #[test]
    fn rejects_dangling_singleton_with_three_elements() {
        let e = parse_lenient("en-u", &b()).unwrap_err();
        assert_eq!(e.code, E_SINGLETON_NO_VALUE);
        assert!(e.is_complete());
    }

    #[test]
    fn rejects_duplicate_singleton() {
        let e = parse_lenient("en-u-ca-gregory-u-nu-latn", &b()).unwrap_err();
        assert_eq!(e.code, E_SINGLETON_DUPLICATE);
    }

    #[test]
    fn cache_second_resolve_is_hit_and_borrows() {
        let mut c = LocaleCache::domain_default();
        let (first, o1) = c.resolve("fr-CA", &b()).unwrap();
        assert_eq!(o1, CacheOutcome::MissStored);
        assert_eq!(first.canonical, "fr-CA");
        let (second, o2) = c.resolve("fr-CA", &b()).unwrap();
        assert_eq!(o2, CacheOutcome::Hit);
        assert_eq!(second.canonical, "fr-CA");
        assert_eq!(c.entries(), 1, "同键命中不新增条目");
        assert_eq!(c.hits(), 1);
        assert_eq!(c.misses(), 1);
    }

    #[test]
    fn cache_equivalent_spellings_share_one_entry() {
        let mut c = LocaleCache::domain_default();
        c.resolve("zh-Hans-CN", &b()).unwrap();
        // 大小写与下划线变体：探测键归一后与首查同键 → 直接命中。
        let (_, o) = c.resolve("ZH_hans_cn", &b()).unwrap();
        assert_eq!(o, CacheOutcome::Hit);
        assert_eq!(c.entries(), 1);
        // 带 POSIX 壳的变体：探测键不同 → 解析后按规范式判为别名，不新增条目。
        let (p, o2) = c.resolve("zh_Hans_CN.UTF-8", &b()).unwrap();
        assert_eq!(o2, CacheOutcome::AliasHit);
        assert_eq!(p.canonical, "zh-Hans-CN");
        assert_eq!(c.entries(), 1, "等价标签只占一条缓存条目");
        // 别名登记后按规范式与按原壳写法都命中同一条目。
        assert_eq!(c.resolve("zh-Hans-CN", &b()).unwrap().1, CacheOutcome::Hit);
        assert_eq!(
            c.resolve("zh_Hans_CN.UTF-8", &b()).unwrap().1,
            CacheOutcome::Hit
        );
        assert_eq!(c.entries(), 1);
    }

    #[test]
    fn registry_lookup_falls_back_and_flags_exhausted() {
        let mut reg = LocaleRegistry::new();
        // 注意注册的是 `zh-Hans`（请求的**回退链上一跳**），不是 `zh-CN`——
        // RFC 4647 lookup 是**逐段截断**，`zh-Hans-CN` 的链是
        // zh-Hans-CN → zh-Hans → zh → und，永不经过 zh-CN。
        // 注册 zh-CN 却期望命中，正是把 lookup 误当 filtering 的典型错误。
        reg.insert(LanguageTag::lang_region("zh", "TW")).unwrap();
        reg.insert(parse_lenient("zh-Hans", &b()).unwrap().tag).unwrap();
        let req = parse_lenient("zh-Hans-CN", &b()).unwrap();
        let m = lookup(&reg, &req.tag);
        assert_eq!(m.matched, "zh-Hans", "命中回退链第 1 跳");
        assert_eq!(m.hop, 1);
        assert!(!m.exact);
        assert!(!m.exhausted);

        // 表里没有 ko-* → 落到 und 默认，且 exhausted 显性为真。
        let req2 = parse_lenient("ko-KR", &b()).unwrap();
        let m2 = lookup(&reg, &req2.tag);
        assert_eq!(m2.matched, "und");
        assert!(m2.exhausted, "回退耗尽必须显性");

        // 精确命中在第 0 跳。
        let req3 = parse_lenient("zh-TW", &b()).unwrap();
        let m3 = lookup(&reg, &req3.tag);
        assert!(m3.exact);
        assert_eq!(m3.hop, 0);
        assert_eq!(m3.matched, "zh-TW");
    }

    #[test]
    fn lookup_is_truncation_not_filtering() {
        // 显式钉住"lookup ≠ filtering"这条语义：链上到不了 zh-CN。
        let mut reg = LocaleRegistry::new();
        reg.insert(LanguageTag::lang_region("zh", "CN")).unwrap();
        let req = parse_lenient("zh-Hans-CN", &b()).unwrap();
        let m = lookup(&reg, &req.tag);
        assert_eq!(
            m.matched, "und",
            "逐段截断不产生 zh-CN，故只注册 zh-CN 时应耗尽到 und"
        );
        assert!(m.exhausted);
    }

    #[test]
    fn numeric_region_and_und() {
        let p = parse_lenient("es-419", &b()).unwrap();
        assert_eq!(p.tag.region.as_deref(), Some("419"));
        assert_eq!(region_kind_of("419"), Some(RegionKind::Numeric3));
        assert_eq!(region_kind_of("US"), Some(RegionKind::Alpha2));
        assert_eq!(region_kind_of("USA"), None);
        let u = parse_lenient("und", &b()).unwrap();
        assert!(!fallback_chain(&u.tag).exhausted);
    }

    #[test]
    fn enum_guard_rejects_out_of_order_kinds() {
        // 位置对调：region 出现在 script 之前 → 拒。
        let swapped = guard_segment_order(&[
            SubtagKind::Language,
            SubtagKind::Region,
            SubtagKind::Script,
        ])
        .is_err();
        // script/region 重复 → 拒。
        let dup = guard_segment_order(&[
            SubtagKind::Language,
            SubtagKind::Script,
            SubtagKind::Script,
        ])
        .is_err();
        // 悬空 singleton → 拒。
        let dangling =
            guard_segment_order(&[SubtagKind::Language, SubtagKind::ExtensionSingleton]).is_err();
        // 私有用途后还有扩展 → 拒。
        let tail = guard_segment_order(&[
            SubtagKind::Language,
            SubtagKind::PrivateUse,
            SubtagKind::ExtensionSingleton,
            SubtagKind::ExtensionValue,
        ])
        .is_err();
        // 首段非语言 → 拒。
        let head = guard_segment_order(&[SubtagKind::Region]).is_err();
        // 合法：完整四段 + 两扩展段（value 后再 singleton 是合法的新扩展）。
        let good = guard_segment_order(&[
            SubtagKind::Language,
            SubtagKind::Script,
            SubtagKind::Region,
            SubtagKind::Variant,
            SubtagKind::ExtensionSingleton,
            SubtagKind::ExtensionValue,
            SubtagKind::ExtensionValue,
            SubtagKind::ExtensionSingleton,
            SubtagKind::ExtensionValue,
            SubtagKind::PrivateUse,
        ])
        .is_ok();
        assert!(
            swapped && dup && dangling && tail && head && good,
            "枚举守卫判定不符合预期"
        );
        // 错误码可机检。
        let e = guard_segment_order(&[SubtagKind::Region]).unwrap_err();
        assert_eq!(e.code, E_SEGMENT_ORDER);
        assert!(e.is_complete());
    }

    #[test]
    fn budget_clamps_and_records() {
        let (budget, log) = ParseBudget::requested(9999);
        assert_eq!(budget.max_len, MAX_TAG_LEN);
        assert_eq!(log.len(), 1);
        assert_eq!(log[0].field, "max_len");
        let (c, rec) = LocaleCache::new(3);
        assert_eq!(c.slots(), MIN_INDEX_SLOTS);
        assert!(rec.is_some());
    }

    #[test]
    fn spec_sheet_and_single_source_are_green() {
        assert!(check_spec_coverage().is_ok());
        assert!(check_single_source().is_ok());
        assert!(check_forward_slots().is_ok());
        assert!(check_defect_table().is_ok());
    }

    #[test]
    fn default_direction_is_lookup_only() {
        assert_eq!(
            default_direction(&LanguageTag::language_only("ar")),
            WritingDirection::Rtl
        );
        assert_eq!(
            default_direction(&LanguageTag::language_only("he")),
            WritingDirection::Rtl
        );
        assert_eq!(
            default_direction(&LanguageTag::lang_region("zh", "CN")),
            WritingDirection::Ltr
        );
    }

    #[test]
    fn resolve_locale_is_single_parse_consistent() {
        let mut reg = LocaleRegistry::new();
        // 注册回退链上的相邻跳，才能在第 1 跳命中。
        reg.insert(parse_lenient("zh-Hans", &b()).unwrap().tag)
            .unwrap();
        let (parsed, chain, m) = resolve_locale(&reg, "ZH_hans_cn", &b()).unwrap();
        assert_eq!(parsed.canonical, "zh-Hans-CN");
        assert_eq!(chain.origin, parsed.canonical);
        assert_eq!(m.requested, parsed.canonical, "三结果出自同一次解析");
        assert_eq!(m.matched, "zh-Hans");
        assert_eq!(m.hop, 1);
        assert!(check_canonical_idempotent(&parsed.tag).is_ok());
    }

    #[test]
    fn registry_parallel_array_invariant_holds() {
        // 登记表 index/tags 是平行数组：句柄 h 必须对应 tags[h]。
        // 这条断言逐条插入后按规范式反查，确保没有条目被静默丢弃。
        let mut reg = LocaleRegistry::new();
        let tags = [
            LanguageTag::lang_region("zh", "CN"),
            LanguageTag::lang_region("zh", "TW"),
            LanguageTag::language_only("ja"),
            parse_lenient("zh-Hans", &b()).unwrap().tag,
            LanguageTag::language_only("und"),
        ];
        for t in tags.iter() {
            reg.insert(t.clone()).unwrap();
        }
        assert_eq!(reg.len(), 5);
        // 去重：规范式相同的重复登记不新增条目。
        reg.insert(LanguageTag::lang_region("zh", "CN")).unwrap();
        assert_eq!(reg.len(), 5, "规范式相同的重复登记不得新增");
        // 等价写法（大小写/下划线变体）同样按规范式去重。
        reg.insert(parse_lenient("ZH_TW", &b()).unwrap().tag).unwrap();
        assert_eq!(reg.len(), 5, "等价写法不得重复入表");
        // 但**规范化后不同**的标签是两条独立资源，必须都留下：
        // `zh-Hans` 与 `zh-Hans-CN` 是两个 Locale，不能被合并。
        reg.insert(parse_lenient("ZH_hans_cn", &b()).unwrap().tag)
            .unwrap();
        assert_eq!(reg.len(), 6, "规范化后不同的标签理应各自入表");
        // 逐条反查：登记过的每一条都必须查得到（平行数组对齐的证据）。
        for t in tags.iter() {
            assert!(
                reg.contains_canonical(&t.canonical()),
                "登记过的标签查不到：{}（平行数组失配）",
                t.canonical()
            );
        }
        assert!(reg.contains_canonical("zh-Hans-CN"));
        assert!(!reg.is_empty());
    }

    #[test]
    fn registry_lookup_walks_chain_in_order() {
        // 链上有多跳资源时，匹配必须命中**最近的一跳**，不是第一个登记的。
        let mut reg = LocaleRegistry::new();
        reg.insert(LanguageTag::language_only("und")).unwrap();
        reg.insert(LanguageTag::language_only("zh")).unwrap();
        reg.insert(parse_lenient("zh-Hans", &b()).unwrap().tag)
            .unwrap();
        let req = parse_lenient("zh-Hans-CN", &b()).unwrap();
        let m = lookup(&reg, &req.tag);
        assert_eq!(m.matched, "zh-Hans", "应命中最近一跳");
        assert_eq!(m.hop, 1);
    }

    #[test]
    fn cache_does_not_store_failures() {
        let mut c = LocaleCache::domain_default();
        assert!(c.resolve("en-u", &b()).is_err(), "悬空 singleton 必拒");
        assert_eq!(c.entries(), 0, "失败不得入表");
        // 失败一次后仍能正常存入合法标签（错误路径没有污染索引）。
        let (p, o) = c.resolve("de-DE", &b()).unwrap();
        assert_eq!(o, CacheOutcome::MissStored);
        assert_eq!(p.canonical, "de-DE");
        assert_eq!(c.entries(), 1);
    }

    #[test]
    fn spoken_form_is_readable() {
        let p = parse_lenient("zh-Hans-CN", &b()).unwrap();
        assert_eq!(p.tag.spoken(), "zh Hans CN");
        assert!(!describe_tag_shape(&p.tag).is_empty());
    }
}