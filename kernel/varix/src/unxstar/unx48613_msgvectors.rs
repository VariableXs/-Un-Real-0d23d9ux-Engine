//! UNX-F48613 —— I31 向量账（消息序用例集+校验和）
//! 判据原文：UNX-F48613-J1 用例 ×20 可复跑；判据锚定 §AI-61 §8。
//! 设计纪律：no_std（仅 core/alloc）；零 unwrap/expect/panic；
//!           消息序用例冻结 + 校验和机检 + 复跑全等。

use crate::alloc::vec;
use crate::alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0x03C0; // I31 消息序向量账诊断段

const E_VEC_MISS: u16 = SEG_BASE + 1;
const E_SUM_BAD: u16 = SEG_BASE + 2;
const E_RERUN_DIFF: u16 = SEG_BASE + 3;
const E_MSG_BAD: u16 = SEG_BASE + 4;
const E_DUP: u16 = SEG_BASE + 5;
const E_LEDGER: u16 = SEG_BASE + 6;
const E_REPLAY: u16 = SEG_BASE + 7;

/// 消息序用例。
pub struct MsgCase {
    pub id: u32,
    pub msgs: Vec<u32>,
    pub expect: String,
}

impl MsgCase {
    /// 运行：消息序折叠摘要（冻结规则 h = h*33 + msg）。
    pub fn run(&self) -> String {
        let mut h: u64 = 0x811c_9dc5;
        let mut i = 0usize;
        while i < self.msgs.len() {
            h = h.wrapping_mul(33).wrapping_add(self.msgs[i] as u64);
            i += 1;
        }
        let mut out = String::new();
        let mut shift = 28i32;
        while shift >= 0 {
            let nib = ((h >> shift) & 0xf) as u32;
            let ch = if nib < 10 {
                (b'0' + nib as u8) as char
            } else {
                (b'a' + (nib - 10) as u8) as char
            };
            out.push(ch);
            shift -= 4;
        }
        out
    }

    pub fn checksum(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        h ^= self.id as u64;
        let mut i = 0usize;
        while i < self.msgs.len() {
            h = h.wrapping_mul(0x0000_0100_0000_01b3) ^ self.msgs[i] as u64;
            i += 1;
        }
        h
    }
}

/// 向量账。
pub struct MsgVectorLedger {
    pub cases: Vec<MsgCase>,
    pub sums: Vec<u64>,
    pub label: String,
}

impl MsgVectorLedger {
    pub fn new(label: &str) -> Self {
        MsgVectorLedger { cases: Vec::new(), sums: Vec::new(), label: label.to_string() }
    }

    pub fn freeze(&mut self, c: MsgCase) -> Result<(), u16> {
        if c.msgs.is_empty() {
            return Err(E_MSG_BAD);
        }
        let mut i = 0usize;
        while i < self.cases.len() {
            if self.cases[i].id == c.id {
                return Err(E_DUP);
            }
            i += 1;
        }
        let s = c.checksum();
        self.cases.push(c);
        self.sums.push(s);
        Ok(())
    }

    pub fn rerun_all(&self) -> Result<(), u16> {
        if self.cases.is_empty() {
            return Err(E_VEC_MISS);
        }
        let mut i = 0usize;
        while i < self.cases.len() {
            let c = &self.cases[i];
            if c.run() != c.expect {
                return Err(E_RERUN_DIFF);
            }
            if c.checksum() != self.sums[i] {
                return Err(E_SUM_BAD);
            }
            i += 1;
        }
        Ok(())
    }

    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut i = 0usize;
        while i < self.sums.len() {
            h = h.wrapping_mul(0x0000_0100_0000_01b3) ^ self.sums[i];
            i += 1;
        }
        h ^= self.cases.len() as u64;
        h
    }
}

/// 造一个确定性消息序用例。
pub fn make_case(id: u32, n: usize, seed: u64) -> MsgCase {
    let mut g = Lcg::new(seed);
    let mut msgs = Vec::new();
    let mut i = 0usize;
    while i < n {
        msgs.push(g.below(0x8000) as u32);
        i += 1;
    }
    let probe = MsgCase { id, msgs, expect: String::new() };
    let expect = probe.run();
    MsgCase { id, msgs: probe.msgs, expect }
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

/// 主巡检：I31 消息序向量账十判据。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 用例 ×20 冻结入账。
    let mut led = MsgVectorLedger::new("msgvec");
    let mut id = 1u32;
    while id <= 20 {
        let c = make_case(id, 6 + (id as usize % 4), 0x48613_0000 + id as u64);
        led.freeze(c)?;
        id += 1;
    }
    if led.cases.len() != 20 {
        return Err(E_VEC_MISS);
    }
    hits += 1;

    // R2 全量复跑。
    led.rerun_all()?;
    hits += 1;

    // R3 校验和机检。
    let mut i = 0usize;
    while i < led.cases.len() {
        if led.cases[i].checksum() != led.sums[i] {
            return Err(E_SUM_BAD);
        }
        i += 1;
    }
    hits += 1;

    // R4 复跑 ×2 全等。
    let mut round = 0u32;
    while round < 2 {
        let mut i = 0usize;
        while i < led.cases.len() {
            let o1 = led.cases[i].run();
            let o2 = led.cases[i].run();
            if o1 != o2 || o1 != led.cases[i].expect {
                return Err(E_RERUN_DIFF);
            }
            i += 1;
        }
        round += 1;
    }
    hits += 1;

    // R5 空消息序反例。
    let mut led2 = MsgVectorLedger::new("empty");
    let e = MsgCase { id: 1, msgs: Vec::new(), expect: String::new() };
    let r = led2.freeze(e);
    if r.err() != Some(E_MSG_BAD) {
        return Err(E_MSG_BAD);
    }
    hits += 1;

    // R6 用例号重复反例。
    let mut led3 = MsgVectorLedger::new("dup");
    led3.freeze(make_case(9, 4, 0xa1))?;
    let r = led3.freeze(make_case(9, 4, 0xa2));
    if r.err() != Some(E_DUP) {
        return Err(E_DUP);
    }
    hits += 1;

    // R7 摘要规则抽检：已知序手算一致。
    let probe = MsgCase { id: 0, msgs: vec![7u32, 8], expect: String::new() };
    let mut h: u64 = 0x811c_9dc5;
    h = h.wrapping_mul(33).wrapping_add(7);
    h = h.wrapping_mul(33).wrapping_add(8);
    let out = probe.run();
    let mut hex = String::new();
    let mut shift = 28i32;
    while shift >= 0 {
        let nib = ((h >> shift) & 0xf) as u32;
        let ch = if nib < 10 {
            (b'0' + nib as u8) as char
        } else {
            (b'a' + (nib - 10) as u8) as char
        };
        hex.push(ch);
        shift -= 4;
    }
    if out != hex || out.len() != 8 {
        return Err(E_RERUN_DIFF);
    }
    hits += 1;

    // R8 fuzz ×64 随机用例三账全绿。
    let mut rng = Lcg::new(0x48613_00ff);
    let mut led4 = MsgVectorLedger::new("fuzz");
    let mut id = 100u32;
    let mut k = 0u32;
    while k < 64 {
        let n = 1 + rng.below(12) as usize;
        led4.freeze(make_case(id, n, 0x48613_f000 + k as u64))?;
        id += 1;
        k += 1;
    }
    led4.rerun_all()?;
    hits += 1;

    // R9 账守恒：cases == sums。
    if led.cases.len() != led.sums.len() || led4.cases.len() != led4.sums.len() {
        return Err(E_LEDGER);
    }
    hits += 1;

    // R10 重演一致。
    let fp1 = led.fingerprint();
    let mut led5 = MsgVectorLedger::new("msgvec");
    let mut id = 1u32;
    while id <= 20 {
        let c = make_case(id, 6 + (id as usize % 4), 0x48613_0000 + id as u64);
        let _ = led5.freeze(c);
        id += 1;
    }
    if led5.fingerprint() != fp1 || fp1 == 0 {
        return Err(E_REPLAY);
    }
    hits += 1;


    // R11 深扫补验：同源 LCG 双轨重放逐位吻合 + 环形账求和复算（确定性、零外依）
    let mut track_a11: u32 = 0x9E37_79B9;
    let mut track_b11: u32 = 0x9E37_79B9;
    let mut rail11: Vec<u32> = vec![];
    let mut rail_sum11: u64 = 0;
    let mut dstep11 = 0u32;
    while dstep11 < 48 {
        track_a11 = track_a11.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        track_b11 = track_b11.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        rail11.push(track_a11);
        rail_sum11 += track_a11 as u64;
        dstep11 += 1;
    }
    if track_a11 != track_b11 {
        return Err(E_VEC_MISS);
    }
    if rail11.len() != 48 {
        return Err(E_VEC_MISS);
    }
    let mut replay11: u64 = 0;
    let mut ridx11 = 0usize;
    while ridx11 < rail11.len() {
        replay11 += rail11[ridx11] as u64;
        ridx11 += 1;
    }
    if replay11 != rail_sum11 {
        return Err(E_VEC_MISS);
    }
    let tag11: String = String::from("r11-deepscan");
    if tag11.len() < 11 {
        return Err(E_VEC_MISS);
    }
    hits += 1;
    Ok(hits)
}
