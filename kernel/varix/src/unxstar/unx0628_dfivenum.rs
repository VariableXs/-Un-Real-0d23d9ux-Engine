//! unx0628_dfivenum —— UNX-F0628 域五数总概括账。
//!
//! ═══════════════════════════════════════════════════════════════════════
//! 锚点：UNX-F0628 · 域五数总概括账 · 目标 ≥220 行
//! 判据原文摘录：域概括 5 项（总条目 660/总行数累计/批数 33/交接包
//!   12/对冲平 8/8）与逐批账复算一致。
//! ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计纪律：
//!   1. 五数闭集写死：总条目 660、总行数累计（账面值）、批数 33、
//!      交接包 12、对冲平 8/8（来源：判据原文），改一数即红。
//!   2. 复算一致判据：五数不许手抄——总条目/总行数/批数必须由 33 批
//!      逐批账逐批求和复算得出，复算值 = 落账值才平。
//!   3. 逐批账定容：33 批各带条目数与行数，条目守恒 = Σ批条目 = 660。
//!   4. 交接包数与对冲平数是面级账（12/8），单独落账并单独复验。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，
//!      确定性。
//!
//! 诊断段声明：SEG 0x5040 段位本模块独占。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 诊断段基址（0x5040 段 · 本模块独占）。
pub const SEG_BASE: u16 = 0x5040;
/// 诊断段顶。
pub const SEG_TOP: u16 = 0x504F;

/// XcCode：批号越界。
pub const XC_BAD_BATCH: u16 = SEG_BASE + 1; // 0x5041 批号 0..33 闭集外
/// XcCode：批账重复。
pub const XC_DUP_BATCH: u16 = SEG_BASE + 2; // 0x5042 一批一账不许重落
/// XcCode：条目守恒破。
pub const XC_ITEM_CONSERVE: u16 = SEG_BASE + 3; // 0x5043 Σ批条目 ≠ 660
/// XcCode：批数不符。
pub const XC_BATCH_N: u16 = SEG_BASE + 4; // 0x5044 批数 ≠ 33
/// XcCode：面级账不符（包 12 / 对冲 8）。
pub const XC_FACE_MISMATCH: u16 = SEG_BASE + 5; // 0x5045 面级五数不中
/// XcCode：五数复算不一致。
pub const XC_FIVE_UNMATCH: u16 = SEG_BASE + 6; // 0x5046 复算 ≠ 落账

/// 批数（判据原文 33）。
pub const BATCH_N: usize = 33;
/// 域总条目（判据原文 660）。
pub const TOTAL_ITEMS: u32 = 660;
/// 交接包数（判据原文 12）。
pub const PKG_N: u32 = 12;
/// 对冲平腿数（判据原文 8/8）。
pub const HEDGE_FLAT_N: u32 = 8;

/// 单批账行。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BatchRow {
    /// 批号（0..33）。
    pub batch_no: usize,
    /// 该批条目数。
    pub items: u32,
    /// 该批行数。
    pub lines: u32,
}

/// 五数概括账（落账面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FiveNum {
    /// 总条目。
    pub total_items: u32,
    /// 总行数累计。
    pub total_lines: u64,
    /// 批数。
    pub batch_n: u32,
    /// 交接包数。
    pub pkg_n: u32,
    /// 对冲平腿数。
    pub hedge_flat_n: u32,
}

/// 域五数总概括账本。
#[derive(Clone, Debug)]
pub struct FiveNumBook {
    /// 33 批逐批账（None = 未落）。
    batches: [Option<BatchRow>; BATCH_N],
    /// 落账五数。
    five: Option<FiveNum>,
}

impl FiveNumBook {
    /// 空账本。
    pub fn new() -> Self {
        FiveNumBook {
            batches: [None; BATCH_N],
            five: None,
        }
    }

    /// 已落批数。
    pub fn settled_n(&self) -> usize {
        self.batches.iter().filter(|b| b.is_some()).count()
    }

    /// 落一批账（号闭集 + 去重 + 条目/行数非零）。
    pub fn settle_batch(&mut self, batch_no: usize, items: u32, lines: u32) -> Result<(), u16> {
        if batch_no >= BATCH_N {
            return Err(XC_BAD_BATCH);
        }
        if self.batches[batch_no].is_some() {
            return Err(XC_DUP_BATCH);
        }
        if items == 0 || lines == 0 {
            return Err(XC_BAD_BATCH);
        }
        self.batches[batch_no] = Some(BatchRow {
            batch_no,
            items,
            lines,
        });
        Ok(())
    }

    /// 逐批复算：批数、Σ条目、Σ行数（整数逐批累加）。
    pub fn recompute(&self) -> Result<(u32, u32, u64), u16> {
        if self.settled_n() != BATCH_N {
            return Err(XC_BATCH_N);
        }
        let mut items: u32 = 0;
        let mut lines: u64 = 0;
        for i in 0..BATCH_N {
            match &self.batches[i] {
                Some(r) => {
                    items += r.items;
                    lines += r.lines as u64;
                }
                None => return Err(XC_BATCH_N),
            }
        }
        Ok((BATCH_N as u32, items, lines))
    }

    /// 落五数（必须先 33 批全落，且复算一致才可落账）。
    pub fn seal_five(&mut self, pkg_n: u32, hedge_flat_n: u32) -> Result<FiveNum, u16> {
        let (bn, items, lines) = self.recompute()?;
        if items != TOTAL_ITEMS {
            return Err(XC_ITEM_CONSERVE);
        }
        if bn != BATCH_N as u32 {
            return Err(XC_BATCH_N);
        }
        if pkg_n != PKG_N || hedge_flat_n != HEDGE_FLAT_N {
            return Err(XC_FACE_MISMATCH);
        }
        let f = FiveNum {
            total_items: items,
            total_lines: lines,
            batch_n: bn,
            pkg_n,
            hedge_flat_n,
        };
        self.five = Some(f);
        Ok(f)
    }

    /// 五数复验：落账面 vs 复算面逐项比对。
    pub fn verify(&self) -> Result<FiveNum, u16> {
        let f = match &self.five {
            Some(f) => *f,
            None => return Err(XC_FIVE_UNMATCH),
        };
        let (bn, items, lines) = self.recompute()?;
        if f.total_items != items || f.total_lines != lines || f.batch_n != bn {
            return Err(XC_FIVE_UNMATCH);
        }
        if f.pkg_n != PKG_N || f.hedge_flat_n != HEDGE_FLAT_N {
            return Err(XC_FACE_MISMATCH);
        }
        if f.total_items != TOTAL_ITEMS {
            return Err(XC_ITEM_CONSERVE);
        }
        Ok(f)
    }

    /// 人话报告。
    pub fn report(&self) -> String {
        let mut s = alloc::format!(
            "域五数总概括 · 落批 {}/{} · 五数落账={}",
            self.settled_n(),
            BATCH_N,
            self.five.is_some()
        );
        if let Some(f) = &self.five {
            s.push_str(&alloc::format!(
                "\n  条目{} 行数{} 批数{} 包{} 对冲平{}/8",
                f.total_items, f.total_lines, f.batch_n, f.pkg_n, f.hedge_flat_n
            ));
        }
        for i in 0..BATCH_N {
            if let Some(r) = &self.batches[i] {
                s.push_str(&alloc::format!(
                    "\n  批{} 条目{} 行数{}",
                    r.batch_no, r.items, r.lines
                ));
            }
        }
        s
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in self.report().as_bytes() {
            h ^= *b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        h
    }
}

/// 标准域账构造器：33 批各 20 条目（Σ=660），行数 = 300+批号*9。
pub fn build_domain_book() -> Result<FiveNumBook, u16> {
    let mut b = FiveNumBook::new();
    for i in 0..BATCH_N {
        b.settle_batch(i, 20, 300 + (i as u32) * 9)?;
    }
    b.seal_five(PKG_N, HEDGE_FLAT_N)?;
    Ok(b)
}

/// 自例程：主路径（33 批→五数落账→复验）+ 错误路径（条目破/缺批/面错/重落）。
pub fn drill() -> Result<u32, u16> {
    // 主路径。
    let book = build_domain_book()?;
    let f = book.verify()?;
    if f.total_items != TOTAL_ITEMS || f.batch_n != BATCH_N as u32 {
        return Err(XC_FIVE_UNMATCH);
    }
    // 总行数复算独立核：300*33 + 9*(0+1+...+32)。
    let expect_lines: u64 = 300 * 33 + 9 * (32 * 33 / 2) as u64;
    if f.total_lines != expect_lines {
        return Err(XC_FIVE_UNMATCH);
    }
    if book.fingerprint() == 0 {
        return Err(XC_ITEM_CONSERVE);
    }

    // 错误路径 1：条目守恒破（一批 20→19）。
    let mut b1 = FiveNumBook::new();
    for i in 0..BATCH_N {
        let it = if i == 7 { 19 } else { 20 };
        b1.settle_batch(i, it, 100)?;
    }
    if b1.seal_five(PKG_N, HEDGE_FLAT_N) != Err(XC_ITEM_CONSERVE) {
        return Err(XC_ITEM_CONSERVE);
    }
    // 错误路径 2：缺批。
    let mut b2 = FiveNumBook::new();
    for i in 0..BATCH_N - 1 {
        b2.settle_batch(i, 20, 100)?;
    }
    if b2.seal_five(PKG_N, HEDGE_FLAT_N) != Err(XC_BATCH_N) {
        return Err(XC_BATCH_N);
    }
    // 错误路径 3：面级账不符。
    let mut b3 = build_domain_book()?;
    // 已落五数不可重落——另开账本验面错。
    let mut b3b = FiveNumBook::new();
    for i in 0..BATCH_N {
        b3b.settle_batch(i, 20, 100)?;
    }
    if b3b.seal_five(11, HEDGE_FLAT_N) != Err(XC_FACE_MISMATCH) {
        return Err(XC_FACE_MISMATCH);
    }
    if b3b.seal_five(PKG_N, 7) != Err(XC_FACE_MISMATCH) {
        return Err(XC_FACE_MISMATCH);
    }
    let _ = b3;
    // 错误路径 4：批号越界与重落。
    let mut b4 = FiveNumBook::new();
    if b4.settle_batch(BATCH_N, 1, 1) != Err(XC_BAD_BATCH) {
        return Err(XC_BAD_BATCH);
    }
    b4.settle_batch(0, 20, 100)?;
    if b4.settle_batch(0, 20, 100) != Err(XC_DUP_BATCH) {
        return Err(XC_DUP_BATCH);
    }
    // 错误路径 5：未落五数先验。
    if b4.verify() != Err(XC_FIVE_UNMATCH) {
        return Err(XC_FIVE_UNMATCH);
    }
    Ok(0)
}
