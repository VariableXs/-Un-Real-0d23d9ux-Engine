//! UNX-F9302 SA_RESTART 重启语义矩阵（逐 syscall 重启面） · 0x7B900段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 F9302 逐字）：
//!   UNX-F9302-J1 SA_RESTART 逐 syscall 重启面矩阵判据过，可重启族逐格重启
//!   可观测，不可重启族逐格 EINTR 格过。
//!
//! 设计要点：
//!   ① 判定模型是「syscall 类 × 信号处置」二维矩阵：20 条 syscall × 4 种处置
//!      = 80 格，每格冻结一个结果码（0 完成 / 1 重启 / 2 EINTR）。逐格 10 轮
//!      复算，格值与冻结表逐字节相等才算过——矩阵是判据落点，不是插图。
//!   ② syscall 三分：Fast（快速路径，永不被信号打断）、SlowRestartable（慢
//!      调用且 SA_RESTART 生效）、NeverRestart（慢调用但内核从不重启）。校验
//!      用两条方向相反的断言同时钉住：可重启格必须观测到重启，不可重启格必须
//!      观测到 EINTR，任何一边为 0 都判红。
//!   ③ 处置四档由 sigaction 三要素解析：handler == SIG_DFL(0) → Dfl，handler
//!      == SIG_IGN(1) → Ignore，否则看 flags & SA_RESTART(0x1000_0000)。解析是
//!      纯函数；禁止把 flags 直接当处置用——SIG_DFL 下即便带 SA_RESTART 位也
//!      不会中断，这一格最容易写错。
//!   ④ 状态机 Frame：Entered → Blocked → Signaled →（Restarted → Blocked |
//!      Returned）。restart 只允许从 Signaled 出发，且必须同时满足
//!      class.restartable() 与 disp.restart_flag()；坏次序（未 Signaled 就
//!      restart、已 Returned 又 deliver）一律拒为 PhaseOrder，不静默纠正。
//!   ⑤ 注入账本：80 格 × 10 轮 = 800 次注入，injected == detected + escaped 且
//!      escaped 恒为 0；另设「重启计数」「EINTR 计数」两本副账逐行累计，作为
//!      「逐格可观测」的量化落点。
//!   ⑥ 号面：号表按 x86_64 号手写，未登记号一律 ENOSYS(38)；参数面（flags 未
//!      定义位）回 EINVAL(22)；次序固定为号面先于参数面，倒序即 BadOrder。
//!   ⑦ 指纹：FNV-1a 32 位，逐字节喂入 80 格实测码 + 20 行双副账，双跑必须逐位
//!      相等——任何隐藏状态（表间缓存、遍历顺序依赖）都会漂移。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 诊断段基址（F9302 独占 0x7B900..=0x7B93F）。
pub const SEG_BASE: u32 = 0x7B900;

/// Linux errno 数值（正数口径，与内核 return -errno 的绝对值一致）。
pub const EPERM: i32 = 1;
pub const ESRCH: i32 = 3;
pub const EINTR: i32 = 4;
pub const EBADF: i32 = 9;
pub const EAGAIN: i32 = 11;
pub const EFAULT: i32 = 14;
pub const EINVAL: i32 = 22;
pub const ENOSYS: i32 = 38;

/// sigaction 位（Linux 通用架构值）。
pub const SA_NOCLDSTOP: u32 = 0x0000_0001;
pub const SA_NOCLDWAIT: u32 = 0x0000_0002;
pub const SA_SIGINFO: u32 = 0x0000_0004;
pub const SA_RESTORER: u32 = 0x0400_0000;
pub const SA_ONSTACK: u32 = 0x0800_0000;
pub const SA_RESTART: u32 = 0x1000_0000;
pub const SA_NODEFER: u32 = 0x4000_0000;
/// 本模块承认的全部 sigaction 位。
pub const KNOWN_SA_BITS: u32 =
    SA_NOCLDSTOP | SA_NOCLDWAIT | SA_SIGINFO | SA_RESTORER | SA_ONSTACK | SA_RESTART | SA_NODEFER;
/// 两种特殊 handler 句柄（Linux 里是常量指针值）。
pub const SIG_DFL: usize = 0;
pub const SIG_IGN: usize = 1;

/// 每格复算轮数、轨迹步上限、矩阵形状。
pub const INJECT_ROUNDS: u32 = 10;
pub const TRACE_SIGNALS: u8 = 4;
pub const DISPOSITIONS: usize = 4;
pub const SYSCALL_ROWS: usize = 20;
pub const MATRIX_CELLS: usize = SYSCALL_ROWS * DISPOSITIONS;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..=+0x0C 闭集）。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RestartCode {
    RestartNotObserved,
    RestartOnUnrestartable,
    EintrMissing,
    FastInterrupted,
    DispositionUnset,
    CellDrift,
    LedgerEscaped,
    PhaseOrder,
    SignalLost,
    BadOrder,
    SegmentViolation,
    NotReproducible,
}

impl RestartCode {
    /// 段内序号（repr(u16) 保证与判别值一致）。
    pub fn code(self) -> u16 { self as u16 }

    /// 段内唯一地址 = 段基址 + 1 + 序号（0x40 窗口内）。
    pub fn addr(self) -> u32 { SEG_BASE + 1 + self.code() as u32 }

    /// 中文诊断与可操作建议（闭集，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            RestartCode::RestartNotObserved => "可重启格未观测到重启：请检查 class.restartable() 与 disp.restart_flag() 的与条件",
            RestartCode::RestartOnUnrestartable => "不可重启族被重启：poll/select/nanosleep/epoll_wait 内核从不重启，SA_RESTART 对其无效",
            RestartCode::EintrMissing => "不可重启格未回 EINTR：慢调用被已安装 handler 的信号打断时必须返回 -EINTR",
            RestartCode::FastInterrupted => "快速路径被误中断：常规文件读写、getpid、fstat 不在可中断集合内",
            RestartCode::DispositionUnset => "处置解析错误：SIG_DFL/SIG_IGN 不得被 flags 覆盖，先判 handler 再判 SA_RESTART 位",
            RestartCode::CellDrift => "矩阵格漂移：实测结果码与冻结表不符，请核对 syscall 类归属与处置档位",
            RestartCode::LedgerEscaped => "注入账逃逸：injected 必须等于 detected 加 escaped 且 escaped 为 0",
            RestartCode::PhaseOrder => "状态机次序违规：restart 只允许从 Signaled 触发，deliver 只允许在 Blocked",
            RestartCode::SignalLost => "信号投递丢失：已投递数必须等于 Signaled 次数，请检查投递被短路",
            RestartCode::BadOrder => "判定次序倒置：号面 ENOSYS 必须先于参数面 EINVAL，先查号表再校验 flags",
            RestartCode::SegmentViolation => "段位越界：段基址必须为 0x7B900 且 0x40 对齐，不得侵占相邻卡段",
            RestartCode::NotReproducible => "双跑不一致：同一语料两次指纹不同，存在隐藏状态或非确定遍历",
        }
    }

    /// 闭集全量（文案完整性与码值唯一性检查用）。
    pub const ALL: &'static [RestartCode] = &[
        RestartCode::RestartNotObserved, RestartCode::RestartOnUnrestartable,
        RestartCode::EintrMissing, RestartCode::FastInterrupted,
        RestartCode::DispositionUnset, RestartCode::CellDrift,
        RestartCode::LedgerEscaped, RestartCode::PhaseOrder,
        RestartCode::SignalLost, RestartCode::BadOrder,
        RestartCode::SegmentViolation, RestartCode::NotReproducible,
    ];

    /// 码值是否落在本卡 0x40 窗口内。
    pub fn in_window(self) -> bool {
        let v = self.addr();
        v > SEG_BASE && v <= SEG_BASE + 0x3F
    }
}

// ── 分类与处置 ────────────────────────────────────────────────────────
/// 慢/快速调用与可重启性三分类。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SyscallClass { Fast, SlowRestartable, NeverRestart }

impl SyscallClass {
    /// 冻结编码（矩阵行形态）。
    pub fn code(self) -> u32 {
        match self {
            SyscallClass::Fast => 0,
            SyscallClass::SlowRestartable => 1,
            SyscallClass::NeverRestart => 2,
        }
    }
    /// 是否会被信号打断（快速路径不会）。
    pub fn blocks(self) -> bool { !matches!(self, SyscallClass::Fast) }
    /// 内核是否在 SA_RESTART 下重启它。
    pub fn restartable(self) -> bool { matches!(self, SyscallClass::SlowRestartable) }
}

/// 信号处置四档（handler + flags 联合解析结果）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Disposition { Dfl, Ignore, HandlerNoRestart, HandlerRestart }

impl Disposition {
    /// 列序 → 档位；越界 None（禁止隐式回绕）。
    pub fn from_ord(n: usize) -> Option<Disposition> {
        match n {
            0 => Some(Disposition::Dfl),
            1 => Some(Disposition::Ignore),
            2 => Some(Disposition::HandlerNoRestart),
            3 => Some(Disposition::HandlerRestart),
            _ => None,
        }
    }
    /// 全档位（固定顺序，遍历基准）。
    pub const fn all() -> [Disposition; DISPOSITIONS] {
        [Disposition::Dfl, Disposition::Ignore,
         Disposition::HandlerNoRestart, Disposition::HandlerRestart]
    }
    /// 是否有真 handler（Dfl/Ignore 不打断）。
    pub fn handler_installed(self) -> bool {
        matches!(self, Disposition::HandlerNoRestart | Disposition::HandlerRestart)
    }
    /// 是否带 SA_RESTART 语义。
    pub fn restart_flag(self) -> bool { matches!(self, Disposition::HandlerRestart) }
    /// 解析 sigaction：handler 优先于 flags。
    ///
    /// 不变量：SIG_DFL/SIG_IGN 是常量句柄，其 flags 位无意义；只有真 handler
    /// 才看 SA_RESTART 位。反过来写会把 Dfl+SA_RESTART 误判成可重启。
    pub fn resolve(handler: usize, flags: u32) -> Disposition {
        if handler == SIG_DFL {
            Disposition::Dfl
        } else if handler == SIG_IGN {
            Disposition::Ignore
        } else if flags & SA_RESTART != 0 {
            Disposition::HandlerRestart
        } else {
            Disposition::HandlerNoRestart
        }
    }
}

/// 单格判定结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Outcome { Completed, Restarted, Interrupted }

impl Outcome {
    /// 冻结码值：0/1/2（冻结表与指纹共用）。
    pub fn code(self) -> u8 {
        match self { Outcome::Completed => 0, Outcome::Restarted => 1, Outcome::Interrupted => 2 }
    }
}

// ── 号表 ──────────────────────────────────────────────────────────────
/// 一条 syscall 行：号、名字、类。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SyscallRow { pub nr: u16, pub name: &'static str, pub class: SyscallClass }

/// 二十条冻结行（x86_64 号；类归属按 signal(7) 的可中断/可重启表）。
pub const SYS_TABLE: [SyscallRow; SYSCALL_ROWS] = [
    SyscallRow { nr: 0, name: "read", class: SyscallClass::SlowRestartable },
    SyscallRow { nr: 1, name: "write", class: SyscallClass::SlowRestartable },
    SyscallRow { nr: 2, name: "open", class: SyscallClass::SlowRestartable },
    SyscallRow { nr: 16, name: "ioctl", class: SyscallClass::SlowRestartable },
    SyscallRow { nr: 61, name: "wait4", class: SyscallClass::SlowRestartable },
    SyscallRow { nr: 43, name: "accept", class: SyscallClass::SlowRestartable },
    SyscallRow { nr: 42, name: "connect", class: SyscallClass::SlowRestartable },
    SyscallRow { nr: 73, name: "flock", class: SyscallClass::SlowRestartable },
    SyscallRow { nr: 72, name: "fcntl", class: SyscallClass::SlowRestartable },
    SyscallRow { nr: 45, name: "recvfrom", class: SyscallClass::SlowRestartable },
    SyscallRow { nr: 44, name: "sendto", class: SyscallClass::SlowRestartable },
    SyscallRow { nr: 7, name: "poll", class: SyscallClass::NeverRestart },
    SyscallRow { nr: 23, name: "select", class: SyscallClass::NeverRestart },
    SyscallRow { nr: 35, name: "nanosleep", class: SyscallClass::NeverRestart },
    SyscallRow { nr: 230, name: "clock_nanosleep", class: SyscallClass::NeverRestart },
    SyscallRow { nr: 232, name: "epoll_wait", class: SyscallClass::NeverRestart },
    SyscallRow { nr: 128, name: "sigtimedwait", class: SyscallClass::NeverRestart },
    SyscallRow { nr: 70, name: "msgrcv", class: SyscallClass::NeverRestart },
    SyscallRow { nr: 39, name: "getpid", class: SyscallClass::Fast },
    SyscallRow { nr: 5, name: "fstat", class: SyscallClass::Fast },
];

/// 号面查表：命中返回行，未登记回 ENOSYS。
///
/// 次序不变量：号面先于参数面——必须先 lookup 成功才有资格校验 flags；
/// 倒序会让「未登记号 + 坏 flags」错报成 EINVAL。
pub fn lookup(nr: u16) -> Result<SyscallRow, i32> {
    let mut i = 0usize;
    while i < SYS_TABLE.len() {
        if let Some(row) = SYS_TABLE.get(i) {
            if row.nr == nr {
                return Ok(*row);
            }
        }
        i += 1;
    }
    Err(ENOSYS)
}

/// 参数面校验：flags 不得带未定义位。
pub fn validate_flags(flags: u32) -> Result<(), i32> {
    if flags & !KNOWN_SA_BITS != 0 { Err(EINVAL) } else { Ok(()) }
}

/// 复合入口：先号面再参数面（次序本身就是判据）。
pub fn resolve_call(nr: u16, flags: u32) -> Result<SyscallRow, i32> {
    let row = lookup(nr)?;
    validate_flags(flags)?;
    Ok(row)
}

/// 单格判定（矩阵与状态机共用同一函数，避免两份语义漂移）。
pub fn decide(class: SyscallClass, disp: Disposition) -> Outcome {
    if !class.blocks() || !disp.handler_installed() {
        return Outcome::Completed;
    }
    if class.restartable() && disp.restart_flag() { Outcome::Restarted } else { Outcome::Interrupted }
}

/// 冻结矩阵（20 行 × 4 列 = 80 格）。列序 = [Dfl, Ignore, HNR, HR]。
/// 行形态三种：快速 0 0 0 0；可重启 0 0 2 1；不可重启 0 0 2 2。
pub const FROZEN_CELLS: [u8; MATRIX_CELLS] = [
    0, 0, 2, 1, // 0 read
    0, 0, 2, 1, // 1 write
    0, 0, 2, 1, // 2 open
    0, 0, 2, 1, // 3 ioctl
    0, 0, 2, 1, // 4 wait4
    0, 0, 2, 1, // 5 accept
    0, 0, 2, 1, // 6 connect
    0, 0, 2, 1, // 7 flock
    0, 0, 2, 1, // 8 fcntl
    0, 0, 2, 1, // 9 recvfrom
    0, 0, 2, 1, // 10 sendto
    0, 0, 2, 2, // 11 poll
    0, 0, 2, 2, // 12 select
    0, 0, 2, 2, // 13 nanosleep
    0, 0, 2, 2, // 14 clock_nanosleep
    0, 0, 2, 2, // 15 epoll_wait
    0, 0, 2, 2, // 16 sigtimedwait
    0, 0, 2, 2, // 17 msgrcv
    0, 0, 0, 0, // 18 getpid
    0, 0, 0, 0, // 19 fstat
];

// ── 注入账本 ──────────────────────────────────────────────────────────
/// 注入账：注入 = 检出 + 逃逸；逃逸恒为 0。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Ledger { pub injected: u32, pub detected: u32, pub escaped: u32 }

impl Ledger {
    pub const fn zero() -> Ledger { Ledger { injected: 0, detected: 0, escaped: 0 } }

    /// 记一笔；返回守恒是否成立（不中止记账，便于观测漂移）。
    pub fn record(&mut self, caught: bool) -> bool {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        self.conserved()
    }
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
    /// 满格检出：非空、逃逸 0、守恒成立。
    pub fn perfect(&self) -> bool { self.injected > 0 && self.escaped == 0 && self.conserved() }
}

// ── 矩阵遍历 ──────────────────────────────────────────────────────────
/// 矩阵复算报告（逐行双副账 + 双向格计数）。
#[derive(Clone, Copy, Debug)]
pub struct MatrixReport {
    pub tested: u32,
    pub matched: u32,
    pub restarts: u32,
    pub eintrs: u32,
    pub restart_cells: u32,
    pub eintr_cells: u32,
    pub per_row_restarts: [u32; SYSCALL_ROWS],
    pub per_row_eintrs: [u32; SYSCALL_ROWS],
}

impl MatrixReport {
    pub const fn empty() -> MatrixReport {
        MatrixReport {
            tested: 0, matched: 0, restarts: 0, eintrs: 0, restart_cells: 0, eintr_cells: 0,
            per_row_restarts: [0u32; SYSCALL_ROWS], per_row_eintrs: [0u32; SYSCALL_ROWS],
        }
    }
    pub fn row_restarts(&self, i: usize) -> u32 {
        match self.per_row_restarts.get(i) { Some(v) => *v, None => 0 }
    }
    pub fn row_eintrs(&self, i: usize) -> u32 {
        match self.per_row_eintrs.get(i) { Some(v) => *v, None => 0 }
    }
    /// 每格都复算了 INJECT_ROUNDS 次且全命中。
    pub fn rounds_complete(&self) -> bool {
        self.tested == MATRIX_CELLS as u32 * INJECT_ROUNDS && self.matched == self.tested
    }
}

/// 全矩阵复算：80 格 × 10 轮，格值必须与冻结表一致且形态自洽。
///
/// 形态自洽是第二道闸：即便格值与表相同，也要验 class/disp 与结果码之间的
/// 逻辑关系，防止冻结表与判定函数被一起改错。
pub fn run_matrix() -> (MatrixReport, Ledger) {
    let mut rep = MatrixReport::empty();
    let mut led = Ledger::zero();
    let disps = Disposition::all();
    let mut i = 0usize;
    while i < SYS_TABLE.len() {
        if let Some(row) = SYS_TABLE.get(i).copied() {
            let mut d = 0usize;
            while d < DISPOSITIONS {
                if let Some(disp) = disps.get(d).copied() {
                    let expect = match FROZEN_CELLS.get(i * DISPOSITIONS + d) {
                        Some(v) => *v,
                        None => 0xFF,
                    };
                    let got = decide(row.class, disp);
                    let shape_ok = match got {
                        Outcome::Restarted => row.class.restartable() && disp.restart_flag(),
                        Outcome::Interrupted => disp.handler_installed()
                            && !(row.class.restartable() && disp.restart_flag()),
                        Outcome::Completed => !row.class.blocks() || !disp.handler_installed(),
                    };
                    let mut r = 0u32;
                    while r < INJECT_ROUNDS {
                        let caught = got.code() == expect && shape_ok;
                        led.record(caught);
                        rep.tested = rep.tested.saturating_add(1);
                        if caught {
                            rep.matched = rep.matched.saturating_add(1);
                        }
                        r += 1;
                    }
                    match got {
                        Outcome::Restarted => {
                            rep.restart_cells = rep.restart_cells.saturating_add(1);
                            rep.restarts = rep.restarts.saturating_add(INJECT_ROUNDS);
                            if let Some(cell) = rep.per_row_restarts.get_mut(i) {
                                *cell = cell.saturating_add(INJECT_ROUNDS);
                            }
                        }
                        Outcome::Interrupted => {
                            rep.eintr_cells = rep.eintr_cells.saturating_add(1);
                            rep.eintrs = rep.eintrs.saturating_add(INJECT_ROUNDS);
                            if let Some(cell) = rep.per_row_eintrs.get_mut(i) {
                                *cell = cell.saturating_add(INJECT_ROUNDS);
                            }
                        }
                        Outcome::Completed => {}
                    }
                }
                d += 1;
            }
        }
        i += 1;
    }
    (rep, led)
}

// ── 状态机 ────────────────────────────────────────────────────────────
/// 调用帧阶段。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Phase { Entered, Blocked, Signaled, Restarted, Returned }

/// 单次系统调用帧（纯值语义，无借用交叉）。
#[derive(Clone, Copy, Debug)]
pub struct Frame {
    pub nr: u16,
    pub class: SyscallClass,
    pub disp: Disposition,
    pub phase: Phase,
    pub restarts: u32,
    pub eintrs: u32,
    pub delivered: u32,
}

impl Frame {
    pub const fn open(nr: u16, class: SyscallClass, disp: Disposition) -> Frame {
        Frame {
            nr, class, disp, phase: Phase::Entered, restarts: 0, eintrs: 0, delivered: 0,
        }
    }
    /// 进入调用：快速路径直接返回，慢调用转阻塞。
    pub fn enter(&mut self) -> Result<(), RestartCode> {
        if self.phase != Phase::Entered {
            return Err(RestartCode::PhaseOrder);
        }
        if self.class.blocks() { self.phase = Phase::Blocked; } else { self.phase = Phase::Returned; }
        Ok(())
    }
    /// 投递信号：仅 Blocked 有效；Dfl/Ignore 不改变阶段（不打断）。
    pub fn deliver(&mut self) -> Result<bool, RestartCode> {
        if self.phase != Phase::Blocked {
            return Err(RestartCode::PhaseOrder);
        }
        self.delivered = self.delivered.saturating_add(1);
        if self.disp.handler_installed() {
            self.phase = Phase::Signaled;
            Ok(true)
        } else {
            Ok(false)
        }
    }
    /// 决策：Signaled → Restarted / Returned；Restarted → Returned（完成）。
    pub fn resume(&mut self) -> Result<Outcome, RestartCode> {
        match self.phase {
            Phase::Signaled => {
                if self.class.restartable() && self.disp.restart_flag() {
                    self.phase = Phase::Restarted;
                    self.restarts = self.restarts.saturating_add(1);
                    Ok(Outcome::Restarted)
                } else {
                    self.phase = Phase::Returned;
                    self.eintrs = self.eintrs.saturating_add(1);
                    Ok(Outcome::Interrupted)
                }
            }
            Phase::Restarted => {
                self.phase = Phase::Returned;
                Ok(Outcome::Completed)
            }
            _ => Err(RestartCode::PhaseOrder),
        }
    }
    /// 重启后重新阻塞（仅 Restarted → Blocked）。
    pub fn reblock(&mut self) -> Result<(), RestartCode> {
        if self.phase != Phase::Restarted {
            return Err(RestartCode::PhaseOrder);
        }
        self.phase = Phase::Blocked;
        Ok(())
    }
    /// 自然完成（仅 Blocked → Returned）。
    pub fn finish(&mut self) -> Result<(), RestartCode> {
        if self.phase != Phase::Blocked {
            return Err(RestartCode::PhaseOrder);
        }
        self.phase = Phase::Returned;
        Ok(())
    }
    pub fn done(&self) -> bool { self.phase == Phase::Returned }
}

/// 轨迹：最多 TRACE_SIGNALS 步的结果码序列。
#[derive(Clone, Copy, Debug)]
pub struct Trace {
    pub outcomes: [u8; TRACE_SIGNALS as usize],
    pub n: u8,
    pub restarts: u32,
    pub eintrs: u32,
    pub final_phase: Phase,
    pub order_error: bool,
}

impl Trace {
    /// 轨迹里每一步是否都是重启（空轨迹为假）。
    pub fn all_restarts(&self) -> bool {
        if self.n == 0 {
            return false;
        }
        let mut i = 0usize;
        while i < self.n as usize {
            match self.outcomes.get(i) {
                Some(c) => {
                    if *c != Outcome::Restarted.code() {
                        return false;
                    }
                }
                None => return false,
            }
            i += 1;
        }
        true
    }
}

/// 模拟一次带 n 个信号的调用（重启后重新阻塞，直到信号耗尽或 EINTR）。
///
/// 不变量：快速路径无 Blocked 可投递；一旦 EINTR 后续投递被判 PhaseOrder
/// 并停止（调用已返回）；restarts 等于成功重启次数，eintrs 属于 {0,1}。
pub fn simulate(row: SyscallRow, disp: Disposition, signals: u8) -> Trace {
    let mut f = Frame::open(row.nr, row.class, disp);
    let mut tr = Trace {
        outcomes: [0u8; TRACE_SIGNALS as usize], n: 0, restarts: 0, eintrs: 0,
        final_phase: f.phase, order_error: false,
    };
    if f.enter().is_err() {
        tr.order_error = true;
        return tr;
    }
    let mut k = 0u8;
    let mut stopped = false;
    while k < signals && !stopped {
        match f.deliver() {
            Ok(true) => match f.resume() {
                Ok(o) => {
                    if tr.n < TRACE_SIGNALS {
                        let at = tr.n as usize;
                        if let Some(slot) = tr.outcomes.get_mut(at) {
                            *slot = o.code();
                        }
                        tr.n = tr.n.saturating_add(1);
                    }
                    if o == Outcome::Restarted {
                        if f.reblock().is_err() {
                            tr.order_error = true;
                            stopped = true;
                        }
                    } else {
                        stopped = true;
                    }
                }
                Err(_) => {
                    tr.order_error = true;
                    stopped = true;
                }
            },
            Ok(false) => {}
            Err(_) => {
                tr.order_error = true;
                stopped = true;
            }
        }
        k = k.saturating_add(1);
    }
    if f.phase == Phase::Blocked && f.finish().is_err() {
        tr.order_error = true;
    }
    tr.restarts = f.restarts;
    tr.eintrs = f.eintrs;
    tr.final_phase = f.phase;
    tr
}

// ── errno 对照 ────────────────────────────────────────────────────────
/// (名称, 正值, 负值, 一句话)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EINTR", EINTR, -4, "系统调用被信号中断，可重启族应重入"),
    ("EINVAL", EINVAL, -22, "参数不合法（flags 未定义位）"),
    ("ENOSYS", ENOSYS, -38, "号面未登记，功能未实装"),
    ("EPERM", EPERM, -1, "权限不足"),
    ("ESRCH", ESRCH, -3, "目标不存在"),
    ("EBADF", EBADF, -9, "fd 非法"),
    ("EAGAIN", EAGAIN, -11, "非阻塞且暂不可用"),
    ("EFAULT", EFAULT, -14, "用户指针不可访问"),
];

/// 按正值查负值；表外返回 0。
pub fn linux_neg(code: i32) -> i32 {
    let mut i = 0usize;
    while i < ERRNO_TABLE.len() {
        if let Some(row) = ERRNO_TABLE.get(i) {
            if row.1 == code {
                return row.2;
            }
        }
        i += 1;
    }
    0
}

/// 与 Linux 逐码一致（表内且符号相反）。
pub fn agrees_with_linux(code: i32) -> bool { code > 0 && linux_neg(code) == -code }

// ── 确定性指纹 ────────────────────────────────────────────────────────
/// FNV-1a 32 位（纯整数、无随机、无墙钟）。
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

/// 规范语料：80 格实测码 + 20 行重启/EINTR 低八位。
pub fn corpus_bytes() -> Vec<u8> {
    let mut out: Vec<u8> = Vec::new();
    let disps = Disposition::all();
    let mut i = 0usize;
    while i < SYS_TABLE.len() {
        if let Some(row) = SYS_TABLE.get(i).copied() {
            let mut d = 0usize;
            while d < DISPOSITIONS {
                if let Some(disp) = disps.get(d).copied() {
                    out.push(decide(row.class, disp).code());
                }
                d += 1;
            }
            let tr = simulate(row, Disposition::HandlerRestart, TRACE_SIGNALS);
            out.push((tr.restarts & 0xFF) as u8);
            out.push((tr.eintrs & 0xFF) as u8);
        }
        i += 1;
    }
    out
}

/// 语料指纹（双跑对比用）。
pub fn corpus_fingerprint() -> u32 { fnv1a(&corpus_bytes()) }

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9302 判据集。
pub fn run_unx9302_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9302");

    let seg_ok = SEG_BASE == 0x7B900 && SEG_BASE % 0x40 == 0 && SEG_BASE < 0x7B940;
    s.add("段位守卫：0x7B900 且 0x40 对齐，不侵占 F9301/F9303", seg_ok,
          "段基址必须唯一落在本卡窗口，段间不得互相覆盖");

    let shape_ok = SyscallClass::Fast.code() == 0
        && SyscallClass::SlowRestartable.code() == 1
        && SyscallClass::NeverRestart.code() == 2
        && !SyscallClass::Fast.blocks()
        && SyscallClass::SlowRestartable.restartable()
        && !SyscallClass::NeverRestart.restartable();
    s.add("分类守卫：快速不阻塞、可重启族可重启、不可重启族不可重启", shape_ok,
          "三分语义是所有格判定的前提，任一取反矩阵即整体倒置");

    let disp_ok = Disposition::resolve(SIG_DFL, SA_RESTART) == Disposition::Dfl
        && Disposition::resolve(SIG_IGN, SA_RESTART | SA_NODEFER) == Disposition::Ignore
        && Disposition::resolve(0x1000, 0) == Disposition::HandlerNoRestart
        && Disposition::resolve(0x1000, SA_RESTART) == Disposition::HandlerRestart;
    s.add("处置解析：SIG_DFL/SIG_IGN 优先于 flags，只有真 handler 才看 SA_RESTART 位", disp_ok,
          "先判 handler 再判位；反序会把 Dfl+SA_RESTART 误判成可重启");

    let (rep, led) = run_matrix();
    s.add("矩阵：80 格 × 10 轮实测与冻结表逐格一致", rep.rounds_complete(),
          "格值漂移一格即判红，冻结表是判据的唯一权威");
    s.add("注入账：800 次注入全检出且逃逸为 0",
          led.perfect() && led.injected == MATRIX_CELLS as u32 * INJECT_ROUNDS,
          "injected 必须等于 detected 加 escaped，escaped 恒 0");

    let restart_family_ok = {
        let mut ok = rep.restart_cells == 11;
        let mut i = 0usize;
        while i < 11 {
            if rep.row_restarts(i) != INJECT_ROUNDS {
                ok = false;
            }
            i += 1;
        }
        ok
    };
    s.add("可重启族逐格重启可观测：11 行 × HandlerRestart 格各 10 次重启", restart_family_ok,
          "可重启族的 SA_RESTART 格必须逐行观测到重启，零重启即未覆盖");

    let never_family_ok = {
        let mut ok = rep.eintr_cells == 25;
        let mut i = 11usize;
        while i < 18 {
            if rep.row_eintrs(i) != INJECT_ROUNDS * 2 {
                ok = false;
            }
            i += 1;
        }
        ok
    };
    s.add("不可重启族逐格 EINTR：7 行 × 两档 handler 各 10 次 EINTR", never_family_ok,
          "poll/select/nanosleep/epoll_wait 等即便置 SA_RESTART 也必须回 EINTR");

    let fast_ok = rep.row_restarts(18) == 0
        && rep.row_restarts(19) == 0
        && rep.row_eintrs(18) == 0
        && rep.row_eintrs(19) == 0
        && decide(SyscallClass::Fast, Disposition::HandlerRestart) == Outcome::Completed;
    s.add("快速路径：getpid/fstat 四档全完成，重启与 EINTR 双零", fast_ok,
          "快速调用不在可中断集合内，任何重启/EINTR 计数都是伪覆盖");

    let dfl_ok = {
        let mut ok = true;
        let disps = Disposition::all();
        let mut i = 0usize;
        while i < SYS_TABLE.len() {
            if let Some(row) = SYS_TABLE.get(i).copied() {
                let mut d = 0usize;
                while d < 2 {
                    if let Some(disp) = disps.get(d).copied() {
                        if decide(row.class, disp) != Outcome::Completed {
                            ok = false;
                        }
                    }
                    d += 1;
                }
            }
            i += 1;
        }
        ok
    };
    s.add("默认/忽略处置：Dfl 与 Ignore 两列 40 格全完成", dfl_ok,
          "无 handler 时信号不打断系统调用，两列不得出现重启或 EINTR");

    let frame_ok = {
        let mut f = Frame::open(0, SyscallClass::SlowRestartable, Disposition::HandlerRestart);
        let e1 = f.enter().is_ok();
        let d1 = f.deliver().ok() == Some(true);
        let r1 = f.resume().ok() == Some(Outcome::Restarted);
        let b1 = f.reblock().is_ok();
        let d2 = f.deliver().ok() == Some(true);
        let r2 = f.resume().ok() == Some(Outcome::Restarted);
        let b2 = f.reblock().is_ok();
        let fin = f.finish().is_ok();
        e1 && d1 && r1 && b1 && d2 && r2 && b2 && fin
            && f.restarts == 2 && f.eintrs == 0 && f.done()
    };
    s.add("状态机：可重启帧两次重启后完成，restarts=2 且 eintrs=0", frame_ok,
          "restart 只能从 Signaled 出发，完成后不得再投递");

    let order_ok = {
        let mut f = Frame::open(7, SyscallClass::NeverRestart, Disposition::HandlerNoRestart);
        let bad_before_enter = f.deliver().err() == Some(RestartCode::PhaseOrder);
        let entered = f.enter().is_ok();
        let d = f.deliver().ok() == Some(true);
        let out = f.resume().ok() == Some(Outcome::Interrupted);
        let bad_after_eintr = f.deliver().err() == Some(RestartCode::PhaseOrder);
        let bad_resume = f.resume().err() == Some(RestartCode::PhaseOrder);
        bad_before_enter && entered && d && out && bad_after_eintr && bad_resume && f.eintrs == 1
    };
    s.add("状态机次序：未阻塞投递、EINTR 后再投递、已返回再决策全部拒为 PhaseOrder", order_ok,
          "坏次序必须显性拒绝，不得静默纠正成合法轨迹");

    let trace_ok = {
        let row = SyscallRow { nr: 0, name: "read", class: SyscallClass::SlowRestartable };
        let tr = simulate(row, Disposition::HandlerRestart, TRACE_SIGNALS);
        let never = simulate(
            SyscallRow { nr: 7, name: "poll", class: SyscallClass::NeverRestart },
            Disposition::HandlerRestart,
            TRACE_SIGNALS,
        );
        let quiet = simulate(row, Disposition::Dfl, 0);
        tr.all_restarts() && tr.n == TRACE_SIGNALS && tr.restarts == TRACE_SIGNALS as u32
            && never.n == 1 && never.eintrs == 1
            && quiet.n == 0 && quiet.final_phase == Phase::Returned
            && !tr.order_error && !never.order_error
    };
    s.add("轨迹：可重启帧四步全重启，不可重启帧一步即 EINTR，无信号帧静默完成", trace_ok,
          "轨迹是逐格可观测的逐步落点，步数与终态都必须精确");

    let nr_ok = lookup(0).map(|r| r.name) == Ok("read")
        && lookup(232).map(|r| r.name) == Ok("epoll_wait")
        && lookup(9999).err() == Some(ENOSYS)
        && resolve_call(9999, 0xFFFF_FFFF).err() == Some(ENOSYS)
        && resolve_call(0, 0xFFFF_FFFF).err() == Some(EINVAL)
        && resolve_call(0, SA_RESTART).is_ok();
    s.add("号面：登记号可查、未登记号 ENOSYS 且先于参数面 EINVAL", nr_ok,
          "号面必须先于参数面，未登记号不得被坏 flags 抢先报 EINVAL");

    let codes_ok = agrees_with_linux(EINTR) && agrees_with_linux(EINVAL)
        && agrees_with_linux(ENOSYS) && linux_neg(EINTR) == -4
        && linux_neg(ENOSYS) == -38 && !agrees_with_linux(0);
    s.add("Linux 逐码一致：EINTR=4/EINVAL=22/ENOSYS=38 表内可复算", codes_ok,
          "错误码只允许 errno 表内正值，表外码一律拒绝");

    let fp_a = corpus_fingerprint();
    let fp_b = corpus_fingerprint();
    s.add("双跑一致：规范语料指纹逐位相等", fp_a == fp_b,
          "同输入两次必须相等，隐藏状态即失败");

    let text_ok = RestartCode::ALL
        .iter()
        .all(|c| !c.detail().is_empty() && c.in_window());
    s.add("文案闭集：12 个诊断码均有中文建议且落在段内窗口", text_ok,
          "每个码必须能取到非空说明，且码值不越出本卡 0x40 窗口");

    let uniq_ok = RestartCode::ALL
        .iter()
        .enumerate()
        .all(|(i, a)| RestartCode::ALL.iter().skip(i + 1).all(|b| a.code() != b.code()));
    s.add("诊断码：12 个码值两两唯一", uniq_ok,
          "段内闭集码不得重复，否则红项无法唯一定位");

    let mut mono = led;
    let before = mono.injected;
    let mut k = 0u32;
    while k < 6 {
        mono.record(true);
        k += 1;
    }
    s.add("账目单调：再注入 6 次只增不减且守恒不破",
          mono.injected == before + 6 && mono.escaped == led.escaped && mono.conserved(),
          "累计计数禁止回退，追加不得改变既有逃逸数");

    let matrix_text = format!("矩阵 {}/{} 格逐格命中，重启 {} 次，EINTR {} 次",
        rep.matched, rep.tested, rep.restarts, rep.eintrs);
    s.add_dyn("矩阵快照", rep.rounds_complete(), &matrix_text);
    let ledger_text = format!("注入 {} / 检出 {} / 逃逸 {}",
        led.injected, led.detected, led.escaped);
    s.add_dyn("注入账目", led.perfect(), &ledger_text);
    let fp_text = format!("语料指纹 0x{:08X}", fp_a);
    s.add_dyn("指纹快照", fp_a == fp_b, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn class_partition_is_exclusive() {
        assert_eq!(SyscallClass::Fast.code(), 0);
        assert_eq!(SyscallClass::SlowRestartable.code(), 1);
        assert_eq!(SyscallClass::NeverRestart.code(), 2);
        assert_eq!(SyscallClass::Fast.blocks(), false);
        assert_eq!(SyscallClass::NeverRestart.restartable(), false);
    }

    #[test]
    fn disposition_resolution_prefers_handler() {
        assert_eq!(Disposition::resolve(SIG_DFL, SA_RESTART), Disposition::Dfl);
        assert_eq!(Disposition::resolve(SIG_IGN, 0), Disposition::Ignore);
        assert_eq!(Disposition::resolve(0x2000, 0), Disposition::HandlerNoRestart);
        assert_eq!(Disposition::resolve(0x2000, SA_RESTART), Disposition::HandlerRestart);
        assert_eq!(Disposition::from_ord(9), None);
        assert_eq!(Disposition::from_ord(3), Some(Disposition::HandlerRestart));
    }

    #[test]
    fn restartable_matrix_shape() {
        assert_eq!(decide(SyscallClass::SlowRestartable, Disposition::HandlerRestart),
                   Outcome::Restarted);
        assert_eq!(decide(SyscallClass::NeverRestart, Disposition::HandlerRestart),
                   Outcome::Interrupted);
        assert_eq!(decide(SyscallClass::NeverRestart, Disposition::Dfl), Outcome::Completed);
        assert_eq!(decide(SyscallClass::Fast, Disposition::HandlerNoRestart),
                   Outcome::Completed);
    }

    #[test]
    fn frozen_table_agrees_with_decide() {
        let disps = Disposition::all();
        let mut i = 0usize;
        while i < SYS_TABLE.len() {
            if let Some(row) = SYS_TABLE.get(i).copied() {
                let mut d = 0usize;
                while d < DISPOSITIONS {
                    if let Some(disp) = disps.get(d).copied() {
                        let expect = match FROZEN_CELLS.get(i * DISPOSITIONS + d) {
                            Some(v) => *v,
                            None => 0xFF,
                        };
                        assert_eq!(decide(row.class, disp).code(), expect);
                    }
                    d += 1;
                }
            }
            i += 1;
        }
    }

    #[test]
    fn matrix_report_is_full() {
        let (rep, led) = run_matrix();
        assert_eq!(rep.tested, 800);
        assert_eq!(rep.matched, 800);
        assert_eq!(rep.restart_cells, 11);
        assert_eq!(rep.eintr_cells, 25);
        assert_eq!(led.injected, 800);
        assert_eq!(led.escaped, 0);
        assert_eq!(led.conserved(), true);
    }

    #[test]
    fn frame_restarts_then_completes() {
        let mut f = Frame::open(0, SyscallClass::SlowRestartable, Disposition::HandlerRestart);
        assert_eq!(f.enter(), Ok(()));
        assert_eq!(f.deliver(), Ok(true));
        assert_eq!(f.resume(), Ok(Outcome::Restarted));
        assert_eq!(f.reblock(), Ok(()));
        assert_eq!(f.finish(), Ok(()));
        assert_eq!(f.restarts, 1);
        assert_eq!(f.eintrs, 0);
        assert_eq!(f.done(), true);
    }

    #[test]
    fn frame_rejects_bad_order() {
        let mut f = Frame::open(7, SyscallClass::NeverRestart, Disposition::HandlerNoRestart);
        assert_eq!(f.resume().err(), Some(RestartCode::PhaseOrder));
        assert_eq!(f.enter(), Ok(()));
        assert_eq!(f.deliver(), Ok(true));
        assert_eq!(f.resume(), Ok(Outcome::Interrupted));
        assert_eq!(f.deliver().err(), Some(RestartCode::PhaseOrder));
        assert_eq!(f.resume().err(), Some(RestartCode::PhaseOrder));
        assert_eq!(f.reblock().err(), Some(RestartCode::PhaseOrder));
    }

    #[test]
    fn trace_shapes() {
        let row = SyscallRow { nr: 1, name: "write", class: SyscallClass::SlowRestartable };
        let tr = simulate(row, Disposition::HandlerRestart, 3);
        assert_eq!(tr.n, 3);
        assert_eq!(tr.restarts, 3);
        assert_eq!(tr.all_restarts(), true);
        let nr = simulate(
            SyscallRow { nr: 35, name: "nanosleep", class: SyscallClass::NeverRestart },
            Disposition::HandlerRestart, 3);
        assert_eq!(nr.n, 1);
        assert_eq!(nr.eintrs, 1);
        assert_eq!(nr.final_phase, Phase::Returned);
    }

    #[test]
    fn lookup_and_flag_order() {
        assert_eq!(lookup(0).map(|r| r.name), Ok("read"));
        assert_eq!(lookup(4242).err(), Some(ENOSYS));
        assert_eq!(resolve_call(4242, 0xFFFF_FFFF).err(), Some(ENOSYS));
        assert_eq!(resolve_call(0, 0xFFFF_FFFF).err(), Some(EINVAL));
        assert_eq!(resolve_call(0, SA_RESTART).is_ok(), true);
    }

    #[test]
    fn fingerprint_is_stable() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
        assert_eq!(corpus_bytes().len(), MATRIX_CELLS + SYSCALL_ROWS * 2);
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx9302_checks();
        assert_eq!(s.all_passed(), true, "{}", s.red_items_text());
    }
}
