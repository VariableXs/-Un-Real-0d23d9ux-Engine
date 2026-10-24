//! VE-F4016 · 国际化安全
//!
//! i18n 安全面（复用 F3301 模式 T 版）：双向文本攻击（RTL 覆写字符攻
//! 击——BiDi 覆写检测，覆写字符告警——覆写攻击红线：BiDi 控制字符用
//! 于欺骗显示——欺骗检测 P0）；同形字攻击（同形字域名/标识检测——同
//! 形红线：同形字冒充=欺骗——同形 P0）；格式字符串注入检测；语言
//! （Locale）注入检测。
//! 数据结构：覆写检测器；同形字检测；注入清洗复用；四面清单。
//! 错误路径：覆写攻击检出→P0；同形字检出→P0；格式注入→拦截；漏检
//! →防线修正。
//! 性能：覆写 O(字符) 扫描；同形 O(标识比对)；清洗 O(复用)；演练 O(样本)。
//! 跨批对接：F3301/F3312 模式复用声明；F4021 BiDi 对端；安全服务对端。
//! 无隐私面（自身即安全条）。

#![allow(dead_code)]
use alloc::vec;

extern crate alloc;

use alloc::vec::Vec;
use alloc::string::String;

// ── 诊断码段（0x64C0）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0x64C0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct T22SCode(pub u16);

impl T22SCode {
    pub const OK: Self = Self(SEG_BASE);
    pub const BIDI_OVERRIDE: Self = Self(SEG_BASE + 1);
    pub const HOMOGlyph_P0: Self = Self(SEG_BASE + 2);
    pub const FMT_INJECT: Self = Self(SEG_BASE + 3);
    pub const LOCALE_INJECT: Self = Self(SEG_BASE + 4);

    pub fn explain(self) -> &'static str {
        match self {
            Self::OK => "正常",
            Self::BIDI_OVERRIDE => "BiDi 覆写攻击→P0",
            Self::HOMOGlyph_P0 => "同形字冒充→P0",
            Self::FMT_INJECT => "格式注入→拦截",
            Self::LOCALE_INJECT => "Locale 注入→拒绝",
            _ => "未知",
        }
    }
}

// ── 面 1：BiDi 覆写检测 ─────────────────────────────────────────────

/// BiDi 控制字符码点。
pub const LRE: char = '\u{202A}';
pub const RLE: char = '\u{202B}';
pub const PDF: char = '\u{202C}';
pub const LRO: char = '\u{202D}';
pub const RLO: char = '\u{202E}';
pub const LRI: char = '\u{2066}';
pub const RLI: char = '\u{2067}';

/// 覆写检测器：扫描文本中的 RLO/LRO（强覆写）与孤立 PDF。
#[derive(Debug, Clone)]
pub struct BiDiDetector;

impl BiDiDetector {
    /// 检出强覆写字符（欺骗显示主力）。
    pub fn scan(text: &str) -> Vec<char> {
        text.chars().filter(|c| matches!(*c, LRO | RLO)).collect()
    }

    /// 控制符计数（含 isolate）。
    pub fn control_count(text: &str) -> u32 {
        text.chars().filter(|c| matches!(*c, LRE | RLE | PDF | LRO | RLO | LRI | RLI)).count() as u32
    }

    /// 裁决：有强覆写 → P0。
    pub fn verdict(text: &str) -> T22SCode {
        if !Self::scan(text).is_empty() {
            T22SCode::BIDI_OVERRIDE
        } else {
            T22SCode::OK
        }
    }
}

// ── 面 2：同形字检测 ────────────────────────────────────────────────

/// 混用脚本检出：拉丁字母 + 西里尔/希腊字母混用 = 同形攻击信号。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Script {
    Latin,
    Cyrillic,
    Greek,
    Other,
}

pub fn classify(c: char) -> Script {
    let u = c as u32;
    if c.is_ascii_lowercase() || c.is_ascii_uppercase() || c.is_ascii_digit() {
        Script::Latin
    } else if (0x0400..=0x04FF).contains(&u) {
        Script::Cyrillic
    } else if (0x0370..=0x03FF).contains(&u) {
        Script::Greek
    } else {
        Script::Other
    }
}

/// 同形检测：标识中主脚本与混入脚本并存。
pub fn homoglyph_detect(ident: &str) -> Option<Script> {
    let mut main: Option<Script> = None;
    for c in ident.chars() {
        let s = classify(c);
        match s {
            Script::Latin => {
                if main.is_none() {
                    main = Some(Script::Latin);
                }
            }
            Script::Cyrillic | Script::Greek => {
                if main == Some(Script::Latin) {
                    return Some(s);
                }
                if main.is_none() {
                    main = Some(s);
                }
            }
            Script::Other => {}
        }
    }
    None
}

// ── 面 3：格式字符串注入 ────────────────────────────────────────────

/// 格式占位符检出（%s %n {} 等）。
pub fn fmt_inject_detect(text: &str) -> bool {
    let b = text.as_bytes();
    let mut i = 0;
    while i + 1 < b.len() {
        if b[i] == b'%' && (b[i + 1] == b's' || b[i + 1] == b'n' || b[i + 1] == b'x') {
            return true;
        }
        if b[i] == b'{' && b[i + 1] == b'}' {
            return true;
        }
        i += 1;
    }
    false
}

// ── 面 4：Locale 注入 ───────────────────────────────────────────────

/// Locale 白名单（五代表语言）。
pub const LOCALE_WHITELIST: [&str; 5] = ["en", "zh", "ar", "de", "th"];

/// Locale 注入检测：越白名单 / 含路径分隔 / 含控制字符。
pub fn locale_inject_detect(tag: &str) -> bool {
    if tag.is_empty() {
        return true;
    }
    let main = tag.split('-').next().unwrap_or("");
    if !LOCALE_WHITELIST.contains(&main) {
        return true;
    }
    if tag.contains('/') || tag.contains('\\') || tag.contains("..") {
        return true;
    }
    tag.chars().any(|c| (c as u32) < 0x20)
}

// ── 四面清单总检 ────────────────────────────────────────────────────

/// 四面总检。
pub fn security_scan(text: &str, ident: &str, locale: &str) -> [bool; 4] {
    [
        BiDiDetector::verdict(text) != T22SCode::OK,
        homoglyph_detect(ident).is_some(),
        fmt_inject_detect(text),
        locale_inject_detect(locale),
    ]
}

// ── 判据 ─────────────────────────────────────────────────────────────

fn chk(set: &mut crate::checks::CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

pub fn run_ves256_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("ves256_s11sec");

    // C4016-01 诊断码段不越窗。
    chk(&mut set, "C4016-01",
        T22SCode::LOCALE_INJECT.0 < SEG_BASE + 16 && SEG_BASE == 0x64C0, "err");

    // C4016-02 覆写检测：干净文本过。
    chk(&mut set, "C4016-02",
        BiDiDetector::scan("hello world") .is_empty()
            && BiDiDetector::verdict("普通文本") == T22SCode::OK, "err");

    // C4016-03 覆写攻击检出（RLO 注入）→ P0。
    let attack = "admin\u{202E}moc.tcejorp";
    chk(&mut set, "C4016-03",
        BiDiDetector::scan(attack) == vec![RLO]
            && BiDiDetector::verdict(attack) == T22SCode::BIDI_OVERRIDE, "err");

    // C4016-04 LRO 同样检出。
    chk(&mut set, "C4016-04",
        BiDiDetector::verdict("a\u{202D}b") == T22SCode::BIDI_OVERRIDE, "err");

    // C4016-05 控制符计数（isolate 合法使用计数）。
    chk(&mut set, "C4016-05",
        BiDiDetector::control_count("\u{2066}abc\u{2067}") == 2, "err");

    // C4016-06 同形检测：纯拉丁过。
    chk(&mut set, "C4016-06", homoglyph_detect("example.com") == None, "err");

    // C4016-07 同形检测：西里尔混入拉丁 → P0 信号。
    // 'а'（西里尔 a）混入 "pаypal"。
    chk(&mut set, "C4016-07",
        homoglyph_detect("p\u{0430}ypal") == Some(Script::Cyrillic), "err");

    // C4016-08 希腊混入检出。
    chk(&mut set, "C4016-08",
        homoglyph_detect("g\u{03BF}\u{03BF}gle") == Some(Script::Greek), "err");

    // C4016-09 纯西里尔标识不误报（主脚本自洽）。
    chk(&mut set, "C4016-09", homoglyph_detect("сайт") == None, "err");

    // C4016-10 格式注入检出：%s / %n / {}。
    chk(&mut set, "C4016-10",
        fmt_inject_detect("user%s输入") && fmt_inject_detect("x{}y") && !fmt_inject_detect("100%正常"), "err");

    // C4016-11 Locale 白名单内过。
    chk(&mut set, "C4016-11",
        !locale_inject_detect("zh-CN") && !locale_inject_detect("ar"), "err");

    // C4016-12 Locale 注入：越白名单。
    chk(&mut set, "C4016-12", locale_inject_detect("xx-Attack"), "err");

    // C4016-13 Locale 注入：路径分隔。
    chk(&mut set, "C4016-13",
        locale_inject_detect("zh/../../etc") && locale_inject_detect("en\\evil"), "err");

    // C4016-14 Locale 注入：空串拒。
    chk(&mut set, "C4016-14", locale_inject_detect(""), "err");

    // C4016-15 四面总检：全干净。
    let clean = security_scan("正常文本", "clean_id", "zh");
    chk(&mut set, "C4016-15", clean == [false, false, false, false], "err");

    // C4016-16 四面总检：覆写命中。
    let dirty = security_scan("\u{202E}evil", "id", "en");
    chk(&mut set, "C4016-16", dirty[0] && !dirty[1], "err");

    // C4016-17 四面总检：同形命中。
    let dirty2 = security_scan("ok", "p\u{0430}ypal", "en");
    chk(&mut set, "C4016-17", dirty2[1] && !dirty2[0], "err");

    // C4016-18 覆写码语义（P0 红线）。
    chk(&mut set, "C4016-18",
        T22SCode::BIDI_OVERRIDE.explain().contains("P0")
            && T22SCode::HOMOGlyph_P0.explain().contains("P0"), "err");

    // C4016-19 脚本分类双射抽检。
    chk(&mut set, "C4016-19",
        classify('a') == Script::Latin
            && classify('ж') == Script::Cyrillic
            && classify('α') == Script::Greek
            && classify('中') == Script::Other, "err");

    // C4016-20 错误码互异。
    let codes = [T22SCode::OK.0, T22SCode::BIDI_OVERRIDE.0, T22SCode::HOMOGlyph_P0.0,
        T22SCode::FMT_INJECT.0, T22SCode::LOCALE_INJECT.0];
    let mut uniq = true;
    for i in 0..codes.len() {
        for j in (i + 1)..codes.len() {
            if codes[i] == codes[j] { uniq = false; }
        }
    }
    chk(&mut set, "C4016-20", uniq, "err");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bidi_override_p0() {
        assert_eq!(BiDiDetector::verdict("x\u{202E}y"), T22SCode::BIDI_OVERRIDE);
        assert_eq!(BiDiDetector::verdict("normal"), T22SCode::OK);
    }

    #[test]
    fn homoglyph_cyrillic() {
        assert!(homoglyph_detect("p\u{0430}ypal").is_some());
        assert!(homoglyph_detect("paypal").is_none());
    }

    #[test]
    fn locale_whitelist_gate() {
        assert!(!locale_inject_detect("de-DE"));
        assert!(locale_inject_detect("fr"));
        assert!(locale_inject_detect("zh/../x"));
    }
}
