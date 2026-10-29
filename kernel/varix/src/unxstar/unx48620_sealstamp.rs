//! UNX-F48620 —— I31 批收官印（20 条冻结/累计 186,000 行）
//! 判据原文：UNX-F48620-J1 批印入册：域账 186,000/240,000；
//!           判据锚定 §AI-61 §7。
//! 设计纪律：no_std（仅 core/alloc）；零 unwrap/expect/panic；
//!           批印冻结表 + 域账守恒机检。

use crate::alloc::vec;
use crate::alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0x04A0; // I31 批收官印诊断段

const E_FROZEN_TOUCH: u16 = SEG_BASE + 1;
const E_COUNT_BAD: u16 = SEG_BASE + 2;
const E_TOTAL_BAD: u16 = SEG_BASE + 3;
const E_STAMP_MISS: u16 = SEG_BASE + 4;
const E_DUP: u16 = SEG_BASE + 5;
const E_LEDGER: u16 = SEG_BASE + 6;
const E_REPLAY: u16 = SEG_BASE + 7;

/// 域账锚点（冻结）。
pub const DOMAIN_TOTAL: u64 = 240_000;
pub const BATCH_CUM: u64 = 186_000;
pub const BATCH_COUNT: u32 = 20;

/// 批印条目。
pub struct StampItem {
    pub card: u32,
    pub rows: u32,
    pub frozen: bool,
}

/// 批收官印。
pub struct BatchSeal {
    pub items: Vec<StampItem>,
    pub sealed: bool,
    pub label: String,
}

impl BatchSeal {
    pub fn new(label: &str) -> Self {
        BatchSeal { items: Vec::new(), sealed: false, label: label.to_string() }
    }

    pub fn freeze_card(&mut self, card: u32, rows: u32) -> Result<(), u16> {
        if self.sealed {
            return Err(E_FROZEN_TOUCH);
        }
        let mut i = 0usize;
        while i < self.items.len() {
            if self.items[i].card == card {
                return Err(E_DUP);
            }
            i += 1;
        }
        self.items.push(StampItem { card, rows, frozen: true });
        Ok(())
    }

    pub fn seal(&mut self) {
        self.sealed = true;
        let mut i = 0usize;
        while i < self.items.len() {
            self.items[i].frozen = true;
            i += 1;
        }
    }

    pub fn check_count(&self) -> Result<(), u16> {
        if self.items.len() as u32 != BATCH_COUNT {
            return Err(E_COUNT_BAD);
        }
        Ok(())
    }

    pub fn check_total(&self, prev_cum: u64) -> Result<(), u16> {
        let mut sum: u64 = 0;
        let mut i = 0usize;
        while i < self.items.len() {
            sum += self.items[i].rows as u64;
            i += 1;
        }
        let cum = prev_cum.wrapping_add(sum);
        if cum != BATCH_CUM {
            return Err(E_TOTAL_BAD);
        }
        if cum > DOMAIN_TOTAL {
            return Err(E_LEDGER);
        }
        Ok(())
    }

    pub fn audit(&self) -> Result<(), u16> {
        if !self.sealed {
            return Err(E_STAMP_MISS);
        }
        let mut i = 0usize;
        while i < self.items.len() {
            if !self.items[i].frozen {
                return Err(E_FROZEN_TOUCH);
            }
            i += 1;
        }
        Ok(())
    }

    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        h ^= self.sealed as u64;
        let mut i = 0usize;
        while i < self.items.len() {
            h = h.wrapping_mul(0x0000_0100_0000_01b3) ^ self.items[i].card as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3) ^ self.items[i].rows as u64;
            i += 1;
        }
        h
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
        if n == 0 {
            return 0;
        }
        (self.next() >> 16) % n
    }
}

/// 主巡检：I31 批收官印十判据。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 二十条冻结入账：本批行数合计 = 186,000 - 180,000 = 6,000。
    let prev_cum: u64 = 180_000;
    let mut seal = BatchSeal::new("batch-i31");
    let mut rng = Lcg::new(0x48620_0001);
    let mut allocated: u32 = 0;
    let mut c = 0u32;
    while c < BATCH_COUNT {
        let card = 48591 + c;
        let rows = if c == BATCH_COUNT - 1 {
            6_000 - allocated
        } else {
            let r = 200 + rng.below(301) as u32;
            allocated += r;
            r
        };
        seal.freeze_card(card, rows)?;
        c += 1;
    }
    seal.check_count()?;
    hits += 1;

    // R2 域账守恒。
    seal.check_total(prev_cum)?;
    hits += 1;

    // R3 封印审计。
    seal.seal();
    seal.audit()?;
    hits += 1;

    // R4 冻结后写反例。
    let r = seal.freeze_card(99999, 100);
    if r.err() != Some(E_FROZEN_TOUCH) {
        return Err(E_FROZEN_TOUCH);
    }
    hits += 1;

    // R5 未封印审计反例。
    let mut seal2 = BatchSeal::new("unsealed");
    seal2.freeze_card(1, 100)?;
    let r = seal2.audit();
    if r.err() != Some(E_STAMP_MISS) {
        return Err(E_STAMP_MISS);
    }
    hits += 1;

    // R6 卡号重复反例。
    let mut seal3 = BatchSeal::new("dup");
    seal3.freeze_card(7, 100)?;
    let r = seal3.freeze_card(7, 200);
    if r.err() != Some(E_DUP) {
        return Err(E_DUP);
    }
    hits += 1;

    // R7 条数不符反例。
    let mut seal4 = BatchSeal::new("short");
    let mut c = 0u32;
    while c < 9 {
        seal4.freeze_card(100 + c, 50)?;
        c += 1;
    }
    seal4.seal();
    let r = seal4.check_count();
    if r.err() != Some(E_COUNT_BAD) {
        return Err(E_COUNT_BAD);
    }
    hits += 1;

    // R8 累计超容反例。
    let mut seal5 = BatchSeal::new("over");
    seal5.freeze_card(1, 70_000)?;
    seal5.seal();
    let r = seal5.check_total(186_000);
    if r.err() != Some(E_LEDGER) {
        return Err(E_LEDGER);
    }
    hits += 1;

    // R9 累计失配反例。
    let mut seal6 = BatchSeal::new("mismatch");
    seal6.freeze_card(1, 5_000)?;
    seal6.freeze_card(2, 1_001)?;
    seal6.seal();
    let r = seal6.check_total(prev_cum);
    if r.err() != Some(E_TOTAL_BAD) {
        return Err(E_TOTAL_BAD);
    }
    hits += 1;

    // R10 重演一致。
    let fp1 = seal.fingerprint();
    let mut seal7 = BatchSeal::new("batch-i31");
    let mut rng2 = Lcg::new(0x48620_0001);
    let mut allocated2: u32 = 0;
    let mut c = 0u32;
    while c < BATCH_COUNT {
        let card = 48591 + c;
        let rows = if c == BATCH_COUNT - 1 {
            6_000 - allocated2
        } else {
            let r2 = 200 + rng2.below(301) as u32;
            allocated2 += r2;
            r2
        };
        let _ = seal7.freeze_card(card, rows);
        c += 1;
    }
    seal7.seal();
    if seal7.fingerprint() != fp1 || fp1 == 0 {
        return Err(E_REPLAY);
    }
    hits += 1;

    Ok(hits)
}
