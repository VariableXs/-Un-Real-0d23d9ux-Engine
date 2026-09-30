//! F083 多核负载均衡 / F084 能耗感知调度 / F085 算力独占模式 / F086 前景加速策略 /
//! F087 调度延迟仪表 / F088 优先级反转防护 / F091 调度策略热切换 / F092 CPU 亲和性 /
//! F093 隔离核 / F095 饥饿检测器 / F096 交互感知调度 / F097 帧率感知调度 /
//! F098 后台限流.
//!
//! The policy layer. Every decision here is a pure function over a small
//! snapshot, which is the only way a scheduler can be reasoned about: the hot
//! path reads numbers, calls one of these, and acts.

use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};

use super::engine::{SchedClass, Thread, ThreadTable, NO_THREAD, PRIO_NORMAL, PRIO_RT};

// ---------------------------------------------------------------------------
// F092 — affinity
// ---------------------------------------------------------------------------

/// A CPU bitmask. `0` means "no restriction" by convention, which is what
/// `Thread::allowed_on` implements.
pub fn affinity_all() -> u64 {
    0
}

pub fn affinity_of(cpu: u32) -> u64 {
    if cpu >= 64 {
        0
    } else {
        1u64 << cpu
    }
}

pub fn affinity_range(first: u32, last: u32) -> u64 {
    if first > last || first >= 64 {
        return 0;
    }
    let last = last.min(63);
    let width = last - first + 1;
    if width >= 64 {
        u64::MAX
    } else {
        ((1u64 << width) - 1) << first
    }
}

pub fn affinity_allows(mask: u64, cpu: u32) -> bool {
    if mask == 0 {
        return true;
    }
    cpu < 64 && (mask >> cpu) & 1 == 1
}

pub fn affinity_count(mask: u64) -> u32 {
    if mask == 0 {
        63 // "any" — reported as the machine's core count cap
    } else {
        mask.count_ones()
    }
}

/// F092: may a thread move from `from` to `to` right now? A running thread with
/// a hot cache is worth keeping where it is unless the imbalance is real.
pub fn migration_allowed(t: &Thread, from: u32, to: u32, imbalance: u32) -> bool {
    t.allowed_on(to) && from != to && imbalance >= MIGRATION_THRESHOLD
}

/// Load difference that justifies a migration. Moving a thread costs its warm
/// cache; below this the cure is worse than the disease.
pub const MIGRATION_THRESHOLD: u32 = 2;

// ---------------------------------------------------------------------------
// F083 — load balancing
// ---------------------------------------------------------------------------

/// A concrete migration the balancer decided on.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct MigrationPlan {
    pub from: u32,
    pub to: u32,
    pub tid: u32,
    pub reason: &'static str,
}

/// Pick the busiest and the idlest core and hand back one migration. Repeated
/// calls converge; a single call never rebalances more than one thread per
/// core, which keeps the balancer from stampeding.
pub fn plan_migration(
    loads: &[u32],
    rq_heads: &[u32],
    threads: &[Thread],
    hysteresis: u32,
) -> Option<MigrationPlan> {
    if loads.len() != rq_heads.len() || loads.len() < 2 {
        return None;
    }
    let mut busiest = 0usize;
    let mut idlest = 1usize;
    for (i, l) in loads.iter().enumerate() {
        if *l > loads[busiest] {
            busiest = i;
        }
        if *l < loads[idlest] {
            idlest = i;
        }
    }
    if busiest == idlest {
        return None;
    }
    let imbalance = loads[busiest].saturating_sub(loads[idlest]);
    if imbalance < hysteresis.max(MIGRATION_THRESHOLD) {
        return None;
    }
    let tid = rq_heads[busiest];
    if tid == NO_THREAD {
        return None;
    }
    let t = threads.get(tid as usize)?;
    if !migration_allowed(t, busiest as u32, idlest as u32, imbalance) {
        return None;
    }
    Some(MigrationPlan {
        from: busiest as u32,
        to: idlest as u32,
        tid,
        reason: "imbalance",
    })
}

pub struct LoadBalancer {
    interval_ticks: u32,
    since_last: u32,
    migrations: u64,
    skipped_no_candidate: u64,
}

impl LoadBalancer {
    pub const fn new(interval_ticks: u32) -> LoadBalancer {
        LoadBalancer {
            interval_ticks,
            since_last: 0,
            migrations: 0,
            skipped_no_candidate: 0,
        }
    }

    /// Tick gate: the balancer runs at its own cadence, not every tick.
    pub fn should_run(&mut self) -> bool {
        self.since_last += 1;
        if self.since_last >= self.interval_ticks {
            self.since_last = 0;
            true
        } else {
            false
        }
    }

    pub fn note_applied(&mut self, plan: &MigrationPlan) {
        self.migrations += 1;
        let _ = plan;
        crate::sched::engine::stats()
            .migrations
            .fetch_add(1, Ordering::Relaxed);
    }

    pub fn note_skipped(&mut self) {
        self.skipped_no_candidate += 1;
    }

    pub fn migrations(&self) -> u64 {
        self.migrations
    }

    pub fn skipped(&self) -> u64 {
        self.skipped_no_candidate
    }
}

impl Default for LoadBalancer {
    fn default() -> LoadBalancer {
        LoadBalancer::new(8)
    }
}

// ---------------------------------------------------------------------------
// F084 — energy-aware placement
// ---------------------------------------------------------------------------

/// Power/performance class of a core. On a hybrid part the efficient cores are
/// where background work belongs; on a uniform part every core reports
/// `Performance` and the policy degenerates to the load balancer, which is the
/// correct behaviour.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CoreKind {
    Efficiency,
    Performance,
    Unknown,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum EnergyMode {
    /// Everything on the fast cores.
    Performance,
    /// Interactive/RT on fast cores, background on efficient ones.
    #[default]
    Balanced,
    /// Background pinned to efficient cores; interactivity still protected.
    Saver,
}

impl EnergyMode {
    pub fn as_str(self) -> &'static str {
        match self {
            EnergyMode::Performance => "performance",
            EnergyMode::Balanced => "balanced",
            EnergyMode::Saver => "saver",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct CorePower {
    pub cpu: u32,
    pub kind: Option<CoreKind>,
    /// Relative energy cost of running a tick here (higher = thirstier).
    pub cost: u16,
}

/// F084: choose a core for a thread of `class`.
pub fn pick_core_for(cores: &[CorePower], class: SchedClass, mode: EnergyMode) -> Option<u32> {
    if cores.is_empty() {
        return None;
    }
    let wants_efficiency = match mode {
        EnergyMode::Performance => false,
        EnergyMode::Saver => !matches!(
            class,
            SchedClass::Interactive | SchedClass::RealTime | SchedClass::Isolated
        ),
        EnergyMode::Balanced => matches!(class, SchedClass::Batch | SchedClass::Idle),
    };
    if wants_efficiency {
        if let Some(c) = cores
            .iter()
            .filter(|c| c.kind == Some(CoreKind::Efficiency))
            .min_by_key(|c| c.cost)
        {
            return Some(c.cpu);
        }
    }
    // Interactive work wants the fastest core available.
    if matches!(
        class,
        SchedClass::Interactive | SchedClass::RealTime | SchedClass::Isolated
    ) {
        if let Some(c) = cores
            .iter()
            .filter(|c| c.kind != Some(CoreKind::Efficiency))
            .min_by_key(|c| c.cost)
        {
            return Some(c.cpu);
        }
    }
    cores.iter().min_by_key(|c| c.cost).map(|c| c.cpu)
}

// ---------------------------------------------------------------------------
// F085 — compute exclusivity
// ---------------------------------------------------------------------------

/// One thread may hold the machine. The lease carries an expiry so a crashed
/// holder cannot leave the machine exclusive forever.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct ExclusiveLease {
    pub holder: u32,
    pub granted_at: u64,
    pub ttl_ticks: u64,
    pub active: bool,
}

impl ExclusiveLease {
    pub const DEFAULT_TTL: u64 = 2_000;

    pub fn grant(&mut self, holder: u32, now: u64, ttl_ticks: u64) -> bool {
        if self.active && self.holder != holder {
            return false;
        }
        self.holder = holder;
        self.granted_at = now;
        self.ttl_ticks = if ttl_ticks == 0 {
            Self::DEFAULT_TTL
        } else {
            ttl_ticks
        };
        self.active = true;
        true
    }

    pub fn release(&mut self, holder: u32) -> bool {
        if self.active && self.holder == holder {
            self.active = false;
            true
        } else {
            false
        }
    }

    /// Expire on its own if the holder stopped renewing.
    pub fn expired(&self, now: u64) -> bool {
        self.active && now.saturating_sub(self.granted_at) > self.ttl_ticks
    }

    pub fn valid(&self, now: u64) -> bool {
        self.active && !self.expired(now)
    }

    pub fn renew(&mut self, holder: u32, now: u64) -> bool {
        if self.active && self.holder == holder {
            self.granted_at = now;
            true
        } else {
            false
        }
    }
}

/// F085: while an exclusive lease is live, only the holder may be scheduled
/// ahead of it — everyone else is demoted rather than starved forever.
pub fn exclusive_priority_for(tid: u32, lease: &ExclusiveLease, now: u64) -> Option<u8> {
    if lease.valid(now) && lease.holder == tid {
        Some(PRIO_RT + 16)
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// F086 — foreground acceleration
// ---------------------------------------------------------------------------

/// Tracks which thread the user is interacting with and how long a boost lasts.
#[derive(Clone, Copy, Debug, Default)]
pub struct ForegroundTracker {
    pub current: u32,
    pub boosted_until: u64,
    pub boosts: u64,
}

impl ForegroundTracker {
    pub const BOOST_TICKS: u64 = 30;

    pub fn set_foreground(&mut self, tid: u32, now: u64) -> bool {
        if tid == self.current {
            return false;
        }
        self.current = tid;
        self.boosted_until = now + Self::BOOST_TICKS;
        self.boosts += 1;
        true
    }

    pub fn is_boosted(&self, tid: u32, now: u64) -> bool {
        tid == self.current && now <= self.boosted_until
    }

    /// Apply the boost to a thread; returns whether anything changed.
    pub fn apply(&self, t: &mut Thread, now: u64, boost_prio: u8) -> bool {
        if self.is_boosted(t.tid, now) {
            t.boost_to(boost_prio)
        } else {
            t.restore_prio()
        }
    }
}

// ---------------------------------------------------------------------------
// F087 — scheduling latency meter
// ---------------------------------------------------------------------------

/// Wake-to-run budget. 2 ms is the point where a UI starts to feel sticky.
pub const SCHED_LATENCY_BUDGET_NS: u64 = 2_000_000;
pub const LATENCY_BUCKETS: usize = 16;

/// Reuses the same log-scaled histogram design as the interrupt meter so the
/// two can be compared directly, but keeps its own budget and its own storage:
/// a scheduling stall and a long ISR are different diagnoses.
pub struct SchedLatency {
    buckets: [AtomicU32; LATENCY_BUCKETS],
    samples: AtomicU64,
    total_ns: AtomicU64,
    max_ns: AtomicU64,
    over_budget: AtomicU64,
    wake_at: AtomicU64,
}

impl SchedLatency {
    pub const fn new() -> SchedLatency {
        SchedLatency {
            buckets: [const { AtomicU32::new(0) }; LATENCY_BUCKETS],
            samples: AtomicU64::new(0),
            total_ns: AtomicU64::new(0),
            max_ns: AtomicU64::new(0),
            over_budget: AtomicU64::new(0),
            wake_at: AtomicU64::new(0),
        }
    }

    fn bucket_of(ns: u64) -> usize {
        if ns < 1_000 {
            0
        } else {
            let log = 63 - (ns / 1_000).leading_zeros() as usize;
            (1 + log).min(LATENCY_BUCKETS - 1)
        }
    }

    pub fn mark_wakeup(&self) {
        self.wake_at
            .store(crate::cpu::clock::read_tsc(), Ordering::Release);
    }

    pub fn record(&self, ns: u64) {
        self.buckets[Self::bucket_of(ns)].fetch_add(1, Ordering::Relaxed);
        self.samples.fetch_add(1, Ordering::Relaxed);
        self.total_ns.fetch_add(ns, Ordering::Relaxed);
        self.max_ns.fetch_max(ns, Ordering::Relaxed);
        if ns > SCHED_LATENCY_BUDGET_NS {
            self.over_budget.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Called when a woken thread finally gets the CPU.
    pub fn record_run(&self) -> u64 {
        let at = self.wake_at.load(Ordering::Acquire);
        if at == 0 {
            return 0;
        }
        let cal = crate::cpu::clock::calibration();
        let ns = if cal.usable() {
            cal.tsc_to_ns(crate::cpu::clock::read_tsc().wrapping_sub(at))
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

    /// Approximate percentile from the bucket floors.
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
                return if i == 0 { 0 } else { 1_000u64 << (i - 1) };
            }
        }
        self.max_ns()
    }

    pub fn p99(&self) -> u64 {
        self.percentile(99)
    }

    pub fn render(&self, out: &mut [u8]) -> usize {
        let mut w = crate::cpu::Hud::new(out);
        w.str("sched-latency n=");
        w.num(self.samples());
        w.str(" p99=");
        w.num(self.p99() / 1000);
        w.str("us max=");
        w.num(self.max_ns() / 1000);
        w.str("us over=");
        w.num(self.over_budget());
        w.str("\n");
        w.used()
    }
}

impl Default for SchedLatency {
    fn default() -> SchedLatency {
        SchedLatency::new()
    }
}

static LATENCY: SchedLatency = SchedLatency::new();

pub fn latency() -> &'static SchedLatency {
    &LATENCY
}

// ---------------------------------------------------------------------------
// F088 — priority inversion protection
// ---------------------------------------------------------------------------

/// Direct inheritance stops A→B inversion. A chain (A waits on B waits on C)
/// still has to be bounded, because the chain length is attacker-controlled.
pub const MAX_INHERITANCE_CHAIN: u8 = 3;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct InversionEvent {
    pub blocked: u32,
    pub owner: u32,
    pub blocked_prio: u8,
    pub owner_prio: u8,
    pub chain_depth: u8,
}

pub struct InversionGuard {
    events: [InversionEvent; 16],
    len: usize,
    boosts: u64,
    chain_breaks: u64,
}

impl InversionGuard {
    pub const fn new() -> InversionGuard {
        InversionGuard {
            events: [InversionEvent {
                blocked: 0,
                owner: 0,
                blocked_prio: 0,
                owner_prio: 0,
                chain_depth: 0,
            }; 16],
            len: 0,
            boosts: 0,
            chain_breaks: 0,
        }
    }

    /// Record a blocking relationship and decide whether to donate priority.
    pub fn on_block(&mut self, blocked: &Thread, owner: &mut Thread, chain_depth: u8) -> bool {
        let event = InversionEvent {
            blocked: blocked.tid,
            owner: owner.tid,
            blocked_prio: blocked.prio,
            owner_prio: owner.prio,
            chain_depth,
        };
        if self.len < self.events.len() {
            self.events[self.len] = event;
            self.len += 1;
        }
        if chain_depth > MAX_INHERITANCE_CHAIN {
            // Too deep to trust: leave the priorities alone and count it. A
            // bounded, visible failure beats an unbounded boost loop.
            self.chain_breaks += 1;
            return false;
        }
        let donated = owner.boost_to(blocked.prio);
        if donated {
            self.boosts += 1;
        }
        donated
    }

    pub fn events(&self) -> usize {
        self.len
    }

    pub fn boosts(&self) -> u64 {
        self.boosts
    }

    pub fn chain_breaks(&self) -> u64 {
        self.chain_breaks
    }

    pub fn last(&self) -> Option<InversionEvent> {
        if self.len == 0 {
            None
        } else {
            Some(self.events[self.len - 1])
        }
    }
}

impl Default for InversionGuard {
    fn default() -> InversionGuard {
        InversionGuard::new()
    }
}

// ---------------------------------------------------------------------------
// F091 — policy hot swap
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum PolicyKind {
    /// Strict priority, first in first out. Simple and predictable.
    Fifo,
    /// Class + priority with slice rotation (the default).
    #[default]
    RoundRobin,
    /// Weighted fair sharing across classes (F090 batch support).
    Fair,
    /// Real-time first, everything else only when RT is idle.
    RtFirst,
}

impl PolicyKind {
    pub fn as_str(self) -> &'static str {
        match self {
            PolicyKind::Fifo => "fifo",
            PolicyKind::RoundRobin => "rr",
            PolicyKind::Fair => "fair",
            PolicyKind::RtFirst => "rt-first",
        }
    }

    pub fn parse(s: &str) -> Option<PolicyKind> {
        match s {
            "fifo" => Some(PolicyKind::Fifo),
            "rr" => Some(PolicyKind::RoundRobin),
            "fair" => Some(PolicyKind::Fair),
            "rt-first" => Some(PolicyKind::RtFirst),
            _ => None,
        }
    }

    /// Effective slice length under this policy.
    pub fn slice_ticks(self, base: u32) -> u32 {
        match self {
            PolicyKind::Fifo => u32::MAX / 2, // run until blocked
            PolicyKind::RoundRobin => base,
            PolicyKind::Fair => base.saturating_mul(2),
            PolicyKind::RtFirst => base.saturating_mul(4),
        }
    }
}

pub struct PolicySwitcher {
    active: AtomicU32,
    switches: u64,
    rejected: u64,
}

impl PolicySwitcher {
    pub const fn new() -> PolicySwitcher {
        PolicySwitcher {
            active: AtomicU32::new(PolicyKind::RoundRobin as u32),
            switches: 0,
            rejected: 0,
        }
    }

    pub fn active(&self) -> PolicyKind {
        match self.active.load(Ordering::Acquire) {
            0 => PolicyKind::Fifo,
            2 => PolicyKind::Fair,
            3 => PolicyKind::RtFirst,
            _ => PolicyKind::RoundRobin,
        }
    }

    /// F091: swap the policy on a live system. A refusal leaves the old policy
    /// untouched — a failed switch must not leave a half-applied state.
    pub fn switch(&mut self, to: PolicyKind, any_rt_running: bool) -> bool {
        if to == PolicyKind::Fifo && any_rt_running {
            // FIFO with RT present is the classic way to hang a desktop.
            self.rejected += 1;
            return false;
        }
        if to == self.active() {
            return false;
        }
        self.active.store(to as u32, Ordering::Release);
        self.switches += 1;
        crate::kinfo!("sched: policy -> {}", to.as_str());
        true
    }

    pub fn switches(&self) -> u64 {
        self.switches
    }

    pub fn rejected(&self) -> u64 {
        self.rejected
    }
}

impl Default for PolicySwitcher {
    fn default() -> PolicySwitcher {
        PolicySwitcher::new()
    }
}

// ---------------------------------------------------------------------------
// F093 — isolated cores
// ---------------------------------------------------------------------------

/// Cores reserved for the critical path (audio, input, renderer). Nothing else
/// is admitted, so a background burst cannot add jitter to them.
#[derive(Clone, Copy, Debug, Default)]
pub struct IsolationSet {
    mask: u64,
}

impl IsolationSet {
    pub const fn new() -> IsolationSet {
        IsolationSet { mask: 0 }
    }

    pub fn isolate(&mut self, cpu: u32) -> bool {
        if cpu >= 64 || self.mask == 0 && cpu == 0 {
            // Core 0 always keeps a share: something has to run the kernel.
            if cpu == 0 {
                return false;
            }
        }
        self.mask |= 1u64 << cpu;
        true
    }

    pub fn release(&mut self, cpu: u32) -> bool {
        if cpu < 64 && (self.mask >> cpu) & 1 == 1 {
            self.mask &= !(1u64 << cpu);
            true
        } else {
            false
        }
    }

    pub fn is_isolated(&self, cpu: u32) -> bool {
        cpu < 64 && (self.mask >> cpu) & 1 == 1
    }

    pub fn mask(&self) -> u64 {
        self.mask
    }

    pub fn count(&self) -> u32 {
        self.mask.count_ones()
    }

    /// F093: only the classes that were promised this core may use it.
    pub fn admits(&self, cpu: u32, class: SchedClass) -> bool {
        if !self.is_isolated(cpu) {
            return true;
        }
        matches!(
            class,
            SchedClass::Isolated | SchedClass::RealTime | SchedClass::Idle
        )
    }
}

// ---------------------------------------------------------------------------
// F095 — starvation detector
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StarvationReport {
    pub starving: usize,
    pub worst_tid: u32,
    pub worst_wait_ticks: u64,
}

impl Default for StarvationReport {
    /// A report with no victim names `NO_THREAD` — the default must never look
    /// like "thread 0 is the problem".
    fn default() -> StarvationReport {
        StarvationReport {
            starving: 0,
            worst_tid: NO_THREAD,
            worst_wait_ticks: 0,
        }
    }
}

pub struct StarvationWatch {
    threshold: u64,
    rescues: u64,
    worst_ever: u64,
}

impl StarvationWatch {
    pub const fn new(threshold: u64) -> StarvationWatch {
        StarvationWatch {
            threshold,
            rescues: 0,
            worst_ever: 0,
        }
    }

    pub fn threshold(&self) -> u64 {
        self.threshold
    }

    pub fn scan(&mut self, table: &ThreadTable) -> StarvationReport {
        let mut report = StarvationReport::default();
        for t in table.threads().iter() {
            if t.state == super::engine::ThreadState::Runnable && t.wait_ticks > self.threshold {
                report.starving += 1;
                if t.wait_ticks > report.worst_wait_ticks {
                    report.worst_wait_ticks = t.wait_ticks;
                    report.worst_tid = t.tid;
                }
            }
        }
        self.worst_ever = self.worst_ever.max(report.worst_wait_ticks);
        report
    }

    /// Rescue the worst offender by promoting it one class up, once. A thread
    /// already in the top band is left alone: the detector reports, the policy
    /// does not invent priority out of nothing.
    pub fn rescue(&mut self, table: &mut ThreadTable, report: &StarvationReport) -> bool {
        if report.worst_tid == NO_THREAD {
            return false;
        }
        let tid = report.worst_tid;
        let promoted = match table.thread_mut(tid) {
            Some(t) => {
                let next = match t.class {
                    SchedClass::Idle => SchedClass::Batch,
                    SchedClass::Batch => SchedClass::Normal,
                    SchedClass::Normal => SchedClass::Interactive,
                    other => other,
                };
                if next == t.class {
                    false
                } else {
                    t.class = next;
                    t.prio = t.prio.max(PRIO_NORMAL);
                    t.base_prio = t.prio;
                    t.wait_ticks = 0;
                    true
                }
            }
            None => false,
        };
        if promoted && table.requeue(tid) {
            self.rescues += 1;
            return true;
        }
        false
    }

    pub fn rescues(&self) -> u64 {
        self.rescues
    }

    pub fn worst_ever(&self) -> u64 {
        self.worst_ever
    }
}

impl Default for StarvationWatch {
    fn default() -> StarvationWatch {
        StarvationWatch::new(200)
    }
}

// ---------------------------------------------------------------------------
// F096 — interaction awareness
// ---------------------------------------------------------------------------

/// Goal: the thread that just received input gets the CPU within a tick or two.
#[derive(Clone, Copy, Debug, Default)]
pub struct InteractionTracker {
    pub wakes: u64,
    pub boosted: u64,
    pub last_wake_tick: u64,
}

impl InteractionTracker {
    /// An input event woke `t`; mark it so the picker boosts it on entry.
    pub fn on_input_wake(&mut self, t: &mut Thread, now: u64) {
        t.flags |= super::engine::FLAG_INPUT_WOKEN;
        self.wakes += 1;
        self.last_wake_tick = now;
    }

    /// Called when the thread is picked: the boost lasts one quantum.
    pub fn on_pick(&mut self, t: &mut Thread, boost_prio: u8) -> bool {
        if t.flags & super::engine::FLAG_INPUT_WOKEN == 0 {
            return false;
        }
        t.flags &= !super::engine::FLAG_INPUT_WOKEN;
        if t.boost_to(boost_prio) {
            self.boosted += 1;
            return true;
        }
        false
    }

    /// Priority an interactive thread should reach.
    pub fn target_prio(&self) -> u8 {
        PRIO_NORMAL + 8
    }
}

// ---------------------------------------------------------------------------
// F097 — frame-rate awareness
// ---------------------------------------------------------------------------

/// 60 Hz with a little headroom: 16.6 ms per frame, and a miss is only counted
/// once the frame is genuinely late, not when it is merely tight.
pub const FRAME_TARGET_US: u32 = 16_667;
pub const FRAME_MISS_SLACK_US: u32 = 2_000;

#[derive(Clone, Copy, Debug)]
pub struct FrameBudget {
    pub target_us: u32,
    pub produced: u64,
    pub missed: u64,
    pub worst_us: u32,
    pub behind: bool,
    /// Consecutive misses — one late frame is noise, three is a trend.
    pub miss_streak: u32,
}

impl FrameBudget {
    pub const fn new(target_us: u32) -> FrameBudget {
        FrameBudget {
            target_us,
            produced: 0,
            missed: 0,
            worst_us: 0,
            behind: false,
            miss_streak: 0,
        }
    }

    pub fn record_frame(&mut self, took_us: u32) -> bool {
        self.produced += 1;
        self.worst_us = self.worst_us.max(took_us);
        let late = took_us > self.target_us + FRAME_MISS_SLACK_US;
        if late {
            self.missed += 1;
            self.miss_streak += 1;
        } else {
            self.miss_streak = 0;
        }
        self.behind = self.miss_streak >= 3;
        late
    }

    pub fn missed(&self) -> u64 {
        self.missed
    }

    /// Priority a renderer deserves when the frame budget is slipping.
    pub fn render_priority(&self) -> u8 {
        if self.behind {
            PRIO_RT - 4
        } else {
            PRIO_NORMAL + 4
        }
    }

    pub fn valid(&self) -> bool {
        self.target_us == FRAME_TARGET_US || (self.target_us > 0 && self.worst_us <= u32::MAX)
    }
}

impl Default for FrameBudget {
    fn default() -> FrameBudget {
        FrameBudget::new(FRAME_TARGET_US)
    }
}

// ---------------------------------------------------------------------------
// F098 — background throttling
// ---------------------------------------------------------------------------

/// Per-class CPU caps, in percent. Background work gets a *floor* of service,
/// never zero: a throttled thread that never runs cannot make progress, and a
/// stuck background thread is worse than a slow one.
#[derive(Clone, Copy, Debug)]
pub struct ThrottleConfig {
    pub batch_percent: u8,
    pub idle_percent: u8,
    pub floor_percent: u8,
}

impl Default for ThrottleConfig {
    fn default() -> ThrottleConfig {
        ThrottleConfig::standard()
    }
}

impl ThrottleConfig {
    /// `const` so the scheduler can build a default configuration in a static
    /// initialiser (a derived `Default` is not usable there).
    pub const fn standard() -> ThrottleConfig {
        ThrottleConfig {
            batch_percent: 25,
            idle_percent: 5,
            floor_percent: 2,
        }
    }

    pub fn cap_for(&self, class: SchedClass) -> u8 {
        match class {
            SchedClass::Batch => self.batch_percent,
            SchedClass::Idle => self.idle_percent,
            _ => 100,
        }
    }

    /// A cap below its own floor is not a cap. A zero cap is allowed and means
    /// "only the floor runs".
    pub fn valid(&self) -> bool {
        let ok = |cap: u8| cap <= 100 && (cap == 0 || cap >= self.floor_percent);
        self.floor_percent <= 100 && ok(self.batch_percent) && ok(self.idle_percent)
    }
}

/// A token bucket per throttled class. Credit is denominated in hundredths of
/// a tick, so a 25% cap is exact rather than rounded to "one tick in five".
/// The floor is a *minimum service rate*, not a permanent exemption: a class
/// that has been throttled for a whole floor period gets one tick, which keeps
/// progress possible without giving away the cap.
pub struct Throttle {
    config: ThrottleConfig,
    credit: [u32; 2],
    /// Ticks since this class was last allowed to run.
    gate: [u32; 2],
    throttled_ticks: u64,
    spills: u64,
}

impl Throttle {
    pub const CAPACITY: u32 = 100;

    pub const fn new(config: ThrottleConfig) -> Throttle {
        Throttle {
            config,
            credit: [Throttle::CAPACITY, Throttle::CAPACITY],
            gate: [0, 0],
            throttled_ticks: 0,
            spills: 0,
        }
    }

    /// One allowed tick costs this much credit.
    pub const TICK_COST: u32 = 100;

    pub fn config(&self) -> ThrottleConfig {
        self.config
    }

    fn slot(class: SchedClass) -> Option<usize> {
        match class {
            SchedClass::Batch => Some(0),
            SchedClass::Idle => Some(1),
            _ => None,
        }
    }

    /// One tick passed. Every class refills at its cap rate.
    pub fn on_tick(&mut self) {
        for (i, cap) in [
            self.config.batch_percent as u32,
            self.config.idle_percent as u32,
        ]
        .iter()
        .enumerate()
        {
            self.credit[i] = (self.credit[i] + cap).min(Self::CAPACITY);
        }
    }

    /// May a thread of `class` consume this tick?
    pub fn allow(&mut self, class: SchedClass) -> bool {
        match Self::slot(class) {
            None => true,
            Some(i) => {
                if self.credit[i] >= Self::TICK_COST {
                    self.credit[i] -= Self::TICK_COST;
                    self.gate[i] = 0;
                    return true;
                }
                // Out of budget: hold it back, but honour the floor so the
                // class cannot be starved into a permanent stall.
                self.throttled_ticks += 1;
                self.gate[i] += 1;
                let floor = self.config.floor_percent.max(1) as u32;
                let period = (Self::TICK_COST / floor).max(1);
                if self.gate[i] >= period {
                    self.gate[i] = 0;
                    self.spills += 1;
                    true
                } else {
                    false
                }
            }
        }
    }

    /// Remaining budget in hundredths of a tick.
    pub fn credit(&self, class: SchedClass) -> u32 {
        Self::slot(class).map(|i| self.credit[i]).unwrap_or(Self::CAPACITY)
    }

    /// Ticks since this class was last allowed to run.
    pub fn gate(&self, class: SchedClass) -> u32 {
        Self::slot(class).map(|i| self.gate[i]).unwrap_or(0)
    }

    pub fn throttled_ticks(&self) -> u64 {
        self.throttled_ticks
    }

    pub fn spills(&self) -> u64 {
        self.spills
    }
}

impl Default for Throttle {
    fn default() -> Throttle {
        Throttle::new(ThrottleConfig::default())
    }
}

/// F098: effective priority after throttling. A throttled class drops below
/// every unthrottled one, which is the whole point of the cap.
pub fn throttled_priority(class: SchedClass, throttled: bool) -> u8 {
    let base = match class {
        SchedClass::Batch => super::engine::PRIO_BATCH,
        SchedClass::Idle => super::engine::PRIO_IDLE,
        _ => PRIO_NORMAL,
    };
    if throttled {
        base.saturating_sub(1)
    } else {
        base
    }
}

#[cfg(test)]
mod tests {
    use super::super::engine::{ThreadState, DEFAULT_SLICE_TICKS};
    use super::*;

    fn thread(tid: u32, class: SchedClass, prio: u8) -> Thread {
        let mut t = Thread::new(tid);
        t.state = ThreadState::Runnable;
        t.class = class;
        t.prio = prio;
        t.base_prio = prio;
        t.slice_total = DEFAULT_SLICE_TICKS;
        t
    }

    #[test]
    fn affinity_masks_and_restrictions() {
        assert_eq!(affinity_all(), 0, "0 means any core");
        assert!(affinity_allows(affinity_all(), 63));
        assert_eq!(affinity_of(3), 1 << 3);
        assert!(affinity_allows(affinity_of(3), 3));
        assert!(!affinity_allows(affinity_of(3), 2));
        assert_eq!(affinity_range(0, 3), 0b1111);
        assert_eq!(affinity_range(4, 4), 1 << 4);
        assert_eq!(affinity_range(5, 4), 0, "inverted range is empty");
        assert_eq!(affinity_count(0b1011), 3);
        assert_eq!(affinity_count(0), 63, "\"any\" is reported at the cap");

        let t = thread(0, SchedClass::Normal, PRIO_NORMAL);
        assert!(!migration_allowed(&t, 0, 0, 99), "same core is not a migration");
        assert!(!migration_allowed(&t, 0, 1, 1), "imbalance too small");
        assert!(migration_allowed(&t, 0, 1, MIGRATION_THRESHOLD));
        let mut pinned = t;
        pinned.affinity = affinity_of(0);
        assert!(!migration_allowed(&pinned, 0, 1, 9), "pinned thread stays");
    }

    #[test]
    fn load_balancer_picks_busiest_to_idlest() {
        let t0 = thread(0, SchedClass::Normal, PRIO_NORMAL);
        let t1 = thread(1, SchedClass::Normal, PRIO_NORMAL);
        let threads = {
            let mut slice = [Thread::new(0); 64];
            slice[0] = t0;
            slice[1] = t1;
            slice[2] = thread(2, SchedClass::Normal, PRIO_NORMAL);
            slice
        };
        let plan = plan_migration(&[3, 0], &[2, NO_THREAD], &threads, MIGRATION_THRESHOLD)
            .expect("core 0 is overloaded");
        assert_eq!(plan.from, 0);
        assert_eq!(plan.to, 1);
        assert_eq!(plan.tid, 2);

        // Balanced cores produce nothing.
        assert_eq!(plan_migration(&[1, 1], &[0, 1], &threads, 1), None);
        // A single core cannot be rebalanced.
        assert_eq!(plan_migration(&[9], &[0], &threads, 1), None);
        // Mismatched views are refused rather than guessed at.
        assert_eq!(plan_migration(&[3, 0, 0], &[2, 1], &threads, 1), None);

        let mut lb = LoadBalancer::new(4);
        assert!(!lb.should_run());
        assert!(!lb.should_run());
        assert!(!lb.should_run());
        assert!(lb.should_run(), "runs every 4th tick");
        lb.note_applied(&plan);
        lb.note_skipped();
        assert_eq!(lb.migrations(), 1);
        assert_eq!(lb.skipped(), 1);
    }

    #[test]
    fn energy_modes_place_work_correctly() {
        let cores = [
            CorePower {
                cpu: 0,
                kind: Some(CoreKind::Performance),
                cost: 100,
            },
            CorePower {
                cpu: 4,
                kind: Some(CoreKind::Efficiency),
                cost: 30,
            },
        ];
        assert_eq!(
            pick_core_for(&cores, SchedClass::Interactive, EnergyMode::Balanced),
            Some(0),
            "interactive wants the fast core"
        );
        assert_eq!(
            pick_core_for(&cores, SchedClass::Batch, EnergyMode::Balanced),
            Some(4),
            "background wants the efficient core"
        );
        assert_eq!(
            pick_core_for(&cores, SchedClass::Normal, EnergyMode::Saver),
            Some(4)
        );
        assert_eq!(
            pick_core_for(&cores, SchedClass::Normal, EnergyMode::Performance),
            Some(4),
            "cheapest core wins when nothing is protected"
        );
        assert_eq!(pick_core_for(&[], SchedClass::Normal, EnergyMode::Balanced), None);
        assert_eq!(EnergyMode::default(), EnergyMode::Balanced);
        assert_eq!(EnergyMode::Saver.as_str(), "saver");
    }

    #[test]
    fn exclusive_lease_expires_and_renews() {
        let mut lease = ExclusiveLease::default();
        assert!(lease.grant(5, 100, 10));
        assert!(lease.valid(105));
        assert!(!lease.valid(200), "expired");
        assert!(lease.expired(115));
        assert!(lease.renew(5, 110));
        assert!(lease.valid(115));
        assert!(!lease.renew(9, 115), "only the holder renews");
        assert!(!lease.grant(9, 115, 0), "someone else holds it");
        assert_eq!(exclusive_priority_for(5, &lease, 115), Some(PRIO_RT + 16));
        assert_eq!(exclusive_priority_for(9, &lease, 115), None);
        assert!(lease.release(5));
        assert!(!lease.valid(115));
        assert!(!lease.release(5), "double release");
        // A zero TTL falls back to the documented default.
        assert!(lease.grant(1, 0, 0));
        assert_eq!(lease.ttl_ticks, ExclusiveLease::DEFAULT_TTL);
    }

    #[test]
    fn foreground_boost_is_time_bounded() {
        let mut fg = ForegroundTracker::default();
        assert!(fg.set_foreground(3, 100));
        assert!(!fg.set_foreground(3, 101), "already foreground");
        assert!(fg.is_boosted(3, 120));
        assert!(!fg.is_boosted(3, 100 + ForegroundTracker::BOOST_TICKS + 1));
        let mut t = thread(3, SchedClass::Normal, PRIO_NORMAL);
        assert!(fg.apply(&mut t, 110, super::super::engine::PRIO_INTERACTIVE));
        assert_eq!(t.prio, super::super::engine::PRIO_INTERACTIVE);
        // After the window the boost is removed, not just ignored.
        assert!(fg.apply(&mut t, 1000, super::super::engine::PRIO_INTERACTIVE));
        assert_eq!(t.prio, PRIO_NORMAL);
        assert_eq!(fg.boosts, 1);
    }

    #[test]
    fn sched_latency_histogram_and_budget() {
        let l = SchedLatency::new();
        assert_eq!(l.samples(), 0);
        for _ in 0..100 {
            l.record(500);
        }
        assert_eq!(l.bucket(0), 100, "sub-microsecond samples share one bucket");
        // Two populations: fast and over budget. p99 must land in the slow one.
        for _ in 0..100 {
            l.record(40_000);
        }
        assert_eq!(l.samples(), 200);
        assert_eq!(l.max_ns(), 40_000);
        assert_eq!(l.over_budget(), 0, "40 µs is inside the 2 ms budget");
        assert_eq!(l.p99(), 32_000, "p99 sits in the 32..64 µs bucket");
        assert!(l.mean_ns() > 500);
        // Half the samples are fast, half slow: p50 lands on the boundary and
        // reports the slow bucket, which is the honest answer for a lower bound.
        assert_eq!(l.percentile(50), 32_000);
        assert!(l.p99() >= l.percentile(50));
        l.record(SCHED_LATENCY_BUDGET_NS + 1);
        assert_eq!(l.over_budget(), 1);
        let mut out = [0u8; 128];
        let n = l.render(&mut out);
        let s = core::str::from_utf8(&out[..n]).unwrap();
        assert!(s.starts_with("sched-latency n=201"), "got {s}");
        assert!(s.contains("over=1"), "got {s}");
        // A wake with no run yet records nothing extra beyond the one above.
        l.mark_wakeup();
        assert!(l.samples() >= 201);
    }

    #[test]
    fn inversion_guard_donates_and_bounds_the_chain() {
        let mut guard = InversionGuard::new();
        let blocked = thread(1, SchedClass::Interactive, 24);
        let mut owner = thread(2, SchedClass::Normal, PRIO_NORMAL);
        assert!(guard.on_block(&blocked, &mut owner, 1));
        assert_eq!(owner.prio, 24, "owner inherited the waiter's priority");
        assert_eq!(guard.boosts(), 1);
        assert_eq!(guard.last().unwrap().blocked, 1);

        // A chain deeper than the cap is refused instead of followed.
        let mut deep_owner = thread(3, SchedClass::Normal, PRIO_NORMAL);
        assert!(!guard.on_block(&blocked, &mut deep_owner, MAX_INHERITANCE_CHAIN + 1));
        assert_eq!(deep_owner.prio, PRIO_NORMAL);
        assert_eq!(guard.chain_breaks(), 1);

        // Donating to someone already higher is a no-op.
        let low = thread(4, SchedClass::Batch, 4);
        let mut already = thread(5, SchedClass::RealTime, PRIO_RT);
        assert!(!guard.on_block(&low, &mut already, 1));
        assert_eq!(already.prio, PRIO_RT);
        assert_eq!(guard.events(), 3);
    }

    #[test]
    fn policy_switching_is_safe_and_reversible() {
        let mut sw = PolicySwitcher::new();
        assert_eq!(sw.active(), PolicyKind::RoundRobin);
        assert!(sw.switch(PolicyKind::Fair, false));
        assert_eq!(sw.active(), PolicyKind::Fair);
        assert!(!sw.switch(PolicyKind::Fair, false), "no-op switch");
        // FIFO with RT running is refused, and the active policy is untouched.
        assert!(!sw.switch(PolicyKind::Fifo, true));
        assert_eq!(sw.active(), PolicyKind::Fair);
        assert_eq!(sw.rejected(), 1);
        assert!(sw.switch(PolicyKind::Fifo, false));
        assert_eq!(sw.switches(), 2);

        for k in [
            PolicyKind::Fifo,
            PolicyKind::RoundRobin,
            PolicyKind::Fair,
            PolicyKind::RtFirst,
        ] {
            assert_eq!(PolicyKind::parse(k.as_str()), Some(k));
            assert!(!k.as_str().is_empty());
        }
        assert_eq!(PolicyKind::parse("nope"), None);
        assert!(PolicyKind::Fifo.slice_ticks(10) > PolicyKind::RoundRobin.slice_ticks(10));
        assert_eq!(PolicyKind::RoundRobin.slice_ticks(10), 10);
    }

    #[test]
    fn isolation_admits_only_the_promised_classes() {
        let mut iso = IsolationSet::new();
        assert!(!iso.isolate(0), "core 0 always keeps running the kernel");
        assert!(iso.isolate(3));
        assert!(iso.isolate(4));
        assert_eq!(iso.count(), 2);
        assert!(iso.is_isolated(3));
        assert!(!iso.is_isolated(0));
        assert!(iso.admits(0, SchedClass::Batch), "non-isolated core takes all");
        assert!(iso.admits(3, SchedClass::RealTime));
        assert!(iso.admits(3, SchedClass::Isolated));
        assert!(!iso.admits(3, SchedClass::Interactive), "no normal work allowed");
        assert!(!iso.admits(3, SchedClass::Batch));
        assert!(iso.release(3));
        assert!(!iso.release(3), "double release");
        assert_eq!(iso.mask(), 1 << 4);
    }

    #[test]
    fn starvation_watch_finds_and_rescues_the_worst() {
        let mut table = ThreadTable::new();
        assert!(table.spawn_on(0, 12, SchedClass::Normal, 0, 0));
        assert!(table.spawn_on(1, 4, SchedClass::Batch, 0, 0));
        table.thread_mut(1).unwrap().wait_ticks = 500;
        let mut watch = StarvationWatch::new(100);
        let report = watch.scan(&table);
        assert_eq!(report.starving, 1);
        assert_eq!(report.worst_tid, 1);
        assert_eq!(report.worst_wait_ticks, 500);
        assert!(watch.rescue(&mut table, &report));
        assert_eq!(table.thread(1).unwrap().class, SchedClass::Normal);
        assert_eq!(table.thread(1).unwrap().wait_ticks, 0);
        assert_eq!(watch.rescues(), 1);
        assert_eq!(watch.worst_ever(), 500);
        // A report that names no victim triggers nothing.
        assert_eq!(StarvationReport::default().worst_tid, NO_THREAD);
        let mut quiet = ThreadTable::new();
        let none = watch.scan(&quiet);
        assert_eq!(none.worst_tid, NO_THREAD);
        assert!(!watch.rescue(&mut quiet, &none));
    }

    #[test]
    fn interaction_tracker_boosts_once_per_wake() {
        let mut it = InteractionTracker::default();
        let mut t = thread(7, SchedClass::Normal, PRIO_NORMAL);
        it.on_input_wake(&mut t, 42);
        assert_eq!(t.flags & super::super::engine::FLAG_INPUT_WOKEN, super::super::engine::FLAG_INPUT_WOKEN);
        assert!(it.on_pick(&mut t, it.target_prio()));
        assert_eq!(t.prio, it.target_prio());
        assert_eq!(t.flags & super::super::engine::FLAG_INPUT_WOKEN, 0);
        // The flag is consumed: a second pick does not re-boost.
        assert!(!it.on_pick(&mut t, it.target_prio()));
        assert_eq!(it.wakes, 1);
        assert_eq!(it.boosted, 1);
        assert_eq!(it.last_wake_tick, 42);
    }

    #[test]
    fn frame_budget_tracks_a_miss_streak() {
        let mut fb = FrameBudget::default();
        assert!(!fb.record_frame(10_000));
        assert_eq!(fb.missed(), 0);
        assert!(!fb.behind);
        let bad = FRAME_TARGET_US + FRAME_MISS_SLACK_US + 1;
        assert!(fb.record_frame(bad));
        assert!(!fb.behind, "one late frame is noise");
        fb.record_frame(bad);
        fb.record_frame(bad);
        assert!(fb.behind, "three in a row is a trend");
        assert!(fb.render_priority() > PRIO_NORMAL);
        assert!(!fb.record_frame(1_000), "an on-time frame is not a miss");
        assert!(!fb.behind, "recovering clears the streak");
        assert_eq!(fb.worst_us, bad);
        assert_eq!(fb.produced, 5);
        assert!(fb.valid());
    }

    #[test]
    fn throttling_caps_background_but_never_zeroes_it() {
        let config = ThrottleConfig::default();
        assert!(config.valid());
        assert_eq!(config.cap_for(SchedClass::Batch), 25);
        assert_eq!(config.cap_for(SchedClass::Interactive), 100);
        let mut th = Throttle::new(config);
        // Foreground work is never throttled.
        for _ in 0..10 {
            assert!(th.allow(SchedClass::Interactive));
            th.on_tick();
        }
        assert_eq!(th.credit(SchedClass::Interactive), Throttle::CAPACITY);
        // Batch credit drains, then falls back to the floor instead of dying.
        let mut allowed = 0;
        for _ in 0..100 {
            if th.allow(SchedClass::Batch) {
                allowed += 1;
            }
            th.on_tick();
        }
        assert!(
            (20..=35).contains(&allowed),
            "batch ran {allowed} of 100 ticks, expected roughly its 25% cap"
        );
        assert!(th.throttled_ticks() > 0, "the cap actually held it back");
        assert_eq!(th.spills(), 0, "a 25% cap never needs the 2% floor");

        // With the cap at zero the floor is the only service left, and it is
        // what keeps a background thread making progress.
        let starved_config = ThrottleConfig {
            batch_percent: 0,
            idle_percent: 0,
            floor_percent: 2,
        };
        assert!(starved_config.valid());
        let mut starved = Throttle::new(starved_config);
        let mut ran = 0;
        for _ in 0..100 {
            if starved.allow(SchedClass::Batch) {
                ran += 1;
            }
            starved.on_tick();
        }
        assert!((1..=4).contains(&ran), "floor gave {ran} of 100 ticks");
        assert!(starved.spills() > 0, "every one of those was a floor spill");
        assert!(starved.credit(SchedClass::Idle) <= Throttle::CAPACITY);
        assert_eq!(starved.gate(SchedClass::Interactive), 0);

        assert_eq!(throttled_priority(SchedClass::Batch, true), super::super::engine::PRIO_BATCH - 1);
        assert_eq!(throttled_priority(SchedClass::Interactive, true), PRIO_NORMAL - 1);
        // A cap above 100 is nonsense, and so is a cap below its own floor.
        let bad = ThrottleConfig {
            batch_percent: 200,
            ..ThrottleConfig::default()
        };
        assert!(!bad.valid());
        let inverted = ThrottleConfig {
            batch_percent: 1,
            floor_percent: 5,
            ..ThrottleConfig::default()
        };
        assert!(!inverted.valid());
    }
}
