//! UNX-F9299 进程族 E 型与 glibc fork/exec 错误路径消费核 · 0x7B840段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 F9299 逐字）：
//!   UNX-F9299-J1 fork 返回 -1 errno 透传判据过，exec 失败子进程 _exit(127)
//!   惯例格过，消费号登记格过。
//!
//! 设计要点：
//!   ① 三层透传链 raw→libc→app：每层都必须保持 (ret == -1, errno == 原值)。
//!      链上任何一层「好心」把 ENOMEM 改成 EAGAIN、或把 -1 变成 -errno，
//!      都算重映射。判据比对的是**三层逐层相等**，而不是只看最外层。
//!   ② fork 成功的三分支：子 pid == 0、父 pid > 0、失败 pid == -1 且 errno 非零。
//!      成功分支的 errno 必须保持未被污染（调用前 errno 值不许被 fork 改动）——
//!      这条比「失败时 errno 正确」更容易被忽略，却是 errno 语义的一半。
//!   ③ exec 失败惯例：子进程**必须** _exit(127)，不允许 return 回父代码、不允许
//!      exit(1)、不允许被信号终止。127 是「命令不存在/不可执行」的 shell 约定，
//!      与 errno 的具体值无关——errno 只进诊断，不进退出码。
//!   ④ wait 状态按位解码：低 7 位为 0 才是正常退出，退出码在次低字节；
//!      故 _exit(127) 的状态是 0x7F00。判据同时钉住「不是 0x0100」「不是信号」。
//!   ⑤ 消费号登记：号从 1 起连续分配，注销后**不回收**（next_no 单调），
//!      重名 EEXIST、满册 ENOMEM、注销未知 ESRCH。回收号会让历史日志指向
//!      新的消费者，是审计面最忌讳的一类复用。
//!   ⑥ 重试面按 errno 分层：EAGAIN/ENOMEM/EINTR 可重试，ENOEXEC/ENOENT/EACCES/
//!      E2BIG 不可重试（exec 语义失败重试只会重复失败）。双阶段位图：7 种故障
//!      × {fork, exec} 各得 7 位命中图，两图全 1 才算错误路径矩阵一次跑通。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use crate::checks::CheckSet;

/// 诊断段基址（F9299 独占 0x7B840..=0x7B87F）。
pub const SEG_BASE: u32 = 0x7B840;
/// 段窗口宽度（下一张卡 F9300 从 0x7B880 起）。
pub const SEG_SPAN: u32 = 0x40;
/// 透传链层数：raw → libc → app。
pub const CHAIN_LAYERS: usize = 3;
/// 故障行数（7 种 errno）。
pub const FAULT_ROWS: usize = 7;
/// 消费号册容量。
pub const MAX_CONSUMERS: usize = 8;
/// exec 失败的子进程退出码。
pub const EXIT_NOT_EXEC: u8 = 127;
/// 「一般失败」对照码（必须被判据排除）。
pub const EXIT_GENERIC_FAIL: u8 = 1;

// ── Linux errno 数值 ─────────────────────────────────────────────────
pub const EPERM: i32 = 1;
pub const ENOENT: i32 = 2;
pub const ESRCH: i32 = 3;
pub const EINTR: i32 = 4;
pub const E2BIG: i32 = 7;
pub const ENOEXEC: i32 = 8;
pub const ECHILD: i32 = 10;
pub const EAGAIN: i32 = 11;
pub const ENOMEM: i32 = 12;
pub const EACCES: i32 = 13;
pub const EEXIST: i32 = 17;
pub const EINVAL: i32 = 22;

/// 进程角色。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Role {
    Parent,
    Child,
}

/// 单次观测：返回槽 + errno。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Observed {
    pub ret: i64,
    pub errno: i32,
}

impl Observed {
    /// 失败观测：-1 加 errno。
    pub const fn failed(errno: i32) -> Observed {
        Observed { ret: -1, errno }
    }

    /// 成功观测：返回 pid，errno 保持 0（未被污染）。
    pub const fn succeeded(pid: i64) -> Observed {
        Observed { ret: pid, errno: 0 }
    }

    /// 是否失败形态。
    pub fn is_failure(&self) -> bool {
        self.ret == -1 && self.errno > 0
    }
}

// ── 三层透传链 ────────────────────────────────────────────────────────
/// raw → libc → app 三层观测。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Chain {
    pub raw: Observed,
    pub libc: Observed,
    pub app: Observed,
}

impl Chain {
    /// 三层是否逐层相同。
    pub fn uniform(&self) -> bool {
        self.raw == self.libc && self.libc == self.app
    }

    /// 是否存在某一层把 errno 改成了别的值（重映射）。
    pub fn remapped(&self) -> bool {
        self.raw.errno != self.libc.errno || self.libc.errno != self.app.errno
    }

    /// 是否出现「把 -1 换成 -errno」的表示漂移。
    pub fn denegated(&self) -> bool {
        self.raw.ret != self.libc.ret || self.libc.ret != self.app.ret
    }
}

/// 构造一次 fork 失败的三层链：每层都原样透传。
pub fn fork_failure(errno: i32) -> Chain {
    let o = Observed::failed(errno);
    Chain { raw: o, libc: o, app: o }
}

/// 透传判据：三层都是 -1、errno 非零且三层相等。
pub fn passthrough_ok(chain: &Chain) -> bool {
    chain.uniform() && chain.raw.is_failure() && !chain.remapped() && !chain.denegated()
}

/// fork 成功判据：子 pid == 0，父 pid > 0，两者 errno 均为 0。
pub fn fork_success_ok(child: &Observed, parent: &Observed) -> bool {
    child.ret == 0 && parent.ret > 0 && child.errno == 0 && parent.errno == 0
}

// ── exec 失败惯例与 wait 状态 ────────────────────────────────────────
/// 子进程在 exec 失败后的终局。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ExecOutcome {
    /// exec 失败时的 errno（只进诊断，不进退出码）
    pub errno: i32,
    /// exec 是否返回到子进程代码（必须为 false）
    pub returned: bool,
    /// 子进程退出码（必须为 127）
    pub exit_code: u8,
    /// 父进程是否已回收（必须为 true，否则是 zombi）
    pub reaped: bool,
}

/// 按 `_exit(127)` 惯例构造 exec 失败终局。
pub const fn exec_failure(errno: i32) -> ExecOutcome {
    ExecOutcome { errno, returned: false, exit_code: EXIT_NOT_EXEC, reaped: true }
}

/// 退出码 → wait 状态（低字节=0、次低字节=退出码）。
pub fn wait_status_of(code: u8) -> u32 {
    (code as u32) << 8
}

/// wait 状态是否「正常退出」（低 7 位为 0）。
pub fn wait_exited(status: u32) -> bool {
    status & 0x7F == 0
}

/// wait 状态是否「被信号终止」（低 7 位非 0）。
pub fn wait_signaled(status: u32) -> bool {
    status & 0x7F != 0
}

/// 取退出码；非正常退出回 0（调用方须先用 wait_exited 判定）。
pub fn wait_exit_code(status: u32) -> u8 {
    if wait_exited(status) { ((status >> 8) & 0xFF) as u8 } else { 0 }
}

// ── 故障表与重试面 ────────────────────────────────────────────────────
/// 故障阶段。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stage {
    Fork,
    Exec,
}

impl Stage {
    pub fn code(self) -> u8 {
        match self { Stage::Fork => 1, Stage::Exec => 2 }
    }
}

/// 一行故障：errno、发生阶段、是否可重试。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FaultRow {
    pub errno: i32,
    pub stage: Stage,
    pub retryable: bool,
}

/// 7 行冻结故障表。可重试集合只含「资源暂缺/被信号打断」三类：
/// 重试 exec 的 ENOEXEC/ENOENT/EACCES/E2BIG 只会重复失败，故不可重试。
pub const FAULTS: [FaultRow; FAULT_ROWS] = [
    FaultRow { errno: EAGAIN, stage: Stage::Fork, retryable: true },
    FaultRow { errno: ENOMEM, stage: Stage::Fork, retryable: true },
    FaultRow { errno: EINTR, stage: Stage::Fork, retryable: true },
    FaultRow { errno: ENOEXEC, stage: Stage::Exec, retryable: false },
    FaultRow { errno: ENOENT, stage: Stage::Exec, retryable: false },
    FaultRow { errno: EACCES, stage: Stage::Exec, retryable: false },
    FaultRow { errno: E2BIG, stage: Stage::Exec, retryable: false },
];

/// errno 是否可重试（与表内 retryable 列必须逐行一致）。
pub fn retryable(errno: i32) -> bool {
    errno == EAGAIN || errno == ENOMEM || errno == EINTR
}

/// 重试建议（诊断文案用，不参与判定）。
pub fn retry_hint(errno: i32) -> &'static str {
    if retryable(errno) {
        "可重试：先回收子进程或等信号处理完再重发"
    } else {
        "不可重试：语义性失败，重发只会重复失败；请改走回退路径"
    }
}

/// 两阶段位图：第 i 位对应 FAULTS[i]，位为 1 表示该行在该阶段判定通过。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct StageBits {
    pub fork: u8,
    pub exec: u8,
}

impl StageBits {
    pub const fn zero() -> StageBits {
        StageBits { fork: 0, exec: 0 }
    }

    /// 两张 7 位图是否都满（bit0..bit6 全 1）。
    pub fn full(&self) -> bool {
        self.fork == 0x7F && self.exec == 0x7F
    }

    /// 某一阶段是否命中第 i 行。
    pub fn hit(&self, stage: Stage, i: usize) -> bool {
        if i >= FAULT_ROWS { return false; }
        let mask = 1u8 << (i as u32);
        match stage {
            Stage::Fork => self.fork & mask != 0,
            Stage::Exec => self.exec & mask != 0,
        }
    }
}

/// 逐行跑两阶段故障矩阵。
///
/// fork 阶段：三层透传且 errno 未被改；
/// exec 阶段：子进程 _exit(127) 且 wait 状态为 0x7F00、不是 1、不是信号。
pub fn run_stage_matrix() -> StageBits {
    let mut bits = StageBits::zero();
    let mut i = 0usize;
    while i < FAULTS.len() {
        if let Some(row) = FAULTS.get(i) {
            let chain = fork_failure(row.errno);
            let fork_ok = passthrough_ok(&chain)
                && chain.app.errno == row.errno
                && retryable(row.errno) == row.retryable;
            if fork_ok {
                bits.fork |= 1u8 << (i as u32);
            }
            let outcome = exec_failure(row.errno);
            let status = wait_status_of(outcome.exit_code);
            let exec_ok = outcome.exit_code == EXIT_NOT_EXEC
                && !outcome.returned
                && outcome.reaped
                && wait_exited(status)
                && !wait_signaled(status)
                && wait_exit_code(status) == EXIT_NOT_EXEC
                && status != wait_status_of(EXIT_GENERIC_FAIL);
            if exec_ok {
                bits.exec |= 1u8 << (i as u32);
            }
        }
        i += 1;
    }
    bits
}

// ── 消费号登记册 ──────────────────────────────────────────────────────
/// 消费者类别。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ConsumerKind {
    /// 命令解释器（依赖 127 惯例）
    Shell,
    /// posix_spawn 类封装（依赖 errno 透传）
    Spawn,
    /// popen 类管道封装
    Popen,
    /// 进程监管器（依赖 wait 语义）
    Supervisor,
    /// 内核测试桩（在用户态不依赖任何惯例，作为对照）
    Ktest,
}

impl ConsumerKind {
    pub fn tag(self) -> u8 {
        match self {
            ConsumerKind::Shell => 1,
            ConsumerKind::Spawn => 2,
            ConsumerKind::Popen => 3,
            ConsumerKind::Supervisor => 4,
            ConsumerKind::Ktest => 5,
        }
    }
}

/// 一个已登记消费者。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Consumer {
    pub no: u16,
    pub name: &'static str,
    pub kind: ConsumerKind,
    pub needs_passthrough: bool,
    pub needs_exit127: bool,
}

impl Consumer {
    /// 依赖位：bit0 = 依赖 errno 透传，bit1 = 依赖 127 惯例。
    pub fn deps(&self) -> u8 {
        let mut d = 0u8;
        if self.needs_passthrough { d |= 1; }
        if self.needs_exit127 { d |= 2; }
        d
    }
}

/// 定容消费号册：号只增不减，注销留洞不回收。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Registry {
    slots: [Option<Consumer>; MAX_CONSUMERS],
    n: usize,
    next_no: u16,
    retired: u32,
    refused: u32,
}

impl Registry {
    pub const fn new() -> Registry {
        Registry { slots: [None; MAX_CONSUMERS], n: 0, next_no: 1, retired: 0, refused: 0 }
    }

    pub fn active(&self) -> usize { self.n }
    pub fn next_no(&self) -> u16 { self.next_no }
    pub fn retired(&self) -> u32 { self.retired }
    pub fn refused(&self) -> u32 { self.refused }

    /// 第 i 槽（可能是洞）。
    pub fn slot(&self, i: usize) -> Option<Consumer> {
        match self.slots.get(i) { Some(s) => *s, None => None }
    }

    /// 按名字查。
    pub fn find(&self, name: &'static str) -> Option<Consumer> {
        let mut i = 0usize;
        while i < MAX_CONSUMERS {
            if let Some(Some(c)) = self.slots.get(i) {
                if c.name == name { return Some(*c); }
            }
            i += 1;
        }
        None
    }

    /// 按号查。
    pub fn by_no(&self, no: u16) -> Option<Consumer> {
        let mut i = 0usize;
        while i < MAX_CONSUMERS {
            if let Some(Some(c)) = self.slots.get(i) {
                if c.no == no { return Some(*c); }
            }
            i += 1;
        }
        None
    }

    /// 登记。重名 EEXIST → 满册 ENOMEM → 分配 next_no。
    pub fn register(
        &mut self,
        name: &'static str,
        kind: ConsumerKind,
        needs_passthrough: bool,
        needs_exit127: bool,
    ) -> Result<u16, i32> {
        if self.find(name).is_some() {
            self.refused = self.refused.saturating_add(1);
            return Err(EEXIST);
        }
        if self.n >= MAX_CONSUMERS {
            self.refused = self.refused.saturating_add(1);
            return Err(ENOMEM);
        }
        let mut i = 0usize;
        while i < MAX_CONSUMERS {
            let free = match self.slots.get(i) { Some(s) => s.is_none(), None => false };
            if free {
                let no = self.next_no;
                if let Some(slot) = self.slots.get_mut(i) {
                    *slot = Some(Consumer { no, name, kind, needs_passthrough, needs_exit127 });
                }
                self.n = self.n.saturating_add(1);
                self.next_no = self.next_no.saturating_add(1);
                return Ok(no);
            }
            i += 1;
        }
        self.refused = self.refused.saturating_add(1);
        Err(ENOMEM)
    }

    /// 注销。未知名字 ESRCH；号留洞不回收。
    pub fn unregister(&mut self, name: &'static str) -> Result<u16, i32> {
        let mut i = 0usize;
        while i < MAX_CONSUMERS {
            let hit = match self.slots.get(i) {
                Some(Some(c)) => c.name == name,
                _ => false,
            };
            if hit {
                let no = match self.slots.get(i) {
                    Some(Some(c)) => c.no,
                    _ => 0,
                };
                if let Some(slot) = self.slots.get_mut(i) {
                    *slot = None;
                }
                self.n = self.n.saturating_sub(1);
                self.retired = self.retired.saturating_add(1);
                return Ok(no);
            }
            i += 1;
        }
        self.refused = self.refused.saturating_add(1);
        Err(ESRCH)
    }

    /// 号只增不回收：在册号必须唯一且落在 1..next_no（洞允许被更大号
    /// 回填，槽序与号序无关，故不按槽序比较）。
    pub fn numbers_monotonic(&self) -> bool {
        let mut i = 0usize;
        while i < MAX_CONSUMERS {
            let cur = match self.slots.get(i) {
                Some(Some(c)) => c.no,
                _ => {
                    i += 1;
                    continue;
                }
            };
            if cur == 0 || cur >= self.next_no {
                return false;
            }
            let mut j = i + 1;
            while j < MAX_CONSUMERS {
                if let Some(Some(c)) = self.slots.get(j) {
                    if c.no == cur {
                        return false;
                    }
                }
                j += 1;
            }
            i += 1;
        }
        true
    }
}

/// 建一张参考册：shell / spawn / popen / supervisor 四家。
pub fn reference_registry() -> Registry {
    let mut r = Registry::new();
    let _ = r.register("sh", ConsumerKind::Shell, true, true);
    let _ = r.register("posix_spawn", ConsumerKind::Spawn, true, false);
    let _ = r.register("popen", ConsumerKind::Popen, true, true);
    let _ = r.register("supervisor", ConsumerKind::Supervisor, true, true);
    r
}

// ── fork/exec 运行状态机 ──────────────────────────────────────────────
/// 运行阶段。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RunPhase {
    Idle,
    Forked,
    ExecAttempted,
    ChildExited,
    Reaped,
}

impl RunPhase {
    pub fn code(self) -> u8 {
        match self {
            RunPhase::Idle => 0, RunPhase::Forked => 1, RunPhase::ExecAttempted => 2,
            RunPhase::ChildExited => 3, RunPhase::Reaped => 4,
        }
    }
}

/// 一次 fork/exec 会话（父侧或子侧各一份）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ForkExecRun {
    role: Role,
    phase: RunPhase,
    errno: i32,
    exit_code: u8,
    reaps: u32,
    refusals: u32,
}

impl ForkExecRun {
    pub const fn new(role: Role) -> ForkExecRun {
        ForkExecRun {
            role, phase: RunPhase::Idle, errno: 0, exit_code: 0, reaps: 0, refusals: 0,
        }
    }

    pub fn role(&self) -> Role { self.role }
    pub fn phase(&self) -> RunPhase { self.phase }
    pub fn errno(&self) -> i32 { self.errno }
    pub fn exit_code(&self) -> u8 { self.exit_code }
    pub fn reaps(&self) -> u32 { self.reaps }
    pub fn refusals(&self) -> u32 { self.refusals }

    fn refuse(&mut self, e: i32) -> Result<(), i32> {
        self.refusals = self.refusals.saturating_add(1);
        Err(e)
    }

    /// 记 fork 成功。父侧与子侧都从 Idle 起步。
    pub fn fork_ok(&mut self) -> Result<(), i32> {
        if self.phase != RunPhase::Idle { return self.refuse(EINVAL); }
        self.phase = RunPhase::Forked;
        Ok(())
    }

    /// 子侧尝试 exec。父侧无 exec 可试（EPERM）；重复尝试 EINVAL。
    pub fn exec_attempt(&mut self) -> Result<(), i32> {
        if self.role != Role::Child { return self.refuse(EPERM); }
        if self.phase != RunPhase::Forked { return self.refuse(EINVAL); }
        self.phase = RunPhase::ExecAttempted;
        Ok(())
    }

    /// exec 失败：子侧落 127 并「退出」，绝不返回父代码。
    pub fn exec_failed(&mut self, errno: i32) -> Result<u8, i32> {
        if self.role != Role::Child { return self.refuse(EPERM).map(|_| 0); }
        if self.phase != RunPhase::ExecAttempted { return self.refuse(EINVAL).map(|_| 0); }
        if errno <= 0 { return self.refuse(EINVAL).map(|_| 0); }
        self.errno = errno;
        self.exit_code = EXIT_NOT_EXEC;
        self.phase = RunPhase::ChildExited;
        Ok(self.exit_code)
    }

    /// 父侧回收子进程。恰好一次；二次 ECHILD；子侧回收 ECHILD。
    pub fn reap(&mut self) -> Result<u32, i32> {
        if self.role != Role::Parent { return self.refuse(ECHILD).map(|_| 0); }
        if self.phase == RunPhase::Reaped { return self.refuse(ECHILD).map(|_| 0); }
        if self.phase != RunPhase::Forked { return self.refuse(ECHILD).map(|_| 0); }
        self.phase = RunPhase::Reaped;
        self.reaps = self.reaps.saturating_add(1);
        Ok(wait_status_of(EXIT_NOT_EXEC))
    }
}

// ── 确定性指纹 ────────────────────────────────────────────────────────
pub const FNV_OFFSET: u32 = 0x811C_9DC5;
pub const FNV_PRIME: u32 = 0x0100_0193;

pub fn fnv1a_step(h: u32, byte: u8) -> u32 {
    (h ^ byte as u32).wrapping_mul(FNV_PRIME)
}

/// 消费核指纹：故障表两阶段位图 + 三层链 + 登记册号位。
pub fn core_fingerprint(bits: &StageBits, reg: &Registry) -> u32 {
    let mut h = FNV_OFFSET;
    h = fnv1a_step(h, bits.fork);
    h = fnv1a_step(h, bits.exec);
    let mut i = 0usize;
    while i < FAULTS.len() {
        if let Some(row) = FAULTS.get(i) {
            h = fnv1a_step(h, (row.errno & 0xFF) as u8);
            h = fnv1a_step(h, row.stage.code());
            h = fnv1a_step(h, if row.retryable { 1 } else { 0 });
        }
        i += 1;
    }
    let mut k = 0usize;
    while k < MAX_CONSUMERS {
        if let Some(c) = reg.slot(k) {
            h = fnv1a_step(h, (c.no & 0xFF) as u8);
            h = fnv1a_step(h, c.kind.tag());
            h = fnv1a_step(h, c.deps());
        }
        k += 1;
    }
    h = fnv1a_step(h, reg.retired() as u8);
    h
}

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（序号 1..=15，段内码 = SEG_BASE + 序号）。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ConsumerCode {
    ErrnoRemapped = 1,
    ReturnDenegated = 2,
    ForkPidWrong = 3,
    ErrnoPolluted = 4,
    ExecReturned = 5,
    ExitCodeWrong = 6,
    ExitBySignal = 7,
    UnreapedChild = 8,
    DoubleReap = 9,
    NumberReused = 10,
    RegisterConflict = 11,
    RegistryFull = 12,
    RetryMismatch = 13,
    SegmentViolation = 14,
    DigestDrift = 15,
}

impl ConsumerCode {
    pub fn code(self) -> u16 { self as u16 }

    pub fn seg_code(self) -> u32 { SEG_BASE + self as u32 }

    pub fn detail(self) -> &'static str {
        match self {
            ConsumerCode::ErrnoRemapped => "errno 被重映射：透传链某层改了错误值；请逐层对齐 raw→libc→app",
            ConsumerCode::ReturnDenegated => "返回槽漂移：-1 被换成 -errno 或反之；请统一用户面 -1 口径",
            ConsumerCode::ForkPidWrong => "fork pid 错：子必须为 0、父必须大于 0；请检查返回值判定方向",
            ConsumerCode::ErrnoPolluted => "成功的 fork 污染了 errno：调用前 errno 不得被改动",
            ConsumerCode::ExecReturned => "exec 失败后返回了子进程代码：必须 _exit(127) 立即终止",
            ConsumerCode::ExitCodeWrong => "退出码不是 127：shell 会把失败原因误报成普通失败",
            ConsumerCode::ExitBySignal => "子进程被信号终止：wait 低 7 位非零，退出码不可读",
            ConsumerCode::UnreapedChild => "子进程未回收：父侧必须 wait 一次，否则留僵尸",
            ConsumerCode::DoubleReap => "重复回收：第二次 wait 必须 ECHILD，不得复用旧状态",
            ConsumerCode::NumberReused => "消费号被回收：号只增不减，注销只留洞；请修分配器",
            ConsumerCode::RegisterConflict => "登记冲突：同名消费者已存在；请先注销或改名",
            ConsumerCode::RegistryFull => "消费号册已满：8 槽耗尽；请注销不再依赖该语义的消费者",
            ConsumerCode::RetryMismatch => "重试策略与 errno 不符：语义性失败不得重试",
            ConsumerCode::SegmentViolation => "段位越界：本模块段基址必须落在 0x7B840 且 0x40 对齐",
            ConsumerCode::DigestDrift => "指纹漂移：两次消费核指纹不等；请排查隐藏状态",
        }
    }

    pub const ALL: &'static [ConsumerCode] = &[
        ConsumerCode::ErrnoRemapped, ConsumerCode::ReturnDenegated, ConsumerCode::ForkPidWrong,
        ConsumerCode::ErrnoPolluted, ConsumerCode::ExecReturned, ConsumerCode::ExitCodeWrong,
        ConsumerCode::ExitBySignal, ConsumerCode::UnreapedChild, ConsumerCode::DoubleReap,
        ConsumerCode::NumberReused, ConsumerCode::RegisterConflict, ConsumerCode::RegistryFull,
        ConsumerCode::RetryMismatch, ConsumerCode::SegmentViolation, ConsumerCode::DigestDrift,
    ];
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9299 判据集（静态 17 条 + 动态 3 条）。
pub fn run_unx9299_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9299");

    let seg_ok = SEG_BASE % SEG_SPAN == 0 && SEG_BASE == 0x7B840
        && SEG_BASE + SEG_SPAN <= 0x7B900;
    s.add("段位守卫：0x7B840 独占 0x40 窗口", seg_ok,
          "段基址须 0x40 对齐且落在本批窗口，不得与相邻卡窗口重叠");

    // 七个 errno 的三层透传逐行复算。
    let mut pass_all = true;
    let mut i = 0usize;
    while i < FAULTS.len() {
        if let Some(row) = FAULTS.get(i) {
            let chain = fork_failure(row.errno);
            if !passthrough_ok(&chain) || chain.remapped() || chain.denegated() {
                pass_all = false;
            }
            if chain.app.errno != row.errno || chain.app.ret != -1 {
                pass_all = false;
            }
        }
        i += 1;
    }
    s.add("fork 失败：-1 与 errno 三层逐层透传",
          pass_all && CHAIN_LAYERS == 3,
          "raw→libc→app 三层都必须保持 -1 与原 errno，任何一层改写即重映射");

    let bad_chain = Chain {
        raw: Observed::failed(ENOMEM),
        libc: Observed::failed(EAGAIN),
        app: Observed::failed(EAGAIN),
    };
    let neg_chain = Chain {
        raw: Observed::failed(ENOMEM),
        libc: Observed { ret: -ENOMEM as i64, errno: ENOMEM },
        app: Observed::failed(ENOMEM),
    };
    s.add("重映射检测：errno 改写与 -errno 漂移都能被抓出",
          bad_chain.remapped() && !passthrough_ok(&bad_chain)
              && neg_chain.denegated() && !passthrough_ok(&neg_chain),
          "判据必须能对错样本报警，否则透传判据恒真、等于没写");

    let child_ok = Observed::succeeded(0);
    let parent_ok = Observed::succeeded(4242);
    let parent_bad = Observed { ret: 0, errno: ENOMEM };
    s.add("fork 成功：子 pid 为 0、父 pid 为正、errno 未被污染",
          fork_success_ok(&child_ok, &parent_ok) && !fork_success_ok(&child_ok, &parent_bad)
              && child_ok.ret == 0 && parent_ok.ret > 0,
          "成功路径的 errno 必须保持原值；把「成功也写 errno」当成规范是常见误读");

    // exec 失败的 7 行逐行核对一次性给结论。
    let mut exec_rows_ok = true;
    i = 0usize;
    while i < FAULTS.len() {
        if let Some(row) = FAULTS.get(i) {
            let o = exec_failure(row.errno);
            let st = wait_status_of(o.exit_code);
            if o.exit_code != EXIT_NOT_EXEC || o.returned || !o.reaped
                || !wait_exited(st) || wait_signaled(st) || wait_exit_code(st) != 127
                || st != 0x7F00 {
                exec_rows_ok = false;
            }
        }
        i += 1;
    }
    s.add("exec 失败：子进程 _exit(127)，wait 状态 0x7F00",
          exec_rows_ok && wait_status_of(EXIT_NOT_EXEC) == 0x7F00,
          "127 落在次低字节，低 7 位必须为 0；0x7F00 是唯一合法状态");

    s.add("exec 失败：不是 exit(1)、也不是被信号终止",
          wait_exit_code(wait_status_of(1)) == 1
              && wait_signaled(wait_status_of(1)) == false
              && wait_exit_code(wait_status_of(127)) != 1
              && wait_signaled(0x0009) && wait_exited(0x0009) == false,
          "shell 靠 127 区分「找不到命令」；退 1 会被误报成命令自身失败");

    // 父侧回收：恰好一次。
    let mut parent = ForkExecRun::new(Role::Parent);
    let fork_p = parent.fork_ok();
    let first_reap = parent.reap();
    let second_reap = parent.reap();
    s.add("父侧回收：恰好一次，二次 ECHILD",
          fork_p.is_ok() && first_reap == Ok(0x7F00) && second_reap == Err(ECHILD)
              && parent.reaps() == 1 && parent.phase() == RunPhase::Reaped,
          "重复回收若成功，会把同一个子进程的终局重复计账");

    // 子侧不得回收、父侧不得 exec。
    let mut child = ForkExecRun::new(Role::Child);
    let _ = child.fork_ok();
    let child_reap = child.reap();
    let mut parent2 = ForkExecRun::new(Role::Parent);
    let _ = parent2.fork_ok();
    let parent_exec = parent2.exec_attempt();
    s.add("角色守卫：子侧不得回收、父侧不得 exec",
          child_reap == Err(ECHILD) && parent_exec == Err(EPERM)
              && child.role() == Role::Child && parent2.role() == Role::Parent,
          "角色错位的调用必须当场拒绝，否则会把父侧语义带到子侧");

    // 子侧 exec 失败后终局。
    let mut c2 = ForkExecRun::new(Role::Child);
    let _ = c2.fork_ok();
    let attempt = c2.exec_attempt();
    let failed = c2.exec_failed(ENOENT);
    let bad_errno = c2.exec_failed(0);
    s.add("子侧 exec 失败：落 127 且不再接受零 errno",
          attempt.is_ok() && failed == Ok(127) && c2.exit_code() == 127
              && c2.errno() == ENOENT && c2.phase() == RunPhase::ChildExited
              && bad_errno == Err(EINVAL),
          "失败终局是一次性的：退出码落定后不得再改，errno 必须为正");

    // 消费号登记。
    let reg = reference_registry();
    let mut nums_ok = true;
    let mut expect = 1u16;
    let mut k = 0usize;
    while k < MAX_CONSUMERS {
        if let Some(c) = reg.slot(k) {
            if c.no != expect { nums_ok = false; }
            expect = expect.saturating_add(1);
        }
        k += 1;
    }
    s.add("消费号：从 1 起连续分配、四家全部在册",
          reg.active() == 4 && nums_ok && reg.numbers_monotonic() && reg.next_no() == 5
              && reg.find("sh").map(|c| c.no) == Some(1)
              && reg.by_no(3).map(|c| c.name) == Some("popen"),
          "号是审计主键：必须连续、唯一、可按号反查，否则日志无法归并");

    let mut dup = reference_registry();
    let dup_reg = dup.register("sh", ConsumerKind::Shell, true, true);
    let unknown_de = dup.unregister("nobody");
    s.add("登记拒绝面：重名 EEXIST、注销未知 ESRCH",
          dup_reg == Err(EEXIST) && unknown_de == Err(ESRCH)
              && dup.active() == 4 && dup.refused() == 2 && dup.retired() == 0,
          "拒绝不得改动册状态：重名不占号、未知注销不增退役计数");

    let mut reuse = reference_registry();
    let retired_no = reuse.unregister("popen");
    let new_no = reuse.register("popen2", ConsumerKind::Popen, true, true);
    s.add("注销不回收号：新消费者拿 next_no 而非旧号",
          retired_no == Ok(3) && new_no == Ok(5) && reuse.find("popen2").map(|c| c.no) == Some(5)
              && reuse.retired() == 1 && reuse.active() == 4
              && reuse.numbers_monotonic(),
          "号回收会让历史日志指向新消费者，是审计面最忌讳的复用");

    let mut full = Registry::new();
    let names = ["a", "b", "c", "d", "e", "f", "g", "h"];
    let mut ok_count = 0u32;
    let mut j = 0usize;
    while j < names.len() {
        if let Some(name) = names.get(j) {
            if full.register(name, ConsumerKind::Ktest, false, false).is_ok() {
                ok_count = ok_count.saturating_add(1);
            }
        }
        j += 1;
    }
    let ninth = full.register("i", ConsumerKind::Ktest, false, false);
    s.add("消费号册容量：8 槽满后第 9 家 ENOMEM",
          ok_count == MAX_CONSUMERS as u32 && full.active() == MAX_CONSUMERS
              && ninth == Err(ENOMEM) && full.next_no() == 9,
          "定容册不得越界写入；满册必须显性 ENOMEM，失败也不消耗新号");

    // 重试策略逐行校验。
    let mut retry_ok = true;
    i = 0usize;
    while i < FAULTS.len() {
        if let Some(row) = FAULTS.get(i) {
            if retryable(row.errno) != row.retryable { retry_ok = false; }
            let hinted = retry_hint(row.errno).starts_with(if row.retryable { "可重试" } else { "不可重试" });
            if !hinted { retry_ok = false; }
        }
        i += 1;
    }
    s.add("重试面：EAGAIN/ENOMEM/EINTR 可重试，其余不可",
          retry_ok && retryable(EAGAIN) && retryable(ENOMEM) && retryable(EINTR)
              && !retryable(ENOEXEC) && !retryable(ENOENT) && !retryable(EACCES)
              && !retryable(E2BIG),
          "语义性失败重试只会重复失败；资源暂缺类才允许重发");

    let bits = run_stage_matrix();
    s.add("两阶段矩阵：7 行 × {fork, exec} 位图全 1",
          bits.full() && bits.fork == 0x7F && bits.exec == 0x7F
              && bits.hit(Stage::Fork, 0) && bits.hit(Stage::Exec, 6)
              && !bits.hit(Stage::Fork, FAULT_ROWS),
          "两张 7 位图必须同时满；任一行为 0 说明该 errno 的错误路径有缺口");

    s.add("errno 数值：与 Linux 头文件逐一相等",
          EAGAIN == 11 && ENOMEM == 12 && EINTR == 4 && ENOEXEC == 8
              && ENOENT == 2 && EACCES == 13 && E2BIG == 7 && ECHILD == 10
              && ESRCH == 3 && EEXIST == 17 && EPERM == 1 && EINVAL == 22,
          "消费核的所有拒绝都按内核码值对外，不引入自定义码");

    let fp_a = core_fingerprint(&run_stage_matrix(), &reference_registry());
    let fp_b = core_fingerprint(&run_stage_matrix(), &reference_registry());
    s.add("双跑一致：消费核指纹相等", fp_a == fp_b && fp_a != 0,
          "同输入两次必须逐字节相等；指纹把位图、故障表与登记册一起钉住");

    let mut uniq = true;
    i = 0usize;
    while i < ConsumerCode::ALL.len() {
        let mut j2 = i + 1;
        while j2 < ConsumerCode::ALL.len() {
            if let (Some(a), Some(b)) = (ConsumerCode::ALL.get(i), ConsumerCode::ALL.get(j2)) {
                if a.seg_code() == b.seg_code() || a.code() == b.code() { uniq = false; }
            }
            j2 += 1;
        }
        i += 1;
    }
    let codes_ok = uniq && ConsumerCode::ALL.len() == 15
        && ConsumerCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("诊断码闭集：15 码段内唯一且文案非空", codes_ok,
          "码值唯一是红项定位的前提；文案为空等于把诊断丢回给读码人");

    // ── 动态明细 ─────────────────────────────────────────────────────
    let bits_text = format!(
        "阶段位图 fork=0b{:07b} exec=0b{:07b}，满 {}",
        bits.fork, bits.exec, bits.full()
    );
    s.add_dyn("矩阵快照", bits.full(), &bits_text);

    let chain_text = format!(
        "三层链 raw({},{}) libc({},{}) app({},{})，重映射 {}",
        chain_probe().raw.ret, chain_probe().raw.errno,
        chain_probe().libc.ret, chain_probe().libc.errno,
        chain_probe().app.ret, chain_probe().app.errno,
        chain_probe().remapped()
    );
    s.add_dyn("透传快照", passthrough_ok(&chain_probe()), &chain_text);

    let reg_text = format!(
        "消费号册 在册 {} 退役 {} 次号 {} 拒绝 {}",
        reg.active(), reg.retired(), reg.next_no(), reg.refused()
    );
    s.add_dyn("登记快照", reg.numbers_monotonic(), &reg_text);

    s
}

/// 参考链（ENOMEM 失败，三层一致）。
fn chain_probe() -> Chain {
    fork_failure(ENOMEM)
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errno_values_match_linux() {
        assert_eq!((EAGAIN, ENOMEM, EINTR, ENOEXEC), (11, 12, 4, 8));
        assert_eq!((EACCES, ECHILD, ESRCH, EEXIST), (13, 10, 3, 17));
    }

    #[test]
    fn passthrough_is_three_layers() {
        let c = fork_failure(ENOMEM);
        assert_eq!((c.uniform(), c.remapped()), (true, false));
        assert_eq!(passthrough_ok(&c), true);
        let bad = Chain { raw: Observed::failed(ENOMEM), libc: Observed::failed(EAGAIN), app: Observed::failed(EAGAIN) };
        assert_eq!(bad.remapped(), true);
        assert_eq!(passthrough_ok(&bad), false);
    }

    #[test]
    fn fork_success_shape() {
        assert_eq!(fork_success_ok(&Observed::succeeded(0), &Observed::succeeded(9)), true);
        assert_eq!(fork_success_ok(&Observed::succeeded(0), &Observed::succeeded(0)), false);
        assert_eq!(fork_success_ok(&Observed::succeeded(0), &Observed { ret: 9, errno: ENOMEM }), false);
        assert_eq!(Observed::failed(ENOMEM).is_failure(), true);
    }

    #[test]
    fn exec_exit_is_127() {
        let o = exec_failure(ENOENT);
        let st = wait_status_of(o.exit_code);
        assert_eq!(st, 0x7F00);
        assert_eq!(wait_exited(st), true);
        assert_eq!(wait_signaled(st), false);
        assert_eq!(wait_exit_code(st), 127);
        assert_eq!(wait_exit_code(wait_status_of(1)), 1);
        assert_eq!(wait_exited(0x0009), false);
        assert_eq!(wait_signaled(0x0009), true);
    }

    #[test]
    fn registry_lifecycle() {
        let mut r = reference_registry();
        assert_eq!((r.active(), r.next_no()), (4, 5));
        assert_eq!(r.register("sh", ConsumerKind::Shell, true, true), Err(EEXIST));
        assert_eq!(r.unregister("nobody"), Err(ESRCH));
        assert_eq!(r.unregister("popen"), Ok(3));
        assert_eq!(r.register("popen2", ConsumerKind::Popen, true, true), Ok(5));
        assert_eq!(r.numbers_monotonic(), true);
        assert_eq!(r.find("popen"), None);
        assert_eq!(r.by_no(5).map(|c| c.name), Some("popen2"));
        let mut full = Registry::new();
        let names = ["a", "b", "c", "d", "e", "f", "g", "h"];
        for n in names.iter() {
            assert_eq!(full.register(n, ConsumerKind::Ktest, false, false).is_ok(), true);
        }
        assert_eq!(full.register("i", ConsumerKind::Ktest, false, false), Err(ENOMEM));
    }

    #[test]
    fn run_state_machine_roles() {
        let mut parent = ForkExecRun::new(Role::Parent);
        assert_eq!(parent.exec_attempt(), Err(EPERM));
        assert_eq!(parent.reap(), Err(ECHILD));
        assert_eq!(parent.fork_ok(), Ok(()));
        assert_eq!(parent.reap(), Ok(0x7F00));
        assert_eq!(parent.reap(), Err(ECHILD));
        let mut child = ForkExecRun::new(Role::Child);
        assert_eq!(child.fork_ok(), Ok(()));
        assert_eq!(child.exec_attempt(), Ok(()));
        assert_eq!(child.exec_failed(ENOENT), Ok(127));
        assert_eq!(child.exec_failed(0), Err(EINVAL));
        assert_eq!(child.reap(), Err(ECHILD));
    }

    #[test]
    fn stage_matrix_and_retry_table() {
        let bits = run_stage_matrix();
        assert_eq!((bits.fork, bits.exec), (0x7F, 0x7F));
        assert_eq!(bits.full(), true);
        assert_eq!(bits.hit(Stage::Exec, 6), true);
        assert_eq!(bits.hit(Stage::Fork, 7), false);
        for row in FAULTS.iter() {
            assert_eq!(retryable(row.errno), row.retryable);
        }
        assert_eq!(retryable(EINTR), true);
        assert_eq!(retryable(ENOEXEC), false);
    }

    #[test]
    fn fingerprint_is_reproducible() {
        let a = core_fingerprint(&run_stage_matrix(), &reference_registry());
        let b = core_fingerprint(&run_stage_matrix(), &reference_registry());
        assert_eq!(a, b);
        let mut r = reference_registry();
        let _ = r.unregister("sh");
        assert_eq!(core_fingerprint(&run_stage_matrix(), &r) != a, true);
    }

    #[test]
    fn codes_and_checks_green() {
        assert_eq!(ConsumerCode::ALL.len(), 15);
        assert_eq!(ConsumerCode::ErrnoRemapped.code(), 1);
        assert_eq!(ConsumerCode::DigestDrift.seg_code(), SEG_BASE + 15);
        for c in ConsumerCode::ALL.iter() {
            assert_eq!(c.detail().is_empty(), false);
        }
        let s = run_unx9299_checks();
        assert_eq!(s.all_passed(), true, "{}", s.red_items_text());
    }
}
