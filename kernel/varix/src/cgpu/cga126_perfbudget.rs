//! cga126_perfbudget — CGPU-F0126 A 域性能总账。
//!
//! 判据原文（施工书锚点 CGPU-F0126）：A 域性能总账（F0383 范式）：逐算
//! 子吞吐表（F0090 汇总）+ 线程池开销 + 任务图开销的域级聚合。总账红
//! 线：A 域抽象开销 ≤ 帧预算 3%（CPU 侧渲染基础设施的税负上限）。
//! 判据：逐项账、聚合交叉验证、红线实测、超标处置、入证据。
//!
//! **设计定盘**：总账三层账目：逐算子吞吐行（算子 id + tick/千调用 +
//! 测量线程数）、线程池开销行、任务图开销行。聚合交叉验证 = 域开销
//! 合计的两种口径必须一致：口径一逐行累加、口径二域级直测；两口径
//! 偏差超 5‰ 即账目不平（拒收）。红线实测 = 域抽象开销 / 帧预算的
//! 千分比 ≤ 30‰（3%）。超标处置 = 超标时生成处置条目（降级项清单），
//! 处置后复测必须回到线内才可封账。
//!
//! 诊断码段 0x01C0~0x01CF（0x01 段第 13 子区）。
//! no_std + alloc 零 unwrap 零 expect 零 panic 面零 IO 零墙钟零浮点。

use alloc::vec::Vec;

/// 模块版本。
pub const CGA126_VERSION: &str = "cga126-perfbudget-v1";

/// 诊断子区基值（0x01 段第 13 子区）。
pub const SEG_BASE: u16 = 0x01C0;

/// 帧预算基准（tick，由 F0004 成本模型冻结口径注入）。
pub const FRAME_BUDGET_TICKS: u64 = 16_667;

/// 抽象开销红线（千分比——3% = 30‰）。
pub const OVERHEAD_REDLINE_PERMILLE: u64 = 30;

/// 双口径聚合容差（千分比）。
pub const CROSS_CHECK_TOL_PERMILLE: u64 = 5;

/// 逐项账行。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LedgerRow {
    /// 账目类别（0=算子 1=线程池开销 2=任务图开销）。
    pub kind: u8,
    /// 主体 id（算子号或组件号）。
    pub subject: u32,
    /// 每 tick 抽象开销（tick）。
    pub overhead_ticks: u64,
}

/// 性能总账。
pub struct PerfLedger {
    /// 逐项账。
    rows: Vec<LedgerRow>,
    /// 域级直测开销（口径二）。
    domain_direct_ticks: Option<u64>,
    /// 处置条目（超标处置）。
    disposals: Vec<(u32, u64)>,
    /// 封账指纹。
    sealed: Option<u64>,
}

impl PerfLedger {
    /// 空账。
    pub fn new() -> Self {
        PerfLedger {
            rows: Vec::new(),
            domain_direct_ticks: None,
            disposals: Vec::new(),
            sealed: None,
        }
    }

    /// 记账行（同类别同主体重复即拒；零开销行拒——零开销要么没测
    /// 要么测错，诚实红线）。
    pub fn record(&mut self, kind: u8, subject: u32, overhead_ticks: u64) -> Result<(), u16> {
        if self.sealed.is_some() {
            return Err(SEG_BASE + 1);
        }
        if kind > 2 {
            return Err(SEG_BASE + 2);
        }
        if overhead_ticks == 0 {
            return Err(SEG_BASE + 3);
        }
        if self
            .rows
            .iter()
            .any(|r| r.kind == kind && r.subject == subject)
        {
            return Err(SEG_BASE + 4);
        }
        self.rows.push(LedgerRow {
            kind,
            subject,
            overhead_ticks,
        });
        Ok(())
    }

    /// 直测登记（口径二；覆盖写即拒——直测不可被后测覆盖，只能重开账）。
    pub fn set_domain_direct(&mut self, ticks: u64) -> Result<(), u16> {
        if self.sealed.is_some() {
            return Err(SEG_BASE + 1);
        }
        if self.domain_direct_ticks.is_some() {
            return Err(SEG_BASE + 5);
        }
        if ticks == 0 {
            return Err(SEG_BASE + 3);
        }
        self.domain_direct_ticks = Some(ticks);
        Ok(())
    }

    /// 口径一合计（逐行累加）。
    pub fn sum_rows(&self) -> u64 {
        self.rows.iter().map(|r| r.overhead_ticks).sum()
    }

    /// 聚合交叉验证（判据"聚合交叉验证"）：两口径偏差 ≤ 5‰。
    pub fn cross_check(&self) -> Result<(), u16> {
        let direct = match self.domain_direct_ticks {
            Some(d) => d,
            None => return Err(SEG_BASE + 6),
        };
        let rows_sum = self.sum_rows();
        let larger = if rows_sum > direct { rows_sum } else { direct };
        if larger == 0 {
            return Err(SEG_BASE + 7);
        }
        let diff = if rows_sum > direct {
            rows_sum - direct
        } else {
            direct - rows_sum
        };
        if diff * 1000 > larger * CROSS_CHECK_TOL_PERMILLE {
            return Err(SEG_BASE + 8);
        }
        Ok(())
    }

    /// 红线实测（判据"红线实测"）：口径一合计 / 帧预算 ≤ 30‰。
    pub fn redline_check(&self) -> Result<u64, u16> {
        if self.rows.is_empty() {
            return Err(SEG_BASE + 9);
        }
        let sum = self.sum_rows();
        let permille = sum * 1000 / FRAME_BUDGET_TICKS;
        if permille > OVERHEAD_REDLINE_PERMILLE {
            Err(SEG_BASE + 10)
        } else {
            Ok(permille)
        }
    }

    /// 超标处置登记（判据"超标处置"：主体 + 预期回收 tick；
    /// 处置后复测：sum - 回收合计 必须回线内）。
    pub fn dispose(&mut self, subject: u32, reclaim_ticks: u64) -> Result<(), u16> {
        if self.sealed.is_some() {
            return Err(SEG_BASE + 1);
        }
        if reclaim_ticks == 0 {
            return Err(SEG_BASE + 11);
        }
        if !self.rows.iter().any(|r| r.subject == subject) {
            return Err(SEG_BASE + 12);
        }
        if self.disposals.iter().any(|(s, _)| *s == subject) {
            return Err(SEG_BASE + 13);
        }
        self.disposals.push((subject, reclaim_ticks));
        Ok(())
    }

    /// 处置后复测。
    pub fn redline_after_disposal(&self) -> Result<u64, u16> {
        let reclaim: u64 = self.disposals.iter().map(|(_, t)| t).sum();
        let sum = self.sum_rows().saturating_sub(reclaim);
        let permille = sum * 1000 / FRAME_BUDGET_TICKS;
        if permille > OVERHEAD_REDLINE_PERMILLE {
            Err(SEG_BASE + 10)
        } else {
            Ok(permille)
        }
    }

    /// 行数。
    pub fn row_count(&self) -> usize {
        self.rows.len()
    }

    /// 封账（判据"入证据"：交叉验证过 + 线内才能封）。
    pub fn seal(&mut self) -> Result<u64, u16> {
        if self.sealed.is_some() {
            return Err(SEG_BASE + 14);
        }
        self.cross_check()?;
        if self.redline_check().is_err() && self.redline_after_disposal().is_err() {
            return Err(SEG_BASE + 15);
        }
        let mut fp = 0xcbf2_9ce4_8422_2325u64;
        for r in &self.rows {
            fp = fp.wrapping_mul(0x100_0000_01b3) ^ r.overhead_ticks;
        }
        self.sealed = Some(fp);
        Ok(fp)
    }
}

impl Default for PerfLedger {
    fn default() -> Self {
        Self::new()
    }
}

/// 账目类别闭集名（判据"逐项账"的投影面）。
pub const KIND_NAMES: [&str; 3] = ["op", "threadpool", "taskgraph"];

/// 处置条目投影行。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DisposalRow {
    /// 处置主体。
    pub subject: u32,
    /// 预期回收 tick。
    pub reclaim_ticks: u64,
}

impl PerfLedger {
    /// 类别计数投影（三类各多少行）。
    pub fn kind_census(&self) -> [usize; 3] {
        let mut out = [0usize; 3];
        for r in &self.rows {
            out[r.kind as usize] += 1;
        }
        out
    }

    /// 类别开销占比（千分比；某类无行返回 None——占比是实数不是缺省）。
    pub fn kind_share_permille(&self, kind: u8) -> Option<u64> {
        if kind > 2 || self.sum_rows() == 0 {
            return None;
        }
        let k: u64 = self
            .rows
            .iter()
            .filter(|r| r.kind == kind)
            .map(|r| r.overhead_ticks)
            .sum();
        Some(k * 1000 / self.sum_rows())
    }

    /// 开销最大的主体（判据"超标处置"的定位面：谁贡献大头谁先处置；
    /// 空账返回 None）。
    pub fn worst_subject(&self) -> Option<(u32, u64)> {
        let mut best: Option<(u32, u64)> = None;
        for r in &self.rows {
            match best {
                Some((_, t)) if t >= r.overhead_ticks => {}
                _ => best = Some((r.subject, r.overhead_ticks)),
            }
        }
        best
    }

    /// 开销排行（降序，同值按主体号升序——整数稳定序）。
    pub fn ranked(&self) -> Vec<(u32, u64)> {
        let mut rows: Vec<(u32, u64)> =
            self.rows.iter().map(|r| (r.subject, r.overhead_ticks)).collect();
        rows.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        rows
    }

    /// 处置投影（判据"超标处置"的台账面）。
    pub fn disposal_rows(&self) -> Vec<DisposalRow> {
        self.disposals
            .iter()
            .map(|(s, t)| DisposalRow { subject: *s, reclaim_ticks: *t })
            .collect()
    }

    /// 回收覆盖核验（处置后复测的前置：回收合计必须 ≥ 超线超出量，
    /// 否则处置方案本身不可信）。
    pub fn disposal_covers_overage(&self) -> Result<(), u16> {
        let reclaim: u64 = self.disposals.iter().map(|(_, t)| t).sum();
        let permille = (self.sum_rows().saturating_sub(reclaim)) * 1000
            / FRAME_BUDGET_TICKS;
        if permille > OVERHEAD_REDLINE_PERMILLE {
            return Err(SEG_BASE + 16);
        }
        Ok(())
    }

    /// 直测值。
    pub fn direct_ticks(&self) -> Option<u64> {
        self.domain_direct_ticks
    }

    /// 封账指纹。
    pub fn seal_fp(&self) -> Option<u64> {
        self.sealed
    }
}


// ===== 深化块（cga126_perfbudget · 性能预算深化审计）：补充审计面，与主体判据同源同纪律 =====
pub mod deepen_perfbudget {
    //! 性能预算深化审计 —— 深化审计面：纯函数 + 常量表，零外部依赖。

    use alloc::vec;
    use alloc::vec::Vec;
    use crate::checks::CheckSet;

    /// 预算桶：16 桶固定直方图，越界样本计入溢出桶。
    pub struct BudgetHistogram {
        buckets: [u32; 16],
        overflow: u32,
        total: u64,
    }

    impl BudgetHistogram {
        pub fn new() -> Self {
            BudgetHistogram { buckets: [0; 16], overflow: 0, total: 0 }
        }

        /// 记录一个样本；width 为 0 拒收（除零防线）。
        pub fn record(&mut self, value: u32, width: u32) -> Result<(), u16> {
            if width == 0 {
                return Err(1);
            }
            let idx = (value / width) as usize;
            self.total += 1;
            if idx >= 16 {
                self.overflow += 1;
            } else {
                self.buckets[idx] += 1;
            }
            Ok(())
        }

        /// 守恒：桶内 + 溢出 == 总样本。
        pub fn conserve(&self) -> bool {
            let inside: u64 = self.buckets.iter().map(|&b| b as u64).sum();
            inside + self.overflow as u64 == self.total
        }

        /// 超支率（千分比）：溢出桶占比；空账显性 None 不谎报。
        pub fn overrun_permille(&self) -> Option<u32> {
            if self.total == 0 {
                return None;
            }
            Some(((self.overflow as u64 * 1000) / self.total) as u32)
        }
    }

    /// 预算修正建议：超支率分档给建议，档位闭集不静默。
    pub fn budget_advice(overrun_permille: u32) -> u8 {
        if overrun_permille >= 200 { 3 } else if overrun_permille >= 100 { 2 } else if overrun_permille >= 50 { 1 } else { 0 }
    }

    pub fn run_deepen_checks() -> CheckSet {
        let mut cs = CheckSet::new("CGPU-F0126-深化");
        let mut h = BudgetHistogram::new();
        for i in 0..100u32 {
            let _ = h.record(i * 10, 100);
        }
        let _ = h.record(99999, 100);
        cs.add("直方图守恒", h.conserve(), "");
        cs.add("超支率有值", h.overrun_permille().is_some(), "");
        cs.add("建议分档闭集", budget_advice(0) <= 3 && budget_advice(u32::MAX) == 3, "");
        cs
    }
}
