//! unx48379_expmerge — UNX-F48379 体验事件与错误事件合流口径（同源时间轴不双记）（诊断段 0xD040）。
//!
//! 判据原文：
//!   J1 合流 ×10 抽查零双记；判据锚定 体验十三·补。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 合流键唯一：合流键 =（源号 src, 同源时戳 ts）二元组；同一合流键
//!      无论体验流、错误流各来一条还是同流重放，账面只许落一条——
//!      双记即口径破坏，零容忍。
//!   2. 成因并集：首条入账定成因位（bit0=体验 bit1=错误），重复键命中
//!      只并成因位不落新账（返回 false + 双记计数 +1）——同源同刻两流
//!      皆有的事件在账面上是一条 cause=3 的合并记录。
//!   3. 时间轴全序：合流账按（src, ts）升序全序排列；同源段内 ts 严格
//!      递增；相邻键恒不等（零双记的账面形态判据）。
//!   4. ×10 抽查批：每批 12 体验 + 7 错误事件，其中恰 4 键两流重叠；
//!      合流后恰 15 条（12+7-4），成因账恰 8 体验 / 3 错误 / 4 合并。
//!   5. 探针双向：对既有键再插必须被拒（len 不变、双记计数 +1）；
//!      全新键必须能入账（len +1）——拒重不拒新。
//!   6. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xD040;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_DUP_RECORD: u16 = SEG_BASE + 1;  // 同键双记（口径破坏）
const E_ORDER_BAD: u16 = SEG_BASE + 2;   // 合流账非全序
const E_TL_BREAK: u16 = SEG_BASE + 3;    // 同源时间轴断裂
const E_CAUSE_BAD: u16 = SEG_BASE + 4;   // 成因账不符
const E_COUNT_BAD: u16 = SEG_BASE + 5;   // 合流条数/重叠计数不符
const E_PROBE_MISS: u16 = SEG_BASE + 6;  // 拒重/纳新探针失效
const E_REPLAY_DRIFT: u16 = SEG_BASE + 7; // 同批重建指纹漂移
const E_CODE_RANGE: u16 = SEG_BASE + 8;  // 诊断码越段

/// 抽查批数（判据 ×10）。
pub const BATCHES: u32 = 10;

/// 每批体验事件数。
pub const EXP_PER: u32 = 12;

/// 每批错误事件数。
pub const ERR_PER: u32 = 7;

/// 每批两流重叠键数。
pub const OVERLAP_PER: u32 = 4;

/// 成因位：bit0=体验，bit1=错误；3=两流合并。
pub const CAUSE_EXP: u8 = 1;
pub const CAUSE_ERR: u8 = 2;
pub const CAUSE_BOTH: u8 = 3;

/// 合流记录：键（src, ts）+ 成因位。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct MergedEvent {
    pub src: u32,
    pub ts: u64,
    pub cause: u8,
}

/// 合流键比较：-1 a<b / 0 相等 / 1 a>b。
pub fn cmp_key(a: (u32, u64), b: (u32, u64)) -> i8 {
    if a.0 != b.0 {
        if a.0 < b.0 {
            return -1;
        }
        return 1;
    }
    if a.1 < b.1 {
        return -1;
    }
    if a.1 > b.1 {
        return 1;
    }
    0
}

/// FNV-1a 折叠（合流账指纹）。
pub fn fnv1a(seed: u64, x: u64) -> u64 {
    let mut h = seed;
    h ^= x;
    h = h.wrapping_mul(0x0000_0100_0000_01B3);
    h
}

/// 合流账指纹。
pub fn ledger_fp(recs: &[MergedEvent]) -> u64 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    let mut i = 0;
    while i < recs.len() {
        h = fnv1a(h, recs[i].src as u64);
        h = fnv1a(h, recs[i].ts);
        h = fnv1a(h, recs[i].cause as u64);
        i += 1;
    }
    h
}

/// 合流账：按（src, ts）升序全序；同键只并成因不落新账。
pub struct MergeLedger {
    recs: Vec<MergedEvent>,
    doubles: u32,
}

impl MergeLedger {
    pub fn new() -> Self {
        MergeLedger { recs: Vec::new(), doubles: 0 }
    }

    /// 入账：新键插入有序位并返回 Ok(true)；既有键并成因位、双记计数
    /// +1 并返回 Ok(false)（同源同刻不双记的唯一通路）。
    pub fn insert(&mut self, src: u32, ts: u64, cause: u8) -> Result<bool, u16> {
        let mut pos = 0usize;
        let mut found = false;
        while pos < self.recs.len() {
            let c = cmp_key((self.recs[pos].src, self.recs[pos].ts), (src, ts));
            if c == 0 {
                found = true;
                break;
            }
            if c == 1 {
                break; // 插入位
            }
            pos += 1;
        }
        if found {
            self.recs[pos].cause |= cause;
            self.doubles += 1;
            return Ok(false);
        }
        self.recs.push(MergedEvent { src, ts, cause });
        let mut i = self.recs.len() - 1;
        while i > pos {
            self.recs.swap(i - 1, i);
            i -= 1;
        }
        Ok(true)
    }

    /// 相邻键恒不等（零双记形态判据）。
    pub fn no_adjacent_dup(&self) -> Result<(), u16> {
        let mut i = 1;
        while i < self.recs.len() {
            if cmp_key(
                (self.recs[i - 1].src, self.recs[i - 1].ts),
                (self.recs[i].src, self.recs[i].ts),
            ) != -1
            {
                return Err(E_DUP_RECORD);
            }
            i += 1;
        }
        Ok(())
    }

    /// 同源段内 ts 严格递增（时间轴不倒流）。
    pub fn timeline_ok(&self) -> Result<(), u16> {
        let mut i = 1;
        while i < self.recs.len() {
            if self.recs[i].src == self.recs[i - 1].src && self.recs[i].ts <= self.recs[i - 1].ts {
                return Err(E_TL_BREAK);
            }
            i += 1;
        }
        Ok(())
    }

    pub fn len(&self) -> usize {
        self.recs.len()
    }

    pub fn doubles(&self) -> u32 {
        self.doubles
    }

    pub fn recs(&self) -> &[MergedEvent] {
        &self.recs
    }

    /// 成因账核对：返回（体验独有, 错误独有, 合并）三类计数。
    pub fn cause_counts(&self) -> (u32, u32, u32) {
        let mut e = 0u32;
        let mut r = 0u32;
        let mut b = 0u32;
        let mut i = 0;
        while i < self.recs.len() {
            match self.recs[i].cause {
                CAUSE_EXP => e += 1,
                CAUSE_ERR => r += 1,
                _ => b += 1,
            }
            i += 1;
        }
        (e, r, b)
    }
}

/// 批夹具：批 b 的体验流 + 错误流。
/// 体验流：i ∈ 0..12 → src = 1+(i%5), ts = base+100+i*3。
/// 错误流：k ∈ 0..7 → k<4 重叠键取 i=k*3；k≥4 新键 src=1+(k%5), ts=base+500+k。
pub fn batch_streams(b: u32) -> (Vec<(u32, u64)>, Vec<(u32, u64)>) {
    let base = (b as u64) * 10_000;
    let mut exp: Vec<(u32, u64)> = Vec::new();
    let mut err: Vec<(u32, u64)> = Vec::new();
    let mut i = 0u32;
    while i < EXP_PER {
        exp.push((1 + i % 5, base + 100 + i as u64 * 3));
        i += 1;
    }
    let mut k = 0u32;
    while k < ERR_PER {
        if k < OVERLAP_PER {
            let i = k * 3; // 重叠键 = 体验流第 i 条
            err.push((1 + i % 5, base + 100 + i as u64 * 3));
        } else {
            err.push((1 + k % 5, base + 500 + k as u64));
        }
        k += 1;
    }
    (exp, err)
}

/// 单批合流：两流全量入账，返回账引用。
pub fn merge_batch(b: u32) -> Result<MergeLedger, u16> {
    let (exp, err) = batch_streams(b);
    let mut led = MergeLedger::new();
    let mut i = 0usize;
    while i < exp.len() {
        led.insert(exp[i].0, exp[i].1, CAUSE_EXP)?;
        i += 1;
    }
    let mut k = 0usize;
    while k < err.len() {
        led.insert(err[k].0, err[k].1, CAUSE_ERR)?;
        k += 1;
    }
    Ok(led)
}

/// 自例程判据：逐条落实 J1（合流 ×10 抽查零双记），返回命中组数。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 ×10 批合流条数：每批恰 15 条（12+7-4），双记计数恰 4。
    let mut ledgers: Vec<MergeLedger> = Vec::new();
    let mut b = 0u32;
    while b < BATCHES {
        let led = merge_batch(b)?;
        if led.len() != (EXP_PER + ERR_PER - OVERLAP_PER) as usize {
            return Err(E_COUNT_BAD);
        }
        if led.doubles() != OVERLAP_PER {
            return Err(E_COUNT_BAD);
        }
        ledgers.push(led);
        b += 1;
    }
    hits += 1;

    // R2 零双记形态：×10 批相邻键恒不等（全序账面）。
    let mut b = 0usize;
    while b < ledgers.len() {
        ledgers[b].no_adjacent_dup()?;
        b += 1;
    }
    hits += 1;

    // R3 同源时间轴：×10 批同源段内 ts 严格递增。
    let mut b = 0usize;
    while b < ledgers.len() {
        ledgers[b].timeline_ok()?;
        b += 1;
    }
    hits += 1;

    // R4 成因账：每批恰 8 体验独有 / 3 错误独有 / 4 两流合并。
    let mut b = 0usize;
    while b < ledgers.len() {
        let (e, r, m) = ledgers[b].cause_counts();
        if e != 8 || r != 3 || m != 4 {
            return Err(E_CAUSE_BAD);
        }
        if e + r + m != ledgers[b].len() as u32 {
            return Err(E_CAUSE_BAD);
        }
        b += 1;
    }
    hits += 1;

    // R5 拒重探针：对既有重叠键再插 → Ok(false)、len 不变、双记 +1；
    //    纳新探针：全新键 → Ok(true)、len +1。
    let (_exp0, err0) = batch_streams(0);
    let mut probe = merge_batch(0)?;
    let before = probe.len() as u32;
    let d_before = probe.doubles();
    let dup = probe.insert(err0[0].0, err0[0].1, CAUSE_ERR)?;
    if dup || probe.len() as u32 != before || probe.doubles() != d_before + 1 {
        return Err(E_PROBE_MISS);
    }
    let fresh_ts = (0u64 + 100 + EXP_PER as u64 * 3) + 9_999;
    let fresh = probe.insert(9, fresh_ts, CAUSE_EXP)?;
    if !fresh || probe.len() as u32 != before + 1 {
        return Err(E_PROBE_MISS);
    }
    hits += 1;

    // R6 重叠键成因并集：批 0 的 4 个重叠键在账面上必须 cause=3。
    let mut found_both = 0u32;
    let mut k = 0usize;
    while k < OVERLAP_PER as usize {
        let key = err0[k];
        let mut i = 0usize;
        while i < probe.recs().len() {
            if probe.recs()[i].src == key.0 && probe.recs()[i].ts == key.1 {
                if probe.recs()[i].cause != CAUSE_BOTH {
                    return Err(E_CAUSE_BAD);
                }
                found_both += 1;
                break;
            }
            i += 1;
        }
        k += 1;
    }
    if found_both != OVERLAP_PER {
        return Err(E_CAUSE_BAD);
    }
    hits += 1;

    // R7 同批重建指纹：×10 批各自重建合流账，指纹逐位一致。
    let mut b = 0u32;
    while b < BATCHES {
        let rebuilt = merge_batch(b)?;
        if ledger_fp(rebuilt.recs()) != ledger_fp(ledgers[b as usize].recs()) {
            return Err(E_REPLAY_DRIFT);
        }
        b += 1;
    }
    hits += 1;

    // R8 诊断码闭包：全部错误码落在 [SEG_BASE+1, SEG_BASE+0x1F] 段内。
    const CODES: [u16; 8] = [
        E_DUP_RECORD,
        E_ORDER_BAD,
        E_TL_BREAK,
        E_CAUSE_BAD,
        E_COUNT_BAD,
        E_PROBE_MISS,
        E_REPLAY_DRIFT,
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
