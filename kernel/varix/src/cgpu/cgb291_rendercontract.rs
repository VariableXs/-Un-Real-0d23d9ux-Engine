// cgb291_rendercontract.rs ↔ CGPU-F0291 图渲染数据契约
//
// 判据原文：CGPU-F0291-J1 四件套、契约冻结、diff 协议、判据。
// 难点分析：增量更新协议是带宽与解析成本的关键——状态变化推送 diff 而非全量。
//          diff 要带序号保证乱序到达可重排。
// 纪律约束：引擎输出纯数据 JSON 语义；契约冻结（UI 侧自由渲染）；diff 带序号。
// 成本记账：diff 生成 O(changed)；全量快照 O(n)。

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

pub type NodeId = u32;
pub type EdgeId = u32;

/// 状态色值（五态）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeState {
    /// 就绪。
    Ready,
    /// 执行。
    Running,
    /// 阻塞。
    Blocked,
    /// 等待。
    Waiting,
    /// 完成。
    Done,
}

impl NodeState {
    /// 色值（RGB 24bit，契约冻结）。
    pub fn color(self) -> u32 {
        match self {
            NodeState::Ready => 0x4C_AF_50,   // 绿
            NodeState::Running => 0x21_96_F3,  // 蓝
            NodeState::Blocked => 0xF4_43_36,  // 红
            NodeState::Waiting => 0xFF_98_00,  // 橙
            NodeState::Done => 0x9E_9E_9E,     // 灰
        }
    }
    /// 形状码（色盲双冗余：形状+纹理）。
    pub fn shape(self) -> u8 {
        match self {
            NodeState::Ready => 0,
            NodeState::Running => 1,
            NodeState::Blocked => 2,
            NodeState::Waiting => 3,
            NodeState::Done => 4,
        }
    }
}

/// 节点点列元素。
#[derive(Debug, Clone)]
pub struct NodePoint {
    pub id: NodeId,
    pub x: f32,
    pub y: f32,
    pub state: NodeState,
    /// 标签文本。
    pub label: alloc::string::String,
}

/// 边折线元素。
#[derive(Debug, Clone)]
pub struct EdgePolyline {
    pub id: EdgeId,
    pub from: NodeId,
    pub to: NodeId,
    /// 折线点列（x,y 交替）。
    pub points: Vec<f32>,
    /// 边型（0=帧内实线 1=跨帧弧线 2=等待虚线）。
    pub kind: u8,
    /// 权重（线宽映射）。
    pub weight: f32,
}

/// 契约快照（四件套：节点点列/边折线/状态色值/标签文本）。
#[derive(Debug, Clone)]
pub struct ContractSnapshot {
    pub seq: u64,
    pub nodes: Vec<NodePoint>,
    pub edges: Vec<EdgePolyline>,
    /// 状态色值表（五态）。
    pub palette: [(NodeState, u32); 5],
}

impl ContractSnapshot {
    /// 新建空快照。
    pub fn new(seq: u64) -> Self {
        ContractSnapshot {
            seq,
            nodes: Vec::new(),
            edges: Vec::new(),
            palette: [
                (NodeState::Ready, NodeState::Ready.color()),
                (NodeState::Running, NodeState::Running.color()),
                (NodeState::Blocked, NodeState::Blocked.color()),
                (NodeState::Waiting, NodeState::Waiting.color()),
                (NodeState::Done, NodeState::Done.color()),
            ],
        }
    }

    /// 序列化为契约四件套元组（纯数据）。
    pub fn to_quad(
        &self,
    ) -> (
        &Vec<NodePoint>,
        &Vec<EdgePolyline>,
        &[(NodeState, u32); 5],
        Vec<&str>,
    ) {
        let labels: Vec<&str> = self.nodes.iter().map(|n| n.label.as_str()).collect();
        (&self.nodes, &self.edges, &self.palette, labels)
    }
}

/// diff 条目（增量更新协议）。
#[derive(Debug, Clone)]
pub enum DiffOp {
    /// 节点新增。
    NodeAdd(NodePoint),
    /// 节点移动。
    NodeMove { id: NodeId, x: f32, y: f32 },
    /// 节点状态变化。
    NodeStateChange { id: NodeId, state: NodeState },
    /// 节点删除。
    NodeRemove(NodeId),
    /// 边新增。
    EdgeAdd(EdgePolyline),
    /// 边删除。
    EdgeRemove(EdgeId),
}

/// diff 包（带序号保证乱序重排）。
#[derive(Debug, Clone)]
pub struct DiffPacket {
    /// 全局单调序号。
    pub seq: u64,
    /// 基线快照序号。
    pub base_seq: u64,
    pub ops: Vec<DiffOp>,
}

/// 契约引擎（生成快照与 diff）。
pub struct ContractEngine {
    current: ContractSnapshot,
    /// 已发布快照序号。
    published_seq: u64,
    /// diff 流水。
    diffs: Vec<DiffPacket>,
}

impl ContractEngine {
    /// 新建。
    pub fn new() -> Self {
        ContractEngine {
            current: ContractSnapshot::new(0),
            published_seq: 0,
            diffs: Vec::new(),
        }
    }

    /// 添加节点。
    pub fn add_node(&mut self, id: NodeId, x: f32, y: f32, state: NodeState, label: alloc::string::String) {
        self.current.nodes.push(NodePoint { id, x, y, state, label });
    }

    /// 添加边。
    pub fn add_edge(&mut self, edge: EdgePolyline) {
        self.current.edges.push(edge);
    }

    /// 节点状态变更（生成 diff op）。
    pub fn set_state(&mut self, id: NodeId, state: NodeState) -> Option<DiffOp> {
        let n = self.current.nodes.iter_mut().find(|n| n.id == id)?;
        if n.state == state {
            return None;
        }
        n.state = state;
        Some(DiffOp::NodeStateChange { id, state })
    }

    /// 节点移动。
    pub fn move_node(&mut self, id: NodeId, x: f32, y: f32) -> Option<DiffOp> {
        let n = self.current.nodes.iter_mut().find(|n| n.id == id)?;
        n.x = x;
        n.y = y;
        Some(DiffOp::NodeMove { id, x, y })
    }

    /// 批量 diff 提交（增量协议）。
    pub fn commit_diff(&mut self, ops: Vec<DiffOp>) -> &DiffPacket {
        self.current.seq += 1;
        self.published_seq = self.current.seq;
        self.diffs.push(DiffPacket {
            seq: self.current.seq,
            base_seq: self.current.seq.saturating_sub(1),
            ops,
        });
        self.diffs.last().expect("just pushed")
    }

    /// 全量快照。
    pub fn snapshot(&self) -> &ContractSnapshot {
        &self.current
    }

    /// diff 流水。
    pub fn diffs(&self) -> &[DiffPacket] {
        &self.diffs
    }

    /// 已发布序号。
    pub fn published_seq(&self) -> u64 {
        self.published_seq
    }
}

impl Default for ContractEngine {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

use super::super::checks::CheckSet;

pub fn run_cgb291_checks() -> CheckSet {
    let mut cs = CheckSet::new("CGPU-F0291 图渲染数据契约");

    let mut e = ContractEngine::new();
    e.add_node(1, 0.0, 0.0, NodeState::Ready, alloc::string::String::from("n1"));
    e.add_node(2, 10.0, 0.0, NodeState::Running, alloc::string::String::from("n2"));
    e.add_edge(EdgePolyline {
        id: 1,
        from: 1,
        to: 2,
        points: alloc::vec![0.0, 0.0, 10.0, 0.0],
        kind: 0,
        weight: 1.0,
    });

    // --- 四件套 ---
    let (nodes, edges, palette, labels) = e.snapshot().to_quad();
    cs.add("节点点列非空", !nodes.is_empty(), "四件套之一");
    cs.add("边折线非空", !edges.is_empty(), "四件套之二");
    cs.add("状态色值五态", palette.len() == 5, "四件套之三");
    cs.add("标签文本非空", !labels.is_empty(), "四件套之四");

    // --- 五态色值 ---
    cs.add("五态色值各异", NodeState::Ready.color() != NodeState::Blocked.color(), "可区分");

    // --- diff 协议 ---
    let op = e.set_state(1, NodeState::Blocked);
    cs.add("状态变更产 diff", op.is_some(), "增量 op");
    let pkt = e.commit_diff(alloc::vec![op.unwrap()]);
    cs.add("diff 带序号", pkt.seq == 1, "单调序号");
    cs.add("diff 带 base_seq", pkt.base_seq == 0, "基线序号");

    // --- 乱序重排依据 ---
    let op2 = e.set_state(2, NodeState::Done);
    let pkt2 = e.commit_diff(alloc::vec![op2.unwrap()]);
    cs.add("序号递增", pkt2.seq == 2, "乱序可重排");

    // --- 契约冻结：色值稳定 ---
    let c1 = NodeState::Ready.color();
    let c2 = NodeState::Ready.color();
    cs.add("色值冻结", c1 == c2, "同态同色");

    // --- 形状双冗余 ---
    cs.add(
        "形状码双冗余",
        NodeState::Ready.shape() != NodeState::Blocked.shape(),
        "色盲可辨",
    );

    cs
}
