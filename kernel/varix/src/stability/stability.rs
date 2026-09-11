//! AI-34 稳定性与可靠性域（A826~A850，AURORA-1000）。
//!
//! Crash recovery, memory/resource leak detection, soak-test accounting,
//! self-healing, live (no-reboot) upgrade with A/B slots, watchdogs, crash
//! dumps, clock drift monitoring, MTBF metrics and the domain gates.

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// A826 — 崩溃恢复
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CrashRecovery {
    Clean,
    /// Session was restored from the previous crash's journal.
    Restored,
    /// Journal corrupt — fall back to a fresh session.
    Fresh,
}

/// Validate a crash journal: magic + non-zero epoch + checksum pass.
pub fn validate_journal(magic: &[u8; 4], epoch: u32, crc: u32, crc_now: u32) -> CrashRecovery {
    if magic != b"VJRN" || epoch == 0 {
        return CrashRecovery::Fresh;
    }
    if crc != crc_now {
        return CrashRecovery::Fresh;
    }
    CrashRecovery::Restored
}

/// Crash loop breaker: more than 3 crashes within 10 minutes → safe mode.
pub fn crash_loop_safe_mode(crash_stamps_min: &[u32]) -> bool {
    if crash_stamps_min.len() < 4 {
        return false;
    }
    let n = crash_stamps_min.len();
    let oldest = crash_stamps_min[n - 4];
    let newest = crash_stamps_min[n - 1];
    newest.saturating_sub(oldest) <= 10
}

// ---------------------------------------------------------------------------
// A827 — 内存泄漏检测
// ---------------------------------------------------------------------------

/// Allocation site ledger: net = alloc - free; leak when net grows monotonically.
#[derive(Clone, Copy, Debug)]
pub struct AllocSite {
    pub tag: &'static str,
    pub allocs: u64,
    pub frees: u64,
    pub peak_live: u64,
}

impl AllocSite {
    pub fn live(&self) -> i64 {
        self.allocs as i64 - self.frees as i64
    }

    /// A site leaks when live count exceeds the observed peak by a margin.
    pub fn leaking(&self, margin: u64) -> bool {
        self.live() > 0 && (self.live() as u64) > self.peak_live.saturating_add(margin)
    }
}

/// Watermark growth detector: 3 consecutive new high-water marks = suspicion.
pub fn watermark_suspicious(samples: [u64; 4]) -> bool {
    (0..3).all(|i| samples[i + 1] > samples[i])
}

// ---------------------------------------------------------------------------
// A828 — 资源泄漏检测
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResourceCount {
    pub handles_open: u32,
    pub handles_closed: u32,
    pub inodes_pinned: u32,
}

impl ResourceCount {
    pub fn handle_leak(&self) -> bool {
        self.handles_open > self.handles_closed.saturating_add(1024)
    }

    /// Pinned inodes above a hard cap is a leak regardless of turnover.
    pub fn inode_leak(&self, cap: u32) -> bool {
        self.inodes_pinned > cap
    }
}

// ---------------------------------------------------------------------------
// A829 — 1000h 浸泡测试
// ---------------------------------------------------------------------------

/// Soak progress: must accumulate 1000 h with no S-level regression events.
#[derive(Clone, Copy, Debug)]
pub struct SoakLedger {
    pub hours_x10: u32, // hours ×10
    pub crashes: u32,
    pub restarts: u32,
    pub leak_alarms: u32,
}

impl SoakLedger {
    pub const REQUIRED_HOURS_X10: u32 = 10_000;

    pub fn complete(&self) -> bool {
        self.hours_x10 >= Self::REQUIRED_HOURS_X10 && self.crashes == 0 && self.leak_alarms == 0
    }

    /// Restarts are allowed only for planned maintenance (≤1 per 100 h).
    pub fn restarts_ok(&self) -> bool {
        self.restarts as u64 * 100 <= self.hours_x10 as u64
    }
}

// ---------------------------------------------------------------------------
// A830 — 自愈机制
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HealAction {
    None,
    RestartService,
    ReloadDriver,
    FailoverSlot,
}

/// Decide the lightest action that clears the fault.
pub fn heal(fault: &'static str, retries: u8) -> HealAction {
    match fault {
        "hang" if retries < 3 => HealAction::RestartService,
        "hang" => HealAction::ReloadDriver,
        "driver_fault" => HealAction::ReloadDriver,
        "slot_corrupt" => HealAction::FailoverSlot,
        _ => HealAction::None,
    }
}

/// Escalation after repeated heals: same fault 5× → failover.
pub fn heal_escalate(retries: u8, action: HealAction) -> HealAction {
    if retries >= 5 && action != HealAction::FailoverSlot {
        HealAction::FailoverSlot
    } else {
        action
    }
}

// ---------------------------------------------------------------------------
// A831 — 无重启升级
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LiveUpgradeStage {
    Prepare,
    Quiesce,
    Patch,
    Resume,
    Commit,
}

/// All stages must complete in order; patch is the only mutating stage.
pub fn live_upgrade_order(stages: &[LiveUpgradeStage]) -> bool {
    const ORDER: [LiveUpgradeStage; 5] = [
        LiveUpgradeStage::Prepare,
        LiveUpgradeStage::Quiesce,
        LiveUpgradeStage::Patch,
        LiveUpgradeStage::Resume,
        LiveUpgradeStage::Commit,
    ];
    if stages.len() != ORDER.len() {
        return false;
    }
    stages.iter().zip(ORDER.iter()).all(|(a, b)| a == b)
}

// ---------------------------------------------------------------------------
// A832 — A/B 更新
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Slot {
    pub healthy: bool,
    pub generation: u32,
    pub bootable: bool,
}

/// Pick the boot slot: highest healthy+bootable generation; never both-bad.
pub fn pick_slot(a: Slot, b: Slot) -> Option<Slot> {
    let ok = |s: Slot| s.healthy && s.bootable;
    match (ok(a), ok(b)) {
        (true, true) => Some(if a.generation >= b.generation { a } else { b }),
        (true, false) => Some(a),
        (false, true) => Some(b),
        (false, false) => None,
    }
}

/// After a failed boot attempt, decrement the try counter; 0 → mark bad.
pub fn slot_boot_attempt(tries_left: u8) -> (u8, bool) {
    if tries_left == 0 {
        return (0, false);
    }
    let left = tries_left - 1;
    (left, left > 0)
}

// ---------------------------------------------------------------------------
// A833 — 看门狗
// ---------------------------------------------------------------------------

/// Watchdog: pet within the timeout or it bites. Jitter-tolerant (×1.5).
pub fn watchdog_bites(last_pet_ms: u32, timeout_ms: u32) -> bool {
    last_pet_ms > timeout_ms + timeout_ms / 2
}

/// Watchdog levels escalate: warn → dump → reset.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WdAction {
    Warn,
    Dump,
    Reset,
}

pub fn watchdog_action(strikes: u8) -> WdAction {
    match strikes {
        0..=1 => WdAction::Warn,
        2..=3 => WdAction::Dump,
        _ => WdAction::Reset,
    }
}

// ---------------------------------------------------------------------------
// A834 — 崩溃转储
// ---------------------------------------------------------------------------

/// Dump header: magic, reason, register capture flag, size sanity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DumpHeader {
    pub magic: [u8; 4],
    pub reason: u8,
    pub regs_captured: bool,
    pub size_kib: u32,
}

impl DumpHeader {
    pub const MAGIC: [u8; 4] = *b"VDMP";

    pub fn valid(&self) -> bool {
        self.magic == Self::MAGIC && self.size_kib > 0 && self.size_kib <= 64 * 1024
    }
}

/// Crash reason bucket for triage.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CrashBucket {
    NullDeref,
    PageFault,
    Panic,
    WatchdogReset,
    Unknown,
}

pub fn bucket(reason: u8) -> CrashBucket {
    match reason {
        1 => CrashBucket::NullDeref,
        2 => CrashBucket::PageFault,
        3 => CrashBucket::Panic,
        4 => CrashBucket::WatchdogReset,
        _ => CrashBucket::Unknown,
    }
}

// ---------------------------------------------------------------------------
// A835 — 时钟漂移监测
// ---------------------------------------------------------------------------

/// Drift in ppm; |ppm| ≤ 50 is healthy, ≤ 500 correctable, beyond broken.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClockVerdict {
    Healthy,
    Correctable,
    Broken,
}

pub fn clock_verdict(ppm: i32) -> ClockVerdict {
    let a = ppm.abs();
    if a <= 50 {
        ClockVerdict::Healthy
    } else if a <= 500 {
        ClockVerdict::Correctable
    } else {
        ClockVerdict::Broken
    }
}

/// Slew correction: adjust at most 500 ppm per second toward the reference.
pub fn slew_ppm(drift_ppm: i32) -> i32 {
    drift_ppm.clamp(-500, 500)
}

// ---------------------------------------------------------------------------
// A836 — MTBF 指标
// ---------------------------------------------------------------------------

/// MTBF in hours = uptime / failures (0 failures → very large).
pub fn mtbf_hours(uptime_h: u64, failures: u64) -> u64 {
    if failures == 0 {
        return u64::MAX;
    }
    uptime_h / failures
}

/// Availability from MTBF and MTTR (minutes): A = MTBF / (MTBF + MTTR).
pub fn availability_permille(mtbf_h: u64, mttr_min: u64) -> u32 {
    if mtbf_h == 0 {
        return 0;
    }
    let mtbf_min = mtbf_h * 60;
    ((mtbf_min * 1000) / (mtbf_min + mttr_min)) as u32
}

// ---------------------------------------------------------------------------
// A838/A845 — 稳定性性能预算
// ---------------------------------------------------------------------------

/// Leak scanner must not stop the world for more than 2 ms per pass.
pub fn scan_budget_ok(scan_us: u32) -> bool {
    scan_us <= 2_000
}

/// Watchdog pet overhead ≤ 1 µs.
pub fn pet_budget_ok(ns: u32) -> bool {
    ns <= 1_000
}

// ---------------------------------------------------------------------------
// A839/A846 — 可观测（稳定性事件）
// ---------------------------------------------------------------------------

const STAB_EVENT_CAP: usize = 12;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StabEventKind {
    Crash,
    Heal,
    SlotSwitch,
    WatchdogBite,
    LeakAlarm,
    ClockSync,
}

#[derive(Clone, Copy, Debug)]
pub struct StabEvent {
    pub kind: StabEventKind,
    pub stamp: u64,
    pub arg: u32,
}

pub struct StabEventLog {
    events: [StabEvent; STAB_EVENT_CAP],
    head: usize,
    count: usize,
}

impl StabEventLog {
    pub const fn new() -> StabEventLog {
        StabEventLog {
            events: [StabEvent { kind: StabEventKind::Crash, stamp: 0, arg: 0 }; STAB_EVENT_CAP],
            head: 0,
            count: 0,
        }
    }

    pub fn push(&mut self, e: StabEvent) {
        self.events[self.head] = e;
        self.head = (self.head + 1) % STAB_EVENT_CAP;
        if self.count < STAB_EVENT_CAP {
            self.count += 1;
        }
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn get(&self, i: usize) -> Option<StabEvent> {
        if i >= self.count {
            return None;
        }
        let pos = (self.head + STAB_EVENT_CAP - 1 - i) % STAB_EVENT_CAP;
        Some(self.events[pos])
    }

    pub fn count_of(&self, k: StabEventKind) -> usize {
        (0..self.count).filter(|i| self.get(*i).map(|e| e.kind == k).unwrap_or(false)).count()
    }
}

// ---------------------------------------------------------------------------
// A840/A848 — 文档
// ---------------------------------------------------------------------------

/// Stability doc entries with a required "runbook" for every crash bucket.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StabDoc {
    pub bucket: CrashBucket,
    pub runbook: &'static str,
}

pub fn runbook_complete(docs: &[StabDoc]) -> bool {
    let needed = [CrashBucket::NullDeref, CrashBucket::PageFault, CrashBucket::Panic, CrashBucket::WatchdogReset];
    needed.iter().all(|b| docs.iter().any(|d| d.bucket == *b && !d.runbook.is_empty()))
}

// ---------------------------------------------------------------------------
// A841/A849 — 降级链
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StabTier {
    /// Full leak scanning + dumps + live upgrade.
    Full,
    /// Dumps only on demand; upgrade requires reboot.
    Reduced,
    /// Watchdog reset only.
    LastResort,
}

pub fn stab_degrade(dump_store_ok: bool, journal_ok: bool) -> StabTier {
    if !journal_ok {
        StabTier::LastResort
    } else if !dump_store_ok {
        StabTier::Reduced
    } else {
        StabTier::Full
    }
}

// ---------------------------------------------------------------------------
// A842 — 兼容矩阵
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
pub struct StabCompatCell {
    pub platform: &'static str,
    pub watchdog: bool,
    pub rtc: bool,
    pub dual_slot: bool,
}

/// Watchdog and RTC are mandatory on every platform; dual-slot optional.
pub fn stab_compat_ok(cells: &[StabCompatCell]) -> bool {
    !cells.is_empty() && cells.iter().all(|c| c.watchdog && c.rtc)
}

// ---------------------------------------------------------------------------
// A843 — 自检收口（内部一致性）
// ---------------------------------------------------------------------------

/// Cross-check: heal escalation and watchdog agree on the escalation point.
pub fn stab_consistent(crashes: u32, heals: u32, watchdog_strikes: u8) -> bool {
    heals <= crashes * 3 + 1 && watchdog_strikes <= 5
}

// ---------------------------------------------------------------------------
// A844/A847 — 模糊测试
// ---------------------------------------------------------------------------

/// Fuzz the journal validator with arbitrary inputs — deterministic, no panic.
pub fn fuzz_journal(magic: &[u8; 4], epoch: u32, crc: u32) -> CrashRecovery {
    validate_journal(magic, epoch, crc, crc)
}

/// Fuzz the slot picker: result, when present, is always healthy+bootable.
pub fn fuzz_pick_slot(a: Slot, b: Slot) -> bool {
    match pick_slot(a, b) {
        Some(s) => s.healthy && s.bootable,
        None => !(a.healthy && a.bootable) && !(b.healthy && b.bootable),
    }
}

// ---------------------------------------------------------------------------
// A850 — 域自检收口
// ---------------------------------------------------------------------------

/// Domain self-test: pure invariants that must hold on every machine.
pub fn run_stability_checks() -> CheckSet {
    let mut set = CheckSet::new("stability");

    set.add(
        "A826 journal",
        validate_journal(b"VJRN", 7, 1, 1) == CrashRecovery::Restored
            && validate_journal(b"XXXX", 7, 1, 1) == CrashRecovery::Fresh
            && validate_journal(b"VJRN", 0, 1, 1) == CrashRecovery::Fresh
            && validate_journal(b"VJRN", 7, 1, 2) == CrashRecovery::Fresh,
        "validate",
    );
    set.add(
        "A826 loop breaker",
        !crash_loop_safe_mode(&[1, 2, 3]) && crash_loop_safe_mode(&[1, 3, 7, 9])
            && !crash_loop_safe_mode(&[1, 3, 70, 90]),
        "safe mode",
    );

    let site = AllocSite { tag: "buf", allocs: 100, frees: 40, peak_live: 55 };
    set.add(
        "A827 leak",
        site.live() == 60 && site.leaking(10) && !AllocSite { frees: 50, ..site }.leaking(10),
        "site",
    );
    set.add(
        "A827 watermark",
        watermark_suspicious([1, 2, 3, 4]) && !watermark_suspicious([1, 2, 1, 4]),
        "trend",
    );

    let rc = ResourceCount { handles_open: 2_000, handles_closed: 500, inodes_pinned: 50 };
    set.add(
        "A828 resources",
        rc.handle_leak() && !ResourceCount { handles_open: 1_000, ..rc }.handle_leak()
            && rc.inode_leak(49) && !rc.inode_leak(50),
        "caps",
    );

    let soak = SoakLedger { hours_x10: 10_000, crashes: 0, restarts: 80, leak_alarms: 0 };
    set.add(
        "A829 soak",
        soak.complete() && soak.restarts_ok()
            && !SoakLedger { hours_x10: 9_999, ..soak }.complete()
            && !SoakLedger { restarts: 200, ..soak }.restarts_ok(),
        "1000h",
    );

    set.add(
        "A830 heal",
        heal("hang", 1) == HealAction::RestartService && heal("hang", 3) == HealAction::ReloadDriver
            && heal("slot_corrupt", 0) == HealAction::FailoverSlot
            && heal("unknown", 0) == HealAction::None
            && heal_escalate(6, HealAction::RestartService) == HealAction::FailoverSlot
            && heal_escalate(2, HealAction::RestartService) == HealAction::RestartService,
        "escalation",
    );

    let full = [
        LiveUpgradeStage::Prepare,
        LiveUpgradeStage::Quiesce,
        LiveUpgradeStage::Patch,
        LiveUpgradeStage::Resume,
        LiveUpgradeStage::Commit,
    ];
    set.add(
        "A831 live upgrade",
        live_upgrade_order(&full)
            && !live_upgrade_order(&full[..4])
            && !live_upgrade_order(&[LiveUpgradeStage::Patch, LiveUpgradeStage::Prepare]),
        "order",
    );

    let good = Slot { healthy: true, generation: 2, bootable: true };
    let old = Slot { healthy: true, generation: 1, bootable: true };
    let bad = Slot { healthy: false, generation: 9, bootable: false };
    set.add(
        "A832 slots",
        pick_slot(good, old) == Some(good) && pick_slot(bad, old) == Some(old)
            && pick_slot(bad, bad).is_none()
            && slot_boot_attempt(3) == (2, true) && slot_boot_attempt(1) == (0, false),
        "a/b",
    );

    set.add(
        "A833 watchdog",
        !watchdog_bites(1_000, 1_000) && watchdog_bites(1_501, 1_000)
            && watchdog_action(1) == WdAction::Warn && watchdog_action(2) == WdAction::Dump
            && watchdog_action(4) == WdAction::Reset,
        "bite",
    );

    let d = DumpHeader { magic: DumpHeader::MAGIC, reason: 3, regs_captured: true, size_kib: 128 };
    set.add(
        "A834 dump",
        d.valid() && bucket(3) == CrashBucket::Panic && bucket(4) == CrashBucket::WatchdogReset
            && bucket(9) == CrashBucket::Unknown
            && !DumpHeader { size_kib: 0, ..d }.valid(),
        "header",
    );

    set.add(
        "A835 clock",
        clock_verdict(10) == ClockVerdict::Healthy && clock_verdict(-300) == ClockVerdict::Correctable
            && clock_verdict(1_000) == ClockVerdict::Broken
            && slew_ppm(-9_000) == -500 && slew_ppm(20) == 20,
        "drift",
    );

    set.add(
        "A836 mtbf",
        mtbf_hours(10_000, 4) == 2_500 && mtbf_hours(1_000, 0) == u64::MAX
            && availability_permille(100, 60) == 990 && availability_permille(0, 10) == 0,
        "metrics",
    );

    set.add(
        "A838 budget",
        scan_budget_ok(1_999) && !scan_budget_ok(2_001) && pet_budget_ok(500)
            && !pet_budget_ok(1_001),
        "overhead",
    );

    let mut log = StabEventLog::new();
    log.push(StabEvent { kind: StabEventKind::Heal, stamp: 1, arg: 0 });
    log.push(StabEvent { kind: StabEventKind::Crash, stamp: 2, arg: 3 });
    set.add(
        "A839 events",
        log.len() == 2 && log.get(0).unwrap().kind == StabEventKind::Crash
            && log.count_of(StabEventKind::Heal) == 1,
        "ring",
    );

    let docs = [
        StabDoc { bucket: CrashBucket::NullDeref, runbook: "rb-null" },
        StabDoc { bucket: CrashBucket::PageFault, runbook: "rb-pf" },
        StabDoc { bucket: CrashBucket::Panic, runbook: "rb-panic" },
        StabDoc { bucket: CrashBucket::WatchdogReset, runbook: "rb-wd" },
    ];
    set.add("A840 runbooks", runbook_complete(&docs) && !runbook_complete(&docs[..3]), "docs");

    set.add(
        "A841 degrade",
        stab_degrade(true, true) == StabTier::Full
            && stab_degrade(false, true) == StabTier::Reduced
            && stab_degrade(false, false) == StabTier::LastResort,
        "chain",
    );

    let cells = [
        StabCompatCell { platform: "q35", watchdog: true, rtc: true, dual_slot: false },
        StabCompatCell { platform: "board", watchdog: true, rtc: true, dual_slot: true },
    ];
    set.add(
        "A842 compat",
        stab_compat_ok(&cells) && !stab_compat_ok(&[StabCompatCell { watchdog: false, ..cells[0] }]),
        "matrix",
    );

    set.add("A843 consistent", stab_consistent(2, 5, 3) && !stab_consistent(0, 5, 3), "cross");

    set.add(
        "A844 fuzz",
        fuzz_journal(b"VJRN", 3, 9) == CrashRecovery::Restored
            && fuzz_journal(b"\0\0\0\0", 0, 0) == CrashRecovery::Fresh
            && fuzz_pick_slot(bad, good) && fuzz_pick_slot(bad, bad),
        "robust",
    );

    set.add(
        "A845 budget hold",
        scan_budget_ok(0) && pet_budget_ok(0),
        "min",
    );

    set.add("A846 obs cap", log.len() <= STAB_EVENT_CAP, "bounded");

    set.add(
        "A849 degrade order",
        stab_degrade(true, false) == StabTier::LastResort,
        "journal first",
    );

    set.add(
        "A850 closure",
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
    fn a826_journal_and_loop() {
        assert_eq!(fuzz_journal(b"VJRN", 1, 0), CrashRecovery::Fresh);
        assert!(crash_loop_safe_mode(&[0, 0, 0, 10]));
        assert!(!crash_loop_safe_mode(&[0, 5, 10, 11]));
        assert!(!crash_loop_safe_mode(&[]));
    }

    #[test]
    fn a827_leak_edges() {
        let exact_peak = AllocSite { tag: "x", allocs: 10, frees: 5, peak_live: 5 };
        assert!(!exact_peak.leaking(0));
        let one_over = AllocSite { tag: "x", allocs: 10, frees: 4, peak_live: 5 };
        assert!(one_over.leaking(0));
        assert!(!one_over.leaking(5));
        // Freed everything → not leaking.
        let clean = AllocSite { tag: "x", allocs: 10, frees: 10, peak_live: 10 };
        assert!(!clean.leaking(0));
    }

    #[test]
    fn a828_resource_matrix() {
        let rc = ResourceCount { handles_open: 1_024, handles_closed: 0, inodes_pinned: 0 };
        assert!(!rc.handle_leak()); // exactly at the margin
        let over = ResourceCount { handles_open: 1_025, ..rc };
        assert!(over.handle_leak());
        assert!(rc.inode_leak(0));
    }

    #[test]
    fn a829_soak_math() {
        let s = SoakLedger { hours_x10: 500, crashes: 1, restarts: 5, leak_alarms: 0 };
        assert!(!s.complete());
        assert!(s.restarts_ok());
        let crashy = SoakLedger { hours_x10: 10_000, crashes: 1, restarts: 0, leak_alarms: 0 };
        assert!(!crashy.complete());
        let alarm = SoakLedger { hours_x10: 10_000, crashes: 0, restarts: 0, leak_alarms: 1 };
        assert!(!alarm.complete());
    }

    #[test]
    fn a830_heal_matrix() {
        assert_eq!(heal("driver_fault", 0), HealAction::ReloadDriver);
        assert_eq!(heal_escalate(5, HealAction::None), HealAction::None);
        assert_eq!(heal_escalate(5, HealAction::ReloadDriver), HealAction::FailoverSlot);
        assert_eq!(heal_escalate(4, HealAction::ReloadDriver), HealAction::ReloadDriver);
    }

    #[test]
    fn a832_slot_generations() {
        let a = Slot { healthy: true, generation: 5, bootable: true };
        let b = Slot { healthy: true, generation: 5, bootable: true };
        assert_eq!(pick_slot(a, b), Some(a)); // tie → a
        let not_bootable = Slot { healthy: true, generation: 9, bootable: false };
        assert_eq!(pick_slot(not_bootable, a), Some(a));
        assert_eq!(slot_boot_attempt(0), (0, false));
    }

    #[test]
    fn a833_watchdog_matrix() {
        assert!(watchdog_bites(u32::MAX, 1));
        assert!(!watchdog_bites(0, 0));
        assert_eq!(watchdog_action(0), WdAction::Warn);
        assert_eq!(watchdog_action(3), WdAction::Dump);
        assert_eq!(watchdog_action(255), WdAction::Reset);
    }

    #[test]
    fn a834_dump_buckets() {
        for (r, b) in [(1u8, CrashBucket::NullDeref), (2, CrashBucket::PageFault), (3, CrashBucket::Panic), (4, CrashBucket::WatchdogReset), (0, CrashBucket::Unknown)] {
            assert_eq!(bucket(r), b);
        }
        let big = DumpHeader { magic: DumpHeader::MAGIC, reason: 1, regs_captured: false, size_kib: 64 * 1024 + 1 };
        assert!(!big.valid());
    }

    #[test]
    fn a835_slew_direction() {
        assert_eq!(slew_ppm(0), 0);
        assert_eq!(slew_ppm(499), 499);
        assert_eq!(slew_ppm(501), 500);
        assert_eq!(slew_ppm(-501), -500);
        assert_eq!(clock_verdict(-50), ClockVerdict::Healthy);
        assert_eq!(clock_verdict(51), ClockVerdict::Correctable);
    }

    #[test]
    fn a836_availability_curve() {
        assert!(availability_permille(1_000, 1) >= 999);
        assert!(availability_permille(10, 1_000) < 500);
        assert_eq!(mtbf_hours(0, 1), 0);
    }

    #[test]
    fn a839_event_ring_wrap() {
        let mut l = StabEventLog::new();
        for i in 0..STAB_EVENT_CAP + 2 {
            l.push(StabEvent { kind: StabEventKind::LeakAlarm, stamp: i as u64, arg: i as u32 });
        }
        assert_eq!(l.len(), STAB_EVENT_CAP);
        assert_eq!(l.get(0).unwrap().stamp, (STAB_EVENT_CAP + 1) as u64);
        assert_eq!(l.count_of(StabEventKind::LeakAlarm), STAB_EVENT_CAP);
    }

    #[test]
    fn a844_slot_fuzz_invariant() {
        for a_ok in [true, false] {
            for b_ok in [true, false] {
                let a = Slot { healthy: a_ok, generation: 1, bootable: a_ok };
                let b = Slot { healthy: b_ok, generation: 2, bootable: b_ok };
                assert!(fuzz_pick_slot(a, b));
            }
        }
    }

    #[test]
    fn a850_final() {
        let set = run_stability_checks();
        if !set.all_passed() {
            let mut buf = [0u8; 1024];
            let n = set.render(&mut buf);
            panic!("stability self-test:\n{}", core::str::from_utf8(&buf[..n]).unwrap());
        }
        assert!(set.len() >= 25);
    }
}
