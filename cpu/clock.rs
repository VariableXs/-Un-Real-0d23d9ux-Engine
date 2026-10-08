//! F037 时钟中断框架 / F038 TSC 校准 / F039 HPET 驱动.
//!
//! One clock abstraction for the whole kernel: a monotonic nanosecond counter
//! plus a periodic tick that every domain can subscribe to. The TSC is the
//! preferred source (cheap, invariant), HPET is the reference used to calibrate
//! it, and the PIT is the last-resort denominator.

use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// Scheduler heartbeat. 1 kHz keeps交互 latency low without melting the budget;
/// the value is a compile-time constant so the tick math stays branch-free.
pub const TICK_HZ: u64 = 1000;
/// Nanoseconds per tick at 1 kHz.
pub const TICK_NS: u64 = 1_000_000_000 / TICK_HZ;
/// Femtoseconds per nanosecond (HPET period unit).
pub const FS_PER_NS: u64 = 1_000_000;
/// HPET period must not exceed 100 ns (ACPI spec: ≤ 10⁻⁷ s).
pub const HPET_MAX_PERIOD_FS: u64 = 100_000_000;

/// Which hardware the monotonic clock is actually reading.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ClockSource {
    #[default]
    None,
    Pit,
    Hpet,
    Tsc,
}

impl ClockSource {
    pub fn as_str(self) -> &'static str {
        match self {
            ClockSource::None => "none",
            ClockSource::Pit => "pit",
            ClockSource::Hpet => "hpet",
            ClockSource::Tsc => "tsc",
        }
    }
}

// ---------------------------------------------------------------------------
// F038 — TSC calibration
// ---------------------------------------------------------------------------

/// How the TSC frequency was determined; more trustworthy sources win.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CalibSource {
    /// Fallback: no firmware data at all.
    Fallback,
    /// Measured against the PIT/HPET.
    Measured,
    /// CPUID leaf 0x16 (nominal frequency).
    CpuLeaf16,
    /// CPUID leaf 0x15 (crystal clock + ratio) — the best answer.
    CpuLeaf15,
    /// Leaf 0x15 exists but the denominator is 0 → unusable.
    Invalid,
}

/// The calibration record for the invariant TSC.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TscCalibration {
    pub hz: u64,
    pub source: CalibSource,
    pub invariant: bool,
}

impl TscCalibration {
    pub const FALLBACK_HZ: u64 = 2_500_000_000;

    /// Prefer leaf 0x15, then 0x16, then a measurement, then the fallback.
    pub fn choose(
        leaf15: Option<(u32, u32, u32)>,
        leaf16: Option<u32>,
        measured: Option<u64>,
        invariant: bool,
    ) -> TscCalibration {
        if let Some((denom, numer, crystal)) = leaf15 {
            if denom != 0 {
                let crystal = if crystal != 0 { crystal as u64 } else { 25_000_000 };
                let hz = crystal * numer as u64 / denom as u64;
                return TscCalibration { hz, source: CalibSource::CpuLeaf15, invariant };
            }
            return TscCalibration { hz: 0, source: CalibSource::Invalid, invariant };
        }
        if let Some(mhz) = leaf16 {
            if mhz != 0 {
                return TscCalibration {
                    hz: mhz as u64 * 1_000_000,
                    source: CalibSource::CpuLeaf16,
                    invariant,
                };
            }
        }
        if let Some(hz) = measured {
            if hz != 0 {
                return TscCalibration { hz, source: CalibSource::Measured, invariant };
            }
        }
        TscCalibration {
            hz: Self::FALLBACK_HZ,
            source: CalibSource::Fallback,
            invariant,
        }
    }

    /// Ticks → nanoseconds without overflowing at typical frequencies.
    pub fn tsc_to_ns(&self, tsc: u64) -> u64 {
        if self.hz == 0 {
            return 0;
        }
        muldiv_u64(tsc, 1_000_000_000, self.hz)
    }

    /// Nanoseconds → TSC ticks.
    pub fn ns_to_tsc(&self, ns: u64) -> u64 {
        if self.hz == 0 {
            return 0;
        }
        muldiv_u64(ns, self.hz, 1_000_000_000)
    }

    /// LAPIC/HPET ticks that make up one scheduler tick.
    pub fn ticks_per_tick(&self) -> u64 {
        self.ns_to_tsc(TICK_NS)
    }

    /// Is this good enough to be the system clock?
    pub fn usable(&self) -> bool {
        self.source != CalibSource::Invalid && self.hz > 0
    }
}

/// 128-bit intermediate multiply/divide, so a 4 GHz TSC cannot overflow.
fn muldiv_u64(v: u64, mul: u64, div: u64) -> u64 {
    let v = v as u128;
    let mul = mul as u128;
    let div = div as u128;
    if div == 0 {
        return 0;
    }
    let r = (v * mul) / div;
    if r > u64::MAX as u128 {
        u64::MAX
    } else {
        r as u64
    }
}

/// Read the TSC. Serialising variant available for latency measurement.
pub fn read_tsc() -> u64 {
    #[cfg(all(target_arch = "x86_64", target_os = "none"))]
    {
        let (hi, lo): (u32, u32);
        // SAFETY: TSC is present on every x86_64 Varix supports.
        unsafe {
            core::arch::asm!(
                "rdtsc",
                out("eax") lo,
                out("edx") hi,
                options(nomem, nostack, preserves_flags)
            );
        }
        ((hi as u64) << 32) | lo as u64
    }
    #[cfg(not(all(target_arch = "x86_64", target_os = "none")))]
    {
        // Host build: a coarse but monotonic stand-in for the real counter.
        static FAKE: AtomicU64 = AtomicU64::new(0);
        FAKE.fetch_add(1000, Ordering::Relaxed)
    }
}

/// `rdtscp` — ordered read that also reports the CPU id (F047 helper).
pub fn read_tsc_aux() -> (u64, u32) {
    #[cfg(all(target_arch = "x86_64", target_os = "none"))]
    {
        let (hi, lo, aux): (u32, u32, u32);
        // SAFETY: `rdtscp` is enumerated by CPUID.80000001:EDX[27]; the BSP
        // falls back to `rdtsc` when it is missing.
        unsafe {
            core::arch::asm!(
                "rdtscp",
                out("eax") lo,
                out("edx") hi,
                out("ecx") aux,
                options(nomem, nostack, preserves_flags)
            );
        }
        (((hi as u64) << 32) | lo as u64, aux)
    }
    #[cfg(not(all(target_arch = "x86_64", target_os = "none")))]
    {
        (read_tsc(), 0)
    }
}

// ---------------------------------------------------------------------------
// F039 — HPET
// ---------------------------------------------------------------------------

pub const HPET_CAP_ID: u64 = 0x000;
pub const HPET_GEN_CONF: u64 = 0x010;
pub const HPET_INT_STATUS: u64 = 0x020;
pub const HPET_MAIN_COUNTER: u64 = 0x0F0;
pub const HPET_TIMER0_CONF: u64 = 0x100;
pub const HPET_TIMER0_COMP: u64 = 0x108;
pub const HPET_TIMER_STRIDE: u64 = 0x020;

pub const HPET_CONF_ENABLE: u64 = 1 << 0;
pub const HPET_CONF_LEGACY: u64 = 1 << 1;
pub const HPET_TIMER_INT_EN: u64 = 1 << 2;
pub const HPET_TIMER_PERIODIC: u64 = 1 << 3;
pub const HPET_TIMER_PERIODIC_CAP: u64 = 1 << 4;
pub const HPET_TIMER_64BIT: u64 = 1 << 5;
pub const HPET_TIMER_SET_ACC: u64 = 1 << 6;
pub const HPET_TIMER_LEVEL: u64 = 1 << 1;

/// Decoded HPET capability register.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HpetCaps {
    /// Main counter period in femtoseconds.
    pub period_fs: u64,
    pub vendor_id: u16,
    pub num_timers: u8,
    pub counter_64bit: bool,
    pub legacy_replacement: bool,
    pub revision: u8,
}

impl HpetCaps {
    pub fn from_raw(raw: u64) -> HpetCaps {
        HpetCaps {
            period_fs: raw >> 32,
            vendor_id: ((raw >> 16) & 0xFFFF) as u16,
            num_timers: (((raw >> 8) & 0x1F) + 1) as u8,
            counter_64bit: raw & (1 << 13) != 0,
            legacy_replacement: raw & (1 << 15) != 0,
            revision: (raw & 0xFF) as u8,
        }
    }

    /// Spec-conformant period? Anything outside this window is a broken
    /// emulation and must not become the system clock.
    pub fn period_valid(&self) -> bool {
        self.period_fs > 0 && self.period_fs <= HPET_MAX_PERIOD_FS
    }

    /// Main counter frequency in Hz, rounded to nearest (a truncated 14.31818
    /// MHz would drift by ~1 ppm — small, but it shows up after a day of
    /// uptime, and there is no reason to accept it).
    pub fn hz(&self) -> u64 {
        if !self.period_valid() {
            return 0;
        }
        let per_second_fs = FS_PER_NS * 1_000_000_000;
        (per_second_fs + self.period_fs / 2) / self.period_fs
    }
}

/// An adopted HPET.
#[derive(Clone, Copy, Debug, Default)]
pub struct Hpet {
    pub base: u64,
    pub caps: Option<HpetCaps>,
    pub enabled: bool,
}

impl Hpet {
    pub const fn new() -> Hpet {
        Hpet {
            base: 0,
            caps: None,
            enabled: false,
        }
    }

    pub fn is_present(&self) -> bool {
        self.base != 0 && self.caps.is_some()
    }

    pub fn period_fs(&self) -> u64 {
        self.caps.map(|c| c.period_fs).unwrap_or(0)
    }

    pub fn ticks_to_ns(&self, ticks: u64) -> u64 {
        let p = self.period_fs();
        if p == 0 {
            return 0;
        }
        muldiv_u64(ticks, p, FS_PER_NS)
    }

    pub fn ns_to_ticks(&self, ns: u64) -> u64 {
        let p = self.period_fs();
        if p == 0 {
            return 0;
        }
        muldiv_u64(ns, FS_PER_NS, p)
    }

    fn read64(&self, off: u64) -> u64 {
        if self.base == 0 {
            return 0;
        }
        // SAFETY: the HPET window is mapped by AI-03 before this runs.
        unsafe { core::ptr::read_volatile((self.base + off) as *const u64) }
    }

    fn write64(&self, off: u64, v: u64) {
        if self.base == 0 {
            return;
        }
        // SAFETY: see `read64`.
        unsafe { core::ptr::write_volatile((self.base + off) as *mut u64, v) }
    }

    pub fn counter(&self) -> u64 {
        self.read64(HPET_MAIN_COUNTER)
    }

    /// Adopt + enable the HPET. Refuses a timer with an insane period.
    pub fn init(&mut self, phys: u64) -> bool {
        // HPET is device MMIO: translate through the HHDM direct map, the raw
        // physical address is not dereferenceable in the higher-half kernel.
        self.base = crate::mem::paging::phys_to_virt(phys);
        let raw = self.read64(HPET_CAP_ID);
        let caps = HpetCaps::from_raw(raw);
        if !caps.period_valid() {
            crate::kwarn!("hpet: period {} fs out of spec — ignored", caps.period_fs);
            self.caps = None;
            self.enabled = false;
            return false;
        }
        // Disable first, zero the counter, then enable with legacy routing off
        // (the IO APIC owns delivery once F033 is up).
        self.write64(HPET_GEN_CONF, 0);
        self.write64(HPET_MAIN_COUNTER, 0);
        self.write64(HPET_GEN_CONF, HPET_CONF_ENABLE);
        self.caps = Some(caps);
        self.enabled = true;
        crate::kinfo!(
            "hpet: {} timers, period={} fs ({} Hz), 64-bit={}",
            caps.num_timers,
            caps.period_fs,
            caps.hz(),
            caps.counter_64bit
        );
        true
    }

    /// Arm comparator `timer` to fire every `ns` (periodic) or once (one-shot).
    pub fn arm(&self, timer: u8, vector: u8, ns: u64, periodic: bool) -> bool {
        let caps = match self.caps {
            Some(c) if (timer as u8) < c.num_timers => c,
            _ => return false,
        };
        if !self.enabled {
            return false;
        }
        let conf_off = HPET_TIMER0_CONF + timer as u64 * HPET_TIMER_STRIDE;
        let comp_off = HPET_TIMER0_COMP + timer as u64 * HPET_TIMER_STRIDE;
        let mut conf = self.read64(conf_off);
        let periodic_cap = conf & HPET_TIMER_PERIODIC_CAP != 0;
        conf &= !(HPET_TIMER_INT_EN | HPET_TIMER_PERIODIC);
        conf |= (vector as u64) << 9; // route to this IO APIC vector
        if periodic {
            if !periodic_cap {
                return false;
            }
            conf |= HPET_TIMER_PERIODIC | HPET_TIMER_SET_ACC;
            self.write64(comp_off, self.ns_to_ticks(ns));
            self.write64(comp_off, self.ns_to_ticks(ns));
        } else {
            self.write64(comp_off, self.counter() + self.ns_to_ticks(ns));
        }
        conf |= HPET_TIMER_INT_EN;
        if caps.counter_64bit {
            conf |= HPET_TIMER_64BIT;
        }
        self.write64(conf_off, conf);
        true
    }
}

/// SAFETY: written exactly once during boot (before SMP start), read-only
/// afterwards; `&mut` is only handed out on the single-threaded boot path.
struct HpetCell {
    inner: core::cell::UnsafeCell<Hpet>,
}
unsafe impl Sync for HpetCell {}
static HPET: HpetCell = HpetCell {
    inner: core::cell::UnsafeCell::new(Hpet::new()),
};

pub fn hpet() -> &'static Hpet {
    // SAFETY: see `HpetCell`.
    unsafe { &*HPET.inner.get() }
}

/// F039 entry point: adopt the HPET if ACPI handed us one.
pub fn init_hpet(phys: u64) -> bool {
    if phys == 0 {
        return false;
    }
    // SAFETY: single-threaded boot path.
    let h = unsafe { &mut *HPET.inner.get() };
    h.init(phys)
}

// ---------------------------------------------------------------------------
// F037 — the tick framework
// ---------------------------------------------------------------------------

/// A tick subscriber. Kept as a plain function pointer: the scheduler, the
/// latency meter and the frame-budget guardian all want the cheapest possible
/// call from the ISR.
pub type TickFn = fn(u64);

pub const MAX_TICK_HANDLERS: usize = 8;

struct TickHub {
    handlers: [Option<TickFn>; MAX_TICK_HANDLERS],
    count: usize,
}

impl TickHub {
    const fn new() -> TickHub {
        TickHub {
            handlers: [None; MAX_TICK_HANDLERS],
            count: 0,
        }
    }
}

// SAFETY: registered during boot / by a single owner at a time.
unsafe impl Sync for TickHubInner {}
struct TickHubInner {
    hub: core::cell::UnsafeCell<TickHub>,
    ticks: AtomicU64,
    started: AtomicBool,
}
static HUB: TickHubInner = TickHubInner {
    hub: core::cell::UnsafeCell::new(TickHub::new()),
    ticks: AtomicU64::new(0),
    started: AtomicBool::new(false),
};

/// Register a tick subscriber. Returns `false` when the hub is full.
pub fn on_tick(f: TickFn) -> bool {
    // SAFETY: boot-time registration; the hub is not published until `start`.
    let hub = unsafe { &mut *HUB.hub.get() };
    if hub.count >= MAX_TICK_HANDLERS {
        return false;
    }
    hub.handlers[hub.count] = Some(f);
    hub.count += 1;
    true
}

pub fn tick_handlers() -> usize {
    // SAFETY: read-only view.
    unsafe { (*HUB.hub.get()).count }
}

/// ISR entry: advance the counter and fan out. Handlers must be short — the
/// frame-budget guardian (F398) watches this number.
pub fn tick() {
    let n = HUB.ticks.fetch_add(1, Ordering::Relaxed) + 1;
    // SAFETY: handlers are function pointers registered before `start`.
    let hub = unsafe { &*HUB.hub.get() };
    for slot in hub.handlers.iter() {
        if let Some(f) = slot {
            f(n);
        }
    }
}

pub fn ticks() -> u64 {
    HUB.ticks.load(Ordering::Relaxed)
}

pub fn started() -> bool {
    HUB.started.load(Ordering::Acquire)
}

/// Wall-clock-ish monotonic time (ns since boot). Uses the calibrated TSC when
/// available, otherwise accumulated ticks.
pub fn now_ns() -> u64 {
    let cal = calibration();
    if cal.usable() {
        cal.tsc_to_ns(read_tsc())
    } else {
        ticks() * TICK_NS
    }
}

/// SAFETY: written exactly once by `calibrate`, read-only afterwards.
struct CalCell {
    inner: core::cell::UnsafeCell<TscCalibration>,
}
unsafe impl Sync for CalCell {}
static CAL: CalCell = CalCell {
    inner: core::cell::UnsafeCell::new(TscCalibration {
        hz: 0,
        source: CalibSource::Invalid,
        invariant: false,
    }),
};

pub fn calibration() -> TscCalibration {
    // SAFETY: see `CalCell`.
    unsafe { *CAL.inner.get() }
}

/// F038: run the calibration ladder and publish the result.
pub fn calibrate(
    leaf15: Option<(u32, u32, u32)>,
    leaf16: Option<u32>,
    measured: Option<u64>,
    invariant: bool,
) -> TscCalibration {
    let c = TscCalibration::choose(leaf15, leaf16, measured, invariant);
    // SAFETY: single-threaded boot path.
    unsafe { *CAL.inner.get() = c };
    crate::kinfo!(
        "tsc: {} Hz via {:?} (invariant={})",
        c.hz,
        c.source,
        c.invariant
    );
    c
}

/// F037: pick the source, arm the periodic interrupt, start ticking.
pub fn init(hpet_phys: u64) -> ClockSource {
    let hpet_ok = init_hpet(hpet_phys);

    let plat = crate::platform::info();
    let invariant = plat.map(|p| p.tsc_hz > 0).unwrap_or(false);
    // AI-01 already resolved the TSC frequency from CPUID; reuse it rather than
    // re-walking the leaves (zero-duplicate rule), so the ladder starts at the
    // measured rung with the platform value as the measurement.
    let cal = calibrate(leaf15_hint(), None, plat.map(|p| p.tsc_hz), invariant);

    let source = if cal.usable() && cal.invariant {
        ClockSource::Tsc
    } else if hpet_ok {
        ClockSource::Hpet
    } else {
        ClockSource::Pit
    };

    match source {
        ClockSource::Tsc => {
            // Deadline/one-shot mode: one LAPIC timer reload per tick.
            crate::cpu::apic::lapic().arm_timer(
                crate::cpu::idt::IRQ_BASE,
                cal.ticks_per_tick() as u32,
                true,
            );
        }
        ClockSource::Hpet => {
            let h = hpet();
            if h.is_present() {
                let _ = h.arm(0, crate::cpu::idt::IRQ_BASE, TICK_NS, true);
            }
        }
        _ => {
            crate::kwarn!("clock: no precise source — ticking from the PIT");
        }
    }

    HUB.started.store(true, Ordering::Release);
    crate::kinfo!("clock: source={} tick={}Hz", source.as_str(), TICK_HZ);
    source
}

/// CPUID.15 hint. The platform domain already parsed the leaf; this keeps the
/// clock domain from re-reading CPUID (zero duplicate work).
fn leaf15_hint() -> Option<(u32, u32, u32)> {
    // AI-01 stores only the resulting frequency, so the ratio is unavailable
    // here; `None` pushes the ladder to the measured/fallback rung.
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calibration_ladder_prefers_leaf15() {
        let c = TscCalibration::choose(Some((2, 200, 25_000_000)), Some(3000), Some(1), true);
        assert_eq!(c.source, CalibSource::CpuLeaf15);
        assert_eq!(c.hz, 25_000_000 * 200 / 2);
        assert!(c.usable());

        let c2 = TscCalibration::choose(None, Some(3000), Some(2_000_000_000), true);
        assert_eq!(c2.source, CalibSource::CpuLeaf16);
        assert_eq!(c2.hz, 3_000_000_000);

        let c3 = TscCalibration::choose(None, None, Some(1_800_000_000), false);
        assert_eq!(c3.source, CalibSource::Measured);

        let c4 = TscCalibration::choose(None, None, None, false);
        assert_eq!(c4.source, CalibSource::Fallback);
        assert_eq!(c4.hz, TscCalibration::FALLBACK_HZ);

        let c5 = TscCalibration::choose(Some((0, 0, 0)), None, Some(1), true);
        assert_eq!(c5.source, CalibSource::Invalid);
        assert!(!c5.usable());
    }

    #[test]
    fn tsc_conversions_round_trip() {
        let c = TscCalibration {
            hz: 3_000_000_000,
            source: CalibSource::CpuLeaf16,
            invariant: true,
        };
        assert_eq!(c.tsc_to_ns(3_000_000_000), 1_000_000_000);
        assert_eq!(c.ns_to_tsc(1_000_000_000), 3_000_000_000);
        assert_eq!(c.ticks_per_tick(), 3_000_000);
        // No overflow at multi-GHz for large deltas.
        assert_eq!(c.tsc_to_ns(u64::MAX / 4), (u64::MAX / 4) / 3);
    }

    #[test]
    fn hpet_caps_decode_and_validate() {
        // Typical QEMU: 100 ns? period 69_841_279 fs (14.318 MHz), 3 timers.
        let raw = (69_841_279u64 << 32) | (0x8086 << 16) | (2 << 8) | (1 << 13) | 1;
        let c = HpetCaps::from_raw(raw);
        assert_eq!(c.period_fs, 69_841_279);
        assert_eq!(c.vendor_id, 0x8086);
        assert_eq!(c.num_timers, 3);
        assert!(c.counter_64bit);
        assert!(!c.legacy_replacement);
        assert!(c.period_valid());
        assert_eq!(c.hz(), 14_318_180);
        // Out-of-spec period is rejected.
        let bad = HpetCaps::from_raw((200_000_000u64 << 32) | 1);
        assert!(!bad.period_valid());
        assert_eq!(bad.hz(), 0);
    }

    #[test]
    fn hpet_time_conversion() {
        let mut h = Hpet::new();
        h.caps = Some(HpetCaps {
            period_fs: 100_000_000, // 10 MHz → 100 ns per tick
            vendor_id: 0,
            num_timers: 3,
            counter_64bit: true,
            legacy_replacement: false,
            revision: 1,
        });
        assert_eq!(h.ticks_to_ns(10), 1_000);
        assert_eq!(h.ns_to_ticks(1_000), 10);
        let zero = Hpet::new();
        assert_eq!(zero.ticks_to_ns(10), 0);
        assert_eq!(zero.ns_to_ticks(10), 0);
    }

    #[test]
    fn tick_hub_registers_and_fans_out() {
        static A: AtomicU64 = AtomicU64::new(0);
        static B: AtomicU64 = AtomicU64::new(0);
        fn a(n: u64) {
            A.store(n, Ordering::Relaxed);
        }
        fn b(n: u64) {
            B.store(n, Ordering::Relaxed);
        }
        assert!(on_tick(a));
        assert!(on_tick(b));
        assert_eq!(tick_handlers(), 2);
        tick();
        tick();
        assert_eq!(A.load(Ordering::Relaxed), 2);
        assert_eq!(B.load(Ordering::Relaxed), 2);
        assert_eq!(ticks(), 2);
    }

    #[test]
    fn tick_constants_are_consistent() {
        assert_eq!(TICK_HZ, 1000);
        assert_eq!(TICK_NS, 1_000_000);
        assert_eq!(FS_PER_NS, 1_000_000);
    }
}
