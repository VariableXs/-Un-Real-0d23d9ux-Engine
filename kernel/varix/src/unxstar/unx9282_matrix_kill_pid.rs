//! UNX-F9282 kill 坏 pid 矩阵（ESRCH/EPERM/EINVAL） · 0x7B400段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 F9282 逐字）：
//!   ① 纯功能代码 ≥360 行；② 判据原文：UNX-F9282-J1 三错误码注入矩阵判据过，
//!   pid=1 EPERM 特例格过，非法 sig 全 EINVAL 格过，不存在 pid 全 ESRCH 格过；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//!   ① 三码分属三个面，不可互替：EINVAL 属参数面（sig 不在 0..=64），ESRCH 属
//!      寻址面（pid 选择器解析不到活体或僵尸），EPERM 属权限面（解析到了但调用
//!      者既非同一 uid 也没有 CAP_KILL）。冻结次序 EINVAL(0) → ESRCH(1) →
//!      EPERM(2)，由 `order_rank` 显性化：次序颠倒会把缺目标报成坏参数，调用方
//!      按 EINVAL 去改参数就永远修不好。
//!   ② sig==0 是探针不是信号：只做存在性与权限判定，成功时 delivered==0 而
//!      targets>0。把探针当投递，kill(pid,0) 就变成一次静默信号。
//!   ③ pid 选择器四形态（>0 单体 / 0 本组 / -1 全表 / <0 取 -pid 组）走四条不同
//!      解析路径：单体先寻址再鉴权；组形态先算成员基数，空组回 ESRCH，成员全被
//!      拒回 EPERM，只要有一个可投就成功（Linux kill(-1) 的部分投递语义）。
//!   ④ pid=1 是保护格：外来 uid 的探针也 EPERM；即便 uid 为 0 且带 CAP_KILL，
//!      SIGKILL/SIGSTOP 仍 EPERM——不可捕获/不可停止的两个信号对 init 一律拒绝，
//!      否则一次误投就是整机不可恢复。
//!   ⑤ 目标状态机 Alive/Zombie/Dead 三档：僵尸可寻址（投递被丢弃但调用成功），
//!      Dead 槽对寻址不可见；pid 复用必须换 generation，旧 gen 命中回 ESRCH，
//!      避免「杀到复用同一 pid 的新进程」。
//!   ⑥ 注入账：12 格 × 10 轮 = 120 次，injected == detected + escaped 且
//!      escaped 恒 0；指纹对同一语料双跑必须逐字节相等。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 诊断段基址（F9282 独占 0x7B400..=0x7B43F）。
pub const SEG_BASE: u32 = 0x7B400;
/// 本批段窗口下界（段位守卫用）。
pub const WINDOW_LO: u32 = 0x7B400;
/// 本批段窗口上界（段位守卫用）。
pub const WINDOW_HI: u32 = 0x7D1C0;

/// 目标表容量（够铺满 1 进程 + 2 组 + 僵尸 + 复用槽）。
pub const MAX_TARGETS: usize = 12;
/// 单格注入复测轮数。
pub const INJECT_ROUNDS: u32 = 10;
/// 冻结矩阵格数。
pub const GRID_CELLS: usize = 12;

/// 合法信号上界（Linux _NSIG-1 = 64）。
pub const SIGRTMAX: i32 = 64;
/// 实时信号下界。
pub const SIGRTMIN: i32 = 32;
/// SIGKILL（不可捕获）。
pub const SIGKILL: i32 = 9;
/// SIGSTOP（不可停止）。
pub const SIGSTOP: i32 = 19;
/// init 的 pid。
pub const INIT_PID: i32 = 1;

/// ESRCH：没有匹配目标。
pub const ESRCH: i32 = 3;
/// EPERM：目标存在但无权限。
pub const EPERM: i32 = 1;
/// EINVAL：信号号非法。
pub const EINVAL: i32 = 22;

/// CAP_KILL 位号。
pub const CAP_KILL_BIT: u32 = 5;
/// CAP_SYS_ADMIN 位号。
pub const CAP_SYS_ADMIN_BIT: u32 = 21;
/// CAP_KILL 位掩码。
pub const CAP_KILL: u32 = 1u32 << CAP_KILL_BIT;
/// CAP_SYS_ADMIN 位掩码。
pub const CAP_SYS_ADMIN: u32 = 1u32 << CAP_SYS_ADMIN_BIT;

/// 语料调用者 pid（同时也是它自己进程组的组长）。
pub const CALLER_PID: i32 = 100;
/// 语料调用者 uid。
pub const CALLER_UID: u32 = 1000;
/// 语料调用者进程组。
pub const CALLER_PGID: i32 = 100;
/// init 所属 uid（root）。
pub const ROOT_UID: u32 = 0;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..=+0x0C 闭集，码值两两唯一）。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KillCode {
    SigOutOfRange,
    InitGuarded,
    NoSuchProcess,
    PermissionDenied,
    GroupEmpty,
    ProbeDelivered,
    StaleGeneration,
    OrderViolation,
    WrongErrno,
    LedgerRegress,
    NotReproducible,
    SegmentViolation,
}

impl KillCode {
    /// 闭集下标（0..11）。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 段内码值 = 段基址 + 1 + 下标（u32 口径，供唯一性复算）。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self.code() as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            KillCode::SigOutOfRange => "信号号越界被拒（EINVAL）：合法域是 0..=64（0 仅作探针）；请先钳制信号号再调用 kill",
            KillCode::InitGuarded => "init 保护格命中（EPERM）：pid=1 不接受外来 uid 投递，SIGKILL/SIGSTOP 即便是 root 也拒绝；请不要向 init 发信号",
            KillCode::NoSuchProcess => "目标不存在（ESRCH）：pid 选择器在当前表中解析不到活体或僵尸；请先确认进程存活，回收后不要复用旧 pid",
            KillCode::PermissionDenied => "权限不足（EPERM）：目标 uid 与调用者不同且没有 CAP_KILL；请改用同 uid 进程或提升能力后再投递",
            KillCode::GroupEmpty => "进程组为空（ESRCH）：pid=0 或 pid=-pgid 的成员基数为 0；请先确认组成员是否已全部退出",
            KillCode::ProbeDelivered => "探针被误当投递：sig=0 必须 delivered==0 而 targets>0；请分离探针与投递两条返回路径",
            KillCode::StaleGeneration => "旧 generation 命中（ESRCH）：pid 已复用，旧句柄不得指向新进程；请按 generation 重新解析",
            KillCode::OrderViolation => "判定次序错误：EINVAL 必须先于 ESRCH 先于 EPERM；顺序错了会把缺目标误报成坏参数",
            KillCode::WrongErrno => "错误码不符：同形态同前置状态必须固定返回同一 errno；请对照冻结矩阵复算判定器",
            KillCode::LedgerRegress => "注入账倒退：计数只增不减；请检查是否有回滚路径误改累计值",
            KillCode::NotReproducible => "双跑不一致：同输入两次结果不同；请排查隐藏状态或非确定哈希",
            KillCode::SegmentViolation => "段位越界：本模块段基址必须落在 0x7B400..=0x7D1C0 且 0x40 对齐",
        }
    }

    /// 闭集全量（顺序即码序）。
    pub const ALL: &'static [KillCode] = &[
        KillCode::SigOutOfRange,
        KillCode::InitGuarded,
        KillCode::NoSuchProcess,
        KillCode::PermissionDenied,
        KillCode::GroupEmpty,
        KillCode::ProbeDelivered,
        KillCode::StaleGeneration,
        KillCode::OrderViolation,
        KillCode::WrongErrno,
        KillCode::LedgerRegress,
        KillCode::NotReproducible,
        KillCode::SegmentViolation,
    ];
}

// ── 信号面 ────────────────────────────────────────────────────────────
/// 信号分类：探针 / 标准信号 / 实时信号 / 非法。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SigKind {
    /// sig==0：只探测存在性与权限。
    Probe,
    /// 1..=31：标准信号。
    Standard,
    /// 32..=64：实时信号（可排队）。
    Realtime,
    /// 其余一律非法。
    Invalid,
}

/// 信号分类（全域覆盖，无遗漏分支）。
pub fn sig_kind(sig: i32) -> SigKind {
    if sig == 0 {
        SigKind::Probe
    } else if sig > 0 && sig < SIGRTMIN {
        SigKind::Standard
    } else if sig >= SIGRTMIN && sig <= SIGRTMAX {
        SigKind::Realtime
    } else {
        SigKind::Invalid
    }
}

/// 信号号是否可进入判定器（含探针 0）。
pub fn sig_ok(sig: i32) -> bool {
    !matches!(sig_kind(sig), SigKind::Invalid)
}

/// 标准信号名册：(名字, 编号, 默认动作)。只用于文案与表驱动复算。
pub const SIGNAL_TABLE: &[(&str, i32, &str)] = &[
    ("SIGHUP", 1, "terminate"),
    ("SIGINT", 2, "terminate"),
    ("SIGQUIT", 3, "core"),
    ("SIGKILL", 9, "terminate-uncatchable"),
    ("SIGSEGV", 11, "core"),
    ("SIGTERM", 15, "terminate"),
    ("SIGSTOP", 19, "stop-uncatchable"),
    ("SIGRTMIN", 32, "queued"),
    ("SIGRTMAX", 64, "queued"),
];

/// 按编号取名；表外回显 `<unnamed>`（禁止空串）。
pub fn sig_name(sig: i32) -> &'static str {
    let mut i = 0usize;
    while i < SIGNAL_TABLE.len() {
        if let Some(row) = SIGNAL_TABLE.get(i) {
            if row.1 == sig {
                return row.0;
            }
        }
        i += 1;
    }
    if sig == 0 {
        "probe-0"
    } else {
        "<unnamed>"
    }
}

/// 是否不可捕获信号（KILL/STOP）。
pub fn sig_uncatchable(sig: i32) -> bool {
    sig == SIGKILL || sig == SIGSTOP
}

// ── 目标模型 ──────────────────────────────────────────────────────────
/// 目标生命周期：活体 / 僵尸 / 已回收。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Life {
    /// 可寻址、可投递。
    Alive,
    /// 可寻址，信号被内核丢弃但仍算成功。
    Zombie,
    /// 对寻址不可见（已回收）。
    Dead,
}

/// 一个目标槽。`gen` 由空间单调发号，用于识别 pid 复用。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Target {
    pub used: bool,
    pub pid: i32,
    pub pgid: i32,
    pub uid: u32,
    pub gen: u32,
    pub life: Life,
    pub is_init: bool,
}

impl Target {
    /// 定容数组初始化占位。
    pub const EMPTY: Target = Target {
        used: false,
        pid: 0,
        pgid: 0,
        uid: 0,
        gen: 0,
        life: Life::Dead,
        is_init: false,
    };

    /// 活体目标（语料构造用）。
    pub const fn alive(pid: i32, pgid: i32, uid: u32, init: bool) -> Target {
        Target {
            used: true,
            pid,
            pgid,
            uid,
            gen: 0,
            life: Life::Alive,
            is_init: init,
        }
    }

    /// 僵尸目标（语料构造用）。
    pub const fn zombie(pid: i32, pgid: i32, uid: u32) -> Target {
        Target {
            used: true,
            pid,
            pgid,
            uid,
            gen: 0,
            life: Life::Zombie,
            is_init: false,
        }
    }

    /// 是否可被寻址（活体或僵尸）。
    pub fn addressable(&self) -> bool {
        self.used && !matches!(self.life, Life::Dead)
    }
}

/// 调用者上下文（4 个字段全部参与鉴权，缺一个就会把跨 uid 当同 uid）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CallerCtx {
    pub pid: i32,
    pub uid: u32,
    pub caps: u32,
    pub pgid: i32,
}

/// 语料调用者：普通 uid、零能力、本组组长。
pub const CALLER: CallerCtx = CallerCtx {
    pid: CALLER_PID,
    uid: CALLER_UID,
    caps: 0,
    pgid: CALLER_PGID,
};

/// 一次成功投递的结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Delivery {
    /// 实际会被投递的目标数（探针时也 >0）。
    pub targets: u32,
    /// 真正入队的信号数（探针恒 0）。
    pub delivered: u32,
    /// 本次调用是否为探针（sig==0）。
    pub probe: bool,
}

// ── 目标空间 ──────────────────────────────────────────────────────────
/// 定容目标表 + 单调 generation 发号器。
#[derive(Clone, Copy, Debug)]
pub struct KillSpace {
    targets: [Target; MAX_TARGETS],
    count: u32,
    next_gen: u32,
}

impl KillSpace {
    /// 空空间。
    pub const fn new() -> KillSpace {
        KillSpace {
            targets: [Target::EMPTY; MAX_TARGETS],
            count: 0,
            next_gen: 1,
        }
    }

    /// 已用槽数。
    pub fn count(&self) -> u32 {
        self.count
    }

    /// 槽视图（越界回 None，绝不裸索引）。
    pub fn slot(&self, i: usize) -> Option<&Target> {
        self.targets.get(i)
    }

    /// 发布一个目标；满表回 None 且不占用任何槽。
    pub fn spawn(&mut self, pid: i32, pgid: i32, uid: u32, init: bool) -> Option<usize> {
        let mut i = 0usize;
        while i < MAX_TARGETS {
            let free = match self.targets.get(i) {
                Some(t) => !t.used,
                None => false,
            };
            if free {
                let gen = self.next_gen;
                self.next_gen = self.next_gen.saturating_add(1);
                if let Some(slot) = self.targets.get_mut(i) {
                    slot.used = true;
                    slot.pid = pid;
                    slot.pgid = pgid;
                    slot.uid = uid;
                    slot.gen = gen;
                    slot.life = Life::Alive;
                    slot.is_init = init;
                }
                self.count = self.count.saturating_add(1);
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 按 pid 寻址（活体/僵尸可见，Dead 不可见）。
    pub fn addr(&self, pid: i32) -> Option<usize> {
        let mut i = 0usize;
        while i < MAX_TARGETS {
            if let Some(t) = self.targets.get(i) {
                if t.addressable() && t.pid == pid {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 按 pid + generation 寻址：旧 gen 一律回 None。
    pub fn addr_gen(&self, pid: i32, gen: u32) -> Option<usize> {
        let idx = self.addr(pid)?;
        match self.targets.get(idx) {
            Some(t) => {
                if t.gen == gen {
                    Some(idx)
                } else {
                    None
                }
            }
            None => None,
        }
    }

    /// 回收一个目标（Alive/Zombie → Dead）。返回是否真的回收。
    pub fn reap(&mut self, pid: i32) -> bool {
        if let Some(idx) = self.addr(pid) {
            if let Some(t) = self.targets.get_mut(idx) {
                t.life = Life::Dead;
                self.count = self.count.saturating_sub(1);
                return true;
            }
        }
        false
    }

    /// 取目标进程组；解析不到回 None。
    pub fn pgid_of(&self, pid: i32) -> Option<i32> {
        match self.addr(pid) {
            Some(i) => self.targets.get(i).map(|t| t.pgid),
            None => None,
        }
    }

    /// 按 pid 取目标副本；解析不到回 None。
    pub fn peek(&self, pid: i32) -> Option<Target> {
        match self.addr(pid) {
            Some(i) => self.targets.get(i).copied(),
            None => None,
        }
    }
}

// ── 权限判定 ──────────────────────────────────────────────────────────
/// kill 权限判定（纯函数，无副作用）。
///
/// 不变量：
///   - 非 init：同 uid 或持 CAP_KILL 即可；
///   - init：自身可投；外来 uid 一律拒绝；KILL/STOP 即便 root+CAP 也拒绝。
pub fn permit(caller: &CallerCtx, target: &Target, sig: i32) -> bool {
    if target.is_init {
        if caller.pid == INIT_PID {
            return true;
        }
        if sig_uncatchable(sig) {
            return false;
        }
        return caller.uid == target.uid && (caller.caps & CAP_KILL) != 0;
    }
    caller.uid == target.uid || (caller.caps & CAP_KILL) != 0
}

/// 组成员基数：(候选数, 可投数)。init 不计入组统计。
pub fn group_tally(space: &KillSpace, caller: &CallerCtx, pgid: i32, sig: i32) -> (u32, u32) {
    let mut candidates = 0u32;
    let mut allowed = 0u32;
    let mut i = 0usize;
    while i < MAX_TARGETS {
        if let Some(t) = space.slot(i) {
            if t.addressable() && !t.is_init && t.pgid == pgid {
                candidates = candidates.saturating_add(1);
                if permit(caller, t, sig) {
                    allowed = allowed.saturating_add(1);
                }
            }
        }
        i += 1;
    }
    (candidates, allowed)
}

/// 全表基数（kill(-1) 用）：(候选数, 可投数)，init 不计入。
pub fn all_tally(space: &KillSpace, caller: &CallerCtx, sig: i32) -> (u32, u32) {
    let mut candidates = 0u32;
    let mut allowed = 0u32;
    let mut i = 0usize;
    while i < MAX_TARGETS {
        if let Some(t) = space.slot(i) {
            if t.addressable() && !t.is_init {
                candidates = candidates.saturating_add(1);
                if permit(caller, t, sig) {
                    allowed = allowed.saturating_add(1);
                }
            }
        }
        i += 1;
    }
    (candidates, allowed)
}

// ── kill 判定器 ───────────────────────────────────────────────────────
/// kill 语义判定（纯函数，可重复；不修改目标表）。
///
/// 次序（冻结）：EINVAL 参数面 → ESRCH 寻址面 → EPERM 权限面。
pub fn do_kill(
    space: &KillSpace,
    caller: &CallerCtx,
    pid: i32,
    sig: i32,
) -> Result<Delivery, i32> {
    // ① 参数面：信号号先于一切寻址判定。
    if !sig_ok(sig) {
        return Err(EINVAL);
    }
    let probe = sig == 0;

    // ② 单体形态：先寻址，再鉴权。
    if pid > 0 {
        let idx = match space.addr(pid) {
            Some(i) => i,
            None => return Err(ESRCH),
        };
        let target = match space.slot(idx) {
            Some(t) => *t,
            None => return Err(ESRCH),
        };
        if !permit(caller, &target, sig) {
            return Err(EPERM);
        }
        return Ok(Delivery {
            targets: 1,
            delivered: if probe { 0 } else { 1 },
            probe,
        });
    }

    // ③ 组/全表形态：先算基数，空集是 ESRCH，全被拒是 EPERM。
    let tally = if pid == 0 {
        group_tally(space, caller, caller.pgid, sig)
    } else if pid == -1 {
        all_tally(space, caller, sig)
    } else {
        let pgid = match pid.checked_neg() {
            Some(g) => g,
            None => return Err(ESRCH),
        };
        if pgid <= 1 {
            // -pid 组号 <= 1 只可能是 init 组，按空组处理。
            (0u32, 0u32)
        } else {
            group_tally(space, caller, pgid, sig)
        }
    };
    let (candidates, allowed) = tally;
    if candidates == 0 {
        return Err(ESRCH);
    }
    if allowed == 0 {
        return Err(EPERM);
    }
    Ok(Delivery {
        targets: allowed,
        delivered: if probe { 0 } else { allowed },
        probe,
    })
}

/// 判定次序等级：EINVAL=0、ESRCH=1、EPERM=2、其它=-1。
pub fn order_rank(err: i32) -> i32 {
    if err == EINVAL {
        0
    } else if err == ESRCH {
        1
    } else if err == EPERM {
        2
    } else {
        -1
    }
}

/// 提取错误的 errno；成功回 0（避免在判据里写 unwrap 分支）。
pub fn err_of<T>(r: Result<T, i32>) -> i32 {
    match r {
        Ok(_) => 0,
        Err(e) => e,
    }
}

// ── 语料空间 ──────────────────────────────────────────────────────────
/// 标准语料：init + root 守护 + 本组三员 + 外 uid + 别组 + 僵尸。
///
/// 槽位规划（顺序即发布序，gen 从 1 起）：
///   1 init(uid0,pgid1) / 2 root(uid0,pgid2) / 100 101 102(uid1000,pgid100)
///   200(uid2000,pgid200) / 300(uid1000,pgid300) / 301(uid1000,pgid301,僵尸)
pub fn fixture_space() -> KillSpace {
    let mut sp = KillSpace::new();
    let mut ok = true;
    if sp.spawn(INIT_PID, 1, ROOT_UID, true).is_none() {
        ok = false;
    }
    if sp.spawn(2, 2, ROOT_UID, false).is_none() {
        ok = false;
    }
    if sp.spawn(100, 100, CALLER_UID, false).is_none() {
        ok = false;
    }
    if sp.spawn(101, 100, CALLER_UID, false).is_none() {
        ok = false;
    }
    if sp.spawn(102, 100, CALLER_UID, false).is_none() {
        ok = false;
    }
    if sp.spawn(200, 200, 2000, false).is_none() {
        ok = false;
    }
    if sp.spawn(300, 300, CALLER_UID, false).is_none() {
        ok = false;
    }
    if sp.spawn(301, 301, CALLER_UID, false).is_none() {
        ok = false;
    }
    if !ok {
        // 语料构造失败时保持空表，判据会显性报红而不是静默半表。
        return KillSpace::new();
    }
    // 301 固定为僵尸：僵尸必须仍可寻址。
    if let Some(idx) = sp.addr(301) {
        if let Some(t) = sp.targets.get_mut(idx) {
            t.life = Life::Zombie;
        }
    }
    sp
}

// ── 注入账本 ──────────────────────────────────────────────────────────
/// 注入账：injected = detected + escaped，逃逸必须为 0。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectLedger {
    pub injected: u32,
    pub detected: u32,
    pub escaped: u32,
}

impl InjectLedger {
    /// 空账。
    pub const fn zero() -> InjectLedger {
        InjectLedger {
            injected: 0,
            detected: 0,
            escaped: 0,
        }
    }

    /// 记一笔注入（只增不减）。
    pub fn record(&mut self, caught: bool) {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
    }

    /// 守恒：检出 + 逃逸 == 注入。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

// ── 冻结矩阵 ──────────────────────────────────────────────────────────
/// 一格：输入 + 期望。`want_err == 0` 表示期望成功。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct KillCell {
    pub pid: i32,
    pub sig: i32,
    pub uid: u32,
    pub caps: u32,
    pub want_err: i32,
    pub targets: u32,
    pub delivered: u32,
}

/// 十二格冻结矩阵（每格独立语料，互不影响）。
///
/// 设计覆盖：init 保护 2 格（外来 KILL / root 探针）、坏信号 2 格（含次序格）、
/// 缺目标 2 格、探针 1 格、正常投递 1 格、跨 uid 1 格、组三形态 3 格。
pub const KILL_GRID: [KillCell; GRID_CELLS] = [
    // ① init + SIGKILL（外来 uid）：EPERM 特例格。
    KillCell { pid: 1, sig: 9, uid: CALLER_UID, caps: 0, want_err: EPERM, targets: 0, delivered: 0 },
    // ② init + 探针（root + CAP_KILL）：探针成功但不投递。
    KillCell { pid: 1, sig: 0, uid: ROOT_UID, caps: CAP_KILL, want_err: 0, targets: 1, delivered: 0 },
    // ③ 坏信号（活体目标仍在，但参数面先拒）。
    KillCell { pid: CALLER_PID, sig: 65, uid: CALLER_UID, caps: 0, want_err: EINVAL, targets: 0, delivered: 0 },
    // ④ 坏信号 + 不存在的 pid：次序格，必须 EINVAL 而非 ESRCH。
    KillCell { pid: 999, sig: 65, uid: CALLER_UID, caps: 0, want_err: EINVAL, targets: 0, delivered: 0 },
    // ⑤ 不存在的 pid + 合法信号：ESRCH。
    KillCell { pid: 999, sig: 15, uid: CALLER_UID, caps: 0, want_err: ESRCH, targets: 0, delivered: 0 },
    // ⑥ 探针：存在且同 uid。
    KillCell { pid: 101, sig: 0, uid: CALLER_UID, caps: 0, want_err: 0, targets: 1, delivered: 0 },
    // ⑦ 正常投递。
    KillCell { pid: 101, sig: 15, uid: CALLER_UID, caps: 0, want_err: 0, targets: 1, delivered: 1 },
    // ⑧ 跨 uid：EPERM。
    KillCell { pid: 200, sig: 15, uid: CALLER_UID, caps: 0, want_err: EPERM, targets: 0, delivered: 0 },
    // ⑨ pid=0：本组三员。
    KillCell { pid: 0, sig: 15, uid: CALLER_UID, caps: 0, want_err: 0, targets: 3, delivered: 3 },
    // ⑩ pid=-300：别组一员。
    KillCell { pid: -300, sig: 15, uid: CALLER_UID, caps: 0, want_err: 0, targets: 1, delivered: 1 },
    // ⑪ 空组：ESRCH。
    KillCell { pid: -777, sig: 15, uid: CALLER_UID, caps: 0, want_err: ESRCH, targets: 0, delivered: 0 },
    // ⑫ kill(-1)：7 个候选里 5 个同 uid 可投，部分投递成功。
    KillCell { pid: -1, sig: 9, uid: CALLER_UID, caps: 0, want_err: 0, targets: 5, delivered: 5 },
];

/// 复算单格：返回结果是否符合期望。
pub fn cell_matches(space: &KillSpace, cell: &KillCell) -> bool {
    let ctx = CallerCtx {
        pid: CALLER_PID,
        uid: cell.uid,
        caps: cell.caps,
        pgid: CALLER_PGID,
    };
    match do_kill(space, &ctx, cell.pid, cell.sig) {
        Ok(d) => {
            cell.want_err == 0
                && d.targets == cell.targets
                && d.delivered == cell.delivered
                && d.probe == (cell.sig == 0)
        }
        Err(e) => e == cell.want_err,
    }
}

/// 十二格 × 十轮注入：返回逐格命中数与账本。
pub fn run_kill_matrix() -> ([u32; GRID_CELLS], InjectLedger) {
    let mut per_cell = [0u32; GRID_CELLS];
    let mut ledger = InjectLedger::zero();
    let mut ci = 0usize;
    while ci < GRID_CELLS {
        if let Some(cell) = KILL_GRID.get(ci).copied() {
            let mut round = 0u32;
            while round < INJECT_ROUNDS {
                let space = fixture_space();
                let caught = cell_matches(&space, &cell);
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

// ── errno 对照表 ─────────────────────────────────────────────────────
/// (名称, Linux 正值, 说明)。本模块只允许出现这三个码。
pub const ERRNO_TABLE: &[(&str, i32, &str)] = &[
    ("EPERM", 1, "操作不被允许：目标存在但调用者无 CAP_KILL 且 uid 不同"),
    ("ESRCH", 3, "没有匹配进程：pid 选择器解析为空"),
    ("EINVAL", 22, "参数非法：信号号不在 0..=64"),
];

/// 表内查码：命中返回正值，表外返回 0。
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

/// 码值是否与 Linux 数值一致。
pub fn agrees_with_linux(code: i32) -> bool {
    code == errno_value("EPERM") || code == errno_value("ESRCH") || code == errno_value("EINVAL")
}

// ── 确定性指纹 ────────────────────────────────────────────────────────
/// FNV-1a 32 位（自写，无浮点/随机/墙钟）。
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

/// 把一个 kill 结果编码进字节流（成功/失败两种标签，长度固定）。
pub fn encode_result(out: &mut Vec<u8>, r: Result<Delivery, i32>) {
    match r {
        Ok(d) => {
            out.push(1);
            out.push((d.targets & 0xFF) as u8);
            out.push((d.delivered & 0xFF) as u8);
            out.push(if d.probe { 1 } else { 0 });
        }
        Err(e) => {
            out.push(2);
            out.push((e & 0x7F) as u8);
            out.push(0);
            out.push(0);
        }
    }
}

/// 规范语料指纹：四条选择器形态 + 探针 + 坏信号 + 跨 uid 各走一遍。
pub fn corpus_fingerprint() -> u32 {
    let space = fixture_space();
    let mut bytes: Vec<u8> = Vec::new();
    encode_result(&mut bytes, do_kill(&space, &CALLER, 101, 0));
    encode_result(&mut bytes, do_kill(&space, &CALLER, 101, 15));
    encode_result(&mut bytes, do_kill(&space, &CALLER, 0, 15));
    encode_result(&mut bytes, do_kill(&space, &CALLER, -300, 15));
    encode_result(&mut bytes, do_kill(&space, &CALLER, -1, 9));
    encode_result(&mut bytes, do_kill(&space, &CALLER, 200, 15));
    encode_result(&mut bytes, do_kill(&space, &CALLER, 999, 15));
    encode_result(&mut bytes, do_kill(&space, &CALLER, 999, 65));
    encode_result(&mut bytes, do_kill(&space, &CALLER, 301, 9));
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9282 判据集（add ≥12）。
pub fn run_unx9282_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9282");

    let seg_ok = SEG_BASE % 0x40 == 0
        && SEG_BASE >= WINDOW_LO
        && SEG_BASE <= WINDOW_HI
        && SEG_BASE + 0x3F < 0x7B440;
    s.add(
        "段位守卫：0x40 对齐且独占 0x7B400..=0x7B43F",
        seg_ok,
        "段基址必须落在本批窗口内且不与 F9283 的 0x7B440 窗口重叠",
    );

    // 信号域全域扫描：-2..=70 每一个整数都必须落进正确分支。
    let space = fixture_space();
    let mut sig_domain_ok = true;
    let mut sig = -2i32;
    while sig <= 70 {
        let got = err_of(do_kill(&space, &CALLER, 999, sig));
        let want = if sig >= 0 && sig <= SIGRTMAX {
            ESRCH
        } else {
            EINVAL
        };
        if got != want {
            sig_domain_ok = false;
        }
        sig += 1;
    }
    s.add(
        "参数面：-2..=70 全域扫描，非法 sig 全 EINVAL",
        sig_domain_ok,
        "0..=64 合法（缺目标回 ESRCH），其余含负数与 65 以上必须 EINVAL",
    );

    // 非法信号对活体目标也必须是 EINVAL，而不是 EPERM/成功。
    let bad_sigs = [-1i32, -9, 65, 100, 255, i32::MIN];
    let mut bad_ok = true;
    let mut bi = 0usize;
    while bi < bad_sigs.len() {
        if let Some(bad) = bad_sigs.get(bi).copied() {
            if err_of(do_kill(&space, &CALLER, 101, bad)) != EINVAL {
                bad_ok = false;
            }
            if err_of(do_kill(&space, &CALLER, 1, bad)) != EINVAL {
                bad_ok = false;
            }
        }
        bi += 1;
    }
    s.add(
        "参数面：坏 sig 对活体与 init 均 EINVAL",
        bad_ok,
        "参数面必须先于寻址面与权限面，坏 sig 不得被 init 保护格截成 EPERM",
    );

    // 探针语义：sig=0 成功但 delivered==0。
    let probe_101 = do_kill(&space, &CALLER, 101, 0);
    let probe_ok = match probe_101 {
        Ok(d) => d.probe && d.targets == 1 && d.delivered == 0,
        Err(_) => false,
    };
    s.add(
        "探针：sig=0 返回存在性且不投递",
        probe_ok,
        "探针成功必须 delivered==0、targets>0，否则 kill(pid,0) 会变成静默信号",
    );

    // pid=1 特例：外来 uid 全 EPERM（含探针）。
    let init_sigs = [0i32, 1, 9, 15, 64];
    let mut init_ok = true;
    let mut ii = 0usize;
    while ii < init_sigs.len() {
        if let Some(sig_v) = init_sigs.get(ii).copied() {
            if err_of(do_kill(&space, &CALLER, INIT_PID, sig_v)) != EPERM {
                init_ok = false;
            }
        }
        ii += 1;
    }
    s.add(
        "pid=1 特例：外来 uid 全 EPERM（含探针）",
        init_ok,
        "init 属 root，普通 uid 连探针都无权，必须 EPERM 而不是 ESRCH",
    );

    // pid=1 的不可捕获信号：即便 root + CAP_KILL 也拒绝。
    let root = CallerCtx {
        pid: CALLER_PID,
        uid: ROOT_UID,
        caps: CAP_KILL | CAP_SYS_ADMIN,
        pgid: CALLER_PGID,
    };
    let init_kill_ok = err_of(do_kill(&space, &root, INIT_PID, SIGKILL)) == EPERM
        && err_of(do_kill(&space, &root, INIT_PID, SIGSTOP)) == EPERM
        && err_of(do_kill(&space, &root, INIT_PID, 15)) == 0;
    s.add(
        "pid=1 特例：KILL/STOP 不受 CAP_KILL 放宽",
        init_kill_ok,
        "root 可投 SIGTERM 但 SIGKILL/SIGSTOP 仍 EPERM，保护 init 不被停止",
    );

    // 不存在 pid 全 ESRCH：多 pid × 多信号。
    let absent_pids = [999i32, 12345, 32000, 65535, i32::MAX];
    let sig_set = [0i32, 1, 9, 15, 32, 64];
    let mut absent_ok = true;
    let mut ai = 0usize;
    while ai < absent_pids.len() {
        if let Some(pid) = absent_pids.get(ai).copied() {
            let mut si = 0usize;
            while si < sig_set.len() {
                if let Some(sig_v) = sig_set.get(si).copied() {
                    if err_of(do_kill(&space, &CALLER, pid, sig_v)) != ESRCH {
                        absent_ok = false;
                    }
                }
                si += 1;
            }
        }
        ai += 1;
    }
    s.add(
        "寻址面：不存在 pid 全 ESRCH（5 pid × 6 sig）",
        absent_ok,
        "缺目标必须在选定的 30 个组合上稳定回 ESRCH，不得混入 EPERM",
    );

    // 次序：EINVAL(0) < ESRCH(1) < EPERM(2)，等级严格递增。
    let rank_param = order_rank(err_of(do_kill(&space, &CALLER, 999, 99)));
    let rank_addr = order_rank(err_of(do_kill(&space, &CALLER, 999, 15)));
    let rank_perm = order_rank(err_of(do_kill(&space, &CALLER, 200, 15)));
    let order_ok = rank_param == 0 && rank_addr == 1 && rank_perm == 2;
    s.add(
        "判定次序：EINVAL → ESRCH → EPERM 三级递增",
        order_ok,
        "三个面各有独立触发条件，等级必须 0<1<2，互换即掩盖真实原因",
    );

    // 组选择器：三种组形态的基数。
    let g0 = do_kill(&space, &CALLER, 0, 15);
    let g100 = do_kill(&space, &CALLER, -100, 15);
    let g300 = do_kill(&space, &CALLER, -300, 15);
    let group_ok = matches!(g0, Ok(d) if d.targets == 3 && d.delivered == 3)
        && matches!(g100, Ok(d) if d.targets == 3)
        && matches!(g300, Ok(d) if d.targets == 1);
    s.add(
        "组选择器：pid=0 与 -pgid 基数正确",
        group_ok,
        "本组三员、别组一员必须逐格对上，组形态不得退化成单体",
    );

    // 空组必须 ESRCH；-1 组在语料里不存在。
    let empty_ok = err_of(do_kill(&space, &CALLER, -777, 15)) == ESRCH
        && err_of(do_kill(&space, &CALLER, -1000, 15)) == ESRCH;
    s.add(
        "组选择器：空组 ESRCH",
        empty_ok,
        "组成员基数为 0 时必须 ESRCH，不能被当成成功投递 0 个",
    );

    // kill(-1)：7 候选 5 可投（root 守护与外 uid 被跳过）。
    let all = do_kill(&space, &CALLER, -1, 9);
    let all_ok = match all {
        Ok(d) => d.targets == 5 && d.delivered == 5 && !d.probe,
        Err(_) => false,
    };
    s.add(
        "kill(-1)：部分投递按可投子集计数",
        all_ok,
        "7 个非 init 目标里 5 个同 uid，必须成功且 targets==5，被拒目标静默跳过",
    );

    // 僵尸可寻址、Dead 不可寻址。
    let zombie_ok = err_of(do_kill(&space, &CALLER, 301, 9)) == 0;
    let mut pruned = fixture_space();
    let reaped = pruned.reap(101);
    let dead_ok = reaped && err_of(do_kill(&pruned, &CALLER, 101, 15)) == ESRCH;
    s.add(
        "状态机：僵尸可寻址、已回收不可寻址",
        zombie_ok && dead_ok,
        "僵尸投递算成功（信号被丢弃），Dead 槽必须对寻址完全不可见",
    );

    // generation 守卫：回收后复用同一 pid，旧 gen 必须失效。
    let mut reuse = fixture_space();
    let old_gen = match reuse.peek(101) {
        Some(t) => t.gen,
        None => 0,
    };
    let reaped2 = reuse.reap(101);
    let respawned = reuse.spawn(101, CALLER_PGID, CALLER_UID, false).is_some();
    let stale_ok = reaped2
        && respawned
        && reuse.addr_gen(101, old_gen).is_none()
        && match reuse.peek(101) {
            Some(t) => reuse.addr_gen(101, t.gen).is_some() && t.gen != old_gen,
            None => false,
        };
    s.add(
        "generation：pid 复用后旧句柄 ESRCH",
        stale_ok,
        "旧 generation 不得命中新进程，否则一次延迟投递会杀到无关进程",
    );

    // 注入矩阵：12 格 × 10 轮。
    let (per_cell, ledger) = run_kill_matrix();
    let mut cells_all = true;
    let mut ci = 0usize;
    while ci < GRID_CELLS {
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
        "注入矩阵：12 格各 10/10 命中",
        cells_all,
        "三错误码 + init 特例 + 探针 + 组三形态必须逐格复算一致",
    );
    s.add(
        "注入账：120 次全检出且逃逸为 0",
        ledger.conserved()
            && ledger.escaped == 0
            && ledger.injected == GRID_CELLS as u32 * INJECT_ROUNDS,
        "injected == detected + escaped，逃逸非零即判定器漏格",
    );

    // errno 数值与 Linux 一致。
    let errno_ok = errno_value("EPERM") == 1
        && errno_value("ESRCH") == 3
        && errno_value("EINVAL") == 22
        && agrees_with_linux(EPERM)
        && agrees_with_linux(ESRCH)
        && agrees_with_linux(EINVAL)
        && !agrees_with_linux(0);
    s.add(
        "errno：EPERM/ESRCH/EINVAL 与 Linux 数值一致",
        errno_ok,
        "错误码只允许 1/3/22，表外码一律拒绝",
    );

    // 双跑指纹。
    let fp_a = corpus_fingerprint();
    let fp_b = corpus_fingerprint();
    let fp_ok = fp_a == fp_b && fp_a != 0;
    s.add(
        "双跑一致：规范语料指纹相等",
        fp_ok,
        "同输入两次必须逐字节相等，隐藏状态即失败",
    );

    // 诊断码闭集：12 个、码值唯一、文案非空。
    let mut uniq = true;
    let mut i = 0usize;
    while i < KillCode::ALL.len() {
        let mut j = i + 1;
        while j < KillCode::ALL.len() {
            if let (Some(a), Some(b)) = (KillCode::ALL.get(i), KillCode::ALL.get(j)) {
                if a.val() == b.val() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    let text_ok = KillCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add(
        "诊断码：12 个码值两两唯一且文案非空",
        uniq && text_ok && KillCode::ALL.len() == 12,
        "闭集码不得重复，且每码都要能取到可操作的中文建议",
    );

    // 账目单调：再记 5 笔只增不减、逃逸仍为 0。
    let mut mono = ledger;
    let before = mono.injected;
    let mut k = 0u32;
    while k < 5 {
        mono.record(true);
        k += 1;
    }
    s.add(
        "账目单调：追加 5 笔只增不减",
        mono.injected == before.saturating_add(5)
            && mono.escaped == 0
            && mono.conserved(),
        "累计计数禁止回退或饱和到旧值",
    );

    // 动态快照。
    let matrix_text = format!(
        "注入 {} / 检出 {} / 逃逸 {}",
        ledger.injected, ledger.detected, ledger.escaped
    );
    s.add_dyn("矩阵账目", ledger.conserved(), &matrix_text);

    let init_text = format!(
        "init: 外来探针={} root-KILL={}",
        err_of(do_kill(&space, &CALLER, INIT_PID, 0)),
        err_of(do_kill(&space, &root, INIT_PID, SIGKILL))
    );
    s.add_dyn("init 特例快照", init_ok && init_kill_ok, &init_text);

    let fp_text = format!("语料指纹 0x{:08X}", fp_a);
    s.add_dyn("指纹快照", fp_ok, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signal_domain_is_total() {
        assert_eq!(sig_kind(0), SigKind::Probe);
        assert_eq!(sig_kind(1), SigKind::Standard);
        assert_eq!(sig_kind(31), SigKind::Standard);
        assert_eq!(sig_kind(32), SigKind::Realtime);
        assert_eq!(sig_kind(64), SigKind::Realtime);
        assert_eq!(sig_kind(65), SigKind::Invalid);
        assert_eq!(sig_kind(-1), SigKind::Invalid);
        assert_eq!(sig_ok(0), true);
        assert_eq!(sig_ok(64), true);
        assert_eq!(sig_ok(65), false);
        assert_eq!(sig_name(9), "SIGKILL");
        assert_eq!(sig_name(0), "probe-0");
        assert_eq!(sig_name(77), "<unnamed>");
    }

    #[test]
    fn init_special_cells() {
        let sp = fixture_space();
        assert_eq!(do_kill(&sp, &CALLER, INIT_PID, 9), Err(EPERM));
        assert_eq!(do_kill(&sp, &CALLER, INIT_PID, 0), Err(EPERM));
        let root = CallerCtx {
            pid: CALLER_PID,
            uid: ROOT_UID,
            caps: CAP_KILL,
            pgid: CALLER_PGID,
        };
        assert_eq!(do_kill(&sp, &root, INIT_PID, 9), Err(EPERM));
        assert_eq!(
            do_kill(&sp, &root, INIT_PID, 15),
            Ok(Delivery {
                targets: 1,
                delivered: 1,
                probe: false
            })
        );
    }

    #[test]
    fn absent_pids_are_esrch() {
        let sp = fixture_space();
        let pids = [999i32, 12345, 32000, i32::MAX];
        let mut i = 0usize;
        while i < pids.len() {
            if let Some(pid) = pids.get(i).copied() {
                assert_eq!(do_kill(&sp, &CALLER, pid, 0), Err(ESRCH));
                assert_eq!(do_kill(&sp, &CALLER, pid, 15), Err(ESRCH));
            }
            i += 1;
        }
    }

    #[test]
    fn probe_delivers_nothing() {
        let sp = fixture_space();
        match do_kill(&sp, &CALLER, 101, 0) {
            Ok(d) => {
                assert_eq!(d.probe, true);
                assert_eq!(d.targets, 1);
                assert_eq!(d.delivered, 0);
            }
            Err(_) => assert_eq!(true, false),
        }
    }

    #[test]
    fn group_selectors_tally() {
        let sp = fixture_space();
        assert_eq!(err_of(do_kill(&sp, &CALLER, 0, 15)), 0);
        assert_eq!(err_of(do_kill(&sp, &CALLER, -777, 15)), ESRCH);
        match do_kill(&sp, &CALLER, 0, 15) {
            Ok(d) => assert_eq!(d.targets, 3),
            Err(_) => assert_eq!(true, false),
        }
        match do_kill(&sp, &CALLER, -300, 15) {
            Ok(d) => assert_eq!(d.targets, 1),
            Err(_) => assert_eq!(true, false),
        }
    }

    #[test]
    fn kill_minus_one_is_partial() {
        let sp = fixture_space();
        match do_kill(&sp, &CALLER, -1, 9) {
            Ok(d) => {
                assert_eq!(d.targets, 5);
                assert_eq!(d.delivered, 5);
            }
            Err(_) => assert_eq!(true, false),
        }
    }

    #[test]
    fn zombie_visible_dead_hidden() {
        let sp = fixture_space();
        assert_eq!(err_of(do_kill(&sp, &CALLER, 301, 9)), 0);
        let mut pruned = fixture_space();
        assert_eq!(pruned.reap(101), true);
        assert_eq!(do_kill(&pruned, &CALLER, 101, 15), Err(ESRCH));
        assert_eq!(pruned.count(), 7);
    }

    #[test]
    fn generation_guard_rejects_stale() {
        let mut sp = fixture_space();
        let old_gen = match sp.peek(101) {
            Some(t) => t.gen,
            None => 0,
        };
        assert_eq!(sp.reap(101), true);
        assert_eq!(sp.spawn(101, CALLER_PGID, CALLER_UID, false).is_some(), true);
        assert_eq!(sp.addr_gen(101, old_gen).is_none(), true);
        let new_gen = match sp.peek(101) {
            Some(t) => t.gen,
            None => 0,
        };
        assert_eq!(new_gen == old_gen, false);
        assert_eq!(sp.addr_gen(101, new_gen).is_some(), true);
    }

    #[test]
    fn order_is_param_addr_perm() {
        let sp = fixture_space();
        assert_eq!(order_rank(err_of(do_kill(&sp, &CALLER, 999, 99))), 0);
        assert_eq!(order_rank(err_of(do_kill(&sp, &CALLER, 999, 15))), 1);
        assert_eq!(order_rank(err_of(do_kill(&sp, &CALLER, 200, 15))), 2);
    }

    #[test]
    fn matrix_is_conserved() {
        let (per, l) = run_kill_matrix();
        assert_eq!(l.conserved(), true);
        assert_eq!(l.escaped, 0);
        assert_eq!(l.injected, 120);
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
    fn fingerprint_is_stable() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
        assert_eq!(corpus_fingerprint() != 0, true);
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx9282_checks();
        assert_eq!(s.all_passed(), true);
        assert_eq!(s.dyn_dropped(), 0);
    }
}
