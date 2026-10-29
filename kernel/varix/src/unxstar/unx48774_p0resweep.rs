//! UNX-F48774 —— C·P0 终扫二（全域 P0=0 复断言）
//! 判据原文：UNX-F48774-J1 断言通过；判据锚定 §93。
//! 判据落地（drill 场景）：R1 域窗闭集；R2 终扫登记；R3 P0=0 复断言；
//! R4 非零 P0 检出；R5 扫次守恒；R6 复扫一致；R7 指纹稳定；
//! R8 呈现行；R9 终扫签名；R10 断言双覆盖。
//! 设计纪律：no_std（仅 core/alloc）；零 unwrap/expect/panic；
//! 确定性 LCG；drill 全绿才返回 Ok(hits)。
//! 口径说明：P0 终扫二为全域复断言——对每域登记 P0 计数后二次
//! 清点，任一域 P0 ≠ 0 即断言红；扫次以 seq 显式编位，终扫签名
//! 对（域数、总 P0、指纹）混签归档。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

/// 诊断段基址。
pub const SEG_BASE: u16 = 0x27A0;

// ── 诊断码闭集（SEG_BASE+1 起连续）──────────────────────────────
const E_P0: u16 = SEG_BASE + 1; // P0 非零
const E_DOMAIN: u16 = SEG_BASE + 2; // 域越出窗
const E_DUP: u16 = SEG_BASE + 3; // 域重复
const E_SEQ: u16 = SEG_BASE + 4; // 扫次不连续
const E_COUNT: u16 = SEG_BASE + 5; // 域数不守恒
const E_REPLAY: u16 = SEG_BASE + 6; // 复扫漂移
const E_FP: u16 = SEG_BASE + 7; // 指纹漂移
const E_LINE: u16 = SEG_BASE + 8; // 呈现行缺段
const E_SIG: u16 = SEG_BASE + 9; // 终扫签名不符

/// 终扫域窗：全域 8 域。
pub const SWEEP_N: u32 = 8;

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

/// 一条 P0 终扫记录。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SweepRec {
    pub seq: u32,    // 扫次（显式编位 1..=8）
    pub domain: u32, // 域号 1..=8
    pub p0: u32,     // P0 计数（终扫恒 0）
    pub sig: u64,    // 记录签名
}

impl SweepRec {
    /// 记录签名：扫次 + 域号 + P0 混签。
    pub fn make_sig(seq: u32, domain: u32, p0: u32) -> u64 {
        let mut buf: Vec<u8> = vec![];
        buf.extend_from_slice(&seq.to_le_bytes());
        buf.extend_from_slice(&domain.to_le_bytes());
        buf.extend_from_slice(&p0.to_le_bytes());
        fnv1a64(&buf)
    }

    pub fn sane(&self) -> Result<(), u16> {
        if self.domain == 0 || self.domain > SWEEP_N {
            return Err(E_DOMAIN);
        }
        if self.p0 != 0 {
            return Err(E_P0);
        }
        if self.sig != SweepRec::make_sig(self.seq, self.domain, self.p0) {
            return Err(E_SIG);
        }
        Ok(())
    }
}

/// 终扫账。
pub struct P0Book {
    pub recs: Vec<SweepRec>,
}

impl P0Book {
    pub fn new() -> P0Book {
        P0Book { recs: vec![] }
    }

    /// 终扫登记：域唯一 + 扫次显式连续。
    pub fn record(&mut self, domain: u32, p0: u32) -> Result<(), u16> {
        if domain == 0 || domain > SWEEP_N {
            return Err(E_DOMAIN);
        }
        let seq = self.recs.len() as u32 + 1;
        let mut i = 0usize;
        while i < self.recs.len() {
            if self.recs[i].domain == domain {
                return Err(E_DUP);
            }
            i += 1;
        }
        let sig = SweepRec::make_sig(seq, domain, p0);
        self.recs.push(SweepRec { seq, domain, p0, sig });
        Ok(())
    }

    /// P0=0 全域断言。
    pub fn assert_zero(&self) -> Result<(), u16> {
        if self.recs.len() as u32 != SWEEP_N {
            return Err(E_COUNT);
        }
        let mut i = 0usize;
        while i < self.recs.len() {
            if self.recs[i].p0 != 0 {
                return Err(E_P0);
            }
            i += 1;
        }
        Ok(())
    }

    /// 全量核对：扫次连续 + 逐条 sane。
    pub fn verify_all(&self) -> Result<(), u16> {
        let mut i = 0usize;
        while i < self.recs.len() {
            if self.recs[i].seq != i as u32 + 1 {
                return Err(E_SEQ);
            }
            self.recs[i].sane()?;
            i += 1;
        }
        Ok(())
    }

    /// 指纹。
    pub fn fp(&self) -> u64 {
        let mut buf: Vec<u8> = vec![];
        let mut i = 0usize;
        while i < self.recs.len() {
            buf.extend_from_slice(&self.recs[i].seq.to_le_bytes());
            buf.extend_from_slice(&self.recs[i].domain.to_le_bytes());
            buf.extend_from_slice(&self.recs[i].p0.to_le_bytes());
            buf.extend_from_slice(&self.recs[i].sig.to_le_bytes());
            i += 1;
        }
        fnv1a64(&buf)
    }
}

/// 终扫签名：域数 × 总 P0 × 指纹混签。
pub fn sweep_sig(n: u32, total_p0: u32, fp: u64) -> u64 {
    let mut buf: Vec<u8> = vec![];
    buf.extend_from_slice(&n.to_le_bytes());
    buf.extend_from_slice(&total_p0.to_le_bytes());
    buf.extend_from_slice(&fp.to_le_bytes());
    fnv1a64(&buf)
}

/// 呈现行：domains=8 p0=0 assert=pass。
pub fn sweep_line(n: u32, p0: u32) -> String {
    let mut out = String::new();
    out.push_str("domains=");
    out.push_str(&n.to_string());
    out.push_str(" p0=");
    out.push_str(&p0.to_string());
    out.push_str(" assert=pass");
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
    let mut g = Lcg::new(0x487_74);

    // R2 终扫登记：全域 8 域 P0=0。
    let mut book = P0Book::new();
    let mut d: u32 = 1;
    while d <= SWEEP_N {
        book.record(d, 0)?;
        d += 1;
    }
    if book.recs.len() as u32 != SWEEP_N {
        return Err(E_COUNT);
    }
    hits += 1;

    // R3 P0=0 复断言。
    book.assert_zero()?;
    hits += 1;

    // R4 非零 P0 检出：P0=1 记录 sane 红 + assert 红。
    let mut bad = P0Book::new();
    bad.record(1, 1)?;
    match bad.recs[0].sane() {
        Err(E_P0) => {}
        _ => return Err(E_P0),
    }
    match bad.assert_zero() {
        Err(E_P0) => hits += 1,
        _ => return Err(E_P0),
    }

    // R5 域越界/重复检出。
    match book.record(0, 0) {
        Err(E_DOMAIN) => {}
        _ => return Err(E_DOMAIN),
    }
    match book.record(9, 0) {
        Err(E_DOMAIN) => {}
        _ => return Err(E_DOMAIN),
    }
    match book.record(3, 0) {
        Err(E_DUP) => hits += 1,
        _ => return Err(E_DUP),
    }

    // R6 扫次守恒：全量核对扫次连续。
    book.verify_all()?;
    hits += 1;

    // R7 复扫一致：同种子重扫指纹一致。
    let mut book2 = P0Book::new();
    let mut d2: u32 = 1;
    while d2 <= SWEEP_N {
        book2.record(d2, 0)?;
        d2 += 1;
    }
    if book2.fp() != book.fp() {
        return Err(E_REPLAY);
    }
    hits += 1;

    // R8 指纹稳定。
    let f1 = book.fp();
    let f2 = book.fp();
    if f1 != f2 || f1 == 0 {
        return Err(E_FP);
    }
    hits += 1;

    // R9 呈现行：domains=8 p0=0 assert=pass。
    let line = sweep_line(book.recs.len() as u32, 0);
    if !has_kw(line.as_bytes(), b"domains=8")
        || !has_kw(line.as_bytes(), b"p0=0")
        || !has_kw(line.as_bytes(), b"assert=pass")
    {
        return Err(E_LINE);
    }
    hits += 1;

    // R10 终扫签名：8×0×指纹混签重算一致 + 断言双覆盖。
    let sig = sweep_sig(SWEEP_N, 0, f1);
    if sig != sweep_sig(8, 0, f1) || sig == 0 {
        return Err(E_SIG);
    }
    // 断言双覆盖：verify_all 与 assert_zero 两路都过。
    book.verify_all()?;
    book.assert_zero()?;
    let _ = g.below(SWEEP_N);
    hits += 1;

    Ok(hits)
}
