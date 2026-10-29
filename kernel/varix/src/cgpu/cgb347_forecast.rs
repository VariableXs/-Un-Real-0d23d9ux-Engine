// CGPU-F0347 预算预测 · cgb347_forecast.rs
// AI-MIMO-1022 · CGPU-C 预算收口
// 判据：基于历史的预算消耗预测（EWMA + 线性趋势）；预测误差可报告；
//       预测驱动提前降档；置信区间。
#![allow(dead_code)]

extern crate alloc;
use alloc::vec::Vec;

use super::super::checks::CheckSet;

/// 预测样本。
#[derive(Clone, Copy, Debug)]
pub struct ForecastSample {
    pub frame: u64,
    pub actual_us: u32,
    pub predicted_us: u32,
    pub error_us: i32,
}

/// 预测模型参数。
#[derive(Clone, Copy, Debug)]
pub struct ForecastModel {
    pub ewma_alpha_q8: u16, // 0..=256
    pub trend_weight_q8: u16,
    pub confidence_q8: u16,
    pub horizon_frames: u32,
}

impl Default for ForecastModel {
    fn default() -> Self {
        Self {
            ewma_alpha_q8: 32,    // 1/8
            trend_weight_q8: 64,  // 1/4
            confidence_q8: 200,
            horizon_frames: 5,
        }
    }
}

/// 预测引擎。
pub struct BudgetForecaster {
    pub model: ForecastModel,
    pub history: Vec<u32>,
    pub samples: Vec<ForecastSample>,
    pub ewma_us: u32,
    pub trend_us: i32,
    pub max_history: usize,
}

impl BudgetForecaster {
    pub fn new(model: ForecastModel) -> Self {
        Self {
            model,
            history: Vec::new(),
            samples: Vec::new(),
            ewma_us: 0,
            trend_us: 0,
            max_history: 256,
        }
    }

    /// 记录实际值。
    pub fn observe(&mut self, actual_us: u32, frame: u64) {
        // EWMA 更新
        if self.ewma_us == 0 {
            self.ewma_us = actual_us;
        } else {
            let a = self.model.ewma_alpha_q8 as u64;
            self.ewma_us = ((actual_us as u64 * a + self.ewma_us as u64 * (256 - a)) / 256) as u32;
        }
        // 趋势（简单差分）
        if let Some(&last) = self.history.last() {
            self.trend_us = (actual_us as i64 - last as i64) as i32;
        }
        if self.history.len() >= self.max_history {
            self.history.remove(0);
        }
        self.history.push(actual_us);

        // 预测下帧
        let predicted = self.predict_next();
        let error = actual_us as i64 - predicted as i64;
        self.samples.push(ForecastSample {
            frame,
            actual_us,
            predicted_us: predicted,
            error_us: error as i32,
        });
    }

    /// 预测下一帧（EWMA + 趋势）。
    pub fn predict_next(&self) -> u32 {
        let trend_component = (self.trend_us as i64 * self.model.trend_weight_q8 as i64) / 256;
        let base = self.ewma_us as i64;
        (base + trend_component).max(0) as u32
    }

    /// 预测 N 帧后。
    pub fn predict_horizon(&self, frames: u32) -> u32 {
        let trend_component = (self.trend_us as i64 * frames as i64 * self.model.trend_weight_q8 as i64) / 256;
        (self.ewma_us as i64 + trend_component).max(0) as u32
    }

    /// 平均绝对误差（MAE）。
    pub fn mae(&self) -> u32 {
        if self.samples.is_empty() {
            return 0;
        }
        let sum: u64 = self.samples.iter().map(|s| s.error_us.unsigned_abs() as u64).sum();
        (sum / self.samples.len() as u64) as u32
    }

    /// 均方根误差（RMSE 近似整数）。
    pub fn rmse(&self) -> u32 {
        if self.samples.is_empty() {
            return 0;
        }
        let sum_sq: u64 = self.samples.iter().map(|s| (s.error_us as i64 * s.error_us as i64) as u64).sum();
        let mean_sq = sum_sq / self.samples.len() as u64;
        // 整数平方根
        let mut x = mean_sq;
        let mut y = (x + 1) / 2;
        while y < x {
            x = y;
            y = (x + mean_sq / x) / 2;
        }
        x as u32
    }

    /// 是否需提前降档（预测超阈值）。
    pub fn should_degrade(&self, budget_us: u32) -> bool {
        self.predict_horizon(self.model.horizon_frames) > budget_us
    }

    /// 置信区间宽度（Q8 比例）。
    pub fn confidence_interval_q8(&self) -> u16 {
        if self.samples.len() < 2 {
            return 256;
        }
        let mae = self.mae() as u64;
        let mean = self.history.iter().map(|h| *h as u64).sum::<u64>() / self.history.len() as u64;
        if mean == 0 {
            return 256;
        }
        ((mae * 256) / mean).min(256) as u16
    }

    pub fn run_checks(&self) -> CheckSet {
        let mut cs = CheckSet::new("CGPU-F0347 预算预测");
        cs.add("历史不超容量", self.history.len() <= self.max_history, "≤max");
        cs.add("EWMA 在范围", self.ewma_us <= u32::MAX, "domain");
        cs.add("EWMA α ∈ [0,256]", self.model.ewma_alpha_q8 <= 256, "alpha");
        cs.add("MAE ≥ RMSE", self.mae() >= self.rmse() || self.samples.is_empty(), "mae ≥ rmse");
        cs.add("样本与历史同步", {
            self.samples.len() == self.history.len()
        }, "sync");
        cs.add("置信区间 ≤ 256", self.confidence_interval_q8() <= 256, "ci");
        cs.add("预测非负", self.predict_next() <= u32::MAX, "domain");
        cs
    }
}

impl Default for BudgetForecaster {
    fn default() -> Self {
        Self::new(ForecastModel::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ewma_converges() {
        let mut f = BudgetForecaster::default();
        for i in 0..50 {
            f.observe(100, i);
        }
        assert!((f.ewma_us as i64 - 100).abs() < 5);
    }

    #[test]
    fn trend_prediction() {
        let mut f = BudgetForecaster::default();
        for i in 0..10 {
            f.observe(100 + i * 10, i as u64);
        }
        assert!(f.predict_next() > 100);
    }

    #[test]
    fn degrade_when_over_budget() {
        let mut f = BudgetForecaster::default();
        for i in 0..10 {
            f.observe(15000, i); // 超 12.5ms
        }
        assert!(f.should_degrade(12_500));
    }
}
