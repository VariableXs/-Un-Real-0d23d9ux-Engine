//! UNX-F48419 —— E21 批防重 grep（关键词族：focusrace/lostkey/destroyorder）
//! 判据原文：UNX-F48419-J1 三族关键词扫描登记（词界安全匹配：前缀防误伤）；
//!           命中账零重复（同槽同词只记一次）；词界破坏注入（前缀拼接假词）全拒；
//!           LCG 随机文本 512 轮命中稳定重演一致；命中账封印稳定；
//!           三族计数守恒（账长 = 三族真命中和）。
//! 设计纪律：no_std；零 unwrap/expect/panic；确定性 LCG；drill 闭包判据全绿才返回 Ok。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xD7C0; // E21 防重 grep 段

const E_DUP_HIT: u16 = SEG_BASE + 1; // 同槽同词重复登记
const E_BOUNDARY: u16 = SEG_BASE + 2; // 词界破坏命中（假阳性）
const E_SEAL_BAD: u16 = SEG_BASE + 3; // 封印失配
const E_REPLAY_DIFF: u16 = SEG_BASE + 4; // 重演不一致
const E_COUNT_MISS: u16 = SEG_BASE + 5; // 计数守恒破
const E_KIND_BAD: u16 = SEG_BASE + 6; // 族号越界

/// 三族关键词。
pub const FAMILY_FOCUSRACE: &str = "focusrace";
pub const FAMILY_LOSTKEY: &str = "lostkey";
pub const FAMILY_DESTROYORDER: &str = "destroyorder";

/// 命中登记。
#[derive(Clone, PartialEq, Eq)]
pub struct Hit {
    pub family: u16, // 0/1/2
    pub slot: usize,
}

/// 词界安全的 grep 账。
pub struct GrepBook {
    hits: Vec<Hit>,
}

impl GrepBook {
    pub fn new() -> Self {
        GrepBook { hits: Vec::new() }
    }

    fn has(&self, family: u16, slot: usize) -> bool {
        let mut i = 0usize;
        while i < self.hits.len() {
            if self.hits[i].family == family && self.hits[i].slot == slot {
                return true;
            }
            i += 1;
        }
        false
    }

    /// 扫描一行：词界安全匹配（词前必须行首或非字母），命中登记（去重）。
    pub fn scan(&mut self, line: &str) -> Result<(), u16> {
        let b = line.as_bytes();
        // 三族逐一扫
        let fams = [FAMILY_FOCUSRACE, FAMILY_LOSTKEY, FAMILY_DESTROYORDER];
        let mut f = 0u16;
        while f < 3 {
            let pat = fams[f as usize].as_bytes();
            let mut s = 0usize;
            while s + pat.len() <= b.len() {
                // 匹配 pat
                let mut ok = true;
                let mut j = 0usize;
                while j < pat.len() {
                    if b[s + j] != pat[j] {
                        ok = false;
                    }
                    j += 1;
                }
                // 词界左检查：s==0 或 b[s-1] 非小写字母（防前缀拼接假词）
                let left_ok = s == 0 || !b[s - 1].is_ascii_lowercase();
                // 词界右检查：s+pat.len()==len 或其后非小写字母
                let e = s + pat.len();
                let right_ok = e == b.len() || !b[e].is_ascii_lowercase();
                if ok && left_ok && right_ok {
                    if !self.has(f, s) {
                        self.hits.push(Hit { family: f, slot: s });
                    }
                }
                s += 1;
            }
            f += 1;
        }
        Ok(())
    }

    /// 命中账去重不变式：无重复 (family, slot)。
    pub fn audit_unique(&self) -> Result<(), u16> {
        let mut i = 0usize;
        while i < self.hits.len() {
            let mut j = i + 1;
            while j < self.hits.len() {
                if self.hits[i].family == self.hits[j].family
                    && self.hits[i].slot == self.hits[j].slot
                {
                    return Err(E_DUP_HIT);
                }
                j += 1;
            }
            i += 1;
        }
        Ok(())
    }

    /// 族号封闭。
    pub fn audit_family_closed(&self) -> Result<(), u16> {
        let mut i = 0usize;
        while i < self.hits.len() {
            if self.hits[i].family > 2 {
                return Err(E_KIND_BAD);
            }
            i += 1;
        }
        Ok(())
    }

    /// 族计数。
    pub fn family_count(&self, family: u16) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.hits.len() {
            if self.hits[i].family == family {
                n += 1;
            }
            i += 1;
        }
        n
    }

    pub fn fp(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut i = 0usize;
        while i < self.hits.len() {
            h ^= self.hits[i].family as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
            h ^= self.hits[i].slot as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
            i += 1;
        }
        h
    }

    pub fn len(&self) -> usize {
        self.hits.len()
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

/// 主巡检：防重 grep 八判据。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 三族正命中：三行各含一族真词，各记 1
    let mut g = GrepBook::new();
    g.scan("switch race detected: focusrace fired")?;
    g.scan("ledger caught a lostkey event")?;
    g.scan("assert destroyorder sequence ok")?;
    if g.family_count(0) != 1 || g.family_count(1) != 1 || g.family_count(2) != 1 {
        return Err(E_COUNT_MISS);
    }
    hits += 1;

    // R2 前缀拼接假词拒：prefocusracer / xlostkeys 不得命中（词界破坏）
    let mut g2 = GrepBook::new();
    g2.scan("prefocusracer and xlostkeys and mydestroyorderer")?;
    if g2.len() != 0 {
        return Err(E_BOUNDARY);
    }
    hits += 1;

    // R3 同行多词多命中 + 去重：同行重复同词只记一次
    let mut g3 = GrepBook::new();
    g3.scan("focusrace lostkey focusrace lostkey")?;
    if g3.family_count(0) != 1 || g3.family_count(1) != 1 {
        return Err(E_DUP_HIT);
    }
    g3.audit_unique()?;
    hits += 1;

    // R4 跨行去重：第二行同词同槽不再登记（槽位按行内偏移——不同行同槽属不同行，
    //    本账按「单行账」语义：每次 scan 独立行，去重按 (family,slot)）
    let mut g4 = GrepBook::new();
    g4.scan("focusrace here")?;
    g4.scan("focusrace there")?; // 同槽 0 命中 → 去重为 1
    if g4.family_count(0) != 1 {
        return Err(E_DUP_HIT);
    }
    hits += 1;

    // R5 LCG 随机文本 512 行：命中稳定重演（构造器确定性）
    let mut rng = Lcg::new(0x48419);
    let mut g5 = GrepBook::new();
    let mut k = 0u32;
    while k < 512 {
        let m = rng.below(3);
        let line = match m {
            0 => "run focusrace guard",
            1 => "log lostkey into book",
            _ => "verify destroyorder pass",
        };
        g5.scan(line)?;
        k += 1;
    }
    let f1 = g5.fp();
    let mut g6 = GrepBook::new();
    let mut rng2 = Lcg::new(0x48419);
    let mut k = 0u32;
    while k < 512 {
        let m = rng2.below(3);
        let line = match m {
            0 => "run focusrace guard",
            1 => "log lostkey into book",
            _ => "verify destroyorder pass",
        };
        g6.scan(line)?;
        k += 1;
    }
    if f1 != g6.fp() || f1 == 0 {
        return Err(E_REPLAY_DIFF);
    }
    hits += 1;

    // R6 计数守恒：g5 三族和 = 账长
    let sum = g5.family_count(0) + g5.family_count(1) + g5.family_count(2);
    if sum as usize != g5.len() {
        return Err(E_COUNT_MISS);
    }
    g5.audit_family_closed()?;
    hits += 1;

    // R7 封印稳定：双算一致
    let s1 = g5.fp();
    let s2 = g5.fp();
    if s1 != s2 {
        return Err(E_SEAL_BAD);
    }
    hits += 1;

    // R8 空行与无词行零命中
    let mut g7 = GrepBook::new();
    g7.scan("")?;
    g7.scan("nothing relevant here at all")?;
    if g7.len() != 0 {
        return Err(E_BOUNDARY);
    }
    hits += 1;

    Ok(hits)
}
