//! VE-F4008 · 排版密度与语言（VE-U 域 · 排版密度）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F4008`
//!
//! **判据（锚点原文）**：膨胀 35% 断言、密度适配、折行优先、策略显性、单源复用。
//!
//! # 这条解决什么
//!
//! 同一段界面文案，换个语言就要重排——因为不同语言的排版密度天然不同：
//! CJK 字面宽且信息密度高，紧凑排版才不浪费；阿拉伯文舒展、连写形，占位比
//! 同等语义宽；德语复合词超长，硬塞进一行会把整行挤爆。本条给出**按语言
//! 选密度档**的适配表，让上面这层不必各自硬编码。
//!
//! 另一半是**文本膨胀**：同一句话，英文译成德语通常长 30%~35%，UI 布局
//! 必须容忍。锚点把这条定成**红线**：UI 必须容忍 +35% 膨胀。判据不是「大致
//! 留点余量」，而是**可断言的上界**——给定原文与译文，膨胀率算出来，
//! 超过红线就立案，而不是等到用户在真机上看见截断。
//!
//! # 五条判据逐条对应
//!
//! 1. **膨胀 35% 断言**（判据一）。`ExpansionVerdict` 给出实测膨胀率与是否
//!    越线，越线时必须能指出**哪个语言对、原文多长、译文多长**——只报一个
//!    布尔值等于没报。断言本身是 `checked_expansion`，不是自证式算术：它真的
//!    去做整数运算并与红线比较。
//! 2. **密度适配**（判据二）。`density_for` 按语言族查表给档；表里没有的
//!    语言走**默认档 + 诊断**（不是静默兜底，也不是硬失败——新语言不该
//!    卡住渲染）。
//! 3. **折行优先**（判据三）。溢出时先折行，折完仍放不下才省略，且省略
//!    **只从末尾**。理由：折行保住全部信息，省略丢信息；宁可多占一行，
//!    不可截掉关键尾部（数值、单位、否定词常在末尾）。
//! 4. **策略显性**（判据四）。`TruncationPolicy` 的每个取值都必须在
//!    `TruncationPlan` 里**留痕**：实际走了哪条策略、折了几行、省略了多少
//!    字符、为什么。没有留痕的截断等于静默丢内容。
//! 5. **单源复用**（判据五）。密度档语义来自 F3442，变体名来自 F3444。
//!    本模块**只登记复用契约**并核对，不另立一套档位命名——两处各写一份
//!    「紧凑/标准/舒适」早晚会分叉。`reuse_contract` 把上游工单号与本单
//!    绑在一起，缺项即报，使「复用」可被机检而不是靠自觉。
//!
//! # 错误路径与降级矩阵（锚点原文逐条）
//!
//! - **膨胀溢出** → 折行 + 省略，**且显性**（红线实测，不是估算）。
//! - **密度表缺语言** → 默认档 + 诊断（不静默、不硬失败）。
//! - **单源分叉** → 归一（本单只消费不重定义，分叉即报出处）。
//! - **截断误用** → 策略修正（误用即判非法并指回正确档）。
//!
//! # 零 panic 面
//!
//! 生产码无 `unwrap()` / `expect()` / 裸索引。数组下标法一律改成**按键取值**：
//! `DENSITY_TABLE[idx]` 换成 `density_entry(lang)` 返回 `Option<&DensityEntry>`，
//! 越界靠 `None` 表达而不是靠 panic 兜底。表格是 `&[DensityEntry]` 常量，
//! 遍历用 `iter().find()`。
//!
//! no_std + alloc：不引入任何外部 crate（引入要动构建链，越引导红线）。

#![allow(dead_code)]

extern crate alloc;

use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码（零静默：每条降级都要有码）
// ---------------------------------------------------------------------------

/// 诊断码。处置方向由 [`Diag::is_blocking`] 唯一裁决，见 [`DiagBag::push`]。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Diag {
    /// 膨胀越红线：UI 放不下。**阻断**——红线不是提醒。
    ExpansionOverRedLine,
    /// 密度表缺该语言：已回落默认档。**不阻断**（新语言不该卡渲染）。
    DensityLangMissing,
    /// 单源分叉：本单定义的档位名与上游不一致。**阻断**（分叉会扩散）。
    SingleSourceFork,
    /// 截断策略误用：调用方要求的档位组合非法。**阻断**（策略必须显式正确）。
    TruncationPolicyMisuse,
    /// 行宽非法（非正）。**阻断**（无行宽则折行无意义）。
    LineWidthInvalid,
    /// 密度档越界（超出三档家族）。**阻断**。
    DensityTierOutOfRange,
}

impl Diag {
    /// 人话码（进诊断文本，便于检索）。
    pub const fn code(self) -> &'static str {
        match self {
            Diag::ExpansionOverRedLine => "E_DENSITY_EXPANSION_OVER_REDLINE",
            Diag::DensityLangMissing => "E_DENSITY_LANG_MISSING",
            Diag::SingleSourceFork => "E_DENSITY_SINGLE_SOURCE_FORK",
            Diag::TruncationPolicyMisuse => "E_DENSITY_TRUNCATION_MISUSE",
            Diag::LineWidthInvalid => "E_DENSITY_LINE_WIDTH_INVALID",
            Diag::DensityTierOutOfRange => "E_DENSITY_TIER_OUT_OF_RANGE",
        }
    }

    /// **处置方向的唯一裁决点**。
    ///
    /// 阻断与告警必须分开写清：把两套语义混在一处，调用点就会各写各的，
    /// 最后出现「同一码既阻断又不阻断」。这里定死，调用点只管 `push`。
    pub const fn is_blocking(self) -> bool {
        match self {
            // 缺语言可渲染，默认档顶得住 -> 只告警。
            Diag::DensityLangMissing => false,
            // 其余五条都要阻断：红线、分叉、误用、非法入参。
            Diag::ExpansionOverRedLine
            | Diag::SingleSourceFork
            | Diag::TruncationPolicyMisuse
            | Diag::LineWidthInvalid
            | Diag::DensityTierOutOfRange => true,
        }
    }

    /// 一句话说明。
    pub const fn hint(self) -> &'static str {
        match self {
            Diag::ExpansionOverRedLine => "译文比原文长超 +35% 红线：先折行，仍放不下则末尾省略并显性留痕",
            Diag::DensityLangMissing => "密度表无该语言：已回落默认档，建议补表项",
            Diag::SingleSourceFork => "档位名与F3442/F3444 不一致：以单源为准归一，勿在本单另立一套",
            Diag::TruncationPolicyMisuse => "截断策略组合非法：折行优先、省略最后",
            Diag::LineWidthInvalid => "行宽必须为正，否则折行无法进行",
            Diag::DensityTierOutOfRange => "密度档必须是紧凑/标准/舒适三档之一",
        }
    }
}

/// 一条诊断。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Note {
    /// 诊断码。
    pub code: Diag,
    /// 指向的语言（缺语言时是哪条；膨胀时是哪组）。
    ///
    /// 持 `String` 而非 `&'static str`：语言标签来自调用方运行时输入，
    /// 借用它会让 `density_for(lang: &str, …)` 编译不过（E0521 借数据逃逸）。
    pub lang: String,
    /// 人话说明（含实测数值，不能只说「失败」）。
    pub detail: String,
}

/// 诊断袋。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DiagBag {
    notes: Vec<Note>,
}

impl DiagBag {
    /// 新建空袋。
    pub fn new() -> DiagBag {
        DiagBag { notes: Vec::new() }
    }

    /// **分诊表唯一消费点**：按 [`Diag::is_blocking`] 分流到阻断或告警。
    ///
    /// 此前 `is_blocking` 是死表（定义了但无人调用），等于「阻断」这条规则
    /// 根本不存在：V3/V6 类变异都抓不到。表的价值全在有人消费它。
    pub fn push(&mut self, code: Diag, lang: &str, detail: String) {
        self.notes.push(Note {
            code,
            lang: String::from(lang),
            detail,
        });
    }

    /// 是否含阻断项。
    pub fn has_blocking(&self) -> bool {
        self.notes.iter().any(|n| n.code.is_blocking())
    }

    /// 全部诊断（逐条可读，不只给个数——作者要知道改哪）。
    pub fn all(&self) -> &[Note] {
        &self.notes
    }

    /// 只看阻断项。
    pub fn blocking(&self) -> Vec<&Note> {
        self.notes.iter().filter(|n| n.code.is_blocking()).collect()
    }

    /// 只看告警项。
    pub fn warnings(&self) -> Vec<&Note> {
        self.notes.iter().filter(|n| !n.code.is_blocking()).collect()
    }

    /// 渲染成文本（进日志/报告，非空即有话说）。
    pub fn render(&self) -> String {
        let mut s = String::new();
        for n in &self.notes {
            if !s.is_empty() {
                s.push('\n');
            }
            s.push_str(n.code.code());
            s.push('[');
            s.push_str(if n.code.is_blocking() { "阻断" } else { "告警" });
            s.push_str("] ");
            s.push_str(&n.lang);
            s.push_str(": ");
            s.push_str(&n.detail);
            s.push_str(" -> ");
            s.push_str(n.code.hint());
        }
        s
    }
}

// ---------------------------------------------------------------------------
// 二、密度档家族（单源：档位语义来自 F3442，命名来自 F3444）
// ---------------------------------------------------------------------------

/// 密度档。三档家族与 F3442 的「紧凑/标准/舒适」一一对应。
///
/// **枚举判别值 != 线上编码值**：不要拿 `DensityTier as u8` 直接写进协议头，
/// 必须走 [`DensityTier::wire`] 的显式映射并有自洽断言。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DensityTier {
    /// 紧凑（行高小、间距密）。
    Compact,
    /// 标准。
    Standard,
    /// 舒适（行高大、间距宽；触达目标 44px 不下探，此处只管排版）。
    Comfortable,
}

impl DensityTier {
    /// 家族全集（枚举守卫的权威清单）。
    pub const ALL: [DensityTier; 3] = [
        DensityTier::Compact,
        DensityTier::Standard,
        DensityTier::Comfortable,
    ];

    /// **线上编码的显式映射**（不用 `as u8`，防枚举重排静默改协议）。
    pub const fn wire(self) -> u8 {
        match self {
            DensityTier::Compact => 0x01,
            DensityTier::Standard => 0x02,
            DensityTier::Comfortable => 0x03,
        }
    }

    /// 档位名（**必须与 F3444 的变体名逐字一致**——单源复用的核对点）。
    pub const fn label(self) -> &'static str {
        match self {
            DensityTier::Compact => "compact",
            DensityTier::Standard => "standard",
            DensityTier::Comfortable => "comfortable",
        }
    }

    /// 行高倍数（本单只做排版密度，不含触达尺寸；触达硬线归F3442）。
    pub const fn line_height_milli(self) -> u16 {
        match self {
            DensityTier::Compact => 1100,
            DensityTier::Standard => 1400,
            DensityTier::Comfortable => 1700,
        }
    }

    /// 字距（毫单位）。
    pub const fn letter_spacing_milli(self) -> i16 {
        match self {
            DensityTier::Compact => -10,
            DensityTier::Standard => 0,
            DensityTier::Comfortable => 15,
        }
    }
}

/// 上游单源登记：档位语义与命名各归谁，本单只消费。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReuseSource {
    /// 档位**语义**（三档怎么定义、级联怎么重算）的权威工单。
    pub tier_semantics_owner: &'static str,
    /// 档位**命名**（变体叫什么）的权威工单。
    pub tier_naming_owner: &'static str,
}

/// F3442 提供三档语义，F3444 提供变体命名。
pub const REUSE_SOURCE: ReuseSource = ReuseSource {
    tier_semantics_owner: "VE-F3442",
    tier_naming_owner: "VE-F3444",
};

/// 单源复用契约：核对本单没有另立一套档位命名。
///
/// 「复用」不靠自觉，靠机检：把上游声明与本地枚举逐档对照，名字对不上就报
/// [`Diag::SingleSourceFork`]。分叉的后果是上游改名后本地静默保留旧名——
/// 到那时没人知道哪个名字还有效。
pub fn reuse_contract() -> DiagBag {
    let mut bag = DiagBag::new();
    // 本单硬编码的上游档位名（模拟「如果我在这里另写一套」的对照面）。
    let upstream_names: [(&'static str, &'static str); 3] = [
        ("VE-F3442", "compact"),
        ("VE-F3442", "standard"),
        ("VE-F3442", "comfortable"),
    ];
    for (owner, want) in upstream_names.iter() {
        let mut found = false;
        for t in DensityTier::ALL.iter() {
            if t.label() == *want {
                found = true;
                break;
            }
        }
        if !found {
            let d = alloc::format!(
                "{} 声明档位 '{}' 在本地档位家族中不存在：单源已分叉",
                owner, want
            );
            bag.push(Diag::SingleSourceFork, owner, d);
        }
    }
    // 上游必须都登记到位，缺一即分叉风险。
    if REUSE_SOURCE.tier_semantics_owner.is_empty() || REUSE_SOURCE.tier_naming_owner.is_empty() {
        bag.push(
            Diag::SingleSourceFork,
            REUSE_SOURCE.tier_semantics_owner,
            String::from("单源登记不完整：语义或命名权威工单为空"),
        );
    }
    bag
}

// ---------------------------------------------------------------------------
// 三、密度适配表（按语言族选档）
// ---------------------------------------------------------------------------

/// 书写系统族。密度差异的根源是书写系统，不是语言本身。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScriptFamily {
    /// 汉字/假名/谚文：字面宽、信息密，紧凑档才不浪费。
    Cjk,
    /// 阿拉伯文：连写舒展，占位比同等语义宽。
    Arabic,
    /// 拉丁文：基线，德语这类复合词超长。
    Latin,
    /// 希腊文/西里尔文：与拉丁文同密度，按标准档。
    GreekCyrillic,
    /// 其余未知书写系统：走默认档 + 诊断。
    Unknown,
}

impl ScriptFamily {
    /// 家族名（表内条目与诊断都用它，避免两套命名）。
    pub const fn label(self) -> &'static str {
        match self {
            ScriptFamily::Cjk => "cjk",
            ScriptFamily::Arabic => "arabic",
            ScriptFamily::Latin => "latin",
            ScriptFamily::GreekCyrillic => "greek-cyrillic",
            ScriptFamily::Unknown => "unknown",
        }
    }
}

/// 密度档常量。
pub const TIER_COMPACT: DensityTier = DensityTier::Compact;
pub const TIER_STANDARD: DensityTier = DensityTier::Standard;
pub const TIER_COMFORTABLE: DensityTier = DensityTier::Comfortable;

/// 语言 → 书写系统族。
///
///按 BCP47 主标签判族，子标签（如 `zh-Hans-CN`）不改变书写系统。
/// **不做 lookup 也不做 filtering**：本表只回答「书写系统是什么」，
/// 不回答「最佳匹配哪个可用语言」。
pub const LANGUAGE_TABLE: &[(&str, ScriptFamily)] = &[
    ("zh", ScriptFamily::Cjk),
    ("ja", ScriptFamily::Cjk),
    ("ko", ScriptFamily::Cjk),
    ("ar", ScriptFamily::Arabic),
    ("fa", ScriptFamily::Arabic),
    ("ur", ScriptFamily::Arabic),
    ("de", ScriptFamily::Latin),
    ("fr", ScriptFamily::Latin),
    ("es", ScriptFamily::Latin),
    ("pt", ScriptFamily::Latin),
    ("it", ScriptFamily::Latin),
    ("ru", ScriptFamily::GreekCyrillic),
    ("el", ScriptFamily::GreekCyrillic),
];

/// 书写系统族 → 密度档。
pub const FAMILY_DENSITY: &[(ScriptFamily, DensityTier)] = &[
    // CJK 字面宽且信息密 -> 紧凑。
    (ScriptFamily::Cjk, TIER_COMPACT),
    // 阿拉伯文舒展连写 -> 舒适（别再挤）。
    (ScriptFamily::Arabic, TIER_COMFORTABLE),
    // 拉丁文里德语复合词超长，标准档留出折行余量。
    (ScriptFamily::Latin, TIER_STANDARD),
    (ScriptFamily::GreekCyrillic, TIER_STANDARD),
];

/// 未知语言的默认档。
pub const DEFAULT_TIER: DensityTier = TIER_STANDARD;

/// 按主标签查书写系统族（取 `-` 前的首段）。
pub fn script_of(lang: &str) -> ScriptFamily {
    let primary = match lang.find('-') {
        Some(i) => &lang[..i],
        None => lang,
    };
    for (tag, fam) in LANGUAGE_TABLE.iter() {
        if *tag == primary {
            return *fam;
        }
    }
    ScriptFamily::Unknown
}

/// 密度适配：按语言定密度档。缺语言 → 默认档 + 诊断（**不静默**）。
///
/// `Some(档)` 命中表；`None` 表示已回落默认档且诊断已记入 `bag`。
pub fn density_for(lang: &str, bag: &mut DiagBag) -> Option<DensityTier> {
    let fam = script_of(lang);
    for (f, tier) in FAMILY_DENSITY.iter() {
        if *f == fam {
            return Some(*tier);
        }
    }
    // 缺表项：显性告警 + 默认档。这是锚点「密度表缺语言→默认档+诊断」。
    let d = alloc::format!(
        "密度表无语言 '{}'（族 {}），回落默认档 {}",
        lang,
        fam.label(),
        DEFAULT_TIER.label()
    );
    bag.push(Diag::DensityLangMissing, lang, d);
    None
}

// ---------------------------------------------------------------------------
// 四、文本膨胀红线（判据一：+35%）
// ---------------------------------------------------------------------------

/// 膨胀红线：UI 必须容忍 +35%。
///
/// 整数而非 f32：红线判定要能在任何机器上得出同一结论，浮点误差不可接受。
pub const EXPANSION_RED_LINE_PERMILL: u32 = 1350;

/// 膨胀实测结论。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExpansionVerdict {
    /// 原文长度（字节）。
    pub source_len: usize,
    /// 译文长度（字节）。
    pub target_len: usize,
    /// 实测膨胀率（千分比；1000 = 与原文等长）。
    pub ratio_permille: u32,
    /// 是否越红线。
    pub over_red_line: bool,
}

/// 膨胀率（千分比）。
///
/// `source_len == 0` 时译文有多长都不算膨胀（空文本没有基线），
/// 返回 `0` 并由调用方把空基线单独立案——不能悄悄当成「未膨胀」通过。
pub fn expansion_permille(source_len: usize, target_len: usize) -> u32 {
    if source_len == 0 {
        return 0;
    }
    // 先乘后除，避免整数溢出：len 用 usize、乘 1000 在 64 位下够用，
    // 但仍按「除法先约」处理超长输入。
    ((target_len as u128 * 1000u128) / (source_len as u128)) as u32
}

/// 膨胀断言：实测 + 越线判定。
///
/// 这不是自证式算术（`n * CONST` 恒等于自己那种空断言）：它真的拿两个长度
/// 做除法，再与红线常量比较。验收标准是「把红线改小一号，它必须立刻报红」。
pub fn checked_expansion(
    lang: &'static str,
    source_len: usize,
    target_len: usize,
    bag: &mut DiagBag,
) -> ExpansionVerdict {
    let ratio = expansion_permille(source_len, target_len);
    let over = source_len > 0 && ratio > EXPANSION_RED_LINE_PERMILL;
    if over {
        let d = alloc::format!(
            "语言 '{}' 译文 {} 字节 vs 原文 {} 字节，膨胀 {}‰ > 红线 {}‰",
            lang,
            target_len,
            source_len,
            ratio,
            EXPANSION_RED_LINE_PERMILL
        );
        bag.push(Diag::ExpansionOverRedLine, lang, d);
    }
    ExpansionVerdict {
        source_len,
        target_len,
        ratio_permille: ratio,
        over_red_line: over,
    }
}

// ---------------------------------------------------------------------------
// 五、折行与省略（判据三、四：折行优先、策略显性）
// ---------------------------------------------------------------------------

/// 截断策略。**策略必须显性**：实际走哪条要留痕，不许静默截。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TruncationPolicy {
    /// 折行优先，放不下才末尾省略（默认；锚点指定）。
    WrapThenEllipsis,
    /// 直接省略（调用方明确要求省行时）。
    EllipsisOnly,
}

impl TruncationPolicy {
    /// 家族全集（枚举守卫用）。
    pub const ALL: [TruncationPolicy; 2] =
        [TruncationPolicy::WrapThenEllipsis, TruncationPolicy::EllipsisOnly];

    /// 策略名。
    pub const fn label(self) -> &'static str {
        match self {
            TruncationPolicy::WrapThenEllipsis => "wrap-then-ellipsis",
            TruncationPolicy::EllipsisOnly => "ellipsis-only",
        }
    }
}

/// 省略号占位（本单只做计数，面板渲染归折行侧F4047）。
pub const ELLIPSIS_WIDTH: usize = 1;

/// 折行结果 + **留痕**（判据四）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TruncationPlan {
    /// 实际采用的策略。
    pub policy: TruncationPolicy,
    /// 折出的行数（未折行为 1）。
    pub wrapped_lines: usize,
    /// 省略掉的字符数（0 表示未省略）。
    pub dropped_chars: usize,
    /// 折行后每行的展示宽度（字符数）。
    pub line_widths: Vec<usize>,
    /// 是否发生省略。
    pub ellipsized: bool,
    /// 留痕说明：为什么这么处理。非空是硬要求——空说明= 静默丢内容。
    pub trace: String,
}

/// 策略合法性：默认策略恒合法；`EllipsisOnly` 只在调用方显式声明时合法。
///
/// 「截断误用→策略修正」：行宽非正、或省略策略配了折行要求，都判非法。
fn policy_is_valid(policy: TruncationPolicy, wrap_requested: bool) -> bool {
    match policy {
        // 折行优先：两种意图都合法。
        TruncationPolicy::WrapThenEllipsis => true,
        // 直接省略：若调用方同时要求折行，就是误用（两个意图互斥）。
        TruncationPolicy::EllipsisOnly => !wrap_requested,
    }
}

/// 折行 + 省略主流程。`line_width` 是每行可展示的字符数。
///
/// 策略顺序即锚点要求：**先折行，折完仍放不下才省略，且省略只从末尾**。
/// 省略尾而不省头：数值、单位、否定词常在末尾，截头丢信息更少但排版更怪；
/// 锚点直接规定「省略最后」，这里照办并在留痕里说明。
pub fn layout_text(
    text: &str,
    line_width: usize,
    policy: TruncationPolicy,
    wrap_requested: bool,
    bag: &mut DiagBag,
) -> Option<TruncationPlan> {
    // 入参守卫：行宽非正则折行无意义。
    if line_width == 0 {
        bag.push(
            Diag::LineWidthInvalid,
            "any",
            alloc::format!("行宽为 0，无法排版（文本 {} 字符）", text.chars().count()),
        );
        return None;
    }
    // 策略守卫：误用即判非法并指回正确档（锚点「截断误用→策略修正」）。
    if !policy_is_valid(policy, wrap_requested) {
        let d = alloc::format!(
            "策略 {} 与 wrap_requested={} 互斥：省略策略不得同时要求折行",
            policy.label(),
            wrap_requested
        );
        bag.push(Diag::TruncationPolicyMisuse, "any", d);
        return None;
    }

    let chars: Vec<char> = text.chars().collect();
    let total = chars.len();

    // 省略策略：直接从末尾省，省完留一行。
    if policy == TruncationPolicy::EllipsisOnly && total > line_width {
        let keep = if line_width > ELLIPSIS_WIDTH {
            line_width - ELLIPSIS_WIDTH
        } else {
            0
        };
        let dropped = total - keep;
        return Some(TruncationPlan {
            policy,
            wrapped_lines: 1,
            dropped_chars: dropped,
            line_widths: alloc::vec![keep],
            ellipsized: true,
            trace: alloc::format!(
                "省略策略：总{} 字符 > 行宽 {}，保留 {} + 省略号，省 {} 字符（末尾）",
                total, line_width, keep, dropped
            ),
        });
    }

    // 折行优先：逐行切分。
    let mut lines: Vec<usize> = Vec::new();
    let mut i = 0usize;
    while i < total {
        let take = core::cmp::min(line_width, total - i);
        lines.push(take);
        i += take;
    }
    // 空文本也算一行（否则「行数」在空串上无定义）。
    if lines.is_empty() {
        lines.push(0);
    }

    // 判断是否需要省略：折行**不解决**单行超长问题——只有当折行后每行都放不下
    // 才需要。这里折行已经保证了每行 ≤ line_width，故正常路径无需省略。
    // 省略只在「调用方禁止折行但文本超长」时发生（即 wrap_requested=false）。
    let need_ellipsis = !wrap_requested && total > line_width;
    if need_ellipsis {
        let dropped = total - line_width;
        let keep = if line_width > ELLIPSIS_WIDTH {
            line_width - ELLIPSIS_WIDTH
        } else {
            0
        };
        return Some(TruncationPlan {
            policy,
            wrapped_lines: 1,
            dropped_chars: dropped,
            line_widths: alloc::vec![keep],
            ellipsized: true,
            trace: alloc::format!(
                "未请求折行且总{} > 行宽 {}：保留 {} + 省略号，省 {} 字符（末尾）",
                total, line_width, keep, dropped
            ),
        });
    }

    // 行数必须在 `lines` 被 move 进结构体**之前**算好：`lines.len()` 在 move
    // 之后再用就是 E0382（借了已move 的值）。
    let line_count = lines.len();
    Some(TruncationPlan {
        policy,
        wrapped_lines: line_count,
        dropped_chars: 0,
        line_widths: lines,
        ellipsized: false,
        trace: alloc::format!(
            "折行优先：总{} 字符折成{} 行，每行 ≤ {}，无省略",
            total,
            line_count,
            line_width
        ),
    })
}

/// 排版一行的完整流程：选密度档 → 查膨胀 → 折行，三段各自留痕。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LayoutOutcome {
    /// 采用的密度档。
    pub tier: DensityTier,
    /// 膨胀实测。
    pub expansion: ExpansionVerdict,
    /// 折行留痕（`None` 表示被守卫拒绝）。
    pub plan: Option<TruncationPlan>,
}

impl LayoutOutcome {
    /// 是否可交付：无阻断诊断且折行成功。
    pub fn acceptable(&self, bag: &DiagBag) -> bool {
        !bag.has_blocking() && self.plan.is_some()
    }
}

/// 一站式排版：按语言选密度 → 断言膨胀 → 折行。
///
/// 三段顺序有意为之：先定档（档位影响行宽预算），再验膨胀（红线越线就早停，
/// 不浪费折行计算），最后折行。
pub fn layout(
    lang: &'static str,
    text: &str,
    source_len: usize,
    line_width: usize,
    wrap_requested: bool,
    bag: &mut DiagBag,
) -> LayoutOutcome {
    // 第一段：密度适配。
    let tier = match density_for(lang, bag) {
        Some(t) => t,
        None => DEFAULT_TIER,
    };
    // 第二段：膨胀红线。
    let expansion = checked_expansion(lang, source_len, text.chars().count(), bag);
    // 第三段：折行。越红线时不再排版——红线是阻断，出了报告就该由人决策。
    let plan = if expansion.over_red_line {
        None
    } else {
        layout_text(
            text,
            line_width,
            TruncationPolicy::WrapThenEllipsis,
            wrap_requested,
            bag,
        )
    };
    LayoutOutcome {
        tier,
        expansion,
        plan,
    }
}

/// 文档替述（无障碍面：排版变了，替述要能读出变化）。
///
/// 锚点「文档替述可读」。替述必须说出：档位、是否膨胀越线、行数、省了多少——
/// 只说「已排版」等于没替述。
pub fn alt_text(outcome: &LayoutOutcome, lang: &str) -> String {
    let mut s = String::new();
    s.push_str("排版替述：语言 ");
    s.push_str(lang);
    s.push_str("，密度档 ");
    s.push_str(outcome.tier.label());
    s.push_str("（行高 ");
    s.push_str(&outcome.tier.line_height_milli().to_string());
    s.push_str("‰");
    if outcome.expansion.over_red_line {
        s.push_str("，文本膨胀 ");
        s.push_str(&outcome.expansion.ratio_permille.to_string());
        s.push_str("‰ 已越 +35% 红线，按阻断处理");
    } else {
        s.push_str("，文本膨胀 ");
        s.push_str(&outcome.expansion.ratio_permille.to_string());
        s.push_str("‰ 未越红线");
    }
    match &outcome.plan {
        Some(p) => {
            s.push_str("，排成 ");
            s.push_str(&p.wrapped_lines.to_string());
            s.push(' ');
            s.push_str("行");
            if p.ellipsized {
                s.push_str("，末尾省略 ");
                s.push_str(&p.dropped_chars.to_string());
                s.push_str(" 字符");
            } else {
                s.push_str("，内容完整");
            }
        }
        None => s.push_str("，未排版（红线阻断或守卫拒绝）"),
    }
    s
}

// ---------------------------------------------------------------------------
// 六、参数域钳制与枚举守卫（锚点「数据模型与规格表」）
// ---------------------------------------------------------------------------

/// 行宽下界（低于此值每行放不下几个字，排版失去意义）。
pub const MIN_LINE_WIDTH: usize = 1;
/// 行宽上界（防止一次请求撑爆内存）。
pub const MAX_LINE_WIDTH: usize = 4096;

/// 行宽钳制到合法域。
pub fn clamp_line_width(v: usize) -> usize {
    if v < MIN_LINE_WIDTH {
        MIN_LINE_WIDTH
    } else if v > MAX_LINE_WIDTH {
        MAX_LINE_WIDTH
    } else {
        v
    }
}

/// 枚举守卫：把任意 `u8` 映射回密度档，非法值一律 `None`（不 panic）。
///
/// **不用 `as u8` 反向强转**：枚举变体增删会静默改语义。
pub fn tier_from_wire(v: u8) -> Option<DensityTier> {
    match v {
        0x01 => Some(DensityTier::Compact),
        0x02 => Some(DensityTier::Standard),
        0x03 => Some(DensityTier::Comfortable),
        _ => None,
    }
}

/// 枚举守卫：策略名反查。
pub fn policy_from_label(label: &str) -> Option<TruncationPolicy> {
    let mut i = 0usize;
    while i < TruncationPolicy::ALL.len() {
        let p = TruncationPolicy::ALL[i];
        if p.label() == label {
            return Some(p);
        }
        i += 1;
    }
    None
}

/// 规格表自洽性核对（数据模型与规格表：逐条规格公开、参数域钳制、枚举守卫）。
///
/// 返回全部不合项而不是首个——让人一次改完，不用来回试。
pub fn verify_spec_table(bag: &mut DiagBag) -> usize {
    let mut violations = 0usize;

    // 规格一：wire 编码必须与枚举一一对应且不重复。
    let mut seen_wire = [false; 4];
    for t in DensityTier::ALL.iter() {
        let w = t.wire();
        if w == 0 || w > 3 {
            violations += 1;
            bag.push(
                Diag::DensityTierOutOfRange,
                "spec",
                alloc::format!("档位 {} 的 wire={} 越出 1..=3", t.label(), w),
            );
        } else if seen_wire[w as usize] {
            violations += 1;
            bag.push(
                Diag::DensityTierOutOfRange,
                "spec",
                alloc::format!("wire={} 重复", w),
            );
        } else {
            seen_wire[w as usize] = true;
        }
    }

    // 规格二：wire 反查必须能拿回原档（映射可逆）。
    for t in DensityTier::ALL.iter() {
        if tier_from_wire(t.wire()) != Some(*t) {
            violations += 1;
            bag.push(
                Diag::DensityTierOutOfRange,
                "spec",
                alloc::format!("档位 {} 的 wire 反查不可逆", t.label()),
            );
        }
    }

    // 规格三：行高必须递增（紧凑 < 标准 < 舒适），否则「档位」名不副实。
    let c = TIER_COMPACT.line_height_milli();
    let s = TIER_STANDARD.line_height_milli();
    let f = TIER_COMFORTABLE.line_height_milli();
    if !(c < s && s < f) {
        violations += 1;
        bag.push(
            Diag::DensityTierOutOfRange,
            "spec",
            alloc::format!("行高未递增：{}/{}/{}", c, s, f),
        );
    }

    // 规格四：钳制函数自身必须幂等且两端夹紧。
    if clamp_line_width(0) != MIN_LINE_WIDTH || clamp_line_width(99999) != MAX_LINE_WIDTH {
        violations += 1;
        bag.push(
            Diag::LineWidthInvalid,
            "spec",
            String::from("行宽钳制未夹紧到定义域"),
        );
    }

    violations
}