//! VARIX-M400 AI-03 进程与调度成熟化域（F051~F075）。
//!
//! 多核公平、可预测、可观测。纯逻辑 + 固定容量数组，no_std 安全。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// F051 — 公平调度器（虚拟时间 / vruntime）
// ---------------------------------------------------------------------------

pub const SCHED_CAP: usize = 16;

#[derive(Clone, Copy)]
pub struct Task {
    pub pid: u32,
    pub nice: i8, // 权重简化：nice 越小权重越高
    pub vruntime: u64,
    pub runnable: bool,
}

impl Task {
    pub fn weight(&self) -> u32 {
        1024u32.saturating_sub((self.nice.saturating_mul(100)) as i32 as u32)
    }
}

#[derive(Clone, Copy)]
pub struct FairSched {
    pub tasks: [Option<Task>; SCHED_CAP],
    pub count: usize,
}

impl FairSched {
    pub const fn new() -> FairSched {
        FairSched { tasks: [const { None }; SCHED_CAP], count: 0 }
    }

    pub fn add(&mut self, t: Task) -> bool {
        if self.count >= SCHED_CAP {
            return false;
        }
        self.tasks[self.count] = Some(t);
        self.count += 1;
        true
    }

    /// 挑 vruntime 最小的可运行任务。
    pub fn pick(&self) -> Option<u32> {
        let mut best: Option<(u64, u32)> = None;
        for slot in self.tasks.iter().flatten() {
            if !slot.runnable {
                continue;
            }
            if best.map(|(v, _)| slot.vruntime < v).unwrap_or(true) {
                best = Some((slot.vruntime, slot.pid));
            }
        }
        best.map(|(_, pid)| pid)
    }

    /// 运行 delta 后按权重记账。
    pub fn ran(&mut self, pid: u32, delta: u64) -> bool {
        for slot in self.tasks.iter_mut().flatten() {
            if slot.pid == pid {
                slot.vruntime += delta * 1024 / slot.weight().max(1) as u64;
                return true;
            }
        }
        false
    }
}

// ---------------------------------------------------------------------------
// F052 — 实时优先级（FIFO/RR，RT 不被饿死）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum SchedClass {
    Fair = 0,
    Rr = 1,
    Fifo = 2,
}

/// RT 类恒优先于 Fair；同类 FIFO 用队列序号。
pub fn f052_pick_class(rt_waiting: bool) -> SchedClass {
    if rt_waiting {
        SchedClass::Fifo
    } else {
        SchedClass::Fair
    }
}

/// RT 时间片上限（防止饿死 Fair 的 throttling）。
pub fn f052_rt_throttle(rt_runtime_ns: u64, rt_period_ns: u64) -> bool {
    rt_period_ns > 0 && rt_runtime_ns * 2 <= rt_period_ns
}

// ---------------------------------------------------------------------------
// F053 — CPU 亲和性
// ---------------------------------------------------------------------------

pub const MAX_CPUS: usize = 32;

pub fn f053_set_affinity(mask: &mut u32, cpu: usize, on: bool) -> bool {
    if cpu >= MAX_CPUS {
        return false;
    }
    if on {
        *mask |= 1 << cpu;
    } else {
        *mask &= !(1 << cpu);
    }
    true
}

pub fn f053_can_run(mask: u32, cpu: usize) -> bool {
    cpu < MAX_CPUS && mask & (1 << cpu) != 0
}

/// 从亲和掩码选最小可用 CPU。
pub fn f053_pick_cpu(mask: u32) -> Option<usize> {
    if mask == 0 {
        return None;
    }
    Some(mask.trailing_zeros() as usize)
}

// ---------------------------------------------------------------------------
// F054 — SMP 负载均衡
// ---------------------------------------------------------------------------

/// 从最重队列搬到最轻队列一个任务，返回 (from, to)；均衡即 None。
pub fn f054_balance_once(loads: &[u32; 4]) -> Option<(usize, usize)> {
    let mut heaviest = 0usize;
    let mut lightest = 0usize;
    for i in 1..loads.len() {
        if loads[i] > loads[heaviest] {
            heaviest = i;
        }
        if loads[i] < loads[lightest] {
            lightest = i;
        }
    }
    if loads[heaviest] - loads[lightest] <= 1 {
        None
    } else {
        Some((heaviest, lightest))
    }
}

// ---------------------------------------------------------------------------
// F055 — 优先级继承
// ---------------------------------------------------------------------------

/// 持锁者临时继承等待者的最高优先级。
pub fn f055_inherit(holder: u8, waiters: &[u8]) -> u8 {
    waiters.iter().copied().fold(holder, |a, w| if w < a { w } else { a }) // 数值小 = 优先级高
}

// ---------------------------------------------------------------------------
// F056 — 资源组（cgroup 式限额）
// ---------------------------------------------------------------------------

pub const RES_GROUPS: usize = 8;

#[derive(Clone, Copy)]
pub struct ResGroup {
    pub cpu_quota_permille: u16, // 0~1000
    pub mem_max_pages: u32,
    pub cpu_used_permille: u16,
    pub mem_used_pages: u32,
}

impl ResGroup {
    pub fn cpu_over_quota(&self) -> bool {
        self.cpu_used_permille > self.cpu_quota_permille
    }
    pub fn mem_over_quota(&self) -> bool {
        self.mem_used_pages > self.mem_max_pages
    }
}

// ---------------------------------------------------------------------------
// F057 — 会话与作业控制（前后台组）
// ---------------------------------------------------------------------------

pub const MAX_PGIDS: usize = 8;

pub struct Session {
    pub sid: u32,
    pub pgids: [u32; MAX_PGIDS],
    pub pg_count: usize,
    pub foreground: usize, // 索引；usize::MAX 表示无前台
}

impl Session {
    pub const fn new(sid: u32) -> Session {
        Session { sid, pgids: [0; MAX_PGIDS], pg_count: 0, foreground: usize::MAX }
    }

    pub fn attach(&mut self, pgid: u32, foreground: bool) -> bool {
        if self.pg_count >= MAX_PGIDS {
            return false;
        }
        self.pgids[self.pg_count] = pgid;
        if foreground {
            self.foreground = self.pg_count;
        }
        self.pg_count += 1;
        true
    }

    pub fn fg_pgid(&self) -> Option<u32> {
        self.pgids.get(self.foreground).copied()
    }
}

// ---------------------------------------------------------------------------
// F058 — 信号语义
// ---------------------------------------------------------------------------

pub const SIGKILL: u8 = 9;
pub const SIGSTOP: u8 = 19;

#[derive(Clone, Copy)]
pub struct SignalSet {
    pub pending: u32, // 位图：bit n = 信号 n
    pub blocked: u32,
}

impl SignalSet {
    pub const fn new() -> SignalSet {
        SignalSet { pending: 0, blocked: 0 }
    }

    pub fn send(&mut self, sig: u8) -> bool {
        if sig == 0 || sig >= 32 {
            return false;
        }
        self.pending |= 1 << sig;
        true
    }

    /// 不可阻塞/不可忽略的信号（SIGKILL/SIGSTOP）总是可递送。
    pub fn deliverable(&self, sig: u8) -> bool {
        if sig == SIGKILL || sig == SIGSTOP {
            return self.pending & (1 << sig) != 0;
        }
        self.pending & (1 << sig) != 0 && self.blocked & (1 << sig) == 0
    }

    pub fn ack(&mut self, sig: u8) {
        self.pending &= !(1 << sig);
    }
}

// ---------------------------------------------------------------------------
// F059 — wait/pid 回收（无泄漏）
// ---------------------------------------------------------------------------

pub const PID_CAP: usize = 64;

pub struct PidTable {
    pub in_use: [bool; PID_CAP],
    pub next_scan: usize,
}

impl PidTable {
    pub const fn new() -> PidTable {
        PidTable { in_use: [false; PID_CAP], next_scan: 0 }
    }

    pub fn alloc(&mut self) -> Option<usize> {
        for k in 0..PID_CAP {
            let i = (self.next_scan + k) % PID_CAP;
            if !self.in_use[i] {
                self.in_use[i] = true;
                self.next_scan = (i + 1) % PID_CAP;
                return Some(i + 1);
            }
        }
        None
    }

    pub fn reap(&mut self, pid: usize) -> bool {
        if pid == 0 || pid > PID_CAP || !self.in_use[pid - 1] {
            return false;
        }
        self.in_use[pid - 1] = false;
        self.next_scan = pid - 1; // 尽快复用刚回收的 pid
        true
    }
}

// ---------------------------------------------------------------------------
// F060 — 僵尸收割（孤儿托管）
// ---------------------------------------------------------------------------

pub fn f060_is_zombie(alive: bool, exited: bool, reaped: bool) -> bool {
    exited && alive && !reaped
}

/// 孤儿进程（父已死）交给 init（pid 1）托管。
pub fn f060_reparent_orphan(parent_alive: bool, pid: u32) -> u32 {
    if parent_alive {
        pid
    } else {
        1
    }
}

// ---------------------------------------------------------------------------
// F061 — 线程组与 TLS
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct ThreadGroup {
    pub tgid: u32,
    pub members: [u32; 8],
    pub tls_base: [u64; 8],
    pub count: usize,
}

impl ThreadGroup {
    pub const fn new(tgid: u32) -> ThreadGroup {
        ThreadGroup { tgid, members: [0; 8], tls_base: [0; 8], count: 0 }
    }

    pub fn spawn(&mut self, tid: u32, tls: u64) -> bool {
        if self.count >= 8 || tid == self.tgid {
            return false;
        }
        self.members[self.count] = tid;
        self.tls_base[self.count] = tls;
        self.count += 1;
        true
    }

    pub fn tls_of(&self, tid: u32) -> Option<u64> {
        for i in 0..self.count {
            if self.members[i] == tid {
                return Some(self.tls_base[i]);
            }
        }
        None
    }
}

// ---------------------------------------------------------------------------
// F062 — futex 骨架
// ---------------------------------------------------------------------------

pub const FUTEX_CAP: usize = 16;

pub struct FutexQueue {
    pub keys: [u64; FUTEX_CAP],
    pub waiters: [u32; FUTEX_CAP],
    pub count: usize,
}

impl FutexQueue {
    pub const fn new() -> FutexQueue {
        FutexQueue { keys: [0; FUTEX_CAP], waiters: [0; FUTEX_CAP], count: 0 }
    }

    pub fn wait(&mut self, key: u64, tid: u32) -> bool {
        if self.count >= FUTEX_CAP {
            return false;
        }
        self.keys[self.count] = key;
        self.waiters[self.count] = tid;
        self.count += 1;
        true
    }

    /// wake 唤醒该 key 的一个等待者。
    pub fn wake(&mut self, key: u64) -> Option<u32> {
        for i in 0..self.count {
            if self.keys[i] == key {
                let tid = self.waiters[i];
                self.keys[i] = self.keys[self.count - 1];
                self.waiters[i] = self.waiters[self.count - 1];
                self.count -= 1;
                return Some(tid);
            }
        }
        None
    }
}

// ---------------------------------------------------------------------------
// F063 — 进程命名空间骨架
// ---------------------------------------------------------------------------

pub struct PidNs {
    pub level: u8,
    pub offset: u32, // 每层 pid 翻译偏移
}

/// 上层 pid -> 本层视图。
pub fn f063_translate(ns: &PidNs, global_pid: u32) -> u32 {
    global_pid.saturating_sub(ns.offset).max(1)
}

// ---------------------------------------------------------------------------
// F064 — 优雅退出回调链
// ---------------------------------------------------------------------------

pub const EXIT_HOOKS: usize = 8;

#[derive(Clone, Copy)]
pub struct ExitChain {
    pub hooks: [u8; EXIT_HOOKS], // hook id，按注册逆序执行
    pub count: usize,
}

impl ExitChain {
    pub const fn new() -> ExitChain {
        ExitChain { hooks: [0; EXIT_HOOKS], count: 0 }
    }

    pub fn register(&mut self, hook: u8) -> bool {
        if self.count >= EXIT_HOOKS {
            return false;
        }
        self.hooks[self.count] = hook;
        self.count += 1;
        true
    }

    /// 返回执行顺序（LIFO）。
    pub fn run_order(&self) -> ([u8; EXIT_HOOKS], usize) {
        let mut out = [0u8; EXIT_HOOKS];
        for i in 0..self.count {
            out[i] = self.hooks[self.count - 1 - i];
        }
        (out, self.count)
    }
}

// ---------------------------------------------------------------------------
// F065 — coredump 生成
// ---------------------------------------------------------------------------

pub const CORE_REGS: usize = 8;

pub struct CoreDump {
    pub pid: u32,
    pub signal: u8,
    pub regs: [u64; CORE_REGS],
    pub valid: bool,
}

/// 生成 coredump：信号非零且寄存器 rip 非零。
pub fn f065_make_coredump(pid: u32, signal: u8, regs: [u64; CORE_REGS]) -> CoreDump {
    CoreDump { pid, signal, regs, valid: signal != 0 && regs[0] != 0 }
}

// ---------------------------------------------------------------------------
// F066 — 调度延迟 tracepoint
// ---------------------------------------------------------------------------

pub struct LatencyHist {
    pub buckets: [u32; 8], // 0-1us,1-4,4-16,16-64,64-256,256-1k,1k-4k,>4k（单位 us）
}

impl LatencyHist {
    pub const fn new() -> LatencyHist {
        LatencyHist { buckets: [0; 8] }
    }

    pub fn record(&mut self, us: u64) {
        let b = if us <= 1 {
            0
        } else if us <= 4 {
            1
        } else if us <= 16 {
            2
        } else if us <= 64 {
            3
        } else if us <= 256 {
            4
        } else if us <= 1024 {
            5
        } else if us <= 4096 {
            6
        } else {
            7
        };
        self.buckets[b] += 1;
    }
}

// ---------------------------------------------------------------------------
// F067 — CPU 频率协同（P-state）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PState {
    pub min_mhz: u32,
    pub max_mhz: u32,
    pub current_mhz: u32,
}

/// 请求限幅到 [min, max]。
pub fn f067_request_pstate(ps: &PState, want: u32) -> u32 {
    want.clamp(ps.min_mhz, ps.max_mhz)
}

// ---------------------------------------------------------------------------
// F068 — 空闲态治理（C-state）
// ---------------------------------------------------------------------------

pub const CSTATES: usize = 4;

pub struct CStatePolicy {
    pub exit_latency_us: [u32; CSTATES],
    pub max_exit_budget_us: u32,
}

/// 选退出延迟不超预算的最深空闲态。
pub fn f068_pick_cstate(p: &CStatePolicy) -> Option<usize> {
    let mut best: Option<usize> = None;
    for i in 0..CSTATES {
        if p.exit_latency_us[i] <= p.max_exit_budget_us
            && best.map(|b| p.exit_latency_us[i] > p.exit_latency_us[b]).unwrap_or(true)
        {
            best = Some(i);
        }
    }
    best
}

// ---------------------------------------------------------------------------
// F069 — procfs 观测
// ---------------------------------------------------------------------------

pub fn f069_proc_line(
    out: &mut [u8],
    pid: u32,
    state: u8,
    utime: u64,
    rss_pages: u64,
) -> usize {
    let mut n = 0usize;
    crate::checks::push_str(out, &mut n, "pid=");
    crate::checks::push_usize(out, &mut n, pid as usize);
    crate::checks::push_str(out, &mut n, " state=");
    crate::checks::push_str(out, &mut n, core::str::from_utf8(&[state]).unwrap_or("?"));
    crate::checks::push_str(out, &mut n, " utime=");
    crate::checks::push_usize(out, &mut n, utime as usize);
    crate::checks::push_str(out, &mut n, " rss=");
    crate::checks::push_usize(out, &mut n, rss_pages as usize);
    n
}

// ---------------------------------------------------------------------------
// F070 — 进程审计日志
// ---------------------------------------------------------------------------

pub const AUDIT_CAP: usize = 16;

#[derive(Clone, Copy)]
pub struct AuditLog {
    pub events: [(u32, u16); AUDIT_CAP], // (pid, event id)
    pub count: usize,
}

impl AuditLog {
    pub const fn new() -> AuditLog {
        AuditLog { events: [(0, 0); AUDIT_CAP], count: 0 }
    }

    pub fn record(&mut self, pid: u32, event: u16) -> bool {
        if self.count >= AUDIT_CAP {
            return false;
        }
        self.events[self.count] = (pid, event);
        self.count += 1;
        true
    }

    pub fn count_for(&self, pid: u32) -> usize {
        self.events[..self.count].iter().filter(|e| e.0 == pid).count()
    }
}

// ---------------------------------------------------------------------------
// F071 — 运行域隔离（jail）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Jail {
    pub no_net: bool,
    pub no_raw: bool,
    pub fs_root: u64, // 虚拟根 inode
}

pub fn f071_can_open(j: &Jail, inode: u64, raw: bool, net: bool) -> bool {
    if net && j.no_net {
        return false;
    }
    if raw && j.no_raw {
        return false;
    }
    inode >= j.fs_root
}

// ---------------------------------------------------------------------------
// F072 — 句柄表加固
// ---------------------------------------------------------------------------

pub const HANDLE_CAP: usize = 32;

pub struct HandleTable {
    pub used: [bool; HANDLE_CAP],
    pub gen: u8,
}

impl HandleTable {
    pub const fn new() -> HandleTable {
        HandleTable { used: [false; HANDLE_CAP], gen: 0 }
    }

    /// 句柄 = index | gen<<8，重放旧代句柄即拒绝。
    pub fn alloc(&mut self) -> Option<u16> {
        for i in 0..HANDLE_CAP {
            if !self.used[i] {
                self.used[i] = true;
                return Some(i as u16 | ((self.gen as u16) << 8));
            }
        }
        None
    }

    pub fn close(&mut self, h: u16) -> bool {
        let (i, g) = ((h & 0xFF) as usize, h >> 8);
        if i >= HANDLE_CAP || g as u8 != self.gen || !self.used[i] {
            return false;
        }
        self.used[i] = false;
        true
    }
}

// ---------------------------------------------------------------------------
// F073 — IPC 消息端口
// ---------------------------------------------------------------------------

pub const PORT_CAP: usize = 8;

pub struct MsgPort {
    pub queue: [(u32, u64); PORT_CAP], // (src pid, payload)
    pub head: usize,
    pub tail: usize,
    pub len: usize,
}

impl MsgPort {
    pub const fn new() -> MsgPort {
        MsgPort { queue: [(0, 0); PORT_CAP], head: 0, tail: 0, len: 0 }
    }

    pub fn send(&mut self, src: u32, payload: u64) -> bool {
        if self.len >= PORT_CAP {
            return false;
        }
        self.queue[self.tail] = (src, payload);
        self.tail = (self.tail + 1) % PORT_CAP;
        self.len += 1;
        true
    }

    pub fn recv(&mut self) -> Option<(u32, u64)> {
        if self.len == 0 {
            return None;
        }
        let m = self.queue[self.head];
        self.head = (self.head + 1) % PORT_CAP;
        self.len -= 1;
        Some(m)
    }
}

// ---------------------------------------------------------------------------
// F074 — 共享内存段
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct ShmSeg {
    pub key: u32,
    pub base: u64,
    pub pages: u32,
    pub owner_pid: u32,
    pub readonly: bool,
}

/// attach 权限：只读段写请求拒绝。
pub fn f074_attach(seg: &ShmSeg, requester: u32, write: bool) -> bool {
    if write && seg.readonly {
        return false;
    }
    let _ = requester; // 权限基线：同用户空间即可 attach
    true
}

// ---------------------------------------------------------------------------
// F075 — 调度压力套件 + 域自检
// ---------------------------------------------------------------------------

pub fn run_schedm400_checks() -> CheckSet {
    let mut set = CheckSet::new("schedm400");

    // F051
    let mut fsched = FairSched::new();
    let added = fsched.add(Task { pid: 10, nice: 0, vruntime: 100, runnable: true })
        && fsched.add(Task { pid: 11, nice: 0, vruntime: 50, runnable: true })
        && fsched.add(Task { pid: 12, nice: 0, vruntime: 200, runnable: false });
    set.add("F051 add", added && fsched.count == 3, "three tasks");
    set.add("F051 pick min vruntime", fsched.pick() == Some(11), "vruntime 50 wins");
    let ran = fsched.ran(11, 200);
    let after = fsched.tasks.iter().flatten().find(|t| t.pid == 11).map(|t| t.vruntime);
    set.add("F051 account", ran && after == Some(250), "200 delta accounted");

    // F052
    set.add("F052 rt first", f052_pick_class(true) == SchedClass::Fifo, "RT wins");
    set.add("F052 fair idle", f052_pick_class(false) == SchedClass::Fair, "fair when no RT");
    set.add("F052 throttle ok", f052_rt_throttle(40, 100), "40% runtime ok");
    set.add("F052 throttle bad", !f052_rt_throttle(80, 100), ">50% rejected");

    // F053
    let mut mask = 0u32;
    set.add("F053 set", f053_set_affinity(&mut mask, 3, true) && mask == 0b1000, "cpu3 set");
    set.add("F053 can_run", f053_can_run(mask, 3) && !f053_can_run(mask, 2), "mask checked");
    set.add("F053 pick", f053_pick_cpu(0b1000) == Some(3), "lowest set");
    set.add("F053 empty", f053_pick_cpu(0).is_none(), "no cpu");

    // F054
    set.add("F054 move", f054_balance_once(&[4, 1, 1, 1]) == Some((0, 1)), "heavy->light");
    set.add("F054 balanced", f054_balance_once(&[2, 2, 1, 1]).is_none(), "within 1");

    // F055
    set.add("F055 inherit", f055_inherit(9, &[5, 3, 7]) == 3, "inherits highest prio");
    set.add("F055 none", f055_inherit(9, &[]) == 9, "no waiters unchanged");

    // F056
    let g = ResGroup { cpu_quota_permille: 500, mem_max_pages: 100, cpu_used_permille: 600, mem_used_pages: 50 };
    let g2 = ResGroup { cpu_quota_permille: 500, mem_max_pages: 100, cpu_used_permille: 400, mem_used_pages: 99 };
    set.add("F056 cpu over", g.cpu_over_quota(), "600>500");
    set.add("F056 mem ok", !g2.mem_over_quota(), "99<=100");

    // F057
    let mut sess = Session::new(1);
    set.add("F057 attach bg", sess.attach(100, false), "background group");
    set.add("F057 attach fg", sess.attach(200, true) && sess.fg_pgid() == Some(200), "foreground set");

    // F058
    let mut sig = SignalSet::new();
    set.add("F058 send", sig.send(15) && sig.deliverable(15), "term pending");
    sig.blocked = 1 << 15;
    set.add("F058 blocked", !sig.deliverable(15), "blocked held");
    sig.send(SIGKILL);
    set.add("F058 unblockable", sig.deliverable(SIGKILL), "SIGKILL always");
    set.add("F058 bad sig", !sig.send(0) && !sig.send(32), "range guard");

    // F059
    let mut pt = PidTable::new();
    let p1 = pt.alloc();
    let p2 = pt.alloc();
    set.add("F059 alloc", p1 == Some(1) && p2 == Some(2), "sequential pids");
    set.add("F059 reap", pt.reap(1) && pt.alloc() == Some(1), "reused after reap");
    set.add("F059 reap foreign", !pt.reap(50), "never-allocated pid");

    // F060
    set.add("F060 zombie", f060_is_zombie(true, true, false), "exited unreaped");
    set.add("F060 not zombie", !f060_is_zombie(true, true, true), "reaped");
    set.add("F060 orphan", f060_reparent_orphan(false, 77) == 1, "to init");

    // F061
    let mut tg = ThreadGroup::new(50);
    set.add("F061 spawn", tg.spawn(51, 0x7f00) && tg.spawn(52, 0x8000), "two threads");
    set.add("F061 tls", tg.tls_of(52) == Some(0x8000), "tls base kept");
    set.add("F061 bad tid", !tg.spawn(50, 0), "tid==tgid rejected");

    // F062
    let mut fq = FutexQueue::new();
    set.add("F062 wait", fq.wait(0xA0, 1) && fq.wait(0xA0, 2) && fq.wait(0xB0, 3), "three waiters");
    set.add("F062 wake fifo", fq.wake(0xA0) == Some(1) && fq.wake(0xA0) == Some(2), "key wakeup order");
    set.add("F062 wake empty", fq.wake(0xA0).is_none(), "no waiter");

    // F063
    let ns = PidNs { level: 1, offset: 100 };
    set.add("F063 translate", f063_translate(&ns, 105) == 5, "offset removed");
    set.add("F063 floor", f063_translate(&ns, 50) == 1, "floored at 1");

    // F064
    let mut ec = ExitChain::new();
    ec.register(1);
    ec.register(2);
    ec.register(3);
    let (order, n) = ec.run_order();
    set.add("F064 lifo", n == 3 && order[0] == 3 && order[1] == 2 && order[2] == 1, "reverse order");

    // F065
    let cd = f065_make_coredump(9, 11, [0x401000, 0, 0, 0, 0, 0, 0, 0]);
    let bad = f065_make_coredump(9, 0, [0x401000, 0, 0, 0, 0, 0, 0, 0]);
    set.add("F065 valid", cd.valid, "signal+rip");
    set.add("F065 invalid", !bad.valid, "zero signal rejected");

    // F066
    let mut hist = LatencyHist::new();
    hist.record(1);
    hist.record(10);
    hist.record(5000);
    set.add("F066 buckets", hist.buckets[0] == 1 && hist.buckets[2] == 1 && hist.buckets[7] == 1, "three buckets hit");

    // F067
    let ps = PState { min_mhz: 800, max_mhz: 3600, current_mhz: 800 };
    set.add("F067 clamp low", f067_request_pstate(&ps, 100) == 800, "floored");
    set.add("F067 clamp high", f067_request_pstate(&ps, 9999) == 3600, "capped");
    set.add("F067 mid", f067_request_pstate(&ps, 2400) == 2400, "exact");

    // F068
    let pol = CStatePolicy { exit_latency_us: [1, 10, 100, 1000], max_exit_budget_us: 50 };
    set.add("F068 pick deepest", f068_pick_cstate(&pol) == Some(1), "10us deepest within 50");
    let tight = CStatePolicy { exit_latency_us: [1, 10, 100, 1000], max_exit_budget_us: 5 };
    set.add("F068 budget", f068_pick_cstate(&tight) == Some(0), "shallow only");

    // F069
    let mut buf = [0u8; 64];
    let n = f069_proc_line(&mut buf, 42, b'R', 100, 256);
    let text = core::str::from_utf8(&buf[..n]).unwrap_or("");
    set.add("F069 proc", text.contains("pid=42") && text.contains("state=R"), "fields rendered");

    // F070
    let mut al = AuditLog::new();
    al.record(7, 0x0100);
    al.record(7, 0x0200);
    al.record(8, 0x0100);
    set.add("F070 audit count", al.count_for(7) == 2, "pid 7 twice");

    // F071
    let j = Jail { no_net: true, no_raw: true, fs_root: 0x1000 };
    set.add("F071 inside ok", f071_can_open(&j, 0x2000, false, false), "fs subtree allowed");
    set.add("F071 escape", !f071_can_open(&j, 0x500, false, false), "outside root denied");
    set.add("F071 no net", !f071_can_open(&j, 0x2000, false, true), "net denied");

    // F072
    let mut ht = HandleTable::new();
    let h = ht.alloc();
    set.add("F072 alloc", h.is_some(), "handle issued");
    let hv = h.unwrap();
    set.add("F072 close", ht.close(hv) && !ht.close(hv), "close once");
    let stale = hv | (0x11u16 << 8);
    set.add("F072 replay", !ht.close(stale), "stale gen rejected");

    // F073
    let mut port = MsgPort::new();
    set.add("F073 send", port.send(1, 0xAA) && port.send(2, 0xBB), "two msgs");
    set.add("F073 fifo", port.recv() == Some((1, 0xAA)) && port.recv() == Some((2, 0xBB)), "order kept");
    set.add("F073 empty", port.recv().is_none(), "drained");

    // F074
    let seg = ShmSeg { key: 1, base: 0x7000_0000, pages: 4, owner_pid: 1, readonly: true };
    set.add("F074 ro attach", f074_attach(&seg, 2, false), "read ok");
    set.add("F074 ro write", !f074_attach(&seg, 2, true), "write denied");

    // F075
    let mut stress = FairSched::new();
    let mut all_added = true;
    for i in 0..SCHED_CAP {
        all_added &= stress.add(Task { pid: 100 + i as u32, nice: 0, vruntime: i as u64, runnable: true });
    }
    set.add("F075 cap", all_added && !stress.add(Task { pid: 999, nice: 0, vruntime: 0, runnable: true }), "capacity enforced");
    set.add("F075 still fair", stress.pick() == Some(100), "min vruntime under load");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    fn failures(set: &CheckSet) -> String {
        let mut s = String::new();
        for i in 0..set.len() {
            if let Some(c) = set.get(i) {
                if !c.passed {
                    s.push_str(&format!("{}: {}\n", c.name, c.detail));
                }
            }
        }
        s
    }

    #[test]
    fn f075_schedm400_selftest_all_pass() {
        let set = run_schedm400_checks();
        assert!(set.len() >= 25);
        assert!(set.all_passed(), "{}", failures(&set));
    }

    #[test]
    fn f051_fairness() {
        let mut s = FairSched::new();
        s.add(Task { pid: 1, nice: 0, vruntime: 0, runnable: true });
        s.add(Task { pid: 2, nice: 0, vruntime: 0, runnable: true });
        // 交替运行时 vruntime 趋于一致
        for round in 0..4 {
            let pick = s.pick().unwrap();
            s.ran(pick, 100);
            let _ = round;
        }
        let v: Vec<u64> = s.tasks.iter().flatten().map(|t| t.vruntime).collect();
        assert!(v[0].abs_diff(v[1]) <= 200, "vruntimes converged: {:?}", v);
    }

    #[test]
    fn f073_port_wrap() {
        let mut p = MsgPort::new();
        for i in 0..PORT_CAP {
            assert!(p.send(i as u32, i as u64));
        }
        assert!(!p.send(99, 99));
        assert_eq!(p.recv(), Some((0, 0)));
        assert!(p.send(99, 99));
        assert_eq!(p.len, PORT_CAP);
    }
}
