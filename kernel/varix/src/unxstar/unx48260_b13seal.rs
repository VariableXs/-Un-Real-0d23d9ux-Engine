//! unx48260_b13seal — UNX-F48260 B13 批收官印（20 条冻结/累计 78,000 行）（诊断段 0xC160）。
//!
//! 判据原文：
//!   J1 批印入册：域账 78,000/240,000；判据锚定 §AI-61 §7。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 收官印定义：批内 20 条交付单元（卡）逐条冻结落账——条目带
//!      （卡号序、行数、冻结位），20 条齐且全部冻结才可盖印。
//!   2. 印面口径：批累计行数（20 条行数合计）与域账分母（240,000）
//!      构成印面分数 78,000/240,000；印面数字以卡内验收原文为准，
//!      盖印时逐位复核，不符即拒印。
//!   3. 印指纹：印面内容（20 条目 + 印面分数）XOR 折叠出批印指纹；
//!      同印面复算同指纹，任一条目变动指纹必变（防偷改留痕）。
//!   4. 封印不可逆：盖印后条目区只读——增删改条目、重盖印一律拒收
//!      并留拒收痕；解封不在本面（封印即终态）。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xC160;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_SEAL_INCOMPLETE: u16 = SEG_BASE + 1;// 条目不齐或未全冻结
const E_FACE_MISMATCH: u16 = SEG_BASE + 2;  // 印面数字与验收原文不符
const E_SEALED_EDIT: u16 = SEG_BASE + 3;    // 封印后改动被拒
const E_FINGER_DRIFT: u16 = SEG_BASE + 4;   // 指纹复算不符
const E_ENTRY_BAD: u16 = SEG_BASE + 5;      // 条目越界/重复
const E_LEDGER_BAD: u16 = SEG_BASE + 6;     // 印账不平
const E_EMPTY_SUM: u16 = SEG_BASE + 7;      // 写回摘要为空

/// 批内单元数（20 条冻结）。
pub const ENTRY_COUNT: usize = 20;
/// 印面分子：B13 批累计行数（验收原文口径）。
pub const FACE_NUMERATOR: u32 = 78_000;
/// 印面分母：域账总量（验收原文口径）。
pub const FACE_DENOMINATOR: u32 = 240_000;

/// 一条冻结条目。
#[derive(Clone, Copy, Debug)]
pub struct SealEntry {
    pub seq: u32,  // 卡号序 0..19
    pub rows: u32, // 该条行数
    pub frozen: bool,
}

/// 收官印枢纽。
pub struct B13Seal {
    entries: Vec<SealEntry>,
    pub sealed: bool,
    pub fingerprint: u32,
    pub reject_total: u32,
    pub seal_round: u32,
}

impl B13Seal {
    pub fn new() -> B13Seal {
        B13Seal {
            entries: Vec::new(),
            sealed: false,
            fingerprint: 0,
            reject_total: 0,
            seal_round: 0,
        }
    }

    /// 登记条目（收印后拒收）：行数须 >0，序号须 =当前长度（连续编发）。
    pub fn add_entry(&mut self, rows: u32) -> Result<u32, u16> {
        if self.sealed {
            self.reject_total += 1;
            return Err(E_SEALED_EDIT);
        }
        if rows == 0 {
            return Err(E_ENTRY_BAD);
        }
        if self.entries.len() >= ENTRY_COUNT {
            return Err(E_ENTRY_BAD); // 超 20 条拒
        }
        let seq = self.entries.len() as u32;
        self.entries.push(SealEntry { seq, rows, frozen: true });
        Ok(seq)
    }

    /// 印面分数复核：20 条行数合计 == 印面分子。
    pub fn check_face(&self) -> Result<u32, u16> {
        if self.entries.len() != ENTRY_COUNT {
            return Err(E_SEAL_INCOMPLETE);
        }
        let mut sum: u64 = 0;
        let mut i = 0usize;
        while i < self.entries.len() {
            if !self.entries[i].frozen {
                return Err(E_SEAL_INCOMPLETE);
            }
            sum += self.entries[i].rows as u64;
            i += 1;
        }
        if sum != FACE_NUMERATOR as u64 {
            return Err(E_FACE_MISMATCH);
        }
        if FACE_NUMERATOR > FACE_DENOMINATOR {
            return Err(E_FACE_MISMATCH);
        }
        Ok(FACE_NUMERATOR)
    }

    /// 印指纹：条目序+行数+印面分数 XOR 折叠。
    pub fn compute_fingerprint(&self) -> u32 {
        let mut fp: u32 = 0xC161;
        let mut i = 0usize;
        while i < self.entries.len() {
            fp = fp
                .wrapping_mul(0x9E3779B1)
                ^ self.entries[i].seq
                ^ self.entries[i].rows
                ^ (i as u32);
            i += 1;
        }
        fp ^= FACE_NUMERATOR ^ FACE_DENOMINATOR;
        fp
    }

    /// 盖印：20 条齐且全冻结 + 印面复核通过 → 封印。
    pub fn seal(&mut self) -> Result<u32, u16> {
        if self.sealed {
            self.reject_total += 1;
            return Err(E_SEALED_EDIT);
        }
        let face = self.check_face()?;
        let fp = self.compute_fingerprint();
        self.sealed = true;
        self.fingerprint = fp;
        self.seal_round += 1;
        Ok(face)
    }

    /// 封印复核：指纹重算与登记一致、条目数不变。
    pub fn verify(&self) -> Result<(), u16> {
        if !self.sealed {
            return Err(E_LEDGER_BAD);
        }
        if self.entries.len() != ENTRY_COUNT {
            return Err(E_SEALED_EDIT);
        }
        if self.compute_fingerprint() != self.fingerprint {
            return Err(E_FINGER_DRIFT);
        }
        Ok(())
    }

    /// ×N 条目覆盖核验。
    pub fn coverage(&self, want: usize) -> Result<(), u16> {
        if self.entries.len() < want {
            return Err(E_SEAL_INCOMPLETE);
        }
        Ok(())
    }

    pub fn entries_len(&self) -> usize {
        self.entries.len()
    }

    /// 完成写回（判据 J2）。
    pub fn writeback(&self, done: bool, summary_len: u32) -> Result<(), u16> {
        if !done || !self.sealed {
            return Err(E_LEDGER_BAD);
        }
        if summary_len == 0 {
            return Err(E_EMPTY_SUM);
        }
        Ok(())
    }
}

/// 收官印行数分配样张：20 条行数合计恰 78,000 的确定性分配。
/// 分配式：每条 3,900 行（78,000/20）。
pub const ENTRY_ROWS: u32 = 3_900;

/// 自例程判据：逐条落实 J1（批印入册：域账 78,000/240,000），返回命中数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1 主盖印：20 条逐条登记（每条 3,900 行，合计 78,000）→
    //    印面复核 → 盖印 → 指纹封存 → 复核全绿。
    let mut seal = B13Seal::new();
    let mut i = 0u32;
    while i < ENTRY_COUNT as u32 {
        let seq = seal.add_entry(ENTRY_ROWS)?;
        if seq != i {
            return Err(E_ENTRY_BAD);
        }
        i += 1;
    }
    seal.coverage(20)?;
    let face = seal.seal()?;
    if face != FACE_NUMERATOR {
        return Err(E_FACE_MISMATCH);
    }
    seal.verify()?;
    if !seal.sealed || seal.seal_round != 1 {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R2 印面分数：78,000/240,000 逐位核对（验收原文口径），分子分母
    //    关系合法（分子 ≤ 分母）。
    if FACE_NUMERATOR != 78_000 || FACE_DENOMINATOR != 240_000 {
        return Err(E_FACE_MISMATCH);
    }
    if FACE_NUMERATOR * 3 != 234_000 || FACE_DENOMINATOR - FACE_NUMERATOR != 162_000 {
        return Err(E_FACE_MISMATCH);
    }
    hits += 1;

    // R3 缺条拒印：19 条即盖印必拒（SEAL_INCOMPLETE），拒后补齐可印。
    let mut seal = B13Seal::new();
    let mut i = 0u32;
    while i < (ENTRY_COUNT - 1) as u32 {
        seal.add_entry(ENTRY_ROWS)?;
        i += 1;
    }
    if seal.seal().is_ok() {
        return Err(E_SEAL_INCOMPLETE);
    }
    if seal.sealed {
        return Err(E_LEDGER_BAD);
    }
    seal.add_entry(ENTRY_ROWS)?;
    seal.seal()?;
    seal.verify()?;
    hits += 1;

    // R4 印面失配拒印：20 条但合计 ≠78,000（掺一条 3,901）必拒
    //    （印面数字以卡内验收原文为准的红线）。
    let mut seal = B13Seal::new();
    let mut i = 0u32;
    while i < (ENTRY_COUNT - 1) as u32 {
        seal.add_entry(ENTRY_ROWS)?;
        i += 1;
    }
    seal.add_entry(ENTRY_ROWS + 1)?; // 合计 78,001
    if seal.seal().is_ok() {
        return Err(E_FACE_MISMATCH);
    }
    if seal.sealed {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R5 封印不可逆：盖印后再加条、再盖印全拒且拒收留痕逐次计数。
    let mut seal = B13Seal::new();
    let mut i = 0u32;
    while i < ENTRY_COUNT as u32 {
        seal.add_entry(ENTRY_ROWS)?;
        i += 1;
    }
    seal.seal()?;
    if seal.add_entry(ENTRY_ROWS).is_ok() {
        return Err(E_SEALED_EDIT);
    }
    if seal.seal().is_ok() {
        return Err(E_SEALED_EDIT);
    }
    if seal.reject_total != 2 {
        return Err(E_LEDGER_BAD);
    }
    seal.verify()?;
    hits += 1;

    // R6 指纹敏感性：同印面复算同指纹；篡改一条行数指纹必变
    //    （防偷改留痕的红线）。
    let mut seal_a = B13Seal::new();
    let mut seal_b = B13Seal::new();
    let mut i = 0u32;
    while i < ENTRY_COUNT as u32 {
        seal_a.add_entry(ENTRY_ROWS)?;
        seal_b.add_entry(ENTRY_ROWS)?;
        i += 1;
    }
    let fa = seal_a.compute_fingerprint();
    let fb = seal_b.compute_fingerprint();
    if fa != fb {
        return Err(E_FINGER_DRIFT);
    }
    seal_b.entries[19].rows = ENTRY_ROWS + 1; // 篡改末条
    let fb2 = seal_b.compute_fingerprint();
    if fb2 == fa {
        return Err(E_FINGER_DRIFT);
    }
    hits += 1;

    // R7 指纹漂移抓拍：伪造登记指纹与重算不符，verify 必红
    // （可红判据，非恒绿）。
    let mut seal = B13Seal::new();
    let mut i = 0u32;
    while i < ENTRY_COUNT as u32 {
        seal.add_entry(ENTRY_ROWS)?;
        i += 1;
    }
    seal.seal()?;
    seal.fingerprint ^= 0x5EED;
    if seal.verify().is_ok() {
        return Err(E_FINGER_DRIFT);
    }
    hits += 1;

    // R8 边界与写回（判据 J2）：零行数条目拒、第 21 条拒、空摘要与
    //    未封印写回拒，正常批写回放行。
    let mut seal = B13Seal::new();
    if seal.add_entry(0).is_ok() {
        return Err(E_ENTRY_BAD);
    }
    let mut i = 0u32;
    while i < ENTRY_COUNT as u32 {
        seal.add_entry(ENTRY_ROWS)?;
        i += 1;
    }
    if seal.add_entry(ENTRY_ROWS).is_ok() {
        return Err(E_ENTRY_BAD); // 第 21 条
    }
    seal.seal()?;
    if seal.writeback(true, 0).is_ok() {
        return Err(E_EMPTY_SUM);
    }
    seal.writeback(true, 0x40)?;
    hits += 1;

    Ok(hits)
}
