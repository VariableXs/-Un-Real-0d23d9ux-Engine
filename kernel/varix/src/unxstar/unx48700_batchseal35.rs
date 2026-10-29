//! UNX-F48700 —— I35 批收官印（20 条冻结/累计 210,000 行）
//! 判据原文：UNX-F48700-J1 批印入册：域账 210,000/240,000；
//! 判据锚定 §AI-61 §7。
//! 判据落地（drill 场景）：R1 二十条在册；R2 未冻即拒；R3 封印与
//! 累计行数；R4 域账口径；R5 冻结后禁改；R6 印章指纹；R7 卡号连续；
//! R8 重封幂等；R9 卡号重复拒；R10 呈现行。
//! 设计纪律：no_std（仅 core/alloc）；零 unwrap/expect/panic；
//! 确定性 LCG；drill 全绿才返回 Ok(hits)。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

/// 诊断段基址。
pub const SEG_BASE: u16 = 0x0EE0;

// ── 诊断码闭集（SEG_BASE+1 起连续）──────────────────────────────
const E_COUNT: u16 = SEG_BASE + 1; // 收官条数不足 20
const E_FREEZE: u16 = SEG_BASE + 2; // 冻结态异常
const E_ROWS: u16 = SEG_BASE + 3; // 累计行数失配
const E_DOMAIN: u16 = SEG_BASE + 4; // 域账口径失配
const E_READONLY: u16 = SEG_BASE + 5; // 冻结后改写未拒
const E_FP: u16 = SEG_BASE + 6; // 印章指纹漂移
const E_GAP: u16 = SEG_BASE + 7; // 卡号跳号或重复
const E_UNSEALED: u16 = SEG_BASE + 8; // 未冻即出印
const E_SEAL: u16 = SEG_BASE + 9; // 重封不幂等
const E_LINE: u16 = SEG_BASE + 10; // 呈现行缺段

/// 收官条数。
pub const SEAL_N: usize = 20;
/// 本批累计行数。
pub const BATCH_ROWS: u32 = 210_000;
/// 域账上限。
pub const DOMAIN_ROWS: u32 = 240_000;
/// 首卡号。
pub const CARD_BASE: u32 = 48_683;
/// 状态闭集。
pub const ST_OPEN: u8 = 1;
pub const ST_SEALED: u8 = 2;

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

/// 一条收官条目。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SealItem {
    pub card_no: u32,
    pub rows: u32,
    pub frozen: bool,
}

/// 批收官印。
pub struct BatchSeal {
    pub items: Vec<SealItem>,
    pub state: u8,
    pub sealed_at: u32,
}

impl BatchSeal {
    pub fn new() -> BatchSeal {
        BatchSeal { items: vec![], state: ST_OPEN, sealed_at: 0 }
    }

    /// 收官登记。
    pub fn register(&mut self, card_no: u32, rows: u32) -> Result<(), u16> {
        if self.state == ST_SEALED {
            return Err(E_READONLY);
        }
        let mut i = 0usize;
        while i < self.items.len() {
            if self.items[i].card_no == card_no {
                return Err(E_GAP);
            }
            i += 1;
        }
        self.items.push(SealItem { card_no, rows, frozen: false });
        Ok(())
    }

    /// 封印。
    pub fn seal(&mut self, at: u32) -> Result<(), u16> {
        if self.items.len() != SEAL_N {
            return Err(E_COUNT);
        }
        let mut i = 0usize;
        while i < self.items.len() {
            self.items[i].frozen = true;
            i += 1;
        }
        self.state = ST_SEALED;
        self.sealed_at = at;
        Ok(())
    }

    /// 冻结数。
    pub fn frozen_count(&self) -> u32 {
        let mut c: u32 = 0;
        let mut i = 0usize;
        while i < self.items.len() {
            if self.items[i].frozen {
                c += 1;
            }
            i += 1;
        }
        c
    }

    /// 累计行数。
    pub fn total_rows(&self) -> u32 {
        let mut s: u32 = 0;
        let mut i = 0usize;
        while i < self.items.len() {
            s = s.wrapping_add(self.items[i].rows);
            i += 1;
        }
        s
    }

    /// 域账口径。
    pub fn domain_ok(&self) -> bool {
        self.total_rows() <= DOMAIN_ROWS
    }

    /// 印章指纹。
    pub fn seal_fp(&self) -> u64 {
        let mut buf: Vec<u8> = vec![];
        let mut i = 0usize;
        while i < self.items.len() {
            buf.extend_from_slice(&self.items[i].card_no.to_le_bytes());
            buf.extend_from_slice(&self.items[i].rows.to_le_bytes());
            buf.push(self.items[i].frozen as u8);
            i += 1;
        }
        buf.push(self.state);
        buf.extend_from_slice(&self.sealed_at.to_le_bytes());
        fnv1a64(&buf)
    }

    /// 卡号连续。
    pub fn cards_contiguous(&self) -> bool {
        if self.items.len() != SEAL_N {
            return false;
        }
        let mut i = 0usize;
        while i < SEAL_N {
            if self.items[i].card_no != CARD_BASE + i as u32 {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 呈现行。
pub fn seal_line(b: &BatchSeal) -> String {
    let mut out = String::new();
    out.push_str("n=");
    out.push_str(&b.items.len().to_string());
    out.push_str(" frozen=");
    out.push_str(&b.frozen_count().to_string());
    out.push_str(" rows=");
    out.push_str(&b.total_rows().to_string());
    if b.state == ST_SEALED {
        out.push_str(" state=sealed");
    } else {
        out.push_str(" state=open");
    }
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
    let mut g = Lcg::new(0x487_00);

    // R1 二十条在册：行数合计恰 210,000。
    let mut seal = BatchSeal::new();
    let mut i: u32 = 0;
    while i < SEAL_N as u32 {
        let rows = if i == 19 {
            let mut s: u32 = 0;
            let mut k: u32 = 0;
            while k < 19 {
                s += 10_500;
                k += 1;
            }
            BATCH_ROWS - s
        } else {
            10_500
        };
        seal.register(CARD_BASE + i, rows)?;
        i += 1;
    }
    if seal.items.len() != SEAL_N {
        return Err(E_COUNT);
    }
    hits += 1;

    // R2 未冻即拒。
    if seal.state == ST_SEALED || seal.frozen_count() != 0 {
        return Err(E_UNSEALED);
    }
    hits += 1;

    // R3 封印 + 累计。
    seal.seal(0x0F00)?;
    if seal.frozen_count() != SEAL_N as u32 || seal.total_rows() != BATCH_ROWS {
        return Err(E_ROWS);
    }
    hits += 1;

    // R4 域账口径。
    if !seal.domain_ok() || BATCH_ROWS > DOMAIN_ROWS {
        return Err(E_DOMAIN);
    }
    hits += 1;

    // R5 冻结后禁改。
    match seal.register(CARD_BASE + 50, 100) {
        Err(E_READONLY) => hits += 1,
        _ => return Err(E_READONLY),
    }

    // R6 印章指纹。
    let f1 = seal.seal_fp();
    let f2 = seal.seal_fp();
    if f1 != f2 || f1 == 0 {
        return Err(E_FP);
    }
    hits += 1;

    // R7 卡号连续。
    if !seal.cards_contiguous() {
        return Err(E_GAP);
    }
    hits += 1;

    // R8 重封幂等。
    seal.seal(0x0F01)?;
    if seal.total_rows() != BATCH_ROWS || seal.seal_fp() != f1 {
        return Err(E_SEAL);
    }
    hits += 1;

    // R9 卡号重复拒。
    let mut probe = BatchSeal::new();
    probe.register(11, 10)?;
    match probe.register(11, 20) {
        Err(E_GAP) => hits += 1,
        _ => return Err(E_GAP),
    }

    // R10 呈现行：n=20 rows=210000 state=sealed。
    let line = seal_line(&seal);
    if !has_kw(line.as_bytes(), b"n=20") || !has_kw(line.as_bytes(), b"rows=210000")
        || !has_kw(line.as_bytes(), b"state=sealed")
    {
        return Err(E_LINE);
    }
    let _ = g.below(10);
    hits += 1;

    Ok(hits)
}
