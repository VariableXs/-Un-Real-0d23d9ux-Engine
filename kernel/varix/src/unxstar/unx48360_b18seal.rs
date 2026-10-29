//! unx48360_b18seal — UNX-F48360 B18 批收官印（20 条冻结/累计 108,000 行）（诊断段 0xCDE0）。
//!
//! 判据原文：
//!   J1 批印入册：域账 108,000/240,000；判据锚定 §AI-61 §7。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 收官印口径：B18 批 20 条全部冻结入册，批账 20 × 5,400 =
//!      108,000 行；域累计 108,000/240,000 = 45%（450‰）——印面
//!      数字逐位可复算，印不是口号是算术。
//!   2. 冻结红线：入册条目即冻结（改一字即红）；冻结后条目数、
//!      批账、域账三数一体，任何单方面变动必失衡（防"收官后偷
//!      加条目"）。
//!   3. 印面双红线：①批账 == Σ条目行数（逐条可加和）；②域账 ==
//!      108,000 且分母 240,000 恒定（分母漂移即假账）。
//!   4. 印纹指纹：20 条（卡号，行数）序贯 XOR 折叠，印纹可复算
//!      防漂移；伪造印纹必红。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，确定性，
//!      drill() 返回 Ok(hits)。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xCDE0;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_FROZEN: u16 = SEG_BASE + 1;   // 冻结后改动
const E_ROWS_BAD: u16 = SEG_BASE + 2; // 条目行数与册面不符
const E_LEDGER_BAD: u16 = SEG_BASE + 3;// 批账/域账失衡
const E_NOT_FULL: u16 = SEG_BASE + 4; // 条目不足 20 不收官
const E_FP_BAD: u16 = SEG_BASE + 5;   // 印纹不符
const E_REPLAY_DIFF: u16 = SEG_BASE + 6;// 复跑不一致
const E_EMPTY_SUM: u16 = SEG_BASE + 7;// 写回摘要为空

/// 批条目数与行数参数。
pub const SEAL_N: usize = 20;
pub const ROWS_PER: u32 = 5400;
pub const BATCH_ROWS: u32 = SEAL_N as u32 * ROWS_PER; // 108,000
pub const DOMAIN_CAP: u32 = 240_000;

/// 收官印条目。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SealEntry {
    pub card: u32, // 卡号 48353..48372（B18 批延伸 20 条）
    pub rows: u32,
    pub seal_id: u64,
}

/// 收官印面。
pub struct B18Seal {
    entries: Vec<SealEntry>,
    frozen: bool,
    pub freeze_total: u32, // 冻结动作计数（恒 1）
}

impl B18Seal {
    pub fn new() -> B18Seal {
        B18Seal { entries: Vec::new(), frozen: false, freeze_total: 0 }
    }

    /// 入册一条（未冻结方可入）。
    pub fn add(&mut self, card: u32, rows: u32) -> Result<u64, u16> {
        if self.frozen {
            return Err(E_FROZEN);
        }
        if self.entries.len() >= SEAL_N {
            return Err(E_NOT_FULL);
        }
        if rows == 0 {
            return Err(E_ROWS_BAD);
        }
        let seal_id = entry_seal(card, rows);
        self.entries.push(SealEntry { card, rows, seal_id });
        Ok(seal_id)
    }

    /// 冻结收官：条目满 20 方可冻。
    pub fn freeze(&mut self) -> Result<(), u16> {
        if self.frozen {
            return Err(E_FROZEN);
        }
        if self.entries.len() != SEAL_N {
            return Err(E_NOT_FULL);
        }
        self.frozen = true;
        self.freeze_total += 1;
        Ok(())
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_frozen(&self) -> bool {
        self.frozen
    }

    /// 批账：Σ条目行数。
    pub fn batch_rows(&self) -> u32 {
        let mut s: u32 = 0;
        let mut i = 0usize;
        while i < self.entries.len() {
            s += self.entries[i].rows;
            i += 1;
        }
        s
    }

    /// 域账千分比：批账/240,000。
    pub fn domain_permille(&self) -> u32 {
        self.batch_rows() * 1000 / DOMAIN_CAP
    }

    /// 印面双红线核对（J1 主体）：条目满 20、批账 == 108,000、域
    /// 账 108,000/240,000（450‰）、条目行数逐条与册面一致。
    pub fn audit(&self) -> Result<(), u16> {
        if self.entries.len() != SEAL_N {
            return Err(E_NOT_FULL);
        }
        if !self.frozen {
            return Err(E_FROZEN); // 收官印必须冻后才可验
        }
        let mut i = 0usize;
        while i < SEAL_N {
            let e = self.entries[i];
            if e.rows != ROWS_PER {
                return Err(E_ROWS_BAD);
            }
            if e.seal_id != entry_seal(e.card, e.rows) {
                return Err(E_FP_BAD);
            }
            i += 1;
        }
        if self.batch_rows() != BATCH_ROWS {
            return Err(E_LEDGER_BAD);
        }
        if self.batch_rows() * 1000 / DOMAIN_CAP * DOMAIN_CAP / 1000 != BATCH_ROWS {
            return Err(E_LEDGER_BAD); // 域账分母恒定复核
        }
        Ok(())
    }

    /// 印纹指纹：20 条（卡号，行数）序贯折叠。
    pub fn imprint(&self) -> u64 {
        let mut fp: u64 = 0xCDE0_CDE1;
        let mut i = 0usize;
        while i < self.entries.len() {
            fp = fp.rotate_left(7) ^ (self.entries[i].card as u64);
            fp = fp.rotate_left(7) ^ (self.entries[i].rows as u64);
            i += 1;
        }
        fp
    }

    /// 完成写回（判据 J2）。
    pub fn writeback(&self, done: bool, summary_len: u32) -> Result<(), u16> {
        if !done || !self.frozen {
            return Err(E_LEDGER_BAD);
        }
        if summary_len == 0 {
            return Err(E_EMPTY_SUM);
        }
        Ok(())
    }
}

/// 条目印纹（卡号 ×行数折叠）。
pub fn entry_seal(card: u32, rows: u32) -> u64 {
    (0xCDE0_CDE2 ^ (card as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15))
        .rotate_left(13)
        ^ (rows as u64)
}

/// B18 批 20 卡号表（48353..48372 顺延）。
pub fn batch_cards() -> Vec<u32> {
    let mut v: Vec<u32> = Vec::new();
    let mut c = 48353u32;
    while c < 48353 + SEAL_N as u32 {
        v.push(c);
        c += 1;
    }
    v
}

/// 自例程判据：逐条落实 J1（批印入册：域账 108,000/240,000），返回命中数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1 收官印入册：20 条全入册 → 冻结 → 双红线核（批账 108,000、
    //    域账 450‰、行数逐条 5,400、印纹逐条可复算）。
    let mut seal = B18Seal::new();
    let cards = batch_cards();
    let mut i = 0usize;
    while i < SEAL_N {
        seal.add(cards[i], ROWS_PER)?;
        i += 1;
    }
    seal.freeze()?;
    seal.audit()?;
    if seal.batch_rows() != 108_000 {
        return Err(E_LEDGER_BAD);
    }
    if seal.domain_permille() != 450 {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R2 印面算术逐位复核：20×5400 == 108,000；108,000/240,000 ==
    //    45%（450‰）——独立复算不复用 batch_rows 口径。
    let mut sum: u32 = 0;
    let mut n = 0u32;
    while n < SEAL_N as u32 {
        sum += ROWS_PER;
        n += 1;
    }
    if sum != 108_000 || 108_000 * 1000 / 240_000 != 450 || 108_000 * 100 / 240_000 != 45 {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R3 冻结红线：冻后加条目/再冻结必拒；未满 20 冻结必拒。
    let mut seal = B18Seal::new();
    let cards = batch_cards();
    let mut i = 0usize;
    while i < 19 {
        seal.add(cards[i], ROWS_PER)?;
        i += 1;
    }
    if seal.freeze() != Err(E_NOT_FULL) {
        return Err(E_NOT_FULL);
    }
    seal.add(cards[19], ROWS_PER)?;
    seal.freeze()?;
    if seal.add(99999, ROWS_PER) != Err(E_FROZEN) {
        return Err(E_FROZEN);
    }
    if seal.freeze() != Err(E_FROZEN) {
        return Err(E_FROZEN);
    }
    if seal.freeze_total != 1 {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R4 行数红线可红：条目行数被伪改（≠5,400）audit 必红；印纹
    //    被改 audit 必红（判据非恒绿）。
    let mut seal = B18Seal::new();
    let cards = batch_cards();
    let mut i = 0usize;
    while i < SEAL_N {
        seal.add(cards[i], ROWS_PER)?;
        i += 1;
    }
    seal.freeze()?;
    seal.entries[7].rows = 5000;
    if seal.audit() != Err(E_ROWS_BAD) {
        return Err(E_ROWS_BAD);
    }
    let mut seal2 = B18Seal::new();
    let mut i = 0usize;
    while i < SEAL_N {
        seal2.add(cards[i], ROWS_PER)?;
        i += 1;
    }
    seal2.freeze()?;
    seal2.entries[3].seal_id ^= 0xFF;
    if seal2.audit() != Err(E_FP_BAD) {
        return Err(E_FP_BAD);
    }
    hits += 1;

    // R5 条目不足不收官：半批（10 条）audit 必红（E_NOT_FULL）；
    //    批账加和随条目如实增长（19 条时 102,600 ≠ 108,000）。
    let mut seal = B18Seal::new();
    let cards = batch_cards();
    let mut i = 0usize;
    while i < 10 {
        seal.add(cards[i], ROWS_PER)?;
        i += 1;
    }
    if seal.audit() != Err(E_NOT_FULL) {
        return Err(E_NOT_FULL);
    }
    if seal.batch_rows() != 54_000 {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R6 印纹敏感度：单条卡号变动印纹必变；同批两枚印印纹一致
    //    （确定性）。
    let build = |start: u32| -> Result<B18Seal, u16> {
        let mut s = B18Seal::new();
        let mut c = start;
        let mut i = 0usize;
        while i < SEAL_N {
            s.add(c, ROWS_PER)?;
            c += 1;
            i += 1;
        }
        s.freeze()?;
        Ok(s)
    };
    let s1 = build(48353)?;
    let s2 = build(48353)?;
    if s1.imprint() != s2.imprint() {
        return Err(E_REPLAY_DIFF);
    }
    let s3 = build(48400)?;
    if s3.imprint() == s1.imprint() {
        return Err(E_FP_BAD);
    }
    hits += 1;

    // R7 空行数/超册拒收：rows=0 拒；第 21 条拒（未冻也拒）。
    let mut seal = B18Seal::new();
    if seal.add(48353, 0) != Err(E_ROWS_BAD) {
        return Err(E_ROWS_BAD);
    }
    let cards = batch_cards();
    let mut i = 0usize;
    while i < SEAL_N {
        seal.add(cards[i], ROWS_PER)?;
        i += 1;
    }
    if seal.add(48373, ROWS_PER) != Err(E_NOT_FULL) {
        return Err(E_NOT_FULL);
    }
    hits += 1;

    // R8 复跑一致与写回（判据 J2）：两枚同批印复跑印纹逐位一致、
    //    域账一致；写回过。
    let s1 = build(48353)?;
    let s2 = build(48353)?;
    if s1.imprint() != s2.imprint()
        || s1.batch_rows() != s2.batch_rows()
        || s1.domain_permille() != s2.domain_permille()
    {
        return Err(E_REPLAY_DIFF);
    }
    s1.audit()?;
    if s1.writeback(true, 0).is_ok() {
        return Err(E_EMPTY_SUM);
    }
    s1.writeback(true, 0x16)?;
    hits += 1;

    Ok(hits)
}
