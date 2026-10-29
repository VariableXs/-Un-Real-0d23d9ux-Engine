//! UNX-F48507 · E·内存无界增长防御（正：池/账/日志三类有界）
//!
//! 判据原文：UNX-F48507-J1 —— 三类资源全部硬上界：对象池（POOL_CAP=64）
//! 满载后 FIFO 淘汰最旧、记账表（LEDGER_CAP=128）满载后淘汰最旧、日志环
//! （LOG_CAP=256）满载后环形回绕——三者容量在任何注入序列下都不超过
//! 上界；满载后插入必成功（淘汰路径生效）；被淘汰者恰为最旧条目；容量
//! 常数冻结；fuzz 长注入后三类尺寸全在界内；账目守恒（插入数 == 现存 +
//! 淘汰数）。
//!
//! 设计纪律：no_std（仅 core/alloc）；零 unwrap/expect/panic；fuzz 用
//! 确定性 LCG（wrapping_mul/wrapping_add），同种子同序列。

use crate::alloc::vec;
use crate::alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xEA40;

pub const E_POOL_OVER: u16 = SEG_BASE + 1;
pub const E_LEDGER_OVER: u16 = SEG_BASE + 2;
pub const E_LOG_OVER: u16 = SEG_BASE + 3;
pub const E_EVICT_WRONG: u16 = SEG_BASE + 4;
pub const E_LEDGER_BAD: u16 = SEG_BASE + 5;
pub const E_NONDET: u16 = SEG_BASE + 6;
pub const E_KEY_MISS: u16 = SEG_BASE + 7;

/// 三类容量常数。
pub const POOL_CAP: usize = 64;
pub const LEDGER_CAP: usize = 128;
pub const LOG_CAP: usize = 256;

/// 对象池：FIFO 淘汰。
pub struct ObjectPool {
    items: Vec<u64>,
    pub inserted: u64,
    pub evicted: u64,
}

impl ObjectPool {
    pub fn new() -> ObjectPool {
        ObjectPool { items: Vec::new(), inserted: 0, evicted: 0 }
    }

    /// 插入：满则淘汰最旧。
    pub fn put(&mut self, v: u64) -> Option<u64> {
        self.inserted += 1;
        let ev = if self.items.len() >= POOL_CAP {
            let oldest = self.items.remove(0);
            self.evicted += 1;
            Some(oldest)
        } else {
            None
        };
        self.items.push(v);
        ev
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// 最旧条目。
    pub fn oldest(&self) -> Option<u64> {
        self.items.first().copied()
    }

    /// 审计：容量上界 + 账目守恒。
    pub fn audit(&self) -> Result<(), u16> {
        if self.items.len() > POOL_CAP {
            return Err(E_POOL_OVER);
        }
        if self.inserted != self.items.len() as u64 + self.evicted {
            return Err(E_LEDGER_BAD);
        }
        Ok(())
    }
}

/// 记账表：键值对，满载淘汰最旧（插入序）。
pub struct LedgerMap {
    keys: Vec<u32>,
    vals: Vec<u64>,
    pub evicted: Vec<(u32, u64)>,
}

impl LedgerMap {
    pub fn new() -> LedgerMap {
        LedgerMap { keys: Vec::new(), vals: Vec::new(), evicted: Vec::new() }
    }

    pub fn put(&mut self, k: u32, v: u64) {
        // 已存在则原位更新（不算新插入）。
        for i in 0..self.keys.len() {
            if self.keys[i] == k {
                self.vals[i] = v;
                return;
            }
        }
        if self.keys.len() >= LEDGER_CAP {
            let ok = self.keys.remove(0);
            let ov = self.vals.remove(0);
            self.evicted.push((ok, ov));
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

    pub fn len(&self) -> usize {
        self.keys.len()
    }

    pub fn audit(&self) -> Result<(), u16> {
        if self.keys.len() > LEDGER_CAP {
            return Err(E_LEDGER_OVER);
        }
        if self.keys.len() != self.vals.len() {
            return Err(E_LEDGER_BAD);
        }
        Ok(())
    }
}

/// 日志环：满载回绕。
pub struct LogRing {
    buf: Vec<u32>,
    head: usize,
    pub written: u64,
}

impl LogRing {
    pub fn new() -> LogRing {
        LogRing { buf: Vec::new(), head: 0, written: 0 }
    }
    pub fn push(&mut self, v: u32) {
        if self.buf.len() < LOG_CAP {
            self.buf.push(v);
        } else {
            self.buf[self.head] = v;
        }
        self.head = (self.head + 1) % LOG_CAP;
        self.written += 1;
    }
    /// 时间序读出。
    pub fn read_oldest_first(&self) -> Vec<u32> {
        let mut out = Vec::new();
        let n = self.buf.len();
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
    pub fn len(&self) -> usize {
        self.buf.len()
    }
}

/// 三类资源复合守卫。
pub struct MemGuard {
    pub pool: ObjectPool,
    pub ledger: LedgerMap,
    pub log: LogRing,
}

impl MemGuard {
    pub fn new() -> MemGuard {
        MemGuard {
            pool: ObjectPool::new(),
            ledger: LedgerMap::new(),
            log: LogRing::new(),
        }
    }
    pub fn audit(&self) -> Result<(), u16> {
        self.pool.audit()?;
        self.ledger.audit()?;
        if self.log.len() > LOG_CAP {
            return Err(E_LOG_OVER);
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

/// R1 池上界：200 次插入后 len == POOL_CAP。
pub fn r1_pool_capped() -> Result<(), u16> {
    let mut p = ObjectPool::new();
    for i in 0..200u64 {
        p.put(i);
    }
    p.audit()?;
    if p.len() != POOL_CAP {
        return Err(E_POOL_OVER);
    }
    if p.evicted != 200 - POOL_CAP as u64 {
        return Err(E_LEDGER_BAD);
    }
    Ok(())
}

/// R2 淘汰最旧：池满后插入 X，最旧应变为序列第 2 位。
pub fn r2_pool_evict_oldest() -> Result<(), u16> {
    let mut p = ObjectPool::new();
    for i in 0..POOL_CAP as u64 {
        p.put(1000 + i);
    }
    let ev = p.put(9999);
    match ev {
        Some(v) => {
            if v != 1000 {
                return Err(E_EVICT_WRONG);
            }
        }
        None => return Err(E_EVICT_WRONG),
    }
    if p.oldest() != Some(1001) {
        return Err(E_EVICT_WRONG);
    }
    Ok(())
}

/// R3 账表上界：300 个不同 key 后 len == LEDGER_CAP。
pub fn r3_ledger_capped() -> Result<(), u16> {
    let mut l = LedgerMap::new();
    for i in 0..300u32 {
        l.put(i, i as u64 * 7);
    }
    l.audit()?;
    if l.len() != LEDGER_CAP {
        return Err(E_LEDGER_OVER);
    }
    Ok(())
}

/// R4 账表淘汰最旧：满后插入新 key，首 key 被淘汰进 evicted。
pub fn r4_ledger_evict() -> Result<(), u16> {
    let mut l = LedgerMap::new();
    for i in 0..LEDGER_CAP as u32 {
        l.put(i, i as u64);
    }
    l.put(9999, 1);
    if l.len() != LEDGER_CAP {
        return Err(E_LEDGER_OVER);
    }
    if l.evicted.len() != 1 || l.evicted[0].0 != 0 {
        return Err(E_EVICT_WRONG);
    }
    if l.get(0).is_some() {
        return Err(E_EVICT_WRONG);
    }
    if l.get(9999) != Some(1) {
        return Err(E_KEY_MISS);
    }
    Ok(())
}

/// R5 账表原位更新：同 key 重写不增容、不淘汰。
pub fn r5_ledger_inplace() -> Result<(), u16> {
    let mut l = LedgerMap::new();
    for i in 0..LEDGER_CAP as u32 {
        l.put(i, 1);
    }
    let before_evict = l.evicted.len();
    l.put(5, 42);
    if l.len() != LEDGER_CAP {
        return Err(E_LEDGER_OVER);
    }
    if l.evicted.len() != before_evict {
        return Err(E_EVICT_WRONG);
    }
    if l.get(5) != Some(42) {
        return Err(E_KEY_MISS);
    }
    Ok(())
}

/// R6 日志环回绕：LOG_CAP + 50 条后读出为最后 LOG_CAP 条。
pub fn r6_log_wrap() -> Result<(), u16> {
    let mut r = LogRing::new();
    for i in 0..(LOG_CAP as u32 + 50) {
        r.push(i);
    }
    if r.len() != LOG_CAP {
        return Err(E_LOG_OVER);
    }
    let seq = r.read_oldest_first();
    if seq.len() != LOG_CAP {
        return Err(E_LOG_OVER);
    }
    if seq[0] != 50 {
        return Err(E_EVICT_WRONG);
    }
    if seq[LOG_CAP - 1] != LOG_CAP as u32 + 49 {
        return Err(E_EVICT_WRONG);
    }
    Ok(())
}

/// R7 fuzz 长注入：三类各 1000 次注入后全部在界内。
pub fn r7_fuzz_bounded() -> Result<(), u16> {
    let mut g = Lcg::new(0x48507_0001);
    let mut mg = MemGuard::new();
    for _ in 0..1000u64 {
        mg.pool.put(g.next());
        mg.ledger.put((g.next() & 0xFF) as u32, g.next());
        mg.log.push((g.next() & 0xFF) as u32);
    }
    mg.audit()?;
    if mg.pool.len() > POOL_CAP || mg.ledger.len() > LEDGER_CAP || mg.log.len() > LOG_CAP {
        return Err(E_POOL_OVER);
    }
    Ok(())
}

/// R8 账目守恒：pool inserted == len + evicted。
pub fn r8_pool_ledger() -> Result<(), u16> {
    let mut g = Lcg::new(0x48507_0002);
    let mut p = ObjectPool::new();
    for _ in 0..500u64 {
        p.put(g.next());
    }
    p.audit()?;
    if p.inserted != 500 {
        return Err(E_LEDGER_BAD);
    }
    Ok(())
}

/// R9 复合守卫账：MemGuard 三类同时满载后仍可插。
pub fn r9_guard_all_capped() -> Result<(), u16> {
    let mut mg = MemGuard::new();
    for i in 0..(POOL_CAP + 10) as u64 {
        mg.pool.put(i);
    }
    for i in 0..(LEDGER_CAP + 10) as u32 {
        mg.ledger.put(i, i as u64);
    }
    for i in 0..(LOG_CAP + 10) as u32 {
        mg.log.push(i);
    }
    mg.audit()?;
    // 满载后再插入仍成功。
    mg.pool.put(0xDEAD);
    mg.ledger.put(0xCAFE, 1);
    mg.log.push(0xBEEF);
    mg.audit()?;
    Ok(())
}

/// R10 确定性重放：同种子两轮复合注入后指纹全等。
pub fn r10_deterministic() -> Result<(), u16> {
    let run = |seed: u64| -> Result<(u64, usize, u64), u16> {
        let mut g = Lcg::new(seed);
        let mut mg = MemGuard::new();
        for _ in 0..400u64 {
            mg.pool.put(g.next());
            mg.ledger.put((g.next() & 0xFF) as u32, g.next());
            mg.log.push((g.next() & 0xFF) as u32);
        }
        mg.audit()?;
        Ok((mg.pool.evicted, mg.log.len(), mg.log.written))
    };
    let a = run(0x48507_0003)?;
    let b = run(0x48507_0003)?;
    if a != b {
        return Err(E_NONDET);
    }
    Ok(())
}

/// 判据主入口：R1..R10，hits 计数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    r1_pool_capped()?;
    hits += 1;
    r2_pool_evict_oldest()?;
    hits += 1;
    r3_ledger_capped()?;
    hits += 1;
    r4_ledger_evict()?;
    hits += 1;
    r5_ledger_inplace()?;
    hits += 1;
    r6_log_wrap()?;
    hits += 1;
    r7_fuzz_bounded()?;
    hits += 1;
    r8_pool_ledger()?;
    hits += 1;
    r9_guard_all_capped()?;
    hits += 1;
    r10_deterministic()?;
    hits += 1;
    Ok(hits)
}
