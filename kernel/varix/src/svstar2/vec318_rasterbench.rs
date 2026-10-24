//! VE-F0718 · 栅格化基准入册
//!
//! 职责：栅格化基准与基线入册——
//! 性能基准（F0708/09/10 的基准集）入 CGPU-Bench、
//! 质量基线图（F0708 对拍图库）入金标准库——栅格化的性能与质量双锚点。
//! 判据：双册入册、判据下限、审查流程、回归集成、版本化。

use alloc::string::{String, ToString};
use alloc::format;
use alloc::vec;
use alloc::vec::Vec;

/// 性能基准条目：名目 + 场景 + 基线值 + 容差。
#[derive(Clone, Debug, PartialEq)]
pub struct PerfBench {
    pub name: String,
    pub scene: String,
    /// 基线值（毫秒）。
    pub baseline_ms: u32,
    /// 容差（±千分位）。
    pub tolerance_permille: u32,
}

impl PerfBench {
    /// 判据下限核验：实测不超过基线 × (1 + 容差)。
    pub fn within(&self, measured_ms: u32) -> bool {
        let limit = self.baseline_ms as u64 * (1000 + self.tolerance_permille as u64) / 1000;
        measured_ms as u64 <= limit
    }
}

/// 质量基线条目：形状 + 金标准分数。
#[derive(Clone, Debug, PartialEq)]
pub struct QualityBench {
    pub shape: String,
    pub gold_score: u32,
}

/// 双册：性能册（CGPU-Bench）与质量册（金标准库）。
pub struct DualBook {
    pub perf: Vec<PerfBench>,
    pub quality: Vec<QualityBench>,
}

impl DualBook {
    pub fn new() -> Self {
        DualBook { perf: Vec::new(), quality: Vec::new() }
    }
    /// 双册入册核验：两册都非空。
    pub fn both_registered(&self) -> bool {
        !self.perf.is_empty() && !self.quality.is_empty()
    }
    /// 回归集成：性能册全条目 + 质量册全条目都须通过判据下限。
    pub fn regression(&self, perf_measured: &[u32], quality_measured: &[u32]) -> Result<(), String> {
        if perf_measured.len() != self.perf.len() {
            return Err(format!("性能实测 {} != 基准 {}", perf_measured.len(), self.perf.len()));
        }
        if quality_measured.len() != self.quality.len() {
            return Err(format!("质量实测 {} != 基线 {}", quality_measured.len(), self.quality.len()));
        }
        for (b, m) in self.perf.iter().zip(perf_measured.iter()) {
            if !b.within(*m) {
                return Err(format!("性能回归红：{}（基线 {} 实测 {}）", b.name, b.baseline_ms, m));
            }
        }
        for (q, m) in self.quality.iter().zip(quality_measured.iter()) {
            // 质量下限：不低于金标准 95%（千分位）。
            if (*m as u64) * 1000 < q.gold_score as u64 * 950 {
                return Err(format!("质量回归红：{}（金标准 {} 实测 {}）", q.shape, q.gold_score, m));
            }
        }
        Ok(())
    }
}

/// 审查流程：入册条目必须过审（审查人 + 结论）。
#[derive(Clone, Debug, PartialEq)]
pub struct ReviewRecord {
    pub item: String,
    pub reviewer: String,
    pub approved: bool,
}

/// 入册前置审查：未过审条目不得入册。
pub fn require_review(item: &str, reviews: &[ReviewRecord]) -> Result<(), String> {
    match reviews.iter().find(|r| r.item == item) {
        Some(r) if r.approved => Ok(()),
        Some(_) => Err(format!("{} 审查未过：不得入册", item)),
        None => Err(format!("{} 无审查记录：先审后入", item)),
    }
}

/// 版本化：册版本随条目增删而升——入册即版本 +1。
pub fn book_version(entries: u32, base: u32) -> u32 {
    base + entries
}

/// 栅格化标准基准集（F0708 质量集 + F0709/10 性能集）。
pub fn standard_book() -> DualBook {
    let mut b = DualBook::new();
    b.perf.push(PerfBench {
        name: "扫描线吞吐".to_string(),
        scene: "10000 边路径".to_string(),
        baseline_ms: 8,
        tolerance_permille: 150,
    });
    b.perf.push(PerfBench {
        name: "SIMD AEL".to_string(),
        scene: "50000 边".to_string(),
        baseline_ms: 12,
        tolerance_permille: 150,
    });
    b.perf.push(PerfBench {
        name: "多线程伸缩".to_string(),
        scene: "8 线程大场景".to_string(),
        baseline_ms: 30,
        tolerance_permille: 200,
    });
    b.quality.push(QualityBench { shape: "圆".to_string(), gold_score: 950 });
    b.quality.push(QualityBench { shape: "星形".to_string(), gold_score: 900 });
    b.quality.push(QualityBench { shape: "文字轮廓".to_string(), gold_score: 920 });
    b
}

pub fn run_vec318_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("svstar2-vec318");
    // 族一：双册标准集
    let book = standard_book();
    s.add("性能册三条", book.perf.len() == 3, "perf-3");
    s.add("质量册三条", book.quality.len() == 3, "qual-3");
    s.add("双册齐备", book.both_registered(), "both");
    s.add("空册不齐备", !DualBook::new().both_registered(), "empty");
    // 族二：判据下限
    let bench = &book.perf[0];
    s.add("基线内通过", bench.within(bench.baseline_ms), "at-baseline");
    s.add("容差内通过", bench.within(9), "in-tol");
    s.add("超容差拒绝", !bench.within(20), "over");
    s.add("快于基线通过", bench.within(4), "faster");
    // 族三：回归集成
    s.add("全绿回归通过", book.regression(&[8, 12, 30], &[960, 910, 930]).is_ok(), "reg-ok");
    match book.regression(&[8, 12, 60], &[960, 910, 930]) {
        Err(m) => s.add("性能红点名", m.contains("多线程伸缩") && m.contains("60"), "perf-red"),
        Ok(()) => s.add("性能红点名", false, "expected err"),
    }
    match book.regression(&[8, 12, 30], &[960, 800, 930]) {
        Err(m) => s.add("质量红点名", m.contains("星形"), "qual-red"),
        Ok(()) => s.add("质量红点名", false, "expected err"),
    }
    s.add("实测数不符拒绝", book.regression(&[8], &[960, 910, 930]).is_err(), "count-mismatch");
    s.add("质量下限 95% 口径", {
        let q = &book.quality[0];
        (q.gold_score as u64 * 950 / 1000) == 902
    }, "floor");
    // 族四：审查流程
    let reviews = vec![
        ReviewRecord { item: "扫描线吞吐".to_string(), reviewer: "AI-WBB-1022".to_string(), approved: true },
        ReviewRecord { item: "圆".to_string(), reviewer: "AI-WBB-1022".to_string(), approved: false },
    ];
    s.add("过审可入册", require_review("扫描线吞吐", &reviews).is_ok(), "reviewed");
    match require_review("圆", &reviews) {
        Err(m) => s.add("未过审拒绝", m.contains("不得入册"), "rejected"),
        Ok(()) => s.add("未过审拒绝", false, "expected err"),
    }
    s.add("无审查拒绝", require_review("SIMD AEL", &reviews).is_err(), "no-review");
    // 族五：版本化
    s.add("入册升版本", book_version(3, 10) == 13, "version");
    s.add("零条目版本不变", book_version(0, 10) == 10, "no-change");
    s.add("双锚点定位：性能+质量", book.perf.len() > 0 && book.quality.len() > 0, "dual-anchor");
    s
}
