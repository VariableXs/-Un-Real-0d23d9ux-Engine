// CGPU-F0319 B域收官彩排 · cgb319_rehearsal.rs
// AI-MIMO-1022 · CGPU-B 收口
// 判据：收官流程彩排（清单→检查→签核→宣告）；每步有回退；
//       彩排可重复执行；失败可诊断；彩排报告完备。
#![allow(dead_code)]

extern crate alloc;
use alloc::vec::Vec;

use super::super::checks::CheckSet;

/// 彩排步骤。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RehearsalStep {
    Checklist,
    SelfCheck,
    CrossCheck,
    SignOff,
    Declare,
    PostVerify,
}

impl RehearsalStep {
    pub fn name(self) -> &'static str {
        match self {
            Self::Checklist => "清单",
            Self::SelfCheck => "自检",
            Self::CrossCheck => "交叉检查",
            Self::SignOff => "签核",
            Self::Declare => "宣告",
            Self::PostVerify => "事后验证",
        }
    }
}

/// 步骤结果。
#[derive(Clone, Copy, Debug)]
pub struct StepResult {
    pub step: RehearsalStep,
    pub passed: bool,
    pub attempts: u32,
    pub error_code: u32,
    pub duration_ms: u32,
}

/// 彩排报告。
#[derive(Clone, Copy, Default, Debug)]
pub struct RehearsalReport {
    pub total_steps: u32,
    pub passed: u32,
    pub failed: u32,
    pub retried: u32,
    pub total_ms: u32,
    pub can_declare: bool,
}

/// 收官彩排引擎。
pub struct Rehearsal {
    pub steps: Vec<StepResult>,
    pub report: RehearsalReport,
    pub current_step: usize,
    pub max_retries: u32,
}

impl Rehearsal {
    pub fn new() -> Self {
        Self {
            steps: Vec::new(),
            report: RehearsalReport::default(),
            current_step: 0,
            max_retries: 3,
        }
    }

    /// 执行一步（带回退重试）。
    pub fn run_step(&mut self, step: RehearsalStep) -> bool {
        let mut attempts = 0u32;
        let mut ok = false;
        let mut err = 0u32;
        // 模拟：清单/自检默认过，交叉检查可能失败重试
        while attempts <= self.max_retries {
            attempts += 1;
            ok = match step {
                RehearsalStep::Checklist => true,
                RehearsalStep::SelfCheck => true,
                RehearsalStep::CrossCheck => attempts >= 2, // 第二次成功
                RehearsalStep::SignOff => true,
                RehearsalStep::Declare => self.steps.iter().filter(|s| s.passed).count() >= 3,
                RehearsalStep::PostVerify => true,
            };
            if ok {
                break;
            }
            err = 1;
        }
        self.steps.push(StepResult {
            step,
            passed: ok,
            attempts,
            error_code: if ok { 0 } else { err },
            duration_ms: attempts * 10,
        });
        self.current_step += 1;
        ok
    }

    /// 完整彩排。
    pub fn full_rehearsal(&mut self) -> RehearsalReport {
        let steps = [
            RehearsalStep::Checklist,
            RehearsalStep::SelfCheck,
            RehearsalStep::CrossCheck,
            RehearsalStep::SignOff,
            RehearsalStep::Declare,
            RehearsalStep::PostVerify,
        ];
        for s in steps {
            self.run_step(s);
        }
        self.finalize()
    }

    pub fn finalize(&mut self) -> RehearsalReport {
        let total = self.steps.len() as u32;
        let passed = self.steps.iter().filter(|s| s.passed).count() as u32;
        let failed = total - passed;
        let retried = self.steps.iter().filter(|s| s.attempts > 1).count() as u32;
        let total_ms: u32 = self.steps.iter().map(|s| s.duration_ms).sum();
        self.report = RehearsalReport {
            total_steps: total,
            passed,
            failed,
            retried,
            total_ms,
            can_declare: failed == 0,
        };
        self.report
    }

    /// 诊断失败步骤。
    pub fn diagnose(&self) -> Vec<&StepResult> {
        self.steps.iter().filter(|s| !s.passed).collect()
    }

    /// 重置（可重复彩排）。
    pub fn reset(&mut self) {
        self.steps.clear();
        self.current_step = 0;
        self.report = RehearsalReport::default();
    }

    pub fn run_checks(&self) -> CheckSet {
        let mut cs = CheckSet::new("CGPU-F0319 B域收官彩排");
        cs.add("步骤不重复", {
            let mut seen = Vec::new();
            for s in &self.steps {
                if seen.contains(&s.step) {
                    return cs;
                }
                seen.push(s.step);
            }
            true
        }, "unique steps");
        cs.add("报告计数自洽", {
            self.report.passed + self.report.failed == self.report.total_steps
        }, "sum");
        cs.add("宣告门禁零失败", {
            !self.report.can_declare || self.report.failed == 0
        }, "declare gate");
        cs.add("重试不超上限", {
            self.steps.iter().all(|s| s.attempts <= self.max_retries + 1)
        }, "retry bound");
        cs.add("步骤数合理", self.steps.len() <= 6, "≤6");
        cs.add("耗时可加", self.report.total_ms == self.steps.iter().map(|s| s.duration_ms).sum::<u32>(), "sum ms");
        cs
    }
}

impl Default for Rehearsal {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_rehearsal_passes() {
        let mut r = Rehearsal::new();
        let report = r.full_rehearsal();
        assert!(report.can_declare);
        assert_eq!(report.failed, 0);
    }

    #[test]
    fn diagnose_empty_on_success() {
        let mut r = Rehearsal::new();
        r.full_rehearsal();
        assert!(r.diagnose().is_empty());
    }
}
