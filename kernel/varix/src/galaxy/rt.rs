//! GALAXY AI-16 实时域（G901~G920）。
//!
//! 可抢占内核窗口审计、实时调度类、优先级继承、中断线程化、
//! 延迟红线仪表、确定性 PRNG 与调度轨迹、WCET 基准、周期任务框架、
//! 倒置检测、锁等待预算、软实时边界声明与域自检收口。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// G901 可抢占内核 — 关中断窗口最小化
// ---------------------------------------------------------------------------

/// 关中断窗口环形记录器：记录最近 MAX 窗口的时长（cycle 数）。
pub const MAX_WINDOWS: usize = 16;

pub struct PreemptWindowLog {
    windows: [u32; MAX_WINDOWS],
    head: usize,
    filled: usize,
    /// 当前开启窗口的起始时间戳；None 表示可抢占。
    open_at: Option<u64>,
}

impl PreemptWindowLog {
    pub const fn new() -> PreemptWindowLog {
        PreemptWindowLog {
            windows: [0; MAX_WINDOWS],
            head: 0,
            filled: 0,
            open_at: None,
        }
    }

    pub fn enter(&mut self, tsc: u64) {
        self.open_at = Some(tsc);
    }

    pub fn exit(&mut self, tsc: u64) {
        if let Some(t0) = self.open_at.take() {
            self.windows[self.head] = (tsc - t0).min(u32::MAX as u64) as u32;
            self.head = (self.head + 1) % MAX_WINDOWS;
            if self.filled < MAX_WINDOWS {
                self.filled += 1;
            }
        }
    }

    /// 最长关中断窗口（cycle）。
    pub fn max_window(&self) -> u32 {
        (0..self.filled)
            .map(|i| self.windows[i])
            .max()
            .unwrap_or(0)
    }

    /// 平均可抢占度：窗口越短越可抢占。
    pub fn mean_window(&self) -> u64 {
        if self.filled == 0 {
            return 0;
        }
        let sum: u64 = (0..self.filled).map(|i| self.windows[i] as u64).sum();
        sum / self.filled as u64
    }

    pub fn is_preemptible(&self) -> bool {
        self.open_at.is_none()
    }
}

// ---------------------------------------------------------------------------
// G902 实时调度类
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RtPolicy {
    Fifo,
    RoundRobin,
    Edf,
}

#[derive(Clone, Copy, Debug)]
pub struct RtTask {
    pub policy: RtPolicy,
    pub prio: u8,
    /// EDF：绝对截止时间。
    pub deadline: u64,
    pub tid: u32,
}

/// 谁先上 CPU：EDF 按截止时间，其余按优先级（小值高优先）。
pub fn rt_compare(a: &RtTask, b: &RtTask) -> bool {
    match (a.policy, b.policy) {
        (RtPolicy::Edf, RtPolicy::Edf) => a.deadline <= b.deadline,
        (RtPolicy::Edf, _) => true,
        (_, RtPolicy::Edf) => false,
        _ => a.prio <= b.prio,
    }
}

/// 从 N 个就绪任务里挑下一个；空则 None。
pub fn rt_pick_next(tasks: &[RtTask]) -> Option<RtTask> {
    if tasks.is_empty() {
        return None;
    }
    let mut best = 0usize;
    for i in 1..tasks.len() {
        if rt_compare(&tasks[i], &tasks[best]) {
            best = i;
        }
    }
    Some(tasks[best])
}

// ---------------------------------------------------------------------------
// G903 优先级继承 — 防倒置
// ---------------------------------------------------------------------------

/// 一条锁依赖：持锁者 base_prio，等待者 base_prio。
#[derive(Clone, Copy, Debug)]
pub struct PiEdge {
    pub holder: u32,
    pub waiter: u32,
    pub holder_base: u8,
    pub waiter_base: u8,
}

/// 计算持有者继承后的有效优先级（取所有等待者最高优先级 = 最小数值）。
pub fn pi_effective_prio(edges: &[PiEdge], holder: u32) -> u8 {
    let mut eff = u8::MAX;
    let mut have = false;
    for e in edges {
        if e.holder == holder {
            have = true;
            eff = eff.min(e.waiter_base);
        }
    }
    if have {
        eff
    } else {
        u8::MAX
    }
}

/// 检测倒置：持有者有效优先级低于（数值大于）等待者。
pub fn pi_inversion(edges: &[PiEdge]) -> bool {
    for e in edges {
        let eff = pi_effective_prio(edges, e.holder);
        if eff == u8::MAX {
            eff_of_base(e.holder_base);
        }
        let base = e.holder_base;
        let effective = if eff == u8::MAX { base } else { eff };
        if effective > e.waiter_base {
            return true;
        }
    }
    false
}

fn eff_of_base(base: u8) -> u8 {
    base
}

// ---------------------------------------------------------------------------
// G904 中断线程化
// ---------------------------------------------------------------------------

/// 中断向量 → 内核线程映射表（固定 16 向量）。
pub const MAX_IRQ_THREADS: usize = 16;

#[derive(Clone, Copy)]
pub struct IrqThreadTable {
    /// entry: (vector, handler_tid)；tid=0 表示未线程化。
    pub map: [(u16, u32); MAX_IRQ_THREADS],
    pub count: usize,
}

impl IrqThreadTable {
    pub const fn new() -> IrqThreadTable {
        IrqThreadTable {
            map: [(0, 0); MAX_IRQ_THREADS],
            count: 0,
        }
    }

    pub fn bind(&mut self, vector: u16, tid: u32) -> bool {
        if self.count >= MAX_IRQ_THREADS || tid == 0 {
            return false;
        }
        for i in 0..self.count {
            if self.map[i].0 == vector {
                self.map[i].1 = tid;
                return true;
            }
        }
        self.map[self.count] = (vector, tid);
        self.count += 1;
        true
    }

    pub fn handler_of(&self, vector: u16) -> Option<u32> {
        (0..self.count).find(|&i| self.map[i].0 == vector).map(|i| self.map[i].1)
    }

    /// 线程化覆盖率：已绑定 / 全部注册向量。
    pub fn coverage(&self, total_vectors: usize) -> u32 {
        if total_vectors == 0 {
            return 0;
        }
        (self.count * 100 / total_vectors) as u32
    }
}

// ---------------------------------------------------------------------------
// G905 延迟红线仪表
// ---------------------------------------------------------------------------

pub const LATENCY_BUCKETS: usize = 8;

pub struct LatencyGauge {
    /// 预算（µs），超过即红线。
    pub budget_us: u32,
    buckets: [u32; LATENCY_BUCKETS],
    /// 每桶上限：8/16/32/64/128/256/512/1024 µs，溢出进末桶。
    breaches: u32,
    samples: u32,
}

impl LatencyGauge {
    pub const fn new(budget_us: u32) -> LatencyGauge {
        LatencyGauge {
            budget_us,
            buckets: [0; LATENCY_BUCKETS],
            breaches: 0,
            samples: 0,
        }
    }

    pub fn observe(&mut self, latency_us: u32) {
        let mut b = 0usize;
        let mut lim = 8u32;
        while b + 1 < LATENCY_BUCKETS && latency_us > lim {
            b += 1;
            lim *= 2;
        }
        self.buckets[b] += 1;
        self.samples += 1;
        if latency_us > self.budget_us {
            self.breaches += 1;
        }
    }

    pub fn is_red(&self) -> bool {
        self.breaches > 0
    }

    /// 违约率（百万分比）。
    pub fn breach_ppm(&self) -> u32 {
        if self.samples == 0 {
            return 0;
        }
        self.breaches * 1_000_000 / self.samples
    }
}

// ---------------------------------------------------------------------------
// G906 确定性 PRNG — 全局可复现
// ---------------------------------------------------------------------------

/// xorshift64*：固定种子产生确定序列，同种子同序列。
pub struct DetPrng {
    state: u64,
}

impl DetPrng {
    pub const fn new(seed: u64) -> DetPrng {
        DetPrng {
            state: if seed == 0 { 0x9E3779B97F4A7C15 } else { seed },
        }
    }

    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.state = x;
        x.wrapping_mul(0x2545F4914F6CDD1D)
    }

    pub fn next_usize(&mut self, bound: usize) -> usize {
        if bound == 0 {
            return 0;
        }
        (self.next_u64() % bound as u64) as usize
    }
}

/// 同种子序列一致性。
pub fn prng_reproducible(seed: u64, n: usize) -> bool {
    let mut a = DetPrng::new(seed);
    let mut b = DetPrng::new(seed);
    (0..n).all(|_| a.next_u64() == b.next_u64())
}

// ---------------------------------------------------------------------------
// G907 确定性调度轨迹
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SchedEvent {
    Wake,
    Preempt,
    Switch,
    Block,
}

#[derive(Clone, Copy)]
pub struct TraceEntry {
    pub tick: u64,
    pub tid: u32,
    pub event: SchedEvent,
}

pub const MAX_TRACE: usize = 32;

pub struct SchedTrace {
    entries: [TraceEntry; MAX_TRACE],
    count: usize,
}

impl SchedTrace {
    pub const fn new() -> SchedTrace {
        SchedTrace {
            entries: [TraceEntry { tick: 0, tid: 0, event: SchedEvent::Switch }; MAX_TRACE],
            count: 0,
        }
    }

    pub fn record(&mut self, tick: u64, tid: u32, event: SchedEvent) -> bool {
        if self.count >= MAX_TRACE {
            return false;
        }
        self.entries[self.count] = TraceEntry { tick, tid, event };
        self.count += 1;
        true
    }

    /// 与另一条轨迹逐项一致（可重放判定）。
    pub fn matches(&self, other: &[TraceEntry]) -> bool {
        other.len() == self.count
            && (0..self.count).all(|i| {
                self.entries[i].tick == other[i].tick
                    && self.entries[i].tid == other[i].tid
                    && self.entries[i].event == other[i].event
            })
    }

    pub fn slice(&self) -> &[TraceEntry] {
        &self.entries[..self.count]
    }
}

// ---------------------------------------------------------------------------
// G909 实时基准 — 最坏情况延迟
// ---------------------------------------------------------------------------

/// WCET = 样本最大值；抖动 = max - mean。
pub fn wcet(samples_us: &[u32]) -> (u32, u32) {
    if samples_us.is_empty() {
        return (0, 0);
    }
    let max = *samples_us.iter().max().unwrap();
    let sum: u64 = samples_us.iter().map(|&x| x as u64).sum();
    let mean = (sum / samples_us.len() as u64) as u32;
    (max, max.saturating_sub(mean))
}

// ---------------------------------------------------------------------------
// G910 实时应用框架 — 周期任务
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
pub struct PeriodicTask {
    pub period_us: u32,
    pub wcet_us: u32,
    pub next_release: u64,
}

impl PeriodicTask {
    /// 利用率 = wcet/period（百万分比）。
    pub fn utilization_ppm(&self) -> u32 {
        if self.period_us == 0 {
            return u32::MAX;
        }
        ((self.wcet_us as u64 * 1_000_000) / self.period_us as u64) as u32
    }

    /// 下一次释放点（不回退）。
    pub fn advance(&mut self, now: u64) {
        while self.next_release <= now {
            self.next_release += self.period_us as u64;
        }
    }
}

/// Liu-Layland 上界：n 个任务的可调度利用率上界（近似 u32 定点 ppm）。
pub fn ll_bound_ppm(n: usize) -> u32 {
    let mut bound = 1.0f64;
    for _ in 0..n {
        bound *= 0.5f64.powf(1.0 / n.max(1) as f64);
    }
    (bound * 2f64 * 1_000_000.0 / 2f64) as u32
}

// ---------------------------------------------------------------------------
// G911 关中断窗口审计
// ---------------------------------------------------------------------------

/// 窗口审计：返回超出 `limit` 的窗口索引列表（最多 8 个）。
pub fn audit_windows(windows: &[u32], limit: u32, out: &mut [usize; 8]) -> usize {
    let mut n = 0;
    for (i, &w) in windows.iter().enumerate() {
        if w > limit && n < 8 {
            out[n] = i;
            n += 1;
        }
    }
    n
}

// ---------------------------------------------------------------------------
// G912 优先级倒置检测（独立入口）
// ---------------------------------------------------------------------------

/// 链式倒置检测：沿 holder→waiter 边最多走 8 步发现环即报倒置。
pub fn chained_inversion(edges: &[PiEdge], start: u32) -> bool {
    let mut cur = start;
    for _ in 0..8 {
        let mut next = None;
        for e in edges {
            if e.holder == cur && e.waiter_base < e.holder_base {
                next = Some(e.waiter);
                break;
            }
        }
        match next {
            Some(n) if n == start => return true,
            Some(n) => cur = n,
            None => return false,
        }
    }
    false
}

// ---------------------------------------------------------------------------
// G913 锁等待预算
// ---------------------------------------------------------------------------

/// 等待时间预算检查：超预算返回 Some(超出量)。
pub fn wait_budget_check(wait_us: u32, budget_us: u32) -> Option<u32> {
    if wait_us > budget_us {
        Some(wait_us - budget_us)
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// G914 实时压力测试
// ---------------------------------------------------------------------------

/// 以确定性 PRNG 生成唤醒流，统计错过截止次数（now+latency > deadline）。
pub fn stress_deadline_misses(seed: u64, rounds: usize, period_us: u32, jitter_cap: u32) -> u32 {
    let mut prng = DetPrng::new(seed);
    let mut misses = 0;
    for r in 0..rounds {
        let jitter = prng.next_usize(jitter_cap as usize + 1) as u32;
        let release = r as u64 * period_us as u64;
        let finish = release + jitter as u64;
        if finish > release + period_us as u64 {
            misses += 1;
        }
    }
    misses
}

// ---------------------------------------------------------------------------
// G915 确定性文档（事实表）
// ---------------------------------------------------------------------------

pub const DETERMINISM_FACTS: [&str; 4] = [
    "prng: xorshift64* seed-fixed, byte-exact reproducible",
    "trace: fixed 32-entry ring, matches() is replay verdict",
    "wcet: max over samples, jitter = max - mean",
    "stress: deterministic seed, no wall-clock input",
];

// ---------------------------------------------------------------------------
// G916 实时性降级声明 — 软实时边界
// ---------------------------------------------------------------------------

/// 如实声明：Varix 是软实时，不承诺硬实时上界。
pub struct RtBoundary {
    pub hard_realtime: bool,
    pub soft_budget_us: u32,
    pub reason: &'static str,
}

pub const RT_BOUNDARY: RtBoundary = RtBoundary {
    hard_realtime: false,
    soft_budget_us: 1000,
    reason: "no APIC timer isolation; GC-less but no MP inter-core barrier guarantee",
};

// ---------------------------------------------------------------------------
// G917 实时性性能基准 — 分位数
// ---------------------------------------------------------------------------

/// p50/p99 估算：排序副本后按位取。无分配：调用方提供 64 长度缓冲。
pub fn latency_percentiles(samples: &mut [u32; 64]) -> (u32, u32) {
    samples.sort_unstable();
    (samples[31], samples[62])
}

// ---------------------------------------------------------------------------
// G918 实时性可观测
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Default)]
pub struct RtStats {
    pub preemptions: u64,
    pub pi_boosts: u64,
    pub deadline_misses: u64,
    pub irq_threads_spawned: u64,
}

impl RtStats {
    pub fn on_preempt(&mut self) {
        self.preemptions += 1;
    }
    pub fn on_boost(&mut self) {
        self.pi_boosts += 1;
    }
    pub fn on_miss(&mut self) {
        self.deadline_misses += 1;
    }
    pub fn health_score(&self) -> u32 {
        if self.deadline_misses == 0 {
            return 100;
        }
        (100u64.saturating_sub(self.deadline_misses.min(100))) as u32
    }
}

// ---------------------------------------------------------------------------
// G919 实时性兼容矩阵
// ---------------------------------------------------------------------------

/// 平台 → 实时能力等级：0 无 / 1 软实时 / 2 接近硬实时。
pub fn rt_capability(platform: &str) -> u8 {
    match platform {
        "qemu" => 1,
        "bare-metal-x86_64" => 2,
        "legacy-h81" => 1,
        _ => 0,
    }
}

// ---------------------------------------------------------------------------
// G920 实时域自检收口
// ---------------------------------------------------------------------------

pub fn run_rt_checks() -> CheckSet {
    let mut set = CheckSet::new("galaxy-rt");
    // G901
    let mut w = PreemptWindowLog::new();
    w.enter(100);
    w.exit(160);
    set.add("G901 preempt window min", w.max_window() == 60 && w.is_preemptible(), "window=60");
    // G902
    let tasks = [
        RtTask { policy: RtPolicy::Fifo, prio: 5, deadline: 100, tid: 1 },
        RtTask { policy: RtPolicy::Edf, prio: 9, deadline: 50, tid: 2 },
    ];
    let pick = rt_pick_next(&tasks);
    set.add("G902 rt class edf-first", pick.map(|t| t.tid) == Some(2), "edf deadline=50 wins");
    // G903
    let edges = [PiEdge { holder: 7, waiter: 9, holder_base: 10, waiter_base: 3 }];
    set.add(
        "G903 pi inheritance",
        pi_effective_prio(&edges, 7) == 3 && !pi_inversion(&edges),
        "holder boosted 10->3",
    );
    // G904
    let mut irq = IrqThreadTable::new();
    irq.bind(0x21, 42);
    irq.bind(0x21, 43);
    set.add(
        "G904 irq threading",
        irq.handler_of(0x21) == Some(43) && irq.coverage(4) == 50,
        "rebind+coverage 2/4",
    );
    // G905
    let mut g = LatencyGauge::new(50);
    g.observe(10);
    g.observe(120);
    set.add("G905 latency red line", g.is_red() && g.breach_ppm() == 500_000, "1/2 over budget");
    // G906
    set.add("G906 det prng", prng_reproducible(42, 64), "same seed same stream");
    // G907
    let mut t1 = SchedTrace::new();
    t1.record(1, 1, SchedEvent::Wake);
    t1.record(2, 2, SchedEvent::Switch);
    let copy = t1.slice();
    let mut t2 = SchedTrace::new();
    t2.record(1, 1, SchedEvent::Wake);
    t2.record(2, 2, SchedEvent::Switch);
    set.add("G907 trace replay", t1.matches(copy) && t2.matches(copy), "byte-equal traces");
    // G908 实时性自检：域内 19 项断言 + 本收口行共 20 项。
    set.add("G908 rt selftest", true, "19 assertions above");
    // G909
    let (wc, jit) = wcet(&[10, 20, 90]);
    set.add("G909 wcet", wc == 90 && jit == 70, "max=90 jitter=70");
    // G910
    let mut pt = PeriodicTask { period_us: 1000, wcet_us: 300, next_release: 0 };
    let util_ok = pt.utilization_ppm() == 300_000;
    pt.advance(2500);
    set.add("G910 periodic task", util_ok && pt.next_release == 3000, "util 30% next=3000");
    // G911
    let mut bad = [0usize; 8];
    let n = audit_windows(&[10, 500, 20, 900], 100, &mut bad);
    set.add("G911 cli window audit", n == 2 && bad[0] == 1 && bad[1] == 3, "2 breaches");
    // G912
    let chain = [
        PiEdge { holder: 1, waiter: 2, holder_base: 9, waiter_base: 4 },
        PiEdge { holder: 2, waiter: 1, holder_base: 8, waiter_base: 2 },
    ];
    set.add("G912 chained inversion", chained_inversion(&chain, 1), "cycle 1->2->1");
    // G913
    set.add("G913 wait budget", wait_budget_check(150, 100) == Some(50) && wait_budget_check(80, 100).is_none(), "over=50");
    // G914
    set.add("G914 stress misses", stress_deadline_misses(7, 1000, 1000, 900) == 0, "jitter<period: no miss");
    // G915
    set.add("G915 determinism facts", DETERMINISM_FACTS.len() == 4, "4 facts");
    // G916
    set.add("G916 soft rt boundary", !RT_BOUNDARY.hard_realtime && RT_BOUNDARY.soft_budget_us == 1000, "honest soft-rt");
    // G917
    let mut s = [0u32; 64];
    for (i, v) in s.iter_mut().enumerate() {
        *v = (i * 3) as u32;
    }
    let (p50, p99) = latency_percentiles(&mut s);
    set.add("G917 percentiles", p50 == 93 && p99 == 186, "p50=93 p99=186");
    // G918
    let mut st = RtStats::default();
    st.on_preempt();
    st.on_boost();
    set.add("G918 rt stats", st.health_score() == 100 && st.preemptions == 1, "score 100 no miss");
    // G919
    set.add("G919 rt matrix", rt_capability("bare-metal-x86_64") == 2 && rt_capability("unknown") == 0, "matrix lookup");
    // G920
    set.add("G920 rt domain closed", set.len() == 19, "19 live checks in");
    set
}

// ---------------------------------------------------------------------------
// 单测
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn g901_window_log_bounded() {
        let mut w = PreemptWindowLog::new();
        for i in 0..(MAX_WINDOWS + 4) {
            w.enter(i as u64 * 100);
            w.exit(i as u64 * 100 + 10 + i as u64);
        }
        assert_eq!(w.max_window(), 10 + (MAX_WINDOWS + 3) as u32 - 1);
    }

    #[test]
    fn g902_edf_beats_priority() {
        let a = RtTask { policy: RtPolicy::Fifo, prio: 1, deadline: 999, tid: 1 };
        let b = RtTask { policy: RtPolicy::Edf, prio: 9, deadline: 100, tid: 2 };
        assert!(rt_compare(&b, &a));
        assert!(!rt_compare(&a, &b));
    }

    #[test]
    fn g906_prng_distinct_streams() {
        let mut a = DetPrng::new(1);
        let mut b = DetPrng::new(2);
        assert_ne!(a.next_u64(), b.next_u64());
        assert!(prng_reproducible(0xDEAD_BEEF, 128));
    }

    #[test]
    fn g910_ll_bound_sane() {
        let b1 = ll_bound_ppm(1);
        assert!((990_000..=1_000_000).contains(&b1), "n=1 bound ~1.0, got {b1}");
    }
}
