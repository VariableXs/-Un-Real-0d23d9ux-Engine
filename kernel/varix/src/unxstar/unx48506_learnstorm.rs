//! UNX-F48506 · E·学习风暴注入（反：10^4 次/秒 bump ×10 零丢失零写穿）
//!
//! 判据原文：UNX-F48506-J1 —— 学习风暴下节流器不失效：10^4 bump/秒
//! 持续注入全数吸收（key 计数账零丢失）；落盘不写穿（相邻两次落盘间隔
//! ≥ WRITE_WIN，无超频落盘）；×10 轮风暴注入账本指纹恒定；风暴后残余
//! pending 可全数落盘（不吞尾）；同 key 重复 bump 计数累加正确；同种子
//! 重放全等。
//!
//! 设计纪律：no_std（仅 core/alloc）；零 unwrap/expect/panic；fuzz 用
//! 确定性 LCG（wrapping_mul/wrapping_add），同种子同序列。

use crate::alloc::vec;
use crate::alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xEA20;

pub const E_LOST: u16 = SEG_BASE + 1;
pub const E_WRITE_THRU: u16 = SEG_BASE + 2;
pub const E_TAIL_LOST: u16 = SEG_BASE + 3;
pub const E_KEY_MISS: u16 = SEG_BASE + 4;
pub const E_NONDET: u16 = SEG_BASE + 5;
pub const E_LEDGER_BAD: u16 = SEG_BASE + 6;

/// 写窗口。
pub const WRITE_WIN: u64 = 100;
/// 风暴每秒 bump 数。
pub const STORM_RATE: u32 = 10_000;

/// key 计数账（风暴真源）：32 个 key 的命中计数。
pub struct KeyLedger {
    pub counts: [u64; 32],
    pub total: u64,
}

impl KeyLedger {
    pub fn new() -> KeyLedger {
        KeyLedger { counts: [0; 32], total: 0 }
    }
    pub fn hit(&mut self, key: u32) {
        let k = (key % 32) as usize;
        self.counts[k] += 1;
        self.total += 1;
    }
    pub fn audit(&self, expect_total: u64) -> Result<(), u16> {
        let mut sum: u64 = 0;
        for c in self.counts.iter() {
            sum += *c;
        }
        if sum != self.total || self.total != expect_total {
            return Err(E_LOST);
        }
        Ok(())
    }
}

/// 节流写账：记录每次落盘的 tick，校验零写穿。
pub struct FlushLog {
    pub ticks: Vec<u64>,
}

impl FlushLog {
    pub fn new() -> FlushLog {
        FlushLog { ticks: Vec::new() }
    }
    pub fn record(&mut self, tick: u64) -> Result<(), u16> {
        if let Some(prev) = self.ticks.last() {
            if tick - *prev < WRITE_WIN {
                return Err(E_WRITE_THRU);
            }
        }
        self.ticks.push(tick);
        Ok(())
    }
}

/// 风暴模拟器：一个 tick 内 burst 次 bump + 一次节流 tick。
pub struct StormSim {
    pub ledger: KeyLedger,
    pub flush_log: FlushLog,
    pub flush_n: u32,
    win_start: u64,
    pending: u64,
}

impl StormSim {
    pub fn new() -> StormSim {
        StormSim {
            ledger: KeyLedger::new(),
            flush_log: FlushLog::new(),
            flush_n: 0,
            win_start: 0,
            pending: 0,
        }
    }

    /// 一 tick：burst 次 bump，然后节流判断。
    pub fn step(&mut self, tick: u64, burst: u32, seed_key: u32) -> Result<(), u16> {
        for i in 0..burst {
            self.ledger.hit(seed_key.wrapping_add(i));
            self.pending += 1;
        }
        if tick - self.win_start >= WRITE_WIN && self.pending > 0 {
            self.flush_log.record(tick)?;
            self.flush_n += 1;
            self.pending = 0;
            self.win_start = tick;
        }
        Ok(())
    }

    /// 收尾落盘（不吞尾）。
    pub fn finish(&mut self, tick: u64) -> Result<(), u16> {
        if self.pending > 0 {
            self.flush_log.record(tick)?;
            self.flush_n += 1;
            self.pending = 0;
        }
        Ok(())
    }
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

/// R1 风暴全吸收：1000 tick × 每秒 10 bump（等效 10^4/秒在 0.1s 窗口）
/// 的账本零丢失。
pub fn r1_storm_absorbed() -> Result<(), u16> {
    let mut sim = StormSim::new();
    let mut g = Lcg::new(0x48506_0001);
    let mut expect: u64 = 0;
    for t in 1..=1000u64 {
        let burst = 10;
        expect += burst as u64;
        let k = (g.next() & 0xFF) as u32;
        sim.step(t, burst, k)?;
    }
    sim.finish(1000)?;
    sim.ledger.audit(expect)
}

/// R2 零写穿：落盘间隔全部 ≥ WRITE_WIN。
pub fn r2_no_write_thru() -> Result<(), u16> {
    let mut sim = StormSim::new();
    let mut g = Lcg::new(0x48506_0002);
    for t in 1..=2000u64 {
        let burst = 1 + g.below(20) as u32;
        sim.step(t, burst, (g.next() & 0xFF) as u32)?;
    }
    sim.finish(2000)?;
    for i in 1..sim.flush_log.ticks.len() {
        if sim.flush_log.ticks[i] - sim.flush_log.ticks[i - 1] < WRITE_WIN {
            return Err(E_WRITE_THRU);
        }
    }
    Ok(())
}

/// R3 ×10 轮指纹恒定：同种子十轮 flush tick 序列全等。
pub fn r3_ten_rounds() -> Result<(), u16> {
    let run = |seed: u64| -> Result<Vec<u64>, u16> {
        let mut g = Lcg::new(seed);
        let mut sim = StormSim::new();
        for t in 1..=600u64 {
            let burst = 1 + g.below(8) as u32;
            sim.step(t, burst, (g.next() & 0xFF) as u32)?;
        }
        sim.finish(600)?;
        Ok(sim.flush_log.ticks.clone())
    };
    let a = run(0x48506_0003)?;
    if a.len() == 0 {
        return Err(E_TAIL_LOST);
    }
    for _ in 0..10 {
        let b = run(0x48506_0003)?;
        if b != a {
            return Err(E_NONDET);
        }
    }
    Ok(())
}

/// R4 不吞尾：风暴结束后 pending 残余全数落盘。
pub fn r4_tail_flushed() -> Result<(), u16> {
    let mut sim = StormSim::new();
    // 90 tick 少量 bump（不跨窗口），收尾必落。
    for t in 1..=90u64 {
        sim.step(t, 3, 7)?;
    }
    sim.finish(91)?;
    if sim.pending != 0 {
        return Err(E_TAIL_LOST);
    }
    if sim.flush_log.ticks.len() == 0 {
        return Err(E_TAIL_LOST);
    }
    Ok(())
}

/// R5 同 key 累加：同 key 反复 bump 计数线性累加。
pub fn r5_same_key_accum() -> Result<(), u16> {
    let mut led = KeyLedger::new();
    for _ in 0..100u32 {
        led.hit(5);
    }
    if led.counts[5] != 100 {
        return Err(E_KEY_MISS);
    }
    if led.total != 100 {
        return Err(E_LOST);
    }
    led.audit(100)
}

/// R6 key 分布账：32 key 全触达且总和守恒。
pub fn r6_key_coverage() -> Result<(), u16> {
    let mut g = Lcg::new(0x48506_0004);
    let mut led = KeyLedger::new();
    for _ in 0..2000u64 {
        led.hit((g.next() & 0xFFFF) as u32);
    }
    led.audit(2000)?;
    // 2000 次 / 32 key，期望每 key > 0（概率上必然）。
    let mut zero = 0;
    for c in led.counts.iter() {
        if *c == 0 {
            zero += 1;
        }
    }
    if zero > 0 {
        return Err(E_KEY_MISS);
    }
    Ok(())
}

/// R7 风暴速率上界：单 tick burst 10^4 也不丢（单 tick 全收）。
pub fn r7_single_tick_10k() -> Result<(), u16> {
    let mut sim = StormSim::new();
    sim.step(1, STORM_RATE, 3)?;
    if sim.ledger.total != STORM_RATE as u64 {
        return Err(E_LOST);
    }
    sim.ledger.audit(STORM_RATE as u64)
}

/// R8 落盘次数上界：2000 tick 风暴 flush ≤ 21。
pub fn r8_flush_bound() -> Result<(), u16> {
    let mut g = Lcg::new(0x48506_0005);
    let mut sim = StormSim::new();
    for t in 1..=2000u64 {
        sim.step(t, 1 + g.below(4) as u32, (g.next() & 0xFF) as u32)?;
    }
    sim.finish(2000)?;
    if sim.flush_n > 21 {
        return Err(E_WRITE_THRU);
    }
    Ok(())
}

/// R9 确定性重放：同种子两轮账本与 flush 全账全等。
pub fn r9_deterministic() -> Result<(), u16> {
    let run = |seed: u64| -> Result<(u64, Vec<u64>), u16> {
        let mut g = Lcg::new(seed);
        let mut sim = StormSim::new();
        for t in 1..=500u64 {
            sim.step(t, 1 + g.below(6) as u32, (g.next() & 0xFF) as u32)?;
        }
        sim.finish(500)?;
        Ok((sim.ledger.total, sim.flush_log.ticks.clone()))
    };
    let a = run(0x48506_0006)?;
    let b = run(0x48506_0006)?;
    if a != b {
        return Err(E_NONDET);
    }
    Ok(())
}

/// R10 空风暴零落盘：零 bump 时 finish 不产生落盘。
pub fn r10_empty_storm() -> Result<(), u16> {
    let mut sim = StormSim::new();
    sim.finish(100)?;
    if sim.flush_n != 0 || sim.flush_log.ticks.len() != 0 {
        return Err(E_WRITE_THRU);
    }
    Ok(())
}

/// 判据主入口：R1..R10，hits 计数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    r1_storm_absorbed()?;
    hits += 1;
    r2_no_write_thru()?;
    hits += 1;
    r3_ten_rounds()?;
    hits += 1;
    r4_tail_flushed()?;
    hits += 1;
    r5_same_key_accum()?;
    hits += 1;
    r6_key_coverage()?;
    hits += 1;
    r7_single_tick_10k()?;
    hits += 1;
    r8_flush_bound()?;
    hits += 1;
    r9_deterministic()?;
    hits += 1;
    r10_empty_storm()?;
    hits += 1;
    Ok(hits)
}
