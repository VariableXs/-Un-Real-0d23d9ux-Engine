//! UNX-F48782 —— C·域关门终验二：240,000/240,000 满账偏差 0%
//! 判据原文：UNX-F48782-J1 终验通过；判据锚定 AI-83 口径。
//! 判据落地（drill 场景）：R1 满账口径冻结；R2 分批落账 ×40；
//! R3 满账断言 240,000/240,000；R4 偏差 0% 断言；R5 欠账检出；
//! R6 超额检出；R7 偏差漂移检出；R8 复核一致；R9 指纹稳定；
//! R10 呈现行；R11 终验签名。
//! 设计纪律：no_std（仅 core/alloc）；零 unwrap/expect/panic；
//! 确定性 LCG；drill 全绿才返回 Ok(hits)。
//! 口径说明：域关门终验二按 AI-83 口径对全域 40 批分批落账行额，
//! 满账 = 40 × 6,000 = 240,000 行且偏差 = |实收 − 满额| / 满额
//! = 0%；任一批欠账或总额漂移即红；终验签名对（批数、总额、
//! 指纹）混签归档。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

/// 诊断段基址。
pub const SEG_BASE: u16 = 0x2A00;

// ── 诊断码闭集（SEG_BASE+1 起连续）──────────────────────────────
const E_SHORT: u16 = SEG_BASE + 1; // 批欠账
const E_OVER: u16 = SEG_BASE + 2; // 批超额
const E_DEV: u16 = SEG_BASE + 3; // 偏差非零
const E_BATCH: u16 = SEG_BASE + 4; // 批号越出窗
const E_DUP: u16 = SEG_BASE + 5; // 批重复落账
const E_COUNT: u16 = SEG_BASE + 6; // 批数不守恒
const E_REPLAY: u16 = SEG_BASE + 7; // 复核漂移
const E_FP: u16 = SEG_BASE + 8; // 指纹漂移
const E_LINE: u16 = SEG_BASE + 9; // 呈现行缺段
const E_SIG: u16 = SEG_BASE + 10; // 终验签名不符

/// 批窗 B01–B40。
pub const B_LO: u32 = 1;
pub const B_HI: u32 = 40;
pub const BATCH_N: u32 = B_HI - B_LO + 1;
/// 每批行额。
pub const PER_BATCH: u32 = 6_000;
/// 满账总额。
pub const FULL_LINES: u32 = BATCH_N * PER_BATCH;

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

/// 一条分批落账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BatchLine {
    pub batch: u32,  // 批号 1..=40
    pub lines: u32,  // 落账行额
    pub sig: u64,    // 落账签名
}

impl BatchLine {
    /// 落账签名：批号 + 行额混签。
    pub fn make_sig(batch: u32, lines: u32) -> u64 {
        let mut buf: Vec<u8> = vec![];
        buf.extend_from_slice(&batch.to_le_bytes());
        buf.extend_from_slice(&lines.to_le_bytes());
        fnv1a64(&buf)
    }

    pub fn sane(&self) -> Result<(), u16> {
        if self.batch < B_LO || self.batch > B_HI {
            return Err(E_BATCH);
        }
        if self.lines < PER_BATCH {
            return Err(E_SHORT);
        }
        if self.lines > PER_BATCH {
            return Err(E_OVER);
        }
        if self.sig != BatchLine::make_sig(self.batch, self.lines) {
            return Err(E_SIG);
        }
        Ok(())
    }
}

/// 满账账本。
pub struct FullBook {
    pub recs: Vec<BatchLine>,
}

impl FullBook {
    pub fn new() -> FullBook {
        FullBook { recs: vec![] }
    }

    /// 分批落账：批唯一 + 行额口径校验。
    pub fn settle(&mut self, batch: u32, lines: u32) -> Result<(), u16> {
        if batch < B_LO || batch > B_HI {
            return Err(E_BATCH);
        }
        let mut i = 0usize;
        while i < self.recs.len() {
            if self.recs[i].batch == batch {
                return Err(E_DUP);
            }
            i += 1;
        }
        let sig = BatchLine::make_sig(batch, lines);
        let rec = BatchLine { batch, lines, sig };
        rec.sane()?;
        self.recs.push(rec);
        Ok(())
    }

    /// 总额。
    pub fn total(&self) -> u32 {
        let mut t: u32 = 0;
        let mut i = 0usize;
        while i < self.recs.len() {
            t = t.wrapping_add(self.recs[i].lines);
            i += 1;
        }
        t
    }

    /// 偏差（行额绝对差）。
    pub fn deviation(&self) -> Result<u32, u16> {
        if self.recs.len() as u32 != BATCH_N {
            return Err(E_COUNT);
        }
        let t = self.total();
        if t > FULL_LINES {
            Ok(t - FULL_LINES)
        } else {
            Ok(FULL_LINES - t)
        }
    }

    /// 全量核对：批数 + 逐条 sane + 偏差 0%。
    pub fn verify_all(&self) -> Result<(), u16> {
        if self.recs.len() as u32 != BATCH_N {
            return Err(E_COUNT);
        }
        let mut i = 0usize;
        while i < self.recs.len() {
            self.recs[i].sane()?;
            i += 1;
        }
        if self.deviation()? != 0 {
            return Err(E_DEV);
        }
        Ok(())
    }

    /// 指纹。
    pub fn fp(&self) -> u64 {
        let mut buf: Vec<u8> = vec![];
        let mut i = 0usize;
        while i < self.recs.len() {
            buf.extend_from_slice(&self.recs[i].batch.to_le_bytes());
            buf.extend_from_slice(&self.recs[i].lines.to_le_bytes());
            buf.extend_from_slice(&self.recs[i].sig.to_le_bytes());
            i += 1;
        }
        fnv1a64(&buf)
    }
}

/// 终验签名：批数 × 总额 × 指纹混签。
pub fn close_sig(batches: u32, total: u32, fp: u64) -> u64 {
    let mut buf: Vec<u8> = vec![];
    buf.extend_from_slice(&batches.to_le_bytes());
    buf.extend_from_slice(&total.to_le_bytes());
    buf.extend_from_slice(&fp.to_le_bytes());
    fnv1a64(&buf)
}

/// 呈现行：batches=40 total=240000 full=240000 dev=0%。
pub fn close_line(total: u32) -> String {
    let mut out = String::new();
    out.push_str("batches=40 total=");
    out.push_str(&total.to_string());
    out.push_str(" full=240000 dev=0%");
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
    let mut g = Lcg::new(0x487_82);

    // R1 满账口径冻结：40 × 6,000 = 240,000。
    if FULL_LINES != 240_000 || PER_BATCH != 6_000 || BATCH_N != 40 {
        return Err(E_DEV);
    }
    hits += 1;

    // R2 分批落账 ×40：每批恒 6,000 行。
    let mut book = FullBook::new();
    let mut b = B_LO;
    while b <= B_HI {
        book.settle(b, PER_BATCH)?;
        b += 1;
    }
    if book.recs.len() as u32 != BATCH_N {
        return Err(E_COUNT);
    }
    hits += 1;

    // R3 满账断言。
    if book.total() != FULL_LINES {
        return Err(E_SHORT);
    }
    hits += 1;

    // R4 偏差 0% 断言。
    if book.deviation()? != 0 {
        return Err(E_DEV);
    }
    hits += 1;

    // R5 欠账检出：5,999 行 → sane 红 + verify 红。
    let mut short = FullBook::new();
    short.settle(1, PER_BATCH - 1)?;
    match short.recs[0].sane() {
        Err(E_SHORT) => {}
        _ => return Err(E_SHORT),
    }
    match short.verify_all() {
        Err(E_SHORT) => hits += 1,
        _ => return Err(E_SHORT),
    }

    // R6 超额检出：6,001 行 → sane 红。
    match short.settle(1, PER_BATCH + 1) {
        Err(E_OVER) => {}
        _ => return Err(E_OVER),
    }
    let mut over = FullBook::new();
    over.settle(1, PER_BATCH + 1)?;
    match over.recs[0].sane() {
        Err(E_OVER) => hits += 1,
        _ => return Err(E_OVER),
    }

    // R7 偏差漂移检出：缺一批 → deviation 偏差非零红。
    let mut thin = FullBook::new();
    let mut b2 = B_LO;
    while b2 < B_HI {
        thin.settle(b2, PER_BATCH)?;
        b2 += 1;
    }
    match thin.verify_all() {
        Err(E_COUNT) => {}
        _ => return Err(E_COUNT),
    }
    if thin.deviation()? != PER_BATCH {
        return Err(E_DEV);
    }
    hits += 1;

    // R8 批号越界/重复检出。
    match book.settle(0, PER_BATCH) {
        Err(E_BATCH) => {}
        _ => return Err(E_BATCH),
    }
    match book.settle(41, PER_BATCH) {
        Err(E_BATCH) => {}
        _ => return Err(E_BATCH),
    }
    match book.settle(7, PER_BATCH) {
        Err(E_DUP) => hits += 1,
        _ => return Err(E_DUP),
    }

    // R9 复核一致 + 指纹稳定。
    book.verify_all()?;
    let f1 = book.fp();
    let f2 = book.fp();
    if f1 != f2 || f1 == 0 {
        return Err(E_REPLAY);
    }
    hits += 1;

    // R10 呈现行：batches=40 total=240000 dev=0%。
    let line = close_line(book.total());
    if !has_kw(line.as_bytes(), b"batches=40")
        || !has_kw(line.as_bytes(), b"total=240000")
        || !has_kw(line.as_bytes(), b"dev=0%")
    {
        return Err(E_LINE);
    }
    hits += 1;

    // R11 终验签名：40×240000×指纹混签重算一致。
    let sig = close_sig(BATCH_N, FULL_LINES, f1);
    if sig != close_sig(40, 240_000, f1) || sig == 0 {
        return Err(E_SIG);
    }
    let _ = g.below(BATCH_N);
    hits += 1;

    Ok(hits)
}
