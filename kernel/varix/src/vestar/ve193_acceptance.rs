//! ve193_acceptance ↔ VE-F0193 域验收矩阵（20 维度逐项过）
//!
//! A 域按 20 维验收清单逐项核验：需求理解、功能完整性、交互正确性、细节、视觉、动画、
//! 性能、代码质量、可拓展性、健壮性、数据安全、一致性、文案、无障碍、兼容、安全隐私、
//! 错误恢复、可观测性、文档、整体感受；每维评分附证据指针，低于标准项阻断交付；
//! A 域特色增维（渲染确定性+性能可解释性）并行列验。
//!
//! 判据：20 维逐项、证据裁决、阻断机制、增维并行。

use alloc::format;
use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 20 个验收维度。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum AcceptDimension {
    /// 需求理解。
    ReqUnderstanding,
    /// 功能完整性。
    FuncCompleteness,
    /// 交互正确性。
    InteractionCorrectness,
    /// 细节。
    Detail,
    /// 视觉。
    Visual,
    /// 动画。
    Animation,
    /// 性能。
    Perf,
    /// 代码质量。
    CodeQuality,
    /// 可拓展性。
    Extensibility,
    /// 健壮性。
    Robustness,
    /// 数据安全。
    DataSafety,
    /// 一致性。
    Consistency,
    /// 文案。
    Copy,
    /// 无障碍。
    Accessibility,
    /// 兼容。
    Compat,
    /// 安全隐私。
    SecurityPrivacy,
    /// 错误恢复。
    ErrorRecovery,
    /// 可观测性。
    Observability,
    /// 文档。
    Docs,
    /// 整体感受。
    Overall,
}

/// 增维（A 域特色）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ExtraAcceptDim {
    /// 渲染确定性。
    RenderDeterminism,
    /// 性能可解释性。
    PerfExplainability,
}

/// 评分与证据。
#[derive(Debug, Clone)]
pub struct ScoreEntry {
    /// 评分（0-100）。
    pub score: u8,
    /// 证据指针。
    pub evidence: String,
    /// 是否达标（>= pass_threshold）。
    pub meets_standard: bool,
}

/// 验收矩阵。
pub struct AcceptanceMatrix {
    /// 20 维评分。
    scores: BTreeMap<AcceptDimension, ScoreEntry>,
    /// 增维评分。
    extra_scores: BTreeMap<ExtraAcceptDim, ScoreEntry>,
    /// 通过阈值。
    pub pass_threshold: u8,
    /// 阻断清单。
    blocking: Vec<String>,
}

/// 所有 20 维。
pub const ALL_DIMS: [AcceptDimension; 20] = [
    AcceptDimension::ReqUnderstanding,
    AcceptDimension::FuncCompleteness,
    AcceptDimension::InteractionCorrectness,
    AcceptDimension::Detail,
    AcceptDimension::Visual,
    AcceptDimension::Animation,
    AcceptDimension::Perf,
    AcceptDimension::CodeQuality,
    AcceptDimension::Extensibility,
    AcceptDimension::Robustness,
    AcceptDimension::DataSafety,
    AcceptDimension::Consistency,
    AcceptDimension::Copy,
    AcceptDimension::Accessibility,
    AcceptDimension::Compat,
    AcceptDimension::SecurityPrivacy,
    AcceptDimension::ErrorRecovery,
    AcceptDimension::Observability,
    AcceptDimension::Docs,
    AcceptDimension::Overall,
];

impl AcceptanceMatrix {
    /// 新建矩阵（阈值 70）。
    pub fn new() -> Self {
        Self {
            scores: BTreeMap::new(),
            extra_scores: BTreeMap::new(),
            pass_threshold: 70,
            blocking: Vec::new(),
        }
    }

    /// 评分（含证据指针）。
    pub fn score(&mut self, dim: AcceptDimension, score: u8, evidence: &str) {
        let meets = score >= self.pass_threshold && !evidence.is_empty();
        if !meets {
            self.blocking.push(alloc::format!("维度[{:?}]评分{}未达标", dim, score));
        }
        self.scores.insert(
            dim,
            ScoreEntry {
                score,
                evidence: evidence.to_string(),
                meets_standard: meets,
            },
        );
    }

    /// 增维评分。
    pub fn score_extra(&mut self, dim: ExtraAcceptDim, score: u8, evidence: &str) {
        let meets = score >= self.pass_threshold && !evidence.is_empty();
        if !meets {
            self.blocking.push(alloc::format!("增维[{:?}]评分{}未达标", dim, score));
        }
        self.extra_scores.insert(
            dim,
            ScoreEntry {
                score,
                evidence: evidence.to_string(),
                meets_standard: meets,
            },
        );
    }

    /// 检查是否全部 20 维已评分。
    pub fn is_complete(&self) -> bool {
        ALL_DIMS.iter().all(|d| self.scores.contains_key(d))
    }

    /// 阻断清单（低于标准或证据缺失）。
    pub fn blocking_items(&self) -> &[String] {
        &self.blocking
    }

    /// 是否可交付（无阻断项）。
    pub fn can_deliver(&self) -> bool {
        self.blocking.is_empty() && self.is_complete()
    }

    /// 汇总报告。
    pub fn summary(&self) -> String {
        let mut out = String::new();
        out.push_str(&alloc::format!(
            "验收矩阵: {}/20 维已评, 阻断项 {} 个\n",
            self.scores.len(),
            self.blocking.len()
        ));
        for dim in &ALL_DIMS {
            if let Some(entry) = self.scores.get(dim) {
                let mark = if entry.meets_standard { "✓" } else { "✗" };
                out.push_str(&alloc::format!(
                    "  {} {:?}: 分={}, 证据={}\n",
                    mark, dim, entry.score, entry.evidence
                ));
            } else {
                out.push_str(&alloc::format!("  - {:?}: 未评分\n", dim));
            }
        }
        for (dim, entry) in &self.extra_scores {
            let mark = if entry.meets_standard { "✓" } else { "✗" };
            out.push_str(&alloc::format!(
                "  [增维] {} {:?}: 分={}, 证据={}\n",
                mark, dim, entry.score, entry.evidence
            ));
        }
        out
    }

    /// 获取指定维度评分。
    pub fn get_score(&self, dim: AcceptDimension) -> Option<&ScoreEntry> {
        self.scores.get(&dim)
    }
}

impl Default for AcceptanceMatrix {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_all_dims_scoring() {
        let mut m = AcceptanceMatrix::new();
        for dim in &ALL_DIMS {
            m.score(*dim, 85, "test_evidence");
        }
        assert!(m.is_complete());
        assert!(m.can_deliver());
    }

    #[test]
    fn test_low_score_blocks() {
        let mut m = AcceptanceMatrix::new();
        m.score(AcceptDimension::Perf, 50, "evidence"); // 低于阈值 70
        assert!(!m.can_deliver());
        assert_eq!(m.blocking_items().len(), 1);
    }

    #[test]
    fn test_missing_evidence_blocks() {
        let mut m = AcceptanceMatrix::new();
        m.score(AcceptDimension::Perf, 85, ""); // 无证据
        assert!(!m.can_deliver());
    }

    #[test]
    fn test_incomplete_cannot_deliver() {
        let mut m = AcceptanceMatrix::new();
        m.score(AcceptDimension::Perf, 85, "e"); // 只评了 1 维
        assert!(!m.can_deliver());
    }

    #[test]
    fn test_extra_dim() {
        let mut m = AcceptanceMatrix::new();
        for dim in &ALL_DIMS {
            m.score(*dim, 85, "e");
        }
        m.score_extra(ExtraAcceptDim::RenderDeterminism, 90, "bit_exact_test");
        m.score_extra(ExtraAcceptDim::PerfExplainability, 88, "budget_report");
        assert!(m.can_deliver());
    }

    #[test]
    fn test_summary_format() {
        let mut m = AcceptanceMatrix::new();
        m.score(AcceptDimension::Perf, 85, "e");
        let s = m.summary();
        assert!(s.contains("Perf"));
        assert!(s.contains("85"));
    }
}
