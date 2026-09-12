//! AI-19 内核健壮性与缺陷防御域（F451~F475·真实高危清单）。
//!
//! Every item here maps to a real class of kernel bug that has bitten this
//! project (the frame-pool DMA aliasing, the OVERLAPPED-in-ISR race, the
//! self-deadlocking event loop): ISR races, deadlocks, priority inversion,
//! spin-lock/IRQ discipline, use-after-free, double free, refcount and handle
//! leaks, page-table concurrency, DMA ownership, bounds, overflow, TOCTOU,
//! panic humanisation, watchdog, hang forensics, watermarks, handle table
//! overflow, time wrap, IRQ storms, log storms, boot rescue and driver-load
//! isolation — closed by the kernel-wide self-test loop (F475).

use crate::checks::{CheckSet, KernelCheckup};

// ---------------------------------------------------------------------------
// F451 — ISR 竞态审计
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShareKind {
    /// Plain field touched from both context — always a bug.
    Unsynchronised,
    AtomicOnly,
    /// Protected by a lock that also masks the interrupt.
    IrqSafeLock,
}

#[derive(Clone, Copy, Debug)]
pub struct SharedData {
    pub name: &'static str,
    pub kind: ShareKind,
    pub read_in_isr: bool,
    pub written_by_thread: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RaceVerdict {
    Safe,
    /// ISR reads while a thread writes and nothing synchronises them.
    Race(&'static str),
}

pub fn audit_shared(data: &[SharedData], out: &mut [Option<&'static str>]) -> usize {
    let mut n = 0usize;
    for d in data {
        let unsafe_share = d.read_in_isr
            && d.written_by_thread
            && matches!(d.kind, ShareKind::Unsynchronised);
        if unsafe_share && n < out.len() {
            out[n] = Some(d.name);
            n += 1;
        }
    }
    n
}

pub fn race_verdict(d: SharedData) -> RaceVerdict {
    if d.read_in_isr && d.written_by_thread && d.kind == ShareKind::Unsynchronised {
        RaceVerdict::Race(d.name)
    } else {
        RaceVerdict::Safe
    }
}

// ---------------------------------------------------------------------------
// F452 — 死锁检测器
// ---------------------------------------------------------------------------

pub const MAX_LOCKS: usize = 8;

#[derive(Clone, Copy, Debug)]
pub struct LockOrderGraph {
    /// `edges[a]` = bitmask of locks acquired while holding `a`.
    edges: [u8; MAX_LOCKS],
    names: [Option<&'static str>; MAX_LOCKS],
    count: usize,
}

impl LockOrderGraph {
    pub const fn new() -> LockOrderGraph {
        LockOrderGraph { edges: [0; MAX_LOCKS], names: [None; MAX_LOCKS], count: 0 }
    }

    pub fn add_lock(&mut self, name: &'static str) -> Option<usize> {
        if self.count >= MAX_LOCKS {
            return None;
        }
        self.names[self.count] = Some(name);
        self.count += 1;
        Some(self.count - 1)
    }

    /// Record "held `from`, then acquired `to`".
    pub fn add_edge(&mut self, from: usize, to: usize) -> bool {
        if from >= self.count || to >= self.count || from == to {
            return false;
        }
        self.edges[from] |= 1 << to;
        true
    }

    /// Depth-first cycle search — a cycle means a lock order inversion.
    pub fn find_cycle(&self) -> Option<(&'static str, &'static str)> {
        let mut state = [0u8; MAX_LOCKS]; // 0 = new, 1 = on stack, 2 = done
        for start in 0..self.count {
            if state[start] == 0 {
                if let Some(pair) = self.dfs(start, &mut state, 0) {
                    return Some(pair);
                }
            }
        }
        None
    }

    fn dfs(
        &self,
        node: usize,
        state: &mut [u8; MAX_LOCKS],
        depth: usize,
    ) -> Option<(&'static str, &'static str)> {
        if depth > MAX_LOCKS {
            return None;
        }
        state[node] = 1;
        for next in 0..self.count {
            if self.edges[node] & (1 << next) == 0 {
                continue;
            }
            match state[next] {
                1 => {
                    return Some((
                        self.names[node].unwrap_or("?"),
                        self.names[next].unwrap_or("?"),
                    ))
                }
                0 => {
                    if let Some(pair) = self.dfs(next, state, depth + 1) {
                        return Some(pair);
                    }
                }
                _ => {}
            }
        }
        state[node] = 2;
        None
    }
}

// ---------------------------------------------------------------------------
// F453 — 优先级反转守卫
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InversionCheck {
    pub holder_priority: u8,
    pub waiter_priority: u8,
    /// Holder may donate its priority to the waiter's owner.
    pub boost_capable: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Inversion {
    pub boost: bool,
    pub donator: u8,
}

pub fn inversion_guard(check: InversionCheck) -> Option<Inversion> {
    if check.waiter_priority > check.holder_priority {
        Some(Inversion {
            boost: check.boost_capable,
            donator: check.holder_priority,
        })
    } else {
        None
    }
}

/// Priority values: higher number = higher priority (0 = idle).
pub fn effective_priority(base: u8, inherited: u8) -> u8 {
    base.max(inherited)
}

// ---------------------------------------------------------------------------
// F454/F455 — 自旋锁关中断纪律、IRQ 上下文禁睡眠
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SpinGuard {
    pub held: bool,
    pub irq_disabled: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DisciplineError {
    /// The lock is held with interrupts still on: an ISR may deadlock.
    IrqNotMasked,
    NotHeld,
}

impl SpinGuard {
    pub const fn new() -> SpinGuard {
        SpinGuard { held: false, irq_disabled: false }
    }

    pub fn acquire(&mut self, irq_disabled: bool) {
        self.held = true;
        self.irq_disabled = irq_disabled;
    }

    pub fn assert_discipline(&self) -> Result<(), DisciplineError> {
        if !self.held {
            return Err(DisciplineError::NotHeld);
        }
        if !self.irq_disabled {
            return Err(DisciplineError::IrqNotMasked);
        }
        Ok(())
    }

    pub fn release(&mut self) {
        self.held = false;
        self.irq_disabled = false;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SleepError {
    SleepingInInterrupt,
}

/// Interrupt-context depth. Any sleep attempt with depth > 0 is a bug that
/// must panic immediately with a readable explanation (F465 link).
#[derive(Clone, Copy, Debug)]
pub struct IrqContext {
    pub depth: u8,
    pub preempt_disabled: bool,
}

impl IrqContext {
    pub const fn new() -> IrqContext {
        IrqContext { depth: 0, preempt_disabled: false }
    }

    pub fn enter(&mut self) {
        self.depth = self.depth.saturating_add(1);
    }

    pub fn exit(&mut self) {
        self.depth = self.depth.saturating_sub(1);
    }

    pub fn in_interrupt(&self) -> bool {
        self.depth > 0
    }

    pub fn try_sleep(&self) -> Result<(), SleepError> {
        if self.in_interrupt() || self.preempt_disabled {
            Err(SleepError::SleepingInInterrupt)
        } else {
            Ok(())
        }
    }
}

// ---------------------------------------------------------------------------
// F456/F457 — use-after-free 与双重释放检测
// ---------------------------------------------------------------------------

pub const POISON_FREED: u64 = 0xDEAD_DEAD_DEAD_DEAD;
pub const POISON_ALLOC: u64 = 0xFEED_FACE_FEED_FACE;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PoisonVerdict {
    /// Poisoned: someone touched freed memory.
    UseAfterFree,
    /// Still poisoned from the allocator: fine while uninitialised.
    Untouched,
    Live,
}

pub fn check_poison(word: u64, expected_live: bool) -> PoisonVerdict {
    if expected_live {
        if word == POISON_FREED {
            PoisonVerdict::UseAfterFree
        } else {
            PoisonVerdict::Live
        }
    } else if word == POISON_FREED {
        PoisonVerdict::Untouched
    } else {
        PoisonVerdict::UseAfterFree
    }
}

pub const MAX_TRACKED_SLOTS: usize = 32;

#[derive(Clone, Copy, Debug)]
pub struct AllocBitmap {
    /// Bit set = slot in use.
    used: u32,
    /// Bit set = slot was freed at least once (double-free detection).
    freed_once: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AllocError {
    DoubleFree,
    OutOfSlots,
    NotAllocated,
}

impl AllocBitmap {
    pub const fn new() -> AllocBitmap {
        AllocBitmap { used: 0, freed_once: 0 }
    }

    pub fn alloc(&mut self, slot: usize) -> Result<(), AllocError> {
        if slot >= MAX_TRACKED_SLOTS {
            return Err(AllocError::OutOfSlots);
        }
        let bit = 1u32 << (slot % 32);
        if self.used & bit != 0 {
            return Err(AllocError::OutOfSlots);
        }
        self.used |= bit;
        Ok(())
    }

    pub fn free(&mut self, slot: usize) -> Result<(), AllocError> {
        if slot >= MAX_TRACKED_SLOTS {
            return Err(AllocError::OutOfSlots);
        }
        let bit = 1u32 << (slot % 32);
        if self.used & bit == 0 {
            self.freed_once |= bit;
            return Err(AllocError::DoubleFree);
        }
        self.used &= !bit;
        self.freed_once |= bit;
        Ok(())
    }

    pub fn live_count(&self) -> u32 {
        self.used.count_ones()
    }
}

// ---------------------------------------------------------------------------
// F458/F459 — 引用计数审计与句柄泄漏检测
// ---------------------------------------------------------------------------

pub const MAX_REFS: usize = 8;

#[derive(Clone, Copy, Debug)]
pub struct RefCountAudit {
    names: [Option<&'static str>; MAX_REFS],
    counts: [i32; MAX_REFS],
    high_water: [u32; MAX_REFS],
    count: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RefVerdict {
    Ok,
    /// Dropped below zero — a double release.
    Underflow(&'static str),
    /// Still referenced at teardown — a leak.
    Leak(&'static str),
    /// Unknown object.
    Unknown,
}

impl RefCountAudit {
    pub const fn new() -> RefCountAudit {
        RefCountAudit {
            names: [None; MAX_REFS],
            counts: [0; MAX_REFS],
            high_water: [0; MAX_REFS],
            count: 0,
        }
    }

    fn index_of(&self, name: &str) -> Option<usize> {
        (0..self.count).find(|i| self.names[*i] == Some(name))
    }

    pub fn track(&mut self, name: &'static str) -> bool {
        if self.count >= MAX_REFS || self.index_of(name).is_some() {
            return false;
        }
        self.names[self.count] = Some(name);
        self.count += 1;
        true
    }

    pub fn acquire(&mut self, name: &'static str) -> RefVerdict {
        match self.index_of(name) {
            Some(i) => {
                self.counts[i] += 1;
                self.high_water[i] = self.high_water[i].max(self.counts[i] as u32);
                RefVerdict::Ok
            }
            None => RefVerdict::Unknown,
        }
    }

    pub fn release(&mut self, name: &'static str) -> RefVerdict {
        match self.index_of(name) {
            Some(i) => {
                self.counts[i] -= 1;
                if self.counts[i] < 0 {
                    RefVerdict::Underflow(name)
                } else {
                    RefVerdict::Ok
                }
            }
            None => RefVerdict::Unknown,
        }
    }

    /// At teardown every count must be back to zero.
    pub fn audit_teardown(&self, out: &mut [Option<&'static str>]) -> usize {
        let mut n = 0usize;
        for i in 0..self.count {
            if self.counts[i] != 0 && n < out.len() {
                out[n] = self.names[i];
                n += 1;
            }
        }
        n
    }

    pub fn peak(&self, name: &str) -> u32 {
        self.index_of(name).map(|i| self.high_water[i]).unwrap_or(0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HandleTrend {
    Stable,
    Growing,
    /// Growing fast enough that a leak is the only explanation.
    Leak(u32),
}

/// Sample handle counts over time; a monotonic climb with no plateau is a leak.
pub fn handle_trend(samples: &[u32], growth_threshold: u32) -> HandleTrend {
    if samples.len() < 3 {
        return HandleTrend::Stable;
    }
    let first = samples[0];
    let last = samples[samples.len() - 1];
    if last <= first {
        return HandleTrend::Stable;
    }
    let growth = last - first;
    if growth >= growth_threshold {
        HandleTrend::Leak(growth)
    } else {
        HandleTrend::Growing
    }
}

// ---------------------------------------------------------------------------
// F460/F461 — 页表并发守卫与 DMA 一致性审计
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PageTableError {
    ModifyWithoutLock,
    StaleTlb,
}

/// A page-table edit is only legal while the table lock is held, and it must
/// be followed by a TLB shootdown before the mapping is used.
pub fn page_table_edit(lock_held: bool, tlb_flushed: bool) -> Result<(), PageTableError> {
    if !lock_held {
        return Err(PageTableError::ModifyWithoutLock);
    }
    if !tlb_flushed {
        return Err(PageTableError::StaleTlb);
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DmaState {
    Cpu,
    Device,
    Free,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DmaAuditError {
    /// The CPU wrote to a buffer the device currently owns.
    CpuWroteWhileDeviceOwned,
    DeviceTouchedWhileFree,
    AlreadyOwned,
}

#[derive(Clone, Copy, Debug)]
pub struct DmaAudit {
    pub state: DmaState,
    pub owner_tag: u32,
}

impl DmaAudit {
    /// A freshly allocated buffer is owned by the CPU; handing it to the
    /// device is what moves it into `Device`.
    pub const fn new() -> DmaAudit {
        DmaAudit { state: DmaState::Cpu, owner_tag: 0 }
    }

    pub fn hand_to_device(&mut self, tag: u32) -> Result<(), DmaAuditError> {
        if self.state != DmaState::Cpu {
            return Err(DmaAuditError::AlreadyOwned);
        }
        self.state = DmaState::Device;
        self.owner_tag = tag;
        Ok(())
    }

    pub fn reclaim(&mut self) -> Result<(), DmaAuditError> {
        if self.state != DmaState::Device {
            return Err(DmaAuditError::DeviceTouchedWhileFree);
        }
        self.state = DmaState::Cpu;
        self.owner_tag = 0;
        Ok(())
    }

    pub fn cpu_write(&self) -> Result<(), DmaAuditError> {
        if self.state == DmaState::Device {
            Err(DmaAuditError::CpuWroteWhileDeviceOwned)
        } else {
            Ok(())
        }
    }
}

// ---------------------------------------------------------------------------
// F462/F463 — 缓冲边界检查与整数溢出审查
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CopyError {
    ZeroLength,
    OutOfBounds,
}

/// Every copy in the kernel goes through this: no raw `copy_from_slice`.
pub fn checked_copy<T: Copy>(dst: &mut [T], src: &[T]) -> Result<usize, CopyError> {
    if src.is_empty() {
        return Err(CopyError::ZeroLength);
    }
    if src.len() > dst.len() {
        return Err(CopyError::OutOfBounds);
    }
    dst[..src.len()].copy_from_slice(src);
    Ok(src.len())
}

pub fn copy_in_bounds(offset: usize, len: usize, buffer_len: usize) -> bool {
    match offset.checked_add(len) {
        Some(end) => end <= buffer_len,
        None => false,
    }
}

pub fn checked_mul(a: usize, b: usize) -> Option<usize> {
    a.checked_mul(b)
}

pub fn checked_add(a: usize, b: usize) -> Option<usize> {
    a.checked_add(b)
}

/// Size of `count` elements — the classic integer-overflow sink.
pub fn total_size(count: usize, element: usize) -> Option<usize> {
    count.checked_mul(element)
}

// ---------------------------------------------------------------------------
// F464 — TOCTOU 排查
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Handle {
    pub id: u32,
    /// Bumped on every reuse of the slot; a stale handle is detected here.
    pub generation: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HandleError {
    Stale,
    Revoked,
    Unknown,
}

#[derive(Clone, Copy, Debug)]
pub struct HandleSlot {
    pub handle: Handle,
    pub valid: bool,
}

/// Resolve a handle atomically: check-and-use in one step, so nothing can be
/// swapped between the check and the use.
pub fn resolve_handle(slot: HandleSlot, candidate: Handle) -> Result<(), HandleError> {
    if !slot.valid {
        return Err(HandleError::Revoked);
    }
    if slot.handle.id != candidate.id {
        return Err(HandleError::Unknown);
    }
    if slot.handle.generation != candidate.generation {
        return Err(HandleError::Stale);
    }
    Ok(())
}

/// Revoking a handle advances its generation, invalidating every copy.
pub fn revoke(slot: &mut HandleSlot) {
    slot.valid = false;
    slot.handle.generation = slot.handle.generation.wrapping_add(1);
}

// ---------------------------------------------------------------------------
// F465 — panic 人话化
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PanicKind {
    PageFault,
    DoubleFault,
    GeneralProtection,
    DivideByZero,
    AssertionFailed,
    Unreachable,
    DoubleFree,
    SleepInInterrupt,
    Deadlock,
    OutOfMemory,
    Unknown(u32),
}

/// Plain-language explanation — the user-facing half of "崩溃不扩散".
pub fn humanize(kind: PanicKind) -> &'static str {
    match kind {
        PanicKind::PageFault => "访问了不存在或无权访问的内存，系统已停止该任务。",
        PanicKind::DoubleFault => "处理器遇到无法恢复的错误，系统需重启（内核未受影响的部分已保存）。",
        PanicKind::GeneralProtection => "程序执行了非法指令或越权操作，已被终止。",
        PanicKind::DivideByZero => "程序执行了除以零的运算，已被终止。",
        PanicKind::AssertionFailed => "内部一致性检查未通过，这通常是内核缺陷，请提交日志。",
        PanicKind::Unreachable => "执行到不可能到达的代码路径，这是内核缺陷。",
        PanicKind::DoubleFree => "内存被重复释放，已隔离该子系统以避免数据损坏。",
        PanicKind::SleepInInterrupt => "在中断处理中尝试休眠，会立刻死锁，已阻止该操作。",
        PanicKind::Deadlock => "检测到锁顺序反转，已拒绝本次加锁以保证系统存活。",
        PanicKind::OutOfMemory => "内存不足，已结束占用最多的任务以保证系统响应。",
        PanicKind::Unknown(_) => "发生未知异常，诊断信息已写入本地日志。",
    }
}

pub fn humanize_code(vector: u32) -> &'static str {
    humanize(match vector {
        0 => PanicKind::DivideByZero,
        6 => PanicKind::Unreachable,
        8 => PanicKind::DoubleFault,
        13 => PanicKind::GeneralProtection,
        14 => PanicKind::PageFault,
        other => PanicKind::Unknown(other),
    })
}

// ---------------------------------------------------------------------------
// F466/F467 — watchdog 与卡死取证
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WatchdogAction {
    Ok,
    /// Fed late but recovered — log a warning.
    Late,
    /// Two windows missed: capture a dump and reset the subsystem.
    Dump,
}

#[derive(Clone, Copy, Debug)]
pub struct Watchdog {
    pub timeout_ms: u32,
    pub last_feed_ms: u32,
    pub missed_windows: u32,
}

impl Watchdog {
    pub const fn new(timeout_ms: u32) -> Watchdog {
        Watchdog { timeout_ms, last_feed_ms: 0, missed_windows: 0 }
    }

    pub fn feed(&mut self, now_ms: u32) {
        self.last_feed_ms = now_ms;
        self.missed_windows = 0;
    }

    pub fn check(&mut self, now_ms: u32) -> WatchdogAction {
        if now_ms.saturating_sub(self.last_feed_ms) <= self.timeout_ms {
            return WatchdogAction::Ok;
        }
        self.missed_windows += 1;
        if self.missed_windows >= 2 {
            WatchdogAction::Dump
        } else {
            WatchdogAction::Late
        }
    }
}

/// Forensics captured when something stops responding (no allocator).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HangDump {
    pub cpu: u8,
    pub rip: u64,
    pub stack_hash: u64,
    pub held_locks: u8,
    pub irq_depth: u8,
}

pub fn capture_hang(cpu: u8, rip: u64, frames: &[u64]) -> HangDump {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for f in frames {
        hash ^= *f;
        hash = hash.wrapping_mul(0x0000_0100_0000_01B3);
    }
    HangDump {
        cpu,
        rip,
        stack_hash: hash,
        held_locks: 0,
        irq_depth: 0,
    }
}

// ---------------------------------------------------------------------------
// F468/F469 — 内存水位、句柄表溢出防护
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WatermarkLevel {
    Normal,
    Low,
    Critical,
    Oom,
}

pub fn watermark_level(free_permille: u16) -> WatermarkLevel {
    match free_permille {
        0..=30 => WatermarkLevel::Oom,
        31..=100 => WatermarkLevel::Critical,
        101..=250 => WatermarkLevel::Low,
        _ => WatermarkLevel::Normal,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HandleTable {
    pub max: u32,
    pub used: u32,
    /// Above this ratio the table starts recycling instead of failing hard.
    pub soft_limit_permille: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HandleAlloc {
    Granted(u32),
    /// Table full: the caller must degrade (F374) instead of panicking.
    AtCapacity,
}

impl HandleTable {
    pub const fn new(max: u32) -> HandleTable {
        HandleTable { max, used: 0, soft_limit_permille: 900 }
    }

    pub fn alloc(&mut self) -> HandleAlloc {
        if self.used >= self.max {
            return HandleAlloc::AtCapacity;
        }
        self.used += 1;
        HandleAlloc::Granted(self.used - 1)
    }

    pub fn release(&mut self, count: u32) {
        self.used = self.used.saturating_sub(count);
    }

    pub fn usage_permille(&self) -> u16 {
        if self.max == 0 {
            return 1000;
        }
        ((self.used as u64 * 1000) / self.max as u64) as u16
    }

    pub fn over_soft_limit(&self) -> bool {
        self.usage_permille() >= self.soft_limit_permille
    }
}

// ---------------------------------------------------------------------------
// F470 — 时间回绕处理
// ---------------------------------------------------------------------------

/// Elapsed ticks across a wrap, for a 32-bit tick counter.
pub fn elapsed_u32(start: u32, now: u32) -> u32 {
    now.wrapping_sub(start)
}

/// Elapsed nanoseconds across an u64 wrap (effectively never, but the sleep
/// path must still be wrap-correct).
pub fn elapsed_u64(start: u64, now: u64) -> u64 {
    now.wrapping_sub(start)
}

/// A suspended system's monotonic clock jumps; the sleep path stores the
/// offset and adds it back so deadlines are not instantly "expired".
pub fn sleep_corrected_deadline(deadline: u64, slept_ns: u64) -> u64 {
    deadline.saturating_add(slept_ns)
}

pub fn expired(deadline: u64, now: u64) -> bool {
    // Wrap-safe comparison: signed difference.
    (now.wrapping_sub(deadline) as i64) >= 0
}

// ---------------------------------------------------------------------------
// F471 — 中断风暴熔断
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CircuitState {
    Closed,
    Open,
    HalfOpen,
}

#[derive(Clone, Copy, Debug)]
pub struct StormBreaker {
    pub window_ms: u32,
    pub max_irqs: u32,
    pub state: CircuitState,
    window_start_ms: u32,
    count: u32,
    open_until_ms: u32,
    pub tripped: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StormAction {
    Permit,
    /// Over the rate: mask the line until the next window.
    Throttle,
    /// Circuit open: refuse the IRQ entirely.
    Refuse,
}

impl StormBreaker {
    pub const fn new(window_ms: u32, max_irqs: u32) -> StormBreaker {
        StormBreaker {
            window_ms,
            max_irqs,
            state: CircuitState::Closed,
            window_start_ms: 0,
            count: 0,
            open_until_ms: 0,
            tripped: 0,
        }
    }

    pub fn on_irq(&mut self, now_ms: u32) -> StormAction {
        if self.state == CircuitState::Open {
            if now_ms < self.open_until_ms {
                return StormAction::Refuse;
            }
            self.state = CircuitState::HalfOpen;
            self.count = 0;
            self.window_start_ms = now_ms;
        }
        if now_ms.saturating_sub(self.window_start_ms) > self.window_ms {
            self.window_start_ms = now_ms;
            self.count = 0;
            if self.state == CircuitState::HalfOpen {
                self.state = CircuitState::Closed;
            }
        }
        self.count += 1;
        if self.count > self.max_irqs {
            if self.state != CircuitState::Open {
                self.tripped += 1;
            }
            self.state = CircuitState::Open;
            self.open_until_ms = now_ms.saturating_add(self.window_ms * 4);
            return StormAction::Throttle;
        }
        // A single clean IRQ after the open window closes the circuit again.
        if self.state == CircuitState::HalfOpen {
            self.state = CircuitState::Closed;
        }
        StormAction::Permit
    }
}

// ---------------------------------------------------------------------------
// F472 — 日志风暴节流
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
pub struct TokenBucket {
    pub rate_per_sec: u32,
    pub burst: u32,
    tokens: u32,
    last_refill_ms: u32,
    pub suppressed: u32,
}

impl TokenBucket {
    pub const fn new(rate_per_sec: u32, burst: u32) -> TokenBucket {
        TokenBucket { rate_per_sec, burst, tokens: burst, last_refill_ms: 0, suppressed: 0 }
    }

    /// `true` = emit, `false` = suppressed (caller emits a summary later).
    pub fn allow(&mut self, now_ms: u32) -> bool {
        let elapsed = now_ms.saturating_sub(self.last_refill_ms);
        if elapsed > 0 {
            let refill = (self.rate_per_sec as u64 * elapsed as u64 / 1000) as u32;
            self.tokens = (self.tokens + refill).min(self.burst);
            self.last_refill_ms = now_ms;
        }
        if self.tokens > 0 {
            self.tokens -= 1;
            true
        } else {
            self.suppressed += 1;
            false
        }
    }

    /// One line telling the user how many messages were dropped.
    pub fn summary(&self) -> u32 {
        self.suppressed
    }
}

// ---------------------------------------------------------------------------
// F473/F474 — 启动失败急救与驱动加载失败隔离
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BootFailure {
    pub consecutive_failures: u8,
    pub last_error: &'static str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RescuePlan {
    pub safe_mode: bool,
    pub minimal_drivers: bool,
    pub serial_console: bool,
    pub offer_rollback: bool,
}

pub const MAX_AUTO_RETRIES: u8 = 2;

/// Three strikes: boot minimal, keep the serial console, offer a rollback.
pub fn rescue_plan(failure: BootFailure) -> RescuePlan {
    let n = failure.consecutive_failures;
    RescuePlan {
        safe_mode: n >= 2,
        minimal_drivers: n >= 2,
        serial_console: n >= 1,
        offer_rollback: n >= 3,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DriverLoadOutcome {
    pub name: &'static str,
    pub loaded: bool,
    pub error: Option<&'static str>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LoadSummary {
    pub attempted: u8,
    pub loaded: u8,
    pub skipped: u8,
    /// Boot must continue even when drivers fail (红线).
    pub boot_continues: bool,
}

pub fn isolate_driver_failures(outcomes: &[DriverLoadOutcome]) -> LoadSummary {
    let mut loaded = 0u8;
    for o in outcomes {
        if o.loaded {
            loaded += 1;
        }
    }
    LoadSummary {
        attempted: outcomes.len().min(u8::MAX as usize) as u8,
        loaded,
        skipped: (outcomes.len() - loaded as usize).min(u8::MAX as usize) as u8,
        boot_continues: true,
    }
}

// ---------------------------------------------------------------------------
// F475 — 内核自检闭环
// ---------------------------------------------------------------------------

/// Aggregate every domain self-test into one verdict. This is the loop that
/// closes the kernel: 10 domains, one report, no reports to a human — the
/// result is consumed by the boot path and the QEMU headless assertions.
pub fn run_kernel_checkup() -> KernelCheckup {
    let mut checkup = KernelCheckup::new();
    checkup.register(crate::power::run_power_checks());
    checkup.register(crate::audio::run_audio_checks());
    checkup.register(crate::driver::run_driver_checks());
    checkup.register(crate::virt::run_virt_checks());
    checkup.register(crate::service::run_service_checks());
    checkup.register(crate::ui::run_ui_checks());
    checkup.register(crate::vsem::run_vsem_checks());
    checkup.register(crate::deploy::run_install_checks());
    checkup.register(crate::deploy::run_deploy_checks());
    checkup.register(crate::robust::run_robust_checks());
    checkup.register(crate::deveco::run_deveco_checks());
    // --- TRINITY-500 AI-01~AI-10 (F001~F250) --------------------------------
    checkup.register(crate::switcher::bootnext::run_boot_checks());
    checkup.register(crate::switcher::hibernate::run_hibernate_checks());
    checkup.register(crate::fs::run_fs_checks());
    checkup.register(crate::share::run_share_checks());
    checkup.register(crate::gfx::run_gfx_checks());
    checkup.register(crate::gfx::text::run_text_checks());
    checkup.register(crate::ui::widgets::run_widget_checks());
    checkup.register(crate::ui::motion::run_motion_checks());
    checkup.register(crate::vwm::run_vwm_checks());
    checkup.register(crate::shell::run_shell_checks());
    // --- GALAXY-1800 AI-08~AI-16 (G421~G960) --------------------------------
    checkup.register(crate::gdist::run_gdist_checks());
    checkup.register(crate::gcons::run_gcons_checks());
    checkup.register(crate::gcont::run_gcont_checks());
    checkup.register(crate::guni::run_guni_checks());
    checkup.register(crate::gpm::run_gpm_checks());
    checkup.register(crate::gperf::run_gperf_checks());
    checkup.register(crate::gobs::run_gobs_checks());
    checkup.register(crate::gsec::run_gsec_checks());
    checkup.register(crate::grtc::run_grtc_checks());
    // --- AURORA-1000 AI-16~AI-30 (A376~A750) --------------------------------
    checkup.register(crate::workspace::run_workspace_checks());
    checkup.register(crate::designsys::run_designsys_checks());
    checkup.register(crate::fileman::run_fileman_checks());
    checkup.register(crate::settings::run_settings_checks());
    checkup.register(crate::apps::run_apps_checks());
    checkup.register(crate::terminal::run_terminal_checks());
    checkup.register(crate::editor::run_editor_checks());
    checkup.register(crate::imageview::run_imageview_checks());
    checkup.register(crate::player::run_player_checks());
    checkup.register(crate::netweb::run_netweb_checks());
    checkup.register(crate::notify::run_notify_checks());
    checkup.register(crate::search::run_search_checks());
    checkup.register(crate::sysmon::run_sysmon_checks());
    checkup.register(crate::pkgstore::run_pkgstore_checks());
    checkup.register(crate::printing::run_printing_checks());
    // --- AURORA-1000 W1 十五域 + 跨域集成收口（步骤 0257 domain-gate 汇总） ---
    checkup.register(crate::aurora::display::run_display_checks());
    checkup.register(crate::aurora::render2d::run_render2d_checks());
    checkup.register(crate::aurora::typography::run_typography_checks());
    checkup.register(crate::aurora::gpu::run_gpu_checks());
    checkup.register(crate::aurora::compositor::run_compositor_checks());
    checkup.register(crate::aurora::image::run_image_checks());
    checkup.register(crate::aurora::input::run_ainput_checks());
    checkup.register(crate::aurora::audio::run_aaudio_checks());
    checkup.register(crate::aurora::window::run_window_checks());
    checkup.register(crate::aurora::motion::run_motion_checks());
    checkup.register(crate::aurora::desktop::run_desktop_checks());
    checkup.register(crate::aurora::appfw::run_appfw_checks());
    checkup.register(crate::aurora::widgets::run_widgets_checks());
    checkup.register(crate::aurora::clipboard::run_clipboard_checks());
    checkup.register(crate::aurora::session::run_session_checks());
    checkup.register(crate::aurora::integration::run_w1_integration_checks());
    // --- AURORA-1000 W2 联调集成域（步骤 0490~0499）--------------------------
    checkup.register(crate::aurora::w2_integration::run_w2_checks());
    // --- AURORA-1000 W3 收口门禁（步骤 0729~0737） ---------------------------
    checkup.register(crate::w3gate::run_w3gate_checks());
    // --- AURORA-1000 W4/W5 AI-31~AI-40 域 CheckSet 收口（步骤 0968~1202）------
    checkup.register(crate::a11y::run_a11y_checks());
    checkup.register(crate::apower::run_apower_checks());
    checkup.register(crate::perf::run_perf_checks());
    checkup.register(crate::stability::run_stability_checks());
    checkup.register(crate::asecurity::run_asecurity_checks());
    checkup.register(crate::testing::run_testing_checks());
    checkup.register(crate::help::run_help_checks());
    checkup.register(crate::acceptance::run_acceptance_checks());
    checkup.register(crate::release::run_release_checks());
    checkup.register(crate::finalize::run_finalize_checks());
    // --- VARIABLE-200 AI-01 用户态进程域（F001~F025，W2）--------------------
    // 页表隔离、ring3 双路切换、进程生命周期与崩溃隔离。
    checkup.register(crate::proc::run_uspace_checks());
    // --- VARIABLE-200 AI-02 可执行加载与 ABI 域（F026~F050，W2）--------------
    // VXELF 加载、W^X、ASLR、入口栈/auxv/TLS、重定位、失败回收。
    checkup.register(crate::exec::run_exec_checks());
    // --- VARIABLE-200 AI-03 系统调用域（F051~F075，W2）----------------------
    // 号表/参数安全层/错误码/调用面/能力审计配额/seccomp/vDSO/fuzz/仪表。
    checkup.register(crate::syscall::run_syscall_checks());
    // --- VARIABLE-200 AI-04~AI-08（F076~F200）--------------------------------
    checkup.register(crate::srv::run_srv_checks());
    checkup.register(crate::gfxsrv::run_gfxsrv_checks());
    checkup.register(crate::hidsrv::run_hidsrv_checks());
    checkup.register(crate::vport::run_vport_checks());
    checkup.register(crate::bootchain::run_bootchain_checks());
    // --- VARIX-M500 AI-06~AI-10（F126~F250）---------------------------------
    checkup.register(crate::gfxsrv::vision::run_vision_checks());
    checkup.register(crate::hidsrv::feel::run_feel_checks());
    checkup.register(crate::audio::tone::run_tone_checks());
    checkup.register(crate::net::netxp::run_netxp_checks());
    checkup.register(crate::sec::trust::run_trust_checks());
    // --- VARIX-M500 AI-11~AI-15（F251~F375，内核成熟化）----------------------
    checkup.register(crate::envpower::run_energy_checks());
    checkup.register(crate::theme::run_theme_checks());
    checkup.register(crate::hwcompat::run_hwcompat_checks());
    checkup.register(crate::appmgr::run_appmgr_checks());
    checkup.register(crate::automation::run_automation_checks());
    // --- VARIX-M500 AI-01~AI-05（F001~F125，M1 底座 + M3 生态门口）----------
    checkup.register(crate::m5boot::run_bootm5_checks());
    checkup.register(crate::m5sched::run_m5sched_checks());
    checkup.register(crate::m5mem::run_m5mem_checks());
    checkup.register(crate::m5srv::run_m5srv_checks());
    checkup.register(crate::m5fs::run_m5fs_checks());
    // --- VARIX-M600 AI-01 / VARIX-M700 AI-01（各域 CheckSet 闭环）------------
    checkup.register(crate::m600boot::run_m600boot_checks());
    checkup.register(crate::m700proc::run_m700proc_checks());
    // --- VARIX-M500 AI-16~AI-20（F376~F500，成熟化系列）----------------------
    checkup.register(crate::deskwis::run_deskwis_checks());
    checkup.register(crate::dataflow::run_dataflow_checks());
    checkup.register(crate::selfheal::run_selfheal_checks());
    checkup.register(crate::observ::run_observ_checks());
    checkup.register(crate::i18n::run_i18n_checks());
    // --- VARIX-M400 AI-09~AI-16（F201~F400，成品体验与看不见的质量）----------
    checkup.register(crate::m4shell::run_m4shell_checks());
    checkup.register(crate::m4compat::run_m4compat_checks());
    checkup.register(crate::m4perf::run_m4perf_checks());
    checkup.register(crate::m4privsec::run_m4privsec_checks());
    checkup.register(crate::m4release::run_m4release_checks());
    checkup.register(crate::m4quality::run_m4quality_checks());
    checkup.register(crate::m4docseco::run_m4docseco_checks());
    checkup.register(crate::m4arts::run_m4arts_checks());
    // --- VARIX-M600 AI-21~AI-24（F501~F600，作品交付波次）--------------------
    checkup.register(crate::m6sync::run_m6sync_checks());
    checkup.register(crate::m6assist::run_m6assist_checks());
    checkup.register(crate::m6a11y::run_m6a11y_checks());
    checkup.register(crate::m6deliver::run_m6deliver_checks());
    // --- VARIX-M700 AI-21~AI-28（F501~F700，内核成熟化收口波次）--------------
    checkup.register(crate::m7sched::run_m7sched_checks());
    checkup.register(crate::m7cgroup::run_m7cgroup_checks());
    // --- VARIX-M600 AI-11~AI-20（F251~F500，各域 CheckSet 闭环）--------------
    checkup.register(crate::m600media::run_m600media_checks());
    checkup.register(crate::m600motion::run_m600motion_checks());
    checkup.register(crate::m600shell::run_m600shell_checks());
    checkup.register(crate::m600files::run_m600files_checks());
    checkup.register(crate::m600sdk::run_m600sdk_checks());
    checkup.register(crate::m600auto::run_m600auto_checks());
    checkup.register(crate::m600net::run_m600net_checks());
    checkup.register(crate::m600svc::run_m600svc_checks());
    checkup.register(crate::m600priv::run_m600priv_checks());
    checkup.register(crate::m600hw::run_m600hw_checks());
    // --- VARIX-M700 AI-11~AI-20（F251~F500，各域 CheckSet 闭环）--------------
    checkup.register(crate::m700cache::run_m700cache_checks());
    checkup.register(crate::m700dev::run_m700dev_checks());
    checkup.register(crate::m700drv::run_m700drv_checks());
    checkup.register(crate::m700input::run_m700input_checks());
    checkup.register(crate::m700gpu::run_m700gpu_checks());
    checkup.register(crate::m700disp::run_m700disp_checks());
    checkup.register(crate::m700net::run_m700net_checks());
    checkup.register(crate::m700rf::run_m700rf_checks());
    checkup.register(crate::m700pwr::run_m700pwr_checks());
    checkup.register(crate::m700smp::run_m700smp_checks());
    checkup.register(crate::m7timelog::run_m7timelog_checks());
    checkup.register(crate::m7bootfw::run_m7bootfw_checks());
    checkup.register(crate::m7testing::run_m7testing_checks());
    checkup.register(crate::m7bench::run_m7bench_checks());
    checkup.register(crate::m7compat::run_m7compat_checks());
    checkup.register(crate::m7docrel::run_m7docrel_checks());
    checkup
}

pub fn run_robust_checks() -> CheckSet {
    let mut set = CheckSet::new("robust");

    let shared = [
        SharedData { name: "frame_head", kind: ShareKind::Unsynchronised, read_in_isr: true, written_by_thread: true },
        SharedData { name: "rx_tail", kind: ShareKind::AtomicOnly, read_in_isr: true, written_by_thread: true },
        SharedData { name: "config", kind: ShareKind::IrqSafeLock, read_in_isr: true, written_by_thread: true },
        SharedData { name: "ui_only", kind: ShareKind::Unsynchronised, read_in_isr: false, written_by_thread: true },
    ];
    let mut races = [None; 4];
    let race_count = audit_shared(&shared, &mut races);
    set.add(
        "F451 isr race audit",
        race_count == 1
            && races[0] == Some("frame_head")
            && race_verdict(shared[1]) == RaceVerdict::Safe
            && race_verdict(shared[3]) == RaceVerdict::Safe,
        "shared-data audit",
    );

    let mut graph = LockOrderGraph::new();
    let a = graph.add_lock("state");
    let b = graph.add_lock("queue");
    let c = graph.add_lock("device");
    let (pa, pb, pc) = (
        a.expect("lock"),
        b.expect("lock"),
        c.expect("lock"),
    );
    let _ = graph.add_edge(pa, pb);
    let _ = graph.add_edge(pb, pc);
    set.add("F452 no deadlock", graph.find_cycle().is_none(), "acyclic order");
    let _ = graph.add_edge(pc, pa);
    set.add(
        "F452 deadlock detected",
        graph.find_cycle().is_some(),
        "cycle found",
    );
    set.add(
        "F452 self edge rejected",
        !graph.add_edge(pa, pa) && !graph.add_edge(99, 0),
        "guards",
    );

    let inversion = inversion_guard(InversionCheck {
        holder_priority: 2,
        waiter_priority: 9,
        boost_capable: true,
    });
    set.add(
        "F453 priority inversion",
        inversion.map(|i| (i.boost, i.donator)) == Some((true, 2))
            && inversion_guard(InversionCheck {
                holder_priority: 9,
                waiter_priority: 2,
                boost_capable: true,
            })
            .is_none()
            && effective_priority(2, 9) == 9,
        "boost",
    );

    let mut spin = SpinGuard::new();
    let before = spin.assert_discipline();
    spin.acquire(false);
    let bad = spin.assert_discipline();
    spin.acquire(true);
    let good = spin.assert_discipline();
    spin.release();
    set.add(
        "F454 spin lock discipline",
        before == Err(DisciplineError::NotHeld)
            && bad == Err(DisciplineError::IrqNotMasked)
            && good.is_ok()
            && !spin.held,
        "irq masked",
    );

    let mut ctx = IrqContext::new();
    let thread_sleep = ctx.try_sleep();
    ctx.enter();
    let irq_sleep = ctx.try_sleep();
    ctx.exit();
    set.add(
        "F455 no sleep in irq",
        thread_sleep.is_ok()
            && irq_sleep == Err(SleepError::SleepingInInterrupt)
            && !ctx.in_interrupt()
            && IrqContext { preempt_disabled: true, ..ctx }.try_sleep().is_err(),
        "sleep assertion",
    );

    set.add(
        "F456 use after free",
        check_poison(POISON_FREED, true) == PoisonVerdict::UseAfterFree
            && check_poison(POISON_FREED, false) == PoisonVerdict::Untouched
            && check_poison(POISON_ALLOC, true) == PoisonVerdict::Live,
        "poison fill",
    );

    let mut bitmap = AllocBitmap::new();
    let first_free = bitmap.free(3);
    let _ = bitmap.alloc(3);
    let ok_free = bitmap.free(3);
    let double = bitmap.free(3);
    set.add(
        "F457 double free",
        first_free == Err(AllocError::DoubleFree)
            && ok_free.is_ok()
            && double == Err(AllocError::DoubleFree)
            && bitmap.live_count() == 0
            && bitmap.alloc(99).is_err(),
        "free bitmap",
    );

    let mut refs = RefCountAudit::new();
    refs.track("dev");
    let _ = refs.acquire("dev");
    let _ = refs.acquire("dev");
    let underflow = {
        for _ in 0..3 {
            let _ = refs.release("dev");
        }
        refs.release("dev")
    };
    set.add(
        "F458 refcount audit",
        underflow == RefVerdict::Underflow("dev")
            && refs.peak("dev") == 2
            && refs.acquire("missing") == RefVerdict::Unknown,
        "underflow detected",
    );
    let mut leaky = RefCountAudit::new();
    leaky.track("buf");
    let _ = leaky.acquire("buf");
    let mut leaks = [None; 4];
    set.add(
        "F458 leak at teardown",
        leaky.audit_teardown(&mut leaks) == 1 && leaks[0] == Some("buf"),
        "teardown leak",
    );

    set.add(
        "F459 handle leak trend",
        handle_trend(&[10, 12, 15, 18], 10) == HandleTrend::Growing
            && handle_trend(&[10, 40, 90], 30) == HandleTrend::Leak(80)
            && handle_trend(&[10, 10, 10], 1) == HandleTrend::Stable
            && handle_trend(&[1, 2], 1) == HandleTrend::Stable,
        "growth detector",
    );

    set.add(
        "F460 page table concurrency",
        page_table_edit(false, true) == Err(PageTableError::ModifyWithoutLock)
            && page_table_edit(true, false) == Err(PageTableError::StaleTlb)
            && page_table_edit(true, true).is_ok(),
        "lock + tlb",
    );

    let mut dma = DmaAudit::new();
    // A freshly allocated buffer starts CPU-owned (see `DmaAudit::new`).
    let handed = dma.hand_to_device(7);
    let cpu_write_while_device = dma.cpu_write();
    let reclaimed = dma.reclaim();
    let false_reclaim = dma.reclaim();
    set.add(
        "F461 dma consistency",
        handed.is_ok()
            && cpu_write_while_device == Err(DmaAuditError::CpuWroteWhileDeviceOwned)
            && reclaimed.is_ok()
            && false_reclaim == Err(DmaAuditError::DeviceTouchedWhileFree)
            && dma.cpu_write().is_ok(),
        "ownership audit",
    );

    let mut dst = [0u8; 8];
    set.add(
        "F462 bounds checked copy",
        checked_copy(&mut dst, &[1, 2, 3]) == Ok(3)
            && checked_copy(&mut dst, &[0u8; 9]) == Err(CopyError::OutOfBounds)
            && checked_copy(&mut dst, &[]) == Err(CopyError::ZeroLength)
            && copy_in_bounds(4, 4, 8)
            && !copy_in_bounds(4, 5, 8)
            && !copy_in_bounds(usize::MAX, 1, 8),
        "copy discipline",
    );

    set.add(
        "F463 integer overflow",
        total_size(4, 4096) == Some(16384)
            && total_size(usize::MAX, 2).is_none()
            && checked_add(usize::MAX, 1).is_none()
            && checked_mul(1 << 32, 1 << 32).is_none(),
        "checked math",
    );

    let mut slot = HandleSlot { handle: Handle { id: 5, generation: 1 }, valid: true };
    let good = resolve_handle(slot, Handle { id: 5, generation: 1 });
    let stale = resolve_handle(slot, Handle { id: 5, generation: 0 });
    let unknown = resolve_handle(slot, Handle { id: 6, generation: 1 });
    revoke(&mut slot);
    set.add(
        "F464 toctou handle",
        good.is_ok()
            && stale == Err(HandleError::Stale)
            && unknown == Err(HandleError::Unknown)
            && resolve_handle(slot, Handle { id: 5, generation: 2 }) == Err(HandleError::Revoked)
            && slot.handle.generation == 2,
        "generation guard",
    );

    set.add(
        "F465 panic humanised",
        humanize(PanicKind::PageFault).len() > 8
            && humanize(PanicKind::SleepInInterrupt).contains("中断")
            && humanize_code(8).contains("重启")
            && humanize_code(999).len() > 4,
        "plain language",
    );

    let mut wd = Watchdog::new(1000);
    wd.feed(0);
    let ok = wd.check(500);
    let late = wd.check(1500);
    let dump = wd.check(3000);
    set.add(
        "F466 watchdog",
        ok == WatchdogAction::Ok && late == WatchdogAction::Late && dump == WatchdogAction::Dump,
        "two-strike dump",
    );

    let hang = capture_hang(2, 0xFFFF_8000, &[0x10, 0x20, 0x30]);
    let hang2 = capture_hang(2, 0xFFFF_8000, &[0x10, 0x20, 0x30]);
    set.add(
        "F467 hang forensics",
        hang.stack_hash == hang2.stack_hash
            && hang.cpu == 2
            && hang.rip == 0xFFFF_8000
            && capture_hang(2, 0xFFFF_8000, &[0x10, 0x21]).stack_hash != hang.stack_hash,
        "dump",
    );

    set.add(
        "F468 memory watermark",
        watermark_level(5) == WatermarkLevel::Oom
            && watermark_level(80) == WatermarkLevel::Critical
            && watermark_level(200) == WatermarkLevel::Low
            && watermark_level(900) == WatermarkLevel::Normal,
        "levels",
    );

    let mut handles = HandleTable::new(4);
    let mut granted = 0u32;
    for _ in 0..4 {
        if let HandleAlloc::Granted(_) = handles.alloc() {
            granted += 1;
        }
    }
    let at_capacity = handles.alloc();
    handles.release(2);
    set.add(
        "F469 handle table overflow",
        granted == 4
            && at_capacity == HandleAlloc::AtCapacity
            && handles.usage_permille() == 500
            && !handles.over_soft_limit(),
        "degrade not panic",
    );

    set.add(
        "F470 time wrap",
        elapsed_u32(u32::MAX - 5, 5) == 11
            && elapsed_u64(u64::MAX - 5, 5) == 11
            && !expired(1000, 500)
            && expired(1000, 1500)
            && sleep_corrected_deadline(1000, 10_000) == 11_000
            && expired(sleep_corrected_deadline(1000, 10_000), 5_000) == false,
        "wrap safe",
    );

    let mut breaker = StormBreaker::new(100, 10);
    let mut permitted = 0u32;
    for i in 0..10 {
        if breaker.on_irq(i) == StormAction::Permit {
            permitted += 1;
        }
    }
    let throttled = breaker.on_irq(10);
    let refused = breaker.on_irq(11);
    set.add(
        "F471 interrupt storm",
        permitted == 10
            && throttled == StormAction::Throttle
            && refused == StormAction::Refuse
            && breaker.state == CircuitState::Open
            && breaker.tripped == 1,
        "circuit breaker",
    );
    let half_open = breaker.on_irq(1000);
    set.add(
        "F471 half open",
        half_open == StormAction::Permit || half_open == StormAction::Throttle,
        "recovery window",
    );

    let mut bucket = TokenBucket::new(10, 5);
    let mut allowed = 0u32;
    for _ in 0..5 {
        if bucket.allow(0) {
            allowed += 1;
        }
    }
    let suppressed = !bucket.allow(0);
    let refilled = bucket.allow(500);
    set.add(
        "F472 log throttle",
        allowed == 5 && suppressed && bucket.summary() == 1 && refilled,
        "token bucket",
    );

    set.add(
        "F473 boot rescue",
        rescue_plan(BootFailure { consecutive_failures: 1, last_error: "e" }).serial_console
            && !rescue_plan(BootFailure { consecutive_failures: 1, last_error: "e" }).safe_mode
            && rescue_plan(BootFailure { consecutive_failures: 2, last_error: "e" }).minimal_drivers
            && rescue_plan(BootFailure { consecutive_failures: 3, last_error: "e" }).offer_rollback,
        "three strikes",
    );

    let outcomes = [
        DriverLoadOutcome { name: "ahci", loaded: true, error: None },
        DriverLoadOutcome { name: "wifi", loaded: false, error: Some("probe timeout") },
        DriverLoadOutcome { name: "audio", loaded: true, error: None },
    ];
    let summary = isolate_driver_failures(&outcomes);
    set.add(
        "F474 driver fault isolation",
        summary.attempted == 3
            && summary.loaded == 2
            && summary.skipped == 1
            && summary.boot_continues,
        "boot continues",
    );

    set.add(
        "F475 checkup loop",
        {
            // Aggregated without `run_kernel_checkup` to keep this set a leaf:
            // the full loop (which includes this domain) lives in
            // `run_kernel_checkup` and is exercised by the test suite.
            let mut partial = KernelCheckup::new();
            partial.register(crate::power::run_power_checks());
            partial.register(crate::audio::run_audio_checks());
            partial.register(crate::driver::run_driver_checks());
            partial.register(crate::virt::run_virt_checks());
            partial.register(crate::service::run_service_checks());
            partial.register(crate::ui::run_ui_checks());
            partial.register(crate::vsem::run_vsem_checks());
            partial.register(crate::deploy::run_install_checks());
            partial.register(crate::deploy::run_deploy_checks());
            partial.register(crate::deveco::run_deveco_checks());
            partial.len() == 10 && partial.all_passed()
        },
        "kernel-wide loop",
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
    fn f451_audit_capacity() {
        let data = [SharedData {
            name: "x",
            kind: ShareKind::Unsynchronised,
            read_in_isr: true,
            written_by_thread: true,
        }; 8];
        let mut out = [None; 2];
        assert_eq!(audit_shared(&data, &mut out), 2);
        assert_eq!(audit_shared(&[], &mut out), 0);
    }

    #[test]
    fn f452_self_loop_is_not_a_cycle() {
        let mut g = LockOrderGraph::new();
        let a = g.add_lock("a").unwrap();
        let _ = g.add_edge(a, a);
        assert!(g.find_cycle().is_none());
        assert!(LockOrderGraph::new().find_cycle().is_none());
    }

    #[test]
    fn f452_long_cycle_found() {
        let mut g = LockOrderGraph::new();
        let ids: Vec<usize> = (0..4).map(|i| g.add_lock(if i == 0 { "a" } else { "b" }).unwrap()).collect();
        for i in 0..3 {
            let _ = g.add_edge(ids[i], ids[i + 1]);
        }
        assert!(g.find_cycle().is_none());
        let _ = g.add_edge(ids[3], ids[0]);
        assert!(g.find_cycle().is_some());
    }

    #[test]
    fn f452_capacity() {
        let mut g = LockOrderGraph::new();
        for _ in 0..MAX_LOCKS {
            assert!(g.add_lock("x").is_some());
        }
        assert!(g.add_lock("y").is_none());
    }

    #[test]
    fn f453_equal_priority_is_not_inversion() {
        assert!(inversion_guard(InversionCheck {
            holder_priority: 5,
            waiter_priority: 5,
            boost_capable: true
        })
        .is_none());
        assert!(inversion_guard(InversionCheck {
            holder_priority: 1,
            waiter_priority: 9,
            boost_capable: false
        })
        .unwrap()
        .boost
            == false);
    }

    #[test]
    fn f456_poison_matrix() {
        assert_eq!(check_poison(0, false), PoisonVerdict::UseAfterFree);
        assert_eq!(check_poison(0, true), PoisonVerdict::Live);
        assert_eq!(check_poison(POISON_ALLOC, false), PoisonVerdict::UseAfterFree);
    }

    #[test]
    fn f457_alloc_reuse() {
        let mut b = AllocBitmap::new();
        assert!(b.alloc(0).is_ok());
        assert!(b.alloc(0).is_err()); // already allocated
        assert!(b.free(0).is_ok());
        assert!(b.alloc(0).is_ok()); // reusable after free
        assert_eq!(b.live_count(), 1);
    }

    #[test]
    fn f458_audit_multi_object() {
        let mut a = RefCountAudit::new();
        a.track("x");
        a.track("y");
        assert!(!a.track("x"));
        let _ = a.acquire("y");
        let mut out = [None; 4];
        assert_eq!(a.audit_teardown(&mut out), 1);
        assert_eq!(out[0], Some("y"));
        assert_eq!(a.peak("x"), 0);
    }

    #[test]
    fn f459_trend_thresholds() {
        assert_eq!(handle_trend(&[5, 5, 6], 10), HandleTrend::Growing);
        assert_eq!(handle_trend(&[5, 9, 20], 15), HandleTrend::Leak(15));
        assert_eq!(handle_trend(&[20, 9, 5], 1), HandleTrend::Stable);
    }

    #[test]
    fn f462_copy_edges() {
        let mut dst = [0u8; 0];
        assert_eq!(checked_copy(&mut dst, &[1]), Err(CopyError::OutOfBounds));
        assert!(copy_in_bounds(0, 0, 0));
    }

    #[test]
    fn f463_shifts_are_checked() {
        assert_eq!(checked_mul(2, 3), Some(6));
        assert!(checked_mul(usize::MAX, 2).is_none());
        assert_eq!(checked_add(1, 2), Some(3));
        assert_eq!(total_size(0, 4096), Some(0));
    }

    #[test]
    fn f464_revoke_twice_keeps_bumping() {
        let mut slot = HandleSlot { handle: Handle { id: 1, generation: 0 }, valid: true };
        revoke(&mut slot);
        assert_eq!(slot.handle.generation, 1);
        revoke(&mut slot);
        assert_eq!(slot.handle.generation, 2);
        assert_eq!(resolve_handle(slot, Handle { id: 1, generation: 1 }), Err(HandleError::Revoked));
    }

    #[test]
    fn f466_watchdog_recovers_on_feed() {
        let mut wd = Watchdog::new(100);
        wd.feed(0);
        assert_eq!(wd.check(50), WatchdogAction::Ok);
        assert_eq!(wd.check(500), WatchdogAction::Late);
        wd.feed(600);
        assert_eq!(wd.check(650), WatchdogAction::Ok);
        assert_eq!(wd.missed_windows, 0);
    }

    #[test]
    fn f468_watermark_boundaries() {
        assert_eq!(watermark_level(0), WatermarkLevel::Oom);
        assert_eq!(watermark_level(30), WatermarkLevel::Oom);
        assert_eq!(watermark_level(31), WatermarkLevel::Critical);
        assert_eq!(watermark_level(101), WatermarkLevel::Low);
        assert_eq!(watermark_level(251), WatermarkLevel::Normal);
    }

    #[test]
    fn f470_expired_across_wrap() {
        assert!(expired(0, 1));
        assert!(!expired(1, 0));
        // A deadline just after the wrap is not expired by a pre-wrap clock.
        assert!(expired(u64::MAX - 5, 5)); // 11 ns elapsed → expired
    }

    #[test]
    fn f471_window_resets() {
        let mut b = StormBreaker::new(100, 5);
        for i in 0..5 {
            assert_eq!(b.on_irq(i), StormAction::Permit);
        }
        assert_eq!(b.on_irq(6), StormAction::Throttle);
        // After the open window expires the breaker half-opens.
        assert_eq!(b.on_irq(10_000), StormAction::Permit);
        assert_eq!(b.state, CircuitState::Closed);
    }

    #[test]
    fn f472_bucket_refills_over_time() {
        let mut b = TokenBucket::new(1000, 2);
        assert!(b.allow(0));
        assert!(b.allow(0));
        assert!(!b.allow(0));
        assert!(b.allow(100)); // 100 ms at 1000/s = 100 tokens
        assert_eq!(b.summary(), 1);
    }

    #[test]
    fn f474_all_failed_still_boots() {
        let s = isolate_driver_failures(&[
            DriverLoadOutcome { name: "a", loaded: false, error: Some("x") },
            DriverLoadOutcome { name: "b", loaded: false, error: Some("y") },
        ]);
        assert_eq!(s.loaded, 0);
        assert_eq!(s.skipped, 2);
        assert!(s.boot_continues);
        assert!(isolate_driver_failures(&[]).boot_continues);
    }

    #[test]
    fn f475_every_domain_reports() {
        let checkup = run_kernel_checkup();
        if !checkup.all_passed() {
            let mut buf = [0u8; 2048];
            let n = checkup.render(&mut buf);
            panic!("kernel checkup:\n{}", core::str::from_utf8(&buf[..n]).unwrap());
        }
        assert!(checkup.len() >= 10);
        let (passed, failed) = checkup.tally();
        assert!(passed > 200, "expected a substantial check set, got {}", passed);
        assert_eq!(failed, 0);
    }

    #[test]
    fn f475_robust_self_test_passes() {
        let set = run_robust_checks();
        if !set.all_passed() {
            let mut buf = [0u8; 1024];
            let n = set.render(&mut buf);
            panic!("robust self-test:\n{}", core::str::from_utf8(&buf[..n]).unwrap());
        }
        assert!(set.len() >= 24);
    }
}
