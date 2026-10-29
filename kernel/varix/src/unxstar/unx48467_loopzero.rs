//! UNX-F48467 —— E·keysym 回注环路检测（正：环路计数器零增长常态）
//! 判据原文：UNX-F48467-J1 回注管线常态（注入→转换→回注→吸收）环路计数器恒 0；
//!           ×1000 常态事件零增长；吸收率 100%（每注入必吸收）；
//!           LCG 随机 512 常态轮计数全零；管线账封印重演一致；
//!           深度有界（管线队列 ≤8）；计数器账守恒。
//! 设计纪律：no_std；零 unwrap/expect/panic；确定性 LCG；drill 闭包判据全绿才返回 Ok。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xDFC0; // E24 环路检测段

pub const PIPE_CAP: usize = 8;

const E_LOOP_GROW: u16 = SEG_BASE + 1; // 环路计数增长
const E_ABSORB_MISS: u16 = SEG_BASE + 2; // 吸收失败
const E_PIPE_OVER: u16 = SEG_BASE + 3; // 管线超容
const E_SEAL_BAD: u16 = SEG_BASE + 4; // 封印失配
const E_REPLAY_DIFF: u16 = SEG_BASE + 5; // 重演不一致
const E_BAL_MISS: u16 = SEG_BASE + 6; // 守恒破
const E_COUNT_MISS: u16 = SEG_BASE + 7; // 计数不符

/// 回注管线：注入 keysym → 转换 → 回注 → 吸收（常态一轮清空）。
pub struct Pipe {
    queue: Vec<u32>,   // 在途 keysym
    loop_counter: u32, // 环路计数器
    absorbed: u64,
    injected: u64,
}

impl Pipe {
    pub fn new() -> Self {
        Pipe { queue: Vec::new(), loop_counter: 0, absorbed: 0, injected: 0 }
    }

    /// 注入：容量守卫。
    pub fn inject(&mut self, sym: u32) -> Result<(), u16> {
        if self.queue.len() >= PIPE_CAP {
            return Err(E_PIPE_OVER);
        }
        self.queue.push(sym);
        self.injected += 1;
        Ok(())
    }

    /// 转换+回注一轮（常态）：吸收全部在途、计数器不变。
    pub fn drain_normal(&mut self) -> Result<u32, u16> {
        let n = self.queue.len() as u32;
        self.absorbed += n as u64;
        self.queue = Vec::new();
        // 常态：环路计数器不动
        Ok(n)
    }

    /// 环路事件：回注未吸收（计数器 +1）——仅供注入卡使用，常态判据断言不出现。
    pub fn loop_once(&mut self) {
        self.loop_counter += 1;
    }

    /// 常态判据：计数器 == 0。
    pub fn audit_zero_loop(&self) -> Result<(), u16> {
        if self.loop_counter != 0 {
            return Err(E_LOOP_GROW);
        }
        Ok(())
    }

    /// 吸收率 100%：absorbed == injected。
    pub fn audit_absorb(&self) -> Result<(), u16> {
        if self.absorbed != self.injected {
            return Err(E_ABSORB_MISS);
        }
        Ok(())
    }

    /// 管线有界。
    pub fn audit_pipe(&self) -> Result<(), u16> {
        if self.queue.len() > PIPE_CAP {
            return Err(E_PIPE_OVER);
        }
        Ok(())
    }

    pub fn fp(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        h ^= self.loop_counter as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        h ^= self.absorbed;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        h ^= self.injected;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        h ^= self.queue.len() as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        h
    }

    pub fn counter(&self) -> u32 {
        self.loop_counter
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

/// 主巡检：环路检测八判据。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 ×1000 常态事件零增长：注入→drain 全循环
    let mut p = Pipe::new();
    let mut k = 0u32;
    while k < 1000 {
        p.inject(0x100 + k % 32)?;
        p.drain_normal()?;
        p.audit_zero_loop()?;
        k += 1;
    }
    if p.counter() != 0 {
        return Err(E_LOOP_GROW);
    }
    hits += 1;

    // R2 吸收率 100%
    p.audit_absorb()?;
    hits += 1;

    // R3 管线有界：连注 8 满后第 9 拒
    let mut p2 = Pipe::new();
    let mut k = 0u32;
    while k < 8 {
        p2.inject(k)?;
        k += 1;
    }
    match p2.inject(99) {
        Err(E_PIPE_OVER) => {}
        _ => return Err(E_PIPE_OVER),
    }
    p2.drain_normal()?;
    p2.inject(99)?; // 清空后可注
    hits += 1;

    // R4 批量常态：单帧 8 注 + 1 drain 全吸收
    let mut p3 = Pipe::new();
    let mut k = 0u32;
    while k < 8 {
        p3.inject(0x50 + k)?;
        k += 1;
    }
    let n = p3.drain_normal()?;
    if n != 8 {
        return Err(E_ABSORB_MISS);
    }
    p3.audit_absorb()?;
    hits += 1;

    // R5 LCG 随机 512 常态轮计数全零：随机注入数（0..=8）+ drain
    let mut p4 = Pipe::new();
    let mut rng = Lcg::new(0x48467);
    let mut k = 0u32;
    while k < 512 {
        let m = rng.below(9) as u32;
        let mut j = 0u32;
        while j < m {
            p4.inject(0x70 + j)?;
            j += 1;
        }
        p4.drain_normal()?;
        p4.audit_zero_loop()?;
        k += 1;
    }
    if p4.counter() != 0 {
        return Err(E_LOOP_GROW);
    }
    hits += 1;

    // R6 重演一致：同种子双跑管线指纹相等
    let f1 = p4.fp();
    let mut p5 = Pipe::new();
    let mut rng2 = Lcg::new(0x48467);
    let mut k = 0u32;
    while k < 512 {
        let m = rng2.below(9) as u32;
        let mut j = 0u32;
        while j < m {
            let _ = p5.inject(0x70 + j);
            j += 1;
        }
        let _ = p5.drain_normal();
        k += 1;
    }
    if f1 != p5.fp() || f1 == 0 {
        return Err(E_REPLAY_DIFF);
    }
    hits += 1;

    // R7 计数器账守恒：injected == absorbed（常态）
    p4.audit_absorb()?;
    hits += 1;

    // R8 封印稳定
    let s1 = p4.fp();
    let s2 = p4.fp();
    if s1 != s2 {
        return Err(E_SEAL_BAD);
    }
    hits += 1;

    Ok(hits)
}
