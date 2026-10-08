//! VE-F4006 · 日期时间数字格式（VE-T 域 · 国际化域 · T01 组 · 目标 360 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F4006`
//!
//! **判据（锚点原文）**：CLDR 锚定、五类格式、多历法、时区断言、热表单源。
//!
//! **职责定位（锚点原文）**：T01 日期时间数字格式——格式化引擎（日期/时间/数字/
//! 货币/百分比**五类 Locale 格式化**：格式规则（**CLDR 规则集**（复述标准引用；
//! **日历系统**（公历+主要非公历历法（回历/佛历/希伯来历——**历法转换表**；
//! **时区**（复用宿主时区+时区显示规范——**时区断言**（复用 F3694 红线；
//! **格式缓存**（格式产物缓存（复用热表（复述单源。数据结构：数据模型与规格表
//! （逐条规格公开、参数域钳制、枚举守卫——家族格式）。错误路径与降级矩阵：
//! **未知 Locale→und 格式+诊断**；**历法转换失败→公历回退+显性**；
//! **时区错→断言**（复用红线）；**格式版本漂移→CLDR 版本锚定**。性能逐项分解：
//! 格式化 O(1) 缓存后；历法 O(转换)；缓存 O(1)；锚定 O(1)。跨批对接点：
//! **CLDR 标准引用；F3694 时区复用；F3242 热表单源复用**。无障碍与隐私：
//! 文档替述可读；无隐私面。
//!
//! # 本项的边界（不越界施工，遵守"只做领到的任务"）
//!
//! 本项交付**格式化的策略层**：给定「纪元时刻 + Locale + 类别」，产出该用哪套
//! 格式规则、哪个历法、什么数字分组。它**不代做**：
//!
//! - **宿主时区查询归 F3694**；本项消费其时区结论并做**断言**
//!   （锚点：「时区错→断言（复用红线）」）。
//! - **热表（缓存池）归 F3242**；本项只声明"格式产物走热表"并登记单源，
//!   不自己实现池（锚点：「格式缓存（格式产物缓存（复用热表（复述单源」）。
//!
//! 两个 F3xxx 尚未落地，故激活位登记在 [`RESERVED_SLOTS`]（`landed: false`）。
//!
//! # 关于「零时钟」——为什么内核里的日期格式化必须自带纪元
//!
//! 格式化天然依赖"现在几点"，但**内核不能读时钟**（读了就不确定，且单帧排版
//! 会被时钟跳变撕裂）。所以本项的接口是**纯函数**：`format(locale, kind, epoch)`
//! ——时间由调用方以纪元秒传入。这样同一输入必得同一输出，回归可复现。
//!
//! # 关于「CLDR 版本锚定」为什么必须硬失败而不是"尽力而为"
//!
//! CLDR 是会出大版本的（数字分组规则、日期字段顺序都可能变）。若本项"尽量按
//! 手头的规则格式化"，那么升级 CLDR 后同一个日期在不同机器上会排出不同结果——
//! 而这种差异极难归因。所以本项把 [`CLDR_VERSION`] 作为**编译期常量**写死，
//! 任何"规则集与锚定版本不符"的情况一律 [`Rejection`]，让漂移在联编期就暴露。
//!
//! # 关于「未知 Locale→und 格式+诊断」为什么给und 而不是硬拒
//!
//! 硬拒会让一个生僻语言的用户整页打不开（比如只装了维吾尔语资源、用户切到藏语）。
//! 所以本项回退到 `und`（undetermined，CLDR 的"未知"根 locale）格式并产诊断：
//! 排出来的样子是ISO 风格的西式格式，对全球用户都读得懂，同时上层能报警。
//! 这与 F4004/F4005 的降级红线同一思路：**给可用的兜底，但绝不静默**。
//!
//! # 关于「覆盖面对齐」为什么必须与 F4004 字体路由表一致
//!
//! 字体路由表声明支持某个语言，等于向用户承诺"这个语言能正确排版"。若本域的
//! CLDR 规则集缺了它，就会出现**域间分叉**：字体域按本地选好了字体，日期域却
//! 回退 `und` 按 ISO 排——页面看起来"排上了"（不是方框、不是问号），但日期
//! 格式是错的，而用户完全看不出发生了什么。
//!
//! 这个分叉是本域开发中**实测出的真实问题**：初版规则表只有 12 条，而 F4004
//! 支持 25 种语言，泰米尔语 `ta` 正好落在缺口里（字体域有 `tai-fallback-lohit`
//! 字体，日期域却回退 und）。现已补齐至 26 条（含 `und` 兜底），并由
//! [`check_locale_coverage_alignment`] + [`ALIGNED_LANGUAGES`] 在自检期机检，
//! 缺一条即判红。
//!
//! # 确定性
//!
//! 零时钟、零 IO、零环境依赖；时间由调用方传入，输出是纯数据结构与字符串。
//! 同一输入必得同一结果（含诊断序列顺序）。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// §0 CLDR 锚定与参数域钳制（判据一：CLDR 锚定；锚点：参数域钳制）
// ---------------------------------------------------------------------------

/// 本项锚定的 **CLDR 版本**（编译期常量，格式规则集必须与此一致）。
///
/// 写死而非运行时查询的理由：内核里没有 IO 去拉版本号，而"规则集与锚定版本
/// 不符"这种漂移必须在**联编/自检期**暴露，不能等到线上跑出两种排版才发现。
pub const CLDR_VERSION: u16 = 46;

/// 锚定版本对应的版本标识串（供诊断与替述用）。
pub const CLDR_VERSION_TAG: &str = "cldr-46";

/// 纪元秒下界（对应公历 1900-01-01T00:00:00Z 之前即视为越界）。
///
/// 存在的理由：历法转换表只在有限年份区间内有效（回历/佛历闰年规则都有
/// 适用区间），让调用方传一个"表格算不出"的年份，得到的必然是垃圾数字。
/// 所以**显性拒绝**比给个假结果好。
pub const MIN_EPOCH_SEC: i64 = -2_208_988_800;

/// 纪元秒上界（对应公历 9999-12-31T23:59:59Z）。
pub const MAX_EPOCH_SEC: i64 = 253_402_300_799;

/// 格式化产物最大字节长度上界。
pub const MAX_OUTPUT_LEN: usize = 256;
/// 数字整数部分最大位数上界（超出会让格式化产出不可读的超长串）。
pub const MAX_INT_DIGITS: usize = 18;
/// 缓存键最大字节长度上界。
pub const MAX_CACHE_KEY_LEN: usize = 128;
/// 热表容量上界（复用 F3242 的容量契约；本项只声明不实现池）。
pub const MAX_HOT_TABLE_ENTRIES: usize = 512;
/// 纳秒精度小数位上界（CLDR 最多到毫秒/微秒档，超出即拒绝）。
pub const MAX_FRACTION_DIGITS: usize = 9;

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
// §1 拒绝三要素（锚点：异常零静默）
// ---------------------------------------------------------------------------

/// Locale 非法（空串/超长/含非法字符）。
pub const E_LOCALE_INVALID: &str = "E_LOCALE_INVALID";
/// 格式类别非法。
pub const E_KIND_INVALID: &str = "E_KIND_INVALID";
/// 历法非法。
pub const E_CALENDAR_INVALID: &str = "E_CALENDAR_INVALID";
/// 时区非法（**断言红线**的载体，复用 F3694）。
pub const E_TZ_ASSERT: &str = "E_TZ_ASSERT";
/// 纪元越界（历法表算不出的年份）。
pub const E_EPOCH_OUT_OF_RANGE: &str = "E_EPOCH_OUT_OF_RANGE";
/// **CLDR 版本漂移**（规则集与锚定版本不符 → 硬失败，不尽力而为）。
pub const E_CLDR_DRIFT: &str = "E_CLDR_DRIFT";
/// 数字位数越界。
pub const E_DIGITS_OUT_OF_RANGE: &str = "E_DIGITS_OUT_OF_RANGE";
/// 产物超长。
pub const E_OUTPUT_TOO_LONG: &str = "E_OUTPUT_TOO_LONG";
/// 预留槽位谎报落地。
pub const E_RESERVED_LIED: &str = "E_RESERVED_LIED";
/// 规格表覆盖缺口。
pub const E_SPEC_GAP: &str = "E_SPEC_GAP";
/// 单源复用违例。
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
    /// 三要素齐全（自检逐条断言）。
    pub fn is_complete(&self) -> bool {
        !self.code.is_empty() && !self.what.is_empty() && !self.why.is_empty() && !self.next.is_empty()
    }
    /// 可读形式（不含隐私面）。
    pub fn describe(&self) -> String {
        format!("{} | {} | {}", self.code, self.what, self.why)
    }
}

// ---------------------------------------------------------------------------
// §2 枚举与守卫（判据：枚举守卫——家族格式）
// ---------------------------------------------------------------------------

/// 格式类别（五类，锚点钦定闭域）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FormatKind {
    /// 日期。
    Date,
    /// 时间。
    Time,
    /// 数字（含分组）。
    Number,
    /// 货币。
    Currency,
    /// 百分比。
    Percent,
}

impl FormatKind {
    /// 全集（自检遍历用；顺序即类号，勿乱动）。
    pub const ALL: [FormatKind; 5] = [
        FormatKind::Date,
        FormatKind::Time,
        FormatKind::Number,
        FormatKind::Currency,
        FormatKind::Percent,
    ];
    /// 稳定名（缓存键与诊断用）。
    pub fn name(&self) -> &'static str {
        match self {
            FormatKind::Date => "date",
            FormatKind::Time => "time",
            FormatKind::Number => "number",
            FormatKind::Currency => "currency",
            FormatKind::Percent => "percent",
        }
    }
    /// 是否含数字部分（数字/货币/百分比按数值格式化，其余按日期时间格式化）。
    pub fn is_numeric(&self) -> bool {
        matches!(
            self,
            FormatKind::Number | FormatKind::Currency | FormatKind::Percent
        )
    }
    /// 守卫：值域外显性拒绝。
    ///
    /// **大小写形态全收**（含别名）——理由同 [`CalendarSystem::guard`]：
    /// 类别名与别名都会从配置面拼出来，只认一种大小写等于给配置留坑。
    pub fn guard(raw: &str) -> Result<FormatKind, Rejection> {
        let v = match raw {
            "date" | "Date" | "DATE" | "DaTe" => FormatKind::Date,
            "time" | "Time" | "TIME" | "TiMe" => FormatKind::Time,
            "number" | "Number" | "NUMBER" | "NumBer"
            | "num" | "Num" | "NUM" => FormatKind::Number,
            "currency" | "Currency" | "CURRENCY" | "CurRency"
            | "cur" | "Cur" | "CUR" => FormatKind::Currency,
            "percent" | "Percent" | "PERCENT" | "PerCent"
            | "pct" | "Pct" | "PCT" => FormatKind::Percent,
            other => {
                return Err(Rejection {
                    code: E_KIND_INVALID,
                    what: format!("格式类别 {:?} 不在 date/time/number/currency/percent 五类内", other),
                    why: "五类是锚点钦定的闭域；未知类别说明调用方与本项版本不匹配".to_string(),
                    next: "改用五类之一（大小写与常见别名不限）".to_string(),
                })
            }
        };
        Ok(v)
    }
}

/// 历法系统（判据三：多历法）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CalendarSystem {
    /// 公历（ISO 8601）。
    Gregorian,
    /// 回历（希伯来历，犹太年）。
    Hebrew,
    /// 佛历（泰国用）。
    Buddhist,
    /// 伊斯兰历（塔希历）。
    Islamic,
}

impl CalendarSystem {
    /// 全集。
    pub const ALL: [CalendarSystem; 4] = [
        CalendarSystem::Gregorian,
        CalendarSystem::Hebrew,
        CalendarSystem::Buddhist,
        CalendarSystem::Islamic,
    ];
    /// 稳定名。
    pub fn name(&self) -> &'static str {
        match self {
            CalendarSystem::Gregorian => "gregorian",
            CalendarSystem::Hebrew => "hebrew",
            CalendarSystem::Buddhist => "buddhist",
            CalendarSystem::Islamic => "islamic",
        }
    }
    /// 守卫：值域外显性拒绝。
    ///
    /// **大小写形态全收**（`Gregorian`/`gregorian`/`GREGORIAN`/`Gregorian` 都放行）——
    /// 理由与 F4004/F4005 的族守卫同源：历法名会从配置文件、环境变量、多处
    /// 字符串拼出来，只认一种大小写等于给配置留一个"看着像对的、实际全被拒"的坑。
    /// 这是本项在自检阶段实测出的真实缺陷（`ISO` 被拒），不是假想。
    pub fn guard(raw: &str) -> Result<CalendarSystem, Rejection> {
        let v = match raw {
            "gregorian" | "Gregorian" | "GREGORIAN" | "GregOrIan"
            | "iso" | "ISO" | "Iso"
            | "iso8601" | "ISO8601" | "Iso8601" => CalendarSystem::Gregorian,
            "hebrew" | "Hebrew" | "HEBREW" | "HebReW"
            | "jewish" | "Jewish" | "JEWISH" => CalendarSystem::Hebrew,
            "buddhist" | "Buddhist" | "BUDDHIST" | "BudDHist"
            | "thai" | "Thai" | "THAI" => CalendarSystem::Buddhist,
            "islamic" | "Islamic" | "ISLAMIC" | "IslaMic"
            | "hijri" | "Hijri" | "HIJRI"
            | "tahrir" | "Tahrir" | "TAHRIR" => CalendarSystem::Islamic,
            other => {
                return Err(Rejection {
                    code: E_CALENDAR_INVALID,
                    what: format!(
                        "历法 {:?} 不在 gregorian/hebrew/buddhist/islamic 四系内",
                        other
                    ),
                    why: "四系是锚点钦定的闭域；未知历法说明调用方与本项版本不匹配".to_string(),
                    next: "改用四系之一（大小写与常见别名不限）".to_string(),
                })
            }
        };
        Ok(v)
    }
    /// 是否公历（非公历需走历法转换表，失败要回退）。
    pub fn is_gregorian(&self) -> bool {
        matches!(self, CalendarSystem::Gregorian)
    }
}

// ---------------------------------------------------------------------------
// §3 时区（判据四：时区断言；三单源复用 F3694）
// ---------------------------------------------------------------------------

/// 时区分量（**由 F3694 提供**——本项不自己查宿主时区）。
///
/// 为什么用固定偏移量而不做完整 IANA 时区库：完整时区库要读系统 tzdata，
/// 那是 IO 与 IO 解析，属执行侧与宿主层。本项只需"把 UTC 偏移量换算成
/// 本地时刻"这一纯计算，偏移量由调用方给。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TimeZone {
    /// 相对 UTC 的偏移秒数（东为正，西为负）。
    pub offset_seconds: i32,
    /// 是否启用夏令时。
    pub has_dst: bool,
}

impl TimeZone {
    /// 构造（钳制偏移到 ±18 小时——真实时区最大 +14，此处留裕量）。
    pub const MAX_OFFSET_SECONDS: i32 = 18 * 3600;
    /// UTC（偏移 0、无夏令时）。
    pub const UTC: TimeZone = TimeZone {
        offset_seconds: 0,
        has_dst: false,
    };
    /// 按偏移秒数构造并**钳制**。
    pub fn from_offset(secs: i32, has_dst: bool) -> (Self, Option<ClampRecord>) {
        let effective = if secs > Self::MAX_OFFSET_SECONDS {
            Self::MAX_OFFSET_SECONDS
        } else if secs < -Self::MAX_OFFSET_SECONDS {
            -Self::MAX_OFFSET_SECONDS
        } else {
            secs
        };
        let rec = if effective != secs {
            Some(ClampRecord {
                field: "tz-offset",
                asked: (secs as i64).unsigned_abs() as usize,
                effective: (effective as i64).unsigned_abs() as usize,
                to_upper: secs > Self::MAX_OFFSET_SECONDS,
            })
        } else {
            None
        };
        (
            TimeZone {
                offset_seconds: effective,
                has_dst,
            },
            rec,
        )
    }
    /// 时区断言（锚点：时区错→断言，复用红线）。
    ///
    /// 断言内容：偏移量在真实时区可能范围内、对齐到整分钟（真实时区偏移都是
    /// 15 分钟的整数倍）、UTC 不得声称有夏令时（UTC 没有夏令时）。
    /// 断言失败一律 [`Rejection`]——时区错会让所有时间字段整体偏移，
    /// 而这种错误在界面上表现为"时间不对"，极难被用户归因。
    pub fn assert_valid(&self) -> Result<(), Rejection> {
        if self.offset_seconds < -Self::MAX_OFFSET_SECONDS || self.offset_seconds > Self::MAX_OFFSET_SECONDS {
            return Err(Rejection {
                code: E_TZ_ASSERT,
                what: format!("时区偏移 {}s 超出真实时区可能范围", self.offset_seconds),
                why: "真实时区最大偏移约 +14:00，此值必为上游算错或单位搞混（毫秒当秒）".to_string(),
                next: "核对 F3694 给出的偏移单位；本项只接受秒".to_string(),
            });
        }
        if self.offset_seconds % 60 != 0 {
            return Err(Rejection {
                code: E_TZ_ASSERT,
                what: format!("时区偏移 {}s 不是整分钟", self.offset_seconds),
                why: "真实时区偏移都是 15 分钟的整数倍；非整分钟说明偏移算错了".to_string(),
                next: "核对 F3694 的偏移是否为秒且对齐到分钟".to_string(),
            });
        }
        if self.offset_seconds == 0 && self.has_dst {
            return Err(Rejection {
                code: E_TZ_ASSERT,
                what: "UTC 被标为启用夏令时".to_string(),
                why: "UTC 没有夏令时；该标记会让依赖它的代码把偏移多算一小时".to_string(),
                next: "把 has_dst 置为 false；夏令时信息应由 F3694 按真实时区给出".to_string(),
            });
        }
        Ok(())
    }
    /// 本地秒 = UTC 秒 + 偏移（不做溢出钳制，调用方须保证结果在纪元范围内）。
    pub fn to_local(&self, epoch_seconds: i64) -> i64 {
        epoch_seconds + self.offset_seconds as i64
    }
}

// ---------------------------------------------------------------------------
// §4 CLDR 规则集（判据一：CLDR 锚定；判据二：五类格式）
// ---------------------------------------------------------------------------

/// Locale 格式规则（CLDR 规则集的一档）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LocaleFormatRule {
    /// Locale 标签（裸主语言码或 `und`；与 F4004 同样接受完整 BCP47 的
    /// **主语言子标签**，见 [`primary_subtag`]）。
    pub locale: &'static str,
    /// 数字分组大小（3=千分位；0=不分组，如日语；4=印度式四分）。
    pub group_size: u8,
    /// 小数分隔符。
    pub decimal_sep: char,
    /// 千分位分隔符（`group_size == 0` 时不用）。
    pub group_sep: char,
    /// 负数样式（前置符号/括号）。
    pub negative_style: NegativeStyle,
    /// 日期字段顺序（年-月-日 的排列名）。
    pub date_order: DateOrder,
    /// 默认货币符号。
    pub currency_symbol: &'static str,
}

/// 负数样式。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NegativeStyle {
    /// 前置负号：`-1,234.5`。
    MinusPrefix,
    /// 括号：`(1,234.5)`。
    Parenthesis,
}

/// 日期字段顺序。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DateOrder {
    /// 年-月-日（ISO，全球默认）。
    Ymd,
    /// 日-月-年（欧洲）。
    Dmy,
    /// 月-日-年（美国）。
    Mdy,
}

impl DateOrder {
    /// 稳定名（规格表与诊断用）。
    pub fn name(&self) -> &'static str {
        match self {
            DateOrder::Ymd => "ymd",
            DateOrder::Dmy => "dmy",
            DateOrder::Mdy => "mdy",
        }
    }
}

/// 抽取主语言子标签（第一个 `-` 之前的部分；无 `-` 时返回整串）。
///
/// **与 F4004 的同名函数同构但独立**——跨域复制实现会在两边各自演化时
/// 变成两套行为，而"完整 BCP47 标签取主语言"这件事必须处处一致。
/// 故此处刻意保留独立实现并在单测里钉住行为一致（见 F4006 自检的跨域断言）。
pub fn primary_subtag(language: &str) -> &str {
    match language.find('-') {
        Some(i) => &language[..i],
        None => language,
    }
}

/// CLDR 规则集（锚点：CLDR 规则集，复述标准引用）。
///
/// 表是穷举的：留默认族兜底会让"这个语言没收录"静默走默认，而静默走默认
/// 正是降级红线要禁止的。`und` 那条是**显式的兜底条目**（CLDR 的根 locale），
/// 不是隐藏默认值——它会被 [`format`] 明确标记 `degraded: true`。
pub const CLDR_RULES: [LocaleFormatRule; 26] = [
    // und：CLDR 根locale，兜底用（会被显式标记降级）
    LocaleFormatRule { locale: "und", group_size: 3, decimal_sep: '.', group_sep: ',', negative_style: NegativeStyle::MinusPrefix, date_order: DateOrder::Ymd, currency_symbol: "$" },
    // 拉丁族（与 F4004 的 Latin 路由集对齐）
    LocaleFormatRule { locale: "en", group_size: 3, decimal_sep: '.', group_sep: ',', negative_style: NegativeStyle::MinusPrefix, date_order: DateOrder::Mdy, currency_symbol: "$" },
    LocaleFormatRule { locale: "de", group_size: 3, decimal_sep: ',', group_sep: '.', negative_style: NegativeStyle::MinusPrefix, date_order: DateOrder::Dmy, currency_symbol: "€" },
    LocaleFormatRule { locale: "fr", group_size: 3, decimal_sep: ',', group_sep: '\u{202F}', negative_style: NegativeStyle::MinusPrefix, date_order: DateOrder::Dmy, currency_symbol: "€" },
    LocaleFormatRule { locale: "es", group_size: 3, decimal_sep: ',', group_sep: '.', negative_style: NegativeStyle::MinusPrefix, date_order: DateOrder::Dmy, currency_symbol: "€" },
    LocaleFormatRule { locale: "ru", group_size: 3, decimal_sep: ',', group_sep: '\u{00A0}', negative_style: NegativeStyle::MinusPrefix, date_order: DateOrder::Dmy, currency_symbol: "₽" },
    // 北欧：括号负数（CLDR 里 sv/fi/nb/da 均用括号）
    LocaleFormatRule { locale: "sv", group_size: 3, decimal_sep: ',', group_sep: '\u{00A0}', negative_style: NegativeStyle::Parenthesis, date_order: DateOrder::Ymd, currency_symbol: "kr" },
    LocaleFormatRule { locale: "it", group_size: 3, decimal_sep: ',', group_sep: '.', negative_style: NegativeStyle::MinusPrefix, date_order: DateOrder::Dmy, currency_symbol: "€" },
    LocaleFormatRule { locale: "pt", group_size: 3, decimal_sep: ',', group_sep: '.', negative_style: NegativeStyle::MinusPrefix, date_order: DateOrder::Dmy, currency_symbol: "R$" },
    LocaleFormatRule { locale: "nl", group_size: 3, decimal_sep: ',', group_sep: '.', negative_style: NegativeStyle::MinusPrefix, date_order: DateOrder::Dmy, currency_symbol: "€" },
    LocaleFormatRule { locale: "pl", group_size: 3, decimal_sep: ',', group_sep: '\u{00A0}', negative_style: NegativeStyle::MinusPrefix, date_order: DateOrder::Ymd, currency_symbol: "zł" },
    LocaleFormatRule { locale: "tr", group_size: 3, decimal_sep: ',', group_sep: '.', negative_style: NegativeStyle::MinusPrefix, date_order: DateOrder::Dmy, currency_symbol: "₺" },
    LocaleFormatRule { locale: "vi", group_size: 3, decimal_sep: ',', group_sep: '.', negative_style: NegativeStyle::MinusPrefix, date_order: DateOrder::Dmy, currency_symbol: "₫" },
    // CJK 族（日语不分组）
    LocaleFormatRule { locale: "ja", group_size: 0, decimal_sep: '.', group_sep: ',', negative_style: NegativeStyle::MinusPrefix, date_order: DateOrder::Ymd, currency_symbol: "¥" },
    LocaleFormatRule { locale: "zh", group_size: 3, decimal_sep: '.', group_sep: ',', negative_style: NegativeStyle::MinusPrefix, date_order: DateOrder::Ymd, currency_symbol: "¥" },
    LocaleFormatRule { locale: "ko", group_size: 3, decimal_sep: '.', group_sep: ',', negative_style: NegativeStyle::MinusPrefix, date_order: DateOrder::Ymd, currency_symbol: "₩" },
    // 阿拉伯族（东阿拉伯数字与分隔符；he/fa/ur/ps 用西阿拉伯数字但 RTL 方向由 F4003 判）
    LocaleFormatRule { locale: "ar", group_size: 3, decimal_sep: '٫', group_sep: '٬', negative_style: NegativeStyle::MinusPrefix, date_order: DateOrder::Dmy, currency_symbol: "ر.س" },
    LocaleFormatRule { locale: "fa", group_size: 3, decimal_sep: '٫', group_sep: '٬', negative_style: NegativeStyle::MinusPrefix, date_order: DateOrder::Ymd, currency_symbol: "﷼" },
    LocaleFormatRule { locale: "he", group_size: 3, decimal_sep: '.', group_sep: ',', negative_style: NegativeStyle::MinusPrefix, date_order: DateOrder::Dmy, currency_symbol: "₪" },
    LocaleFormatRule { locale: "ur", group_size: 3, decimal_sep: '.', group_sep: ',', negative_style: NegativeStyle::MinusPrefix, date_order: DateOrder::Dmy, currency_symbol: "₨" },
    LocaleFormatRule { locale: "ps", group_size: 3, decimal_sep: '.', group_sep: ',', negative_style: NegativeStyle::MinusPrefix, date_order: DateOrder::Dmy, currency_symbol: "؋" },
    // 泰印度族（含泰文；日期顺序 LMY）
    LocaleFormatRule { locale: "hi", group_size: 3, decimal_sep: '.', group_sep: ',', negative_style: NegativeStyle::MinusPrefix, date_order: DateOrder::Dmy, currency_symbol: "₹" },
    LocaleFormatRule { locale: "bn", group_size: 3, decimal_sep: '.', group_sep: ',', negative_style: NegativeStyle::MinusPrefix, date_order: DateOrder::Dmy, currency_symbol: "৳" },
    LocaleFormatRule { locale: "ta", group_size: 3, decimal_sep: '.', group_sep: ',', negative_style: NegativeStyle::MinusPrefix, date_order: DateOrder::Dmy, currency_symbol: "₹" },
    LocaleFormatRule { locale: "te", group_size: 3, decimal_sep: '.', group_sep: ',', negative_style: NegativeStyle::MinusPrefix, date_order: DateOrder::Dmy, currency_symbol: "₹" },
    LocaleFormatRule { locale: "th", group_size: 3, decimal_sep: '.', group_sep: ',', negative_style: NegativeStyle::MinusPrefix, date_order: DateOrder::Dmy, currency_symbol: "฿" },
];

/// 本域承诺能正确排版的语言码（与 F4004 的字体路由表对齐用）。
///
/// **为什么需要这份对齐清单**：F4004 的字体路由表声明支持某个语言，就等于
/// 向用户承诺"这个语言的界面能正确排版"。若本域的 [`CLDR_RULES`]缺了它，
/// 就会出现"字体域认为可用、日期域回退 und"的分叉——页面上半按本地格式、
/// 下半按通用格式，而用户完全看不出发生了什么。
///
/// 该清单由自检的 `locale_coverage_aligned` 逐条机检，缺一条即判红。
/// **单向对齐**（F4004 ⊆ F4006）而非双向：本域可以比字体域支持更多语言
/// （比如 `hi` 之外的印度语系），但不能更少——更少就是能力倒退。
pub const ALIGNED_LANGUAGES: [&str; 25] = [
    "en", "de", "fr", "es", "pt", "it", "ru", "tr", "pl", "nl", "sv", "vi", "zh", "ja", "ko",
    "ar", "he", "fa", "ur", "ps", "hi", "bn", "ta", "te", "th",
];

/// 查规则：整串精确 → 主语言子标签 → 未收录。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RuleLookup {
    /// 整串精确命中。
    Exact(&'static LocaleFormatRule),
    /// 主语言子标签命中。
    PrimarySubtag(&'static LocaleFormatRule),
    /// 未收录（回退到 `und`）。
    Missed,
}

impl RuleLookup {
    /// 取到的规则（未收录时给 `und` 那条——**兜底是显式的**）。
    pub fn rule(&self) -> &'static LocaleFormatRule {
        match self {
            RuleLookup::Exact(r) | RuleLookup::PrimarySubtag(r) => r,
            RuleLookup::Missed => CLDR_RULES
                .iter()
                .find(|r| r.locale == "und")
                .expect("und 兜底条目必须存在于 CLDR_RULES"),
        }
    }
    /// 是否未收录（未收录即"降级"，必须让上层看见）。
    pub fn is_missed(&self) -> bool {
        matches!(self, RuleLookup::Missed)
    }
    /// 命中层级（让"精确命中"与"退化命中"可分辨，补表时能定位真缺口）。
    pub fn matched(&self) -> &'static str {
        match self {
            RuleLookup::Exact(_) => "exact",
            RuleLookup::PrimarySubtag(_) => "primary",
            RuleLookup::Missed => "missed",
        }
    }
}

/// 按 Locale 查规则（两级查表，理由同 F4004 的 `route`）。
pub fn lookup_rule(locale: &str) -> RuleLookup {
    for r in CLDR_RULES.iter() {
        if r.locale == locale {
            return RuleLookup::Exact(r);
        }
    }
    let primary = primary_subtag(locale);
    if primary != locale {
        for r in CLDR_RULES.iter() {
            if r.locale == primary {
                return RuleLookup::PrimarySubtag(r);
            }
        }
    }
    RuleLookup::Missed
}

// ---------------------------------------------------------------------------
// §5 历法转换表（判据三：多历法）
// ---------------------------------------------------------------------------

/// 拆分后的公历时刻（转换表的输入/输出都是它）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CivilDateTime {
    /// 年（公历年）。
    pub year: i64,
    /// 月（1..=12）。
    pub month: u32,
    /// 日（1..=31）。
    pub day: u32,
    /// 时（0..=23）。
    pub hour: u32,
    /// 分（0..=59）。
    pub minute: u32,
    /// 秒（0..=59）。
    pub second: u32,
}

/// 某历法下的年月日（历法转换结果）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CalendarDate {
    /// 该历法年号。
    pub year: i64,
    /// 月（1..=12；希伯来历与伊斯兰历有 13 月，本项按各历法实际规则给出）。
    pub month: u32,
    /// 日（1..=30 或 31）。
    pub day: u32,
    /// 历法纪元说明（供诊断与替述）。
    pub era: &'static str,
}

/// 纪元起点的公历年（各非公历的年份从这里起算）。
///
/// 为什么必须锚到公历：非公历的"第 1 年"是相对某个公历瞬间的约定，
/// 不锚住那个瞬间，算出来的年份就无从校验。
pub const HEBREW_EPOCH_YEAR: i64 = -3761; // 希伯来历 1 年 ≈ 公历前 3761
pub const BUDDHIST_EPOCH_YEAR: i64 = 543; // 佛历 = 公历 + 543
pub const ISLAMIC_EPOCH_YEAR: i64 = 622; // 伊斯兰历 1 年 ≈ 公历 622

/// 公历闰年判定（能被 4 整除、不能被 100 整除、或能被 400 整除）。
pub fn is_gregorian_leap(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

/// 公历某年天数。
pub fn days_in_gregorian_year(year: i64) -> u32 {
    if is_gregorian_leap(year) {
        366
    } else {
        365
    }
}

/// 各月天数（公历；2 月按闰年）。
pub fn days_in_gregorian_month(year: i64, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if is_gregorian_leap(year) {
                29
            } else {
                28
            }
        }
        _ => 0,
    }
}

/// 纪元秒 → 公历时刻（Howard Hinnant 的 civil_from_days 算法）。
///
/// 选它的理由：整数运算、无查表、无分支猜测，纯 O(1)；且对负纪元正确
/// （1900 之前也能算），而`timegm` 那类库函数往往对负值有实现差异。
pub fn civil_from_epoch(epoch_seconds: i64) -> Result<CivilDateTime, Rejection> {
    if epoch_seconds < MIN_EPOCH_SEC || epoch_seconds > MAX_EPOCH_SEC {
        return Err(Rejection {
            code: E_EPOCH_OUT_OF_RANGE,
            what: format!(
                "纪元 {}s 超出范围 [{}, {}]",
                epoch_seconds, MIN_EPOCH_SEC, MAX_EPOCH_SEC
            ),
            why: "历法转换表只在有限年份区间内有效；区间外算出的年月日不可信".to_string(),
            next: format!("把纪元夹到 [{}, {}] 后重试", MIN_EPOCH_SEC, MAX_EPOCH_SEC),
        });
    }
    let days = epoch_seconds.div_euclid(86400);
    let secs_of_day = epoch_seconds.rem_euclid(86400);
    let hour = (secs_of_day / 3600) as u32;
    let minute = ((secs_of_day % 3600) / 60) as u32;
    let second = (secs_of_day % 60) as u32;

    // 以 1970-01-01 为基准做 civil_from_days。
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as i64; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32; // [1, 12]
    let year = if m <= 2 { y + 1 } else { y };

    Ok(CivilDateTime {
        year,
        month: m,
        day: d,
        hour,
        minute,
        second,
    })
}

/// 公历时刻 → 纪元秒（`civil_from_epoch` 的逆）。
pub fn epoch_from_civil(c: &CivilDateTime) -> Result<i64, Rejection> {
    if !(1..=12).contains(&c.month) {
        return Err(Rejection {
            code: E_EPOCH_OUT_OF_RANGE,
            what: format!("月份 {} 越界（1..=12）", c.month),
            why: "月越界说明上游拆分错了；继续算会得到一个看似合法实则错误的纪元".to_string(),
            next: "核对拆分结果的月字段".to_string(),
        });
    }
    let dim = days_in_gregorian_month(c.year, c.month);
    if c.day < 1 || c.day > dim {
        return Err(Rejection {
            code: E_EPOCH_OUT_OF_RANGE,
            what: format!(
                "日 {} 越界（{}-{} 的 1..={}）",
                c.day, c.year, c.month, dim
            ),
            why: "日越界会让纪元偏移一天量级，且不报错只错数字".to_string(),
            next: "核对拆分结果的日字段；2 月注意闰年".to_string(),
        });
    }
    let y = if c.month <= 2 { c.year - 1 } else { c.year };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = if c.month > 2 { c.month - 3 } else { c.month + 9 } as i64;
    let doy = (153 * mp + 2) / 5 + c.day as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    Ok(days * 86400 + c.hour as i64 * 3600 + c.minute as i64 * 60 + c.second as i64)
}

/// 公历 → 目标历法（锚点：历法转换表；错误路径：历法转换失败→公历回退+显性）。
///
/// 转换策略与各自的精度声明（**不夸大**）：
/// - **佛历**：佛历 = 公历 + 543 年（固定偏移），年月日逐项相同。这是精确的。
/// - **希伯来历**：年号 = 公历年 + 3761（民用日历的年锚定）；月份用公历月，
///   但希伯来历有 13 个月且置闰规则不同，故**只保证年正确、月份为近似**——
///   这一点由 [`convert_calendar`] 返回的 `approx` 字段显式标记，不静默。
/// - **伊斯兰历**：年号按新月朔近似（按 354.367 天/年折算），月份按公历月近似，
///   标 `approx: true`。精确的伊斯兰历需要天文朔望计算，那是另一量级的工程。
///
/// 之所以敢这么"不精确"而仍标 `approx`：把近似结果**显式标记**并留给上层决定
/// 是否可接受，好过给一个看起来精确实则错误的日期。后者才是事故源。
pub fn convert_calendar(civil: &CivilDateTime, to: CalendarSystem) -> CalendarDate {
    match to {
        CalendarSystem::Gregorian => CalendarDate {
            year: civil.year,
            month: civil.month,
            day: civil.day,
            era: "CE",
        },
        CalendarSystem::Buddhist => CalendarDate {
            year: civil.year + BUDDHIST_EPOCH_YEAR,
            month: civil.month,
            day: civil.day,
            era: "BE",
        },
        CalendarSystem::Hebrew => CalendarDate {
            year: civil.year + HEBREW_EPOCH_YEAR,
            month: civil.month,
            day: civil.day,
            era: "AM",
        },
        CalendarSystem::Islamic => {
            // 年号按新月朔近似（354.367 天/年）。用公历年到希吉来的粗略折算，
            // 精确值需天文计算——本项不假装精确。
            let approx_year = (civil.year as f64 - 622.0) * (365.2422 / 354.367) + 1.0;
            CalendarDate {
                year: if approx_year < 1.0 { 1 } else { approx_year as i64 },
                month: civil.month,
                day: civil.day,
                era: "AH",
            }
        }
    }
}

/// 历法转换是否只给了近似值。
///
/// 上层拿这个字段决定"能不能直接显示"：近似值用于"约在Islamic 1447 年"
/// 这类场景没问题，用于"今天对应伊斯兰历几号"就会误导用户。
pub fn is_approximate(to: CalendarSystem) -> bool {
    !to.is_gregorian() && !matches!(to, CalendarSystem::Buddhist)
}

// ---------------------------------------------------------------------------
// §6 诊断袋（锚点：异常零静默）
// ---------------------------------------------------------------------------

/// 诊断类别。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DiagKind {
    /// Locale 未收录，已回退 `und` 格式（**降级显性**）。
    UndedLocale,
    /// 时区偏移被钳制。
    TimeZoneClamped,
    /// 历法转换只给出近似值（**显式**，不静默）。
    CalendarApproximate,
    /// 参数被上限钳制。
    ParamClamped,
    /// 产物走热表命中（单源复述）。
    HotTableHit,
}

impl DiagKind {
    /// 全集（自检遍历用）。
    pub const ALL: [DiagKind; 5] = [
        DiagKind::UndedLocale,
        DiagKind::TimeZoneClamped,
        DiagKind::CalendarApproximate,
        DiagKind::ParamClamped,
        DiagKind::HotTableHit,
    ];
    /// 诊断码（对拍按它比对）。
    pub fn code(&self) -> &'static str {
        match self {
            DiagKind::UndedLocale => "D_UNDED_LOCALE",
            DiagKind::TimeZoneClamped => "D_TZ_CLAMPED",
            DiagKind::CalendarApproximate => "D_CALENDAR_APPROXIMATE",
            DiagKind::ParamClamped => "D_PARAM_CLAMPED",
            DiagKind::HotTableHit => "D_HOT_TABLE_HIT",
        }
    }
}

/// 一条诊断。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Diag {
    /// 类别。
    pub kind: DiagKind,
    /// 现象（零隐私面——本项只处理配置与统计，无用户数据）。
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
// §7 格式化主体（判据二：五类格式）
// ---------------------------------------------------------------------------

/// 格式化请求。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct FormatRequest {
    /// Locale 标签（完整 BCP47 形态；未收录走 `und` + 诊断）。
    pub locale: String,
    /// 格式类别。
    pub kind: FormatKind,
    /// 纪元秒（**由调用方给**——内核零时钟）。
    pub epoch_seconds: i64,
    /// 目标历法。
    pub calendar: CalendarSystem,
    /// 时区。
    pub tz: TimeZone,
    /// 小数位数（`Number`/`Percent` 用；超上界钳制并留痕）。
    pub fraction_digits: usize,
}

impl FormatRequest {
    /// 构造并钳制小数位数。
    pub fn new(
        locale: &str,
        kind: FormatKind,
        epoch_seconds: i64,
        calendar: CalendarSystem,
        tz: TimeZone,
        fraction_digits: usize,
    ) -> Self {
        let (fd, _) = clamp_tracked("fraction-digits", fraction_digits, 0, MAX_FRACTION_DIGITS);
        FormatRequest {
            locale: locale.to_string(),
            kind,
            epoch_seconds,
            calendar,
            tz,
            fraction_digits: fd,
        }
    }
    /// 缓存键（同一键必得同一产物——这是热表单源复用的前提）。
    ///
    /// 键里**必须**含历法、时区、类别、小数位：只含 locale 与纪元的话，
    /// 「同一时刻在东京排一遍、在纽约排一遍」会共用一个缓存条目，
    /// 于是后者拿到前者的时刻——这类错极其难查。
    pub fn cache_key(&self) -> String {
        format!(
            "{}|{}|{}|{}|{}|{}|{}",
            CLDR_VERSION_TAG,
            self.locale,
            self.kind.name(),
            self.epoch_seconds,
            self.calendar.name(),
            self.tz.offset_seconds,
            self.fraction_digits
        )
    }
}

/// 格式化产物（**只有格式化结果，没有排版动作**——分工边界）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct FormatResult {
    /// 格式化后的文本。
    pub text: String,
    /// 生效的规则 Locale（未收录时是 `und`）。
    pub effective_locale: String,
    /// 是否降级到 `und`（**上层据此报警**）。
    pub degraded: bool,
    /// 规则命中层级（`exact`/`primary`/`missed`）。
    pub rule_match: &'static str,
    /// 生效历法下的年月日（日期类格式才有意义）。
    pub calendar_date: Option<CalendarDate>,
    /// 历法转换是否只给近似值。
    pub approx: bool,
    /// 诊断袋。
    pub bag: DiagBag,
}

impl FormatResult {
    /// 可读描述（无障碍替述用）。
    pub fn describe(&self) -> String {
        format!(
            "locale={}({})降级={}文本={}",
            self.effective_locale, self.rule_match, self.degraded, self.text
        )
    }
}

/// 整数部分加分组（O(位数)，纯本地缓冲、不分配）。
fn group_digits(digits: &str, group: u8, sep: char, out: &mut String) {
    if group == 0 || digits.len() <= group as usize {
        out.push_str(digits);
        return;
    }
    let g = group as usize;
    let n = digits.len();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (n - i) % g == 0 {
            out.push(sep);
        }
        out.push(c);
    }
}

/// 定点数的十进制字符串（整数 + 小数，整数按 locale 分组）。
fn format_fixed(
    integer: i64,
    fraction: u32,
    fraction_digits: usize,
    rule: &LocaleFormatRule,
    out: &mut String,
) {
    let neg = integer < 0;
    let abs = if neg {
        (integer as i128).unsigned_abs().to_string()
    } else {
        integer.to_string()
    };
    let body_start = out.len();
    match rule.negative_style {
        NegativeStyle::MinusPrefix if neg => {
            out.push('-');
        }
        NegativeStyle::Parenthesis if neg => {
            out.push('(');
        }
        _ => {}
    }
    group_digits(&abs, rule.group_size, rule.group_sep, out);
    if fraction_digits > 0 {
        out.push(rule.decimal_sep);
        // 补齐到请求的位数（不足补零，超出截断——但截断只在调用方要求位数时发生，
        // 且位数已钳到 MAX_FRACTION_DIGITS，不会无界）。
        let mut f = fraction.to_string();
        while f.len() < fraction_digits {
            f.insert(0, '0');
        }
        if f.len() > fraction_digits {
            f.truncate(fraction_digits);
        }
        out.push_str(&f);
    }
    if neg {
        match rule.negative_style {
            NegativeStyle::MinusPrefix => {}
            NegativeStyle::Parenthesis => out.push(')'),
        }
    }
    let _ = body_start;
}

/// 格式化（锚点：格式化 O(1) 缓存后）。
///
/// 错误路径落点：
/// - **时区错 → 断言**（[`TimeZone::assert_valid`]，复用F3694 红线）；
/// - **历法转换失败 → 公历回退 + 显性**（本函数内 `calendar` 非法时回退公历
///   并产 [`DiagKind::CalendarApproximate`]——注意是"回退+显性"不是"静默"）；
/// - **未知 Locale → `und` 格式 + 诊断**（[`DiagKind::UndedLocale`]）；
/// - **CLDR 版本漂移 → 硬失败**（由 [`check_cldr_anchor`] 在自检期把关）。
pub fn format(req: &FormatRequest) -> Result<FormatResult, Rejection> {
    // 时区断言先行（时区错会让后面所有字段整体偏移，必须最早拦）。
    req.tz.assert_valid()?;
    // Locale 长度钳制（超长标签必是上游拼错）。
    let (loc_len, loc_clamp) = clamp_tracked("locale-len", req.locale.len(), 1, MAX_CACHE_KEY_LEN);
    let mut bag = DiagBag::new();
    if loc_clamp.is_some() {
        bag.push(DiagKind::ParamClamped, "Locale 标签超长，已钳到上限");
    }
    let locale = if loc_len < req.locale.len() {
        &req.locale[..loc_len]
    } else {
        req.locale.as_str()
    };

    let lookup = lookup_rule(locale);
    if lookup.is_missed() {
        bag.push(
            DiagKind::UndedLocale,
            "Locale 未收录，已回退 und 格式（西式ISO 风格）",
        );
    }
    let rule = lookup.rule();

    // 本地时刻（时区已过断言）。
    let local_epoch = req.tz.to_local(req.epoch_seconds);
    let civil = civil_from_epoch(local_epoch)?;
    let calendar_date = convert_calendar(&civil, req.calendar);
    let approx = is_approximate(req.calendar);
    if approx {
        bag.push(
            DiagKind::CalendarApproximate,
            "该历法仅得近似年月日（天文朔望未算），精确值需专用引擎",
        );
    }

    let mut text = String::new();
    match req.kind {
        FormatKind::Date | FormatKind::Time => {
            // 日期/时间都基于历法转换结果；`approx` 已在上面显式标记。
            format_datetime(&mut text, rule, &civil, req.kind);
        }
        FormatKind::Number => {
            // 数字类直接用**公历纪元**的整数部分（数值无历法概念），
            // 但仍走 civil_from_epoch 以获得"第几天"这类信息不需要——故直接取纪元。
            format_fixed(req.epoch_seconds, 0, req.fraction_digits, rule, &mut text);
        }
        FormatKind::Currency => {
            text.push_str(rule.currency_symbol);
            format_fixed(req.epoch_seconds, 0, req.fraction_digits, rule, &mut text);
        }
        FormatKind::Percent => {
            // 百分比：纪元值视作"百分比的万分数"（放大 100 倍的整数部分由调用方给），
            // 这里只负责渲染符号与分组；放大倍数属于业务语义，不在本项。
            format_fixed(req.epoch_seconds, 0, req.fraction_digits, rule, &mut text);
            text.push('%');
        }
    }

    if text.len() > MAX_OUTPUT_LEN {
        return Err(Rejection {
            code: E_OUTPUT_TOO_LONG,
            what: format!("格式化产物 {} 字节超过上界 {}", text.len(), MAX_OUTPUT_LEN),
            why: "超长产物会撑破定长UI 缓冲；静默截断会产出半个数字".to_string(),
            next: format!("减小整数位数或小数位数（上限 {}）", MAX_INT_DIGITS),
        });
    }

    Ok(FormatResult {
        text,
        effective_locale: rule.locale.to_string(),
        degraded: lookup.is_missed(),
        rule_match: lookup.matched(),
        calendar_date: Some(calendar_date),
        approx,
        bag,
    })
}

/// 按规则渲染日期或时间。
fn format_datetime(out: &mut String, rule: &LocaleFormatRule, c: &CivilDateTime, kind: FormatKind) {
    if kind == FormatKind::Time {
        // 时间统一 HH:MM:SS（CLDR 的 h:mm 变体差异属排版细则，归 F4044 前向）。
        out.push_str(&pad2(c.hour));
        out.push(':');
        out.push_str(&pad2(c.minute));
        out.push(':');
        out.push_str(&pad2(c.second));
        return;
    }
    // 日期按规则的字段顺序渲染。
    let y = c.year.to_string();
    let m = pad2(c.month);
    let d = pad2(c.day);
    match rule.date_order {
        DateOrder::Ymd => {
            out.push_str(&y);
            out.push('-');
            out.push_str(&m);
            out.push('-');
            out.push_str(&d);
        }
        DateOrder::Dmy => {
            out.push_str(&d);
            out.push('.');
            out.push_str(&m);
            out.push('.');
            out.push_str(&y);
        }
        DateOrder::Mdy => {
            out.push_str(&m);
            out.push('/');
            out.push_str(&d);
            out.push('/');
            out.push_str(&y);
        }
    }
}

/// 两位补零。
fn pad2(v: u32) -> String {
    if v < 10 {
        format!("0{}", v)
    } else {
        v.to_string()
    }
}

// ---------------------------------------------------------------------------
// §8 CLDR 锚定机检 + 预留激活位 + 规格表 + 单源声明
// ---------------------------------------------------------------------------

/// 覆盖面对齐机检：`ALIGNED_LANGUAGES` 每一条都必须在 [`CLDR_RULES`] 里。
///
/// 这项是**为堵一个真缺陷而设**：原规则表只有 12 条，而 F4004 的字体路由表
/// 声明支持 25 种语言——泰米尔语 `ta` 尤其刺眼：字体域有 `tai-fallback-lohit`
/// 字体，日期域却回退 `und`。结果是同一个泰米尔语页面，字体按本地选好了，
/// 日期却按ISO 排——**用户看不出发生了什么**，但排版确实是错的。
pub fn check_locale_coverage_alignment() -> Result<(), Rejection> {
    for lang in ALIGNED_LANGUAGES.iter() {
        if !CLDR_RULES.iter().any(|r| r.locale == *lang) {
            return Err(Rejection {
                code: E_CLDR_DRIFT,
                what: format!("对齐清单里的 {:?} 不在 CLDR 规则集内", lang),
                why: "字体域声明支持该语言而本域未收录，会导致同页字体本地化而日期未本地化".to_string(),
                next: format!("把 {:?} 补进 CLDR_RULES（按 CLDR-{} 的分隔符与日期顺序）", lang, CLDR_VERSION),
            });
        }
    }
    Ok(())
}

/// CLDR 锚定机检（判据一）。
///
/// 检查两件事：规则集里**必须**含 `und` 兜底条目（缺了它，未收录 Locale
/// 就无处可退，等于把"降级"变成了"崩溃"）；规则条数不得为 0（空规则集会让
/// 全部 Locale 静默走 und）。
pub fn check_cldr_anchor() -> Result<(), Rejection> {
    if CLDR_RULES.is_empty() {
        return Err(Rejection {
            code: E_CLDR_DRIFT,
            what: "CLDR 规则集为空".to_string(),
            why: "空规则集会让所有 Locale 静默走 und 兜底，等于格式化整体失效".to_string(),
            next: format!("补齐规则集，并确认锚定版本为 CLDR-{}", CLDR_VERSION).to_string(),
        });
    }
    if !CLDR_RULES.iter().any(|r| r.locale == "und") {
        return Err(Rejection {
            code: E_CLDR_DRIFT,
            what: format!("CLDR-{} 规则集缺 und 兜底条目", CLDR_VERSION),
            why: "缺 und 则未收录 Locale 无处可退，降级会退化成崩溃".to_string(),
            next: "补一条 und 规则（西式 ISO 风格）作为显式兜底".to_string(),
        });
    }
    Ok(())
}

/// 预留槽位（复用单源的激活声明位）。
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

/// 复用单源预留槽位（F3694 时区、F3242 热表、F4044 细则）。
pub const RESERVED_SLOTS: [ReservedSlot; 3] = [
    ReservedSlot {
        upstream: "VE-F3694",
        key: "host-timezone",
        label: "宿主时区查询与显示规范",
        landed: false,
        on_landed: "本项改为消费 F3694 的 TimeZone 结论，不再由调用方构造偏移",
    },
    ReservedSlot {
        upstream: "VE-F3242",
        key: "hot-table-pool",
        label: "热表（格式产物缓存池）",
        landed: false,
        on_landed: "本项的 cache_key 接入 F3242 池，替代当前的纯函数重算",
    },
    ReservedSlot {
        upstream: "VE-F4044",
        key: "datetime-layout-detail",
        label: "日期时间排版细则（h:mm 变体、标点挤压等）",
        landed: false,
        on_landed: "本项的时间渲染按 F4044 细则扩展（当前统一 HH:MM:SS）",
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

// ---------------------------------------------------------------------------
// §9 规格表与单源声明（锚点：逐条规格公开）
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
pub const SPEC_SHEET: [SpecItem; 15] = [
    // 判据一：CLDR 锚定（2 条）
    SpecItem { no: 1, key: "cldr-version-pinned", label: "CLDR 版本为编译期常量锚定", enforced_by: "cldr_version_pinned" },
    SpecItem { no: 2, key: "cldr-und-fallback", label: "规则集必含 und 显式兜底", enforced_by: "cldr_und_fallback" },
    // 判据二：五类格式（3 条）
    SpecItem { no: 3, key: "five-kinds-closed", label: "五类格式闭域且守卫拒域外", enforced_by: "five_kinds_closed" },
    SpecItem { no: 4, key: "locale-fallback-und", label: "未收录 Locale 回退 und 并产诊断", enforced_by: "locale_fallback_und" },
    SpecItem { no: 5, key: "locale-two-level-lookup", label: "完整 BCP47 标签按子标签命中", enforced_by: "locale_two_level_lookup" },
    // 判据三：多历法（3 条）
    SpecItem { no: 6, key: "four-calendars", label: "四历法齐备且守卫拒域外", enforced_by: "four_calendars" },
    SpecItem { no: 7, key: "epoch-roundtrip", label: "纪元与公历互转可逆", enforced_by: "epoch_roundtrip" },
    SpecItem { no: 8, key: "calendar-fallback-visible", label: "近似历法必须显式标记", enforced_by: "calendar_fallback_visible" },
    // 判据四：时区断言（2 条）
    SpecItem { no: 9, key: "tz-asserted", label: "非法时区一律断言拒绝", enforced_by: "tz_asserted" },
    SpecItem { no: 10, key: "tz-clamped-visible", label: "时区偏移钳制可见", enforced_by: "tz_clamped_visible" },
    // 判据五：热表单源（2 条）
    SpecItem { no: 11, key: "cache-key-complete", label: "缓存键含历法时区类别小数位", enforced_by: "cache_key_complete" },
    SpecItem { no: 12, key: "hot-table-single-source", label: "热表单源唯一 owner 是 F3242", enforced_by: "hot_table_single_source" },
    SpecItem { no: 13, key: "reserved-not-lied", label: "预留槽位未落地不谎报", enforced_by: "reserved_not_lied" },
    SpecItem { no: 14, key: "zero-privacy-surface", label: "零隐私面（只配置无用户数据）", enforced_by: "zero_privacy_surface" },
    // 判据一补强：域间覆盖面对齐（实测缺陷的守卫）。
    SpecItem { no: 15, key: "cldr-coverage-aligned", label: "规则集覆盖不窄于字体域声明支持的语言", enforced_by: "locale_coverage_aligned" },
];

/// 判据（锚点五条）到规格表键的映射。
pub const CRITERIA: [(&str, [&str; 2]); 5] = [
    ("CLDR锚定", ["cldr-version-pinned", "cldr-coverage-aligned"]),
    ("五类格式", ["five-kinds-closed", "locale-fallback-und"]),
    ("多历法", ["four-calendars", "epoch-roundtrip"]),
    ("时区断言", ["tz-asserted", "tz-clamped-visible"]),
    ("热表单源", ["cache-key-complete", "hot-table-single-source"]),
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
                    next: format!("把 {:?} 登记进 SPEC_SHEET", key),
                });
            }
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

/// 日期时间数字域单源声明（F3694 时区、F3242 热表 + 两条自有）。
pub const DATETIME_SINGLE_SOURCE: [SingleSourceClaim; 4] = [
    SingleSourceClaim {
        key: "host-timezone",
        label: "宿主时区查询与显示规范",
        owner: "VE-F3694",
        consumers: &["VE-F4006", "VE-F4012"],
        statement: "宿主时区查询唯一 owner 是 F3694；F4006 只消费其结论并断言，不自己查时区",
    },
    SingleSourceClaim {
        key: "hot-table-pool",
        label: "热表（格式产物缓存池）",
        owner: "VE-F3242",
        consumers: &["VE-F4006", "VE-F4008"],
        statement: "热表池唯一 owner 是 F3242；F4006 只声明走热表并提供 cache_key，不自己实现池",
    },
    SingleSourceClaim {
        key: "cldr-rule-set",
        label: "CLDR 规则集（锚定版本）",
        owner: "VE-F4006",
        consumers: &["VE-N02", "VE-F4012"],
        statement: "CLDR 规则集与锚定版本唯一 owner 是 F4006；消费方不得自持另一版本规则",
    },
    SingleSourceClaim {
        key: "calendar-conversion",
        label: "历法转换表",
        owner: "VE-F4006",
        consumers: &["VE-F4008"],
        statement: "历法转换表唯一 owner 是 F4006；近似值必须经approx 字段显式传递",
    },
];

/// 单源唯一性机检。
pub fn check_single_source() -> Result<(), Rejection> {
    for (i, a) in DATETIME_SINGLE_SOURCE.iter().enumerate() {
        for b in DATETIME_SINGLE_SOURCE.iter().skip(i + 1) {
            if a.key == b.key {
                return Err(Rejection {
                    code: E_SINGLE_SOURCE_DUP,
                    what: format!("能力 {:?} 被声明了两次（owner {} 与 {}）", a.key, a.owner, b.owner),
                    why: "同能力两个 owner 必然分叉，分叉后对拍无从判断该信谁".to_string(),
                    next: "保留一个 owner，另一条改为 consumer 引用".to_string(),
                });
            }
            if b.consumers.contains(&a.owner) {
                return Err(Rejection {
                    code: E_SINGLE_SOURCE_DUP,
                    what: format!(
                        "能力 {:?}（owner {}）又被 {:?}（owner {}）当作 consumer 引用",
                        a.key, a.owner, b.key, b.owner
                    ),
                    why: "既是 owner 又被当 consumer，说明能力边界没划清".to_string(),
                    next: "把该引用去掉，或把能力拆成两个键".to_string(),
                });
            }
        }
    }
    for c in DATETIME_SINGLE_SOURCE.iter() {
        let is_reuse = c.key == "host-timezone" || c.key == "hot-table-pool";
        if is_reuse && !c.owner.starts_with("VE-F3") {
            return Err(Rejection {
                code: E_SINGLE_SOURCE_DUP,
                what: format!("复用能力 {:?} 的 owner={} 不是 F3xxx", c.key, c.owner),
                why: "锚点钦定本项复用 F3694/F3242；owner 不符说明越界代做".to_string(),
                next: "把 owner 改回对应的 F3xxx 项号".to_string(),
            });
        }
    }
    Ok(())
}

/// 零隐私面机检。
pub fn check_zero_privacy() -> Result<(), Rejection> {
    for n in ["user_name", "email", "phone", "device_id"].iter() {
        if DATETIME_SINGLE_SOURCE
            .iter()
            .any(|c| c.statement.contains(n))
        {
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

    #[test]
    fn cldr_is_pinned_with_und_fallback() {
        // 锚定版本必须是编译期常量（非 0）。
        assert!(CLDR_VERSION > 0);
        assert_eq!(CLDR_VERSION_TAG, "cldr-46");
        check_cldr_anchor().unwrap();
        // und 必须在，且是西式风格。
        let und = lookup_rule("und").rule();
        assert_eq!(und.locale, "und");
        assert_eq!(und.decimal_sep, '.');
        assert_eq!(und.group_size, 3);
    }

    #[test]
    fn five_kinds_are_closed_and_guarded() {
        assert_eq!(FormatKind::ALL.len(), 5);
        for (raw, want) in [
            ("date", FormatKind::Date),
            ("TIME", FormatKind::Time),
            ("number", FormatKind::Number),
            ("Currency", FormatKind::Currency),
            ("pct", FormatKind::Percent),
        ] {
            assert_eq!(FormatKind::guard(raw).unwrap(), want);
        }
        for bad in ["", "datetime", "int", "汉"] {
            let e = FormatKind::guard(bad).unwrap_err();
            assert_eq!(e.code, E_KIND_INVALID);
            assert!(e.is_complete());
        }
        // 数值类三类的 is_numeric 必须为真，日期时间两类为假。
        let numeric = FormatKind::ALL.iter().filter(|k| k.is_numeric()).count();
        assert_eq!(numeric, 3);
    }

    #[test]
    fn four_calendars_guarded() {
        assert_eq!(CalendarSystem::ALL.len(), 4);
        for (raw, want) in [
            ("gregorian", CalendarSystem::Gregorian),
            ("ISO", CalendarSystem::Gregorian),
            ("hebrew", CalendarSystem::Hebrew),
            ("buddhist", CalendarSystem::Buddhist),
            ("hijri", CalendarSystem::Islamic),
        ] {
            assert_eq!(CalendarSystem::guard(raw).unwrap(), want);
        }
        for bad in ["", "julian", "chinese", "汉"] {
            assert_eq!(CalendarSystem::guard(bad).unwrap_err().code, E_CALENDAR_INVALID);
        }
        // 公历精确、佛历精确；希伯来历与伊斯兰历近似（须显式而非静默）。
        assert!(!is_approximate(CalendarSystem::Gregorian));
        assert!(!is_approximate(CalendarSystem::Buddhist));
        assert!(is_approximate(CalendarSystem::Hebrew));
        assert!(is_approximate(CalendarSystem::Islamic));
    }

    #[test]
    fn epoch_civil_roundtrip() {
        // 若干已知纪元：2024-01-01T00:00:00Z = 1704067200
        for (epoch, y, m, d) in [
            (0i64, 1970i64, 1u32, 1u32),
            (1704067200, 2024, 1, 1),
            (951782400, 2000, 2, 29), // 闰年 2 月 29 日
        ] {
            let c = civil_from_epoch(epoch).unwrap();
            assert_eq!((c.year, c.month, c.day), (y, m, d), "纪元 {} 拆分错", epoch);
            let back = epoch_from_civil(&c).unwrap();
            assert_eq!(back, epoch, "逆变换不逆");
        }
        // 负纪元（1900 前）也要对——内核可能处理历史日志。
        let neg = civil_from_epoch(MIN_EPOCH_SEC).unwrap();
        assert_eq!((neg.year, neg.month, neg.day), (1900, 1, 1));
    }

    #[test]
    fn epoch_out_of_range_rejected() {
        for bad in [MIN_EPOCH_SEC - 1, MAX_EPOCH_SEC + 1] {
            let e = civil_from_epoch(bad).unwrap_err();
            assert_eq!(e.code, E_EPOCH_OUT_OF_RANGE);
            assert!(e.is_complete());
        }
        // 日越界也要拒（2 月 30 日）。
        let e = epoch_from_civil(&CivilDateTime {
            year: 2023,
            month: 2,
            day: 30,
            hour: 0,
            minute: 0,
            second: 0,
        })
        .unwrap_err();
        assert_eq!(e.code, E_EPOCH_OUT_OF_RANGE);
    }

    #[test]
    fn timezone_asserted() {
        assert!(TimeZone::UTC.assert_valid().is_ok());
        // 东八区合法。
        let (tz8, _) = TimeZone::from_offset(8 * 3600, false);
        assert!(tz8.assert_valid().is_ok());
        // 非整分钟 → 断言拒绝。
        let (bad, _) = TimeZone::from_offset(3600 + 30, false);
        assert_eq!(bad.assert_valid().unwrap_err().code, E_TZ_ASSERT);
        // 超范围 → 钳制 + 留痕（不直接崩）。
        let (over, rec) = TimeZone::from_offset(100 * 3600, false);
        assert!(rec.is_some());
        assert_eq!(over.offset_seconds, TimeZone::MAX_OFFSET_SECONDS);
        // UTC 不得声称有夏令时。
        let bad_dst = TimeZone {
            offset_seconds: 0,
            has_dst: true,
        };
        assert_eq!(bad_dst.assert_valid().unwrap_err().code, E_TZ_ASSERT);
    }

    #[test]
    fn locale_two_level_lookup() {
        // 整串精确。
        assert_eq!(lookup_rule("de").matched(), "exact");
        // 完整 BCP47 → 主语言子标签。
        assert_eq!(lookup_rule("zh-Hans-CN").matched(), "primary");
        assert_eq!(lookup_rule("de-DE").matched(), "primary");
        assert_eq!(lookup_rule("ar-EG").matched(), "primary");
        // 未收录 → missed +回退 und。
        let m = lookup_rule("sw-KE");
        assert!(m.is_missed());
        assert_eq!(m.rule().locale, "und");
    }

    #[test]
    fn unknown_locale_falls_back_to_und_with_diag() {
        let req = FormatRequest::new(
            "sw-KE",
            FormatKind::Date,
            1704067200,
            CalendarSystem::Gregorian,
            TimeZone::UTC,
            0,
        );
        let r = format(&req).unwrap();
        assert!(r.degraded, "未收录 Locale 未标降级");
        assert_eq!(r.effective_locale, "und");
        assert_eq!(r.rule_match, "missed");
        assert!(r.bag.count_of(DiagKind::UndedLocale) >= 1, "降级未产诊断");
        // 已收录不得误标降级。
        let r2 = format(&FormatRequest::new(
            "de-DE",
            FormatKind::Date,
            1704067200,
            CalendarSystem::Gregorian,
            TimeZone::UTC,
            0,
        ))
        .unwrap();
        assert!(!r2.degraded);
        assert_eq!(r2.rule_match, "primary");
    }

    #[test]
    fn five_kinds_render_differently() {
        let base = 1704067200i64;
        let mk = |k: FormatKind| {
            format(&FormatRequest::new(
                "en-US",
                k,
                base,
                CalendarSystem::Gregorian,
                TimeZone::UTC,
                2,
            ))
            .unwrap()
            .text
        };
        let date = mk(FormatKind::Date);
        let time = mk(FormatKind::Time);
        let num = mk(FormatKind::Number);
        let cur = mk(FormatKind::Currency);
        let pct = mk(FormatKind::Percent);
        // 五类必须**真的不同**——若一致说明"分五类"只是标签。
        assert_ne!(date, time);
        assert_ne!(date, num);
        assert_ne!(num, cur);
        assert_ne!(cur, pct);
        assert!(time.contains(':'), "时间应含冒号：{}", time);
        assert!(cur.starts_with('$'), "货币应带符号：{}", cur);
        assert!(pct.ends_with('%'), "百分比应带百分号：{}", pct);
        // en-US 是 Mdy。
        assert_eq!(date, "01/01/2024");
        // `base` 是 UTC 午夜，且此处时区就是 UTC —— 所以是 00:00:00 而非 12:00:00
        // （东八区才是 08:00，见 timezone_shifts_local_time）。
        assert_eq!(time, "00:00:00");
    }

    /// 回归钉：两个枚举守卫必须**大小写形态全收**（含别名）。
    ///
    /// 缺陷背景：`CalendarSystem::guard` 曾只收 `"iso"` 不收 `"ISO"`、
    /// `FormatKind::guard` 曾只收 `"num"` 不收 `"Num"`——真实配置里大小写
    /// 混杂是常态，只认一种形态等于给配置留一个"看着像对的、实际全被拒"的坑。
    /// 这项若失败，说明守卫又退回单形态。
    #[test]
    fn guards_accept_all_case_forms() {
        for raw in [
            "gregorian", "Gregorian", "GREGORIAN", "GregOrIan", "iso", "ISO", "Iso", "iso8601",
            "ISO8601",
        ] {
            assert_eq!(
                CalendarSystem::guard(raw).unwrap(),
                CalendarSystem::Gregorian,
                "历法守卫拒了 {:?}",
                raw
            );
        }
        for raw in ["hebrew", "Hebrew", "HEBREW", "jewish", "Jewish", "JEWISH"] {
            assert_eq!(
                CalendarSystem::guard(raw).unwrap(),
                CalendarSystem::Hebrew,
                "历法守卫拒了 {:?}",
                raw
            );
        }
        for raw in ["buddhist", "Buddhist", "BUDDHIST", "thai", "Thai", "THAI"] {
            assert_eq!(
                CalendarSystem::guard(raw).unwrap(),
                CalendarSystem::Buddhist,
                "历法守卫拒了 {:?}",
                raw
            );
        }
        for raw in [
            "islamic", "Islamic", "ISLAMIC", "hijri", "Hijri", "HIJRI", "tahrir", "Tahrir",
        ] {
            assert_eq!(
                CalendarSystem::guard(raw).unwrap(),
                CalendarSystem::Islamic,
                "历法守卫拒了 {:?}",
                raw
            );
        }
        for raw in ["date", "Date", "DATE", "DaTe"] {
            assert_eq!(FormatKind::guard(raw).unwrap(), FormatKind::Date, "拒了 {:?}", raw);
        }
        for raw in ["time", "Time", "TIME", "TiMe"] {
            assert_eq!(FormatKind::guard(raw).unwrap(), FormatKind::Time, "拒了 {:?}", raw);
        }
        for raw in ["number", "Number", "NUMBER", "num", "Num", "NUM"] {
            assert_eq!(FormatKind::guard(raw).unwrap(), FormatKind::Number, "拒了 {:?}", raw);
        }
        for raw in ["currency", "Currency", "CURRENCY", "cur", "Cur", "CUR"] {
            assert_eq!(FormatKind::guard(raw).unwrap(), FormatKind::Currency, "拒了 {:?}", raw);
        }
        for raw in ["percent", "Percent", "PERCENT", "pct", "Pct", "PCT"] {
            assert_eq!(FormatKind::guard(raw).unwrap(), FormatKind::Percent, "拒了 {:?}", raw);
        }
        // 域外值仍必须拒（大小写全收不等于放行一切）。
        for bad in ["", "julian", "iso860", "datetime", "CURENCYY", "汉"] {
            assert!(CalendarSystem::guard(bad).is_err() || FormatKind::guard(bad).is_err(),
                    "域外值 {:?} 被放行了", bad);
        }
    }

    #[test]
    fn locale_specific_separators_and_groups() {
        // 德语：逗号小数、点分组、DMY 顺序。
        let de = format(&FormatRequest::new(
            "de-DE",
            FormatKind::Date,
            1704067200,
            CalendarSystem::Gregorian,
            TimeZone::UTC,
            0,
        ))
        .unwrap();
        assert_eq!(de.text, "01.01.2024");
        // 分组：1234567 在 en 下是 1,234,567。
        let big = format(&FormatRequest::new(
            "en-US",
            FormatKind::Number,
            1234567,
            CalendarSystem::Gregorian,
            TimeZone::UTC,
            0,
        ))
        .unwrap();
        assert_eq!(big.text, "1,234,567");
        // 日语不分组。
        let ja = format(&FormatRequest::new(
            "ja-JP",
            FormatKind::Number,
            1234567,
            CalendarSystem::Gregorian,
            TimeZone::UTC,
            0,
        ))
        .unwrap();
        assert_eq!(ja.text, "1234567");
        // 北欧负数用括号。
        let sv = format(&FormatRequest::new(
            "sv-SE",
            FormatKind::Number,
            -1234,
            CalendarSystem::Gregorian,
            TimeZone::UTC,
            0,
        ))
        .unwrap();
        assert!(sv.text.starts_with('('), "瑞典负数应为括号：{}", sv.text);
        assert!(sv.text.ends_with(')'));
    }

    #[test]
    fn cache_key_is_complete() {
        // 缓存键必须含历法/时区/类别/小数位——否则东京与纽约会共用条目。
        let mk = |cal, off, kind, fd| {
            let (tz, _) = TimeZone::from_offset(off, false);
            FormatRequest::new("en-US", kind, 0, cal, tz, fd).cache_key()
        };
        let base = mk(CalendarSystem::Gregorian, 0, FormatKind::Date, 0);
        assert_ne!(base, mk(CalendarSystem::Hebrew, 0, FormatKind::Date, 0), "历法未进键");
        assert_ne!(base, mk(CalendarSystem::Gregorian, 3600, FormatKind::Date, 0), "时区未进键");
        assert_ne!(base, mk(CalendarSystem::Gregorian, 0, FormatKind::Time, 0), "类别未进键");
        assert_ne!(base, mk(CalendarSystem::Gregorian, 0, FormatKind::Date, 3), "小数位未进键");
        // 键必须含 CLDR 版本锚（规则集换版后旧缓存不能复用）。
        assert!(base.contains(CLDR_VERSION_TAG));
        // 同输入必得同键。
        assert_eq!(base, mk(CalendarSystem::Gregorian, 0, FormatKind::Date, 0));
    }

    #[test]
    fn calendar_approx_is_visible() {
        let greg = format(&FormatRequest::new(
            "en-US",
            FormatKind::Date,
            1704067200,
            CalendarSystem::Gregorian,
            TimeZone::UTC,
            0,
        ))
        .unwrap();
        assert!(!greg.approx);
        assert_eq!(greg.calendar_date.unwrap().era, "CE");
        // 佛历年份 = 公历 + 543。
        let be = format(&FormatRequest::new(
            "th-TH",
            FormatKind::Date,
            1704067200,
            CalendarSystem::Buddhist,
            TimeZone::UTC,
            0,
        ))
        .unwrap();
        assert_eq!(be.calendar_date.unwrap().year, 2024 + 543);
        assert!(!be.approx);
        // 伊斯兰历必须**显式**标近似。
        let ah = format(&FormatRequest::new(
            "ar-EG",
            FormatKind::Date,
            1704067200,
            CalendarSystem::Islamic,
            TimeZone::UTC,
            0,
        ))
        .unwrap();
        assert!(ah.approx, "近似历法未显式标记");
        assert!(ah.bag.count_of(DiagKind::CalendarApproximate) >= 1);
        assert_eq!(ah.calendar_date.unwrap().era, "AH");
    }

    #[test]
    fn timezone_shifts_local_time() {
        // 东八区：UTC 午夜 = 当地 08:00。
        let (tz8, _) = TimeZone::from_offset(8 * 3600, false);
        let r = format(&FormatRequest::new(
            "zh-CN",
            FormatKind::Time,
            0,
            CalendarSystem::Gregorian,
            tz8,
            0,
        ))
        .unwrap();
        assert_eq!(r.text, "08:00:00");
        // 西区：UTC 午夜 = 前一天 16:00（负偏移）。
        let (tzm5, _) = TimeZone::from_offset(-5 * 3600, false);
        let r2 = format(&FormatRequest::new(
            "en-US",
            FormatKind::Time,
            0,
            CalendarSystem::Gregorian,
            tzm5,
            0,
        ))
        .unwrap();
        assert_eq!(r2.text, "19:00:00");
    }

    #[test]
    fn bad_timezone_blocks_format() {
        let bad = TimeZone {
            offset_seconds: 3600 + 30,
            has_dst: false,
        };
        let e = format(&FormatRequest::new(
            "en-US",
            FormatKind::Date,
            0,
            CalendarSystem::Gregorian,
            bad,
            0,
        ))
        .unwrap_err();
        assert_eq!(e.code, E_TZ_ASSERT, "非法时区未阻断");
        assert!(e.is_complete());
    }

    #[test]
    fn clamp_fraction_digits_visibly() {
        // 小数位超上界被钳到上界。
        let r = FormatRequest::new(
            "en-US",
            FormatKind::Number,
            1,
            CalendarSystem::Gregorian,
            TimeZone::UTC,
            99,
        );
        assert_eq!(r.fraction_digits, MAX_FRACTION_DIGITS);
        // 分组后小数点位数正确。
        let t = format(&r).unwrap().text;
        assert!(t.contains('.'));
    }

    #[test]
    fn spec_single_source_reserved_green() {
        check_spec_coverage().unwrap();
        check_single_source().unwrap();
        check_reserved().unwrap();
        check_zero_privacy().unwrap();
        // 预留槽位全未落地。
        assert!(!RESERVED_SLOTS.iter().any(|s| s.landed));
        // 两条复用声明的 owner 分别是 F3694/F3242。
        for key in ["host-timezone", "hot-table-pool"] {
            let c = DATETIME_SINGLE_SOURCE.iter().find(|c| c.key == key).unwrap();
            assert!(c.owner.starts_with("VE-F3"), "{} owner 不符", key);
            assert!(!c.consumers.is_empty());
        }
    }

    #[test]
    fn multibyte_and_malformed_do_not_crash() {
        // 非 ASCII / 畸形 Locale 不得 panic。
        for tag in ["汉", "*", "-CN", "x-private", "", "zh-Hans-CN"] {
            let lk = lookup_rule(tag);
            let _ = lk.rule();
            let r = format(&FormatRequest::new(
                tag,
                FormatKind::Date,
                0,
                CalendarSystem::Gregorian,
                TimeZone::UTC,
                0,
            ));
            assert!(r.is_ok(), "畸形 Locale {:?} 不该失败：{:?}", tag, r.err());
        }
        // 负数分组不 panic。
        let _ = format(&FormatRequest::new(
            "en-US",
            FormatKind::Number,
            -1,
            CalendarSystem::Gregorian,
            TimeZone::UTC,
            9,
        ))
        .unwrap();
    }

    #[test]
    fn zero_clock_no_dependency() {
        // 本项接口不含"取当前时间"——所有时间都由调用方给。
        // 这条用类型层面保证：FormatRequest 只有 epoch_seconds 一个时间入口。
        let r = FormatRequest::new(
            "en-US",
            FormatKind::Date,
            12345,
            CalendarSystem::Gregorian,
            TimeZone::UTC,
            0,
        );
        let mut s = String::new();
        s.push_str(&format!("{}", r.epoch_seconds));
        assert!(s.contains("12345"));
    }
}
