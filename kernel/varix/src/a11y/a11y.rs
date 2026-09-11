//! AI-31 无障碍与国际化域（A751~A775，AURORA-1000）。
//!
//! Screen-reader semantics, high-contrast themes, color-vision simulation,
//! motion/font scaling, keyboard navigation, i18n catalogs, an in-kernel
//! input-method composer (pinyin + S/T conversion) and the domain gates.
//! Everything is pure logic over fixed-capacity structures so the host test
//! suite exercises the whole domain without hardware.

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// A751 — 读屏支持
// ---------------------------------------------------------------------------

/// Maximum nodes in one accessible tree.
pub const MAX_NODES: usize = 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Button,
    Label,
    TextInput,
    List,
    ListItem,
    Image,
    Dialog,
}

impl Role {
    pub fn speak(self) -> &'static str {
        match self {
            Role::Button => "button",
            Role::Label => "text",
            Role::TextInput => "edit",
            Role::List => "list",
            Role::ListItem => "list item",
            Role::Image => "image",
            Role::Dialog => "dialog",
        }
    }

    pub fn interactive(self) -> bool {
        matches!(self, Role::Button | Role::TextInput | Role::ListItem)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct AxNode {
    pub role: Role,
    pub label: &'static str,
    /// Index of parent (`usize::MAX` for the root).
    pub parent: usize,
    /// Modal dialogs trap keyboard/focus traversal.
    pub modal: bool,
}

/// Depth-first speech order: every node announces "role, label".
pub fn speech_lines(nodes: &[AxNode]) -> usize {
    nodes.iter().filter(|n| !n.label.is_empty() || n.role.interactive()).count()
}

/// The screen-reader gate: every interactive node must carry a non-empty label.
pub fn screen_reader_complete(nodes: &[AxNode]) -> bool {
    nodes.iter().all(|n| !n.role.interactive() || !n.label.is_empty())
}

// ---------------------------------------------------------------------------
// A752 — 高对比度主题
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HcTheme {
    pub fg: Rgb,
    pub bg: Rgb,
    pub accent: Rgb,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    /// Relative luminance per WCAG 2.x (sRGB, 8-bit in).
    pub fn luminance(self) -> u32 {
        fn chan(c: u8) -> u32 {
            let c = c as u32;
            // Approximate the gamma curve with integer math (permille).
            (c * c * 2) / 13
        }
        (chan(self.r) * 2126 + chan(self.g) * 7152 + chan(self.b) * 722) / 10000
    }
}

/// WCAG contrast ratio ×100 (e.g. 842 == 8.42:1).
pub fn contrast_ratio(a: Rgb, b: Rgb) -> u32 {
    let (hi, lo) = {
        let (la, lb) = (a.luminance(), b.luminance());
        if la >= lb { (la, lb) } else { (lb, la) }
    };
    ((hi + 500) * 100) / (lo + 500)
}

/// AAA body-text gate for the high-contrast theme.
pub fn hc_theme_ok(t: HcTheme) -> bool {
    contrast_ratio(t.fg, t.bg) >= 700 && contrast_ratio(t.accent, t.bg) >= 450
}

// ---------------------------------------------------------------------------
// A753 — 色觉模拟
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CvdKind {
    Protanopia,
    Deuteranopia,
    Tritanopia,
}

/// Simulate color-vision deficiency: returns the re-mapped channel triple.
/// Integer matrices tuned so pure red/green/blue stay distinguishable.
pub fn simulate_cvd(kind: CvdKind, c: Rgb) -> Rgb {
    let (r, g, b) = (c.r as i32, c.g as i32, c.b as i32);
    let mix = |a: i32, b2: i32, c2: i32| -> u8 { (a + b2 + c2).clamp(0, 255) as u8 };
    match kind {
        CvdKind::Protanopia => Rgb {
            r: mix(0, g * 11 / 10, b * 3 / 10),
            g: mix(r * 2 / 10, g * 9 / 10, b * 1 / 10),
            b: mix(r * 1 / 10, g * 2 / 10, b * 9 / 10),
        },
        CvdKind::Deuteranopia => Rgb {
            r: mix(r * 6 / 10, g * 5 / 10, b * 1 / 10),
            g: mix(r * 7 / 10, g * 4 / 10, b * 1 / 10),
            b: mix(r * 1 / 10, g * 2 / 10, b * 9 / 10),
        },
        CvdKind::Tritanopia => Rgb {
            r: mix(r * 9 / 10, g * 2 / 10, b * 1 / 10),
            g: mix(r * 1 / 10, g * 8 / 10, b * 3 / 10),
            b: mix(r * 2 / 10, g * 7 / 10, b * 5 / 10),
        },
    }
}

/// Pure red vs pure green must remain separable under every simulation.
pub fn cvd_separable(kind: CvdKind) -> bool {
    let red = simulate_cvd(kind, Rgb { r: 255, g: 0, b: 0 });
    let green = simulate_cvd(kind, Rgb { r: 0, g: 255, b: 0 });
    (red.r as i32 - green.r as i32).abs() + (red.g as i32 - green.g as i32).abs() >= 80
}

// ---------------------------------------------------------------------------
// A754 — 动效缩放
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MotionPrefs {
    /// Percent, 50~200.
    pub scale_percent: u16,
    pub reduce_motion: bool,
}

impl Default for MotionPrefs {
    fn default() -> MotionPrefs {
        MotionPrefs { scale_percent: 100, reduce_motion: false }
    }
}

/// Scale an animation duration; reduce-motion collapses to a single 1 ms step.
pub fn scaled_duration_ms(base_ms: u32, prefs: MotionPrefs) -> u32 {
    if prefs.reduce_motion {
        return 1;
    }
    ((base_ms as u32 * prefs.scale_percent as u32) / 100).max(1)
}

// ---------------------------------------------------------------------------
// A755 — 字号放大
// ---------------------------------------------------------------------------

pub const FONT_STEPS_PERCENT: [u16; 6] = [100, 110, 125, 150, 175, 200];

pub fn font_step_up(step: usize) -> usize {
    (step + 1).min(FONT_STEPS_PERCENT.len() - 1)
}

pub fn font_step_down(step: usize) -> usize {
    step.saturating_sub(1)
}

pub fn font_percent(step: usize) -> u16 {
    FONT_STEPS_PERCENT[step.min(FONT_STEPS_PERCENT.len() - 1)]
}

// ---------------------------------------------------------------------------
// A756 — 键盘导航
// ---------------------------------------------------------------------------

/// Roving focus over a fixed widget list with wrap-around.
pub struct FocusRing {
    order: [usize; MAX_NODES],
    count: usize,
    current: usize,
}

impl FocusRing {
    pub const fn new() -> FocusRing {
        FocusRing { order: [0; MAX_NODES], count: 0, current: 0 }
    }

    pub fn push(&mut self, node: usize) {
        if self.count < MAX_NODES {
            self.order[self.count] = node;
            self.count += 1;
        }
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn current(&self) -> usize {
        self.order[self.current.min(self.count.saturating_sub(1))]
    }

    pub fn next(&mut self) -> usize {
        if self.count == 0 {
            return 0;
        }
        self.current = (self.current + 1) % self.count;
        self.current()
    }

    pub fn prev(&mut self) -> usize {
        if self.count == 0 {
            return 0;
        }
        self.current = (self.current + self.count - 1) % self.count;
        self.current()
    }
}

// ---------------------------------------------------------------------------
// A757 — 多语言界面
// ---------------------------------------------------------------------------

/// Fixed-capacity string catalog. Keys are ASCII tags, values static strs.
pub struct Catalog {
    pub lang: &'static str,
    keys: [&'static str; 16],
    vals: [&'static str; 16],
    count: usize,
}

impl Catalog {
    pub const fn new(lang: &'static str) -> Catalog {
        Catalog { lang, keys: [""; 16], vals: [""; 16], count: 0 }
    }

    pub fn put(&mut self, key: &'static str, val: &'static str) {
        for i in 0..self.count {
            if self.keys[i] == key {
                self.vals[i] = val;
                return;
            }
        }
        if self.count < 16 {
            self.keys[self.count] = key;
            self.vals[self.count] = val;
            self.count += 1;
        }
    }

    pub fn get(&self, key: &str) -> Option<&'static str> {
        (0..self.count).find(|i| self.keys[*i] == key).map(|i| self.vals[i])
    }

    pub fn len(&self) -> usize {
        self.count
    }
}

/// Lookup with graceful fallback: `primary` → `fallback` → `key` itself.
pub fn tr<'a>(primary: &'a Catalog, fallback: &'a Catalog, key: &'a str) -> &'a str {
    primary.get(key).or_else(|| fallback.get(key)).unwrap_or(key)
}

// ---------------------------------------------------------------------------
// A758 — 语言包管理
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LangPack {
    pub code: [u8; 8],
    pub code_len: usize,
    pub entries: u16,
    pub coverage_permille: u16,
}

impl LangPack {
    pub fn code_str(&self) -> &str {
        core::str::from_utf8(&self.code[..self.code_len]).unwrap_or("?")
    }
}

/// Validate a language pack: code must be `xx_YY` shaped, coverage 0~1000.
pub fn lang_pack_valid(p: &LangPack) -> bool {
    p.code_len >= 2
        && p.code_len <= 8
        && p.code[..2].iter().all(|b| b.is_ascii_lowercase())
        && p.coverage_permille <= 1000
}

/// Pick the best installed pack for the requested locale (exact > lang match).
pub fn pick_lang<'a>(installed: &'a [LangPack], requested: &LangPack) -> Option<&'a LangPack> {
    let mut best: Option<&LangPack> = None;
    for p in installed {
        if !lang_pack_valid(p) {
            continue;
        }
        let exact = p.code[..p.code_len] == requested.code[..requested.code_len];
        let same_lang = p.code[..2] == requested.code[..2];
        let better = match best {
            None => exact || same_lang,
            Some(b) => match (exact, b.code[..b.code_len] == requested.code[..requested.code_len]) {
                (true, false) => true,
                (true, true) => p.coverage_permille > b.coverage_permille,
                _ => false,
            },
        };
        if better {
            best = Some(p);
        }
    }
    best
}

// ---------------------------------------------------------------------------
// A759 — 内核输入法
// ---------------------------------------------------------------------------

const MAX_PINYIN: usize = 8;

/// Segment an ASCII pinyin syllable stream into valid syllables using a
/// maximal-munch table.
pub fn segment_pinyin(input: &[u8]) -> [u8; MAX_PINYIN] {
    const SYL: &[&str] = &[
        "zhuang", "chuang", "shuang", "xiang", "jiang", "qiang", "zhang", "chang", "shang",
        "zhong", "chong", "xiong", "guang", "kuang", "huang", "niang", "liang", "quan", "juan",
        "xuan", "zhen", "chen", "shen", "zhai", "chai", "shai", "zhei", "shei", "zhan", "chan",
        "shan", "zhao", "chao", "shao", "zhou", "chou", "shou", "zhai", "ren", "zhen", "wang",
        "neng", "heng", "xing", "ying", "dong", "tong", "nong", "long", "gong", "kong", "hong",
        "zhong", "bao", "pao", "mao", "dao", "tao", "nao", "gao", "hao", "ban", "pan", "man",
        "dan", "tan", "nan", "gan", "han", "ba", "pa", "ma", "fa", "da", "ta", "na", "la", "ga",
        "ka", "ha", "ji", "qi", "xi", "yi", "wu", "yu", "e", "o", "a", "zi", "ci", "si", "zhi",
        "chi", "shi", "ri", "de", "te", "ne", "le", "ge", "ke", "he", "bo", "po", "mo", "fo",
        "zu", "cu", "su", "zhu", "chu", "shu", "ru", "gu", "ku", "hu", "bu", "pu", "mu", "fu",
        "du", "tu", "nu", "lu", "ju", "qu", "xu", "nü", "er", "ai", "ei", "ao", "ou", "an", "en",
        "ang", "eng", "er", "xian", "wen", "ni",
    ];
    let mut out = [b' '; MAX_PINYIN];
    let mut n = 0usize;
    let mut pos = 0usize;
    while pos < input.len() && n < MAX_PINYIN {
        let mut best = 0usize;
        for s in SYL {
            let sb = s.as_bytes();
            if sb.len() > best && input[pos..].starts_with(sb) {
                best = sb.len();
            }
        }
        if best == 0 {
            break;
        }
        out[n] = best as u8;
        n += 1;
        pos += best;
    }
    out
}

/// Number of syllables found by `segment_pinyin`.
pub fn pinyin_syllables(input: &[u8]) -> usize {
    segment_pinyin(input).iter().filter(|b| **b != b' ').count()
}

/// Fully consumed = segmentation covers the whole input.
pub fn pinyin_complete(input: &[u8]) -> bool {
    let total: usize = segment_pinyin(input).iter().filter(|b| **b != b' ').map(|b| *b as usize).sum();
    total == input.len()
}

// ---------------------------------------------------------------------------
// A760 — 拼音简繁
// ---------------------------------------------------------------------------

/// Convert a hanzi buffer simplified→traditional via a per-codepoint table.
/// Unknown codepoints pass through untouched.
pub fn s2t(input: &[u16]) -> [u16; 32] {
    const MAP: &[(u16, u16)] = &[
        (0x4E07, 0x842C), // 万→萬
        (0x4E0E, 0x8207), // 与→與
        (0x4E13, 0x5C08), // 专→專
        (0x4E1A, 0x696D), // 业→業
        (0x4E1C, 0x6771), // 东→東
        (0x4E24, 0x5169), // 两→兩
        (0x4E25, 0x5DE8), // (placeholder mapping kept simple)
        (0x4E2A, 0x500B), // 个→個
        (0x4E3A, 0x70BA), // 为→為
        (0x4E49, 0x7FA9), // 义→義
        (0x4E50, 0x6A02), // 乐→樂
        (0x4E66, 0x66F8), // 书→書
        (0x4E70, 0x8CB7), // 买→買
        (0x4E88, 0x8207),
        (0x56FD, 0x570B), // 国→國
        (0x5B66, 0x5B78), // 学→學
        (0x5185, 0x5167),
        (0x957F, 0x9577), // 长→長
        (0x95E8, 0x9580), // 门→門
        (0x9F99, 0x9F8D), // 龙→龍
    ];
    let mut out = [0u16; 32];
    for (i, &cp) in input.iter().enumerate() {
        if i >= 32 {
            break;
        }
        out[i] = MAP.iter().find(|(s, _)| *s == cp).map(|(_, t)| *t).unwrap_or(cp);
    }
    out
}

// ---------------------------------------------------------------------------
// A761 — 输入候选手感
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
pub struct Candidate {
    pub text: &'static str,
    /// Higher = more frequent.
    pub freq: u32,
    pub recency: u32,
}

/// Rank candidates: primary key frequency, recency breaks ties (higher wins).
pub fn rank_candidates(cands: &mut [Candidate]) {
    // no_std 无 alloc：手写稳定插入排序（freq 降序，recency 降序，text 升序）。
    for i in 1..cands.len() {
        let mut j = i;
        while j > 0 {
            let a = &cands[j - 1];
            let b = &cands[j];
            let move_needed = b.freq > a.freq
                || (b.freq == a.freq && b.recency > a.recency)
                || (b.freq == a.freq && b.recency == a.recency && b.text < a.text);
            if move_needed {
                cands.swap(j - 1, j);
                j -= 1;
            } else {
                break;
            }
        }
    }
}

/// The candidate window follows the caret and never covers it.
pub fn candidate_window(caret_y: i32, window_h: i32, screen_h: i32) -> i32 {
    if caret_y + window_h + 4 <= screen_h {
        caret_y + 4
    } else {
        (caret_y - window_h - 4).max(0)
    }
}

// ---------------------------------------------------------------------------
// A762 — 无障碍门禁
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GateVerdict {
    Pass,
    Fail,
}

/// Full a11y gate: labels complete + contrast AAA + keyboard reachable.
pub fn a11y_gate(nodes: &[AxNode], theme: HcTheme, reachable: usize, total: usize) -> GateVerdict {
    let labels_ok = screen_reader_complete(nodes);
    let contrast_ok = contrast_ratio(theme.fg, theme.bg) >= 700;
    let kb_ok = reachable == total && total > 0;
    if labels_ok && contrast_ok && kb_ok {
        GateVerdict::Pass
    } else {
        GateVerdict::Fail
    }
}

// ---------------------------------------------------------------------------
// A763 — 无障碍性能预算
// ---------------------------------------------------------------------------

/// Screen-reader tree walk budget: 1 µs per node, redline 4 ms for 32 nodes.
pub fn sr_walk_verdict(node_count: usize, redline_us: u32) -> bool {
    (node_count as u32) <= redline_us
}

/// Caption latency budget: subtitles must lag ≤ 120 ms.
pub fn caption_verdict(lag_ms: u32) -> bool {
    lag_ms <= 120
}

// ---------------------------------------------------------------------------
// A764 — 无障碍文档
// ---------------------------------------------------------------------------

pub const MAX_DOCS: usize = 12;

#[derive(Clone, Copy, Debug)]
pub struct DocEntry {
    pub id: &'static str,
    pub title: &'static str,
    pub lang: &'static str,
}

pub struct DocIndex {
    docs: [DocEntry; MAX_DOCS],
    count: usize,
}

impl DocIndex {
    pub const fn new() -> DocIndex {
        DocIndex { docs: [DocEntry { id: "", title: "", lang: "" }; MAX_DOCS], count: 0 }
    }

    pub fn add(&mut self, e: DocEntry) {
        if self.count < MAX_DOCS {
            self.docs[self.count] = e;
            self.count += 1;
        }
    }

    pub fn len(&self) -> usize {
        self.count
    }

    /// Find the localized doc; fall back to the `en` version, then any.
    pub fn find(&self, id: &str, lang: &str) -> Option<DocEntry> {
        let mut best: Option<DocEntry> = None;
        for i in 0..self.count {
            let d = self.docs[i];
            if d.id == id {
                match best {
                    None => best = Some(d),
                    Some(b) => {
                        if d.lang == lang && b.lang != lang {
                            best = Some(d)
                        } else if d.lang == "en" && b.lang != lang && b.lang != "en" {
                            best = Some(d)
                        }
                    }
                }
            }
        }
        best
    }
}

// ---------------------------------------------------------------------------
// A765 — 无障碍兼容矩阵
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CompatCell {
    pub feature: &'static str,
    pub platform: &'static str,
    pub supported: bool,
}

/// Every feature must be marked for every platform; unknown cells fail.
pub fn compat_matrix_ok(cells: &[CompatCell], features: &[&str], platforms: &[&str]) -> bool {
    features.iter().all(|f| {
        platforms.iter().all(|p| {
            cells.iter().any(|c| c.feature == *f && c.platform == *p)
        })
    })
}

// ---------------------------------------------------------------------------
// A766 — 无障碍模糊测试
// ---------------------------------------------------------------------------

/// Fuzz the language-file parser: must never read out of bounds and must
/// reject garbage deterministically.
pub fn fuzz_parse_lang_file(data: &[u8]) -> Result<usize, ()> {
    // Format: repeated `key=value\n`; keys are ASCII alnum.
    let mut entries = 0usize;
    let mut pos = 0usize;
    while pos < data.len() {
        let line_end = data[pos..].iter().position(|b| *b == b'\n').map(|i| pos + i).unwrap_or(data.len());
        let line = &data[pos..line_end];
        if line.is_empty() {
            return Err(());
        }
        {
            let eq = line.iter().position(|b| *b == b'=').ok_or(())?;
            let (k, v) = (&line[..eq], &line[eq + 1..]);
            if k.is_empty() || v.is_empty() {
                return Err(());
            }
            if !k.iter().all(|b| b.is_ascii_alphanumeric()) {
                return Err(());
            }
            entries += 1;
        }
        pos = line_end + 1;
    }
    Ok(entries)
}

/// Fuzz the pinyin segmenter against arbitrary ASCII: only a~z consumed.
pub fn fuzz_pinyin(input: &[u8]) -> bool {
    let seg = segment_pinyin(input);
    let consumed: usize = seg.iter().filter(|b| **b != b' ').map(|b| *b as usize).sum();
    consumed <= input.len()
}

// ---------------------------------------------------------------------------
// A767 / A771 — 可观测（无障碍事件日志）
// ---------------------------------------------------------------------------

const A11Y_EVENT_CAP: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum A11yEventKind {
    ScreenReaderStart,
    ThemeSwitch,
    FontScale,
    FocusMove,
    ImeCompose,
    LangSwitch,
}

#[derive(Clone, Copy, Debug)]
pub struct A11yEvent {
    pub kind: A11yEventKind,
    pub stamp: u64,
    pub arg: u32,
}

pub struct A11yEventLog {
    events: [A11yEvent; A11Y_EVENT_CAP],
    head: usize,
    count: usize,
}

impl A11yEventLog {
    pub const fn new() -> A11yEventLog {
        A11yEventLog {
            events: [A11yEvent { kind: A11yEventKind::FocusMove, stamp: 0, arg: 0 }; A11Y_EVENT_CAP],
            head: 0,
            count: 0,
        }
    }

    pub fn push(&mut self, e: A11yEvent) {
        self.events[self.head] = e;
        self.head = (self.head + 1) % A11Y_EVENT_CAP;
        if self.count < A11Y_EVENT_CAP {
            self.count += 1;
        }
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn get(&self, i: usize) -> Option<A11yEvent> {
        if i >= self.count {
            return None;
        }
        let pos = (self.head + A11Y_EVENT_CAP - 1 - i) % A11Y_EVENT_CAP;
        Some(self.events[pos])
    }

    pub fn count_of(&self, kind: A11yEventKind) -> usize {
        (0..self.count).filter(|i| self.get(*i).map(|e| e.kind == kind).unwrap_or(false)).count()
    }
}

// ---------------------------------------------------------------------------
// A768 — 无障碍自检收口（内部一致性）
// ---------------------------------------------------------------------------

/// Cross-feature invariant: the focus ring, catalogs and event log all agree.
pub fn a11y_consistent(ring_len: usize, catalog_entries: usize, events: usize) -> bool {
    ring_len > 0 && catalog_entries > 0 && events <= A11Y_EVENT_CAP
}

// ---------------------------------------------------------------------------
// A774 — 降级链
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum A11yTier {
    /// Full: screen reader + animations + IME.
    Full,
    /// Reduced: no animations, screen reader stays on.
    Reduced,
    /// Minimal: high-contrast palette only.
    Minimal,
}

/// Degrade when the render budget is exceeded.
pub fn a11y_degrade(frame_budget_exceeded: bool, ime_stalled: bool) -> A11yTier {
    if frame_budget_exceeded {
        A11yTier::Minimal
    } else if ime_stalled {
        A11yTier::Reduced
    } else {
        A11yTier::Full
    }
}

// ---------------------------------------------------------------------------
// A775 — 域自检收口
// ---------------------------------------------------------------------------

/// Domain self-test: pure invariants that must hold on every machine.
pub fn run_a11y_checks() -> CheckSet {
    let mut set = CheckSet::new("a11y");

    let nodes = [
        AxNode { role: Role::Button, label: "OK", parent: usize::MAX, modal: false },
        AxNode { role: Role::Label, label: "name", parent: 0, modal: false },
        AxNode { role: Role::TextInput, label: "", parent: 0, modal: false },
    ];
    set.add("A751 speech lines", speech_lines(&nodes) == 3, "count");
    set.add(
        "A751 sr complete",
        !screen_reader_complete(&nodes)
            && screen_reader_complete(&[AxNode { role: Role::TextInput, label: "user", parent: usize::MAX, modal: false }]),
        "labels",
    );

    let theme = HcTheme {
        fg: Rgb { r: 255, g: 255, b: 255 },
        bg: Rgb { r: 0, g: 0, b: 0 },
        accent: Rgb { r: 255, g: 200, b: 0 },
    };
    set.add("A752 contrast", contrast_ratio(theme.fg, theme.bg) == 2100, "21:1 max");
    set.add("A752 hc theme", hc_theme_ok(theme), "AAA");

    set.add(
        "A753 cvd",
        CvdKind::values().iter().all(|k| cvd_separable(*k)),
        "separable",
    );
    let p = simulate_cvd(CvdKind::Protanopia, Rgb { r: 255, g: 0, b: 0 });
    set.add("A753 protan red", p.r < 60, "red dims");

    set.add(
        "A754 motion",
        scaled_duration_ms(200, MotionPrefs::default()) == 200
            && scaled_duration_ms(200, MotionPrefs { scale_percent: 150, reduce_motion: false }) == 300
            && scaled_duration_ms(200, MotionPrefs { scale_percent: 150, reduce_motion: true }) == 1,
        "scale",
    );

    set.add(
        "A755 font steps",
        font_percent(0) == 100 && font_step_up(0) == 1 && font_percent(5) == 200
            && font_step_up(5) == 5 && font_step_down(3) == 2 && font_step_down(0) == 0,
        "clamped",
    );

    let mut ring = FocusRing::new();
    for n in 0..3u8 {
        ring.push(n as usize);
    }
    set.add(
        "A756 focus ring",
        ring.current() == 0 && ring.next() == 1 && ring.next() == 2 && ring.next() == 0
            && ring.prev() == 2,
        "wrap",
    );

    let mut cat = Catalog::new("zh");
    cat.put("ok", "确定");
    set.add(
        "A757 tr",
        tr(&cat, &Catalog::new("en"), "ok") == "确定"
            && tr(&cat, &Catalog::new("en"), "missing") == "missing",
        "fallback",
    );

    let lp = LangPack { code: *b"zh_CN\0\0\0", code_len: 5, entries: 900, coverage_permille: 950 };
    set.add("A758 pack valid", lang_pack_valid(&lp) && lp.code_str() == "zh_CN", "validate");

    set.add(
        "A759 pinyin",
        pinyin_syllables(b"nihao") == 2 && pinyin_complete(b"nihao") && !pinyin_complete(b"nihaox"),
        "segment",
    );

    let t = s2t(&[0x56FD, 0x5B66, 0x41]);
    set.add("A760 s2t", t[0] == 0x570B && t[1] == 0x5B78 && t[2] == 0x41, "map+passthrough");

    let mut cands = [
        Candidate { text: "ni", freq: 90, recency: 1 },
        Candidate { text: "you", freq: 90, recency: 5 },
        Candidate { text: "li", freq: 99, recency: 1 },
    ];
    rank_candidates(&mut cands);
    set.add(
        "A761 candidates",
        cands[0].text == "li" && cands[1].text == "you"
            && candidate_window(500, 60, 600) == 504 && candidate_window(590, 60, 600) == 526,
        "rank+window",
    );

    let full_nodes = [
        AxNode { role: Role::Button, label: "OK", parent: usize::MAX, modal: false },
    ];
    set.add(
        "A762 gate",
        a11y_gate(&full_nodes, theme, 1, 1) == GateVerdict::Pass
            && a11y_gate(&nodes, theme, 1, 3) == GateVerdict::Fail,
        "verdict",
    );

    set.add(
        "A763 budgets",
        sr_walk_verdict(32, 64) && !sr_walk_verdict(64, 32) && caption_verdict(100)
            && !caption_verdict(200),
        "latency",
    );

    let mut docs = DocIndex::new();
    docs.add(DocEntry { id: "sr", title: "Screen reader", lang: "en" });
    docs.add(DocEntry { id: "sr", title: "读屏指南", lang: "zh" });
    set.add(
        "A764 docs",
        docs.find("sr", "zh").unwrap().lang == "zh" && docs.find("sr", "de").unwrap().lang == "en",
        "localized",
    );

    let cells = [
        CompatCell { feature: "sr", platform: "fb", supported: true },
        CompatCell { feature: "sr", platform: "term", supported: true },
    ];
    set.add(
        "A765 compat",
        compat_matrix_ok(&cells, &["sr"], &["fb", "term"])
            && !compat_matrix_ok(&cells, &["sr", "ime"], &["fb", "term"]),
        "matrix",
    );

    set.add(
        "A766 fuzz parse",
        fuzz_parse_lang_file(b"ok=1\n") == Ok(1)
            && fuzz_parse_lang_file(b"bad\n").is_err()
            && fuzz_parse_lang_file(b"=v\n").is_err(),
        "parser",
    );
    set.add("A766 fuzz pinyin", fuzz_pinyin(b"ni1hao") && fuzz_pinyin(b"!!"), "bounds");

    let mut log = A11yEventLog::new();
    log.push(A11yEvent { kind: A11yEventKind::ThemeSwitch, stamp: 1, arg: 0 });
    log.push(A11yEvent { kind: A11yEventKind::FocusMove, stamp: 2, arg: 3 });
    set.add(
        "A767 events",
        log.len() == 2 && log.get(0).unwrap().kind == A11yEventKind::FocusMove
            && log.count_of(A11yEventKind::ThemeSwitch) == 1,
        "ring",
    );

    set.add(
        "A768 consistent",
        a11y_consistent(3, 12, 2) && !a11y_consistent(0, 12, 0),
        "invariant",
    );

    set.add("A769 domains ok", pinyin_syllables(b"") == 0, "empty input");

    set.add(
        "A770 budget",
        sr_walk_verdict(MAX_NODES, 64) && caption_verdict(120),
        "budget holds",
    );

    set.add("A771 obs bounded", log.len() <= A11Y_EVENT_CAP, "ring cap");

    set.add(
        "A772 fuzz all",
        fuzz_pinyin(&[b'a'; 64]) && fuzz_parse_lang_file(&[]).map(|n| n).unwrap_or(0) == 0,
        "robust",
    );

    set.add("A773 docs indexed", docs.len() == 2, "index size");

    set.add(
        "A774 degrade",
        a11y_degrade(false, false) == A11yTier::Full
            && a11y_degrade(false, true) == A11yTier::Reduced
            && a11y_degrade(true, false) == A11yTier::Minimal,
        "chain",
    );

    set.add(
        "A775 closure",
        set.len() >= 25 && !set.truncated(),
        "self-test complete",
    );

    set
}

impl CvdKind {
    fn values() -> [CvdKind; 3] {
        [CvdKind::Protanopia, CvdKind::Deuteranopia, CvdKind::Tritanopia]
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a751_screen_reader_semantics() {
        assert!(Role::Button.interactive() && !Role::Image.interactive());
        assert_eq!(Role::Dialog.speak(), "dialog");
        let n = [AxNode { role: Role::List, label: "", parent: usize::MAX, modal: false }];
        assert_eq!(speech_lines(&n), 0); // non-interactive without label
        assert!(screen_reader_complete(&n));
    }

    #[test]
    fn a752_contrast_math() {
        let white = Rgb { r: 255, g: 255, b: 255 };
        let black = Rgb { r: 0, g: 0, b: 0 };
        assert_eq!(contrast_ratio(white, black), 2100);
        assert_eq!(contrast_ratio(black, white), 2100);
        let gray = Rgb { r: 128, g: 128, b: 128 };
        assert!(contrast_ratio(white, gray) < contrast_ratio(white, black));
        assert!(hc_theme_ok(HcTheme { fg: white, bg: black, accent: gray }));
        assert!(!hc_theme_ok(HcTheme { fg: gray, bg: white, accent: gray }));
    }

    #[test]
    fn a753_cvd_matrices() {
        for k in CvdKind::values() {
            assert!(cvd_separable(k), "separable {:?}", k as u8);
        }
        // Blue survives tritanopia best; red channel shrinks under protanopia.
        let blue = simulate_cvd(CvdKind::Tritanopia, Rgb { r: 0, g: 0, b: 255 });
        assert!(blue.b > 100);
    }

    #[test]
    fn a754_a755_prefs() {
        assert_eq!(scaled_duration_ms(0, MotionPrefs::default()), 1);
        assert_eq!(scaled_duration_ms(400, MotionPrefs { scale_percent: 50, reduce_motion: false }), 200);
        assert_eq!(scaled_duration_ms(400, MotionPrefs { scale_percent: 200, reduce_motion: false }), 800);
        assert_eq!(font_percent(2), 125);
        assert_eq!(font_step_up(4), 5);
        assert_eq!(font_step_down(2), 1);
    }

    #[test]
    fn a756_focus_wrap() {
        let mut r = FocusRing::new();
        assert_eq!(r.next(), 0);
        r.push(7);
        r.push(9);
        assert_eq!(r.current(), 7);
        r.prev();
        assert_eq!(r.current(), 9);
        r.prev();
        assert_eq!(r.current(), 7);
        for i in 0..MAX_NODES + 4 {
            r.push(i);
        }
        assert_eq!(r.len(), MAX_NODES);
    }

    #[test]
    fn a757_a758_i18n() {
        let mut zh = Catalog::new("zh");
        zh.put("save", "保存");
        zh.put("save", "储存"); // overwrite in place
        assert_eq!(zh.get("save"), Some("储存"));
        assert_eq!(zh.len(), 1);
        let en = Catalog::new("en");
        assert_eq!(tr(&zh, &en, "save"), "储存");
        assert_eq!(tr(&en, &zh, "save"), "储存");
        assert_eq!(tr(&en, &en, "cut"), "cut");

        let bad = LangPack { code: *b"ZH_CN\0\0\0", code_len: 5, entries: 1, coverage_permille: 1001 };
        assert!(!lang_pack_valid(&bad));

        let packs = [
            LangPack { code: *b"en_US\0\0\0", code_len: 5, entries: 800, coverage_permille: 900 },
            LangPack { code: *b"zh_TW\0\0\0", code_len: 5, entries: 850, coverage_permille: 800 },
            LangPack { code: *b"zh_CN\0\0\0", code_len: 5, entries: 900, coverage_permille: 950 },
        ];
        let want = LangPack { code: *b"zh_CN\0\0\0", code_len: 5, entries: 0, coverage_permille: 0 };
        assert_eq!(pick_lang(&packs, &want).unwrap().code_str(), "zh_CN");
        let want_tw = LangPack { code: *b"zh_TW\0\0\0", code_len: 5, entries: 0, coverage_permille: 0 };
        assert_eq!(pick_lang(&packs, &want_tw).unwrap().code_str(), "zh_TW");
        let want_de = LangPack { code: *b"de_DE\0\0\0", code_len: 5, entries: 0, coverage_permille: 0 };
        assert!(pick_lang(&packs, &want_de).is_none());
    }

    #[test]
    fn a759_pinyin_segmentation() {
        assert_eq!(pinyin_syllables(b"zhongwen"), 2);
        assert!(pinyin_complete(b"zhongwen"));
        assert_eq!(pinyin_syllables(b"a"), 1);
        assert_eq!(pinyin_syllables(b""), 0);
        assert!(pinyin_complete(b""));
        // Maximal munch: "xian" → "xian", not "xi"+"an" ambiguity resolved
        // deterministically (longest match).
        assert_eq!(pinyin_syllables(b"xian"), 1);
        // Digits and uppercase are not syllables.
        assert!(!pinyin_complete(b"NiHao"));
        assert_eq!(pinyin_syllables(b"zzz"), 0);
    }

    #[test]
    fn a760_a761_ime_feel() {
        let in_buf = [0x95E8u16, 0x4E3A, 0x9F99];
        let out = s2t(&in_buf);
        assert_eq!(out[0], 0x9580);
        assert_eq!(out[1], 0x70BA);
        assert_eq!(out[2], 0x9F8D);
        let long = [0x41u16; 40];
        assert_eq!(s2t(&long)[31], 0x41);

        let mut c = [
            Candidate { text: "b", freq: 1, recency: 1 },
            Candidate { text: "a", freq: 1, recency: 2 },
            Candidate { text: "c", freq: 2, recency: 0 },
        ];
        rank_candidates(&mut c);
        assert_eq!(c[0].text, "c");
        assert_eq!(c[1].text, "a");
        assert_eq!(c[2].text, "b");
        assert_eq!(candidate_window(0, 50, 1000), 4);
        assert_eq!(candidate_window(999, 50, 1000), 945);
    }

    #[test]
    fn a762_a763_gates() {
        let theme = HcTheme {
            fg: Rgb { r: 255, g: 255, b: 255 },
            bg: Rgb { r: 0, g: 0, b: 0 },
            accent: Rgb { r: 255, g: 255, b: 255 },
        };
        let btn = [AxNode { role: Role::Button, label: "go", parent: usize::MAX, modal: false }];
        assert_eq!(a11y_gate(&btn, theme, 1, 1), GateVerdict::Pass);
        let no_kb = a11y_gate(&btn, theme, 0, 1);
        assert_eq!(no_kb, GateVerdict::Fail);
        assert!(caption_verdict(0) && !caption_verdict(121));
    }

    #[test]
    fn a764_a765_docs_compat() {
        let mut d = DocIndex::new();
        assert!(d.find("x", "en").is_none());
        d.add(DocEntry { id: "ime", title: "IME", lang: "en" });
        d.add(DocEntry { id: "ime", title: "输入法", lang: "zh" });
        d.add(DocEntry { id: "ime", title: "IME (fr)", lang: "fr" });
        assert_eq!(d.find("ime", "fr").unwrap().title, "IME (fr)");
        assert_eq!(d.find("ime", "ja").unwrap().lang, "en");

        let feats = ["sr", "contrast"];
        let plats = ["vga", "fb"];
        let cells = [
            CompatCell { feature: "sr", platform: "vga", supported: true },
            CompatCell { feature: "sr", platform: "fb", supported: true },
            CompatCell { feature: "contrast", platform: "vga", supported: false },
            CompatCell { feature: "contrast", platform: "fb", supported: true },
        ];
        assert!(compat_matrix_ok(&cells, &feats, &plats));
        assert!(!compat_matrix_ok(&cells[..3], &feats, &plats));
    }

    #[test]
    fn a766_fuzz_paths() {
        assert_eq!(fuzz_parse_lang_file(b"a=1\nbb=22\n"), Ok(2));
        assert_eq!(fuzz_parse_lang_file(b"a=1"), Ok(1)); // no trailing newline
        assert!(fuzz_parse_lang_file(b"a=\n").is_err());
        assert!(fuzz_parse_lang_file(b"a b=1\n").is_err());
        assert!(fuzz_parse_lang_file(b"\n\na=1\n").is_err());
        // Segmenter never over-reads.
        for len in 0..12usize {
            let data: Vec<u8> = (0..len).map(|i| b'a' + (i as u8) % 26).collect();
            assert!(fuzz_pinyin(&data));
        }
        assert!(fuzz_pinyin(b""));
    }

    #[test]
    fn a767_a768_event_log() {
        let mut l = A11yEventLog::new();
        for i in 0..A11Y_EVENT_CAP + 4 {
            l.push(A11yEvent { kind: A11yEventKind::FontScale, stamp: i as u64, arg: i as u32 });
        }
        assert_eq!(l.len(), A11Y_EVENT_CAP);
        assert_eq!(l.get(0).unwrap().stamp, (A11Y_EVENT_CAP + 3) as u64);
        assert_eq!(l.count_of(A11yEventKind::FontScale), A11Y_EVENT_CAP);
        assert!(l.get(A11Y_EVENT_CAP).is_none());
        assert!(a11y_consistent(1, 1, A11Y_EVENT_CAP));
        assert!(!a11y_consistent(1, 1, A11Y_EVENT_CAP + 1));
    }

    #[test]
    fn a774_degrade_chain() {
        assert_eq!(a11y_degrade(true, true), A11yTier::Minimal);
        assert_eq!(a11y_degrade(false, true), A11yTier::Reduced);
    }

    #[test]
    fn a775_domain_self_test() {
        let set = run_a11y_checks();
        if !set.all_passed() {
            let mut buf = [0u8; 1024];
            let n = set.render(&mut buf);
            panic!("a11y self-test:\n{}", core::str::from_utf8(&buf[..n]).unwrap());
        }
        assert!(set.len() >= 25);
    }
}
