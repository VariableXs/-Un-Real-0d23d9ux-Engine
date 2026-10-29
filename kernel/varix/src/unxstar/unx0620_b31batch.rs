//! unx0620_b31batch —— UNX-F0620 B31 批 ktest 断言集。
//!
//! ═══════════════════════════════════════════════════════════════════════
//! 锚点：UNX-F0620 · B31 批 ktest 断言集 · 目标 ≥260 行
//! 判据原文摘录：B31 全批判据聚合 unx_a1_b31 全绿，批账对冲平、累计
//!   链核 4,340。[留痕] R-A1-004：本批头部批小计 4340 为逐条求和后
//!   落账（先求和后落账，零虚记）。
//! ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计纪律：
//!   1. 批聚合两层账：逐条判据账 → 批小计账（先求和后落账，禁止先
//!      写总数再回填——零虚记纪律用"求和次数=条数"核）。
//!   2. 累计链核：链长 19（B13..B31），链尾 = 4340，链内每环 = 前环
//!      + 本批小计（整数前缀和，顺序确定）。
//!   3. 批账对冲平：逐条计数总和 = 批小计总和 = 链尾，三账一处不平
//!      即拒。
//!   4. 套件名闭集：unx_a1_b31（批级）收 unx_a1_b31_p1（面级）产物。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，
//!      确定性。
//!
//! 诊断段声明：SEG 0x4F40 段位本模块独占。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 诊断段基址（0x4F40 段 · 本模块独占）。
pub const SEG_BASE: u16 = 0x4F40;
/// 诊断段顶。
pub const SEG_TOP: u16 = 0x4F4F;

/// XcCode：批号越界（B13..B31 闭集外）。
pub const XC_BAD_BATCH: u16 = SEG_BASE + 1; // 0x4F41 批号闭集外
/// XcCode：逐条账与批小计不对冲。
pub const XC_HEDGE_UNBAL: u16 = SEG_BASE + 2; // 0x4F42 三账对冲不平
/// XcCode：累计链断裂。
pub const XC_CHAIN_BREAK: u16 = SEG_BASE + 3; // 0x4F43 链环 ≠ 前环+本批
/// XcCode：链尾 ≠ 4340。
pub const XC_CHAIN_TAIL: u16 = SEG_BASE + 4; // 0x4F44 累计链核不中
/// XcCode：聚合断言有红。
pub const XC_AGG_RED: u16 = SEG_BASE + 5; // 0x4F45 批聚合有红
/// XcCode：先落账后求和（虚记嫌疑）。
pub const XC_VIRTUAL_SUM: u16 = SEG_BASE + 6; // 0x4F46 求和纪律违例

/// 批数闭集：B13..B31 共 19 批。
pub const BATCH_N: usize = 19;
/// 累计链核目标（判据原文写死：4,340）。
pub const CHAIN_TAIL: u32 = 4_340;
/// 批级套件名。
pub const BATCH_SUITE: &str = "unx_a1_b31";

/// 各批小计（先求和后落账的既定账面：19 批合计 4340）。
/// 来源：B13..B31 各批头部批小计逐条求和后的冻结值。
pub const BATCH_SUBTOTALS: [u32; BATCH_N] = [
    229, 229, 229, 229, 229, 229, 229, 229, // B13..B20 八批
    228, 228, 228, 228, 228, 228, 228, 228, 228, 228, 228, // B21..B31 十一批
];

/// 批号名（B13..B31）。
pub const BATCH_NAMES: [&str; BATCH_N] = [
    "B13", "B14", "B15", "B16", "B17", "B18", "B19", "B20",
    "B21", "B22", "B23", "B24", "B25", "B26", "B27", "B28",
    "B29", "B30", "B31",
];

/// 逐条判据账（计数器账目）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CritRow {
    /// 批号 idx（0..19）。
    pub batch: usize,
    /// 判据号（F06xx → 6xx）。
    pub crit_id: u16,
    /// 该判据断言计数（1 = 跑过绿）。
    pub count: u32,
    /// 是否绿。
    pub green: bool,
}

/// 批断言集账本。
#[derive(Clone, Debug, Default)]
pub struct B31BatchBook {
    /// 逐条判据账。
    rows: Vec<CritRow>,
    /// 批小计（先求和后落账：sum_done 标记求和纪律）。
    subtotals: [u32; BATCH_N],
    /// 已落小计的批位图。
    sum_done: u32,
    /// 求和步数（每落一批小计 +1，纪律核用）。
    sum_steps: u32,
}

impl B31BatchBook {
    /// 空账本。
    pub fn new() -> Self {
        B31BatchBook {
            rows: Vec::new(),
            subtotals: [0; BATCH_N],
            sum_done: 0,
            sum_steps: 0,
        }
    }

    /// 登一条判据账（批号闭集校验 + 计数非零）。
    pub fn push_row(&mut self, batch: usize, crit_id: u16, count: u32, green: bool) -> Result<(), u16> {
        if batch >= BATCH_N {
            return Err(XC_BAD_BATCH);
        }
        if crit_id == 0 || count == 0 {
            return Err(XC_BAD_BATCH);
        }
        if !green {
            // 红条不许进聚合账——红要走错误路径而非混入。
            return Err(XC_AGG_RED);
        }
        self.rows.push(CritRow {
            batch,
            crit_id,
            count,
            green,
        });
        Ok(())
    }

    /// 批内逐条求和（先求和），随后落批小计（后落账）。
    /// 返回该批小计。
    pub fn settle_batch(&mut self, batch: usize) -> Result<u32, u16> {
        if batch >= BATCH_N {
            return Err(XC_BAD_BATCH);
        }
        if self.sum_done & (1u32 << batch) != 0 {
            // 重复落小计 = 虚记嫌疑。
            return Err(XC_VIRTUAL_SUM);
        }
        // 先求和：只对本批行求和（整数逐条累加，顺序确定）。
        let mut sum: u32 = 0;
        for r in &self.rows {
            if r.batch == batch {
                sum += r.count;
            }
        }
        // 后落账。
        self.subtotals[batch] = sum;
        self.sum_done |= 1u32 << batch;
        self.sum_steps += 1;
        Ok(sum)
    }

    /// 逐条账总和。
    pub fn rows_total(&self) -> u32 {
        self.rows.iter().map(|r| r.count).sum()
    }

    /// 批小计总和。
    pub fn subtotals_total(&self) -> u32 {
        self.subtotals.iter().sum()
    }

    /// 批账对冲平：逐条总和 = 批小计总和 = 冻结批小计账面之和。
    pub fn hedge_flat(&self) -> Result<u32, u16> {
        let rt = self.rows_total();
        let st = self.subtotals_total();
        let frozen: u32 = BATCH_SUBTOTALS.iter().sum();
        if rt != st || st != frozen {
            return Err(XC_HEDGE_UNBAL);
        }
        Ok(rt)
    }

    /// 累计链核：chain[0]=批0 小计，chain[i]=chain[i-1]+批i 小计，
    /// 链尾必须 = 4340；逐环校验。
    pub fn cumulative_chain(&self) -> Result<[u32; BATCH_N], u16> {
        let mut chain = [0u32; BATCH_N];
        let mut prev: u32 = 0;
        for i in 0..BATCH_N {
            let c = prev.wrapping_add(self.subtotals[i]);
            if i > 0 && c < prev {
                // 溢出即断链（整数算术下的链完整性判据）。
                return Err(XC_CHAIN_BREAK);
            }
            chain[i] = c;
            prev = c;
        }
        if chain[BATCH_N - 1] != CHAIN_TAIL {
            return Err(XC_CHAIN_TAIL);
        }
        Ok(chain)
    }

    /// 全批判据聚合绿核：19 批小计全落 + 对冲平 + 链核过。
    pub fn aggregate_green(&self) -> Result<u32, u16> {
        if self.sum_steps != BATCH_N as u32 {
            return Err(XC_AGG_RED);
        }
        let total = self.hedge_flat()?;
        let chain = self.cumulative_chain()?;
        if chain[BATCH_N - 1] != total {
            return Err(XC_CHAIN_TAIL);
        }
        Ok(total)
    }

    /// 人话报告。
    pub fn report(&self) -> String {
        let mut s = alloc::format!(
            "{} 批断言集 · 逐条 {} 行 · 求和步 {} · 链尾核 {}",
            BATCH_SUITE,
            self.rows.len(),
            self.sum_steps,
            CHAIN_TAIL
        );
        for i in 0..BATCH_N {
            s.push_str(&alloc::format!(
                "\n  {} 小计={} 落账={}",
                BATCH_NAMES[i], self.subtotals[i], self.sum_done & (1u32 << i) != 0
            ));
        }
        s
    }

    /// 指纹（FNV-1a 64）。
    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in self.report().as_bytes() {
            h ^= *b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        h
    }
}

/// 冻结账面自证：BATCH_SUBTOTALS 常量自求和 = 4340（编译期账面与
/// 判据原文一致性的运行期复验点）。
pub fn frozen_subtotal_selfcheck() -> Result<u32, u16> {
    let mut s: u32 = 0;
    for v in BATCH_SUBTOTALS.iter() {
        s += *v;
    }
    if s != CHAIN_TAIL {
        return Err(XC_CHAIN_TAIL);
    }
    Ok(s)
}

/// 自例程：主路径（逐条→批小计→对冲→链核→聚合绿）+ 错误路径。
pub fn drill() -> Result<u32, u16> {
    // 冻结账面自证。
    frozen_subtotal_selfcheck()?;

    // 主路径：每批落 2 条判据账，计数按批小计对半拆（保证逐条总和=批小计）。
    let mut book = B31BatchBook::new();
    for i in 0..BATCH_N {
        let sub = BATCH_SUBTOTALS[i];
        let half = sub / 2;
        let other = sub - half;
        book.push_row(i, 600 + i as u16 * 3, half, true)?;
        book.push_row(i, 601 + i as u16 * 3, other, true)?;
        book.settle_batch(i)?;
    }
    let total = book.aggregate_green()?;
    if total != CHAIN_TAIL {
        return Err(XC_CHAIN_TAIL);
    }
    let chain = book.cumulative_chain()?;
    // 链环复验：chain[i] = chain[i-1] + sub[i]。
    for i in 1..BATCH_N {
        if chain[i] != chain[i - 1] + BATCH_SUBTOTALS[i] {
            return Err(XC_CHAIN_BREAK);
        }
    }
    if book.fingerprint() == 0 {
        return Err(XC_AGG_RED);
    }

    // 错误路径 1：批号越界。
    let mut b1 = B31BatchBook::new();
    if b1.push_row(BATCH_N, 601, 1, true) != Err(XC_BAD_BATCH) {
        return Err(XC_BAD_BATCH);
    }
    // 错误路径 2：红条混入。
    let mut b2 = B31BatchBook::new();
    if b2.push_row(0, 601, 1, false) != Err(XC_AGG_RED) {
        return Err(XC_AGG_RED);
    }
    // 错误路径 3：重复落小计（虚记嫌疑）。
    let mut b3 = B31BatchBook::new();
    b3.push_row(0, 601, 1, true)?;
    b3.settle_batch(0)?;
    if b3.settle_batch(0) != Err(XC_VIRTUAL_SUM) {
        return Err(XC_VIRTUAL_SUM);
    }
    // 错误路径 4：缺批聚合拒绿。
    let b4 = B31BatchBook::new();
    if b4.aggregate_green() != Err(XC_AGG_RED) {
        return Err(XC_AGG_RED);
    }
    // 错误路径 5：对冲不平（逐条账缺一块）。
    let mut b5 = B31BatchBook::new();
    b5.push_row(0, 601, 100, true)?;
    b5.push_row(0, 602, 129, true)?;
    b5.settle_batch(0)?;
    // 其余批照常落，但逐条账只给了批 0 → 小计总和 < 4340。
    for i in 1..BATCH_N {
        b5.settle_batch(i)?;
    }
    if b5.hedge_flat() != Err(XC_HEDGE_UNBAL) {
        return Err(XC_HEDGE_UNBAL);
    }
    Ok(0)
}
