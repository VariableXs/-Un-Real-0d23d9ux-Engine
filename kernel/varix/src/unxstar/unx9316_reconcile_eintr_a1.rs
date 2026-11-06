//! UNX-F9316 EINTR 与 A1 调度让出协议对账格 · 0x7BC80段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 UNX-F9316 逐字）：
//!   ① 纯功能代码 ≥320 行；② 判据原文：UNX-F9316-J1 对账判据过，让出
//!   协议单源格过，上游依赖登记格过；③ 完成时 API/软件写回：状态=已完成、
//!   结果摘要非空。
//!
//! 设计要点：
//!   ① 让出协议单源：全模块只有一条判定通路 `judge_yield(state, sig)`，
//!      状态机 Idle→Running→Yielding→Resumed 按秩（rank 0..=3）单调
//!      前进；倒退、同级、越级一律 EINVAL 拒绝。冻结对账表重放、注入
//!      器、指纹语料全部经由这同一个判定器，不存在第二份协议逻辑。
//!   ② EINTR 语义锚定阻塞点：只有 Running→Yielding 与 Yielding→Resumed
//!      两个「阻塞点」上信号挂起才回 EINTR；Idle 尚无阻塞点，信号只
//!      挂账不构成 EINTR（POSIX：信号在慢调用之前到达不打断尚未发起
//!      的调用）。终态 Resumed 再让出按状态判废回 EINVAL，且判废先于
//!      信号判定（判定次序判据）。
//!   ③ EINTR 回落安全：阻塞点被打断后由 `recover_resumed` 把状态安全
//!      回落 Resumed，任务账目除 eintr_seen 记一笔外零改动（重试零损
//!      坏）；重试经 `retry_new_cycle` 从 Resumed 进入新周期 Running，
//!      周期号单调自增，completed 只在完整到达 Resumed 时记账。
//!   ④ A1 对账册冻结表 14 行：场景/让出前状态/信号注入/操作/期望码/
//!      期望后状态逐行独立复算；EINTR 行同时复算回落后的终态，EINVAL
//!      行复算「拒绝后状态不损坏」；期望码 0 表示 Ok。
//!   ⑤ 上游依赖登记：本模块显式登记所依赖的协议常量来源（errno 码表
//!      的 EINTR/EAGAIN/EINVAL/ESRCH 与 A1 让出码表 0..=3），逐项做
//!      存在性 + 值一致 + 说明非空三重检查，登记缺失即红。
//!   ⑥ 三类注入器（坏状态/信号注入/倒退迁移）各 10 轮全抓，注入账
//!      守恒（注入=检出+逃逸，逃逸恒 0）；等待队列容量 16，满则 EAGAIN
//!      且账不丢；全模块指纹双跑一致（FNV-1a，确定性 LCG，无随机）。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F9316 独占 0x40 窗口）。
pub const SEG_BASE: u32 = 0x7BC80;

/// Linux errno：慢速系统调用被信号打断（正数码口径，系统调用回负值）。
pub const EINTR: i32 = 4;
/// Linux errno：资源暂不可用（本模块用于等待队列满）。
pub const EAGAIN: i32 = 11;
/// Linux errno：非法参数（本模块用于非法迁移/非法回落/非法重试）。
pub const EINVAL: i32 = 22;
/// Linux errno：目标不存在（本模块用于唤醒未登记任务）。
pub const ESRCH: i32 = 3;

/// 单类注入的复测轮数（判据要求 10/10 全抓）。
pub const INJECT_ROUNDS: u32 = 10;
/// 注入器类别数（坏状态/信号注入/倒退迁移）。
pub const INJECTOR_CLASSES: usize = 3;
/// A1 冻结对账册行数（判据要求 10~14 行，取上沿 14）。
pub const LEDGER_ROWS: usize = 14;
/// 等待队列容量（阻塞任务登记上限）。
pub const QUEUE_CAP: usize = 16;
/// 空槽标记（任务号从 1 起，0 保留为空）。
pub const EMPTY_SLOT: u32 = 0;
/// 合法任务号上界（24 位任务号空间）。
pub const MAX_TASK_ID: u32 = 0x00FF_FFFF;

// ── A1 让出协议状态机 ─────────────────────────────────────────────────
/// 让出协议四态。秩（rank）0..=3 定义单调前进方向：任何
/// 目标秩 ≠ 当前秩+1 的迁移都属非法（倒退/同级/越级）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum YieldState {
    /// 空闲：任务尚未进入调度视野
    Idle = 0,
    /// 运行：正在用户态执行、尚未触及阻塞点
    Running = 1,
    /// 让出阻塞：已登记进等待队列，等待唤醒
    Yielding = 2,
    /// 恢复：本轮让出协议完成（或被打断后安全回落）
    Resumed = 3,
}

impl YieldState {
    /// 协议秩（单调性的唯一依据）。
    pub fn rank(self) -> u32 {
        self as u32
    }

    /// 按秩还原状态；越界秩返回 None（坏状态注入在此被抓）。
    pub fn from_rank(rank: u32) -> Option<YieldState> {
        match rank {
            0 => Some(YieldState::Idle),
            1 => Some(YieldState::Running),
            2 => Some(YieldState::Yielding),
            3 => Some(YieldState::Resumed),
            _ => None,
        }
    }

    /// 下一协议态；终态 Resumed 无下一态（返回 None）。
    pub fn next(self) -> Option<YieldState> {
        match self.rank().checked_add(1) {
            Some(r) => YieldState::from_rank(r),
            None => None,
        }
    }

    /// 闭集全量（供文案/唯一性检查与遍历）。
    pub const ALL: [YieldState; 4] = [
        YieldState::Idle,
        YieldState::Running,
        YieldState::Yielding,
        YieldState::Resumed,
    ];
}

/// 状态中文名（诊断文案用，闭集）。
pub fn state_name(s: YieldState) -> &'static str {
    match s {
        YieldState::Idle => "空闲",
        YieldState::Running => "运行",
        YieldState::Yielding => "让出阻塞",
        YieldState::Resumed => "恢复",
    }
}

// ── 让出协议单源判定器 ────────────────────────────────────────────────
/// 让出协议单源判定器：全模块唯一的一条协议通路。
///
/// 输入当前协议态与「信号是否挂起」，输出下一态或错误码：
///   - Idle    + 无信号 → Ok(Running)：调度启动；
///   - Idle    + 有信号 → Ok(Running)：尚无阻塞点，信号只挂账，
///     不构成 EINTR（这是与「阻塞点被打断」的关键语义分野）；
///   - Running + 无信号 → Ok(Yielding)：让出成功，进入阻塞点；
///   - Running + 有信号 → Err(EINTR)：阻塞点发现信号挂起，必回
///     EINTR，随后 recover_resumed 把状态安全回落 Resumed；
///   - Yielding+ 无信号 → Ok(Resumed)：等待完成被唤醒；
///   - Yielding+ 有信号 → Err(EINTR)：阻塞中被信号打断（POSIX
///     慢系统调用语义），同样回落 Resumed 等待重试；
///   - Resumed（终态，无论信号）→ Err(EINVAL)：本轮协议已完成，
///     再让出属状态判废，且判废先于信号判定。
///
/// 返回的 errno 一律取正数码口径（系统调用层回负值）。
pub fn judge_yield(state: YieldState, sig_pending: bool) -> Result<YieldState, i32> {
    match state {
        YieldState::Idle => {
            // Idle 没有阻塞点：信号挂账不中断启动。
            let _ = sig_pending;
            Ok(YieldState::Running)
        }
        YieldState::Running => {
            if sig_pending {
                Err(EINTR)
            } else {
                Ok(YieldState::Yielding)
            }
        }
        YieldState::Yielding => {
            if sig_pending {
                Err(EINTR)
            } else {
                Ok(YieldState::Resumed)
            }
        }
        // 终态判废先于信号判定：即使有信号挂起也回 EINVAL 而非 EINTR。
        YieldState::Resumed => Err(EINVAL),
    }
}

/// 迁移合法性判定：只允许秩差恰好 +1 的前进。
/// 倒退（目标秩更小）、同级（差 0）、越级（差 ≥2）一律 EINVAL。
pub fn request_transition(from: YieldState, to: YieldState) -> Result<YieldState, i32> {
    let f = from.rank();
    let t = to.rank();
    if t == f.saturating_add(1) {
        Ok(to)
    } else {
        Err(EINVAL)
    }
}

/// EINTR 后的安全回落：仅 Running/Yielding（两个阻塞点前后态）
/// 可以回落 Resumed；Idle/Resumed 回落属非法来源，EINVAL 拒绝。
pub fn recover_resumed(state: YieldState) -> Result<YieldState, i32> {
    match state {
        YieldState::Running | YieldState::Yielding => Ok(YieldState::Resumed),
        _ => Err(EINVAL),
    }
}

/// 重试入口：仅终态 Resumed 可进入新周期（回 Running）；其余状态
/// 重试属非法，EINVAL 拒绝。周期号由任务账目负责自增。
pub fn retry_new_cycle(state: YieldState) -> Result<YieldState, i32> {
    match state {
        YieldState::Resumed => Ok(YieldState::Running),
        _ => Err(EINVAL),
    }
}

/// 按秩构造状态后推进一步：坏秩在构造层即被拒（返回 EINVAL），
/// 供坏状态注入器与冻结表以外的通用入口使用。
pub fn step_from_rank(rank: u32, sig_pending: bool) -> Result<YieldState, i32> {
    match YieldState::from_rank(rank) {
        Some(s) => judge_yield(s, sig_pending),
        None => Err(EINVAL),
    }
}

// ── 任务账目（重试零损坏的可观测面） ──────────────────────────────────
/// 单个任务在让出协议下的账目。corrupted 一旦置位即表示账目
/// 出现过内部不一致（非法回落成功、非法重试等），永不复位。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct YieldTask {
    /// 当前周期号（从 1 起；每次合法重试自增）
    pub cycle: u32,
    /// 当前协议态
    pub state: YieldState,
    /// 累计 EINTR 次数
    pub eintr_seen: u32,
    /// 累计完整让出次数（到达 Resumed 才记）
    pub completed: u32,
    /// 账目损坏标记
    pub corrupted: bool,
}

impl YieldTask {
    /// 新任务：周期 1，从 Idle 起。
    pub const fn new() -> YieldTask {
        YieldTask {
            cycle: 1,
            state: YieldState::Idle,
            eintr_seen: 0,
            completed: 0,
            corrupted: false,
        }
    }

    /// 启动：Idle→Running。经由单源判定器；拒绝不改状态
    /// （拒绝后状态不损坏）。
    pub fn begin(&mut self) -> Result<YieldState, i32> {
        match judge_yield(self.state, false) {
            Ok(next) => {
                self.state = next;
                Ok(next)
            }
            Err(code) => Err(code),
        }
    }

    /// 推进一步（可带信号注入）。EINTR 时：eintr_seen 记一笔、
    /// 状态经 recover_resumed 安全回落 Resumed、其余账目零改动
    /// （重试零损坏）；回落意外失败置 corrupted。
    pub fn step(&mut self, sig_pending: bool) -> Result<YieldState, i32> {
        let before = self.state;
        match judge_yield(before, sig_pending) {
            Ok(next) => {
                self.state = next;
                if next == YieldState::Resumed {
                    self.completed = self.completed.saturating_add(1);
                }
                Ok(next)
            }
            Err(code) => {
                if code == EINTR {
                    self.eintr_seen = self.eintr_seen.saturating_add(1);
                    match recover_resumed(before) {
                        Ok(s) => {
                            self.state = s;
                        }
                        // 阻塞态回落必须成功；走到这里即内部不一致。
                        Err(_) => {
                            self.corrupted = true;
                        }
                    }
                }
                Err(code)
            }
        }
    }

    /// 重试：仅 Resumed 态合法，进入新周期 Running。非法重试置
    /// corrupted（协议被误用）并拒绝。
    pub fn retry(&mut self) -> Result<YieldState, i32> {
        match retry_new_cycle(self.state) {
            Ok(s) => {
                self.cycle = self.cycle.saturating_add(1);
                self.state = s;
                Ok(s)
            }
            Err(code) => {
                self.corrupted = true;
                Err(code)
            }
        }
    }
}

// ── 等待队列（容量耗尽面） ────────────────────────────────────────────
/// 定容等待队列：Yielding 态任务在此登记。队列满回 EAGAIN 且账
/// 不丢；唤醒未登记任务回 ESRCH。槽内直接存任务号，0 为空。
#[derive(Clone, Copy, Debug)]
pub struct YieldQueue {
    slots: [u32; QUEUE_CAP],
    used: usize,
    registered: u32,
    resumed: u32,
    rejected_full: u32,
    unknown_resumes: u32,
}

impl YieldQueue {
    /// 空队列。
    pub const fn new() -> YieldQueue {
        YieldQueue {
            slots: [EMPTY_SLOT; QUEUE_CAP],
            used: 0,
            registered: 0,
            resumed: 0,
            rejected_full: 0,
            unknown_resumes: 0,
        }
    }

    /// 登记阻塞任务。非法号（0/超上界）与重复登记回 EINVAL，不占
    /// 容量账；队列满回 EAGAIN 并记 rejected_full 一笔。
    pub fn register(&mut self, task: u32) -> Result<usize, i32> {
        if task == EMPTY_SLOT || task > MAX_TASK_ID {
            return Err(EINVAL);
        }
        let mut i = 0usize;
        while i < QUEUE_CAP {
            if let Some(slot) = self.slots.get(i) {
                if *slot == task {
                    return Err(EINVAL);
                }
            }
            i += 1;
        }
        let mut j = 0usize;
        while j < QUEUE_CAP {
            if let Some(slot) = self.slots.get_mut(j) {
                if *slot == EMPTY_SLOT {
                    *slot = task;
                    self.used = self.used.saturating_add(1);
                    self.registered = self.registered.saturating_add(1);
                    return Ok(j);
                }
            }
            j += 1;
        }
        self.rejected_full = self.rejected_full.saturating_add(1);
        Err(EAGAIN)
    }

    /// 唤醒任务：命中则清槽并回 Ok(Resumed)；未登记回 ESRCH。
    pub fn resume(&mut self, task: u32) -> Result<YieldState, i32> {
        if task == EMPTY_SLOT {
            self.unknown_resumes = self.unknown_resumes.saturating_add(1);
            return Err(ESRCH);
        }
        let mut i = 0usize;
        while i < QUEUE_CAP {
            if let Some(slot) = self.slots.get_mut(i) {
                if *slot == task {
                    *slot = EMPTY_SLOT;
                    self.used = self.used.saturating_sub(1);
                    self.resumed = self.resumed.saturating_add(1);
                    return Ok(YieldState::Resumed);
                }
            }
            i += 1;
        }
        self.unknown_resumes = self.unknown_resumes.saturating_add(1);
        Err(ESRCH)
    }

    /// 当前在队任务数。
    pub fn used(&self) -> u32 {
        self.used as u32
    }

    /// 累计登记成功数。
    pub fn registered(&self) -> u32 {
        self.registered
    }

    /// 累计唤醒成功数。
    pub fn resumed(&self) -> u32 {
        self.resumed
    }

    /// 累计满拒数。
    pub fn rejected_full(&self) -> u32 {
        self.rejected_full
    }

    /// 账守恒：登记成功 = 唤醒成功 + 仍在队。满拒不进此式（它们
    /// 从未占槽），但由 rejected_full 单独可见。
    pub fn conserved(&self) -> bool {
        self.registered == self.resumed.saturating_add(self.used as u32)
    }
}

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..+0x0D 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Unx9316Code {
    /// Idle 期信号挂账：不构成 EINTR，不得误报
    IdleSigDeferred,
    /// 坏状态：协议秩越界（rank ≥ 4）
    BadStateRank,
    /// 倒退迁移：目标秩小于当前秩
    RegressiveMove,
    /// 越级迁移：秩差 ≥ 2
    LevelSkip,
    /// 阻塞点 EINTR：信号在 Running/Yielding 打断
    EintrAtBlock,
    /// 非法回落来源：Idle/Resumed 不得回落
    RecoverBadSource,
    /// 非法重试：非 Resumed 态不得重试
    RetryBadState,
    /// 终态误用：Resumed 再让出被拒
    TerminalMisuse,
    /// 等待队列满：EAGAIN
    QueueFull,
    /// 唤醒未登记任务：ESRCH
    QueueUnknownTask,
    /// 冻结对账册行冲突
    LedgerConflict,
    /// 双跑指纹不一致
    NotReproducible,
    /// 段位越界
    SegmentViolation,
}

impl Unx9316Code {
    /// 码值（段基址 + 变体序号）。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            Unx9316Code::IdleSigDeferred => "Idle 期信号只挂账不中断：尚无阻塞点，\
                不得误报 EINTR；请检查信号是否在慢调用发起后才到达",
            Unx9316Code::BadStateRank => "坏状态被拒：协议秩越界（合法域 0..=3）；\
                请用 YieldState::from_rank 的 None 分支拒绝而非截断",
            Unx9316Code::RegressiveMove => "倒退迁移被拒：目标秩小于当前秩；\
                让出协议只许前进，请改走 recover/retry 合法入口",
            Unx9316Code::LevelSkip => "越级迁移被拒：秩差超过 1；每步只许前进一态，\
                请拆成逐级迁移",
            Unx9316Code::EintrAtBlock => "阻塞点 EINTR：信号挂起时 Running/Yielding \
                必回 EINTR；请确认 recover_resumed 已把状态回落 Resumed 再重试",
            Unx9316Code::RecoverBadSource => "非法回落来源：Idle/Resumed 无阻塞点\
                可回落；请只对 Running/Yielding 调 recover_resumed",
            Unx9316Code::RetryBadState => "非法重试：仅 Resumed 态可进入新周期；\
                请先完成回落再重试，Running 态直接继续 step",
            Unx9316Code::TerminalMisuse => "终态误用：Resumed 是本轮协议终点，\
                再让出回 EINVAL；请走 retry_new_cycle 开新周期",
            Unx9316Code::QueueFull => "等待队列满：回 EAGAIN 且账不丢；\
                请先唤醒腾槽再登记，rejected_full 计数不得清零",
            Unx9316Code::QueueUnknownTask => "唤醒未登记任务：回 ESRCH；\
                请核对任务号来源，防陈旧句柄重复唤醒",
            Unx9316Code::LedgerConflict => "冻结对账册行冲突：重放结果与 A1 册\
                期望不一致；请核对单源判定器是否被旁路",
            Unx9316Code::NotReproducible => "双跑不一致：同语料两次指纹不同；\
                请排查隐藏可变状态或非确定来源",
            Unx9316Code::SegmentViolation => "段位越界：本模块段基址必须落在 \
                0x7A000..=0x7D1C0 且 0x40 对齐",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [Unx9316Code; 13] = [
        Unx9316Code::IdleSigDeferred,
        Unx9316Code::BadStateRank,
        Unx9316Code::RegressiveMove,
        Unx9316Code::LevelSkip,
        Unx9316Code::EintrAtBlock,
        Unx9316Code::RecoverBadSource,
        Unx9316Code::RetryBadState,
        Unx9316Code::TerminalMisuse,
        Unx9316Code::QueueFull,
        Unx9316Code::QueueUnknownTask,
        Unx9316Code::LedgerConflict,
        Unx9316Code::NotReproducible,
        Unx9316Code::SegmentViolation,
    ];
}

// ── A1 冻结对账册（14 行） ────────────────────────────────────────────
/// 对账册操作类别。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LedgerOp {
    /// 单源判定器推进一步（可带信号）
    Step,
    /// EINTR 安全回落
    Recover,
    /// 重试进入新周期
    Retry,
    /// 强制迁移（用于倒退/越级/同级注入行）
    MoveTo(YieldState),
}

/// 冻结表行：场景 / 让出前状态 / 信号注入 / 操作 / 期望码 / 期望后状态。
/// 期望码 0 表示 Ok；非 0 为 errno 正数码。期望后状态对 EINTR 行指
/// 回落后的 Resumed，对 EINVAL 行指拒绝后不损坏的原状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LedgerRow {
    /// 场景名
    pub scene: &'static str,
    /// 让出前状态
    pub before: YieldState,
    /// 信号注入
    pub sig: bool,
    /// 操作
    pub op: LedgerOp,
    /// 期望码（0=Ok）
    pub expect_code: i32,
    /// 期望后状态
    pub expect_after: YieldState,
}

/// A1 对账册冻结表（14 行，判据要求 10~14 行取上沿）。
/// 本表是冻结物：任何一行重放冲突即整格红。
pub const FROZEN_LEDGER: [LedgerRow; LEDGER_ROWS] = [
    LedgerRow { scene: "冷启动无信号", before: YieldState::Idle, sig: false,
        op: LedgerOp::Step, expect_code: 0, expect_after: YieldState::Running },
    LedgerRow { scene: "冷启动带信号挂账", before: YieldState::Idle, sig: true,
        op: LedgerOp::Step, expect_code: 0, expect_after: YieldState::Running },
    LedgerRow { scene: "让出成功入阻塞", before: YieldState::Running, sig: false,
        op: LedgerOp::Step, expect_code: 0, expect_after: YieldState::Yielding },
    LedgerRow { scene: "阻塞点信号打断", before: YieldState::Running, sig: true,
        op: LedgerOp::Step, expect_code: EINTR, expect_after: YieldState::Resumed },
    LedgerRow { scene: "等待完成被唤醒", before: YieldState::Yielding, sig: false,
        op: LedgerOp::Step, expect_code: 0, expect_after: YieldState::Resumed },
    LedgerRow { scene: "阻塞中信号打断", before: YieldState::Yielding, sig: true,
        op: LedgerOp::Step, expect_code: EINTR, expect_after: YieldState::Resumed },
    LedgerRow { scene: "终态再让出被拒", before: YieldState::Resumed, sig: false,
        op: LedgerOp::Step, expect_code: EINVAL, expect_after: YieldState::Resumed },
    LedgerRow { scene: "终态带信号仍先判状态", before: YieldState::Resumed, sig: true,
        op: LedgerOp::Step, expect_code: EINVAL, expect_after: YieldState::Resumed },
    LedgerRow { scene: "倒退迁移 Running→Idle", before: YieldState::Running, sig: false,
        op: LedgerOp::MoveTo(YieldState::Idle), expect_code: EINVAL, expect_after: YieldState::Running },
    LedgerRow { scene: "越级迁移 Idle→Yielding", before: YieldState::Idle, sig: false,
        op: LedgerOp::MoveTo(YieldState::Yielding), expect_code: EINVAL, expect_after: YieldState::Idle },
    LedgerRow { scene: "同级迁移 Running→Running", before: YieldState::Running, sig: false,
        op: LedgerOp::MoveTo(YieldState::Running), expect_code: EINVAL, expect_after: YieldState::Running },
    LedgerRow { scene: "阻塞态安全回落", before: YieldState::Yielding, sig: false,
        op: LedgerOp::Recover, expect_code: 0, expect_after: YieldState::Resumed },
    LedgerRow { scene: "空闲态回落被拒", before: YieldState::Idle, sig: false,
        op: LedgerOp::Recover, expect_code: EINVAL, expect_after: YieldState::Idle },
    LedgerRow { scene: "重试进入新周期", before: YieldState::Resumed, sig: false,
        op: LedgerOp::Retry, expect_code: 0, expect_after: YieldState::Running },
];

/// 单行重放：全程经由单源判定器（judge_yield / recover_resumed /
/// retry_new_cycle / request_transition），不复制第二份协议逻辑。
/// 返回（最终状态, 判定结果）：
///   - Step 成功 → 终态即 Ok 值；Step 回 EINTR → 终态为回落后的
///     Resumed；Step 回 EINVAL → 终态为原状态（拒绝不损坏）；
///   - Recover/Retry/MoveTo 成功 → 终态即结果态，失败 → 原状态。
pub fn replay_row(row: &LedgerRow) -> (YieldState, Result<YieldState, i32>) {
    match row.op {
        LedgerOp::Step => match judge_yield(row.before, row.sig) {
            Ok(next) => (next, Ok(next)),
            Err(code) => {
                if code == EINTR {
                    match recover_resumed(row.before) {
                        Ok(s) => (s, Err(code)),
                        Err(_) => (row.before, Err(code)),
                    }
                } else {
                    (row.before, Err(code))
                }
            }
        },
        LedgerOp::Recover => match recover_resumed(row.before) {
            Ok(s) => (s, Ok(s)),
            Err(e) => (row.before, Err(e)),
        },
        LedgerOp::Retry => match retry_new_cycle(row.before) {
            Ok(s) => (s, Ok(s)),
            Err(e) => (row.before, Err(e)),
        },
        LedgerOp::MoveTo(target) => match request_transition(row.before, target) {
            Ok(s) => (s, Ok(s)),
            Err(e) => (row.before, Err(e)),
        },
    }
}

/// 对账报告。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ReconcileReport {
    /// 册内行数
    pub rows: u32,
    /// 逐行复算一致数
    pub matched: u32,
    /// 冲突数（必须为 0）
    pub conflicts: u32,
    /// 首个冲突行下标（-1 表示无冲突）
    pub first_conflict: i32,
}

/// 冻结册全量重放对账：14 行逐行独立复算，零冲突才算过。
pub fn reconcile_ledger() -> ReconcileReport {
    let mut rep = ReconcileReport {
        rows: FROZEN_LEDGER.len() as u32,
        matched: 0,
        conflicts: 0,
        first_conflict: -1,
    };
    let mut i = 0usize;
    while i < FROZEN_LEDGER.len() {
        if let Some(row) = FROZEN_LEDGER.get(i) {
            let (final_state, res) = replay_row(row);
            let code_ok = if row.expect_code == 0 {
                res.is_ok()
            } else {
                res == Err(row.expect_code)
            };
            if code_ok && final_state == row.expect_after {
                rep.matched = rep.matched.saturating_add(1);
            } else {
                rep.conflicts = rep.conflicts.saturating_add(1);
                if rep.first_conflict < 0 {
                    rep.first_conflict = i as i32;
                }
            }
        }
        i += 1;
    }
    rep
}

// ── 上游依赖登记 ──────────────────────────────────────────────────────
/// 上游依赖登记表：(登记名, 依赖值, 来源说明)。
/// 前四项来自 errno 码表，后四项来自 A1 让出码表（与 YieldState
/// 秩一一对应）。登记缺失或值漂移即整格红。
pub const UPSTREAM_DEPS: &[(&str, i32, &str)] = &[
    ("EINTR", EINTR, "errno 码表：慢速系统调用被信号打断"),
    ("EAGAIN", EAGAIN, "errno 码表：资源暂不可用（等待队列满）"),
    ("EINVAL", EINVAL, "errno 码表：非法迁移/回落/重试参数"),
    ("ESRCH", ESRCH, "errno 码表：唤醒未登记任务"),
    ("YIELD_IDLE", 0, "A1 让出码表：Idle=0"),
    ("YIELD_RUNNING", 1, "A1 让出码表：Running=1"),
    ("YIELD_YIELDING", 2, "A1 让出码表：Yielding=2"),
    ("YIELD_RESUMED", 3, "A1 让出码表：Resumed=3"),
];

/// 按登记名取依赖值（存在性检查的可复算入口；表外返回 None）。
pub fn dep_lookup(name: &str) -> Option<i32> {
    let mut i = 0usize;
    while i < UPSTREAM_DEPS.len() {
        if let Some(row) = UPSTREAM_DEPS.get(i) {
            if row.0 == name {
                return Some(row.1);
            }
        }
        i += 1;
    }
    None
}

/// 登记项的本地权威值：errno 常量与让出码表秩的独立来源，
/// 用于和登记表逐项对数（登记值 ≠ 权威值即漂移）。
fn expected_dep_value(name: &str) -> Option<i32> {
    match name {
        "EINTR" => Some(EINTR),
        "EAGAIN" => Some(EAGAIN),
        "EINVAL" => Some(EINVAL),
        "ESRCH" => Some(ESRCH),
        "YIELD_IDLE" => Some(YieldState::Idle.rank() as i32),
        "YIELD_RUNNING" => Some(YieldState::Running.rank() as i32),
        "YIELD_YIELDING" => Some(YieldState::Yielding.rank() as i32),
        "YIELD_RESUMED" => Some(YieldState::Resumed.rank() as i32),
        _ => None,
    }
}

/// 依赖登记审计报告。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DepReport {
    /// 三重检查全过的登记项数
    pub registered: u32,
    /// 权威值缺失（登记名不在权威表内）
    pub missing: u32,
    /// 登记值与权威值不一致或说明为空
    pub mismatched: u32,
}

/// 上游依赖登记格：逐项做存在性 + 值一致 + 名称/说明非空三重检查。
pub fn audit_deps() -> DepReport {
    let mut rep = DepReport { registered: 0, missing: 0, mismatched: 0 };
    let mut i = 0usize;
    while i < UPSTREAM_DEPS.len() {
        if let Some(row) = UPSTREAM_DEPS.get(i) {
            let (name, val, note) = *row;
            match expected_dep_value(name) {
                Some(v) => {
                    if v == val && !name.is_empty() && !note.is_empty() {
                        rep.registered = rep.registered.saturating_add(1);
                    } else {
                        rep.mismatched = rep.mismatched.saturating_add(1);
                    }
                }
                None => {
                    rep.missing = rep.missing.saturating_add(1);
                }
            }
        }
        i += 1;
    }
    rep
}

// ── 注入器与注入账 ────────────────────────────────────────────────────
/// 注入账：注入 = 检出 + 逃逸，逃逸必须恒为 0。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectLedger {
    /// 注入总次数
    pub injected: u32,
    /// 检出次数
    pub detected: u32,
    /// 逃逸次数（必须为 0）
    pub escaped: u32,
}

impl InjectLedger {
    const fn zero() -> InjectLedger {
        InjectLedger { injected: 0, detected: 0, escaped: 0 }
    }

    /// 记一次注入结果（守恒由 conserved 复算）。
    pub fn record(&mut self, caught: bool) {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
    }

    /// 守恒式复算。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 固定种子 LCG（确定性，无随机）：参数取经典 Numerical Recipes
/// 常量，序列只由种子决定，双跑必然一致。
pub struct Lcg {
    seed: u32,
}

impl Lcg {
    /// 固定种子构造。
    pub const fn new(seed: u32) -> Lcg {
        Lcg { seed }
    }

    /// 下一伪随机数（确定性）。
    pub fn next_u32(&mut self) -> u32 {
        self.seed = self.seed.wrapping_mul(1_103_515_245).wrapping_add(12_345);
        self.seed
    }

    /// [0, bound) 内取值；bound 为 0 时恒回 0（显性防御）。
    pub fn next_below(&mut self, bound: u32) -> u32 {
        if bound == 0 {
            return 0;
        }
        self.next_u32() % bound
    }
}

/// 三类注入器各 10 轮：返回逐类检出数与总账。
///
/// 注入器一（坏状态）：秩取 4..=13，全部越界，from_rank 必回 None，
/// step_from_rank 必回 EINVAL——10/10 必抓；
/// 注入器二（信号注入）：Running/Yielding 交替注入 sig=true，单源
/// 判定器必回 EINTR 且 recover 必落 Resumed——10/10 必抓；
/// 注入器三（倒退/越级）：目标秩恒取 (当前秩+2)%4，对任意当前秩
/// 都不可能是「+1 前进」（a≥2 时是倒退，a≤1 时是越级）——10/10 必抓。
pub fn run_injectors() -> ([u32; INJECTOR_CLASSES], InjectLedger) {
    let mut lcg = Lcg::new(0x9316_7BC8);
    let mut ledger = InjectLedger::zero();
    let mut per = [0u32; INJECTOR_CLASSES];

    // 注入器一：坏状态（秩越界）
    let mut r = 0u32;
    while r < INJECT_ROUNDS {
        let rank = 4 + lcg.next_below(10);
        let caught = step_from_rank(rank, false) == Err(EINVAL);
        ledger.record(caught);
        if caught {
            if let Some(slot) = per.get_mut(0) {
                *slot = slot.saturating_add(1);
            }
        }
        r += 1;
    }

    // 注入器二：信号注入（两个阻塞点交替）
    let mut r = 0u32;
    while r < INJECT_ROUNDS {
        let at_running = lcg.next_below(2) == 0;
        let st = if at_running { YieldState::Running } else { YieldState::Yielding };
        let caught = judge_yield(st, true) == Err(EINTR)
            && recover_resumed(st) == Ok(YieldState::Resumed);
        ledger.record(caught);
        if caught {
            if let Some(slot) = per.get_mut(1) {
                *slot = slot.saturating_add(1);
            }
        }
        r += 1;
    }

    // 注入器三：倒退/越级迁移（(a+2)%4 恒非法）
    let mut r = 0u32;
    while r < INJECT_ROUNDS {
        let a = lcg.next_below(4);
        let b = (a + 2) % 4;
        let caught = match (YieldState::from_rank(a), YieldState::from_rank(b)) {
            (Some(f), Some(t)) => request_transition(f, t) == Err(EINVAL),
            _ => false,
        };
        ledger.record(caught);
        if caught {
            if let Some(slot) = per.get_mut(2) {
                *slot = slot.saturating_add(1);
            }
        }
        r += 1;
    }

    (per, ledger)
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
        i += 1;
    }
    h
}

/// u32 大端推入字节语料。
fn push_u32(bytes: &mut Vec<u8>, v: u32) {
    bytes.push((v >> 24) as u8);
    bytes.push(((v >> 16) & 0xFF) as u8);
    bytes.push(((v >> 8) & 0xFF) as u8);
    bytes.push((v & 0xFF) as u8);
}

// ── 队列探针 ──────────────────────────────────────────────────────────
/// 队列探针报告：QUEUE_CAP+1 次登记（末次必 EAGAIN）、全量唤醒、
/// 一次未知唤醒（必 ESRCH），账目闭合由 closed 汇总。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct QueueReport {
    /// 登记成功数（应为 QUEUE_CAP）
    pub registered: u32,
    /// 唤醒成功数（应为 QUEUE_CAP）
    pub resumed: u32,
    /// 满拒数（应为 1）
    pub full_rejects: u32,
    /// 未知唤醒数（应为 1）
    pub unknown_resumes: u32,
    /// 全部子账闭合
    pub closed: bool,
}

/// 队列容量耗尽探针：第 QUEUE_CAP+1 次登记必回 EAGAIN；随后
/// 逐个唤醒全成功；未知任务必回 ESRCH；账目登记=唤醒+在队。
pub fn run_queue_probe() -> QueueReport {
    let mut q = YieldQueue::new();
    let mut rep = QueueReport {
        registered: 0,
        resumed: 0,
        full_rejects: 0,
        unknown_resumes: 0,
        closed: true,
    };

    // 登记至容量 +1：末次必满拒
    let mut id: u32 = 1;
    while id <= QUEUE_CAP as u32 + 1 {
        match q.register(id) {
            Ok(_) => {
                rep.registered = rep.registered.saturating_add(1);
            }
            Err(code) => {
                if code == EAGAIN {
                    rep.full_rejects = rep.full_rejects.saturating_add(1);
                } else {
                    rep.closed = false;
                }
            }
        }
        id += 1;
    }

    // 逐个唤醒：全成功
    let mut id2: u32 = 1;
    while id2 <= QUEUE_CAP as u32 {
        match q.resume(id2) {
            Ok(_) => {
                rep.resumed = rep.resumed.saturating_add(1);
            }
            Err(_) => {
                rep.closed = false;
            }
        }
        id2 += 1;
    }

    // 未知唤醒：必 ESRCH
    match q.resume(0x00AB_CDEF) {
        Err(ESRCH) => {
            rep.unknown_resumes = rep.unknown_resumes.saturating_add(1);
        }
        _ => {
            rep.closed = false;
        }
    }

    // 子账逐项核对（全部硬期望，不达标即 closed=false）
    if rep.registered != QUEUE_CAP as u32
        || rep.resumed != QUEUE_CAP as u32
        || rep.full_rejects != 1
        || rep.unknown_resumes != 1
        || !q.conserved()
        || q.used() != 0
    {
        rep.closed = false;
    }
    rep
}

/// 规范语料双跑指纹：冻结册重放账 + 三类注入账 + 队列探针账 +
/// 规范任务轨迹（含一次 EINTR 重试）+ 依赖登记数，全量 FNV-1a。
/// 同输入两次必须逐字节一致。
pub fn corpus_fingerprint() -> u32 {
    let mut bytes: Vec<u8> = Vec::new();

    // ① 冻结册对账账目
    let rep = reconcile_ledger();
    push_u32(&mut bytes, rep.rows);
    push_u32(&mut bytes, rep.matched);
    push_u32(&mut bytes, rep.conflicts);
    push_u32(&mut bytes, rep.first_conflict.max(0) as u32);

    // ② 三类注入账
    let (per, ledger) = run_injectors();
    let mut ci = 0usize;
    while ci < per.len() {
        if let Some(n) = per.get(ci) {
            push_u32(&mut bytes, *n);
        }
        ci += 1;
    }
    push_u32(&mut bytes, ledger.injected);
    push_u32(&mut bytes, ledger.detected);
    push_u32(&mut bytes, ledger.escaped);

    // ③ 队列探针账（17 登记全唤醒 + 未知唤醒）
    let qrep = run_queue_probe();
    push_u32(&mut bytes, qrep.registered);
    push_u32(&mut bytes, qrep.resumed);
    push_u32(&mut bytes, qrep.full_rejects);
    push_u32(&mut bytes, qrep.unknown_resumes);
    push_u32(&mut bytes, if qrep.closed { 1 } else { 0 });

    // ④ 规范任务轨迹：完整一轮 + 一次 EINTR 重试新周期
    let mut t = YieldTask::new();
    let _ = t.begin();
    let _ = t.step(false);
    let _ = t.step(true);
    let _ = t.retry();
    let _ = t.step(false);
    let _ = t.step(false);
    push_u32(&mut bytes, t.cycle);
    push_u32(&mut bytes, t.state.rank());
    push_u32(&mut bytes, t.eintr_seen);
    push_u32(&mut bytes, t.completed);
    push_u32(&mut bytes, if t.corrupted { 1 } else { 0 });

    // ⑤ 依赖登记数
    let drep = audit_deps();
    push_u32(&mut bytes, drep.registered);
    push_u32(&mut bytes, drep.missing);
    push_u32(&mut bytes, drep.mismatched);

    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9316 判据集（≥14 条静态判据；动态明细 ≤8 条）。
pub fn run_unx9316_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9316");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x7A000..=0x7D1C0).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x7A000..=0x7D1C0", seg_ok,
          "段基址必须唯一且落在本批窗口；越界即拒绝注册");

    // 2. 正常流：无信号全程让出 Idle→Running→Yielding→Resumed
    let mut t1 = YieldTask::new();
    let mut normal_ok = t1.begin() == Ok(YieldState::Running);
    if normal_ok {
        normal_ok = t1.step(false) == Ok(YieldState::Yielding);
    }
    if normal_ok {
        normal_ok = t1.step(false) == Ok(YieldState::Resumed);
    }
    let normal_ok = normal_ok
        && t1.completed == 1
        && t1.eintr_seen == 0
        && t1.cycle == 1
        && !t1.corrupted;
    s.add("正常流：无信号全程让出且账目闭合", normal_ok,
          "合法三步迁移应逐级达成 Resumed，completed 恰记一笔");

    // 3. 重试零损坏：EINTR 回落后新周期完整达成
    let mut t2 = YieldTask::new();
    let _ = t2.begin();
    let eintr_res = t2.step(true);
    let recovered = t2.state == YieldState::Resumed
        && t2.eintr_seen == 1
        && t2.completed == 0
        && !t2.corrupted;
    let retried = t2.retry() == Ok(YieldState::Running) && t2.cycle == 2;
    let _ = t2.step(false);
    let _ = t2.step(false);
    let retry_ok = eintr_res == Err(EINTR)
        && recovered
        && retried
        && t2.completed == 1
        && !t2.corrupted;
    s.add("重试零损坏：EINTR 落 Resumed 后新周期完整达成", retry_ok,
          "EINTR 只许记 eintr_seen 一笔，重试后 completed 恰为 1");

    // 4. 边界一：Idle 期信号挂账，不构成 EINTR
    let idle_sig_ok = judge_yield(YieldState::Idle, true) == Ok(YieldState::Running)
        && judge_yield(YieldState::Idle, false) == Ok(YieldState::Running);
    s.add("边界一：Idle 挂账信号不构成 EINTR", idle_sig_ok,
          "尚无阻塞点时信号只挂账；误报 EINTR 即判据红");

    // 5. 边界二：终态 Resumed 再让出被拒（上限态拒绝）
    let terminal_ok = judge_yield(YieldState::Resumed, false) == Err(EINVAL)
        && judge_yield(YieldState::Resumed, true) == Err(EINVAL);
    s.add("边界二：终态 Resumed 拒绝再让出", terminal_ok,
          "本轮协议终点不得再让出；重试须经 retry_new_cycle");

    // 6. 边界三：越级迁移 Idle→Yielding / Running→Resumed 被拒
    let skip_ok = request_transition(YieldState::Idle, YieldState::Yielding) == Err(EINVAL)
        && request_transition(YieldState::Running, YieldState::Resumed) == Err(EINVAL)
        && request_transition(YieldState::Idle, YieldState::Resumed) == Err(EINVAL);
    s.add("边界三：越级迁移全部被拒", skip_ok,
          "秩差 ≥2 一律 EINVAL；每步只许前进一态");

    // 7. 边界四：同级迁移被拒，四级合法前进全通
    let mut legal_all = true;
    let mut rk = 0u32;
    while rk < 3 {
        if let (Some(f), Some(t)) = (YieldState::from_rank(rk), YieldState::from_rank(rk + 1)) {
            if request_transition(f, t) != Ok(t) {
                legal_all = false;
            }
        } else {
            legal_all = false;
        }
        rk += 1;
    }
    let same_ok = request_transition(YieldState::Running, YieldState::Running) == Err(EINVAL)
        && request_transition(YieldState::Resumed, YieldState::Resumed) == Err(EINVAL);
    s.add("边界四：同级迁移被拒且四级合法前进全通", same_ok && legal_all,
          "差 0 与差 +1 的分野必须精确；合法三边一条不许误杀");

    // 8. 判定次序一：状态判废先于信号判定
    let order1_ok = judge_yield(YieldState::Resumed, true) == Err(EINVAL);
    s.add("判定次序一：终态判废先于信号（EINVAL 而非 EINTR）", order1_ok,
          "状态非法与信号同时成立时必须先回 EINVAL");

    // 9. 判定次序二：回落/重试来源校验独立于信号
    let order2_ok = recover_resumed(YieldState::Idle) == Err(EINVAL)
        && recover_resumed(YieldState::Resumed) == Err(EINVAL)
        && recover_resumed(YieldState::Running) == Ok(YieldState::Resumed)
        && recover_resumed(YieldState::Yielding) == Ok(YieldState::Resumed)
        && retry_new_cycle(YieldState::Running) == Err(EINVAL)
        && retry_new_cycle(YieldState::Resumed) == Ok(YieldState::Running);
    s.add("判定次序二：回落/重试来源校验精确", order2_ok,
          "recover 只收 Running/Yielding，retry 只收 Resumed");

    // 10. A1 冻结对账册：14 行逐行独立复算零冲突
    let rep = reconcile_ledger();
    let ledger_ok = rep.rows == LEDGER_ROWS as u32
        && rep.matched == LEDGER_ROWS as u32
        && rep.conflicts == 0
        && rep.first_conflict < 0;
    s.add("冻结对账册：14 行逐行复算零冲突", ledger_ok,
          "A1 册是冻结物；任何一行重放冲突即整格红");

    // 11. 上游依赖登记格：8 项存在且值一致
    let drep = audit_deps();
    let deps_ok = drep.registered == UPSTREAM_DEPS.len() as u32
        && drep.missing == 0
        && drep.mismatched == 0
        && dep_lookup("EINTR") == Some(EINTR)
        && dep_lookup("YIELD_RESUMED") == Some(YieldState::Resumed.rank() as i32)
        && dep_lookup("未登记项探针").is_none();
    s.add("上游依赖登记：errno 码表与让出码表逐项在册", deps_ok,
          "登记缺失或值漂移即红；表外名必须显性 None");

    // 12. 注入器一：坏状态 10/10 全抓
    let (per, ledger) = run_injectors();
    let bad_state_caught = match per.get(0) {
        Some(n) => *n == INJECT_ROUNDS,
        None => false,
    };
    s.add("注入器一：坏状态秩越界 10/10 全抓", bad_state_caught,
          "rank 4..=13 必须全部在 from_rank/step_from_rank 被拒");

    // 13. 注入器二：信号注入 10/10 全抓
    let sig_caught = match per.get(1) {
        Some(n) => *n == INJECT_ROUNDS,
        None => false,
    };
    s.add("注入器二：阻塞点信号注入 10/10 全抓", sig_caught,
          "Running/Yielding 带信号必回 EINTR 且回落 Resumed");

    // 14. 注入器三：倒退/越级迁移 10/10 全抓
    let regress_caught = match per.get(2) {
        Some(n) => *n == INJECT_ROUNDS,
        None => false,
    };
    s.add("注入器三：倒退/越级迁移 10/10 全抓", regress_caught,
          "(a+2)%4 目标恒非法，10 轮全部 EINVAL 拒绝");

    // 15. 注入账守恒：30 注入 = 30 检出 + 0 逃逸
    let conserve_ok = ledger.conserved()
        && ledger.escaped == 0
        && ledger.injected == INJECTOR_CLASSES as u32 * INJECT_ROUNDS
        && ledger.detected == INJECTOR_CLASSES as u32 * INJECT_ROUNDS;
    s.add("注入账守恒：30 注入 30 检出 0 逃逸", conserve_ok,
          "注入=检出+逃逸；逃逸非零即协议被旁路");

    // 16. 队列容量耗尽：第 17 次登记必 EAGAIN 且账不丢
    let qrep = run_queue_probe();
    let queue_ok = qrep.closed
        && qrep.full_rejects == 1
        && qrep.registered == QUEUE_CAP as u32;
    s.add("队列容量耗尽：满拒 EAGAIN 且登记账不丢", queue_ok,
          "容量 16 恰满后第 17 次必拒；拒登不占槽、不清账");

    // 17. 队列闭合：全量唤醒 + 未知唤醒 ESRCH + 守恒式
    let queue_close_ok = qrep.resumed == QUEUE_CAP as u32
        && qrep.unknown_resumes == 1;
    s.add("队列闭合：唤醒/ESRCH/守恒式全过", queue_close_ok,
          "登记=唤醒+在队；未知任务必须显性 ESRCH");

    // 18. 双跑一致：规范语料指纹相等
    let fp_a = corpus_fingerprint();
    let fp_b = corpus_fingerprint();
    s.add("双跑一致：规范语料指纹相等", fp_a == fp_b,
          "同输入两次必须逐字节一致，隐藏状态即失败");

    // 19. 文案闭集：13 个诊断码均有非空中文建议
    let text_ok = Unx9316Code::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("文案闭集：全部诊断码带中文建议", text_ok,
          "每个码必须能取到非空且可操作的说明");

    // 20. 诊断码唯一：13 个码值两两不同
    let mut uniq = true;
    let mut i = 0usize;
    while i < Unx9316Code::ALL.len() {
        let mut j = i + 1;
        while j < Unx9316Code::ALL.len() {
            if let (Some(a), Some(b)) = (Unx9316Code::ALL.get(i), Unx9316Code::ALL.get(j)) {
                if a.val() == b.val() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    s.add("诊断码：13 个码值两两唯一", uniq,
          "段内闭集码不得重复，否则判据无法定位");

    // 21. 容量守卫：判据集未截断（含动态明细槽）
    let cap_ok = !s.truncated() && s.dyn_dropped() == 0;
    s.add("容量守卫：判据集与动态明细均未截断", cap_ok,
          "静态超容或动态槽溢出都属信息损失，显性红");

    // 动态明细（≤8）：账目/对账/登记/队列/周期/指纹快照
    let ledger_text = format!(
        "注入{} 检出{} 逃逸{}",
        ledger.injected, ledger.detected, ledger.escaped
    );
    s.add_dyn("注入账快照", ledger.conserved() && ledger.escaped == 0, &ledger_text);

    let recon_text = format!(
        "{}/{} 行零冲突",
        rep.matched, rep.rows
    );
    s.add_dyn("对账快照", rep.conflicts == 0, &recon_text);

    let dep_text = format!(
        "登记{} 缺失{} 漂移{}",
        drep.registered, drep.missing, drep.mismatched
    );
    s.add_dyn("依赖登记快照", drep.missing == 0 && drep.mismatched == 0, &dep_text);

    let queue_text = format!(
        "登{} 唤{} 拒满{} 未知{}",
        qrep.registered, qrep.resumed, qrep.full_rejects, qrep.unknown_resumes
    );
    s.add_dyn("队列快照", qrep.closed, &queue_text);

    let task_text = format!(
        "周期{} EINTR{} 完成{} 损坏{}",
        t2.cycle, t2.eintr_seen, t2.completed, if t2.corrupted { 1 } else { 0 }
    );
    s.add_dyn("周期快照", t2.completed == 1 && !t2.corrupted, &task_text);

    let fp_text = format!("语料指纹 0x{:08X}", fp_a);
    s.add_dyn("指纹快照", fp_a == fp_b, &fp_text);

    // 22. 收口：判据条数达到声明值（阈值保守取 14）
    let n_before = s.len();
    s.add("收口：判据条数达到声明值", n_before >= 14, "阈值保守取 14，实际条数见计数");

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_full_cycle() {
        let mut t = YieldTask::new();
        assert_eq!(t.begin(), Ok(YieldState::Running));
        assert_eq!(t.step(false), Ok(YieldState::Yielding));
        assert_eq!(t.step(false), Ok(YieldState::Resumed));
        assert_eq!(t.completed, 1);
        assert_eq!(t.eintr_seen, 0);
        assert!(!t.corrupted);
    }

    #[test]
    fn idle_signal_deferred_not_eintr() {
        // Idle 期无阻塞点：信号挂账不构成 EINTR（边界一）。
        assert_eq!(judge_yield(YieldState::Idle, true), Ok(YieldState::Running));
        assert_eq!(judge_yield(YieldState::Idle, false), Ok(YieldState::Running));
    }

    #[test]
    fn terminal_state_rejects_step() {
        // 终态再让出被拒（无论信号），坏秩在构造层即被拒。
        assert_eq!(judge_yield(YieldState::Resumed, false), Err(EINVAL));
        assert_eq!(judge_yield(YieldState::Resumed, true), Err(EINVAL));
        assert_eq!(step_from_rank(4, false), Err(EINVAL));
        assert_eq!(step_from_rank(13, true), Err(EINVAL));
        assert!(YieldState::from_rank(3).is_some());
        assert!(YieldState::from_rank(4).is_none());
    }

    #[test]
    fn regress_and_skip_rejected() {
        // 倒退：目标秩更小。越级：秩差 ≥2。同级：差 0。
        assert_eq!(request_transition(YieldState::Running, YieldState::Idle), Err(EINVAL));
        assert_eq!(request_transition(YieldState::Resumed, YieldState::Yielding), Err(EINVAL));
        assert_eq!(request_transition(YieldState::Idle, YieldState::Yielding), Err(EINVAL));
        assert_eq!(request_transition(YieldState::Running, YieldState::Resumed), Err(EINVAL));
        assert_eq!(request_transition(YieldState::Running, YieldState::Running), Err(EINVAL));
        // 合法前进三边全通。
        assert_eq!(request_transition(YieldState::Idle, YieldState::Running), Ok(YieldState::Running));
        assert_eq!(request_transition(YieldState::Running, YieldState::Yielding), Ok(YieldState::Yielding));
        assert_eq!(request_transition(YieldState::Yielding, YieldState::Resumed), Ok(YieldState::Resumed));
    }

    #[test]
    fn eintr_recovers_to_resumed_zero_damage() {
        // 两个阻塞点带信号必回 EINTR，回落 Resumed，账零损坏。
        let mut t = YieldTask::new();
        let _ = t.begin();
        assert_eq!(t.step(true), Err(EINTR));
        assert_eq!(t.state, YieldState::Resumed);
        assert_eq!(t.eintr_seen, 1);
        assert_eq!(t.completed, 0);
        assert!(!t.corrupted);
        // Yielding 态被打断同样回落。
        let mut u = YieldTask::new();
        let _ = u.begin();
        let _ = u.step(false);
        assert_eq!(u.step(true), Err(EINTR));
        assert_eq!(u.state, YieldState::Resumed);
        assert_eq!(u.eintr_seen, 1);
        assert!(!u.corrupted);
    }

    #[test]
    fn retry_only_from_resumed() {
        let mut t = YieldTask::new();
        assert_eq!(t.retry(), Err(EINVAL));
        assert!(t.corrupted, "非法重试必须置损坏标记");
        let mut u = YieldTask::new();
        let _ = u.begin();
        let _ = u.step(true);
        assert_eq!(u.retry(), Ok(YieldState::Running));
        assert_eq!(u.cycle, 2);
    }

    #[test]
    fn three_injectors_ten_rounds_all_caught() {
        let (per, ledger) = run_injectors();
        assert_eq!(per.len(), INJECTOR_CLASSES);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS, "每类必须 10/10 全抓");
        }
        assert!(ledger.conserved());
        assert_eq!(ledger.escaped, 0);
        assert_eq!(ledger.injected, INJECTOR_CLASSES as u32 * INJECT_ROUNDS);
        assert_eq!(ledger.detected, INJECTOR_CLASSES as u32 * INJECT_ROUNDS);
    }

    #[test]
    fn frozen_ledger_reconciles() {
        let rep = reconcile_ledger();
        assert_eq!(rep.rows, LEDGER_ROWS as u32);
        assert_eq!(rep.matched, LEDGER_ROWS as u32);
        assert_eq!(rep.conflicts, 0);
        assert_eq!(rep.first_conflict, -1);
        // 逐行抽查两条关键语义：阻塞点 EINTR 回落、终态判废不损坏。
        let mut hit_eintr = 0u32;
        let mut hit_inval = 0u32;
        let mut i = 0usize;
        while i < FROZEN_LEDGER.len() {
            if let Some(row) = FROZEN_LEDGER.get(i) {
                let (final_state, res) = replay_row(row);
                if row.expect_code == EINTR {
                    hit_eintr += 1;
                    assert_eq!(res, Err(EINTR));
                    assert_eq!(final_state, YieldState::Resumed);
                } else if row.expect_code == EINVAL {
                    hit_inval += 1;
                    assert_eq!(res, Err(EINVAL));
                    assert_eq!(final_state, row.before, "EINVAL 拒绝后状态不损坏");
                }
            }
            i += 1;
        }
        assert_eq!(hit_eintr, 2, "册内恰有两条 EINTR 行");
        assert_eq!(hit_inval, 6, "册内恰有六条 EINVAL 行");
    }

    #[test]
    fn deps_registry_complete() {
        let rep = audit_deps();
        assert_eq!(rep.registered, UPSTREAM_DEPS.len() as u32);
        assert_eq!(rep.missing, 0);
        assert_eq!(rep.mismatched, 0);
        assert_eq!(dep_lookup("EINTR"), Some(EINTR));
        assert_eq!(dep_lookup("EAGAIN"), Some(EAGAIN));
        assert_eq!(dep_lookup("EINVAL"), Some(EINVAL));
        assert_eq!(dep_lookup("ESRCH"), Some(ESRCH));
        assert_eq!(dep_lookup("YIELD_IDLE"), Some(0));
        assert_eq!(dep_lookup("YIELD_RESUMED"), Some(3));
        assert!(dep_lookup("未登记项探针").is_none());
        assert!(dep_lookup("").is_none());
    }

    #[test]
    fn queue_capacity_and_esrch() {
        let mut q = YieldQueue::new();
        // 恰好装满 QUEUE_CAP 个。
        let mut id = 1u32;
        while id <= QUEUE_CAP as u32 {
            assert!(q.register(id).is_ok());
            id += 1;
        }
        // 第 QUEUE_CAP+1 次必满拒。
        assert_eq!(q.register(QUEUE_CAP as u32 + 1), Err(EAGAIN));
        assert_eq!(q.rejected_full(), 1);
        // 重复登记不占容量账，回 EINVAL；零号任务同样 EINVAL。
        assert_eq!(q.register(1), Err(EINVAL));
        assert_eq!(q.register(0), Err(EINVAL));
        assert_eq!(q.rejected_full(), 1, "EINVAL 拒绝不计入满拒");
        // 全量唤醒成功，随后再唤醒回 ESRCH。
        let mut id2 = 1u32;
        while id2 <= QUEUE_CAP as u32 {
            assert_eq!(q.resume(id2), Ok(YieldState::Resumed));
            id2 += 1;
        }
        assert_eq!(q.resume(0x00AB_CDEF), Err(ESRCH));
        assert_eq!(q.used(), 0);
        assert!(q.conserved());
        assert_eq!(q.registered(), q.resumed());
    }

    #[test]
    fn queue_probe_report_closed() {
        let rep = run_queue_probe();
        assert!(rep.closed);
        assert_eq!(rep.registered, QUEUE_CAP as u32);
        assert_eq!(rep.resumed, QUEUE_CAP as u32);
        assert_eq!(rep.full_rejects, 1);
        assert_eq!(rep.unknown_resumes, 1);
    }

    #[test]
    fn fingerprint_double_run_equal() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
        // 指纹非零（语料非空的显性证据）。
        assert_ne!(corpus_fingerprint(), 0);
    }

    #[test]
    fn segment_guard() {
        assert!(SEG_BASE % 0x40 == 0);
        assert!((0x7A000..=0x7D1C0).contains(&SEG_BASE));
        assert_eq!(SEG_BASE, 0x7BC80);
    }

    #[test]
    fn code_detail_closed_set() {
        assert_eq!(Unx9316Code::ALL.len(), 13);
        for c in Unx9316Code::ALL.iter() {
            assert!(!c.detail().is_empty(), "诊断码文案不得为空");
        }
        // 码值两两唯一且落在段窗口内。
        let mut i = 0usize;
        while i < Unx9316Code::ALL.len() {
            let mut j = i + 1;
            while j < Unx9316Code::ALL.len() {
                if let (Some(a), Some(b)) = (Unx9316Code::ALL.get(i), Unx9316Code::ALL.get(j)) {
                    assert_ne!(a.val(), b.val());
                }
                j += 1;
            }
            if let Some(a) = Unx9316Code::ALL.get(i) {
                assert!(a.val() >= SEG_BASE);
                assert!(a.val() <= SEG_BASE + 0x40);
            }
            i += 1;
        }
    }

    #[test]
    fn state_names_and_all_closed() {
        // 四态闭集：秩、中文名、next 链全部可复算。
        assert_eq!(YieldState::ALL.len(), 4);
        let mut rk = 0u32;
        while rk < 4 {
            if let Some(s) = YieldState::from_rank(rk) {
                assert_eq!(s.rank(), rk);
                assert!(!state_name(s).is_empty());
            } else {
                assert!(false, "秩 0..=3 必须可还原");
            }
            rk += 1;
        }
        assert_eq!(YieldState::Idle.next(), Some(YieldState::Running));
        assert_eq!(YieldState::Running.next(), Some(YieldState::Yielding));
        assert_eq!(YieldState::Yielding.next(), Some(YieldState::Resumed));
        assert_eq!(YieldState::Resumed.next(), None);
    }

    #[test]
    fn checks_all_green_and_counted() {
        let s = run_unx9316_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
        assert!(s.len() >= 14, "判据条数必须达到声明值");
        assert!(!s.truncated());
        assert_eq!(s.dyn_dropped(), 0);
    }
}
