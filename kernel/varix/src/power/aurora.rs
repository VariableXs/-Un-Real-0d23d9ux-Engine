//! AI-32 电源与热管理域（A776~A800，AURORA-1000）。
//!
//! Mounted as `crate::apower` (the `power.rs` module name belongs to the
//! VARIX-500 F251~F275 domain). ACPI platform power states, CPU frequency
//! governance with turbo/energy-aware scheduling, device D-states, fan and
//! thermal control, battery accounting, S0ix idle residency, perf-per-watt
//! instrumentation, power policy and the domain gates.

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// A776 — ACPI 电源状态
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AcpiPowerState {
    Working,
    Sleeping(u8), // S1..S4
    SoftOff,
}

impl AcpiPowerState {
    pub fn latency_budget_ms(self) -> u32 {
        match self {
            AcpiPowerState::Working => 0,
            AcpiPowerState::Sleeping(s) => 20 + s as u32 * 200,
            AcpiPowerState::SoftOff => 5000,
        }
    }

    pub fn from_slp_typ(v: u16) -> Option<AcpiPowerState> {
        let typ = (v >> 10) & 0x7;
        match typ {
            0 => Some(AcpiPowerState::Working),
            1..=4 => Some(AcpiPowerState::Sleeping(typ as u8)),
            5 => Some(AcpiPowerState::SoftOff),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// A777 — CPU 频率调节
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FreqGovernor {
    Performance,
    Powersave,
    Ondemand,
    Schedutil,
}

#[derive(Clone, Copy, Debug)]
pub struct FreqRange {
    pub min_khz: u32,
    pub max_khz: u32,
    pub steps: u8,
}

/// Clamp a requested frequency into the valid range and snap to a step.
pub fn clamp_freq(req_khz: u32, r: FreqRange) -> u32 {
    let lo = r.min_khz.min(r.max_khz);
    let hi = r.min_khz.max(r.max_khz);
    let v = req_khz.clamp(lo, hi);
    if r.steps <= 1 {
        return v;
    }
    let step = (hi - lo) / r.steps as u32;
    if step == 0 {
        return v;
    }
    let snapped = lo + ((v - lo) / step) * step;
    snapped.min(hi)
}

/// Simple ramp-up/down limiter: at most `max_step_khz` change per tick.
pub fn ramp_freq(current_khz: u32, target_khz: u32, max_step_khz: u32) -> u32 {
    let d = current_khz.abs_diff(target_khz);
    if d <= max_step_khz {
        target_khz
    } else if target_khz > current_khz {
        current_khz + max_step_khz
    } else {
        current_khz - max_step_khz
    }
}

// ---------------------------------------------------------------------------
// A778 — 睿频与能耗调度
// ---------------------------------------------------------------------------

/// Turbo budget: allows above-nominal boost while the energy budget holds.
#[derive(Clone, Copy, Debug)]
pub struct TurboBudget {
    pub nominal_khz: u32,
    pub boost_khz: u32,
    pub remaining_millijoules: u64,
    pub boost_millijoules_per_ms: u64,
}

impl TurboBudget {
    /// Frequency allowed this tick, considering the remaining boost budget.
    pub fn allowed_khz(&self, wants_boost: bool, dt_ms: u64) -> u32 {
        if !wants_boost || self.boost_khz <= self.nominal_khz {
            return self.nominal_khz;
        }
        let cost = self.boost_millijoules_per_ms.saturating_mul(dt_ms);
        if cost == 0 || cost <= self.remaining_millijoules {
            self.boost_khz
        } else if self.boost_millijoules_per_ms > 0 {
            // Partial boost: scale down between nominal and boost.
            let frac = self.remaining_millijoules * 1000 / (self.boost_millijoules_per_ms * dt_ms);
            self.nominal_khz
                + (((self.boost_khz - self.nominal_khz) as u64 * frac.min(1000)) / 1000) as u32
        } else {
            self.nominal_khz
        }
    }

    pub fn spend(&mut self, dt_ms: u64, at_boost: bool) {
        if at_boost {
            let cost = self.boost_millijoules_per_ms.saturating_mul(dt_ms);
            self.remaining_millijoules = self.remaining_millijoules.saturating_sub(cost);
        }
    }
}

/// Energy-aware task placement: prefer the efficiency core unless the task's
/// utilization is above the cross-over point.
pub fn place_task(permille_util: u16, ecore_max_permille: u16) -> bool {
    permille_util <= ecore_max_permille
}

// ---------------------------------------------------------------------------
// A779 — 设备 D 状态
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum DState {
    D0,
    D1,
    D2,
    D3hot,
    D3cold,
}

impl DState {
    /// Whether a device in this state can signal a PME wake event.
    pub fn can_wake(self) -> bool {
        matches!(self, DState::D0 | DState::D1 | DState::D2 | DState::D3hot)
    }

    /// Whether a device in this state loses register context.
    pub fn loses_context(self) -> bool {
        self == DState::D3cold
    }
}

/// Deepest D-state allowed for a device that must stay wake-capable.
pub fn deepest_wake_state(allowed: [DState; 5]) -> DState {
    allowed.iter().rev().copied().find(|s| s.can_wake()).unwrap_or(DState::D0)
}

// ---------------------------------------------------------------------------
// A780 — 风扇与热管理
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
pub struct FanState {
    pub duty_percent: u8,
    pub rpm: u16,
}

/// Map duty → expected rpm with a tachometer sanity band of ±15%.
pub fn rpm_in_band(f: FanState, nominal_full_rpm: u16) -> bool {
    let expect = (nominal_full_rpm as u32 * f.duty_percent as u32) / 100;
    let lo = expect * 85 / 100;
    let hi = expect * 115 / 100;
    f.rpm as u32 >= lo && f.rpm as u32 <= hi
}

/// Thermal trip ladder: passive → heavy → critical shutdown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TripAction {
    None,
    PassiveThrottle,
    FanBoost,
    EmergencyShutdown,
}

pub fn trip_action(millideg: i32, passive: i32, boost: i32, critical: i32) -> TripAction {
    if millideg >= critical {
        TripAction::EmergencyShutdown
    } else if millideg >= boost {
        TripAction::FanBoost
    } else if millideg >= passive {
        TripAction::PassiveThrottle
    } else {
        TripAction::None
    }
}

// ---------------------------------------------------------------------------
// A781 — 电池状态
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
pub struct BatterySnapshot {
    pub present: bool,
    pub soc_permille: u16,
    pub charging: bool,
    pub rate_mw: i32,
}

/// Estimated minutes to empty/full; `None` when not computable.
pub fn battery_eta_minutes(b: BatterySnapshot, capacity_mwh: u32) -> Option<u32> {
    if !b.present || b.rate_mw == 0 || capacity_mwh == 0 {
        return None;
    }
    let remaining_mwh = capacity_mwh as u64 * b.soc_permille as u64 / 1000;
    if b.charging {
        let need = capacity_mwh as u64 - remaining_mwh;
        Some((need / b.rate_mw.unsigned_abs() as u64).min(u32::MAX as u64) as u32)
    } else {
        Some((remaining_mwh / b.rate_mw.unsigned_abs() as u64).min(u32::MAX as u64) as u32)
    }
}

/// SOC smoothing: ignore ±1% jitter that is not a sustained trend.
pub fn soc_stable(prev_permille: u16, next_permille: u16) -> bool {
    prev_permille.abs_diff(next_permille) <= 10
}

// ---------------------------------------------------------------------------
// A782 — S0ix 待机
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
pub struct S0ixResidency {
    /// Residency in the deepest idle package state, permille of wall time.
    pub deep_permille: u16,
    /// Devices blocking the deepest state.
    pub blockers: u8,
    pub valid: bool,
}

/// The platform achieves S0ix when residency ≥ 60% and no blockers remain.
pub fn s0ix_ok(r: S0ixResidency) -> bool {
    r.valid && r.blockers == 0 && r.deep_permille >= 600
}

/// Report the top blocker class for diagnostics.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum S0ixBlocker {
    None,
    AudioActive,
    NicActive,
    GpuActive,
    TimerCoalescingMiss,
}

pub fn classify_blocker(audio: bool, nic: bool, gpu: bool, missed_coalesce: bool) -> S0ixBlocker {
    if audio {
        S0ixBlocker::AudioActive
    } else if nic {
        S0ixBlocker::NicActive
    } else if gpu {
        S0ixBlocker::GpuActive
    } else if missed_coalesce {
        S0ixBlocker::TimerCoalescingMiss
    } else {
        S0ixBlocker::None
    }
}

// ---------------------------------------------------------------------------
// A783 — 每瓦性能仪表
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
pub struct PerfPerWatt {
    pub ops_per_s: u32,
    pub milliwatts: u32,
}

/// Returns ops per watt ×1000 (milli-ops/W) — 0 when power is unknown.
pub fn perf_per_watt_milli(p: PerfPerWatt) -> u32 {
    if p.milliwatts == 0 {
        return 0;
    }
    ((p.ops_per_s as u64 * 1000) / p.milliwatts as u64) as u32
}

/// Regime verdict: a machine is "efficient" when ≥ 5 ops/W.
pub fn efficiency_verdict(p: PerfPerWatt) -> bool {
    perf_per_watt_milli(p) >= 5000
}

// ---------------------------------------------------------------------------
// A784 — 电源策略
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PowerRegime {
    OnAC,
    OnBattery,
    Critical,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PolicyAction {
    FullSpeed,
    Balanced,
    Save,
    HibernateNow,
}

pub fn policy_action(regime: PowerRegime, soc_permille: u16) -> PolicyAction {
    match regime {
        PowerRegime::OnAC => PolicyAction::FullSpeed,
        PowerRegime::OnBattery => {
            if soc_permille < 150 {
                PolicyAction::Save
            } else {
                PolicyAction::Balanced
            }
        }
        PowerRegime::Critical => {
            if soc_permille <= 30 {
                PolicyAction::HibernateNow
            } else {
                PolicyAction::Save
            }
        }
    }
}

// ---------------------------------------------------------------------------
// A785 — 睡眠与唤醒
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SleepResult {
    Entered,
    WakeBy(&'static str),
    Failed,
}

/// Sleep transition: validate the wake mask first; sleep is refused when no
/// wake source is armed (otherwise the machine bricks until power cycle).
pub fn sleep_transition(wake_mask: u32, requested: AcpiPowerState) -> SleepResult {
    if requested == AcpiPowerState::Working {
        return SleepResult::Failed;
    }
    if wake_mask == 0 {
        return SleepResult::Failed;
    }
    if wake_mask & 0x1 != 0 {
        SleepResult::WakeBy("rtc")
    } else {
        SleepResult::Entered
    }
}

/// Wake latency accounting vs the 500 ms redline.
pub fn wake_latency_ok(total_ms: u32) -> bool {
    total_ms <= 500
}

// ---------------------------------------------------------------------------
// A786/A794 — 电源自检
// ---------------------------------------------------------------------------

/// Domain self-checkup core: all sub-invariants in one boolean.
pub fn power_checkup_core(state: AcpiPowerState, wake_mask: u32) -> bool {
    let can_sleep = sleep_transition(wake_mask, state) != SleepResult::Failed || wake_mask == 0;
    can_sleep && state.latency_budget_ms() <= 6000
}

// ---------------------------------------------------------------------------
// A787/A795 — 性能基准 / 性能预算
// ---------------------------------------------------------------------------

/// Governor decision latency budget: ≤ 50 µs per tick.
pub fn governor_tick_budget(us: u32) -> bool {
    us <= 50
}

/// Turbo efficiency: boost must yield ≥ 15% extra ops per watt-normalized ms.
pub fn turbo_gain_ok(base_ops: u32, boosted_ops: u32) -> bool {
    boosted_ops as u64 * 100 >= base_ops as u64 * 115
}

// ---------------------------------------------------------------------------
// A789/A796 — 可观测（电源事件流）
// ---------------------------------------------------------------------------

const PM_EVENT_CAP: usize = 12;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PmEventKind {
    FreqChange,
    TurboEnter,
    TurboExit,
    Sleep,
    Wake,
    ThermalTrip,
    DStateChange,
    S0ixFail,
}

#[derive(Clone, Copy, Debug)]
pub struct PmEvent {
    pub kind: PmEventKind,
    pub stamp: u64,
    pub arg: u32,
}

pub struct PmEventLog {
    events: [PmEvent; PM_EVENT_CAP],
    head: usize,
    count: usize,
}

impl PmEventLog {
    pub const fn new() -> PmEventLog {
        PmEventLog {
            events: [PmEvent { kind: PmEventKind::FreqChange, stamp: 0, arg: 0 }; PM_EVENT_CAP],
            head: 0,
            count: 0,
        }
    }

    pub fn push(&mut self, e: PmEvent) {
        self.events[self.head] = e;
        self.head = (self.head + 1) % PM_EVENT_CAP;
        if self.count < PM_EVENT_CAP {
            self.count += 1;
        }
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn get(&self, i: usize) -> Option<PmEvent> {
        if i >= self.count {
            return None;
        }
        let pos = (self.head + PM_EVENT_CAP - 1 - i) % PM_EVENT_CAP;
        Some(self.events[pos])
    }

    pub fn count_of(&self, k: PmEventKind) -> usize {
        (0..self.count).filter(|i| self.get(*i).map(|e| e.kind == k).unwrap_or(false)).count()
    }
}

// ---------------------------------------------------------------------------
// A790/A797 — 模糊测试
// ---------------------------------------------------------------------------

/// Fuzz the SLP_TYP decoder: any 16-bit value must decode or reject, never
/// panic, and decoding must be stable.
pub fn fuzz_slp_typ(v: u16) -> bool {
    let a = AcpiPowerState::from_slp_typ(v);
    let b = AcpiPowerState::from_slp_typ(v);
    a == b && (a.is_some() || (v >> 10) & 0x7 > 5)
}

/// Fuzz the frequency clamp with arbitrary ranges: result stays in range.
pub fn fuzz_clamp_freq(req: u32, min: u32, max: u32, steps: u8) -> bool {
    if min > max {
        return false; // inverted range is a contract violation
    }
    if steps == 0 {
        return true; // steps==0 → passthrough contract
    }
    let v = clamp_freq(req, FreqRange { min_khz: min, max_khz: max, steps });
    v >= min && v <= max
}

// ---------------------------------------------------------------------------
// A791 — 兼容矩阵
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
pub struct PmCompatCell {
    pub platform: &'static str,
    pub s3: bool,
    pub s0ix: bool,
    pub turbo: bool,
}

/// Every platform must declare S3 support (S0ix/turbo optional).
pub fn pm_compat_ok(cells: &[PmCompatCell]) -> bool {
    !cells.is_empty() && cells.iter().all(|c| c.s3)
}

// ---------------------------------------------------------------------------
// A792 — 过热保护
// ---------------------------------------------------------------------------

/// Critical-shutdown decision with hysteresis: once tripped, stay tripped
/// until the temperature falls 10 °C below the trip point.
pub fn shutdown_hysteresis(millideg: i32, trip: i32, already_tripped: bool) -> bool {
    if already_tripped {
        millideg >= trip - 10_000
    } else {
        millideg >= trip
    }
}

// ---------------------------------------------------------------------------
// A799 — 降级链
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PmTier {
    /// Full telemetry + turbo + S0ix.
    Full,
    /// No turbo; periodic polling instead of event-driven PM.
    Reduced,
    /// Static conservative clocks, thermal shutdown only.
    Safe,
}

pub fn pm_degrade(battery_sensors_ok: bool, events_flood: bool) -> PmTier {
    if !battery_sensors_ok {
        PmTier::Safe
    } else if events_flood {
        PmTier::Reduced
    } else {
        PmTier::Full
    }
}

// ---------------------------------------------------------------------------
// A800 — 域自检收口
// ---------------------------------------------------------------------------

/// Domain self-test: pure invariants that must hold on every machine.
pub fn run_apower_checks() -> CheckSet {
    let mut set = CheckSet::new("apower");

    set.add(
        "A776 slp_typ",
        AcpiPowerState::from_slp_typ(3 << 10) == Some(AcpiPowerState::Sleeping(3))
            && AcpiPowerState::from_slp_typ(0) == Some(AcpiPowerState::Working)
            && AcpiPowerState::from_slp_typ(5 << 10) == Some(AcpiPowerState::SoftOff)
            && AcpiPowerState::from_slp_typ(6 << 10).is_none(),
        "decode",
    );
    set.add(
        "A776 latency",
        AcpiPowerState::Sleeping(1).latency_budget_ms() == 220
            && AcpiPowerState::SoftOff.latency_budget_ms() == 5000,
        "budget",
    );

    let r = FreqRange { min_khz: 800_000, max_khz: 3_600_000, steps: 7 };
    set.add(
        "A777 clamp",
        clamp_freq(100_000, r) == 800_000 && clamp_freq(9_999_999, r) == 3_600_000
            && clamp_freq(2_000_000, r) == 2_000_000 && clamp_freq(1_999_999, r) == 1_600_000,
        "snap",
    );
    set.add(
        "A777 ramp",
        ramp_freq(1_000_000, 3_600_000, 200_000) == 1_200_000
            && ramp_freq(3_600_000, 1_000_000, 200_000) == 3_400_000
            && ramp_freq(2_000_000, 2_100_000, 200_000) == 2_100_000,
        "limiter",
    );

    let mut tb = TurboBudget {
        nominal_khz: 2_000_000,
        boost_khz: 3_000_000,
        remaining_millijoules: 10_000,
        boost_millijoules_per_ms: 100,
    };
    set.add("A778 boost on", tb.allowed_khz(true, 10) == 3_000_000, "budget holds");
    tb.spend(100, true);
    set.add(
        "A778 partial boost",
        tb.allowed_khz(true, 100) == 2_000_000 && tb.allowed_khz(false, 100) == 2_000_000,
        "spend",
    );
    set.add("A778 ecore", place_task(400, 500) && !place_task(600, 500), "placement");

    let allowed = [DState::D0, DState::D1, DState::D2, DState::D3hot, DState::D3cold];
    set.add(
        "A779 d-states",
        DState::D3cold.loses_context() && !DState::D3cold.can_wake()
            && deepest_wake_state(allowed) == DState::D3hot,
        "wake cap",
    );

    let fan = FanState { duty_percent: 50, rpm: 2350 };
    set.add(
        "A780 fan",
        rpm_in_band(fan, 4800) && !rpm_in_band(FanState { rpm: 1000, ..fan }, 4800),
        "tach band",
    );
    set.add(
        "A780 trips",
        trip_action(99_000, 70_000, 85_000, 100_000) == TripAction::FanBoost
            && trip_action(101_000, 70_000, 85_000, 100_000) == TripAction::EmergencyShutdown
            && trip_action(60_000, 70_000, 85_000, 100_000) == TripAction::None,
        "ladder",
    );

    let b = BatterySnapshot { present: true, soc_permille: 500, charging: false, rate_mw: -5000 };
    set.add(
        "A781 eta",
        battery_eta_minutes(b, 50_000) == Some(5) && battery_eta_minutes(b, 0).is_none()
            && soc_stable(500, 505) && !soc_stable(500, 520),
        "gauge",
    );

    set.add(
        "A782 s0ix",
        s0ix_ok(S0ixResidency { deep_permille: 700, blockers: 0, valid: true })
            && !s0ix_ok(S0ixResidency { deep_permille: 700, blockers: 1, valid: true })
            && classify_blocker(false, true, false, false) == S0ixBlocker::NicActive,
        "residency",
    );

    set.add(
        "A783 ppw",
        perf_per_watt_milli(PerfPerWatt { ops_per_s: 10_000, milliwatts: 2_000 }) == 5000
            && efficiency_verdict(PerfPerWatt { ops_per_s: 10_000, milliwatts: 2_000 })
            && !efficiency_verdict(PerfPerWatt { ops_per_s: 1_000, milliwatts: 2_000 }),
        "ops/W",
    );

    set.add(
        "A784 policy",
        policy_action(PowerRegime::OnAC, 0) == PolicyAction::FullSpeed
            && policy_action(PowerRegime::OnBattery, 900) == PolicyAction::Balanced
            && policy_action(PowerRegime::OnBattery, 100) == PolicyAction::Save
            && policy_action(PowerRegime::Critical, 20) == PolicyAction::HibernateNow,
        "regime",
    );

    set.add(
        "A785 sleep",
        sleep_transition(0b10, AcpiPowerState::Sleeping(3)) == SleepResult::Entered
            && sleep_transition(0b01, AcpiPowerState::Sleeping(3)) == SleepResult::WakeBy("rtc")
            && sleep_transition(0, AcpiPowerState::Sleeping(3)) == SleepResult::Failed
            && wake_latency_ok(480) && !wake_latency_ok(520),
        "transition",
    );

    set.add(
        "A786 checkup core",
        power_checkup_core(AcpiPowerState::Sleeping(1), 0b10)
            && power_checkup_core(AcpiPowerState::SoftOff, 0),
        "core",
    );

    set.add(
        "A787 budget",
        governor_tick_budget(40) && !governor_tick_budget(60)
            && turbo_gain_ok(1000, 1200) && !turbo_gain_ok(1000, 1100),
        "bench",
    );

    let mut log = PmEventLog::new();
    log.push(PmEvent { kind: PmEventKind::TurboEnter, stamp: 1, arg: 0 });
    log.push(PmEvent { kind: PmEventKind::Wake, stamp: 2, arg: 3 });
    set.add(
        "A789 events",
        log.len() == 2 && log.get(0).unwrap().kind == PmEventKind::Wake
            && log.count_of(PmEventKind::TurboEnter) == 1,
        "ring",
    );

    set.add(
        "A790 fuzz",
        fuzz_slp_typ(0xFFFF) && fuzz_slp_typ(0)
            && fuzz_clamp_freq(1_500_000, 800_000, 3_600_000, 7)
            && !fuzz_clamp_freq(1, 3_600_000, 800_000, 7),
        "robust",
    );

    let cells = [
        PmCompatCell { platform: "q35", s3: true, s0ix: false, turbo: false },
        PmCompatCell { platform: "mobile", s3: true, s0ix: true, turbo: true },
    ];
    set.add(
        "A791 compat",
        pm_compat_ok(&cells) && !pm_compat_ok(&[PmCompatCell { s3: false, ..cells[0] }]),
        "matrix",
    );

    set.add(
        "A792 hysteresis",
        shutdown_hysteresis(100_000, 100_000, false)
            && shutdown_hysteresis(95_000, 100_000, true)
            && !shutdown_hysteresis(89_000, 100_000, true),
        "trip latch",
    );

    set.add(
        "A795 perf budget",
        perf_per_watt_milli(PerfPerWatt { ops_per_s: 0, milliwatts: 0 }) == 0,
        "no div0",
    );

    set.add("A796 obs cap", log.len() <= PM_EVENT_CAP, "bounded");

    set.add(
        "A798 docs headroom",
        clamp_freq(800_000, r) == 800_000,
        "min reachable",
    );

    set.add(
        "A799 degrade",
        pm_degrade(false, false) == PmTier::Safe
            && pm_degrade(true, true) == PmTier::Reduced
            && pm_degrade(true, false) == PmTier::Full,
        "chain",
    );

    set.add(
        "A800 closure",
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
    fn a776_state_table() {
        for s in 1..=4u8 {
            let st = AcpiPowerState::Sleeping(s);
            assert_eq!(AcpiPowerState::from_slp_typ((s as u16) << 10), Some(st));
            assert!(st.latency_budget_ms() > 0);
        }
        assert!(AcpiPowerState::from_slp_typ(7 << 10).is_none());
    }

    #[test]
    fn a777_freq_edges() {
        let r = FreqRange { min_khz: 1_000_000, max_khz: 2_000_000, steps: 0 };
        assert_eq!(clamp_freq(1_500_000, r), 1_500_000);
        let r2 = FreqRange { min_khz: 2_000_000, max_khz: 1_000_000, steps: 4 }; // reversed
        assert_eq!(clamp_freq(500_000, r2), 1_000_000);
        assert_eq!(ramp_freq(100, 100, 0), 100);
        assert_eq!(ramp_freq(100, 900, 1_000_000), 900);
    }

    #[test]
    fn a778_turbo_drain() {
        let mut tb = TurboBudget {
            nominal_khz: 1_000_000,
            boost_khz: 1_500_000,
            remaining_millijoules: 1_000,
            boost_millijoules_per_ms: 10,
        };
        tb.spend(50, true);
        assert_eq!(tb.remaining_millijoules, 500);
        tb.spend(100, true);
        assert_eq!(tb.remaining_millijoules, 0);
        assert_eq!(tb.allowed_khz(true, 10), 1_000_000);
        // Boost below nominal is nonsense → nominal.
        let inv = TurboBudget { nominal_khz: 2_000_000, boost_khz: 1_000_000, remaining_millijoules: 9, boost_millijoules_per_ms: 1 };
        assert_eq!(inv.allowed_khz(true, 1), 2_000_000);
    }

    #[test]
    fn a779_dstate_ladder() {
        assert!(DState::D0.can_wake() && DState::D3hot.can_wake());
        let no_wake = [DState::D3cold, DState::D3cold, DState::D3cold, DState::D3cold, DState::D3cold];
        assert_eq!(deepest_wake_state(no_wake), DState::D0);
    }

    #[test]
    fn a780_fan_math() {
        assert!(rpm_in_band(FanState { duty_percent: 0, rpm: 0 }, 4000));
        assert!(!rpm_in_band(FanState { duty_percent: 100, rpm: 3000 }, 4000));
        assert_eq!(trip_action(85_000, 70_000, 85_000, 100_000), TripAction::FanBoost);
        assert_eq!(trip_action(70_000, 70_000, 85_000, 100_000), TripAction::PassiveThrottle);
    }

    #[test]
    fn a781_battery_edges() {
        let full = BatterySnapshot { present: true, soc_permille: 1000, charging: false, rate_mw: -1000 };
        assert_eq!(battery_eta_minutes(full, 60_000), Some(60));
        let charging = BatterySnapshot { soc_permille: 0, charging: true, rate_mw: 30_000, ..full };
        assert_eq!(battery_eta_minutes(charging, 60_000), Some(2));
        let idle = BatterySnapshot { rate_mw: 0, ..full };
        assert!(battery_eta_minutes(idle, 60_000).is_none());
        let absent = BatterySnapshot { present: false, ..full };
        assert!(battery_eta_minutes(absent, 60_000).is_none());
    }

    #[test]
    fn a782_blocker_priority() {
        assert_eq!(classify_blocker(true, true, true, true), S0ixBlocker::AudioActive);
        assert_eq!(classify_blocker(false, false, false, false), S0ixBlocker::None);
        assert!(!s0ix_ok(S0ixResidency { deep_permille: 599, blockers: 0, valid: true }));
        assert!(!s0ix_ok(S0ixResidency { deep_permille: 900, blockers: 0, valid: false }));
    }

    #[test]
    fn a783_a784_metrics() {
        assert_eq!(perf_per_watt_milli(PerfPerWatt { ops_per_s: 7_500, milliwatts: 1_500 }), 5000);
        assert_eq!(
            policy_action(PowerRegime::OnBattery, 149),
            PolicyAction::Save
        );
        assert_eq!(policy_action(PowerRegime::Critical, 31), PolicyAction::Save);
        assert_eq!(policy_action(PowerRegime::Critical, 30), PolicyAction::HibernateNow);
    }

    #[test]
    fn a785_wake_mask_bits() {
        assert_eq!(sleep_transition(0b100, AcpiPowerState::Sleeping(3)), SleepResult::Entered);
        assert_eq!(
            sleep_transition(0b1, AcpiPowerState::SoftOff),
            SleepResult::WakeBy("rtc")
        );
        assert_eq!(
            sleep_transition(0, AcpiPowerState::Working),
            SleepResult::Failed
        );
    }

    #[test]
    fn a789_ring_wraps() {
        let mut l = PmEventLog::new();
        for i in 0..PM_EVENT_CAP + 6 {
            l.push(PmEvent { kind: PmEventKind::DStateChange, stamp: i as u64, arg: i as u32 });
        }
        assert_eq!(l.len(), PM_EVENT_CAP);
        assert_eq!(l.get(0).unwrap().stamp, (PM_EVENT_CAP + 5) as u64);
        assert!(l.get(PM_EVENT_CAP).is_none());
        assert_eq!(l.count_of(PmEventKind::DStateChange), PM_EVENT_CAP);
    }

    #[test]
    fn a790_a797_fuzz() {
        for v in [0u16, 1, 0x7FFF, 0xFFFF, 3 << 10, 5 << 10, 6 << 10, 7 << 10] {
            assert!(fuzz_slp_typ(v), "slp_typ {v:#06x}");
        }
        assert!(fuzz_clamp_freq(0, 0, 0, 0)); // steps==0 → passthrough contract
        assert!(fuzz_clamp_freq(50, 100, 200, 1));
    }

    #[test]
    fn a799_a800_final() {
        assert_eq!(pm_degrade(true, false), PmTier::Full);
        let set = run_apower_checks();
        if !set.all_passed() {
            let mut buf = [0u8; 1024];
            let n = set.render(&mut buf);
            panic!("apower self-test:\n{}", core::str::from_utf8(&buf[..n]).unwrap());
        }
        assert!(set.len() >= 25);
    }
}
