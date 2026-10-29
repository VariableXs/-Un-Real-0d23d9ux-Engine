//! UNX-F48481 · E·抖动合并窗口 40ms 边界（正：20ms 内双击合并、40ms 外不合并）
//!
//! 判据原文：UNX-F48481-J1 —— 抖动合并窗口语义：与上一接受事件间隔
//! gap ≥ WIN(40ms) → 独立单击；DEB_MIN(20ms) ≤ gap < WIN → 合并双击；
//! gap < DEB_MIN → 抖动噪声直接吸收（不出事件）；39ms 合并、40ms 独立、
//! 20ms 合并、19ms 吸收四边界恒定；×10 边界重复结果恒定；随机 gap fuzz
//! 结局全在白名单；统计账守恒（合并+独立+吸收 == 总事件）；同序列重放
//! 确定性一致。
//!
//! 设计纪律：no_std（仅 core/alloc）；零 unwrap/expect/panic；fuzz 用
//! 确定性 LCG（wrapping_mul/wrapping_add），同种子同序列。

use crate::alloc::vec;
use crate::alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xE620;

pub const E_WIN_FROZEN: u16 = SEG_BASE + 1;
pub const E_GAP_OOB: u16 = SEG_BASE + 2;
pub const E_THIRD_STATE: u16 = SEG_BASE + 3;
pub const E_LEDGER_BAD: u16 = SEG_BASE + 4;
pub const E_NONDET: u16 = SEG_BASE + 5;
pub const E_BOUND_DRIFT: u16 = SEG_BASE + 6;

/// 合并窗口上界（毫秒）。
pub const DEB_WIN_MS: u32 = 40;
/// 合并下界（毫秒）：低于此视为纯抖动噪声。
pub const DEB_MIN_MS: u32 = 20;

/// 结局白名单。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DebOutcome {
    /// 抖动噪声：直接吸收，不出事件。
    Absorb,
    /// 与上一接受事件合并为双击。
    Double,
    /// 独立单击。
    Single,
}

impl DebOutcome {
    pub fn code(&self) -> u8 {
        match self {
            DebOutcome::Absorb => 0,
            DebOutcome::Double => 1,
            DebOutcome::Single => 2,
        }
    }
}

/// 抖动合并器状态机。
pub struct Debouncer {
    pub last_accepted_ms: Option<u32>,
    pub absorb_n: u32,
    pub double_n: u32,
    pub single_n: u32,
    pub last_outcome: Option<DebOutcome>,
}

impl Debouncer {
    pub fn new() -> Debouncer {
        Debouncer {
            last_accepted_ms: None,
            absorb_n: 0,
            double_n: 0,
            single_n: 0,
            last_outcome: None,
        }
    }

    /// 事件到达：now 为当前毫秒时刻（须单调不减）。
    pub fn press(&mut self, now_ms: u32) -> Result<DebOutcome, u16> {
        match self.last_accepted_ms {
            None => {
                self.last_accepted_ms = Some(now_ms);
                self.single_n += 1;
                self.last_outcome = Some(DebOutcome::Single);
                Ok(DebOutcome::Single)
            }
            Some(prev) => {
                if now_ms < prev {
                    return Err(E_GAP_OOB);
                }
                let gap = now_ms - prev;
                let out = if gap < DEB_MIN_MS {
                    // 纯噪声：吸收，且不推进 last_accepted（保持窗口基准）。
                    self.absorb_n += 1;
                    self.last_outcome = Some(DebOutcome::Absorb);
                    DebOutcome::Absorb
                } else if gap < DEB_WIN_MS {
                    // 合并双击：推进基准。
                    self.last_accepted_ms = Some(now_ms);
                    self.double_n += 1;
                    self.last_outcome = Some(DebOutcome::Double);
                    DebOutcome::Double
                } else {
                    // 窗口外：独立单击。
                    self.last_accepted_ms = Some(now_ms);
                    self.single_n += 1;
                    self.last_outcome = Some(DebOutcome::Single);
                    DebOutcome::Single
                };
                Ok(out)
            }
        }
    }

    /// 统计账守恒。
    pub fn audit_ledger(&self, total: u32) -> Result<(), u16> {
        if self.absorb_n + self.double_n + self.single_n != total {
            return Err(E_LEDGER_BAD);
        }
        Ok(())
    }

    pub fn reset(&mut self) {
        self.last_accepted_ms = None;
        self.absorb_n = 0;
        self.double_n = 0;
        self.single_n = 0;
        self.last_outcome = None;
    }
}

/// 常量冻结审计。
pub fn audit_constants() -> Result<(), u16> {
    if DEB_WIN_MS != 40 || DEB_MIN_MS != 20 {
        return Err(E_WIN_FROZEN);
    }
    if DEB_MIN_MS >= DEB_WIN_MS {
        return Err(E_WIN_FROZEN);
    }
    Ok(())
}

/// R1 首击即独立：冷启动第一按 = Single。
pub fn r1_first_single() -> Result<(), u16> {
    let mut d = Debouncer::new();
    let o = d.press(1000)?;
    if o != DebOutcome::Single {
        return Err(E_THIRD_STATE);
    }
    if d.single_n != 1 || d.double_n != 0 || d.absorb_n != 0 {
        return Err(E_LEDGER_BAD);
    }
    Ok(())
}

/// R2 四边界恒定：19 吸收 / 20 合并 / 39 合并 / 40 独立。
pub fn r2_four_bounds() -> Result<(), u16> {
    // 19ms → Absorb。
    let mut d = Debouncer::new();
    let _ = d.press(100)?;
    if d.press(119)? != DebOutcome::Absorb {
        return Err(E_BOUND_DRIFT);
    }
    // 20ms → Double（吸收不推进基准，从 100 起算）。
    let mut d2 = Debouncer::new();
    let _ = d2.press(100)?;
    if d2.press(120)? != DebOutcome::Double {
        return Err(E_BOUND_DRIFT);
    }
    // 39ms → Double。
    let mut d3 = Debouncer::new();
    let _ = d3.press(100)?;
    if d3.press(139)? != DebOutcome::Double {
        return Err(E_BOUND_DRIFT);
    }
    // 40ms → Single。
    let mut d4 = Debouncer::new();
    let _ = d4.press(100)?;
    if d4.press(140)? != DebOutcome::Single {
        return Err(E_BOUND_DRIFT);
    }
    Ok(())
}

/// R3 41ms 外恒独立：gap 41 与 1000 均独立。
pub fn r3_outside_single() -> Result<(), u16> {
    let mut d = Debouncer::new();
    let _ = d.press(0)?;
    if d.press(41)? != DebOutcome::Single {
        return Err(E_BOUND_DRIFT);
    }
    if d.press(1041)? != DebOutcome::Single {
        return Err(E_BOUND_DRIFT);
    }
    if d.single_n != 3 || d.double_n != 0 {
        return Err(E_LEDGER_BAD);
    }
    Ok(())
}

/// R4 ×10 边界重复：39/41 交替 10 轮，结果恒定（39→Double、41→Single）。
pub fn r4_bound_repeat() -> Result<(), u16> {
    for _ in 0..10u32 {
        let mut d = Debouncer::new();
        let _ = d.press(0)?;
        if d.press(39)? != DebOutcome::Double {
            return Err(E_BOUND_DRIFT);
        }
        let mut d2 = Debouncer::new();
        let _ = d2.press(0)?;
        if d2.press(41)? != DebOutcome::Single {
            return Err(E_BOUND_DRIFT);
        }
    }
    Ok(())
}

/// R5 噪声不推进基准：三次 5ms 噪声后，第 25ms 处仍是合并（基准仍为首击）。
pub fn r5_absorb_no_advance() -> Result<(), u16> {
    let mut d = Debouncer::new();
    let _ = d.press(0)?;
    for _ in 0..3 {
        if d.press(5)? != DebOutcome::Absorb {
            return Err(E_THIRD_STATE);
        }
    }
    if d.last_accepted_ms != Some(0) {
        return Err(E_LEDGER_BAD);
    }
    if d.press(25)? != DebOutcome::Double {
        return Err(E_BOUND_DRIFT);
    }
    Ok(())
}

/// R6 三段流重演：0/10/25/60/65/100 → Single/Absorb/Double/Single/Absorb/Double。
pub fn r6_stream_replay() -> Result<(), u16> {
    let ticks: [u32; 6] = [0, 10, 25, 60, 65, 100];
    let expect: [DebOutcome; 6] = [
        DebOutcome::Single,
        DebOutcome::Absorb,
        DebOutcome::Double,
        DebOutcome::Single,
        DebOutcome::Absorb,
        DebOutcome::Double,
    ];
    let mut d = Debouncer::new();
    for i in 0..6 {
        let o = d.press(ticks[i])?;
        if o != expect[i] {
            return Err(E_NONDET);
        }
    }
    d.audit_ledger(6)
}

/// R7 结局白名单：随机 gap 流 200 击全在 {Absorb, Double, Single}。
pub fn r7_outcome_whitelist() -> Result<(), u16> {
    let mut g = Lcg::new(0x48481_0001);
    let mut d = Debouncer::new();
    let mut now: u32 = 0;
    let mut total: u32 = 0;
    for _ in 0..200 {
        now += 1 + g.below(80) as u32;
        let o = d.press(now)?;
        match o.code() {
            0 | 1 | 2 => {}
            _ => return Err(E_THIRD_STATE),
        }
        total += 1;
    }
    d.audit_ledger(total)
}

/// R8 统计账守恒：随机流后合并+独立+吸收 == 总数。
pub fn r8_ledger() -> Result<(), u16> {
    let mut g = Lcg::new(0x48481_0002);
    let mut d = Debouncer::new();
    let mut now: u32 = 0;
    for _ in 0..150 {
        now += 1 + g.below(60) as u32;
        let _ = d.press(now)?;
    }
    d.audit_ledger(150)
}

/// R9 确定性重放：同 tick 序列两台合并器结局序列全等。
pub fn r9_deterministic() -> Result<(), u16> {
    let mut g = Lcg::new(0x48481_0003);
    let mut ticks: Vec<u32> = Vec::new();
    let mut now: u32 = 0;
    for _ in 0..80 {
        now += 1 + g.below(50) as u32;
        ticks.push(now);
    }
    let mut a = Debouncer::new();
    let mut b = Debouncer::new();
    for t in ticks.iter() {
        let oa = a.press(*t)?;
        let ob = b.press(*t)?;
        if oa != ob {
            return Err(E_NONDET);
        }
    }
    if a.absorb_n != b.absorb_n || a.double_n != b.double_n || a.single_n != b.single_n {
        return Err(E_NONDET);
    }
    Ok(())
}

/// R10 时钟回拨拒收：now < last_accepted 报 E_GAP_OOB。
pub fn r10_clock_rollback() -> Result<(), u16> {
    let mut d = Debouncer::new();
    let _ = d.press(500)?;
    match d.press(499) {
        Err(E_GAP_OOB) => {}
        _ => return Err(E_GAP_OOB),
    }
    Ok(())
}

/// 确定性 LCG。
pub struct Lcg {
    st: u64,
}

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg { st: seed }
    }
    pub fn next(&mut self) -> u64 {
        self.st = self
            .st
            .wrapping_mul(1_664_525)
            .wrapping_add(1_013_904_223);
        self.st
    }
    pub fn below(&mut self, b: u64) -> u64 {
        if b == 0 {
            return 0;
        }
        self.next() % b
    }
}

/// 判据主入口：R1..R10，hits 计数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    audit_constants()?;
    hits += 1;
    r1_first_single()?;
    hits += 1;
    r2_four_bounds()?;
    hits += 1;
    r3_outside_single()?;
    hits += 1;
    r4_bound_repeat()?;
    hits += 1;
    r5_absorb_no_advance()?;
    hits += 1;
    r6_stream_replay()?;
    hits += 1;
    r7_outcome_whitelist()?;
    hits += 1;
    r8_ledger()?;
    hits += 1;
    r9_deterministic()?;
    hits += 1;
    r10_clock_rollback()?;
    hits += 1;
    Ok(hits)
}
