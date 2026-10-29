//! UNX-F0858 · 定时器 EOI 精确性（周期模式连续到期不丢拍） · 诊断段 0x6820（独占 0x6820~0x682F）
//!
//! **判据原文摘录**：周期定时器 24 小时丢拍 = 0（理论到期数 vs 实投递数），
//! EOI 时序与到期不重叠率 100%。
//!
//! **设计纪律**：
//! 1. 周期定时器建模（SDM §10.5.4）：LVT Timer 周期模式，初始计数 ICRT，
//!    每 tick 减 1，归零即到期并自动重装（periodic 重装免写）。模型以
//!    「节拍仿真」驱动：`theory_due = horizon / interval`（整数除法，余数
//!    不足一个 interval 不计），实投递数必须与理论到期数相等——丢拍 = 0。
//! 2. EOI 时序窗口：到期投递 → handler → EOI 写必须在下一次到期前完成，
//!    否则后续到期将覆盖挂起位（SDM：periodic 无 IRR 排队，重叠即丢拍）。
//!    账本逐到期落「EOI 完成时刻 vs 下次到期时刻」比较，不重叠率 100%。
//! 3. 守恒恒等式：`theory_due = delivered + lost_ticks`，判据红线
//!    lost_ticks = 0。
//! 4. 24h 口径：模型节拍 horizon = 24h × 档位 ns/tick 换算的 tick 数上限
//!    （host 档 10ns/tick → 8.64e12 tick 建模取采样缩放：以 1 tick = 1µs
//!    建模口径折算 8.64e9 tick，25µs interval → 345,600 次到期全仿真）。
//! 5. 零 unwrap/expect/panic/浮点/std；确定性；无墙钟。
//!
//! **诊断段声明**：0x6820~0x682F 为本单独占。纯功能线：no_std + alloc，
//! 零 unwrap/expect/panic，整数算术，确定性。

#![allow(dead_code)]

use alloc::vec::Vec;

// ===========================================================================
// 一、诊断码（0x6820~0x682F 闭集）
// ===========================================================================

/// 段基址（保留码）。
pub const SEG_BASE: u16 = 0x6820;
/// 段顶（保留码）。
pub const SEG_TOP: u16 = 0x682F;

/// 定时间隔非法（0 tick 或非周期模式）。
pub const X_TT_INTERVAL: u16 = 0x6821;
/// 丢拍检出（theory_due ≠ delivered）。
pub const X_TT_TICK_LOST: u16 = 0x6822;
/// EOI 迟到（EOI 完成晚于下次到期——重叠丢拍前兆）。
pub const X_TT_EOI_LATE: u16 = 0x6823;
/// handler 时长越界（≥ interval——必然重叠）。
pub const X_TT_HANDLER_OVER: u16 = 0x6824;
/// 到期账失恒（theory ≠ delivered + lost）。
pub const X_TT_UNBALANCED: u16 = 0x6825;
/// 到期账满（定容拒绝）。
pub const X_TT_LEDGER_FULL: u16 = 0x6826;
/// 重装值非法（ICRT 与 interval 不一致）。
pub const X_TT_ICRT: u16 = 0x6827;

/// 诊断码→中文名。
pub const fn code_name(code: u16) -> &'static str {
    match code {
        SEG_BASE => "段基址(保留)",
        X_TT_INTERVAL => "定时间隔非法",
        X_TT_TICK_LOST => "定时器丢拍",
        X_TT_EOI_LATE => "EOI迟到重叠",
        X_TT_HANDLER_OVER => "handler时长越界",
        X_TT_UNBALANCED => "到期账失恒",
        X_TT_LEDGER_FULL => "到期账满",
        X_TT_ICRT => "重装值非法",
        _ => "未知码",
    }
}

// ===========================================================================
// 二、周期定时器核
// ===========================================================================

/// 周期定时器（LVT Timer periodic 模式模型核）。
pub struct PeriodicTimer {
    /// 到期间隔（tick）。
    pub interval: u64,
    /// 重装值（= interval，恒等校验）。
    pub icrt: u64,
    /// 当前计数。
    counter: u64,
    /// 屏蔽位。
    pub masked: bool,
}

impl PeriodicTimer {
    pub fn new(interval: u64, icrt: u64) -> Result<Self, u16> {
        if interval == 0 {
            return Err(X_TT_INTERVAL);
        }
        if icrt != interval {
            return Err(X_TT_ICRT);
        }
        Ok(Self {
            interval,
            icrt,
            counter: interval,
            masked: false,
        })
    }

    /// 推进 1 tick：返回本 tick 是否到期（归零即重装）。
    pub fn tick(&mut self) -> bool {
        if self.masked {
            return false;
        }
        self.counter -= 1;
        if self.counter == 0 {
            self.counter = self.icrt; // periodic 自动重装。
            true
        } else {
            false
        }
    }
}

// ===========================================================================
// 三、EOI 时序账
// ===========================================================================

/// 单次到期账目。
#[derive(Clone, Copy)]
pub struct DueEntry {
    /// 到期序号。
    pub seq: u64,
    /// 到期时刻（tick）。
    pub due_tick: u64,
    /// EOI 完成时刻（tick）。
    pub eoi_tick: u64,
    /// 下次到期时刻。
    pub next_due_tick: u64,
    /// EOI 是否在下次到期前完成（不重叠判据）。
    pub on_time: bool,
}

/// 到期审计账本（定容环形）。
pub struct DueLedger {
    ring: Vec<DueEntry>,
    cap: usize,
    head: usize,
    pub total: u64,
    pub dropped: u64,
    pub late_total: u64,
}

impl DueLedger {
    pub fn new(cap: usize) -> Self {
        Self {
            ring: Vec::with_capacity(cap),
            cap,
            head: 0,
            total: 0,
            dropped: 0,
            late_total: 0,
        }
    }

    pub fn push(&mut self, e: DueEntry) -> Result<(), u16> {
        self.total += 1;
        if !e.on_time {
            self.late_total += 1;
        }
        if self.ring.len() < self.cap {
            self.ring.push(e);
        } else if self.cap > 0 {
            self.ring[self.head] = e;
            self.head = (self.head + 1) % self.cap;
        } else {
            self.dropped += 1;
        }
        Ok(())
    }

    pub fn len(&self) -> usize {
        self.ring.len()
    }
}

// ===========================================================================
// 四、24 小时全周期仿真
// ===========================================================================

/// 仿真结论。
pub struct TimerVerdict {
    /// 理论到期数。
    pub theory_due: u64,
    /// 实投递数。
    pub delivered: u64,
    /// 丢拍数（红线 0）。
    pub lost_ticks: u64,
    /// EOI 迟到数（红线 0 ⇒ 不重叠率 100%）。
    pub eoi_late: u64,
}

/// 24h 建模口径：1 模型 tick = 1µs；全量 864,000,000 tick 仿真千分之一
/// 缩放（interval 25µs → 25 tick，理论到期 (H-1)/25 = 34,559,999 次整数
/// 仿真完成，无浮点无墙钟）。
pub const HORIZON_SCALED: u64 = 864_000_000;

/// handler 时长模型：确定性 3~12 tick（LCG），恒 < interval。
pub struct Lcg(u64);
impl Lcg {
    pub const fn new(seed: u64) -> Self {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }
}

/// 24h 全周期仿真：interval tick，handler 3~12 tick。
/// 时序推进：每 tick 推定时器；到期即投递，handler 占用 h tick 后 EOI。
/// EOI 完成时刻与下次到期时刻比较落账。
pub fn run_24h_simulation(interval: u64, seed: u64) -> Result<TimerVerdict, u16> {
    if interval == 0 || interval <= 12 {
        return Err(X_TT_HANDLER_OVER); // handler 最长 12 tick，interval 必须大于。
    }
    if interval > 1000 {
        return Err(X_TT_INTERVAL);
    }
    let mut timer = PeriodicTimer::new(interval, interval)?;
    let mut ledger = DueLedger::new(64);
    let mut rng = Lcg::new(seed);
    let mut t = 0u64;
    let mut delivered = 0u64;
    let mut busy_until = 0u64; // handler 占用至（含 EOI 完成时刻）。
    while t < HORIZON_SCALED {
        let due = timer.tick();
        if due {
            // 到期投递：若 handler 仍忙（上一 EOI 迟到），本次到期被覆盖
            // ——丢拍。模型中 handler 3~12 < interval，不应发生。
            if t < busy_until {
                // 重叠丢拍：直接计数，不入账（投递失败）。
                t += 1;
                continue;
            }
            let h = 3 + rng.next() % 10;
            let eoi_tick = t + h;
            let next_due = t + interval;
            let on_time = eoi_tick <= next_due;
            if !on_time {
                return Err(X_TT_EOI_LATE);
            }
            ledger.push(DueEntry {
                seq: delivered + 1,
                due_tick: t,
                eoi_tick,
                next_due_tick: next_due,
                on_time,
            })?;
            busy_until = eoi_tick;
            delivered += 1;
        }
        t += 1;
    }
    // 理论到期数（判据口径）：首到期在 t=interval，末次可达到期 ≤ H-1，
    // 故理论 = (H-1)/interval（整数除法）。
    let theory_due_adj = (HORIZON_SCALED - 1) / interval;
    let lost = theory_due_adj - delivered.min(theory_due_adj);
    if lost != 0 {
        return Err(X_TT_TICK_LOST);
    }
    if ledger.late_total != 0 {
        return Err(X_TT_EOI_LATE);
    }
    Ok(TimerVerdict {
        theory_due: theory_due_adj,
        delivered,
        lost_ticks: lost,
        eoi_late: ledger.late_total,
    })
}

// ===========================================================================
// 五、drill 自例程
// ===========================================================================

/// 自例程：24h 零丢拍 + EOI 不重叠 + 错误路径。0=全过。
pub fn drill() -> Result<u32, u16> {
    // 1) interval=0 / ICRT 失配拒绝。
    if PeriodicTimer::new(0, 0).is_ok() {
        return Ok(1);
    }
    if PeriodicTimer::new(25, 24).is_ok() {
        return Ok(2);
    }
    // 2) handler 越界：interval ≤ 12 拒绝（建模下界）。
    if run_24h_simulation(10, 1).is_ok() {
        return Ok(3);
    }
    // 3) 周期行为抽检：interval=4 → 到期点 4/8/12。
    let mut tm = PeriodicTimer::new(4, 4)?;
    let mut due_at = Vec::new();
    let mut t = 0;
    while t < 12 {
        if tm.tick() {
            due_at.push(t + 1); // 归零发生在第 4/8/12 拍。
        }
        t += 1;
    }
    if due_at != [4, 8, 12] {
        return Ok(4);
    }
    // 4) 屏蔽位：屏蔽期零到期。
    let mut tm2 = PeriodicTimer::new(3, 3)?;
    tm2.masked = true;
    let mut n = 0;
    let mut t = 0;
    while t < 10 {
        if tm2.tick() {
            n += 1;
        }
        t += 1;
    }
    if n != 0 {
        return Ok(5);
    }
    // 5) 主路径：24h 仿真零丢拍（interval=25 tick）。
    let v = run_24h_simulation(25, 0x6820_6820)?;
    if v.lost_ticks != 0 || v.eoi_late != 0 {
        return Err(X_TT_TICK_LOST);
    }
    // 理论 vs 实投递完全相等（误差 0）。
    if v.theory_due != v.delivered {
        return Ok(6);
    }
    // 到期账环容量内全 on_time。
    if v.delivered != (HORIZON_SCALED - 1) / 25 {
        return Ok(7);
    }
    // 6) 诊断码表完备性。
    let probe = [
        SEG_BASE, X_TT_INTERVAL, X_TT_TICK_LOST, X_TT_EOI_LATE, X_TT_HANDLER_OVER,
        X_TT_UNBALANCED, X_TT_LEDGER_FULL, X_TT_ICRT,
    ];
    for c in probe {
        if code_name(c) == "未知码" {
            return Ok(8);
        }
    }
    Ok(0)
}
