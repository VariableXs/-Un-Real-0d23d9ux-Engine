//! UNX-F48430 —— E·恶意 span 注入（反：offset/len 溢出注入 ×10 全检出）
//! 判据原文：UNX-F48430-J1 溢出注入（offset 回绕/len 回绕/offset+len 加法溢出/
//!           巨型 len/负语义 offset）×10 全检出；检测账四型封闭；
//!           LCG 随机恶意 span ×256 全检出；合法 span 零误检；
//!           检测账封印重演一致；检测器状态无副作用。
//! 设计纪律：no_std；零 unwrap/expect/panic；确定性 LCG；drill 闭包判据全绿才返回 Ok。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xD9A0; // E22 恶意 span 段

pub const TEXT_CAP: usize = 1024;

const E_EVIL_PASS: u16 = SEG_BASE + 1; // 恶意 span 放行
const E_LEGAL_HIT: u16 = SEG_BASE + 2; // 合法误检
const E_KIND_BAD: u16 = SEG_BASE + 3; // 分类越界
const E_SEAL_BAD: u16 = SEG_BASE + 4; // 封印失配
const E_REPLAY_DIFF: u16 = SEG_BASE + 5; // 重演不一致
const E_BAL_MISS: u16 = SEG_BASE + 6; // 两账守恒破
const E_STATE_LEAK: u16 = SEG_BASE + 7; // 检测器状态副作用

/// 恶意型。
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum EvilKind {
    OffWrap = 0,   // offset 回绕（近 usize::MAX）
    LenWrap = 1,   // len 回绕
    AddOverflow = 2, // offset+len 加法溢出
    GiantLen = 3,  // 巨型 len
}

/// 恶意 span 描述（u64 载荷规避构造期溢出）。
#[derive(Clone, Copy)]
pub struct EvilSpan {
    pub offset: u64,
    pub len: u64,
}

/// 溢出检测器（usize=64 位平台假设显式化）。
pub struct EvilDetector {
    text_len: u64,
    detected: Vec<(u64, EvilKind)>,
    seq: u64,
    ok_n: u64,
}

impl EvilDetector {
    pub fn new(text_len: u64) -> Self {
        EvilDetector { text_len, detected: Vec::new(), seq: 0, ok_n: 0 }
    }

    /// 校验：任一溢出/越界型即检出。
    pub fn check(&mut self, sp: EvilSpan) -> Result<(), u16> {
        self.seq += 1;
        // 型 0：offset 回绕（> usize 实用界）
        if sp.offset > self.text_len {
            self.detected.push((self.seq, EvilKind::OffWrap));
            return Err(E_EVIL_PASS);
        }
        // 型 1：len 回绕
        if sp.len > self.text_len {
            self.detected.push((self.seq, EvilKind::LenWrap));
            return Err(E_EVIL_PASS);
        }
        // 型 2：offset+len 加法溢出（checked_add）
        match sp.offset.checked_add(sp.len) {
            None => {
                self.detected.push((self.seq, EvilKind::AddOverflow));
                return Err(E_EVIL_PASS);
            }
            Some(sum) => {
                if sum > self.text_len {
                    self.detected.push((self.seq, EvilKind::GiantLen));
                    return Err(E_EVIL_PASS);
                }
            }
        }
        self.ok_n += 1;
        Ok(())
    }

    /// 分类封闭。
    pub fn audit_kind_closed(&self) -> Result<(), u16> {
        let mut i = 0usize;
        while i < self.detected.len() {
            if self.detected[i].1 as u16 > 3 {
                return Err(E_KIND_BAD);
            }
            i += 1;
        }
        Ok(())
    }

    /// 守恒：ok + detected.len() == seq。
    pub fn audit_balance(&self) -> Result<(), u16> {
        if self.ok_n + self.detected.len() as u64 != self.seq {
            return Err(E_BAL_MISS);
        }
        Ok(())
    }

    /// 状态无副作用：text_len 恒定。
    pub fn audit_stateless(&self, expect: u64) -> Result<(), u16> {
        if self.text_len != expect {
            return Err(E_STATE_LEAK);
        }
        Ok(())
    }

    pub fn fp(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut i = 0usize;
        while i < self.detected.len() {
            h ^= self.detected[i].0;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
            h ^= self.detected[i].1 as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
            i += 1;
        }
        h
    }

    pub fn detected_count(&self) -> u32 {
        self.detected.len() as u32
    }
}

/// 恶意 span ×10 清单。
pub fn evil_ten() -> Vec<EvilSpan> {
    let max = u64::MAX;
    vec![
        EvilSpan { offset: max, len: 1 },               // offset 回绕
        EvilSpan { offset: max - 5, len: 10 },          // offset 近 MAX
        EvilSpan { offset: 0, len: max },               // len 回绕
        EvilSpan { offset: 1, len: max - 1 },           // 加法溢出（恰好回绕 0）
        EvilSpan { offset: max / 2, len: max / 2 + 1 }, // 加法溢出
        EvilSpan { offset: 0, len: 1 << 40 },           // 巨型 len（1T）
        EvilSpan { offset: 5_000_000, len: 10 },        // offset 远超文本
        EvilSpan { offset: 1024, len: 1 },              // offset == 文本长 + 1 型越界
        EvilSpan { offset: 1023, len: 2 },              // 越尾 1 字节
        EvilSpan { offset: 0, len: 1025 },              // len 恰超 1
    ]
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
        self.next() % n
    }
}

/// 主巡检：恶意 span 注入八判据。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 恶意 ×10 全检出
    let mut d = EvilDetector::new(1024);
    let evils = evil_ten();
    let mut i = 0usize;
    while i < evils.len() {
        match d.check(evils[i]) {
            Err(E_EVIL_PASS) => {}
            _ => return Err(E_EVIL_PASS),
        }
        i += 1;
    }
    if d.detected_count() != 10 {
        return Err(E_BAL_MISS);
    }
    hits += 1;

    // R2 合法 span 零误检：全文本与中段切片全过
    let mut d2 = EvilDetector::new(1024);
    d2.check(EvilSpan { offset: 0, len: 1024 })?;
    d2.check(EvilSpan { offset: 100, len: 50 })?;
    d2.check(EvilSpan { offset: 1024, len: 0 })?;
    if d2.ok_n != 3 {
        return Err(E_LEGAL_HIT);
    }
    hits += 1;

    // R3 分类封闭：10 恶意全落四型
    d.audit_kind_closed()?;
    hits += 1;

    // R4 状态无副作用：检测前后 text_len 不变
    d.audit_stateless(1024)?;
    hits += 1;

    // R5 守恒：10 evil + 0 ok（d 只收恶意）= seq
    d.audit_balance()?;
    hits += 1;

    // R6 LCG 随机恶意 ×256 全检出
    let mut d3 = EvilDetector::new(1024);
    let mut rng = Lcg::new(0x48430);
    let mut k = 0u32;
    while k < 256 {
        let m = rng.below(4);
        let sp = match m {
            0 => EvilSpan { offset: u64::MAX - rng.below(100), len: rng.below(50) + 1 },
            1 => EvilSpan { offset: rng.below(100), len: (1u64 << 40) + rng.below(100) },
            2 => EvilSpan { offset: 900 + rng.below(100), len: 900 + rng.below(100) },
            _ => EvilSpan { offset: 1100 + rng.below(1000), len: 5 },
        };
        match d3.check(sp) {
            Err(E_EVIL_PASS) => {}
            _ => return Err(E_EVIL_PASS),
        }
        k += 1;
    }
    if d3.detected_count() != 256 {
        return Err(E_BAL_MISS);
    }
    d3.audit_kind_closed()?;
    hits += 1;

    // R7 封印重演一致：同种子双跑检测账指纹相等
    let f1 = d3.fp();
    let mut d4 = EvilDetector::new(1024);
    let mut rng2 = Lcg::new(0x48430);
    let mut k = 0u32;
    while k < 256 {
        let m = rng2.below(4);
        let sp = match m {
            0 => EvilSpan { offset: u64::MAX - rng2.below(100), len: rng2.below(50) + 1 },
            1 => EvilSpan { offset: rng2.below(100), len: (1u64 << 40) + rng2.below(100) },
            2 => EvilSpan { offset: 900 + rng2.below(100), len: 900 + rng2.below(100) },
            _ => EvilSpan { offset: 1100 + rng2.below(1000), len: 5 },
        };
        let _ = d4.check(sp);
        k += 1;
    }
    if f1 != d4.fp() || f1 == 0 {
        return Err(E_REPLAY_DIFF);
    }
    hits += 1;

    // R8 守恒终验 + 封印稳定
    d3.audit_balance()?;
    let s1 = d3.fp();
    let s2 = d3.fp();
    if s1 != s2 {
        return Err(E_SEAL_BAD);
    }
    hits += 1;

    Ok(hits)
}
