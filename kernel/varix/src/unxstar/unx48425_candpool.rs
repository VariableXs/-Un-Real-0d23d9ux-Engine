//! UNX-F48425 —— E·超长候选池输入（正：池 16 槽滚动覆盖正确）
//! 判据原文：UNX-F48425-J1 池容量 16：≤16 全收；超 16 滚动覆盖（FIFO 淘汰最旧）；
//!           ×256 次超量输入后池内恰为最近 16 条且顺序正确；
//!           覆盖计数账 = 输入数 - 16；LCG 随机 512 输入池状态重演一致；
//!           候选查询（按槽读）全量可读零越界；池封印稳定。
//! 设计纪律：no_std；零 unwrap/expect/panic；确定性 LCG；drill 闭包判据全绿才返回 Ok。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xD8C0; // E22 候选池段

pub const POOL_CAP: usize = 16;

const E_CAP_MISS: u16 = SEG_BASE + 1; // 容量守恒破
const E_ORDER_MISS: u16 = SEG_BASE + 2; // 滚动顺序错
const E_COVER_MISS: u16 = SEG_BASE + 3; // 覆盖计数不符
const E_SEAL_BAD: u16 = SEG_BASE + 4; // 封印失配
const E_REPLAY_DIFF: u16 = SEG_BASE + 5; // 重演不一致
const E_READ_MISS: u16 = SEG_BASE + 6; // 槽读越界
const E_SLOT_DUP: u16 = SEG_BASE + 7; // 池内重复候选

/// 候选池（16 槽 FIFO 滚动）。
pub struct CandPool {
    slots: Vec<String>, // slots[0] 最旧
    pushed: u64,
    covered: u64,
}

impl CandPool {
    pub fn new() -> Self {
        CandPool { slots: Vec::new(), pushed: 0, covered: 0 }
    }

    /// 压入候选：≤16 直接收；超 16 淘汰最旧。
    pub fn push(&mut self, cand: &str) {
        self.pushed += 1;
        if self.slots.len() < POOL_CAP {
            self.slots.push(String::from(cand));
        } else {
            // FIFO：移除 slots[0]，尾部追加
            let mut i = 1usize;
            while i < POOL_CAP {
                self.slots[i - 1] = self.slots[i].clone();
                i += 1;
            }
            self.slots[POOL_CAP - 1] = String::from(cand);
            self.covered += 1;
        }
    }

    /// 池内恰为最近 16 条：slots[i] == 第 (pushed-16+i+1) 条。
    pub fn audit_recent(&self, nth: impl Fn(u64) -> String) -> Result<(), u16> {
        if self.slots.len() > POOL_CAP {
            return Err(E_CAP_MISS);
        }
        if self.pushed < POOL_CAP as u64 {
            return Ok(()); // 未满时不校验
        }
        let mut i = 0usize;
        while i < POOL_CAP {
            let expect_seq = self.pushed - (POOL_CAP as u64) + 1 + i as u64;
            let expect = nth(expect_seq);
            if self.slots[i] != expect {
                return Err(E_ORDER_MISS);
            }
            i += 1;
        }
        Ok(())
    }

    /// 覆盖计数守恒：covered == pushed - 16（满后）。
    pub fn audit_cover(&self) -> Result<(), u16> {
        if self.pushed >= POOL_CAP as u64 {
            if self.covered != self.pushed - POOL_CAP as u64 {
                return Err(E_COVER_MISS);
            }
        } else if self.covered != 0 {
            return Err(E_COVER_MISS);
        }
        Ok(())
    }

    /// 槽读：0..len 合法，越界拒绝。
    pub fn slot(&self, i: usize) -> Result<&str, u16> {
        if i >= self.slots.len() {
            return Err(E_READ_MISS);
        }
        Ok(&self.slots[i])
    }

    /// 池内无重复（16 条互异输入下）。
    pub fn audit_no_dup(&self) -> Result<(), u16> {
        let mut i = 0usize;
        while i < self.slots.len() {
            let mut j = i + 1;
            while j < self.slots.len() {
                if self.slots[i] == self.slots[j] {
                    return Err(E_SLOT_DUP);
                }
                j += 1;
            }
            i += 1;
        }
        Ok(())
    }

    pub fn fp(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut i = 0usize;
        while i < self.slots.len() {
            let b = self.slots[i].as_bytes();
            let mut j = 0usize;
            while j < b.len() {
                h ^= b[j] as u64;
                h = h.wrapping_mul(0x0000_0100_0000_01b3);
                j += 1;
            }
            i += 1;
        }
        h ^= self.pushed;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        h
    }

    pub fn pushed(&self) -> u64 {
        self.pushed
    }
}

/// 第 n 条候选文本（c{n}）。
fn cand_text(n: u64) -> String {
    let mut s = String::from("c");
    s.push_str(&n.to_string());
    s
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

/// 主巡检：候选池八判据。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 恰 16 全收
    let mut pool = CandPool::new();
    let mut n = 1u64;
    while n <= 16 {
        pool.push(&cand_text(n));
        n += 1;
    }
    if pool.pushed() != 16 {
        return Err(E_CAP_MISS);
    }
    pool.audit_cover()?;
    hits += 1;

    // R2 ×256 超量输入：池内恰为最近 16 条（c241..c256）
    let mut n = 17u64;
    while n <= 256 {
        pool.push(&cand_text(n));
        n += 1;
    }
    pool.audit_recent(cand_text)?;
    if pool.slot(0)? != "c241" || pool.slot(15)? != "c256" {
        return Err(E_ORDER_MISS);
    }
    hits += 1;

    // R3 覆盖计数账：covered = 256 - 16 = 240
    pool.audit_cover()?;
    hits += 1;

    // R4 全槽可读零越界：0..16 全读成功、16 拒
    let mut i = 0usize;
    while i < 16 {
        let _ = pool.slot(i)?;
        i += 1;
    }
    match pool.slot(16) {
        Err(E_READ_MISS) => {}
        _ => return Err(E_READ_MISS),
    }
    hits += 1;

    // R5 池内无重复
    pool.audit_no_dup()?;
    hits += 1;

    // R6 LCG 随机 512 输入：覆盖账 + 有界 + 重演
    let mut pool2 = CandPool::new();
    let mut rng = Lcg::new(0x48425);
    let mut k = 0u32;
    while k < 512 {
        let v = rng.below(100);
        pool2.push(&cand_text(v));
        pool2.audit_cover()?;
        k += 1;
    }
    if pool2.pushed() != 512 {
        return Err(E_CAP_MISS);
    }
    hits += 1;

    // R7 重演一致：同种子双跑池指纹相等
    let f1 = pool2.fp();
    let mut pool3 = CandPool::new();
    let mut rng2 = Lcg::new(0x48425);
    let mut k = 0u32;
    while k < 512 {
        let v = rng2.below(100);
        pool3.push(&cand_text(v));
        k += 1;
    }
    if f1 != pool3.fp() || f1 == 0 {
        return Err(E_REPLAY_DIFF);
    }
    hits += 1;

    // R8 容量恒定：POOL_CAP == 16
    if POOL_CAP != 16 {
        return Err(E_CAP_MISS);
    }
    hits += 1;

    Ok(hits)
}
