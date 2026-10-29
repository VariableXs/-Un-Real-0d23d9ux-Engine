//! unx48380_b19seal — UNX-F48380 B19 批收官印（20 条冻结/累计 114,000 行）（诊断段 0xD060）。
//!
//! 判据原文：
//!   J1 批印入册：域账 114,000/240,000；判据锚定 §AI-61 §7。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 收官印 = 确定性批戳：对 20 条冻结清单（卡号, 行数）有序折叠
//!      FNV-1a 得批戳；同清单重盖同戳（可复现），清单任一位被篡改
//!      则复算戳必不等于在册戳（防篡改）。
//!   2. 冻结单向：收官后清单只读，二次收官拒绝（幂等守印）；补条/
//!      删条/改行数一律走「新批戳」流程且必须显式换批号，不许就地改冻。
//!   3. 域账递推：累计行数 = 前累计 + 本批行数，恒等式逐位核对；
//!      本批封账后域账 114,000/240,000（额度线 475‰ 断言）。
//!   4. 条目规约：批清单恰 20 条；卡号 48363..48382 严格递增无重号；
//!      行数全为正且本批合计恰 8,400；本批后 10 卡行数必须与卡面
//!      验收行数逐条全等（292/272/272/272/272/232/292/232/417/317）。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xD060;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_ITEM_COUNT: u16 = SEG_BASE + 1;  // 清单条数 != 20
const E_CARD_ORDER: u16 = SEG_BASE + 2;  // 卡号乱序/重号
const E_ROWS_BAD: u16 = SEG_BASE + 3;    // 行数非正/合计不符
const E_RESEAL: u16 = SEG_BASE + 4;      // 二次收官未拒绝
const E_STAMP_DRIFT: u16 = SEG_BASE + 5; // 复算戳漂移（同清单异戳）
const E_TAMPER_MISS: u16 = SEG_BASE + 6; // 清单篡改未检出
const E_QUOTA_BAD: u16 = SEG_BASE + 7;   // 域账递推/额度断言失败
const E_CARD_ROWS: u16 = SEG_BASE + 8;   // 卡面行数不符
const E_CODE_RANGE: u16 = SEG_BASE + 9;  // 诊断码越段

/// 批清单条数。
pub const ITEMS: usize = 20;

/// 本批合计行数。
pub const BATCH_ROWS: u64 = 8_400;

/// 封账前累计行数（B18 收官时点）。
pub const PRIOR_CUMUL: u64 = 105_600;

/// 封账后累计行数（B19 收官时点，判据 114,000/240,000）。
pub const CUMUL: u64 = 114_000;

/// 域账额度。
pub const QUOTA: u64 = 240_000;

/// 本卡（F48373..F48382）卡面验收行数，与清单逐条全等核对。
pub const CARD_ROWS: [u32; 10] = [292, 272, 272, 272, 272, 232, 292, 232, 417, 317];

/// 批条目：卡号 + 冻结行数。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct BatchItem {
    pub card: u32,
    pub rows: u32,
}

/// 批戳：对 (card, rows) 序对有序 FNV-1a 折叠，混入批号；高位置位保非零。
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
    h | (1 << 63)
}

/// 收官印账：批号 → 批戳 + 冻结清单。
pub struct SealBook {
    batch_id: u32,
    stamp: u64,
    items: Vec<BatchItem>,
    sealed: bool,
}

impl SealBook {
    pub fn new() -> Self {
        SealBook { batch_id: 0, stamp: 0, items: Vec::new(), sealed: false }
    }

    /// 清单校验：恰 20 条 + 卡号严格递增无重号 + 行数全正 + 合计 8,400。
    pub fn validate(items: &[BatchItem]) -> Result<u64, u16> {
        if items.len() != ITEMS {
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
        if sum != BATCH_ROWS {
            return Err(E_ROWS_BAD);
        }
        Ok(sum)
    }

    /// 卡面行数核对：本批后 10 卡（F48373..F48382）与验收行数逐条全等。
    pub fn card_rows_ok(items: &[BatchItem]) -> Result<(), u16> {
        if items.len() != ITEMS {
            return Err(E_ITEM_COUNT);
        }
        let mut k = 0usize;
        while k < 10 {
            let item = items[10 + k];
            if item.card != 48373 + k as u32 {
                return Err(E_CARD_ROWS);
            }
            if item.rows != CARD_ROWS[k] {
                return Err(E_CARD_ROWS);
            }
            k += 1;
        }
        Ok(())
    }

    /// 收官：校验 → 折戳 → 冻结。已冻则拒绝（幂等守印）。
    pub fn seal(&mut self, batch_id: u32, items: &[BatchItem]) -> Result<u64, u16> {
        if self.sealed {
            return Err(E_RESEAL);
        }
        Self::validate(items)?;
        Self::card_rows_ok(items)?;
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

    /// 复核：在册戳 vs 按在册清单复算戳。
    pub fn verify(&self) -> bool {
        if !self.sealed {
            return false;
        }
        self.stamp == stamp_of(self.batch_id, &self.items)
    }

    /// 对外部清单的复算：篡改后复算戳必不等于在册戳。
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
        QuotaBook { cumulative: prior, quota }
    }

    /// 递推：累计 += 批行数；返回（前累计, 后累计）供恒等式核对。
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

/// B19 批清单夹具：20 条连续卡（48363..48382），行数合计恰 8,400。
/// 前 10 卡（48363..48372）行数 535+4i（535..571，闭式和 5,530）；
/// 后 10 卡与 CARD_ROWS 全等（合计 2,870）→ 5,530+2,870=8,400。
pub fn batch_manifest() -> Vec<BatchItem> {
    let mut items: Vec<BatchItem> = Vec::new();
    let mut i = 0usize;
    while i < ITEMS {
        let rows = if i < 10 { 535 + 4 * i as u32 } else { CARD_ROWS[i - 10] };
        items.push(BatchItem { card: 48363 + i as u32, rows });
        i += 1;
    }
    items
}

/// 自例程判据：逐条落实 J1（批印入册：域账 114,000/240,000），返回命中组数。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;
    let manifest = batch_manifest();

    // R1 清单规约：20 条 / 递增无重号 / 行数全正 / 合计恰 8,400。
    let sum = SealBook::validate(&manifest)?;
    if sum != BATCH_ROWS {
        return Err(E_ROWS_BAD);
    }
    hits += 1;

    // R2 卡面核对：后 10 卡行数与卡面验收行数逐条全等。
    SealBook::card_rows_ok(&manifest)?;
    hits += 1;

    // R3 收官：批戳产出非零且高位在册；封账态置位。
    let mut book = SealBook::new();
    let stamp = book.seal(0x0B19, &manifest)?;
    if stamp == 0 || !book.sealed() || stamp < (1u64 << 63) {
        return Err(E_STAMP_DRIFT);
    }
    hits += 1;

    // R4 复现：同清单同批号在全新印簿重盖 → 批戳逐位一致。
    let mut book2 = SealBook::new();
    let stamp2 = book2.seal(0x0B19, &manifest)?;
    if stamp2 != stamp || !book.verify() || !book2.verify() {
        return Err(E_STAMP_DRIFT);
    }
    hits += 1;

    // R5 幂等守印：已收官印簿再收官 → 必须拒绝。
    match book.seal(0x0B19, &manifest) {
        Err(E_RESEAL) => {}
        _ => return Err(E_RESEAL),
    }
    hits += 1;

    // R6 防篡改：行数 +1 / 卡号改位 / 换批号 → 复算必不一致。
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
    tam2[3].card = 48372; // 破坏递增（与前位撞序）
    if book.verify_against(&tam2) {
        return Err(E_TAMPER_MISS);
    }
    let mut book3 = SealBook::new();
    let stamp_b = book3.seal(0x0B1A, &manifest)?; // 换批号
    if stamp_b == stamp {
        return Err(E_TAMPER_MISS);
    }
    hits += 1;

    // R7 域账递推：前累计 105,600 + 本批 8,400 = 114,000（恒等式核对）。
    let mut quota = QuotaBook::new(PRIOR_CUMUL, QUOTA);
    let (before, after) = quota.add_batch(BATCH_ROWS as u32);
    if before != PRIOR_CUMUL || after != CUMUL || quota.cumulative() != CUMUL {
        return Err(E_QUOTA_BAD);
    }
    if !quota.within_quota() {
        return Err(E_QUOTA_BAD);
    }
    hits += 1;

    // R8 额度线：114,000/240,000 = 475‰，千分比断言。
    if quota.per_mille() != 475 {
        return Err(E_QUOTA_BAD);
    }
    hits += 1;

    // R9 诊断码闭包：全部错误码落在 [SEG_BASE+1, SEG_BASE+0x1F] 段内。
    const CODES: [u16; 9] = [
        E_ITEM_COUNT,
        E_CARD_ORDER,
        E_ROWS_BAD,
        E_RESEAL,
        E_STAMP_DRIFT,
        E_TAMPER_MISS,
        E_QUOTA_BAD,
        E_CARD_ROWS,
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
