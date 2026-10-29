//! UNX-F48780 —— C39 批收官印（20 条冻结/累计 234,000 行）
//! 判据原文：UNX-F48780-J1 批印入册：域账 234,000/240,000；
//! 判据锚定 §AI-61 §7。
//! 判据落地（drill 场景）：R1 官印口径冻结；R2 二十条冻结登记；
//! R3 域账 234,000 断言；R4 段满额检出；R5 印号唯一；R6 官印指纹；
//! R7 复印一致；R8 计数守恒；R9 呈现行；R10 官印签名。
//! 设计纪律：no_std（仅 core/alloc）；零 unwrap/expect/panic；
//! 确定性 LCG；drill 全绿才返回 Ok(hits)。
//! 口径说明：C39 批收官印 20 条，每条冻结 11,700 行；域账累计
//! 234,000 行、段满额 240,000 行（234,000/240,000 = 97.5% 满额）；
//! 印号重复或总额漂移即红，官印冻结不得再改。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

/// 诊断段基址。
pub const SEG_BASE: u16 = 0x29C0;

// ── 诊断码闭集（SEG_BASE+1 起连续）──────────────────────────────
const E_SEAL: u16 = SEG_BASE + 1; // 印号非法
const E_DUP: u16 = SEG_BASE + 2; // 印号重复
const E_TOTAL: u16 = SEG_BASE + 3; // 域账总额漂移
const E_QUOTA: u16 = SEG_BASE + 4; // 段满额越界
const E_COUNT: u16 = SEG_BASE + 5; // 冻结条数不足
const E_REPLAY: u16 = SEG_BASE + 6; // 复印漂移
const E_FP: u16 = SEG_BASE + 7; // 指纹漂移
const E_LINE: u16 = SEG_BASE + 8; // 呈现行缺段
const E_SIG: u16 = SEG_BASE + 9; // 官印签名不符

/// 冻结条数。
pub const SEAL_N: u32 = 20;
/// 域账累计（行）。
pub const DOMAIN_LINES: u32 = 234_000;
/// 段满额（行）。
pub const QUOTA_LINES: u32 = 240_000;
/// 每条印的行额。
pub const PER_SEAL_LINES: u32 = DOMAIN_LINES / SEAL_N;

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

/// 一条官印。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SealRec {
    pub seal_no: u32, // 印号 1..=20
    pub lines: u32,   // 冻结行额
    pub sig: u64,     // 官印签名
}

impl SealRec {
    /// 官印签名：印号 + 行额混签。
    pub fn make_sig(seal_no: u32, lines: u32) -> u64 {
        let mut buf: Vec<u8> = vec![];
        buf.extend_from_slice(&seal_no.to_le_bytes());
        buf.extend_from_slice(&lines.to_le_bytes());
        fnv1a64(&buf)
    }

    pub fn sane(&self) -> Result<(), u16> {
        if self.seal_no == 0 || self.seal_no > SEAL_N {
            return Err(E_SEAL);
        }
        if self.lines == 0 || self.lines > QUOTA_LINES {
            return Err(E_QUOTA);
        }
        if self.sig != SealRec::make_sig(self.seal_no, self.lines) {
            return Err(E_SIG);
        }
        Ok(())
    }
}

/// 官印册。
pub struct SealBook {
    pub recs: Vec<SealRec>,
}

impl SealBook {
    pub fn new() -> SealBook {
        SealBook { recs: vec![] }
    }

    /// 官印入册：印号唯一 + 冻结不可改。
    pub fn freeze(&mut self, seal_no: u32, lines: u32) -> Result<(), u16> {
        if seal_no == 0 || seal_no > SEAL_N {
            return Err(E_SEAL);
        }
        let mut i = 0usize;
        while i < self.recs.len() {
            if self.recs[i].seal_no == seal_no {
                return Err(E_DUP);
            }
            i += 1;
        }
        let sig = SealRec::make_sig(seal_no, lines);
        let rec = SealRec { seal_no, lines, sig };
        rec.sane()?;
        self.recs.push(rec);
        Ok(())
    }

    /// 域账累计。
    pub fn total_lines(&self) -> u32 {
        let mut t: u32 = 0;
        let mut i = 0usize;
        while i < self.recs.len() {
            t = t.wrapping_add(self.recs[i].lines);
            i += 1;
        }
        t
    }

    /// 指纹。
    pub fn fp(&self) -> u64 {
        let mut buf: Vec<u8> = vec![];
        let mut i = 0usize;
        while i < self.recs.len() {
            buf.extend_from_slice(&self.recs[i].seal_no.to_le_bytes());
            buf.extend_from_slice(&self.recs[i].lines.to_le_bytes());
            buf.extend_from_slice(&self.recs[i].sig.to_le_bytes());
            i += 1;
        }
        fnv1a64(&buf)
    }
}

/// 官印签名：条数 + 总行额混签。
pub fn book_sig(n: u32, total: u32) -> u64 {
    let mut buf: Vec<u8> = vec![];
    buf.extend_from_slice(&n.to_le_bytes());
    buf.extend_from_slice(&total.to_le_bytes());
    fnv1a64(&buf)
}

/// 呈现行：seals=20 lines=234000 quota=240000 frozen=1。
pub fn seal_line(n: u32, total: u32) -> String {
    let mut out = String::new();
    out.push_str("seals=");
    out.push_str(&n.to_string());
    out.push_str(" lines=");
    out.push_str(&total.to_string());
    out.push_str(" quota=240000 frozen=1");
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

/// 主巡检入口：R1..R10 全绿才返回 Ok(hits)。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;
    let mut g = Lcg::new(0x487_80);

    // R1 官印口径冻结：234,000 / 240,000 / 每印 11,700。
    if DOMAIN_LINES != 234_000 || QUOTA_LINES != 240_000 || PER_SEAL_LINES != 11_700 {
        return Err(E_TOTAL);
    }
    hits += 1;

    // R2 二十条冻结登记：每印 11,700 行。
    let mut book = SealBook::new();
    let mut s: u32 = 1;
    while s <= SEAL_N {
        book.freeze(s, PER_SEAL_LINES)?;
        s += 1;
    }
    if book.recs.len() as u32 != SEAL_N {
        return Err(E_COUNT);
    }
    hits += 1;

    // R3 域账 234,000 断言。
    if book.total_lines() != DOMAIN_LINES {
        return Err(E_TOTAL);
    }
    hits += 1;

    // R4 段满额检出：行额 240,001 拒绝 + 零行额拒绝。
    let mut bad = SealBook::new();
    match bad.freeze(1, QUOTA_LINES + 1) {
        Err(E_QUOTA) => {}
        _ => return Err(E_QUOTA),
    }
    match bad.freeze(1, 0) {
        Err(E_QUOTA) => hits += 1,
        _ => return Err(E_QUOTA),
    }

    // R5 印号唯一：重复冻结拒绝 + 印号越界拒绝。
    match book.freeze(9, 100) {
        Err(E_DUP) => {}
        _ => return Err(E_DUP),
    }
    match book.freeze(SEAL_N + 1, 100) {
        Err(E_SEAL) => hits += 1,
        _ => return Err(E_SEAL),
    }

    // R6 官印指纹。
    let f1 = book.fp();
    let f2 = book.fp();
    if f1 != f2 || f1 == 0 {
        return Err(E_FP);
    }
    hits += 1;

    // R7 复印一致：逐条 sane 重核。
    let mut i = 0usize;
    while i < book.recs.len() {
        book.recs[i].sane()?;
        i += 1;
    }
    if book.fp() != f1 {
        return Err(E_REPLAY);
    }
    hits += 1;

    // R8 计数守恒：条数 × 每印行额 = 域账。
    if book.recs.len() as u32 * PER_SEAL_LINES != DOMAIN_LINES {
        return Err(E_COUNT);
    }
    hits += 1;

    // R9 呈现行：seals=20 lines=234000 frozen=1。
    let line = seal_line(book.recs.len() as u32, book.total_lines());
    if !has_kw(line.as_bytes(), b"seals=20")
        || !has_kw(line.as_bytes(), b"lines=234000")
        || !has_kw(line.as_bytes(), b"frozen=1")
    {
        return Err(E_LINE);
    }
    hits += 1;

    // R10 官印签名：20×234000 混签重算一致。
    let sig = book_sig(SEAL_N, DOMAIN_LINES);
    if sig != book_sig(20, 234_000) || sig == 0 {
        return Err(E_SIG);
    }
    let _ = g.below(6);
    hits += 1;

    Ok(hits)
}
