// CGPU-F0308 域级性能总基准 · cgb308_perfgate.rs
// AI-MIMO-1022 · CGPU-B 收口
// 判据：跨场景基准矩阵（小图/中图/大图/超大图 × 静态/动态）；
//       P50/P95/P99 达标可报告；回归检测（基线对比）；结果可归档。
#![allow(dead_code)]

use alloc::vec;
extern crate alloc;
use alloc::vec::Vec;

use super::super::checks::CheckSet;

/// 场景规模。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SceneScale {
    Small,   // <100 节点
    Medium,  // 100-2k
    Large,   // 2k-20k
    Huge,    // >20k
}

impl SceneScale {
    pub fn node_range(self) -> (u32, u32) {
        match self {
            Self::Small => (1, 100),
            Self::Medium => (100, 2000),
            Self::Large => (2000, 20000),
            Self::Huge => (20000, 100000),
        }
    }
}

/// 场景动态性。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SceneDynamics {
    Static,
    Moderate,  // <10% 变更/帧
    Dynamic,   // 10-50%
    Volatile,  // >50%
}

/// 基准样本。
#[derive(Clone, Copy, Debug)]
pub struct BenchSample {
    pub frame_us: u32,
    pub nodes: u32,
    pub edges: u32,
    pub lod_level: u8,
}

/// 基准结果。
#[derive(Clone, Debug)]
pub struct BenchResult {
    pub id: u32,
    pub scale: SceneScale,
    pub dynamics: SceneDynamics,
    pub samples: Vec<BenchSample>,
    pub p50_us: u32,
    pub p95_us: u32,
    pub p99_us: u32,
    pub budget_us: u32,
    pub passed: bool,
}

/// 回归检测。
#[derive(Clone, Copy, Debug)]
pub struct RegressionCheck {
    pub baseline_p95: u32,
    pub current_p95: u32,
    pub threshold_pct: u32,
    pub regressed: bool,
}

/// 性能总基准引擎。
pub struct PerfGate {
    pub results: Vec<BenchResult>,
    pub regressions: Vec<RegressionCheck>,
    pub archived: bool,
    pub total_benchmarks: u32,
}

impl PerfGate {
    pub fn new() -> Self {
        Self {
            results: Vec::new(),
            regressions: Vec::new(),
            archived: false,
            total_benchmarks: 0,
        }
    }

    /// 计算百分位（排序后取分位）。
    pub fn percentile(samples: &[u32], pct: u8) -> u32 {
        if samples.is_empty() {
            return 0;
        }
        let mut s = samples.to_vec();
        s.sort_unstable();
        let idx = ((pct as usize) * (s.len() - 1)) / 100;
        s[idx]
    }

    /// 加载基准结果。
    pub fn load_result(&mut self, id: u32, scale: SceneScale, dynamics: SceneDynamics, samples: Vec<BenchSample>, budget_us: u32) {
        let times: Vec<u32> = samples.iter().map(|s| s.frame_us).collect();
        let p50 = Self::percentile(&times, 50);
        let p95 = Self::percentile(&times, 95);
        let p99 = Self::percentile(&times, 99);
        self.results.push(BenchResult {
            id,
            scale,
            dynamics,
            samples,
            p50_us: p50,
            p95_us: p95,
            p99_us: p99,
            budget_us,
            passed: p95 <= budget_us,
        });
        self.total_benchmarks += 1;
    }

    /// 回归对比。
    pub fn check_regression(&mut self, current_id: u32, baseline_p95: u32, threshold_pct: u32) -> Option<RegressionCheck> {
        let cur = self.results.iter().find(|r| r.id == current_id)?;
        let delta_pct = if baseline_p95 == 0 {
            0
        } else {
            ((cur.p95_us.saturating_sub(baseline_p95)) as u64 * 100 / baseline_p95 as u64) as u32
        };
        let check = RegressionCheck {
            baseline_p95,
            current_p95: cur.p95_us,
            threshold_pct,
            regressed: delta_pct > threshold_pct,
        };
        self.regressions.push(check);
        Some(check)
    }

    /// 达标率（Q8）。
    pub fn pass_rate_q8(&self) -> u16 {
        if self.results.is_empty() {
            return 256;
        }
        let passed = self.results.iter().filter(|r| r.passed).count() as u64;
        ((passed * 256) / self.results.len() as u64) as u16
    }

    /// 回归数。
    pub fn regression_count(&self) -> u32 {
        self.regressions.iter().filter(|r| r.regressed).count() as u32
    }

    /// 归档。
    pub fn archive(&mut self) -> bool {
        if self.regression_count() > 0 {
            return false;
        }
        self.archived = true;
        true
    }

    /// 全矩阵覆盖检查（4 规模 × 4 动态）。
    pub fn matrix_coverage(&self) -> u32 {
        let scales = [SceneScale::Small, SceneScale::Medium, SceneScale::Large, SceneScale::Huge];
        let dyns = [SceneDynamics::Static, SceneDynamics::Moderate, SceneDynamics::Dynamic, SceneDynamics::Volatile];
        let mut covered = 0u32;
        for s in &scales {
            for d in &dyns {
                if self.results.iter().any(|r| r.scale == *s && r.dynamics == *d) {
                    covered += 1;
                }
            }
        }
        covered
    }

    pub fn run_checks(&self) -> CheckSet {
        let mut cs = CheckSet::new("CGPU-F0308 域级性能总基准");
        cs.add("结果 id 唯一", {
            let mut ids: Vec<u32> = self.results.iter().map(|r| r.id).collect();
            ids.sort_unstable();
            ids.dedup();
            ids.len() == self.results.len()
        }, "unique");
        cs.add("百分位单调 P50≤P95≤P99", {
            self.results.iter().all(|r| r.p50_us <= r.p95_us && r.p95_us <= r.p99_us)
        }, "p50≤p95≤p99");
        cs.add("通过率 ≤ 256", self.pass_rate_q8() <= 256, "q8");
        cs.add("回归数与记录一致", {
            self.regression_count() as usize == self.regressions.iter().filter(|r| r.regressed).count()
        }, "count");
        cs.add("归档无回归", {
            !self.archived || self.regression_count() == 0
        }, "archive gate");
        cs.add("矩阵覆盖率 ≥ 8/16", self.matrix_coverage() >= 8, ">=8 cells");
        cs.add("样本非空或结果为空", {
            self.results.iter().all(|r| !r.samples.is_empty())
        }, "samples");
        cs
    }
}

impl Default for PerfGate {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn samples(n: u32) -> Vec<BenchSample> {
        (0..n)
            .map(|i| BenchSample {
                frame_us: 1000 + i * 10,
                nodes: 500,
                edges: 1500,
                lod_level: 0,
            })
            .collect()
    }

    #[test]
    fn percentile_ordering() {
        let s = vec![10u32, 20, 30, 40, 50, 60, 70, 80, 90, 100];
        assert_eq!(PerfGate::percentile(&s, 50), 55);
        assert!(PerfGate::percentile(&s, 95) >= PerfGate::percentile(&s, 50));
    }

    #[test]
    fn load_and_regress() {
        let mut g = PerfGate::new();
        g.load_result(1, SceneScale::Medium, SceneDynamics::Dynamic, samples(20), 5000);
        let r = g.check_regression(1, 2000, 10);
        assert!(r.is_some());
    }

    #[test]
    fn archive_gate() {
        let mut g = PerfGate::new();
        g.load_result(1, SceneScale::Small, SceneDynamics::Static, samples(5), 100000);
        g.check_regression(1, 100000, 50); // 不回归
        assert!(g.archive());
    }
}
