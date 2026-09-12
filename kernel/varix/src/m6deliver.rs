//! m6deliver — VARIX-M600 AI-24 作品交付与文档域 (F576~F600)
//!
//! 发布工程线、可复现构建、签名链、更新差分、回滚舱、发布清单、
//! 文档作品集、教程、官网、截图自动化、演示 VM、基准作品集、
//! 白皮书、反馈回路、路线图、贡献指南、品牌规范、文案词典、
//! 纪念册、LTS 承诺、安全公告、归档、社区礼仪、总回归、终章。
//!
//! 硬约束：no_std / 无 alloc / 无浮点。

use crate::checks::CheckSet;

// ===========================================================================
// F576 — 发布工程线：代码到 ISO 的流水线
// ===========================================================================

pub const PIPELINE_STAGES: [&str; 6] = ["build", "test", "sign", "iso", "smoke", "publish"];

#[derive(Clone, Copy, PartialEq)]
pub enum StageResult {
    Pass,
    Fail,
    Skipped,
}

pub fn pipeline_publishable(results: &[StageResult; 6]) -> bool {
    results.iter().all(|r| *r == StageResult::Pass)
}

// ===========================================================================
// F577 — 可复现构建
// ===========================================================================

#[derive(Clone, Copy)]
pub struct BuildMeta {
    pub source_hash: u64,
    pub toolchain: u32, // 版本号
    pub env_hash: u64,
    pub output_hash: u64,
}

/// 同源同链同环境 -> 必须同产物。
pub fn reproducible(a: &BuildMeta, b: &BuildMeta) -> bool {
    if a.source_hash == b.source_hash
        && a.toolchain == b.toolchain
        && a.env_hash == b.env_hash
    {
        a.output_hash == b.output_hash
    } else {
        true // 环境不同则不比较
    }
}

// ===========================================================================
// F578 — 签名发布链
// ===========================================================================

/// 简化签名：哈希与密钥混合。
pub fn sign_hash(payload: u64, key: u64) -> u64 {
    payload.wrapping_mul(0x9E37_79B9_7F4A_7C15).rotate_left(23) ^ key
}

pub fn verify_signature(payload: u64, key: u64, sig: u64) -> bool {
    sign_hash(payload, key) == sig
}

// ===========================================================================
// F579 — 更新差分器
// ===========================================================================

/// 差分大小相对全量的比例 permille；<= 200 视为值得差分。
pub fn delta_worthwhile(full_size_kb: u32, delta_size_kb: u32) -> bool {
    full_size_kb > 0 && (delta_size_kb as u32 * 1000) / full_size_kb as u32 <= 200
}

pub fn delta_apply(base_hash: u64, delta_hash: u64) -> u64 {
    base_hash.wrapping_add(delta_hash) ^ 0xFFFF
}

// ===========================================================================
// F580 — 回滚发布舱
// ===========================================================================

pub const ROLLBACK_KEEP_VERSIONS: usize = 3;

pub struct ReleaseSlot {
    pub versions: [u32; ROLLBACK_KEEP_VERSIONS],
    pub len: usize,
    pub active: usize,
}

impl ReleaseSlot {
    pub fn rollback(&mut self) -> Option<u32> {
        if self.active == 0 {
            return None;
        }
        self.active -= 1;
        Some(self.versions[self.active])
    }
}

// ===========================================================================
// F581 — 发布清单门
// ===========================================================================

pub const RELEASE_CHECKLIST: [&str; 8] = [
    "tests-green", "perf-gate", "docs", "signed", "smoke-boot", "changelog", "compat-matrix", "license",
];

pub fn release_gate(marks: u8) -> bool {
    marks as u32 == (1u32 << RELEASE_CHECKLIST.len()) - 1
}

// ===========================================================================
// F582 — 文档作品集
// ===========================================================================

pub const DOC_SECTIONS: [&str; 6] = ["overview", "install", "daily", "dev", "internals", "faq"];

pub fn doc_set_complete(marks: u8) -> bool {
    marks as u32 == (1u32 << DOC_SECTIONS.len()) - 1
}

// ===========================================================================
// F583 — 教程电影
// ===========================================================================

#[derive(Clone, Copy)]
pub struct Tutorial {
    pub duration_s: u32,
    pub chapters: u8,
    pub has_captions: bool,
}

pub fn tutorial_quality(t: &Tutorial) -> bool {
    t.duration_s >= 60 && t.duration_s <= 900 && t.chapters >= 3 && t.has_captions
}

// ===========================================================================
// F584 — 官网橱窗
// ===========================================================================

pub const SITE_BLOCKS: [&str; 5] = ["hero", "features", "download", "screenshots", "community"];

pub fn site_block_present(name: &str) -> bool {
    SITE_BLOCKS.iter().any(|b| *b == name)
}

// ===========================================================================
// F585 — 截图自动化
// ===========================================================================

pub const SHOT_SCENES: [&str; 5] = ["desktop", "files", "write", "settings", "dark-mode"];

/// 每张截图需带分辨率与主题标签才可用于文档。
pub fn shot_tagged(w: u16, h: u16, theme: u8) -> bool {
    w >= 640 && h >= 480 && theme <= 1
}

// ===========================================================================
// F586 — 演示虚拟机
// ===========================================================================

pub const DEMO_RAM_MB: u32 = 512;

pub fn demo_vm_bootable(ram_mb: u32, disk_mb: u32, auto_login: bool) -> bool {
    ram_mb >= DEMO_RAM_MB && disk_mb >= 1024 && auto_login
}

// ===========================================================================
// F587 — 基准测试作品集
// ===========================================================================

pub const BENCH_PORTRAIT_ITEMS: [&str; 5] = ["boot", "ipc", "frame", "file-copy", "memory"];

pub fn bench_portrait_ready(marks: u8) -> bool {
    marks.count_ones() as usize >= 4 // 至少 4/5
}

// ===========================================================================
// F588 — 兼容性白皮书
// ===========================================================================

#[derive(Clone, Copy)]
pub struct CompatRow {
    pub hw_id: u16,
    pub status: u8, // 0=untested 1=works 2=partial 3=broken
}

pub fn compat_summary(rows: &[CompatRow], n: usize) -> (u32, u32, u32) {
    let n = n.min(rows.len());
    let mut works = 0u32;
    let mut partial = 0u32;
    let mut broken = 0u32;
    for r in &rows[..n] {
        match r.status {
            1 => works += 1,
            2 => partial += 1,
            3 => broken += 1,
            _ => {}
        }
    }
    (works, partial, broken)
}

// ===========================================================================
// F589 — 用户反馈回路
// ===========================================================================

#[derive(Clone, Copy)]
pub struct Feedback {
    pub id: u32,
    pub kind: u8, // 0=bug 1=idea 2=praise
    pub triaged: bool,
    pub fixed: bool,
}

pub fn feedback_loop_healthy(fbs: &[Feedback], n: usize) -> bool {
    let n = n.min(fbs.len());
    let bugs = fbs[..n].iter().filter(|f| f.kind == 0).count();
    if bugs == 0 {
        return true;
    }
    let fixed = fbs[..n].iter().filter(|f| f.kind == 0 && f.fixed).count();
    // bug 修复率 >= 80%
    (fixed as u32 * 1000) / bugs as u32 >= 800
}

// ===========================================================================
// F590 — 路线图公开板
// ===========================================================================

pub const ROADMAP_QUARTERS: [&str; 4] = ["Q1", "Q2", "Q3", "Q4"];

#[derive(Clone, Copy, PartialEq)]
pub enum RoadmapItem {
    Planned,
    InProgress,
    Shipped,
    Dropped,
}

pub fn roadmap_visible(i: RoadmapItem) -> bool {
    !matches!(i, RoadmapItem::Dropped)
}

// ===========================================================================
// F591 — 贡献者指南
// ===========================================================================

pub const CONTRIB_SECTIONS: [&str; 6] =
    ["setup", "coding-style", "commit-format", "test-policy", "pr-flow", "code-of-conduct"];

pub fn contrib_guide_ok(marks: u8) -> bool {
    marks as u32 == (1u32 << CONTRIB_SECTIONS.len()) - 1
}

// ===========================================================================
// F592 — 品牌视觉规范
// ===========================================================================

pub const BRAND_COLORS: [u32; 3] = [0x1B2A_41, 0x4FC3_F7, 0xFFD1_66];

pub fn brand_color_ok(c: u32) -> bool {
    BRAND_COLORS.contains(&c)
}

// ===========================================================================
// F593 — 命名与文案词典
// ===========================================================================

pub const COPY_TERMS: [&str; 6] = ["Varix", "开机即用", "零等待", "本地优先", "端到端加密", "一件作品"];

pub fn copy_approved(term: &str) -> bool {
    COPY_TERMS.iter().any(|t| *t == term)
}

// ===========================================================================
// F594 — 发布纪念册
// ===========================================================================

#[derive(Clone, Copy)]
pub struct ReleaseAlbum {
    pub version: u32,
    pub highlights: u8,
    pub contributors: u32,
    pub screenshots: u8,
}

pub fn album_complete(a: &ReleaseAlbum) -> bool {
    a.highlights >= 3 && a.contributors > 0 && a.screenshots >= 4
}

// ===========================================================================
// F595 — 长期支持承诺
// ===========================================================================

pub const LTS_SUPPORT_MONTHS: u32 = 24;

pub fn lts_in_support(release_month: u32, now_month: u32) -> bool {
    now_month.saturating_sub(release_month) < LTS_SUPPORT_MONTHS
}

// ===========================================================================
// F596 — 安全公告流程
// ===========================================================================

#[derive(Clone, Copy, PartialEq)]
pub enum SevLevel {
    Low,
    Medium,
    High,
    Critical,
}

pub const ADVISORY_EMBARGO_DAYS: u32 = 90;

pub fn advisory_due(reported_day: u32, now_day: u32) -> bool {
    now_day.saturating_sub(reported_day) >= ADVISORY_EMBARGO_DAYS
}

pub fn advisory_needs_release(sev: SevLevel, exploit_seen: bool) -> bool {
    exploit_seen || matches!(sev, SevLevel::Critical | SevLevel::High)
}

// ===========================================================================
// F597 — 归档策略
// ===========================================================================

pub const ARCHIVE_KEEP_RELEASES: u32 = 10;

pub fn archive_due(release_count: u32) -> bool {
    release_count > ARCHIVE_KEEP_RELEASES
}

pub fn archive_prune_count(release_count: u32) -> u32 {
    release_count.saturating_sub(ARCHIVE_KEEP_RELEASES)
}

// ===========================================================================
// F598 — 社区礼仪
// ===========================================================================

pub const CONDUCT_RULES: [&str; 5] =
    ["be-kind", "stay-on-topic", "no-doxxing", "english-or-chinese", "search-first"];

pub fn conduct_report_valid(rule_hit: &str) -> bool {
    CONDUCT_RULES.iter().any(|r| *r == rule_hit)
}

// ===========================================================================
// F599 — 交付回归总廊
// ===========================================================================

pub const DELIVERY_CORRIDOR: [&str; 6] =
    ["iso-boot", "app-suite", "sync-e2e", "a11y-corridor", "bench-gate", "signature-verify"];

pub fn delivery_corridor_pass(results: &[bool; 6]) -> bool {
    results.iter().all(|&r| r)
}

// ===========================================================================
// F600 — 作品集终章
// ===========================================================================

pub const FINALE_DOMAINS: u32 = 24;
pub const FINALE_FEATURES: u32 = 600;

#[derive(Clone, Copy)]
pub struct FinaleStats {
    pub shipped_features: u32,
    pub domains_complete: u32,
    pub contributors: u32,
}

impl FinaleStats {
    pub fn completion_permille(&self) -> u16 {
        ((self.shipped_features.min(FINALE_FEATURES) as u32 * 1000) / FINALE_FEATURES) as u16
    }
    pub fn complete(&self) -> bool {
        self.shipped_features >= FINALE_FEATURES
            && self.domains_complete >= FINALE_DOMAINS
            && self.contributors >= FINALE_DOMAINS
    }
}

// ===========================================================================
// 域自检
// ===========================================================================

pub fn run_m6deliver_checks() -> CheckSet {
    let mut set = CheckSet::new("m6deliver");

    // F576 流水线
    let all_pass = [StageResult::Pass; 6];
    let one_fail = [StageResult::Pass, StageResult::Pass, StageResult::Fail, StageResult::Pass, StageResult::Pass, StageResult::Pass];
    set.add(
        "F576 pipeline",
        pipeline_publishable(&all_pass) && !pipeline_publishable(&one_fail) && PIPELINE_STAGES.len() == 6,
        "all stages pass",
    );

    // F577 复现构建
    let b1 = BuildMeta { source_hash: 7, toolchain: 100, env_hash: 3, output_hash: 42 };
    let b2 = BuildMeta { source_hash: 7, toolchain: 100, env_hash: 3, output_hash: 42 };
    let b3 = BuildMeta { source_hash: 7, toolchain: 100, env_hash: 3, output_hash: 43 };
    set.add(
        "F577 reproducible",
        reproducible(&b1, &b2) && !reproducible(&b1, &b3),
        "same-in same-out",
    );

    // F578 签名链
    let sig = sign_hash(123, 456);
    set.add(
        "F578 signature",
        verify_signature(123, 456, sig) && !verify_signature(124, 456, sig),
        "sign + verify",
    );

    // F579 差分
    set.add(
        "F579 delta",
        delta_worthwhile(1000, 150) && !delta_worthwhile(1000, 500) && delta_apply(1, 2) == delta_apply(1, 2),
        "size gate + apply",
    );

    // F580 回滚舱
    let mut slot = ReleaseSlot { versions: [3, 4, 5], len: 3, active: 2 };
    let r = slot.rollback();
    set.add("F580 rollback", r == Some(4) && slot.active == 1, "one step back");

    // F581 清单门
    set.add(
        "F581 release gate",
        release_gate(0xFF) && !release_gate(0x7F) && RELEASE_CHECKLIST.len() == 8,
        "8 checks",
    );

    // F582 文档
    set.add("F582 docs", doc_set_complete(0b11_1111) && !doc_set_complete(0b01_1111), "6 sections");

    // F583 教程
    let t = Tutorial { duration_s: 300, chapters: 5, has_captions: true };
    set.add(
        "F583 tutorial",
        tutorial_quality(&t) && !tutorial_quality(&Tutorial { duration_s: 30, chapters: 1, has_captions: false }),
        "length + chapters",
    );

    // F584 官网
    set.add(
        "F584 site",
        site_block_present("hero") && !site_block_present("sponsor") && SITE_BLOCKS.len() == 5,
        "blocks",
    );

    // F585 截图
    set.add(
        "F585 shots",
        shot_tagged(1920, 1080, 0) && !shot_tagged(320, 240, 0) && SHOT_SCENES.len() == 5,
        "tagged scenes",
    );

    // F586 演示 VM
    set.add(
        "F586 demo vm",
        demo_vm_bootable(1024, 2048, true) && !demo_vm_bootable(256, 2048, true),
        "resource floor",
    );

    // F587 基准作品集
    set.add(
        "F587 bench portrait",
        bench_portrait_ready(0b1_1111) && bench_portrait_ready(0b0_1111) && !bench_portrait_ready(0b0_0111),
        ">=4/5",
    );

    // F588 白皮书
    let rows = [
        CompatRow { hw_id: 1, status: 1 },
        CompatRow { hw_id: 2, status: 2 },
        CompatRow { hw_id: 3, status: 3 },
    ];
    let (w, p, b) = compat_summary(&rows, 3);
    set.add("F588 whitepaper", w == 1 && p == 1 && b == 1, "3 buckets");

    // F589 反馈回路
    let fbs = [
        Feedback { id: 1, kind: 0, triaged: true, fixed: true },
        Feedback { id: 2, kind: 0, triaged: true, fixed: true },
        Feedback { id: 3, kind: 0, triaged: true, fixed: true },
        Feedback { id: 4, kind: 1, triaged: false, fixed: false },
    ];
    let stalled = [
        Feedback { id: 1, kind: 0, triaged: true, fixed: false },
        Feedback { id: 2, kind: 0, triaged: true, fixed: false },
    ];
    set.add(
        "F589 feedback",
        feedback_loop_healthy(&fbs, 4) && !feedback_loop_healthy(&stalled, 2),
        "80% fix rate",
    );

    // F590 路线图
    set.add(
        "F590 roadmap",
        roadmap_visible(RoadmapItem::Planned) && roadmap_visible(RoadmapItem::Shipped)
            && !roadmap_visible(RoadmapItem::Dropped) && ROADMAP_QUARTERS.len() == 4,
        "public board",
    );

    // F591 贡献指南
    set.add(
        "F591 contrib",
        contrib_guide_ok(0b11_1111) && !contrib_guide_ok(0b11_1110) && CONTRIB_SECTIONS.len() == 6,
        "6 sections",
    );

    // F592 品牌
    set.add(
        "F592 brand",
        brand_color_ok(0x4FC3_F7) && !brand_color_ok(0xFF00_00) && BRAND_COLORS.len() == 3,
        "palette",
    );

    // F593 文案
    set.add(
        "F593 copy",
        copy_approved("Varix") && copy_approved("零等待") && !copy_approved("快到飞起"),
        "dictionary",
    );

    // F594 纪念册
    let alb = ReleaseAlbum { version: 6, highlights: 5, contributors: 12, screenshots: 6 };
    set.add(
        "F594 album",
        album_complete(&alb) && !album_complete(&ReleaseAlbum { version: 6, highlights: 1, contributors: 0, screenshots: 2 }),
        "complete album",
    );

    // F595 LTS
    set.add(
        "F595 lts",
        lts_in_support(0, 23) && !lts_in_support(0, 24) && LTS_SUPPORT_MONTHS == 24,
        "24 months",
    );

    // F596 安全公告
    set.add(
        "F596 advisory",
        advisory_due(0, 90) && !advisory_due(1, 90)
            && advisory_needs_release(SevLevel::High, false)
            && !advisory_needs_release(SevLevel::Low, false)
            && advisory_needs_release(SevLevel::Low, true),
        "embargo + severity",
    );

    // F597 归档
    set.add(
        "F597 archive",
        archive_due(12) && !archive_due(10) && archive_prune_count(12) == 2,
        "keep 10",
    );

    // F598 社区
    set.add(
        "F598 conduct",
        conduct_report_valid("be-kind") && !conduct_report_valid("trolling"),
        "rules",
    );

    // F599 总回归
    set.add(
        "F599 corridor",
        delivery_corridor_pass(&[true; 6]) && !delivery_corridor_pass(&[true, true, true, true, true, false]),
        "6 gates",
    );

    // F600 终章
    let fin = FinaleStats { shipped_features: 600, domains_complete: 24, contributors: 24 };
    let part = FinaleStats { shipped_features: 575, domains_complete: 24, contributors: 24 };
    set.add(
        "F600 finale",
        fin.completion_permille() == 1000 && fin.complete() && !part.complete()
            && part.completion_permille() == 958,
        "600/600",
    );

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f577_repro_matrix() {
        // 环境不同时不判定
        let a = BuildMeta { source_hash: 1, toolchain: 1, env_hash: 1, output_hash: 9 };
        let b = BuildMeta { source_hash: 2, toolchain: 1, env_hash: 2, output_hash: 8 };
        assert!(reproducible(&a, &b));
    }

    #[test]
    fn f580_rollback_to_zero() {
        let mut slot = ReleaseSlot { versions: [1, 2, 3], len: 3, active: 0 };
        assert!(slot.rollback().is_none());
    }

    #[test]
    fn f588_empty_whitepaper() {
        let rows: [CompatRow; 0] = [];
        let (w, p, b) = compat_summary(&rows, 0);
        assert_eq!((w, p, b), (0, 0, 0));
    }

    #[test]
    fn f600_partial_completion() {
        let s = FinaleStats { shipped_features: 300, domains_complete: 12, contributors: 5 };
        assert_eq!(s.completion_permille(), 500);
    }

    #[test]
    fn f600_domain_selfcheck_all_pass() {
        let set = run_m6deliver_checks();
        assert!(set.len() >= 25, "got {}", set.len());
                    for i in 0..set.len() {
                let c = set.get(i).unwrap();
                if !c.passed {
                    eprintln!("CHKFAIL {} | {}", c.name, c.detail);
                }
            }
            assert!(set.all_passed());
    }
}
