// CGPU-F0315 B域 20维度验收执行 · cgb315_accept20.rs
// AI-MIMO-1022 · CGPU-B 收口
// 判据：20 维度逐一评分附证据；任一维度低于阈值阻断交付；
//       综合评分可报告；验收矩阵可签署。
#![allow(dead_code)]

use alloc::format;
extern crate alloc;
use alloc::vec::Vec;
use alloc::string::String;

use super::super::checks::CheckSet;

/// 验收维度。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AcceptDimension {
    Functional,
    Performance,
    Reliability,
    Security,
    Usability,
    Accessibility,
    Maintainability,
    Portability,
    Scalability,
    Interoperability,
    Determinism,
    Observability,
    Recoverability,
    Compatibility,
    Documentation,
    TestCoverage,
    CodeQuality,
    ApiStability,
    ResourceEfficiency,
    Compliance,
}

impl AcceptDimension {
    pub fn name(self) -> &'static str {
        match self {
            Self::Functional => "功能",
            Self::Performance => "性能",
            Self::Reliability => "可靠性",
            Self::Security => "安全",
            Self::Usability => "易用",
            Self::Accessibility => "无障碍",
            Self::Maintainability => "可维护",
            Self::Portability => "可移植",
            Self::Scalability => "可扩展",
            Self::Interoperability => "互操作",
            Self::Determinism => "确定性",
            Self::Observability => "可观测",
            Self::Recoverability => "可恢复",
            Self::Compatibility => "兼容",
            Self::Documentation => "文档",
            Self::TestCoverage => "测试覆盖",
            Self::CodeQuality => "代码质量",
            Self::ApiStability => "API稳定",
            Self::ResourceEfficiency => "资源效率",
            Self::Compliance => "合规",
        }
    }
}

/// 维度评分。
#[derive(Clone, Copy, Debug)]
pub struct DimensionScore {
    pub dim: AcceptDimension,
    pub score_q8: u16, // 0..=256
    pub threshold_q8: u16,
    pub evidence_id: u32,
    pub passed: bool,
}

/// 验收矩阵。
pub struct AcceptanceMatrix {
    pub scores: Vec<DimensionScore>,
    pub overall_q8: u16,
    pub signed: bool,
    pub blocker_count: u32,
}

impl AcceptanceMatrix {
    pub fn new() -> Self {
        Self {
            scores: Vec::new(),
            overall_q8: 0,
            signed: false,
            blocker_count: 0,
        }
    }

    /// 加载标准 20 维度。
    pub fn load_standard(&mut self) {
        let dims = [
            AcceptDimension::Functional,
            AcceptDimension::Performance,
            AcceptDimension::Reliability,
            AcceptDimension::Security,
            AcceptDimension::Usability,
            AcceptDimension::Accessibility,
            AcceptDimension::Maintainability,
            AcceptDimension::Portability,
            AcceptDimension::Scalability,
            AcceptDimension::Interoperability,
            AcceptDimension::Determinism,
            AcceptDimension::Observability,
            AcceptDimension::Recoverability,
            AcceptDimension::Compatibility,
            AcceptDimension::Documentation,
            AcceptDimension::TestCoverage,
            AcceptDimension::CodeQuality,
            AcceptDimension::ApiStability,
            AcceptDimension::ResourceEfficiency,
            AcceptDimension::Compliance,
        ];
        for d in dims {
            self.scores.push(DimensionScore {
                dim: d,
                score_q8: 0,
                threshold_q8: 179, // 70%
                evidence_id: 0,
                passed: false,
            });
        }
    }

    /// 评分（附证据）。
    pub fn score(&mut self, dim: AcceptDimension, score_q8: u16, evidence_id: u32) -> bool {
        if let Some(s) = self.scores.iter_mut().find(|s| s.dim == dim) {
            s.score_q8 = score_q8.min(256);
            s.evidence_id = evidence_id;
            s.passed = s.score_q8 >= s.threshold_q8 && evidence_id > 0;
            true
        } else {
            false
        }
    }

    /// 综合评分（加权平均）。
    pub fn compute_overall(&mut self) -> u16 {
        if self.scores.is_empty() {
            self.overall_q8 = 0;
            return 0;
        }
        let sum: u32 = self.scores.iter().map(|s| s.score_q8 as u32).sum();
        self.overall_q8 = (sum / self.scores.len() as u32) as u16;
        self.blocker_count = self.scores.iter().filter(|s| !s.passed).count() as u32;
        self.overall_q8
    }

    /// 签署（零 blocker）。
    pub fn sign(&mut self) -> bool {
        self.compute_overall();
        if self.blocker_count > 0 {
            return false;
        }
        self.signed = true;
        true
    }

    /// 阻断维度列表。
    pub fn blockers(&self) -> Vec<&DimensionScore> {
        self.scores.iter().filter(|s| !s.passed).collect()
    }

    /// 人话报告。
    pub fn summary(&self) -> String {
        let mut s = String::from("20 维度验收：");
        s.push_str(&alloc::format!("综合 {}/256", self.overall_q8));
        s.push_str(&alloc::format!(" | 通过 {}/20", 20 - self.blocker_count));
        for b in self.blockers() {
            s.push_str(&alloc::format!(" [阻断:{}]", b.dim.name()));
        }
        s
    }

    pub fn run_checks(&self) -> CheckSet {
        let mut cs = CheckSet::new("CGPU-F0315 B域 20 维度验收");
        cs.add_dyn("20 维度齐全", self.scores.len() == 20, &alloc::format!("n={}", self.scores.len()));
        cs.add("维度不重复", {
            let mut n = 0;
            for i in 0..self.scores.len() {
                for j in (i + 1)..self.scores.len() {
                    if self.scores[i].dim == self.scores[j].dim {
                        n += 1;
                    }
                }
            }
            n == 0
        }, "unique dims");
        cs.add("评分 ≤ 256", self.scores.iter().all(|s| s.score_q8 <= 256), "range");
        cs.add("通过的必有证据", {
            self.scores.iter().filter(|s| s.passed).all(|s| s.evidence_id > 0)
        }, "evidence");
        cs.add("签署零阻断", {
            !self.signed || self.blocker_count == 0
        }, "gate");
        cs.add("综合评分在范围内", self.overall_q8 <= 256, "overall");
        cs.add("阈值合理", self.scores.iter().all(|s| s.threshold_q8 <= 256), "threshold");
        cs
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
    fn all_pass_sign() {
        let mut m = AcceptanceMatrix::new();
        m.load_standard();
        for i in 0..20 {
            let dim = m.scores[i].dim;
            m.score(dim, 200, i as u32 + 1);
        }
        assert!(m.sign());
    }

    #[test]
    fn blocker_prevents_sign() {
        let mut m = AcceptanceMatrix::new();
        m.load_standard();
        for i in 0..20 {
            let dim = m.scores[i].dim;
            m.score(dim, 200, i as u32 + 1);
        }
        m.score(AcceptDimension::Security, 100, 1); // 不达标
        assert!(!m.sign());
        assert_eq!(m.blockers().len(), 1);
    }
}
