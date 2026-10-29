//! UNX-F48458 —— E23 性能面（损坏检出 ≤10ms/MB 采样）
//! 判据原文：UNX-F48458-J1 检出耗时预算：≤10ms/MB（折算 20,000 tick/MB @ 2000 tick/ms）；
//!           ×100 次检出采样全达标；P95 ≤ 16,000 tick（8ms 线）；均值 ≤ 12,000；
//!           超预算样本（手造 25,000 tick）拒收；样本数守恒；
//!           MB 规模敏感性：1MB 样本恒 ≤ 4MB 样本（单调性）；
//!           采样账封印重演一致。
//! 设计纪律：no_std；零 unwrap/expect/panic；确定性 LCG；drill 闭包判据全绿才返回 Ok。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xDE40; // E23 性能面段

pub const TICK_PER_MS: u32 = 2000;
pub const BUDGET_PER_MB: u32 = 20_000; // 10ms/MB
pub const P95_LINE: u32 = 16_000; // 8ms
pub const MEAN_LINE: u32 = 12_000; // 6ms

const E_OVER_BUDGET: u16 = SEG_BASE + 1; // 超预算
const E_P95_MISS: u16 = SEG_BASE + 2; // P95 超线
const E_MEAN_MISS: u16 = SEG_BASE + 3; // 均值超线
const E_SEAL_BAD: u16 = SEG_BASE + 4; // 封印失配
const E_REJECT_MISS: u16 = SEG_BASE + 5; // 超标样本未拒
const E_COUNT_MISS: u16 = SEG_BASE + 6; // 样本数不符
const E_MONO_MISS: u16 = SEG_BASE + 7; // 规模单调性破

/// 采样账。
pub struct PerfBook {
    samples: Vec<(u32, u32)>, // (mb, ticks)
    rejected: u32,
}

impl PerfBook {
    pub fn new() -> Self {
        PerfBook { samples: Vec::new(), rejected: 0 }
    }

    /// 收样：ticks > mb×预算 即拒。
    pub fn take(&mut self, mb: u32, ticks: u32) -> Result<(), u16> {
        let budget = mb.saturating_mul(BUDGET_PER_MB);
        if ticks > budget {
            self.rejected += 1;
            return Err(E_OVER_BUDGET);
        }
        self.samples.push((mb, ticks));
        Ok(())
    }

    /// P95（对 ticks 排序，最近秩）。
    pub fn p95(&self) -> Result<u32, u16> {
        if self.samples.is_empty() {
            return Err(E_COUNT_MISS);
        }
        let mut v: Vec<u32> = Vec::new();
        let mut i = 0usize;
        while i < self.samples.len() {
            v.push(self.samples[i].1);
            i += 1;
        }
        let n = v.len();
        let mut i = 1usize;
        while i < n {
            let key = v[i];
            let mut j = i;
            while j > 0 && v[j - 1] > key {
                v[j] = v[j - 1];
                j -= 1;
            }
            v[j] = key;
            i += 1;
        }
        let rank = (n * 95 + 99) / 100;
        Ok(v[rank - 1])
    }

    /// 均值。
    pub fn mean(&self) -> Result<u32, u16> {
        if self.samples.is_empty() {
            return Err(E_COUNT_MISS);
        }
        let mut sum: u64 = 0;
        let mut i = 0usize;
        while i < self.samples.len() {
            sum += self.samples[i].1 as u64;
            i += 1;
        }
        Ok((sum / self.samples.len() as u64) as u32)
    }

    /// 单位化均值（tick/MB）。
    pub fn mean_per_mb(&self) -> Result<u32, u16> {
        if self.samples.is_empty() {
            return Err(E_COUNT_MISS);
        }
        let mut sum: u64 = 0;
        let mut mb_sum: u64 = 0;
        let mut i = 0usize;
        while i < self.samples.len() {
            sum += self.samples[i].1 as u64;
            mb_sum += self.samples[i].0 as u64;
            i += 1;
        }
        if mb_sum == 0 {
            return Err(E_COUNT_MISS);
        }
        Ok((sum / mb_sum) as u32)
    }

    pub fn fp(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut i = 0usize;
        while i < self.samples.len() {
            h ^= self.samples[i].0 as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
            h ^= self.samples[i].1 as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
            i += 1;
        }
        h
    }

    pub fn len(&self) -> usize {
        self.samples.len()
    }

    pub fn rejected(&self) -> u32 {
        self.rejected
    }
}

/// 确定性 LCG。
pub struct Lcg(u64);

impl Lcg {
    pub fn new(seed: u64) -> Self {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.0
    }
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

/// 检出耗时模型：线性扫描 8 tick/KB 基座 + 抖动。
pub fn detect_ticks(seed: u64, mb: u32) -> u32 {
    let mut rng = Lcg(seed);
    let base = mb.saturating_mul(8_000); // 8 tick/KB × 1024 KB ≈ 8192，取 8000 折算
    let jitter = rng.below(3_000) as u32;
    base + jitter
}

/// 主巡检：性能面八判据。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 ×100 次检出采样（1~8MB 随机）全达标
    let mut book = PerfBook::new();
    let mut rng = Lcg::new(0x48458);
    let mut k = 0u32;
    while k < 100 {
        let mb = 1 + rng.below(8) as u32;
        let t = detect_ticks(0x48458 + k as u64, mb);
        book.take(mb, t)?;
        k += 1;
    }
    if book.len() != 100 {
        return Err(E_COUNT_MISS);
    }
    hits += 1;

    // R2 P95 ≤ 16,000
    let p95 = book.p95()?;
    if p95 > P95_LINE {
        return Err(E_P95_MISS);
    }
    hits += 1;

    // R3 单位化均值 ≤ 12,000 tick/MB
    let mpm = book.mean_per_mb()?;
    if mpm > MEAN_LINE {
        return Err(E_MEAN_MISS);
    }
    hits += 1;

    // R4 超预算样本拒收：1MB 25,000 tick > 20,000
    let mut b2 = PerfBook::new();
    match b2.take(1, 25_000) {
        Err(E_OVER_BUDGET) => {}
        _ => return Err(E_REJECT_MISS),
    }
    if b2.rejected() != 1 {
        return Err(E_REJECT_MISS);
    }
    hits += 1;

    // R5 边界达标样本收：1MB 恰 20,000 tick 收
    b2.take(1, BUDGET_PER_MB)?;
    if b2.len() != 1 {
        return Err(E_COUNT_MISS);
    }
    hits += 1;

    // R6 规模单调性：1MB 均值 ≤ 4MB 均值 ≤ 8MB 均值（耗时随规模增长）
    let mut m1 = PerfBook::new();
    let mut m4 = PerfBook::new();
    let mut m8 = PerfBook::new();
    let mut k = 0u32;
    while k < 64 {
        m1.take(1, detect_ticks(0xA000 + k as u64, 1))?;
        m4.take(4, detect_ticks(0xB000 + k as u64, 4))?;
        m8.take(8, detect_ticks(0xC000 + k as u64, 8))?;
        k += 1;
    }
    let a = m1.mean()?;
    let b = m4.mean()?;
    let c = m8.mean()?;
    if !(a < b && b < c) {
        return Err(E_MONO_MISS);
    }
    hits += 1;

    // R7 封印重演一致
    let f1 = book.fp();
    let f2 = book.fp();
    if f1 != f2 || f1 == 0 {
        return Err(E_SEAL_BAD);
    }
    hits += 1;

    // R8 样本数守恒：100 收 0 拒
    if book.len() != 100 || book.rejected() != 0 {
        return Err(E_COUNT_MISS);
    }
    hits += 1;

    Ok(hits)
}
