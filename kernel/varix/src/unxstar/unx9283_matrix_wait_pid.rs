//! UNX-F9283 wait 坏 pid ECHILD 矩阵（联 B18 对账） · 0x7B440段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 F9283 逐字）：
//!   ① 纯功能代码 ≥320 行；② 判据原文：UNX-F9283-J1 无子进程注入判据过，
//!   全 ECHILD 格过，与 B18 wait 矩阵零重复分层格过；③ 完成时 API/软件写回：
//!   状态=已完成、结果摘要非空。
//!
//! 设计要点：
//!   ① wait 的坏 pid 只回 ECHILD，永不回 ESRCH：waitpid 是「相对调用者的子进程
//!      集合」查询，pid 在系统里存在与否无关紧要，只要不是我子进程就是 ECHILD。
//!      模块把这条差异做成一条独立判据（全表扫描断言无 ESRCH）。
//!   ② 选择器译码与匹配分离：`Selector` 四形态（Exact/OwnGroup/AnyChild/Group）
//!      由 `decode` 一次性译出，`pick` 只在「ppid == 调用者且未回收」的子集里匹
//!      配。两条路径分开后，「不存在 pid」「别人的子进程」「自己的父进程」都自然
//!      落进同一个 ECHILD 出口，而不会各自写一条特判。
//!   ③ 子进程状态机 Running→Zombie→Reaped 单向；Stopped 是旁支（WUNTRACED 报
//!      告但不回收）。回收是唯一消耗状态的操作，二次 wait 必然 ECHILD。
//!   ④ 选项面先于选择器面：非法位（超出 WNOHANG|WUNTRACED|WCONTINUED|
//!      __WNOTHREAD|__WALL|__WCLONE）回 EINVAL。EINVAL 与 ECHILD 不可互换，
//!      否则调用方永远修不好坏 options。
//!   ⑤ 与 B18 分层：B18 拥有「状态字编码/WNOHANG 轮询/WUNTRACED 报告/rusage/
//!      pid 回显」八格；本模块拥有「坏 pid 选择器 → ECHILD」八格。两张 lane 表
//!      逐名比对必须零交集（`reconcile_b18`），自己矩阵每格必须落在 OWN 表内。
//!   ⑥ 注入账：10 格 × 10 轮 = 100 次全检出，escaped 恒 0；指纹双跑相等。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 诊断段基址（F9283 独占 0x7B440..=0x7B47F）。
pub const SEG_BASE: u32 = 0x7B440;
/// 本批段窗口下界。
pub const WINDOW_LO: u32 = 0x7B400;
/// 本批段窗口上界。
pub const WINDOW_HI: u32 = 0x7D1C0;

/// 子进程表容量。
pub const MAX_CHILDREN: usize = 16;
/// 单格注入轮数。
pub const INJECT_ROUNDS: u32 = 10;
/// ECHILD 格数。
pub const ECHILD_CELLS: usize = 10;
/// 本模块 lane 格数。
pub const OWN_CELLS: usize = 8;
/// B18 lane 格数。
pub const B18_CELLS: usize = 8;

/// WNOHANG：不阻塞，无就绪子进程回 0。
pub const WNOHANG: u32 = 0x0000_0001;
/// WUNTRACED：报告停止的子进程。
pub const WUNTRACED: u32 = 0x0000_0002;
/// WCONTINUED：报告继续的子进程。
pub const WCONTINUED: u32 = 0x0000_0008;
/// __WNOTHREAD：只看本线程的子进程。
pub const WNOTHREAD: u32 = 0x2000_0000;
/// __WALL：包含 clone 子进程。
pub const WALL: u32 = 0x4000_0000;
/// __WCLONE：只看 clone 子进程。
pub const WCLONE: u32 = 0x8000_0000;
/// 合法选项位全集。
pub const OPT_MASK: u32 = WNOHANG | WUNTRACED | WCONTINUED | WNOTHREAD | WALL | WCLONE;

/// ECHILD：没有匹配的子进程。
pub const ECHILD: i32 = 10;
/// EINVAL：options 含非法位。
pub const EINVAL: i32 = 22;
/// ESRCH：本模块任何路径都不得返回（差异判据）。
pub const ESRCH: i32 = 3;
/// EINTR：阻塞等待被信号打断。
pub const EINTR: i32 = 4;

/// B18 的状态字右移量（与内核 WEXITSTATUS 一致）。
pub const B18_STATUS_SHIFT: u32 = 8;
/// B18 的退出码掩码（与内核一致）。
pub const B18_EXIT_MASK: u32 = 0x7F;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..=+0x0C 闭集）。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WaitCode {
    NoChildAtAll,
    NotMyChild,
    AlreadyReaped,
    EmptyGroup,
    UndecodableSelector,
    BadOptions,
    WrongErrno,
    BlockingInterrupted,
    LaneOverlap,
    Escaped,
    NotReproducible,
    SegmentViolation,
}

impl WaitCode {
    /// 闭集下标。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 段内码值。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self.code() as u32
    }

    /// 中文诊断与可操作建议。
    pub fn detail(self) -> &'static str {
        match self {
            WaitCode::NoChildAtAll => "无子进程（ECHILD）：调用者名下没有任何未回收子进程；请先确认进程确实是父进程，或改 wait 别的 pid",
            WaitCode::NotMyChild => "不是我的子进程（ECHILD）：waitpid 只能回收自己的子进程；请核对 ppid，不要拿父进程/兄弟/孙进程的 pid 去 wait",
            WaitCode::AlreadyReaped => "重复回收（ECHILD）：该子进程已被 wait 走，状态是 Reaped；请勿对同一 pid 二次 wait",
            WaitCode::EmptyGroup => "组选择器为空（ECHILD）：pid=0/-pgid 没有匹配到本组子进程；请确认子进程是否换了进程组",
            WaitCode::UndecodableSelector => "选择器不可译（ECHILD）：pid=INT_MIN 的取负溢出；请改用合法的正 pid 或 -1",
            WaitCode::BadOptions => "选项非法（EINVAL）：options 含未定义位；请只保留 WNOHANG/WUNTRACED/WCONTINUED/__WNOTHREAD/__WALL/__WCLONE",
            WaitCode::WrongErrno => "错误码不符：坏 pid 路径只许 ECHILD，不许 ESRCH/EINVAL；请对照冻结矩阵复算",
            WaitCode::BlockingInterrupted => "阻塞被打断（EINTR）：无 WNOHANG 且收到信号；请按 EINTR 重试，不要当成无子进程",
            WaitCode::LaneOverlap => "分层重叠：本模块与 B18 的判定格出现同名项；请把状态字/选项语义归 B18，坏 pid 归这里",
            WaitCode::Escaped => "注入逃逸：ECHILD 格未被检出；请回到 pick 确认「非子进程」分支可达",
            WaitCode::NotReproducible => "双跑不一致：同输入两次结果不同；请排查隐藏状态或非确定哈希",
            WaitCode::SegmentViolation => "段位越界：本模块段基址必须落在 0x7B400..=0x7D1C0 且 0x40 对齐",
        }
    }

    /// 闭集全量。
    pub const ALL: &'static [WaitCode] = &[
        WaitCode::NoChildAtAll,
        WaitCode::NotMyChild,
        WaitCode::AlreadyReaped,
        WaitCode::EmptyGroup,
        WaitCode::UndecodableSelector,
        WaitCode::BadOptions,
        WaitCode::WrongErrno,
        WaitCode::BlockingInterrupted,
        WaitCode::LaneOverlap,
        WaitCode::Escaped,
        WaitCode::NotReproducible,
        WaitCode::SegmentViolation,
    ];
}

// ── 子进程状态机 ──────────────────────────────────────────────────────
/// 子进程状态：Running / Stopped / Zombie / Reaped 四档。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChildState {
    Running,
    Stopped,
    Zombie,
    Reaped,
}

impl ChildState {
    /// 是否已经是终态（不可再被 wait 命中）。
    pub fn terminal(self) -> bool {
        matches!(self, ChildState::Reaped)
    }

    /// 是否可被 wait 命中（回收后不可）。
    pub fn waitable(self) -> bool {
        !self.terminal()
    }

    /// 状态转移是否合法：Running→Zombie/Stopped，Stopped→Running/Zombie，
    /// Zombie→Reaped。其余（含自反）一律非法。
    pub fn can_step_to(self, next: ChildState) -> bool {
        match self {
            ChildState::Running => matches!(next, ChildState::Zombie | ChildState::Stopped),
            ChildState::Stopped => matches!(next, ChildState::Running | ChildState::Zombie),
            ChildState::Zombie => matches!(next, ChildState::Reaped),
            ChildState::Reaped => false,
        }
    }
}

/// 一个子进程槽。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Child {
    pub used: bool,
    pub pid: i32,
    pub ppid: i32,
    pub pgid: i32,
    pub state: ChildState,
    pub code: i32,
    pub gen: u32,
}

impl Child {
    /// 定容数组占位。
    pub const EMPTY: Child = Child {
        used: false,
        pid: 0,
        ppid: 0,
        pgid: 0,
        state: ChildState::Reaped,
        code: 0,
        gen: 0,
    };
}

/// 等待者上下文：pid 与 pgid 都参与选择器匹配。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Waiter {
    pub pid: i32,
    pub pgid: i32,
}

/// 选择器：pid 参数译码后的四形态（外加不可译一档）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Selector {
    /// pid > 0：精确匹配这个子进程。
    Exact(i32),
    /// pid == 0：本进程组内的任意子进程。
    OwnGroup,
    /// pid == -1：任意子进程。
    AnyChild,
    /// pid < -1：进程组 -pid 内的任意子进程。
    Group(i32),
    /// pid == i32::MIN：取负溢出，不可译。
    Undecodable,
}

/// 选择器译码（纯函数，覆盖 i32 全域）。
pub fn decode_selector(pid: i32) -> Selector {
    if pid > 0 {
        Selector::Exact(pid)
    } else if pid == 0 {
        Selector::OwnGroup
    } else if pid == -1 {
        Selector::AnyChild
    } else {
        match pid.checked_neg() {
            Some(g) => Selector::Group(g),
            None => Selector::Undecodable,
        }
    }
}

/// wait 的返回样本：已回收 / 已停止 / 仍需等待。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WaitSample {
    /// 回收了一个僵尸子进程。
    Reaped { pid: i32, status: u32 },
    /// 报告一个停止的子进程（WUNTRACED，不回收）。
    Stopped { pid: i32, sig: i32 },
    /// 无就绪项（WNOHANG）或仍在阻塞。
    Pending,
}

/// 退出状态编码（B18 owns 该编码，本模块只在返回值里回显）。
pub fn exited_status(code: i32) -> u32 {
    ((code as u32) & 0xFF) << B18_STATUS_SHIFT
}

/// 从状态字取退出码（B18 口径复算）。
pub fn exit_code_of(status: u32) -> u32 {
    (status >> B18_STATUS_SHIFT) & B18_EXIT_MASK
}

// ── 子进程表 ──────────────────────────────────────────────────────────
/// 定容子进程表 + 生育/回收两本计数。
#[derive(Clone, Copy, Debug)]
pub struct ChildTable {
    kids: [Child; MAX_CHILDREN],
    len: u32,
    births: u32,
    reaps: u32,
    next_gen: u32,
}

impl ChildTable {
    /// 空表。
    pub const fn new() -> ChildTable {
        ChildTable {
            kids: [Child::EMPTY; MAX_CHILDREN],
            len: 0,
            births: 0,
            reaps: 0,
            next_gen: 1,
        }
    }

    /// 在册子进程数（含僵尸，不含已回收）。
    pub fn len(&self) -> u32 {
        self.len
    }

    /// 累计回收数。
    pub fn reaps(&self) -> u32 {
        self.reaps
    }

    /// 累计出生数。
    pub fn births(&self) -> u32 {
        self.births
    }

    /// 槽视图。
    pub fn slot(&self, i: usize) -> Option<&Child> {
        self.kids.get(i)
    }

    /// 发布一个子进程（初始 Running）。满表回 None。
    pub fn spawn(&mut self, pid: i32, ppid: i32, pgid: i32) -> Option<usize> {
        let mut i = 0usize;
        while i < MAX_CHILDREN {
            let free = match self.kids.get(i) {
                Some(k) => !k.used,
                None => false,
            };
            if free {
                let gen = self.next_gen;
                self.next_gen = self.next_gen.saturating_add(1);
                if let Some(slot) = self.kids.get_mut(i) {
                    slot.used = true;
                    slot.pid = pid;
                    slot.ppid = ppid;
                    slot.pgid = pgid;
                    slot.state = ChildState::Running;
                    slot.code = 0;
                    slot.gen = gen;
                }
                self.len = self.len.saturating_add(1);
                self.births = self.births.saturating_add(1);
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 按 pid 查未回收的子进程槽。
    pub fn find(&self, pid: i32) -> Option<usize> {
        let mut i = 0usize;
        while i < MAX_CHILDREN {
            if let Some(k) = self.kids.get(i) {
                if k.used && k.state.waitable() && k.pid == pid {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 取副本（避免调用方长时间持借用）。
    pub fn peek(&self, pid: i32) -> Option<Child> {
        match self.find(pid) {
            Some(i) => self.kids.get(i).copied(),
            None => None,
        }
    }

    /// 调用者名下是否还有未回收子进程（「无子进程」短路）。
    pub fn has_child(&self, ppid: i32) -> bool {
        let mut i = 0usize;
        while i < MAX_CHILDREN {
            if let Some(k) = self.kids.get(i) {
                if k.used && k.state.waitable() && k.ppid == ppid {
                    return true;
                }
            }
            i += 1;
        }
        false
    }

    /// 调用者名下未回收子进程数。
    pub fn child_count(&self, ppid: i32) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < MAX_CHILDREN {
            if let Some(k) = self.kids.get(i) {
                if k.used && k.state.waitable() && k.ppid == ppid {
                    n = n.saturating_add(1);
                }
            }
            i += 1;
        }
        n
    }

    /// 子进程退出：Running/Stopped → Zombie。返回转移是否合法。
    pub fn exit(&mut self, pid: i32, code: i32) -> bool {
        let idx = match self.find(pid) {
            Some(i) => i,
            None => return false,
        };
        let prev = match self.kids.get(idx) {
            Some(k) => k.state,
            None => return false,
        };
        if !prev.can_step_to(ChildState::Zombie) {
            return false;
        }
        if let Some(k) = self.kids.get_mut(idx) {
            k.state = ChildState::Zombie;
            k.code = code;
        }
        true
    }

    /// 子进程停止：Running → Stopped。
    pub fn stop(&mut self, pid: i32) -> bool {
        let idx = match self.find(pid) {
            Some(i) => i,
            None => return false,
        };
        let prev = match self.kids.get(idx) {
            Some(k) => k.state,
            None => return false,
        };
        if !prev.can_step_to(ChildState::Stopped) {
            return false;
        }
        if let Some(k) = self.kids.get_mut(idx) {
            k.state = ChildState::Stopped;
        }
        true
    }

    /// 选择器匹配：只在「ppid == 调用者且未回收」的子集里找，顺序即发布序。
    pub fn pick(&self, caller: &Waiter, sel: Selector) -> Option<usize> {
        let mut i = 0usize;
        while i < MAX_CHILDREN {
            if let Some(k) = self.kids.get(i) {
                if k.used && k.state.waitable() && k.ppid == caller.pid {
                    let hit = match sel {
                        Selector::Exact(p) => k.pid == p,
                        Selector::OwnGroup => k.pgid == caller.pgid,
                        Selector::AnyChild => true,
                        Selector::Group(g) => k.pgid == g,
                        Selector::Undecodable => false,
                    };
                    if hit {
                        return Some(i);
                    }
                }
            }
            i += 1;
        }
        None
    }

    /// wait4 判定（纯逻辑 + 唯一的回收副作用）。
    ///
    /// 次序（冻结）：options 面 EINVAL → 无子进程 ECHILD → 选不中 ECHILD →
    /// 就绪回收 / 停止报告 / Pending（或 EINTR）。
    pub fn wait4(
        &mut self,
        caller: &Waiter,
        pid: i32,
        options: u32,
        interrupted: bool,
    ) -> Result<WaitSample, i32> {
        if options & !OPT_MASK != 0 {
            return Err(EINVAL);
        }
        if !self.has_child(caller.pid) {
            return Err(ECHILD);
        }
        let sel = decode_selector(pid);
        let idx = match self.pick(caller, sel) {
            Some(i) => i,
            None => return Err(ECHILD),
        };
        let (kpid, kstate, kcode) = match self.kids.get(idx) {
            Some(k) => (k.pid, k.state, k.code),
            None => return Err(ECHILD),
        };
        match kstate {
            ChildState::Zombie => {
                if kstate.can_step_to(ChildState::Reaped) {
                    if let Some(k) = self.kids.get_mut(idx) {
                        k.state = ChildState::Reaped;
                        k.used = false;
                    }
                    self.len = self.len.saturating_sub(1);
                    self.reaps = self.reaps.saturating_add(1);
                    return Ok(WaitSample::Reaped {
                        pid: kpid,
                        status: exited_status(kcode),
                    });
                }
                Err(ECHILD)
            }
            ChildState::Stopped => {
                if options & WUNTRACED != 0 {
                    Ok(WaitSample::Stopped { pid: kpid, sig: 19 })
                } else if options & WNOHANG != 0 {
                    Ok(WaitSample::Pending)
                } else if interrupted {
                    Err(EINTR)
                } else {
                    Ok(WaitSample::Pending)
                }
            }
            ChildState::Running => {
                if options & WNOHANG != 0 {
                    Ok(WaitSample::Pending)
                } else if interrupted {
                    Err(EINTR)
                } else {
                    Ok(WaitSample::Pending)
                }
            }
            ChildState::Reaped => Err(ECHILD),
        }
    }
}

// ── 语料 ──────────────────────────────────────────────────────────────
/// 主语料调用者。
pub const CALLER: Waiter = Waiter {
    pid: 500,
    pgid: 500,
};

/// 主语料：7 个子进程 + 1 个孙 + 1 个父 + 1 个他人子。
///
/// 槽位规划（发布序即匹配序）：
///   600(500,pg500,Running) / 601(500,pg500,Zombie) / 602(500,pg700,Running)
///   603(500,pg500,Stopped) / 604(500,pg500,Reaped) / 700(999,pg500,Running)
///   501(400,pg400,Running) / 800(600,pg500,Running)
pub fn main_table() -> ChildTable {
    let mut t = ChildTable::new();
    let mut ok = true;
    if t.spawn(600, 500, 500).is_none() {
        ok = false;
    }
    if t.spawn(601, 500, 500).is_none() {
        ok = false;
    }
    if t.spawn(602, 500, 700).is_none() {
        ok = false;
    }
    if t.spawn(603, 500, 500).is_none() {
        ok = false;
    }
    let reaped_slot = t.spawn(604, 500, 500);
    if t.spawn(700, 999, 500).is_none() {
        ok = false;
    }
    if t.spawn(501, 400, 400).is_none() {
        ok = false;
    }
    if t.spawn(800, 600, 500).is_none() {
        ok = false;
    }
    if !ok {
        return ChildTable::new();
    }
    // 601 置僵尸，603 置停止，604 直接置已回收。
    if t.exit(601, 7) {
        // 预期路径：Running → Zombie。
    } else {
        return ChildTable::new();
    }
    if t.stop(603) {
        // 预期路径：Running → Stopped。
    } else {
        return ChildTable::new();
    }
    if let Some(idx) = reaped_slot {
        if let Some(k) = t.kids.get_mut(idx) {
            k.state = ChildState::Reaped;
            k.used = false;
        }
        t.len = t.len.saturating_sub(1);
        t.reaps = t.reaps.saturating_add(1);
    }
    t
}

/// 空语料：调用者 510 名下无任何子进程（只有一个别人家的孩子）。
pub fn bare_table() -> ChildTable {
    let mut t = ChildTable::new();
    let mut ok = true;
    if t.spawn(611, 700, 510).is_none() {
        ok = false;
    }
    if t.spawn(612, 700, 511).is_none() {
        ok = false;
    }
    if !ok {
        return ChildTable::new();
    }
    t
}

// ── B18 分层对账 ──────────────────────────────────────────────────────
/// B18 拥有的判定格（状态字/轮询/停止报告/rusage/pid 回显）。
pub const B18_LANES: [&str; B18_CELLS] = [
    "b18.status.wifexited",
    "b18.status.wifsignaled",
    "b18.status.exitcode",
    "b18.wnohang.poll",
    "b18.wuntraced.stop",
    "b18.wcontinued.cont",
    "b18.rusage.out",
    "b18.pid.echo",
];

/// 本模块拥有的判定格（坏 pid 选择器 → ECHILD）。
pub const OWN_LANES: [&str; OWN_CELLS] = [
    "f9283.no.children",
    "f9283.exact.absent",
    "f9283.exact.not.child",
    "f9283.exact.self",
    "f9283.exact.parent",
    "f9283.exact.grandchild",
    "f9283.exact.reaped",
    "f9283.group.empty",
];

/// 分层对账结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Reconcile {
    pub own: u32,
    pub b18: u32,
    pub overlap: u32,
    pub status_roundtrip: bool,
}

/// 两张 lane 表逐名比对：同名即重叠（必须为 0）。
pub fn reconcile_b18() -> Reconcile {
    let mut overlap = 0u32;
    let mut i = 0usize;
    while i < OWN_LANES.len() {
        let mut j = 0usize;
        while j < B18_LANES.len() {
            if let (Some(a), Some(b)) = (OWN_LANES.get(i), B18_LANES.get(j)) {
                if *a == *b {
                    overlap = overlap.saturating_add(1);
                }
            }
            j += 1;
        }
        i += 1;
    }
    Reconcile {
        own: OWN_LANES.len() as u32,
        b18: B18_LANES.len() as u32,
        overlap,
        status_roundtrip: exit_code_of(exited_status(7)) == 7,
    }
}

// ── 注入矩阵 ──────────────────────────────────────────────────────────
/// 一格 ECHILD 注入：fixture 0=主表、1=空表；lane 是 OWN_LANES 下标。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct EchildCell {
    pub fixture: u8,
    pub pid: i32,
    pub options: u32,
    pub lane: u8,
}

/// 十格 ECHILD 冻结矩阵。
pub const ECHILD_GRID: [EchildCell; ECHILD_CELLS] = [
    // ① 空表 pid=-1：无子进程。
    EchildCell { fixture: 1, pid: -1, options: 0, lane: 0 },
    // ② 空表 pid=0 WNOHANG：无子进程，不阻塞。
    EchildCell { fixture: 1, pid: 0, options: WNOHANG, lane: 0 },
    // ③ 主表 pid 不存在。
    EchildCell { fixture: 0, pid: 650, options: 0, lane: 1 },
    // ④ 主表 wait 自己。
    EchildCell { fixture: 0, pid: 500, options: 0, lane: 3 },
    // ⑤ 主表 wait 父进程。
    EchildCell { fixture: 0, pid: 501, options: 0, lane: 4 },
    // ⑥ 主表 wait 别人的孩子（同进程组也不行）。
    EchildCell { fixture: 0, pid: 700, options: 0, lane: 2 },
    // ⑦ 主表 wait 孙进程。
    EchildCell { fixture: 0, pid: 800, options: 0, lane: 5 },
    // ⑧ 主表 wait 已回收子进程。
    EchildCell { fixture: 0, pid: 604, options: 0, lane: 6 },
    // ⑨ 主表空组。
    EchildCell { fixture: 0, pid: -777, options: 0, lane: 7 },
    // ⑩ 主表不可译选择器（INT_MIN），仍是 ECHILD 而非 EINVAL。
    EchildCell { fixture: 0, pid: i32::MIN, options: 0, lane: 2 },
];

/// 复算单格：是否恰好回 ECHILD。
pub fn cell_matches(cell: &EchildCell) -> bool {
    let mut table = if cell.fixture == 0 {
        main_table()
    } else {
        bare_table()
    };
    let got = table.wait4(&CALLER, cell.pid, cell.options, false);
    matches!(got, Err(ECHILD))
}

/// 注入账本（wait 版：额外记回收/待定两本账）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct WitnessLedger {
    pub injected: u32,
    pub detected: u32,
    pub escaped: u32,
    pub reaps: u32,
    pub pendings: u32,
}

impl WitnessLedger {
    /// 空账。
    pub const fn zero() -> WitnessLedger {
        WitnessLedger {
            injected: 0,
            detected: 0,
            escaped: 0,
            reaps: 0,
            pendings: 0,
        }
    }

    /// 记一笔注入。
    pub fn record(&mut self, caught: bool) {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
    }

    /// 记一次回收。
    pub fn note_reap(&mut self) {
        self.reaps = self.reaps.saturating_add(1);
    }

    /// 记一次待定。
    pub fn note_pending(&mut self) {
        self.pendings = self.pendings.saturating_add(1);
    }

    /// 守恒。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 十格 × 十轮跑矩阵。
pub fn run_echild_matrix() -> ([u32; ECHILD_CELLS], WitnessLedger) {
    let mut per_cell = [0u32; ECHILD_CELLS];
    let mut ledger = WitnessLedger::zero();
    let mut ci = 0usize;
    while ci < ECHILD_CELLS {
        if let Some(cell) = ECHILD_GRID.get(ci).copied() {
            let mut round = 0u32;
            while round < INJECT_ROUNDS {
                let caught = cell_matches(&cell);
                ledger.record(caught);
                if caught {
                    if let Some(slot) = per_cell.get_mut(ci) {
                        *slot = slot.saturating_add(1);
                    }
                }
                round += 1;
            }
        }
        ci += 1;
    }
    (per_cell, ledger)
}

// ── 指纹 ──────────────────────────────────────────────────────────────
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

/// 编码一个 wait 结果（三类样本 + 三类错误）。
pub fn encode_sample(out: &mut Vec<u8>, r: Result<WaitSample, i32>) {
    match r {
        Ok(WaitSample::Reaped { pid, status }) => {
            out.push(1);
            out.push((pid & 0xFF) as u8);
            out.push((status & 0xFF) as u8);
        }
        Ok(WaitSample::Stopped { pid, sig }) => {
            out.push(2);
            out.push((pid & 0xFF) as u8);
            out.push((sig & 0xFF) as u8);
        }
        Ok(WaitSample::Pending) => {
            out.push(3);
            out.push(0);
            out.push(0);
        }
        Err(e) => {
            out.push(4);
            out.push((e & 0x7F) as u8);
            out.push(0);
        }
    }
}

/// 规范语料双跑指纹。
pub fn corpus_fingerprint() -> u32 {
    let mut bytes: Vec<u8> = Vec::new();
    let mut t = main_table();
    encode_sample(&mut bytes, t.wait4(&CALLER, 601, 0, false));
    encode_sample(&mut bytes, t.wait4(&CALLER, 601, 0, false));
    encode_sample(&mut bytes, t.wait4(&CALLER, 600, WNOHANG, false));
    encode_sample(&mut bytes, t.wait4(&CALLER, 603, 0, false));
    encode_sample(&mut bytes, t.wait4(&CALLER, 603, WUNTRACED, false));
    encode_sample(&mut bytes, t.wait4(&CALLER, 650, 0, false));
    encode_sample(&mut bytes, t.wait4(&CALLER, 700, 0, false));
    encode_sample(&mut bytes, t.wait4(&CALLER, 0, WNOHANG, false));
    encode_sample(&mut bytes, t.wait4(&CALLER, -700, WNOHANG, false));
    encode_sample(&mut bytes, t.wait4(&CALLER, 601, 0x10, false));
    fnv1a(&bytes)
}

// ── errno 对照 ────────────────────────────────────────────────────────
/// (名称, Linux 正值, 说明)。
pub const ERRNO_TABLE: &[(&str, i32, &str)] = &[
    ("ESRCH", 3, "wait 路径不得返回：pid 是否存在与本调用无关"),
    ("EINTR", 4, "阻塞被打断，可重试"),
    ("ECHILD", 10, "调用者名下没有匹配的子进程"),
    ("EINVAL", 22, "options 含未定义位"),
];

/// 按名查码；表外 0。
pub fn errno_value(name: &str) -> i32 {
    let mut i = 0usize;
    while i < ERRNO_TABLE.len() {
        if let Some(row) = ERRNO_TABLE.get(i) {
            if row.0 == name {
                return row.1;
            }
        }
        i += 1;
    }
    0
}

/// 码是否允许出现在 wait 路径（ESRCH 明确不允许）。
pub fn wait_errno_allowed(code: i32) -> bool {
    code == errno_value("ECHILD") || code == errno_value("EINVAL") || code == errno_value("EINTR")
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9283 判据集（add ≥12）。
pub fn run_unx9283_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9283");

    let seg_ok = SEG_BASE % 0x40 == 0
        && SEG_BASE >= WINDOW_LO
        && SEG_BASE <= WINDOW_HI
        && SEG_BASE >= 0x7B440
        && SEG_BASE + 0x3F < 0x7B480;
    s.add(
        "段位守卫：0x7B440 窗口且与 F9282/F9284 不重叠",
        seg_ok,
        "段基址必须 0x40 对齐并落在 0x7B440..=0x7B47F",
    );

    // 无子进程注入：空表上四种选择器一律 ECHILD。
    let mut no_child_ok = true;
    let sels = [-1i32, 0, 611, -510];
    let mut si = 0usize;
    while si < sels.len() {
        if let Some(pid) = sels.get(si).copied() {
            let mut t = bare_table();
            if !matches!(t.wait4(&CALLER, pid, 0, false), Err(ECHILD)) {
                no_child_ok = false;
            }
        }
        si += 1;
    }
    s.add(
        "无子进程注入：空表四选择器全 ECHILD",
        no_child_ok,
        "空表是独立语料（611 的 ppid 是 700），不能从主表推断",
    );

    // 全 ECHILD 格：十格 × 十轮。
    let (per_cell, ledger) = run_echild_matrix();
    let mut cells_all = true;
    let mut ci = 0usize;
    while ci < ECHILD_CELLS {
        match per_cell.get(ci) {
            Some(n) => {
                if *n != INJECT_ROUNDS {
                    cells_all = false;
                }
            }
            None => cells_all = false,
        }
        ci += 1;
    }
    s.add(
        "全 ECHILD 格：10 格各 10/10 命中",
        cells_all,
        "不存在/自身/父/他人子/孙/已回收/空组/INT_MIN 必须逐格回 ECHILD",
    );
    s.add(
        "注入账：100 次全检出且逃逸为 0",
        ledger.conserved()
            && ledger.escaped == 0
            && ledger.injected == ECHILD_CELLS as u32 * INJECT_ROUNDS,
        "injected == detected + escaped；逃逸非零即有格被误判",
    );

    // 非我子进程四连：自身/父/他人子/孙。
    let mut foreign_ok = true;
    let foreign = [500i32, 501, 700, 800];
    let mut fi = 0usize;
    while fi < foreign.len() {
        if let Some(pid) = foreign.get(fi).copied() {
            let mut t = main_table();
            if !matches!(t.wait4(&CALLER, pid, 0, false), Err(ECHILD)) {
                foreign_ok = false;
            }
        }
        fi += 1;
    }
    s.add(
        "选择器面：非我子进程全 ECHILD",
        foreign_ok,
        "waitpid 只认 ppid == 调用者的子集，同进程组的别人的孩子也不行",
    );

    // 已回收子进程：二次 wait 必须 ECHILD，且回收计数只加一次。
    let mut twice = main_table();
    let first = twice.wait4(&CALLER, 601, 0, false);
    let first_ok = matches!(first, Ok(WaitSample::Reaped { pid: 601, status }) if status == exited_status(7));
    let reaps_after_first = twice.reaps();
    let second = twice.wait4(&CALLER, 601, 0, false);
    let twice_ok = first_ok
        && matches!(second, Err(ECHILD))
        && twice.reaps() == reaps_after_first
        && reaps_after_first >= 1;
    s.add(
        "状态机：僵尸回收后二次 wait ECHILD",
        twice_ok,
        "Reaped 是终态，重复回收必须失败且不得重复计数",
    );

    // 选项面：非法位 EINVAL，且先于 ECHILD（空表上也是 EINVAL）。
    let mut opt_ok = true;
    let bad_opts = [0x10u32, 0x4, 0x100, 0x0020];
    let mut oi = 0usize;
    while oi < bad_opts.len() {
        if let Some(o) = bad_opts.get(oi).copied() {
            let mut t = main_table();
            if !matches!(t.wait4(&CALLER, 601, o, false), Err(EINVAL)) {
                opt_ok = false;
            }
            let mut b = bare_table();
            if !matches!(b.wait4(&CALLER, -1, o, false), Err(EINVAL)) {
                opt_ok = false;
            }
        }
        oi += 1;
    }
    s.add(
        "选项面：非法位全 EINVAL 且先于 ECHILD",
        opt_ok,
        "空表上也要先报 EINVAL，否则调用方会去改 pid 而不是改 options",
    );

    // 合法选项位全组合不得触发 EINVAL。
    let legal_opts = [0u32, WNOHANG, WUNTRACED, WCONTINUED, WNOTHREAD, WALL, WCLONE, WNOHANG | WUNTRACED];
    let mut legal_ok = true;
    let mut li = 0usize;
    while li < legal_opts.len() {
        if let Some(o) = legal_opts.get(li).copied() {
            let mut t = main_table();
            if matches!(t.wait4(&CALLER, 601, o, false), Err(EINVAL)) {
                legal_ok = false;
            }
        }
        li += 1;
    }
    s.add(
        "选项面：八个合法组合无 EINVAL",
        legal_ok,
        "合法位组合必须放行到选择器面，不得因组合误报参数错误",
    );

    // ECHILD ≠ ESRCH：全域扫描错误码集合。
    let mut only_echild = true;
    let sweep = [-1i32, 0, 1, 500, 501, 604, 650, 700, 800, -700, i32::MIN];
    let mut wi = 0usize;
    while wi < sweep.len() {
        if let Some(pid) = sweep.get(wi).copied() {
            let mut t = main_table();
            if let Err(e) = t.wait4(&CALLER, pid, 0, false) {
                if e == ESRCH {
                    only_echild = false;
                }
            }
        }
        wi += 1;
    }
    s.add(
        "差异判据：wait 路径永不返回 ESRCH",
        only_echild,
        "pid 在系统里是否存在与 wait 无关，出现 ESRCH 即语义串味",
    );

    // WNOHANG：运行中的子进程不阻塞、不改状态。
    let mut hang = main_table();
    let pending = hang.wait4(&CALLER, 600, WNOHANG, false);
    let before = match hang.peek(600) {
        Some(k) => k.state,
        None => ChildState::Reaped,
    };
    let hang_ok = matches!(pending, Ok(WaitSample::Pending))
        && before == ChildState::Running
        && hang.reaps() == 1;
    s.add(
        "WNOHANG：运行中子进程回 Pending 且状态不变",
        hang_ok,
        "轮询语义归 B18，但不得污染本模块的回收计数",
    );

    // 停止子进程：WUNTRACED 报告、不带则 Pending。
    let mut st = main_table();
    let with = st.wait4(&CALLER, 603, WUNTRACED, false);
    let without = st.wait4(&CALLER, 603, 0, false);
    let stop_ok = matches!(with, Ok(WaitSample::Stopped { pid: 603, sig: 19 }))
        && matches!(without, Ok(WaitSample::Pending));
    s.add(
        "停止子进程：WUNTRACED 报告、缺省不报",
        stop_ok,
        "停止报告属 B18 语义，本模块只验证它不被坏 pid 判定吞掉",
    );

    // 组选择器：pgid 700 命中 602，空组 777 ECHILD。
    let mut grp = main_table();
    let g_hit = grp.wait4(&CALLER, -700, WNOHANG, false);
    let g_empty = grp.wait4(&CALLER, -777, WNOHANG, false);
    let grp_ok = matches!(g_hit, Ok(WaitSample::Pending)) && matches!(g_empty, Err(ECHILD));
    s.add(
        "组选择器：-700 命中子进程、-777 空组 ECHILD",
        grp_ok,
        "组形态只在调用者自己的子进程里筛，空组回 ECHILD 而非成功 0",
    );

    // 选择器译码全域：INT_MIN 不可译但不产生 EINVAL。
    let dec_ok = decode_selector(i32::MIN) == Selector::Undecodable
        && decode_selector(-1) == Selector::AnyChild
        && decode_selector(0) == Selector::OwnGroup
        && decode_selector(7) == Selector::Exact(7)
        && decode_selector(-7) == Selector::Group(7);
    s.add(
        "译码：INT_MIN 不可译且仍走 ECHILD",
        dec_ok,
        "取负溢出是选择器面的事实，不是参数面错误",
    );

    // B18 分层：零重叠、自有格齐全、状态字往返一致。
    let rec = reconcile_b18();
    s.add(
        "B18 对账：八格自有与八格 B18 零重叠",
        rec.overlap == 0
            && rec.own == OWN_CELLS as u32
            && rec.b18 == B18_CELLS as u32
            && rec.status_roundtrip,
        "状态字/轮询/停止/rusage/pid 回显归 B18，坏 pid 归本模块",
    );

    // 自有矩阵每格 lane 必须落在 OWN_LANES 内。
    let mut lane_ok = true;
    let mut gi = 0usize;
    while gi < ECHILD_GRID.len() {
        if let Some(cell) = ECHILD_GRID.get(gi) {
            if (cell.lane as usize) >= OWN_LANES.len() {
                lane_ok = false;
            }
        }
        gi += 1;
    }
    s.add(
        "分层归属：十格全部落在自有 lane",
        lane_ok,
        "任何一格越界就说明它其实是 B18 的格，必须移出本矩阵",
    );

    // errno 数值。
    let errno_ok = errno_value("ECHILD") == 10
        && errno_value("EINVAL") == 22
        && errno_value("ESRCH") == 3
        && errno_value("EINTR") == 4
        && wait_errno_allowed(ECHILD)
        && wait_errno_allowed(EINVAL)
        && !wait_errno_allowed(ESRCH);
    s.add(
        "errno：ECHILD/EINVAL/EINTR 合法、ESRCH 禁入",
        errno_ok,
        "wait 面错误码闭集为 {3 之外}，表外码一律拒绝",
    );

    // 双跑指纹。
    let fp_a = corpus_fingerprint();
    let fp_b = corpus_fingerprint();
    s.add(
        "双跑一致：规范语料指纹相等",
        fp_a == fp_b && fp_a != 0,
        "同输入两次必须逐字节相等",
    );

    // 诊断码闭集唯一性与文案。
    let mut uniq = true;
    let mut i = 0usize;
    while i < WaitCode::ALL.len() {
        let mut j = i + 1;
        while j < WaitCode::ALL.len() {
            if let (Some(a), Some(b)) = (WaitCode::ALL.get(i), WaitCode::ALL.get(j)) {
                if a.val() == b.val() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    let text_ok = WaitCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add(
        "诊断码：12 个码值唯一且文案非空",
        uniq && text_ok && WaitCode::ALL.len() == 12,
        "闭集码不得重复，每码必须有可操作建议",
    );

    // 账目单调。
    let mut mono = ledger;
    let base = mono.injected;
    let mut k = 0u32;
    while k < 4 {
        mono.record(true);
        mono.note_pending();
        k += 1;
    }
    s.add(
        "账目单调：追加 4 笔只增不减",
        mono.injected == base.saturating_add(4)
            && mono.escaped == 0
            && mono.pendings == 4
            && mono.conserved(),
        "累计计数禁止回退，Pendings 账独立累加",
    );

    // 动态快照。
    let matrix_text = format!(
        "ECHILD 矩阵 {} / 检出 {} / 逃逸 {}",
        ledger.injected, ledger.detected, ledger.escaped
    );
    s.add_dyn("矩阵账目", ledger.conserved(), &matrix_text);

    let lane_text = format!(
        "自有 {} 格 / B18 {} 格 / 重叠 {}",
        rec.own, rec.b18, rec.overlap
    );
    s.add_dyn("分层对账", rec.overlap == 0, &lane_text);

    let fp_text = format!("语料指纹 0x{:08X}", fp_a);
    s.add_dyn("指纹快照", fp_a == fp_b, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selector_decode_is_total() {
        assert_eq!(decode_selector(1), Selector::Exact(1));
        assert_eq!(decode_selector(0), Selector::OwnGroup);
        assert_eq!(decode_selector(-1), Selector::AnyChild);
        assert_eq!(decode_selector(-9), Selector::Group(9));
        assert_eq!(decode_selector(i32::MIN), Selector::Undecodable);
    }

    #[test]
    fn bare_table_has_no_children() {
        let t = bare_table();
        assert_eq!(t.has_child(CALLER.pid), false);
        assert_eq!(t.child_count(CALLER.pid), 0);
        assert_eq!(t.len(), 2);
    }

    #[test]
    fn absent_and_foreign_are_echild() {
        let mut t = main_table();
        assert_eq!(t.wait4(&CALLER, 650, 0, false), Err(ECHILD));
        assert_eq!(t.wait4(&CALLER, 500, 0, false), Err(ECHILD));
        assert_eq!(t.wait4(&CALLER, 501, 0, false), Err(ECHILD));
        assert_eq!(t.wait4(&CALLER, 700, 0, false), Err(ECHILD));
        assert_eq!(t.wait4(&CALLER, 800, 0, false), Err(ECHILD));
    }

    #[test]
    fn reaped_child_is_gone() {
        let mut t = main_table();
        let first = t.wait4(&CALLER, 601, 0, false);
        assert_eq!(first, Ok(WaitSample::Reaped { pid: 601, status: exited_status(7) }));
        assert_eq!(t.wait4(&CALLER, 601, 0, false), Err(ECHILD));
        assert_eq!(t.reaps(), 2);
    }

    #[test]
    fn bad_options_win_over_echild() {
        let mut bare = bare_table();
        assert_eq!(bare.wait4(&CALLER, -1, 0x10, false), Err(EINVAL));
        let mut main = main_table();
        assert_eq!(main.wait4(&CALLER, 601, 0x4, false), Err(EINVAL));
    }

    #[test]
    fn wnohang_does_not_change_state() {
        let mut t = main_table();
        assert_eq!(t.wait4(&CALLER, 600, WNOHANG, false), Ok(WaitSample::Pending));
        match t.peek(600) {
            Some(k) => assert_eq!(k.state, ChildState::Running),
            None => assert_eq!(true, false),
        }
    }

    #[test]
    fn stopped_reported_only_with_wuntraced() {
        let mut t = main_table();
        assert_eq!(t.wait4(&CALLER, 603, WUNTRACED, false), Ok(WaitSample::Stopped { pid: 603, sig: 19 }));
        assert_eq!(t.wait4(&CALLER, 603, 0, false), Ok(WaitSample::Pending));
    }

    #[test]
    fn group_selector_filters() {
        let mut t = main_table();
        assert_eq!(t.wait4(&CALLER, -700, WNOHANG, false), Ok(WaitSample::Pending));
        assert_eq!(t.wait4(&CALLER, -777, 0, false), Err(ECHILD));
        assert_eq!(t.wait4(&CALLER, -1000, 0, false), Err(ECHILD));
    }

    #[test]
    fn interrupt_is_eintr_not_echild() {
        let mut t = main_table();
        assert_eq!(t.wait4(&CALLER, 600, 0, true), Err(EINTR));
        assert_eq!(t.wait4(&CALLER, 600, WNOHANG, true), Ok(WaitSample::Pending));
    }

    #[test]
    fn state_machine_is_directional() {
        assert_eq!(ChildState::Running.can_step_to(ChildState::Zombie), true);
        assert_eq!(ChildState::Zombie.can_step_to(ChildState::Reaped), true);
        assert_eq!(ChildState::Reaped.can_step_to(ChildState::Zombie), false);
        assert_eq!(ChildState::Zombie.can_step_to(ChildState::Running), false);
    }

    #[test]
    fn matrix_conserved() {
        let (per, l) = run_echild_matrix();
        assert_eq!(l.conserved(), true);
        assert_eq!(l.escaped, 0);
        assert_eq!(l.injected, 100);
        let mut i = 0usize;
        while i < per.len() {
            match per.get(i) {
                Some(n) => assert_eq!(*n, INJECT_ROUNDS),
                None => assert_eq!(true, false),
            }
            i += 1;
        }
    }

    #[test]
    fn b18_lanes_are_disjoint_and_status_roundtrips() {
        let r = reconcile_b18();
        assert_eq!(r.overlap, 0);
        assert_eq!(r.own, 8);
        assert_eq!(r.b18, 8);
        assert_eq!(replace_check(), true);
        assert_eq!(r.status_roundtrip, true);
    }

    // 帮助断言：状态字编码必须是纯粹的位移，不得夹带其它位。
    fn replace_check() -> bool {
        let st = exited_status(0x7F);
        exit_code_of(st) == 0x7F && st == 0x7F00
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx9283_checks();
        assert_eq!(s.all_passed(), true);
        assert_eq!(s.dyn_dropped(), 0);
    }
}
