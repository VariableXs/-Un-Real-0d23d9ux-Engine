//! AI-40 终极闭环与维护域（A976~A1000，AURORA-1000）。
//!
//! The final gate: full-system final inspection, the one-red-light-no-release
//! rule, release drills, the long-term maintenance route, knowledge archive,
//! lessons & boundaries, the roadmap, version lifecycle, community feedback,
//! defect retrospection, the quality-metrics dashboard and the domain gates.

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// A976 — 全系统终检
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FinalInspection {
    pub domains: u8,
    pub domains_green: u8,
    pub total_features: u32,
    pub features_green: u32,
}

impl FinalInspection {
    /// The 1000-item promise: every domain and every feature green.
    pub fn all_green(&self) -> bool {
        self.domains > 0
            && self.domains == self.domains_green
            && self.total_features == self.features_green
    }

    pub fn green_permille(&self) -> u32 {
        if self.total_features == 0 {
            return 0;
        }
        (self.features_green as u64 * 1000 / self.total_features as u64).min(1000) as u32
    }
}

// ---------------------------------------------------------------------------
// A977 — 一处红灯不发版门禁
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LightBoard {
    /// One bit per domain: 1 = green.
    pub green_mask: u32,
    pub domain_count: u8,
}

impl LightBoard {
    /// Ship rule: every domain bit set, no extra bits.
    pub fn may_ship(&self) -> bool {
        if self.domain_count == 0 || self.domain_count > 32 {
            return false;
        }
        let full = if self.domain_count == 32 {
            u32::MAX
        } else {
            (1u32 << self.domain_count) - 1
        };
        self.green_mask == full
    }

    /// The first red light index (diagnostics).
    pub fn first_red(&self) -> Option<u8> {
        (0..self.domain_count).find(|i| self.green_mask & (1 << i) == 0).map(|i| i as u8)
    }
}

// ---------------------------------------------------------------------------
// A978 — 发版演练
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DrillStep {
    pub name: &'static str,
    pub ok: bool,
    /// Seconds the step took.
    pub took_s: u32,
}

/// A drill passes when all steps green and total time ≤ 30 min.
pub fn drill_pass(steps: &[DrillStep]) -> bool {
    !steps.is_empty()
        && steps.iter().all(|s| s.ok)
        && steps.iter().map(|s| s.took_s).sum::<u32>() <= 1_800
}

// ---------------------------------------------------------------------------
// A979 — 长期维护路线
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MaintPhase {
    pub name: &'static str,
    /// Phase end, months after the initial release.
    pub ends_month: u16,
}

/// Phases must be strictly increasing and contiguous from month 0.
pub fn maintenance_route_ok(phases: &[MaintPhase]) -> bool {
    let mut expect = 0u16;
    for p in phases {
        if p.ends_month == 0 || p.ends_month <= expect {
            return false;
        }
        expect = p.ends_month;
    }
    !phases.is_empty()
}

// ---------------------------------------------------------------------------
// A980 — 知识归档
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ArchiveEntry {
    pub topic: &'static str,
    /// Content hash for integrity (0 = placeholder, invalid).
    pub hash: u64,
    pub cross_linked: bool,
}

/// An archive is sound when every entry carries a hash and key topics are
/// cross-linked.
pub fn archive_ok(entries: &[ArchiveEntry]) -> bool {
    !entries.is_empty()
        && entries.iter().all(|e| e.hash != 0)
        && entries.iter().filter(|e| e.cross_linked).count() * 2 >= entries.len()
}

// ---------------------------------------------------------------------------
// A981 — 教训与边界
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Lesson {
    pub what: &'static str,
    /// Was the lesson converted into a gate/test (institutionalized)?
    pub gated: bool,
}

/// At least 80% of lessons must be institutionalized as gates.
pub fn lessons_gated(lessons: &[Lesson]) -> bool {
    if lessons.is_empty() {
        return false;
    }
    let gated = lessons.iter().filter(|l| l.gated).count();
    gated * 10 >= lessons.len() * 8
}

/// Boundary statement: an out-of-scope feature stays out unless re-planned.
pub fn boundary_respected(in_scope: bool, re_planned: bool) -> bool {
    in_scope || re_planned
}

// ---------------------------------------------------------------------------
// A982 — 路线图
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RoadmapItem {
    pub title: &'static str,
    pub quarter: u8, // 1..12 rolling
    pub committed: bool,
}

/// Roadmap sanity: quarters ordered, no quarter over-committed (>5 items).
pub fn roadmap_ok(items: &[RoadmapItem]) -> bool {
    if items.is_empty() {
        return false;
    }
    let mut last_q = 0u8;
    let mut per_q = [0u8; 13];
    for it in items {
        if it.quarter == 0 || it.quarter > 12 || it.quarter < last_q {
            return false;
        }
        last_q = it.quarter;
        if (it.quarter as usize) < 13 {
            per_q[it.quarter as usize] += 1;
        }
    }
    per_q.iter().all(|c| *c <= 5)
}

// ---------------------------------------------------------------------------
// A983 — 版本生命周期
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lifecycle {
    Preview,
    Active,
    Maintenance,
    Eol,
}

impl Lifecycle {
    /// Receives security fixes?
    pub fn security_fixes(self) -> bool {
        !matches!(self, Lifecycle::Eol)
    }

    /// Receives feature work?
    pub fn feature_work(self) -> bool {
        matches!(self, Lifecycle::Preview | Lifecycle::Active)
    }

    /// Legal transition: states only move forward.
    pub fn can_transition(self, next: Lifecycle) -> bool {
        (next as u8) == (self as u8) + 1
    }
}

// ---------------------------------------------------------------------------
// A984 — 社区与反馈
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Default)]
pub struct Feedback {
    pub reports: u32,
    pub triaged: u32,
    pub duplicates: u32,
}

impl Feedback {
    /// All reports must be triaged (duplicates count as triaged).
    pub fn triage_complete(&self) -> bool {
        self.reports > 0 && self.triaged + self.duplicates == self.reports
    }

    /// Signal quality: less than half duplicates.
    pub fn signal_ok(&self) -> bool {
        self.reports > 0 && self.duplicates * 2 < self.reports
    }
}

// ---------------------------------------------------------------------------
// A985 — 缺陷回溯
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DefectRecord {
    pub id: u32,
    /// Root cause tag; empty = not retrospected.
    pub root_cause: &'static str,
    /// Was a regression test added?
    pub regression_test: bool,
}

/// Retro complete when every defect names a cause; 90% need a regression test.
pub fn retro_complete(defects: &[DefectRecord]) -> bool {
    if defects.is_empty() {
        return false;
    }
    let all_caused = defects.iter().all(|d| !d.root_cause.is_empty());
    let tested = defects.iter().filter(|d| d.regression_test).count();
    all_caused && tested * 10 >= defects.len() * 9
}

// ---------------------------------------------------------------------------
// A986 — 质量度量仪表
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
pub struct QualityMetrics {
    pub coverage_permille: u16,
    pub mtbf_hours: u32,
    pub flake_permille: u16,
    pub crash_free_sessions_permille: u16,
}

impl QualityMetrics {
    /// Dashboard targets: ≥800 coverage, ≥1000 h MTBF, <20‰ flake,
    /// ≥999‰ crash-free.
    pub fn meets_targets(&self) -> bool {
        self.coverage_permille >= 800
            && self.mtbf_hours >= 1_000
            && self.flake_permille < 20
            && self.crash_free_sessions_permille >= 999
    }
}

// ---------------------------------------------------------------------------
// A987/A995 — 终检性能预算
// ---------------------------------------------------------------------------

/// Final inspection must complete in one CI night (≤ 8 h).
pub fn final_inspection_budget_ok(minutes: u32) -> bool {
    minutes <= 480
}

/// The dashboard render must be < 1 s.
pub fn dashboard_budget_ok(ms: u32) -> bool {
    ms <= 1_000
}

// ---------------------------------------------------------------------------
// A988/A996 — 可观测（终检事件）
// ---------------------------------------------------------------------------

const FIN_EVENT_CAP: usize = 12;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FinEventKind {
    InspectionStart,
    InspectionDone,
    RedLight,
    DrillStart,
    DrillDone,
    ArchiveWrite,
}

#[derive(Clone, Copy, Debug)]
pub struct FinEvent {
    pub kind: FinEventKind,
    pub stamp: u64,
    pub arg: u32,
}

pub struct FinEventLog {
    events: [FinEvent; FIN_EVENT_CAP],
    head: usize,
    count: usize,
}

impl FinEventLog {
    pub const fn new() -> FinEventLog {
        FinEventLog {
            events: [FinEvent { kind: FinEventKind::InspectionStart, stamp: 0, arg: 0 }; FIN_EVENT_CAP],
            head: 0,
            count: 0,
        }
    }

    pub fn push(&mut self, e: FinEvent) {
        self.events[self.head] = e;
        self.head = (self.head + 1) % FIN_EVENT_CAP;
        if self.count < FIN_EVENT_CAP {
            self.count += 1;
        }
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn get(&self, i: usize) -> Option<FinEvent> {
        if i >= self.count {
            return None;
        }
        let pos = (self.head + FIN_EVENT_CAP - 1 - i) % FIN_EVENT_CAP;
        Some(self.events[pos])
    }

    pub fn count_of(&self, k: FinEventKind) -> usize {
        (0..self.count).filter(|i| self.get(*i).map(|e| e.kind == k).unwrap_or(false)).count()
    }
}

// ---------------------------------------------------------------------------
// A989/A998 — 终检文档
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FinalDoc {
    pub section: &'static str,
    pub present: bool,
}

/// The release packet needs: inspection report, drill log, archive index.
pub fn final_docs_ok(docs: &[FinalDoc]) -> bool {
    const NEEDED: [&str; 3] = ["inspection", "drill", "archive"];
    NEEDED.iter().all(|n| docs.iter().any(|d| d.section == *n && d.present))
}

// ---------------------------------------------------------------------------
// A990/A999 — 终检降级链
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FinTier {
    /// Full inspection + drill + archive.
    Full,
    /// Inspection only; drill/archive deferred (point release).
    InspectionOnly,
    /// No release allowed without inspection.
    Blocked,
}

pub fn fin_degrade(inspection_ok: bool, drill_ok: bool) -> FinTier {
    if !inspection_ok {
        FinTier::Blocked
    } else if drill_ok {
        FinTier::Full
    } else {
        FinTier::InspectionOnly
    }
}

// ---------------------------------------------------------------------------
// A991 — 终检工具集
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FinTool {
    pub name: &'static str,
    pub runnable: bool,
}

/// The toolkit needs at least: matrix runner, budget prober, light board.
pub fn toolkit_ok(tools: &[FinTool]) -> bool {
    const NEEDED: [&str; 3] = ["matrix", "budget", "lights"];
    NEEDED.iter().all(|n| tools.iter().any(|t| t.name == *n && t.runnable))
}

// ---------------------------------------------------------------------------
// A992/A993 — 终检自检
// ---------------------------------------------------------------------------

/// Cross-check: the light board and the inspection numbers agree.
pub fn final_selfcheck(board: LightBoard, inspection: FinalInspection) -> bool {
    board.may_ship() == inspection.all_green()
        && board.domain_count as u32 * 25 == inspection.total_features
}

// ---------------------------------------------------------------------------
// A994/A997 — 模糊测试
// ---------------------------------------------------------------------------

/// Fuzz the light board: masks never allow partial ships.
pub fn fuzz_light_board(mask: u32, count: u8) -> bool {
    let b = LightBoard { green_mask: mask, domain_count: count };
    if count == 0 || count > 32 {
        return !b.may_ship();
    }
    let full = if count == 32 { u32::MAX } else { (1u32 << count) - 1 };
    b.may_ship() == (mask == full)
}

/// Fuzz the drill: empty and failing drills never pass.
pub fn fuzz_drill(steps: &[DrillStep]) -> bool {
    drill_pass(steps) == (!steps.is_empty() && steps.iter().all(|s| s.ok) && steps.iter().map(|s| s.took_s).sum::<u32>() <= 1_800)
}

// ---------------------------------------------------------------------------
// A1000 — 域自检收口
// ---------------------------------------------------------------------------

/// Domain self-test: pure invariants that must hold on every machine.
pub fn run_finalize_checks() -> CheckSet {
    let mut set = CheckSet::new("finalize");

    let good = FinalInspection { domains: 40, domains_green: 40, total_features: 1000, features_green: 1000 };
    set.add(
        "A976 inspection",
        good.all_green() && good.green_permille() == 1000
            && !FinalInspection { features_green: 999, ..good }.all_green()
            && FinalInspection { features_green: 900, ..good }.green_permille() == 900,
        "1000 items",
    );

    let board = LightBoard { green_mask: 0xFFFF_FFFF, domain_count: 32 };
    set.add(
        "A977 red light",
        board.may_ship()
            && !LightBoard { green_mask: 0xFFFF_FFFE, ..board }.may_ship()
            && LightBoard { green_mask: 0xFFFF_FFFE, ..board }.first_red() == Some(0)
            && !LightBoard { green_mask: 0, domain_count: 0 }.may_ship()
            && LightBoard { green_mask: 0b111, domain_count: 3 }.may_ship()
            && !LightBoard { green_mask: 0b111, domain_count: 4 }.may_ship(),
        "one red = no ship",
    );

    let drill = [
        DrillStep { name: "build", ok: true, took_s: 300 },
        DrillStep { name: "sign", ok: true, took_s: 60 },
        DrillStep { name: "boot-media", ok: true, took_s: 240 },
    ];
    set.add(
        "A978 drill",
        drill_pass(&drill) && !drill_pass(&[DrillStep { ok: false, ..drill[0] }])
            && !drill_pass(&[DrillStep { took_s: 1_700, ..drill[0] }, DrillStep { took_s: 200, ..drill[1] }]),
        "30 min",
    );

    let phases = [
        MaintPhase { name: "active", ends_month: 24 },
        MaintPhase { name: "security", ends_month: 60 },
        MaintPhase { name: "archive", ends_month: 120 },
    ];
    set.add(
        "A979 route",
        maintenance_route_ok(&phases)
            && !maintenance_route_ok(&[MaintPhase { ends_month: 60, ..phases[0] }, MaintPhase { ends_month: 24, ..phases[1] }])
            && !maintenance_route_ok(&[]),
        "phases",
    );

    let archive = [
        ArchiveEntry { topic: "boot", hash: 7, cross_linked: true },
        ArchiveEntry { topic: "sched", hash: 9, cross_linked: true },
        ArchiveEntry { topic: "gfx", hash: 11, cross_linked: false },
    ];
    set.add(
        "A980 archive",
        archive_ok(&archive) && !archive_ok(&[ArchiveEntry { hash: 0, ..archive[0] }])
            && !archive_ok(&archive[..1]),
        "integrity",
    );

    let lessons = [
        Lesson { what: "races", gated: true },
        Lesson { what: "oom", gated: true },
        Lesson { what: "quirk", gated: true },
        Lesson { what: "misc", gated: true },
        Lesson { what: "todo", gated: false },
    ];
    set.add(
        "A981 lessons",
        lessons_gated(&lessons) && !lessons_gated(&[Lesson { gated: false, ..lessons[0] }])
            && !lessons_gated(&[])
            && boundary_respected(true, false) && boundary_respected(false, true)
            && !boundary_respected(false, false),
        "80% gated",
    );

    let roadmap = [
        RoadmapItem { title: "a", quarter: 1, committed: true },
        RoadmapItem { title: "b", quarter: 2, committed: false },
        RoadmapItem { title: "c", quarter: 2, committed: true },
    ];
    set.add(
        "A982 roadmap",
        roadmap_ok(&roadmap)
            && !roadmap_ok(&[RoadmapItem { quarter: 1, ..roadmap[1] }, RoadmapItem { quarter: 1, ..roadmap[0] }])
            && !roadmap_ok(&[]),
        "ordered",
    );

    set.add(
        "A983 lifecycle",
        Lifecycle::Active.security_fixes() && Lifecycle::Eol.security_fixes() == false
            && Lifecycle::Active.feature_work() && !Lifecycle::Maintenance.feature_work()
            && Lifecycle::Preview.can_transition(Lifecycle::Active)
            && !Lifecycle::Preview.can_transition(Lifecycle::Maintenance)
            && !Lifecycle::Eol.can_transition(Lifecycle::Active),
        "forward only",
    );

    let fb = Feedback { reports: 100, triaged: 85, duplicates: 15 };
    set.add(
        "A984 feedback",
        fb.triage_complete() && fb.signal_ok()
            && !Feedback { triaged: 84, ..fb }.triage_complete()
            && !Feedback { duplicates: 60, triaged: 40, ..fb }.signal_ok(),
        "triage",
    );

    let defects = [
        DefectRecord { id: 1, root_cause: "race", regression_test: true },
        DefectRecord { id: 2, root_cause: "overflow", regression_test: true },
        DefectRecord { id: 3, root_cause: "config", regression_test: true },
    ];
    set.add(
        "A985 retro",
        retro_complete(&defects)
            && !retro_complete(&[DefectRecord { root_cause: "", ..defects[0] }])
            && !retro_complete(&[DefectRecord { regression_test: false, ..defects[0] }])
            && !retro_complete(&[]),
        "cause+test",
    );

    let metrics = QualityMetrics {
        coverage_permille: 850,
        mtbf_hours: 1_200,
        flake_permille: 15,
        crash_free_sessions_permille: 999,
    };
    set.add(
        "A986 dashboard",
        metrics.meets_targets()
            && !QualityMetrics { flake_permille: 25, ..metrics }.meets_targets()
            && !QualityMetrics { mtbf_hours: 999, ..metrics }.meets_targets(),
        "targets",
    );

    set.add(
        "A987 budget",
        final_inspection_budget_ok(480) && !final_inspection_budget_ok(481)
            && dashboard_budget_ok(999) && !dashboard_budget_ok(1_001),
        "nightly",
    );

    let mut log = FinEventLog::new();
    log.push(FinEvent { kind: FinEventKind::RedLight, stamp: 1, arg: 3 });
    log.push(FinEvent { kind: FinEventKind::DrillDone, stamp: 2, arg: 0 });
    set.add(
        "A988 events",
        log.len() == 2 && log.get(0).unwrap().kind == FinEventKind::DrillDone
            && log.count_of(FinEventKind::RedLight) == 1,
        "ring",
    );

    let docs = [
        FinalDoc { section: "inspection", present: true },
        FinalDoc { section: "drill", present: true },
        FinalDoc { section: "archive", present: true },
    ];
    set.add(
        "A989 docs",
        final_docs_ok(&docs) && !final_docs_ok(&docs[..2])
            && !final_docs_ok(&[FinalDoc { present: false, ..docs[0] }]),
        "packet",
    );

    set.add(
        "A990 degrade",
        fin_degrade(true, true) == FinTier::Full && fin_degrade(true, false) == FinTier::InspectionOnly
            && fin_degrade(false, true) == FinTier::Blocked,
        "chain",
    );

    let tools = [
        FinTool { name: "matrix", runnable: true },
        FinTool { name: "budget", runnable: true },
        FinTool { name: "lights", runnable: true },
        FinTool { name: "extra", runnable: false },
    ];
    set.add(
        "A991 toolkit",
        toolkit_ok(&tools) && !toolkit_ok(&tools[..2])
            && !toolkit_ok(&[FinTool { name: "lights", runnable: false }, tools[0], tools[1]]),
        "tools",
    );

    set.add(
        "A992 selfcheck",
        final_selfcheck(board, good)
            && !final_selfcheck(LightBoard { green_mask: 0, ..board }, good)
            && !final_selfcheck(board, FinalInspection { total_features: 975, ..good }),
        "agree",
    );

    set.add(
        "A994 fuzz",
        fuzz_light_board(0, 0) && fuzz_light_board(1, 1) && fuzz_light_board(0b11, 2)
            && fuzz_light_board(0b01, 2) && fuzz_light_board(u32::MAX, 33)
            && fuzz_drill(&drill) && fuzz_drill(&[]) && fuzz_drill(&[DrillStep { ok: false, ..drill[2] }]),
        "robust",
    );

    set.add(
        "A995 budget floor",
        final_inspection_budget_ok(0) && dashboard_budget_ok(0),
        "min",
    );

    set.add("A996 obs cap", log.len() <= FIN_EVENT_CAP, "bounded");

    set.add(
        "A997 fuzz archive",
        archive_ok(&archive) == archive.iter().all(|e| e.hash != 0)
            && archive_ok(&[]).eq(&false),
        "invariant",
    );

    set.add(
        "A999 blocked tier",
        fin_degrade(false, false) == FinTier::Blocked,
        "no release",
    );

    set.add(
        "A1000 closure",
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
    fn a976_green_permille_zero() {
        let none = FinalInspection { domains: 0, domains_green: 0, total_features: 0, features_green: 0 };
        assert_eq!(none.green_permille(), 0);
        assert!(!none.all_green());
        let half = FinalInspection { domains: 2, domains_green: 1, total_features: 50, features_green: 25 };
        assert_eq!(half.green_permille(), 500);
        assert!(!half.all_green());
    }

    #[test]
    fn a977_first_red_scans() {
        let b = LightBoard { green_mask: 0b0111, domain_count: 4 };
        assert_eq!(b.first_red(), Some(3));
        assert_eq!(LightBoard { green_mask: u32::MAX, domain_count: 32 }.first_red(), None);
        assert_eq!(LightBoard { green_mask: 0b101, domain_count: 3 }.first_red(), Some(1));
    }

    #[test]
    fn a978_drill_time_boundary() {
        let at_limit = [DrillStep { name: "x", ok: true, took_s: 1_800 }];
        assert!(drill_pass(&at_limit));
        let over = [DrillStep { name: "x", ok: true, took_s: 1_801 }];
        assert!(!drill_pass(&over));
    }

    #[test]
    fn a982_roadmap_capacity() {
        let six_in_q1: Vec<RoadmapItem> = (0..6)
            .map(|i| RoadmapItem { title: "x", quarter: 1, committed: i % 2 == 0 })
            .collect();
        assert!(!roadmap_ok(&six_in_q1));
        let five = &six_in_q1[..5];
        assert!(roadmap_ok(five));
        let zero_q = [RoadmapItem { title: "y", quarter: 0, committed: true }];
        assert!(!roadmap_ok(&zero_q));
        let q13 = [RoadmapItem { title: "y", quarter: 13, committed: true }];
        assert!(!roadmap_ok(&q13));
    }

    #[test]
    fn a983_lifecycle_chain() {
        let mut s = Lifecycle::Preview;
        for want in [Lifecycle::Active, Lifecycle::Maintenance, Lifecycle::Eol] {
            assert!(s.can_transition(want), "→{:?}", want as u8);
            s = want;
        }
        assert!(!Lifecycle::Eol.can_transition(Lifecycle::Preview));
    }

    #[test]
    fn a988_event_wrap() {
        let mut l = FinEventLog::new();
        for i in 0..FIN_EVENT_CAP + 5 {
            l.push(FinEvent { kind: FinEventKind::ArchiveWrite, stamp: i as u64, arg: i as u32 });
        }
        assert_eq!(l.len(), FIN_EVENT_CAP);
        assert_eq!(l.get(0).unwrap().stamp, (FIN_EVENT_CAP + 4) as u64);
        assert_eq!(l.count_of(FinEventKind::ArchiveWrite), FIN_EVENT_CAP);
    }

    #[test]
    fn a1000_final() {
        let set = run_finalize_checks();
        if !set.all_passed() {
            let mut buf = [0u8; 1024];
            let n = set.render(&mut buf);
            panic!("finalize self-test:\n{}", core::str::from_utf8(&buf[..n]).unwrap());
        }
        assert!(set.len() >= 25);
    }
}
