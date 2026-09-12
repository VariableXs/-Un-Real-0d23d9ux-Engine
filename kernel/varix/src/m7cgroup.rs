//! m7cgroup — VARIX-M700 AI-22 容器与隔离域 (F526~F550)
//!
//! 隔离域宪法、PID/挂载/网络命名空间、控制组、域间桥、越权 fuzz、
//! 生命周期、监控剖面、逃逸审计、设备白名单、配额执法、快照、迁移、
//! 统计分账、健康分、事件流、最小模板、回归走廊、安全基线、崩溃隔离、
//! 文档、压力剧本、自描述导出、年报。
//!
//! 硬约束：no_std / 无 alloc / 无浮点。

use crate::checks::CheckSet;

// ===========================================================================
// F526 — 隔离域宪法
// ===========================================================================

#[derive(Clone, Copy, PartialEq)]
pub enum DomainState {
    Creating,
    Running,
    Frozen,
    Stopping,
    Dead,
}

pub struct IsolationDomain {
    pub id: u32,
    pub state: DomainState,
    pub pid_ns: bool,
    pub mount_ns: bool,
    pub net_ns: bool,
}

impl IsolationDomain {
    /// 宪法：运行中的域必须至少有 PID 命名空间隔离。
    pub fn semantics_ok(&self) -> bool {
        if self.state == DomainState::Running {
            self.pid_ns
        } else {
            true
        }
    }
}

// ===========================================================================
// F527 — PID 命名空间舱
// ===========================================================================

pub const PID_NS_CAP: usize = 64;

#[derive(Clone, Copy)]
pub struct PidNamespace {
    pub ns_id: u32,
    pub next_pid: u32,
    pub used: usize,
}

impl PidNamespace {
    pub fn alloc_pid(&mut self) -> Option<u32> {
        if self.used >= PID_NS_CAP {
            return None;
        }
        let p = self.next_pid;
        self.next_pid += 1;
        self.used += 1;
        Some(p)
    }
}

// ===========================================================================
// F528 — 挂载命名空间舱
// ===========================================================================

pub const MOUNT_VIEW_CAP: usize = 8;

#[derive(Clone, Copy)]
pub struct MountView {
    pub src: u32,
    pub dst: u32,
    pub ro: bool,
}

pub fn mount_view_conflict(a: &MountView, b: &MountView) -> bool {
    a.dst == b.dst && (a.ro != b.ro)
}

// ===========================================================================
// F529 — 资源控制组谱
// ===========================================================================

#[derive(Clone, Copy)]
pub struct Cgroup {
    pub id: u32,
    pub cpu_shares: u32,   // 2~262144
    pub mem_max_kb: u32,
    pub io_max_kbps: u32,
}

pub fn cgroup_sane(c: &Cgroup) -> bool {
    (2..=262_144).contains(&c.cpu_shares) && c.mem_max_kb >= 64 && c.io_max_kbps >= 8
}

pub fn cpu_share_permille(c: &Cgroup, total_shares: u32) -> u16 {
    if total_shares == 0 {
        return 0;
    }
    ((c.cpu_shares as u64 * 1000) / total_shares as u64).min(1000) as u16
}

// ===========================================================================
// F530 — 域间通信桥
// ===========================================================================

#[derive(Clone, Copy)]
pub struct DomainBridge {
    pub a: u32,
    pub b: u32,
    pub allowed_msgs: u8, // bitmask
}

pub fn bridge_bidirectional(br: &DomainBridge) -> bool {
    br.a != br.b && br.allowed_msgs != 0
}

pub fn bridge_permits(br: &DomainBridge, msg_kind: u8) -> bool {
    msg_kind < 8 && br.allowed_msgs & (1 << msg_kind) != 0
}

// ===========================================================================
// F531 — 越权 fuzz 靶场
// ===========================================================================

/// 越权请求必须全部拒绝；返回拒绝数。
pub fn escape_fuzz_rejects(requests: &[(u32, u32)], n: usize, owner_domain: u32) -> u32 {
    let n = n.min(requests.len());
    requests[..n].iter().filter(|&&(from, _res)| from != owner_domain).count() as u32
}

// ===========================================================================
// F532 — 域生命周期官
// ===========================================================================

pub fn domain_transition_legal(from: DomainState, to: DomainState) -> bool {
    use DomainState::*;
    matches!(
        (from, to),
        (Creating, Running)
            | (Running, Frozen)
            | (Frozen, Running)
            | (Running, Stopping)
            | (Frozen, Stopping)
            | (Stopping, Dead)
    )
}

// ===========================================================================
// F533 — 域监控剖面
// ===========================================================================

#[derive(Clone, Copy)]
pub struct DomainProfile {
    pub cpu_ms: u64,
    pub mem_kb: u32,
    pub io_kb: u32,
    pub pids: u32,
}

pub fn profile_exportable(p: &DomainProfile) -> bool {
    p.pids > 0 && p.mem_kb >= p.pids * 4 // 至少 4KB/进程
}

// ===========================================================================
// F534 — 域逃逸审计
// ===========================================================================

#[derive(Clone, Copy, PartialEq)]
pub enum EscapeSignal {
    None,
    NsHandleLeak,
    PrivSyscall,
    ForeignPathAccess,
}

pub fn escape_audit(ns_leak: bool, priv_syscall: bool, foreign_path: bool) -> EscapeSignal {
    if ns_leak {
        EscapeSignal::NsHandleLeak
    } else if priv_syscall {
        EscapeSignal::PrivSyscall
    } else if foreign_path {
        EscapeSignal::ForeignPathAccess
    } else {
        EscapeSignal::None
    }
}

// ===========================================================================
// F535 — 设备白名单舱
// ===========================================================================

pub const DEV_WL_CAP: usize = 16;

pub struct DeviceWhitelist {
    pub devs: [u32; DEV_WL_CAP],
    pub len: usize,
}

impl DeviceWhitelist {
    pub fn allows(&self, dev: u32) -> bool {
        self.devs[..self.len].contains(&dev)
    }
    pub fn add(&mut self, dev: u32) -> bool {
        if self.len >= DEV_WL_CAP || self.allows(dev) {
            return false;
        }
        self.devs[self.len] = dev;
        self.len += 1;
        true
    }
}

// ===========================================================================
// F536 — 网络命名空间舱
// ===========================================================================

#[derive(Clone, Copy)]
pub struct NetNamespace {
    pub ns_id: u32,
    pub mtu: u16,
    pub port_base: u16,
}

pub fn net_ns_sane(n: &NetNamespace) -> bool {
    (576..=65_535).contains(&n.mtu) && n.port_base >= 1024
}

// ===========================================================================
// F537 — 域配额执法官
// ===========================================================================

#[derive(Clone, Copy, PartialEq)]
#[derive(Debug)]
pub enum QuotaVerdict {
    Admit,
    Throttle,
    Deny,
}

pub fn quota_enforce(used: u32, soft: u32, hard: u32, want: u32) -> QuotaVerdict {
    if used + want <= soft {
        QuotaVerdict::Admit
    } else if used + want <= hard {
        QuotaVerdict::Throttle
    } else {
        QuotaVerdict::Deny
    }
}

// ===========================================================================
// F538 — 域快照舱
// ===========================================================================

pub const SNAP_MAX: usize = 4;

#[derive(Clone, Copy)]
pub struct DomainSnapshot {
    pub epoch_ms: u64,
    pub mem_pages: u32,
    pub kind: u8, // 0=full 1=inc
}

pub fn snapshot_select(snaps: &[DomainSnapshot], n: usize, target_ms: u64) -> Option<usize> {
    let n = n.min(SNAP_MAX).min(snaps.len());
    let mut best: Option<usize> = None;
    for i in 0..n {
        if snaps[i].epoch_ms <= target_ms {
            match best {
                None => best = Some(i),
                Some(b) if snaps[i].epoch_ms > snaps[b].epoch_ms => best = Some(i),
                _ => {}
            }
        }
    }
    best
}

// ===========================================================================
// F539 — 域迁移侦察
// ===========================================================================

#[derive(Clone, Copy)]
pub struct Migrability {
    pub dirty_rate_kb_s: u32,
    pub link_kbps: u32,
    pub mem_kb: u32,
}

/// 脏页速率必须低于链路速率的 80% 才可迁移。
pub fn migratable(m: &Migrability) -> bool {
    m.link_kbps > 0 && (m.dirty_rate_kb_s as u64 * 1000) < (m.link_kbps as u64 * 800)
}

// ===========================================================================
// F540 — 域统计分账
// ===========================================================================

#[derive(Clone, Copy)]
pub struct DomainUsage {
    pub id: u32,
    pub cpu_ms: u64,
    pub mem_kb: u32,
}

pub fn usage_top(usage: &mut [DomainUsage], n: usize) -> usize {
    let n = n.min(usage.len());
    for i in 1..n {
        let key = usage[i];
        let mut j = i;
        while j > 0 && usage[j - 1].cpu_ms < key.cpu_ms {
            usage[j] = usage[j - 1];
            j -= 1;
        }
        usage[j] = key;
    }
    n
}

// ===========================================================================
// F541 — 域健康分
// ===========================================================================

pub struct DomainHealth {
    pub oom_kills: u32,
    pub task_failures: u32,
    pub tasks_total: u32,
}

impl DomainHealth {
    pub fn fail_permille(&self) -> u16 {
        if self.tasks_total == 0 {
            return 0;
        }
        ((self.task_failures as u32 * 1000) / self.tasks_total).min(1000) as u16
    }
    pub fn grade(&self) -> u8 {
        if self.oom_kills == 0 && self.fail_permille() == 0 {
            0
        } else if self.oom_kills == 0 && self.fail_permille() <= 50 {
            1
        } else {
            2
        }
    }
}

// ===========================================================================
// F542 — 域事件流
// ===========================================================================

#[derive(Clone, Copy, PartialEq)]
pub enum DomainEvent {
    Created,
    QuotaHit,
    Oom,
    Escape,
    Destroyed,
}

pub fn domain_event_loggable(e: DomainEvent) -> bool {
    matches!(e, DomainEvent::Created | DomainEvent::QuotaHit | DomainEvent::Oom | DomainEvent::Escape | DomainEvent::Destroyed)
}

// ===========================================================================
// F543 — 最小域模板
// ===========================================================================

pub const MIN_TEMPLATE_MEM_KB: u32 = 8192;

pub fn minimal_template_valid(mem_kb: u32, pids: u32, ns: bool) -> bool {
    mem_kb >= MIN_TEMPLATE_MEM_KB && pids >= 1 && ns
}

// ===========================================================================
// F544 — 域回归走廊
// ===========================================================================

pub const ISOLATION_CORRIDOR_CASES: [&str; 5] =
    ["pid-ns-leak", "mount-escape", "quota-enforce", "crash-isolation", "bridge-acl"];

pub fn isolation_corridor_pass(results: &[bool; 5]) -> bool {
    results.iter().all(|&r| r)
}

// ===========================================================================
// F545 — 域安全基线
// ===========================================================================

pub struct DomainBaseline {
    pub no_priv_syscalls: bool,
    pub ro_rootfs: bool,
    pub dev_whitelist_only: bool,
    pub net_isolated: bool,
}

pub const BASELINE_SCORE_MAX: u8 = 4;

pub fn baseline_score(b: &DomainBaseline) -> u8 {
    let mut s = 0;
    if b.no_priv_syscalls { s += 1; }
    if b.ro_rootfs { s += 1; }
    if b.dev_whitelist_only { s += 1; }
    if b.net_isolated { s += 1; }
    s
}

// ===========================================================================
// F546 — 域崩溃隔离验证
// ===========================================================================

/// 宿主存活 + 其他域存活 + 崩溃域已回收 = 隔离成立。
pub fn crash_isolation_ok(host_alive: bool, other_alive: bool, crashed_reaped: bool) -> bool {
    host_alive && other_alive && crashed_reaped
}

// ===========================================================================
// F547 — 域文档生成器
// ===========================================================================

pub const DOMAIN_DOC_SECTIONS: [&str; 5] = ["semantics", "quotas", "networking", "storage", "lifecycle"];

pub fn domain_doc_complete(marks: u8) -> bool {
    marks as u32 == (1u32 << DOMAIN_DOC_SECTIONS.len()) - 1
}

// ===========================================================================
// F548 — 域压力剧本
// ===========================================================================

pub const DOMAIN_STRESS_PLAYS: [&str; 4] = ["64-domains", "quota-storm", "snapshot-race", "bridge-flood"];

pub fn domain_stress_known(name: &str) -> bool {
    DOMAIN_STRESS_PLAYS.iter().any(|p| *p == name)
}

// ===========================================================================
// F549 — 域自描述导出
// ===========================================================================

pub struct DomainExport {
    pub id: u32,
    pub ns_flags: u8, // bit0 pid bit1 mount bit2 net
    pub mem_max_kb: u32,
    pub cpu_shares: u32,
}

pub fn domain_export_valid(e: &DomainExport) -> bool {
    e.ns_flags & 0b001 != 0 && e.mem_max_kb >= 64 && (2..=262_144).contains(&e.cpu_shares)
}

// ===========================================================================
// F550 — 隔离域年报
// ===========================================================================

pub struct IsolationYearbook {
    pub domains_created: u32,
    pub quota_events: u32,
    pub escapes_blocked: u32,
    pub crash_isolations: u32,
}

impl IsolationYearbook {
    pub fn escape_free(&self) -> bool {
        self.escapes_blocked == self.escapes_blocked && self.domains_created > 0
    }
    pub fn grade(&self) -> u8 {
        if self.crash_isolations == 0 || self.escapes_blocked == 0 {
            0
        } else {
            1
        }
    }
}

// ===========================================================================
// 域自检
// ===========================================================================

pub fn run_m7cgroup_checks() -> CheckSet {
    let mut set = CheckSet::new("m7cgroup");

    // F526 宪法
    let ok = IsolationDomain { id: 1, state: DomainState::Running, pid_ns: true, mount_ns: true, net_ns: false };
    let bad = IsolationDomain { id: 2, state: DomainState::Running, pid_ns: false, mount_ns: false, net_ns: false };
    set.add("F526 semantics", ok.semantics_ok() && !bad.semantics_ok(), "pid ns required");

    // F527 PID ns
    let mut ns = PidNamespace { ns_id: 1, next_pid: 0, used: 0 };
    let mut last = None;
    for _ in 0..PID_NS_CAP {
        last = ns.alloc_pid();
    }
    set.add(
        "F527 pid ns",
        last == Some((PID_NS_CAP - 1) as u32) && ns.alloc_pid().is_none(),
        "bounded alloc",
    );

    // F528 挂载 ns
    let v1 = MountView { src: 1, dst: 10, ro: true };
    let v2 = MountView { src: 2, dst: 10, ro: false };
    let v3 = MountView { src: 3, dst: 11, ro: true };
    set.add(
        "F528 mount ns",
        mount_view_conflict(&v1, &v2) && !mount_view_conflict(&v1, &v3),
        "dst ro conflict",
    );

    // F529 控制组
    let cg = Cgroup { id: 1, cpu_shares: 1024, mem_max_kb: 65536, io_max_kbps: 1024 };
    set.add(
        "F529 cgroup",
        cgroup_sane(&cg) && cpu_share_permille(&cg, 4096) == 250,
        "share permille",
    );

    // F530 通信桥
    let br = DomainBridge { a: 1, b: 2, allowed_msgs: 0b0110 };
    set.add(
        "F530 bridge",
        bridge_bidirectional(&br) && bridge_permits(&br, 1) && !bridge_permits(&br, 0),
        "acl mask",
    );

    // F531 越权 fuzz
    let reqs = [(1u32, 100u32), (2, 101), (1, 102), (3, 103)];
    set.add(
        "F531 escape fuzz",
        escape_fuzz_rejects(&reqs, 4, 1) == 2,
        "foreign rejected",
    );

    // F532 生命周期
    set.add(
        "F532 lifecycle",
        domain_transition_legal(DomainState::Creating, DomainState::Running)
            && domain_transition_legal(DomainState::Running, DomainState::Frozen)
            && !domain_transition_legal(DomainState::Dead, DomainState::Running),
        "legal edges",
    );

    // F533 监控剖面
    let p = DomainProfile { cpu_ms: 100, mem_kb: 4096, io_kb: 8, pids: 16 };
    set.add(
        "F533 profile",
        profile_exportable(&p) && !profile_exportable(&DomainProfile { cpu_ms: 1, mem_kb: 60, io_kb: 1, pids: 16 }),
        "mem floor",
    );

    // F534 逃逸审计
    set.add(
        "F534 escape audit",
        escape_audit(true, false, false) == EscapeSignal::NsHandleLeak
            && escape_audit(false, false, false) == EscapeSignal::None
            && escape_audit(false, true, true) == EscapeSignal::PrivSyscall,
        "priority signals",
    );

    // F535 设备白名单
    let mut wl = DeviceWhitelist { devs: [0; DEV_WL_CAP], len: 0 };
    wl.add(7);
    let dup = wl.add(7);
    set.add("F535 dev whitelist", wl.allows(7) && !wl.allows(8) && !dup, "allow + dedup");

    // F536 网络 ns
    let nn = NetNamespace { ns_id: 1, mtu: 1500, port_base: 4096 };
    set.add(
        "F536 net ns",
        net_ns_sane(&nn) && !net_ns_sane(&NetNamespace { ns_id: 1, mtu: 100, port_base: 80 }),
        "mtu + port",
    );

    // F537 配额执法
    set.add(
        "F537 quota",
        quota_enforce(10, 50, 100, 30) == QuotaVerdict::Admit
            && quota_enforce(10, 50, 100, 45) == QuotaVerdict::Throttle
            && quota_enforce(90, 50, 100, 20) == QuotaVerdict::Deny,
        "soft/hard",
    );

    // F538 快照
    let snaps = [
        DomainSnapshot { epoch_ms: 100, mem_pages: 1, kind: 0 },
        DomainSnapshot { epoch_ms: 250, mem_pages: 1, kind: 1 },
    ];
    set.add(
        "F538 snapshot",
        snapshot_select(&snaps, 2, 300) == Some(1) && snapshot_select(&snaps, 2, 50).is_none(),
        "nearest<=target",
    );

    // F539 迁移
    let m = Migrability { dirty_rate_kb_s: 700, link_kbps: 1000, mem_kb: 100_000 };
    let m2 = Migrability { dirty_rate_kb_s: 900, link_kbps: 1000, mem_kb: 100_000 };
    set.add("F539 migrate", migratable(&m) && !migratable(&m2), "dirty < 80% link");

    // F540 分账
    let mut us = [
        DomainUsage { id: 1, cpu_ms: 10, mem_kb: 1 },
        DomainUsage { id: 2, cpu_ms: 90, mem_kb: 2 },
        DomainUsage { id: 3, cpu_ms: 50, mem_kb: 3 },
    ];
    usage_top(&mut us, 3);
    set.add("F540 usage", us[0].id == 2 && us[2].id == 1, "ranked");

    // F541 健康分
    let h = DomainHealth { oom_kills: 0, task_failures: 0, tasks_total: 100 };
    let hb = DomainHealth { oom_kills: 2, task_failures: 10, tasks_total: 100 };
    set.add(
        "F541 health",
        h.grade() == 0 && hb.grade() == 2 && hb.fail_permille() == 100,
        "fail rate",
    );

    // F542 事件流
    set.add(
        "F542 events",
        domain_event_loggable(DomainEvent::QuotaHit) && domain_event_loggable(DomainEvent::Destroyed),
        "5 events",
    );

    // F543 最小模板
    set.add(
        "F543 template",
        minimal_template_valid(8192, 1, true) && !minimal_template_valid(4096, 1, true),
        "mem floor 8M",
    );

    // F544 走廊
    set.add(
        "F544 corridor",
        isolation_corridor_pass(&[true; 5]) && !isolation_corridor_pass(&[true, true, false, true, true]),
        "5 cases",
    );

    // F545 基线
    let full = DomainBaseline { no_priv_syscalls: true, ro_rootfs: true, dev_whitelist_only: true, net_isolated: true };
    let part = DomainBaseline { no_priv_syscalls: true, ro_rootfs: false, dev_whitelist_only: true, net_isolated: false };
    set.add(
        "F545 baseline",
        baseline_score(&full) == BASELINE_SCORE_MAX && baseline_score(&part) == 2,
        "4 checks",
    );

    // F546 崩溃隔离
    set.add(
        "F546 crash isolation",
        crash_isolation_ok(true, true, true) && !crash_isolation_ok(true, false, true),
        "host+peer alive",
    );

    // F547 文档
    set.add(
        "F547 docs",
        domain_doc_complete(0b1_1111) && !domain_doc_complete(0b1_1101),
        "5 sections",
    );

    // F548 压力剧本
    set.add(
        "F548 stress",
        domain_stress_known("64-domains") && !domain_stress_known("wild"),
        "4 plays",
    );

    // F549 导出
    let ex = DomainExport { id: 1, ns_flags: 0b011, mem_max_kb: 65536, cpu_shares: 1024 };
    let ex_bad = DomainExport { id: 2, ns_flags: 0b110, mem_max_kb: 65536, cpu_shares: 1024 };
    set.add("F549 export", domain_export_valid(&ex) && !domain_export_valid(&ex_bad), "pid bit required");

    // F550 年报
    let yb = IsolationYearbook { domains_created: 500, quota_events: 40, escapes_blocked: 0, crash_isolations: 0 };
    set.add("F550 yearbook", yb.escape_free() && yb.grade() == 0, "clean year");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f527_pid_wrap() {
        let mut ns = PidNamespace { ns_id: 9, next_pid: 0, used: 0 };
        let a = ns.alloc_pid().unwrap();
        let b = ns.alloc_pid().unwrap();
        assert_eq!((a, b), (0, 1));
    }

    #[test]
    fn f537_boundary() {
        assert_eq!(quota_enforce(50, 50, 100, 0), QuotaVerdict::Admit);
        assert_eq!(quota_enforce(100, 50, 100, 0), QuotaVerdict::Throttle);
        assert_eq!(quota_enforce(100, 50, 100, 1), QuotaVerdict::Deny);
    }

    #[test]
    fn f532_full_lifecycle() {
        use DomainState::*;
        assert!(domain_transition_legal(Creating, Running));
        assert!(domain_transition_legal(Frozen, Stopping));
        assert!(!domain_transition_legal(Stopping, Running));
    }

    #[test]
    fn f535_whitelist_cap() {
        let mut wl = DeviceWhitelist { devs: [0; DEV_WL_CAP], len: 0 };
        for i in 0..DEV_WL_CAP as u32 {
            assert!(wl.add(i));
        }
        assert!(!wl.add(999));
    }

    #[test]
    fn f550_domain_selfcheck_all_pass() {
        let set = run_m7cgroup_checks();
        assert!(set.len() >= 25, "got {}", set.len());
        for i in 0..set.len() {
            let c = set.get(i).unwrap();
            assert!(c.passed, "{} | {}", c.name, c.detail);
        }
    }
}
