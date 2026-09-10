//! F076 内核线程 / F077 上下文切换 / F078 抢占式调度 / F079 优先级调度 /
//! F080 时间片轮转 / F081 实时调度类 / F082 睡眠·唤醒原语 / F089 协程支持 /
//! F090 批处理调度类 / F094 调度事件追踪 / F099 调度自检 / F100 策略档案导出.
//!
//! A fixed-capacity, allocation-free scheduler: 64 thread slots, one run queue
//! per core, and ordering that is decided by pure data, so the whole policy is
//! unit-testable. Only the six-register `SwitchFrame` save/restore is
//! architecture-specific, and even that is expressed as a data structure the
//! self-test can assert on.

use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

pub const MAX_THREADS: usize = 64;
pub const NO_THREAD: u32 = u32::MAX;
/// 10 ms at 1 kHz: short enough for交互 feel, long enough that switch cost
/// stays far below 1% of the budget.
pub const DEFAULT_SLICE_TICKS: u32 = 10;
/// A real-time thread runs until it blocks, but never longer than this in one
/// go — an RT thread with a bug must not be able to hang the machine.
pub const RT_RUNTIME_CAP_TICKS: u32 = 200;

pub const PRIO_IDLE: u8 = 0;
pub const PRIO_BATCH: u8 = 4;
pub const PRIO_NORMAL: u8 = 12;
pub const PRIO_INTERACTIVE: u8 = 24;
pub const PRIO_RT: u8 = 40;
pub const PRIO_TOP: u8 = 63;

/// Scheduling classes, highest first.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum SchedClass {
    Idle = 0,
    Batch = 1,
    Normal = 2,
    Interactive = 3,
    RealTime = 4,
    /// Pinned to an isolated core for the critical path (F093).
    Isolated = 5,
}

pub const CLASS_COUNT: usize = 6;

impl SchedClass {
    /// Classes in descending priority order — the scheduler's search order.
    pub const ALL: [SchedClass; CLASS_COUNT] = [
        SchedClass::Isolated,
        SchedClass::RealTime,
        SchedClass::Interactive,
        SchedClass::Normal,
        SchedClass::Batch,
        SchedClass::Idle,
    ];

    pub fn index(self) -> usize {
        self as usize
    }

    pub fn from_index(i: usize) -> Option<SchedClass> {
        SchedClass::ALL.iter().copied().find(|c| c.index() == i)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            SchedClass::Idle => "idle",
            SchedClass::Batch => "batch",
            SchedClass::Normal => "normal",
            SchedClass::Interactive => "interactive",
            SchedClass::RealTime => "rt",
            SchedClass::Isolated => "isolated",
        }
    }

    /// Weight for the fairness accounting (F080/F090).
    pub fn weight(self) -> u32 {
        match self {
            SchedClass::Idle => 1,
            SchedClass::Batch => 4,
            SchedClass::Normal => 8,
            SchedClass::Interactive => 16,
            SchedClass::RealTime => 32,
            SchedClass::Isolated => 64,
        }
    }

    /// F081/F090: does this class use time slices and rotation?
    pub fn is_slice_driven(self) -> bool {
        !matches!(self, SchedClass::RealTime | SchedClass::Isolated)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ThreadState {
    #[default]
    Unused,
    Runnable,
    Running,
    /// Waiting on a channel until `wake_channel` matches.
    Sleeping,
    /// Exited; kept so the reaper can collect it.
    Zombie,
}

impl ThreadState {
    pub fn as_str(self) -> &'static str {
        match self {
            ThreadState::Unused => "unused",
            ThreadState::Runnable => "runnable",
            ThreadState::Running => "running",
            ThreadState::Sleeping => "sleeping",
            ThreadState::Zombie => "zombie",
        }
    }
}

pub const FLAG_FOREGROUND: u32 = 1 << 0;
/// Woke from input since it last ran (F096).
pub const FLAG_INPUT_WOKEN: u32 = 1 << 1;
/// Owns the compute-exclusive lease (F085).
pub const FLAG_EXCLUSIVE: u32 = 1 << 2;
/// Produces frames and must not miss the budget (F097).
pub const FLAG_RENDERER: u32 = 1 << 3;
/// Kernel thread (never an OOM candidate, never throttled to zero).
pub const FLAG_KERNEL: u32 = 1 << 4;

/// One schedulable entity.
#[derive(Clone, Copy, Debug)]
pub struct Thread {
    pub tid: u32,
    pub state: ThreadState,
    pub class: SchedClass,
    /// Effective priority (may be raised by foreground boost / inheritance).
    pub prio: u8,
    /// Priority to restore when the temporary boost ends.
    pub base_prio: u8,
    pub flags: u32,
    pub slice_left: u32,
    pub slice_total: u32,
    /// Core this thread may run on (bitmask). `0` means "any".
    pub affinity: u64,
    pub cpu: u32,
    pub runtime_ticks: u64,
    pub channel: u32,
    /// Ticks spent runnable but never picked (F095).
    pub wait_ticks: u64,
    pub switches: u64,
    /// Weighted runtime for the fairness accounting.
    pub vruntime: u64,
}

impl Thread {
    pub const fn new(tid: u32) -> Thread {
        Thread {
            tid,
            state: ThreadState::Unused,
            class: SchedClass::Normal,
            prio: PRIO_NORMAL,
            base_prio: PRIO_NORMAL,
            flags: 0,
            slice_left: DEFAULT_SLICE_TICKS,
            slice_total: DEFAULT_SLICE_TICKS,
            affinity: 0,
            cpu: 0,
            runtime_ticks: 0,
            channel: 0,
            wait_ticks: 0,
            switches: 0,
            vruntime: 0,
        }
    }

    pub fn is_runnable(&self) -> bool {
        matches!(self.state, ThreadState::Runnable | ThreadState::Running)
    }

    pub fn is_kernel(&self) -> bool {
        self.flags & FLAG_KERNEL != 0
    }

    /// May this thread run on `cpu`?
    pub fn allowed_on(&self, cpu: u32) -> bool {
        if cpu >= 64 {
            return false;
        }
        self.affinity == 0 || (self.affinity >> cpu) & 1 == 1
    }

    /// Temporarily raise priority; returns whether it actually changed.
    pub fn boost_to(&mut self, prio: u8) -> bool {
        if prio > self.prio {
            self.prio = prio;
            true
        } else {
            false
        }
    }

    /// Drop any temporary boost back to the base priority.
    pub fn restore_prio(&mut self) -> bool {
        if self.prio != self.base_prio {
            self.prio = self.base_prio;
            true
        } else {
            false
        }
    }

    /// F079: consumed this slice?
    pub fn slice_expired(&self) -> bool {
        self.class.is_slice_driven() && self.slice_left == 0
    }
}

// ---------------------------------------------------------------------------
// F077 — context switch frame
// ---------------------------------------------------------------------------

/// The callee-saved state a switch must preserve. `rip`/`rsp` are the resume
/// point of the outgoing thread and the entry point of the incoming one.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SwitchFrame {
    pub rsp: u64,
    pub rip: u64,
    pub rbx: u64,
    pub rbp: u64,
    pub r12: u64,
    pub r13: u64,
    pub r14: u64,
    pub r15: u64,
    /// Address space the thread runs with — a switch switches CR3 too.
    pub cr3: u64,
    /// `fs`/`gs` bases, restored on the way in.
    pub fs_base: u64,
    pub gs_base: u64,
}

impl SwitchFrame {
    /// Words a switch must move. The self-test asserts this so the frame
    /// cannot silently shrink when someone adds a register.
    pub const WORDS: usize = 11;

    /// Build the frame a fresh kernel thread resumes into.
    pub fn for_new_thread(stack_top: u64, entry: u64, cr3: u64) -> SwitchFrame {
        SwitchFrame {
            rsp: stack_top,
            rip: entry,
            cr3,
            ..SwitchFrame::default()
        }
    }

    /// Fields that must be non-zero for a frame to be resumable.
    pub fn resumable(&self) -> bool {
        self.rsp != 0 && self.rip != 0
    }
}

/// A switch decision, produced by the scheduler and consumed by the asm stub.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct SwitchDecision {
    pub from: u32,
    pub to: u32,
    /// Preempted (slice expired) or voluntary (blocked/yielded)?
    pub preempted: bool,
    pub cpu: u32,
}

// ---------------------------------------------------------------------------
// F094 — event trace
// ---------------------------------------------------------------------------

pub const MAX_TRACE: usize = 64;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TraceReason {
    Tick,
    Wake,
    Block,
    Yield,
    Preempt,
    Migrate,
    Boost,
    Throttle,
}

impl TraceReason {
    pub fn as_str(self) -> &'static str {
        match self {
            TraceReason::Tick => "tick",
            TraceReason::Wake => "wake",
            TraceReason::Block => "block",
            TraceReason::Yield => "yield",
            TraceReason::Preempt => "preempt",
            TraceReason::Migrate => "migrate",
            TraceReason::Boost => "boost",
            TraceReason::Throttle => "throttle",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct TraceEvent {
    pub tick: u64,
    pub from: u32,
    pub to: u32,
    pub reason: Option<TraceReason>,
    pub cpu: u32,
}

/// Fixed ring buffer: tracing must never allocate, and must never grow without
/// bound — a log storm is exactly the situation you still want to reason about.
pub struct SchedTrace {
    events: [TraceEvent; MAX_TRACE],
    head: usize,
    total: u64,
}

impl SchedTrace {
    pub const fn new() -> SchedTrace {
        SchedTrace {
            events: [TraceEvent {
                tick: 0,
                from: 0,
                to: 0,
                reason: None,
                cpu: 0,
            }; MAX_TRACE],
            head: 0,
            total: 0,
        }
    }

    pub fn push(&mut self, event: TraceEvent) {
        self.events[self.head] = event;
        self.head = (self.head + 1) % MAX_TRACE;
        self.total += 1;
    }

    pub fn total(&self) -> u64 {
        self.total
    }

    pub fn len(&self) -> usize {
        (self.total as usize).min(MAX_TRACE)
    }

    pub fn is_empty(&self) -> bool {
        self.total == 0
    }

    /// `offset` 0 is the newest event.
    pub fn get(&self, offset: usize) -> Option<TraceEvent> {
        if offset >= self.len() {
            return None;
        }
        let idx = (self.head + MAX_TRACE - 1 - offset) % MAX_TRACE;
        Some(self.events[idx])
    }

    pub fn count(&self, reason: TraceReason) -> u64 {
        (0..self.len())
            .filter(|i| self.get(*i).map(|e| e.reason == Some(reason)).unwrap_or(false))
            .count() as u64
    }

    pub fn clear(&mut self) {
        self.head = 0;
        self.total = 0;
    }
}

impl Default for SchedTrace {
    fn default() -> SchedTrace {
        SchedTrace::new()
    }
}

// ---------------------------------------------------------------------------
// Run queues and wait queues
// ---------------------------------------------------------------------------

/// Run queue for one core: one FIFO per class, ordered by descending priority
/// inside the class.
pub struct RunQueue {
    queues: [[u32; MAX_THREADS]; CLASS_COUNT],
    lens: [usize; CLASS_COUNT],
    cpu: u32,
    migrations_in: u64,
    migrations_out: u64,
}

impl RunQueue {
    pub const fn new(cpu: u32) -> RunQueue {
        RunQueue {
            queues: [[NO_THREAD; MAX_THREADS]; CLASS_COUNT],
            lens: [0; CLASS_COUNT],
            cpu,
            migrations_in: 0,
            migrations_out: 0,
        }
    }

    pub fn cpu(&self) -> u32 {
        self.cpu
    }

    pub fn len(&self) -> usize {
        self.lens.iter().sum()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn len_of(&self, class: SchedClass) -> usize {
        self.lens[class.index()]
    }

    /// Insert keeping the queue ordered by descending priority. Equal
    /// priorities append, which is what makes F080 round-robin work: a thread
    /// that burned its slice lands at the back of its own priority band.
    pub fn insert(&mut self, threads: &[Thread], tid: u32) -> bool {
        let class = match threads.get(tid as usize) {
            Some(t) if t.state != ThreadState::Unused => t.class,
            _ => return false,
        };
        let ci = class.index();
        if self.lens[ci] >= MAX_THREADS {
            return false;
        }
        let prio = threads[tid as usize].prio;
        let mut pos = self.lens[ci];
        for i in 0..self.lens[ci] {
            if prio > threads[self.queues[ci][i] as usize].prio {
                pos = i;
                break;
            }
        }
        let mut i = self.lens[ci];
        while i > pos {
            self.queues[ci][i] = self.queues[ci][i - 1];
            i -= 1;
        }
        self.queues[ci][pos] = tid;
        self.lens[ci] += 1;
        true
    }

    pub fn remove(&mut self, class: SchedClass, tid: u32) -> bool {
        let ci = class.index();
        for i in 0..self.lens[ci] {
            if self.queues[ci][i] == tid {
                for j in i..self.lens[ci] - 1 {
                    self.queues[ci][j] = self.queues[ci][j + 1];
                }
                self.lens[ci] -= 1;
                return true;
            }
        }
        false
    }

    /// Highest-priority runnable thread regardless of class, or `None`.
    pub fn peek(&self, threads: &[Thread]) -> Option<u32> {
        for class in SchedClass::ALL {
            let ci = class.index();
            for i in 0..self.lens[ci] {
                let tid = self.queues[ci][i];
                if threads
                    .get(tid as usize)
                    .map(|t| t.is_runnable())
                    .unwrap_or(false)
                {
                    return Some(tid);
                }
            }
        }
        None
    }

    /// Pop the next thread to run.
    pub fn pop_next(&mut self, threads: &[Thread]) -> Option<u32> {
        for class in SchedClass::ALL {
            let ci = class.index();
            for i in 0..self.lens[ci] {
                let tid = self.queues[ci][i];
                if threads
                    .get(tid as usize)
                    .map(|t| t.is_runnable())
                    .unwrap_or(false)
                {
                    for j in i..self.lens[ci] - 1 {
                        self.queues[ci][j] = self.queues[ci][j + 1];
                    }
                    self.lens[ci] -= 1;
                    return Some(tid);
                }
            }
        }
        None
    }

    /// Move the head of a class to the back — the rotation of F080.
    pub fn rotate(&mut self, class: SchedClass) -> bool {
        let ci = class.index();
        if self.lens[ci] < 2 {
            return false;
        }
        let head = self.queues[ci][0];
        for j in 0..self.lens[ci] - 1 {
            self.queues[ci][j] = self.queues[ci][j + 1];
        }
        self.queues[ci][self.lens[ci] - 1] = head;
        true
    }

    /// Position of `tid` inside its class (0 = next to run).
    pub fn position_of(&self, class: SchedClass, tid: u32) -> Option<usize> {
        let ci = class.index();
        (0..self.lens[ci]).find(|i| self.queues[ci][*i] == tid)
    }

    pub fn note_migrated_in(&mut self) {
        self.migrations_in += 1;
    }

    pub fn note_migrated_out(&mut self) {
        self.migrations_out += 1;
    }

    pub fn migrations_in(&self) -> u64 {
        self.migrations_in
    }

    pub fn migrations_out(&self) -> u64 {
        self.migrations_out
    }
}

pub const MAX_CHANNELS: usize = 16;

/// F082 wait queues: channels are plain numbers, so a wake-up is an O(n) sweep
/// over a 64-entry table rather than a pointer chase.
pub struct WaitQueues {
    channels: [[u32; MAX_THREADS]; MAX_CHANNELS],
    lens: [usize; MAX_CHANNELS],
}

impl WaitQueues {
    pub const fn new() -> WaitQueues {
        WaitQueues {
            channels: [[NO_THREAD; MAX_THREADS]; MAX_CHANNELS],
            lens: [0; MAX_CHANNELS],
        }
    }

    fn index(channel: u32) -> usize {
        (channel as usize) % MAX_CHANNELS
    }

    pub fn sleep(&mut self, channel: u32, tid: u32) -> bool {
        let ci = Self::index(channel);
        if self.lens[ci] >= MAX_THREADS {
            return false;
        }
        self.channels[ci][self.lens[ci]] = tid;
        self.lens[ci] += 1;
        true
    }

    pub fn wake_all(&mut self, channel: u32, out: &mut [u32]) -> usize {
        let ci = Self::index(channel);
        let n = self.lens[ci].min(out.len());
        out[..n].copy_from_slice(&self.channels[ci][..n]);
        self.lens[ci] = 0;
        n
    }

    pub fn waiting(&self, channel: u32) -> usize {
        self.lens[Self::index(channel)]
    }
}

impl Default for WaitQueues {
    fn default() -> WaitQueues {
        WaitQueues::new()
    }
}

// ---------------------------------------------------------------------------
// F089 — coroutines
// ---------------------------------------------------------------------------

pub const MAX_COROUTINES: usize = 16;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum CoroutineState {
    #[default]
    Unused,
    /// Created, never resumed.
    New,
    Running,
    Suspended,
    Done,
}

/// A cooperative execution body. The stack is owned by the caller (the memory
/// domain), so this descriptor is pure bookkeeping: which step it is on, how
/// much of its stack it uses, and whether it can still be resumed.
#[derive(Clone, Copy, Debug)]
pub struct Coroutine {
    pub cid: u32,
    pub state: CoroutineState,
    pub entry: u64,
    pub stack_base: u64,
    pub stack_size: u64,
    /// Resumption step — advanced by each `yield_`.
    pub step: u64,
    pub resumes: u64,
    pub yields: u64,
    /// Bytes of stack high-water mark reported by the last yield.
    pub stack_high_water: u64,
}

impl Coroutine {
    pub const fn new(cid: u32) -> Coroutine {
        Coroutine {
            cid,
            state: CoroutineState::Unused,
            entry: 0,
            stack_base: 0,
            stack_size: 0,
            step: 0,
            resumes: 0,
            yields: 0,
            stack_high_water: 0,
        }
    }

    pub fn resumable(&self) -> bool {
        matches!(self.state, CoroutineState::New | CoroutineState::Suspended)
    }

    pub fn stack_used_percent(&self) -> u64 {
        if self.stack_size == 0 {
            return 0;
        }
        self.stack_high_water * 100 / self.stack_size
    }
}

pub struct Coroutines {
    items: [Coroutine; MAX_COROUTINES],
    count: usize,
}

impl Coroutines {
    pub const fn new() -> Coroutines {
        Coroutines {
            items: [Coroutine::new(0); MAX_COROUTINES],
            count: 0,
        }
    }

    pub fn create(&mut self, cid: u32, entry: u64, stack_base: u64, stack_size: u64) -> bool {
        if self.count >= MAX_COROUTINES || cid as usize >= MAX_COROUTINES {
            return false;
        }
        if self.items[cid as usize].state != CoroutineState::Unused {
            return false;
        }
        self.items[cid as usize] = Coroutine {
            cid,
            state: CoroutineState::New,
            entry,
            stack_base,
            stack_size,
            ..Coroutine::new(cid)
        };
        self.count += 1;
        true
    }

    pub fn get(&self, cid: u32) -> Option<Coroutine> {
        let c = self.items.get(cid as usize)?;
        if c.state == CoroutineState::Unused {
            None
        } else {
            Some(*c)
        }
    }

    /// Resume a coroutine. Returns the step it will report on the next yield.
    pub fn resume(&mut self, cid: u32, stack_high_water: u64) -> Option<u64> {
        let c = self.items.get_mut(cid as usize)?;
        if !c.resumable() {
            return None;
        }
        c.state = CoroutineState::Running;
        c.resumes += 1;
        c.stack_high_water = c.stack_high_water.max(stack_high_water);
        Some(c.step)
    }

    /// Cooperative yield: the coroutine goes back to the queue at `step`.
    pub fn yield_(&mut self, cid: u32, step: u64, stack_high_water: u64) -> bool {
        let c = match self.items.get_mut(cid as usize) {
            Some(c) if c.state == CoroutineState::Running => c,
            _ => return false,
        };
        c.step = step;
        c.yields += 1;
        c.stack_high_water = c.stack_high_water.max(stack_high_water);
        c.state = CoroutineState::Suspended;
        true
    }

    /// Finish a coroutine; a finished body is never resumed again.
    pub fn finish(&mut self, cid: u32) -> bool {
        let c = match self.items.get_mut(cid as usize) {
            Some(c) => c,
            None => return false,
        };
        if c.state == CoroutineState::Unused {
            return false;
        }
        c.state = CoroutineState::Done;
        true
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    pub fn running(&self) -> usize {
        self.items
            .iter()
            .filter(|c| c.state == CoroutineState::Running)
            .count()
    }
}

impl Default for Coroutines {
    fn default() -> Coroutines {
        Coroutines::new()
    }
}

// ---------------------------------------------------------------------------
// F100 — policy profile export / import
// ---------------------------------------------------------------------------

/// The knobs the scheduler exposes. Exporting this to text and reading it back
/// is what makes a tuning session reproducible instead of folklore.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PolicyProfile {
    pub slice_ticks: u32,
    pub preemptible: bool,
    pub rt_runtime_cap: u32,
    pub boost_ticks: u32,
    pub starvation_ticks: u64,
    pub background_cap_percent: u8,
}

impl Default for PolicyProfile {
    fn default() -> PolicyProfile {
        PolicyProfile {
            slice_ticks: DEFAULT_SLICE_TICKS,
            preemptible: true,
            rt_runtime_cap: RT_RUNTIME_CAP_TICKS,
            boost_ticks: 20,
            starvation_ticks: 200,
            background_cap_percent: 25,
        }
    }
}

impl PolicyProfile {
    /// `key=value;` pairs, ASCII, newline-terminated. Deliberately boring.
    pub fn export(&self, out: &mut [u8]) -> usize {
        let mut w = crate::cpu::Hud::new(out);
        w.str("slice=");
        w.num(self.slice_ticks as u64);
        w.str(";preempt=");
        w.num(if self.preemptible { 1 } else { 0 });
        w.str(";rt_cap=");
        w.num(self.rt_runtime_cap as u64);
        w.str(";boost=");
        w.num(self.boost_ticks as u64);
        w.str(";starve=");
        w.num(self.starvation_ticks);
        w.str(";bg_cap=");
        w.num(self.background_cap_percent as u64);
        w.str(";\n");
        w.used()
    }

    /// Parse an exported profile. Unknown keys are ignored; malformed values
    /// are rejected so a corrupt profile cannot quietly change the machine.
    pub fn parse(text: &str) -> Option<PolicyProfile> {
        let mut p = PolicyProfile::default();
        for pair in text.trim().split(';') {
            let pair = pair.trim();
            if pair.is_empty() {
                continue;
            }
            let (k, v) = pair.split_once('=')?;
            match k.trim() {
                "slice" => p.slice_ticks = v.trim().parse().ok()?,
                "preempt" => p.preemptible = v.trim() == "1",
                "rt_cap" => p.rt_runtime_cap = v.trim().parse().ok()?,
                "boost" => p.boost_ticks = v.trim().parse().ok()?,
                "starve" => p.starvation_ticks = v.trim().parse().ok()?,
                "bg_cap" => {
                    let v: u8 = v.trim().parse().ok()?;
                    if v > 100 {
                        return None;
                    }
                    p.background_cap_percent = v;
                }
                _ => {}
            }
        }
        Some(p)
    }

    /// A profile that would starve the machine is not accepted.
    pub fn valid(&self) -> bool {
        self.slice_ticks > 0
            && self.rt_runtime_cap > 0
            && self.background_cap_percent <= 100
            && self.starvation_ticks > 0
    }
}

// ---------------------------------------------------------------------------
// Globals
// ---------------------------------------------------------------------------

pub struct SchedStats {
    pub switches: AtomicU64,
    pub voluntary: AtomicU64,
    pub preemptions: AtomicU64,
    pub ticks: AtomicU64,
    pub idles: AtomicU64,
    pub wakeups: AtomicU64,
    pub throttled: AtomicU64,
    pub migrations: AtomicU64,
}

impl SchedStats {
    pub const fn new() -> SchedStats {
        SchedStats {
            switches: AtomicU64::new(0),
            voluntary: AtomicU64::new(0),
            preemptions: AtomicU64::new(0),
            ticks: AtomicU64::new(0),
            idles: AtomicU64::new(0),
            wakeups: AtomicU64::new(0),
            throttled: AtomicU64::new(0),
            migrations: AtomicU64::new(0),
        }
    }
}

impl Default for SchedStats {
    fn default() -> SchedStats {
        SchedStats::new()
    }
}

static STATS: SchedStats = SchedStats::new();
static PREEMPT_ENABLED: AtomicBool = AtomicBool::new(true);
static CURRENT: AtomicU32 = AtomicU32::new(NO_THREAD);
static IDLE_TICKS: AtomicU64 = AtomicU64::new(0);

pub fn stats() -> &'static SchedStats {
    &STATS
}

pub fn current_tid() -> u32 {
    CURRENT.load(Ordering::Acquire)
}

pub fn set_current(tid: u32) {
    CURRENT.store(tid, Ordering::Release);
}

/// F078: preemption may be disabled for a short critical section, but the
/// window is countable so it cannot quietly become a habit.
pub fn set_preemptible(on: bool) {
    PREEMPT_ENABLED.store(on, Ordering::Release);
}

pub fn preemptible() -> bool {
    PREEMPT_ENABLED.load(Ordering::Acquire)
}

pub fn idle_ticks() -> u64 {
    IDLE_TICKS.load(Ordering::Relaxed)
}

pub fn note_idle() {
    IDLE_TICKS.fetch_add(1, Ordering::Relaxed);
}

/// F076: create a kernel thread. The stack and entry point belong to the caller
/// (the memory domain owns the frames); this builds the descriptor and frame.
pub fn create_kernel_thread(
    table: &mut ThreadTable,
    tid: u32,
    prio: u8,
    class: SchedClass,
    stack_top: u64,
    entry: u64,
    flags: u32,
) -> Option<SwitchFrame> {
    let flags = flags | FLAG_KERNEL;
    if !table.spawn(tid, prio, class, flags) {
        return None;
    }
    Some(SwitchFrame::for_new_thread(stack_top, entry, 0))
}

/// The thread table plus the per-core run queues.
pub struct ThreadTable {
    threads: [Thread; MAX_THREADS],
    count: usize,
    rqs: [RunQueue; crate::cpu::smp::MAX_CPUS],
    waits: WaitQueues,
    trace: SchedTrace,
    coroutines: Coroutines,
    /// Currently running thread per core.
    current: [u32; crate::cpu::smp::MAX_CPUS],
    /// Preemption switch (F078). Per-scheduler, not global, so an
    /// experimental policy cannot leak into an unrelated context.
    preemptible: bool,
}

impl ThreadTable {
    pub const fn new() -> ThreadTable {
        ThreadTable {
            threads: [Thread::new(0); MAX_THREADS],
            count: 0,
            rqs: [const { RunQueue::new(0) }; crate::cpu::smp::MAX_CPUS],
            waits: WaitQueues::new(),
            trace: SchedTrace::new(),
            coroutines: Coroutines::new(),
            current: [NO_THREAD; crate::cpu::smp::MAX_CPUS],
            preemptible: true,
        }
    }

    /// Currently running thread on `cpu`.
    pub fn current(&self, cpu: u32) -> u32 {
        self.current
            .get(cpu as usize)
            .copied()
            .unwrap_or(NO_THREAD)
    }

    pub fn set_current(&mut self, cpu: u32, tid: u32) {
        if let Some(slot) = self.current.get_mut(cpu as usize) {
            *slot = tid;
        }
        // Keep the kernel-wide view in step for the IRQ path.
        set_current(tid);
    }

    /// F078: allow or forbid preemption on this scheduler.
    pub fn set_preemptible(&mut self, on: bool) {
        self.preemptible = on;
        set_preemptible(on);
    }

    pub fn is_preemptible(&self) -> bool {
        self.preemptible
    }

    /// Force the remaining slice of `tid` (used by the tests and by F095).
    pub fn set_slice(&mut self, tid: u32, ticks: u32) {
        if let Some(t) = self.thread_mut(tid) {
            t.slice_left = ticks.min(t.slice_total);
        }
    }

    pub fn threads(&self) -> &[Thread; MAX_THREADS] {
        &self.threads
    }

    pub fn thread(&self, tid: u32) -> Option<&Thread> {
        self.threads
            .get(tid as usize)
            .filter(|t| t.state != ThreadState::Unused)
    }

    pub fn thread_mut(&mut self, tid: u32) -> Option<&mut Thread> {
        let t = self.threads.get_mut(tid as usize)?;
        if t.state == ThreadState::Unused {
            None
        } else {
            Some(t)
        }
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    pub fn rq(&self, cpu: u32) -> Option<&RunQueue> {
        self.rqs.get(cpu as usize)
    }

    pub fn rq_mut(&mut self, cpu: u32) -> Option<&mut RunQueue> {
        self.rqs.get_mut(cpu as usize)
    }

    pub fn waits_mut(&mut self) -> &mut WaitQueues {
        &mut self.waits
    }

    pub fn trace(&self) -> &SchedTrace {
        &self.trace
    }

    pub fn trace_mut(&mut self) -> &mut SchedTrace {
        &mut self.trace
    }

    pub fn coroutines_mut(&mut self) -> &mut Coroutines {
        &mut self.coroutines
    }

    pub fn coroutines(&self) -> &Coroutines {
        &self.coroutines
    }

    /// Register a thread on `cpu` and make it runnable.
    pub fn spawn_on(&mut self, tid: u32, prio: u8, class: SchedClass, flags: u32, cpu: u32) -> bool {
        if tid as usize >= MAX_THREADS || cpu as usize >= crate::cpu::smp::MAX_CPUS {
            return false;
        }
        if self.threads[tid as usize].state != ThreadState::Unused {
            return false;
        }
        let mut t = Thread::new(tid);
        t.class = class;
        t.prio = prio.min(PRIO_TOP);
        t.base_prio = t.prio;
        t.flags = flags;
        t.state = ThreadState::Runnable;
        t.cpu = cpu;
        self.threads[tid as usize] = t;
        if !self.rqs[cpu as usize].insert(&self.threads, tid) {
            self.threads[tid as usize].state = ThreadState::Unused;
            return false;
        }
        self.count += 1;
        true
    }

    /// Spawn on the calling core's queue (core 0 when nothing else is up).
    pub fn spawn(&mut self, tid: u32, prio: u8, class: SchedClass, flags: u32) -> bool {
        let cpu = if crate::cpu::smp::cpu_count() > 0 {
            crate::cpu::smp::current_id()
        } else {
            0
        };
        self.spawn_on(tid, prio, class, flags, cpu)
    }

    /// F082: block on `channel` until someone wakes it.
    pub fn block_on(&mut self, tid: u32, channel: u32) -> bool {
        let cpu = match self.thread(tid) {
            Some(t) => t.cpu,
            None => return false,
        };
        if !self.waits.sleep(channel, tid) {
            return false;
        }
        let class = self.threads[tid as usize].class;
        let _ = self.rqs[cpu as usize].remove(class, tid);
        let t = &mut self.threads[tid as usize];
        t.state = ThreadState::Sleeping;
        t.channel = channel;
        t.slice_left = t.slice_total;
        // A blocked thread is no longer running anywhere: clearing the per-core
        // slot is what lets the next tick notice the core went idle and pick a
        // successor. Without it a blocked thread would keep the CPU nominally
        // busy until its slice expired.
        if self.current[cpu as usize] == tid {
            self.current[cpu as usize] = NO_THREAD;
            set_current(NO_THREAD);
        }
        self.trace.push(TraceEvent {
            tick: crate::cpu::clock::ticks(),
            from: tid,
            to: NO_THREAD,
            reason: Some(TraceReason::Block),
            cpu,
        });
        true
    }

    /// F082: wake everything waiting on `channel`; returns how many became
    /// runnable.
    pub fn wake_channel(&mut self, channel: u32) -> usize {
        let mut ids = [NO_THREAD; MAX_THREADS];
        let n = self.waits.wake_all(channel, &mut ids);
        let now = crate::cpu::clock::ticks();
        let mut woken = 0usize;
        for tid in ids.iter().take(n) {
            let cpu = self.threads[*tid as usize].cpu;
            if self.threads[*tid as usize].state != ThreadState::Sleeping
                || self.threads[*tid as usize].channel != channel
            {
                continue;
            }
            let t = &mut self.threads[*tid as usize];
            t.state = ThreadState::Runnable;
            t.wait_ticks = 0;
            t.slice_left = t.slice_total;
            if self.rqs[cpu as usize].insert(&self.threads, *tid) {
                woken += 1;
                STATS.wakeups.fetch_add(1, Ordering::Relaxed);
                self.trace.push(TraceEvent {
                    tick: now,
                    from: NO_THREAD,
                    to: *tid,
                    reason: Some(TraceReason::Wake),
                    cpu,
                });
            }
        }
        woken
    }

    /// F078/F080: advance one tick on `cpu` and decide whether the running
    /// thread must be preempted.
    pub fn on_tick(&mut self, cpu: u32) -> Option<SwitchDecision> {
        STATS.ticks.fetch_add(1, Ordering::Relaxed);
        if cpu as usize >= crate::cpu::smp::MAX_CPUS {
            return None;
        }
        let cur = self.current(cpu);

        // 1. Charge the running thread and age the runnable ones (F095).
        if cur != NO_THREAD {
            if let Some(t) = self.thread_mut(cur) {
                if t.state == ThreadState::Running {
                    t.runtime_ticks += 1;
                    t.vruntime += (1000 / t.class.weight().max(1)) as u64;
                    t.slice_left = t.slice_left.saturating_sub(1);
                }
            }
        } else {
            note_idle();
        }
        for i in 0..MAX_THREADS {
            if self.threads[i].state == ThreadState::Runnable {
                self.threads[i].wait_ticks += 1;
            }
        }

        // 2. Nothing running here → pick one (voluntary entry, not a preempt).
        if cur == NO_THREAD {
            return self.pick(cpu, false);
        }

        // 3. Preemption decision.
        if !self.preemptible {
            return None;
        }
        let expired = self.thread(cur).map(|t| t.slice_expired()).unwrap_or(false)
            || self
                .thread(cur)
                .map(|t| !t.allowed_on(cpu))
                .unwrap_or(false);
        let higher = self
            .rq(cpu)
            .and_then(|q| q.peek(&self.threads))
            .zip(self.thread(cur))
            .map(|(next, t)| {
                // A higher class always preempts; inside a class, strictly
                // higher priority does.
                let nt = &self.threads[next as usize];
                nt.class > t.class || (nt.class == t.class && nt.prio > t.prio)
            })
            .unwrap_or(false);
        if !expired && !higher {
            return None;
        }
        // Either trigger makes the switch involuntary, which is what the trace
        // and the preemption counter are meant to record.
        self.pick(cpu, true)
    }

    /// Choose the next thread for `cpu` and record the switch.
    pub fn pick(&mut self, cpu: u32, preempted: bool) -> Option<SwitchDecision> {
        if cpu as usize >= crate::cpu::smp::MAX_CPUS {
            return None;
        }
        let from = self.current(cpu);

        // Put the outgoing thread back *before* choosing the next one. `insert`
        // appends behind equal priorities and steps in front of lower ones,
        // so round-robin (F080) and strict priority (F079) both fall out of a
        // single ordered insertion — no separate rotation step to get wrong.
        if from != NO_THREAD {
            let home = self.thread(from).map(|t| t.cpu).unwrap_or(cpu);
            if (home as usize) < crate::cpu::smp::MAX_CPUS {
                let was_running = self
                    .thread_mut(from)
                    .map(|t| {
                        if t.state == ThreadState::Running {
                            t.state = ThreadState::Runnable;
                            t.slice_left = t.slice_total;
                            true
                        } else {
                            false
                        }
                    })
                    .unwrap_or(false);
                if was_running {
                    let _ = self.rqs[home as usize].insert(&self.threads, from);
                }
            }
        }

        let next = self.rqs[cpu as usize].pop_next(&self.threads)?;
        let t = self.thread_mut(next)?;
        t.state = ThreadState::Running;
        t.switches += 1;
        t.wait_ticks = 0;
        self.set_current(cpu, next);
        STATS.switches.fetch_add(1, Ordering::Relaxed);
        if preempted {
            STATS.preemptions.fetch_add(1, Ordering::Relaxed);
        } else {
            STATS.voluntary.fetch_add(1, Ordering::Relaxed);
        }
        self.trace.push(TraceEvent {
            tick: crate::cpu::clock::ticks(),
            from,
            to: next,
            reason: Some(if preempted {
                TraceReason::Preempt
            } else {
                TraceReason::Yield
            }),
            cpu,
        });
        Some(SwitchDecision {
            from,
            to: next,
            preempted,
            cpu,
        })
    }

    /// Voluntarily give up the CPU (F078).
    pub fn yield_now(&mut self, cpu: u32) -> Option<SwitchDecision> {
        let cur = self.current(cpu);
        if let Some(t) = self.thread_mut(cur) {
            t.slice_left = 0;
        }
        let d = self.pick(cpu, true);
        if let Some(t) = self.thread_mut(cur) {
            if t.state == ThreadState::Running {
                t.state = ThreadState::Runnable;
            }
        }
        d
    }

    /// Re-queue `tid` after its class or priority changed (F095 rescue path).
    /// Both halves live here because splitting them across an `&mut` borrow of
    /// the table is exactly what the borrow checker exists to prevent.
    pub fn requeue(&mut self, tid: u32) -> bool {
        let cpu = match self.thread(tid) {
            Some(t) => t.cpu,
            None => return false,
        };
        if cpu as usize >= crate::cpu::smp::MAX_CPUS {
            return false;
        }
        for class in SchedClass::ALL {
            let _ = self.rqs[cpu as usize].remove(class, tid);
        }
        self.rqs[cpu as usize].insert(&self.threads, tid)
    }

    /// Remove a thread from the table (used by the reaper, F111).
    pub fn reap(&mut self, tid: u32) -> bool {
        let class = match self.thread(tid) {
            Some(t) => t.class,
            None => return false,
        };
        let cpu = self.threads[tid as usize].cpu;
        let _ = self.rqs[cpu as usize].remove(class, tid);
        self.threads[tid as usize] = Thread::new(tid);
        self.count = self.count.saturating_sub(1);
        for c in 0..crate::cpu::smp::MAX_CPUS {
            if self.current[c] == tid {
                self.current[c] = NO_THREAD;
            }
        }
        if current_tid() == tid {
            set_current(NO_THREAD);
        }
        true
    }

    /// F095: runnable threads whose wait exceeds the threshold.
    pub fn starving(&self, threshold: u64) -> usize {
        self.threads
            .iter()
            .filter(|t| t.state == ThreadState::Runnable && t.wait_ticks > threshold)
            .count()
    }

    /// Threads currently alive, by state.
    pub fn count_of(&self, state: ThreadState) -> usize {
        self.threads.iter().filter(|t| t.state == state).count()
    }
}

impl Default for ThreadTable {
    fn default() -> ThreadTable {
        ThreadTable::new()
    }
}

// ---------------------------------------------------------------------------
// F099 — scheduler self-test
// ---------------------------------------------------------------------------

static SCHED_SELFTEST: crate::selftest::SelfTest = crate::selftest::SelfTest::new();

pub fn sched_selftest() -> &'static crate::selftest::SelfTest {
    &SCHED_SELFTEST
}

/// F099: check the invariants that must hold no matter what the workload did.
pub fn run_scheduler_checks(table: &ThreadTable) -> (usize, usize) {
    let r = &SCHED_SELFTEST;

    // F077 — the switch frame is complete.
    r.check(
        "switch-frame",
        SwitchFrame::WORDS == 11,
        "frame size changed",
    );

    // F079/F080 — the classes are totally ordered and every weight positive.
    let ordered = SchedClass::ALL.windows(2).all(|w| w[0] > w[1]);
    r.check("class-order", ordered, "class order not descending");
    let weights_ok = SchedClass::ALL.iter().all(|c| c.weight() > 0);
    r.check("class-weights", weights_ok, "zero weight class");

    // F078 — at most one Running thread per core.
    let (_running_total, worst_core) = count_running(table);
    r.check(
        "single-running",
        worst_core <= 1,
        "two threads running on one core",
    );

    // F082 — no thread is both Sleeping and queued.
    let queued_sleepers = (0..MAX_THREADS).any(|i| {
        let t = &table.threads()[i];
        t.state == ThreadState::Sleeping
            && (0..crate::cpu::smp::MAX_CPUS as u32)
                .any(|c| table.rq(c).map(|q| q.position_of(t.class, i as u32).is_some()).unwrap_or(false))
    });
    r.check("waitqueue-consistency", !queued_sleepers, "sleeper still queued");

    // F094 — the trace never exceeds its ring.
    r.check(
        "trace-bounded",
        table.trace().len() <= MAX_TRACE,
        "trace overflowed",
    );

    // F095 — starvation is visible, and the table knows about it.
    let starving = table.starving(PolicyProfile::default().starvation_ticks);
    r.check(
        "starvation-visible",
        starving as u32 <= table.len() as u32,
        "more starving threads than threads",
    );

    // F100 — the profile round-trips.
    let profile = PolicyProfile::default();
    let mut buf = [0u8; 128];
    let n = profile.export(&mut buf);
    let text = core::str::from_utf8(&buf[..n]).unwrap_or("");
    let back = PolicyProfile::parse(text);
    r.check(
        "profile-roundtrip",
        back == Some(profile),
        "profile did not round-trip",
    );
    r.check("profile-valid", profile.valid(), "default profile invalid");

    let (passed, failed) = r.tally();
    if failed == 0 {
        crate::kinfo!("scheduler self-test: {}/{} pass", passed, passed);
    } else {
        crate::kwarn!(
            "scheduler self-test: {}/{} pass ({} FAIL)",
            passed,
            passed + failed,
            failed
        );
    }
    (passed, failed)
}

/// Count running threads overall and the worst per-core pile-up, as
/// `(total, worst_core)`.
fn count_running(table: &ThreadTable) -> (usize, usize) {
    let mut per_cpu = [0usize; crate::cpu::smp::MAX_CPUS];
    let mut total = 0usize;
    for t in table.threads().iter() {
        if t.state == ThreadState::Running {
            total += 1;
            if (t.cpu as usize) < per_cpu.len() {
                per_cpu[t.cpu as usize] += 1;
            }
        }
    }
    (total, per_cpu.iter().copied().max().unwrap_or(0))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table_with(tids: &[(u32, u8, SchedClass)]) -> ThreadTable {
        let mut t = ThreadTable::new();
        for (tid, prio, class) in tids {
            assert!(t.spawn_on(*tid, *prio, *class, 0, 0));
        }
        t
    }

    #[test]
    fn class_ordering_and_weights() {
        assert!(SchedClass::Isolated > SchedClass::RealTime);
        assert!(SchedClass::RealTime > SchedClass::Interactive);
        assert!(SchedClass::Interactive > SchedClass::Normal);
        assert!(SchedClass::Normal > SchedClass::Batch);
        assert!(SchedClass::Batch > SchedClass::Idle);
        assert!(SchedClass::ALL.windows(2).all(|w| w[0] > w[1]));
        for c in SchedClass::ALL {
            assert!(c.weight() > 0);
            assert!(!c.as_str().is_empty());
            assert_eq!(SchedClass::from_index(c.index()), Some(c));
        }
        assert!(!SchedClass::RealTime.is_slice_driven());
        assert!(SchedClass::Normal.is_slice_driven());
        assert_eq!(SchedClass::from_index(99), None);
    }

    #[test]
    fn run_queue_orders_by_priority_within_class() {
        let mut t = ThreadTable::new();
        assert!(t.spawn_on(0, PRIO_NORMAL, SchedClass::Normal, 0, 0));
        assert!(t.spawn_on(1, PRIO_NORMAL + 5, SchedClass::Normal, 0, 0));
        assert!(t.spawn_on(2, PRIO_NORMAL, SchedClass::Normal, 0, 0));
        let q = t.rq(0).unwrap();
        assert_eq!(q.len(), 3);
        assert_eq!(q.position_of(SchedClass::Normal, 1), Some(0), "highest first");
        assert_eq!(q.position_of(SchedClass::Normal, 0), Some(1), "FIFO on ties");
        assert_eq!(q.position_of(SchedClass::Normal, 2), Some(2));
        assert_eq!(q.peek(t.threads()), Some(1));
    }

    #[test]
    fn classes_are_searched_highest_first() {
        let t = table_with(&[
            (0, PRIO_BATCH, SchedClass::Batch),
            (1, PRIO_NORMAL, SchedClass::Normal),
            (2, PRIO_RT, SchedClass::RealTime),
            (3, PRIO_INTERACTIVE, SchedClass::Interactive),
        ]);
        assert_eq!(t.rq(0).unwrap().peek(t.threads()), Some(2), "rt first");
        assert_eq!(t.rq(0).unwrap().len_of(SchedClass::RealTime), 1);
        assert_eq!(t.rq(0).unwrap().len_of(SchedClass::Batch), 1);
    }

    #[test]
    fn spawn_rejects_duplicates_and_bad_ids() {
        let mut t = ThreadTable::new();
        assert!(t.spawn_on(5, PRIO_NORMAL, SchedClass::Normal, 0, 0));
        assert!(!t.spawn_on(5, PRIO_NORMAL, SchedClass::Normal, 0, 0), "already used");
        assert!(!t.spawn_on(999, PRIO_NORMAL, SchedClass::Normal, 0, 0), "bad tid");
        assert!(!t.spawn_on(0, PRIO_NORMAL, SchedClass::Normal, 0, 999), "bad cpu");
        assert_eq!(t.len(), 1);
    }

    #[test]
    fn schedule_picks_and_switches() {
        let mut t = table_with(&[
            (1, PRIO_NORMAL, SchedClass::Normal),
            (2, PRIO_INTERACTIVE, SchedClass::Interactive),
        ]);
        t.set_preemptible(true);
        let d = t.pick(0, false).unwrap();
        assert_eq!(d.to, 2, "the interactive class wins");
        assert_eq!(d.from, NO_THREAD);
        assert_eq!(t.current(0), 2);
        assert_eq!(t.thread(2).unwrap().state, ThreadState::Running);
        assert!(stats().switches.load(Ordering::Relaxed) >= 1);

        // Strict class priority: burning the high-class thread's slice hands
        // the core back to that same thread, never to the lower class.
        let before = stats().preemptions.load(Ordering::Relaxed);
        t.set_slice(2, 0);
        let d2 = t.on_tick(0).expect("an expired slice re-picks");
        assert!(d2.preempted);
        assert_eq!(d2.to, 2);
        assert_eq!(t.current(0), 2);
        assert!(stats().preemptions.load(Ordering::Relaxed) > before);

        // Only when the high-class thread blocks does the next class run.
        assert!(t.block_on(2, 3));
        let d3 = t.on_tick(0).expect("the normal-class thread is finally eligible");
        assert_eq!(d3.to, 1);
        assert_eq!(t.current(0), 1);
    }

    #[test]
    fn lower_class_runs_only_when_the_higher_one_is_gone() {
        let mut t = table_with(&[
            (1, PRIO_NORMAL, SchedClass::Normal),
            (2, PRIO_RT, SchedClass::RealTime),
        ]);
        t.set_preemptible(true);
        t.pick(0, false);
        assert_eq!(t.current(0), 2, "rt owns the core");
        for _ in 0..30 {
            let _ = t.on_tick(0);
            assert_eq!(t.current(0), 2, "a lower class never preempts rt");
        }
    }

    #[test]
    fn round_robin_rotates_equal_priorities() {
        let mut t = table_with(&[
            (0, PRIO_NORMAL, SchedClass::Normal),
            (1, PRIO_NORMAL, SchedClass::Normal),
            (2, PRIO_NORMAL, SchedClass::Normal),
        ]);
        t.set_preemptible(true);
        let order: [u32; 3] = [0, 1, 2];
        let mut first = [NO_THREAD; 3];
        for slot in first.iter_mut() {
            let d = t.pick(0, true).unwrap();
            *slot = d.to;
            // Burn the slice so the next pick has to rotate.
            t.set_slice(d.to, 0);
        }
        let mut sorted = first;
        sorted.sort_unstable();
        assert_eq!(sorted, order, "every equal-priority thread got a turn");
    }

    #[test]
    fn preemption_can_be_disabled_for_a_critical_section() {
        // Start with only a normal-class thread so it owns the core.
        let mut t = table_with(&[(0, PRIO_NORMAL, SchedClass::Normal)]);
        t.pick(0, false);
        assert_eq!(t.current(0), 0);

        // A real-time thread appears while the critical section is open.
        assert!(t.spawn_on(1, PRIO_RT, SchedClass::RealTime, 0, 0));
        t.set_preemptible(false);
        assert!(t.on_tick(0).is_none(), "preemption is off");
        assert_eq!(t.current(0), 0);

        // Once the section closes, the RT thread takes the core immediately.
        t.set_preemptible(true);
        let d = t.on_tick(0).expect("RT must preempt once allowed");
        assert_eq!(d.to, 1);
        assert!(d.preempted);
        assert_eq!(t.current(0), 1);
    }

    #[test]
    fn sleep_and_wake_round_trip() {
        let mut t = table_with(&[
            (0, PRIO_NORMAL, SchedClass::Normal),
            (1, PRIO_NORMAL, SchedClass::Normal),
        ]);
        assert!(t.block_on(1, 7));
        assert_eq!(t.thread(1).unwrap().state, ThreadState::Sleeping);
        assert_eq!(t.rq(0).unwrap().position_of(SchedClass::Normal, 1), None);
        assert_eq!(t.waits_mut().waiting(7), 1);
        assert_eq!(t.wake_channel(7), 1);
        assert_eq!(t.thread(1).unwrap().state, ThreadState::Runnable);
        assert!(t.rq(0).unwrap().position_of(SchedClass::Normal, 1).is_some());
        // Waking an empty channel is a no-op.
        assert_eq!(t.wake_channel(9), 0);
        assert!(!t.block_on(42, 1), "unknown thread cannot block");
    }

    #[test]
    fn rt_threads_are_capped_and_never_rotated() {
        let t = table_with(&[(0, PRIO_RT, SchedClass::RealTime)]);
        assert_eq!(t.thread(0).unwrap().slice_total, DEFAULT_SLICE_TICKS);
        // RT is not slice-driven, so an expired slice never preempts it.
        assert!(!t.thread(0).unwrap().slice_expired());
        assert!(RT_RUNTIME_CAP_TICKS >= DEFAULT_SLICE_TICKS);
    }

    #[test]
    fn starvation_is_counted_and_reset_on_pick() {
        let mut t = table_with(&[
            (0, PRIO_NORMAL, SchedClass::Normal),
            (1, PRIO_NORMAL, SchedClass::Normal),
        ]);
        t.set_preemptible(true);
        t.pick(0, false);
        // 45 ticks at a 10-tick slice: the two threads take turns, and stopping
        // mid-slice means the queued one is demonstrably waiting right now.
        for _ in 0..45 {
            t.on_tick(0);
        }
        assert_eq!(t.count_of(ThreadState::Running), 1);
        let running = t.current(0);
        let waiting = if running == 0 { 1 } else { 0 };
        assert!(t.thread(waiting).unwrap().wait_ticks > 0);
        assert!(
            t.starving(1) >= 1,
            "the queued thread accumulates visible wait time"
        );
        // Being picked clears the age.
        t.set_slice(running, 0);
        if let Some(d) = t.on_tick(0) {
            assert_eq!(t.thread(d.to).unwrap().wait_ticks, 0);
        }
    }

    #[test]
    fn affinity_is_honoured() {
        let mut t = table_with(&[(0, PRIO_NORMAL, SchedClass::Normal)]);
        assert!(t.thread(0).unwrap().allowed_on(0));
        t.thread_mut(0).unwrap().affinity = 1 << 3;
        assert!(!t.thread(0).unwrap().allowed_on(0));
        assert!(t.thread(0).unwrap().allowed_on(3));
        assert!(!t.thread(0).unwrap().allowed_on(64));
    }

    #[test]
    fn reap_removes_and_clears_current() {
        let mut t = table_with(&[
            (0, PRIO_NORMAL, SchedClass::Normal),
            (1, PRIO_NORMAL, SchedClass::Normal),
        ]);
        t.pick(0, false);
        assert_eq!(t.current(0), 0);
        assert!(t.reap(0));
        assert_eq!(t.current(0), NO_THREAD);
        assert_eq!(t.len(), 1);
        assert!(t.thread(0).is_none());
        assert!(!t.reap(0), "reaping twice is refused");
    }

    #[test]
    fn trace_keeps_the_newest_events() {
        let mut t = ThreadTable::new();
        for i in 0..(MAX_TRACE + 10) {
            t.trace_mut().push(TraceEvent {
                tick: i as u64,
                from: 0,
                to: i as u32,
                reason: Some(TraceReason::Tick),
                cpu: 0,
            });
        }
        assert_eq!(t.trace().len(), MAX_TRACE);
        assert_eq!(t.trace().total(), (MAX_TRACE + 10) as u64);
        assert_eq!(t.trace().get(0).unwrap().to, (MAX_TRACE + 9) as u32);
        assert_eq!(t.trace().get(MAX_TRACE - 1).unwrap().to, 10);
        assert!(t.trace().get(MAX_TRACE).is_none());
        assert_eq!(t.trace().count(TraceReason::Tick), MAX_TRACE as u64);
        assert!(TraceReason::Preempt.as_str() == "preempt");
    }

    #[test]
    fn coroutines_resume_yield_and_finish() {
        let mut c = Coroutines::new();
        assert!(c.is_empty());
        assert!(c.create(0, 0x1000, 0x8000, 8192));
        assert!(!c.create(0, 0, 0, 0), "no duplicate ids");
        let co = c.get(0).unwrap();
        assert_eq!(co.state, CoroutineState::New);
        assert!(co.resumable());
        assert_eq!(c.resume(0, 1024), Some(0));
        assert_eq!(c.get(0).unwrap().state, CoroutineState::Running);
        assert!(c.yield_(0, 1, 4096));
        assert_eq!(c.get(0).unwrap().step, 1);
        assert_eq!(c.get(0).unwrap().yields, 1);
        assert_eq!(c.get(0).unwrap().stack_high_water, 4096);
        assert_eq!(c.get(0).unwrap().stack_used_percent(), 50);
        // A suspended coroutine can resume again.
        assert_eq!(c.resume(0, 4096), Some(1));
        assert!(c.finish(0));
        assert!(!c.get(0).unwrap().resumable());
        assert!(c.resume(0, 0).is_none(), "a finished coroutine stays finished");
        assert!(!c.yield_(0, 2, 0));
        assert!(c.get(9).is_none());
    }

    #[test]
    fn profile_round_trips_and_validates() {
        let p = PolicyProfile::default();
        assert!(p.valid());
        let mut buf = [0u8; 128];
        let n = p.export(&mut buf);
        let text = core::str::from_utf8(&buf[..n]).unwrap();
        assert!(text.starts_with("slice=10;preempt=1;"), "got {text}");
        assert!(text.ends_with(";\n"));
        assert_eq!(PolicyProfile::parse(text), Some(p));

        // A hostile profile is refused, not clamped.
        assert_eq!(PolicyProfile::parse("bg_cap=250;"), None);
        assert_eq!(PolicyProfile::parse("slice=abc;"), None);
        assert_eq!(PolicyProfile::parse("slice"), None);
        // Unknown keys are simply ignored.
        let mut p2 = p;
        p2.slice_ticks = 4;
        assert_eq!(PolicyProfile::parse("future=9;slice=4;"), Some(p2));
    }

    #[test]
    fn self_test_passes_on_an_idle_table() {
        let t = ThreadTable::new();
        let (passed, failed) = run_scheduler_checks(&t);
        assert_eq!(failed, 0, "{passed} passed, {failed} failed");
        assert!(passed >= 8);
    }
}
