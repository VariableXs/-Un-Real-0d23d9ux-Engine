//! UNX-F9312 EINTR 错误注入器（信号发生器协议） · 0x7BB80段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（派工卡 UNX-F9312 逐字）：
//!   ① 纯功能代码 ≥400 行；② 判据原文：UNX-F9312-J1 信号发生器注入协议判据过，
//!   注入时序可控可观测，100 次注入统计复测格过；③ 完成时 API/软件写回：
//!   状态=已完成、结果摘要非空。
//!
//! 设计要点：
//!   ① 注入器由三件互相独立的东西组成：定容预约环（8 发，FIFO）、相位掩码、
//!      发射预算。预约必须按轮次严格单调推进——环满回 EBUSY、轮次倒退回
//!      EINVAL，且两条失败路径都**先返回后写槽位**，保证「预约」「已发射」
//!      「被掩掉」三个计数不会被一次失败的 arm 搅乱。
//!   ② 四相位协议（这是「注入时序可控」的可执行定义）：
//!        PreWait    进等待队列之前  → 放弃本次调用，零字节，返回 EINTR
//!        InWait     队列内自旋中    → 放弃，零字节，EINTR（已入队，可观测）
//!        PostWake   已唤醒、未提交  → 放弃，零字节，EINTR（已入队，可观测）
//!        PostCommit 数据已提交之后  → 本次调用成功返回字节数，信号记 late
//!                                     欠账，由下一次系统调用**入口**交付 EINTR
//!      「可观测」= 每次真正发射都产出一条 Trace，写明命中相位、是否入队、
//!      是否提交、返回值/errno 与欠账状态；掩码压掉的预约不计发射但单独计
//!      masked——统计口径不允许把「没打出去」算成「打出去了」。
//!   ③ 100 次注入统计按 4 相位分层排布（i % 4），不靠伪随机凑分布：每相位恰好
//!      25 发；逐相位、逐 syscall 直方图必须与冻结期望相等；两次独立重放必须
//!      逐字节相同（指纹双跑）。
//!   ④ 账本三分且守恒：injected = direct + late + restarts；late 必须由后续
//!      flush 调用全部交付（late_delivered == late），残留 pending 归零，
//!      escaped 由 injected - (direct + late + restarts) **真实算出**而非写死 0。
//!   ⑤ SA_RESTART：可重启调用（read/write）在等待中被注入且用户态声明了
//!      SA_RESTART 时，内核重启该调用——用户态看不到 EINTR，注入器必须把它记
//!      成 restarts 而不是 direct；不可重启的 nanosleep 即使打了 restart 标记
//!      也仍回 EINTR。这条差异是本模块最容易做错的地方，单独成判据。
//!   ⑥ 诊断码段内闭集 12 个（+0x00..=+0x0B），码值两两唯一，每码一句中文处置。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 诊断段基址（F9312 独占 0x7BB80..=0x7BBBF 的 0x40 窗口）。
pub const SEG_BASE: u32 = 0x7BB80;
/// 前一张卡的段基址（段不重叠的下界锚点，F9311 = 0x7BB40）。
pub const SEG_PREV: u32 = 0x7BB40;
/// 本批段窗口上界（含），沿用 0x7A000..=0x7D1C0 的族窗口。
pub const SEG_LIMIT: u32 = 0x7D1C0;

/// errno：被信号中断（Linux 数值）。
pub const EINTR: i32 = 4;
/// errno：资源暂时不可用（本模块用作 flush 侧的无数据标记）。
pub const EAGAIN: i32 = 11;
/// errno：预约环/预算已占满。
pub const EBUSY: i32 = 16;
/// errno：轮次倒退等参数面错误。
pub const EINVAL: i32 = 22;
/// errno：协议表外相位（不可达，保留用于诊断码路径的自证）。
pub const ENOSYS: i32 = 38;

/// 预约环容量（2 的幂，取模用掩码）。
pub const RING_CAP: usize = 8;
/// 相位数量。
pub const PHASE_N: usize = 4;
/// syscall 种类数量。
pub const SYS_N: usize = 3;
/// 100 次注入统计的固定轮数（卡面「100 次注入统计复测格」）。
pub const INJECT_TOTAL: u32 = 100;
/// 单相位注入次数（100 / 4，分层排布的结果）。
pub const PHASE_SHOTS: u32 = 25;
/// 相位全开掩码。
pub const MASK_ALL: u8 = 0x0F;
/// 发射预算（远大于环容量，用于「预算耗尽」独立判据）。
pub const BUDGET: u32 = 128;
/// 每次普通调用期望搬运的字节数（非 0 才能区分「完成」与「零进展」）。
pub const XFER_BYTES: u64 = 64;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 注入器诊断码（段内 +0x00..=+0x0B 闭集）。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GenCode {
    /// 预约相位与发射相位不一致。
    PhaseMissed,
    /// 预约环 8 槽已满。
    RingFull,
    /// 轮次未严格递增。
    RoundRegress,
    /// 发射预算耗尽。
    BudgetSpent,
    /// 发射未产出观测记录。
    NotObservable,
    /// PostCommit 的迟到欠账未清。
    PendingLeak,
    /// 注入账不守恒。
    LedgerLeak,
    /// 被掩掉的预约混入了注入统计。
    MaskedDrift,
    /// 重启语义与调用类型不符。
    RestartMismatch,
    /// 统计直方图与冻结期望不符。
    StatsDrift,
    /// 双跑结果不一致。
    NotReproducible,
    /// 段位越界或与邻卡重叠。
    SegmentViolation,
}

impl GenCode {
    /// 段内码值（SEG_BASE + 序号），两两唯一。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 绝对诊断码（段基址 + 序号）。
    pub fn val(self) -> u32 {
        SEG_BASE + self.code() as u32
    }

    /// 中文处置建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            GenCode::PhaseMissed => "发射相位与预约相位不一致：先查掩码再看调度表，相位不得被重排",
            GenCode::RingFull => "预约环已满 8 发：先消费再预约，禁止覆盖尚未发射的预约",
            GenCode::RoundRegress => "轮次倒退：round 必须严格递增，重排调度表时同步修正序号",
            GenCode::BudgetSpent => "发射预算用尽：提高 budget 或新开发生器，预算不得就地重置",
            GenCode::NotObservable => "发射没有观测记录：每次注入必须落一条 Trace 才算可观测",
            GenCode::PendingLeak => "迟到信号欠账未清：PostCommit 的 EINTR 必须在下一次调用入口交付",
            GenCode::LedgerLeak => "注入账不守恒：injected 必须等于 direct + late + restarts",
            GenCode::MaskedDrift => "掩码语义漂移：被掩掉的预约只计 masked，不得计入注入统计",
            GenCode::RestartMismatch => "重启语义不符：SA_RESTART 只对可重启调用生效，nanosleep 不重启",
            GenCode::StatsDrift => "直方图与冻结期望不符：按相位分层重算，勿用伪随机补足计数",
            GenCode::NotReproducible => "双跑不一致：同调度表两次重放必须逐字节相同",
            GenCode::SegmentViolation => "段位越界：须 0x40 对齐且落在 0x7A000..=0x7D1C0",
        }
    }

    /// 闭集全量（顺序即码序，码值唯一性判据遍历它）。
    pub const ALL: &[GenCode] = &[
        GenCode::PhaseMissed,
        GenCode::RingFull,
        GenCode::RoundRegress,
        GenCode::BudgetSpent,
        GenCode::NotObservable,
        GenCode::PendingLeak,
        GenCode::LedgerLeak,
        GenCode::MaskedDrift,
        GenCode::RestartMismatch,
        GenCode::StatsDrift,
        GenCode::NotReproducible,
        GenCode::SegmentViolation,
    ];
}

// ── 相位与调用族 ──────────────────────────────────────────────────────
/// 注入相位（时序的唯一刻度）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Phase {
    /// 进等待队列之前。
    PreWait,
    /// 队列内自旋（已入队，可观测）。
    InWait,
    /// 已唤醒、尚未提交数据。
    PostWake,
    /// 数据已提交之后的迟到信号。
    PostCommit,
}

impl Phase {
    /// 相位全序（调度分层与直方图下标都用它）。
    pub const ALL: [Phase; PHASE_N] = [
        Phase::PreWait,
        Phase::InWait,
        Phase::PostWake,
        Phase::PostCommit,
    ];

    /// 掩码位。
    pub fn bit(self) -> u8 {
        match self {
            Phase::PreWait => 0x01,
            Phase::InWait => 0x02,
            Phase::PostWake => 0x04,
            Phase::PostCommit => 0x08,
        }
    }

    /// 直方图下标（0..=3）。
    pub fn rank(self) -> usize {
        match self {
            Phase::PreWait => 0,
            Phase::InWait => 1,
            Phase::PostWake => 2,
            Phase::PostCommit => 3,
        }
    }

    /// 该相位是否放弃本次调用（PostCommit 不放弃，只记欠账）。
    pub fn aborts(self) -> bool {
        !matches!(self, Phase::PostCommit)
    }

    /// 该相位下调用是否已经进入等待队列。
    pub fn entered_wait(self) -> bool {
        !matches!(self, Phase::PreWait)
    }

    /// 短名（Trace 文本化用）。
    pub fn tag(self) -> &'static str {
        match self {
            Phase::PreWait => "PRE",
            Phase::InWait => "IN",
            Phase::PostWake => "WAKE",
            Phase::PostCommit => "POST",
        }
    }
}

/// 被注入的 syscall 族（重启语义的载体）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Sys {
    /// read(2)：SA_RESTART 生效。
    Read,
    /// write(2)：SA_RESTART 生效。
    Write,
    /// nanosleep(2)：不可重启（内核不回放已消耗的睡眠时间）。
    NanoSleep,
}

impl Sys {
    /// syscall 全量。
    pub const ALL: [Sys; SYS_N] = [Sys::Read, Sys::Write, Sys::NanoSleep];

    /// 直方图下标。
    pub fn idx(self) -> usize {
        match self {
            Sys::Read => 0,
            Sys::Write => 1,
            Sys::NanoSleep => 2,
        }
    }

    /// 是否可被 SA_RESTART 重启——nanosleep 永远返回 EINTR。
    pub fn restartable(self) -> bool {
        matches!(self, Sys::Read | Sys::Write)
    }

    /// 名字（冻结表与诊断文本用）。
    pub fn name(self) -> &'static str {
        match self {
            Sys::Read => "read",
            Sys::Write => "write",
            Sys::NanoSleep => "nanosleep",
        }
    }
}

// ── 预约环 ────────────────────────────────────────────────────────────
/// 一发预约（相位 + 目标调用 + 轮次 + 是否带 restart 标记）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Shot {
    pub phase: Phase,
    pub sys: Sys,
    pub round: u16,
    pub restart: bool,
}

impl Shot {
    /// 空槽初始化值（数组 const 初始化需要）。
    pub const EMPTY: Shot = Shot {
        phase: Phase::PreWait,
        sys: Sys::Read,
        round: 0,
        restart: false,
    };

    /// 构造一发预约。
    pub const fn new(phase: Phase, sys: Sys, round: u16, restart: bool) -> Shot {
        Shot { phase, sys, round, restart }
    }
}

/// 定容 FIFO 预约环：只增 used，环形复用 head。
///
/// 不变量：
///   - `used <= RING_CAP`，`head < RING_CAP`；
///   - `last_round` 只在 arm 成功时推进，失败路径一字不改；
///   - 未发射的预约永不被覆盖（环满即拒，不做丢弃策略）。
#[derive(Clone, Copy, Debug)]
pub struct Ring {
    slots: [Shot; RING_CAP],
    head: usize,
    used: usize,
    last_round: u16,
}

impl Ring {
    /// 空环。
    pub const fn new() -> Ring {
        Ring {
            slots: [Shot::EMPTY; RING_CAP],
            head: 0,
            used: 0,
            last_round: 0,
        }
    }

    /// 已预约发数。
    pub fn used(&self) -> usize {
        self.used
    }

    /// 最近一次成功预约的轮次。
    pub fn last_round(&self) -> u16 {
        self.last_round
    }

    /// 队首预约（不消费）。
    pub fn peek(&self) -> Option<Shot> {
        if self.used == 0 {
            return None;
        }
        self.slots.get(self.head).copied()
    }

    /// 预约一发：满回 EBUSY，轮次非严格递增回 EINVAL。
    ///
    /// 两条失败路径都在写槽位之前返回，故失败的 arm 对 `used`/`last_round`
    /// 零副作用——这是「统计复测格」能对齐的前提。
    pub fn arm(&mut self, shot: Shot) -> Result<usize, i32> {
        if self.used >= RING_CAP {
            return Err(EBUSY);
        }
        if shot.round <= self.last_round {
            return Err(EINVAL);
        }
        let off = (self.head + self.used) & (RING_CAP - 1);
        match self.slots.get_mut(off) {
            Some(slot) => {
                *slot = shot;
            }
            None => return Err(EINVAL),
        }
        self.used += 1;
        self.last_round = shot.round;
        Ok(off)
    }

    /// 消费队首一发。
    pub fn take(&mut self) -> Option<Shot> {
        if self.used == 0 {
            return None;
        }
        let shot = self.slots.get(self.head).copied()?;
        self.head = (self.head + 1) & (RING_CAP - 1);
        self.used -= 1;
        Some(shot)
    }
}

// ── 调用模型与观测记录 ────────────────────────────────────────────────
/// 单次调用的推进状态（单向：Fresh → Waiting → Woken → Committed）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CallState {
    /// 尚未入队。
    Fresh,
    /// 已入等待队列。
    Waiting,
    /// 已唤醒、未提交。
    Woken,
    /// 已提交数据。
    Committed,
}

impl CallState {
    /// 状态次序（乱序推进即为协议违例）。
    pub fn rank(self) -> u8 {
        match self {
            CallState::Fresh => 0,
            CallState::Waiting => 1,
            CallState::Woken => 2,
            CallState::Committed => 3,
        }
    }
}

/// 一次被注入的调用。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Call {
    /// 调用族。
    pub sys: Sys,
    /// 期望搬运字节。
    pub want: u64,
    /// 已提交字节。
    pub got: u64,
    /// 推进状态。
    pub state: CallState,
    /// 被内核重放的次数（SA_RESTART）。
    pub restarts: u32,
    /// 本次调用是否由调用方声明了 SA_RESTART。
    pub restart_armed: bool,
}

impl Call {
    /// 新调用。
    pub const fn new(sys: Sys, want: u64, restart_armed: bool) -> Call {
        Call {
            sys,
            want,
            got: 0,
            state: CallState::Fresh,
            restarts: 0,
            restart_armed,
        }
    }
}

/// 一发注入的可观测记录。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Trace {
    /// 命中相位；`None` 表示本调用没有被注入（或被掩码压掉）。
    pub phase: Option<Phase>,
    /// 是否进入了等待队列。
    pub wait_entered: bool,
    /// 是否提交了数据。
    pub committed: bool,
    /// 交付字节。
    pub got: u64,
    /// 返回码（-1 表示错误，正数为字节数）。
    pub ret: i32,
    /// errno（0 表示成功）。
    pub errno: i32,
    /// 调用结束时是否留下迟到欠账。
    pub pending_after: bool,
    /// 是否走的是内核重启路径。
    pub restarted: bool,
}

impl Trace {
    /// 空白观测。
    pub const EMPTY: Trace = Trace {
        phase: None,
        wait_entered: false,
        committed: false,
        got: 0,
        ret: 0,
        errno: 0,
        pending_after: false,
        restarted: false,
    };

    /// 是否为零进展中断（用户态应重试的形态）。
    pub fn interrupted(&self) -> bool {
        self.errno == EINTR && self.ret < 0 && self.got == 0
    }
}

// ── 注入账本 ──────────────────────────────────────────────────────────
/// 注入账：escaped 由差集真实算出，不写死。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Ledger {
    /// 真正发射（掩码通过）的发数。
    pub injected: u32,
    /// 立即回 EINTR 的发数。
    pub direct: u32,
    /// 迟到欠账（PostCommit）。
    pub late: u32,
    /// 被 SA_RESTART 吸收、用户态不可见的发数。
    pub restarts: u32,
    /// 已完成交付的迟到欠账（flush 调用数）。
    pub late_delivered: u32,
    /// 逃逸（应为 0）。
    pub escaped: u32,
}

impl Ledger {
    /// 零账。
    pub const fn zero() -> Ledger {
        Ledger {
            injected: 0,
            direct: 0,
            late: 0,
            restarts: 0,
            late_delivered: 0,
            escaped: 0,
        }
    }

    /// 守恒：injected == direct + late + restarts，且逃逸为 0。
    pub fn conserved(&self) -> bool {
        self.direct
            .saturating_add(self.late)
            .saturating_add(self.restarts)
            == self.injected
            && self.escaped == 0
    }

    /// 迟到欠账是否已全部结算。
    pub fn settled(&self) -> bool {
        self.late_delivered == self.late
    }

    /// 用户态可见的 EINTR 总数（立即 + 迟到交付）。
    pub fn user_visible(&self) -> u32 {
        self.direct.saturating_add(self.late_delivered)
    }
}

// ── 信号发生器 ────────────────────────────────────────────────────────
/// 信号发生器：预约环 + 相位掩码 + 预算 + 四个单调计数。
#[derive(Clone, Debug)]
pub struct Generator {
    ring: Ring,
    mask: u8,
    budget: u32,
    fired: u32,
    masked: u32,
    direct: u32,
    late: u32,
    restarts: u32,
    late_delivered: u32,
    per_phase: [u32; PHASE_N],
    per_sys: [u32; SYS_N],
    pending: bool,
}

impl Generator {
    /// 全开掩码、默认预算的发生器。
    pub const fn new() -> Generator {
        Generator {
            ring: Ring::new(),
            mask: MASK_ALL,
            budget: BUDGET,
            fired: 0,
            masked: 0,
            direct: 0,
            late: 0,
            restarts: 0,
            late_delivered: 0,
            per_phase: [0; PHASE_N],
            per_sys: [0; SYS_N],
            pending: false,
        }
    }

    /// 指定预算（预算耗尽判据用）。
    pub const fn with_budget(budget: u32) -> Generator {
        let mut g = Generator::new();
        g.budget = budget;
        g
    }

    /// 预约一发（预算与环容量双闸）。
    pub fn arm(&mut self, shot: Shot) -> Result<usize, i32> {
        if self.spent() >= self.budget {
            return Err(EBUSY);
        }
        self.ring.arm(shot)
    }

    /// 已消耗的发射名额（已发射 + 已掩掉 + 在环未发）。
    pub fn spent(&self) -> u32 {
        self.fired
            .saturating_add(self.masked)
            .saturating_add(self.ring.used() as u32)
    }

    /// 设置相位掩码（只影响后续发射，不动历史计数）。
    pub fn set_mask(&mut self, mask: u8) {
        self.mask = mask & MASK_ALL;
    }

    /// 当前掩码。
    pub fn mask(&self) -> u8 {
        self.mask
    }

    /// 是否有迟到欠账待交付。
    pub fn pending(&self) -> bool {
        self.pending
    }

    /// 已发射数。
    pub fn fired(&self) -> u32 {
        self.fired
    }

    /// 被掩掉的预约数。
    pub fn masked(&self) -> u32 {
        self.masked
    }

    /// 逐相位直方图快照。
    pub fn phase_counts(&self) -> [u32; PHASE_N] {
        self.per_phase
    }

    /// 逐 syscall 直方图快照。
    pub fn sys_counts(&self) -> [u32; SYS_N] {
        self.per_sys
    }

    /// 账本快照（escaped 由差集算出）。
    pub fn ledger(&self) -> Ledger {
        let accounted = self
            .direct
            .saturating_add(self.late)
            .saturating_add(self.restarts);
        Ledger {
            injected: self.fired,
            direct: self.direct,
            late: self.late,
            restarts: self.restarts,
            late_delivered: self.late_delivered,
            escaped: self.fired.saturating_sub(accounted),
        }
    }

    /// 在给定相位尝试发射：相位不匹配不消费；掩码压掉则消费但不计注入。
    pub fn fire_at(&mut self, phase: Phase) -> Option<Shot> {
        let head = self.ring.peek()?;
        if head.phase != phase {
            return None;
        }
        if self.mask & phase.bit() == 0 {
            if self.ring.take().is_some() {
                self.masked = self.masked.saturating_add(1);
            }
            return None;
        }
        let shot = self.ring.take()?;
        self.fired = self.fired.saturating_add(1);
        bump(&mut self.per_phase, phase.rank());
        bump(&mut self.per_sys, shot.sys.idx());
        Some(shot)
    }

    /// 记一笔立即中断。
    pub fn note_direct(&mut self) {
        self.direct = self.direct.saturating_add(1);
    }

    /// 记一笔迟到欠账。
    pub fn note_late(&mut self) {
        self.late = self.late.saturating_add(1);
        self.pending = true;
    }

    /// 记一笔被内核重启吸收的注入。
    pub fn note_restart(&mut self) {
        self.restarts = self.restarts.saturating_add(1);
    }

    /// 入口交付迟到欠账：只在这里把 pending 清掉。
    pub fn take_pending(&mut self) -> bool {
        if !self.pending {
            return false;
        }
        self.pending = false;
        self.late_delivered = self.late_delivered.saturating_add(1);
        true
    }
}

/// 直方图 +1（越界静默不写，绝不裸索引）。
fn bump(dst: &mut [u32], i: usize) {
    if let Some(v) = dst.get_mut(i) {
        *v = v.saturating_add(1);
    }
}

// ── 四相位协议冻结表 ──────────────────────────────────────────────────
/// 协议行：相位 → (是否放弃调用, 是否回 EINTR, 是否留欠账, 是否已入队)。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ProtoRow {
    pub phase: Phase,
    pub aborts: bool,
    pub eintr: bool,
    pub pending: bool,
    pub entered: bool,
}

/// 冻结协议（顺序与 `Phase::ALL` 一致；这是注入器对外承诺的全部时序）。
pub const PROTOCOL: [ProtoRow; PHASE_N] = [
    ProtoRow {
        phase: Phase::PreWait,
        aborts: true,
        eintr: true,
        pending: false,
        entered: false,
    },
    ProtoRow {
        phase: Phase::InWait,
        aborts: true,
        eintr: true,
        pending: false,
        entered: true,
    },
    ProtoRow {
        phase: Phase::PostWake,
        aborts: true,
        eintr: true,
        pending: false,
        entered: true,
    },
    ProtoRow {
        phase: Phase::PostCommit,
        aborts: false,
        eintr: false,
        pending: true,
        entered: true,
    },
];

/// 按相位查协议行。
pub fn protocol_row(phase: Phase) -> Option<ProtoRow> {
    let mut i = 0usize;
    while i < PROTOCOL.len() {
        if let Some(row) = PROTOCOL.get(i) {
            if row.phase == phase {
                return Some(*row);
            }
        }
        i += 1;
    }
    None
}

// ── 调用驱动 ──────────────────────────────────────────────────────────
/// 驱动器：把一次调用推进过四个相位，最多消费一发预约。
///
/// 关键次序（不可交换）：入口欠账交付 → PreWait → 入队 → InWait → 唤醒 →
/// PostWake → 提交 → PostCommit。迟到欠账**先于**入队交付，否则下一发会抢
/// 在欠账之前落在自己的相位，导致「100 次注入 = 100 次可观测」不成立。
pub fn run_call(gen: &mut Generator, call: &mut Call) -> Trace {
    let mut t = Trace::EMPTY;

    // ① 入口：迟到信号优先交付，且不消费任何预约。
    if gen.take_pending() {
        t.phase = Some(Phase::PostCommit);
        t.ret = -1;
        t.errno = EINTR;
        return t;
    }

    // ② PreWait：尚未入队。
    call.state = CallState::Fresh;
    if let Some(shot) = gen.fire_at(Phase::PreWait) {
        return abort_or_restart(gen, call, t, shot, false);
    }

    // ③ 入队并自旋。
    call.state = CallState::Waiting;
    if let Some(shot) = gen.fire_at(Phase::InWait) {
        return abort_or_restart(gen, call, t, shot, true);
    }

    // ④ 被唤醒、未提交。
    call.state = CallState::Woken;
    if let Some(shot) = gen.fire_at(Phase::PostWake) {
        return abort_or_restart(gen, call, t, shot, true);
    }

    // ⑤ 提交数据。
    call.got = call.want;
    call.state = CallState::Committed;
    t.committed = true;
    t.wait_entered = true;
    t.got = call.got;

    // ⑥ 提交之后的迟到信号：本次成功，欠账留给下一次调用入口。
    if let Some(shot) = gen.fire_at(Phase::PostCommit) {
        gen.note_late();
        t.phase = Some(shot.phase);
        t.pending_after = true;
        t.ret = clamp_ret(call.got);
        return t;
    }

    t.ret = clamp_ret(call.got);
    t
}

/// 中断或重启：同一发射点上的两条互斥出口。
fn abort_or_restart(
    gen: &mut Generator,
    call: &mut Call,
    mut t: Trace,
    shot: Shot,
    entered: bool,
) -> Trace {
    t.phase = Some(shot.phase);
    t.wait_entered = entered;
    // 只有「发生器要求重启」且「调用可重启」且「调用方声明 SA_RESTART」三者
    // 同时成立，用户态才看不到 EINTR；缺一即回 EINTR。
    if shot.restart && call.restart_armed && call.sys.restartable() {
        call.restarts = call.restarts.saturating_add(1);
        call.got = call.want;
        call.state = CallState::Committed;
        t.restarted = true;
        t.committed = true;
        t.got = call.got;
        t.ret = clamp_ret(call.got);
        t.errno = 0;
        gen.note_restart();
    } else {
        // 放弃：数据一字不动，状态回到 Fresh（下一次调用重新开始）。
        call.got = 0;
        call.state = CallState::Fresh;
        t.ret = -1;
        t.errno = EINTR;
        gen.note_direct();
    }
    t
}

/// 字节数 → 返回码（超过 i32::MAX 时饱和，避免隐式截断）。
fn clamp_ret(got: u64) -> i32 {
    if got > i32::MAX as u64 {
        i32::MAX
    } else {
        got as i32
    }
}

// ── 调度表（分层，不靠伪随机） ────────────────────────────────────────
/// 第 `i` 发预约：相位按 i % 4 轮转（每相位恰好 25 发），
/// syscall 按 i % 3 轮转（100 发下为 34/33/33）。
pub fn schedule(i: u32) -> Option<Shot> {
    if i >= INJECT_TOTAL {
        return None;
    }
    let phase = match i % (PHASE_N as u32) {
        0 => Phase::PreWait,
        1 => Phase::InWait,
        2 => Phase::PostWake,
        _ => Phase::PostCommit,
    };
    let sys = match i % (SYS_N as u32) {
        0 => Sys::Read,
        1 => Sys::Write,
        _ => Sys::NanoSleep,
    };
    Some(Shot::new(phase, sys, (i as u16).saturating_add(1), false))
}

// ── 统计报告 ──────────────────────────────────────────────────────────
/// 一次完整注入统计的报告。
#[derive(Clone, Copy, Debug)]
pub struct Report {
    /// 账本。
    pub ledger: Ledger,
    /// 逐相位直方图。
    pub per_phase: [u32; PHASE_N],
    /// 逐 syscall 直方图。
    pub per_sys: [u32; SYS_N],
    /// flush 调用数（迟到交付次数）。
    pub flushes: u32,
    /// 有观测记录的发数。
    pub observed: u32,
    /// 轨迹指纹。
    pub checksum: u32,
    /// 结束时是否仍有欠账。
    pub pending_left: bool,
}

impl Report {
    /// 直方图总和是否等于注入数。
    pub fn histograms_closed(&self) -> bool {
        let mut sum = 0u32;
        let mut i = 0usize;
        while i < self.per_phase.len() {
            if let Some(n) = self.per_phase.get(i) {
                sum = sum.saturating_add(*n);
            }
            i += 1;
        }
        let mut sum_sys = 0u32;
        let mut k = 0usize;
        while k < self.per_sys.len() {
            if let Some(n) = self.per_sys.get(k) {
                sum_sys = sum_sys.saturating_add(*n);
            }
            k += 1;
        }
        sum == self.ledger.injected && sum_sys == self.ledger.injected
    }

    /// 每相位是否都命中预期发数。
    pub fn phase_uniform(&self, want: u32) -> bool {
        let mut i = 0usize;
        while i < self.per_phase.len() {
            match self.per_phase.get(i) {
                Some(n) => {
                    if *n != want {
                        return false;
                    }
                }
                None => return false,
            }
            i += 1;
        }
        true
    }
}

/// 跑完整 100 次注入（掩码可关掉某些相位）。
pub fn run_all(mask: u8) -> Report {
    let mut g = Generator::new();
    g.set_mask(mask);
    let mut bytes: Vec<u8> = Vec::new();
    let mut observed = 0u32;
    let mut flushes = 0u32;
    let mut i = 0u32;
    while i < INJECT_TOTAL {
        let shot = match schedule(i) {
            Some(s) => s,
            None => break,
        };
        if g.arm(shot).is_err() {
            break;
        }
        let mut call = Call::new(shot.sys, XFER_BYTES, false);
        let t = run_call(&mut g, &mut call);
        if t.phase.is_some() {
            observed = observed.saturating_add(1);
        }
        encode_trace(&mut bytes, i, &t);
        // PostCommit 留下的欠账必须在下一次调用入口清掉，否则下一发会
        // 抢在欠账之前发射，「一发一条观测」的对应关系就断了。
        if g.pending() {
            let mut flush = Call::new(Sys::Read, 0, false);
            let ft = run_call(&mut g, &mut flush);
            if ft.interrupted() {
                flushes = flushes.saturating_add(1);
            }
            encode_trace(&mut bytes, i, &ft);
        }
        i += 1;
    }
    Report {
        ledger: g.ledger(),
        per_phase: g.phase_counts(),
        per_sys: g.sys_counts(),
        flushes,
        observed,
        checksum: fnv1a(&bytes),
        pending_left: g.pending(),
    }
}

/// 卡面「100 次注入统计」标准跑法（相位全开）。
pub fn run_100_injections() -> Report {
    run_all(MASK_ALL)
}

/// 重启语义样本：同一发射点、不同调用族的两种出口。
#[derive(Clone, Copy, Debug)]
pub struct RestartCase {
    /// nanosleep 在 InWait 注入：不可重启，回 EINTR。
    pub hard: Trace,
    /// read 在 InWait 注入：可重启且声明了 SA_RESTART，用户态看到完成。
    pub soft: Trace,
    /// soft 侧的重放次数。
    pub restarts: u32,
}

/// 构造并跑两个重启样本（各自独立发生器，互不污染）。
pub fn run_restart_case() -> RestartCase {
    let mut hard_gen = Generator::new();
    let hard_shot = Shot::new(Phase::InWait, Sys::NanoSleep, 1, true);
    if hard_gen.arm(hard_shot).is_err() {
        return RestartCase {
            hard: Trace::EMPTY,
            soft: Trace::EMPTY,
            restarts: 0,
        };
    }
    let mut hard_call = Call::new(Sys::NanoSleep, 0, true);
    let hard = run_call(&mut hard_gen, &mut hard_call);

    let mut soft_gen = Generator::new();
    let soft_shot = Shot::new(Phase::InWait, Sys::Read, 1, true);
    if soft_gen.arm(soft_shot).is_err() {
        return RestartCase {
            hard,
            soft: Trace::EMPTY,
            restarts: 0,
        };
    }
    let mut soft_call = Call::new(Sys::Read, XFER_BYTES, true);
    let soft = run_call(&mut soft_gen, &mut soft_call);
    RestartCase {
        hard,
        soft,
        restarts: soft_call.restarts,
    }
}

/// 单发样本：给定相位、syscall，返回 (观测, 账本, 欠账)。
pub fn run_single(phase: Phase, sys: Sys, want: u64, restart: bool) -> (Trace, Ledger, bool) {
    let mut g = Generator::new();
    let shot = Shot::new(phase, sys, 1, restart);
    if g.arm(shot).is_err() {
        return (Trace::EMPTY, g.ledger(), g.pending());
    }
    let mut call = Call::new(sys, want, restart);
    let t = run_call(&mut g, &mut call);
    (t, g.ledger(), g.pending())
}

/// 迟到样本：先 PostCommit，再空跑一次 flush，返回 (迟到观测, flush 观测)。
pub fn run_late_pair() -> (Trace, Trace) {
    let mut g = Generator::new();
    let shot = Shot::new(Phase::PostCommit, Sys::Read, 1, false);
    if g.arm(shot).is_err() {
        return (Trace::EMPTY, Trace::EMPTY);
    }
    let mut call = Call::new(Sys::Read, XFER_BYTES, false);
    let late = run_call(&mut g, &mut call);
    let mut flush = Call::new(Sys::Read, 0, false);
    let delivered = run_call(&mut g, &mut flush);
    (late, delivered)
}

// ── 确定性指纹 ────────────────────────────────────────────────────────
/// FNV-1a 32 位偏移基。
pub const FNV_OFFSET: u32 = 0x811C_9DC5;
/// FNV-1a 32 位质数。
pub const FNV_PRIME: u32 = 0x0100_0193;

/// FNV-1a（逐字节喂入，无浮点/随机/墙钟）。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h = FNV_OFFSET;
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            h ^= *b as u32;
            h = h.wrapping_mul(FNV_PRIME);
        }
        i += 1;
    }
    h
}

/// 把一条观测编码进轨迹缓冲（定长 8 字节，字段序固定）。
fn encode_trace(out: &mut Vec<u8>, seq: u32, t: &Trace) {
    out.push((seq & 0xFF) as u8);
    out.push(match t.phase {
        Some(p) => p.bit(),
        None => 0,
    });
    out.push(if t.wait_entered { 1 } else { 0 });
    out.push(if t.committed { 2 } else { 0 });
    out.push(if t.restarted { 4 } else { 0 });
    out.push(if t.pending_after { 8 } else { 0 });
    out.push((t.errno & 0x7F) as u8);
    out.push((t.got & 0x3F) as u8);
}

/// 规范轨迹指纹（100 发口径）。
pub fn corpus_fingerprint() -> u32 {
    run_100_injections().checksum
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9312 判据集（add ≥ 12；add_dyn ≤ 8）。
pub fn run_unx9312_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9312");

    // ① 段位守卫：0x40 对齐、族窗口内、且严格位于邻卡之上。
    let seg_ok = SEG_BASE % 0x40 == 0
        && SEG_BASE > SEG_PREV
        && SEG_BASE < SEG_LIMIT
        && SEG_BASE + 0x40 <= SEG_LIMIT;
    s.add(
        "段位守卫：0x40 对齐且不与 F9311 重叠",
        seg_ok,
        "段基址唯一且落在本批窗口，段间不得互相覆盖",
    );

    // ② 协议表自洽：行序 = Phase::ALL，四个布尔位与相位语义逐行一致。
    let mut proto_ok = PROTOCOL.len() == PHASE_N;
    let mut pi = 0usize;
    while pi < PROTOCOL.len() {
        match (PROTOCOL.get(pi), Phase::ALL.get(pi)) {
            (Some(row), Some(ph)) => {
                if row.phase != *ph
                    || row.aborts != ph.aborts()
                    || row.entered != ph.entered_wait()
                    || row.eintr != row.aborts
                    || row.pending != !row.aborts
                {
                    proto_ok = false;
                }
            }
            _ => proto_ok = false,
        }
        pi += 1;
    }
    s.add(
        "协议表：四行与 Phase::ALL 逐行同序同义",
        proto_ok,
        "协议表是注入器对外的唯一时序承诺，行序与语义都不得漂移",
    );

    // ③ PreWait：未入队、零字节、EINTR。
    let (pre, pre_led, pre_pending) = run_single(Phase::PreWait, Sys::Read, XFER_BYTES, false);
    let pre_ok = pre.interrupted()
        && !pre.wait_entered
        && !pre.committed
        && !pre_pending
        && pre.phase == Some(Phase::PreWait)
        && pre_led.direct == 1
        && pre_led.late == 0;
    s.add(
        "相位 PreWait：入队前注入零字节 EINTR",
        pre_ok,
        "入队前放弃的调用不得写数据，也不得留下欠账",
    );

    // ④ InWait：已入队、零字节、EINTR。
    let (inw, inw_led, _) = run_single(Phase::InWait, Sys::Write, XFER_BYTES, false);
    let inw_ok = inw.interrupted()
        && inw.wait_entered
        && !inw.committed
        && inw.got == 0
        && inw.phase == Some(Phase::InWait)
        && inw_led.direct == 1;
    s.add(
        "相位 InWait：队列内注入零字节 EINTR",
        inw_ok,
        "自旋中被打断必须可观测为「已入队且未提交」",
    );

    // ⑤ PostWake：已入队、未提交、EINTR。
    let (wk, wk_led, _) = run_single(Phase::PostWake, Sys::Read, XFER_BYTES, false);
    let wk_ok = wk.interrupted()
        && wk.wait_entered
        && !wk.committed
        && wk.phase == Some(Phase::PostWake)
        && wk_led.direct == 1;
    s.add(
        "相位 PostWake：唤醒未提交时注入仍回 EINTR",
        wk_ok,
        "唤醒后提交前是最后一个可放弃的窗口，错过就必须走完成路径",
    );

    // ⑥ PostCommit：调用成功、留欠账。
    let (late, delivered) = run_late_pair();
    let late_ok = late.committed
        && late.got == XFER_BYTES
        && late.ret == XFER_BYTES as i32
        && late.errno == 0
        && late.pending_after
        && late.phase == Some(Phase::PostCommit)
        && delivered.interrupted()
        && delivered.phase == Some(Phase::PostCommit)
        && !delivered.pending_after;
    s.add(
        "相位 PostCommit：迟到信号不丢、下一次入口交付",
        late_ok,
        "已提交的调用必须返回字节数，信号转欠账并在下一次调用入口兑现",
    );

    // ⑦ 100 次注入：四相位各 25。
    let r = run_100_injections();
    let uniform = r.phase_uniform(PHASE_SHOTS);
    let total_ok = r.ledger.injected == INJECT_TOTAL && r.histograms_closed();
    s.add(
        "100 次注入：逐相位直方图各 25 且总和守恒",
        uniform && total_ok,
        "分层排布（i%4）必须给出每相位 25 发，直方图总和等于注入数",
    );

    // ⑧ 逐 syscall 直方图：34/33/33。
    let mut sys_ok = r.per_sys.len() == SYS_N;
    let want_sys = [34u32, 33u32, 33u32];
    let mut si = 0usize;
    while si < SYS_N {
        match (r.per_sys.get(si), want_sys.get(si)) {
            (Some(got), Some(want)) => {
                if got != want {
                    sys_ok = false;
                }
            }
            _ => sys_ok = false,
        }
        si += 1;
    }
    s.add(
        "100 次注入：逐 syscall 计数 34/33/33",
        sys_ok,
        "i%3 分层在 100 发下的确定分布，任何偏差都说明调度表被改",
    );

    // ⑨ 账本三分：75 立即 + 25 迟到。
    let split_ok = r.ledger.direct == 75
        && r.ledger.late == 25
        && r.ledger.restarts == 0
        && r.ledger.injected == 100;
    s.add(
        "账本三分：75 立即 EINTR + 25 迟到欠账",
        split_ok,
        "四相位等分下只有 PostCommit 走迟到路径，其余三相位立即中断",
    );

    // ⑩ 欠账结算：25 次 flush 后 pending 归零。
    let settle_ok = r.flushes == 25
        && r.ledger.settled()
        && !r.pending_left
        && r.ledger.user_visible() == 100;
    s.add(
        "欠账结算：25 次 flush 后 100 次 EINTR 全部可见",
        settle_ok,
        "迟到欠账必须逐笔兑现，pending 归零且用户态可见数等于注入数",
    );

    // ⑪ 守恒律与逃逸为零。
    s.add(
        "账本守恒：injected = direct + late + restarts 且逃逸为 0",
        r.ledger.conserved() && r.ledger.escaped == 0,
        "逃逸由差集真实算出，不为零说明有注入既没中断也没记账",
    );

    // ⑫ 可观测性：一发一条观测。
    s.add(
        "可观测性：100 发各产出一条命中相位的观测",
        r.observed == INJECT_TOTAL && r.ledger.injected == r.observed,
        "「可观测」的可执行定义就是发射数与带相位观测数相等",
    );

    // ⑬ 容量：8 槽满后第 9 发 EBUSY，且不覆盖未发射预约。
    let mut cap = Generator::new();
    let mut armed = 0u32;
    let mut k = 0u32;
    while k < RING_CAP as u32 {
        let shot = Shot::new(Phase::PreWait, Sys::Read, (k as u16).saturating_add(1), false);
        if cap.arm(shot).is_ok() {
            armed = armed.saturating_add(1);
        }
        k += 1;
    }
    let ninth = cap.arm(Shot::new(Phase::PreWait, Sys::Read, 9, false));
    let cap_ok = armed == RING_CAP as u32 && ninth == Err(EBUSY) && cap.spent() == RING_CAP as u32;
    s.add(
        "容量：8 槽填满后第 9 发回 EBUSY",
        cap_ok,
        "定容环不得覆盖未发射预约，满表必须显性回忙",
    );

    // ⑭ 次序：轮次倒退回 EINVAL，且环状态不变。
    let mut ord = Generator::new();
    let first = ord.arm(Shot::new(Phase::InWait, Sys::Read, 5, false));
    let back = ord.arm(Shot::new(Phase::InWait, Sys::Read, 5, false));
    let back2 = ord.arm(Shot::new(Phase::InWait, Sys::Read, 4, false));
    let ord_ok = first.is_ok()
        && back == Err(EINVAL)
        && back2 == Err(EINVAL)
        && ord.spent() == 1;
    s.add(
        "次序：轮次未严格递增回 EINVAL 且不占槽",
        ord_ok,
        "预约轮次是统计对齐的锚点，倒退或重复都必须被拒在写槽之前",
    );

    // ⑮ 预算：预算 2 时第 3 发 EBUSY。
    let mut bud = Generator::with_budget(2);
    let b1 = bud.arm(Shot::new(Phase::PreWait, Sys::Read, 1, false));
    let b2 = bud.arm(Shot::new(Phase::PreWait, Sys::Read, 2, false));
    let b3 = bud.arm(Shot::new(Phase::PreWait, Sys::Read, 3, false));
    s.add(
        "预算：2 发额度用尽后第 3 发回 EBUSY",
        b1.is_ok() && b2.is_ok() && b3 == Err(EBUSY) && bud.spent() == 2,
        "预算与实际发射名额必须同源，额度只能由发射/掩掉消耗",
    );

    // ⑯ 掩码：关掉 PostCommit 后注入数降为 75 且无欠账。
    let masked = run_all(0x07);
    let mask_ok = masked.ledger.injected == 75
        && masked.ledger.late == 0
        && masked.ledger.direct == 75
        && !masked.pending_left
        && masked.flushes == 0
        && masked.per_phase.get(Phase::PostCommit.rank()).copied() == Some(0);
    s.add(
        "掩码：关掉 PostCommit 后只发射 75 发且零欠账",
        mask_ok,
        "被掩掉的预约只计 masked，不得混入注入统计或留下欠账",
    );

    // ⑰ SA_RESTART：nanosleep 回 EINTR，read 被重启吸收。
    let rc = run_restart_case();
    let restart_ok = rc.hard.interrupted()
        && rc.hard.restarted == false
        && rc.soft.restarted
        && rc.soft.errno == 0
        && rc.soft.got == XFER_BYTES
        && rc.restarts == 1;
    s.add(
        "重启语义：nanosleep 回 EINTR、read 被 SA_RESTART 吸收",
        restart_ok,
        "同一发射点上重启与否只由调用族与调用方声明决定",
    );

    // ⑱ 双跑一致。
    let fp_a = corpus_fingerprint();
    let fp_b = corpus_fingerprint();
    s.add(
        "双跑一致：100 发轨迹指纹相等",
        fp_a == fp_b,
        "同调度两次重放必须逐字节相同，隐藏状态即失败",
    );

    // ⑲ 码值两两唯一。
    let mut uniq = true;
    let mut i = 0usize;
    while i < GenCode::ALL.len() {
        let mut j = i + 1;
        while j < GenCode::ALL.len() {
            if let (Some(a), Some(b)) = (GenCode::ALL.get(i), GenCode::ALL.get(j)) {
                if a.val() == b.val() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    s.add(
        "诊断码：12 个码值两两唯一",
        uniq,
        "段内闭集码重复会让判据无法定位到唯一模块",
    );

    // ⑳ 文案闭集非空。
    s.add(
        "文案闭集：12 个诊断码均有中文处置建议",
        GenCode::ALL.iter().all(|c| !c.detail().is_empty()),
        "每个码都要能给出可操作的一句话，否则等于没有诊断",
    );

    // 动态明细 1：注入账目快照。
    let text_led = format!(
        "注入 {} = 立即 {} + 迟到 {} + 重启 {}；欠账交付 {}；逃逸 {}",
        r.ledger.injected,
        r.ledger.direct,
        r.ledger.late,
        r.ledger.restarts,
        r.ledger.late_delivered,
        r.ledger.escaped
    );
    s.add_dyn(
        "注入账目快照",
        r.ledger.conserved() && r.ledger.settled(),
        &text_led,
    );

    // 动态明细 2：直方图快照。
    let text_hist = format!(
        "相位 [{},{},{},{}]；sys [read {}, write {}, ns {}]",
        r.per_phase.get(0).copied().unwrap_or(0),
        r.per_phase.get(1).copied().unwrap_or(0),
        r.per_phase.get(2).copied().unwrap_or(0),
        r.per_phase.get(3).copied().unwrap_or(0),
        r.per_sys.get(Sys::Read.idx()).copied().unwrap_or(0),
        r.per_sys.get(Sys::Write.idx()).copied().unwrap_or(0),
        r.per_sys.get(Sys::NanoSleep.idx()).copied().unwrap_or(0)
    );
    s.add_dyn("直方图快照", r.histograms_closed(), &text_hist);

    // 动态明细 3：指纹快照。
    let text_fp = format!("100 发轨迹指纹 0x{:08X}", fp_a);
    s.add_dyn("指纹快照", fp_a == fp_b, &text_fp);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ring_is_fifo_and_bounded() {
        let mut ring = Ring::new();
        let a = ring.arm(Shot::new(Phase::PreWait, Sys::Read, 1, false));
        let b = ring.arm(Shot::new(Phase::InWait, Sys::Read, 2, false));
        assert_eq!(a, Ok(0));
        assert_eq!(b, Ok(1));
        assert_eq!(ring.used(), 2);
        assert_eq!(ring.peek().map(|s| s.round), Some(1));
        let first = ring.take();
        assert_eq!(first.map(|s| s.round), Some(1));
        assert_eq!(ring.used(), 1);
        assert_eq!(ring.last_round(), 2);
    }

    #[test]
    fn ring_rejects_full_and_regress() {
        let mut ring = Ring::new();
        let mut k = 0u32;
        while k < RING_CAP as u32 {
            let r = ring.arm(Shot::new(Phase::PreWait, Sys::Read, (k as u16) + 1, false));
            assert_eq!(r.is_ok(), true);
            k += 1;
        }
        assert_eq!(
            ring.arm(Shot::new(Phase::PreWait, Sys::Read, 9, false)),
            Err(EBUSY)
        );
        assert_eq!(ring.used(), RING_CAP);
        let mut fresh = Ring::new();
        assert_eq!(
            fresh.arm(Shot::new(Phase::InWait, Sys::Read, 3, false)).is_ok(),
            true
        );
        assert_eq!(
            fresh.arm(Shot::new(Phase::InWait, Sys::Read, 3, false)),
            Err(EINVAL)
        );
        assert_eq!(
            fresh.arm(Shot::new(Phase::InWait, Sys::Read, 2, false)),
            Err(EINVAL)
        );
        assert_eq!(fresh.used(), 1);
    }

    #[test]
    fn prewait_observes_entry_false() {
        let (t, led, pending) = run_single(Phase::PreWait, Sys::Read, XFER_BYTES, false);
        assert_eq!(t.wait_entered, false);
        assert_eq!(t.committed, false);
        assert_eq!(t.errno, EINTR);
        assert_eq!(t.ret, -1);
        assert_eq!(pending, false);
        assert_eq!(led.direct, 1);
        assert_eq!(led.conserved(), true);
    }

    #[test]
    fn inwait_and_postwake_are_entered() {
        let (a, _, _) = run_single(Phase::InWait, Sys::Read, XFER_BYTES, false);
        let (b, _, _) = run_single(Phase::PostWake, Sys::Read, XFER_BYTES, false);
        assert_eq!(a.wait_entered, true);
        assert_eq!(a.committed, false);
        assert_eq!(b.wait_entered, true);
        assert_eq!(b.committed, false);
        assert_eq!(a.got, 0);
        assert_eq!(b.got, 0);
    }

    #[test]
    fn postcommit_defers_and_flush_delivers() {
        let (late, delivered) = run_late_pair();
        assert_eq!(late.committed, true);
        assert_eq!(late.got, XFER_BYTES);
        assert_eq!(late.pending_after, true);
        assert_eq!(late.errno, 0);
        assert_eq!(delivered.interrupted(), true);
        assert_eq!(delivered.pending_after, false);
        assert_eq!(delivered.committed, false);
    }

    #[test]
    fn restart_depends_on_call_family() {
        let rc = run_restart_case();
        assert_eq!(rc.hard.errno, EINTR);
        assert_eq!(rc.hard.restarted, false);
        assert_eq!(rc.soft.restarted, true);
        assert_eq!(rc.soft.errno, 0);
        assert_eq!(rc.soft.got, XFER_BYTES);
        assert_eq!(rc.restarts, 1);
    }

    #[test]
    fn hundred_injections_statistics() {
        let r = run_100_injections();
        assert_eq!(r.ledger.injected, 100);
        assert_eq!(r.ledger.direct, 75);
        assert_eq!(r.ledger.late, 25);
        assert_eq!(r.ledger.restarts, 0);
        assert_eq!(r.ledger.escaped, 0);
        assert_eq!(r.flushes, 25);
        assert_eq!(r.observed, 100);
        assert_eq!(r.phase_uniform(PHASE_SHOTS), true);
        assert_eq!(r.histograms_closed(), true);
        assert_eq!(r.ledger.settled(), true);
        assert_eq!(r.ledger.user_visible(), 100);
        assert_eq!(r.pending_left, false);
    }

    #[test]
    fn syscall_histogram_is_34_33_33() {
        let r = run_100_injections();
        assert_eq!(r.per_sys.get(0).copied(), Some(34));
        assert_eq!(r.per_sys.get(1).copied(), Some(33));
        assert_eq!(r.per_sys.get(2).copied(), Some(33));
    }

    #[test]
    fn mask_suppresses_without_accounting() {
        let r = run_all(0x07);
        assert_eq!(r.ledger.injected, 75);
        assert_eq!(r.ledger.direct, 75);
        assert_eq!(r.ledger.late, 0);
        assert_eq!(r.flushes, 0);
        assert_eq!(r.pending_left, false);
        assert_eq!(r.per_phase.get(Phase::PostCommit.rank()).copied(), Some(0));
    }

    #[test]
    fn budget_caps_arming() {
        let mut g = Generator::with_budget(2);
        assert_eq!(g.arm(Shot::new(Phase::PreWait, Sys::Read, 1, false)).is_ok(), true);
        assert_eq!(g.arm(Shot::new(Phase::PreWait, Sys::Read, 2, false)).is_ok(), true);
        assert_eq!(
            g.arm(Shot::new(Phase::PreWait, Sys::Read, 3, false)),
            Err(EBUSY)
        );
        assert_eq!(g.spent(), 2);
    }

    #[test]
    fn fingerprint_is_reproducible() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
        assert_eq!(run_100_injections().checksum, corpus_fingerprint());
    }

    #[test]
    fn schedule_is_stratified_and_bounded() {
        assert_eq!(schedule(0).map(|s| s.phase), Some(Phase::PreWait));
        assert_eq!(schedule(3).map(|s| s.phase), Some(Phase::PostCommit));
        assert_eq!(schedule(4).map(|s| s.phase), Some(Phase::PreWait));
        assert_eq!(schedule(100).is_none(), true);
        assert_eq!(schedule(99).map(|s| s.round), Some(100));
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx9312_checks();
        assert_eq!(s.all_passed(), true, "{}", s.red_items_text());
    }
}
