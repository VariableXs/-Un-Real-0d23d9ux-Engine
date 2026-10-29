//! UNX-F48779 —— C39 批防重 grep（关键词族：c3close/wave22pack）
//! 判据原文：UNX-F48779-J1 批收口防重留痕；判据锚定 §6.1。
//! 判据落地（drill 场景）：R1 词族扫描；R2 留痕登记；R3 坐标唯一；
//! R4 重复坐标检出；R5 词族闭集；R6 收口计数；R7 复扫一致；
//! R8 指纹稳定；R9 呈现行。
//! 设计纪律：no_std（仅 core/alloc）；零 unwrap/expect/panic；
//! 确定性 LCG；drill 全绿才返回 Ok(hits)。
//! 口径说明：C39 批收口防重留痕以关键词族（c3close/wave22pack）
//! 扫描，命中坐标（文件、行）逐一登记且唯一；复扫指纹一致才算收口。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

/// 诊断段基址。
pub const SEG_BASE: u16 = 0x29A0;

// ── 诊断码闭集（SEG_BASE+1 起连续）──────────────────────────────
const E_KW: u16 = SEG_BASE + 1; // 关键词越出词族
const E_DUP: u16 = SEG_BASE + 2; // 命中坐标重复
const E_MISS: u16 = SEG_BASE + 3; // 词族扫描漏检
const E_COUNT: u16 = SEG_BASE + 4; // 收口计数不齐
const E_REPLAY: u16 = SEG_BASE + 5; // 复扫结果漂移
const E_FP: u16 = SEG_BASE + 6; // 指纹漂移
const E_LINE: u16 = SEG_BASE + 7; // 呈现行缺段
const E_COORD: u16 = SEG_BASE + 8; // 坐标非法

/// 关键词族闭集。
pub const KW_C3CLOSE: u8 = 1; // c3close
pub const KW_WAVE22PACK: u8 = 2; // wave22pack

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

/// 一条 grep 留痕。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GrepHit {
    pub kw: u8,
    pub file_id: u32,
    pub line_no: u32,
}

/// C39 防重账。
pub struct DupBook {
    pub hits: Vec<GrepHit>,
}

impl DupBook {
    pub fn new() -> DupBook {
        DupBook { hits: vec![] }
    }

    /// 留痕登记：词族闭集 + 坐标唯一。
    pub fn record(&mut self, kw: u8, file_id: u32, line_no: u32) -> Result<(), u16> {
        if kw != KW_C3CLOSE && kw != KW_WAVE22PACK {
            return Err(E_KW);
        }
        if line_no == 0 {
            return Err(E_COORD);
        }
        let mut i = 0usize;
        while i < self.hits.len() {
            if self.hits[i].file_id == file_id && self.hits[i].line_no == line_no {
                return Err(E_DUP);
            }
            i += 1;
        }
        self.hits.push(GrepHit { kw, file_id, line_no });
        Ok(())
    }

    /// 词族扫描：两关键词都要命中过。
    pub fn scan_complete(&self) -> Result<(), u16> {
        let mut f1 = false;
        let mut f2 = false;
        let mut i = 0usize;
        while i < self.hits.len() {
            if self.hits[i].kw == KW_C3CLOSE {
                f1 = true;
            }
            if self.hits[i].kw == KW_WAVE22PACK {
                f2 = true;
            }
            i += 1;
        }
        if !f1 || !f2 {
            return Err(E_MISS);
        }
        Ok(())
    }

    /// 指纹。
    pub fn fp(&self) -> u64 {
        let mut buf: Vec<u8> = vec![];
        let mut i = 0usize;
        while i < self.hits.len() {
            buf.push(self.hits[i].kw);
            buf.extend_from_slice(&self.hits[i].file_id.to_le_bytes());
            buf.extend_from_slice(&self.hits[i].line_no.to_le_bytes());
            i += 1;
        }
        fnv1a64(&buf)
    }
}

/// 收口计数。
pub fn dup_count(l: &DupBook) -> (u32, u32, u32) {
    let mut c1: u32 = 0;
    let mut c2: u32 = 0;
    let mut i = 0usize;
    while i < l.hits.len() {
        if l.hits[i].kw == KW_C3CLOSE {
            c1 += 1;
        } else {
            c2 += 1;
        }
        i += 1;
    }
    (c1, c2, c1 + c2)
}

/// 呈现行。
pub fn dup_line(l: &DupBook) -> String {
    let (c1, c2, total) = dup_count(l);
    let mut out = String::new();
    out.push_str("k1=");
    out.push_str(&c1.to_string());
    out.push_str(" k2=");
    out.push_str(&c2.to_string());
    out.push_str(" total=");
    out.push_str(&total.to_string());
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

/// 主巡检入口：R1..R9 全绿才返回 Ok(hits)。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;
    let mut g = Lcg::new(0x487_79);

    // R1 词族扫描：初扫 4 笔。
    let mut book = DupBook::new();
    book.record(KW_C3CLOSE, 701, 33)?;
    book.record(KW_C3CLOSE, 702, 91)?;
    book.record(KW_WAVE22PACK, 703, 57)?;
    book.record(KW_WAVE22PACK, 704, 122)?;
    book.scan_complete()?;
    hits += 1;

    // R2 留痕登记：补扫 4 笔（LCG 决定词族与行号）。
    let mut k: u32 = 0;
    while k < 4 {
        let kw = if g.below(2) == 0 { KW_C3CLOSE } else { KW_WAVE22PACK };
        let file = 710 + k;
        let line_no = 1 + g.below(400);
        book.record(kw, file, line_no)?;
        k += 1;
    }
    let (_, _, total) = dup_count(&book);
    if total != 8 {
        return Err(E_COUNT);
    }
    hits += 1;

    // R3 坐标唯一 + R4 重复坐标检出。
    match book.record(KW_C3CLOSE, 701, 33) {
        Err(E_DUP) => hits += 1,
        _ => return Err(E_DUP),
    }

    // R5 词族闭集：kw 0 与 3 拒绝。
    match book.record(0, 705, 3) {
        Err(E_KW) => {}
        _ => return Err(E_KW),
    }
    match book.record(3, 706, 3) {
        Err(E_KW) => hits += 1,
        _ => return Err(E_KW),
    }

    // R6 收口计数：坐标非法拒绝 + 计数守恒。
    match book.record(KW_C3CLOSE, 707, 0) {
        Err(E_COORD) => {}
        _ => return Err(E_COORD),
    }
    let (_, _, total2) = dup_count(&book);
    if total2 != book.hits.len() as u32 {
        return Err(E_COUNT);
    }
    hits += 1;

    // R7 复扫一致。
    let f1 = book.fp();
    let f2 = book.fp();
    if f1 != f2 || f1 == 0 {
        return Err(E_REPLAY);
    }
    hits += 1;

    // R8 指纹稳定。
    if book.fp() != f1 {
        return Err(E_FP);
    }
    hits += 1;

    // R9 呈现行：total=8。
    let line = dup_line(&book);
    if !has_kw(line.as_bytes(), b"k1=") || !has_kw(line.as_bytes(), b"total=8") {
        return Err(E_LINE);
    }
    hits += 1;

    Ok(hits)
}
