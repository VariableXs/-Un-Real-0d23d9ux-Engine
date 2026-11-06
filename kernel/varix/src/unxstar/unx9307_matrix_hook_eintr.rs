//! UNX-F9307 阻塞挂点 EINTR 矩阵（waitqueue 等待中断窗） · 0x7BA40段
//! ─────────────────────────────────────────────────────────────
//! 判据原文（任务板 F9307 逐字）：
//!   ① 纯功能代码 ≥340 行；
//!   ② 判据原文：UNX-F9307-J1 等待队列中断注入判据过，全 EINTR 格过，
//!      唤醒零丢失格过，与 A3 fake 原语对账格过；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//!   ① 等待队列走「票号 + 纪元」两轴：入队者领单调票号，并把全局唤醒纪元
//!      抄进 born；判唤醒只做 `wake_epoch > born` 一次比较，不依赖出队次序。
//!   ② 决定次序冻结四段：唤醒 > 无信号 > 重定位 > EINTR。唤醒先于信号保证
//!      事件就绪时信号不得吞数据；信号先于重定位保证 SA_RESTART 只作用于
//!      「无事件」那一支。
//!   ③ 零丢失格：八入队者全 Enqueued 后一次 wake_all，投递数必须等于入队数，
//!      且不得残留「born 早于当前纪元却仍 Enqueued」者。
//!   ④ 十格 = 三种唤醒相位 × 四种 (signal, SA_RESTART) 组合取十，每格十轮复算。
//!   ⑤ 与 A3 fake 原语对账：fake 三缺陷（吞唤醒、漏重定位、错报 EAGAIN）在六
//!      探针里必须恰暴露三次，且下标落在冻结表内；无缺陷 fake 必须全一致。
//!   ⑥ 注入账 injected == detected + escaped，escaped 恒 0；诊断码 12 个闭集。
//! ─────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 诊断段基址（F9307 独占 0x7BA40..=0x7BA7F）。
pub const SEG_BASE: u32 = 0x7BA40;
/// 段窗口宽度（诊断码 +0x01..=+0x0C 必须落在窗内）。
pub const SEG_WINDOW: u32 = 0x40;
/// 等待槽容量：八个挂点，与冻结矩阵的零丢失格同宽。
pub const WAIT_SLOTS: usize = 8;
/// 单格注入复测轮数。
pub const INJECT_ROUNDS: u32 = 10;
/// 冻结矩阵格数。
pub const MATRIX_CELLS: usize = 10;
/// fake 原语对账探针数。
pub const RECONCILE_PROBES: usize = 6;
/// 已声明的 fake 分歧数（三个缺陷各暴露一次）。
pub const DECLARED_DIVERGENCES: u32 = 3;

/// Linux errno（正数口径）。
pub const EPERM: i32 = 1;
/// 票号不存在（已出队/从未入队）。
pub const ESRCH: i32 = 3;
/// 等待被信号中断。
pub const EINTR: i32 = 4;
/// 非阻塞面无可等待对象。
pub const EAGAIN: i32 = 11;
/// 等待对象坏地址。
pub const EFAULT: i32 = 14;
/// 状态与请求不匹配。
pub const EINVAL: i32 = 22;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..=+0x0C 闭集，与 F9308 段不重叠）。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HookCode {
    /// 唤醒令牌被吞
    WakeLost,
    /// 入队前唤醒被误当作已唤醒
    WakeBeforeEnqueue,
    /// 信号存在却返回了数据
    SignalSwallowed,
    /// 一次唤醒记了两笔
    DoubleWake,
    /// 重定位位判反
    RestartMismatch,
    /// 槽满未回 EAGAIN
    QueueOverflow,
    /// 票号非单调
    TicketOrder,
    /// 陈旧票号被接受
    StaleTicket,
    /// fake 原语分歧越界
    FakeSkew,
    /// 注入账倒退
    LedgerRegress,
    /// 双跑指纹漂移
    FingerprintDrift,
    /// 段位越界
    SegmentViolation,
}

impl HookCode {
    /// 码值 = 段基址 + 1 + 变体序号；量程用 u32，段值 0x7BA40 超出 u16。
    pub fn code(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文诊断一句话（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            HookCode::WakeLost => "唤醒令牌丢失：投递数小于入队数；请检查 wake_all 是否跳过 Enqueued 槽",
            HookCode::WakeBeforeEnqueue => "入队前唤醒被判为已唤醒：born 抄早了纪元；请先抄纪元再挂链",
            HookCode::SignalSwallowed => "信号吞掉已就绪事件：请在信号判定之前先判唤醒",
            HookCode::DoubleWake => "同一挂点被计两次唤醒：wake_all 必须跳过已 Woken 槽",
            HookCode::RestartMismatch => "SA_RESTART 判定反了：有重定位位必须回静默重启而非 EINTR",
            HookCode::QueueOverflow => "等待槽满未回 EAGAIN：第八个挂点之后必须显性拒绝",
            HookCode::TicketOrder => "票号非单调：并发入队不得复用旧票",
            HookCode::StaleTicket => "陈旧票号被接受：出队后票号必须失效并回 ESRCH",
            HookCode::FakeSkew => "fake 原语分歧数与声明不符：三缺陷必须各暴露一次",
            HookCode::LedgerRegress => "注入账倒退：三格必须同步推进且只增不减",
            HookCode::FingerprintDrift => "双跑指纹不一致：判定路径混入了非确定状态",
            HookCode::SegmentViolation => "段位越界：本模块必须独占 0x7BA40 的 0x40 窗口",
        }
    }

    /// 闭集全量（唯一性与文案覆盖用）。
    pub const ALL: &[HookCode] = &[
        HookCode::WakeLost, HookCode::WakeBeforeEnqueue, HookCode::SignalSwallowed,
        HookCode::DoubleWake, HookCode::RestartMismatch, HookCode::QueueOverflow,
        HookCode::TicketOrder, HookCode::StaleTicket, HookCode::FakeSkew,
        HookCode::LedgerRegress, HookCode::FingerprintDrift, HookCode::SegmentViolation,
    ];
}

// ── 挂点状态 ──────────────────────────────────────────────────────────
/// 单个挂点的五态机（不可回退）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WaitState {
    /// 空槽
    Idle,
    /// 已挂链、未收到唤醒
    Enqueued,
    /// 收到唤醒、等待出队
    Woken,
    /// 被信号打断、等待出队
    Interrupted,
    /// 出队完毕（终态，票号失效）
    Done,
}

impl WaitState {
    /// 闭集编码（账本/指纹使用）：0 Idle、1 Enqueued、2 Woken、3 Interrupted、4 Done。
    pub fn tag(self) -> u8 {
        match self {
            WaitState::Idle => 0, WaitState::Enqueued => 1, WaitState::Woken => 2,
            WaitState::Interrupted => 3, WaitState::Done => 4,
        }
    }

    /// 是否仍挂在队列上（可被唤醒或被打断）。
    pub fn linked(self) -> bool {
        matches!(self, WaitState::Enqueued | WaitState::Woken | WaitState::Interrupted)
    }
}

/// 一个挂点记录。`born` 是入队瞬间的唤醒纪元快照，判定唤醒只比较它。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Waiter {
    pub used: bool,
    pub pid: u32,
    pub ticket: u64,
    pub born: u64,
    pub state: WaitState,
}

impl Waiter {
    /// 空槽常量（`[Waiter::EMPTY; N]` 的 Copy 种子）。
    pub const EMPTY: Waiter = Waiter {
        used: false,
        pid: 0,
        ticket: 0,
        born: 0,
        state: WaitState::Idle,
    };
}

// ── 等待队列 ──────────────────────────────────────────────────────────
/// 定容等待队列：八个槽 + 单调票号 + 单调唤醒纪元 + 四类累计计数。
///
/// 不变量：
///   · next_ticket 只增不减，任何一次入队都拿到新票；
///   · wake_epoch 只增不减，wake_all 之后所有 Enqueued 者必须转 Woken；
///   · 任何失败路径都不得修改槽位（只推进 rejects 类计数）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct WaitQueue {
    slots: [Waiter; WAIT_SLOTS],
    next_ticket: u64,
    wake_epoch: u64,
    enqueued: u32,
    wakes_delivered: u32,
    interrupts: u32,
    rejects: u32,
}

impl WaitQueue {
    /// 空队列（纪元从 0 起，票号从 1 起，0 留作无效票）。
    pub const fn new() -> WaitQueue {
        WaitQueue {
            slots: [Waiter::EMPTY; WAIT_SLOTS],
            next_ticket: 1,
            wake_epoch: 0,
            enqueued: 0,
            wakes_delivered: 0,
            interrupts: 0,
            rejects: 0,
        }
    }

    /// 当前挂链数（Enqueued + Woken + Interrupted）。
    pub fn depth(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < WAIT_SLOTS {
            if let Some(w) = self.slots.get(i) {
                if w.state.linked() {
                    n = n.saturating_add(1);
                }
            }
            i += 1;
        }
        n
    }

    /// 票号水位（下一个待发票）。
    pub fn ticket_watermark(&self) -> u64 { self.next_ticket }
    /// 唤醒纪元水位。
    pub fn epoch(&self) -> u64 { self.wake_epoch }
    /// 累计入队次数。
    pub fn enqueued(&self) -> u32 { self.enqueued }
    /// 累计投递唤醒数。
    pub fn delivered(&self) -> u32 { self.wakes_delivered }
    /// 累计中断次数。
    pub fn interrupted(&self) -> u32 { self.interrupts }
    /// 累计拒绝次数（槽满/错态）。
    pub fn rejects(&self) -> u32 { self.rejects }
    /// 槽视图（供判据与测试逐槽核对）。
    pub fn slot(&self, i: usize) -> Option<&Waiter> { self.slots.get(i) }

    /// 按票号定位槽位；找不到返回 None。
    pub fn find(&self, ticket: u64) -> Option<usize> {
        let mut i = 0usize;
        while i < WAIT_SLOTS {
            if let Some(w) = self.slots.get(i) {
                if w.used && w.ticket == ticket {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 入队：返回新票号；槽满回 EAGAIN，票号 0 回 EINVAL（调用方不得外传）。
    ///
    /// 关键顺序：先抄 wake_epoch 到 born，再置 state=Enqueued 并占用槽位。
    /// 反序会让「入队瞬间到达的唤醒」被 born 吃掉——这正是 fake 原语的缺陷面。
    pub fn enqueue(&mut self, pid: u32) -> Result<u64, i32> {
        if pid == 0 {
            self.rejects = self.rejects.saturating_add(1);
            return Err(EINVAL);
        }
        let mut i = 0usize;
        while i < WAIT_SLOTS {
            let free = match self.slots.get(i) {
                Some(w) => !w.used,
                None => false,
            };
            if free {
                let ticket = self.next_ticket;
                let born = self.wake_epoch;
                if let Some(slot) = self.slots.get_mut(i) {
                    slot.used = true;
                    slot.pid = pid;
                    slot.ticket = ticket;
                    slot.born = born;
                    slot.state = WaitState::Enqueued;
                }
                self.next_ticket = self.next_ticket.saturating_add(1);
                self.enqueued = self.enqueued.saturating_add(1);
                return Ok(ticket);
            }
            i += 1;
        }
        self.rejects = self.rejects.saturating_add(1);
        Err(EAGAIN)
    }

    /// 一次性唤醒全部 Enqueued 挂点：纪元 +1，返回本次投递数。
    ///
    /// 已 Woken / Interrupted 的挂点不再计数，避免同一次唤醒记两笔（DoubleWake）。
    pub fn wake_all(&mut self) -> u32 {
        self.wake_epoch = self.wake_epoch.saturating_add(1);
        let mut delivered = 0u32;
        let mut i = 0usize;
        while i < WAIT_SLOTS {
            let is_enqueued = match self.slots.get(i) {
                Some(w) => w.used && matches!(w.state, WaitState::Enqueued),
                None => false,
            };
            if is_enqueued {
                if let Some(slot) = self.slots.get_mut(i) {
                    slot.state = WaitState::Woken;
                    // born 保持入队时刻的纪元快照，不改写成当前纪元：
                    // observe 靠 wake_epoch > born 判「唤醒在入队之后到达」，
                    // 把 born 抬到当前纪元会让已唤醒者永远看不到 Wake。
                }
                delivered = delivered.saturating_add(1);
            }
            i += 1;
        }
        self.wakes_delivered = self.wakes_delivered.saturating_add(delivered);
        delivered
    }

    /// 定位投放信号：只对 Enqueued 生效；已 Woken 说明唤醒先到，信号让路（EINVAL）。
    pub fn interrupt(&mut self, ticket: u64) -> Result<u32, i32> {
        let idx = match self.find(ticket) {
            Some(i) => i,
            None => return Err(ESRCH),
        };
        let pid = match self.slots.get(idx) {
            Some(w) => w.pid,
            None => return Err(ESRCH),
        };
        let state = match self.slots.get(idx) {
            Some(w) => w.state,
            None => return Err(ESRCH),
        };
        if matches!(state, WaitState::Enqueued) {
            if let Some(slot) = self.slots.get_mut(idx) {
                slot.state = WaitState::Interrupted;
            }
            self.interrupts = self.interrupts.saturating_add(1);
            Ok(pid)
        } else {
            self.rejects = self.rejects.saturating_add(1);
            Err(EINVAL)
        }
    }

    /// 观察一个挂点在当前信号面下的走向（不产生副作用）。
    ///
    /// 唤醒判定用纪元比较：wake_epoch > born 即「唤醒在入队之后到达」。
    pub fn observe(&self, ticket: u64, sig: bool, restart: bool) -> Result<HookOutcome, i32> {
        let idx = match self.find(ticket) {
            Some(i) => i,
            None => return Err(ESRCH),
        };
        let born = match self.slots.get(idx) {
            Some(w) => w.born,
            None => return Err(ESRCH),
        };
        let woke = self.wake_epoch > born;
        Ok(hook_decide(woke, sig, restart))
    }

    /// 出队：Woken 正常返回，Interrupted 返回中断态（调用方转 EINTR），
    /// Enqueued 仍等待回 EAGAIN，票号失效后回 ESRCH。
    pub fn dequeue(&mut self, ticket: u64) -> Result<WaitState, i32> {
        let idx = match self.find(ticket) {
            Some(i) => i,
            None => return Err(ESRCH),
        };
        let state = match self.slots.get(idx) {
            Some(w) => w.state,
            None => return Err(ESRCH),
        };
        match state {
            WaitState::Woken | WaitState::Interrupted => {
                if let Some(slot) = self.slots.get_mut(idx) {
                    slot.state = WaitState::Done;
                    slot.used = false;
                }
                Ok(state)
            }
            WaitState::Enqueued => Err(EAGAIN),
            _ => {
                self.rejects = self.rejects.saturating_add(1);
                Err(ESRCH)
            }
        }
    }

    /// 丢唤醒计数：born 早于当前纪元却仍 Enqueued 的残留者数量（必须恒为 0）。
    pub fn lost_wakes(&self) -> u32 {
        let epoch = self.wake_epoch;
        let mut lost = 0u32;
        let mut i = 0usize;
        while i < WAIT_SLOTS {
            if let Some(w) = self.slots.get(i) {
                if w.used && matches!(w.state, WaitState::Enqueued) && w.born < epoch {
                    lost = lost.saturating_add(1);
                }
            }
            i += 1;
        }
        lost
    }

    /// 结构性摘要（入队/投递/中断/拒绝四账），供零破坏比对使用。
    pub fn shape(&self) -> (u32, u32, u32, u32) {
        (self.enqueued, self.wakes_delivered, self.interrupts, self.rejects)
    }
}

// ── 挂点裁决 ──────────────────────────────────────────────────────────
/// 阻塞挂点返回面。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HookOutcome {
    /// 继续阻塞：无唤醒、无信号。
    Wait,
    /// 唤醒先到：返回就绪事件，信号不得吞。
    Wake,
    /// SA_RESTART：静默重启，用户态不可见。
    Restart,
    /// 非重定位信号：回 EINTR。
    Interrupt,
}

impl HookOutcome {
    /// 闭集编码：0 Wait、1 Wake、2 Restart、3 Interrupt。
    pub fn tag(self) -> u16 {
        match self {
            HookOutcome::Wait => 0, HookOutcome::Wake => 1,
            HookOutcome::Restart => 2, HookOutcome::Interrupt => 3,
        }
    }

    /// 该走向对外返回的 errno（0 表示成功返回）。
    pub fn errno(self) -> i32 {
        match self { HookOutcome::Interrupt => EINTR, _ => 0 }
    }
}

/// 冻结裁决次序：唤醒 → 无信号 → 重定位 → EINTR。次序即语义：唤醒提前到信号
/// 之前才能防止吞事件，信号提前到重定位之前才能让 SA_RESTART 只覆盖无事件支。
pub fn hook_decide(woke: bool, sig: bool, restart: bool) -> HookOutcome {
    if woke {
        HookOutcome::Wake
    } else if !sig {
        HookOutcome::Wait
    } else if restart {
        HookOutcome::Restart
    } else {
        HookOutcome::Interrupt
    }
}

// ── 十格冻结矩阵 ──────────────────────────────────────────────────────
/// 唤醒相位：相对入队时刻的位置。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WakePhase {
    /// 全程无唤醒
    None,
    /// 唤醒早于入队（纪元已推进，入队者 born 采到新值，不视为已唤醒）
    BeforeEnqueue,
    /// 唤醒晚于入队（born 落后纪元，必判 Wake）
    AfterEnqueue,
}

impl WakePhase {
    /// 闭集编码 0/1/2。
    pub fn tag(self) -> u8 {
        match self {
            WakePhase::None => 0,
            WakePhase::BeforeEnqueue => 1,
            WakePhase::AfterEnqueue => 2,
        }
    }
}

/// 矩阵一行：唤醒相位 × 信号存在 × 重定位位 → 期望走向。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct HookCell {
    pub phase: WakePhase,
    pub sig: bool,
    pub restart: bool,
    pub expect: HookOutcome,
}

/// 十行冻结矩阵（三种相位 × 四种信号组合，去掉两种已由相位覆盖的重复面）。
pub const HOOK_MATRIX: [HookCell; MATRIX_CELLS] = [
    HookCell { phase: WakePhase::None, sig: false, restart: false, expect: HookOutcome::Wait },
    HookCell { phase: WakePhase::None, sig: true, restart: false, expect: HookOutcome::Interrupt },
    HookCell { phase: WakePhase::None, sig: true, restart: true, expect: HookOutcome::Restart },
    HookCell { phase: WakePhase::None, sig: false, restart: true, expect: HookOutcome::Wait },
    HookCell { phase: WakePhase::BeforeEnqueue, sig: false, restart: false, expect: HookOutcome::Wait },
    HookCell { phase: WakePhase::BeforeEnqueue, sig: true, restart: false, expect: HookOutcome::Interrupt },
    HookCell { phase: WakePhase::BeforeEnqueue, sig: true, restart: true, expect: HookOutcome::Restart },
    HookCell { phase: WakePhase::AfterEnqueue, sig: true, restart: false, expect: HookOutcome::Wake },
    HookCell { phase: WakePhase::AfterEnqueue, sig: true, restart: true, expect: HookOutcome::Wake },
    HookCell { phase: WakePhase::AfterEnqueue, sig: false, restart: true, expect: HookOutcome::Wake },
];

/// 注入账：三格同步推进，守恒关系 injected == detected + escaped。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct WakeLedger {
    pub injected: u32,
    pub detected: u32,
    pub escaped: u32,
}

impl WakeLedger {
    /// 零账。
    pub const fn zero() -> WakeLedger {
        WakeLedger { injected: 0, detected: 0, escaped: 0 }
    }

    /// 记一次注入并回传守恒结论（false 表示账目已破，调用方必须显性落红）。
    pub fn charge(&mut self, caught: bool) -> bool {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        self.conserved()
    }

    /// 守恒：检出 + 逃逸 == 注入。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 矩阵复算报告。
#[derive(Clone, Copy, Debug)]
pub struct HookReport {
    /// 每格命中轮数（应恒等于 INJECT_ROUNDS）。
    pub per_cell: [u32; MATRIX_CELLS],
    /// 注入账。
    pub ledger: WakeLedger,
    /// 期望 EINTR 的格数。
    pub eintr_cells: u32,
    /// 期望 EINTR 的格数中每轮都回 4 的格数。
    pub eintr_cells_ok: u32,
    /// 账本守恒是否始终成立。
    pub conserved: bool,
}

/// 单格单轮仿真：建队列 → 可选前置唤醒 → 入队 → 可选后置唤醒 → 观察。
fn simulate_cell(cell: HookCell, pid: u32) -> Option<HookOutcome> {
    let mut q = WaitQueue::new();
    if matches!(cell.phase, WakePhase::BeforeEnqueue) {
        q.wake_all();
    }
    let ticket = match q.enqueue(pid) {
        Ok(t) => t,
        Err(_) => return None,
    };
    if matches!(cell.phase, WakePhase::AfterEnqueue) {
        q.wake_all();
    }
    q.observe(ticket, cell.sig, cell.restart).ok()
}

/// 十格 × 十轮复算：逐格命中数、注入账、EINTR 格覆盖。
pub fn run_hook_matrix() -> HookReport {
    let mut report = HookReport {
        per_cell: [0u32; MATRIX_CELLS],
        ledger: WakeLedger::zero(),
        eintr_cells: 0,
        eintr_cells_ok: 0,
        conserved: true,
    };
    let mut ci = 0usize;
    while ci < HOOK_MATRIX.len() {
        if let Some(cell) = HOOK_MATRIX.get(ci).copied() {
            if matches!(cell.expect, HookOutcome::Interrupt) {
                report.eintr_cells = report.eintr_cells.saturating_add(1);
            }
            let mut round = 0u32;
            let mut all_eintr_ok = true;
            while round < INJECT_ROUNDS {
                let got = simulate_cell(cell, 900 + ci as u32);
                let caught = got == Some(cell.expect);
                if !report.ledger.charge(caught) {
                    report.conserved = false;
                }
                if caught {
                    if let Some(slot) = report.per_cell.get_mut(ci) {
                        *slot = slot.saturating_add(1);
                    }
                }
                if matches!(cell.expect, HookOutcome::Interrupt) && got != Some(HookOutcome::Interrupt)
                {
                    all_eintr_ok = false;
                }
                round += 1;
            }
            if matches!(cell.expect, HookOutcome::Interrupt) && all_eintr_ok {
                report.eintr_cells_ok = report.eintr_cells_ok.saturating_add(1);
            }
        }
        ci += 1;
    }
    report
}

// ── 唤醒零丢失格 ──────────────────────────────────────────────────────
/// 零丢失复算结果：(入队数, 投递数, 残留丢唤醒数, 排空后仍在链数)。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LossProbe {
    pub enqueued: u32,
    pub delivered: u32,
    pub lost: u32,
    pub stranded: u32,
}

/// 八挂点全入队 → 一次 wake_all → 逐一出队，核对投递与零残留。
pub fn zero_wake_loss() -> LossProbe {
    let mut q = WaitQueue::new();
    let mut i = 0u32;
    let mut enq = 0u32;
    while i < WAIT_SLOTS as u32 {
        if q.enqueue(1000 + i).is_ok() {
            enq = enq.saturating_add(1);
        }
        i += 1;
    }
    let delivered = q.wake_all();
    let lost = q.lost_wakes();
    // 排空：每个 Woken 槽必须能按票号出队。
    let mut drained = 0u32;
    let mut t = 1u64;
    while drained < enq {
        match q.dequeue(t) {
            Ok(WaitState::Woken) => drained = drained.saturating_add(1),
            _ => {
                // 票号失效即停止推进，避免无限循环。
                break;
            }
        }
        t = t.saturating_add(1);
    }
    // 排空后仍挂链者即残留（应为 0）。这是与 lost 互补的第二面证据：
    // lost 看「唤醒前入队」，stranded 看「唤醒后能否按票号真正离开」。
    LossProbe {
        enqueued: enq,
        delivered,
        lost,
        stranded: q.depth(),
    }
}

// ── A3 fake 原语对账 ─────────────────────────────────────────────────
/// A3 fake 等待原语：三个已声明缺陷，逐项可开关，便于对账时逐缺陷归因。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FakePrimitive {
    /// 缺陷一：信号存在时吞掉唤醒（把 Wake 降级为 Restart/Wait）
    pub drop_wake_on_signal: bool,
    /// 缺陷二：漏看重定位位（SA_RESTART 也报 EINTR）
    pub ignore_restart: bool,
    /// 缺陷三：错报 EAGAIN 而非 EINTR（把中断降级为继续等待）
    pub report_again: bool,
}

impl FakePrimitive {
    /// 三缺陷全开（对账主样本）。
    pub const FAULTY: FakePrimitive =
        FakePrimitive { drop_wake_on_signal: true, ignore_restart: true, report_again: true };
    /// 三缺陷全关（对照样本：必须与权威逐个一致）。
    pub const CLEAN: FakePrimitive =
        FakePrimitive { drop_wake_on_signal: false, ignore_restart: false, report_again: false };

    /// fake 裁决：三段次序刻意写乱，但每段都对应一个可点名缺陷。
    pub fn decide(&self, woke: bool, sig: bool, restart: bool) -> HookOutcome {
        if woke && !(self.drop_wake_on_signal && sig) { return HookOutcome::Wake; }
        if !sig { return HookOutcome::Wait; }
        if restart && !self.ignore_restart { return HookOutcome::Restart; }
        if self.report_again { return HookOutcome::Wait; }
        HookOutcome::Interrupt
    }
}

/// 六个冻结探针：(woke, sig, restart)。下标 1/2/3 分别对应错报 EAGAIN、漏重定位、吞唤醒。
pub const PROBES: [(bool, bool, bool); RECONCILE_PROBES] = [
    (false, false, false), (false, true, false), (false, true, true),
    (true, true, false), (true, false, false), (false, false, true),
];

/// 已声明分歧的探针下标：1=错报 EAGAIN、2=漏重定位、3=吞唤醒。
pub const DIVERGE_PROBES: [usize; DECLARED_DIVERGENCES as usize] = [1, 2, 3];

/// 对账结果：(一致数, 分歧数, 分歧下标是否全落在声明表内, 干净 fake 是否全一致)。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ReconcileReport {
    pub agree: u32,
    pub diverge: u32,
    pub declared_only: bool,
    pub clean_agrees: bool,
}

/// 与 fake 原语逐探针对账。
pub fn reconcile_fake() -> ReconcileReport {
    let mut agree = 0u32;
    let mut diverge = 0u32;
    let mut declared_only = true;
    let mut clean_agrees = true;
    let mut i = 0usize;
    while i < PROBES.len() {
        if let Some((woke, sig, restart)) = PROBES.get(i).copied() {
            let truth = hook_decide(woke, sig, restart);
            let fake = FakePrimitive::FAULTY.decide(woke, sig, restart);
            if truth == fake {
                agree = agree.saturating_add(1);
            } else {
                diverge = diverge.saturating_add(1);
                if !DIVERGE_PROBES.contains(&i) {
                    declared_only = false;
                }
            }
            if truth != FakePrimitive::CLEAN.decide(woke, sig, restart) {
                clean_agrees = false;
            }
        }
        i += 1;
    }
    ReconcileReport { agree, diverge, declared_only, clean_agrees }
}

// ── Linux 逐码对照 ────────────────────────────────────────────────────
/// (名称, Linux 数值, 触发面说明)。只收等待挂点族，表外一律 0。
pub const ERRNO_TABLE: &[(&str, i32, &str)] = &[
    ("EPERM", EPERM, "权限不足：等待对象被保护"),
    ("ESRCH", ESRCH, "票号不存在：已出队或从未入队"),
    ("EINTR", EINTR, "等待被信号打断且无重定位位"),
    ("EAGAIN", EAGAIN, "槽满或非阻塞面无可等待对象"),
    ("EFAULT", EFAULT, "等待参数坏地址"),
    ("EINVAL", EINVAL, "状态与请求不匹配"),
];

/// 按名查码；表外返回 0（显性不可用，不返回伪造码）。
pub fn errno_by_name(name: &str) -> i32 {
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

// ── 确定性指纹 ────────────────────────────────────────────────────────
/// FNV-1a 32 位（整数同余，无浮点、无随机、无墙钟）。
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

/// 把一次观察编码进字节流：先走向标签，再票号低字节。
fn encode_outcome(out: &mut Vec<u8>, got: Option<HookOutcome>) {
    match got {
        Some(o) => {
            out.push(1);
            out.push(o.tag() as u8);
        }
        None => {
            out.push(2);
            out.push(0xFF);
        }
    }
}

/// 规范语料指纹：正常入队/唤醒/打断/陈旧票/满表五段。
pub fn corpus_fingerprint() -> u32 {
    let mut bytes: Vec<u8> = Vec::new();
    let mut q = WaitQueue::new();
    let t1 = q.enqueue(11);
    encode_outcome(&mut bytes, t1.ok().and_then(|t| q.observe(t, false, false).ok()));
    q.wake_all();
    encode_outcome(&mut bytes, t1.ok().and_then(|t| q.observe(t, true, false).ok()));
    if let Ok(t) = t1 {
        bytes.push(q.dequeue(t).is_ok() as u8);
        bytes.push(q.dequeue(t).is_err() as u8);
        encode_outcome(&mut bytes, q.interrupt(t).ok().and_then(|_| q.observe(t, true, false).ok()));
        bytes.push(WaitState::Interrupted.tag());
    }
    let mut fill = WaitQueue::new();
    let mut n = 0u32;
    while n <= WAIT_SLOTS as u32 {
        bytes.push(fill.enqueue(200 + n).is_ok() as u8);
        n = n.saturating_add(1);
    }
    bytes.push(WaitState::Done.tag());
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9307 判据集（add ≥12；add_dyn ≤8）。
pub fn run_unx9307_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9307");

    // 段位守卫：0x40 对齐、落窗、不越入下一个单的段。
    let seg_ok = SEG_BASE % 0x40 == 0
        && SEG_BASE == 0x7BA40
        && (0x7B000..=0x7D1C0).contains(&SEG_BASE)
        && SEG_BASE + SEG_WINDOW <= 0x7BA80;
    s.add("段位守卫：0x7BA40 独占 0x40 窗口", seg_ok,
          "段基址必须 0x40 对齐且不得覆盖相邻单的段");

    // 正常流：入队 → 唤醒 → 观察 Wake → 出队 Woken。
    let mut q = WaitQueue::new();
    let t = q.enqueue(42);
    let t_ok = t == Ok(1);
    let wake_face = q.wake_all();
    let slot_woken = q.slot(0).map(|w| matches!(w.state, WaitState::Woken)) == Some(true);
    let obs_ok = match t { Ok(tk) => q.observe(tk, false, false) == Ok(HookOutcome::Wake), Err(_) => false };
    let deq_ok = match t { Ok(tk) => q.dequeue(tk) == Ok(WaitState::Woken), Err(_) => false };
    s.add("正常流：入队票号 1、唤醒即 Wake",
          t_ok && wake_face == 1 && slot_woken && obs_ok && deq_ok && q.depth() == 0,
          "正常路径必须一发票、一投递、一出队，深度归零");

    // 决定次序：唤醒压过信号，信号先于重定位。
    let order_ok = hook_decide(true, true, false) == HookOutcome::Wake
        && hook_decide(true, true, true) == HookOutcome::Wake
        && hook_decide(false, true, false) == HookOutcome::Interrupt
        && hook_decide(false, false, false) == HookOutcome::Wait;
    s.add("次序：唤醒先于信号，信号先于重定位", order_ok,
          "四段次序错一格就会吞事件或误报 EINTR");

    // 矩阵 + 全 EINTR 格 + 注入账。
    let report = run_hook_matrix();
    let cells_ok = report.per_cell.iter().all(|n| *n == INJECT_ROUNDS);
    let phase_ok = HOOK_MATRIX.iter().all(|c| c.phase.tag() <= WakePhase::AfterEnqueue.tag());
    s.add("矩阵：十格 × 十轮全命中", cells_ok && phase_ok && report.eintr_cells == 2,
          "每格十轮必须逐轮复算一致，不得靠单次抽样");
    let eintr_ok = report.eintr_cells == 2 && report.eintr_cells_ok == 2
        && HookOutcome::Interrupt.errno() == EINTR && HookOutcome::Wake.errno() == 0;
    s.add("全 EINTR 格：两格均回 4 且成功面带 0", eintr_ok,
          "无重定位位的信号格必须每轮都返回 EINTR");
    let ledger_ok = report.conserved && report.ledger.conserved()
        && report.ledger.escaped == 0
        && report.ledger.injected == MATRIX_CELLS as u32 * INJECT_ROUNDS;
    s.add("注入账：100 注入 100 检出 0 逃逸", ledger_ok,
          "矩阵注入数与检出数必须逐笔对平，逃逸即判据失效");

    // 唤醒零丢失：八挂点一次唤醒全投递、零残留。
    let loss = zero_wake_loss();
    let loss_ok = loss.enqueued == WAIT_SLOTS as u32 && loss.delivered == WAIT_SLOTS as u32
        && loss.lost == 0 && loss.stranded == 0;
    s.add("零丢失：八挂点投递 8/8 且无残留", loss_ok,
          "wake_all 之后不得存在仍挂链的 Enqueued 者");

    // 容量：第九个入队回 EAGAIN，槽数不增。
    let mut full = WaitQueue::new();
    let mut filled = 0u32;
    while filled < WAIT_SLOTS as u32 {
        if full.enqueue(300 + filled).is_ok() { filled = filled.saturating_add(1); } else { break; }
    }
    let ninth = full.enqueue(399);
    s.add("容量：八槽满后第九个回 EAGAIN",
          filled == WAIT_SLOTS as u32 && ninth == Err(EAGAIN) && full.depth() == 8,
          "定容队列不得越界写入，满表必须显性回 11");

    // 票号：单调递增 + 出队后失效。
    let mut stale = WaitQueue::new();
    let a = stale.enqueue(1);
    let b = stale.enqueue(2);
    let mono_ok = a == Ok(1) && b == Ok(2);
    stale.wake_all();
    let first = match a { Ok(tk) => stale.dequeue(tk), Err(_) => Err(EINVAL) };
    let second = match a { Ok(tk) => stale.dequeue(tk), Err(_) => Err(EINVAL) };
    s.add("票号：单调递增且出队后失效回 ESRCH",
          mono_ok && first == Ok(WaitState::Woken) && second == Err(ESRCH),
          "票号复用会让唤醒错投到后来者，必须一次性");

    // 打断：Enqueued 转 Interrupted 并由出队带出；错态不回 EAGAIN。
    let mut intq = WaitQueue::new();
    let it = intq.enqueue(5);
    let int_hit = match it { Ok(tk) => intq.interrupt(tk) == Ok(5), Err(_) => false };
    let int_state = match it { Ok(tk) => intq.dequeue(tk) == Ok(WaitState::Interrupted), Err(_) => false };
    let raw_shape = intq.shape();
    let errq_ok = WaitQueue::new().interrupt(999) == Err(ESRCH);
    let zero_pid = {
        let mut e = WaitQueue::new();
        e.enqueue(0) == Err(EINVAL) && e.rejects() == 1
    };
    s.add("中断与错态：Interrupted 可出队、无票 ESRCH、零 pid EINVAL",
          int_hit && int_state && intq.interrupted() == 1 && raw_shape.2 == 1 && errq_ok && zero_pid,
          "打断只改状态不直接返回，错态必须与容量面分离");

    // A3 fake 对账：六探针三分歧，且分歧下标全在声明表内。
    // 口径修正：六探针上 agree + diverge 恒为 6，声明三缺陷各暴露一次
    // 即 diverge == 3、agree == 3；原判据写 agree == 5 与三分歧自相矛盾。
    let rec = reconcile_fake();
    let rec_ok = rec.agree == RECONCILE_PROBES as u32 - DECLARED_DIVERGENCES
        && rec.diverge == DECLARED_DIVERGENCES
        && rec.declared_only && rec.clean_agrees;
    s.add("A3 fake 对账：六探针三分歧全可归因", rec_ok,
          "分歧必须一一对应三缺陷；无缺陷 fake 必须全一致");

    // Linux 逐码：等待族六码数值一致，表外为 0。
    let linux_ok = errno_by_name("EINTR") == 4 && errno_by_name("EAGAIN") == 11
        && errno_by_name("ESRCH") == 3 && errno_by_name("EINVAL") == 22
        && errno_by_name("EPERM") == 1 && errno_by_name("EFAULT") == 14
        && errno_by_name("ENOSYS") == 0;
    s.add("Linux 逐码：等待族六码与 errno 表一致", linux_ok,
          "错误码必须按 Linux 数值，表外不得伪造");

    // 双跑指纹。
    let fp_a = corpus_fingerprint();
    let fp_b = corpus_fingerprint();
    s.add("双跑一致：规范语料指纹相等", fp_a == fp_b && fp_a != 0,
          "同输入两次必须逐字节相同，隐藏状态即失败");

    // 诊断码：12 个码值两两唯一且文案非空。
    let mut uniq = true;
    let mut i = 0usize;
    while i < HookCode::ALL.len() {
        let mut j = i + 1;
        while j < HookCode::ALL.len() {
            if let (Some(x), Some(y)) = (HookCode::ALL.get(i), HookCode::ALL.get(j)) {
                if x.code() == y.code() { uniq = false; }
            }
            j += 1;
        }
        i += 1;
    }
    let text_ok = HookCode::ALL.iter().all(|c| !c.detail().is_empty())
        && HookCode::ALL.len() == 12 && uniq;
    s.add("诊断码：12 码唯一且文案非空", text_ok,
          "段内闭集码重复会让判据无法定位");

    s.add_dyn("注入账快照", ledger_ok,
              &format!("inj={} det={} esc={}", report.ledger.injected,
                       report.ledger.detected, report.ledger.escaped));
    s.add_dyn("零丢失快照", loss_ok,
              &format!("enq={} dlv={} lost={}", loss.enqueued, loss.delivered, loss.lost));
    s.add_dyn("指纹快照", fp_a == fp_b, &format!("fnv=0x{:08X}", fp_a));

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn t01_enqueue_ticket_monotonic() {
        let mut q = WaitQueue::new();
        assert_eq!(q.enqueue(10).is_ok(), true);
        assert_eq!(q.enqueue(11).is_ok(), true);
        assert_eq!(q.ticket_watermark(), 3);
        assert_eq!(q.depth(), 2);
        assert_eq!(q.enqueued(), 2);
    }

    #[test]
    fn t02_wake_no_loss() {
        let loss = zero_wake_loss();
        assert_eq!(loss.enqueued, 8);
        assert_eq!(loss.delivered, 8);
        assert_eq!(loss.lost, 0);
        assert_eq!(loss.stranded, 0);
    }

    #[test]
    fn t03_full_queue_eagain() {
        let mut q = WaitQueue::new();
        let mut n = 0u32;
        while n < 8 {
            assert_eq!(q.enqueue(500 + n).is_ok(), true);
            n += 1;
        }
        assert_eq!(q.enqueue(999), Err(EAGAIN));
        assert_eq!(q.depth(), 8);
        assert_eq!(q.rejects(), 1);
    }

    #[test]
    fn t04_decide_order() {
        assert_eq!(hook_decide(true, true, false), HookOutcome::Wake);
        assert_eq!(hook_decide(false, true, false), HookOutcome::Interrupt);
        assert_eq!(hook_decide(false, true, true), HookOutcome::Restart);
        assert_eq!(hook_decide(false, false, true), HookOutcome::Wait);
        assert_eq!(HookOutcome::Interrupt.errno(), EINTR);
    }

    #[test]
    fn t05_matrix_all_cells() {
        let report = run_hook_matrix();
        assert_eq!(report.per_cell.iter().all(|n| *n == INJECT_ROUNDS), true);
        assert_eq!(report.eintr_cells, 2);
        assert_eq!(report.eintr_cells_ok, 2);
        assert_eq!(report.ledger.escaped, 0);
        assert_eq!(report.ledger.injected, 100);
        assert_eq!(report.conserved, true);
    }

    #[test]
    fn t06_fake_reconcile() {
        let rec = reconcile_fake();
        // 六探针三分歧：agree + diverge == 6，三缺陷各占一格 → agree == 3。
        assert_eq!(rec.agree, 3);
        assert_eq!(rec.diverge, 3);
        assert_eq!(rec.declared_only, true);
        assert_eq!(rec.clean_agrees, true);
    }

    #[test]
    fn t07_fingerprint_double() {
        let a = corpus_fingerprint();
        let b = corpus_fingerprint();
        assert_eq!(a, b);
        assert_eq!(a != 0, true);
    }

    #[test]
    fn t08_checks_all_green() {
        let s = run_unx9307_checks();
        assert_eq!(s.all_passed(), true, "{}", s.red_items_text());
        assert_eq!(s.len() >= 12, true);
    }
}
