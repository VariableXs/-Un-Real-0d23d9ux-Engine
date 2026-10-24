// CGPU-F0349 预算对比 · cgb349_compare.rs
// AI-MIMO-1022 · CGPU-C 预算收口
// 判据：两预算方案对比（分配/使用率/公平性/超支次数）；
//       对比报告可导出；优胜者判定；差异显著性。
#![allow(dead_code)]

use alloc::format;
extern crate alloc;
use alloc::vec::Vec;
use alloc::string::String;

use super::super::checks::CheckSet;

/// 预算方案指标。
#[derive(Clone, Copy, Default, Debug)]
pub struct SchemeMetrics {
    pub total_allocated: u64,
    pub total_consumed: u64,
    pub peak_usage_q8: u16,
    pub mean_usage_q8: u16,
    pub jain_fairness_q8: u16,
    pub over_budget_count: u32,
    pub subjects: u32,
}

/// 对比结果。
#[derive(Clone, Copy, Debug)]
pub struct CompareResult {
    pub winner: u8, // 0=tie 1=a 2=b
    pub score_a_q8: u16,
    pub score_b_q8: u16,
    pub delta_usage_pct_q8: i32,
    pub delta_fairness_q8: i32,
    pub significant: bool,
}

/// 预算对比引擎。
pub struct BudgetComparator {
    pub scheme_a: SchemeMetrics,
    pub scheme_b: SchemeMetrics,
    pub results: Vec<CompareResult>,
    pub weight_usage_q8: u16,
    pub weight_fairness_q8: u16,
    pub weight_overbudget_q8: u16,
}

impl BudgetComparator {
    pub fn new() -> Self {
        Self {
            scheme_a: SchemeMetrics::default(),
            scheme_b: SchemeMetrics::default(),
            results: Vec::new(),
            weight_usage_q8: 80,
            weight_fairness_q8: 128,
            weight_overbudget_q8: 48,
        }
    }

    pub fn set_schemes(&mut self, a: SchemeMetrics, b: SchemeMetrics) {
        self.scheme_a = a;
        self.scheme_b = b;
    }

    /// 评分（使用率低+公平性高+超支少 = 高分）。
    fn score(&self, m: &SchemeMetrics) -> u16 {
        let usage_score = if m.peak_usage_q8 == 0 { 256 } else { 256u32.saturating_sub(m.peak_usage_q8 as u32) };
        let fairness_score = m.jain_fairness_q8 as u32;
        let overbudget_score = 256u32.saturating_sub(m.over_budget_count * 16);
        let total_weight = self.weight_usage_q8 as u32 + self.weight_fairness_q8 as u32 + self.weight_overbudget_q8 as u32;
        if total_weight == 0 {
            return 0;
        }
        ((usage_score * self.weight_usage_q8 as u32
            + fairness_score * self.weight_fairness_q8 as u32
            + overbudget_score * self.weight_overbudget_q8 as u32)
            / total_weight) as u16
    }

    /// 执行对比。
    pub fn compare(&mut self) -> CompareResult {
        let score_a = self.score(&self.scheme_a);
        let score_b = self.score(&self.scheme_b);
        let delta_usage = self.scheme_b.peak_usage_q8 as i32 - self.scheme_a.peak_usage_q8 as i32;
        let delta_fairness = self.scheme_b.jain_fairness_q8 as i32 - self.scheme_a.jain_fairness_q8 as i32;
        let diff = (score_a as i32 - score_b as i32).abs();
        let result = CompareResult {
            winner: if diff < 8 {
                0
            } else if score_a > score_b {
                1
            } else {
                2
            },
            score_a_q8: score_a,
            score_b_q8: score_b,
            delta_usage_pct_q8: delta_usage,
            delta_fairness_q8: delta_fairness,
            significant: diff >= 16,
        };
        self.results.push(result);
        result
    }

    /// 对比报告（人话）。
    pub fn report(&self) -> String {
        let mut s = String::from("预算方案对比：");
        s.push_str(&alloc::format!("A({}/256) vs B({}/256)", self.score(&self.scheme_a), self.score(&self.scheme_b)));
        if let Some(r) = self.results.last() {
            match r.winner {
                1 => s.push_str(" | A 胜"),
                2 => s.push_str(" | B 胜"),
                _ => s.push_str(" | 平局"),
            }
        }
        s
    }

    /// 消耗率（Q8）。
    pub fn consumption_rate(&self, m: &SchemeMetrics) -> u16 {
        if m.total_allocated == 0 {
            return 0;
        }
        ((m.total_consumed * 256) / m.total_allocated) as u16
    }

    pub fn run_checks(&self) -> CheckSet {
        let mut cs = CheckSet::new("CGPU-F0349 预算对比");
        cs.add("权重非全零", {
            self.weight_usage_q8 + self.weight_fairness_q8 + self.weight_overbudget_q8 > 0
        }, "weights");
        cs.add("评分 ≤ 256", {
            self.score(&self.scheme_a) <= 256 && self.score(&self.scheme_b) <= 256
        }, "score");
        cs.add("优胜者 ∈ {0,1,2}", {
            self.results.iter().all(|r| r.winner <= 2)
        }, "winner");
        cs.add("公平性 ≤ 256", {
            self.scheme_a.jain_fairness_q8 <= 256 && self.scheme_b.jain_fairness_q8 <= 256
        }, "jain");
        cs.add("对比结果非空或已调用", {
            !self.results.is_empty() || self.scheme_a.subjects == 0
        }, "called");
        cs.add("显著性判定合理", {
            self.results.iter().all(|r| {
                let diff = (r.score_a_q8 as i32 - r.score_b_q8 as i32).abs();
                r.significant == (diff >= 16)
            })
        }, "significance");
        cs
    }
}

impl Default for BudgetComparator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn better_scheme_wins() {
        let mut c = BudgetComparator::new();
        c.set_schemes(
            SchemeMetrics {
                peak_usage_q8: 200,
                jain_fairness_q8: 128,
                over_budget_count: 5,
                ..Default::default()
            },
            SchemeMetrics {
                peak_usage_q8: 150,
                jain_fairness_q8: 256,
                over_budget_count: 0,
                ..Default::default()
            },
        );
        let r = c.compare();
        assert_eq!(r.winner, 2); // B 更好
    }
}
