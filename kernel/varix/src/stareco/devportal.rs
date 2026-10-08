//! F135 开发者文档站 · 完整设计（STAR I 主册 G-D-10）。
//!
//! **判据（主册）**：五区内容完整度核对清单全绿；API 提取覆盖率 >90%
//! （公共接口口径）。
//!
//! **设计要点（主册）**：五区固定结构（快速开始/API 参考/概念指南/
//! 规范/词典）；复用 F119 帮助中心渲染器；每 API 页必含四段（签名/
//! 示例/错误码/版本史——模板门禁）；API 表从代码注释自动提取（CI 校验
//! ——注释与文档冲突即红）；「本页对应源码」直链；搜索跨五区。
//!
//! 本模块是文档站的**内容完整性核对核**：五区清单、API 页四段模板
//! 门禁、提取覆盖率计算（ebase 覆盖度口径）、注释-文档一致性校验。

use alloc::vec::Vec;
use crate::checks::CheckSet;
use crate::stareco::ebase::{coverage_is_beta, coverage_ppt, fnv1a64};

// ---------------------------------------------------------------------------
// 五区固定结构
// ---------------------------------------------------------------------------

pub const SECTIONS: [&str; 5] = ["quickstart", "api-ref", "concepts", "specs", "dictionary"];

/// 五区内容完整度：每区最低页数（核对清单的量化线）。
pub const SECTION_MIN_PAGES: [usize; 5] = [3, 1, 3, 3, 1];

// ---------------------------------------------------------------------------
// API 页四段模板门禁
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ApiPage {
    pub symbol: &'static str,
    /// 源码注记指纹（注释里 `#doc` 标记段的 FNV）——CI 用它对拍。
    pub source_fp: u64,
    /// 四段：签名/示例/错误码/版本史。
    pub has_signature: bool,
    pub has_example: bool,
    pub has_error_codes: bool,
    pub has_version_history: bool,
    /// 「本页对应源码」直链。
    pub source_link: &'static str,
    /// 级别标注（F137 两级——文档页徽标数据源）。
    pub stable: Option<bool>,
}

impl ApiPage {
    /// 四段模板门禁：缺一段即页红（渲染不出口）。
    pub fn template_ok(&self) -> bool {
        self.has_signature && self.has_example && self.has_error_codes && self.has_version_history
    }
}

// ---------------------------------------------------------------------------
// 门户
// ---------------------------------------------------------------------------

pub struct DevPortal {
    /// 每区页数计数。
    pages_per_section: [usize; 5],
    api_pages: [Option<ApiPage>; 24],
    api_count: usize,
    /// 公共接口全集（提取源——代码注记清单）。
    pub public_symbols: usize,
}

impl DevPortal {
    pub fn new(public_symbols: usize) -> DevPortal {
        DevPortal {
            pages_per_section: [0; 5],
            api_pages: [None; 24],
            api_count: 0,
            public_symbols,
        }
    }

    pub fn add_section_page(&mut self, section: &str) -> Result<(), &'static str> {
        for (i, s) in SECTIONS.iter().enumerate() {
            if *s == section {
                self.pages_per_section[i] += 1;
                return Ok(());
            }
        }
        Err("unknown section")
    }

    /// 五区核对清单（判据前半句）。
    pub fn sections_complete(&self) -> bool {
        self.pages_per_section
            .iter()
            .zip(SECTION_MIN_PAGES.iter())
            .all(|(have, min)| have >= min)
    }

    pub fn add_api_page(&mut self, page: ApiPage) -> Result<(), &'static str> {
        if self.api_count >= 24 {
            return Err("api pages full");
        }
        self.api_pages[self.api_count] = Some(page);
        self.api_count += 1;
        Ok(())
    }

    /// API 提取覆盖率（千分比，ebase 口径）：有页的符号 / 公共符号。
    /// total=0 视为 0（无接口谈不上覆盖）。
    pub fn extraction_coverage_ppt(&self) -> u32 {
        coverage_ppt(self.api_count, self.public_symbols)
    }

    /// 各区页数只读视图（覆盖报告用）。
    pub fn pages_view(&self) -> [usize; 5] {
        self.pages_per_section
    }

    /// 判据线：>90%。
    pub fn coverage_green(&self) -> bool {
        !coverage_is_beta(self.api_count, self.public_symbols.max(1)) || self.public_symbols == 0
    }

    /// 模板门禁扫描：返回四段不齐的页数（>0 = CI 红）。
    pub fn template_violations(&self) -> usize {
        self.api_pages[..self.api_count]
            .iter()
            .flatten()
            .filter(|p| !p.template_ok())
            .count()
    }

    /// 注释-文档一致性：页面指纹与源码注记指纹逐条对拍。
    /// 返回不一致符号数（判据「文档漂移即 CI 红」）。
    pub fn drift_check(&self, source_fp_of: impl Fn(&str) -> Option<u64>) -> usize {
        self.api_pages[..self.api_count]
            .iter()
            .flatten()
            .filter(|p| source_fp_of(p.symbol) != Some(p.source_fp))
            .count()
    }

    /// 未覆盖符号清单（F136 示例/补页工作面的数据源）。
    pub fn missing_symbols(&self, known: &[&'static str]) -> Vec<&'static str> {
        let mut out = Vec::new();
        for sym in known {
            let covered = self
                .api_pages[..self.api_count]
                .iter()
                .flatten()
                .any(|p| p.symbol == *sym);
            if !covered {
                out.push(*sym);
            }
        }
        out
    }
}

/// 源码注记指纹的规范算法：`fn vx_doc_fp(comment_text: &str) -> u64`。
/// 注释与文档用同一算法——一处一事实。
pub fn vx_doc_fp(comment_text: &str) -> u64 {
    fnv1a64(comment_text.as_bytes())
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub const F135_TAG: &str = "stareco-F135-devportal";

pub fn run_devportal_checks() -> CheckSet {
    let mut set = CheckSet::new(F135_TAG);

    let mut portal = DevPortal::new(12);
    // 五区补齐
    for (sec, min) in SECTIONS.iter().zip(SECTION_MIN_PAGES.iter()) {
        for _ in 0..*min {
            portal.add_section_page(sec).expect("section");
        }
    }
    set.add("f135 five sections complete", portal.sections_complete(), "checklist green");
    set.add("f135 unknown section rejected", portal.add_section_page("blog").is_err(), "fixed structure");

    // API 页：12 公共符号，11 有页 → 覆盖率 11/12 = 91.6% > 90% 绿
    for i in 0..11u32 {
        let sym = match i {
            0 => "vx_window_create",
            1 => "vx_window_close",
            2 => "vx_draw_rect",
            3 => "vx_draw_text",
            4 => "vx_input_poll",
            5 => "vx_theme_get",
            6 => "vx_theme_set",
            7 => "vx_fs_open",
            8 => "vx_fs_read",
            9 => "vx_clip_get",
            _ => "vx_clip_set",
        };
        portal
            .add_api_page(ApiPage {
                symbol: sym,
                source_fp: vx_doc_fp(sym),
                has_signature: true,
                has_example: true,
                has_error_codes: true,
                has_version_history: true,
                source_link: "src/api.rs",
                stable: Some(true),
            })
            .expect("api page");
    }
    let cov = portal.extraction_coverage_ppt();
    set.add("f135 coverage >90%", cov > 900 && portal.coverage_green(), "11/12 pages");
    set.add("f135 template gate clean", portal.template_violations() == 0, "four sections each");

    // 注入一个四段缺页 → 门禁红
    portal
        .add_api_page(ApiPage {
            symbol: "vx_bad_symbol",
            source_fp: 7,
            has_signature: true,
            has_example: false,
            has_error_codes: true,
            has_version_history: false,
            source_link: "x",
            stable: Some(false),
        })
        .expect("bad page in");
    set.add("f135 template violation caught", portal.template_violations() == 1, "CI red on missing sections");

    // 覆盖率下探：<90% 判红
    let mut thin = DevPortal::new(20);
    thin.add_section_page("quickstart").ok();
    set.add("f135 thin portal red", !thin.coverage_green() && thin.extraction_coverage_ppt() < 900, "1/20");

    // 漂移检测
    let drifted = portal.drift_check(|sym| if sym == "vx_bad_symbol" { Some(99) } else { Some(vx_doc_fp(sym)) });
    set.add("f135 drift detected", drifted == 1, "doc-source mismatch");

    // 未覆盖清单
    let known = ["vx_window_create", "vx_clip_set", "vx_not_documented", "vx_also_missing"];
    let missing = portal.missing_symbols(&known);
    set.add(
        "f135 missing symbols listed",
        missing.contains(&"vx_not_documented") && missing.contains(&"vx_also_missing") && missing.len() == 2,
        "work queue",
    );

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coverage_boundary() {
        // 90% 整 = 不达标（判据 >90%）；91% 达标
        assert!(!coverage_green_at(9, 10));
        assert!(coverage_green_at(11, 12));
    }

    fn coverage_green_at(covered: usize, total: usize) -> bool {
        coverage_ppt(covered, total) > 900
    }
}
