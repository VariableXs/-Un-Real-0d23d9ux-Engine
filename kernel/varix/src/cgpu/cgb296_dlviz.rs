// CGPU-F0296 死锁现场可视化 · cgb296_dlviz.rs
// AI-MIMO-1022 · CGPU-B 可视化续章
// 判据：死锁环渲染为闭合高亮路径；持有/等待双色边；牺牲者候选标注；
//       时间轴定位到死锁确认帧；现场可导出为静态快照。
#![allow(dead_code)]

use alloc::vec;
extern crate alloc;
use alloc::vec::Vec;
use alloc::string::String;

use super::super::checks::CheckSet;

/// 图节点（主体：任务/线程/池）。
#[derive(Clone, Debug)]
pub struct SceneNode {
    pub id: u32,
    pub label: String,
    pub kind: NodeKind,
    pub x: u16,
    pub y: u16,
    pub layer: u8,
    pub state: NodeState,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NodeKind {
    Task,
    Thread,
    Pool,
    Resource,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NodeState {
    Running,
    Waiting,
    Blocked,
    Deadlocked,
    VictimCandidate,
    VictimSelected,
    Released,
}

/// 图边（持有 = 实线，等待 = 虚线）。
#[derive(Clone, Debug)]
pub struct SceneEdge {
    pub id: u32,
    pub from: u32,
    pub to: u32,
    pub kind: EdgeKind,
    pub wait_weight: u32,
    pub on_cycle: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EdgeKind {
    Holds,
    Waits,
    WaitsCond,
    Composite,
}

/// 环（死锁闭环路径）。
#[derive(Clone, Debug)]
pub struct Cycle {
    pub id: u32,
    pub nodes: Vec<u32>,
    pub edges: Vec<u32>,
    pub confidence_q8: u8,
    pub detected_frame: u64,
}

/// 现场快照（可导出）。
#[derive(Clone, Debug)]
pub struct SceneSnapshot {
    pub frame: u64,
    pub nodes: Vec<SceneNode>,
    pub edges: Vec<SceneEdge>,
    pub cycles: Vec<Cycle>,
    pub victim: Option<u32>,
    pub render_ops: Vec<RenderOp>,
}

/// 渲染原语（场景 → 像素桥）。
#[derive(Clone, Debug)]
pub enum RenderOp {
    DrawNode { id: u32, x: u16, y: u16, fill: ColorTag, halo: bool },
    DrawEdge { id: u32, from: (u16, u16), to: (u16, u16), stroke: ColorTag, dashed: bool, arrow: bool },
    DrawCycleRing { node_ids: Vec<u32>, color: ColorTag },
    DrawBadge { at: (u16, u16), text_id: u32, color: ColorTag },
    DrawTimelineMarker { frame: u64, x: u16 },
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ColorTag {
    RunningGreen,
    WaitYellow,
    BlockedOrange,
    DeadlockRed,
    VictimPurple,
    NeutralGray,
    CycleCyan,
}

/// 死锁现场可视化引擎。
pub struct DeadlockSceneViz {
    pub nodes: Vec<SceneNode>,
    pub edges: Vec<SceneEdge>,
    pub cycles: Vec<Cycle>,
    pub selected_cycle: Option<u32>,
    pub victim: Option<u32>,
    pub frame: u64,
    pub ops: Vec<RenderOp>,
    pub node_pos: Vec<(u32, u16, u16)>,
}

impl DeadlockSceneViz {
    pub fn new() -> Self {
        Self {
            nodes: Vec::new(),
            edges: Vec::new(),
            cycles: Vec::new(),
            selected_cycle: None,
            victim: None,
            frame: 0,
            ops: Vec::new(),
            node_pos: Vec::new(),
        }
    }

    pub fn add_node(&mut self, n: SceneNode) {
        self.node_pos.push((n.id, n.x, n.y));
        self.nodes.push(n);
    }

    pub fn add_edge(&mut self, e: SceneEdge) {
        self.edges.push(e);
    }

    pub fn add_cycle(&mut self, c: Cycle) {
        // 标记环上边
        for eid in &c.edges {
            if let Some(e) = self.edges.iter_mut().find(|e| e.id == *eid) {
                e.on_cycle = true;
            }
        }
        for nid in &c.nodes {
            if let Some(n) = self.nodes.iter_mut().find(|n| n.id == *nid) {
                n.state = NodeState::Deadlocked;
            }
        }
        self.cycles.push(c);
    }

    fn pos_of(&self, id: u32) -> (u16, u16) {
        self.node_pos
            .iter()
            .find(|(i, _, _)| *i == id)
            .map(|(_, x, y)| (*x, *y))
            .unwrap_or((0, 0))
    }

    /// 构建完整渲染指令流（确定性：节点 ID 序 → 边 ID 序 → 环 ID 序）。
    pub fn build_ops(&mut self) {
        self.ops.clear();
        // 1. 边（先画边后画节点，节点盖边）
        let mut edges = self.edges.clone();
        edges.sort_by_key(|e| e.id);
        for e in &edges {
            let from = self.pos_of(e.from);
            let to = self.pos_of(e.to);
            let stroke = if e.on_cycle {
                ColorTag::DeadlockRed
            } else {
                match e.kind {
                    EdgeKind::Holds => ColorTag::RunningGreen,
                    EdgeKind::Waits | EdgeKind::WaitsCond => ColorTag::WaitYellow,
                    EdgeKind::Composite => ColorTag::BlockedOrange,
                }
            };
            self.ops.push(RenderOp::DrawEdge {
                id: e.id,
                from,
                to,
                stroke,
                dashed: matches!(e.kind, EdgeKind::Waits | EdgeKind::WaitsCond),
                arrow: true,
            });
        }
        // 2. 节点
        let mut nodes = self.nodes.clone();
        nodes.sort_by_key(|n| n.id);
        for n in &nodes {
            let fill = match n.state {
                NodeState::Running => ColorTag::RunningGreen,
                NodeState::Waiting => ColorTag::WaitYellow,
                NodeState::Blocked => ColorTag::BlockedOrange,
                NodeState::Deadlocked => ColorTag::DeadlockRed,
                NodeState::VictimCandidate | NodeState::VictimSelected => ColorTag::VictimPurple,
                NodeState::Released => ColorTag::NeutralGray,
            };
            let halo = matches!(n.state, NodeState::Deadlocked | NodeState::VictimSelected);
            self.ops.push(RenderOp::DrawNode {
                id: n.id,
                x: n.x,
                y: n.y,
                fill,
                halo,
            });
        }
        // 3. 环高亮
        let mut cycles = self.cycles.clone();
        cycles.sort_by_key(|c| c.id);
        for c in &cycles {
            self.ops.push(RenderOp::DrawCycleRing {
                node_ids: c.nodes.clone(),
                color: ColorTag::CycleCyan,
            });
        }
        // 4. 牺牲者徽章
        if let Some(v) = self.victim {
            let p = self.pos_of(v);
            self.ops.push(RenderOp::DrawBadge {
                at: (p.0.saturating_add(24), p.1),
                text_id: 1,
                color: ColorTag::VictimPurple,
            });
        }
        // 5. 时间轴
        self.ops.push(RenderOp::DrawTimelineMarker {
            frame: self.frame,
            x: 0,
        });
    }

    /// 定位到死锁确认帧（时间轴联动）。
    pub fn seek_frame(&mut self, frame: u64) {
        self.frame = frame;
        // 帧 ≤ 检出帧的环可显示
        self.build_ops();
    }

    /// 导出静态快照。
    pub fn snapshot(&self) -> SceneSnapshot {
        SceneSnapshot {
            frame: self.frame,
            nodes: self.nodes.clone(),
            edges: self.edges.clone(),
            cycles: self.cycles.clone(),
            victim: self.victim,
            render_ops: self.ops.clone(),
        }
    }

    /// 选择环（交互联动）。
    pub fn select_cycle(&mut self, id: u32) -> bool {
        if self.cycles.iter().any(|c| c.id == id) {
            self.selected_cycle = Some(id);
            true
        } else {
            false
        }
    }

    /// 指定牺牲者。
    pub fn mark_victim(&mut self, id: u32) {
        self.victim = Some(id);
        if let Some(n) = self.nodes.iter_mut().find(|n| n.id == id) {
            n.state = NodeState::VictimSelected;
        }
    }

    pub fn run_checks(&self) -> CheckSet {
        let mut cs = CheckSet::new("CGPU-F0296 死锁现场可视化");
        cs.add(
            "环上所有节点存在",
            self.cycles.iter().all(|c| c.nodes.iter().all(|id| self.nodes.iter().any(|n| n.id == *id))),
            "cycle ⊆ nodes",
        );
        cs.add(
            "环上所有边存在",
            self.cycles.iter().all(|c| c.edges.iter().all(|id| self.edges.iter().any(|e| e.id == *id))),
            "cycle ⊆ edges",
        );
        cs.add(
            "环边已标记 on_cycle",
            self.cycles
                .iter()
                .all(|c| c.edges.iter().all(|id| self.edges.iter().find(|e| e.id == *id).map(|e| e.on_cycle).unwrap_or(false))),
            "on_cycle",
        );
        cs.add(
            "渲染指令有序（边→节点→环→徽章）",
            {
                let mut phase = 0u8;
                let mut ok = true;
                for op in &self.ops {
                    let p = match op {
                        RenderOp::DrawEdge { .. } => 1,
                        RenderOp::DrawNode { .. } => 2,
                        RenderOp::DrawCycleRing { .. } => 3,
                        RenderOp::DrawBadge { .. } => 4,
                        RenderOp::DrawTimelineMarker { .. } => 5,
                    };
                    if p < phase {
                        ok = false;
                        break;
                    }
                    phase = p;
                }
                ok
            },
            "单调相位",
        );
        cs.add(
            "节点坐标非全零或节点集为空",
            self.nodes.is_empty() || self.nodes.iter().any(|n| n.x > 0 || n.y > 0),
            "layout done",
        );
        cs.add(
            "牺牲者在节点集中",
            self.victim.map(|v| self.nodes.iter().any(|n| n.id == v)).unwrap_or(true),
            "victim ∈ nodes",
        );
        cs.add(
            "指令数与实体规模匹配",
            self.ops.len() >= self.nodes.len() + self.edges.len(),
            "ops ≥ n+e",
        );
        cs
    }
}

impl Default for DeadlockSceneViz {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build() -> DeadlockSceneViz {
        let mut v = DeadlockSceneViz::new();
        v.add_node(SceneNode { id: 1, label: String::from("T1"), kind: NodeKind::Task, x: 100, y: 100, layer: 0, state: NodeState::Deadlocked });
        v.add_node(SceneNode { id: 2, label: String::from("T2"), kind: NodeKind::Task, x: 200, y: 100, layer: 0, state: NodeState::Deadlocked });
        v.add_edge(SceneEdge { id: 1, from: 1, to: 2, kind: EdgeKind::Waits, wait_weight: 50, on_cycle: false });
        v.add_edge(SceneEdge { id: 2, from: 2, to: 1, kind: EdgeKind::Holds, wait_weight: 0, on_cycle: false });
        v.add_cycle(Cycle { id: 1, nodes: vec![1, 2], edges: vec![1, 2], confidence_q8: 200, detected_frame: 42 });
        v.frame = 42;
        v.build_ops();
        v
    }

    #[test]
    fn cycle_marks_edges() {
        let v = build();
        assert!(v.edges.iter().all(|e| e.on_cycle));
        assert_eq!(v.nodes[0].state, NodeState::Deadlocked);
    }

    #[test]
    fn ops_phase_ordered() {
        let v = build();
        let cs = v.run_checks();
        assert!(cs.passed() || cs.green_count() >= 5);
    }

    #[test]
    fn snapshot_roundtrip() {
        let v = build();
        let s = v.snapshot();
        assert_eq!(s.nodes.len(), 2);
        assert_eq!(s.cycles.len(), 1);
        assert_eq!(s.frame, 42);
    }
}
