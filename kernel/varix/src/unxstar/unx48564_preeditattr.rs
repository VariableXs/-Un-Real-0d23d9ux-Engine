//! UNX-F48564 —— I·preedit 样式到绘制语义联签（attr 三值映射表联签冻结）
//! 判据原文：UNX-F48564-J1 preedit attr 三值（下划线/高亮/无）到绘制语义
//!           （色对/线宽/占位）的映射表冻结联签：三值全量映射确定；
//!           映射指纹逐次调用恒定（冻结）；未知 attr 值拒签；
//!           映射为双射（无重影）；attr 序列往返一致；
//!           LCG 随机 attr 流 ×512 全归类零崩；联签账目守恒与重演一致。
//! 设计纪律：no_std；零 unwrap/expect/panic；确定性 LCG；drill 闭包判据全绿才返回 Ok。

use crate::alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xF5E0; // I29 preedit attr 联签段

const E_MAP_MISS: u16 = SEG_BASE + 1; // 三值映射缺项
const E_FROZEN_BREAK: u16 = SEG_BASE + 2; // 冻结指纹漂移
const E_UNKNOW_ATTR: u16 = SEG_BASE + 3; // 未知 attr 混入
const E_BROKEN_BIJ: u16 = SEG_BASE + 4; // 映射非双射
const E_ROUNDTRIP: u16 = SEG_BASE + 5; // 往返失真
const E_KIND_LEAK: u16 = SEG_BASE + 6; // 模糊流归类泄漏
const E_LEDGER_MISS: u16 = SEG_BASE + 7; // 账目不符
const E_REPLAY_DIFF: u16 = SEG_BASE + 8; // 重演不一致

/// preedit attr 三值。
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum PreeditAttr {
    Underline = 0,
    Highlight = 1,
    Plain = 2,
}

/// 绘制语义。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct DrawSem {
    pub fg: u8,    // 前景色索引
    pub bg: u8,    // 背景色索引
    pub thick: u8, // 线宽
}

/// 冻结映射表：attr → DrawSem（联签基准）。
pub fn frozen_table() -> [(u8, DrawSem); 3] {
    [
        (PreeditAttr::Underline as u8, DrawSem { fg: 2, bg: 0, thick: 1 }),
        (PreeditAttr::Highlight as u8, DrawSem { fg: 0, bg: 3, thick: 0 }),
        (PreeditAttr::Plain as u8, DrawSem { fg: 1, bg: 0, thick: 0 }),
    ]
}

/// 查表：未知 attr 返回 None。
pub fn lookup(attr: u8) -> Option<DrawSem> {
    let t = frozen_table();
    let mut i = 0usize;
    while i < t.len() {
        if t[i].0 == attr {
            return Some(t[i].1);
        }
        i += 1;
    }
    None
}

/// 联签器。
pub struct AttrSigner {
    pub signed: Vec<(u8, DrawSem)>,
    pub rejected: Vec<(u64, u8)>,
    pub seq: u64,
    pub label: String,
}

impl AttrSigner {
    pub fn new() -> Self {
        AttrSigner {
            signed: Vec::new(),
            rejected: Vec::new(),
            seq: 0,
            label: "attr-signer".to_string(),
        }
    }

    /// 签一条 attr：查冻结表，命中即签，未命中拒签记账。
    pub fn sign(&mut self, attr: u8) -> Result<DrawSem, u16> {
        self.seq += 1;
        match lookup(attr) {
            Some(sem) => {
                self.signed.push((attr, sem));
                Ok(sem)
            }
            None => {
                self.rejected.push((self.seq, attr));
                Err(E_UNKNOW_ATTR)
            }
        }
    }

    /// 三值全量映射：attr 0/1/2 全部有语义且互不相同。
    pub fn audit_full_map(&self) -> Result<(), u16> {
        let mut a = 0u8;
        while a < 3 {
            match lookup(a) {
                Some(s) => {
                    let _ = s;
                }
                None => return Err(E_MAP_MISS),
            }
            a += 1;
        }
        let s0 = lookup(0);
        let s1 = lookup(1);
        let s2 = lookup(2);
        if s0 == s1 || s1 == s2 || s0 == s2 {
            return Err(E_BROKEN_BIJ);
        }
        Ok(())
    }

    /// 冻结性：连续三次取表指纹恒定。
    pub fn audit_frozen(&self) -> Result<(), u16> {
        let f0 = table_fp();
        let f1 = table_fp();
        let f2 = table_fp();
        if f0 != f1 || f1 != f2 || f0 == 0 {
            return Err(E_FROZEN_BREAK);
        }
        Ok(())
    }

    /// 双射复核：签名侧三语义计数各一。
    pub fn audit_bijective(&self) -> Result<(), u16> {
        let t = frozen_table();
        let mut i = 0usize;
        while i < t.len() {
            let mut j = i + 1;
            while j < t.len() {
                if t[i].1 == t[j].1 {
                    return Err(E_BROKEN_BIJ);
                }
                j += 1;
            }
            i += 1;
        }
        Ok(())
    }

    /// 往返：attr → 语义 → 反查 attr 一致。
    pub fn audit_roundtrip(&self) -> Result<(), u16> {
        let mut a = 0u8;
        while a < 3 {
            let sem = match lookup(a) {
                Some(s) => s,
                None => return Err(E_MAP_MISS),
            };
            // 由语义反查唯一 attr
            let t = frozen_table();
            let mut hit = 0u32;
            let mut i = 0usize;
            while i < t.len() {
                if t[i].1 == sem {
                    hit += 1;
                }
                i += 1;
            }
            if hit != 1 {
                return Err(E_ROUNDTRIP);
            }
            a += 1;
        }
        Ok(())
    }

    pub fn audit_balance(&self) -> Result<(), u16> {
        let total = (self.signed.len() + self.rejected.len()) as u64;
        if total != self.seq {
            return Err(E_LEDGER_MISS);
        }
        Ok(())
    }

    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        h ^= self.signed.len() as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        let mut i = 0usize;
        while i < self.signed.len() {
            h ^= self.signed[i].0 as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
            i += 1;
        }
        h ^= self.rejected.len() as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        h
    }
}

/// 冻结表指纹。
pub fn table_fp() -> u64 {
    let t = frozen_table();
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut i = 0usize;
    while i < t.len() {
        h ^= t[i].0 as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        h ^= t[i].1.fg as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        h ^= t[i].1.bg as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        h ^= t[i].1.thick as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        i += 1;
    }
    h
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

/// 主巡检：attr 联签九判据。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 三值全量映射确定：0/1/2 语义互异且与基准相等
    let base = frozen_table();
    let mut s = AttrSigner::new();
    let mut a = 0u8;
    while a < 3 {
        let sem = match s.sign(a) {
            Ok(v) => v,
            Err(_) => return Err(E_MAP_MISS),
        };
        if sem != base[a as usize].1 {
            return Err(E_MAP_MISS);
        }
        a += 1;
    }
    s.audit_full_map()?;
    s.audit_bijective()?;
    hits += 1;

    // R2 冻结指纹恒定
    s.audit_frozen()?;
    if table_fp() != table_fp() {
        return Err(E_FROZEN_BREAK);
    }
    hits += 1;

    // R3 未知 attr 拒签
    let unknown: [u8; 6] = [3, 4, 8, 16, 128, 255];
    let mut i = 0usize;
    while i < unknown.len() {
        match s.sign(unknown[i]) {
            Err(E_UNKNOW_ATTR) => {}
            _ => return Err(E_UNKNOW_ATTR),
        }
        i += 1;
    }
    if s.rejected.len() != 6 {
        return Err(E_LEDGER_MISS);
    }
    hits += 1;

    // R4 attr 序列往返一致：0,1,2,1,0 逐一签出
    let seq: [u8; 5] = [0, 1, 2, 1, 0];
    let mut i = 0usize;
    while i < seq.len() {
        let got = match s.sign(seq[i]) {
            Ok(v) => v,
            Err(_) => return Err(E_ROUNDTRIP),
        };
        if got != lookup(seq[i]).unwrap_or(DrawSem { fg: 9, bg: 9, thick: 9 }) {
            return Err(E_ROUNDTRIP);
        }
        i += 1;
    }
    s.audit_roundtrip()?;
    hits += 1;

    // R5 LCG 随机 attr 流 ×512 全归类零崩
    let mut rng = Lcg::new(0x48564);
    let mut g2 = AttrSigner::new();
    let mut k = 0u32;
    while k < 512 {
        let attr = rng.below(6) as u8; // 0..5：一半合法一半非法
        match g2.sign(attr) {
            Ok(_) => {
                if attr > 2 {
                    return Err(E_KIND_LEAK);
                }
            }
            Err(E_UNKNOW_ATTR) => {
                if attr <= 2 {
                    return Err(E_KIND_LEAK);
                }
            }
            Err(_) => return Err(E_KIND_LEAK),
        }
        k += 1;
    }
    g2.audit_balance()?;
    if g2.signed.len() + g2.rejected.len() != 512 {
        return Err(E_LEDGER_MISS);
    }
    hits += 1;

    // R6 合法占比守恒：attr 0..5 中 0/1/2 合法 → 恰约一半签出
    if g2.signed.len() < 200 || g2.rejected.len() < 200 {
        return Err(E_KIND_LEAK);
    }
    hits += 1;

    // R7 签出条目语义与冻结表逐条一致
    let mut i = 0usize;
    while i < g2.signed.len() {
        let a = g2.signed[i].0;
        if g2.signed[i].1 != lookup(a).unwrap_or(DrawSem { fg: 9, bg: 9, thick: 9 }) {
            return Err(E_MAP_MISS);
        }
        i += 1;
    }
    hits += 1;

    // R8 重演一致：同种子指纹相等
    let fp1 = g2.fingerprint();
    let mut rng2 = Lcg::new(0x48564);
    let mut g3 = AttrSigner::new();
    let mut k = 0u32;
    while k < 512 {
        let attr = rng2.below(6) as u8;
        let _ = g3.sign(attr);
        k += 1;
    }
    if g3.fingerprint() != fp1 || fp1 == 0 {
        return Err(E_REPLAY_DIFF);
    }
    hits += 1;

    // R9 账目守恒终验
    s.audit_balance()?;
    if s.seq != (s.signed.len() + s.rejected.len()) as u64 {
        return Err(E_LEDGER_MISS);
    }
    hits += 1;

    Ok(hits)
}
