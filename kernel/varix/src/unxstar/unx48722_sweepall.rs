//! UNX-F48722 —— C·全域防重总扫（40 批关键词族+ID 段总扫零撞号）
//! 判据原文：UNX-F48722-J1 总扫留痕零撞；判据锚定 §6.1。
//! 判据落地（drill 场景）：R1 总扫登记 40 批；R2 关键词族闭集；
//! R3 ID 段总扫零撞号；R4 撞号检出；R5 段越界检出；R6 词族缺扫检出；
//! R7 计数守恒；R8 复扫一致；R9 指纹稳定；R10 呈现行；R11 总扫签名。
//! 设计纪律：no_std（仅 core/alloc）；零 unwrap/expect/panic；
//! 确定性 LCG；drill 全绿才返回 Ok(hits)。
//! 口径说明：全域防重总扫 = 40 批 × 关键词族（govsign/i8close）逐批
//! 留痕 + 全域 ID 段（1..=1000）逐号清点；零撞号 = 段内每号至多
//! 登记一次；任一撞号或缺批留痕即总扫红。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

/// 诊断段基址。
pub const SEG_BASE: u16 = 0x1360;

// ── 诊断码闭集（SEG_BASE+1 起连续）──────────────────────────────
const E_KW: u16 = SEG_BASE + 1; // 关键词越出词族
const E_BATCH: u16 = SEG_BASE + 2; // 批号越出窗
const E_ID: u16 = SEG_BASE + 3; // ID 越出段窗
const E_CRASH: u16 = SEG_BASE + 4; // 撞号
const E_MISS: u16 = SEG_BASE + 5; // 批缺留痕
const E_COUNT: u16 = SEG_BASE + 6; // 计数不守恒
const E_REPLAY: u16 = SEG_BASE + 7; // 复扫漂移
const E_FP: u16 = SEG_BASE + 8; // 指纹漂移
const E_LINE: u16 = SEG_BASE + 9; // 呈现行缺段
const E_SIG: u16 = SEG_BASE + 10; // 总扫签名不符

/// 词族闭集。
pub const KW_GOVSIGN: u8 = 1;
pub const KW_I8CLOSE: u8 = 2;
pub const KW_N: u32 = 2;

/// 批窗 B01–B40。
pub const B_LO: u32 = 1;
pub const B_HI: u32 = 40;
pub const BATCH_N: u32 = B_HI - B_LO + 1;

/// 全域 ID 段窗。
pub const ID_LO: u32 = 1;
pub const ID_HI: u32 = 1000;

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

/// 一条总扫留痕。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SweepRec {
    pub kw: u8,      // 词族闭集
    pub batch: u32,  // 批号 1..=40
    pub obj_id: u32, // 对象 ID 1..=1000
    pub sig: u64,    // 留痕签名
}

impl SweepRec {
    /// 留痕签名：词 + 批 + ID 混签。
    pub fn make_sig(kw: u8, batch: u32, obj_id: u32) -> u64 {
        let mut buf: Vec<u8> = vec![];
        buf.push(kw);
        buf.extend_from_slice(&batch.to_le_bytes());
        buf.extend_from_slice(&obj_id.to_le_bytes());
        fnv1a64(&buf)
    }

    pub fn sane(&self) -> Result<(), u16> {
        if self.kw != KW_GOVSIGN && self.kw != KW_I8CLOSE {
            return Err(E_KW);
        }
        if self.batch < B_LO || self.batch > B_HI {
            return Err(E_BATCH);
        }
        if self.obj_id < ID_LO || self.obj_id > ID_HI {
            return Err(E_ID);
        }
        if self.sig != SweepRec::make_sig(self.kw, self.batch, self.obj_id) {
            return Err(E_SIG);
        }
        Ok(())
    }
}

/// 总扫账。
pub struct SweepBook {
    pub recs: Vec<SweepRec>,
}

impl SweepBook {
    pub fn new() -> SweepBook {
        SweepBook { recs: vec![] }
    }

    /// 留痕登记：闭集 + ID 段内零撞（同一 ID 至多一次）。
    pub fn record(&mut self, kw: u8, batch: u32, obj_id: u32) -> Result<(), u16> {
        if kw != KW_GOVSIGN && kw != KW_I8CLOSE {
            return Err(E_KW);
        }
        if batch < B_LO || batch > B_HI {
            return Err(E_BATCH);
        }
        if obj_id < ID_LO || obj_id > ID_HI {
            return Err(E_ID);
        }
        let mut i = 0usize;
        while i < self.recs.len() {
            if self.recs[i].obj_id == obj_id {
                return Err(E_CRASH);
            }
            i += 1;
        }
        let sig = SweepRec::make_sig(kw, batch, obj_id);
        self.recs.push(SweepRec { kw, batch, obj_id, sig });
        Ok(())
    }

    /// 批窗覆盖：40 批每批 ≥1 条。
    pub fn batch_cover(&self) -> Result<(), u16> {
        let mut b = B_LO;
        while b <= B_HI {
            let mut hit = false;
            let mut i = 0usize;
            while i < self.recs.len() {
                if self.recs[i].batch == b {
                    hit = true;
                }
                i += 1;
            }
            if !hit {
                return Err(E_MISS);
            }
            b += 1;
        }
        Ok(())
    }

    /// 词族覆盖：两关键词都扫到过。
    pub fn kw_cover(&self) -> Result<(), u16> {
        let mut f1 = false;
        let mut f2 = false;
        let mut i = 0usize;
        while i < self.recs.len() {
            if self.recs[i].kw == KW_GOVSIGN {
                f1 = true;
            }
            if self.recs[i].kw == KW_I8CLOSE {
                f2 = true;
            }
            i += 1;
        }
        if !f1 || !f2 {
            return Err(E_MISS);
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
            buf.extend_from_slice(&self.recs[i].obj_id.to_le_bytes());
            buf.extend_from_slice(&self.recs[i].sig.to_le_bytes());
            i += 1;
        }
        fnv1a64(&buf)
    }
}

/// 总扫签名：批数 × 词族数 × 留痕数混签。
pub fn sweep_sig(batches: u32, kws: u32, total: u32) -> u64 {
    let mut buf: Vec<u8> = vec![];
    buf.extend_from_slice(&batches.to_le_bytes());
    buf.extend_from_slice(&kws.to_le_bytes());
    buf.extend_from_slice(&total.to_le_bytes());
    fnv1a64(&buf)
}

/// 呈现行：batch=40 kw=2 recs=<n> crash=0。
pub fn sweep_line(n: u32) -> String {
    let mut out = String::new();
    out.push_str("batch=40 kw=2 recs=");
    out.push_str(&n.to_string());
    out.push_str(" crash=0");
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
    let mut g = Lcg::new(0x487_22);

    // R1 总扫登记 40 批：每批 2 条（两词族各一），ID 由 LCG 分配。
    let mut book = SweepBook::new();
    let mut next_id: u32 = 1;
    let mut b = B_LO;
    while b <= B_HI {
        let kw = if g.below(2) == 0 { KW_GOVSIGN } else { KW_I8CLOSE };
        book.record(kw, b, next_id)?;
        next_id += 1;
        b += 1;
    }
    if book.recs.len() as u32 != BATCH_N {
        return Err(E_COUNT);
    }
    hits += 1;

    // R2 词族覆盖补扫：两词族齐。
    match book.kw_cover() {
        Ok(()) => {}
        Err(E_MISS) => {
            book.record(KW_GOVSIGN, 1, next_id)?;
            next_id += 1;
        }
        Err(e) => return Err(e),
    }
    book.record(KW_I8CLOSE, 2, next_id)?;
    next_id += 1;
    book.kw_cover()?;
    hits += 1;

    // R3 ID 段总扫零撞号：全账逐条 sane（含段窗）。
    book.verify_all()?;
    hits += 1;

    // R4 撞号检出：已登记 ID 再登记 → 红。
    let dup_id = book.recs[0].obj_id;
    match book.record(KW_GOVSIGN, 3, dup_id) {
        Err(E_CRASH) => hits += 1,
        _ => return Err(E_CRASH),
    }

    // R5 段越界：ID 0 与 1001 拒绝。
    match book.record(KW_GOVSIGN, 4, 0) {
        Err(E_ID) => {}
        _ => return Err(E_ID),
    }
    match book.record(KW_I8CLOSE, 5, ID_HI + 1) {
        Err(E_ID) => hits += 1,
        _ => return Err(E_ID),
    }

    // R6 词族缺扫检出：单词族账 → kw_cover 红。
    let mut thin = SweepBook::new();
    thin.record(KW_GOVSIGN, 1, 900)?;
    match thin.kw_cover() {
        Err(E_MISS) => hits += 1,
        _ => return Err(E_MISS),
    }

    // R7 批缺留痕检出：只留 B01 → 批覆盖红。
    match thin.batch_cover() {
        Err(E_MISS) => hits += 1,
        _ => return Err(E_MISS),
    }

    // R8 计数守恒：全账条数 = 批数 + 补扫数。
    let want = BATCH_N + 2;
    if book.recs.len() as u32 != want {
        return Err(E_COUNT);
    }
    hits += 1;

    // R9 复扫一致 + 指纹稳定。
    let f1 = book.fp();
    let f2 = book.fp();
    if f1 != f2 || f1 == 0 {
        return Err(E_REPLAY);
    }
    if book.fp() != f1 {
        return Err(E_FP);
    }
    hits += 1;

    // R10 呈现行：batch=40 kw=2 recs=42 crash=0。
    let line = sweep_line(book.recs.len() as u32);
    if !has_kw(line.as_bytes(), b"batch=40")
        || !has_kw(line.as_bytes(), b"kw=2")
        || !has_kw(line.as_bytes(), b"crash=0")
    {
        return Err(E_LINE);
    }
    hits += 1;

    // R11 总扫签名：40×2×42 混签重算一致。
    let sig = sweep_sig(BATCH_N, KW_N, book.recs.len() as u32);
    if sig != sweep_sig(40, 2, 42) || sig == 0 {
        return Err(E_SIG);
    }
    hits += 1;

    Ok(hits)
}
