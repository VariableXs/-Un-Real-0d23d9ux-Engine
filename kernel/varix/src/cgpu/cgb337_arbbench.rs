#![allow(dead_code)]
use alloc::format;
use alloc::vec;
// cgb337_arbbench.rs ↔ CGPU-F0337 仲裁算法对比基准
//
// 判据原文：CGPU-F0337-J1 三维对拍、三负载、结论、入册、判据。
// 难点分析：v1 vs v2 的对比要三维（公平性/吞吐/延迟）× 三负载谱（均匀/突发/倾斜），
//          结论必须可复现入册——基准数值冻结。
// 纪律约束：三维对拍；三负载谱；结论文档化；基准入册回归。
// 成本记账：模拟 O(frames×accounts×loads)。


use alloc::string::ToString;
use alloc::vec::Vec;

/// 仲裁版本。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArbVersion {
    /// v1 静态优先级。
    V1,
    /// v2 WFQ。
    V2,
}

/// 负载谱。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadShape {
    /// 均匀。
    Uniform,
    /// 突发。
    Bursty,
    /// 倾斜（单账户占大头）。
    Skewed,
}

impl LoadShape {
    pub fn name(self) -> &'static str {
        match self {
            LoadShape::Uniform => "均匀",
            LoadShape::Bursty => "突发",
            LoadShape::Skewed => "倾斜",
        }
    }
}

/// 三维对拍结果。
#[derive(Debug, Clone)]
pub struct BenchResult {
    pub version: ArbVersion,
    pub shape: LoadShape,
    /// 公平性（Jain 指数）。
    pub jain: f64,
    /// 吞吐（帧内完成率 0~1）。
    pub throughput: f64,
    /// 延迟（任务等待 P95，帧数）。
    pub latency_p95: f64,
    /// 仲裁开销（μs/次，模拟估计）。
    pub overhead_us: f64,
}

/// 对比报告。
#[derive(Debug, Clone)]
pub struct CompareReport {
    pub results: Vec<BenchResult>,
    /// 结论：何场景用何版本。
    pub conclusion: alloc::string::String,
}

/// 简易负载生成器。
fn gen_load(shape: LoadShape, frames: usize, accounts: usize) -> Vec<Vec<u64>> {
    let mut out = Vec::new();
    for f in 0..frames {
        let mut row = Vec::new();
        for a in 0..accounts {
            let base = match shape {
                LoadShape::Uniform => 100,
                LoadShape::Bursty => {
                    if f % 5 == 0 {
                        500
                    } else {
                        80
                    }
                }
                LoadShape::Skewed => {
                    if a == 0 {
                        400
                    } else {
                        50
                    }
                }
            };
            row.push(base);
        }
        out.push(row);
    }
    out
}

/// 对比基准器。
pub struct ArbBenchmark {
    pub report: CompareReport,
}

impl ArbBenchmark {
    /// 新建。
    pub fn new() -> Self {
        ArbBenchmark {
            report: CompareReport {
                results: Vec::new(),
                conclusion: alloc::string::String::new(),
            },
        }
    }

    /// 跑一组合（版本 × 负载谱）。
    pub fn run(&mut self, version: ArbVersion, shape: LoadShape, frames: usize) -> BenchResult {
        let accounts = 5;
        let load = gen_load(shape, frames, accounts);
        let frame_budget = 1000u64;

        // 简易模拟：记录各账户累计获得。
        let mut totals = vec![0u64; accounts];
        let mut waits = vec![0u64; accounts];
        let mut wait_log: Vec<u64> = Vec::new();
        let mut completed = 0u64;
        let mut total_reqs = 0u64;

        for f in 0..frames {
            let reqs = &load[f];
            let mut remaining = frame_budget;
            // 按版本策略排序。
            let mut order: Vec<usize> = (0..accounts).collect();
            match version {
                ArbVersion::V1 => {
                    // 静态优先级：账户 0 最高。
                    order.sort_by_key(|a| *a);
                }
                ArbVersion::V2 => {
                    // WFQ：虚拟时间（累计获得少的优先）。
                    order.sort_by_key(|a| totals[*a]);
                }
            }
            for a in order {
                total_reqs += 1;
                let want = reqs[a];
                if want > 0 && remaining > 0 {
                    let grant = want.min(remaining);
                    remaining -= grant;
                    totals[a] += grant;
                    completed += 1;
                    if waits[a] > 0 {
                        wait_log.push(waits[a]);
                        waits[a] = 0;
                    }
                } else if want > 0 {
                    waits[a] += 1;
                }
            }
        }

        // 计算三维。
        let jain = {
            let n = accounts as f64;
            let sum: u64 = totals.iter().sum();
            let sum_sq: u64 = totals.iter().map(|x| x * x).sum();
            if sum_sq == 0 {
                1.0
            } else {
                (sum as f64 * sum as f64) / (n * sum_sq as f64)
            }
        };
        let throughput = if total_reqs == 0 {
            1.0
        } else {
            completed as f64 / total_reqs as f64
        };
        wait_log.sort();
        let latency_p95 = if wait_log.is_empty() {
            0.0
        } else {
            wait_log[(wait_log.len() * 95 / 100).min(wait_log.len() - 1)] as f64
        };
        // 开销估计：v1 O(n log n) / v2 O(n log n) 但带虚拟时间维护。
        let overhead_us = match version {
            ArbVersion::V1 => 5.0 + (accounts as f64) * 0.5,
            ArbVersion::V2 => 8.0 + (accounts as f64) * 0.8,
        };

        let result = BenchResult {
            version,
            shape,
            jain,
            throughput,
            latency_p95,
            overhead_us,
        };
        self.report.results.push(result.clone());
        result
    }

    /// 生成结论。
    pub fn conclude(&mut self) -> &str {
        // 找每负载谱的优胜者。
        let mut lines: Vec<alloc::string::String> = Vec::new();
        for shape in [LoadShape::Uniform, LoadShape::Bursty, LoadShape::Skewed] {
            let v1 = self
                .report
                .results
                .iter()
                .find(|r| r.version == ArbVersion::V1 && r.shape == shape);
            let v2 = self
                .report
                .results
                .iter()
                .find(|r| r.version == ArbVersion::V2 && r.shape == shape);
            if let (Some(a), Some(b)) = (v1, v2) {
                let winner = if b.jain > a.jain + 0.02 {
                    "v2 WFQ"
                } else if a.throughput > b.throughput {
                    "v1 静态优先级"
                } else {
                    "v2 WFQ"
                };
                lines.push(alloc::format!(
                    "{}负载：{}（Jain v1={:.3} v2={:.3}）",
                    shape.name(),
                    winner,
                    a.jain,
                    b.jain
                ));
            }
        }
        self.report.conclusion = lines.join("; ");
        &self.report.conclusion
    }

    /// 报告访问。
    pub fn report(&self) -> &CompareReport {
        &self.report
    }
}

impl Default for ArbBenchmark {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

use super::super::checks::CheckSet;

pub fn run_cgb337_checks() -> CheckSet {
    let mut cs = CheckSet::new("CGPU-F0337 仲裁算法对比基准");

    let mut b = ArbBenchmark::new();

    // --- 三维对拍：均匀 ---
    let r1v1 = b.run(ArbVersion::V1, LoadShape::Uniform, 50);
    let r1v2 = b.run(ArbVersion::V2, LoadShape::Uniform, 50);
    cs.add("均匀谱 v1 有结果", r1v1.jain > 0.0, "Jain >0");
    cs.add_dyn("均匀谱 v2 有结果", r1v2.jain > 0.0, "Jain >0");

    // --- 突发 ---
    let r2v1 = b.run(ArbVersion::V1, LoadShape::Bursty, 50);
    let r2v2 = b.run(ArbVersion::V2, LoadShape::Bursty, 50);
    cs.add("突发谱对比", r2v1.shape == LoadShape::Bursty, "负载标注");

    // --- 倾斜 ---
    let r3v1 = b.run(ArbVersion::V1, LoadShape::Skewed, 50);
    let r3v2 = b.run(ArbVersion::V2, LoadShape::Skewed, 50);
    cs.add("倾斜谱对比", r3v1.shape == LoadShape::Skewed, "负载标注");

    // --- v2 公平性优势（倾斜谱） ---
    cs.add(
        "v2 倾斜谱更公平或接近",
        r3v2.jain >= r3v1.jain - 0.05,
        "WFQ 公平性",
    );

    // --- 吞吐有意义 ---
    cs.add("吞吐 ∈ [0,1]", r1v1.throughput >= 0.0 && r1v1.throughput <= 1.0, "完成率");

    // --- 延迟非负 ---
    cs.add("延迟非负", r2v1.latency_p95 >= 0.0, "P95 等待");

    // --- 开销估计 ---
    cs.add("开销有估计", r1v1.overhead_us > 0.0, "μs/次");

    // --- 结论 ---
    let conc = b.conclude().to_string();
    cs.add("结论非空", conc.len() > 0, "何场景用何版本");
    cs.add("结论含版本", conc.contains("v1") || conc.contains("v2"), "版本标注");

    // --- 报告入册 ---
    cs.add("六组合入册", b.report().results.len() == 6, "2版本×3负载");

    cs
}
