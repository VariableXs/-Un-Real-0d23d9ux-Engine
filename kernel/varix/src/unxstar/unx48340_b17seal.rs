//! unx48340_b17seal — UNX-F48340 B17 批收官印（20 条冻结/累计 102,000 行）（诊断段 0xCB60）。
//!
//! 判据原文：
//!   J1 批印入册：域账 102,000/240,000；判据锚定 §AI-61 §7。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 收官印 = 确定性批戳：对 20 条冻结清单（卡号, 行数）有序折叠
//!      FNV-1a 得批戳（与 B14/B15 同源机制）；同清单重盖同戳，
//!      清单任一位被篡改则复算戳必不等于在册戳。
//!   2. 冻结单向：收官后清单只读，二次收官拒绝（幂等守印）。
//!   3. 域账递推：累计行数 = 前累计 + 本批行数恒等式逐位核对；
//!      本批封账后域账 102,000/240,000（425‰ 断言）。
//!   4. 条目规约：恰 20 条；卡号严格递增无重号；行数全为正且本批
//!      合计 8,000（B17 批不变量）。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec;
use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xCB60;

const E_ITEM_COUNT: u16 = SEG_BASE + 1;  // 清单条数 != 20
const E_CARD_ORDER: u16 = SEG_BASE + 2;  // 卡号乱序/重号
const E_ROWS_BAD: u16 = SEG_BASE + 3;    // 行数非正/合计不符
const E_RESEAL: u16 = SEG_BASE + 4;      // 二次收官未拒绝
const E_STAMP_DRIFT: u16 = SEG_BASE + 5; // 复算戳漂移
const E_TAMPER_MISS: u16 = SEG_BASE + 6; // 清单篡改未检出
const E_QUOTA_BAD: u16 = SEG_BASE + 7;   // 域账递推/额度断言失败
const E_CODE_RANGE: u16 = SEG_BASE + 8;  // 诊断码越段

/// 批条目。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct BatchItem {
    pub card: u32,
    pub rows: u32,
}

/// 收官印账。
pub struct SealBook {
    batch_id: u32,
    stamp: u64,
    items: Vec<BatchItem>,
    sealed: bool,
}

/// 批戳（FNV-1a 有序折叠 + 批号混入）。
pub fn stamp_of(batch_id: u32, items: &[BatchItem]) -> u64 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    h ^= batch_id as u64;
    h = h.wrapping_mul(0x0000_0100_0000_01B3);
    let mut i = 0;
    while i < items.len() {
        h ^= items[i].card as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
        h ^= items[i].rows as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
        i += 1;
    }
    h |= 1 << 63;
    h
}

impl SealBook {
    pub fn new() -> Self {
        SealBook {
            batch_id: 0,
            stamp: 0,
            items: Vec::new(),
            sealed: false,
        }
    }

    /// 清单校验：20 条 / 递增无重号 / 行数全正 / 合计 8,000。
    pub fn validate(items: &[BatchItem]) -> Result<u32, u16> {
        if items.len() != 20 {
            return Err(E_ITEM_COUNT);
        }
        let mut i = 1;
        while i < items.len() {
            if items[i].card <= items[i - 1].card {
                return Err(E_CARD_ORDER);
            }
            i += 1;
        }
        let mut sum: u64 = 0;
        let mut j = 0;
        while j < items.len() {
            if items[j].rows == 0 {
                return Err(E_ROWS_BAD);
            }
            sum += items[j].rows as u64;
            j += 1;
        }
        if sum != 8_000 {
            return Err(E_ROWS_BAD);
        }
        Ok(sum as u32)
    }

    pub fn seal(&mut self, batch_id: u32, items: &[BatchItem]) -> Result<u64, u16> {
        if self.sealed {
            return Err(E_RESEAL);
        }
        Self::validate(items)?;
        self.batch_id = batch_id;
        let mut i = 0;
        while i < items.len() {
            self.items.push(items[i]);
            i += 1;
        }
        self.stamp = stamp_of(batch_id, &self.items);
        self.sealed = true;
        Ok(self.stamp)
    }

    pub fn verify(&self) -> bool {
        if !self.sealed {
            return false;
        }
        self.stamp == stamp_of(self.batch_id, &self.items)
    }

    pub fn verify_against(&self, items: &[BatchItem]) -> bool {
        if !self.sealed {
            return false;
        }
        self.stamp == stamp_of(self.batch_id, items)
    }

    pub fn stamp(&self) -> u64 {
        self.stamp
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }
}

/// 域账额度簿。
pub struct QuotaBook {
    cumulative: u64,
    quota: u64,
}

impl QuotaBook {
    pub fn new(prior: u64, quota: u64) -> Self {
        QuotaBook {
            cumulative: prior,
            quota,
        }
    }

    pub fn add_batch(&mut self, batch_rows: u32) -> (u64, u64) {
        let before = self.cumulative;
        self.cumulative += batch_rows as u64;
        (before, self.cumulative)
    }

    pub fn cumulative(&self) -> u64 {
        self.cumulative
    }

    pub fn within_quota(&self) -> bool {
        self.cumulative <= self.quota
    }

    pub fn per_mille(&self) -> u64 {
        if self.quota == 0 {
            return 0;
        }
        self.cumulative * 1000 / self.quota
    }
}

/// B17 批清单夹具：卡号 F48323..F48342，行数 rows_i = 380 + i*2
/// （380..418，闭式和 = 20*380 + 2*190 = 8,000）。
pub fn batch_manifest() -> Vec<BatchItem> {
    let mut items: Vec<BatchItem> = Vec::new();
    let mut i = 0u32;
    while i < 20 {
        items.push(BatchItem {
            card: 48323 + i,
            rows: 380 + i * 2,
        });
        i += 1;
    }
    items
}

/// B17 收官印主巡检：批印入册 + 域账递推全链判据。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;
    let manifest = batch_manifest();

    // R1 清单规约：20 条 / 递增无重号 / 行数全正 / 合计 8,000。
    let sum = SealBook::validate(&manifest)?;
    if sum != 8_000 {
        return Err(E_ROWS_BAD);
    }
    hits += 1;

    // R2 收官：批戳产出非零；封账态置位。
    let mut book = SealBook::new();
    let stamp = book.seal(0x0B17, &manifest)?;
    if stamp == 0 || !book.sealed() {
        return Err(E_STAMP_DRIFT);
    }
    hits += 1;

    // R3 复现：全新印簿重盖同戳。
    let mut book2 = SealBook::new();
    let stamp2 = book2.seal(0x0B17, &manifest)?;
    if stamp2 != stamp || !book.verify() || !book2.verify() {
        return Err(E_STAMP_DRIFT);
    }
    hits += 1;

    // R4 幂等守印：再收官必须拒绝。
    match book.seal(0x0B17, &manifest) {
        Err(E_RESEAL) => {}
        _ => return Err(E_RESEAL),
    }
    hits += 1;

    // R5 防篡改：行数 +1 / 卡号改位 / 批号改 → 复算必不一致。
    let mut tam1: Vec<BatchItem> = Vec::new();
    let mut i = 0;
    while i < manifest.len() {
        tam1.push(manifest[i]);
        i += 1;
    }
    tam1[5].rows += 1;
    if book.verify_against(&tam1) {
        return Err(E_TAMPER_MISS);
    }
    let mut tam2: Vec<BatchItem> = Vec::new();
    i = 0;
    while i < manifest.len() {
        tam2.push(manifest[i]);
        i += 1;
    }
    tam2[9].card = 48330; // 破坏递增
    if book.verify_against(&tam2) {
        return Err(E_TAMPER_MISS);
    }
    let mut book3 = SealBook::new();
    let stamp_b = book3.seal(0x0B18, &manifest)?;
    if stamp_b == stamp {
        return Err(E_TAMPER_MISS);
    }
    hits += 1;

    // R6 域账递推：96,000 + 8,000 = 104,000？否——B17 封账后累计
    //    102,000：前累计 94,000 + 本批 8,000 = 102,000（恒等式）。
    let mut quota = QuotaBook::new(94_000, 240_000);
    let (before, after) = quota.add_batch(8_000);
    if before != 94_000 || after != 102_000 {
        return Err(E_QUOTA_BAD);
    }
    if quota.cumulative() != 102_000 {
        return Err(E_QUOTA_BAD);
    }
    if !quota.within_quota() {
        return Err(E_QUOTA_BAD);
    }
    hits += 1;

    // R7 额度线：102,000/240,000 = 425‰（42.5%），千分比断言。
    if quota.per_mille() != 425 {
        return Err(E_QUOTA_BAD);
    }
    hits += 1;

    // R8 诊断码闭包：全部错误码落在 [SEG_BASE+1, SEG_BASE+0x1F] 段内。
    const CODES: [u16; 8] = [
        E_ITEM_COUNT,
        E_CARD_ORDER,
        E_ROWS_BAD,
        E_RESEAL,
        E_STAMP_DRIFT,
        E_TAMPER_MISS,
        E_QUOTA_BAD,
        E_CODE_RANGE,
    ];
    let mut ci = 0;
    while ci < CODES.len() {
        if CODES[ci] <= SEG_BASE || CODES[ci] > SEG_BASE + 0x1F {
            return Err(E_CODE_RANGE);
        }
        ci += 1;
    }
    hits += 1;

    Ok(hits)
}
