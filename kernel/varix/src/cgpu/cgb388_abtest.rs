//! CGPU-F0388 · 策略 A/B 框架（目标 340 行）
//! ==========================================================================
//! 锚点判据（施工书原文摘录）：
//! - A/B：影子运行（两仲裁策略分流量对比——不干扰生产）；三指标自动汇总
//!   （公平指数/完成率/延迟）；结论输出（统计显著性+建议）；影子开销上限。
//! - 判据：影子运行、三指标、结论、开销、判据。
//!
//! 设计纪律：
//! - 影子面只读生产流：B 组决策不入账（零生产污染），仅记对比账。
//! - 分流按请求 id 稳定哈希——同请求恒落同桶（可复现对比）。
//! - 显著性用大数定律近似：样本差超阈值带即判显著（保守，不假装精算）。
//! - no_std + alloc；零 unwrap/expect 于运行路径；诊断段 0x0B10 独占。

use alloc::vec::Vec;

/// 诊断段基址（0x0B10，全仓独占）。
pub const SEG_BASE: u16 = 0x0B10;

pub const E_TRAFFIC_LEAK: u16 = SEG_BASE + 1;
pub const E_INSUFFICIENT: u16 = SEG_BASE + 2;
pub const E_OVERHEAD_CAP: u16 = SEG_BASE + 3;
pub const E_METRIC_CORRUPT: u16 = SEG_BASE + 4;

/// 影子开销上限：影子样本/生产样本（千分位）≤ 400‰
///（与 MIN_SAMPLES=32 的显著性下界数学自洽：份额 ≥ 24% 才可能双侧达标）。
pub const SHADOW_CAP_PM: u32 = 400;
/// 显著性判定：三指标加权差（千分位）超此阈值且样本≥下限时判显著。
pub const SIGNIFICANCE_THRESHOLD_PM: u32 = 30;
pub const MIN_SAMPLES: usize = 32;

/// 策略标识。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Side {
    /// 生产侧（A）。
    A,
    /// 影子侧（B——不干扰生产）。
    B,
}

impl Side {
    pub fn other(self) -> Side {
        match self {
            Side::A => Side::B,
            Side::B => Side::A,
        }
    }
}

/// 稳定分流哈希（同 id 恒同桶——可复现分流）。
pub fn traffic_split(request_id: u64, b_ratio_pm: u32) -> Side {
    // FNV 单轮近似：把请求 id 摊到 0..1000。
    let mut h = 0xcbf29ce484222325u64 ^ request_id;
    h = h.wrapping_mul(0x100000001b3);
    let bucket = ((h >> 17) % 1000) as u32;
    if bucket < b_ratio_pm.min(1000) {
        Side::B
    } else {
        Side::A
    }
}

/// 单样本三指标。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sample {
    /// 公平指数（0~1000，越高越公平）。
    pub fairness_pm: u32,
    /// 完成率（0~1000）。
    pub completion_pm: u32,
    /// 延迟（周期）。
    pub latency: u32,
}

impl Sample {
    fn ok(&self) -> bool {
        self.fairness_pm <= 1000 && self.completion_pm <= 1000
    }
}

/// 单侧账本。
#[derive(Default)]
pub struct SideBook {
    pub samples: Vec<Sample>,
}

impl SideBook {
    fn summary(&self) -> Option<(u32, u32, u32)> {
        if self.samples.is_empty() {
            return None;
        }
        let f: u64 = self.samples.iter().map(|s| s.fairness_pm as u64).sum();
        let c: u64 = self.samples.iter().map(|s| s.completion_pm as u64).sum();
        let l: u64 = self.samples.iter().map(|s| s.latency as u64).sum();
        let n = self.samples.len() as u64;
        Some(((f / n) as u32, (c / n) as u32, (l / n) as u32))
    }
}

/// 三指标汇总（判据「三指标自动汇总」）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TriSummary {
    pub fairness_pm: u32,
    pub completion_pm: u32,
    pub latency_mean: u32,
}

/// 影子运行器。
pub struct ShadowRunner {
    b_ratio_pm: u32,
    a: SideBook,
    b: SideBook,
    /// 影子决策入账拦截计数（判据「不干扰生产」的审计面）。
    pub leak_blocks: u64,
}

impl ShadowRunner {
    pub fn new(b_ratio_pm: u32) -> Self {
        ShadowRunner { b_ratio_pm: b_ratio_pm.min(1000), a: SideBook::default(), b: SideBook::default(), leak_blocks: 0 }
    }

    /// 采集一条（按分流落桶）。影子侧样本必须带影子标记（leak 检测面）。
    pub fn collect(&mut self, request_id: u64, s: Sample, is_shadow_result: bool) -> Result<Side, u16> {
        if !s.ok() {
            return Err(E_METRIC_CORRUPT);
        }
        let side = traffic_split(request_id, self.b_ratio_pm);
        match side {
            Side::A => {
                // A 桶收到影子结果 = 生产流被污染。
                if is_shadow_result {
                    self.leak_blocks += 1;
                    return Err(E_TRAFFIC_LEAK);
                }
                self.a.samples.push(s);
            }
            Side::B => self.b.samples.push(s),
        }
        Ok(side)
    }

    /// 三指标汇总。
    pub fn summarize(&self) -> Option<(TriSummary, TriSummary)> {
        let (fa, ca, la) = self.a.summary()?;
        let (fb, cb, lb) = self.b.summary()?;
        Some((
            TriSummary { fairness_pm: fa, completion_pm: ca, latency_mean: la },
            TriSummary { fairness_pm: fb, completion_pm: cb, latency_mean: lb },
        ))
    }

    /// 影子开销（B 样本 / (A+B 样本)，千分位）。
    pub fn shadow_overhead_pm(&self) -> u32 {
        let total = self.a.samples.len() + self.b.samples.len();
        if total == 0 {
            return 0;
        }
        ((self.b.samples.len() as u64) * 1000 / total as u64) as u32
    }

    /// 结论输出（判据「结论」）：显著性 + 建议。
    pub fn conclude(&self) -> Result<(&'static str, &'static str), u16> {
        if self.shadow_overhead_pm() > SHADOW_CAP_PM {
            return Err(E_OVERHEAD_CAP);
        }
        if self.a.samples.len() < MIN_SAMPLES || self.b.samples.len() < MIN_SAMPLES {
            return Err(E_INSUFFICIENT);
        }
        let (sa, sb) = self.summarize().ok_or(E_INSUFFICIENT)?;
        // 三指标加权差：公平 0.4 + 完成 0.4 − 延迟 0.2（归一千分位近似）。
        let fair_d = sb.fairness_pm as i64 - sa.fairness_pm as i64;
        let comp_d = sb.completion_pm as i64 - sa.completion_pm as i64;
        let lat_d = (sa.latency_mean as i64 - sb.latency_mean as i64) * 1000 / (sa.latency_mean.max(1) as i64);
        let weighted = (fair_d * 2 + comp_d * 2 + lat_d * 1) / 5;
        if weighted.abs() < SIGNIFICANCE_THRESHOLD_PM as i64 {
            return Ok(("tie", "keep-a"));
        }
        if weighted > 0 {
            Ok(("b-better", "promote-b"))
        } else {
            Ok(("a-better", "keep-a"))
        }
    }

    pub fn counts(&self) -> (usize, usize) {
        (self.a.samples.len(), self.b.samples.len())
    }
}

// ---------------------------------------------------------------------------
// 判据自检
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    fn s(f: u32, c: u32, l: u32) -> Sample {
        Sample { fairness_pm: f, completion_pm: c, latency: l }
    }

    #[test]
    fn a88_影子运行_稳定分流与零泄漏() {
        let mut r = ShadowRunner::new(500);
        // 同 id 恒同桶。
        for _ in 0..10 {
            assert_eq!(traffic_split(42, 500), traffic_split(42, 500));
        }
        // 采集 64 条：A/B 都有落桶。
        for id in 0..64u64 {
            r.collect(id, s(600, 800, 100), false).unwrap();
        }
        let (na, nb) = r.counts();
        assert_eq!(na + nb, 64);
        assert!(na > 0 && nb > 0);
        // 泄漏检测：A 桶收到影子结果必拒并计数。
        let mut leak = ShadowRunner::new(0); // 全 A
        leak.collect(1, s(1, 1, 1), false).unwrap();
        assert_eq!(leak.collect(2, s(1, 1, 1), true).unwrap_err(), E_TRAFFIC_LEAK);
        assert_eq!(leak.leak_blocks, 1);
        // 非法指标拒。
        assert_eq!(r.collect(999, s(1001, 1, 1), false).unwrap_err(), E_METRIC_CORRUPT);
    }

    #[test]
    fn a88_三指标_自动汇总() {
        let mut r = ShadowRunner::new(300);
        for id in 0..80u64 {
            // 按分流结果赋值（确定性：A 侧定值、B 侧定值，不依赖哈希分布）。
            let side = traffic_split(id, 300);
            let m = if side == Side::B { s(700, 900, 100) } else { s(500, 700, 200) };
            r.collect(id, m, side == Side::B).unwrap();
        }
        let (na, nb) = r.counts();
        assert!(na > 0 && nb > 0);
        let (a, b) = r.summarize().unwrap();
        assert_eq!(a.fairness_pm, 500);
        assert_eq!(b.fairness_pm, 700);
        assert_eq!(a.latency_mean, 200);
        assert_eq!(b.latency_mean, 100);
        assert_eq!(a.completion_pm, 700);
        assert_eq!(b.completion_pm, 900);
        // 开销超上限路径：全量影子（ratio=1000）→ 1000‰ > 400‰ → 拒。
        let mut flood = ShadowRunner::new(1000);
        flood.collect(1, s(1, 1, 1), true).unwrap();
        assert_eq!(flood.conclude().unwrap_err(), E_OVERHEAD_CAP);
    }

    #[test]
    fn a88_结论_显著性与建议() {
        // B 明显更好（公平+200、完成+200、延迟减半）且开销合规（200‰=上限内）。
        let mut r = ShadowRunner::new(400);
        for id in 0..100u64 {
            let side = traffic_split(id, 400);
            let (m, is_shadow) = if side == Side::B {
                (s(700, 900, 100), true) // B 侧影子账本
            } else {
                (s(500, 700, 200), false) // A 侧生产账本
            };
            r.collect(id, m, is_shadow).unwrap();
        }
        assert!(r.shadow_overhead_pm() <= SHADOW_CAP_PM);
        assert_eq!(r.conclude().unwrap(), ("b-better", "promote-b"));
        // 样本不足。
        let mut thin = ShadowRunner::new(100);
        for id in 0..20u64 {
            let side = traffic_split(id, 100);
            thin.collect(id, s(1, 1, 1), side == Side::B).unwrap();
        }
        assert_eq!(thin.conclude().unwrap_err(), E_INSUFFICIENT);
        // 平局带内 → keep-a。
        let mut tie = ShadowRunner::new(400);
        for id in 0..100u64 {
            let side = traffic_split(id, 400);
            let (m, is_shadow) = if side == Side::B {
                (s(501, 700, 200), true)
            } else {
                (s(500, 700, 200), false)
            };
            tie.collect(id, m, is_shadow).unwrap();
        }
        assert_eq!(tie.conclude().unwrap(), ("tie", "keep-a"));
        // 常量自检。
        assert_eq!(SIGNIFICANCE_THRESHOLD_PM, 30);
        assert_eq!(Side::A.other(), Side::B);
    }
}
