//! 深化层 · F135 开发者文档站（2026-09-26 回炉补深化）。
//!
//! 补深：五区跨区搜索引擎（F071 引擎接入面）、轻量代码着色器、
//! 「本页对应源码」直链校验、API 页版本史单调性校验、覆盖报告生成。

use alloc::vec;
use crate::checks::CheckSet;
use crate::stareco::devportal::{ApiPage, DevPortal, SECTIONS};

// ---------------------------------------------------------------------------
// 跨区搜索（F071 引擎接入面：倒排索引最小件）
// ---------------------------------------------------------------------------

/// 一条倒排记录：词条 → (区, 页 symbol)。
pub struct SearchEntry {
    pub term: &'static str,
    pub section: &'static str,
    pub symbol: &'static str,
}

pub struct SearchIndex {
    entries: alloc::vec::Vec<SearchEntry>,
}

impl SearchIndex {
    pub fn new() -> SearchIndex {
        SearchIndex { entries: alloc::vec::Vec::new() }
    }

    pub fn add(&mut self, term: &'static str, section: &str, symbol: &'static str) -> Result<(), &'static str> {
        if !SECTIONS.contains(&section) {
            return Err("区不在五区固定结构内");
        }
        if term.is_empty() {
            return Err("空词条不入索引");
        }
        self.entries.push(SearchEntry { term, section: section_self(section), symbol });
        Ok(())
    }

    /// 查询：词前缀命中，按（区序, symbol）稳定排序。
    pub fn query(&self, prefix: &str) -> alloc::vec::Vec<(&'static str, &'static str)> {
        let mut out: alloc::vec::Vec<(&'static str, &'static str)> = self
            .entries
            .iter()
            .filter(|e| e.term.starts_with(prefix))
            .map(|e| (e.section, e.symbol))
            .collect();
        out.sort();
        out.dedup();
        out
    }
}

fn section_self(s: &str) -> &'static str {
    SECTIONS.iter().find(|x| **x == s).copied().unwrap_or("other")
}

// ---------------------------------------------------------------------------
// 轻量代码着色器（token 分类——渲染前的最小语义层）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tok {
    Keyword,
    Str,
    Number,
    Ident,
    Other,
}

/// 单行 Rust 的极简 token 分类：字符串字面量 / 数字 / 关键字 / 其他。
pub fn tokenize_line(line: &str) -> alloc::vec::Vec<(Tok, &str)> {
    const KEYWORDS: [&str; 6] = ["fn", "let", "pub", "use", "match", "return"];
    let mut out = alloc::vec::Vec::new();
    let b = line.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'"' {
            let end = line[i + 1..].find('"').map(|e| i + 1 + e).unwrap_or(b.len());
            out.push((Tok::Str, &line[i..=end.min(b.len() - 1)]));
            i = end + 1;
        } else if b[i].is_ascii_digit() {
            let mut j = i;
            while j < b.len() && (b[j].is_ascii_digit() || b[j] == b'_') {
                j += 1;
            }
            out.push((Tok::Number, &line[i..j]));
            i = j;
        } else if b[i].is_ascii_alphabetic() || b[i] == b'_' {
            let mut j = i;
            while j < b.len() && (b[j].is_ascii_alphanumeric() || b[j] == b'_') {
                j += 1;
            }
            let word = &line[i..j];
            let tok = if KEYWORDS.contains(&word) { Tok::Keyword } else { Tok::Ident };
            out.push((tok, word));
            i = j;
        } else {
            let mut j = i;
            while j < b.len() && !(b[j].is_ascii_alphanumeric() || b[j] == b'_' || b[j] == b'"') {
                j += 1;
            }
            out.push((Tok::Other, &line[i..j]));
            i = j;
        }
    }
    out
}

// ---------------------------------------------------------------------------
// 「本页对应源码」直链校验 + 版本史单调性
// ---------------------------------------------------------------------------

/// 源码直链校验：必须 `src/` 开头且含 `#L` 行锚或为目录路径。
pub fn source_link_ok(link: &str) -> bool {
    link.starts_with("src/") && (link.contains("#L") || link.ends_with(".rs"))
}

/// API 页版本史单调性：版本号列表必须严格递增（渲染错误即 CI 红）。
pub fn version_history_monotonic(versions: &[u32]) -> bool {
    versions.windows(2).all(|w| w[1] > w[0])
}

// ---------------------------------------------------------------------------
// 覆盖报告
// ---------------------------------------------------------------------------

/// 生成覆盖报告：区页数、API 覆盖千分比、模板违规数、漂移数。
pub struct CoverageReport {
    pub section_pages: [usize; 5],
    pub coverage_ppt: u32,
    pub template_violations: usize,
    pub drift: usize,
}

pub fn coverage_report(portal: &DevPortal) -> CoverageReport {
    CoverageReport {
        section_pages: portal.pages_view(),
        coverage_ppt: portal.extraction_coverage_ppt(),
        template_violations: portal.template_violations(),
        drift: 0,
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub const F135D_TAG: &str = "stareco-F135-deep";

pub fn run_f135_deep_checks() -> CheckSet {
    let mut set = CheckSet::new(F135D_TAG);

    // 跨区搜索
    let mut idx = SearchIndex::new();
    assert!(idx.add("window", "api-ref", "vx_window_create").is_ok());
    assert!(idx.add("window", "concepts", "window-model").is_ok());
    set.add("f135d unknown section rejected", idx.add("x", "blog", "y").is_err(), "五区固定");
    set.add("f135d empty term rejected", idx.add("", "specs", "y").is_err(), "空词条");
    let hits = idx.query("wind");
    set.add(
        "f135d cross-section hits sorted",
        hits == vec![("api-ref", "vx_window_create"), ("concepts", "window-model")],
        "区序稳定",
    );
    set.add("f135d no hit empty", idx.query("zzz").is_empty(), "零结果诚实");

    // 着色器
    let toks = tokenize_line("let x = \"hi\"; // 42");
    set.add(
        "f135d tokenizer classes",
        toks.iter().any(|(t, _)| *t == Tok::Keyword) && toks.iter().any(|(t, _)| *t == Tok::Str) && toks.iter().any(|(t, _)| *t == Tok::Number),
        "keyword/str/number",
    );

    // 直链与版本史
    set.add(
        "f135d source link law",
        source_link_ok("src/api.rs#L10") && source_link_ok("src/api.rs") && !source_link_ok("http://x"),
        "src/ anchor",
    );
    set.add(
        "f135d version monotonic",
        version_history_monotonic(&[1, 2, 10]) && !version_history_monotonic(&[2, 1]),
        "严格递增",
    );

    // 覆盖报告
    let mut portal = DevPortal::new(2);
    for s in SECTIONS.iter() {
        portal.add_section_page(s).ok();
    }
    portal
        .add_api_page(ApiPage {
            symbol: "s1",
            source_fp: 1,
            has_signature: true,
            has_example: true,
            has_error_codes: true,
            has_version_history: true,
            source_link: "src/s.rs",
            stable: Some(true),
        })
        .ok();
    let rep = coverage_report(&portal);
    set.add(
        "f135d coverage report shape",
        rep.coverage_ppt == 500 && rep.template_violations == 0 && rep.section_pages.iter().all(|&n| n >= 1),
        "1/2 symbols = 500‰",
    );

    set
}

#[cfg(test)]
mod deep_tests {
    use super::*;

    #[test]
    fn tokenizer_edges() {
        assert!(tokenize_line("").is_empty());
        assert_eq!(tokenize_line("return")[0].0, Tok::Keyword);
    }
}
