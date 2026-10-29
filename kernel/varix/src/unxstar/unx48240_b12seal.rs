//! unx48240_b12seal — UNX-F48240 B12 批收官印（20 条冻结/累计 72,000 行）（诊断段 0xBEE0）。
//!
//! 判据原文：
//!   J1 批印入册：域账 72,000/240,000；判据锚定 §AI-61 §7。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 收官印=批账封存：B12 批 20 条的 (卡号, 行数, 冻结指纹) 清单
//!      一次性入册；印面数字（本批 20,000 / 域累计 72,000 / 域总
//!      240,000）三列写死可审计。
//!   2. 冻结语义：印落即账封——再向批内追加条目必须拒绝并显性
//!      （解冻是独立升版动作，不与本印混同）。
//!   3. 域账守恒：批账之和 = 域累计账（72,000），批账 20 条逐条
//!      可核；差一即印面坏账。
//!   4. 指纹封账：20 条卡指纹叠加成批印指纹；复读必须一致。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xBEE0;

const E_BAD_CARD: u16 = SEG_BASE + 1;    // 卡号批外
const E_DUP_CARD: u16 = SEG_BASE + 2;    // 卡号重复
const E_FROZEN_ADD: u16 = SEG_BASE + 3;  // 落印后追加
const E_SUM_MISMATCH: u16 = SEG_BASE + 4; // 域账守恒破
const E_ZERO_ROWS: u16 = SEG_BASE + 5;   // 零行卡
const E_SEAL_MISMATCH: u16 = SEG_BASE + 6; // 印指纹不符
const E_GUARD_UNBAL: u16 = SEG_BASE + 7; // 印账不平

/// 印面三列数字（§AI-61 §7 口径）。
pub const SEAL_BATCH_ROWS: u32 = 20_000;
pub const SEAL_DOMAIN_ROWS: u32 = 72_000;
pub const SEAL_DOMAIN_TOTAL: u32 = 240_000;

/// B12 批卡号段：UNX-F48221..F48240。
pub const B12_LO: u32 = 48221;
pub const B12_HI: u32 = 48240;

/// 一条批账。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BatchRow {
    pub card: u32,
    pub rows: u32,
    pub fp: u32, // 卡指纹（内容指纹简账）
}

/// 收官印总账。
#[derive(Debug)]
pub struct B12Seal {
    rows: Vec<BatchRow>,
    sealed: Option<u32>,
    rejects: u32,
}

impl B12Seal {
    pub fn new() -> Self {
        B12Seal { rows: Vec::new(), sealed: None, rejects: 0 }
    }

    /// 登记批账一条：卡号批内 + 行数>0 + 号唯一。
    pub fn add(&mut self, card: u32, rows: u32, fp: u32) -> Result<(), u16> {
        if self.sealed.is_some() {
            self.rejects += 1;
            return Err(E_FROZEN_ADD);
        }
        if card < B12_LO || card > B12_HI {
            self.rejects += 1;
            return Err(E_BAD_CARD);
        }
        if rows == 0 {
            self.rejects += 1;
            return Err(E_ZERO_ROWS);
        }
        if self.rows.iter().any(|r| r.card == card) {
            self.rejects += 1;
            return Err(E_DUP_CARD);
        }
        self.rows.push(BatchRow { card, rows, fp });
        Ok(())
    }

    /// 落印：批账和=20,000 且 20 条全 → 批印指纹封存。
    pub fn stamp(&mut self) -> Result<u32, u16> {
        if self.rows.len() != (B12_HI - B12_LO + 1) as usize {
            return Err(E_GUARD_UNBAL);
        }
        let sum: u32 = self.rows.iter().map(|r| r.rows).sum();
        if sum != SEAL_BATCH_ROWS {
            return Err(E_SUM_MISMATCH);
        }
        let fp = self.seal_fp();
        self.sealed = Some(fp);
        Ok(fp)
    }

    fn seal_fp(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        for r in &self.rows {
            h ^= r.card;
            h = h.wrapping_mul(0x0100_0193);
            h ^= r.rows;
            h = h.wrapping_mul(0x0100_0193);
            h ^= r.fp;
            h = h.wrapping_mul(0x0100_0193);
        }
        h
    }

    /// 印面复读：指纹一致 + 三列数字在印。
    pub fn verify(&self, fp: u32) -> Result<(), u16> {
        match self.sealed {
            Some(s) if s == fp => {
                let sum: u32 = self.rows.iter().map(|r| r.rows).sum();
                if sum != SEAL_BATCH_ROWS {
                    return Err(E_SUM_MISMATCH);
                }
                Ok(())
            }
            _ => Err(E_SEAL_MISMATCH),
        }
    }

    /// 域账守恒核对：批账和 + 批外已冻结账 = 域累计。
    /// 批外账由调用方传入（B01..B11 + B13..B15 的账面）。
    pub fn domain_check(&self, outside: u32) -> Result<(), u16> {
        let sum: u32 = self.rows.iter().map(|r| r.rows).sum();
        if sum + outside != SEAL_DOMAIN_ROWS {
            return Err(E_SUM_MISMATCH);
        }
        if SEAL_DOMAIN_ROWS > SEAL_DOMAIN_TOTAL {
            return Err(E_GUARD_UNBAL);
        }
        Ok(())
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }
    pub fn sum(&self) -> u32 {
        self.rows.iter().map(|r| r.rows).sum()
    }
    pub fn rejects(&self) -> u32 {
        self.rejects
    }
}

/// 自例程判据：落实 J1「批印入册：域账 72,000/240,000」全链。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 场景：20 条批账（每卡 1,000 行均匀账面）→ 落印 → 复读。
    let mut s = B12Seal::new();
    for i in B12_LO..=B12_HI {
        s.add(i, 1_000, 0xA000 + (i - B12_LO))?;
    }
    if s.len() != 20 || s.sum() != 20_000 {
        return Err(E_GUARD_UNBAL);
    }
    let fp = s.stamp()?;
    s.verify(fp)?;
    hits += 1;

    // R2 场景：落印后追加拒收（冻结语义）。
    match s.add(B12_LO, 1, 1) {
        Err(E_FROZEN_ADD) => {}
        _ => return Err(E_FROZEN_ADD),
    }
    if s.rejects() != 1 {
        return Err(E_GUARD_UNBAL);
    }
    hits += 1;

    // R3 场景：域账守恒 —— 批外 52,000 + 批内 20,000 = 72,000。
    s.domain_check(52_000)?;
    match s.domain_check(52_001) {
        Err(E_SUM_MISMATCH) => {}
        _ => return Err(E_SUM_MISMATCH),
    }
    hits += 1;

    // R4 场景：批外卡号拒收（48220 与 48241）。
    let mut s4 = B12Seal::new();
    match s4.add(48220, 100, 1) {
        Err(E_BAD_CARD) => {}
        _ => return Err(E_BAD_CARD),
    }
    match s4.add(48241, 100, 1) {
        Err(E_BAD_CARD) => {}
        _ => return Err(E_BAD_CARD),
    }
    hits += 1;

    // R5 场景：零行卡拒收。
    let mut s5 = B12Seal::new();
    match s5.add(48221, 0, 1) {
        Err(E_ZERO_ROWS) => {}
        _ => return Err(E_ZERO_ROWS),
    }
    hits += 1;

    // R6 场景：卡号重复拒收。
    let mut s6 = B12Seal::new();
    s6.add(48221, 500, 1)?;
    match s6.add(48221, 600, 2) {
        Err(E_DUP_CARD) => {}
        _ => return Err(E_DUP_CARD),
    }
    hits += 1;

    // R7 场景：条数不足落印拒收（19 条）。
    let mut s7 = B12Seal::new();
    for i in B12_LO..B12_HI {
        s7.add(i, 1_000, i as u32)?;
    }
    match s7.stamp() {
        Err(E_GUARD_UNBAL) => {}
        _ => return Err(E_GUARD_UNBAL),
    }
    hits += 1;

    // R8 场景：批账和≠20,000 落印拒收（坏账显性）。
    let mut s8 = B12Seal::new();
    for i in B12_LO..=B12_HI {
        let rows = if i == B12_HI { 2_000 } else { 1_000 };
        s8.add(i, rows, i)?;
    }
    match s8.stamp() {
        Err(E_SUM_MISMATCH) => {}
        _ => return Err(E_SUM_MISMATCH),
    }
    hits += 1;

    // R9 场景：印指纹敏感 —— 卡指纹变化 → 印面指纹变化。
    let mut s9 = B12Seal::new();
    for i in B12_LO..=B12_HI {
        s9.add(i, 1_000, 0xB000 + (i - B12_LO))?;
    }
    let f9 = s9.stamp()?;
    let mut s10 = B12Seal::new();
    for i in B12_LO..=B12_HI {
        s10.add(i, 1_000, 0xB000 + (i - B12_LO) + 1)?;
    }
    let f10 = s10.stamp()?;
    if f9 == f10 {
        return Err(E_SEAL_MISMATCH);
    }
    match s9.verify(f10) {
        Err(E_SEAL_MISMATCH) => {}
        _ => return Err(E_SEAL_MISMATCH),
    }
    hits += 1;

    // R10 场景：域总印面口径 —— 累计 ≤ 总量、三列数字单调自恰。
    if !(SEAL_BATCH_ROWS == 20_000 && SEAL_DOMAIN_ROWS == 72_000 && SEAL_DOMAIN_TOTAL == 240_000) {
        return Err(E_GUARD_UNBAL);
    }
    if SEAL_DOMAIN_ROWS > SEAL_DOMAIN_TOTAL {
        return Err(E_GUARD_UNBAL);
    }
    hits += 1;

    Ok(hits)
}
