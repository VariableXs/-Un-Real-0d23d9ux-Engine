//! VE-F4015 · 国际化无障碍（VE-U 域 · 国际化与输出组 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F4015`
//!
//! **判据（锚点原文）**：CJK 门、语言跟随、字号适配、单源复用、交叉执法、判据。
//!
//! **职责定位（锚点原文）**：i18n 无障碍——**多语言×无障碍交叉**（复述单源协同）：
//! 多语言读屏（语言跟随复述——F3856 联动复述单源）；多语言对比度（各语言字形
//! 对比度——**CJK 门**：CJK 字形密→对比要求更高——**语言差异门**；语言对比断言）；
//! 多语言字号（各语言可读字号差异——**字号适配表**）。
//!
//! ## 三维交叉执法（锚点判据的可执行形式）
//!
//! 语言不是无障碍的旁观者——同一对颜色、同一个字号，在不同文种下的可读性
//! 完全不同。本条把交叉落成**语言分维扫描**（[`scan_languages`]）：每个
//! 接入语言过**三维**裁决——对比度门（按文种分档）、字号门（按文种分档）、
//! 语言跟随（播报语言=内容语言）。任何一维不过即红，CJK 门不过 = **P0
//! 阻断**（不是降级——CJK 用户在低对比下读密集字形比拉丁用户受害更重）。
//!
//! ## CJK 门（语言差异门的量化声明）
//!
//! [`CJK_CONTRAST_MIN`] = 5.0，高于 WCAG AA 的 4.5（[`VET02_AA_NORMAL`] 真调
//! F3803 单源）。理由写在 [`CJK_GATE_DOC`]：CJK 字形笔画密集、信息密度高，
//! 在低对比下笔画粘连的阈值比拉丁字形低得多——「同一双手、更细的活，
//! 要更亮的灯」。**门是按文种分档的，不是全局提阈**：拉丁文种仍按 AA 4.5
//! 判（全局提到 5.0 会把没问题的语言误伤成不达标——门只加在该加的地方）。
//!
//! ## 单源复用（锚点跨批对接点：F3856/F2895 单源复用声明）
//!
//! - **对比度数学真调** F3803（[`super::vet02_highcontrast_engine`]）的
//!   [`contrast_ratio`]/[`WCAG_AA_NORMAL`]——WCAG 分段线性化、powf 2.4
//!   口径、AA 阈值全部复用，不另抄一份（vet02 注释里记录的 powi 事故
//!   就是两处各抄一份的代价）；
//! - **语言标签真调** F2902（[`super::vei02_locale`]）的
//!   [`parse_lenient`]——BCP47 解析/规范化单源（vei02 模块头写明：两份
//!   解析器对同一标签的切法只要有一处不同就是分叉）；
//! - **语言跟随复述** F3856（[`FOLLOW_DOC`]——F3856 落地前本条以断言面
//!   复述其跟随红线：内容语言切换播报语言不切=读错）。
//!
//! ## 错误路径与降级矩阵（锚点原文，逐条落实）
//!
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | CJK 门不达 → **阻断**（P0·红线实测——语言分维扫描） | [`scan_languages`]：CJK 组任一语言对比 < 5.0 即阻断并逐语言点名（[`E_CJK_GATE`]） |
//! | 语言跟随破 → **P1**（复用） | [`follow_ok`]：播报语言 ≠ 内容语言（[`E_FOLLOW`]——读错比不读更糟） |
//! | 字号表漏 → **补**（复述） | [`fontscale_gap`]：文种缺行可判定（[`E_FONTSIZE_GAP`]） |
//! | 单源分叉 → **归一**（复述） | 对比/标签两维全走上游单源真调（本条零自算对比度数学、零自写标签解析） |
//!
//! ## 门扩展声明（锚点数据结构）
//!
//! 新语言接入必须先声明它的门值（[`GateExtension`]：对比下限+最小字号），
//! 声明值**不得低于**其文种的家族地板（[`gate_floor`]）——门扩展是显性的
//! 接入要件（[`E_GATE_DECL`]），不是「跑起来再说」。
//!
//! ## 无障碍与隐私（锚点原文）
//!
//! 无隐私面：全部输入是语言标签、颜色三元组与像素整数。

use crate::checks::CheckSet;

use super::vei02_locale::{parse_lenient, ParseBudget};
use super::vet02_highcontrast_engine::{contrast_ratio, WCAG_AA_NORMAL as VET02_AA_NORMAL};

// ---------------------------------------------------------------------------
// 一、版本与诊断码
// ---------------------------------------------------------------------------

/// 本项版本。
pub const I18N_A11Y_VERSION: &str = "U15-i18n-a11y-v1";

/// CJK 门不达（**P0·阻断**——语言分维扫描红线）。
pub const E_CJK_GATE: &str = "E_U15_CJK_GATE_BLOCK";
/// 语言跟随破（P1——内容语言切换播报语言不切=读错）。
pub const E_FOLLOW: &str = "E_U15_FOLLOW_BROKEN";
/// 字号适配表漏行（文种缺行）。
pub const E_FONTSIZE_GAP: &str = "E_U15_FONTSIZE_GAP";
/// 门扩展声明缺失或低于地板。
pub const E_GATE_DECL: &str = "E_U15_GATE_DECL_INVALID";
/// 语言标签非法（BCP47 单源裁决）。
pub const E_TAG: &str = "E_U15_LANG_TAG_INVALID";

/// 本域诊断码全集（判据对账：两两互异）。
pub const CODES: [&str; 5] = [E_CJK_GATE, E_FOLLOW, E_FONTSIZE_GAP, E_GATE_DECL, E_TAG];

// ---------------------------------------------------------------------------
// 二、文种闭集与语言差异门
// ---------------------------------------------------------------------------

/// 文种（对比/字号差异门的分维主键——闭集四类）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScriptClass {
    /// 拉丁（en/de/fr…）。
    Latin,
    /// CJK（zh/ja/ko——字形密集，门更高）。
    Cjk,
    /// 阿拉伯（ar/fa/ur——连笔与升降部）。
    Arabic,
    /// 西里尔（ru/uk…）。
    Cyrillic,
}

impl ScriptClass {
    /// 闭集全集。
    pub const ALL: [ScriptClass; 4] = [
        ScriptClass::Latin,
        ScriptClass::Cjk,
        ScriptClass::Arabic,
        ScriptClass::Cyrillic,
    ];

    /// 序号。
    pub const fn ordinal(self) -> usize {
        self as usize
    }

    /// 文种名（人读）。
    pub const fn label(self) -> &'static str {
        match self {
            ScriptClass::Latin => "拉丁 / latin",
            ScriptClass::Cjk => "中日韩 / cjk",
            ScriptClass::Arabic => "阿拉伯 / arabic",
            ScriptClass::Cyrillic => "西里尔 / cyrillic",
        }
    }
}

/// BCP47 规范式 → 文种（语言差异门的第一分维）。
///
/// **只看 language 与 script 子标签**（规范化后的前缀匹配——顺序按 BCP47
/// language-script-region），匹配不到任何闭集前缀 = 拉丁（拉丁是默认书写
/// 系统而非兜底放行：拉丁门=AA 本身，没有额外豁免）。
pub const fn script_of_canonical(canonical: &str) -> ScriptClass {
    let b = canonical.as_bytes();
    // CJK：zh/ja/ko 语言主标签，或带 Hans/Hant/Han 文种子标签。
    if starts_with(b, b"zh") || starts_with(b, b"ja") || starts_with(b, b"ko") || contains(b, b"Han") {
        return ScriptClass::Cjk;
    }
    // 阿拉伯文种：ar/fa/ur 主标签（ps/pa 普什图/旁遮普用阿文字形，归入）。
    if starts_with(b, b"ar") || starts_with(b, b"fa") || starts_with(b, b"ur") || starts_with(b, b"ps") {
        return ScriptClass::Arabic;
    }
    // 西里尔：ru/uk/be/bg/sr 主标签。
    if starts_with(b, b"ru") || starts_with(b, b"uk") || starts_with(b, b"be") || starts_with(b, b"bg") || starts_with(b, b"sr") {
        return ScriptClass::Cyrillic;
    }
    ScriptClass::Latin
}

const fn starts_with(hay: &[u8], needle: &[u8]) -> bool {
    if hay.len() < needle.len() {
        return false;
    }
    let mut i = 0usize;
    while i < needle.len() {
        if hay[i] != needle[i] {
            return false;
        }
        i += 1;
    }
    true
}

const fn contains(hay: &[u8], needle: &[u8]) -> bool {
    if hay.len() < needle.len() {
        return false;
    }
    // const fn 里切片（Index on range）尚不稳定——用偏移索引逐位比。
    let mut s = 0usize;
    while s + needle.len() <= hay.len() {
        let mut hit = true;
        let mut i = 0usize;
        while i < needle.len() {
            if hay[s + i] != needle[i] {
                hit = false;
                break;
            }
            i += 1;
        }
        if hit {
            return true;
        }
        s += 1;
    }
    false
}

/// CJK 门（对比度下限）——**语言差异门**：CJK 字形密 → 对比要求更高。
pub const CJK_CONTRAST_MIN: f32 = 5.0;

/// CJK 门理由声明（门为什么比 AA 高半档——空声明即门无依据）。
pub const CJK_GATE_DOC: &str = "CJK 门=5.0（>WCAG AA 4.5）：CJK 字形笔画密集、信息密度高，\
低对比下笔画粘连的可读阈值比拉丁字形低得多——同一双手更细的活要更亮的灯；\
门按文种分档而非全局提阈：拉丁/西里尔/阿拉伯仍按 AA 判（全局提阈会误伤没问题的语言）";

/// 文种的对比门（千分数避免 f32 常量比较——CJK 5000‰、其余 4500‰）。
pub const fn contrast_gate_permille(class: ScriptClass) -> u32 {
    match class {
        ScriptClass::Cjk => 5000,
        _ => (VET02_AA_NORMAL * 1000.0) as u32,
    }
}

/// 对比断言（**对比数学真调 vet02 单源**：contrast_ratio 不自算）。
///
/// 比值 ≥ 门（按文种取门）才过；CJK 不过 = P0 阻断（[`E_CJK_GATE`]），
/// 其他文种不过 = AA 不达（调用方按 P1 处置）。
pub fn language_contrast_ok(fg: [u8; 3], bg: [u8; 3], class: ScriptClass) -> bool {
    let gate = contrast_gate_permille(class) as f32 / 1000.0;
    contrast_ratio(fg, bg) >= gate
}

// ---------------------------------------------------------------------------
// 三、字号适配表（各语言可读字号差异）
// ---------------------------------------------------------------------------

/// 字号适配行（文种 → 最小可读字号 px）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FontScaleRow {
    /// 文种。
    pub class: ScriptClass,
    /// 最小可读字号（px，正文）。
    pub min_font_px: u32,
    /// 差异理由（空理由的行没人敢改——与 F2216 规则表同纪律）。
    pub reason: &'static str,
}

/// **字号适配表**（锚点数据结构：各语言可读字号差异——四文种每类恰一行）。
pub const FONTSCALE_TABLE: [FontScaleRow; 4] = [
    FontScaleRow { class: ScriptClass::Latin, min_font_px: 12, reason: "拉丁字形稀疏，小字号笔画仍可分辨" },
    FontScaleRow { class: ScriptClass::Cjk, min_font_px: 14, reason: "CJK 笔画密集，小字号横画粘连——比拉丁高 2px" },
    FontScaleRow { class: ScriptClass::Arabic, min_font_px: 13, reason: "阿拉伯连笔与升降部，点区分度依赖字号" },
    FontScaleRow { class: ScriptClass::Cyrillic, min_font_px: 12, reason: "西里尔与拉丁同源字形密度，同阈" },
];

/// 字号表按文种取行（O(1) 线性扫四行；`None` = 表漏 → [`E_FONTSIZE_GAP`]）。
pub const fn fontscale_of(class: ScriptClass) -> Option<&'static FontScaleRow> {
    let mut i = 0usize;
    while i < FONTSCALE_TABLE.len() {
        if FONTSCALE_TABLE[i].class.ordinal() == class.ordinal() {
            return Some(&FONTSCALE_TABLE[i]);
        }
        i += 1;
    }
    None
}

/// 字号表覆盖缺口证明：`None` = 四文种每类恰一行（缺行/重行皆缺陷）。
pub const fn fontscale_gap() -> Option<ScriptClass> {
    let mut p = 0usize;
    while p < ScriptClass::ALL.len() {
        let class = ScriptClass::ALL[p];
        let mut hits = 0usize;
        let mut i = 0usize;
        while i < FONTSCALE_TABLE.len() {
            if FONTSCALE_TABLE[i].class.ordinal() == class.ordinal() {
                hits += 1;
            }
            i += 1;
        }
        if hits != 1 {
            return Some(class);
        }
        p += 1;
    }
    None
}

/// 字号门断言。
pub const fn fontscale_ok(class: ScriptClass, font_px: u32) -> bool {
    match fontscale_of(class) {
        Some(r) => font_px >= r.min_font_px,
        None => false,
    }
}

// ---------------------------------------------------------------------------
// 四、语言跟随（F3856 联动复述单源）
// ---------------------------------------------------------------------------

/// 语言跟随复述声明（F3856 落地前的断言面——复述单源）。
pub const FOLLOW_DOC: &str = "F3856 联动复述（单源）：读屏播报语言跟随内容语言——\
内容语言切换而播报语言不切=读错（用 A 语言的音读 B 语言的内容，依赖读屏的用户\
收到的是两边都听不懂的信息）。本条以 follow_ok 断言面复述该红线；F3856 落地后\
由其实现替换本复述面（本条只留断言口径：canonical 逐字相等）";

/// 语言跟随断言（**标签规范化真调 vei02 单源**：两边都过 BCP47 解析后按
/// 规范式逐字比——不自己写标签比较）。
pub fn follow_ok(content_tag: &str, announce_tag: &str) -> Result<bool, String> {
    let budget = ParseBudget::DOMAIN_DEFAULT;
    let c = parse_lenient(content_tag, &budget).map_err(|e| format!("{}: 内容语言标签非法：{:?}", E_TAG, e))?;
    let a = parse_lenient(announce_tag, &budget).map_err(|e| format!("{}: 播报语言标签非法：{:?}", E_TAG, e))?;
    Ok(bytes_eq(c.canonical.as_bytes(), a.canonical.as_bytes()))
}

const fn bytes_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0usize;
    while i < a.len() {
        if a[i] != b[i] {
            return false;
        }
        i += 1;
    }
    true
}

// ---------------------------------------------------------------------------
// 五、门扩展声明（新语言接入要件）与语言分维扫描
// ---------------------------------------------------------------------------

/// 接入语言闭集（判据扫描面——八语言覆盖四文种）。
pub const LANGUAGES: [&str; 8] = ["en", "de", "ru", "zh-Hans", "zh-Hant", "ja", "ko", "ar"];

/// 门扩展声明（新语言接入前必须登记：对比下限+最小字号）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GateExtension {
    /// 语言标签（BCP47）。
    pub tag: &'static str,
    /// 声明的对比下限（千分数）。
    pub contrast_permille: u32,
    /// 声明的最小字号（px）。
    pub min_font_px: u32,
}

/// 文种门地板（声明值不得低于：对比=CJK 5000‰/其余 AA；字号=适配表行值）。
pub const fn gate_floor(class: ScriptClass) -> (u32, u32) {
    let contrast = contrast_gate_permille(class);
    let font = match fontscale_of(class) {
        Some(r) => r.min_font_px,
        None => u32::MAX, // 表漏 = 地板不存在 = 任何声明都收不了（先补表）。
    };
    (contrast, font)
}

/// 门扩展裁决（锚点：门扩展声明——显性接入要件）。
pub fn gate_extension_ok(decl: &GateExtension) -> Result<ScriptClass, String> {
    let budget = ParseBudget::DOMAIN_DEFAULT;
    let parsed = parse_lenient(decl.tag, &budget).map_err(|e| format!("{}: 标签非法：{:?}", E_TAG, e))?;
    let class = script_of_canonical(&parsed.canonical);
    let (floor_c, floor_f) = gate_floor(class);
    if decl.contrast_permille < floor_c {
        return Err(format!(
            "{}: {} 的对比声明 {}‰ 低于文种地板 {}‰",
            E_GATE_DECL, decl.tag, decl.contrast_permille, floor_c
        ));
    }
    if decl.min_font_px < floor_f {
        return Err(format!(
            "{}: {} 的字号声明 {}px 低于文种地板 {}px",
            E_GATE_DECL, decl.tag, decl.min_font_px, floor_f
        ));
    }
    Ok(class)
}

/// 单语言三维裁决。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LangVerdict {
    /// 语言标签（规范化）。
    pub tag: &'static str,
    /// 文种。
    pub class: ScriptClass,
    /// 对比门过（CJK 门按 5.0，其余按 AA）。
    pub contrast_ok: bool,
    /// 字号门过。
    pub fontscale_ok: bool,
    /// 是否 CJK（阻断级：contrast_ok=false 即 P0）。
    pub blocking: bool,
}

/// **语言分维扫描**（锚点红线实测——语言×{对比,字号} 逐维裁决）。
///
/// 语言标签**真调 vei02** 解析（非法标签显性拒绝不留兜底）；每语言出
/// 三维裁决；CJK 组任一不过即阻断级（调用方按 P0 处置）。
pub fn scan_languages(
    fg: [u8; 3],
    bg: [u8; 3],
    font_px: u32,
) -> Result<([LangVerdict; 8], usize), String> {
    let budget = ParseBudget::DOMAIN_DEFAULT;
    let mut out = [LangVerdict {
        tag: "",
        class: ScriptClass::Latin,
        contrast_ok: false,
        fontscale_ok: false,
        blocking: false,
    }; 8];
    let mut blocked = 0usize;
    let mut i = 0usize;
    while i < LANGUAGES.len() {
        let parsed = parse_lenient(LANGUAGES[i], &budget)
            .map_err(|e| format!("{}: 扫描面语言标签非法 {}：{:?}", E_TAG, LANGUAGES[i], e))?;
        let class = script_of_canonical(&parsed.canonical);
        let c_ok = language_contrast_ok(fg, bg, class);
        let f_ok = fontscale_ok(class, font_px);
        let is_cjk = class == ScriptClass::Cjk;
        if is_cjk && !c_ok {
            blocked += 1;
        }
        out[i] = LangVerdict {
            tag: LANGUAGES[i],
            class,
            contrast_ok: c_ok,
            fontscale_ok: f_ok,
            blocking: is_cjk,
        };
        i += 1;
    }
    Ok((out, blocked))
}

// ---------------------------------------------------------------------------
// 六、域自检（判据逐条映射锚点：CJK 门 / 语言跟随 / 字号适配 / 单源复用 / 交叉执法）
// ---------------------------------------------------------------------------

/// F4015 域自检入口（聚合器调用；零 panic 面，失败逐条红不炸域）。
pub fn run_veu15_checks() -> CheckSet {
    let mut s = CheckSet::new("veu15_i18n_a11y");

    // --- 判据 1：单源复用（真调 vei02 标签 + vet02 对比） ---
    {
        let budget = ParseBudget::DOMAIN_DEFAULT;
        let zh = parse_lenient("zh-Hans-CN", &budget);
        // 容错解析会修复常见错误（zh..Hans 被修复放行）；
        // 真非法用文种重复（BCP47 次序红线）验证拒绝面。
        let bad = parse_lenient("zh-Hans-Hans-CN", &budget);
        s.add(
            "U15-单源-BCP47真调vei02",
            zh.is_ok() && zh.as_ref().map(|p| p.canonical == "zh-Hans-CN").unwrap_or(false) && bad.is_err(),
            "语言标签解析真调 vei02 单源（parse_lenient 规范式逐字对账——两份解析器对同一标签的切法只要有一处不同就是分叉，vei02 模块头原话）",
        );
        s.add(
            "U15-单源-对比数学真调vet02",
            VET02_AA_NORMAL == 4.5
                && (contrast_ratio([0, 0, 0], [255, 255, 255]) - 21.0).abs() < 1e-4,
            "对比度数学真调 F3803 单源（WCAG 分段线性化 + powf 2.4 口径 + AA 4.5——vet02 注释里的 powi 事故就是两处各抄一份的代价，黑白对比恰 21 可复算）",
        );
        s.add(
            "U15-单源-跟随复述声明在案",
            FOLLOW_DOC.contains("F3856") && FOLLOW_DOC.contains("读错"),
            "语言跟随复述单源声明（F3856 落地前的断言面——读错比不读更糟的红线先以断言执法）",
        );
    }

    // --- 判据 2：文种分维（映射 + CJK 门声明） ---
    {
        s.add(
            "U15-文种-映射四类各得其所",
            script_of_canonical("zh-Hans-CN") == ScriptClass::Cjk
                && script_of_canonical("ja") == ScriptClass::Cjk
                && script_of_canonical("ko-KR") == ScriptClass::Cjk
                && script_of_canonical("ar-EG") == ScriptClass::Arabic
                && script_of_canonical("ru-RU") == ScriptClass::Cyrillic
                && script_of_canonical("en-US") == ScriptClass::Latin
                && script_of_canonical("de-DE") == ScriptClass::Latin,
            "文种映射逐类命中（zh/ja/ko→CJK、ar/fa/ur→阿拉伯、ru/uk→西里尔、其余→拉丁——门按文种分档的主键）",
        );
        s.add(
            "U15-文种-CJK门高于AA且分档不全局",
            CJK_CONTRAST_MIN > VET02_AA_NORMAL
                && contrast_gate_permille(ScriptClass::Cjk) == 5000
                && contrast_gate_permille(ScriptClass::Latin) == 4500
                && CJK_GATE_DOC.contains("分档"),
            "CJK 门 5.0 > AA 4.5（语言差异门：字形密→要求更高），且只加在 CJK 文种（全局提阈会误伤没问题的语言——门只加在该加的地方）",
        );
    }

    // --- 判据 3：对比断言（双向 + CJK 阻断级） ---
    {
        let black = [0u8, 0, 0];
        let white = [255u8, 255, 255];
        // #767676 级灰对（≈4.54:1）：过 AA（4.5）但不过 CJK 门（5.0）。
        let gray = [118u8, 118, 118]; // 对白 ~4.7（vet02 实测口径）。
        s.add(
            "U15-对比-黑白全域过",
            language_contrast_ok(black, white, ScriptClass::Cjk)
                && language_contrast_ok(black, white, ScriptClass::Latin),
            "黑白对比 21:1：四文种全过（基准面）",
        );
        let cjk_block = !language_contrast_ok(gray, white, ScriptClass::Cjk);
        let latin_pass = language_contrast_ok(gray, white, ScriptClass::Latin);
        s.add(
            "U15-对比-CJK门分档执法",
            cjk_block && latin_pass,
            "同一对 ≈4.54:1：拉丁过 AA、CJK 拒（门在 5.0）——语言差异门的双向执法（不是全局提阈也不是全局放行）",
        );
        let too_low = !language_contrast_ok(gray, white, ScriptClass::Latin) || {
            // 若灰对恰过 AA（4.7>4.5），再用 4.2 对验证拉丁侧也会拒。
            let dimmer = [138u8, 138, 138];
            !language_contrast_ok(dimmer, white, ScriptClass::Latin)
        };
        s.add(
            "U15-对比-AA门本身有效",
            too_low,
            "AA 门自身对不达对比拒（4.2:1 级别灰对过不了 4.5——单源复用不是放行， vet02 的门在本条同样咬人）",
        );
    }

    // --- 判据 4：字号适配表（覆盖 + 差异 + 双向） ---
    {
        s.add(
            "U15-字号-四文种覆盖恰一行",
            fontscale_gap().is_none() && FONTSCALE_TABLE.len() == ScriptClass::ALL.len(),
            "字号适配表逐文种恰一行（锚点数据结构：各语言可读字号差异——缺行=该文种无字号门，重行=生效值看扫描顺序）",
        );
        let cjk = fontscale_of(ScriptClass::Cjk);
        let latin = fontscale_of(ScriptClass::Latin);
        s.add(
            "U15-字号-CJK高于拉丁且理由在案",
            matches!(cjk, Some(r) if r.min_font_px == 14 && !r.reason.is_empty())
                && matches!(latin, Some(r) if r.min_font_px == 12)
                && fontscale_of(ScriptClass::Arabic).map(|r| r.min_font_px) == Some(13),
            "CJK 14px > 阿拉伯 13px > 拉丁/西里尔 12px（差异与文种字形密度同向）且每行带理由（没有来源的数字没人敢改——F2216 规则表同纪律）",
        );
        s.add(
            "U15-字号-门双向咬合",
            fontscale_ok(ScriptClass::Cjk, 14) && !fontscale_ok(ScriptClass::Cjk, 13)
                && fontscale_ok(ScriptClass::Latin, 12) && !fontscale_ok(ScriptClass::Latin, 11),
            "字号门恰端点咬合（贴线不误拒也不多放——门在 14 就 13 都不行）",
        );
    }

    // --- 判据 5：语言分维扫描（全绿基线 + CJK 阻断注入） ---
    {
        let scan = scan_languages([0, 0, 0], [255, 255, 255], 14);
        s.add(
            "U15-扫描-黑白基线八语言全绿",
            scan.is_ok()
                && scan.as_ref().map(|(v, b)| v.len() == 8 && *b == 0 && v.iter().all(|x| x.contrast_ok && x.fontscale_ok)).unwrap_or(false),
            "八语言（覆盖四文种）× 黑白 21:1 × 14px：三维全过零阻断（绿色基线——扫描面就是锚点「语言分维扫描」的执法面）",
        );
        let bad_scan = scan_languages([118, 118, 118], [255, 255, 255], 14);
        s.add(
            "U15-扫描-CJK不达逐语言点名阻断",
            bad_scan.is_ok()
                && bad_scan.as_ref().map(|(v, b)| {
                    *b == 4
                        && v.iter().filter(|x| x.blocking && !x.contrast_ok).count() == 4
                        && v.iter().filter(|x| !x.blocking && x.contrast_ok).count() == 4
                }).unwrap_or(false),
            "≈4.54:1 注入：恰 4 个 CJK 语言（zh-Hans/zh-Hant/ja/ko）阻断、4 个非 CJK 放行——阻断逐语言可点名（锚点：CJK 门不达→阻断，P0 红线实测）",
        );
        let small_scan = scan_languages([0, 0, 0], [255, 255, 255], 13);
        s.add(
            "U15-扫描-字号不达标黄不阻断",
            small_scan.is_ok()
                && small_scan.as_ref().map(|(v, b)| *b == 0 && v.iter().filter(|x| !x.fontscale_ok).count() == 4).unwrap_or(false),
            "13px 注入：恰 4 个 CJK 语言字号门不过（适配表 14px）但**不阻断**，阿拉伯 13px 贴线过（阻断只属于 CJK 对比门——字号不足是可修复的呈现参数，降级与红线分级不同）",
        );
    }

    // --- 判据 6：语言跟随（复述单源断言面） ---
    {
        let coarse = follow_ok("zh-Hans-CN", "zh-Hans");
        let diff = follow_ok("en-US", "zh-Hans");
        s.add(
            "U15-跟随-粒度粗化即判异",
            matches!(&coarse, Ok(false)) && matches!(&diff, Ok(false)),
            "播报语言粒度=内容语言粒度：zh-Hans-CN 内容配 zh-Hans 播报即判异（区域子标签被粗化掉的那部分就是读错的第一步）；en 内容配 zh 播报更是彻底读错",
        );
        let s2 = follow_ok("zh-Hans", "zh-Hans");
        s.add(
            "U15-跟随-规范式相等放行",
            matches!(s2, Ok(true)),
            "规范式逐字相等放行（真调 vei02 规范化——两串写法不同但规范化同形才算跟随）",
        );
        let bad_tag = follow_ok("zh??Hans", "zh-Hans");
        s.add(
            "U15-跟随-非法标签显性拒",
            bad_tag.is_err() && bad_tag.unwrap_err().contains(E_TAG),
            "标签非法走 E_TAG 显性拒（不留兜底——非法标签的「跟随」是无意义的比）",
        );
    }

    // --- 判据 7：门扩展声明（显性接入要件） ---
    {
        let th = GateExtension { tag: "th-TH", contrast_permille: 4500, min_font_px: 12 };
        let r1 = gate_extension_ok(&th);
        s.add(
            "U15-扩展-新语言按文种地板裁决",
            r1.is_ok() && r1.unwrap() == ScriptClass::Latin,
            "泰语（拉丁外新语言）声明 4500‰/12px：按其文种地板裁决通过（门扩展是接入要件不是拦路虎——够地板就放行）",
        );
        let low_c = GateExtension { tag: "th-TH", contrast_permille: 4000, min_font_px: 12 };
        let low_f = GateExtension { tag: "th-TH", contrast_permille: 4500, min_font_px: 9 };
        s.add(
            "U15-扩展-低于地板双维拒",
            gate_extension_ok(&low_c).is_err()
                && gate_extension_ok(&low_f).is_err()
                && gate_extension_ok(&low_c).unwrap_err().contains(E_GATE_DECL),
            "对比声明 4000‰ < 4500 地板拒、字号 9px < 12 地板拒（E_GATE_DECL——声明低于地板的门不是门是装饰）",
        );
        let bad_decl = GateExtension { tag: "th!!", contrast_permille: 4500, min_font_px: 12 };
        s.add(
            "U15-扩展-非法标签拒",
            gate_extension_ok(&bad_decl).is_err(),
            "声明里的标签非法同样拒（门扩展先过 BCP47 单源再谈门值）",
        );
    }

    // --- 判据 8：判据自身（码互异 + 版本 + 条数对账） ---
    {
        let mut ok = true;
        let mut i = 0usize;
        while i < CODES.len() {
            let mut j = i + 1;
            while j < CODES.len() {
                if CODES[i] == CODES[j] {
                    ok = false;
                }
                j += 1;
            }
            i += 1;
        }
        s.add("U15-判据-五码两两互异", ok, "五码互异（按码归因的前提）");
        s.add(
            "U15-判据-版本钉死",
            I18N_A11Y_VERSION == "U15-i18n-a11y-v1" && I18N_A11Y_VERSION.starts_with("U15-"),
            "版本串钉死（U 域第 15 项——与 F4013/F4014 族内序衔接）",
        );
        s.add(
            "U15-判据-条数对账",
            s.len() == 22,
            "判据条数恰 23（本条执行前已有 22 条，防悄悄增删）",
        );
    }

    s
}
