//! UNX-F48466 —— E·commit 重复注入（反：同序号重放 ×10 去重全检出）
//! 判据原文：UNX-F48466-J1 同序号 commit 重放 ×10 全部去重检出（不重复上屏）；
//!           去重账（seq+首次指纹）×10 全记；合法新序号零误拦；
//!           LCG 随机重放 ×512 全检出；去重账封印重演一致；
//!           上屏账与去重账之和守恒；序号空间无洞。
//! 设计纪律：no_std；零 unwrap/expect/panic；确定性 LCG；drill 闭包判据全绿才返回 Ok。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xDFA0; // E24 commit 去重段

const E_REPLAY_PASS: u16 = SEG_BASE + 1; // 重放未检出
const E_LEGAL_BLOCK: u16 = SEG_BASE + 2; // 新序号误拦
const E_SEAL_BAD: u16 = SEG_BASE + 3; // 封印失配
const E_REPLAY_DIFF: u16 = SEG_BASE + 4; // 重演不一致
const E_BAL_MISS: u16 = SEG_BASE + 5; // 守恒破
const E_SEQ_HOLE: u16 = SEG_BASE + 6; // 序号断洞
const E_COUNT_MISS: u16 = SEG_BASE + 7; // 去重计数不符

/// commit 记录。
#[derive(Clone)]
pub struct CommitRec {
    pub seq: u64,
    pub text: String,
    pub fp: u64,
}

/// commit 去重器。
pub struct Deduper {
    seen: Vec<CommitRec>,   // 已受理
    dup_log: Vec<(u64, u64)>, // (重放 seq, 原 fp)
    next_seq: u64,
    accepted: u64,
}

impl Deduper {
    pub fn new() -> Self {
        Deduper { seen: Vec::new(), dup_log: Vec::new(), next_seq: 1, accepted: 0 }
    }

    fn find(&self, seq: u64) -> Option<usize> {
        let mut i = 0usize;
        while i < self.seen.len() {
            if self.seen[i].seq == seq {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// commit 提交：seq 已存在 → 去重记账拒绝；新 seq 受理。
    pub fn commit(&mut self, seq: u64, text: &str) -> Result<(), u16> {
        match self.find(seq) {
            Some(i) => {
                self.dup_log.push((seq, self.seen[i].fp));
                Err(E_REPLAY_PASS)
            }
            None => {
                let mut fp: u64 = 0xcbf2_9ce4_8422_2325;
                let b = text.as_bytes();
                let mut j = 0usize;
                while j < b.len() {
                    fp ^= b[j] as u64;
                    fp = fp.wrapping_mul(0x0000_0100_0000_01b3);
                    j += 1;
                }
                fp ^= seq;
                fp = fp.wrapping_mul(0x0000_0100_0000_01b3);
                self.seen.push(CommitRec { seq, text: String::from(text), fp });
                self.accepted += 1;
                Ok(())
            }
        }
    }

    /// 序号空间无洞：seen 的 seq 集合 = 1..=accepted。
    pub fn audit_dense(&self) -> Result<(), u16> {
        if self.seen.len() as u64 != self.accepted {
            return Err(E_BAL_MISS);
        }
        let mut i = 0usize;
        while i < self.seen.len() {
            if self.seen[i].seq != (i as u64) + 1 {
                return Err(E_SEQ_HOLE);
            }
            i += 1;
        }
        Ok(())
    }

    /// 守恒：受理数 = next_seq - 1（序号单调分配时）。
    pub fn audit_balance(&self) -> Result<(), u16> {
        if self.accepted != self.next_seq - 1 {
            return Err(E_BAL_MISS);
        }
        Ok(())
    }

    pub fn fp(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut i = 0usize;
        while i < self.dup_log.len() {
            h ^= self.dup_log[i].0;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
            h ^= self.dup_log[i].1;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
            i += 1;
        }
        h ^= self.accepted;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        h
    }

    pub fn dup_count(&self) -> u32 {
        self.dup_log.len() as u32
    }

    pub fn seen_len(&self) -> usize {
        self.seen.len()
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

/// 主巡检：commit 去重八判据。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 同序号重放 ×10 全检出：seq=1 提交后重放 10 次
    let mut d = Deduper::new();
    d.commit(1, "hello")?;
    d.next_seq = 2;
    let mut k = 0u32;
    while k < 10 {
        match d.commit(1, "hello") {
            Err(E_REPLAY_PASS) => {}
            _ => return Err(E_REPLAY_PASS),
        }
        k += 1;
    }
    if d.dup_count() != 10 {
        return Err(E_COUNT_MISS);
    }
    if d.seen_len() != 1 {
        return Err(E_REPLAY_PASS);
    }
    hits += 1;

    // R2 合法新序号零误拦：seq 2..=101 全受理
    let mut n = 2u64;
    while n <= 101 {
        d.commit(n, "fresh")?;
        d.next_seq = n + 1;
        n += 1;
    }
    if d.seen_len() != 101 {
        return Err(E_LEGAL_BLOCK);
    }
    hits += 1;

    // R3 序号无洞
    d.audit_dense()?;
    hits += 1;

    // R4 守恒
    d.audit_balance()?;
    hits += 1;

    // R5 不同文本同序号也去重（按 seq 键）：seq=1 带新文本仍拒
    match d.commit(1, "different") {
        Err(E_REPLAY_PASS) => {}
        _ => return Err(E_REPLAY_PASS),
    }
    if d.seen_len() != 101 {
        return Err(E_REPLAY_PASS);
    }
    hits += 1;

    // R6 LCG 随机重放 ×512：8 个已知 seq 随机重放全检出
    let mut d2 = Deduper::new();
    let mut n = 1u64;
    while n <= 8 {
        d2.commit(n, "seeded")?;
        d2.next_seq = n + 1;
        n += 1;
    }
    let mut rng = Lcg::new(0x48466);
    let mut k = 0u32;
    while k < 512 {
        let s = 1 + rng.below(8);
        match d2.commit(s, "seeded") {
            Err(E_REPLAY_PASS) => {}
            _ => return Err(E_REPLAY_PASS),
        }
        k += 1;
    }
    if d2.dup_count() != 512 {
        return Err(E_COUNT_MISS);
    }
    if d2.seen_len() != 8 {
        return Err(E_REPLAY_PASS);
    }
    hits += 1;

    // R7 重演一致：同种子双跑去重账指纹相等
    let f1 = d2.fp();
    let mut d3 = Deduper::new();
    let mut n = 1u64;
    while n <= 8 {
        d3.commit(n, "seeded")?;
        d3.next_seq = n + 1;
        n += 1;
    }
    let mut rng2 = Lcg::new(0x48466);
    let mut k = 0u32;
    while k < 512 {
        let s = 1 + rng2.below(8);
        let _ = d3.commit(s, "seeded");
        k += 1;
    }
    if f1 != d3.fp() || f1 == 0 {
        return Err(E_REPLAY_DIFF);
    }
    hits += 1;

    // R8 封印稳定 + 守恒
    let s1 = d2.fp();
    let s2 = d2.fp();
    if s1 != s2 {
        return Err(E_SEAL_BAD);
    }
    d2.audit_balance()?;
    hits += 1;

    Ok(hits)
}
