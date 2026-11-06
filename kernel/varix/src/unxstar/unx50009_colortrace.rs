//! UNX-F50009 色彩埋点（映射/意图/降级三点）（0x42d40 段）
//!
//! 锚点：UNX-F50009-J1 埋点 100% 覆盖；判据锚定 体验十三·补。
//!
//! 数据段 SEG_BASE = 0x42d40，诊断码闭集为 Base+1..Base+10。
//! 每次色彩作业必须依次留下三个埋点：映射（mapping）→ 意图（intent）→
//! 降级（fallback）。缺一点就等于现场无法归因，因此覆盖率按**作业**统计：
//! 三点的位掩码必须等于 0b111 才算覆盖，事件条数多寡不算数。
//!
//! 结构清单：
//! * `TraceStage`/`TraceEvent` 埋点三阶段与事件记录；
//! * `JobState`/`JobTrace` 作业级状态机（Started→Mapped→Intented→Traced），
//!   拒绝跳点与重复点，并单独记录时间回退；
//! * `TraceRing` 有界环形账本（覆盖最旧、票号单调、丢弃计数、容量 0 拒绝）；
//! * `CoverageReport` 覆盖率审计器（full/jobs 精确分数 + 缺失点清单）；
//! * 注入核 `injection_core()`：五类坏样本逐类断言显式拒绝。
//!
//! 错误纪律：内核路径零 panic；环形缓冲的所有取放都先算模再 `get_mut()`，
//! 容量为 0 时在取模之前就拒绝，杜绝除零。

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x42d40 独占 0x20）。
pub const SEG_BASE: u32 = 0x42d40;

pub const E_EMPTY: u16 = 1;
pub const E_CAPACITY: u16 = 2;
pub const E_ORDER: u16 = 3;
pub const E_BADCODE: u16 = 4;
pub const E_STAGEMASK: u16 = 5;
pub const E_STALE: u16 = 6;
pub const E_TIMEOUT: u16 = 7;
pub const E_UNKNOWNJOB: u16 = 8;
pub const E_DUPLICATE: u16 = 9;
pub const E_OVERFLOW: u16 = 10;

/// 埋点错误闭集。`code()` 返回 `SEG_BASE + n`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TraceError {
    Empty,
    Capacity,
    Order,
    BadCode,
    StageMask,
    Stale,
    Timeout,
    UnknownJob,
    Duplicate,
    Overflow,
}

impl TraceError {
    pub fn code(self) -> u32 {
        let n = match self {
            TraceError::Empty => E_EMPTY,
            TraceError::Capacity => E_CAPACITY,
            TraceError::Order => E_ORDER,
            TraceError::BadCode => E_BADCODE,
            TraceError::StageMask => E_STAGEMASK,
            TraceError::Stale => E_STALE,
            TraceError::Timeout => E_TIMEOUT,
            TraceError::UnknownJob => E_UNKNOWNJOB,
            TraceError::Duplicate => E_DUPLICATE,
            TraceError::Overflow => E_OVERFLOW,
        };
        SEG_BASE + n as u32
    }

    pub fn message(self) -> &'static str {
        match self {
            TraceError::Empty => "no trace events",
            TraceError::Capacity => "ring capacity is zero",
            TraceError::Order => "event sequence/timestamp out of order",
            TraceError::BadCode => "payload code outside 0..3",
            TraceError::StageMask => "stage skipped or missing",
            TraceError::Stale => "event older than deadline",
            TraceError::Timeout => "job exceeded tick budget",
            TraceError::UnknownJob => "job id not tracked",
            TraceError::Duplicate => "stage already recorded for this job",
            TraceError::Overflow => "ring overflow",
        }
    }
}

/// 三个埋点阶段。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TraceStage {
    Mapping,
    Intent,
    Fallback,
}

impl TraceStage {
    pub fn code(self) -> u8 {
        match self {
            TraceStage::Mapping => 0,
            TraceStage::Intent => 1,
            TraceStage::Fallback => 2,
        }
    }

    pub fn from_code(c: u8) -> Result<TraceStage, TraceError> {
        match c {
            0 => Ok(TraceStage::Mapping),
            1 => Ok(TraceStage::Intent),
            2 => Ok(TraceStage::Fallback),
            _ => Err(TraceError::BadCode),
        }
    }

    /// 位掩码位：三点并集恰为 0b111，覆盖率直接由 popcount 推出。
    pub fn bit(self) -> u8 {
        1u8 << self.code()
    }

    pub fn label(self) -> &'static str {
        match self {
            TraceStage::Mapping => "mapping",
            TraceStage::Intent => "intent",
            TraceStage::Fallback => "fallback",
        }
    }
}

/// 全部阶段位掩码（= 0b111）。
pub const FULL_MASK: u8 = 0b111;
pub const STAGE_COUNT: usize = 3;

/// 一条埋点事件。`code` 是阶段载荷：映射/意图记渲染意图码 0..3，
/// 降级记处置码（0=未降级，1=内置档，2=裁剪，3=中止）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TraceEvent {
    pub job: u16,
    pub stage: TraceStage,
    pub tick: u64,
    pub seq: u32,
    pub code: u8,
}

impl TraceEvent {
    pub const fn new(job: u16, stage: TraceStage, tick: u64, seq: u32, code: u8) -> TraceEvent {
        TraceEvent {
            job,
            stage,
            tick,
            seq,
            code,
        }
    }

    /// 载荷合法性：四档码值闭集 0..3。
    pub fn payload_ok(&self) -> bool {
        self.code < 4
    }
}

/// 作业埋点状态机。按阶段推进，跳点与重复都显式失败。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JobState {
    Started,
    Mapped,
    Intented,
    Traced,
    Failed,
}

impl JobState {
    /// 该状态下允许记录的下一阶段。只有顺序前进这一条路。
    pub fn expects(self) -> Option<TraceStage> {
        match self {
            JobState::Started => Some(TraceStage::Mapping),
            JobState::Mapped => Some(TraceStage::Intent),
            JobState::Intented => Some(TraceStage::Fallback),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            JobState::Started => "started",
            JobState::Mapped => "mapped",
            JobState::Intented => "intented",
            JobState::Traced => "traced",
            JobState::Failed => "failed",
        }
    }
}

/// 单作业埋点轨迹。mask 是覆盖率的事实来源；regress 记录时间回退次数。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct JobTrace {
    pub job: u16,
    pub state: JobState,
    pub mask: u8,
    pub ticks: [u64; 3],
    pub last_tick: u64,
    pub regress: u32,
}

impl JobTrace {
    pub fn new(job: u16) -> JobTrace {
        JobTrace {
            job,
            state: JobState::Started,
            mask: 0,
            ticks: [0; STAGE_COUNT],
            last_tick: 0,
            regress: 0,
        }
    }

    /// 记录一个阶段。成功返回推进后的状态；重复点报 Duplicate，跳点报 StageMask。
    /// 时间回退不拒绝（现场时钟可能回拨），但计数——静默吞掉才是问题。
    pub fn record(&mut self, stage: TraceStage, tick: u64) -> Result<JobState, TraceError> {
        let bit = stage.bit();
        if (self.mask & bit) == bit {
            return Err(TraceError::Duplicate);
        }
        match self.state.expects() {
            Some(expect) => {
                if expect != stage {
                    return Err(TraceError::StageMask);
                }
            }
            None => return Err(TraceError::StageMask),
        }
        if tick < self.last_tick {
            self.regress += 1;
        } else {
            self.last_tick = tick;
        }
        if let Some(cell) = self.ticks.get_mut(stage.code() as usize) {
            *cell = tick;
        }
        self.mask |= bit;
        self.state = match stage {
            TraceStage::Mapping => JobState::Mapped,
            TraceStage::Intent => JobState::Intented,
            TraceStage::Fallback => JobState::Traced,
        };
        Ok(self.state)
    }

    /// 覆盖点位数 0..3。
    pub fn points(&self) -> u32 {
        self.mask.count_ones()
    }

    pub fn is_full(&self) -> bool {
        self.mask == FULL_MASK
    }

    /// 覆盖率 0..1，按作业（而不是事件）计。
    pub fn coverage(&self) -> f32 {
        self.points() as f32 / STAGE_COUNT as f32
    }
}

/// 有界环形埋点账本。满了覆盖最旧一条并累加 dropped；
/// 票号必须严格递增，回退票号直接拒绝，保证回放顺序确定。
#[derive(Clone, Debug)]
pub struct TraceRing {
    buf: Vec<Option<TraceEvent>>,
    head: usize,
    len: usize,
    cap: usize,
    last_seq: u32,
    pushed: u64,
    dropped: u64,
    rejected: u64,
}

impl TraceRing {
    pub fn new(cap: usize) -> TraceRing {
        let mut buf = Vec::with_capacity(cap);
        let mut i = 0usize;
        while i < cap {
            buf.push(None);
            i += 1;
        }
        TraceRing {
            buf,
            head: 0,
            len: 0,
            cap,
            last_seq: 0,
            pushed: 0,
            dropped: 0,
            rejected: 0,
        }
    }

    pub fn capacity(&self) -> usize {
        self.cap
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn dropped(&self) -> u64 {
        self.dropped
    }

    pub fn pushed(&self) -> u64 {
        self.pushed
    }

    pub fn rejected(&self) -> u64 {
        self.rejected
    }

    pub fn clear(&mut self) {
        let mut i = 0usize;
        while i < self.buf.len() {
            if let Some(slot) = self.buf.get_mut(i) {
                *slot = None;
            }
            i += 1;
        }
        self.head = 0;
        self.len = 0;
        self.last_seq = 0;
        self.pushed = 0;
        self.dropped = 0;
        self.rejected = 0;
    }

    /// 入环。容量 0 在取模前就拒绝（否则 `% 0` 会 panic）。
    pub fn push(&mut self, ev: TraceEvent) -> Result<(), TraceError> {
        if self.cap == 0 {
            self.rejected += 1;
            return Err(TraceError::Capacity);
        }
        if !ev.payload_ok() {
            self.rejected += 1;
            return Err(TraceError::BadCode);
        }
        if self.pushed > 0 && ev.seq <= self.last_seq {
            self.rejected += 1;
            return Err(TraceError::Order);
        }
        if self.len == self.cap {
            let idx = self.head;
            if let Some(slot) = self.buf.get_mut(idx) {
                *slot = Some(ev);
            }
            self.head = (self.head + 1) % self.cap;
            self.dropped += 1;
        } else {
            let idx = (self.head + self.len) % self.cap;
            if let Some(slot) = self.buf.get_mut(idx) {
                *slot = Some(ev);
            }
            self.len += 1;
        }
        self.last_seq = ev.seq;
        self.pushed += 1;
        Ok(())
    }

    /// 按入环先后（FIFO）导出快照。被覆盖的旧事件不会出现在快照里。
    pub fn snapshot(&self) -> Vec<TraceEvent> {
        let mut out = Vec::with_capacity(self.len);
        let mut i = 0usize;
        while i < self.len {
            if self.cap == 0 {
                break;
            }
            let idx = (self.head + i) % self.cap;
            if let Some(Some(ev)) = self.buf.get(idx) {
                out.push(*ev);
            }
            i += 1;
        }
        out
    }

    /// 按作业抽取事件（保持 FIFO），审计失败时也能看单作业轨迹。
    pub fn events_for(&self, job: u16) -> Vec<TraceEvent> {
        let all = self.snapshot();
        let mut out = Vec::new();
        let mut i = 0usize;
        while i < all.len() {
            if let Some(ev) = all.get(i) {
                if ev.job == job {
                    out.push(*ev);
                }
            }
            i += 1;
        }
        out
    }
}

/// 覆盖率审计报告。`coverage()` 是 full/jobs 的精确分数，不用近似值。
#[derive(Clone, Debug, PartialEq)]
pub struct CoverageReport {
    pub jobs: usize,
    pub full: usize,
    pub events: usize,
    pub missing: Vec<(u16, u8)>,
    pub duplicates: usize,
    pub inversions: usize,
    pub stale: usize,
    pub fallback_applied: usize,
    pub ordered: usize,
}

impl CoverageReport {
    /// 覆盖率 = 三点齐全的作业数 / 作业总数。jobs 为 0 只可能来自空输入（已拒绝）。
    pub fn coverage(&self) -> f32 {
        if self.jobs == 0 {
            return 0.0;
        }
        self.full as f32 / self.jobs as f32
    }

    pub fn is_full(&self) -> bool {
        self.jobs > 0 && self.full == self.jobs && self.missing.is_empty()
    }

    pub fn missing_count(&self) -> usize {
        self.missing.len()
    }

    pub fn summary(&self) -> String {
        format!(
            "色彩埋点 jobs={} events={} coverage={:.3} missing={} dup={} inv={} stale={} fallback={}",
            self.jobs,
            self.events,
            self.coverage(),
            self.missing.len(),
            self.duplicates,
            self.inversions,
            self.stale,
            self.fallback_applied
        )
    }
}

/// 浮点绝对值（no_std 下 `f32::abs` 不可用，走 libm 同义实现）。
fn fabs(v: f32) -> f32 {
    if v < 0.0 {
        -v
    } else {
        v
    }
}

fn find_job(traces: &[JobTrace], job: u16) -> Option<usize> {
    let mut i = 0usize;
    while i < traces.len() {
        if let Some(t) = traces.get(i) {
            if t.job == job {
                return Some(i);
            }
        }
        i += 1;
    }
    None
}

/// 审计一批事件。空输入显式报 Empty；其余一律返回完整报告（不吞掉坏点）。
pub fn audit(events: &[TraceEvent], deadline: u64) -> Result<CoverageReport, TraceError> {
    if events.is_empty() {
        return Err(TraceError::Empty);
    }
    let mut traces: Vec<JobTrace> = Vec::new();
    let mut bad_order: Vec<bool> = Vec::new();
    let mut duplicates = 0usize;
    let mut inversions = 0usize;
    let mut stale = 0usize;
    let mut fallback_applied = 0usize;

    let mut i = 0usize;
    while i < events.len() {
        let ev = match events.get(i) {
            Some(e) => *e,
            None => break,
        };
        if ev.tick > deadline {
            stale += 1;
        }
        if ev.stage == TraceStage::Fallback && ev.code != 0 {
            fallback_applied += 1;
        }
        let idx = match find_job(&traces, ev.job) {
            Some(idx) => idx,
            None => {
                traces.push(JobTrace::new(ev.job));
                bad_order.push(false);
                traces.len() - 1
            }
        };
        let mut dup = false;
        let mut ord = false;
        if let Some(t) = traces.get_mut(idx) {
            match t.record(ev.stage, ev.tick) {
                Ok(_) => {}
                Err(TraceError::Duplicate) => dup = true,
                Err(_) => ord = true,
            }
        }
        if dup {
            duplicates += 1;
        }
        if ord {
            inversions += 1;
            if let Some(f) = bad_order.get_mut(idx) {
                *f = true;
            }
        }
        i += 1;
    }

    let mut missing: Vec<(u16, u8)> = Vec::new();
    let mut full = 0usize;
    let mut ordered = 0usize;
    let mut j = 0usize;
    while j < traces.len() {
        if let Some(t) = traces.get(j) {
            inversions += t.regress as usize;
            if t.is_full() {
                full += 1;
            } else {
                let mut c = 0u8;
                while (c as usize) < STAGE_COUNT {
                    if (t.mask & (1u8 << c)) == 0 {
                        missing.push((t.job, c));
                    }
                    c += 1;
                }
            }
        }
        if bad_order.get(j) == Some(&false) {
            ordered += 1;
        }
        j += 1;
    }

    Ok(CoverageReport {
        jobs: traces.len(),
        full,
        events: events.len(),
        missing,
        duplicates,
        inversions,
        stale,
        fallback_applied,
        ordered,
    })
}

// ---------------------------------------------------------------------------
// 注入核
// ---------------------------------------------------------------------------

fn stage_of(c: u8) -> TraceStage {
    match TraceStage::from_code(c) {
        Ok(s) => s,
        Err(_) => TraceStage::Mapping,
    }
}

/// 五类坏样本 → 命中数。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 5usize;

    // ① 空批次审计
    if audit(&[], 100).err() == Some(TraceError::Empty) {
        caught += 1;
    }

    // ② 跳点：作业只有意图点
    let mut t = JobTrace::new(1);
    if t.record(TraceStage::Intent, 5).err() == Some(TraceError::StageMask) {
        caught += 1;
    }

    // ③ 重复点
    let mut t = JobTrace::new(2);
    let ok = t.record(TraceStage::Mapping, 1).is_ok()
        && t.record(TraceStage::Mapping, 2).err() == Some(TraceError::Duplicate);
    if ok {
        caught += 1;
    }

    // ④ 票号回退
    let mut ring = TraceRing::new(8);
    let ok = ring.push(TraceEvent::new(1, TraceStage::Mapping, 1, 5, 0)).is_ok()
        && ring.push(TraceEvent::new(1, TraceStage::Intent, 2, 4, 0)).err()
            == Some(TraceError::Order)
        && ring.len() == 1;
    if ok {
        caught += 1;
    }

    // ⑤ 载荷码越界
    let ok = ring
        .push(TraceEvent::new(1, TraceStage::Intent, 3, 6, 9))
        .err()
        == Some(TraceError::BadCode);
    if ok {
        caught += 1;
    }

    (caught, total)
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn run_unx50009_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50009");

    // J1-1 阶段码闭集与位掩码并集
    let mut mask = 0u8;
    let mut codes = [false; 3];
    let mut k = 0u8;
    while k < 3 {
        if let Ok(st) = TraceStage::from_code(k) {
            mask |= st.bit();
            codes[k as usize] = true;
        }
        k += 1;
    }
    let ok = mask == FULL_MASK && codes.iter().all(|v| *v) && TraceStage::from_code(3).is_err();
    let det = format!("mask={:03b}", mask);
    s.add_dyn("50009-J1-阶段闭集", ok, &det);

    // J1-2 状态机推进：三步到位，覆盖 1.0
    let mut t = JobTrace::new(1);
    let mut ok = t.record(TraceStage::Mapping, 10) == Ok(JobState::Mapped);
    ok &= t.record(TraceStage::Intent, 20) == Ok(JobState::Intented);
    ok &= t.record(TraceStage::Fallback, 30) == Ok(JobState::Traced);
    ok &= t.is_full() && t.points() == 3 && fabs(t.coverage() - 1.0) < 1e-6;
    let det = format!("state={} mask={:03b}", t.state.label(), t.mask);
    s.add_dyn("50009-J1-状态机", ok, &det);

    // J1-3 跳点必拒、终态后不可再记
    let mut t2 = JobTrace::new(2);
    let jump = t2.record(TraceStage::Fallback, 10).err();
    let mut t3 = JobTrace::new(3);
    let mut done = t3.record(TraceStage::Mapping, 1).is_ok();
    done &= t3.record(TraceStage::Intent, 2).is_ok();
    done &= t3.record(TraceStage::Fallback, 3).is_ok();
    let after = t3.record(TraceStage::Mapping, 4).err();
    let ok = jump == Some(TraceError::StageMask)
        && done
        && after == Some(TraceError::Duplicate);
    let det = format!("jump={:?} after={:?}", jump, after);
    s.add_dyn("50009-J1-跳点必拒", ok, &det);

    // J1-4 100% 覆盖：64 作业 × 3 点
    let mut ring = TraceRing::new(256);
    let jobs = 64u16;
    let mut j = 0u16;
    let mut seq = 1u32;
    while j < jobs {
        let base = 1000u64 * j as u64;
        let mut c = 0u8;
        while c < 3 {
            let ev = TraceEvent::new(j, stage_of(c), base + c as u64 + 1, seq, c);
            if ring.push(ev).is_err() {
                break;
            }
            seq += 1;
            c += 1;
        }
        j += 1;
    }
    let snap = ring.snapshot();
    let report = match audit(&snap, 1_000_000) {
        Ok(r) => r,
        Err(_) => return s,
    };
    let ok = report.jobs == jobs as usize
        && report.full == jobs as usize
        && report.events == jobs as usize * 3
        && report.missing.is_empty()
        && fabs(report.coverage() - 1.0) < 1e-6
        && report.duplicates == 0
        && report.inversions == 0
        && report.stale == 0;
    let det = format!(
        "jobs={} full={} cov={:.3} missing={}",
        report.jobs, report.full, report.coverage(), report.missing.len()
    );
    s.add_dyn("50009-J1-全覆盖", ok, &det);

    // J1-5 缺一点：摘掉 job 7 的降级点
    let mut holed: Vec<TraceEvent> = Vec::with_capacity(snap.len());
    let mut i = 0usize;
    while i < snap.len() {
        if let Some(ev) = snap.get(i) {
            if !(ev.job == 7 && ev.stage == TraceStage::Fallback) {
                holed.push(*ev);
            }
        }
        i += 1;
    }
    let r2 = audit(&holed, 1_000_000);
    let ok = match r2 {
        Ok(r) => {
            r.full == jobs as usize - 1
                && r.missing.contains(&(7u16, 2u8))
                && r.missing_count() == 1
                && fabs(r.coverage() - 63.0 / 64.0) < 1e-6
        }
        Err(_) => false,
    };
    let det = format!("holed_len={}", holed.len());
    s.add_dyn("50009-J1-缺点可定位", ok, &det);

    // J1-6 重复点计数
    let mut dup_events = snap.clone();
    dup_events.push(TraceEvent::new(3, TraceStage::Intent, 5, 99999, 1));
    let r3 = audit(&dup_events, 1_000_000);
    let ok = match r3 {
        Ok(r) => r.duplicates == 1 && r.inversions == 0 && r.full == jobs as usize,
        Err(_) => false,
    };
    s.add_dyn("50009-J1-重复点计数", ok, if ok { "" } else { "重复点未计数" });

    // J1-7 阶段乱序计数（意图先于映射）
    let disorder = [
        TraceEvent::new(9, TraceStage::Intent, 10, 1, 1),
        TraceEvent::new(9, TraceStage::Mapping, 20, 2, 0),
        TraceEvent::new(9, TraceStage::Fallback, 30, 3, 0),
    ];
    let r4 = audit(&disorder, 1_000_000);
    // 意图点先到会被拒（1 次）；随后降级点又因状态机停在 Started/Mapped 被拒（第 2 次），
    // 所以乱序作业的 inversions 至少 2、覆盖率为 1/3 且缺点清单含两个阶段。
    let (ok, inv, miss) = match r4 {
        Ok(r) => (
            r.inversions >= 2 && r.full == 0 && r.jobs == 1 && r.missing.len() == 2,
            r.inversions,
            r.missing.len(),
        ),
        Err(_) => (false, 0, 0),
    };
    let det = format!("inv={} missing={}", inv, miss);
    s.add_dyn("50009-J1-乱序可判", ok, &det);

    // J1-8 时间回退计数：阶段顺序正确但 tick 倒退
    let regress = [
        TraceEvent::new(4, TraceStage::Mapping, 50, 1, 0),
        TraceEvent::new(4, TraceStage::Intent, 10, 2, 1),
        TraceEvent::new(4, TraceStage::Fallback, 60, 3, 0),
    ];
    let r5 = audit(&regress, 1_000_000);
    let ok = match r5 {
        Ok(r) => r.inversions == 1 && r.full == 1,
        Err(_) => false,
    };
    s.add_dyn("50009-J1-时钟回退", ok, if ok { "" } else { "时间回退未计数" });

    // J1-9 环形覆盖：cap=4 push 6 → 保留最后 4 条、丢弃 2
    let mut small = TraceRing::new(4);
    let mut q = 1u32;
    let mut all_ok = true;
    while q <= 6 {
        if small
            .push(TraceEvent::new(1, TraceStage::Mapping, q as u64, q, 0))
            .is_err()
        {
            all_ok = false;
        }
        q += 1;
    }
    let kept = small.snapshot();
    let seqs: Vec<u32> = kept.iter().map(|e| e.seq).collect();
    let mut expect: Vec<u32> = Vec::new();
    expect.push(3u32);
    expect.push(4);
    expect.push(5);
    expect.push(6);
    let ok = all_ok
        && small.len() == 4
        && small.dropped() == 2
        && small.pushed() == 6
        && seqs == expect;
    let det = format!("seqs={:?} dropped={}", seqs, small.dropped());
    s.add_dyn("50009-J1-环形覆盖", ok, &det);

    // J1-10 容量 0 拒绝（不在取模前就除零）
    let mut zero = TraceRing::new(0);
    let err = zero.push(TraceEvent::new(1, TraceStage::Mapping, 1, 1, 0)).err();
    let ok = err == Some(TraceError::Capacity) && zero.len() == 0 && zero.rejected() == 1;
    s.add("50009-J1-零容量", ok, if ok { "" } else { "零容量未拒" });

    // J1-11 票号回退拒绝且不污染账本
    let mut mono = TraceRing::new(8);
    let ok = mono.push(TraceEvent::new(1, TraceStage::Mapping, 1, 5, 0)).is_ok()
        && mono.push(TraceEvent::new(1, TraceStage::Intent, 2, 4, 0)).err()
            == Some(TraceError::Order)
        && mono.len() == 1
        && mono.rejected() == 1;
    s.add("50009-J1-票号单调", ok, if ok { "" } else { "回退票号被收" });

    // J1-12 载荷越界与时钟超期各自可判
    let mut bad = TraceRing::new(4);
    let code_err = bad
        .push(TraceEvent::new(1, TraceStage::Mapping, 1, 1, 7))
        .err();
    let mut late = TraceRing::new(4);
    let mut ok = code_err == Some(TraceError::BadCode);
    ok &= late.push(TraceEvent::new(2, TraceStage::Mapping, 900, 1, 0)).is_ok();
    ok &= late.push(TraceEvent::new(2, TraceStage::Intent, 950, 2, 0)).is_ok();
    ok &= late.push(TraceEvent::new(2, TraceStage::Fallback, 990, 3, 0)).is_ok();
    let rl = audit(&late.snapshot(), 500);
    let ok2 = match rl {
        Ok(r) => r.stale == 3 && r.jobs == 1,
        Err(_) => false,
    };
    let ok = ok && ok2;
    let det = format!("code_err={:?}", code_err);
    s.add_dyn("50009-J1-载荷与超期", ok, &det);

    // J1-13 审计确定性：同输入两次逐字段相等
    let ra = audit(&snap, 1_000_000);
    let rb = audit(&snap, 1_000_000);
    let ok = ra == rb && ra.is_ok();
    s.add("50009-J1-审计确定", ok, if ok { "" } else { "两次审计不一致" });

    // J1-14 单作业抽取
    let one = ring.events_for(5);
    let ok = one.len() == 3
        && one.iter().all(|e| e.job == 5)
        && one.first().map(|e| e.stage) == Some(TraceStage::Mapping);
    s.add("50009-J1-单作业抽取", ok, if ok { "" } else { "抽取错乱" });

    // J1-15 摘要非空且含覆盖率字段
    let text = match audit(&snap, 1_000_000) {
        Ok(r) => r.summary(),
        Err(_) => String::new(),
    };
    let ok = !text.is_empty() && text.contains("coverage=1.000") && text.contains("jobs=64");
    s.add("50009-J1-摘要非空", ok, if ok { "" } else { "摘要缺失" });

    // J1-16 五类注入全抓
    let (caught, total) = injection_core();
    let ok = caught == total;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50009-J1-注入必抓", ok, &det);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50009_checks_green() {
        let set = run_unx50009_checks();
        assert!(set.all_passed(), "UNX-F50009 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50009_ring_wraps() {
        let mut ring = TraceRing::new(2);
        assert!(ring.push(TraceEvent::new(1, TraceStage::Mapping, 1, 1, 0)).is_ok());
        assert!(ring.push(TraceEvent::new(1, TraceStage::Intent, 2, 2, 0)).is_ok());
        assert!(ring.push(TraceEvent::new(1, TraceStage::Fallback, 3, 3, 0)).is_ok());
        let snap = ring.snapshot();
        assert_eq!(snap.len(), 2);
        assert_eq!(snap.first().map(|e| e.seq), Some(2));
    }

    #[test]
    fn unx50009_job_state_machine() {
        let mut t = JobTrace::new(7);
        assert_eq!(t.record(TraceStage::Intent, 1).err(), Some(TraceError::StageMask));
        assert!(t.record(TraceStage::Mapping, 2).is_ok());
        assert_eq!(t.record(TraceStage::Mapping, 3).err(), Some(TraceError::Duplicate));
        assert!(t.record(TraceStage::Intent, 4).is_ok());
        assert!(t.record(TraceStage::Fallback, 5).is_ok());
        assert!(t.is_full());
    }

    #[test]
    fn unx50009_error_codes_in_segment() {
        assert_eq!(TraceError::Empty.code(), SEG_BASE + 1);
        assert_eq!(TraceError::Overflow.code(), SEG_BASE + 10);
    }
}
