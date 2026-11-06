//! UNX-F50090 字体埋点（子集化/下载/替换三点）（0x43760 段）
//!
//! 锚点：UNX-F50090-J1 埋点 100% 覆盖；判据锚定 体验十三·补。
//!
//! 数据段 `SEG_BASE = 0x43760`：诊断码闭集 `SEG_BASE + 1 ..= SEG_BASE + 11`，
//! 逐项见 [`FontTraceError`]。
//!
//! 字体作业的埋点不是「三个点都打一遍」：一份已内嵌、无需替换的字体根本不
//! 会有下载与替换事件。覆盖率必须按**作业计划**计算——[`JobPlan`] 先声明
//! 本作业应出现哪些点（子集化 → 下载 → 替换，顺序固定，下载依赖子集化），
//! [*] 只统计计划内的点：
//! * 计划外的点被 [`FontTraceError::NotApplicable`] 拒绝，不计入覆盖；
//! * 计划内的点缺失会进入报告的 missing 清单，覆盖率随之下降；
//! * 每个点的载荷有真实语义（保留字形数 / 下载 KiB / 替换动因码），并做
//!   范围校验；子集化保留字形数超过源字形数按 Overrun 拒绝。
//!
//! 结构清单：
//! * [`FontPoint`]/[`JobPlan`]：三点位掩码与作业计划（含合法性校验）；
//! * [`JobTrace`] 单作业状态机：Planned→Running→Complete，跳点/重复/越权全拒；
//! * [`TraceBook`] 计划注册表：按 job 查找、容量、重复计划、未知作业；
//! * [`TraceSpool`] 有界环形事件账本：票号单调、覆盖丢弃、窗口期拒绝；
//! * [`CoverageReport`] 重放审计器：missing/unexpected/duplicate/order/payload/
//!   overrun/stale 全部逐项计数，覆盖率按计划内点计算；
//! * 注入核 [`injection_core`]：十二类坏样本逐类断言具体诊断码。

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x43760 独占 0x20）。
pub const SEG_BASE: u32 = 0x43760;

pub const E_EMPTY: u16 = 1;
pub const E_CAPACITY: u16 = 2;
pub const E_ORDER: u16 = 3;
pub const E_PAYLOAD: u16 = 4;
pub const E_NOT_APPLICABLE: u16 = 5;
pub const E_DUPLICATE: u16 = 6;
pub const E_OVERRUN: u16 = 7;
pub const E_STALE: u16 = 8;
pub const E_UNKNOWN_JOB: u16 = 9;
pub const E_PLAN: u16 = 10;
pub const E_FULL: u16 = 11;

/// 字体埋点错误闭集。`code()` 返回 `SEG_BASE + n`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FontTraceError {
    Empty,
    Capacity,
    Order,
    Payload,
    NotApplicable,
    Duplicate,
    Overrun,
    Stale,
    UnknownJob,
    Plan,
    Full,
}

impl FontTraceError {
    pub fn code(self) -> u32 {
        let n = match self {
            FontTraceError::Empty => E_EMPTY,
            FontTraceError::Capacity => E_CAPACITY,
            FontTraceError::Order => E_ORDER,
            FontTraceError::Payload => E_PAYLOAD,
            FontTraceError::NotApplicable => E_NOT_APPLICABLE,
            FontTraceError::Duplicate => E_DUPLICATE,
            FontTraceError::Overrun => E_OVERRUN,
            FontTraceError::Stale => E_STALE,
            FontTraceError::UnknownJob => E_UNKNOWN_JOB,
            FontTraceError::Plan => E_PLAN,
            FontTraceError::Full => E_FULL,
        };
        SEG_BASE + n as u32
    }

    pub fn message(self) -> &'static str {
        match self {
            FontTraceError::Empty => "no plans or events supplied",
            FontTraceError::Capacity => "spool capacity is zero",
            FontTraceError::Order => "trace point sequence out of order",
            FontTraceError::Payload => "point payload outside legal range",
            FontTraceError::NotApplicable => "point not required by this job plan",
            FontTraceError::Duplicate => "point already recorded",
            FontTraceError::Overrun => "subset retained more glyphs than source",
            FontTraceError::Stale => "event older than spool window",
            FontTraceError::UnknownJob => "job id has no registered plan",
            FontTraceError::Plan => "job plan is inconsistent",
            FontTraceError::Full => "plan registry capacity exhausted",
        }
    }
}

/// 埋点三点。位序固定，覆盖掩码直接由 popcount 推导。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FontPoint {
    Subset,
    Download,
    Substitute,
}

/// 固定的现场顺序：先子集化算出要哪些字形，再下载载荷，最后登记替换。
pub const POINT_ORDER: [FontPoint; 3] = [FontPoint::Subset, FontPoint::Download, FontPoint::Substitute];

impl FontPoint {
    pub fn code(self) -> u8 {
        match self {
            FontPoint::Subset => 0,
            FontPoint::Download => 1,
            FontPoint::Substitute => 2,
        }
    }

    pub fn from_code(c: u8) -> Result<FontPoint, FontTraceError> {
        match c {
            0 => Ok(FontPoint::Subset),
            1 => Ok(FontPoint::Download),
            2 => Ok(FontPoint::Substitute),
            _ => Err(FontTraceError::Payload),
        }
    }

    pub fn bit(self) -> u8 {
        1u8 << self.code()
    }

    pub fn label(self) -> &'static str {
        match self {
            FontPoint::Subset => "subset",
            FontPoint::Download => "download",
            FontPoint::Substitute => "substitute",
        }
    }
}

/// 作业计划。`needs_download` 依赖 `needs_subset`：未定子集就下载等于盲拉全量。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JobPlan {
    pub job: u16,
    pub source_glyphs: u16,
    pub needs_subset: bool,
    pub needs_download: bool,
    pub needs_subst: bool,
}

impl JobPlan {
    /// 计划合法性在入口一次判清，后续覆盖计算不再处理矛盾计划。
    pub fn new(
        job: u16,
        source_glyphs: u16,
        needs_subset: bool,
        needs_download: bool,
        needs_subst: bool,
    ) -> Result<JobPlan, FontTraceError> {
        if job == 0 || source_glyphs == 0 {
            return Err(FontTraceError::Plan);
        }
        if needs_download && !needs_subset {
            return Err(FontTraceError::Plan);
        }
        let p = JobPlan {
            job,
            source_glyphs,
            needs_subset,
            needs_download,
            needs_subst,
        };
        if p.expected_mask() == 0 {
            return Err(FontTraceError::Plan);
        }
        Ok(p)
    }

    /// 计划内点位的并集。覆盖率只认这张掩码，不认事件条数。
    pub fn expected_mask(&self) -> u8 {
        let mut m = 0u8;
        if self.needs_subset {
            m |= FontPoint::Subset.bit();
        }
        if self.needs_download {
            m |= FontPoint::Download.bit();
        }
        if self.needs_subst {
            m |= FontPoint::Substitute.bit();
        }
        m
    }

    pub fn expected_points(&self) -> usize {
        self.expected_mask().count_ones() as usize
    }
}

/// 计划内、尚未记录的下一个点（None 表示已记满）。
fn next_point(expected: u8, mask: u8) -> Option<FontPoint> {
    let mut i = 0usize;
    while i < POINT_ORDER.len() {
        if let Some(p) = POINT_ORDER.get(i) {
            let bit = p.bit();
            if (expected & bit) == bit && (mask & bit) == 0 {
                return Some(*p);
            }
        }
        i += 1;
    }
    None
}

fn payload_ok(point: FontPoint, payload: u16, source_glyphs: u16) -> Result<(), FontTraceError> {
    match point {
        FontPoint::Subset => {
            if payload == 0 || payload > 60_000 {
                return Err(FontTraceError::Payload);
            }
            if payload > source_glyphs {
                return Err(FontTraceError::Overrun);
            }
        }
        FontPoint::Download => {
            if payload == 0 || payload > 60_000 {
                return Err(FontTraceError::Payload);
            }
        }
        FontPoint::Substitute => {
            if payload > 3 {
                return Err(FontTraceError::Payload);
            }
        }
    }
    Ok(())
}

/// 一条埋点事件。payload 语义随点位而变：保留字形数 / KiB / 替换动因码。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FontEvent {
    pub job: u16,
    pub point: FontPoint,
    pub tick: u64,
    pub seq: u32,
    pub payload: u16,
}

impl FontEvent {
    pub const fn new(job: u16, point: FontPoint, tick: u64, seq: u32, payload: u16) -> FontEvent {
        FontEvent {
            job,
            point,
            tick,
            seq,
            payload,
        }
    }
}

/// 单作业状态机。Complete 之后不再收点；任何非法点都不改掩码。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TraceState {
    Planned,
    Running,
    Complete,
    Failed,
}

impl TraceState {
    pub fn label(self) -> &'static str {
        match self {
            TraceState::Planned => "planned",
            TraceState::Running => "running",
            TraceState::Complete => "complete",
            TraceState::Failed => "failed",
        }
    }
}

/// 单作业轨迹：计划 + 掩码 + 时钟。
#[derive(Clone, Debug, PartialEq)]
pub struct JobTrace {
    pub plan: JobPlan,
    pub mask: u8,
    pub state: TraceState,
    pub last_tick: u64,
    pub regress: u16,
}

impl JobTrace {
    pub fn new(plan: JobPlan) -> JobTrace {
        JobTrace {
            plan,
            mask: 0,
            state: TraceState::Planned,
            last_tick: 0,
            regress: 0,
        }
    }

    pub fn expected_mask(&self) -> u8 {
        self.plan.expected_mask()
    }

    pub fn missing_mask(&self) -> u8 {
        self.expected_mask() & !self.mask
    }

    pub fn is_full(&self) -> bool {
        self.mask == self.expected_mask()
    }

    /// 记一个点。顺序、重复、适用性、载荷四道校验，任一失败都保持原掩码。
    pub fn record(
        &mut self,
        point: FontPoint,
        payload: u16,
        tick: u64,
    ) -> Result<TraceState, FontTraceError> {
        let expected = self.expected_mask();
        let bit = point.bit();
        if (expected & bit) != bit {
            return Err(FontTraceError::NotApplicable);
        }
        if (self.mask & bit) == bit {
            return Err(FontTraceError::Duplicate);
        }
        match next_point(expected, self.mask) {
            Some(p) if p == point => {}
            _ => return Err(FontTraceError::Order),
        }
        payload_ok(point, payload, self.plan.source_glyphs)?;
        if tick < self.last_tick {
            self.regress += 1;
        } else {
            self.last_tick = tick;
        }
        self.mask |= bit;
        self.state = if self.mask == expected {
            TraceState::Complete
        } else {
            TraceState::Running
        };
        Ok(self.state)
    }

    /// 补齐一个计划内点的预期事件（供审计与重放使用，不改变轨迹）。
    pub fn next_expected(&self) -> Option<FontPoint> {
        next_point(self.expected_mask(), self.mask)
    }
}

/// 计划注册表：job 唯一，容量按作业计。
#[derive(Clone, Debug)]
pub struct TraceBook {
    traces: Vec<JobTrace>,
    capacity: usize,
    pub accepted: u32,
    pub rejected: u32,
}

impl TraceBook {
    pub fn new(capacity: usize) -> TraceBook {
        TraceBook {
            traces: Vec::new(),
            capacity,
            accepted: 0,
            rejected: 0,
        }
    }

    /// 登记计划。重复 job 与超容都在写入前拒绝。
    pub fn add_plan(&mut self, plan: JobPlan) -> Result<(), FontTraceError> {
        if self.traces.len() >= self.capacity {
            self.rejected += 1;
            return Err(FontTraceError::Full);
        }
        if self.index_of(plan.job).is_some() {
            self.rejected += 1;
            return Err(FontTraceError::Duplicate);
        }
        self.traces.push(JobTrace::new(plan));
        Ok(())
    }

    fn index_of(&self, job: u16) -> Option<usize> {
        let mut i = 0usize;
        while i < self.traces.len() {
            if let Some(t) = self.traces.get(i) {
                if t.plan.job == job {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    pub fn trace(&self, job: u16) -> Option<&JobTrace> {
        match self.index_of(job) {
            Some(i) => self.traces.get(i),
            None => None,
        }
    }

    /// 对指定 job 记账。未知 job 一律 UnknownJob，绝不隐式建计划。
    pub fn record(
        &mut self,
        job: u16,
        point: FontPoint,
        payload: u16,
        tick: u64,
    ) -> Result<TraceState, FontTraceError> {
        let idx = match self.index_of(job) {
            Some(i) => i,
            None => {
                self.rejected += 1;
                return Err(FontTraceError::UnknownJob);
            }
        };
        let result = match self.traces.get_mut(idx) {
            Some(t) => t.record(point, payload, tick),
            None => Err(FontTraceError::UnknownJob),
        };
        match result {
            Ok(state) => {
                self.accepted += 1;
                Ok(state)
            }
            Err(e) => {
                self.rejected += 1;
                Err(e)
            }
        }
    }

    pub fn len(&self) -> usize {
        self.traces.len()
    }

    pub fn complete_count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.traces.len() {
            if let Some(t) = self.traces.get(i) {
                if t.state == TraceState::Complete {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    pub fn plans(&self) -> Vec<JobPlan> {
        let mut out = Vec::with_capacity(self.traces.len());
        let mut i = 0usize;
        while i < self.traces.len() {
            if let Some(t) = self.traces.get(i) {
                out.push(t.plan);
            }
            i += 1;
        }
        out
    }
}

/// 有界环形事件账本。容量满覆盖最旧，丢弃计数显式保留。
#[derive(Clone, Debug)]
pub struct TraceSpool {
    buf: Vec<FontEvent>,
    cap: usize,
    head: usize,
    len: usize,
    deadline: u64,
    pub pushed: u32,
    pub dropped: u32,
    pub rejected: u32,
    last_seq: u32,
}

impl TraceSpool {
    pub fn new(capacity: usize, deadline: u64) -> TraceSpool {
        TraceSpool {
            buf: Vec::new(),
            cap: capacity,
            head: 0,
            len: 0,
            deadline,
            pushed: 0,
            dropped: 0,
            rejected: 0,
            last_seq: 0,
        }
    }

    /// 入环。容量 0、票号回退、超窗期、载荷越界四类都显式拒绝。
    pub fn push(&mut self, ev: FontEvent) -> Result<(), FontTraceError> {
        if self.cap == 0 {
            self.rejected += 1;
            return Err(FontTraceError::Capacity);
        }
        if self.pushed > 0 && ev.seq <= self.last_seq {
            self.rejected += 1;
            return Err(FontTraceError::Order);
        }
        if ev.tick > self.deadline {
            self.rejected += 1;
            return Err(FontTraceError::Stale);
        }
        if payload_ok(ev.point, ev.payload, ev.payload) != Ok(()) {
            self.rejected += 1;
            return Err(FontTraceError::Payload);
        }
        if self.len < self.cap {
            self.buf.push(ev);
            self.len += 1;
        } else {
            // 覆盖最旧：head 指向最旧槽位，写入后前移。
            if let Some(cell) = self.buf.get_mut(self.head) {
                *cell = ev;
            }
            self.head = (self.head + 1) % self.cap;
            self.dropped += 1;
        }
        self.last_seq = ev.seq;
        self.pushed += 1;
        Ok(())
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn capacity(&self) -> usize {
        self.cap
    }

    /// 按时间序（最旧→最新）导出。空环返回空 Vec。
    pub fn snapshot(&self) -> Vec<FontEvent> {
        let mut out = Vec::with_capacity(self.len);
        let mut i = 0usize;
        while i < self.len {
            let at = (self.head + i) % self.cap;
            if let Some(ev) = self.buf.get(at) {
                out.push(*ev);
            }
            i += 1;
        }
        out
    }

    pub fn events_for(&self, job: u16) -> Vec<FontEvent> {
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

/// 覆盖率报告。missing 为 (job, point.code()) 清单，现场可逐条补埋。
#[derive(Clone, Debug, PartialEq)]
pub struct CoverageReport {
    pub jobs: usize,
    pub full: usize,
    pub expected_points: usize,
    pub recorded_points: usize,
    pub missing: Vec<(u16, u8)>,
    pub unexpected: usize,
    pub duplicates: usize,
    pub order_errors: usize,
    pub payload_errors: usize,
    pub overruns: usize,
    pub stale: usize,
    pub download_kib: u32,
    pub subset_glyphs: u32,
}

impl CoverageReport {
    pub fn missing_count(&self) -> usize {
        self.missing.len()
    }

    /// 覆盖率按计划内点计算；分母为 0 时规定为 0（空计划已在入口拒绝）。
    pub fn coverage(&self) -> f32 {
        if self.expected_points == 0 {
            0.0
        } else {
            self.recorded_points as f32 / self.expected_points as f32
        }
    }

    pub fn summary(&self) -> String {
        format!(
            "jobs={} full={} coverage={:.3} expected={} recorded={} missing={} stale={}",
            self.jobs,
            self.full,
            self.coverage(),
            self.expected_points,
            self.recorded_points,
            self.missing.len(),
            self.stale
        )
    }
}

/// 重放审计：从计划与事件重建覆盖事实。重复/乱序/越权事件不写掩码，只计数。
pub fn audit_events(
    plans: &[JobPlan],
    events: &[FontEvent],
    deadline: u64,
) -> Result<CoverageReport, FontTraceError> {
    if plans.is_empty() || events.is_empty() {
        return Err(FontTraceError::Empty);
    }
    let n = plans.len();
    let mut masks: Vec<u8> = Vec::with_capacity(n);
    let mut i = 0usize;
    while i < n {
        let p = match plans.get(i) {
            Some(v) => v,
            None => return Err(FontTraceError::Plan),
        };
        if p.expected_mask() == 0 || p.source_glyphs == 0 {
            return Err(FontTraceError::Plan);
        }
        // 计划表内 job 必须唯一，否则覆盖归属无法判定。
        let mut j = 0usize;
        while j < i {
            if let Some(prev) = plans.get(j) {
                if prev.job == p.job {
                    return Err(FontTraceError::Duplicate);
                }
            }
            j += 1;
        }
        masks.push(0);
        i += 1;
    }

    let mut reported = CoverageReport {
        jobs: n,
        full: 0,
        expected_points: 0,
        recorded_points: 0,
        missing: Vec::new(),
        unexpected: 0,
        duplicates: 0,
        order_errors: 0,
        payload_errors: 0,
        overruns: 0,
        stale: 0,
        download_kib: 0,
        subset_glyphs: 0,
    };

    let mut e = 0usize;
    while e < events.len() {
        let ev = match events.get(e) {
            Some(v) => *v,
            None => break,
        };
        let mut idx: Option<usize> = None;
        let mut p = 0usize;
        while p < plans.len() {
            if let Some(plan) = plans.get(p) {
                if plan.job == ev.job {
                    idx = Some(p);
                }
            }
            p += 1;
        }
        let at = match idx {
            Some(v) => v,
            None => {
                reported.unexpected += 1;
                e += 1;
                continue;
            }
        };
        let plan = match plans.get(at) {
            Some(v) => *v,
            None => {
                reported.unexpected += 1;
                e += 1;
                continue;
            }
        };
        let bit = ev.point.bit();
        let expected = plan.expected_mask();
        if (expected & bit) != bit {
            reported.unexpected += 1;
            e += 1;
            continue;
        }
        let mask = match masks.get(at) {
            Some(v) => *v,
            None => {
                e += 1;
                continue;
            }
        };
        if (mask & bit) == bit {
            reported.duplicates += 1;
            e += 1;
            continue;
        }
        match next_point(expected, mask) {
            Some(np) if np == ev.point => {}
            _ => {
                reported.order_errors += 1;
                e += 1;
                continue;
            }
        }
        match payload_ok(ev.point, ev.payload, plan.source_glyphs) {
            Ok(()) => {}
            Err(FontTraceError::Overrun) => {
                reported.overruns += 1;
                e += 1;
                continue;
            }
            Err(_) => {
                reported.payload_errors += 1;
                e += 1;
                continue;
            }
        }
        if let Some(cell) = masks.get_mut(at) {
            *cell |= bit;
        }
        if ev.tick > deadline {
            reported.stale += 1;
        }
        match ev.point {
            FontPoint::Subset => reported.subset_glyphs += ev.payload as u32,
            FontPoint::Download => reported.download_kib += ev.payload as u32,
            FontPoint::Substitute => {}
        }
        e += 1;
    }

    let mut q = 0usize;
    while q < plans.len() {
        let plan = match plans.get(q) {
            Some(v) => *v,
            None => break,
        };
        let mask = match masks.get(q) {
            Some(v) => *v,
            None => 0,
        };
        reported.expected_points += plan.expected_points();
        reported.recorded_points += mask.count_ones() as usize;
        if mask == plan.expected_mask() {
            reported.full += 1;
        } else {
            let missing = plan.expected_mask() & !mask;
            let mut o = 0usize;
            while o < POINT_ORDER.len() {
                if let Some(pt) = POINT_ORDER.get(o) {
                    if (missing & pt.bit()) == pt.bit() {
                        reported.missing.push((plan.job, pt.code()));
                    }
                }
                o += 1;
            }
        }
        q += 1;
    }
    Ok(reported)
}

// ---------------------------------------------------------------------------
// 注入核：十二类坏样本
// ---------------------------------------------------------------------------

fn plan(job: u16, subset: bool, download: bool, subst: bool) -> JobPlan {
    match JobPlan::new(job, 1000, subset, download, subst) {
        Ok(p) => p,
        Err(_) => JobPlan {
            job: 1,
            source_glyphs: 1,
            needs_subset: true,
            needs_download: false,
            needs_subst: false,
        },
    }
}

/// 注入核。返回（被抓数, 总数），每条对照具体诊断码。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 12usize;

    // 1. 空审计输入
    if audit_events(&[], &[], 100).err() == Some(FontTraceError::Empty) {
        caught += 1;
    }
    // 2. 矛盾计划：下载但不子集化
    if JobPlan::new(1, 100, false, true, false).err() == Some(FontTraceError::Plan) {
        caught += 1;
    }
    // 3. 计划外点
    let mut t = JobTrace::new(plan(2, true, false, false));
    if t.record(FontPoint::Download, 10, 1).err() == Some(FontTraceError::NotApplicable) {
        caught += 1;
    }
    // 4. 跳点（替换先于子集化）
    let mut o = JobTrace::new(plan(3, true, true, true));
    if o.record(FontPoint::Substitute, 1, 1).err() == Some(FontTraceError::Order) {
        caught += 1;
    }
    // 5. 重复点
    let mut d = JobTrace::new(plan(4, true, false, false));
    let first = d.record(FontPoint::Subset, 500, 1).is_ok();
    if first && d.record(FontPoint::Subset, 500, 2).err() == Some(FontTraceError::Duplicate) {
        caught += 1;
    }
    // 6. 载荷为 0
    let mut p0 = JobTrace::new(plan(5, true, false, false));
    if p0.record(FontPoint::Subset, 0, 1).err() == Some(FontTraceError::Payload) {
        caught += 1;
    }
    // 7. 子集化超出源字形数
    let mut ov = JobTrace::new(plan(6, true, false, false));
    if ov.record(FontPoint::Subset, 1001, 1).err() == Some(FontTraceError::Overrun) {
        caught += 1;
    }
    // 8. 未知作业
    let mut book = TraceBook::new(4);
    let _ = book.add_plan(plan(7, true, false, false));
    if book.record(99, FontPoint::Subset, 10, 1).err() == Some(FontTraceError::UnknownJob) {
        caught += 1;
    }
    // 9. 零容量环
    let mut zero = TraceSpool::new(0, 1_000);
    if zero.push(FontEvent::new(7, FontPoint::Subset, 1, 1, 10)).err()
        == Some(FontTraceError::Capacity)
    {
        caught += 1;
    }
    // 10. 票号回退
    let mut sp = TraceSpool::new(4, 1_000);
    let a = sp.push(FontEvent::new(8, FontPoint::Subset, 1, 5, 10)).is_ok();
    if a && sp.push(FontEvent::new(8, FontPoint::Download, 2, 4, 10)).err() == Some(FontTraceError::Order) {
        caught += 1;
    }
    // 11. 超窗期
    let mut late = TraceSpool::new(4, 50);
    if late.push(FontEvent::new(9, FontPoint::Subset, 100, 1, 10)).err()
        == Some(FontTraceError::Stale)
    {
        caught += 1;
    }
    // 12. 注册表容量满
    let mut full = TraceBook::new(1);
    let r1 = full.add_plan(plan(10, true, false, false)).is_ok();
    if r1 && full.add_plan(plan(11, true, false, false)).err() == Some(FontTraceError::Full) {
        caught += 1;
    }
    (caught, total)
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

fn plan_for(i: u16) -> JobPlan {
    let combo = i % 5;
    let subset = combo != 4;
    let download = combo == 1 || combo == 3;
    let subst = combo >= 2;
    match JobPlan::new(i + 1, 1000 + i, subset, download, subst) {
        Ok(p) => p,
        Err(_) => plan(1, true, false, false),
    }
}

fn record_plan(book: &mut TraceBook, plan: JobPlan, tick: &mut u64, seq: &mut u32) -> bool {
    let order_subset = plan.needs_subset;
    let order_download = plan.needs_download;
    let order_subst = plan.needs_subst;
    if order_subset {
        let payload = plan.source_glyphs / 2 + 1;
        if book
            .record(plan.job, FontPoint::Subset, payload, *tick)
            .is_err()
        {
            return false;
        }
        *tick += 1;
        *seq += 1;
    }
    if order_download {
        if book
            .record(plan.job, FontPoint::Download, 100 + plan.job, *tick)
            .is_err()
        {
            return false;
        }
        *tick += 1;
        *seq += 1;
    }
    if order_subst {
        if book
            .record(plan.job, FontPoint::Substitute, plan.job % 4, *tick)
            .is_err()
        {
            return false;
        }
        *tick += 1;
        *seq += 1;
    }
    true
}

pub fn run_unx50090_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50090");
    let mut book = TraceBook::new(32);
    let mut plans: Vec<JobPlan> = Vec::with_capacity(16);
    let mut events: Vec<FontEvent> = Vec::with_capacity(32);
    let mut tick = 1u64;
    let mut seq = 1u32;
    let mut i = 0u16;
    while i < 16 {
        let p = plan_for(i);
        let _ = book.add_plan(p);
        plans.push(p);
        // 记录前先把事件造出来，保证事件流与 live 轨迹同源可审计。
        let before = tick;
        if record_plan(&mut book, p, &mut tick, &mut seq) {
            let mut t = before;
            let mut local_seq = seq - ((tick - before) as u32);
            if p.needs_subset {
                events.push(FontEvent::new(
                    p.job,
                    FontPoint::Subset,
                    t,
                    local_seq,
                    p.source_glyphs / 2 + 1,
                ));
                t += 1;
                local_seq += 1;
            }
            if p.needs_download {
                events.push(FontEvent::new(
                    p.job,
                    FontPoint::Download,
                    t,
                    local_seq,
                    100 + p.job,
                ));
                t += 1;
                local_seq += 1;
            }
            if p.needs_subst {
                events.push(FontEvent::new(
                    p.job,
                    FontPoint::Substitute,
                    t,
                    local_seq,
                    p.job % 4,
                ));
            }
        }
        i += 1;
    }

    // J1-1 埋点 100% 覆盖：16 作业全部按计划记满
    let audit = audit_events(&plans, &events, 1_000_000);
    let ok = match &audit {
        Ok(r) => {
            r.jobs == 16
                && r.full == 16
                && r.expected_points == 28
                && r.recorded_points == 28
                && r.missing_count() == 0
                && r.coverage() == 1.0
                && r.unexpected == 0
                && r.duplicates == 0
                && r.order_errors == 0
                && r.payload_errors == 0
                && r.overruns == 0
        }
        Err(_) => false,
    };
    let det = match &audit {
        Ok(r) => r.summary(),
        Err(_) => String::new(),
    };
    s.add_dyn("50090-J1-埋点100%覆盖", ok, &det);

    // J1-2 live 轨迹与重放一致：complete 数、掩码数、事件数对齐
    let mut mask_sum = 0usize;
    let mut j = 0u16;
    while j < 16 {
        if let Some(t) = book.trace(j + 1) {
            if t.state == TraceState::Complete {
                mask_sum += t.mask.count_ones() as usize;
            }
        }
        j += 1;
    }
    let ok = book.complete_count() == 16
        && book.accepted == 28
        && book.rejected == 0
        && mask_sum == 28
        && events.len() == 28;
    let det = format!(
        "complete={} accepted={} events={}",
        book.complete_count(),
        book.accepted,
        events.len()
    );
    s.add_dyn("50090-J1-轨迹与重放一致", ok, &det);

    // J1-3 缺一点可定位：摘掉 job 7 的下载点
    let mut holed: Vec<FontEvent> = Vec::with_capacity(events.len());
    let mut h = 0usize;
    while h < events.len() {
        if let Some(ev) = events.get(h) {
            if !(ev.job == 7 && ev.point == FontPoint::Download) {
                holed.push(*ev);
            }
        }
        h += 1;
    }
    let r2 = audit_events(&plans, &holed, 1_000_000);
    let ok = match &r2 {
        Ok(r) => {
            r.full == 15
                && r.missing.len() == 1
                && r.missing.first() == Some(&(7u16, FontPoint::Download.code()))
                && r.recorded_points == 27
                && fabsf(r.coverage() - 27.0 / 28.0) < 1e-6
        }
        Err(_) => false,
    };
    let det = match &r2 {
        Ok(r) => format!("missing={:?} full={}", r.missing, r.full),
        Err(_) => String::new(),
    };
    s.add_dyn("50090-J1-缺点可定位", ok, &det);

    // J1-4 计划外点必须拒绝，且不改掩码
    let mut live = JobTrace::new(plan(40, true, false, false));
    let before_mask = live.mask;
    let err = live.record(FontPoint::Download, 10, 1);
    let ok = err.err() == Some(FontTraceError::NotApplicable) && live.mask == before_mask;
    s.add("50090-J1-计划外必拒", ok, if ok { "" } else { "计划外点被收" });

    // J1-5 跳点与重复各自可判，且状态停在 Running
    let mut live2 = JobTrace::new(plan(41, true, true, true));
    let jump = live2.record(FontPoint::Substitute, 1, 1).err();
    let s1 = live2.record(FontPoint::Subset, 500, 2);
    let dup = live2.record(FontPoint::Subset, 500, 3).err();
    let ok = jump == Some(FontTraceError::Order)
        && s1 == Ok(TraceState::Running)
        && dup == Some(FontTraceError::Duplicate)
        && live2.mask == FontPoint::Subset.bit();
    s.add("50090-J1-跳点与重复", ok, if ok { "" } else { "跳点/重复未判" });

    // J1-6 载荷语义：0 拒、超源拒、替换码越界拒、边界值收
    let mut pl = JobTrace::new(plan(42, true, false, true));
    let zero = pl.record(FontPoint::Subset, 0, 1).err();
    let over = pl.record(FontPoint::Subset, 1001, 2).err();
    let ok_subset = pl.record(FontPoint::Subset, 1000, 3).is_ok();
    let bad_subst = pl.record(FontPoint::Substitute, 4, 4).err();
    let good_subst = pl.record(FontPoint::Substitute, 3, 5);
    let ok = zero == Some(FontTraceError::Payload)
        && over == Some(FontTraceError::Overrun)
        && ok_subset
        && bad_subst == Some(FontTraceError::Payload)
        && good_subst == Ok(TraceState::Complete);
    let det = format!("zero={:?} over={:?}", zero, over);
    s.add_dyn("50090-J1-载荷语义", ok, &det);

    // J1-7 未知作业与重复计划
    let mut rb = TraceBook::new(4);
    let p = plan(43, true, false, false);
    let a = rb.add_plan(p).is_ok();
    let unknown = rb.record(999, FontPoint::Subset, 10, 1).err();
    let dup = rb.add_plan(p).err();
    let ok = a
        && unknown == Some(FontTraceError::UnknownJob)
        && dup == Some(FontTraceError::Duplicate)
        && rb.len() == 1;
    s.add("50090-J1-未知与重复计划", ok, if ok { "" } else { "未知作业/重复计划未拒" });

    // J1-8 计划校验四类：job=0、source=0、下载未子集化、空计划
    let a = JobPlan::new(0, 100, true, false, false).err();
    let b = JobPlan::new(1, 0, true, false, false).err();
    let c = JobPlan::new(1, 100, false, true, false).err();
    let d = JobPlan::new(1, 100, false, false, false).err();
    let ok = a == Some(FontTraceError::Plan)
        && b == Some(FontTraceError::Plan)
        && c == Some(FontTraceError::Plan)
        && d == Some(FontTraceError::Plan);
    s.add("50090-J1-计划校验", ok, if ok { "" } else { "矛盾计划被收" });

    // J1-9 环形覆盖：cap=4 推 6 条保留最后 4 条，丢弃计数 2
    let mut ring = TraceSpool::new(4, 1_000);
    let mut q = 1u32;
    let mut all_ok = true;
    while q <= 6 {
        if ring
            .push(FontEvent::new(1, FontPoint::Subset, q as u64, q, 100 + q as u16))
            .is_err()
        {
            all_ok = false;
        }
        q += 1;
    }
    let kept = ring.snapshot();
    let ok = all_ok
        && kept.len() == 4
        && ring.pushed == 6
        && ring.dropped == 2
        && match (kept.first(), kept.last()) {
            (Some(f), Some(l)) => f.seq == 3 && l.seq == 6,
            _ => false,
        };
    let det = format!("pushed={} dropped={} len={}", ring.pushed, ring.dropped, kept.len());
    s.add_dyn("50090-J1-环形覆盖", ok, &det);

    // J1-10 环的拒绝路径：零容量、票号回退、超窗期、载荷越界
    let mut z = TraceSpool::new(0, 100);
    let zero_cap = z.push(FontEvent::new(1, FontPoint::Subset, 1, 1, 10)).err();
    let mut mono = TraceSpool::new(4, 1_000);
    let p1 = mono.push(FontEvent::new(1, FontPoint::Subset, 1, 5, 10)).is_ok();
    let back = mono.push(FontEvent::new(1, FontPoint::Download, 2, 4, 10)).err();
    let mut late = TraceSpool::new(4, 50);
    let stale = late.push(FontEvent::new(2, FontPoint::Subset, 60, 1, 10)).err();
    let mut badp = TraceSpool::new(4, 1_000);
    let payload = badp.push(FontEvent::new(3, FontPoint::Substitute, 1, 1, 9)).err();
    let ok = zero_cap == Some(FontTraceError::Capacity)
        && p1
        && back == Some(FontTraceError::Order)
        && stale == Some(FontTraceError::Stale)
        && payload == Some(FontTraceError::Payload)
        && mono.len() == 1;
    s.add("50090-J1-环拒绝路径", ok, if ok { "" } else { "环拒绝失效" });

    // J1-11 覆盖重放对重复/乱序/越权只计数，不污染掩码
    let mut noisy: Vec<FontEvent> = Vec::with_capacity(events.len() + 3);
    let mut n = 0usize;
    while n < events.len() {
        if let Some(ev) = events.get(n) {
            noisy.push(*ev);
        }
        n += 1;
    }
    noisy.push(FontEvent::new(1, FontPoint::Download, 100, 999, 10));
    noisy.push(FontEvent::new(2, FontPoint::Subset, 100, 998, 400));
    noisy.push(FontEvent::new(99, FontPoint::Subset, 100, 997, 10));
    let rn = audit_events(&plans, &noisy, 1_000_000);
    let ok = match &rn {
        Ok(r) => {
            r.unexpected == 2
                && r.duplicates == 1
                && r.recorded_points == 28
                && r.full == 16
        }
        Err(_) => false,
    };
    let det = match &rn {
        Ok(r) => format!("unexpected={} dup={}", r.unexpected, r.duplicates),
        Err(_) => String::new(),
    };
    s.add_dyn("50090-J1-噪声只计数", ok, &det);

    // J1-12 超窗期审计：deadline=0 时全部已记录事件计入 stale
    let rs = audit_events(&plans, &events, 0);
    let ok = match &rs {
        Ok(r) => r.stale == 28 && r.full == 16,
        Err(_) => false,
    };
    let det = match &rs {
        Ok(r) => format!("stale={}", r.stale),
        Err(_) => String::new(),
    };
    s.add_dyn("50090-J1-超窗期计数", ok, &det);

    // J1-13 载荷汇总守恒：子集字形与下载 KiB 等于事件逐条求和
    let mut subset_sum = 0u32;
    let mut download_sum = 0u32;
    let mut m = 0usize;
    while m < events.len() {
        if let Some(ev) = events.get(m) {
            match ev.point {
                FontPoint::Subset => subset_sum += ev.payload as u32,
                FontPoint::Download => download_sum += ev.payload as u32,
                FontPoint::Substitute => {}
            }
        }
        m += 1;
    }
    let ok = match &audit {
        Ok(r) => r.subset_glyphs == subset_sum && r.download_kib == download_sum,
        Err(_) => false,
    };
    let det = format!("subset={} dl={}", subset_sum, download_sum);
    s.add_dyn("50090-J1-载荷守恒", ok, &det);

    // J1-14 审计确定性：同输入两次逐字段一致
    let ra = audit_events(&plans, &events, 1_000_000);
    let rb = audit_events(&plans, &events, 1_000_000);
    let ok = ra == rb && ra.is_ok();
    s.add("50090-J1-审计确定", ok, if ok { "" } else { "两次审计不一致" });

    // J1-15 摘要非空且含覆盖率字段
    let text = match &audit {
        Ok(r) => r.summary(),
        Err(_) => String::new(),
    };
    let ok = !text.is_empty()
        && text.contains("jobs=16")
        && text.contains("coverage=1.000")
        && text.contains("missing=0");
    s.add("50090-J1-摘要非空", ok, if ok { "" } else { "摘要缺字段" });

    // J1-16 错误码闭集：十一码连续、消息非空
    let all = [
        FontTraceError::Empty,
        FontTraceError::Capacity,
        FontTraceError::Order,
        FontTraceError::Payload,
        FontTraceError::NotApplicable,
        FontTraceError::Duplicate,
        FontTraceError::Overrun,
        FontTraceError::Stale,
        FontTraceError::UnknownJob,
        FontTraceError::Plan,
        FontTraceError::Full,
    ];
    let mut e = 0usize;
    let mut ok = true;
    while e < all.len() {
        if let Some(err) = all.get(e) {
            if err.code() != SEG_BASE + (e as u32) + 1 || err.message().is_empty() {
                ok = false;
            }
        }
        e += 1;
    }
    s.add("50090-J1-码值闭集", ok, if ok { "" } else { "码值漂移" });

    // J1-17 注入核十二类全抓
    let (caught, total) = injection_core();
    let ok = caught == total;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50090-J1-注入必抓", ok, &det);

    s
}

fn fabsf(v: f32) -> f32 {
    if v < 0.0 {
        -v
    } else {
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50090_checks_green() {
        let set = run_unx50090_checks();
        assert!(set.all_passed(), "UNX-F50090 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50090_coverage_counts_plan_only() {
        let p = plan(5, true, false, true);
        let mut t = JobTrace::new(p);
        assert!(t.record(FontPoint::Subset, 500, 1).is_ok());
        assert_eq!(
            t.record(FontPoint::Download, 10, 2).err(),
            Some(FontTraceError::NotApplicable)
        );
        assert!(t.record(FontPoint::Substitute, 1, 3).is_ok());
        assert!(t.is_full());
    }

    #[test]
    fn unx50090_spool_wraps() {
        let mut sp = TraceSpool::new(1, 1_000);
        assert!(sp.push(FontEvent::new(1, FontPoint::Subset, 1, 1, 10)).is_ok());
        assert!(sp.push(FontEvent::new(1, FontPoint::Download, 2, 2, 20)).is_ok());
        let snap = sp.snapshot();
        assert_eq!(snap.len(), 1);
        assert_eq!(snap.first().map(|e| e.seq), Some(2));
        assert_eq!(sp.dropped, 1);
    }
}
