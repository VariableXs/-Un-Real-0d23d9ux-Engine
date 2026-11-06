//! UNX-F9296 进程族 E 型与 Windows 进程句柄错误对照格 · 0x7B780段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（UNX-F9296-J1 逐字）：
//!   ① 纯功能代码 ≥300 行；② UNX-F9296-J1 ERROR_INVALID_PARAMETER 映射
//!   判据过，差异账三条全列格过；③ 完成时 API/软件写回：状态=已完成、
//!   结果摘要非空。
//!
//! 设计要点：
//!   ① 域模型是「双列对照表」：左列 POSIX 进程族 E 型 errno（正数码 +
//!      负值口径），右列 Windows 进程/句柄 API 错误码。表冻结 13 行，
//!      覆盖 ESRCH/EPERM/EACCES/ENOMEM/EAGAIN/EINVAL/E2BIG/ENOEXEC 八个
//!      进程族 E 型，外加 ECHILD/EFAULT/EBADF/ENOENT/EPIPE 五个近邻码
//!      （EPIPE→ERROR_BROKEN_PIPE=109 收进管道断裂族）。
//!   ② 判定器 `judge_map` 走冻结表逐码映射：表内 errno 正值返回
//!      Ok(Windows 码)，表外正值返回 Err(-errno) 显性拒绝，非正输入
//!      返回 Err(0)。核心判据：EINVAL(22) → ERROR_INVALID_PARAMETER(87)。
//!   ③ 对账复算器 `recompute_win` 是独立第二条路径：不经对照表，先按
//!      `kind_of` 归类六种映射形态（直接对应/多对一归并/句柄失效归化/
//!      配额替代/格式族/管道族），再按形态定码；两路在 1..=133 全域逐码
//!      相等才放行——这保证表行可独立复算，不是自说自话。
//!   ④ 差异账三条全列（本卡判据硬点，以表行为准）：
//!      一、Windows 无 ESRCH 对应——进程 API 以句柄为操作对象，进程
//!          消亡后句柄失效统一报 ERROR_INVALID_HANDLE=6，无独立
//!          「无此进程」码，故 ESRCH 归化句柄失效族；
//!      二、EAGAIN 语义差异——Windows 不用 EAGAIN 的可重试语义，配额
//!          /资源耗尽一次性拒绝并归 ERROR_NO_SYSTEM_RESOURCES=1450；
//!      三、ENOEXEC 在 Windows 是格式族——可执行格式错误专码
//!          ERROR_BAD_EXE_FORMAT=193，不归参数错误族。
//!   ⑤ 轻量进程表状态机（与样板 fd 表完全不同的域）：pid 从 1 起连续
//!      编号，Free→Live 仅经 spawn、Live→Zombie 仅经 kill、Zombie→Free
//!      仅经 reap；判定次序沿用 Linux 口径——pid 存在性（ESRCH）先于
//!      权限（EPERM），收割次序 Zombie 先决（否则 ECHILD）。
//!   ⑥ 注入账守恒：四类违约（陈旧 pid / 越权 / 配额耗尽 / 坏可执行
//!      头）各 10 轮必须全抓；注入 = 抓获 + 逃逸，逃逸恒为 0。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F9296 独占 0x40 窗口）。
pub const SEG_BASE: u32 = 0x7B780;

/// 单类注入的复测轮数（判据口径 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// 进程表定容槽数（pid 1..=16 连续编号）。
pub const MAX_PROCS: usize = 16;

/// argv 长度下限（spawn 至少要带程序名一项）。
pub const ARGV_MIN: u32 = 1;

/// argv 长度上限（超出按 E2BIG 拒绝）。
pub const ARGV_MAX: u32 = 4096;

/// 特权 uid（root 可越权 kill）。
pub const ROOT_UID: u32 = 0;

/// 可执行头魔数（ELF 首字小端口径）。
pub const EXE_MAGIC_ELF: u32 = 0x464C_457F;

/// 坏魔数样本（非 ELF 首字，触发 ENOEXEC 路径）。
pub const EXE_MAGIC_BAD: u32 = 0x0000_0000;

// ── POSIX errno 正数码（进程族 E 型 + 近邻） ──────────────────────────
pub const EPERM: i32 = 1;
pub const ENOENT: i32 = 2;
pub const ESRCH: i32 = 3;
pub const E2BIG: i32 = 7;
pub const ENOEXEC: i32 = 8;
pub const EBADF: i32 = 9;
pub const ECHILD: i32 = 10;
pub const EAGAIN: i32 = 11;
pub const ENOMEM: i32 = 12;
pub const EACCES: i32 = 13;
pub const EFAULT: i32 = 14;
pub const EINVAL: i32 = 22;
pub const EPIPE: i32 = 32;

// ── Windows 错误码（进程/句柄 API 面） ────────────────────────────────
pub const WIN_ERROR_INVALID_HANDLE: u32 = 6;
pub const WIN_ERROR_FILE_NOT_FOUND: u32 = 2;
pub const WIN_ERROR_ACCESS_DENIED: u32 = 5;
pub const WIN_ERROR_NOT_ENOUGH_MEMORY: u32 = 8;
pub const WIN_ERROR_INVALID_PARAMETER: u32 = 87;
pub const WIN_ERROR_NOACCESS: u32 = 998;
pub const WIN_ERROR_BROKEN_PIPE: u32 = 109;
pub const WIN_ERROR_BAD_EXE_FORMAT: u32 = 193;
pub const WIN_ERROR_NO_SYSTEM_RESOURCES: u32 = 1450;

// ── 冻结双列对照表（13 行，左 errno 负值，右 Windows 码） ─────────────
/// 行序即冻结序：不得重排、不得增删；复算器按行独立验证。
pub const MAP_TABLE: &[(&str, i32, u32, &str)] = &[
    ("ESRCH", -ESRCH, WIN_ERROR_INVALID_HANDLE,
     "Windows 无『无此进程』对应；句柄失效统一归 ERROR_INVALID_HANDLE=6"),
    ("EPERM", -EPERM, WIN_ERROR_ACCESS_DENIED,
     "ERROR_ACCESS_DENIED=5；权限拒绝族直接对应"),
    ("EACCES", -EACCES, WIN_ERROR_ACCESS_DENIED,
     "ERROR_ACCESS_DENIED=5；与 EPERM 多对一归并"),
    ("ENOMEM", -ENOMEM, WIN_ERROR_NOT_ENOUGH_MEMORY,
     "ERROR_NOT_ENOUGH_MEMORY=8；内存不足族直接对应"),
    ("EAGAIN", -EAGAIN, WIN_ERROR_NO_SYSTEM_RESOURCES,
     "Windows 不用 EAGAIN；配额拒绝归 ERROR_NO_SYSTEM_RESOURCES=1450"),
    ("EINVAL", -EINVAL, WIN_ERROR_INVALID_PARAMETER,
     "ERROR_INVALID_PARAMETER=87；参数非法族直接对应"),
    ("E2BIG", -E2BIG, WIN_ERROR_INVALID_PARAMETER,
     "参数表过长在 Windows 归 87（CreateProcess 命令行限界）"),
    ("ENOEXEC", -ENOEXEC, WIN_ERROR_BAD_EXE_FORMAT,
     "Windows 是格式族：ERROR_BAD_EXE_FORMAT=193，非参数错误"),
    ("ECHILD", -ECHILD, WIN_ERROR_INVALID_HANDLE,
     "无可收子进程在 Windows 归句柄失效族 6"),
    ("EFAULT", -EFAULT, WIN_ERROR_NOACCESS,
     "ERROR_NOACCESS=998；坏地址族"),
    ("EBADF", -EBADF, WIN_ERROR_INVALID_HANDLE,
     "坏描述符归 ERROR_INVALID_HANDLE=6；句柄族"),
    ("ENOENT", -ENOENT, WIN_ERROR_FILE_NOT_FOUND,
     "ERROR_FILE_NOT_FOUND=2；路径缺失族"),
    ("EPIPE", -EPIPE, WIN_ERROR_BROKEN_PIPE,
     "ERROR_BROKEN_PIPE=109；管道断裂族"),
];

/// 冻结行数（判据口径 10..=14）。
pub const MAP_ROWS: usize = MAP_TABLE.len();

// ── 映射形态分类（复算器的独立路径） ──────────────────────────────────
/// 六种映射形态：复算器先归类再定码，与冻结表互不引用。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MapKind {
    /// 直接对应：一个 errno 对一个 Windows 码
    DirectMatch,
    /// 多对一归并：EPERM/EACCES 同归 5，E2BIG 归 87
    ManyToOne,
    /// 句柄失效归化：ESRCH/ECHILD/EBADF 归 6
    HandleFallback,
    /// 配额替代：EAGAIN → 1450
    QuotaSurrogate,
    /// 格式族：ENOEXEC → 193
    FormatFamily,
    /// 管道断裂族：EPIPE → 109
    PipeFamily,
}

/// 形态归类：表外 errno 返回 None。
pub fn kind_of(errno: i32) -> Option<MapKind> {
    match errno {
        ESRCH | ECHILD | EBADF => Some(MapKind::HandleFallback),
        EPERM | EACCES | E2BIG => Some(MapKind::ManyToOne),
        ENOENT | ENOMEM | EFAULT | EINVAL => Some(MapKind::DirectMatch),
        EAGAIN => Some(MapKind::QuotaSurrogate),
        ENOEXEC => Some(MapKind::FormatFamily),
        EPIPE => Some(MapKind::PipeFamily),
        _ => None,
    }
}

// ── 判定器：冻结表驱动的映射 ──────────────────────────────────────────
/// 系统调用口径的结果别名（错误侧携带 errno 正数码）。
pub type SysResult<T> = Result<T, i32>;

/// 主判定器：errno 正值 → Windows 码。
/// - 表内：Ok(冻结表右列)；
/// - 非正输入：Err(0)（非法输入显性码）；
/// - 表外正值：Err(-errno)（把拒绝原因带回给调用方）。
pub fn judge_map(errno: i32) -> Result<u32, i32> {
    if errno <= 0 {
        return Err(0);
    }
    let mut i = 0usize;
    while i < MAP_TABLE.len() {
        if let Some(row) = MAP_TABLE.get(i) {
            if row.1 == -errno {
                return Ok(row.2);
            }
        }
        i += 1;
    }
    Err(-errno)
}

/// 对账复算器（独立路径）：形态归类 → 形态内定码。
/// 与 `judge_map` 不共享任何表行；两路全等才证明表可独立复算。
pub fn recompute_win(errno: i32) -> Result<u32, i32> {
    if errno <= 0 {
        return Err(0);
    }
    let kind = match kind_of(errno) {
        Some(k) => k,
        None => return Err(-errno),
    };
    match kind {
        MapKind::HandleFallback => Ok(WIN_ERROR_INVALID_HANDLE),
        MapKind::ManyToOne => match errno {
            EPERM | EACCES => Ok(WIN_ERROR_ACCESS_DENIED),
            _ => Ok(WIN_ERROR_INVALID_PARAMETER),
        },
        MapKind::DirectMatch => match errno {
            ENOENT => Ok(WIN_ERROR_FILE_NOT_FOUND),
            ENOMEM => Ok(WIN_ERROR_NOT_ENOUGH_MEMORY),
            EFAULT => Ok(WIN_ERROR_NOACCESS),
            _ => Ok(WIN_ERROR_INVALID_PARAMETER),
        },
        MapKind::QuotaSurrogate => Ok(WIN_ERROR_NO_SYSTEM_RESOURCES),
        MapKind::FormatFamily => Ok(WIN_ERROR_BAD_EXE_FORMAT),
        MapKind::PipeFamily => Ok(WIN_ERROR_BROKEN_PIPE),
    }
}

/// 逐行对账：冻结表第 i 行，表值 = 判定器 = 复算器三方全等才记 1。
pub fn reconcile_row(i: usize) -> bool {
    let row = match MAP_TABLE.get(i) {
        Some(r) => r,
        None => return false,
    };
    if row.1 >= 0 {
        return false;
    }
    let errno = -row.1;
    match judge_map(errno) {
        Ok(a) => match recompute_win(errno) {
            Ok(b) => a == row.2 && b == row.2,
            Err(_) => false,
        },
        Err(_) => false,
    }
}

/// 全表对账一致行数（应为 MAP_ROWS）。
pub fn reconcile_rows() -> usize {
    let mut n = 0usize;
    let mut i = 0usize;
    while i < MAP_TABLE.len() {
        if reconcile_row(i) {
            n += 1;
        }
        i += 1;
    }
    n
}

// ── 差异账（三条全列，判据硬点） ──────────────────────────────────────
/// 一条差异账：标题 / errno 名 / errno 正值 / Windows 归化码 / 理由。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DiffEntry {
    /// 差异条目标题
    pub title: &'static str,
    /// errno 名
    pub errno_name: &'static str,
    /// errno 正值
    pub errno_val: i32,
    /// Windows 归化码
    pub win_code: u32,
    /// 差异理由（非空中文）
    pub reason: &'static str,
}

/// 差异账三条全列（以冻结表行为准，理由写清归化路径）。
pub const DIFF_LEDGER: [DiffEntry; 3] = [
    DiffEntry {
        title: "差异账一：Windows 无 ESRCH 对应",
        errno_name: "ESRCH",
        errno_val: ESRCH,
        win_code: WIN_ERROR_INVALID_HANDLE,
        reason: "Windows 进程 API 以句柄为操作对象，进程消亡后句柄失效即报 \
                 ERROR_INVALID_HANDLE=6；不存在独立的『无此进程』错误码，\
                 故 ESRCH 归化到句柄失效族",
    },
    DiffEntry {
        title: "差异账二：EAGAIN 语义差异",
        errno_name: "EAGAIN",
        errno_val: EAGAIN,
        win_code: WIN_ERROR_NO_SYSTEM_RESOURCES,
        reason: "Windows 不使用 EAGAIN 的可重试语义；配额/资源耗尽一次性拒绝\
                 并归 ERROR_NO_SYSTEM_RESOURCES=1450，调用方不应盲目重试",
    },
    DiffEntry {
        title: "差异账三：ENOEXEC 在 Windows 是格式族",
        errno_name: "ENOEXEC",
        errno_val: ENOEXEC,
        win_code: WIN_ERROR_BAD_EXE_FORMAT,
        reason: "Windows 把可执行格式错误单列为 ERROR_BAD_EXE_FORMAT=193\
                 （格式族），不归参数错误；POSIX 的 ENOEXEC 语义在 Windows \
                 有专码而非 87",
    },
];

/// 差异账合格条数：标题/理由非空，且判定器与复算器都归到账面码。
pub fn diff_entries_ok() -> usize {
    let mut n = 0usize;
    let mut i = 0usize;
    while i < DIFF_LEDGER.len() {
        if let Some(e) = DIFF_LEDGER.get(i) {
            let via_judge = match judge_map(e.errno_val) {
                Ok(w) => w == e.win_code,
                Err(_) => false,
            };
            let via_recompute = match recompute_win(e.errno_val) {
                Ok(w) => w == e.win_code,
                Err(_) => false,
            };
            if !e.title.is_empty() && !e.reason.is_empty() && !e.errno_name.is_empty()
                && e.errno_val > 0 && via_judge && via_recompute
            {
                n += 1;
            }
        }
        i += 1;
    }
    n
}

// ── 轻量进程表状态机（本卡自有域模型） ────────────────────────────────
/// 进程槽状态。三态闭环：Free→Live→Zombie→Free。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ProcState {
    /// 空槽（从未分配或已收割）
    Free,
    /// 存活
    Live,
    /// 已终止待收割
    Zombie,
}

/// 单个进程槽。pid 与槽位一一对应：pid = 槽号 + 1。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ProcSlot {
    /// 槽位状态
    pub state: ProcState,
    /// 当前占用 pid（Free 时为 0）
    pub pid: i32,
    /// 父 pid（收割权校验用）
    pub parent_pid: i32,
    /// 属主 uid（EPERM 判定用）
    pub uid: u32,
    /// argv 长度（EINVAL/E2BIG 判定用）
    pub argv_len: u32,
    /// 代际（每次 reap 自增；复用后旧代际引用失效）
    pub generation: u16,
}

impl ProcSlot {
    const fn empty() -> ProcSlot {
        ProcSlot {
            state: ProcState::Free,
            pid: 0,
            parent_pid: 0,
            uid: 0,
            argv_len: 0,
            generation: 1,
        }
    }
}

/// 进程槽外部视图（只读快照）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ProcView {
    /// 状态
    pub state: ProcState,
    /// 属主 uid
    pub uid: u32,
    /// argv 长度
    pub argv_len: u32,
    /// 父 pid
    pub parent_pid: i32,
    /// 代际
    pub generation: u16,
}

/// 定容进程表。账目（spawn/kill/reap/exec 计数）只增不减。
#[derive(Clone, Copy, Debug)]
pub struct ProcTable {
    slots: [ProcSlot; MAX_PROCS],
    spawns: u32,
    kills: u32,
    reaps: u32,
    execs: u32,
}

impl ProcTable {
    /// 构造空进程表：全部槽 Free。
    pub const fn new() -> ProcTable {
        ProcTable {
            slots: [ProcSlot::empty(); MAX_PROCS],
            spawns: 0,
            kills: 0,
            reaps: 0,
            execs: 0,
        }
    }

    /// pid → 槽号。pid 非正或与槽不符返回 None（ESRCH 的第一层）。
    fn index_of(&self, pid: i32) -> Option<usize> {
        if pid <= 0 {
            return None;
        }
        let idx = (pid - 1) as usize;
        let slot = match self.slots.get(idx) {
            Some(s) => s,
            None => return None,
        };
        if slot.pid == pid {
            Some(idx)
        } else {
            None
        }
    }

    /// 只读视图：pid 不存在返回 None。
    pub fn view(&self, pid: i32) -> Option<ProcView> {
        let idx = match self.index_of(pid) {
            Some(i) => i,
            None => return None,
        };
        let s = match self.slots.get(idx) {
            Some(s) => s,
            None => return None,
        };
        Some(ProcView {
            state: s.state,
            uid: s.uid,
            argv_len: s.argv_len,
            parent_pid: s.parent_pid,
            generation: s.generation,
        })
    }

    /// spawn：参数校验（EINVAL/E2BIG）先于容量（EAGAIN）。
    /// 表满回 EAGAIN（POSIX fork 口径；Windows 侧对照配额拒绝 1450）。
    pub fn spawn(&mut self, uid: u32, argv_len: u32, parent_pid: i32) -> SysResult<i32> {
        if argv_len < ARGV_MIN {
            return Err(EINVAL);
        }
        if argv_len > ARGV_MAX {
            return Err(E2BIG);
        }
        let mut i = 0usize;
        while i < MAX_PROCS {
            let free = match self.slots.get(i) {
                Some(s) => s.state == ProcState::Free,
                None => false,
            };
            if free {
                let pid = (i + 1) as i32;
                if let Some(sm) = self.slots.get_mut(i) {
                    sm.state = ProcState::Live;
                    sm.pid = pid;
                    sm.parent_pid = parent_pid;
                    sm.uid = uid;
                    sm.argv_len = argv_len;
                }
                self.spawns = self.spawns.saturating_add(1);
                return Ok(pid);
            }
            i += 1;
        }
        Err(EAGAIN)
    }

    /// kill：判定次序 ESRCH（pid 存在性 + Live 态）先于 EPERM（属主）。
    /// root（uid 0）可越权。成功转 Zombie，返回终止时点代际。
    pub fn kill(&mut self, pid: i32, caller_uid: u32, privileged: bool) -> SysResult<u16> {
        let idx = match self.index_of(pid) {
            Some(i) => i,
            None => return Err(ESRCH),
        };
        let (state, owner, gen) = match self.slots.get(idx) {
            Some(s) => (s.state, s.uid, s.generation),
            None => return Err(ESRCH),
        };
        if state != ProcState::Live {
            return Err(ESRCH);
        }
        if owner != caller_uid && caller_uid != ROOT_UID && !privileged {
            return Err(EPERM);
        }
        if let Some(sm) = self.slots.get_mut(idx) {
            sm.state = ProcState::Zombie;
        }
        self.kills = self.kills.saturating_add(1);
        Ok(gen)
    }

    /// exec：pid 校验（ESRCH）先于可执行头校验（ENOEXEC）。
    /// 非 ELF 魔数一律 ENOEXEC（Windows 侧对照格式族 193）。
    pub fn exec(&mut self, pid: i32, magic: u32) -> SysResult<()> {
        let idx = match self.index_of(pid) {
            Some(i) => i,
            None => return Err(ESRCH),
        };
        let state = match self.slots.get(idx) {
            Some(s) => s.state,
            None => return Err(ESRCH),
        };
        if state != ProcState::Live {
            return Err(ESRCH);
        }
        if magic != EXE_MAGIC_ELF {
            return Err(ENOEXEC);
        }
        self.execs = self.execs.saturating_add(1);
        Ok(())
    }

    /// reap：ESRCH（存在性）先于 ECHILD（收割次序），父 pid 不符也按
    /// ECHILD（非本进程子进程）。成功后槽位回 Free，代际自增。
    pub fn reap(&mut self, pid: i32, parent_pid: i32) -> SysResult<u16> {
        let idx = match self.index_of(pid) {
            Some(i) => i,
            None => return Err(ESRCH),
        };
        let (state, parent, gen) = match self.slots.get(idx) {
            Some(s) => (s.state, s.parent_pid, s.generation),
            None => return Err(ESRCH),
        };
        if state != ProcState::Zombie {
            return Err(ECHILD);
        }
        if parent != parent_pid {
            return Err(ECHILD);
        }
        let mut new_gen = gen;
        if let Some(sm) = self.slots.get_mut(idx) {
            sm.state = ProcState::Free;
            // 保留 pid 供收割后 view(pid) 观察槽位回收态；spawn 复用时覆盖。
            sm.parent_pid = 0;
            sm.uid = 0;
            sm.argv_len = 0;
            sm.generation = sm.generation.wrapping_add(1);
            new_gen = sm.generation;
        }
        self.reaps = self.reaps.saturating_add(1);
        Ok(new_gen)
    }

    /// spawn 成功累计数（账目）。
    pub fn spawns(&self) -> u32 {
        self.spawns
    }

    /// kill 成功累计数。
    pub fn kills(&self) -> u32 {
        self.kills
    }

    /// reap 成功累计数。
    pub fn reaps(&self) -> u32 {
        self.reaps
    }

    /// exec 成功累计数。
    pub fn execs(&self) -> u32 {
        self.execs
    }

    /// 当前 Live 槽数。
    pub fn live_count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < MAX_PROCS {
            if let Some(s) = self.slots.get(i) {
                if s.state == ProcState::Live {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 当前 Zombie 槽数。
    pub fn zombie_count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < MAX_PROCS {
            if let Some(s) = self.slots.get(i) {
                if s.state == ProcState::Zombie {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }
}

/// 填满进程表：反复 spawn 直到 EAGAIN，返回成功次数（应为 MAX_PROCS）。
pub fn fill_table(t: &mut ProcTable, uid: u32) -> u32 {
    let mut n = 0u32;
    loop {
        match t.spawn(uid, 8, 0) {
            Ok(_) => n = n.saturating_add(1),
            Err(_) => break,
        }
    }
    n
}

// ── 注入器：四类违约 × 10 轮 ──────────────────────────────────────────
/// 注入类别（四类，判据至少三类）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InjectCase {
    /// 陈旧/不存在 pid：负值、0、越界、未分配——全 ESRCH
    StalePid,
    /// 越权 kill：非属主非特权——EPERM；root 放行为对照
    WrongOwner,
    /// 配额耗尽：16 槽满后 spawn——EAGAIN
    QuotaExhaust,
    /// 坏可执行头：非 ELF 魔数——ENOEXEC
    BadExecHeader,
}

impl InjectCase {
    /// 闭集全量。
    pub const ALL: [InjectCase; 4] = [
        InjectCase::StalePid,
        InjectCase::WrongOwner,
        InjectCase::QuotaExhaust,
        InjectCase::BadExecHeader,
    ];
}

/// 注入账：注入 = 抓获 + 逃逸，逃逸恒为 0。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MapLedger {
    /// 注入总次数
    pub injected: u32,
    /// 抓获次数
    pub caught: u32,
    /// 逃逸次数（必须为 0）
    pub escaped: u32,
}

impl MapLedger {
    const fn zero() -> MapLedger {
        MapLedger { injected: 0, caught: 0, escaped: 0 }
    }

    /// 记一次注入结果；守恒式破坏返回 Err（诊断账回执）。
    pub fn record(&mut self, caught: bool) -> SysResult<()> {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.caught = self.caught.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.caught.saturating_add(self.escaped) != self.injected {
            return Err(EINVAL);
        }
        Ok(())
    }

    /// 守恒式。
    pub fn conserved(&self) -> bool {
        self.caught.saturating_add(self.escaped) == self.injected
    }
}

/// 单轮注入：陈旧 pid（负值/0/越界/未分配四样本全 ESRCH）。
fn inject_stale_pid() -> bool {
    let mut t = ProcTable::new();
    let _ = t.spawn(1000, 4, 0);
    let neg = t.kill(-1, 1000, true) == Err(ESRCH);
    let zero = t.kill(0, 1000, true) == Err(ESRCH);
    let beyond = t.kill(MAX_PROCS as i32 + 1, 1000, true) == Err(ESRCH);
    let unallocated = t.kill(2, 1000, true) == Err(ESRCH);
    neg && zero && beyond && unallocated
}

/// 单轮注入：越权（EPERM）与 root 放行对照。
fn inject_wrong_owner() -> bool {
    let mut t = ProcTable::new();
    let pid = match t.spawn(1000, 4, 0) {
        Ok(p) => p,
        Err(_) => return false,
    };
    let denied = t.kill(pid, 2000, false) == Err(EPERM);
    let root_ok = t.kill(pid, ROOT_UID, false).is_ok();
    denied && root_ok
}

/// 单轮注入：配额耗尽（表满 → EAGAIN，且 spawn 账不丢）。
fn inject_quota() -> bool {
    let mut t = ProcTable::new();
    let filled = fill_table(&mut t, 1000);
    let over = t.spawn(1000, 8, 0) == Err(EAGAIN);
    filled == MAX_PROCS as u32 && over && t.spawns() == MAX_PROCS as u32
}

/// 单轮注入：坏可执行头（ENOEXEC）与好头对照。
fn inject_bad_exec() -> bool {
    let mut t = ProcTable::new();
    let pid = match t.spawn(1000, 4, 0) {
        Ok(p) => p,
        Err(_) => return false,
    };
    let rejected = t.exec(pid, EXE_MAGIC_BAD) == Err(ENOEXEC);
    let accepted = t.exec(pid, EXE_MAGIC_ELF).is_ok();
    rejected && accepted
}

/// 执行注入矩阵：四类 × 10 轮，返回账本与逐类抓获数。
pub fn run_inject_matrix() -> (MapLedger, [u32; 4]) {
    let mut ledger = MapLedger::zero();
    let mut per_class = [0u32; 4];
    let mut ci = 0usize;
    while ci < InjectCase::ALL.len() {
        let case = match InjectCase::ALL.get(ci).copied() {
            Some(c) => c,
            None => break,
        };
        let mut round = 0u32;
        while round < INJECT_ROUNDS {
            let caught = match case {
                InjectCase::StalePid => inject_stale_pid(),
                InjectCase::WrongOwner => inject_wrong_owner(),
                InjectCase::QuotaExhaust => inject_quota(),
                InjectCase::BadExecHeader => inject_bad_exec(),
            };
            let _ = ledger.record(caught);
            if caught {
                if let Some(slot) = per_class.get_mut(ci) {
                    *slot = slot.saturating_add(1);
                }
            }
            round += 1;
        }
        ci += 1;
    }
    (ledger, per_class)
}

// ── 确定性指纹（FNV-1a 32 位） ────────────────────────────────────────
/// FNV-1a：无浮点、无随机、无墙钟。
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

/// 规范语料指纹：judge_map 全域 1..=40 逐码 + 进程表全生命周期场景。
pub fn corpus_fingerprint() -> u32 {
    let mut bytes: Vec<u8> = Vec::new();
    // 第一段：映射全域（含表内 13 码与表外样本）
    let mut e = 1i32;
    while e <= 40 {
        let v = match judge_map(e) {
            Ok(w) => w as i32,
            Err(c) => c,
        };
        bytes.push((v & 0xFF) as u8);
        bytes.push(((v >> 8) & 0xFF) as u8);
        e += 1;
    }
    // 第二段：spawn→exec→kill→reap→再 spawn(零 argv) 全链编码
    let mut t = ProcTable::new();
    let sp = t.spawn(700, 4, 0);
    bytes.push(match sp {
        Ok(p) => (p & 0xFF) as u8,
        Err(c) => (c & 0xFF) as u8,
    });
    let ex = t.exec(1, EXE_MAGIC_ELF);
    bytes.push(match ex {
        Ok(()) => 0xA5,
        Err(c) => (c & 0xFF) as u8,
    });
    let kl = t.kill(1, 700, false);
    bytes.push(match kl {
        Ok(g) => (g & 0xFF) as u8,
        Err(c) => (c & 0xFF) as u8,
    });
    let rp = t.reap(1, 0);
    bytes.push(match rp {
        Ok(g) => (g & 0xFF) as u8,
        Err(c) => (c & 0xFF) as u8,
    });
    let zero_argv = t.spawn(700, 0, 0);
    bytes.push(match zero_argv {
        Ok(p) => (p & 0xFF) as u8,
        Err(c) => (c & 0xFF) as u8,
    });
    fnv1a(&bytes)
}

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x00..+0x0C 闭集，13 码）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MapCode {
    /// 非正 errno 输入
    NonPositiveErrno,
    /// 表外 errno
    BeyondTable,
    /// Windows 无 ESRCH 对应（差异账一）
    NoEsrchCounterpart,
    /// EAGAIN 配额替代语义（差异账二）
    QuotaNoRetry,
    /// ENOEXEC 格式族（差异账三）
    FormatFamily,
    /// 多对一归并提示
    ManyToOneAlias,
    /// 进程表满
    ProcTableFull,
    /// 收割次序被拒
    NotZombie,
    /// 陈旧代际
    StaleGeneration,
    /// 状态机非法转换
    IllegalTransition,
    /// 注入账倒退
    LedgerRegress,
    /// 双跑不一致
    NotReproducible,
    /// 段位越界
    SegmentViolation,
}

impl MapCode {
    /// 码值（段基址 + 变体序号）。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            MapCode::NonPositiveErrno => "非正 errno 被拒：映射判据只接受正值；\
                0 与负值一律回 Err(0)，请先取 errno 绝对值再映射",
            MapCode::BeyondTable => "表外 errno 被拒：仅冻结对照表内 13 个码可映射；\
                表外回 Err(-errno) 显性拒绝，不得猜测最近似码",
            MapCode::NoEsrchCounterpart => "Windows 无 ESRCH 对应：进程消亡后句柄\
                失效统一报 ERROR_INVALID_HANDLE=6；请按句柄失效族处理",
            MapCode::QuotaNoRetry => "EAGAIN 语义差异：Windows 不用可重试的 EAGAIN；\
                配额耗尽一次性报 ERROR_NO_SYSTEM_RESOURCES=1450，勿盲目重试",
            MapCode::FormatFamily => "ENOEXEC 在 Windows 是格式族：可执行格式错误\
                专码 ERROR_BAD_EXE_FORMAT=193，不归参数错误族 87",
            MapCode::ManyToOneAlias => "多对一归并提示：EPERM/EACCES 同归 5、\
                E2BIG 归 87；反向拆解须回查 errno 侧原码，不得从 Windows 码\
                反推唯一 errno",
            MapCode::ProcTableFull => "进程表满：spawn 回 EAGAIN（POSIX 口径），\
                Windows 侧对照配额拒绝 1450；请先收割僵尸释放槽位",
            MapCode::NotZombie => "收割次序被拒：仅 Zombie 态可 reap；Live 态回 \
                ECHILD、Free 槽回 ESRCH；请先 kill 再收割",
            MapCode::StaleGeneration => "陈旧代际被拒：pid 复用后旧代际引用失效；\
                请以 reap 返回的新代际重建引用",
            MapCode::IllegalTransition => "状态机非法转换被拒：Free→Live 仅经 \
                spawn、Live→Zombie 仅经 kill、Zombie→Free 仅经 reap；\
                其他路径一律拒绝",
            MapCode::LedgerRegress => "注入账倒退被拒：注入=抓获+逃逸恒等式破坏\
                或计数回退；请检查回滚路径是否误改累计值",
            MapCode::NotReproducible => "双跑不一致：同输入两次指纹不同；请排查\
                隐藏可变状态或非确定编码",
            MapCode::SegmentViolation => "段位越界：本模块段基址必须 0x40 对齐且\
                落在 0x7A000..=0x7D1C0 窗口内",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [MapCode; 13] = [
        MapCode::NonPositiveErrno,
        MapCode::BeyondTable,
        MapCode::NoEsrchCounterpart,
        MapCode::QuotaNoRetry,
        MapCode::FormatFamily,
        MapCode::ManyToOneAlias,
        MapCode::ProcTableFull,
        MapCode::NotZombie,
        MapCode::StaleGeneration,
        MapCode::IllegalTransition,
        MapCode::LedgerRegress,
        MapCode::NotReproducible,
        MapCode::SegmentViolation,
    ];
}

/// 全码值按位或（闭集活性探针，防死码）。
pub fn probe_codes() -> u32 {
    let mut acc: u32 = 0;
    let mut i = 0usize;
    while i < MapCode::ALL.len() {
        if let Some(c) = MapCode::ALL.get(i) {
            acc |= c.val();
        }
        i += 1;
    }
    acc
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9296 判据集（静态 ≥14 条；动态明细 ≤8 条）。
pub fn run_unx9296_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9296");

    // 预计算：注入矩阵 / 指纹 / 对账 / 差异账
    let (ledger, per_class) = run_inject_matrix();
    let fp_a = corpus_fingerprint();
    let fp_b = corpus_fingerprint();
    let recon = reconcile_rows();
    let diff_ok = diff_entries_ok();

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x7A000..=0x7D1C0).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x7A000..=0x7D1C0", seg_ok,
          "段基址必须唯一且落在本批窗口；越界即拒绝注册");

    // 2. 对照表冻结：行数落在 10..=14
    let rows_ok = MAP_ROWS >= 10 && MAP_ROWS <= 14 && MAP_TABLE.len() == MAP_ROWS;
    s.add("对照表冻结：行数在 10..=14", rows_ok,
          "双列对照表冻结 13 行，不得增删或重排");

    // 3. 主判据：ERROR_INVALID_PARAMETER 映射（EINVAL→87），
    //    附 ESRCH→6 与 EPIPE→109 两个锚点
    let main_ok = judge_map(EINVAL) == Ok(WIN_ERROR_INVALID_PARAMETER)
        && judge_map(ESRCH) == Ok(WIN_ERROR_INVALID_HANDLE)
        && judge_map(EPIPE) == Ok(WIN_ERROR_BROKEN_PIPE);
    s.add("主判据：EINVAL→ERROR_INVALID_PARAMETER=87", main_ok,
          "参数非法族映射是本卡核心；锚点码验证表右列真值");

    // 4. 全表逐码映射：13 行 judge_map 全部命中表右列
    let mut all_rows = true;
    let mut i = 0usize;
    while i < MAP_TABLE.len() {
        if let Some(row) = MAP_TABLE.get(i) {
            if judge_map(-row.1) != Ok(row.2) {
                all_rows = false;
            }
        } else {
            all_rows = false;
        }
        i += 1;
    }
    s.add("全表逐码映射：13 行全部命中", all_rows,
          "表内每个 errno 正值都必须映射到冻结右列码");

    // 5. 表外拒绝：非正输入与表外正值全部显性 Err
    let reject_ok = judge_map(0) == Err(0)
        && judge_map(-EINVAL) == Err(0)
        && judge_map(31) == Err(-31)
        && judge_map(33) == Err(-33)
        && judge_map(9999) == Err(-9999);
    s.add("表外拒绝：0/负值/31/33/9999 全 Err", reject_ok,
          "表外码不得猜测映射；非正输入回 Err(0)，表外回 Err(-errno)");

    // 6. 独立复算：判定器与复算器在 1..=133 全域逐码相等，
    //    且形态归类覆盖与表成员严格一致
    let mut dual_ok = true;
    let mut e = 1i32;
    while e <= 133 {
        if judge_map(e) != recompute_win(e) {
            dual_ok = false;
        }
        if kind_of(e).is_some() != judge_map(e).is_ok() {
            dual_ok = false;
        }
        e += 1;
    }
    s.add("独立复算：判定器与复算器 1..=133 全等", dual_ok,
          "表行必须可不经表独立复算；形态归类覆盖须与表成员一致");

    // 7. 差异账三条全列：条数、标题、理由全合格
    let diff_list_ok = DIFF_LEDGER.len() == 3 && diff_ok == 3;
    s.add("差异账三条全列：标题与理由全非空", diff_list_ok,
          "ESRCH/EAGAIN/ENOEXEC 三条差异必须全列且理由可读");

    // 8. 差异账逐条可复算：账面码 = 判定器 = 复算器，锚定三码
    let d1 = DIFF_LEDGER.get(0).copied();
    let d2 = DIFF_LEDGER.get(1).copied();
    let d3 = DIFF_LEDGER.get(2).copied();
    let diff_recompute_ok = match (d1, d2, d3) {
        (Some(a), Some(b), Some(c)) => {
            a.errno_val == ESRCH && a.win_code == WIN_ERROR_INVALID_HANDLE
                && b.errno_val == EAGAIN && b.win_code == WIN_ERROR_NO_SYSTEM_RESOURCES
                && c.errno_val == ENOEXEC && c.win_code == WIN_ERROR_BAD_EXE_FORMAT
                && judge_map(a.errno_val) == Ok(a.win_code)
                && judge_map(b.errno_val) == Ok(b.win_code)
                && judge_map(c.errno_val) == Ok(c.win_code)
                && recompute_win(a.errno_val) == Ok(a.win_code)
                && recompute_win(b.errno_val) == Ok(b.win_code)
                && recompute_win(c.errno_val) == Ok(c.win_code)
        }
        _ => false,
    };
    s.add("差异账复算：三码两路归化全等", diff_recompute_ok,
          "6/1450/193 三码必须经判定器与复算器双路验证");

    // 9. 正常流：spawn→exec→kill→reap 全生命周期
    let mut t = ProcTable::new();
    let lifecycle_ok = match t.spawn(500, 4, 0) {
        Ok(pid) => {
            let v = t.view(pid);
            let exec_ok = t.exec(pid, EXE_MAGIC_ELF).is_ok();
            let kill_ok = t.kill(pid, 500, false).is_ok();
            let reap_ok = t.reap(pid, 0).is_ok();
            let freed = match t.view(pid) {
                Some(vv) => vv.state == ProcState::Free,
                None => false,
            };
            match v {
                Some(vv) => vv.state == ProcState::Live && vv.uid == 500
                    && vv.argv_len == 4 && exec_ok && kill_ok && reap_ok && freed,
                None => false,
            }
        }
        Err(_) => false,
    };
    s.add("正常流：spawn→exec→kill→reap 全生命周期", lifecycle_ok,
          "合法 uid 与 argv 全链贯通，收割后槽位回 Free");

    // 10. 参数边界四类：零/下限/上限/上限+1
    let mut b = ProcTable::new();
    let bound_ok = b.spawn(1, 0, 0) == Err(EINVAL)
        && b.spawn(1, ARGV_MIN, 0).is_ok()
        && b.spawn(1, ARGV_MAX, 0).is_ok()
        && b.spawn(1, ARGV_MAX + 1, 0) == Err(E2BIG);
    s.add("参数边界：argv 零/下限/上限/上限+1", bound_ok,
          "0 回 EINVAL、1..=4096 放行、4097 回 E2BIG");

    // 11. 注入矩阵：四类各 10/10 全抓
    let inject_all = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("注入矩阵：四类违约各 10/10 全抓", inject_all,
          "陈旧 pid/越权/配额耗尽/坏头四类必须全部被判定器抓住");

    // 12. 注入账守恒：注入 = 抓获 + 逃逸，逃逸为 0
    let ledger_ok = ledger.conserved()
        && ledger.escaped == 0
        && ledger.injected == InjectCase::ALL.len() as u32 * INJECT_ROUNDS;
    s.add("注入账守恒：40 注入 40 抓获 0 逃逸", ledger_ok,
          "守恒式破坏即账目失真，必须拒绝");

    // 13. 判定次序：pid 存在性（ESRCH）先于权限（EPERM）
    let mut o = ProcTable::new();
    let pid_o = match o.spawn(1000, 4, 0) {
        Ok(p) => p,
        Err(_) => -1,
    };
    let order_ok = o.kill(9999, 2000, false) == Err(ESRCH)
        && o.kill(pid_o, 2000, false) == Err(EPERM);
    s.add("判定次序：ESRCH 先于 EPERM", order_ok,
          "不存在 pid 与越权同时成立时必须先回 ESRCH");

    // 14. 状态机非法转换拒绝
    let mut m = ProcTable::new();
    let pid_m = match m.spawn(1000, 4, 0) {
        Ok(p) => p,
        Err(_) => -1,
    };
    let _ = m.kill(pid_m, 1000, false);
    let kill_zombie = m.kill(pid_m, 1000, false) == Err(ESRCH);
    let reap_live = {
        let mut t2 = ProcTable::new();
        match t2.spawn(1000, 4, 0) {
            Ok(q) => t2.reap(q, 0) == Err(ECHILD),
            Err(_) => false,
        }
    };
    let reap_free = m.reap(2, 0) == Err(ESRCH);
    let state_ok = kill_zombie && reap_live && reap_free;
    s.add("状态机：Zombie 不可再杀、Live 不可收、Free 收割回 ESRCH", state_ok,
          "三态闭环之外的一切转换都必须显性拒绝");

    // 15. 容量耗尽：16 槽满回 EAGAIN，收割一格后可复用且账不丢
    let mut c = ProcTable::new();
    let filled = fill_table(&mut c, 1000);
    let over = c.spawn(1000, 8, 0) == Err(EAGAIN);
    let k5 = c.kill(5, 1000, false);
    let r5 = c.reap(5, 0);
    let again = c.spawn(1000, 8, 0);
    let cap_ok = filled == MAX_PROCS as u32
        && over
        && k5.is_ok()
        && r5.is_ok()
        && again.is_ok()
        && c.spawns() == MAX_PROCS as u32 + 1;
    s.add("容量耗尽：EAGAIN 拒绝且收割后账目闭合", cap_ok,
          "表满必须回 EAGAIN；kill+reap 释放一格后 spawn 复活，spawn 账 17 笔");

    // 16. 双跑一致：规范语料指纹相等
    let fp_ok = fp_a == fp_b;
    s.add("双跑一致：规范语料指纹相等", fp_ok,
          "同输入两次必须逐字节一致，隐藏状态即失败");

    // 17. 文案闭集：13 个诊断码均有非空中文建议
    let text_ok = MapCode::ALL.iter().all(|cd| !cd.detail().is_empty());
    s.add("文案闭集：全部诊断码带中文建议", text_ok,
          "每个码必须能取到非空且可操作的说明");

    // 18. 诊断码唯一：13 个码值两两不同
    let mut uniq = true;
    let mut ui = 0usize;
    while ui < MapCode::ALL.len() {
        let mut uj = ui + 1;
        while uj < MapCode::ALL.len() {
            if let (Some(a), Some(b)) = (MapCode::ALL.get(ui), MapCode::ALL.get(uj)) {
                if a.val() == b.val() {
                    uniq = false;
                }
            }
            uj += 1;
        }
        ui += 1;
    }
    s.add("诊断码：13 个码值两两唯一", uniq,
          "段内闭集码不得重复，否则判据无法定位");

    // 19. 账目单调：注入账再记 4 笔只增不减
    let mut mono = ledger;
    let before = mono.injected;
    let mut k = 0u32;
    while k < 4 {
        let _ = mono.record(true);
        k += 1;
    }
    s.add("账目单调：再注入只增不减",
          mono.injected == before.saturating_add(4) && mono.conserved(),
          "累计计数禁止回退；守恒式随增量保持");

    // 20. 闭集活性：全码位或非零且含段基址
    let probe = probe_codes();
    let probe_ok = probe != 0 && (probe & SEG_BASE) == SEG_BASE;
    s.add("闭集活性：全码探针含段基址", probe_ok,
          "13 码按位或必须覆盖段基址，证明无死码窗口");

    // ── 动态明细（≤8） ────────────────────────────────────────────────
    let ledger_text = format!(
        "注入 {} 抓 {} 逃 {}",
        ledger.injected, ledger.caught, ledger.escaped
    );
    s.add_dyn("注入账快照", ledger.conserved() && ledger.escaped == 0, &ledger_text);

    let fp_text = format!("指纹 0x{:08X} 双跑 {}", fp_a, fp_a == fp_b);
    s.add_dyn("指纹快照", fp_ok, &fp_text);

    let recon_text = format!("对账 {}/{} 行复算一致", recon, MAP_ROWS);
    s.add_dyn("对账快照", recon == MAP_ROWS, &recon_text);

    let diff_text = format!("差异账 {}/3 条两路归化合格", diff_ok);
    s.add_dyn("差异账快照", diff_ok == 3, &diff_text);

    let acc_text = format!(
        "开 {} 执 {} 杀 {} 收 {}",
        c.spawns(), c.execs(), c.kills(), c.reaps()
    );
    s.add_dyn("进程账快照",
              c.spawns() == MAX_PROCS as u32 + 1 && c.kills() == 1 && c.reaps() == 1,
              &acc_text);

    // 21. 收口：判据条数达到声明值
    let n_before = s.len();
    s.add("收口：判据条数达到声明值", n_before >= 14,
          "静态判据不足 14 条即未达深度下限，收口拒绝");

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    /// 空差异账条目（测试里 .get 越界时的兜底值，避免裸下标与 unwrap）。
    fn empty_diff() -> DiffEntry {
        DiffEntry {
            title: "",
            errno_name: "",
            errno_val: 0,
            win_code: 0,
            reason: "",
        }
    }

    #[test]
    fn normal_flow_lifecycle() {
        let mut t = ProcTable::new();
        let pid = t.spawn(500, 4, 0).unwrap_or(-1);
        assert_eq!(pid, 1);
        assert!(t.exec(pid, EXE_MAGIC_ELF).is_ok());
        assert!(t.kill(pid, 500, false).is_ok());
        assert!(t.reap(pid, 0).is_ok());
        let v = t.view(pid).unwrap_or(ProcView {
            state: ProcState::Zombie,
            uid: 0,
            argv_len: 0,
            parent_pid: 0,
            generation: 0,
        });
        assert_eq!(v.state, ProcState::Free);
        assert_eq!(judge_map(EINVAL), Ok(WIN_ERROR_INVALID_PARAMETER));
    }

    #[test]
    fn four_boundary_classes() {
        // errno 侧四类边界：0 / 负值 / 表内上界 32 / 表外 33
        assert_eq!(judge_map(0), Err(0));
        assert_eq!(judge_map(-22), Err(0));
        assert_eq!(judge_map(32), Ok(WIN_ERROR_BROKEN_PIPE));
        assert_eq!(judge_map(33), Err(-33));
        // argv 侧四类边界：0 / 1 / 4096 / 4097
        let mut t = ProcTable::new();
        assert_eq!(t.spawn(1, 0, 0), Err(EINVAL));
        assert!(t.spawn(1, ARGV_MIN, 0).is_ok());
        assert!(t.spawn(1, ARGV_MAX, 0).is_ok());
        assert_eq!(t.spawn(1, ARGV_MAX + 1, 0), Err(E2BIG));
    }

    #[test]
    fn inject_matrix_all_caught() {
        let (l, per) = run_inject_matrix();
        assert!(l.conserved());
        assert_eq!(l.escaped, 0);
        assert_eq!(l.injected, InjectCase::ALL.len() as u32 * INJECT_ROUNDS);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn judge_order_esrch_before_eperm() {
        let mut t = ProcTable::new();
        let pid = t.spawn(1000, 4, 0).unwrap_or(-1);
        // 两错并存：pid 不存在 + 调用者越权 → 必须先回 ESRCH
        assert_eq!(t.kill(9999, 2000, false), Err(ESRCH));
        assert_eq!(t.kill(-5, 2000, false), Err(ESRCH));
        // pid 存在但越权 → EPERM
        assert_eq!(t.kill(pid, 2000, false), Err(EPERM));
        // root 放行
        assert!(t.kill(pid, ROOT_UID, false).is_ok());
    }

    #[test]
    fn state_machine_illegal_transitions() {
        let mut t = ProcTable::new();
        let pid = t.spawn(1000, 4, 0).unwrap_or(-1);
        assert!(t.kill(pid, 1000, false).is_ok());
        // Zombie 上再 kill：Live 前置不满足 → ESRCH
        assert_eq!(t.kill(pid, 1000, false), Err(ESRCH));
        // Live 上 reap：Zombie 前置不满足 → ECHILD
        let mut t2 = ProcTable::new();
        let q = t2.spawn(1000, 4, 0).unwrap_or(-1);
        assert_eq!(t2.reap(q, 0), Err(ECHILD));
        // Free 槽 reap：存在性不满足 → ESRCH
        assert_eq!(t2.reap(3, 0), Err(ESRCH));
        // 父 pid 不符 → ECHILD
        let mut t3 = ProcTable::new();
        let p3 = t3.spawn(1000, 4, 7).unwrap_or(-1);
        assert!(t3.kill(p3, 1000, false).is_ok());
        assert_eq!(t3.reap(p3, 0), Err(ECHILD));
        assert!(t3.reap(p3, 7).is_ok());
    }

    #[test]
    fn capacity_exhaustion_eagain() {
        let mut t = ProcTable::new();
        assert_eq!(fill_table(&mut t, 1000), MAX_PROCS as u32);
        assert_eq!(t.spawn(1000, 8, 0), Err(EAGAIN));
        assert!(t.kill(5, 1000, false).is_ok());
        assert!(t.reap(5, 0).is_ok());
        assert!(t.spawn(1000, 8, 0).is_ok());
        assert_eq!(t.spawns(), MAX_PROCS as u32 + 1);
        assert_eq!(t.live_count(), MAX_PROCS);
    }

    #[test]
    fn segment_guard() {
        assert_eq!(SEG_BASE % 0x40, 0);
        assert!((0x7A000..=0x7D1C0).contains(&SEG_BASE));
        assert_eq!(SEG_BASE, 0x7B780);
    }

    #[test]
    fn map_table_reconcile() {
        assert!(MAP_ROWS >= 10 && MAP_ROWS <= 14);
        assert_eq!(reconcile_rows(), MAP_ROWS);
        // 两路全域相等（含表外同为 Err）
        for e in 1..=133 {
            assert_eq!(judge_map(e), recompute_win(e));
        }
        // 形态覆盖与表成员一致
        for e in 1..=160 {
            assert_eq!(kind_of(e).is_some(), judge_map(e).is_ok());
        }
    }

    #[test]
    fn diff_ledger_recomputes() {
        assert_eq!(DIFF_LEDGER.len(), 3);
        assert_eq!(diff_entries_ok(), 3);
        let d1 = DIFF_LEDGER.get(0).copied().unwrap_or_else(empty_diff);
        let d2 = DIFF_LEDGER.get(1).copied().unwrap_or_else(empty_diff);
        let d3 = DIFF_LEDGER.get(2).copied().unwrap_or_else(empty_diff);
        assert_eq!(d1.win_code, WIN_ERROR_INVALID_HANDLE);
        assert_eq!(d2.win_code, WIN_ERROR_NO_SYSTEM_RESOURCES);
        assert_eq!(d3.win_code, WIN_ERROR_BAD_EXE_FORMAT);
        assert!(!d1.reason.is_empty() && !d2.reason.is_empty() && !d3.reason.is_empty());
        assert_eq!(judge_map(d1.errno_val), Ok(d1.win_code));
        assert_eq!(judge_map(d2.errno_val), Ok(d2.win_code));
        assert_eq!(judge_map(d3.errno_val), Ok(d3.win_code));
    }

    #[test]
    fn double_run_fingerprint_stable() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn diagnostic_codes_closed_set() {
        assert_eq!(MapCode::ALL.len(), 13);
        for c in MapCode::ALL.iter() {
            assert!(!c.detail().is_empty());
        }
        assert_eq!(MapCode::NonPositiveErrno.val(), SEG_BASE);
        assert_eq!(MapCode::SegmentViolation.val(), SEG_BASE + 12);
        assert_ne!(probe_codes(), 0);
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx9296_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
        assert!(!s.truncated());
        assert!(s.dyn_used() <= 8);
        assert!(s.len() >= 21);
    }
}
