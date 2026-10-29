//! UNX-F48459 —— E23 批防重 grep（关键词族：corrupt/rollback/fallback）
//! 判据原文：UNX-F48459-J1 三族关键词扫描登记（词界安全）；命中账零重复；
//!           词界破坏注入（前缀/后缀拼接假词）全拒；LCG 随机文本 512 轮重演一致；
//!           命中账封印稳定；三族计数守恒；空行零命中。
//! 设计纪律：no_std；零 unwrap/expect/panic；确定性 LCG；drill 闭包判据全绿才返回 Ok。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xDE60; // E23 防重 grep 段

const E_DUP_HIT: u16 = SEG_BASE + 1; // 同槽同词重复
const E_BOUNDARY: u16 = SEG_BASE + 2; // 词界破坏假阳性
const E_SEAL_BAD: u16 = SEG_BASE + 3; // 封印失配
const E_REPLAY_DIFF: u16 = SEG_BASE + 4; // 重演不一致
const E_COUNT_MISS: u16 = SEG_BASE + 5; // 计数守恒破
const E_KIND_BAD: u16 = SEG_BASE + 6; // 族号越界

pub const FAM_CORRUPT: &str = "corrupt";
pub const FAM_ROLLBACK: &str = "rollback";
pub const FAM_FALLBACK: &str = "fallback";

/// 命中登记。
#[derive(Clone, PartialEq, Eq)]
pub struct Hit {
    pub family: u16,
    pub slot: usize,
}

/// 词界安全 grep 账。
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

    /// 扫描一行：三族逐一、词界双检。
    pub fn scan(&mut self, line: &str) -> Result<(), u16> {
        let b = line.as_bytes();
        let fams = [FAM_CORRUPT, FAM_ROLLBACK, FAM_FALLBACK];
        let mut f = 0u16;
        while f < 3 {
            let pat = fams[f as usize].as_bytes();
            let mut s = 0usize;
            while s + pat.len() <= b.len() {
                let mut ok = true;
                let mut j = 0usize;
                while j < pat.len() {
                    if b[s + j] != pat[j] {
                        ok = false;
                    }
                    j += 1;
                }
                let left_ok = s == 0 || !b[s - 1].is_ascii_lowercase();
                let e = s + pat.len();
                let right_ok = e == b.len() || !b[e].is_ascii_lowercase();
                if ok && left_ok && right_ok && !self.has(f, s) {
                    self.hits.push(Hit { family: f, slot: s });
                }
                s += 1;
            }
            f += 1;
        }
        Ok(())
    }

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

    // R1 三族正命中各 1
    let mut g = GrepBook::new();
    g.scan("state corrupt detected")?;
    g.scan("rollback to snapshot ok")?;
    g.scan("fallback path taken")?;
    if g.family_count(0) != 1 || g.family_count(1) != 1 || g.family_count(2) != 1 {
        return Err(E_COUNT_MISS);
    }
    hits += 1;

    // R2 前缀拼接假词拒：xcorrupts / rollbackx / myfallbacks
    let mut g2 = GrepBook::new();
    g2.scan("xcorrupts rollbackx myfallbacks")?;
    if g2.len() != 0 {
        return Err(E_BOUNDARY);
    }
    hits += 1;

    // R3 同行重复去重：corrupt ×2 只记 1
    let mut g3 = GrepBook::new();
    g3.scan("corrupt then corrupt again")?;
    if g3.family_count(0) != 1 {
        return Err(E_DUP_HIT);
    }
    g3.audit_unique()?;
    hits += 1;

    // R4 跨行同槽去重：单行账语义
    let mut g4 = GrepBook::new();
    g4.scan("corrupt here")?;
    g4.scan("corrupt there")?;
    if g4.family_count(0) != 1 {
        return Err(E_DUP_HIT);
    }
    hits += 1;

    // R5 fallback 是 rollback 的子串？——不是（词界互不误伤）：
    // "rollback" 含 "back" 不含 "fallback"；校验互不串扰
    let mut g5 = GrepBook::new();
    g5.scan("rollback only")?;
    if g5.family_count(2) != 0 {
        return Err(E_BOUNDARY);
    }
    hits += 1;

    // R6 LCG 随机 512 行重演一致
    let mut rng = Lcg::new(0x48459);
    let mut g6 = GrepBook::new();
    let lines = ["corrupt event", "rollback done", "fallback used", "nothing here"];
    let mut k = 0u32;
    while k < 512 {
        let ln = lines[rng.below(4) as usize];
        g6.scan(ln)?;
        k += 1;
    }
    let f1 = g6.fp();
    let mut g7 = GrepBook::new();
    let mut rng2 = Lcg::new(0x48459);
    let mut k = 0u32;
    while k < 512 {
        let ln = lines[rng2.below(4) as usize];
        g7.scan(ln)?;
        k += 1;
    }
    if f1 != g7.fp() || f1 == 0 {
        return Err(E_REPLAY_DIFF);
    }
    hits += 1;

    // R7 计数守恒 + 族封闭
    let sum = g6.family_count(0) + g6.family_count(1) + g6.family_count(2);
    if sum as usize != g6.len() {
        return Err(E_COUNT_MISS);
    }
    g6.audit_family_closed()?;
    hits += 1;

    // R8 空行零命中
    let mut g8 = GrepBook::new();
    g8.scan("")?;
    g8.scan("clean line")?;
    if g8.len() != 0 {
        return Err(E_BOUNDARY);
    }
    hits += 1;

    Ok(hits)
}
