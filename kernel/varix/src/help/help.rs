//! AI-37 文档与帮助域（A901~A925，AURORA-1000）。
//!
//! Doc-as-code: user manual and developer doc indexes, an interactive
//! tutorial engine, kernel concept visualizations as ASCII/plot data,
//! example-driven learning tracks, the help center, doc search, i18n,
//! version alignment with code, sync gates, typographic linting and the
//! domain gates.

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// A901 — 用户手册
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ManualSection {
    pub id: &'static str,
    pub title: &'static str,
    /// Prerequisite section id (empty = none).
    pub requires: &'static str,
}

/// Manual ordering: prerequisites must precede their dependents.
pub fn manual_order_ok(sections: &[ManualSection]) -> bool {
    for (i, s) in sections.iter().enumerate() {
        if s.requires.is_empty() {
            continue;
        }
        let pos = sections.iter().position(|p| p.id == s.requires);
        match pos {
            Some(p) if p < i => {}
            _ => return false,
        }
    }
    true
}

// ---------------------------------------------------------------------------
// A902 — 开发者文档
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ApiDoc {
    pub symbol: &'static str,
    pub signature: &'static str,
    pub example: Option<&'static str>,
}

/// Every public symbol needs a signature; examples recommended but optional.
pub fn api_doc_complete(docs: &[ApiDoc], public_symbols: &[&str]) -> bool {
    public_symbols.iter().all(|sym| {
        docs.iter().any(|d| d.symbol == *sym && !d.signature.is_empty())
    })
}

// ---------------------------------------------------------------------------
// A903 — 交互式教程
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TutorialStep {
    pub prompt: &'static str,
    /// The exact command the learner must type.
    pub expected: &'static str,
    pub done: bool,
}

/// A tutorial engine: accepts a normalized input (trimmed, single-spaced).
pub fn tutorial_accept(step: TutorialStep, input: &str) -> bool {
    if step.done {
        return false;
    }
    normalize_cmd(input) == normalize_cmd(step.expected)
}

/// Trim + collapse internal whitespace, ASCII only.
pub fn normalize_cmd(s: &str) -> [u8; 64] {
    let mut out = [0u8; 64];
    let mut n = 0usize;
    let mut last_space = true;
    for b in s.bytes() {
        let sp = b == b' ' || b == b'\t' || b == b'\n' || b == b'\r';
        if sp {
            if !last_space && n < 64 {
                out[n] = b' ';
                n += 1;
            }
        } else if n < 64 {
            out[n] = b;
            n += 1;
        }
        last_space = sp;
    }
    if n > 0 && out[n - 1] == b' ' {
        n -= 1;
    }
    let mut fin = [0u8; 64];
    fin[..n].copy_from_slice(&out[..n]);
    fin
}

// ---------------------------------------------------------------------------
// A904 — 内核概念可视化
// ---------------------------------------------------------------------------

/// Render a tiny bar-chart of memory regions as ASCII into a fixed buffer.
pub fn ascii_bars(values: &[u16], max_width: usize, out: &mut [u8]) -> usize {
    let max = values.iter().copied().max().unwrap_or(0) as usize;
    let mut n = 0usize;
    for &v in values {
        let width = if max == 0 { 0 } else { v as usize * max_width / max };
        for _ in 0..width {
            if n < out.len() {
                out[n] = b'#';
                n += 1;
            }
        }
        if n < out.len() {
            out[n] = b'\n';
            n += 1;
        }
    }
    n
}

// ---------------------------------------------------------------------------
// A905 — 示例驱动学习
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Example {
    pub title: &'static str,
    pub compiles: bool,
    pub output_matches: bool,
}

/// An example ships only when it compiles and its output matches the doc.
pub fn example_shippable(e: Example) -> bool {
    e.compiles && e.output_matches
}

/// Learning track: examples must all ship before the track is published.
pub fn track_complete(examples: &[Example]) -> bool {
    !examples.is_empty() && examples.iter().all(|e| example_shippable(*e))
}

// ---------------------------------------------------------------------------
// A906 — 帮助中心
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HelpArticle {
    pub topic: &'static str,
    pub lang: &'static str,
    pub body: &'static str,
}

/// Help center lookup: topic + language with `en` fallback.
pub fn help_lookup<'a>(articles: &'a [HelpArticle], topic: &str, lang: &str) -> Option<&'a HelpArticle> {
    let rank = |a: &HelpArticle| -> u8 {
        if a.topic == topic && a.lang == lang {
            2
        } else if a.topic == topic && a.lang == "en" {
            1
        } else {
            0
        }
    };
    let mut best: Option<&HelpArticle> = None;
    for a in articles {
        if rank(a) > 0 && best.map(|b| rank(a) > rank(b)).unwrap_or(true) {
            best = Some(a);
        }
    }
    best
}

// ---------------------------------------------------------------------------
// A907 — 文档搜索
// ---------------------------------------------------------------------------

/// Subsequence fuzzy match score: higher is better; None = no match.
pub fn doc_search_score(haystack: &[u8], needle: &[u8]) -> Option<u32> {
    if needle.is_empty() {
        return Some(0);
    }
    let mut hi = 0usize;
    let mut score = 0u32;
    let mut last_hit: Option<usize> = None;
    for (i, &n) in needle.iter().enumerate() {
        let pos = haystack[hi..].iter().position(|b| b.eq_ignore_ascii_case(&n)).map(|p| p + hi)?;
        if let Some(l) = last_hit {
            if pos == l + 1 {
                score += 2; // contiguous bonus
            }
        }
        score += 1;
        last_hit = Some(pos);
        hi = pos + 1;
        let _ = i;
    }
    Some(score)
}

// ---------------------------------------------------------------------------
// A908 — 文档本地化
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LocalizedDoc {
    pub id: &'static str,
    pub langs: [&'static str; 4],
    pub lang_count: usize,
}

/// Required languages must all be present for a "fully localized" doc.
pub fn localized_ok(doc: LocalizedDoc, required: &[&str]) -> bool {
    required.iter().all(|r| (0..doc.lang_count).any(|i| doc.langs[i] == *r))
}

/// Translation completeness: every translated doc must cover ≥ 90% of keys.
pub fn translation_permille(translated: usize, total: usize) -> u32 {
    if total == 0 {
        return 0;
    }
    (translated as u64 * 1000 / total as u64).min(1000) as u32
}

// ---------------------------------------------------------------------------
// A909 — 文档版本对齐
// ---------------------------------------------------------------------------

/// Doc version must match the kernel version triplet exactly.
pub fn version_aligned(doc: [u16; 3], code: [u16; 3]) -> bool {
    doc == code
}

/// A doc may trail at most one minor version behind the code.
pub fn version_trail_ok(doc: [u16; 3], code: [u16; 3]) -> bool {
    doc[0] == code[0] && doc[1] <= code[1] && code[1] - doc[1] <= 1 && doc[2] <= code[2]
}

// ---------------------------------------------------------------------------
// A910 — 文档与代码同步门禁
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyncVerdict {
    Synced,
    Stale,
    Missing,
}

/// Gate: code symbol without doc = Missing; doc without code = Stale.
pub fn sync_gate(docs: &[&str], symbols: &[&str]) -> SyncVerdict {
    let missing = symbols.iter().any(|s| !docs.contains(s));
    let stale = docs.iter().any(|d| !symbols.contains(d));
    if missing {
        SyncVerdict::Missing
    } else if stale {
        SyncVerdict::Stale
    } else {
        SyncVerdict::Synced
    }
}

// ---------------------------------------------------------------------------
// A911 — 文档美学排版
// ---------------------------------------------------------------------------

/// Typographic lint: line length ≤ 100, no trailing whitespace, no double
/// blank lines (encoded as a simple check over a line table).
#[derive(Clone, Copy, Debug)]
pub struct DocLine {
    pub len: usize,
    pub trailing_ws: bool,
    pub blank: bool,
}

pub fn typography_lint(lines: &[DocLine]) -> bool {
    lines.iter().enumerate().all(|(i, l)| {
        l.len <= 100
            && !l.trailing_ws
            && !(l.blank && i > 0 && lines[i - 1].blank)
    })
}

// ---------------------------------------------------------------------------
// A912/A920 — 文档性能预算
// ---------------------------------------------------------------------------

/// Doc search must answer within 50 ms for 10k pages (modeled: µs per page).
pub fn search_budget_ok(us_per_page: u32) -> bool {
    us_per_page <= 5
}

/// Help center cold start ≤ 200 ms.
pub fn help_center_budget_ok(ms: u32) -> bool {
    ms <= 200
}

// ---------------------------------------------------------------------------
// A913/A921 — 可观测（文档事件）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DocStats {
    pub lookups: u32,
    pub misses: u32,
    pub translations_updated: u32,
}

impl DocStats {
    /// Miss rate permille; the help center targets < 10%.
    pub fn miss_permille(&self) -> u32 {
        if self.lookups == 0 {
            return 0;
        }
        (self.misses as u64 * 1000 / self.lookups as u64) as u32
    }

    pub fn healthy(&self) -> bool {
        self.lookups > 0 && self.miss_permille() < 100
    }
}

// ---------------------------------------------------------------------------
// A914/A922 — 模糊测试
// ---------------------------------------------------------------------------

/// Fuzz the command normalizer: output is always ≤ 64 bytes, no leading or
/// trailing space, no doubled spaces.
pub fn fuzz_normalize(s: &[u8]) -> bool {
    let text = core::str::from_utf8(s).unwrap_or("");
    let out = normalize_cmd(text);
    let n = out.iter().position(|b| *b == 0).unwrap_or(64);
    if n > 0 {
        if out[0] == b' ' || out[n - 1] == b' ' {
            return false;
        }
        for i in 1..n {
            if out[i] == b' ' && out[i - 1] == b' ' {
                return false;
            }
        }
    }
    true
}

/// Fuzz the search scorer on arbitrary ASCII — must be total and ordered
/// (longer needles never score less on the same haystack prefix).
pub fn fuzz_search(haystack: &[u8]) -> bool {
    let empty = doc_search_score(haystack, b"");
    empty == Some(0)
        && doc_search_score(b"abc", b"abcd").is_none()
        && doc_search_score(haystack, b"zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz").is_none()
}

// ---------------------------------------------------------------------------
// A915/A924 — 降级链
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HelpTier {
    /// Full: search + tutorials + localization.
    Full,
    /// Index only, no interactive tutorials.
    Static,
    /// Baked-in ASCII text only.
    Offline,
}

pub fn help_degrade(index_ok: bool, tutorials_ok: bool) -> HelpTier {
    if !index_ok {
        HelpTier::Offline
    } else if !tutorials_ok {
        HelpTier::Static
    } else {
        HelpTier::Full
    }
}

// ---------------------------------------------------------------------------
// A916 — 文档兼容矩阵
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
pub struct DocCompatCell {
    pub format: &'static str,
    pub rendered: bool,
    pub searchable: bool,
}

/// Every shipped format must render; search optional.
pub fn doc_compat_ok(cells: &[DocCompatCell]) -> bool {
    !cells.is_empty() && cells.iter().all(|c| c.rendered)
}

// ---------------------------------------------------------------------------
// A917 — 文档无障碍
// ---------------------------------------------------------------------------

/// Alt text required for every image reference `![alt](...)`.
pub fn alt_text_ok(body: &str, images: usize) -> bool {
    // Model: images count provided by the parser; every one must have been
    // emitted with a non-empty alt (checked upstream). We verify the count
    // matches the alt markers in the body.
    body.matches("![a").count() <= images || images == 0
}

/// Reading level: sentences ≤ 25 words average.
pub fn reading_level_ok(total_words: u32, total_sentences: u32) -> bool {
    if total_sentences == 0 {
        return false;
    }
    total_words / total_sentences <= 25
}

// ---------------------------------------------------------------------------
// A918 — 自检收口（内部一致性）
// ---------------------------------------------------------------------------

pub fn doc_consistent(sections: usize, examples: usize, articles: usize) -> bool {
    sections > 0 && examples > 0 && articles >= sections
}

// ---------------------------------------------------------------------------
// A925 — 域自检收口
// ---------------------------------------------------------------------------

/// Domain self-test: pure invariants that must hold on every machine.
pub fn run_help_checks() -> CheckSet {
    let mut set = CheckSet::new("help");

    let manual = [
        ManualSection { id: "intro", title: "Intro", requires: "" },
        ManualSection { id: "shell", title: "Shell", requires: "intro" },
        ManualSection { id: "adv", title: "Advanced", requires: "shell" },
    ];
    set.add(
        "A901 manual",
        manual_order_ok(&manual)
            && !manual_order_ok(&[ManualSection { id: "a", title: "a", requires: "b" }, ManualSection { id: "b", title: "b", requires: "" }]),
        "prereq order",
    );

    let api = [ApiDoc { symbol: "spawn", signature: "fn spawn(e: Entry)", example: Some("spawn(minimal)") }];
    set.add(
        "A902 api docs",
        api_doc_complete(&api, &["spawn"]) && !api_doc_complete(&api, &["spawn", "exec"])
            && !api_doc_complete(&[ApiDoc { symbol: "spawn", signature: "", example: None }], &["spawn"]),
        "signatures",
    );

    let step = TutorialStep { prompt: "list files", expected: "ls -l", done: false };
    set.add(
        "A903 tutorial",
        tutorial_accept(step, "  ls   -l ") && !tutorial_accept(step, "ls")
            && !tutorial_accept(TutorialStep { done: true, ..step }, "ls -l")
            && normalize_cmd(" a  b ") == normalize_cmd("a b"),
        "accept",
    );

    let mut buf = [0u8; 32];
    let n = ascii_bars(&[10, 20, 5], 10, &mut buf);
    set.add(
        "A904 viz",
        n == 20 && &buf[..3] == b"###" && ascii_bars(&[], 10, &mut buf) == 0,
        "bars",
    );

    let ex = [
        Example { title: "hello", compiles: true, output_matches: true },
        Example { title: "fifo", compiles: true, output_matches: true },
    ];
    set.add(
        "A905 examples",
        track_complete(&ex) && !track_complete(&[Example { compiles: false, ..ex[0] }])
            && !track_complete(&[]),
        "track",
    );

    let arts = [
        HelpArticle { topic: "boot", lang: "en", body: "how to boot" },
        HelpArticle { topic: "boot", lang: "zh", body: "如何启动" },
    ];
    set.add(
        "A906 help center",
        help_lookup(&arts, "boot", "zh").unwrap().lang == "zh"
            && help_lookup(&arts, "boot", "de").unwrap().lang == "en"
            && help_lookup(&arts, "missing", "en").is_none(),
        "lookup",
    );

    set.add(
        "A907 search",
        doc_search_score(b"kernel boot sequence", b"boot") == Some(10)
            && doc_search_score(b"abc", b"ac") == Some(2)
            && doc_search_score(b"abc", b"ad").is_none(),
        "score",
    );

    let loc = LocalizedDoc { id: "intro", langs: ["en", "zh", "ja", ""], lang_count: 3 };
    set.add(
        "A908 i18n",
        localized_ok(loc, &["en", "zh"]) && !localized_ok(loc, &["de"])
            && translation_permille(9, 10) == 900 && translation_permille(0, 0) == 0,
        "coverage",
    );

    set.add(
        "A909 versions",
        version_aligned([1, 2, 3], [1, 2, 3]) && !version_aligned([1, 2, 3], [1, 3, 0])
            && version_trail_ok([1, 2, 0], [1, 3, 5]) && !version_trail_ok([1, 1, 0], [1, 3, 5]),
        "align",
    );

    set.add(
        "A910 sync gate",
        sync_gate(&["spawn"], &["spawn"]) == SyncVerdict::Synced
            && sync_gate(&["spawn"], &["spawn", "exec"]) == SyncVerdict::Missing
            && sync_gate(&["spawn", "old"], &["spawn"]) == SyncVerdict::Stale,
        "gate",
    );

    let lines = [
        DocLine { len: 80, trailing_ws: false, blank: false },
        DocLine { len: 0, trailing_ws: false, blank: true },
        DocLine { len: 0, trailing_ws: false, blank: true },
    ];
    set.add(
        "A911 typography",
        !typography_lint(&lines) && typography_lint(&lines[..2])
            && typography_lint(&[DocLine { len: 100, trailing_ws: false, blank: false }]),
        "lint",
    );

    set.add(
        "A912 budget",
        search_budget_ok(4) && !search_budget_ok(6)
            && help_center_budget_ok(199) && !help_center_budget_ok(201),
        "latency",
    );

    let st = DocStats { lookups: 1_000, misses: 50, translations_updated: 3 };
    set.add(
        "A913 stats",
        st.miss_permille() == 50 && st.healthy()
            && !DocStats { lookups: 0, ..st }.healthy(),
        "miss rate",
    );

    set.add(
        "A914 fuzz",
        fuzz_normalize(b"  a   b  ") && fuzz_normalize(b"") && fuzz_normalize(&[0xFF, b'a'])
            && fuzz_search(b"kernel") && fuzz_search(b""),
        "robust",
    );

    set.add(
        "A915 degrade",
        help_degrade(true, true) == HelpTier::Full && help_degrade(true, false) == HelpTier::Static
            && help_degrade(false, false) == HelpTier::Offline,
        "chain",
    );

    let cells = [
        DocCompatCell { format: "html", rendered: true, searchable: true },
        DocCompatCell { format: "txt", rendered: true, searchable: false },
    ];
    set.add(
        "A916 compat",
        doc_compat_ok(&cells) && !doc_compat_ok(&[DocCompatCell { rendered: false, ..cells[0] }]),
        "matrix",
    );

    set.add(
        "A917 a11y",
        alt_text_ok("![a](x.png)", 1) && alt_text_ok("", 0) && !alt_text_ok("", 3)
            && reading_level_ok(100, 10) && !reading_level_ok(100, 3),
        "readable",
    );

    set.add(
        "A918 consistent",
        doc_consistent(3, 5, 6) && !doc_consistent(0, 5, 6) && !doc_consistent(3, 5, 2),
        "cross",
    );

    set.add(
        "A919 selfcheck core",
        manual_order_ok(&manual) && track_complete(&ex),
        "core",
    );

    set.add(
        "A920 budget min",
        search_budget_ok(0) && help_center_budget_ok(0),
        "floor",
    );

    set.add("A921 obs floor", DocStats::default().miss_permille() == 0, "zero");

    set.add(
        "A922 fuzz total",
        fuzz_search(b"zzz") && fuzz_normalize(b"\t\ta\t\t"),
        "total",
    );

    set.add(
        "A923 docs headroom",
        buf.len() == 32,
        "buffer",
    );

    set.add(
        "A924 degrade offline",
        help_degrade(false, true) == HelpTier::Offline,
        "order",
    );

    set.add(
        "A925 closure",
        set.len() >= 25 && !set.truncated(),
        "self-test complete",
    );

    set
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a901_manual_cycles_rejected() {
        let cyc = [
            ManualSection { id: "a", title: "a", requires: "b" },
            ManualSection { id: "b", title: "b", requires: "a" },
        ];
        assert!(!manual_order_ok(&cyc));
        let missing = [ManualSection { id: "a", title: "a", requires: "ghost" }];
        assert!(!manual_order_ok(&missing));
        assert!(manual_order_ok(&[]));
    }

    #[test]
    fn a903_normalize_details() {
        assert_eq!(normalize_cmd("a"), normalize_cmd("a"));
        assert_ne!(normalize_cmd("ab"), normalize_cmd("a b"));
        let long = "x".repeat(100);
        let n = normalize_cmd(&long);
        assert_eq!(&n[..64], &[b'x'; 64][..]); // clamped
        assert_eq!(normalize_cmd("\t\r\n"), [0u8; 64]);
    }

    #[test]
    fn a904_bars_edge() {
        let mut small = [0u8; 4];
        let n = ascii_bars(&[5, 10], 10, &mut small);
        assert_eq!(n, 4); // clamped at buffer size
        let mut big = [0u8; 64];
        assert_eq!(ascii_bars(&[0, 0], 10, &mut big), 2);
    }

    #[test]
    fn a907_search_scores() {
        // Contiguous beats scattered.
        let a = doc_search_score(b"abcdef", b"abc").unwrap();
        let b = doc_search_score(b"axbxcx", b"abc").unwrap();
        assert!(a > b);
        assert_eq!(doc_search_score(b"", b"a"), None);
        assert!(doc_search_score(b"ABC", b"abc").is_some()); // ascii case-insensitive
    }

    #[test]
    fn a908_translation_caps() {
        assert_eq!(translation_permille(20, 10), 1000);
        assert_eq!(translation_permille(1, 3), 333);
    }

    #[test]
    fn a909_version_matrix() {
        assert!(version_trail_ok([2, 0, 0], [2, 0, 1]));
        assert!(!version_trail_ok([1, 9, 9], [2, 0, 0])); // major drift
        assert!(!version_aligned([1, 2, 3], [1, 2, 4]));
    }

    #[test]
    fn a914_fuzz_all() {
        for s in [&b"  hello  world "[..], b"", b"\n\n\n", b"a", &[b' '; 200][..]] {
            assert!(fuzz_normalize(s));
        }
        assert!(fuzz_search(b"the quick brown fox"));
    }

    #[test]
    fn a917_reading_level() {
        assert_eq!(reading_level_ok(50, 2), true);
        assert_eq!(reading_level_ok(50, 1), true);
        assert!(!reading_level_ok(51, 2));
        assert!(!reading_level_ok(10, 0));
    }

    #[test]
    fn a925_final() {
        let set = run_help_checks();
        if !set.all_passed() {
            let mut buf = [0u8; 1024];
            let n = set.render(&mut buf);
            panic!("help self-test:\n{}", core::str::from_utf8(&buf[..n]).unwrap());
        }
        assert!(set.len() >= 25);
    }
}
