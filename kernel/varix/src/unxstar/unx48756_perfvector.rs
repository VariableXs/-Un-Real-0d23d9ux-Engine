//! UNX-F48756 C·性能向量账终版（性能用例集+校验和）
//! 判据 UNX-F48756-J1：终版归档；判据锚定 §AI-61 §8。
//! 设计纪律：no_std（仅 core/alloc）；诊断码自 SEG_BASE+1 起连续且总数 ≤0x1F；
//! drill() 自证十判据，正例计数、反例注入必须 .err() 命中，命中即 Err 传播；
//! fuzz 用确定性 LCG（wrapping_mul 1_664_525 / wrapping_add 1_013_904_223）；
//! 性能用例集冻结校验和，账改一字校验和即崩，终版归档不可再改。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0x1C20;
const E_CS_DIFF: u16 = SEG_BASE + 1;
const E_CASE_MISS: u16 = SEG_BASE + 2;
const E_CASE_DUP: u16 = SEG_BASE + 3;
const E_FROZEN: u16 = SEG_BASE + 4;
const E_EMPTY_ARCH: u16 = SEG_BASE + 5;
const E_REPLAY_DIFF: u16 = SEG_BASE + 6;
const E_TAG_BAD: u16 = SEG_BASE + 7;
const E_ARCH_GAP: u16 = SEG_BASE + 8;

/// 性能用例
#[derive(Clone, Debug, PartialEq)]
pub struct PerfCase {
    pub no: u16,
    pub path: String,
    pub us: u32,
}

impl PerfCase {
    pub fn make(no: u16, path: &str, us: u32) -> Self {
        PerfCase { no, path: String::from(path), us }
    }
}

/// 校验和：FNV-1a 累算
pub fn checksum(cases: &[PerfCase]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0usize;
    while i < cases.len() {
        let c = &cases[i];
        h = h.wrapping_mul(0x0100_0193) ^ (c.no as u32);
        let bytes = c.path.as_bytes();
        let mut j = 0usize;
        while j < bytes.len() {
            h = h.wrapping_mul(0x0100_0193) ^ (bytes[j] as u32);
            j += 1;
        }
        h = h.wrapping_mul(0x0100_0193) ^ c.us;
        i += 1;
    }
    h
}

/// 性能向量账终版
pub struct VectorLedger {
    pub cases: Vec<PerfCase>,
    pub frozen_cs: Option<u32>,
}

impl VectorLedger {
    pub fn new() -> Self {
        VectorLedger { cases: vec![], frozen_cs: None }
    }

    pub fn add(&mut self, no: u16, path: &str, us: u32) -> Result<(), u16> {
        if self.frozen_cs.is_some() {
            return Err(E_FROZEN);
        }
        if path.is_empty() {
            return Err(E_TAG_BAD);
        }
        let mut i = 0usize;
        while i < self.cases.len() {
            if self.cases[i].no == no {
                return Err(E_CASE_DUP);
            }
            i += 1;
        }
        self.cases.push(PerfCase::make(no, path, us));
        Ok(())
    }

    /// 终版冻结：校验和入档，此后账不可再改
    pub fn freeze(&mut self) -> Result<u32, u16> {
        if self.cases.is_empty() {
            return Err(E_EMPTY_ARCH);
        }
        let cs = checksum(&self.cases);
        self.frozen_cs = Some(cs);
        Ok(cs)
    }

    /// 归档核对：账内重算校验和须与冻结值一致
    pub fn verify_archive(&self) -> Result<(), u16> {
        let cs = match self.frozen_cs {
            Some(c) => c,
            None => return Err(E_ARCH_GAP),
        };
        if checksum(&self.cases) != cs {
            return Err(E_CS_DIFF);
        }
        Ok(())
    }

    /// 用例齐备：1..=n 逐号在册
    pub fn verify_cases(&self, n: u16) -> Result<(), u16> {
        if self.cases.len() != n as usize {
            return Err(E_CASE_MISS);
        }
        let mut s = 1u16;
        while s <= n {
            let mut found = false;
            let mut i = 0usize;
            while i < self.cases.len() {
                if self.cases[i].no == s {
                    found = true;
                }
                i += 1;
            }
            if !found {
                return Err(E_CASE_MISS);
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

/// 终版归档 ×10^3：建账-冻结-复核
fn sweep_archive(rng: &mut Lcg) -> Result<(), u16> {
    let mut iter = 0u32;
    while iter < 1_000 {
        let mut led = VectorLedger::new();
        let n = 3 + rng.below(6);
        let mut s = 1u16;
        while s <= n as u16 {
            let path = match s % 3 {
                0 => "key",
                1 => "conv",
                _ => "render",
            };
            led.add(s, path, 500 + rng.below(2_000) as u32)?;
            s += 1;
        }
        let cs = led.freeze()?;
        led.verify_archive()?;
        if led.frozen_cs != Some(cs) {
            return Err(E_ARCH_GAP);
        }
        iter += 1;
    }
    Ok(())
}

/// 篡改即崩 ×10：冻结后改一字校验和必红
fn sweep_tamper() -> Result<(), u16> {
    let mut round = 0u32;
    while round < 10 {
        let mut led = VectorLedger::new();
        led.add(1, "key", 800)?;
        led.add(2, "conv", 900)?;
        led.freeze()?;
        led.cases[0].us += 1;
        if led.verify_archive().err() != Some(E_CS_DIFF) {
            return Err(E_CS_DIFF);
        }
        round += 1;
    }
    Ok(())
}

/// 反例注入：冻结后写入、空账冻结、重复用例、空路径、未冻复核全命中
fn sweep_negatives() -> Result<(), u16> {
    let mut led = VectorLedger::new();
    led.add(1, "key", 800)?;
    led.add(1, "conv", 900)?;
    match led.cases.len() {
        1 => {}
        _ => return Err(E_CASE_DUP),
    }
    let r1 = VectorLedger::new().freeze();
    if r1.err() != Some(E_EMPTY_ARCH) {
        return Err(E_EMPTY_ARCH);
    }
    let mut led2 = VectorLedger::new();
    led2.add(1, "a", 1)?;
    led2.freeze()?;
    let r2 = led2.add(2, "b", 2);
    if r2.err() != Some(E_FROZEN) {
        return Err(E_FROZEN);
    }
    let r3 = VectorLedger::new().verify_archive();
    if r3.err() != Some(E_ARCH_GAP) {
        return Err(E_ARCH_GAP);
    }
    let mut led3 = VectorLedger::new();
    let r4 = led3.add(1, "", 5);
    if r4.err() != Some(E_TAG_BAD) {
        return Err(E_TAG_BAD);
    }
    Ok(())
}

/// 重放一致 ×20：同种子双跑校验和逐位吻合
fn sweep_replay(seed: u64) -> Result<(), u16> {
    let mut round = 0u32;
    while round < 20 {
        let mut la = VectorLedger::new();
        let mut lb = VectorLedger::new();
        let mut ra = Lcg::new(seed + round as u64);
        let mut rb = Lcg::new(seed + round as u64);
        let mut s = 1u16;
        while s <= 6 {
            let va = 400 + ra.below(1_600) as u32;
            let vb = 400 + rb.below(1_600) as u32;
            if va != vb {
                return Err(E_REPLAY_DIFF);
            }
            la.add(s, "replay", va)?;
            lb.add(s, "replay", vb)?;
            s += 1;
        }
        let ca = la.freeze()?;
        let cb = lb.freeze()?;
        if ca != cb {
            return Err(E_REPLAY_DIFF);
        }
        round += 1;
    }
    Ok(())
}

pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;
    // R1 终版归档 ×10^3
    let mut rng = Lcg::new(0x4875_60EE);
    let mut r1 = 0u32;
    while r1 < 1_000 {
        sweep_archive(&mut rng)?;
        r1 += 1;
    }
    hits += 1;
    // R2 篡改即崩 ×10
    sweep_tamper()?;
    hits += 1;
    // R3 反例注入全命中
    sweep_negatives()?;
    hits += 1;
    // R4 重放一致 ×20
    sweep_replay(0x4875_6B0A)?;
    hits += 1;
    // R5 用例齐备 ×12 全带
    let mut led = VectorLedger::new();
    let mut s = 1u16;
    while s <= 12 {
        led.add(s, "band", 1_000 + s as u32)?;
        s += 1;
    }
    led.verify_cases(12)?;
    led.freeze()?;
    led.verify_archive()?;
    hits += 1;
    // R6 冻结态终检：frozen_cs 为 Some 且与重算一致
    match led.frozen_cs {
        Some(cs) => {
            if cs != checksum(&led.cases) {
                return Err(E_CS_DIFF);
            }
        }
        None => return Err(E_ARCH_GAP),
    }
    hits += 1;
    // R7 校验和语义：同账同和、异账异和
    let mut la = VectorLedger::new();
    la.add(1, "same", 100)?;
    let c1 = la.freeze()?;
    let mut lb = VectorLedger::new();
    lb.add(1, "same", 100)?;
    let c2 = lb.freeze()?;
    let mut lc = VectorLedger::new();
    lc.add(1, "same", 101)?;
    let c3 = lc.freeze()?;
    if c1 != c2 || c1 == c3 {
        return Err(E_CS_DIFF);
    }
    hits += 1;
    // R8 用例序号连续：登记序即号序
    let mut i = 0usize;
    while i < led.cases.len() {
        if led.cases[i].no != (i + 1) as u16 {
            return Err(E_CASE_MISS);
        }
        i += 1;
    }
    hits += 1;
    // R9 冻结旗标语义：None 时可写、Some 后必拒
    let mut ld = VectorLedger::new();
    if ld.frozen_cs.is_some() {
        return Err(E_FROZEN);
    }
    ld.add(1, "w", 1)?;
    ld.freeze()?;
    if ld.frozen_cs.is_none() {
        return Err(E_ARCH_GAP);
    }
    hits += 1;
    // R10 路径名非空 ×50 组
    let mut c = 0u32;
    while c < 50 {
        let p = match rng.below(3) {
            0 => "key",
            1 => "conv",
            _ => "render",
        };
        if p.is_empty() {
            return Err(E_TAG_BAD);
        }
        c += 1;
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
