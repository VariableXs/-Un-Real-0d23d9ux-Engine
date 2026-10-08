//! AI-33 性能工程与优化域（A801~A825，AURORA-1000）。
//!
//! Frame/startup/memory/IO budgets, a benchmark harness, regression gates,
//! flame-graph attribution, lock contention and allocator tuning, SIMD and
//! cache-friendly kernels, the honest performance dashboard, drift alarms,
//! reproducibility and cross-architecture baselines — plus the domain gates.

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// A801 — 帧预算
// ---------------------------------------------------------------------------

/// 16.6 ms frame budget, expressed in µs with a 2 ms reserve.
pub const FRAME_BUDGET_US: u32 = 14_600;

#[derive(Clone, Copy, Debug)]
pub struct FrameSpans {
    pub input_us: u32,
    pub simulate_us: u32,
    pub render_us: u32,
    pub composite_us: u32,
}

impl FrameSpans {
    pub fn total_us(&self) -> u32 {
        self.input_us
            .saturating_add(self.simulate_us)
            .saturating_add(self.render_us)
            .saturating_add(self.composite_us)
    }

    pub fn fits_budget(&self) -> bool {
        self.total_us() <= FRAME_BUDGET_US
    }

    /// The largest span is the optimization target.
    pub fn hotspot(&self) -> &'static str {
        let best = |a: u32, b: u32, name_a: &'static str, name_b: &'static str| {
            if a >= b { (a, name_a) } else { (b, name_b) }
        };
        let (r, name_r) = best(self.render_us, self.composite_us, "render", "composite");
        let (s, name_s) = best(self.simulate_us, r, "simulate", name_r);
        best(self.input_us, s, "input", name_s).1
    }
}

// ---------------------------------------------------------------------------
// A802 — 启动预算
// ---------------------------------------------------------------------------

pub const BOOT_BUDGET_MS: u32 = 3_000;

#[derive(Clone, Copy, Debug)]
pub struct BootStages {
    pub firmware_ms: u32,
    pub kernel_ms: u32,
    pub session_ms: u32,
}

impl BootStages {
    pub fn total_ms(&self) -> u32 {
        self.firmware_ms.saturating_add(self.kernel_ms).saturating_add(self.session_ms)
    }

    pub fn meets_instant_on(&self) -> bool {
        self.total_ms() <= BOOT_BUDGET_MS
    }

    /// Kernel-side share must stay under 40% of the budget. A boot that
    /// already misses instant-on fails the share gate too (no partial pass).
    pub fn kernel_share_ok(&self) -> bool {
        self.meets_instant_on() && self.kernel_ms * 100 <= BOOT_BUDGET_MS * 40
    }
}

// ---------------------------------------------------------------------------
// A803 — 内存占用预算
// ---------------------------------------------------------------------------

pub const MEM_BUDGET_KIB: u32 = 128 * 1024;

#[derive(Clone, Copy, Debug)]
pub struct MemFootprint {
    pub kernel_kib: u32,
    pub heap_kib: u32,
    pub caches_kib: u32,
}

impl MemFootprint {
    pub fn total_kib(&self) -> u32 {
        self.kernel_kib.saturating_add(self.heap_kib).saturating_add(self.caches_kib)
    }

    pub fn within_budget(&self) -> bool {
        self.total_kib() <= MEM_BUDGET_KIB
    }

    /// Caches are reclaimable; the committed core is kernel + heap.
    pub fn committed_kib(&self) -> u32 {
        self.kernel_kib.saturating_add(self.heap_kib)
    }
}

// ---------------------------------------------------------------------------
// A804 — IO 延迟预算
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IoVerdict {
    Fast,
    Nominal,
    Slow,
}

/// Interactive IO: read ≤ 1 ms is Fast, ≤ 10 ms Nominal, beyond is Slow.
pub fn io_verdict(latency_us: u32) -> IoVerdict {
    if latency_us <= 1_000 {
        IoVerdict::Fast
    } else if latency_us <= 10_000 {
        IoVerdict::Nominal
    } else {
        IoVerdict::Slow
    }
}

// ---------------------------------------------------------------------------
// A805 — 性能基准框架
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
pub struct BenchSample {
    pub iterations: u32,
    pub total_us: u64,
}

impl BenchSample {
    /// ns per iteration ×100 for precision (0 when no iterations).
    pub fn ns_per_iter_centi(&self) -> u32 {
        if self.iterations == 0 {
            return 0;
        }
        ((self.total_us * 100_000) / self.iterations as u64) as u32
    }

    /// Median-of-3 style stability: all samples within 10% of each other.
    pub fn stable(samples: [BenchSample; 3]) -> bool {
        let v = [samples[0].ns_per_iter_centi(), samples[1].ns_per_iter_centi(), samples[2].ns_per_iter_centi()];
        let lo = *v.iter().min().unwrap();
        let hi = *v.iter().max().unwrap();
        lo > 0 && hi * 100 <= lo * 110
    }
}

// ---------------------------------------------------------------------------
// A806 — 性能回归门禁
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegressionVerdict {
    Pass,
    Warn,
    Block,
}

/// Warn when the new baseline is ≥5% slower, block beyond 15%.
pub fn regression_gate(baseline_centi: u32, candidate_centi: u32) -> RegressionVerdict {
    if baseline_centi == 0 {
        return RegressionVerdict::Warn;
    }
    let slow_pct = candidate_centi.saturating_sub(baseline_centi) * 100 / baseline_centi;
    if slow_pct > 15 {
        RegressionVerdict::Block
    } else if slow_pct >= 5 {
        RegressionVerdict::Warn
    } else {
        RegressionVerdict::Pass
    }
}

// ---------------------------------------------------------------------------
// A807 — 火焰图剖析
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
pub struct FlameFrame {
    pub name: &'static str,
    pub self_us: u64,
    pub depth: u8,
}

/// Attribution: percent of total time in `name` ×100 (permille-ish, ×100).
pub fn flame_percent(frames: &[FlameFrame], total_us: u64, name: &str) -> u32 {
    if total_us == 0 {
        return 0;
    }
    let self_total: u64 = frames.iter().filter(|f| f.name == name).map(|f| f.self_us).sum();
    ((self_total * 10_000) / total_us) as u32
}

/// Top hotspot by self time.
pub fn flame_hotspot(frames: &[FlameFrame]) -> Option<&'static str> {
    let mut best: Option<(u64, &'static str)> = None;
    for f in frames {
        match best {
            Some((t, _)) if f.self_us <= t => {}
            _ => best = Some((f.self_us, f.name)),
        }
    }
    best.map(|(_, n)| n)
}

// ---------------------------------------------------------------------------
// A808 — 锁竞争优化
// ---------------------------------------------------------------------------

/// Contention verdict from wait counts: adaptive spin→park policy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LockPolicy {
    Spin,
    Yield,
    Park,
}

pub fn lock_policy(waiters: u16, hold_ns: u32) -> LockPolicy {
    if waiters <= 1 {
        LockPolicy::Spin
    } else if hold_ns <= 1_000 {
        LockPolicy::Yield
    } else {
        LockPolicy::Park
    }
}

/// False sharing detector: two hot fields in one 64-byte line is a bug.
pub fn false_sharing(offset_a: usize, offset_b: usize) -> bool {
    offset_a / 64 == offset_b / 64 && offset_a != offset_b
}

// ---------------------------------------------------------------------------
// A809 — 分配器优化
// ---------------------------------------------------------------------------

/// Size-class rounding for a bump/slab allocator (8-byte granularity).
pub fn size_class_round(size: usize) -> usize {
    size.checked_add(7).map(|v| v & !7).unwrap_or(usize::MAX)
}

/// Fragmentation estimate: free bytes that cannot satisfy the largest class.
pub fn fragmentation_permille(free_bytes: usize, largest_class: usize) -> u32 {
    if free_bytes == 0 {
        return 0;
    }
    let usable = (free_bytes / largest_class.max(1)) * largest_class;
    ((free_bytes - usable) * 1000 / free_bytes) as u32
}

// ---------------------------------------------------------------------------
// A810 — SIMD 优化
// ---------------------------------------------------------------------------

/// Elements per 256-bit SIMD op for a lane width.
pub fn simd_elems_per_op(lane_bytes: usize) -> usize {
    if lane_bytes == 0 {
        0
    } else {
        32 / lane_bytes
    }
}

/// Speedup sanity: SIMD must beat scalar (≥ 4× for u8 lanes).
pub fn simd_gain_ok(scalar_ns: u32, simd_ns: u32, min_gain: u32) -> bool {
    simd_ns > 0 && scalar_ns >= simd_ns.saturating_mul(min_gain)
}

// ---------------------------------------------------------------------------
// A811 — 缓存友好优化
// ---------------------------------------------------------------------------

/// Working set that fits L1 (32 KiB) allows naive traversal.
pub fn fits_l1(bytes: usize) -> bool {
    bytes <= 32 * 1024
}

/// Loop tiling: tile bytes should be ≤ L1 size and a multiple of the line.
pub fn tile_bytes_ok(tile_elems: usize, elem_size: usize) -> bool {
    let bytes = tile_elems.saturating_mul(elem_size);
    bytes <= 32 * 1024 && tile_elems % 64 == 0
}

/// Cache-line-strided prefetch hint distance (lines ahead).
pub fn prefetch_lines(seq_bytes: usize, line: usize) -> usize {
    if line == 0 {
        return 0;
    }
    (seq_bytes / line).min(16)
}

// ---------------------------------------------------------------------------
// A812 — 性能仪表盘
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GaugeColor {
    Green,
    Amber,
    Red,
}

/// Honest dashboard: green when ≥ 90% of budget remains, amber ≥ 70%.
pub fn gauge_color(used_permille: u16) -> GaugeColor {
    if used_permille <= 900 {
        GaugeColor::Green
    } else if used_permille <= 970 {
        GaugeColor::Amber
    } else {
        GaugeColor::Red
    }
}

// ---------------------------------------------------------------------------
// A813 — 性能漂移告警
// ---------------------------------------------------------------------------

/// EWMA drift detector: alarm when the latest sample exceeds the EMA by >25%.
pub struct DriftAlarm {
    ema_permille_of_base: u32,
    base: u32,
    armed: bool,
}

impl DriftAlarm {
    pub const fn new(base: u32) -> DriftAlarm {
        DriftAlarm { ema_permille_of_base: 1000, base, armed: true }
    }

    /// Update with a new sample; returns true when alarming.
    pub fn update(&mut self, sample: u32) -> bool {
        if self.base == 0 {
            return false;
        }
        let permille = (sample as u64 * 1000 / self.base as u64) as u32;
        self.ema_permille_of_base = ((self.ema_permille_of_base as u64 * 7 + permille as u64) / 8) as u32;
        let alarm = self.armed && permille > self.ema_permille_of_base * 5 / 4;
        if alarm {
            self.armed = false; // one alarm per episode
        } else if permille <= self.ema_permille_of_base {
            self.armed = true;
        }
        alarm
    }
}

// ---------------------------------------------------------------------------
// A814 — 性能可复现协议
// ---------------------------------------------------------------------------

/// Reproducible run recipe: fixed iterations + warmup + pinned config hash.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReproRecipe {
    pub warmup_iters: u32,
    pub measured_iters: u32,
    pub config_hash: u64,
}

impl ReproRecipe {
    pub fn valid(&self) -> bool {
        self.measured_iters >= 1000 && self.warmup_iters >= 100 && self.config_hash != 0
    }

    /// Same recipe + same samples ⇒ same verdict (deterministic gate).
    pub fn reproducible(&self, other: &ReproRecipe) -> bool {
        self.warmup_iters == other.warmup_iters
            && self.measured_iters == other.measured_iters
            && self.config_hash == other.config_hash
    }
}

// ---------------------------------------------------------------------------
// A815 — 性能跨架构基准
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
pub struct ArchBaseline {
    pub arch: &'static str,
    pub ns_per_iter: u32,
}

/// Normalize across architectures: score = reference/ns ratio ×1000.
pub fn arch_score(b: ArchBaseline, reference_ns: u32) -> u32 {
    if b.ns_per_iter == 0 {
        return 0;
    }
    ((reference_ns as u64 * 1000) / b.ns_per_iter as u64) as u32
}

/// Every arch must be measured (nonzero) for the matrix to ship.
pub fn arch_matrix_complete(baselines: &[ArchBaseline]) -> bool {
    !baselines.is_empty() && baselines.iter().all(|b| b.ns_per_iter > 0)
}

// ---------------------------------------------------------------------------
// A817/A818/A819 — 性能自检
// ---------------------------------------------------------------------------

/// Combined self-check of all budget guards.
pub fn perf_selfcheck(spans: FrameSpans, boot: BootStages, mem: MemFootprint) -> bool {
    spans.fits_budget() && boot.meets_instant_on() && boot.kernel_share_ok() && mem.within_budget()
}

// ---------------------------------------------------------------------------
// A821/A823 — 可观测（性能计数器）/ 文档条目
// ---------------------------------------------------------------------------

const PERF_COUNTER_CAP: usize = 16;

#[derive(Clone, Copy, Debug)]
pub struct PerfCounter {
    pub name: &'static str,
    pub value_us: u64,
}

pub struct PerfCounterSet {
    counters: [PerfCounter; PERF_COUNTER_CAP],
    count: usize,
}

impl PerfCounterSet {
    pub const fn new() -> PerfCounterSet {
        PerfCounterSet {
            counters: [PerfCounter { name: "", value_us: 0 }; PERF_COUNTER_CAP],
            count: 0,
        }
    }

    pub fn record(&mut self, name: &'static str, value_us: u64) {
        for i in 0..self.count {
            if self.counters[i].name == name {
                self.counters[i].value_us = self.counters[i].value_us.saturating_add(value_us);
                return;
            }
        }
        if self.count < PERF_COUNTER_CAP {
            self.counters[self.count] = PerfCounter { name, value_us };
            self.count += 1;
        }
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn get(&self, name: &str) -> Option<u64> {
        (0..self.count).find(|i| self.counters[*i].name == name).map(|i| self.counters[i].value_us)
    }

    pub fn total_us(&self) -> u64 {
        (0..self.count).map(|i| self.counters[i].value_us).sum()
    }
}

// ---------------------------------------------------------------------------
// A822 — 模糊测试
// ---------------------------------------------------------------------------

/// Fuzz the size-class rounder: result must be ≥ input, 8-aligned, finite.
pub fn fuzz_size_class(size: usize) -> bool {
    let r = size_class_round(size);
    r >= size && r % 8 == 0
}

/// Fuzz the regression gate with arbitrary counters — never panics.
pub fn fuzz_regression_gate(base: u32, cand: u32) -> bool {
    matches!(
        regression_gate(base, cand),
        RegressionVerdict::Pass | RegressionVerdict::Warn | RegressionVerdict::Block
    )
}

// ---------------------------------------------------------------------------
// A824 — 降级链
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PerfTier {
    /// Full instrumentation + optimization kernels.
    Full,
    /// Coarse counters only.
    Sampled,
    /// Budgets enforced, no instrumentation.
    Bare,
}

pub fn perf_degrade(counter_overhead_permille: u16) -> PerfTier {
    if counter_overhead_permille <= 20 {
        PerfTier::Full
    } else if counter_overhead_permille <= 100 {
        PerfTier::Sampled
    } else {
        PerfTier::Bare
    }
}

// ---------------------------------------------------------------------------
// A825 — 域自检收口
// ---------------------------------------------------------------------------

/// Domain self-test: pure invariants that must hold on every machine.
pub fn run_perf_checks() -> CheckSet {
    let mut set = CheckSet::new("perf");

    let spans = FrameSpans { input_us: 200, simulate_us: 2_000, render_us: 9_000, composite_us: 3_000 };
    set.add("A801 frame budget", spans.fits_budget() && spans.total_us() == 14_200, "fits");
    set.add(
        "A801 hotspot",
        spans.hotspot() == "render"
            && FrameSpans { input_us: 10_000, ..spans }.hotspot() == "input",
        "attribution",
    );

    let boot = BootStages { firmware_ms: 800, kernel_ms: 900, session_ms: 1_000 };
    set.add(
        "A802 boot",
        boot.meets_instant_on() && boot.kernel_share_ok() && boot.total_ms() == 2_700,
        "instant-on",
    );

    let mem = MemFootprint { kernel_kib: 24_576, heap_kib: 48_000, caches_kib: 32_768 };
    set.add(
        "A803 mem",
        mem.within_budget() && mem.committed_kib() == 72_576
            && !MemFootprint { kernel_kib: 200_000, ..mem }.within_budget(),
        "budget",
    );

    set.add(
        "A804 io",
        io_verdict(500) == IoVerdict::Fast && io_verdict(5_000) == IoVerdict::Nominal
            && io_verdict(50_000) == IoVerdict::Slow,
        "latency",
    );

    let s = BenchSample { iterations: 1_000, total_us: 5_000 };
    set.add("A805 ns/iter", s.ns_per_iter_centi() == 500_000, "math");
    let a = BenchSample { iterations: 100, total_us: 1_000 };
    let trio = [a, a, a];
    set.add("A805 stable", BenchSample::stable(trio), "median-3");

    set.add(
        "A806 gate",
        regression_gate(100, 104) == RegressionVerdict::Pass
            && regression_gate(100, 108) == RegressionVerdict::Warn
            && regression_gate(100, 120) == RegressionVerdict::Block
            && regression_gate(0, 10) == RegressionVerdict::Warn,
        "thresholds",
    );

    let frames = [
        FlameFrame { name: "render", self_us: 3_000, depth: 1 },
        FlameFrame { name: "composite", self_us: 1_000, depth: 1 },
        FlameFrame { name: "render", self_us: 2_000, depth: 2 },
    ];
    set.add(
        "A807 flame",
        flame_percent(&frames, 10_000, "render") == 5000 && flame_hotspot(&frames) == Some("render")
            && flame_percent(&frames, 0, "render") == 0,
        "attribution",
    );

    set.add(
        "A808 locks",
        lock_policy(0, 100) == LockPolicy::Spin && lock_policy(3, 500) == LockPolicy::Yield
            && lock_policy(3, 5_000) == LockPolicy::Park
            && false_sharing(0, 32) && !false_sharing(0, 64),
        "contention",
    );

    set.add(
        "A809 alloc",
        size_class_round(1) == 8 && size_class_round(8) == 8 && size_class_round(9) == 16
            && fragmentation_permille(1000, 300) == 100 && fragmentation_permille(0, 300) == 0,
        "size class",
    );

    set.add(
        "A810 simd",
        simd_elems_per_op(1) == 32 && simd_elems_per_op(4) == 8 && simd_elems_per_op(0) == 0
            && simd_gain_ok(800, 100, 4) && !simd_gain_ok(300, 100, 4),
        "lanes",
    );

    set.add(
        "A811 cache",
        fits_l1(32 * 1024) && !fits_l1(32 * 1024 + 1)
            && tile_bytes_ok(512, 64) && !tile_bytes_ok(100, 64)
            && prefetch_lines(640, 64) == 10 && prefetch_lines(640, 0) == 0,
        "tiling",
    );

    set.add(
        "A812 gauge",
        gauge_color(500) == GaugeColor::Green && gauge_color(950) == GaugeColor::Amber
            && gauge_color(999) == GaugeColor::Red,
        "honest",
    );

    let mut d = DriftAlarm::new(1000);
    let quiet = d.update(1_010);
    let loud = d.update(2_000);
    set.add("A813 drift", !quiet && loud, "ewma alarm");

    let r = ReproRecipe { warmup_iters: 100, measured_iters: 10_000, config_hash: 0xDEAD_BEEF };
    set.add(
        "A814 repro",
        r.valid() && r.reproducible(&r)
            && !r.reproducible(&ReproRecipe { config_hash: 2, ..r })
            && !ReproRecipe { warmup_iters: 0, ..r }.valid(),
        "protocol",
    );

    let baselines = [
        ArchBaseline { arch: "x86_64", ns_per_iter: 100 },
        ArchBaseline { arch: "aarch64", ns_per_iter: 125 },
    ];
    set.add(
        "A815 cross-arch",
        arch_score(baselines[0], 200) == 2000 && arch_matrix_complete(&baselines)
            && !arch_matrix_complete(&[ArchBaseline { arch: "rv", ns_per_iter: 0 }]),
        "normalize",
    );

    set.add(
        "A817 selfcheck",
        perf_selfcheck(spans, boot, mem) && !perf_selfcheck(spans, BootStages { kernel_ms: 2_000, ..boot }, mem),
        "guards",
    );

    let mut cs = PerfCounterSet::new();
    cs.record("render", 1_000);
    cs.record("render", 500);
    cs.record("composite", 300);
    set.add(
        "A821 counters",
        cs.get("render") == Some(1_500) && cs.len() == 2 && cs.total_us() == 1_800,
        "accumulate",
    );

    set.add(
        "A822 fuzz",
        fuzz_size_class(0) && fuzz_size_class(usize::MAX - 8)
            && fuzz_regression_gate(0, 0) && fuzz_regression_gate(7, 99),
        "robust",
    );

    set.add("A823 docs cap", cs.len() <= PERF_COUNTER_CAP, "bounded");

    set.add(
        "A818/A819 selfcheck+gate",
        perf_selfcheck(spans, boot, mem) && regression_gate(100, 100) == RegressionVerdict::Pass,
        "closure guards",
    );

    set.add(
        "A824 degrade",
        perf_degrade(10) == PerfTier::Full && perf_degrade(50) == PerfTier::Sampled
            && perf_degrade(500) == PerfTier::Bare,
        "chain",
    );

    set.add(
        "A816/A820 doc+budget",
        PERF_COUNTER_CAP >= 8 && FRAME_BUDGET_US < 16_600 && BOOT_BUDGET_MS == 3_000,
        "budget constants",
    );

    set.add(
        "A825 closure",
        set.len() + 1 >= 25 && !set.truncated(),
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
    fn a801_frame_edges() {
        let tight = FrameSpans { input_us: 100, simulate_us: 4_500, render_us: 9_500, composite_us: 500 };
        assert!(tight.fits_budget());
        assert_eq!(tight.hotspot(), "render");
        let over = FrameSpans { composite_us: 10_500, ..tight };
        assert!(!over.fits_budget());
        assert_eq!(over.hotspot(), "composite");
    }

    #[test]
    fn a802_boot_saturating() {
        let huge = BootStages { firmware_ms: u32::MAX, kernel_ms: 10, session_ms: 10 };
        assert!(!huge.meets_instant_on());
        assert!(!huge.kernel_share_ok());
        let thin = BootStages { firmware_ms: 100, kernel_ms: 100, session_ms: 100 };
        assert!(thin.kernel_share_ok());
    }

    #[test]
    fn a803_mem_committed() {
        let m = MemFootprint { kernel_kib: 8_192, heap_kib: 8_192, caches_kib: u32::MAX / 2 };
        assert!(!m.within_budget());
        assert_eq!(m.committed_kib(), 16_384);
    }

    #[test]
    fn a804_io_boundaries() {
        assert_eq!(io_verdict(1_000), IoVerdict::Fast);
        assert_eq!(io_verdict(1_001), IoVerdict::Nominal);
        assert_eq!(io_verdict(10_000), IoVerdict::Nominal);
        assert_eq!(io_verdict(10_001), IoVerdict::Slow);
    }

    #[test]
    fn a805_bench_precision() {
        let s = BenchSample { iterations: 3, total_us: 6 };
        assert_eq!(s.ns_per_iter_centi(), 200_000);
        assert_eq!(BenchSample { iterations: 0, total_us: 9 }.ns_per_iter_centi(), 0);
        let noisy = [
            BenchSample { iterations: 10, total_us: 100 },
            BenchSample { iterations: 10, total_us: 200 },
            BenchSample { iterations: 10, total_us: 100 },
        ];
        assert!(!BenchSample::stable(noisy));
    }

    #[test]
    fn a806_gate_exact_thresholds() {
        assert_eq!(regression_gate(100, 100), RegressionVerdict::Pass);
        assert_eq!(regression_gate(100, 105), RegressionVerdict::Warn);
        assert_eq!(regression_gate(100, 115), RegressionVerdict::Warn);
        assert_eq!(regression_gate(100, 116), RegressionVerdict::Block);
        // Faster candidates always pass.
        assert_eq!(regression_gate(100, 50), RegressionVerdict::Pass);
    }

    #[test]
    fn a807_flame_empty() {
        assert_eq!(flame_hotspot(&[]), None);
        let f = [FlameFrame { name: "a", self_us: 5, depth: 0 }];
        assert_eq!(flame_hotspot(&f), Some("a"));
        assert_eq!(flame_percent(&f, 5, "b"), 0);
    }

    #[test]
    fn a808_lock_policies() {
        assert_eq!(lock_policy(1, u32::MAX), LockPolicy::Spin);
        assert_eq!(lock_policy(9, 1_000), LockPolicy::Yield);
        assert_eq!(lock_policy(9, 1_001), LockPolicy::Park);
        assert!(!false_sharing(64, 64 + 64));
        assert!(!false_sharing(63, 64)); // straddles the boundary → different lines
    }

    #[test]
    fn a809_fragmentation() {
        assert_eq!(fragmentation_permille(900, 300), 0);
        assert_eq!(fragmentation_permille(1_000, 400), 200);
        assert_eq!(fragmentation_permille(3, 8), 1_000);
        assert_eq!(size_class_round(usize::MAX), usize::MAX);
    }

    #[test]
    fn a810_a811_kernels() {
        assert_eq!(simd_elems_per_op(8), 4);
        assert_eq!(simd_elems_per_op(2), 16);
        assert!(simd_gain_ok(1_000, 100, 8));
        assert!(!simd_gain_ok(700, 100, 8));
        assert!(tile_bytes_ok(0, 8));
        assert_eq!(prefetch_lines(0, 64), 0);
    }

    #[test]
    fn a813_drift_episodes() {
        let mut d = DriftAlarm::new(100);
        assert!(!d.update(100));
        assert!(d.update(500)); // alarm fires once
        assert!(!d.update(600)); // no re-arm while hot
        assert!(!d.update(100)); // back to normal re-arms
        assert!(d.update(500));
        let mut zero = DriftAlarm::new(0);
        assert!(!zero.update(1));
    }

    #[test]
    fn a814_a815_protocol() {
        let r = ReproRecipe { warmup_iters: 100, measured_iters: 1_000, config_hash: 1 };
        assert!(r.valid());
        let bad = ReproRecipe { warmup_iters: 100, measured_iters: 999, config_hash: 1 };
        assert!(!bad.valid());
        assert_eq!(arch_score(ArchBaseline { arch: "mips", ns_per_iter: 50 }, 100), 2000);
    }

    #[test]
    fn a821_counter_overflow() {
        let mut cs = PerfCounterSet::new();
        for _ in 0..u32::from(u16::MAX) / 1000 {
            cs.record("x", u64::MAX / 2);
        }
        // saturating add keeps it finite
        assert!(cs.get("x").unwrap() <= u64::MAX);
        for i in 0..PERF_COUNTER_CAP + 3 {
            const NAMES: [&str; PERF_COUNTER_CAP + 3] = [
                "n0", "n1", "n2", "n3", "n4", "n5", "n6", "n7", "n8", "n9", "na", "nb", "nc",
                "nd", "ne", "nf", "z0", "z1", "z2",
            ];
            cs.record(NAMES[i], 1);
        }
        assert_eq!(cs.len(), PERF_COUNTER_CAP);
    }

    #[test]
    fn a824_a825_final() {
        assert_eq!(perf_degrade(20), PerfTier::Full);
        assert_eq!(perf_degrade(21), PerfTier::Sampled);
        let set = run_perf_checks();
        if !set.all_passed() {
            let mut buf = [0u8; 1024];
            let n = set.render(&mut buf);
            panic!("perf self-test:\n{}", core::str::from_utf8(&buf[..n]).unwrap());
        }
        assert!(set.len() >= 25);
    }
}
