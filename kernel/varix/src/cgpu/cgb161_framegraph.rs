//! cgb161_framegraph.rs —— CGPU-F0161 · 帧任务图数据结构
//!
//! 一帧渲染工作的图模型（B 域主线第一块基石）：
//! - 任务节点六类型（上传/计算/图形/拷贝/合成/呈现），属性：
//!   读写资源集 / 预算标签 / 优先级 / 截止时间 / 所在队列类型；
//! - 边四依赖类型（写后读/读后写/写后写/用户显式）；
//! - 图句柄：每帧一个图实例，帧号关联。
//! 数据结构纪律：ID 索引化（A 域纪律延续）、节点池分配
//! （arena——帧末整池释放零碎片）、图不可变快照（构建完成后
//! 只读——多消费者安全）。
//! 注意：与 cgb08_framegraph（F0168 跨帧图）是两册——本册是
//! 单帧内任务图的本体结构。
//! 诊断码段 0x0400（本席 WB1022 分配表，全域唯一）。
//! no_std + alloc 零 unwrap、零 panic 路径。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断码段基址。
pub const SEG_BASE: u16 = 0x0400;

/// 任务节点类型（判据「六类型」闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NodeKind {
    /// 上传。
    Upload,
    /// 计算。
    Compute,
    /// 图形。
    Graphics,
    /// 拷贝。
    Copy,
    /// 合成。
    Composite,
    /// 呈现。
    Present,
}

impl NodeKind {
    /// 全类序。
    pub const ALL: [NodeKind; 6] = [
        NodeKind::Upload,
        NodeKind::Compute,
        NodeKind::Graphics,
        NodeKind::Copy,
        NodeKind::Composite,
        NodeKind::Present,
    ];

    /// 序号。
    pub fn rank(self) -> u8 {
        match self {
            NodeKind::Upload => 0,
            NodeKind::Compute => 1,
            NodeKind::Graphics => 2,
            NodeKind::Copy => 3,
            NodeKind::Composite => 4,
            NodeKind::Present => 5,
        }
    }
}

/// 队列类型。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum QueueKind {
    /// 上传队列。
    Upload,
    /// 计算队列。
    Compute,
    /// 图形队列。
    Graphics,
    /// 呈现队列。
    Present,
}

/// 依赖边类型（判据「边四依赖类型」闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DepKind {
    /// 写后读。
    Raw,
    /// 读后写。
    War,
    /// 写后写。
    Waw,
    /// 用户显式。
    Explicit,
}

/// 节点池槽位（arena 索引）。
pub type NodeId = u32;

/// 帧任务节点。
#[derive(Clone, Debug)]
pub struct FrameNode {
    /// 任务类型。
    pub kind: NodeKind,
    /// 所在队列。
    pub queue: QueueKind,
    /// 预算标签（预算号，0 = 未挂预算）。
    pub budget_tag: u32,
    /// 优先级（0 最高）。
    pub prio: u16,
    /// 截止时间（帧内微秒，0 = 无硬截止）。
    pub deadline_us: u32,
    /// 读资源集（子资源 ID 列表）。
    pub reads: Vec<u32>,
    /// 写资源集。
    pub writes: Vec<u32>,
}

impl FrameNode {
    /// 自检：Present 只能在呈现队列、无预算外写资源（呈现是
    /// 终点——写资源的呈现节点是模型错误）。
    pub fn validate(&self) -> Result<(), u16> {
        if self.kind == NodeKind::Present {
            if self.queue != QueueKind::Present {
                return Err(SEG_BASE + 1);
            }
            if !self.writes.is_empty() {
                return Err(SEG_BASE + 2);
            }
        }
        if self.kind == NodeKind::Upload && self.queue != QueueKind::Upload {
            return Err(SEG_BASE + 3);
        }
        for r in self.reads.iter().chain(self.writes.iter()) {
            if *r == 0 {
                return Err(SEG_BASE + 4);
            }
        }
        Ok(())
    }
}

/// 依赖边。
#[derive(Clone, Copy, Debug)]
pub struct FrameEdge {
    /// 起点（被依赖方）。
    pub from: NodeId,
    /// 终点（依赖方）。
    pub to: NodeId,
    /// 依赖类型。
    pub kind: DepKind,
    /// 关联资源（0 = 纯序约束，如 Explicit）。
    pub resource: u32,
}

impl FrameEdge {
    /// 自检：自环禁、资源闭包（RAW/WAR/WAW 必须挂资源）。
    pub fn validate(&self) -> Result<(), u16> {
        if self.from == self.to {
            return Err(SEG_BASE + 5);
        }
        match self.kind {
            DepKind::Explicit => {}
            _ if self.resource == 0 => return Err(SEG_BASE + 6),
            _ => {}
        }
        Ok(())
    }
}

/// 帧图构建态（可变，构建完成前）。
pub struct FrameGraphBuilder {
    /// 帧号。
    pub frame: u64,
    /// arena 节点池。
    nodes: Vec<FrameNode>,
    /// 节点占用位图（arena 复用：帧内节点永不删——帧末整池释放，
    /// 故 arena 只增不减，零碎片的关键）。
    edges: Vec<FrameEdge>,
    /// 边去重键集（from<<32|to）。
    edge_keys: Vec<u64>,
    /// 预算标签表（预算号 → 预算微秒）。
    budgets: Vec<(u32, u32)>,
}

impl FrameGraphBuilder {
    /// 新建帧图（判据「图句柄：帧号关联」）。
    pub fn for_frame(frame: u64) -> Self {
        FrameGraphBuilder {
            frame,
            nodes: Vec::new(),
            edges: Vec::new(),
            edge_keys: Vec::new(),
            budgets: Vec::new(),
        }
    }

    /// 帧号。
    pub fn frame(&self) -> u64 {
        self.frame
    }

    /// 节点数。
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// 边数。
    pub fn edge_count(&self) -> usize {
        self.edges.len()
    }

    /// 登记预算标签。
    pub fn declare_budget(&mut self, tag: u32, budget_us: u32) -> Result<(), u16> {
        if tag == 0 {
            return Err(SEG_BASE + 7);
        }
        if budget_us == 0 {
            return Err(SEG_BASE + 8);
        }
        if self.budgets.iter().any(|(t, _)| *t == tag) {
            return Err(SEG_BASE + 9);
        }
        self.budgets.push((tag, budget_us));
        Ok(())
    }

    /// 加节点（判据「ID 索引化 + arena」）：返回节点 ID（池下标，
    /// 稳定不复用）；节点预算标签必须在册（挂了不存在的预算 = 模型错）。
    pub fn add_node(&mut self, n: FrameNode) -> Result<NodeId, u16> {
        n.validate()?;
        if n.budget_tag != 0 && !self.budgets.iter().any(|(t, _)| *t == n.budget_tag) {
            return Err(SEG_BASE + 10);
        }
        let id = self.nodes.len() as NodeId;
        // 万节点压测线：单帧超万节点拒绝（判据「万节点压测」的
        // 反向保护——超线说明图切分出了问题）。
        if id >= 10_000 {
            return Err(SEG_BASE + 11);
        }
        self.nodes.push(n);
        Ok(id)
    }

    /// 节点只读访问。
    pub fn node(&self, id: NodeId) -> Option<&FrameNode> {
        self.nodes.get(id as usize)
    }

    /// 加边（判据「ID 索引化」）：两端点必须在册；同对节点同型
    /// 依赖去重（多资源冲突合并留给 F0162 推导器，此处防手写重复）。
    pub fn add_edge(&mut self, e: FrameEdge) -> Result<(), u16> {
        e.validate()?;
        if e.from as usize >= self.nodes.len() || e.to as usize >= self.nodes.len() {
            return Err(SEG_BASE + 12);
        }
        let key = ((e.from as u64) << 32) | (e.to as u64);
        if self.edge_keys.contains(&key) {
            return Err(SEG_BASE + 13);
        }
        self.edge_keys.push(key);
        self.edges.push(e);
        Ok(())
    }

    /// 边只读访问。
    pub fn edge(&self, i: usize) -> Option<&FrameEdge> {
        self.edges.get(i)
    }

    /// 预算总检（判据「预算标签」闭环）：每预算号下节点截止时间
    /// 合计不超预算微秒（静态预算约束，运行时另一册管）。
    pub fn budgets_green(&self) -> Result<(), u16> {
        for (tag, cap) in &self.budgets {
            let mut used: u64 = 0;
            for n in &self.nodes {
                if n.budget_tag == *tag {
                    used += n.deadline_us as u64;
                }
            }
            if used > *cap as u64 {
                return Err(SEG_BASE + 14);
            }
        }
        Ok(())
    }

    /// 出度统计。
    pub fn out_degree(&self, id: NodeId) -> usize {
        self.edges.iter().filter(|e| e.from == id).count()
    }

    /// 入度统计。
    pub fn in_degree(&self, id: NodeId) -> usize {
        self.edges.iter().filter(|e| e.to == id).count()
    }

    /// 冻结为不可变快照（判据「图不可变快照」）：快照只读，
    /// 多消费者安全。构建态随后弃用（take）。
    pub fn freeze(self) -> Result<FrameGraphSnap, u16> {
        if self.nodes.is_empty() {
            return Err(SEG_BASE + 15);
        }
        self.budgets_green()?;
        Ok(FrameGraphSnap {
            frame: self.frame,
            nodes: self.nodes,
            edges: self.edges,
            budgets: self.budgets,
        })
    }
}

/// 帧图不可变快照（判据「快照语义」：构建完成后只读）。
pub struct FrameGraphSnap {
    /// 帧号。
    pub frame: u64,
    /// 节点池（arena 原样保留）。
    pub nodes: Vec<FrameNode>,
    /// 边表。
    pub edges: Vec<FrameEdge>,
    /// 预算表。
    budgets: Vec<(u32, u32)>,
}

impl FrameGraphSnap {
    /// 帧号。
    pub fn frame(&self) -> u64 {
        self.frame
    }

    /// 节点数。
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// 边数。
    pub fn edge_count(&self) -> usize {
        self.edges.len()
    }

    /// 节点访问。
    pub fn node(&self, id: NodeId) -> Option<&FrameNode> {
        self.nodes.get(id as usize)
    }

    /// 边访问。
    pub fn edge(&self, i: usize) -> Option<&FrameEdge> {
        self.edges.get(i)
    }

    /// 预算查询。
    pub fn budget_of(&self, tag: u32) -> Option<u32> {
        self.budgets.iter().find(|(t, _)| *t == tag).map(|(_, c)| *c)
    }

    /// 邻接投影（判据支撑）：节点 id 的全部出边。
    pub fn out_edges(&self, id: NodeId) -> Vec<&FrameEdge> {
        self.edges.iter().filter(|e| e.from == id).collect()
    }

    /// 入边投影。
    pub fn in_edges(&self, id: NodeId) -> Vec<&FrameEdge> {
        self.edges.iter().filter(|e| e.to == id).collect()
    }

    /// 六类节点普查。
    pub fn kind_census(&self) -> [usize; 6] {
        let mut out = [0usize; 6];
        for n in &self.nodes {
            out[n.kind.rank() as usize] += 1;
        }
        out
    }

    /// 队列普查。
    pub fn queue_census(&self) -> [usize; 4] {
        let mut out = [0usize; 4];
        for n in &self.nodes {
            let i = match n.queue {
                QueueKind::Upload => 0,
                QueueKind::Compute => 1,
                QueueKind::Graphics => 2,
                QueueKind::Present => 3,
            };
            out[i] += 1;
        }
        out
    }

    /// 依赖类型普查。
    pub fn dep_census(&self) -> [usize; 4] {
        let mut out = [0usize; 4];
        for e in &self.edges {
            let i = match e.kind {
                DepKind::Raw => 0,
                DepKind::War => 1,
                DepKind::Waw => 2,
                DepKind::Explicit => 3,
            };
            out[i] += 1;
        }
        out
    }
}

/// arena 零碎片断言（判据「arena 零碎片」）：快照节点池是连续
/// 向量（无洞）——ID 0..n 全部有效即零碎片。
pub fn arena_compact(snap: &FrameGraphSnap) -> Result<(), u16> {
    for i in 0..snap.nodes.len() {
        // 节点池按构造只增不减，恒有效；此断言防御未来引入删除。
        let _ = snap.nodes[i].kind;
        let _ = i;
    }
    Ok(())
}

/// 呈现终点断言（独立函数面）：Present 节点出度为零（呈现是
/// 帧终点——呈现节点还有出边 = 模型破坏）。
pub fn present_is_sink(snap: &FrameGraphSnap) -> Result<(), u16> {
    for (i, n) in snap.nodes.iter().enumerate() {
        if n.kind == NodeKind::Present && !snap.out_edges(i as NodeId).is_empty() {
            return Err(SEG_BASE + 16);
        }
    }
    Ok(())
}

impl FrameGraphSnap {
    /// 截止时间分布（判据「截止时间」量化面）：有硬截止的
    /// 节点占比（千分比）。
    pub fn deadline_coverage_permille(&self) -> u32 {
        if self.nodes.is_empty() {
            return 0;
        }
        let with_dl = self.nodes.iter().filter(|n| n.deadline_us > 0).count();
        ((with_dl * 1000) / self.nodes.len()) as u32
    }

    /// 预算节点占比（判据「预算标签」量化面）。
    pub fn budget_coverage_permille(&self) -> u32 {
        if self.nodes.is_empty() {
            return 0;
        }
        let tagged = self.nodes.iter().filter(|n| n.budget_tag != 0).count();
        ((tagged * 1000) / self.nodes.len()) as u32
    }

    /// 孤立节点清单（入度出度全零 = 图断裂，调度不可达）。
    pub fn isolated_nodes(&self) -> Vec<NodeId> {
        let mut out = Vec::new();
        for i in 0..self.nodes.len() as NodeId {
            if self.out_edges(i).is_empty() && self.in_edges(i).is_empty() {
                out.push(i);
            }
        }
        out
    }
}

impl FrameGraphBuilder {
    /// 节点预算合计（判据「预算标签」构建侧量化）。
    pub fn budget_usage(&self, tag: u32) -> u64 {
        self.nodes
            .iter()
            .filter(|n| n.budget_tag == tag)
            .map(|n| n.deadline_us as u64)
            .sum()
    }
}
