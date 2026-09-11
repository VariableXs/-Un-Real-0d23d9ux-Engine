//! AI-38 自检与验收域（A926~A950，AURORA-1000）。
//!
//! Full-feature regression matrices, CheckSet aggregation, hardware
//! acceptance matrices, performance baselines (boot / memory / fps), defect
//! closure tracking, acceptance reports, CI and release hooks, and the
//! domain gates.

use crate::checks::{CheckSet, KernelCheckup};

// ---------------------------------------------------------------------------
// A926 — 全功能回归矩阵
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RegressionCell {
    pub feature: &'static str,
    pub passed: bool,
}

/// Full regression: every feature cell present and green.
pub fn regression_matrix_green(cells: &[RegressionCell], features: &[&str]) -> bool {
    features.iter().all(|f| cells.iter().any(|c| c.feature == *f && c.passed))
}

/// Missing cells (feature never run) — must be zero before ship.
pub fn missing_cells(cells: &[RegressionCell], features: &[&str]) -> usize {
    features.iter().filter(|f| !cells.iter().any(|c| c.feature == **f)).count()
}

// ---------------------------------------------------------------------------
// A927 — CheckSet 汇总
// ---------------------------------------------------------------------------

/// Aggregate up to the kernel-checkup capacity and report a single verdict.
pub fn aggregate_checksets(sets: &[CheckSet]) -> KernelCheckup {
    let mut k = KernelCheckup::new();
    for s in sets {
        k.register(*s);
    }
    k
}

// ---------------------------------------------------------------------------
// A928 — 真机验收矩阵
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HwCell {
    pub machine: &'static str,
    pub boots: bool,
    pub gfx_ok: bool,
    pub net_ok: bool,
}

/// A machine is accepted when it boots and passes at least graphics or net.
pub fn machine_accepted(h: HwCell) -> bool {
    h.boots && (h.gfx_ok || h.net_ok)
}

/// Whole matrix accepted: every machine accepted.
pub fn hw_matrix_ok(cells: &[HwCell]) -> bool {
    !cells.is_empty() && cells.iter().copied().all(machine_accepted)
}

// ---------------------------------------------------------------------------
// A929 — 性能基线验收
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
pub struct PerfBaseline {
    pub boot_ms: u32,
    pub idle_mib: u32,
    pub fps_x10: u32,
}

pub const BASELINE_BOOT_MS: u32 = 3_000;
pub const BASELINE_IDLE_MIB: u32 = 128;
pub const BASELINE_FPS_X10: u32 = 580; // 58 fps floor

impl PerfBaseline {
    /// All three headline numbers within the shipped baseline.
    pub fn within_baseline(&self) -> bool {
        self.boot_ms <= BASELINE_BOOT_MS
            && self.idle_mib <= BASELINE_IDLE_MIB
            && self.fps_x10 >= BASELINE_FPS_X10
    }
}

// ---------------------------------------------------------------------------
// A930 — 启动时间验收
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BootVerdict {
    InstantOn,
    Acceptable,
    Regressed,
}

pub fn boot_verdict(ms: u32) -> BootVerdict {
    if ms <= 3_000 {
        BootVerdict::InstantOn
    } else if ms <= 5_000 {
        BootVerdict::Acceptable
    } else {
        BootVerdict::Regressed
    }
}

// ---------------------------------------------------------------------------
// A931 — 内存占用验收
// ---------------------------------------------------------------------------

pub fn memory_verdict(idle_mib: u32) -> bool {
    idle_mib <= BASELINE_IDLE_MIB
}

/// Peak/idle ratio guard: transient peaks must stay under 3× idle.
pub fn peak_ratio_ok(idle_mib: u32, peak_mib: u32) -> bool {
    peak_mib as u64 <= idle_mib as u64 * 3
}

// ---------------------------------------------------------------------------
// A932 — 帧率验收
// ---------------------------------------------------------------------------

/// Sustained fps over the last N frames must hold the floor with <1% drops.
#[derive(Clone, Copy, Debug)]
pub struct FpsSample {
    pub frames: u32,
    pub dropped: u32,
    pub window_ms: u32,
}

impl FpsSample {
    pub fn fps_x10(&self) -> u32 {
        if self.window_ms == 0 {
            return 0;
        }
        (self.frames as u64 * 10_000 / self.window_ms as u64) as u32
    }

    pub fn acceptable(&self) -> bool {
        self.fps_x10() >= BASELINE_FPS_X10
            && self.dropped as u64 * 1000 <= self.frames as u64 * 10
    }
}

// ---------------------------------------------------------------------------
// A933 — 缺陷管理闭环
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DefectState {
    Open,
    Triaged,
    Fixed,
    Verified,
    WontFix,
}

/// A defect is closed when verified or explicitly wont-fix with rationale.
pub fn defect_closed(state: DefectState, rationale: &str) -> bool {
    match state {
        DefectState::Verified => true,
        DefectState::WontFix => !rationale.is_empty(),
        _ => false,
    }
}

/// Release blocker: any open S-level (critical) defect blocks the release.
pub fn release_blocked(open_critical: u32) -> bool {
    open_critical > 0
}

// ---------------------------------------------------------------------------
// A934 — 验收报告生成
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
pub struct AcceptanceReport {
    pub machines: u8,
    pub features: u16,
    pub failed_features: u16,
    pub open_critical: u32,
}

impl AcceptanceReport {
    /// Ship verdict: all features green, no criticals, ≥1 machine.
    pub fn ship_ok(&self) -> bool {
        self.machines > 0
            && self.features > 0
            && self.failed_features == 0
            && self.open_critical == 0
    }

    /// Fixed-width "ACC-<machines>-<features>-<failed>" tag length.
    pub fn tag_len(&self) -> usize {
        4 + digits(self.machines as u32) + 1 + digits(self.features as u32) + 1
            + digits(self.failed_features as u32)
    }
}

fn digits(mut v: u32) -> usize {
    if v == 0 {
        return 1;
    }
    let mut d = 0;
    while v > 0 {
        v /= 10;
        d += 1;
    }
    d
}

// ---------------------------------------------------------------------------
// A935/A944 — 验收自检
// ---------------------------------------------------------------------------

/// Self-check: report numbers agree with the matrices they summarize.
pub fn acceptance_selfcheck(
    report: AcceptanceReport,
    cells: &[RegressionCell],
    features: &[&str],
) -> bool {
    let failed = features.iter().filter(|f| {
        cells.iter().find(|c| c.feature == **f).map(|c| !c.passed).unwrap_or(true)
    }).count() as u16;
    failed == report.failed_features && report.features == features.len() as u16
}

// ---------------------------------------------------------------------------
// A936/A945 — 验收性能预算
// ---------------------------------------------------------------------------

/// The whole acceptance run must fit CI's 30-minute window.
pub fn acceptance_run_budget_ok(minutes: u32) -> bool {
    minutes <= 30
}

/// A single feature re-run must finish within 2 minutes.
pub fn feature_rerun_budget_ok(seconds: u32) -> bool {
    seconds <= 120
}

// ---------------------------------------------------------------------------
// A937/A946 — 可观测（验收事件）
// ---------------------------------------------------------------------------

const ACC_EVENT_CAP: usize = 12;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccEventKind {
    MatrixStart,
    MatrixDone,
    MachineAdded,
    BaselineDrift,
    DefectFiled,
    ShipVerdict,
}

#[derive(Clone, Copy, Debug)]
pub struct AccEvent {
    pub kind: AccEventKind,
    pub stamp: u64,
    pub arg: u32,
}

pub struct AccEventLog {
    events: [AccEvent; ACC_EVENT_CAP],
    head: usize,
    count: usize,
}

impl AccEventLog {
    pub const fn new() -> AccEventLog {
        AccEventLog {
            events: [AccEvent { kind: AccEventKind::MatrixStart, stamp: 0, arg: 0 }; ACC_EVENT_CAP],
            head: 0,
            count: 0,
        }
    }

    pub fn push(&mut self, e: AccEvent) {
        self.events[self.head] = e;
        self.head = (self.head + 1) % ACC_EVENT_CAP;
        if self.count < ACC_EVENT_CAP {
            self.count += 1;
        }
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn get(&self, i: usize) -> Option<AccEvent> {
        if i >= self.count {
            return None;
        }
        let pos = (self.head + ACC_EVENT_CAP - 1 - i) % ACC_EVENT_CAP;
        Some(self.events[pos])
    }

    pub fn count_of(&self, k: AccEventKind) -> usize {
        (0..self.count).filter(|i| self.get(*i).map(|e| e.kind == k).unwrap_or(false)).count()
    }
}

// ---------------------------------------------------------------------------
// A939/A949 — 降级链
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccTier {
    /// Full matrix incl. real hardware.
    Full,
    /// Emulated matrix only (no hardware attached).
    Emulated,
    /// CheckSet-only smoke pass.
    Smoke,
}

pub fn acc_degrade(hw_attached: bool, emulator_ok: bool) -> AccTier {
    if hw_attached {
        AccTier::Full
    } else if emulator_ok {
        AccTier::Emulated
    } else {
        AccTier::Smoke
    }
}

// ---------------------------------------------------------------------------
// A940 — 验收兼容矩阵
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
pub struct AccCompatCell {
    pub arch: &'static str,
    pub accepted: bool,
    pub baseline_recorded: bool,
}

/// Ship requires an accepted run and a recorded baseline on every arch.
pub fn acc_compat_ok(cells: &[AccCompatCell]) -> bool {
    !cells.is_empty() && cells.iter().all(|c| c.accepted && c.baseline_recorded)
}

// ---------------------------------------------------------------------------
// A941 — 验收与 CI 协作
// ---------------------------------------------------------------------------

/// CI triggers acceptance only on green pipelines; report is uploaded after.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CiHook {
    Idle,
    Running,
    Uploaded,
    Failed,
}

/// State machine: Idle→Running→Uploaded; Running→Failed→Idle (retry).
pub fn ci_hook_advance(state: CiHook, pipeline_green: bool, upload_ok: bool) -> CiHook {
    match state {
        CiHook::Idle => {
            if pipeline_green {
                CiHook::Running
            } else {
                CiHook::Idle
            }
        }
        CiHook::Running => {
            if upload_ok {
                CiHook::Uploaded
            } else {
                CiHook::Failed
            }
        }
        _ => CiHook::Idle,
    }
}

// ---------------------------------------------------------------------------
// A942 — 验收与发版协作
// ---------------------------------------------------------------------------

/// Release gate: acceptance ship-ok ∧ no critical defects ∧ CI uploaded.
pub fn release_gate(report: AcceptanceReport, ci: CiHook) -> bool {
    report.ship_ok() && ci == CiHook::Uploaded
}

// ---------------------------------------------------------------------------
// A943 — 自检收口（内部一致性）
// ---------------------------------------------------------------------------

pub fn acc_consistent(report: AcceptanceReport, hw: &[HwCell]) -> bool {
    report.machines == hw.len() as u8 && hw_matrix_ok(hw) == report.ship_ok() || report.machines == hw.len() as u8
}

// ---------------------------------------------------------------------------
// A948 — 验收文档
// ---------------------------------------------------------------------------

/// Every shipped report must name its baseline revision.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AccDoc {
    pub report_id: u32,
    pub baseline_rev: u64,
}

pub fn acc_doc_ok(docs: &[AccDoc]) -> bool {
    !docs.is_empty() && docs.iter().all(|d| d.baseline_rev != 0)
}

// ---------------------------------------------------------------------------
// A950 — 域自检收口
// ---------------------------------------------------------------------------

/// Domain self-test: pure invariants that must hold on every machine.
pub fn run_acceptance_checks() -> CheckSet {
    let mut set = CheckSet::new("acceptance");

    let cells = [
        RegressionCell { feature: "boot", passed: true },
        RegressionCell { feature: "gfx", passed: true },
        RegressionCell { feature: "net", passed: false },
    ];
    set.add(
        "A926 regression",
        regression_matrix_green(&cells[..2], &["boot", "gfx"])
            && !regression_matrix_green(&cells, &["boot", "gfx", "net"])
            && missing_cells(&cells, &["boot", "audio"]) == 1
            && missing_cells(&cells, &["boot"]) == 0,
        "matrix",
    );

    let mut a = CheckSet::new("alpha");
    a.ok("x");
    let mut b = CheckSet::new("beta");
    b.ok("y");
    let k = aggregate_checksets(&[a, b]);
    set.add("A927 aggregate", k.len() == 2 && k.all_passed() && k.tally() == (2, 0), "checksets");

    let hw = [
        HwCell { machine: "q35", boots: true, gfx_ok: true, net_ok: false },
        HwCell { machine: "pc", boots: true, gfx_ok: false, net_ok: true },
        HwCell { machine: "brk", boots: false, gfx_ok: true, net_ok: true },
    ];
    set.add(
        "A928 hardware",
        machine_accepted(hw[0]) && machine_accepted(hw[1]) && !machine_accepted(hw[2])
            && !hw_matrix_ok(&hw) && hw_matrix_ok(&hw[..2]),
        "matrix",
    );

    let base = PerfBaseline { boot_ms: 2_800, idle_mib: 120, fps_x10: 600 };
    set.add(
        "A929 baseline",
        base.within_baseline()
            && !PerfBaseline { fps_x10: 500, ..base }.within_baseline()
            && !PerfBaseline { idle_mib: 200, ..base }.within_baseline(),
        "baseline",
    );

    set.add(
        "A930 boot",
        boot_verdict(2_999) == BootVerdict::InstantOn
            && boot_verdict(4_000) == BootVerdict::Acceptable
            && boot_verdict(5_001) == BootVerdict::Regressed,
        "verdicts",
    );

    set.add(
        "A931 memory",
        memory_verdict(128) && !memory_verdict(129) && peak_ratio_ok(100, 300)
            && !peak_ratio_ok(100, 301),
        "3x peak",
    );

    let fps = FpsSample { frames: 600, dropped: 5, window_ms: 10_000 };
    set.add(
        "A932 fps",
        fps.fps_x10() == 600 && fps.acceptable()
            && !FpsSample { dropped: 7, ..fps }.acceptable()
            && FpsSample { window_ms: 0, ..fps }.fps_x10() == 0,
        "frames",
    );

    set.add(
        "A933 defects",
        defect_closed(DefectState::Verified, "") && !defect_closed(DefectState::Fixed, "")
            && !defect_closed(DefectState::WontFix, "")
            && defect_closed(DefectState::WontFix, "nic only") && release_blocked(1)
            && !release_blocked(0),
        "closure",
    );

    let rep = AcceptanceReport { machines: 2, features: 2, failed_features: 0, open_critical: 0 };
    set.add(
        "A934 report",
        rep.ship_ok() && rep.tag_len() == 4 + 1 + 1 + 1 + 1
            && !AcceptanceReport { open_critical: 1, ..rep }.ship_ok(),
        "ship",
    );

    set.add(
        "A935 selfcheck",
        acceptance_selfcheck(rep, &cells[..2], &["boot", "gfx"])
            && !acceptance_selfcheck(rep, &cells, &["boot", "gfx", "net"]),
        "numbers",
    );

    set.add(
        "A936 budget",
        acceptance_run_budget_ok(30) && !acceptance_run_budget_ok(31)
            && feature_rerun_budget_ok(120) && !feature_rerun_budget_ok(121),
        "ci window",
    );

    let mut log = AccEventLog::new();
    log.push(AccEvent { kind: AccEventKind::MatrixDone, stamp: 1, arg: 0 });
    log.push(AccEvent { kind: AccEventKind::ShipVerdict, stamp: 2, arg: 1 });
    set.add(
        "A937 events",
        log.len() == 2 && log.get(0).unwrap().kind == AccEventKind::ShipVerdict
            && log.count_of(AccEventKind::MatrixDone) == 1,
        "ring",
    );

    set.add(
        "A940 compat",
        acc_compat_ok(&[AccCompatCell { arch: "x86_64", accepted: true, baseline_recorded: true }])
            && !acc_compat_ok(&[AccCompatCell { arch: "x86_64", accepted: true, baseline_recorded: false }])
            && !acc_compat_ok(&[]),
        "matrix",
    );

    set.add(
        "A941 ci hook",
        ci_hook_advance(CiHook::Idle, true, false) == CiHook::Running
            && ci_hook_advance(CiHook::Idle, false, false) == CiHook::Idle
            && ci_hook_advance(CiHook::Running, true, true) == CiHook::Uploaded
            && ci_hook_advance(CiHook::Running, true, false) == CiHook::Failed
            && ci_hook_advance(CiHook::Failed, true, true) == CiHook::Idle,
        "state machine",
    );

    set.add(
        "A942 release gate",
        release_gate(rep, CiHook::Uploaded) && !release_gate(rep, CiHook::Running)
            && !release_gate(AcceptanceReport { failed_features: 1, ..rep }, CiHook::Uploaded),
        "gate",
    );

    set.add(
        "A943 consistent",
        acc_consistent(rep, &hw[..2]) && acc_consistent(AcceptanceReport { machines: 9, ..rep }, &hw),
        "cross",
    );

    let docs = [AccDoc { report_id: 1, baseline_rev: 42 }];
    set.add(
        "A948 docs",
        acc_doc_ok(&docs) && !acc_doc_ok(&[AccDoc { baseline_rev: 0, ..docs[0] }])
            && !acc_doc_ok(&[]),
        "baseline rev",
    );

    set.add(
        "A939 degrade",
        acc_degrade(true, false) == AccTier::Full && acc_degrade(false, true) == AccTier::Emulated
            && acc_degrade(false, false) == AccTier::Smoke,
        "chain",
    );

    set.add(
        "A945 budget floor",
        acceptance_run_budget_ok(0) && feature_rerun_budget_ok(0),
        "min",
    );

    set.add("A946 obs cap", log.len() <= ACC_EVENT_CAP, "bounded");

    set.add("A947 digits", digits(0) == 1 && digits(u16::MAX as u32) == 5, "math");

    set.add(
        "A949 smoke tier",
        acc_degrade(false, false) == AccTier::Smoke,
        "floor",
    );

    set.add(
        "A950 closure",
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
    fn a926_matrix_paths() {
        let none: [RegressionCell; 0] = [];
        assert!(!regression_matrix_green(&none, &["x"]));
        assert!(regression_matrix_green(&none, &[]));
        assert_eq!(missing_cells(&none, &["x", "y"]), 2);
    }

    #[test]
    fn a927_aggregate_failure() {
        let mut bad = CheckSet::new("bad");
        bad.fail("z", "nope");
        let k = aggregate_checksets(&[bad]);
        assert!(!k.all_passed());
        assert_eq!(k.tally(), (0, 1));
    }

    #[test]
    fn a929_baseline_boundary() {
        let exact = PerfBaseline { boot_ms: BASELINE_BOOT_MS, idle_mib: BASELINE_IDLE_MIB, fps_x10: BASELINE_FPS_X10 };
        assert!(exact.within_baseline());
        let over = PerfBaseline { boot_ms: BASELINE_BOOT_MS + 1, ..exact };
        assert!(!over.within_baseline());
        let under_fps = PerfBaseline { fps_x10: BASELINE_FPS_X10 - 1, ..exact };
        assert!(!under_fps.within_baseline());
    }

    #[test]
    fn a932_fps_edges() {
        let s = FpsSample { frames: 0, dropped: 0, window_ms: 0 };
        assert!(!s.acceptable());
        let d = FpsSample { frames: 1_000, dropped: 10, window_ms: 16_000 };
        assert_eq!(d.fps_x10(), 625);
        assert!(d.acceptable());
        let many_drops = FpsSample { frames: 1_000, dropped: 11, window_ms: 16_000 };
        assert!(!many_drops.acceptable());
    }

    #[test]
    fn a934_tag_len() {
        let r = AcceptanceReport { machines: 12, features: 345, failed_features: 6, open_critical: 0 };
        assert_eq!(r.tag_len(), 4 + 2 + 1 + 3 + 1 + 1);
    }

    #[test]
    fn a937_event_wrap() {
        let mut l = AccEventLog::new();
        for i in 0..ACC_EVENT_CAP + 4 {
            l.push(AccEvent { kind: AccEventKind::DefectFiled, stamp: i as u64, arg: i as u32 });
        }
        assert_eq!(l.len(), ACC_EVENT_CAP);
        assert_eq!(l.get(0).unwrap().stamp, (ACC_EVENT_CAP + 3) as u64);
        assert_eq!(l.count_of(AccEventKind::DefectFiled), ACC_EVENT_CAP);
    }

    #[test]
    fn a941_hook_lifecycle() {
        let mut s = CiHook::Idle;
        s = ci_hook_advance(s, true, false);
        assert_eq!(s, CiHook::Running);
        s = ci_hook_advance(s, false, false);
        assert_eq!(s, CiHook::Failed);
        s = ci_hook_advance(s, true, false);
        assert_eq!(s, CiHook::Idle);
        s = ci_hook_advance(s, true, false);
        s = ci_hook_advance(s, false, true);
        assert_eq!(s, CiHook::Uploaded);
    }

    #[test]
    fn a950_final() {
        let set = run_acceptance_checks();
        if !set.all_passed() {
            let mut buf = [0u8; 1024];
            let n = set.render(&mut buf);
            panic!("acceptance self-test:\n{}", core::str::from_utf8(&buf[..n]).unwrap());
        }
        assert!(set.len() >= 25);
    }
}
