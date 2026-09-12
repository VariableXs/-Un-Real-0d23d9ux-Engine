//! F043 自旋锁体系 / F044 优先级继承锁 / F045 中断嵌套策略 / F046 关中断窗口审计.
//!
//! Four answers to the same question: "how long can this core stop making
//! progress?" Queued spin locks keep waiters fair, the PI mutex keeps a low
//! priority holder from blocking a high priority waiter, the nesting policy
//! decides which interrupt may preempt which, and the window audit turns
//! "interrupts were off for too long" from a feeling into a number.

use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

// ---------------------------------------------------------------------------
// F043 — queued (ticket) spin lock
// ---------------------------------------------------------------------------

/// A FIFO ticket lock. Unlike a bare `test_and_set` spin this cannot starve a
/// waiter and it does not thunder: waiters spin on their own ticket, not on a
/// shared word, so releasing invalidates exactly one cache line.
pub struct SpinLock {
    /// Ticket handed to the next arrival.
    next: AtomicU32,
    /// Ticket currently owning the lock.
    owner: AtomicU32,
    /// Contention counter — the load-balancer reads this (F083).
    contentions: AtomicU64,
    /// Longest observed hold time, in TSC ticks.
    longest_hold: AtomicU64,
}

impl SpinLock {
    pub const fn new() -> SpinLock {
        SpinLock {
            next: AtomicU32::new(0),
            owner: AtomicU32::new(0),
            contentions: AtomicU64::new(0),
            longest_hold: AtomicU64::new(0),
        }
    }

    /// Take the ticket. Spins with `core::hint::spin_loop()`.
    pub fn lock(&self) -> SpinGuard<'_> {
        let ticket = self.next.fetch_add(1, Ordering::Relaxed);
        if ticket != self.owner.load(Ordering::Relaxed) {
            self.contentions.fetch_add(1, Ordering::Relaxed);
        }
        let start = crate::cpu::clock::read_tsc();
        while self.owner.load(Ordering::Acquire) != ticket {
            core::hint::spin_loop();
        }
        let held = crate::cpu::clock::read_tsc().wrapping_sub(start);
        self.longest_hold.fetch_max(held, Ordering::Relaxed);
        SpinGuard { lock: self }
    }

    /// Non-blocking attempt.
    pub fn try_lock(&self) -> Option<SpinGuard<'_>> {
        let cur = self.owner.load(Ordering::Acquire);
        if self
            .next
            .compare_exchange(cur, cur + 1, Ordering::AcqRel, Ordering::Relaxed)
            .is_ok()
        {
            Some(SpinGuard { lock: self })
        } else {
            None
        }
    }

    pub fn is_locked(&self) -> bool {
        self.owner.load(Ordering::Acquire) != self.next.load(Ordering::Acquire)
    }

    pub fn contentions(&self) -> u64 {
        self.contentions.load(Ordering::Relaxed)
    }

    pub fn longest_hold_ticks(&self) -> u64 {
        self.longest_hold.load(Ordering::Relaxed)
    }

    fn unlock(&self) {
        self.owner.fetch_add(1, Ordering::Release);
    }
}

impl Default for SpinLock {
    fn default() -> SpinLock {
        SpinLock::new()
    }
}

pub struct SpinGuard<'a> {
    lock: &'a SpinLock,
}

impl Drop for SpinGuard<'_> {
    fn drop(&mut self) {
        self.lock.unlock();
    }
}

/// A data cell guarded by a queued spin lock.
pub struct SpinProtected<T> {
    lock: SpinLock,
    data: core::cell::UnsafeCell<T>,
}

// SAFETY: `T` is only reachable through `lock()`, which serialises access.
unsafe impl<T: Send> Sync for SpinProtected<T> {}
unsafe impl<T: Send> Send for SpinProtected<T> {}

impl<T> SpinProtected<T> {
    pub const fn new(data: T) -> SpinProtected<T> {
        SpinProtected {
            lock: SpinLock::new(),
            data: core::cell::UnsafeCell::new(data),
        }
    }

    pub fn lock(&self) -> SpinDataGuard<'_, T> {
        SpinDataGuard {
            _guard: self.lock.lock(),
            data: unsafe { &mut *self.data.get() },
        }
    }

    pub fn contentions(&self) -> u64 {
        self.lock.contentions()
    }
}

pub struct SpinDataGuard<'a, T> {
    _guard: SpinGuard<'a>,
    data: &'a mut T,
}

impl<T> core::ops::Deref for SpinDataGuard<'_, T> {
    type Target = T;
    fn deref(&self) -> &T {
        self.data
    }
}

impl<T> core::ops::DerefMut for SpinDataGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        self.data
    }
}

// ---------------------------------------------------------------------------
// F044 — priority inheritance mutex
// ---------------------------------------------------------------------------

pub const PRIO_IDLE: u8 = 0;
pub const PRIO_MIN: u8 = 1;
pub const PRIO_NORMAL: u8 = 8;
pub const PRIO_RT: u8 = 32;
pub const PRIO_MAX: u8 = 63;
/// Number of waiters tracked for inheritance (bounded: no allocation in IRQs).
pub const MAX_PI_WAITERS: usize = 8;

/// A blocking lock that hands its holder the priority of its hottest waiter.
pub struct PiMutex {
    /// Owning thread id, `u32::MAX` when free.
    owner: AtomicU32,
    /// The owner's *original* priority, restored on release.
    saved_prio: AtomicU32,
    /// Highest priority currently donated to the owner.
    effective: AtomicU32,
    waiters: [AtomicU32; MAX_PI_WAITERS],
    boosts: AtomicU64,
    inversions_blocked: AtomicU64,
}

impl PiMutex {
    pub const fn new() -> PiMutex {
        PiMutex {
            owner: AtomicU32::new(u32::MAX),
            saved_prio: AtomicU32::new(u32::MAX),
            effective: AtomicU32::new(u32::MAX),
            waiters: [const { AtomicU32::new(u32::MAX) }; MAX_PI_WAITERS],
            boosts: AtomicU64::new(0),
            inversions_blocked: AtomicU64::new(0),
        }
    }

    pub fn is_free(&self) -> bool {
        self.owner.load(Ordering::Acquire) == u32::MAX
    }

    pub fn owner(&self) -> Option<u32> {
        let o = self.owner.load(Ordering::Acquire);
        if o == u32::MAX {
            None
        } else {
            Some(o)
        }
    }

    /// Acquire for `tid` at `prio`. When the lock is held, `tid` is recorded as
    /// a waiter and the owner inherits the higher of the two priorities.
    pub fn acquire(&self, tid: u32, prio: u8) -> bool {
        match self.owner.compare_exchange(
            u32::MAX,
            tid,
            Ordering::AcqRel,
            Ordering::Relaxed,
        ) {
            Ok(_) => {
                self.saved_prio.store(prio as u32, Ordering::Relaxed);
                self.effective.store(prio as u32, Ordering::Relaxed);
                true
            }
            Err(_) => {
                if self.record_waiter(tid, prio) {
                    let cur = self.effective.load(Ordering::Acquire);
                    if cur == u32::MAX || prio as u32 > cur {
                        self.effective.store(prio as u32, Ordering::Release);
                        self.boosts.fetch_add(1, Ordering::Relaxed);
                    }
                    self.inversions_blocked.fetch_add(1, Ordering::Relaxed);
                }
                false
            }
        }
    }

    fn record_waiter(&self, tid: u32, prio: u8) -> bool {
        let packed = ((prio as u32) << 24) | (tid & 0x00FF_FFFF);
        for slot in self.waiters.iter() {
            if slot
                .compare_exchange(u32::MAX, packed, Ordering::AcqRel, Ordering::Relaxed)
                .is_ok()
            {
                return true;
            }
        }
        false
    }

    /// Priority the owner should now run at (the donated one).
    pub fn effective_priority(&self) -> Option<u8> {
        let e = self.effective.load(Ordering::Acquire);
        if e == u32::MAX {
            None
        } else {
            Some(e as u8)
        }
    }

    /// Release and hand the lock to the highest-priority waiter.
    pub fn release(&self, tid: u32) -> Option<u32> {
        if self.owner.load(Ordering::Acquire) != tid {
            return None;
        }
        let mut best: Option<(u8, u32)> = None;
        let mut best_slot = 0usize;
        for (i, slot) in self.waiters.iter().enumerate() {
            let v = slot.load(Ordering::Acquire);
            if v == u32::MAX {
                continue;
            }
            let prio = (v >> 24) as u8;
            let id = v & 0x00FF_FFFF;
            if best.map(|(p, _)| prio > p).unwrap_or(true) {
                best = Some((prio, id));
                best_slot = i;
            }
        }
        match best {
            Some((prio, id)) => {
                self.waiters[best_slot].store(u32::MAX, Ordering::Release);
                self.owner.store(id, Ordering::Release);
                self.saved_prio.store(prio as u32, Ordering::Relaxed);
                // Remaining waiters still donate their priority.
                let rest = self
                    .waiters
                    .iter()
                    .map(|s| s.load(Ordering::Acquire))
                    .filter(|v| *v != u32::MAX)
                    .map(|v| (v >> 24) as u8)
                    .max();
                self.effective
                    .store(rest.unwrap_or(prio) as u32, Ordering::Release);
                Some(id)
            }
            None => {
                self.owner.store(u32::MAX, Ordering::Release);
                self.effective.store(u32::MAX, Ordering::Release);
                self.saved_prio.store(u32::MAX, Ordering::Relaxed);
                None
            }
        }
    }

    pub fn boosts(&self) -> u64 {
        self.boosts.load(Ordering::Relaxed)
    }

    pub fn inversions_blocked(&self) -> u64 {
        self.inversions_blocked.load(Ordering::Relaxed)
    }
}

impl Default for PiMutex {
    fn default() -> PiMutex {
        PiMutex::new()
    }
}

// ---------------------------------------------------------------------------
// F045 — interrupt nesting policy
// ---------------------------------------------------------------------------

/// How deep interrupts may nest. Beyond this the kernel drops to "queue it" —
/// an unbounded nesting depth is how interrupt stacks get exhausted.
pub const MAX_NESTING: u32 = 3;

/// A policy: an incoming interrupt only nests when it is strictly more urgent
/// than what is already running.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NestingPolicy {
    /// Everything serialises (safest; used while probing hardware).
    Flat,
    /// Only higher-priority vectors nest.
    Priority,
    /// Anything may nest up to `MAX_NESTING`.
    Free,
}

pub struct Nesting {
    policy: AtomicU32,
    max_depth: AtomicU32,
    violations: AtomicU64,
    deepest: AtomicU32,
}

impl Nesting {
    pub const fn new() -> Nesting {
        Nesting {
            policy: AtomicU32::new(NestingPolicy::Priority as u32),
            max_depth: AtomicU32::new(MAX_NESTING),
            violations: AtomicU64::new(0),
            deepest: AtomicU32::new(0),
        }
    }

    pub fn policy(&self) -> NestingPolicy {
        match self.policy.load(Ordering::Acquire) {
            0 => NestingPolicy::Flat,
            2 => NestingPolicy::Free,
            _ => NestingPolicy::Priority,
        }
    }

    pub fn set_policy(&self, p: NestingPolicy) {
        self.policy.store(p as u32, Ordering::Release);
    }

    pub fn max_depth(&self) -> u32 {
        self.max_depth.load(Ordering::Relaxed)
    }

    pub fn set_max_depth(&self, d: u32) {
        self.max_depth.store(d, Ordering::Release);
    }

    /// Decide whether `incoming` may preempt the current handler.
    /// `current_prio` is the priority of whatever is running (0 = thread).
    pub fn allows(&self, incoming_prio: u8, current_prio: u8) -> bool {
        self.allows_at_depth(incoming_prio, current_prio, crate::cpu::smp::current().irq_depth())
    }

    /// The same decision for an explicit depth — used by the policy tests and
    /// by the nesting self-check, which must not depend on live IRQ state.
    pub fn allows_at_depth(&self, incoming_prio: u8, current_prio: u8, depth: u32) -> bool {
        if depth >= self.max_depth() {
            self.violations.fetch_add(1, Ordering::Relaxed);
            return false;
        }
        match self.policy() {
            NestingPolicy::Flat => depth == 0,
            NestingPolicy::Free => true,
            NestingPolicy::Priority => depth == 0 || incoming_prio > current_prio,
        }
    }

    pub fn note_depth(&self, depth: u32) {
        self.deepest.fetch_max(depth, Ordering::Relaxed);
    }

    pub fn deepest(&self) -> u32 {
        self.deepest.load(Ordering::Relaxed)
    }

    pub fn violations(&self) -> u64 {
        self.violations.load(Ordering::Relaxed)
    }
}

static NESTING: Nesting = Nesting::new();

pub fn nesting() -> &'static Nesting {
    &NESTING
}

// ---------------------------------------------------------------------------
// F046 — interrupt-off window audit
// ---------------------------------------------------------------------------

/// The red line: no critical section may hold IRQs off longer than this.
/// 20 µs keeps a 1 kHz tick (and therefore audio/USB service) inside budget.
pub const IRQ_OFF_BUDGET_NS: u64 = 20_000;

pub struct IrqOffAudit {
    longest_ns: AtomicU64,
    total_ns: AtomicU64,
    samples: AtomicU64,
    violations: AtomicU64,
    budget_ns: AtomicU64,
    open: AtomicBool,
    opened_at: AtomicU64,
}

impl IrqOffAudit {
    pub const fn new() -> IrqOffAudit {
        IrqOffAudit {
            longest_ns: AtomicU64::new(0),
            total_ns: AtomicU64::new(0),
            samples: AtomicU64::new(0),
            violations: AtomicU64::new(0),
            budget_ns: AtomicU64::new(IRQ_OFF_BUDGET_NS),
            open: AtomicBool::new(false),
            opened_at: AtomicU64::new(0),
        }
    }

    /// Close the window; returns the token handed back to `end`.
    pub fn begin(&self) -> u64 {
        let t = crate::cpu::clock::read_tsc();
        self.opened_at.store(t, Ordering::Relaxed);
        self.open.store(true, Ordering::Release);
        disable_irqs();
        t
    }

    /// Reopen interrupts and account for the closed period.
    pub fn end(&self, _token: u64) -> u64 {
        let t = crate::cpu::clock::read_tsc();
        let start = self.opened_at.load(Ordering::Relaxed);
        enable_irqs();
        self.open.store(false, Ordering::Release);
        let cal = crate::cpu::clock::calibration();
        let ns = if cal.usable() {
            cal.tsc_to_ns(t.wrapping_sub(start))
        } else {
            0
        };
        self.note(ns)
    }

    /// Account for one measured window. Split out from `end` so the accounting
    /// (max/mean/violations) is testable without a real TSC behind it.
    pub fn note(&self, ns: u64) -> u64 {
        self.longest_ns.fetch_max(ns, Ordering::Relaxed);
        self.total_ns.fetch_add(ns, Ordering::Relaxed);
        self.samples.fetch_add(1, Ordering::Relaxed);
        if ns > self.budget_ns.load(Ordering::Relaxed) {
            self.violations.fetch_add(1, Ordering::Relaxed);
        }
        ns
    }

    pub fn longest_ns(&self) -> u64 {
        self.longest_ns.load(Ordering::Relaxed)
    }

    pub fn mean_ns(&self) -> u64 {
        let s = self.samples.load(Ordering::Relaxed);
        if s == 0 {
            0
        } else {
            self.total_ns.load(Ordering::Relaxed) / s
        }
    }

    pub fn samples(&self) -> u64 {
        self.samples.load(Ordering::Relaxed)
    }

    pub fn violations(&self) -> u64 {
        self.violations.load(Ordering::Relaxed)
    }

    pub fn budget_ns(&self) -> u64 {
        self.budget_ns.load(Ordering::Relaxed)
    }

    pub fn set_budget_ns(&self, ns: u64) {
        self.budget_ns.store(ns, Ordering::Release);
    }

    /// F050 gate: no window has blown the budget.
    pub fn within_budget(&self) -> bool {
        self.violations() == 0
    }

    pub fn is_open(&self) -> bool {
        self.open.load(Ordering::Acquire)
    }
}

static IRQ_OFF: IrqOffAudit = IrqOffAudit::new();

pub fn irq_off_audit() -> &'static IrqOffAudit {
    &IRQ_OFF
}

/// Convenience: run `f` with interrupts off and the window measured.
pub fn with_irqs_off<R>(f: impl FnOnce() -> R) -> R {
    let token = irq_off_audit().begin();
    let r = f();
    irq_off_audit().end(token);
    r
}

#[cfg(all(target_arch = "x86_64", target_os = "none"))]
fn disable_irqs() {
    // SAFETY: `cli` is privileged but harmless; it is undone by `end`.
    unsafe { core::arch::asm!("cli", options(nomem, nostack, preserves_flags)) };
}

#[cfg(not(all(target_arch = "x86_64", target_os = "none")))]
fn disable_irqs() {}

#[cfg(all(target_arch = "x86_64", target_os = "none"))]
fn enable_irqs() {
    // SAFETY: mirrors `disable_irqs`.
    unsafe { core::arch::asm!("sti", options(nomem, nostack, preserves_flags)) };
}

#[cfg(not(all(target_arch = "x86_64", target_os = "none")))]
fn enable_irqs() {}

/// F043~F046 bring-up: publish the defaults and wire the tick that samples
/// the deepest nesting seen so far.
pub fn init() {
    nesting().set_policy(NestingPolicy::Priority);
    nesting().set_max_depth(MAX_NESTING);
    let _ = crate::cpu::clock::on_tick(|_| {
        nesting().note_depth(crate::cpu::smp::current().irq_depth());
    });
    crate::kinfo!(
        "sync: spin-locks ready, pi-mutex ready, nesting<= {}, irq-off budget {} ns",
        MAX_NESTING,
        IRQ_OFF_BUDGET_NS
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spin_lock_is_fifo_and_unlocks() {
        let l = SpinLock::new();
        assert!(!l.is_locked());
        {
            let _g = l.lock();
            assert!(l.is_locked());
            // A second lock would deadlock, so only test try_lock here.
            assert!(l.try_lock().is_none());
        }
        assert!(!l.is_locked());
        let g = l.try_lock();
        assert!(g.is_some());
        drop(g);
        assert!(!l.is_locked());
    }

    #[test]
    fn spin_protected_data_is_exclusive() {
        let cell = SpinProtected::new(0u64);
        {
            let mut g = cell.lock();
            *g += 41;
        }
        {
            let mut g = cell.lock();
            *g += 1;
        }
        assert_eq!(*cell.lock(), 42);
    }

    #[test]
    fn spin_lock_is_mutually_exclusive_under_threads() {
        // Multi-core smoke test: 8 threads × 1000 increments must not lose one.
        let cell = SpinProtected::new(0u64);
        std::thread::scope(|s| {
            for _ in 0..8 {
                s.spawn(|| {
                    for _ in 0..1000 {
                        let mut g = cell.lock();
                        *g += 1;
                    }
                });
            }
        });
        assert_eq!(*cell.lock(), 8000);
        // 争用只有在多核（≥2 并行度）时才必然发生；单 vCPU 的 CI runner 上
        // 线程不会真正抢锁，此时只验证计数不丢失，不强制 contentions > 0。
        let cores = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1);
        if cores >= 2 {
            assert!(cell.contentions() > 0, "the test should actually contend");
        }
    }

    #[test]
    fn pi_mutex_donates_priority_to_the_owner() {
        let m = PiMutex::new();
        assert!(m.is_free());
        // Low priority owner takes it.
        assert!(m.acquire(1, PRIO_MIN));
        assert_eq!(m.owner(), Some(1));
        assert_eq!(m.effective_priority(), Some(PRIO_MIN));
        // A real-time waiter fails to acquire but donates its priority.
        assert!(!m.acquire(2, PRIO_RT));
        assert_eq!(m.effective_priority(), Some(PRIO_RT));
        assert_eq!(m.boosts(), 1);
        assert_eq!(m.inversions_blocked(), 1);
        // Release hands the lock straight to the high-priority waiter.
        assert_eq!(m.release(1), Some(2));
        assert_eq!(m.owner(), Some(2));
        assert!(m.release(2).is_none());
        assert!(m.is_free());
        assert_eq!(m.effective_priority(), None);
    }

    #[test]
    fn pi_mutex_picks_the_hottest_waiter() {
        let m = PiMutex::new();
        assert!(m.acquire(1, PRIO_MAX));
        assert!(!m.acquire(2, PRIO_MIN));
        assert!(!m.acquire(3, PRIO_NORMAL));
        // Highest priority wins.
        assert_eq!(m.release(1), Some(3));
        // Remaining waiter still donates.
        assert_eq!(m.effective_priority(), Some(PRIO_MIN));
        assert_eq!(m.release(3), Some(2));
        assert!(m.release(2).is_none());
    }

    #[test]
    fn pi_mutex_ignores_a_wrong_release() {
        let m = PiMutex::new();
        assert!(m.acquire(1, PRIO_NORMAL));
        assert_eq!(m.release(99), None, "a non-owner cannot release");
        assert_eq!(m.owner(), Some(1));
        assert!(m.release(1).is_none());
    }

    #[test]
    fn nesting_policy_gates_by_priority_and_depth() {
        let n = Nesting::new();
        assert_eq!(n.policy(), NestingPolicy::Priority);
        assert_eq!(n.max_depth(), MAX_NESTING);

        // Priority policy: the first level always enters, deeper ones need a
        // strictly more urgent vector.
        assert!(n.allows_at_depth(1, 0, 0));
        assert!(!n.allows_at_depth(4, 4, 1), "equal priority does not nest");
        assert!(n.allows_at_depth(9, 4, 1), "more urgent vector nests");

        // Flat policy: nothing nests.
        n.set_policy(NestingPolicy::Flat);
        assert_eq!(n.policy(), NestingPolicy::Flat);
        assert!(n.allows_at_depth(1, 0, 0));
        assert!(!n.allows_at_depth(63, 1, 1));

        // Free policy: anything nests, up to the cap.
        n.set_policy(NestingPolicy::Free);
        assert_eq!(n.policy(), NestingPolicy::Free);
        assert!(n.allows_at_depth(1, 63, 2));

        // The cap is a hard red line and it is counted.
        let before = n.violations();
        assert!(!n.allows_at_depth(63, 1, MAX_NESTING));
        assert_eq!(n.violations(), before + 1);
    }

    #[test]
    fn irq_off_audit_accounts_windows() {
        let a = IrqOffAudit::new();
        assert!(a.within_budget());
        let t = a.begin();
        assert!(a.is_open());
        let ns = a.end(t);
        assert!(!a.is_open());
        assert_eq!(a.samples(), 1);
        let _ = ns;
        // A zero budget turns every measured window into a violation. Accounted
        // directly here because the host build has no TSC behind `end`.
        a.set_budget_ns(0);
        a.note(5);
        assert_eq!(a.violations(), 1);
        assert!(!a.within_budget());
        assert_eq!(a.samples(), 2);
        assert_eq!(a.longest_ns(), 5);
        assert!(a.mean_ns() <= a.longest_ns());
    }

    #[test]
    fn with_irqs_off_returns_the_value() {
        assert_eq!(with_irqs_off(|| 7u32), 7);
        assert_eq!(irq_off_audit().samples() > 0, true);
    }
}
