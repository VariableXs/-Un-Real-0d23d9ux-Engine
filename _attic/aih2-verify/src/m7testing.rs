//! m7testing — VARIX-M700 AI-25 可测试性域 (F601~F625)
//!
//! 测试宪法、注册台、模拟设备、确定性调度、覆盖率、故障注入、
//! 沙盒、时间加速、金样本、编排台、噪声猎手、统计分账、事件流、
//! 预算官、文档生成、压力发生器、断言库、回归走廊、健康分、
//! 自描述导出、联测剧本、优先级谱、降级律、一日补丁、年报。
//!
//! 硬约束：no_std / 无 alloc / 无浮点。

use crate::checks::CheckSet;

// ===========================================================================
// F601 — 测试宪法：分层
// ===========================================================================

#[derive(Clone, Copy, PartialEq, PartialOrd)]
pub enum TestTier {
    Unit,
    SelfCheck,
    Fuzz,
    Smoke,
}

pub fn tier_order(t: TestTier) -> u8 {
    match t {
        TestTier::Unit => 0,
        TestTier::SelfCheck => 1,
        TestTier::Fuzz => 2,
        TestTier::Smoke => 3,
    }
}

// ===========================================================================
// F602 — 测试注册台
// ===========================================================================

pub const TESTREG_CAP: usize = 64;

#[derive(Clone, Copy)]
pub struct TestReg {
    pub name: &'static str,
    pub tier: TestTier,
    pub enabled: bool,
}

pub fn testreg_register(table: &mut [Option<TestReg>; TESTREG_CAP], len: &mut usize, t: TestReg) -> bool {
    if *len >= TESTREG_CAP || t.name.is_empty() {
        return false;
    }
    for i in 0..*len {
        if table[i].map(|r| r.name) == Some(t.name) {
            return false; // 去重
        }
    }
    table[*len] = Some(t);
    *len += 1;
    true
}

pub fn testreg_filter(table: &[Option<TestReg>; TESTREG_CAP], len: usize, tier: TestTier) -> usize {
    let len = len.min(TESTREG_CAP);
    table[..len].iter().flatten().filter(|r| r.tier == tier && r.enabled).count()
}

// ===========================================================================
// F603 — 模拟设备军火库
// ===========================================================================

pub const MOCK_DEVICES: [&str; 5] = ["nvme-disk", "e1000-net", "virtio-gpu", "uart", "rtc"];

#[derive(Clone, Copy)]
pub struct MockDevice {
    pub kind: u8,
    pub latency_us: u32,
    pub fail_rate_permille: u16,
}

pub fn mock_device_sane(d: &MockDevice) -> bool {
    (d.kind as usize) < MOCK_DEVICES.len() && d.fail_rate_permille <= 1000
}

// ===========================================================================
// F604 — 确定性调度测试：固定种子
// ===========================================================================

pub struct DetPrng {
    pub state: u64,
}

impl DetPrng {
    pub fn new(seed: u64) -> Self {
        DetPrng { state: seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407) | 1 }
    }
    pub fn next(&mut self) -> u64 {
        self.state = self.state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.state >> 16
    }
}

pub fn deterministic_sequence(seed: u64, n: usize) -> u64 {
    let mut p = DetPrng::new(seed);
    let mut acc = 0u64;
    for _ in 0..n {
        acc = acc.rotate_left(8) ^ p.next();
    }
    acc
}

// ===========================================================================
// F605 — 测试覆盖率官
// ===========================================================================

#[derive(Clone, Copy)]
pub struct Coverage {
    pub lines_total: u32,
    pub lines_hit: u32,
    pub branches_total: u32,
    pub branches_hit: u32,
}

impl Coverage {
    pub fn line_permille(&self) -> u16 {
        if self.lines_total == 0 {
            return 0;
        }
        ((self.lines_hit as u64 * 1000) / self.lines_total as u64).min(1000) as u16
    }
    pub fn branch_permille(&self) -> u16 {
        if self.branches_total == 0 {
            return 0;
        }
        ((self.branches_hit as u64 * 1000) / self.branches_total as u64).min(1000) as u16
    }
}

// ===========================================================================
// F606 — 故障注入 API
// ===========================================================================

#[derive(Clone, Copy, PartialEq)]
pub enum FaultKind {
    Errno,
    Delay,
    Drop,
    Corrupt,
}

pub struct FaultInjector {
    pub kind: FaultKind,
    pub every_n: u32,
    pub count: u32,
}

impl FaultInjector {
    /// 第 n 次调用触发注入。
    pub fn should_inject(&mut self) -> bool {
        self.count += 1;
        self.every_n > 0 && self.count % self.every_n == 0
    }
}

// ===========================================================================
// F607 — 测试沙盒舱
// ===========================================================================

pub const SANDBOX_MEM_KB: u32 = 4096;

pub fn sandbox_admits(mem_kb: u32, io_ops: u32) -> bool {
    mem_kb <= SANDBOX_MEM_KB && io_ops <= 10_000
}

// ===========================================================================
// F608 — 测试时间加速器
// ===========================================================================

pub struct VTime {
    pub now_ms: u64,
    pub scale: u32, // 每次推进 scale ms
}

impl VTime {
    pub fn tick(&mut self) -> u64 {
        self.now_ms += self.scale.max(1) as u64;
        self.now_ms
    }
    pub fn fast_forward(&mut self, ticks: u32) -> u64 {
        for _ in 0..ticks {
            self.tick();
        }
        self.now_ms
    }
}

// ===========================================================================
// F609 — 测试金样本管理
// ===========================================================================

pub fn golden_diff(actual: &[u8], expected: &[u8]) -> usize {
    actual.iter().zip(expected.iter()).filter(|(a, b)| a != b).count()
        + actual.len().abs_diff(expected.len())
}

pub fn golden_acceptable(actual: &[u8], expected: &[u8], max_diff: usize) -> bool {
    golden_diff(actual, expected) <= max_diff
}

// ===========================================================================
// F610 — 测试编排台
// ===========================================================================

#[derive(Clone, Copy, PartialEq)]
pub enum SuiteMode {
    Serial,
    Parallel,
}

pub fn orchestrate(names: &[&str], mode: SuiteMode) -> usize {
    let _ = mode;
    names.len()
}

pub fn orchestrate_order_sane(first: u32, second: u32) -> bool {
    first < second
}

// ===========================================================================
// F611 — 测试噪声猎手：flaky 检测
// ===========================================================================

/// 同一用例在 N 轮中结果不一致 = flaky。
pub fn flaky_detected(results: &[bool], n: usize) -> bool {
    let n = n.min(results.len());
    if n == 0 {
        return false;
    }
    let first = results[0];
    results[..n].iter().any(|&r| r != first)
}

pub fn flaky_retry_advised(results: &[bool], n: usize) -> bool {
    flaky_detected(results, n)
}

// ===========================================================================
// F612 — 测试统计分账
// ===========================================================================

#[derive(Clone, Copy)]
pub struct SuiteStat {
    pub name: &'static str,
    pub passed: u32,
    pub failed: u32,
    pub duration_ms: u32,
}

impl SuiteStat {
    pub fn pass_permille(&self) -> u16 {
        let total = self.passed + self.failed;
        if total == 0 {
            return 0;
        }
        ((self.passed as u64 * 1000) / total as u64) as u16
    }
}

pub fn slowest_suite(stats: &[SuiteStat], n: usize) -> Option<usize> {
    let n = n.min(stats.len());
    let mut best: Option<usize> = None;
    for i in 0..n {
        match best {
            None => best = Some(i),
            Some(b) if stats[i].duration_ms > stats[b].duration_ms => best = Some(i),
            _ => {}
        }
    }
    best
}

// ===========================================================================
// F613 — 测试事件流
// ===========================================================================

#[derive(Clone, Copy, PartialEq)]
pub enum TestEvent {
    Started,
    Passed,
    Failed,
    Skipped,
    Flaky,
}

pub fn test_event_loggable(e: TestEvent) -> bool {
    matches!(e, TestEvent::Started | TestEvent::Passed | TestEvent::Failed | TestEvent::Skipped | TestEvent::Flaky)
}

// ===========================================================================
// F614 — 测试预算官
// ===========================================================================

pub const SUITE_BUDGET_MS: u32 = 60_000;

pub fn suite_budget_ok(duration_ms: u32) -> bool {
    duration_ms <= SUITE_BUDGET_MS
}

// ===========================================================================
// F615 — 测试文档生成器
// ===========================================================================

pub const TEST_DOC_FIELDS: [&str; 5] = ["name", "tier", "inputs", "expected", "owner"];

pub fn test_doc_complete(marks: u8) -> bool {
    marks as u32 == (1u32 << TEST_DOC_FIELDS.len()) - 1
}

// ===========================================================================
// F616 — 随机压力发生器
// ===========================================================================

/// 生成受约束的随机操作序列长度。
pub fn stress_op_count(prng: &mut DetPrng, min_ops: u32, max_ops: u32) -> u32 {
    let range = max_ops.saturating_sub(min_ops) + 1;
    min_ops + (prng.next() % range as u64) as u32
}

// ===========================================================================
// F617 — 断言辅助库
// ===========================================================================

pub fn assert_in_range(v: u32, lo: u32, hi: u32) -> bool {
    (lo..=hi).contains(&v)
}

pub fn assert_sorted(data: &[u32], n: usize) -> bool {
    let n = n.min(data.len());
    (1..n).all(|i| data[i - 1] <= data[i])
}

pub fn invariant_no_overlap(a: (u32, u32), b: (u32, u32)) -> bool {
    a.1 <= b.0 || b.1 <= a.0
}

// ===========================================================================
// F618 — 测试回归走廊
// ===========================================================================

pub const TEST_CORRIDOR_CASES: [&str; 5] =
    ["unit-all", "selfcheck-all", "fuzz-10min", "smoke-boot", "coverage-gate"];

pub fn test_corridor_pass(results: &[bool; 5]) -> bool {
    results.iter().all(|&r| r)
}

// ===========================================================================
// F619 — 测试健康分
// ===========================================================================

pub struct TestHealth {
    pub runs: u32,
    pub flakes: u32,
    pub infra_failures: u32, // 测试设施自身故障
}

impl TestHealth {
    pub fn flake_permille(&self) -> u16 {
        if self.runs == 0 {
            return 0;
        }
        ((self.flakes as u64 * 1000) / self.runs as u64).min(1000) as u16
    }
    pub fn grade(&self) -> u8 {
        if self.flake_permille() <= 5 && self.infra_failures == 0 {
            0
        } else if self.flake_permille() <= 30 {
            1
        } else {
            2
        }
    }
}

// ===========================================================================
// F620 — 测试自描述导出
// ===========================================================================

pub struct TestExport {
    pub total: u32,
    pub by_tier: [u32; 4],
}

pub fn test_export_valid(e: &TestExport) -> bool {
    e.by_tier.iter().sum::<u32>() == e.total
}

// ===========================================================================
// F621 — 跨域联测剧本
// ===========================================================================

pub const CROSS_DOMAIN_PLAYS: [&str; 5] =
    ["sched-mem", "fs-vm", "net-syscall", "gfx-input", "boot-power"];

pub fn cross_domain_known(name: &str) -> bool {
    CROSS_DOMAIN_PLAYS.iter().any(|p| *p == name)
}

// ===========================================================================
// F622 — 测试优先级谱：按风险分级
// ===========================================================================

#[derive(Clone, Copy, PartialEq, PartialOrd)]
pub enum Risk {
    Low,
    Medium,
    High,
    Critical,
}

pub fn risk_rank(r: Risk) -> u8 {
    match r {
        Risk::Low => 0,
        Risk::Medium => 1,
        Risk::High => 2,
        Risk::Critical => 3,
    }
}

pub fn run_first(a: Risk, b: Risk) -> Risk {
    if risk_rank(a) >= risk_rank(b) {
        a
    } else {
        b
    }
}

// ===========================================================================
// F623 — 测试降级律
// ===========================================================================

#[derive(Clone, Copy, PartialEq)]
pub enum InfraState {
    Healthy,
    Degraded,
    Down,
}

pub fn test_downgrade(infra: InfraState, elapsed_ms: u32) -> u8 {
    // 返回可运行的最低层级（0=全量 1=去 fuzz 2=仅自检）
    match infra {
        InfraState::Healthy if elapsed_ms <= SUITE_BUDGET_MS => 0,
        InfraState::Healthy | InfraState::Degraded => 1,
        InfraState::Down => 2,
    }
}

// ===========================================================================
// F624 — 一日补丁演练台
// ===========================================================================

pub const ONE_DAY_PATCH_STEPS: [&str; 6] =
    ["clone", "build", "test", "fuzz", "review", "merge"];

pub fn one_day_patch_complete(marks: u8) -> bool {
    marks as u32 == (1u32 << ONE_DAY_PATCH_STEPS.len()) - 1
}

// ===========================================================================
// F625 — 测试域年报
// ===========================================================================

pub struct TestYearbook {
    pub total_runs: u64,
    pub failures: u64,
    pub flakes: u64,
    pub coverage_line_permille: u16,
}

impl TestYearbook {
    pub fn fail_permille(&self) -> u16 {
        if self.total_runs == 0 {
            return 0;
        }
        ((self.failures as u64 * 1000) / self.total_runs as u64).min(1000) as u16
    }
    pub fn coverage_gate(&self) -> bool {
        self.coverage_line_permille >= 700
    }
}

// ===========================================================================
// 域自检
// ===========================================================================

pub fn run_m7testing_checks() -> CheckSet {
    let mut set = CheckSet::new("m7testing");

    // F601 宪法
    set.add(
        "F601 tiers",
        tier_order(TestTier::Unit) < tier_order(TestTier::Fuzz)
            && tier_order(TestTier::Fuzz) < tier_order(TestTier::Smoke),
        "4 tiers",
    );

    // F602 注册台
    let mut table = [const { None }; TESTREG_CAP];
    let mut n = 0usize;
    let t1 = TestReg { name: "f501-core", tier: TestTier::Unit, enabled: true };
    set.add(
        "F602 registry",
        testreg_register(&mut table, &mut n, t1)
            && !testreg_register(&mut table, &mut n, t1)
            && testreg_filter(&table, n, TestTier::Unit) == 1,
        "dedup + filter",
    );

    // F603 模拟设备
    let d = MockDevice { kind: 0, latency_us: 100, fail_rate_permille: 50 };
    set.add(
        "F603 mocks",
        mock_device_sane(&d) && !mock_device_sane(&MockDevice { kind: 9, latency_us: 0, fail_rate_permille: 0 }),
        "5 devices",
    );

    // F604 确定性
    let s1 = deterministic_sequence(42, 10);
    let s2 = deterministic_sequence(42, 10);
    let s3 = deterministic_sequence(43, 10);
    set.add(
        "F604 determinism",
        s1 == s2 && s1 != s3,
        "seed reproducible",
    );

    // F605 覆盖率
    let cov = Coverage { lines_total: 1000, lines_hit: 850, branches_total: 500, branches_hit: 400 };
    set.add(
        "F605 coverage",
        cov.line_permille() == 850 && cov.branch_permille() == 800,
        "permille",
    );

    // F606 故障注入
    let mut fj = FaultInjector { kind: FaultKind::Errno, every_n: 3, count: 0 };
    let hits = (0..6).filter(|_| fj.should_inject()).count();
    set.add(
        "F606 fault inject",
        hits == 2 && FaultInjector { kind: FaultKind::Drop, every_n: 0, count: 0 }.should_inject() == false,
        "every_n cadence",
    );

    // F607 沙盒
    set.add(
        "F607 sandbox",
        sandbox_admits(4096, 10_000) && !sandbox_admits(8192, 1),
        "mem/io caps",
    );

    // F608 时间加速
    let mut vt = VTime { now_ms: 0, scale: 10 };
    let t = vt.fast_forward(100);
    set.add("F608 vtime", t == 1000 && vt.now_ms == 1000, "100 ticks x 10ms");

    // F609 金样本
    let a = [1u8, 2, 9];
    let e = [1u8, 2, 3];
    set.add(
        "F609 golden",
        golden_diff(&a, &e) == 1 && golden_acceptable(&a, &e, 1) && !golden_acceptable(&a, &e, 0),
        "diff gate",
    );

    // F610 编排
    let suites = ["unit", "selfcheck"];
    set.add(
        "F610 orchestrate",
        orchestrate(&suites, SuiteMode::Parallel) == 2
            && orchestrate_order_sane(1, 2)
            && !orchestrate_order_sane(2, 1),
        "count + order",
    );

    // F611 噪声猎手
    let stable = [true, true, true];
    let flaky = [true, false, true];
    set.add(
        "F611 flaky",
        !flaky_detected(&stable, 3) && flaky_detected(&flaky, 3) && flaky_retry_advised(&flaky, 3),
        "inconsistency",
    );

    // F612 分账
    let stats = [
        SuiteStat { name: "unit", passed: 90, failed: 10, duration_ms: 100 },
        SuiteStat { name: "fuzz", passed: 1, failed: 0, duration_ms: 900 },
    ];
    set.add(
        "F612 stats",
        stats[0].pass_permille() == 900 && slowest_suite(&stats, 2) == Some(1),
        "rate + slowest",
    );

    // F613 事件流
    set.add(
        "F613 events",
        test_event_loggable(TestEvent::Flaky) && test_event_loggable(TestEvent::Skipped),
        "5 events",
    );

    // F614 预算
    set.add(
        "F614 budget",
        suite_budget_ok(60_000) && !suite_budget_ok(60_001) && SUITE_BUDGET_MS == 60_000,
        "60s cap",
    );

    // F615 文档
    set.add(
        "F615 docs",
        test_doc_complete(0b1_1111) && !test_doc_complete(0b0_1111),
        "5 fields",
    );

    // F616 压力发生器
    let mut p = DetPrng::new(7);
    let ops = stress_op_count(&mut p, 10, 20);
    set.add(
        "F616 stress gen",
        (10..=20).contains(&ops),
        "bounded ops",
    );

    // F617 断言库
    set.add(
        "F617 asserts",
        assert_in_range(5, 1, 10) && !assert_in_range(11, 1, 10)
            && assert_sorted(&[1, 2, 3], 3) && !assert_sorted(&[2, 1], 2)
            && invariant_no_overlap((0, 10), (10, 20)) && !invariant_no_overlap((0, 10), (5, 15)),
        "range/sorted/overlap",
    );

    // F618 走廊
    set.add(
        "F618 corridor",
        test_corridor_pass(&[true; 5]) && !test_corridor_pass(&[true, true, true, true, false]),
        "5 cases",
    );

    // F619 健康分
    let h = TestHealth { runs: 1000, flakes: 3, infra_failures: 0 };
    let hb = TestHealth { runs: 100, flakes: 10, infra_failures: 2 };
    set.add(
        "F619 health",
        h.grade() == 0 && h.flake_permille() == 3 && hb.grade() == 2,
        "flake ladder",
    );

    // F620 导出
    let ex = TestExport { total: 100, by_tier: [60, 20, 15, 5] };
    set.add(
        "F620 export",
        test_export_valid(&ex) && !test_export_valid(&TestExport { total: 100, by_tier: [60, 20, 15, 4] }),
        "sum == total",
    );

    // F621 联测
    set.add(
        "F621 cross domain",
        cross_domain_known("sched-mem") && !cross_domain_known("random"),
        "5 plays",
    );

    // F622 优先级
    set.add(
        "F622 priority",
        run_first(Risk::Critical, Risk::Low) == Risk::Critical
            && run_first(Risk::Low, Risk::High) == Risk::High,
        "critical first",
    );

    // F623 降级
    set.add(
        "F623 downgrade",
        test_downgrade(InfraState::Healthy, 1000) == 0
            && test_downgrade(InfraState::Healthy, 99_999) == 1
            && test_downgrade(InfraState::Down, 1) == 2,
        "ladder",
    );

    // F624 一日补丁
    set.add(
        "F624 one-day patch",
        one_day_patch_complete(0b11_1111) && !one_day_patch_complete(0b11_1110),
        "6 steps",
    );

    // F625 年报
    let yb = TestYearbook { total_runs: 10_000, failures: 50, flakes: 10, coverage_line_permille: 850 };
    set.add(
        "F625 yearbook",
        yb.fail_permille() == 5 && yb.coverage_gate(),
        "fail rate + gate",
    );

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f604_seed_zero_still_sequences() {
        let a = deterministic_sequence(0, 5);
        let b = deterministic_sequence(0, 5);
        assert_eq!(a, b);
    }

    #[test]
    fn f609_length_diff() {
        assert_eq!(golden_diff(&[1], &[1, 2, 3]), 2);
    }

    #[test]
    fn f616_always_in_range_many_seeds() {
        for seed in 0..32u64 {
            let mut p = DetPrng::new(seed);
            let ops = stress_op_count(&mut p, 5, 9);
            assert!((5..=9).contains(&ops));
        }
    }

    #[test]
    fn f625_domain_selfcheck_all_pass() {
        let set = run_m7testing_checks();
        assert!(set.len() >= 25, "got {}", set.len());
        for i in 0..set.len() {
            let c = set.get(i).unwrap();
            assert!(c.passed, "{} | {}", c.name, c.detail);
        }
    }
}
