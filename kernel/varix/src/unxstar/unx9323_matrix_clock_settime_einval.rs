//! UNX-F9323 clock_settime 非法时钟 EINVAL 矩阵 · 0x7BE40段
//! ─────────────────────────────────────────────────────────────
//! 判据原文（任务板 F9323 逐字）：
//!   ① 纯功能代码 ≥360 行；② 判据原文：UNX-F9323-J1 非法 clockid 注入判据过，
//!   全 EINVAL 格过，CLOCK_REALTIME 正判据格过，特权检查挂点格过；③ 完成时
//!   API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//!   ① 时钟注册表驱动：十个 clockid 各自带「可设置性 / 所需能力 / 分辨率 /
//!      秒上界」四元组，判定全部走表，判定路径不写死 if 链。
//!   ② 闸门次序与 Linux 逐条对齐，且次序本身就是判据：
//!      G0 负 id→EINVAL（早于用户内存访问）；G1 拷贝 tp→EFAULT；G2 正未知 id
//!      →EINVAL；G3 只读时钟→EINVAL；G4 能力（ALARM 要 CAP_WAKE_ALARM）→EPERM；
//!      G5 timespec 范围→EINVAL；G6 写入者占用→EBUSY；G7 更新寄存器 + 追加日志。
//!      于是「负 id + 坏指针」必须 EINVAL，「正未知 id + 坏指针」必须 EFAULT，
//!      「无能力 + 坏 nsec」必须 EPERM——三条次序互斥，特权挂点无法被参数面掩盖。
//!   ③ 日志是定容环（8 条），只记成功提交；拒绝不记，故 committed 恒等于真实
//!      提交次数（账实相符判据）。
//!   ④ 回写读回：提交后 read 必须立刻等于写入值，prev 必须接续上一笔 next。
//!   ⑤ 注入：14 格 × 10 轮 + 8 范围格 + 4 特权格 + 3 次序格，账本
//!      injected == detected + escaped，escaped 恒 0。
//!   ⑥ 段基址 0x7BE40 超出 u16，诊断码判别值用 u32（码值 = SEG_BASE + 序号）。
//! ─────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 诊断段基址（F9323 独占 0x7BE40..=0x7BE7F）。
pub const SEG_BASE: u32 = 0x7BE40;

/// Linux errno（正数口径）。
pub const EPERM: i32 = 1;
pub const EFAULT: i32 = 14;
pub const EBUSY: i32 = 16;
pub const EINVAL: i32 = 22;

/// 能力位（与 Linux capability 编号同值，方便逐位对账）。
pub const CAP_SYS_TIME: u32 = 1 << 25;
pub const CAP_WAKE_ALARM: u32 = 1 << 22;

/// 时钟 id（Linux uapi）。
pub const CLOCK_REALTIME: i32 = 0;
pub const CLOCK_MONOTONIC: i32 = 1;
pub const CLOCK_PROCESS_CPUTIME_ID: i32 = 2;
pub const CLOCK_THREAD_CPUTIME_ID: i32 = 3;
pub const CLOCK_MONOTONIC_RAW: i32 = 4;
pub const CLOCK_REALTIME_COARSE: i32 = 5;
pub const CLOCK_MONOTONIC_COARSE: i32 = 6;
pub const CLOCK_BOOTTIME: i32 = 7;
pub const CLOCK_REALTIME_ALARM: i32 = 8;
pub const CLOCK_TAI: i32 = 11;

/// REALTIME 可写秒上界（2100-01-01T00:00:00Z，纯整数）。
pub const MAX_REALTIME_SEC: i64 = 4_102_444_800;
/// 纳秒每秒。
pub const NSEC_PER_SEC: i64 = 1_000_000_000;
/// 注册表容量。
pub const MAX_CLOCKS: usize = 10;
/// 写入日志容量（环）。
pub const JOURNAL_CAP: usize = 8;
/// 注入轮数。
pub const INJECT_ROUNDS: u32 = 10;
/// clockid 矩阵行数。
pub const CLOCKID_ROWS: usize = 14;
/// 范围矩阵行数。
pub const RANGE_ROWS: usize = 8;
/// 特权格数。
pub const PRIV_CELLS: usize = 4;
/// 次序格数。
pub const ORDER_CELLS: usize = 3;

/// boot 时刻各时钟初值（下标与注册表同序）。
pub const BOOT_VALUES: [Timespec; MAX_CLOCKS] = [
    Timespec { sec: 1_700_000_000, nsec: 0 }, // REALTIME
    Timespec { sec: 4_100_000, nsec: 0 },     // MONOTONIC
    Timespec { sec: 0, nsec: 0 },             // PROCESS_CPUTIME
    Timespec { sec: 0, nsec: 0 },             // THREAD_CPUTIME
    Timespec { sec: 4_100_000, nsec: 0 },     // MONOTONIC_RAW
    Timespec { sec: 1_700_000_000, nsec: 0 }, // REALTIME_COARSE
    Timespec { sec: 4_100_000, nsec: 0 },     // MONOTONIC_COARSE
    Timespec { sec: 4_101_000, nsec: 0 },     // BOOTTIME
    Timespec { sec: 1_700_000_000, nsec: 0 }, // REALTIME_ALARM
    Timespec { sec: 1_700_000_037, nsec: 0 }, // TAI
];

/// boot 时刻的 REALTIME 值（具名常量，判定路径不用裸索引）。
pub const BOOT_REALTIME: Timespec = Timespec { sec: 1_700_000_000, nsec: 0 };

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（码值 = SEG_BASE + 1 + 变体序号）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u32)]
pub enum SetCode {
    SegmentDrift = SEG_BASE + 1,
    NegativeIdEarly = SEG_BASE + 2,
    CopyFault = SEG_BASE + 3,
    UnknownClock = SEG_BASE + 4,
    ReadOnlyClock = SEG_BASE + 5,
    CpuClockRejected = SEG_BASE + 6,
    AlarmCapMissing = SEG_BASE + 7,
    NsecOutOfRange = SEG_BASE + 8,
    SecOutOfRange = SEG_BASE + 9,
    BusyWriter = SEG_BASE + 10,
    JournalDrift = SEG_BASE + 11,
    ReadbackDrift = SEG_BASE + 12,
}

impl SetCode {
    /// 段内绝对码值。
    pub fn code(self) -> u32 {
        self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            SetCode::SegmentDrift => "段位漂移：诊断码必须落在 0x7BE40..=0x7BE7F 且按 0x40 对齐；请核对施工卡段基址",
            SetCode::NegativeIdEarly => "负 clockid 未早拒：Linux 在拷贝 tp 之前就判 id<0 回 EINVAL；请把负值闸提到最前",
            SetCode::CopyFault => "tp 拷贝失败：空指针或不可读用户页回 EFAULT；调用方应传入合法 timespec 地址",
            SetCode::UnknownClock => "未知 clockid：注册表内没有该 id；请用 CLOCK_REALTIME/CLOCK_TAI 等合法值",
            SetCode::ReadOnlyClock => "只读时钟被写：MONOTONIC 族与 COARSE 不可设置；请改用 CLOCK_REALTIME",
            SetCode::CpuClockRejected => "CPU 时钟被写：进程/线程 CPU 时钟由内核记账；不得用 clock_settime 篡改",
            SetCode::AlarmCapMissing => "唤醒闹钟缺能力：CLOCK_REALTIME_ALARM 需要 CAP_WAKE_ALARM；请补权或换普通时钟",
            SetCode::NsecOutOfRange => "纳秒越界：tv_nsec 必须落在 [0, 1e9)；请先规范化再做进位",
            SetCode::SecOutOfRange => "秒越界：tv_sec 必须落在 [0, 2100-01-01]；请改用合法的绝对时刻",
            SetCode::BusyWriter => "写入者占用：同一时钟已有并发 setter，回 EBUSY；请重试或串行化写入",
            SetCode::JournalDrift => "日志漂移：提交条数与寄存器变化次数不一致；请检查拒绝路径是否误记",
            SetCode::ReadbackDrift => "读回漂移：提交后 read 与写入值不等；请检查提交是否被中间闸短路",
        }
    }

    /// 闭集全量。
    pub const ALL: &'static [SetCode] = &[
        SetCode::SegmentDrift, SetCode::NegativeIdEarly, SetCode::CopyFault,
        SetCode::UnknownClock, SetCode::ReadOnlyClock, SetCode::CpuClockRejected,
        SetCode::AlarmCapMissing, SetCode::NsecOutOfRange, SetCode::SecOutOfRange,
        SetCode::BusyWriter, SetCode::JournalDrift, SetCode::ReadbackDrift,
    ];
}

// ── 时钟注册表 ────────────────────────────────────────────────────────
/// 写入所需能力类别。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CapKind {
    None,
    SysTime,
    WakeAlarm,
}

impl CapKind {
    /// 闭集编码（None=0 / SysTime=1 / WakeAlarm=2）。
    pub fn code(self) -> u32 {
        self as u32
    }
}

/// 一个时钟的四元组描述。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ClockDesc {
    pub id: i32, pub name: &'static str, pub settable: bool,
    pub cap: CapKind, pub resolution_ns: u32, pub max_sec: i64,
}

/// 冻结注册表（下标即存储槽位）。
pub const CLOCKS: &'static [ClockDesc] = &[
    ClockDesc { id: CLOCK_REALTIME, name: "REALTIME", settable: true, cap: CapKind::SysTime, resolution_ns: 1, max_sec: MAX_REALTIME_SEC },
    ClockDesc { id: CLOCK_MONOTONIC, name: "MONOTONIC", settable: false, cap: CapKind::None, resolution_ns: 1, max_sec: 0 },
    ClockDesc { id: CLOCK_PROCESS_CPUTIME_ID, name: "PROCESS_CPUTIME_ID", settable: false, cap: CapKind::None, resolution_ns: 1, max_sec: 0 },
    ClockDesc { id: CLOCK_THREAD_CPUTIME_ID, name: "THREAD_CPUTIME_ID", settable: false, cap: CapKind::None, resolution_ns: 1, max_sec: 0 },
    ClockDesc { id: CLOCK_MONOTONIC_RAW, name: "MONOTONIC_RAW", settable: false, cap: CapKind::None, resolution_ns: 1, max_sec: 0 },
    ClockDesc { id: CLOCK_REALTIME_COARSE, name: "REALTIME_COARSE", settable: false, cap: CapKind::None, resolution_ns: 4_000_000, max_sec: 0 },
    ClockDesc { id: CLOCK_MONOTONIC_COARSE, name: "MONOTONIC_COARSE", settable: false, cap: CapKind::None, resolution_ns: 4_000_000, max_sec: 0 },
    ClockDesc { id: CLOCK_BOOTTIME, name: "BOOTTIME", settable: false, cap: CapKind::None, resolution_ns: 1, max_sec: 0 },
    ClockDesc { id: CLOCK_REALTIME_ALARM, name: "REALTIME_ALARM", settable: true, cap: CapKind::WakeAlarm, resolution_ns: 1, max_sec: MAX_REALTIME_SEC },
    ClockDesc { id: CLOCK_TAI, name: "TAI", settable: true, cap: CapKind::SysTime, resolution_ns: 1, max_sec: MAX_REALTIME_SEC },
];

/// clockid → 注册表下标。
pub fn index_of(id: i32) -> Option<usize> {
    let mut i = 0usize;
    while i < CLOCKS.len() {
        if let Some(d) = CLOCKS.get(i) {
            if d.id == id {
                return Some(i);
            }
        }
        i += 1;
    }
    None
}

/// 调用者能力集合。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CapSet { pub bits: u32 }

impl CapSet {
    pub const fn none() -> CapSet { CapSet { bits: 0 } }
    pub const fn sys_time() -> CapSet { CapSet { bits: CAP_SYS_TIME } }
    pub const fn full() -> CapSet { CapSet { bits: CAP_SYS_TIME | CAP_WAKE_ALARM } }
    /// 是否具备该类能力。
    pub fn allows(&self, cap: CapKind) -> bool {
        match cap {
            CapKind::None => true,
            CapKind::SysTime => self.bits & CAP_SYS_TIME != 0,
            CapKind::WakeAlarm => self.bits & CAP_WAKE_ALARM != 0,
        }
    }
}

/// 规范化时间值（nsec 用 i64 以便注入负值）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Timespec { pub sec: i64, pub nsec: i64 }

impl Timespec {
    /// 规范化判据：[0,1e9) 的纳秒 + 非负秒（范围闸的唯一口径）。
    pub fn normalized(&self) -> bool {
        self.nsec >= 0 && self.nsec < NSEC_PER_SEC && self.sec >= 0
    }
}

/// 用户 timespec 的可读性（拷贝模型）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum UserTs {
    /// 空指针。
    Absent,
    /// 不可读页。
    Unreadable,
    /// 可读，携带值。
    Read(Timespec),
}

/// 一次 clock_settime 请求。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SetRequest { pub clockid: i32, pub ts: UserTs }

impl SetRequest {
    pub const fn new(clockid: i32, ts: UserTs) -> SetRequest {
        SetRequest { clockid, ts }
    }
}

/// 时钟寄存器堆（按注册表下标）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ClockStore { values: [Timespec; MAX_CLOCKS] }

impl ClockStore {
    pub const fn boot() -> ClockStore { ClockStore { values: BOOT_VALUES } }
    /// 按下标回写。
    pub fn write_at(&mut self, idx: usize, v: Timespec) -> Result<(), i32> {
        match self.values.get_mut(idx) {
            Some(slot) => { *slot = v; Ok(()) }
            None => Err(EINVAL),
        }
    }
    /// 按下标读取。
    pub fn read_at(&self, idx: usize) -> Option<Timespec> { self.values.get(idx).copied() }
    /// 按 clockid 读取。
    pub fn read(&self, clockid: i32) -> Option<Timespec> {
        match index_of(clockid) {
            Some(i) => self.read_at(i),
            None => None,
        }
    }
}

// ── 写入日志（定容环） ────────────────────────────────────────────────
/// 一条成功提交。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct JournalEntry {
    pub seq: u32, pub clockid: i32,
    pub prev_sec: i64, pub prev_nsec: i64,
    pub next_sec: i64, pub next_nsec: i64,
}

/// 定容环 + 单调计数。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ClockJournal {
    entries: [Option<JournalEntry>; JOURNAL_CAP], ring: usize,
    pub seq: u32, pub committed: u32, pub rejected: u32,
    pub busy_hits: u32, pub busy: bool,
}

impl ClockJournal {
    pub const fn new() -> ClockJournal {
        ClockJournal {
            entries: [None; JOURNAL_CAP], ring: 0,
            seq: 0, committed: 0, rejected: 0, busy_hits: 0, busy: false,
        }
    }
    /// 追加一条（覆盖最旧），返回新序号。
    pub fn push(&mut self, mut e: JournalEntry) -> u32 {
        self.seq = self.seq.saturating_add(1);
        e.seq = self.seq;
        if let Some(slot) = self.entries.get_mut(self.ring) {
            *slot = Some(e);
        }
        self.ring = (self.ring + 1) % JOURNAL_CAP;
        self.committed = self.committed.saturating_add(1);
        self.seq
    }
    /// 取日志视图（环序，非时间序）。
    pub fn slot(&self, i: usize) -> Option<JournalEntry> {
        match self.entries.get(i) {
            Some(Some(e)) => Some(*e),
            _ => None,
        }
    }
    /// 最近一条。
    pub fn last(&self) -> Option<JournalEntry> {
        let i = (self.ring + JOURNAL_CAP - 1) % JOURNAL_CAP;
        self.slot(i)
    }
}

// ── 判定主路径 ────────────────────────────────────────────────────────
/// 闸门序号（诊断与次序判据共用）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Gate {
    NegativeId,
    Copy,
    UnknownId,
    ReadOnly,
    Capability,
    Range,
    Busy,
    Commit,
}

impl Gate {
    /// 闸门序号（0..=7，与 G0..G7 一一对应）。
    pub fn code(self) -> u32 {
        self as u32
    }
}

/// 判定结果：成功带序号，失败带 errno（拒绝一律记账）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Outcome { pub errno: i32, pub gate: Option<Gate>, pub seq: u32 }

impl Outcome {
    pub const fn ok(seq: u32) -> Outcome { Outcome { errno: 0, gate: Some(Gate::Commit), seq } }
    pub const fn err(errno: i32, gate: Gate) -> Outcome { Outcome { errno, gate: Some(gate), seq: 0 } }
}

/// clock_settime 主判定：G0..G7 冻结次序。
pub fn clock_settime(
    req: &SetRequest,
    cred: CapSet,
    store: &mut ClockStore,
    journal: &mut ClockJournal,
) -> Outcome {
    // G0：负 clockid 早于用户内存访问（Linux 同序）。
    if req.clockid < 0 {
        journal.rejected = journal.rejected.saturating_add(1);
        return Outcome::err(EINVAL, Gate::NegativeId);
    }
    // G1：拷贝 tp。
    let ts = match req.ts {
        UserTs::Read(v) => v,
        UserTs::Absent | UserTs::Unreadable => {
            journal.rejected = journal.rejected.saturating_add(1);
            return Outcome::err(EFAULT, Gate::Copy);
        }
    };
    // G2：正未知 id。
    let idx = match index_of(req.clockid) {
        Some(i) => i,
        None => {
            journal.rejected = journal.rejected.saturating_add(1);
            return Outcome::err(EINVAL, Gate::UnknownId);
        }
    };
    let desc = match CLOCKS.get(idx).copied() {
        Some(d) => d,
        None => {
            journal.rejected = journal.rejected.saturating_add(1);
            return Outcome::err(EINVAL, Gate::UnknownId);
        }
    };
    // G3：只读时钟。
    if !desc.settable {
        journal.rejected = journal.rejected.saturating_add(1);
        return Outcome::err(EINVAL, Gate::ReadOnly);
    }
    // G4：能力（早于范围校验，故无能力者拿不到 EINVAL 泄漏）。
    if !cred.allows(desc.cap) {
        journal.rejected = journal.rejected.saturating_add(1);
        return Outcome::err(EPERM, Gate::Capability);
    }
    // G5：范围。
    if ts.nsec < 0 || ts.nsec >= NSEC_PER_SEC || ts.sec < 0 || ts.sec > desc.max_sec {
        journal.rejected = journal.rejected.saturating_add(1);
        return Outcome::err(EINVAL, Gate::Range);
    }
    // G6：并发写入者。
    if journal.busy {
        journal.busy_hits = journal.busy_hits.saturating_add(1);
        return Outcome::err(EBUSY, Gate::Busy);
    }
    // G7：提交。
    let prev = match store.read_at(idx) {
        Some(v) => v,
        None => {
            journal.rejected = journal.rejected.saturating_add(1);
            return Outcome::err(EINVAL, Gate::UnknownId);
        }
    };
    let next = Timespec { sec: ts.sec, nsec: ts.nsec };
    if store.write_at(idx, next).is_err() {
        journal.rejected = journal.rejected.saturating_add(1);
        return Outcome::err(EINVAL, Gate::UnknownId);
    }
    let seq = journal.push(JournalEntry {
        seq: 0,
        clockid: req.clockid,
        prev_sec: prev.sec,
        prev_nsec: prev.nsec,
        next_sec: next.sec,
        next_nsec: next.nsec,
    });
    Outcome::ok(seq)
}

// ── 注入账本 ──────────────────────────────────────────────────────────
/// 注入 = 检出 + 逃逸，逃逸恒 0。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectLedger { pub injected: u32, pub detected: u32, pub escaped: u32 }

impl InjectLedger {
    pub const fn zero() -> InjectLedger {
        InjectLedger { injected: 0, detected: 0, escaped: 0 }
    }
    pub fn record(&mut self, caught: bool) -> Result<(), i32> {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(EINVAL);
        }
        Ok(())
    }
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

// ── clockid 矩阵 ──────────────────────────────────────────────────────
/// 冻结行：clockid + 期望 errno（0 = 成功）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct IdRow { pub clockid: i32, pub expect: i32, pub label: &'static str }

/// 14 行：2 成功、11 EINVAL、1 EPERM。
pub const ID_ROWS: &'static [IdRow] = &[
    IdRow { clockid: -1, expect: EINVAL, label: "负 id 早拒" },
    IdRow { clockid: -6, expect: EINVAL, label: "动态负 id" },
    IdRow { clockid: CLOCK_REALTIME, expect: 0, label: "REALTIME 正判据" },
    IdRow { clockid: CLOCK_MONOTONIC, expect: EINVAL, label: "MONOTONIC 只读" },
    IdRow { clockid: CLOCK_PROCESS_CPUTIME_ID, expect: EINVAL, label: "进程 CPU 钟" },
    IdRow { clockid: CLOCK_THREAD_CPUTIME_ID, expect: EINVAL, label: "线程 CPU 钟" },
    IdRow { clockid: CLOCK_MONOTONIC_RAW, expect: EINVAL, label: "MONOTONIC_RAW 只读" },
    IdRow { clockid: CLOCK_REALTIME_COARSE, expect: EINVAL, label: "REALTIME_COARSE 只读" },
    IdRow { clockid: CLOCK_MONOTONIC_COARSE, expect: EINVAL, label: "MONOTONIC_COARSE 只读" },
    IdRow { clockid: CLOCK_BOOTTIME, expect: EINVAL, label: "BOOTTIME 只读" },
    IdRow { clockid: CLOCK_REALTIME_ALARM, expect: EPERM, label: "闹钟缺 WAKE_ALARM" },
    IdRow { clockid: CLOCK_TAI, expect: 0, label: "TAI 正判据" },
    IdRow { clockid: 99, expect: EINVAL, label: "未知正 id" },
    IdRow { clockid: 0x7FFF_FFFF, expect: EINVAL, label: "未知超大 id" },
];

/// 矩阵结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct IdMatrix {
    pub per_row: [u32; CLOCKID_ROWS], pub einval_rows: u32,
    pub ok_rows: u32, pub ledger: InjectLedger, pub ledger_ok: bool,
}

/// 合法写入值（矩阵统一用 1_800_000_000.123456789）。
pub const PROBE_TS: Timespec = Timespec { sec: 1_800_000_000, nsec: 123_456_789 };

/// 14 行 × 10 轮：每行独立寄存器堆与日志。
pub fn run_clockid_matrix() -> IdMatrix {
    let mut out = IdMatrix {
        per_row: [0u32; CLOCKID_ROWS],
        einval_rows: 0,
        ok_rows: 0,
        ledger: InjectLedger::zero(),
        ledger_ok: true,
    };
    let mut ri = 0usize;
    while ri < CLOCKID_ROWS {
        let row = match ID_ROWS.get(ri).copied() {
            Some(r) => r,
            None => break,
        };
        let mut round = 0u32;
        let mut all_einval = true;
        let mut all_ok = true;
        while round < INJECT_ROUNDS {
            let mut store = ClockStore::boot();
            let mut journal = ClockJournal::new();
            let req = SetRequest::new(row.clockid, UserTs::Read(PROBE_TS));
            let got = clock_settime(&req, CapSet::sys_time(), &mut store, &mut journal);
            let want_ok = row.expect == 0;
            let mut caught = if want_ok {
                got.errno == 0 && got.seq == 1 && journal.committed == 1
            } else {
                got.errno == row.expect && got.seq == 0 && journal.committed == 0
            };
            if want_ok {
                // 回写读回：寄存器必须等于写入值，日志 prev 必须等于旧值。
                let back = store.read(row.clockid);
                let ok_back = back == Some(PROBE_TS);
                let ok_prev = match journal.last() {
                    Some(e) => e.prev_sec == 1_700_000_000
                        || (row.clockid == CLOCK_TAI && e.prev_sec == 1_700_000_037),
                    None => false,
                };
                if !ok_back || !ok_prev || journal.rejected != 0 {
                    caught = false;
                    all_ok = false;
                }
            } else {
                if got.errno != row.expect || journal.rejected != 1 {
                    all_einval = false;
                }
            }
            if caught {
                if let Some(slot) = out.per_row.get_mut(ri) {
                    *slot = slot.saturating_add(1);
                }
            }
            if out.ledger.record(caught).is_err() {
                out.ledger_ok = false;
            }
            round += 1;
        }
        if row.expect == EINVAL && all_einval {
            out.einval_rows = out.einval_rows.saturating_add(1);
        }
        if row.expect == 0 && all_ok {
            out.ok_rows = out.ok_rows.saturating_add(1);
        }
        ri += 1;
    }
    out
}

// ── 范围矩阵 ──────────────────────────────────────────────────────────
/// 冻结范围行：sec, nsec, 期望成功。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RangeRow { pub sec: i64, pub nsec: i64, pub ok: bool, pub label: &'static str }

/// 8 行：3 成功、5 EINVAL（含负 nsec、1e9 边界、i64 极大值、负秒、超上界）。
pub const RANGE_ROWS_TABLE: &'static [RangeRow] = &[
    RangeRow { sec: 0, nsec: 0, ok: true, label: "纪元零点" },
    RangeRow { sec: 1_700_000_000, nsec: 999_999_999, ok: true, label: "纳秒上沿" },
    RangeRow { sec: 1_700_000_000, nsec: 1_000_000_000, ok: false, label: "纳秒恰越界" },
    RangeRow { sec: 1_700_000_000, nsec: -1, ok: false, label: "负纳秒" },
    RangeRow { sec: 1_700_000_000, nsec: i64::MAX, ok: false, label: "纳秒 i64 极大" },
    RangeRow { sec: -1, nsec: 0, ok: false, label: "负秒" },
    RangeRow { sec: MAX_REALTIME_SEC, nsec: 0, ok: true, label: "秒上界贴合" },
    RangeRow { sec: MAX_REALTIME_SEC + 1, nsec: 0, ok: false, label: "秒上界越界" },
];

/// 范围矩阵结果：成功行数、EINVAL 行数。
pub fn run_range_matrix() -> (u32, u32) {
    let mut ok_rows = 0u32;
    let mut einval_rows = 0u32;
    let mut i = 0usize;
    while i < RANGE_ROWS {
        let row = match RANGE_ROWS_TABLE.get(i).copied() {
            Some(r) => r,
            None => break,
        };
        let mut store = ClockStore::boot();
        let mut journal = ClockJournal::new();
        let req = SetRequest::new(CLOCK_REALTIME, UserTs::Read(Timespec { sec: row.sec, nsec: row.nsec }));
        let got = clock_settime(&req, CapSet::sys_time(), &mut store, &mut journal);
        if row.ok && got.errno == 0 && store.read(CLOCK_REALTIME) == Some(Timespec { sec: row.sec, nsec: row.nsec }) {
            ok_rows = ok_rows.saturating_add(1);
        } else if !row.ok && got.errno == EINVAL && journal.committed == 0 {
            einval_rows = einval_rows.saturating_add(1);
        }
        i += 1;
    }
    (ok_rows, einval_rows)
}

// ── 特权挂点 ──────────────────────────────────────────────────────────
/// 特权格结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PrivResult { pub eperm: u32, pub einval: u32, pub ok: u32, pub total: u32 }

/// 单发一次带新寄存器堆的写入（矩阵共用，避免每格重复样板）。
pub fn probe_set(id: i32, ts: Timespec, cred: CapSet) -> (Outcome, ClockStore, ClockJournal) {
    let mut store = ClockStore::boot();
    let mut journal = ClockJournal::new();
    let got = clock_settime(&SetRequest::new(id, UserTs::Read(ts)), cred, &mut store, &mut journal);
    (got, store, journal)
}

/// 四格：无权普通钟 / 无权且坏 nsec（次序）/ 有权只读钟（次序）/ 有权闹钟。
pub fn run_privilege_matrix() -> PrivResult {
    let mut r = PrivResult { eperm: 0, einval: 0, ok: 0, total: 0 };
    // ① 无能力 + REALTIME + 合法值 → EPERM。
    if probe_set(CLOCK_REALTIME, PROBE_TS, CapSet::none()).0.errno == EPERM {
        r.eperm = r.eperm.saturating_add(1);
    }
    r.total = r.total.saturating_add(1);
    // ② 无能力 + 坏 nsec → 仍是 EPERM（特权先于范围）。
    let bad = Timespec { sec: 0, nsec: 2_000_000_000 };
    if probe_set(CLOCK_REALTIME, bad, CapSet::none()).0.errno == EPERM {
        r.eperm = r.eperm.saturating_add(1);
    }
    r.total = r.total.saturating_add(1);
    // ③ 有 SYS_TIME 但写 MONOTONIC → EINVAL（只读闸先于能力闸）。
    if probe_set(CLOCK_MONOTONIC, PROBE_TS, CapSet::sys_time()).0.errno == EINVAL {
        r.einval = r.einval.saturating_add(1);
    }
    r.total = r.total.saturating_add(1);
    // ④ 双能力 + REALTIME_ALARM → 成功且可读回。
    let (got, store, _) = probe_set(CLOCK_REALTIME_ALARM, PROBE_TS, CapSet::full());
    if got.errno == 0 && store.read(CLOCK_REALTIME_ALARM) == Some(PROBE_TS) {
        r.ok = r.ok.saturating_add(1);
    }
    r.total = r.total.saturating_add(1);
    r
}

// ── 次序格 ────────────────────────────────────────────────────────────
/// 三格：负 id 先 EINVAL、正未知 id 先 EFAULT、空指针 EFAULT。
pub fn run_order_matrix() -> (u32, u32) {
    let mut matched = 0u32;
    // ① 负 id + 空指针 → EINVAL。
    let (a, _, _) = probe_user(SetRequest::new(-1, UserTs::Absent), CapSet::sys_time());
    if a.errno == EINVAL && a.gate == Some(Gate::NegativeId) {
        matched = matched.saturating_add(1);
    }
    // ② 正未知 id + 空指针 → EFAULT（拷贝先于未知 id）。
    let (b, _, _) = probe_user(SetRequest::new(99, UserTs::Absent), CapSet::sys_time());
    if b.errno == EFAULT && b.gate == Some(Gate::Copy) {
        matched = matched.saturating_add(1);
    }
    // ③ 合法 id + 不可读页 → EFAULT 且寄存器不变。
    let (c, store, _) = probe_user(SetRequest::new(CLOCK_REALTIME, UserTs::Unreadable), CapSet::sys_time());
    if c.errno == EFAULT && c.gate == Some(Gate::Copy) && store.read(CLOCK_REALTIME) == Some(BOOT_REALTIME) {
        matched = matched.saturating_add(1);
    }
    (matched, ORDER_CELLS as u32)
}

/// 按原始 UserTs 单发一次（次序格用，需要区分 Absent/Unreadable）。
pub fn probe_user(req: SetRequest, cred: CapSet) -> (Outcome, ClockStore, ClockJournal) {
    let mut store = ClockStore::boot();
    let mut journal = ClockJournal::new();
    let got = clock_settime(&req, cred, &mut store, &mut journal);
    (got, store, journal)
}

// ── 确定性指纹 ────────────────────────────────────────────────────────
/// FNV-1a 32 位。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            h ^= *b as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        i += 1;
    }
    h
}

/// 规范语料：14 个请求按序打在同一条寄存器堆/日志上。
pub fn corpus_fingerprint() -> u32 {
    let mut store = ClockStore::boot();
    let mut journal = ClockJournal::new();
    let mut bytes: Vec<u8> = Vec::new();
    let mut i = 0usize;
    while i < ID_ROWS.len() {
        if let Some(row) = ID_ROWS.get(i).copied() {
            let req = SetRequest::new(row.clockid, UserTs::Read(PROBE_TS));
            let got = clock_settime(&req, CapSet::full(), &mut store, &mut journal);
            bytes.push((got.errno & 0x7F) as u8);
            bytes.push((got.seq & 0xFF) as u8);
            if let Some(v) = store.read(CLOCK_REALTIME) {
                bytes.push((v.sec & 0xFF) as u8);
                bytes.push((v.nsec & 0xFF) as u8);
            }
        }
        i += 1;
    }
    bytes.push((journal.committed & 0xFF) as u8);
    bytes.push((journal.rejected & 0xFF) as u8);
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9323 判据集（静态 15 条 + 动态 3 条）。
pub fn run_unx9323_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9323");

    let seg_ok = SEG_BASE == 0x7BE40 && SEG_BASE % 0x40 == 0 && (0x7BE40..=0x7BE7F).contains(&SEG_BASE);
    s.add("段位守卫：0x7BE40 起 0x40 对齐且独占窗口", seg_ok, "段基址必须与施工卡一致，诊断码不得越窗");

    // 注册表自洽：10 项、id 唯一、可设置恰 3 项。
    let mut reg_ok = CLOCKS.len() == MAX_CLOCKS;
    let mut settable = 0u32;
    let mut i = 0usize;
    while i < CLOCKS.len() {
        match CLOCKS.get(i) {
            Some(d) => {
                if d.settable {
                    settable = settable.saturating_add(1);
                }
                if index_of(d.id) != Some(i) {
                    reg_ok = false;
                }
            }
            None => reg_ok = false,
        }
        i += 1;
    }
    s.add("注册表：十项 id 唯一且可设置恰三项", reg_ok && settable == 3, "可设置集合必须是 {REALTIME, REALTIME_ALARM, TAI}");

    // clockid 矩阵 14 行 × 10 轮。
    let m = run_clockid_matrix();
    let per_row_ok = m.per_row.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("注入：14 个 clockid × 10 轮逐格 10/10 检出", per_row_ok, "任一行掉轮即说明闸门次序与冻结表分叉");

    s.add("全 EINVAL 格：11 个 EINVAL 行逐轮全中", m.einval_rows == 11, "只读时钟与未知 id 必须一律 EINVAL，不得泄漏 EPERM");

    s.add("CLOCK_REALTIME 正判据：提交序号 1 且写回读回一致", m.ok_rows == 2, "REALTIME 与 TAI 两个成功行都必须落盘且读回相等");

    // 范围矩阵。
    let (range_ok, range_einval) = run_range_matrix();
    s.add("timespec 范围：8 格中 3 成功 5 EINVAL", range_ok == 3 && range_einval == 5, "纳秒 [0,1e9) 与秒 [0,2100] 两端都必须钉住");

    // 特权挂点。
    let p = run_privilege_matrix();
    s.add(
        "特权挂点：缺 CAP 一律 EPERM 且先于范围校验",
        p.eperm == 2 && p.einval == 1 && p.ok == 1 && p.total == PRIV_CELLS as u32,
        "能力闸必须早于范围闸，否则无权限者可探测 timespec 合法性",
    );
    // 次序格。
    let (order_hit, order_total) = run_order_matrix();
    s.add("次序：负 id 先于 EFAULT、正未知 id 后于 EFAULT", order_hit == order_total && order_total == ORDER_CELLS as u32, "三条次序格必须同时成立，次序不可交换");

    // 账本守恒。
    let ledger_ok = m.ledger_ok
        && m.ledger.conserved()
        && m.ledger.escaped == 0
        && m.ledger.injected == CLOCKID_ROWS as u32 * INJECT_ROUNDS;
    s.add("注入账：140 次注入全检出、逃逸为 0", ledger_ok, "injected == detected + escaped，且 escaped 必须为 0");

    // 日志账实相符：只有成功提交入账。
    let mut jn = ClockJournal::new();
    let mut st = ClockStore::boot();
    let r1 = clock_settime(&SetRequest::new(CLOCK_REALTIME, UserTs::Read(PROBE_TS)), CapSet::sys_time(), &mut st, &mut jn);
    let r2 = clock_settime(&SetRequest::new(CLOCK_MONOTONIC, UserTs::Read(PROBE_TS)), CapSet::sys_time(), &mut st, &mut jn);
    let r3 = clock_settime(&SetRequest::new(CLOCK_REALTIME, UserTs::Absent), CapSet::sys_time(), &mut st, &mut jn);
    s.add(
        "日志：仅成功提交入账，拒绝不占日志",
        r1.seq == 1 && r2.errno == EINVAL && r3.errno == EFAULT && jn.committed == 1
            && jn.rejected == 2 && jn.slot(0).map(|e| e.seq) == Some(1),
        "拒绝路径误记会让日志条数虚高，账实分离即红",
    );

    // 写回单调：两次成功写入序号递增、prev 接续。
    let mut jn2 = ClockJournal::new();
    let mut st2 = ClockStore::boot();
    let a1 = clock_settime(&SetRequest::new(CLOCK_REALTIME, UserTs::Read(Timespec { sec: 1_800_000_000, nsec: 1 })), CapSet::sys_time(), &mut st2, &mut jn2);
    let a2 = clock_settime(&SetRequest::new(CLOCK_REALTIME, UserTs::Read(Timespec { sec: 1_800_000_001, nsec: 2 })), CapSet::sys_time(), &mut st2, &mut jn2);
    let prev_ok = match jn2.last() {
        Some(e) => e.prev_sec == 1_800_000_000 && e.next_sec == 1_800_000_001,
        None => false,
    };
    s.add(
        "写回链：连续两次提交序号递增且 prev 接续",
        a1.seq == 1 && a2.seq == 2 && prev_ok && jn2.committed == 2,
        "日志 prev 必须等于上一笔 next，否则时间线断裂",
    );

    // 忙写：EBUSY 且寄存器不变。
    let mut jn3 = ClockJournal::new();
    jn3.busy = true;
    let mut st3 = ClockStore::boot();
    let busy = clock_settime(&SetRequest::new(CLOCK_REALTIME, UserTs::Read(PROBE_TS)), CapSet::sys_time(), &mut st3, &mut jn3);
    s.add(
        "忙写：EBUSY 且寄存器与日志均不变",
        busy.errno == EBUSY && jn3.committed == 0 && jn3.busy_hits == 1 && st3.read(CLOCK_REALTIME) == Some(BOOT_REALTIME),
        "并发写入者占用时不得半提交，回 EBUSY 由调用方重试",
    );

    // 双跑指纹。
    let fp_a = corpus_fingerprint();
    let fp_b = corpus_fingerprint();
    s.add("双跑一致：共享寄存器堆语料指纹相等", fp_a == fp_b && fp_a != 0, "同输入两次必须逐字节相等，隐藏状态即失败");

    // 诊断码闭集：文案非空、码值在窗口内、两两唯一。
    let mut text_ok = SetCode::ALL.len() == 12;
    let mut a = 0usize;
    while a < SetCode::ALL.len() {
        match SetCode::ALL.get(a) {
            Some(x) => {
                if x.detail().is_empty() || x.code() < SEG_BASE + 1 || x.code() > SEG_BASE + 12 {
                    text_ok = false;
                }
                let mut b = a + 1;
                while b < SetCode::ALL.len() {
                    if let Some(y) = SetCode::ALL.get(b) {
                        if x.code() == y.code() {
                            text_ok = false;
                        }
                    }
                    b += 1;
                }
            }
            None => text_ok = false,
        }
        a += 1;
    }
    s.add("诊断码：12 个码值两两唯一且文案非空", text_ok, "段内闭集码不得重复，否则判据无法定位");

    let id_text = format!(
        "clockid 矩阵 注入 {} / 检出 {} / 逃逸 {}；EINVAL 行 {} / 成功行 {}",
        m.ledger.injected, m.ledger.detected, m.ledger.escaped, m.einval_rows, m.ok_rows
    );
    s.add_dyn("clockid 矩阵快照", per_row_ok && ledger_ok, &id_text);
    let range_text = format!("范围 成功 {} / EINVAL {}；特权 EPERM {} EINVAL {} OK {}", range_ok, range_einval, p.eperm, p.einval, p.ok);
    s.add_dyn("范围与特权快照", range_ok == 3 && p.eperm == 2, &range_text);
    let fp_text = format!("语料指纹 0x{:08X}", fp_a);
    s.add_dyn("指纹快照", fp_a == fp_b, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_and_order_precedence() {
        assert_eq!(CLOCKS.len(), MAX_CLOCKS);
        assert_eq!(index_of(CLOCK_REALTIME), Some(0));
        assert_eq!(index_of(CLOCK_TAI), Some(9));
        assert_eq!(index_of(99), None);
        assert_eq!(index_of(-1), None);
        let (neg, _, jn_neg) = probe_user(SetRequest::new(-1, UserTs::Absent), CapSet::sys_time());
        assert_eq!(neg.errno, EINVAL);
        assert_eq!(neg.gate, Some(Gate::NegativeId));
        assert_eq!(jn_neg.rejected, 1);
        let (unk, _, _) = probe_user(SetRequest::new(99, UserTs::Absent), CapSet::sys_time());
        assert_eq!(unk.errno, EFAULT);
        assert_eq!(unk.gate, Some(Gate::Copy));
        let (unk2, _, _) = probe_user(SetRequest::new(99, UserTs::Read(PROBE_TS)), CapSet::sys_time());
        assert_eq!(unk2.errno, EINVAL);
        assert_eq!(unk2.gate, Some(Gate::UnknownId));
    }

    #[test]
    fn realtime_commit_and_readback() {
        let mut st = ClockStore::boot();
        let mut jn = ClockJournal::new();
        let got = clock_settime(&SetRequest::new(CLOCK_REALTIME, UserTs::Read(PROBE_TS)), CapSet::sys_time(), &mut st, &mut jn);
        assert_eq!(got.errno, 0);
        assert_eq!(got.seq, 1);
        assert_eq!(st.read(CLOCK_REALTIME), Some(PROBE_TS));
        assert_eq!(jn.committed, 1);
        assert_eq!(jn.last().map(|e| (e.prev_sec, e.next_sec)), Some((1_700_000_000, PROBE_TS.sec)));
    }

    #[test]
    fn read_only_and_cpu_clocks_reject() {
        let clocks: [i32; 6] = [CLOCK_MONOTONIC, CLOCK_PROCESS_CPUTIME_ID, CLOCK_THREAD_CPUTIME_ID, CLOCK_MONOTONIC_RAW, CLOCK_REALTIME_COARSE, CLOCK_BOOTTIME];
        let mut i = 0usize;
        while i < clocks.len() {
            if let Some(id) = clocks.get(i).copied() {
                let (got, _, jn) = probe_set(id, PROBE_TS, CapSet::full());
                assert_eq!(got.errno, EINVAL);
                assert_eq!(got.gate, Some(Gate::ReadOnly));
                assert_eq!(jn.committed, 0);
            }
            i += 1;
        }
    }

    #[test]
    fn capability_precedes_range() {
        let bad = Timespec { sec: 0, nsec: 2_000_000_000 };
        let (got, _, _) = probe_set(CLOCK_REALTIME, bad, CapSet::none());
        assert_eq!(got.errno, EPERM);
        assert_eq!(got.gate, Some(Gate::Capability));
        assert_eq!(probe_set(CLOCK_REALTIME_ALARM, PROBE_TS, CapSet::sys_time()).0.errno, EPERM);
        assert_eq!(probe_set(CLOCK_REALTIME_ALARM, PROBE_TS, CapSet::full()).0.errno, 0);
    }

    #[test]
    fn range_and_clockid_matrices() {
        let (ok, einval) = run_range_matrix();
        assert_eq!(ok, 3);
        assert_eq!(einval, 5);
        let edge = probe_set(0, Timespec { sec: MAX_REALTIME_SEC + 1, nsec: 0 }, CapSet::sys_time());
        assert_eq!(edge.0.errno, EINVAL);
        assert_eq!(edge.0.gate, Some(Gate::Range));
        let m = run_clockid_matrix();
        let mut i = 0usize;
        while i < CLOCKID_ROWS {
            match m.per_row.get(i) {
                Some(n) => assert_eq!(*n, INJECT_ROUNDS),
                None => assert_eq!(1, 0),
            }
            i += 1;
        }
        assert_eq!(m.einval_rows, 11);
        assert_eq!(m.ok_rows, 2);
        assert_eq!(m.ledger.injected, 140);
        assert_eq!(m.ledger.escaped, 0);
    }

    #[test]
    fn busy_writer_blocks_commit() {
        let mut st = ClockStore::boot();
        let mut jn = ClockJournal::new();
        jn.busy = true;
        let got = clock_settime(&SetRequest::new(CLOCK_REALTIME, UserTs::Read(PROBE_TS)), CapSet::sys_time(), &mut st, &mut jn);
        assert_eq!(got.errno, EBUSY);
        assert_eq!(jn.committed, 0);
        assert_eq!(jn.busy_hits, 1);
        assert_eq!(st.read(CLOCK_REALTIME), Some(BOOT_REALTIME));
        jn.busy = false;
        let again = clock_settime(&SetRequest::new(CLOCK_REALTIME, UserTs::Read(PROBE_TS)), CapSet::sys_time(), &mut st, &mut jn);
        assert_eq!(again.errno, 0);
    }

    #[test]
    fn journal_ring_overwrites_oldest() {
        let mut st = ClockStore::boot();
        let mut jn = ClockJournal::new();
        let mut k = 0i64;
        while k < (JOURNAL_CAP as i64 + 2) {
            let req = SetRequest::new(CLOCK_REALTIME, UserTs::Read(Timespec { sec: 1_800_000_000 + k, nsec: 0 }));
            assert_eq!(clock_settime(&req, CapSet::sys_time(), &mut st, &mut jn).errno, 0);
            k += 1;
        }
        assert_eq!(jn.committed as usize, JOURNAL_CAP + 2);
        assert_eq!(jn.seq as usize, JOURNAL_CAP + 2);
        assert_eq!(jn.last().map(|e| e.next_sec), Some(1_800_000_000 + JOURNAL_CAP as i64 + 1));
    }

    #[test]
    fn privilege_and_order_matrices() {
        let p = run_privilege_matrix();
        assert_eq!(p.total, 4);
        assert_eq!(p.eperm, 2);
        assert_eq!(p.einval, 1);
        assert_eq!(p.ok, 1);
        let (hit, total) = run_order_matrix();
        assert_eq!(hit, total);
        assert_eq!(total, 3);
    }

    #[test]
    fn fingerprint_and_codes() {
        let fp = corpus_fingerprint();
        assert_eq!(fp, corpus_fingerprint());
        assert_eq!(fp != 0, true);
        assert_eq!(SetCode::ALL.len(), 12);
        let mut a = 0usize;
        while a < SetCode::ALL.len() {
            let x = match SetCode::ALL.get(a) {
                Some(v) => *v,
                None => break,
            };
            assert_eq!(x.detail().is_empty(), false);
            let mut b = a + 1;
            while b < SetCode::ALL.len() {
                if let Some(y) = SetCode::ALL.get(b) {
                    assert_eq!(x.code() != y.code(), true);
                }
                b += 1;
            }
            a += 1;
        }
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx9323_checks();
        assert_eq!(s.all_passed(), true);
    }
}
