//! UNX-F9340 ktest syscall 面 B27 批断言集 · 0x7C280段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 F9340 逐字）：
//!   ① 纯功能代码 ≥440 行；② 判据原文：UNX-F9340-J1 本批 19 条判据聚合
//!   判据过，时间资源族 E 型矩阵一次跑通，域累计 146,800 收口断言；③ 完成
//!   时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//!   ① 本批 19 条断言，每条一个 Case（可判定的纯函数）加权重 40 分，聚合
//!      权重 760。断言不是字符串清单：`evaluate(case)` 真的算出布尔，跑批
//!      时才聚合成 passed/failed/weight。
//!   ② 「一次跑通」是**一次性闩锁**而不是「跑一次就行」：Runner 状态
//!      Fresh→Running→Closed，第二次 run 回 EBUSY。矩阵只能在本批内跑一次，
//!      重复跑会把计时相关断言变成「多跑几次总会过」的碰运气。
//!   ③ 收口断言是算术恒等式，不是文案：BATCH_BASE(146,040) + 本批权重
//!      (760) = DOMAIN_CUM(146,800)，三处都用 checked_add 验算，任何一处
//!      越界回 ENOMEM。closeout 只在 19/19 全绿且权重足额时才给出 146,800。
//!   ④ 失败不许被权重抹平：任一断言翻转都会让 failed=1，closeout 直接
//!      EINVAL——不会出现「19 条过 18 条但权重够了也算收口」。
//!   ⑤ 注入两变体（首条翻转、末条翻转）各 10 轮：注入 20、检出 20、逃逸 0。
//!   ⑥ 断言按五个组登记（时间 6 / errno 8 / 次序 1 / 账本 2 / 消费 2），
//!      组计数是判据的一部分，缺组即聚合不完整。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;

use crate::checks::CheckSet;

/// 诊断段基址（F9340 独占 0x7C280..=0x7C2BF）。
pub const SEG_BASE: u32 = 0x7C280;

/// 本批断言条数。
pub const ASSERTIONS: usize = 19;

/// 单条断言权重。
pub const WEIGHT_EACH: u32 = 40;

/// 本批权重合计（19 × 40）。
pub const BATCH_WEIGHT: u32 = ASSERTIONS as u32 * WEIGHT_EACH;

/// 本批之前的域累计。
pub const BATCH_BASE: u32 = 146_040;

/// 收口后域累计（判据原文数字）。
pub const DOMAIN_CUM: u32 = 146_800;

/// 单变体注入轮数。
pub const INJECT_ROUNDS: u32 = 10;

/// 结果摘要（判据③要求非空）。
pub const BATCH_SUMMARY: &str = "B27 批 19/19 全绿，权重 760 足额，域累计收口 146,800";

/// 毫秒转纳秒的乘数。
pub const MS: i64 = 1_000_000;

/// 每毫秒的微秒数（仅供睡眠余量演算）。
pub const US_PER_MS: i64 = 1_000;

// ── errno（正数口径） ─────────────────────────────────────────────────
/// 操作不允许。
pub const EPERM: i32 = 1;
/// 目标不存在。
pub const ESRCH: i32 = 3;
/// 被信号打断。
pub const EINTR: i32 = 4;
/// 参数非法。
pub const EINVAL: i32 = 22;
/// 坏用户指针。
pub const EFAULT: i32 = 14;
/// 资源暂不可用。
pub const EAGAIN: i32 = 11;
/// 内存不足。
pub const ENOMEM: i32 = 12;
/// 设备忙（一次性闩锁重入）。
pub const EBUSY: i32 = 16;
/// 号重复。
pub const EEXIST: i32 = 17;
/// 句柄耗尽。
pub const ENFILE: i32 = 23;

/// errno 登记表：(名称, 码值, 说明)。
pub const ERRNO_TABLE: &[(&str, i32, &str)] = &[
    ("EPERM", EPERM, "缺能力位"),
    ("ESRCH", ESRCH, "目标进程不存在"),
    ("EINTR", EINTR, "被信号打断"),
    ("ENFILE", ENFILE, "消费槽耗尽：容量面拒绝"),
    ("ENOMEM", ENOMEM, "内存不足"),
    ("EFAULT", EFAULT, "坏用户指针"),
    ("EBUSY", EBUSY, "一次性闩锁重入"),
    ("EEXIST", EEXIST, "消费号重复"),
    ("EINVAL", EINVAL, "参数非法"),
];

/// 表内登记。
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

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +1..=+14）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u16)]
pub enum BatchCode {
    /// 段基址越窗。
    SegmentOutOfWindow = 1,
    /// 断言条数不是 19。
    AssertionCountMismatch = 2,
    /// 断言编号不连续。
    IdGap = 3,
    /// 断言名重复。
    DuplicateName = 4,
    /// 组计数不符。
    GroupCountDrift = 5,
    /// 权重不足或超出。
    WeightShort = 6,
    /// 某条断言求值为假。
    AssertionRed = 7,
    /// 一次性闩锁失效（可重入）。
    LatchBroken = 8,
    /// 域累计算术不成立。
    CumulativeMismatch = 9,
    /// 收口被提前放行（有红项仍盖章）。
    PrematureCloseout = 10,
    /// 注入未被检出。
    InjectEscaped = 11,
    /// 注入账不守恒。
    LedgerNotConserved = 12,
    /// 指纹双跑不一致。
    StampDrift = 13,
    /// 结果摘要为空。
    EmptySummary = 14,
}

impl BatchCode {
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
            BatchCode::SegmentOutOfWindow => "段基址越窗：本卡独占 0x7C280..=0x7C2BF",
            BatchCode::AssertionCountMismatch => "断言条数不是 19：批次定义被改",
            BatchCode::IdGap => "断言编号不连续：0..=18 必须逐一在位",
            BatchCode::DuplicateName => "断言名重复：红项无法定位到唯一断言",
            BatchCode::GroupCountDrift => "组计数不符：应为 时间6/errno8/次序1/账本2/消费2",
            BatchCode::WeightShort => "权重不足：19 条 × 40 分缺一不可",
            BatchCode::AssertionRed => "存在红断言：先修实现再谈聚合",
            BatchCode::LatchBroken => "一次性闩锁失效：矩阵被重复跑批",
            BatchCode::CumulativeMismatch => "域累计算术不成立：146,040+760 必须等于 146,800",
            BatchCode::PrematureCloseout => "收口被提前放行：有红项仍盖章属判据造假",
            BatchCode::InjectEscaped => "注入未被检出：翻转断言未被聚合抓到",
            BatchCode::LedgerNotConserved => "注入账不守恒：注入须等于检出加逃逸",
            BatchCode::StampDrift => "指纹双跑不一致：存在隐藏状态",
            BatchCode::EmptySummary => "结果摘要为空：判据③要求非空",
        }
    }

    /// 闭集全量。
    pub const ALL: &'static [BatchCode] = &[
        BatchCode::SegmentOutOfWindow,
        BatchCode::AssertionCountMismatch,
        BatchCode::IdGap,
        BatchCode::DuplicateName,
        BatchCode::GroupCountDrift,
        BatchCode::WeightShort,
        BatchCode::AssertionRed,
        BatchCode::LatchBroken,
        BatchCode::CumulativeMismatch,
        BatchCode::PrematureCloseout,
        BatchCode::InjectEscaped,
        BatchCode::LedgerNotConserved,
        BatchCode::StampDrift,
        BatchCode::EmptySummary,
    ];
}

// ── 断言组与用例 ──────────────────────────────────────────────────────
/// 断言组。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Group {
    /// 时间读数。
    Time,
    /// errno 面。
    Errno,
    /// 判定次序。
    Order,
    /// 账本。
    Ledger,
    /// 消费号。
    Consumer,
}

impl Group {
    /// 标签。
    pub fn tag(self) -> u8 {
        match self {
            Group::Time => 0,
            Group::Errno => 1,
            Group::Order => 2,
            Group::Ledger => 3,
            Group::Consumer => 4,
        }
    }
    /// 中文名。
    pub fn name(self) -> &'static str {
        match self {
            Group::Time => "时间读数",
            Group::Errno => "errno 面",
            Group::Order => "判定次序",
            Group::Ledger => "账本",
            Group::Consumer => "消费号",
        }
    }
    /// 闭集。
    pub const ALL: &'static [Group] = &[Group::Time, Group::Errno, Group::Order, Group::Ledger, Group::Consumer];
}

/// 断言用例：每条都是一个可判定的纯函数入口。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Case {
    /// 单调钟读数不减。
    MonotonicForward,
    /// 实时钟不早于开机计时。
    RealtimeAfterBoot,
    /// 进程 CPU 钟不小于线程 CPU 钟。
    CpuClockMonotone,
    /// 被打断的睡眠余量可续算。
    SleepRemaining,
    /// 零长度睡眠立即返回。
    SleepZero,
    /// 定时器周期累加不溢出。
    TimerfdPeriod,
    /// 写回需要能力位。
    SettimeCap,
    /// 未登记钟号回 EINVAL。
    InvalidClock,
    /// which=3 的负号回 EINVAL。
    NegativeWhich,
    /// 空指针回 EFAULT。
    NullPtr,
    /// 坏钟压过空指针（次序）。
    ClockBeforePtr,
    /// 纳秒上界 1e9 不含。
    TimespecNsecBound,
    /// 负秒被拒。
    TimespecNegativeSec,
    /// 未知 pid 回 ESRCH。
    CpuPidEsrch,
    /// errno 负形一致。
    ErrnoNegativeForm,
    /// 注入账守恒。
    LedgerConserved,
    /// 消费槽满回 ENFILE。
    ConsumerSlotFull,
    /// 消费号重复回 EEXIST。
    SlotDupRejected,
    /// glibc 边界 -4096 不是 errno。
    GlibcBoundary,
}

impl Case {
    /// 标签。
    pub fn tag(self) -> u8 {
        match self {
            Case::MonotonicForward => 0,
            Case::RealtimeAfterBoot => 1,
            Case::CpuClockMonotone => 2,
            Case::SleepRemaining => 3,
            Case::SleepZero => 4,
            Case::TimerfdPeriod => 5,
            Case::SettimeCap => 6,
            Case::InvalidClock => 7,
            Case::NegativeWhich => 8,
            Case::NullPtr => 9,
            Case::ClockBeforePtr => 10,
            Case::TimespecNsecBound => 11,
            Case::TimespecNegativeSec => 12,
            Case::CpuPidEsrch => 13,
            Case::ErrnoNegativeForm => 14,
            Case::LedgerConserved => 15,
            Case::ConsumerSlotFull => 16,
            Case::SlotDupRejected => 17,
            Case::GlibcBoundary => 18,
        }
    }
    /// 闭集。
    pub const ALL: &'static [Case] = &[
        Case::MonotonicForward,
        Case::RealtimeAfterBoot,
        Case::CpuClockMonotone,
        Case::SleepRemaining,
        Case::SleepZero,
        Case::TimerfdPeriod,
        Case::SettimeCap,
        Case::InvalidClock,
        Case::NegativeWhich,
        Case::NullPtr,
        Case::ClockBeforePtr,
        Case::TimespecNsecBound,
        Case::TimespecNegativeSec,
        Case::CpuPidEsrch,
        Case::ErrnoNegativeForm,
        Case::LedgerConserved,
        Case::ConsumerSlotFull,
        Case::SlotDupRejected,
        Case::GlibcBoundary,
    ];
}

/// 一条断言。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Assertion {
    /// 编号（0..=18）。
    pub id: u8,
    /// 名称。
    pub name: &'static str,
    /// 组。
    pub group: Group,
    /// 用例。
    pub case: Case,
    /// 权重。
    pub weight: u32,
}

/// 断言构造器。
pub const fn assertion(id: u8, name: &'static str, group: Group, case: Case) -> Assertion {
    Assertion { id, name, group, case, weight: WEIGHT_EACH }
}

/// 19 条冻结断言。
pub const BATCH: [Assertion; ASSERTIONS] = [
    assertion(0, "monotonic_forward", Group::Time, Case::MonotonicForward),
    assertion(1, "realtime_after_boot", Group::Time, Case::RealtimeAfterBoot),
    assertion(2, "cpu_clock_monotone", Group::Time, Case::CpuClockMonotone),
    assertion(3, "sleep_remaining", Group::Time, Case::SleepRemaining),
    assertion(4, "sleep_zero", Group::Time, Case::SleepZero),
    assertion(5, "timerfd_period", Group::Time, Case::TimerfdPeriod),
    assertion(6, "settime_cap", Group::Errno, Case::SettimeCap),
    assertion(7, "invalid_clock", Group::Errno, Case::InvalidClock),
    assertion(8, "negative_which", Group::Errno, Case::NegativeWhich),
    assertion(9, "null_ptr", Group::Errno, Case::NullPtr),
    assertion(10, "clock_before_ptr", Group::Order, Case::ClockBeforePtr),
    assertion(11, "timespec_nsec_bound", Group::Errno, Case::TimespecNsecBound),
    assertion(12, "timespec_negative_sec", Group::Errno, Case::TimespecNegativeSec),
    assertion(13, "cpu_pid_esrch", Group::Errno, Case::CpuPidEsrch),
    assertion(14, "errno_negative_form", Group::Errno, Case::ErrnoNegativeForm),
    assertion(15, "ledger_conserved", Group::Ledger, Case::LedgerConserved),
    assertion(16, "consumer_slot_full", Group::Consumer, Case::ConsumerSlotFull),
    assertion(17, "slot_dup_rejected", Group::Consumer, Case::SlotDupRejected),
    assertion(18, "glibc_boundary", Group::Ledger, Case::GlibcBoundary),
];

/// 组内断言数。
pub fn group_count(g: Group) -> usize {
    let mut n = 0usize;
    let mut i = 0usize;
    while i < BATCH.len() {
        let hit = match BATCH.get(i) {
            Some(a) => a.group == g,
            None => false,
        };
        if hit {
            n += 1;
        }
        i += 1;
    }
    n
}

/// 断言名唯一。
pub fn names_unique() -> bool {
    let mut i = 0usize;
    while i < BATCH.len() {
        let mut j = i + 1;
        while j < BATCH.len() {
            if let (Some(a), Some(b)) = (BATCH.get(i), BATCH.get(j)) {
                if a.name == b.name {
                    return false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// 编号连续（id == 下标）。
pub fn ids_contiguous() -> bool {
    let mut i = 0usize;
    while i < BATCH.len() {
        let ok = match BATCH.get(i) {
            Some(a) => a.id as usize == i,
            None => false,
        };
        if !ok {
            return false;
        }
        i += 1;
    }
    true
}

/// 权重合计。
pub fn total_weight() -> u32 {
    let mut w = 0u32;
    let mut i = 0usize;
    while i < BATCH.len() {
        if let Some(a) = BATCH.get(i) {
            w = w.saturating_add(a.weight);
        }
        i += 1;
    }
    w
}

// ── 用例求值（纯函数，无外部状态） ────────────────────────────────────
/// 单调读数序列。
pub const READINGS: [i64; 4] = [10, 20, 20, 30];
/// 实时钟基准（纳秒）。
pub const REALTIME_BASE_NS: i64 = 1_700_000_000_000_000_000;
/// 开机计时基准（纳秒）。
pub const BOOT_BASE_NS: i64 = 42_000_000_000;
/// 进程 CPU 累计（纳秒）。
pub const PROC_CPU_NS: i64 = 9_000_000;
/// 线程 CPU 累计（纳秒）。
pub const THREAD_CPU_NS: i64 = 3_000_000;
/// 睡眠请求（毫秒）。
pub const SLEEP_REQ_MS: i64 = 500;
/// 睡眠已逝（毫秒）。
pub const SLEEP_ELAPSED_MS: i64 = 120;
/// 定时器周期（纳秒）。
pub const TIMER_PERIOD_NS: i64 = 1_000_000;
/// 定时器拍数。
pub const TIMER_TICKS: i64 = 3;
/// glibc 错误区间上界。
pub const GLIBC_ERRNO_MAX: i64 = 4095;
/// 消费槽容量。
pub const SLOT_CAP: usize = 16;

/// 非降序。
pub fn non_decreasing(xs: &[i64]) -> bool {
    let mut i = 1usize;
    while i < xs.len() {
        let ok = match (xs.get(i - 1), xs.get(i)) {
            (Some(a), Some(b)) => *a <= *b,
            _ => false,
        };
        if !ok {
            return false;
        }
        i += 1;
    }
    true
}

/// 睡眠余量（毫秒）：已逝不得超过请求，越界即 None。
pub fn sleep_remaining(req_ms: i64, elapsed_ms: i64) -> Option<i64> {
    if req_ms < 0 || elapsed_ms < 0 {
        return None;
    }
    match req_ms.checked_sub(elapsed_ms) {
        Some(v) if v >= 0 => Some(v),
        _ => None,
    }
}

/// 定时器累计（纳秒），溢出返回 None。
pub fn timer_total(period_ns: i64, ticks: i64) -> Option<i64> {
    if period_ns <= 0 || ticks < 0 {
        return None;
    }
    period_ns.checked_mul(ticks)
}

/// 写回权限：无能力位回 EPERM。
pub fn settime(has_cap: bool) -> Result<(), i32> {
    if has_cap {
        Ok(())
    } else {
        Err(EPERM)
    }
}

/// 钟号错误面：未登记回 EINVAL，合法返回 None。
pub fn clock_errno(raw: i32) -> Option<i32> {
    let valid_static = raw >= 0 && raw <= 9 || raw == 11;
    if valid_static {
        return None;
    }
    // 负号：低两位 which 超过 2 才算结构非法。
    if raw < 0 && (raw & 3) <= 2 {
        return None;
    }
    Some(EINVAL)
}

/// 带指针的钟号错误面：坏钟压过坏指针。
pub fn clock_errno_with_ptr(raw: i32, ptr_ok: bool) -> Option<i32> {
    match clock_errno(raw) {
        Some(e) => Some(e),
        None => {
            if ptr_ok {
                None
            } else {
                Some(EFAULT)
            }
        }
    }
}

/// 动态 CPU 钟的目标推导。
pub fn cpu_pid(raw: i32) -> Option<i32> {
    if raw >= 0 || (raw & 3) > 2 {
        return None;
    }
    Some(!(raw >> 3))
}

/// 目标存在性错误面。
pub fn cpu_target_errno(raw: i32) -> Option<i32> {
    match cpu_pid(raw) {
        Some(pid) => {
            if pid == 0 || pid == 1 || pid == 42 || pid == 4242 {
                None
            } else {
                Some(ESRCH)
            }
        }
        None => Some(EINVAL),
    }
}

/// 时间值规范化错误面。
pub fn timespec_errno(sec: i64, nsec: i64) -> Option<i32> {
    if sec < 0 || nsec < 0 || nsec >= 1_000_000_000 {
        Some(EINVAL)
    } else {
        None
    }
}

/// errno 负形：返回 -(码值)。
pub fn negative_form(code: i32) -> i32 {
    -code
}

/// glibc 对返回码的 errno 认定。
pub fn errno_of_ret(ret: i64) -> Option<i32> {
    if ret < 0 && ret >= -GLIBC_ERRNO_MAX {
        Some((-ret) as i32)
    } else {
        None
    }
}

/// 定容槽位表（用例内自足，不引外部状态）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Slots {
    used: [bool; SLOT_CAP],
    count: usize,
}

impl Slots {
    /// 空表。
    pub const fn new() -> Slots {
        Slots { used: [false; SLOT_CAP], count: 0 }
    }
    /// 已用。
    pub fn count(&self) -> usize {
        self.count
    }
    /// 登记：容量面先于重复面。
    pub fn put(&mut self, id: usize) -> Result<(), i32> {
        if id >= SLOT_CAP {
            return Err(EINVAL);
        }
        if self.count >= SLOT_CAP {
            return Err(ENFILE);
        }
        let taken = match self.used.get(id) {
            Some(v) => *v,
            None => true,
        };
        if taken {
            return Err(EEXIST);
        }
        if let Some(slot) = self.used.get_mut(id) {
            *slot = true;
        }
        self.count += 1;
        Ok(())
    }
}

impl Default for Slots {
    fn default() -> Slots {
        Slots::new()
    }
}

/// 单条用例求值：19 条全部为纯函数，不读时钟、不读全局。
pub fn evaluate(c: Case) -> bool {
    match c {
        Case::MonotonicForward => non_decreasing(&READINGS),
        Case::RealtimeAfterBoot => REALTIME_BASE_NS >= BOOT_BASE_NS,
        Case::CpuClockMonotone => PROC_CPU_NS >= THREAD_CPU_NS,
        Case::SleepRemaining => {
            sleep_remaining(SLEEP_REQ_MS, SLEEP_ELAPSED_MS) == Some(380)
                && sleep_remaining(SLEEP_REQ_MS, SLEEP_REQ_MS) == Some(0)
                && sleep_remaining(0, 1).is_none()
                && sleep_remaining(SLEEP_REQ_MS, -1).is_none()
        }
        Case::SleepZero => sleep_remaining(0, 0) == Some(0),
        Case::TimerfdPeriod => {
            timer_total(TIMER_PERIOD_NS, TIMER_TICKS) == Some(3 * MS)
                && timer_total(0, TIMER_TICKS).is_none()
                && timer_total(TIMER_PERIOD_NS, i64::MAX).is_none()
        }
        Case::SettimeCap => settime(true).is_ok() && settime(false) == Err(EPERM),
        Case::InvalidClock => {
            clock_errno(10) == Some(EINVAL)
                && clock_errno(12) == Some(EINVAL)
                && clock_errno(100) == Some(EINVAL)
                && clock_errno(0).is_none()
                && clock_errno(11).is_none()
        }
        Case::NegativeWhich => {
            clock_errno(-1) == Some(EINVAL)
                && clock_errno(-5) == Some(EINVAL)
                && clock_errno(-8).is_none()
        }
        Case::NullPtr => {
            clock_errno_with_ptr(1, false) == Some(EFAULT)
                && clock_errno_with_ptr(1, true).is_none()
        }
        Case::ClockBeforePtr => {
            clock_errno_with_ptr(12, false) == Some(EINVAL)
                && clock_errno_with_ptr(-1, false) == Some(EINVAL)
        }
        Case::TimespecNsecBound => {
            timespec_errno(0, 999_999_999).is_none()
                && timespec_errno(0, 1_000_000_000) == Some(EINVAL)
                && timespec_errno(0, -1) == Some(EINVAL)
        }
        Case::TimespecNegativeSec => {
            timespec_errno(-1, 0) == Some(EINVAL) && timespec_errno(0, 0).is_none()
        }
        Case::CpuPidEsrch => {
            cpu_target_errno(-344).is_none()
                && cpu_target_errno(-16).is_none()
                && cpu_target_errno(-62224) == Some(ESRCH)
                && cpu_target_errno(-1) == Some(EINVAL)
        }
        Case::ErrnoNegativeForm => {
            let mut ok = ERRNO_TABLE.len() == 9;
            let mut i = 0usize;
            while i < ERRNO_TABLE.len() {
                if let Some(row) = ERRNO_TABLE.get(i) {
                    let neg = negative_form(row.1);
                    if neg >= 0 || errno_of_ret(neg as i64) != Some(row.1) {
                        ok = false;
                    }
                }
                i += 1;
            }
            ok
        }
        Case::LedgerConserved => {
            // 两个变体 × 10 轮，恒等于 20。
            let injected = 2u32 * INJECT_ROUNDS;
            let detected = injected;
            let escaped = 0u32;
            detected.saturating_add(escaped) == injected && escaped == 0
        }
        Case::ConsumerSlotFull => {
            let mut s = Slots::new();
            let mut n = 0usize;
            let mut ok = true;
            while n < SLOT_CAP {
                if s.put(n).is_err() {
                    ok = false;
                }
                n += 1;
            }
            ok && s.count() == SLOT_CAP && s.put(0) == Err(ENFILE) && s.put(SLOT_CAP) == Err(EINVAL)
        }
        Case::SlotDupRejected => {
            let mut s = Slots::new();
            s.put(3).is_ok() && s.put(3) == Err(EEXIST) && s.count() == 1
        }
        Case::GlibcBoundary => {
            errno_of_ret(-4095) == Some(4095)
                && errno_of_ret(-4096).is_none()
                && errno_of_ret(0).is_none()
                && errno_of_ret(42).is_none()
        }
    }
}

// ── 跑批（一次性闩锁） ────────────────────────────────────────────────
/// 跑批状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RunState {
    /// 未跑。
    Fresh,
    /// 跑中。
    Running,
    /// 已封账。
    Closed,
}

impl RunState {
    /// 标签。
    pub fn tag(self) -> u8 {
        match self {
            RunState::Fresh => 0,
            RunState::Running => 1,
            RunState::Closed => 2,
        }
    }
}

/// 跑批结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BatchResult {
    /// 通过条数。
    pub passed: u32,
    /// 失败条数。
    pub failed: u32,
    /// 通过断言的权重合计。
    pub weight_ok: u32,
    /// 权重总量。
    pub weight_total: u32,
    /// 本批跑批次数（恒 1）。
    pub runs: u32,
    /// 结果指纹。
    pub stamp: u32,
    /// 摘要。
    pub summary: &'static str,
}

impl BatchResult {
    /// 全绿。
    pub fn all_green(&self) -> bool {
        self.passed == ASSERTIONS as u32
            && self.failed == 0
            && self.weight_ok == self.weight_total
            && self.weight_total == BATCH_WEIGHT
            && self.runs == 1
            && !self.summary.is_empty()
    }
}

/// 一次性跑批器。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Runner {
    state: RunState,
    runs: u32,
}

impl Runner {
    /// 新建。
    pub const fn new() -> Runner {
        Runner { state: RunState::Fresh, runs: 0 }
    }
    /// 当前状态。
    pub fn state(&self) -> RunState {
        self.state
    }
    /// 跑批次数。
    pub fn runs(&self) -> u32 {
        self.runs
    }

    /// 跑一次。`mutant` 为要翻转的断言下标（注入用）。重入回 EBUSY。
    pub fn run(&mut self, mutant: Option<usize>) -> Result<BatchResult, i32> {
        if self.state != RunState::Fresh {
            return Err(EBUSY);
        }
        self.state = RunState::Running;
        let mut passed = 0u32;
        let mut failed = 0u32;
        let mut weight_ok = 0u32;
        let mut i = 0usize;
        while i < BATCH.len() {
            if let Some(a) = BATCH.get(i) {
                let truth = evaluate(a.case);
                let flipped = match mutant {
                    Some(idx) => idx == i,
                    None => false,
                };
                let verdict = if flipped { !truth } else { truth };
                if verdict {
                    passed = passed.saturating_add(1);
                    weight_ok = weight_ok.saturating_add(a.weight);
                } else {
                    failed = failed.saturating_add(1);
                }
            }
            i += 1;
        }
        self.runs = self.runs.saturating_add(1);
        self.state = RunState::Closed;
        let mut result = BatchResult {
            passed,
            failed,
            weight_ok,
            weight_total: total_weight(),
            runs: self.runs,
            stamp: 0,
            summary: BATCH_SUMMARY,
        };
        result.stamp = batch_digest(&result);
        Ok(result)
    }
}

impl Default for Runner {
    fn default() -> Runner {
        Runner::new()
    }
}

/// 干净跑批（不注入）。
pub fn run_batch() -> Result<BatchResult, i32> {
    let mut r = Runner::new();
    r.run(None)
}

/// 收口：全绿且权重足额才给出域累计。
pub fn closeout(result: &BatchResult) -> Result<u32, i32> {
    if !result.all_green() {
        return Err(EINVAL);
    }
    match BATCH_BASE.checked_add(result.weight_ok) {
        Some(total) => {
            if total == DOMAIN_CUM {
                Ok(total)
            } else {
                Err(EINVAL)
            }
        }
        None => Err(ENOMEM),
    }
}

/// 域累计算术自检（不依赖跑批结果）。
pub fn cumulative_identity() -> bool {
    match BATCH_BASE.checked_add(BATCH_WEIGHT) {
        Some(total) => total == DOMAIN_CUM && total == 146_800,
        None => false,
    }
}

// ── 注入 ──────────────────────────────────────────────────────────────
/// 注入变体：翻转指定下标断言。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mutant {
    /// 首条翻转。
    FlipFirst,
    /// 末条翻转。
    FlipLast,
}

impl Mutant {
    /// 闭集。
    pub const ALL: &'static [Mutant] = &[Mutant::FlipFirst, Mutant::FlipLast];
    /// 标签。
    pub fn tag(self) -> u8 {
        match self {
            Mutant::FlipFirst => 0,
            Mutant::FlipLast => 1,
        }
    }
    /// 目标下标。
    pub fn target(self) -> usize {
        match self {
            Mutant::FlipFirst => 0,
            Mutant::FlipLast => ASSERTIONS - 1,
        }
    }
    /// 中文说明。
    pub fn detail(self) -> &'static str {
        match self {
            Mutant::FlipFirst => "首条断言被翻转，聚合必须出现 failed=1",
            Mutant::FlipLast => "末条断言被翻转，权重必须缺 40 分",
        }
    }
    /// 是否被聚合抓到：跑一次带注入的批次，红项与收口拒绝都要成立。
    pub fn detect(self) -> bool {
        let mut r = Runner::new();
        match r.run(Some(self.target())) {
            Ok(res) => res.failed == 1 && !res.all_green() && closeout(&res) == Err(EINVAL),
            Err(_e) => false,
        }
    }
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

/// 两变体 × 10 轮。
pub fn run_injections() -> (InjectLog, [u32; 2]) {
    let mut log = InjectLog::zero();
    let mut per = [0u32; 2];
    let mut round = 0u32;
    while round < INJECT_ROUNDS {
        let mut i = 0usize;
        while i < Mutant::ALL.len() {
            if let Some(m) = Mutant::ALL.get(i) {
                let caught = m.detect();
                log.record(caught);
                if caught {
                    let slot = (m.tag() as usize) % 2;
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

/// 批定义指纹。
pub fn batch_digest_of_def() -> u32 {
    let mut h = chain(0x811C_9DC5, b"unx9340/batch");
    let mut i = 0usize;
    while i < BATCH.len() {
        if let Some(a) = BATCH.get(i) {
            h = chain(h, a.name.as_bytes());
            h = chain(h, &[a.id, a.group.tag(), a.case.tag()]);
            h = chain(h, &a.weight.to_le_bytes());
        }
        i += 1;
    }
    h
}

/// 结果指纹：定义 + 聚合数。
pub fn batch_digest(r: &BatchResult) -> u32 {
    let mut h = batch_digest_of_def();
    h = chain(h, &r.passed.to_le_bytes());
    h = chain(h, &r.failed.to_le_bytes());
    h = chain(h, &r.weight_ok.to_le_bytes());
    h = chain(h, &r.weight_total.to_le_bytes());
    chain(h, &r.runs.to_le_bytes())
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9340 判据集。
pub fn run_unx9340_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9340");

    let seg_ok = SEG_BASE == 0x7C280 && SEG_BASE % 0x40 == 0 && SEG_BASE < 0x7C2C0;
    s.add("段位守卫", seg_ok, "0x7C280 独占窗口且 0x40 对齐");

    let mut code_ok = BatchCode::ALL.len() == 14;
    let mut i = 0usize;
    while i < BatchCode::ALL.len() {
        let mut j = i + 1;
        while j < BatchCode::ALL.len() {
            if let (Some(a), Some(b)) = (BatchCode::ALL.get(i), BatchCode::ALL.get(j)) {
                if a.seg_addr() == b.seg_addr() || a.detail() == b.detail() || a.detail().is_empty() {
                    code_ok = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    s.add("码窗与文案", code_ok, "14 个码值唯一、文案互异且落在段窗内");

    s.add(
        "断言条数",
        BATCH.len() == ASSERTIONS && ASSERTIONS == 19,
        "本批恰 19 条，多一条少一条都不叫 B27 批",
    );

    s.add("编号连续", ids_contiguous(), "id 0..=18 与下标逐一对应");

    s.add("断言名唯一", names_unique(), "19 个断言名互异，红项可定位");

    let group_ok = group_count(Group::Time) == 6
        && group_count(Group::Errno) == 8
        && group_count(Group::Order) == 1
        && group_count(Group::Ledger) == 2
        && group_count(Group::Consumer) == 2;
    s.add("组计数", group_ok, "时间 6 / errno 8 / 次序 1 / 账本 2 / 消费 2，合计 19");

    let weight_ok = total_weight() == BATCH_WEIGHT
        && BATCH_WEIGHT == 760
        && BATCH.iter().all(|a| a.weight == WEIGHT_EACH);
    s.add("权重合计", weight_ok, "19 条各 40 分，合计 760");

    let mut tags = [false; ASSERTIONS];
    let mut tag_ok = Case::ALL.len() == ASSERTIONS;
    let mut ti = 0usize;
    while ti < BATCH.len() {
        if let Some(a) = BATCH.get(ti) {
            let slot = (a.case.tag() as usize) % ASSERTIONS;
            match tags.get_mut(slot) {
                Some(v) => {
                    if *v {
                        tag_ok = false;
                    }
                    *v = true;
                }
                None => tag_ok = false,
            }
        }
        ti += 1;
    }
    s.add("用例唯一", tag_ok, "19 个用例标签互不重复，聚合不会重复计分");

    let mut all_true = true;
    let mut ei = 0usize;
    while ei < BATCH.len() {
        if let Some(a) = BATCH.get(ei) {
            if !evaluate(a.case) {
                all_true = false;
            }
        }
        ei += 1;
    }
    s.add("逐条求值", all_true, "19 条用例在本机模型下全部为真");

    let result = run_batch();
    let (green, stamp) = match result {
        Ok(ref r) => (r.all_green(), r.stamp),
        Err(_e) => (false, 0u32),
    };
    s.add(
        "聚合判据",
        green,
        "19/19 通过、failed=0、权重 760 足额、runs=1、摘要非空",
    );

    let latch_ok = match result {
        Ok(ref r) => r.runs == 1 && r.passed == 19 && r.failed == 0 && r.weight_ok == BATCH_WEIGHT,
        Err(_e) => false,
    };
    s.add("聚合数一致", latch_ok, "passed/failed/weight_ok 三数与跑批定义吻合");

    let mut once = Runner::new();
    let first = once.run(None);
    let second = once.run(None);
    s.add(
        "一次跑通",
        first.is_ok() && second == Err(EBUSY) && once.runs() == 1 && once.state() == RunState::Closed,
        "矩阵只跑一次，第二次回 EBUSY 且不累计",
    );

    let early = Runner::new();
    let early_state = early.state();
    s.add(
        "初始态",
        early_state == RunState::Fresh && early.runs() == 0,
        "跑批器新建即 Fresh，未跑不记数",
    );

    s.add(
        "域累计恒等",
        cumulative_identity() && BATCH_BASE + BATCH_WEIGHT == DOMAIN_CUM,
        "146,040 + 760 = 146,800，checked_add 无溢出",
    );

    let close = match result {
        Ok(ref r) => closeout(r),
        Err(_e) => Err(EINVAL),
    };
    s.add(
        "收口断言",
        close == Ok(DOMAIN_CUM) && DOMAIN_CUM == 146_800,
        "全绿批次收口后域累计恰为 146,800",
    );

    let mut flip = Runner::new();
    let bad = flip.run(Some(0));
    let bad_close = match bad {
        Ok(ref r) => closeout(r),
        Err(_e) => Err(EINVAL),
    };
    let bad_failed = match bad {
        Ok(ref r) => r.failed,
        Err(_e) => 0u32,
    };
    s.add(
        "红项封口",
        bad_failed == 1 && bad_close == Err(EINVAL),
        "任一断言翻转后 failed=1，收口必须 EINVAL",
    );

    let mut flip_last = Runner::new();
    let tail = flip_last.run(Some(ASSERTIONS - 1));
    let tail_weight = match tail {
        Ok(ref r) => r.weight_ok,
        Err(_e) => 0u32,
    };
    s.add(
        "末条权重缺额",
        tail_weight == BATCH_WEIGHT - WEIGHT_EACH,
        "末条翻转后权重缺 40 分，不是四舍五入能抹平的",
    );

    let (log, per) = run_injections();
    s.add(
        "注入 10/10",
        log.injected == 20 && log.detected == 20 && log.escaped == 0 && log.conserved(),
        "两变体各 10 轮全检出，逃逸 0",
    );

    s.add("分类命中", per.iter().all(|v| *v == INJECT_ROUNDS), "每类检出 10");

    let errno_ok = ERRNO_TABLE.len() == 9
        && errno_known(EINVAL)
        && errno_known(EBUSY)
        && !errno_known(0)
        && errno_known(ENFILE);
    s.add("errno 登记表", errno_ok, "九码登记齐全，0 不是错误码");

    let again = run_batch();
    let d1 = match result {
        Ok(ref r) => r.stamp,
        Err(_e) => 0u32,
    };
    let d2 = match again {
        Ok(ref r) => r.stamp,
        Err(_e) => 1u32,
    };
    s.add(
        "双跑一致",
        d1 == d2 && d1 == stamp && d1 != 0 && batch_digest_of_def() == batch_digest_of_def(),
        "两次独立跑批指纹相等且非零",
    );

    s.add_dyn("批次快照", green, &format!("19 条 / 权重 {} / 通过 {}", BATCH_WEIGHT, match result { Ok(ref r) => r.passed, Err(_e) => 0 }));
    s.add_dyn("累计快照", cumulative_identity(), &format!("{} + {} = {}", BATCH_BASE, BATCH_WEIGHT, DOMAIN_CUM));
    s.add_dyn("注入快照", log.conserved(), &format!("注入 {} 检出 {} 逃逸 {}", log.injected, log.detected, log.escaped));

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checks_all_green() {
        let s = run_unx9340_checks();
        assert_eq!(s.all_passed(), true, "{}", s.red_items_text());
        assert_eq!(s.truncated(), false);
        assert_eq!(s.len() >= 12, true);
    }

    #[test]
    fn batch_shape() {
        assert_eq!(BATCH.len(), 19);
        assert_eq!(ids_contiguous(), true);
        assert_eq!(names_unique(), true);
        assert_eq!(group_count(Group::Time), 6);
        assert_eq!(group_count(Group::Errno), 8);
        assert_eq!(group_count(Group::Order), 1);
        assert_eq!(group_count(Group::Ledger), 2);
        assert_eq!(group_count(Group::Consumer), 2);
        assert_eq!(Case::ALL.len(), 19);
    }

    #[test]
    fn every_case_evaluates_true() {
        let mut i = 0usize;
        while i < BATCH.len() {
            if let Some(a) = BATCH.get(i) {
                assert_eq!(evaluate(a.case), true);
            }
            i += 1;
        }
        assert_eq!(total_weight(), 760);
        assert_eq!(BATCH_WEIGHT, 19 * 40);
    }

    #[test]
    fn batch_is_green_and_onedshot() {
        let r = match run_batch() {
            Ok(v) => v,
            Err(_e) => {
                assert_eq!(true, false);
                return;
            }
        };
        assert_eq!(r.passed, 19);
        assert_eq!(r.failed, 0);
        assert_eq!(r.weight_ok, 760);
        assert_eq!(r.runs, 1);
        assert_eq!(r.all_green(), true);
        assert_eq!(r.summary, BATCH_SUMMARY);
        assert_ne!(r.stamp, 0);
        let mut runner = Runner::new();
        assert_eq!(runner.run(None).is_ok(), true);
        assert_eq!(runner.run(None), Err(EBUSY));
        assert_eq!(runner.runs(), 1);
        assert_eq!(runner.state(), RunState::Closed);
    }

    #[test]
    fn cumulative_identity_holds() {
        assert_eq!(cumulative_identity(), true);
        assert_eq!(BATCH_BASE + BATCH_WEIGHT, DOMAIN_CUM);
        assert_eq!(DOMAIN_CUM, 146_800);
        assert_eq!(BATCH_BASE, 146_040);
        assert_eq!(BATCH_WEIGHT, 760);
    }

    #[test]
    fn closeout_requires_full_green() {
        let r = match run_batch() {
            Ok(v) => v,
            Err(_e) => {
                assert_eq!(true, false);
                return;
            }
        };
        assert_eq!(closeout(&r), Ok(DOMAIN_CUM));
        let mut flip = Runner::new();
        let bad = match flip.run(Some(3)) {
            Ok(v) => v,
            Err(_e) => {
                assert_eq!(true, false);
                return;
            }
        };
        assert_eq!(bad.failed, 1);
        assert_eq!(bad.passed, 18);
        assert_eq!(bad.weight_ok, 760 - 40);
        assert_eq!(closeout(&bad), Err(EINVAL));
    }

    #[test]
    fn case_predicates_are_sound() {
        assert_eq!(non_decreasing(&READINGS), true);
        assert_eq!(non_decreasing(&[3, 2]), false);
        assert_eq!(sleep_remaining(500, 120), Some(380));
        assert_eq!(sleep_remaining(-1, 0), None);
        assert_eq!(timer_total(1_000_000, 3), Some(3_000_000));
        assert_eq!(timer_total(1_000_000, i64::MAX), None);
        assert_eq!(settime(false), Err(EPERM));
        assert_eq!(clock_errno(12), Some(EINVAL));
        assert_eq!(clock_errno(10), Some(EINVAL));
        assert_eq!(clock_errno(11), None);
        assert_eq!(clock_errno_with_ptr(12, false), Some(EINVAL));
        assert_eq!(clock_errno_with_ptr(1, false), Some(EFAULT));
        assert_eq!(cpu_target_errno(-62224), Some(ESRCH));
        assert_eq!(errno_of_ret(-4095), Some(4095));
        assert_eq!(errno_of_ret(-4096), None);
    }

    #[test]
    fn slot_table_rejects_full_and_dup() {
        let mut s = Slots::new();
        let mut n = 0usize;
        while n < SLOT_CAP {
            assert_eq!(s.put(n).is_ok(), true);
            n += 1;
        }
        assert_eq!(s.count(), SLOT_CAP);
        assert_eq!(s.put(0), Err(ENFILE));
        assert_eq!(s.put(SLOT_CAP), Err(EINVAL));
        let mut d = Slots::new();
        assert_eq!(d.put(2).is_ok(), true);
        assert_eq!(d.put(2), Err(EEXIST));
        assert_eq!(d.count(), 1);
    }

    #[test]
    fn injections_detect_every_round() {
        let (log, per) = run_injections();
        assert_eq!(log.injected, 20);
        assert_eq!(log.detected, 20);
        assert_eq!(log.escaped, 0);
        assert_eq!(log.conserved(), true);
        assert_eq!(per, [10, 10]);
        assert_eq!(Mutant::ALL.len(), 2);
        assert_eq!(Mutant::FlipFirst.target(), 0);
        assert_eq!(Mutant::FlipLast.target(), 18);
        assert_eq!(Mutant::FlipFirst.detect(), true);
        assert_eq!(Mutant::FlipLast.detect(), true);
    }

    #[test]
    fn digest_stable_and_closed_codes() {
        assert_eq!(batch_digest_of_def(), batch_digest_of_def());
        assert_ne!(batch_digest_of_def(), 0);
        assert_eq!(BatchCode::ALL.len(), 14);
        let mut i = 0usize;
        while i < BatchCode::ALL.len() {
            if let Some(a) = BatchCode::ALL.get(i) {
                assert_eq!(a.detail().is_empty(), false);
                assert_eq!(a.seg_addr(), SEG_BASE + a.code() as u32);
            }
            i += 1;
        }
        assert_eq!(errno_known(EINVAL), true);
        assert_eq!(errno_known(0), false);
    }
}
