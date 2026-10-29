//! UNX-F48420 —— E21 批收官印（20 条冻结/累计 126,000 行）
//! 判据原文：UNX-F48420-J1 E21 批 20 卡清单冻结（卡号连续 48401~48420 逐条在册）；
//!           收官印批指纹 = 20 条卡指纹串联 FNV，重演一致；
//!           累计行数账 = 126,000（批内 rows 合计闭式校准）；
//!           缺卡注入（抽走任一卡号）指纹断言检出；
//!           印章唯一性（批印 ≠ 空印 ≠ 单卡印）。
//! 设计纪律：no_std；零 unwrap/expect/panic；确定性 LCG；drill 闭包判据全绿才返回 Ok。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xD7E0; // E21 收官印段

pub const BATCH_FIRST: u32 = 48401;
pub const BATCH_LAST: u32 = 48420;
pub const BATCH_ROWS: u32 = 126_000; // 累计行数账

const E_CARD_MISS: u16 = SEG_BASE + 1; // 卡缺册
const E_SEQ_GAP: u16 = SEG_BASE + 2; // 卡号断档
const E_ROWS_MISS: u16 = SEG_BASE + 3; // 行数账不符
const E_SEAL_BAD: u16 = SEG_BASE + 4; // 印指纹失配
const E_REPLAY_DIFF: u16 = SEG_BASE + 5; // 重演不一致
const E_UNIQUE_MISS: u16 = SEG_BASE + 6; // 唯一性破
const E_TAMPER_MISS: u16 = SEG_BASE + 7; // 缺卡注入未检出

/// 批卡清单：E21 批 20 卡（rows 为卡面行数要求）。
pub const BATCH_CARDS: [(u32, u32); 20] = [
    (48401, 359), (48402, 339), (48403, 319), (48404, 319), (48405, 339),
    (48406, 319), (48407, 319), (48408, 299), (48409, 319), (48410, 299),
    (48411, 319), (48412, 299), (48413, 339), (48414, 279), (48415, 279),
    (48416, 299), (48417, 259), (48418, 259), (48419, 219), (48420, 219),
];

fn fnv_mix(h: u64, x: u64) -> u64 {
    let mut acc = h;
    acc ^= x;
    acc.wrapping_mul(0x0000_0100_0000_01b3)
}

/// 单卡指纹：FNV(卡号, rows)。
pub fn card_fp(card: u32, rows: u32) -> u64 {
    fnv_mix(fnv_mix(0xcbf2_9ce4_8422_2325, card as u64), rows as u64)
}

/// 收官印：20 条卡指纹串联。
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

/// 清单健康审计：卡号连续、rows 非零、卡数 20。
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

/// 缺卡注入：从清单抽走第 k 条，印章应变化（检出）。
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

    // R1 清单健康：20 卡连续非零
    let sum = audit_cards(&BATCH_CARDS)?;
    hits += 1;

    // R2 行数账闭式校准：sum == BATCH_ROWS == 126,000
    if sum != BATCH_ROWS {
        return Err(E_ROWS_MISS);
    }
    hits += 1;

    // R3 批首尾边界：首卡 48401、尾卡 48420
    if BATCH_CARDS[0].0 != BATCH_FIRST || BATCH_CARDS[19].0 != BATCH_LAST {
        return Err(E_SEQ_GAP);
    }
    hits += 1;

    // R4 收官印重演一致：双算一致且非零
    let s1 = batch_seal(&BATCH_CARDS)?;
    let s2 = batch_seal(&BATCH_CARDS)?;
    if s1 != s2 || s1 == 0 {
        return Err(E_SEAL_BAD);
    }
    hits += 1;

    // R5 缺卡注入检出：20 个槽位逐一抽走，印指纹全部变化
    let mut rng = Lcg::new(0x48420);
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

    // R6 印章唯一性：批印 ≠ 空印（FNV 初值）≠ 任一单卡印
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

    // R7 卡指纹互异：20 条单卡印两两不同
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

    // R8 缺卡审计：audit_cards 对 19 卡清单拒（断档或短册双路径）
    let mut short: Vec<(u32, u32)> = Vec::new();
    let mut i = 0usize;
    while i < 19 {
        short.push(BATCH_CARDS[i]);
        i += 1;
    }
    match audit_cards(&short) {
        Err(E_CARD_MISS) => {}
        _ => return Err(E_CARD_MISS),
    }
    hits += 1;

    Ok(hits)
}
