//! UNX-F9292 进程退出竞态注入（SIGCHLD 投递竞态） · 0x7B680段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 F9292 逐字）：
//!   ① 纯功能代码 ≥340 行；② 判据原文：UNX-F9292-J1 SIGCHLD 投递竞态注入
//!   判据过，投递不丢失 100 次统计格过，与 B11 链对账一致格过；③ 完成时
//!   API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//!   ① 投递只用三件东西：阻塞掩码的 SIGCHLD 位、单个 pending 位（标准信号
//!      的合并语义）、投递计数。没有队列——SIGCHLD 是标准信号，阻塞期两次
//!      退出只留一个 pending 位；但两个僵尸都必须可收割，所以「不丢失」考的是
//!      僵尸不丢，而不是 pending 位不合并。
//!   ② 竞态脚本固定 10 条，每条是 RaceStep 序列驱动同一台状态机。step 不携带
//!      下标，恒作用于最低序号的活跃子进程 / 最低序号僵尸；因此脚本与表容量
//!      解耦，扩子进程表不会改写历史结果。
//!   ③ 100 次统计格 = 10 条脚本 × 10 轮。每格比较六元组
//!      (投递, 收割, ECHILD, 僵尸, pending, 合并次数)，六项全等才算检出。
//!   ④ 与 B11 链对账：链上五条断言（收割唯一、双 wait 第二次 ECHILD、收割
//!      不清 pending、SIG_IGN 不投递、合并后僵尸不减）各自有本地可复算探针，
//!      对账值由状态机真跑得出，而不是抄常量。
//!   ⑤ SIG_IGN / SA_NOCLDWAIT 走自动收割分支：不产生 SIGCHLD，wait 直接
//!      ECHILD；SA_NOCLDSTOP 只吞 stop/cont 的投递，退出投递不受影响。
//!   ⑥ 诊断码段内闭集 12 个（+1..=+12），码值 = SEG_BASE + 序号，两两唯一。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 诊断段基址（F9292 独占 0x7B680..=0x7B6BF）。
pub const SEG_BASE: u32 = 0x7B680;
/// 段窗口宽度（每卡 0x40）。
pub const SEG_SPAN: u32 = 0x40;
/// 本批段窗口下界（含）。
pub const SEG_WINDOW_LO: u32 = 0x7B000;
/// 本批段窗口上界（含）。
pub const SEG_WINDOW_HI: u32 = 0x7BFFF;

/// Linux SIGCHLD 信号号。
pub const SIGCHLD: u32 = 17;
/// 默认处置。
pub const SIG_DFL: u32 = 0;
/// 忽略处置。
pub const SIG_IGN: u32 = 1;
/// sigprocmask 的 SIG_BLOCK。
pub const SIG_BLOCK: u32 = 0;
/// sigprocmask 的 SIG_UNBLOCK。
pub const SIG_UNBLOCK: u32 = 1;
/// sigprocmask 的 SIG_SETMASK。
pub const SIG_SETMASK: u32 = 2;

/// 无此子进程。
pub const ECHILD: i32 = 10;
/// 无此进程。
pub const ESRCH: i32 = 3;
/// 参数坏值。
pub const EINVAL: i32 = 22;
/// 非阻塞等待无可收割对象（waitpid 返回 0，不置 errno）。
pub const EAGAIN: i32 = 11;

/// 子进程表容量。
pub const CHILD_CAP: usize = 8;
/// 竞态脚本条数。
pub const SCENARIO_COUNT: usize = 10;
/// 每脚本注入轮数。
pub const INJECT_ROUNDS: u32 = 10;
/// 100 次统计格总数。
pub const TOTAL_CELLS: u32 = 100;
/// 脚本事件日志容量。
pub const LOG_CAP: usize = 24;
/// B11 链对账行数。
pub const CHAIN_COUNT: usize = 5;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// F9292 诊断码（段内 +1..=+12 闭集）。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SigCode {
    /// 阻塞期投递丢失
    DeliveryLost,
    /// 合并计数漂移
    CoalesceDrift,
    /// 重复投递
    DoubleDeliver,
    /// 同一子进程被收割两次
    DoubleReap,
    /// 收割误清 pending
    PendingCleared,
    /// 忽略处置仍投递
    IgnoredDelivery,
    /// NOCLDSTOP 未吞 stop/cont
    StopLeaked,
    /// 事件顺序被改写
    OrderDrift,
    /// 注入账不守恒
    LedgerRegress,
    /// 双跑不一致
    NotReproducible,
    /// B11 链对账不一致
    ChainMismatch,
    /// 段位越界
    SegmentViolation,
}

impl SigCode {
    /// 段内序号（契约口径的 u16 码）。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 段内码值 = SEG_BASE + 1 + 序号，两两唯一。
    pub fn val(self) -> u32 {
        SEG_BASE
            .saturating_add(1)
            .saturating_add(self.code() as u32)
    }

    /// 中文一句话建议（闭集，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            SigCode::DeliveryLost => "阻塞期投递丢失：pending 位必须置起，解除阻塞后补投一次",
            SigCode::CoalesceDrift => "合并计数漂移：标准信号只保留一个 pending 位，被合并的退出要单独记账",
            SigCode::DoubleDeliver => "重复投递：一次退出只能产生一次 SIGCHLD，pending 消费后必须清零",
            SigCode::DoubleReap => "重复收割：同一子进程只能被 wait 收割一次，第二次必须走 ECHILD",
            SigCode::PendingCleared => "收割误清 pending：wait 消费僵尸不等于信号已投递，pending 位不得随收割清零",
            SigCode::IgnoredDelivery => "忽略处置仍投递：SIG_IGN/SA_NOCLDWAIT 下子进程自动收割且不产生 SIGCHLD",
            SigCode::StopLeaked => "NOCLDSTOP 未吞 stop/cont：置位后停止与继续都不该产生投递",
            SigCode::OrderDrift => "事件顺序被改写：脚本日志必须按步序落盘，顺序变了统计格就不可复算",
            SigCode::LedgerRegress => "注入账不守恒：注入必须等于检出加逃逸，且逃逸恒为零",
            SigCode::NotReproducible => "双跑不一致：同脚本两次结果必须逐字节相等，隐藏状态即失败",
            SigCode::ChainMismatch => "B11 链对账不一致：链上断言必须由本地状态机复算命中，不得照抄常量",
            SigCode::SegmentViolation => "段位越界：段基址须 0x40 对齐且落在 0x7B000..=0x7BFFF",
        }
    }

    /// 闭集全量。
    pub const ALL: &'static [SigCode] = &[
        SigCode::DeliveryLost,
        SigCode::CoalesceDrift,
        SigCode::DoubleDeliver,
        SigCode::DoubleReap,
        SigCode::PendingCleared,
        SigCode::IgnoredDelivery,
        SigCode::StopLeaked,
        SigCode::OrderDrift,
        SigCode::LedgerRegress,
        SigCode::NotReproducible,
        SigCode::ChainMismatch,
        SigCode::SegmentViolation,
    ];
}

// ── 子进程相位 ────────────────────────────────────────────────────────
/// 子进程三相位（只进不退：Live → Zombie → Reaped；自动收割 Live → Reaped）。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChildPhase {
    Live = 0,
    Zombie = 1,
    Reaped = 2,
}

impl ChildPhase {
    /// 相位编码（指纹与日志使用）。
    pub fn code(self) -> u32 {
        self as u32
    }
}

/// 相位初值（`[INIT; N]` 需要 Copy 常量）。
const PHASE_LIVE: ChildPhase = ChildPhase::Live;

// ── 信号掩码 ──────────────────────────────────────────────────────────
/// 仅承载 SIGCHLD 的阻塞掩码（u64 位图，位号即信号号-1）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SigMask {
    bits: u64,
}

impl SigMask {
    /// 空掩码。
    pub const fn empty() -> SigMask {
        SigMask { bits: 0 }
    }

    /// 只含 SIGCHLD 的掩码。
    pub const fn sigchld_only() -> SigMask {
        SigMask {
            bits: 1u64 << (SIGCHLD as u64 - 1),
        }
    }

    /// 位图是否含某信号。
    pub fn contains(&self, signo: u32) -> bool {
        if signo == 0 || signo > 64 {
            return false;
        }
        self.bits & (1u64 << (signo - 1)) != 0
    }

    /// sigprocmask 三元语义；非法 how 返回 EINVAL 且掩码不动。
    pub fn apply(&mut self, how: u32, set: SigMask) -> Result<(), i32> {
        match how {
            SIG_BLOCK => {
                self.bits |= set.bits;
                Ok(())
            }
            SIG_UNBLOCK => {
                self.bits &= !set.bits;
                Ok(())
            }
            SIG_SETMASK => {
                self.bits = set.bits;
                Ok(())
            }
            _ => Err(EINVAL),
        }
    }

    /// 位图快照（指纹使用）。
    pub fn bits(&self) -> u64 {
        self.bits
    }
}

// ── 竞态脚本步 ────────────────────────────────────────────────────────
/// 一步竞态动作（不带下标：恒作用于最低序号对象，保证脚本与容量解耦）。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RaceStep {
    /// 最低序号活跃子进程退出。
    Exit = 0,
    /// 最低序号活跃子进程收到停止信号。
    Stop = 1,
    /// 最低序号活跃子进程继续运行。
    Cont = 2,
    /// 阻塞 SIGCHLD。
    Block = 3,
    /// 解除 SIGCHLD 阻塞（有待投递则补投）。
    Unblock = 4,
    /// waitpid(-1, 阻塞)：收割一个僵尸，无僵尸回 ECHILD。
    WaitAny = 5,
    /// waitpid(-1, WNOHANG)：收割一个僵尸，无僵尸返回 0。
    WaitNohang = 6,
    /// SIG_IGN / SA_NOCLDWAIT：自动收割，不投递。
    IgnoreSigchld = 7,
    /// SA_NOCLDSTOP：stop/cont 不投递。
    NocldStop = 8,
}

impl RaceStep {
    /// 步编码（日志与指纹使用）。
    pub fn code(self) -> u32 {
        self as u32
    }
}

// ── 状态机 ────────────────────────────────────────────────────────────
/// SIGCHLD 投递状态机。
///
/// 不变量：
/// - `zombies` 恒等于相位表中 Zombie 的格数；
/// - `pending ∈ {0,1}`，置位只发生在阻塞期，清零只发生在补投或 SIG_IGN；
/// - 投递计数只增不减，收割不清 pending。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SigchldMachine {
    phases: [ChildPhase; CHILD_CAP],
    mask: SigMask,
    pending: u8,
    deliveries: u32,
    reaps: u32,
    echld: u32,
    nohang_zero: u32,
    coalesced: u32,
    stop_suppressed: u32,
    nocldwait: bool,
    nocldstop: bool,
    invalids: u32,
    last: i32,
    log: [u8; LOG_CAP],
    log_len: usize,
}

impl SigchldMachine {
    /// 初始态：8 个子进程全部活跃、掩码空。
    pub const fn new() -> SigchldMachine {
        SigchldMachine {
            phases: [PHASE_LIVE; CHILD_CAP],
            mask: SigMask::empty(),
            pending: 0,
            deliveries: 0,
            reaps: 0,
            echld: 0,
            nohang_zero: 0,
            coalesced: 0,
            stop_suppressed: 0,
            nocldwait: false,
            nocldstop: false,
            invalids: 0,
            last: 0,
            log: [0u8; LOG_CAP],
            log_len: 0,
        }
    }

    /// 当前僵尸数（由相位表实时统计，不另存字段）。
    pub fn zombies(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < CHILD_CAP {
            if let Some(p) = self.phases.get(i) {
                if *p == ChildPhase::Zombie {
                    n = n.saturating_add(1);
                }
            }
            i = i.saturating_add(1);
        }
        n
    }

    /// 最低序号活跃子进程。
    fn first_live(&self) -> Option<usize> {
        let mut i = 0usize;
        while i < CHILD_CAP {
            if let Some(p) = self.phases.get(i) {
                if *p == ChildPhase::Live {
                    return Some(i);
                }
            }
            i = i.saturating_add(1);
        }
        None
    }

    /// 最低序号僵尸。
    fn first_zombie(&self) -> Option<usize> {
        let mut i = 0usize;
        while i < CHILD_CAP {
            if let Some(p) = self.phases.get(i) {
                if *p == ChildPhase::Zombie {
                    return Some(i);
                }
            }
            i = i.saturating_add(1);
        }
        None
    }

    /// 写步日志（容量满后截断并计数 invalids 之外不静默）。
    fn note(&mut self, step: RaceStep) {
        if let Some(slot) = self.log.get_mut(self.log_len) {
            *slot = step.code() as u8;
            self.log_len = self.log_len.saturating_add(1);
        }
    }

    /// 信号发生：阻塞期置 pending，非阻塞期立即投递一次。
    fn signal(&mut self) {
        if self.mask.contains(SIGCHLD) {
            if self.pending == 1 {
                self.coalesced = self.coalesced.saturating_add(1);
            } else {
                self.pending = 1;
            }
        } else {
            if self.pending == 1 {
                // 防御：非阻塞态不应残留 pending，残留视为被合并。
                self.pending = 0;
                self.coalesced = self.coalesced.saturating_add(1);
            }
            self.deliveries = self.deliveries.saturating_add(1);
        }
    }

    /// 一步脚本动作；返回本次动作的 errno（0 表示无错误）。
    pub fn step(&mut self, step: RaceStep) -> i32 {
        self.note(step);
        match step {
            RaceStep::Exit => match self.first_live() {
                Some(i) => {
                    if self.nocldwait {
                        if let Some(p) = self.phases.get_mut(i) {
                            *p = ChildPhase::Reaped;
                        }
                        self.last = 0;
                        0
                    } else {
                        if let Some(p) = self.phases.get_mut(i) {
                            *p = ChildPhase::Zombie;
                        }
                        self.signal();
                        self.last = 0;
                        0
                    }
                }
                None => {
                    self.invalids = self.invalids.saturating_add(1);
                    self.last = ESRCH;
                    ESRCH
                }
            },
            RaceStep::Stop | RaceStep::Cont => match self.first_live() {
                Some(_i) => {
                    if self.nocldstop {
                        self.stop_suppressed = self.stop_suppressed.saturating_add(1);
                    } else {
                        self.signal();
                    }
                    self.last = 0;
                    0
                }
                None => {
                    self.invalids = self.invalids.saturating_add(1);
                    self.last = ESRCH;
                    ESRCH
                }
            },
            RaceStep::Block => {
                let mut set = SigMask::empty();
                let applied = set.apply(SIG_BLOCK, SigMask::sigchld_only());
                if applied.is_ok() {
                    let merged = self.mask.apply(SIG_BLOCK, set);
                    if merged.is_ok() {
                        self.last = 0;
                        return 0;
                    }
                }
                self.invalids = self.invalids.saturating_add(1);
                self.last = EINVAL;
                EINVAL
            }
            RaceStep::Unblock => {
                let mut set = SigMask::empty();
                let applied = set.apply(SIG_BLOCK, SigMask::sigchld_only());
                if applied.is_ok() {
                    let merged = self.mask.apply(SIG_UNBLOCK, set);
                    if merged.is_ok() {
                        if self.pending == 1 {
                            self.pending = 0;
                            self.deliveries = self.deliveries.saturating_add(1);
                        }
                        self.last = 0;
                        return 0;
                    }
                }
                self.invalids = self.invalids.saturating_add(1);
                self.last = EINVAL;
                EINVAL
            }
            RaceStep::WaitAny => match self.first_zombie() {
                Some(i) => {
                    if let Some(p) = self.phases.get_mut(i) {
                        *p = ChildPhase::Reaped;
                    }
                    self.reaps = self.reaps.saturating_add(1);
                    self.last = 0;
                    0
                }
                None => {
                    self.echld = self.echld.saturating_add(1);
                    self.last = ECHILD;
                    ECHILD
                }
            },
            RaceStep::WaitNohang => match self.first_zombie() {
                Some(i) => {
                    if let Some(p) = self.phases.get_mut(i) {
                        *p = ChildPhase::Reaped;
                    }
                    self.reaps = self.reaps.saturating_add(1);
                    self.last = 0;
                    0
                }
                None => {
                    self.nohang_zero = self.nohang_zero.saturating_add(1);
                    self.last = 0;
                    0
                }
            },
            RaceStep::IgnoreSigchld => {
                self.nocldwait = true;
                self.pending = 0;
                let mut i = 0usize;
                while i < CHILD_CAP {
                    let zombie = match self.phases.get(i) {
                        Some(p) => *p == ChildPhase::Zombie,
                        None => false,
                    };
                    if zombie {
                        if let Some(p) = self.phases.get_mut(i) {
                            *p = ChildPhase::Reaped;
                        }
                    }
                    i = i.saturating_add(1);
                }
                self.last = 0;
                0
            }
            RaceStep::NocldStop => {
                self.nocldstop = true;
                self.last = 0;
                0
            }
        }
    }

    /// 六元组快照（100 格比对的唯一口径）。
    pub fn outcome(&self) -> Outcome {
        Outcome {
            deliveries: self.deliveries,
            reaps: self.reaps,
            echld: self.echld,
            zombies: self.zombies(),
            pending: self.pending as u32,
            coalesced: self.coalesced,
        }
    }

    /// 最近一次动作的 errno。
    pub fn last_errno(&self) -> i32 {
        self.last
    }

    /// 日志切片（顺序判据使用）；截断时按容量取前缀，不回绕。
    pub fn log_slice(&self) -> &[u8] {
        let n = if self.log_len <= LOG_CAP {
            self.log_len
        } else {
            LOG_CAP
        };
        match self.log.get(..n) {
            Some(s) => s,
            None => &[],
        }
    }

    /// 被拒动作数（非法步不得有副作用）。
    pub fn invalids(&self) -> u32 {
        self.invalids
    }

    /// 停止/继续被 SA_NOCLDSTOP 吞掉的次数。
    pub fn stop_suppressed(&self) -> u32 {
        self.stop_suppressed
    }

    /// 非阻塞等待返回 0 的次数。
    pub fn nohang_zero(&self) -> u32 {
        self.nohang_zero
    }

    /// 掩码快照。
    pub fn mask(&self) -> SigMask {
        self.mask
    }
}

// ── 六元组 ────────────────────────────────────────────────────────────
/// 一次脚本的六个可观察量。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Outcome {
    pub deliveries: u32,
    pub reaps: u32,
    pub echld: u32,
    pub zombies: u32,
    pub pending: u32,
    pub coalesced: u32,
}

impl Outcome {
    /// 常量构造（冻结表使用）。
    pub const fn new(
        deliveries: u32,
        reaps: u32,
        echld: u32,
        zombies: u32,
        pending: u32,
        coalesced: u32,
    ) -> Outcome {
        Outcome {
            deliveries,
            reaps,
            echld,
            zombies,
            pending,
            coalesced,
        }
    }
}

// ── 十条竞态脚本（冻结） ──────────────────────────────────────────────
const STEPS_S1: &[RaceStep] = &[RaceStep::Exit];
const STEPS_S2: &[RaceStep] = &[
    RaceStep::Block,
    RaceStep::Exit,
    RaceStep::Exit,
    RaceStep::Unblock,
];
const STEPS_S3: &[RaceStep] = &[
    RaceStep::Block,
    RaceStep::Exit,
    RaceStep::WaitAny,
    RaceStep::Unblock,
];
const STEPS_S4: &[RaceStep] = &[
    RaceStep::Block,
    RaceStep::Exit,
    RaceStep::Unblock,
    RaceStep::WaitAny,
    RaceStep::WaitAny,
];
const STEPS_S5: &[RaceStep] = &[RaceStep::Exit, RaceStep::Exit];
const STEPS_S6: &[RaceStep] = &[
    RaceStep::IgnoreSigchld,
    RaceStep::Exit,
    RaceStep::WaitAny,
];
const STEPS_S7: &[RaceStep] = &[
    RaceStep::NocldStop,
    RaceStep::Stop,
    RaceStep::Cont,
];
const STEPS_S8: &[RaceStep] = &[RaceStep::Stop, RaceStep::Cont, RaceStep::Exit];
const STEPS_S9: &[RaceStep] = &[
    RaceStep::Exit,
    RaceStep::Exit,
    RaceStep::WaitAny,
    RaceStep::WaitAny,
    RaceStep::WaitAny,
];
const STEPS_S10: &[RaceStep] = &[
    RaceStep::Block,
    RaceStep::Exit,
    RaceStep::WaitAny,
    RaceStep::Unblock,
    RaceStep::Exit,
    RaceStep::WaitNohang,
];

/// 一条竞态脚本及其冻结六元组。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Scenario {
    pub id: &'static str,
    pub steps: &'static [RaceStep],
    pub want: Outcome,
}

/// 十条脚本的冻结表（数值由状态机手算并逐轮复算）。
pub const SCENARIOS: &[Scenario] = &[
    Scenario {
        id: "S1-exit-unblocked",
        steps: STEPS_S1,
        want: Outcome::new(1, 0, 0, 1, 0, 0),
    },
    Scenario {
        id: "S2-two-exits-coalesce",
        steps: STEPS_S2,
        want: Outcome::new(1, 0, 0, 2, 0, 1),
    },
    Scenario {
        id: "S3-reap-keeps-pending",
        steps: STEPS_S3,
        want: Outcome::new(1, 1, 0, 0, 0, 0),
    },
    Scenario {
        id: "S4-reap-then-second-wait",
        steps: STEPS_S4,
        want: Outcome::new(1, 1, 1, 0, 0, 0),
    },
    Scenario {
        id: "S5-two-exits-inline",
        steps: STEPS_S5,
        want: Outcome::new(2, 0, 0, 2, 0, 0),
    },
    Scenario {
        id: "S6-sig-ign-auto-reap",
        steps: STEPS_S6,
        want: Outcome::new(0, 0, 1, 0, 0, 0),
    },
    Scenario {
        id: "S7-nocldstop-suppressed",
        steps: STEPS_S7,
        want: Outcome::new(0, 0, 0, 0, 0, 0),
    },
    Scenario {
        id: "S8-stop-cont-exit",
        steps: STEPS_S8,
        want: Outcome::new(3, 0, 0, 1, 0, 0),
    },
    Scenario {
        id: "S9-double-wait-echild",
        steps: STEPS_S9,
        want: Outcome::new(2, 2, 1, 0, 0, 0),
    },
    Scenario {
        id: "S10-deferred-then-nohang",
        steps: STEPS_S10,
        want: Outcome::new(2, 2, 0, 0, 0, 0),
    },
];

/// 跑一条脚本。
pub fn run_scenario(sc: &Scenario) -> (SigchldMachine, Outcome) {
    let mut m = SigchldMachine::new();
    let mut i = 0usize;
    while i < sc.steps.len() {
        if let Some(step) = sc.steps.get(i).copied() {
            m.step(step);
        }
        i = i.saturating_add(1);
    }
    let out = m.outcome();
    (m, out)
}

// ── 注入账 ────────────────────────────────────────────────────────────
/// 注入账：注入 = 检出 + 逃逸，逃逸恒为 0。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectLedger {
    pub injected: u32,
    pub detected: u32,
    pub escaped: u32,
}

impl InjectLedger {
    /// 零账。
    pub const fn zero() -> InjectLedger {
        InjectLedger {
            injected: 0,
            detected: 0,
            escaped: 0,
        }
    }

    /// 记一笔；返回守恒是否仍成立。
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

    /// 守恒。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 100 次统计格：10 脚本 × 10 轮，逐格六元组全等。
///
/// 返回（账本, 每脚本命中轮数, 守恒失败计数）。
pub fn run_noloss_matrix() -> (InjectLedger, [u32; SCENARIO_COUNT], u32) {
    let mut ledger = InjectLedger::zero();
    let mut per = [0u32; SCENARIO_COUNT];
    let mut broken = 0u32;
    let mut si = 0usize;
    while si < SCENARIOS.len() {
        if let Some(sc) = SCENARIOS.get(si) {
            let mut round = 0u32;
            while round < INJECT_ROUNDS {
                let (_m, got) = run_scenario(sc);
                let caught = got == sc.want;
                let rec = ledger.record(caught);
                if rec.is_err() {
                    broken = broken.saturating_add(1);
                }
                if caught {
                    if let Some(slot) = per.get_mut(si) {
                        *slot = slot.saturating_add(1);
                    }
                }
                round = round.saturating_add(1);
            }
        }
        si = si.saturating_add(1);
    }
    (ledger, per, broken)
}

// ── B11 链对账 ────────────────────────────────────────────────────────
/// 链上探针：每个探针都真跑一遍本地状态机取观察值。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChainProbe {
    /// 两子退出 → 收割数必须为 2（收割唯一，不重不漏）。
    ReapOnce = 0,
    /// 第三次 wait 必须 ECHILD。
    DoubleWaitEchld = 1,
    /// 收割不清 pending：解除阻塞仍补投一次。
    ReapKeepsPending = 2,
    /// SIG_IGN 不投递（投递数 0）。
    IgnoreSilent = 3,
    /// 合并后两个僵尸都可收割（僵尸数 2，投递数 1）。
    CoalesceKeepsBoth = 4,
}

impl ChainProbe {
    /// 探针编码。
    pub fn code(self) -> u32 {
        self as u32
    }

    /// 链上对账文本。
    pub fn claim(self) -> &'static str {
        match self {
            ChainProbe::ReapOnce => "每个僵尸只能被收割一次：两个子进程退出后收割数恰为 2",
            ChainProbe::DoubleWaitEchld => "僵尸耗尽后继续 wait 必须 ECHILD，不得返回旧状态",
            ChainProbe::ReapKeepsPending => "wait 消费僵尸不等于信号已投递：pending 必须保留到解除阻塞",
            ChainProbe::IgnoreSilent => "SIG_IGN/SA_NOCLDWAIT 下子进程自动收割且零投递",
            ChainProbe::CoalesceKeepsBoth => "阻塞期两次退出只投递一次，但两个僵尸都必须可收割",
        }
    }
}

/// 复算一个探针的观察值（0 也合法，故用 Result 区分「无法复算」）。
pub fn chain_probe_value(probe: ChainProbe) -> Result<u32, i32> {
    let steps: &[RaceStep] = match probe {
        ChainProbe::ReapOnce => STEPS_S9,
        ChainProbe::DoubleWaitEchld => STEPS_S9,
        ChainProbe::ReapKeepsPending => STEPS_S3,
        ChainProbe::IgnoreSilent => STEPS_S6,
        ChainProbe::CoalesceKeepsBoth => STEPS_S2,
    };
    let sc = Scenario {
        id: "probe",
        steps,
        want: Outcome::new(0, 0, 0, 0, 0, 0),
    };
    let (_m, out) = run_scenario(&sc);
    match probe {
        ChainProbe::ReapOnce => Ok(out.reaps),
        ChainProbe::DoubleWaitEchld => Ok(out.echld),
        ChainProbe::ReapKeepsPending => {
            if out.reaps == 1 && out.deliveries == 1 {
                Ok(out.deliveries)
            } else {
                Err(SigCode::PendingCleared as i32)
            }
        }
        ChainProbe::IgnoreSilent => Ok(out.deliveries),
        ChainProbe::CoalesceKeepsBoth => {
            if out.deliveries == 1 {
                Ok(out.zombies)
            } else {
                Err(SigCode::CoalesceDrift as i32)
            }
        }
    }
}

/// 一条链上对账行。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ChainRow {
    /// 对端卡号（B11 链上的兄弟单）。
    pub peer: &'static str,
    /// 探针。
    pub probe: ChainProbe,
    /// 期望观察值。
    pub want: u32,
}

/// 五条对账行（B11 链：F9290 execve 边界 / F9291 wait-exit / F9281 进程四域）。
pub const CHAIN: &[ChainRow] = &[
    ChainRow {
        peer: "F9291",
        probe: ChainProbe::ReapOnce,
        want: 2,
    },
    ChainRow {
        peer: "F9291",
        probe: ChainProbe::DoubleWaitEchld,
        want: 1,
    },
    ChainRow {
        peer: "F9291",
        probe: ChainProbe::ReapKeepsPending,
        want: 1,
    },
    ChainRow {
        peer: "F9281",
        probe: ChainProbe::IgnoreSilent,
        want: 0,
    },
    ChainRow {
        peer: "F9290",
        probe: ChainProbe::CoalesceKeepsBoth,
        want: 2,
    },
];

/// 逐行对账；返回（命中行数, 总行数）。
pub fn run_chain_reconcile() -> (u32, u32) {
    let mut matched = 0u32;
    let mut i = 0usize;
    while i < CHAIN.len() {
        if let Some(row) = CHAIN.get(i) {
            let got = chain_probe_value(row.probe);
            if got == Ok(row.want) {
                matched = matched.saturating_add(1);
            }
        }
        i = i.saturating_add(1);
    }
    (matched, CHAIN.len() as u32)
}

// ── 确定性指纹 ────────────────────────────────────────────────────────
/// FNV-1a 32 位（无浮点、无随机、无墙钟）。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            h ^= *b as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        i = i.saturating_add(1);
    }
    h
}

fn push_u32(out: &mut Vec<u8>, v: u32) {
    out.push((v >> 24) as u8);
    out.push((v >> 16) as u8);
    out.push((v >> 8) as u8);
    out.push(v as u8);
}

fn push_outcome(out: &mut Vec<u8>, o: &Outcome) {
    push_u32(out, o.deliveries);
    push_u32(out, o.reaps);
    push_u32(out, o.echld);
    push_u32(out, o.zombies);
    push_u32(out, o.pending);
    push_u32(out, o.coalesced);
}

/// 规范语料指纹：十脚本终态 + 事件日志 + 链上五值。
pub fn corpus_fingerprint() -> u32 {
    let mut out: Vec<u8> = Vec::new();
    push_u32(&mut out, SEG_BASE);
    push_u32(&mut out, SCENARIO_COUNT as u32);

    let mut i = 0usize;
    while i < SCENARIOS.len() {
        if let Some(sc) = SCENARIOS.get(i) {
            let (m, o) = run_scenario(sc);
            push_outcome(&mut out, &o);
            push_u32(&mut out, m.log_len as u32);
            let log = m.log_slice();
            let mut k = 0usize;
            while k < log.len() {
                if let Some(b) = log.get(k) {
                    out.push(*b);
                }
                k = k.saturating_add(1);
            }
            push_u32(&mut out, m.invalids());
            push_u32(&mut out, m.mask().bits() as u32);
        }
        i = i.saturating_add(1);
    }

    let mut c = 0usize;
    while c < CHAIN.len() {
        if let Some(row) = CHAIN.get(c) {
            let v = match chain_probe_value(row.probe) {
                Ok(x) => x,
                Err(e) => e as u32,
            };
            push_u32(&mut out, v);
        }
        c = c.saturating_add(1);
    }

    fnv1a(&out)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9292 判据集。
pub fn run_unx9292_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9292");

    let seg_ok = SEG_BASE % SEG_SPAN == 0
        && (SEG_WINDOW_LO..=SEG_WINDOW_HI).contains(&SEG_BASE)
        && SEG_BASE == 0x7B680;
    s.add(
        "段位守卫：0x40 对齐且落在 B11 窗口",
        seg_ok,
        "段基址唯一、0x40 对齐，且与相邻卡段互不覆盖",
    );

    let mut mask = SigMask::empty();
    let block_set = SigMask::sigchld_only();
    let b1 = mask.apply(SIG_BLOCK, block_set);
    let blocked = mask.contains(SIGCHLD);
    let bad_how = mask.apply(9, block_set);
    let still_blocked = mask.contains(SIGCHLD);
    let ub = mask.apply(SIG_UNBLOCK, block_set);
    s.add(
        "掩码：SIGCHLD 阻塞/解阻塞与非法 how",
        b1.is_ok()
            && blocked
            && bad_how == Err(EINVAL)
            && still_blocked
            && ub.is_ok()
            && !mask.contains(SIGCHLD)
            && !mask.contains(0)
            && !mask.contains(65),
        "sigprocmask 三元语义必须精确，非法 how 不得改动掩码",
    );

    let sc1 = match SCENARIOS.first() {
        Some(v) => *v,
        None => Scenario {
            id: "",
            steps: STEPS_S1,
            want: Outcome::new(0, 0, 0, 0, 0, 0),
        },
    };
    let (m1, o1) = run_scenario(&sc1);
    s.add(
        "正常流：非阻塞退出立即投递一次",
        o1 == sc1.want
            && o1.deliveries == 1
            && o1.zombies == 1
            && o1.pending == 0
            && m1.last_errno() == 0,
        "一次退出对应一次 SIGCHLD，僵尸立即可收割",
    );

    let (m2, o2) = run_scenario(match SCENARIOS.get(1) {
        Some(v) => v,
        None => &sc1,
    });
    s.add(
        "合并：阻塞期两次退出只投递一次且两僵尸俱在",
        o2.deliveries == 1
            && o2.zombies == 2
            && o2.coalesced == 1
            && o2.pending == 0
            && m2.stop_suppressed() == 0,
        "标准信号合并 pending 位，但僵尸不得被合并掉",
    );

    let sc3 = match SCENARIOS.get(2) {
        Some(v) => v,
        None => &sc1,
    };
    let (_m3, o3) = run_scenario(sc3);
    s.add(
        "次序：收割僵尸不清 pending，解除阻塞仍补投",
        o3.reaps == 1 && o3.deliveries == 1 && o3.echld == 0 && o3.pending == 0,
        "wait 与信号投递是两条独立路径，收割不得吞掉待投递",
    );

    let sc4 = match SCENARIOS.get(3) {
        Some(v) => v,
        None => &sc1,
    };
    let (_m4, o4) = run_scenario(sc4);
    s.add(
        "次序：僵尸耗尽后第二次 wait 回 ECHILD",
        o4.reaps == 1 && o4.echld == 1 && o4.deliveries == 1,
        "单次收割唯一性：第二次 wait 不得重复收割同一子进程",
    );

    let sc6 = match SCENARIOS.get(5) {
        Some(v) => v,
        None => &sc1,
    };
    let (m6, o6) = run_scenario(sc6);
    s.add(
        "忽略处置：SIG_IGN 自动收割且零投递",
        o6.deliveries == 0
            && o6.reaps == 0
            && o6.echld == 1
            && o6.zombies == 0
            && m6.last_errno() == ECHILD,
        "SIG_IGN / SA_NOCLDWAIT 不得残留僵尸，wait 直接 ECHILD",
    );

    let sc7 = match SCENARIOS.get(6) {
        Some(v) => v,
        None => &sc1,
    };
    let (m7, o7) = run_scenario(sc7);
    s.add(
        "NOCLDSTOP：stop/cont 不投递",
        o7.deliveries == 0 && m7.stop_suppressed() == 2,
        "置 SA_NOCLDSTOP 后停止与继续都必须被吞掉",
    );

    let sc8 = match SCENARIOS.get(7) {
        Some(v) => v,
        None => &sc1,
    };
    let (_m8, o8) = run_scenario(sc8);
    s.add(
        "默认处置：stop/cont/exit 各投递一次",
        o8.deliveries == 3 && o8.zombies == 1,
        "未置 NOCLDSTOP 时停止与继续同样产生 SIGCHLD",
    );

    let (ledger, per_cell, broken) = run_noloss_matrix();
    let cells_ok = per_cell.iter().all(|n| *n == INJECT_ROUNDS);
    s.add(
        "投递不丢失：100 次统计格逐格六元组全等",
        cells_ok && ledger.injected == TOTAL_CELLS && broken == 0,
        "10 脚本 × 10 轮，每格六项全等才算出格",
    );
    s.add(
        "注入账：100 次全检出、逃逸为 0",
        ledger.conserved()
            && ledger.escaped == 0
            && ledger.detected == TOTAL_CELLS
            && ledger.injected == TOTAL_CELLS,
        "注入数必须等于检出数，逃逸恒为零",
    );

    let (matched, total) = run_chain_reconcile();
    s.add(
        "B11 链对账：五条断言逐条复算一致",
        matched == total && total == CHAIN_COUNT as u32 && matched == 5,
        "链上断言必须由本地状态机真跑命中，不得照抄常量",
    );

    let sum_deliveries = {
        let mut acc = 0u32;
        let mut i = 0usize;
        while i < SCENARIOS.len() {
            if let Some(sc) = SCENARIOS.get(i) {
                let (_m, o) = run_scenario(sc);
                acc = acc.saturating_add(o.deliveries);
            }
            i = i.saturating_add(1);
        }
        acc
    };
    s.add(
        "全量投递数：十脚本合计 13 次",
        sum_deliveries == 13,
        "冻结总量用于发现脚本语义的静默漂移",
    );

    let mut invalid_probe = SigchldMachine::new();
    let mut k = 0u32;
    while k < CHILD_CAP as u32 {
        invalid_probe.step(RaceStep::Exit);
        k = k.saturating_add(1);
    }
    let over = invalid_probe.step(RaceStep::Exit);
    let refused = over == ESRCH
        && invalid_probe.invalids() == 1
        && invalid_probe.outcome().zombies == CHILD_CAP as u32;
    s.add(
        "状态机：无活跃子进程再退出回 ESRCH 且不改状态",
        refused,
        "拒绝路径只累加 invalids，僵尸数不得变化",
    );

    let sc9 = match SCENARIOS.get(8) {
        Some(v) => v,
        None => &sc1,
    };
    let (m9, o9) = run_scenario(sc9);
    let log_ok = m9.log_len == sc9.steps.len()
        && m9.log_slice().first().copied() == Some(RaceStep::Exit.code() as u8);
    s.add(
        "事件顺序：日志按步序逐条落盘",
        log_ok && o9.reaps == 2 && o9.echld == 1,
        "顺序漂移会让统计格不可复算，日志长度必须等于步数",
    );

    let fp_a = corpus_fingerprint();
    let fp_b = corpus_fingerprint();
    s.add(
        "双跑一致：脚本与链值指纹相等",
        fp_a == fp_b && fp_a != 0,
        "同输入两次必须逐字节相等，隐藏状态即失败",
    );

    s.add(
        "诊断码：12 个码文案非空",
        SigCode::ALL.len() == 12 && SigCode::ALL.iter().all(|c| !c.detail().is_empty()),
        "每个码都要给出可操作的中文建议，禁止空串",
    );

    let mut uniq = true;
    let mut a = 0usize;
    while a < SigCode::ALL.len() {
        let mut b = a.saturating_add(1);
        while b < SigCode::ALL.len() {
            if let (Some(x), Some(y)) = (SigCode::ALL.get(a), SigCode::ALL.get(b)) {
                if x.val() == y.val() || x.code() == y.code() {
                    uniq = false;
                }
            }
            b = b.saturating_add(1);
        }
        a = a.saturating_add(1);
    }
    s.add(
        "诊断码：12 个码值两两唯一",
        uniq,
        "段内闭集码不得重复，否则判据无法定位",
    );

    let cell_text = format!(
        "统计格 {} 检出 {} 逃逸 {} 合计投递 {}",
        ledger.injected, ledger.detected, ledger.escaped, sum_deliveries
    );
    s.add_dyn(
        "投递账快照",
        ledger.conserved() && ledger.escaped == 0,
        &cell_text,
    );

    let chain_text = format!("B11 链对账 {}/{} 行一致", matched, total);
    s.add_dyn("链对账快照", matched == total, &chain_text);

    let fp_text = format!("指纹 0x{:08X}", fp_a);
    s.add_dyn("指纹快照", fp_a == fp_b, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mask_three_way_semantics() {
        let mut m = SigMask::empty();
        let set = SigMask::sigchld_only();
        assert_eq!(m.apply(SIG_BLOCK, set), Ok(()));
        assert_eq!(m.contains(SIGCHLD), true);
        assert_eq!(m.apply(SIG_UNBLOCK, set), Ok(()));
        assert_eq!(m.contains(SIGCHLD), false);
        assert_eq!(m.apply(SIG_SETMASK, set), Ok(()));
        assert_eq!(m.contains(SIGCHLD), true);
        assert_eq!(m.apply(7, set), Err(EINVAL));
        assert_eq!(m.contains(SIGCHLD), true);
    }

    #[test]
    fn exit_delivers_once() {
        let mut m = SigchldMachine::new();
        assert_eq!(m.step(RaceStep::Exit), 0);
        let o = m.outcome();
        assert_eq!(o.deliveries, 1);
        assert_eq!(o.zombies, 1);
        assert_eq!(o.pending, 0);
        assert_eq!(m.last_errno(), 0);
    }

    #[test]
    fn blocked_exits_coalesce_but_keep_zombies() {
        let mut m = SigchldMachine::new();
        m.step(RaceStep::Block);
        m.step(RaceStep::Exit);
        m.step(RaceStep::Exit);
        let before = m.outcome();
        assert_eq!(before.deliveries, 0);
        assert_eq!(before.pending, 1);
        assert_eq!(before.zombies, 2);
        assert_eq!(before.coalesced, 1);
        m.step(RaceStep::Unblock);
        let after = m.outcome();
        assert_eq!(after.deliveries, 1);
        assert_eq!(after.pending, 0);
        assert_eq!(after.zombies, 2);
    }

    #[test]
    fn wait_does_not_clear_pending() {
        let mut m = SigchldMachine::new();
        m.step(RaceStep::Block);
        m.step(RaceStep::Exit);
        m.step(RaceStep::WaitAny);
        assert_eq!(m.outcome().reaps, 1);
        assert_eq!(m.outcome().pending, 1);
        m.step(RaceStep::Unblock);
        assert_eq!(m.outcome().deliveries, 1);
    }

    #[test]
    fn second_wait_is_echild() {
        let mut m = SigchldMachine::new();
        m.step(RaceStep::Exit);
        assert_eq!(m.step(RaceStep::WaitAny), 0);
        assert_eq!(m.step(RaceStep::WaitAny), ECHILD);
        assert_eq!(m.outcome().reaps, 1);
        assert_eq!(m.outcome().echld, 1);
    }

    #[test]
    fn ignore_auto_reaps_without_signal() {
        let mut m = SigchldMachine::new();
        m.step(RaceStep::IgnoreSigchld);
        m.step(RaceStep::Exit);
        assert_eq!(m.outcome().deliveries, 0);
        assert_eq!(m.outcome().zombies, 0);
        assert_eq!(m.step(RaceStep::WaitAny), ECHILD);
    }

    #[test]
    fn nocldstop_suppresses_stop_and_cont() {
        let mut m = SigchldMachine::new();
        m.step(RaceStep::NocldStop);
        m.step(RaceStep::Stop);
        m.step(RaceStep::Cont);
        assert_eq!(m.outcome().deliveries, 0);
        assert_eq!(m.stop_suppressed(), 2);
        m.step(RaceStep::Exit);
        assert_eq!(m.outcome().deliveries, 1);
    }

    #[test]
    fn matrix_has_100_cells() {
        let (ledger, per, broken) = run_noloss_matrix();
        assert_eq!(broken, 0);
        assert_eq!(ledger.injected, 100);
        assert_eq!(ledger.detected, 100);
        assert_eq!(ledger.escaped, 0);
        assert_eq!(ledger.conserved(), true);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn chain_reconciles_five_rows() {
        let (matched, total) = run_chain_reconcile();
        assert_eq!(total, 5);
        assert_eq!(matched, 5);
        assert_eq!(chain_probe_value(ChainProbe::ReapOnce), Ok(2));
        assert_eq!(chain_probe_value(ChainProbe::IgnoreSilent), Ok(0));
    }

    #[test]
    fn fingerprint_is_reproducible() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx9292_checks();
        assert_eq!(s.all_passed(), true, "{}", s.red_items_text());
        assert_eq!(s.len() >= 12, true);
    }
}
