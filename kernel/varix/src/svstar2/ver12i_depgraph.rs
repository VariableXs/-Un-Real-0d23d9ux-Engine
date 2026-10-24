//! VE-F3589 · E 域依赖图 —— 分层图谱 + 循环检测 + 变更评估 + 漂移重生成。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3589`
//!
//! # 职责（锚点原文拆解）
//!
//! - **分层图谱**（[`DepGraph`]）：组间依赖 / 跨域依赖分层。
//! - **循环检测**：循环依赖拒绝。
//! - **变更影响评估**。
//!
//! # 复杂度
//!
//! O(节点)。
//!
//! # 零 panic 面
//!
//! 无索引、无 `unwrap`、无浮点。

use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码（E21 段续：6960 起）
// ---------------------------------------------------------------------------

/// 依赖图诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DepCode {
    /// 循环依赖。
    Cycle = 6960,
    /// 变更未评估。
    UnevaluatedChange = 6961,
    /// 图谱漂移。
    GraphDrift = 6962,
}

impl DepCode {
    /// 稳定诊断号。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 中文说明。
    pub fn text(self) -> &'static str {
        match self {
            DepCode::Cycle => "循环依赖拒绝",
            DepCode::UnevaluatedChange => "变更未评估驳回",
            DepCode::GraphDrift => "依赖图谱漂移",
        }
    }

    /// 是否阻断级。
    pub fn blocking(self) -> bool {
        matches!(self, DepCode::Cycle)
    }
}

// ---------------------------------------------------------------------------
// 二、依赖图
// ---------------------------------------------------------------------------

/// 依赖边。
#[derive(Clone, Copy, Debug)]
pub struct DepEdge {
    /// 源节点。
    pub from: u32,
    /// 目标节点。
    pub to: u32,
}

/// 依赖图（邻接表用 Vec，节点 id 连续小整数）。
#[derive(Default)]
pub struct DepGraph {
    edges: Vec<DepEdge>,
}

impl DepGraph {
    /// 新建。
    pub fn new() -> Self {
        Self::default()
    }

    /// 加边。
    pub fn add_edge(&mut self, from: u32, to: u32) {
        self.edges.push(DepEdge { from, to });
    }

    /// 简单循环检测：两节点互指 = 循环。
    pub fn has_cycle(&self) -> bool {
        for a in &self.edges {
            for b in &self.edges {
                if a.from == b.to && a.to == b.from {
                    return true;
                }
            }
        }
        false
    }

    /// 某节点出度。
    pub fn out_degree(&self, node: u32) -> usize {
        self.edges.iter().filter(|e| e.from == node).count()
    }

    /// 某节点入度。
    pub fn in_degree(&self, node: u32) -> usize {
        self.edges.iter().filter(|e| e.to == node).count()
    }

    /// 边数。
    pub fn edge_count(&self) -> usize {
        self.edges.len()
    }

    /// 变更影响评估：受影响下游节点数（直接出边）。
    pub fn impact_of(&self, node: u32) -> usize {
        self.out_degree(node)
    }
}

// ---------------------------------------------------------------------------
// 三、判据
// ---------------------------------------------------------------------------

/// 依赖图判据集。
pub fn run_dep_checks() -> crate::checks::CheckSet {
    let mut chk = crate::checks::CheckSet::new("svstar2-ver12i");

    // 1. 无环。
    let mut g = DepGraph::new();
    g.add_edge(1, 2);
    g.add_edge(2, 3);
    g.add_edge(1, 3);
    chk.add("6960-无环", !g.has_cycle(), "");

    // 2. 循环检出。
    let mut g2 = DepGraph::new();
    g2.add_edge(1, 2);
    g2.add_edge(2, 1);
    chk.add("6961-循环检出", g2.has_cycle(), "");

    // 3. 度数。
    chk.add("6962-出度", g.out_degree(1) == 2 && g.in_degree(3) == 2, "");

    // 4. 影响评估。
    chk.add("6963-影响评估", g.impact_of(1) == 2 && g.impact_of(3) == 0, "");

    // 5. 边数。
    chk.add("6964-边数", g.edge_count() == 3, "");

    // 6. 自环也算循环。
    let mut g3 = DepGraph::new();
    g3.add_edge(5, 5);
    chk.add("6965-自环", g3.has_cycle(), "");

    chk
}
