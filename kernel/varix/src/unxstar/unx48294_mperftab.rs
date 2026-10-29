//! unx48294_mperftab — UNX-F48294 M 段性能总账（P95 响应/内存/启动采样汇总，试产校准定）（诊断段 0xC5A0）。
//!
//! 判据原文：
//!   J1 汇总落账带校准标注；判据锚定 体验八 性能感知。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 三族指标闭集：响应延迟（P95，ms）、内存占用（峰值，KB）、
//!      启动耗时（ms），各族独立采样、独立直方图、独立分位——三族
//!      共用分位引擎但不共享样本（族间隔离）。
//!   2. 校准标注：每族落账必须带校准标注四元（族号、阈值、样本量、
//!      校准轮次），标注缺位的汇总行视为无主账（拒收）——试产校准
//!      定是每族数字的出生证。
//!   3. 分位口径：计数排序直方图，P95 = 升序第 ⌈0.95·n⌉ 位；汇总
//!      行逐族记 (P95/峰值/启动值, 阈值, 判定)，判定全过才汇总绿。
//!   4. 汇总落账口径（J1）：三族各 10^3 采样落账后，汇总行三族齐
//!      带标注、三判定全过、直方图守恒——汇总不是加总而是带标注的
//!      逐族结论。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xC5A0;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_OVER_BUDGET: u16 = SEG_BASE + 1; // 族判定超阈
const E_NO_CALIB: u16 = SEG_BASE + 2;    // 无校准标注（无主账）
const E_CALIB_FROZEN: u16 = SEG_BASE + 3;// 校准冻结后再改
const E_SAMPLE_MISS: u16 = SEG_BASE + 4; // 采样不足
const E_METRIC_BAD: u16 = SEG_BASE + 5;  // 族号越界
const E_LEDGER_BAD: u16 = SEG_BASE + 6;  // 总账不平
const E_EMPTY_SUM: u16 = SEG_BASE + 7;   // 写回摘要为空

/// 族闭集。
pub const METRIC_COUNT: usize = 3;
pub const M_RESP: usize = 0; // 响应延迟 P95
pub const M_MEM: usize = 1;  // 内存峰值
pub const M_BOOT: usize = 2; // 启动耗时

pub const METRIC_NAMES: [&str; METRIC_COUNT] = ["resp_p95", "mem_peak", "boot_ms"];

/// 直方图桶上限（族内通用域）。
pub const HIST_MAX: u32 = 200;
/// 每族采样数。
pub const SAMPLES_PER: u32 = 1000;

/// 校准标注（每族一条）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CalibMark {
    pub metric: usize,
    pub threshold: u32,
    pub samples: u32,
    pub round: u32,
}

/// 一族采样器：直方图 + 计数。
pub struct MetricCol {
    pub metric: usize,
    hist: Vec<u32>,
    pub total: u32,
    pub over_total: u32,
    pub calib: Option<CalibMark>,
}

impl MetricCol {
    pub fn new(metric: usize) -> Result<MetricCol, u16> {
        if metric >= METRIC_COUNT {
            return Err(E_METRIC_BAD);
        }
        let mut hist: Vec<u32> = Vec::new();
        let mut i = 0u32;
        while i <= HIST_MAX {
            hist.push(0);
            i += 1;
        }
        Ok(MetricCol { metric, hist, total: 0, over_total: 0, calib: None })
    }

    /// 校准标注：一次冻结（threshold ∈ 1..HIST_MAX）。
    pub fn calibrate(&mut self, threshold: u32, round: u32) -> Result<CalibMark, u16> {
        if self.calib.is_some() {
            return Err(E_CALIB_FROZEN);
        }
        if threshold == 0 || threshold > HIST_MAX {
            return Err(E_LEDGER_BAD);
        }
        let mark = CalibMark { metric: self.metric, threshold, samples: SAMPLES_PER, round };
        self.calib = Some(mark);
        Ok(mark)
    }

    /// 采一点（值 ∈ 0..=HIST_MAX，溢出进顶桶并计超阈）。
    pub fn sample(&mut self, v: u32) -> Result<(), u16> {
        if v > HIST_MAX {
            self.hist[HIST_MAX as usize] += 1;
            self.over_total += 1;
            self.total += 1;
            return Ok(());
        }
        self.hist[v as usize] += 1;
        self.total += 1;
        if let Some(c) = self.calib {
            if v > c.threshold {
                self.over_total += 1;
            }
        }
        Ok(())
    }

    /// 分位：升序第 ⌈q_num/q_den·n⌉ 位。
    pub fn percentile(&self, q_num: u32, q_den: u32) -> Result<u32, u16> {
        if self.total == 0 || q_den == 0 {
            return Err(E_LEDGER_BAD);
        }
        let n = self.total as u64;
        let mut rank = (q_num as u64 * n + q_den as u64 - 1) / q_den as u64;
        if rank == 0 {
            rank = 1;
        }
        let mut acc: u64 = 0;
        let mut i = 0u32;
        while i <= HIST_MAX {
            acc += self.hist[i as usize] as u64;
            if acc >= rank {
                return Ok(i);
            }
            i += 1;
        }
        Err(E_LEDGER_BAD)
    }

    /// 直方图守恒。
    pub fn audit_hist(&self) -> Result<(), u16> {
        let mut sum: u64 = 0;
        let mut i = 0u32;
        while i <= HIST_MAX {
            sum += self.hist[i as usize] as u64;
            i += 1;
        }
        if sum != self.total as u64 {
            return Err(E_LEDGER_BAD);
        }
        Ok(())
    }

    /// 族判定：P95 ≤ 阈值且样本量足。
    pub fn judge(&self) -> Result<u32, u16> {
        let c = match self.calib {
            Some(c) => c,
            None => return Err(E_NO_CALIB), // 无标注不得判定
        };
        if self.total < c.samples {
            return Err(E_SAMPLE_MISS);
        }
        let p = self.percentile(95, 100)?;
        if p > c.threshold {
            return Err(E_OVER_BUDGET);
        }
        Ok(p)
    }
}

/// 一行汇总账：族号 + 分位值 + 阈值 + 校准标注 + 判定。
#[derive(Clone, Copy, Debug)]
pub struct SummaryRow {
    pub metric: usize,
    pub p95: u32,
    pub threshold: u32,
    pub calib_round: u32, // 校准标注（>0 即有出生证）
    pub pass: bool,
}

/// M 段性能总账枢纽。
pub struct MPerfTab {
    cols: [MetricCol; METRIC_COUNT], // 无法直接放：MetricCol 无 Copy
    rows: Vec<SummaryRow>,
    pub row_total: u32,
    pub fail_total: u32,
}

impl MPerfTab {
    pub fn new() -> Result<MPerfTab, u16> {
        Ok(MPerfTab {
            cols: [
                MetricCol::new(M_RESP)?,
                MetricCol::new(M_MEM)?,
                MetricCol::new(M_BOOT)?,
            ],
            rows: Vec::new(),
            row_total: 0,
            fail_total: 0,
        })
    }

    pub fn col_mut(&mut self, metric: usize) -> Result<&mut MetricCol, u16> {
        if metric >= METRIC_COUNT {
            return Err(E_METRIC_BAD);
        }
        Ok(&mut self.cols[metric])
    }

    /// 汇总落账：逐族判定（无标注族拒收），带标注行入账（J1 主体）。
    pub fn summarize(&mut self) -> Result<u32, u16> {
        let mut passed = 0u32;
        let mut m = 0usize;
        while m < METRIC_COUNT {
            let col = &self.cols[m];
            let p = match col.judge() {
                Ok(p) => p,
                Err(e) => {
                    self.fail_total += 1;
                    return Err(e);
                }
            };
            let c = match col.calib {
                Some(c) => c,
                None => return Err(E_NO_CALIB),
            };
            let pass = p <= c.threshold;
            self.rows.push(SummaryRow {
                metric: m,
                p95: p,
                threshold: c.threshold,
                calib_round: c.round,
                pass,
            });
            self.row_total += 1;
            if pass {
                passed += 1;
            } else {
                self.fail_total += 1;
            }
            m += 1;
        }
        Ok(passed)
    }

    /// ×N 行覆盖核验（J1 硬口径）。
    pub fn coverage(&self, want: u32) -> Result<(), u16> {
        if self.row_total < want {
            return Err(E_SAMPLE_MISS);
        }
        Ok(())
    }

    /// 总账审计：三行齐带标注（calib_round>0）、判定全过、直方图守恒。
    pub fn audit(&self) -> Result<(), u16> {
        if self.rows.len() as u32 != self.row_total {
            return Err(E_LEDGER_BAD);
        }
        if self.fail_total != 0 {
            return Err(E_OVER_BUDGET);
        }
        if self.rows.len() != METRIC_COUNT {
            return Err(E_SAMPLE_MISS);
        }
        let mut i = 0usize;
        while i < self.rows.len() {
            let r = &self.rows[i];
            if r.calib_round == 0 {
                return Err(E_NO_CALIB); // 无标注 = 无主账
            }
            if !r.pass || r.p95 > r.threshold {
                return Err(E_OVER_BUDGET);
            }
            i += 1;
        }
        let mut m = 0usize;
        while m < METRIC_COUNT {
            self.cols[m].audit_hist()?;
            m += 1;
        }
        Ok(())
    }

    pub fn rows_len(&self) -> usize {
        self.rows.len()
    }

    /// 完成写回（判据 J2）。
    pub fn writeback(&self, done: bool, summary_len: u32) -> Result<(), u16> {
        if !done || self.row_total != METRIC_COUNT as u32 {
            return Err(E_LEDGER_BAD);
        }
        if summary_len == 0 {
            return Err(E_EMPTY_SUM);
        }
        Ok(())
    }
}

/// 确定性样本生成：族 m 第 i 个样本值（同参必同果）。
pub fn sample_value(m: usize, i: u32) -> u32 {
    match m {
        // 响应延迟：0..50（P95=50 之内）
        M_RESP => (i.wrapping_mul(37).wrapping_add(11)) % 51,
        // 内存峰值：0..90
        M_MEM => (i.wrapping_mul(53).wrapping_add(29)) % 91,
        // 启动耗时：0..120 中低段（P95 落 80 内）
        _ => (i.wrapping_mul(29).wrapping_add(7)) % 81,
    }
}

/// 自例程判据：逐条落实 J1（汇总落账带校准标注），返回命中数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1 主汇总：三族校准（阈 50/120/150）→ 各 10^3 采样 → 汇总落账
    //    三行齐带标注、三判定全过、直方图守恒。
    let mut tab = MPerfTab::new()?;
    tab.col_mut(M_RESP)?.calibrate(50, 1)?;
    tab.col_mut(M_MEM)?.calibrate(120, 1)?;
    tab.col_mut(M_BOOT)?.calibrate(150, 1)?;
    let mut m = 0usize;
    while m < METRIC_COUNT {
        let mut i = 0u32;
        while i < SAMPLES_PER {
            let v = sample_value(m, i);
            tab.col_mut(m)?.sample(v)?;
            i += 1;
        }
        m += 1;
    }
    let passed = tab.summarize()?;
    if passed != 3 {
        return Err(E_OVER_BUDGET);
    }
    tab.coverage(3)?;
    tab.audit()?;
    if tab.rows_len() != 3 {
        return Err(E_LEDGER_BAD);
    }
    // 校准标注逐行在位（出生证口径）。
    let mut i = 0usize;
    while i < 3 {
        if tab.rows[i].calib_round != 1 || tab.rows[i].threshold == 0 {
            return Err(E_NO_CALIB);
        }
        i += 1;
    }
    hits += 1;

    // R2 分位可复算与分布健全：三族 P95 两次查询同值，P50 ≤ P95。
    let mut m = 0usize;
    while m < METRIC_COUNT {
        let a = tab.cols[m].percentile(95, 100)?;
        let b = tab.cols[m].percentile(95, 100)?;
        if a != b {
            return Err(E_LEDGER_BAD);
        }
        let p50 = tab.cols[m].percentile(50, 100)?;
        if p50 > a {
            return Err(E_LEDGER_BAD);
        }
        m += 1;
    }
    hits += 1;

    // R3 无标注拒判：新建一族不校准直接 judge 必拒（无主账红线）。
    let mut col = MetricCol::new(M_RESP)?;
    col.sample(10)?;
    if col.judge().is_ok() {
        return Err(E_NO_CALIB);
    }
    hits += 1;

    // R4 校准冻结：二次校准拒；校准四元逐位不变。
    let mut tab = MPerfTab::new()?;
    let mark = tab.col_mut(M_RESP)?.calibrate(60, 3)?;
    if mark.threshold != 60 || mark.samples != SAMPLES_PER || mark.round != 3 {
        return Err(E_LEDGER_BAD);
    }
    if tab.col_mut(M_RESP)?.calibrate(70, 4).is_ok() {
        return Err(E_CALIB_FROZEN);
    }
    hits += 1;

    // R5 超阈判红：一族校准阈 10、注入 20 个 30ms 样本，judge 必红
    // （判定非恒绿）；超阈计数如实 20。
    let mut tab = MPerfTab::new()?;
    tab.col_mut(M_RESP)?.calibrate(10, 1)?;
    let mut i = 0u32;
    while i < 20 {
        tab.col_mut(M_RESP)?.sample(30)?;
        i += 1;
    }
    if tab.summarize().is_ok() {
        return Err(E_OVER_BUDGET);
    }
    if tab.cols[M_RESP].over_total != 20 {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R6 溢出桶：v > HIST_MAX 进顶桶且守恒不破；分位查询仍闭合。
    let mut col = MetricCol::new(M_BOOT)?;
    col.calibrate(150, 1)?;
    col.sample(500)?; // 溢出
    col.sample(10)?;
    col.audit_hist()?;
    if col.over_total != 1 {
        return Err(E_LEDGER_BAD);
    }
    col.percentile(95, 100)?;
    hits += 1;

    // R7 族号越界拒收：MetricCol::new(3)、col_mut(3) 全拒。
    if MetricCol::new(3).is_ok() {
        return Err(E_METRIC_BAD);
    }
    let mut tab = MPerfTab::new()?;
    if tab.col_mut(3).is_ok() {
        return Err(E_METRIC_BAD);
    }
    hits += 1;

    // R8 缺采样拒汇总与写回（判据 J2）：校准过但零采样 judge 拒；
    //    主链补齐后写回放行。
    let mut tab = MPerfTab::new()?;
    tab.col_mut(M_RESP)?.calibrate(50, 1)?;
    tab.col_mut(M_MEM)?.calibrate(120, 1)?;
    tab.col_mut(M_BOOT)?.calibrate(150, 1)?;
    if tab.summarize().is_ok() {
        return Err(E_SAMPLE_MISS); // 零采样不得出汇总
    }
    let mut m = 0usize;
    while m < METRIC_COUNT {
        let mut i = 0u32;
        while i < SAMPLES_PER {
            let v = sample_value(m, i);
            tab.col_mut(m)?.sample(v)?;
            i += 1;
        }
        m += 1;
    }
    tab.summarize()?;
    tab.audit()?;
    if tab.writeback(true, 0).is_ok() {
        return Err(E_EMPTY_SUM);
    }
    tab.writeback(true, 0x40)?;
    hits += 1;

    Ok(hits)
}
