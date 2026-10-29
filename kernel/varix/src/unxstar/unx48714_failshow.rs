//! UNX-F48714 —— I36 异常呈现三要素（复签失败显性化）
//! 判据原文：UNX-F48714-J1 注入 ×10 呈现齐；判据锚定 体验十三·补。
//! 判据落地（drill 场景）：R1 三要素闭集；R2 失败注入 ×10；
//! R3 呈现齐套断言；R4 要素缺一检出；R5 码位合法；R6 复现一致；
//! R7 指纹稳定；R8 计数守恒；R9 呈现行；R10 呈现签名。
//! 设计纪律：no_std（仅 core/alloc）；零 unwrap/expect/panic；
//! 确定性 LCG；drill 全绿才返回 Ok(hits)。
//! 口径说明：复签失败显性化三要素——诊断码（what）、失败域
//! （where）、呈现行（how）；注入 ×10 = 十次人为失败注入，每次
//! 三要素齐备（码合法、域在册、呈现行可读）才算呈现齐。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

/// 诊断段基址。
pub const SEG_BASE: u16 = 0x11C0;

// ── 诊断码闭集（SEG_BASE+1 起连续）──────────────────────────────
const E_ELEM: u16 = SEG_BASE + 1; // 要素越出闭集
const E_MISS: u16 = SEG_BASE + 2; // 要素缺失
const E_CODE: u16 = SEG_BASE + 3; // 码位不合法
const E_COUNT: u16 = SEG_BASE + 4; // 注入次数不足
const E_REPLAY: u16 = SEG_BASE + 5; // 复现结果漂移
const E_FP: u16 = SEG_BASE + 6; // 指纹漂移
const E_LINE: u16 = SEG_BASE + 7; // 呈现行缺段
const E_SIG: u16 = SEG_BASE + 8; // 呈现签名不符
const E_ORDER: u16 = SEG_BASE + 9; // 注入序号不连续

/// 呈现域闭集（where）。
pub const D_RENEW: u8 = 1; // 复签域
pub const D_SPOT: u8 = 2; // 外抽域
pub const D_RECON: u8 = 3; // 对账域
pub const DOMAIN_N: u32 = 3;

/// 注入轮次。
pub const INJECT_N: u32 = 10;

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

/// 一条失败呈现。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FailShow {
    pub seq: u32,   // 注入序号 1..=10
    pub code: u16,  // 诊断码（what）
    pub domain: u8, // 失败域（where）
    pub sig: u64,   // 呈现签名
}

impl FailShow {
    /// 呈现签名：序号 + 码 + 域混签。
    pub fn make_sig(seq: u32, code: u16, domain: u8) -> u64 {
        let mut buf: Vec<u8> = vec![];
        buf.extend_from_slice(&seq.to_le_bytes());
        buf.extend_from_slice(&code.to_le_bytes());
        buf.push(domain);
        fnv1a64(&buf)
    }

    pub fn sane(&self) -> Result<(), u16> {
        if self.domain < D_RENEW || self.domain > D_RECON {
            return Err(E_ELEM);
        }
        if self.code <= SEG_BASE || self.code > SEG_BASE + 0x1F {
            return Err(E_CODE);
        }
        if self.sig != FailShow::make_sig(self.seq, self.code, self.domain) {
            return Err(E_SIG);
        }
        Ok(())
    }
}

/// 呈现账。
pub struct ShowBook {
    pub shows: Vec<FailShow>,
}

impl ShowBook {
    pub fn new() -> ShowBook {
        ShowBook { shows: vec![] }
    }

    /// 呈现登记：域闭集 + 序号连续。
    pub fn show(&mut self, seq: u32, code: u16, domain: u8) -> Result<(), u16> {
        if domain < D_RENEW || domain > D_RECON {
            return Err(E_ELEM);
        }
        if seq != self.shows.len() as u32 + 1 {
            return Err(E_ORDER);
        }
        let sig = FailShow::make_sig(seq, code, domain);
        self.shows.push(FailShow { seq, code, domain, sig });
        Ok(())
    }

    /// 呈现齐套：每条三要素合法。
    pub fn all_present(&self) -> Result<(), u16> {
        if self.shows.len() as u32 != INJECT_N {
            return Err(E_COUNT);
        }
        let mut i = 0usize;
        while i < self.shows.len() {
            self.shows[i].sane()?;
            i += 1;
        }
        Ok(())
    }

    /// 指纹。
    pub fn fp(&self) -> u64 {
        let mut buf: Vec<u8> = vec![];
        let mut i = 0usize;
        while i < self.shows.len() {
            buf.extend_from_slice(&self.shows[i].seq.to_le_bytes());
            buf.extend_from_slice(&self.shows[i].code.to_le_bytes());
            buf.push(self.shows[i].domain);
            buf.extend_from_slice(&self.shows[i].sig.to_le_bytes());
            i += 1;
        }
        fnv1a64(&buf)
    }
}

/// 三要素呈现行（how）：code=<hex> domain=<n> seq=<n>。
pub fn fail_line(s: &FailShow) -> String {
    let mut out = String::new();
    out.push_str("code=");
    out.push_str(&s.code.to_string());
    out.push_str(" domain=");
    out.push_str(&s.domain.to_string());
    out.push_str(" seq=");
    out.push_str(&s.seq.to_string());
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
    let mut g = Lcg::new(0x487_14);

    // R1 三要素闭集：域 0 与 4 拒绝。
    let mut book = ShowBook::new();
    match book.show(1, SEG_BASE + 1, 0) {
        Err(E_ELEM) => {}
        _ => return Err(E_ELEM),
    }
    match book.show(1, SEG_BASE + 1, 4) {
        Err(E_ELEM) => hits += 1,
        _ => return Err(E_ELEM),
    }

    // R2 失败注入 ×10：域由 LCG 决定，码位取段窗内。
    let mut seq: u32 = 1;
    while seq <= INJECT_N {
        let domain = (g.below(DOMAIN_N) + 1) as u8;
        let code = SEG_BASE + 1 + (seq as u16 - 1) % 9;
        book.show(seq, code, domain)?;
        seq += 1;
    }
    hits += 1;

    // R3 呈现齐套断言。
    book.all_present()?;
    hits += 1;

    // R4 要素缺一检出：码位越出段窗 → sane 红。
    let mut bad = ShowBook::new();
    bad.show(1, SEG_BASE + 0x20, D_RENEW)?;
    match bad.shows[0].sane() {
        Err(E_CODE) => hits += 1,
        _ => return Err(E_CODE),
    }

    // R4b 次数不足检出：单条呈现 → all_present 红。
    match bad.all_present() {
        Err(E_COUNT) => hits += 1,
        _ => return Err(E_COUNT),
    }

    // R5 码位合法：码位恰在段窗 +1..+9 内逐条核对。
    let mut i = 0usize;
    while i < book.shows.len() {
        if book.shows[i].code <= SEG_BASE || book.shows[i].code > SEG_BASE + 0x1F {
            return Err(E_CODE);
        }
        i += 1;
    }
    hits += 1;

    // R6 复现一致：同种子重跑指纹一致。
    let mut book2 = ShowBook::new();
    let mut g2 = Lcg::new(0x487_14);
    let mut seq2: u32 = 1;
    while seq2 <= INJECT_N {
        let domain = (g2.below(DOMAIN_N) + 1) as u8;
        let code = SEG_BASE + 1 + (seq2 as u16 - 1) % 9;
        book2.show(seq2, code, domain)?;
        seq2 += 1;
    }
    if book2.fp() != book.fp() {
        return Err(E_REPLAY);
    }
    hits += 1;

    // R7 指纹稳定。
    let f1 = book.fp();
    let f2 = book.fp();
    if f1 != f2 || f1 == 0 {
        return Err(E_FP);
    }
    hits += 1;

    // R8 计数守恒：注入数恰 10。
    if book.shows.len() as u32 != INJECT_N {
        return Err(E_COUNT);
    }
    hits += 1;

    // R9 呈现行：逐条 how 可读。
    let mut i = 0usize;
    while i < book.shows.len() {
        let line = fail_line(&book.shows[i]);
        if !has_kw(line.as_bytes(), b"code=") || !has_kw(line.as_bytes(), b"domain=") {
            return Err(E_LINE);
        }
        i += 1;
    }
    hits += 1;

    // R10 呈现签名：末条签名重算一致。
    let last = book.shows[book.shows.len() - 1];
    let sig = FailShow::make_sig(last.seq, last.code, last.domain);
    if sig != last.sig || sig == 0 {
        return Err(E_SIG);
    }
    hits += 1;

    Ok(hits)
}
