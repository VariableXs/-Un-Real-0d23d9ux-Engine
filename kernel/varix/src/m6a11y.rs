//! m6a11y — VARIX-M600 AI-23 无障碍与全球化域 (F551~F575)
//!
//! 屏幕阅读器、全键盘、焦点规范、高对比、放大镜、语音控制、字幕、
//! 实时转写、单手模式、认知辅助、阅读字体、色觉适配、i18n 框架、
//! 伪本地化、RTL、字体回退、时区诚信、度量衡、输入法谱、语音合成、
//! 审计机器人、WCAG、回归走廊、本地化质量门、包容年报。
//!
//! 硬约束：no_std / 无 alloc / 无浮点（全部 permille / 整数）。

use crate::checks::CheckSet;

// ===========================================================================
// F551 — 屏幕阅读器
// ===========================================================================

#[derive(Clone, Copy)]
pub struct A11yNode {
    pub role: u8, // 0=button 1=label 2=text 3=image 4=heading 5=unknown
    pub label_id: u32,
    pub child_count: u8,
}

pub const ROLE_NAMES: [&str; 6] = ["button", "label", "text", "image", "heading", "unknown"];

/// 可朗读：有名字的节点；图片必须有替代文本。
pub fn node_announceable(n: &A11yNode) -> bool {
    if n.role == 3 {
        n.label_id != 0
    } else {
        n.role != 5 && n.label_id != 0
    }
}

// ===========================================================================
// F552 — 全键盘操作
// ===========================================================================

pub const HOTKEY_MAX_PER_CTX: u8 = 12;

#[derive(Clone, Copy)]
pub struct KeyBinding {
    pub ctx: u8,
    pub key: u16,
    pub action: u16,
}

/// 同一上下文内按键不得重复；数量有上限。
pub fn keymap_valid(bindings: &[KeyBinding], n: usize) -> bool {
    let n = n.min(bindings.len());
    // 按 ctx 统计
    let mut ctx_count = [0u8; 16];
    for b in &bindings[..n] {
        let c = (b.ctx as usize) % 16;
        ctx_count[c] += 1;
        if ctx_count[c] > HOTKEY_MAX_PER_CTX {
            return false;
        }
    }
    for i in 0..n {
        for j in (i + 1)..n {
            if bindings[i].ctx == bindings[j].ctx && bindings[i].key == bindings[j].key {
                return false;
            }
        }
    }
    true
}

// ===========================================================================
// F553 — 焦点管理规范
// ===========================================================================

#[derive(Clone, Copy)]
pub struct FocusRing {
    pub order: [u16; 8],
    pub len: usize,
    pub pos: usize,
}

impl FocusRing {
    pub fn next(&mut self) -> Option<u16> {
        if self.len == 0 {
            return None;
        }
        self.pos = (self.pos + 1) % self.len;
        Some(self.order[self.pos])
    }
    pub fn prev(&mut self) -> Option<u16> {
        if self.len == 0 {
            return None;
        }
        self.pos = (self.pos + self.len - 1) % self.len;
        Some(self.order[self.pos])
    }
}

pub fn focus_ring_sane(r: &FocusRing) -> bool {
    r.len > 0 && r.len <= 8 && r.pos < r.len
}

// ===========================================================================
// F554 — 高对比模式
// ===========================================================================

/// 对比度 permille（0~1000，WCAG AA 正文要求 >= 700，AAA >= 850）。
pub fn contrast_grade(contrast_permille: u16) -> u8 {
    if contrast_permille >= 850 {
        2 // AAA
    } else if contrast_permille >= 700 {
        1 // AA
    } else {
        0 // fail
    }
}

// ===========================================================================
// F555 — 放大镜工坊
// ===========================================================================

pub const MAG_MIN_PERMILLE: u16 = 1000; // 1.0x
pub const MAG_MAX_PERMILLE: u16 = 4000; // 4.0x

pub struct Magnifier {
    pub zoom_permille: u16,
    pub follow_caret: bool,
}

impl Magnifier {
    pub fn set_zoom(&mut self, z: u16) -> bool {
        if (MAG_MIN_PERMILLE..=MAG_MAX_PERMILLE).contains(&z) {
            self.zoom_permille = z;
            true
        } else {
            false
        }
    }
}

// ===========================================================================
// F556 — 语音控制
// ===========================================================================

#[derive(Clone, Copy, PartialEq)]
pub enum VoiceCmd {
    Tap,
    Scroll,
    Back,
    Open,
    Unknown,
}

pub fn voice_route(_slots: u8, words: u8) -> VoiceCmd {
    if words & 0b0001 != 0 {
        VoiceCmd::Tap
    } else if words & 0b0010 != 0 {
        VoiceCmd::Scroll
    } else if words & 0b0100 != 0 {
        VoiceCmd::Back
    } else if words & 0b1000 != 0 {
        VoiceCmd::Open
    } else {
        VoiceCmd::Unknown
    }
}

// ===========================================================================
// F557 — 字幕系统
// ===========================================================================

pub const SUB_TITLE_MAX_LEN: usize = 42; // 每行字幕字符上限

pub struct Cue {
    pub t0_ms: u32,
    pub t1_ms: u32,
    pub text_len: usize,
}

pub fn cue_ok(c: &Cue) -> bool {
    c.t1_ms > c.t0_ms && c.text_len > 0 && c.text_len <= SUB_TITLE_MAX_LEN * 2
}

/// 字幕与音轨重叠检测。
pub fn cue_overlap(a: &Cue, b: &Cue) -> bool {
    a.t0_ms < b.t1_ms && b.t0_ms < a.t1_ms
}

// ===========================================================================
// F558 — 实时转写
// ===========================================================================

pub const TRANSCRIBE_LATENCY_BUDGET_MS: u32 = 300;

pub fn transcribe_latency_ok(speech_ms: u32, render_ms: u32) -> bool {
    speech_ms.saturating_add(render_ms) <= TRANSCRIBE_LATENCY_BUDGET_MS
}

// ===========================================================================
// F559 — 单手模式
// ===========================================================================

/// 交互元素下沉（下移 permille）使拇指可达。
pub fn onehand_shift(screen_h_permille: u16, shift_permille: u16) -> u16 {
    let shifted = screen_h_permille.saturating_sub(shift_permille);
    shifted.clamp(200, 1000)
}

// ===========================================================================
// F560 — 认知辅助
// ===========================================================================

#[derive(Clone, Copy, PartialEq)]
pub enum CogAid {
    StepByStep,
    Reminders,
    FocusMode,
    None,
}

pub fn cognitive_aid(task_steps: u32, deadline_min: u32, distraction_events: u32) -> CogAid {
    if distraction_events >= 5 {
        CogAid::FocusMode
    } else if task_steps > 10 {
        CogAid::StepByStep
    } else if deadline_min > 0 {
        CogAid::Reminders
    } else {
        CogAid::None
    }
}

// ===========================================================================
// F561 — 阅读障碍字体
// ===========================================================================

pub const DYSLEXIC_SPACING_PERMILLE: u16 = 150; // 字距加大 15%

pub fn dyslexic_layout_ok(spacing_permille: u16, line_height_permille: u16) -> bool {
    spacing_permille >= DYSLEXIC_SPACING_PERMILLE && line_height_permille >= 1500
}

// ===========================================================================
// F562 — 色觉全谱适配
// ===========================================================================

#[derive(Clone, Copy, PartialEq)]
pub enum ColorVision {
    Normal,
    Protan,   // 红色弱
    Deutan,   // 绿色弱
    Tritan,   // 蓝色弱
    Achroma,  // 全色盲
}

/// 红绿依赖的信息必须改用形状/纹理冗余编码。
pub fn color_adapt(cv: ColorVision) -> bool {
    !matches!(cv, ColorVision::Normal)
}

// ===========================================================================
// F563 — 国际化框架
// ===========================================================================

pub const I18N_KEYS_MIN: usize = 200;

pub struct I18nCatalog {
    pub lang: [u8; 8],
    pub keys: usize,
    pub translated: usize,
}

impl I18nCatalog {
    pub fn coverage_permille(&self) -> u16 {
        if self.keys == 0 {
            return 0;
        }
        ((self.translated as u32 * 1000) / self.keys as u32) as u16
    }
    pub fn complete(&self) -> bool {
        self.translated == self.keys && self.keys >= I18N_KEYS_MIN
    }
}

/// 复数规则：slavic 类需要 3 种复数形式。
pub fn plural_forms_needed(lang_family: u8) -> u8 {
    match lang_family {
        0 => 2, // en/zh 简单
        1 => 3, // slavic
        2 => 4, // arabic
        _ => 1,
    }
}

// ===========================================================================
// F564 — 伪本地化测试
// ===========================================================================

/// 伪翻译膨胀率 30%~40% 用于暴露截断。
pub fn pseudo_expand_len(orig: usize) -> usize {
    orig + orig * 35 / 100
}

pub fn pseudo_exposes_truncation(orig: usize, ui_max: usize) -> bool {
    pseudo_expand_len(orig) > ui_max
}

// ===========================================================================
// F565 — RTL 布局
// ===========================================================================

#[derive(Clone, Copy, PartialEq)]
pub enum Dir {
    Ltr,
    Rtl,
}

/// 镜像：start<->end，但数字/代码块不镜像。
pub fn rtl_mirror(logical_start: u16, width: u16) -> u16 {
    width.saturating_sub(logical_start)
}

pub const RTL_LANGS: [&str; 3] = ["ar", "he", "fa"];

pub fn is_rtl(lang: &str) -> bool {
    RTL_LANGS.iter().any(|l| *l == lang)
}

// ===========================================================================
// F566 — 字体回退链
// ===========================================================================

pub const FONT_FALLBACK: [&str; 5] = ["sans", "cjk", "arabic", "emoji", "symbol"];

pub fn font_fallback_covers(scripts: &[&str], n: usize) -> bool {
    let n = n.min(scripts.len());
    scripts[..n].iter().all(|s| FONT_FALLBACK.iter().any(|f| f == s))
}

// ===========================================================================
// F567 — 日期时区诚信
// ===========================================================================

/// 内核只存 UTC；显示层换算。
pub fn utc_to_local_offset(utc_epoch: u64, offset_min: i32) -> u64 {
    if offset_min >= 0 {
        utc_epoch + (offset_min as u64) * 60
    } else {
        utc_epoch.saturating_sub((-offset_min) as u64 * 60)
    }
}

pub fn tz_offset_valid(offset_min: i32) -> bool {
    (-14 * 60..=14 * 60).contains(&offset_min) && offset_min % 15 == 0
}

// ===========================================================================
// F568 — 度量衡文化
// ===========================================================================

#[derive(Clone, Copy, PartialEq)]
pub enum Measure {
    Metric,
    Imperial,
}

pub fn measure_for(lang: &str) -> Measure {
    match lang {
        "en-US" => Measure::Imperial,
        _ => Measure::Metric,
    }
}

/// 温度换算（×10 整数）：C -> F。
pub fn celsius_to_fahren_x10(c_x10: i32) -> i32 {
    c_x10 * 9 / 5 + 320
}

// ===========================================================================
// F569 — 输入法全球谱
// ===========================================================================

pub const IME_KINDS: [&str; 6] = ["pinyin", "romaji", "hangul", "arabic", "devanagari", "latin"];

pub fn ime_supported(kind: &str) -> bool {
    IME_KINDS.iter().any(|k| *k == kind)
}

// ===========================================================================
// F570 — 语音合成多语种
// ===========================================================================

pub const TTS_LANGS: [&str; 5] = ["zh", "en", "ja", "ko", "de"];

pub fn tts_available(lang: &str) -> bool {
    TTS_LANGS.iter().any(|l| *l == lang)
}

/// 语速 permille：50%~200%。
pub fn tts_rate_valid(rate_permille: u16) -> bool {
    (500..=2000).contains(&rate_permille)
}

// ===========================================================================
// F571 — 无障碍审计机器人
// ===========================================================================

#[derive(Clone, Copy)]
pub struct A11yIssue {
    pub node: u32,
    pub kind: u8, // 0=missing-label 1=low-contrast 2=no-kbd 3=focus-trap
    pub severity: u8, // 0=info 1=warn 2=crit
}

pub fn audit_issues(issues: &[A11yIssue], n: usize) -> usize {
    let n = n.min(issues.len());
    issues[..n].iter().filter(|i| i.severity == 2).count()
}

pub fn audit_gate(issues: &[A11yIssue], n: usize) -> bool {
    audit_issues(issues, n) == 0
}

// ===========================================================================
// F572 — WCAG 合规清单
// ===========================================================================

pub const WCAG_CHECKS: [&str; 8] = [
    "perceivable", "operable", "understandable", "robust",
    "contrast-aa", "keyboard-all", "focus-visible", "text-resize",
];

pub fn wcag_score(marks: u8) -> u16 {
    (((marks as u32 * 1000) / WCAG_CHECKS.len() as u32).min(1000)) as u16
}

pub fn wcag_aa_pass(marks: u8) -> bool {
    marks as u32 == (1u32 << WCAG_CHECKS.len()) - 1
}

// ===========================================================================
// F573 — 无障碍回归走廊
// ===========================================================================

pub const A11Y_CORRIDOR_CASES: [&str; 5] =
    ["reader-tree", "kbd-fullpath", "contrast-aa", "focus-ring", "rtl-mirror"];

pub fn a11y_corridor_pass(results: &[bool; 5]) -> bool {
    results.iter().all(|&r| r)
}

// ===========================================================================
// F574 — 本地化质量门
// ===========================================================================

pub struct LocQualityGate {
    pub coverage_permille: u16,
    pub pseudo_pass: bool,
    pub rtl_pass: bool,
}

impl LocQualityGate {
    pub fn pass(&self) -> bool {
        self.coverage_permille >= 990 && self.pseudo_pass && self.rtl_pass
    }
}

// ===========================================================================
// F575 — 包容年报
// ===========================================================================

pub struct InclusionYearbook {
    pub a11y_issues_open: u32,
    pub a11y_issues_fixed: u32,
    pub langs: u32,
    pub wcag_score_permille: u16,
}

impl InclusionYearbook {
    pub fn fix_rate_permille(&self) -> u16 {
        let total = self.a11y_issues_open + self.a11y_issues_fixed;
        if total == 0 {
            return 1000;
        }
        ((self.a11y_issues_fixed as u32 * 1000) / total) as u16
    }
    pub fn grade(&self) -> u8 {
        if self.wcag_score_permille >= 1000 && self.langs >= 6 {
            0
        } else if self.wcag_score_permille >= 875 {
            1
        } else {
            2
        }
    }
}

// ===========================================================================
// 域自检
// ===========================================================================

pub fn run_m6a11y_checks() -> CheckSet {
    let mut set = CheckSet::new("m6a11y");

    // F551 阅读器
    let ok = A11yNode { role: 0, label_id: 5, child_count: 0 };
    let img_no_alt = A11yNode { role: 3, label_id: 0, child_count: 0 };
    set.add(
        "F551 reader",
        node_announceable(&ok) && !node_announceable(&img_no_alt),
        "image needs alt",
    );

    // F552 全键盘
    let b = [
        KeyBinding { ctx: 0, key: 1, action: 10 },
        KeyBinding { ctx: 0, key: 2, action: 11 },
        KeyBinding { ctx: 1, key: 1, action: 12 },
    ];
    let dup = [
        KeyBinding { ctx: 0, key: 1, action: 10 },
        KeyBinding { ctx: 0, key: 1, action: 11 },
    ];
    set.add("F552 keyboard", keymap_valid(&b, 3) && !keymap_valid(&dup, 2), "no dup per ctx");

    // F553 焦点环
    let mut ring = FocusRing { order: [1, 2, 3, 0, 0, 0, 0, 0], len: 3, pos: 0 };
    let n1 = ring.next();
    let p = ring.prev();
    set.add(
        "F553 focus ring",
        focus_ring_sane(&ring) && n1 == Some(2) && p == Some(1),
        "wrap around",
    );

    // F554 对比
    set.add(
        "F554 contrast",
        contrast_grade(900) == 2 && contrast_grade(750) == 1 && contrast_grade(500) == 0,
        "AA/AAA ladder",
    );

    // F555 放大镜
    let mut mag = Magnifier { zoom_permille: 1000, follow_caret: true };
    set.add(
        "F555 magnifier",
        mag.set_zoom(2500) && mag.zoom_permille == 2500 && !mag.set_zoom(5000) && !mag.set_zoom(500),
        "zoom clamp",
    );

    // F556 语音控制
    set.add(
        "F556 voice",
        voice_route(0, 0b0001) == VoiceCmd::Tap
            && voice_route(0, 0b0100) == VoiceCmd::Back
            && voice_route(0, 0) == VoiceCmd::Unknown,
        "word routing",
    );

    // F557 字幕
    let c1 = Cue { t0_ms: 0, t1_ms: 2000, text_len: 40 };
    let c2 = Cue { t0_ms: 1000, t1_ms: 3000, text_len: 10 };
    let c3 = Cue { t0_ms: 2500, t1_ms: 4000, text_len: 10 };
    set.add(
        "F557 subtitle",
        cue_ok(&c1) && !cue_ok(&Cue { t0_ms: 2000, t1_ms: 2000, text_len: 5 })
            && cue_overlap(&c1, &c2) && !cue_overlap(&c1, &c3),
        "valid + overlap",
    );

    // F558 转写
    set.add(
        "F558 transcribe",
        transcribe_latency_ok(200, 100) && !transcribe_latency_ok(200, 200),
        "300ms budget",
    );

    // F559 单手
    set.add(
        "F559 onehand",
        onehand_shift(1000, 300) == 700 && onehand_shift(1000, 2000) == 200,
        "shift clamp",
    );

    // F560 认知辅助
    set.add(
        "F560 cognitive",
        cognitive_aid(3, 0, 6) == CogAid::FocusMode
            && cognitive_aid(12, 0, 0) == CogAid::StepByStep
            && cognitive_aid(2, 30, 0) == CogAid::Reminders,
        "aid ladder",
    );

    // F561 阅读障碍
    set.add(
        "F561 dyslexic",
        dyslexic_layout_ok(150, 1600) && !dyslexic_layout_ok(100, 1600),
        "spacing rules",
    );

    // F562 色觉
    set.add(
        "F562 color vision",
        color_adapt(ColorVision::Deutan) && !color_adapt(ColorVision::Normal),
        "redundant encoding",
    );

    // F563 i18n
    let cat = I18nCatalog { lang: *b"zh-CN\0\0\0", keys: 250, translated: 250 };
    let part = I18nCatalog { lang: *b"de\0\0\0\0\0\0", keys: 250, translated: 200 };
    set.add(
        "F563 i18n",
        cat.complete() && !part.complete() && part.coverage_permille() == 800
            && plural_forms_needed(1) == 3 && plural_forms_needed(2) == 4,
        "coverage + plurals",
    );

    // F564 伪本地化
    set.add(
        "F564 pseudo",
        pseudo_expand_len(100) == 135 && pseudo_exposes_truncation(100, 120)
            && !pseudo_exposes_truncation(100, 140),
        "35% expand",
    );

    // F565 RTL
    set.add(
        "F565 rtl",
        is_rtl("ar") && !is_rtl("zh") && rtl_mirror(100, 800) == 700,
        "mirror start",
    );

    // F566 字体回退
    set.add(
        "F566 font fallback",
        font_fallback_covers(&["sans", "cjk", "emoji"], 3) && !font_fallback_covers(&["klingon"], 1),
        "chain coverage",
    );

    // F567 时区
    set.add(
        "F567 tz honest",
        utc_to_local_offset(1000, 480) == 1000 + 480 * 60
            && utc_to_local_offset(10_000, -60) == 10_000 - 3_600
            && tz_offset_valid(540) && !tz_offset_valid(541),
        "utc base + offset",
    );

    // F568 度量衡
    set.add(
        "F568 measure",
        measure_for("en-US") == Measure::Imperial && measure_for("zh") == Measure::Metric
            && celsius_to_fahren_x10(0) == 320 && celsius_to_fahren_x10(1000) == 2120,
        "unit culture",
    );

    // F569 输入法
    set.add("F569 ime", ime_supported("pinyin") && ime_supported("romaji") && !ime_supported("dvorak"), "global ime");

    // F570 TTS
    set.add(
        "F570 tts",
        tts_available("zh") && !tts_available("fr") && tts_rate_valid(1000) && !tts_rate_valid(100),
        "langs + rate",
    );

    // F571 审计机器人
    let issues = [
        A11yIssue { node: 1, kind: 0, severity: 2 },
        A11yIssue { node: 2, kind: 1, severity: 1 },
    ];
    let clean = [A11yIssue { node: 1, kind: 1, severity: 0 }];
    set.add("F571 audit bot", audit_issues(&issues, 2) == 1 && audit_gate(&clean, 1) && !audit_gate(&issues, 2), "crit blocks");

    // F572 WCAG
    set.add(
        "F572 wcag",
        wcag_score(0xFF) == 1000 && wcag_aa_pass(0xFF) && !wcag_aa_pass(0xFE) && WCAG_CHECKS.len() == 8,
        "all 8 checks",
    );

    // F573 回归走廊
    set.add(
        "F573 corridor",
        a11y_corridor_pass(&[true, true, true, true, true])
            && !a11y_corridor_pass(&[true, true, true, true, false]),
        "5 cases",
    );

    // F574 质量门
    let g = LocQualityGate { coverage_permille: 1000, pseudo_pass: true, rtl_pass: true };
    let bad = LocQualityGate { coverage_permille: 950, pseudo_pass: true, rtl_pass: true };
    set.add("F574 loc gate", g.pass() && !bad.pass(), "coverage>=990");

    // F575 年报
    let yb = InclusionYearbook { a11y_issues_open: 10, a11y_issues_fixed: 90, langs: 8, wcag_score_permille: 1000 };
    set.add(
        "F575 yearbook",
        yb.fix_rate_permille() == 900 && yb.grade() == 0,
        "fix rate + grade",
    );

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f553_focus_ring_wrap() {
        let mut ring = FocusRing { order: [7, 8, 9, 0, 0, 0, 0, 0], len: 3, pos: 2 };
        assert_eq!(ring.next(), Some(7));
        assert_eq!(ring.prev(), Some(9));
    }

    #[test]
    fn f567_utc_only_storage() {
        assert_eq!(utc_to_local_offset(0, 0), 0);
        assert_eq!(utc_to_local_offset(60, -60), 0);
    }

    #[test]
    fn f568_temp_conversion() {
        assert_eq!(celsius_to_fahren_x10(370), 986); // 37C -> 98.6F
    }

    #[test]
    fn f552_ctx_cap() {
        let mut b = [KeyBinding { ctx: 0, key: 0, action: 0 }; 13];
        for (i, x) in b.iter_mut().enumerate() {
            x.key = i as u16;
        }
        assert!(!keymap_valid(&b, 13));
    }

    #[test]
    fn f575_domain_selfcheck_all_pass() {
        let set = run_m6a11y_checks();
        assert!(set.len() >= 25, "got {}", set.len());
                    for i in 0..set.len() {
                let c = set.get(i).unwrap();
                if !c.passed {
                    eprintln!("CHKFAIL {} | {}", c.name, c.detail);
                }
            }
                        for i in 0..set.len() {
                let c = set.get(i).unwrap();
                if !c.passed {
                    eprintln!("CHKFAIL {} | {}", c.name, c.detail);
                }
            }
            assert!(set.all_passed());
    }
}
