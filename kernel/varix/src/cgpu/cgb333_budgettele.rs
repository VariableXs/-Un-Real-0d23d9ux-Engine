// CGPU-F0333 预算遥测 · cgb333_budgettele.rs
// AI-MIMO-1022 · CGPU-C 预算收口
// 判据：预算使用率/超支次数/交易量/公平性指标遥测；实时可查；
//       聚合窗口可配；告警阈值联动。
#![allow(dead_code)]

extern crate alloc;
use alloc::vec::Vec;

use super::super::checks::CheckSet;

/// 预算遥测指标。
#[derive(Clone, Copy, Default, Debug)]
pub struct BudgetTelemetry {
    pub usage_q8: u16,
    pub over_budget_count: u32,
    pub transactions: u32,
    pub jain_fairness_q8: u16,
    pub peak_usage_q8: u16,
    pub mean_usage_q8: u16,
    pub samples: u64,
}

/// 聚合窗口。
#[derive(Clone, Copy, Debug)]
pub struct AggWindow {
    pub size_ms: u32,
    pub buckets: u32,
    pub current: u32,
}

/// 告警级别。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AlertLevel {
    Normal,
    Warning,
    Critical,
}

/// 告警记录。
#[derive(Clone, Copy, Debug)]
pub struct BudgetAlert {
    pub level: AlertLevel,
    pub metric_id: u32,
    pub value_q8: u16,
    pub threshold_q8: u16,
    pub frame: u64,
}

/// 预算遥测引擎。
pub struct BudgetTelemetryEngine {
    pub current: BudgetTelemetry,
    pub window: AggWindow,
    pub alerts: Vec<BudgetAlert>,
    pub warn_threshold_q8: u16,
    pub crit_threshold_q8: u16,
    pub samples: Vec<u16>,
    pub max_samples: usize,
}

impl BudgetTelemetryEngine {
    pub fn new(window_ms: u32, buckets: u32) -> Self {
        Self {
            current: BudgetTelemetry::default(),
            window: AggWindow {
                size_ms: window_ms,
                buckets,
                current: 0,
            },
            alerts: Vec::new(),
            warn_threshold_q8: 205, // 80%
            crit_threshold_q8: 243, // 95%
            samples: Vec::new(),
            max_samples: 4096,
        }
    }

    /// 记录使用率样本。
    pub fn record(&mut self, usage_q8: u16, frame: u64) {
        self.current.samples += 1;
        self.current.usage_q8 = usage_q8;
        if usage_q8 > self.current.peak_usage_q8 {
            self.current.peak_usage_q8 = usage_q8;
        }
        if usage_q8 > 256 {
            self.current.over_budget_count += 1;
        }
        if self.samples.len() >= self.max_samples {
            self.samples.remove(0);
        }
        self.samples.push(usage_q8);
        // 均值更新
        let sum: u32 = self.samples.iter().map(|s| *s as u32).sum();
        self.current.mean_usage_q8 = (sum / self.samples.len() as u32) as u16;
        // 告警
        if usage_q8 >= self.crit_threshold_q8 {
            self.alerts.push(BudgetAlert {
                level: AlertLevel::Critical,
                metric_id: 1,
                value_q8: usage_q8,
                threshold_q8: self.crit_threshold_q8,
                frame,
            });
        } else if usage_q8 >= self.warn_threshold_q8 {
            self.alerts.push(BudgetAlert {
                level: AlertLevel::Warning,
                metric_id: 1,
                value_q8: usage_q8,
                threshold_q8: self.warn_threshold_q8,
                frame,
            });
        }
    }

    /// Jain 公平性指数（Q8）。
    pub fn compute_jain(allocations: &[u32]) -> u16 {
        if allocations.is_empty() {
            return 256;
        }
        let n = allocations.len() as u64;
        let sum: u64 = allocations.iter().map(|a| *a as u64).sum();
        let sum_sq: u64 = allocations.iter().map(|a| (*a as u64) * (*a as u64)).sum();
        if sum_sq == 0 {
            return 256;
        }
        // Jain = (sum)^2 / (n * sum_sq)
        ((sum * sum * 256) / (n * sum_sq)) as u16
    }

    pub fn set_jain(&mut self, allocations: &[u32]) {
        self.current.jain_fairness_q8 = Self::compute_jain(allocations);
    }

    /// 记交易。
    pub fn record_transaction(&mut self) {
        self.current.transactions += 1;
    }

    /// 窗口推进。
    pub fn advance_window(&mut self) {
        self.window.current = (self.window.current + 1) % self.window.buckets.max(1);
    }

    /// 告警统计。
    pub fn alert_count(&self, level: AlertLevel) -> u32 {
        self.alerts.iter().filter(|a| a.level == level).count() as u32
    }

    pub fn run_checks(&self) -> CheckSet {
        let mut cs = CheckSet::new("CGPU-F0333 预算遥测");
        cs.add("使用率 ≤ 256 或超支计数", {
            self.current.usage_q8 <= 256 || self.current.over_budget_count > 0
        }, "usage domain");
        cs.add("峰值 ≥ 均值", {
            self.current.peak_usage_q8 >= self.current.mean_usage_q8 || self.current.samples == 0
        }, "peak ≥ mean");
        cs.add("Jain ∈ [0,256]", self.current.jain_fairness_q8 <= 256, "jain");
        cs.add("样本不超容量", self.samples.len() <= self.max_samples, "≤max");
        cs.add("告警阈值有序", self.warn_threshold_q8 < self.crit_threshold_q8, "warn < crit");
        cs.add("窗口桶正", self.window.buckets > 0, "buckets > 0");
        cs.add("交易计数非负", self.current.transactions <= u32::MAX, "domain");
        cs
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_and_alert() {
        let mut e = BudgetTelemetryEngine::new(1000, 10);
        e.record(100, 1);
        e.record(220, 2); // warning
        e.record(250, 3); // critical
        assert_eq!(e.alert_count(AlertLevel::Warning), 1);
        assert_eq!(e.alert_count(AlertLevel::Critical), 1);
    }

    #[test]
    fn jain_fairness() {
        assert_eq!(BudgetTelemetryEngine::compute_jain(&[100, 100, 100]), 256);
        let unfair = BudgetTelemetryEngine::compute_jain(&[100, 0, 0]);
        assert!(unfair < 128);
    }
}
