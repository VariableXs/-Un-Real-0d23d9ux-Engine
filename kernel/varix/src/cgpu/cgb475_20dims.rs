//! CGPU-F0475 · 20 维度验收（目标 360 行）
//! ==========================================================================
//! 锚点判据（施工书原文摘录）：
//! - 20 维度验收：域级逐维度打分（卷级 20 维度的预算域适配版——
//!   公平性/可观测/守恒正确性/诚实性为重心）；
//! - 每维度证据链；
//! - 低于标准回炉清单（不达标不收官）。判据：20 维度、证据链、
//!   回炉、判据。
//!
//! 设计纪律：
//! - 20 维度枚举（预算域适配版）：四个重心维度权重加倍（公平性/
//!   可观测/守恒正确性/诚实性），其余 16 维标准权重——总分按权
//!   重聚合（‰ 定点）。
//! - 每维度证据链：维度分必须携带证据引用（判据号），无证据的分
//!   数无效（证据链断裂即拒）。
//! - 回炉判定：任一重心维度 < 900‰ 或总分 < 850‰ → 回炉清单
//!   （不达标不收官）；回炉清单逐项带维度名。
//! - no_std + alloc；零 unwrap/expect 于运行路径；诊断段 0x10E0 独占。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址（0x10E0，全仓独占）。
pub const SEG_BASE: u16 = 0x10E0;

pub const E_AV_DIM: u16 = SEG_BASE + 1; // 维度非法
pub const E_AV_NOEVID: u16 = SEG_BASE + 2; // 证据链断裂
pub const E_AV_REWORK: u16 = SEG_BASE + 3; // 不达标回炉
pub const E_AV_SCORE: u16 = SEG_BASE + 4; // 分值越界

/// 20 维度（0~19；前 4 维为重心）。
pub const V_FAIR: u8 = 0; // 公平性（重心）
pub const V_OBSERV: u8 = 1; // 可观测（重心）
pub const V_CONSERVE: u8 = 2; // 守恒正确性（重心）
pub const V_HONESTY: u8 = 3; // 诚实性（重心）
pub const V_LATENCY: u8 = 4;
pub const V_THROUGHPUT: u8 = 5;
pub const V_ISOLATION: u8 = 6;
pub const V_RECOVER: u8 = 7;
pub const V_COMPAT: u8 = 8;
pub const V_CONSIST: u8 = 9;
pub const V_SECURITY: u8 = 10;
pub const V_DOCS: u8 = 11;
pub const V_TESTCOV: u8 = 12;
pub const V_PERF: u8 = 13;
pub const V_USABILITY: u8 = 14;
pub const V_ROBUST: u8 = 15;
pub const V_SCALAB: u8 = 16;
pub const V_AUDITABLE: u8 = 17;
pub const V_ERRORS: u8 = 18;
pub const V_TELEMETRY: u8 = 19;
pub const DIMS: u8 = 20;

/// 重心维度集。
pub const HEAVY: [u8; 4] = [V_FAIR, V_OBSERV, V_CONSERVE, V_HONESTY];
/// 重心权重 ×2（千分位）。
pub const WEIGHT_HEAVY_PM: u64 = 2000;
pub const WEIGHT_NORMAL_PM: u64 = 1000;

pub fn is_heavy(d: u8) -> bool {
    HEAVY.contains(&d)
}

pub fn weight_pm(d: u8) -> Result<u64, u16> {
    if d >= DIMS {
        return Err(E_AV_DIM);
    }
    Ok(if is_heavy(d) { WEIGHT_HEAVY_PM } else { WEIGHT_NORMAL_PM })
}

/// 维度评分（分值+证据引用）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DimScore {
    pub dim: u8,
    /// 分值（‰，0~1000）。
    pub score: u64,
    /// 证据引用（判据号文本，如 "F0469"）。
    pub evidence: &'static str,
}

impl DimScore {
    pub fn valid(&self) -> Result<(), u16> {
        if self.dim >= DIMS {
            return Err(E_AV_DIM);
        }
        if self.score > 1000 {
            return Err(E_AV_SCORE);
        }
        if self.evidence.is_empty() {
            return Err(E_AV_NOEVID);
        }
        Ok(())
    }
}

/// 加权总分（‰ 定点）：Σ(score×weight)/Σ(weight)。
pub fn weighted_total(scores: &[DimScore]) -> Result<u64, u16> {
    if scores.len() != DIMS as usize {
        return Err(E_AV_DIM);
    }
    let mut num: u64 = 0;
    let mut den: u64 = 0;
    for s in scores {
        s.valid()?;
        num += s.score * weight_pm(s.dim)?;
        den += weight_pm(s.dim)?;
    }
    if den == 0 {
        return Err(E_AV_DIM);
    }
    Ok(num / den)
}

/// 回炉清单：重心维度 < 900‰ 或总分 < 850‰ → 不达标不收官。
pub const HEAVY_FLOOR_PM: u64 = 900;
pub const TOTAL_FLOOR_PM: u64 = 850;

pub fn rework_list(scores: &[DimScore]) -> Result<Vec<&'static str>, u16> {
    let total = weighted_total(scores)?;
    let mut out: Vec<&'static str> = Vec::new();
    for s in scores {
        s.valid()?;
        if is_heavy(s.dim) && s.score < HEAVY_FLOOR_PM {
            out.push(dim_name(s.dim));
        }
    }
    if total < TOTAL_FLOOR_PM {
        out.push("TOTAL");
    }
    if !out.is_empty() {
        return Ok(out); // 有回炉项（调用方判空决定收官）
    }
    Ok(out)
}

pub fn dim_name(d: u8) -> &'static str {
    match d {
        V_FAIR => "fairness",
        V_OBSERV => "observability",
        V_CONSERVE => "conservation",
        V_HONESTY => "honesty",
        V_LATENCY => "latency",
        V_THROUGHPUT => "throughput",
        V_ISOLATION => "isolation",
        V_RECOVER => "recovery",
        V_COMPAT => "compatibility",
        V_CONSIST => "consistency",
        V_SECURITY => "security",
        V_DOCS => "docs",
        V_TESTCOV => "test-coverage",
        V_PERF => "performance",
        V_USABILITY => "usability",
        V_ROBUST => "robustness",
        V_SCALAB => "scalability",
        V_AUDITABLE => "auditability",
        V_ERRORS => "error-discipline",
        V_TELEMETRY => "telemetry",
        _ => "unknown",
    }
}

pub(crate) fn run_checks() -> Vec<(&'static str, bool)> {
    let mut v: Vec<(&'static str, bool)> = Vec::new();
    // 一、20 维度：权重声明。
    {
        v.push(("F0475-20维度-枚举完整", DIMS == 20));
        v.push((
            "F0475-20维度-四重心权重加倍",
            weight_pm(V_FAIR) == Ok(WEIGHT_HEAVY_PM) && weight_pm(V_LATENCY) == Ok(WEIGHT_NORMAL_PM),
        ));
        v.push(("F0475-20维度-维度越界拒", weight_pm(99) == Err(E_AV_DIM)));
    }
    // 二、满分卷通过。
    {
        let full: Vec<DimScore> = (0..DIMS)
            .map(|d| DimScore { dim: d, score: 1000, evidence: "F0466" })
            .collect();
        v.push(("F0475-满分卷-总分千分位", weighted_total(&full) == Ok(1000)));
        v.push(("F0475-满分卷-回炉清单空", rework_list(&full) == Ok(Vec::new())));
    }
    // 三、证据链：无证据分数无效。
    {
        let mut full: Vec<DimScore> = (0..DIMS).map(|d| DimScore { dim: d, score: 950, evidence: "F0469" }).collect();
        full[7].evidence = "";
        v.push(("F0475-证据链-断链拒", weighted_total(&full) == Err(E_AV_NOEVID)));
        full[7].evidence = "F0459";
        v.push(("F0475-证据链-补齐通过", weighted_total(&full).is_ok()));
        // 分值越界。
        let mut over = full.clone();
        over[2].score = 1001;
        v.push(("F0475-证据链-分值越界拒", weighted_total(&over) == Err(E_AV_SCORE)));
    }
    // 四、回炉判定：重心低分拦。
    {
        let mut weak: Vec<DimScore> = (0..DIMS).map(|d| DimScore { dim: d, score: 950, evidence: "F0466" }).collect();
        weak[V_FAIR as usize].score = 850; // 重心维度低于 900
        let r = rework_list(&weak);
        v.push(("F0475-回炉-重心低分回炉", r.as_ref().map(|l| l.contains(&"fairness")) == Ok(true)));
        // 总分不足回炉。
        let mut low_total: Vec<DimScore> = (0..DIMS).map(|d| DimScore { dim: d, score: 780, evidence: "F0466" }).collect();
        // 重心拉高避开单维红线，总分仍低。
        for d in HEAVY.iter() {
            low_total[*d as usize].score = 950;
        }
        let r2 = rework_list(&low_total);
        v.push(("F0475-回炉-总分不足回炉", r2.as_ref().map(|l| l.contains(&"TOTAL")) == Ok(true)));
    }
    // 五、不达标不收官：回炉非空 ⇒ 收官拒（编排语义）。
    {
        let mut weak: Vec<DimScore> = (0..DIMS).map(|d| DimScore { dim: d, score: 960, evidence: "F0466" }).collect();
        weak[V_HONESTY as usize].score = 700;
        let rl = rework_list(&weak).unwrap_or_default();
        v.push(("F0475-判据-回炉非空不收官", !rl.is_empty()));
        // 维度名覆盖重心四枚举。
        v.push((
            "F0475-判据-重心名齐备",
            dim_name(V_FAIR) == "fairness" && dim_name(V_OBSERV) == "observability" && dim_name(V_CONSERVE) == "conservation" && dim_name(V_HONESTY) == "honesty",
        ));
    }
    // 六、判据自检。
    {
        v.push(("F0475-判据-重心恰好四维", HEAVY.len() == 4));
        v.push(("F0475-判据-权重和=24000", (0..DIMS).map(|d| weight_pm(d).unwrap_or(0)).sum::<u64>() == 24000));
    }
    v
}

#[cfg(test)]
mod tests {
    use super::run_checks;
    #[test]
    fn f0475_20dims_checks() {
        for (name, ok) in run_checks() {
            assert!(ok, "红项: {}", name);
        }
    }
}
