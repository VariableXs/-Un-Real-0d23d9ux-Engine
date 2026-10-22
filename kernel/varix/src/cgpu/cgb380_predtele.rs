//! CGPU-F0380 · 预测遥测（目标 300 行）
//! ==========================================================================
//! 锚点判据（施工书原文摘录）：
//! - 遥测：命中率/偏差分布/回退次数/预留浪费率/开销实测——五指标进 B08
//!   总线（预测质量可观测）。
//! - 判据：五指标、进总线、判据。
//!
//! 设计纪律：
//! - 五指标各自独立账本，单点写、快照读；指标缺失显性报 None 不补零。
//! - B08 总线包格式与 cgb364（B08 延迟直方图）同族：定长桶 + 溢出计数。
//! - no_std + alloc；零 unwrap/expect 于运行路径；诊断段 0x0A70 独占。

use alloc::vec::Vec;

/// 诊断段基址（0x0A70，全仓独占）。
pub const SEG_BASE: u16 = 0x0A70;

pub const E_METRIC_UNKNOWN: u16 = SEG_BASE + 1;
pub const E_SAMPLE_MISMATCH: u16 = SEG_BASE + 2;
pub const E_SNAPSHOT_BUSY: u16 = SEG_BASE + 3;

/// 指标编号（B08 总线槽位）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MetricId {
    /// 命中率（预测落进区间的比例，0~1000‰）。
    HitRate = 0,
    /// 偏差分布（|预测−实际| 的直方图）。
    BiasDist = 1,
    /// 回退次数（预测失败转兜底的累计计数）。
    FallbackCount = 2,
    /// 预留浪费率（预而未用 / 总预留，0~1000‰）。
    WasteRate = 3,
    /// 开销实测（千分位——F0379 账本直读）。
    Overhead = 4,
}

pub const METRIC_SLOTS: usize = 5;

/// 偏差直方图：8 桶定长（0/±50/±100/±200/±400/±800/±1600/超界 千分位绝对差）。
pub const BIAS_BUCKETS: usize = 8;
pub const BUCKET_BOUNDS_PM: [u32; BIAS_BUCKETS - 1] = [50, 100, 200, 400, 800, 1600, 3200];

pub fn bias_bucket(abs_bias_pm: u32) -> usize {
    for (i, b) in BUCKET_BOUNDS_PM.iter().enumerate() {
        if abs_bias_pm < *b {
            return i;
        }
    }
    BIAS_BUCKETS - 1
}

/// 五指标账本（单点写）。
#[derive(Clone, Debug)]
pub struct MetricBooks {
    /// 命中：预测点/区间覆盖实际帧的次数。
    hits: u64,
    /// 预测总次数（命中率分母）。
    preds: u64,
    bias_hist: [u64; BIAS_BUCKETS],
    fallbacks: u64,
    /// 预留总量与实际消耗量（浪费率分子=预留−消耗 的累计）。
    reserved_pm_total: u64,
    consumed_pm_total: u64,
    /// 开销千分位样本（滚动）。
    overhead_hist: Vec<u32>,
}

impl MetricBooks {
    pub fn new() -> Self {
        MetricBooks {
            hits: 0,
            preds: 0,
            bias_hist: [0; BIAS_BUCKETS],
            fallbacks: 0,
            reserved_pm_total: 0,
            consumed_pm_total: 0,
            overhead_hist: Vec::new(),
        }
    }

    /// 记一次预测结果：实际值是否落在预测区间 + 偏差入桶。
    pub fn record_outcome(&mut self, predicted_pm: u32, low_pm: u32, high_pm: u32, actual_pm: u32) -> Result<(), u16> {
        if low_pm > high_pm || high_pm > 10_000 {
            return Err(E_SAMPLE_MISMATCH);
        }
        self.preds += 1;
        if actual_pm >= low_pm && actual_pm <= high_pm {
            self.hits += 1;
        }
        let abs_bias = if actual_pm > predicted_pm { actual_pm - predicted_pm } else { predicted_pm - actual_pm };
        self.bias_hist[bias_bucket(abs_bias)] += 1;
        Ok(())
    }

    /// 记一次回退。
    pub fn record_fallback(&mut self) {
        self.fallbacks += 1;
    }

    /// 记一笔预留与消耗（千分位）。
    pub fn record_reservation(&mut self, reserved_pm: u32, consumed_pm: u32) -> Result<(), u16> {
        if consumed_pm > reserved_pm {
            return Err(E_SAMPLE_MISMATCH);
        }
        self.reserved_pm_total += reserved_pm as u64;
        self.consumed_pm_total += consumed_pm as u64;
        Ok(())
    }

    /// 记一笔开销实测（千分位）。
    pub fn record_overhead(&mut self, permille: u32) {
        self.overhead_hist.push(permille);
        if self.overhead_hist.len() > 64 {
            self.overhead_hist.remove(0);
        }
    }

    pub fn hit_rate_permille(&self) -> Option<u32> {
        if self.preds == 0 {
            None
        } else {
            Some(((self.hits * 1000) / self.preds) as u32)
        }
    }

    pub fn waste_rate_permille(&self) -> Option<u32> {
        if self.reserved_pm_total == 0 {
            None
        } else {
            let waste = self.reserved_pm_total - self.consumed_pm_total;
            Some(((waste * 1000) / self.reserved_pm_total) as u32)
        }
    }

    pub fn overhead_mean_permille(&self) -> Option<u32> {
        if self.overhead_hist.is_empty() {
            return None;
        }
        let s: u64 = self.overhead_hist.iter().map(|&v| v as u64).sum();
        Some((s / self.overhead_hist.len() as u64) as u32)
    }

    pub fn bias_hist(&self) -> &[u64; BIAS_BUCKETS] {
        &self.bias_hist
    }

    pub fn fallbacks(&self) -> u64 {
        self.fallbacks
    }
}

impl Default for MetricBooks {
    fn default() -> Self {
        Self::new()
    }
}

/// B08 总线快照包（定长 5 槽——判据「五指标进总线」的物理格式）。
/// 每槽 = (有效标志, 数值)。无效 = 账本尚无样本（不补零，诚实缺报）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct B08Snapshot {
    pub slots: [(bool, u32); METRIC_SLOTS],
    pub seq: u64,
}

impl B08Snapshot {
    pub fn slot(&self, id: MetricId) -> Option<u32> {
        let s = self.slots[id as usize];
        if s.0 {
            Some(s.1)
        } else {
            None
        }
    }

    pub fn valid_count(&self) -> usize {
        self.slots.iter().filter(|s| s.0).count()
    }
}

/// B08 发布器：账本 → 快照包（seq 单调递增——消费者可检测丢包）。
pub struct B08Publisher {
    seq: u64,
    last: Option<B08Snapshot>,
}

impl B08Publisher {
    pub fn new() -> Self {
        B08Publisher { seq: 0, last: None }
    }

    /// 打快照并发布（seq 自增）。
    pub fn publish(&mut self, books: &MetricBooks) -> B08Snapshot {
        self.seq += 1;
        let snap = B08Snapshot {
            slots: [
                (books.hit_rate_permille().is_some(), books.hit_rate_permille().unwrap_or(0)),
                (books.preds > 0, books.bias_hist().iter().sum::<u64>() as u32),
                (true, books.fallbacks().min(u32::MAX as u64) as u32),
                (books.waste_rate_permille().is_some(), books.waste_rate_permille().unwrap_or(0)),
                (books.overhead_mean_permille().is_some(), books.overhead_mean_permille().unwrap_or(0)),
            ],
            seq: self.seq,
        };
        self.last = Some(snap);
        snap
    }

    pub fn last(&self) -> Option<&B08Snapshot> {
        self.last.as_ref()
    }
}

impl Default for B08Publisher {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 判据自检
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a80_五指标_账本读数() {
        let mut b = MetricBooks::new();
        // 空账本诚实缺报。
        assert_eq!(b.hit_rate_permille(), None);
        assert_eq!(b.waste_rate_permille(), None);
        assert_eq!(b.overhead_mean_permille(), None);
        // 命中 3/4。
        assert!(b.record_outcome(4_000, 3_500, 4_500, 4_200).is_ok());
        assert!(b.record_outcome(4_000, 3_500, 4_500, 3_600).is_ok());
        assert!(b.record_outcome(4_000, 3_500, 4_500, 4_400).is_ok());
        assert!(b.record_outcome(4_000, 3_500, 4_500, 6_000).is_ok());
        assert_eq!(b.hit_rate_permille(), Some(750));
        // 偏差入桶（左闭右开界）：200→桶3、400→桶4(×2)、2000→桶6。
        assert_eq!(b.bias_hist()[3], 1);
        assert_eq!(b.bias_hist()[4], 2);
        assert_eq!(b.bias_hist()[6], 1);
        // 浪费率：预留 800 消耗 600 → 25%。
        assert!(b.record_reservation(800, 600).is_ok());
        assert_eq!(b.waste_rate_permille(), Some(250));
        // 消耗 > 预留必拒。
        assert_eq!(b.record_reservation(100, 200).unwrap_err(), E_SAMPLE_MISMATCH);
        // 回退与开销。
        b.record_fallback();
        b.record_overhead(4);
        assert_eq!(b.fallbacks(), 1);
        assert_eq!(b.overhead_mean_permille(), Some(4));
    }

    #[test]
    fn a80_偏差分桶_边界闭开() {
        assert_eq!(bias_bucket(0), 0);
        assert_eq!(bias_bucket(49), 0);
        assert_eq!(bias_bucket(50), 1);
        assert_eq!(bias_bucket(1_599), 5);
        assert_eq!(bias_bucket(1_600), 6);
        assert_eq!(bias_bucket(3_200), 7);
        assert_eq!(bias_bucket(u32::MAX), 7);
    }

    #[test]
    fn a80_进总线_快照包() {
        let mut b = MetricBooks::new();
        let mut p = B08Publisher::new();
        // 空账本发布：槽位诚实无效。
        let s0 = p.publish(&b);
        assert_eq!(s0.seq, 1);
        assert_eq!(s0.valid_count(), 1); // 只有回退计数槽恒有效(值为0)
        assert_eq!(s0.slot(MetricId::HitRate), None);
        // 有数据后全槽有效。
        assert!(b.record_outcome(5_000, 4_000, 6_000, 5_100).is_ok());
        assert!(b.record_reservation(500, 400).is_ok());
        b.record_overhead(6);
        let s1 = p.publish(&b);
        assert_eq!(s1.seq, 2);
        assert_eq!(s1.valid_count(), METRIC_SLOTS);
        assert_eq!(s1.slot(MetricId::HitRate), Some(1000));
        assert_eq!(s1.slot(MetricId::FallbackCount), Some(0));
        assert_eq!(s1.slot(MetricId::WasteRate), Some(200));
        assert_eq!(s1.slot(MetricId::Overhead), Some(6));
        // seq 单调（丢包可检测）。
        let s2 = p.publish(&b);
        assert_eq!(s2.seq, s1.seq + 1);
        assert!(p.last().is_some());
        // 槽号枚举与槽位数一致。
        assert_eq!(MetricId::Overhead as usize, METRIC_SLOTS - 1);
    }
}
