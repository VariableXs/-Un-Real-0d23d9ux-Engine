//! ve194_depgraph ↔ VE-F0194 域间依赖图
//!
//! A 域与后续各域（B 驱动适配族起）的依赖关系图显性登记：A 提供什么（冻结接口、
//! AdapterReport 契约、遥测总线）、依赖什么（宿主窗口系统、驱动环境）、
//! 禁止什么（A 不依赖后续域内部实现）；依赖变更走升版流程。
//!
//! 判据：三列表、循环阻断、违规登记、升版流程。

use alloc::format;
use alloc::vec;
use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 依赖关系类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelationKind {
    /// A 提供给下游的能力。
    Provides,
    /// A 依赖的上游能力。
    DependsOn,
    /// A 禁止的反向依赖。
    Forbidden,
}

/// 依赖边。
#[derive(Debug, Clone)]
pub struct DepEdge {
    /// 关系类型。
    pub kind: RelationKind,
    /// 目标域。
    pub target_domain: String,
    /// 能力/接口描述。
    pub capability: String,
    /// 是否已升版。
    pub versioned: bool,
}

/// 变更记录。
#[derive(Debug, Clone)]
pub struct ChangeRecord {
    /// 变更序号。
    pub seq: u64,
    /// 变更描述。
    pub desc: String,
    /// 变更时间。
    pub timestamp: u64,
}

/// 域间依赖图。
pub struct DomainDepGraph {
    /// 三列表（提供×依赖×禁止）。
    edges: Vec<DepEdge>,
    /// 变更记录。
    changes: Vec<ChangeRecord>,
    /// 当前版本。
    version: u64,
    /// 违规登记。
    violations: Vec<String>,
}

impl DomainDepGraph {
    /// 新建依赖图。
    pub fn new() -> Self {
        Self {
            edges: Vec::new(),
            changes: Vec::new(),
            version: 1,
            violations: Vec::new(),
        }
    }

    /// 登记提供能力。
    pub fn add_provides(&mut self, target_domain: &str, capability: &str) -> Result<(), u16> {
        if target_domain.is_empty() || capability.is_empty() {
            return Err(0x8601);
        }
        self.edges.push(DepEdge {
            kind: RelationKind::Provides,
            target_domain: target_domain.to_string(),
            capability: capability.to_string(),
            versioned: true,
        });
        self.version += 1;
        Ok(())
    }

    /// 登记依赖。
    pub fn add_depends(&mut self, target_domain: &str, capability: &str) -> Result<(), u16> {
        if target_domain.is_empty() || capability.is_empty() {
            return Err(0x8601);
        }
        self.edges.push(DepEdge {
            kind: RelationKind::DependsOn,
            target_domain: target_domain.to_string(),
            capability: capability.to_string(),
            versioned: true,
        });
        self.version += 1;
        Ok(())
    }

    /// 登记禁止反向依赖。
    pub fn add_forbidden(&mut self, target_domain: &str, capability: &str) -> Result<(), u16> {
        if target_domain.is_empty() || capability.is_empty() {
            return Err(0x8601);
        }
        self.edges.push(DepEdge {
            kind: RelationKind::Forbidden,
            target_domain: target_domain.to_string(),
            capability: capability.to_string(),
            versioned: true,
        });
        self.version += 1;
        Ok(())
    }

    /// 检查循环依赖：提供关系是否成环（简化检查——同域自依赖）。
    pub fn check_cycles(&self) -> Vec<String> {
        let mut issues = Vec::new();
        for edge in &self.edges {
            if edge.kind == RelationKind::Provides && edge.target_domain == "VE-A" {
                issues.push("发现自依赖: VE-A 提供给自己".to_string());
            }
        }
        issues
    }

    /// 违规登记（下游反向依赖 A 内部）。
    pub fn register_violation(&mut self, from_domain: &str, detail: &str) {
        self.violations
            .push(alloc::format!("违规: {} → {}", from_domain, detail));
    }

    /// 违规列表。
    pub fn violations(&self) -> &[String] {
        &self.violations
    }

    /// 升版流程（依赖变更）。
    pub fn bump_version(&mut self, desc: &str) -> u64 {
        self.changes.push(ChangeRecord {
            seq: self.changes.len() as u64,
            desc: desc.to_string(),
            timestamp: self.version,
        });
        self.version += 1;
        self.version
    }

    /// 当前版本。
    pub fn version(&self) -> u64 {
        self.version
    }

    /// 按类型列边。
    pub fn list_by_kind(&self, kind: RelationKind) -> Vec<&DepEdge> {
        self.edges.iter().filter(|e| e.kind == kind).collect()
    }

    /// 全量边。
    pub fn all_edges(&self) -> &[DepEdge] {
        &self.edges
    }

    /// 变更记录。
    pub fn change_log(&self) -> &[ChangeRecord] {
        &self.changes
    }

    /// 未登记依赖检查：传入实际依赖列表，找出未登记的。
    pub fn find_unregistered(&self, actual_deps: &[(RelationKind, &str, &str)]) -> Vec<String> {
        actual_deps
            .iter()
            .filter(|(kind, domain, cap)| {
                !self.edges.iter().any(|e| {
                    e.kind == *kind && e.target_domain == *domain && e.capability == *cap
                })
            })
            .map(|(kind, domain, cap)| {
                alloc::format!("未登记依赖: {:?} {} → {}", kind, domain, cap)
            })
            .collect()
    }
}

impl Default for DomainDepGraph {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_three_lists() {
        let mut g = DomainDepGraph::new();
        g.add_provides("B-driver", "冻结接口").unwrap();
        g.add_depends("宿主窗口系统", "窗口句柄").unwrap();
        g.add_forbidden("B-driver", "A 内部实现").unwrap();
        assert_eq!(g.list_by_kind(RelationKind::Provides).len(), 1);
        assert_eq!(g.list_by_kind(RelationKind::DependsOn).len(), 1);
        assert_eq!(g.list_by_kind(RelationKind::Forbidden).len(), 1);
    }

    #[test]
    fn test_cycle_detection() {
        let mut g = DomainDepGraph::new();
        g.add_provides("VE-A", "自环").unwrap(); // 自依赖
        assert_eq!(g.check_cycles().len(), 1);
    }

    #[test]
    fn test_violation_registration() {
        let mut g = DomainDepGraph::new();
        g.register_violation("B-driver", "直接访问 A 内部缓存");
        assert_eq!(g.violations().len(), 1);
    }

    #[test]
    fn test_bump_version() {
        let mut g = DomainDepGraph::new();
        let v = g.bump_version("新增对 C 域的依赖");
        assert_eq!(v, 2);
        assert_eq!(g.change_log().len(), 1);
    }

    #[test]
    fn test_find_unregistered() {
        let mut g = DomainDepGraph::new();
        g.add_provides("B", "iface").unwrap();
        let actual = vec![
            (RelationKind::Provides, "B", "iface"),
            (RelationKind::DependsOn, "X", "window"),
        ];
        let missing = g.find_unregistered(&actual);
        assert_eq!(missing.len(), 1);
        assert!(missing[0].contains("X"));
    }

    #[test]
    fn test_empty_rejected() {
        let mut g = DomainDepGraph::new();
        assert_eq!(g.add_provides("", "cap"), Err(0x8601));
    }
}
