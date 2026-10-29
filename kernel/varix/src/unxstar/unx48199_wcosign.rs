//! unx48199_wcosign — UNX-F48199 B10 批 AI-19/21 联签执行账（消息面映射序联签登记）（诊断段 0xB9C0）。
//!
//! 判据原文：
//!   J1 联签登记入册；判据锚定 §AI-61 §8。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 联签对象是"消息面映射序"：每条目收一组有序映射项
//!      MapItem{seq, surface_in, surface_out}（IMM 面 id → Wine 面
//!      id 的映射步），seq 必须严格递增——映射序乱序即拒收，联签
//!      签的是"序"本身，不是散点。
//!   2. 双主联签：AI-19（域主）与 AI-21（审计主）各落一签；签值 =
//!      FNV-1a 64(映射序本体)，AI-21 签值再叠 0xA121 审计盐。两签
//!      与账面重算摘要各自相等才许 seal——签错位/签错条目当场炸账。
//!   3. seal 前置：条目未满双签即 seal 拒；seal 后条目冻结，任何
//!      追签/改签拒（E_FROZEN）——收官账不允许回笔。
//!   4. 入册防重：条目号唯一（E_DUP_NO）；空映射序不入册
//!      （E_SEQ_EMPTY）；空册不得出总审计。
//!   5. 入册口径（判据 J1）：×8 条目全部双签齐 + 摘要核销 + 逐条
//!      严格递增复核 → 全册 seal 并入册；一条不齐即 Err。
//!   6. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xB9C0;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_SEQ_ORDER: u16 = SEG_BASE + 1; // 映射序非严格递增
const E_SEQ_EMPTY: u16 = SEG_BASE + 2; // 空映射序入册
const E_DUP_NO: u16 = SEG_BASE + 3; // 条目号重复
const E_SIG_BAD: u16 = SEG_BASE + 4; // 签值与重算摘要不符
const E_UNSEAL: u16 = SEG_BASE + 5; // 双签未齐即 seal
const E_FROZEN: u16 = SEG_BASE + 6; // 冻结条目追签/改签
const E_BOOK_MISS: u16 = SEG_BASE + 7; // 条目不在册
const E_EMPTY_SUM: u16 = SEG_BASE + 8; // 写回摘要为空

// ── 消息面 id 域（IMM 面 → Wine 面映射口径）────────────────
pub const IMM_FACE_BASE: u32 = 0xE000;
pub const WINE_FACE_BASE: u32 = 0x0280;
/// 映射步内最大面偏移（防越域造号）。
pub const FACE_SPAN: u32 = 0x0020;

/// 一条映射步：序号 + IMM 面 id + Wine 面 id。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MapItem {
    pub seq: u16,
    pub surface_in: u32,
    pub surface_out: u32,
}

/// 联签条目：映射序 + 双签 + 冻结位。
#[derive(Clone, Debug)]
pub struct CosignEntry {
    pub no: u16,
    pub items: Vec<MapItem>,
    pub sig19: Option<u64>,
    pub sig21: Option<u64>,
    pub sealed: bool,
}

impl CosignEntry {
    /// 摘要：条目号 + 映射序逐项（seq/in/out 三元展开）FNV-1a 64。
    pub fn digest(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        h ^= self.no as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
        let mut i = 0usize;
        while i < self.items.len() {
            let it = self.items[i];
            h ^= it.seq as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01B3);
            h ^= it.surface_in as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01B3);
            h ^= it.surface_out as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01B3);
            i += 1;
        }
        h
    }

    /// 映射序严格递增复核（纪律 1）。
    pub fn order_ok(&self) -> bool {
        let mut i = 1usize;
        while i < self.items.len() {
            if self.items[i].seq <= self.items[i - 1].seq {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 联签主身份。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Principal {
    Ai19,
    Ai21,
}

/// 联签总册。
pub struct CosignBook {
    pub entries: Vec<CosignEntry>,
}

impl CosignBook {
    pub fn new() -> CosignBook {
        CosignBook { entries: Vec::new() }
    }

    /// 登记映射序条目（纪律 4）：号查重、空序拒、序乱拒。
    pub fn register(&mut self, no: u16, items: Vec<MapItem>) -> Result<(), u16> {
        if items.len() == 0 {
            return Err(E_SEQ_EMPTY);
        }
        let mut i = 0usize;
        while i < self.entries.len() {
            if self.entries[i].no == no {
                return Err(E_DUP_NO);
            }
            i += 1;
        }
        let mut e = CosignEntry { no, items, sig19: None, sig21: None, sealed: false };
        if !e.order_ok() {
            return Err(E_SEQ_ORDER);
        }
        self.entries.push(e);
        Ok(())
    }

    /// 找条目下标。
    fn find(&self, no: u16) -> Option<usize> {
        let mut i = 0usize;
        while i < self.entries.len() {
            if self.entries[i].no == no {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 落签（纪律 2）：签值即摘要重算；冻结条目拒签。
    pub fn sign(&mut self, no: u16, who: Principal) -> Result<u64, u16> {
        let idx = match self.find(no) {
            Some(i) => i,
            None => return Err(E_BOOK_MISS),
        };
        if self.entries[idx].sealed {
            return Err(E_FROZEN);
        }
        let sig = self.entries[idx].digest();
        match who {
            Principal::Ai19 => self.entries[idx].sig19 = Some(sig),
            Principal::Ai21 => self.entries[idx].sig21 = Some(sig.wrapping_add(0xA121)),
        }
        Ok(sig)
    }

    /// seal（纪律 3）：双签齐 + 两签核销 + 序复核，封后冻结。
    pub fn seal(&mut self, no: u16) -> Result<(), u16> {
        let idx = match self.find(no) {
            Some(i) => i,
            None => return Err(E_BOOK_MISS),
        };
        if self.entries[idx].sealed {
            return Err(E_FROZEN);
        }
        let s19 = match self.entries[idx].sig19 {
            Some(s) => s,
            None => return Err(E_UNSEAL),
        };
        let s21 = match self.entries[idx].sig21 {
            Some(s) => s,
            None => return Err(E_UNSEAL),
        };
        let d = self.entries[idx].digest();
        if s19 != d || s21 != d.wrapping_add(0xA121) {
            return Err(E_SIG_BAD);
        }
        if !self.entries[idx].order_ok() {
            return Err(E_SEQ_ORDER);
        }
        self.entries[idx].sealed = true;
        Ok(())
    }

    /// 全册审计（判据 J1）：×N 条目全 sealed 且号连续不重。
    pub fn audit_all(&self, want: u32) -> Result<u32, u16> {
        if (self.entries.len() as u32) < want {
            return Err(E_BOOK_MISS);
        }
        let mut sealed_n = 0u32;
        let mut i = 0usize;
        while i < self.entries.len() {
            if !self.entries[i].sealed {
                return Err(E_UNSEAL);
            }
            if !self.entries[i].order_ok() {
                return Err(E_SEQ_ORDER);
            }
            let mut j = i + 1;
            while j < self.entries.len() {
                if self.entries[i].no == self.entries[j].no {
                    return Err(E_DUP_NO);
                }
                j += 1;
            }
            sealed_n += 1;
            i += 1;
        }
        Ok(sealed_n)
    }
}

/// LCG 映射序生成器（确定性）。
pub struct Lcg {
    s: u32,
}

impl Lcg {
    pub fn new(seed: u32) -> Lcg {
        Lcg { s: seed }
    }
    pub fn next(&mut self) -> u32 {
        self.s = self.s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.s
    }
    pub fn below(&mut self, n: u32) -> u32 {
        if n == 0 { 0 } else { self.next() % n }
    }
}

/// 生成一条合法映射序（seq 从 1 严格递增，面 id 带内）。
pub fn gen_items(rng: &mut Lcg, n: u32) -> Vec<MapItem> {
    let mut out: Vec<MapItem> = Vec::new();
    let mut seq: u16 = 1;
    let mut i = 0u32;
    while i < n {
        let off = rng.below(FACE_SPAN);
        out.push(MapItem {
            seq,
            surface_in: IMM_FACE_BASE + off,
            surface_out: WINE_FACE_BASE + off * 2,
        });
        seq += 1;
        i += 1;
    }
    out
}

/// 写回判定（判据 J2）。
fn writeback(done: bool, summary_len: u32) -> Result<(), u16> {
    if !done || summary_len == 0 {
        return Err(E_EMPTY_SUM);
    }
    Ok(())
}

/// 自例程判据：逐条落实 J1（联签登记入册），返回命中数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    let mut rng = Lcg::new(0xB9C0_0008);

    // R1 主判据：×8 条目登记 → 双签 → seal → 全册审计 ×8 全封。
    let mut book = CosignBook::new();
    let mut no: u16 = 1;
    while no <= 8 {
        let items = gen_items(&mut rng, 3 + (no as u32 % 3));
        book.register(no, items)?;
        book.sign(no, Principal::Ai19)?;
        // 双签未齐即 seal 必拒（逐条探针）。
        if book.seal(no).is_ok() {
            return Err(E_UNSEAL);
        }
        book.sign(no, Principal::Ai21)?;
        book.seal(no)?;
        no += 1;
    }
    let sealed = book.audit_all(8)?;
    if sealed != 8 {
        return Err(E_UNSEAL);
    }
    hits += 1;

    // R2 签值核销：AI-19 签值 == 摘要；AI-21 签值 == 摘要+盐（纪律 2）。
    let mut book = CosignBook::new();
    let items = gen_items(&mut rng, 5);
    book.register(0x11, items)?;
    let d19 = book.sign(0x11, Principal::Ai19)?;
    let d21 = book.sign(0x11, Principal::Ai21)?;
    let d = match book.entries.first() {
        Some(e) => e.digest(),
        None => return Err(E_BOOK_MISS),
    };
    if d19 != d || d21 != d.wrapping_add(0xA121) {
        return Err(E_SIG_BAD);
    }
    hits += 1;

    // R3 序乱拒收：seq 回退序（1,1 / 3,2）两条构造样本均拒。
    let mut book = CosignBook::new();
    let bad1: Vec<MapItem> = {
        let mut v = Vec::new();
        v.push(MapItem { seq: 1, surface_in: IMM_FACE_BASE, surface_out: WINE_FACE_BASE });
        v.push(MapItem { seq: 1, surface_in: IMM_FACE_BASE + 1, surface_out: WINE_FACE_BASE + 2 });
        v
    };
    if book.register(0x21, bad1).is_ok() {
        return Err(E_SEQ_ORDER);
    }
    let bad2: Vec<MapItem> = {
        let mut v = Vec::new();
        v.push(MapItem { seq: 3, surface_in: IMM_FACE_BASE, surface_out: WINE_FACE_BASE });
        v.push(MapItem { seq: 2, surface_in: IMM_FACE_BASE + 1, surface_out: WINE_FACE_BASE + 2 });
        v
    };
    if book.register(0x22, bad2).is_ok() {
        return Err(E_SEQ_ORDER);
    }
    hits += 1;

    // R4 入册防重：同号重入册、空序入册、在册外条目落签各拒一次。
    let items = gen_items(&mut rng, 2);
    book.register(0x23, items)?;
    if book.register(0x23, gen_items(&mut rng, 2)).is_ok() {
        return Err(E_DUP_NO);
    }
    if book.register(0x24, Vec::new()).is_ok() {
        return Err(E_SEQ_EMPTY);
    }
    if book.sign(0x99, Principal::Ai19).is_ok() {
        return Err(E_BOOK_MISS);
    }
    hits += 1;

    // R5 冻结回笔拒：seal 后追签必拒（收官账不回笔）。
    let mut book = CosignBook::new();
    book.register(0x31, gen_items(&mut rng, 3))?;
    book.sign(0x31, Principal::Ai19)?;
    book.sign(0x31, Principal::Ai21)?;
    book.seal(0x31)?;
    if book.sign(0x31, Principal::Ai19).is_ok() {
        return Err(E_FROZEN);
    }
    if book.seal(0x31).is_ok() {
        return Err(E_FROZEN);
    }
    hits += 1;

    // R6 签错位炸账：AI-21 未签、AI-19 签后篡改映射序 → seal 必报
    //    E_SIG_BAD（签的是序本体，序动签必破）。经 register 内联改
    //    签不可行，改用"签后重开册重算摘要不等"路径核销。
    let mut book = CosignBook::new();
    book.register(0x41, gen_items(&mut rng, 4))?;
    let s1 = book.sign(0x41, Principal::Ai19)?;
    book.sign(0x41, Principal::Ai21)?;
    // 模拟签值漂移：把 AI-21 签换成 AI-19 值再 seal → 核销破。
    let idx = match book.entries.first_mut() {
        Some(e) => e,
        None => return Err(E_BOOK_MISS),
    };
    idx.sig21 = Some(s1);
    if book.seal(0x41).is_ok() {
        return Err(E_SIG_BAD);
    }
    hits += 1;

    // R7 写回（判据 J2）：完成态 + 非空摘要放行；空摘要拒。
    if writeback(true, 0).is_ok() {
        return Err(E_EMPTY_SUM);
    }
    writeback(true, 0x19A21)?;
    hits += 1;

    Ok(hits)
}
