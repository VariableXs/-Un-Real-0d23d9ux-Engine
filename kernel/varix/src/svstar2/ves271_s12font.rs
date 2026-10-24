//! VE-F4031 · RTL 与 T 域字体
//!
//! RTL×T 域字体协同：阿拉伯字体链（F4005 回退链 RTL 段——复用回退链
//! ——单源）；阿拉伯整形（字形整形——F4044 深化前向——整形协同声明）；
//! 数字形态（阿拉伯数字形态：东方/西方数字——数字形态规则表——CLDR
//! 复述）。
//! 数据结构：回退复述；整形协同声明；数字形态表。
//! 错误路径：整形错→F4044 立案；数字形态错→P1；回退缺→链补；单源
//! 分叉→归一。
//! 性能：回退 O(复用)；整形 O(协同)；形态 O(1) 表查；归一 O(断言)。
//! 跨批对接：F4005/F4044 单源复用声明；Q 字体对端；N02 对端。
//! 无隐私面。

#![allow(dead_code)]
use alloc::vec;

extern crate alloc;

use alloc::vec::Vec;
use alloc::string::String;

// ── 诊断码段（0x6DC0）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0x6DC0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct T37FCode(pub u16);

impl T37FCode {
    pub const OK: Self = Self(SEG_BASE);
    pub const NUMFORM_P1: Self = Self(SEG_BASE + 1);
    pub const FALLBACK_MISS: Self = Self(SEG_BASE + 2);
    pub const SOURCE_SPLIT: Self = Self(SEG_BASE + 3);

    pub fn explain(self) -> &'static str {
        match self {
            Self::OK => "正常",
            Self::NUMFORM_P1 => "数字形态错→P1",
            Self::FALLBACK_MISS => "回退缺→链补",
            Self::SOURCE_SPLIT => "单源分叉→归一",
            _ => "未知",
        }
    }
}

// ── 阿拉伯字体回退链（RTL 段——复用 F4005）─────────────────────────

/// 回退链：主字体 → 备选 → 系统兜底。
#[derive(Debug, Clone)]
pub struct FallbackChain {
    pub lang: &'static str,
    pub chain: Vec<&'static str>,
}

/// RTL 语言标准回退链。
pub fn rtl_fallback(lang: &str) -> Option<FallbackChain> {
    match lang {
        "ar" => Some(FallbackChain {
            lang: "ar",
            chain: vec!["NotoNaskhArabic", "Amiri", "GeezaPro", "system-fallback"],
        }),
        "he" => Some(FallbackChain {
            lang: "he",
            chain: vec!["NotoSansHebrew", "David", "system-fallback"],
        }),
        "fa" => Some(FallbackChain {
            lang: "fa",
            chain: vec!["NotoNaskhArabic", "Vazirmatn", "system-fallback"],
        }),
        "ur" => Some(FallbackChain {
            lang: "ur",
            chain: vec!["NotoNastaliqUrdu", "JameelNoori", "system-fallback"],
        }),
        _ => None,
    }
}

/// 回退链断言：链尾必有系统兜底。
pub fn fallback_assert(c: &FallbackChain) -> T37FCode {
    if c.chain.last() == Some(&"system-fallback") {
        T37FCode::OK
    } else {
        T37FCode::FALLBACK_MISS
    }
}

/// 单源归一：同语言多次取链返回同链（单源声明——缓存一致性）。
pub fn source_unify(a: &FallbackChain, b: &FallbackChain) -> T37FCode {
    if a.lang == b.lang && a.chain == b.chain {
        T37FCode::OK
    } else {
        T37FCode::SOURCE_SPLIT
    }
}

// ── 阿拉伯整形协同声明 ──────────────────────────────────────────────

/// 整形协同声明：字符类 → 整形需求。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShapingNeed {
    /// 无整形。
    None,
    /// 初/中/末/独立四态整形（阿拉伯字母）。
    Contextual,
    /// 连字（Lam-Alef 等）。
    Ligature,
}

/// 整形需求表（字符 → 需求）。
pub fn shaping_need(c: char) -> ShapingNeed {
    let u = c as u32;
    if (0x0621..=0x064A).contains(&u) {
        ShapingNeed::Contextual // 阿拉伯字母
    } else if c == '\u{0644}' {
        ShapingNeed::Ligature // Lam
    } else {
        ShapingNeed::None
    }
}

/// 整形协同断言：需要整形的字符必须有整形声明。
pub fn shaping_assert(text: &str, shaping_on: bool) -> T37FCode {
    let needs: Vec<ShapingNeed> = text.chars().map(shaping_need).filter(|n| *n != ShapingNeed::None).collect();
    if !needs.is_empty() && !shaping_on {
        // 整形错 → F4044 立案通道。
        T37FCode::NUMFORM_P1
    } else {
        T37FCode::OK
    }
}

// ── 数字形态规则表（CLDR 复述）─────────────────────────────────────

/// 数字形态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NumForm {
    /// 西方数字（0-9）。
    Western,
    /// 东方阿拉伯数字（٠-٩）。
    EasternArabic,
    /// 波斯数字（۰-۹）。
    Persian,
}

/// 语言 → 数字形态（CLDR 默认）。
pub fn numform_rule(lang: &str) -> NumForm {
    match lang {
        "ar" | "he" => NumForm::EasternArabic,
        "fa" | "ur" => NumForm::Persian,
        _ => NumForm::Western,
    }
}

/// 数字形态断言：语言与形态匹配。
pub fn numform_assert(lang: &str, used: NumForm) -> T37FCode {
    if numform_rule(lang) == used {
        T37FCode::OK
    } else {
        T37FCode::NUMFORM_P1
    }
}

// ── 判据 ─────────────────────────────────────────────────────────────

fn chk(set: &mut crate::checks::CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

pub fn run_ves271_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("ves271_s12font");

    // C4031-01 诊断码段不越窗。
    chk(&mut set, "C4031-01",
        T37FCode::SOURCE_SPLIT.0 < SEG_BASE + 16 && SEG_BASE == 0x6DC0, "err");

    // C4031-02 阿拉伯回退链四层。
    let ar = rtl_fallback("ar").unwrap();
    chk(&mut set, "C4031-02",
        ar.chain.len() == 4 && ar.chain[0] == "NotoNaskhArabic", "err");

    // C4031-03 四语言回退链齐备。
    chk(&mut set, "C4031-03",
        RTL4.iter().all(|l| rtl_fallback(l).is_some()), "err");

    // C4031-04 回退链断言：链尾系统兜底。
    for l in RTL4.iter() {
        let c = rtl_fallback(l).unwrap();
        if fallback_assert(&c) != T37FCode::OK {
            chk(&mut set, "C4031-04", false, "err");
            return set;
        }
    }
    chk(&mut set, "C4031-04", true, "err");

    // C4031-05 回退缺检出。
    let bad = FallbackChain { lang: "ar", chain: vec!["Amiri"] };
    chk(&mut set, "C4031-05", fallback_assert(&bad) == T37FCode::FALLBACK_MISS, "err");

    // C4031-06 单源归一：同链过。
    let ar2 = rtl_fallback("ar").unwrap();
    chk(&mut set, "C4031-06", source_unify(&ar, &ar2) == T37FCode::OK, "err");

    // C4031-07 单源分叉检出。
    let split = FallbackChain { lang: "ar", chain: vec!["别的"] };
    chk(&mut set, "C4031-07", source_unify(&ar, &split) == T37FCode::SOURCE_SPLIT, "err");

    // C4031-08 越名语言无链。
    chk(&mut set, "C4031-08", rtl_fallback("xx").is_none(), "err");

    // C4031-09 整形需求：阿拉伯字母需四态。
    chk(&mut set, "C4031-09",
        shaping_need('ب') == ShapingNeed::Contextual, "err");

    // C4031-10 整形需求：拉丁免。
    chk(&mut set, "C4031-10", shaping_need('a') == ShapingNeed::None, "err");

    // C4031-11 整形需求：Lam 连字。
    chk(&mut set, "C4031-11", shaping_need('\u{0644}') == ShapingNeed::Ligature, "err");

    // C4031-12 整形断言：开着过。
    chk(&mut set, "C4031-12", shaping_assert("سلام", true) == T37FCode::OK, "err");

    // C4031-13 整形断言：关着需整形 → 立案通道。
    chk(&mut set, "C4031-13", shaping_assert("سلام", false) != T37FCode::OK, "err");

    // C4031-14 纯拉丁关整形不误报。
    chk(&mut set, "C4031-14", shaping_assert("hello", false) == T37FCode::OK, "err");

    // C4031-15 数字形态：ar → 东方。
    chk(&mut set, "C4031-15", numform_rule("ar") == NumForm::EasternArabic, "err");

    // C4031-16 数字形态：fa/ur → 波斯。
    chk(&mut set, "C4031-16",
        numform_rule("fa") == NumForm::Persian && numform_rule("ur") == NumForm::Persian, "err");

    // C4031-17 数字形态：en → 西方。
    chk(&mut set, "C4031-17", numform_rule("en") == NumForm::Western, "err");

    // C4031-18 形态断言：匹配过。
    chk(&mut set, "C4031-18", numform_assert("ar", NumForm::EasternArabic) == T37FCode::OK, "err");

    // C4031-19 形态断言：错配 → P1。
    chk(&mut set, "C4031-19",
        numform_assert("ar", NumForm::Western) == T37FCode::NUMFORM_P1, "err");

    // C4031-20 错误码互异。
    let codes = [T37FCode::OK.0, T37FCode::NUMFORM_P1.0, T37FCode::FALLBACK_MISS.0, T37FCode::SOURCE_SPLIT.0];
    chk(&mut set, "C4031-20",
        codes[0] != codes[1] && codes[1] != codes[2] && codes[2] != codes[3], "err");

    set
}

pub const RTL4: [&str; 4] = ["ar", "he", "fa", "ur"];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fallback_chain_tail() {
        for l in RTL4.iter() {
            let c = rtl_fallback(l).unwrap();
            assert_eq!(fallback_assert(&c), T37FCode::OK);
        }
    }

    #[test]
    fn numform_cldr() {
        assert_eq!(numform_rule("ar"), NumForm::EasternArabic);
        assert_eq!(numform_rule("ur"), NumForm::Persian);
    }

    #[test]
    fn shaping_gate() {
        assert_eq!(shaping_assert("سلام", true), T37FCode::OK);
        assert_ne!(shaping_assert("سلام", false), T37FCode::OK);
    }
}
