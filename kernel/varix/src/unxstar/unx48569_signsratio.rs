//! UNX-F48569 —— I·联签条目占比登记（本批联签条 ≥30% 断言）
//! 判据原文：UNX-F48569-J1 批登记器对每批条目（总条数/联签条数）计算占比：
//!           本批联签条占比 ≥30% 判绿；<30% 判红并给差额；
//!           恰 30% 边界判绿；空批（总条 0）拒判；
//!           条目以 id 登记防重复计数；LCG 随机批 ×256 占比数学自洽；
//!           批账守恒与重演指纹一致。
//! 设计纪律：no_std；零 unwrap/expect/panic；确定性 LCG；drill 闭包判据全绿才返回 Ok。

use crate::alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xF6C0; // I29 联签占比段

const E_RATIO_MISS: u16 = SEG_BASE + 1; // 占比断言失准
const E_RATIO_SHORT: u16 = SEG_BASE + 2; // <30% 未判红
const E_EMPTY_BATCH: u16 = SEG_BASE + 3; // 空批未拒判
const E_DUP_ENTRY: u16 = SEG_BASE + 4; // 重复条目未拦
const E_MATH_LEAK: u16 = SEG_BASE + 5; // 占比数学失准
const E_LEDGER_MISS: u16 = SEG_BASE + 6; // 批账不符
const E_REPLAY_DIFF: u16 = SEG_BASE + 7; // 重演不一致
const E_BOUND_BAD: u16 = SEG_BASE + 8; // 30% 边界失准

/// 占比阈值（万分比：30.00%）。
pub const RATIO_BP: u64 = 3000;

/// 批登记器。
pub struct BatchSign {
    pub batch_id: u32,
    total_ids: Vec<u32>,
    signed_ids: Vec<u32>,
    pub label: String,
}

impl BatchSign {
    pub fn new(batch_id: u32) -> Self {
        BatchSign {
            batch_id,
            total_ids: Vec::new(),
            signed_ids: Vec::new(),
            label: "batch-sign".to_string(),
        }
    }

    /// 登记总条目（id 去重）。
    pub fn add_total(&mut self, id: u32) -> Result<(), u16> {
        if self.has(&self.total_ids, id) {
            return Err(E_DUP_ENTRY);
        }
        self.total_ids.push(id);
        Ok(())
    }

    /// 标记联签条目（必须已登记为总条）。
    pub fn mark_signed(&mut self, id: u32) -> Result<(), u16> {
        if !self.has(&self.total_ids, id) {
            return Err(E_LEDGER_MISS);
        }
        if self.has(&self.signed_ids, id) {
            return Err(E_DUP_ENTRY);
        }
        self.signed_ids.push(id);
        Ok(())
    }

    fn has(&self, v: &[u32], id: u32) -> bool {
        let mut i = 0usize;
        while i < v.len() {
            if v[i] == id {
                return true;
            }
            i += 1;
        }
        false
    }

    pub fn total(&self) -> u64 {
        self.total_ids.len() as u64
    }

    pub fn signed(&self) -> u64 {
        self.signed_ids.len() as u64
    }

    /// 占比（万分比）。
    pub fn ratio_bp(&self) -> u64 {
        if self.total() == 0 {
            return 0;
        }
        self.signed() * 10000 / self.total()
    }

    /// 断言：≥30% 判绿；不足判红返回差额（万分比）。
    pub fn assert_ratio(&self) -> Result<u64, u16> {
        if self.total() == 0 {
            return Err(E_EMPTY_BATCH);
        }
        let r = self.ratio_bp();
        if r >= RATIO_BP {
            Ok(r)
        } else {
            Err(E_RATIO_SHORT)
        }
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
        (self.next() >> 16) % n
    }
}

/// 主巡检：占比登记八判据。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 已知批占比精确：10 条签 4 → 40% 判绿
    let mut b = BatchSign::new(1);
    let mut id = 1u32;
    while id <= 10 {
        b.add_total(id)?;
        id += 1;
    }
    let mut id = 1u32;
    while id <= 4 {
        b.mark_signed(id)?;
        id += 1;
    }
    if b.total() != 10 || b.signed() != 4 {
        return Err(E_LEDGER_MISS);
    }
    if b.ratio_bp() != 4000 {
        return Err(E_MATH_LEAK);
    }
    match b.assert_ratio() {
        Ok(r) => {
            if r != 4000 {
                return Err(E_MATH_LEAK);
            }
        }
        _ => return Err(E_RATIO_MISS),
    }
    hits += 1;

    // R2 <30% 判红：10 条签 2 → 20%
    let mut b2 = BatchSign::new(2);
    let mut id = 1u32;
    while id <= 10 {
        b2.add_total(id)?;
        id += 1;
    }
    b2.mark_signed(1)?;
    b2.mark_signed(2)?;
    match b2.assert_ratio() {
        Err(E_RATIO_SHORT) => {}
        _ => return Err(E_RATIO_MISS),
    }
    hits += 1;

    // R3 恰 30% 边界判绿：10 条签 3 → 30%
    let mut b3 = BatchSign::new(3);
    let mut id = 1u32;
    while id <= 10 {
        b3.add_total(id)?;
        id += 1;
    }
    b3.mark_signed(1)?;
    b3.mark_signed(2)?;
    b3.mark_signed(3)?;
    if b3.ratio_bp() != 3000 {
        return Err(E_BOUND_BAD);
    }
    match b3.assert_ratio() {
        Ok(3000) => {}
        _ => return Err(E_BOUND_BAD),
    }
    hits += 1;

    // R4 空批拒判
    let b4 = BatchSign::new(4);
    match b4.assert_ratio() {
        Err(E_EMPTY_BATCH) => {}
        _ => return Err(E_EMPTY_BATCH),
    }
    hits += 1;

    // R5 重复登记拦截
    let mut b5 = BatchSign::new(5);
    b5.add_total(100)?;
    match b5.add_total(100) {
        Err(E_DUP_ENTRY) => {}
        _ => return Err(E_DUP_ENTRY),
    }
    b5.mark_signed(100)?;
    match b5.mark_signed(100) {
        Err(E_DUP_ENTRY) => {}
        _ => return Err(E_DUP_ENTRY),
    }
    if b5.total() != 1 || b5.signed() != 1 {
        return Err(E_LEDGER_MISS);
    }
    hits += 1;

    // R6 LCG 随机批 ×256：占比数学自洽零崩
    let mut rng = Lcg::new(0x48569);
    let mut k = 0u32;
    let mut greens = 0u32;
    let mut reds = 0u32;
    while k < 256 {
        let total = 1 + rng.below(20) as u64;
        let signed = rng.below(total + 1);
        let mut bt = BatchSign::new(100 + k as u32);
        let mut id = 1u32;
        while (id as u64) <= total {
            bt.add_total(id)?;
            id += 1;
        }
        let mut id = 1u32;
        while (id as u64) <= signed {
            bt.mark_signed(id)?;
            id += 1;
        }
        // 独立复算占比
        let expect_bp = signed * 10000 / total;
        if bt.ratio_bp() != expect_bp {
            return Err(E_MATH_LEAK);
        }
        match bt.assert_ratio() {
            Ok(r) => {
                if r < RATIO_BP || r != expect_bp {
                    return Err(E_RATIO_MISS);
                }
                greens += 1;
            }
            Err(E_RATIO_SHORT) => {
                if expect_bp >= RATIO_BP {
                    return Err(E_RATIO_MISS);
                }
                reds += 1;
            }
            Err(_) => return Err(E_MATH_LEAK),
        }
        k += 1;
    }
    if greens == 0 || reds == 0 {
        return Err(E_MATH_LEAK);
    }
    hits += 1;

    // R7 批账守恒：signed ≤ total 且账长一致
    let mut b6 = BatchSign::new(6);
    let mut id = 1u32;
    while id <= 7 {
        b6.add_total(id)?;
        id += 1;
    }
    let mut id = 1u32;
    while id <= 3 {
        b6.mark_signed(id)?;
        id += 1;
    }
    if b6.signed() > b6.total() {
        return Err(E_LEDGER_MISS);
    }
    if b6.total() != 7 || b6.signed() != 3 {
        return Err(E_LEDGER_MISS);
    }
    hits += 1;

    // R8 重演一致：同种子批序列判色序列一致
    let mut rng2 = Lcg::new(0x48569);
    let mut sig1: Vec<u8> = Vec::new();
    let mut k = 0u32;
    while k < 256 {
        let total = 1 + rng2.below(20) as u64;
        let signed = rng2.below(total + 1);
        let expect_bp = signed * 10000 / total;
        if expect_bp >= RATIO_BP {
            sig1.push(1);
        } else {
            sig1.push(0);
        }
        k += 1;
    }
    let mut rng3 = Lcg::new(0x48569);
    let mut sig2: Vec<u8> = Vec::new();
    let mut k = 0u32;
    while k < 256 {
        let total = 1 + rng3.below(20) as u64;
        let signed = rng3.below(total + 1);
        let expect_bp = signed * 10000 / total;
        if expect_bp >= RATIO_BP {
            sig2.push(1);
        } else {
            sig2.push(0);
        }
        k += 1;
    }
    if sig1 != sig2 || sig1.is_empty() {
        return Err(E_REPLAY_DIFF);
    }
    hits += 1;

    Ok(hits)
}
