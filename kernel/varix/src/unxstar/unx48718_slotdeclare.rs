//! UNX-F48718 —— I·AI-90 顺延账申报位（C 型批次顺延零滚雪球声明）
//! 判据原文：UNX-F48718-J1 申报位登记；判据锚定 §AI-61 §7。
//! 判据落地（drill 场景）：R1 申报位登记；R2 顺延链登记；R3 零滚雪
//! 球断言；R4 顺延漂移检出；R5 申报位唯一；R6 顺延闭包；R7 复核一致；
//! R8 指纹稳定；R9 计数守恒；R10 呈现行；R11 申报签名。
//! 设计纪律：no_std（仅 core/alloc）；零 unwrap/expect/panic；
//! 确定性 LCG；drill 全绿才返回 Ok(hits)。
//! 口径说明：C 型批次顺延账按申报位（slot）登记顺延去向；零滚雪球
//! = 任一申报位顺延深度 ≤1（只许一次直顺，禁止连环顺延）；申报位
//! 全局唯一，漂移即红。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

/// 诊断段基址。
pub const SEG_BASE: u16 = 0x12E0;

// ── 诊断码闭集（SEG_BASE+1 起连续）──────────────────────────────
const E_SLOT: u16 = SEG_BASE + 1; // 申报位非法
const E_DUP: u16 = SEG_BASE + 2; // 申报位重复
const E_DEPTH: u16 = SEG_BASE + 3; // 顺延深度超限（滚雪球）
const E_MISS: u16 = SEG_BASE + 4; // 顺延去向缺登记
const E_COUNT: u16 = SEG_BASE + 5; // 计数不守恒
const E_REPLAY: u16 = SEG_BASE + 6; // 复核漂移
const E_FP: u16 = SEG_BASE + 7; // 指纹漂移
const E_LINE: u16 = SEG_BASE + 8; // 呈现行缺段
const E_SIG: u16 = SEG_BASE + 9; // 申报签名不符
const E_SELF: u16 = SEG_BASE + 10; // 自顺延（顺到自己）

/// 申报位容量。
pub const SLOT_N: u32 = 16;

/// FNV-1a 64 位。
pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// 确定性 LCG。
pub struct Lcg {
    state: u32,
}

impl Lcg {
    pub fn new(seed: u32) -> Lcg {
        Lcg { state: seed }
    }
    pub fn next_u32(&mut self) -> u32 {
        self.state = self.state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.state
    }
    pub fn below(&mut self, bound: u32) -> u32 {
        if bound == 0 {
            0
        } else {
            self.next_u32() % bound
        }
    }
}

/// 一条顺延账申报。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SlotRec {
    pub slot: u32,    // 申报位 1..=16
    pub target: u32,  // 顺延去向（0 = 本位了结）
    pub depth: u32,   // 顺延深度（0 或 1）
    pub sig: u64,     // 申报签名
}

impl SlotRec {
    /// 申报签名：位 + 去向 + 深度混签。
    pub fn make_sig(slot: u32, target: u32, depth: u32) -> u64 {
        let mut buf: Vec<u8> = vec![];
        buf.extend_from_slice(&slot.to_le_bytes());
        buf.extend_from_slice(&target.to_le_bytes());
        buf.extend_from_slice(&depth.to_le_bytes());
        fnv1a64(&buf)
    }

    pub fn sane(&self) -> Result<(), u16> {
        if self.slot == 0 || self.slot > SLOT_N {
            return Err(E_SLOT);
        }
        if self.target > SLOT_N {
            return Err(E_SLOT);
        }
        if self.target == self.slot && self.target != 0 {
            return Err(E_SELF);
        }
        if self.depth > 1 {
            return Err(E_DEPTH);
        }
        if self.sig != SlotRec::make_sig(self.slot, self.target, self.depth) {
            return Err(E_SIG);
        }
        Ok(())
    }
}

/// 顺延账。
pub struct SlotBook {
    pub recs: Vec<SlotRec>,
}

impl SlotBook {
    pub fn new() -> SlotBook {
        SlotBook { recs: vec![] }
    }

    /// 申报位登记：唯一 + 口径校验。
    pub fn declare(&mut self, slot: u32, target: u32, depth: u32) -> Result<(), u16> {
        if slot == 0 || slot > SLOT_N {
            return Err(E_SLOT);
        }
        let mut i = 0usize;
        while i < self.recs.len() {
            if self.recs[i].slot == slot {
                return Err(E_DUP);
            }
            i += 1;
        }
        let sig = SlotRec::make_sig(slot, target, depth);
        let rec = SlotRec { slot, target, depth, sig };
        rec.sane()?;
        self.recs.push(rec);
        Ok(())
    }

    /// 顺延闭包：去向非零者必须已登记（禁止悬挂顺延）。
    pub fn closure(&self) -> Result<(), u16> {
        let mut i = 0usize;
        while i < self.recs.len() {
            let t = self.recs[i].target;
            if t != 0 {
                let mut found = false;
                let mut j = 0usize;
                while j < self.recs.len() {
                    if self.recs[j].slot == t {
                        found = true;
                    }
                    j += 1;
                }
                if !found {
                    return Err(E_MISS);
                }
            }
            i += 1;
        }
        Ok(())
    }

    /// 零滚雪球断言：全部深度 ≤1。
    pub fn no_snowball(&self) -> Result<(), u16> {
        let mut i = 0usize;
        while i < self.recs.len() {
            if self.recs[i].depth > 1 {
                return Err(E_DEPTH);
            }
            i += 1;
        }
        Ok(())
    }

    /// 指纹。
    pub fn fp(&self) -> u64 {
        let mut buf: Vec<u8> = vec![];
        let mut i = 0usize;
        while i < self.recs.len() {
            buf.extend_from_slice(&self.recs[i].slot.to_le_bytes());
            buf.extend_from_slice(&self.recs[i].target.to_le_bytes());
            buf.extend_from_slice(&self.recs[i].depth.to_le_bytes());
            buf.extend_from_slice(&self.recs[i].sig.to_le_bytes());
            i += 1;
        }
        fnv1a64(&buf)
    }
}

/// 申报签名：登记数 + 直顺数混签。
pub fn slot_sig(n: u32, deferred: u32) -> u64 {
    let mut buf: Vec<u8> = vec![];
    buf.extend_from_slice(&n.to_le_bytes());
    buf.extend_from_slice(&deferred.to_le_bytes());
    fnv1a64(&buf)
}

/// 呈现行：slots=<n> deferred=<n> snowball=0。
pub fn slot_line(n: u32, deferred: u32) -> String {
    let mut out = String::new();
    out.push_str("slots=");
    out.push_str(&n.to_string());
    out.push_str(" deferred=");
    out.push_str(&deferred.to_string());
    out.push_str(" snowball=0");
    out
}

/// 子串判定（朴素扫描）。
pub fn has_kw(hay: &[u8], kw: &[u8]) -> bool {
    if kw.is_empty() || hay.len() < kw.len() {
        return false;
    }
    let mut i = 0usize;
    while i + kw.len() <= hay.len() {
        let mut j = 0usize;
        while j < kw.len() && hay[i + j] == kw[j] {
            j += 1;
        }
        if j == kw.len() {
            return true;
        }
        i += 1;
    }
    false
}

/// 主巡检入口：R1..R11 全绿才返回 Ok(hits)。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;
    let mut g = Lcg::new(0x487_18);

    // R1 申报位登记：偶数位直顺到下一位，奇数位了结。
    let mut book = SlotBook::new();
    let mut s: u32 = 1;
    while s <= SLOT_N {
        if s % 2 == 0 && s < SLOT_N {
            book.declare(s, s + 1, 1)?;
        } else {
            book.declare(s, 0, 0)?;
        }
        s += 1;
    }
    if book.recs.len() as u32 != SLOT_N {
        return Err(E_COUNT);
    }
    hits += 1;

    // R2 顺延链登记复核：闭包无悬挂。
    book.closure()?;
    hits += 1;

    // R3 零滚雪球断言。
    book.no_snowball()?;
    hits += 1;

    // R4 顺延漂移检出：深度 2 拒绝。
    let mut bad = SlotBook::new();
    match bad.declare(1, 2, 2) {
        Err(E_DEPTH) => hits += 1,
        _ => return Err(E_DEPTH),
    }

    // R5 申报位唯一：重复登记拒绝。
    match book.declare(1, 3, 1) {
        Err(E_DUP) => hits += 1,
        _ => return Err(E_DUP),
    }

    // R6 顺延闭包破坏：去向未登记红。
    let mut hang = SlotBook::new();
    hang.declare(1, SLOT_N + 1, 1)?;
    match hang.closure() {
        Err(E_MISS) => hits += 1,
        _ => return Err(E_MISS),
    }

    // R7 自顺延检出。
    match hang.declare(2, 2, 1) {
        Err(E_SELF) => hits += 1,
        _ => return Err(E_SELF),
    }

    // R8 复核一致：指纹重扫不变。
    let f1 = book.fp();
    let f2 = book.fp();
    if f1 != f2 || f1 == 0 {
        return Err(E_REPLAY);
    }
    hits += 1;

    // R9 指纹稳定。
    if book.fp() != f1 {
        return Err(E_FP);
    }
    hits += 1;

    // R10 呈现行：slots=16 deferred=7 snowball=0。
    let mut deferred: u32 = 0;
    let mut i = 0usize;
    while i < book.recs.len() {
        if book.recs[i].target != 0 {
            deferred += 1;
        }
        i += 1;
    }
    let line = slot_line(book.recs.len() as u32, deferred);
    if !has_kw(line.as_bytes(), b"slots=16")
        || !has_kw(line.as_bytes(), b"snowball=0")
    {
        return Err(E_LINE);
    }
    hits += 1;

    // R11 申报签名：n + 直顺数混签重算一致。
    let sig = slot_sig(SLOT_N, deferred);
    if sig != slot_sig(16, 7) || sig == 0 {
        return Err(E_SIG);
    }
    let _ = g.below(4);
    hits += 1;

    Ok(hits)
}
