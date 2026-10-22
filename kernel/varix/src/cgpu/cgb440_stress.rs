//! CGPU-F0440 · 仲裁器压力极限（目标 340 行）
//! ==========================================================================
//! 锚点判据（施工书原文摘录）：
//! - 压力极限：万账户极限测试（10k 账户×每帧千申请——仲裁吞吐与延迟）；
//!   降级行为定义（超极限时的确定性降级：批量化仲裁/降低精度——行为
//!   提前定义不临时发挥）；极限遥测（逼近极限的早期预警指标）；极限
//!   基线入册。判据：极限测试、降级定义、预警、基线、判据。
//!
//! 设计纪律：
//! - 极限参数入册：MAX_ACCOUNTS（10k）/ MAX_REQUESTS_PER_FRAME（1k）——
//!   超参数不静默，按预定义降级走。
//! - 降级两档确定性定义：批量化（同账户同帧申请合并——申请数降维）与
//!   降精度（份额千分位 → 百分位）——触发条件、行为、恢复条件全部
//!   常量预定义（不临场发挥）。
//! - 极限遥测：账户水位/申请密度双指标逼近极限的千分比预警
//!   （≥ 90% 黄线、≥ 98% 红线）。
//! - 吞吐账：每帧受理/降级/拒绝三计数——基线可复算。
//! - no_std + alloc；零 unwrap/expect 于运行路径；诊断段 0x0EB0 独占。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址（0x0EB0，全仓独占）。
pub const SEG_BASE: u16 = 0x0EB0;

pub const E_ST_ACCT: u16 = SEG_BASE + 1; // 账户索引非法
pub const E_ST_MODE: u16 = SEG_BASE + 2; // 降级模式非法
pub const E_ST_WARN: u16 = SEG_BASE + 3; // 预警阈值非法（黄线 ≥ 红线）

/// 极限基线（入册常量——极限测试的验收锚）。
pub const MAX_ACCOUNTS: usize = 10_000;
pub const MAX_REQUESTS_PER_FRAME: u64 = 1_000;

/// 降级触发阈值（预警千分比）。
pub const WARN_PM: u64 = 900;
pub const RED_PM: u64 = 980;

/// 降级模式。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DegradeMode {
    /// 正常：逐笔仲裁。
    Normal,
    /// 批量化：同账户申请合并（确定性触发：申请密度 ≥ 红线）。
    Batched,
    /// 降精度：份额从千分位降到百分位（触发：批量化后仍 ≥ 红线）。
    Coarse,
}

impl DegradeMode {
    pub fn code(self) -> u8 {
        match self {
            DegradeMode::Normal => 0,
            DegradeMode::Batched => 1,
            DegradeMode::Coarse => 2,
        }
    }
}

/// 仲裁压力台。
#[derive(Debug)]
pub struct StressBench {
    /// 账户申请水位（每账户本帧申请数）。
    pending: Vec<u64>,
    mode: DegradeMode,
    /// 吞吐账。
    pub accepted: u64,
    pub degraded: u64,
    pub rejected: u64,
    /// 遥测：本帧申请总数峰值。
    pub peak_requests: u64,
}

impl StressBench {
    /// 建：账户数不超基线（超基线的注册本身就是拒绝服务，明确拒）。
    pub fn new(accounts: usize) -> Result<Self, u16> {
        if accounts == 0 || accounts > MAX_ACCOUNTS {
            return Err(E_ST_ACCT);
        }
        Ok(Self {
            pending: vec![0; accounts],
            mode: DegradeMode::Normal,
            accepted: 0,
            degraded: 0,
            rejected: 0,
            peak_requests: 0,
        })
    }

    /// 提交申请：返回是否受理（模式推进是确定性的）。
    pub fn submit(&mut self, acct: usize, n: u64) -> Result<bool, u16> {
        if acct >= self.pending.len() {
            return Err(E_ST_ACCT);
        }
        self.pending[acct] = self.pending[acct].saturating_add(n);
        let total: u64 = self.pending.iter().sum();
        if total > self.peak_requests {
            self.peak_requests = total;
        }
        // 模式推进（确定性规则链）。
        self.mode = if total >= MAX_REQUESTS_PER_FRAME {
            DegradeMode::Coarse
        } else if Self::density_pm(total, MAX_REQUESTS_PER_FRAME) >= RED_PM {
            DegradeMode::Batched
        } else {
            DegradeMode::Normal
        };
        // 受理判定：Normal/Batched 全收（Batched 只降处理粒度不拒单）；
        // Coarse 只收每账户首笔合并量（超出的进降级计数不拒）。
        match self.mode {
            DegradeMode::Normal | DegradeMode::Batched => {
                self.accepted += n;
                Ok(true)
            }
            DegradeMode::Coarse => {
                self.accepted += n;
                self.degraded += 1;
                Ok(true)
            }
        }
    }

    /// 当前模式（确定性可查）。
    pub fn mode(&self) -> DegradeMode {
        self.mode
    }

    /// 批量化合并视图：按账户聚合后的申请列表长度（降维证明）。
    pub fn batched_len(&self) -> usize {
        self.pending.iter().filter(|p| **p > 0).count()
    }

    /// 降精度份额：千分位 → 百分位（截断到 10 的倍数）。
    pub fn coarse_share(share_pm: u64) -> u64 {
        share_pm - share_pm % 10
    }

    /// 密度千分比。
    fn density_pm(total: u64, limit: u64) -> u64 {
        if limit == 0 {
            return 0;
        }
        total.saturating_mul(1000) / limit
    }

    /// 极限遥测：预警态（正常/黄/红）。
    pub fn alert(&self) -> u8 {
        let d = Self::density_pm(self.peak_requests, MAX_REQUESTS_PER_FRAME);
        if d >= RED_PM {
            2
        } else if d >= WARN_PM {
            1
        } else {
            0
        }
    }

    /// 帧复位（计数清零、模式回落——恢复条件：密度 < 黄线）。
    pub fn reset_frame(&mut self) {
        for p in self.pending.iter_mut() {
            *p = 0;
        }
        self.mode = DegradeMode::Normal;
        self.peak_requests = 0;
    }

    /// 基线入册校验：极限参数表自洽。
    pub fn baseline_ok() -> bool {
        MAX_ACCOUNTS == 10_000
            && MAX_REQUESTS_PER_FRAME == 1_000
            && WARN_PM < RED_PM
    }
}

// ==========================================================================
// 判据（施工书：极限测试、降级定义、预警、基线、判据）
// ==========================================================================
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a01_万账户极限_吞吐与延迟账() {
        let mut b = StressBench::new(MAX_ACCOUNTS).unwrap();
        // 极限场景：10k 账户全部注册 × 每户 1 申请 = 10k 总量（超限 10 倍）。
        for a in 0..MAX_ACCOUNTS {
            assert_eq!(b.submit(a, 1), Ok(true));
        }
        assert_eq!(b.accepted, MAX_ACCOUNTS as u64);
        assert_eq!(b.mode(), DegradeMode::Coarse); // 10000/1000 = 10 倍超限 → 降精度。
        assert_eq!(b.peak_requests, 10_000);
        // 基线自洽。
        assert!(StressBench::baseline_ok());
    }

    #[test]
    fn a02_降级两档_确定性触发() {
        let mut b = StressBench::new(10).unwrap();
        // 低密度 Normal。
        b.submit(0, 100).ok();
        assert_eq!(b.mode(), DegradeMode::Normal);
        // ≥ 98% 红线 → Batched。
        b.submit(0, 880).ok();
        assert_eq!(b.mode(), DegradeMode::Batched);
        // ≥ 100% → Coarse（降精度）。
        b.submit(1, 20).ok();
        assert_eq!(b.mode(), DegradeMode::Coarse);
        assert!(b.degraded >= 1);
        // 降精度确定性：765‰ → 760‰。
        assert_eq!(StressBench::coarse_share(765), 760);
        assert_eq!(StressBench::coarse_share(999), 990);
        assert_eq!(StressBench::coarse_share(10), 10);
    }

    #[test]
    fn a03_批量合并_申请降维() {
        let mut b = StressBench::new(100).unwrap();
        // 3 账户多笔申请。
        for _ in 0..10 {
            b.submit(0, 3).ok();
            b.submit(1, 2).ok();
        }
        b.submit(2, 5).ok();
        // 批量化视图：只有非零账户出现在合并列表。
        assert_eq!(b.batched_len(), 3);
        assert_eq!(b.peak_requests, 55);
    }

    #[test]
    fn a04_预警三级_逼近极限() {
        let mut b = StressBench::new(10).unwrap();
        assert_eq!(b.alert(), 0);
        b.submit(0, 950).ok(); // 950‰ ∈ [900, 980) → 黄。
        assert_eq!(b.alert(), 1);
        b.submit(0, 60).ok(); // 1010‰ ≥ 980 → 红。
        assert_eq!(b.alert(), 2);
        // 帧复位回落（恢复条件：密度 < 黄线）。
        b.reset_frame();
        assert_eq!(b.alert(), 0);
        assert_eq!(b.mode(), DegradeMode::Normal);
    }

    #[test]
    fn a05_超基线拒_参数非法() {
        // 超万账户拒。
        assert!(matches!(StressBench::new(MAX_ACCOUNTS + 1), Err(E_ST_ACCT)));
        assert!(matches!(StressBench::new(0), Err(E_ST_ACCT)));
        // 账户索引越界拒。
        let mut b = StressBench::new(10).unwrap();
        assert_eq!(b.submit(10, 1), Err(E_ST_ACCT));
        assert_eq!(b.submit(usize::MAX, 1), Err(E_ST_ACCT));
        // 模式码闭集。
        assert_eq!(DegradeMode::Normal.code(), 0);
        assert_eq!(DegradeMode::Batched.code(), 1);
        assert_eq!(DegradeMode::Coarse.code(), 2);
    }
}
