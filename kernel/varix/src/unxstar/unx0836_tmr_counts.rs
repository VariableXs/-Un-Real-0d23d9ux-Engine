//! unx0836_tmr_counts — UNX-F0836 定时器初始计数/当前计数双寄存器（诊断段 0x65A0）。
//!
//! 判据原文：双寄存器语义正确（写 ICR 启动/TCC 递减只读），
//!           周期模式自动重装 10/10 次。
//! 设计纪律：
//!   1. 初始计数寄存器 ICR（xAPIC 0x380）写即装值并启动定时器（SDM
//!      Vol.3 §10.5.4：写 ICR 使 TCC 装值、分频器清零、定时器重启）；
//!      当前计数 TCC（0x390）只读递减，写口拒绝。
//!   2. 递减模型：每 tick 减 1，TCC=0 触发到期事件；one-shot 停摆，
//!      periodic 自动从 ICR 重装（写 ICR=0 在 periodic 档为停表令牌）。
//!   3. 守恒账：重装次数 == periodic 到期次数（一到期一重装，不灭失）；
//!      消耗 tick 数 == 累计装值 − 现行 TCC − 每周期残值，双账横平。
//!   4. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! 诊断段 0x65A0 本单独占。

pub const SEG_BASE: u16 = 0x65A0;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_TCC_RO: u16 = SEG_BASE + 1;   // TCC 只读，写口拒绝
const E_IDLE_TICK: u16 = SEG_BASE + 2;// 停摆态 tick 计数（语义违例）
const E_RELOAD: u16 = SEG_BASE + 3;   // periodic 重装值 ≠ ICR
const E_BAL: u16 = SEG_BASE + 4;      // 双账不平
const E_ONESHOT_RELOAD: u16 = SEG_BASE + 5; // one-shot 档重装（语义违例）
const E_STOP: u16 = SEG_BASE + 6;     // periodic 档 ICR=0 停表后仍计时

/// 定时器运行状态机。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TimerRun {
    Stopped,
    Running,
}

/// 双计数寄存器引擎。
pub struct TimerCounts {
    pub icr: u32,        // 初始计数（写口有效）
    tcc: u32,            // 当前计数（只读递减）
    pub run: TimerRun,
    pub periodic: bool,
    pub fires: u32,      // 到期事件数
    pub reloads: u32,    // periodic 重装次数
    ticks_total: u64,    // 消耗 tick 总账
    loaded_total: u64,   // 累计装值总账
    dropped: u64,        // 装值废弃账（运行态改写 ICR 时未耗满的残值）
}

impl TimerCounts {
    pub fn new(periodic: bool) -> TimerCounts {
        TimerCounts {
            icr: 0,
            tcc: 0,
            run: TimerRun::Stopped,
            periodic,
            fires: 0,
            reloads: 0,
            ticks_total: 0,
            loaded_total: 0,
            dropped: 0,
        }
    }

    /// 写 ICR：装值 + 启动（SDM：写 ICR 即重启定时器）。
    /// periodic 档 ICR=0 为停表令牌（SDM：写 0 停止定时器）。
    /// 运行态改写：旧装值未耗满部分入废弃账（装值账闭合义务）。
    pub fn write_icr(&mut self, v: u32) -> Result<(), u16> {
        if self.periodic && v == 0 {
            // 停表令牌：在途 TCC 残值入废弃账
            if self.run == TimerRun::Running {
                self.dropped += self.tcc as u64;
            }
            self.icr = 0;
            self.tcc = 0;
            self.run = TimerRun::Stopped;
            return Ok(());
        }
        if self.run == TimerRun::Running && self.tcc > 0 {
            self.dropped += self.tcc as u64;
        }
        self.icr = v;
        self.tcc = v;
        self.run = TimerRun::Running;
        self.loaded_total += v as u64;
        Ok(())
    }

    /// 读 TCC（只读口）。
    pub fn read_tcc(&self) -> u32 {
        self.tcc
    }

    /// 写 TCC：只读语义，拒绝。
    pub fn write_tcc(&mut self, _v: u32) -> Result<(), u16> {
        Err(E_TCC_RO)
    }

    /// 单 tick：运行态 TCC 递减；TCC 触 0 触发到期。
    /// periodic：自动重装 ICR；one-shot：停摆。
    pub fn tick(&mut self) -> Result<(), u16> {
        if self.run != TimerRun::Running {
            return Err(E_IDLE_TICK);
        }
        if self.tcc == 0 {
            return Err(E_IDLE_TICK); // 装零即到期的空转态
        }
        self.tcc -= 1;
        self.ticks_total += 1;
        if self.tcc == 0 {
            self.fires += 1;
            if self.periodic {
                if self.icr == 0 {
                    return Err(E_STOP); // 停表令牌后不应再计时
                }
                self.tcc = self.icr;
                self.loaded_total += self.icr as u64;
                self.reloads += 1;
                if self.tcc != self.icr {
                    return Err(E_RELOAD);
                }
            } else {
                self.run = TimerRun::Stopped;
            }
        }
        Ok(())
    }

    /// 批量 tick（确定性展开为单 tick 语义）。
    pub fn ticks(&mut self, n: u32) -> Result<(), u16> {
        for _ in 0..n {
            self.tick()?;
        }
        Ok(())
    }

    /// 装值账闭合恒等式：装值总账 == 消耗账 + 在途 TCC + 废弃账。
    /// 每 tick 恰耗 1，每装值要么耗满、要么在途、要么改写时废弃，不灭失。
    pub fn ledger_balanced(&self) -> bool {
        self.loaded_total == self.ticks_total + self.tcc as u64 + self.dropped
    }

    /// periodic 守恒（判据「自动重装 10/10」）：重装数 == 到期数。
    pub fn periodic_reload_consistent(&self) -> bool {
        if self.periodic {
            self.reloads == self.fires
        } else {
            self.reloads == 0
        }
    }
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_TCC_RO => "TCC 只读，写口拒绝",
        E_IDLE_TICK => "停摆态 tick 计数（语义违例）",
        E_RELOAD => "periodic 重装值 ≠ ICR",
        E_BAL => "双账不平（装值账/消耗账/在途账）",
        E_ONESHOT_RELOAD => "one-shot 档发生重装（语义违例）",
        E_STOP => "periodic 档 ICR=0 停表后仍计时",
        _ => "未知诊断码",
    }
}

/// 确定性演练：写 ICR 启动 + TCC 递减只读 + periodic 重装 10/10 + 错误路径。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：写 ICR 启动 + TCC 递减只读（判据前半）。
    let mut t = TimerCounts::new(false);
    t.write_icr(5)?;
    if t.read_tcc() != 5 || t.run != TimerRun::Running {
        return Err(E_BAL);
    }
    t.tick()?;
    t.tick()?;
    if t.read_tcc() != 3 {
        return Err(E_BAL);
    }
    if t.write_tcc(100) != Err(E_TCC_RO) {
        return Err(E_TCC_RO);
    }
    if t.read_tcc() != 3 {
        return Err(E_BAL); // 写拒后 TCC 未受扰
    }
    hits += 1;

    // R2：one-shot 到期停摆，不重装。
    t.ticks(3)?; // TCC 触 0
    if t.fires != 1 || t.run != TimerRun::Stopped || t.reloads != 0 {
        return Err(E_ONESHOT_RELOAD);
    }
    // 停摆态 tick → E_IDLE_TICK。
    if t.tick() != Err(E_IDLE_TICK) {
        return Err(E_IDLE_TICK);
    }
    if !t.ledger_balanced() || !t.periodic_reload_consistent() {
        return Err(E_BAL);
    }
    hits += 1;

    // R3：periodic 自动重装 10/10（判据后半）——ICR=4，跑 4×10+3 tick。
    let mut p = TimerCounts::new(true);
    p.write_icr(4)?;
    for _ in 0..(4 * 10 + 3) {
        p.tick()?;
    }
    if p.fires != 10 || p.reloads != 10 || !p.periodic_reload_consistent() {
        return Err(E_RELOAD);
    }
    // 重装值恒等于 ICR：跑中 TCC 必在 1..=4。
    let tcc = p.read_tcc();
    if tcc == 0 || tcc > 4 {
        return Err(E_RELOAD);
    }
    if !p.ledger_balanced() {
        return Err(E_BAL);
    }
    hits += 1;

    // R4：写 ICR 即重启（装值账入册、TCC 顶格）。
    p.write_icr(4)?;
    if p.read_tcc() != 4 {
        return Err(E_RELOAD);
    }
    p.ticks(2)?;
    p.write_icr(9)?; // 中途改写：TCC 顶到 9
    if p.read_tcc() != 9 {
        return Err(E_RELOAD);
    }
    hits += 1;

    // R5：periodic 停表令牌 ICR=0。
    p.write_icr(0)?;
    if p.run != TimerRun::Stopped {
        return Err(E_STOP);
    }
    if p.tick() != Err(E_IDLE_TICK) {
        return Err(E_IDLE_TICK);
    }
    hits += 1;

    // I1：装零即 tick（one-shot 档 ICR=0 写入启动）→ 空转态拒绝。
    let mut z = TimerCounts::new(false);
    z.write_icr(1)?;
    z.tick()?; // 到期停摆
    if z.tick() != Err(E_IDLE_TICK) {
        return Err(E_IDLE_TICK);
    }
    hits += 1;

    // I2：periodic 档重装后停表令牌触发路径 → E_STOP。
    // 构造：重装发生在 tcc=0 清算点，若 ICR 已被清零则 E_STOP。
    let mut q = TimerCounts::new(true);
    q.write_icr(1)?;
    // 直接污染 icr 为零后 tick（模拟停表令牌与到期竞态）
    q.icr = 0;
    if q.tick() != Err(E_STOP) {
        return Err(E_STOP);
    }
    hits += 1;

    // I3：批量 tick 守恒——总消耗 == 单 tick 累计。
    let mut a = TimerCounts::new(true);
    a.write_icr(3)?;
    let before_fires = a.fires;
    a.ticks(7)?; // 3+3 消耗后重装 1 次 + 剩 1
    if a.fires - before_fires != 2 {
        return Err(E_BAL);
    }
    if !a.ledger_balanced() || !a.periodic_reload_consistent() {
        return Err(E_BAL);
    }
    hits += 1;

    Ok(hits)
}
