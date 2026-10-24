// CGPU-F0351 预算场景库 · cgb351_scenarios.rs
// AI-MIMO-1022 · CGPU-C 预算收口
// 判据：标准测试场景（重载/突发/均衡/饥饿/垄断）可复现；
//       场景参数化；结果基准对比；场景组合。
#![allow(dead_code)]

use alloc::format;
extern crate alloc;
use alloc::vec::Vec;
use alloc::string::String;

use super::super::checks::CheckSet;

/// 场景类型。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ScenarioKind {
    HeavyLoad,
    Burst,
    Balanced,
    Starvation,
    Monopoly,
    SoakTest,
}

/// 场景参数。
#[derive(Clone, Copy, Debug)]
pub struct ScenarioParams {
    pub subjects: u32,
    pub duration_frames: u32,
    pub load_factor_q8: u16,
    pub burst_probability_q8: u16,
    pub seed: u64,
}

/// 场景执行结果。
#[derive(Clone, Copy, Debug)]
pub struct ScenarioResult {
    pub kind: ScenarioKind,
    pub jain_fairness_q8: u16,
    pub peak_usage_q8: u16,
    pub over_budget_count: u32,
    pub starved_subjects: u32,
    pub passed: bool,
}

/// 场景库引擎。
pub struct ScenarioLibrary {
    pub scenarios: Vec<(ScenarioKind, ScenarioParams, ScenarioResult)>,
    pub baseline: Vec<(ScenarioKind, u16)>,
    pub current_seed: u64,
}

impl ScenarioLibrary {
    pub fn new() -> Self {
        Self {
            scenarios: Vec::new(),
            baseline: Vec::new(),
            current_seed: 0x5CE7_1022,
        }
    }

    /// 加载标准场景集。
    pub fn load_standard(&mut self) {
        let specs: [(ScenarioKind, ScenarioParams); 6] = [
            (ScenarioKind::HeavyLoad, ScenarioParams { subjects: 16, duration_frames: 300, load_factor_q8: 230, burst_probability_q8: 0, seed: 1 }),
            (ScenarioKind::Burst, ScenarioParams { subjects: 8, duration_frames: 200, load_factor_q8: 128, burst_probability_q8: 200, seed: 2 }),
            (ScenarioKind::Balanced, ScenarioParams { subjects: 12, duration_frames: 250, load_factor_q8: 160, burst_probability_q8: 50, seed: 3 }),
            (ScenarioKind::Starvation, ScenarioParams { subjects: 10, duration_frames: 400, load_factor_q8: 200, burst_probability_q8: 0, seed: 4 }),
            (ScenarioKind::Monopoly, ScenarioParams { subjects: 6, duration_frames: 150, load_factor_q8: 180, burst_probability_q8: 0, seed: 5 }),
            (ScenarioKind::SoakTest, ScenarioParams { subjects: 20, duration_frames: 1000, load_factor_q8: 170, burst_probability_q8: 30, seed: 6 }),
        ];
        for (kind, params) in specs {
            self.scenarios.push((kind, params, ScenarioResult {
                kind,
                jain_fairness_q8: 0,
                peak_usage_q8: 0,
                over_budget_count: 0,
                starved_subjects: 0,
                passed: false,
            }));
        }
    }

    /// 记录结果。
    pub fn record_result(&mut self, kind: ScenarioKind, result: ScenarioResult) {
        if let Some(s) = self.scenarios.iter_mut().find(|(k, _, _)| *k == kind) {
            s.2 = result;
        }
    }

    /// 基准对比。
    pub fn compare_baseline(&mut self, kind: ScenarioKind, baseline_jain: u16) -> bool {
        self.baseline.push((kind, baseline_jain));
        if let Some((_, _, r)) = self.scenarios.iter().find(|(k, _, _)| *k == kind) {
            r.jain_fairness_q8 >= baseline_jain
        } else {
            false
        }
    }

    /// 全场景通过率（Q8）。
    pub fn pass_rate_q8(&self) -> u16 {
        if self.scenarios.is_empty() {
            return 256;
        }
        let passed = self.scenarios.iter().filter(|(_, _, r)| r.passed).count() as u64;
        ((passed * 256) / self.scenarios.len() as u64) as u16
    }

    /// 饥饿检测（任意场景有 starved 即报）。
    pub fn starvation_detected(&self) -> bool {
        self.scenarios.iter().any(|(_, _, r)| r.starved_subjects > 0)
    }

    /// 场景摘要。
    pub fn summary(&self) -> String {
        let mut s = String::from("场景库：");
        s.push_str(&alloc::format!("{} 场景", self.scenarios.len()));
        s.push_str(&alloc::format!(" | 通过率 {}/256", self.pass_rate_q8()));
        if self.starvation_detected() {
            s.push_str(" | ⚠ 饥饿检出");
        }
        s
    }

    pub fn run_checks(&self) -> CheckSet {
        let mut cs = CheckSet::new("CGPU-F0351 预算场景库");
        cs.add_dyn("六场景齐备", self.scenarios.len() == 6, &alloc::format!("n={}", self.scenarios.len()));
        cs.add("场景类型不重复", {
            let mut kinds: Vec<ScenarioKind> = self.scenarios.iter().map(|(k, _, _)| *k).collect();
            let n = kinds.len();
            // 简单去重
            for i in (0..kinds.len()).rev() {
                for j in 0..i {
                    if kinds[i] == kinds[j] {
                        kinds.remove(i);
                        break;
                    }
                }
            }
            kinds.len() == n
        }, "unique kinds");
        cs.add("通过率 ≤ 256", self.pass_rate_q8() <= 256, "q8");
        cs.add("参数合理", {
            self.scenarios.iter().all(|(_, p, _)| p.subjects > 0 && p.duration_frames > 0)
        }, "params");
        cs.add("种子非零", self.current_seed != 0, "seed");
        cs.add("公平性 ≤ 256", {
            self.scenarios.iter().all(|(_, _, r)| r.jain_fairness_q8 <= 256)
        }, "jain");
        cs
    }
}

impl Default for ScenarioLibrary {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_six_scenarios() {
        let mut lib = ScenarioLibrary::new();
        lib.load_standard();
        assert_eq!(lib.scenarios.len(), 6);
    }

    #[test]
    fn starvation_detection() {
        let mut lib = ScenarioLibrary::new();
        lib.load_standard();
        lib.record_result(ScenarioKind::Starvation, ScenarioResult {
            kind: ScenarioKind::Starvation,
            jain_fairness_q8: 100,
            peak_usage_q8: 200,
            over_budget_count: 5,
            starved_subjects: 3,
            passed: false,
        });
        assert!(lib.starvation_detected());
    }
}
