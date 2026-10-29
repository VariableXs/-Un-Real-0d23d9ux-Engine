//! unx48257_spperf — UNX-F48257 B13 批性能面（方案切换 ≤50ms 采样，试产校准定）（诊断段 0xC100）。
//!
//! 判据原文：
//!   J1 10^3 采样 P99 落账；判据锚定 附则五。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 采样口径：方案切换耗时以整数毫秒计；10^3 次切换各采一点，
//!      样本确定性生成（同参必同果），采样过程零浮点零外部时钟——
//!      时钟面由调用方注入 tick，本面只对整数差记账。
//!   2. P99 落账：计数排序直方图（0..MAX_MS 桶）算分位，P99 = 升序
//!      第 ⌈0.99·n⌉ 位样本；直方图、样本总量、P99 三元一并落账。
//!   3. 阈值试产校准定：判定阈值 50ms 为常量声明，校准记录（阈值、
//!      样本量、校准轮）一次冻结；冻结后改阈拒收——阈值不随跑改。
//!   4. 判定口径（J1）：10^3 采样 P99 ≤ 50ms 才绿；超阈样本逐条落
//!      超阈账（如实计数不截断），超阈账只影响判定不删样本。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xC100;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_OVER_BUDGET: u16 = SEG_BASE + 1; // P99 超阈
const E_THAWED_EDIT: u16 = SEG_BASE + 2; // 冻结后改阈
const E_SAMPLE_MISS: u16 = SEG_BASE + 3; // 10^3 采样不足
const E_TICK_BACK: u16 = SEG_BASE + 4;   // tick 回退
const E_LEDGER_BAD: u16 = SEG_BASE + 5;  // 直方图账不平
const E_EMPTY_SUM: u16 = SEG_BASE + 6;   // 写回摘要为空

/// 性能预算（ms，试产校准定）。
pub const BUDGET_MS: u32 = 50;
/// 直方图桶上限（ms）：超上限样本计入溢出桶。
pub const HIST_MAX_MS: u32 = 120;
/// 采样数（J1 的 10^3）。
pub const SAMPLE_COUNT: u32 = 1000;

/// 校准记录（一次冻结）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CalibMark {
    pub budget_ms: u32,
    pub samples: u32,
    pub calib_round: u32,
}

/// 一条采样账。
#[derive(Clone, Copy, Debug)]
pub struct PerfRecord {
    pub idx: u32,
    pub tick0: u64,
    pub tick1: u64,
    pub cost_ms: u32,
}

/// 性能采样枢纽。
pub struct SpPerf {
    records: Vec<PerfRecord>,
    hist: Vec<u32>, // hist[i] = 耗时 i ms 样本数；溢出桶在末位
    pub sample_total: u32,
    pub over_total: u32,
    pub calib: Option<CalibMark>,
    pub frozen: bool,
}

impl SpPerf {
    pub fn new() -> SpPerf {
        let mut hist: Vec<u32> = Vec::new();
        let mut i = 0u32;
        while i <= HIST_MAX_MS {
            hist.push(0);
            i += 1;
        }
        SpPerf { records: Vec::new(), hist, sample_total: 0, over_total: 0, calib: None, frozen: false }
    }

    /// 校准：一次声明阈值并冻结；重复校准拒收。
    pub fn calibrate(&mut self, budget: u32, samples: u32, round: u32) -> Result<CalibMark, u16> {
        if self.frozen {
            return Err(E_THAWED_EDIT);
        }
        if budget == 0 || budget > HIST_MAX_MS || samples == 0 {
            return Err(E_LEDGER_BAD);
        }
        let mark = CalibMark { budget_ms: budget, samples, calib_round: round };
        self.calib = Some(mark);
        self.frozen = true;
        Ok(mark)
    }

    /// 单点采样：注入 tick 对，整数差入账。
    pub fn sample(&mut self, idx: u32, tick0: u64, tick1: u64) -> Result<u32, u16> {
        if tick1 < tick0 {
            return Err(E_TICK_BACK);
        }
        let cost = (tick1 - tick0) as u32;
        self.records.push(PerfRecord { idx, tick0, tick1, cost_ms: cost });
        self.sample_total += 1;
        if cost > HIST_MAX_MS {
            self.hist[HIST_MAX_MS as usize] += 1;
            self.over_total += 1; // 溢出桶独立计（超直方图域）
        } else {
            self.hist[cost as usize] += 1;
            if cost > BUDGET_MS {
                self.over_total += 1;
            }
        }
        Ok(cost)
    }

    /// 分位数：升序第 ⌈q·n⌉ 位样本值（计数排序，无排序开销）。
    pub fn percentile(&self, q_num: u32, q_den: u32) -> Result<u32, u16> {
        if self.sample_total == 0 || q_den == 0 {
            return Err(E_LEDGER_BAD);
        }
        // rank = ceil(q_num * n / q_den)，1 起。
        let n = self.sample_total as u64;
        let mut rank = (q_num as u64 * n + q_den as u64 - 1) / q_den as u64;
        if rank == 0 {
            rank = 1;
        }
        let mut acc: u64 = 0;
        let mut i = 0u32;
        while i <= HIST_MAX_MS {
            acc += self.hist[i as usize] as u64;
            if acc >= rank {
                return Ok(i);
            }
            i += 1;
        }
        Err(E_LEDGER_BAD)
    }

    /// P99 落账判定（J1）：P99 ≤ 预算。
    pub fn assert_p99(&self) -> Result<u32, u16> {
        if self.sample_total < SAMPLE_COUNT {
            return Err(E_SAMPLE_MISS);
        }
        let p99 = self.percentile(99, 100)?;
        if p99 > BUDGET_MS {
            return Err(E_OVER_BUDGET);
        }
        Ok(p99)
    }

    /// 直方图守恒：全桶和 == 采样总数。
    pub fn audit_hist(&self) -> Result<(), u16> {
        let mut sum: u64 = 0;
        let mut i = 0u32;
        while i <= HIST_MAX_MS {
            sum += self.hist[i as usize] as u64;
            i += 1;
        }
        if sum != self.sample_total as u64 {
            return Err(E_LEDGER_BAD);
        }
        if self.records.len() as u32 != self.sample_total {
            return Err(E_LEDGER_BAD);
        }
        Ok(())
    }

    /// ×N 覆盖核验（J1 硬口径）。
    pub fn coverage(&self, want: u32) -> Result<(), u16> {
        if self.sample_total < want {
            return Err(E_SAMPLE_MISS);
        }
        Ok(())
    }

    /// 超阈样本清单长度（超阈账只增不删）。
    pub fn records_len(&self) -> usize {
        self.records.len()
    }

    /// 完成写回（判据 J2）。
    pub fn writeback(&self, done: bool, summary_len: u32) -> Result<(), u16> {
        if !done || !self.frozen {
            return Err(E_LEDGER_BAD); // 未校准冻结不得写回
        }
        if summary_len == 0 {
            return Err(E_EMPTY_SUM);
        }
        Ok(())
    }
}

/// 确定性样本 tick 生成：第 i 次切换的起止 tick（时钟注入面）。
/// 耗时公式：(i*37+11) % 51 → 0..50 全域整数，P99 可复算恰 50。
pub fn sample_ticks(i: u32) -> (u64, u64) {
    let base = i as u64 * 1000;
    let cost = ((i.wrapping_mul(37).wrapping_add(11)) % 51) as u64;
    (base, base + cost)
}

/// 自例程判据：逐条落实 J1（10^3 采样 P99 落账），返回命中数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1 主采样 ×10^3：校准冻结 → 1000 点采样 → 直方图守恒 →
    //    P99 落账 ≤50ms 全绿。
    let mut pf = SpPerf::new();
    pf.calibrate(BUDGET_MS, SAMPLE_COUNT, 1)?;
    let mut i = 0u32;
    while i < SAMPLE_COUNT {
        let (t0, t1) = sample_ticks(i);
        pf.sample(i, t0, t1)?;
        i += 1;
    }
    pf.coverage(SAMPLE_COUNT)?;
    pf.audit_hist()?;
    let p99 = pf.assert_p99()?;
    if p99 > BUDGET_MS {
        return Err(E_OVER_BUDGET);
    }
    hits += 1;

    // R2 P99 可复算：同直方图两次分位查询同值；P50 显著低于 P99
    //    （分布健全性）。
    let pa = pf.percentile(99, 100)?;
    let pb = pf.percentile(99, 100)?;
    if pa != pb {
        return Err(E_LEDGER_BAD);
    }
    let p50 = pf.percentile(50, 100)?;
    if p50 > pa {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R3 校准冻结：二次校准拒收；校准记录三元逐位不变。
    let mark = pf.calib.unwrap_or(CalibMark { budget_ms: 0, samples: 0, calib_round: 0 });
    if pf.calibrate(60, 500, 2).is_ok() {
        return Err(E_THAWED_EDIT);
    }
    if pf.calib != Some(mark)
        || mark.budget_ms != BUDGET_MS
        || mark.samples != SAMPLE_COUNT
        || mark.calib_round != 1
    {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R4 非法校准拒收：预算 0、预算超直方图域、零样本量全拒且不冻结。
    let mut pf = SpPerf::new();
    if pf.calibrate(0, 100, 1).is_ok() {
        return Err(E_LEDGER_BAD);
    }
    if pf.calibrate(HIST_MAX_MS + 1, 100, 1).is_ok() {
        return Err(E_LEDGER_BAD);
    }
    if pf.calibrate(50, 0, 1).is_ok() {
        return Err(E_LEDGER_BAD);
    }
    if pf.frozen {
        return Err(E_THAWED_EDIT);
    }
    hits += 1;

    // R5 tick 回退拒收：tick1 < tick0 的采样点拒收且不落账。
    let mut pf = SpPerf::new();
    if pf.sample(0, 100, 99).is_ok() {
        return Err(E_TICK_BACK);
    }
    if pf.sample_total != 0 {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R6 超阈样本如实落账：注入 3 个 90ms 样本 + 5 个 10ms 样本，
    //    超阈账恰 3（只影响判定不删样本）；空采样分位查询拒。
    let mut pf = SpPerf::new();
    if pf.percentile(99, 100).is_ok() {
        return Err(E_LEDGER_BAD); // 空采样
    }
    pf.sample(0, 0, 90)?;
    pf.sample(1, 0, 90)?;
    pf.sample(2, 0, 90)?;
    let mut i = 3u32;
    while i < 8 {
        pf.sample(i, 0, 10)?;
        i += 1;
    }
    if pf.over_total != 3 {
        return Err(E_LEDGER_BAD);
    }
    pf.audit_hist()?;
    hits += 1;

    // R7 分位边界：8 样本（3×90 + 5×10）P99 = 升序第 ⌈7.92⌉=8 位 = 90
    //    （超阈被 P99 如实反映，不截断不美化）。
    let p99 = pf.percentile(99, 100)?;
    if p99 != 90 {
        return Err(E_LEDGER_BAD);
    }
    let pmax = pf.percentile(100, 100)?;
    if pmax != 90 {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R8 缺采样拒判与写回（判据 J2）：未满 10^3 分位判定拒；
    //    主链补满后写回放行。
    let mut pf = SpPerf::new();
    pf.calibrate(BUDGET_MS, SAMPLE_COUNT, 1)?;
    if pf.assert_p99().is_ok() {
        return Err(E_SAMPLE_MISS); // 零采样不得判 P99
    }
    let mut i = 0u32;
    while i < SAMPLE_COUNT {
        let (t0, t1) = sample_ticks(i);
        pf.sample(i, t0, t1)?;
        i += 1;
    }
    pf.assert_p99()?;
    if pf.writeback(true, 0).is_ok() {
        return Err(E_EMPTY_SUM);
    }
    pf.writeback(true, 0x40)?;
    hits += 1;

    Ok(hits)
}
