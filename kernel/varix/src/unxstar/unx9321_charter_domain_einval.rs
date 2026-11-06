//! UNX-F9321 时间资源错误面总纲（EINVAL 判定域） · 0x7BDC0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 F9321 逐字）：
//!   ① 纯功能代码 ≥400 行；② 判据原文：UNX-F9321-J1 时间资源错误面判据过，
//!   EINVAL 触发域（非法时钟/非法标志/值域越界）逐格落账，与 Linux 语义一致格过；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//!   ① 错误面按「面 × 触发域」组织：八个系统调用面（clock_gettime、clock_settime、
//!      nanosleep、clock_nanosleep、timerfd_create、timerfd_settime、timer_create、
//!      timer_settime），三个触发域（非法时钟、非法标志、值域越界）。每一格都必须
//!      落到具体触发域，禁止出现「拒绝但归不了类」的格子——归不了类的拒绝在
//!      用户态就是不可诊断的 EINVAL。
//!   ② 判定次序是冻结的：时钟 → 标志 → 值域。复合坏例（时钟坏 + 标志坏 +
//!      纳秒越界）只能落在第一个触发域。这不是实现细节：用户态据此先修哪个
//!      参数，次序错了会把「时钟 id 写错」报成「纳秒越界」。
//!   ③ 时钟域分两张集合：全局合法时钟 11 个（0..=9 与 11，10 是空洞），以及
//!      每个面的能力子集。clock_settime 只允许可设时钟（REALTIME / REALTIME_ALARM
//!      / TAI），clock_nanosleep 只允许 POSIX 最小集（REALTIME / MONOTONIC /
//!      BOOTTIME）。用 MONOTONIC 去 clock_settime 是 EINVAL，不是 EPERM。
//!   ④ 标志域按掩码收口：timerfd_create 只认 NONBLOCK|CLOEXEC，timerfd_settime 只认
//!      ABSTIME|CANCEL_ON_SET，且 CANCEL_ON_SET 必须与 ABSTIME 同时出现；clock_nanosleep
//!      只认 TIMER_ABSTIME。掩码外的任何位都是 EINVAL，不做「忽略未知位」。
//!   ⑤ 值域以 timespec 为轴：tv_nsec 必须落在 [0, 1e9)，tv_sec 不得为负。边界
//!      取值 0 与 1e9-1 放行、1e9 与 -1 拒绝，四个边界格逐个冻结。
//!   ⑥ 注入固定 10 轮 × 21 例 = 210 次；三类落账 5/5/4 例，逐轮累计 50/50/40，
//!      通过 70 次；账本 injected == detected + escaped 且 escaped == 0。
//!   ⑦ 诊断码 12 个、码值唯一；指纹覆盖面表、21 例与三域计数。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 诊断段基址（F9321 独占 0x7BDC0..=0x7BDFF）。
pub const SEG_BASE: u32 = 0x7BDC0;
/// 段窗口。
pub const SEG_SPAN: u32 = 0x40;
pub const SEG_WINDOW_LO: u32 = 0x7B000;
pub const SEG_WINDOW_HI: u32 = 0x7BFFF;

// ── Linux errno（正数口径） ───────────────────────────────────────────
pub const EINVAL: i32 = 22;
pub const EFAULT: i32 = 14;
pub const EAGAIN: i32 = 11;
pub const ENOTSUP: i32 = 95;

// ── 时钟 id ───────────────────────────────────────────────────────────
pub const CLOCK_REALTIME: i32 = 0;
pub const CLOCK_MONOTONIC: i32 = 1;
pub const CLOCK_PROCESS_CPUTIME_ID: i32 = 2;
pub const CLOCK_THREAD_CPUTIME_ID: i32 = 3;
pub const CLOCK_MONOTONIC_RAW: i32 = 4;
pub const CLOCK_REALTIME_COARSE: i32 = 5;
pub const CLOCK_MONOTONIC_COARSE: i32 = 6;
pub const CLOCK_BOOTTIME: i32 = 7;
pub const CLOCK_REALTIME_ALARM: i32 = 8;
pub const CLOCK_BOOTTIME_ALARM: i32 = 9;
pub const CLOCK_TAI: i32 = 11;

/// 一纳秒的边界值。
pub const NS_PER_SEC: i64 = 1_000_000_000;

// ── 标志位 ────────────────────────────────────────────────────────────
/// timerfd_settime: 绝对时间。
pub const TFD_TIMER_ABSTIME: u32 = 0x1;
/// timerfd_settime: 时钟被设置时取消。
pub const TFD_TIMER_CANCEL_ON_SET: u32 = 0x2;
/// timerfd_settime 合法掩码。
pub const TFD_SETTIME_MASK: u32 = TFD_TIMER_ABSTIME | TFD_TIMER_CANCEL_ON_SET;
/// timerfd_create: 非阻塞。
pub const TFD_NONBLOCK: u32 = 0x800;
/// timerfd_create: 执行时关闭。
pub const TFD_CLOEXEC: u32 = 0x80000;
/// timerfd_create 合法掩码。
pub const TFD_CREATE_MASK: u32 = TFD_NONBLOCK | TFD_CLOEXEC;
/// clock_nanosleep: 绝对时间。
pub const TIMER_ABSTIME: u32 = 0x1;

/// 面数。
pub const SURFACE_COUNT: usize = 8;
/// 冻结用例数。
pub const PROBE_CASES: usize = 21;
/// 注入轮数。
pub const INJECT_ROUNDS: u32 = 10;
/// 三类触发域的预期用例数（非法时钟 / 非法标志 / 值域越界）。
pub const CASE_BAD_CLOCK: u32 = 5;
pub const CASE_BAD_FLAGS: u32 = 5;
pub const CASE_RANGE: u32 = 4;
/// 预期通过用例数。
pub const CASE_ACCEPTED: u32 = 7;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// F9321 诊断码（段内 +1..=+12）。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DomainCode {
    SurfaceGap,
    ClockUnknown,
    ClockNotSettable,
    FlagUnknown,
    FlagCombination,
    RangeNsec,
    RangeSec,
    DomainEmpty,
    DomainSumMismatch,
    OrderViolation,
    LedgerRegress,
    NotReproducible,
}

impl DomainCode {
    /// 段内序号。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 段内码值。
    pub fn val(self) -> u32 {
        SEG_BASE.saturating_add(1).saturating_add(self.code() as u32)
    }

    /// 中文建议。
    pub fn detail(self) -> &'static str {
        match self {
            DomainCode::SurfaceGap => "错误面有缺口：八个面必须各有落点，缺面即漏判",
            DomainCode::ClockUnknown => "时钟 id 未登记：10 是空洞，负数与越界值一律 EINVAL",
            DomainCode::ClockNotSettable => "时钟不可设置：MONOTONIC/CPU 时钟只读，设时钟必须 EINVAL",
            DomainCode::FlagUnknown => "标志位越出掩码：未知位必须 EINVAL，不能静默忽略",
            DomainCode::FlagCombination => "标志组合非法：CANCEL_ON_SET 必须与 ABSTIME 同时出现",
            DomainCode::RangeNsec => "纳秒越界：tv_nsec 必须在 [0,1e9)，1e9 与负值都拒绝",
            DomainCode::RangeSec => "秒域为负：tv_sec 不得为负，负值必须 EINVAL",
            DomainCode::DomainEmpty => "触发域为空：三类域都必须有落账，空域说明判据没覆盖",
            DomainCode::DomainSumMismatch => "三域之和与拒绝总数不等：有拒绝归不了类",
            DomainCode::OrderViolation => "判定次序错乱：时钟先于标志、标志先于值域",
            DomainCode::LedgerRegress => "注入账不守恒：注入必须等于检出加逃逸，逃逸恒为零",
            DomainCode::NotReproducible => "双跑不一致：同用例两次结论不同，判定路径藏了状态",
        }
    }

    /// 闭集全量。
    pub const ALL: &'static [DomainCode] = &[
        DomainCode::SurfaceGap,
        DomainCode::ClockUnknown,
        DomainCode::ClockNotSettable,
        DomainCode::FlagUnknown,
        DomainCode::FlagCombination,
        DomainCode::RangeNsec,
        DomainCode::RangeSec,
        DomainCode::DomainEmpty,
        DomainCode::DomainSumMismatch,
        DomainCode::OrderViolation,
        DomainCode::LedgerRegress,
        DomainCode::NotReproducible,
    ];
}

// ── 触发域 ────────────────────────────────────────────────────────────
/// EINVAL 的三个触发域（外加「未触发」用于放行格）。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TriggerClass {
    NotTriggered = 0,
    BadClock = 1,
    BadFlags = 2,
    RangeViolation = 3,
}

impl TriggerClass {
    /// 域码。
    pub fn code(self) -> u32 {
        self as u32
    }

    /// 域名。
    pub fn name(self) -> &'static str {
        match self {
            TriggerClass::NotTriggered => "未触发",
            TriggerClass::BadClock => "非法时钟",
            TriggerClass::BadFlags => "非法标志",
            TriggerClass::RangeViolation => "值域越界",
        }
    }

    /// 是否属于拒绝域。
    pub fn is_reject(self) -> bool {
        !matches!(self, TriggerClass::NotTriggered)
    }
}

/// 时间资源错误面。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Surface {
    ClockGettime = 0,
    ClockSettime = 1,
    NanoSleep = 2,
    ClockNanoSleep = 3,
    TimerfdCreate = 4,
    TimerfdSettime = 5,
    TimerCreate = 6,
    TimerSettime = 7,
}

impl Surface {
    /// 面码。
    pub fn code(self) -> u32 {
        self as u32
    }

    /// 面名。
    pub fn name(self) -> &'static str {
        match self {
            Surface::ClockGettime => "clock_gettime",
            Surface::ClockSettime => "clock_settime",
            Surface::NanoSleep => "nanosleep",
            Surface::ClockNanoSleep => "clock_nanosleep",
            Surface::TimerfdCreate => "timerfd_create",
            Surface::TimerfdSettime => "timerfd_settime",
            Surface::TimerCreate => "timer_create",
            Surface::TimerSettime => "timer_settime",
        }
    }

    /// 该面是否读取时钟 id。
    pub fn uses_clock(self) -> bool {
        matches!(
            self,
            Surface::ClockGettime
                | Surface::ClockSettime
                | Surface::ClockNanoSleep
                | Surface::TimerCreate
        )
    }
}

/// 面码 → 面。
pub fn surface_from_code(code: u32) -> Option<Surface> {
    match code {
        0 => Some(Surface::ClockGettime),
        1 => Some(Surface::ClockSettime),
        2 => Some(Surface::NanoSleep),
        3 => Some(Surface::ClockNanoSleep),
        4 => Some(Surface::TimerfdCreate),
        5 => Some(Surface::TimerfdSettime),
        6 => Some(Surface::TimerCreate),
        7 => Some(Surface::TimerSettime),
        _ => None,
    }
}

// ── 时钟域 ────────────────────────────────────────────────────────────
/// 全局合法时钟名（10 是空洞，故意不在表内）。
pub fn clock_name(id: i32) -> Option<&'static str> {
    match id {
        CLOCK_REALTIME => Some("CLOCK_REALTIME"),
        CLOCK_MONOTONIC => Some("CLOCK_MONOTONIC"),
        CLOCK_PROCESS_CPUTIME_ID => Some("CLOCK_PROCESS_CPUTIME_ID"),
        CLOCK_THREAD_CPUTIME_ID => Some("CLOCK_THREAD_CPUTIME_ID"),
        CLOCK_MONOTONIC_RAW => Some("CLOCK_MONOTONIC_RAW"),
        CLOCK_REALTIME_COARSE => Some("CLOCK_REALTIME_COARSE"),
        CLOCK_MONOTONIC_COARSE => Some("CLOCK_MONOTONIC_COARSE"),
        CLOCK_BOOTTIME => Some("CLOCK_BOOTTIME"),
        CLOCK_REALTIME_ALARM => Some("CLOCK_REALTIME_ALARM"),
        CLOCK_BOOTTIME_ALARM => Some("CLOCK_BOOTTIME_ALARM"),
        CLOCK_TAI => Some("CLOCK_TAI"),
        _ => None,
    }
}

/// 时钟是否登记。
pub fn clock_known(id: i32) -> bool {
    clock_name(id).is_some()
}

/// clock_settime 允许的时钟（可设时钟集）。
pub fn clock_settable(id: i32) -> bool {
    matches!(id, CLOCK_REALTIME | CLOCK_REALTIME_ALARM | CLOCK_TAI)
}

/// clock_nanosleep 允许的时钟（POSIX 最小集）。
pub fn nanosleep_clock_ok(id: i32) -> bool {
    matches!(id, CLOCK_REALTIME | CLOCK_MONOTONIC | CLOCK_BOOTTIME)
}

/// 值域：tv_sec 非负且 tv_nsec 落在 [0, 1e9)。
pub fn ts_valid(sec: i64, nsec: i64) -> bool {
    sec >= 0 && nsec >= 0 && nsec < NS_PER_SEC
}

/// sigevent.sigev_notify 合法值（承载在用例的 flags 字段）。
pub fn sigevent_notify_ok(notify: u32) -> bool {
    matches!(notify, 0 | 1 | 2 | 4)
}

// ── 判定 ──────────────────────────────────────────────────────────────
/// 判定结论：errno + 触发域。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Verdict {
    pub errno: i32,
    pub class: TriggerClass,
}

impl Verdict {
    /// 放行。
    pub const fn accept() -> Verdict {
        Verdict {
            errno: 0,
            class: TriggerClass::NotTriggered,
        }
    }

    /// 以某域拒绝（errno 固定 EINVAL）。
    pub const fn reject(class: TriggerClass) -> Verdict {
        Verdict {
            errno: EINVAL,
            class,
        }
    }

    /// 是否放行。
    pub fn accepted(&self) -> bool {
        self.errno == 0 && !self.class.is_reject()
    }
}

/// 一个冻结用例。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ProbeCase {
    pub surface: Surface,
    /// 时钟 id（不使用时钟的面填 0，仅占位）。
    pub clock: i32,
    /// 标志位；timer_create 面承载 sigev_notify。
    pub flags: u32,
    /// timespec.tv_sec。
    pub sec: i64,
    /// timespec.tv_nsec。
    pub nsec: i64,
    /// 冻结期望：errno。
    pub expect_errno: i32,
    /// 冻结期望：触发域。
    pub expect_class: TriggerClass,
    pub note: &'static str,
}

/// 判定次序：时钟 → 标志 → 值域。
pub fn validate(c: &ProbeCase) -> Verdict {
    match c.surface {
        Surface::ClockGettime => {
            if !clock_known(c.clock) {
                return Verdict::reject(TriggerClass::BadClock);
            }
            Verdict::accept()
        }
        Surface::ClockSettime => {
            // 已知但不可设的时钟同样落在时钟域：语义是「该面不接受这个时钟」。
            if !clock_known(c.clock) || !clock_settable(c.clock) {
                return Verdict::reject(TriggerClass::BadClock);
            }
            Verdict::accept()
        }
        Surface::NanoSleep => {
            if !ts_valid(c.sec, c.nsec) {
                return Verdict::reject(TriggerClass::RangeViolation);
            }
            Verdict::accept()
        }
        Surface::ClockNanoSleep => {
            if !clock_known(c.clock) || !nanosleep_clock_ok(c.clock) {
                return Verdict::reject(TriggerClass::BadClock);
            }
            if c.flags & !TIMER_ABSTIME != 0 {
                return Verdict::reject(TriggerClass::BadFlags);
            }
            if !ts_valid(c.sec, c.nsec) {
                return Verdict::reject(TriggerClass::RangeViolation);
            }
            Verdict::accept()
        }
        Surface::TimerfdCreate => {
            if c.flags & !TFD_CREATE_MASK != 0 {
                return Verdict::reject(TriggerClass::BadFlags);
            }
            Verdict::accept()
        }
        Surface::TimerfdSettime => {
            if c.flags & !TFD_SETTIME_MASK != 0 {
                return Verdict::reject(TriggerClass::BadFlags);
            }
            if c.flags & TFD_TIMER_CANCEL_ON_SET != 0 && c.flags & TFD_TIMER_ABSTIME == 0 {
                return Verdict::reject(TriggerClass::BadFlags);
            }
            if !ts_valid(c.sec, c.nsec) {
                return Verdict::reject(TriggerClass::RangeViolation);
            }
            Verdict::accept()
        }
        Surface::TimerCreate => {
            if !clock_known(c.clock) {
                return Verdict::reject(TriggerClass::BadClock);
            }
            if !sigevent_notify_ok(c.flags) {
                return Verdict::reject(TriggerClass::BadFlags);
            }
            Verdict::accept()
        }
        Surface::TimerSettime => {
            if !ts_valid(c.sec, c.nsec) {
                return Verdict::reject(TriggerClass::RangeViolation);
            }
            Verdict::accept()
        }
    }
}

// ── 冻结用例表 ────────────────────────────────────────────────────────
/// 二十一个冻结用例：五个非法时钟、五个非法标志、四个值域越界、七个放行。
pub const PROBES: [ProbeCase; PROBE_CASES] = [
    ProbeCase {
        surface: Surface::ClockGettime,
        clock: CLOCK_REALTIME,
        flags: 0,
        sec: 0,
        nsec: 0,
        expect_errno: 0,
        expect_class: TriggerClass::NotTriggered,
        note: "实时钟读取：最常用路径必须放行",
    },
    ProbeCase {
        surface: Surface::ClockGettime,
        clock: 10,
        flags: 0,
        sec: 0,
        nsec: 0,
        expect_errno: EINVAL,
        expect_class: TriggerClass::BadClock,
        note: "时钟 10 是空洞：未登记 id 一律 EINVAL",
    },
    ProbeCase {
        surface: Surface::ClockGettime,
        clock: -1,
        flags: 0,
        sec: 0,
        nsec: 0,
        expect_errno: EINVAL,
        expect_class: TriggerClass::BadClock,
        note: "负时钟 id：符号位越界，必须拒绝",
    },
    ProbeCase {
        surface: Surface::ClockGettime,
        clock: CLOCK_TAI,
        flags: 0,
        sec: 0,
        nsec: 0,
        expect_errno: 0,
        expect_class: TriggerClass::NotTriggered,
        note: "TAI 是合法时钟：读路径放行",
    },
    ProbeCase {
        surface: Surface::ClockSettime,
        clock: CLOCK_MONOTONIC,
        flags: 0,
        sec: 0,
        nsec: 0,
        expect_errno: EINVAL,
        expect_class: TriggerClass::BadClock,
        note: "单调钟只读：设时钟必须 EINVAL 而非 EPERM",
    },
    ProbeCase {
        surface: Surface::ClockSettime,
        clock: CLOCK_REALTIME,
        flags: 0,
        sec: 0,
        nsec: 0,
        expect_errno: 0,
        expect_class: TriggerClass::NotTriggered,
        note: "实时钟可设：放行",
    },
    ProbeCase {
        surface: Surface::NanoSleep,
        clock: 0,
        flags: 0,
        sec: 0,
        nsec: NS_PER_SEC,
        expect_errno: EINVAL,
        expect_class: TriggerClass::RangeViolation,
        note: "纳秒等于 1e9：越界下界为开区间",
    },
    ProbeCase {
        surface: Surface::NanoSleep,
        clock: 0,
        flags: 0,
        sec: 0,
        nsec: -1,
        expect_errno: EINVAL,
        expect_class: TriggerClass::RangeViolation,
        note: "纳秒为负：符号越界",
    },
    ProbeCase {
        surface: Surface::NanoSleep,
        clock: 0,
        flags: 0,
        sec: -1,
        nsec: 0,
        expect_errno: EINVAL,
        expect_class: TriggerClass::RangeViolation,
        note: "秒为负：sleep 语义不接受负时长",
    },
    ProbeCase {
        surface: Surface::NanoSleep,
        clock: 0,
        flags: 0,
        sec: 0,
        nsec: 1,
        expect_errno: 0,
        expect_class: TriggerClass::NotTriggered,
        note: "一纳秒：最小正值必须放行",
    },
    ProbeCase {
        surface: Surface::ClockNanoSleep,
        clock: CLOCK_REALTIME,
        flags: 0x2,
        sec: 0,
        nsec: 0,
        expect_errno: EINVAL,
        expect_class: TriggerClass::BadFlags,
        note: "clock_nanosleep 只认 TIMER_ABSTIME：其余位 EINVAL",
    },
    ProbeCase {
        surface: Surface::ClockNanoSleep,
        clock: CLOCK_REALTIME,
        flags: TIMER_ABSTIME,
        sec: 0,
        nsec: 0,
        expect_errno: 0,
        expect_class: TriggerClass::NotTriggered,
        note: "绝对时间睡眠：放行",
    },
    ProbeCase {
        surface: Surface::ClockNanoSleep,
        clock: CLOCK_PROCESS_CPUTIME_ID,
        flags: 0,
        sec: 0,
        nsec: 0,
        expect_errno: EINVAL,
        expect_class: TriggerClass::BadClock,
        note: "CPU 时钟不可用于睡眠：能力子集外一律 EINVAL",
    },
    ProbeCase {
        surface: Surface::ClockNanoSleep,
        clock: 10,
        flags: 0x9,
        sec: 0,
        nsec: NS_PER_SEC,
        expect_errno: EINVAL,
        expect_class: TriggerClass::BadClock,
        note: "复合坏例：时钟先于标志、标志先于值域",
    },
    ProbeCase {
        surface: Surface::TimerfdCreate,
        clock: 0,
        flags: 0x20,
        sec: 0,
        nsec: 0,
        expect_errno: EINVAL,
        expect_class: TriggerClass::BadFlags,
        note: "timerfd_create 掩码外位：EINVAL",
    },
    ProbeCase {
        surface: Surface::TimerfdCreate,
        clock: 0,
        flags: TFD_NONBLOCK | TFD_CLOEXEC,
        sec: 0,
        nsec: 0,
        expect_errno: 0,
        expect_class: TriggerClass::NotTriggered,
        note: "NONBLOCK|CLOEXEC：合法组合放行",
    },
    ProbeCase {
        surface: Surface::TimerfdSettime,
        clock: 0,
        flags: 0x4,
        sec: 0,
        nsec: 0,
        expect_errno: EINVAL,
        expect_class: TriggerClass::BadFlags,
        note: "timerfd_settime 掩码外位：EINVAL",
    },
    ProbeCase {
        surface: Surface::TimerfdSettime,
        clock: 0,
        flags: TFD_TIMER_CANCEL_ON_SET,
        sec: 0,
        nsec: 0,
        expect_errno: EINVAL,
        expect_class: TriggerClass::BadFlags,
        note: "CANCEL_ON_SET 缺 ABSTIME：组合非法",
    },
    ProbeCase {
        surface: Surface::TimerfdSettime,
        clock: 0,
        flags: TFD_TIMER_ABSTIME | TFD_TIMER_CANCEL_ON_SET,
        sec: 0,
        nsec: 0,
        expect_errno: 0,
        expect_class: TriggerClass::NotTriggered,
        note: "ABSTIME|CANCEL_ON_SET：合法组合放行",
    },
    ProbeCase {
        surface: Surface::TimerCreate,
        clock: CLOCK_REALTIME,
        flags: 3,
        sec: 0,
        nsec: 0,
        expect_errno: EINVAL,
        expect_class: TriggerClass::BadFlags,
        note: "sigev_notify=3 未登记：EINVAL",
    },
    ProbeCase {
        surface: Surface::TimerSettime,
        clock: 0,
        flags: 0,
        sec: 0,
        nsec: NS_PER_SEC,
        expect_errno: EINVAL,
        expect_class: TriggerClass::RangeViolation,
        note: "定时器间隔纳秒越界：EINVAL",
    },
];

/// 逐例复算，返回（命中数, 结论快照）。
pub fn run_domain() -> (u32, Vec<Verdict>) {
    let mut matched = 0u32;
    let mut outs: Vec<Verdict> = Vec::new();
    let mut i = 0usize;
    while i < PROBES.len() {
        if let Some(c) = PROBES.get(i).copied() {
            let v = validate(&c);
            if v.errno == c.expect_errno && v.class == c.expect_class {
                matched = matched.saturating_add(1);
            }
            outs.push(v);
        }
        i = i.saturating_add(1);
    }
    (matched, outs)
}

// ── 三域落账 ──────────────────────────────────────────────────────────
/// 域账本：总注入/检出/逃逸 + 三类计数 + 放行计数。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DomainLedger {
    pub injected: u32,
    pub detected: u32,
    pub escaped: u32,
    pub bad_clock: u32,
    pub bad_flags: u32,
    pub range: u32,
    pub accepted: u32,
}

impl DomainLedger {
    /// 零值。
    pub const fn zero() -> DomainLedger {
        DomainLedger {
            injected: 0,
            detected: 0,
            escaped: 0,
            bad_clock: 0,
            bad_flags: 0,
            range: 0,
            accepted: 0,
        }
    }

    /// 记一笔：按结论的触发域分桶（放行进 accepted）。
    pub fn record(&mut self, verdict: Verdict, caught: bool) {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        match verdict.class {
            TriggerClass::BadClock => self.bad_clock = self.bad_clock.saturating_add(1),
            TriggerClass::BadFlags => self.bad_flags = self.bad_flags.saturating_add(1),
            TriggerClass::RangeViolation => self.range = self.range.saturating_add(1),
            TriggerClass::NotTriggered => self.accepted = self.accepted.saturating_add(1),
        }
    }

    /// 三域之和。
    pub fn reject_sum(&self) -> u32 {
        self.bad_clock
            .saturating_add(self.bad_flags)
            .saturating_add(self.range)
    }

    /// 守恒。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }

    /// 三域非空。
    pub fn classes_nonempty(&self) -> bool {
        self.bad_clock > 0 && self.bad_flags > 0 && self.range > 0
    }
}

/// 注入：10 轮 × 21 例。
pub fn run_injection() -> DomainLedger {
    let mut ledger = DomainLedger::zero();
    let mut round = 0u32;
    while round < INJECT_ROUNDS {
        let mut i = 0usize;
        while i < PROBES.len() {
            if let Some(c) = PROBES.get(i).copied() {
                let v = validate(&c);
                let caught = v.errno == c.expect_errno && v.class == c.expect_class;
                ledger.record(v, caught);
            }
            i = i.saturating_add(1);
        }
        round = round.saturating_add(1);
    }
    ledger
}

/// 冻结用例的域分布（单轮）。
pub fn case_distribution() -> [u32; 4] {
    let mut counts = [0u32; 4];
    let mut i = 0usize;
    while i < PROBES.len() {
        if let Some(c) = PROBES.get(i) {
            let idx = c.expect_class.code() as usize;
            if let Some(slot) = counts.get_mut(idx) {
                *slot = slot.saturating_add(1);
            }
        }
        i = i.saturating_add(1);
    }
    counts
}

/// 面覆盖：八个面各至少一条用例。
pub fn surface_coverage() -> [u32; SURFACE_COUNT] {
    let mut counts = [0u32; SURFACE_COUNT];
    let mut i = 0usize;
    while i < PROBES.len() {
        if let Some(c) = PROBES.get(i) {
            let idx = c.surface.code() as usize;
            if let Some(slot) = counts.get_mut(idx) {
                *slot = slot.saturating_add(1);
            }
        }
        i = i.saturating_add(1);
    }
    counts
}

/// 与 Linux 语义一致：拒绝一律 EINVAL(22)，放行一律 0，且域与 errno 对应。
pub fn agrees_with_linux() -> bool {
    let mut i = 0usize;
    while i < PROBES.len() {
        match PROBES.get(i) {
            Some(c) => {
                if c.expect_class.is_reject() {
                    if c.expect_errno != EINVAL {
                        return false;
                    }
                } else if c.expect_errno != 0 {
                    return false;
                }
                let v = validate(c);
                if v.errno != c.expect_errno || v.class != c.expect_class {
                    return false;
                }
                if c.surface.uses_clock() && !clock_known(c.clock) && c.expect_class != TriggerClass::BadClock {
                    return false;
                }
            }
            None => return false,
        }
        i = i.saturating_add(1);
    }
    true
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
        i = i.saturating_add(1);
    }
    h
}

fn push_u32(out: &mut Vec<u8>, v: u32) {
    out.push((v >> 24) as u8);
    out.push((v >> 16) as u8);
    out.push((v >> 8) as u8);
    out.push(v as u8);
}

fn push_i64(out: &mut Vec<u8>, v: i64) {
    let bits = v as u64;
    push_u32(out, (bits >> 32) as u32);
    push_u32(out, bits as u32);
}

/// 规范语料指纹：面表 + 21 例 + 三域计数。
pub fn corpus_fingerprint() -> u32 {
    let mut out: Vec<u8> = Vec::new();
    push_u32(&mut out, SEG_BASE);
    push_u32(&mut out, PROBE_CASES as u32);
    let mut i = 0usize;
    while i < PROBES.len() {
        if let Some(c) = PROBES.get(i) {
            push_u32(&mut out, c.surface.code());
            push_u32(&mut out, c.clock as u32);
            push_u32(&mut out, c.flags);
            push_i64(&mut out, c.sec);
            push_i64(&mut out, c.nsec);
            push_u32(&mut out, c.expect_errno as u32);
            push_u32(&mut out, c.expect_class.code());
        }
        i = i.saturating_add(1);
    }
    let ledger = run_injection();
    push_u32(&mut out, ledger.bad_clock);
    push_u32(&mut out, ledger.bad_flags);
    push_u32(&mut out, ledger.range);
    push_u32(&mut out, ledger.accepted);
    push_u32(&mut out, ledger.reject_sum());
    let mut s = 0u32;
    while s < SURFACE_COUNT as u32 {
        if let Some(surf) = surface_from_code(s) {
            push_u32(&mut out, surf.code());
            push_u32(&mut out, if surf.uses_clock() { 1 } else { 0 });
        }
        s = s.saturating_add(1);
    }
    fnv1a(&out)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9321 判据集。
pub fn run_unx9321_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9321");

    let seg_ok = SEG_BASE % SEG_SPAN == 0
        && (SEG_WINDOW_LO..=SEG_WINDOW_HI).contains(&SEG_BASE)
        && SEG_BASE == 0x7BDC0;
    s.add(
        "段位守卫：0x40 对齐且落在本批窗口",
        seg_ok,
        "0x7BDC0..=0x7BDFF 归本卡，不得与前后卡交叉",
    );

    let known_ok = clock_known(0)
        && clock_known(1)
        && clock_known(2)
        && clock_known(3)
        && clock_known(4)
        && clock_known(5)
        && clock_known(6)
        && clock_known(7)
        && clock_known(8)
        && clock_known(9)
        && clock_known(11)
        && !clock_known(10)
        && !clock_known(12)
        && !clock_known(-1)
        && !clock_known(1024)
        && clock_name(10).is_none();
    s.add(
        "时钟闭集：十一个合法 id，10 是空洞",
        known_ok,
        "0..=9 与 11 合法；10/12/负数/1024 必须判未登记",
    );

    s.add(
        "时钟域：可设时钟仅 REALTIME/REALTIME_ALARM/TAI",
        clock_settable(0)
            && clock_settable(8)
            && clock_settable(11)
            && !clock_settable(1)
            && !clock_settable(2)
            && !clock_settable(4)
            && !clock_settable(7),
        "单调钟与 CPU 时钟只读，设时钟必须 EINVAL",
    );

    s.add(
        "时钟域：睡眠仅支持 REALTIME/MONOTONIC/BOOTTIME",
        nanosleep_clock_ok(0)
            && nanosleep_clock_ok(1)
            && nanosleep_clock_ok(7)
            && !nanosleep_clock_ok(2)
            && !nanosleep_clock_ok(3)
            && !nanosleep_clock_ok(8),
        "CPU 时钟不可睡眠，能力子集外一律 EINVAL",
    );

    let flags_ok = TFD_CREATE_MASK == (TFD_NONBLOCK | TFD_CLOEXEC)
        && TFD_SETTIME_MASK == 3
        && TIMER_ABSTIME == 1
        && TFD_NONBLOCK == 0x800
        && TFD_CLOEXEC == 0x80000;
    s.add(
        "标志域：三面掩码与位值冻结",
        flags_ok,
        "timerfd_create/timerfd_settime/clock_nanosleep 的掩码不得漂移",
    );

    let combo = ProbeCase {
        surface: Surface::TimerfdSettime,
        clock: 0,
        flags: TFD_TIMER_CANCEL_ON_SET,
        sec: 0,
        nsec: 0,
        expect_errno: EINVAL,
        expect_class: TriggerClass::BadFlags,
        note: "组合校验",
    };
    s.add(
        "标志域：CANCEL_ON_SET 必须与 ABSTIME 同现",
        validate(&combo).class == TriggerClass::BadFlags
            && validate(&combo).errno == EINVAL,
        "缺 ABSTIME 的 CANCEL_ON_SET 是组合非法，不是值域问题",
    );

    let range_ok = ts_valid(0, 0)
        && ts_valid(0, NS_PER_SEC - 1)
        && ts_valid(1, 0)
        && !ts_valid(0, NS_PER_SEC)
        && !ts_valid(0, -1)
        && !ts_valid(-1, 0);
    s.add(
        "值域：tv_nsec in [0,1e9) 且 tv_sec 非负",
        range_ok,
        "两个边界值放行，1e9 与负值拒绝",
    );

    let (matched, outs) = run_domain();
    s.add(
        "逐格落账：21 例全部与冻结期望一致",
        matched == PROBE_CASES as u32 && outs.len() == PROBE_CASES,
        "errno 与触发域两项全对才算该格落账",
    );

    let ledger = run_injection();
    s.add(
        "三域：非法时钟/非法标志/值域越界全非空",
        ledger.classes_nonempty()
            && ledger.bad_clock == CASE_BAD_CLOCK * INJECT_ROUNDS
            && ledger.bad_flags == CASE_BAD_FLAGS * INJECT_ROUNDS
            && ledger.range == CASE_RANGE * INJECT_ROUNDS
            && ledger.accepted == CASE_ACCEPTED * INJECT_ROUNDS,
        "逐域计数必须等于该域用例数乘轮次",
    );

    s.add(
        "三域之和：拒绝总数等于三域相加",
        ledger.reject_sum() == (CASE_BAD_CLOCK + CASE_BAD_FLAGS + CASE_RANGE) * INJECT_ROUNDS
            && ledger.reject_sum() + ledger.accepted == ledger.injected,
        "归不了类的拒绝就是不可诊断的 EINVAL",
    );

    let order_case = PROBES.get(13).copied();
    let order_ok = match order_case {
        Some(c) => {
            let v = validate(&c);
            c.clock == 10
                && c.flags == 0x9
                && c.nsec == NS_PER_SEC
                && v.class == TriggerClass::BadClock
                && v.errno == EINVAL
        }
        None => false,
    };
    s.add(
        "判定次序：时钟先于标志，标志先于值域",
        order_ok,
        "复合坏例只能落在第一个触发域",
    );

    let expect_inject = (PROBE_CASES as u32).saturating_mul(INJECT_ROUNDS);
    s.add(
        "注入账：10 轮 × 21 例全检出、逃逸为零",
        ledger.conserved()
            && ledger.escaped == 0
            && ledger.injected == expect_inject
            && ledger.detected == expect_inject,
        "210 次注入必须 210 次检出",
    );

    s.add(
        "与 Linux 语义一致：拒绝全 EINVAL、放行全 0",
        agrees_with_linux() && EINVAL == 22,
        "本面只产 EINVAL 与 0，其他 errno 视为语义漂移",
    );

    let cov = surface_coverage();
    let cov_sum: u32 = cov.iter().fold(0u32, |acc, v| acc.saturating_add(*v));
    s.add(
        "面覆盖：八个面各至少一条用例",
        cov.iter().all(|v| *v > 0) && cov_sum == PROBE_CASES as u32,
        "缺面即漏判，面表与实际用例必须对齐",
    );

    let dist = case_distribution();
    let d0 = dist.first().copied().unwrap_or(0);
    let d1 = dist.get(1).copied().unwrap_or(0);
    let d2 = dist.get(2).copied().unwrap_or(0);
    let d3 = dist.get(3).copied().unwrap_or(0);
    s.add(
        "域分布：5 非法时钟 / 5 非法标志 / 4 值域 / 7 放行",
        d1 == CASE_BAD_CLOCK
            && d2 == CASE_BAD_FLAGS
            && d3 == CASE_RANGE
            && d0 == CASE_ACCEPTED,
        "分布与冻结配额一致，多一例少一例都不合格",
    );

    let fp_a = corpus_fingerprint();
    let fp_b = corpus_fingerprint();
    s.add(
        "双跑一致：用例与三域计数指纹相等",
        fp_a == fp_b && fp_a != 0,
        "同输入两次必须逐字节相等",
    );

    s.add(
        "诊断码：12 个码文案非空",
        DomainCode::ALL.len() == 12 && DomainCode::ALL.iter().all(|c| !c.detail().is_empty()),
        "每个码都要给出可操作的中文建议",
    );

    let mut uniq = true;
    let mut a = 0usize;
    while a < DomainCode::ALL.len() {
        let mut b = a.saturating_add(1);
        while b < DomainCode::ALL.len() {
            if let (Some(x), Some(y)) = (DomainCode::ALL.get(a), DomainCode::ALL.get(b)) {
                if x.val() == y.val() {
                    uniq = false;
                }
            }
            b = b.saturating_add(1);
        }
        a = a.saturating_add(1);
    }
    s.add(
        "诊断码：12 个码值两两唯一",
        uniq,
        "段内闭集码不得重复",
    );

    let dom_text = format!(
        "钟{} 标{} 值{} 放{}",
        ledger.bad_clock, ledger.bad_flags, ledger.range, ledger.accepted
    );
    s.add_dyn(
        "三域落账快照",
        ledger.classes_nonempty() && ledger.conserved(),
        &dom_text,
    );
    let grid_text = format!("用例 {}/{} 面 {}", matched, PROBE_CASES, cov_sum);
    s.add_dyn(
        "用例格快照",
        matched == PROBE_CASES as u32,
        &grid_text,
    );
    let fp_text = format!("指纹 0x{:08X}", fp_a);
    s.add_dyn("指纹快照", fp_a == fp_b, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clock_set_membership() {
        assert_eq!(clock_known(CLOCK_REALTIME), true);
        assert_eq!(clock_known(CLOCK_TAI), true);
        assert_eq!(clock_known(10), false);
        assert_eq!(clock_known(12), false);
        assert_eq!(clock_known(-1), false);
        assert_eq!(clock_known(1024), false);
        assert_eq!(clock_name(10), None);
        assert_eq!(clock_name(CLOCK_BOOTTIME), Some("CLOCK_BOOTTIME"));
    }

    #[test]
    fn settable_and_sleepable_grids() {
        assert_eq!(clock_settable(CLOCK_REALTIME), true);
        assert_eq!(clock_settable(CLOCK_REALTIME_ALARM), true);
        assert_eq!(clock_settable(CLOCK_TAI), true);
        assert_eq!(clock_settable(CLOCK_MONOTONIC), false);
        assert_eq!(clock_settable(CLOCK_MONOTONIC_RAW), false);
        assert_eq!(nanosleep_clock_ok(CLOCK_MONOTONIC), true);
        assert_eq!(nanosleep_clock_ok(CLOCK_BOOTTIME), true);
        assert_eq!(nanosleep_clock_ok(CLOCK_THREAD_CPUTIME_ID), false);
        assert_eq!(nanosleep_clock_ok(CLOCK_REALTIME_ALARM), false);
    }

    #[test]
    fn timespec_boundaries() {
        assert_eq!(ts_valid(0, 0), true);
        assert_eq!(ts_valid(0, NS_PER_SEC - 1), true);
        assert_eq!(ts_valid(0, NS_PER_SEC), false);
        assert_eq!(ts_valid(0, -1), false);
        assert_eq!(ts_valid(-1, 0), false);
    }

    #[test]
    fn flags_masks_frozen() {
        assert_eq!(TFD_CREATE_MASK, 0x80800);
        assert_eq!(TFD_SETTIME_MASK, 3);
        assert_eq!(TIMER_ABSTIME, 1);
        assert_eq!(sigevent_notify_ok(2), true);
        assert_eq!(sigevent_notify_ok(3), false);
        assert_eq!(sigevent_notify_ok(5), false);
    }

    #[test]
    fn domain_grid_all_match() {
        let (matched, outs) = run_domain();
        assert_eq!(outs.len(), PROBE_CASES);
        assert_eq!(matched, PROBE_CASES as u32);
        assert_eq!(outs.iter().filter(|v| v.errno == EINVAL).count(), 14);
        assert_eq!(outs.iter().filter(|v| v.errno == 0).count(), 7);
    }

    #[test]
    fn class_distribution_is_frozen() {
        let d = case_distribution();
        assert_eq!(d.first().copied(), Some(CASE_ACCEPTED));
        assert_eq!(d.get(1).copied(), Some(CASE_BAD_CLOCK));
        assert_eq!(d.get(2).copied(), Some(CASE_BAD_FLAGS));
        assert_eq!(d.get(3).copied(), Some(CASE_RANGE));
        let sum: u32 = d.iter().fold(0u32, |a, v| a + *v);
        assert_eq!(sum, PROBE_CASES as u32);
    }

    #[test]
    fn ordering_prefers_clock() {
        let c = PROBES.get(13);
        assert_eq!(c.is_some(), true);
        let v = match c {
            Some(p) => validate(p),
            None => Verdict::accept(),
        };
        assert_eq!(v.class, TriggerClass::BadClock);
        assert_eq!(v.errno, EINVAL);
    }

    #[test]
    fn injection_is_conserved() {
        let l = run_injection();
        assert_eq!(l.injected, 210);
        assert_eq!(l.detected, 210);
        assert_eq!(l.escaped, 0);
        assert_eq!(l.conserved(), true);
        assert_eq!(l.bad_clock, 50);
        assert_eq!(l.bad_flags, 50);
        assert_eq!(l.range, 40);
        assert_eq!(l.accepted, 70);
    }

    #[test]
    fn rejects_are_always_einval() {
        assert_eq!(EINVAL, 22);
        assert_eq!(agrees_with_linux(), true);
        let mut i = 0usize;
        while i < PROBES.len() {
            if let Some(c) = PROBES.get(i) {
                if c.expect_class.is_reject() {
                    assert_eq!(c.expect_errno, EINVAL);
                } else {
                    assert_eq!(c.expect_errno, 0);
                }
            }
            i += 1;
        }
    }

    #[test]
    fn surface_coverage_is_complete() {
        let cov = surface_coverage();
        assert_eq!(cov.iter().all(|v| *v > 0), true);
        assert_eq!(cov.iter().fold(0u32, |a, v| a + *v), PROBE_CASES as u32);
        assert_eq!(surface_from_code(7).map(|s| s.name()), Some("timer_settime"));
        assert_eq!(surface_from_code(8).is_none(), true);
    }

    #[test]
    fn fingerprint_stable() {
        let a = corpus_fingerprint();
        let b = corpus_fingerprint();
        assert_eq!(a, b);
        assert_eq!(a != 0, true);
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx9321_checks();
        assert_eq!(s.all_passed(), true, "{}", s.red_items_text());
        assert_eq!(s.len() >= 12, true);
    }
}
