//! m600svc — VARIX-M600 AI-18 服务与进程域 (F426~F450)
//!
//! 服务总线中枢/服务依赖图/惰性启动公约/服务健康心跳/进程家谱/
//! 资源配额官/僵尸进程清道夫/优雅退出公约/信号礼仪/会话管理器/
//! 用户多实例隔离/权限分离执行体/最小特权审计/服务降级阶梯/
//! 服务热更新/系统调用审计/句柄泄漏纠察/进程优先级策展/
//! 崩溃隔离舱/服务时间轴/依赖死锁拆解/服务文档化/进程回放/
//! 服务回归走廊/服务年报。
//!
//! 硬约束：no_std / 无 alloc / 无浮点（全部 permille/定点）/ 纯逻辑。

use crate::checks::CheckSet;

// ===========================================================================
// F426 — 服务总线中枢：服务注册与寻址
// ===========================================================================

pub const SVC_MAX: usize = 16;

#[derive(Clone, Copy)]
pub struct ServiceRegistry {
    ids: [u32; SVC_MAX],
    count: usize,
}

impl ServiceRegistry {
    pub const fn new() -> ServiceRegistry {
        ServiceRegistry { ids: [0; SVC_MAX], count: 0 }
    }
    /// 注册：重复 id 拒绝，满员拒绝。
    pub fn register(&mut self, id: u32) -> bool {
        for i in 0..self.count {
            if self.ids[i] == id {
                return false;
            }
        }
        if self.count >= SVC_MAX {
            return false;
        }
        self.ids[self.count] = id;
        self.count += 1;
        true
    }
    pub fn lookup(&self, id: u32) -> bool {
        (0..self.count).any(|i| self.ids[i] == id)
    }
    pub fn len(&self) -> usize {
        self.count
    }
}

// ===========================================================================
// F427 — 服务依赖图：邻接表（每服务最多 3 依赖）
// ===========================================================================

pub const DEP_MAX_OUT: usize = 3;

/// 依赖边的合法性：无自依赖。
pub fn dep_edge_ok(from: usize, to: usize, service_count: usize) -> bool {
    from != to && from < service_count && to < service_count
}

// ===========================================================================
// F428 — 惰性启动公约：按需拉起
// ===========================================================================

#[derive(Clone, Copy)]
pub struct LazyService {
    pub started: bool,
    pub start_count: u32,
}

impl LazyService {
    pub const fn new() -> LazyService {
        LazyService { started: false, start_count: 0 }
    }
    /// 惰性启动：首次请求才 start，重复 start 不叠加。
    pub fn ensure_started(&mut self) -> bool {
        if self.started {
            false
        } else {
            self.started = true;
            self.start_count += 1;
            true
        }
    }
}

// ===========================================================================
// F429 — 服务健康心跳：错过判定
// ===========================================================================

pub const HEARTBEAT_INTERVAL_MS: u32 = 1000;
pub const HEARTBEAT_MISSES_MAX: u32 = 3;

/// 距上次心跳超过 interval*(max+1) 视为失联（整数乘法防溢出用 u64）。
pub fn heartbeat_lost(ms_since_last: u32) -> bool {
    let threshold = HEARTBEAT_INTERVAL_MS as u64 * (HEARTBEAT_MISSES_MAX as u64 + 1);
    ms_since_last as u64 > threshold
}

// ===========================================================================
// F430 — 进程家谱：父子深度与祖先链
// ===========================================================================

pub const PROC_MAX: usize = 32;

/// parent[i] = 父进程下标；u16::MAX 表示根。祖先链长度（不含自身）。
pub fn ancestry_depth(parent: &[u16; PROC_MAX], pid: usize) -> usize {
    let mut depth = 0usize;
    let mut cur = pid;
    while cur < PROC_MAX && parent[cur] != u16::MAX && depth <= PROC_MAX {
        cur = parent[cur] as usize;
        depth += 1;
    }
    depth
}

// ===========================================================================
// F431 — 资源配额官：CPU/内存双配额
// ===========================================================================

#[derive(Clone, Copy)]
pub struct Quota {
    pub cpu_permille: u16,  // 0~1000
    pub mem_kib: u32,
}

pub const QUOTA_CPU_MAX: u16 = 300; // 单服务最多 30% CPU
pub const QUOTA_MEM_MAX_KIB: u32 = 32 * 1024;

pub fn quota_within(q: Quota) -> bool {
    q.cpu_permille <= QUOTA_CPU_MAX && q.mem_kib <= QUOTA_MEM_MAX_KIB
}

// ===========================================================================
// F432 — 僵尸进程清道夫：可收割判定
// ===========================================================================

#[derive(Clone, Copy)]
pub struct ProcState {
    pub exited: bool,
    pub reaped: bool,
    pub parent_alive: bool,
}

/// 僵尸：已退出、未收割、父进程还活着（父死后由 init 收割，不算僵尸）。
pub fn is_zombie(p: ProcState) -> bool {
    p.exited && !p.reaped && p.parent_alive
}

// ===========================================================================
// F433 — 优雅退出公约：退出分阶段时限
// ===========================================================================

pub const EXIT_SAVE_STATE_MS: u32 = 1500;
pub const EXIT_DRAIN_IO_MS: u32 = 2000;
pub const EXIT_HARD_MS: u32 = 5000;

/// 退出预算自洽：总硬限 ≥ 各阶段之和。
pub fn exit_budget_sane() -> bool {
    EXIT_HARD_MS >= EXIT_SAVE_STATE_MS + EXIT_DRAIN_IO_MS
}

// ===========================================================================
// F434 — 信号礼仪：信号处理次序
// ===========================================================================

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Signal {
    Term,
    Int,
    Hup,
    Kill,
}

/// Kill 不可拦截；其余可注册处理器。
pub fn signal_maskable(s: Signal) -> bool {
    !matches!(s, Signal::Kill)
}

/// 处理次序：先停收新请求，再排空，最后退出。
pub const SIGNAL_HANDLING_ORDER: [&str; 3] = ["stop-accept", "drain", "exit"];

// ===========================================================================
// F435 — 会话管理器：会话令牌有效期
// ===========================================================================

pub const SESSION_TTL_MS: u32 = 30 * 60 * 1000;

#[derive(Clone, Copy)]
pub struct Session {
    pub token: u32,
    pub age_ms: u32,
}

pub fn session_valid(s: Session) -> bool {
    s.token != 0 && s.age_ms <= SESSION_TTL_MS
}

// ===========================================================================
// F436 — 用户多实例隔离：每用户独立实例
// ===========================================================================

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct InstanceKey {
    pub user_id: u32,
    pub app_id: u32,
}

/// 多实例约束：同一 (user, app) 只能一个实例；不同用户互不干扰。
pub fn instance_unique(keys: &[InstanceKey]) -> bool {
    for i in 0..keys.len() {
        for j in (i + 1)..keys.len() {
            if keys[i] == keys[j] {
                return false;
            }
        }
    }
    true
}

// ===========================================================================
// F437 — 权限分离执行体：特权面最小化
// ===========================================================================

#[derive(Clone, Copy)]
pub struct ExecBody {
    pub privileged: bool,
    pub caps: u32,
}

pub const CAP_ALL: u32 = 0xFFFF;

/// 特权执行体持全能力；非特权执行体能力必须为空。
pub fn exec_body_sane(e: ExecBody) -> bool {
    if e.privileged {
        e.caps == CAP_ALL
    } else {
        e.caps == 0
    }
}

// ===========================================================================
// F438 — 最小特权审计：越权项清点
// ===========================================================================

#[derive(Clone, Copy)]
pub struct PrivAuditItem {
    pub service_id: u32,
    pub needed_caps: u32,
    pub granted_caps: u32,
}

/// 越权：授予超出所需。
pub fn audit_overgrant(item: PrivAuditItem) -> bool {
    item.granted_caps & !item.needed_caps != 0
}

/// 审计通过：清单里无越权项。
pub fn audit_clean(items: &[PrivAuditItem]) -> bool {
    items.iter().all(|i| !audit_overgrant(*i))
}

// ===========================================================================
// F439 — 服务降级阶梯：档位递降
// ===========================================================================

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DegradationTier {
    Full,
    Cached,
    ReadOnly,
    Disabled,
}

/// 负载 permille → 档位：<700 全量，<850 缓存态，<950 只读，否则停用。
pub fn degradation_tier(load_permille: u16) -> DegradationTier {
    if load_permille < 700 {
        DegradationTier::Full
    } else if load_permille < 850 {
        DegradationTier::Cached
    } else if load_permille < 950 {
        DegradationTier::ReadOnly
    } else {
        DegradationTier::Disabled
    }
}

// ===========================================================================
// F440 — 服务热更新：版本代际与回滚
// ===========================================================================

#[derive(Clone, Copy)]
pub struct HotUpdate {
    pub gen: u32, // 代际，单调递增
    pub alive: bool,
}

impl HotUpdate {
    /// 新代际可替换旧代际：代际必须严格递增。
    pub fn can_replace(&self, new_gen: u32) -> bool {
        self.alive && new_gen > self.gen
    }
}

// ===========================================================================
// F441 — 系统调用审计：调用面登记
// ===========================================================================

pub const SYSCALL_AUDIT_SLOTS: usize = 32;

#[derive(Clone, Copy)]
pub struct SyscallAudit {
    counts: [u32; SYSCALL_AUDIT_SLOTS],
}

impl SyscallAudit {
    pub const fn new() -> SyscallAudit {
        SyscallAudit { counts: [0; SYSCALL_AUDIT_SLOTS] }
    }
    pub fn record(&mut self, nr: usize) {
        if nr < SYSCALL_AUDIT_SLOTS {
            self.counts[nr] += 1;
        }
    }
    pub fn count(&self, nr: usize) -> u32 {
        if nr < SYSCALL_AUDIT_SLOTS {
            self.counts[nr]
        } else {
            0
        }
    }
}

// ===========================================================================
// F442 — 句柄泄漏纠察：分配/释放记账
// ===========================================================================

#[derive(Clone, Copy)]
pub struct HandleLedger {
    pub opened: u32,
    pub closed: u32,
}

impl HandleLedger {
    pub fn open(&mut self) {
        self.opened += 1;
    }
    pub fn close(&mut self) -> bool {
        if self.closed < self.opened {
            self.closed += 1;
            true
        } else {
            false
        }
    }
    pub fn leaked(&self) -> u32 {
        self.opened - self.closed
    }
}

/// 进程退出时句柄必须归零，否则判泄漏。
pub fn handle_exit_clean(l: HandleLedger) -> bool {
    l.leaked() == 0
}

// ===========================================================================
// F443 — 进程优先级策展：优先级区间
// ===========================================================================

#[derive(Clone, Copy)]
pub struct Priority {
    pub nice: i8, // -20~19，越小越优先
}

pub fn priority_sane(p: Priority) -> bool {
    p.nice >= -20 && p.nice <= 19
}

/// 前台应用可提升（nice 减小），后台只能降或保持。
pub fn priority_adjust_ok(cur: i8, new: i8, foreground: bool) -> bool {
    if !priority_sane(Priority { nice: new }) {
        return false;
    }
    if foreground {
        new <= cur
    } else {
        new >= cur
    }
}

// ===========================================================================
// F444 — 崩溃隔离舱：崩溃不传染
// ===========================================================================

#[derive(Clone, Copy)]
pub struct CrashCell {
    pub pid: u32,
    pub crashed: bool,
    pub contained: bool,
}

/// 隔离合格：崩溃进程必须被围困（不拖垮邻居）。
pub fn crash_contained(c: CrashCell) -> bool {
    !c.crashed || c.contained
}

// ===========================================================================
// F445 — 服务时间轴：事件时序记录
// ===========================================================================

#[derive(Clone, Copy)]
pub struct TimelineEvent {
    pub at_ms: u32,
    pub kind: u8, // 0=start 1=ready 2=stop 3=crash
}

/// 时间轴单调：事件按时间非递减排列。
pub fn timeline_monotonic(events: &[TimelineEvent]) -> bool {
    for w in events.windows(2) {
        if w[1].at_ms < w[0].at_ms {
            return false;
        }
    }
    true
}

// ===========================================================================
// F446 — 依赖死锁拆解：等待环检测（Kahn）
// ===========================================================================

/// wait_graph[i] 的第 j 位表示 i 在等 j（bitmask，服务数 ≤ 16）。
/// 存在等待环即死锁。
pub fn deadlock_exists(wait_graph: &[u16]) -> bool {
    let n = wait_graph.len();
    if n > 16 {
        return true; // 超容量按死锁防御处理
    }
    let mut resolved = [false; 16];
    let mut left = n;
    loop {
        let mut progress = false;
        for i in 0..n {
            if !resolved[i] && wait_graph[i] & !resolved_mask(&resolved) == 0 {
                resolved[i] = true;
                left -= 1;
                progress = true;
            }
        }
        if !progress {
            break;
        }
    }
    left > 0
}

fn resolved_mask(resolved: &[bool; 16]) -> u16 {
    let mut m = 0u16;
    for i in 0..16 {
        if resolved[i] {
            m |= 1 << i;
        }
    }
    m
}

// ===========================================================================
// F447 — 服务文档化：文档字段齐备
// ===========================================================================

#[derive(Clone, Copy)]
pub struct SvcDoc {
    pub name: &'static str,
    pub purpose: &'static str,
    pub deps: &'static str,
}

pub fn svc_doc_complete(d: &SvcDoc) -> bool {
    !d.name.is_empty() && !d.purpose.is_empty() && !d.deps.is_empty()
}

// ===========================================================================
// F448 — 进程回放：启动序列确定性
// ===========================================================================

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct BootStep {
    pub service_id: u32,
    pub phase: u8,
}

/// 回放一致：两段启动序列逐步相同。
pub fn boot_replay_matches(a: &[BootStep], b: &[BootStep]) -> bool {
    a.len() == b.len() && a.iter().zip(b.iter()).all(|(x, y)| x == y)
}

// ===========================================================================
// F449 — 服务回归走廊：回归用例门槛
// ===========================================================================

pub const REGRESSION_MIN_CASES: usize = 12;

#[derive(Clone, Copy)]
pub struct RegressionCase {
    pub name: &'static str,
    pub passed: bool,
}

/// 回归放行：用例数达门槛且全过。
pub fn regression_pass(cases: &[RegressionCase]) -> bool {
    cases.len() >= REGRESSION_MIN_CASES && cases.iter().all(|c| c.passed)
}

// ===========================================================================
// F450 — 服务年报：运行统计板块
// ===========================================================================

pub const SVC_REPORT_SECTIONS: [&str; 4] = ["restarts", "crash-cells", "dep-depth", "uptime"];

/// 年报就绪：板块齐 + 重启次数 ≤ 10。
pub fn svc_report_ready(sections: &[&str], restarts: u16) -> bool {
    SVC_REPORT_SECTIONS.iter().all(|s| sections.contains(s)) && restarts <= 10
}

// ===========================================================================
// 域自检
// ===========================================================================

pub fn run_m600svc_checks() -> CheckSet {
    let mut set = CheckSet::new("m600svc");

    // F426 服务总线
    let mut reg = ServiceRegistry::new();
    let r1 = reg.register(0xA1);
    let dup = reg.register(0xA1);
    let r2 = reg.register(0xB2);
    set.add(
        "F426 registry",
        r1 && !dup && r2 && reg.lookup(0xB2) && !reg.lookup(0xC3) && reg.len() == 2,
        "dedupe+lookup",
    );

    // F427 依赖图
    set.add(
        "F427 dep edges",
        dep_edge_ok(0, 1, 4) && !dep_edge_ok(2, 2, 4) && !dep_edge_ok(0, 9, 4),
        "no self/oob",
    );

    // F428 惰性启动
    let mut lazy = LazyService::new();
    let first = lazy.ensure_started();
    let second = lazy.ensure_started();
    set.add(
        "F428 lazy start",
        first && !second && lazy.started && lazy.start_count == 1,
        "start once",
    );

    // F429 心跳
    set.add(
        "F429 heartbeat",
        !heartbeat_lost(HEARTBEAT_INTERVAL_MS * HEARTBEAT_MISSES_MAX)
            && heartbeat_lost(HEARTBEAT_INTERVAL_MS * (HEARTBEAT_MISSES_MAX + 1) + 1),
        "3 misses",
    );

    // F430 进程家谱
    let mut parent = [u16::MAX; PROC_MAX];
    parent[1] = 0;
    parent[2] = 1;
    parent[3] = 2;
    let depth_mid = ancestry_depth(&parent, 3);
    set.add(
        "F430 ancestry",
        depth_mid == 3 && ancestry_depth(&parent, 0) == 0,
        "chain depth",
    );

    // F431 资源配额
    set.add(
        "F431 quota",
        quota_within(Quota { cpu_permille: 300, mem_kib: QUOTA_MEM_MAX_KIB })
            && !quota_within(Quota { cpu_permille: 301, mem_kib: 1 })
            && !quota_within(Quota { cpu_permille: 1, mem_kib: QUOTA_MEM_MAX_KIB + 1 }),
        "cpu+mem caps",
    );

    // F432 僵尸清道夫
    set.add(
        "F432 zombie",
        is_zombie(ProcState { exited: true, reaped: false, parent_alive: true })
            && !is_zombie(ProcState { exited: true, reaped: true, parent_alive: true })
            && !is_zombie(ProcState { exited: true, reaped: false, parent_alive: false }),
        "exit+unreaped+parent",
    );

    // F433 优雅退出
    set.add(
        "F433 exit budget",
        exit_budget_sane() && EXIT_HARD_MS == 5000,
        "hard ≥ phases",
    );

    // F434 信号礼仪
    set.add(
        "F434 signals",
        signal_maskable(Signal::Term) && signal_maskable(Signal::Hup) && !signal_maskable(Signal::Kill),
        "kill unmaskable",
    );
    set.add(
        "F434 signal order",
        SIGNAL_HANDLING_ORDER.len() == 3
            && SIGNAL_HANDLING_ORDER[0] == "stop-accept"
            && SIGNAL_HANDLING_ORDER[2] == "exit",
        "3 phases",
    );

    // F435 会话管理器
    set.add(
        "F435 session",
        session_valid(Session { token: 7, age_ms: SESSION_TTL_MS })
            && !session_valid(Session { token: 7, age_ms: SESSION_TTL_MS + 1 })
            && !session_valid(Session { token: 0, age_ms: 0 }),
        "ttl+token",
    );

    // F436 多实例隔离
    let keys = [
        InstanceKey { user_id: 1, app_id: 9 },
        InstanceKey { user_id: 2, app_id: 9 },
        InstanceKey { user_id: 1, app_id: 10 },
    ];
    let dup_keys = [
        InstanceKey { user_id: 1, app_id: 9 },
        InstanceKey { user_id: 1, app_id: 9 },
    ];
    set.add(
        "F436 instances",
        instance_unique(&keys) && !instance_unique(&dup_keys),
        "(user,app) unique",
    );

    // F437 权限分离执行体
    set.add(
        "F437 exec bodies",
        exec_body_sane(ExecBody { privileged: true, caps: CAP_ALL })
            && exec_body_sane(ExecBody { privileged: false, caps: 0 })
            && !exec_body_sane(ExecBody { privileged: false, caps: 1 })
            && !exec_body_sane(ExecBody { privileged: true, caps: 1 }),
        "all-or-nothing",
    );

    // F438 最小特权审计
    let items = [
        PrivAuditItem { service_id: 1, needed_caps: 0b0011, granted_caps: 0b0011 },
        PrivAuditItem { service_id: 2, needed_caps: 0b0100, granted_caps: 0b0100 },
    ];
    let dirty = [
        PrivAuditItem { service_id: 3, needed_caps: 0b0001, granted_caps: 0b1001 },
    ];
    set.add(
        "F438 least privilege",
        audit_clean(&items) && !audit_clean(&dirty) && audit_overgrant(dirty[0]),
        "overgrant detect",
    );

    // F439 降级阶梯
    set.add(
        "F439 degradation",
        degradation_tier(699) == DegradationTier::Full
            && degradation_tier(700) == DegradationTier::Cached
            && degradation_tier(849) == DegradationTier::Cached
            && degradation_tier(949) == DegradationTier::ReadOnly
            && degradation_tier(950) == DegradationTier::Disabled,
        "tier ladder",
    );

    // F440 热更新
    let cur = HotUpdate { gen: 3, alive: true };
    set.add(
        "F440 hot update",
        cur.can_replace(4) && !cur.can_replace(3) && !HotUpdate { gen: 3, alive: false }.can_replace(9),
        "gen monotonic",
    );

    // F441 系统调用审计
    let mut audit = SyscallAudit::new();
    audit.record(5);
    audit.record(5);
    audit.record(9);
    audit.record(99); // 越界忽略
    set.add(
        "F441 syscall audit",
        audit.count(5) == 2 && audit.count(9) == 1 && audit.count(99) == 0,
        "counted",
    );

    // F442 句柄泄漏
    let mut l = HandleLedger { opened: 0, closed: 0 };
    l.open();
    l.open();
    l.open();
    let c1 = l.close();
    let leaked_mid = l.leaked();
    let c2 = l.close();
    let c3 = l.close();
    let over = l.close();
    set.add(
        "F442 handle ledger",
        c1 && c2 && c3 && !over && leaked_mid == 2 && handle_exit_clean(l),
        "no leak",
    );
    set.add(
        "F442 leak detect",
        !handle_exit_clean(HandleLedger { opened: 4, closed: 2 }),
        "2 leaked",
    );

    // F443 优先级策展
    set.add(
        "F443 priority",
        priority_sane(Priority { nice: -20 })
            && priority_sane(Priority { nice: 19 })
            && !priority_sane(Priority { nice: 20 })
            && priority_adjust_ok(0, -5, true)
            && !priority_adjust_ok(0, -5, false)
            && priority_adjust_ok(0, 5, false),
        "nice bounds+policy",
    );

    // F444 崩溃隔离舱
    set.add(
        "F444 crash cell",
        crash_contained(CrashCell { pid: 1, crashed: true, contained: true })
            && !crash_contained(CrashCell { pid: 2, crashed: true, contained: false })
            && crash_contained(CrashCell { pid: 3, crashed: false, contained: false }),
        "contained only",
    );

    // F445 服务时间轴
    let tl = [
        TimelineEvent { at_ms: 0, kind: 0 },
        TimelineEvent { at_ms: 50, kind: 1 },
        TimelineEvent { at_ms: 50, kind: 2 },
        TimelineEvent { at_ms: 90, kind: 3 },
    ];
    let bad_tl = [
        TimelineEvent { at_ms: 10, kind: 0 },
        TimelineEvent { at_ms: 5, kind: 1 },
    ];
    set.add(
        "F445 timeline",
        timeline_monotonic(&tl) && !timeline_monotonic(&bad_tl),
        "monotonic",
    );

    // F446 死锁拆解
    let no_deadlock = [0b0000, 0b0001, 0b0000]; // 1 等 0，0 空闲 → 可解
    let deadlocked = [0b0010, 0b0001, 0b0000]; // 0↔1 互等
    set.add(
        "F446 deadlock",
        !deadlock_exists(&no_deadlock) && deadlock_exists(&deadlocked),
        "wait-cycle",
    );

    // F447 服务文档化
    set.add(
        "F447 svc docs",
        svc_doc_complete(&SvcDoc { name: "netd", purpose: "网络守护", deps: "clockd" })
            && !svc_doc_complete(&SvcDoc { name: "netd", purpose: "", deps: "" }),
        "fields complete",
    );

    // F448 进程回放
    let seq_a = [BootStep { service_id: 1, phase: 0 }, BootStep { service_id: 2, phase: 1 }];
    set.add(
        "F448 boot replay",
        boot_replay_matches(&seq_a, &seq_a)
            && !boot_replay_matches(&seq_a, &[BootStep { service_id: 1, phase: 0 }]),
        "deterministic",
    );

    // F449 回归走廊
    let mut cases: [RegressionCase; REGRESSION_MIN_CASES] =
        [RegressionCase { name: "c", passed: true }; REGRESSION_MIN_CASES];
    set.add(
        "F449 regression pass",
        regression_pass(&cases),
        "12 cases all pass",
    );
    cases[0].passed = false;
    set.add(
        "F449 regression fail",
        !regression_pass(&cases) && !regression_pass(&cases[..REGRESSION_MIN_CASES - 1]),
        "any-fail blocks",
    );

    // F450 服务年报
    set.add(
        "F450 svc report",
        svc_report_ready(&SVC_REPORT_SECTIONS, 8)
            && !svc_report_ready(&SVC_REPORT_SECTIONS, 11)
            && !svc_report_ready(&["restarts"], 1),
        "sections+restarts",
    );

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f426_registry_capacity() {
        let mut reg = ServiceRegistry::new();
        for i in 0..SVC_MAX {
            assert!(reg.register(i as u32 + 1));
        }
        assert!(!reg.register(99));
        assert_eq!(reg.len(), SVC_MAX);
    }

    #[test]
    fn f429_heartbeat_exact_boundary() {
        let threshold = HEARTBEAT_INTERVAL_MS * (HEARTBEAT_MISSES_MAX + 1);
        assert!(!heartbeat_lost(threshold));
        assert!(heartbeat_lost(threshold + 1));
    }

    #[test]
    fn f430_ancestry_self_root() {
        let parent = [u16::MAX; PROC_MAX];
        assert_eq!(ancestry_depth(&parent, 5), 0);
    }

    #[test]
    fn f446_self_wait_is_deadlock() {
        let g = [0b0001, 0b0000];
        assert!(deadlock_exists(&g)); // 0 等自己
    }

    #[test]
    fn f448_replay_length_mismatch() {
        let a = [BootStep { service_id: 1, phase: 0 }];
        let b: [BootStep; 0] = [];
        assert!(!boot_replay_matches(&a, &b));
        let empty: [BootStep; 0] = [];
        assert!(boot_replay_matches(&empty, &empty));
    }

    #[test]
    fn f450_domain_selfcheck_all_pass() {
        let set = run_m600svc_checks();
        assert!(set.len() >= 25, "got {}", set.len());
        assert!(set.all_passed());
    }
}
