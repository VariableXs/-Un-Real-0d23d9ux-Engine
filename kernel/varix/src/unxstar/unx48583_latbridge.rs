//! UNX-F48583 —— I·M2 延迟账分段对接
//!         （正：M2 采集段→本域桥段时戳衔接零断档）
//! 判据原文：UNX-F48583-J1 衔接 ×10^4 零断档；判据锚定 附则五 延迟账。
//! 设计纪律：no_std（仅 core/alloc）；零 unwrap/expect/panic；
//!           确定性 LCG fuzz；drill 内 R1..Rn 自证判据，返回 Ok(hits)。

use crate::alloc::vec;
use crate::alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0x0000; // M2 延迟账分段对接诊断段

const E_ORDER: u16 = SEG_BASE + 1; // 时戳次序违例（桥段早于采集）
const E_DELTA: u16 = SEG_BASE + 2; // 衔接超窗
const E_GAP: u16 = SEG_BASE + 3;   // 段内断档
const E_LOSS: u16 = SEG_BASE + 4;  // 序列号跳号（事件丢失）
const E_DUP: u16 = SEG_BASE + 5;   // 序列号重复
const E_LEDGER: u16 = SEG_BASE + 6; // 账目守恒失配
const E_REPLAY: u16 = SEG_BASE + 7; // 重演指纹不一致

/// 单事件时戳对：m2 = M2 采集时刻，br = 本域桥段入账时刻。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct TsPair {
    pub m2: u64,
    pub br: u64,
}

impl TsPair {
    /// 衔接延迟 = 桥段入账 − 采集时刻。
    pub fn delta(&self) -> u64 {
        self.br.wrapping_sub(self.m2)
    }
}

/// M2 采集段 → 本域桥段延迟账（单段）。
pub struct LatLedger {
    pub label: String,
    pub pairs: Vec<TsPair>,
    /// 单事件最大允许衔接窗（桥段入账 − 采集）。
    pub cap_delta: u64,
    /// 相邻采集事件允许最大时戳间隔；0 表示不检查间隔。
    pub cap_gap: u64,
}

impl LatLedger {
    pub fn new(label: &str, cap_delta: u64, cap_gap: u64) -> Self {
        LatLedger {
            label: label.to_string(),
            pairs: Vec::new(),
            cap_delta,
            cap_gap,
        }
    }

    /// 采集→桥段单事件入账：先校验次序与衔接窗，再落账。
    pub fn feed(&mut self, m2: u64, br: u64) -> Result<(), u16> {
        if br < m2 {
            return Err(E_ORDER);
        }
        if br.wrapping_sub(m2) > self.cap_delta {
            return Err(E_DELTA);
        }
        if self.pairs.len() > 0 {
            let last = self.pairs[self.pairs.len() - 1];
            if m2 < last.m2 {
                return Err(E_ORDER);
            }
            if self.cap_gap > 0 && m2.wrapping_sub(last.m2) > self.cap_gap {
                return Err(E_GAP);
            }
        }
        self.pairs.push(TsPair { m2, br });
        Ok(())
    }

    /// 全账审计：逐对重验次序与衔接窗，段内零断档。
    pub fn audit(&self) -> Result<(), u16> {
        let mut i = 0usize;
        while i < self.pairs.len() {
            let p = self.pairs[i];
            if p.br < p.m2 || p.delta() > self.cap_delta {
                return Err(E_DELTA);
            }
            if i > 0 {
                let q = self.pairs[i - 1];
                if p.m2 < q.m2 || p.br < q.br {
                    return Err(E_ORDER);
                }
                if self.cap_gap > 0 && p.m2.wrapping_sub(q.m2) > self.cap_gap {
                    return Err(E_GAP);
                }
            }
            i += 1;
        }
        Ok(())
    }

    /// 段内事件数。
    pub fn count(&self) -> u32 {
        self.pairs.len() as u32
    }

    /// 全账最大衔接延迟。
    pub fn max_delta(&self) -> u64 {
        let mut m: u64 = 0;
        let mut i = 0usize;
        while i < self.pairs.len() {
            let d = self.pairs[i].delta();
            if d > m {
                m = d;
            }
            i += 1;
        }
        m
    }

    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut i = 0usize;
        while i < self.pairs.len() {
            h = h.wrapping_mul(0x0000_0100_0000_01b3) ^ self.pairs[i].m2;
            h = h.wrapping_mul(0x0000_0100_0000_01b3) ^ self.pairs[i].br;
            i += 1;
        }
        h ^= self.pairs.len() as u64;
        h
    }
}

/// 桥段链：序列号连续（零丢零重）、桥段时戳单调不减。
pub struct BridgeChain {
    pub nodes: Vec<(u64, u64)>, // (seq, br_ts)
    pub expect_seq: u64,
}

impl BridgeChain {
    pub fn new() -> Self {
        BridgeChain { nodes: Vec::new(), expect_seq: 0 }
    }

    /// 链上挂接一节：seq 必须等于期望序，时戳必须单调。
    pub fn link(&mut self, seq: u64, ts: u64) -> Result<(), u16> {
        if seq > self.expect_seq {
            return Err(E_LOSS);
        }
        if seq < self.expect_seq {
            return Err(E_DUP);
        }
        if self.nodes.len() > 0 {
            let last = self.nodes[self.nodes.len() - 1];
            if ts < last.1 {
                return Err(E_ORDER);
            }
        }
        self.nodes.push((seq, ts));
        self.expect_seq += 1;
        Ok(())
    }

    /// 链审计：seq 连续、时戳单调。
    pub fn audit(&self) -> Result<(), u16> {
        let mut i = 0usize;
        while i < self.nodes.len() {
            if self.nodes[i].0 != i as u64 {
                return Err(E_LOSS);
            }
            if i > 0 && self.nodes[i].1 < self.nodes[i - 1].1 {
                return Err(E_ORDER);
            }
            i += 1;
        }
        Ok(())
    }

    /// 链跨度（首尾时戳差）。
    pub fn span(&self) -> u64 {
        if self.nodes.len() < 2 {
            return 0;
        }
        let f = self.nodes[0].1;
        let l = self.nodes[self.nodes.len() - 1].1;
        l.wrapping_sub(f)
    }

    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut i = 0usize;
        while i < self.nodes.len() {
            h = h.wrapping_mul(0x0000_0100_0000_01b3) ^ self.nodes[i].0;
            h = h.wrapping_mul(0x0000_0100_0000_01b3) ^ self.nodes[i].1;
            i += 1;
        }
        h
    }
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

/// 主巡检：M2 延迟账分段对接十判据。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 单事件正衔接：采集→桥段入账，审计零断档。
    let mut led = LatLedger::new("m2bridge", 300, 0);
    led.feed(1_000, 1_050)?;
    led.audit()?;
    if led.count() != 1 {
        return Err(E_LEDGER);
    }
    hits += 1;

    // R2 衔接 ×10^4 fuzz 零断档：确定性 LCG 生成 10^4 对。
    let mut rng = Lcg::new(0x48583_0001);
    let mut led2 = LatLedger::new("m2bridge-fuzz", 300, 400);
    let mut prev_m2: u64 = 0;
    let mut k = 0u32;
    while k < 10_000 {
        let step = rng.below(400);
        prev_m2 = prev_m2.wrapping_add(1 + step);
        let d = rng.below(301);
        led2.feed(prev_m2, prev_m2 + d)?;
        k += 1;
    }
    led2.audit()?;
    if led2.count() != 10_000 {
        return Err(E_LEDGER);
    }
    hits += 1;

    // R3 衔接超窗反例：注入超 cap_delta 事件须返 E_DELTA。
    let mut led3 = LatLedger::new("m2bridge-cap", 50, 0);
    let r = led3.feed(10, 10 + 51);
    if r.err() != Some(E_DELTA) {
        return Err(E_LEDGER);
    }
    if led3.count() != 0 {
        return Err(E_LEDGER);
    }
    hits += 1;

    // R4 次序违例反例：桥段早于采集须返 E_ORDER。
    let mut led4 = LatLedger::new("m2bridge-order", 300, 0);
    let r = led4.feed(100, 99);
    if r.err() != Some(E_ORDER) {
        return Err(E_LEDGER);
    }
    hits += 1;

    // R5 段内断档反例：间隔超 cap_gap 须返 E_GAP。
    let mut led5 = LatLedger::new("m2bridge-gap", 300, 100);
    led5.feed(0, 10)?;
    let r = led5.feed(1_000, 1_010);
    if r.err() != Some(E_GAP) {
        return Err(E_LEDGER);
    }
    hits += 1;

    // R6 桥段链 ×10^3 连续挂接零丢零重。
    let mut ch = BridgeChain::new();
    let mut ts: u64 = 0;
    let mut s = 0u64;
    while s < 1_000 {
        ts += 1 + rng.below(3);
        ch.link(s, ts)?;
        s += 1;
    }
    ch.audit()?;
    if ch.expect_seq != 1_000 {
        return Err(E_LEDGER);
    }
    hits += 1;

    // R7 跳号注入：seq 越过期望序须返 E_LOSS。
    let mut ch2 = BridgeChain::new();
    ch2.link(0, 5)?;
    let r = ch2.link(2, 9);
    if r.err() != Some(E_LOSS) {
        return Err(E_LEDGER);
    }
    hits += 1;

    // R8 重复注入：seq 回退须返 E_DUP；时戳回退须返 E_ORDER。
    let mut ch3 = BridgeChain::new();
    ch3.link(0, 5)?;
    let r = ch3.link(0, 6);
    if r.err() != Some(E_DUP) {
        return Err(E_LEDGER);
    }
    let r = ch3.link(1, 4);
    if r.err() != Some(E_ORDER) {
        return Err(E_LEDGER);
    }
    hits += 1;

    // R9 账目守恒：账内事件数 == 喂入数，链跨度 == 首尾差。
    if led2.count() != 10_000 || led2.max_delta() > 300 {
        return Err(E_LEDGER);
    }
    if ch.span() != ts.wrapping_sub(ch.nodes[0].1) {
        return Err(E_LEDGER);
    }
    hits += 1;

    // R10 重演一致：同种子重建 ×10^4 账与链，指纹相等且非零。
    let fp1 = (led2.fingerprint(), ch.fingerprint());
    let mut rng2 = Lcg::new(0x48583_0001);
    let mut led5b = LatLedger::new("m2bridge-fuzz", 300, 400);
    let mut pm: u64 = 0;
    let mut k = 0u32;
    while k < 10_000 {
        let step = rng2.below(400);
        pm = pm.wrapping_add(1 + step);
        let d = rng2.below(301);
        led5b.feed(pm, pm + d)?;
        k += 1;
    }
    let mut ch4 = BridgeChain::new();
    let mut ts4: u64 = 0;
    let mut s4 = 0u64;
    while s4 < 1_000 {
        ts4 += 1 + rng2.below(3);
        ch4.link(s4, ts4)?;
        s4 += 1;
    }
    if (led5b.fingerprint(), ch4.fingerprint()) != fp1 || fp1.0 == 0 || fp1.1 == 0 {
        return Err(E_REPLAY);
    }
    hits += 1;

    Ok(hits)
}
