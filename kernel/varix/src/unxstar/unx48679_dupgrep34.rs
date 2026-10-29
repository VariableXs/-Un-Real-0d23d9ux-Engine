//! UNX-F48679 —— I34 批防重 grep（关键词族：o5pack/auditjoint）
//! 判据原文：UNX-F48679-J1 批收口防重留痕；判据锚定 §6.1。
//! 判据落地（drill 场景）：R1 关键词族扫描；R2 留痕登记；R3 坐标唯一；
//! R4 重复坐标检出；R5 词族闭集；R6 批收口计数；R7 复扫一致；
//! R8 指纹稳定；R9 文件集闭包；R10 呈现行。
//! 设计纪律：no_std（仅 core/alloc）；零 unwrap/expect/panic；
//! 确定性 LCG；drill 全绿才返回 Ok(hits)。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

/// 诊断段基址。
pub const SEG_BASE: u16 = 0x0C40;

// ── 诊断码闭集（SEG_BASE+1 起连续）──────────────────────────────
const E_KW: u16 = SEG_BASE + 1; // 关键词越出词族
const E_DUP: u16 = SEG_BASE + 2; // 命中坐标重复
const E_MISS: u16 = SEG_BASE + 3; // 词族扫描漏检
const E_COUNT: u16 = SEG_BASE + 4; // 批收口计数不齐
const E_REPLAY: u16 = SEG_BASE + 5; // 复扫结果漂移
const E_FP: u16 = SEG_BASE + 6; // 指纹漂移
const E_CLOSURE: u16 = SEG_BASE + 7; // 文件集闭包破坏
const E_LINE: u16 = SEG_BASE + 8; // 呈现行缺段
const E_COORD: u16 = SEG_BASE + 9; // 坐标非法

/// 关键词族闭集。
pub const KW_O5PACK: u8 = 1; // o5pack
pub const KW_AUDITJOINT: u8 = 2; // auditjoint

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

/// 关键词族判定与名称。
pub fn kw_ok(k: u8) -> bool {
    k == KW_O5PACK || k == KW_AUDITJOINT
}

pub fn kw_name(k: u8) -> &'static str {
    if k == KW_O5PACK {
        "o5pack"
    } else {
        "auditjoint"
    }
}

/// 一条 grep 留痕。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct GrepHit {
    pub kw: u8,
    pub file_id: u32,
    pub line_no: u32,
}

impl GrepHit {
    pub fn coord_ok(&self) -> bool {
        self.line_no > 0
    }
}

/// 批防重账。
pub struct DupGrepLedger {
    pub hits: Vec<GrepHit>,
}

impl DupGrepLedger {
    pub fn new() -> DupGrepLedger {
        DupGrepLedger { hits: vec![] }
    }

    /// 留痕登记：词族闭集 + 坐标合法 + 坐标唯一。
    pub fn record(&mut self, kw: u8, file_id: u32, line_no: u32) -> Result<(), u16> {
        if !kw_ok(kw) {
            return Err(E_KW);
        }
        let h = GrepHit { kw, file_id, line_no };
        if !h.coord_ok() {
            return Err(E_COORD);
        }
        let mut i = 0usize;
        while i < self.hits.len() {
            if self.hits[i].file_id == file_id && self.hits[i].line_no == line_no {
                return Err(E_DUP);
            }
            i += 1;
        }
        self.hits.push(h);
        Ok(())
    }

    /// 词族扫描：两词各至少一笔。
    pub fn scan_complete(&self) -> Result<(), u16> {
        let mut f1 = false;
        let mut f2 = false;
        let mut i = 0usize;
        while i < self.hits.len() {
            if self.hits[i].kw == KW_O5PACK {
                f1 = true;
            }
            if self.hits[i].kw == KW_AUDITJOINT {
                f2 = true;
            }
            i += 1;
        }
        if !f1 || !f2 {
            return Err(E_MISS);
        }
        Ok(())
    }

    /// 文件集闭包。
    pub fn file_closure(&self, expect_files: u32) -> bool {
        let mut files: Vec<u32> = vec![];
        let mut i = 0usize;
        while i < self.hits.len() {
            let f = self.hits[i].file_id;
            let mut seen = false;
            let mut j = 0usize;
            while j < files.len() {
                if files[j] == f {
                    seen = true;
                }
                j += 1;
            }
            if !seen {
                files.push(f);
            }
            i += 1;
        }
        files.len() as u32 == expect_files
    }
}

/// 批收口计数。
pub fn batch_count(l: &DupGrepLedger) -> (u32, u32, u32) {
    let mut c1: u32 = 0;
    let mut c2: u32 = 0;
    let mut i = 0usize;
    while i < l.hits.len() {
        if l.hits[i].kw == KW_O5PACK {
            c1 += 1;
        } else {
            c2 += 1;
        }
        i += 1;
    }
    (c1, c2, c1 + c2)
}

/// 账面指纹。
pub fn ledger_fp(l: &DupGrepLedger) -> u64 {
    let mut buf: Vec<u8> = vec![];
    let mut i = 0usize;
    while i < l.hits.len() {
        buf.push(l.hits[i].kw);
        buf.extend_from_slice(&l.hits[i].file_id.to_le_bytes());
        buf.extend_from_slice(&l.hits[i].line_no.to_le_bytes());
        i += 1;
    }
    fnv1a64(&buf)
}

/// 呈现行。
pub fn ledger_line(l: &DupGrepLedger) -> String {
    let (c1, c2, total) = batch_count(l);
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

/// 主巡检入口：R1..R10 全绿才返回 Ok(hits)。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;
    let mut g = Lcg::new(0x486_79);

    // R1 关键词族扫描。
    let mut led = DupGrepLedger::new();
    led.record(KW_O5PACK, 301, 9)?;
    led.record(KW_O5PACK, 302, 41)?;
    led.record(KW_AUDITJOINT, 303, 17)?;
    led.record(KW_AUDITJOINT, 304, 88)?;
    led.scan_complete()?;
    hits += 1;

    // R2 留痕登记计数。
    let (_, _, total) = batch_count(&led);
    if total != 4 || led.hits.len() as u32 != total {
        return Err(E_COUNT);
    }
    hits += 1;

    // R3 坐标唯一。
    match led.record(KW_O5PACK, 301, 9) {
        Err(E_DUP) => hits += 1,
        _ => return Err(E_DUP),
    }

    // R4 坐标非法（行号 0）拒收。
    match led.record(KW_AUDITJOINT, 305, 0) {
        Err(E_COORD) => hits += 1,
        _ => return Err(E_COORD),
    }

    // R5 词族闭集。
    match led.record(0, 306, 5) {
        Err(E_KW) => {}
        _ => return Err(E_KW),
    }
    match led.record(5, 307, 5) {
        Err(E_KW) => hits += 1,
        _ => return Err(E_KW),
    }

    // R6 批收口计数：LCG 补扫 8 笔。
    let mut round = 0u32;
    while round < 8 {
        let kw = if g.below(2) == 0 { KW_O5PACK } else { KW_AUDITJOINT };
        let file = 400 + round;
        let line_no = 1 + g.below(400);
        led.record(kw, file, line_no)?;
        round += 1;
    }
    let (_, _, total2) = batch_count(&led);
    if total2 != total + 8 {
        return Err(E_COUNT);
    }
    hits += 1;

    // R7 复扫一致。
    let f1 = ledger_fp(&led);
    let f2 = ledger_fp(&led);
    if f1 != f2 || f1 == 0 {
        return Err(E_REPLAY);
    }
    hits += 1;

    // R8 指纹稳定。
    if ledger_fp(&led) != f1 {
        return Err(E_FP);
    }
    hits += 1;

    // R9 文件集闭包：12 文件。
    if !led.file_closure(12) {
        return Err(E_CLOSURE);
    }
    hits += 1;

    // R10 呈现行：total=12。
    let line = ledger_line(&led);
    if !has_kw(line.as_bytes(), b"k1=") || !has_kw(line.as_bytes(), b"k2=")
        || !has_kw(line.as_bytes(), b"total=12")
    {
        return Err(E_LINE);
    }
    hits += 1;

    Ok(hits)
}
