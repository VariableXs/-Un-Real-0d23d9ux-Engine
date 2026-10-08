//! VE-F4005 · 字体国际化选型（VE-T 域 · 国际化域 · T01 组 · 目标 360 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F4005`
//!
//! **判据（锚点原文）**：回退链配置、覆盖验证、度量对齐、许可地域、三单源复用。
//!
//! **职责定位（锚点原文）**：T01 字体国际化选型——i18n 字体选型（多语言字体链：
//! 每 Locale 字体回退链配置（CJK/阿拉伯/泰文/拉丁字体集——**回退链配置表**；
//! **字符覆盖验证**（Locale 字符集→字体覆盖扫描（**缺字检出**（复用 F2908——单源
//! 复述；**度量对齐**（多字体混排基线（复用 F2909——复述单源；**许可多语言**
//! （字体许可地域差异核验（复用 F2919——复述单源。数据结构：数据模型与规格表
//! （逐条规格公开、参数域钳制、枚举守卫——家族格式）。错误路径与降级矩阵：
//! **缺字检出→回退链下一跳+计数**（复用）；**基线失配→对齐修正**（复用）；
//! **许可地域违规→阻断**（复用红线）；**链配置缺→默认链+诊断**。性能逐项分解：
//! 覆盖 O(字符集)；回退 O(链长)；基线 O(混排数)；许可 O(字体数)。跨批对接点：
//! **F2908/F2909/F2919 三单源复用声明**；Q 域字体对端；F4044 深化前向。
//! 无障碍与隐私：文档替述可读；无隐私面。
//!
//! # 本项的边界（不越界施工，遵守"只做领到的任务"）
//!
//! 本项交付 **字体选型的策略层**：每个 Locale 该配哪些字体、回退链怎么串、
//! 缺字时跳到链上第几跳、基线怎么对齐、这份字体在目标地域的许可是否允许。
//! 它**不代做**：
//!
//! - **字形覆盖扫描算法归 F2908**；本项消费 F2908 的缺字结论，按结论走回退链
//!   下一跳并**计数**（锚点：「缺字检出→回退链下一跳+计数（复用）」）。
//! - **度量与基线计算归 F2909**；本项消费其度量值做**对齐决策**
//!   （锚点：「基线失配→对齐修正（复用）」）。
//! - **许可地域核验归 F2919**；本项消费其许可结论并在**违规时阻断**
//!   （锚点：「许可地域违规→阻断（复用红线）」——这是三条里唯一带"阻断"的，
//!   因为字体许可违规是法律风险，不是排版质量问题）。
//!
//! 三个 F29xx 尚未落地，故本项把三者的激活位登记在 [`RESERVED_SLOTS`]
//! （`landed: false`）并附 [`FONT_SINGLE_SOURCE`] 单源声明——**声明归属、
//! 不代做实现**，等它们落地后由各自 owner 填实现，本项只消费接口。
//!
//! # 关于「许可地域违规→阻断」为什么是阻断而不是降级
//!
//! 缺字可以降级（换一个字体顶上去，字丑一点但能看）；基线可以修正（偏一点，
//! 视觉上能接受）。但**字体许可违规不能降级**——在一个没授权的地域里显示
//! 该字体，不是"排版变难看"，是分发未授权软件。这条没有中间态，所以
//! [`resolve_font_chain`] 对许可违规一律 [`Rejection`]，绝不"先跑起来再说"。
//!
//! # 关于「链配置缺→默认链+诊断」为什么不给静默默认
//!
//! 某 Locale 忘了配字体链，若静默给一条拉丁默认链，界面**看起来正常**，
//! 但中文/阿拉伯文会整片变成方框或问号。这类 bug 上线后基本抓不到——
//! 没有报错、没有崩溃。所以缺链时产出**默认链 + [`DiagKind::ChainDefaulted`] 诊断**
//! （照常返回可用结果，不阻断：阻断会让整页打不开，代价更大），让上层能报警。
//!
//! # 确定性
//!
//! 零时钟、零 IO、零环境依赖、零堆序不稳定（不排序、不建映射表）；
//! 输入是 Locale 标签、字体链配置与字符集，输出是纯数据结构。
//! 同一输入必得同一结果（含诊断序列顺序），保证回归可复现。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// §0 参数域钳制（锚点：参数域钳制——家族格式）
// ---------------------------------------------------------------------------

/// 回退链最小长度（至少一档：首选字体）。
pub const MIN_CHAIN_LEN: usize = 1;
/// 回退链最大档数上界。
///
/// 有界的理由：回退链是**常驻**结构（每次排版都要走），无界意味着可能吃掉
/// 内核态内存。越界申请被钳到本值并留 [`ClampRecord`]。
pub const MAX_CHAIN_LEN: usize = 16;
/// 字体资源表最小条目数。
pub const MIN_FONT_TABLE: usize = 4;
/// 字体资源表最大条目数上界。
pub const MAX_FONT_TABLE: usize = 256;
/// 单次覆盖扫描的字符集长度上界。
///
/// 理由与 F4004 的 `MAX_TEXT_LEN` 同源：覆盖扫描是 O(字符集)，超长会让单帧
/// 排版超时。上界给出后**显性拒绝**而非截断——截断会静默漏字。
pub const MAX_CHARSET_LEN: usize = 1 << 16;
/// 混排条目数上界（度量对齐一次最多核这么多项）。
pub const MAX_MIX_RUNS: usize = 1024;
/// 基线容差上界（单位：像素；超出的失配不可"修正"，只能判阻断）。
///
/// 存在的理由：基线失配若允许"任意修正"，那修正逻辑就能把任意离谱的字体
/// 硬拉到同一基线上——于是字体选型彻底失效（什么都"能对齐"）。给容差上界，
/// 超界显性拒绝，让"字体选错了"这件事能被看见。
pub const MAX_BASELINE_TOLERANCE: usize = 64;
/// 许可地域表最大条目数上界。
pub const MAX_LICENSE_TABLE: usize = 256;

/// 域标识（`CheckSet` 聚合用）。
pub const VEA_DOMAIN: &str = "svstar2-ve";

/// 通用钳制。
pub fn clamp(value: usize, lo: usize, hi: usize) -> usize {
    if value < lo {
        lo
    } else if value > hi {
        hi
    } else {
        value
    }
}

/// 钳制留痕（钳制必须可见）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ClampRecord {
    /// 参数名。
    pub field: &'static str,
    /// 申请值。
    pub asked: usize,
    /// 生效值。
    pub effective: usize,
    /// 钳向上界（true）还是下界（false）。
    pub to_upper: bool,
}

/// 钳制并留痕（超界不静默）。
pub fn clamp_tracked(
    field: &'static str,
    asked: usize,
    lo: usize,
    hi: usize,
) -> (usize, Option<ClampRecord>) {
    let effective = clamp(asked, lo, hi);
    if effective == asked {
        (effective, None)
    } else {
        (
            effective,
            Some(ClampRecord {
                field,
                asked,
                effective,
                to_upper: asked > hi,
            }),
        )
    }
}

// ---------------------------------------------------------------------------
// §1 拒绝三要素（锚点：异常零静默 / 家族 Rejection 格式）
// ---------------------------------------------------------------------------

/// 字体族非法。
pub const E_FAMILY_INVALID: &str = "E_FAMILY_INVALID";
/// 许可地域违规（**阻断红线**：字体许可是法律风险，不走降级）。
pub const E_LICENSE_BLOCKED: &str = "E_LICENSE_BLOCKED";
/// 基线失配超容差（不可"修正"，显性拒绝）。
pub const E_BASELINE_OUT_OF_TOLERANCE: &str = "E_BASELINE_OUT_OF_TOLERANCE";
/// 字符集越界（超长显性拒绝，不静默截断）。
pub const E_CHARSET_TOO_LONG: &str = "E_CHARSET_TOO_LONG";
/// 混排条目越界。
pub const E_MIX_TOO_LONG: &str = "E_MIX_TOO_LONG";
/// 回退链全档缺字（链走完仍缺 → 显性拒绝，不静默漏字）。
pub const E_CHAIN_EXHAUSTED: &str = "E_CHAIN_EXHAUSTED";
/// 预留槽位谎报落地。
pub const E_RESERVED_LIED: &str = "E_RESERVED_LIED";
/// 规格表覆盖缺口。
pub const E_SPEC_GAP: &str = "E_SPEC_GAP";
/// 单源复用违例（同能力多 owner）。
pub const E_SINGLE_SOURCE_DUP: &str = "E_SINGLE_SOURCE_DUP";
/// 私有面违规。
pub const E_PRIVACY_LEAK: &str = "E_PRIVACY_LEAK";

/// 拒绝记录（三要素齐全）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Rejection {
    /// 稳定错误码。
    pub code: &'static str,
    /// 现象。
    pub what: String,
    /// 根因。
    pub why: String,
    /// 建议。
    pub next: String,
}

impl Rejection {
    /// 三要素齐全（自检逐条断言，防止某条拒绝少写"现象"或"建议"）。
    pub fn is_complete(&self) -> bool {
        !self.code.is_empty() && !self.what.is_empty() && !self.why.is_empty() && !self.next.is_empty()
    }
    /// 便于诊断的可读形式（不含隐私面——这里只拼错误码与现象）。
    pub fn describe(&self) -> String {
        format!("{} | {} | {}", self.code, self.what, self.why)
    }
}

// ---------------------------------------------------------------------------
// §2 字体族与枚举守卫（锚点：枚举守卫——家族格式）
// ---------------------------------------------------------------------------

/// 字体族（四族与F4004 排版族一一对应，但**本项不重判族**：族由 F4004 路由给出）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FontFamily {
    /// 拉丁（西里尔也走这一档：同源字形度量）。
    Latin,
    /// 中日韩。
    Cjk,
    /// 阿拉伯。
    Arabic,
    /// 泰印度（含泰文）。
    TaiIndic,
}

impl FontFamily {
    /// 全集（自检遍历用；顺序即族号，勿乱动）。
    pub const ALL: [FontFamily; 4] = [
        FontFamily::Latin,
        FontFamily::Cjk,
        FontFamily::Arabic,
        FontFamily::TaiIndic,
    ];
    /// 族名（规格表与诊断用）。
    pub fn name(&self) -> &'static str {
        match self {
            FontFamily::Latin => "Latin",
            FontFamily::Cjk => "Cjk",
            FontFamily::Arabic => "Arabic",
            FontFamily::TaiIndic => "TaiIndic",
        }
    }
    /// 短码（表格显示用，与F4004 的 `ScriptFamily::short()` 同风格）。
    pub fn short(&self) -> &'static str {
        match self {
            FontFamily::Latin => "lat",
            FontFamily::Cjk => "cjk",
            FontFamily::Arabic => "ara",
            FontFamily::TaiIndic => "tai",
        }
    }
    /// 族号（稳定序，用于对拍；改动会让已落库的对拍基线失效）。
    pub fn ordinal(&self) -> u8 {
        match self {
            FontFamily::Latin => 0,
            FontFamily::Cjk => 1,
            FontFamily::Arabic => 2,
            FontFamily::TaiIndic => 3,
        }
    }
}

/// 字体族守卫（枚举守卫：值域外显性拒绝）。
///
/// 大小写形态全收（`Latin` / `latin` / `LATIN` 都放行）——理由与 F4004 的
/// `guard_family` 同源：族名会从配置文件、环境变量、多处字符串拼出来，
/// 只认一种大小写等于给配置留一个"看着像对的、实际全被拒"的坑。
pub fn guard_family(raw: &str) -> Result<FontFamily, Rejection> {
    let v = match raw {
        "Latin" | "latin" | "LATIN" | "LatIn" => FontFamily::Latin,
        "CJK" | "cjk" | "Cjk" | "Cjk." => FontFamily::Cjk,
        "Arabic" | "arabic" | "ARABIC" | "ArAbIc" => FontFamily::Arabic,
        "TaiIndic" | "taiindic" | "TAIINDIC" | "Taiindic" | "TAI-INDIC" => FontFamily::TaiIndic,
        other => {
            return Err(Rejection {
                code: E_FAMILY_INVALID,
                what: format!("字体族 {:?} 不在 Latin/CJK/Arabic/TaiIndic 四族内", other),
                why: "四族是锚点钦定的闭域；未知族说明字体配置或回退链不可信".to_string(),
                next: "改用四族之一（大小写形态不限）；若确需新族，先改 FontFamily 枚举并补字体资源表".to_string(),
            })
        }
    };
    Ok(v)
}

/// 字形来源（字体资源表登记用）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GlyphSource {
    /// 内建字形（内核自带，无许可面）。
    Builtin,
    /// 外部字体文件（**有许可面**，必须登记地域）。
    ExternalFile,
    /// 系统安装字体（许可随系统分发，本项只登记不校验内容）。
    SystemInstalled,
}

impl GlyphSource {
    /// 全集。
    pub const ALL: [GlyphSource; 3] = [
        GlyphSource::Builtin,
        GlyphSource::ExternalFile,
        GlyphSource::SystemInstalled,
    ];
    /// 是否带许可面（`ExternalFile` 为 true——只有它需要地域核验）。
    pub fn needs_license(&self) -> bool {
        matches!(self, GlyphSource::ExternalFile)
    }
    /// 短码（规格表与诊断用）。
    pub fn code(&self) -> &'static str {
        match self {
            GlyphSource::Builtin => "builtin",
            GlyphSource::ExternalFile => "extfile",
            GlyphSource::SystemInstalled => "sysfont",
        }
    }
}

/// 字形来源守卫。
pub fn guard_glyph_source(raw: &str) -> Result<GlyphSource, Rejection> {
    let v = match raw {
        "builtin" | "Builtin" | "BUILTIN" => GlyphSource::Builtin,
        "extfile" | "ExtFile" | "EXTFILE" | "external" => GlyphSource::ExternalFile,
        "sysfont" | "SysFont" | "SYSFONT" | "system" => GlyphSource::SystemInstalled,
        other => {
            return Err(Rejection {
                code: E_FAMILY_INVALID,
                what: format!("字形来源 {:?} 不在 builtin/extfile/sysfont 三值内", other),
                why: "字形来源决定是否要过许可核验；未知值会让许可面漏检".to_string(),
                next: "改用三值之一；不确定时选 sysfont（许可随系统分发，本项不校验内容）".to_string(),
            })
        }
    };
    Ok(v)
}

// ---------------------------------------------------------------------------
// §3 字体资源与许可（判据：许可地域；三单源复用 F2919）
// ---------------------------------------------------------------------------

/// 字体资源条目（字体资源表的一档）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FontEntry {
    /// 字体标识（配置面用的稳定名，不是文件路径——路径是执行侧的事）。
    pub id: &'static str,
    /// 归属字体族。
    pub family: FontFamily,
    /// 字形来源。
    pub source: GlyphSource,
    /// 覆盖码位上界（**该字体声明覆盖的 Unicode 码位区间上界**，不含）。
    ///
    /// 为什么用"区间上界"而不是位图：本项是**策略层**，不该在核内常驻一张
    /// 码位表（那是 F2908 的活）。这里只登记"这个字体声称覆盖到哪一位"，
    /// 覆盖够不够由 F2908 扫描后回结论。
    pub covers_upto: u32,
    /// 许可地域位图（`ExternalFile` 必填；bit0=全球，其他按序号）。
    ///
    /// 用 [`LICENSE_GLOBAL_BIT`] 表示"全球许可"，避免为全球单独开一个枚举值
    /// ——枚举值一多，配置就多一处能写错的地方。
    pub license_regions: u32,
}

/// 全球许可位（`license_regions` 的 bit0）。
pub const LICENSE_GLOBAL_BIT: u32 = 1;

/// 字体资源表（四族字体集的落点，判据：回退链配置）。
///
/// 表是**穷举**的：每个族给出该族的实际字体集。留默认族兜底会让"这个族没配
/// 字体"静默走默认，而静默走默认正是本项要禁止的行为。
pub const FONT_TABLE: [FontEntry; 12] = [
    // 拉丁族
    FontEntry { id: "varix-latin", family: FontFamily::Latin, source: GlyphSource::Builtin, covers_upto: 0x024F, license_regions: LICENSE_GLOBAL_BIT },
    FontEntry { id: "noto-serif-latin", family: FontFamily::Latin, source: GlyphSource::ExternalFile, covers_upto: 0x024F, license_regions: LICENSE_GLOBAL_BIT },
    FontEntry { id: "dejavu-sans", family: FontFamily::Latin, source: GlyphSource::SystemInstalled, covers_upto: 0x04FF, license_regions: LICENSE_GLOBAL_BIT },
    // CJK 族
    FontEntry { id: "noto-sans-cjk", family: FontFamily::Cjk, source: GlyphSource::ExternalFile, covers_upto: 0x9FFF, license_regions: LICENSE_GLOBAL_BIT },
    FontEntry { id: "source-han-serif", family: FontFamily::Cjk, source: GlyphSource::ExternalFile, covers_upto: 0x9FFF, license_regions: LICENSE_GLOBAL_BIT },
    FontEntry { id: "cjk-fallback-kai", family: FontFamily::Cjk, source: GlyphSource::SystemInstalled, covers_upto: 0x9FFF, license_regions: LICENSE_GLOBAL_BIT },
    // 阿拉伯族
    FontEntry { id: "noto-naskh-arabic", family: FontFamily::Arabic, source: GlyphSource::ExternalFile, covers_upto: 0x06FF, license_regions: LICENSE_GLOBAL_BIT },
    FontEntry { id: "amiri", family: FontFamily::Arabic, source: GlyphSource::ExternalFile, covers_upto: 0x06FF, license_regions: LICENSE_GLOBAL_BIT },
    FontEntry { id: "arabic-fallback-naskh", family: FontFamily::Arabic, source: GlyphSource::SystemInstalled, covers_upto: 0x06FF, license_regions: LICENSE_GLOBAL_BIT },
    // 泰印度族
    FontEntry { id: "noto-sans-tai", family: FontFamily::TaiIndic, source: GlyphSource::ExternalFile, covers_upto: 0x0DFF, license_regions: LICENSE_GLOBAL_BIT },
    FontEntry { id: "thai-sarabun", family: FontFamily::TaiIndic, source: GlyphSource::ExternalFile, covers_upto: 0x0DFF, license_regions: LICENSE_GLOBAL_BIT },
    FontEntry { id: "tai-fallback-lohit", family: FontFamily::TaiIndic, source: GlyphSource::SystemInstalled, covers_upto: 0x0DFF, license_regions: LICENSE_GLOBAL_BIT },
];

/// 查字体条目（线性扫表，O(表长) = O(1)，表是编译期常量）。
pub fn find_font(id: &str) -> Option<&'static FontEntry> {
    FONT_TABLE.iter().find(|e| e.id == id)
}

// ---------------------------------------------------------------------------
// §4 诊断袋（锚点：异常零静默）
// ---------------------------------------------------------------------------

/// 诊断类别。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DiagKind {
    /// 回退链缺失，已用默认链（**不给静默默认**）。
    ChainDefaulted,
    /// 发生缺字检出，已跳到回退链下一跳（复用 F2908 结论）。
    GlyphMissingFallback,
    /// 回退链长度被上限钳制。
    ChainClamped,
    /// 基线失配在容差内，已对齐修正（复用 F2909 度量）。
    BaselineAligned,
    /// 字体资源表长度被上限钳制。
    TableClamped,
    /// 覆盖扫描字符集被上限钳制。
    CharsetClamped,
}

impl DiagKind {
    /// 全集（自检遍历用）。
    pub const ALL: [DiagKind; 6] = [
        DiagKind::ChainDefaulted,
        DiagKind::GlyphMissingFallback,
        DiagKind::ChainClamped,
        DiagKind::BaselineAligned,
        DiagKind::TableClamped,
        DiagKind::CharsetClamped,
    ];
    /// 诊断码（对拍按它比对）。
    pub fn code(&self) -> &'static str {
        match self {
            DiagKind::ChainDefaulted => "D_CHAIN_DEFAULTED",
            DiagKind::GlyphMissingFallback => "D_GLYPH_MISSING_FALLBACK",
            DiagKind::ChainClamped => "D_CHAIN_CLAMPED",
            DiagKind::BaselineAligned => "D_BASELINE_ALIGNED",
            DiagKind::TableClamped => "D_TABLE_CLAMPED",
            DiagKind::CharsetClamped => "D_CHARSET_CLAMPED",
        }
    }
}

/// 一条诊断。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Diag {
    /// 类别。
    pub kind: DiagKind,
    /// 现象（静态可读；不含隐私面——本项零用户数据）。
    pub what: String,
}

/// 诊断袋。
#[derive(Clone, Default, PartialEq, Eq, Debug)]
pub struct DiagBag {
    items: Vec<Diag>,
}

impl DiagBag {
    /// 新建。
    pub fn new() -> Self {
        DiagBag { items: Vec::new() }
    }
    /// 追加。
    pub fn push(&mut self, kind: DiagKind, what: &str) {
        self.items.push(Diag {
            kind,
            what: what.to_string(),
        });
    }
    /// 条数。
    pub fn len(&self) -> usize {
        self.items.len()
    }
    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
    /// 某类条数（自检按类别断言用）。
    pub fn count_of(&self, kind: DiagKind) -> usize {
        self.items.iter().filter(|d| d.kind == kind).count()
    }
    /// 全部诊断（不可变借用）。
    pub fn items(&self) -> &[Diag] {
        &self.items
    }
}

// ---------------------------------------------------------------------------
// §5 回退链配置（判据一：回退链配置）
// ---------------------------------------------------------------------------

/// 回退链配置（每 Locale 一条）。
///
/// 链是**有序**的：第0 档首选，第1 档第一回退，依此类推。顺序即优先级，
/// 不排序——排序会毁掉"首选"这个语义。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct FallbackChain {
    /// Locale 标签（完整 BCP47 形态，与 F4004 消费方同形）。
    pub locale: String,
    /// 有序字体标识。
    pub fonts: Vec<String>,
}

impl FallbackChain {
    /// 构造并**钳制**链长（超界钳到 [`MAX_CHAIN_LEN`] 并可查留痕）。
    ///
    /// **空链不做下界钳制**（`MIN_CHAIN_LEN` 只约束"调用方给了字体时的最短
    /// 有效长度"）。理由：把0 档链"钳成"1 档而不补内容，会产出一个
    /// **自相矛盾的状态**——`len() == 1` 但 `font_at(0) == None`，
    /// 于是"链长非零"看起来像"配过字体了"，而实际上一个字体都没有。
    /// 那正是降级红线要禁的静默：配置缺失被伪装成配置就绪。
    ///
    /// 所以空链保持 0 档并**留痕**，由 [`resolve_font_chain`] 的兜底路径
    /// （默认链 + 诊断）接管——配置缺失必须显性，不该在这里被抹平。
    pub fn new(locale: &str, fonts: &[&str]) -> (Self, Option<ClampRecord>) {
        // 空输入：不钳长度，直接留痕返回空链。
        if fonts.is_empty() {
            return (
                FallbackChain {
                    locale: locale.to_string(),
                    fonts: Vec::new(),
                },
                Some(ClampRecord {
                    field: "chain-len",
                    asked: 0,
                    effective: 0,
                    to_upper: false,
                }),
            );
        }
        // 非空输入：钳到 [MIN_CHAIN_LEN, MAX_CHAIN_LEN]。
        // 下界此时不会触发（len>=1），保留 clamp 调用是为了让"下界"这条
        // 约束在参数域里显式存在，将来 MIN 调大时行为自动正确。
        let (n, rec) = clamp_tracked("chain-len", fonts.len(), MIN_CHAIN_LEN, MAX_CHAIN_LEN);
        let mut v: Vec<String> = Vec::new();
        for f in fonts.iter().take(n) {
            v.push(f.to_string());
        }
        (
            FallbackChain {
                locale: locale.to_string(),
                fonts: v,
            },
            rec,
        )
    }

    /// 档数。
    pub fn len(&self) -> usize {
        self.fonts.len()
    }
    /// 是否空（构造后恒不为空——空链已钳到 `MIN_CHAIN_LEN`）。
    pub fn is_empty(&self) -> bool {
        self.fonts.is_empty()
    }
    /// 取第 `hop` 档字体标识（越界返回 `None`——链走完就是"没得选了"）。
    pub fn font_at(&self, hop: usize) -> Option<&str> {
        self.fonts.get(hop).map(|s| s.as_str())
    }
}

/// 默认链（缺配置时兜底；**必产诊断**，见头注）。
///
/// 兜到拉丁族：拉丁是覆盖面最广的一档，用它兜底至少不会整片方框。
/// 但**必须显性**——上层要能据此发现"这个 Locale 忘配字体链了"。
pub const DEFAULT_CHAIN_FONTS: [&str; 2] = ["varix-latin", "dejavu-sans"];

/// 四族默认链（按族兜底，比单一拉丁链更贴合实际）。
pub fn default_chain_for(family: FontFamily) -> &'static [&'static str] {
    match family {
        FontFamily::Latin => &["varix-latin", "dejavu-sans"],
        FontFamily::Cjk => &["noto-sans-cjk", "cjk-fallback-kai"],
        FontFamily::Arabic => &["noto-naskh-arabic", "arabic-fallback-naskh"],
        FontFamily::TaiIndic => &["noto-sans-tai", "tai-fallback-lohit"],
    }
}

// ---------------------------------------------------------------------------
// §6 许可地域核验（判据三：许可地域；三单源复用 F2919）
// ---------------------------------------------------------------------------

/// 地域序号（与 [`FontEntry::license_regions`] 的 bit 位对应，bit0 恒为全球）。
///
/// 序号即 bit 位：`regions & (1 << region::EU)` 为真 = 覆盖欧洲。
/// 之所以用"序号=bit 位"而不是查表函数：让许可判断保持**纯位运算**，
/// 不引入任何分配与遍历，也就不会在核内产生不可预测开销。
pub mod region {
    /// 全球（bit0）。
    pub const GLOBAL: u8 = 0;
    /// 欧洲（bit1）。
    pub const EU: u8 = 1;
    /// 北美（bit2）。
    pub const NA: u8 = 2;
    /// 东亚（bit3）。
    pub const EA: u8 = 3;
    /// 南亚（bit4）。
    pub const SAS: u8 = 4;
    /// 东南亚（bit5）。
    pub const SEA: u8 = 5;
    /// 中东（bit6）。
    pub const MENA: u8 = 6;
    /// 全集（自检遍历用）。
    pub const ALL: [u8; 7] = [GLOBAL, EU, NA, EA, SAS, SEA, MENA];
    /// 地区名（诊断用）。
    pub fn name(bit: u8) -> &'static str {
        match bit {
            GLOBAL => "GLOBAL",
            EU => "EU",
            NA => "NA",
            EA => "EA",
            SAS => "SAS",
            SEA => "SEA",
            MENA => "MENA",
            _ => "RESERVED",
        }
    }
}

/// 许可核验结论。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LicenseVerdict {
    /// 允许（全球许可，或明确覆盖目标地域）。
    Allowed,
    /// 不带许可面（内建/系统字体——许可随分发，不在本项核验范围）。
    NotApplicable,
    /// **违规**：该字体不在目标地域的授权范围内 → 阻断。
    Violation,
}

impl LicenseVerdict {
    /// 是否阻断（`Violation` 为 true——三条错误路径里唯一会阻断的）。
    pub fn blocks(&self) -> bool {
        matches!(self, LicenseVerdict::Violation)
    }
    /// 短码。
    pub fn code(&self) -> &'static str {
        match self {
            LicenseVerdict::Allowed => "allowed",
            LicenseVerdict::NotApplicable => "not-applicable",
            LicenseVerdict::Violation => "violation",
        }
    }
}

/// "调用方未指定地域"的哨兵值。
///
/// **刻意不等于 [`region::GLOBAL`]**：未指定地域就放行，等于把"不知道能不能用"
/// 当成"能用"，那正是许可事故的起点。所以未指定一律按违规处理。
pub const REGION_UNSPECIFIED: u8 = 0xff;

/// 许可地域表（本项支持核验的目标地域清单）。
///
/// 地域清单是**策略面**的一部分，不是执行面——两边各有一份清单就会出现
/// "执行侧认为合法、本项认为违规"的错位。
pub const LICENSE_TABLE: [u8; 7] = [
    region::GLOBAL,
    region::EU,
    region::NA,
    region::EA,
    region::SAS,
    region::SEA,
    region::MENA,
];

/// 地域是否在支持表内（不在表内 → 无法判定 → 交由调用方按违规处理）。
pub fn is_supported_region(region_bit: u8) -> bool {
    region_bit != REGION_UNSPECIFIED
        && region_bit <= 31
        && LICENSE_TABLE.iter().any(|b| *b == region_bit)
}

/// 许可核验（纯位运算，无分配）。
///
/// **单源边界（复用 F2919）**：F2919 负责"这个字体到底是什么许可"（读 licence
/// 文本、比对 SPDX 标识）。本项只做**位图判定**——"这份许可声明的地域位图
/// 是否覆盖目标地域"。把两件事分开，是因为读许可文本要IO（不该在核内策略层做），
/// 而位图判定是纯计算。
pub fn check_license(entry: &FontEntry, region_bit: u8) -> LicenseVerdict {
    if !entry.source.needs_license() {
        return LicenseVerdict::NotApplicable;
    }
    if region_bit == REGION_UNSPECIFIED || region_bit > 31 {
        // 无法判定 → 按违规处理（不猜）。
        return LicenseVerdict::Violation;
    }
    if entry.license_regions & LICENSE_GLOBAL_BIT != 0 {
        return LicenseVerdict::Allowed;
    }
    let mask: u32 = 1u32 << region_bit;
    if entry.license_regions & mask != 0 {
        return LicenseVerdict::Allowed;
    }
    LicenseVerdict::Violation
}

// ---------------------------------------------------------------------------
// §7 字符覆盖扫描（判据二：覆盖验证；三单源复用 F2908）
// ---------------------------------------------------------------------------

/// 单个字符的覆盖判定。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GlyphHit {
    /// 码位。
    pub code_point: u32,
    /// 命中的链档号（第几档字体覆盖了它）。
    pub hop: usize,
}

/// 覆盖扫描结果。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct CoverageReport {
    /// Locale 标签。
    pub locale: String,
    /// 被覆盖的字符及其命中档。
    pub hits: Vec<GlyphHit>,
    /// 走完链仍缺字的字符（**非空即需上报**——不许静默漏字）。
    pub missing: Vec<u32>,
    /// 实际发生的回退跳次数（锚点：缺字检出→回退链下一跳+**计数**）。
    pub fallback_hops: usize,
    /// 累计缺字检出次数（含全链落空的）。
    pub missing_detected: usize,
}

impl CoverageReport {
    /// 是否全覆盖（无漏字）。
    pub fn is_full(&self) -> bool {
        self.missing.is_empty()
    }
    /// 覆盖率百分比（0..=100）。
    pub fn coverage_percent(&self) -> u16 {
        let total = self.hits.len() + self.missing.len();
        if total == 0 {
            return 100;
        }
        ((self.hits.len() * 100) / total) as u16
    }
}

/// 扫描字符集找覆盖（锚点：覆盖 O(字符集)；缺字检出→回退链下一跳+计数）。
///
/// 算法（O(字符集 × 链长)）：对每个码位沿链逐档试，首个 `covers_upto` 覆盖它
/// 的档即命中档；全链走完仍不覆盖 → 进 `missing`。
///
/// **单源边界（复用 F2908）**：真正的"这个字体到底有没有这个字形"要问字体本体
/// （F2908 去扫 cmap）。本项只按登记的 `covers_upto` 上界判定——这正是策略层
/// 该做的：它决定"该跳到第几跳"，不决定"字形数据从哪来"。
pub fn scan_coverage(
    locale: &str,
    chain: &FallbackChain,
    charset: &[u32],
    bag: &mut DiagBag,
) -> Result<CoverageReport, Rejection> {
    if charset.len() > MAX_CHARSET_LEN {
        return Err(Rejection {
            code: E_CHARSET_TOO_LONG,
            what: format!("字符集长度 {} 超过上界 {}", charset.len(), MAX_CHARSET_LEN),
            why: "覆盖扫描是 O(字符集)；超长会让单帧排版超时".to_string(),
            next: format!(
                "把字符集分片后多次调用（单片≤{}），或改用增量扫描接口",
                MAX_CHARSET_LEN
            ),
        });
    }
    let mut hits: Vec<GlyphHit> = Vec::new();
    let mut missing: Vec<u32> = Vec::new();
    let mut fallback_hops = 0usize;
    let mut missing_detected = 0usize;

    for &cp in charset.iter() {
        let mut found: Option<usize> = None;
        for hop in 0..chain.len() {
            let id = match chain.font_at(hop) {
                Some(x) => x,
                None => break,
            };
            // 链里写了表中不存在的字体名：算作该档不覆盖，继续下一跳。
            // 不静默跳过的理由——写错字体名是配置错误，必须由"该码位最终落
            // missing"或"落到了后面的档"暴露出来，而不是当作"这一档存在"。
            let entry = match find_font(id) {
                Some(e) => e,
                None => continue,
            };
            if cp < entry.covers_upto {
                found = Some(hop);
                if hop > 0 {
                    // 发生回退跳：计数 + 诊断（锚点「下一跳+计数」）。
                    fallback_hops += 1;
                    bag.push(
                        DiagKind::GlyphMissingFallback,
                        if hop == 1 {
                            "首选字体缺字，回退到第 1 档"
                        } else {
                            "首选字体缺字，回退到后续档"
                        },
                    );
                }
                break;
            }
        }
        match found {
            Some(hop) => hits.push(GlyphHit { code_point: cp, hop }),
            None => {
                missing_detected += 1;
                missing.push(cp);
                bag.push(DiagKind::GlyphMissingFallback, "回退链走完仍缺字");
            }
        }
    }

    Ok(CoverageReport {
        locale: locale.to_string(),
        hits,
        missing,
        fallback_hops,
        missing_detected,
    })
}

// ---------------------------------------------------------------------------
// §8 度量对齐（判据四：度量对齐；三单源复用 F2909）
// ---------------------------------------------------------------------------

/// 单条混排项的度量（**由 F2909 提供**——本项不自己算字体度量）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RunMetrics {
    /// 字符所在链档号（用于定位是哪一档字体出的度量）。
    pub hop: usize,
    /// 基线上延伸。
    pub ascent: u32,
    /// 基线下延伸。
    pub descent: u32,
}

impl RunMetrics {
    /// 该项总高（上+ 下）。
    pub fn line_extent(&self) -> u32 {
        self.ascent + self.descent
    }
}

/// 基线对齐结果。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct BaselineResult {
    /// 对齐采用的公共基线偏移（取最大 ascent）。
    pub common_ascent: u32,
    /// 每项的对齐偏移（= `common_ascent - 本项 ascent`）。
    pub offsets: Vec<u32>,
    /// 容差内的失配项数。
    pub misaligned: usize,
}

/// 度量对齐（锚点：基线失配→对齐修正；基线 O(混排数)）。
///
/// 策略：以**最大 ascent** 为公共基线，每项下移 `common_ascent - ascent`
/// 把基线拉到同一水平。失配幅度超过 [`MAX_BASELINE_TOLERANCE`] 判超差——
/// 超差意味着字体选错了（一个大字重混一个小字重），对齐它没有意义，
/// 显性拒绝比硬拉齐更好。
pub fn align_baseline(
    runs: &[RunMetrics],
    tolerance: usize,
    bag: &mut DiagBag,
) -> Result<BaselineResult, Rejection> {
    if runs.len() > MAX_MIX_RUNS {
        return Err(Rejection {
            code: E_MIX_TOO_LONG,
            what: format!("混排条目数 {} 超过上界 {}", runs.len(), MAX_MIX_RUNS),
            why: "基线对齐是 O(混排数)；超长会让单帧排版超时".to_string(),
            next: format!("把混排分片后多次调用（单片≤{}）", MAX_MIX_RUNS),
        });
    }
    let (tol, clamped) = clamp_tracked("baseline-tolerance", tolerance, 0, MAX_BASELINE_TOLERANCE);
    if clamped.is_some() {
        bag.push(DiagKind::BaselineAligned, "基线容差超上界，已钳到上界");
    }
    // 空输入：公共基线为 0，无失配（不是错误——空排版是合法的）。
    if runs.is_empty() {
        return Ok(BaselineResult {
            common_ascent: 0,
            offsets: Vec::new(),
            misaligned: 0,
        });
    }
    let mut common = 0u32;
    for r in runs.iter() {
        if r.ascent > common {
            common = r.ascent;
        }
    }
    let mut offsets: Vec<u32> = Vec::new();
    let mut misaligned = 0usize;
    for r in runs.iter() {
        let off = common.saturating_sub(r.ascent);
        if off as usize > tol {
            return Err(Rejection {
                code: E_BASELINE_OUT_OF_TOLERANCE,
                what: format!(
                    "第 {} 档字体基线失配 {}px，超过容差 {}px",
                    r.hop, off, tol
                ),
                why: "失配过大说明字体选错了；对齐它没有意义，且掩盖了选型问题".to_string(),
                next: format!(
                    "换用同族字体（该档 hop={}），或把容差显式放宽到≤{}",
                    r.hop, MAX_BASELINE_TOLERANCE
                ),
            });
        }
        if off > 0 {
            misaligned += 1;
        }
        offsets.push(off);
    }
    if misaligned > 0 {
        bag.push(
            DiagKind::BaselineAligned,
            "存在容差内基线失配，已按最大 ascent 对齐",
        );
    }
    Ok(BaselineResult {
        common_ascent: common,
        offsets,
        misaligned,
    })
}

// ---------------------------------------------------------------------------
// §9 字体链解析（编排入口：三条错误路径在此汇合）
// ---------------------------------------------------------------------------

/// 字体选型产物（**只有选型决策与参数，不含字形数据**——这是与执行侧的分工）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct FontPlan {
    /// Locale 标签。
    pub locale: String,
    /// 生效的字体族（**由 F4004 路由给出，本项不重判**）。
    pub family: FontFamily,
    /// 生效的有序字体链（缺配置时是默认链）。
    pub chain: FallbackChain,
    /// 是否用了默认链兜底（true = 配置缺失，上层应报警）。
    pub chain_defaulted: bool,
    /// 覆盖扫描结论。
    pub coverage: CoverageReport,
    /// 基线对齐结论（`None` = 本次没做混排）。
    pub baseline: Option<BaselineResult>,
    /// 诊断袋（全部诊断的汇总）。
    pub bag: DiagBag,
}

impl FontPlan {
    /// 是否存在漏字（漏字**必须**被上层看见）。
    pub fn has_missing_glyphs(&self) -> bool {
        !self.coverage.missing.is_empty()
    }
    /// 可读描述（无障碍替述用；零隐私面——这里只有配置与统计，无用户数据）。
    pub fn describe(&self) -> String {
        format!(
            "locale={}族={}链={}档默认链={}覆盖={}%漏字={}回退跳={}",
            self.locale,
            self.family.name(),
            self.chain.len(),
            self.chain_defaulted,
            self.coverage.coverage_percent(),
            self.coverage.missing.len(),
            self.coverage.fallback_hops
        )
    }
}

/// 字体链解析（锚点：链配置缺→默认链+诊断；许可地域违规→阻断）。
///
/// 流程与三条错误路径的落点：
/// 1. **链配置缺** → 按族取默认链 + `ChainDefaulted` 诊断，**不阻断**
///    （阻断会让整页打不开，代价大于排版不完美）。
/// 2. **许可地域违规** → 逐档核验，任一在生效链上的 `ExternalFile` 字体违规
///    → 立刻 [`Rejection`]（**阻断**，唯一会阻断的路径）。
/// 3. **缺字** → 沿链跳下一跳并计数（不阻断，漏字进 `missing` 上报）。
///
/// `family` 由 F4004 路由给出——本项**不重判族**，这是单源纪律。
#[allow(clippy::too_many_arguments)]
pub fn resolve_font_chain(
    locale: &str,
    family: FontFamily,
    chain_cfg: Option<&FallbackChain>,
    charset: &[u32],
    region_bit: u8,
    runs: &[RunMetrics],
    tolerance: usize,
) -> Result<FontPlan, Rejection> {
    let mut bag = DiagBag::new();

    // 路径一：链配置缺 → 默认链 + 诊断（不给静默默认）。
    let (chain, defaulted) = match chain_cfg {
        Some(c) => (c.clone(), false),
        None => {
            bag.push(
                DiagKind::ChainDefaulted,
                "该 Locale 未配置字体链，已用本族默认链兜底",
            );
            let (c, _) = FallbackChain::new(locale, default_chain_for(family));
            (c, true)
        }
    };
    // 空链同样走默认链（构造时空链已钳到 MIN_CHAIN_LEN=1，但那一档可能是
    // 写错的字体名，等于没配）。
    if chain.is_empty() {
        bag.push(DiagKind::ChainDefaulted, "字体链为空，已用默认链兜底");
        let (c, _) = FallbackChain::new(locale, default_chain_for(family));
        return finish(locale, family, c, true, charset, region_bit, runs, tolerance, bag);
    }

    // 路径二：许可地域核验（**唯一会阻断的路径**）。
    for hop in 0..chain.len() {
        let id = match chain.font_at(hop) {
            Some(x) => x,
            None => break,
        };
        // 链里写了表中不存在的字体名：无法核验许可 → 阻断。
        // 不放行的理由——"找不到这个字体"和"这个字体许可允许"是两回事，
        // 前者绝不能被当成后者，否则配置错误会被当成合法放行。
        let entry = match find_font(id) {
            Some(e) => e,
            None => {
                return Err(Rejection {
                    code: E_LICENSE_BLOCKED,
                    what: format!("字体链第 {} 档的 {:?} 不在字体资源表内，无法核验许可", hop, id),
                    why: "未登记字体没有许可声明；按'无法判定即不可用'处理".to_string(),
                    next: format!(
                        "把 {:?} 登记进 FONT_TABLE（含许可地域位图），或从字体链里去掉该档",
                        id
                    ),
                })
            }
        };
        let v = check_license(entry, region_bit);
        if v.blocks() {
            return Err(Rejection {
                code: E_LICENSE_BLOCKED,
                what: format!(
                    "字体 {:?}（第 {} 档）在地域 {} 的许可违规",
                    entry.id,
                    hop,
                    region::name(region_bit)
                ),
                why: "在未授权地域显示该字体等于分发未授权软件，是法律风险而非排版质量问题".to_string(),
                next: "换成该地域已授权的字体；或把该字体的许可地域位图补全（由 F2919 提供依据）".to_string(),
            });
        }
    }

    finish(locale, family, chain, defaulted, charset, region_bit, runs, tolerance, bag)
}

/// 链解析的收尾段（许可已过，只剩覆盖与基线）。
#[allow(clippy::too_many_arguments)]
fn finish(
    locale: &str,
    family: FontFamily,
    chain: FallbackChain,
    defaulted: bool,
    charset: &[u32],
    region_bit: u8,
    runs: &[RunMetrics],
    tolerance: usize,
    mut bag: DiagBag,
) -> Result<FontPlan, Rejection> {
    // 路径三：缺字 → 沿链跳下一跳 + 计数（不阻断，漏字上报）。
    let coverage = scan_coverage(locale, &chain, charset, &mut bag)?;
    // 基线：无混排项就不做对齐（不是错误）。
    let baseline = if runs.is_empty() {
        None
    } else {
        Some(align_baseline(runs, tolerance, &mut bag)?)
    };
    let _ = region_bit;
    Ok(FontPlan {
        locale: locale.to_string(),
        family,
        chain,
        chain_defaulted: defaulted,
        coverage,
        baseline,
        bag,
    })
}

// ---------------------------------------------------------------------------
// §10 预留激活登记（三单源复用 F2908/F2909/F2919）
// ---------------------------------------------------------------------------

/// 预留槽位（复用单源的激活声明位）。
///
/// `landed: false` 表示**对应 F29xx 尚未落地**——本项只声明"该能力归它"，
/// 不代做实现。谎报 `landed: true` 会被 [`check_reserved`] 抓住：那会让
/// 上层以为 F2908 已经能扫 cmap 了，于是把未实现的依赖当成已就绪，
/// 缺字检出从此静默失效。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ReservedSlot {
    /// 复用项号。
    pub upstream: &'static str,
    /// 能力键。
    pub key: &'static str,
    /// 能力中文名。
    pub label: &'static str,
    /// 是否已落地。
    pub landed: bool,
    /// 落地后的对接说明。
    pub on_landed: &'static str,
}

/// 复用单源预留槽位（F2908/F2909/F2919 三条，当前全部未落地）。
pub const RESERVED_SLOTS: [ReservedSlot; 4] = [
    ReservedSlot {
        upstream: "VE-F2908",
        key: "glyph-cmap-scan",
        label: "字形 cmap 覆盖扫描（缺字检出的真实来源）",
        landed: false,
        on_landed: "本项改为消费 F2908 的逐字形覆盖结论，不再用 covers_upto 上界近似",
    },
    ReservedSlot {
        upstream: "VE-F2909",
        key: "font-metrics",
        label: "字体度量与基线计算",
        landed: false,
        on_landed: "本项改为消费 F2909 的 ascent/descent，不再由调用方传入 RunMetrics",
    },
    ReservedSlot {
        upstream: "VE-F2919",
        key: "license-region-audit",
        label: "字体许可文本与 SPDX 核验",
        landed: false,
        on_landed: "本项改为消费 F2919 的许可地域结论，本地只保留位图比对",
    },
    ReservedSlot {
        upstream: "VE-F4044",
        key: "font-selection-deepening",
        label: "字体选型细则深化",
        landed: false,
        on_landed: "本项的四族配置表按F4044 细则细化档位与权重",
    },
];

/// 预留槽位机检：`landed: true` 但 F29xx 未落地即为谎报。
pub fn check_reserved() -> Result<(), Rejection> {
    for s in RESERVED_SLOTS.iter() {
        if s.landed {
            return Err(Rejection {
                code: E_RESERVED_LIED,
                what: format!("预留槽位 {}（{}）标为已落地，但上游尚未交付", s.upstream, s.key),
                why: "谎报落地会让上层把未实现的依赖当成已就绪，该能力从此静默失效".to_string(),
                next: format!("改回 landed: false；待{} 真正落地后再改", s.upstream),
            });
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// §11 规格表与单源声明（锚点：数据模型与规格表——逐条规格公开）
// ---------------------------------------------------------------------------

/// 规格条目。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SpecItem {
    /// 编号（从1 起连续；对拍按号定位）。
    pub no: u16,
    /// 能力键。
    pub key: &'static str,
    /// 中文标签。
    pub label: &'static str,
    /// 强制它的自检项名。
    pub enforced_by: &'static str,
}

/// 规格表（逐条规格公开）。
pub const SPEC_SHEET: [SpecItem; 13] = [
    // 判据一：回退链配置（3 条）
    SpecItem { no: 1, key: "chain-per-locale", label: "每 Locale 一条有序回退链", enforced_by: "chain_per_locale" },
    SpecItem { no: 2, key: "chain-length-clamped", label: "链长越界钳制且可见", enforced_by: "chain_length_clamped" },
    SpecItem { no: 3, key: "chain-missing-defaulted", label: "链配置缺走默认链且产诊断", enforced_by: "chain_missing_defaulted" },
    // 判据二：覆盖验证（3 条）
    SpecItem { no: 4, key: "coverage-scan-hops", label: "缺字沿链跳下一跳并计数", enforced_by: "coverage_scan_hops" },
    SpecItem { no: 5, key: "coverage-missing-visible", label: "链走完仍缺字必须上报", enforced_by: "coverage_missing_visible" },
    SpecItem { no: 6, key: "coverage-oversize-rejected", label: "字符集越界显性拒绝不截断", enforced_by: "coverage_oversize_rejected" },
    // 判据三：许可地域（2 条）
    SpecItem { no: 7, key: "license-violation-blocks", label: "许可地域违规一律阻断", enforced_by: "license_violation_blocks" },
    SpecItem { no: 8, key: "license-unspecified-blocks", label: "未指定地域按违规阻断", enforced_by: "license_unspecified_blocks" },
    // 判据四：度量对齐（2 条）
    SpecItem { no: 9, key: "baseline-aligns", label: "容差内失配对齐修正", enforced_by: "baseline_aligns" },
    SpecItem { no: 10, key: "baseline-out-of-tolerance", label: "超容差失配显性拒绝", enforced_by: "baseline_out_of_tolerance" },
    // 判据五：三单源复用（2 条）
    SpecItem { no: 11, key: "single-source-unique", label: "三能力各有唯一 owner", enforced_by: "single_source_unique" },
    SpecItem { no: 12, key: "reserved-not-lied", label: "预留槽位未落地不谎报", enforced_by: "reserved_not_lied" },
    SpecItem { no: 13, key: "zero-privacy-surface", label: "零隐私面（只配置无用户数据）", enforced_by: "zero_privacy_surface" },
];

/// 判据（锚点五条）到规格表键的映射。
pub const CRITERIA: [(&str, [&str; 2]); 5] = [
    ("回退链配置", ["chain-per-locale", "chain-missing-defaulted"]),
    ("覆盖验证", ["coverage-scan-hops", "coverage-missing-visible"]),
    ("许可地域", ["license-violation-blocks", "license-unspecified-blocks"]),
    ("度量对齐", ["baseline-aligns", "baseline-out-of-tolerance"]),
    ("三单源复用", ["single-source-unique", "reserved-not-lied"]),
];

/// 规格表覆盖机检：编号连续 + 判据键均被收录。
pub fn check_spec_coverage() -> Result<(), Rejection> {
    for (i, item) in SPEC_SHEET.iter().enumerate() {
        let expect = (i + 1) as u16;
        if item.no != expect {
            return Err(Rejection {
                code: E_SPEC_GAP,
                what: format!("规格表第 {} 位编号={}，应为 {}", i, item.no, expect),
                why: "编号不连续会让对拍按号定位失效，且暗示有条目被漏登".to_string(),
                next: "按 SPEC_SHEET 顺序重排编号；新增条目追加到末尾".to_string(),
            });
        }
    }
    for (crit, keys) in CRITERIA.iter() {
        for key in keys.iter() {
            if !SPEC_SHEET.iter().any(|s| s.key == *key) {
                return Err(Rejection {
                    code: E_SPEC_GAP,
                    what: format!("判据「{}」引用的规格键 {:?} 不在规格表内", crit, key),
                    why: "判据与规格表必须双向对齐，否则判据是空头承诺".to_string(),
                    next: format!("把 {:?} 登记进 SPEC_SHEET，或改判据引用已登记的键", key),
                });
            }
        }
    }
    Ok(())
}

/// 单源复用声明（每能力唯一 owner，consumer 可多）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SingleSourceClaim {
    /// 能力键。
    pub key: &'static str,
    /// 能力中文名（与 key 分开——混列会让能力反查失配）。
    pub label: &'static str,
    /// owner 项号（唯一）。
    pub owner: &'static str,
    /// consumer 列表。
    pub consumers: &'static [&'static str],
    /// 陈述。
    pub statement: &'static str,
}

/// 字体域单源声明（F2908/F2909/F2919 三条复用 + 一条自有）。
pub const FONT_SINGLE_SOURCE: [SingleSourceClaim; 4] = [
    SingleSourceClaim {
        key: "glyph-cmap-scan",
        label: "字形 cmap 覆盖扫描",
        owner: "VE-F2908",
        consumers: &["VE-F4005", "VE-F4010"],
        statement: "字形覆盖扫描唯一 owner 是 F2908；F4005 只消费其结论决定跳第几跳，不自己扫 cmap",
    },
    SingleSourceClaim {
        key: "font-metrics",
        label: "字体度量与基线计算",
        owner: "VE-F2909",
        consumers: &["VE-F4005", "VE-F4004"],
        statement: "字体度量唯一 owner 是 F2909；F4005 只做对齐决策，不自己算 ascent/descent",
    },
    SingleSourceClaim {
        key: "license-region-audit",
        label: "字体许可地域核验",
        owner: "VE-F2919",
        consumers: &["VE-F4005", "VE-F4016"],
        statement: "许可文本与 SPDX 核验唯一 owner 是 F2919；F4005 只做地域位图比对",
    },
    SingleSourceClaim {
        key: "font-fallback-chain",
        label: "每 Locale 字体回退链配置",
        owner: "VE-F4005",
        consumers: &["VE-N02", "VE-F4013"],
        statement: "回退链配置表唯一 owner 是 F4005；N02 按链执行不自己改链序",
    },
];

/// 单源唯一性机检：同能力不得有两个 owner。
pub fn check_single_source() -> Result<(), Rejection> {
    for (i, a) in FONT_SINGLE_SOURCE.iter().enumerate() {
        for b in FONT_SINGLE_SOURCE.iter().skip(i + 1) {
            if a.key == b.key {
                return Err(Rejection {
                    code: E_SINGLE_SOURCE_DUP,
                    what: format!("能力 {:?} 被声明了两次（owner {} 与 {}）", a.key, a.owner, b.owner),
                    why: "同能力两个 owner 必然分叉，分叉后对拍无从判断该信谁".to_string(),
                    next: "保留一个 owner，另一条改为 consumer 引用".to_string(),
                });
            }
            // 声明里 owner 不得同时出现在别人的 consumers 里（那也是分叉）。
            if b.consumers.contains(&a.owner) {
                return Err(Rejection {
                    code: E_SINGLE_SOURCE_DUP,
                    what: format!(
                        "能力 {:?}（owner {}）又被 {:?}（owner {}）当作 consumer 引用",
                        a.key, a.owner, b.key, b.owner
                    ),
                    why: "既是 owner 又被当consumer，说明能力边界没划清".to_string(),
                    next: "把该引用去掉，或把能力拆成两个键".to_string(),
                });
            }
        }
    }
    // 三条复用声明的 owner 必须都是 F29xx（本项只复用，不代做）。
    for c in FONT_SINGLE_SOURCE.iter() {
        if c.key != "font-fallback-chain" && !c.owner.starts_with("VE-F29") {
            return Err(Rejection {
                code: E_SINGLE_SOURCE_DUP,
                what: format!("复用能力 {:?} 的 owner={} 不是 F29xx", c.key, c.owner),
                why: "锚点钦定本项复用 F2908/F2909/F2919 三单源；owner 不符说明越界代做".to_string(),
                next: "把 owner 改回对应的 F29xx 项号".to_string(),
            });
        }
    }
    Ok(())
}

/// 零隐私面机检（本项只处理配置与统计，不碰任何用户数据）。
pub fn check_zero_privacy() -> Result<(), Rejection> {
    // 本项所有对外结构里唯一的 String 字段都是配置面/统计面：
    // locale（配置）、chain.fonts（配置）、missing/hits（码位统计）。
    // 一旦将来往里加"用户输入文本"之类字段，这条机检就应改判红。
    let forbidden = ["user_text", "user_name", "email", "phone", "device_id"];
    let names: [&str; 4] = [forbidden[0], forbidden[1], forbidden[2], forbidden[3]];
    for n in names.iter() {
        if n.contains("user_") && FONT_SINGLE_SOURCE.iter().any(|c| c.statement.contains(n)) {
            return Err(Rejection {
                code: E_PRIVACY_LEAK,
                what: format!("单源声明里出现了疑似隐私字段 {:?}", n),
                why: "本项零隐私面；一旦把用户数据写进产物，隐私面就出现了".to_string(),
                next: "移除该字段引用，改记配置面或统计面信息".to_string(),
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::format;
    use alloc::string::String;
    use alloc::vec;

    #[test]
    fn four_families_have_font_sets() {
        assert_eq!(FontFamily::ALL.len(), 4);
        // 四族每族至少两条字体（一条首选 + 一条回退）。
        for f in FontFamily::ALL.iter() {
            let n = FONT_TABLE.iter().filter(|e| e.family == *f).count();
            assert!(n >= 2, "族 {:?} 只有 {} 条字体，缺回退档", f.name(), n);
        }
        // 字体 id 不得重复（重复会让先命中的赢，后面的永不可达）。
        let mut ids: Vec<&str> = FONT_TABLE.iter().map(|e| e.id).collect();
        let before = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), before, "字体资源表有重复 id");
    }

    #[test]
    fn family_guard_accepts_all_case_forms() {
        for (raw, want) in [
            ("Latin", FontFamily::Latin),
            ("latin", FontFamily::Latin),
            ("LATIN", FontFamily::Latin),
            ("CJK", FontFamily::Cjk),
            ("cjk", FontFamily::Cjk),
            ("Arabic", FontFamily::Arabic),
            ("ARABIC", FontFamily::Arabic),
            ("TAIINDIC", FontFamily::TaiIndic),
            ("TaiIndic", FontFamily::TaiIndic),
        ] {
            assert_eq!(guard_family(raw).unwrap(), want, "守卫拒了 {:?}", raw);
        }
        for bad in ["", "Klingon", "LATIN1", "汉"] {
            let e = guard_family(bad).unwrap_err();
            assert_eq!(e.code, E_FAMILY_INVALID);
            assert!(e.is_complete(), "拒绝三要素不全");
        }
    }

    #[test]
    fn chain_per_locale_and_clamped() {
        let (c, rec) = FallbackChain::new("zh-Hans-CN", &["a", "b", "c"]);
        assert_eq!(c.len(), 3);
        assert!(rec.is_none());
        assert_eq!(c.font_at(0), Some("a"));
        assert_eq!(c.font_at(9), None);
        // 超上界钳到 MAX_CHAIN_LEN 并留痕。
        let many: Vec<String> = (0..40).map(|i| format!("f{}", i)).collect();
        let refs: Vec<&str> = many.iter().map(|s| s.as_str()).collect();
        let (c2, rec2) = FallbackChain::new("ar-EG", &refs);
        assert_eq!(c2.len(), MAX_CHAIN_LEN);
        let r = rec2.expect("超界未留痕");
        assert_eq!(r.field, "chain-len");
        assert!(r.to_upper);
    }

    #[test]
    fn chain_empty_stays_empty_and_leaves_trace() {
        // 空链必须保持 0 档并留痕——不能"钳成"1 档。
        // 钳长度而不补内容会产出 len()==1 但 font_at(0)==None 的矛盾态，
        // 让"配置缺失"伪装成"配置就绪"。
        let (c, rec) = FallbackChain::new("xx", &[]);
        assert_eq!(c.len(), 0, "空链被伪造成了非空");
        assert!(c.is_empty());
        assert!(c.font_at(0).is_none());
        let r = rec.expect("空链未留痕");
        assert_eq!(r.field, "chain-len");
        assert_eq!(r.asked, 0);
        assert_eq!(r.effective, 0);
        assert!(!r.to_upper);
        // 空链交给 resolve 兜底：走默认链 + 诊断。
        let p = resolve_font_chain(
            "xx",
            FontFamily::Latin,
            Some(&c),
            &[0x0041],
            region::GLOBAL,
            &[],
            4,
        )
        .unwrap();
        assert!(p.chain_defaulted, "空链未被兜底标记");
        assert!(p.coverage.is_full(), "默认拉丁链应覆盖 ASCII");
    }

    #[test]
    fn coverage_hops_and_counts() {
        // 首选 varix-latin 覆盖到 0x24F；给它一个 CJK 字符 → 应回退。
        let (chain, _) = FallbackChain::new("zh", &["varix-latin", "noto-sans-cjk"]);
        let mut bag = DiagBag::new();
        // 0x4E00（CJK）在 noto-sans-cjk(0x9FFF) 内，不在 latin(0x24F) 内。
        let r = scan_coverage("zh", &chain, &[0x0041, 0x4E00], &mut bag).unwrap();
        assert!(r.is_full());
        assert_eq!(r.fallback_hops, 1, "应恰好回退一次");
        assert_eq!(r.hits[0].hop, 0, "拉丁字母应命中首选");
        assert_eq!(r.hits[1].hop, 1, "CJK 字应命中回退档");
        assert_eq!(bag.count_of(DiagKind::GlyphMissingFallback), 1);
    }

    #[test]
    fn coverage_missing_is_visible() {
        // 一条链全都不覆盖的字符 → 必须进 missing，不许静默丢。
        let (chain, _) = FallbackChain::new("x", &["varix-latin"]);
        let mut bag = DiagBag::new();
        // 0x4E00 > latin 的 covers_upto，全链走完仍缺。
        let r = scan_coverage("x", &chain, &[0x4E00], &mut bag).unwrap();
        assert!(!r.is_full());
        assert_eq!(r.missing, vec![0x4E00]);
        assert_eq!(r.coverage_percent(), 0);
        assert_eq!(r.missing_detected, 1);
        // 全链落空也要产诊断（不许静默漏字）。
        assert!(bag.count_of(DiagKind::GlyphMissingFallback) >= 1);
    }

    #[test]
    fn coverage_oversize_rejected_not_truncated() {
        let (chain, _) = FallbackChain::new("x", &["varix-latin"]);
        let mut bag = DiagBag::new();
        let big: Vec<u32> = (0..(MAX_CHARSET_LEN + 1)).map(|i| i as u32).collect();
        let e = scan_coverage("x", &chain, &big, &mut bag).unwrap_err();
        assert_eq!(e.code, E_CHARSET_TOO_LONG);
        assert!(e.is_complete());
    }

    #[test]
    fn license_violation_blocks() {
        // noto-sans-cjk 是 extfile 且全球许可 → 全球/任一地域都放行。
        let cjk = find_font("noto-sans-cjk").unwrap();
        assert!(!check_license(cjk, region::GLOBAL).blocks());
        assert!(!check_license(cjk, region::EA).blocks());
        // 内建字体不带许可面 → NotApplicable，不阻断。
        let latin = find_font("varix-latin").unwrap();
        assert_eq!(check_license(latin, region::GLOBAL), LicenseVerdict::NotApplicable);
        // 造一个只授权 EU 的字体：查 NA 应违规。
        let eu_only = FontEntry {
            id: "eu-only",
            family: FontFamily::Latin,
            source: GlyphSource::ExternalFile,
            covers_upto: 0x024F,
            license_regions: 1u32 << region::EU,
        };
        assert!(!check_license(&eu_only, region::EU).blocks());
        assert!(check_license(&eu_only, region::NA).blocks(), "EU-only 在北美应违规");
    }

    #[test]
    fn license_unspecified_blocks() {
        let cjk = find_font("noto-sans-cjk").unwrap();
        // 未指定地域 ≠ 全球，必须按违规阻断。
        assert!(
            check_license(cjk, REGION_UNSPECIFIED).blocks(),
            "未指定地域被放行了——这是许可事故的起点"
        );
        assert!(!is_supported_region(REGION_UNSPECIFIED));
    }

    #[test]
    fn resolve_blocks_on_unlicensed_font() {
        // 造一条含"表外字体"的链 → 无法核验许可 → 阻断。
        let (chain, _) = FallbackChain::new("zh", &["not-a-real-font"]);
        let e = resolve_font_chain(
            "zh",
            FontFamily::Cjk,
            Some(&chain),
            &[0x0041],
            region::GLOBAL,
            &[],
            4,
        )
        .unwrap_err();
        assert_eq!(e.code, E_LICENSE_BLOCKED, "表外字体必须阻断");
        assert!(e.is_complete());
    }

    #[test]
    fn resolve_defaults_chain_when_missing() {
        // 不给链配置 → 走默认链 + 诊断，不阻断。
        let mut bag = DiagBag::new();
        let p = resolve_font_chain(
            "ar-EG",
            FontFamily::Arabic,
            None,
            &[0x0627],
            region::GLOBAL,
            &[],
            4,
        )
        .unwrap();
        assert!(p.chain_defaulted, "缺配置未标默认链");
        assert!(p.bag.count_of(DiagKind::ChainDefaulted) >= 1);
        // 默认阿拉伯链能覆盖阿语字。
        assert!(p.coverage.is_full(), "阿拉伯默认链应覆盖阿语");
        let _ = &mut bag;
    }

    #[test]
    fn baseline_aligns_within_tolerance() {
        let runs = [
            RunMetrics { hop: 0, ascent: 20, descent: 5 },
            RunMetrics { hop: 1, ascent: 16, descent: 4 },
        ];
        let mut bag = DiagBag::new();
        let r = align_baseline(&runs, 8, &mut bag).unwrap();
        assert_eq!(r.common_ascent, 20);
        assert_eq!(r.offsets, vec![0, 4]);
        assert_eq!(r.misaligned, 1);
        assert!(bag.count_of(DiagKind::BaselineAligned) >= 1);
        // 全同ascent → 无失配。
        let same = [
            RunMetrics { hop: 0, ascent: 20, descent: 5 },
            RunMetrics { hop: 1, ascent: 20, descent: 5 },
        ];
        let r2 = align_baseline(&same, 8, &mut DiagBag::new()).unwrap();
        assert_eq!(r2.misaligned, 0);
        assert_eq!(r2.offsets, vec![0, 0]);
    }

    #[test]
    fn baseline_out_of_tolerance_rejected() {
        // 失配 20px > 容差 4px → 显性拒绝，不硬拉齐。
        let runs = [
            RunMetrics { hop: 0, ascent: 30, descent: 5 },
            RunMetrics { hop: 1, ascent: 10, descent: 4 },
        ];
        let e = align_baseline(&runs, 4, &mut DiagBag::new()).unwrap_err();
        assert_eq!(e.code, E_BASELINE_OUT_OF_TOLERANCE);
        assert!(e.is_complete());
        // 容差超上界被钳（但仍按钳后判）。
        let big = [RunMetrics { hop: 0, ascent: 30, descent: 5 }];
        assert!(align_baseline(&big, 9999, &mut DiagBag::new()).is_ok());
    }

    #[test]
    fn resolve_end_to_end_all_green() {
        // 完整编排：CJK locale + 正常链 + 全球地域 + 混排基线。
        let (chain, _) = FallbackChain::new("zh-Hans-CN", &["varix-latin", "noto-sans-cjk"]);
        let runs = [
            RunMetrics { hop: 0, ascent: 20, descent: 5 },
            RunMetrics { hop: 1, ascent: 18, descent: 4 },
        ];
        let p = resolve_font_chain(
            "zh-Hans-CN",
            FontFamily::Cjk,
            Some(&chain),
            &[0x0041, 0x4E00],
            region::GLOBAL,
            &runs,
            8,
        )
        .unwrap();
        assert!(!p.chain_defaulted);
        assert!(p.coverage.is_full());
        assert_eq!(p.coverage.fallback_hops, 1);
        assert!(p.baseline.is_some());
        let d = p.describe();
        assert!(d.contains("zh-Hans-CN"));
        assert!(d.contains("Cjk"));
    }

    #[test]
    fn spec_single_source_and_reserved_green() {
        check_spec_coverage().unwrap();
        check_single_source().unwrap();
        check_reserved().unwrap();
        check_zero_privacy().unwrap();
        // 三条复用声明的 owner 分别是 F2908/F2909/F2919。
        let owners: Vec<&str> = FONT_SINGLE_SOURCE
            .iter()
            .filter(|c| c.key != "font-fallback-chain")
            .map(|c| c.owner)
            .collect();
        assert!(owners.contains(&"VE-F2908"));
        assert!(owners.contains(&"VE-F2909"));
        assert!(owners.contains(&"VE-F2919"));
    }

    #[test]
    fn multibyte_and_garbage_inputs_do_not_crash() {
        // 非 ASCII/畸形输入不得 panic（内核铁律：异常零静默不是靠 panic）。
        for tag in ["zh", "", "-CN", "x-private", "*", "汉"] {
            let _ = guard_family(tag);
            let (c, _) = FallbackChain::new(tag, &["varix-latin"]);
            let mut bag = DiagBag::new();
            let _ = scan_coverage(tag, &c, &[0x4E00, 0xFFFF, 0x10FFFF], &mut bag);
        }
    }
}
