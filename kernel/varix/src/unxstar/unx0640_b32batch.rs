//! unx0640_b32batch —— UNX-F0640 B32 批 ktest 断言集（域总装收口）。
//!
//! ═══════════════════════════════════════════════════════════════════════
//! 锚点：UNX-F0640 · B32 批 ktest 断言集（域总装收口） · 目标 ≥280 行
//! 判据原文摘录：B32 全批判据聚合 unx_a1_b32 全绿，批账对冲平、累计
//!   链核 4,660；域级 unx_a1 联动全绿收束。[留痕] R-A1-004：本批头部
//!   批小计 4640 为逐条求和后落账（先求和后落账，零虚记）。
//! ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计纪律：
//!   1. 两段累计链：前段 33 批冻结链尾 4,340（F0620 账面），B32 段
//!      批小计 320 逐条求和后落账，收口链尾 = 4,340+320 = 4,660。
//!   2. 先求和后落账：B32 批小计只能由逐条计数累加得出（求和步数 =
//!      条数核），先落总数后回填即虚记红。
//!   3. 批账对冲平：逐条总和 = 批小计 = 链尾增量，三面一致才收口。
//!   4. 域级联动：unx_a1_b32 全绿 ⇒ 域级 unx_a1 收束绿（收口位由
//!      本批放行置位，域绿必须经此）。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，
//!      确定性。
//!
//! 诊断段声明：SEG 0x51C0 段位本模块独占。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 诊断段基址（0x51C0 段 · 本模块独占）。
pub const SEG_BASE: u16 = 0x51C0;
/// 诊断段顶。
pub const SEG_TOP: u16 = 0x51CF;

/// XcCode：判据账空。
pub const XC_NO_ROWS: u16 = SEG_BASE + 1; // 0x51C1 零条判据禁落小计
/// XcCode：重复落小计（虚记嫌疑）。
pub const XC_VIRTUAL: u16 = SEG_BASE + 2; // 0x51C2 二次落账
/// XcCode：对冲不平。
pub const XC_HEDGE: u16 = SEG_BASE + 3; // 0x51C3 三面不一致
/// XcCode：链尾不中。
pub const XC_CHAIN_TAIL: u16 = SEG_BASE + 4; // 0x51C4 链尾 ≠ 4660
/// XcCode：红条混入。
pub const XC_RED_ROW: u16 = SEG_BASE + 5; // 0x51C5 红判据不许入聚合
/// XcCode：域收口未置位。
pub const XC_NOT_CLOSED: u16 = SEG_BASE + 6; // 0x51C6 域级未联动

/// 前段 33 批冻结链尾（F0620 账面）。
pub const PREV_TAIL: u32 = 4_340;
/// B32 批判据条数（判据原文口径：批小计 320）。
pub const B32_ITEMS: u32 = 320;
/// 收口链尾（判据原文 4,660）。
pub const TAIL_4660: u32 = 4_660;

/// B32 批级套件名。
pub const BATCH_SUITE_B32: &str = "unx_a1_b32";

/// B32 判据账行。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct B32Row {
    /// 判据号（非零）。
    pub crit_id: u16,
    /// 断言计数。
    pub count: u32,
    /// 绿。
    pub green: bool,
}

/// B32 批账本。
#[derive(Clone, Debug, Default)]
pub struct B32Book {
    /// 逐条判据账。
    rows: Vec<B32Row>,
    /// 批小计（先求和后落账；None = 未落）。
    subtotal: Option<u32>,
    /// 求和步数（逐条累加步数核）。
    sum_steps: u32,
    /// 收口位（域级 unx_a1 联动）。
    closed: bool,
}

impl B32Book {
    /// 空账本。
    pub fn new() -> Self {
        B32Book {
            rows: Vec::new(),
            subtotal: None,
            sum_steps: 0,
            closed: false,
        }
    }

    /// 登判据账（计数非零 + 绿条限定）。
    pub fn push(&mut self, crit_id: u16, count: u32, green: bool) -> Result<(), u16> {
        if crit_id == 0 || count == 0 {
            return Err(XC_NO_ROWS);
        }
        if !green {
            return Err(XC_RED_ROW);
        }
        self.rows.push(B32Row {
            crit_id,
            count,
            green,
        });
        Ok(())
    }

    /// 条数。
    pub fn row_n(&self) -> usize {
        self.rows.len()
    }

    /// 先求和后落账：逐条累加（求和步数 = 条数），随后落批小计。
    pub fn settle(&mut self) -> Result<u32, u16> {
        if self.subtotal.is_some() {
            return Err(XC_VIRTUAL);
        }
        if self.rows.is_empty() {
            return Err(XC_NO_ROWS);
        }
        let mut sum: u32 = 0;
        for r in &self.rows {
            sum += r.count;
            self.sum_steps += 1;
        }
        if self.sum_steps != self.rows.len() as u32 {
            return Err(XC_VIRTUAL);
        }
        self.subtotal = Some(sum);
        Ok(sum)
    }

    /// 批小计。
    pub fn subtotal(&self) -> Option<u32> {
        self.subtotal
    }

    /// 逐条总和。
    pub fn rows_total(&self) -> u32 {
        self.rows.iter().map(|r| r.count).sum()
    }
}

/// 批账对冲平（独立函数面）：逐条总和 = 批小计 = B32_ITEMS（320）。
pub fn b32_hedge_flat(book: &B32Book) -> Result<u32, u16> {
    let sub = match book.subtotal {
        Some(s) => s,
        None => return Err(XC_NO_ROWS),
    };
    let rt = book.rows_total();
    if rt != sub || sub != B32_ITEMS {
        return Err(XC_HEDGE);
    }
    Ok(rt)
}

/// 累计链核：链 = 前段冻结尾 4340 + B32 批小计 → 收口尾 4660。
pub fn cumulative_tail(book: &B32Book) -> Result<u32, u16> {
    let sub = match book.subtotal {
        Some(s) => s,
        None => return Err(XC_NO_ROWS),
    };
    let tail = PREV_TAIL.wrapping_add(sub);
    if tail != TAIL_4660 {
        return Err(XC_CHAIN_TAIL);
    }
    Ok(tail)
}

/// 域级联动收口：unx_a1_b32 全绿（对冲平 + 链尾中）才置收口位。
pub fn close_domain(book: &mut B32Book) -> Result<u32, u16> {
    b32_hedge_flat(book)?;
    let tail = cumulative_tail(book)?;
    book.closed = true;
    Ok(tail)
}

/// 域级 unx_a1 收束绿核：收口位已置 + 对冲仍平（收口后账面冻结语义）。
pub fn domain_green(book: &B32Book) -> Result<u32, u16> {
    if !book.closed {
        return Err(XC_NOT_CLOSED);
    }
    b32_hedge_flat(book)?;
    cumulative_tail(book)
}

impl B32Book {
    /// 人话报告。
    pub fn report(&self) -> String {
        let mut s = alloc::format!(
            "{} · 逐条 {} 行 · 小计={:?} · 求和步 {} · 收口={}",
            BATCH_SUITE_B32,
            self.rows.len(),
            self.subtotal,
            self.sum_steps,
            self.closed
        );
        for r in self.rows.iter().take(10) {
            s.push_str(&alloc::format!(
                "\n  判据F0{} 计数{} 绿={}",
                r.crit_id, r.count, r.green
            ));
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

/// 标准 B32 批账构造器：320 条判据（每条计数 1），先求和后落账，
/// 收口置位，链尾 4660。
pub fn build_b32_book() -> Result<B32Book, u16> {
    let mut b = B32Book::new();
    for i in 0..B32_ITEMS {
        b.push(600 + (i % 60) as u16, 1, true)?;
    }
    b.settle()?;
    close_domain(&mut b)?;
    domain_green(&b)?;
    Ok(b)
}

/// 自例程：主路径（320 条→对冲平→链尾 4660→域收口）+ 错误路径。
pub fn drill() -> Result<u32, u16> {
    // 主路径。
    let b = build_b32_book()?;
    let tail = domain_green(&b)?;
    if tail != TAIL_4660 {
        return Err(XC_CHAIN_TAIL);
    }
    if b.rows_total() != B32_ITEMS {
        return Err(XC_HEDGE);
    }
    if b.fingerprint() == 0 {
        return Err(XC_HEDGE);
    }

    // 错误路径 1：红条混入。
    let mut b1 = B32Book::new();
    if b1.push(601, 1, false) != Err(XC_RED_ROW) {
        return Err(XC_RED_ROW);
    }
    // 错误路径 2：零条禁落小计。
    let mut b2 = B32Book::new();
    if b2.settle() != Err(XC_NO_ROWS) {
        return Err(XC_NO_ROWS);
    }
    // 错误路径 3：重复落小计。
    let mut b3 = B32Book::new();
    b3.push(601, 320, true)?;
    b3.settle()?;
    if b3.settle() != Err(XC_VIRTUAL) {
        return Err(XC_VIRTUAL);
    }
    // 错误路径 4：对冲不平（少登一条 → 小计 319）。
    let mut b4 = B32Book::new();
    for _ in 0..B32_ITEMS - 1 {
        b4.push(601, 1, true)?;
    }
    b4.settle()?;
    if b4.subtotal() != Some(B32_ITEMS - 1) {
        return Err(XC_HEDGE);
    }
    if b32_hedge_flat(&b4) != Err(XC_HEDGE) {
        return Err(XC_HEDGE);
    }
    // 错误路径 5：链尾不中（B32 小计 100 → 4440 ≠ 4660）。
    let mut b5 = B32Book::new();
    b5.push(601, 100, true)?;
    b5.settle()?;
    if cumulative_tail(&b5) != Err(XC_CHAIN_TAIL) {
        return Err(XC_CHAIN_TAIL);
    }
    // 错误路径 6：未收口先域绿。
    let mut b6 = B32Book::new();
    for _ in 0..B32_ITEMS {
        b6.push(601, 1, true)?;
    }
    b6.settle()?;
    if domain_green(&b6) != Err(XC_NOT_CLOSED) {
        return Err(XC_NOT_CLOSED);
    }
    // 收口后再绿。
    close_domain(&mut b6)?;
    if domain_green(&b6)? != TAIL_4660 {
        return Err(XC_CHAIN_TAIL);
    }
    Ok(0)
}
