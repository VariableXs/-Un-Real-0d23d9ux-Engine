//! unx48280_b14seal — UNX-F48280 B14 批收官印（20 条冻结/累计 84,000 行）（诊断段 0xC3E0）。
//!
//! 判据原文：
//!   J1 批印入册：域账 84,000/240,000；判据锚定 §AI-61 §7。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 收官印 = 确定性批戳：对 20 条冻结清单（卡号, 行数）有序折叠
//!      FNV-1a 得批戳；同清单重盖同戳（可复现），清单任一位被篡改
//!      则复算戳必不等于在册戳（防篡改）。
//!   2. 冻结单向：收官后清单只读，二次收官拒绝（幂等守印）；补条/
//!      删条/改行数全部走「新批戳」流程且必须显式换批号，不允许
//!      就地改冻。
//!   3. 域账递推：累计行数 = 前累计 + 本批行数，恒等式逐位核对；
//!      本批封账后域账 84,000/240,000（35% 额度线断言）。
//!   4. 条目规约：批清单恰 20 条；卡号严格递增无重号；行数全为正
//!      且本批合计 8,400（夹具不变量，域账递推的输入）。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xC3E0;

const E_ITEM_COUNT: u16 = SEG_BASE + 1; // 清单条数 != 20
const E_CARD_ORDER: u16 = SEG_BASE + 2; // 卡号乱序/重号
const E_ROWS_BAD: u16 = SEG_BASE + 3;   // 行数非正/合计不符
const E_RESEAL: u16 = SEG_BASE + 4;     // 二次收官未拒绝
const E_STAMP_DRIFT: u16 = SEG_BASE + 5; // 复算戳漂移（同清单异戳）
const E_TAMPER_MISS: u16 = SEG_BASE + 6; // 清单篡改未检出
const E_QUOTA_BAD: u16 = SEG_BASE + 7;  // 域账递推/额度断言失败
const E_CODE_RANGE: u16 = SEG_BASE + 8; // 诊断码越段

/// 批条目：卡号 + 冻结行数。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct BatchItem {
    pub card: u32,
    pub rows: u32,
}

/// 收官印账：批号 → 批戳（FNV-1a 有序折叠）+ 冻结清单。
pub struct SealBook {
    batch_id: u32,
    stamp: u64,
    items: Vec<BatchItem>,
    sealed: bool,
}

/// 批戳：对 (card, rows) 序对有序 FNV-1a 折叠，混入批号。
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
    h |= 1 << 63; // 高位置位：戳恒非零，与「未盖印」状态可区分
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

    /// 清单校验：恰 20 条 + 卡号严格递增无重号 + 行数全正 + 合计 8,400。
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
        if sum != 8_400 {
            return Err(E_ROWS_BAD);
        }
        Ok(sum as u32)
    }

    /// 收官：校验 → 折戳 → 冻结。已冻则拒绝（幂等守印）。
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

    /// 复核：在册戳 vs 按在册清单复算戳（防篡改判据）。
    pub fn verify(&self) -> bool {
        if !self.sealed {
            return false;
        }
        self.stamp == stamp_of(self.batch_id, &self.items)
    }

    /// 对外部篡改清单的复算：篡改后复算戳必不等于在册戳。
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

/// 域账额度簿：累计行数递推 + 额度断言。
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

    /// 递推：累计 += 批行数；返回 (前累计, 后累计) 供恒等式核对。
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

    /// 千分比（相对额度，整数千分位）。
    pub fn per_mille(&self) -> u64 {
        if self.quota == 0 {
            return 0;
        }
        self.cumulative * 1000 / self.quota
    }
}

/// B14 批清单夹具：20 条连续卡，行数合计 8,400。
pub fn batch_manifest() -> Vec<BatchItem> {
    let mut items: Vec<BatchItem> = Vec::new();
    // 卡号 F48273..F48292（十进制 48273..48292），行数递推合计 8,400：
    // rows_i = 401 + 2i（401..439，闭式和 = 20*401 + 2*190 = 8,400）。
    let mut i = 0u32;
    while i < 20 {
        let rows = 401 + i * 2;
        items.push(BatchItem {
            card: 48273 + i,
            rows,
        });
        i += 1;
    }
    items
}

/// B14 收官印主巡检：批印入册 + 域账递推全链判据。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;
    let manifest = batch_manifest();

    // R1 清单规约：20 条 / 递增无重号 / 行数全正 / 合计 8,400。
    let sum = SealBook::validate(&manifest)?;
    if sum != 8_400 {
        return Err(E_ROWS_BAD);
    }
    hits += 1;

    // R2 收官：批戳产出非零；封账态置位。
    let mut book = SealBook::new();
    let stamp = book.seal(0x0B14, &manifest)?;
    if stamp == 0 || !book.sealed() {
        return Err(E_STAMP_DRIFT);
    }
    hits += 1;

    // R3 复现：同清单同批号在全新印簿上重盖 → 批戳逐位一致。
    let mut book2 = SealBook::new();
    let stamp2 = book2.seal(0x0B14, &manifest)?;
    if stamp2 != stamp {
        return Err(E_STAMP_DRIFT);
    }
    // 印簿自身 verify 复算通路也要一致。
    if !book.verify() || !book2.verify() {
        return Err(E_STAMP_DRIFT);
    }
    hits += 1;

    // R4 幂等守印：已收官印簿再收官 → 必须拒绝。
    let again = book.seal(0x0B14, &manifest);
    match again {
        Err(E_RESEAL) => { /* 预期路径 */ }
        _ => return Err(E_RESEAL),
    }
    hits += 1;

    // R5 防篡改：清单任一条行数 +1 / 任一卡号改位 / 批号改 → 复算必不一致。
    let mut tam1: Vec<BatchItem> = Vec::new();
    let mut i = 0;
    while i < manifest.len() {
        tam1.push(manifest[i]);
        i += 1;
    }
    tam1[7].rows += 1;
    if book.verify_against(&tam1) {
        return Err(E_TAMPER_MISS);
    }
    let mut tam2: Vec<BatchItem> = Vec::new();
    i = 0;
    while i < manifest.len() {
        tam2.push(manifest[i]);
        i += 1;
    }
    tam2[3].card = 48299; // 破坏递增（与后位撞序）
    if book.verify_against(&tam2) {
        return Err(E_TAMPER_MISS);
    }
    let mut book3 = SealBook::new();
    let stamp_b = book3.seal(0x0B15, &manifest)?; // 换批号
    if stamp_b == stamp {
        return Err(E_TAMPER_MISS);
    }
    hits += 1;

    // R6 域账递推：前累计 75,600 + 本批 8,400 = 84,000（恒等式逐位核对）。
    let mut quota = QuotaBook::new(75_600, 240_000);
    let (before, after) = quota.add_batch(8_400);
    if before != 75_600 || after != 84_000 {
        return Err(E_QUOTA_BAD);
    }
    if quota.cumulative() != 84_000 {
        return Err(E_QUOTA_BAD);
    }
    if !quota.within_quota() {
        return Err(E_QUOTA_BAD);
    }
    hits += 1;

    // R7 额度线：84,000/240,000 = 350‰（35%），千分比断言。
    if quota.per_mille() != 350 {
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
