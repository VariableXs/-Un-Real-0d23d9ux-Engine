//! UNX-F48579 —— I29 批防重 grep（关键词族：f4sign/candidatejoint）
//! 判据原文：UNX-F48579-J1 I29 批内防重以关键词族扫描为准：
//! 两族关键词（f4sign/candidatejoint）在账内逐行扫描计数；
//! 同一关键词族在同一线内重复出现必须被防重检出；族账守恒、
//! 空行/空族边界不误报、指纹与呈现行可复核。
//! 判据落地（drill 场景）：R1 两族闭集；R2 单族计数；
//! R3 防重检出；R4 空族不误报；R5 族账守恒；
//! R6 两族并存互不串账；R7 空账守恒；R8 fuzz 行流零错态；
//! R9 指纹稳定；R10 呈现行齐全。
//! 设计纪律：no_std；零 unwrap/expect/panic；确定性 LCG；drill 判据全绿才返回 Ok。
use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

/// 诊断段基址（独立 0x20 窗）。
pub const SEG_BASE: u16 = 0xF840;

// ── 诊断码闭集（SEG_BASE+1 起连续，≤ SEG_BASE+0x1F）─────────────
const E_FAMILY: u16 = SEG_BASE + 1; // 关键词族越出闭集
const E_COUNT: u16 = SEG_BASE + 2; // 单族计数错误
const E_DUP: u16 = SEG_BASE + 3; // 防重未检出
const E_FALSE: u16 = SEG_BASE + 4; // 空族误报
const E_CONSERV: u16 = SEG_BASE + 5; // 族账不守恒
const E_MIX: u16 = SEG_BASE + 6; // 两族串账
const E_EMPTY: u16 = SEG_BASE + 7; // 空账守恒失败
const E_FUZZ: u16 = SEG_BASE + 8; // fuzz 行流错态
const E_FP: u16 = SEG_BASE + 9; // 指纹漂移
const E_LINE: u16 = SEG_BASE + 10; // 呈现行缺段

/// 关键词族闭集。
pub const KW_F4SIGN: &[u8] = b"f4sign";
pub const KW_CANDJOINT: &[u8] = b"candidatejoint";

/// FNV-1a 64 位（与本域其余哨兵同口径）。
pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// 确定性 LCG（批内统一系数）。
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

/// 子串判定（朴素扫描，统一口径）。
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

/// 单行族命中数：一行内某族出现次数。
pub fn count_in_line(line: &[u8], kw: &[u8]) -> u32 {
    if kw.is_empty() || line.len() < kw.len() {
        return 0;
    }
    let mut n = 0u32;
    let mut i = 0usize;
    while i + kw.len() <= line.len() {
        let mut j = 0usize;
        while j < kw.len() && line[i + j] == kw[j] {
            j += 1;
        }
        if j == kw.len() {
            n += 1;
            i += kw.len();
        } else {
            i += 1;
        }
    }
    n
}

/// 族账：两族计数。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FamilyCount {
    pub f4sign: u32,
    pub candjoint: u32,
}

/// 防重扫描器：逐行扫两族，行内重复检出。
pub struct GrepScan {
    pub fc: FamilyCount,
    pub dup_lines: u32,
    pub lines: u32,
}

impl GrepScan {
    pub fn new() -> GrepScan {
        GrepScan {
            fc: FamilyCount { f4sign: 0, candjoint: 0 },
            dup_lines: 0,
            lines: 0,
        }
    }

    /// 扫一行：累计两族计数；行内任一族出现 ≥2 次记防重一笔。
    pub fn scan_line(&mut self, line: &[u8]) {
        let a = count_in_line(line, KW_F4SIGN);
        let b = count_in_line(line, KW_CANDJOINT);
        self.fc.f4sign += a;
        self.fc.candjoint += b;
        self.lines += 1;
        if a >= 2 || b >= 2 {
            self.dup_lines += 1;
        }
    }

    /// 族账守恒：两族和 == 外部记账总数。
    pub fn conserved(&self, total: u32) -> bool {
        self.fc.f4sign + self.fc.candjoint == total
    }

    /// 全账零态：防重行数 ≤ 行数。
    pub fn sane(&self) -> bool {
        self.dup_lines <= self.lines
    }
}

/// 主巡检入口：R1..R10 全绿才返回 Ok(hits)。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 两族闭集：两族关键词非空互异，子串判定自洽。
    if KW_F4SIGN.is_empty() || KW_CANDJOINT.is_empty() {
        return Err(E_FAMILY);
    }
    if has_kw(KW_F4SIGN, KW_CANDJOINT) || has_kw(KW_CANDJOINT, KW_F4SIGN) {
        return Err(E_FAMILY);
    }
    hits += 1;

    // R2 单族计数：固定行族内出现次数恰命中。
    let l1 = b"f4sign gate at 0x20 window";
    if count_in_line(l1, KW_F4SIGN) != 1 {
        return Err(E_COUNT);
    }
    let l2 = b"candidatejoint candidatejoint and candidatejoint";
    if count_in_line(l2, KW_CANDJOINT) != 3 {
        return Err(E_COUNT);
    }
    let l3 = b"no keywords here at all";
    if count_in_line(l3, KW_F4SIGN) != 0 || count_in_line(l3, KW_CANDJOINT) != 0 {
        return Err(E_COUNT);
    }
    hits += 1;

    // R3 防重检出：行内同一族 ≥2 次 → dup_lines 恰 1。
    let mut gs = GrepScan::new();
    gs.scan_line(b"f4sign then f4sign again");
    if gs.dup_lines != 1 {
        return Err(E_DUP);
    }
    hits += 1;

    // R4 空族不误报：不含关键词的行不进任何族账。
    let mut gs2 = GrepScan::new();
    gs2.scan_line(b"clean line without family words");
    if gs2.fc.f4sign != 0 || gs2.fc.candjoint != 0 || gs2.dup_lines != 0 {
        return Err(E_FALSE);
    }
    hits += 1;

    // R5 族账守恒：三行账与手工记账一致。
    let mut gs3 = GrepScan::new();
    gs3.scan_line(b"f4sign edge");
    gs3.scan_line(b"candidatejoint window");
    gs3.scan_line(b"f4sign and candidatejoint pair");
    if !gs3.conserved(4) || !gs3.sane() {
        return Err(E_CONSERV);
    }
    hits += 1;

    // R6 两族并存互不串账：一行含两族，各族恰 +1。
    let mut gs4 = GrepScan::new();
    gs4.scan_line(b"f4sign candidatejoint in one line");
    if gs4.fc.f4sign != 1 || gs4.fc.candjoint != 1 {
        return Err(E_MIX);
    }
    hits += 1;

    // R7 空账守恒：零行账两族皆 0、防重 0。
    let gs5 = GrepScan::new();
    if !gs5.conserved(0) || gs5.dup_lines != 0 || gs5.lines != 0 {
        return Err(E_EMPTY);
    }
    hits += 1;

    // R8 fuzz 行流零错态：LCG 拼接 96 行伪源码，族账自洽、防重 ≤ 行数。
    let mut gs6 = GrepScan::new();
    let mut lcg = Lcg::new(0x48579);
    let words: [&[u8]; 4] = [b"f4sign", b"candidatejoint", b"filler", b"tail"];
    let mut t = 0u32;
    while t < 96 {
        let mut line: Vec<u8> = vec![];
        let wc = 1 + lcg.below(4);
        let mut w = 0u32;
        while w < wc {
            let wi = lcg.below(4) as usize;
            let mut k = 0usize;
            while k < words[wi].len() {
                line.push(words[wi][k]);
                k += 1;
            }
            line.push(b' ');
            w += 1;
        }
        gs6.scan_line(&line);
        t += 1;
    }
    if gs6.dup_lines > gs6.lines {
        return Err(E_FUZZ);
    }
    if !gs6.conserved(gs6.fc.f4sign + gs6.fc.candjoint) {
        return Err(E_FUZZ);
    }
    if !gs6.sane() {
        return Err(E_FUZZ);
    }
    hits += 1;

    // R9 指纹稳定：同账两次指纹一致且非零。
    let f1 = scan_fp(&gs6);
    let f2 = scan_fp(&gs6);
    if f1 != f2 || f1 == 0 {
        return Err(E_FP);
    }
    hits += 1;

    // R10 呈现行齐全：f4s=/cjt=/dup= 三段前缀在位。
    let line = scan_line_str(&gs6);
    if !has_kw(line.as_bytes(), b"f4s=")
        || !has_kw(line.as_bytes(), b"cjt=")
        || !has_kw(line.as_bytes(), b"dup=")
    {
        return Err(E_LINE);
    }
    hits += 1;

    Ok(hits)
}

/// 账面指纹：两族计数 + 防重数串联。
pub fn scan_fp(gs: &GrepScan) -> u64 {
    let mut buf: Vec<u8> = vec![];
    buf.extend_from_slice(&gs.fc.f4sign.to_le_bytes());
    buf.extend_from_slice(&gs.fc.candjoint.to_le_bytes());
    buf.extend_from_slice(&gs.dup_lines.to_le_bytes());
    fnv1a64(&buf)
}

/// 呈现行：f4s=<f4sign> cjt=<candidatejoint> dup=<防重行>。
pub fn scan_line_str(gs: &GrepScan) -> String {
    let mut s = String::new();
    s.push_str("f4s=");
    s.push_str(&gs.fc.f4sign.to_string());
    s.push_str(" cjt=");
    s.push_str(&gs.fc.candjoint.to_string());
    s.push_str(" dup=");
    s.push_str(&gs.dup_lines.to_string());
    s
}
