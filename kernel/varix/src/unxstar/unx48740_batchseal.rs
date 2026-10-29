//! UNX-F48740 C37 批收官印（20 条冻结/累计 222,000 行）
//! 判据：UNX-F48740-J1 批印入册；判据锚定 §AI-61 §7。
//! 纪律：20 条冻结判据逐条验印、累计行数守恒、批印冻结不可逆。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0x1900;

const E_SEAL_MISS: u16 = SEG_BASE + 1; // 冻结条缺
const E_SEAL_DUP: u16 = SEG_BASE + 2; // 冻结条重复
const E_LINE_BREAK: u16 = SEG_BASE + 3; // 累计行数守恒破
const E_FROZEN_TOUCH: u16 = SEG_BASE + 4; // 冻结后改
const E_STAMP_BAD: u16 = SEG_BASE + 5; // 批印非法
const E_ENTRY_BAD: u16 = SEG_BASE + 6; // 条目值非法
const E_COUNT_BREAK: u16 = SEG_BASE + 7; // 条数守恒破
const E_HASH_DIFF: u16 = SEG_BASE + 8; // 批印指纹漂移

/// 冻结条数。
pub const SEAL_TOTAL: u32 = 20;
/// 累计行数预算。
pub const LINES_TOTAL: u64 = 222_000;

/// 冻结条。
#[derive(Clone)]
pub struct SealEntry {
    /// 序号（1..=20）。
    pub seq: u32,
    /// 判据锚。
    pub anchor: u32,
    /// 贡献行数。
    pub lines: u64,
}

/// 批收官印。
pub struct BatchSeal {
    entries: Vec<SealEntry>,
    frozen: bool,
    stamp: u32,
}

impl BatchSeal {
    pub fn new() -> BatchSeal {
        BatchSeal {
            entries: Vec::new(),
            frozen: false,
            stamp: 0,
        }
    }

    /// 登记冻结条（序号 1..=20 唯一、锚非零、行数正）。
    pub fn put(&mut self, seq: u32, anchor: u32, lines: u64) -> Result<bool, u16> {
        if seq == 0 || seq > SEAL_TOTAL {
            return Err(E_ENTRY_BAD);
        }
        if anchor == 0 {
            return Err(E_ENTRY_BAD);
        }
        if lines == 0 {
            return Err(E_ENTRY_BAD);
        }
        if self.frozen {
            return Err(E_FROZEN_TOUCH);
        }
        for e in self.entries.iter() {
            if e.seq == seq {
                return Err(E_SEAL_DUP);
            }
        }
        self.entries.push(SealEntry { seq, anchor, lines });
        Ok(true)
    }

    /// FNV-1a 32 位批印指纹（序号+锚+行数联合）。
    fn fingerprint(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        let mut es = self.entries.clone();
        es.sort_by(|a, b| a.seq.cmp(&b.seq));
        for e in es.iter() {
            h ^= e.seq;
            h = h.wrapping_mul(0x0100_0193);
            h ^= e.anchor;
            h = h.wrapping_mul(0x0100_0193);
            h ^= (e.lines & 0xFFFF_FFFF) as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        h
    }

    /// 批印入册：20 条全齐 + 行数守恒 + 冻结。
    pub fn stamp(&mut self) -> Result<u32, u16> {
        if self.entries.len() as u32 != SEAL_TOTAL {
            return Err(E_SEAL_MISS);
        }
        let mut total = 0u64;
        for e in self.entries.iter() {
            total += e.lines;
        }
        if total != LINES_TOTAL {
            return Err(E_LINE_BREAK);
        }
        let fp = self.fingerprint();
        self.frozen = true;
        self.stamp = fp;
        Ok(fp)
    }

    /// 验印：指纹一致。
    pub fn verify(&self, fp: u32) -> Result<bool, u16> {
        if !self.frozen {
            return Err(E_STAMP_BAD);
        }
        Ok(self.stamp == fp)
    }

    /// 冻结态断言。
    pub fn assert_frozen(&self) -> Result<(), u16> {
        if !self.frozen {
            return Err(E_STAMP_BAD);
        }
        Ok(())
    }

    /// 逐条行数对拍：按序号取贡献行数。
    pub fn lookup(&self, seq: u32) -> Result<u64, u16> {
        if !self.frozen {
            return Err(E_STAMP_BAD);
        }
        for e in self.entries.iter() {
            if e.seq == seq {
                return Ok(e.lines);
            }
        }
        Err(E_SEAL_MISS)
    }

    /// 条数守恒。
    pub fn assert_count(&self) -> Result<(), u16> {
        if self.entries.len() as u32 != SEAL_TOTAL {
            return Err(E_COUNT_BREAK);
        }
        Ok(())
    }
}

/// 确定性 LCG。
pub struct Lcg(u64);

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.0
    }
    pub fn below(&mut self, n: u64) -> u64 {
        (self.next() >> 16) % n.max(1)
    }
}

/// 批收官印主判据：20 条冻结 222,000 行批印全绿。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 20 条登记：行数分配 = 基准 + 余数补尾
    let mut bs = BatchSeal::new();
    let base = LINES_TOTAL / SEAL_TOTAL as u64;
    let mut rem = LINES_TOTAL % SEAL_TOTAL as u64;
    let mut s = 1u32;
    while s <= SEAL_TOTAL {
        let mut lines = base;
        if rem > 0 {
            lines += 1;
            rem -= 1;
        }
        bs.put(s, 0x7700 + s, lines)?;
        s += 1;
    }
    bs.assert_count()?;
    hits += 1;

    // R2 重复序号必拒
    if bs.put(1, 0x1, 100).err() == Some(E_SEAL_DUP) {
        hits += 1;
    } else {
        return Err(E_SEAL_DUP);
    }

    // R3 序号越界必拒
    if bs.put(21, 0x1, 100).err() == Some(E_ENTRY_BAD) {
        hits += 1;
    } else {
        return Err(E_ENTRY_BAD);
    }

    // R4 零行必拒
    if bs.put(21, 0x2, 0).err() == Some(E_ENTRY_BAD) {
        hits += 1;
    } else {
        return Err(E_ENTRY_BAD);
    }

    // R5 缺条批印必抓（当前 20 条齐，先测未冻结验印拒）
    if bs.verify(0).err() == Some(E_STAMP_BAD) {
        hits += 1;
    } else {
        return Err(E_STAMP_BAD);
    }

    // R6 批印入册 + 验印全绿
    let fp = bs.stamp()?;
    bs.assert_frozen()?;
    if !bs.verify(fp)? {
        return Err(E_HASH_DIFF);
    }
    hits += 1;

    // R7 验印漂移必抓：错指纹必假
    if bs.verify(fp ^ 1)? {
        return Err(E_HASH_DIFF);
    }
    hits += 1;

    // R8 冻结后登记必拒
    if bs.put(21, 0x3, 100).err() == Some(E_FROZEN_TOUCH) {
        hits += 1;
    } else {
        return Err(E_FROZEN_TOUCH);
    }

    // R9 逐条行数对拍 ×20：全等分配基准
    let mut s2 = 1u32;
    while s2 <= SEAL_TOTAL {
        let ln = bs.lookup(s2)?;
        if ln != base {
            return Err(E_LINE_BREAK);
        }
        s2 += 1;
    }
    hits += 1;

    // R10 LCG 随机分配批印 ×8 轮：指纹确定性重演一致
    let mut rng = Lcg::new(0x48740);
    let mut r = 0u32;
    while r < 8 {
        let seed = rng.below(0xFFFF) as u64 + 1;
        let mut bs2 = BatchSeal::new();
        let mut rng2 = Lcg(seed);
        // 随机拆分 222,000 为 20 份
        let mut parts: Vec<u64> = Vec::new();
        let mut left = LINES_TOTAL;
        let mut i = 0u32;
        while i < SEAL_TOTAL {
            if i == SEAL_TOTAL - 1 {
                parts.push(left);
            } else {
                let take = rng2.below(left / (SEAL_TOTAL as u64 - i as u64) + 1);
                parts.push(take + 1);
                left -= take + 1;
            }
            i += 1;
        }
        let mut j = 0u32;
        while j < SEAL_TOTAL {
            bs2.put(j + 1, 0x8800 + j, parts[j as usize])?;
            j += 1;
        }
        let fp1 = bs2.stamp()?;
        // 独立重演：同种子同拆分同指纹
        let mut bs3 = BatchSeal::new();
        let mut rng3 = Lcg(seed);
        let mut parts2: Vec<u64> = Vec::new();
        let mut left2 = LINES_TOTAL;
        let mut i2 = 0u32;
        while i2 < SEAL_TOTAL {
            if i2 == SEAL_TOTAL - 1 {
                parts2.push(left2);
            } else {
                let take2 = rng3.below(left2 / (SEAL_TOTAL as u64 - i2 as u64) + 1);
                parts2.push(take2 + 1);
                left2 -= take2 + 1;
            }
            i2 += 1;
        }
        let mut j2 = 0u32;
        while j2 < SEAL_TOTAL {
            bs3.put(j2 + 1, 0x8800 + j2, parts2[j2 as usize])?;
            j2 += 1;
        }
        let fp2 = bs3.stamp()?;
        if fp1 != fp2 {
            return Err(E_HASH_DIFF);
        }
        r += 1;
    }
    hits += 1;

    Ok(hits)
}
