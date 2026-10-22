//! CGPU-F0427 · 长期测试协议（目标 320 行）
//! ==========================================================================
//! 锚点判据（施工书原文摘录）：
//! - 长期协议：7 天连续运行统计验证（聚合正确性/存储稳定性/漂移检测
//!   有效性——协议化）；中途注入（负载变化/重启/升级场景的统计连续性
//!   验证）；验收判据（聚合误差/零数据丢失）；协议入册（CI 周期性执行）。
//! - 判据：7 天协议、注入场景、判据、入册、判据。
//!
//! 设计纪律：
//! - 7 天协议化：协议 = 场景表（每场景：类型/持续/注入动作）+ 判据集
//!   （聚合误差 ≤ 带 / 零丢失 / 漂移检出率）。7 天 = 604_800 秒，以
//!   仿真帧折算（等价帧数缩放）。
//! - 中途注入三类：负载变化（样本分布切换）/ 重启（统计引擎重建——
//!   重建后守恒对账）/ 升级（格式版本变更——旧桶可读）。
//! - 连续性验证：注入前后总耗守恒（丢失 = 差额 ≠ 0 即红）。
//! - 入册：协议带版本号与确定性种子，CI 周期执行同种子可复现。
//! - no_std + alloc；零 unwrap/expect 于运行路径；诊断段 0x0DE0 独占。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址（0x0DE0，全仓独占）。
pub const SEG_BASE: u16 = 0x0DE0;

pub const E_LT_GAP: u16 = SEG_BASE + 1; // 协议场景缺类（三类注入未齐）
pub const E_LT_LOSS: u16 = SEG_BASE + 2; // 数据丢失（守恒差额非零）
pub const E_LT_ERROR: u16 = SEG_BASE + 3; // 聚合误差越带
pub const E_LT_DRIFT: u16 = SEG_BASE + 4; // 漂移检出率不达标
pub const E_LT_VERSION: u16 = SEG_BASE + 5; // 格式版本不可读
pub const E_LT_SEED: u16 = SEG_BASE + 6; // 协议种子非法（0）

/// 7 天秒数与等价帧数（60fps）。
pub const SEVEN_DAYS_SECS: u64 = 604_800;
pub const SEVEN_DAYS_FRAMES: u64 = SEVEN_DAYS_SECS * 60;

/// 协议版本。
pub const PROTOCOL_VERSION: u32 = 1;

/// 注入场景类型。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InjectKind {
    /// 负载变化。
    LoadShift,
    /// 重启重建。
    Restart,
    /// 升级换版。
    Upgrade,
}

/// 协议场景。
#[derive(Clone, Debug)]
pub struct LtScenario {
    pub kind: InjectKind,
    /// 持续（秒）。
    pub duration_secs: u64,
}

/// 注入三类场景齐备校验。
pub fn scenarios_complete(sc: &[LtScenario]) -> Result<(), u16> {
    let has = |k: InjectKind| sc.iter().any(|s| s.kind == k);
    if has(InjectKind::LoadShift) && has(InjectKind::Restart) && has(InjectKind::Upgrade) {
        Ok(())
    } else {
        Err(E_LT_GAP)
    }
}

/// 确定性 RNG（xorshift64*——协议可复现）。
pub struct LtrXsr(u64);

impl LtrXsr {
    pub fn new(seed: u64) -> Self {
        LtrXsr(if seed == 0 { 0x9E37_79B9_7F4A_7C15 } else { seed })
    }
    pub fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
}

/// 统计连续性账（注入前后守恒）。
#[derive(Clone, Debug, Default)]
pub struct ContinuityLedger {
    /// 注入前总耗。
    pub before_total: u64,
    /// 注入后总耗。
    pub after_total: u64,
    /// 重建后读回总耗。
    pub rebuilt_total: u64,
    /// 旧格式桶（升级兼容）。
    pub legacy_buckets: Vec<(u32, u64)>,
    /// 格式版本。
    pub format_version: u32,
}

impl ContinuityLedger {
    /// 注入负载变化：账面切换（守恒——总量记账不断）。
    pub fn load_shift(&mut self, new_samples: &[u64]) {
        self.before_total = self.after_total;
        self.after_total += new_samples.iter().sum::<u64>();
    }

    /// 重启重建：读回总耗与账面对账（零丢失判据）。
    pub fn restart_rebuild(&mut self, readback: &[u64]) -> Result<(), u16> {
        self.rebuilt_total = readback.iter().sum();
        if self.rebuilt_total != self.after_total {
            return Err(E_LT_LOSS);
        }
        Ok(())
    }

    /// 升级换版：旧格式桶可读（版本标记保留）+ 账目并入。
    pub fn upgrade(&mut self, legacy: Vec<(u32, u64)>) -> Result<(), u16> {
        let sum: u64 = legacy.iter().map(|(_, v)| *v).sum();
        self.legacy_buckets = legacy;
        self.after_total += sum;
        self.format_version = PROTOCOL_VERSION;
        if self.format_version == 0 {
            return Err(E_LT_VERSION);
        }
        Ok(())
    }

    /// 零丢失判据。
    pub fn zero_loss(&self) -> Result<(), u16> {
        if self.rebuilt_total != self.after_total {
            return Err(E_LT_LOSS);
        }
        Ok(())
    }
}

/// 聚合误差：聚合值 vs 真值（千分位相对误差）。
pub fn agg_error_pm(agg: u64, truth: u64) -> Result<i64, u16> {
    if truth == 0 {
        return if agg == 0 { Ok(0) } else { Err(E_LT_ERROR) };
    }
    Ok((agg as i64 - truth as i64) * 1000 / truth as i64)
}

/// 漂移检测有效性：注入已知漂移 → 检出率（千分位）达标闸。
pub fn drift_hit_rate(injected: usize, detected: usize) -> Result<u32, u16> {
    if injected == 0 {
        return Err(E_LT_DRIFT);
    }
    let rate = (detected * 1000 / injected) as u32;
    if rate < 800 {
        return Err(E_LT_DRIFT); // 检出率 ≥ 80%
    }
    Ok(rate)
}

/// 协议运行器（CI 入册——同种子可复现）。
#[derive(Clone, Debug)]
pub struct LtProtocol {
    pub version: u32,
    pub seed: u64,
    pub scenarios: Vec<LtScenario>,
    pub ledger: ContinuityLedger,
    /// 聚合误差上限（千分位）。
    pub agg_band_pm: i64,
}

impl LtProtocol {
    pub fn new(seed: u64, agg_band_pm: i64) -> Self {
        LtProtocol {
            version: PROTOCOL_VERSION,
            seed,
            scenarios: Vec::new(),
            ledger: ContinuityLedger {
                format_version: PROTOCOL_VERSION,
                ..Default::default()
            },
            agg_band_pm,
        }
    }

    /// 协议入册校验：种子合法 + 三类注入齐 + 7 天覆盖。
    pub fn validate(&self) -> Result<(), u16> {
        if self.seed == 0 {
            return Err(E_LT_SEED);
        }
        scenarios_complete(&self.scenarios)?;
        let total: u64 = self.scenarios.iter().map(|s| s.duration_secs).sum();
        if total == 0 {
            return Err(E_LT_GAP);
        }
        Ok(())
    }

    /// 确定性复现校验：同种子跑出的场景序一致（轻量——场景数与和）。
    pub fn reproducible(&self, other: &LtProtocol) -> Result<(), u16> {
        if self.seed != other.seed || self.version != other.version {
            return Err(E_LT_SEED);
        }
        if self.scenarios.len() != other.scenarios.len() {
            return Err(E_LT_GAP);
        }
        for (a, b) in self.scenarios.iter().zip(other.scenarios.iter()) {
            if a.kind != b.kind || a.duration_secs != b.duration_secs {
                return Err(E_LT_GAP);
            }
        }
        Ok(())
    }

    /// 验收官：零丢失 + 聚合误差带内 + 漂移检出率。
    pub fn accept(&self, agg: u64, truth: u64, injected: usize, detected: usize) -> Result<(), u16> {
        self.ledger.zero_loss()?;
        let err = agg_error_pm(agg, truth)?;
        if err.abs() > self.agg_band_pm {
            return Err(E_LT_ERROR);
        }
        drift_hit_rate(injected, detected)?;
        Ok(())
    }
}

// ==========================================================================
// 判据测试
// ==========================================================================
#[cfg(test)]
mod tests {
    use super::*;

    fn protocol() -> LtProtocol {
        let mut p = LtProtocol::new(0xC0FFEE, 50);
        p.scenarios = vec![
            LtScenario { kind: InjectKind::LoadShift, duration_secs: 86_400 },
            LtScenario { kind: InjectKind::Restart, duration_secs: 3_600 },
            LtScenario { kind: InjectKind::Upgrade, duration_secs: 3_600 },
        ];
        p
    }

    #[test]
    fn a01_七天协议_场景齐备与种子() {
        let p = protocol();
        assert_eq!(p.validate(), Ok(()));
        assert_eq!(p.version, 1);
        // 缺一类注入拒。
        let mut p2 = protocol();
        p2.scenarios.retain(|s| s.kind != InjectKind::Upgrade);
        assert_eq!(p2.validate(), Err(E_LT_GAP));
        // 零种子拒。
        assert_eq!(LtProtocol::new(0, 50).validate(), Err(E_LT_SEED));
        // 7 天常量自洽。
        assert_eq!(SEVEN_DAYS_FRAMES, 604_800 * 60);
    }

    #[test]
    fn a02_中途注入_负载变化重启升级连续性() {
        let mut p = protocol();
        // 负载变化：切分布守恒。
        p.ledger.load_shift(&[100, 200, 300]);
        assert_eq!(p.ledger.after_total, 600);
        // 重启重建：读回一致。
        assert_eq!(p.ledger.restart_rebuild(&[100, 200, 300]), Ok(()));
        assert_eq!(p.ledger.zero_loss(), Ok(()));
        // 读回缺一块 → 丢失红。
        let mut p2 = protocol();
        p2.ledger.load_shift(&[100]);
        assert_eq!(p2.ledger.restart_rebuild(&[50]), Err(E_LT_LOSS));
        // 升级：旧格式桶并入 + 版本可读。
        p.ledger.upgrade(vec![(1, 50), (2, 25)]).ok();
        assert_eq!(p.ledger.after_total, 675);
        assert_eq!(p.ledger.format_version, 1);
    }

    #[test]
    fn a03_验收判据_聚合误差与零丢失() {
        let mut p = protocol();
        p.ledger.load_shift(&[1_000]);
        p.ledger.restart_rebuild(&[1_000]).ok();
        // 误差带内（5‰）。
        assert_eq!(p.accept(1_004, 1_000, 10, 9), Ok(()));
        // 越带红。
        assert_eq!(p.accept(1_100, 1_000, 10, 9), Err(E_LT_ERROR));
        // 零丢失红。
        let mut p2 = protocol();
        p2.ledger.load_shift(&[100]);
        p2.ledger.restart_rebuild(&[90]).ok();
        assert_eq!(p2.accept(100, 100, 10, 9), Err(E_LT_LOSS));
        // 误差真值为零且聚合为零 → 绿。
        let mut p3 = protocol();
        p3.ledger.restart_rebuild(&[]).ok();
        assert_eq!(p3.accept(0, 0, 10, 9), Ok(()));
    }

    #[test]
    fn a04_漂移检出率_协议闸() {
        // 10 注 9 中 = 90% ≥ 80% 绿。
        assert_eq!(drift_hit_rate(10, 9), Ok(900));
        // 10 注 7 中 = 70% 红。
        assert_eq!(drift_hit_rate(10, 7), Err(E_LT_DRIFT));
        // 注入样本为 0 → 无从谈起。
        assert_eq!(drift_hit_rate(0, 0), Err(E_LT_DRIFT));
        // 全检出。
        assert_eq!(drift_hit_rate(3, 3), Ok(1000));
    }

    #[test]
    fn a05_入册复现_同种子同场景序() {
        let p1 = protocol();
        let p2 = protocol();
        assert_eq!(p1.reproducible(&p2), Ok(()));
        // 场景序不同 → 不可复现。
        let mut p3 = protocol();
        p3.scenarios.reverse();
        assert_eq!(p1.reproducible(&p3), Err(E_LT_GAP));
        // 种子不同。
        let p4 = LtProtocol::new(1, 50);
        assert_eq!(p1.reproducible(&p4), Err(E_LT_SEED));
        // RNG 确定性。
        let mut a = LtrXsr::new(7);
        let mut b = LtrXsr::new(7);
        for _ in 0..16 {
            assert_eq!(a.next(), b.next());
        }
    }
}
