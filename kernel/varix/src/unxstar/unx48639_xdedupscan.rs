//! UNX-F48639 —— I32 批防重 grep（关键词族：ximsign/xbridge）
//! 判据原文：UNX-F48639-J1 批收口防重留痕；判据锚定 §6.1。
//! 设计纪律：no_std（仅 core/alloc）；零 unwrap/expect/panic；
//!           关键词族留痕 + 撞号机检 + 收口封账。

use crate::alloc::vec;
use crate::alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0x0700; // I32 批防重 grep 诊断段

const E_DUP: u16 = SEG_BASE + 1;
const E_KEY_BAD: u16 = SEG_BASE + 2;
const E_TRACE_MISS: u16 = SEG_BASE + 3;
const E_FROZEN_BAD: u16 = SEG_BASE + 4;
const E_LEDGER: u16 = SEG_BASE + 5;
const E_REPLAY: u16 = SEG_BASE + 6;
const E_RANGE_BAD: u16 = SEG_BASE + 7;

/// 关键词族（冻结）。
pub const KEY_XIMSIGN: &str = "ximsign";
pub const KEY_XBRIDGE: &str = "xbridge";

/// 留痕条目。
pub struct TraceItem {
    pub keyword: String,
    pub card: u32,
    pub hits: u32,
}

impl TraceItem {
    pub fn keyword_ok(&self) -> Result<(), u16> {
        if self.keyword.len() == 0 {
            return Err(E_KEY_BAD);
        }
        let b = self.keyword.as_bytes();
        let mut i = 0usize;
        while i < b.len() {
            if !(b[i] >= b'a' && b[i] <= b'z') {
                return Err(E_KEY_BAD);
            }
            i += 1;
        }
        Ok(())
    }
}

/// 防重留痕账。
pub struct DedupLedger {
    pub items: Vec<TraceItem>,
    pub sealed: bool,
    pub label: String,
}

impl DedupLedger {
    pub fn new(label: &str) -> Self {
        DedupLedger { items: Vec::new(), sealed: false, label: label.to_string() }
    }

    pub fn trace(&mut self, keyword: &str, card: u32, hits: u32) -> Result<(), u16> {
        if self.sealed {
            return Err(E_FROZEN_BAD);
        }
        if card == 0 || card > 0xffffff {
            return Err(E_RANGE_BAD);
        }
        let it = TraceItem { keyword: keyword.to_string(), card, hits };
        it.keyword_ok()?;
        let mut i = 0usize;
        while i < self.items.len() {
            if self.items[i].card == card {
                return Err(E_DUP);
            }
            i += 1;
        }
        self.items.push(it);
        Ok(())
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }

    pub fn machine_check(&self) -> Result<(), u16> {
        if self.items.is_empty() {
            return Err(E_TRACE_MISS);
        }
        let mut i = 0usize;
        while i < self.items.len() {
            let kw = &self.items[i].keyword;
            if kw != KEY_XIMSIGN && kw != KEY_XBRIDGE {
                return Err(E_KEY_BAD);
            }
            let mut j = i + 1;
            while j < self.items.len() {
                if self.items[i].card == self.items[j].card {
                    return Err(E_DUP);
                }
                j += 1;
            }
            i += 1;
        }
        if !self.sealed {
            return Err(E_TRACE_MISS);
        }
        Ok(())
    }

    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        h ^= self.sealed as u64;
        let mut i = 0usize;
        while i < self.items.len() {
            h = h.wrapping_mul(0x0000_0100_0000_01b3) ^ self.items[i].card as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3) ^ self.items[i].hits as u64;
            let kb = self.items[i].keyword.as_bytes();
            let mut j = 0usize;
            while j < kb.len() {
                h = h.wrapping_mul(0x0000_0100_0000_01b3) ^ kb[j] as u64;
                j += 1;
            }
            i += 1;
        }
        h
    }
}

/// 文本关键词命中计数。
pub fn grep_keyword(text: &str, keyword: &str) -> u32 {
    let tb = text.as_bytes();
    let kb = keyword.as_bytes();
    if kb.len() == 0 || tb.len() < kb.len() {
        return 0;
    }
    let mut n = 0u32;
    let mut i = 0usize;
    while i + kb.len() <= tb.len() {
        let mut m = true;
        let mut j = 0usize;
        while j < kb.len() {
            if tb[i + j] != kb[j] {
                m = false;
            }
            j += 1;
        }
        if m {
            n += 1;
        }
        i += 1;
    }
    n
}

/// 确定性 LCG。
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

/// 主巡检：I32 批防重 grep 十判据。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 关键词命中基线。
    let text = "xbridge routes ximsign trails across xbridge spans";
    let h1 = grep_keyword(text, KEY_XIMSIGN);
    let h2 = grep_keyword(text, KEY_XBRIDGE);
    if h1 != 1 || h2 != 2 {
        return Err(E_TRACE_MISS);
    }
    hits += 1;

    // R2 留痕 ×10。
    let mut led = DedupLedger::new("batch-i32");
    let mut card = 48621u32;
    let mut i = 0u32;
    while i < 10 {
        let kw = if i % 2 == 0 { KEY_XIMSIGN } else { KEY_XBRIDGE };
        led.trace(kw, card + i, 1 + i)?;
        i += 1;
    }
    if led.items.len() != 10 {
        return Err(E_TRACE_MISS);
    }
    hits += 1;

    // R3 撞号反例。
    let r = led.trace(KEY_XIMSIGN, card, 3);
    if r.err() != Some(E_DUP) {
        return Err(E_DUP);
    }
    hits += 1;

    // R4 非法关键词反例。
    let mut led2 = DedupLedger::new("badkw");
    let r = led2.trace("", 1, 1);
    if r.err() != Some(E_KEY_BAD) {
        return Err(E_KEY_BAD);
    }
    let r = led2.trace("Ximsign", 1, 1);
    if r.err() != Some(E_KEY_BAD) {
        return Err(E_KEY_BAD);
    }
    hits += 1;

    // R5 卡号越界反例。
    let mut led3 = DedupLedger::new("badcard");
    let r = led3.trace(KEY_XIMSIGN, 0, 1);
    if r.err() != Some(E_RANGE_BAD) {
        return Err(E_RANGE_BAD);
    }
    let r = led3.trace(KEY_XIMSIGN, 0x100_0000, 1);
    if r.err() != Some(E_RANGE_BAD) {
        return Err(E_RANGE_BAD);
    }
    hits += 1;

    // R6 收口封账 + 机检。
    led.seal();
    led.machine_check()?;
    hits += 1;

    // R7 收口后写反例。
    let r = led.trace(KEY_XBRIDGE, card + 55, 1);
    if r.err() != Some(E_FROZEN_BAD) {
        return Err(E_FROZEN_BAD);
    }
    hits += 1;

    // R8 未收口机检反例。
    let led4 = DedupLedger::new("unsealed");
    let r = led4.machine_check();
    if r.err() != Some(E_TRACE_MISS) {
        return Err(E_TRACE_MISS);
    }
    hits += 1;

    // R9 fuzz ×128 随机卡号留痕：撞号即拒账守恒。
    let mut rng = Lcg::new(0x48639_0001);
    let mut led5 = DedupLedger::new("fuzz");
    let mut added = 0u32;
    let mut k = 0u32;
    while k < 128 {
        let c = 500 + rng.below(200) as u32;
        let kw = if rng.below(2) == 0 { KEY_XIMSIGN } else { KEY_XBRIDGE };
        let r = led5.trace(kw, c, 1);
        if r.is_ok() {
            added += 1;
        } else if r.err() != Some(E_DUP) {
            return Err(E_KEY_BAD);
        }
        k += 1;
    }
    if led5.items.len() as u32 != added {
        return Err(E_LEDGER);
    }
    led5.seal();
    led5.machine_check()?;
    hits += 1;

    // R10 重演一致。
    let fp1 = led5.fingerprint();
    let mut rng2 = Lcg::new(0x48639_0001);
    let mut led6 = DedupLedger::new("fuzz");
    let mut k = 0u32;
    while k < 128 {
        let c = 500 + rng2.below(200) as u32;
        let kw = if rng2.below(2) == 0 { KEY_XIMSIGN } else { KEY_XBRIDGE };
        let _ = led6.trace(kw, c, 1);
        k += 1;
    }
    led6.seal();
    if led6.fingerprint() != fp1 || fp1 == 0 {
        return Err(E_REPLAY);
    }
    hits += 1;

    Ok(hits)
}
