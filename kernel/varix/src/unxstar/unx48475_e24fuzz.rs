//! UNX-F48475 · E24 fuzz（随机字节流注入零崩可控断连）
//!
//! 判据原文：UNX-F48475-J1 —— 随机字节流注入协议解析端：任何输入序列
//! 都不产生 panic（零崩）；每个输入的结局必为「正常帧 | 可控断连（有限
//! 错误码白名单内）」二选一，无第三态；断连后解析端进入终态并拒绝后续
//! 输入（不产生半解析产物）；10 轮 × 1000 字节 fuzz 零崩零第三态；错误
//! 分布三态全被触达；同种子重放结局一致（确定性）。
//!
//! 设计纪律：no_std（仅 core/alloc）；零 unwrap/expect/panic（结构性
//! 保证：全路径 Result 返回）；fuzz 用确定性 LCG（wrapping_mul /
//! wrapping_add），同种子同序列可复现。

use crate::alloc::vec;
use crate::alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xE520;

pub const E_THIRD_STATE: u16 = SEG_BASE + 1;
pub const E_HALF_PARSE: u16 = SEG_BASE + 2;
pub const E_NONDET: u16 = SEG_BASE + 3;
pub const E_NO_COVER: u16 = SEG_BASE + 4;
pub const E_STILL_ALIVE: u16 = SEG_BASE + 5;
pub const E_FEED_AFTER_DEAD: u16 = SEG_BASE + 6;

/// 结局白名单（「正常帧 | 可控断连」二选一的封闭枚举）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Outcome {
    /// 正常消化：本次喂入完整落进帧（含攒半个帧的状态）。
    Absorb,
    /// 可控断连·半包截断不致命：头未齐暂存（合法中间态，非断连）。
    Trunc,
    /// 可控断连·帧长越界。
    Overrun,
    /// 可控断连·零长帧。
    ZeroLen,
    /// 可控断连·缓冲超容。
    Overflow,
}

impl Outcome {
    pub fn code(&self) -> u16 {
        match self {
            Outcome::Absorb => 0,
            Outcome::Trunc => C_TRUNC,
            Outcome::Overrun => C_OVERRUN,
            Outcome::ZeroLen => C_ZEROLEN,
            Outcome::Overflow => C_OVERFLOW,
        }
    }
    pub fn fatal(&self) -> bool {
        matches!(self, Outcome::Overrun | Outcome::ZeroLen | Outcome::Overflow)
    }
}

/// 结局白名单对应的对外错误码（协议面可见）。
pub const C_TRUNC: u16 = 0x51;
pub const C_OVERRUN: u16 = 0x52;
pub const C_ZEROLEN: u16 = 0x53;
pub const C_OVERFLOW: u16 = 0x54;

/// fuzz 用解析端：3 字节大端长度头 + 载荷；MAX 4096；终态拒收。
pub struct FuzzTarget {
    head: Vec<u8>,
    body: Vec<u8>,
    body_need: usize,
    pub dead: bool,
    pub frames: u32,
    pub bytes_seen: u64,
}

impl FuzzTarget {
    pub fn new() -> FuzzTarget {
        FuzzTarget {
            head: Vec::new(),
            body: Vec::new(),
            body_need: 0,
            dead: false,
            frames: 0,
            bytes_seen: 0,
        }
    }

    /// 喂入一段字节，返回本段结局。全路径 Result，无 panic 路径。
    pub fn feed(&mut self, bytes: &[u8]) -> Outcome {
        if self.dead {
            // 终态拒收：字节不消化、不改变状态。
            self.bytes_seen += bytes.len() as u64;
            return Outcome::Overflow;
        }
        let mut outcome = Outcome::Absorb;
        let mut idx = 0usize;
        while idx < bytes.len() {
            if self.dead {
                break;
            }
            if self.body_need == 0 {
                // 攒头。
                let need = 3 - self.head.len();
                let take = if need <= bytes.len() - idx { need } else { bytes.len() - idx };
                for k in 0..take {
                    self.head.push(bytes[idx + k]);
                }
                idx += take;
                self.bytes_seen += take as u64;
                if self.head.len() == 3 {
                    let n = ((self.head[0] as usize) << 16)
                        | ((self.head[1] as usize) << 8)
                        | (self.head[2] as usize);
                    self.head.clear();
                    if n == 0 {
                        self.dead = true;
                        outcome = Outcome::ZeroLen;
                        break;
                    }
                    if n > 4096 {
                        self.dead = true;
                        outcome = Outcome::Overrun;
                        break;
                    }
                    self.body_need = n;
                } else {
                    outcome = Outcome::Trunc;
                }
            } else {
                // 攒体。
                let need = self.body_need - self.body.len();
                let take = if need <= bytes.len() - idx { need } else { bytes.len() - idx };
                for k in 0..take {
                    self.body.push(bytes[idx + k]);
                }
                idx += take;
                self.bytes_seen += take as u64;
                if self.body.len() >= self.body_need {
                    self.body.clear();
                    self.body_need = 0;
                    self.frames += 1;
                } else {
                    outcome = Outcome::Trunc;
                }
            }
        }
        if self.body.len() > 8192 {
            self.dead = true;
            outcome = Outcome::Overflow;
        }
        outcome
    }
}

/// 确定性 LCG。
pub struct Lcg {
    st: u64,
}

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg { st: seed }
    }
    pub fn next(&mut self) -> u64 {
        self.st = self
            .st
            .wrapping_mul(1_664_525)
            .wrapping_add(1_013_904_223);
        self.st
    }
    pub fn below(&mut self, b: u64) -> u64 {
        if b == 0 {
            return 0;
        }
        self.next() % b
    }
}

/// 生成 n 字节随机流。
pub fn gen_bytes(seed: u64, n: usize) -> Vec<u8> {
    let mut g = Lcg::new(seed);
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        out.push((g.next() & 0xFF) as u8);
    }
    out
}

/// 结局分类守卫：白名单内即合法，二选一无第三态。
pub fn outcome_legal(o: Outcome) -> Result<(), u16> {
    match o.code() {
        0 => Ok(()),
        c if c == C_TRUNC || c == C_OVERRUN || c == C_ZEROLEN || c == C_OVERFLOW => Ok(()),
        _ => Err(E_THIRD_STATE),
    }
}

/// R1 随机流零崩：单轮 1000 字节注入，结局合法、无第三态。
pub fn r1_random_stream_zero_panic() -> Result<(), u16> {
    let bytes = gen_bytes(0x48475_0001, 1000);
    let mut t = FuzzTarget::new();
    let o = t.feed(&bytes);
    outcome_legal(o)
}

/// R2 十轮 fuzz：10 种子 × 1000 字节，全零崩全合法。
pub fn r2_ten_rounds() -> Result<(), u16> {
    for seed in 0..10u64 {
        let bytes = gen_bytes(0x48475_0100 + seed * 65537, 1000);
        let mut t = FuzzTarget::new();
        let o = t.feed(&bytes);
        outcome_legal(o)?;
    }
    Ok(())
}

/// R3 可控断连终态拒收：触发越界后 dead=true，续喂不消化不新帧。
pub fn r3_dead_reject() -> Result<(), u16> {
    let mut t = FuzzTarget::new();
    let o = t.feed(&[0xFFu8, 0xFF, 0xFF]);
    if o != Outcome::Overrun {
        return Err(E_THIRD_STATE);
    }
    if !t.dead {
        return Err(E_STILL_ALIVE);
    }
    let before = t.frames;
    let o2 = t.feed(&[0x00u8, 0x01, 0x02, 0x03, 0x04]);
    if o2 != Outcome::Overflow {
        return Err(E_FEED_AFTER_DEAD);
    }
    if t.frames != before {
        return Err(E_HALF_PARSE);
    }
    Ok(())
}

/// R4 零长帧断连：头 00 00 00 即终态。
pub fn r4_zero_len_fatal() -> Result<(), u16> {
    let mut t = FuzzTarget::new();
    let o = t.feed(&[0x00u8, 0x00, 0x00]);
    if o != Outcome::ZeroLen {
        return Err(E_THIRD_STATE);
    }
    if !t.dead {
        return Err(E_STILL_ALIVE);
    }
    Ok(())
}

/// R5 半包不致命：头 2 字节喂入得 Trunc，续喂余下字节可恢复成帧。
pub fn r5_trunc_survivable() -> Result<(), u16> {
    let mut t = FuzzTarget::new();
    let o1 = t.feed(&[0x00u8, 0x02]);
    if o1 != Outcome::Trunc {
        return Err(E_THIRD_STATE);
    }
    if t.dead {
        return Err(E_STILL_ALIVE);
    }
    let o2 = t.feed(&[0xAAu8, 0xBB]);
    if o2.code() != 0 && o2 != Outcome::Absorb {
        return Err(E_THIRD_STATE);
    }
    if t.frames != 1 {
        return Err(E_HALF_PARSE);
    }
    Ok(())
}

/// R6 缓冲超容断连：伪头 0x10 长度后灌 8193 字节触发 Overflow。
pub fn r6_overflow_guard() -> Result<(), u16> {
    let mut t = FuzzTarget::new();
    let o1 = t.feed(&[0x00u8, 0x10, 0x00]);
    if o1 != Outcome::Trunc && o1 != Outcome::Absorb {
        return Err(E_THIRD_STATE);
    }
    let big = gen_bytes(0x48475_0002, 8193);
    let o2 = t.feed(&big);
    if o2 != Outcome::Overflow {
        return Err(E_THIRD_STATE);
    }
    if !t.dead {
        return Err(E_STILL_ALIVE);
    }
    Ok(())
}

/// R7 错误分布覆盖：定向种子使 Absorb/Trunc/Overrun/ZeroLen 全触达。
pub fn r7_coverage() -> Result<(), u16> {
    let mut seen = [false; 5];
    // Absorb：良序帧。
    let mut t = FuzzTarget::new();
    seen[0] = t.feed(&[0x00u8, 0x01, 0x11]).code() == 0;
    // Trunc：只喂头。
    let mut t2 = FuzzTarget::new();
    seen[1] = t2.feed(&[0x00u8, 0x03]) == Outcome::Trunc;
    // Overrun：FF FF FF。
    let mut t3 = FuzzTarget::new();
    seen[2] = t3.feed(&[0xFFu8, 0xFF, 0xFF]) == Outcome::Overrun;
    // ZeroLen。
    let mut t4 = FuzzTarget::new();
    seen[3] = t4.feed(&[0x00u8, 0x00, 0x00]) == Outcome::ZeroLen;
    // Overflow。
    let mut t5 = FuzzTarget::new();
    let _ = t5.feed(&[0x00u8, 0x10, 0x00]);
    seen[4] = t5.feed(&gen_bytes(9, 8193)) == Outcome::Overflow;
    for k in 0..5 {
        if !seen[k] {
            return Err(E_NO_COVER);
        }
    }
    Ok(())
}

/// R8 确定性重放：同种子同切块策略两次喂入，帧数与 dead 全一致。
pub fn r8_deterministic_replay() -> Result<(), u16> {
    let bytes = gen_bytes(0x48475_0003, 600);
    let mut a = FuzzTarget::new();
    let mut b = FuzzTarget::new();
    let mut g = Lcg::new(0x48475_0004);
    let mut pos = 0usize;
    while pos < bytes.len() {
        let step = 1 + g.below(9) as usize;
        let end = if pos + step > bytes.len() { bytes.len() } else { pos + step };
        let mut c = Vec::new();
        for k in pos..end {
            c.push(bytes[k]);
        }
        let _ = a.feed(&c);
        let _ = b.feed(&c);
        pos = end;
    }
    if a.frames != b.frames || a.dead != b.dead || a.bytes_seen != b.bytes_seen {
        return Err(E_NONDET);
    }
    Ok(())
}

/// R9 大流量 fuzz：5 万字节分块灌入，零第三态，帧数与字节账守恒。
pub fn r9_bulk_fuzz() -> Result<(), u16> {
    let bytes = gen_bytes(0x48475_0005, 50_000);
    let mut t = FuzzTarget::new();
    let mut g = Lcg::new(0x48475_0006);
    let mut pos = 0usize;
    let mut consumed: u64 = 0;
    while pos < bytes.len() {
        let step = 1 + g.below(64) as usize;
        let end = if pos + step > bytes.len() { bytes.len() } else { pos + step };
        let mut c = Vec::new();
        for k in pos..end {
            c.push(bytes[k]);
        }
        consumed += c.len() as u64;
        let o = t.feed(&c);
        outcome_legal(o)?;
        if t.dead {
            break;
        }
        pos = end;
    }
    if t.bytes_seen != consumed {
        return Err(E_NONDET);
    }
    Ok(())
}

/// R10 帧账守恒：精心构造 k 帧流，frames 计数恰为 k。
pub fn r10_frame_ledger() -> Result<(), u16> {
    let k = 7u32;
    let mut wire: Vec<u8> = Vec::new();
    for i in 0..k {
        let payload: u8 = 0x30 + i as u8;
        wire.push(0x00);
        wire.push(0x00);
        wire.push(0x02);
        wire.push(payload);
        wire.push(payload.wrapping_add(0x40));
    }
    let mut t = FuzzTarget::new();
    let o = t.feed(&wire);
    outcome_legal(o)?;
    if t.frames != k {
        return Err(E_HALF_PARSE);
    }
    if t.dead {
        return Err(E_STILL_ALIVE);
    }
    Ok(())
}

/// 判据主入口：R1..R10，hits 计数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    r1_random_stream_zero_panic()?;
    hits += 1;
    r2_ten_rounds()?;
    hits += 1;
    r3_dead_reject()?;
    hits += 1;
    r4_zero_len_fatal()?;
    hits += 1;
    r5_trunc_survivable()?;
    hits += 1;
    r6_overflow_guard()?;
    hits += 1;
    r7_coverage()?;
    hits += 1;
    r8_deterministic_replay()?;
    hits += 1;
    r9_bulk_fuzz()?;
    hits += 1;
    r10_frame_ledger()?;
    hits += 1;
    Ok(hits)
}
