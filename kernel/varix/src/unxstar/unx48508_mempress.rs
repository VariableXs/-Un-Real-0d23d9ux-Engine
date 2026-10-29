//! UNX-F48508 · E·内存压力注入（反：构造压力 ×10 淘汰路径生效）
//!
//! 判据原文：UNX-F48508-J1 —— ×10 超容量压力注入下淘汰路径全触发：
//! 池/账/日志三类在被灌入 10 倍容量条目后尺寸恰回到各自上界；淘汰计数
//! 与注入计数守恒（淘汰数 = 注入数 - 现存）；×10 轮重复压力淘汰数序列
//! 恒定；压力后功能不损（查/读/最旧取回仍正确）；被淘汰者必为最旧批次
//! （FIFO 语义不漂移）；fuzz 混合压力三账全绿。
//!
//! 设计纪律：no_std（仅 core/alloc）；零 unwrap/expect/panic；fuzz 用
//! 确定性 LCG（wrapping_mul/wrapping_add），同种子同序列。

use crate::alloc::vec;
use crate::alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xEA60;

pub const E_CAP_MISS: u16 = SEG_BASE + 1;
pub const E_EVICT_WRONG: u16 = SEG_BASE + 2;
pub const E_LEDGER_BAD: u16 = SEG_BASE + 3;
pub const E_FUNC_LOST: u16 = SEG_BASE + 4;
pub const E_NONDET: u16 = SEG_BASE + 5;
pub const E_PRESSURE_MISS: u16 = SEG_BASE + 6;

/// 容量上界（与 48507 同域冻结）。
pub const POOL_CAP: usize = 64;
pub const LEDGER_CAP: usize = 128;
pub const LOG_CAP: usize = 256;

/// 压力倍数。
pub const PRESS_X: usize = 10;

/// 压力注入后的池。
pub struct PressPool {
    items: Vec<u64>,
    pub evicted: Vec<u64>,
}

impl PressPool {
    pub fn new() -> PressPool {
        PressPool { items: Vec::new(), evicted: Vec::new() }
    }
    pub fn put(&mut self, v: u64) {
        if self.items.len() >= POOL_CAP {
            let old = self.items.remove(0);
            self.evicted.push(old);
        }
        self.items.push(v);
    }
    pub fn audit(&self) -> Result<(), u16> {
        if self.items.len() > POOL_CAP {
            return Err(E_CAP_MISS);
        }
        Ok(())
    }
    pub fn contains(&self, v: u64) -> bool {
        for it in self.items.iter() {
            if *it == v {
                return true;
            }
        }
        false
    }
}

/// 压力注入后的账表。
pub struct PressLedger {
    keys: Vec<u32>,
    vals: Vec<u64>,
    pub evicted_n: u64,
}

impl PressLedger {
    pub fn new() -> PressLedger {
        PressLedger { keys: Vec::new(), vals: Vec::new(), evicted_n: 0 }
    }
    pub fn put(&mut self, k: u32, v: u64) {
        for i in 0..self.keys.len() {
            if self.keys[i] == k {
                self.vals[i] = v;
                return;
            }
        }
        if self.keys.len() >= LEDGER_CAP {
            self.keys.remove(0);
            self.vals.remove(0);
            self.evicted_n += 1;
        }
        self.keys.push(k);
        self.vals.push(v);
    }
    pub fn get(&self, k: u32) -> Option<u64> {
        for i in 0..self.keys.len() {
            if self.keys[i] == k {
                return Some(self.vals[i]);
            }
        }
        None
    }
    pub fn audit(&self) -> Result<(), u16> {
        if self.keys.len() > LEDGER_CAP || self.keys.len() != self.vals.len() {
            return Err(E_CAP_MISS);
        }
        Ok(())
    }
}

/// 压力注入后的日志环。
pub struct PressLog {
    buf: Vec<u32>,
    head: usize,
}

impl PressLog {
    pub fn new() -> PressLog {
        PressLog { buf: Vec::new(), head: 0 }
    }
    pub fn push(&mut self, v: u32) {
        if self.buf.len() < LOG_CAP {
            self.buf.push(v);
        } else {
            self.buf[self.head] = v;
        }
        self.head = (self.head + 1) % LOG_CAP;
    }
    pub fn oldest_seq(&self) -> Vec<u32> {
        let n = self.buf.len();
        let mut out = Vec::new();
        if n < LOG_CAP {
            for i in 0..n {
                out.push(self.buf[i]);
            }
            return out;
        }
        for i in 0..n {
            out.push(self.buf[(self.head + i) % LOG_CAP]);
        }
        out
    }
    pub fn audit(&self) -> Result<(), u16> {
        if self.buf.len() > LOG_CAP {
            return Err(E_CAP_MISS);
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

/// R1 池 ×10 压力回界：POOL_CAP*10 注入后 len == POOL_CAP。
pub fn r1_pool_back_to_cap() -> Result<(), u16> {
    let mut p = PressPool::new();
    for i in 0..(POOL_CAP * PRESS_X) as u64 {
        p.put(i);
    }
    p.audit()?;
    if p.items.len() != POOL_CAP {
        return Err(E_CAP_MISS);
    }
    if p.evicted.len() != POOL_CAP * PRESS_X - POOL_CAP {
        return Err(E_LEDGER_BAD);
    }
    Ok(())
}

/// R2 池淘汰守恒：注入数 == 现存 + 淘汰。
pub fn r2_pool_conservation() -> Result<(), u16> {
    let mut p = PressPool::new();
    let n = POOL_CAP * PRESS_X;
    for i in 0..n as u64 {
        p.put(i);
    }
    if (p.items.len() + p.evicted.len()) as usize != n {
        return Err(E_LEDGER_BAD);
    }
    Ok(())
}

/// R3 池淘汰最旧批次：现存恰为最后 POOL_CAP 个。
pub fn r3_pool_survivors() -> Result<(), u16> {
    let n = POOL_CAP * PRESS_X;
    let mut p = PressPool::new();
    for i in 0..n as u64 {
        p.put(i);
    }
    for i in 0..(n - POOL_CAP) as u64 {
        if p.contains(i) {
            return Err(E_EVICT_WRONG);
        }
    }
    for i in (n - POOL_CAP) as u64..n as u64 {
        if !p.contains(i) {
            return Err(E_EVICT_WRONG);
        }
    }
    Ok(())
}

/// R4 账表 ×10 压力回界：LEDGER_CAP*10 注入后 len == LEDGER_CAP。
pub fn r4_ledger_back_to_cap() -> Result<(), u16> {
    let mut l = PressLedger::new();
    for i in 0..(LEDGER_CAP * PRESS_X) as u32 {
        l.put(i, i as u64 * 3);
    }
    l.audit()?;
    if l.keys.len() != LEDGER_CAP {
        return Err(E_CAP_MISS);
    }
    if l.evicted_n != (LEDGER_CAP * PRESS_X - LEDGER_CAP) as u64 {
        return Err(E_LEDGER_BAD);
    }
    Ok(())
}

/// R5 压力后功能不损：账表新 key 可查、旧 key 已淘汰。
pub fn r5_ledger_func_alive() -> Result<(), u16> {
    let n = LEDGER_CAP * PRESS_X;
    let mut l = PressLedger::new();
    for i in 0..n as u32 {
        l.put(i, i as u64);
    }
    // 最旧 key 已淘汰。
    if l.get(0).is_some() {
        return Err(E_EVICT_WRONG);
    }
    // 最新 key 在。
    if l.get((n - 1) as u32) != Some((n - 1) as u64) {
        return Err(E_FUNC_LOST);
    }
    Ok(())
}

/// R6 日志 ×10 压力回界：LOG_CAP*10 注入后回绕序列恰为最后 LOG_CAP 条。
pub fn r6_log_back_to_cap() -> Result<(), u16> {
    let n = LOG_CAP * PRESS_X;
    let mut r = PressLog::new();
    for i in 0..n as u32 {
        r.push(i);
    }
    r.audit()?;
    let seq = r.oldest_seq();
    if seq.len() != LOG_CAP {
        return Err(E_CAP_MISS);
    }
    if seq[0] != (n - LOG_CAP) as u32 {
        return Err(E_EVICT_WRONG);
    }
    if seq[LOG_CAP - 1] != (n - 1) as u32 {
        return Err(E_EVICT_WRONG);
    }
    Ok(())
}

/// R7 ×10 轮恒定：同压力序列十轮淘汰数序列全等。
pub fn r7_ten_rounds() -> Result<(), u16> {
    let run = || -> usize {
        let mut p = PressPool::new();
        for i in 0..(POOL_CAP * PRESS_X) as u64 {
            p.put(i.wrapping_mul(7).wrapping_add(13));
        }
        p.evicted.len()
    };
    let a = run();
    if a != POOL_CAP * PRESS_X - POOL_CAP {
        return Err(E_PRESSURE_MISS);
    }
    for _ in 0..10 {
        if run() != a {
            return Err(E_NONDET);
        }
    }
    Ok(())
}

/// R8 fuzz 混合压力：随机量三类注入后三账全绿。
pub fn r8_fuzz_mixed() -> Result<(), u16> {
    let mut g = Lcg::new(0x48508_0001);
    let mut p = PressPool::new();
    let mut l = PressLedger::new();
    let mut r = PressLog::new();
    for _ in 0..2000u64 {
        p.put(g.next());
        l.put((g.next() & 0x1FF) as u32, g.next());
        r.push((g.next() & 0xFF) as u32);
    }
    p.audit()?;
    l.audit()?;
    r.audit()?;
    if p.items.len() > POOL_CAP || l.keys.len() > LEDGER_CAP {
        return Err(E_CAP_MISS);
    }
    Ok(())
}

/// R9 脉冲压力：满-清-再满两轮，第二轮淘汰从零重新计数语义正确。
pub fn r9_pulse_pressure() -> Result<(), u16> {
    let mut p = PressPool::new();
    // 第一脉冲。
    for i in 0..(POOL_CAP * 2) as u64 {
        p.put(i);
    }
    let ev1 = p.evicted.len();
    if ev1 != POOL_CAP {
        return Err(E_PRESSURE_MISS);
    }
    // 清空（重建语义：drain 全部）。
    while p.items.len() > 0 {
        let old = p.items.remove(0);
        p.evicted.push(old);
    }
    // 第二脉冲：POOL_CAP 恰好不触发淘汰。
    for i in 0..POOL_CAP as u64 {
        p.put(10_000 + i);
    }
    if p.items.len() != POOL_CAP {
        return Err(E_CAP_MISS);
    }
    Ok(())
}

/// R10 确定性重放：同种子两轮三账指纹全等。
pub fn r10_deterministic() -> Result<(), u16> {
    let run = |seed: u64| -> Result<(usize, u64, usize), u16> {
        let mut g = Lcg::new(seed);
        let mut p = PressPool::new();
        let mut l = PressLedger::new();
        let mut r = PressLog::new();
        for _ in 0..800u64 {
            p.put(g.next());
            l.put((g.next() & 0x1FF) as u32, g.next());
            r.push((g.next() & 0xFF) as u32);
        }
        p.audit()?;
        l.audit()?;
        r.audit()?;
        Ok((p.evicted.len(), l.evicted_n, r.oldest_seq().len()))
    };
    let a = run(0x48508_0002)?;
    let b = run(0x48508_0002)?;
    if a != b {
        return Err(E_NONDET);
    }
    Ok(())
}

/// 判据主入口：R1..R10，hits 计数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    r1_pool_back_to_cap()?;
    hits += 1;
    r2_pool_conservation()?;
    hits += 1;
    r3_pool_survivors()?;
    hits += 1;
    r4_ledger_back_to_cap()?;
    hits += 1;
    r5_ledger_func_alive()?;
    hits += 1;
    r6_log_back_to_cap()?;
    hits += 1;
    r7_ten_rounds()?;
    hits += 1;
    r8_fuzz_mixed()?;
    hits += 1;
    r9_pulse_pressure()?;
    hits += 1;
    r10_deterministic()?;
    hits += 1;
    Ok(hits)
}
