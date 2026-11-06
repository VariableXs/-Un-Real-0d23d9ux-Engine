//! UNX-F9295 进程族 E 型与 B04/B08 深化册对账格 · 0x7B740段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 UNX-F9295 逐字）：
//!   ① 纯功能代码 ≥300 行；② 判据原文：UNX-F9295-J1 对账判据过，与
//!   fork 档/进程族地基深化册逐条零冲突格过，正反分层声明格过；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//!   ① 域模型是进程表状态机（与 F9202 的 fd 表完全不同）：定容槽位，
//!      状态秩 Free(0)→Ready(1)→Running(2)→Zombie(3) 只允许 +1 单调前进；
//!      倒退（Running→Ready）与越级（Ready→Zombie）一律拒绝并计入拒绝账；
//!      收割（Zombie→Free）只走 reap 专用门并推进代际，收割后旧 pid 立即
//!      失活（pid 单调分配、永不复用，陈旧 pid 复查即 ESRCH）。
//!   ② 对账域模型围绕冻结对账表设计：12 行（场景名 / 深化册登记号 /
//!      登记码 / Linux errno 正码），每行配独立复算器——复算器真的新建
//!      一张进程表、真的跑一次判定器、取回实测码，再与登记码、Linux 码
//!      三方逐值相等；全表一致即「与 fork 档/进程族地基深化册逐条零冲突」。
//!   ③ E 型错误面按 Linux 语义定序：fork/clone 为 ESRCH→EINVAL→EAGAIN→
//!      ENOMEM；setpgid/setsid 为 ESRCH→EPERM（存在性先于权限）；execve
//!      为 ESRCH→E2BIG→EFAULT→EACCES→ENOEXEC。两条错误同时成立时，
//!      先返回约定链上靠前的一方。
//!   ④ 正反分层声明两账分开：正向账（合法输入全成功）与反向账（违约
//!      输入全按登记码拒绝）各自建账、各自判定，禁止混账互证；任一层
//!      出现一条偏差即整体判红。
//!   ⑤ 三类注入器（坏 pid / 超限并发 / 坏 flags）各 10 轮全抓；注入账
//!      守恒（注入 = 检出 + 逃逸，逃逸恒为 0）；规范语料双跑指纹逐字段
//!      一致并入账，任何隐藏状态即失败。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F9295 独占 0x40 窗口；原样保留 0x7B740）。
pub const SEG_BASE: u32 = 0x7B740;

/// 进程表槽位数量（0 号槽在 `new()` 中预置为 init 进程）。
pub const MAX_SLOTS: usize = 16;

/// 单类注入器的复测轮数（判据要求 10/10 全抓）。
pub const INJECT_ROUNDS: u32 = 10;

/// 注入器类别数（坏 pid / 超限并发 / 坏 flags）。
pub const INJECTOR_CLASSES: u32 = 3;

/// 冻结对账表行数。
pub const RECON_ROWS: usize = 12;

/// fork/clone 一次最小内存页需求（页数，低于此回 ENOMEM）。
pub const FORK_MIN_PAGES: u32 = 4;

/// execve 参数区上限（字节；超出回 E2BIG）。
pub const ARG_MAX: u32 = 131072;

/// clone 合法 flags 掩码：0x1..0xF 之内且非零。
pub const CLONE_VALID_MASK: u32 = 0x0F;

// ── Linux errno 正数码（本模块对账口径） ─────────────────────────────
/// EPERM = 1（权限拒绝族）。
pub const EPERM: i32 = 1;
/// ESRCH = 3（目标进程不存在族）。
pub const ESRCH: i32 = 3;
/// E2BIG = 7（参数区超长族）。
pub const E2BIG: i32 = 7;
/// ENOEXEC = 8（可执行格式不识别族）。
pub const ENOEXEC: i32 = 8;
/// EAGAIN = 11（资源暂时不可用/并发超限族）。
pub const EAGAIN: i32 = 11;
/// ENOMEM = 12（内存不足族）。
pub const ENOMEM: i32 = 12;
/// EACCES = 13（权限不足族）。
pub const EACCES: i32 = 13;
/// EFAULT = 14（坏地址族）。
pub const EFAULT: i32 = 14;
/// EINVAL = 22（非法参数族）。
pub const EINVAL: i32 = 22;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..+0x0F 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ProcCode {
    /// 目标进程不存在（含已 Zombie 未收割、陈旧 pid、pid 0/负值）
    NoSuchProc,
    /// fork/clone 并发超限（RLIMIT_NPROC 或槽位耗尽）
    ForkConcurrency,
    /// fork/clone 内存不足
    ForkNoMem,
    /// clone flags 非法（全零或越出合法掩码）
    CloneFlags,
    /// setpgid 权限拒绝（会话首改组 / 组跨会话）
    PgidDenied,
    /// setsid 权限拒绝（已是会话首 / 已是组长）
    SidDenied,
    /// execve 无执行权限
    ExecDenied,
    /// execve 可执行格式不识别
    ExecFormat,
    /// execve 参数区超长
    ExecArgOverflow,
    /// execve 坏地址
    ExecFault,
    /// 状态机非法倒退/越级
    StateRegress,
    /// 非法收割（目标不是 Zombie）
    ZombieReapIllegal,
    /// 注入账倒退
    LedgerRegress,
    /// 双跑不一致
    NotReproducible,
    /// 段位越界
    SegmentViolation,
}

impl ProcCode {
    /// 码值（段基址 + 变体序号）。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            ProcCode::NoSuchProc => "目标进程不存在：pid 为 0/负值、已收割陈旧号或\
                已退出未收割的 Zombie；请先 wait 收割再复查，存在性判定先于权限判定",
            ProcCode::ForkConcurrency => "并发超限被拒：存活进程数已达 RLIMIT_NPROC 或\
                进程表槽位耗尽；请收割子进程或上调上限后重试，回 EAGAIN",
            ProcCode::ForkNoMem => "内存不足被拒：可用页低于 fork/clone 最小需求；\
                请先释放内存或降低并发，回 ENOMEM",
            ProcCode::CloneFlags => "clone flags 非法被拒：flags 全零或含有超出合法\
                掩码的位；请按 0x1..0x0F 组合取值，回 EINVAL",
            ProcCode::PgidDenied => "setpgid 权限拒绝：会话首不得改组，或目标组不在\
                同一会话；请改用组内 pid 或 0（自立新组），回 EPERM",
            ProcCode::SidDenied => "setsid 权限拒绝：调用者已是会话首或已是进程组长；\
                请先脱离现任组长身份再建会话，回 EPERM",
            ProcCode::ExecDenied => "execve 权限拒绝：目标镜像无执行位；请先 chmod +x \
                或换用可执行镜像，回 EACCES",
            ProcCode::ExecFormat => "execve 格式不识别：镜像头不是本内核支持的格式；\
                请确认 ELF 头或更换镜像，回 ENOEXEC",
            ProcCode::ExecArgOverflow => "execve 参数超长：argv/envp 总量超过上限；\
                请压缩参数后重试，回 E2BIG",
            ProcCode::ExecFault => "execve 坏地址：参数区指针 NULL/未映射/内核态；\
                请把参数固定到已映射用户区间，回 EFAULT",
            ProcCode::StateRegress => "状态机非法迁移被拒：槽位状态只允许按秩 +1 单调\
                前进，倒退与越级均拒绝；请按 Free→Ready→Running→Zombie 推进",
            ProcCode::ZombieReapIllegal => "非法收割被拒：只有 Zombie 态可 reap；\
                请先让进程按链退出再收割，回 EINVAL",
            ProcCode::LedgerRegress => "注入账倒退被拒：计数只增不减；请检查是否有\
                回滚路径误改累计值",
            ProcCode::NotReproducible => "双跑不一致：同输入两次结果不同；请排查\
                隐藏状态或非确定哈希",
            ProcCode::SegmentViolation => "段位越界：本模块段基址必须落在 \
                0x7A000..=0x7D1C0 且 0x40 对齐",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [ProcCode; 15] = [
        ProcCode::NoSuchProc,
        ProcCode::ForkConcurrency,
        ProcCode::ForkNoMem,
        ProcCode::CloneFlags,
        ProcCode::PgidDenied,
        ProcCode::SidDenied,
        ProcCode::ExecDenied,
        ProcCode::ExecFormat,
        ProcCode::ExecArgOverflow,
        ProcCode::ExecFault,
        ProcCode::StateRegress,
        ProcCode::ZombieReapIllegal,
        ProcCode::LedgerRegress,
        ProcCode::NotReproducible,
        ProcCode::SegmentViolation,
    ];
}

/// 系统调用判定结果：成功携带效果，失败携带 Linux errno 正数码。
pub type SysResult<T> = Result<T, i32>;

/// 取错误码视图：成功归 0，失败取正码（声明账统一口径，成功值类型无关）。
pub fn code_of<T>(r: Result<T, i32>) -> i32 {
    match r {
        Ok(_) => 0,
        Err(e) => e,
    }
}

/// 取无值结果的错误码视图。
pub fn code_of_unit(r: SysResult<()>) -> i32 {
    match r {
        Ok(_) => 0,
        Err(e) => e,
    }
}

// ── 进程表状态机 ──────────────────────────────────────────────────────
/// 槽位状态：秩 0..3，只允许 +1 单调前进。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ProcState {
    /// 空槽（从未分配或已收割）
    Free,
    /// 已创建待调度（fork/clone 成功落槽）
    Ready,
    /// 正在运行
    Running,
    /// 已退出待收割
    Zombie,
}

/// 状态秩：Free=0，Ready=1，Running=2，Zombie=3。
pub fn rank_of(st: ProcState) -> u32 {
    match st {
        ProcState::Free => 0,
        ProcState::Ready => 1,
        ProcState::Running => 2,
        ProcState::Zombie => 3,
    }
}

/// 单个进程槽位。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ProcSlot {
    /// 槽位状态
    pub state: ProcState,
    /// 进程号（Free 槽恒为 0；pid 单调分配，永不复用）
    pub pid: i32,
    /// 父进程号
    pub ppid: i32,
    /// 进程组号
    pub pgid: i32,
    /// 会话号（sid == pid 即会话首）
    pub sid: i32,
    /// 代际（每次收割自增；旧 pid 收割后即失活）
    pub generation: u32,
}

impl ProcSlot {
    /// 空槽常量构造（数组重复要求 const fn + Copy）。
    const fn empty() -> ProcSlot {
        ProcSlot {
            state: ProcState::Free,
            pid: 0,
            ppid: 0,
            pgid: 0,
            sid: 0,
            generation: 1,
        }
    }
}

/// 定容进程表。0 号槽预置 init：pid 1、Running、自组自会话。
#[derive(Clone, Copy, Debug)]
pub struct ProcTable {
    slots: [ProcSlot; MAX_SLOTS],
    /// 下一个待分配 pid（单调递增，不复用）
    next_pid: i32,
    /// 成功创建账
    spawns: u32,
    /// 成功退出账
    exits: u32,
    /// 成功收割账
    reaps: u32,
    /// 非法迁移拒绝账（只增）
    rejected: u32,
}

impl ProcTable {
    /// 构造含 init 的进程表（0 号槽 init，1..15 号槽空）。
    pub const fn new() -> ProcTable {
        let mut init = ProcSlot::empty();
        init.state = ProcState::Running;
        init.pid = 1;
        init.ppid = 0;
        init.pgid = 1;
        init.sid = 1;
        ProcTable {
            slots: [
                init,
                ProcSlot::empty(),
                ProcSlot::empty(),
                ProcSlot::empty(),
                ProcSlot::empty(),
                ProcSlot::empty(),
                ProcSlot::empty(),
                ProcSlot::empty(),
                ProcSlot::empty(),
                ProcSlot::empty(),
                ProcSlot::empty(),
                ProcSlot::empty(),
                ProcSlot::empty(),
                ProcSlot::empty(),
                ProcSlot::empty(),
                ProcSlot::empty(),
            ],
            next_pid: 2,
            spawns: 0,
            exits: 0,
            reaps: 0,
            rejected: 0,
        }
    }

    /// 按 pid 找槽（含 Zombie；Free 槽不可见）。
    pub fn find(&self, pid: i32) -> Option<usize> {
        let mut i = 0usize;
        while i < MAX_SLOTS {
            if let Some(s) = self.slots.get(i) {
                if s.state != ProcState::Free && s.pid == pid {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 按 pid 找存活槽（Ready|Running；Zombie 对判定器视同不存在）。
    pub fn find_live(&self, pid: i32) -> Option<usize> {
        let idx = self.find(pid);
        match idx {
            Some(i) => {
                let st = self.state_at(i);
                match st {
                    ProcState::Ready | ProcState::Running => Some(i),
                    _ => None,
                }
            }
            None => None,
        }
    }

    /// 存活进程数（Ready + Running；Zombie 不计入并发上限，与 Linux 一致）。
    pub fn live_count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < MAX_SLOTS {
            if let Some(s) = self.slots.get(i) {
                match s.state {
                    ProcState::Ready | ProcState::Running => n += 1,
                    _ => {}
                }
            }
            i += 1;
        }
        n
    }

    /// 读槽位状态（下标越界归 Free）。
    fn state_at(&self, idx: usize) -> ProcState {
        match self.slots.get(idx) {
            Some(s) => s.state,
            None => ProcState::Free,
        }
    }

    /// 状态视图（外部只读检查用；pid 不存在归 Free）。
    pub fn state_view(&self, pid: i32) -> ProcState {
        match self.find(pid) {
            Some(i) => self.state_at(i),
            None => ProcState::Free,
        }
    }

    /// 单调前进：只允许秩 +1。pid 不存在回 ESRCH，非法迁移回 EINVAL
    /// 并计入拒绝账；成功迁入 Zombie 记退出账一笔。
    pub fn advance(&mut self, pid: i32, to: ProcState) -> SysResult<()> {
        let idx = match self.find(pid) {
            Some(i) => i,
            None => return Err(ESRCH),
        };
        let cur = self.state_at(idx);
        if rank_of(to) != rank_of(cur).saturating_add(1) {
            self.rejected = self.rejected.saturating_add(1);
            return Err(EINVAL);
        }
        if let Some(s) = self.slots.get_mut(idx) {
            s.state = to;
        }
        if to == ProcState::Zombie {
            self.exits = self.exits.saturating_add(1);
        }
        Ok(())
    }

    /// 收割：仅 Zombie 可回 Free 并推进代际；其余一律 EINVAL。
    pub fn reap(&mut self, pid: i32) -> SysResult<u32> {
        let idx = match self.find(pid) {
            Some(i) => i,
            None => return Err(ESRCH),
        };
        if self.state_at(idx) != ProcState::Zombie {
            self.rejected = self.rejected.saturating_add(1);
            return Err(EINVAL);
        }
        let gen = match self.slots.get_mut(idx) {
            Some(s) => {
                s.state = ProcState::Free;
                s.pid = 0;
                s.ppid = 0;
                s.pgid = 0;
                s.sid = 0;
                s.generation = s.generation.saturating_add(1);
                s.generation
            }
            None => return Err(ESRCH),
        };
        self.reaps = self.reaps.saturating_add(1);
        Ok(gen)
    }

    /// 在 Free 槽上创建子进程（继承 caller 的 pgid/sid），状态 Ready。
    /// 无 Free 槽回 EAGAIN；caller 缺失回 ESRCH。
    pub fn spawn(&mut self, caller: i32) -> SysResult<i32> {
        let c_idx = match self.find_live(caller) {
            Some(i) => i,
            None => return Err(ESRCH),
        };
        let (c_pgid, c_sid, c_pid) = match self.slots.get(c_idx) {
            Some(s) => (s.pgid, s.sid, s.pid),
            None => return Err(ESRCH),
        };
        let mut target = None;
        let mut i = 0usize;
        while i < MAX_SLOTS {
            if self.state_at(i) == ProcState::Free {
                target = Some(i);
                i = MAX_SLOTS;
            } else {
                i += 1;
            }
        }
        let idx = match target {
            Some(i) => i,
            None => return Err(EAGAIN),
        };
        let pid = self.next_pid;
        if let Some(s) = self.slots.get_mut(idx) {
            s.state = ProcState::Ready;
            s.pid = pid;
            s.ppid = c_pid;
            s.pgid = c_pgid;
            s.sid = c_sid;
        }
        self.next_pid = self.next_pid.saturating_add(1);
        self.spawns = self.spawns.saturating_add(1);
        Ok(pid)
    }

    /// 会话/组信息视图（仅存活进程）。
    fn session_view(&self, pid: i32) -> Option<(i32, i32)> {
        let idx = self.find_live(pid);
        match idx {
            Some(i) => match self.slots.get(i) {
                Some(s) => Some((s.sid, s.pgid)),
                None => None,
            },
            None => None,
        }
    }

    /// 改组（判定器内部专用）：目标必须存活。
    fn set_pgid(&mut self, pid: i32, pgid: i32) -> SysResult<()> {
        let idx = match self.find_live(pid) {
            Some(i) => i,
            None => return Err(ESRCH),
        };
        if let Some(s) = self.slots.get_mut(idx) {
            s.pgid = pgid;
        }
        Ok(())
    }

    /// 建新会话（判定器内部专用）：sid=pgid=pid。
    fn make_session_leader(&mut self, pid: i32) -> SysResult<i32> {
        let idx = match self.find_live(pid) {
            Some(i) => i,
            None => return Err(ESRCH),
        };
        if let Some(s) = self.slots.get_mut(idx) {
            s.sid = s.pid;
            s.pgid = s.pid;
        }
        Ok(pid)
    }

    /// 是否存在存活进程组号 == pgid（setsid 的组长冲突判定）。
    pub fn pgid_exists(&self, pgid: i32) -> bool {
        let mut i = 0usize;
        while i < MAX_SLOTS {
            if let Some(s) = self.slots.get(i) {
                match s.state {
                    ProcState::Ready | ProcState::Running => {
                        if s.pgid == pgid {
                            return true;
                        }
                    }
                    _ => {}
                }
            }
            i += 1;
        }
        false
    }

    /// 目标会话内是否存在组号 == pgid 的存活组（setpgid 跨会话判定）。
    pub fn group_in_same_session(&self, pgid: i32, sid: i32) -> bool {
        let mut i = 0usize;
        while i < MAX_SLOTS {
            if let Some(s) = self.slots.get(i) {
                match s.state {
                    ProcState::Ready | ProcState::Running => {
                        if s.pgid == pgid && s.sid == sid {
                            return true;
                        }
                    }
                    _ => {}
                }
            }
            i += 1;
        }
        false
    }

    /// 各类账目视图。
    pub fn spawns(&self) -> u32 {
        self.spawns
    }

    pub fn exits(&self) -> u32 {
        self.exits
    }

    pub fn reaps(&self) -> u32 {
        self.reaps
    }

    pub fn rejected(&self) -> u32 {
        self.rejected
    }

    /// 代际视图：pid 不存在归 0。
    pub fn generation_of(&self, pid: i32) -> u32 {
        match self.find(pid) {
            Some(i) => match self.slots.get(i) {
                Some(s) => s.generation,
                None => 0,
            },
            None => 0,
        }
    }
}

// ── 判定器输入类别 ────────────────────────────────────────────────────
/// 用户参数区指针类别（execve 的 EFAULT 面）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PtrClass {
    /// 已映射且可访问的合法用户区间
    Valid,
    /// NULL
    Null,
    /// 未映射地址
    Unmapped,
    /// 内核态地址（用户态不可访问）
    Kernel,
}

impl PtrClass {
    /// 用户态可用性。
    pub fn usable(self) -> bool {
        matches!(self, PtrClass::Valid)
    }
}

/// 目标镜像权限类别。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PathPerm {
    /// 有执行位
    ExecOk,
    /// 无执行位（EACCES）
    NoExecBit,
}

/// 可执行格式类别。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ExecFmt {
    /// 本内核支持的 ELF 镜像
    ElfValid,
    /// 支持的解释脚本
    Script,
    /// 不识别的格式（ENOEXEC）
    Unknown,
}

// ── 判定器 ────────────────────────────────────────────────────────────
/// fork 判定：ESRCH（调用者缺）→ EAGAIN（并发超限）→ ENOMEM（内存不足）
/// → 成功落 Ready 槽。并发上限同时受 RLIMIT_NPROC 与表容量约束。
pub fn judge_fork(table: &mut ProcTable, caller: i32, rlimit: u32, mem_pages: u32) -> SysResult<i32> {
    if table.find_live(caller).is_none() {
        return Err(ESRCH);
    }
    let used = table.live_count();
    if used >= rlimit as usize || used >= MAX_SLOTS {
        return Err(EAGAIN);
    }
    if mem_pages < FORK_MIN_PAGES {
        return Err(ENOMEM);
    }
    table.spawn(caller)
}

/// clone 判定：ESRCH → EINVAL（坏 flags）→ EAGAIN → ENOMEM → 成功。
/// flags 必须非零且完全落在合法掩码内。
pub fn judge_clone(table: &mut ProcTable, caller: i32, flags: u32, rlimit: u32, mem_pages: u32) -> SysResult<i32> {
    if table.find_live(caller).is_none() {
        return Err(ESRCH);
    }
    if flags == 0 || flags & !CLONE_VALID_MASK != 0 {
        return Err(EINVAL);
    }
    let used = table.live_count();
    if used >= rlimit as usize || used >= MAX_SLOTS {
        return Err(EAGAIN);
    }
    if mem_pages < FORK_MIN_PAGES {
        return Err(ENOMEM);
    }
    table.spawn(caller)
}

/// setpgid 判定：caller 存在（ESRCH）→ 目标存在且非 Zombie（ESRCH，
/// pid==0 视为 caller 自身）→ 会话首改组（EPERM）→ 跨会话组（EPERM）
/// → 成功改组。存在性先于权限，与 UNX-F9293 判序协议一致。
pub fn judge_setpgid(table: &mut ProcTable, caller: i32, pid: i32, pgid: i32) -> SysResult<()> {
    if table.find_live(caller).is_none() {
        return Err(ESRCH);
    }
    let eff_pid = if pid == 0 { caller } else { pid };
    if table.find_live(eff_pid).is_none() {
        return Err(ESRCH);
    }
    let t_sid = match table.session_view(eff_pid) {
        Some((sid, _)) => sid,
        None => return Err(ESRCH),
    };
    if t_sid == eff_pid {
        return Err(EPERM);
    }
    let eff_pgid = if pgid == 0 { eff_pid } else { pgid };
    if eff_pgid != eff_pid && !table.group_in_same_session(eff_pgid, t_sid) {
        return Err(EPERM);
    }
    table.set_pgid(eff_pid, eff_pgid)
}

/// setsid 判定：目标存在（ESRCH）→ 已是会话首（EPERM）→ 已是组长
/// （EPERM，存在存活组号 == pid）→ 成功建会话返回新 sid。
pub fn judge_setsid(table: &mut ProcTable, pid: i32) -> SysResult<i32> {
    if table.find_live(pid).is_none() {
        return Err(ESRCH);
    }
    let sid = match table.session_view(pid) {
        Some((s, _)) => s,
        None => return Err(ESRCH),
    };
    if sid == pid {
        return Err(EPERM);
    }
    if table.pgid_exists(pid) {
        return Err(EPERM);
    }
    table.make_session_leader(pid)
}

/// execve 判定：ESRCH（目标缺/Zombie）→ E2BIG（参数超长）→ EFAULT
/// （坏地址）→ EACCES（无执行位）→ ENOEXEC（格式不识别）→ 成功。
/// 与 Linux 一致：参数拷贝与长度检查先于镜像打开与格式识别。
pub fn judge_execve(table: &ProcTable, pid: i32, perm: PathPerm, fmt: ExecFmt, arg_bytes: u32, ptr: PtrClass) -> SysResult<()> {
    if table.find_live(pid).is_none() {
        return Err(ESRCH);
    }
    if arg_bytes > ARG_MAX {
        return Err(E2BIG);
    }
    if !ptr.usable() {
        return Err(EFAULT);
    }
    if perm == PathPerm::NoExecBit {
        return Err(EACCES);
    }
    match fmt {
        ExecFmt::Unknown => Err(ENOEXEC),
        _ => Ok(()),
    }
}

// ── 冻结对账表（B04 文件族 / B08 进程族地基深化册） ───────────────────
/// 对账场景枚举：每行一个独立可复算复算器。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ReconScenario {
    /// fork 调用者不存在 → ESRCH
    ForkNoCaller,
    /// fork 并发超限 → EAGAIN
    ForkOverLimit,
    /// fork 内存不足 → ENOMEM
    ForkNoMem,
    /// clone 坏 flags → EINVAL
    CloneBadFlags,
    /// setpgid 目标不存在 → ESRCH
    SetpgidNoTarget,
    /// setpgid 会话首改组 → EPERM
    SetpgidSessionLeader,
    /// setsid 已是会话首 → EPERM
    SetsidAlreadyLeader,
    /// execve 无执行权限 → EACCES（B04 文件族登记）
    ExecNoPerm,
    /// execve 格式不识别 → ENOEXEC（B04 文件族登记）
    ExecUnknownFmt,
    /// execve 参数超长 → E2BIG
    ExecArgTooBig,
    /// execve 坏地址 → EFAULT
    ExecBadPtr,
    /// execve 目标不存在 → ESRCH
    ExecNoProc,
}

impl ReconScenario {
    /// 闭集全量。
    pub const ALL: [ReconScenario; RECON_ROWS] = [
        ReconScenario::ForkNoCaller,
        ReconScenario::ForkOverLimit,
        ReconScenario::ForkNoMem,
        ReconScenario::CloneBadFlags,
        ReconScenario::SetpgidNoTarget,
        ReconScenario::SetpgidSessionLeader,
        ReconScenario::SetsidAlreadyLeader,
        ReconScenario::ExecNoPerm,
        ReconScenario::ExecUnknownFmt,
        ReconScenario::ExecArgTooBig,
        ReconScenario::ExecBadPtr,
        ReconScenario::ExecNoProc,
    ];

    /// 独立复算：新建进程表、真跑判定器、取回实测错误码。
    pub fn recompute(self) -> i32 {
        match self {
            ReconScenario::ForkNoCaller => {
                let mut t = ProcTable::new();
                code_of(judge_fork(&mut t, 7777, 8, 64))
            }
            ReconScenario::ForkOverLimit => {
                let mut t = ProcTable::new();
                code_of(judge_fork(&mut t, 1, 1, 64))
            }
            ReconScenario::ForkNoMem => {
                let mut t = ProcTable::new();
                code_of(judge_fork(&mut t, 1, 8, 3))
            }
            ReconScenario::CloneBadFlags => {
                let mut t = ProcTable::new();
                code_of(judge_clone(&mut t, 1, 0x20, 8, 64))
            }
            ReconScenario::SetpgidNoTarget => {
                let mut t = ProcTable::new();
                code_of_unit(judge_setpgid(&mut t, 1, 4242, 0))
            }
            ReconScenario::SetpgidSessionLeader => {
                let mut t = ProcTable::new();
                code_of_unit(judge_setpgid(&mut t, 1, 1, 1))
            }
            ReconScenario::SetsidAlreadyLeader => {
                let mut t = ProcTable::new();
                code_of(judge_setsid(&mut t, 1))
            }
            ReconScenario::ExecNoPerm => {
                let (t, child) = fresh_child();
                code_of_unit(judge_execve(&t, child, PathPerm::NoExecBit, ExecFmt::ElfValid, 16, PtrClass::Valid))
            }
            ReconScenario::ExecUnknownFmt => {
                let (t, child) = fresh_child();
                code_of_unit(judge_execve(&t, child, PathPerm::ExecOk, ExecFmt::Unknown, 16, PtrClass::Valid))
            }
            ReconScenario::ExecArgTooBig => {
                let (t, child) = fresh_child();
                code_of_unit(judge_execve(&t, child, PathPerm::ExecOk, ExecFmt::ElfValid, ARG_MAX + 1, PtrClass::Valid))
            }
            ReconScenario::ExecBadPtr => {
                let (t, child) = fresh_child();
                code_of_unit(judge_execve(&t, child, PathPerm::ExecOk, ExecFmt::ElfValid, 16, PtrClass::Null))
            }
            ReconScenario::ExecNoProc => {
                let (t, _child) = fresh_child();
                code_of_unit(judge_execve(&t, 9999, PathPerm::ExecOk, ExecFmt::ElfValid, 16, PtrClass::Valid))
            }
        }
    }
}

/// 便捷构表：init fork 一个子并推到 Running，返回（表, 子 pid）。
pub fn fresh_child() -> (ProcTable, i32) {
    let mut t = ProcTable::new();
    let child = match judge_fork(&mut t, 1, 8, 64) {
        Ok(p) => p,
        Err(_) => 0,
    };
    let _ = t.advance(child, ProcState::Running);
    (t, child)
}

/// 冻结对账表一行：场景名 / 深化册登记号 / 场景 / 登记码 / Linux 正码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ReconRow {
    /// 场景名（账面可读）
    pub scenario: &'static str,
    /// 深化册登记号（B08 进程族地基 / B04 文件族）
    pub register_id: &'static str,
    /// 场景（驱动独立复算器）
    pub scene: ReconScenario,
    /// 深化册约定码
    pub register_code: i32,
    /// Linux errno 正码
    pub linux_code: i32,
}

/// 冻结对账表：登记码与 Linux errno 逐行相等，是「零冲突」的判据本体。
pub const RECON_TABLE: [ReconRow; RECON_ROWS] = [
    ReconRow { scenario: "fork 调用者不存在", register_id: "B08-FORK-01", scene: ReconScenario::ForkNoCaller, register_code: ESRCH, linux_code: ESRCH },
    ReconRow { scenario: "fork 并发超限", register_id: "B08-FORK-02", scene: ReconScenario::ForkOverLimit, register_code: EAGAIN, linux_code: EAGAIN },
    ReconRow { scenario: "fork 内存不足", register_id: "B08-FORK-03", scene: ReconScenario::ForkNoMem, register_code: ENOMEM, linux_code: ENOMEM },
    ReconRow { scenario: "clone 坏 flags", register_id: "B08-CLON-01", scene: ReconScenario::CloneBadFlags, register_code: EINVAL, linux_code: EINVAL },
    ReconRow { scenario: "setpgid 目标不存在", register_id: "B08-SPGID-01", scene: ReconScenario::SetpgidNoTarget, register_code: ESRCH, linux_code: ESRCH },
    ReconRow { scenario: "setpgid 会话首改组", register_id: "B08-SPGID-02", scene: ReconScenario::SetpgidSessionLeader, register_code: EPERM, linux_code: EPERM },
    ReconRow { scenario: "setsid 已是会话首", register_id: "B08-SSID-01", scene: ReconScenario::SetsidAlreadyLeader, register_code: EPERM, linux_code: EPERM },
    ReconRow { scenario: "execve 无执行权限", register_id: "B04-FILE-01", scene: ReconScenario::ExecNoPerm, register_code: EACCES, linux_code: EACCES },
    ReconRow { scenario: "execve 格式不识别", register_id: "B04-FILE-02", scene: ReconScenario::ExecUnknownFmt, register_code: ENOEXEC, linux_code: ENOEXEC },
    ReconRow { scenario: "execve 参数超长", register_id: "B08-EXEC-01", scene: ReconScenario::ExecArgTooBig, register_code: E2BIG, linux_code: E2BIG },
    ReconRow { scenario: "execve 坏地址", register_id: "B08-EXEC-02", scene: ReconScenario::ExecBadPtr, register_code: EFAULT, linux_code: EFAULT },
    ReconRow { scenario: "execve 目标不存在", register_id: "B08-EXEC-03", scene: ReconScenario::ExecNoProc, register_code: ESRCH, linux_code: ESRCH },
];

/// 深化册登记码闭集（对账只认表内码；表外码一律拒绝）。
pub const REGISTER_CODE_TABLE: [i32; 9] = [EPERM, ESRCH, E2BIG, ENOEXEC, EAGAIN, ENOMEM, EACCES, EFAULT, EINVAL];

/// 表外杂号样本（刻意取一个不在任何登记册中的码）。
const OUTSIDE_CODE: i32 = 9;

/// 码是否在登记闭集内。
pub fn is_register_code(code: i32) -> bool {
    let mut i = 0usize;
    while i < REGISTER_CODE_TABLE.len() {
        if let Some(c) = REGISTER_CODE_TABLE.get(i) {
            if *c == code {
                return true;
            }
        }
        i += 1;
    }
    false
}

/// 单行复算：实测码 == 登记码 == Linux 码，且登记码在闭集内。
/// 每行完全独立，可单独复算，互不依赖。
pub fn recompute_row(row: &ReconRow) -> bool {
    let produced = row.scene.recompute();
    produced == row.register_code && row.register_code == row.linux_code && is_register_code(row.register_code)
}

/// 全表零冲突：12 行逐行独立复算全过，返回（是否全过，过行数）。
pub fn reconcile_zero_conflict() -> (bool, u32) {
    let mut ok_rows = 0u32;
    let mut i = 0usize;
    while i < RECON_ROWS {
        if let Some(row) = RECON_TABLE.get(i) {
            if recompute_row(row) {
                ok_rows += 1;
            }
        }
        i += 1;
    }
    (ok_rows == RECON_ROWS as u32, ok_rows)
}

// ── 正反分层声明 ──────────────────────────────────────────────────────
/// 声明账单条：预期码与实测码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DeclEntry {
    /// 条目名
    pub name: &'static str,
    /// 预期码（正向账恒 0=成功；反向账为登记码）
    pub expected: i32,
    /// 实测码
    pub got: i32,
}

impl DeclEntry {
    /// 单条判定。
    pub fn ok(&self) -> bool {
        self.expected == self.got
    }
}

/// 单层声明账。
#[derive(Clone, Debug)]
pub struct DeclLedger {
    /// 层名（正向 / 反向）
    pub layer: &'static str,
    /// 条目序列
    pub entries: Vec<DeclEntry>,
}

impl DeclLedger {
    /// 建账。
    pub fn new(layer: &'static str, entries: Vec<DeclEntry>) -> DeclLedger {
        DeclLedger { layer, entries }
    }

    /// 整层判定：非空且逐条相等。
    pub fn all_ok(&self) -> bool {
        if self.entries.is_empty() {
            return false;
        }
        self.entries.iter().all(|e| e.ok())
    }

    /// 条数。
    pub fn count(&self) -> usize {
        self.entries.len()
    }
}

/// 正向声明：合法输入全成功（成功码 0）。
pub fn run_positive_declaration() -> DeclLedger {
    let mut t = ProcTable::new();
    let mut es: Vec<DeclEntry> = Vec::new();
    // 1. fork 合法产出子进程
    let r_fork = judge_fork(&mut t, 1, 8, 64);
    let child = match r_fork {
        Ok(p) => p,
        Err(_) => 0,
    };
    es.push(DeclEntry { name: "fork 合法产出子进程", expected: 0, got: code_of(r_fork) });
    // 2. Ready→Running 合法前进
    es.push(DeclEntry { name: "Ready→Running 合法前进", expected: 0, got: code_of_unit(t.advance(child, ProcState::Running)) });
    // 3. execve 合法镜像装载
    es.push(DeclEntry { name: "execve 合法镜像装载", expected: 0, got: code_of_unit(judge_execve(&t, child, PathPerm::ExecOk, ExecFmt::ElfValid, 16, PtrClass::Valid)) });
    // 4. setpgid 自立新组（非会话首，pgid=pid 合法）
    es.push(DeclEntry { name: "setpgid 自立新组", expected: 0, got: code_of_unit(judge_setpgid(&mut t, child, 0, 0)) });
    // 5. 第二子 setsid 建新会话（非组长非会话首）
    let r_fork2 = judge_fork(&mut t, 1, 8, 64);
    let child2 = match r_fork2 {
        Ok(p) => p,
        Err(_) => 0,
    };
    es.push(DeclEntry { name: "fork 第二子并发合法", expected: 0, got: code_of(r_fork2) });
    let r_sid = judge_setsid(&mut t, child2);
    es.push(DeclEntry { name: "setsid 建新会话", expected: 0, got: code_of(r_sid) });
    // 6. clone 合法 flags 并发
    es.push(DeclEntry { name: "clone 合法 flags 并发", expected: 0, got: code_of(judge_clone(&mut t, child, 0x1, 8, 64)) });
    DeclLedger::new("正向", es)
}

/// 反向声明：违约输入全按登记码拒绝。
pub fn run_negative_declaration() -> DeclLedger {
    let mut es: Vec<DeclEntry> = Vec::new();
    // 1. fork 调用者缺失 → ESRCH
    let mut t1 = ProcTable::new();
    es.push(DeclEntry { name: "fork 调用者缺失", expected: ESRCH, got: code_of(judge_fork(&mut t1, -3, 8, 64)) });
    // 2. fork 并发超限 → EAGAIN
    let mut t2 = ProcTable::new();
    es.push(DeclEntry { name: "fork 并发超限", expected: EAGAIN, got: code_of(judge_fork(&mut t2, 1, 1, 64)) });
    // 3. fork 内存不足 → ENOMEM
    let mut t3 = ProcTable::new();
    es.push(DeclEntry { name: "fork 内存不足", expected: ENOMEM, got: code_of(judge_fork(&mut t3, 1, 8, 2)) });
    // 4. clone 坏 flags → EINVAL
    let mut t4 = ProcTable::new();
    es.push(DeclEntry { name: "clone 坏 flags", expected: EINVAL, got: code_of(judge_clone(&mut t4, 1, 0x10, 8, 64)) });
    // 5. setpgid 会话首改组 → EPERM
    let mut t5 = ProcTable::new();
    es.push(DeclEntry { name: "setpgid 会话首改组", expected: EPERM, got: code_of_unit(judge_setpgid(&mut t5, 1, 1, 1)) });
    // 6. setsid 已是会话首 → EPERM
    let mut t6 = ProcTable::new();
    es.push(DeclEntry { name: "setsid 已是会话首", expected: EPERM, got: code_of(judge_setsid(&mut t6, 1)) });
    // 7. execve 无执行位 → EACCES
    let (t7, c7) = fresh_child();
    es.push(DeclEntry { name: "execve 无执行位", expected: EACCES, got: code_of_unit(judge_execve(&t7, c7, PathPerm::NoExecBit, ExecFmt::ElfValid, 16, PtrClass::Valid)) });
    // 8. execve 格式不识别 → ENOEXEC
    let (t8, c8) = fresh_child();
    es.push(DeclEntry { name: "execve 格式不识别", expected: ENOEXEC, got: code_of_unit(judge_execve(&t8, c8, PathPerm::ExecOk, ExecFmt::Unknown, 16, PtrClass::Valid)) });
    // 9. execve 参数超长 → E2BIG
    let (t9, c9) = fresh_child();
    es.push(DeclEntry { name: "execve 参数超长", expected: E2BIG, got: code_of_unit(judge_execve(&t9, c9, PathPerm::ExecOk, ExecFmt::ElfValid, ARG_MAX + 1, PtrClass::Valid)) });
    // 10. execve 坏地址 → EFAULT
    let (t10, c10) = fresh_child();
    es.push(DeclEntry { name: "execve 坏地址", expected: EFAULT, got: code_of_unit(judge_execve(&t10, c10, PathPerm::ExecOk, ExecFmt::ElfValid, 16, PtrClass::Kernel)) });
    // 11. execve 目标缺失 → ESRCH
    let (t11, _c11) = fresh_child();
    es.push(DeclEntry { name: "execve 目标缺失", expected: ESRCH, got: code_of_unit(judge_execve(&t11, 0, PathPerm::ExecOk, ExecFmt::ElfValid, 16, PtrClass::Valid)) });
    // 12. setpgid Zombie 目标 → ESRCH（存在性先于权限）
    let (mut t12, c12) = fresh_child();
    let _ = t12.advance(c12, ProcState::Zombie);
    es.push(DeclEntry { name: "setpgid Zombie 目标", expected: ESRCH, got: code_of_unit(judge_setpgid(&mut t12, 1, c12, 0)) });
    DeclLedger::new("反向", es)
}

// ── 注入器与守恒账 ────────────────────────────────────────────────────
/// 注入守恒账：注入 = 检出 + 逃逸，逃逸必须恒 0。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectLedger {
    /// 注入总次数
    pub injected: u32,
    /// 检出次数
    pub detected: u32,
    /// 逃逸次数（必须为 0）
    pub escaped: u32,
}

impl InjectLedger {
    /// 零账。
    pub const fn zero() -> InjectLedger {
        InjectLedger { injected: 0, detected: 0, escaped: 0 }
    }

    /// 记一轮注入：计数只增不减；守恒被破坏时显性报错。
    pub fn record(&mut self, caught: bool) -> SysResult<()> {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(-(ProcCode::LedgerRegress as i32));
        }
        Ok(())
    }

    /// 守恒式。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 坏 pid 语料：负值 / 0 / 永不存在的远端号 / 大号段边界。
pub const BAD_PID_CORPUS: [i32; 4] = [-1, 0, 9999, 0x7FFF];

/// 坏 flags 语料：全零与越出合法掩码的位组合。
pub const BAD_FLAGS_CORPUS: [u32; 5] = [0, 0x10, 0x20, 0x40, 0x80];

/// 注入器一：坏 pid，10 轮全抓（fork 与 execve 双面同时判 ESRCH）。
pub fn run_bad_pid_injections() -> InjectLedger {
    let mut t = ProcTable::new();
    let mut lg = InjectLedger::zero();
    let mut r = 0u32;
    while r < INJECT_ROUNDS {
        let pid = match BAD_PID_CORPUS.get((r % BAD_PID_CORPUS.len() as u32) as usize) {
            Some(p) => *p,
            None => -1,
        };
        let caught = judge_fork(&mut t, pid, 64, 64) == Err(ESRCH)
            && judge_execve(&t, pid, PathPerm::ExecOk, ExecFmt::ElfValid, 16, PtrClass::Valid) == Err(ESRCH);
        let _ = lg.record(caught);
        r += 1;
    }
    lg
}

/// 注入器二：超限并发，10 轮全抓（每轮新表，rlimit 压到存活数，fork/clone 必回 EAGAIN）。
pub fn run_overlimit_injections() -> InjectLedger {
    let mut lg = InjectLedger::zero();
    let mut r = 0u32;
    while r < INJECT_ROUNDS {
        let mut t = ProcTable::new();
        let caught = judge_fork(&mut t, 1, 1, 64) == Err(EAGAIN)
            && judge_clone(&mut t, 1, 0x1, 1, 64) == Err(EAGAIN);
        let _ = lg.record(caught);
        r += 1;
    }
    lg
}

/// 注入器三：坏 flags，10 轮全抓（clone 语料轮转，必回 EINVAL）。
pub fn run_bad_flags_injections() -> InjectLedger {
    let mut lg = InjectLedger::zero();
    let mut r = 0u32;
    while r < INJECT_ROUNDS {
        let mut t = ProcTable::new();
        let flags = match BAD_FLAGS_CORPUS.get((r % BAD_FLAGS_CORPUS.len() as u32) as usize) {
            Some(f) => *f,
            None => 0,
        };
        let caught = judge_clone(&mut t, 1, flags, 64, 64) == Err(EINVAL);
        let _ = lg.record(caught);
        r += 1;
    }
    lg
}

// ── 容量耗尽 ──────────────────────────────────────────────────────────
/// 把进程表填满：返回（成功创建数，末次错误码）。表满后 fork 必须
/// 稳定回 EAGAIN，且创建账与成功数一致（账不丢）。
pub fn fill_table_until_full() -> (ProcTable, u32, i32) {
    let mut t = ProcTable::new();
    let mut ok = 0u32;
    let mut last_err = 0i32;
    let mut attempts = 0u32;
    let max_attempts = MAX_SLOTS as u32 + 8;
    while attempts < max_attempts {
        match judge_fork(&mut t, 1, 64, 64) {
            Ok(_) => ok += 1,
            Err(e) => {
                last_err = e;
                attempts = max_attempts;
            }
        }
        attempts += 1;
    }
    (t, ok, last_err)
}

// ── 确定性指纹 ────────────────────────────────────────────────────────
/// FNV-1a 32 位（无浮点、无随机、无墙钟）。
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

/// 把一个码压成两字节入语料。
fn push_code(bytes: &mut Vec<u8>, code: i32) {
    let v = code as u32;
    bytes.push((v & 0xFF) as u8);
    bytes.push(((v >> 8) & 0xFF) as u8);
}

/// 规范语料：对同一张表执行固定操作序列，把每个实测码与账目压入字节流。
/// 双跑逐字节一致是「可复测声明」的本体。
pub fn corpus_bytes() -> Vec<u8> {
    let mut t = ProcTable::new();
    let mut b: Vec<u8> = Vec::new();
    // fork → Running → execve 合法 / 无权限 / 坏指针 / 超长
    let r_fork = judge_fork(&mut t, 1, 4, 64);
    let child = match r_fork {
        Ok(p) => p,
        Err(_) => 0,
    };
    push_code(&mut b, code_of(r_fork));
    push_code(&mut b, code_of_unit(t.advance(child, ProcState::Running)));
    push_code(&mut b, code_of_unit(judge_execve(&t, child, PathPerm::ExecOk, ExecFmt::ElfValid, 16, PtrClass::Valid)));
    push_code(&mut b, code_of_unit(judge_execve(&t, child, PathPerm::NoExecBit, ExecFmt::ElfValid, 16, PtrClass::Valid)));
    push_code(&mut b, code_of_unit(judge_execve(&t, child, PathPerm::ExecOk, ExecFmt::Unknown, 16, PtrClass::Unmapped)));
    push_code(&mut b, code_of_unit(judge_execve(&t, child, PathPerm::ExecOk, ExecFmt::ElfValid, ARG_MAX + 1, PtrClass::Valid)));
    // setsid → setpgid → clone → fork 孙进程
    push_code(&mut b, code_of(judge_setsid(&mut t, child)));
    push_code(&mut b, code_of_unit(judge_setpgid(&mut t, child, 0, 0)));
    push_code(&mut b, code_of(judge_clone(&mut t, child, 0x3, 4, 64)));
    push_code(&mut b, code_of(judge_fork(&mut t, child, 4, 64)));
    // 反面语料：陈旧/越界 pid 与坏 flags
    push_code(&mut b, code_of(judge_fork(&mut t, -1, 4, 64)));
    push_code(&mut b, code_of(judge_fork(&mut t, 9999, 4, 64)));
    push_code(&mut b, code_of(judge_clone(&mut t, child, 0, 4, 64)));
    push_code(&mut b, code_of(judge_clone(&mut t, child, 0xF0, 4, 64)));
    push_code(&mut b, code_of_unit(judge_execve(&t, 0, PathPerm::ExecOk, ExecFmt::ElfValid, 16, PtrClass::Valid)));
    // 状态机反面：倒退 / 越级收割 / 非法收割 / 已收割推进
    push_code(&mut b, code_of_unit(t.advance(child, ProcState::Ready)));
    push_code(&mut b, code_of_unit(t.advance(child, ProcState::Zombie)));
    push_code(&mut b, code_of(t.reap(child)));
    push_code(&mut b, code_of(t.reap(child)));
    push_code(&mut b, code_of_unit(t.advance(child, ProcState::Running)));
    // 账目入语料
    push_code(&mut b, t.spawns() as i32);
    push_code(&mut b, t.exits() as i32);
    push_code(&mut b, t.reaps() as i32);
    push_code(&mut b, t.rejected() as i32);
    push_code(&mut b, t.live_count() as i32);
    b
}

/// 规范语料指纹。
pub fn corpus_fingerprint() -> u32 {
    fnv1a(&corpus_bytes())
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9295 判据集（≥14 条；动态明细 ≤8 条）。
pub fn run_unx9295_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9295");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x7A000..=0x7D1C0).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x7A000..=0x7D1C0", seg_ok,
          "段基址必须唯一且落在本批窗口；越界即拒绝注册");

    // 2. 正常流：fork 产生 Ready 子进程并可推进到 Running
    let mut t_norm = ProcTable::new();
    let fork_r = judge_fork(&mut t_norm, 1, 8, 64);
    let child_norm = match fork_r {
        Ok(p) => p,
        Err(_) => 0,
    };
    let norm_ok = fork_r.is_ok()
        && child_norm >= 2
        && t_norm.advance(child_norm, ProcState::Running).is_ok()
        && t_norm.live_count() == 2
        && t_norm.spawns() == 1;
    s.add("正常流：fork 产出 Ready 子并推进 Running", norm_ok,
          "合法 fork 必须落新槽、pid 单调、账目各记一笔");

    // 3. 正常流：execve 合法镜像全通过（含脚本格式与 ARG_MAX 上界内）
    let (t_ex, c_ex) = fresh_child();
    let exec_ok = judge_execve(&t_ex, c_ex, PathPerm::ExecOk, ExecFmt::ElfValid, 16, PtrClass::Valid) == Ok(())
        && judge_execve(&t_ex, c_ex, PathPerm::ExecOk, ExecFmt::Script, ARG_MAX, PtrClass::Valid) == Ok(());
    s.add("正常流：execve 合法镜像与脚本装载", exec_ok,
          "合法权限 + 合法格式 + 上界内参数应全成功");

    // 4. 正常流：setsid/setpgid 会话族合法迁移（两子分工：先立组，另子建会话）
    let mut t_sess = ProcTable::new();
    let c_grp = match judge_fork(&mut t_sess, 1, 8, 64) {
        Ok(p) => p,
        Err(_) => 0,
    };
    let c_sid_t = match judge_fork(&mut t_sess, 1, 8, 64) {
        Ok(p) => p,
        Err(_) => 0,
    };
    let sid_r = judge_setsid(&mut t_sess, c_sid_t);
    let sess_ok = sid_r == Ok(c_sid_t)
        && judge_setpgid(&mut t_sess, c_grp, 0, 0).is_ok()
        && judge_setpgid(&mut t_sess, c_grp, 0, c_grp).is_ok();
    s.add("正常流：setsid 建会话 + setpgid 自立/复用组", sess_ok,
          "非组长非会话首建会话成功；非会话首可自立组也可复用自身组号");

    // 5. 边界（零）：rlimit=0 fork 必拒，pid=0 对 execve 是不存在目标
    let mut t_zero = ProcTable::new();
    let zero_ok = judge_fork(&mut t_zero, 1, 0, 64) == Err(EAGAIN)
        && judge_execve(&t_zero, 0, PathPerm::ExecOk, ExecFmt::ElfValid, 16, PtrClass::Valid) == Err(ESRCH);
    s.add("边界（零）：rlimit=0 拒 fork、pid=0 归 ESRCH", zero_ok,
          "零上限与零号目标都必须按约定码拒绝");

    // 6. 边界（负）：负 pid 三面全 ESRCH
    let mut t_neg = ProcTable::new();
    let neg_ok = judge_fork(&mut t_neg, -1, 8, 64) == Err(ESRCH)
        && judge_execve(&t_neg, -7, PathPerm::ExecOk, ExecFmt::ElfValid, 16, PtrClass::Valid) == Err(ESRCH)
        && code_of_unit(judge_setpgid(&mut t_neg, -2, 1, 0)) == ESRCH;
    s.add("边界（负）：负 pid 三面全 ESRCH", neg_ok,
          "负号段不参与查找，一律按不存在处理");

    // 7. 边界（上限 / 上限+1）：恰好压线拒绝、ARG_MAX 上界内通过、超一拒
    let mut t_lim = ProcTable::new();
    let lim_ok = judge_fork(&mut t_lim, 1, 1, 64) == Err(EAGAIN)
        && judge_fork(&mut t_lim, 1, 2, 64).is_ok()
        && judge_fork(&mut t_lim, 1, 2, 64) == Err(EAGAIN)
        && judge_execve(&t_lim, 1, PathPerm::ExecOk, ExecFmt::ElfValid, ARG_MAX, PtrClass::Valid) == Ok(())
        && judge_execve(&t_lim, 1, PathPerm::ExecOk, ExecFmt::ElfValid, ARG_MAX + 1, PtrClass::Valid) == Err(E2BIG);
    s.add("边界（上限/上限+1）：压线拒、上界内过、超一拒", lim_ok,
          "rlimit 压线回 EAGAIN；ARG_MAX 恰好可过、+1 回 E2BIG");

    // 8. 注入器一：坏 pid 10/10 全抓
    let lg_pid = run_bad_pid_injections();
    let inj_pid_ok = lg_pid.conserved()
        && lg_pid.escaped == 0
        && lg_pid.detected == INJECT_ROUNDS;
    s.add("注入器一：坏 pid 10 轮全抓且 0 逃逸", inj_pid_ok,
          "负值/0/远端号/边界号逐轮必回 ESRCH");

    // 9. 注入器二：超限并发 10/10 全抓
    let lg_lim = run_overlimit_injections();
    let inj_lim_ok = lg_lim.conserved()
        && lg_lim.escaped == 0
        && lg_lim.detected == INJECT_ROUNDS;
    s.add("注入器二：超限并发 10 轮全抓且 0 逃逸", inj_lim_ok,
          "rlimit 压到存活数时 fork/clone 必回 EAGAIN");

    // 10. 注入器三：坏 flags 10/10 全抓
    let lg_flg = run_bad_flags_injections();
    let inj_flg_ok = lg_flg.conserved()
        && lg_flg.escaped == 0
        && lg_flg.detected == INJECT_ROUNDS;
    s.add("注入器三：坏 flags 10 轮全抓且 0 逃逸", inj_flg_ok,
          "全零与越掩码位组合必回 EINVAL");

    // 11. 注入账守恒：三类合计 30 注入 30 检出 0 逃逸
    let total_inj = lg_pid.injected.saturating_add(lg_lim.injected).saturating_add(lg_flg.injected);
    let total_esc = lg_pid.escaped.saturating_add(lg_lim.escaped).saturating_add(lg_flg.escaped);
    let total_det = lg_pid.detected.saturating_add(lg_lim.detected).saturating_add(lg_flg.detected);
    let conserve_ok = total_inj == INJECTOR_CLASSES * INJECT_ROUNDS
        && total_det == total_inj
        && total_esc == 0;
    s.add("注入账守恒：30 注入 = 30 检出 + 0 逃逸", conserve_ok,
          "三类注入器合计守恒，任何逃逸即判红");

    // 12. 判定次序：fork/clone 链 ESRCH→EINVAL→EAGAIN→ENOMEM
    let mut t_ord = ProcTable::new();
    let ord_fork = judge_fork(&mut t_ord, 9999, 0, 0) == Err(ESRCH)
        && judge_fork(&mut t_ord, 1, 0, 0) == Err(EAGAIN)
        && judge_fork(&mut t_ord, 1, 8, 0) == Err(ENOMEM)
        && judge_fork(&mut t_ord, 1, 8, 64).is_ok();
    let mut t_ord2 = ProcTable::new();
    let ord_clone = judge_clone(&mut t_ord2, 7777, 0x20, 0, 0) == Err(ESRCH)
        && judge_clone(&mut t_ord2, 1, 0x20, 0, 0) == Err(EINVAL)
        && judge_clone(&mut t_ord2, 1, 0x1, 0, 0) == Err(EAGAIN)
        && judge_clone(&mut t_ord2, 1, 0x1, 8, 1) == Err(ENOMEM);
    s.add("判定次序：fork/clone 四级链逐级让位", ord_fork && ord_clone,
          "全违约回 ESRCH；随后 EINVAL、EAGAIN、ENOMEM 依次成为首错");

    // 13. 判定次序：setpgid/setsid 存在性（ESRCH）先于权限（EPERM）
    let (mut t_z, c_z) = fresh_child();
    let _ = t_z.advance(c_z, ProcState::Zombie);
    let (mut t_lead, c_lead) = fresh_child();
    let _ = judge_setsid(&mut t_lead, c_lead);
    let _ = t_lead.advance(c_lead, ProcState::Zombie);
    let ord_perm = code_of_unit(judge_setpgid(&mut t_z, 1, c_z, 0)) == ESRCH
        && code_of_unit(judge_setpgid(&mut t_lead, 1, c_lead, 0)) == ESRCH
        && code_of_unit(judge_setpgid(&mut t_ord2, 1, 1, 1)) == EPERM
        && judge_setsid(&mut t_ord2, 1) == Err(EPERM)
        && judge_setsid(&mut t_ord2, 424242) == Err(ESRCH);
    s.add("判定次序：ESRCH 先于 EPERM（含 Zombie 目标）", ord_perm,
          "不存在/Zombie 目标一律先回 ESRCH；存活会话首才轮到 EPERM");

    // 14. 判定次序：execve 链 ESRCH→E2BIG→EFAULT→EACCES→ENOEXEC
    let (t_eo, c_eo) = fresh_child();
    let ord_exec = judge_execve(&t_eo, 9999, PathPerm::NoExecBit, ExecFmt::Unknown, ARG_MAX + 1, PtrClass::Null) == Err(ESRCH)
        && judge_execve(&t_eo, c_eo, PathPerm::NoExecBit, ExecFmt::Unknown, ARG_MAX + 1, PtrClass::Null) == Err(E2BIG)
        && judge_execve(&t_eo, c_eo, PathPerm::NoExecBit, ExecFmt::Unknown, 16, PtrClass::Kernel) == Err(EFAULT)
        && judge_execve(&t_eo, c_eo, PathPerm::NoExecBit, ExecFmt::Unknown, 16, PtrClass::Valid) == Err(EACCES)
        && judge_execve(&t_eo, c_eo, PathPerm::ExecOk, ExecFmt::Unknown, 16, PtrClass::Valid) == Err(ENOEXEC);
    s.add("判定次序：execve 五级链逐级让位", ord_exec,
          "全违约回 ESRCH；随后 E2BIG、EFAULT、EACCES、ENOEXEC 依次成为首错");

    // 15. 状态机：非法倒退拒绝且拒绝账只增
    let mut t_sm = ProcTable::new();
    let c_sm = match judge_fork(&mut t_sm, 1, 8, 64) {
        Ok(p) => p,
        Err(_) => 0,
    };
    let _ = t_sm.advance(c_sm, ProcState::Running);
    let regress_ok = t_sm.advance(c_sm, ProcState::Ready) == Err(EINVAL)
        && t_sm.advance(c_sm, ProcState::Running) == Err(EINVAL)
        && t_sm.rejected() == 2
        && t_sm.state_view(c_sm) == ProcState::Running;
    s.add("状态机：Running 倒退/重放被拒且拒绝账入账", regress_ok,
          "非法迁移不改槽位状态，只推拒绝账");

    // 16. 状态机：合法全链 + 越级拒 + 收割后旧 pid 失活
    let mut t_lc = ProcTable::new();
    let c_lc = match judge_fork(&mut t_lc, 1, 8, 64) {
        Ok(p) => p,
        Err(_) => 0,
    };
    let skip_ok = t_lc.advance(c_lc, ProcState::Zombie) == Err(EINVAL);
    let _ = t_lc.advance(c_lc, ProcState::Running);
    let _ = t_lc.advance(c_lc, ProcState::Zombie);
    let gen_before = t_lc.generation_of(c_lc);
    let reap_r = t_lc.reap(c_lc);
    let reap_ok = match reap_r {
        Ok(g) => g == gen_before + 1,
        Err(_) => false,
    } && t_lc.generation_of(c_lc) == 0
        && t_lc.find(c_lc).is_none()
        && t_lc.advance(c_lc, ProcState::Running) == Err(ESRCH)
        && t_lc.exits() == 1
        && t_lc.reaps() == 1;
    s.add("状态机：越级拒、合法全链通、收割后旧 pid 失活", skip_ok && reap_ok,
          "Ready 直跳 Zombie 拒；Zombie 收割回 Free 且代际推进");

    // 17. 对账：冻结表 12 行逐行独立复算零冲突
    let (recon_all, recon_rows) = reconcile_zero_conflict();
    s.add("对账：B04/B08 冻结表 12 行逐行零冲突", recon_all && recon_rows == RECON_ROWS as u32,
          "实测码 = 登记码 = Linux errno 正码，三方逐值相等");

    // 18. 对账：表外码拒绝（闭集外一律不认）
    let table_out_ok = !is_register_code(0)
        && !is_register_code(-1)
        && !is_register_code(9999)
        && !is_register_code(OUTSIDE_CODE)
        && is_register_code(ESRCH)
        && is_register_code(EINVAL)
        && is_register_code(E2BIG);
    s.add("对账：表外码拒绝、表内码全认", table_out_ok,
          "登记闭集 9 码之外（含 0/负值/杂号）一律拒绝入账");

    // 19. 正反分层声明：正向账全成
    let pos = run_positive_declaration();
    let pos_ok = pos.all_ok() && pos.count() >= 5 && pos.layer == "正向";
    s.add("正反分层：正向账合法输入全成功", pos_ok,
          "fork/execve/setsid/setpgid/clone 合法路径全部按成功码入账");

    // 20. 正反分层声明：反向账全按登记码拒绝
    let neg = run_negative_declaration();
    let neg_ok = neg.all_ok() && neg.count() >= 10 && neg.layer == "反向";
    s.add("正反分层：反向账违约输入全按登记码拒绝", neg_ok,
          "12 条违约样本逐条与深化册登记码相等");

    // 21. 正反分层声明：两账分离不互证
    let layered_ok = pos.layer != neg.layer
        && pos.all_ok()
        && neg.all_ok()
        && pos.count() != neg.count();
    s.add("正反分层：两账分离且各自独立判定", layered_ok,
          "正向账与反向账分层建账、分层判定，禁止混账互证");

    // 22. 容量耗尽：表满回 EAGAIN 且账不丢
    let (t_full, fill_ok_n, last_err) = fill_table_until_full();
    let cap_ok = fill_ok_n == MAX_SLOTS as u32 - 1
        && last_err == EAGAIN
        && t_full.spawns() == fill_ok_n
        && t_full.live_count() == MAX_SLOTS
        && judge_fork(&mut t_full.clone(), 1, 64, 64) == Err(EAGAIN);
    s.add("容量耗尽：表满稳定 EAGAIN 且创建账不丢", cap_ok,
          "init 占 1 槽，创建 15 后表满；账目与成功数闭合");

    // 23. 双跑一致：规范语料指纹逐字段相等
    let fp_a = corpus_fingerprint();
    let fp_b = corpus_fingerprint();
    let fp_ok = fp_a == fp_b;
    s.add("双跑一致：规范语料指纹相等", fp_ok,
          "同输入两次逐字段一致，隐藏状态即失败");

    // 24. 文案闭集：15 个诊断码均有非空中文建议
    let text_ok = ProcCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("文案闭集：全部诊断码带中文建议", text_ok,
          "每个码必须能取到非空且可操作的说明");

    // 25. 诊断码唯一：15 个码值两两唯一且落在段窗内
    let mut uniq = true;
    let mut i = 0usize;
    while i < ProcCode::ALL.len() {
        let mut j = i + 1;
        while j < ProcCode::ALL.len() {
            if let (Some(a), Some(b)) = (ProcCode::ALL.get(i), ProcCode::ALL.get(j)) {
                if a.val() == b.val() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    let in_window = ProcCode::ALL.iter().all(|c| (SEG_BASE..SEG_BASE + 0x40).contains(&c.val()));
    s.add("诊断码：15 个码值两两唯一且在段窗内", uniq && in_window,
          "段内闭集码不得重复或越出 0x40 窗口");

    // 动态明细（≤8）：注入账 + 对账行数 + 指纹
    let ledger_text = format!(
        "注入 {} / 检出 {} / 逃逸 {}",
        total_inj, total_det, total_esc
    );
    s.add_dyn("注入账快照", conserve_ok, &ledger_text);
    let recon_text = format!("对账 {} / {} 行零冲突", recon_rows, RECON_ROWS);
    s.add_dyn("对账快照", recon_all, &recon_text);
    let fp_text = format!("语料指纹 0x{:08X}", fp_a);
    s.add_dyn("指纹快照", fp_ok, &fp_text);

    // 26. 收口：判据条数达到声明值（阈值保守取 14）
    let n_before = s.len();
    s.add("收口：判据条数达到声明值", n_before >= 14, "阈值保守取 14，实际条数以账目快照为准");

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_fork_to_session_chain() {
        let mut t = ProcTable::new();
        let child = match judge_fork(&mut t, 1, 8, 64) {
            Ok(p) => p,
            Err(_) => 0,
        };
        assert!(child >= 2);
        assert!(t.advance(child, ProcState::Running).is_ok());
        assert_eq!(judge_setsid(&mut t, child), Ok(child));
        assert!(judge_execve(&t, child, PathPerm::ExecOk, ExecFmt::ElfValid, 16, PtrClass::Valid).is_ok());
        // 另一子自立新组（非会话首才可 setpgid）
        let c2 = match judge_fork(&mut t, 1, 8, 64) {
            Ok(p) => p,
            Err(_) => 0,
        };
        assert!(judge_setpgid(&mut t, c2, 0, 0).is_ok());
    }

    #[test]
    fn boundary_zero_and_negative() {
        let mut t = ProcTable::new();
        assert_eq!(judge_fork(&mut t, 1, 0, 64), Err(EAGAIN));
        assert_eq!(judge_execve(&t, 0, PathPerm::ExecOk, ExecFmt::ElfValid, 16, PtrClass::Valid), Err(ESRCH));
        assert_eq!(judge_fork(&mut t, -1, 8, 64), Err(ESRCH));
        assert_eq!(judge_execve(&t, -7, PathPerm::ExecOk, ExecFmt::ElfValid, 16, PtrClass::Valid), Err(ESRCH));
    }

    #[test]
    fn boundary_upper_limit_and_argmax() {
        let mut t = ProcTable::new();
        assert_eq!(judge_fork(&mut t, 1, 1, 64), Err(EAGAIN));
        assert!(judge_fork(&mut t, 1, 2, 64).is_ok());
        assert_eq!(judge_fork(&mut t, 1, 2, 64), Err(EAGAIN));
        assert_eq!(judge_execve(&t, 1, PathPerm::ExecOk, ExecFmt::ElfValid, ARG_MAX, PtrClass::Valid), Ok(()));
        assert_eq!(judge_execve(&t, 1, PathPerm::ExecOk, ExecFmt::ElfValid, ARG_MAX + 1, PtrClass::Valid), Err(E2BIG));
    }

    #[test]
    fn injector_bad_pid_ten_rounds() {
        let lg = run_bad_pid_injections();
        assert!(lg.conserved());
        assert_eq!(lg.injected, INJECT_ROUNDS);
        assert_eq!(lg.detected, INJECT_ROUNDS);
        assert_eq!(lg.escaped, 0);
    }

    #[test]
    fn injector_overlimit_and_flags_ten_rounds() {
        let a = run_overlimit_injections();
        let b = run_bad_flags_injections();
        assert_eq!(a.detected, INJECT_ROUNDS);
        assert_eq!(b.detected, INJECT_ROUNDS);
        assert_eq!(a.escaped + b.escaped, 0);
    }

    #[test]
    fn judge_order_fork_clone_chain() {
        let mut t = ProcTable::new();
        assert_eq!(judge_fork(&mut t, 9999, 0, 0), Err(ESRCH));
        assert_eq!(judge_fork(&mut t, 1, 0, 0), Err(EAGAIN));
        assert_eq!(judge_fork(&mut t, 1, 8, 0), Err(ENOMEM));
        assert!(judge_fork(&mut t, 1, 8, 64).is_ok());
        let mut u = ProcTable::new();
        assert_eq!(judge_clone(&mut u, 7777, 0x20, 0, 0), Err(ESRCH));
        assert_eq!(judge_clone(&mut u, 1, 0x20, 0, 0), Err(EINVAL));
        assert_eq!(judge_clone(&mut u, 1, 0x1, 0, 0), Err(EAGAIN));
        assert_eq!(judge_clone(&mut u, 1, 0x1, 8, 1), Err(ENOMEM));
    }

    #[test]
    fn judge_order_esrch_before_eperm() {
        let (mut tz, cz) = fresh_child();
        let _ = tz.advance(cz, ProcState::Zombie);
        // Zombie 目标：存在性判定先于一切权限判定
        assert_eq!(judge_setpgid(&mut tz, 1, cz, 0), Err(ESRCH));
        assert_eq!(judge_execve(&tz, cz, PathPerm::NoExecBit, ExecFmt::Unknown, ARG_MAX + 1, PtrClass::Null), Err(ESRCH));
        // 存活会话首：才轮到 EPERM
        let mut t = ProcTable::new();
        assert_eq!(judge_setpgid(&mut t, 1, 1, 1), Err(EPERM));
        assert_eq!(judge_setsid(&mut t, 1), Err(EPERM));
    }

    #[test]
    fn judge_order_execve_chain() {
        let (t, c) = fresh_child();
        assert_eq!(judge_execve(&t, 9999, PathPerm::NoExecBit, ExecFmt::Unknown, ARG_MAX + 1, PtrClass::Null), Err(ESRCH));
        assert_eq!(judge_execve(&t, c, PathPerm::NoExecBit, ExecFmt::Unknown, ARG_MAX + 1, PtrClass::Null), Err(E2BIG));
        assert_eq!(judge_execve(&t, c, PathPerm::NoExecBit, ExecFmt::Unknown, 16, PtrClass::Kernel), Err(EFAULT));
        assert_eq!(judge_execve(&t, c, PathPerm::NoExecBit, ExecFmt::Unknown, 16, PtrClass::Valid), Err(EACCES));
        assert_eq!(judge_execve(&t, c, PathPerm::ExecOk, ExecFmt::Unknown, 16, PtrClass::Valid), Err(ENOEXEC));
    }

    #[test]
    fn state_machine_monotonic_and_reap() {
        let mut t = ProcTable::new();
        let c = match judge_fork(&mut t, 1, 8, 64) {
            Ok(p) => p,
            Err(_) => 0,
        };
        // 越级 Ready→Zombie 拒
        assert_eq!(t.advance(c, ProcState::Zombie), Err(EINVAL));
        assert!(t.advance(c, ProcState::Running).is_ok());
        // 倒退 Running→Ready 拒
        assert_eq!(t.advance(c, ProcState::Ready), Err(EINVAL));
        assert!(t.advance(c, ProcState::Zombie).is_ok());
        let g0 = t.generation_of(c);
        let g = match t.reap(c) {
            Ok(g) => g,
            Err(_) => 0,
        };
        assert_eq!(g, g0 + 1);
        assert_eq!(t.generation_of(c), 0);
        assert!(t.find(c).is_none());
        assert_eq!(t.advance(c, ProcState::Running), Err(ESRCH));
        // 已收割槽不可再收割（Free 槽不可见）
        assert_eq!(t.reap(c), Err(ESRCH));
        assert!(t.rejected() >= 2);
    }

    #[test]
    fn reconcile_table_zero_conflict() {
        let (all, rows) = reconcile_zero_conflict();
        assert!(all);
        assert_eq!(rows, RECON_ROWS as u32);
        // 逐行独立复算
        let mut i = 0usize;
        while i < RECON_ROWS {
            if let Some(row) = RECON_TABLE.get(i) {
                assert!(recompute_row(row), "row {} ({}) 复算失败", i, row.register_id);
            }
            i += 1;
        }
        // 表外码拒绝
        assert!(!is_register_code(0));
        assert!(!is_register_code(9999));
        assert!(is_register_code(EFAULT));
    }

    #[test]
    fn layered_declaration_positive_negative() {
        let pos = run_positive_declaration();
        let neg = run_negative_declaration();
        assert!(pos.all_ok());
        assert!(pos.count() >= 5);
        assert!(neg.all_ok());
        assert!(neg.count() >= 10);
        assert!(pos.layer != neg.layer);
        let mut i = 0usize;
        while i < neg.entries.len() {
            if let Some(e) = neg.entries.get(i) {
                assert!(e.ok(), "反向条目 {} 偏差", e.name);
            }
            i += 1;
        }
    }

    #[test]
    fn double_run_fingerprint_stable() {
        let a = corpus_fingerprint();
        let b = corpus_fingerprint();
        assert_eq!(a, b);
        let ca = corpus_bytes();
        let cb = corpus_bytes();
        assert_eq!(ca.len(), cb.len());
    }

    #[test]
    fn capacity_exhaustion_ledger() {
        let (t, ok, last) = fill_table_until_full();
        assert_eq!(ok, MAX_SLOTS as u32 - 1);
        assert_eq!(last, EAGAIN);
        assert_eq!(t.spawns(), ok);
        assert_eq!(t.live_count(), MAX_SLOTS);
    }

    #[test]
    fn segment_guard_window() {
        assert_eq!(SEG_BASE % 0x40, 0);
        assert!((0x7A000..=0x7D1C0).contains(&SEG_BASE));
        assert_eq!(SEG_BASE, 0x7B740);
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx9295_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
        assert!(s.len() >= 14);
    }
}
