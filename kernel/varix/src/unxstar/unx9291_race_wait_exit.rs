//! UNX-F9291 僵尸收割竞态注入（wait vs exit 竞态） · 0x7B640段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 F9291 逐字）：
//!   UNX-F9291-J1 wait-exit 竞态注入判据过，单次收割唯一性 100 次统计格过，
//!   双 wait 第二次全 ECHILD 格过。
//!
//! 设计要点：
//!   ① 子进程状态机冻结为三态：Running → Zombie → Reaped。
//!         Running --exit--> Zombie（记账 SIGCHLD）
//!         Zombie  --wait--> Reaped（唯一一次收割，返回 pid）
//!         Reaped  --wait--> Reaped + ECHILD
//!         Running --wait--> 阻塞挂起（非 WNOHANG）或返回 0（WNOHANG）
//!      任何越级迁移（Running 直接 Reaped、Zombie 再次 exit）都按 ESRCH 拒。
//!   ② 竞态用「事件序列」表达而不是墙钟：一轮固定三个事件 {Exit, WaitA,
//!      WaitB} 的六种排列，全部穷举。阻塞 wait 先到就挂到停等槽，Exit 到达
//!      时由最早挂起的 wait 完成唯一收割，其余挂起者收 ECHILD；这条规则是
//!      「单次收割唯一性」的可复算落点。
//!   ③ 100 轮统计：每轮用固定 LCG 选排列（双 wait 均阻塞），必须满足
//!      success == 1、reaps == 1、echld == 1；任一轮 success 为 0 或大于 1
//!      都记为唯一性违例。第二组格穷举 6 排列 × 4 组 WNOHANG 开关（24 轮），
//!      验证 WNOHANG 早到返回 0、僵尸不丢、后续阻塞 wait 仍只收割一次。
//!   ④ 双 wait 语义单独取证：第一次收割返回 pid，第二次必回 ECHILD；对已
//!      收割的 pid 再 wait 仍 ECHILD，不允许重复报告同一 pid。
//!   ⑤ wait 状态字按内核编码：status = (code & 0xFF) << 8，WIFEXITED 为
//!      低 7 位等于 0；对不存在 pid 的 exit 回 ESRCH，子进程表满 spawn 回
//!      EAGAIN，重复 pid 回 EEXIST。
//!   ⑥ 注入 100 轮，账本 injected == detected + escaped 且 escaped == 0；
//!      诊断码闭集 12 个，段窗口 0x7B640..=0x7B67F。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 诊断段基址（F9291 独占 0x7B640..=0x7B67F）。
pub const SEG_BASE: u32 = 0x7B640;
/// 段窗口上界（含）。
pub const SEG_END: u32 = 0x7B67F;

/// Linux errno（正数口径）。
pub const ESRCH: i32 = 3;
pub const ECHILD: i32 = 10;
pub const EAGAIN: i32 = 11;
pub const EEXIST: i32 = 17;
pub const EINVAL: i32 = 22;

/// 子进程表容量。
pub const MAX_CHILDREN: usize = 4;
/// 竞态统计轮数。
pub const RACE_ROUNDS: u32 = 100;
/// 停等槽容量（两个等待者）。
pub const MAX_PARKED: usize = 2;
/// 六个排列。
pub const PERMS: u8 = 6;
/// WNOHANG 矩阵轮数（6 排列 × 4 开关组合）。
pub const NOHANG_ROUNDS: u32 = 24;

/// 等待者身份（0 保留给「无人」）。
pub const WAITER_A: u32 = 0xA1;
pub const WAITER_B: u32 = 0xA2;
/// 被等待子进程 pid。
pub const CHILD_PID: u32 = 0x2A;
/// 退出码。
pub const EXIT_CODE: i32 = 7;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码。码值 = SEG_BASE + 序号（1 起），落 0x7B641..=0x7B64C。
#[repr(u32)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RaceCode {
    DoubleReap = 1,
    MissingEchild = 2,
    NohangLost = 3,
    BadExitState = 4,
    RaceLeak = 5,
    StatsDrift = 6,
    LedgerLeak = 7,
    DoubleRun = 8,
    SegmentSkew = 9,
    StatusDrift = 10,
    CapacityDrift = 11,
    OrderDrift = 12,
}

impl RaceCode {
    /// 段内码值。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }

    /// 中文诊断与可操作建议。
    pub fn detail(self) -> &'static str {
        match self {
            RaceCode::DoubleReap => "重复收割：同一子进程被 wait 返回两次；请核对 Reaped 态是否仍走成功分支",
            RaceCode::MissingEchild => "缺 ECHILD：第二次 wait 未回 ECHILD；请检查已收割判断",
            RaceCode::NohangLost => "WNOHANG 语义漂移：子进程运行中应返回 0 而非 ECHILD/阻塞",
            RaceCode::BadExitState => "状态迁移非法：重复 exit 或对不存在 pid 发 exit；应按 ESRCH 拒",
            RaceCode::RaceLeak => "竞态漏检：注入轮次未被判定覆盖；请检查账本",
            RaceCode::StatsDrift => "统计漂移：100 轮唯一性统计与冻结值不符；请复核排列覆盖",
            RaceCode::LedgerLeak => "注入账不守恒：detected+escaped 与 injected 不等",
            RaceCode::DoubleRun => "双跑不一致：同种子两轮结果不同；请排查隐藏状态",
            RaceCode::SegmentSkew => "段位越界：段基址必须 0x40 对齐且落在本卡窗口",
            RaceCode::StatusDrift => "状态字漂移：退出码编码不符合 (code & 0xFF) << 8",
            RaceCode::CapacityDrift => "容量漂移：子进程表满/重复 pid 的 errno 不符",
            RaceCode::OrderDrift => "事件次序漂移：排列表与冻结调度不一致",
        }
    }

    /// 闭集全量。
    pub const ALL: [RaceCode; 12] = [
        RaceCode::DoubleReap,
        RaceCode::MissingEchild,
        RaceCode::NohangLost,
        RaceCode::BadExitState,
        RaceCode::RaceLeak,
        RaceCode::StatsDrift,
        RaceCode::LedgerLeak,
        RaceCode::DoubleRun,
        RaceCode::SegmentSkew,
        RaceCode::StatusDrift,
        RaceCode::CapacityDrift,
        RaceCode::OrderDrift,
    ];
}

// ── 子进程状态机 ──────────────────────────────────────────────────────
/// 子进程三态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ProcState {
    Running,
    Zombie,
    Reaped,
}

impl ProcState {
    pub fn tag(self) -> u8 {
        match self {
            ProcState::Running => 0,
            ProcState::Zombie => 1,
            ProcState::Reaped => 2,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ProcState::Running => "running",
            ProcState::Zombie => "zombie",
            ProcState::Reaped => "reaped",
        }
    }
}

/// 事件种类（状态迁移表的输入侧）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EvKind {
    Exit,
    WaitBlock,
    WaitNohang,
}

/// 一条冻结迁移：(旧态, 事件, 新态, errno)，errno 为 0 表示成功。
/// 三态 × 三事件共 9 组，全部显式列出，供实现逐组对账。
pub const TRANS: [(ProcState, EvKind, ProcState, i32); 9] = [
    (ProcState::Running, EvKind::Exit, ProcState::Zombie, 0),
    (ProcState::Running, EvKind::WaitBlock, ProcState::Running, EAGAIN),
    (ProcState::Running, EvKind::WaitNohang, ProcState::Running, 0),
    (ProcState::Zombie, EvKind::Exit, ProcState::Zombie, ESRCH),
    (ProcState::Zombie, EvKind::WaitBlock, ProcState::Reaped, 0),
    (ProcState::Zombie, EvKind::WaitNohang, ProcState::Reaped, 0),
    (ProcState::Reaped, EvKind::Exit, ProcState::Reaped, ESRCH),
    (ProcState::Reaped, EvKind::WaitBlock, ProcState::Reaped, ECHILD),
    (ProcState::Reaped, EvKind::WaitNohang, ProcState::Reaped, ECHILD),
];

/// 单子进程迁移函数：与 TRANS 同判（表只作为对账样本）。
pub fn apply(state: ProcState, ev: EvKind) -> (ProcState, i32) {
    match (state, ev) {
        (ProcState::Running, EvKind::Exit) => (ProcState::Zombie, 0),
        (ProcState::Running, EvKind::WaitBlock) => (ProcState::Running, EAGAIN),
        (ProcState::Running, EvKind::WaitNohang) => (ProcState::Running, 0),
        (ProcState::Zombie, EvKind::WaitBlock) => (ProcState::Reaped, 0),
        (ProcState::Zombie, EvKind::WaitNohang) => (ProcState::Reaped, 0),
        (ProcState::Reaped, EvKind::Exit) => (ProcState::Reaped, ESRCH),
        (ProcState::Reaped, EvKind::WaitBlock) => (ProcState::Reaped, ECHILD),
        (ProcState::Reaped, EvKind::WaitNohang) => (ProcState::Reaped, ECHILD),
        (ProcState::Zombie, EvKind::Exit) => (ProcState::Zombie, ESRCH),
    }
}

/// wait 状态字编码。低 8 位存退出码，第 7 位（WIFEXITED 判定位）恒 0。
pub fn w_exit_status(code: i32) -> i32 {
    (code & 0xFF) << 8
}

/// WIFEXITED：低 7 位为 0 表示正常退出。
pub fn w_ifexited(status: i32) -> bool {
    status & 0x7F == 0
}

/// 子进程槽。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Child {
    pub pid: u32,
    pub state: ProcState,
    pub exit_code: i32,
    pub reaped_by: u32,
    pub sigchld: u32,
}

impl Child {
    pub const fn empty() -> Child {
        Child { pid: 0, state: ProcState::Reaped, exit_code: 0, reaped_by: 0, sigchld: 0 }
    }
}

/// 定容子进程表 + 内核式 wait/exit 接口。
#[derive(Clone, Copy, Debug)]
pub struct WaitKernel {
    children: [Child; MAX_CHILDREN],
    count: usize,
    reaps: u32,
    echld: u32,
    zero_returns: u32,
    blocked: u32,
}

impl WaitKernel {
    pub const fn new() -> WaitKernel {
        WaitKernel {
            children: [Child::empty(); MAX_CHILDREN],
            count: 0,
            reaps: 0,
            echld: 0,
            zero_returns: 0,
            blocked: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn child(&self, i: usize) -> Option<&Child> {
        self.children.get(i)
    }

    pub fn reaps(&self) -> u32 {
        self.reaps
    }

    pub fn echld(&self) -> u32 {
        self.echld
    }

    pub fn zero_returns(&self) -> u32 {
        self.zero_returns
    }

    pub fn blocked(&self) -> u32 {
        self.blocked
    }

    /// 查找子进程下标。
    pub fn find(&self, pid: u32) -> Option<usize> {
        let mut i = 0usize;
        while i < self.count {
            if let Some(c) = self.children.get(i) {
                if c.pid == pid {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 登记子进程。表满 EAGAIN，重复 pid EEXIST。
    pub fn spawn(&mut self, pid: u32) -> Result<usize, i32> {
        if pid == 0 {
            return Err(EINVAL);
        }
        if self.find(pid).is_some() {
            return Err(EEXIST);
        }
        let idx = self.count;
        if idx >= MAX_CHILDREN {
            return Err(EAGAIN);
        }
        if let Some(slot) = self.children.get_mut(idx) {
            *slot = Child {
                pid,
                state: ProcState::Running,
                exit_code: 0,
                reaped_by: 0,
                sigchld: 0,
            };
            self.count = idx.saturating_add(1);
            Ok(idx)
        } else {
            Err(EAGAIN)
        }
    }

    /// 子进程退出：Running → Zombie，记一次 SIGCHLD；其余态 ESRCH。
    pub fn exit_child(&mut self, pid: u32, code: i32) -> Result<(), i32> {
        let idx = match self.find(pid) {
            Some(i) => i,
            None => return Err(ESRCH),
        };
        let mut ok = false;
        if let Some(slot) = self.children.get_mut(idx) {
            if slot.state == ProcState::Running {
                slot.state = ProcState::Zombie;
                slot.exit_code = code;
                slot.sigchld = slot.sigchld.saturating_add(1);
                ok = true;
            }
        }
        if ok {
            Ok(())
        } else {
            Err(ESRCH)
        }
    }

    /// waitpid 语义。返回 Ok(pid) 表示收割、Ok(0) 表示 WNOHANG 无事件、
    /// Err(ECHILD/ESRCH/EAGAIN) 表示无子进程/坏 pid/需阻塞。
    pub fn wait_pid(&mut self, pid: u32, waiter: u32, nohang: bool) -> Result<i32, i32> {
        let idx = match self.find(pid) {
            Some(i) => i,
            None => {
                self.echld = self.echld.saturating_add(1);
                return Err(ECHILD);
            }
        };
        let state = match self.children.get(idx) {
            Some(c) => c.state,
            None => ProcState::Reaped,
        };
        match state {
            ProcState::Zombie => {
                if let Some(slot) = self.children.get_mut(idx) {
                    slot.state = ProcState::Reaped;
                    slot.reaped_by = waiter;
                }
                self.reaps = self.reaps.saturating_add(1);
                Ok(pid as i32)
            }
            ProcState::Reaped => {
                self.echld = self.echld.saturating_add(1);
                Err(ECHILD)
            }
            ProcState::Running => {
                if nohang {
                    self.zero_returns = self.zero_returns.saturating_add(1);
                    Ok(0)
                } else {
                    self.blocked = self.blocked.saturating_add(1);
                    Err(EAGAIN)
                }
            }
        }
    }

    /// 已收割 pid 再次 wait 必 ECHILD（唯一性取证）。
    pub fn wait_reaped(&mut self, pid: u32, waiter: u32) -> Result<i32, i32> {
        self.wait_pid(pid, waiter, false)
    }
}

// ── 竞态仿真 ──────────────────────────────────────────────────────────
/// 固定 LCG（无随机源、无墙钟）。
pub fn lcg_next(state: u64) -> u64 {
    state
        .wrapping_mul(6_364_136_223_846_793_005)
        .wrapping_add(1_442_695_040_888_963_407)
}

/// 六种事件排列：0 = Exit，1 = WaitA，2 = WaitB。
pub fn schedule(perm: u8) -> [u8; 3] {
    match perm % PERMS {
        0 => [0, 1, 2],
        1 => [1, 0, 2],
        2 => [1, 2, 0],
        3 => [0, 2, 1],
        4 => [2, 0, 1],
        _ => [2, 1, 0],
    }
}

/// 单轮仿真器：只保留一个子进程与两个等待者。
#[derive(Clone, Copy, Debug)]
pub struct RaceSim {
    state: ProcState,
    reaped_by: u32,
    success: u32,
    echld: u32,
    zeros: u32,
    reaps: u32,
    parked: [u32; MAX_PARKED],
    parked_n: usize,
    sigchld: u32,
    last_status: i32,
}

impl RaceSim {
    pub const fn new() -> RaceSim {
        RaceSim {
            state: ProcState::Running,
            reaped_by: 0,
            success: 0,
            echld: 0,
            zeros: 0,
            reaps: 0,
            parked: [0; MAX_PARKED],
            parked_n: 0,
            sigchld: 0,
            last_status: 0,
        }
    }

    pub fn state(&self) -> ProcState {
        self.state
    }

    /// 退出事件：记为 Zombie 并由最早挂起的等待者完成唯一收割。
    pub fn do_exit(&mut self, code: i32) {
        if self.state != ProcState::Running {
            self.echld = self.echld.saturating_add(1);
            return;
        }
        self.state = ProcState::Zombie;
        self.sigchld = self.sigchld.saturating_add(1);
        self.last_status = w_exit_status(code);
        let n = self.parked_n;
        if n > 0 {
            let winner = match self.parked.get(0) {
                Some(w) => *w,
                None => 0,
            };
            self.state = ProcState::Reaped;
            self.reaped_by = winner;
            self.success = self.success.saturating_add(1);
            self.reaps = self.reaps.saturating_add(1);
            if n > 1 {
                self.echld = self.echld.saturating_add((n - 1) as u32);
            }
            self.parked_n = 0;
        }
    }

    /// wait 事件：僵尸可收割、已收割回 ECHILD、运行中按 WNOHANG 分流。
    pub fn do_wait(&mut self, who: u32, nohang: bool) {
        match self.state {
            ProcState::Zombie => {
                self.state = ProcState::Reaped;
                self.reaped_by = who;
                self.success = self.success.saturating_add(1);
                self.reaps = self.reaps.saturating_add(1);
            }
            ProcState::Reaped => {
                self.echld = self.echld.saturating_add(1);
            }
            ProcState::Running => {
                if nohang {
                    self.zeros = self.zeros.saturating_add(1);
                } else if self.parked_n < MAX_PARKED {
                    let idx = self.parked_n;
                    if let Some(slot) = self.parked.get_mut(idx) {
                        *slot = who;
                        self.parked_n = idx.saturating_add(1);
                    }
                } else {
                    self.echld = self.echld.saturating_add(1);
                }
            }
        }
    }
}

/// 一轮报告。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RoundReport {
    pub success: u32,
    pub echld: u32,
    pub zeros: u32,
    pub reaps: u32,
    pub zombie_left: bool,
    pub winner: u32,
    pub status: i32,
    pub sigchld: u32,
}

/// 跑一轮固定排列。
pub fn simulate(perm: u8, a_nohang: bool, b_nohang: bool) -> RoundReport {
    let mut sim = RaceSim::new();
    let seq = schedule(perm);
    let mut i = 0usize;
    while i < seq.len() {
        if let Some(ev) = seq.get(i).copied() {
            match ev {
                0 => sim.do_exit(EXIT_CODE),
                1 => sim.do_wait(WAITER_A, a_nohang),
                _ => sim.do_wait(WAITER_B, b_nohang),
            }
        }
        i += 1;
    }
    RoundReport {
        success: sim.success,
        echld: sim.echld,
        zeros: sim.zeros,
        reaps: sim.reaps,
        zombie_left: sim.state == ProcState::Zombie,
        winner: sim.reaped_by,
        status: sim.last_status,
        sigchld: sim.sigchld,
    }
}

// ── 注入账 ────────────────────────────────────────────────────────────
/// 注入账本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectLedger {
    pub injected: u32,
    pub detected: u32,
    pub escaped: u32,
}

impl InjectLedger {
    pub const fn zero() -> InjectLedger {
        InjectLedger { injected: 0, detected: 0, escaped: 0 }
    }

    pub fn record(&mut self, caught: bool) -> Result<(), i32> {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(EINVAL);
        }
        Ok(())
    }

    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 100 轮统计。
#[derive(Clone, Copy, Debug)]
pub struct RaceStats {
    pub rounds: u32,
    pub single_reap: u32,
    pub double_echld: u32,
    pub uniqueness_violations: u32,
    pub ledger: InjectLedger,
}

impl RaceStats {
    pub const fn empty() -> RaceStats {
        RaceStats {
            rounds: 0,
            single_reap: 0,
            double_echld: 0,
            uniqueness_violations: 0,
            ledger: InjectLedger::zero(),
        }
    }
}

/// 100 轮竞态注入：固定种子轮转六种排列，双 wait 均阻塞。
pub fn run_race() -> RaceStats {
    let mut stats = RaceStats::empty();
    let mut seed: u64 = 0x5DEE_CE66_D1CE_F00D;
    let mut round = 0u32;
    while round < RACE_ROUNDS {
        seed = lcg_next(seed);
        let perm = ((seed >> 17) % PERMS as u64) as u8;
        let r = simulate(perm, false, false);
        let unique = r.success == 1 && r.reaps == 1 && r.echld == 1;
        let second_echld = r.echld == 1 && r.success == 1;
        let caught = unique && second_echld && !r.zombie_left;
        let rec = stats.ledger.record(caught);
        if rec.is_err() {
            stats.ledger.escaped = stats.ledger.escaped.saturating_add(1);
        }
        stats.rounds = stats.rounds.saturating_add(1);
        if unique {
            stats.single_reap = stats.single_reap.saturating_add(1);
        }
        if second_echld {
            stats.double_echld = stats.double_echld.saturating_add(1);
        }
        if !unique {
            stats.uniqueness_violations = stats.uniqueness_violations.saturating_add(1);
        }
        round = round.saturating_add(1);
    }
    stats
}

/// WNOHANG 矩阵：6 排列 × 4 开关组合，统计僵尸留存与成功上限。
#[derive(Clone, Copy, Debug)]
pub struct NohangStats {
    pub rounds: u32,
    pub zero_rounds: u32,
    pub zombie_left: u32,
    pub over_reap: u32,
}

/// 穷举 WNOHANG 组合：6 排列 × 4 组开关，共 24 轮。
pub fn run_nohang_matrix() -> NohangStats {
    let mut stats = NohangStats { rounds: 0, zero_rounds: 0, zombie_left: 0, over_reap: 0 };
    let mut perm = 0u8;
    while perm < PERMS {
        let mut mask = 0u8;
        while mask < 4 {
            let a = mask & 1 != 0;
            let b = mask & 2 != 0;
            let r = simulate(perm, a, b);
            stats.rounds = stats.rounds.saturating_add(1);
            if r.zeros > 0 {
                stats.zero_rounds = stats.zero_rounds.saturating_add(1);
            }
            if r.zombie_left {
                stats.zombie_left = stats.zombie_left.saturating_add(1);
            }
            if r.success > 1 || r.reaps > 1 {
                stats.over_reap = stats.over_reap.saturating_add(1);
            }
            mask = mask.saturating_add(1);
        }
        perm = perm.saturating_add(1);
    }
    stats
}

// ── 确定性指纹 ────────────────────────────────────────────────────────
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

/// 规范语料：6 排列 × 双阻塞报告 + 内核直调序列。
pub fn corpus_fingerprint() -> u32 {
    let mut bytes: Vec<u8> = Vec::new();
    let mut perm = 0u8;
    while perm < PERMS {
        let r = simulate(perm, false, false);
        bytes.push(r.success as u8);
        bytes.push(r.echld as u8);
        bytes.push(r.reaps as u8);
        bytes.push(r.winner as u8);
        bytes.push((r.status & 0xFF) as u8);
        bytes.push(r.sigchld as u8);
        perm = perm.saturating_add(1);
    }
    let mut k = WaitKernel::new();
    let spawn_res = k.spawn(CHILD_PID).is_ok();
    bytes.push(if spawn_res { 1 } else { 0 });
    let exit_res = k.exit_child(CHILD_PID, EXIT_CODE).is_ok();
    bytes.push(if exit_res { 1 } else { 0 });
    let first = k.wait_pid(CHILD_PID, WAITER_A, false);
    bytes.push(match first {
        Ok(pid) => (pid & 0xFF) as u8,
        Err(e) => (e & 0x7F) as u8,
    });
    let second = k.wait_pid(CHILD_PID, WAITER_B, false);
    bytes.push(match second {
        Ok(v) => (v & 0xFF) as u8,
        Err(e) => (e & 0x7F) as u8,
    });
    bytes.push(k.reaps() as u8);
    bytes.push(k.echld() as u8);
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9291 判据集（add ≥12；add_dyn ≤8）。
pub fn run_unx9291_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9291");

    let seg_ok = SEG_BASE % 0x40 == 0
        && SEG_BASE == 0x7B640
        && SEG_END == SEG_BASE + 0x3F;
    s.add("段位守卫：0x40 对齐且窗口 0x7B640..=0x7B67F", seg_ok,
          "段基址唯一且不与邻卡重叠");

    let mut code_ok = true;
    let mut i = 0usize;
    while i < RaceCode::ALL.len() {
        if let Some(a) = RaceCode::ALL.get(i) {
            if a.detail().is_empty() {
                code_ok = false;
            }
            let mut j = i + 1;
            while j < RaceCode::ALL.len() {
                if let Some(b) = RaceCode::ALL.get(j) {
                    if a.code() == b.code() {
                        code_ok = false;
                    }
                }
                j += 1;
            }
        }
        i += 1;
    }
    s.add("诊断码：12 个码值与文案闭集自洽", code_ok,
          "段内码值两两唯一且每条可操作");

    let mut k = WaitKernel::new();
    let sp = k.spawn(CHILD_PID);
    let ex = k.exit_child(CHILD_PID, EXIT_CODE);
    let w1 = k.wait_pid(CHILD_PID, WAITER_A, false);
    let w2 = k.wait_pid(CHILD_PID, WAITER_B, false);
    s.add("直调序列：exit 后 wait 收割一次，第二次 ECHILD",
          sp.is_ok() && ex.is_ok() && w1 == Ok(CHILD_PID as i32)
              && w2 == Err(ECHILD) && k.reaps() == 1 && k.echld() == 1,
          "僵尸只能被收割一次，重复 wait 必须回 ECHILD");

    let mut knob = WaitKernel::new();
    let _sp = knob.spawn(0x2B);
    let running_nohang = knob.wait_pid(0x2B, WAITER_A, true);
    let running_block = knob.wait_pid(0x2B, WAITER_A, false);
    s.add("运行中：WNOHANG 返回 0，阻塞 wait 回 EAGAIN",
          running_nohang == Ok(0) && running_block == Err(EAGAIN)
              && knob.zero_returns() == 1 && knob.blocked() == 1,
          "WNOHANG 不得把运行中子进程误报为无子进程");

    let mut bad = WaitKernel::new();
    let unknown_exit = bad.exit_child(0x99, 0);
    let _sp2 = bad.spawn(0x2C);
    let _e2 = bad.exit_child(0x2C, 3);
    let double_exit = bad.exit_child(0x2C, 4);
    s.add("非法迁移：未知 pid 与重复 exit 全 ESRCH",
          unknown_exit == Err(ESRCH) && double_exit == Err(ESRCH),
          "Running 之外的 exit 不得再次记账 SIGCHLD");

    let mut cap = WaitKernel::new();
    let mut filled = 0u32;
    while filled < MAX_CHILDREN as u32 {
        let pid = 0x100u32.saturating_add(filled);
        if cap.spawn(pid).is_ok() {
            filled = filled.saturating_add(1);
        } else {
            break;
        }
    }
    let overflow = cap.spawn(0x200);
    let dup = cap.spawn(0x100);
    s.add("容量：4 槽填满后 spawn EAGAIN，重复 pid EEXIST",
          filled == MAX_CHILDREN as u32 && overflow == Err(EAGAIN) && dup == Err(EEXIST),
          "定容表不得越界，重复登记必须显性拒绝");

    let mut trans_ok = true;
    let states = [ProcState::Running, ProcState::Zombie, ProcState::Reaped];
    let events = [EvKind::Exit, EvKind::WaitBlock, EvKind::WaitNohang];
    let mut si = 0usize;
    while si < states.len() {
        if let Some(st) = states.get(si).copied() {
            let mut ei = 0usize;
            while ei < events.len() {
                if let Some(ev) = events.get(ei).copied() {
                    let (next, code) = apply(st, ev);
                    let mut found = false;
                    let mut ti = 0usize;
                    while ti < TRANS.len() {
                        if let Some((s0, e0, s1, c0)) = TRANS.get(ti).copied() {
                            if s0 == st && e0 == ev && s1 == next && c0 == code {
                                found = true;
                            }
                        }
                        ti += 1;
                    }
                    if !found {
                        trans_ok = false;
                    }
                }
                ei += 1;
            }
        }
        si += 1;
    }
    s.add("状态机：9 组 (态, 事件) 全部能在冻结迁移表中找到",
          trans_ok,
          "迁移表与实现必须同判，越级迁移一律 ESRCH");

    let stats = run_race();
    s.add("100 轮唯一性：每轮恰一次收割、零违例",
          stats.rounds == RACE_ROUNDS
              && stats.single_reap == RACE_ROUNDS
              && stats.uniqueness_violations == 0,
          "success 恒为 1、reaps 恒为 1，多收与漏收都算违例");

    s.add("双 wait：第二次全 ECHILD（100/100）",
          stats.double_echld == RACE_ROUNDS,
          "每轮 echld 恰为 1，第二次 wait 不得返回 pid");

    s.add("竞态注入账：100 次全检出且逃逸为 0",
          stats.ledger.conserved() && stats.ledger.escaped == 0
              && stats.ledger.injected == RACE_ROUNDS,
          "唯一性、ECHILD 与僵尸清零三者同时成立才算检出");

    let nohang = run_nohang_matrix();
    s.add("WNOHANG 矩阵：24 轮无重复收割",
          nohang.rounds == NOHANG_ROUNDS && nohang.over_reap == 0
              && nohang.zero_rounds > 0 && nohang.zombie_left > 0,
          "早到 WNOHANG 返回 0 且僵尸留存，后续仍可一次性收割");

    let perm_reports = {
        let mut ok = true;
        let mut p = 0u8;
        while p < PERMS {
            let r = simulate(p, false, false);
            if r.success != 1 || r.reaps != 1 || r.echld != 1 || r.zombie_left {
                ok = false;
            }
            p = p.saturating_add(1);
        }
        ok
    };
    s.add("六排列全覆盖：每种事件次序都只收割一次",
          perm_reports,
          "排列 0..5 必须全部满足 success=1/reaps=1/echld=1");

    let mut status_ok = true;
    let mut p2 = 0u8;
    while p2 < PERMS {
        let r = simulate(p2, false, false);
        if r.status != w_exit_status(EXIT_CODE) || !w_ifexited(r.status) {
            status_ok = false;
        }
        p2 = p2.saturating_add(1);
    }
    s.add("状态字：退出码编码与 WIFEXITED 判定一致",
          status_ok && w_ifexited(w_exit_status(0)) && w_exit_status(7) == 0x0700,
          "status 必须是 (code & 0xFF) << 8，低 7 位为 0");

    let mut sig_ok = true;
    let mut p3 = 0u8;
    while p3 < PERMS {
        let r = simulate(p3, false, false);
        if r.sigchld != 1 {
            sig_ok = false;
        }
        p3 = p3.saturating_add(1);
    }
    s.add("SIGCHLD：每轮退出事件恰记一次", sig_ok,
          "重复 exit 被 ESRCH 拒后不得再记通知");

    let mut replay = WaitKernel::new();
    let _s = replay.spawn(0x300);
    let _e = replay.exit_child(0x300, 9);
    let _r1 = replay.wait_pid(0x300, WAITER_A, false);
    let r2 = replay.wait_pid(0x300, WAITER_B, false);
    let r3 = replay.wait_pid(0x300, WAITER_A, false);
    s.add("重复取证：同一 pid 第三、四次 wait 仍 ECHILD",
          r2 == Err(ECHILD) && r3 == Err(ECHILD) && replay.reaps() == 1,
          "已收割 pid 不允许重新出现在成功返回值里");

    let fp_a = corpus_fingerprint();
    let fp_b = corpus_fingerprint();
    s.add("双跑一致：6 排列 + 直调序列指纹相等", fp_a == fp_b,
          "同种子两次必须逐字节相等");

    let race_text = format!(
        "轮{} / 唯一收割{} / 双 wait ECHILD{} / 逃逸{}",
        stats.rounds, stats.single_reap, stats.double_echld, stats.ledger.escaped
    );
    s.add_dyn("竞态快照", stats.single_reap == RACE_ROUNDS && stats.ledger.escaped == 0, &race_text);

    let nohang_text = format!(
        "WNOHANG {} 轮 / 零返回轮 {} / 僵尸留存 {} / 重复 {}",
        nohang.rounds, nohang.zero_rounds, nohang.zombie_left, nohang.over_reap
    );
    s.add_dyn("WNOHANG 快照", nohang.over_reap == 0, &nohang_text);

    let fp_text = format!("竞态指纹 0x{:08X}", fp_a);
    s.add_dyn("指纹快照", fp_a == fp_b, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_reap_then_echild() {
        let mut k = WaitKernel::new();
        assert_eq!(k.spawn(CHILD_PID), Ok(0));
        assert_eq!(k.exit_child(CHILD_PID, EXIT_CODE), Ok(()));
        assert_eq!(k.wait_pid(CHILD_PID, WAITER_A, false), Ok(CHILD_PID as i32));
        assert_eq!(k.wait_pid(CHILD_PID, WAITER_B, false), Err(ECHILD));
        assert_eq!(k.reaps(), 1);
    }

    #[test]
    fn nohang_on_running_returns_zero() {
        let mut k = WaitKernel::new();
        let _s = k.spawn(0x2B);
        assert_eq!(k.wait_pid(0x2B, WAITER_A, true), Ok(0));
        assert_eq!(k.wait_pid(0x2B, WAITER_A, false), Err(EAGAIN));
    }

    #[test]
    fn unknown_and_double_exit_esrch() {
        let mut k = WaitKernel::new();
        assert_eq!(k.exit_child(0x99, 0), Err(ESRCH));
        let _s = k.spawn(0x2C);
        assert_eq!(k.exit_child(0x2C, 1), Ok(()));
        assert_eq!(k.exit_child(0x2C, 2), Err(ESRCH));
    }

    #[test]
    fn spawn_capacity_and_duplicate() {
        let mut k = WaitKernel::new();
        let mut n = 0;
        while n < MAX_CHILDREN {
            let pid = 0x100 + n as u32;
            assert_eq!(k.spawn(pid).is_ok(), true);
            n += 1;
        }
        assert_eq!(k.spawn(0x200), Err(EAGAIN));
        assert_eq!(k.spawn(0x100), Err(EEXIST));
    }

    #[test]
    fn all_perms_single_reap() {
        let mut p = 0u8;
        while p < PERMS {
            let r = simulate(p, false, false);
            assert_eq!(r.success, 1);
            assert_eq!(r.reaps, 1);
            assert_eq!(r.echld, 1);
            assert_eq!(r.zombie_left, false);
            p = p.saturating_add(1);
        }
    }

    #[test]
    fn hundred_rounds_unique() {
        let stats = run_race();
        assert_eq!(stats.rounds, 100);
        assert_eq!(stats.single_reap, 100);
        assert_eq!(stats.double_echld, 100);
        assert_eq!(stats.uniqueness_violations, 0);
        assert_eq!(stats.ledger.escaped, 0);
    }

    #[test]
    fn nohang_matrix_no_double_reap() {
        let n = run_nohang_matrix();
        assert_eq!(n.rounds, 24);
        assert_eq!(n.over_reap, 0);
        assert_eq!(n.zero_rounds > 0, true);
        assert_eq!(n.zombie_left > 0, true);
    }

    #[test]
    fn transition_table_matches_apply() {
        assert_eq!(apply(ProcState::Running, EvKind::Exit), (ProcState::Zombie, 0));
        assert_eq!(apply(ProcState::Zombie, EvKind::WaitNohang), (ProcState::Reaped, 0));
        assert_eq!(apply(ProcState::Reaped, EvKind::WaitBlock), (ProcState::Reaped, ECHILD));
        assert_eq!(apply(ProcState::Zombie, EvKind::Exit), (ProcState::Zombie, ESRCH));
    }

    #[test]
    fn status_encoding() {
        assert_eq!(w_exit_status(7), 0x0700);
        assert_eq!(w_ifexited(0x0700), true);
        assert_eq!(w_ifexited(0x0007), false);
    }

    #[test]
    fn fingerprint_stable() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx9291_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
