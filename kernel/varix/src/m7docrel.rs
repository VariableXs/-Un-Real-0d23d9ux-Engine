//! m7docrel — VARIX-M700 AI-28 内核文档与发布域 (F676~F700)
//!
//! 文档宪法、文档-代码对账、注释礼仪、错误码词典、API 参考生成、
//! 架构叙事、变更日志、版本宪法、发布工程线、发布清单、回滚舱、
//! 签名链、纪念册、文档金样、统计分账、事件流、健康分、自描述导出、
//! 压力剧本、回归走廊、新人指南、术语词典、预算官、自检入口、终章。
//!
//! 硬约束：no_std / 无 alloc / 无浮点。

use crate::checks::CheckSet;

// ===========================================================================
// F676 — 文档宪法：分层
// ===========================================================================

pub const DOC_LAYERS: [&str; 3] = ["spec", "guide", "reference"];

pub fn doc_layer_known(name: &str) -> bool {
    DOC_LAYERS.iter().any(|l| *l == name)
}

// ===========================================================================
// F677 — 文档-代码对账官
// ===========================================================================

#[derive(Clone, Copy)]
pub struct DocRef {
    pub symbol: &'static str,
    pub still_in_code: bool,
}

/// 文档引用的符号必须真实存在；返回失配数。
pub fn doc_code_mismatch(refs: &[DocRef], n: usize) -> u32 {
    let n = n.min(refs.len());
    refs[..n].iter().filter(|r| !r.still_in_code).count() as u32
}

pub fn doc_sync_gate(refs: &[DocRef], n: usize) -> bool {
    doc_code_mismatch(refs, n) == 0
}

// ===========================================================================
// F678 — 注释礼仪规范
// ===========================================================================

pub const COMMENT_LANGS: [&str; 2] = ["zh", "en"];

pub fn comment_lang_ok(lang: &str) -> bool {
    COMMENT_LANGS.iter().any(|l| *l == lang)
}

/// 注释密度：每 100 行代码 10~40 行注释为健康。
pub fn comment_density_ok(code_lines: u32, comment_lines: u32) -> bool {
    if code_lines == 0 {
        return false;
    }
    let permille = (comment_lines as u64 * 1_000) / code_lines as u64;
    (100..=400).contains(&permille)
}

// ===========================================================================
// F679 — 错误码词典
// ===========================================================================

pub const ERRNO_OK: i32 = 0;
pub const ERRNO_INVAL: i32 = 22;
pub const ERRNO_NOMEM: i32 = 12;
pub const ERRNO_NOSYS: i32 = 38;
pub const ERRNO_PERM: i32 = 1;
pub const ERRNO_FAULT: i32 = 14;

pub fn errno_documented(code: i32) -> bool {
    matches!(code, ERRNO_OK | ERRNO_INVAL | ERRNO_NOMEM | ERRNO_NOSYS | ERRNO_PERM | ERRNO_FAULT)
}

pub fn errno_usage_ok(code: i32) -> bool {
    errno_documented(code) && code >= 0
}

// ===========================================================================
// F680 — API 参考生成器
// ===========================================================================

#[derive(Clone, Copy)]
pub struct ApiEntry {
    pub name: &'static str,
    pub documented: bool,
    pub has_example: bool,
}

pub fn api_ref_quality(entries: &[ApiEntry], n: usize) -> u16 {
    let n = n.min(entries.len());
    if n == 0 {
        return 0;
    }
    let good = entries[..n].iter().filter(|e| e.documented && e.has_example).count();
    ((good as u64 * 1000) / n as u64) as u16
}

// ===========================================================================
// F681 — 架构叙事书
// ===========================================================================

pub const ARCH_CHAPTERS: [&str; 6] =
    ["boot", "memory", "processes", "filesystem", "network", "drivers"];

pub fn arch_book_complete(marks: u8) -> bool {
    marks as u32 == (1u32 << ARCH_CHAPTERS.len()) - 1
}

// ===========================================================================
// F682 — 变更日志官
// ===========================================================================

pub const CHANGELOG_KINDS: [&str; 5] = ["added", "changed", "fixed", "removed", "security"];

pub fn changelog_entry_valid(kind: &str, desc_len: usize) -> bool {
    CHANGELOG_KINDS.iter().any(|k| *k == kind) && desc_len >= 8
}

// ===========================================================================
// F683 — 版本宪法：语义化版本
// ===========================================================================

#[derive(Clone, Copy, PartialEq)]
pub enum VersionBump {
    Major,
    Minor,
    Patch,
}

pub fn semver_bump_legal(current_major: u32, bump: VersionBump, abi_break: bool) -> bool {
    match bump {
        VersionBump::Major => true,
        VersionBump::Minor | VersionBump::Patch => !abi_break || current_major == 0,
    }
}

// ===========================================================================
// F684 — 发布工程线
// ===========================================================================

pub const RELEASE_ARTIFACTS: [&str; 4] = ["kernel-elf", "varix.iso", "userspace-bundle", "symbols-debug"];

pub fn artifacts_present(marks: u8) -> bool {
    marks & 0b0111 == 0b0111 // 前三件必需，debug 符号可选
}

// ===========================================================================
// F685 — 发布清单门
// ===========================================================================

pub const M700_RELEASE_CHECKLIST: [&str; 6] =
    ["ktest-green", "kbuild-clean", "qemu-boot", "docs-sync", "signed", "changelog"];

pub fn m700_release_gate(marks: u8) -> bool {
    marks as u32 == (1u32 << M700_RELEASE_CHECKLIST.len()) - 1
}

// ===========================================================================
// F686 — 回滚发布舱
// ===========================================================================

pub const REL_KEEP_VERSIONS: usize = 3;

pub struct ReleaseRing {
    pub versions: [u32; REL_KEEP_VERSIONS],
    pub len: usize,
    pub active: usize,
}

impl ReleaseRing {
    pub fn rollback(&mut self) -> Option<u32> {
        if self.active == 0 {
            return None;
        }
        self.active -= 1;
        Some(self.versions[self.active])
    }
}

// ===========================================================================
// F687 — 发布签名链
// ===========================================================================

pub fn rel_sign(payload: u64, key: u64) -> u64 {
    payload.rotate_left(17) ^ key.wrapping_mul(0x9E37_79B9_7F4A_7C15)
}

pub fn rel_verify(payload: u64, key: u64, sig: u64) -> bool {
    rel_sign(payload, key) == sig
}

// ===========================================================================
// F688 — 发布纪念册
// ===========================================================================

#[derive(Clone, Copy)]
pub struct RelAlbum {
    pub version: u32,
    pub features: u16,
    pub fixes: u16,
    pub contributors: u16,
}

pub fn rel_album_complete(a: &RelAlbum) -> bool {
    a.features > 0 && a.fixes > 0 && a.contributors >= 3
}

// ===========================================================================
// F689 — 文档金样本
// ===========================================================================

pub const GOLDEN_DOC_STYLE: [&str; 5] =
    ["说人话", "给例子", "给原因", "说清边界", "可运行"];

pub fn doc_style_point(name: &str) -> bool {
    GOLDEN_DOC_STYLE.iter().any(|s| *s == name)
}

// ===========================================================================
// F690 — 文档统计分账
// ===========================================================================

#[derive(Clone, Copy)]
pub struct DocStat {
    pub layer: u8,
    pub pages: u32,
    pub fresh_days: u32, // 距上次更新
}

pub const DOC_STALE_DAYS: u32 = 90;

pub fn doc_stale(s: &DocStat) -> bool {
    s.fresh_days > DOC_STALE_DAYS
}

pub fn doc_pages_total(stats: &[DocStat], n: usize) -> u32 {
    let n = n.min(stats.len());
    stats[..n].iter().map(|s| s.pages).sum()
}

// ===========================================================================
// F691 — 文档事件流
// ===========================================================================

#[derive(Clone, Copy, PartialEq)]
pub enum DocEvent {
    Created,
    Updated,
    StaleFlagged,
    Deprecated,
}

pub fn doc_event_loggable(e: DocEvent) -> bool {
    matches!(e, DocEvent::Created | DocEvent::Updated | DocEvent::StaleFlagged | DocEvent::Deprecated)
}

// ===========================================================================
// F692 — 文档健康分
// ===========================================================================

pub struct DocHealth {
    pub total_pages: u32,
    pub stale_pages: u32,
    pub broken_links: u32,
}

impl DocHealth {
    pub fn grade(&self) -> u8 {
        if self.stale_pages == 0 && self.broken_links == 0 {
            0
        } else if self.broken_links == 0 && self.stale_pages * 10 <= self.total_pages {
            1
        } else {
            2
        }
    }
}

// ===========================================================================
// F693 — 文档自描述导出
// ===========================================================================

pub struct DocExport {
    pub layers: u8,
    pub pages: u32,
    pub stale_count: u32,
}

pub fn doc_export_valid(e: &DocExport) -> bool {
    (e.layers as usize) <= DOC_LAYERS.len() && e.pages >= e.stale_count
}

// ===========================================================================
// F694 — 文档压力剧本
// ===========================================================================

pub const DOC_BUILD_PLAYS: [&str; 3] = ["full-site", "incremental", "single-page"];

pub fn doc_build_play_known(name: &str) -> bool {
    DOC_BUILD_PLAYS.iter().any(|p| *p == name)
}

// ===========================================================================
// F695 — 文档回归走廊
// ===========================================================================

pub const DOC_CORRIDOR_CASES: [&str; 5] =
    ["links-valid", "code-blocks-compile", "toc-sync", "glossary-hit", "style-golden"];

pub fn doc_corridor_pass(results: &[bool; 5]) -> bool {
    results.iter().all(|&r| r)
}

// ===========================================================================
// F696 — 新人指南：三日上手
// ===========================================================================

pub const ONBOARD_DAYS: [&str; 3] = ["day1-build-and-boot", "day2-read-boot-path", "day3-first-patch"];

pub fn onboard_day_known(name: &str) -> bool {
    ONBOARD_DAYS.iter().any(|d| *d == name)
}

// ===========================================================================
// F697 — 术语词典
// ===========================================================================

pub const KERNEL_TERMS: [&str; 6] =
    ["HHDM", "KASLR", "TLB", "命名空间", "控制组", "VAE"];

pub fn term_defined(term: &str) -> bool {
    KERNEL_TERMS.iter().any(|t| *t == term)
}

// ===========================================================================
// F698 — 文档预算官
// ===========================================================================

pub const DOC_BUILD_BUDGET_S: u32 = 120;

pub fn doc_build_budget_ok(seconds: u32) -> bool {
    seconds <= DOC_BUILD_BUDGET_S
}

// ===========================================================================
// F699 — 文档自检入口
// ===========================================================================

pub const DOC_SELFTESTS: [&str; 4] = ["link-check", "code-block-check", "glossary-check", "stale-check"];

pub fn doc_selftest_known(name: &str) -> bool {
    DOC_SELFTESTS.iter().any(|s| *s == name)
}

// ===========================================================================
// F700 — 内核域终章：全量索引
// ===========================================================================

pub const FINALE_M700_FEATURES: u32 = 700;
pub const FINALE_M700_DOMAINS: u32 = 28;

#[derive(Clone, Copy)]
pub struct M700Finale {
    pub features_done: u32,
    pub domains_done: u32,
}

impl M700Finale {
    pub fn completion_permille(&self) -> u16 {
        ((self.features_done.min(FINALE_M700_FEATURES) as u64 * 1000) / FINALE_M700_FEATURES as u64) as u16
    }
    pub fn complete(&self) -> bool {
        self.features_done >= FINALE_M700_FEATURES && self.domains_done >= FINALE_M700_DOMAINS
    }
}

// ===========================================================================
// 域自检
// ===========================================================================

pub fn run_m7docrel_checks() -> CheckSet {
    let mut set = CheckSet::new("m7docrel");

    // F676 宪法
    set.add(
        "F676 layers",
        doc_layer_known("spec") && doc_layer_known("reference") && !doc_layer_known("blog"),
        "3 layers",
    );

    // F677 对账
    let refs = [
        DocRef { symbol: "kernel_boot", still_in_code: true },
        DocRef { symbol: "old_api_removed", still_in_code: false },
    ];
    set.add(
        "F677 reconcile",
        doc_code_mismatch(&refs, 2) == 1 && !doc_sync_gate(&refs, 2)
            && doc_sync_gate(&refs[..1], 1),
        "stale symbol flagged",
    );

    // F678 注释礼仪
    set.add(
        "F678 comments",
        comment_lang_ok("zh") && !comment_lang_ok("de")
            && comment_density_ok(100, 20) && !comment_density_ok(100, 5) && !comment_density_ok(100, 50),
        "lang + density",
    );

    // F679 错误码
    set.add(
        "F679 errno",
        errno_documented(ERRNO_INVAL) && !errno_documented(999)
            && errno_usage_ok(ERRNO_NOMEM) && !errno_usage_ok(-1),
        "dictionary",
    );

    // F680 API 参考
    let apis = [
        ApiEntry { name: "vm_map", documented: true, has_example: true },
        ApiEntry { name: "vm_unmap", documented: true, has_example: false },
        ApiEntry { name: "vm_chg", documented: false, has_example: false },
    ];
    set.add(
        "F680 api ref",
        api_ref_quality(&apis, 3) == 333,
        "documented+example rate",
    );

    // F681 叙事书
    set.add(
        "F681 arch book",
        arch_book_complete(0b11_1111) && !arch_book_complete(0b11_1101) && ARCH_CHAPTERS.len() == 6,
        "6 chapters",
    );

    // F682 变更日志
    set.add(
        "F682 changelog",
        changelog_entry_valid("fixed", 12) && !changelog_entry_valid("stuff", 12) && !changelog_entry_valid("fixed", 3),
        "kind + desc",
    );

    // F683 版本宪法
    set.add(
        "F683 semver",
        semver_bump_legal(3, VersionBump::Major, true)
            && !semver_bump_legal(3, VersionBump::Minor, true)
            && semver_bump_legal(3, VersionBump::Minor, false)
            && semver_bump_legal(0, VersionBump::Minor, true),
        "abi break -> major",
    );

    // F684 工程线
    set.add(
        "F684 artifacts",
        artifacts_present(0b1111) && artifacts_present(0b0111) && !artifacts_present(0b0011),
        "3 required",
    );

    // F685 清单门
    set.add(
        "F685 gate",
        m700_release_gate(0b11_1111) && !m700_release_gate(0b11_1110),
        "6 checks",
    );

    // F686 回滚舱
    let mut ring = ReleaseRing { versions: [3, 4, 5], len: 3, active: 2 };
    set.add(
        "F686 rollback",
        ring.rollback() == Some(4) && ring.rollback() == Some(3) && ring.rollback().is_none(),
        "sequential back",
    );

    // F687 签名链
    let sig = rel_sign(0x1234, 0xABCD);
    set.add(
        "F687 signature",
        rel_verify(0x1234, 0xABCD, sig) && !rel_verify(0x1235, 0xABCD, sig),
        "sign/verify",
    );

    // F688 纪念册
    let alb = RelAlbum { version: 7, features: 25, fixes: 40, contributors: 12 };
    set.add(
        "F688 album",
        rel_album_complete(&alb) && !rel_album_complete(&RelAlbum { version: 7, features: 0, fixes: 0, contributors: 1 }),
        "complete",
    );

    // F689 金样
    set.add(
        "F689 doc golden",
        doc_style_point("说人话") && doc_style_point("可运行") && !doc_style_point("晦涩"),
        "5 style points",
    );

    // F690 分账
    let stats = [
        DocStat { layer: 0, pages: 30, fresh_days: 10 },
        DocStat { layer: 1, pages: 50, fresh_days: 120 },
        DocStat { layer: 2, pages: 20, fresh_days: 1 },
    ];
    set.add(
        "F690 stats",
        doc_stale(&stats[1]) && !doc_stale(&stats[0]) && doc_pages_total(&stats, 3) == 100,
        "stale + total",
    );

    // F691 事件流
    set.add(
        "F691 events",
        doc_event_loggable(DocEvent::StaleFlagged) && doc_event_loggable(DocEvent::Deprecated),
        "4 events",
    );

    // F692 健康分
    let h = DocHealth { total_pages: 100, stale_pages: 0, broken_links: 0 };
    let hb = DocHealth { total_pages: 100, stale_pages: 30, broken_links: 5 };
    set.add("F692 health", h.grade() == 0 && hb.grade() == 2, "stale/link grade");

    // F693 导出
    let ex = DocExport { layers: 3, pages: 100, stale_count: 2 };
    set.add(
        "F693 export",
        doc_export_valid(&ex) && !doc_export_valid(&DocExport { layers: 5, pages: 1, stale_count: 2 }),
        "layers <= 3",
    );

    // F694 压力
    set.add(
        "F694 build plays",
        doc_build_play_known("incremental") && !doc_build_play_known("wild"),
        "3 plays",
    );

    // F695 走廊
    set.add(
        "F695 corridor",
        doc_corridor_pass(&[true; 5]) && !doc_corridor_pass(&[true, false, true, true, true]),
        "5 cases",
    );

    // F696 新人指南
    set.add(
        "F696 onboard",
        onboard_day_known("day1-build-and-boot") && onboard_day_known("day3-first-patch"),
        "3 days",
    );

    // F697 术语
    set.add(
        "F697 terms",
        term_defined("HHDM") && term_defined("控制组") && !term_defined("随便"),
        "6 terms",
    );

    // F698 预算
    set.add(
        "F698 budget",
        doc_build_budget_ok(120) && !doc_build_budget_ok(121) && DOC_BUILD_BUDGET_S == 120,
        "2min cap",
    );

    // F699 自检
    set.add(
        "F699 selftest",
        doc_selftest_known("link-check") && !doc_selftest_known("vibes-check"),
        "4 selftests",
    );

    // F700 终章
    let fin = M700Finale { features_done: 700, domains_done: 28 };
    let part = M700Finale { features_done: 500, domains_done: 20 };
    set.add(
        "F700 finale",
        fin.complete() && fin.completion_permille() == 1000
            && !part.complete() && part.completion_permille() == 714,
        "700/700",
    );

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f686_ring_bounds() {
        let mut ring = ReleaseRing { versions: [1, 2, 3], len: 3, active: 0 };
        assert!(ring.rollback().is_none());
    }

    #[test]
    fn f683_zero_major_pre_release() {
        assert!(semver_bump_legal(0, VersionBump::Patch, true));
    }

    #[test]
    fn f690_empty_stats() {
        let stats: [DocStat; 0] = [];
        assert_eq!(doc_pages_total(&stats, 0), 0);
    }

    #[test]
    fn f700_round_down() {
        let f = M700Finale { features_done: 699, domains_done: 28 };
        assert_eq!(f.completion_permille(), 998);
    }

    #[test]
    fn f700_domain_selfcheck_all_pass() {
        let set = run_m7docrel_checks();
        assert!(set.len() >= 25, "got {}", set.len());
        for i in 0..set.len() {
            let c = set.get(i).unwrap();
            assert!(c.passed, "{} | {}", c.name, c.detail);
        }
    }
}
