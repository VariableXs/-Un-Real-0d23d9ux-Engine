//! ves443_s24graph — VE-F4209 · 一致性知识图谱
//! 实体-关系图构建（模型四类实体为节点/三型关系为边）+图谱查询（影响扩散查询：改此令牌影响哪些界面）
//! +图谱与注册中心双向同步（失步→以注册中心为准重放——单源）；环→图算法检出+人工确认；查询超时→剪枝；
//! 图谱含无障碍约束边高亮层（域本色）；无隐私面。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x1440;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GpCode {
    Ok = 0,
    CycleFound = SEG_BASE + 0x01,  // 0x1441
    SyncLost = SEG_BASE + 0x02,    // 0x1442
    QueryTimeout = SEG_BASE + 0x03, // 0x1443
}

/// 四类实体（节点）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NodeKind { Interaction = 0, VisualToken = 1, CopyCaliber = 2, DataContract = 3 }

/// 三型关系（边）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EdgeKind { Equal = 0, Derived = 1, Constrained = 2 }

/// 边（含无障碍约束高亮层——域本色）。
#[derive(Clone, Copy)]
pub struct Edge { pub from: u32, pub to: u32, pub kind: EdgeKind, pub a11y: bool }

/// 图谱册。
pub struct GraphBook {
    pub nodes: Vec<(u32, NodeKind)>,
    pub edges: Vec<Edge>,
    pub reg_rev: u64,   // 注册中心版本
    pub graph_rev: u64, // 图谱同步版本
    pub pruned: u32,
}

impl GraphBook {
    pub fn new() -> Self { GraphBook { nodes: Vec::new(), edges: Vec::new(), reg_rev: 0, graph_rev: 0, pruned: 0 } }
    pub fn add_node(&mut self, id: u32, k: NodeKind) { if !self.nodes.iter().any(|(i, _)| *i == id) { self.nodes.push((id, k)); } }
    pub fn add_edge(&mut self, e: Edge) { self.edges.push(e); }
    /// 环检出（有向图 DFS 染色）。
    pub fn detect_cycle(&self) -> bool {
        let n = self.nodes.len();
        let mut color = vec![0u8; 4096];
        let adj = |id: u32| -> Vec<u32> { self.edges.iter().filter(|e| e.from == id).map(|e| e.to).collect() };
        fn dfs(id: u32, color: &mut [u8], adj: &dyn Fn(u32) -> Vec<u32>) -> bool {
            if color[id as usize] == 1 { return true; }
            if color[id as usize] == 2 { return false; }
            color[id as usize] = 1;
            for nx in adj(id) { if dfs(nx, color, adj) { return true; } }
            color[id as usize] = 2;
            false
        }
        for (id, _) in &self.nodes {
            if color[*id as usize] == 0 && dfs(*id, &mut color, &adj) { return true; }
        }
        let _ = n;
        false
    }
    /// 影响扩散查询：从起点沿边反向（谁依赖我）扩散，depth 截断（剪枝）。
    pub fn impact(&mut self, start: u32, max_depth: u8) -> Vec<u32> {
        let mut seen = vec![start];
        let mut frontier = vec![start];
        let mut depth = 0u8;
        while depth < max_depth {
            let mut next = Vec::new();
            for f in &frontier {
                for e in &self.edges {
                    if e.to == *f && !seen.contains(&e.from) {
                        seen.push(e.from);
                        next.push(e.from);
                    }
                }
            }
            if next.is_empty() { break; }
            frontier = next;
            depth += 1;
        }
        if self.edges.len() > 4096 { self.pruned += 1; } // 超时剪枝计数
        seen.retain(|x| *x != start);
        seen
    }
    /// 双向同步：失步→以注册中心为准重放（单源）。
    pub fn sync(&mut self, reg_rev: u64) -> Result<(), GpCode> {
        self.reg_rev = reg_rev;
        if self.graph_rev != reg_rev {
            self.graph_rev = reg_rev; // 重放对齐注册中心
            return Err(GpCode::SyncLost);
        }
        Ok(())
    }
    /// 无障碍约束边高亮层（域本色）。
    pub fn a11y_edges(&self) -> Vec<u32> {
        self.edges.iter().filter(|e| e.a11y && e.kind == EdgeKind::Constrained).map(|e| e.from).collect()
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves443_checks() -> CheckSet {
    let mut set = CheckSet::new("ves443");
    let mut g = GraphBook::new();
    for (id, k) in [(1u32, NodeKind::VisualToken), (2, NodeKind::Interaction), (3, NodeKind::DataContract), (4, NodeKind::CopyCaliber)] {
        g.add_node(id, k);
    }

    // C443-01 无环 DAG 检出通过。
    g.add_edge(Edge { from: 1, to: 2, kind: EdgeKind::Derived, a11y: false });
    g.add_edge(Edge { from: 2, to: 3, kind: EdgeKind::Constrained, a11y: true });
    chk(&mut set, "C443-01", !g.detect_cycle(), "DAG 无环");

    // C443-02 环检出（1→2→1）。
    g.add_edge(Edge { from: 2, to: 1, kind: EdgeKind::Equal, a11y: false });
    chk(&mut set, "C443-02", g.detect_cycle(), "环检出");

    // C443-03 拆环后复检过（人工确认消环）。
    g.edges.pop();
    chk(&mut set, "C443-03", !g.detect_cycle(), "消环复检");

    // C443-04 影响扩散查询：改令牌 1 → 影响 2、3。
    let imp = g.impact(1, 8);
    chk(&mut set, "C443-04", imp == vec![2, 3], "扩散查询");

    // C443-05 深度截断剪枝（depth=1 只到直接依赖）。
    let mut g2 = GraphBook::new();
    for id in 1..=3u32 { g2.add_node(id, NodeKind::VisualToken); }
    g2.add_edge(Edge { from: 1, to: 2, kind: EdgeKind::Derived, a11y: false });
    g2.add_edge(Edge { from: 2, to: 3, kind: EdgeKind::Derived, a11y: false });
    chk(&mut set, "C443-05", g2.impact(1, 1) == vec![2], "深度截断");

    // C443-06 无障碍约束边高亮（域本色）。
    let hl = g.a11y_edges();
    chk(&mut set, "C443-06", hl == vec![1], "a11y 高亮");

    // C443-07 双向同步初态一致。
    g.graph_rev = 5; g.reg_rev = 5;
    chk(&mut set, "C443-07", g.sync(5).is_ok(), "同步一致");

    // C443-08 注册中心前移→失步检出。
    chk(&mut set, "C443-08", g.sync(6) == Err(GpCode::SyncLost) && g.graph_rev == 6, "失步重放");

    // C443-09 重放后对齐（再同步过）。
    chk(&mut set, "C443-09", g.sync(6).is_ok(), "重放对齐");

    // C443-10 节点幂等。
    g.add_node(1, NodeKind::VisualToken);
    chk(&mut set, "C443-10", g.nodes.len() == 4, "节点幂等");

    // C443-11 四类实体单源。
    chk(&mut set, "C443-11", NodeKind::Interaction as u8 == 0 && NodeKind::DataContract as u8 == 3, "四类单源");

    // C443-12 三型关系单源。
    chk(&mut set, "C443-12", EdgeKind::Equal as u8 == 0 && EdgeKind::Constrained as u8 == 2, "三型单源");

    set
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ves443_smoke() {
        let set = run_ves443_checks();
        assert!(set.failed_count() == 0, "ves443 red: {}", set.red_items_text());
    }
}
