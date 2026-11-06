//! UNX-F50071 纸张处理埋点（选择/切换/缺纸三点）（0x43500 段）
//!
//! 锚点：UNX-F50071-J1 埋点 100% 覆盖；判据锚定 体验十三·补。
//!
//! 数据段：`SEG_BASE = 0x43500`；诊断码闭集 `SEG_BASE + 1 ..= SEG_BASE + 10`，
//! 逐项见 [`TraceError`]。不得借位相邻卡（0x434e0 双面页序卡；0x43520 起
//! 为下一卡）。
//!
//! 埋点的价值不在「记了多少条」，而在「三个关键点是否都记到了」。本模块
//! 因此把埋点拆成两层：
//! * `TraceRing`：定容环形记录器。序号由记录器单调分配、时间戳不得回退，
//!   保证「同一次操作两次记录」可被区分；容量满时策略二选一（拒收并暴露
//!   `RingFull` / 丢最旧并记 `dropped`），绝不静默丢事件；
//! * `Coverage` + `TraceAudit`：覆盖账。三个点（选择/切换/缺纸）各自计数，
//!   掩码 `0b111` 才算 100% 覆盖；`close_strict` 在覆盖不全时拒绝关账，
//!   防止「少记一个点也算完成」；
//! * 回放：`replay` 把事件压成确定性文本，供日志/联签复核；
//! * 注入核：7 类坏样本（坏点码、零纸号、时间回退、环满、坏策略、坏容量、
//!   覆盖不全关账），逐类断言诊断码。
//!
//! 为什么时间戳允许相等：驱动侧 `ticks` 可能来自毫秒级时钟，同一次
//! 「选择→切换」在同一毫秒内完成是常态；只禁止严格回退，既守住因果，
//! 又不把正常高频操作判成坏样本。

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x43500 独占 0x20）。
pub const SEG_BASE: u32 = 0x43500;

/// 单次会话环形容量上限。256 条足够覆盖一次换纸排障窗口；
/// 再大应当落盘而不是占内核内存。
pub const MAX_CAP: usize = 256;
/// 默认容量。
pub const DEFAULT_CAP: usize = 64;

const E_BAD_POINT: u16 = 1;
const E_ZERO_PAPER: u16 = 2;
const E_TIME_WARP: u16 = 3;
const E_RING_FULL: u16 = 4;
const E_BAD_POLICY: u16 = 5;
const E_SEQUENCE: u16 = 6;
const E_CLOSED: u16 = 7;
const E_COVERAGE: u16 = 8;
const E_BAD_CAPACITY: u16 = 9;
const E_REPLAY_EMPTY: u16 = 10;

/// 埋点错误十型。`code()` 恒为 `SEG_BASE + n`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TraceError {
    /// 埋点类型码不在 1..=3。
    BadPoint,
    /// 纸 ID 为 0（0 表示「无纸」，不能作为事件主体）。
    ZeroPaper,
    /// 时间戳严格回退。
    TimeWarp,
    /// 环形缓冲已满且策略为拒收。
    RingFull,
    /// 满环策略码不在 0..=1。
    BadPolicy,
    /// 导入事件的序号不连续（跳号/重号）。
    SequenceBreak,
    /// 会话已关账，拒绝再写入/重复关账。
    SessionClosed,
    /// 关账时三点未全部覆盖。
    CoverageIncomplete,
    /// 容量为 0 或超过 256。
    BadCapacity,
    /// 空环回放。
    ReplayEmpty,
}

impl TraceError {
    /// 段内序号（1 起），与 `code()` 一一对应。
    fn ordinal(self) -> u16 {
        match self {
            TraceError::BadPoint => E_BAD_POINT,
            TraceError::ZeroPaper => E_ZERO_PAPER,
            TraceError::TimeWarp => E_TIME_WARP,
            TraceError::RingFull => E_RING_FULL,
            TraceError::BadPolicy => E_BAD_POLICY,
            TraceError::SequenceBreak => E_SEQUENCE,
            TraceError::SessionClosed => E_CLOSED,
            TraceError::CoverageIncomplete => E_COVERAGE,
            TraceError::BadCapacity => E_BAD_CAPACITY,
            TraceError::ReplayEmpty => E_REPLAY_EMPTY,
        }
    }

    /// 诊断码：段基址 + 序号。
    pub fn code(self) -> u32 {
        SEG_BASE + self.ordinal() as u32
    }

    /// 一行诊断文本。
    pub fn message(self) -> &'static str {
        match self {
            TraceError::BadPoint => "paper trace point code invalid",
            TraceError::ZeroPaper => "paper id zero in trace event",
            TraceError::TimeWarp => "trace ticks moved backwards",
            TraceError::RingFull => "trace ring full (reject policy)",
            TraceError::BadPolicy => "overflow policy code invalid",
            TraceError::SequenceBreak => "imported trace seq not contiguous",
            TraceError::SessionClosed => "trace session already closed",
            TraceError::CoverageIncomplete => "select/switch/out not all traced",
            TraceError::BadCapacity => "trace capacity outside 1..=256",
            TraceError::ReplayEmpty => "replay of empty trace",
        }
    }
}

/// 全错误表。
pub const ALL_ERRORS: [TraceError; 10] = [
    TraceError::BadPoint,
    TraceError::ZeroPaper,
    TraceError::TimeWarp,
    TraceError::RingFull,
    TraceError::BadPolicy,
    TraceError::SequenceBreak,
    TraceError::SessionClosed,
    TraceError::CoverageIncomplete,
    TraceError::BadCapacity,
    TraceError::ReplayEmpty,
];

/// 三个必测点。码值即覆盖掩码位序（1/2/3 → bit0/bit1/bit2）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TracePoint {
    Select,
    Switch,
    OutOfPaper,
}

impl TracePoint {
    /// 三点全表（判据遍历用）。
    pub const ALL: [TracePoint; 3] = [
        TracePoint::Select,
        TracePoint::Switch,
        TracePoint::OutOfPaper,
    ];

    /// 从事件码解析：1=选择、2=切换、3=缺纸。
    pub fn from_code(v: u8) -> Result<TracePoint, TraceError> {
        match v {
            1 => Ok(TracePoint::Select),
            2 => Ok(TracePoint::Switch),
            3 => Ok(TracePoint::OutOfPaper),
            _ => Err(TraceError::BadPoint),
        }
    }

    /// 对外码。
    pub fn code(self) -> u8 {
        match self {
            TracePoint::Select => 1,
            TracePoint::Switch => 2,
            TracePoint::OutOfPaper => 3,
        }
    }

    /// 覆盖掩码位。
    pub fn bit(self) -> u8 {
        1u8 << (self.code() - 1)
    }

    /// 展示名（回放文本用）。
    pub fn label(self) -> &'static str {
        match self {
            TracePoint::Select => "select",
            TracePoint::Switch => "switch",
            TracePoint::OutOfPaper => "out-of-paper",
        }
    }
}

/// 满环策略：拒收（默认，显式报错）或丢最旧（记 dropped 后继续）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TracePolicy {
    Reject,
    DropOldest,
}

impl TracePolicy {
    /// 从策略码解析：0=拒收、1=丢最旧。
    pub fn from_code(v: u8) -> Result<TracePolicy, TraceError> {
        match v {
            0 => Ok(TracePolicy::Reject),
            1 => Ok(TracePolicy::DropOldest),
            _ => Err(TraceError::BadPolicy),
        }
    }

    /// 对外码。
    pub fn code(self) -> u8 {
        match self {
            TracePolicy::Reject => 0,
            TracePolicy::DropOldest => 1,
        }
    }
}

/// 一条埋点事件。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TraceEvent {
    pub seq: u32,
    pub point: TracePoint,
    pub paper_id: u16,
    pub ticks: u64,
}

/// 覆盖账：三点计数 + 掩码。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Coverage {
    pub select: u32,
    pub switch: u32,
    pub out_of_paper: u32,
    pub mask: u8,
}

impl Coverage {
    /// 空覆盖账。
    pub const fn empty() -> Coverage {
        Coverage {
            select: 0,
            switch: 0,
            out_of_paper: 0,
            mask: 0,
        }
    }

    /// 是否三点齐全。
    pub fn is_complete(&self) -> bool {
        self.mask == 0b111
    }

    /// 某点的计数。
    pub fn count(&self, point: TracePoint) -> u32 {
        match point {
            TracePoint::Select => self.select,
            TracePoint::Switch => self.switch,
            TracePoint::OutOfPaper => self.out_of_paper,
        }
    }
}

/// 定容环形埋点记录器（同时是会话状态机：打开 → 关账）。
#[derive(Clone, Debug)]
pub struct TraceRing {
    events: Vec<TraceEvent>,
    cap: usize,
    next_seq: u32,
    dropped: u32,
    last_ticks: u64,
    policy: TracePolicy,
    closed: bool,
}

impl TraceRing {
    /// 以容量构造。0 或 >256 显式拒绝。
    pub fn new(cap: usize) -> Result<TraceRing, TraceError> {
        if cap == 0 || cap > MAX_CAP {
            return Err(TraceError::BadCapacity);
        }
        Ok(TraceRing {
            events: Vec::with_capacity(cap),
            cap,
            next_seq: 0,
            dropped: 0,
            last_ticks: 0,
            policy: TracePolicy::Reject,
            closed: false,
        })
    }

    /// 默认容量记录器。
    pub fn default_ring() -> TraceRing {
        TraceRing {
            events: Vec::with_capacity(DEFAULT_CAP),
            cap: DEFAULT_CAP,
            next_seq: 0,
            dropped: 0,
            last_ticks: 0,
            policy: TracePolicy::Reject,
            closed: false,
        }
    }

    /// 当前事件数。
    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// 是否空环。
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// 容量。
    pub fn capacity(&self) -> usize {
        self.cap
    }

    /// 丢最旧策略下被丢弃的条数（显性记账，不静默）。
    pub fn dropped(&self) -> u32 {
        self.dropped
    }

    /// 当前满环策略。
    pub fn policy(&self) -> TracePolicy {
        self.policy
    }

    /// 是否已关账。
    pub fn is_closed(&self) -> bool {
        self.closed
    }

    /// 按下标取事件。
    pub fn at(&self, idx: usize) -> Option<TraceEvent> {
        self.events.get(idx).copied()
    }

    /// 设置满环策略。
    pub fn set_policy(&mut self, code: u8) -> Result<(), TraceError> {
        let p = TracePolicy::from_code(code)?;
        self.policy = p;
        Ok(())
    }

    /// 记录一条事件，返回分配到的序号。
    ///
    /// 校验顺序固定：关账 → 纸号 → 时间 → 容量。顺序固定让同一坏样本
    /// 无论何时注入都得到同一个诊断码（注入核据此逐类断言）。
    pub fn record(&mut self, point: TracePoint, paper_id: u16, ticks: u64) -> Result<u32, TraceError> {
        if self.closed {
            return Err(TraceError::SessionClosed);
        }
        if paper_id == 0 {
            return Err(TraceError::ZeroPaper);
        }
        if ticks < self.last_ticks {
            return Err(TraceError::TimeWarp);
        }
        if self.events.len() >= self.cap {
            match self.policy {
                TracePolicy::Reject => return Err(TraceError::RingFull),
                TracePolicy::DropOldest => {
                    // 非空（len == cap ≥ 1）时才丢；remove 的下标 0 恒合法。
                    let _ = self.events.remove(0);
                    self.dropped = self.dropped.saturating_add(1);
                }
            }
        }
        if self.next_seq == u32::MAX {
            // 序号空间耗尽：宁可拒收也不回绕成 0 与历史事件撞号。
            return Err(TraceError::RingFull);
        }
        let seq = self.next_seq;
        self.next_seq = self.next_seq.saturating_add(1);
        self.last_ticks = ticks;
        self.events.push(TraceEvent {
            seq,
            point,
            paper_id,
            ticks,
        });
        Ok(seq)
    }

    /// 扫描覆盖账。
    pub fn coverage(&self) -> Coverage {
        let mut c = Coverage::empty();
        let mut i = 0usize;
        while i < self.events.len() {
            if let Some(e) = self.events.get(i) {
                match e.point {
                    TracePoint::Select => c.select = c.select.saturating_add(1),
                    TracePoint::Switch => c.switch = c.switch.saturating_add(1),
                    TracePoint::OutOfPaper => {
                        c.out_of_paper = c.out_of_paper.saturating_add(1)
                    }
                }
                c.mask |= e.point.bit();
            }
            i += 1;
        }
        c
    }

    /// 严格关账：三点不齐拒绝关账（这是 J1「100% 覆盖」的执行点）。
    pub fn close_strict(&mut self) -> Result<Coverage, TraceError> {
        if self.closed {
            return Err(TraceError::SessionClosed);
        }
        let c = self.coverage();
        if !c.is_complete() {
            return Err(TraceError::CoverageIncomplete);
        }
        self.closed = true;
        Ok(c)
    }

    /// 事件内容摘要（FNV-1a），覆盖序号/类型/纸号/时间戳。
    pub fn digest(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        h = mix(h, self.events.len() as u64);
        let mut i = 0usize;
        while i < self.events.len() {
            if let Some(e) = self.events.get(i) {
                h = mix(h, e.seq as u64);
                h = mix(h, e.point.code() as u64);
                h = mix(h, e.paper_id as u64);
                h = mix(h, e.ticks);
            }
            i += 1;
        }
        h
    }

    /// 确定性回放文本：`select#0@100/7;switch#1@200/9;...`。
    /// 空环返回 `ReplayEmpty`——空回放看起来像「全绿」，必须显式失败。
    pub fn replay(&self) -> Result<String, TraceError> {
        if self.events.is_empty() {
            return Err(TraceError::ReplayEmpty);
        }
        let mut out = String::with_capacity(self.events.len() * 24);
        let mut i = 0usize;
        while i < self.events.len() {
            if let Some(e) = self.events.get(i) {
                let piece = format!(
                    "{}#{}@{}/{};",
                    e.point.label(),
                    e.seq,
                    e.ticks,
                    e.paper_id
                );
                out.push_str(&piece);
            }
            i += 1;
        }
        Ok(out)
    }

    /// 导入外部事件序列：序号必须从当前 `next_seq` 起严格连续。
    /// 这样「从日志恢复」不会把别人的序号空间混进本会话。
    pub fn import(&mut self, src: &[TraceEvent]) -> Result<usize, TraceError> {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < src.len() {
            match src.get(i) {
                Some(ev) => {
                    if ev.seq != self.next_seq {
                        return Err(TraceError::SequenceBreak);
                    }
                    self.record(ev.point, ev.paper_id, ev.ticks)?;
                    n += 1;
                }
                None => return Err(TraceError::SequenceBreak),
            }
            i += 1;
        }
        Ok(n)
    }
}

/// FNV-1a 混合原语。
fn mix(h: u64, v: u64) -> u64 {
    let x = h ^ v;
    x.wrapping_mul(0x100_0000_01b3)
}

/// 覆盖总账：跨会话累计，回答「这轮排障是否每类点都至少记过一次」。
#[derive(Clone, Debug)]
pub struct TraceAudit {
    sessions: Vec<Coverage>,
}

impl TraceAudit {
    /// 空账。
    pub fn new() -> TraceAudit {
        TraceAudit {
            sessions: Vec::with_capacity(16),
        }
    }

    /// 登记一次会话的覆盖账。
    pub fn push(&mut self, c: Coverage) {
        self.sessions.push(c);
    }

    /// 会话数。
    pub fn total(&self) -> usize {
        self.sessions.len()
    }

    /// 三点齐全的会话数。
    pub fn covered(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.sessions.len() {
            if let Some(c) = self.sessions.get(i) {
                if c.is_complete() {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 覆盖率 ×100（整数百分比，避免浮点）。空账记 0，不记 100。
    pub fn percent_x100(&self) -> u32 {
        if self.sessions.is_empty() {
            return 0;
        }
        ((self.covered() as u64) * 100 / (self.sessions.len() as u64)) as u32
    }

    /// 是否 100% 覆盖（且至少一场）。
    pub fn is_full(&self) -> bool {
        !self.sessions.is_empty() && self.covered() == self.sessions.len()
    }
}

// ---------------------------------------------------------------------------
// 注入核：七类坏样本必须全部被抓
// ---------------------------------------------------------------------------

/// 构造 7 类坏样本并断言诊断码。返回 (抓到, 总数)。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 7usize;

    // 1. 坏点码：9 不在 1..=3。
    if TracePoint::from_code(9).err() == Some(TraceError::BadPoint) {
        caught += 1;
    }
    let mut a = match TraceRing::new(4) {
        Ok(r) => r,
        Err(_) => return (0, total),
    };
    // 2. 零纸号。
    if a.record(TracePoint::Select, 0, 10).err() == Some(TraceError::ZeroPaper) {
        caught += 1;
    }
    // 3. 时间回退：先 100 再 99。
    let warp = a.record(TracePoint::Select, 1, 100).is_ok()
        && a.record(TracePoint::Switch, 1, 99).err() == Some(TraceError::TimeWarp);
    if warp {
        caught += 1;
    }
    // 4. 环满拒收：容量 2，写第 3 条。
    let mut b = match TraceRing::new(2) {
        Ok(r) => r,
        Err(_) => return (caught, total),
    };
    let full = b.record(TracePoint::Select, 1, 1).is_ok()
        && b.record(TracePoint::Switch, 1, 2).is_ok()
        && b.record(TracePoint::OutOfPaper, 1, 3).err() == Some(TraceError::RingFull);
    if full {
        caught += 1;
    }
    // 5. 坏策略码。
    if a.set_policy(2).err() == Some(TraceError::BadPolicy) {
        caught += 1;
    }
    // 6. 坏容量。
    if TraceRing::new(0).err() == Some(TraceError::BadCapacity)
        && TraceRing::new(MAX_CAP + 1).err() == Some(TraceError::BadCapacity)
    {
        caught += 1;
    }
    // 7. 覆盖不全就关账。
    let mut c = match TraceRing::new(8) {
        Ok(r) => r,
        Err(_) => return (caught, total),
    };
    if c.record(TracePoint::Select, 1, 10).is_ok()
        && c.close_strict().err() == Some(TraceError::CoverageIncomplete)
    {
        caught += 1;
    }
    (caught, total)
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// 跑 10 场完整会话（选择→切换→缺纸→严格关账），返回覆盖总账。
pub fn audit_sessions() -> TraceAudit {
    let mut audit = TraceAudit::new();
    let mut s = 0usize;
    while s < 10 {
        let mut ring = match TraceRing::new(8) {
            Ok(r) => r,
            Err(_) => break,
        };
        let base = (s as u64) * 100;
        let paper = (s as u16) + 1;
        let ok = ring.record(TracePoint::Select, paper, base + 10).is_ok()
            && ring.record(TracePoint::Switch, paper + 1, base + 20).is_ok()
            && ring.record(TracePoint::OutOfPaper, paper + 1, base + 30).is_ok();
        if ok {
            match ring.close_strict() {
                Ok(cov) => audit.push(cov),
                Err(_) => audit.push(ring.coverage()),
            }
        }
        s += 1;
    }
    audit
}

pub fn run_unx50071_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50071");

    // J1-1 主判据：10 场会话全部三点覆盖，总账 100%。
    let audit = audit_sessions();
    let ok = audit.total() == 10 && audit.covered() == 10 && audit.percent_x100() == 100 && audit.is_full();
    let det = format!(
        "covered={}/{} pct={}",
        audit.covered(),
        audit.total(),
        audit.percent_x100()
    );
    s.add_dyn("50071-J1-埋点全覆盖", ok, &det);

    // J1-2 部分覆盖必被总账识别：9 全 + 1 缺缺纸点 → 90%，且非满覆盖。
    let mut partial = TraceAudit::new();
    let mut i = 0usize;
    while i < 9 {
        let mut r = match TraceRing::new(4) {
            Ok(x) => x,
            Err(_) => break,
        };
        let _ = r.record(TracePoint::Select, 1, 10);
        let _ = r.record(TracePoint::Switch, 2, 20);
        let _ = r.record(TracePoint::OutOfPaper, 2, 30);
        if let Ok(c) = r.close_strict() {
            partial.push(c);
        }
        i += 1;
    }
    let mut short = match TraceRing::new(4) {
        Ok(x) => x,
        Err(_) => return s,
    };
    let _ = short.record(TracePoint::Select, 1, 10);
    let _ = short.record(TracePoint::Switch, 2, 20);
    match short.close_strict() {
        Ok(c) => partial.push(c),
        Err(_) => partial.push(short.coverage()),
    }
    let ok = partial.total() == 10
        && partial.covered() == 9
        && partial.percent_x100() == 90
        && !partial.is_full();
    let det = format!(
        "covered={}/{} pct={}",
        partial.covered(),
        partial.total(),
        partial.percent_x100()
    );
    s.add_dyn("50071-J1-部分覆盖识别", ok, &det);

    // J1-3 覆盖计数：2 选择 + 1 切换 + 1 缺纸，掩码 0b111 且逐点计数正确。
    let mut count = match TraceRing::new(8) {
        Ok(x) => x,
        Err(_) => return s,
    };
    let ok = count.record(TracePoint::Select, 1, 10).is_ok()
        && count.record(TracePoint::Switch, 2, 20).is_ok()
        && count.record(TracePoint::Select, 3, 30).is_ok()
        && count.record(TracePoint::OutOfPaper, 3, 40).is_ok();
    let cov = count.coverage();
    let ok = ok
        && cov.select == 2
        && cov.switch == 1
        && cov.out_of_paper == 1
        && cov.mask == 0b111
        && cov.count(TracePoint::Select) == 2;
    let det = format!(
        "s={} sw={} o={} mask={:03b}",
        cov.select, cov.switch, cov.out_of_paper, cov.mask
    );
    s.add_dyn("50071-J1-覆盖计数", ok, &det);

    // J1-4 序号与时间单调：50 条事件 seq 严格递增、ticks 不回退。
    let mut mono = match TraceRing::new(DEFAULT_CAP) {
        Ok(x) => x,
        Err(_) => return s,
    };
    let mut seq_bad = 0usize;
    let mut k = 0u32;
    while k < 50 {
        let point = match k % 3 {
            0 => TracePoint::Select,
            1 => TracePoint::Switch,
            _ => TracePoint::OutOfPaper,
        };
        let paper = ((k % 5) as u16) + 1;
        match mono.record(point, paper, k as u64 * 2) {
            Ok(seq) => {
                if seq != k {
                    seq_bad += 1;
                }
            }
            Err(_) => seq_bad += 1,
        }
        k += 1;
    }
    let mut order_bad = 0usize;
    let mut prev = 0u64;
    let mut idx = 0usize;
    while idx < mono.len() {
        if let Some(e) = mono.at(idx) {
            if e.ticks < prev {
                order_bad += 1;
            }
            prev = e.ticks;
        }
        idx += 1;
    }
    let ok = mono.len() == 50 && seq_bad == 0 && order_bad == 0;
    let det = format!("len={} seq_bad={} order_bad={}", mono.len(), seq_bad, order_bad);
    s.add_dyn("50071-J1-序号单调", ok, &det);

    // J1-5 时间回退拒绝、时间相等允许（同毫秒双事件是常态）。
    let mut clock = match TraceRing::new(4) {
        Ok(x) => x,
        Err(_) => return s,
    };
    let ok = clock.record(TracePoint::Select, 1, 100).is_ok()
        && clock.record(TracePoint::Switch, 2, 100).is_ok()
        && clock.record(TracePoint::OutOfPaper, 2, 99).err() == Some(TraceError::TimeWarp)
        && clock.len() == 2;
    s.add("50071-J1-时间因果", ok, if ok { "" } else { "时间因果失守" });

    // J1-6 满环拒收：容量 4 写满后第 5 条 RingFull，长度/丢计数不变。
    let mut full = match TraceRing::new(4) {
        Ok(x) => x,
        Err(_) => return s,
    };
    let mut f = 0u64;
    let mut filled = 0usize;
    while f < 4 {
        if full.record(TracePoint::Select, 1, f).is_ok() {
            filled += 1;
        }
        f += 1;
    }
    let ok = filled == 4
        && full.len() == 4
        && full.record(TracePoint::Switch, 2, 10).err() == Some(TraceError::RingFull)
        && full.dropped() == 0;
    let det = format!("filled={} len={} dropped={}", filled, full.len(), full.dropped());
    s.add_dyn("50071-J1-满环拒收", ok, &det);

    // J1-7 丢最旧：容量 3 写 5 条，保留 seq 2..4，dropped=2。
    let mut drop = match TraceRing::new(3) {
        Ok(x) => x,
        Err(_) => return s,
    };
    let mut set_ok = drop.set_policy(1).is_ok();
    let mut d = 0u64;
    while d < 5 {
        if drop.record(TracePoint::Select, 1, d).is_err() {
            set_ok = false;
        }
        d += 1;
    }
    let first_seq = drop.at(0).map(|e| e.seq);
    let last_seq = drop.at(2).map(|e| e.seq);
    let ok = set_ok
        && drop.len() == 3
        && drop.dropped() == 2
        && first_seq == Some(2)
        && last_seq == Some(4);
    let det = format!(
        "len={} dropped={} first={:?} last={:?}",
        drop.len(),
        drop.dropped(),
        first_seq,
        last_seq
    );
    s.add_dyn("50071-J1-丢最旧", ok, &det);

    // J1-8 回放：非空回放确定且含三类标签；空环 ReplayEmpty。
    let mut rep = match TraceRing::new(8) {
        Ok(x) => x,
        Err(_) => return s,
    };
    let _ = rep.record(TracePoint::Select, 7, 100);
    let _ = rep.record(TracePoint::Switch, 9, 200);
    let _ = rep.record(TracePoint::OutOfPaper, 9, 300);
    let r1 = rep.replay();
    let r2 = rep.replay();
    let empty_replay = TraceRing::new(2);
    let ok = match (&r1, &r2, empty_replay) {
        (Ok(a), Ok(b), Ok(e)) => {
            a == b
                && a.contains("select#0@100/7")
                && a.contains("switch#1@200/9")
                && a.contains("out-of-paper#2@300/9")
                && e.replay().err() == Some(TraceError::ReplayEmpty)
        }
        _ => false,
    };
    let det = match r1 {
        Ok(t) => t,
        Err(_) => String::from("err"),
    };
    s.add_dyn("50071-J1-回放确定", ok, &det);

    // J1-9 导入：序号连续可导入；跳号/重号一律 SequenceBreak。
    let mut imp = match TraceRing::new(8) {
        Ok(x) => x,
        Err(_) => return s,
    };
    let good = [
        TraceEvent {
            seq: 0,
            point: TracePoint::Select,
            paper_id: 1,
            ticks: 10,
        },
        TraceEvent {
            seq: 1,
            point: TracePoint::Switch,
            paper_id: 2,
            ticks: 20,
        },
    ];
    let gap = [TraceEvent {
        seq: 5,
        point: TracePoint::OutOfPaper,
        paper_id: 2,
        ticks: 30,
    }];
    let ok = imp.import(&good) == Ok(2)
        && imp.import(&gap).err() == Some(TraceError::SequenceBreak)
        && imp.len() == 2;
    let det = format!("imported={} len={}", 2, imp.len());
    s.add_dyn("50071-J1-导入连续性", ok, &det);

    // J1-10 关账单向：关账后写入/重复关账均 SessionClosed。
    let mut close = match TraceRing::new(8) {
        Ok(x) => x,
        Err(_) => return s,
    };
    let _ = close.record(TracePoint::Select, 1, 10);
    let _ = close.record(TracePoint::Switch, 2, 20);
    let _ = close.record(TracePoint::OutOfPaper, 2, 30);
    let ok = match close.close_strict() {
        Ok(c) => {
            c.is_complete()
                && close.is_closed()
                && close.record(TracePoint::Select, 1, 40).err() == Some(TraceError::SessionClosed)
                && close.close_strict().err() == Some(TraceError::SessionClosed)
        }
        Err(_) => false,
    };
    s.add("50071-J1-关账单向", ok, if ok { "" } else { "关账后可写" });

    // J1-11 摘要确定性与敏感性：同事件同摘要；改纸号/时间即变。
    let mut q1 = match TraceRing::new(4) {
        Ok(x) => x,
        Err(_) => return s,
    };
    let mut q2 = match TraceRing::new(4) {
        Ok(x) => x,
        Err(_) => return s,
    };
    let mut q3 = match TraceRing::new(4) {
        Ok(x) => x,
        Err(_) => return s,
    };
    let _ = q1.record(TracePoint::Select, 1, 10);
    let _ = q2.record(TracePoint::Select, 1, 10);
    let _ = q3.record(TracePoint::Select, 2, 10);
    let ok = q1.digest() == q2.digest() && q1.digest() != q3.digest();
    let det = format!("q1={:016x} q3={:016x}", q1.digest(), q3.digest());
    s.add_dyn("50071-J1-摘要敏感", ok, &det);

    // J1-12 策略/容量/点码闭集越界显式拒绝。
    let ok = TracePolicy::from_code(0) == Ok(TracePolicy::Reject)
        && TracePolicy::from_code(1) == Ok(TracePolicy::DropOldest)
        && TracePolicy::from_code(2).err() == Some(TraceError::BadPolicy)
        && TracePoint::from_code(0).err() == Some(TraceError::BadPoint)
        && TracePoint::from_code(4).err() == Some(TraceError::BadPoint)
        && TraceRing::new(MAX_CAP + 1).err() == Some(TraceError::BadCapacity);
    s.add("50071-J1-闭集解析", ok, if ok { "" } else { "闭集越界未拒" });

    // J1-13 错误码闭集：10 条，码值 = SEG_BASE + 1..=10 且两两不同。
    let mut codes_ok = ALL_ERRORS.len() == 10;
    let mut marks = [false; 11];
    let mut e = 0usize;
    while e < ALL_ERRORS.len() {
        if let Some(err) = ALL_ERRORS.get(e) {
            if err.code() != SEG_BASE + (e as u32) + 1 {
                codes_ok = false;
            }
            let off = (err.code() - SEG_BASE) as usize;
            match marks.get_mut(off) {
                Some(m) => {
                    if *m {
                        codes_ok = false;
                    }
                    *m = true;
                }
                None => codes_ok = false,
            }
        }
        e += 1;
    }
    s.add("50071-J1-错误码闭集", codes_ok, if codes_ok { "" } else { "码值漂移" });

    // J1-14 注入核 7 类全抓。
    let (caught, total) = injection_core();
    let ok = caught == total;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50071-J1-注入必抓", ok, &det);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50071_checks_green() {
        let set = run_unx50071_checks();
        assert!(set.all_passed(), "UNX-F50071 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50071_full_session_is_complete() {
        let mut r = TraceRing::new(8);
        assert!(r.is_ok());
        if let Ok(ring) = r.as_mut() {
            assert!(ring.record(TracePoint::Select, 1, 1).is_ok());
            assert!(ring.record(TracePoint::Switch, 2, 2).is_ok());
            assert!(ring.record(TracePoint::OutOfPaper, 2, 3).is_ok());
            match ring.close_strict() {
                Ok(c) => assert!(c.is_complete()),
                Err(_) => panic!("close failed"),
            }
        }
    }

    #[test]
    fn unx50071_replay_is_stable() {
        let mut r = TraceRing::new(4);
        assert!(r.is_ok());
        if let Ok(ring) = r.as_mut() {
            let _ = ring.record(TracePoint::Select, 3, 9);
            assert_eq!(ring.replay().ok(), ring.replay().ok());
        }
    }
}
