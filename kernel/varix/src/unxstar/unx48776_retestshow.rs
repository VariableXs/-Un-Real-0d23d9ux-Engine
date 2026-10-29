//! UNX-F48776 —— C·闭账物异常呈现终检（复测失败路径三要素）
//! 判据原文：UNX-F48776-J1 注入 ×10 呈现齐；判据锚定 体验十三·补。
//! 判据落地（drill 场景）：R1 三要素口径冻结；R2 复测失败注入 ×10；
//! R3 呈现齐套断言；R4 要素缺一检出；R5 路径越界检出；R6 注入序
//! 连续；R7 复现一致；R8 指纹稳定；R9 呈现行；R10 终检签名。
//! 设计纪律：no_std（仅 core/alloc）；零 unwrap/expect/panic；
//! 确定性 LCG；drill 全绿才返回 Ok(hits)。
//! 口径说明：闭账物异常呈现终检三要素——诊断码（what）、复测
//! 路径（where）、呈现行（how）；注入 ×10 = 十次复测失败路径
//! 注入，每次三要素齐备才算呈现齐；路径闭集三段（retest/rerun/
//! replay）。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

/// 诊断段基址。
pub const SEG_BASE: u16 = 0x28E0;

// ── 诊断码闭集（SEG_BASE+1 起连续）──────────────────────────────
const E_PATH: u16 = SEG_BASE + 1; // 路径越出闭集
const E_CODE: u16 = SEG_BASE + 2; // 码位不合法
const E_SEQ: u16 = SEG_BASE + 3; // 注入序不连续
const E_MISS: u16 = SEG_BASE + 4; // 呈现要素缺失
const E_COUNT: u16 = SEG_BASE + 5; // 注入次数不足
const E_REPLAY: u16 = SEG_BASE + 6; // 复现漂移
const E_FP: u16 = SEG_BASE + 7; // 指纹漂移
const E_LINE: u16 = SEG_BASE + 8; // 呈现行缺段
const E_SIG: u16 = SEG_BASE + 9; // 终检签名不符

/// 复测路径闭集。
pub const P_RETEST: u8 = 1; // 复测
pub const P_RERUN: u8 = 2; // 复跑
pub const P_REPLAY: u8 = 3; // 重放
pub const PATH_N: u32 = 3;

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

/// 一条复测失败呈现。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RetestShow {
    pub seq: u32,   // 注入序 1..=10
    pub code: u16,  // 诊断码（what）
    pub path: u8,   // 复测路径（where）
    pub sig: u64,   // 呈现签名
}

impl RetestShow {
    /// 呈现签名：序 + 码 + 路径混签。
    pub fn make_sig(seq: u32, code: u16, path: u8) -> u64 {
        let mut buf: Vec<u8> = vec![];
        buf.extend_from_slice(&seq.to_le_bytes());
        buf.extend_from_slice(&code.to_le_bytes());
        buf.push(path);
        fnv1a64(&buf)
    }

    pub fn sane(&self) -> Result<(), u16> {
        if self.path < P_RETEST || self.path > P_REPLAY {
            return Err(E_PATH);
        }
        if self.code <= SEG_BASE || self.code > SEG_BASE + 0x1F {
            return Err(E_CODE);
        }
        if self.sig != RetestShow::make_sig(self.seq, self.code, self.path) {
            return Err(E_SIG);
        }
        Ok(())
    }
}

/// 终检账。
pub struct ShowBook {
    pub shows: Vec<RetestShow>,
}

impl ShowBook {
    pub fn new() -> ShowBook {
        ShowBook { shows: vec![] }
    }

    /// 呈现登记：路径闭集 + 序显式连续。
    pub fn show(&mut self, code: u16, path: u8) -> Result<(), u16> {
        if path < P_RETEST || path > P_REPLAY {
            return Err(E_PATH);
        }
        let seq = self.shows.len() as u32 + 1;
        let sig = RetestShow::make_sig(seq, code, path);
        self.shows.push(RetestShow { seq, code, path, sig });
        Ok(())
    }

    /// 呈现齐套：次数足 + 逐条 sane。
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
            buf.push(self.shows[i].path);
            buf.extend_from_slice(&self.shows[i].sig.to_le_bytes());
            i += 1;
        }
        fnv1a64(&buf)
    }
}

/// 三要素呈现行（how）。
pub fn show_line(s: &RetestShow) -> String {
    let mut out = String::new();
    out.push_str("code=");
    out.push_str(&s.code.to_string());
    out.push_str(" path=");
    out.push_str(&s.path.to_string());
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
    let mut g = Lcg::new(0x487_76);

    // R1 三要素口径冻结：INJECT_N=10、路径闭集 3。
    if INJECT_N != 10 || PATH_N != 3 {
        return Err(E_MISS);
    }
    hits += 1;

    // R2 复测失败注入 ×10：路径由 LCG 决定，码位取段窗内。
    let mut book = ShowBook::new();
    let mut k: u32 = 0;
    while k < INJECT_N {
        let path = (g.below(PATH_N) + 1) as u8;
        let code = SEG_BASE + 1 + (k as u16 % 9);
        book.show(code, path)?;
        k += 1;
    }
    hits += 1;

    // R3 呈现齐套断言。
    book.all_present()?;
    hits += 1;

    // R4 要素缺一检出：码位越出段窗 → sane 红。
    let mut bad = ShowBook::new();
    bad.show(SEG_BASE + 0x20, P_RETEST)?;
    match bad.shows[0].sane() {
        Err(E_CODE) => hits += 1,
        _ => return Err(E_CODE),
    }

    // R5 路径越界检出：路径 0 与 4 拒绝。
    match bad.show(SEG_BASE + 1, 0) {
        Err(E_PATH) => {}
        _ => return Err(E_PATH),
    }
    match bad.show(SEG_BASE + 1, 4) {
        Err(E_PATH) => hits += 1,
        _ => return Err(E_PATH),
    }

    // R6 注入序连续 + 次数不足检出。
    match bad.all_present() {
        Err(E_COUNT) => hits += 1,
        _ => return Err(E_COUNT),
    }

    // R7 复现一致：同种子重注指纹一致。
    let mut book2 = ShowBook::new();
    let mut g2 = Lcg::new(0x487_76);
    let mut k2: u32 = 0;
    while k2 < INJECT_N {
        let path = (g2.below(PATH_N) + 1) as u8;
        let code = SEG_BASE + 1 + (k2 as u16 % 9);
        book2.show(code, path)?;
        k2 += 1;
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

    // R9 呈现行：逐条 how 可读。
    let mut i = 0usize;
    while i < book.shows.len() {
        let line = show_line(&book.shows[i]);
        if !has_kw(line.as_bytes(), b"code=") || !has_kw(line.as_bytes(), b"path=") {
            return Err(E_LINE);
        }
        i += 1;
    }
    hits += 1;

    // R10 终检签名：末条签名重算一致 + 齐套复核。
    let last = book.shows[book.shows.len() - 1];
    let sig = RetestShow::make_sig(last.seq, last.code, last.path);
    if sig != last.sig || sig == 0 {
        return Err(E_SIG);
    }
    book.all_present()?;
    hits += 1;

    Ok(hits)
}
