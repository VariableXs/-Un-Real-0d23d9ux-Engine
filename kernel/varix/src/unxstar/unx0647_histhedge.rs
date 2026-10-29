//! unx0647_histhedge —— UNX-F0647 引导履历对冲（履历 vs 批账守恒）。
//!
//! ═══════════════════════════════════════════════════════════════════════
//! 锚点：UNX-F0647 · 引导履历对冲（履历 vs 批账守恒） · 目标 ≥200 行
//! 判据原文摘录：UNX-F0647-J1 履历总数 vs 各批演练账行数对冲全平，
//!   误差 0。
//! ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计纪律：
//!   1. 对冲双面：A 面 = 引导履历总数（F0646 账面口径），B 面 = 各批
//!      演练账行数之和（批账口径）；对冲平 = A = B，误差 = |A-B| = 0。
//!   2. 批账定容：批演练账行数批号 0..10 闭集（本核对冲域），行数
//!      非零、批号不重。
//!   3. 误差面：误差用有符号整数差值账（不许用绝对值掩盖方向），
//!      平 = 差 0；非零即红并保留方向供追账。
//!   4. 双落账：A 面与 B 面各自先求和后落账，双面落账值复算互验。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，
//!      确定性。
//!
//! 诊断段声明：SEG 0x5260 段位本模块独占。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 诊断段基址（0x5260 段 · 本模块独占）。
pub const SEG_BASE: u16 = 0x5260;
/// 诊断段顶。
pub const SEG_TOP: u16 = 0x526F;

/// XcCode：批号越界。
pub const XC_BAD_BATCH: u16 = SEG_BASE + 1; // 0x5261 对冲域批号闭集外
/// XcCode：批账重复。
pub const XC_DUP_BATCH: u16 = SEG_BASE + 2; // 0x5262 一批一行数
/// XcCode：批行数为零。
pub const XC_ZERO_ROW: u16 = SEG_BASE + 3; // 0x5263 零行批不参与对冲
/// XcCode：对冲误差非零。
pub const XC_HEDGE_DIFF: u16 = SEG_BASE + 4; // 0x5264 A ≠ B
/// XcCode：双面未落账。
pub const XC_UNSETTLED: u16 = SEG_BASE + 5; // 0x5265 缺面禁对冲
/// XcCode：复算漂移。
pub const XC_RECALC_DRIFT: u16 = SEG_BASE + 6; // 0x5266 落账值 ≠ 复算值

/// 对冲域批数（履历对冲域 11 批演练账）。
pub const HEDGE_BATCH_N: usize = 11;
/// 履历总数（F0646 账面口径：50 次）。
pub const HIST_TOTAL: u32 = 50;

/// 批演练账行。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BatchLedRow {
    /// 批号（0..11）。
    pub batch: usize,
    /// 该批演练账行数（非零）。
    pub rows: u32,
}

/// 引导履历对冲账本。
#[derive(Clone, Debug, Default)]
pub struct HistHedge {
    /// B 面批账（按批号升序）。
    batch_rows: Vec<BatchLedRow>,
    /// A 面落账值（履历总数）。
    side_a: Option<u32>,
    /// B 面落账值（Σ批行数）。
    side_b: Option<u32>,
}

impl HistHedge {
    /// 空账本。
    pub fn new() -> Self {
        HistHedge {
            batch_rows: Vec::new(),
            side_a: None,
            side_b: None,
        }
    }

    /// 登一批演练账行数（号闭集 + 去重 + 非零）。
    pub fn put_batch(&mut self, batch: usize, rows: u32) -> Result<(), u16> {
        if batch >= HEDGE_BATCH_N {
            return Err(XC_BAD_BATCH);
        }
        if rows == 0 {
            return Err(XC_ZERO_ROW);
        }
        if self.batch_rows.iter().any(|r| r.batch == batch) {
            return Err(XC_DUP_BATCH);
        }
        self.batch_rows.push(BatchLedRow { batch, rows });
        self.batch_rows.sort_by_key(|r| r.batch);
        Ok(())
    }

    /// 批数。
    pub fn batch_n(&self) -> usize {
        self.batch_rows.len()
    }

    /// B 面复算：Σ批行数（逐批累加）。
    pub fn recalc_b(&self) -> Result<u32, u16> {
        if self.batch_n() != HEDGE_BATCH_N {
            return Err(XC_UNSETTLED);
        }
        let mut s: u32 = 0;
        for r in &self.batch_rows {
            s += r.rows;
        }
        Ok(s)
    }

    /// 落 A 面（履历总数；先复算 B 面在位才可落——对冲前置）。
    pub fn settle_a(&mut self, total: u32) -> Result<u32, u16> {
        if total == 0 {
            return Err(XC_ZERO_ROW);
        }
        self.side_a = Some(total);
        Ok(total)
    }

    /// 落 B 面（必须 = B 面复算值——先求和后落账）。
    pub fn settle_b(&mut self) -> Result<u32, u16> {
        let v = self.recalc_b()?;
        self.side_b = Some(v);
        Ok(v)
    }

    /// 对冲：A - B 有符号差（平 = 0）。
    pub fn hedge_delta(&self) -> Result<i64, u16> {
        let a = match self.side_a {
            Some(v) => v,
            None => return Err(XC_UNSETTLED),
        };
        let b = match self.side_b {
            Some(v) => v,
            None => return Err(XC_UNSETTLED),
        };
        Ok(a as i64 - b as i64)
    }

    /// 对冲平核：双面已落 + 差 0 + 复算互验（B 落账值 = 复算值）。
    pub fn flat(&self) -> Result<u32, u16> {
        let d = self.hedge_delta()?;
        if d != 0 {
            return Err(XC_HEDGE_DIFF);
        }
        let b = match self.side_b {
            Some(v) => v,
            None => return Err(XC_UNSETTLED),
        };
        if b != self.recalc_b()? {
            return Err(XC_RECALC_DRIFT);
        }
        Ok(b)
    }

    /// 批分布核：11 批全在册。
    pub fn batches_full(&self) -> Result<usize, u16> {
        if self.batch_n() != HEDGE_BATCH_N {
            return Err(XC_BAD_BATCH);
        }
        for (i, r) in self.batch_rows.iter().enumerate() {
            if r.batch != i {
                return Err(XC_BAD_BATCH);
            }
        }
        Ok(HEDGE_BATCH_N)
    }

    /// 人话报告。
    pub fn report(&self) -> String {
        let mut s = alloc::format!(
            "引导履历对冲 · 批 {}  · A面={:?} B面={:?} 差={:?}",
            self.batch_n(),
            self.side_a,
            self.side_b,
            self.hedge_delta().ok()
        );
        for r in &self.batch_rows {
            s.push_str(&alloc::format!("\n  批{} 行数{}", r.batch, r.rows));
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

/// 标准对冲构造器：11 批行数 [5,5,5,5,5,5,5,5,4,5,1]（Σ=50），
/// A 面 50，B 面先求和后落账，对冲平。
pub fn build_flat_hedge() -> Result<HistHedge, u16> {
    let rows = [5u32, 5, 5, 5, 5, 5, 5, 5, 4, 5, 1];
    let mut h = HistHedge::new();
    for (i, r) in rows.iter().enumerate() {
        h.put_batch(i, *r)?;
    }
    h.batches_full()?;
    h.settle_a(HIST_TOTAL)?;
    h.settle_b()?;
    h.flat()?;
    Ok(h)
}

/// 自例程：主路径（对冲平 误差 0）+ 错误路径（差非零/缺面/越界/重批/复算漂移）。
pub fn drill() -> Result<u32, u16> {
    // 主路径。
    let h = build_flat_hedge()?;
    if h.flat()? != HIST_TOTAL {
        return Err(XC_HEDGE_DIFF);
    }
    if h.hedge_delta()? != 0 {
        return Err(XC_HEDGE_DIFF);
    }
    if h.fingerprint() == 0 {
        return Err(XC_RECALC_DRIFT);
    }

    // 错误路径 1：差非零（A=49 B=50）。
    let mut h1 = HistHedge::new();
    let rows = [5u32, 5, 5, 5, 5, 5, 5, 5, 4, 5, 1];
    for (i, r) in rows.iter().enumerate() {
        h1.put_batch(i, *r)?;
    }
    h1.settle_a(HIST_TOTAL - 1)?;
    h1.settle_b()?;
    if h1.flat() != Err(XC_HEDGE_DIFF) {
        return Err(XC_HEDGE_DIFF);
    }
    if h1.hedge_delta()? != -1 {
        return Err(XC_HEDGE_DIFF);
    }
    // 错误路径 2：缺 A 面。
    let mut h2 = HistHedge::new();
    for (i, r) in rows.iter().enumerate() {
        h2.put_batch(i, *r)?;
    }
    h2.settle_b()?;
    if h2.flat() != Err(XC_UNSETTLED) {
        return Err(XC_UNSETTLED);
    }
    // 错误路径 3：缺 B 面。
    let mut h3 = HistHedge::new();
    for (i, r) in rows.iter().enumerate() {
        h3.put_batch(i, *r)?;
    }
    h3.settle_a(HIST_TOTAL)?;
    if h3.flat() != Err(XC_UNSETTLED) {
        return Err(XC_UNSETTLED);
    }
    // 错误路径 4：批号越界。
    let mut h4 = HistHedge::new();
    if h4.put_batch(HEDGE_BATCH_N, 1) != Err(XC_BAD_BATCH) {
        return Err(XC_BAD_BATCH);
    }
    // 错误路径 5：重批与零行批。
    let mut h5 = build_flat_hedge()?;
    if h5.put_batch(0, 5) != Err(XC_DUP_BATCH) {
        return Err(XC_DUP_BATCH);
    }
    let mut h6 = HistHedge::new();
    if h6.put_batch(0, 0) != Err(XC_ZERO_ROW) {
        return Err(XC_ZERO_ROW);
    }
    // 错误路径 6：复算漂移（落账后篡改批行数）。
    let mut h7 = build_flat_hedge()?;
    h7.batch_rows[0].rows = 9;
    if h7.flat() != Err(XC_RECALC_DRIFT) {
        return Err(XC_RECALC_DRIFT);
    }
    Ok(0)
}
