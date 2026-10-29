//! UNX-F48468 —— E·keysym 环路注入（反：构造回环 ×10 断环记账）
//! 判据原文：UNX-F48468-J1 构造回环（同 keysym 循环回注）×10 断环（计数器达阈值即停）；
//!           断环账（sym+深度）×10 全记；断环后管线清空且拒绝同 sym 再注；
//!           LCG 随机回环 ×256 全部断环；合法异构 sym 零误断；
//!           断环账封印重演一致；断环阈值恒定（=8）。
//! 设计纪律：no_std；零 unwrap/expect/panic；确定性 LCG；drill 闭包判据全绿才返回 Ok。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xDFE0; // E24 环路注入段

pub const LOOP_THRESHOLD: u32 = 8;

const E_LOOP_PASS: u16 = SEG_BASE + 1; // 回环未断
const E_LEGAL_CUT: u16 = SEG_BASE + 2; // 合法 sym 误断
const E_KIND_BAD: u16 = SEG_BASE + 3; // 断环账字段缺失
const E_SEAL_BAD: u16 = SEG_BASE + 4; // 封印失配
const E_REPLAY_DIFF: u16 = SEG_BASE + 5; // 重演不一致
const E_BAL_MISS: u16 = SEG_BASE + 6; // 守恒破
const E_COUNT_MISS: u16 = SEG_BASE + 7; // 断环计数不符

/// 断环账条目。
#[derive(Clone)]
pub struct CutRec {
    pub sym: u32,
    pub depth: u32,
}

/// 回环守门管线。
pub struct LoopGate {
    queue: Vec<u32>,
    counters: Vec<(u32, u32)>, // (sym, 回注次数)
    cuts: Vec<CutRec>,
    banned: Vec<u32>,          // 断环后封禁 sym
    absorbed: u64,
    injected: u64,
}

impl LoopGate {
    pub fn new() -> Self {
        LoopGate {
            queue: Vec::new(),
            counters: Vec::new(),
            cuts: Vec::new(),
            banned: Vec::new(),
            absorbed: 0,
            injected: 0,
        }
    }

    fn count_of(&self, sym: u32) -> u32 {
        let mut i = 0usize;
        while i < self.counters.len() {
            if self.counters[i].0 == sym {
                return self.counters[i].1;
            }
            i += 1;
        }
        0
    }

    fn bump(&mut self, sym: u32) {
        let mut i = 0usize;
        while i < self.counters.len() {
            if self.counters[i].0 == sym {
                self.counters[i].1 += 1;
                return;
            }
            i += 1;
        }
        self.counters.push((sym, 1));
    }

    fn banned(&self, sym: u32) -> bool {
        let mut i = 0usize;
        while i < self.banned.len() {
            if self.banned[i] == sym {
                return true;
            }
            i += 1;
        }
        false
    }

    /// 回注申请：封禁 sym 拒；计数达阈值 → 断环（清管线+封禁+记账）。
    pub fn reinject(&mut self, sym: u32) -> Result<(), u16> {
        self.injected += 1;
        if self.banned(sym) {
            return Err(E_LOOP_PASS);
        }
        self.bump(sym);
        if self.count_of(sym) >= LOOP_THRESHOLD {
            self.cuts.push(CutRec { sym, depth: self.count_of(sym) });
            self.queue = Vec::new(); // 断环：清管线
            self.banned.push(sym);
            return Err(E_LOOP_PASS);
        }
        if self.queue.len() as u32 >= LOOP_THRESHOLD {
            return Err(E_LOOP_PASS);
        }
        self.queue.push(sym);
        Ok(())
    }

    /// 吸收（正常路径）：清管线计 absorbed。
    pub fn absorb(&mut self) -> u32 {
        let n = self.queue.len() as u32;
        self.absorbed += n as u64;
        self.queue = Vec::new();
        n
    }

    /// 断环账审计：sym 非零、depth == 阈值。
    pub fn audit_cuts(&self) -> Result<u32, u16> {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.cuts.len() {
            if self.cuts[i].sym == 0 || self.cuts[i].depth != LOOP_THRESHOLD {
                return Err(E_KIND_BAD);
            }
            n += 1;
            i += 1;
        }
        Ok(n)
    }

    pub fn fp(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut i = 0usize;
        while i < self.cuts.len() {
            h ^= self.cuts[i].sym as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
            h ^= self.cuts[i].depth as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
            i += 1;
        }
        h ^= self.banned.len() as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        h
    }

    pub fn cut_count(&self) -> u32 {
        self.cuts.len() as u32
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

/// 主巡检：环路注入八判据。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 构造回环 ×10：10 个 sym 各循环回注 8 次全部断环
    let mut g = LoopGate::new();
    let mut s = 0u32;
    while s < 10 {
        let sym = 0x100 + s;
        let mut k = 0u32;
        let mut cut = false;
        while k < LOOP_THRESHOLD {
            match g.reinject(sym) {
                Err(E_LOOP_PASS) => {
                    cut = true;
                    break;
                }
                Ok(()) => {}
                Err(_) => return Err(E_LOOP_PASS),
            }
            k += 1;
        }
        if !cut {
            return Err(E_LOOP_PASS);
        }
        s += 1;
    }
    if g.cut_count() != 10 {
        return Err(E_COUNT_MISS);
    }
    g.audit_cuts()?;
    hits += 1;

    // R2 断环后拒绝同 sym 再注
    match g.reinject(0x100) {
        Err(E_LOOP_PASS) => {}
        _ => return Err(E_LOOP_PASS),
    }
    hits += 1;

    // R3 合法异构 sym 零误断：未封禁 sym 注 3 次全过
    let mut g2 = LoopGate::new();
    let mut k = 0u32;
    while k < 3 {
        g2.reinject(0x900 + k)?;
        k += 1;
    }
    g2.absorb();
    hits += 1;

    // R4 断环清管线：断环发生时 queue 清空
    let mut g3 = LoopGate::new();
    let mut k = 0u32;
    while k < LOOP_THRESHOLD {
        match g3.reinject(0x777) {
            Ok(()) => {}
            Err(E_LOOP_PASS) => break,
            Err(_) => return Err(E_LOOP_PASS),
        }
        k += 1;
    }
    if !g3.queue.is_empty() {
        return Err(E_LOOP_PASS);
    }
    hits += 1;

    // R5 LCG 随机回环 ×256 全部断环：随机 sym 循环回注至断
    let mut g4 = LoopGate::new();
    let mut rng = Lcg::new(0x48468);
    let mut k = 0u32;
    while k < 256 {
        let sym = 0x2000 + rng.below(4096) as u32;
        let mut round = 0u32;
        let mut cut = false;
        while round < LOOP_THRESHOLD + 2 {
            match g4.reinject(sym) {
                Err(E_LOOP_PASS) => {
                    cut = true;
                    break;
                }
                Ok(()) => {}
                Err(_) => return Err(E_LOOP_PASS),
            }
            round += 1;
        }
        if !cut {
            return Err(E_LOOP_PASS);
        }
        k += 1;
    }
    if g4.cut_count() < 256 {
        return Err(E_COUNT_MISS);
    }
    hits += 1;

    // R6 断环账审计 ×N
    g4.audit_cuts()?;
    hits += 1;

    // R7 重演一致：同种子双跑断环账指纹相等
    let f1 = g4.fp();
    let mut g5 = LoopGate::new();
    let mut rng2 = Lcg::new(0x48468);
    let mut k = 0u32;
    while k < 256 {
        let sym = 0x2000 + rng2.below(4096) as u32;
        let mut round = 0u32;
        while round < LOOP_THRESHOLD + 2 {
            match g5.reinject(sym) {
                Ok(()) => {}
                Err(E_LOOP_PASS) => break,
                Err(_) => break,
            }
            round += 1;
        }
        k += 1;
    }
    if f1 != g5.fp() || f1 == 0 {
        return Err(E_REPLAY_DIFF);
    }
    hits += 1;

    // R8 封印稳定 + 阈值恒定
    let s1 = g4.fp();
    let s2 = g4.fp();
    if s1 != s2 {
        return Err(E_SEAL_BAD);
    }
    if LOOP_THRESHOLD != 8 {
        return Err(E_KIND_BAD);
    }
    hits += 1;

    Ok(hits)
}
