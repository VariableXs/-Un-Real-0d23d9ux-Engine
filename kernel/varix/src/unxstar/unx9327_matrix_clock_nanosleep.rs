//! UNX-F9327 clock_nanosleep 非法标志/时钟矩阵 · 0x7BF40段
//! ─────────────────────────────────────────────────────────────
//! 判据原文（任务板 F9327 逐字）：
//!   ① 纯功能代码 ≥360 行；
//!   ② 判据原文：UNX-F9327-J1 非法标志注入判据过，全 EINVAL/ENOTSUP
//!      逐格检出，非法时钟全 EINVAL 格过，与 B20 对账格过；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//!   ① 第一轴是 flags 而非 clock：内核先用 `flags & ~TIMER_ABSTIME` 做闭集
//!      判定，任何未登记位（0x2 这类「看着像标志」的位、0x8000_0000 高位
//!      垃圾）都回 EINVAL。矩阵里同一时钟在 flags=0x2 列的结论必须与
//!      flags=0x0 列不同，这正是「非法标志注入」的可观测落点。
//!   ② 第二轴把「未登记 clockid」与「已登记但不支持睡眠」拆开：未登记
//!      （42/-7）回 EINVAL（Linux 口径），已登记的 CPU/COARSE/RAW/ALARM/
//!      TAI 回 ENOTSUP=95（POSIX 口径）。两码混用会让「逐格检出」退化为
//!      单码，故分层计数本身也是判据。
//!   ③ 冻结矩阵 12 个 clockid × 3 个 flags 取法 = 36 格，每格 10 轮复算：
//!      期望码写死在表里（不现算），模型必须逐格命中；EINVAL 14 格、
//!      ENOTSUP 16 格、成功 6 格，三数之和必须等于 36。
//!   ④ 次序是语义：flags → clock 登记 → 睡眠能力 → request 指针 → 值域 →
//!      中断路径 remain 指针。六行对抗样例专挑「两个故障面同时命中」，
//!      另以步号行走复算（1..=6），跳步或回退即判红。
//!   ⑤ 与 B20 对账：六条用例的期望码冻结在本地镜像里，模型逐条复算；用例名
//!      两两不同（零冲突格），防止同格被两侧重复认领后「对账」实为自证。
//!   ⑥ 注入账 injected == detected + escaped 且 escaped == 0；总量 36×10=360，
//!      逐格计数与总量双向核对。
//!   ⑦ 诊断码段内闭集 12 个（0x7BF41..=0x7BF4C），码值两两唯一。
//! ─────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 诊断段基址（F9327 独占 0x7BF40..=0x7BF7F；F9328 从 0x7BF80 起）。
pub const SEG_BASE: u32 = 0x7BF40;
/// 段窗口宽度。
pub const SEG_WINDOW: u32 = 0x40;
/// 唯一被承认的标志位。
pub const TIMER_ABSTIME: u32 = 0x0000_0001;
/// 成功返回 0（不是剩余时间）。
pub const OK: i32 = 0;
/// 被信号打断（remain 已回写）。
pub const EINTR: i32 = 4;
/// request/remain 指针不可读写。
pub const EFAULT: i32 = 14;
/// 标志/时钟/值域非法。
pub const EINVAL: i32 = 22;
/// 已登记时钟不支持睡眠（Linux 上等于 EOPNOTSUPP）。
pub const ENOTSUP: i32 = 95;
/// tv_nsec 右开上界。
pub const NSEC_PER_SEC: i64 = 1_000_000_000;
/// 单格注入复测轮数。
pub const INJECT_ROUNDS: u32 = 10;
/// 已登记时钟行数 / 网格时钟取法（含 1 个未登记探针）。
pub const CLOCK_KINDS: usize = 11;
pub const GRID_CLOCKS: usize = 12;
/// 网格 flags 取法 / 冻结格数。
pub const GRID_FLAGS: usize = 3;
pub const GRID_CELLS: usize = GRID_CLOCKS * GRID_FLAGS;
/// 值域 / 指针 / 次序 / 对账样例数。
pub const TS_CASES: usize = 8;
pub const PTR_CASES: usize = 6;
pub const ORDER_CASES: usize = 6;
pub const B20_ROWS: usize = 6;

// ── 时钟表 ────────────────────────────────────────────────────────────
/// 一个 clockid 的能力行。`sleepable` 是核心能力位：只有 REALTIME/
/// MONOTONIC/BOOTTIME 允许进入 clock_nanosleep。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ClockSpec {
    pub id: i32,
    pub name: &'static str,
    pub sleepable: bool,
    pub cpu: bool,
    pub coarse: bool,
    pub alarm: bool,
}

/// 11 行冻结时钟表；顺序即指纹顺序。
pub const CLOCKS: [ClockSpec; CLOCK_KINDS] = [
    ClockSpec { id: 0, name: "CLOCK_REALTIME", sleepable: true, cpu: false, coarse: false, alarm: false },
    ClockSpec { id: 1, name: "CLOCK_MONOTONIC", sleepable: true, cpu: false, coarse: false, alarm: false },
    ClockSpec { id: 2, name: "CLOCK_PROCESS_CPUTIME_ID", sleepable: false, cpu: true, coarse: false, alarm: false },
    ClockSpec { id: 3, name: "CLOCK_THREAD_CPUTIME_ID", sleepable: false, cpu: true, coarse: false, alarm: false },
    ClockSpec { id: 4, name: "CLOCK_MONOTONIC_RAW", sleepable: false, cpu: false, coarse: false, alarm: false },
    ClockSpec { id: 5, name: "CLOCK_REALTIME_COARSE", sleepable: false, cpu: false, coarse: true, alarm: false },
    ClockSpec { id: 6, name: "CLOCK_MONOTONIC_COARSE", sleepable: false, cpu: false, coarse: true, alarm: false },
    ClockSpec { id: 7, name: "CLOCK_BOOTTIME", sleepable: true, cpu: false, coarse: false, alarm: false },
    ClockSpec { id: 8, name: "CLOCK_REALTIME_ALARM", sleepable: false, cpu: false, coarse: false, alarm: true },
    ClockSpec { id: 9, name: "CLOCK_BOOTTIME_ALARM", sleepable: false, cpu: false, coarse: false, alarm: true },
    ClockSpec { id: 11, name: "CLOCK_TAI", sleepable: false, cpu: false, coarse: false, alarm: false },
];

/// 线性查表；未登记返回 None（调用方必须转 EINVAL，不得回落默认行）。
pub fn lookup_clock(id: i32) -> Option<ClockSpec> {
    let mut i = 0usize;
    while i < CLOCKS.len() {
        if let Some(row) = CLOCKS.get(i).copied() {
            if row.id == id {
                return Some(row);
            }
        }
        i += 1;
    }
    None
}

/// 族计数：(可睡眠, CPU, COARSE, ALARM)；冻结期望 (3,2,2,2)。
pub fn family_counts() -> (u32, u32, u32, u32) {
    let (mut sl, mut cpu, mut coarse, mut alarm) = (0u32, 0u32, 0u32, 0u32);
    let mut i = 0usize;
    while i < CLOCKS.len() {
        if let Some(row) = CLOCKS.get(i) {
            if row.sleepable {
                sl = sl.saturating_add(1);
            }
            if row.cpu {
                cpu = cpu.saturating_add(1);
            }
            if row.coarse {
                coarse = coarse.saturating_add(1);
            }
            if row.alarm {
                alarm = alarm.saturating_add(1);
            }
        }
        i += 1;
    }
    (sl, cpu, coarse, alarm)
}

// ── flags 解码 ────────────────────────────────────────────────────────
/// 标志取法三态闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FlagMode {
    Relative,
    Absolute,
    Invalid,
}

/// `!TIMER_ABSTIME` 掩码一次判掉全部未登记位，不做逐位白名单。
pub fn decode_flags(flags: u32) -> FlagMode {
    if flags & !TIMER_ABSTIME != 0 {
        FlagMode::Invalid
    } else if flags & TIMER_ABSTIME != 0 {
        FlagMode::Absolute
    } else {
        FlagMode::Relative
    }
}

/// 标志可打印名。
pub fn flag_name(flags: u32) -> &'static str {
    match decode_flags(flags) {
        FlagMode::Relative => "REL",
        FlagMode::Absolute => "ABS",
        FlagMode::Invalid => "BAD",
    }
}

// ── timespec 值域 ─────────────────────────────────────────────────────
/// 请求时间。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Timespec {
    pub sec: i64,
    pub nsec: i64,
}

impl Timespec {
    pub const fn new(sec: i64, nsec: i64) -> Timespec {
        Timespec { sec, nsec }
    }
    /// tv_sec >= 0 且 0 <= tv_nsec < 1e9。
    pub fn in_range(&self) -> bool {
        self.sec >= 0 && self.nsec >= 0 && self.nsec < NSEC_PER_SEC
    }
    /// 零睡眠（相对睡眠立即返回）。
    pub fn is_zero(&self) -> bool {
        self.sec == 0 && self.nsec == 0
    }
}

/// 绝对时刻推进：sec 溢出返回 None，绝不回绕。
pub fn add_checked(ts: &Timespec, delta: i64) -> Option<Timespec> {
    let sec = ts.sec.checked_add(delta)?;
    Some(Timespec::new(sec, ts.nsec))
}

// ── 指针模型 ──────────────────────────────────────────────────────────
/// 用户指针三态；空指针在 uaccess 里同样不可读。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PtrState {
    Good,
    Null,
    Unmapped,
}

/// 只有 Good 可读。
pub fn ptr_readable(p: PtrState) -> bool {
    matches!(p, PtrState::Good)
}

/// remain 指针的中断路径口径：只有未映射页回 EFAULT。
///
/// 空指针表示调用方放弃剩余时间回写（clock_nanosleep 的 rmtp=NULL 是
/// 合法用法，睡眠照常被信号打断并回 EINTR），故 Null 不得按 EFAULT 判；
/// request 指针没有这个豁免，仍走 ptr_readable。
pub fn remain_usable(p: PtrState) -> bool {
    !matches!(p, PtrState::Unmapped)
}

// ── 请求与判定 ────────────────────────────────────────────────────────
/// 一次 clock_nanosleep 调用的完整输入。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Request {
    pub clock: i32,
    pub flags: u32,
    pub req: Timespec,
    pub req_ptr: PtrState,
    pub remain_ptr: PtrState,
    pub interrupted: bool,
}

impl Request {
    pub const fn new(
        clock: i32,
        flags: u32,
        req: Timespec,
        req_ptr: PtrState,
        remain_ptr: PtrState,
        interrupted: bool,
    ) -> Request {
        Request { clock, flags, req, req_ptr, remain_ptr, interrupted }
    }
}

/// 判定：0 / EINTR / EINVAL / ENOTSUP / EFAULT。次序即语义，禁止调整。
pub fn decide(r: Request) -> i32 {
    if decode_flags(r.flags) == FlagMode::Invalid {
        return EINVAL;
    }
    let spec = match lookup_clock(r.clock) {
        Some(row) => row,
        None => return EINVAL,
    };
    if !spec.sleepable {
        return ENOTSUP;
    }
    if !ptr_readable(r.req_ptr) {
        return EFAULT;
    }
    if !r.req.in_range() {
        return EINVAL;
    }
    if r.interrupted {
        if !remain_usable(r.remain_ptr) {
            return EFAULT;
        }
        return EINTR;
    }
    OK
}

// ── 次序行走 ──────────────────────────────────────────────────────────
/// 走一遍判定，返回 (停在哪一步, 返回码)：1 标志、2 时钟、3 能力、4 指针、
/// 5 值域、6 remain/完成。步号即相位，禁止跳步、回退或越界。
pub fn walk(r: Request) -> (u8, i32) {
    if decode_flags(r.flags) == FlagMode::Invalid {
        return (1, EINVAL);
    }
    let spec = match lookup_clock(r.clock) {
        Some(row) => row,
        None => return (2, EINVAL),
    };
    if !spec.sleepable {
        return (3, ENOTSUP);
    }
    if !ptr_readable(r.req_ptr) {
        return (4, EFAULT);
    }
    if !r.req.in_range() {
        return (5, EINVAL);
    }
    if r.interrupted {
        if !remain_usable(r.remain_ptr) {
            return (6, EFAULT);
        }
        return (6, EINTR);
    }
    (6, OK)
}

// ── 注入账 ────────────────────────────────────────────────────────────
/// 注入 = 检出 + 逃逸，逃逸恒为 0。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectLedger {
    pub injected: u32,
    pub detected: u32,
    pub escaped: u32,
}

impl InjectLedger {
    pub const fn zero() -> InjectLedger {
        InjectLedger { injected: 0, detected: 0, escaped: 0 }
    }
    /// 三格同步推进；守恒式由检查方另行核对。
    pub fn record(&mut self, caught: bool) {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
    }
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

// ── 冻结网格：clockid × flags ─────────────────────────────────────────
/// 一格：时钟 + 标志 + 期望码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GridCell {
    pub clock: i32,
    pub flags: u32,
    pub expect: i32,
}

const fn gc(clock: i32, flags: u32, expect: i32) -> GridCell {
    GridCell { clock, flags, expect }
}

/// 36 格冻结矩阵，一行一个时钟（REL / ABS / BAD 三列）。
/// 0/1/7 可睡眠；2/3/4/5/6/8/9/11 已登记但不可睡眠（ENOTSUP）；
/// 42 未登记（EINVAL）；flags=0x2 列因标志优先一律 EINVAL。
pub const FLAG_CLOCK_GRID: [GridCell; GRID_CELLS] = [
    gc(0, 0x0, OK), gc(0, 0x1, OK), gc(0, 0x2, EINVAL),
    gc(1, 0x0, OK), gc(1, 0x1, OK), gc(1, 0x2, EINVAL),
    gc(2, 0x0, ENOTSUP), gc(2, 0x1, ENOTSUP), gc(2, 0x2, EINVAL),
    gc(3, 0x0, ENOTSUP), gc(3, 0x1, ENOTSUP), gc(3, 0x2, EINVAL),
    gc(4, 0x0, ENOTSUP), gc(4, 0x1, ENOTSUP), gc(4, 0x2, EINVAL),
    gc(5, 0x0, ENOTSUP), gc(5, 0x1, ENOTSUP), gc(5, 0x2, EINVAL),
    gc(6, 0x0, ENOTSUP), gc(6, 0x1, ENOTSUP), gc(6, 0x2, EINVAL),
    gc(7, 0x0, OK), gc(7, 0x1, OK), gc(7, 0x2, EINVAL),
    gc(8, 0x0, ENOTSUP), gc(8, 0x1, ENOTSUP), gc(8, 0x2, EINVAL),
    gc(9, 0x0, ENOTSUP), gc(9, 0x1, ENOTSUP), gc(9, 0x2, EINVAL),
    gc(11, 0x0, ENOTSUP), gc(11, 0x1, ENOTSUP), gc(11, 0x2, EINVAL),
    gc(42, 0x0, EINVAL), gc(42, 0x1, EINVAL), gc(42, 0x2, EINVAL),
];

/// 逐格 10 轮注入：每格命中数 + 账本。
pub fn run_flag_clock_grid() -> ([u32; GRID_CELLS], InjectLedger) {
    let mut per_cell = [0u32; GRID_CELLS];
    let mut ledger = InjectLedger::zero();
    let mut ci = 0usize;
    while ci < FLAG_CLOCK_GRID.len() {
        if let Some(cell) = FLAG_CLOCK_GRID.get(ci).copied() {
            let mut round = 0u32;
            while round < INJECT_ROUNDS {
                let req = Request::new(cell.clock, cell.flags, Timespec::new(1, 0), PtrState::Good, PtrState::Good, false);
                let caught = decide(req) == cell.expect;
                ledger.record(caught);
                if caught {
                    if let Some(slot) = per_cell.get_mut(ci) {
                        *slot = slot.saturating_add(1);
                    }
                }
                round = round.saturating_add(1);
            }
        }
        ci += 1;
    }
    (per_cell, ledger)
}

/// 网格里期望码为 `code` 的格数。
pub fn count_expected(code: i32) -> u32 {
    let mut n = 0u32;
    let mut i = 0usize;
    while i < FLAG_CLOCK_GRID.len() {
        if let Some(cell) = FLAG_CLOCK_GRID.get(i) {
            if cell.expect == code {
                n = n.saturating_add(1);
            }
        }
        i += 1;
    }
    n
}

// ── 值域网格 ──────────────────────────────────────────────────────────
/// timespec 样例。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TsCase {
    pub ts: Timespec,
    pub expect: i32,
    pub note: &'static str,
}

const fn tc(sec: i64, nsec: i64, expect: i32, note: &'static str) -> TsCase {
    TsCase { ts: Timespec { sec, nsec }, expect, note }
}

/// 8 个值域样例（MONOTONIC / 相对 / 双指针可读）。
pub const TS_GRID: [TsCase; TS_CASES] = [
    tc(0, 0, OK, "零睡眠"),
    tc(1, 0, OK, "整秒"),
    tc(0, 999_999_999, OK, "纳秒上界内"),
    tc(0, 1_000_000_000, EINVAL, "纳秒达 1e9"),
    tc(0, -1, EINVAL, "纳秒为负"),
    tc(-1, 0, EINVAL, "秒为负"),
    tc(i64::MAX, 0, OK, "极大秒"),
    tc(0, i64::MAX, EINVAL, "纳秒爆上界"),
];

/// 逐样例复算：(命中, 总数)。
pub fn run_ts_grid() -> (u32, u32) {
    let mut matched = 0u32;
    let mut i = 0usize;
    while i < TS_GRID.len() {
        if let Some(c) = TS_GRID.get(i).copied() {
            let req = Request::new(1, 0, c.ts, PtrState::Good, PtrState::Good, false);
            if decide(req) == c.expect {
                matched = matched.saturating_add(1);
            }
        }
        i += 1;
    }
    (matched, TS_GRID.len() as u32)
}

// ── 指针网格 ──────────────────────────────────────────────────────────
/// 指针样例：remain 只在中断路径被触碰。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PtrCase {
    pub req_ptr: PtrState,
    pub remain_ptr: PtrState,
    pub interrupted: bool,
    pub expect: i32,
    pub note: &'static str,
}

const fn pc(rp: PtrState, mp: PtrState, intr: bool, expect: i32, note: &'static str) -> PtrCase {
    PtrCase { req_ptr: rp, remain_ptr: mp, interrupted: intr, expect, note }
}

/// 6 个指针样例。
pub const PTR_GRID: [PtrCase; PTR_CASES] = [
    pc(PtrState::Good, PtrState::Good, false, OK, "双指针可读"),
    pc(PtrState::Null, PtrState::Good, false, EFAULT, "request 为空"),
    pc(PtrState::Unmapped, PtrState::Good, false, EFAULT, "request 未映射"),
    pc(PtrState::Good, PtrState::Null, true, EINTR, "中断且 remain 为空"),
    pc(PtrState::Good, PtrState::Unmapped, true, EFAULT, "中断且 remain 不可写"),
    pc(PtrState::Good, PtrState::Good, true, EINTR, "中断且 remain 可写"),
];

/// 逐样例复算：(命中, 总数)。
pub fn run_ptr_grid() -> (u32, u32) {
    let mut matched = 0u32;
    let mut i = 0usize;
    while i < PTR_GRID.len() {
        if let Some(c) = PTR_GRID.get(i).copied() {
            let req = Request::new(1, 0, Timespec::new(1, 0), c.req_ptr, c.remain_ptr, c.interrupted);
            if decide(req) == c.expect {
                matched = matched.saturating_add(1);
            }
        }
        i += 1;
    }
    (matched, PTR_GRID.len() as u32)
}

// ── 次序样例 ──────────────────────────────────────────────────────────
/// 次序对抗样例：每行同时命中两个故障面，考的是谁先返回。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct OrderCase {
    pub name: &'static str,
    pub req: Request,
    pub expect: i32,
}

const fn oc(name: &'static str, clock: i32, flags: u32, sec: i64, nsec: i64,
            rp: PtrState, mp: PtrState, intr: bool, expect: i32) -> OrderCase {
    OrderCase { name, req: Request::new(clock, flags, Timespec::new(sec, nsec), rp, mp, intr), expect }
}

/// 六行冻结次序。
pub const ORDER_GRID: [OrderCase; ORDER_CASES] = [
    oc("坏标志+未知时钟", 42, 0x2, 1, 0, PtrState::Null, PtrState::Good, false, EINVAL),
    oc("未知时钟+坏指针", 42, 0x0, 1, 0, PtrState::Null, PtrState::Good, false, EINVAL),
    oc("CPU时钟+坏指针", 2, 0x0, 1, 0, PtrState::Null, PtrState::Good, false, ENOTSUP),
    oc("坏指针+坏值域", 1, 0x0, -1, 0, PtrState::Null, PtrState::Good, false, EFAULT),
    oc("值域越界", 1, 0x0, 0, 1_000_000_000, PtrState::Good, PtrState::Good, false, EINVAL),
    oc("中断+remain坏", 1, 0x0, 1, 0, PtrState::Good, PtrState::Unmapped, true, EFAULT),
];

/// 逐行复算并核对步号：(命中, 总数, 步号自洽)。
pub fn run_order_grid() -> (u32, u32, bool) {
    let mut matched = 0u32;
    let mut walk_ok = true;
    let mut i = 0usize;
    while i < ORDER_GRID.len() {
        if let Some(c) = ORDER_GRID.get(i).copied() {
            let got = decide(c.req);
            if got == c.expect {
                matched = matched.saturating_add(1);
            }
            let (step, ret) = walk(c.req);
            if ret != got || step == 0 || step > 6 {
                walk_ok = false;
            }
        }
        i += 1;
    }
    (matched, ORDER_GRID.len() as u32, walk_ok)
}

/// 次序样例里期望码为 `code` 的行数。
pub fn order_count(code: i32) -> u32 {
    let mut n = 0u32;
    let mut i = 0usize;
    while i < ORDER_GRID.len() {
        if let Some(c) = ORDER_GRID.get(i) {
            if c.expect == code {
                n = n.saturating_add(1);
            }
        }
        i += 1;
    }
    n
}

// ── B20 对账 ──────────────────────────────────────────────────────────
/// B20 用例的本地只读镜像（不引用外部模块）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct B20Row {
    pub case: &'static str,
    pub clock: i32,
    pub flags: u32,
    pub ts: Timespec,
    pub req_ptr: PtrState,
    pub remain_ptr: PtrState,
    pub interrupted: bool,
    pub expect: i32,
}

const fn b20(case: &'static str, clock: i32, flags: u32, sec: i64, nsec: i64,
             rp: PtrState, mp: PtrState, intr: bool, expect: i32) -> B20Row {
    B20Row { case, clock, flags, ts: Timespec::new(sec, nsec), req_ptr: rp, remain_ptr: mp, interrupted: intr, expect }
}

/// 六条对账用例：标志两位 / 未知时钟 / CPU 时钟 / 空指针 / 纳秒越界。
pub const B20_LEDGER: [B20Row; B20_ROWS] = [
    b20("b20:cn-flag-bit2", 1, 0x2, 1, 0, PtrState::Good, PtrState::Good, false, EINVAL),
    b20("b20:cn-flag-high", 1, 0x8000_0000, 1, 0, PtrState::Good, PtrState::Good, false, EINVAL),
    b20("b20:cn-clock-unknown", 42, 0x0, 1, 0, PtrState::Good, PtrState::Good, false, EINVAL),
    b20("b20:cn-clock-cpu", 2, 0x0, 1, 0, PtrState::Good, PtrState::Good, false, ENOTSUP),
    b20("b20:cn-req-null", 1, 0x0, 1, 0, PtrState::Null, PtrState::Good, false, EFAULT),
    b20("b20:cn-nsec-1e9", 0, 0x0, 0, 1_000_000_000, PtrState::Good, PtrState::Good, false, EINVAL),
];

/// 逐条复算镜像：(一致, 总数)。
pub fn b20_agreement() -> (u32, u32) {
    let mut hit = 0u32;
    let mut i = 0usize;
    while i < B20_LEDGER.len() {
        if let Some(row) = B20_LEDGER.get(i).copied() {
            let req = Request::new(row.clock, row.flags, row.ts, row.req_ptr, row.remain_ptr, row.interrupted);
            if decide(req) == row.expect {
                hit = hit.saturating_add(1);
            }
        }
        i += 1;
    }
    (hit, B20_LEDGER.len() as u32)
}

/// 对账冲突：用例名两两重复即冲突。
pub fn b20_conflicts() -> u32 {
    let mut n = 0u32;
    let mut i = 0usize;
    while i < B20_LEDGER.len() {
        if let Some(a) = B20_LEDGER.get(i) {
            let mut j = i + 1;
            while j < B20_LEDGER.len() {
                if let Some(b) = B20_LEDGER.get(j) {
                    if a.case == b.case {
                        n = n.saturating_add(1);
                    }
                }
                j += 1;
            }
        }
        i += 1;
    }
    n
}

// ── Linux errno 对照 ─────────────────────────────────────────────────
/// (名称, Linux 正值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EINTR", 4, 995, "ERROR_OPERATION_ABORTED；睡眠被打断"),
    ("EFAULT", 14, 998, "ERROR_NOACCESS；用户指针不可读写"),
    ("EINVAL", 22, 87, "ERROR_INVALID_PARAMETER；标志/时钟/值域非法"),
    ("ENOTSUP", 95, 50, "ERROR_NOT_SUPPORTED；该时钟不支持睡眠"),
];

/// 是否为本模块承认的 errno 正值。
pub fn errno_known(code: i32) -> bool {
    let mut i = 0usize;
    while i < ERRNO_TABLE.len() {
        if let Some(row) = ERRNO_TABLE.get(i) {
            if row.1 == code {
                return true;
            }
        }
        i += 1;
    }
    false
}

/// errno 表内数值重复数（必须为 0）。
pub fn errno_conflicts() -> u32 {
    let mut n = 0u32;
    let mut i = 0usize;
    while i < ERRNO_TABLE.len() {
        if let Some(a) = ERRNO_TABLE.get(i) {
            let mut j = i + 1;
            while j < ERRNO_TABLE.len() {
                if let Some(b) = ERRNO_TABLE.get(j) {
                    if a.1 == b.1 {
                        n = n.saturating_add(1);
                    }
                }
                j += 1;
            }
        }
        i += 1;
    }
    n
}

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..=+0x0C）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u16)]
pub enum ClockCode {
    BadFlagBit,
    UnknownClock,
    UnsupportedClock,
    RequestPointer,
    RemainPointer,
    NsecRange,
    NegSeconds,
    OrderViolation,
    LedgerBreak,
    B20Conflict,
    SegmentViolation,
    NotReproducible,
}

impl ClockCode {
    /// 批内相对码（低 16 位口径）。
    pub fn code(self) -> u16 {
        SEG_BASE as u16 + 1 + self as u16
    }
    /// 全宽段内码（窗口守卫用）。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }
    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            ClockCode::BadFlagBit => "flags 含 TIMER_ABSTIME 之外的位：请清零未登记位，只承认 0x1",
            ClockCode::UnknownClock => "clockid 未登记：野生 id 请回 EINVAL，勿回落默认时钟",
            ClockCode::UnsupportedClock => "已登记时钟不支持睡眠：CPU/COARSE/RAW/ALARM/TAI 回 ENOTSUP",
            ClockCode::RequestPointer => "request 指针不可读：空指针与未映射页同判 EFAULT",
            ClockCode::RemainPointer => "remain 指针不可写：仅中断路径需回写，坏指针回 EFAULT",
            ClockCode::NsecRange => "tv_nsec 越界：必须在 [0,1e9)，1e9 应进位到 tv_sec",
            ClockCode::NegSeconds => "tv_sec 为负：相对睡眠不接受负时长",
            ClockCode::OrderViolation => "判定次序错误：标志早于时钟，时钟早于指针，指针早于值域",
            ClockCode::LedgerBreak => "注入账不守恒：injected 必须等于 detected+escaped",
            ClockCode::B20Conflict => "B20 对账冲突：用例名重复认领或期望码与模型分歧",
            ClockCode::SegmentViolation => "段位越界：本模块独占 0x7BF40 的 0x40 窗口",
            ClockCode::NotReproducible => "双跑指纹不一致：判定路径混入非确定状态",
        }
    }
    /// 闭集全量。
    pub const ALL: &[ClockCode] = &[
        ClockCode::BadFlagBit,
        ClockCode::UnknownClock,
        ClockCode::UnsupportedClock,
        ClockCode::RequestPointer,
        ClockCode::RemainPointer,
        ClockCode::NsecRange,
        ClockCode::NegSeconds,
        ClockCode::OrderViolation,
        ClockCode::LedgerBreak,
        ClockCode::B20Conflict,
        ClockCode::SegmentViolation,
        ClockCode::NotReproducible,
    ];
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

/// 大端压入 i32。
fn push_i32(out: &mut Vec<u8>, v: i32) {
    let u = v as u32;
    out.push((u >> 24) as u8);
    out.push((u >> 16) as u8);
    out.push((u >> 8) as u8);
    out.push(u as u8);
}

/// 规范语料指纹：网格 36 + 值域 8 + 指针 6 + 次序 6 + B20 6 = 62 个判定。
pub fn corpus_fingerprint() -> u32 {
    let mut bytes: Vec<u8> = Vec::new();
    let mut i = 0usize;
    while i < FLAG_CLOCK_GRID.len() {
        if let Some(c) = FLAG_CLOCK_GRID.get(i).copied() {
            push_i32(&mut bytes, decide(Request::new(c.clock, c.flags, Timespec::new(1, 0), PtrState::Good, PtrState::Good, false)));
        }
        i += 1;
    }
    let mut t = 0usize;
    while t < TS_GRID.len() {
        if let Some(c) = TS_GRID.get(t).copied() {
            push_i32(&mut bytes, decide(Request::new(1, 0, c.ts, PtrState::Good, PtrState::Good, false)));
        }
        t += 1;
    }
    let mut p = 0usize;
    while p < PTR_GRID.len() {
        if let Some(c) = PTR_GRID.get(p).copied() {
            push_i32(&mut bytes, decide(Request::new(1, 0, Timespec::new(1, 0), c.req_ptr, c.remain_ptr, c.interrupted)));
        }
        p += 1;
    }
    let mut o = 0usize;
    while o < ORDER_GRID.len() {
        if let Some(c) = ORDER_GRID.get(o).copied() {
            push_i32(&mut bytes, decide(c.req));
        }
        o += 1;
    }
    let mut b = 0usize;
    while b < B20_LEDGER.len() {
        if let Some(row) = B20_LEDGER.get(b).copied() {
            push_i32(&mut bytes, decide(Request::new(row.clock, row.flags, row.ts, row.req_ptr, row.remain_ptr, row.interrupted)));
        }
        b += 1;
    }
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9327 判据集（add ≥12；add_dyn ≤8）。
pub fn run_unx9327_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9327");
    let (per_cell, ledger) = run_flag_clock_grid();
    let (ts_hit, ts_total) = run_ts_grid();
    let (ptr_hit, ptr_total) = run_ptr_grid();
    let (order_hit, order_total, walk_ok) = run_order_grid();
    let (b20_hit, b20_total) = b20_agreement();
    let ok_cells = count_expected(OK);
    let einval_cells = count_expected(EINVAL);
    let enotsup_cells = count_expected(ENOTSUP);
    let (sleepable, cpu, coarse, alarm) = family_counts();
    let fp_a = corpus_fingerprint();
    let fp_b = corpus_fingerprint();

    let seg_ok = SEG_BASE == 0x7BF40
        && SEG_BASE % 0x40 == 0
        && (0x7B000..=0x7D1C0).contains(&SEG_BASE)
        && SEG_BASE + SEG_WINDOW <= 0x7BF80;
    s.add("段位守卫：0x7BF40 起 0x40 窗口不与 F9328 重叠", seg_ok,
          "段基址必须 0x40 对齐且本模块不得覆盖相邻单的段");

    let mut clock_shape = CLOCKS.len() == CLOCK_KINDS;
    let mut i = 0usize;
    while i < CLOCKS.len() {
        if let Some(a) = CLOCKS.get(i) {
            if a.name.is_empty() {
                clock_shape = false;
            }
            let mut j = i + 1;
            while j < CLOCKS.len() {
                if let Some(b) = CLOCKS.get(j) {
                    if a.id == b.id {
                        clock_shape = false;
                    }
                }
                j += 1;
            }
        }
        i += 1;
    }
    s.add("时钟表：11 行 id 唯一且名称非空", clock_shape,
          "重复 id 会让查表结果不确定");

    s.add("时钟族计数：3 可睡眠 / 2 CPU / 2 COARSE / 2 ALARM",
          sleepable == 3 && cpu == 2 && coarse == 2 && alarm == 2,
          "族计数冻结，防止把 COARSE 误标可睡眠后矩阵整体变绿");

    let flag_ok = decode_flags(0) == FlagMode::Relative
        && decode_flags(TIMER_ABSTIME) == FlagMode::Absolute
        && decode_flags(0x2) == FlagMode::Invalid
        && decode_flags(0x8000_0000) == FlagMode::Invalid
        && decode_flags(0xFFFF_FFFF) == FlagMode::Invalid;
    s.add("标志闭集：0/1 合法，0x2 与高位垃圾一律非法", flag_ok,
          "未登记位必须整字掩掉，不得逐位白名单遗漏");

    let advance_ok = add_checked(&Timespec::new(i64::MAX, 0), 1).is_none()
        && add_checked(&Timespec::new(0, 0), NSEC_PER_SEC).is_some()
        && add_checked(&Timespec::new(-1, 0), 1) == Some(Timespec::new(0, 0));
    s.add("绝对时刻推进：溢出返回 None 不回绕", advance_ok,
          "deadline 加偏移必须 checked_add，回绕会把长眠变成忙等");

    let cells_ok = per_cell.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("网格注入：36 格各 10/10 命中冻结期望码", cells_ok,
          "任一格漂移都说明 decide 与冻结表脱钩");

    let ledger_ok = ledger.conserved()
        && ledger.escaped == 0
        && ledger.injected == (GRID_CELLS as u32) * INJECT_ROUNDS
        && ledger.detected == ledger.injected;
    s.add("注入账：360 次全检出且逃逸为 0", ledger_ok,
          "injected 必须等于 detected+escaped，逃逸不得静默");

    s.add("矩阵分层：EINVAL 14 格 / ENOTSUP 16 格 / 成功 6 格",
          einval_cells == 14 && enotsup_cells == 16 && ok_cells == 6
              && einval_cells + enotsup_cells + ok_cells == GRID_CELLS as u32,
          "两码必须分层计数，混成一码即为「逐格检出」失效");

    let mut badflag_einval = 0u32;
    let mut g = 0usize;
    while g < FLAG_CLOCK_GRID.len() {
        if let Some(c) = FLAG_CLOCK_GRID.get(g).copied() {
            if c.flags == 0x2 {
                let req = Request::new(c.clock, c.flags, Timespec::new(1, 0), PtrState::Good, PtrState::Good, false);
                if decide(req) == EINVAL {
                    badflag_einval = badflag_einval.saturating_add(1);
                }
            }
        }
        g += 1;
    }
    s.add("非法标志：12 个时钟的 flags=0x2 列全部 EINVAL", badflag_einval == 12,
          "标志优先于时钟，CPU 时钟在坏标志下也必须回 EINVAL 而非 ENOTSUP");

    let unknown_ok = decide(Request::new(42, 0, Timespec::new(1, 0), PtrState::Good, PtrState::Good, false)) == EINVAL
        && decide(Request::new(42, 1, Timespec::new(1, 0), PtrState::Good, PtrState::Good, false)) == EINVAL
        && decide(Request::new(-7, 0, Timespec::new(1, 0), PtrState::Good, PtrState::Good, false)) == EINVAL;
    s.add("非法时钟：未登记 id（42/-7）全 EINVAL", unknown_ok,
          "未登记 id 必须走 EINVAL 分支，不得回落到 MONOTONIC");

    s.add("值域网格：8/8 样例逐格一致",
          ts_hit == ts_total && ts_total == TS_CASES as u32,
          "tv_sec 负数与 tv_nsec 越界都必须 EINVAL，i64::MAX 秒合法");

    s.add("指针网格：6/6 样例逐格一致（EFAULT/EINTR）",
          ptr_hit == ptr_total && ptr_total == PTR_CASES as u32,
          "remain 只在中断路径被触碰，空 remain 合法、坏 remain 回 EFAULT");

    s.add("判定次序：6/6 对抗样例命中且步号终点自洽",
          order_hit == order_total && order_total == ORDER_CASES as u32 && walk_ok
              && order_count(EINVAL) == 3 && order_count(EFAULT) == 2 && order_count(ENOTSUP) == 1,
          "标志先于时钟、时钟先于指针、指针先于值域，六行逐行复算");

    let mut step_ok = true;
    let mut k = 0usize;
    while k < ORDER_GRID.len() {
        if let Some(c) = ORDER_GRID.get(k).copied() {
            let (step, ret) = walk(c.req);
            if step == 0 || step > 6 || ret != decide(c.req) {
                step_ok = false;
            }
        }
        k += 1;
    }
    s.add("次序行走：步号落在 1..=6 且 ret 与判定一致", step_ok,
          "多走一步意味着有人在值域之后再查指针，次序已被改动");

    s.add("B20 对账：6/6 一致且用例名零重复",
          b20_hit == b20_total && b20_total == B20_ROWS as u32 && b20_conflicts() == 0,
          "同一用例名被两侧重复认领时对账等于自证，必须显性拒绝");

    let errno_ok = errno_known(EINTR) && errno_known(EFAULT) && errno_known(EINVAL) && errno_known(ENOTSUP)
        && !errno_known(OK) && !errno_known(-EINVAL) && errno_conflicts() == 0 && ERRNO_TABLE.len() == 4;
    s.add("errno 对照：4 码数值与 Linux 一致且表内零重复", errno_ok,
          "错误码只允许表内正值；负值口径由调用方统一取负");

    let mut code_unique = true;
    let mut code_in_window = true;
    let mut text_ok = true;
    let mut ci = 0usize;
    while ci < ClockCode::ALL.len() {
        if let Some(a) = ClockCode::ALL.get(ci) {
            if a.detail().is_empty() {
                text_ok = false;
            }
            if a.val() <= SEG_BASE || a.val() > SEG_BASE + 0x0C {
                code_in_window = false;
            }
            let mut cj = ci + 1;
            while cj < ClockCode::ALL.len() {
                if let Some(b) = ClockCode::ALL.get(cj) {
                    if a.val() == b.val() {
                        code_unique = false;
                    }
                }
                cj += 1;
            }
        }
        ci += 1;
    }
    s.add("诊断码：12 个码值唯一且全部落在 0x7BF41..=0x7BF4C",
          ClockCode::ALL.len() == 12 && code_unique && code_in_window && text_ok,
          "段内闭集越窗会与相邻单的诊断码撞车，文案不得为空");

    s.add("双跑一致：62 个判定的规范语料指纹相等", fp_a == fp_b,
          "同输入两次必须逐字节相等，隐藏状态即失败");

    let mut mono = ledger;
    let before = mono.injected;
    let mut kk = 0u32;
    let mut mono_ok = true;
    while kk < 4 {
        mono.record(true);
        if !mono.conserved() {
            mono_ok = false;
        }
        kk = kk.saturating_add(1);
    }
    s.add("账目单调：追加 4 次仅增不减且持续守恒", mono_ok && mono.injected == before + 4,
          "累计计数禁止回退或饱和到旧值");

    let grid_text = format!("网格 {} 格 × {} 轮 = 注入 {} / 检出 {} / 逃逸 {}",
                            GRID_CELLS, INJECT_ROUNDS, ledger.injected, ledger.detected, ledger.escaped);
    s.add_dyn("网格账本快照", ledger.conserved() && ledger.escaped == 0, &grid_text);

    let layer_text = format!("EINVAL {} / ENOTSUP {} / OK {} / 合计 {}",
                             einval_cells, enotsup_cells, ok_cells,
                             einval_cells + enotsup_cells + ok_cells);
    s.add_dyn("分层快照", einval_cells + enotsup_cells + ok_cells == GRID_CELLS as u32, &layer_text);

    let recon_text = format!("B20 {}/{} 一致，冲突 {}，指纹 0x{:08X}", b20_hit, b20_total, b20_conflicts(), fp_a);
    s.add_dyn("对账快照", b20_hit == b20_total && b20_conflicts() == 0, &recon_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flag_and_clock_tables() {
        assert_eq!(decode_flags(0), FlagMode::Relative);
        assert_eq!(decode_flags(TIMER_ABSTIME), FlagMode::Absolute);
        assert_eq!(decode_flags(TIMER_ABSTIME | 0x2), FlagMode::Invalid);
        assert_eq!(decode_flags(0x8000_0000), FlagMode::Invalid);
        assert_eq!(flag_name(2), "BAD");
        assert_eq!(CLOCKS.len(), CLOCK_KINDS);
        assert_eq!(lookup_clock(0).map(|c| c.sleepable), Some(true));
        assert_eq!(lookup_clock(2).map(|c| c.cpu), Some(true));
        assert_eq!(lookup_clock(42).is_none(), true);
        assert_eq!(family_counts(), (3, 2, 2, 2));
    }

    #[test]
    fn timespec_range_rules() {
        assert_eq!(Timespec::new(0, 0).in_range(), true);
        assert_eq!(Timespec::new(0, NSEC_PER_SEC - 1).in_range(), true);
        assert_eq!(Timespec::new(0, NSEC_PER_SEC).in_range(), false);
        assert_eq!(Timespec::new(-1, 0).in_range(), false);
        assert_eq!(Timespec::new(0, -1).in_range(), false);
        assert_eq!(Timespec::new(0, 0).is_zero(), true);
    }

    #[test]
    fn grid_counts_are_frozen() {
        assert_eq!(count_expected(OK), 6);
        assert_eq!(count_expected(ENOTSUP), 16);
        assert_eq!(count_expected(EINVAL), 14);
        assert_eq!(count_expected(EFAULT), 0);
        assert_eq!(count_expected(OK) + count_expected(ENOTSUP) + count_expected(EINVAL), 36);
    }

    #[test]
    fn grid_injection_all_caught() {
        let (per_cell, ledger) = run_flag_clock_grid();
        assert_eq!(per_cell.iter().all(|n| *n == INJECT_ROUNDS), true);
        assert_eq!(per_cell.len(), GRID_CELLS);
        assert_eq!(ledger.injected, 360);
        assert_eq!(ledger.detected, 360);
        assert_eq!(ledger.escaped, 0);
        assert_eq!(ledger.conserved(), true);
    }

    #[test]
    fn pointer_and_ts_grids() {
        assert_eq!(run_ts_grid(), (8, 8));
        assert_eq!(run_ptr_grid(), (6, 6));
    }

    #[test]
    fn order_precedence() {
        let bad_flag = Request::new(42, 0x2, Timespec::new(1, 0), PtrState::Null, PtrState::Good, false);
        assert_eq!(decide(bad_flag), EINVAL);
        let cpu_bad_ptr = Request::new(2, 0, Timespec::new(1, 0), PtrState::Null, PtrState::Good, false);
        assert_eq!(decide(cpu_bad_ptr), ENOTSUP);
        let ptr_before_range = Request::new(1, 0, Timespec::new(-1, 0), PtrState::Null, PtrState::Good, false);
        assert_eq!(decide(ptr_before_range), EFAULT);
        assert_eq!(run_order_grid(), (6, 6, true));
        assert_eq!(order_count(EINVAL), 3);
        assert_eq!(order_count(EFAULT), 2);
        assert_eq!(order_count(ENOTSUP), 1);
    }

    #[test]
    fn walk_stops_at_declared_step() {
        let bad = walk(Request::new(1, 0x2, Timespec::new(1, 0), PtrState::Good, PtrState::Good, false));
        assert_eq!(bad, (1, EINVAL));
        let unknown = walk(Request::new(42, 0, Timespec::new(1, 0), PtrState::Good, PtrState::Good, false));
        assert_eq!(unknown, (2, EINVAL));
        let cpu = walk(Request::new(2, 0, Timespec::new(1, 0), PtrState::Good, PtrState::Good, false));
        assert_eq!(cpu, (3, ENOTSUP));
        let ok = walk(Request::new(0, 0, Timespec::new(1, 0), PtrState::Good, PtrState::Good, false));
        assert_eq!(ok, (6, OK));
        let intr = walk(Request::new(7, 1, Timespec::new(1, 0), PtrState::Good, PtrState::Good, true));
        assert_eq!(intr, (6, EINTR));
    }

    #[test]
    fn b20_ledger_reconciles() {
        assert_eq!(b20_agreement(), (6, 6));
        assert_eq!(b20_conflicts(), 0);
        assert_eq!(B20_LEDGER.len(), B20_ROWS);
    }

    #[test]
    fn errno_and_fingerprint_are_stable() {
        assert_eq!(errno_known(EINVAL), true);
        assert_eq!(errno_known(0), false);
        assert_eq!(errno_conflicts(), 0);
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
        assert_eq!(corpus_fingerprint() != 0, true);
    }

    #[test]
    fn diagnostic_codes_unique_and_in_window() {
        assert_eq!(ClockCode::ALL.len(), 12);
        let mut i = 0usize;
        while i < ClockCode::ALL.len() {
            if let Some(a) = ClockCode::ALL.get(i) {
                assert_eq!(a.detail().is_empty(), false);
                assert_eq!(a.val() > SEG_BASE, true);
                assert_eq!(a.val() <= SEG_BASE + 0x0C, true);
                let mut j = i + 1;
                while j < ClockCode::ALL.len() {
                    if let Some(b) = ClockCode::ALL.get(j) {
                        assert_eq!(a.val() == b.val(), false);
                        assert_eq!(a.code() == b.code(), false);
                    }
                    j += 1;
                }
            }
            i += 1;
        }
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx9327_checks();
        assert_eq!(s.all_passed(), true, "{}", s.red_items_text());
    }
}
