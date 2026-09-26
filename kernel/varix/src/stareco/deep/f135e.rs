//! 深化层二 · F135 开发者文档站（2026-09-26 深化批次二）。
//!
//! 补深主册【数据与存储】「API 表从代码注释自动提取（可执行文档）」
//! +【设计细节】每 API 页四段模板（主册 G-D-10）：API 提取器
//! （rustdoc 思路——符号清单→页数据→缺失段报告→覆盖率口径）、五区
//! 内容清单管理、多词 AND 跨区搜索、示例链接校验、直链表与漂移报告。

use crate::checks::CheckSet;
use crate::stareco::devportal::{ApiPage, DevPortal, SECTIONS};
use crate::stareco::ebase;

// ---------------------------------------------------------------------------
// API 提取器：符号注册面 → 文档页数据（rustdoc 思路的最小机）
// ---------------------------------------------------------------------------

/// 源码侧符号卡（提取输入：CI 从代码注记生成）。
pub struct SymbolCard {
    pub symbol: &'static str,
    /// 注记指纹（`#doc` 段 FNV——与 devportal::vx_doc_fp 同源）。
    pub source_fp: u64,
    /// 四段模板齐套位。
    pub has_signature: bool,
    pub has_example: bool,
    pub has_error_codes: bool,
    pub has_version_history: bool,
    pub source_link: &'static str,
    /// 级别标注（Some(true)=稳定 / Some(false)=实验 / None=未标注——未标注即提取失败）。
    pub stability: Option<bool>,
}

impl SymbolCard {
    /// 提取器门禁：未标注级别的符号不出页（100% 标注判据的提取侧）。
    pub fn extractable(&self) -> bool {
        self.stability.is_some()
            && self.has_signature
            && self.has_example
            && self.has_error_codes
            && self.has_version_history
            && !self.symbol.is_empty()
            && self.source_fp != 0
    }

    /// 转文档页（仅 extractable 通过后调用）。
    pub fn to_page(&self) -> ApiPage {
        ApiPage {
            symbol: self.symbol,
            source_fp: self.source_fp,
            has_signature: self.has_signature,
            has_example: self.has_example,
            has_error_codes: self.has_error_codes,
            has_version_history: self.has_version_history,
            source_link: self.source_link,
            stable: self.stability,
        }
    }
}

/// 提取报告：进页/缺段/未标注三分清单（每符号一条去向，零静默）。
pub struct ExtractReport {
    pub pages_ok: usize,
    /// 因模板缺段未进页的符号。
    pub missing_sections: alloc::vec::Vec<&'static str>,
    /// 因级别未标注未进页的符号。
    pub unannotated: alloc::vec::Vec<&'static str>,
}

/// 批量提取：按序进门户（页满/重名错误如实返回——调用方处理）。
pub fn extract_all(portal: &mut DevPortal, cards: &[SymbolCard]) -> ExtractReport {
    let mut rep = ExtractReport { pages_ok: 0, missing_sections: alloc::vec::Vec::new(), unannotated: alloc::vec::Vec::new() };
    for c in cards {
        if !c.extractable() {
            if c.stability.is_none() {
                rep.unannotated.push(c.symbol);
            } else {
                rep.missing_sections.push(c.symbol);
            }
            continue;
        }
        if portal.add_api_page(c.to_page()).is_ok() {
            rep.pages_ok += 1;
        } else {
            rep.missing_sections.push(c.symbol);
        }
    }
    rep
}

// ---------------------------------------------------------------------------
// 五区内容清单管理：区序 + 最低页数 + 缺页清单（核对清单全绿判据）
// ---------------------------------------------------------------------------

/// 五区核对结果：区名/现有页/最低要求/达标。
pub struct SectionAudit {
    pub rows: alloc::vec::Vec<(&'static str, usize, usize, bool)>,
    pub all_green: bool,
}

pub fn audit_sections(portal: &DevPortal) -> SectionAudit {
    let pages = portal.pages_view();
    let mut rows = alloc::vec::Vec::new();
    let mut all = true;
    for (i, s) in SECTIONS.iter().enumerate() {
        let ok = pages[i] >= crate::stareco::devportal::SECTION_MIN_PAGES[i];
        if !ok {
            all = false;
        }
        rows.push((*s, pages[i], crate::stareco::devportal::SECTION_MIN_PAGES[i], ok));
    }
    SectionAudit { rows, all_green: all }
}

// ---------------------------------------------------------------------------
// 跨区搜索深化：多词 AND + 区过滤（F071 引擎接入面二阶）
// ---------------------------------------------------------------------------

/// 多词 AND 查询：每个词都命中的 (区, 符号) 才返回——词表为空 = 无结果
/// （空查询不出全表，防误触全量导出）。
pub fn query_and(
    entries: &[(&'static str, &'static str, &'static str)], // (term, section, symbol)
    terms: &[&str],
    section_filter: Option<&str>,
) -> alloc::vec::Vec<(&'static str, &'static str)> {
    if terms.is_empty() {
        return alloc::vec::Vec::new();
    }
    let mut hits: alloc::vec::Vec<(&'static str, &'static str)> = alloc::vec::Vec::new();
    for (_, sec, sym) in entries {
        if let Some(f) = section_filter {
            if *sec != f {
                continue;
            }
        }
        let all = terms.iter().all(|t| entries.iter().any(|(term, s2, sym2)| *term == *t && *s2 == *sec && *sym2 == *sym));
        if all {
            let key = (*sec, *sym);
            if !hits.contains(&key) {
                hits.push(key);
            }
        }
    }
    hits.sort();
    hits
}

// ---------------------------------------------------------------------------
// 示例链接校验：文档页 → F136 示例号
// ---------------------------------------------------------------------------

/// 直链格式 `examples/<1-5>`——越界/非数字/前导零即拒（示例号固定 1..=5）。
pub fn example_link_ok(link: &str) -> bool {
    let Some(rest) = link.strip_prefix("examples/") else {
        return false;
    };
    if rest.len() > 1 && rest.starts_with('0') {
        return false; // 前导零：格式纪律（05 不是合法示例号）
    }
    match rest.parse::<u8>() {
        Ok(n) => (1..=5).contains(&n),
        Err(_) => false,
    }
}

// ---------------------------------------------------------------------------
// 直链表与漂移报告
// ---------------------------------------------------------------------------

pub struct SourceMap {
    /// (页 symbol, 源码路径, 注记指纹)。
    maps: alloc::vec::Vec<(&'static str, &'static str, u64)>,
}

impl SourceMap {
    pub fn new() -> SourceMap {
        SourceMap { maps: alloc::vec::Vec::new() }
    }

    pub fn register(&mut self, symbol: &'static str, path: &'static str, fp: u64) -> Result<(), &'static str> {
        if symbol.is_empty() || path.is_empty() {
            return Err("页与路径必填：直链不许悬空");
        }
        if fp == 0 {
            return Err("零指纹不入表：指纹缺失等于没有对账锚点");
        }
        if self.maps.iter().any(|(s, _, _)| *s == symbol) {
            return Err("页重复登记：一页一条直链");
        }
        self.maps.push((symbol, path, fp));
        Ok(())
    }

    /// 漂移对拍：给定源码当前指纹提取器，逐一核对——不一致即漂移条目。
    pub fn drift_check(&self, current_fp_of: impl Fn(&str) -> Option<u64>) -> alloc::vec::Vec<&'static str> {
        self.maps
            .iter()
            .filter(|(sym, _, fp)| current_fp_of(sym).map_or(true, |cur| cur != *fp))
            .map(|(sym, _, _)| *sym)
            .collect()
    }

    pub fn len(&self) -> usize {
        self.maps.len()
    }
}

/// 页直链存在性审计：页里的 source_link 必须在直链表中（防悬空引用）。
pub fn dangling_links(portal_pages: &[ApiPage], map: &SourceMap) -> alloc::vec::Vec<&'static str> {
    portal_pages
        .iter()
        .filter(|p| p.source_link.is_empty() || !link_registered(map, p.symbol))
        .map(|p| p.symbol)
        .collect()
}

fn link_registered(map: &SourceMap, symbol: &str) -> bool {
    map.linked(symbol)
}

impl SourceMap {
    fn linked(&self, symbol: &str) -> bool {
        self.maps.iter().any(|(s, _, _)| *s == symbol)
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub const F135E_TAG: &str = "stareco-F135-deep2";

pub fn run_f135_deep2_checks() -> CheckSet {
    let mut set = CheckSet::new(F135E_TAG);

    // 提取器：齐套进页 / 缺段 / 未标注 / 重名
    let mut portal = DevPortal::new(4);
    for s in SECTIONS {
        let _ = portal.add_section_page(s);
    }
    let cards = alloc::vec![
        SymbolCard {
            symbol: "vx_window_create",
            source_fp: ebase::fnv1a64(b"#doc window create"),
            has_signature: true,
            has_example: true,
            has_error_codes: true,
            has_version_history: true,
            source_link: "src/window.rs",
            stability: Some(true),
        },
        SymbolCard {
            symbol: "vx_theme_set",
            source_fp: ebase::fnv1a64(b"#doc theme set"),
            has_signature: true,
            has_example: false, // 缺示例段
            has_error_codes: true,
            has_version_history: true,
            source_link: "src/theme.rs",
            stability: Some(false),
        },
        SymbolCard {
            symbol: "vx_clip_read",
            source_fp: ebase::fnv1a64(b"#doc clip"),
            has_signature: true,
            has_example: true,
            has_error_codes: true,
            has_version_history: true,
            source_link: "src/clip.rs",
            stability: None, // 未标注级别
        },
    ];
    let rep = extract_all(&mut portal, &cards);
    set.add("f135e extract ok", rep.pages_ok == 1, "齐套符号进页");
    set.add(
        "f135e extract missing",
        rep.missing_sections == alloc::vec!["vx_theme_set"],
        "缺段清单点名",
    );
    set.add(
        "f135e extract unannotated",
        rep.unannotated == alloc::vec!["vx_clip_read"],
        "未标注级别点名",
    );

    // 五区核对
    let audit = audit_sections(&portal);
    set.add("f135e sections rows", audit.rows.len() == 5, "五区全核对");
    set.add("f135e sections verdict", !audit.all_green, "每区一页不满足最低线（核对器不虚报）");

    // 多词 AND
    let entries: alloc::vec::Vec<(&'static str, &'static str, &'static str)> = alloc::vec![
        ("window", "api-ref", "vx_window_create"),
        ("theme", "api-ref", "vx_theme_set"),
        ("window", "concepts", "window-model"),
        ("theme", "specs", "token-table"),
    ];
    set.add(
        "f135e and query",
        query_and(&entries, &["window"], None) == alloc::vec![("api-ref", "vx_window_create"), ("concepts", "window-model")],
        "单词多命中",
    );
    let both = query_and(&entries, &["window", "theme"], None);
    set.add("f135e and strict", both.is_empty(), "无同符号双词命中 = 空（AND 语义）");
    set.add(
        "f135e and filter",
        query_and(&entries, &["window"], Some("api-ref")) == alloc::vec![("api-ref", "vx_window_create")],
        "区过滤生效",
    );
    set.add("f135e and empty terms", query_and(&entries, &[], None).is_empty(), "空词表不出全表");

    // 示例链接
    set.add("f135e exlink ok", example_link_ok("examples/3"), "合法示例号");
    set.add("f135e exlink zero", !example_link_ok("examples/0"), "0 越界");
    set.add("f135e exlink six", !example_link_ok("examples/6"), "6 越界（只有五例）");
    set.add("f135e exlink junk", !example_link_ok("examples/abc"), "非数字拒绝");
    set.add("f135e exlink bare", !example_link_ok("examples/"), "空号拒绝");

    // 直链表
    let mut map = SourceMap::new();
    let _ = map.register("vx_window_create", "src/window.rs", 111);
    set.add("f135e map reg", map.len() == 1, "登记成功");
    set.add("f135e map dup", map.register("vx_window_create", "x", 1).is_err(), "重复页拒绝");
    set.add("f135e map zero fp", map.register("vx_clip_read", "x", 0).is_err(), "零指纹拒绝");
    set.add(
        "f135e map drift",
        map.drift_check(|s| if s == "vx_window_create" { Some(222) } else { None }) == alloc::vec!["vx_window_create"],
        "指纹漂移点名",
    );
    set.add("f135e map fresh", map.drift_check(|s| if s == "vx_window_create" { Some(111) } else { None }).is_empty(), "指纹一致零漂移");

    // 悬空直链
    let pages = alloc::vec![portal_add_helper(&mut portal)];
    let dangling = dangling_links(&pages, &map);
    set.add(
        "f135e dangling",
        dangling == alloc::vec!["vx_theme_set"],
        "直链未登记的页点名",
    );

    set
}

/// 自检辅助：补一页缺段页（直链悬空样本）。
fn portal_add_helper(portal: &mut DevPortal) -> ApiPage {
    let p = ApiPage {
        symbol: "vx_theme_set",
        source_fp: 9,
        has_signature: true,
        has_example: false,
        has_error_codes: true,
        has_version_history: true,
        source_link: "src/theme.rs",
        stable: Some(false),
    };
    let _ = portal.add_api_page(p);
    p
}

#[cfg(test)]
mod deep2_tests {
    use super::*;

    #[test]
    fn extractor_coverage_math() {
        let mut portal = DevPortal::new(2);
        for s in SECTIONS {
            let _ = portal.add_section_page(s);
        }
        let cards = alloc::vec![
            SymbolCard {
                symbol: "a",
                source_fp: 1,
                has_signature: true,
                has_example: true,
                has_error_codes: true,
                has_version_history: true,
                source_link: "a.rs",
                stability: Some(true),
            },
            SymbolCard {
                symbol: "b",
                source_fp: 2,
                has_signature: false,
                has_example: true,
                has_error_codes: true,
                has_version_history: true,
                source_link: "b.rs",
                stability: Some(true),
            },
        ];
        let rep = extract_all(&mut portal, &cards);
        assert_eq!(rep.pages_ok, 1);
        // 提取覆盖率 = 进页数 / 公共符号数——50% 低于 90% 线 → 门禁红
        assert!(!portal.coverage_green());
    }

    #[test]
    fn source_map_link_check() {
        let mut m = SourceMap::new();
        assert!(m.register("s", "p.rs", 5).is_ok());
        assert!(m.linked("s"));
        assert!(!m.linked("t"));
        assert!(m.register("", "p", 1).is_err());
    }

    #[test]
    fn example_link_boundaries() {
        assert!(example_link_ok("examples/1"));
        assert!(example_link_ok("examples/5"));
        assert!(!example_link_ok("examples/05")); // 前导零照拒——格式纪律
        assert!(!example_link_ok("example/1"));
    }
}
