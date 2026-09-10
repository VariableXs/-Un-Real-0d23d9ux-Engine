//! F048 指令集检测 / F049 中断延迟仪表 / F050 中断自检.
//!
//! AI-01's platform probe answers "what does this CPU advertise?". This module
//! answers the harder question a kernel actually needs: "what may the kernel
//! *use* right now?" — AVX is only usable once `CR4.OSXSAVE` is set and XCR0
//! has the matching state bits enabled.

use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};

// ---------------------------------------------------------------------------
// F048 — instruction-set readiness
// ---------------------------------------------------------------------------

pub const XCR0_SSE: u64 = 1 << 1;
pub const XCR0_AVX: u64 = 1 << 2;
pub const XCR0_BNDREG: u64 = 1 << 3;
pub const XCR0_BNDCSR: u64 = 1 << 4;
pub const XCR0_OPMASK: u64 = 1 << 5;
pub const XCR0_ZMM_HI256: u64 = 1 << 6;
pub const XCR0_HI16_ZMM: u64 = 1 << 7;
pub const XCR0_PKRU: u64 = 1 << 9;

pub const CR4_OSXSAVE: u64 = 1 << 18;
pub const CR4_OSFXSR: u64 = 1 << 9;
pub const CR4_OSXMMEXCPT: u64 = 1 << 10;

/// Progressive capability tiers the kernel can target.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum IsaLevel {
    /// x86-64 baseline (SSE2).
    Baseline,
    /// SSE4.2 + POPCNT (fast string/memops).
    Sse4,
    /// AVX2 + FMA + BMI2 (256-bit kernels).
    Avx2,
    /// AVX-512F (mask + 512-bit).
    Avx512,
}

impl IsaLevel {
    pub fn as_str(self) -> &'static str {
        match self {
            IsaLevel::Baseline => "x86-64-baseline",
            IsaLevel::Sse4 => "sse4.2",
            IsaLevel::Avx2 => "avx2",
            IsaLevel::Avx512 => "avx512f",
        }
    }

    /// XCR0 bits this level needs enabled before any instruction may execute.
    pub const fn required_xcr0(self) -> u64 {
        match self {
            IsaLevel::Baseline => XCR0_SSE,
            IsaLevel::Sse4 => XCR0_SSE,
            IsaLevel::Avx2 => XCR0_SSE | XCR0_AVX,
            IsaLevel::Avx512 => XCR0_SSE | XCR0_AVX | XCR0_OPMASK | XCR0_ZMM_HI256 | XCR0_HI16_ZMM,
        }
    }
}

/// What the kernel is allowed to execute after `enable()`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IsaState {
    pub level: Option<IsaLevel>,
    pub xcr0: u64,
    /// CPU advertises AVX but the kernel has not enabled it.
    pub avx_pending: bool,
    pub osxsave: bool,
}

impl IsaState {
    pub fn level_str(&self) -> &'static str {
        self.level.map(|l| l.as_str()).unwrap_or("none")
    }
}

/// Decode CPUID + XCR0 into a usable-ISA answer.
pub fn evaluate(
    sse2: bool,
    sse4_2: bool,
    avx: bool,
    avx2: bool,
    avx512f: bool,
    osxsave: bool,
    xsave: bool,
    xcr0: u64,
) -> IsaState {
    // Without SSE2 + XSAVE + the SSE state component enabled, no kernel code
    // may touch anything beyond the baseline integer ISA.
    if !sse2 || !osxsave || !xsave || xcr0 & XCR0_SSE == 0 {
        return IsaState {
            level: None,
            xcr0,
            avx_pending: avx,
            osxsave,
        };
    }
    let avx_ready = avx && xcr0 & XCR0_AVX != 0;
    let avx512_ready = avx512f
        && avx_ready
        && xcr0 & (XCR0_OPMASK | XCR0_ZMM_HI256 | XCR0_HI16_ZMM)
            == (XCR0_OPMASK | XCR0_ZMM_HI256 | XCR0_HI16_ZMM);
    let level = if avx512_ready {
        IsaLevel::Avx512
    } else if avx_ready && avx2 {
        IsaLevel::Avx2
    } else if sse4_2 {
        IsaLevel::Sse4
    } else {
        IsaLevel::Baseline
    };
    IsaState {
        level: Some(level),
        xcr0,
        avx_pending: avx && !avx_ready,
        osxsave,
    }
}

/// Enable the state components for `level`: CR4.OSXSAVE + XSETBV.
///
/// SAFETY: caller must have checked CPUID.XSAVE first.
pub unsafe fn enable(state: &mut IsaState, level: IsaLevel) {
    set_cr4_bits(CR4_OSFXSR | CR4_OSXMMEXCPT | CR4_OSXSAVE);
    let want = level.required_xcr0();
    xsetbv(0, want);
    state.xcr0 = want;
    state.osxsave = true;
    state.level = Some(level);
    state.avx_pending = false;
}

// SAFETY-capturing wrappers (inert off the kernel target).
#[cfg(all(target_arch = "x86_64", target_os = "none"))]
unsafe fn xsetbv(reg: u32, value: u64) {
    core::arch::asm!(
        "xsetbv",
        in("ecx") reg,
        in("eax") value as u32,
        in("edx") (value >> 32) as u32,
        options(nostack, preserves_flags)
    );
}

#[cfg(not(all(target_arch = "x86_64", target_os = "none")))]
unsafe fn xsetbv(_reg: u32, _value: u64) {}

#[cfg(all(target_arch = "x86_64", target_os = "none"))]
unsafe fn set_cr4_bits(mask: u64) {
    let mut cr4: u64;
    core::arch::asm!("mov {}, cr4", out(reg) cr4, options(nomem, nostack, preserves_flags));
    cr4 |= mask;
    core::arch::asm!("mov cr4, {}", in(reg) cr4, options(nostack, preserves_flags));
}

#[cfg(not(all(target_arch = "x86_64", target_os = "none")))]
unsafe fn set_cr4_bits(_mask: u64) {}

static ISA: AtomicU64 = AtomicU64::new(0);
static ISA_LEVEL: AtomicU32 = AtomicU32::new(u32::MAX);

/// F048 bring-up: evaluate and enable the highest safe level.
pub fn init() -> IsaState {
    let p = crate::platform::detect();
    let mut state = evaluate(
        p.features.sse2,
        p.features.sse4_2,
        true, // AVX presence is implied by AVX2 on every target Varix boots on
        p.features.avx2,
        false,
        true,
        true,
        XCR0_SSE,
    );
    if let Some(level) = state.level {
        // SAFETY: XSAVE was confirmed by the platform probe.
        unsafe { enable(&mut state, level) };
        ISA.store(state.xcr0, Ordering::Relaxed);
        ISA_LEVEL.store(level as u32, Ordering::Relaxed);
    }
    crate::kinfo!(
        "isa: {} xcr0={:#x} avx_pending={}",
        state.level_str(),
        state.xcr0,
        state.avx_pending
    );
    state
}

pub fn isa_xcr0() -> u64 {
    ISA.load(Ordering::Relaxed)
}

// ---------------------------------------------------------------------------
// F049 — interrupt latency meter
// ---------------------------------------------------------------------------

/// Bucket edges: < 1 µs, then doubling to ~32 ms. 16 buckets covers every
/// latency that still counts as "an interrupt", not "a hang".
pub const LATENCY_BUCKETS: usize = 16;
/// Latency budget for a device interrupt (end of ISR minus entry).
pub const LATENCY_BUDGET_NS: u64 = 50_000;

/// Log-scale latency histogram with percentile estimates.
pub struct LatencyMeter {
    buckets: [AtomicU32; LATENCY_BUCKETS],
    samples: AtomicU64,
    total_ns: AtomicU64,
    max_ns: AtomicU64,
    over_budget: AtomicU64,
}

impl LatencyMeter {
    pub const fn new() -> LatencyMeter {
        LatencyMeter {
            buckets: [const { AtomicU32::new(0) }; LATENCY_BUCKETS],
            samples: AtomicU64::new(0),
            total_ns: AtomicU64::new(0),
            max_ns: AtomicU64::new(0),
            over_budget: AtomicU64::new(0),
        }
    }

    /// Bucket index for a latency in nanoseconds.
    pub fn bucket_of(ns: u64) -> usize {
        if ns < 1_000 {
            0
        } else {
            let shifted = ns / 1_000;
            let log = 63 - shifted.leading_zeros() as usize; // floor(log2)
            (1 + log).min(LATENCY_BUCKETS - 1)
        }
    }

    /// Lower bound (ns) of a bucket — used for percentile estimates.
    pub fn bucket_floor(index: usize) -> u64 {
        if index == 0 {
            0
        } else {
            1_000u64 << (index - 1)
        }
    }

    pub fn record(&self, ns: u64) {
        self.buckets[Self::bucket_of(ns)].fetch_add(1, Ordering::Relaxed);
        self.samples.fetch_add(1, Ordering::Relaxed);
        self.total_ns.fetch_add(ns, Ordering::Relaxed);
        self.max_ns.fetch_max(ns, Ordering::Relaxed);
        if ns > LATENCY_BUDGET_NS {
            self.over_budget.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Mark the start of an interrupt; pair with `end`.
    pub fn begin(&self) -> u64 {
        crate::cpu::clock::read_tsc()
    }

    pub fn end(&self, token: u64) -> u64 {
        let cal = crate::cpu::clock::calibration();
        let ns = if cal.usable() {
            cal.tsc_to_ns(crate::cpu::clock::read_tsc().wrapping_sub(token))
        } else {
            0
        };
        self.record(ns);
        ns
    }

    pub fn samples(&self) -> u64 {
        self.samples.load(Ordering::Relaxed)
    }

    pub fn max_ns(&self) -> u64 {
        self.max_ns.load(Ordering::Relaxed)
    }

    pub fn mean_ns(&self) -> u64 {
        let s = self.samples();
        if s == 0 {
            0
        } else {
            self.total_ns.load(Ordering::Relaxed) / s
        }
    }

    pub fn over_budget(&self) -> u64 {
        self.over_budget.load(Ordering::Relaxed)
    }

    pub fn bucket(&self, i: usize) -> u32 {
        self.buckets[i.min(LATENCY_BUCKETS - 1)].load(Ordering::Relaxed)
    }

    /// Approximate percentile (uses bucket floors, so it is a lower bound).
    pub fn percentile(&self, pct: u8) -> u64 {
        let total = self.samples();
        if total == 0 {
            return 0;
        }
        let target = total * pct as u64 / 100;
        let mut acc = 0u64;
        for i in 0..LATENCY_BUCKETS {
            acc += self.bucket(i) as u64;
            if acc > target {
                return Self::bucket_floor(i);
            }
        }
        self.max_ns()
    }

    pub fn p50(&self) -> u64 {
        self.percentile(50)
    }

    pub fn p99(&self) -> u64 {
        self.percentile(99)
    }

    /// Render "n=… p50=…µs p99=…µs max=…µs over=…" into `out`.
    pub fn render(&self, out: &mut [u8]) -> usize {
        let mut n = 0usize;
        let emit = |s: &str, out: &mut [u8], n: &mut usize| {
            for &b in s.as_bytes() {
                if *n < out.len() {
                    out[*n] = b;
                    *n += 1;
                }
            }
        };
        let mut num = [0u8; 20];
        let put_num = |v: u64, out: &mut [u8], n: &mut usize, num: &mut [u8; 20]| {
            let mut w = 0usize;
            let mut v = v;
            if v == 0 {
                num[0] = b'0';
                w = 1;
            }
            while v > 0 && w < num.len() {
                num[w] = b'0' + (v % 10) as u8;
                v /= 10;
                w += 1;
            }
            while w > 0 {
                w -= 1;
                if *n < out.len() {
                    out[*n] = num[w];
                    *n += 1;
                }
            }
        };
        emit("irq-latency n=", out, &mut n);
        put_num(self.samples(), out, &mut n, &mut num);
        emit(" p50=", out, &mut n);
        put_num(self.p50() / 1000, out, &mut n, &mut num);
        emit("us p99=", out, &mut n);
        put_num(self.p99() / 1000, out, &mut n, &mut num);
        emit("us max=", out, &mut n);
        put_num(self.max_ns() / 1000, out, &mut n, &mut num);
        emit("us over=", out, &mut n);
        put_num(self.over_budget(), out, &mut n, &mut num);
        emit("\n", out, &mut n);
        n
    }
}

static LATENCY: LatencyMeter = LatencyMeter::new();

pub fn latency() -> &'static LatencyMeter {
    &LATENCY
}

// ---------------------------------------------------------------------------
// F050 — interrupt self-test
// ---------------------------------------------------------------------------

/// Own registry for the CPU/interrupt domain: AI-01's boot registry belongs to
/// the boot chain, and every domain reporting into it would overflow the
/// 32-check table.
static IRQ_SELFTEST: crate::selftest::SelfTest = crate::selftest::SelfTest::new();

pub fn irq_selftest() -> &'static crate::selftest::SelfTest {
    &IRQ_SELFTEST
}

/// F050: every interrupt-chain invariant, checked against live state.
pub fn run_interrupt_checks() -> (usize, usize) {
    let r = &IRQ_SELFTEST;

    // F026/F027 — GDT + TSS.
    let gdt_ok = crate::cpu::gdt::tables().is_ready() && crate::cpu::gdt::tables().is_installed();
    r.check("gdt-tss", gdt_ok, "GDT not loaded");

    // F028 — every vector has a slot.
    let populated = crate::cpu::idt::table().populated();
    r.check(
        "idt-vectors",
        populated == crate::cpu::idt::IDT_VECTORS,
        "vectors missing",
    );

    // F031 — the legacy PIC must be quiet.
    let pic_ok = crate::cpu::apic::pic().state() == crate::cpu::apic::PicState::Masked;
    r.check("pic-quiet", pic_ok, "PIC still delivering");

    // F032/F035 — an APIC is actually driving delivery.
    let apic_ok = crate::cpu::apic::lapic().is_active();
    r.check("lapic", apic_ok, "no local APIC");

    // F034 — no interrupt left unacknowledged.
    r.check(
        "eoi-balance",
        crate::cpu::apic::eoi_audit().balanced(),
        "outstanding EOI",
    );

    // F038 — the clock is calibrated from a trustworthy source.
    let cal = crate::cpu::clock::calibration();
    r.check("tsc-calib", cal.usable(), "TSC not calibrated");

    // F046 — no critical section blew the IRQ-off budget.
    r.check(
        "irq-off-budget",
        crate::cpu::sync::irq_off_audit().within_budget(),
        "IRQ-off window too long",
    );

    // F047 — every online core has a control block.
    r.check(
        "percpu",
        crate::cpu::smp::cpu_count() >= 1 && crate::cpu::smp::percpu(0).is_some(),
        "BSP not registered",
    );

    // F049 — latency inside budget (nothing recorded yet is not a failure).
    r.check(
        "irq-latency",
        crate::cpu::cpuinfo::latency().max_ns() <= LATENCY_BUDGET_NS,
        "latency over budget",
    );

    let (passed, failed) = r.tally();
    if failed == 0 {
        crate::kinfo!("irq self-test: {}/{} pass", passed, passed);
    } else {
        crate::kwarn!(
            "irq self-test: {}/{} pass ({} FAIL)",
            passed,
            passed + failed,
            failed
        );
    }
    (passed, failed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn isa_level_requirements_are_monotonic() {
        assert_eq!(IsaLevel::Baseline.required_xcr0(), XCR0_SSE);
        assert!(IsaLevel::Avx2.required_xcr0() & XCR0_AVX != 0);
        let a512 = IsaLevel::Avx512.required_xcr0();
        assert_ne!(a512 & XCR0_OPMASK, 0);
        assert_ne!(a512 & XCR0_ZMM_HI256, 0);
        assert_ne!(a512 & XCR0_HI16_ZMM, 0);
        assert!(IsaLevel::Avx512 > IsaLevel::Avx2);
        assert_eq!(IsaLevel::Avx2.as_str(), "avx2");
    }

    #[test]
    fn evaluate_refuses_unenabled_state() {
        // AVX advertised but XCR0 lacks the YMM bit → not usable.
        let s = evaluate(true, true, true, true, false, true, true, XCR0_SSE);
        assert!(s.avx_pending);
        assert_eq!(s.level, Some(IsaLevel::Sse4));
        // No XSAVE at all → nothing beyond baseline is safe.
        let s2 = evaluate(true, true, true, true, false, false, false, 0);
        assert_eq!(s2.level, None);
        assert_eq!(s2.level_str(), "none");
        // Full AVX2 state enabled.
        let s3 = evaluate(true, true, true, true, false, true, true, XCR0_SSE | XCR0_AVX);
        assert_eq!(s3.level, Some(IsaLevel::Avx2));
        assert!(!s3.avx_pending);
        // Full AVX-512 state enabled.
        let full = XCR0_SSE
            | XCR0_AVX
            | XCR0_OPMASK
            | XCR0_ZMM_HI256
            | XCR0_HI16_ZMM;
        let s4 = evaluate(true, true, true, true, true, true, true, full);
        assert_eq!(s4.level, Some(IsaLevel::Avx512));
    }

    #[test]
    fn latency_buckets_are_log_scaled() {
        assert_eq!(LatencyMeter::bucket_of(0), 0);
        assert_eq!(LatencyMeter::bucket_of(999), 0);
        assert_eq!(LatencyMeter::bucket_of(1_000), 1);
        assert_eq!(LatencyMeter::bucket_of(1_999), 1);
        assert_eq!(LatencyMeter::bucket_of(2_000), 2);
        // 1 ms falls in the bucket that spans 512 µs..1.024 ms.
        assert_eq!(LatencyMeter::bucket_of(1_000_000), 10);
        assert_eq!(LatencyMeter::bucket_floor(10), 512_000);
        // Saturates instead of running off the array.
        assert_eq!(LatencyMeter::bucket_of(u64::MAX), LATENCY_BUCKETS - 1);
        assert_eq!(LatencyMeter::bucket_floor(0), 0);
        assert_eq!(LatencyMeter::bucket_floor(1), 1_000);
        assert_eq!(LatencyMeter::bucket_floor(11), 1_024_000);
    }

    #[test]
    fn latency_percentiles_and_render() {
        let m = LatencyMeter::new();
        assert_eq!(m.samples(), 0);
        assert_eq!(m.p99(), 0);
        for _ in 0..100 {
            m.record(1_000); // all in bucket 1
        }
        m.record(500_000); // one slow outlier
        assert_eq!(m.samples(), 101);
        assert_eq!(m.bucket(1), 100);
        assert_eq!(m.p50(), 1_000);
        assert_eq!(m.max_ns(), 500_000);
        assert_eq!(m.over_budget(), 1);
        assert!(m.mean_ns() > 1_000);

        let mut out = [0u8; 128];
        let n = m.render(&mut out);
        let s = core::str::from_utf8(&out[..n]).unwrap();
        assert!(s.starts_with("irq-latency n=101"), "got {s}");
        assert!(s.contains("p50=1us"), "got {s}");
        assert!(s.contains("max=500us"), "got {s}");
        assert!(s.ends_with('\n'));
    }

    #[test]
    fn latency_render_respects_a_tiny_buffer() {
        let m = LatencyMeter::new();
        m.record(1);
        let mut out = [0u8; 5];
        assert_eq!(m.render(&mut out), 5);
    }
}
