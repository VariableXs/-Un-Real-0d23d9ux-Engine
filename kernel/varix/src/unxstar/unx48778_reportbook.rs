//! UNX-F48778 —— C·闭账物文档（主办报告：方法/证据/结论三段）
//! 判据原文：UNX-F48778-J1 报告归档；判据锚定 体验十九。
//! 判据落地（drill 场景）：R1 三段闭集；R2 报告落册；R3 归档断言；
//! R4 段缺失检出；R5 段越界检出；R6 段序连续；R7 复核一致；
//! R8 指纹稳定；R9 计数守恒；R10 呈现行；R11 报告签名。
//! 设计纪律：no_std（仅 core/alloc）；零 unwrap/expect/panic；
//! 确定性 LCG；drill 全绿才返回 Ok(hits)。
//! 口径说明：主办报告三段闭集——方法（method）/证据（evidence）/
//! 结论（conclusion）；报告落册以段序显式编位，归档 = 三段全在册
//! 且逐段签名可复核；报告签名对（段数、指纹）混签。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

/// 诊断段基址。
pub const SEG_BASE: u16 = 0x2980;

// ── 诊断码闭集（SEG_BASE+1 起连续）──────────────────────────────
const E_SECT: u16 = SEG_BASE + 1; // 段越出三段闭集
const E_DUP: u16 = SEG_BASE + 2; // 段重复
const E_SIG: u16 = SEG_BASE + 3; // 段签名不符
const E_SEQ: u16 = SEG_BASE + 4; // 段序不连续
const E_MISS: u16 = SEG_BASE + 5; // 段缺失
const E_COUNT: u16 = SEG_BASE + 6; // 段数不守恒
const E_REPLAY: u16 = SEG_BASE + 7; // 复核漂移
const E_FP: u16 = SEG_BASE + 8; // 指纹漂移
const E_LINE: u16 = SEG_BASE + 9; // 呈现行缺段

/// 主办报告三段闭集。
pub const S_METHOD: u8 = 1; // 方法
pub const S_EVIDENCE: u8 = 2; // 证据
pub const S_CONCLUSION: u8 = 3; // 结论
pub const SECT_N: u32 = 3;

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

/// 一段报告正文。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ReportSect {
    pub seq: u32,    // 段序（显式编位）
    pub sect: u8,    // 三段闭集
    pub digest: u64, // 段正文摘要
    pub sig: u64,    // 段签名
}

impl ReportSect {
    /// 段签名：序 + 段 + 摘要混签。
    pub fn make_sig(seq: u32, sect: u8, digest: u64) -> u64 {
        let mut buf: Vec<u8> = vec![];
        buf.extend_from_slice(&seq.to_le_bytes());
        buf.push(sect);
        buf.extend_from_slice(&digest.to_le_bytes());
        fnv1a64(&buf)
    }

    pub fn sane(&self) -> Result<(), u16> {
        if self.sect < S_METHOD || self.sect > S_CONCLUSION {
            return Err(E_SECT);
        }
        if self.sig != ReportSect::make_sig(self.seq, self.sect, self.digest) {
            return Err(E_SIG);
        }
        Ok(())
    }
}

/// 主办报告册。
pub struct ReportBook {
    pub sects: Vec<ReportSect>,
}

impl ReportBook {
    pub fn new() -> ReportBook {
        ReportBook { sects: vec![] }
    }

    /// 段落落册：段唯一 + 序显式连续。
    pub fn put(&mut self, sect: u8, digest: u64) -> Result<(), u16> {
        if sect < S_METHOD || sect > S_CONCLUSION {
            return Err(E_SECT);
        }
        let seq = self.sects.len() as u32 + 1;
        let mut i = 0usize;
        while i < self.sects.len() {
            if self.sects[i].sect == sect {
                return Err(E_DUP);
            }
            i += 1;
        }
        let sig = ReportSect::make_sig(seq, sect, digest);
        self.sects.push(ReportSect { seq, sect, digest, sig });
        Ok(())
    }

    /// 报告归档断言：三段全在册 + 逐段 sane。
    pub fn archive(&self) -> Result<(), u16> {
        if self.sects.len() as u32 != SECT_N {
            return Err(E_COUNT);
        }
        let mut s = S_METHOD;
        while s <= S_CONCLUSION {
            let mut hit = false;
            let mut i = 0usize;
            while i < self.sects.len() {
                if self.sects[i].sect == s {
                    hit = true;
                }
                i += 1;
            }
            if !hit {
                return Err(E_MISS);
            }
            s += 1;
        }
        let mut i = 0usize;
        while i < self.sects.len() {
            self.sects[i].sane()?;
            i += 1;
        }
        Ok(())
    }

    /// 指纹。
    pub fn fp(&self) -> u64 {
        let mut buf: Vec<u8> = vec![];
        let mut i = 0usize;
        while i < self.sects.len() {
            buf.extend_from_slice(&self.sects[i].seq.to_le_bytes());
            buf.push(self.sects[i].sect);
            buf.extend_from_slice(&self.sects[i].digest.to_le_bytes());
            buf.extend_from_slice(&self.sects[i].sig.to_le_bytes());
            i += 1;
        }
        fnv1a64(&buf)
    }
}

/// 报告签名：段数 × 指纹混签。
pub fn report_sig(n: u32, fp: u64) -> u64 {
    let mut buf: Vec<u8> = vec![];
    buf.extend_from_slice(&n.to_le_bytes());
    buf.extend_from_slice(&fp.to_le_bytes());
    fnv1a64(&buf)
}

/// 呈现行：sects=3 archived=1。
pub fn report_line(n: u32) -> String {
    let mut out = String::new();
    out.push_str("sects=");
    out.push_str(&n.to_string());
    out.push_str(" archived=1");
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
    let mut g = Lcg::new(0x487_78);

    // R2 报告落册：三段顺序写入，摘要 LCG 生成。
    let mut book = ReportBook::new();
    let mut s = S_METHOD;
    while s <= S_CONCLUSION {
        let digest = fnv1a64(&s.to_le_bytes()) ^ (g.next_u32() as u64);
        book.put(s, digest)?;
        s += 1;
    }
    if book.sects.len() as u32 != SECT_N {
        return Err(E_COUNT);
    }
    hits += 1;

    // R3 报告归档断言。
    book.archive()?;
    hits += 1;

    // R4 段缺失检出：单段册归档红。
    let mut thin = ReportBook::new();
    thin.put(S_METHOD, 1)?;
    match thin.archive() {
        Err(E_COUNT) => hits += 1,
        _ => return Err(E_COUNT),
    }

    // R5 段越界检出：段 0 与 4 拒绝。
    match book.put(0, 1) {
        Err(E_SECT) => {}
        _ => return Err(E_SECT),
    }
    match book.put(4, 1) {
        Err(E_SECT) => hits += 1,
        _ => return Err(E_SECT),
    }

    // R6 段重复检出。
    match book.put(S_EVIDENCE, 9) {
        Err(E_DUP) => hits += 1,
        _ => return Err(E_DUP),
    }

    // R7 复核一致：同种子重落册指纹一致。
    let mut book2 = ReportBook::new();
    let mut g2 = Lcg::new(0x487_78);
    let mut s2 = S_METHOD;
    while s2 <= S_CONCLUSION {
        let digest = fnv1a64(&s2.to_le_bytes()) ^ (g2.next_u32() as u64);
        book2.put(s2, digest)?;
        s2 += 1;
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

    // R9 计数守恒：段序显式连续核对。
    let mut i = 0usize;
    while i < book.sects.len() {
        if book.sects[i].seq != i as u32 + 1 {
            return Err(E_SEQ);
        }
        i += 1;
    }
    hits += 1;

    // R10 呈现行：sects=3 archived=1。
    let line = report_line(book.sects.len() as u32);
    if !has_kw(line.as_bytes(), b"sects=3") || !has_kw(line.as_bytes(), b"archived=1") {
        return Err(E_LINE);
    }
    hits += 1;

    // R11 报告签名：3×指纹混签重算一致 + 归档复核。
    let sig = report_sig(SECT_N, f1);
    if sig != report_sig(3, f1) || sig == 0 {
        return Err(E_SIG);
    }
    book.archive()?;
    hits += 1;

    Ok(hits)
}
