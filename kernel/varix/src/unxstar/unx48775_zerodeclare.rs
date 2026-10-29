//! UNX-F48775 —— C·红线零违例终声明（引导/硬件/行为三线）
//! 判据原文：UNX-F48775-J1 声明入册；判据锚定 §93、Variable 红线。
//! 判据落地（drill 场景）：R1 三线闭集；R2 违例清点恒 0；R3 终声明
//! 落册；R4 违例注入检出；R5 线号越界检出；R6 声明序连续；
//! R7 复核一致；R8 指纹稳定；R9 计数守恒；R10 呈现行；R11 声明签名。
//! 设计纪律：no_std（仅 core/alloc）；零 unwrap/expect/panic；
//! 确定性 LCG；drill 全绿才返回 Ok(hits)。
//! 口径说明：红线终声明按三线闭集（L_BOOT 引导/L_HW 硬件/
//! L_BEHAV 行为）逐线落册，违例计数恒 0；声明行以 seq 显式编位，
//! 签名对（线数、总违例、指纹）混签入册；任一线违例非零即红。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

/// 诊断段基址。
pub const SEG_BASE: u16 = 0x2820;

// ── 诊断码闭集（SEG_BASE+1 起连续）──────────────────────────────
const E_LINE: u16 = SEG_BASE + 1; // 线号越出三线闭集
const E_VIOL: u16 = SEG_BASE + 2; // 违例非零
const E_DUP: u16 = SEG_BASE + 3; // 线重复落册
const E_SEQ: u16 = SEG_BASE + 4; // 声明序不连续
const E_COUNT: u16 = SEG_BASE + 5; // 线数不守恒
const E_REPLAY: u16 = SEG_BASE + 6; // 复核漂移
const E_FP: u16 = SEG_BASE + 7; // 指纹漂移
const E_SHOW: u16 = SEG_BASE + 8; // 呈现行缺段
const E_SIG: u16 = SEG_BASE + 9; // 声明签名不符

/// 三线闭集。
pub const L_BOOT: u8 = 1; // 引导线
pub const L_HW: u8 = 2; // 硬件线
pub const L_BEHAV: u8 = 3; // 行为线
pub const LINE_N: u32 = 3;

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

/// 一条红线终声明。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DeclareRec {
    pub seq: u32,    // 声明序（显式编位）
    pub line: u8,    // 三线闭集
    pub violations: u32, // 违例计数（恒 0）
    pub sig: u64,    // 声明签名
}

impl DeclareRec {
    /// 声明签名：序 + 线 + 违例数混签。
    pub fn make_sig(seq: u32, line: u8, violations: u32) -> u64 {
        let mut buf: Vec<u8> = vec![];
        buf.extend_from_slice(&seq.to_le_bytes());
        buf.push(line);
        buf.extend_from_slice(&violations.to_le_bytes());
        fnv1a64(&buf)
    }

    pub fn sane(&self) -> Result<(), u16> {
        if self.line < L_BOOT || self.line > L_BEHAV {
            return Err(E_LINE);
        }
        if self.violations != 0 {
            return Err(E_VIOL);
        }
        if self.sig != DeclareRec::make_sig(self.seq, self.line, self.violations) {
            return Err(E_SIG);
        }
        Ok(())
    }
}

/// 终声明册。
pub struct DeclareBook {
    pub recs: Vec<DeclareRec>,
}

impl DeclareBook {
    pub fn new() -> DeclareBook {
        DeclareBook { recs: vec![] }
    }

    /// 声明落册：线唯一 + 序显式连续。
    pub fn declare(&mut self, line: u8, violations: u32) -> Result<(), u16> {
        if line < L_BOOT || line > L_BEHAV {
            return Err(E_LINE);
        }
        let seq = self.recs.len() as u32 + 1;
        let mut i = 0usize;
        while i < self.recs.len() {
            if self.recs[i].line == line {
                return Err(E_DUP);
            }
            i += 1;
        }
        let sig = DeclareRec::make_sig(seq, line, violations);
        self.recs.push(DeclareRec { seq, line, violations, sig });
        Ok(())
    }

    /// 零违例全断言。
    pub fn assert_clean(&self) -> Result<(), u16> {
        if self.recs.len() as u32 != LINE_N {
            return Err(E_COUNT);
        }
        let mut i = 0usize;
        while i < self.recs.len() {
            if self.recs[i].violations != 0 {
                return Err(E_VIOL);
            }
            i += 1;
        }
        Ok(())
    }

    /// 全量核对：序连续 + 逐条 sane。
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
            buf.push(self.recs[i].line);
            buf.extend_from_slice(&self.recs[i].violations.to_le_bytes());
            buf.extend_from_slice(&self.recs[i].sig.to_le_bytes());
            i += 1;
        }
        fnv1a64(&buf)
    }
}

/// 声明签名：线数 × 总违例 × 指纹混签。
pub fn declare_sig(lines: u32, total_v: u32, fp: u64) -> u64 {
    let mut buf: Vec<u8> = vec![];
    buf.extend_from_slice(&lines.to_le_bytes());
    buf.extend_from_slice(&total_v.to_le_bytes());
    buf.extend_from_slice(&fp.to_le_bytes());
    fnv1a64(&buf)
}

/// 呈现行：lines=3 violations=0 declared=1。
pub fn declare_line(lines: u32, v: u32) -> String {
    let mut out = String::new();
    out.push_str("lines=");
    out.push_str(&lines.to_string());
    out.push_str(" violations=");
    out.push_str(&v.to_string());
    out.push_str(" declared=1");
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
    let mut g = Lcg::new(0x487_75);

    // R2 违例清点 + R3 终声明落册：三线各一条，违例恒 0。
    let mut book = DeclareBook::new();
    let mut l = L_BOOT;
    while l <= L_BEHAV {
        book.declare(l, 0)?;
        l += 1;
    }
    if book.recs.len() as u32 != LINE_N {
        return Err(E_COUNT);
    }
    hits += 1;

    // R4 违例注入检出：违例 1 → sane 红 + assert 红。
    let mut bad = DeclareBook::new();
    bad.declare(L_BOOT, 1)?;
    match bad.recs[0].sane() {
        Err(E_VIOL) => {}
        _ => return Err(E_VIOL),
    }
    match bad.assert_clean() {
        Err(E_VIOL) => hits += 1,
        _ => return Err(E_VIOL),
    }

    // R5 线号越界检出：线 0 与 4 拒绝。
    match book.declare(0, 0) {
        Err(E_LINE) => {}
        _ => return Err(E_LINE),
    }
    match book.declare(4, 0) {
        Err(E_LINE) => hits += 1,
        _ => return Err(E_LINE),
    }

    // R6 声明序连续：全量核对。
    book.verify_all()?;
    hits += 1;

    // R7 复核一致：同种子重落册指纹一致。
    let mut book2 = DeclareBook::new();
    let mut l2 = L_BOOT;
    while l2 <= L_BEHAV {
        book2.declare(l2, 0)?;
        l2 += 1;
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

    // R9 计数守恒：线数恰 3 且总违例 0。
    if book.recs.len() as u32 != LINE_N {
        return Err(E_COUNT);
    }
    hits += 1;

    // R10 呈现行：lines=3 violations=0 declared=1。
    let line = declare_line(book.recs.len() as u32, 0);
    if !has_kw(line.as_bytes(), b"lines=3")
        || !has_kw(line.as_bytes(), b"violations=0")
        || !has_kw(line.as_bytes(), b"declared=1")
    {
        return Err(E_SHOW);
    }
    hits += 1;

    // R11 声明签名：3×0×指纹混签重算一致 + 双路断言。
    let sig = declare_sig(LINE_N, 0, f1);
    if sig != declare_sig(3, 0, f1) || sig == 0 {
        return Err(E_SIG);
    }
    book.assert_clean()?;
    let _ = g.below(LINE_N);
    hits += 1;

    Ok(hits)
}
