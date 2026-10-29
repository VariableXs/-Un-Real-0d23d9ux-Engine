//! UNX-F48572 —— I29 错误码族 I 段（6 条：联签失配/通知失败等）
//! 判据原文：UNX-F48572-J1 I29 错误码族 I 段登记 6 条
//!           （联签失配/通知失败/窗口类失联/占比不足/换位越界/账目失恒），
//!           码值 SEG_BASE+1 起连续且 ≤ SEG_BASE+0x1F；
//!           每码附类别与可重试位；按类别反查码值；未知类别拒查；
//!           码表冻结指纹恒定；LCG 随机类别 ×256 全归类零崩；
//!           登记账守恒与重演一致。
//! 设计纪律：no_std；零 unwrap/expect/panic；确定性 LCG；drill 闭包判据全绿才返回 Ok。

use crate::alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xF740; // I29 错误码族段

const E_REG_MISS: u16 = SEG_BASE + 1; // 码登记缺项
const E_GAP_BAD: u16 = SEG_BASE + 2; // 码值不连续/越窗
const E_KIND_BAD: u16 = SEG_BASE + 3; // 类别越界
const E_LOOKUP_MISS: u16 = SEG_BASE + 4; // 类别反查失败
const E_FROZEN_BREAK: u16 = SEG_BASE + 5; // 码表冻结漂移
const E_KIND_LEAK: u16 = SEG_BASE + 6; // 随机类别归类泄漏
const E_LEDGER_MISS: u16 = SEG_BASE + 7; // 账目不符
const E_REPLAY_DIFF: u16 = SEG_BASE + 8; // 重演不一致

/// 错误类别（六条）。
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ErrKind {
    SignMismatch = 0,  // 联签失配
    NotifyFail = 1,    // 通知失败
    ClassLost = 2,     // 窗口类失联
    RatioShort = 3,    // 占比不足
    MoveOob = 4,       // 换位越界
    LedgerBroken = 5,  // 账目失恒
}

/// 码项。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ErrItem {
    pub code: u16,
    pub kind: u8,
    pub retryable: bool,
}

/// 冻结码表：SEG_BASE+1 起连续六条。
pub fn frozen_codes() -> [ErrItem; 6] {
    [
        ErrItem { code: SEG_BASE + 1, kind: ErrKind::SignMismatch as u8, retryable: false },
        ErrItem { code: SEG_BASE + 2, kind: ErrKind::NotifyFail as u8, retryable: true },
        ErrItem { code: SEG_BASE + 3, kind: ErrKind::ClassLost as u8, retryable: true },
        ErrItem { code: SEG_BASE + 4, kind: ErrKind::RatioShort as u8, retryable: false },
        ErrItem { code: SEG_BASE + 5, kind: ErrKind::MoveOob as u8, retryable: false },
        ErrItem { code: SEG_BASE + 6, kind: ErrKind::LedgerBroken as u8, retryable: false },
    ]
}

/// 按类别反查码值。
pub fn lookup_kind(kind: u8) -> Option<u16> {
    let t = frozen_codes();
    let mut i = 0usize;
    while i < t.len() {
        if t[i].kind == kind {
            return Some(t[i].code);
        }
        i += 1;
    }
    None
}

/// 码表指纹。
pub fn table_fp() -> u64 {
    let t = frozen_codes();
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut i = 0usize;
    while i < t.len() {
        h ^= t[i].code as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        h ^= t[i].kind as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        h ^= t[i].retryable as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        i += 1;
    }
    h
}

/// 登记账。
pub struct CodeLedger {
    pub lookups: Vec<(u64, u8, Option<u16>)>,
    pub seq: u64,
    pub label: String,
}

impl CodeLedger {
    pub fn new() -> Self {
        CodeLedger {
            lookups: Vec::new(),
            seq: 0,
            label: "i29-err-ledger".to_string(),
        }
    }

    pub fn probe(&mut self, kind: u8) -> Option<u16> {
        self.seq += 1;
        let r = lookup_kind(kind);
        self.lookups.push((self.seq, kind, r));
        r
    }

    pub fn audit_balance(&self) -> Result<(), u16> {
        if self.lookups.len() as u64 != self.seq {
            return Err(E_LEDGER_MISS);
        }
        let mut i = 0usize;
        while i < self.lookups.len() {
            let k = self.lookups[i].1;
            if k > 5 {
                // 未知类别必须查空
                if self.lookups[i].2.is_some() {
                    return Err(E_KIND_BAD);
                }
            }
            i += 1;
        }
        Ok(())
    }

    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut i = 0usize;
        while i < self.lookups.len() {
            h ^= self.lookups[i].1 as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
            let hit = match self.lookups[i].2 {
                Some(c) => c as u64 + 1,
                None => 0,
            };
            h ^= hit;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
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
        (self.next() >> 16) % n
    }
}

/// 主巡检：错误码族九判据。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 六条码连续登记：SEG_BASE+1..+6
    let t = frozen_codes();
    if t.len() != 6 {
        return Err(E_REG_MISS);
    }
    let mut i = 0usize;
    while i < t.len() {
        if t[i].code != SEG_BASE + 1 + i as u16 {
            return Err(E_GAP_BAD);
        }
        if t[i].code > SEG_BASE + 0x1F {
            return Err(E_GAP_BAD);
        }
        i += 1;
    }
    hits += 1;

    // R2 六类别一一对应：k → code 且互异
    let mut k = 0u8;
    while k < 6 {
        let c = match lookup_kind(k) {
            Some(c) => c,
            None => return Err(E_LOOKUP_MISS),
        };
        if c != SEG_BASE + 1 + k as u16 {
            return Err(E_LOOKUP_MISS);
        }
        k += 1;
    }
    let mut a = 0u8;
    while a < 6 {
        let mut b = a + 1;
        while b < 6 {
            if lookup_kind(a) == lookup_kind(b) {
                return Err(E_REG_MISS);
            }
            b += 1;
        }
        a += 1;
    }
    hits += 1;

    // R3 未知类别查空
    let unknown: [u8; 5] = [6, 7, 32, 200, 255];
    let mut i = 0usize;
    while i < unknown.len() {
        if lookup_kind(unknown[i]).is_some() {
            return Err(E_KIND_LEAK);
        }
        i += 1;
    }
    hits += 1;

    // R4 冻结指纹恒定
    if table_fp() != table_fp() || table_fp() == 0 {
        return Err(E_FROZEN_BREAK);
    }
    let fp_before = table_fp();
    let _ = lookup_kind(2);
    if table_fp() != fp_before {
        return Err(E_FROZEN_BREAK);
    }
    hits += 1;

    // R5 可重试位语义：NotifyFail/ClassLost 可重试，其余不可
    let t2 = frozen_codes();
    if !t2[1].retryable || !t2[2].retryable {
        return Err(E_REG_MISS);
    }
    if t2[0].retryable || t2[3].retryable || t2[4].retryable || t2[5].retryable {
        return Err(E_REG_MISS);
    }
    hits += 1;

    // R6 LCG 随机类别 ×256 全归类零崩
    let mut rng = Lcg::new(0x48572);
    let mut lg = CodeLedger::new();
    let mut k = 0u32;
    while k < 256 {
        let kind = rng.below(9) as u8; // 0..8：六合法三非法
        let got = lg.probe(kind);
        if kind < 6 {
            match got {
                Some(c) => {
                    if c != SEG_BASE + 1 + kind as u16 {
                        return Err(E_KIND_LEAK);
                    }
                }
                None => return Err(E_KIND_LEAK),
            }
        } else if got.is_some() {
            return Err(E_KIND_LEAK);
        }
        k += 1;
    }
    lg.audit_balance()?;
    hits += 1;

    // R7 账目守恒
    if lg.seq != lg.lookups.len() as u64 {
        return Err(E_LEDGER_MISS);
    }
    hits += 1;

    // R8 重演一致：同种子指纹相等
    let fp1 = lg.fingerprint();
    let mut rng2 = Lcg::new(0x48572);
    let mut lg2 = CodeLedger::new();
    let mut k = 0u32;
    while k < 256 {
        let kind = rng2.below(9) as u8;
        let _ = lg2.probe(kind);
        k += 1;
    }
    if lg2.fingerprint() != fp1 || fp1 == 0 {
        return Err(E_REPLAY_DIFF);
    }
    hits += 1;

    // R9 码值域上界复核：全部 ≤ SEG_BASE+0x1F 且 ≥ SEG_BASE+1
    let mut i = 0usize;
    while i < t2.len() {
        if t2[i].code < SEG_BASE + 1 || t2[i].code > SEG_BASE + 0x1F {
            return Err(E_GAP_BAD);
        }
        i += 1;
    }
    hits += 1;

    Ok(hits)
}
