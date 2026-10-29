//! UNX-F48400 —— B20 批收官印（20 条冻结/累计 120,000 行·M12 段满额）
//! 判据原文：UNX-F48400-J1 按卡面验收句执行——
//!   J1-1 纯功能代码 ≥237 行；
//!   J1-2 B01–B20 共 20 条批次记录满额登记，逐条冻结；
//!   J1-3 各批行数之和必须恰为 120,000（累计行数总账）；
//!   J1-4 缺批（<20）密封拒绝；行数不齐密封拒绝；零行批次拒绝；
//!   J1-5 密封后再登记必须拒绝（E_FROZEN）；
//!   J1-6 收官印字段齐备（印值非零/批数 20/行数 120000），指纹两次构建一致。
//! 设计纪律：no_std；零 unwrap/expect/panic；确定性 LCG；drill 闭包判据全绿才返回 Ok。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xD2E0; // 卡面分配段

// ── 诊断码闭集（SEG_BASE+1 起连续编号，全部 ≤ SEG_BASE+0x1F）──────
const E_EMPTY: u16 = SEG_BASE + 1;  // 空批次账
const E_STRAY: u16 = SEG_BASE + 2;  // 闭集外批次号
const E_DUP: u16 = SEG_BASE + 3;    // 批次重复登记
const E_ROWZ: u16 = SEG_BASE + 4;   // 零行批次
const E_SHORT: u16 = SEG_BASE + 5;  // 缺批：不足 20 即密封
const E_SUM: u16 = SEG_BASE + 6;    // 行数不齐：累计 ≠ 120,000
const E_FROZEN: u16 = SEG_BASE + 7; // 密封后再登记
const E_STAMP: u16 = SEG_BASE + 8;  // 收官印字段残缺
const E_FINGER: u16 = SEG_BASE + 9; // 指纹不稳定

pub fn detail(code: u16) -> &'static str {
    match code {
        E_EMPTY => "空批次账：无批可封",
        E_STRAY => "闭集外批次号：B01–B20 之外拒绝",
        E_DUP => "批次重复登记：后到拒绝",
        E_ROWZ => "零行批次：行数账破坏",
        E_SHORT => "缺批密封：不足 20 条",
        E_SUM => "行数不齐：累计行数 ≠ 120,000",
        E_FROZEN => "账已冻结：再登记拒绝",
        E_STAMP => "收官印字段残缺：印值/批数/行数核对失败",
        E_FINGER => "指纹不稳定：确定性被破坏",
        _ => "未知诊断码",
    }
}

/// 确定性 LCG。
fn lcg_next(state: &mut u32) -> u32 {
    *state = state
        .wrapping_mul(1_664_525)
        .wrapping_add(1_013_904_223);
    *state
}

/// FNV-1a 64。
fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

// ── 数据模型 ────────────────────────────────────────────────
/// B01–B20 批次号闭集。
pub const BATCH_IDS: [&str; 20] = [
    "B01", "B02", "B03", "B04", "B05", "B06", "B07", "B08", "B09", "B10",
    "B11", "B12", "B13", "B14", "B15", "B16", "B17", "B18", "B19", "B20",
];

/// 各批行数规范表：前 4 批浮动，其余 6000，总和恰为 120,000。
pub const BATCH_ROWS: [u32; 20] = [
    6100, 5900, 6050, 5950, 6000, 6000, 6000, 6000,
    6000, 6000, 6000, 6000, 6000, 6000, 6000, 6000,
    6000, 6000, 6000, 6000,
];

/// 累计行数总账目标。
pub const TOTAL_ROWS: u32 = 120_000;

/// 单批记录。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BatchRow {
    pub id: &'static str,
    pub rows: u32,
    pub frozen: bool,
}

/// 收官印。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SealImprint {
    pub stamp: u64,
    pub batches: usize,
    pub total_rows: u32,
}

impl SealImprint {
    /// 字段齐备判据。
    pub fn intact(&self) -> bool {
        self.stamp != 0 && self.batches == 20 && self.total_rows == TOTAL_ROWS
    }
}

/// 收官账。
pub struct SealLedger {
    pub batches: Vec<BatchRow>,
    pub sealed: bool,
    pub imprint: Option<SealImprint>,
}

impl SealLedger {
    pub fn new() -> SealLedger {
        SealLedger {
            batches: Vec::new(),
            sealed: false,
            imprint: None,
        }
    }

    /// 批次登记：闭集内 + 不重复 + 行数非零 + 未冻结。
    pub fn register(&mut self, id: &'static str, rows: u32) -> Result<u32, u16> {
        if self.sealed {
            return Err(E_FROZEN);
        }
        if id.is_empty() {
            return Err(E_EMPTY);
        }
        if !BATCH_IDS.contains(&id) {
            return Err(E_STRAY);
        }
        if self.batches.iter().any(|b| b.id == id) {
            return Err(E_DUP);
        }
        if rows == 0 {
            return Err(E_ROWZ);
        }
        self.batches.push(BatchRow { id, rows, frozen: true });
        Ok(self.batches.len() as u32)
    }

    /// 行数累计。
    pub fn sum_rows(&self) -> u64 {
        self.batches.iter().map(|b| b.rows as u64).sum()
    }

    /// 满额判定：20 批且行数总账相符。
    pub fn full(&self) -> bool {
        self.batches.len() == 20 && self.sum_rows() == TOTAL_ROWS as u64
    }

    /// 密封并颁发收官印。
    pub fn seal(&mut self) -> Result<SealImprint, u16> {
        if self.batches.is_empty() {
            return Err(E_EMPTY);
        }
        if self.batches.len() < 20 {
            return Err(E_SHORT);
        }
        if self.sum_rows() != TOTAL_ROWS as u64 {
            return Err(E_SUM);
        }
        for b in &self.batches {
            if b.rows == 0 {
                return Err(E_ROWZ);
            }
        }
        let mut buf: Vec<u8> = vec![0xB2u8, 0xE0];
        for b in &self.batches {
            buf.extend_from_slice(b.id.as_bytes());
            buf.extend_from_slice(&b.rows.to_le_bytes());
        }
        let imp = SealImprint {
            stamp: fnv1a64(&buf),
            batches: self.batches.len(),
            total_rows: TOTAL_ROWS,
        };
        if !imp.intact() {
            return Err(E_STAMP);
        }
        self.sealed = true;
        self.imprint = Some(imp);
        Ok(imp)
    }

    /// 收官报告。
    pub fn report(&self) -> Result<String, u16> {
        let imp = match self.imprint {
            Some(i) => i,
            None => return Err(E_STAMP),
        };
        let mut s = String::new();
        s.push_str("b20seal 批数 ");
        s.push_str(&imp.batches.to_string());
        s.push_str(" 累计 ");
        s.push_str(&imp.total_rows.to_string());
        s.push_str(" 行，M12 段满额");
        Ok(s)
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut buf: Vec<u8> = Vec::new();
        for b in &self.batches {
            buf.extend_from_slice(b.id.as_bytes());
            buf.extend_from_slice(&b.rows.to_le_bytes());
        }
        buf.push(self.sealed as u8);
        match self.imprint {
            Some(i) => buf.extend_from_slice(&i.stamp.to_le_bytes()),
            None => buf.push(0),
        }
        fnv1a64(&buf)
    }
}

/// 规范收官账：按 BATCH_ROWS 满额登记。
pub fn canonical_ledger() -> Result<SealLedger, u16> {
    let mut led = SealLedger::new();
    for (i, id) in BATCH_IDS.iter().enumerate() {
        led.register(id, BATCH_ROWS[i])?;
    }
    Ok(led)
}

// ── 主巡检入口 ──────────────────────────────────────────────
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：20 批满额登记并密封，收官印字段齐备。
    let mut led = canonical_ledger()?;
    if led.batches.len() != 20 {
        return Err(E_SHORT);
    }
    let imp = led.seal()?;
    if !imp.intact() {
        return Err(E_STAMP);
    }
    hits += 1;

    // R2：行数累计对账：逐项和 == 120,000 == 常量总账。
    let sum = led.sum_rows();
    if sum != TOTAL_ROWS as u64 {
        return Err(E_SUM);
    }
    let const_sum: u64 = BATCH_ROWS.iter().map(|r| *r as u64).sum();
    if const_sum != TOTAL_ROWS as u64 {
        return Err(E_SUM);
    }
    hits += 1;

    // R3：段满额判定：sealed && full()。
    if !led.sealed || !led.full() {
        return Err(E_SHORT);
    }
    hits += 1;

    // R4：密后再写拒绝 → E_FROZEN。
    if led.register("B01", 6000) != Err(E_FROZEN) {
        return Err(E_FROZEN);
    }
    hits += 1;

    // R5：缺批密封拒绝 → E_SHORT。
    let mut led5 = SealLedger::new();
    for i in 0..19 {
        led5.register(BATCH_IDS[i], BATCH_ROWS[i])?;
    }
    if led5.seal() != Err(E_SHORT) {
        return Err(E_SHORT);
    }
    hits += 1;

    // R6：行数不齐密封拒绝 → E_SUM（末批 +1）。
    let mut led6 = canonical_ledger()?;
    led6.batches[19].rows = led6.batches[19].rows + 1;
    if led6.seal() != Err(E_SUM) {
        return Err(E_SUM);
    }
    hits += 1;

    // R7：零行批次拒绝 → E_ROWZ；重复批次拒绝 → E_DUP；闭集外批次拒绝 → E_STRAY。
    let mut led7 = SealLedger::new();
    if led7.register("B01", 0) != Err(E_ROWZ) {
        return Err(E_ROWZ);
    }
    led7.register("B01", 6000)?;
    if led7.register("B01", 6000) != Err(E_DUP) {
        return Err(E_DUP);
    }
    if led7.register("B99", 6000) != Err(E_STRAY) {
        return Err(E_STRAY);
    }
    hits += 1;

    // R8：LCG 抽批复核：抽 3 批行数与规范表一致且冻结位为真。
    let led8 = canonical_ledger()?;
    let mut state: u32 = 0xD2E0_0001;
    for _ in 0..3 {
        let idx = (lcg_next(&mut state) % 20) as usize;
        let b = led8
            .batches
            .iter()
            .find(|b| b.id == BATCH_IDS[idx])
            .ok_or(E_SHORT)?;
        if b.rows != BATCH_ROWS[idx] || !b.frozen {
            return Err(E_STAMP);
        }
    }
    hits += 1;

    // R9：指纹：同构两账（含印）指纹一致；未密封账指纹不同。
    let mut led9a = canonical_ledger()?;
    let mut led9b = canonical_ledger()?;
    led9a.seal()?;
    led9b.seal()?;
    if led9a.fingerprint() != led9b.fingerprint() {
        return Err(E_FINGER);
    }
    let led9c = canonical_ledger()?;
    if led9c.fingerprint() == led9a.fingerprint() {
        return Err(E_FINGER);
    }
    hits += 1;

    // R10：收官报告：含批数 20 与累计 120000。
    let rep = led9a.report()?;
    if !rep.contains("20") || !rep.contains("120000") {
        return Err(E_STAMP);
    }
    hits += 1;

    Ok(hits)
}
