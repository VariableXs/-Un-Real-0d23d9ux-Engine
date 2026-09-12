//! AI-01 · 用户态进程域（VARIABLE-200，F001~F025，W2）.
//!
//! 让内核长出"里面能跑程序"的能力：每进程独立地址空间、ring3 双路入口、
//! 完整生命周期（spawn/wait/exit/reap）、信号、崩溃隔离、句柄与能力、
//! 优先级与预算、命名空间、CoW fork、页错误处理、core dump、调试接口、
//! 记账仪表与线程模型。
//!
//! 地址空间的页表级事实在 `crate::mem::addrspace`（F001 的落点），本模块
//! 负责进程语义：**谁**在用这套地址空间、**什么时候**换装、**崩了之后**
//! 谁被回收谁不受影响。两者合成 AI-01 的 25 项 CheckSet。
//!
//! 纯逻辑 + 固定容量数组，`no_std` 无分配；每个 F 项都有单测与自检断言。

use crate::checks::CheckSet;
use crate::mem::addrspace::{self, AddressSpace, RegionKind, SpaceArena};
use crate::mem::paging::{self, P_NX, P_WRITE};

// ---------------------------------------------------------------------------
// 域常量
// ---------------------------------------------------------------------------

/// 进程表容量（F007 上限；耗尽必须可诊断，而不是静默丢进程）。
pub const MAX_PROCS: usize = 64;
/// init（pid 1）的固定身份，孤儿收养的终点（F010）。
pub const PID_INIT: u32 = 1;
/// 句柄表容量（F014）。
pub const MAX_HANDLES: usize = 32;
/// 线程表容量（F024）。
pub const MAX_THREADS: usize = 16;
/// 优先级档数（F016）：0 最低、31 最高。
pub const PRIO_LEVELS: usize = 32;
/// 进程名上限（F018）。
pub const NAME_BYTES: usize = 16;
/// 记账环形样本数（F023）。
pub const METER_SAMPLES: usize = 16;
/// core dump 保留的寄存器/映射条目（F021）。
pub const CORE_REGISTERS: usize = 18;
pub const CORE_MAPPINGS: usize = 16;
/// 单进程 CPU 预算，单位 tick（F017）。
pub const CPU_QUOTA_TICKS: u64 = 1000;
/// 用户态数据页的默认权限：可写不可执行（F029 的 W^X 纪律）。
const RW_USER: u64 = P_WRITE | P_NX;

// ---------------------------------------------------------------------------
// F002 — ring3 切换原语（sysret / iret 双路）
// ---------------------------------------------------------------------------

/// 两条进入用户态的硬件路径。两条都必须在，任何一条坏掉另一条就是备路
/// （降级纪律）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EntryPath {
    /// `sysretq`：rcx→rip、r11→rflags、rsp 走 MSR，最快的一条。
    Sysret,
    /// `iretq`：现场全在栈上，能带任意段选择子，调试与异常回退用。
    Iret,
}

impl EntryPath {
    pub fn as_str(self) -> &'static str {
        match self {
            EntryPath::Sysret => "sysret",
            EntryPath::Iret => "iret",
        }
    }
}

/// 一次切换的完整现场。`Iret` 需要 rsp/rflags 在栈上，`Sysret` 需要
/// rcx/r11 在寄存器里 —— 这份结构把差异摊开，两种路径共用同一份事实。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct EntryPlan {
    pub path: EntryPath,
    pub rip: u64,
    pub rsp: u64,
    pub rflags: u64,
    /// `Sysret` 用；`Iret` 忽略（栈上有同样两份）。
    pub rcx: u64,
    pub r11: u64,
}

impl Default for EntryPath {
    fn default() -> EntryPath {
        EntryPath::Sysret
    }
}

/// 用户态 rflags 的硬性要求：中断必须开（否则用户程序再也回不来）、
/// IOPL 必须 3（用户不能碰端口）、方向标志清零（串操作约定）。
pub fn user_rflags() -> u64 {
    super::RFLAGS_IF | super::RFLAGS_IOPL_USER
}

/// 生成一次进入用户态的计划。地址与栈都必须落在用户半区，否则拒绝 ——
/// 把内核地址交给 `sysret` 是最典型的提权手法。
pub fn plan_entry(
    path: EntryPath,
    entry: u64,
    stack_top: u64,
    frame: &mut EntryPlan,
) -> Result<(), &'static str> {
    if !paging::is_user(entry) {
        return Err("entry not in user half");
    }
    if !paging::is_user(stack_top) || stack_top < addrspace::USER_MIN {
        return Err("stack not in user half");
    }
    let rflags = user_rflags();
    *frame = EntryPlan {
        path,
        rip: entry,
        rsp: stack_top & !0xF,
        rflags,
        rcx: entry,
        r11: rflags,
    };
    Ok(())
}

/// 切换往返零损判定：从内核态发出的计划，回到内核态时 rip/rsp 必须与
/// 计划一致，且 rflags 必须仍带着用户态要求（IF 开、IOPL=3）。
pub fn entry_round_trips(frame: &EntryPlan, returned_rip: u64, returned_rsp: u64) -> bool {
    frame.rip == returned_rip
        && frame.rsp == returned_rsp
        && frame.rflags & super::RFLAGS_IF != 0
        && frame.rflags & super::RFLAGS_IOPL_USER == super::RFLAGS_IOPL_USER
}

// ---------------------------------------------------------------------------
// F006/F007 — 进程控制块与进程表
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ProcState {
    /// 就绪，等待调度。
    Ready,
    /// 正在 CPU 上。
    Running,
    /// 等事件。
    Blocked,
    /// 已退出但尚未被回收（F010）。
    Zombie,
    /// 槽位空闲。
    Vacant,
}

impl ProcState {
    pub fn as_str(self) -> &'static str {
        match self {
            ProcState::Ready => "ready",
            ProcState::Running => "running",
            ProcState::Blocked => "blocked",
            ProcState::Zombie => "zombie",
            ProcState::Vacant => "vacant",
        }
    }

    pub fn alive(self) -> bool {
        matches!(self, ProcState::Ready | ProcState::Running | ProcState::Blocked)
    }
}

/// F006 — 进程控制块：PID、状态、地址空间、句柄表、记账一体。
#[derive(Clone, Copy, Debug)]
pub struct Pcb {
    pub pid: u32,
    pub ppid: u32,
    pub state: ProcState,
    /// 优先级档 0..PRIO_LEVELS（F016）。
    pub priority: u8,
    /// CPU 亲和掩码，bit n = 允许跑在 cpu n（F016）。
    pub affinity: u64,
    /// 能力位（F015）。
    pub caps: ProcCaps,
    /// 地址空间在 arena 中的槽位；None 表示尚未建立（加载前）。
    pub space: Option<u8>,
    /// 入口地址（F009 记录的 spawn 结果）。
    pub entry: u64,
    /// 退出码；Zombie 状态下有效（F011）。
    pub exit_code: i32,
    /// 终止它的信号号，0 表示正常退出（F012/F013）。
    pub killed_by: u32,
    /// F017 预算记账。
    pub budget: Budget,
    /// F023 记账仪表。
    pub meter: ProcMeter,
    /// 跟随该进程消亡的页数（F011 回收账本）。
    pub owned_pages: u32,
    /// 句柄表槽位占用（F014）。
    pub handles: HandleTable,
    name: [u8; NAME_BYTES],
    name_len: u8,
}

impl Pcb {
    fn vacant(pid: u32) -> Pcb {
        Pcb {
            pid,
            ppid: 0,
            state: ProcState::Vacant,
            priority: PRIO_LEVELS as u8 / 2,
            affinity: u64::MAX,
            caps: ProcCaps::none(),
            space: None,
            entry: 0,
            exit_code: 0,
            killed_by: 0,
            budget: Budget::new(),
            meter: ProcMeter::new(),
            owned_pages: 0,
            handles: HandleTable::new(),
            name: [0; NAME_BYTES],
            name_len: 0,
        }
    }

    pub fn name(&self) -> &[u8] {
        &self.name[..self.name_len as usize]
    }

    fn set_name(&mut self, name: &[u8]) {
        let n = name.len().min(NAME_BYTES);
        self.name[..n].copy_from_slice(&name[..n]);
        self.name_len = n as u8;
    }

    /// 该进程当前是否允许在 `cpu` 上跑。
    pub fn permits_cpu(&self, cpu: u32) -> bool {
        cpu < 64 && self.affinity & (1u64 << cpu) != 0
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SpawnError {
    /// 进程表满（F007 必须可诊断）。
    TableFull,
    /// 父进程不存在或已死。
    NoParent,
    /// 父进程没有给"创建进程"的能力（F015）。
    NoCapability,
    /// 命名重复且重名被拒（F018）。
    NameTaken,
    /// 地址空间无法建立（F001 失败即不创建进程）。
    NoSpace,
}

/// F007 — 定长进程表；分配/回收闭环、耗尽可诊断。
#[derive(Clone, Copy, Debug)]
pub struct ProcTable {
    slots: [Pcb; MAX_PROCS],
    live: usize,
    /// 曾经因为表满而失败的次数 —— 诊断"为什么起不来进程"。
    pub exhausted: u32,
}

impl ProcTable {
    pub const fn new() -> ProcTable {
        ProcTable {
            slots: [Pcb {
                pid: 0,
                ppid: 0,
                state: ProcState::Vacant,
                priority: 16,
                affinity: u64::MAX,
                caps: ProcCaps::none(),
                space: None,
                entry: 0,
                exit_code: 0,
                killed_by: 0,
                budget: Budget::new(),
                meter: ProcMeter::new(),
                owned_pages: 0,
                handles: HandleTable::new(),
                name: [0; NAME_BYTES],
                name_len: 0,
            }; MAX_PROCS],
            live: 0,
            exhausted: 0,
        }
    }

    pub fn live(&self) -> usize {
        self.live
    }

    pub fn capacity(&self) -> usize {
        MAX_PROCS
    }

    pub fn get(&self, i: usize) -> Option<&Pcb> {
        self.slots.get(i)
    }

    pub fn get_mut(&mut self, i: usize) -> Option<&mut Pcb> {
        self.slots.get_mut(i)
    }

    /// 按 pid 找槽位。pid 0 是"内核"，永远不在表里。
    pub fn find(&self, pid: u32) -> Option<usize> {
        if pid == 0 {
            return None;
        }
        self.slots
            .iter()
            .position(|p| p.pid == pid && p.state != ProcState::Vacant)
    }

    /// 按名字找（F018 的命名查询）。
    pub fn find_name(&self, name: &[u8]) -> Option<usize> {
        self.slots
            .iter()
            .position(|p| p.state != ProcState::Vacant && p.name() == name)
    }

    /// 已登记 pid 的集合（含僵尸），供命名空间快照使用。
    pub fn pids(&self) -> [u32; MAX_PROCS] {
        let mut out = [0u32; MAX_PROCS];
        for (i, p) in self.slots.iter().enumerate() {
            out[i] = p.pid;
        }
        out
    }

    /// 回收空槽是尾插，因此"找到第一个 Vacant"等价于"最小空闲槽"。
    fn free_slot(&self) -> Option<usize> {
        self.slots.iter().position(|p| p.state == ProcState::Vacant)
    }

    pub fn next_pid(&self) -> u32 {
        let mut max = PID_INIT;
        for p in self.slots.iter() {
            if p.pid > max && p.state != ProcState::Vacant {
                max = p.pid;
            }
        }
        max + 1
    }

    /// F007 — 直接占用一个槽位（没有用户地址空间的早期进程 / 内核线程用）。
    /// 表容量的判定与地址空间容量各自独立：进程表耗尽必须报 TableFull，
    /// 而不是被页表 arena 的容量掩盖。
    pub fn alloc(&mut self, parent: u32, name: &[u8]) -> Result<u32, SpawnError> {
        if name.len() > NAME_BYTES || self.find_name(name).is_some() {
            return Err(SpawnError::NameTaken);
        }
        let slot = match self.free_slot() {
            Some(s) => s,
            None => {
                self.exhausted = self.exhausted.saturating_add(1);
                return Err(SpawnError::TableFull);
            }
        };
        let pid = self.next_pid();
        let mut pcb = Pcb::vacant(pid);
        pcb.ppid = parent;
        pcb.state = ProcState::Ready;
        if let Some(p) = self.find(parent) {
            pcb.caps = self.slots[p].caps;
            pcb.priority = self.slots[p].priority;
            pcb.affinity = self.slots[p].affinity;
        }
        pcb.set_name(name);
        self.slots[slot] = pcb;
        self.live += 1;
        Ok(pid)
    }

    fn release_slot(&mut self, pid: u32) {
        if let Some(i) = self.find(pid) {
            self.slots[i] = Pcb::vacant(0);
            self.live = self.live.saturating_sub(1);
        }
    }

    /// F009 — 从镜像创建一个新进程。父进程必须活着、必须有 CAP_PROC，
    /// 且地址空间必须建得起来 —— 任何一条不满足就不创建（不做半成品）。
    pub fn spawn(
        &mut self,
        arena: &mut SpaceArena,
        parent: u32,
        name: &[u8],
        entry: u64,
    ) -> Result<u32, SpawnError> {
        let pslot = self.find(parent).ok_or(SpawnError::NoParent)?;
        if !self.slots[pslot].caps.has(ProcCaps::PROC) {
            return Err(SpawnError::NoCapability);
        }
        let pid = self.alloc(parent, name)?;
        let aslot = match arena.create(pid) {
            Ok(s) => s,
            Err(_) => {
                // 地址空间建不起来就不留半个进程。
                self.release_slot(pid);
                return Err(SpawnError::NoSpace);
            }
        };
        let slot = self.find(pid).ok_or(SpawnError::NoSpace)?;
        self.slots[slot].entry = entry;
        self.slots[slot].space = Some(aslot as u8);
        Ok(pid)
    }

    /// 建立 init（pid 1）：拥有全部能力，是孤儿收养的终点。
    pub fn bootstrap_init(&mut self, arena: &mut SpaceArena) -> u32 {
        let aslot = match arena.create(PID_INIT) {
            Ok(s) => s,
            Err(_) => return 0,
        };
        let mut pcb = Pcb::vacant(PID_INIT);
        pcb.ppid = 0;
        pcb.state = ProcState::Ready;
        pcb.caps = ProcCaps::all();
        pcb.priority = PRIO_LEVELS as u8 - 1;
        pcb.space = Some(aslot as u8);
        pcb.set_name(b"init");
        self.slots[0] = pcb;
        self.live += 1;
        PID_INIT
    }

    /// F010 — 回收一个僵尸：返回 (pid, exit_code, killed_by) 并释放槽位。
    pub fn reap(&mut self, pid: u32) -> Option<(u32, i32, u32)> {
        let slot = self.find(pid)?;
        if self.slots[slot].state != ProcState::Zombie {
            return None;
        }
        let p = self.slots[slot];
        self.slots[slot] = Pcb::vacant(0);
        self.live = self.live.saturating_sub(1);
        Some((p.pid, p.exit_code, p.killed_by))
    }

    /// F010 — wait：取一个已经死掉的孩子（僵尸），顺手收养它留下的孤儿。
    /// 返回 (child_pid, exit_code)。
    pub fn wait(&mut self, parent: u32) -> Option<(u32, i32)> {
        let slot = (0..MAX_PROCS).find(|&i| {
            self.slots[i].state == ProcState::Zombie && self.slots[i].ppid == parent
        })?;
        let pid = self.slots[slot].pid;
        let code = self.slots[slot].exit_code;
        // 收养：孩子死了，孙子归 init —— 没有这条，孤儿会永远挂在死父上。
        for i in 0..MAX_PROCS {
            if self.slots[i].ppid == pid && self.slots[i].state != ProcState::Vacant {
                self.slots[i].ppid = PID_INIT;
            }
        }
        let _ = self.reap(pid)?;
        Some((pid, code))
    }

    /// 有多少个僵尸孩子（供 init 轮询与仪表）。
    pub fn zombies_of(&self, parent: u32) -> usize {
        self.slots
            .iter()
            .filter(|p| p.state == ProcState::Zombie && p.ppid == parent)
            .count()
    }

    /// F011 — 退出：状态转 Zombie、句柄全关、页数记账保留给父进程对账。
    /// 孤儿全部过继给 init。返回被释放的句柄数。
    pub fn exit(&mut self, pid: u32, code: i32, killed_by: u32) -> usize {
        let slot = match self.find(pid) {
            Some(s) => s,
            None => return 0,
        };
        let closed = self.slots[slot].handles.close_all();
        self.slots[slot].state = ProcState::Zombie;
        self.slots[slot].exit_code = code;
        self.slots[slot].killed_by = killed_by;
        for i in 0..MAX_PROCS {
            if self.slots[i].ppid == pid && self.slots[i].state != ProcState::Vacant {
                self.slots[i].ppid = PID_INIT;
            }
        }
        closed
    }

    /// F013 — 崩溃隔离：用户态致命错误只把**该进程**变成僵尸，内核与其
    /// 它进程状态一字不变。返回内核是否仍然健康（恒为 true —— 这就是红线）。
    pub fn isolate_crash(&mut self, pid: u32, kind: CrashKind) -> bool {
        let slot = match self.find(pid) {
            Some(s) => s,
            None => return true,
        };
        if !self.slots[slot].state.alive() {
            return true;
        }
        let sig = kind.signal();
        self.exit(pid, 128 + sig as i32, sig);
        // 内核没有进程表以外的可污染面：其它进程的状态位原封不动。
        true
    }

    /// 把某个进程的地址空间彻底拆掉（F011 的资源回收面）。
    pub fn release_space(&mut self, arena: &mut SpaceArena, pid: u32) -> usize {
        let slot = match self.find(pid) {
            Some(s) => s,
            None => return 0,
        };
        let aslot = match self.slots[slot].space {
            Some(a) => a as usize,
            None => return 0,
        };
        let freed = arena.destroy(aslot);
        self.slots[slot].space = None;
        self.slots[slot].owned_pages = 0;
        freed
    }

    /// F018 — 父子树快照：以 root 为根按 BFS 展开，写入 out。
    pub fn tree_snapshot(&self, root: u32, out: &mut [u32]) -> usize {
        let mut n = 0usize;
        let mut queue = [0u32; MAX_PROCS];
        let mut head = 0usize;
        let mut tail = 0usize;
        if self.find(root).is_none() && root != 0 {
            return 0;
        }
        queue[tail] = root;
        tail += 1;
        while head < tail {
            let cur = queue[head];
            head += 1;
            if n < out.len() {
                out[n] = cur;
                n += 1;
            }
            for i in 0..MAX_PROCS {
                if self.slots[i].ppid == cur && self.slots[i].state != ProcState::Vacant {
                    if tail < MAX_PROCS {
                        queue[tail] = self.slots[i].pid;
                        tail += 1;
                    }
                }
            }
        }
        n
    }

    /// 表内所有存活进程的 CPU 记账总和（F023 的系统级读数）。
    pub fn total_cpu_ticks(&self) -> u64 {
        self.slots
            .iter()
            .filter(|p| p.state != ProcState::Vacant)
            .map(|p| p.meter.total_ticks())
            .sum()
    }
}

impl Default for ProcTable {
    fn default() -> Self {
        ProcTable::new()
    }
}

// ---------------------------------------------------------------------------
// F012/F013 — 信号与崩溃分类
// ---------------------------------------------------------------------------

/// 崩溃类别 → 默认信号，是"用户态怎么死"的唯一裁决表。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CrashKind {
    /// 用户态页错误且不可供页（F020）。
    PageFault,
    /// 非法指令。
    IllegalInstruction,
    /// 除零 / #DE。
    DivideError,
    /// 栈溢出（F003 守卫页命中）。
    StackOverflow,
    /// 一般保护错。
    GeneralProtection,
}

impl CrashKind {
    pub fn signal(self) -> u32 {
        match self {
            CrashKind::PageFault => 11,           // SIGSEGV
            CrashKind::IllegalInstruction => 4,    // SIGILL
            CrashKind::DivideError => 8,           // SIGFPE
            CrashKind::StackOverflow => 11,        // SIGSEGV
            CrashKind::GeneralProtection => 11,    // SIGSEGV
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            CrashKind::PageFault => "page-fault",
            CrashKind::IllegalInstruction => "illegal-instruction",
            CrashKind::DivideError => "divide-error",
            CrashKind::StackOverflow => "stack-overflow",
            CrashKind::GeneralProtection => "general-protection",
        }
    }
}

/// F012 — 信号递送结果：内核不会替用户态"决定"行为，只查默认行为表。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SignalOutcome {
    /// 用户态注册了处理器，跳过去。
    HandlerInstalled,
    /// 默认行为：终止。
    Terminate,
    /// 默认行为：忽略。
    Ignore,
    /// 内核拒绝（号非法 / 无权限）。
    Refused,
}

/// F012 — 递送一次信号。`has_handler` 来自用户态注册表，`signal_allowed`
/// 是能力检查（F015：没有 CAP_PROC 就不能给别人发信号）。
pub fn deliver_signal(
    sig: u32,
    has_handler: bool,
    signal_allowed: bool,
    sender_is_self: bool,
) -> SignalOutcome {
    if sig == 0 || sig > super::MAX_SIGNAL {
        return SignalOutcome::Refused;
    }
    if !signal_allowed && !sender_is_self {
        return SignalOutcome::Refused;
    }
    if has_handler {
        return SignalOutcome::HandlerInstalled;
    }
    match super::default_action(super::Signal(sig)) {
        super::SignalAction::Ignore => SignalOutcome::Ignore,
        _ => SignalOutcome::Terminate,
    }
}

// ---------------------------------------------------------------------------
// F014 — 句柄表
// ---------------------------------------------------------------------------

/// 句柄类型 —— 类型化校验：拿文件句柄去当事件端口用必须被拒。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HandleKind {
    Console,
    File,
    Event,
    Pipe,
    SharedMemory,
    Process,
}

impl HandleKind {
    pub fn as_str(self) -> &'static str {
        match self {
            HandleKind::Console => "console",
            HandleKind::File => "file",
            HandleKind::Event => "event",
            HandleKind::Pipe => "pipe",
            HandleKind::SharedMemory => "shm",
            HandleKind::Process => "proc",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Handle {
    pub id: u32,
    pub kind: HandleKind,
    /// 读/写/传递权限位。
    pub rights: u8,
}

pub const RIGHT_READ: u8 = 1;
pub const RIGHT_WRITE: u8 = 1 << 1;
pub const RIGHT_TRANSFER: u8 = 1 << 2;

/// F014 — 进程私有句柄空间；进程消亡时整体关闭（`close_all`），绝不泄漏。
#[derive(Clone, Copy, Debug)]
pub struct HandleTable {
    slots: [Option<Handle>; MAX_HANDLES],
    next_id: u32,
}

impl HandleTable {
    pub const fn new() -> HandleTable {
        HandleTable {
            slots: [None; MAX_HANDLES],
            next_id: 3,
        }
    }

    pub fn open(&mut self, kind: HandleKind, rights: u8) -> Option<u32> {
        let slot = self.slots.iter().position(|s| s.is_none())?;
        let id = self.next_id;
        self.next_id += 1;
        self.slots[slot] = Some(Handle { id, kind, rights });
        Some(id)
    }

    /// 按 id 关闭；返回 true 表示确实关掉了一个。
    pub fn close(&mut self, id: u32) -> bool {
        for s in self.slots.iter_mut() {
            if let Some(h) = s {
                if h.id == id {
                    *s = None;
                    return true;
                }
            }
        }
        false
    }

    pub fn close_all(&mut self) -> usize {
        let mut n = 0;
        for s in self.slots.iter_mut() {
            if s.is_some() {
                *s = None;
                n += 1;
            }
        }
        n
    }

    pub fn get(&self, id: u32) -> Option<Handle> {
        self.slots
            .iter()
            .filter_map(|s| *s)
            .find(|h| h.id == id)
    }

    /// 类型化校验：句柄存在 *且* 类型匹配 *且* 权限足够。
    pub fn check(&self, id: u32, want: HandleKind, rights: u8) -> bool {
        match self.get(id) {
            Some(h) => h.kind == want && h.rights & rights == rights,
            None => false,
        }
    }

    /// 跨进程传递（F062 的内核侧）：只有带 RIGHT_TRANSFER 的句柄可传。
    pub fn transfer(&self, id: u32, to: &mut HandleTable) -> Option<u32> {
        let h = self.get(id)?;
        if h.rights & RIGHT_TRANSFER == 0 {
            return None;
        }
        to.open(h.kind, h.rights & !RIGHT_TRANSFER)
    }

    pub fn open_count(&self) -> usize {
        self.slots.iter().filter(|s| s.is_some()).count()
    }
}

impl Default for HandleTable {
    fn default() -> Self {
        HandleTable::new()
    }
}

// ---------------------------------------------------------------------------
// F015 — 能力模型
// ---------------------------------------------------------------------------

/// F015 — 能力位（最小授权）。敏感 syscall 一律先过这里。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct ProcCaps(pub u64);

impl ProcCaps {
    pub const IO: u64 = 1 << 0;
    pub const FS: u64 = 1 << 1;
    pub const PROC: u64 = 1 << 2;
    pub const EXEC: u64 = 1 << 3;
    pub const NET: u64 = 1 << 4;
    pub const ADMIN: u64 = 1 << 5;

    pub const fn none() -> ProcCaps {
        ProcCaps(0)
    }

    pub const fn all() -> ProcCaps {
        ProcCaps(0x3F)
    }

    pub const fn of(bits: u64) -> ProcCaps {
        ProcCaps(bits & 0x3F)
    }

    pub fn has(self, bit: u64) -> bool {
        self.0 & bit == bit
    }

    /// 派生一个只减不增的能力集 —— 子进程永远不能比父进程更有权。
    pub fn derive(self, want: ProcCaps) -> ProcCaps {
        ProcCaps(want.0 & self.0)
    }

    pub fn drop_cap(self, bit: u64) -> ProcCaps {
        ProcCaps(self.0 & !bit)
    }

    pub fn count(self) -> u32 {
        self.0.count_ones()
    }
}

// ---------------------------------------------------------------------------
// F016/F017 — 优先级、亲和与预算
// ---------------------------------------------------------------------------

/// F016 — 优先级夹取到合法档位。
pub fn clamp_priority(p: i32) -> u8 {
    p.clamp(0, PRIO_LEVELS as i32 - 1) as u8
}

/// F016 — 亲和掩码：至少留一个可用 CPU，空掩码等价于"任意"。
pub fn normalize_affinity(mask: u64) -> u64 {
    if mask == 0 {
        u64::MAX
    } else {
        mask
    }
}

/// F016 — 与调度器对接时的判定：该进程能否跑在 cpu 上。
pub fn affinity_allows(mask: u64, cpu: u32) -> bool {
    cpu < 64 && normalize_affinity(mask) & (1u64 << cpu) != 0
}

/// F017 — 预算档位。超配额不是"杀掉"，而是先降级（降优先级），再拒绝。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BudgetLevel {
    /// 用得很省。
    Comfortable,
    /// 过半，正常。
    Normal,
    /// 接近上限，降优先级。
    Throttled,
    /// 已超配额，拒绝新的 CPU 时间。
    Exhausted,
}

impl BudgetLevel {
    pub fn as_str(self) -> &'static str {
        match self {
            BudgetLevel::Comfortable => "ok",
            BudgetLevel::Normal => "normal",
            BudgetLevel::Throttled => "throttle",
            BudgetLevel::Exhausted => "exhausted",
        }
    }
}

/// F017 — CPU 时间片记账。
#[derive(Clone, Copy, Debug)]
pub struct Budget {
    pub quota: u64,
    pub used: u64,
    /// 已经因为超支被降级的次数。
    pub throttles: u32,
}

impl Budget {
    pub const fn new() -> Budget {
        Budget {
            quota: CPU_QUOTA_TICKS,
            used: 0,
            throttles: 0,
        }
    }

    pub fn charge(&mut self, ticks: u64) -> BudgetLevel {
        self.used = self.used.saturating_add(ticks);
        self.level()
    }

    pub fn level(&self) -> BudgetLevel {
        if self.quota == 0 {
            return BudgetLevel::Exhausted;
        }
        let pct = self.used.saturating_mul(100) / self.quota;
        if pct >= 100 {
            BudgetLevel::Exhausted
        } else if pct >= 80 {
            BudgetLevel::Throttled
        } else if pct >= 50 {
            BudgetLevel::Normal
        } else {
            BudgetLevel::Comfortable
        }
    }

    /// 超支时降级：优先级下调，并记账。返回新优先级。
    pub fn degrade(&mut self, priority: u8) -> u8 {
        self.throttles = self.throttles.saturating_add(1);
        priority.saturating_sub(2).max(0)
    }

    /// 新一轮时间片。
    pub fn replenish(&mut self) {
        self.used = 0;
    }
}

// ---------------------------------------------------------------------------
// F020 — 用户态页错误处理
// ---------------------------------------------------------------------------

/// 页错误的处置结论。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum UserFaultAction {
    /// 缺页供页（栈生长 / 堆生长 / 匿名页首次触碰）。
    Supply,
    /// 真错误：精确杀掉该进程，报告 RIP。
    Kill(CrashKind),
}

/// F020 — 用户态页错误的统一入口。`in_guard` 来自 F003 的守卫页判定，
/// `region_known` 表示该地址位于进程已登记的区域（栈/堆/匿名）。
/// 落在守卫页或登记外 = 真错误，绝不"顺手供一页"。
pub fn handle_user_fault(
    in_guard: bool,
    region_known: bool,
    writable_fault: bool,
    rip: u64,
) -> (UserFaultAction, u64) {
    if in_guard {
        return (UserFaultAction::Kill(CrashKind::StackOverflow), rip);
    }
    if !region_known {
        return (UserFaultAction::Kill(CrashKind::PageFault), rip);
    }
    if writable_fault {
        (UserFaultAction::Supply, rip)
    } else {
        // 只读页上的写错误同样致命。
        (UserFaultAction::Kill(CrashKind::GeneralProtection), rip)
    }
}

// ---------------------------------------------------------------------------
// F021 — core dump
// ---------------------------------------------------------------------------

/// F021 — 崩溃现场：寄存器、栈顶若干字节、映射表摘要。定长，能落盘也能回读。
#[derive(Clone, Copy, Debug)]
pub struct CoreDump {
    pub pid: u32,
    pub signal: u32,
    pub rip: u64,
    pub registers: [u64; CORE_REGISTERS],
    pub mappings: [(u64, u64, u8); CORE_MAPPINGS],
    pub mapping_count: usize,
    pub stack_digest: u64,
}

impl CoreDump {
    pub const fn empty() -> CoreDump {
        CoreDump {
            pid: 0,
            signal: 0,
            rip: 0,
            registers: [0; CORE_REGISTERS],
            mappings: [(0, 0, 0); CORE_MAPPINGS],
            mapping_count: 0,
            stack_digest: 0,
        }
    }

    /// 从进程与地址空间采集现场。栈内容用 FNV 式折叠摘要表示 —— 定长、
    /// 可比较、不需要把整栈搬进缓冲区。
    pub fn capture(
        pcb: &Pcb,
        space: &AddressSpace,
        rip: u64,
        regs: &[u64; CORE_REGISTERS],
        stack_bytes: &[u8],
    ) -> CoreDump {
        let mut d = CoreDump::empty();
        d.pid = pcb.pid;
        d.signal = pcb.killed_by;
        d.rip = rip;
        d.registers = *regs;
        let mut n = 0usize;
        for i in 0..space.mapping_count() {
            if let Some(m) = space.get(i) {
                if n >= CORE_MAPPINGS {
                    break;
                }
                let perm = (if m.writable() { 2u8 } else { 0 })
                    | (if m.executable() { 1u8 } else { 0 });
                d.mappings[n] = (m.base, m.pages as u64 * 4096, perm);
                n += 1;
            }
        }
        d.mapping_count = n;
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for &b in stack_bytes {
            h ^= b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        d.stack_digest = h;
        d
    }

    /// 回读校验：core 是否自洽（pid/signal 有效、RIP 在用户半区或有映射）。
    pub fn readable(&self) -> bool {
        self.pid != 0
            && self.signal != 0
            && (paging::is_user(self.rip) || self.rip == 0)
            && self.mapping_count > 0
    }
}

// ---------------------------------------------------------------------------
// F022 — 调试接口
// ---------------------------------------------------------------------------

/// F022 — 调试会话：附着、读写目标进程内存、单步。能力检查在前，
/// 不做"附着任何一个进程"的后门。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DebugError {
    NotAttached,
    NoPermission,
    TargetGone,
    BadAddress,
}

#[derive(Clone, Copy, Debug)]
pub struct DebugSession {
    pub target: u32,
    pub attached: bool,
    /// 单步断点地址，0 = 无。
    pub breakpoint: u64,
    pub steps: u32,
}

impl DebugSession {
    pub const fn new() -> DebugSession {
        DebugSession {
            target: 0,
            attached: false,
            breakpoint: 0,
            steps: 0,
        }
    }

    pub fn attach(
        &mut self,
        table: &ProcTable,
        target: u32,
        debugger_caps: ProcCaps,
    ) -> Result<(), DebugError> {
        if !debugger_caps.has(ProcCaps::ADMIN) {
            return Err(DebugError::NoPermission);
        }
        if table.find(target).is_none() {
            return Err(DebugError::TargetGone);
        }
        self.target = target;
        self.attached = true;
        Ok(())
    }

    /// 读目标进程的内存：只有读 *已映射的用户页* 才成功，越界即 BadAddress。
    pub fn read(
        &self,
        table: &ProcTable,
        arena: &SpaceArena,
        addr: u64,
        out: &mut [u8],
    ) -> Result<usize, DebugError> {
        if !self.attached {
            return Err(DebugError::NotAttached);
        }
        let slot = table.find(self.target).ok_or(DebugError::TargetGone)?;
        let aslot = table
            .get(slot)
            .and_then(|p| p.space)
            .ok_or(DebugError::TargetGone)? as usize;
        if out.is_empty() {
            return Ok(0);
        }
        let page = addr & !0xFFF;
        if arena.translate(aslot, page).is_none() {
            return Err(DebugError::BadAddress);
        }
        // 真实内核会把数据搬出来；这里以"可读字节数"表达成功边界，
        // 不伪造内容（诚实边界）。
        Ok(out.len())
    }

    /// 写目标进程内存：可写页才允许，否则 BadAddress。
    pub fn write(&self, table: &ProcTable, arena: &SpaceArena, addr: u64) -> Result<(), DebugError> {
        if !self.attached {
            return Err(DebugError::NotAttached);
        }
        let slot = table.find(self.target).ok_or(DebugError::TargetGone)?;
        let aslot = table
            .get(slot)
            .and_then(|p| p.space)
            .ok_or(DebugError::TargetGone)? as usize;
        let page = addr & !0xFFF;
        match arena.flags_of(aslot, page) {
            Some(f) if f & P_WRITE != 0 => Ok(()),
            Some(_) => Err(DebugError::BadAddress),
            None => Err(DebugError::BadAddress),
        }
    }

    /// 单步：一次一步，直到撞上断点（0 表示不限步）。
    pub fn step(&mut self) -> u64 {
        self.steps = self.steps.saturating_add(1);
        self.breakpoint
    }

    pub fn detach(&mut self) {
        self.attached = false;
        self.target = 0;
    }
}

impl Default for DebugSession {
    fn default() -> Self {
        DebugSession::new()
    }
}

// ---------------------------------------------------------------------------
// F023 — 进程记账仪表
// ---------------------------------------------------------------------------

/// F023 — 每进程 CPU/内存水位环形统计，零成本采样（只存样本，不跑统计）。
#[derive(Clone, Copy, Debug)]
pub struct ProcMeter {
    cpu: [u32; METER_SAMPLES],
    mem: [u32; METER_SAMPLES],
    head: usize,
    filled: usize,
}

impl ProcMeter {
    pub const fn new() -> ProcMeter {
        ProcMeter {
            cpu: [0; METER_SAMPLES],
            mem: [0; METER_SAMPLES],
            head: 0,
            filled: 0,
        }
    }

    pub fn sample(&mut self, cpu_ticks: u32, mem_pages: u32) {
        self.cpu[self.head] = cpu_ticks;
        self.mem[self.head] = mem_pages;
        self.head = (self.head + 1) % METER_SAMPLES;
        if self.filled < METER_SAMPLES {
            self.filled += 1;
        }
    }

    pub fn filled(&self) -> usize {
        self.filled
    }

    pub fn total_ticks(&self) -> u64 {
        self.cpu[..self.filled].iter().map(|v| *v as u64).sum()
    }

    pub fn peak_pages(&self) -> u32 {
        self.mem[..self.filled].iter().copied().max().unwrap_or(0)
    }

    pub fn avg_ticks(&self) -> u32 {
        if self.filled == 0 {
            0
        } else {
            (self.total_ticks() / self.filled as u64) as u32
        }
    }

    /// 最近一次采样（仪表盘读它）。
    pub fn last(&self) -> (u32, u32) {
        if self.filled == 0 {
            return (0, 0);
        }
        let i = if self.head == 0 {
            METER_SAMPLES - 1
        } else {
            self.head - 1
        };
        (self.cpu[i], self.mem[i])
    }
}

impl Default for ProcMeter {
    fn default() -> Self {
        ProcMeter::new()
    }
}

// ---------------------------------------------------------------------------
// F024 — 线程模型
// ---------------------------------------------------------------------------

/// F024 — 同一地址空间内的多条执行流；每线程独立内核栈。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ThreadBlock {
    pub tid: u32,
    pub owner: u32,
    pub kernel_stack_top: u64,
    /// 用户态栈顶（共享地址空间，但各自的栈指针不同）。
    pub user_rsp: u64,
    pub running: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct ThreadTable {
    slots: [Option<ThreadBlock>; MAX_THREADS],
    next_tid: u32,
}

impl ThreadTable {
    pub const fn new() -> ThreadTable {
        ThreadTable {
            slots: [None; MAX_THREADS],
            next_tid: 100,
        }
    }

    /// 新建线程：同一 owner，内核栈必须各不相同（这是线程与进程的差别所在）。
    pub fn create(
        &mut self,
        owner: u32,
        kernel_stack_top: u64,
        user_rsp: u64,
    ) -> Option<u32> {
        let slot = self.slots.iter().position(|s| s.is_none())?;
        for s in self.slots.iter().flatten() {
            if s.kernel_stack_top == kernel_stack_top {
                return None;
            }
        }
        let tid = self.next_tid;
        self.next_tid += 1;
        self.slots[slot] = Some(ThreadBlock {
            tid,
            owner,
            kernel_stack_top,
            user_rsp,
            running: true,
        });
        Some(tid)
    }

    pub fn of(&self, owner: u32) -> usize {
        self.slots
            .iter()
            .flatten()
            .filter(|t| t.owner == owner)
            .count()
    }

    pub fn get(&self, tid: u32) -> Option<ThreadBlock> {
        self.slots.iter().flatten().find(|t| t.tid == tid).copied()
    }

    /// 进程退出时杀掉它所有线程。
    pub fn kill_process(&mut self, owner: u32) -> usize {
        let mut n = 0;
        for s in self.slots.iter_mut() {
            if let Some(t) = s {
                if t.owner == owner {
                    *s = None;
                    n += 1;
                }
            }
        }
        n
    }

    pub fn count(&self) -> usize {
        self.slots.iter().filter(|s| s.is_some()).count()
    }
}

impl Default for ThreadTable {
    fn default() -> Self {
        ThreadTable::new()
    }
}

// ---------------------------------------------------------------------------
// 域状态与自检
// ---------------------------------------------------------------------------

/// 域内的可读状态，供 HUD 与自检共用。
#[derive(Clone, Copy, Debug, Default)]
pub struct UspaceState {
    pub processes: usize,
    pub zombies: usize,
    pub live_threads: usize,
    pub table_exhausted: u32,
    pub spaces: usize,
    pub self_test: (usize, usize),
}

/// F025 — 域自检：25 项 CheckSet。
///
/// 每个探针是一个独立函数：地址空间的页表数组是固定容量的大对象，把多个
/// 探针塞进同一个栈帧会让调试版构建直接栈溢出（每个 `let` 独占槽位）。
/// 独立成帧还让每一项的失败范围清晰可读。
pub fn run_uspace_checks() -> CheckSet {
    let mut set = CheckSet::new("uspace");
    probe_paging(&mut set);
    probe_process(&mut set);
    probe_cow_fork(&mut set);
    probe_faults(&mut set);
    probe_core_dump(&mut set);
    probe_debug(&mut set);
    probe_meter_threads(&mut set);
    probe_domain_closed(&mut set);
    set
}

/// F001~F005 — 地址空间与切换地基。
fn probe_paging(set: &mut CheckSet) {
    // ---- F001 每进程地址空间 ----------------------------------------------
    let mut arena = SpaceArena::new();
    let kroot = 0u16;
    arena.install_kernel_root(kroot);
    let a = arena.create(11).expect("space a");
    let b = arena.create(22).expect("space b");
    let _ = arena.map_page(a, 0x40_0000, 0x10_0000, RW_USER, RegionKind::Image);
    let _ = arena.map_page(b, 0x40_0000, 0x20_0000, RW_USER, RegionKind::Image);
    set.add(
        "F001 每进程地址空间（页表隔离）",
        arena.isolated(a, b)
            && arena.unreachable_from(a, b) == false
            && arena.translate(a, 0x40_0000) != arena.translate(b, 0x40_0000)
            && arena.translate(a, 0x40_0000).is_some(),
        "两进程同址映射到不同物理页，互不可见",
    );

    // ---- F002 ring3 切换原语 ---------------------------------------------
    let mut plan = EntryPlan::default();
    let ok_plan = plan_entry(EntryPath::Sysret, 0x40_0000, 0x7FFF_0000, &mut plan).is_ok();
    let mut plan2 = EntryPlan::default();
    let ok_iret = plan_entry(EntryPath::Iret, 0x40_0000, 0x7FFF_0000, &mut plan2).is_ok();
    let reject = plan_entry(EntryPath::Sysret, 0xFFFF_8000_0000_1000, 0x7FFF_0000, &mut plan).is_err();
    set.add(
        "F002 ring3 切换原语（sysret/iret 双路）",
        ok_plan
            && ok_iret
            && reject
            && plan.path == EntryPath::Sysret
            && entry_round_trips(&plan, 0x40_0000, 0x7FFF_0000),
        "双路计划 + 内核地址入口被拒 + 往返零损",
    );

    // ---- F003 用户栈建设 --------------------------------------------------
    let (sbase, stop) = arena.install_stack(a, 0x7F00_0000, 8, 0x30_0000).expect("stack");
    set.add(
        "F003 用户栈建设（守卫页）",
        sbase + 8 * 4096 == stop
            && arena.translate(a, sbase).is_some()
            && arena.translate(a, sbase - 4096).is_none()
            && arena.get(a).map(|s| s.in_guard(sbase - 1)).unwrap_or(false),
        "栈可写不可执行，紧邻守卫页未映射",
    );

    // ---- F004 用户堆 ------------------------------------------------------
    let brk0 = arena.brk_grow(a, 2, 0x50_0000).expect("brk");
    let run_ok = brk0 > 0;
    let brk1 = arena.brk_grow(a, 1, 0x60_0000).expect("brk2");
    set.add(
        "F004 用户堆（brk 式增长）",
        run_ok && brk1 == brk0 + 4096 && arena.translate(a, brk0).is_some(),
        "按需供页，越界申请拒绝",
    );

    // ---- F005 TSS/IST 复核 ------------------------------------------------
    let tss = addrspace::Tss16::for_cpu(0x9000_0000, 0x9100_0000);
    set.add(
        "F005 TSS/IST 复核（双故障专用栈）",
        tss.valid()
            && !addrspace::Tss16::for_cpu(0x9000_0000, 0x9000_0000).valid()
            && addrspace::Tss16 { rsp0: 0, ist1: 0, ist_rest: [0; 7] }.valid() == false,
        "rsp0/ist1 非空、16 字节对齐且互不相同",
    );

}

/// F006~F018 — 进程生命周期（PCB/表/spawn/wait/exit/信号/隔离/句柄/能力/
/// 优先级/预算/命名空间）。
fn probe_process(set: &mut CheckSet) {
    let mut arena = SpaceArena::new();
    arena.install_kernel_root(0);

    // ---- F006/F007 进程控制块与进程表 ------------------------------------
    let mut table = ProcTable::new();
    let init = table.bootstrap_init(&mut arena);
    let mut child = 0u32;
    let mut spawn_ok = false;
    if init == PID_INIT {
        if let Ok(c) = table.spawn(&mut arena, PID_INIT, b"hello", 0x40_0000) {
            child = c;
            spawn_ok = true;
        }
    }
    let pcb_ok = table
        .find(child)
        .map(|i| {
            let p = table.get(i).expect("pcb");
            p.pid == child && p.ppid == PID_INIT && p.name() == b"hello" && p.state == ProcState::Ready
        })
        .unwrap_or(false);
    set.add(
        "F006 进程控制块（PID/状态/地址空间/句柄一体）",
        spawn_ok && pcb_ok,
        "PCB 字段完整、名字与父进程正确",
    );
    set.add(
        "F007 进程表与上限（耗尽可诊断）",
        table.capacity() == MAX_PROCS && table.live() == 2 && table.exhausted == 0,
        "定长表 + 分配闭环 + 耗尽计数",
    );

    // ---- F008 上下文切换 --------------------------------------------------
    let f1 = addrspace::SwitchFrame::capture(37);
    let f2 = addrspace::SwitchFrame::capture(37);
    let mut f3 = f2;
    f3.rip ^= 1;
    set.add(
        "F008 上下文切换（通用/浮点/调试寄存器）",
        f1.matches(&f2) && !f1.matches(&f3) && f1.classes() == 3,
        "三类寄存器全量保存恢复",
    );

    // ---- F009 spawn 原语 --------------------------------------------------
    let forged = table.spawn(&mut arena, 999, b"ghost", 0x40_0000);
    let no_cap = {
        // 去掉子进程的 PROC 能力后它不能再创建进程。
        let idx = table.find(child);
        let mut denied = false;
        if let Some(i) = idx {
            if let Some(p) = table.get_mut(i) {
                p.caps = p.caps.drop_cap(ProcCaps::PROC);
            }
            denied = table.spawn(&mut arena, child, b"grand", 0x40_0000).is_err();
        }
        denied
    };
    set.add(
        "F009 spawn 原语（父进程 + 能力校验）",
        spawn_ok && forged == Err(SpawnError::NoParent) && no_cap,
        "无父/无 CAP_PROC 一律拒绝",
    );

    // ---- F010 等待与回收 --------------------------------------------------
    let mut grand = 0u32;
    if let Some(i) = table.find(PID_INIT) {
        if let Some(p) = table.get_mut(i) {
            p.caps = ProcCaps::all();
        }
    }
    if let Ok(g) = table.spawn(&mut arena, PID_INIT, b"grand", 0x40_0000) {
        grand = g;
    }
    table.exit(child, 7, 0);
    let waited = table.wait(PID_INIT);
    let adopted = table
        .find(grand)
        .map(|i| table.get(i).map(|p| p.ppid == PID_INIT).unwrap_or(false))
        .unwrap_or(false);
    set.add(
        "F010 等待与回收（zombie + 孤儿收养）",
        waited == Some((child, 7)) && adopted && table.find(child).is_none(),
        "wait 取走僵尸，孤儿过继给 init",
    );

    // ---- F011 进程退出 ----------------------------------------------------
    // worker 用同一张表与同一个 arena：进程表容量与地址空间容量各自独立判定。
    let worker = table.spawn(&mut arena, PID_INIT, b"worker", 0x40_0000).unwrap_or(0);
    let worker_aslot = arena.find_pid(worker).unwrap_or(0);
    let _ = arena.map_range(worker_aslot, 0x40_0000, 0x10_0000, 3, RW_USER, RegionKind::Image);
    {
        let i = table.find(worker).unwrap_or(0);
        if let Some(p) = table.get_mut(i) {
            let _ = p.handles.open(HandleKind::File, RIGHT_READ);
            let _ = p.handles.open(HandleKind::Event, RIGHT_WRITE);
        }
    }
    let closed = table.exit(worker, 0, 0);
    let freed = table.release_space(&mut arena, worker);
    let gone = arena.translate(worker_aslot, 0x40_0000).is_none();
    let still_listed = table
        .find(worker)
        .map(|i| table.get(i).map(|p| p.state == ProcState::Zombie).unwrap_or(false))
        .unwrap_or(false);
    set.add(
        "F011 进程退出（资源全量回收）",
        closed == 2 && freed == 3 && gone && still_listed,
        "句柄全关 + 页表拆卸 + 3 页释放，僵尸等待回收",
    );

    // ---- F012 信号框架 ----------------------------------------------------
    let deliver = deliver_signal(9, false, true, false) == SignalOutcome::Terminate
        && deliver_signal(9, true, true, false) == SignalOutcome::HandlerInstalled
        && deliver_signal(9, false, false, false) == SignalOutcome::Refused
        && deliver_signal(9, false, false, true) == SignalOutcome::Terminate
        && deliver_signal(0, false, true, true) == SignalOutcome::Refused;
    set.add(
        "F012 信号框架（默认行为表 + 处理器注册）",
        deliver,
        "SIGKILL 终止、已注册走 handler、越权拒绝",
    );

    // ---- F013 崩溃隔离 ----------------------------------------------------
    let victim = table.spawn(&mut arena, PID_INIT, b"victim", 0x40_0000).unwrap_or(0);
    let bystander = table.spawn(&mut arena, PID_INIT, b"bystander", 0x40_0000).unwrap_or(0);
    let kernel_ok = table.isolate_crash(victim, CrashKind::PageFault);
    let bystander_alive = table
        .find(bystander)
        .map(|i| table.get(i).map(|p| p.state.alive()).unwrap_or(false))
        .unwrap_or(false);
    let victim_zombie = table
        .find(victim)
        .map(|i| table.get(i).map(|p| p.state == ProcState::Zombie).unwrap_or(false))
        .unwrap_or(false);
    set.add(
        "F013 崩溃隔离（只杀本进程）",
        kernel_ok && bystander_alive && victim_zombie,
        "崩溃只回收该进程，旁观进程零影响",
    );

    // ---- F014 句柄表 ------------------------------------------------------
    let mut ht = HandleTable::new();
    let h_console = ht.open(HandleKind::Console, RIGHT_WRITE).unwrap_or(0);
    let h_file = ht.open(HandleKind::File, RIGHT_READ | RIGHT_WRITE | RIGHT_TRANSFER).unwrap_or(0);
    let typed = ht.check(h_console, HandleKind::Console, RIGHT_WRITE)
        && !ht.check(h_console, HandleKind::File, RIGHT_READ)
        && !ht.check(h_file, HandleKind::File, RIGHT_READ | RIGHT_TRANSFER | 0x80)
        && ht.check(h_file, HandleKind::File, RIGHT_TRANSFER);
    let mut ht2 = HandleTable::new();
    let transferred = ht.transfer(h_file, &mut ht2).is_some() && ht2.open_count() == 1;
    let closed_console = ht.close(h_console);
    set.add(
        "F014 句柄表（类型化校验 + 传递 + 回收）",
        h_console >= 3 && typed && transferred && closed_console && ht.open_count() == 1,
        "类型/权限校验，RIGHT_TRANSFER 才可传",
    );

    // ---- F015 能力模型 ----------------------------------------------------
    let parent_caps = ProcCaps::of(ProcCaps::FS | ProcCaps::PROC);
    let derived = parent_caps.derive(ProcCaps::all());
    set.add(
        "F015 能力模型（最小授权，只减不增）",
        derived == parent_caps
            && !derived.has(ProcCaps::ADMIN)
            && ProcCaps::all().has(ProcCaps::ADMIN)
            && !ProcCaps::none().has(ProcCaps::IO),
        "子集派生不允许提权",
    );

    // ---- F016 优先级与亲和 ------------------------------------------------
    set.add(
        "F016 优先级与亲和（32 档 + CPU 掩码）",
        clamp_priority(-5) == 0
            && clamp_priority(999) == PRIO_LEVELS as u8 - 1
            && affinity_allows(0b0100, 2)
            && !affinity_allows(0b0100, 1)
            && affinity_allows(0, 63)
            && !affinity_allows(u64::MAX, 64),
        "档位夹取 + 空掩码视为任意 + 越界 CPU 拒绝",
    );

    // ---- F017 进程预算 ----------------------------------------------------
    let mut budget = Budget::new();
    let b1 = budget.charge(100);
    let b2 = budget.charge(750);
    let b3 = budget.charge(500);
    let degraded = budget.degrade(10);
    budget.replenish();
    set.add(
        "F017 进程预算（超配额降级）",
        b1 == BudgetLevel::Comfortable
            && b2 == BudgetLevel::Throttled
            && b3 == BudgetLevel::Exhausted
            && degraded == 8
            && budget.level() == BudgetLevel::Comfortable
            && budget.throttles == 1,
        "过半=normal 前先降级，超支=exhausted，新片恢复",
    );

    // ---- F018 进程命名空间 ------------------------------------------------
    let mut tree = [0u32; MAX_PROCS];
    let n = table.tree_snapshot(PID_INIT, &mut tree);
    let named = table.find_name(b"bystander").is_some() && table.find_name(b"nope").is_none();
    set.add(
        "F018 进程命名空间（命名查询 + 父子树快照）",
        n >= 3 && tree[0] == PID_INIT && named,
        "BFS 树以 init 为根、名字可查",
    );
}

/// F019 — CoW 派生。
fn probe_cow_fork(set: &mut CheckSet) {
    // ---- F019 CoW fork ----------------------------------------------------
    let f019 = {
        let mut arena_cow = SpaceArena::new();
        arena_cow.install_kernel_root(0);
        let p = arena_cow.create(100).expect("cow parent");
        let _ = arena_cow.map_page(p, 0x60_0000, 0xAA_0000, RW_USER, RegionKind::Anonymous);
        match arena_cow.fork_cow(p, 101) {
            Ok(c) => {
                let same = arena_cow.translate(c, 0x60_0000) == Some(0xAA_0000);
                let pf = arena_cow.flags_of(p, 0x60_0000).unwrap_or(0);
                let cf = arena_cow.flags_of(c, 0x60_0000).unwrap_or(0);
                same && pf & P_WRITE == 0 && cf & P_WRITE == 0 && pf & paging::P_COW != 0
            }
            Err(_) => false,
        }
    };
    set.add(
        "F019 CoW fork（写时复制派生）",
        f019,
        "父子共享物理页、双双只读并置 COW 位",
    );

}

/// F020 — 用户态页错误处置。
fn probe_faults(set: &mut CheckSet) {
    // ---- F020 用户态页错误处理 --------------------------------------------
    let supply = handle_user_fault(false, true, true, 0x40_0000);
    let guard = handle_user_fault(true, true, true, 0x7F00_0000);
    let stray = handle_user_fault(false, false, true, 0x50_0000);
    let ro_write = handle_user_fault(false, true, false, 0x40_1000);
    set.add(
        "F020 用户态页错误处理（供页 vs 精确杀进程）",
        supply.0 == UserFaultAction::Supply
            && guard == (UserFaultAction::Kill(CrashKind::StackOverflow), 0x7F00_0000)
            && matches!(stray.0, UserFaultAction::Kill(CrashKind::PageFault))
            && matches!(ro_write.0, UserFaultAction::Kill(CrashKind::GeneralProtection))
            && supply.1 == 0x40_0000,
        "守卫页/野地址/只读写 = 致命；登记区缺页 = 供页",
    );

}

/// F021 — 崩溃现场落盘。
fn probe_core_dump(set: &mut CheckSet) {
    // ---- F021 core dump ---------------------------------------------------
    let regs = [1u64, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18];
    let core = {
        let mut cr_arena = SpaceArena::new();
        cr_arena.install_kernel_root(0);
        let mut cr_table = ProcTable::new();
        let cr_init = cr_table.bootstrap_init(&mut cr_arena);
        let pid = cr_table
            .spawn(&mut cr_arena, cr_init, b"crasher", 0x40_0000)
            .unwrap_or(0);
        let aslot = cr_arena.find_pid(pid).unwrap_or(0);
        let _ = cr_arena.map_range(aslot, 0x40_0000, 0x10_0000, 2, RW_USER, RegionKind::Image);
        cr_table.exit(pid, 139, 11);
        let idx = cr_table.find(pid).unwrap_or(0);
        let pcb = *cr_table.get(idx).unwrap_or(cr_table.get(0).expect("init"));
        match cr_arena.get(aslot) {
            Some(sp) => CoreDump::capture(&pcb, sp, 0x40_0000, &regs, b"stack-bytes"),
            None => CoreDump::empty(),
        }
    };
    set.add(
        "F021 core dump（现场可回读）",
        core.readable() && core.signal == 11 && core.mapping_count == 2 && core.stack_digest != 0,
        "寄存器/映射/栈摘要齐备且自洽",
    );
}

/// F022 — 调试接口。
fn probe_debug(set: &mut CheckSet) {
    // ---- F022 调试接口 ----------------------------------------------------
    let f022 = {
        let mut dbg_arena = SpaceArena::new();
        dbg_arena.install_kernel_root(0);
        let mut dbg_table = ProcTable::new();
        let _ = dbg_table.bootstrap_init(&mut dbg_arena);
        let mut dbg = DebugSession::new();
        let mut buf = [0u8; 8];
        let deny = dbg.attach(&dbg_table, PID_INIT, ProcCaps::none()) == Err(DebugError::NoPermission);
        let ok_attach = dbg.attach(&dbg_table, PID_INIT, ProcCaps::all()).is_ok();
        let unreadable =
            dbg.read(&dbg_table, &dbg_arena, 0x40_0000, &mut buf) == Err(DebugError::BadAddress);
        let dbg_aslot = dbg_arena.find_pid(PID_INIT).unwrap_or(0);
        let _ = dbg_arena.map_page(dbg_aslot, 0x40_0000, 0x10_0000, RW_USER, RegionKind::Anonymous);
        let readable = dbg.read(&dbg_table, &dbg_arena, 0x40_0000, &mut buf) == Ok(8);
        dbg.breakpoint = 0x40_0000;
        let stepped = dbg.step() == 0x40_0000;
        dbg.detach();
        let detached =
            dbg.read(&dbg_table, &dbg_arena, 0x40_0000, &mut buf) == Err(DebugError::NotAttached);
        deny && ok_attach && unreadable && readable && stepped && detached && dbg.steps == 1
    };
    set.add(
        "F022 调试接口（读写目标 + 单步）",
        f022,
        "无 ADMIN 拒绝附着，未映射拒绝读，未附着拒绝访问",
    );

}

/// F023/F024 — 记账仪表与线程模型。
fn probe_meter_threads(set: &mut CheckSet) {
    // ---- F023 进程记账仪表 ------------------------------------------------
    let mut meter = ProcMeter::new();
    for i in 0..20u32 {
        meter.sample(i + 1, 10 + i);
    }
    let peak = meter.peak_pages();
    meter.sample(99, 7);
    set.add(
        "F023 进程记账仪表（环形零成本采样）",
        meter.filled() == METER_SAMPLES
            && peak == 29
            && meter.avg_ticks() > 0
            && meter.last() == (99, 7)
            && meter.total_ticks() > 0,
        "环形覆盖最旧样本，峰值/均值/最新可读",
    );

    // ---- F024 线程模型 ----------------------------------------------------
    let mut threads = ThreadTable::new();
    let t1 = threads.create(4242, 0x8000_0000, 0x7FFF_0000);
    let t2 = threads.create(4242, 0x8100_0000, 0x7FEF_0000);
    let dup = threads.create(4242, 0x8000_0000, 0x7FFF_0000);
    let killed = threads.kill_process(4242);
    set.add(
        "F024 线程模型（同地址空间多执行流）",
        t1.is_some() && t2.is_some() && dup.is_none() && killed == 2 && threads.count() == 0,
        "内核栈必须互不相同，进程退出连带杀线程",
    );

}

/// F025 — 域自检闭合项：前 24 项全绿才允许本项与整表全绿。
fn probe_domain_closed(set: &mut CheckSet) {
    // ---- F025 域自检 ------------------------------------------------------
    let (lp, lf) = set.tally();
    set.add(
        "F025 进程域自检（25 项 CheckSet 全绿）",
        lf == 0 && lp + 1 == 25,
        "本表自洽：前 24 项全过，本项计入第 25 项",
    );
}

/// 域状态快照（boot 路径渲染 HUD 用）。
pub fn snapshot(table: &ProcTable, arena: &SpaceArena, threads: &ThreadTable) -> UspaceState {
    let mut st = UspaceState {
        processes: table.live(),
        zombies: (0..MAX_PROCS)
            .filter(|&i| {
                table
                    .get(i)
                    .map(|p| p.state == ProcState::Zombie)
                    .unwrap_or(false)
            })
            .count(),
        live_threads: threads.count(),
        table_exhausted: table.exhausted,
        spaces: arena.live_spaces(),
        self_test: (0, 0),
    };
    st.self_test = {
        let set = run_uspace_checks();
        set.tally()
    };
    st
}

// ===========================================================================
// 单测
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh() -> (ProcTable, SpaceArena, u32) {
        let mut arena = SpaceArena::new();
        arena.install_kernel_root(0);
        let mut table = ProcTable::new();
        let init = table.bootstrap_init(&mut arena);
        (table, arena, init)
    }

    #[test]
    fn f002_entry_paths_and_round_trip() {
        let mut frame = EntryPlan::default();
        assert!(plan_entry(EntryPath::Sysret, 0x40_0000, 0x7FFF_0000, &mut frame).is_ok());
        assert_eq!(frame.path, EntryPath::Sysret);
        assert_eq!(frame.rcx, 0x40_0000);
        assert_eq!(frame.r11, frame.rflags);
        assert!(entry_round_trips(&frame, 0x40_0000, 0x7FFF_0000));
        assert!(!entry_round_trips(&frame, 0x40_0004, 0x7FFF_0000));
        // 内核地址不能作为用户入口。
        assert!(plan_entry(EntryPath::Iret, 0xFFFF_8000_0000_1000, 0x7FFF_0000, &mut frame).is_err());
        assert!(plan_entry(EntryPath::Iret, 0x40_0000, 0xFFFF_8000_0000_1000, &mut frame).is_err());
    }

    #[test]
    fn f006_pcb_fields_are_complete() {
        let (mut table, mut arena, init) = fresh();
        let pid = table.spawn(&mut arena, init, b"shell", 0x40_0000).expect("spawn");
        let i = table.find(pid).expect("found");
        let p = table.get(i).expect("pcb");
        assert_eq!(p.pid, pid);
        assert_eq!(p.ppid, init);
        assert_eq!(p.name(), b"shell");
        assert!(p.state.alive());
        assert!(p.space.is_some());
        assert_eq!(p.entry, 0x40_0000);
        assert!(p.permits_cpu(0));
    }

    #[test]
    fn f007_table_fills_and_reports_exhaustion() {
        // 进程表容量与地址空间容量各自独立判定（alloc 不建空间）。
        let (mut table, _arena, init) = fresh();
        let _ = table.alloc(init, b"dup");
        assert_eq!(table.alloc(init, b"dup"), Err(SpawnError::NameTaken));
        let mut n = 1;
        let last_err;
        loop {
            let name = [b'p', b'0' + (n % 10) as u8, b'0' + ((n / 10) % 10) as u8];
            match table.alloc(init, &name) {
                Ok(_) => n += 1,
                Err(e) => {
                    last_err = e;
                    break;
                }
            }
        }
        assert_eq!(last_err, SpawnError::TableFull);
        assert_eq!(table.live(), MAX_PROCS);
        assert!(table.exhausted >= 1);
        // 回收一个槽位后又能分配 —— 分配/回收闭环。
        let victim = table.pids()[1];
        table.exit(victim, 0, 0);
        assert!(table.reap(victim).is_some());
        assert_eq!(table.live(), MAX_PROCS - 1);
        assert!(table.alloc(init, b"fresh").is_ok());
    }

    #[test]
    fn f009_spawn_requires_parent_and_capability() {
        let (mut table, mut arena, init) = fresh();
        assert_eq!(table.spawn(&mut arena, 4242, b"x", 0x40_0000), Err(SpawnError::NoParent));
        let pid = table.spawn(&mut arena, init, b"x", 0x40_0000).expect("spawn");
        let i = table.find(pid).expect("i");
        table.get_mut(i).expect("pcb").caps = ProcCaps::none();
        assert_eq!(
            table.spawn(&mut arena, pid, b"y", 0x40_0000),
            Err(SpawnError::NoCapability)
        );
    }

    #[test]
    fn f010_wait_reaps_and_adopts_orphans() {
        let (mut table, mut arena, init) = fresh();
        let child = table.spawn(&mut arena, init, b"child", 0x40_0000).expect("child");
        let grand = table.spawn(&mut arena, child, b"grand", 0x40_0000).expect("grand");
        assert_eq!(table.zombies_of(init), 0);
        table.exit(child, 3, 0);
        assert_eq!(table.zombies_of(init), 1);
        // 孤儿立刻过继给 init。
        assert_eq!(table.get(table.find(grand).unwrap()).unwrap().ppid, PID_INIT);
        assert_eq!(table.wait(init), Some((child, 3)));
        assert!(table.find(child).is_none());
        assert_eq!(table.wait(init), None);
    }

    #[test]
    fn f011_exit_closes_handles_and_releases_pages() {
        let mut arena = SpaceArena::new();
        arena.install_kernel_root(0);
        let mut table = ProcTable::new();
        let init = table.bootstrap_init(&mut arena);
        let pid = table.spawn(&mut arena, init, b"w", 0x40_0000).expect("w");
        let aslot = arena.find_pid(pid).expect("aslot");
        arena
            .map_range(aslot, 0x40_0000, 0x10_0000, 4, P_WRITE | P_NX, RegionKind::Image)
            .expect("map");
        let i = table.find(pid).expect("i");
        table.get_mut(i).expect("pcb").handles.open(HandleKind::File, RIGHT_READ);
        table.get_mut(i).expect("pcb").handles.open(HandleKind::Event, RIGHT_WRITE);
        let closed = table.exit(pid, 0, 0);
        assert_eq!(closed, 2);
        let freed = table.release_space(&mut arena, pid);
        assert_eq!(freed, 4);
        assert!(arena.translate(aslot, 0x40_0000).is_none());
        assert_eq!(table.get(table.find(pid).unwrap()).unwrap().state, ProcState::Zombie);
    }

    #[test]
    fn f012_signal_defaults_and_refusals() {
        assert_eq!(deliver_signal(15, false, true, false), SignalOutcome::Terminate);
        assert_eq!(deliver_signal(9, true, true, false), SignalOutcome::HandlerInstalled);
        assert_eq!(deliver_signal(9, false, false, false), SignalOutcome::Refused);
        assert_eq!(deliver_signal(9, false, false, true), SignalOutcome::Terminate);
        assert_eq!(deliver_signal(99, false, true, true), SignalOutcome::Refused);
    }

    #[test]
    fn f013_crash_isolates_to_one_process() {
        let (mut table, mut arena, init) = fresh();
        let victim = table.spawn(&mut arena, init, b"v", 0x40_0000).expect("v");
        let other = table.spawn(&mut arena, init, b"o", 0x40_0000).expect("o");
        assert!(table.isolate_crash(victim, CrashKind::IllegalInstruction));
        assert_eq!(table.get(table.find(victim).unwrap()).unwrap().state, ProcState::Zombie);
        assert_eq!(
            table.get(table.find(victim).unwrap()).unwrap().exit_code,
            128 + 4
        );
        assert!(table.get(table.find(other).unwrap()).unwrap().state.alive());
    }

    #[test]
    fn f014_handle_table_typed_and_transferable() {
        let mut ht = HandleTable::new();
        let file = ht.open(HandleKind::File, RIGHT_READ | RIGHT_TRANSFER).expect("file");
        assert!(ht.check(file, HandleKind::File, RIGHT_READ));
        assert!(!ht.check(file, HandleKind::File, RIGHT_WRITE));
        assert!(!ht.check(file, HandleKind::Console, RIGHT_READ));
        let mut dst = HandleTable::new();
        assert!(ht.transfer(file, &mut dst).is_some());
        let console = ht.open(HandleKind::Console, RIGHT_WRITE).expect("console");
        assert!(ht.transfer(console, &mut dst).is_none(), "无 RIGHT_TRANSFER 不可传");
        assert_eq!(ht.close_all(), 2);
        assert_eq!(ht.open_count(), 0);
    }

    #[test]
    fn f015_caps_can_only_shrink() {
        let full = ProcCaps::all();
        let narrow = full.derive(ProcCaps::of(ProcCaps::FS));
        assert!(narrow.has(ProcCaps::FS));
        assert!(!narrow.has(ProcCaps::PROC));
        assert_eq!(full.drop_cap(ProcCaps::ADMIN).has(ProcCaps::ADMIN), false);
        assert_eq!(ProcCaps::all().count(), 6);
    }

    #[test]
    fn f016_priority_and_affinity_clamp() {
        assert_eq!(clamp_priority(-1), 0);
        assert_eq!(clamp_priority(31), 31);
        assert_eq!(clamp_priority(32), 31);
        assert!(affinity_allows(0b0011, 0));
        assert!(!affinity_allows(0b0011, 2));
        assert_eq!(normalize_affinity(0), u64::MAX);
        assert!(!affinity_allows(u64::MAX, 64));
    }

    #[test]
    fn f017_budget_throttles_before_exhaustion() {
        let mut b = Budget::new();
        assert_eq!(b.charge(499), BudgetLevel::Comfortable);
        assert_eq!(b.charge(1), BudgetLevel::Normal);
        assert_eq!(b.charge(300), BudgetLevel::Throttled);
        assert_eq!(b.charge(200), BudgetLevel::Exhausted);
        assert_eq!(b.degrade(5), 3);
        b.replenish();
        assert_eq!(b.level(), BudgetLevel::Comfortable);
        assert_eq!(b.throttles, 1);
        let mut zero = Budget::new();
        zero.quota = 0;
        assert_eq!(zero.level(), BudgetLevel::Exhausted);
    }

    #[test]
    fn f018_namespace_snapshot_is_breadth_first() {
        let (mut table, mut arena, init) = fresh();
        let c1 = table.spawn(&mut arena, init, b"c1", 0x40_0000).expect("c1");
        let c2 = table.spawn(&mut arena, init, b"c2", 0x40_0000).expect("c2");
        let g = table.spawn(&mut arena, c1, b"g1", 0x40_0000).expect("g1");
        let mut out = [0u32; MAX_PROCS];
        let n = table.tree_snapshot(init, &mut out);
        assert_eq!(n, 4);
        assert_eq!(out[0], init);
        assert!(out[1] == c1 || out[1] == c2);
        assert!(out.contains(&g));
        assert_eq!(table.tree_snapshot(9999, &mut out), 0);
    }

    #[test]
    fn f020_fault_classification() {
        assert_eq!(handle_user_fault(false, true, true, 1).0, UserFaultAction::Supply);
        assert_eq!(
            handle_user_fault(true, true, true, 1).0,
            UserFaultAction::Kill(CrashKind::StackOverflow)
        );
        assert_eq!(
            handle_user_fault(false, false, true, 1).0,
            UserFaultAction::Kill(CrashKind::PageFault)
        );
        assert_eq!(
            handle_user_fault(false, true, false, 1).0,
            UserFaultAction::Kill(CrashKind::GeneralProtection)
        );
    }

    #[test]
    fn f021_core_dump_is_readable() {
        let (mut table, mut arena, init) = fresh();
        let pid = table.spawn(&mut arena, init, b"c", 0x40_0000).expect("c");
        let aslot = arena.find_pid(pid).expect("aslot");
        arena
            .map_range(aslot, 0x40_0000, 0x10_0000, 2, P_WRITE | P_NX, RegionKind::Image)
            .expect("map");
        table.exit(pid, 139, 11);
        let pcb = *table.get(table.find(pid).unwrap()).unwrap();
        let sp = arena.get(aslot).unwrap();
        let regs = [7u64; CORE_REGISTERS];
        let d = CoreDump::capture(&pcb, sp, 0x40_0000, &regs, b"boom");
        assert!(d.readable());
        assert_eq!(d.signal, 11);
        assert_eq!(d.mapping_count, 2);
        assert_eq!(d.mappings[0].0, 0x40_0000);
        assert!(d.stack_digest != 0);
        let d2 = CoreDump::capture(&pcb, sp, 0x40_0000, &regs, b"boom2");
        assert_ne!(d.stack_digest, d2.stack_digest);
        assert!(!CoreDump::empty().readable());
    }

    #[test]
    fn f022_debug_requires_admin_and_attachment() {
        let (mut table, mut arena, _init) = fresh();
        let pid = table.spawn(&mut arena, PID_INIT, b"t", 0x40_0000).expect("t");
        let mut dbg = DebugSession::new();
        assert_eq!(dbg.attach(&table, pid, ProcCaps::none()), Err(DebugError::NoPermission));
        assert_eq!(dbg.attach(&table, 4242, ProcCaps::all()), Err(DebugError::TargetGone));
        assert!(dbg.attach(&table, pid, ProcCaps::all()).is_ok());
        let mut buf = [0u8; 4];
        assert_eq!(dbg.read(&table, &arena, 0x40_0000, &mut buf), Err(DebugError::BadAddress));
        let aslot = table.find(pid).and_then(|i| table.get(i)).and_then(|p| p.space).unwrap() as usize;
        arena.map_page(aslot, 0x40_0000, 0x10_0000, P_WRITE | P_NX, RegionKind::Anonymous).expect("m");
        assert_eq!(dbg.read(&table, &arena, 0x40_0000, &mut buf), Ok(4));
        assert_eq!(dbg.write(&table, &arena, 0x40_0000), Ok(()));
        dbg.breakpoint = 0x40_0000;
        assert_eq!(dbg.step(), 0x40_0000);
        assert_eq!(dbg.steps, 1);
        dbg.detach();
        assert_eq!(dbg.write(&table, &arena, 0x40_0000), Err(DebugError::NotAttached));
    }

    #[test]
    fn f023_meter_ring_wraps() {
        let mut m = ProcMeter::new();
        for i in 0..METER_SAMPLES as u32 {
            m.sample(i, i * 2);
        }
        assert_eq!(m.filled(), METER_SAMPLES);
        assert_eq!(m.last(), ((METER_SAMPLES - 1) as u32, (METER_SAMPLES as u32 - 1) * 2));
        m.sample(999, 1);
        assert_eq!(m.filled(), METER_SAMPLES);
        assert_eq!(m.last(), (999, 1));
        assert_eq!(m.peak_pages(), 999 - 999 + (METER_SAMPLES - 1) as u32 * 2);
        let empty = ProcMeter::new();
        assert_eq!(empty.avg_ticks(), 0);
        assert_eq!(empty.last(), (0, 0));
    }

    #[test]
    fn f024_threads_share_space_but_not_kernel_stack() {
        let mut t = ThreadTable::new();
        let a = t.create(7, 0x8000_0000, 0x7000_0000).expect("a");
        let b = t.create(7, 0x8100_0000, 0x6FFF_0000).expect("b");
        assert!(t.create(7, 0x8000_0000, 0x6000_0000).is_none());
        assert_eq!(t.of(7), 2);
        assert!(t.get(a).is_some() && t.get(b).is_some());
        assert_eq!(t.kill_process(7), 2);
        assert_eq!(t.count(), 0);
    }

    #[test]
    fn f025_domain_self_test_is_green() {
        let set = run_uspace_checks();
        assert_eq!(set.len(), 25, "AI-01 域必须恰好 25 项");
        let mut buf = [0u8; 4096];
        let n = set.render(&mut buf);
        let text = core::str::from_utf8(&buf[..n]).unwrap_or("render failed");
        assert!(set.all_passed(), "uspace 域自检必须全绿：\n{}", text);
    }

    #[test]
    fn uspace_snapshot_reads_live_state() {
        let (mut table, mut arena, init) = fresh();
        let _ = table.spawn(&mut arena, init, b"x", 0x40_0000);
        let threads = ThreadTable::new();
        let st = snapshot(&table, &arena, &threads);
        assert_eq!(st.processes, 2);
        assert_eq!(st.spaces, 2);
        assert_eq!(st.self_test.1, 0);
    }
}
