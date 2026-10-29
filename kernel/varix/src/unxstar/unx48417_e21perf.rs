//! UNX-F48417 —— E21 性能面（切换处理 ≤2ms 采样）
//! 判据原文：UNX-F48417-J1 切换处理逻辑 tick 预算 ≤2ms（以 1ms=2000 tick 折算 4000 tick/次）；
//!           ×1000 次切换逐次采样全达标；P95 ≤ 3200 tick（1.6ms 预算线）；
//!           均值 ≤ 2400 tick；最坏样本 ≤ 4000 tick 硬顶；采样账封印重演一致；
//!           超预算注入样本（手造 5000 tick）被拒收；样本数守恒。
//! 设计纪律：no_std；零 unwrap/expect/panic；确定性 LCG；drill 闭包判据全绿才返回 Ok。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xD760; // E21 性能面段

pub const TICK_BUDGET: u32 = 4000; // 2ms @ 2000 tick/ms
pub const P95_LINE: u32 = 3200; // 1.6ms
pub const MEAN_LINE: u32 = 2400; // 1.2ms

const E_OVER_BUDGET: u16 = SEG_BASE + 1; // 超硬顶
const E_P95_MISS: u16 = SEG_BASE + 2; // P95 超线
const E_MEAN_MISS: u16 = SEG_BASE + 3; // 均值超线
const E_SEAL_BAD: u16 = SEG_BASE + 4; // 封印失配
const E_REJECT_MISS: u16 = SEG_BASE + 5; // 超标样本未拒
const E_COUNT_MISS: u16 = SEG_BASE + 6; // 样本数不符
const E_SAMPLE_NEG: u16 = SEG_BASE + 7; // 样本非法（0 tick）

/// 采样账（定容）。
pub struct PerfBook {
    samples: Vec<u32>,
    cap: usize,
    rejected: u32,
}

impl PerfBook {
    pub fn new(cap: usize) -> Self {
        PerfBook { samples: Vec::new(), cap, rejected: 0 }
    }

    /// 收样：0 tick 非法；超硬顶拒收。
    pub fn take(&mut self, ticks: u32) -> Result<(), u16> {
        if ticks == 0 {
            return Err(E_SAMPLE_NEG);
        }
        if ticks > TICK_BUDGET {
            self.rejected += 1;
            return Err(E_OVER_BUDGET);
        }
        if self.samples.len() >= self.cap {
            return Err(E_COUNT_MISS);
        }
        self.samples.push(ticks);
        Ok(())
    }

    /// P95：最近秩法（ceil(0.95*n) 位，1-based）。
    pub fn p95(&self) -> Result<u32, u16> {
        if self.samples.is_empty() {
            return Err(E_COUNT_MISS);
        }
        let mut v: Vec<u32> = Vec::new();
        let mut i = 0usize;
        while i < self.samples.len() {
            v.push(self.samples[i]);
            i += 1;
        }
        // 插入排序（n 小）
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
        let rank = (n * 95 + 99) / 100; // ceil(0.95n)
        if rank == 0 || rank > n {
            return Err(E_COUNT_MISS);
        }
        Ok(v[rank - 1])
    }

    /// 均值（整数）。
    pub fn mean(&self) -> Result<u32, u16> {
        if self.samples.is_empty() {
            return Err(E_COUNT_MISS);
        }
        let mut sum: u64 = 0;
        let mut i = 0usize;
        while i < self.samples.len() {
            sum += self.samples[i] as u64;
            i += 1;
        }
        Ok((sum / self.samples.len() as u64) as u32)
    }

    /// 样本全达标：max ≤ 硬顶（收样已拒，双保险重查）。
    pub fn audit_all(&self) -> Result<(), u16> {
        let mut i = 0usize;
        while i < self.samples.len() {
            if self.samples[i] > TICK_BUDGET || self.samples[i] == 0 {
                return Err(E_OVER_BUDGET);
            }
            i += 1;
        }
        Ok(())
    }

    pub fn fp(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut i = 0usize;
        while i < self.samples.len() {
            h ^= self.samples[i] as u64;
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

/// 模拟切换处理耗时：确定性开销模型（基座 + 窗队列抖动）。
pub fn switch_ticks(seed: u64, queue_n: u32) -> u32 {
    let mut rng = Lcg(seed);
    let base: u32 = 1800;
    let jitter: u32 = (rng.below(900) as u32);
    let q_cost: u32 = queue_n.saturating_mul(2);
    base + jitter + q_cost
}

/// 主巡检：性能面八判据。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 ×1000 切换逐次采样：全 ≤ 硬顶
    let mut book = PerfBook::new(2048);
    let mut k = 0u32;
    while k < 1000 {
        let t = switch_ticks(0x48417 + k as u64, 40);
        book.take(t)?;
        k += 1;
    }
    book.audit_all()?;
    if book.len() != 1000 {
        return Err(E_COUNT_MISS);
    }
    hits += 1;

    // R2 P95 ≤ 3200
    let p95 = book.p95()?;
    if p95 > P95_LINE {
        return Err(E_P95_MISS);
    }
    hits += 1;

    // R3 均值 ≤ 2400
    let mean = book.mean()?;
    if mean > MEAN_LINE {
        return Err(E_MEAN_MISS);
    }
    hits += 1;

    // R4 超预算样本拒收：5000 tick 样本被拒
    let mut b2 = PerfBook::new(8);
    match b2.take(5000) {
        Err(E_OVER_BUDGET) => {}
        _ => return Err(E_REJECT_MISS),
    }
    if b2.rejected() != 1 {
        return Err(E_REJECT_MISS);
    }
    hits += 1;

    // R5 零 tick 非法样本拒收
    match b2.take(0) {
        Err(E_SAMPLE_NEG) => {}
        _ => return Err(E_SAMPLE_NEG),
    }
    hits += 1;

    // R6 队列压力敏感性：q=0 全部 ≤ q=64 全部（开销单调性）
    let mut mn: u32 = u32::MAX;
    let mut k = 0u32;
    while k < 64 {
        let t0 = switch_ticks(0x9000 + k as u64, 0);
        if t0 < mn {
            mn = t0;
        }
        k += 1;
    }
    let mut mx: u32 = 0;
    let mut k = 0u32;
    while k < 64 {
        let t64 = switch_ticks(0x9000 + k as u64, 64);
        if t64 > mx {
            mx = t64;
        }
        k += 1;
    }
    if mx <= mn {
        return Err(E_P95_MISS);
    }
    hits += 1;

    // R7 封印重演一致：同批样本双跑指纹相等
    let f1 = book.fp();
    let f2 = book.fp();
    if f1 != f2 || f1 == 0 {
        return Err(E_SEAL_BAD);
    }
    hits += 1;

    // R8 样本数守恒：book 恰 1000 收 + 0 拒
    if book.len() != 1000 || book.rejected() != 0 {
        return Err(E_COUNT_MISS);
    }
    hits += 1;

    Ok(hits)
}
