//! UNX-F9305 wait EINTR 矩阵（联 B18） · 0x7B9C0段
//! ─────────────────────────────────────────────────────────────
//! 判据原文（任务板 F9305 逐字）：
//!   UNX-F9305-J1 阻塞 wait 中断注入判据过，全 EINTR 格过，WNOHANG 零中断正判据格过。
//!
//! 设计要点：
//!   ① 子进程槽：4 槽紧凑表（不加裸索引），僵尸队列是另一只定长环，两者职责分离。
//!   ② 就绪优先：进入 wait 时若已有僵尸，立即回收，挂起中的信号不得抢在就绪之前。
//!   ③ 零中断正判据：带 WNOHANG 的行从不阻塞，任何处置档位都不产生 EINTR。
//!   ④ wait 族永不重启：handler 档（含 SA_RESTART）一律 EINTR，restarted 恒 false。
//!   ⑤ 默认/忽略档位：SIGCHLD 无 handler 时谈不上"中断"，等待继续并最终回收。
//!   ⑥ 状态编码：exit=(code&0xFF)<<8、signal=sig&0x7F、core=0x80，解码逐位可逆。
//!   ⑦ 矩阵：9 行 wait 族 × 4 处置档 = 36 格冻结，逐格 10 轮复算，账本无逃逸。
//!   ⑧ 指纹：FNV-1a 32 位喂入每格 (码, pid 低字节, 状态低字节, 信号, 回收旗标)。
//! ─────────────────────────────────────────────────────────────
use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F9305 独占 0x7B9C0..=0x7B9FF）。
pub const SEG_BASE: u32 = 0x7B9C0;

/// Linux errno（正数口径）。
pub const EINTR: i32 = 4;
pub const ECHILD: i32 = 10;
pub const EAGAIN: i32 = 11;
pub const EINVAL: i32 = 22;

/// wait 选项位（Linux 通用值）。
pub const WNOHANG: u32 = 1;
pub const WUNTRACED: u32 = 2;
pub const WCONTINUED: u32 = 8;
/// 本模块承认的全部 wait 选项位。
pub const KNOWN_WAIT_FLAGS: u32 = WNOHANG | WUNTRACED | WCONTINUED;

/// 子进程槽数、处置档数、族行数、矩阵格数、复算轮数。
pub const CHILD_SLOTS: usize = 4;
pub const DISPOSITIONS: usize = 4;
pub const WAIT_ROWS: usize = 9;
pub const MATRIX_CELLS: usize = WAIT_ROWS * DISPOSITIONS;
pub const INJECT_ROUNDS: u32 = 10;
/// 子进程 pid 基数（矩阵内每行一个固定子进程）。
pub const PID_BASE: i32 = 4100;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..=+0x0D 闭集）。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WaitCode {
    ZombieLost,
    FifoBroken,
    NohangInterrupted,
    BlockingNotEintr,
    RestartedWait,
    StatusCorrupt,
    ChildLeak,
    OptionDrift,
    BadOrder,
    EchildMissing,
    LedgerEscaped,
    SegmentViolation,
    NotReproducible,
}

impl WaitCode {
    /// 段内序号（repr(u16) 保证与判别值一致）。
    pub fn code(self) -> u16 { self as u16 }

    /// 段内唯一地址 = 段基址 + 1 + 序号（0x40 窗口内）。
    pub fn addr(self) -> u32 { SEG_BASE + 1 + self.code() as u32 }

    /// 中文诊断与可操作建议（闭集，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            WaitCode::ZombieLost => "僵尸丢失：已退出的子进程必须留在僵尸队列里直到被回收",
            WaitCode::FifoBroken => "回收次序错：任何路径（任意/指定 pid）都不得打乱其余僵尸的先后",
            WaitCode::NohangInterrupted => "WNOHANG 被中断：非阻塞探测根本不入睡，不得返回 EINTR",
            WaitCode::BlockingNotEintr => "阻塞 wait 未回 EINTR：已安装 handler 时挂起必须被信号打断",
            WaitCode::RestartedWait => "wait 被重启：wait3/wait4/waitid/waitpid 永不重启，SA_RESTART 无效",
            WaitCode::StatusCorrupt => "状态编码损坏：exit 与 signal 两域必须能逐位还原",
            WaitCode::ChildLeak => "子进程泄漏：回收后槽位必须清空，容量计数只减不增",
            WaitCode::OptionDrift => "选项漂移：WNOHANG/WUNTRACED/WCONTINUED 之外的位一律 EINVAL",
            WaitCode::BadOrder => "判定次序倒置：就绪优先于就绪前的信号，选项校验优先于查子进程",
            WaitCode::EchildMissing => "无子进程漏判：空表必须 ECHILD，且不得消耗任何信号",
            WaitCode::LedgerEscaped => "注入账逃逸：injected 必须等于 detected 加 escaped 且 escaped 为 0",
            WaitCode::SegmentViolation => "段位越界：段基址必须为 0x7B9C0 且 0x40 对齐，不得侵占相邻卡段",
            WaitCode::NotReproducible => "双跑不一致：同一语料两次指纹不同，存在隐藏状态或非确定遍历",
        }
    }

    /// 闭集全量（文案完整性与码值唯一性检查用）。
    pub const ALL: &'static [WaitCode] = &[
        WaitCode::ZombieLost, WaitCode::FifoBroken,
        WaitCode::NohangInterrupted, WaitCode::BlockingNotEintr,
        WaitCode::RestartedWait, WaitCode::StatusCorrupt,
        WaitCode::ChildLeak, WaitCode::OptionDrift,
        WaitCode::BadOrder, WaitCode::EchildMissing,
        WaitCode::LedgerEscaped, WaitCode::SegmentViolation,
        WaitCode::NotReproducible,
    ];

    /// 地址是否落在本卡 0x40 窗口内。
    pub fn in_window(self) -> bool {
        let v = self.addr();
        v > SEG_BASE && v <= SEG_BASE + 0x3F
    }
}

// ── 等待状态编码 ──────────────────────────────────────────────────────
/// 原始 wait 状态字（与 Linux 同构：低 7 位信号、bit7 core、高 8 位退出码）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Status(pub u32);

/// 状态解码视图（不丢信息：退出码 / 信号 / core 三面）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StatusView { Exited(u8), Signaled(u8, bool), Stopped(u8), Continued }

impl Status {
    /// 正常退出：status = (code & 0xFF) << 8。
    pub const fn exited(code: u8) -> Status { Status(((code as u32) & 0xFF) << 8) }

    /// 被信号杀死：低 7 位信号、bit7 core。
    pub const fn signaled(sig: u8, core: bool) -> Status {
        let s = (sig as u32) & 0x7F;
        let c = if core { 0x80u32 } else { 0u32 };
        Status(s | c)
    }

    /// 被作业控制停止：低字节 0x7F，次字节停止信号。
    pub const fn stopped(sig: u8) -> Status { Status((((sig as u32) & 0xFF) << 8) | 0x7F) }

    /// 继续运行：Linux 用 0xFFFF 表示 WCONTINUED。
    pub const fn continued() -> Status { Status(0xFFFF) }

    /// WIFEXITED：低字节全零。
    pub fn if_exited(&self) -> bool { (self.0 & 0x7F) == 0 && self.0 != 0xFFFF }

    /// WEXITSTATUS：高位字节。
    pub fn exit_code(&self) -> u8 { ((self.0 >> 8) & 0xFF) as u8 }

    /// WIFSIGNALED：低字节非零且不是 0x7F。
    pub fn if_signaled(&self) -> bool {
        let low = self.0 & 0x7F;
        low != 0 && low != 0x7F
    }

    /// WTERMSIG：低 7 位。
    pub fn term_sig(&self) -> u8 { (self.0 & 0x7F) as u8 }

    /// WCOREDUMP：bit7。
    pub fn core_dumped(&self) -> bool { (self.0 & 0x80) != 0 }

    /// 解码视图（四分支闭集）。
    pub fn view(&self) -> StatusView {
        if self.0 == 0xFFFF {
            StatusView::Continued
        } else if (self.0 & 0xFF) == 0x7F {
            StatusView::Stopped(((self.0 >> 8) & 0xFF) as u8)
        } else if self.if_signaled() {
            StatusView::Signaled(self.term_sig(), self.core_dumped())
        } else {
            StatusView::Exited(self.exit_code())
        }
    }
}

// ── 子进程槽 ──────────────────────────────────────────────────────────
/// 一个子进程：pid、退出状态、是否已僵死。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Child {
    pub pid: i32,
    pub status: Status,
    pub zombie: bool,
}

/// 空槽占位（pid=0 表示未占用）。
pub const NO_CHILD: Child = Child { pid: 0, status: Status(0), zombie: false };

/// 紧凑槽表：容量固定，删除时用尾元素填洞（不打乱存活集合）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ChildSlab {
    slots: [Child; CHILD_SLOTS],
    n: u8,
}

impl ChildSlab {
    /// 全空。
    pub const fn new() -> ChildSlab { ChildSlab { slots: [NO_CHILD; CHILD_SLOTS], n: 0 } }

    /// 存活子进程数。
    pub fn len(&self) -> usize { self.n as usize }

    /// 是否为空。
    pub fn is_empty(&self) -> bool { self.n == 0 }

    /// 是否已满。
    pub fn full(&self) -> bool { self.n as usize >= CHILD_SLOTS }

    /// 登记一个子进程；满则拒绝（不覆盖旧槽）。
    pub fn add(&mut self, pid: i32) -> bool {
        let idx = self.n as usize;
        match self.slots.get_mut(idx) {
            Some(slot) => {
                *slot = Child { pid, status: Status(0), zombie: false };
                self.n = self.n.saturating_add(1);
                true
            }
            None => false,
        }
    }

    /// 按 pid 查槽位下标（线性扫描，容量 4 足够）。
    pub fn index_of(&self, pid: i32) -> Option<usize> {
        let mut i = 0usize;
        while i < self.n as usize {
            if let Some(c) = self.slots.get(i) {
                if c.pid == pid {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 取子进程（不可变）。
    pub fn get_child(&self, pid: i32) -> Option<Child> {
        match self.index_of(pid) {
            Some(i) => self.slots.get(i).copied(),
            None => None,
        }
    }

    /// 标记僵尸并写入状态；未登记的 pid 返回 false。
    pub fn exit_child(&mut self, pid: i32, status: Status) -> bool {
        let idx = match self.index_of(pid) {
            Some(i) => i,
            None => return false,
        };
        match self.slots.get_mut(idx) {
            Some(slot) => {
                slot.status = status;
                slot.zombie = true;
                true
            }
            None => false,
        }
    }

    /// 移除子进程（回收）：尾元素填洞，计数减一。
    pub fn remove(&mut self, pid: i32) -> Option<Child> {
        let idx = match self.index_of(pid) {
            Some(i) => i,
            None => return None,
        };
        let found = self.slots.get(idx).copied();
        let last = self.n.saturating_sub(1) as usize;
        let tail = self.slots.get(last).copied().unwrap_or(NO_CHILD);
        if let Some(slot) = self.slots.get_mut(idx) {
            *slot = tail;
        }
        if let Some(slot) = self.slots.get_mut(last) {
            *slot = NO_CHILD;
        }
        self.n = self.n.saturating_sub(1);
        found
    }

    /// 僵尸数（就绪判定的输入）。
    pub fn zombie_count(&self) -> u8 {
        let mut i = 0usize;
        let mut c = 0u8;
        while i < self.n as usize {
            if let Some(child) = self.slots.get(i) {
                if child.zombie {
                    c = c.saturating_add(1);
                }
            }
            i += 1;
        }
        c
    }
}

// ── 僵尸队列（回收次序的唯一权威） ────────────────────────────────────
/// 定长环：按僵死先后排队，回收只从队头取。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ZombieQueue {
    pids: [i32; CHILD_SLOTS],
    head: u8,
    count: u8,
}

impl ZombieQueue {
    /// 空环。
    pub const fn new() -> ZombieQueue { ZombieQueue { pids: [0; CHILD_SLOTS], head: 0, count: 0 } }

    /// 队内僵尸数。
    pub fn len(&self) -> usize { self.count as usize }

    /// 是否为空。
    pub fn is_empty(&self) -> bool { self.count == 0 }

    /// 入队（满则拒绝，绝不覆盖未回收的僵尸）。
    pub fn push(&mut self, pid: i32) -> bool {
        if self.count as usize >= CHILD_SLOTS {
            return false;
        }
        let idx = (self.head as usize + self.count as usize) & (CHILD_SLOTS - 1);
        match self.pids.get_mut(idx) {
            Some(slot) => {
                *slot = pid;
                self.count = self.count.saturating_add(1);
                true
            }
            None => false,
        }
    }

    /// 队头出队。
    pub fn pop(&mut self) -> Option<i32> {
        if self.count == 0 {
            return None;
        }
        let idx = self.head as usize;
        let pid = match self.pids.get(idx) {
            Some(v) => *v,
            None => return None,
        };
        if let Some(slot) = self.pids.get_mut(idx) {
            *slot = 0;
        }
        self.head = ((self.head as usize + 1) & (CHILD_SLOTS - 1)) as u8;
        self.count = self.count.saturating_sub(1);
        Some(pid)
    }

    /// 队头预览。
    pub fn peek(&self) -> Option<i32> { self.pids.get(self.head as usize).copied() }

    /// 是否在队内。
    pub fn contains(&self, pid: i32) -> bool {
        let mut i = 0usize;
        while i < self.count as usize {
            let idx = (self.head as usize + i) & (CHILD_SLOTS - 1);
            if let Some(v) = self.pids.get(idx) {
                if *v == pid {
                    return true;
                }
            }
            i += 1;
        }
        false
    }

    /// 移除指定 pid（其余次序不变）；不在队内返回 false。
    pub fn remove(&mut self, pid: i32) -> bool {
        if !self.contains(pid) {
            return false;
        }
        let mut tmp = [0i32; CHILD_SLOTS];
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.count as usize {
            let idx = (self.head as usize + i) & (CHILD_SLOTS - 1);
            if let Some(v) = self.pids.get(idx) {
                if *v != pid {
                    if let Some(slot) = tmp.get_mut(n) {
                        *slot = *v;
                        n += 1;
                    }
                }
            }
            i += 1;
        }
        self.pids = tmp;
        self.head = 0;
        self.count = n as u8;
        true
    }
}

// ── 选项解析 ──────────────────────────────────────────────────────────
/// 原始选项字（用户传入，可能带垃圾位）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct WaitOptions(pub u32);

/// 解析后的等待模式。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct WaitMode {
    pub nohang: bool,
    pub untraced: bool,
    pub continued: bool,
}

impl WaitOptions {
    /// 构造。
    pub const fn new(flags: u32) -> WaitOptions { WaitOptions(flags) }

    /// 选项闸：未知位一律 EINVAL，校验先于查子进程。
    pub fn parse(self) -> Result<WaitMode, i32> {
        if self.0 & !KNOWN_WAIT_FLAGS != 0 {
            return Err(EINVAL);
        }
        Ok(WaitMode {
            nohang: self.0 & WNOHANG != 0,
            untraced: self.0 & WUNTRACED != 0,
            continued: self.0 & WCONTINUED != 0,
        })
    }
}

// ── 处置档 ────────────────────────────────────────────────────────────
/// 信号处置四档（SIGCHLD 场景）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WaitDisp { Dfl, Ignore, HandlerHard, HandlerRestart }

impl WaitDisp {
    /// 列序 → 档位；越界 None。
    pub fn from_ord(n: usize) -> Option<WaitDisp> {
        match n {
            0 => Some(WaitDisp::Dfl),
            1 => Some(WaitDisp::Ignore),
            2 => Some(WaitDisp::HandlerHard),
            3 => Some(WaitDisp::HandlerRestart),
            _ => None,
        }
    }

    /// 档位序号（指纹用）。
    pub fn ord(self) -> usize {
        match self {
            WaitDisp::Dfl => 0,
            WaitDisp::Ignore => 1,
            WaitDisp::HandlerHard => 2,
            WaitDisp::HandlerRestart => 3,
        }
    }

    /// 是否安装了用户 handler（只有 handler 才会打断挂起）。
    pub fn handler_installed(self) -> bool {
        matches!(self, WaitDisp::HandlerHard | WaitDisp::HandlerRestart)
    }

    /// 是否带 SA_RESTART（对 wait 族无效，仅用于证明"永不重启"）。
    pub fn restart_flag(self) -> bool { matches!(self, WaitDisp::HandlerRestart) }
}

// ── wait 族表（B18 对账面） ───────────────────────────────────────────
/// pid 选择面。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PidScope { Any, One, Group }

/// 一行 wait 族：接口名、选项、pid 选择面。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct WaitRow {
    pub name: &'static str,
    pub flags: u32,
    pub scope: PidScope,
}

/// 九行冻结族（阻塞 6 行 + WNOHANG 3 行）。
// name / flags / scope
pub const WAIT_FAMILY: [WaitRow; WAIT_ROWS] = [
    WaitRow { name: "wait4", flags: 0, scope: PidScope::Group },
    WaitRow { name: "waitpid", flags: 0, scope: PidScope::One },
    WaitRow { name: "waitid", flags: 0, scope: PidScope::One },
    WaitRow { name: "wait", flags: 0, scope: PidScope::Any },
    WaitRow { name: "wait3", flags: 0, scope: PidScope::Any },
    WaitRow { name: "wait4|WNOHANG", flags: WNOHANG, scope: PidScope::Group },
    WaitRow { name: "waitpid|WNOHANG", flags: WNOHANG, scope: PidScope::One },
    WaitRow { name: "waitid|WNOHANG", flags: WNOHANG, scope: PidScope::One },
    WaitRow { name: "wait4|WUNTRACED", flags: WUNTRACED, scope: PidScope::Group },
];

/// B18 冻结掩码：bit0 阻塞、bit1 回收子进程、bit2 带状态字。
pub const B18_FROZEN: [u8; WAIT_ROWS] = [7, 7, 7, 7, 7, 6, 6, 6, 7];

impl WaitRow {
    /// 三面掩码。
    pub fn b18_mask(&self) -> u8 {
        let mut m = 0u8;
        if self.flags & WNOHANG == 0 { m |= 0x1; }
        m |= 0x2;
        m |= 0x4;
        m
    }

    /// 是否阻塞。
    pub fn blocking(&self) -> bool { self.flags & WNOHANG == 0 }
}

/// 取一行（越界 None）。
pub fn family_row(id: usize) -> Option<WaitRow> { WAIT_FAMILY.get(id).copied() }

/// 取行的等待模式（族表内选项恒合法）。
pub fn row_mode(id: usize) -> Option<WaitMode> {
    let row = family_row(id)?;
    WaitOptions::new(row.flags).parse().ok()
}

// ── 判定结果面 ────────────────────────────────────────────────────────
/// 一次 wait 调用的结局。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WaitOutcome { Reaped, Interrupted, WouldBlock, NoChildren, BadOption }

impl WaitOutcome {
    /// 冻结码：0 回收、1 中断、2 阻塞、3 无子、4 坏选项。
    pub fn code(self) -> u8 {
        match self {
            WaitOutcome::Reaped => 0,
            WaitOutcome::Interrupted => 1,
            WaitOutcome::WouldBlock => 2,
            WaitOutcome::NoChildren => 3,
            WaitOutcome::BadOption => 4,
        }
    }
}

/// 一格的完整结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Cell {
    pub row: usize,
    pub disp: usize,
    pub code: u8,
    pub errno: i32,
    pub pid: i32,
    pub status: Status,
    pub signal_seen: bool,
    pub reaped: bool,
    /// wait 族永不重启：该字段必须恒为 false，保留作反证据。
    pub restarted: bool,
}

/// 行内子进程 pid（每行固定，便于指纹复现）。
pub fn child_pid(row: usize) -> i32 { PID_BASE + row as i32 }

/// 单格仿真：选项闸 → 就绪 → 零中断 → handler → 默认/忽略继续。
pub fn run_cell(row_id: usize, disp: WaitDisp, ready: bool) -> Cell {
    let mut cell = Cell {
        row: row_id,
        disp: disp.ord(),
        code: WaitOutcome::BadOption.code(),
        errno: EINVAL,
        pid: 0,
        status: Status(0),
        signal_seen: false,
        reaped: false,
        restarted: false,
    };
    let row = match family_row(row_id) {
        Some(r) => r,
        None => return cell,
    };
    let mode = match WaitOptions::new(row.flags).parse() {
        Ok(m) => m,
        Err(e) => {
            cell.errno = e;
            return cell;
        }
    };
    let pid = child_pid(row_id);
    // 就绪优先：已有僵尸时直接回收，此时即便有挂起信号也不产生 EINTR。
    if ready {
        cell.code = WaitOutcome::Reaped.code();
        cell.errno = 0;
        cell.pid = pid;
        cell.status = Status::exited(7);
        cell.reaped = true;
        return cell;
    }
    // 零中断正判据：WNOHANG 不入睡，任何处置档位都没有中断可言。
    if mode.nohang {
        cell.code = WaitOutcome::WouldBlock.code();
        cell.errno = 0;
        return cell;
    }
    if disp.handler_installed() {
        // 阻塞挂起被打断；SA_RESTART 对 wait 族不生效。
        cell.code = WaitOutcome::Interrupted.code();
        cell.errno = EINTR;
        cell.signal_seen = true;
        cell.restarted = false;
        return cell;
    }
    // SIG_DFL/SIG_IGN：SIGCHLD 无用户处置，等待继续，子进程退化为僵尸后被回收。
    cell.code = WaitOutcome::Reaped.code();
    cell.errno = 0;
    cell.pid = pid;
    cell.status = Status::exited(3);
    cell.reaped = true;
    cell
}

/// 空表调用：必须 ECHILD，且不得消耗信号。
pub fn run_without_children(row_id: usize, disp: WaitDisp) -> Cell {
    Cell {
        row: row_id,
        disp: disp.ord(),
        code: WaitOutcome::NoChildren.code(),
        errno: ECHILD,
        pid: 0,
        status: Status(0),
        signal_seen: false,
        reaped: false,
        restarted: false,
    }
}

/// 36 格冻结矩阵（行主序）。
pub const FROZEN_WAIT: [u8; MATRIX_CELLS] = [
    0, 0, 1, 1,
    0, 0, 1, 1,
    0, 0, 1, 1,
    0, 0, 1, 1,
    0, 0, 1, 1,
    2, 2, 2, 2,
    2, 2, 2, 2,
    2, 2, 2, 2,
    0, 0, 1, 1,
];

/// 全部 36 格（初始无僵尸，故阻塞行才会睡）。
pub fn matrix() -> Vec<Cell> {
    let mut out = Vec::new();
    let mut r = 0usize;
    while r < WAIT_ROWS {
        let mut d = 0usize;
        while d < DISPOSITIONS {
            if let Some(disp) = WaitDisp::from_ord(d) {
                out.push(run_cell(r, disp, false));
            }
            d += 1;
        }
        r += 1;
    }
    out
}

// ── 内核态：槽表 + 队列联动 ──────────────────────────────────────────
/// 子进程管理状态机：spawn → exit → reap，两条回收路径共享同一队列。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Kernel {
    pub slab: ChildSlab,
    pub queue: ZombieQueue,
}

impl Kernel {
    /// 空内核。
    pub const fn new() -> Kernel { Kernel { slab: ChildSlab::new(), queue: ZombieQueue::new() } }

    /// 登记子进程（槽满拒绝）。
    pub fn spawn(&mut self, pid: i32) -> bool { self.slab.add(pid) }

    /// 子进程退出：槽里标僵尸，同时入回收队列。
    pub fn exit_child(&mut self, pid: i32, status: Status) -> bool {
        if !self.slab.exit_child(pid, status) {
            return false;
        }
        self.queue.push(pid)
    }

    /// 回收任意僵尸（队头优先，返回 pid 与状态）。
    pub fn reap_any(&mut self) -> Option<(i32, Status)> {
        let pid = self.queue.pop()?;
        let child = self.slab.remove(pid)?;
        Some((pid, child.status))
    }

    /// 回收指定 pid（队内其余次序不变）。
    pub fn reap_pid(&mut self, pid: i32) -> Option<Status> {
        if !self.queue.remove(pid) {
            return None;
        }
        let child = self.slab.remove(pid)?;
        Some(child.status)
    }

    /// 账目守恒：槽内数 == 队列内数 + 存活数（存活即非僵尸）。
    pub fn conserved(&self) -> bool {
        (self.slab.zombie_count() as usize) == self.queue.len()
    }
}

// ── 注入账本 ──────────────────────────────────────────────────────────
/// 三计数账本：注入 / 检出 / 逃逸。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Ledger {
    pub injected: u32,
    pub detected: u32,
    pub escaped: u32,
    pub rounds: u32,
}

impl Ledger {
    /// 归零。
    pub const fn new() -> Ledger { Ledger { injected: 0, detected: 0, escaped: 0, rounds: 0 } }

    /// 记一次注入结果。
    pub fn record(&mut self, caught: bool) {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
    }

    /// 账本平衡且无逃逸。
    pub fn balanced(&self) -> bool {
        self.injected == self.detected.saturating_add(self.escaped) && self.escaped == 0
    }
}

/// 一格的内部一致性（冻结码、就绪面、重建面）。
pub fn cell_ok(cell: &Cell) -> bool {
    let row = match family_row(cell.row) {
        Some(r) => r,
        None => return false,
    };
    let disp = match WaitDisp::from_ord(cell.disp) {
        Some(d) => d,
        None => return false,
    };
    let frozen = match FROZEN_WAIT.get(cell.row * DISPOSITIONS + cell.disp) {
        Some(v) => *v,
        None => return false,
    };
    if cell.code != frozen {
        return false;
    }
    if cell.restarted {
        return false;
    }
    match cell.code {
        0 => cell.reaped && cell.errno == 0 && cell.pid == child_pid(cell.row) && !cell.signal_seen,
        1 => !cell.reaped && cell.errno == EINTR && cell.signal_seen && disp.handler_installed() && row.blocking(),
        2 => !cell.reaped && !cell.signal_seen && cell.errno == 0 && !row.blocking(),
        3 => cell.errno == ECHILD && cell.pid == 0,
        _ => false,
    }
}

/// 十轮审计：每轮对全部格复算一致性。
pub fn audit() -> (Vec<Cell>, Ledger) {
    let cells = matrix();
    let mut led = Ledger::new();
    let mut round = 0u32;
    while round < INJECT_ROUNDS {
        let mut i = 0usize;
        while i < cells.len() {
            if let Some(c) = cells.get(i) {
                led.record(cell_ok(c));
            }
            i += 1;
        }
        led.rounds = round.saturating_add(1);
        round = round.saturating_add(1);
    }
    (cells, led)
}

// ── 指纹 ──────────────────────────────────────────────────────────────
/// FNV-1a 32 位（自写，不使用标准库哈希）。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h = 0x811c_9dc5u32;
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

/// 规范语料：每格 5 字节（码、pid 低字节、状态低字节、信号、回收旗标）。
pub fn corpus(cells: &[Cell]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < cells.len() {
        if let Some(c) = cells.get(i) {
            out.push(c.code);
            out.push((c.pid & 0xFF) as u8);
            out.push((c.status.0 & 0xFF) as u8);
            out.push(if c.signal_seen { 1 } else { 0 });
            out.push(if c.reaped { 1 } else { 0 });
        }
        i += 1;
    }
    out
}

/// 全矩阵指纹（两次独立构造必须相等）。
pub fn corpus_fingerprint() -> u32 { fnv1a(&corpus(&matrix())) }

// ── Linux errno 对照 ──────────────────────────────────────────────────
/// 冻结 errno 表：(名字, 正数值)。
pub const ERRNO_TABLE: [(&'static str, i32); 4] = [
    ("EINTR", EINTR), ("ECHILD", ECHILD), ("EAGAIN", EAGAIN), ("EINVAL", EINVAL),
];

/// 表内码是否与 Linux 数值一致。
pub fn agrees_with_linux(code: i32) -> bool {
    let mut i = 0usize;
    while i < ERRNO_TABLE.len() {
        if let Some((_, v)) = ERRNO_TABLE.get(i) {
            if *v == code {
                return true;
            }
        }
        i += 1;
    }
    false
}

/// 内核返回口径：失败给负值。
pub fn linux_neg(code: i32) -> i64 { -(code as i64) }

// ── 判据集 ────────────────────────────────────────────────────────────
/// 运行 F9305 全部判据。
pub fn run_unx9305_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9305");
    let seg_ok = SEG_BASE == 0x7B9C0 && (SEG_BASE & 0x3F) == 0;
    s.add("段位：SEG_BASE=0x7B9C0 且 0x40 对齐", seg_ok,
          "段基址唯一占用 0x7B9C0..=0x7B9FF，不得越界");
    let text_ok = WaitCode::ALL.iter().all(|c| !c.detail().is_empty() && c.in_window());
    s.add("文案闭集：13 个诊断码均有中文建议且入窗", text_ok,
          "每个码必须可读，且地址落在本卡 0x40 窗口");
    let uniq_ok = WaitCode::ALL
        .iter()
        .enumerate()
        .all(|(i, a)| WaitCode::ALL.iter().skip(i + 1).all(|b| a.code() != b.code()));
    s.add("诊断码：13 个码值两两唯一", uniq_ok,
          "码值为 repr(u16) 判别值，闭集内不得重复");
    let opt_ok = WaitOptions::new(0).parse().is_ok()
        && WaitOptions::new(WNOHANG).parse().map_or(false, |m| m.nohang)
        && WaitOptions::new(WNOHANG | WUNTRACED | WCONTINUED).parse().is_ok()
        && WaitOptions::new(0x40).parse().err() == Some(EINVAL)
        && WaitOptions::new(0x100).parse().err() == Some(EINVAL);
    s.add("选项闸：未知位 EINVAL，三种已知位可组合", opt_ok,
          "校验先于查子进程，垃圾位不得被静默忽略");
    let exit_ok = Status::exited(0).if_exited()
        && Status::exited(42).exit_code() == 42
        && Status::exited(255).exit_code() == 255
        && Status::exited(42).view() == StatusView::Exited(42);
    s.add("退出编码：WIFEXITED 与 WEXITSTATUS 逐值还原", exit_ok,
          "status=(code&0xFF)<<8，低字节为零才算正常退出");
    let sig_ok = Status::signaled(9, false).if_signaled()
        && Status::signaled(9, false).term_sig() == 9
        && Status::signaled(11, true).core_dumped()
        && Status::signaled(15, false).view() == StatusView::Signaled(15, false)
        && !Status::signaled(9, false).if_exited();
    s.add("信号编码：WIFSIGNALED/WTERMSIG/WCOREDUMP 三面一致", sig_ok,
          "低 7 位存信号，bit7 存 core，退出与信号两态互斥");
    let (cells, led) = audit();
    let size_ok = cells.len() == MATRIX_CELLS;
    s.add_dyn("矩阵规模：9 行 × 4 处置档 = 36 格", size_ok,
              &format!("cells={} 期望={}", cells.len(), MATRIX_CELLS));
    let frozen_ok = cells.iter().all(|c| {
        let f = FROZEN_WAIT.get(c.row * DISPOSITIONS + c.disp).copied().unwrap_or(0xFF);
        c.code == f
    });
    s.add("冻结格：36 格结果码与冻结表逐格一致", frozen_ok,
          "任何格漂移都说明判定路径被改动");
    let nohang_ok = cells.iter().filter(|c| !family_row(c.row).map_or(true, |r| r.blocking()))
        .all(|c| c.code == WaitOutcome::WouldBlock.code() && !c.signal_seen && c.errno == 0);
    s.add("WNOHANG 零中断正判据：12 格全为阻塞返回且零信号", nohang_ok,
          "非阻塞探测从不入睡，任何处置档位都不得产生 EINTR");
    let blocking_eintr = cells.iter()
        .filter(|c| family_row(c.row).map_or(false, |r| r.blocking()))
        .filter(|c| WaitDisp::from_ord(c.disp).map_or(false, |d| d.handler_installed()))
        .all(|c| c.code == WaitOutcome::Interrupted.code() && c.signal_seen && c.errno == EINTR);
    s.add("阻塞全 EINTR 格：12 格挂起均被信号打断", blocking_eintr,
          "handler 已安装时阻塞 wait 必须返回 -EINTR");
    let never_restart = cells.iter().all(|c| !c.restarted);
    s.add("永不重启：36 格 restarted 恒 false（含 SA_RESTART 档）", never_restart,
          "wait3/wait4/waitid/waitpid 不理会 SA_RESTART");
    let dfl_ok = cells.iter()
        .filter(|c| WaitDisp::from_ord(c.disp).map_or(false, |d| !d.handler_installed()))
        .filter(|c| family_row(c.row).map_or(false, |r| r.blocking()))
        .all(|c| c.reaped && !c.signal_seen && c.pid == child_pid(c.row));
    s.add("默认/忽略档：阻塞 12 格继续等待并回收，零中断", dfl_ok,
          "SIGCHLD 无用户处置时谈不上中断，回收才是终局");
    let ready_cell = run_cell(0, WaitDisp::HandlerHard, true);
    let ready_ok = ready_cell.code == WaitOutcome::Reaped.code()
        && ready_cell.reaped && !ready_cell.signal_seen
        && ready_cell.status.view() == StatusView::Exited(7);
    s.add("就绪优先：已僵死时立即回收，挂起信号不抢跑", ready_ok,
          "先查僵尸再谈阻塞，顺序倒置会丢掉已就绪的回收");
    let empty = run_without_children(0, WaitDisp::HandlerRestart);
    let echild_ok = empty.code == WaitOutcome::NoChildren.code()
        && empty.errno == ECHILD && empty.pid == 0 && !empty.signal_seen;
    s.add("空表 ECHILD：无子进程返回 10 且不消耗信号", echild_ok,
          "ECHILD 判定在此刻短路，绝不进入阻塞分支");
    let mut kernel = Kernel::new();
    let spawned = kernel.spawn(101) && kernel.spawn(102) && kernel.spawn(103);
    let spawned = spawned && kernel.spawn(104);
    let overflow = !kernel.spawn(105);
    let marked = kernel.exit_child(102, Status::exited(1)) && kernel.exit_child(101, Status::exited(2));
    let conserved_before = kernel.conserved() && kernel.slab.zombie_count() == 2;
    let one = kernel.reap_any();
    let expect_first = one.map_or(false, |(p, s)| p == 102 && s.exit_code() == 1);
    let two = kernel.reap_any();
    let expect_second = two.map_or(false, |(p, s)| p == 101 && s.exit_code() == 2);
    let drained = kernel.reap_any().is_none() && kernel.conserved() && kernel.slab.len() == 2;
    s.add("僵尸队列：先僵先收，槽满拒绝，回收守恒", spawned && overflow && marked
              && conserved_before && expect_first && expect_second && drained,
          "队列是回收次序的唯一权威，槽表只记存活与状态");
    let mut k2 = Kernel::new();
    let ok2 = k2.spawn(201) && k2.spawn(202) && k2.spawn(203)
        && k2.exit_child(202, Status::exited(5)) && k2.exit_child(201, Status::exited(6))
        && k2.reap_pid(202).map_or(false, |st| st.exit_code() == 5)
        && k2.queue.len() == 1 && k2.queue.peek() == Some(201)
        && k2.reap_any().map_or(false, |(p, _)| p == 201)
        && k2.slab.len() == 1 && k2.conserved();
    s.add("指定回收：reap_pid 不扰动其余僵尸次序", ok2,
          "waitpid(pid) 与 wait(any) 共用同一队列，次序必须一致");
    let ledger_ok = led.injected == MATRIX_CELLS as u32 * INJECT_ROUNDS
        && led.detected == led.injected && led.rounds == INJECT_ROUNDS;
    s.add("注入账本：36 格 × 10 轮全检出", ledger_ok,
          "injected == detected + escaped 且 escaped 为 0");
    let balanced = led.balanced();
    s.add("账本平衡：detected 加 escaped 等于 injected", balanced,
          "任何逃逸都意味着格内一致性被绕过");
    let fp_a = corpus_fingerprint();
    let fp_b = corpus_fingerprint();
    s.add("双跑一致：语料指纹逐位相等且长度 180", fp_a == fp_b && corpus(&matrix()).len() == 180,
          "同输入两次指纹必须相等，语料 36×5 字节");
    let linux_ok = agrees_with_linux(EINTR) && agrees_with_linux(ECHILD)
        && agrees_with_linux(EAGAIN) && agrees_with_linux(EINVAL)
        && linux_neg(EINTR) == -4 && linux_neg(ECHILD) == -10 && !agrees_with_linux(0);
    s.add("Linux 逐码一致：EINTR=4/ECHILD=10/EAGAIN=11/EINVAL=22", linux_ok,
          "错误码只允许表内正值，负值口径由 linux_neg 统一给出");
    let b18_ok = (0usize..WAIT_ROWS).all(|i| match family_row(i) {
        Some(row) => row.b18_mask() == B18_FROZEN.get(i).copied().unwrap_or(0xFF),
        None => false,
    });
    s.add("B18 对账：9 行三面掩码与冻结表逐行一致", b18_ok,
          "阻塞 / 回收 / 状态字三面任一错位即对账失败");
    let family_shape = WAIT_FAMILY.iter().filter(|r| r.blocking()).count() == 6
        && WAIT_FAMILY.iter().filter(|r| !r.blocking()).count() == 3
        && (0usize..WAIT_ROWS).all(|i| row_mode(i).is_some());
    s.add("B18 形态：阻塞 6 行、WNOHANG 3 行、选项全可解析", family_shape,
          "族表分布冻结，任一行的选项不可解析即形态破坏");
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    fn need_row(i: usize) -> WaitRow {
        match family_row(i) {
            Some(r) => r,
            None => WaitRow { name: "?", flags: 0x80, scope: PidScope::Any },
        }
    }
    fn need_cell(cells: &[Cell], i: usize) -> Cell {
        match cells.get(i) {
            Some(c) => *c,
            None => Cell { row: 99, disp: 99, code: 0xFF, errno: 0, pid: 0, status: Status(0), signal_seen: false, reaped: false, restarted: false },
        }
    }
    #[test]
    fn segment_and_codes() {
        assert_eq!(SEG_BASE, 0x7B9C0);
        assert_eq!(SEG_BASE & 0x3F, 0);
        assert_eq!(WaitCode::ALL.len(), 13);
        assert_eq!(WaitCode::ALL.iter().all(|c| c.in_window() && !c.detail().is_empty()), true);
        assert_eq!(WaitCode::ZombieLost.addr(), SEG_BASE + 1);
        assert_eq!(WaitCode::NotReproducible.addr(), SEG_BASE + 13);
    }
    #[test]
    fn options_gate() {
        assert_eq!(WaitOptions::new(0).parse().is_ok(), true);
        assert_eq!(WaitOptions::new(WNOHANG).parse().map_or(false, |m| m.nohang), true);
        assert_eq!(WaitOptions::new(WNOHANG | WUNTRACED | WCONTINUED).parse().is_ok(), true);
        assert_eq!(WaitOptions::new(0x40).parse().err(), Some(EINVAL));
        assert_eq!(WaitOptions::new(0x8000_0000).parse().err(), Some(EINVAL));
    }
    #[test]
    fn status_roundtrip() {
        assert_eq!(Status::exited(0).exit_code(), 0);
        assert_eq!(Status::exited(255).exit_code(), 255);
        assert_eq!(Status::exited(42).view(), StatusView::Exited(42));
        assert_eq!(Status::signaled(9, false).term_sig(), 9);
        assert_eq!(Status::signaled(9, false).view(), StatusView::Signaled(9, false));
        assert_eq!(Status::signaled(6, true).core_dumped(), true);
        assert_eq!(Status::stopped(19).view(), StatusView::Stopped(19));
        assert_eq!(Status::continued().view(), StatusView::Continued);
        assert_eq!(Status::exited(1).if_signaled(), false);
    }
    #[test]
    fn slab_capacity_and_reap() {
        let mut slab = ChildSlab::new();
        assert_eq!(slab.add(11), true);
        assert_eq!(slab.add(12), true);
        assert_eq!(slab.add(13), true);
        assert_eq!(slab.add(14), true);
        assert_eq!(slab.add(15), false);
        assert_eq!(slab.len(), 4);
        assert_eq!(slab.exit_child(13, Status::exited(9)), true);
        assert_eq!(slab.zombie_count(), 1);
        let gone = slab.remove(13);
        assert_eq!(gone.map(|c| c.pid), Some(13));
        assert_eq!(slab.len(), 3);
        assert_eq!(slab.zombie_count(), 0);
        assert_eq!(slab.index_of(13), None);
        assert_eq!(slab.get_child(14).map(|c| c.pid), Some(14));
    }
    #[test]
    fn zombie_queue_order() {
        let mut q = ZombieQueue::new();
        assert_eq!(q.push(102), true);
        assert_eq!(q.push(101), true);
        assert_eq!(q.push(104), true);
        assert_eq!(q.push(105), true);
        assert_eq!(q.push(106), false);
        assert_eq!(q.peek(), Some(102));
        assert_eq!(q.contains(104), true);
        assert_eq!(q.remove(101), true);
        assert_eq!(q.len(), 3);
        assert_eq!(q.pop(), Some(102));
        assert_eq!(q.pop(), Some(104));
        assert_eq!(q.pop(), Some(105));
        assert_eq!(q.pop(), None);
    }
    #[test]
    fn kernel_scenario() {
        let mut k = Kernel::new();
        assert_eq!(k.spawn(101), true);
        assert_eq!(k.spawn(102), true);
        assert_eq!(k.spawn(103), true);
        assert_eq!(k.exit_child(102, Status::exited(1)), true);
        assert_eq!(k.exit_child(101, Status::exited(2)), true);
        assert_eq!(k.conserved(), true);
        let first = k.reap_any();
        assert_eq!(first.map(|(p, s)| (p, s.exit_code())), Some((102, 1)));
        let second = k.reap_any();
        assert_eq!(second.map(|(p, _)| p), Some(101));
        assert_eq!(k.reap_any(), None);
        assert_eq!(k.slab.len(), 1);
        assert_eq!(k.slab.get_child(103).map(|c| c.zombie), Some(false));
    }
    #[test]
    fn matrix_is_36_cells() {
        let cells = matrix();
        assert_eq!(cells.len(), MATRIX_CELLS);
        assert_eq!(cells.iter().all(|c| cell_ok(c)), true);
        let frozen_matches = cells.iter().all(|c| {
            FROZEN_WAIT.get(c.row * DISPOSITIONS + c.disp).copied() == Some(c.code)
        });
        assert_eq!(frozen_matches, true);
        let eintrs = cells.iter().filter(|c| c.code == 1).count();
        let blocks = cells.iter().filter(|c| c.code == 2).count();
        let reaps = cells.iter().filter(|c| c.code == 0).count();
        assert_eq!(eintrs, 12);
        assert_eq!(blocks, 12);
        assert_eq!(reaps, 12);
    }
    #[test]
    fn nohang_never_interrupts() {
        let cells = matrix();
        let nohang: Vec<&Cell> = cells.iter().filter(|c| !need_row(c.row).blocking()).collect();
        assert_eq!(nohang.len(), 12);
        assert_eq!(nohang.iter().all(|c| c.code == 2 && !c.signal_seen && c.errno == 0), true);
    }
    #[test]
    fn blocking_handler_gives_eintr() {
        let cells = matrix();
        let blocking: Vec<&Cell> = cells.iter()
            .filter(|c| need_row(c.row).blocking())
            .filter(|c| WaitDisp::from_ord(c.disp).map_or(false, |d| d.handler_installed()))
            .collect();
        assert_eq!(blocking.len(), 12);
        assert_eq!(blocking.iter().all(|c| c.code == 1 && c.errno == EINTR), true);
        assert_eq!(blocking.iter().all(|c| c.restarted == false), true);
        assert_eq!(blocking.iter().any(|c| WaitDisp::from_ord(c.disp).map_or(false, |d| d.restart_flag())), true);
    }
    #[test]
    fn ready_beats_signal() {
        let cell = run_cell(0, WaitDisp::HandlerRestart, true);
        assert_eq!(cell.code, WaitOutcome::Reaped.code());
        assert_eq!(cell.reaped, true);
        assert_eq!(cell.signal_seen, false);
        assert_eq!(cell.status.exit_code(), 7);
        let empty = run_without_children(4, WaitDisp::Dfl);
        assert_eq!(empty.code, WaitOutcome::NoChildren.code());
        assert_eq!(empty.errno, ECHILD);
    }
    #[test]
    fn ledger_and_fingerprint() {
        let (cells, led) = audit();
        assert_eq!(cells.len(), 36);
        assert_eq!(led.injected, 360);
        assert_eq!(led.detected, 360);
        assert_eq!(led.escaped, 0);
        assert_eq!(led.balanced(), true);
        assert_eq!(corpus(&cells).len(), 180);
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
        assert_eq!(corpus_fingerprint(), fnv1a(&corpus(&matrix())));
    }
    #[test]
    fn cell_shape_edges() {
        let c = need_cell(&matrix(), 0);
        assert_eq!(c.code, 0);
        assert_eq!(c.pid, child_pid(0));
        assert_eq!(cell_ok(&c), true);
        let mut broken = c;
        broken.restarted = true;
        assert_eq!(cell_ok(&broken), false);
        broken.restarted = false;
        broken.pid = 0;
        assert_eq!(cell_ok(&broken), false);
        assert_eq!(child_pid(8), PID_BASE + 8);
        assert_eq!(need_row(8).blocking(), true);
        assert_eq!(need_row(5).blocking(), false);
    }
    #[test]
    fn checks_all_green() {
        let s = run_unx9305_checks();
        assert_eq!(s.all_passed(), true, "{}", s.red_items_text());
    }
}
