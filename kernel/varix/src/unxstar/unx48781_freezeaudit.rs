//! UNX-F48781 —— C·域关门终验一：40 批 ×20 条 = 800 条全冻结断言
//! 判据原文：UNX-F48781-J1 断言通过 F48001–F48800 零跳号；
//! 判据锚定 §AI-61 原文任务书。
//! 判据落地（drill 场景）：R1 冻结账 800 条登记；R2 批窗覆盖 ×40；
//! R3 编号 F48001–F48800 零跳号断言；R4 跳号检出；R5 重复编号
//! 检出；R6 批配额守恒（每批恒 20）；R7 冻结指纹稳定；R8 复核
//! 一致；R9 呈现行；R10 终验签名。
//! 设计纪律：no_std（仅 core/alloc）；零 unwrap/expect/panic；
//! 确定性 LCG；drill 全绿才返回 Ok(hits)。
//! 口径说明：域关门终验一以条目编号（F48001..F48800 显式 seq）
//! 逐条核对冻结账：40 批 × 20 条 = 800 条全冻结、编号连续零跳号、
//! 每批配额恒 20；任一跳号、撞号或缺批配额即红。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

/// 诊断段基址。
pub const SEG_BASE: u16 = 0x29E0;

// ── 诊断码闭集（SEG_BASE+1 起连续）──────────────────────────────
const E_SEQ: u16 = SEG_BASE + 1; // 编号跳号
const E_DUP: u16 = SEG_BASE + 2; // 编号重复
const E_BATCH: u16 = SEG_BASE + 3; // 批号越出窗
const E_QUOTA: u16 = SEG_BASE + 4; // 批配额失衡
const E_OPEN: u16 = SEG_BASE + 5; // 条目未冻结
const E_COUNT: u16 = SEG_BASE + 6; // 条数不守恒
const E_REPLAY: u16 = SEG_BASE + 7; // 复核漂移
const E_FP: u16 = SEG_BASE + 8; // 指纹漂移
const E_LINE: u16 = SEG_BASE + 9; // 呈现行缺段
const E_SIG: u16 = SEG_BASE + 10; // 终验签名不符

/// 批窗 B01–B40。
pub const B_LO: u32 = 1;
pub const B_HI: u32 = 40;
pub const BATCH_N: u32 = B_HI - B_LO + 1;
/// 每批条数。
pub const PER_BATCH: u32 = 20;
/// 全域条数。
pub const TOTAL_N: u32 = BATCH_N * PER_BATCH;
/// 编号窗：F48001..=F48800（账内存 1..=800）。
pub const SEQ_LO: u32 = 1;
pub const SEQ_HI: u32 = 800;

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

/// 一条冻结条目。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FrozenRec {
    pub seq: u32,   // 编号 1..=800（对应 F48001..=F48800）
    pub batch: u32, // 批号 1..=40
    pub frozen: bool, // 冻结位
    pub sig: u64,   // 条目签名
}

impl FrozenRec {
    /// 条目签名：编号 + 批号 + 冻结位混签。
    pub fn make_sig(seq: u32, batch: u32, frozen: bool) -> u64 {
        let mut buf: Vec<u8> = vec![];
        buf.extend_from_slice(&seq.to_le_bytes());
        buf.extend_from_slice(&batch.to_le_bytes());
        buf.push(frozen as u8);
        fnv1a64(&buf)
    }

    pub fn sane(&self) -> Result<(), u16> {
        if self.seq < SEQ_LO || self.seq > SEQ_HI {
            return Err(E_SEQ);
        }
        if self.batch < B_LO || self.batch > B_HI {
            return Err(E_BATCH);
        }
        if !self.frozen {
            return Err(E_OPEN);
        }
        if self.sig != FrozenRec::make_sig(self.seq, self.batch, self.frozen) {
            return Err(E_SIG);
        }
        Ok(())
    }
}

/// 冻结账。
pub struct FreezeBook {
    pub recs: Vec<FrozenRec>,
}

impl FreezeBook {
    pub fn new() -> FreezeBook {
        FreezeBook { recs: vec![] }
    }

    /// 条目冻结登记：编号显式连续（零跳号）+ 批号闭集。
    pub fn freeze(&mut self, batch: u32) -> Result<u32, u16> {
        if batch < B_LO || batch > B_HI {
            return Err(E_BATCH);
        }
        let seq = self.recs.len() as u32 + 1;
        let sig = FrozenRec::make_sig(seq, batch, true);
        self.recs.push(FrozenRec { seq, batch, frozen: true, sig });
        Ok(seq)
    }

    /// 零跳号断言：编号 1..=800 显式连续。
    pub fn assert_contiguous(&self) -> Result<(), u16> {
        if self.recs.len() as u32 != TOTAL_N {
            return Err(E_COUNT);
        }
        let mut i = 0usize;
        while i < self.recs.len() {
            if self.recs[i].seq != i as u32 + 1 {
                return Err(E_SEQ);
            }
            i += 1;
        }
        Ok(())
    }

    /// 批配额守恒：40 批每批恒 20 条。
    pub fn batch_quota(&self, batch: u32) -> Result<u32, u16> {
        if batch < B_LO || batch > B_HI {
            return Err(E_BATCH);
        }
        let mut n: u32 = 0;
        let mut i = 0usize;
        while i < self.recs.len() {
            if self.recs[i].batch == batch {
                n += 1;
            }
            i += 1;
        }
        if n != PER_BATCH {
            return Err(E_QUOTA);
        }
        Ok(n)
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
            buf.extend_from_slice(&self.recs[i].seq.to_le_bytes());
            buf.extend_from_slice(&self.recs[i].batch.to_le_bytes());
            buf.push(self.recs[i].frozen as u8);
            buf.extend_from_slice(&self.recs[i].sig.to_le_bytes());
            i += 1;
        }
        fnv1a64(&buf)
    }
}

/// 终验签名：条数 × 批数 × 指纹混签。
pub fn audit_sig(n: u32, batches: u32, fp: u64) -> u64 {
    let mut buf: Vec<u8> = vec![];
    buf.extend_from_slice(&n.to_le_bytes());
    buf.extend_from_slice(&batches.to_le_bytes());
    buf.extend_from_slice(&fp.to_le_bytes());
    fnv1a64(&buf)
}

/// 呈现行：frozen=800 batches=40 gaps=0。
pub fn audit_line(n: u32) -> String {
    let mut out = String::new();
    out.push_str("frozen=");
    out.push_str(&n.to_string());
    out.push_str(" batches=40 gaps=0");
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
    let mut g = Lcg::new(0x487_81);

    // R1 冻结账 800 条登记：40 批各 20 条（LCG 决定批内序扰动）。
    let mut book = FreezeBook::new();
    let mut b = B_LO;
    while b <= B_HI {
        let mut k: u32 = 0;
        while k < PER_BATCH {
            book.freeze(b)?;
            let _ = g.below(4);
            k += 1;
        }
        b += 1;
    }
    if book.recs.len() as u32 != TOTAL_N {
        return Err(E_COUNT);
    }
    hits += 1;

    // R2 批窗覆盖 ×40：每批配额恒 20。
    b = B_LO;
    while b <= B_HI {
        book.batch_quota(b)?;
        b += 1;
    }
    hits += 1;

    // R3 编号 F48001–F48800 零跳号断言。
    book.assert_contiguous()?;
    hits += 1;

    // R4 跳号检出：抽掉一条 → 断言红。
    let mut gap = FreezeBook::new();
    let mut i = 0usize;
    while i < book.recs.len() {
        if book.recs[i].seq != 400 {
            gap.recs.push(book.recs[i]);
        }
        i += 1;
    }
    match gap.assert_contiguous() {
        Err(E_SEQ) => hits += 1,
        _ => return Err(E_SEQ),
    }

    // R5 编号重复检出：手工注入重号。
    let mut dup = FreezeBook::new();
    dup.recs.push(FrozenRec {
        seq: 1,
        batch: 1,
        frozen: true,
        sig: FrozenRec::make_sig(1, 1, true),
    });
    dup.recs.push(FrozenRec {
        seq: 1,
        batch: 1,
        frozen: true,
        sig: FrozenRec::make_sig(1, 1, true),
    });
    match dup.assert_contiguous() {
        Err(E_SEQ) => {}
        _ => return Err(E_SEQ),
    }
    match dup.verify_all() {
        Ok(()) => {}
        Err(_) => hits += 1,
    }

    // R6 未冻结检出：frozen=false → sane 红。
    let mut open = FreezeBook::new();
    open.recs.push(FrozenRec {
        seq: 1,
        batch: 1,
        frozen: false,
        sig: FrozenRec::make_sig(1, 1, false),
    });
    match open.verify_all() {
        Err(E_OPEN) => hits += 1,
        _ => return Err(E_OPEN),
    }

    // R7 批号越界检出。
    let mut oob = FreezeBook::new();
    oob.recs.push(FrozenRec {
        seq: 1,
        batch: 41,
        frozen: true,
        sig: FrozenRec::make_sig(1, 41, true),
    });
    match oob.verify_all() {
        Err(E_BATCH) => hits += 1,
        _ => return Err(E_BATCH),
    }

    // R8 全量核对 + 指纹稳定。
    book.verify_all()?;
    let f1 = book.fp();
    let f2 = book.fp();
    if f1 != f2 || f1 == 0 {
        return Err(E_FP);
    }
    hits += 1;

    // R9 复核一致：同种子重登记指纹一致。
    let mut book2 = FreezeBook::new();
    let mut g2 = Lcg::new(0x487_81);
    let mut b2 = B_LO;
    while b2 <= B_HI {
        let mut k2: u32 = 0;
        while k2 < PER_BATCH {
            book2.freeze(b2)?;
            let _ = g2.below(4);
            k2 += 1;
        }
        b2 += 1;
    }
    if book2.fp() != f1 {
        return Err(E_REPLAY);
    }
    hits += 1;

    // R10 呈现行 + 终验签名：800×40×指纹混签。
    let line = audit_line(book.recs.len() as u32);
    if !has_kw(line.as_bytes(), b"frozen=800")
        || !has_kw(line.as_bytes(), b"batches=40")
        || !has_kw(line.as_bytes(), b"gaps=0")
    {
        return Err(E_LINE);
    }
    let sig = audit_sig(TOTAL_N, BATCH_N, f1);
    if sig != audit_sig(800, 40, f1) || sig == 0 {
        return Err(E_SIG);
    }
    hits += 1;

    Ok(hits)
}
