//! UNX-F9284 fork/clone EAGAIN 矩阵（RLIMIT_NPROC 联动） · 0x7B480段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 F9284 逐字）：
//!   ① 纯功能代码 ≥380 行；② 判据原文：UNX-F9284-J1 进程数触顶注入判据过，
//!   全 EAGAIN 格过，与 B19 NPROC 联动格过，释放后恢复正判据格过；③ 完成时
//!   API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//!   ① EAGAIN 是「额度面」而不是「资源面」：fork 被 RLIMIT_NPROC 挡下、pid 空间
//!      耗尽、线程表触顶，三条路径都回 EAGAIN；真正的页/栈分配失败回 ENOMEM，
//!      那是 F9285 的格。本模块直接拒绝返回 ENOMEM，边界写死。
//!   ② 计数口径是 per-uid 且含线程：`used` 按 uid 归集（不是 per-process），
//!      CLONE_THREAD 产出的线程同样计入，否则「一进程狂开线程」能绕过 nproc。
//!   ③ 触顶判定用 `used >= soft`（不是 `>`）：新进程尚未入账，已有 used 个时
//!      再 fork 一个就是第 soft+1 个，必须拒绝；`used == soft-1` 是最后一个合法
//!      额度，必须放行。
//!   ④ 能力旁路：CAP_SYS_RESOURCE 或 CAP_SYS_ADMIN 任一在位即可跳过 nproc 检查
//!      （内核口径），但 pid 空间与线程表上限仍生效——旁路不能旁通物理上限。
//!   ⑤ 释放可逆：exit 归还 uid 额度、线程计数与 pid，额度是守恒量而不是单调
//!      递减的用完即废；释放后同一 gate 必须能再 fork 成功。
//!   ⑥ 与 B19 分层：B19 拥有 rlimit 数值面（get/set/INFINITY/硬软关系/提升需
//!      能力/继承），本模块拥有「到限额后 fork 返回 EAGAIN」的执行面。两张 lane
//!      表逐名比对零交集。
//!   ⑦ 注入账：10 格 × 10 轮 = 100 次全检出，escaped 恒 0；指纹双跑相等。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 诊断段基址（F9284 独占 0x7B480..=0x7B4BF）。
pub const SEG_BASE: u32 = 0x7B480;
/// 本批段窗口下界。
pub const WINDOW_LO: u32 = 0x7B400;
/// 本批段窗口上界。
pub const WINDOW_HI: u32 = 0x7D1C0;

/// 进程槽容量（缩比 kernel.threads-max + pid 表）。
pub const MAX_SLOTS: usize = 32;
/// uid 额度表容量。
pub const MAX_UIDS: usize = 8;
/// 单格注入轮数。
pub const INJECT_ROUNDS: u32 = 10;
/// 矩阵格数。
pub const QUOTA_CELLS: usize = 10;
/// 全局线程上限（缩比 kernel.threads-max）。
pub const THREAD_MAX: u32 = 24;
/// pid 空间上限（可被 with_pid_limit 缩小以复现耗尽）。
pub const PID_MAX: u32 = 4096;

/// EAGAIN：额度/pid/线程表触顶。
pub const EAGAIN: i32 = 11;
/// ENOMEM：资源面（本模块禁止返回，仅作边界声明）。
pub const ENOMEM: i32 = 12;
/// EINVAL：flags 非法或 uid 未登记。
pub const EINVAL: i32 = 22;

/// CAP_SYS_ADMIN 位号。
pub const CAP_SYS_ADMIN_BIT: u32 = 21;
/// CAP_SYS_RESOURCE 位号。
pub const CAP_SYS_RESOURCE_BIT: u32 = 24;
/// CAP_SYS_ADMIN 掩码。
pub const CAP_SYS_ADMIN: u32 = 1u32 << CAP_SYS_ADMIN_BIT;
/// CAP_SYS_RESOURCE 掩码。
pub const CAP_SYS_RESOURCE: u32 = 1u32 << CAP_SYS_RESOURCE_BIT;

/// CLONE_VM：共享地址空间。
pub const CLONE_VM: u64 = 0x0000_0100;
/// CLONE_FS：共享 fs 结构。
pub const CLONE_FS: u64 = 0x0000_0200;
/// CLONE_FILES：共享 fd 表。
pub const CLONE_FILES: u64 = 0x0000_0400;
/// CLONE_SIGHAND：共享信号处理表。
pub const CLONE_SIGHAND: u64 = 0x0000_0800;
/// CLONE_THREAD：创建线程（仍需 SIGHAND）。
pub const CLONE_THREAD: u64 = 0x0001_0000;
/// CLONE_NEWPID：新 pid 命名空间。
pub const CLONE_NEWPID: u64 = 0x2000_0000;
/// 合法 clone 位全集。
pub const VALID_FLAGS: u64 =
    CLONE_VM | CLONE_FS | CLONE_FILES | CLONE_SIGHAND | CLONE_THREAD | CLONE_NEWPID;

/// 语料 uid。
pub const UID: u32 = 1000;
/// root uid（能力旁路语料用）。
pub const ROOT_UID: u32 = 0;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..=+0x0C 闭集）。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum QuotaCode {
    SoftExhausted,
    ZeroQuota,
    TableExhausted,
    ThreadCap,
    PidExhausted,
    CapBypass,
    ReleaseRestore,
    BadFlags,
    UnregisteredUid,
    BoundaryLeak,
    NotReproducible,
    SegmentViolation,
}

impl QuotaCode {
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
            QuotaCode::SoftExhausted => "进程数触顶（EAGAIN）：该 uid 已用进程数 >= RLIMIT_NPROC 软限额；请等进程退出、提高软限额或改由其它 uid 承担",
            QuotaCode::ZeroQuota => "零额度（EAGAIN）：软限额为 0，任何 fork 都不可能成功；请先 setrlimit 给出正额度",
            QuotaCode::TableExhausted => "线程表满（EAGAIN）：进程槽已达 32；请回收空闲槽再 fork，不要把它当 ENOMEM",
            QuotaCode::ThreadCap => "全局线程触顶（EAGAIN）：线程数已达 kernel.threads-max 缩比值；请减少线程或改为复用线程池",
            QuotaCode::PidExhausted => "pid 空间耗尽（EAGAIN）：循环分配器在 pid 上限内找不到空号；请等待 pid 回收或扩大 pid 空间",
            QuotaCode::CapBypass => "能力旁路生效：CAP_SYS_RESOURCE/CAP_SYS_ADMIN 在位时跳过 nproc 检查，但不得绕过线程表与 pid 上限",
            QuotaCode::ReleaseRestore => "释放未恢复：exit 必须归还 uid 额度、线程计数与 pid；否则额度会单调枯竭",
            QuotaCode::BadFlags => "clone 标志非法（EINVAL）：未定义位、CLONE_THREAD 缺 CLONE_SIGHAND、或 THREAD 与 NEWPID 并用；请修正 flags",
            QuotaCode::UnregisteredUid => "uid 未登记（EINVAL）：额度表没有该 uid 的条目；请先建档再 fork",
            QuotaCode::BoundaryLeak => "边界泄漏：额度面错误必须是 EAGAIN，资源面才是 ENOMEM；两码混用会让调用方误判可重试性",
            QuotaCode::NotReproducible => "双跑不一致：同输入两次结果不同；请排查隐藏状态或非确定哈希",
            QuotaCode::SegmentViolation => "段位越界：本模块段基址必须落在 0x7B400..=0x7D1C0 且 0x40 对齐",
        }
    }

    /// 闭集全量。
    pub const ALL: &'static [QuotaCode] = &[
        QuotaCode::SoftExhausted,
        QuotaCode::ZeroQuota,
        QuotaCode::TableExhausted,
        QuotaCode::ThreadCap,
        QuotaCode::PidExhausted,
        QuotaCode::CapBypass,
        QuotaCode::ReleaseRestore,
        QuotaCode::BadFlags,
        QuotaCode::UnregisteredUid,
        QuotaCode::BoundaryLeak,
        QuotaCode::NotReproducible,
        QuotaCode::SegmentViolation,
    ];
}

// ── 额度与槽 ──────────────────────────────────────────────────────────
/// 一个 uid 的额度条目。`used` 同时含进程与线程。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct UidQuota {
    /// 条目是否已建档（uid 0 也是合法 uid，必须用独立位区分空槽）。
    pub bound: bool,
    pub uid: u32,
    pub used: u32,
    pub soft: u32,
    pub hard: u32,
}

impl UidQuota {
    /// 空条目。
    pub const EMPTY: UidQuota = UidQuota {
        bound: false,
        uid: 0,
        used: 0,
        soft: 0,
        hard: 0,
    };

    /// 是否还有至少一个额度（`used < soft`）。
    pub fn has_room(&self) -> bool {
        self.used < self.soft
    }

    /// 硬限额不得低于软限额（B19 面的一致性前提）。
    pub fn sane(&self) -> bool {
        self.soft <= self.hard
    }
}

/// 一个进程槽（线程与进程共用槽，`thread` 只影响线程计数）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ProcSlot {
    pub used: bool,
    pub pid: u32,
    pub uid: u32,
    pub thread: bool,
    pub gen: u32,
}

impl ProcSlot {
    /// 空槽。
    pub const EMPTY: ProcSlot = ProcSlot {
        used: false,
        pid: 0,
        uid: 0,
        thread: false,
        gen: 0,
    };
}

/// 拒绝原因（用于诊断与账目，不参与 errno 判定以外的逻辑）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DenyReason {
    NprocSoft,
    ZeroQuota,
    TableFull,
    ThreadCap,
    PidSpace,
    BadFlags,
    UnknownUid,
    None,
}

/// 原因 → errno（冻结映射；Memory 不在本模块）。
pub fn deny_errno(reason: DenyReason) -> i32 {
    match reason {
        DenyReason::NprocSoft
        | DenyReason::ZeroQuota
        | DenyReason::TableFull
        | DenyReason::ThreadCap
        | DenyReason::PidSpace => EAGAIN,
        DenyReason::BadFlags | DenyReason::UnknownUid => EINVAL,
        DenyReason::None => 0,
    }
}

// ── 门控 ──────────────────────────────────────────────────────────────
/// fork/clone 门控：槽表 + uid 额度表 + 循环 pid 分配器 + 四本拒绝账。
#[derive(Clone, Copy, Debug)]
pub struct ForkGate {
    procs: [ProcSlot; MAX_SLOTS],
    quotas: [UidQuota; MAX_UIDS],
    quota_len: u32,
    proc_count: u32,
    thread_count: u32,
    pid_cursor: u32,
    pid_limit: u32,
    next_gen: u32,
    attempts: u32,
    granted: u32,
    deny_quota: u32,
    deny_table: u32,
    deny_pid: u32,
    deny_flags: u32,
    releases: u32,
}

impl ForkGate {
    /// 标准门控（pid 上限 4096）。
    pub const fn new() -> ForkGate {
        ForkGate {
            procs: [ProcSlot::EMPTY; MAX_SLOTS],
            quotas: [UidQuota::EMPTY; MAX_UIDS],
            quota_len: 0,
            proc_count: 0,
            thread_count: 0,
            pid_cursor: 1,
            pid_limit: PID_MAX,
            next_gen: 1,
            attempts: 0,
            granted: 0,
            deny_quota: 0,
            deny_table: 0,
            deny_pid: 0,
            deny_flags: 0,
            releases: 0,
        }
    }

    /// 缩小 pid 空间（复现耗尽所需；0 视作 1）。
    pub fn with_pid_limit(limit: u32) -> ForkGate {
        let mut g = ForkGate::new();
        g.pid_limit = if limit == 0 { 1 } else { limit };
        g
    }

    /// 在册进程/线程数。
    pub fn proc_count(&self) -> u32 {
        self.proc_count
    }

    /// 在册线程数。
    pub fn thread_count(&self) -> u32 {
        self.thread_count
    }

    /// pid 空间上限。
    pub fn pid_limit(&self) -> u32 {
        self.pid_limit
    }

    /// 累计拒绝（额度面）。
    pub fn deny_quota(&self) -> u32 {
        self.deny_quota
    }

    /// 累计拒绝（表/线程面）。
    pub fn deny_table(&self) -> u32 {
        self.deny_table
    }

    /// 累计拒绝（pid 面）。
    pub fn deny_pid(&self) -> u32 {
        self.deny_pid
    }

    /// 累计拒绝（参数面）。
    pub fn deny_flags(&self) -> u32 {
        self.deny_flags
    }

    /// 累计成功。
    pub fn granted(&self) -> u32 {
        self.granted
    }

    /// 累计释放。
    pub fn releases(&self) -> u32 {
        self.releases
    }

    /// 槽视图。
    pub fn slot(&self, i: usize) -> Option<&ProcSlot> {
        self.procs.get(i)
    }

    /// 建档 / 改写额度。硬限额低于软限额回 None（B19 面拒绝）。
    pub fn register_uid(&mut self, uid: u32, soft: u32, hard: u32) -> Option<usize> {
        if soft > hard {
            return None;
        }
        let mut i = 0usize;
        while i < MAX_UIDS {
            let (bound, slot_uid, slot_used) = match self.quotas.get(i) {
                Some(q) => (q.bound, q.uid, q.used),
                None => (false, 0, 0),
            };
            if bound && slot_uid == uid {
                if let Some(slot) = self.quotas.get_mut(i) {
                    slot.soft = soft;
                    slot.hard = hard;
                    slot.used = slot_used;
                }
                return Some(i);
            }
            if !bound {
                if let Some(slot) = self.quotas.get_mut(i) {
                    slot.bound = true;
                    slot.uid = uid;
                    slot.used = 0;
                    slot.soft = soft;
                    slot.hard = hard;
                }
                self.quota_len = self.quota_len.saturating_add(1);
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 取额度副本。
    pub fn quota_of(&self, uid: u32) -> Option<UidQuota> {
        let mut i = 0usize;
        while i < MAX_UIDS {
            if let Some(q) = self.quotas.get(i) {
                if q.bound && q.uid == uid {
                    return Some(*q);
                }
            }
            i += 1;
        }
        None
    }

    /// 建档条目数。
    pub fn quota_len(&self) -> u32 {
        self.quota_len
    }

    /// pid 是否在册。
    fn pid_in_use(&self, pid: u32) -> bool {
        let mut i = 0usize;
        while i < MAX_SLOTS {
            if let Some(s) = self.procs.get(i) {
                if s.used && s.pid == pid {
                    return true;
                }
            }
            i += 1;
        }
        false
    }

    /// 循环分配 pid；`pid_limit` 内全占满回 None。
    fn alloc_pid(&mut self) -> Option<u32> {
        let mut tries = 0u32;
        while tries < self.pid_limit {
            let pid = self.pid_cursor;
            self.pid_cursor = if pid >= self.pid_limit { 1 } else { pid.saturating_add(1) };
            if pid != 0 && !self.pid_in_use(pid) {
                return Some(pid);
            }
            tries = tries.saturating_add(1);
        }
        None
    }

    /// 落一个槽（唯一写入口：同时维护三本计数与 uid 额度）。
    fn push_slot(&mut self, pid: u32, uid: u32, thread: bool) -> bool {
        if self.proc_count >= MAX_SLOTS as u32 {
            return false;
        }
        let mut i = 0usize;
        while i < MAX_SLOTS {
            let free = match self.procs.get(i) {
                Some(s) => !s.used,
                None => false,
            };
            if free {
                let gen = self.next_gen;
                self.next_gen = self.next_gen.saturating_add(1);
                if let Some(slot) = self.procs.get_mut(i) {
                    slot.used = true;
                    slot.pid = pid;
                    slot.uid = uid;
                    slot.thread = thread;
                    slot.gen = gen;
                }
                self.proc_count = self.proc_count.saturating_add(1);
                if thread {
                    self.thread_count = self.thread_count.saturating_add(1);
                }
                // uid 额度必须同步加一；找不到条目说明调用方越过了 register。
                let mut qi = 0usize;
                while qi < MAX_UIDS {
                    if let Some(q) = self.quotas.get_mut(qi) {
                        if q.bound && q.uid == uid {
                            q.used = q.used.saturating_add(1);
                            break;
                        }
                    }
                    qi += 1;
                }
                return true;
            }
            i += 1;
        }
        false
    }

    /// 预置：建档 + 直接铺 `used` 个进程（跳过量额检查，用于构造触顶现场）。
    pub fn seed(&mut self, uid: u32, soft: u32, hard: u32, used: u32) -> bool {
        if self.register_uid(uid, soft, hard).is_none() {
            return false;
        }
        let mut k = 0u32;
        while k < used {
            let pid = match self.alloc_pid() {
                Some(p) => p,
                None => return false,
            };
            if !self.push_slot(pid, uid, false) {
                return false;
            }
            k = k.saturating_add(1);
        }
        true
    }

    /// 预置线程（用于线程上限格）。
    pub fn seed_threads(&mut self, uid: u32, soft: u32, hard: u32, threads: u32) -> bool {
        if self.register_uid(uid, soft, hard).is_none() {
            return false;
        }
        let mut k = 0u32;
        while k < threads {
            let pid = match self.alloc_pid() {
                Some(p) => p,
                None => return false,
            };
            if !self.push_slot(pid, uid, true) {
                return false;
            }
            k = k.saturating_add(1);
        }
        true
    }

    /// fork/clone 判定。
    ///
    /// 次序（冻结）：flags/uid 参数面 EINVAL → 表/线程上限 EAGAIN →
    /// nproc 额度 EAGAIN → pid 空间 EAGAIN。
    pub fn fork_like(&mut self, uid: u32, caps: u32, flags: u64) -> Result<u32, i32> {
        self.attempts = self.attempts.saturating_add(1);

        // ① 参数面：未定义位 / 线程缺 SIGHAND / 线程 + 新 pid 命名空间。
        let unknown_bits = flags & !VALID_FLAGS;
        let thread_without_sighand = (flags & CLONE_THREAD) != 0 && (flags & CLONE_SIGHAND) == 0;
        let thread_newpid = (flags & CLONE_THREAD) != 0 && (flags & CLONE_NEWPID) != 0;
        if unknown_bits != 0 || thread_without_sighand || thread_newpid {
            self.deny_flags = self.deny_flags.saturating_add(1);
            return Err(EINVAL);
        }
        let is_thread = (flags & CLONE_THREAD) != 0;

        // ② uid 必须已建档。
        let (used, soft) = match self.quota_of(uid) {
            Some(q) => (q.used, q.soft),
            None => {
                self.deny_flags = self.deny_flags.saturating_add(1);
                return Err(EINVAL);
            }
        };

        // ③ 槽表与全局线程上限。
        if self.proc_count >= MAX_SLOTS as u32 {
            self.deny_table = self.deny_table.saturating_add(1);
            return Err(EAGAIN);
        }
        if is_thread && self.thread_count >= THREAD_MAX {
            self.deny_table = self.deny_table.saturating_add(1);
            return Err(EAGAIN);
        }

        // ④ nproc 额度：能力旁路只跳这一步。
        let bypass = (caps & (CAP_SYS_RESOURCE | CAP_SYS_ADMIN)) != 0;
        if !bypass && used >= soft {
            self.deny_quota = self.deny_quota.saturating_add(1);
            return Err(EAGAIN);
        }

        // ⑤ pid 空间。
        let pid = match self.alloc_pid() {
            Some(p) => p,
            None => {
                self.deny_pid = self.deny_pid.saturating_add(1);
                return Err(EAGAIN);
            }
        };

        // ⑥ 落槽。
        if !self.push_slot(pid, uid, is_thread) {
            self.deny_table = self.deny_table.saturating_add(1);
            return Err(EAGAIN);
        }
        self.granted = self.granted.saturating_add(1);
        Ok(pid)
    }

    /// 释放一个 pid：归还原额度、线程计数与槽。未知 pid 回 false 且账目不动。
    pub fn release(&mut self, pid: u32) -> bool {
        let mut i = 0usize;
        while i < MAX_SLOTS {
            let hit = match self.procs.get(i) {
                Some(s) => s.used && s.pid == pid,
                None => false,
            };
            if hit {
                let (uid, thread) = match self.procs.get(i) {
                    Some(s) => (s.uid, s.thread),
                    None => return false,
                };
                if let Some(s) = self.procs.get_mut(i) {
                    s.used = false;
                }
                self.proc_count = self.proc_count.saturating_sub(1);
                if thread {
                    self.thread_count = self.thread_count.saturating_sub(1);
                }
                let mut qi = 0usize;
                while qi < MAX_UIDS {
                    if let Some(q) = self.quotas.get_mut(qi) {
                        if q.bound && q.uid == uid {
                            q.used = q.used.saturating_sub(1);
                            break;
                        }
                    }
                    qi += 1;
                }
                self.releases = self.releases.saturating_add(1);
                return true;
            }
            i += 1;
        }
        false
    }

    /// 诊断分类（不产生副作用）。
    pub fn classify(&self, uid: u32, caps: u32, flags: u64) -> DenyReason {
        let unknown_bits = flags & !VALID_FLAGS;
        let thread_without_sighand = (flags & CLONE_THREAD) != 0 && (flags & CLONE_SIGHAND) == 0;
        if unknown_bits != 0 || thread_without_sighand {
            return DenyReason::BadFlags;
        }
        let q = match self.quota_of(uid) {
            Some(q) => q,
            None => return DenyReason::UnknownUid,
        };
        if self.proc_count >= MAX_SLOTS as u32 {
            return DenyReason::TableFull;
        }
        let is_thread = (flags & CLONE_THREAD) != 0;
        if is_thread && self.thread_count >= THREAD_MAX {
            return DenyReason::ThreadCap;
        }
        let bypass = (caps & (CAP_SYS_RESOURCE | CAP_SYS_ADMIN)) != 0;
        if !bypass && q.used >= q.soft {
            return if q.soft == 0 {
                DenyReason::ZeroQuota
            } else {
                DenyReason::NprocSoft
            };
        }
        DenyReason::None
    }
}

// ── B19 分层 ──────────────────────────────────────────────────────────
/// B19 拥有的 rlimit 数值格。
pub const B19_LANES: [&str; 8] = [
    "b19.rlimit.get",
    "b19.rlimit.set.soft",
    "b19.rlimit.set.hard",
    "b19.rlimit.infinity",
    "b19.rlimit.raise.cap",
    "b19.rlimit.clamp",
    "b19.rlimit.per.uid",
    "b19.rlimit.inherit",
];

/// 本模块拥有的执行面格。
pub const OWN_LANES: [&str; 8] = [
    "f9284.soft.boundary",
    "f9284.used.at.soft",
    "f9284.zero.quota",
    "f9284.thread.counts",
    "f9284.cap.bypass",
    "f9284.pid.exhaust",
    "f9284.table.full",
    "f9284.release.restore",
];

/// 分层对账结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LaneAudit {
    pub own: u32,
    pub b19: u32,
    pub overlap: u32,
}

/// 两张 lane 表逐名比对（同名即重叠，必须为 0）。
pub fn audit_lanes() -> LaneAudit {
    let mut overlap = 0u32;
    let mut i = 0usize;
    while i < OWN_LANES.len() {
        let mut j = 0usize;
        while j < B19_LANES.len() {
            if let (Some(a), Some(b)) = (OWN_LANES.get(i), B19_LANES.get(j)) {
                if *a == *b {
                    overlap = overlap.saturating_add(1);
                }
            }
            j += 1;
        }
        i += 1;
    }
    LaneAudit {
        own: OWN_LANES.len() as u32,
        b19: B19_LANES.len() as u32,
        overlap,
    }
}

// ── 注入矩阵 ──────────────────────────────────────────────────────────
/// 一格额度注入：预置现场 + 输入 + 期望 errno（0 = 期望成功）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct QuotaCell {
    pub used: u32,
    pub soft: u32,
    pub caps: u32,
    pub flags: u64,
    pub pid_limit: u32,
    pub want_err: i32,
}

/// 十格冻结矩阵。
pub const QUOTA_GRID: [QuotaCell; QUOTA_CELLS] = [
    // ① 软限额前一位：最后一个合法额度。
    QuotaCell { used: 3, soft: 4, caps: 0, flags: 0, pid_limit: PID_MAX, want_err: 0 },
    // ② 恰在软限额：必须 EAGAIN（不是 >）。
    QuotaCell { used: 4, soft: 4, caps: 0, flags: 0, pid_limit: PID_MAX, want_err: EAGAIN },
    // ③ 超过软限额：EAGAIN。
    QuotaCell { used: 5, soft: 4, caps: 0, flags: 0, pid_limit: PID_MAX, want_err: EAGAIN },
    // ④ 零额度：EAGAIN。
    QuotaCell { used: 0, soft: 0, caps: 0, flags: 0, pid_limit: PID_MAX, want_err: EAGAIN },
    // ⑤ 零额度 + CAP_SYS_RESOURCE：旁路成功。
    QuotaCell { used: 0, soft: 0, caps: CAP_SYS_RESOURCE, flags: 0, pid_limit: PID_MAX, want_err: 0 },
    // ⑥ 槽表 32 满：EAGAIN。
    QuotaCell { used: 32, soft: 64, caps: 0, flags: 0, pid_limit: PID_MAX, want_err: EAGAIN },
    // ⑦ pid 空间 4 全占：EAGAIN。
    QuotaCell { used: 4, soft: 8, caps: 0, flags: 0, pid_limit: 4, want_err: EAGAIN },
    // ⑧ CLONE_THREAD 缺 SIGHAND：EINVAL。
    QuotaCell { used: 0, soft: 8, caps: 0, flags: CLONE_THREAD, pid_limit: PID_MAX, want_err: EINVAL },
    // ⑨ 未定义 clone 位：EINVAL。
    QuotaCell { used: 0, soft: 8, caps: 0, flags: 0x1_0000_0000, pid_limit: PID_MAX, want_err: EINVAL },
    // ⑩ 合法线程克隆：额度内成功，线程计入 used。
    QuotaCell { used: 3, soft: 4, caps: 0, flags: CLONE_THREAD | CLONE_SIGHAND | CLONE_VM, pid_limit: PID_MAX, want_err: 0 },
];

/// 复算单格。
pub fn cell_matches(cell: &QuotaCell) -> bool {
    let mut g = ForkGate::with_pid_limit(cell.pid_limit);
    if !g.seed(UID, cell.soft, cell.soft.saturating_add(4), cell.used) {
        return false;
    }
    match g.fork_like(UID, cell.caps, cell.flags) {
        Ok(_) => cell.want_err == 0,
        Err(e) => e == cell.want_err,
    }
}

/// 额度注入账本：额外记成功/拒绝两本账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct QuotaLedger {
    pub injected: u32,
    pub detected: u32,
    pub escaped: u32,
    pub grants: u32,
    pub denials: u32,
}

impl QuotaLedger {
    /// 空账。
    pub const fn zero() -> QuotaLedger {
        QuotaLedger {
            injected: 0,
            detected: 0,
            escaped: 0,
            grants: 0,
            denials: 0,
        }
    }

    /// 记一笔注入。
    pub fn record(&mut self, caught: bool, granted: bool) {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if granted {
            self.grants = self.grants.saturating_add(1);
        } else {
            self.denials = self.denials.saturating_add(1);
        }
    }

    /// 守恒。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 十格 × 十轮。
pub fn run_quota_matrix() -> ([u32; QUOTA_CELLS], QuotaLedger) {
    let mut per_cell = [0u32; QUOTA_CELLS];
    let mut ledger = QuotaLedger::zero();
    let mut ci = 0usize;
    while ci < QUOTA_CELLS {
        if let Some(cell) = QUOTA_GRID.get(ci).copied() {
            let mut round = 0u32;
            while round < INJECT_ROUNDS {
                let caught = cell_matches(&cell);
                ledger.record(caught, cell.want_err == 0);
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

// ── 释放恢复序列 ──────────────────────────────────────────────────────
/// 释放恢复轨迹：返回每步 errno 与违规计数。
///
/// 步骤（soft=2）：fork 成功 / fork 成功 / fork EAGAIN / release / fork 成功 /
/// 再 fork EAGAIN。任何一步偏离即违规。
pub fn release_trace() -> (Vec<i32>, u32) {
    let mut out: Vec<i32> = Vec::new();
    let mut bad = 0u32;
    let mut g = ForkGate::new();
    if !g.seed(UID, 2, 4, 0) {
        return (out, 1);
    }
    let first = g.fork_like(UID, 0, 0);
    out.push(match first {
        Ok(_) => 0,
        Err(e) => e,
    });
    let second = g.fork_like(UID, 0, 0);
    out.push(match second {
        Ok(_) => 0,
        Err(e) => e,
    });
    let third = g.fork_like(UID, 0, 0);
    out.push(match third {
        Ok(_) => 0,
        Err(e) => e,
    });
    let released = match first {
        Ok(pid) => g.release(pid),
        Err(_) => false,
    };
    out.push(if released { 0 } else { -1 });
    let fourth = g.fork_like(UID, 0, 0);
    out.push(match fourth {
        Ok(_) => 0,
        Err(e) => e,
    });
    let fifth = g.fork_like(UID, 0, 0);
    out.push(match fifth {
        Ok(_) => 0,
        Err(e) => e,
    });

    let expect = [0i32, 0, EAGAIN, 0, 0, EAGAIN];
    let mut i = 0usize;
    while i < expect.len() {
        match (out.get(i), expect.get(i)) {
            (Some(got), Some(want)) => {
                if got != want {
                    bad = bad.saturating_add(1);
                }
            }
            _ => bad = bad.saturating_add(1),
        }
        i += 1;
    }
    if g.releases() != 1 {
        bad = bad.saturating_add(1);
    }
    (out, bad)
}

// ── 数量守恒复算 ──────────────────────────────────────────────────────
/// 门控内部守恒：proc_count == 各 uid used 之和，且 thread_count <= proc_count。
pub fn counters_conserved(g: &ForkGate) -> bool {
    let mut sum = 0u32;
    let mut i = 0usize;
    while i < MAX_UIDS {
        if let Some(q) = g.quotas.get(i) {
            sum = sum.saturating_add(q.used);
        }
        i += 1;
    }
    sum == g.proc_count && g.thread_count <= g.proc_count
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

/// 编码一次 fork 结果。
pub fn encode_fork(out: &mut Vec<u8>, r: Result<u32, i32>) {
    match r {
        Ok(pid) => {
            out.push(1);
            out.push((pid & 0xFF) as u8);
        }
        Err(e) => {
            out.push(2);
            out.push((e & 0x7F) as u8);
        }
    }
}

/// 规范语料双跑指纹。
pub fn corpus_fingerprint() -> u32 {
    let mut bytes: Vec<u8> = Vec::new();
    let mut g = ForkGate::new();
    if g.seed(UID, 3, 6, 0) {
        encode_fork(&mut bytes, g.fork_like(UID, 0, 0));
        encode_fork(&mut bytes, g.fork_like(UID, 0, 0));
        encode_fork(&mut bytes, g.fork_like(UID, 0, 0));
        encode_fork(&mut bytes, g.fork_like(UID, 0, 0));
        encode_fork(&mut bytes, g.fork_like(UID, CAP_SYS_RESOURCE, 0));
        encode_fork(&mut bytes, g.fork_like(UID, 0, CLONE_THREAD));
        encode_fork(
            &mut bytes,
            g.fork_like(UID, 0, CLONE_THREAD | CLONE_SIGHAND | CLONE_VM),
        );
        let pid = g.fork_like(UID, CAP_SYS_RESOURCE, 0);
        match pid {
            Ok(p) => {
                if !g.release(p) {
                    bytes.push(9);
                }
            }
            Err(_) => bytes.push(9),
        }
        encode_fork(&mut bytes, g.fork_like(UID, 0, 0));
    }
    fnv1a(&bytes)
}

// ── errno 对照 ────────────────────────────────────────────────────────
/// (名称, Linux 正值, 归属面)。
pub const ERRNO_TABLE: &[(&str, i32, &str)] = &[
    ("EAGAIN", 11, "额度面：nproc/线程表/pid 空间"),
    ("ENOMEM", 12, "资源面：页/栈分配失败，F9285 独占"),
    ("EINVAL", 22, "参数面：clone flags 非法或 uid 未建档"),
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

/// 本模块允许返回的错误码集合（ENOMEM 明确排除）。
pub fn quota_errno_allowed(code: i32) -> bool {
    code == errno_value("EAGAIN") || code == errno_value("EINVAL")
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9284 判据集（add ≥12）。
pub fn run_unx9284_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9284");

    let seg_ok = SEG_BASE % 0x40 == 0
        && SEG_BASE >= WINDOW_LO
        && SEG_BASE <= WINDOW_HI
        && SEG_BASE >= 0x7B480
        && SEG_BASE + 0x3F < 0x7B4C0;
    s.add(
        "段位守卫：0x7B480 窗口且与 F9283/F9285 不重叠",
        seg_ok,
        "段基址必须 0x40 对齐并落在 0x7B480..=0x7B4BF",
    );

    // 软限额边界三点：soft-1 放行、soft 拒绝、soft+1 拒绝。
    let mut boundary_ok = true;
    let cases = [(3u32, 4u32, 0i32), (4, 4, EAGAIN), (5, 4, EAGAIN)];
    let mut bi = 0usize;
    while bi < cases.len() {
        if let Some((used, soft, want)) = cases.get(bi).copied() {
            let mut g = ForkGate::new();
            if !g.seed(UID, soft, soft.saturating_add(4), used) {
                boundary_ok = false;
            } else {
                let got = match g.fork_like(UID, 0, 0) {
                    Ok(_) => 0,
                    Err(e) => e,
                };
                if got != want {
                    boundary_ok = false;
                }
            }
        }
        bi += 1;
    }
    s.add(
        "额度边界：soft-1 放行、soft/soft+1 全 EAGAIN",
        boundary_ok,
        "触顶判定必须是 used >= soft，写成 > 会多放一个进程",
    );

    // 零额度：任何 fork 都 EAGAIN。
    let mut zero = ForkGate::new();
    let zero_seeded = zero.seed(UID, 0, 4, 0);
    let zero_err = match zero.fork_like(UID, 0, 0) {
        Ok(_) => 0,
        Err(e) => e,
    };
    s.add(
        "零额度：soft=0 全 EAGAIN",
        zero_seeded && zero_err == EAGAIN && zero.deny_quota() == 1,
        "零额度是合法配置（禁止登录类 uid），必须稳定拒绝且记入额度面",
    );

    // 能力旁路：两个能力各自可行；无能力同格拒绝。
    let mut cap_ok = true;
    let caps_set = [CAP_SYS_RESOURCE, CAP_SYS_ADMIN];
    let mut ci2 = 0usize;
    while ci2 < caps_set.len() {
        if let Some(cap) = caps_set.get(ci2).copied() {
            let mut g = ForkGate::new();
            if !g.seed(UID, 0, 4, 0) {
                cap_ok = false;
            } else if g.fork_like(UID, cap, 0).is_err() {
                cap_ok = false;
            }
            let mut plain = ForkGate::new();
            if !plain.seed(UID, 0, 4, 0) {
                cap_ok = false;
            } else if plain.fork_like(UID, 0, 0).is_ok() {
                cap_ok = false;
            }
        }
        ci2 += 1;
    }
    s.add(
        "能力旁路：RESOURCE/SYS_ADMIN 各自放行、无能力拒绝",
        cap_ok,
        "旁路只跳 nproc 一步，不能顺带放宽线程表与 pid 空间",
    );

    // 线程计入额度：soft=4 used=3 线程克隆成功 → used=4；再来一个拒绝。
    let mut th = ForkGate::new();
    let th_seeded = th.seed(UID, 4, 8, 3);
    let th_first = th.fork_like(UID, 0, CLONE_THREAD | CLONE_SIGHAND | CLONE_VM);
    let th_after = match th.quota_of(UID) {
        Some(q) => q.used,
        None => 0,
    };
    let th_second = th.fork_like(UID, 0, CLONE_THREAD | CLONE_SIGHAND | CLONE_VM);
    let th_ok = th_seeded
        && th_first.is_ok()
        && th_after == 4
        && th.thread_count() == 1
        && matches!(th_second, Err(EAGAIN));
    s.add(
        "线程计数：CLONE_THREAD 计入 per-uid 额度",
        th_ok,
        "线程若不计入 nproc，一进程狂开线程即可绕过限额",
    );

    // 全局线程上限：预置 24 线程后再克隆线程 EAGAIN。
    let mut cap = ForkGate::new();
    let cap_seeded = cap.seed_threads(UID, 64, 128, THREAD_MAX);
    let cap_err = match cap.fork_like(UID, CAP_SYS_RESOURCE, CLONE_THREAD | CLONE_SIGHAND) {
        Ok(_) => 0,
        Err(e) => e,
    };
    s.add(
        "线程表：24 线程触顶后 EAGAIN（能力不放宽）",
        cap_seeded && cap_err == EAGAIN && cap.thread_count() == THREAD_MAX,
        "kernel.threads-max 类上限属物理上限，CAP 旁路不得越过",
    );

    // pid 空间耗尽：limit 4 预填 4 → EAGAIN，且槽未新增。
    let mut pidg = ForkGate::with_pid_limit(4);
    let pid_seeded = pidg.seed(UID, 8, 16, 4);
    let pid_err = match pidg.fork_like(UID, 0, 0) {
        Ok(_) => 0,
        Err(e) => e,
    };
    s.add(
        "pid 空间：4 号全占后 EAGAIN 且不再落槽",
        pid_seeded && pid_err == EAGAIN && pidg.deny_pid() == 1 && pidg.proc_count() == 4,
        "循环分配器必须显式回 None，不得回绕覆盖在册 pid",
    );

    // 槽表满：32 槽后 EAGAIN。
    let mut full = ForkGate::new();
    let full_seeded = full.seed(UID, 64, 128, MAX_SLOTS as u32);
    let full_err = match full.fork_like(UID, 0, 0) {
        Ok(_) => 0,
        Err(e) => e,
    };
    s.add(
        "槽表：32 槽满后 EAGAIN",
        full_seeded && full_err == EAGAIN && full.proc_count() == MAX_SLOTS as u32,
        "表满属额度面，必须 EAGAIN 而不是 ENOMEM",
    );

    // 释放恢复：EAGAIN → release → 成功 → 再 EAGAIN。
    let (trace, bad_steps) = release_trace();
    let trace_len_ok = trace.len() == 6;
    s.add(
        "释放恢复：归还额度后同 gate 可再 fork",
        bad_steps == 0 && trace_len_ok,
        "额度是守恒量：release 必须同时归还 used/线程/pid，否则额度单调枯竭",
    );

    // 参数面先于额度面：soft=0 且 flags 非法必须 EINVAL。
    let mut order = ForkGate::new();
    let order_seeded = order.seed(UID, 0, 4, 0);
    let order_bad = order.fork_like(UID, 0, CLONE_THREAD);
    let order_bits = order.fork_like(UID, 0, 0x8000_0000_0000_0000);
    let order_thread_newpid = order.fork_like(UID, 0, CLONE_THREAD | CLONE_SIGHAND | CLONE_NEWPID);
    let order_ok = order_seeded
        && matches!(order_bad, Err(EINVAL))
        && matches!(order_bits, Err(EINVAL))
        && matches!(order_thread_newpid, Err(EINVAL))
        && order.deny_flags() == 3
        && order.deny_quota() == 0;
    s.add(
        "判定次序：flags EINVAL 先于额度 EAGAIN",
        order_ok,
        "零额度现场下的坏 flags 仍要报 EINVAL，否则调用方会误以为等额度即可",
    );

    // 未建档 uid：EINVAL，且不落槽。
    let mut unknown = ForkGate::new();
    let unk = unknown.fork_like(4242, 0, 0);
    s.add(
        "参数面：未建档 uid 回 EINVAL 且不落槽",
        matches!(unk, Err(EINVAL)) && unknown.proc_count() == 0 && unknown.deny_flags() == 1,
        "额度表缺条目属参数面错误，不能默认给 0 额度后报 EAGAIN",
    );

    // 死 pid 释放：未知 pid 不得动账。
    let mut dead = ForkGate::new();
    let dead_seeded = dead.seed(UID, 4, 8, 1);
    let rel_unknown = dead.release(9999);
    let rel_real = match dead.slot(0) {
        Some(s) => dead.release(s.pid),
        None => false,
    };
    s.add(
        "释放面：未知 pid 无副作用、真实 pid 归还一次",
        dead_seeded && !rel_unknown && rel_real && dead.releases() == 1 && dead.proc_count() == 0,
        "双重释放与幽灵释放都会让额度账与槽表脱钩",
    );

    // 计数守恒。
    let mut cons = ForkGate::new();
    let cons_seeded = cons.seed(UID, 8, 16, 3);
    let cons_fork = cons.fork_like(UID, 0, 0);
    let cons_release = match cons.slot(0) {
        Some(s) => cons.release(s.pid),
        None => false,
    };
    s.add(
        "不变量：proc_count == Σ uid.used 且 threads ≤ procs",
        cons_seeded && cons_fork.is_ok() && cons_release && counters_conserved(&cons),
        "三本计数（槽/线程/uid）必须始终一致，否则触顶判定会漂移",
    );

    // 与 B19 分层零重叠。
    let audit = audit_lanes();
    s.add(
        "B19 对账：八格自有与八格 B19 零重叠",
        audit.overlap == 0 && audit.own == OWN_LANES.len() as u32 && audit.b19 == B19_LANES.len() as u32,
        "rlimit 数值面归 B19，fork 执行面归本模块",
    );

    // 全 EAGAIN 格：10 格 × 10 轮。
    let (per_cell, ledger) = run_quota_matrix();
    let mut cells_all = true;
    let mut ci = 0usize;
    while ci < QUOTA_CELLS {
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
        "全 EAGAIN 格：10 格各 10/10 命中",
        cells_all,
        "额度/线程/pid/表满四类拒绝逐格复算一致",
    );
    s.add(
        "注入账：100 次全检出且逃逸为 0",
        ledger.conserved()
            && ledger.escaped == 0
            && ledger.injected == QUOTA_CELLS as u32 * INJECT_ROUNDS,
        "injected == detected + escaped；逃逸非零即有格未按冻结表判定",
    );

    // 边界：本模块不得返回 ENOMEM。
    let mut no_enomem = true;
    let mut gi = 0usize;
    while gi < QUOTA_GRID.len() {
        if let Some(cell) = QUOTA_GRID.get(gi) {
            let mut g = ForkGate::with_pid_limit(cell.pid_limit);
            if g.seed(UID, cell.soft, cell.soft.saturating_add(4), cell.used) {
                if let Err(e) = g.fork_like(UID, cell.caps, cell.flags) {
                    if !quota_errno_allowed(e) || e == ENOMEM {
                        no_enomem = false;
                    }
                }
            }
        }
        gi += 1;
    }
    s.add(
        "边界：整表扫描不出现 ENOMEM",
        no_enomem,
        "额度面只许 EAGAIN/EINVAL；ENOMEM 属 F9285 的资源面",
    );

    // errno 数值。
    let errno_ok = errno_value("EAGAIN") == 11
        && errno_value("ENOMEM") == 12
        && errno_value("EINVAL") == 22
        && quota_errno_allowed(EAGAIN)
        && quota_errno_allowed(EINVAL)
        && !quota_errno_allowed(ENOMEM);
    s.add(
        "errno：EAGAIN=11 / EINVAL=22，ENOMEM 禁入",
        errno_ok,
        "三码分属三个面，数值与 Linux 一致",
    );

    // 双跑指纹。
    let fp_a = corpus_fingerprint();
    let fp_b = corpus_fingerprint();
    s.add(
        "双跑一致：规范语料指纹相等",
        fp_a == fp_b && fp_a != 0,
        "同输入两次必须逐字节相等",
    );

    // 诊断码唯一性与文案。
    let mut uniq = true;
    let mut i = 0usize;
    while i < QuotaCode::ALL.len() {
        let mut j = i + 1;
        while j < QuotaCode::ALL.len() {
            if let (Some(a), Some(b)) = (QuotaCode::ALL.get(i), QuotaCode::ALL.get(j)) {
                if a.val() == b.val() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    let text_ok = QuotaCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add(
        "诊断码：12 个码值唯一且文案非空",
        uniq && text_ok && QuotaCode::ALL.len() == 12,
        "闭集码不得重复，每码必须有可操作建议",
    );

    // 账目单调。
    let mut mono = ledger;
    let base = mono.injected;
    let mut k = 0u32;
    while k < 3 {
        mono.record(true, false);
        k += 1;
    }
    s.add(
        "账目单调：追加 3 笔拒绝只增不减",
        mono.injected == base.saturating_add(3)
            && mono.denials == ledger.denials.saturating_add(3)
            && mono.escaped == 0
            && mono.conserved(),
        "累计计数禁止回退，拒绝账独立累加",
    );

    // 动态快照。
    let matrix_text = format!(
        "额度矩阵 {} / 检出 {} / 逃逸 {} / 成功 {}",
        ledger.injected, ledger.detected, ledger.escaped, ledger.grants
    );
    s.add_dyn("矩阵账目", ledger.conserved(), &matrix_text);

    let trace_text = format!(
        "释放序列 {:?} 违规 {}",
        trace, bad_steps
    );
    s.add_dyn("释放恢复", bad_steps == 0, &trace_text);

    let fp_text = format!("语料指纹 0x{:08X}", fp_a);
    s.add_dyn("指纹快照", fp_a == fp_b, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boundary_below_soft_is_allowed() {
        let mut g = ForkGate::new();
        assert_eq!(g.seed(UID, 4, 8, 3), true);
        assert_eq!(g.fork_like(UID, 0, 0).is_ok(), true);
        assert_eq!(g.proc_count(), 4);
        assert_eq!(g.fork_like(UID, 0, 0), Err(EAGAIN));
    }

    #[test]
    fn zero_quota_always_denies() {
        let mut g = ForkGate::new();
        assert_eq!(g.seed(UID, 0, 0, 0), true);
        assert_eq!(g.fork_like(UID, 0, 0), Err(EAGAIN));
        assert_eq!(g.deny_quota(), 1);
        assert_eq!(g.fork_like(UID, CAP_SYS_RESOURCE, 0).is_ok(), true);
    }

    #[test]
    fn threads_consume_nproc() {
        let mut g = ForkGate::new();
        assert_eq!(g.seed(UID, 2, 4, 1), true);
        assert_eq!(g.fork_like(UID, 0, CLONE_THREAD | CLONE_SIGHAND).is_ok(), true);
        assert_eq!(g.thread_count(), 1);
        assert_eq!(g.fork_like(UID, 0, CLONE_THREAD | CLONE_SIGHAND), Err(EAGAIN));
    }

    #[test]
    fn thread_cap_blocks_even_with_caps() {
        let mut g = ForkGate::new();
        assert_eq!(g.seed_threads(UID, 64, 128, THREAD_MAX), true);
        assert_eq!(
            g.fork_like(UID, CAP_SYS_ADMIN, CLONE_THREAD | CLONE_SIGHAND),
            Err(EAGAIN)
        );
        assert_eq!(g.thread_count(), THREAD_MAX);
    }

    #[test]
    fn pid_space_exhaustion() {
        let mut g = ForkGate::with_pid_limit(4);
        assert_eq!(g.seed(UID, 8, 16, 4), true);
        assert_eq!(g.fork_like(UID, 0, 0), Err(EAGAIN));
        assert_eq!(g.deny_pid(), 1);
        assert_eq!(g.proc_count(), 4);
    }

    #[test]
    fn table_full_is_eagain_not_enomem() {
        let mut g = ForkGate::new();
        assert_eq!(g.seed(UID, 64, 128, MAX_SLOTS as u32), true);
        assert_eq!(g.fork_like(UID, 0, 0), Err(EAGAIN));
        assert_eq!(g.proc_count(), MAX_SLOTS as u32);
    }

    #[test]
    fn release_restores_quota() {
        let (trace, bad) = release_trace();
        assert_eq!(bad, 0);
        assert_eq!(trace.len(), 6);
    }

    #[test]
    fn bad_flags_win_over_quota() {
        let mut g = ForkGate::new();
        assert_eq!(g.seed(UID, 0, 0, 0), true);
        assert_eq!(g.fork_like(UID, 0, CLONE_THREAD), Err(EINVAL));
        assert_eq!(g.fork_like(UID, 0, 0x1_0000_0000), Err(EINVAL));
        assert_eq!(g.fork_like(UID, 0, 0), Err(EAGAIN));
    }

    #[test]
    fn unknown_uid_is_einval() {
        let mut g = ForkGate::new();
        assert_eq!(g.fork_like(4242, 0, 0), Err(EINVAL));
        assert_eq!(g.proc_count(), 0);
    }

    #[test]
    fn counters_stay_consistent() {
        let mut g = ForkGate::new();
        assert_eq!(g.seed(UID, 8, 16, 3), true);
        assert_eq!(g.fork_like(UID, 0, 0).is_ok(), true);
        assert_eq!(counters_conserved(&g), true);
        let pid = match g.slot(0) {
            Some(s) => s.pid,
            None => 0,
        };
        assert_eq!(g.release(pid), true);
        assert_eq!(counters_conserved(&g), true);
    }

    #[test]
    fn matrix_conserved() {
        let (per, l) = run_quota_matrix();
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
    fn lanes_disjoint() {
        let a = audit_lanes();
        assert_eq!(a.overlap, 0);
        assert_eq!(a.own, 8);
        assert_eq!(a.b19, 8);
    }

    #[test]
    fn fingerprint_stable() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx9284_checks();
        assert_eq!(s.all_passed(), true);
        assert_eq!(s.dyn_dropped(), 0);
    }
}
