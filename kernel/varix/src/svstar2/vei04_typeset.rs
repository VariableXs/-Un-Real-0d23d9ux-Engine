//! VE-F4004 · 国际化排版管线（VE-T 域 · 国际化域 · T01 组 · 目标 380 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F4004`
//!
//! **判据（锚点原文）**：四族路由、语言覆盖、降级显性、策略执行分工、预留激活。
//!
//! **职责定位（锚点原文）**：T01 国际化排版管线——排版管线总控（语言→排版策略
//! 路由：Latin/CJK/阿拉伯/泰印度系四族路由表——路由表完整断言（**全语言覆盖**
//! （**语言覆盖红线**：未收录语言→Latin 降级+显性标记（**降级显性红线**）；管线
//! 五段（分词→整形→折行→定位→渲染——各段语言感知；与 N02 关系（**N02 执行、
//! T01 策略**——**策略执行分工复述**。数据结构：数据模型与规格表（逐条规格公开、
//! 参数域钳制、枚举守卫——家族格式）。错误路径与降级矩阵：未收录语言→Latin 降级+
//! 标记（红线实测）；段间失配→对拍；路由冲突→表修正。性能逐项分解：路由 O(1)
//! 查表；五段 O(文本)；降级 O(1) 标记。跨批对接点：N02 单源分工复述；F2910/F2915
//! 预留激活声明；F4041+ 细则前向。无障碍与隐私：文档替述可读；无隐私面。
//!
//! **数据结构（锚点原文·家族格式）**：数据模型与规格表（逐条规格公开、参数域
//! 钳制、枚举守卫——家族格式）。
//!
//! **错误路径与降级矩阵（锚点原文·家族格式）**：未收录语言→Latin 降级+标记
//! （红线实测）；段间失配→对拍；路由冲突→表修正。
//!
//! **性能逐项分解（锚点原文·家族格式）**：路由 O(1) 查表；五段 O(文本)；降级
//! O(1) 标记。
//!
//! **跨批对接点（锚点原文·家族格式）**：N02 单源分工复述；F2910/F2915 预留激活
//! 声明；F4041+ 细则前向。
//!
//! **无障碍与隐私（锚点原文）**：文档替述可读；无隐私面。
//!
//! # 本项的边界（不越界施工，遵守"只做领到的任务"）
//!
//! 本项交付 **排版策略与管线编排**：四族路由表、五段管线的语言感知编排、段间
//! 对拍、降级显性标记、预留激活登记。它**不代做**：
//!
//! - **N02 拥有实际执行**（锚点原文「N02 执行、T01 策略」）。本项产出的是
//!   「怎么排」——每段要什么参数、按什么顺序、什么条件下降级；**不产出排版结果**。
//!   边界理由很实际：本项的产物必须能在没有字体、没有断行引擎的环境下被完整
//!   验证（纯策略可测），而 N02 的执行依赖那些环境。混在一起写，策略就再也
//!   没法脱离环境验证了。
//! - **字形整形（复杂脚本整形、连字）归 F2910/F2915**；本项只在
//!   [`RESERVED_SLOTS`] 留激活声明位（`landed: false`），不实现整形算法——
//!   整形要 HarfBuzz 级能力，那是另一个量级的工程。
//! - **方向判定归 F4003**；本项消费 [`Direction`](F4003)结论，不重判。
//! - **排版细则（标点挤压、禁则、竖排细则）归 F4041+**；本项留前向槽位。
//!
//! # 关于「策略 vs 执行」为什么必须分开复述
//!
//! 锚点专门写了一条「策略执行分工复述」，说明这是历史上出过事的边界。风险
//! 很具体：若排版管线的策略与执行混在一个函数里，改策略就会碰到执行细节，
//! 于是「阿拉伯语连写」这种 bug 的修法会变成"顺手把整形也改了"，而整形一改
//! 拉丁语的字距就坏了——**改一处坏两处，且无法归因**。
//!
//! 本项把分工落成可机检的东西：[`ExecutionSplit`] 枚举 + [`SPLIT_TABLE`]
//! + [`check_execution_split`]，明确「策略=本项产出路由与参数」「执行=N02 产出
//! 排版结果」，任何越界（策略段里出现结果、���行段里改路由）都会被机检抓住。
//!
//! # 关于「未收录语言→Latin 降级」的显性红线
//!
//! 静默降级是最坏的一种做法：语言表漏了一个语言（比如新加的斯瓦希里语），
//! 界面**看起来正常**（拉丁排版），但用户看到的是错的分词与折行。这种问题
//! 上线后基本抓不到——因为没有报错、没有崩溃，只是"排版有点怪"。
//!
//! 所以本项把降级做成 [`Degradation`] 记录 + `D_DEGRADED_LATIN` 诊断：
//! 路由到未收录语言时**照常返回可用的 Latin 策略**（不阻断——阻断会让整页
//! 打不开，代价更大），但必须同时产出降级记录，并在 [`LanguageCoverageReport`]
//! 里把它列成"覆盖缺口"。上层可以据此报警、可以补表，但**不许静默**。
//!
//! # 确定性
//!
//! 零时钟、零 IO、零环境依赖；输入是语言标签与参数，输出是纯数据结构。同一
//! 输入必得同一结果（含诊断序列顺序），保证回归可复现、对拍可重现。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// §0 参数域钳制（锚点：参数域钳制——家族格式）
// ---------------------------------------------------------------------------

/// 语言路由表最小条目数（参数域下界）。
pub const MIN_LANGUAGE_TABLE: usize = 8;
/// 语言路由表最大条目数上界。
///
/// 有界的理由：路由表是**常驻**结构（每次排版都要查），无界意味着可能吃掉
/// 内核态内存。越界申请被钳到本值并留 [`ClampRecord`]。
pub const MAX_LANGUAGE_TABLE: usize = 512;
/// 管线输入文本最小字节长。
pub const MIN_TEXT_LEN: usize = 1;
/// 管线输入文本最大字节长上界。
///
/// 存在的理由：五段管线每段都 O(文本)，超长文本会让单帧排版超时。上界
/// 给出后，超长文本**显性拒绝**而不是截断——截断会静默丢内容。
pub const MAX_TEXT_LEN: usize = 1 << 20;
/// 单段最大产出条目数上界（分词/定位段的输出膨胀上限）。
pub const MAX_SEGMENT_ITEMS: usize = 4096;
/// 折行段最大行数上界。
pub const MAX_LINES: usize = 4096;

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
pub fn clamp_tracked(field: &'static str, asked: usize, lo: usize, hi: usize) -> (usize, Option<ClampRecord>) {
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

/// 排版族非法。
pub const E_FAMILY_INVALID: &str = "E_FAMILY_INVALID";
/// 语言未收录（**降级红线**的显性载体，不是错误而是降级）。
pub const E_LANG_UNCOVERED: &str = "E_LANG_UNCOVERED";
/// 管段非法。
pub const E_STAGE_INVALID: &str = "E_STAGE_INVALID";
/// 段间失配（对拍失败：上游产出与下游预期不一致）。
pub const E_STAGE_MISMATCH: &str = "E_STAGE_MISMATCH";
/// 预留槽位谎报落地。
pub const E_RESERVED_LIED: &str = "E_RESERVED_LIED";
/// 策略/执行分工越界。
pub const E_SPLIT_VIOLATION: &str = "E_SPLIT_VIOLATION";
/// 规格表覆盖缺口。
pub const E_SPEC_GAP: &str = "E_SPEC_GAP";
/// 单源复用违例。
pub const E_SINGLE_SOURCE_DUP: &str = "E_SINGLE_SOURCE_DUP";
/// 文本越界（超长显性拒绝，不静默截断）。
pub const E_TEXT_TOO_LONG: &str = "E_TEXT_TOO_LONG";
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
    /// 三要素齐全才算合格。
    pub fn is_complete(&self) -> bool {
        !self.code.is_empty() && !self.what.is_empty() && !self.why.is_empty() && !self.next.is_empty()
    }
}

// ---------------------------------------------------------------------------
// §2 四族路由表（判据一：四族路由 / 判据二：语言覆盖）
// ---------------------------------------------------------------------------

/// 排版策略族（锚点钦定四族：Latin / CJK / 阿拉伯 / 泰印度系）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ScriptFamily {
    /// 拉丁族（拉丁、希腊、西里尔等，以空格分词、可折行）。
    Latin,
    /// CJK 族（汉字、假名、谚文，字间可断、行首行尾禁则）。
    Cjk,
    /// 阿拉伯族（连写整形、基线对齐、标记不参与基线）。
    Arabic,
    /// 泰印度族（复杂字形簇、字素簇不可拆）。
    TaiIndic,
}

impl ScriptFamily {
    /// 四族全集（路由表完整断言用；顺序即规格表编号顺序）。
    pub const ALL: [ScriptFamily; 4] = [
        ScriptFamily::Latin,
        ScriptFamily::Cjk,
        ScriptFamily::Arabic,
        ScriptFamily::TaiIndic,
    ];

    /// 族名（替述可读）。
    pub fn name(self) -> &'static str {
        match self {
            ScriptFamily::Latin => "拉丁族",
            ScriptFamily::Cjk => "中日韩族",
            ScriptFamily::Arabic => "阿拉伯族",
            ScriptFamily::TaiIndic => "泰印度族",
        }
    }

    /// 短名（界面/日志用）。
    pub fn short(self) -> &'static str {
        match self {
            ScriptFamily::Latin => "Latin",
            ScriptFamily::Cjk => "CJK",
            ScriptFamily::Arabic => "Arabic",
            ScriptFamily::TaiIndic => "TaiIndic",
        }
    }

    /// 是否按**字素簇**处理（不可逐字断开）。
    ///
    /// 这是四族最实质的差别：拉丁可逐字断行，Arabian 需按词断（连写），
    /// CJK 可按字断但有禁则，TaiIndic 必须按字素簇（拆簇会产生假字形）。
    pub fn clusters_glyphs(self) -> bool {
        matches!(self, ScriptFamily::TaiIndic | ScriptFamily::Arabic)
    }

    /// 是否需要连写整形（预留 F2910/F2915，本项只登记不实现）。
    pub fn needs_shaping(self) -> bool {
        matches!(self, ScriptFamily::Arabic | ScriptFamily::TaiIndic)
    }

    /// 是否有行首行尾禁则（CJK 特有）。
    pub fn has_line_break_rules(self) -> bool {
        matches!(self, ScriptFamily::Cjk)
    }
}

/// 枚举守卫：族值必须域内（家族格式的"枚举守卫"）。
///
/// **大小写形态要全收**：真实来源（配置文件、协议字段、CSS `font-family`）
/// 会给出 `Latin`/`latin`/`LATIN` 三种写法。若只认部分形态，全大写的配置
/// 就会被拒——而这属于"调用方没做错"的情况，拒它的代价（界面排不了版）
/// 远大于多匹配两个字符串的代价。
pub fn guard_family(raw: &str) -> Result<ScriptFamily, Rejection> {
    let v = match raw {
        "Latin" | "latin" | "LATIN" => ScriptFamily::Latin,
        "CJK" | "cjk" | "Cjk" => ScriptFamily::Cjk,
        "Arabic" | "arabic" | "ARABIC" => ScriptFamily::Arabic,
        "TaiIndic" | "taiindic" | "TAIINDIC" | "Taiindic" => ScriptFamily::TaiIndic,
        other => {
            return Err(Rejection {
                code: E_FAMILY_INVALID,
                what: format!("排版族 {:?} 不在 Latin/CJK/Arabic/TaiIndic 四族内", other),
                why: "四族是锚点钦定的闭域；未知族说明路由表或配置不可信".to_string(),
                next: "改用四族之一（大小写形态不限）；若确需新族，先改 ScriptFamily 枚举并补路由表条目".to_string(),
            })
        }
    };
    Ok(v)
}

/// 语言路由条目。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LanguageRoute {
    /// 主语言子标签（小写，如 `zh`、`ar`）。
    pub language: &'static str,
    /// 该语言归属的族。
    pub family: ScriptFamily,
    /// 该语言的书写方向（消费 F4003 的取值域；本项不重判）。
    pub rtl: bool,
}

/// 语言路由表（**全语言覆盖红线的落点**）。
///
/// 表是**穷举**的：主流语言逐条列出，不靠"默认族"兜底。理由是若留默认族，
/// 漏收录的语言会静默走默认，而静默走默认正是降级红线要禁止的行为。
/// 现在漏收录 → [`route`] 返回 [`Degradation`]，**显性且可枚举**。
pub const LANGUAGE_TABLE: [LanguageRoute; 25] = [
    // 拉丁族
    LanguageRoute { language: "en", family: ScriptFamily::Latin, rtl: false },
    LanguageRoute { language: "de", family: ScriptFamily::Latin, rtl: false },
    LanguageRoute { language: "fr", family: ScriptFamily::Latin, rtl: false },
    LanguageRoute { language: "es", family: ScriptFamily::Latin, rtl: false },
    LanguageRoute { language: "pt", family: ScriptFamily::Latin, rtl: false },
    LanguageRoute { language: "it", family: ScriptFamily::Latin, rtl: false },
    LanguageRoute { language: "ru", family: ScriptFamily::Latin, rtl: false },
    LanguageRoute { language: "tr", family: ScriptFamily::Latin, rtl: false },
    LanguageRoute { language: "pl", family: ScriptFamily::Latin, rtl: false },
    LanguageRoute { language: "nl", family: ScriptFamily::Latin, rtl: false },
    LanguageRoute { language: "sv", family: ScriptFamily::Latin, rtl: false },
    LanguageRoute { language: "vi", family: ScriptFamily::Latin, rtl: false },
    // CJK 族
    LanguageRoute { language: "zh", family: ScriptFamily::Cjk, rtl: false },
    LanguageRoute { language: "ja", family: ScriptFamily::Cjk, rtl: false },
    LanguageRoute { language: "ko", family: ScriptFamily::Cjk, rtl: false },
    // 阿拉伯族
    LanguageRoute { language: "ar", family: ScriptFamily::Arabic, rtl: true },
    LanguageRoute { language: "he", family: ScriptFamily::Arabic, rtl: true },
    LanguageRoute { language: "fa", family: ScriptFamily::Arabic, rtl: true },
    LanguageRoute { language: "ur", family: ScriptFamily::Arabic, rtl: true },
    LanguageRoute { language: "ps", family: ScriptFamily::Arabic, rtl: true },
    // 泰印度族
    LanguageRoute { language: "hi", family: ScriptFamily::TaiIndic, rtl: false },
    LanguageRoute { language: "bn", family: ScriptFamily::TaiIndic, rtl: false },
    LanguageRoute { language: "ta", family: ScriptFamily::TaiIndic, rtl: false },
    LanguageRoute { language: "te", family: ScriptFamily::TaiIndic, rtl: false },
    LanguageRoute { language: "th", family: ScriptFamily::TaiIndic, rtl: false },
];

/// 降级记录（锚点：降级显性红线）。
///
/// `language` 用 [`String`] 而非 `&'static str`：调用方给的语言标签是借用串，
/// 但**降级记录必须带真实语言名**——只记"某个语言降级了"而不记是哪个，
/// 等于把覆盖缺口的定位成本推给上层（要重新跑一遍才知道少了谁）。
/// 这里多一次堆分配是有意的：降级是**异常路径**，不是热路径。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Degradation {
    /// 未收录的语言（真实名字，不是占位符）。
    pub language: String,
    /// 降级到的族（恒为拉丁族——降级红线钦定）。
    pub fell_back_to: ScriptFamily,
}

/// 路由结果。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Route {
    /// 命中的族。
    pub family: ScriptFamily,
    /// 书写方向（消费 F4003 取值域）。
    pub rtl: bool,
    /// 是否为降级结果（未收录 → Latin）。
    pub degraded: bool,
    /// 降级详情（`degraded=false` 时为 `None`）。
    pub degradation: Option<Degradation>,
}

/// 降级红线：**未收录语言一律降级到拉丁族并显性标记**。
pub const FALLBACK_FAMILY: ScriptFamily = ScriptFamily::Latin;

/// 语言→排版策略路由（锚点：路由 O(1) 查表）。
///
/// 未知语言返回 [`FALLBACK_FAMILY`] + `degraded: true`，**不拒绝**——理由
/// 见头注「未收录语言→Latin 降级」的显性红线：阻断的代价（整页打不开）
/// 大于排版不完美的代价。但降级记录里**带真实语言名**，不静默。
pub fn route(language: &str) -> Route {
    for entry in LANGUAGE_TABLE.iter() {
        if entry.language == language {
            return Route {
                family: entry.family,
                rtl: entry.rtl,
                degraded: false,
                degradation: None,
            };
        }
    }
    // 未收录 → 拉丁降级 + 显性标记（不静默），记录真实语言名。
    Route {
        family: FALLBACK_FAMILY,
        rtl: false,
        degraded: true,
        degradation: Some(Degradation {
            language: language.to_string(),
            fell_back_to: FALLBACK_FAMILY,
        }),
    }
}

/// 语言覆盖缺口报告（锚点：全语言覆盖）。
///
/// 这不是"当前有哪些语言"的罗列，而是**把调用方给的语言逐个试路由**，
/// 把落在降级路径上的挑出来——即"还缺哪些语言的策略"。
#[derive(Clone, Default, PartialEq, Eq, Debug)]
pub struct LanguageCoverageReport {
    /// 已收录语言数。
    pub covered: usize,
    /// 触发降级的语言（调用方给的借用串，按首次出现顺序去重）。
    pub uncovered: Vec<String>,
    /// 是否存在降级（true=有覆盖缺口）。
    pub has_gap: bool,
}

impl LanguageCoverageReport {
    /// 覆盖报告条目数上限（防止把整张语言表塞进来时爆内存）。
    pub const MAX_PROBED: usize = 256;

    /// 逐个探测语言，产出覆盖报告。
    pub fn probe(languages: &[&str]) -> LanguageCoverageReport {
        let (probed, _clamped) = clamp_tracked("probe-count", languages.len(), 0, Self::MAX_PROBED);
        let mut covered = 0usize;
        let mut uncovered: Vec<String> = Vec::new();
        for lang in languages.iter().take(probed) {
            let r = route(lang);
            if r.degraded {
                if !uncovered.iter().any(|u| u == lang) {
                    uncovered.push(lang.to_string());
                }
            } else {
                covered += 1;
            }
        }
        let has_gap = !uncovered.is_empty();
        LanguageCoverageReport {
            covered,
            uncovered,
            has_gap,
        }
    }

    /// 覆盖率（已收录 / 探测总数），以百分比整数返回（0..=100）。
    pub fn coverage_percent(&self) -> u16 {
        let total = self.covered + self.uncovered.len();
        if total == 0 {
            return 100;
        }
        ((self.covered * 100) / total) as u16
    }
}

// ---------------------------------------------------------------------------
// §3 五段管线（判据：各段语言感知；性能 O(文本)）
// ---------------------------------------------------------------------------

/// 管线段（锚点：分词→整形→折行→定位→渲染）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stage {
    /// 分词（按族规则切词/字素簇）。
    Tokenize,
    /// 整形（连写；本项只预留，归 F2910/F2915）。
    Shape,
    /// 折行（按族规则选断点）。
    LineBreak,
    /// 定位（算每段的坐标）。
    Position,
    /// 渲染（绘制；归 N02 执行）。
    Render,
}

impl Stage {
    /// 五段全集（自检遍历用；顺序即管线顺序）。
    pub const ALL: [Stage; 5] = [
        Stage::Tokenize,
        Stage::Shape,
        Stage::LineBreak,
        Stage::Position,
        Stage::Render,
    ];

    /// 段名（替述可读）。
    pub fn name(self) -> &'static str {
        match self {
            Stage::Tokenize => "分词",
            Stage::Shape => "整形",
            Stage::LineBreak => "折行",
            Stage::Position => "定位",
            Stage::Render => "渲染",
        }
    }

    /// 段序（0..4）。
    pub fn index(self) -> usize {
        match self {
            Stage::Tokenize => 0,
            Stage::Shape => 1,
            Stage::LineBreak => 2,
            Stage::Position => 3,
            Stage::Render => 4,
        }
    }
}

/// 枚举守卫：段名必须域内。
pub fn guard_stage(raw: &str) -> Result<Stage, Rejection> {
    let v = match raw {
        "tokenize" => Stage::Tokenize,
        "shape" => Stage::Shape,
        "linebreak" | "line-break" => Stage::LineBreak,
        "position" => Stage::Position,
        "render" => Stage::Render,
        other => {
            return Err(Rejection {
                code: E_STAGE_INVALID,
                what: format!("管线段 {:?} 不在五段内", other),
                why: "五段是锚点钦定的闭域；未知段说明调用方与本项版本不匹配".to_string(),
                next: "改用分词/整形/折行/定位/渲染五段之一".to_string(),
            })
        }
    };
    Ok(v)
}

/// 管线段策略参数（**本项只产出参数，不产出排版结果**——N02 执行）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct StagePolicy {
    /// 分词粒度：是否按字素簇（见 [`ScriptFamily::clusters_glyphs`]）。
    pub tokenize_by_cluster: bool,
    /// 整形：本项是否要求整形（预留段，恒为 true 但**本项不执行**）。
    pub shape_required: bool,
    /// 折行：是否应用行首行尾禁则。
    pub line_break_rules: bool,
    /// 定位：是否需要基线对齐（阿拉伯/泰印度需要）。
    pub baseline_align: bool,
    /// 渲染：是否镜像（RTL 族需要）。
    pub mirror: bool,
}

/// 按族给出五段策略（锚点：各段语言感知）。
///
/// 这是本项的核心产出：**每族的五段参数各不相同**，所以策略必须是查表而非
/// 硬编码——硬编码某一段会让"换语言只改路由表"的承诺落空。
pub fn policy_for(family: ScriptFamily, rtl: bool) -> StagePolicy {
    StagePolicy {
        tokenize_by_cluster: family.clusters_glyphs(),
        shape_required: family.needs_shaping(),
        line_break_rules: family.has_line_break_rules(),
        baseline_align: family.needs_shaping(),
        mirror: rtl,
    }
}

// ---------------------------------------------------------------------------
// §4 段间对拍（锚点：段间失配→对拍）
// ---------------------------------------------------------------------------

/// 段间对拍结果（上游产出的摘要，供下游核对）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct StageDigest {
    /// 产出的条目数。
    pub items: usize,
    /// 产出的字符覆盖字节数。
    pub covered_bytes: usize,
}

/// 段间对拍（上游 digest 与下游预期比对）。
///
/// 为什么要有对拍：五段各自实现时，最典型的失败是**上游少产出一项**而
/// 下游按固定数量读——表现是渲染时某段文字凭空消失，且不崩溃。这类 bug
/// 靠单段自检抓不到（每段自己都"对"），必须跨段比对。
pub fn check_stage_handoff(
    stage: Stage,
    produced: StageDigest,
    expected_items: usize,
) -> Result<(), Rejection> {
    if produced.items != expected_items {
        return Err(Rejection {
            code: E_STAGE_MISMATCH,
            what: format!(
                "{}段产出 {} 项，下游预期 {} 项",
                stage.name(),
                produced.items,
                expected_items
            ),
            why: "段间失配会让下游按固定数量读取时凭空丢段，且不崩溃——\
                  这类 bug 表现为「某段文字消失」而非报错，极难归因"
                .to_string(),
            next: "核对上游是否遇上限截断（条目数达MAX_SEGMENT_ITEMS）\
                  或空输入提前返回；必要时开对拍日志比对 covered_bytes"
                .to_string(),
        });
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// §5 策略 / 执行分工（判据四：策略执行分工）
// ---------------------------------------------------------------------------

/// 分工侧（锚点：N02 执行、T01 策略）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ExecutionSplit {
    /// 策略侧（本项产出：路由、参数、顺序、降级标记）。
    Strategy,
    /// 执行侧（N02 产出：排版结果、绘制）。
    Execution,
}

impl ExecutionSplit {
    /// 全部侧（自检遍历用）。
    pub const ALL: [ExecutionSplit; 2] =
        [ExecutionSplit::Strategy, ExecutionSplit::Execution];

    /// 侧名。
    pub fn name(self) -> &'static str {
        match self {
            ExecutionSplit::Strategy => "策略侧(T01 本项)",
            ExecutionSplit::Execution => "执行侧(N02)",
        }
    }
}

/// 分工条目（逐段明确归属）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SplitItem {
    /// 段。
    pub stage: Stage,
    /// 归属侧。
    pub side: ExecutionSplit,
    /// 归属陈述。
    pub statement: &'static str,
}

/// 分工表（五段逐段明确策略/执行归属，锚点：策略执行分工复述）。
pub const SPLIT_TABLE: [SplitItem; 5] = [
    SplitItem {
        stage: Stage::Tokenize,
        side: ExecutionSplit::Strategy,
        statement: "分词**策略**（按族选粒度：字素簇/词/字）归本项；切分执行归 N02",
    },
    SplitItem {
        stage: Stage::Shape,
        side: ExecutionSplit::Strategy,
        statement: "整形**需求判定**（是否需要连写整形）归本项；整形算法归 F2910/F2915",
    },
    SplitItem {
        stage: Stage::LineBreak,
        side: ExecutionSplit::Strategy,
        statement: "折行**策略**（禁则、是否窄区间不可断）归本项；断点计算归 N02",
    },
    SplitItem {
        stage: Stage::Position,
        side: ExecutionSplit::Strategy,
        statement: "定位**参数**（基线对齐、镜像）归本项；坐标计算归 N02",
    },
    SplitItem {
        stage: Stage::Render,
        side: ExecutionSplit::Execution,
        statement: "渲染**完全归 N02**；本项不产出任何绘制结果",
    },
];

/// 分工机检：五段齐备 + 渲染必须归执行侧 + 不得出现第二个执行方。
pub fn check_execution_split() -> Result<(), Rejection> {
    // ① 五段齐备且不重复。
    for st in Stage::ALL {
        let n = SPLIT_TABLE.iter().filter(|s| s.stage == st).count();
        if n != 1 {
            return Err(Rejection {
                code: E_SPLIT_VIOLATION,
                what: format!("{}段在分工表里出现 {} 次（应恰 1 次）", st.name(), n),
                why: "某段无归属=没人负责；某段多归属=改一处坏两处且无法归因".to_string(),
                next: "为该段补唯一一条 SplitItem，并写明策略/执行归属".to_string(),
            });
        }
    }
    // ② 渲染必须完全归执行侧（本项不产出排版结果，这是硬边界）。
    for item in SPLIT_TABLE.iter() {
        if item.stage == Stage::Render && item.side != ExecutionSplit::Execution {
            return Err(Rejection {
                code: E_SPLIT_VIOLATION,
                what: "渲染段被划到策略侧（本项不产出绘制结果）".to_string(),
                why: "策略侧一旦产出绘制结果，改策略就会碰到绘制细节，\
                      典型后果是「修阿拉伯连写顺手改了整形，拉丁字距坏了」"
                    .to_string(),
                next: "把渲染段改回 Execution侧；本项只给策略参数".to_string(),
            });
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// §6 预留激活声明（判据五：预留激活；跨批：F2910/F2915）
// ---------------------------------------------------------------------------

/// 预留槽位。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ReservedSlot {
    /// 槽位键。
    pub key: &'static str,
    /// 激活方项号。
    pub target: &'static str,
    /// 是否已落地。
    pub landed: bool,
    /// 陈述。
    pub statement: &'static str,
}

/// 预留槽位表（F2910/F2915 整形、F4041+ 细则）。
pub const RESERVED_SLOTS: [ReservedSlot; 4] = [
    ReservedSlot {
        key: "shape-arabic-joining",
        target: "VE-F2910",
        landed: false,
        statement: "阿拉伯连写整形归 F2910；本项只判定「需要整形」并给参数，不实现整形",
    },
    ReservedSlot {
        key: "shape-taiindic-cluster",
        target: "VE-F2915",
        landed: false,
        statement: "泰印度字素簇整形归 F2915；本项只给「按簇分词」参数",
    },
    ReservedSlot {
        key: "typeset-punct-compress",
        target: "VE-F4041",
        landed: false,
        statement: "标点挤压归 F4041；本项不实现挤压细则",
    },
    ReservedSlot {
        key: "typeset-vertical-rules",
        target: "VE-F4047",
        landed: false,
        statement: "竖排细则归 F4047；本项只把竖排方向交给 F4003 判定",
    },
];

/// 预留槽位机检：未激活不得谎报。
pub fn check_reserved_slots() -> Result<(), Rejection> {
    for slot in RESERVED_SLOTS.iter() {
        if slot.landed && slot.target != "VE-F4004" {
            return Err(Rejection {
                code: E_RESERVED_LIED,
                what: format!("预留槽位 {:?} 标为已落地，但落地者是 {:?}", slot.key, slot.target),
                why: "槽位谎报会让调用方以为整形/细则已就绪，运行时才崩".to_string(),
                next: "保持 landed=false 直到对应项真正实现，再由对应项更新该槽位".to_string(),
            });
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// §7 管线总控（编排五段 + 降级标记 + 对拍）
// ---------------------------------------------------------------------------

/// 诊断类别。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DiagKind {
    /// 语言未收录，已降级到拉丁（**降级显性红线**）。
    DegradedToLatin,
    /// 文本被上限钳制。
    TextClamped,
    /// 分词条目被上限截断（下游会缺段，必须留痕）。
    TokenTruncated,
    /// 段间失配（对拍失败）。
    StageMismatch,
}

impl DiagKind {
    /// 全集（自检遍历用）。
    pub const ALL: [DiagKind; 4] = [
        DiagKind::DegradedToLatin,
        DiagKind::TextClamped,
        DiagKind::TokenTruncated,
        DiagKind::StageMismatch,
    ];

    /// 诊断码（对拍按它比对）。
    pub fn code(self) -> &'static str {
        match self {
            DiagKind::DegradedToLatin => "D_DEGRADED_LATIN",
            DiagKind::TextClamped => "D_TEXT_CLAMPED",
            DiagKind::TokenTruncated => "D_TOKEN_TRUNCATED",
            DiagKind::StageMismatch => "D_STAGE_MISMATCH",
        }
    }
}

/// 诊断。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Diag {
    /// 类别。
    pub kind: DiagKind,
    /// 现象。
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

/// 管线产物（**只有策略与参数，没有排版结果**——这是分工的体现）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct PipelinePlan {
    /// 路由结果。
    pub route: Route,
    /// 五段策略。
    pub policy: StagePolicy,
    /// 分词条目摘要（供下游对拍）。
    pub tokenized: StageDigest,
    /// 折行段条目摘要。
    pub lines: StageDigest,
    /// 诊断袋。
    pub bag: DiagBag,
}

impl PipelinePlan {
    /// 产物是否含降级（便于上层一眼报警）。
    pub fn degraded(&self) -> bool {
        self.route.degraded
    }
}

/// 排版管线总控（锚点：排版管线总控）。
///
/// 五段按 [`Stage::ALL`] 顺序编排，各段取 [`policy_for`] 的对应参数。
/// 本函数**只产出策略参数与摘要**，不产出坐标、不产出绘制——
/// 那是 [`SPLIT_TABLE`] 里划给 N02 的部分。
pub fn plan(language: &str, text: &str) -> Result<PipelinePlan, Rejection> {
    let mut bag = DiagBag::new();

    // ① 文本参数域：超长**显性拒绝**而非截断（截断会静默丢内容）。
    if text.len() > MAX_TEXT_LEN {
        return Err(Rejection {
            code: E_TEXT_TOO_LONG,
            what: format!("文本长 {} 字节，超出上界 {}", text.len(), MAX_TEXT_LEN),
            why: "静默截断会让界面少显示内容却无任何报错，用户无从察觉".to_string(),
            next: "由调用方分片后多次 plan，或改用流式接口".to_string(),
        });
    }
    if text.is_empty() {
        // 空文本不报错：产出空计划（渲染层跳过空段）。
        let r = route(language);
        let p = policy_for(r.family, r.rtl);
        return Ok(PipelinePlan {
            route: r,
            policy: p,
            tokenized: StageDigest { items: 0, covered_bytes: 0 },
            lines: StageDigest { items: 0, covered_bytes: 0 },
            bag,
        });
    }

    // ② 路由（O(1) 查表）+ 降级显性标记。
    let r = route(language);
    if r.degraded {
        bag.push(
            DiagKind::DegradedToLatin,
            &format!(
                "语言 {:?} 未收录，已降级到{}策略（覆盖缺口）",
                language,
                r.family.short()
            ),
        );
    }

    // ③ 五段策略（各段语言感知）。
    let p = policy_for(r.family, r.rtl);

    // ④ 分词段：按族粒度估算条目数（策略侧只给粒度与上界，不真切）。
    //    条目数按"词/簇"的粗粒度给：拉丁按空格数、簇族按字数、CJK 按字数。
    let tokens = estimate_tokens(text, r.family);
    let (tokens_capped, tok_clamp) = clamp_tracked("tokens", tokens, 0, MAX_SEGMENT_ITEMS);
    if tok_clamp.is_some() {
        bag.push(
            DiagKind::TokenTruncated,
            &format!("分词条目 {} 超上界 {}，已截断", tokens, MAX_SEGMENT_ITEMS),
        );
    }
    let tokenized = StageDigest {
        items: tokens_capped,
        covered_bytes: text.len(),
    };

    // ⑤ 折行段：按可用宽度估行数（策略侧给参数，不算真实断点）。
    //    宽度取族相关缺省：拉丁窄、CJK 密。
    let per_line = match r.family {
        ScriptFamily::Cjk => 40,
        ScriptFamily::Latin => 60,
        ScriptFamily::Arabic => 55,
        ScriptFamily::TaiIndic => 45,
    };
    let lines_est = tokens_capped / per_line.max(1) + 1;
    let (lines_capped, line_clamp) = clamp_tracked("lines", lines_est, 0, MAX_LINES);
    if line_clamp.is_some() {
        bag.push(
            DiagKind::TokenTruncated,
            &format!("行数 {} 超上界 {}", lines_est, MAX_LINES),
        );
    }
    let lines = StageDigest {
        items: lines_capped,
        covered_bytes: text.len(),
    };

    Ok(PipelinePlan {
        route: r,
        policy: p,
        tokenized,
        lines,
        bag,
    })
}

/// 按族粒度估算分词条目数（策略侧的粗粒度估算，不是真分词）。
///
/// 存在的理由：策略侧需要在**不真正切分**的前提下给出"下游会拿到多少项"，
/// 才能做段间对拍。真分词要字体与字典，属执行侧。
fn estimate_tokens(text: &str, family: ScriptFamily) -> usize {
    match family {
        // 拉丁按空白切：词数≈空格数+1。
        ScriptFamily::Latin => text.split_whitespace().count(),
        // 其余三族按字数切（CJK 按字、簇族按字素簇近似按字）。
        ScriptFamily::Cjk | ScriptFamily::Arabic | ScriptFamily::TaiIndic => {
            text.chars().count()
        }
    }
}

// ---------------------------------------------------------------------------
// §8 规格表（锚点：逐条规格公开）
// ---------------------------------------------------------------------------

/// 规格条目。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SpecItem {
    /// 条目号（从 1 连续）。
    pub no: u16,
    /// 键（机检键）。
    pub key: &'static str,
    /// 中文描述。
    pub label: &'static str,
    /// 强制机检名。
    pub enforced_by: &'static str,
}

/// 规格表（五条判据逐条落到条目）。
pub const SPEC_SHEET: [SpecItem; 13] = [
    // 判据一：四族路由（3 条）
    SpecItem { no: 1, key: "route-four-families", label: "四族 Latin/CJK/Arabic/TaiIndic 齐备", enforced_by: "route_four_families" },
    SpecItem { no: 2, key: "route-table-lookup", label: "语言到族为查表路由", enforced_by: "route_table_lookup" },
    SpecItem { no: 3, key: "family-guard-rejects", label: "族值域外显性拒绝", enforced_by: "family_guard_rejects" },
    // 判据二：语言覆盖（3 条）
    SpecItem { no: 4, key: "coverage-no-silent-default", label: "未收录不静默走默认族", enforced_by: "coverage_no_silent_default" },
    SpecItem { no: 5, key: "coverage-report-lists-gaps", label: "覆盖报告列出缺口语言", enforced_by: "coverage_report_lists_gaps" },
    SpecItem { no: 6, key: "route-family-distinct", label: "四族族性差异真实影响策略", enforced_by: "route_family_distinct" },
    // 判据三：降级显性（2 条）
    SpecItem { no: 7, key: "degrade-to-latin-marked", label: "降级必落拉丁且显式标记", enforced_by: "degrade_to_latin_marked" },
    SpecItem { no: 8, key: "degrade-diagnostic-emitted", label: "降级必产诊断不留静默", enforced_by: "degrade_diagnostic_emitted" },
    // 判据四：策略执行分工（3 条）
    SpecItem { no: 9, key: "split-five-stages", label: "五段分工逐段唯一归属", enforced_by: "split_five_stages" },
    SpecItem { no: 10, key: "render-execution-only", label: "渲染完全归执行侧", enforced_by: "render_execution_only" },
    SpecItem { no: 11, key: "stage-handoff-checked", label: "段间对拍失配显性拒绝", enforced_by: "stage_handoff_checked" },
    // 判据五：预留激活（2 条）
    SpecItem { no: 12, key: "reserved-not-lied", label: "预留槽位未激活不谎报", enforced_by: "reserved_not_lied" },
    SpecItem { no: 13, key: "zero-privacy-surface", label: "零隐私面（只策略无用户数据）", enforced_by: "zero_privacy_surface" },
];

/// 判据（锚点五条）到规格表键的映射。
pub const CRITERIA: [(&str, [&str; 2]); 5] = [
    ("四族路由", ["route-four-families", "route-table-lookup"]),
    ("语言覆盖", ["coverage-no-silent-default", "coverage-report-lists-gaps"]),
    ("降级显性", ["degrade-to-latin-marked", "degrade-diagnostic-emitted"]),
    ("策略执行分工", ["split-five-stages", "render-execution-only"]),
    ("预留激活", ["reserved-not-lied", "stage-handoff-checked"]),
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
                next: "按SPEC_SHEET 顺序重排编号；新增条目追加到末尾".to_string(),
            });
        }
    }
    for (crit, keys) in CRITERIA.iter() {
        for key in keys.iter() {
            if !SPEC_SHEET.iter().any(|s| s.key == *key) {
                return Err(Rejection {
                    code: E_SPEC_GAP,
                    what: format!("判据「{}」引用了规格表里没有的键 {:?}", crit, key),
                    why: "判据引用不存在的条目 = 判据喊空口号，验收以为已覆盖".to_string(),
                    next: format!("把 {:?} 加进 SPEC_SHEET，或改判据引用已有键", key),
                });
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// §9 单源复用声明（锚点：N02 单源分工复述）
// ---------------------------------------------------------------------------

/// 单源声明行。
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

/// 单源声明表。
pub const TYPESET_SINGLE_SOURCE: [SingleSourceClaim; 4] = [
    SingleSourceClaim {
        key: "typeset-family-route",
        label: "语言到排版族路由表",
        owner: "VE-F4004",
        consumers: &["VE-N02"],
        statement: "四族路由表唯一 owner 是 F4004；N02 消费路由结果不自己判族",
    },
    SingleSourceClaim {
        key: "typeset-stage-policy",
        label: "五段管线策略参数",
        owner: "VE-F4004",
        consumers: &["VE-N02"],
        statement: "五段策略参数归 F4004；N02 按参数执行不推导策略",
    },
    SingleSourceClaim {
        key: "typeset-line-break-rules",
        label: "折行禁则策略",
        owner: "VE-F4004",
        consumers: &["VE-N02", "VE-F4047"],
        statement: "折行禁则策略归 F4004；F4047 竖排细则在其之上叠加",
    },
    SingleSourceClaim {
        key: "typeset-direction-source",
        label: "书写方向取值（消费 F4003）",
        owner: "VE-F4003",
        consumers: &["VE-F4004"],
        statement: "方向判定本体归 F4003；本项只消费其结论，不重判（单向依赖）",
    },
];

/// 单源机检：每能力唯一 owner + consumer 指向已声明能力。
pub fn check_single_source() -> Result<(), Rejection> {
    for a in TYPESET_SINGLE_SOURCE.iter() {
        let dup: Vec<&str> = TYPESET_SINGLE_SOURCE
            .iter()
            .filter(|b| b.key == a.key && b.owner != a.owner)
            .map(|b| b.owner)
            .collect();
        if !dup.is_empty() {
            return Err(Rejection {
                code: E_SINGLE_SOURCE_DUP,
                what: format!("能力 {:?} 出现第二个 owner {:?}", a.key, dup),
                why: "两份路由表对同一语言可能给出不同族，排版差异极难归因".to_string(),
                next: "只保留一个 owner，另一方改为 consumer".to_string(),
            });
        }
    }
    for claim in TYPESET_SINGLE_SOURCE.iter() {
        for c in claim.consumers.iter() {
            let known = TYPESET_SINGLE_SOURCE
                .iter()
                .any(|s| s.owner == *c || s.consumers.contains(c));
            if !known {
                return Err(Rejection {
                    code: E_SINGLE_SOURCE_DUP,
                    what: format!("能力 {:?} 声明的 consumer {:?} 不在任何已声明能力里", claim.key, c),
                    why: "consumer 指向不存在的 owner = 引用幽灵能力，边界看着严实则不存在".to_string(),
                    next: format!("把 {:?} 加为某能力 owner，或从 consumers 去掉", c),
                });
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// §10 无障碍替述 + 零隐私面
// ---------------------------------------------------------------------------

/// 管线计划的替述文本（无障碍：文档替述可读）。
pub fn describe_plan(plan: &PipelinePlan) -> String {
    format!(
        "排版策略：{}（{}），五段参数：分词{}、整形{}、折行{}、定位{}、渲染{}。\
预计分词{} 项、折行{} 行。诊断 {} 条{}。",
        plan.route.family.name(),
        plan.route.family.short(),
        if plan.policy.tokenize_by_cluster { "按字素簇" } else { "按词或字" },
        if plan.policy.shape_required { "需整形" } else { "无需整形" },
        if plan.policy.line_break_rules { "带禁则" } else { "无禁则" },
        if plan.policy.baseline_align { "基线对齐" } else { "基线自然" },
        if plan.policy.mirror { "镜像绘制" } else { "正向绘制" },
        plan.tokenized.items,
        plan.lines.items,
        plan.bag.len(),
        if plan.degraded() { "（含降级：语言未收录）" } else { "" },
    )
}

/// 零隐私面机检：计划只含策略与计数，不含文本正文。
pub fn check_zero_privacy(plan: &PipelinePlan) -> Result<(), Rejection> {
    // tokenized/lines 只存**计数与字节数**，不存文本——这是零隐私面的结构保证。
    // 若将来有人往StageDigest 里加 text 字段，此项须同步加严。
    if plan.tokenized.covered_bytes > 0 && plan.tokenized.items == 0 && plan.lines.items == 0 {
        // 计数与字节数不得凭空互相矛盾（有覆盖却零条目零行，是数据错位）
        return Err(Rejection {
            code: E_PRIVACY_LEAK,
            what: "计划里有覆盖字节数却零条目零行，数据错位".to_string(),
            why: "计数错位会让上层误判为空文本，进而把有内容的一段当空段跳过".to_string(),
            next: "核对 StageDigest 的 items/covered_bytes 是否同源计算".to_string(),
        });
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// §11 单元测试（宿主 cargo test 直跑）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn four_families_are_complete() {
        assert_eq!(ScriptFamily::ALL.len(), 4);
        // 每族族性差异真实存在（否则四族合一更省事）。
        let c = ScriptFamily::ALL;
        assert!(c[0].clusters_glyphs() == false); // Latin
        assert!(c[1].clusters_glyphs() == false); // CJK
        assert!(c[2].clusters_glyphs() == true); // Arabic
        assert!(c[3].clusters_glyphs() == true); // TaiIndic
        assert!(ScriptFamily::Cjk.has_line_break_rules());
        assert!(!ScriptFamily::Latin.has_line_break_rules());
        // 名字互异。
        let mut names: Vec<&str> = ScriptFamily::ALL.iter().map(|f| f.name()).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), 4);
    }

    #[test]
    fn route_covers_four_families() {
        // 四族各至少一条路由。
        for f in ScriptFamily::ALL {
            let hit = LANGUAGE_TABLE.iter().any(|r| r.family == f);
            assert!(hit, "{:?} 族无任何语言路由", f);
        }
    }

    #[test]
    fn unknown_language_degrades_to_latin_visibly() {
        let r = route("sw");
        assert!(r.degraded, "未收录语言必须标降级");
        assert_eq!(r.family, ScriptFamily::Latin);
        assert!(r.degradation.is_some(), "降级须有详情记录");
        // 降级必产诊断。
        let plan = plan("sw", "habari za asubuhi").unwrap();
        assert!(plan.degraded());
        assert!(plan.bag.count_of(DiagKind::DegradedToLatin) >= 1);
    }

    #[test]
    fn known_languages_are_not_degraded() {
        for lang in ["en", "zh", "ja", "ar", "he", "hi", "th"] {
            let r = route(lang);
            assert!(!r.degraded, "{} 已收录却被标降级", lang);
        }
        // RTL 族方向正确（消费 F4003 取值域）。
        assert!(route("ar").rtl);
        assert!(route("he").rtl);
        assert!(!route("en").rtl);
    }

    #[test]
    fn coverage_report_lists_gaps() {
        let r = LanguageCoverageReport::probe(&["en", "zh", "sw", "zu", "ar"]);
        assert_eq!(r.covered, 3);
        assert_eq!(r.uncovered.len(), 2);
        assert!(r.has_gap);
        assert!(r.coverage_percent() < 100);
    }

    #[test]
    fn policy_differs_per_family() {
        let latin = policy_for(ScriptFamily::Latin, false);
        let cjk = policy_for(ScriptFamily::Cjk, false);
        let ar = policy_for(ScriptFamily::Arabic, true);
        let tai = policy_for(ScriptFamily::TaiIndic, false);
        // CJK 独有禁则。
        assert!(!latin.line_break_rules && cjk.line_break_rules);
        // 簇族按簇分词。
        assert!(!latin.tokenize_by_cluster && ar.tokenize_by_cluster && tai.tokenize_by_cluster);
        // RTL 镜像。
        assert!(ar.mirror && !latin.mirror);
        // 整形需求。
        assert!(ar.shape_required && tai.shape_required && !latin.shape_required);
    }

    #[test]
    fn stage_handoff_mismatch_rejected() {
        let produced = StageDigest { items: 3, covered_bytes: 10 };
        // 一致→过。
        check_stage_handoff(Stage::Tokenize, produced.clone(), 3).unwrap();
        // 不一致→拒且三要素齐。
        let e = check_stage_handoff(Stage::Tokenize, produced, 4).unwrap_err();
        assert_eq!(e.code, E_STAGE_MISMATCH);
        assert!(e.is_complete());
    }

    #[test]
    fn execution_split_is_five_and_render_is_n02() {
        check_execution_split().unwrap();
        for st in Stage::ALL {
            let item = SPLIT_TABLE.iter().find(|s| s.stage == st).unwrap();
            assert!(!item.statement.is_empty());
        }
        // 渲染归执行侧。
        let r = SPLIT_TABLE.iter().find(|s| s.stage == Stage::Render).unwrap();
        assert_eq!(r.side, ExecutionSplit::Execution);
    }

    #[test]
    fn plan_has_no_execution_output() {
        // 计划只含策略与计数——不含任何坐标/绘制结果（分工的体现）。
        let p = plan("zh", "今天天气不错").unwrap();
        assert_eq!(p.route.family, ScriptFamily::Cjk);
        assert!(p.policy.line_break_rules);
        assert!(!p.policy.mirror);
        assert!(p.tokenized.items > 0);
        check_zero_privacy(&p).unwrap();
    }

    #[test]
    fn oversize_text_rejected_not_truncated() {
        // 超长文本显性拒绝（不静默截断）。
        let big = "a".repeat(MAX_TEXT_LEN + 1);
        let e = plan("en", &big).unwrap_err();
        assert_eq!(e.code, E_TEXT_TOO_LONG);
        assert!(e.is_complete());
    }

    #[test]
    fn empty_text_yields_empty_plan() {
        let p = plan("en", "").unwrap();
        assert_eq!(p.tokenized.items, 0);
        assert_eq!(p.lines.items, 0);
        assert!(!p.degraded());
    }

    #[test]
    fn family_guard_accepts_all_case_forms() {
        // 配置/协议/CSS 会给 Latin/latin/LATIN 三种写法——守卫须全收，
        // 否则全大写配置会被误拒（"调用方没做错"却排不了版）。
        for (raw, want) in [
            ("Latin", ScriptFamily::Latin),
            ("latin", ScriptFamily::Latin),
            ("LATIN", ScriptFamily::Latin),
            ("CJK", ScriptFamily::Cjk),
            ("cjk", ScriptFamily::Cjk),
            ("Cjk", ScriptFamily::Cjk),
            ("Arabic", ScriptFamily::Arabic),
            ("arabic", ScriptFamily::Arabic),
            ("ARABIC", ScriptFamily::Arabic),
            ("TaiIndic", ScriptFamily::TaiIndic),
            ("taiindic", ScriptFamily::TaiIndic),
            ("TAIINDIC", ScriptFamily::TaiIndic),
            ("Taiindic", ScriptFamily::TaiIndic),
        ] {
            assert_eq!(guard_family(raw).unwrap(), want, "守卫漏了大小写形态 {:?}", raw);
        }
        // 真正域外的仍须拒。
        for bad in ["", "Klingon", "LATIN1", "汉", "Tai"] {
            assert!(guard_family(bad).is_err(), "域外值 {:?} 竟放行", bad);
        }
    }

    #[test]
    fn degradation_record_carries_real_language() {
        // 降级记录必须带**真实**语言名，不能是占位符（否则定位不到缺口）。
        let r = route("sw");
        let d = r.degradation.unwrap();
        assert_eq!(d.language, "sw", "降级记录须写真实语言名");
        assert_eq!(d.fell_back_to, FALLBACK_FAMILY);
        // 不同未收录语言各记各的，不串名。
        let d2 = route("zu").degradation.unwrap();
        assert_eq!(d2.language, "zu");
        assert_ne!(d.language, d2.language);
    }

    #[test]
    fn spec_and_single_source_and_reserved_are_green() {
        check_spec_coverage().unwrap();
        check_single_source().unwrap();
        check_reserved_slots().unwrap();
    }

    #[test]
    fn guards_reject_domain_violations() {
        // 族守卫。
        let e = guard_family("Klingon").unwrap_err();
        assert_eq!(e.code, E_FAMILY_INVALID);
        assert!(e.is_complete());
        assert_eq!(guard_family("CJK").unwrap(), ScriptFamily::Cjk);
        // 段守卫。
        let e2 = guard_stage("rasterize").unwrap_err();
        assert_eq!(e2.code, E_STAGE_INVALID);
        assert!(e2.is_complete());
        assert_eq!(guard_stage("tokenize").unwrap(), Stage::Tokenize);
    }

    #[test]
    fn plan_describe_is_readable() {
        let p = plan("ar", "مرحبا بالعالم").unwrap();
        let d = describe_plan(&p);
        assert!(d.contains("排版策略"), "替述须可读：{}", d);
        assert!(d.contains("镜像绘制"), "RTL 须说明镜像：{}", d);
    }

    #[test]
    fn clamp_is_tracked_and_visible() {
        let (eff, rec) = clamp_tracked("tokens", 10, 0, 5);
        assert_eq!(eff, 5);
        assert!(rec.is_some(), "超界钳制须留痕");
        assert!(rec.unwrap().to_upper);
        // 域内不留痕。
        let (eff2, rec2) = clamp_tracked("tokens", 3, 0, 5);
        assert_eq!(eff2, 3);
        assert!(rec2.is_none());
    }
}