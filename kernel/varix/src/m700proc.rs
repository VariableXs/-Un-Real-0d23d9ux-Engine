//! m700proc — VARIX-M700 AI-01 进程与线程域 (F001~F025)
//!
//! 进程状态机全谱/fork 语义精铸/exec 加载管线/线程轻量级谱/等待队列律/
//! 孤儿收养所/进程凭证夹/会话与进程组/退出清算官/线程局部存储/
//! 进程资源台账/fork 火焰剖面/线程命名协议/优先级带谱/内核栈哨兵/
//! 僵尸超时收容/线程取消协议/进程冻结舱/clone 旗标谱系/进程凭据审计/
//! 线程亲和微调/进程自描述块/栈切换快照/fork 池预热/进程域年报。
//!
//! 硬约束：no_std / 无 alloc / 固定容量数组 / 纯逻辑状态机。

use crate::checks::CheckSet;

// ===========================================================================
// F001 — 进程状态机全谱：完整状态转移表
// ===========================================================================

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PState {
    New,
    Ready,
    Running,
    Blocked,
    Zombie,
    Dead,
}

/// 合法转移表；非法转移一律拒绝（返回原状态）。
pub fn proc_transition(cur: PState, event: PEvent) -> PState {
    use PEvent::*;
    use PState::*;
    match (cur, event) {
        (New, Admit) => Ready,
        (Ready, Dispatch) => Running,
        (Running, Yield) => Ready,
        (Running, Block) => Blocked,
        (Blocked, Wake) => Ready,
        (Running, Exit) => Zombie,
        (Zombie, Reap) => Dead,
        _ => cur,
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PEvent {
    Admit,
    Dispatch,
    Yield,
    Block,
    Wake,
    Exit,
    Reap,
}

// ===========================================================================
// F002 — fork 语义精铸：写时复制页的引用计数契约
// ===========================================================================

#[derive(Clone, Copy)]
pub struct CowPage {
    pub refs: u16,
    pub writable: bool,
}

/// fork 时共享页：引用 +1、父子皆不可写。
pub fn fork_page(p: &mut CowPage) {
    p.refs += 1;
    p.writable = false;
}

/// 缺页写时：引用减一，减到 1 时恢复可写（独占）。
pub fn cow_break(p: &mut CowPage) {
    if p.refs > 1 {
        p.refs -= 1;
        p.writable = false;
    } else {
        p.writable = true;
    }
}

// ===========================================================================
// F003 — exec 加载管线：ELF 头校验五连
// ===========================================================================

pub fn exec_header_ok(hdr: &[u8; 8]) -> bool {
    hdr[0] == 0x7F
        && hdr[1] == b'E'
        && hdr[2] == b'L'
        && hdr[3] == b'F'
        && hdr[4] == 2 // ELFCLASS64
}

// ===========================================================================
// F004 — 线程轻量级谱：线程与进程共享地址空间的判定
// ===========================================================================

#[derive(Clone, Copy)]
pub struct ThreadDesc {
    pub tid: u16,
    pub pid: u16,
    pub owns_mm: bool,
}

/// 线程轻量 = 同进程且共享地址空间。
pub fn threads_share_mm(a: &ThreadDesc, b: &ThreadDesc) -> bool {
    a.pid == b.pid && !a.owns_mm && !b.owns_mm
}

// ===========================================================================
// F005 — 等待队列律：FIFO 唤醒、去重注册
// ===========================================================================

pub const WQ_MAX: usize = 8;

#[derive(Clone, Copy)]
pub struct WaitQueue {
    pub queue: [u16; WQ_MAX],
    pub len: usize,
    pub dup_rejected: usize,
}

impl WaitQueue {
    pub const fn new() -> WaitQueue {
        WaitQueue { queue: [0; WQ_MAX], len: 0, dup_rejected: 0 }
    }
    pub fn wait(&mut self, tid: u16) -> bool {
        for i in 0..self.len {
            if self.queue[i] == tid {
                self.dup_rejected += 1;
                return false;
            }
        }
        if self.len < WQ_MAX {
            self.queue[self.len] = tid;
            self.len += 1;
            true
        } else {
            false
        }
    }
    pub fn wake_one(&mut self) -> Option<u16> {
        if self.len == 0 {
            return None;
        }
        let tid = self.queue[0];
        for i in 1..self.len {
            self.queue[i - 1] = self.queue[i];
        }
        self.len -= 1;
        Some(tid)
    }
}

// ===========================================================================
// F006 — 孤儿收养所：父死后子进程移交 init(pid=1)
// ===========================================================================

pub const INIT_PID: u16 = 1;

pub fn orphan_adopt(child_parent: u16, parent_alive: bool) -> u16 {
    if parent_alive {
        child_parent
    } else {
        INIT_PID
    }
}

// ===========================================================================
// F007 — 进程凭证夹：uid/gid/caps 三元组
// ===========================================================================

#[derive(Clone, Copy)]
pub struct Cred {
    pub uid: u16,
    pub gid: u16,
    pub caps: u32,
}

pub const CAP_KILL: u32 = 1 << 5;

pub fn cred_can_kill(c: &Cred, target_uid: u16) -> bool {
    c.uid == 0 || c.uid == target_uid || c.caps & CAP_KILL != 0
}

// ===========================================================================
// F008 — 会话与进程组：setsid 的约束（组长不能自立新会话）
// ===========================================================================

#[derive(Clone, Copy)]
pub struct ProcGroup {
    pub pid: u16,
    pub pgid: u16,
    pub sid: u16,
}

pub fn setsid(p: &ProcGroup) -> Option<u16> {
    // 进程组长（pid == pgid）不允许再建会话。
    if p.pid == p.pgid {
        return None;
    }
    Some(p.pid)
}

// ===========================================================================
// F009 — 退出清算官：退出码语义 + 资源回收顺序
// ===========================================================================

/// 0..=255 合法退出码；文件/内存/句柄按固定顺序清算。
pub fn exit_code_valid(code: u32) -> bool {
    code <= 255
}

pub const CLEANUP_ORDER: [&str; 3] = ["close-handles", "release-memory", "flush-files"];

// ===========================================================================
// F010 — 线程局部存储：每线程独立槽位
// ===========================================================================

pub const TLS_SLOTS: usize = 4;

#[derive(Clone, Copy)]
pub struct TlsBlock {
    pub slots: [u64; TLS_SLOTS],
}

impl TlsBlock {
    pub const fn new() -> TlsBlock {
        TlsBlock { slots: [0; TLS_SLOTS] }
    }
    pub fn set(&mut self, idx: usize, v: u64) -> bool {
        if idx < TLS_SLOTS {
            self.slots[idx] = v;
            true
        } else {
            false
        }
    }
    pub fn get(&self, idx: usize) -> Option<u64> {
        self.slots.get(idx).copied()
    }
}

// ===========================================================================
// F011 — 进程资源台账：配额记账 + 去重登记
// ===========================================================================

pub const LEDGER_MAX: usize = 8;

#[derive(Clone, Copy)]
pub struct ResourceLedger {
    pub entries: [(u8, u32); LEDGER_MAX], // (资源类型, 占用量)
    pub len: usize,
    pub dup_rejected: usize,
}

impl ResourceLedger {
    pub const fn new() -> ResourceLedger {
        ResourceLedger { entries: [(0, 0); LEDGER_MAX], len: 0, dup_rejected: 0 }
    }
    pub fn record(&mut self, kind: u8, amount: u32) -> bool {
        for i in 0..self.len {
            if self.entries[i].0 == kind {
                self.dup_rejected += 1;
                return false;
            }
        }
        if self.len < LEDGER_MAX {
            self.entries[self.len] = (kind, amount);
            self.len += 1;
            true
        } else {
            false
        }
    }
    pub fn usage_of(&self, kind: u8) -> u32 {
        for i in 0..self.len {
            if self.entries[i].0 == kind {
                return self.entries[i].1;
            }
        }
        0
    }
    pub fn over_quota(&self, kind: u8, quota: u32) -> bool {
        self.usage_of(kind) > quota
    }
}

// ===========================================================================
// F012 — fork 火焰剖面：fork 各阶段耗时占比（permille）
// ===========================================================================

pub const FORK_STAGES: usize = 3; // 0=复制描述 1=建页表 2=挂队列

pub fn fork_stage_permille(stage_ms: &[u32; FORK_STAGES], idx: usize) -> u16 {
    let total = stage_ms[0] as u64 + stage_ms[1] as u64 + stage_ms[2] as u64;
    if total == 0 {
        return 0;
    }
    ((stage_ms[idx] as u64 * 1000) / total) as u16
}

// ===========================================================================
// F013 — 线程命名协议：名字规范化（截断 16 字节、空白转 '_'）
// ===========================================================================

pub const TNAME_MAX: usize = 16;

pub fn thread_name_norm(src: &[u8], out: &mut [u8; TNAME_MAX]) -> usize {
    let mut n = 0usize;
    for &b in src {
        if n >= TNAME_MAX {
            break;
        }
        out[n] = if b == b' ' || b == b'\t' { b'_' } else { b };
        n += 1;
    }
    n
}

// ===========================================================================
// F014 — 优先级带谱：0~255 分成四带，带内不允许越带提升
// ===========================================================================

pub fn prio_band(prio: u8) -> u8 {
    prio / 64
}

pub fn prio_clamp_within_band(request: u8, origin: u8) -> u8 {
    let band = prio_band(origin);
    let lo = band * 64;
    let hi = lo + 63;
    if request < lo {
        lo
    } else if request > hi {
        hi
    } else {
        request
    }
}

// ===========================================================================
// F015 — 内核栈哨兵：栈底金丝雀是否被踩
// ===========================================================================

pub const STACK_CANARY: u64 = 0xCAFE_BABE_DEAD_F00D;

pub fn stack_canary_intact(canary: u64) -> bool {
    canary == STACK_CANARY
}

// ===========================================================================
// F016 — 僵尸超时收容：Zombie 超过 N 拍强制转 Dead
// ===========================================================================

pub fn zombie_reap_on_timeout(age_ticks: u32, limit: u32) -> Option<PState> {
    if age_ticks > limit {
        Some(PState::Dead)
    } else {
        None
    }
}

// ===========================================================================
// F017 — 线程取消协议：取消点 + 清理句柄先于退出
// ===========================================================================

pub const CANCEL_PENDING: u8 = 1;
pub const CANCEL_CLEANUP: u8 = 2;
pub const CANCEL_DONE: u8 = 3;

pub fn cancel_advance(cur: u8, at_cancellation_point: bool) -> u8 {
    match cur {
        0 => {
            if at_cancellation_point {
                CANCEL_PENDING
            } else {
                0
            }
        }
        CANCEL_PENDING => CANCEL_CLEANUP,
        CANCEL_CLEANUP => CANCEL_DONE,
        _ => cur,
    }
}

// ===========================================================================
// F018 — 进程冻结舱：冻结期间信号挂起、解冻按序回放
// ===========================================================================

pub const FREEZE_SIG_MAX: usize = 4;

#[derive(Clone, Copy)]
pub struct FreezeCapsule {
    pub frozen: bool,
    pub pending: [u8; FREEZE_SIG_MAX],
    pub len: usize,
    pub dropped: usize,
}

impl FreezeCapsule {
    pub const fn new() -> FreezeCapsule {
        FreezeCapsule { frozen: false, pending: [0; FREEZE_SIG_MAX], len: 0, dropped: 0 }
    }
    pub fn signal(&mut self, sig: u8) {
        if self.frozen {
            if self.len < FREEZE_SIG_MAX {
                self.pending[self.len] = sig;
                self.len += 1;
            } else {
                self.dropped += 1;
            }
        }
    }
    pub fn thaw(&mut self) -> usize {
        self.frozen = false;
        let n = self.len;
        self.len = 0;
        n
    }
}

// ===========================================================================
// F019 — clone 旗标谱系：旗标组合合法性
// ===========================================================================

pub const CL_NEWMM: u32 = 0b0001;
pub const CL_NEWSIG: u32 = 0b0010;
pub const CL_NEWTLS: u32 = 0b0100;

/// NEWMM(独立地址空间) 与共享语义不冲突，但 NEWMM 必须伴随 NEWSIG。
pub fn clone_flags_valid(flags: u32) -> bool {
    if flags & CL_NEWMM != 0 {
        flags & CL_NEWSIG != 0
    } else {
        true
    }
}

// ===========================================================================
// F020 — 进程凭据审计：提权事件必须留痕
// ===========================================================================

pub const AUDIT_MAX: usize = 8;

#[derive(Clone, Copy)]
pub struct PrivAudit {
    pub events: [(u16, bool); AUDIT_MAX], // (目标 uid, 是否提权)
    pub len: usize,
    pub dup_rejected: usize,
}

impl PrivAudit {
    pub const fn new() -> PrivAudit {
        PrivAudit { events: [(0, false); AUDIT_MAX], len: 0, dup_rejected: 0 }
    }
    pub fn record(&mut self, uid: u16, elevated: bool) -> bool {
        for i in 0..self.len {
            if self.events[i].0 == uid && self.events[i].1 == elevated {
                self.dup_rejected += 1;
                return false;
            }
        }
        if self.len < AUDIT_MAX {
            self.events[self.len] = (uid, elevated);
            self.len += 1;
            true
        } else {
            false
        }
    }
    pub fn elevation_count(&self) -> usize {
        let mut n = 0;
        for i in 0..self.len {
            if self.events[i].1 {
                n += 1;
            }
        }
        n
    }
}

// ===========================================================================
// F021 — 线程亲和微调：亲和位图必须与 CPU 数自洽
// ===========================================================================

pub fn affinity_valid(mask: u64, cpu_count: u8) -> bool {
    if cpu_count == 0 || cpu_count >= 64 {
        return false;
    }
    let valid_bits = (1u64 << cpu_count) - 1;
    mask & valid_bits == mask && mask != 0
}

// ===========================================================================
// F022 — 进程自描述块：名字+状态+父进程+票据版本
// ===========================================================================

pub const DESC_VERSION: u32 = 3;

#[derive(Clone, Copy)]
pub struct ProcDesc {
    pub name: [u8; TNAME_MAX],
    pub name_len: usize,
    pub state: PState,
    pub parent: u16,
    pub version: u32,
}

impl ProcDesc {
    pub fn describe(&self, out: &mut [u8]) -> usize {
        let mut n = 0;
        for &b in &self.name[..self.name_len.min(TNAME_MAX)] {
            if n < out.len() {
                out[n] = b;
                n += 1;
            }
        }
        let s: &[u8] = match self.state {
            PState::New => b":new",
            PState::Ready => b":ready",
            PState::Running => b":run",
            PState::Blocked => b":blk",
            PState::Zombie => b":zmb",
            PState::Dead => b":dead",
        };
        for &b in s {
            if n < out.len() {
                out[n] = b;
                n += 1;
            }
        }
        n
    }
}

// ===========================================================================
// F023 — 栈切换快照：保存/恢复栈指针与返回地址配对
// ===========================================================================

#[derive(Clone, Copy)]
pub struct StackSnapshot {
    pub sp: u64,
    pub ra: u64,
    pub valid: bool,
}

pub fn snapshot_save(sp: u64, ra: u64) -> StackSnapshot {
    // 对齐 16 字节且非空才有效。
    let valid = sp != 0 && ra != 0 && sp % 16 == 0;
    StackSnapshot { sp, ra, valid }
}

pub fn snapshot_restore(s: &StackSnapshot, cur_sp: u64) -> bool {
    s.valid && s.sp != cur_sp // 恢复到自己当前栈指针属于非法回卷
}

// ===========================================================================
// F024 — fork 池预热：预建进程描述符池，fork 时即取即用
// ===========================================================================

pub const POOL_MAX: usize = 8;

#[derive(Clone, Copy)]
pub struct ForkPool {
    pub free_ids: [u16; POOL_MAX],
    pub len: usize,
    pub prewarmed: usize,
    pub served: usize,
}

impl ForkPool {
    pub const fn new() -> ForkPool {
        ForkPool { free_ids: [0; POOL_MAX], len: 0, prewarmed: 0, served: 0 }
    }
    pub fn prewarm(&mut self, next_id: u16) -> usize {
        let mut added = 0;
        while self.len < POOL_MAX && added < 4 {
            self.free_ids[self.len] = next_id + added as u16;
            self.len += 1;
            self.prewarmed += 1;
            added += 1;
        }
        added
    }
    pub fn take(&mut self) -> Option<u16> {
        if self.len == 0 {
            return None;
        }
        let id = self.free_ids[0];
        for i in 1..self.len {
            self.free_ids[i - 1] = self.free_ids[i];
        }
        self.len -= 1;
        self.served += 1;
        Some(id)
    }
}

// ===========================================================================
// F025 — 进程域年报：域级仪表汇总
// ===========================================================================

#[derive(Clone, Copy)]
pub struct ProcYearbook {
    pub forks: u64,
    pub exits: u64,
    pub reaps: u64,
    pub zombie_timeouts: u64,
    pub frozen_now: u32,
}

impl ProcYearbook {
    pub const fn new() -> ProcYearbook {
        ProcYearbook { forks: 0, exits: 0, reaps: 0, zombie_timeouts: 0, frozen_now: 0 }
    }
    pub fn record_fork(&mut self) {
        self.forks += 1;
    }
    pub fn record_exit(&mut self) {
        self.exits += 1;
    }
    pub fn record_reap(&mut self) {
        self.reaps += 1;
    }
    /// 未回收的僵尸数：exit - reap（允许超时收容冲账）。
    pub fn unclean_exits(&self) -> u64 {
        self.exits - self.reaps.min(self.exits)
    }
}

// ===========================================================================
// 域自检（≥25 项）
// ===========================================================================

pub fn run_m700proc_checks() -> CheckSet {
    let mut set = CheckSet::new("m700proc");

    // F001
    let s = proc_transition(PState::New, PEvent::Admit);
    let s2 = proc_transition(PState::Running, PEvent::Exit);
    let s3 = proc_transition(PState::New, PEvent::Exit);
    set.add("F001 admit", s == PState::Ready, "admit wrong");
    set.add("F001 exit", s2 == PState::Zombie, "exit wrong");
    set.add("F001 illegal rejected", s3 == PState::New, "illegal wrong");

    // F002
    let mut pg = CowPage { refs: 1, writable: true };
    fork_page(&mut pg);
    let forked = pg.refs == 2 && !pg.writable;
    cow_break(&mut pg);
    let break1 = pg.refs == 1 && !pg.writable;
    cow_break(&mut pg);
    set.add("F002 fork shares", forked, "fork wrong");
    set.add("F002 cow break half", break1, "break1 wrong");
    set.add("F002 cow break exclusive", pg.writable, "break2 wrong");

    // F003
    let hdr = [0x7F, b'E', b'L', b'F', 2, 0, 0, 0];
    let bad = [0x7F, b'E', b'L', b'F', 1, 0, 0, 0];
    set.add("F003 elf header", exec_header_ok(&hdr) && !exec_header_ok(&bad), "hdr wrong");

    // F004
    let t1 = ThreadDesc { tid: 1, pid: 7, owns_mm: false };
    let t2 = ThreadDesc { tid: 2, pid: 7, owns_mm: false };
    let t3 = ThreadDesc { tid: 3, pid: 8, owns_mm: false };
    set.add("F004 share mm", threads_share_mm(&t1, &t2), "share wrong");
    set.add("F004 cross proc", !threads_share_mm(&t1, &t3), "cross wrong");

    // F005
    let mut wq = WaitQueue::new();
    let a = wq.wait(10);
    let dup = wq.wait(10);
    let b = wq.wait(11);
    let w1 = wq.wake_one();
    let w2 = wq.wake_one();
    set.add("F005 wait register", a && b, "register wrong");
    set.add("F005 wait dedup", !dup && wq.dup_rejected == 1, "dup wrong");
    set.add("F005 fifo wake", w1 == Some(10) && w2 == Some(11), "fifo wrong");

    // F006
    set.add("F006 adopt init", orphan_adopt(9, false) == INIT_PID, "adopt wrong");
    set.add("F006 parent keeps", orphan_adopt(9, true) == 9, "keep wrong");

    // F007
    let c0 = Cred { uid: 0, gid: 0, caps: 0 };
    let ck = Cred { uid: 5, gid: 5, caps: CAP_KILL };
    let cn = Cred { uid: 5, gid: 5, caps: 0 };
    set.add("F007 root kill", cred_can_kill(&c0, 9), "root wrong");
    set.add("F007 cap kill", cred_can_kill(&ck, 9), "cap wrong");
    set.add("F007 plain deny", !cred_can_kill(&cn, 9), "deny wrong");

    // F008
    let leader = ProcGroup { pid: 4, pgid: 4, sid: 1 };
    let member = ProcGroup { pid: 5, pgid: 4, sid: 1 };
    set.add("F008 leader denied", setsid(&leader).is_none(), "leader wrong");
    set.add("F008 member ok", setsid(&member) == Some(5), "member wrong");

    // F009
    set.add("F009 exit code range", exit_code_valid(255) && !exit_code_valid(256), "range wrong");

    // F010
    let mut tls = TlsBlock::new();
    tls.set(0, 42);
    tls.set(3, 7);
    let oob = tls.set(4, 1);
    set.add("F010 tls set", tls.get(0) == Some(42) && tls.get(3) == Some(7), "set wrong");
    set.add("F010 tls oob", !oob && tls.get(4).is_none(), "oob wrong");

    // F011
    let mut led = ResourceLedger::new();
    let r1 = led.record(1, 100);
    let dup = led.record(1, 200);
    led.record(2, 5);
    set.add("F011 ledger register", r1 && !dup, "register wrong");
    set.add("F011 ledger usage", led.usage_of(1) == 100 && led.usage_of(2) == 5, "usage wrong");
    set.add("F011 ledger quota", led.over_quota(1, 99) && !led.over_quota(1, 100), "quota wrong");

    // F012
    let ms = [200u32, 300, 500];
    set.add("F012 fork profile", fork_stage_permille(&ms, 0) == 200 && fork_stage_permille(&ms, 2) == 500, "profile wrong");

    // F013
    let mut out = [0u8; TNAME_MAX];
    let n = thread_name_norm(b"worker thread x", &mut out);
    set.add("F013 name norm", n == 15 && &out[..6] == b"worker", "norm wrong");
    set.add("F013 name space", out[6] == b'_', "space wrong");

    // F014
    set.add("F014 band", prio_band(200) == 3 && prio_band(63) == 0, "band wrong");
    let clamped = prio_clamp_within_band(10, 200);
    let up = prio_clamp_within_band(250, 200);
    set.add("F014 clamp low", clamped == 192, "clamp low");
    set.add("F014 clamp high", up == 250, "clamp high");

    // F015
    set.add("F015 canary", stack_canary_intact(STACK_CANARY) && !stack_canary_intact(0), "canary wrong");

    // F016
    set.add("F016 zombie timeout", zombie_reap_on_timeout(100, 99) == Some(PState::Dead), "timeout wrong");
    set.add("F016 zombie grace", zombie_reap_on_timeout(99, 99).is_none(), "grace wrong");

    // F017
    let c0 = cancel_advance(0, false);
    let c1 = cancel_advance(0, true);
    let c2 = cancel_advance(c1, false);
    let c3 = cancel_advance(c2, false);
    set.add("F017 cancel wait", c0 == 0, "wait wrong");
    set.add("F017 cancel chain", c1 == CANCEL_PENDING && c2 == CANCEL_CLEANUP && c3 == CANCEL_DONE, "chain wrong");

    // F018
    let mut fc = FreezeCapsule::new();
    fc.frozen = true;
    fc.signal(9);
    fc.signal(10);
    let drained = fc.thaw();
    set.add("F018 freeze hold", drained == 2 && fc.pending[0] == 9, "hold wrong");
    set.add("F018 freeze drain", fc.len == 0 && !fc.frozen, "drain wrong");

    // F019
    set.add("F019 flags ok", clone_flags_valid(CL_NEWMM | CL_NEWSIG), "ok wrong");
    set.add("F019 flags bad", !clone_flags_valid(CL_NEWMM), "bad wrong");
    set.add("F019 flags plain", clone_flags_valid(CL_NEWTLS), "plain wrong");

    // F020
    let mut au = PrivAudit::new();
    au.record(0, true);
    let dup = au.record(0, true);
    au.record(5, false);
    set.add("F020 audit record", au.len == 2 && !dup, "record wrong");
    set.add("F020 audit elevation", au.elevation_count() == 1, "elevation wrong");

    // F021
    set.add("F021 affinity valid", affinity_valid(0b101, 3), "valid wrong");
    set.add("F021 affinity oob", !affinity_valid(0b1000, 3), "oob wrong");
    set.add("F021 affinity empty", !affinity_valid(0, 3), "empty wrong");

    // F022
    let mut name = [0u8; TNAME_MAX];
    let nl = thread_name_norm(b"init", &mut name);
    let pd = ProcDesc { name, name_len: nl, state: PState::Running, parent: 0, version: DESC_VERSION };
    let mut dbuf = [0u8; 32];
    let dn = pd.describe(&mut dbuf);
    set.add("F022 desc render", dn == 8 && &dbuf[..8] == b"init:run", "render wrong");
    set.add("F022 desc version", pd.version == DESC_VERSION, "version wrong");

    // F023
    let snap = snapshot_save(0x7FFF0, 0x4000);
    let bad = snapshot_save(0x7FFF3, 0x4000);
    set.add("F023 snapshot save", snap.valid && !bad.valid, "save wrong");
    set.add("F023 snapshot restore", snapshot_restore(&snap, 0x8000) && !snapshot_restore(&snap, 0x7FFF0), "restore wrong");

    // F024
    let mut pool = ForkPool::new();
    let added = pool.prewarm(100);
    let t1 = pool.take();
    let t2 = pool.take();
    set.add("F024 pool prewarm", added == 4 && pool.prewarmed == 4, "prewarm wrong");
    set.add("F024 pool serve", t1 == Some(100) && t2 == Some(101) && pool.served == 2, "serve wrong");

    // F025
    let mut yb = ProcYearbook::new();
    yb.record_fork();
    yb.record_exit();
    yb.record_exit();
    yb.record_reap();
    set.add("F025 yearbook unclean", yb.unclean_exits() == 1 && yb.forks == 1, "yearbook wrong");

    set
}

// ===========================================================================
// 单测
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f001_full_lifecycle() {
        let mut s = PState::New;
        s = proc_transition(s, PEvent::Admit);
        assert_eq!(s, PState::Ready);
        s = proc_transition(s, PEvent::Dispatch);
        assert_eq!(s, PState::Running);
        s = proc_transition(s, PEvent::Block);
        assert_eq!(s, PState::Blocked);
        s = proc_transition(s, PEvent::Wake);
        assert_eq!(s, PState::Ready);
        s = proc_transition(s, PEvent::Dispatch);
        s = proc_transition(s, PEvent::Exit);
        assert_eq!(s, PState::Zombie);
        s = proc_transition(s, PEvent::Reap);
        assert_eq!(s, PState::Dead);
    }

    #[test]
    fn f001_yield_roundtrip() {
        let s = proc_transition(PState::Running, PEvent::Yield);
        assert_eq!(s, PState::Ready);
    }

    #[test]
    fn f001_blocked_cannot_exit_directly() {
        assert_eq!(proc_transition(PState::Blocked, PEvent::Exit), PState::Blocked);
    }

    #[test]
    fn f002_cow_lifecycle() {
        let mut p = CowPage { refs: 1, writable: true };
        fork_page(&mut p);
        fork_page(&mut p);
        assert_eq!(p.refs, 3);
        assert!(!p.writable);
        cow_break(&mut p);
        cow_break(&mut p);
        assert_eq!(p.refs, 1);
        cow_break(&mut p);
        assert!(p.writable);
        cow_break(&mut p);
        assert!(p.writable);
    }

    #[test]
    fn f003_header_matrix() {
        assert!(exec_header_ok(&[0x7F, b'E', b'L', b'F', 2, 0, 0, 0]));
        assert!(!exec_header_ok(&[0x7F, b'E', b'L', b'F', 1, 0, 0, 0]));
        assert!(!exec_header_ok(&[0x00, b'E', b'L', b'F', 2, 0, 0, 0]));
    }

    #[test]
    fn f004_thread_ownership_blocks_share() {
        let a = ThreadDesc { tid: 1, pid: 1, owns_mm: true };
        let b = ThreadDesc { tid: 2, pid: 1, owns_mm: false };
        assert!(!threads_share_mm(&a, &b));
    }

    #[test]
    fn f005_queue_overflow() {
        let mut q = WaitQueue::new();
        for t in 0..WQ_MAX as u16 {
            assert!(q.wait(t));
        }
        assert!(!q.wait(99));
        assert_eq!(q.wake_one(), Some(0));
        assert!(q.wait(99));
    }

    #[test]
    fn f005_wake_empty() {
        let mut q = WaitQueue::new();
        assert!(q.wake_one().is_none());
    }

    #[test]
    fn f006_adopt_paths() {
        assert_eq!(orphan_adopt(1, false), INIT_PID);
        assert_eq!(orphan_adopt(1, true), 1);
        assert_eq!(orphan_adopt(0, false), INIT_PID);
    }

    #[test]
    fn f007_cap_bit_exact() {
        let c = Cred { uid: 7, gid: 0, caps: 0b111111 };
        assert!(cred_can_kill(&c, 100));
        let c2 = Cred { uid: 7, gid: 0, caps: CAP_KILL << 1 };
        assert!(!cred_can_kill(&c2, 100));
    }

    #[test]
    fn f008_group_leader_rule() {
        let p = ProcGroup { pid: 1, pgid: 2, sid: 1 };
        assert_eq!(setsid(&p), Some(1));
    }

    #[test]
    fn f009_cleanup_order_fixed() {
        assert_eq!(CLEANUP_ORDER[0], "close-handles");
        assert_eq!(CLEANUP_ORDER[2], "flush-files");
    }

    #[test]
    fn f010_tls_isolation() {
        let mut a = TlsBlock::new();
        let mut b = TlsBlock::new();
        a.set(1, 111);
        assert_eq!(b.get(1), Some(0));
        a.set(1, 222);
        assert_eq!(a.get(1), Some(222));
        assert_eq!(b.get(1), Some(0));
    }

    #[test]
    fn f011_ledger_capacity() {
        let mut l = ResourceLedger::new();
        for k in 0..LEDGER_MAX as u8 {
            assert!(l.record(k, 1));
        }
        assert!(!l.record(99, 1));
        assert_eq!(l.len, LEDGER_MAX);
    }

    #[test]
    fn f012_profile_sums() {
        let ms = [1u32, 1, 2];
        assert_eq!(fork_stage_permille(&ms, 0) + fork_stage_permille(&ms, 1) + fork_stage_permille(&ms, 2), 1000);
        assert_eq!(fork_stage_permille(&[0; 3], 1), 0);
    }

    #[test]
    fn f013_name_truncates() {
        let mut out = [0u8; TNAME_MAX];
        let n = thread_name_norm(&[b'a'; 40], &mut out);
        assert_eq!(n, TNAME_MAX);
        assert!(out.iter().all(|&b| b == b'a'));
    }

    #[test]
    fn f014_band_edges() {
        assert_eq!(prio_band(0), 0);
        assert_eq!(prio_band(64), 1);
        assert_eq!(prio_band(128), 2);
        assert_eq!(prio_band(255), 3);
        assert_eq!(prio_clamp_within_band(0, 64), 64);
        assert_eq!(prio_clamp_within_band(64, 64), 64);
    }

    #[test]
    fn f015_canary_constant() {
        assert_ne!(STACK_CANARY, 0);
        assert!(stack_canary_intact(STACK_CANARY));
    }

    #[test]
    fn f016_timeout_boundary() {
        assert!(zombie_reap_on_timeout(0, 0).is_none());
        assert_eq!(zombie_reap_on_timeout(1, 0), Some(PState::Dead));
    }

    #[test]
    fn f017_done_is_terminal() {
        assert_eq!(cancel_advance(CANCEL_DONE, true), CANCEL_DONE);
    }

    #[test]
    fn f018_freeze_overflow_drops() {
        let mut f = FreezeCapsule::new();
        f.frozen = true;
        for s in 0..(FREEZE_SIG_MAX as u8 + 2) {
            f.signal(s);
        }
        assert_eq!(f.len, FREEZE_SIG_MAX);
        assert_eq!(f.dropped, 2);
    }

    #[test]
    fn f019_flags_matrix() {
        assert!(clone_flags_valid(0));
        assert!(clone_flags_valid(CL_NEWSIG));
        assert!(clone_flags_valid(CL_NEWTLS | CL_NEWSIG));
        assert!(!clone_flags_valid(CL_NEWMM | CL_NEWTLS));
    }

    #[test]
    fn f020_audit_capacity_dedup() {
        let mut a = PrivAudit::new();
        for uid in 0..AUDIT_MAX as u16 {
            assert!(a.record(uid, uid % 2 == 0));
        }
        assert!(!a.record(0, true));
        assert_eq!(a.dup_rejected, 1);
        assert_eq!(a.elevation_count(), AUDIT_MAX / 2);
    }

    #[test]
    fn f021_affinity_edges() {
        assert!(!affinity_valid(u64::MAX, 64));
        assert!(affinity_valid(1, 1));
        assert!(!affinity_valid(0b11, 1));
    }

    #[test]
    fn f022_describe_states() {
        let mut name = [0u8; TNAME_MAX];
        let nl = thread_name_norm(b"kth", &mut name);
        let pd = ProcDesc { name, name_len: nl, state: PState::Zombie, parent: 0, version: DESC_VERSION };
        let mut o = [0u8; 32];
        let n = pd.describe(&mut o);
        assert_eq!(&o[..n], b"kth:zmb");
    }

    #[test]
    fn f023_snapshot_zero_rejected() {
        assert!(!snapshot_save(0, 0x4000).valid);
        assert!(!snapshot_save(0x8000, 0).valid);
    }

    #[test]
    fn f024_pool_exhaust_then_none() {
        let mut p = ForkPool::new();
        p.prewarm(1);
        for i in 0..4 {
            assert_eq!(p.take(), Some(1 + i));
        }
        assert!(p.take().is_none());
        assert_eq!(p.served, 4);
    }

    #[test]
    fn f025_yearbook_no_underflow() {
        let mut y = ProcYearbook::new();
        y.record_reap();
        assert_eq!(y.unclean_exits(), 0);
        y.record_exit();
        y.record_exit();
        assert_eq!(y.unclean_exits(), 1);
    }

    #[test]
    fn domain_self_test_must_pass() {
        let set = run_m700proc_checks();
        if !set.all_passed() {
            let mut buf = [0u8; 1024];
            let n = set.render(&mut buf);
            panic!("m700proc self-test:\n{}", core::str::from_utf8(&buf[..n]).unwrap());
        }
        assert!(set.len() >= 25);
    }
}
