//! AI-04 · 调度域（F076~F100）.
//!
//! The scheduler is the last thing to come up and the first thing that never
//! stops: once the tick handler is registered it runs on every interrupt for
//! the rest of the machine's life. `init` therefore ends by handing interrupts
//! over to it, which is also the point where the CPU domain stops keeping them
//! disabled.

pub mod engine;
pub mod policy;

use core::sync::atomic::Ordering;

use engine::ThreadTable;
use engine::{SwitchDecision, NO_THREAD, PRIO_NORMAL, PRIO_TOP};
use policy::{
    EnergyMode, FrameBudget, InteractionTracker, IsolationSet, LoadBalancer, PolicyKind,
    PolicySwitcher, SchedLatency, StarvationWatch, Throttle, ThrottleConfig,
};

/// Everything the rest of the kernel wants to know about scheduling.
#[derive(Clone, Copy, Debug)]
pub struct SchedDomainState {
    pub threads: usize,
    pub switches: u64,
    pub preemptions: u64,
    pub migrations: u64,
    pub policy: PolicyKind,
    pub energy: EnergyMode,
    pub isolated_cores: u32,
    pub latency_p99_us: u64,
    pub latency_over_budget: u64,
    pub starving: usize,
    pub self_test: (usize, usize),
}

impl Default for SchedDomainState {
    fn default() -> SchedDomainState {
        SchedDomainState {
            threads: 0,
            switches: 0,
            preemptions: 0,
            migrations: 0,
            policy: PolicyKind::RoundRobin,
            energy: EnergyMode::Balanced,
            isolated_cores: 0,
            latency_p99_us: 0,
            latency_over_budget: 0,
            starving: 0,
            self_test: (0, 0),
        }
    }
}

impl SchedDomainState {
    pub fn ok(&self) -> bool {
        self.threads > 0 && self.self_test.1 == 0
    }

    pub fn render(&self, out: &mut [u8]) -> usize {
        let mut w = crate::cpu::Hud::new(out);
        w.str("sched threads=");
        w.num(self.threads as u64);
        w.str(" switches=");
        w.num(self.switches);
        w.str(" preempt=");
        w.num(self.preemptions);
        w.str(" migrate=");
        w.num(self.migrations);
        w.str(" policy=");
        w.str(self.policy.as_str());
        w.str(" energy=");
        w.str(self.energy.as_str());
        w.str(" p99=");
        w.num(self.latency_p99_us);
        w.str("us [");
        w.num(self.self_test.0 as u64);
        w.str("/");
        w.num((self.self_test.0 + self.self_test.1) as u64);
        w.str("]\n");
        w.used()
    }
}

/// The live scheduler state. One instance: this is the domain's spine.
pub struct Scheduler {
    pub table: ThreadTable,
    pub balancer: LoadBalancer,
    pub starving: StarvationWatch,
    pub interaction: InteractionTracker,
    pub frames: FrameBudget,
    pub throttle: Throttle,
    pub isolation: IsolationSet,
    pub switcher: PolicySwitcher,
    pub lease: policy::ExclusiveLease,
    pub inversion: policy::InversionGuard,
    pub energy: EnergyMode,
    pub ticks: u64,
}

impl Scheduler {
    pub const fn new() -> Scheduler {
        Scheduler {
            table: ThreadTable::new(),
            balancer: LoadBalancer::new(8),
            starving: StarvationWatch::new(200),
            interaction: InteractionTracker {
                wakes: 0,
                boosted: 0,
                last_wake_tick: 0,
            },
            frames: FrameBudget {
                target_us: policy::FRAME_TARGET_US,
                produced: 0,
                missed: 0,
                worst_us: 0,
                behind: false,
                miss_streak: 0,
            },
            throttle: Throttle::new(ThrottleConfig::standard()),
            isolation: IsolationSet::new(),
            switcher: PolicySwitcher::new(),
            lease: policy::ExclusiveLease {
                holder: 0,
                granted_at: 0,
                ttl_ticks: 0,
                active: false,
            },
            inversion: policy::InversionGuard::new(),
            energy: EnergyMode::Balanced,
            ticks: 0,
        }
    }

    /// F095: how many threads are currently past the starvation threshold.
    pub fn starving_count(&mut self) -> usize {
        self.starving.scan(&self.table).starving
    }

    /// One tick of the whole policy stack, driven by the timer interrupt.
    pub fn tick(&mut self, cpu: u32) -> Option<SwitchDecision> {
        self.ticks += 1;
        self.throttle.on_tick();
        let decision = self.table.on_tick(cpu);

        // F095 — starvation is checked on every tick, but only acted on when
        // the worst offender is genuinely stuck.
        let report = self.starving.scan(&self.table);
        if report.starving > 0 && report.worst_wait_ticks > self.starving.threshold() {
            let _ = self.starving.rescue(&mut self.table, &report);
        }

        // F083 — migration runs at its own cadence, never on every tick.
        if self.balancer.should_run() {
            let mut loads = [0u32; crate::cpu::smp::MAX_CPUS];
            let mut heads = [NO_THREAD; crate::cpu::smp::MAX_CPUS];
            for c in 0..crate::cpu::smp::MAX_CPUS as u32 {
                if let Some(q) = self.table.rq(c) {
                    loads[c as usize] = q.len() as u32;
                    heads[c as usize] = q.peek(self.table.threads()).unwrap_or(NO_THREAD);
                }
            }
            match policy::plan_migration(
                &loads,
                &heads,
                self.table.threads(),
                policy::MIGRATION_THRESHOLD,
            ) {
                Some(plan) => self.balancer.note_applied(&plan),
                None => self.balancer.note_skipped(),
            }
        }
        decision
    }
}

impl Default for Scheduler {
    fn default() -> Scheduler {
        Scheduler::new()
    }
}

static SCHED: crate::cpu::sync::SpinProtected<Scheduler> =
    crate::cpu::sync::SpinProtected::new(Scheduler::new());

pub fn scheduler() -> &'static crate::cpu::sync::SpinProtected<Scheduler> {
    &SCHED
}

/// Thread ids the boot sequence hands out. 0 is idle; 1..8 are reserved for the
/// per-domain worker threads that arrive with their own subsystems.
pub const TID_IDLE: u32 = 0;
pub const TID_BOOT: u32 = 1;
pub const FIRST_FREE_TID: u32 = 8;

/// F076~F100 bring-up.
pub fn init() -> SchedDomainState {
    let mut s = SCHED.lock();

    // F076 — an idle thread and the boot thread. The idle thread is what the
    // run queue falls back to; the boot thread is this execution context.
    let _ = s.table.spawn_on(
        TID_IDLE,
        engine::PRIO_IDLE,
        engine::SchedClass::Idle,
        engine::FLAG_KERNEL,
        0,
    );
    let _ = s.table.spawn_on(
        TID_BOOT,
        PRIO_NORMAL,
        engine::SchedClass::Normal,
        engine::FLAG_KERNEL | engine::FLAG_FOREGROUND,
        0,
    );
    s.table.set_preemptible(true);
    s.switcher.switch(PolicyKind::RoundRobin, false);
    s.energy = EnergyMode::Balanced;

    // Drive the whole policy from the timer tick. Registered last so a fault in
    // the scheduler cannot leave the clock half-initialised.
    let _ = crate::cpu::clock::on_tick(tick_from_irq);

    let (passed, failed) = engine::run_scheduler_checks(&s.table);
    let latency = policy::latency();
    let state = SchedDomainState {
        threads: s.table.len(),
        switches: engine::stats().switches.load(Ordering::Relaxed),
        preemptions: engine::stats().preemptions.load(Ordering::Relaxed),
        migrations: engine::stats().migrations.load(Ordering::Relaxed),
        policy: s.switcher.active(),
        energy: s.energy,
        isolated_cores: s.isolation.count(),
        latency_p99_us: latency.p99() / 1000,
        latency_over_budget: latency.over_budget(),
        starving: 0,
        self_test: (passed, failed),
    };
    drop(s);

    crate::kinfo!(
        "sched: {} threads, policy={} energy={} latency budget {}us, self-test {}/{}",
        state.threads,
        state.policy.as_str(),
        state.energy.as_str(),
        policy::SCHED_LATENCY_BUDGET_NS / 1000,
        state.self_test.0,
        state.self_test.0 + state.self_test.1
    );

    // The scheduler owns the tick, so it also owns the moment interrupts become
    // useful. From here the machine is live.
    crate::cpu::enable_interrupts();
    state
}

/// Tick handler invoked from the interrupt path. Kept as a plain `fn` so it can
/// be registered as a function pointer (no closures, no allocation).
fn tick_from_irq(_n: u64) {
    let cpu = crate::cpu::smp::current_id();
    let decision = scheduler().lock().tick(cpu);
    if let Some(d) = decision {
        crate::ktrace!("switch {} -> {} on cpu {}", d.from, d.to, d.cpu);
        // F087 — a thread finally got the CPU after being woken.
        policy::latency().record_run();
        let mut s = scheduler().lock();
        let target = s.interaction.target_prio();
        if let Some(t) = s.table.thread(d.to) {
            let mut copy = *t;
            if s.interaction.on_pick(&mut copy, target) {
                if let Some(slot) = s.table.thread_mut(d.to) {
                    *slot = copy;
                }
            }
        }
    }
}

/// Render the domain HUD onto the console.
pub fn render_to_console(st: &SchedDomainState) {
    let mut buf = [0u8; 256];
    let n = st.render(&mut buf);
    if let Some(c) = crate::console::installed_ref() {
        for &b in &buf[..n] {
            c.put_byte(b);
        }
    }
    let mut detail = [0u8; 320];
    let m = policy::latency().render(&mut detail);
    if let Some(c) = crate::console::installed_ref() {
        for &b in &detail[..m] {
            c.put_byte(b);
        }
    }
    let mut fails = [0u8; 512];
    let f = engine::sched_selftest().render(&mut fails);
    if let Some(c) = crate::console::installed_ref() {
        for &b in &fails[..f] {
            c.put_byte(b);
        }
    }
}

/// The current snapshot, for the shell / diagnostic dump (F072, F487).
pub fn snapshot() -> SchedDomainState {
    let mut s = SCHED.lock();
    let latency: &SchedLatency = policy::latency();
    SchedDomainState {
        threads: s.table.len(),
        switches: engine::stats().switches.load(Ordering::Relaxed),
        preemptions: engine::stats().preemptions.load(Ordering::Relaxed),
        migrations: engine::stats().migrations.load(Ordering::Relaxed),
        policy: s.switcher.active(),
        energy: s.energy,
        isolated_cores: s.isolation.count(),
        latency_p99_us: latency.p99() / 1000,
        latency_over_budget: latency.over_budget(),
        starving: s.starving_count(),
        self_test: engine::sched_selftest().tally(),
    }
}

/// F092/F093: pin a thread to a core and reserve that core for the critical
/// path. Exposed for the audio/input domains when they arrive.
pub fn isolate_for(tid: u32, cpu: u32) -> bool {
    let mut s = SCHED.lock();
    if !s.isolation.isolate(cpu) {
        return false;
    }
    match s.table.thread_mut(tid) {
        Some(t) => {
            t.affinity = policy::affinity_of(cpu);
            t.cpu = cpu;
            t.class = engine::SchedClass::Isolated;
            t.prio = t.prio.min(PRIO_TOP);
            true
        }
        None => false,
    }
}
