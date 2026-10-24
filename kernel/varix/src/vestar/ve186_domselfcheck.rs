//! ve186_domselfcheck ↔ VE-F0186 VE-A 域聚合自检
//!
//! A09 自查组（家族自查模板源头域）；范围声明（A01-A08 八组×20 项=160 条+组间对账+七维汇总）；
//! A 域特色增维声明（渲染确定性维、性能可解释性维）；证据引用制（结论必须附证据指针）。
//!
//! 判据：源头模板、增维声明、证据引用制、七维汇总。

use alloc::format;
use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 七维核验维度。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Dimension {
    /// API 一致性。
    Api,
    /// 测试覆盖。
    Test,
    /// 性能达标。
    Perf,
    /// 安全合规。
    Security,
    /// 无障碍。
    Accessibility,
    /// 文档完整。
    Docs,
    /// 预算合规。
    Budget,
}

/// A 域特色增维（渲染确定性、性能可解释性）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ExtraDimension {
    /// 渲染确定性（bit-exact 回归语义）。
    RenderDeterminism,
    /// 性能可解释性（每帧预算归因）。
    PerfExplainability,
}

/// 证据指针。
#[derive(Debug, Clone)]
pub struct EvidencePointer {
    /// 证据 ID。
    pub id: String,
    /// 证据位置（文件路径或测试名）。
    pub location: String,
    /// 证据摘要。
    pub summary: String,
    /// 是否过期。
    pub stale: bool,
}

/// 核验结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// 通过。
    Pass,
    /// 不通过（阻断）。
    Fail,
    /// 证据缺失（阻断并回补）。
    MissingEvidence,
    /// 证据过期（增量复查）。
    StaleEvidence,
}

/// 自查条目。
#[derive(Debug, Clone)]
pub struct CheckItem {
    /// 条目 ID。
    pub id: String,
    /// 组号（A01~A08）。
    pub group: u8,
    /// 维度。
    pub dimension: Dimension,
    /// 核验结果。
    pub verdict: Verdict,
    /// 证据指针（必须非空才可 Pass）。
    pub evidence: Option<EvidencePointer>,
}

/// 增维声明。
#[derive(Debug, Clone)]
pub struct ExtraDimDecl {
    /// 增维。
    pub dim: ExtraDimension,
    /// 理由。
    pub reason: String,
    /// 标准。
    pub standard: String,
    /// 色标联动（绿/黄/红阈值）。
    pub color_thresholds: [f64; 3],
}

/// 域聚合自检器。
pub struct DomainSelfCheck {
    /// 自查条目表。
    items: Vec<CheckItem>,
    /// 增维声明。
    extra_decls: Vec<ExtraDimDecl>,
    /// 证据指针表。
    evidence_table: BTreeMap<String, EvidencePointer>,
    /// 版本号。
    version: u64,
}

impl DomainSelfCheck {
    /// 新建自检器。
    pub fn new() -> Self {
        Self {
            items: Vec::new(),
            extra_decls: Vec::new(),
            evidence_table: BTreeMap::new(),
            version: 1,
        }
    }

    /// 登记增维声明。
    pub fn declare_extra_dim(
        &mut self,
        dim: ExtraDimension,
        reason: &str,
        standard: &str,
        thresholds: [f64; 3],
    ) -> Result<(), u16> {
        if reason.is_empty() || standard.is_empty() {
            return Err(0x7F01); // 增维无标准→先立标准后入矩阵
        }
        self.extra_decls.push(ExtraDimDecl {
            dim,
            reason: reason.to_string(),
            standard: standard.to_string(),
            color_thresholds: thresholds,
        });
        self.version += 1;
        Ok(())
    }

    /// 添加自查条目（证据引用制：Pass 必须附证据）。
    pub fn add_item(
        &mut self,
        id: &str,
        group: u8,
        dimension: Dimension,
        verdict: Verdict,
        evidence: Option<EvidencePointer>,
    ) -> Result<(), u16> {
        // 证据引用制：Pass 但无证据 → 阻断
        if verdict == Verdict::Pass && evidence.is_none() {
            return Err(0x7F02);
        }
        if let Some(ref ev) = evidence {
            self.evidence_table.insert(ev.id.clone(), ev.clone());
        }
        self.items.push(CheckItem {
            id: id.to_string(),
            group,
            dimension,
            verdict,
            evidence,
        });
        self.version += 1;
        Ok(())
    }

    /// 按维度汇总。
    pub fn summary_by_dimension(&self) -> BTreeMap<Dimension, (usize, usize)> {
        let mut map: BTreeMap<Dimension, (usize, usize)> = BTreeMap::new();
        for item in &self.items {
            let entry = map.entry(item.dimension).or_insert((0, 0));
            entry.0 += 1; // 总数
            if item.verdict == Verdict::Pass {
                entry.1 += 1; // 通过数
            }
        }
        map
    }

    /// 按组汇总。
    pub fn summary_by_group(&self) -> BTreeMap<u8, (usize, usize)> {
        let mut map: BTreeMap<u8, (usize, usize)> = BTreeMap::new();
        for item in &self.items {
            let entry = map.entry(item.group).or_insert((0, 0));
            entry.0 += 1;
            if item.verdict == Verdict::Pass {
                entry.1 += 1;
            }
        }
        map
    }

    /// 检查证据完整性：所有 Pass 条目必须有非过期证据。
    pub fn check_evidence_integrity(&self) -> Vec<String> {
        let mut issues = Vec::new();
        for item in &self.items {
            if item.verdict == Verdict::Pass {
                match &item.evidence {
                    None => issues.push(alloc::format!("条目[{}] Pass 但无证据", item.id)),
                    Some(ev) => {
                        if ev.stale {
                            issues.push(alloc::format!("条目[{}] 证据已过期", item.id));
                        }
                    }
                }
            }
        }
        issues
    }

    /// 阻断清单（Fail / MissingEvidence）。
    pub fn blocking_items(&self) -> Vec<&CheckItem> {
        self.items
            .iter()
            .filter(|i| i.verdict == Verdict::Fail || i.verdict == Verdict::MissingEvidence)
            .collect()
    }

    /// 总条目数。
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// 版本号。
    pub fn version(&self) -> u64 {
        self.version
    }

    /// 增维声明列表。
    pub fn extra_declarations(&self) -> &[ExtraDimDecl] {
        &self.extra_decls
    }

    /// 全量条目。
    pub fn all_items(&self) -> &[CheckItem] {
        &self.items
    }
}

impl Default for DomainSelfCheck {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(id: &str) -> EvidencePointer {
        EvidencePointer {
            id: id.to_string(),
            location: "test_x".to_string(),
            summary: "ok".to_string(),
            stale: false,
        }
    }

    #[test]
    fn test_pass_requires_evidence() {
        let mut d = DomainSelfCheck::new();
        // Pass 但无证据 → 拒绝
        assert!(d
            .add_item("x1", 1, Dimension::Api, Verdict::Pass, None)
            .is_err());
        // Pass 有证据 → 接受
        assert!(d
            .add_item("x1", 1, Dimension::Api, Verdict::Pass, Some(ev("e1")))
            .is_ok());
    }

    #[test]
    fn test_summary_by_dimension() {
        let mut d = DomainSelfCheck::new();
        d.add_item("a", 1, Dimension::Api, Verdict::Pass, Some(ev("e1")))
            .unwrap();
        d.add_item("b", 1, Dimension::Api, Verdict::Fail, None).unwrap();
        d.add_item("c", 2, Dimension::Test, Verdict::Pass, Some(ev("e2")))
            .unwrap();
        let sum = d.summary_by_dimension();
        assert_eq!(sum[&Dimension::Api], (2, 1));
        assert_eq!(sum[&Dimension::Test], (1, 1));
    }

    #[test]
    fn test_stale_evidence_flagged() {
        let mut d = DomainSelfCheck::new();
        let mut e = ev("e1");
        e.stale = true;
        d.add_item("x", 1, Dimension::Api, Verdict::Pass, Some(e))
            .unwrap();
        assert_eq!(d.check_evidence_integrity().len(), 1);
    }

    #[test]
    fn test_extra_dim_decl() {
        let mut d = DomainSelfCheck::new();
        assert!(d
            .declare_extra_dim(ExtraDimension::RenderDeterminism, "r", "s", [0.9, 0.7, 0.5])
            .is_ok());
        assert_eq!(d.extra_declarations().len(), 1);
    }

    #[test]
    fn test_blocking_items() {
        let mut d = DomainSelfCheck::new();
        d.add_item("a", 1, Dimension::Api, Verdict::Fail, None).unwrap();
        d.add_item("b", 1, Dimension::Api, Verdict::Pass, Some(ev("e")))
            .unwrap();
        d.add_item("c", 1, Dimension::Api, Verdict::MissingEvidence, None)
            .unwrap();
        assert_eq!(d.blocking_items().len(), 2);
    }

    #[test]
    fn test_summary_by_group() {
        let mut d = DomainSelfCheck::new();
        d.add_item("a", 1, Dimension::Api, Verdict::Pass, Some(ev("e1")))
            .unwrap();
        d.add_item("b", 1, Dimension::Api, Verdict::Pass, Some(ev("e2")))
            .unwrap();
        d.add_item("c", 2, Dimension::Api, Verdict::Fail, None).unwrap();
        let sum = d.summary_by_group();
        assert_eq!(sum[&1u8], (2, 2));
        assert_eq!(sum[&2u8], (1, 0));
    }
}
