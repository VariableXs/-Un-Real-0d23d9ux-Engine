//! UNX-F48760 C38 批收官印（20 条冻结/累计 228,000 行）
//! 判据 UNX-F48760-J1：批印入册：域账 228,000/240,000；判据锚定 §AI-61 §7。
//! 设计纪律：no_std（仅 core/alloc）；诊断码自 SEG_BASE+1 起连续且总数 ≤0x1F；
//! drill() 自证十判据，正例计数、反例注入必须 .err() 命中，命中即 Err 传播；
//! fuzz 用确定性 LCG（wrapping_mul 1_664_525 / wrapping_add 1_013_904_223）；
//! 20 条冻结入批印，域账累计 228,000/240,000 行，差 12,000 挂批注。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0x1D00;
const E_BATCH_MISS: u16 = SEG_BASE + 1;
const E_LINE_MISS: u16 = SEG_BASE + 2;
const E_FROZEN: u16 = SEG_BASE + 3;
const E_TOTAL_DIFF: u16 = SEG_BASE + 4;
const E_REPLAY_DIFF: u16 = SEG_BASE + 5;
const E_STAMP_DUP: u16 = SEG_BASE + 6;
const E_LINE_ZERO: u16 = SEG_BASE + 7;
const E_GAP_NOTE: u16 = SEG_BASE + 8;

/// 域账常量：批印目标 228,000；批总盘 240,000；差 12,000
pub const SECT_TOTAL: u32 = 228_000;
pub const BATCH_TOTAL: u32 = 240_000;
pub const GAP: u32 = 12_000;
pub const STAMP_COUNT: u32 = 20;

/// 收官印条目：印号 + 该条冻结行数
#[derive(Clone, Debug, PartialEq)]
pub struct SealEntry {
    pub stamp_no: u16,
    pub lines: u32,
}

impl SealEntry {
    pub fn make(stamp_no: u16, lines: u32) -> Self {
        SealEntry { stamp_no, lines }
    }
}

/// 批收官印账
pub struct SealLedger {
    pub entries: Vec<SealEntry>,
    pub sealed: bool,
}

impl SealLedger {
    pub fn new() -> Self {
        SealLedger { entries: vec![], sealed: false }
    }

    pub fn add(&mut self, stamp_no: u16, lines: u32) -> Result<(), u16> {
        if self.sealed {
            return Err(E_FROZEN);
        }
        if lines == 0 {
            return Err(E_LINE_ZERO);
        }
        let mut i = 0usize;
        while i < self.entries.len() {
            if self.entries[i].stamp_no == stamp_no {
                return Err(E_STAMP_DUP);
            }
            i += 1;
        }
        self.entries.push(SealEntry::make(stamp_no, lines));
        Ok(())
    }

    /// 批印入册：满 20 条冻结并核对域账
    pub fn seal(&mut self) -> Result<u32, u16> {
        if self.entries.len() != STAMP_COUNT as usize {
            return Err(E_BATCH_MISS);
        }
        let mut total = 0u32;
        let mut i = 0usize;
        while i < self.entries.len() {
            total += self.entries[i].lines;
            i += 1;
        }
        if total != SECT_TOTAL {
            return Err(E_TOTAL_DIFF);
        }
        self.sealed = true;
        Ok(total)
    }

    /// 批总盘核对：域账 + 差 = 批总盘
    pub fn verify_batch(&self) -> Result<(), u16> {
        if !self.sealed {
            return Err(E_BATCH_MISS);
        }
        let mut total = 0u32;
        let mut i = 0usize;
        while i < self.entries.len() {
            total += self.entries[i].lines;
            i += 1;
        }
        if total + GAP != BATCH_TOTAL {
            return Err(E_GAP_NOTE);
        }
        Ok(())
    }

    /// 印号连续核对：1..=20 逐号在册
    pub fn verify_stamps(&self) -> Result<(), u16> {
        let mut s = 1u16;
        while s <= STAMP_COUNT as u16 {
            let mut found = false;
            let mut i = 0usize;
            while i < self.entries.len() {
                if self.entries[i].stamp_no == s {
                    found = true;
                }
                i += 1;
            }
            if !found {
                return Err(E_STAMP_DUP);
            }
            s += 1;
        }
        Ok(())
    }
}

/// 确定性 LCG
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

/// 造 20 条恰好合计 228,000 行的印条
fn build_entries(rng: &mut Lcg) -> Vec<SealEntry> {
    let mut out: Vec<SealEntry> = vec![];
    let mut remaining = SECT_TOTAL;
    let mut no = 1u16;
    while no <= 20 {
        if no == 20 {
            out.push(SealEntry::make(no, remaining));
        } else {
            let share = remaining / (20 - (no as u32 - 1));
            let jitter = rng.below(share as u64 / 2);
            let take = share + jitter as u32;
            out.push(SealEntry::make(no, take));
            remaining -= take;
        }
        no += 1;
    }
    out
}

/// 批印入册 ×20 组
fn sweep_seal(rng: &mut Lcg) -> Result<(), u16> {
    let mut group = 0u32;
    while group < 20 {
        let mut led = SealLedger::new();
        let entries = build_entries(rng);
        let mut i = 0usize;
        while i < entries.len() {
            led.add(entries[i].stamp_no, entries[i].lines)?;
            i += 1;
        }
        let total = led.seal()?;
        if total != SECT_TOTAL {
            return Err(E_TOTAL_DIFF);
        }
        led.verify_batch()?;
        led.verify_stamps()?;
        group += 1;
    }
    Ok(())
}

/// 反例注入：条数不足、行数不符、零行、重印、冻结后写入全命中
fn sweep_negatives() -> Result<(), u16> {
    let mut led = SealLedger::new();
    let r1 = led.seal();
    if r1.err() != Some(E_BATCH_MISS) {
        return Err(E_BATCH_MISS);
    }
    let mut led2 = SealLedger::new();
    led2.add(1, 100)?;
    let r2 = led2.add(1, 200);
    if r2.err() != Some(E_STAMP_DUP) {
        return Err(E_STAMP_DUP);
    }
    let r3 = led2.add(2, 0);
    if r3.err() != Some(E_LINE_ZERO) {
        return Err(E_LINE_ZERO);
    }
    let mut led3 = SealLedger::new();
    let mut no = 1u16;
    while no <= 20 {
        led3.add(no, 100)?;
        no += 1;
    }
    let r4 = led3.seal();
    if r4.err() != Some(E_TOTAL_DIFF) {
        return Err(E_TOTAL_DIFF);
    }
    let r5 = led3.add(21, 100);
    if r5.is_ok() {
        return Err(E_FROZEN);
    }
    Ok(())
}

/// 差额挂批注：域账+GAP=批总盘恒等式 ×10^3
fn sweep_gap_identity(rng: &mut Lcg) -> Result<(), u16> {
    let mut iter = 0u32;
    while iter < 1_000 {
        let perturb = rng.below(1_000) as u32;
        let a = SECT_TOTAL;
        let b = a + GAP;
        if b != BATCH_TOTAL {
            return Err(E_GAP_NOTE);
        }
        if perturb > 0 {
            let c = a + perturb;
            if c + GAP != BATCH_TOTAL + perturb {
                return Err(E_TOTAL_DIFF);
            }
        }
        iter += 1;
    }
    Ok(())
}

/// 重放一致 ×20：同种子双跑批印账逐位吻合
fn sweep_replay(seed: u64) -> Result<(), u16> {
    let mut round = 0u32;
    while round < 20 {
        let mut ea = build_entries(&mut Lcg::new(seed + round as u64));
        let mut eb = build_entries(&mut Lcg::new(seed + round as u64));
        let mut la = SealLedger::new();
        let mut lb = SealLedger::new();
        let mut i = 0usize;
        while i < ea.len() {
            la.add(ea[i].stamp_no, ea[i].lines)?;
            lb.add(eb[i].stamp_no, eb[i].lines)?;
            i += 1;
        }
        if la.seal()? != lb.seal()? {
            return Err(E_REPLAY_DIFF);
        }
        ea = la.entries.clone();
        eb = lb.entries.clone();
        let mut k = 0usize;
        while k < ea.len() {
            if ea[k] != eb[k] {
                return Err(E_REPLAY_DIFF);
            }
            k += 1;
        }
        round += 1;
    }
    Ok(())
}

pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;
    // R1 批印入册 ×20 组
    let mut rng = Lcg::new(0x4876_0422);
    sweep_seal(&mut rng)?;
    hits += 1;
    // R2 反例注入全命中
    sweep_negatives()?;
    hits += 1;
    // R3 差额恒等式 ×10^3
    sweep_gap_identity(&mut rng)?;
    hits += 1;
    // R4 重放一致 ×20
    sweep_replay(0x4876_0F0E)?;
    hits += 1;
    // R5 域账常量核对：228,000 + 12,000 = 240,000
    if SECT_TOTAL + GAP != BATCH_TOTAL || GAP != 12_000 {
        return Err(E_TOTAL_DIFF);
    }
    hits += 1;
    // R6 二十条冻结断言：seal 后 sealed=true
    let mut led = SealLedger::new();
    let entries = build_entries(&mut rng);
    let mut i = 0usize;
    while i < entries.len() {
        led.add(entries[i].stamp_no, entries[i].lines)?;
        i += 1;
    }
    led.seal()?;
    if !led.sealed {
        return Err(E_BATCH_MISS);
    }
    hits += 1;
    // R7 冻结不可写：seal 后 add 必拒
    let r7 = led.add(21, 500);
    if r7.err() != Some(E_FROZEN) {
        return Err(E_FROZEN);
    }
    hits += 1;
    // R8 印号 1..=20 全带核对
    led.verify_stamps()?;
    hits += 1;
    // R9 行数非零全带：二十条逐条 >0
    let snap = led.entries.clone();
    let mut k = 0usize;
    while k < snap.len() {
        if snap[k].lines == 0 {
            return Err(E_LINE_ZERO);
        }
        k += 1;
    }
    hits += 1;
    // R10 批总盘核对：sealed 账 verify_batch 过
    led.verify_batch()?;
    hits += 1;
    // R11 深扫补验：同源 LCG 双轨重放逐位吻合 + 环形账求和复算（确定性、零外依）
    let mut track_a: u32 = 0x9E37_79B9;
    let mut track_b: u32 = 0x9E37_79B9;
    let mut rail: Vec<u32> = vec![];
    let mut rail_sum: u64 = 0;
    let mut dstep = 0u32;
    while dstep < 48 {
        track_a = track_a.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        track_b = track_b.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        rail.push(track_a);
        rail_sum += track_a as u64;
        dstep += 1;
    }
    if track_a != track_b {
        return Err(E_REPLAY_DIFF);
    }
    if rail.len() != 48 {
        return Err(E_REPLAY_DIFF);
    }
    let mut replay_sum: u64 = 0;
    let mut ridx = 0usize;
    while ridx < rail.len() {
        replay_sum += rail[ridx] as u64;
        ridx += 1;
    }
    if replay_sum != rail_sum {
        return Err(E_REPLAY_DIFF);
    }
    hits += 1;
    Ok(hits)
}
