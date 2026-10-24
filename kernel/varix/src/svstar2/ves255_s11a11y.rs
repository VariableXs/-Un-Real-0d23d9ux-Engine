//! VE-F4015 · 国际化无障碍
//!
//! i18n 无障碍（多语言×无障碍交叉——复述单源协同）：多语言读屏（语言
//! 跟随复述——F3856 联动复述单源）；多语言对比度（各语言字形对比度——
//! CJK 密集字形对比要求更高——语言对比断言 + CJK 门）；多语言字号（各
//! 语言可读字号差异——字号适配表）。
//! 数据结构：语言对比断言；字号适配表；单源复述；门扩展声明。
//! 错误路径：CJK 门不达→阻断（语言分维扫描）；语言跟随破→P1；字号
//! 表漏→补；单源分叉→归一。
//! 性能：断言 O(语言×组合)；跟随 O(复用)；表 O(适配)；归一 O(复述)。
//! 跨批对接：F3856/F2895 单源复用声明；S02 对端；N02 对端。
//! 无隐私面。

#![allow(dead_code)]
use alloc::vec;

extern crate alloc;

use alloc::vec::Vec;
use alloc::string::String;

// ── 诊断码段（0x6440）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0x6440;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct T21ICode(pub u16);

impl T21ICode {
    pub const OK: Self = Self(SEG_BASE);
    pub const CJK_GATE: Self = Self(SEG_BASE + 1);
    pub const FOLLOW_BROKEN: Self = Self(SEG_BASE + 2);
    pub const TABLE_MISS: Self = Self(SEG_BASE + 3);
    pub const SOURCE_SPLIT: Self = Self(SEG_BASE + 4);

    pub fn explain(self) -> &'static str {
        match self {
            Self::OK => "正常",
            Self::CJK_GATE => "CJK 门不达→阻断",
            Self::FOLLOW_BROKEN => "语言跟随破→P1",
            Self::TABLE_MISS => "字号表漏→补",
            Self::SOURCE_SPLIT => "单源分叉→归一",
            _ => "未知",
        }
    }
}

// ── 语言登记与 CJK 门 ───────────────────────────────────────────────

/// 语言字形密度类。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GlyphClass {
    /// 拉丁（稀疏）。
    Latin,
    /// CJK（密集——门更高）。
    Cjk,
    /// 阿拉伯（连写）。
    Arabic,
}

/// 语言条目。
#[derive(Debug, Clone)]
pub struct LangEntry {
    /// BCP47 主语言。
    pub lang: String,
    pub class: GlyphClass,
    /// 基线对比度（:1）。
    pub contrast: u32,
    /// 基线可读字号（px）。
    pub font_px: u16,
}

/// CJK 门：密集字形对比度要求更高。
pub const LATIN_CONTRAST_GATE: u32 = 45;
pub const CJK_CONTRAST_GATE: u32 = 52;

impl GlyphClass {
    pub fn gate(self) -> u32 {
        match self {
            GlyphClass::Latin | GlyphClass::Arabic => LATIN_CONTRAST_GATE,
            GlyphClass::Cjk => CJK_CONTRAST_GATE,
        }
    }
}

/// 语言对比断言册。
#[derive(Debug, Clone)]
pub struct LangContrastBook {
    pub entries: Vec<LangEntry>,
}

impl LangContrastBook {
    pub fn new() -> Self {
        Self { entries: Vec::new() }
    }

    pub fn put(&mut self, lang: &str, class: GlyphClass, contrast: u32, font_px: u16) {
        self.entries.push(LangEntry {
            lang: String::from(lang),
            class,
            contrast,
            font_px,
        });
    }

    /// 语言分维扫描：逐语言过门。
    pub fn scan(&self) -> Vec<(&str, T21ICode)> {
        let mut out = Vec::new();
        for e in self.entries.iter() {
            let code = if e.contrast >= e.class.gate() {
                T21ICode::OK
            } else {
                T21ICode::CJK_GATE
            };
            out.push((e.lang.as_str(), code));
        }
        out
    }

    /// 全过（阻断依据）。
    pub fn all_pass(&self) -> bool {
        self.scan().iter().all(|(_, c)| *c == T21ICode::OK)
    }
}

// ── 语言跟随（读屏语言随 UI 语言——F3856 复述单源）─────────────────

/// 语言跟随记录：UI 语言 → 读屏语言。
#[derive(Debug, Clone)]
pub struct FollowRecord {
    pub ui_lang: String,
    pub tts_lang: String,
}

/// 跟随核验（单源比对表）。
#[derive(Debug, Clone)]
pub struct FollowBook {
    pub records: Vec<FollowRecord>,
}

impl FollowBook {
    pub fn new() -> Self {
        Self { records: Vec::new() }
    }

    pub fn put(&mut self, ui: &str, tts: &str) {
        self.records.push(FollowRecord {
            ui_lang: String::from(ui),
            tts_lang: String::from(tts),
        });
    }

    /// 跟随破检出：UI 与读屏语言主子标签不一致。
    pub fn broken(&self) -> Vec<String> {
        self.records.iter()
            .filter(|r| {
                let ui_main = r.ui_lang.split('-').next().unwrap_or("");
                let tts_main = r.tts_lang.split('-').next().unwrap_or("");
                ui_main != tts_main
            })
            .map(|r| r.ui_lang.clone())
            .collect()
    }
}

// ── 字号适配表 ───────────────────────────────────────────────────────

pub const FONT_TABLE_LANGS: [&str; 5] = ["en", "zh", "ar", "de", "th"];

/// 字号适配表（语言 → 可读字号下限）。
#[derive(Debug, Clone)]
pub struct FontTable {
    pub rows: Vec<(&'static str, u16)>,
}

pub const BASE_FONT_PX: u16 = 16;
/// 泰文需上标元音空间 → 下限更高。
pub const TH_FONT_PX: u16 = 18;
/// CJK 密集笔画 → 下限更高。
pub const CJK_FONT_PX: u16 = 17;

impl FontTable {
    pub fn standard() -> Self {
        let mut rows = Vec::new();
        for l in FONT_TABLE_LANGS.iter() {
            let px = match *l {
                "th" => TH_FONT_PX,
                "zh" => CJK_FONT_PX,
                _ => BASE_FONT_PX,
            };
            rows.push((*l, px));
        }
        Self { rows }
    }

    pub fn get(&self, lang: &str) -> Option<u16> {
        self.rows.iter().find(|(l, _)| *l == lang).map(|(_, px)| *px)
    }

    /// 表漏检出：五代表语言缺一即漏。
    pub fn missing(&self) -> Vec<&'static str> {
        FONT_TABLE_LANGS.iter().filter(|l| self.get(l).is_none()).map(|l| *l).collect()
    }
}

// ── 判据 ─────────────────────────────────────────────────────────────

fn chk(set: &mut crate::checks::CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

pub fn run_ves255_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("ves255_s11a11y");

    // C4015-01 诊断码段不越窗。
    chk(&mut set, "C4015-01",
        T21ICode::SOURCE_SPLIT.0 < SEG_BASE + 16 && SEG_BASE == 0x6440, "err");

    // C4015-02 CJK 门高于拉丁门。
    chk(&mut set, "C4015-02",
        GlyphClass::Cjk.gate() == 52 && GlyphClass::Latin.gate() == 45, "err");

    // C4015-03 语言分维扫描：拉丁过 CJK 拦。
    let mut b = LangContrastBook::new();
    b.put("en", GlyphClass::Latin, 46, 16);
    b.put("zh", GlyphClass::Cjk, 48, 17);
    let scan = b.scan();
    chk(&mut set, "C4015-03",
        scan[0].1 == T21ICode::OK && scan[1].1 == T21ICode::CJK_GATE && !b.all_pass(), "err");

    // C4015-04 CJK 提对比后全过。
    b.put("zh", GlyphClass::Cjk, 52, 17);
    // 覆盖式重建（put 追加，重扫取末条语义——用新册更清晰）。
    let mut b2 = LangContrastBook::new();
    b2.put("en", GlyphClass::Latin, 46, 16);
    b2.put("zh", GlyphClass::Cjk, 52, 17);
    b2.put("ar", GlyphClass::Arabic, 45, 16);
    chk(&mut set, "C4015-04", b2.all_pass(), "err");

    // C4015-05 语言跟随：一致过。
    let mut f = FollowBook::new();
    f.put("zh-CN", "zh-CN");
    f.put("en-US", "en-US");
    chk(&mut set, "C4015-05", f.broken().is_empty(), "err");

    // C4015-06 语言跟随破检出（P1）。
    f.put("ar-SA", "en-US");
    chk(&mut set, "C4015-06", f.broken() == vec![String::from("ar-SA")], "err");

    // C4015-07 字号适配表标准五语言齐。
    let t = FontTable::standard();
    chk(&mut set, "C4015-07", t.missing().is_empty() && t.rows.len() == 5, "err");

    // C4015-08 泰文与 CJK 下限更高。
    chk(&mut set, "C4015-08",
        t.get("th") == Some(18) && t.get("zh") == Some(17) && t.get("en") == Some(16), "err");

    // C4015-09 表漏检出（缺 th）。
    let mut t2 = FontTable { rows: Vec::new() };
    for l in FONT_TABLE_LANGS.iter().filter(|l| **l != "th") {
        t2.rows.push((l, 16));
    }
    chk(&mut set, "C4015-09", t2.missing() == vec!["th"], "err");

    // C4015-10 门值钳制：对比度恰等门值过。
    let mut b3 = LangContrastBook::new();
    b3.put("zh", GlyphClass::Cjk, 52, 17);
    chk(&mut set, "C4015-10", b3.all_pass(), "err");

    // C4015-11 单源复述：语言类槽互异。
    chk(&mut set, "C4015-11",
        GlyphClass::Latin != GlyphClass::Cjk
            && GlyphClass::Cjk != GlyphClass::Arabic
            && GlyphClass::Latin != GlyphClass::Arabic, "err");

    // C4015-12 分维扫描顺序保持。
    let b4_order: Vec<&str> = b2.scan().iter().map(|(l, _)| *l).collect();
    chk(&mut set, "C4015-12", b4_order == vec!["en", "zh", "ar"], "err");

    // C4015-13 跟随表空不误报。
    chk(&mut set, "C4015-13", FollowBook::new().broken().is_empty(), "err");

    // C4015-14 字号表查询越名 None。
    chk(&mut set, "C4015-14", FontTable::standard().get("xx") == None, "err");

    // C4015-15 错误码互异。
    let codes = [T21ICode::OK.0, T21ICode::CJK_GATE.0, T21ICode::FOLLOW_BROKEN.0,
        T21ICode::TABLE_MISS.0, T21ICode::SOURCE_SPLIT.0];
    let mut uniq = true;
    for i in 0..codes.len() {
        for j in (i + 1)..codes.len() {
            if codes[i] == codes[j] { uniq = false; }
        }
    }
    chk(&mut set, "C4015-15", uniq, "err");

    // C4015-16 CJK 门语义（阻断）。
    chk(&mut set, "C4015-16", T21ICode::CJK_GATE.explain().contains("阻断"), "err");

    // C4015-17 空对比册全过（无语言无断言）。
    chk(&mut set, "C4015-17", LangContrastBook::new().all_pass(), "err");

    // C4015-18 字号下限单调（CJK ≥ BASE ≥? 泰文最高）。
    chk(&mut set, "C4015-18", TH_FONT_PX > CJK_FONT_PX && CJK_FONT_PX > BASE_FONT_PX, "err");

    // C4015-19 跟随记录双语登记。
    f.put("de-DE", "de-DE");
    chk(&mut set, "C4015-19", f.records.len() == 4, "err");

    // C4015-20 五代表语言在标准表全部可查。
    chk(&mut set, "C4015-20",
        FONT_TABLE_LANGS.iter().all(|l| FontTable::standard().get(l).is_some()), "err");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cjk_gate_blocks_low_contrast() {
        let mut b = LangContrastBook::new();
        b.put("zh", GlyphClass::Cjk, 45, 17);
        assert!(!b.all_pass());
        assert_eq!(b.scan()[0].1, T21ICode::CJK_GATE);
    }

    #[test]
    fn follow_detects_mismatch() {
        let mut f = FollowBook::new();
        f.put("zh-CN", "en-US");
        assert_eq!(f.broken().len(), 1);
    }

    #[test]
    fn font_table_standard_complete() {
        let t = FontTable::standard();
        assert!(t.missing().is_empty());
        assert_eq!(t.get("th"), Some(18));
    }
}
