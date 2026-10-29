//! UNX-F48717 —— I·AI-84 防重抽检对账（全域防重 grep 留痕齐套断言）
//! 判据原文：UNX-F48717-J1 对账 ×1 齐套；判据锚定 §6.1。
//! 判据落地（drill 场景）：R1 留痕登记；R2 词族闭集；R3 坐标唯一；
//! R4 对账 ×1 齐套；R5 漏痕检出；R6 坐标非法检出；R7 计数守恒；
//! R8 复账一致；R9 指纹稳定；R10 呈现行；R11 批窗覆盖。
//! 设计纪律：no_std（仅 core/alloc）；零 unwrap/expect/panic；
//! 确定性 LCG；drill 全绿才返回 Ok(hits)。
//! 口径说明：全域防重 grep 留痕按（词族、批号、行号）三元组登记；
//! 对账 ×1 齐套 = 词族两关键词 × 批窗 B01–B20 全坐标可查且无悬挂；
//! 任一词族在任一批缺留痕即红。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

/// 诊断段基址。
pub const SEG_BASE: u16 = 0x12A0;

// ── 诊断码闭集（SEG_BASE+1 起连续）──────────────────────────────
const E_KW: u16 = SEG_BASE + 1; // 关键词越出词族
const E_DUP: u16 = SEG_BASE + 2; // 坐标重复
const E_COORD: u16 = SEG_BASE + 3; // 坐标非法
const E_MISS: u16 = SEG_BASE + 4; // 留痕缺失
const E_BATCH: u16 = SEG_BASE + 5; // 批号越出窗
const E_COUNT: u16 = SEG_BASE + 6; // 计数不守恒
const E_REPLAY: u16 = SEG_BASE + 7; // 复账漂移
const E_FP: u16 = SEG_BASE + 8; // 指纹漂移
const E_LINE: u16 = SEG_BASE + 9; // 呈现行缺段
const E_SIG: u16 = SEG_BASE + 10; // 对账签名不符

/// 词族闭集。
pub const KW_GOVSIGN: u8 = 1; // govsign
pub const KW_I8CLOSE: u8 = 2; // i8close

/// 批窗 B01–B20。
pub const B_LO: u32 = 1;
pub const B_HI: u32 = 20;

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

/// 一条防重 grep 留痕。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GrepRec {
    pub kw: u8,
    pub batch: u32, // 1..=20
    pub line_no: u32,
    pub sig: u64,
}

impl GrepRec {
    /// 留痕签名：词 + 批 + 行混签。
    pub fn make_sig(kw: u8, batch: u32, line_no: u32) -> u64 {
        let mut buf: Vec<u8> = vec![];
        buf.push(kw);
        buf.extend_from_slice(&batch.to_le_bytes());
        buf.extend_from_slice(&line_no.to_le_bytes());
        fnv1a64(&buf)
    }

    pub fn sane(&self) -> Result<(), u16> {
        if self.kw != KW_GOVSIGN && self.kw != KW_I8CLOSE {
            return Err(E_KW);
        }
        if self.batch < B_LO || self.batch > B_HI {
            return Err(E_BATCH);
        }
        if self.line_no == 0 {
            return Err(E_COORD);
        }
        if self.sig != GrepRec::make_sig(self.kw, self.batch, self.line_no) {
            return Err(E_SIG);
        }
        Ok(())
    }
}

/// 防重留痕账。
pub struct TraceBook {
    pub recs: Vec<GrepRec>,
}

impl TraceBook {
    pub fn new() -> TraceBook {
        TraceBook { recs: vec![] }
    }

    /// 留痕登记：三元组唯一。
    pub fn record(&mut self, kw: u8, batch: u32, line_no: u32) -> Result<(), u16> {
        if kw != KW_GOVSIGN && kw != KW_I8CLOSE {
            return Err(E_KW);
        }
        if batch < B_LO || batch > B_HI {
            return Err(E_BATCH);
        }
        if line_no == 0 {
            return Err(E_COORD);
        }
        let mut i = 0usize;
        while i < self.recs.len() {
            if self.recs[i].kw == kw && self.recs[i].batch == batch && self.recs[i].line_no == line_no {
                return Err(E_DUP);
            }
            i += 1;
        }
        let sig = GrepRec::make_sig(kw, batch, line_no);
        self.recs.push(GrepRec { kw, batch, line_no, sig });
        Ok(())
    }

    /// 批窗覆盖：两词族 × 全批窗每批 ≥1 条。
    pub fn full_cover(&self) -> Result<(), u16> {
        let mut kw = KW_GOVSIGN;
        while kw <= KW_I8CLOSE {
            let mut b = B_LO;
            while b <= B_HI {
                let mut hit = false;
                let mut i = 0usize;
                while i < self.recs.len() {
                    if self.recs[i].kw == kw && self.recs[i].batch == b {
                        hit = true;
                    }
                    i += 1;
                }
                if !hit {
                    return Err(E_MISS);
                }
                b += 1;
            }
            kw += 1;
        }
        Ok(())
    }

    /// 全量核对：逐条 sane。
    pub fn verify_all(&self) -> Result<(), u16> {
        let mut i = 0usize;
        while i < self.recs.len() {
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
            buf.push(self.recs[i].kw);
            buf.extend_from_slice(&self.recs[i].batch.to_le_bytes());
            buf.extend_from_slice(&self.recs[i].line_no.to_le_bytes());
            buf.extend_from_slice(&self.recs[i].sig.to_le_bytes());
            i += 1;
        }
        fnv1a64(&buf)
    }
}

/// 对账签名：批窗宽 × 词族数 × 留痕数混签。
pub const BATCH_N: u32 = B_HI - B_LO + 1;
pub const KW_N: u32 = 2;

pub fn trace_sig(batches: u32, kws: u32, total: u32) -> u64 {
    let mut buf: Vec<u8> = vec![];
    buf.extend_from_slice(&batches.to_le_bytes());
    buf.extend_from_slice(&kws.to_le_bytes());
    buf.extend_from_slice(&total.to_le_bytes());
    fnv1a64(&buf)
}

/// 呈现行：kw=2 batch=20 recs=<n> cover=full。
pub fn trace_line(n: u32) -> String {
    let mut out = String::new();
    out.push_str("kw=2 batch=20 recs=");
    out.push_str(&n.to_string());
    out.push_str(" cover=full");
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
    let mut g = Lcg::new(0x487_17);

    // R1 留痕登记：两词族 × 20 批各 1 条，行号 LCG。
    let mut book = TraceBook::new();
    let mut b = B_LO;
    while b <= B_HI {
        book.record(KW_GOVSIGN, b, 10 + g.below(900))?;
        book.record(KW_I8CLOSE, b, 10 + g.below(900))?;
        b += 1;
    }
    if book.recs.len() as u32 != BATCH_N * KW_N {
        return Err(E_COUNT);
    }
    hits += 1;

    // R2 词族闭集：kw 0 与 3 拒绝。
    match book.record(0, 1, 50) {
        Err(E_KW) => {}
        _ => return Err(E_KW),
    }
    match book.record(3, 1, 50) {
        Err(E_KW) => hits += 1,
        _ => return Err(E_KW),
    }

    // R3 坐标唯一：同三元组重复拒绝。
    let first_kw = book.recs[0].kw;
    let first_b = book.recs[0].batch;
    let first_l = book.recs[0].line_no;
    match book.record(first_kw, first_b, first_l) {
        Err(E_DUP) => hits += 1,
        _ => return Err(E_DUP),
    }

    // R4 对账 ×1 齐套：批窗全覆盖。
    book.full_cover()?;
    hits += 1;

    // R5 漏痕检出：抹掉一个批的留痕 → 覆盖红。
    let mut thin = TraceBook::new();
    let mut i = 0usize;
    while i < book.recs.len() {
        if !(book.recs[i].kw == KW_GOVSIGN && book.recs[i].batch == 11) {
            thin.recs.push(book.recs[i]);
        }
        i += 1;
    }
    match thin.full_cover() {
        Err(E_MISS) => hits += 1,
        _ => return Err(E_MISS),
    }

    // R6 坐标非法：行号 0 拒绝 + 批号越窗拒绝。
    match book.record(KW_GOVSIGN, 1, 0) {
        Err(E_COORD) => {}
        _ => return Err(E_COORD),
    }
    match book.record(KW_I8CLOSE, 21, 5) {
        Err(E_BATCH) => hits += 1,
        _ => return Err(E_BATCH),
    }

    // R7 计数守恒：全量核对逐条 sane。
    book.verify_all()?;
    hits += 1;

    // R8 复账一致：同种子重跑指纹一致。
    let mut book2 = TraceBook::new();
    let mut g2 = Lcg::new(0x487_17);
    let mut b2 = B_LO;
    while b2 <= B_HI {
        book2.record(KW_GOVSIGN, b2, 10 + g2.below(900))?;
        book2.record(KW_I8CLOSE, b2, 10 + g2.below(900))?;
        b2 += 1;
    }
    if book2.fp() != book.fp() {
        return Err(E_REPLAY);
    }
    hits += 1;

    // R9 指纹稳定。
    let f1 = book.fp();
    let f2 = book.fp();
    if f1 != f2 || f1 == 0 {
        return Err(E_FP);
    }
    hits += 1;

    // R10 呈现行：recs=40 cover=full。
    let line = trace_line(book.recs.len() as u32);
    if !has_kw(line.as_bytes(), b"recs=40") || !has_kw(line.as_bytes(), b"cover=full") {
        return Err(E_LINE);
    }
    hits += 1;

    // R11 对账签名：20×2×40 混签重算一致。
    let sig = trace_sig(BATCH_N, KW_N, book.recs.len() as u32);
    if sig != trace_sig(20, 2, 40) || sig == 0 {
        return Err(E_SIG);
    }
    hits += 1;

    Ok(hits)
}
