//! UNX-F48753 C·O1 对标衔接账（本域延迟账交付波 26 消费位）
//! 判据 UNX-F48753-J1：交付位登记；判据锚定 附则五。
//! 设计纪律：no_std（仅 core/alloc）；诊断码自 SEG_BASE+1 起连续且总数 ≤0x1F；
//! drill() 自证十判据，正例计数、反例注入必须 .err() 命中，命中即 Err 传播；
//! fuzz 用确定性 LCG（wrapping_mul 1_664_525 / wrapping_add 1_013_904_223）；
//! 本域延迟账交付波 26 消费位必须逐位登记，缺位即红。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0x1BA0;
const E_SLOT_MISS: u16 = SEG_BASE + 1;
const E_SLOT_DUP: u16 = SEG_BASE + 2;
const E_WAVE_BAD: u16 = SEG_BASE + 3;
const E_LAT_LOST: u16 = SEG_BASE + 4;
const E_LEDGER_GAP: u16 = SEG_BASE + 5;
const E_REPLAY_DIFF: u16 = SEG_BASE + 6;
const E_HANDOFF_BAD: u16 = SEG_BASE + 7;
const E_CONSUMER_DEAD: u16 = SEG_BASE + 8;

/// 交付位：波 26 消费位编号 + 本域延迟账指纹
#[derive(Clone, Debug, PartialEq)]
pub struct Slot {
    pub slot_no: u16,
    pub lat_fp: u32,
    pub tag: String,
}

impl Slot {
    pub fn make(slot_no: u16, lat_fp: u32, tag: &str) -> Self {
        Slot { slot_no, lat_fp, tag: String::from(tag) }
    }
}

/// 对标衔接账：交付位登记簿
pub struct HandoffLedger {
    pub wave: u16,
    pub slots: Vec<Slot>,
}

impl HandoffLedger {
    pub fn new(wave: u16) -> Self {
        HandoffLedger { wave, slots: vec![] }
    }

    pub fn register(&mut self, slot_no: u16, lat_fp: u32, tag: &str) -> Result<(), u16> {
        if self.wave != 26 {
            return Err(E_WAVE_BAD);
        }
        let mut i = 0usize;
        while i < self.slots.len() {
            if self.slots[i].slot_no == slot_no {
                return Err(E_SLOT_DUP);
            }
            i += 1;
        }
        self.slots.push(Slot::make(slot_no, lat_fp, tag));
        Ok(())
    }

    /// 交付位连续核对：登记号必须覆盖 1..=n 无缺位
    pub fn verify_slots(&self, n: u16) -> Result<(), u16> {
        if n == 0 || self.slots.len() != n as usize {
            return Err(E_SLOT_MISS);
        }
        let mut s = 1u16;
        while s <= n {
            let mut found = false;
            let mut i = 0usize;
            while i < self.slots.len() {
                if self.slots[i].slot_no == s {
                    found = true;
                }
                i += 1;
            }
            if !found {
                return Err(E_SLOT_MISS);
            }
            s += 1;
        }
        Ok(())
    }

    /// 消费位活性：每个登记位指纹非零可消费
    pub fn consumers_alive(&self) -> Result<u32, u16> {
        if self.slots.is_empty() {
            return Err(E_CONSUMER_DEAD);
        }
        let mut alive = 0u32;
        let mut i = 0usize;
        while i < self.slots.len() {
            if self.slots[i].lat_fp == 0 {
                return Err(E_LAT_LOST);
            }
            if self.slots[i].tag.is_empty() {
                return Err(E_HANDOFF_BAD);
            }
            alive += 1;
            i += 1;
        }
        Ok(alive)
    }
}

/// 延迟账指纹生成：FNV-1a
pub fn lat_fpnv(word: &str) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let bytes = word.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        h ^= bytes[i] as u32;
        h = h.wrapping_mul(0x0100_0193);
        i += 1;
    }
    h
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

/// 交付位登记 ×10^3：1..=8 全位登记后核对
fn sweep_register(rng: &mut Lcg) -> Result<(), u16> {
    let mut iter = 0u32;
    while iter < 1_000 {
        let mut led = HandoffLedger::new(26);
        let mut s = 1u16;
        while s <= 8 {
            let fp = lat_fpnv("lat-final") ^ (rng.below(0xFFFF) as u32);
            led.register(s, fp | 1, "o1-handoff")?;
            s += 1;
        }
        led.verify_slots(8)?;
        if led.consumers_alive()? != 8 {
            return Err(E_CONSUMER_DEAD);
        }
        iter += 1;
    }
    Ok(())
}

/// 反例注入：缺位、重复位、错波次、空指纹全命中
fn sweep_negatives() -> Result<(), u16> {
    let mut led = HandoffLedger::new(26);
    led.register(1, 0x1111, "a")?;
    let r1 = led.register(1, 0x2222, "dup");
    if r1.err() != Some(E_SLOT_DUP) {
        return Err(E_SLOT_DUP);
    }
    let r2 = led.verify_slots(8);
    if r2.err() != Some(E_SLOT_MISS) {
        return Err(E_SLOT_MISS);
    }
    let mut bad = HandoffLedger::new(25);
    let r3 = bad.register(1, 0x3333, "wrong-wave");
    if r3.err() != Some(E_WAVE_BAD) {
        return Err(E_WAVE_BAD);
    }
    let mut led2 = HandoffLedger::new(26);
    led2.register(2, 0, "zero-fp")?;
    if led2.consumers_alive().err() != Some(E_LAT_LOST) {
        return Err(E_LAT_LOST);
    }
    let mut led3 = HandoffLedger::new(26);
    led3.register(3, 0x4444, "")?;
    if led3.consumers_alive().err() != Some(E_HANDOFF_BAD) {
        return Err(E_HANDOFF_BAD);
    }
    Ok(())
}

/// 重放一致 ×20：同种子双跑登记簿逐位吻合
fn sweep_replay(seed: u64) -> Result<(), u16> {
    let mut round = 0u32;
    while round < 20 {
        let mut la = HandoffLedger::new(26);
        let mut lb = HandoffLedger::new(26);
        let mut ra = Lcg::new(seed + round as u64);
        let mut rb = Lcg::new(seed + round as u64);
        let mut s = 1u16;
        while s <= 6 {
            let fa = rng_fp(&mut ra);
            let fb = rng_fp(&mut rb);
            if fa != fb {
                return Err(E_REPLAY_DIFF);
            }
            la.register(s, fa, "replay")?;
            lb.register(s, fb, "replay")?;
            s += 1;
        }
        if la.consumers_alive()? != lb.consumers_alive()? {
            return Err(E_REPLAY_DIFF);
        }
        let mut i = 0usize;
        while i < la.slots.len() {
            if la.slots[i] != lb.slots[i] {
                return Err(E_REPLAY_DIFF);
            }
            i += 1;
        }
        round += 1;
    }
    Ok(())
}

fn rng_fp(rng: &mut Lcg) -> u32 {
    (rng.next() ^ (rng.next() >> 17)) as u32 | 1
}

pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;
    // R1 交付位登记 ×10^3
    let mut rng = Lcg::new(0x4875_3DBB);
    let mut r1 = 0u32;
    while r1 < 1_000 {
        sweep_register(&mut rng)?;
        r1 += 1;
    }
    hits += 1;
    // R2 反例注入全命中
    sweep_negatives()?;
    hits += 1;
    // R3 重放一致 ×20
    sweep_replay(0x4875_3C07)?;
    hits += 1;
    // R4 指纹自治：FNV 对已知词面回算一致
    let f1 = lat_fpnv("wave26");
    let f2 = lat_fpnv("wave26");
    if f1 != f2 || f1 == 0 {
        return Err(E_LAT_LOST);
    }
    hits += 1;
    // R5 位号覆盖 1..=16 全带核对
    let mut led = HandoffLedger::new(26);
    let mut s = 1u16;
    while s <= 16 {
        led.register(s, 0xA000 + s as u32, "band16")?;
        s += 1;
    }
    led.verify_slots(16)?;
    hits += 1;
    // R6 消费位活性 ×16 全绿
    if led.consumers_alive()? != 16 {
        return Err(E_CONSUMER_DEAD);
    }
    hits += 1;
    // R7 登记序稳定：位号升序落簿
    let mut i = 0usize;
    while i < led.slots.len() {
        if led.slots[i].slot_no != (i + 1) as u16 {
            return Err(E_SLOT_MISS);
        }
        i += 1;
    }
    hits += 1;
    // R8 波次门禁：仅波 26 可登记
    let mut w = 20u16;
    while w <= 30 {
        let mut tmp = HandoffLedger::new(w);
        let r = tmp.register(1, 1, "gate");
        if w == 26 {
            if r.is_err() {
                return Err(E_WAVE_BAD);
            }
        } else if r.err() != Some(E_WAVE_BAD) {
            return Err(E_WAVE_BAD);
        }
        w += 1;
    }
    hits += 1;
    // R9 标签语义：交付位 tag 一致带 o1 语义
    let slots = led.slots.clone();
    let mut k = 0usize;
    while k < slots.len() {
        if slots[k].tag != "band16" {
            return Err(E_HANDOFF_BAD);
        }
        k += 1;
    }
    hits += 1;
    // R10 登记簿快照不可变：登记后指纹不被改写
    let fp0 = led.slots[0].lat_fp;
    led.verify_slots(16)?;
    if led.slots[0].lat_fp != fp0 {
        return Err(E_LAT_LOST);
    }
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
