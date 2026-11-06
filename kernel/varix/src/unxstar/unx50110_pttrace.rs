//! UNX-F50110 直通埋点（判定/透传/回退三点）（0x439e0 段）
//!
//! 锚点：UNX-F50110-J1 埋点 100% 覆盖；判据锚定 体验十三·补。
//!
//! 直通链只有三个可观测点：**判定**（要不要直通）、**透传**（原样交出）、
//! **回退**（改走光栅化）。三点之外一律不算证据——因此「覆盖 100%」的定义是
//! 每个作业都留下「判定 + 唯一终结」并把作业收口，而不是「打了什么日志」。
//!
//! 数据段：`SEG_BASE = 0x439e0`；诊断码闭集 = `SEG_BASE + 1..=8`。
//!
//! 结构清单：
//! * `PtPoint` / `JobPhase` —— 三点闭集 + 每作业状态机；
//! * `TraceRing` —— 定容环形缓冲，覆盖写显式记账（written = len + evicted）；
//! * `Span` / `JobTrace` —— 单条埋点与每作业轨迹；
//! * `TraceBook` —— 序号/时钟单调闸 + 状态机闸 + 覆盖审计；
//! * `injection_core` —— 8 类坏埋点序列逐类断码。

use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（本模块独占 0x439e0）。
pub const SEG_BASE: u32 = 0x439e0;

pub const E_EMPTY: u16 = 1;
pub const E_DUP: u16 = 2;
pub const E_ORDER: u16 = 3;
pub const E_UNKNOWN: u16 = 4;
pub const E_CAPACITY: u16 = 5;
pub const E_SEQ: u16 = 6;
pub const E_COVERAGE: u16 = 7;
pub const E_TICK: u16 = 8;

/// 埋点错误闭集。`code()` 返回 `SEG_BASE + n`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PtTraceError {
    /// 账本里一个作业都没有（空批）。
    Empty,
    /// 同一作业同一埋点重复（判定两次 / 同一终结两次 / 重复收口）。
    Dup,
    /// 状态机非法转移（孤儿回退、未判定即收口、判定晚于终结）。
    Order,
    /// 作业号非法或收口未登记的作业。
    UnknownJob,
    /// 作业表或环形缓冲容量为 0 / 作业表满。
    Capacity,
    /// 落账序号不单调（账本被外部篡改或跨代拼接）。
    SeqDrift,
    /// 覆盖审计发现未收口作业。
    Coverage,
    /// 时钟回退（tick 小于上一条）。
    Tick,
}

impl PtTraceError {
    pub fn n(self) -> u16 {
        match self {
            PtTraceError::Empty => E_EMPTY,
            PtTraceError::Dup => E_DUP,
            PtTraceError::Order => E_ORDER,
            PtTraceError::UnknownJob => E_UNKNOWN,
            PtTraceError::Capacity => E_CAPACITY,
            PtTraceError::SeqDrift => E_SEQ,
            PtTraceError::Coverage => E_COVERAGE,
            PtTraceError::Tick => E_TICK,
        }
    }

    pub fn code(self) -> u32 {
        SEG_BASE + self.n() as u32
    }

    pub fn message(self) -> &'static str {
        match self {
            PtTraceError::Empty => "trace book has no job",
            PtTraceError::Dup => "duplicate trace point",
            PtTraceError::Order => "trace point violates job state machine",
            PtTraceError::UnknownJob => "unknown job",
            PtTraceError::Capacity => "job table or ring capacity exhausted",
            PtTraceError::SeqDrift => "trace sequence not monotonic",
            PtTraceError::Coverage => "job not closed, coverage incomplete",
            PtTraceError::Tick => "tick regression",
        }
    }

    /// 码值反解（零漂移判据）。
    pub fn from_code(code: u32) -> Option<PtTraceError> {
        if code <= SEG_BASE {
            return None;
        }
        match (code - SEG_BASE) as u16 {
            E_EMPTY => Some(PtTraceError::Empty),
            E_DUP => Some(PtTraceError::Dup),
            E_ORDER => Some(PtTraceError::Order),
            E_UNKNOWN => Some(PtTraceError::UnknownJob),
            E_CAPACITY => Some(PtTraceError::Capacity),
            E_SEQ => Some(PtTraceError::SeqDrift),
            E_COVERAGE => Some(PtTraceError::Coverage),
            E_TICK => Some(PtTraceError::Tick),
            _ => None,
        }
    }
}

/// 三个埋点。顺序即语义顺序：判定必须先于终结。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PtPoint {
    Decide,
    Pass,
    Fallback,
}

impl PtPoint {
    pub fn code(self) -> u8 {
        match self {
            PtPoint::Decide => 0,
            PtPoint::Pass => 1,
            PtPoint::Fallback => 2,
        }
    }

    pub fn index(self) -> usize {
        self.code() as usize
    }

    /// 位掩码：三点的并集必须等于 0b111 才算「类别全覆盖」。
    pub fn mask(self) -> u8 {
        1u8 << self.code()
    }

    pub fn label(self) -> &'static str {
        match self {
            PtPoint::Decide => "decide",
            PtPoint::Pass => "pass",
            PtPoint::Fallback => "fallback",
        }
    }

    /// 终结点（判定之外的两种）。
    pub fn is_terminal(self) -> bool {
        !matches!(self, PtPoint::Decide)
    }

    pub fn from_code(c: u8) -> Result<PtPoint, PtTraceError> {
        match c {
            0 => Ok(PtPoint::Decide),
            1 => Ok(PtPoint::Pass),
            2 => Ok(PtPoint::Fallback),
            _ => Err(PtTraceError::UnknownJob),
        }
    }
}

/// 每作业轨迹状态机：未开 → 已判定 → 透传/回退 → 收口。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JobPhase {
    Unopened,
    Decided,
    Passed,
    FellBack,
    Closed,
}

impl JobPhase {
    pub fn code(self) -> u8 {
        match self {
            JobPhase::Unopened => 0,
            JobPhase::Decided => 1,
            JobPhase::Passed => 2,
            JobPhase::FellBack => 3,
            JobPhase::Closed => 4,
        }
    }

    /// 收口只允许从两个终结态进入。
    pub fn is_terminal(self) -> bool {
        matches!(self, JobPhase::Passed | JobPhase::FellBack)
    }

    pub fn label(self) -> &'static str {
        match self {
            JobPhase::Unopened => "unopened",
            JobPhase::Decided => "decided",
            JobPhase::Passed => "passed",
            JobPhase::FellBack => "fell-back",
            JobPhase::Closed => "closed",
        }
    }
}

/// 单条埋点。`seq` 是全局落账序（跨作业唯一），`tick` 是调用方时钟。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span {
    pub job: u16,
    pub point: PtPoint,
    pub seq: u32,
    pub tick: u32,
}

/// 每作业轨迹。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JobTrace {
    pub job: u16,
    pub phase: JobPhase,
    pub events: u8,
    pub mask: u8,
    pub open_seq: u32,
    pub close_seq: u32,
}

impl JobTrace {
    fn open(job: u16) -> JobTrace {
        JobTrace {
            job,
            phase: JobPhase::Unopened,
            events: 0,
            mask: 0,
            open_seq: 0,
            close_seq: 0,
        }
    }

    /// 是否三点都落过（作业内部，不含其它作业）。
    pub fn full_mask(&self) -> bool {
        self.mask == 0b111
    }
}

/// 定容环形埋点缓冲。
///
/// 为什么环形而不是无限追加：埋点在热路径上，容量必须可预算；溢出时丢**最旧**
/// 的一条并显式记 `evicted`，守恒式 `written == len + evicted` 恒成立——
/// 「丢了但不说」在这里是不允许的。
pub struct TraceRing {
    buf: Vec<Span>,
    cap: usize,
    head: usize,
    written: u32,
    evicted: u32,
}

impl TraceRing {
    pub fn new(cap: usize) -> TraceRing {
        TraceRing {
            buf: Vec::with_capacity(cap),
            cap,
            head: 0,
            written: 0,
            evicted: 0,
        }
    }

    pub fn cap(&self) -> usize {
        self.cap
    }

    pub fn len(&self) -> usize {
        self.buf.len()
    }

    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    pub fn written(&self) -> u32 {
        self.written
    }

    pub fn evicted(&self) -> u32 {
        self.evicted
    }

    /// 写入一条；满则覆盖最旧。
    pub fn push(&mut self, s: Span) {
        if self.cap == 0 {
            self.written = self.written.saturating_add(1);
            self.evicted = self.evicted.saturating_add(1);
            return;
        }
        if self.buf.len() < self.cap {
            self.buf.push(s);
        } else {
            if let Some(slot) = self.buf.get_mut(self.head) {
                *slot = s;
            }
            self.evicted = self.evicted.saturating_add(1);
        }
        self.head = match self.cap {
            0 => 0,
            n => (self.head + 1) % n,
        };
        self.written = self.written.saturating_add(1);
    }

    /// 逻辑序读取（0 = 最旧保留项）。
    pub fn get(&self, i: usize) -> Option<&Span> {
        if self.cap == 0 || i >= self.buf.len() {
            return None;
        }
        let start = if self.buf.len() < self.cap { 0 } else { self.head };
        let idx = (start + i) % self.cap;
        self.buf.get(idx)
    }

    /// 最新一条（写入序最后）。
    pub fn latest(&self) -> Option<&Span> {
        if self.buf.is_empty() {
            return None;
        }
        self.get(self.buf.len() - 1)
    }

    /// 序号严格单调（保留窗口内）。
    pub fn seq_monotonic(&self) -> bool {
        let mut last = 0u32;
        let mut i = 0usize;
        while i < self.buf.len() {
            match self.get(i) {
                Some(s) => {
                    if s.seq <= last {
                        return false;
                    }
                    last = s.seq;
                }
                None => return false,
            }
            i += 1;
        }
        true
    }

    /// 时钟非回退（保留窗口内）。
    pub fn tick_monotonic(&self) -> bool {
        let mut last = 0u32;
        let mut first = true;
        let mut i = 0usize;
        while i < self.buf.len() {
            match self.get(i) {
                Some(s) => {
                    if !first && s.tick < last {
                        return false;
                    }
                    last = s.tick;
                    first = false;
                }
                None => return false,
            }
            i += 1;
        }
        true
    }

    /// 守恒式：写入总数 = 保留数 + 覆盖数。
    pub fn conserved(&self) -> bool {
        self.written == self.buf.len() as u32 + self.evicted
    }
}

/// 埋点账本：状态机闸 + 单调闸 + 覆盖审计。
pub struct TraceBook {
    jobs: Vec<JobTrace>,
    job_cap: usize,
    ring: TraceRing,
    seq: u32,
    last_tick: u32,
    counts: [u32; 3],
    rejected: u32,
}

impl TraceBook {
    /// `job_cap` 约束同时活跃的作业数，`ring_cap` 约束埋点保留窗口。
    pub fn new(job_cap: usize, ring_cap: usize) -> TraceBook {
        TraceBook {
            jobs: Vec::new(),
            job_cap,
            ring: TraceRing::new(ring_cap),
            seq: 0,
            last_tick: 0,
            counts: [0u32; 3],
            rejected: 0,
        }
    }

    pub fn jobs(&self) -> &[JobTrace] {
        &self.jobs
    }

    pub fn ring(&self) -> &TraceRing {
        &self.ring
    }

    pub fn rejected(&self) -> u32 {
        self.rejected
    }

    pub fn point_count(&self, p: PtPoint) -> u32 {
        match self.counts.get(p.index()) {
            Some(&c) => c,
            None => 0,
        }
    }

    /// 三点类别并集（0b111 = 三类都出现过）。
    pub fn kind_mask(&self) -> u8 {
        let mut m = 0u8;
        let mut i = 0usize;
        while i < self.jobs.len() {
            if let Some(j) = self.jobs.get(i) {
                m |= j.mask;
            }
            i += 1;
        }
        m
    }

    fn slot_of(&self, job: u16) -> Option<usize> {
        let mut i = 0usize;
        while i < self.jobs.len() {
            if let Some(j) = self.jobs.get(i) {
                if j.job == job {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    pub fn job(&self, job: u16) -> Option<&JobTrace> {
        match self.slot_of(job) {
            Some(s) => self.jobs.get(s),
            None => None,
        }
    }

    /// 落一条埋点。三道闸依次是：时钟、容量、状态机。
    pub fn record(&mut self, job: u16, point: PtPoint, tick: u32) -> Result<u32, PtTraceError> {
        if job == 0 {
            self.rejected += 1;
            return Err(PtTraceError::UnknownJob);
        }
        if tick < self.last_tick {
            self.rejected += 1;
            return Err(PtTraceError::Tick);
        }
        if self.ring.cap() == 0 {
            self.rejected += 1;
            return Err(PtTraceError::Capacity);
        }
        let slot = match self.slot_of(job) {
            Some(s) => s,
            None => {
                // 孤儿终结（无判定的透传/回退）不建槽：拒绝不能留下幽灵作业，
                // 否则「覆盖率」会被一个从没判定过的空壳作业污染。
                if point != PtPoint::Decide {
                    self.rejected += 1;
                    return Err(PtTraceError::Order);
                }
                if self.jobs.len() >= self.job_cap {
                    self.rejected += 1;
                    return Err(PtTraceError::Capacity);
                }
                self.jobs.push(JobTrace::open(job));
                match self.jobs.len().checked_sub(1) {
                    Some(v) => v,
                    None => {
                        self.rejected += 1;
                        return Err(PtTraceError::Capacity);
                    }
                }
            }
        };
        let phase = match self.jobs.get(slot) {
            Some(j) => j.phase,
            None => {
                self.rejected += 1;
                return Err(PtTraceError::UnknownJob);
            }
        };
        let next = match point {
            PtPoint::Decide => match phase {
                JobPhase::Unopened => JobPhase::Decided,
                JobPhase::Decided => {
                    self.rejected += 1;
                    return Err(PtTraceError::Dup);
                }
                _ => {
                    self.rejected += 1;
                    return Err(PtTraceError::Order);
                }
            },
            PtPoint::Pass => match phase {
                JobPhase::Decided => JobPhase::Passed,
                JobPhase::Passed => {
                    self.rejected += 1;
                    return Err(PtTraceError::Dup);
                }
                JobPhase::Unopened => {
                    // 没判定就报透传：埋点本身在说谎。
                    self.rejected += 1;
                    return Err(PtTraceError::Order);
                }
                _ => {
                    self.rejected += 1;
                    return Err(PtTraceError::Order);
                }
            },
            PtPoint::Fallback => match phase {
                JobPhase::Decided => JobPhase::FellBack,
                JobPhase::FellBack => {
                    self.rejected += 1;
                    return Err(PtTraceError::Dup);
                }
                JobPhase::Unopened => {
                    self.rejected += 1;
                    return Err(PtTraceError::Order);
                }
                _ => {
                    self.rejected += 1;
                    return Err(PtTraceError::Order);
                }
            },
        };
        self.seq = self.seq.saturating_add(1);
        let span = Span {
            job,
            point,
            seq: self.seq,
            tick,
        };
        self.ring.push(span);
        if let Some(j) = self.jobs.get_mut(slot) {
            j.phase = next;
            j.events = j.events.saturating_add(1);
            j.mask |= point.mask();
            if point == PtPoint::Decide {
                j.open_seq = self.seq;
            }
        }
        if let Some(c) = self.counts.get_mut(point.index()) {
            *c = c.saturating_add(1);
        }
        self.last_tick = tick;
        Ok(self.seq)
    }

    /// 收口：只有透传/回退之后才允许；收口后不允许再落点。
    pub fn close(&mut self, job: u16) -> Result<u32, PtTraceError> {
        let slot = match self.slot_of(job) {
            Some(s) => s,
            None => {
                self.rejected += 1;
                return Err(PtTraceError::UnknownJob);
            }
        };
        let phase = match self.jobs.get(slot) {
            Some(j) => j.phase,
            None => {
                self.rejected += 1;
                return Err(PtTraceError::UnknownJob);
            }
        };
        if phase == JobPhase::Closed {
            self.rejected += 1;
            return Err(PtTraceError::Dup);
        }
        if !phase.is_terminal() {
            self.rejected += 1;
            return Err(PtTraceError::Order);
        }
        self.seq = self.seq.saturating_add(1);
        if let Some(j) = self.jobs.get_mut(slot) {
            j.phase = JobPhase::Closed;
            j.close_seq = self.seq;
            j.events = j.events.saturating_add(1);
        }
        Ok(self.seq)
    }

    /// 回放外部埋点流（落盘回放 / 跨代拼接）。
    ///
    /// 为什么需要：跨进程拼接最容易出现序号跳变或重复；若直接 push 进环，
    /// 账本会静默接受一段不属于自己的历史。这里逐条校验**接续性**
    /// （必须恰好等于本账 seq + 1），任何跳号即 `SeqDrift`，且不写环。
    pub fn replay(&mut self, spans: &[Span]) -> Result<usize, PtTraceError> {
        let mut i = 0usize;
        while i < spans.len() {
            let sp = match spans.get(i) {
                Some(v) => *v,
                None => break,
            };
            if sp.seq != self.seq.saturating_add(1) {
                self.rejected += 1;
                return Err(PtTraceError::SeqDrift);
            }
            self.seq = sp.seq;
            self.ring.push(sp);
            if let Some(c) = self.counts.get_mut(sp.point.index()) {
                *c = c.saturating_add(1);
            }
            if sp.tick > self.last_tick {
                self.last_tick = sp.tick;
            }
            i += 1;
        }
        Ok(i)
    }

    /// 覆盖统计：`(已收口作业数, 作业总数)`——J1 的 100% 就是两数相等。
    pub fn coverage(&self) -> (usize, usize) {
        let mut closed = 0usize;
        let mut i = 0usize;
        while i < self.jobs.len() {
            if let Some(j) = self.jobs.get(i) {
                if j.phase == JobPhase::Closed {
                    closed += 1;
                }
            }
            i += 1;
        }
        (closed, self.jobs.len())
    }

    /// 覆盖千分比（无作业时返回 0，不伪造 1000）。
    pub fn coverage_permille(&self) -> u16 {
        let (closed, total) = self.coverage();
        if total == 0 {
            return 0;
        }
        match (closed.saturating_mul(1000)) / total {
            v if v > 1000 => 1000,
            v => v as u16,
        }
    }

    /// 覆盖审计：空账 → Empty；有未收口 → Coverage；全收口返回作业数。
    pub fn audit(&self) -> Result<usize, PtTraceError> {
        if self.jobs.is_empty() {
            return Err(PtTraceError::Empty);
        }
        let mut i = 0usize;
        while i < self.jobs.len() {
            match self.jobs.get(i) {
                Some(j) => {
                    if j.phase != JobPhase::Closed {
                        return Err(PtTraceError::Coverage);
                    }
                }
                None => return Err(PtTraceError::Coverage),
            }
            i += 1;
        }
        Ok(i)
    }

    /// 账本自带诊断：序号漂移（被外部拼接/回放破坏）在此显形。
    pub fn self_check(&self) -> Result<(), PtTraceError> {
        if !self.ring.conserved() {
            return Err(PtTraceError::SeqDrift);
        }
        if !self.ring.seq_monotonic() || !self.ring.tick_monotonic() {
            return Err(PtTraceError::SeqDrift);
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 注入核：8 类坏埋点序列
// ---------------------------------------------------------------------------

/// 返回 `(命中数, 总类数)`；每类断言具体诊断码。
pub fn injection_core() -> (usize, usize) {
    let total = 9usize;
    let mut caught = 0usize;

    // 1) 孤儿回退：未判定就报回退
    let mut b = TraceBook::new(4, 8);
    if b.record(1, PtPoint::Fallback, 1).err() == Some(PtTraceError::Order) {
        caught += 1;
    }

    // 2) 重复判定
    let mut b = TraceBook::new(4, 8);
    let _ = b.record(2, PtPoint::Decide, 1);
    if b.record(2, PtPoint::Decide, 2).err() == Some(PtTraceError::Dup) {
        caught += 1;
    }

    // 3) 未终结即收口
    let mut b = TraceBook::new(4, 8);
    let _ = b.record(3, PtPoint::Decide, 1);
    if b.close(3).err() == Some(PtTraceError::Order) {
        caught += 1;
    }

    // 4) 收口未知作业
    let mut b = TraceBook::new(4, 8);
    if b.close(99).err() == Some(PtTraceError::UnknownJob) {
        caught += 1;
    }

    // 5) 时钟回退
    let mut b = TraceBook::new(4, 8);
    let _ = b.record(5, PtPoint::Decide, 10);
    if b.record(5, PtPoint::Pass, 9).err() == Some(PtTraceError::Tick) {
        caught += 1;
    }

    // 6) 作业表容量满（job_cap = 1）
    let mut b = TraceBook::new(1, 8);
    let _ = b.record(6, PtPoint::Decide, 1);
    if b.record(7, PtPoint::Decide, 2).err() == Some(PtTraceError::Capacity) {
        caught += 1;
    }

    // 7) 覆盖审计：两个作业只收口一个
    let mut b = TraceBook::new(4, 8);
    let _ = b.record(8, PtPoint::Decide, 1);
    let _ = b.record(8, PtPoint::Pass, 2);
    let _ = b.close(8);
    let _ = b.record(9, PtPoint::Decide, 3);
    if b.audit().err() == Some(PtTraceError::Coverage) {
        caught += 1;
    }

    // 8) 空账审计
    let b = TraceBook::new(4, 8);
    if b.audit().err() == Some(PtTraceError::Empty) {
        caught += 1;
    }

    // 9) 回放跳号（期望接续 seq=1，实际给 2）
    let mut b = TraceBook::new(4, 8);
    let sp = Span {
        job: 10,
        point: PtPoint::Decide,
        seq: 2,
        tick: 1,
    };
    if b.replay(&[sp]).err() == Some(PtTraceError::SeqDrift) {
        caught += 1;
    }

    (caught, total)
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// 造 12 个作业的闭环：前 9 个走透传，后 3 个走回退。
fn closed_batch() -> TraceBook {
    let mut b = TraceBook::new(16, 64);
    let mut tick = 1u32;
    let mut job = 1u16;
    while job <= 12 {
        let _ = b.record(job, PtPoint::Decide, tick);
        tick = tick.saturating_add(1);
        if job <= 9 {
            let _ = b.record(job, PtPoint::Pass, tick);
        } else {
            let _ = b.record(job, PtPoint::Fallback, tick);
        }
        tick = tick.saturating_add(1);
        let _ = b.close(job);
        job += 1;
    }
    b
}

pub fn run_unx50110_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50110");

    // J1-1 覆盖 100%：12 个作业全部判定 + 终结 + 收口
    let b = closed_batch();
    let (closed, total) = b.coverage();
    let ok = closed == 12 && total == 12 && b.coverage_permille() == 1000 && b.audit() == Ok(12);
    let det = format!("{}/{} {}permille", closed, total, b.coverage_permille());
    s.add_dyn("50110-J1-覆盖百分百", ok, &det);

    // J1-2 三点全出现：类别并集 0b111，判定 12 / 透传 9 / 回退 3
    let ok = b.kind_mask() == 0b111
        && b.point_count(PtPoint::Decide) == 12
        && b.point_count(PtPoint::Pass) == 9
        && b.point_count(PtPoint::Fallback) == 3;
    let det = format!(
        "mask={} d{} p{} f{}",
        b.kind_mask(),
        b.point_count(PtPoint::Decide),
        b.point_count(PtPoint::Pass),
        b.point_count(PtPoint::Fallback)
    );
    s.add_dyn("50110-J1-三点齐备", ok, &det);

    // J1-3 每作业恰好 3 个事件（2 埋点 + 1 收口），且判定先于终结
    let mut ok = true;
    let mut i = 0usize;
    while i < b.jobs().len() {
        if let Some(j) = b.jobs().get(i) {
            if j.events != 3 || j.open_seq == 0 || j.close_seq <= j.open_seq {
                ok = false;
            }
            if j.phase != JobPhase::Closed {
                ok = false;
            }
        }
        i += 1;
    }
    s.add("50110-J1-事件计数", ok, if ok { "" } else { "事件数/序号不对" });

    // J1-4 序号严格单调 + 时钟非回退 + 账本自检通过
    let ok = b.ring().seq_monotonic() && b.ring().tick_monotonic() && b.self_check() == Ok(());
    s.add("50110-J1-单调闸", ok, if ok { "" } else { "序/时钟回退" });

    // J1-5 环守恒：cap=8 写 20 条 → written=20、len=8、evicted=12
    let mut ring = TraceRing::new(8);
    let mut k = 1u32;
    while k <= 20 {
        ring.push(Span {
            job: 1,
            point: PtPoint::Decide,
            seq: k,
            tick: k,
        });
        k += 1;
    }
    let ok = ring.written() == 20 && ring.len() == 8 && ring.evicted() == 12 && ring.conserved();
    let det = format!("w={} l={} e={}", ring.written(), ring.len(), ring.evicted());
    s.add_dyn("50110-J1-环守恒", ok, &det);

    // J1-6 环逻辑序：最新项是最后写入的那条，最旧项是第 13 条
    let ok = match (ring.latest(), ring.get(0)) {
        (Some(last), Some(first)) => last.seq == 20 && first.seq == 13,
        _ => false,
    };
    let det = format!("{:?}", ring.latest().map(|s| s.seq));
    s.add_dyn("50110-J1-环逻辑序", ok, &det);

    // J1-7 状态机：孤儿透传 / 判定晚于终结 / 重复收口
    let mut t = TraceBook::new(4, 16);
    let orphan = t.record(1, PtPoint::Pass, 1);
    let _ = t.record(2, PtPoint::Decide, 2);
    let _ = t.record(2, PtPoint::Pass, 3);
    let late = t.record(2, PtPoint::Decide, 4);
    let _ = t.close(2);
    let twice = t.close(2);
    let ok = orphan.err() == Some(PtTraceError::Order)
        && late.err() == Some(PtTraceError::Order)
        && twice.err() == Some(PtTraceError::Dup);
    s.add("50110-J1-状态机闸", ok, if ok { "" } else { "非法转移被放行" });

    // J1-8 回退与透传互斥：已透传再报回退必拒
    let mut t = TraceBook::new(4, 16);
    let _ = t.record(3, PtPoint::Decide, 1);
    let _ = t.record(3, PtPoint::Pass, 2);
    let both = t.record(3, PtPoint::Fallback, 3);
    let ok = both.err() == Some(PtTraceError::Order);
    s.add("50110-J1-终结互斥", ok, if ok { "" } else { "双终结被接受" });

    // J1-9 时钟回退必拒（tick 小于上一条）
    let mut t = TraceBook::new(4, 16);
    let _ = t.record(4, PtPoint::Decide, 50);
    let regress = t.record(4, PtPoint::Pass, 49);
    let same = t.record(4, PtPoint::Pass, 50);
    let ok = regress.err() == Some(PtTraceError::Tick) && same.is_ok();
    s.add("50110-J1-时钟闸", ok, if ok { "" } else { "时钟回退被接受" });

    // J1-10 作业表容量：cap=2 时第三个作业 Capacity，已有作业不受影响
    let mut t = TraceBook::new(2, 16);
    let _ = t.record(1, PtPoint::Decide, 1);
    let _ = t.record(2, PtPoint::Decide, 2);
    let third = t.record(3, PtPoint::Decide, 3);
    let ok = third.err() == Some(PtTraceError::Capacity) && t.jobs().len() == 2;
    s.add("50110-J1-作业容量", ok, if ok { "" } else { "容量闸失效" });

    // J1-11 零容量环：任何埋点都 Capacity（不静默丢）
    let mut t = TraceBook::new(4, 0);
    let r = t.record(1, PtPoint::Decide, 1);
    let ok = r.err() == Some(PtTraceError::Capacity) && t.ring().written() == 0;
    s.add("50110-J1-零容量显式", ok, if ok { "" } else { "零容量被当成功" });

    // J1-12 覆盖审计：空账 Empty、未收口 Coverage、全收口 Ok(n)
    let empty = TraceBook::new(4, 8).audit();
    let mut t = TraceBook::new(4, 8);
    let _ = t.record(1, PtPoint::Decide, 1);
    let partial = t.audit();
    let ok = empty.err() == Some(PtTraceError::Empty) && partial.err() == Some(PtTraceError::Coverage);
    s.add("50110-J1-审计三态", ok, if ok { "" } else { "审计状态错" });

    // J1-13 拒绝可数：所有被拒序列都落在 rejected 计数里
    let mut t = TraceBook::new(4, 8);
    let _ = t.record(0, PtPoint::Decide, 1);
    let _ = t.record(1, PtPoint::Fallback, 2);
    let _ = t.close(77);
    let ok = t.rejected() == 3 && t.jobs().is_empty();
    let det = format!("rej={}", t.rejected());
    s.add_dyn("50110-J1-拒绝可数", ok, &det);

    // J1-14 确定性：同序列跑两次，埋点序号/判定结果逐条一致
    let a = closed_batch();
    let c = closed_batch();
    let mut ok = a.ring().len() == c.ring().len();
    let mut i = 0usize;
    while i < a.ring().len() && ok {
        match (a.ring().get(i), c.ring().get(i)) {
            (Some(x), Some(y)) => {
                if x != y {
                    ok = false;
                }
            }
            _ => ok = false,
        }
        i += 1;
    }
    ok = ok && a.ring().len() == 24;
    let det = format!("spans={}", a.ring().len());
    s.add_dyn("50110-J1-重放确定", ok, &det);

    // J1-15 码值零漂移：8 个码互异、连续、可反解
    let all = [
        PtTraceError::Empty,
        PtTraceError::Dup,
        PtTraceError::Order,
        PtTraceError::UnknownJob,
        PtTraceError::Capacity,
        PtTraceError::SeqDrift,
        PtTraceError::Coverage,
        PtTraceError::Tick,
    ];
    let mut ok = true;
    let mut seen = [false; 8];
    let mut i = 0usize;
    while i < all.len() {
        let n = all[i].n() as usize;
        if n < 1 || n > 8 || seen[n - 1] {
            ok = false;
        } else {
            seen[n - 1] = true;
        }
        if PtTraceError::from_code(all[i].code()) != Some(all[i]) {
            ok = false;
        }
        i += 1;
    }
    ok = ok && PtTraceError::from_code(SEG_BASE).is_none();
    s.add("50110-J1-码值不漂", ok, if ok { "" } else { "码值漂移" });

    // J1-16 注入核：8 类坏序列全抓
    let (caught, total) = injection_core();
    let ok = caught == total;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50110-J1-注入必抓", ok, &det);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50110_checks_green() {
        let set = run_unx50110_checks();
        assert!(set.all_passed(), "UNX-F50110 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50110_ring_conserves_under_overwrite() {
        let mut r = TraceRing::new(4);
        let mut i = 1u32;
        while i <= 9 {
            r.push(Span {
                job: 1,
                point: PtPoint::Pass,
                seq: i,
                tick: i,
            });
            i += 1;
        }
        assert_eq!(r.written(), 9);
        assert_eq!(r.len(), 4);
        assert_eq!(r.evicted(), 5);
        assert!(r.conserved());
        assert_eq!(r.get(0).map(|s| s.seq), Some(6));
        assert_eq!(r.latest().map(|s| s.seq), Some(9));
    }

    #[test]
    fn unx50110_point_mask_covers_three() {
        let m = PtPoint::Decide.mask() | PtPoint::Pass.mask() | PtPoint::Fallback.mask();
        assert_eq!(m, 0b111);
        assert!(!PtPoint::Decide.is_terminal());
        assert!(PtPoint::Pass.is_terminal() && PtPoint::Fallback.is_terminal());
    }
}
