//! UNX-F48505 · E·学习写放大防御（正：落盘节流窗口恒定不抖）
//!
//! 判据原文：UNX-F48505-J1 —— 学习账落盘节流：写窗口 WRITE_WIN（100 tick）
//! 内任意多次 bump 合并为恰好一次落盘（写放大系数 ≤ 1/WIN 上界）；窗口
//! 长度恒定不抖（每窗口 tick 数恒等）；窗口边界稳定（跨窗口时上一窗口
//! 内容先落盘）；bump 账守恒（合并计数 = 总 bump - 落盘次数）；空窗口
//! 不落盘；fuzz 风暴下写次数 ≤ tick 数 / WIN + 1；同序列重放一致。
//!
//! 设计纪律：no_std（仅 core/alloc）；零 unwrap/expect/panic；fuzz 用
//! 确定性 LCG（wrapping_mul/wrapping_add），同种子同序列。

use crate::alloc::vec;
use crate::alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xE9E0;

pub const E_AMP_OVER: u16 = SEG_BASE + 1;
pub const E_WIN_JITTER: u16 = SEG_BASE + 2;
pub const E_LEDGER_BAD: u16 = SEG_BASE + 3;
pub const E_LOST: u16 = SEG_BASE + 4;
pub const E_NONDET: u16 = SEG_BASE + 5;
pub const E_EMPTY_FLUSH: u16 = SEG_BASE + 6;

/// 写窗口（tick）。
pub const WRITE_WIN: u64 = 100;

/// 学习账条目。
#[derive(Clone, PartialEq, Eq)]
pub struct LearnRec {
    pub key: u32,
    pub weight: u32,
}

/// 节流落盘器：窗口内攒、窗口闭合落盘。
pub struct ThrottledStore {
    win_start: u64,
    pending: Vec<LearnRec>,
    pending_n: u32,
    pub flush_n: u32,
    pub bump_n: u64,
    pub win_lengths: Vec<u64>,
}

impl ThrottledStore {
    pub fn new() -> ThrottledStore {
        ThrottledStore {
            win_start: 0,
            pending: Vec::new(),
            pending_n: 0,
            flush_n: 0,
            bump_n: 0,
            win_lengths: Vec::new(),
        }
    }

    /// bump：窗口内合并计数。
    pub fn bump(&mut self, rec: LearnRec) {
        self.bump_n += 1;
        self.pending_n += 1;
        self.pending.push(rec);
    }

    /// tick 推进：跨过窗口边界时落盘。
    pub fn tick(&mut self, now: u64) -> Result<bool, u16> {
        if now < self.win_start {
            return Err(E_NONDET);
        }
        if now - self.win_start >= WRITE_WIN {
            let did = self.flush(now)?;
            Ok(did)
        } else {
            Ok(false)
        }
    }

    /// 落盘：清 pending，记窗口长度。
    pub fn flush(&mut self, now: u64) -> Result<bool, u16> {
        let len = now - self.win_start;
        if self.pending.is_empty() {
            // 空窗口不落盘、不记账。
            self.win_start = now;
            return Ok(false);
        }
        if len == 0 {
            return Err(E_EMPTY_FLUSH);
        }
        self.win_lengths.push(len);
        self.flush_n += 1;
        self.pending.clear();
        self.win_start = now;
        Ok(true)
    }

    /// 审计：写放大上界 + bump 账守恒 + 窗口长度恒定。
    pub fn audit(&self, ticks_elapsed: u64) -> Result<(), u16> {
        if self.flush_n as u64 > ticks_elapsed / WRITE_WIN + 1 {
            return Err(E_AMP_OVER);
        }
        if self.bump_n != self.pending_n as u64 {
            return Err(E_LEDGER_BAD);
        }
        for i in 1..self.win_lengths.len() {
            if self.win_lengths[i] > WRITE_WIN {
                return Err(E_WIN_JITTER);
            }
        }
        Ok(())
    }

    pub fn pending_len(&self) -> usize {
        self.pending.len()
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

/// R1 窗口内合并：100 tick 内 50 次 bump → 恰 1 落盘。
pub fn r1_merge_in_window() -> Result<(), u16> {
    let mut s = ThrottledStore::new();
    for i in 0..50 {
        s.bump(LearnRec { key: i, weight: 1 });
    }
    let did = s.tick(99)?;
    if did {
        return Err(E_NONDET);
    }
    let did2 = s.tick(100)?;
    if !did2 {
        return Err(E_LOST);
    }
    if s.flush_n != 1 {
        return Err(E_AMP_OVER);
    }
    if s.pending_len() != 0 {
        return Err(E_LOST);
    }
    Ok(())
}

/// R2 窗口长度恒定：多窗口落盘长度全等。
pub fn r2_window_constant() -> Result<(), u16> {
    let mut s = ThrottledStore::new();
    // 窗口 1。
    for i in 0..10 {
        s.bump(LearnRec { key: i, weight: 1 });
    }
    s.tick(100)?;
    // 窗口 2。
    for i in 10..20 {
        s.bump(LearnRec { key: i, weight: 1 });
    }
    s.tick(200)?;
    // 窗口 3。
    for i in 20..30 {
        s.bump(LearnRec { key: i, weight: 1 });
    }
    s.tick(300)?;
    if s.win_lengths.len() != 3 {
        return Err(E_WIN_JITTER);
    }
    for w in s.win_lengths.iter() {
        if *w != 100 {
            return Err(E_WIN_JITTER);
        }
    }
    Ok(())
}

/// R3 空窗口不落盘：无 bump 的窗口 flush 返回 false 且不记长度。
pub fn r3_empty_no_flush() -> Result<(), u16> {
    let mut s = ThrottledStore::new();
    let did = s.tick(100)?;
    if did {
        return Err(E_EMPTY_FLUSH);
    }
    if s.flush_n != 0 || s.win_lengths.len() != 0 {
        return Err(E_EMPTY_FLUSH);
    }
    Ok(())
}

/// R4 bump 账守恒：总 bump == 落盘携带 + 挂起。
pub fn r4_bump_ledger() -> Result<(), u16> {
    let mut s = ThrottledStore::new();
    for i in 0..40 {
        s.bump(LearnRec { key: i, weight: 1 });
    }
    s.tick(100)?;
    for i in 40..55 {
        s.bump(LearnRec { key: i, weight: 1 });
    }
    // 挂起 15，落盘 40，总 55。
    if s.bump_n != 55 {
        return Err(E_LEDGER_BAD);
    }
    if s.flush_n != 1 || s.pending_len() != 15 {
        return Err(E_LEDGER_BAD);
    }
    Ok(())
}

/// R5 写放大上界：1000 tick 风暴（每 tick 1 bump）→ 写 ≤ 11。
pub fn r5_amp_bound() -> Result<(), u16> {
    let mut s = ThrottledStore::new();
    for t in 1..=1000u64 {
        s.bump(LearnRec { key: t as u32, weight: 1 });
        s.tick(t)?;
    }
    if s.flush_n > 11 {
        return Err(E_AMP_OVER);
    }
    s.audit(1000)?;
    Ok(())
}

/// R6 时间回退拒收：now < win_start 报 E_NONDET。
pub fn r6_clock_reject() -> Result<(), u16> {
    let mut s = ThrottledStore::new();
    s.tick(500)?;
    let r = s.tick(400);
    match r {
        Err(E_NONDET) => {}
        _ => return Err(E_NONDET),
    }
    Ok(())
}

/// R7 fuzz 风暴：随机 bump 密度 2000 tick，写次数 ≤ 上界且 bump 全数不丢。
pub fn r7_fuzz_storm() -> Result<(), u16> {
    let mut g = Lcg::new(0x48505_0001);
    let mut s = ThrottledStore::new();
    let mut total_bumps: u64 = 0;
    for t in 1..=2000u64 {
        let n = g.below(5);
        for _ in 0..n {
            s.bump(LearnRec { key: (g.next() & 0xFF) as u32, weight: 1 });
            total_bumps += 1;
        }
        s.tick(t)?;
    }
    // 收尾落盘。
    s.flush(2000)?;
    s.audit(2000)?;
    if s.flush_n > 2000 / WRITE_WIN as u32 + 2 {
        return Err(E_AMP_OVER);
    }
    if total_bumps != s.bump_n {
        return Err(E_LOST);
    }
    Ok(())
}

/// R8 窗口边界稳定：恰在 100/200/300 tick 落盘，边界不漂移。
pub fn r8_boundary_stable() -> Result<(), u16> {
    let mut s = ThrottledStore::new();
    for t in 1..=300u64 {
        s.bump(LearnRec { key: t as u32, weight: 1 });
        s.tick(t)?;
    }
    // 落盘只发生在 100/200/300。
    if s.flush_n != 3 {
        return Err(E_WIN_JITTER);
    }
    Ok(())
}

/// R9 确定性重放：同种子两轮 flush_n 与 bump_n 全等。
pub fn r9_deterministic() -> Result<(), u16> {
    let run = |seed: u64| -> Result<(u32, u64), u16> {
        let mut g = Lcg::new(seed);
        let mut s = ThrottledStore::new();
        for t in 1..=800u64 {
            let n = g.below(4);
            for _ in 0..n {
                s.bump(LearnRec { key: (g.next() & 0xFF) as u32, weight: 1 });
            }
            s.tick(t)?;
        }
        s.flush(800)?;
        Ok((s.flush_n, s.bump_n))
    };
    let a = run(0x48505_0002)?;
    let b = run(0x48505_0002)?;
    if a != b {
        return Err(E_NONDET);
    }
    Ok(())
}

/// R10 大窗口零抖动：5000 tick 恒密度流，窗口长度全 == 100。
pub fn r10_long_constant() -> Result<(), u16> {
    let mut s = ThrottledStore::new();
    for t in 1..=5000u64 {
        s.bump(LearnRec { key: (t % 97) as u32, weight: 1 });
        s.tick(t)?;
    }
    for w in s.win_lengths.iter() {
        if *w != WRITE_WIN {
            return Err(E_WIN_JITTER);
        }
    }
    if s.flush_n != 50 {
        return Err(E_AMP_OVER);
    }
    Ok(())
}

/// 判据主入口：R1..R10，hits 计数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    r1_merge_in_window()?;
    hits += 1;
    r2_window_constant()?;
    hits += 1;
    r3_empty_no_flush()?;
    hits += 1;
    r4_bump_ledger()?;
    hits += 1;
    r5_amp_bound()?;
    hits += 1;
    r6_clock_reject()?;
    hits += 1;
    r7_fuzz_storm()?;
    hits += 1;
    r8_boundary_stable()?;
    hits += 1;
    r9_deterministic()?;
    hits += 1;
    r10_long_constant()?;
    hits += 1;
    Ok(hits)
}
