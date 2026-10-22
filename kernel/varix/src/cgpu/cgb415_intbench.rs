//! CGPU-F0415 · 集成基准（目标 300 行）
//! ==========================================================================
//! 锚点判据（施工书原文摘录）：
//! - 基准：集成开销（各联动点的延迟开销实测——预算内声明）；联动延迟
//!   基准（可见性→预算调整 <1 帧实测）——入册回归。
//! - 判据：开销实测、入册、判据。
//!
//! 设计纪律：
//! - 开销实测：每个联动点（D/E/H/I/J/K + VE）一个开销账——联动动作
//!   从发起到生效的耗时（µs），实测样本入账后按 P95 汇总。
//! - 预算内声明：每点有开销预算（µs/次），P95 ≤ 预算即「预算内」声明
//!   成立；越预算即红（入册回归挡）。
//! - 联动延迟基准：可见性事件 → 预算调整生效的帧延迟，实测 <1 帧
//!   （同帧生效）——基准入册后做回归（新测 vs 基线偏差带）。
//! - 入册回归：基准册冻结 v1，回归口径与 F0399 同构（偏差带判定）。
//! - no_std + alloc；零 unwrap/expect 于运行路径；诊断段 0x0D20 独占。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址（0x0D20，全仓独占）。
pub const SEG_BASE: u16 = 0x0D20;

pub const E_IB_POINT: u16 = SEG_BASE + 1; // 联动点非法
pub const E_IB_OVERBUDGET: u16 = SEG_BASE + 2; // P95 越开销预算
pub const E_IB_FRAME: u16 = SEG_BASE + 3; // 联动延迟 ≥1 帧（违反 <1 帧合同）
pub const E_IB_FROZEN: u16 = SEG_BASE + 4; // 基线已冻结
pub const E_IB_DRIFT: u16 = SEG_BASE + 5; // 回归越带
pub const E_IB_EMPTY: u16 = SEG_BASE + 6; // 无样本即声明

/// 联动点（六域 + VE = 7）。
pub const IB_COUNT: usize = 7;
/// 可见性联动的帧预算：1 帧（延迟 <1 帧合同）。
pub const VIS_BUDGET_US: u32 = 12_500; // 80fps 单帧 12.5ms
/// 回归偏差带（千分位）。
pub const IB_BAND_PM: u32 = 150;

pub fn point_name(p: usize) -> &'static str {
    match p {
        0 => "D帧联动",
        1 => "E表面联动",
        2 => "H显存联动",
        3 => "I带宽联动",
        4 => "J功耗联动",
        5 => "K异构联动",
        _ => "VE跨卷",
    }
}

/// 开销账：一个联动点的实测样本集。
#[derive(Clone, Debug, Default)]
pub struct OverheadLedger {
    pub samples_us: Vec<u32>,
    /// 每次联动的生效帧延迟（0 = 同帧生效）。
    pub lat_frames: Vec<u8>,
}

impl OverheadLedger {
    pub fn record(&mut self, us: u32, lat_frames: u8) {
        self.samples_us.push(us);
        self.lat_frames.push(lat_frames);
    }

    /// P95（升序取 95% 位）。
    pub fn p95(&self) -> Option<u32> {
        if self.samples_us.is_empty() {
            return None;
        }
        let mut s = self.samples_us.clone();
        s.sort_unstable();
        let idx = (s.len() * 95 / 100).min(s.len() - 1);
        Some(s[idx])
    }

    /// <1 帧合同：所有联动延迟均为 0（同帧生效）。
    pub fn within_one_frame(&self) -> bool {
        !self.lat_frames.is_empty() && self.lat_frames.iter().all(|f| *f == 0)
    }

    /// 声明：P95 ≤ 预算 且 延迟 <1 帧。
    pub fn declare(&self, budget_us: u32) -> Result<u32, u16> {
        let p = self.p95().ok_or(E_IB_EMPTY)?;
        if p > budget_us {
            return Err(E_IB_OVERBUDGET);
        }
        if !self.within_one_frame() {
            return Err(E_IB_FRAME);
        }
        Ok(p)
    }
}

/// 集成开销基准册。
#[derive(Clone, Debug)]
pub struct IntBench {
    /// 每点开销预算（µs/次）。
    pub budgets: [u32; IB_COUNT],
    pub ledgers: [OverheadLedger; IB_COUNT],
    /// 基线（冻结后只回归）。
    pub baseline: [u32; IB_COUNT],
    pub frozen: bool,
}

impl IntBench {
    pub fn new(budgets: [u32; IB_COUNT]) -> Self {
        IntBench {
            budgets,
            ledgers: Default::default(),
            baseline: [0; IB_COUNT],
            frozen: false,
        }
    }

    /// 实测记录。
    pub fn record(&mut self, point: usize, us: u32, lat: u8) -> Result<(), u16> {
        if point >= IB_COUNT {
            return Err(E_IB_POINT);
        }
        self.ledgers[point].record(us, lat);
        Ok(())
    }

    /// 预算内声明（逐点）。
    pub fn declare(&self, point: usize) -> Result<u32, u16> {
        if point >= IB_COUNT {
            return Err(E_IB_POINT);
        }
        self.ledgers[point].declare(self.budgets[point])
    }

    /// 基线装订：P95 写入基线并冻结。
    pub fn freeze_baseline(&mut self) -> Result<(), u16> {
        if self.frozen {
            return Err(E_IB_FROZEN);
        }
        for p in 0..IB_COUNT {
            let p95 = self.ledgers[p].p95().ok_or(E_IB_EMPTY)?;
            self.baseline[p] = p95;
        }
        self.frozen = true;
        Ok(())
    }

    /// 入册回归：新样本对基线偏差带判定（方向：开销越大越坏）。
    pub fn regress(&self, point: usize, us: u32) -> Result<i64, u16> {
        if point >= IB_COUNT {
            return Err(E_IB_POINT);
        }
        if !self.frozen {
            return Err(E_IB_FROZEN);
        }
        let base = self.baseline[point] as i64;
        let drift = us as i64 - base;
        if drift > base * IB_BAND_PM as i64 / 1000 {
            return Err(E_IB_DRIFT);
        }
        Ok(drift)
    }

    /// 全点回归（册级）。
    pub fn regress_all(&self, samples: &[(usize, u32)]) -> Result<usize, u16> {
        let mut n = 0;
        for (p, us) in samples {
            self.regress(*p, *us)?;
            n += 1;
        }
        Ok(n)
    }
}

// ==========================================================================
// 判据测试
// ==========================================================================
#[cfg(test)]
mod tests {
    use super::*;

    fn bench() -> IntBench {
        IntBench::new([800, 500, 600, 400, 300, 700, 1_000])
    }

    #[test]
    fn a01_开销实测_样本入账与P95() {
        let mut b = bench();
        for v in [300u32, 320, 310, 305, 330, 315, 325, 308, 312, 318] {
            b.record(0, v, 0).ok();
        }
        assert_eq!(b.ledgers[0].samples_us.len(), 10);
        // P95 在最大值附近。
        let p = b.ledgers[0].p95().unwrap_or(0);
        assert!(p >= 318 && p <= 330);
        // 点越界拒。
        assert_eq!(b.record(9, 1, 0), Err(E_IB_POINT));
    }

    #[test]
    fn a02_预算内声明_P95越线即红() {
        let mut b = bench();
        // J 功耗联动预算 300µs。
        for v in [100u32; 10] {
            b.record(4, v, 0).ok();
        }
        assert_eq!(b.declare(4), Ok(100));
        // 越预算样本混入 ⇒ P95 越线红。
        let mut b2 = bench();
        for v in [100u32; 9] {
            b2.record(4, v, 0).ok();
        }
        b2.record(4, 1_000, 0).ok();
        assert_eq!(b2.declare(4), Err(E_IB_OVERBUDGET));
        // 无样本声明拒。
        assert_eq!(bench().declare(0), Err(E_IB_EMPTY));
    }

    #[test]
    fn a03_联动延迟_同帧生效合同() {
        let mut b = bench();
        for v in [200u32; 5] {
            b.record(1, v, 0).ok(); // 同帧
        }
        assert_eq!(b.declare(1), Ok(200));
        // 有一帧延迟 ≥1 ⇒ 合同破。
        let mut b2 = bench();
        for v in [200u32; 4] {
            b2.record(1, v, 0).ok();
        }
        b2.record(1, 200, 1).ok();
        assert_eq!(b2.declare(1), Err(E_IB_FRAME));
        assert!(!b2.ledgers[1].within_one_frame());
        assert!(b.ledgers[1].within_one_frame());
    }

    #[test]
    fn a04_入册_基线冻结与回归带() {
        let mut b = bench();
        for p in 0..IB_COUNT {
            for _ in 0..5 {
                b.record(p, 200, 0).ok();
            }
        }
        assert_eq!(b.freeze_baseline(), Ok(()));
        assert!(b.frozen);
        assert!(b.baseline.iter().all(|v| *v == 200));
        // 冻后不可再冻。
        assert_eq!(b.freeze_baseline(), Err(E_IB_FROZEN));
        // 带内回归绿（+10%）；恰在带上绿；越带红（200×150‰=30）。
        assert_eq!(b.regress(0, 220), Ok(20));
        assert_eq!(b.regress(0, 230), Ok(30));
        assert_eq!(b.regress(0, 240), Err(E_IB_DRIFT));
        assert_eq!(b.regress(0, 250), Err(E_IB_DRIFT));
        // 未冻结回归拒。
        assert_eq!(bench().regress(0, 100), Err(E_IB_FROZEN));
    }

    #[test]
    fn a05_册级回归_批量与点名表() {
        let mut b = bench();
        for p in 0..IB_COUNT {
            for _ in 0..3 {
                b.record(p, 150, 0).ok();
            }
        }
        b.freeze_baseline().ok();
        let samples = [(0usize, 160u32), (3, 170), (6, 155)];
        assert_eq!(b.regress_all(&samples), Ok(3));
        // 单点越带拖红全批。
        assert_eq!(b.regress_all(&[(0, 999)]), Err(E_IB_DRIFT));
        // 点名表完整。
        assert_eq!(point_name(0), "D帧联动");
        assert_eq!(point_name(6), "VE跨卷");
        assert_eq!(IB_COUNT, 7);
    }
}
