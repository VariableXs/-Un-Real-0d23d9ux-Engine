//! UNX-F48460 —— E23 批收官印（20 条冻结/累计 138,000 行）
//! 判据原文：UNX-F48460-J1 E23 批 20 卡清单冻结（卡号连续 48441~48460）；
//!           收官印批指纹串联 FNV 重演一致；累计行数账 = 138,000 闭式校准；
//!           缺卡注入 ×20 槽全检出；印章唯一性（批印≠空印≠单卡印）；
//!           卡指纹两两互异。
//! 设计纪律：no_std；零 unwrap/expect/panic；确定性 LCG；drill 闭包判据全绿才返回 Ok。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xDEA0; // E23 收官印段

pub const BATCH_FIRST: u32 = 48441;
pub const BATCH_LAST: u32 = 48460;
pub const BATCH_ROWS: u32 = 138_000;

const E_CARD_MISS: u16 = SEG_BASE + 1;
const E_SEQ_GAP: u16 = SEG_BASE + 2;
const E_ROWS_MISS: u16 = SEG_BASE + 3;
const E_SEAL_BAD: u16 = SEG_BASE + 4;
const E_UNIQUE_MISS: u16 = SEG_BASE + 5;
const E_TAMPER_MISS: u16 = SEG_BASE + 6;
const E_SHORT_MISS: u16 = SEG_BASE + 7;

/// E23 批 20 卡清单。
pub const BATCH_CARDS: [(u32, u32); 20] = [
    (48441, 349), (48442, 329), (48443, 309), (48444, 309), (48445, 329),
    (48446, 329), (48447, 309), (48448, 309), (48449, 289), (48450, 289),
    (48451, 309), (48452, 329), (48453, 349), (48454, 289), (48455, 289),
    (48456, 269), (48457, 289), (48458, 269), (48459, 229), (48460, 229),
];

fn fnv_mix(h: u64, x: u64) -> u64 {
    let mut acc = h;
    acc ^= x;
    acc.wrapping_mul(0x0000_0100_0000_01b3)
}

/// 单卡指纹。
pub fn card_fp(card: u32, rows: u32) -> u64 {
    fnv_mix(fnv_mix(0xcbf2_9ce4_8422_2325, card as u64), rows as u64)
}

/// 收官印。
pub fn batch_seal(cards: &[(u32, u32)]) -> Result<u64, u16> {
    if cards.len() != 20 {
        return Err(E_CARD_MISS);
    }
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut i = 0usize;
    while i < cards.len() {
        h = fnv_mix(h, cards[i].0 as u64);
        h = fnv_mix(h, cards[i].1 as u64);
        i += 1;
    }
    Ok(h)
}

/// 清单健康审计。
pub fn audit_cards(cards: &[(u32, u32)]) -> Result<u32, u16> {
    if cards.len() != 20 {
        return Err(E_CARD_MISS);
    }
    let mut rows_sum = 0u32;
    let mut i = 0usize;
    while i < cards.len() {
        if i > 0 && cards[i].0 != cards[i - 1].0 + 1 {
            return Err(E_SEQ_GAP);
        }
        if cards[i].1 == 0 {
            return Err(E_ROWS_MISS);
        }
        rows_sum += cards[i].1;
        i += 1;
    }
    Ok(rows_sum)
}

/// 缺卡注入检出。
pub fn inject_missing(cards: &[(u32, u32)], k: usize) -> Result<bool, u16> {
    if k >= cards.len() {
        return Err(E_CARD_MISS);
    }
    let mut cut: Vec<(u32, u32)> = Vec::new();
    let mut i = 0usize;
    while i < cards.len() {
        if i != k {
            cut.push(cards[i]);
        }
        i += 1;
    }
    let full = batch_seal(cards)?;
    let bad = batch_seal(&cut)?;
    Ok(full != bad)
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

/// 主巡检：收官印八判据。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 清单健康
    let sum = audit_cards(&BATCH_CARDS)?;
    hits += 1;

    // R2 行数账闭式 = 138,000
    if sum != BATCH_ROWS {
        return Err(E_ROWS_MISS);
    }
    hits += 1;

    // R3 首尾边界
    if BATCH_CARDS[0].0 != BATCH_FIRST || BATCH_CARDS[19].0 != BATCH_LAST {
        return Err(E_SEQ_GAP);
    }
    hits += 1;

    // R4 印重演一致非零
    let s1 = batch_seal(&BATCH_CARDS)?;
    let s2 = batch_seal(&BATCH_CARDS)?;
    if s1 != s2 || s1 == 0 {
        return Err(E_SEAL_BAD);
    }
    hits += 1;

    // R5 缺卡注入 ×20 全检出
    let mut rng = Lcg::new(0x48460);
    let mut k = 0usize;
    while k < 20 {
        let hit = inject_missing(&BATCH_CARDS, k)?;
        if !hit {
            return Err(E_TAMPER_MISS);
        }
        k += 1;
    }
    let _ = rng.below(1);
    hits += 1;

    // R6 印唯一性：≠空印 ≠单卡印
    let seal = batch_seal(&BATCH_CARDS)?;
    if seal == 0xcbf2_9ce4_8422_2325 {
        return Err(E_UNIQUE_MISS);
    }
    let mut i = 0usize;
    while i < 20 {
        if seal == card_fp(BATCH_CARDS[i].0, BATCH_CARDS[i].1) {
            return Err(E_UNIQUE_MISS);
        }
        i += 1;
    }
    hits += 1;

    // R7 卡指纹互异
    let mut i = 0usize;
    while i < 20 {
        let mut j = i + 1;
        while j < 20 {
            if card_fp(BATCH_CARDS[i].0, BATCH_CARDS[i].1)
                == card_fp(BATCH_CARDS[j].0, BATCH_CARDS[j].1)
            {
                return Err(E_UNIQUE_MISS);
            }
            j += 1;
        }
        i += 1;
    }
    hits += 1;

    // R8 短册拒：19 卡清单报 CARD_MISS
    let mut short: Vec<(u32, u32)> = Vec::new();
    let mut i = 0usize;
    while i < 19 {
        short.push(BATCH_CARDS[i]);
        i += 1;
    }
    match audit_cards(&short) {
        Err(E_CARD_MISS) => {}
        _ => return Err(E_SHORT_MISS),
    }
    hits += 1;

    Ok(hits)
}
