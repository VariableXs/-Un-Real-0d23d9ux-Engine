//! UNX-F9339 时间/资源族 E 型与 glibc clock 家族消费核 · 0x7C240段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 F9339 逐字）：
//!   ① 纯功能代码 ≥340 行；② 判据原文：UNX-F9339-J1 clock_gettime errno
//!   透传判据过，失效时钟全 EINVAL 可观测，消费号登记格过；③ 完成时
//!   API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//!   ① 时钟登记表 11 行（0..=9 与 11），每行带分辨率与能力位。编号不连号是
//!      Linux 事实（10 留空），表必须照抄这个空洞，否则「失效时钟」的判定
//!      会把 10 当成合法钟。
//!   ② 动态 CPU 时钟号按 Linux 位编码解码：
//!        which = id & 3（0 虚拟 / 1 剖面 / 2 调度，>2 非法）
//!        per_thread = (id & 4) != 0
//!        pid = !(id >> 3)（负号即编码的一部分，算术右移后取反还原）
//!      于是 -1 是「which=3 的非法钟」而不是任意负数，-338 是「pid 42 的
//!      线程调度钟」。把负数一律当坏值是最常见的实现错误，本模块用解码器
//!      把这条钉死。
//!   ③ 判定次序（内核口径）：先定钟、再拷贝时间结构。所以
//!      clock_gettime(999, 空指针) 回 EINVAL 而不是 EFAULT——「失效时钟全
//!      EINVAL 可观测」正是靠这个次序成立。指针面只在钟已确定后才参与。
//!   ④ 三种失败面严格分开：无此钟 → EINVAL；钟存在但目标进程/线程不存在
//!      → ESRCH；钟与目标都对但用户结构不可写 → EFAULT。三者在同一入口
//!      按固定次序产生，互不吞并。
//!   ⑤ 透传：模型返回 Err(e) 时系统调用返回 -e，glibc 只在 -4095..=-1 区间
//!      才认作 errno。-4096 是合法返回值，不得当错误——这是透传判据的边界。
//!   ⑥ 消费号登记 16 槽：钟非法 → EINVAL、号重复 → EEXIST、槽满 → ENFILE，
//!      三面各自记账，accepted + rejected == attempts 恒守恒。
//!   ⑦ 注入三类各 10 轮：错误码被翻译、失效钟误报 EBADF、重复号被放行。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;

use crate::checks::CheckSet;

/// 诊断段基址（F9339 独占 0x7C240..=0x7C27F）。
pub const SEG_BASE: u32 = 0x7C240;

/// 静态时钟行数。
pub const CLOCK_ROWS: usize = 11;

/// 消费号槽位。
pub const CONSUMER_CAP: usize = 16;

/// 单类注入轮数。
pub const INJECT_ROUNDS: u32 = 10;

/// 每秒纳秒（常量除法，非浮点）。
pub const NS_PER_SEC: i64 = 1_000_000_000;

/// glibc 认定的错误区间上界（-1..=-4095）。
pub const GLIBC_ERRNO_MAX: i64 = 4095;

/// 结果摘要（判据③要求非空）。
pub const CONSUMER_SUMMARY: &str = "11 个静态钟 + 动态 CPU 钟解码就绪，errno 全量透传，消费号 12 个已登记";

// ── errno（正数口径） ─────────────────────────────────────────────────
/// 坏描述符。
pub const EBADF: i32 = 9;
/// 目标进程/线程不存在。
pub const ESRCH: i32 = 3;
/// 参数非法（时钟号、时间值）。
pub const EINVAL: i32 = 22;
/// 坏用户指针。
pub const EFAULT: i32 = 14;
/// 操作不允许（写回需要 CAP_SYS_TIME）。
pub const EPERM: i32 = 1;
/// 号重复。
pub const EEXIST: i32 = 17;
/// 打开文件过多（槽满，按消费者句柄口径）。
pub const ENFILE: i32 = 23;

/// errno 登记表：(名称, 码值, 说明)。
pub const ERRNO_TABLE: &[(&str, i32, &str)] = &[
    ("EPERM", EPERM, "写回钟需要 CAP_SYS_TIME"),
    ("ESRCH", ESRCH, "CPU 钟的目标进程不存在"),
    ("EBADF", EBADF, "坏时钟句柄"),
    ("EFAULT", EFAULT, "用户时间结构不可写"),
    ("EEXIST", EEXIST, "消费号重复"),
    ("ENFILE", ENFILE, "消费号槽位耗尽"),
    ("EINVAL", EINVAL, "时钟号或时间值非法"),
];

/// 表内登记判定。
pub fn errno_known(code: i32) -> bool {
    let mut i = 0usize;
    while i < ERRNO_TABLE.len() {
        let hit = match ERRNO_TABLE.get(i) {
            Some(row) => row.1 == code,
            None => false,
        };
        if hit {
            return true;
        }
        i += 1;
    }
    false
}

/// 表内说明（表外兜底非空）。
pub fn errno_note(code: i32) -> &'static str {
    let mut i = 0usize;
    while i < ERRNO_TABLE.len() {
        if let Some(row) = ERRNO_TABLE.get(i) {
            if row.1 == code {
                return row.2;
            }
        }
        i += 1;
    }
    "未登记错误码：拒绝透传"
}

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +1..=+14）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u16)]
pub enum ClockCode {
    /// 段基址越窗。
    SegmentOutOfWindow = 1,
    /// 静态表行数不对。
    RowCountMismatch = 2,
    /// 时钟编号与 Linux 不符。
    RawIdMismatch = 3,
    /// 分辨率非正。
    BadResolution = 4,
    /// CPU 钟解码错位。
    CpuClockDecode = 5,
    /// 判定次序颠倒（指针先于钟）。
    OrderInverted = 6,
    /// 失效钟未回 EINVAL。
    InvalidClockNotEinval = 7,
    /// 目标不存在未回 ESRCH。
    MissingTargetNotEsrch = 8,
    /// 时间值越界未回 EINVAL。
    TimespecOutOfRange = 9,
    /// 透传被翻译（错误码被改写）。
    ErrnoTranslated = 10,
    /// glibc 边界判错（-4096 被当错误）。
    GlibcBoundary = 11,
    /// 消费号重复未拒。
    DuplicateConsumer = 12,
    /// 消费槽满未拒。
    ConsumerOverflow = 13,
    /// 账目不守恒。
    LedgerNotConserved = 14,
}

impl ClockCode {
    /// 序号。
    pub fn code(self) -> u16 {
        self as u16
    }
    /// 码值 = 段基址 + 序号。
    pub fn seg_addr(self) -> u32 {
        SEG_BASE + self.code() as u32
    }
    /// 中文诊断。
    pub fn detail(self) -> &'static str {
        match self {
            ClockCode::SegmentOutOfWindow => "段基址越窗：本卡独占 0x7C240..=0x7C27F",
            ClockCode::RowCountMismatch => "静态钟表行数不是 11：0..=9 与 11，10 必须留空",
            ClockCode::RawIdMismatch => "钟编号与 Linux 不符：编号是 ABI，不能重排",
            ClockCode::BadResolution => "分辨率为零或负：resol_ns 必须为正整数",
            ClockCode::CpuClockDecode => "CPU 钟解码错位：which/pid 的位段被改",
            ClockCode::OrderInverted => "判定次序颠倒：先定钟再拷结构，指针面不得抢先",
            ClockCode::InvalidClockNotEinval => "失效钟未回 EINVAL：坏钟号必须可观测为参数错",
            ClockCode::MissingTargetNotEsrch => "目标不存在未回 ESRCH：pid 未知与钟非法不是一回事",
            ClockCode::TimespecOutOfRange => "时间值越界未回 EINVAL：nsec 必须落在 0..1e9",
            ClockCode::ErrnoTranslated => "透传被翻译：wrapper 只能取反，不得改码",
            ClockCode::GlibcBoundary => "glibc 边界判错：-4096 是合法返回值，不是 errno",
            ClockCode::DuplicateConsumer => "消费号重复未拒：同号二次登记必须 EEXIST",
            ClockCode::ConsumerOverflow => "消费槽满未拒：第 17 个登记必须 ENFILE",
            ClockCode::LedgerNotConserved => "账目不守恒：accepted + rejected 必须等于 attempts",
        }
    }

    /// 闭集全量。
    pub const ALL: &'static [ClockCode] = &[
        ClockCode::SegmentOutOfWindow,
        ClockCode::RowCountMismatch,
        ClockCode::RawIdMismatch,
        ClockCode::BadResolution,
        ClockCode::CpuClockDecode,
        ClockCode::OrderInverted,
        ClockCode::InvalidClockNotEinval,
        ClockCode::MissingTargetNotEsrch,
        ClockCode::TimespecOutOfRange,
        ClockCode::ErrnoTranslated,
        ClockCode::GlibcBoundary,
        ClockCode::DuplicateConsumer,
        ClockCode::ConsumerOverflow,
        ClockCode::LedgerNotConserved,
    ];
}

// ── 时钟登记表 ────────────────────────────────────────────────────────
/// 能力位：受支持。
pub const F_SUPPORTED: u8 = 1;
/// 能力位：单调。
pub const F_MONOTONIC: u8 = 2;
/// 能力位：CPU 时钟。
pub const F_CPU: u8 = 4;
/// 能力位：可写回。
pub const F_SETTABLE: u8 = 8;
/// 能力位：闹钟钟。
pub const F_ALARM: u8 = 16;
/// 能力位：粗粒度。
pub const F_COARSE: u8 = 32;

/// 一行静态时钟。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ClockRow {
    /// ABI 编号（正数）。
    pub raw: i32,
    /// 名称。
    pub name: &'static str,
    /// 分辨率（纳秒）。
    pub resol_ns: i64,
    /// 能力位。
    pub flags: u8,
}

impl ClockRow {
    /// 是否单调。
    pub fn monotonic(&self) -> bool {
        self.flags & F_MONOTONIC != 0
    }
    /// 是否 CPU 钟。
    pub fn cpu(&self) -> bool {
        self.flags & F_CPU != 0
    }
    /// 是否可写回。
    pub fn settable(&self) -> bool {
        self.flags & F_SETTABLE != 0
    }
    /// 是否粗粒度。
    pub fn coarse(&self) -> bool {
        self.flags & F_COARSE != 0
    }
}

/// 十一行静态钟表（10 号位留空，Linux 事实）。
pub const CLOCKS: [ClockRow; CLOCK_ROWS] = [
    ClockRow { raw: 0, name: "CLOCK_REALTIME", resol_ns: 1, flags: F_SUPPORTED | F_SETTABLE | F_ALARM },
    ClockRow { raw: 1, name: "CLOCK_MONOTONIC", resol_ns: 1, flags: F_SUPPORTED | F_MONOTONIC | F_ALARM },
    ClockRow { raw: 2, name: "CLOCK_PROCESS_CPUTIME_ID", resol_ns: 1, flags: F_SUPPORTED | F_CPU },
    ClockRow { raw: 3, name: "CLOCK_THREAD_CPUTIME_ID", resol_ns: 1, flags: F_SUPPORTED | F_CPU },
    ClockRow { raw: 4, name: "CLOCK_MONOTONIC_RAW", resol_ns: 1, flags: F_SUPPORTED | F_MONOTONIC },
    ClockRow { raw: 5, name: "CLOCK_REALTIME_COARSE", resol_ns: 1_000_000, flags: F_SUPPORTED | F_COARSE },
    ClockRow { raw: 6, name: "CLOCK_MONOTONIC_COARSE", resol_ns: 1_000_000, flags: F_SUPPORTED | F_COARSE | F_MONOTONIC },
    ClockRow { raw: 7, name: "CLOCK_BOOTTIME", resol_ns: 1, flags: F_SUPPORTED | F_MONOTONIC | F_ALARM },
    ClockRow { raw: 8, name: "CLOCK_REALTIME_ALARM", resol_ns: 1, flags: F_SUPPORTED | F_ALARM | F_SETTABLE },
    ClockRow { raw: 9, name: "CLOCK_BOOTTIME_ALARM", resol_ns: 1, flags: F_SUPPORTED | F_ALARM | F_MONOTONIC },
    ClockRow { raw: 11, name: "CLOCK_TAI", resol_ns: 1, flags: F_SUPPORTED },
];

/// 静态表查找（只认正编号）。
pub fn lookup(raw: i32) -> Option<ClockRow> {
    if raw < 0 {
        return None;
    }
    let mut i = 0usize;
    while i < CLOCKS.len() {
        if let Some(row) = CLOCKS.get(i) {
            if row.raw == raw {
                return Some(*row);
            }
        }
        i += 1;
    }
    None
}

/// 表内分辨率；表外回 0（调用方据此判参数面错误）。
pub fn resolution(raw: i32) -> i64 {
    match lookup(raw) {
        Some(row) => row.resol_ns,
        None => 0,
    }
}

/// 行名唯一性。
pub fn rows_unique() -> bool {
    let mut i = 0usize;
    while i < CLOCKS.len() {
        let mut j = i + 1;
        while j < CLOCKS.len() {
            if let (Some(a), Some(b)) = (CLOCKS.get(i), CLOCKS.get(j)) {
                if a.name == b.name || a.raw == b.raw {
                    return false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// ── 动态 CPU 时钟号 ───────────────────────────────────────────────────
/// 解码后的 CPU 时钟号。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CpuClock {
    /// 是否线程钟。
    pub per_thread: bool,
    /// which 段（0 虚拟 / 1 剖面 / 2 调度）。
    pub which: u8,
    /// 目标 pid/tid（0 表示当前）。
    pub pid: i32,
}

/// 解码动态 CPU 钟号；which>2 或非负号一律 None。
pub fn decode_cpu_clock(id: i32) -> Option<CpuClock> {
    if id >= 0 {
        return None;
    }
    let which = (id & 3) as u8;
    if which > 2 {
        return None;
    }
    let per_thread = (id & 4) != 0;
    let shifted = id >> 3;
    let pid = !shifted;
    Some(CpuClock { per_thread, which, pid })
}

/// 已知目标集合（本内核视图里的三个活体）。
pub const KNOWN_PIDS: &[i32] = &[1, 42, 4242];

/// 目标是否已知（0 表示当前，恒可用）。
pub fn target_known(pid: i32) -> bool {
    if pid == 0 {
        return true;
    }
    let mut i = 0usize;
    while i < KNOWN_PIDS.len() {
        let hit = match KNOWN_PIDS.get(i) {
            Some(v) => *v == pid,
            None => false,
        };
        if hit {
            return true;
        }
        i += 1;
    }
    false
}

/// 构造进程 CPU 钟号（与内核宏同构）。
///
/// 位段冻结：bit0..1 = which，bit2 = per_thread（由 make_thread_cpu_clock
/// 置位传入），故低三位掩码是 7 而不是 3——掩 3 会把 per_thread 位抹掉。
pub fn make_process_cpu_clock(pid: i32, which: u8) -> i32 {
    ((!pid) << 3) | (which as i32 & 7)
}

/// 构造线程 CPU 钟号。
pub fn make_thread_cpu_clock(tid: i32, which: u8) -> i32 {
    make_process_cpu_clock(tid, which | 4)
}

// ── 时间值与调用模型 ──────────────────────────────────────────────────
/// 时间结构。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Timespec {
    /// 秒。
    pub sec: i64,
    /// 纳秒（0..1e9）。
    pub nsec: i64,
}

impl Timespec {
    /// 合法：秒非负、纳秒在 [0,1e9)。
    pub fn valid(&self) -> bool {
        self.sec >= 0 && self.nsec >= 0 && self.nsec < NS_PER_SEC
    }
    /// 以纳秒总计表达（溢出饱和，不回绕）。
    pub fn total_ns(&self) -> i64 {
        self.sec.saturating_mul(NS_PER_SEC).saturating_add(self.nsec)
    }
}

/// 规范化：纳秒越界回 EINVAL，绝不静默进位或截断。
pub fn normalize(sec: i64, nsec: i64) -> Result<Timespec, i32> {
    let ts = Timespec { sec, nsec };
    if ts.valid() {
        Ok(ts)
    } else {
        Err(EINVAL)
    }
}

/// 由固定虚拟时钟派生某钟的读数（无墙钟、无随机）。
pub fn read_clock(raw: i32, now_ns: i64) -> Result<Timespec, i32> {
    // 偏移取绝对值：负编号是编码的一部分，不能让读数落到负时间上。
    let mag = if raw < 0 { -(raw as i64) } else { raw as i64 };
    let skew = mag.saturating_mul(1_000_000);
    let total = now_ns.saturating_add(skew);
    if total < 0 {
        return Err(EINVAL);
    }
    let sec = total / NS_PER_SEC;
    let nsec = total % NS_PER_SEC;
    normalize(sec, nsec)
}

/// clock_gettime 模型。
///
/// 判定次序固定：钟面（EINVAL/ESRCH）→ 指针面（EFAULT）→ 值面（EINVAL）。
/// 次序就是语义：坏钟号与空指针同时出现时必须报 EINVAL。
pub fn clock_gettime(raw: i32, ptr_ok: bool, now_ns: i64) -> Result<Timespec, i32> {
    match lookup(raw) {
        Some(_row) => {}
        None => match decode_cpu_clock(raw) {
            Some(c) => {
                if !target_known(c.pid) {
                    return Err(ESRCH);
                }
            }
            None => return Err(EINVAL),
        },
    }
    if !ptr_ok {
        return Err(EFAULT);
    }
    read_clock(raw, now_ns)
}

/// 系统调用返回码：成功 0，失败 -errno。
pub fn syscall_ret(raw: i32, ptr_ok: bool, now_ns: i64) -> i64 {
    match clock_gettime(raw, ptr_ok, now_ns) {
        Ok(_ts) => 0,
        Err(e) => -(e as i64),
    }
}

/// glibc 的 errno 认定：仅 -1..=-4095 视为错误。
pub fn glibc_errno(ret: i64) -> Option<i32> {
    if ret < 0 && ret >= -GLIBC_ERRNO_MAX {
        Some((-ret) as i32)
    } else {
        None
    }
}

/// 透传判定：模型错误码、返回码、glibc 认定三者必须同码闭环。
pub fn passthrough_ok(raw: i32, ptr_ok: bool, now_ns: i64) -> bool {
    match clock_gettime(raw, ptr_ok, now_ns) {
        Ok(_ts) => {
            let ret = syscall_ret(raw, ptr_ok, now_ns);
            ret == 0 && glibc_errno(ret).is_none()
        }
        Err(e) => {
            let ret = syscall_ret(raw, ptr_ok, now_ns);
            ret == -(e as i64) && glibc_errno(ret) == Some(e)
        }
    }
}

/// 失效钟集合（正数未登记 + which=3 的负号）。
pub const DEAD_CLOCKS: &[i32] = &[12, 13, 24, 100, -1, -5, -9, i32::MAX];

/// 钟号是否可用：静态表命中，或动态 CPU 钟且目标存在。
pub fn clock_known(raw: i32) -> bool {
    if lookup(raw).is_some() {
        return true;
    }
    match decode_cpu_clock(raw) {
        Some(c) => target_known(c.pid),
        None => false,
    }
}

/// 注入用错误实现：失效钟一律回 EBADF（最常见的「句柄化」误实现）。
pub fn faulty_dead_clock(_raw: i32) -> Result<Timespec, i32> {
    Err(EBADF)
}

/// 注入用错误翻译：EINVAL 被「兼容层」翻成 EBADF。
pub fn translate_errno(e: i32) -> i32 {
    if e == EINVAL {
        EBADF
    } else {
        e
    }
}

// ── 消费号登记 ────────────────────────────────────────────────────────
/// 一个消费号。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Consumer {
    /// 消费号（0..=15）。
    pub id: u8,
    /// 消费的钟号。
    pub raw: i32,
    /// 登记顺序。
    pub seq: u16,
}

/// 空槽哨兵（`[None; N]` 要求 Copy，这里显式给常量）。
const EMPTY_SLOT: Option<Consumer> = None;

/// 定容消费号表。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Registry {
    slots: [Option<Consumer>; CONSUMER_CAP],
    used: usize,
    attempts: u32,
    accepted: u32,
    rejected: u32,
}

impl Registry {
    /// 空表。
    pub const fn new() -> Registry {
        Registry {
            slots: [EMPTY_SLOT; CONSUMER_CAP],
            used: 0,
            attempts: 0,
            accepted: 0,
            rejected: 0,
        }
    }
    /// 已用槽位。
    pub fn used(&self) -> usize {
        self.used
    }
    /// 尝试次数。
    pub fn attempts(&self) -> u32 {
        self.attempts
    }
    /// 接受次数。
    pub fn accepted(&self) -> u32 {
        self.accepted
    }
    /// 拒绝次数。
    pub fn rejected(&self) -> u32 {
        self.rejected
    }
    /// 取槽。
    pub fn slot(&self, i: usize) -> Option<Consumer> {
        match self.slots.get(i) {
            Some(v) => *v,
            None => None,
        }
    }
    /// 账目守恒。
    pub fn conserved(&self) -> bool {
        self.accepted.saturating_add(self.rejected) == self.attempts
    }
    /// 某号是否已登记。
    pub fn contains(&self, id: u8) -> bool {
        let mut i = 0usize;
        while i < CONSUMER_CAP {
            let hit = match self.slots.get(i) {
                Some(Some(c)) => c.id == id,
                _ => false,
            };
            if hit {
                return true;
            }
            i += 1;
        }
        false
    }

    /// 登记：钟面 → 重复面 → 容量面，三面各自回不同码。
    pub fn register(&mut self, id: u8, raw: i32, seq: u16) -> Result<usize, i32> {
        self.attempts = self.attempts.saturating_add(1);
        if !clock_known(raw) {
            self.rejected = self.rejected.saturating_add(1);
            return Err(EINVAL);
        }
        if (id as usize) >= CONSUMER_CAP {
            self.rejected = self.rejected.saturating_add(1);
            return Err(EINVAL);
        }
        // 容量先于重复：满表时任何登记都回 ENFILE，不再区分号是否重复。
        if self.used >= CONSUMER_CAP {
            self.rejected = self.rejected.saturating_add(1);
            return Err(ENFILE);
        }
        if self.contains(id) {
            self.rejected = self.rejected.saturating_add(1);
            return Err(EEXIST);
        }
        let mut i = 0usize;
        while i < CONSUMER_CAP {
            let free = match self.slots.get(i) {
                Some(None) => true,
                _ => false,
            };
            if free {
                if let Some(slot) = self.slots.get_mut(i) {
                    *slot = Some(Consumer { id, raw, seq });
                }
                self.used += 1;
                self.accepted = self.accepted.saturating_add(1);
                return Ok(i);
            }
            i += 1;
        }
        self.rejected = self.rejected.saturating_add(1);
        Err(ENFILE)
    }

    /// 故障注入入口：跳过钟面与重复面检查直接落槽（只用于负向判据）。
    pub fn force_insert(&mut self, id: u8, raw: i32, seq: u16) -> Result<usize, i32> {
        self.attempts = self.attempts.saturating_add(1);
        let mut i = 0usize;
        while i < CONSUMER_CAP {
            let free = match self.slots.get(i) {
                Some(None) => true,
                _ => false,
            };
            if free {
                if let Some(slot) = self.slots.get_mut(i) {
                    *slot = Some(Consumer { id, raw, seq });
                }
                self.used += 1;
                self.accepted = self.accepted.saturating_add(1);
                return Ok(i);
            }
            i += 1;
        }
        self.rejected = self.rejected.saturating_add(1);
        Err(ENFILE)
    }
}

impl Default for Registry {
    fn default() -> Registry {
        Registry::new()
    }
}

/// 规范登记语料：十二个消费号（11 个静态钟 + 1 个动态 CPU 钟）。
pub fn register_corpus() -> Registry {
    let mut r = Registry::new();
    let mut i = 0usize;
    while i < CLOCKS.len() {
        if let Some(row) = CLOCKS.get(i) {
            let id = (i % CONSUMER_CAP) as u8;
            if let Err(_code) = r.register(id, row.raw, i as u16 + 1) {
                // 语料内的拒绝由 accepted 计数与守恒判据负责报红。
            }
        }
        i += 1;
    }
    let dyn_id = (CLOCKS.len() % CONSUMER_CAP) as u8;
    let dyn_raw = make_process_cpu_clock(42, 0);
    if let Err(_code) = r.register(dyn_id, dyn_raw, CLOCKS.len() as u16 + 1) {
        // 同上：动态钟登记失败必须体现在 accepted 账里。
    }
    r
}

// ── 注入（三类各 10 轮） ──────────────────────────────────────────────
/// 注入变体。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mutant {
    /// 错误码被「兼容层」翻译（EINVAL→EBADF）。
    TranslateErrno,
    /// 失效钟误报 EBADF。
    DeadClockAsBadFd,
    /// 重复消费号被放行。
    DuplicateConsumer,
}

impl Mutant {
    /// 闭集。
    pub const ALL: &'static [Mutant] = &[
        Mutant::TranslateErrno,
        Mutant::DeadClockAsBadFd,
        Mutant::DuplicateConsumer,
    ];

    /// 标签。
    pub fn tag(self) -> u8 {
        match self {
            Mutant::TranslateErrno => 0,
            Mutant::DeadClockAsBadFd => 1,
            Mutant::DuplicateConsumer => 2,
        }
    }

    /// 中文说明。
    pub fn detail(self) -> &'static str {
        match self {
            Mutant::TranslateErrno => "兼容层把 EINVAL 翻成 EBADF",
            Mutant::DeadClockAsBadFd => "失效钟返回 EBADF 而非 EINVAL",
            Mutant::DuplicateConsumer => "同号二次登记被放行",
        }
    }

    /// 是否被现有判定抓到：把注入的错误实现与冻结要求逐项对比，分歧即检出。
    pub fn detect(self) -> bool {
        match self {
            Mutant::TranslateErrno => {
                let mut diverged = false;
                let mut i = 0usize;
                while i < DEAD_CLOCKS.len() {
                    if let Some(raw) = DEAD_CLOCKS.get(i) {
                        if let Err(e) = clock_gettime(*raw, true, 42) {
                            let honest = -(e as i64);
                            let faulty = -(translate_errno(e) as i64);
                            if honest != faulty {
                                diverged = true;
                            }
                        }
                    }
                    i += 1;
                }
                diverged
            }
            Mutant::DeadClockAsBadFd => {
                let mut diverged = false;
                let mut i = 0usize;
                while i < DEAD_CLOCKS.len() {
                    if let Some(raw) = DEAD_CLOCKS.get(i) {
                        if clock_gettime(*raw, true, 42) != faulty_dead_clock(*raw) {
                            diverged = true;
                        }
                    }
                    i += 1;
                }
                diverged
            }
            Mutant::DuplicateConsumer => {
                let mut strict = Registry::new();
                let s1 = strict.register(0, 0, 1);
                let s2 = strict.register(0, 1, 2);
                let mut lenient = Registry::new();
                let l1 = lenient.force_insert(0, 0, 1);
                let l2 = lenient.force_insert(0, 1, 2);
                s1.is_ok()
                    && s2 == Err(EEXIST)
                    && strict.used() == 1
                    && l1.is_ok()
                    && l2.is_ok()
                    && lenient.used() == 2
            }
        }
    }
}

/// 带翻译的透传演练：正确实现下返回 true（错误码未被改写）。
pub fn translate_eq_ok(translate: fn(i32) -> i32) -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < DEAD_CLOCKS.len() {
        if let Some(raw) = DEAD_CLOCKS.get(i) {
            match clock_gettime(*raw, true, 42) {
                Ok(_ts) => {}
                Err(e) => {
                    let ret = -(e as i64);
                    let translated = -(translate(e) as i64);
                    if translated != ret || glibc_errno(translated) != Some(translate(e)) {
                        ok = false;
                    }
                }
            }
        }
        i += 1;
    }
    ok
}

/// 注入账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectLog {
    /// 注入。
    pub injected: u32,
    /// 检出。
    pub detected: u32,
    /// 逃逸。
    pub escaped: u32,
}

impl InjectLog {
    /// 空账。
    pub const fn zero() -> InjectLog {
        InjectLog { injected: 0, detected: 0, escaped: 0 }
    }
    /// 记一笔。
    pub fn record(&mut self, caught: bool) {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
    }
    /// 守恒。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

impl Default for InjectLog {
    fn default() -> InjectLog {
        InjectLog::zero()
    }
}

/// 三类 × 10 轮。
pub fn run_injections() -> (InjectLog, [u32; 3]) {
    let mut log = InjectLog::zero();
    let mut per = [0u32; 3];
    let mut round = 0u32;
    while round < INJECT_ROUNDS {
        let mut i = 0usize;
        while i < Mutant::ALL.len() {
            if let Some(m) = Mutant::ALL.get(i) {
                let caught = m.detect();
                log.record(caught);
                if caught {
                    let slot = (m.tag() as usize) % 3;
                    if let Some(v) = per.get_mut(slot) {
                        *v = v.saturating_add(1);
                    }
                }
            }
            i += 1;
        }
        round += 1;
    }
    (log, per)
}

// ── 指纹 ──────────────────────────────────────────────────────────────
/// FNV-1a 32 位续算。
pub fn chain(h: u32, bytes: &[u8]) -> u32 {
    let mut acc = h;
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            acc ^= *b as u32;
            acc = acc.wrapping_mul(0x0100_0193);
        }
        i += 1;
    }
    acc
}

/// 钟表指纹：编号 + 名 + 分辨率 + 能力位。
pub fn table_digest() -> u32 {
    let mut h = chain(0x811C_9DC5, b"unx9339/clocks");
    let mut i = 0usize;
    while i < CLOCKS.len() {
        if let Some(row) = CLOCKS.get(i) {
            h = chain(h, row.name.as_bytes());
            h = chain(h, &row.raw.to_le_bytes());
            h = chain(h, &row.resol_ns.to_le_bytes());
            h = chain(h, &[row.flags]);
        }
        i += 1;
    }
    h
}

/// 语料指纹：钟表 + 十二次登记结果。
pub fn corpus_digest() -> u32 {
    let mut h = table_digest();
    let r = register_corpus();
    h = chain(h, &r.used().to_le_bytes());
    h = chain(h, &r.accepted().to_le_bytes());
    let mut i = 0usize;
    while i < CONSUMER_CAP {
        if let Some(c) = r.slot(i) {
            h = chain(h, &[c.id]);
            h = chain(h, &c.raw.to_le_bytes());
            h = chain(h, &c.seq.to_le_bytes());
        }
        i += 1;
    }
    h
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9339 判据集。
pub fn run_unx9339_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9339");

    let seg_ok = SEG_BASE == 0x7C240 && SEG_BASE % 0x40 == 0 && SEG_BASE < 0x7C280;
    s.add("段位守卫", seg_ok, "0x7C240 独占窗口且 0x40 对齐");

    let mut code_ok = ClockCode::ALL.len() == 14;
    let mut i = 0usize;
    while i < ClockCode::ALL.len() {
        let mut j = i + 1;
        while j < ClockCode::ALL.len() {
            if let (Some(a), Some(b)) = (ClockCode::ALL.get(i), ClockCode::ALL.get(j)) {
                if a.seg_addr() == b.seg_addr() || a.detail() == b.detail() || a.detail().is_empty() {
                    code_ok = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    s.add("码窗与文案", code_ok, "14 个码值唯一、文案互异且全部落在段窗内");

    s.add(
        "静态钟行数",
        CLOCKS.len() == CLOCK_ROWS && rows_unique(),
        "11 行（0..=9 与 11），编号与名称互异",
    );

    let numbering_ok = (0..10).all(|k| lookup(k).is_some())
        && lookup(10).is_none()
        && lookup(11).is_some()
        && lookup(12).is_none();
    s.add("编号空洞", numbering_ok, "10 号位必须留空，12 号起未登记");

    let resol_ok = CLOCKS.iter().all(|r| r.resol_ns > 0)
        && resolution(5) == 1_000_000
        && resolution(6) == 1_000_000
        && resolution(0) == 1;
    s.add("分辨率", resol_ok, "全表分辨率为正，粗粒度钟为 1ms");

    let cpu_mask_ok = lookup(0).map(|r| !r.cpu()).unwrap_or(false)
        && lookup(2).map(|r| r.cpu()).unwrap_or(false)
        && lookup(3).map(|r| r.cpu()).unwrap_or(false)
        && lookup(1).map(|r| r.monotonic()).unwrap_or(false)
        && lookup(0).map(|r| !r.monotonic()).unwrap_or(false);
    s.add("能力位", cpu_mask_ok, "CPU 钟标记只落在 2/3 号，单调位与 Linux 一致");

    let dec_p = decode_cpu_clock(make_process_cpu_clock(42, 0));
    let dec_t = decode_cpu_clock(make_thread_cpu_clock(42, 2));
    let dec_bad = decode_cpu_clock(-1).is_none() && decode_cpu_clock(7).is_none();
    let decode_ok = match (dec_p, dec_t) {
        (Some(p), Some(t)) => {
            p.pid == 42
                && !p.per_thread
                && p.which == 0
                && t.pid == 42
                && t.per_thread
                && t.which == 2
                && dec_bad
                && make_process_cpu_clock(42, 0) == -344
        }
        _ => false,
    };
    s.add("CPU 钟解码", decode_ok, "pid/which/per_thread 三字段往返一致，-1 判非法");

    let order_ok = clock_gettime(999, false, 42) == Err(EINVAL)
        && clock_gettime(1, false, 42) == Err(EFAULT)
        && clock_gettime(make_process_cpu_clock(42, 0), false, 42) == Err(EFAULT);
    s.add("判定次序", order_ok, "坏钟先于坏指针：EINVAL 压过 EFAULT");

    let mut dead_all = true;
    let mut di = 0usize;
    while di < DEAD_CLOCKS.len() {
        if let Some(raw) = DEAD_CLOCKS.get(di) {
            if clock_gettime(*raw, true, 42) != Err(EINVAL) {
                dead_all = false;
            }
        }
        di += 1;
    }
    s.add("失效钟全 EINVAL", dead_all && DEAD_CLOCKS.len() == 8, "八个失效编号逐一回 EINVAL，无一例外");

    let mut esrch_all = true;
    let mut ei = 0usize;
    while ei < KNOWN_PIDS.len() {
        if let Some(pid) = KNOWN_PIDS.get(ei) {
            if clock_gettime(make_process_cpu_clock(*pid, 0), true, 42).is_err() {
                esrch_all = false;
            }
        }
        ei += 1;
    }
    let unknown = clock_gettime(make_process_cpu_clock(7777, 0), true, 42);
    s.add(
        "目标存在性",
        esrch_all && unknown == Err(ESRCH) && clock_gettime(-1, true, 42) == Err(EINVAL),
        "已知 pid 可取钟，未知 pid 回 ESRCH，非法 which 回 EINVAL",
    );

    let norm_ok = normalize(0, 0).is_ok()
        && normalize(0, NS_PER_SEC - 1).is_ok()
        && normalize(0, NS_PER_SEC) == Err(EINVAL)
        && normalize(0, -1) == Err(EINVAL)
        && normalize(-1, 0) == Err(EINVAL);
    s.add("时间值值域", norm_ok, "nsec 必须落在 [0,1e9)，越界与负秒一律 EINVAL");

    let pass_ok = passthrough_ok(999, true, 42)
        && passthrough_ok(1, false, 42)
        && passthrough_ok(make_process_cpu_clock(7777, 0), true, 42)
        && passthrough_ok(1, true, 42)
        && syscall_ret(999, true, 42) == -(EINVAL as i64);
    s.add("errno 透传", pass_ok, "模型码 → 返回码 → glibc 认定三者同码闭环");

    let bound_ok = glibc_errno(-1) == Some(1)
        && glibc_errno(-4095) == Some(4095)
        && glibc_errno(-4096).is_none()
        && glibc_errno(0).is_none()
        && glibc_errno(7).is_none();
    s.add("glibc 边界", bound_ok, "-4095 是 errno，-4096 不是；成功返回 0 不设 errno");

    let reg = register_corpus();
    s.add(
        "消费号登记",
        reg.used() == 12 && reg.accepted() == 12 && reg.rejected() == 0 && reg.conserved(),
        "12 个消费号（11 静态 + 1 动态）全部落槽，账目守恒",
    );

    let mut dup = Registry::new();
    let first = dup.register(3, 1, 1);
    let second = dup.register(3, 1, 2);
    let bad_clock = dup.register(4, 10, 3);
    s.add(
        "重复号与坏钟",
        first.is_ok() && second == Err(EEXIST) && bad_clock == Err(EINVAL) && dup.used() == 1,
        "重复号 EEXIST、未登记钟 EINVAL，失败不占槽",
    );

    let mut fill = Registry::new();
    let mut n = 0u8;
    let mut ok_fill = true;
    while (n as usize) < CONSUMER_CAP {
        if fill.register(n, 1, n as u16 + 1).is_err() {
            ok_fill = false;
        }
        n += 1;
    }
    let over = fill.register(0, 1, 99);
    s.add(
        "槽满拒绝",
        ok_fill && fill.used() == CONSUMER_CAP && over == Err(ENFILE) && fill.conserved(),
        "16 槽填满后第 17 次登记回 ENFILE",
    );

    let (log, per) = run_injections();
    s.add(
        "注入 10/10",
        log.injected == 30 && log.detected == 30 && log.escaped == 0 && log.conserved(),
        "三类变体各 10 轮全检出，逃逸 0",
    );

    let per_ok = per.iter().all(|v| *v == INJECT_ROUNDS);
    s.add("分类命中", per_ok, "每类检出数均为 10");

    let errno_ok = ERRNO_TABLE.len() == 7
        && errno_known(EINVAL)
        && errno_known(ESRCH)
        && !errno_known(0)
        && !errno_note(EBADF).is_empty()
        && !errno_note(38).is_empty();
    s.add("errno 登记表", errno_ok, "七码登记齐全，表外码文案仍非空");

    let d1 = corpus_digest();
    let d2 = corpus_digest();
    s.add("双跑一致", d1 == d2 && d1 != 0 && table_digest() == table_digest(), "钟表与语料指纹双跑相等");

    let mut variant = register_corpus();
    let extra = variant.register(0, 0, 90);
    s.add(
        "语料可扩",
        extra == Err(EEXIST) && variant.conserved(),
        "语料 0 号已被占用，再登记走 EEXIST 且账目守恒",
    );

    s.add_dyn("钟表快照", CLOCKS.len() == CLOCK_ROWS, &format!("静态钟 {} 行 / 指纹 0x{:08X}", CLOCKS.len(), table_digest()));
    s.add_dyn("登记快照", reg.used() == 12, &format!("已用 {}/16 接受 {} 拒绝 {}", reg.used(), reg.accepted(), reg.rejected()));
    s.add_dyn("注入快照", log.conserved(), &format!("注入 {} 检出 {} 逃逸 {}", log.injected, log.detected, log.escaped));

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checks_all_green() {
        let s = run_unx9339_checks();
        assert_eq!(s.all_passed(), true, "{}", s.red_items_text());
        assert_eq!(s.truncated(), false);
        assert_eq!(s.len() >= 12, true);
    }

    #[test]
    fn clock_table_has_a_hole_at_ten() {
        assert_eq!(CLOCKS.len(), CLOCK_ROWS);
        assert_eq!(rows_unique(), true);
        assert_eq!(lookup(10).is_none(), true);
        assert_eq!(lookup(9).is_some(), true);
        assert_eq!(lookup(11).is_some(), true);
        assert_eq!(lookup(-1).is_none(), true);
        assert_eq!(resolution(5), 1_000_000);
        assert_eq!(resolution(99), 0);
    }

    #[test]
    fn cpu_clock_decoder_round_trips() {
        let p = match decode_cpu_clock(make_process_cpu_clock(42, 0)) {
            Some(v) => v,
            None => {
                assert_eq!(true, false);
                return;
            }
        };
        assert_eq!(p.pid, 42);
        assert_eq!(p.which, 0);
        assert_eq!(p.per_thread, false);
        let t = match decode_cpu_clock(make_thread_cpu_clock(42, 2)) {
            Some(v) => v,
            None => {
                assert_eq!(true, false);
                return;
            }
        };
        assert_eq!(t.pid, 42);
        assert_eq!(t.which, 2);
        assert_eq!(t.per_thread, true);
        assert_eq!(decode_cpu_clock(-1).is_none(), true);
        assert_eq!(decode_cpu_clock(0).is_none(), true);
    }

    #[test]
    fn clock_before_pointer_order() {
        assert_eq!(clock_gettime(999, false, 42), Err(EINVAL));
        assert_eq!(clock_gettime(1, false, 42), Err(EFAULT));
        assert_eq!(clock_gettime(-1, false, 42), Err(EINVAL));
        let good = clock_gettime(1, true, 42);
        assert_eq!(good.is_ok(), true);
    }

    #[test]
    fn dead_clocks_are_all_einval() {
        let mut i = 0usize;
        while i < DEAD_CLOCKS.len() {
            if let Some(raw) = DEAD_CLOCKS.get(i) {
                assert_eq!(clock_gettime(*raw, true, 42), Err(EINVAL));
                assert_eq!(syscall_ret(*raw, true, 42), -(EINVAL as i64));
            }
            i += 1;
        }
    }

    #[test]
    fn unknown_target_is_esrch_not_einval() {
        assert_eq!(clock_gettime(make_process_cpu_clock(7777, 1), true, 42), Err(ESRCH));
        let mut i = 0usize;
        while i < KNOWN_PIDS.len() {
            if let Some(pid) = KNOWN_PIDS.get(i) {
                assert_eq!(clock_gettime(make_process_cpu_clock(*pid, 0), true, 42).is_ok(), true);
            }
            i += 1;
        }
    }

    #[test]
    fn timespec_range_is_strict() {
        assert_eq!(normalize(0, NS_PER_SEC - 1).is_ok(), true);
        assert_eq!(normalize(0, NS_PER_SEC), Err(EINVAL));
        assert_eq!(normalize(0, -5), Err(EINVAL));
        assert_eq!(normalize(-2, 0), Err(EINVAL));
        let ts = Timespec { sec: 2, nsec: 3 };
        assert_eq!(ts.valid(), true);
        assert_eq!(ts.total_ns(), 2 * NS_PER_SEC + 3);
    }

    #[test]
    fn passthrough_and_glibc_boundary() {
        assert_eq!(glibc_errno(-4095), Some(4095));
        assert_eq!(glibc_errno(-4096).is_none(), true);
        assert_eq!(glibc_errno(0).is_none(), true);
        assert_eq!(passthrough_ok(999, true, 42), true);
        assert_eq!(passthrough_ok(1, true, 42), true);
        assert_eq!(syscall_ret(1, true, 42), 0);
        assert_eq!(syscall_ret(12, true, 42), -(EINVAL as i64));
    }

    #[test]
    fn registry_registers_twelve() {
        let r = register_corpus();
        assert_eq!(r.used(), 12);
        assert_eq!(r.accepted(), 12);
        assert_eq!(r.rejected(), 0);
        assert_eq!(r.conserved(), true);
        assert_eq!(r.attempts(), 12);
        assert_eq!(r.contains(0), true);
        assert_eq!(r.contains(15), false);
    }

    #[test]
    fn registry_rejects_dup_bad_clock_and_overflow() {
        let mut r = Registry::new();
        assert_eq!(r.register(2, 1, 1), Ok(0));
        assert_eq!(r.register(2, 1, 2), Err(EEXIST));
        assert_eq!(r.register(3, 10, 3), Err(EINVAL));
        assert_eq!(r.register(CONSUMER_CAP as u8, 1, 4), Err(EINVAL));
        assert_eq!(r.used(), 1);
        let mut f = Registry::new();
        let mut n = 0u8;
        while (n as usize) < CONSUMER_CAP {
            assert_eq!(f.register(n, 1, n as u16 + 1).is_ok(), true);
            n += 1;
        }
        assert_eq!(f.register(0, 1, 77), Err(ENFILE));
        assert_eq!(f.used(), CONSUMER_CAP);
    }

    #[test]
    fn injections_all_detected() {
        let (log, per) = run_injections();
        assert_eq!(log.injected, 30);
        assert_eq!(log.detected, 30);
        assert_eq!(log.escaped, 0);
        assert_eq!(per, [10, 10, 10]);
        assert_eq!(translate_eq_ok(|e| e), true);
        assert_eq!(translate_eq_ok(|e| if e == EINVAL { EBADF } else { e }), false);
    }

    #[test]
    fn digest_is_deterministic() {
        assert_eq!(table_digest(), table_digest());
        assert_eq!(corpus_digest(), corpus_digest());
        assert_ne!(corpus_digest(), 0);
        assert_eq!(Mutant::ALL.len(), 3);
        assert_eq!(ClockCode::ALL.len(), 14);
        assert_ne!(ClockCode::ALL.len(), 0);
    }
}
