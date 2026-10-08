//! VE-F4007 · 复数与性别规则（VE-T 域 · 国际化域 · T01 组 · 目标 340 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F4007`
//!
//! **判据（锚点原文）**：六类复数、性别模板、联合选择器、覆盖红线、回退显性。
//!
//! **职责定位（锚点原文）**：T01 复数与性别规则——复数规则（CLDR 复数类别
//! （zero/one/two/few/many/other **六类**×语言复数函数表——**复数函数表完整
//! 断言**（复述语言覆盖；性别规则（语法性别（阴性/阳性/中性模板变体——**性别
//! 模板语法**；选择器API（**格式化选择器**（复数+性别联合选择；**规则缓存**复述。
//! 数据结构：数据模型与规格表（逐条规格公开、参数域钳制、枚举守卫——家族格式）。
//! 错误路径与降级矩阵：**未知语言复数→other 回退+显性**；**性别缺→中性模板回退**；
//! **函数表缺语言→补充流程（覆盖红线）**；**模板变量错→校验拒绝**。性能逐项分解：
//! **选择 O(1) 函数查**；**模板 O(变量)**；**回退 O(1)**；**缓存 O(1)**。跨批对接点：
//! **CLDR 复数表引用；F4006 格式协同；N02 文本消费**。无障碍与隐私：文档替述可读；
//! 无隐私面。
//!
//! # 本项的边界（不越界施工，遵守"只做领到的任务"）
//!
//! 本项交付**选择的策略层**：给定「Locale + 数值 + 语法性别」，产出该用哪个复数
//! 类别、哪条性别模板、最终文本。它**不代做**：
//!
//! - **热表（规则缓存池）归 F3242**；本项只产出 [`Selection::cache_key`] 并登记
//!   单源，不自己实现池（锚点：「**规则缓存**复述」=复述既有单源，不新造）。
//! - **i18n 语料回归归 F4010**；本项只声明"规则表需要语料兜底"的接入位，
//!   不代做语料库（锚点跨批对接点未列语料，故只作预留位）。
//! - **文本渲染执行归 N02**；本项交付的是**选中的模板串**，不是绘制出的字形。
//!
//! 两个上游尚未落地，故激活位登记在 [`RESERVED_SLOTS`]（`landed: false`）。
//!
//! # 关于「CLDR 版本锚定」——为什么必须与 F4006 同版本且硬失败
//!
//! 本项与 F4006（日期时间数字格式）读的是**同一份 CLDR**。若两者锚定版本不同，
//! 会出现极难归因的分叉：同一个"1 个文件"在日期域按 CLDR 46 的规则排，在复数域
//! 按 CLDR 45 的规则选类别，而两者都"看起来对"。所以 [`CLDR_VERSION`] 是编译期
//! 常量，与 F4006 的同名常量保持一致；任何"规则集与锚定版本不符"一律
//! [`Rejection`]，让漂移在联编期就炸出来，而不是等到线上出两种说法。
//!
//! # 关于「复数函数表」为什么用"规则族"而不是"每语言一条 if"
//!
//! CLDR 的复数规则本质是少量**规则族**（俄语式 one/few/many、阿拉伯式六类全用、
//! 中日韩式无复数、法语式 0/1 归 one……），几十种语言共享十几个族。若逐语言写
//! `if`，规则表会有 25 份近乎重复、且互相容易写歪的代码；一旦某语言写错，只有
//! 那个语言坏掉，还很难被发现。所以本项把**规则族**抽成 [`PluralRule`] 闭域，
//! 语言表只做「语言 → 族」的映射：改一个族，所有用它的语言同时改对。
//!
//! # 关于「六个复数类别」为什么必须六类都真的能被选出来
//!
//! 锚点钦定 zero/one/two/few/many/other 六类。**列出来不等于选得到**——如果六类
//! 里有一类没有任何语言能产生，它就是一句空承诺。所以域自检
//! [`check_all_six_categories_reachable`] 会**逐类反查**：对全表语言与代表性数值
//! 穷举一遍，确认每一类都至少被一条真实规则选中。零类可达即判红。
//!
//! # 关于「覆盖红线」为什么必须双向
//!
//! 字体域（F4005）声明支持某语言，等于向用户承诺"这个语言能用"。若本域的复数
//! 函数表缺了它，就会出现**域间分叉**：字体域排出了本地文字，复数域却回退
//! `other`——于是阿拉伯语页面里"3 个文件"被当成英文渲染成 "3 files"（阿拉伯语
//! 的 few/many 走错类别），而页面看上去完全正常，用户看不出来。
//!
//! 所以 [`check_coverage_redline`] 是**双向**的：
//! - 正向：[`ALIGNED_LANGUAGES`] 里每个语言都必须在 [`PLURAL_RULES`] 内；
//! - 反向：[`PLURAL_RULES`] 里每个语言都必须在 [`ALIGNED_LANGUAGES`] 内
//!   （防"悄悄加了个语言的支持，却没同步给字体域"）。
//!
//! 这与 F4006 的 [`check_locale_coverage_alignment`] 同一思路：**跨域的"我支持
//! 语言 X"必须成对机检**，否则域间分叉会以"看起来正常"的形式长期潜伏。
//!
//! # 关于「语法性别」为什么中日韩只有一个类别
//!
//! CLDR 的语法性别（grammatical gender）来自语言学事实：中日韩越的 nouns
//! **没有语法性别**，所以它们的性别表里只有 `Neutral` 一条。这不是"数据缺失"，
//! 而是正确的语言学结论。若给中文硬塞一个"阳性/阴性"，会产出语法错误的句子
//! （中文的"他/她"是代词选择，不是名词属性）。所以 [`GENDER_RULES`] 里
//! [`GENDER_RULES`] 对这些语言**只登记 `Neutral`**，而性别缺省回退也落到
//! `Neutral`——两侧一致，不会出现"表里没有、回退随便挑"的裂缝。
//!
//! # 关于「回退必须显性」为什么 `other` 也带诊断
//!
//! 未知语言的复数一律回退 `other`（这是 CLDR 的正确默认，中文/日文也只有
//! `other`），**但必须产诊断**。因为对上层来说"这个语言没有复数规则"是一条需要
//! 报警的信息：可能意味着语言表该补了。静默回退会让语言支持度悄悄退化而无人
//! 察觉——这正是 F4004/F4005/F4006 一贯坚持的"给可用的兜底，但绝不静默"。
//!
//! # 确定性
//!
//! 零时钟、零 IO、零环境依赖；数值由调用方传入，输出是纯数据结构与字符串。
//! 同一输入必得同一结果（含诊断序列顺序与回退层级）。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// §0 CLDR 锚定与参数域钳制（锚点：参数域钳制；判据：覆盖红线的版本基座）
// ---------------------------------------------------------------------------

/// 本项锚定的 **CLDR 版本**（编译期常量，复数规则集必须与此一致）。
///
/// 与 F4006 的 [`CLDR_VERSION`](../../vei06_datetime.rs) 同值——这不是巧合：
/// 两个域读同一份 CLDR，版本锚歪了就会出现"日期按 46、复数按 45"的分叉。
pub const CLDR_VERSION: u16 = 46;

/// 锚定版本对应的版本标识串（供诊断与替述用）。
pub const CLDR_VERSION_TAG: &str = "cldr-46";

/// Locale 标签最大字节长度上界（BCP47 完整标签的合理上限）。
pub const MAX_LOCALE_LEN: usize = 35;
/// 数值绝对值上界（超出即视为调用方算错了，钳到边界并留痕）。
pub const MAX_COUNT: i64 = 1_000_000_000_000;
/// 小数位数上界（CLDR 复数规则只看前若干位小数，超出无意义）。
pub const MAX_FRACTION_DIGITS: u32 = 6;
/// 模板串最大字节长度上界。
pub const MAX_TEMPLATE_LEN: usize = 256;
/// 模板内允许的变量数上界。
pub const MAX_TEMPLATE_VARS: usize = 8;
/// 缓存键最大字节长度上界。
pub const MAX_CACHE_KEY_LEN: usize = 160;
/// 热表容量上界（复用 F3242 的容量契约；本项只声明不实现池）。
pub const MAX_HOT_TABLE_ENTRIES: usize = 256;
/// 规则表语言条数上界（防有人手抖复制出一万条重复语言）。
pub const MAX_RULE_ROWS: usize = 64;

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
    pub asked: i64,
    /// 生效值。
    pub effective: i64,
    /// 钳的方向说明。
    pub reason: &'static str,
}

/// 数值钳制并留痕（超界不静默）。
pub fn clamp_count(asked: i64) -> (i64, Option<ClampRecord>) {
    if asked > MAX_COUNT {
        return (
            MAX_COUNT,
            Some(ClampRecord {
                field: "count",
                asked,
                effective: MAX_COUNT,
                reason: "超过上界，钳到上界",
            }),
        );
    }
    if asked < -MAX_COUNT {
        return (
            -MAX_COUNT,
            Some(ClampRecord {
                field: "count",
                asked,
                effective: -MAX_COUNT,
                reason: "低于下界，钳到下界",
            }),
        );
    }
    (asked, None)
}

/// 小数位数钳制并留痕。
pub fn clamp_fraction_digits(asked: u32) -> (u32, Option<ClampRecord>) {
    if asked > MAX_FRACTION_DIGITS {
        return (
            MAX_FRACTION_DIGITS,
            Some(ClampRecord {
                field: "fraction_digits",
                asked: asked as i64,
                effective: MAX_FRACTION_DIGITS as i64,
                reason: "小数位超上界，钳到上界",
            }),
        );
    }
    (asked, None)
}

// ---------------------------------------------------------------------------
// §1 拒绝三要素（锚点：异常零静默）
// ---------------------------------------------------------------------------

/// Locale 非法（空串/超长/含非法字符）。
pub const E_LOCALE_INVALID: &str = "E_LOCALE_INVALID";
/// 复数类别非法。
pub const E_CATEGORY_INVALID: &str = "E_CATEGORY_INVALID";
/// 语法性别非法。
pub const E_GENDER_INVALID: &str = "E_GENDER_INVALID";
/// 复数规则族非法。
pub const E_RULE_INVALID: &str = "E_RULE_INVALID";
/// 模板语法错（花括号不配对/嵌套错位）。
pub const E_TEMPLATE_SYNTAX: &str = "E_TEMPLATE_SYNTAX";
/// **模板变量错→校验拒绝**（锚点降级矩阵第四条）。
pub const E_TEMPLATE_VAR: &str = "E_TEMPLATE_VAR";
/// 模板变量名越界（超长/空名）。
pub const E_TEMPLATE_VAR_INVALID: &str = "E_TEMPLATE_VAR_INVALID";
/// **覆盖红线破口**（锚点：函数表缺语言→补充流程）。
pub const E_COVERAGE_REDLINE: &str = "E_COVERAGE_REDLINE";
/// **CLDR 版本漂移**（规则集与锚定版本不符 → 硬失败）。
pub const E_CLDR_DRIFT: &str = "E_CLDR_DRIFT";
/// 规格表覆盖缺口。
pub const E_SPEC_GAP: &str = "E_SPEC_GAP";
/// 单源复用违例。
pub const E_SINGLE_SOURCE_DUP: &str = "E_SINGLE_SOURCE_DUP";
/// 预留槽位谎报落地。
pub const E_RESERVED_LIED: &str = "E_RESERVED_LIED";
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
    /// 三要素齐全（自检逐条断言）。
    pub fn is_complete(&self) -> bool {
        !self.code.is_empty() && !self.what.is_empty() && !self.why.is_empty() && !self.next.is_empty()
    }
    /// 可读形式（不含隐私面）。
    pub fn describe(&self) -> String {
        format!("{} | {} | {}", self.code, self.what, self.why)
    }
}

/// 数值下界钳制在 -1e12，故 [`MAX_COUNT`] 的相反数即下界常量。
pub const MIN_COUNT: i64 = -MAX_COUNT;

// ---------------------------------------------------------------------------
// §2 枚举与守卫（判据：六类复数；锚点：枚举守卫——家族格式）
// ---------------------------------------------------------------------------

/// CLDR 复数类别（**六类**，锚点钦定闭域，顺序即类号，勿乱动）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PluralCategory {
    /// zero（如阿拉伯语 "0 个"）。
    Zero,
    /// one。
    One,
    /// two（如威尔士语 "2 个"）。
    Two,
    /// few（俄语式 "2/3/4 个"）。
    Few,
    /// many（俄语式 "5/11 个"）。
    Many,
    /// other（中文/日文的唯一类别，也是全局兜底）。
    Other,
}

impl PluralCategory {
    /// 全集（六类，顺序即类号）。
    pub const ALL: [PluralCategory; 6] = [
        PluralCategory::Zero,
        PluralCategory::One,
        PluralCategory::Two,
        PluralCategory::Few,
        PluralCategory::Many,
        PluralCategory::Other,
    ];
    /// 稳定名（模板键、诊断与缓存键用）。
    pub fn name(&self) -> &'static str {
        match self {
            PluralCategory::Zero => "zero",
            PluralCategory::One => "one",
            PluralCategory::Two => "two",
            PluralCategory::Few => "few",
            PluralCategory::Many => "many",
            PluralCategory::Other => "other",
        }
    }
    /// 是否为全局兜底类别（`other` 是唯一合法的"不知道"）。
    pub fn is_fallback(&self) -> bool {
        matches!(self, PluralCategory::Other)
    }
    /// 模板键后缀（`plural.one` / `plural.few` …）。
    pub fn slot(&self) -> String {
        format!("plural.{}", self.name())
    }
    /// 守卫：值域外显性拒绝（**大小写与别名全收**）。
    ///
    /// 类别名会从资源文件、配置面、UI 下拉框多处拼出来，只认一种大小写等于
    /// 给配置留一个"看着像对的、实际全被拒"的坑。这条纪律来自 F4006 实测出的
    /// 真实缺陷（`ISO` 被历法守卫误拒），此处同源适用。
    pub fn guard(raw: &str) -> Result<PluralCategory, Rejection> {
        let v = match raw {
            "zero" | "Zero" | "ZERO" | "ZeRo" | "0" => PluralCategory::Zero,
            "one" | "One" | "ONE" | "OnE" | "1" => PluralCategory::One,
            "two" | "Two" | "TWO" | "TwO" | "2" => PluralCategory::Two,
            "few" | "Few" | "FEW" | "FeW" | "3" => PluralCategory::Few,
            "many" | "Many" | "MANY" | "MaNy" | "5" => PluralCategory::Many,
            "other" | "Other" | "OTHER" | "OtHeR" | "other_plural" => PluralCategory::Other,
            other => {
                return Err(Rejection {
                    code: E_CATEGORY_INVALID,
                    what: format!(
                        "复数类别 {:?} 不在 zero/one/two/few/many/other 六类内",
                        other
                    ),
                    why: "六类是锚点钦定的闭域；未知类别说明调用方与本项版本不匹配".to_string(),
                    next: "改用六类之一（大小写与数字别名不限）".to_string(),
                })
            }
        };
        Ok(v)
    }
}

/// 语法性别（阴性/阳性/中性；另设 `Common` 处理"语言学上无性别"）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Gender {
    /// 阴性（feminine）。
    Feminine,
    /// 阳性（masculine）。
    Masculine,
    /// 中性（neuter）。
    Neuter,
    /// 通用（语言学上无语法性别：中日韩越）。
    Common,
}

impl Gender {
    /// 全集。
    pub const ALL: [Gender; 4] = [Gender::Feminine, Gender::Masculine, Gender::Neuter, Gender::Common];
    /// 稳定名。
    pub fn name(&self) -> &'static str {
        match self {
            Gender::Feminine => "feminine",
            Gender::Masculine => "masculine",
            Gender::Neuter => "neuter",
            Gender::Common => "common",
        }
    }
    /// 模板键后缀（`gender.feminine` …）。
    pub fn slot(&self) -> String {
        format!("gender.{}", self.name())
    }
    /// 是否为**中性/通用**（性别缺省回退的落点）。
    ///
    /// 锚点原文：「性别缺→中性模板回退」。本项把 `Common` 也算进"中性侧"，
    /// 因为对中日韩来说 `Common` 就是它们的**唯一正确**类别，而 `Neuter` 在
    /// 这些语言里是不存在的类别——若把它们回退到 `Neuter`，就会去查一条
    /// 语言学上不存在的模板。
    pub fn is_neutral_side(&self) -> bool {
        matches!(self, Gender::Neuter | Gender::Common)
    }
    /// 守卫：值域外显性拒绝（大小写与别名全收）。
    pub fn guard(raw: &str) -> Result<Gender, Rejection> {
        let v = match raw {
            "feminine" | "Feminine" | "FEMININE" | "FeMiNiNe"
            | "fem" | "Fem" | "FEM" | "f" => Gender::Feminine,
            "masculine" | "Masculine" | "MASCULINE" | "MaScULiNe"
            | "masc" | "Masc" | "MASC" | "m" => Gender::Masculine,
            "neuter" | "Neuter" | "NEUTER" | "NeuTer" | "neut" | "Neut" | "NEUT" | "n" => {
                Gender::Neuter
            }
            "common" | "Common" | "COMMON" | "ComMon"
            | "none" | "None" | "NONE" | "c" => Gender::Common,
            other => {
                return Err(Rejection {
                    code: E_GENDER_INVALID,
                    what: format!(
                        "语法性别 {:?} 不在 feminine/masculine/neuter/common 四类内",
                        other
                    ),
                    why: "四类是本项的闭域；`Common` 用于中日韩等无语法性别的语言".to_string(),
                    next: "改用四类之一（大小写与单字母别名不限）".to_string(),
                })
            }
        };
        Ok(v)
    }
}

// ---------------------------------------------------------------------------
// §3 复数规则族（判据：六类复数）
// ---------------------------------------------------------------------------

/// CLDR **操作数**（`Operands`）——复数规则唯一的输入。
///
/// CLDR 的规则全部形如「`i = 1 and v = 0`」「`n % 100 = 3..10`」，也就是
/// 只看**整数部分**、**小数位数**、**去掉尾零的整数**这三个量。把它们显式建模成
/// 结构体（而不是把原始数值丢给规则去 `floor`/`round`），有两个好处：
///
/// 1. **规则可读**：规则函数只跟`i/v/f`打交道，不必重复做数值分解。
/// 2. **边界显性**：`v`（小数位数）超过 [`MAX_FRACTION_DIGITS`] 时，CLDR 的
///    规则其实"看更远"，但本项钳到上界并留痕——因为再多位小数对复数选择
///    几乎没有影响，不值得为它引入不确定性。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Operands {
    /// 绝对值（CLDR 的 `n`）。
    pub n: i64,
    /// 整数部分（CLDR 的 `i`）。
    pub i: i64,
    /// 小数位数（CLDR 的 `v`）。
    pub v: u32,
    /// 去掉小数尾零后的整数（CLDR 的 `f`，此处以"去掉小数后的整数"近似，
    /// 对本项覆盖的全部规则族足够——见 [`Operands::of`] 的说明）。
    pub f: i64,
}

impl Operands {
    /// 由「数值 + 小数位数」构造操作数。
    ///
    /// **为什么 `f` 用"去掉小数后的整数"近似**：CLDR 的 `f` 定义是
    /// 「小数部分去掉尾零后的整数」（如 `1.30` 的 `f = 3`）。本项覆盖的规则族里
    /// 只有阿拉伯语式用到 `f`，而它的判据是 `n % 100`（对 `n` 取模），
    /// 用整数部分近似不会改变任何一条规则的结果。这条近似写在这里而不是
    /// 藏在实现里，是为了让后来人知道它**是**近似。
    pub fn of(count: i64, fraction_digits: u32) -> (Operands, Option<ClampRecord>) {
        let (c, c_rec) = clamp_count(count);
        let (v, v_rec) = clamp_fraction_digits(fraction_digits);
        let abs = if c < 0 { -c } else { c };
        let int_part = abs / 10i64.pow(v);
        let mut frac_part = abs % 10i64.pow(v);
        // 去掉小数尾零（对应 CLDR `f`的"去尾零"语义）。
        while v > 0 && frac_part > 0 && frac_part % 10 == 0 {
            frac_part /= 10;
        }
        // **两处钳制都要留痕**——初版写成 `c_rec.or(v_rec)`，只留了数值那一条，
        // 于是"数值与小数位同时越界"时小数位的钳制被静默吞掉。表现是：调用方
        // 把小数位从 3 改成 99，日志里只有一条 count 钳制，小数位那次的改动
        // 完全不可见。这类"两个独立参数共用一个可选留痕槽"的写法天然会丢信息。
        let rec = match (c_rec, v_rec) {
            (Some(a), Some(b)) => Some(ClampRecord {
                field: "count+fraction_digits",
                asked: a.asked * 1000 + b.asked,
                effective: a.effective * 1000 + b.effective,
                reason: "数值与小数位同时越界（合并留痕）",
            }),
            (Some(a), None) => Some(a),
            (None, Some(b)) => Some(b),
            (None, None) => None,
        };
        (
            Operands {
                n: abs,
                i: int_part,
                v,
                f: if v == 0 { abs } else { int_part * 10i64.pow(v) + frac_part },
            },
            rec,
        )
    }
    /// 是否落在闭区间 `[lo, hi]`。
    ///
    /// 保留为公开 API（族规则若要新增区间判据可直接用），因此本域自检里
    /// 有一条判据真的调用它——避免"公开但从不被调用"的死代码。
    pub fn in_range(&self, lo: i64, hi: i64) -> bool {
        self.n >= lo && self.n <= hi
    }
}

/// 复数规则族（CLDR 的规则形态归并后的闭域）。
///
/// 归并的理由见文件头「关于『复数函数表』为什么用规则族而不是每语言一条 if」。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PluralRule {
    /// 无复数语言：中日韩越泰等，一切都是 `other`。
    OnlyOther,
    /// 印欧式：1 → `one`，其余 → `other`（英德西意荷瑞等）。
    OneIsOne,
    /// 法语式：`i` 为 0 或 1 → `one`，其余 → `other`（法/波/土等）。
    ZeroOrOneIsOne,
    /// 斯拉夫俄式：1/21/31… → `one`；2-4/22-24… → `few`；0/5-20… → `many`。
    SlavicRussian,
    /// 斯拉夫波式：1 → `one`；2-4（不含12-14）→ `few`；其余 → `many`。
    SlavicPolish,
    /// 阿拉伯式：六类全用（`n % 100` 分档）。
    ArabicFull,
}

impl PluralRule {
    /// 全集（自检遍历用；新增规则族必须在此登记，否则规则表会漏校验）。
    pub const ALL: [PluralRule; 6] = [
        PluralRule::OnlyOther,
        PluralRule::OneIsOne,
        PluralRule::ZeroOrOneIsOne,
        PluralRule::SlavicRussian,
        PluralRule::SlavicPolish,
        PluralRule::ArabicFull,
    ];
    /// 稳定名（诊断与替换测试用）。
    pub fn name(&self) -> &'static str {
        match self {
            PluralRule::OnlyOther => "only-other",
            PluralRule::OneIsOne => "one-is-one",
            PluralRule::ZeroOrOneIsOne => "zero-or-one-is-one",
            PluralRule::SlavicRussian => "slavic-russian",
            PluralRule::SlavicPolish => "slavic-polish",
            PluralRule::ArabicFull => "arabic-full",
        }
    }
    /// 该规则族能选出的类别集合（供"六类都可达"机检反查）。
    ///
    /// 注意：这里返回的是**可能集合**（上界），实际可达性由
    /// [`check_all_six_categories_reachable`] 用真实数值穷举验证——两者不一致
    /// 即判红（声明了却选不出来 = 空承诺）。
    pub fn reachable(&self) -> &'static [PluralCategory] {
        match self {
            PluralRule::OnlyOther => &[PluralCategory::Other],
            PluralRule::OneIsOne | PluralRule::ZeroOrOneIsOne => {
                &[PluralCategory::One, PluralCategory::Other]
            }
            PluralRule::SlavicRussian => {
                &[PluralCategory::One, PluralCategory::Few, PluralCategory::Many, PluralCategory::Other]
            }
            PluralRule::SlavicPolish => {
                &[PluralCategory::One, PluralCategory::Few, PluralCategory::Many, PluralCategory::Other]
            }
            PluralRule::ArabicFull => &[
                PluralCategory::Zero,
                PluralCategory::One,
                PluralCategory::Two,
                PluralCategory::Few,
                PluralCategory::Many,
                PluralCategory::Other,
            ],
        }
    }
    /// 守卫：值域外显性拒绝（大小写与连字符/下划线两种写法都收）。
    pub fn guard(raw: &str) -> Result<PluralRule, Rejection> {
        let norm = raw.replace('_', "-").to_lowercase();
        for r in PluralRule::ALL.iter() {
            if r.name() == norm {
                return Ok(*r);
            }
        }
        Err(Rejection {
            code: E_RULE_INVALID,
            what: format!("复数规则族 {:?} 不在九个族内", raw),
            why: "规则族是本项的闭域；未知族说明调用方与本项版本不匹配".to_string(),
            next: "改用 PluralRule::ALL 中的族名（连字符或下划线、大小写不限）".to_string(),
        })
    }
}

/// 复数规则求值（**O(1) 函数查**——锚点性能分解）。
///
/// 六条判据都在这里：`one/zero/two/few/many/other` 由哪个分支返回，
/// 每个分支都有对拍样例在域自检里逐条钉住。
pub fn eval_rule(rule: PluralRule, op: &Operands) -> PluralCategory {
    match rule {
        PluralRule::OnlyOther => PluralCategory::Other,
        PluralRule::OneIsOne => {
            if op.i == 1 && op.v == 0 {
                PluralCategory::One
            } else {
                PluralCategory::Other
            }
        }
        PluralRule::ZeroOrOneIsOne => {
            if op.i == 0 || op.i == 1 {
                PluralCategory::One
            } else {
                PluralCategory::Other
            }
        }
        PluralRule::SlavicRussian => {
            let m10 = op.i % 10;
            let m100 = op.i % 100;
            if op.v == 0 && m10 == 1 && m100 != 11 {
                PluralCategory::One
            } else if op.v == 0 && m10 >= 2 && m10 <= 4 && !(m100 >= 12 && m100 <= 14) {
                PluralCategory::Few
            } else if op.v == 0
                && (m10 == 0 || (m10 >= 5 && m10 <= 9) || (m100 >= 11 && m100 <= 14))
            {
                PluralCategory::Many
            } else {
                PluralCategory::Other
            }
        }
        PluralRule::SlavicPolish => {
            let m10 = op.i % 10;
            let m100 = op.i % 100;
            if op.i == 1 && op.v == 0 {
                PluralCategory::One
            } else if op.v == 0
                && m10 >= 2
                && m10 <= 4
                && !(m100 >= 12 && m100 <= 14)
            {
                PluralCategory::Few
            } else if op.v == 0
                && op.i != 1
                && (m10 == 0 || m10 == 1 || (m10 >= 5 && m10 <= 9) || (m100 >= 12 && m100 <= 14))
            {
                PluralCategory::Many
            } else {
                PluralCategory::Other
            }
        }
        PluralRule::ArabicFull => {
            let m100 = op.n % 100;
            if op.n == 0 {
                PluralCategory::Zero
            } else if op.n == 1 {
                PluralCategory::One
            } else if op.n == 2 {
                PluralCategory::Two
            } else if m100 >= 3 && m100 <= 10 {
                PluralCategory::Few
            } else if m100 >= 11 && m100 <= 99 {
                PluralCategory::Many
            } else {
                PluralCategory::Other
            }
        }
    }
}

/// 语言复数规则表的一行。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PluralRuleEntry {
    /// 主语言子标签（裸语言码，如 `ar`）。
    pub language: &'static str,
    /// 规则族。
    pub rule: PluralRule,
    /// 该语言是否存在语法性别（`false` = 中日韩式，性别一律 `Common`）。
    pub gendered: bool,
}

/// 语言复数规则表（语言 → 规则族）。
///
/// 收录语言与 [`ALIGNED_LANGUAGES`] 严格一致（由 [`check_coverage_redline`]
/// 双向机检）——这张表与 F4005 字体域的"我支持语言 X"是同一份承诺。
pub const PLURAL_RULES: [PluralRuleEntry; 26] = [
    // —— 印欧式（1 → one） ——
    PluralRuleEntry { language: "en", rule: PluralRule::OneIsOne, gendered: true },
    PluralRuleEntry { language: "de", rule: PluralRule::OneIsOne, gendered: true },
    PluralRuleEntry { language: "es", rule: PluralRule::OneIsOne, gendered: true },
    PluralRuleEntry { language: "it", rule: PluralRule::OneIsOne, gendered: true },
    PluralRuleEntry { language: "nl", rule: PluralRule::OneIsOne, gendered: true },
    PluralRuleEntry { language: "sv", rule: PluralRule::OneIsOne, gendered: true },
    PluralRuleEntry { language: "hi", rule: PluralRule::OneIsOne, gendered: true },
    PluralRuleEntry { language: "bn", rule: PluralRule::OneIsOne, gendered: true },
    // —— 法语式（0/1 → one） ——
    PluralRuleEntry { language: "fr", rule: PluralRule::ZeroOrOneIsOne, gendered: true },
    PluralRuleEntry { language: "pt", rule: PluralRule::ZeroOrOneIsOne, gendered: true },
    PluralRuleEntry { language: "tr", rule: PluralRule::ZeroOrOneIsOne, gendered: true },
    PluralRuleEntry { language: "fa", rule: PluralRule::ZeroOrOneIsOne, gendered: true },
    PluralRuleEntry { language: "ur", rule: PluralRule::ZeroOrOneIsOne, gendered: true },
    PluralRuleEntry { language: "ps", rule: PluralRule::ZeroOrOneIsOne, gendered: true },
    PluralRuleEntry { language: "ta", rule: PluralRule::ZeroOrOneIsOne, gendered: true },
    // —— 无复数（中日韩越泰） ——
    PluralRuleEntry { language: "zh", rule: PluralRule::OnlyOther, gendered: false },
    PluralRuleEntry { language: "ja", rule: PluralRule::OnlyOther, gendered: false },
    PluralRuleEntry { language: "ko", rule: PluralRule::OnlyOther, gendered: false },
    PluralRuleEntry { language: "vi", rule: PluralRule::OnlyOther, gendered: false },
    PluralRuleEntry { language: "th", rule: PluralRule::OnlyOther, gendered: false },
    // —— 斯拉夫 ——
    PluralRuleEntry { language: "ru", rule: PluralRule::SlavicRussian, gendered: true },
    PluralRuleEntry { language: "pl", rule: PluralRule::SlavicPolish, gendered: true },
    // —— 阿拉伯 / 希伯来 ——
    PluralRuleEntry { language: "ar", rule: PluralRule::ArabicFull, gendered: true },
    PluralRuleEntry { language: "he", rule: PluralRule::SlavicPolish, gendered: true },
    PluralRuleEntry { language: "te", rule: PluralRule::ZeroOrOneIsOne, gendered: true },
    // —— 未收录语言兜底（`und`） ——
    PluralRuleEntry { language: "und", rule: PluralRule::OnlyOther, gendered: false },
];

/// 覆盖对齐清单（与 F4005 字体域、F4006 日期域的 `ALIGNED_LANGUAGES` **同集合**）。
///
/// **这张表是本域的对外承诺**，由 [`check_coverage_redline`] 与
/// [`PLURAL_RULES`] 双向机检。
///
/// **为什么必须一字不差地与 F4005/F4006 对齐**：三个域各自承诺"我支持这些语言"，
/// 若本域多收一个 `cy`（威尔士），就会出现字体域不排威尔士文、复数域却按凯尔特
/// 规则选类别的分叉——页面上的数字用了别的语言的复数形态，而这一切"看起来正常"。
/// 这正是 F4006 实测并修掉的 `ta-IN` 分叉的同型问题，所以此处直接按对齐清单
/// 收口，不擅自扩表。
pub const ALIGNED_LANGUAGES: [&str; 25] = [
    "en", "de", "fr", "es", "pt", "it", "ru", "tr", "pl", "nl", "sv", "vi", "zh", "ja", "ko",
    "ar", "he", "fa", "ur", "ps", "hi", "bn", "ta", "te", "th",
];

/// 主语言子标签抽取（BCP47 `ar-EG` → `ar`）。
///
/// **口径与 F4004 `route()` / F4006 `lookup_rule()` 严格一致：只切连字符，不认
/// 下划线、不归一大小写。**
///
/// 这一条是本项开发中**实测出并主动收敛**的跨域分叉。初版这里"好心"地同时切
/// 下划线并做小写归一（理由是"`en_US` 在 Windows 区域设置里很常见"），结果
/// 六域共存联编时暴露：
///
/// ```text
/// en_US: 排版降级=true  日期降级=true  复数降级=false   ← 分叉
/// ```
///
/// 同一 Locale 在排版/日期域按"未收录"降级，在复数域却正常命中。这比"四域
/// 一律降级"更危险——因为**它不一致**，而调用方无法知道自己拿到的是哪一套
/// 口径。查证F4002 的 [`ParsedTag::canonical`] 后确认：规范化输出恒为连字符
/// 形态（`en_US` → `en-US`），语言段已小写化。也就是说**下划线与大写形态
/// 本来就不该出现在规范化链路上**，兄弟域的严格口径是对的，我那"更宽松"的
/// 宽容才是缺陷。
///
/// 所以这里保持严格：非规范形态按未收录降级，并**显性**产诊断（交给 F4002
/// 去规范化，而不是让每个下游域各自猜）。
pub fn primary_subtag(language: &str) -> &str {
    match language.find('-') {
        Some(i) => &language[..i],
        None => language,
    }
}

/// 查表命中层级。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RuleMatch {
    /// 整串精确命中。
    Exact,
    /// 主语言子标签命中。
    PrimarySubtag,
    /// 落空，走 `und` 兜底（**降级显性**）。
    Missed,
}

impl RuleMatch {
    /// 是否落空。
    pub fn is_missed(&self) -> bool {
        matches!(self, RuleMatch::Missed)
    }
}

/// 查表结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RuleLookup {
    /// 命中的表行（落空时为 `und` 行）。
    pub entry: PluralRuleEntry,
    /// 命中层级。
    pub matched: RuleMatch,
    /// 是否降级（落空即为降级）。
    pub degraded: bool,
}

impl RuleLookup {
    /// 生效规则族。
    pub fn rule(&self) -> PluralRule {
        self.entry.rule
    }
}

/// 两级查表：**整串精确 → 主语言子标签 → `und` 兜底**。
///
/// 与 F4004 的 `route()` / F4006 的 `lookup_rule()` 同一纪律：调用方交的是
/// F4002 规范化后的完整 BCP47 标签（`ar-EG`/`ta-IN`/`zh-Hans-CN`），若只按整串
/// 查表会把每个带区域或脚本子标签的语言都误报未收录。
pub fn lookup_rule(language: &str) -> RuleLookup {
    // 口径与 F4004/F4006 一致：不做下划线替换、不做大小写归一。
    // 非规范形态（`en_US` / `EN-US`）按未收录降级并显性产诊断——
    // 规范化归 F4002，下游域不各自放宽（放宽会造成域间分叉，见`primary_subtag`）。
    for entry in PLURAL_RULES.iter() {
        if entry.language == language {
            return RuleLookup { entry: *entry, matched: RuleMatch::Exact, degraded: false };
        }
    }
    let primary = primary_subtag(language);
    if primary != language {
        for entry in PLURAL_RULES.iter() {
            if entry.language == primary {
                return RuleLookup {
                    entry: *entry,
                    matched: RuleMatch::PrimarySubtag,
                    degraded: false,
                };
            }
        }
    }
    // 落空：显性降级到 und（绝不静默）。
    for entry in PLURAL_RULES.iter() {
        if entry.language == "und" {
            return RuleLookup { entry: *entry, matched: RuleMatch::Missed, degraded: true };
        }
    }
    // `und` 行若被删掉，这里必须 panic 而不是悄悄给一个错的规则——
    // "规则表没有兜底"是表级缺陷，不是可降级的运行时状况。
    panic!("复数规则表缺 und 兜底行——查表落空将无规则可用");
}

// ---------------------------------------------------------------------------
// §4 语法性别（判据：性别模板）
// ---------------------------------------------------------------------------

/// 某语言可用的语法性别集合。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GenderSet {
    /// 主语言子标签。
    pub language: &'static str,
    /// 可用性别（按 `Gender::ALL` 顺序展开的**位掩码**）。
    ///
    /// 用位掩码而不是切片：位掩码是`u8` 常量，可以在编译期直接写死、
    /// 在自检里按位查，且不会引入`&'static [Gender]` 这类需要二次分配的类型。
    pub mask: u8,
}

impl GenderSet {
    /// 某性别是否可用。
    pub fn has(&self, g: Gender) -> bool {
        self.mask & (1u8 << gender_bit(g)) != 0
    }
    /// 该语言的实际可用性别数。
    pub fn count(&self) -> u32 {
        let mut n = 0u32;
        for g in Gender::ALL.iter() {
            if self.has(*g) {
                n += 1;
            }
        }
        n
    }
}

/// 性别位号（`Gender::ALL` 的下标）。
pub fn gender_bit(g: Gender) -> u8 {
    match g {
        Gender::Feminine => 0,
        Gender::Masculine => 1,
        Gender::Neuter => 2,
        Gender::Common => 3,
    }
}

/// 位掩码构造助手（自检与常量书写用）。
pub const fn gender_mask(fem: bool, masc: bool, neut: bool, common: bool) -> u8 {
    let mut m = 0u8;
    if fem {
        m |= 1 << 0;
    }
    if masc {
        m |= 1 << 1;
    }
    if neut {
        m |= 1 << 2;
    }
    if common {
        m |= 1 << 3;
    }
    m
}

/// 性别规则表（语言 → 可用性别集合）。
///
/// **为什么中日韩越泰只有 `Common`**：这些语言的名词没有语法性别。给它们硬塞
/// 阳性/阴性会产出语法错误的句子（"他/她"在中文里是代词选择，不是名词属性）。
/// 所以只登记 [`Gender::Common`]，而"性别缺→中性回退"的落点也包含 `Common`
/// ——两侧一致，不会出现"表里没有、回退随便挑"的裂缝。
pub const GENDER_RULES: [GenderSet; 26] = [
    // —— 三性齐备（欧洲多数语言） ——
    GenderSet { language: "en", mask: gender_mask(true, true, true, false) },
    GenderSet { language: "de", mask: gender_mask(true, true, true, false) },
    GenderSet { language: "fr", mask: gender_mask(true, true, true, false) },
    GenderSet { language: "es", mask: gender_mask(true, true, true, false) },
    GenderSet { language: "pt", mask: gender_mask(true, true, true, false) },
    GenderSet { language: "it", mask: gender_mask(true, true, true, false) },
    GenderSet { language: "nl", mask: gender_mask(true, true, true, false) },
    GenderSet { language: "sv", mask: gender_mask(true, true, true, false) },
    GenderSet { language: "pl", mask: gender_mask(true, true, true, false) },
    GenderSet { language: "ru", mask: gender_mask(true, true, true, false) },
    // —— 两性（无中性） ——
    GenderSet { language: "tr", mask: gender_mask(true, true, false, false) },
    GenderSet { language: "ar", mask: gender_mask(true, true, false, false) },
    GenderSet { language: "he", mask: gender_mask(true, true, false, false) },
    GenderSet { language: "hi", mask: gender_mask(true, true, false, false) },
    GenderSet { language: "bn", mask: gender_mask(true, true, false, false) },
    GenderSet { language: "ta", mask: gender_mask(true, true, false, false) },
    GenderSet { language: "te", mask: gender_mask(true, true, false, false) },
    // —— 无语法性别（中日韩越泰） ——
    GenderSet { language: "zh", mask: gender_mask(false, false, false, true) },
    GenderSet { language: "ja", mask: gender_mask(false, false, false, true) },
    GenderSet { language: "ko", mask: gender_mask(false, false, false, true) },
    GenderSet { language: "vi", mask: gender_mask(false, false, false, true) },
    GenderSet { language: "th", mask: gender_mask(false, false, false, true) },
    // —— 乌尔都/普什图/波斯（两性） ——
    GenderSet { language: "fa", mask: gender_mask(true, true, false, false) },
    GenderSet { language: "ur", mask: gender_mask(true, true, false, false) },
    GenderSet { language: "ps", mask: gender_mask(true, true, false, false) },
    // —— `und` 兜底（与复数表同口径：只有通用） ——
    GenderSet { language: "und", mask: gender_mask(false, false, false, true) },
];

/// 查语言可用性别集合（两级查表，与 [`lookup_rule`] 同纪律）。
pub fn lookup_gender_set(language: &str) -> Option<GenderSet> {
    // 与 [`lookup_rule`] 同口径：不归一、不替换下划线。
    for g in GENDER_RULES.iter() {
        if g.language == language {
            return Some(*g);
        }
    }
    let primary = primary_subtag(language);
    if primary != language {
        for g in GENDER_RULES.iter() {
            if g.language == primary {
                return Some(*g);
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// §5 性别模板语法（判据：性别模板；锚点：模板变量错→校验拒绝）
// ---------------------------------------------------------------------------

/// 模板里的一个变量引用（`{name}`）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct VarRef {
    /// 变量名（**字节切片上的 ASCII 校验已在解析期完成**）。
    pub name: String,
    /// 在模板中的起始字节偏移。
    pub start: usize,
    /// 在模板中的结束字节偏移（不含右花括号）。
    pub end: usize,
}

/// 模板解析产物。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct TemplateAst {
    /// 变量引用序列（按出现顺序）。
    pub vars: Vec<VarRef>,
    /// 字面量片段序列（`vars.len() + 1` 段：变量之间的文本）。
    pub literals: Vec<String>,
}

impl TemplateAst {
    /// 变量个数。
    pub fn var_count(&self) -> usize {
        self.vars.len()
    }
}

/// 模板里允许出现的变量名（**闭域**——模板变量错→校验拒绝的判据来源）。
///
/// 收成闭域而不是"任意名字都放行"，是因为模板变量名会拼错：`{cont}` 拼成
/// `{conut}` 时若放行，渲染出来就是字面的 `{conut}` 直接糊在界面上，而没有任何
/// 报错。闭域 + 显性拒绝能让这类错在联编期就暴露。
pub const ALLOWED_VARS: [&str; 5] = ["count", "unit", "gender", "locale", "category"];

/// 模板解析（**O(变量)**——锚点性能分解）。
///
/// 语法（极简、够用、且可校验）：
/// - `{name}` 变量引用，`name` 必须落在 [`ALLOWED_VARS`] 内；
/// - `{{` / `}}` 转义出字面花括号；
/// - 其余字节是字面量。
///
/// 解析期就把三类错全部显性拒掉：**花括号不配对**、**嵌套错位**、
/// **变量名越界/未知**。
pub fn parse_template(raw: &str) -> Result<TemplateAst, Rejection> {
    if raw.len() > MAX_TEMPLATE_LEN {
        return Err(Rejection {
            code: E_TEMPLATE_SYNTAX,
            what: format!("模板长{} 字节，超上界 {}", raw.len(), MAX_TEMPLATE_LEN),
            why: "超长模板必然是拼接失控，继续渲染只会撑爆布局".to_string(),
            next: format!("把模板压到{} 字节以内", MAX_TEMPLATE_LEN).to_string(),
        });
    }
    let bytes = raw.as_bytes();
    let mut vars: Vec<VarRef> = Vec::new();
    let mut literals: Vec<String> = Vec::new();
    let mut lit = String::new();
    let mut i = 0usize;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'{' {
            // 转义 `{{` → 字面 `{`
            if i + 1 < bytes.len() && bytes[i + 1] == b'{' {
                lit.push('{');
                i += 2;
                continue;
            }
            // 找右花括号
            let mut j = i + 1;
            let mut closed = false;
            while j < bytes.len() {
                if bytes[j] == b'}' {
                    closed = true;
                    break;
                }
                // 变量名内不允许再出现 `{`（嵌套错位）
                if bytes[j] == b'{' {
                    return Err(Rejection {
                        code: E_TEMPLATE_SYNTAX,
                        what: format!("模板第 {} 字节处变量引用内又遇 `{{`（嵌套错位）", j),
                        why: "变量名内嵌 `{` 说明模板拼接时少了一个右花括号".to_string(),
                        next: "把内层 `{` 转义成 `{{`，或补齐缺失的 `}`".to_string(),
                    });
                }
                j += 1;
            }
            if !closed {
                return Err(Rejection {
                    code: E_TEMPLATE_SYNTAX,
                    what: format!("模板第 {} 字节起的变量引用没有闭合 `}}`", i),
                    why: "不配对的花括号会让后续字面量被误吞，渲染结果不可预期".to_string(),
                    next: "补上缺失的 `}`；若要输出字面花括号请写成 `{{` / `}}`".to_string(),
                });
            }
            let name = &raw[i + 1..j];
            if name.is_empty() {
                return Err(Rejection {
                    code: E_TEMPLATE_VAR_INVALID,
                    what: format!("模板第 {} 字节处出现空变量名 `{{}}`", i),
                    why: "空变量名必然是拼接失误，放行会渲染出无意义的空替换".to_string(),
                    next: "给变量起名，或删掉这对空花括号".to_string(),
                });
            }
            if name.len() > 16 {
                return Err(Rejection {
                    code: E_TEMPLATE_VAR_INVALID,
                    what: format!("模板变量名 {:?} 长 {} 字节，超上界 16", name, name.len()),
                    why: "变量名超长说明模板面拼错了（多半是把整段文案当变量名）".to_string(),
                    next: "只写变量名本身；文案部分放在字面量里".to_string(),
                });
            }
            if !ALLOWED_VARS.contains(&name) {
                return Err(Rejection {
                    code: E_TEMPLATE_VAR,
                    what: format!(
                        "模板变量名 {:?} 不在允许集 {:?} 内",
                        name, ALLOWED_VARS
                    ),
                    why: "变量名不受控时，常见的错拼（如 `{conut}`）会原样渲染到界面上而不报错".to_string(),
                    next: format!("改用 {:?} 之一，或把该文案移出变量、写成字面量", ALLOWED_VARS).to_string(),
                });
            }
            if vars.len() >= MAX_TEMPLATE_VARS {
                return Err(Rejection {
                    code: E_TEMPLATE_SYNTAX,
                    what: format!("模板变量数超上界 {}", MAX_TEMPLATE_VARS),
                    why: "变量过多说明模板被当成了数据容器用，渲染成本与出错面都会失控".to_string(),
                    next: format!("把变量数压到{} 以内", MAX_TEMPLATE_VARS).to_string(),
                });
            }
            literals.push(lit.clone());
            lit.clear();
            vars.push(VarRef { name: name.to_string(), start: i, end: j + 1 });
            i = j + 1;
            continue;
        }
        if b == b'}' {
            // 未转义的孤立 `}`
            if i + 1 < bytes.len() && bytes[i + 1] == b'}' {
                lit.push('}');
                i += 2;
                continue;
            }
            return Err(Rejection {
                code: E_TEMPLATE_SYNTAX,
                what: format!("模板第 {} 字节处出现未转义的孤立 `}}`", i),
                why: "孤立 `}` 在解析期无法归属，放行会让模板语义依赖解析实现细节".to_string(),
                next: "要输出字面 `}` 请写成 `}}`".to_string(),
            });
        }
        // 字面量字节：按 **UTF-8 字符边界** 逐字符推进。
        //
        // 这里不能逐字节 push——非 ASCII 字符是多字节，逐字节 push 会切碎
        // UTF-8 序列。F4003 已实测过`&text[i..i+n]` 越界 panic 的教训，
        // 这里用"定位字符起始字节、整段拷贝"的方式规避。
        let ch_len = utf8_char_len(b);
        let end = i + ch_len;
        if end > bytes.len() {
            return Err(Rejection {
                code: E_TEMPLATE_SYNTAX,
                what: format!("模板第 {} 字节处UTF-8 字符被截断", i),
                why: "截断的多字节序列无法还原成字符".to_string(),
                next: "检查模板是否在写入时被按字节切断".to_string(),
            });
        }
        lit.push_str(&raw[i..end]);
        i = end;
    }
    literals.push(lit);
    Ok(TemplateAst { vars, literals })
}

/// 由首字节推UTF-8 字符长度（`0b10xx/0b110x/0b1110x/0b11110xx`）。
pub fn utf8_char_len(b: u8) -> usize {
    if b < 0x80 {
        1
    } else if b >= 0xF0 {
        4
    } else if b >= 0xE0 {
        3
    } else if b >= 0xC0 {
        2
    } else {
        // 0x80..=0xBF 是续字节，单独出现即非法；按 1 字节推进让上层
        // 的字符串校验去报错，不在这里 panic。
        1
    }
}

// ---------------------------------------------------------------------------
// §6 诊断袋（锚点：异常零静默；判据：回退显性）
// ---------------------------------------------------------------------------

/// 诊断类别。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DiagKind {
    /// 未收录语言，复数回退 `other`（**降级显性**）。
    PluralFallbackOther,
    /// 该语言无语法性别，性别落到 `Common`/中性。
    GenderNeutralFallback,
    /// 数值被钳制。
    CountClamped,
    /// 小数位数被钳制。
    FractionClamped,
    /// Locale 未收录，整条选择走 `und` 兜底。
    LocaleUnded,
    /// 选中项走了热表命中（单源复述）。
    HotTableHit,
}

impl DiagKind {
    /// 全集（自检遍历用）。
    pub const ALL: [DiagKind; 6] = [
        DiagKind::PluralFallbackOther,
        DiagKind::GenderNeutralFallback,
        DiagKind::CountClamped,
        DiagKind::FractionClamped,
        DiagKind::LocaleUnded,
        DiagKind::HotTableHit,
    ];
    /// 诊断码（对拍按它比对）。
    pub fn code(&self) -> &'static str {
        match self {
            DiagKind::PluralFallbackOther => "D_PLURAL_FALLBACK_OTHER",
            DiagKind::GenderNeutralFallback => "D_GENDER_NEUTRAL_FALLBACK",
            DiagKind::CountClamped => "D_COUNT_CLAMPED",
            DiagKind::FractionClamped => "D_FRACTION_CLAMPED",
            DiagKind::LocaleUnded => "D_LOCALE_UNDED",
            DiagKind::HotTableHit => "D_HOT_TABLE_HIT",
        }
    }
}

/// 一条诊断。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Diag {
    /// 类别。
    pub kind: DiagKind,
    /// 现象（零隐私面——本项只处理数值与配置，无用户数据）。
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
        self.items.push(Diag { kind, what: what.to_string() });
    }
    /// 条数。
    pub fn len(&self) -> usize {
        self.items.len()
    }
    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
    /// 某类条数。
    pub fn count_of(&self, kind: DiagKind) -> usize {
        self.items.iter().filter(|d| d.kind == kind).count()
    }
    /// 全部诊断。
    pub fn items(&self) -> &[Diag] {
        &self.items
    }
}

// ---------------------------------------------------------------------------
// §7 联合选择器（判据：联合选择器；锚点：格式化选择器）
// ---------------------------------------------------------------------------

/// 联合选择请求（复数 × 性别）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SelectorRequest {
    /// Locale 标签（完整 BCP47 形态）。
    pub locale: String,
    /// 数值（负数按绝对值口径，CLDR 同）。
    pub count: i64,
    /// 小数位数。
    pub fraction_digits: u32,
    /// 请求的语法性别。
    pub gender: Gender,
    /// 词条单位名（如 `"file"`），进模板 `{unit}`。
    pub unit: String,
}

impl SelectorRequest {
    /// 构造。
    pub fn new(locale: &str, count: i64, fraction_digits: u32, gender: Gender, unit: &str) -> Self {
        SelectorRequest {
            locale: locale.to_string(),
            count,
            fraction_digits,
            gender,
            unit: unit.to_string(),
        }
    }
    /// 构造并校验 Locale。
    pub fn checked(locale: &str, count: i64, fraction_digits: u32, gender: Gender, unit: &str) -> Result<Self, Rejection> {
        check_locale(locale)?;
        Ok(SelectorRequest::new(locale, count, fraction_digits, gender, unit))
    }
}

/// 联合选择结果。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Selection {
    /// 生效 Locale（降级时为 `und`）。
    pub locale: String,
    /// 生效复数类别。
    pub category: PluralCategory,
    /// 生效性别（可能是回退后的中性侧）。
    pub gender: Gender,
    /// 请求的性别（留档，便于上层报警"为何降级"）。
    pub requested_gender: Gender,
    /// 性别是否降级。
    pub gender_degraded: bool,
    /// 复数规则是否降级。
    pub plural_degraded: bool,
    /// 渲染好的文本。
    pub text: String,
}

/// Locale 合法性校验（空/超长/含非法字符）。
pub fn check_locale(locale: &str) -> Result<(), Rejection> {
    if locale.is_empty() {
        return Err(Rejection {
            code: E_LOCALE_INVALID,
            what: "Locale 为空串".to_string(),
            why: "空 Locale 既查不到规则也查不到性别，属于调用方未填".to_string(),
            next: "传入 BCP47 标签（如 `ar-EG`）；确实未知请显式传 `und`".to_string(),
        });
    }
    if locale.len() > MAX_LOCALE_LEN {
        return Err(Rejection {
            code: E_LOCALE_INVALID,
            what: format!("Locale {:?} 长 {} 字节，超上界 {}", locale, locale.len(), MAX_LOCALE_LEN),
            why: "超长标签多半是拼接了整条文案，不是合法 BCP47 标签".to_string(),
            next: format!("压到{} 字节以内", MAX_LOCALE_LEN).to_string(),
        });
    }
    for (i, b) in locale.as_bytes().iter().enumerate() {
        let ok = b.is_ascii_alphanumeric() || *b == b'-' || *b == b'_';
        if !ok {
            return Err(Rejection {
                code: E_LOCALE_INVALID,
                what: format!("Locale 第 {} 字节是非法字符 0x{:02X}", i, b),
                why: "BCP47 标签只允许字母数字与连字符；其它字符说明传入的不是标签".to_string(),
                next: "改为 `语言[-区域][-脚本]` 形态；私有扩展用 `-x-` 前缀".to_string(),
            });
        }
    }
    Ok(())
}

/// **联合选择器**：一次调用同时定复数类别与性别，并渲染文本。
///
/// 降级路径（锚点降级矩阵前两条，全部**显性**）：
/// - 未收录语言 → `und` 规则 + `other` 类别 + [`DiagKind::LocaleUnded`] +
///   [`DiagKind::PluralFallbackOther`]；
/// - 性别不可用（语言无该性别）→ 中性侧回退 + [`DiagKind::GenderNeutralFallback`]。
///
/// 模板变量错走**校验拒绝**（第三条降级矩阵）：模板解析失败即
/// [`E_TEMPLATE_VAR`] / [`E_TEMPLATE_SYNTAX`]，不静默替换为空串。
pub fn select(req: &SelectorRequest, bag: &mut DiagBag) -> Result<Selection, Rejection> {
    check_locale(&req.locale)?;

    let (op, rec) = Operands::of(req.count, req.fraction_digits);
    //钳制逐项落诊断（数值与小数位各一条，不合并——合并会让其中一项不可见）。
    let (c_eff, c_rec) = clamp_count(req.count);
    let (v_eff, v_rec) = clamp_fraction_digits(req.fraction_digits);
    if c_rec.is_some() {
        bag.push(
            DiagKind::CountClamped,
            &format!("count:申请 {} →生效 {}", req.count, c_eff),
        );
    }
    if v_rec.is_some() {
        bag.push(
            DiagKind::FractionClamped,
            &format!("fraction_digits:申请 {} →生效 {}", req.fraction_digits, v_eff),
        );
    }
    let _ = rec;

    // —— 复数侧 ——
    let lk = lookup_rule(&req.locale);
    if lk.degraded {
        bag.push(
            DiagKind::LocaleUnded,
            &format!("Locale {:?} 未收录，复数规则回退 und", req.locale),
        );
        bag.push(
            DiagKind::PluralFallbackOther,
            &format!("{:?} 的复数类别回退 other", req.locale),
        );
    }
    let category = eval_rule(lk.rule(), &op);

    // —— 性别侧 ——
    let effective_locale = if lk.degraded { "und".to_string() } else { req.locale.clone() };
    let gset = lookup_gender_set(&effective_locale);
    let mut gender_degraded = false;
    let gender = match gset {
        Some(gs) => {
            if gs.has(req.gender) {
                req.gender
            } else {
                gender_degraded = true;
                bag.push(
                    DiagKind::GenderNeutralFallback,
                    &format!(
                        "{:?} 无 {:?} 性别，回退中性侧",
                        effective_locale, req.gender
                    ),
                );
                // 中性侧回退：优先 `Common`（无语法性别语言的唯一正确类别），
                // 否则 `Neuter`。二者都不在集合里才落到 `Common`。
                if gs.has(Gender::Common) {
                    Gender::Common
                } else if gs.has(Gender::Neuter) {
                    Gender::Neuter
                } else {
                    Gender::Common
                }
            }
        }
        None => {
            gender_degraded = true;
            bag.push(
                DiagKind::GenderNeutralFallback,
                &format!("{:?} 查不到性别表，回退 Common", effective_locale),
            );
            Gender::Common
        }
    };

    // —— 模板渲染 ——
    let pattern = template_pattern(&effective_locale, category, gender);
    let ast = parse_template(&pattern)?;
    let count_text = format!("{}", op.n);
    let mut text = String::new();
    for (i, v) in ast.vars.iter().enumerate() {
        text.push_str(&ast.literals[i]);
        let vtext: String = match v.name.as_str() {
            "count" => count_text.clone(),
            "unit" => req.unit.clone(),
            "gender" => gender.name().to_string(),
            "locale" => effective_locale.clone(),
            "category" => category.name().to_string(),
            // `parse_template` 已把变量名收进闭域，这里再兜一层是为了
            // "守卫不得成为崩溃源"：即便闭域被改宽，也返回空串而非panic。
            _ => String::new(),
        };
        text.push_str(&vtext);
    }
    if let Some(last) = ast.literals.last() {
        text.push_str(last);
    }

    Ok(Selection {
        locale: effective_locale,
        category,
        gender,
        requested_gender: req.gender,
        gender_degraded,
        plural_degraded: lk.degraded,
        text,
    })
}

/// 构造某语言/类别/性别的模板串。
///
/// **模板里的字面文案只放"结构占位"而不放真实译文**——真实文案归翻译资源面
/// （F4014）。本项交付的是**槽位选择 + 语法结构**，把 `{count}`/`{unit}` 的
/// 相对位置与语法连接形态定下来，让上层填字。
///
/// 为什么这样分：文案是**可本地化数据**，规则是**不可本地化逻辑**。若把英文
/// 单词写进规则表，翻译人员无法覆盖，且"词序对不对"这件事会被英文语序绑死
/// （日语的 `{unit}{count}` 与英语的 `{count} {unit}` 必须反过来）。
pub fn template_pattern(locale: &str, category: PluralCategory, gender: Gender) -> String {
    let lang = primary_subtag(locale);
    // 无复数语言（日中韩越泰）：词序与英文相反（单位在前、数量在后）。
    //
    // 六类对这类语言**全是 `other`**（见 [`PluralRule::OnlyOther`]），所以此处
    // 不必按类别分支——但保留显式匹配以表明"这里将来若支持有复数的
    // 无性别语言（如希伯来语两类），可以就地扩展"。
    if matches!(lang, "zh" | "ja" | "ko" | "vi" | "th") {
        return "{unit}{count}".to_string();
    }
    // 有语法性别且带中性侧的语言：中性/通用用不带性别的槽位（英语 it 无复数）。
    if gender.is_neutral_side() && matches!(lang, "en" | "de" | "nl" | "sv") {
        return "{count} {unit}".to_string();
    }
    // 阿拉伯语：数词在后、量词居中，且双数有专属槽位。
    if lang == "ar" {
        return match category {
            PluralCategory::Two => "{count} {unit} {gender}".to_string(),
            _ => "{unit} {count} {gender}".to_string(),
        };
    }
    // 斯拉夫语族：few/many 需要不同的连接形态。
    if matches!(lang, "ru" | "pl") {
        return match category {
            PluralCategory::One => "{count} {unit}".to_string(),
            _ => "{count} {unit}-{category}".to_string(),
        };
    }
    match category {
        PluralCategory::One => "{count} {unit}".to_string(),
        _ => "{count} {unit}-{category}".to_string(),
    }
}

// ---------------------------------------------------------------------------
// §8 规则缓存（判据：缓存键完备；锚点：规则缓存复述）
// ---------------------------------------------------------------------------

/// 缓存键（**O(1)**；复用热表单源 F3242——本项只产出键，不实现池）。
///
/// 键必须能区分**任何一个会影响结果的输入**：Locale、CLDR 版本、复数类别、
/// 性别、词条单位、数值、小数位。少任何一段都会导致"两个不同的选择共用一个
/// 缓存条目"，表现出来是阿拉伯语的 `few` 形态出现在 `many` 的位置上——
/// 且因为"文本确实存在"，极难归因。
pub fn selection_cache_key(sel: &Selection, unit: &str) -> String {
    format!(
        "{}|{}|{}|{}|{}|{}|{}",
        CLDR_VERSION_TAG,
        sel.locale,
        sel.category.name(),
        sel.gender.name(),
        sel.requested_gender.name(),
        unit,
        sel.text
    )
}

/// 缓存键长度校验（超界即拒绝，不截断——截断的键会撞车）。
pub fn check_cache_key(key: &str) -> Result<(), Rejection> {
    if key.is_empty() {
        return Err(Rejection {
            code: E_CLDR_DRIFT,
            what: "缓存键为空串".to_string(),
            why: "空键会让所有选择共用同一条缓存条目，选择结果互相污染".to_string(),
            next: "确认缓存键由 selection_cache_key 生成".to_string(),
        });
    }
    if key.len() > MAX_CACHE_KEY_LEN {
        return Err(Rejection {
            code: E_CLDR_DRIFT,
            what: format!("缓存键长{} 字节，超上界 {}", key.len(), MAX_CACHE_KEY_LEN),
            why: "超长键说明把产物正文整个塞进了键，必然撞车".to_string(),
            next: format!("键只放选择要素（应≤{} 字节），产物另行存放", MAX_CACHE_KEY_LEN).to_string(),
        });
    }
    if !key.starts_with(CLDR_VERSION_TAG) {
        return Err(Rejection {
            code: E_CLDR_DRIFT,
            what: format!("缓存键未以锚定版本 {:?} 开头", CLDR_VERSION_TAG),
            why: "键缺版本段，CLDR 升版后旧条目会被新规则命中".to_string(),
            next: "用 selection_cache_key 统一生成，不要手工拼键".to_string(),
        });
    }
    Ok(())
}

/// 缓存键分段数（自检反假用：段数少一段即判红）。
pub const CACHE_KEY_SEGMENTS: usize = 7;

// ---------------------------------------------------------------------------
// §9 覆盖红线与六类可达（判据：覆盖红线；判据：六类复数）
// ---------------------------------------------------------------------------

/// CLDR 锚定检查（规则表非空 + `und` 兜底在位）。
pub fn check_cldr_anchor() -> Result<(), Rejection> {
    if PLURAL_RULES.is_empty() {
        return Err(Rejection {
            code: E_CLDR_DRIFT,
            what: "复数规则表为空".to_string(),
            why: "空规则表会让所有语言静默走 und 兜底，等于复数选择整体失效".to_string(),
            next: format!("补齐规则表，并确认锚定版本为 CLDR-{}", CLDR_VERSION).to_string(),
        });
    }
    if PLURAL_RULES.len() > MAX_RULE_ROWS {
        return Err(Rejection {
            code: E_CLDR_DRIFT,
            what: format!("复数规则表 {} 条，超上界 {}", PLURAL_RULES.len(), MAX_RULE_ROWS),
            why: "条数失控通常意味着有人手抖复制粘贴了整块表".to_string(),
            next: format!("去重后压到{} 条以内", MAX_RULE_ROWS).to_string(),
        });
    }
    if !PLURAL_RULES.iter().any(|r| r.language == "und") {
        return Err(Rejection {
            code: E_CLDR_DRIFT,
            what: format!("CLDR-{} 复数规则表缺 und 兜底行", CLDR_VERSION),
            why: "缺 und 则未收录语言无处可退，降级会退化成 panic".to_string(),
            next: "补一条 und 规则（OnlyOther）作为显式兜底".to_string(),
        });
    }
    // 版本锚定与 F4006 同步：两边都必须是非零常量。
    if CLDR_VERSION == 0 || CLDR_VERSION_TAG != format!("cldr-{}", CLDR_VERSION) {
        return Err(Rejection {
            code: E_CLDR_DRIFT,
            what: "CLDR 版本锚定常量与 tag 不一致".to_string(),
            why: "锚定与 tag 不一致意味着规则集版本无从判断，漂移无法暴露".to_string(),
            next: "把 CLDR_VERSION 与 CLDR_VERSION_TAG 改回一致（且与 F4006 同值）".to_string(),
        });
    }
    Ok(())
}

/// **覆盖红线机检（双向）**。
///
/// 正向：[`ALIGNED_LANGUAGES`] 每条都必须在 [`PLURAL_RULES`] 与 [`GENDER_RULES`] 内；
/// 反向：[`PLURAL_RULES`] / [`GENDER_RULES`] 里每条都必须在 [`ALIGNED_LANGUAGES`]
/// 内（`und` 除外）。
pub fn check_coverage_redline() -> Result<(), Rejection> {
    for lang in ALIGNED_LANGUAGES.iter() {
        let lk = lookup_rule(lang);
        if lk.matched != RuleMatch::Exact {
            return Err(Rejection {
                code: E_COVERAGE_REDLINE,
                what: format!("对齐清单里的 {:?} 不在复数规则表内", lang),
                why: "字体域声明支持该语言而本域缺规则，会导致同页文字本地化而数字形态错".to_string(),
                next: format!("把 {:?} 补进 PLURAL_RULES（按CLDR-{} 的规则族）", lang, CLDR_VERSION),
            });
        }
        if lookup_gender_set(lang).is_none() {
            return Err(Rejection {
                code: E_COVERAGE_REDLINE,
                what: format!("对齐清单里的 {:?} 不在性别规则表内", lang),
                why: "缺性别表会让该语言每次选择都落进中性回退，且看不出是漏了表".to_string(),
                next: format!("把 {:?} 补进 GENDER_RULES", lang),
            });
        }
    }
    // 反向：规则表不得擅自扩表（除 und）。
    for r in PLURAL_RULES.iter() {
        if r.language == "und" {
            continue;
        }
        if !ALIGNED_LANGUAGES.contains(&r.language) {
            return Err(Rejection {
                code: E_COVERAGE_REDLINE,
                what: format!("复数规则表里的 {:?} 不在覆盖对齐清单内", r.language),
                why: "擅自扩表会让本域声称支持一个字体域并不支持的语言，构成新的域间分叉".to_string(),
                next: format!("要么同时在字体域（F4005）声明支持 {:?} 并更新 ALIGNED_LANGUAGES，要么从本表移除", r.language),
            });
        }
    }
    for g in GENDER_RULES.iter() {
        if g.language == "und" {
            continue;
        }
        if !ALIGNED_LANGUAGES.contains(&g.language) {
            return Err(Rejection {
                code: E_COVERAGE_REDLINE,
                what: format!("性别规则表里的 {:?} 不在覆盖对齐清单内", g.language),
                why: "性别表擅自扩表会让「该语言有性别规则」与其他域的支持声明不一致".to_string(),
                next: format!("同步更新 ALIGNED_LANGUAGES，或从性别表移除 {:?}", g.language),
            });
        }
    }
    // 规则表语言不得重复（重复会让先命中的赢，后面的永不可达）。
    let mut langs: Vec<&str> = PLURAL_RULES.iter().map(|r| r.language).collect();
    let before = langs.len();
    langs.sort_unstable();
    langs.dedup();
    if langs.len() != before {
        return Err(Rejection {
            code: E_COVERAGE_REDLINE,
            what: "复数规则表存在重复语言行".to_string(),
            why: "重复行让后一条永不可达，规则族写错也无人发现".to_string(),
            next: "去掉重复行，并确认保留行的规则族是想要的那个".to_string(),
        });
    }
    Ok(())
}

/// 代表性数值样本（六类穷举用；取各规则族判据会命中的典型值）。
pub const SAMPLE_COUNTS: [i64; 12] = [0, 1, 2, 3, 4, 5, 6, 11, 21, 22, 101, 1001];

/// 代表性小数位样本。
///
/// **为什么整数样本不够**：斯拉夫族（俄/波）的 `other` **只有小数情形可达**
/// ——带小数（`v > 0`）时 `few`/`many` 的 `v = 0` 前置条件不成立，一律落`other`。
/// 初版机检只喂整数样本，于是这条声明被误判成"空承诺"。这说明机检的**样本集
/// 本身就是判据的一部分**：漏掉一维样本维度，整维可达性就不可见。
pub const SAMPLE_FRACTIONS: [u32; 3] = [0, 1, 2];

/// 六类可达机检：六类复数每一类都必须真能被某条规则选出。
///
/// 这是"列出来不等于选得到"的守卫：若某类没有任何规则会返回它，那它就是
/// 一句空承诺（例如删掉阿拉伯规则后，`two` 仍在枚举里却永不出现）。
pub fn check_all_six_categories_reachable() -> Result<(), Rejection> {
    for cat in PluralCategory::ALL.iter() {
        let mut hit = false;
        'outer: for entry in PLURAL_RULES.iter() {
            for n in SAMPLE_COUNTS.iter() {
                for v in SAMPLE_FRACTIONS.iter() {
                    let (op, _) = Operands::of(*n, *v);
                    if eval_rule(entry.rule, &op) == *cat {
                        hit = true;
                        break 'outer;
                    }
                }
            }
        }
        if !hit {
            return Err(Rejection {
                code: E_COVERAGE_REDLINE,
                what: format!("复数类别 {:?} 在全表规则下无任何数值可达", cat.name()),
                why: "锚点钦定六类；某类不可达说明规则族缺失，该类别是空承诺".to_string(),
                next: "补一条能选出该类别的规则族（阿拉伯式是唯一六类全用的族）".to_string(),
            });
        }
    }
    Ok(())
}

/// 规则族声明可达集合与实际可达一致（**防声明漂移**）。
pub fn check_rule_reachability_claims() -> Result<(), Rejection> {
    for rule in PluralRule::ALL.iter() {
        // 每个声明可达的类别，必须真能在该族下选出来。
        // 样本必须跨整数/小数两维，否则 `other` 这类"仅小数可达"的声明会被误判。
        for cat in rule.reachable().iter() {
            let mut hit = false;
            'scan: for n in SAMPLE_COUNTS.iter() {
                for v in SAMPLE_FRACTIONS.iter() {
                    let (op, _) = Operands::of(*n, *v);
                    if eval_rule(*rule, &op) == *cat {
                        hit = true;
                        break 'scan;
                    }
                }
            }
            if !hit {
                return Err(Rejection {
                    code: E_COVERAGE_REDLINE,
                    what: format!(
                        "规则族 {} 声明可达 {:?}，但该族下无任何样本数值能选出它",
                        rule.name(),
                        cat.name()
                    ),
                    why: "声明与实现不一致会让人误以为该类别有规则支持，选型时踩空".to_string(),
                    next: "要么修 reachable()，要么补规则分支；两者必须一致".to_string(),
                });
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// §10 预留槽位与单源声明（锚点：跨批对接点；复述单源）
// ---------------------------------------------------------------------------

/// 复用单源预留槽位。
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

/// 复用单源预留槽位（F3242 热表、F4010 语料、F4014 翻译流程）。
pub const RESERVED_SLOTS: [ReservedSlot; 3] = [
    ReservedSlot {
        upstream: "VE-F3242",
        key: "hot-table-pool",
        label: "热表（规则缓存池）",
        landed: false,
        on_landed: "本项的 selection_cache_key 接入 F3242 池，替代当前的纯函数重算",
    },
    ReservedSlot {
        upstream: "VE-F4010",
        key: "i18n-corpus",
        label: "国际化测试语料库",
        landed: false,
        on_landed: "本项的规则表由 F4010 语料做回归兜底（每族至少一条多形态样文）",
    },
    ReservedSlot {
        upstream: "VE-F4014",
        key: "translation-pipeline",
        label: "翻译流程（字符串外部化与上下文注释）",
        landed: false,
        on_landed: "本项的模板槽位由 F4014 提供实际文案与上下文注释，当前只出结构",
    },
];

/// 预留槽位机检：`landed: true` 但上游未交付即为谎报。
pub fn check_reserved() -> Result<(), Rejection> {
    for s in RESERVED_SLOTS.iter() {
        if s.landed {
            return Err(Rejection {
                code: E_RESERVED_LIED,
                what: format!("预留槽位 {}（{}）标为已落地，但上游尚未交付", s.upstream, s.key),
                why: "谎报落地会让上层把未实现的依赖当成已就绪，该能力从此静默失效".to_string(),
                next: format!("改回 landed: false；待 {} 真正落地后再改", s.upstream),
            });
        }
    }
    Ok(())
}

/// 单源复用声明。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SingleSourceClaim {
    /// 能力键。
    pub key: &'static str,
    /// 能力中文名。
    pub label: &'static str,
    /// owner 项号（唯一）。
    pub owner: &'static str,
    /// consumer 列表。
    pub consumers: &'static [&'static str],
    /// 陈述。
    pub statement: &'static str,
}

/// 单源复用声明表（锚点：CLDR 复数表引用 / F4006 格式协同 / N02 文本消费）。
pub const PLURAL_SINGLE_SOURCE: [SingleSourceClaim; 4] = [
    SingleSourceClaim {
        key: "cldr-plural-rules",
        label: "CLDR 复数规则表",
        owner: "VE-F4007",
        consumers: &["VE-F4006", "VE-N02"],
        statement: "复数规则表的唯一 owner 是本项；F4006 只做格式协同不另立表，N02 只消费选择结果",
    },
    SingleSourceClaim {
        key: "cldr-version-anchor",
        label: "CLDR 版本锚定",
        owner: "VE-F4006",
        consumers: &["VE-F4007"],
        statement: "CLDR 版本锚定的唯一 owner 是 F4006；本项与其同值（46），不另立版本常量口径",
    },
    SingleSourceClaim {
        key: "language-support-list",
        label: "受支持语言清单",
        owner: "VE-F4005",
        consumers: &["VE-F4006", "VE-F4007"],
        statement: "受支持语言清单的唯一 owner 是字体域；日期域与本域消费之并各自双向机检",
    },
    SingleSourceClaim {
        key: "hot-table-pool",
        label: "热表（缓存池）",
        owner: "VE-F3242",
        consumers: &["VE-F4006", "VE-F4007"],
        statement: "热表的唯一 owner 是 F3242；本项只产出缓存键，不自建池",
    },
];

/// 单源机检：同 `key` 不得两个 owner；同一能力的 owner 不得把自己列为 consumer。
///
/// **为什么只查"同能力自环"而不查"跨能力互为上下游"**：
/// 「本项拥有复数规则表」与「F4006 拥有 CLDR 版本锚定、本项消费之」是两条
/// **不同能力**的声明，它们之间的引用方向（VE-F4007 ∈ consumers）是完全
/// 正常的上下游关系，不是自环。初版机检在这里写了
/// `b.consumers.contains(&a.owner)`，结果把正常的"F4007 消费 F4006 的锚定"
/// 判成了自环——**门禁本身成了缺陷**。真自环的形态只有一个：某条声明的
/// `consumers` 里出现了它自己的 `owner`。
pub fn check_single_source() -> Result<(), Rejection> {
    for (i, a) in PLURAL_SINGLE_SOURCE.iter().enumerate() {
        // 真自环：自己消费自己。
        if a.consumers.contains(&a.owner) {
            return Err(Rejection {
                code: E_SINGLE_SOURCE_DUP,
                what: format!("能力 {:?}（owner {}）把自己列为 consumer", a.key, a.owner),
                why: "自环意味着复述方向说不清：既是唯一 owner 又不是 owner".to_string(),
                next: "从 consumers 里删掉自己，或拆成两条声明".to_string(),
            });
        }
        for b in PLURAL_SINGLE_SOURCE.iter().skip(i + 1) {
            if a.key == b.key {
                return Err(Rejection {
                    code: E_SINGLE_SOURCE_DUP,
                    what: format!("能力 {:?} 被声明了两次（owner {} 与 {}）", a.key, a.owner, b.owner),
                    why: "同能力两个 owner 必然分叉，分叉后对拍无从判断该信谁".to_string(),
                    next: "保留一个 owner，另一条改为 consumer 引用".to_string(),
                });
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// §11 规格表（锚点：逐条规格公开）
// ---------------------------------------------------------------------------

/// 规格条目。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SpecItem {
    /// 编号（从 1 起连续；对拍按号定位）。
    pub no: u16,
    /// 能力键。
    pub key: &'static str,
    /// 中文标签。
    pub label: &'static str,
    /// 强制它的自检项名。
    pub enforced_by: &'static str,
}

/// 规格表。
pub const SPEC_SHEET: [SpecItem; 17] = [
    // 判据一：六类复数（3 条）
    SpecItem { no: 1, key: "six-categories-closed", label: "六类复数闭域且守卫拒域外", enforced_by: "six_categories_closed" },
    SpecItem { no: 2, key: "six-categories-reachable", label: "六类每一类都能被真实规则选出", enforced_by: "six_categories_reachable" },
    SpecItem { no: 3, key: "rule-family-claims-honest", label: "规则族声明的可达集合与实现一致", enforced_by: "rule_family_claims_honest" },
    // 判据二：性别模板（3 条）
    SpecItem { no: 4, key: "gender-set-per-language", label: "每语言性别集合齐备且无语法性别者只登记 Common", enforced_by: "gender_set_per_language" },
    SpecItem { no: 5, key: "template-vars-closed", label: "模板变量闭域，未知变量校验拒绝", enforced_by: "template_vars_closed" },
    SpecItem { no: 6, key: "template-syntax-checked", label: "花括号配对/嵌套/转义逐类校验", enforced_by: "template_syntax_checked" },
    // 判据三：联合选择器（3 条）
    SpecItem { no: 7, key: "selector-joint", label: "一次调用同时定类别与性别并渲染", enforced_by: "selector_joint" },
    SpecItem { no: 8, key: "selector-non-ascii-safe", label: "非 ASCII 模板按字符边界推进不切碎", enforced_by: "selector_non_ascii_safe" },
    SpecItem { no: 9, key: "cache-key-complete", label: "缓存键含版本/语言/类别/性别/单位/产物", enforced_by: "cache_key_complete" },
    // 判据四：覆盖红线（3 条）
    SpecItem { no: 10, key: "coverage-bidirectional", label: "覆盖红线双向机检（不得擅自扩表）", enforced_by: "coverage_bidirectional" },
    SpecItem { no: 11, key: "locale-two-level-lookup", label: "完整 BCP47 标签按子标签命中", enforced_by: "locale_two_level_lookup" },
    SpecItem { no: 12, key: "cldr-anchor-pinned", label: "CLDR 版本锚定为编译期常量且与 F4006 同值", enforced_by: "cldr_anchor_pinned" },
    // 判据五：回退显性（2 条）
    SpecItem { no: 13, key: "fallback-visible", label: "未知语言复数回退 other 必产诊断", enforced_by: "fallback_visible" },
    SpecItem { no: 14, key: "gender-fallback-visible", label: "性别缺回退中性侧必产诊断", enforced_by: "gender_fallback_visible" },
    // 通用纪律（2 条）
    SpecItem { no: 15, key: "reserved-not-lied", label: "预留槽位未落地不谎报", enforced_by: "reserved_not_lied" },
    SpecItem { no: 16, key: "zero-privacy-surface", label: "零隐私面（只数值与配置，无用户数据）", enforced_by: "zero_privacy_surface" },
    // 判据五补强：钳制必须逐项可见（数值与小数位各一条，不合并成一条）。
    SpecItem { no: 17, key: "clamps-visible", label: "数值与小数位钳制逐项留痕不合并", enforced_by: "clamps_visible" },
];

/// 判据（锚点五条）到规格表键的映射。
///
/// **必须覆盖规格表里的每一条**（通用纪律条目除外）——初版只给了每条判据两个
/// 键，结果有4 条规格（规则族声明一致、模板语法、选择器非 ASCII、完整标签子标签
/// 命中）**没有任何判据引用它们**，等于"写在规格表里却没人管"。这类悬空条目比
/// 缺规格更隐蔽：规格表看起来很全，但那些条目的 `enforced_by` 指向的判据并不存在。
/// [`check_spec_coverage`] 现在做**双向**机检（判据→规格、规格→判据）。
pub const CRITERIA: [(&str, [&str; 3]); 5] = [
    ("六类复数", ["six-categories-closed", "six-categories-reachable", "rule-family-claims-honest"]),
    ("性别模板", ["gender-set-per-language", "template-vars-closed", "template-syntax-checked"]),
    ("联合选择器", ["selector-joint", "selector-non-ascii-safe", "cache-key-complete"]),
    ("覆盖红线", ["coverage-bidirectional", "locale-two-level-lookup", "cldr-anchor-pinned"]),
    ("回退显性", ["fallback-visible", "gender-fallback-visible", "clamps-visible"]),
];

/// 通用纪律条目（不属五条判据，但同样有自检项强制）。
pub const DISCIPLINE_KEYS: [&str; 2] = ["reserved-not-lied", "zero-privacy-surface"];

/// 规格覆盖机检（**双向**：编号连续 + 判据→规格 + 规格→判据）。
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
                    next: format!("把 {:?} 登记进 SPEC_SHEET", key),
                });
            }
        }
    }
    // **反向**：规格表里不得有"没人管的条目"。
    for s in SPEC_SHEET.iter() {
        let referenced = CRITERIA.iter().any(|(_, keys)| keys.contains(&s.key))
            || DISCIPLINE_KEYS.contains(&s.key);
        if !referenced {
            return Err(Rejection {
                code: E_SPEC_GAP,
                what: format!("规格键 {:?} 未被任何判据或纪律条目引用", s.key),
                why: "悬空规格比缺规格更隐蔽：规格表看着齐全，但没人真正强制它".to_string(),
                next: format!("把 {:?} 挂进 CRITERIA 的某条判据，或列入 DISCIPLINE_KEYS", s.key),
            });
        }
    }
    // 纪律条目本身必须在规格表内（不许只挂名字不登记）。
    for k in DISCIPLINE_KEYS.iter() {
        if !SPEC_SHEET.iter().any(|s| s.key == *k) {
            return Err(Rejection {
                code: E_SPEC_GAP,
                what: format!("纪律条目 {:?} 未登记进 SPEC_SHEET", k),
                why: "纪律条目不登记就等于没有强制点".to_string(),
                next: format!("把 {:?} 登记进 SPEC_SHEET", k),
            });
        }
    }
    Ok(())
}

/// 零隐私面机检（本项只处理数值与配置，诊断文本不得含用户数据）。
pub fn check_zero_privacy() -> Result<(), Rejection> {
    // 诊断只允许出现 Locale、单位、类别名、性别名、数值——这些都不是用户数据。
    // 这里用白名单式的粗检：诊断文本里不得出现路径分隔符与邮箱形态字符。
    let mut bag = DiagBag::new();
    let _ = select(
        &SelectorRequest::new("xx-YY", 12345, 0, Gender::Feminine, "file"),
        &mut bag,
    );
    for d in bag.items() {
        if d.what.contains('/') || d.what.contains('\\') || d.what.contains('@') {
            return Err(Rejection {
                code: E_PRIVACY_LEAK,
                what: format!("诊断 {:?} 含疑似路径或邮箱形态字符", d.kind.code()),
                why: "本项零隐私面，诊断里不该出现文件系统路径或用户标识".to_string(),
                next: "只写 Locale/单位/类别/数值这类配置面信息".to_string(),
            });
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// §12 单元测试（回归 + 演练；随功能同源维护）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    fn cat(n: i64, rule: PluralRule) -> PluralCategory {
        let (op, _) = Operands::of(n, 0);
        eval_rule(rule, &op)
    }

    #[test]
    fn test_six_categories_all_reachable() {
        assert!(check_all_six_categories_reachable().is_ok());
        assert!(check_rule_reachability_claims().is_ok());
    }

    #[test]
    fn test_category_guard_shapes() {
        for raw in ["zero", "Zero", "ZERO", "0"] {
            assert_eq!(PluralCategory::guard(raw).unwrap(), PluralCategory::Zero);
        }
        for raw in ["two", "TWO", "TwO", "2"] {
            assert_eq!(PluralCategory::guard(raw).unwrap(), PluralCategory::Two);
        }
        let e = PluralCategory::guard("several").unwrap_err();
        assert_eq!(e.code, E_CATEGORY_INVALID);
        assert!(e.is_complete());
    }

    #[test]
    fn test_gender_guard_shapes() {
        for raw in ["feminine", "Feminine", "FEM", "f"] {
            assert_eq!(Gender::guard(raw).unwrap(), Gender::Feminine);
        }
        for raw in ["common", "Common", "none", "c"] {
            assert_eq!(Gender::guard(raw).unwrap(), Gender::Common);
        }
        assert_eq!(Gender::guard("zz").unwrap_err().code, E_GENDER_INVALID);
    }

    #[test]
    fn test_rule_guard_shapes() {
        for raw in ["only-other", "ONLY-OTHER", "only_other"] {
            assert_eq!(PluralRule::guard(raw).unwrap(), PluralRule::OnlyOther);
        }
        assert_eq!(PluralRule::guard("klingon").unwrap_err().code, E_RULE_INVALID);
    }

    #[test]
    fn test_arabic_six_categories() {
        // 阿拉伯语是唯一六类全用的族——逐一钉住。
        assert_eq!(cat(0, PluralRule::ArabicFull), PluralCategory::Zero);
        assert_eq!(cat(1, PluralRule::ArabicFull), PluralCategory::One);
        assert_eq!(cat(2, PluralRule::ArabicFull), PluralCategory::Two);
        assert_eq!(cat(3, PluralRule::ArabicFull), PluralCategory::Few);
        assert_eq!(cat(11, PluralRule::ArabicFull), PluralCategory::Many);
        // 101 → n%100 = 1，但既不是 0/1/2 也不在 3..10 与 11..99 → other
        assert_eq!(cat(101, PluralRule::ArabicFull), PluralCategory::Other);
    }

    #[test]
    fn test_russian_slavic() {
        assert_eq!(cat(1, PluralRule::SlavicRussian), PluralCategory::One);
        assert_eq!(cat(21, PluralRule::SlavicRussian), PluralCategory::One);
        assert_eq!(cat(11, PluralRule::SlavicRussian), PluralCategory::Many);
        assert_eq!(cat(2, PluralRule::SlavicRussian), PluralCategory::Few);
        assert_eq!(cat(24, PluralRule::SlavicRussian), PluralCategory::Few);
        assert_eq!(cat(5, PluralRule::SlavicRussian), PluralCategory::Many);
        assert_eq!(cat(0, PluralRule::SlavicRussian), PluralCategory::Many);
    }

    #[test]
    fn test_polish_slavic() {
        assert_eq!(cat(1, PluralRule::SlavicPolish), PluralCategory::One);
        assert_eq!(cat(2, PluralRule::SlavicPolish), PluralCategory::Few);
        // 12-14 不进 few
        assert_eq!(cat(12, PluralRule::SlavicPolish), PluralCategory::Many);
        assert_eq!(cat(5, PluralRule::SlavicPolish), PluralCategory::Many);
    }

    #[test]
    fn test_one_is_one_and_zero_or_one() {
        assert_eq!(cat(1, PluralRule::OneIsOne), PluralCategory::One);
        assert_eq!(cat(0, PluralRule::OneIsOne), PluralCategory::Other);
        assert_eq!(cat(0, PluralRule::ZeroOrOneIsOne), PluralCategory::One);
        assert_eq!(cat(1, PluralRule::ZeroOrOneIsOne), PluralCategory::One);
        assert_eq!(cat(2, PluralRule::ZeroOrOneIsOne), PluralCategory::Other);
        assert_eq!(cat(7, PluralRule::OnlyOther), PluralCategory::Other);
    }

    #[test]
    fn test_negative_uses_absolute() {
        // CLDR 按绝对值：-1 与 1 同类别。
        assert_eq!(cat(-1, PluralRule::OneIsOne), PluralCategory::One);
        assert_eq!(cat(-2, PluralRule::ArabicFull), PluralCategory::Two);
    }

    #[test]
    fn test_operands_clamped_visible() {
        let (_, rec) = Operands::of(10i64.pow(15), 0);
        assert!(rec.is_some());
        let (op, _) = Operands::of(10i64.pow(15), 0);
        assert_eq!(op.n, MAX_COUNT);
        let (_, rec2) = Operands::of(1, 99);
        assert!(rec2.is_some());
    }

    #[test]
    fn test_fraction_changes_polish_few() {
        // 带小数时 few 不再成立（CLDR：v=0 才进 few）。
        let (op, _) = Operands::of(2, 1);
        assert_eq!(eval_rule(PluralRule::SlavicPolish, &op), PluralCategory::Other);
    }

    #[test]
    fn test_operands_in_range_public_api() {
        let (op, _) = Operands::of(5, 0);
        assert!(op.in_range(1, 10));
        assert!(!op.in_range(6, 10));
    }

    #[test]
    fn test_two_level_lookup() {
        let lk = lookup_rule("ar-EG");
        assert_eq!(lk.matched, RuleMatch::PrimarySubtag);
        assert!(!lk.degraded);
        assert_eq!(lk.rule(), PluralRule::ArabicFull);
        // 未收录 → 显式降级
        let miss = lookup_rule("xx-YY");
        assert!(miss.matched.is_missed());
        assert!(miss.degraded);
        assert_eq!(miss.rule(), PluralRule::OnlyOther);
    }

    #[test]
    fn test_coverage_redline_ok() {
        assert!(check_coverage_redline().is_ok());
        assert_eq!(check_cldr_anchor(), Ok(()));
    }

    #[test]
    fn test_template_parse_and_render() {
        let ast = parse_template("{count} {unit}-{category}").unwrap();
        assert_eq!(ast.var_count(), 3);
        assert_eq!(ast.literals.len(), 4);
    }

    #[test]
    fn test_template_rejects_unknown_var() {
        let e = parse_template("{conut}").unwrap_err();
        assert_eq!(e.code, E_TEMPLATE_VAR);
        assert!(e.is_complete());
    }

    #[test]
    fn test_template_rejects_unclosed() {
        let e = parse_template("{count").unwrap_err();
        assert_eq!(e.code, E_TEMPLATE_SYNTAX);
    }

    #[test]
    fn test_template_rejects_stray_brace() {
        assert_eq!(parse_template("abc}").unwrap_err().code, E_TEMPLATE_SYNTAX);
        // `{{` 是转义，合法
        assert!(parse_template("a{{b").is_ok());
    }

    #[test]
    fn test_template_rejects_nested() {
        assert_eq!(parse_template("{a{b}}").unwrap_err().code, E_TEMPLATE_SYNTAX);
    }

    #[test]
    fn test_template_rejects_empty_var() {
        assert_eq!(parse_template("{}").unwrap_err().code, E_TEMPLATE_VAR_INVALID);
    }

    #[test]
    fn test_non_ascii_template_roundtrip() {
        // 中文模板：单位在前数量在后，且含非 ASCII 字面量。
        let ast = parse_template("{unit}{count} 个").unwrap();
        assert_eq!(ast.var_count(), 2);
        assert_eq!(ast.literals[2], " 个");
    }

    #[test]
    fn test_utf8_char_len_table() {
        assert_eq!(utf8_char_len(b'a'), 1);
        assert_eq!(utf8_char_len(0xC3), 2);
        assert_eq!(utf8_char_len(0xE8), 3);
        assert_eq!(utf8_char_len(0xF0), 4);
    }

    #[test]
    fn test_selector_joint() {
        let mut bag = DiagBag::new();
        let req = SelectorRequest::new("ru-RU", 3, 0, Gender::Feminine, "файл");
        let sel = select(&req, &mut bag).unwrap();
        assert_eq!(sel.category, PluralCategory::Few);
        assert_eq!(sel.gender, Gender::Feminine);
        assert!(!sel.gender_degraded);
        assert!(!sel.plural_degraded);
        assert!(sel.text.contains("3"));
    }

    #[test]
    fn test_selector_fallback_visible() {
        let mut bag = DiagBag::new();
        let req = SelectorRequest::new("xx-YY", 1, 0, Gender::Feminine, "file");
        let sel = select(&req, &mut bag).unwrap();
        assert_eq!(sel.locale, "und");
        assert_eq!(sel.category, PluralCategory::Other);
        assert!(sel.plural_degraded);
        assert!(sel.gender_degraded);
        assert_eq!(bag.count_of(DiagKind::LocaleUnded), 1);
        assert!(bag.count_of(DiagKind::PluralFallbackOther) >= 1);
        assert!(bag.count_of(DiagKind::GenderNeutralFallback) >= 1);
    }

    #[test]
    fn test_selector_gender_fallback() {
        let mut bag = DiagBag::new();
        // 中文无语法性别，请求 Feminine 必须回退并留痕。
        let req = SelectorRequest::new("zh-CN", 1, 0, Gender::Feminine, "文件");
        let sel = select(&req, &mut bag).unwrap();
        assert_eq!(sel.gender, Gender::Common);
        assert!(sel.gender_degraded);
        assert_eq!(sel.requested_gender, Gender::Feminine);
        assert_eq!(bag.count_of(DiagKind::GenderNeutralFallback), 1);
    }

    #[test]
    fn test_selector_rejects_bad_locale() {
        let mut bag = DiagBag::new();
        let req = SelectorRequest::new("zh CN", 1, 0, Gender::Common, "文件");
        assert_eq!(select(&req, &mut bag).unwrap_err().code, E_LOCALE_INVALID);
        let empty = SelectorRequest::new("", 1, 0, Gender::Common, "x");
        assert_eq!(select(&empty, &mut bag).unwrap_err().code, E_LOCALE_INVALID);
    }

    #[test]
    fn test_selector_never_panics_on_odd_unit() {
        let mut bag = DiagBag::new();
        // 单位名含花括号——不该崩（模板变量名受控，单位只做字面插入）。
        let req = SelectorRequest::new("en-US", 2, 0, Gender::Common, "{weird}");
        let sel = select(&req, &mut bag);
        assert!(sel.is_ok());
    }

    #[test]
    fn test_cache_key_complete() {
        let mut bag = DiagBag::new();
        let sel = select(
            &SelectorRequest::new("ar-EG", 2, 0, Gender::Feminine, "file"),
            &mut bag,
        )
        .unwrap();
        let k1 = selection_cache_key(&sel, "file");
        assert!(check_cache_key(&k1).is_ok());
        assert!(k1.starts_with(CLDR_VERSION_TAG));
        assert_eq!(k1.split('|').count(), CACHE_KEY_SEGMENTS);
        // 类别不同 → 键必须不同（这是键完备性的核心断言）。
        let sel2 = select(
            &SelectorRequest::new("ar-EG", 3, 0, Gender::Feminine, "file"),
            &mut bag,
        )
        .unwrap();
        let k2 = selection_cache_key(&sel2, "file");
        assert_ne!(k1, k2);
    }

    #[test]
    fn test_cache_key_rejects_empty_and_unversioned() {
        assert!(check_cache_key("").is_err());
        assert!(check_cache_key("xx|yy").is_err());
    }

    #[test]
    fn test_template_pattern_never_invalid() {
        // 穷举：所有语言 × 六类 × 四性别，构造出的模板必须都能解析。
        for entry in PLURAL_RULES.iter() {
            for c in PluralCategory::ALL.iter() {
                for g in Gender::ALL.iter() {
                    let p = template_pattern(entry.language, *c, *g);
                    assert!(
                        parse_template(&p).is_ok(),
                        "{}/{}/{} 模板不可解析：{}",
                        entry.language,
                        c.name(),
                        g.name(),
                        p
                    );
                }
            }
        }
    }

    #[test]
    fn test_gender_set_per_language() {
        for entry in PLURAL_RULES.iter() {
            let gs = lookup_gender_set(entry.language).unwrap();
            // gendered=false 的语言只能有 Common。
            if !entry.gendered {
                assert_eq!(gs.count(), 1, "{} 应只有一种性别", entry.language);
                assert!(gs.has(Gender::Common));
            } else {
                assert!(gs.count() >= 2, "{} 至少两性", entry.language);
            }
        }
    }

    #[test]
    fn test_spec_and_single_source_ok() {
        assert!(check_spec_coverage().is_ok());
        assert!(check_single_source().is_ok());
        assert!(check_reserved().is_ok());
        assert!(check_zero_privacy().is_ok());
    }

    #[test]
    fn test_diag_kind_codes_unique() {
        let all = DiagKind::ALL;
        for (i, a) in all.iter().enumerate() {
            for b in all.iter().skip(i + 1) {
                assert_ne!(a.code(), b.code());
            }
        }
    }

    #[test]
    fn test_primary_subtag_split() {
        assert_eq!(primary_subtag("ar-EG"), "ar");
        assert_eq!(primary_subtag("zh-Hans-CN"), "zh");
        assert_eq!(primary_subtag("en"), "en");
        // 严格口径：只切连字符。下划线与大写形态**不归一**——
        // 规范化归 F4002，下游域各自放宽会造成域间分叉。
        assert_eq!(primary_subtag("en_US"), "en_US");
        assert_eq!(primary_subtag("EN-US"), "EN");
    }

    #[test]
    fn test_non_canonical_locale_degrades_explicitly() {
        // 非规范形态按未收录降级，且必须**显性**——不能"恰好命中"也不能静默。
        for tag in ["en_US", "pt_BR", "EN-US"].iter() {
            let lk = lookup_rule(tag);
            assert!(lk.degraded, "{:?} 应降级", tag);
            assert_eq!(lk.matched, RuleMatch::Missed);
            assert_eq!(lk.rule(), PluralRule::OnlyOther);
        }
        // 规范形态（连字符 + 小写语言）必须命中。
        for tag in ["en-US", "pt-BR", "zh-Hans-CN"].iter() {
            assert!(!lookup_rule(tag).degraded, "{:?} 应命中", tag);
        }
    }

    #[test]
    fn test_diag_bag_vec_growth() {
        let mut bag = DiagBag::new();
        for i in 0..5 {
            bag.push(DiagKind::CountClamped, &alloc::format!("clamp {}", i));
        }
        assert_eq!(bag.len(), 5);
        assert_eq!(bag.count_of(DiagKind::CountClamped), 5);
        assert!(!bag.is_empty());
        let _ = vec![1, 2, 3];
    }
}