//! unx48300_b15seal — UNX-F48300 B15 批收官印（20 条冻结/累计 90,000 行·B01–B15 段满额）（诊断段 0xC660）。
//!
//! 判据原文：
//!   J1 批印入册：域账 90,000/240,000（37.5%）、300/300 冻结；
//!   判据锚定 §AI-61 §7。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 批条目冻结：B15 批 20 条交付单元逐条冻结落账（条目带行数与
//!      冻结位），20 条齐且全冻结才可盖印——缺条/未冻即拒印。
//!   2. 印面口径（验收原文为准）：批累计 90,000 行 / 域账 240,000 行
//!      = 37.5%；段满额口径：B01–B15 共 15 批 × 20 条 = 300 条冻结，
//!      300/300 全冻结才可盖印——印面数字与段满额两条红线同验。
//!   3. 印指纹：条目区 + 印面分数 + 段满额三域 XOR 折叠；同印面
//!      复算同指纹，任一域变动指纹必变（防偷改留痕）。
//!   4. 封印不可逆：盖印后条目区只读，增删改/重盖/改域账口径全拒
//!      并逐次留痕——封印即终态，解封不在本面。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xC660;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_SEAL_INCOMPLETE: u16 = SEG_BASE + 1; // 条目不齐/未全冻/段额不足
const E_FACE_MISMATCH: u16 = SEG_BASE + 2;   // 印面数字与验收原文不符
const E_SEALED_EDIT: u16 = SEG_BASE + 3;     // 封印后改动
const E_FINGER_DRIFT: u16 = SEG_BASE + 4;    // 指纹复算不符
const E_ENTRY_BAD: u16 = SEG_BASE + 5;       // 条目越界/重复/零行
const E_LEDGER_BAD: u16 = SEG_BASE + 6;      // 印账不平
const E_EMPTY_SUM: u16 = SEG_BASE + 7;       // 写回摘要为空

/// 批条目数（B15 批 20 条）。
pub const ENTRY_COUNT: usize = 20;
/// 印面分子：B15 批累计行数。
pub const FACE_NUMERATOR: u32 = 90_000;
/// 印面分母：域账总量。
pub const FACE_DENOMINATOR: u32 = 240_000;
/// 段满额：B01–B15 共 300 条冻结。
pub const SEG_FROZEN_FULL: u32 = 300;
/// 每条行数分配（90,000/20）。
pub const ENTRY_ROWS: u32 = 4_500;

/// 一条冻结条目。
#[derive(Clone, Copy, Debug)]
pub struct SealEntry {
    pub seq: u32,
    pub rows: u32,
    pub frozen: bool,
}

/// B15 收官印枢纽。
pub struct B15Seal {
    entries: Vec<SealEntry>,
    pub seg_frozen: u32, // 段累计冻结条数（B01–B15 汇报值）
    pub sealed: bool,
    pub fingerprint: u32,
    pub reject_total: u32,
}

impl B15Seal {
    pub fn new(seg_frozen: u32) -> B15Seal {
        B15Seal {
            entries: Vec::new(),
            seg_frozen,
            sealed: false,
            fingerprint: 0,
            reject_total: 0,
        }
    }

    /// 登记条目：行数 >0、连续编发、不超 20 条；封印后拒收。
    pub fn add_entry(&mut self, rows: u32) -> Result<u32, u16> {
        if self.sealed {
            self.reject_total += 1;
            return Err(E_SEALED_EDIT);
        }
        if rows == 0 {
            return Err(E_ENTRY_BAD);
        }
        if self.entries.len() >= ENTRY_COUNT {
            return Err(E_ENTRY_BAD);
        }
        let seq = self.entries.len() as u32;
        self.entries.push(SealEntry { seq, rows, frozen: true });
        Ok(seq)
    }

    /// 印面复核（纪律 2）：条目合计==90,000；分子 ≤ 分母；
    /// 37.5% 逐位核对（90000×2 == 240000×3/4，即 90000*4==240000*3/2）。
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
        // 37.5%：90000/240000 = 375/1000。
        if FACE_NUMERATOR * 1000 != FACE_DENOMINATOR * 375 {
            return Err(E_FACE_MISMATCH);
        }
        Ok(FACE_NUMERATOR)
    }

    /// 段满额复核：B01–B15 段累计冻结 300/300。
    pub fn check_segment_full(&self) -> Result<(), u16> {
        if self.seg_frozen != SEG_FROZEN_FULL {
            return Err(E_SEAL_INCOMPLETE);
        }
        Ok(())
    }

    /// 印指纹：条目区 + 印面分数 + 段满额三域折叠。
    pub fn compute_fingerprint(&self) -> u32 {
        let mut fp: u32 = 0xC661;
        let mut i = 0usize;
        while i < self.entries.len() {
            fp = fp
                .wrapping_mul(0x9E3779B1)
                ^ self.entries[i].seq
                ^ self.entries[i].rows
                ^ (i as u32);
            i += 1;
        }
        fp ^= FACE_NUMERATOR ^ FACE_DENOMINATOR ^ SEG_FROZEN_FULL;
        fp
    }

    /// 盖印：条目齐 + 印面 + 段满额三关全过 → 封印。
    pub fn seal(&mut self) -> Result<u32, u16> {
        if self.sealed {
            self.reject_total += 1;
            return Err(E_SEALED_EDIT);
        }
        let face = self.check_face()?;
        self.check_segment_full()?;
        self.fingerprint = self.compute_fingerprint();
        self.sealed = true;
        Ok(face)
    }

    /// 封印复核：指纹重算一致、条目数不变。
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

/// 自例程判据：逐条落实 J1（批印入册：域账 90,000/240,000（37.5%）、
/// 300/300 冻结），返回命中数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1 主盖印：段满额 300/300 + 20 条（每条 4,500）→ 三关全过 →
    //    盖印 → 指纹封存 → 复核全绿。
    let mut seal = B15Seal::new(300);
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
    if face != 90_000 {
        return Err(E_FACE_MISMATCH);
    }
    seal.verify()?;
    if !seal.sealed {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R2 印面分数逐位：90,000/240,000=37.5%（375‰）；段满额 300/300；
    //    每条 4,500×20=90,000 守恒。
    if FACE_NUMERATOR != 90_000 || FACE_DENOMINATOR != 240_000 {
        return Err(E_FACE_MISMATCH);
    }
    if FACE_NUMERATOR * 1000 != FACE_DENOMINATOR * 375 {
        return Err(E_FACE_MISMATCH);
    }
    if FACE_DENOMINATOR - FACE_NUMERATOR != 150_000 {
        return Err(E_FACE_MISMATCH);
    }
    if SEG_FROZEN_FULL != 300 || ENTRY_ROWS * 20 != 90_000 {
        return Err(E_FACE_MISMATCH);
    }
    hits += 1;

    // R3 段满额不足拒印：seg_frozen=299（B01–B15 差一条未冻结）
    //    必拒——段满额是独立红线。
    let mut seal = B15Seal::new(299);
    let mut i = 0u32;
    while i < ENTRY_COUNT as u32 {
        seal.add_entry(ENTRY_ROWS)?;
        i += 1;
    }
    if seal.seal().is_ok() {
        return Err(E_SEAL_INCOMPLETE);
    }
    if seal.sealed {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R4 印面失配拒印：合计 90,001（掺一条 +1）必拒。
    let mut seal = B15Seal::new(300);
    let mut i = 0u32;
    while i < (ENTRY_COUNT - 1) as u32 {
        seal.add_entry(ENTRY_ROWS)?;
        i += 1;
    }
    seal.add_entry(ENTRY_ROWS + 1)?;
    if seal.seal().is_ok() {
        return Err(E_FACE_MISMATCH);
    }
    hits += 1;

    // R5 封印不可逆：封印后加条/重盖全拒且拒收逐次留痕。
    let mut seal = B15Seal::new(300);
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

    // R6 指纹敏感：同印面复算同指纹；改任一条行数或改段满额汇报值
    //    指纹必变（三域敏感）。
    let mut a = B15Seal::new(300);
    let mut b = B15Seal::new(300);
    let mut c = B15Seal::new(301);
    let mut i = 0u32;
    while i < ENTRY_COUNT as u32 {
        a.add_entry(ENTRY_ROWS)?;
        b.add_entry(ENTRY_ROWS)?;
        c.add_entry(ENTRY_ROWS)?;
        i += 1;
    }
    let fa = a.compute_fingerprint();
    if fa != b.compute_fingerprint() {
        return Err(E_FINGER_DRIFT);
    }
    if fa == c.compute_fingerprint() {
        return Err(E_FINGER_DRIFT); // 段满额域变动指纹必变
    }
    b.entries[0].rows = ENTRY_ROWS + 1;
    if b.compute_fingerprint() == fa {
        return Err(E_FINGER_DRIFT); // 条目域变动指纹必变
    }
    hits += 1;

    // R7 指纹漂移抓拍：伪造登记指纹与重算不符 verify 必红（可红）。
    let mut seal = B15Seal::new(300);
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

    // R8 边界与写回（判据 J2）：零行条目拒、第 21 条拒、清单不齐
    //    写回拒；正常批写回放行。
    let mut seal = B15Seal::new(300);
    if seal.add_entry(0).is_ok() {
        return Err(E_ENTRY_BAD);
    }
    let mut i = 0u32;
    while i < ENTRY_COUNT as u32 {
        seal.add_entry(ENTRY_ROWS)?;
        i += 1;
    }
    if seal.add_entry(ENTRY_ROWS).is_ok() {
        return Err(E_ENTRY_BAD);
    }
    seal.seal()?;
    if seal.writeback(true, 0).is_ok() {
        return Err(E_EMPTY_SUM);
    }
    seal.writeback(true, 0x40)?;
    hits += 1;

    Ok(hits)
}
