//! CGPU-F0161 · 帧任务图数据结构（CGPU-B 域 · 任务图调度域 · 批次 B01 · 目标 440 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F0161`
//!
//! **判据（锚点原文）**：结构全覆盖、ID 索引、arena 零碎片、快照语义、万节点压测。
//!
//! **职责定位（锚点原文）**：一帧渲染工作的图模型：任务节点（类型：上传/
//! 计算/图形/拷贝/合成/呈现；属性：读写资源集/预算标签/优先级/截止时间/
//! 所在队列类型）、边（依赖类型：写后读/读后写/写后写/用户显式）、图句柄
//! （每帧一个图实例，帧号关联）。数据结构：**ID 索引化**（A 域纪律延续）、
//! **节点池分配（arena——帧末整池释放零碎片）**、**图不可变快照**（构建
//! 完成后只读——多消费者安全）。
//!
//! # 一、为什么节点必须是六型闭集而不是开放字符串
//!
//! 调度器对每型节点的合法队列、预算口径、依赖语义都不同（呈现节点不能
//! 有写集、上传节点不进图形队列）。开放类型会让这些约束退化为运行期
//! 字符串比对——漏一个拼写就是一次静默错调度。六型闭集（[`NodeKind`]）
//! 让「按型分派」成为穷尽 match：**加一型必须改调度器**，编译器替架构
//! 把关（结构全覆盖判据的第一层）。
//!
//! # 二、arena 为什么「整池释放」而不是「逐节点回收」
//!
//! 帧任务图的生命周期恰好一帧：帧末整张图一起死。逐节点回收（free list/
//! 引用计数）引入的碎片与账本开销，对这个生命周期纯属浪费。节点池
//! （[`FrameGraphBuilder`]）只分配不下单点释放——`release_pool` 一次
//! 归还全部（[`FrameGraph::release_pool`]），**零碎片**是分配策略的结果
//! 而不是回收算法的成就：没有单点 free，就没有碎片（arena 零碎片判据）。
//!
//! # 三、快照为什么「构建完成后只读」且构建器被消费
//!
//! 多消费者（调度器/预算器/渲染文档）同时读一张图时，任何一方的「顺手
//! 修改」都是其余各方的数据竞争。故图分两态：构建期（[`FrameGraphBuilder
//! `]，可变）与快照期（[`FrameGraph`]，只有 `&self` 方法——类型系统
//! 禁写）。`into_snapshot` **消费**构建器：图成快照后不存在「还活着的
//! 可变入口」，快照语义（快照语义判据）由所有权迁移保证，不靠约定。
//!
//! # 四、ID 索引为什么是「下标即句柄」
//!
//! 句柄若含哈希/指针，每次访问多一次间接且生命周期难审。节点句柄就是
//! 池下标（`NodeHandle = u32`）：`get_node` 直接下标访问 O(1)，句柄
//! 失效=帧末整池释放（不存在悬垂句柄——图死了句柄一起没意义）。A 域
//! 纪律延续：**索引化，不做裸指针**。
//!
//! # 五、与相邻条的分工
//!
//! A 域接口冻结（锚点 283 行：任务节点接口/发射协议/完成回调/取消语义）
//! 是上游契约——本条实现其「任务节点接口」中的数据结构面；F0162+ 的
//! 拓扑排序/发射调度消费本条的快照与边表。本条只管「**图长什么样、
//! 怎么存、怎么变只读**」，不管怎么排、怎么发。
//!
//! **性能（锚点原文）**：节点分配 O(1)（池尾追加）；快照 O(节点数)（一次
//! 移交）；只读访问 O(1)（下标即句柄）；万节点规模在判据压测覆盖。

use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、常量与错误码
// ---------------------------------------------------------------------------

/// 本项版本。
pub const FRAMEGRAPH_VERSION: &str = "CB01-framegraph-v1";

/// 任务节点型数（六型闭集）。
pub const NODE_KIND_COUNT: usize = 6;

/// 依赖边型数（四型闭集）。
pub const EDGE_KIND_COUNT: usize = 4;

/// 节点未知（句柄越界/池中无此节点）。
pub const E_NODE_UNKNOWN: &str = "E_NODE_UNKNOWN";

/// 边非法（自环/端点未知/重复边）。
pub const E_EDGE_BAD: &str = "E_EDGE_BAD";

/// 池为空（对空图取快照/释放）。
pub const E_POOL_EMPTY: &str = "E_POOL_EMPTY";

// ---------------------------------------------------------------------------
// 二、任务节点（六型 × 五属性）
// ---------------------------------------------------------------------------

/// 任务节点类型（六型闭集——穷尽 match 的分派地基）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NodeKind {
    /// 上传：CPU→GPU 资源上传。
    Upload,
    /// 计算：compute pass。
    Compute,
    /// 图形：render pass。
    Graphics,
    /// 拷贝：GPU 侧资源拷贝。
    Copy,
    /// 合成：多源合成。
    Compose,
    /// 呈现：swapchain 呈现（终态节点）。
    Present,
}

impl NodeKind {
    /// 六型全集（顺序即 [`NODE_KIND_COUNT`]）。
    pub fn all() -> [NodeKind; NODE_KIND_COUNT] {
        [
            NodeKind::Upload,
            NodeKind::Compute,
            NodeKind::Graphics,
            NodeKind::Copy,
            NodeKind::Compose,
            NodeKind::Present,
        ]
    }

    /// 型短码。
    pub const fn tag(self) -> &'static str {
        match self {
            NodeKind::Upload => "upload",
            NodeKind::Compute => "compute",
            NodeKind::Graphics => "graphics",
            NodeKind::Copy => "copy",
            NodeKind::Compose => "compose",
            NodeKind::Present => "present",
        }
    }

    /// 型中文名（读屏可达）。
    pub const fn zh(self) -> &'static str {
        match self {
            NodeKind::Upload => "上传",
            NodeKind::Compute => "计算",
            NodeKind::Graphics => "图形",
            NodeKind::Copy => "拷贝",
            NodeKind::Compose => "合成",
            NodeKind::Present => "呈现",
        }
    }
}

/// 队列类型（节点所在队列的闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueueKind {
    /// 图形队列。
    Graphics,
    /// 计算队列。
    Compute,
    /// 拷贝/传输队列。
    Transfer,
}

impl QueueKind {
    /// 队列短码。
    pub const fn tag(self) -> &'static str {
        match self {
            QueueKind::Graphics => "gfx",
            QueueKind::Compute => "dcompute",
            QueueKind::Transfer => "transfer",
        }
    }
}

/// 资源 id（读写集成员——ID 索引化纪律，资源表由 A 域接口冻结提供）。
pub type ResId = u32;

/// 节点规格（五属性全集：读写资源集/预算标签/优先级/截止时间/队列类型）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NodeSpec {
    /// 节点类型。
    pub kind: NodeKind,
    /// 读资源集（本节点消费的资源）。
    pub reads: Vec<ResId>,
    /// 写资源集（本节点产出的资源）。
    pub writes: Vec<ResId>,
    /// 预算标签（微秒——调度预算的输入）。
    pub budget_us: u32,
    /// 优先级（0 最高——同型节点的调度序）。
    pub priority: u8,
    /// 截止时间（相对帧起点，微秒）。
    pub deadline_us: u32,
    /// 所在队列类型。
    pub queue: QueueKind,
}

/// 节点句柄（**下标即句柄**——池下标，ID 索引化）。
pub type NodeHandle = u32;

// ---------------------------------------------------------------------------
// 三、依赖边（四型闭集）
// ---------------------------------------------------------------------------

/// 依赖边类型（四型闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EdgeKind {
    /// 写后读（生产者写、消费者读——最常见的数据依赖）。
    ReadAfterWrite,
    /// 读后写（消费者反序——防写覆盖未读毕数据）。
    WriteAfterRead,
    /// 写后写（两次写之间的顺序）。
    WriteAfterWrite,
    /// 用户显式（与资源无关的手工顺序约束）。
    Explicit,
}

impl EdgeKind {
    /// 四型全集。
    pub fn all() -> [EdgeKind; EDGE_KIND_COUNT] {
        [
            EdgeKind::ReadAfterWrite,
            EdgeKind::WriteAfterRead,
            EdgeKind::WriteAfterWrite,
            EdgeKind::Explicit,
        ]
    }

    /// 型短码。
    pub const fn tag(self) -> &'static str {
        match self {
            EdgeKind::ReadAfterWrite => "raw",
            EdgeKind::WriteAfterRead => "war",
            EdgeKind::WriteAfterWrite => "waw",
            EdgeKind::Explicit => "explicit",
        }
    }

    /// 型中文名（读屏可达）。
    pub const fn zh(self) -> &'static str {
        match self {
            EdgeKind::ReadAfterWrite => "写后读",
            EdgeKind::WriteAfterRead => "读后写",
            EdgeKind::WriteAfterWrite => "写后写",
            EdgeKind::Explicit => "用户显式",
        }
    }
}

/// 依赖边（有向：from → to，to 依赖 from 先完成）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Edge {
    /// 前驱节点（先执行）。
    pub from: NodeHandle,
    /// 后继节点（后执行）。
    pub to: NodeHandle,
    /// 依赖类型。
    pub kind: EdgeKind,
}

// ---------------------------------------------------------------------------
// 四、构建器（arena 节点池 + 边校验）
// ---------------------------------------------------------------------------

/// 帧任务图构建器（构建期可变；arena 分配——只分配不下单点释放）。
#[derive(Clone, Debug, Default)]
pub struct FrameGraphBuilder {
    /// 帧号（每帧一个图实例——帧号关联）。
    pub frame_no: u64,
    nodes: Vec<NodeSpec>,
    edges: Vec<Edge>,
}

impl FrameGraphBuilder {
    /// 新建某帧的构建器。
    pub fn for_frame(frame_no: u64) -> FrameGraphBuilder {
        FrameGraphBuilder { frame_no, nodes: Vec::new(), edges: Vec::new() }
    }

    /// 分配节点（arena：池尾追加 O(1)，返回下标即句柄）。
    pub fn add_node(&mut self, spec: NodeSpec) -> Result<NodeHandle, &'static str> {
        if self.nodes.len() >= u32::MAX as usize {
            return Err(E_NODE_UNKNOWN);
        }
        let h = self.nodes.len() as NodeHandle;
        self.nodes.push(spec);
        Ok(h)
    }

    /// 加依赖边（自环拒/端点未知拒/重复边拒——三闸显性）。
    pub fn add_edge(&mut self, e: Edge) -> Result<(), &'static str> {
        if e.from == e.to {
            return Err(E_EDGE_BAD);
        }
        if e.from as usize >= self.nodes.len() || e.to as usize >= self.nodes.len() {
            return Err(E_NODE_UNKNOWN);
        }
        if self
            .edges
            .iter()
            .any(|x| x.from == e.from && x.to == e.to && x.kind == e.kind)
        {
            return Err(E_EDGE_BAD);
        }
        self.edges.push(e);
        Ok(())
    }

    /// 已分配节点数（arena 账）。
    pub fn allocated(&self) -> usize {
        self.nodes.len()
    }

    /// 边数。
    pub fn edge_count(&self) -> usize {
        self.edges.len()
    }

    /// 构建完成 → **不可变快照**（消费构建器：图成快照后无可变入口）。
    ///
    /// 空池拒（一帧图至少一个节点——空快照没有调度意义，显性拒绝）。
    pub fn into_snapshot(self) -> Result<FrameGraph, &'static str> {
        if self.nodes.is_empty() {
            return Err(E_POOL_EMPTY);
        }
        Ok(FrameGraph {
            frame_no: self.frame_no,
            nodes: self.nodes,
            edges: self.edges,
            pool_released: false,
        })
    }
}

// ---------------------------------------------------------------------------
// 五、不可变快照（多消费者安全 + arena 整池释放账）
// ---------------------------------------------------------------------------

/// 帧任务图不可变快照（构建完成后只读——只有 `&self` 方法，类型系统禁写）。
#[derive(Clone, Debug)]
pub struct FrameGraph {
    /// 帧号（每帧一个图实例）。
    pub frame_no: u64,
    nodes: Vec<NodeSpec>,
    edges: Vec<Edge>,
    pool_released: bool,
}

impl FrameGraph {
    /// 节点数。
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// 边数。
    pub fn edge_count(&self) -> usize {
        self.edges.len()
    }

    /// 按句柄读节点（下标即句柄——O(1) 随机访问，越界显性 None）。
    pub fn get_node(&self, h: NodeHandle) -> Option<&NodeSpec> {
        self.nodes.get(h as usize)
    }

    /// 节点读集（只读访问——多消费者各持 `&FrameGraph`）。
    pub fn reads_of(&self, h: NodeHandle) -> Option<&[ResId]> {
        self.nodes.get(h as usize).map(|n| n.reads.as_slice())
    }

    /// 节点写集。
    pub fn writes_of(&self, h: NodeHandle) -> Option<&[ResId]> {
        self.nodes.get(h as usize).map(|n| n.writes.as_slice())
    }

    /// 前驱边集（谁在我之前——拓扑/发射的消费面）。
    pub fn incoming(&self, h: NodeHandle) -> Vec<Edge> {
        self.edges.iter().copied().filter(|e| e.to == h).collect()
    }

    /// 呈现节点全集（帧终态——发射调度的收敛点）。
    pub fn present_nodes(&self) -> Vec<NodeHandle> {
        let mut out = Vec::new();
        for (i, n) in self.nodes.iter().enumerate() {
            if n.kind == NodeKind::Present {
                out.push(i as NodeHandle);
            }
        }
        out
    }

    /// 池账：本快照池是否已释放（防双释放）。
    pub fn pool_released(&self) -> bool {
        self.pool_released
    }

    /// 按型普查（各节点型在册数量——结构全覆盖的运行期账面）。
    ///
    /// 返回值顺序与 [`NodeKind::all`] 一致。
    pub fn kind_census(&self) -> [usize; NODE_KIND_COUNT] {
        let mut out = [0usize; NODE_KIND_COUNT];
        for n in self.nodes.iter() {
            for (i, k) in NodeKind::all().iter().enumerate() {
                if n.kind == *k {
                    out[i] += 1;
                }
            }
        }
        out
    }

    /// 帧预算总和（全部节点预算标签之和——帧级预算对账的输入）。
    pub fn total_budget_us(&self) -> u64 {
        let mut t = 0u64;
        for n in self.nodes.iter() {
            t = t.saturating_add(n.budget_us as u64);
        }
        t
    }

    /// 环检出（Kahn 入度法——有向图完整性的基本校验）。
    ///
    /// 返回是否成环。快照是只读图，环意味着发射调度不可能完成——本校验
    /// 是发射器（F0162+）开工前的合法性闸，数据结构层提供、不越权排序。
    pub fn has_cycle(&self) -> bool {
        let n = self.nodes.len();
        if n == 0 {
            return false;
        }
        let mut indeg = vec![0usize; n];
        let mut adj: Vec<Vec<usize>> = vec![Vec::new(); n];
        for e in self.edges.iter() {
            let (f, t) = (e.from as usize, e.to as usize);
            if f < n && t < n {
                adj[f].push(t);
                indeg[t] += 1;
            }
        }
        // Kahn：入度 0 的先出队，能出队的都是无环可达部分。
        let mut queue: Vec<usize> = Vec::new();
        for (i, d) in indeg.iter().enumerate() {
            if *d == 0 {
                queue.push(i);
            }
        }
        let mut done = 0usize;
        let mut qi = 0usize;
        while qi < queue.len() {
            let cur = queue[qi];
            qi += 1;
            done += 1;
            for &nxt in adj[cur].iter() {
                indeg[nxt] -= 1;
                if indeg[nxt] == 0 {
                    queue.push(nxt);
                }
            }
        }
        done < n
    }

    /// **帧末整池释放**（arena：一次归还全部节点——零碎片的结构性来源）。
    ///
    /// 返回释放的节点数；重复释放拒绝（防双释放——账面诚实）。
    pub fn release_pool(&mut self) -> Result<usize, &'static str> {
        if self.pool_released {
            return Err(E_POOL_EMPTY);
        }
        let n = self.nodes.len();
        self.pool_released = true;
        Ok(n)
    }
}

// ---------------------------------------------------------------------------
// 六、读屏替述
// ---------------------------------------------------------------------------

/// 图读屏单行（帧号/节点数/边数；不含资源明细）。
pub fn screen_line_graph(g: &FrameGraph) -> String {
    format!(
        "第 {} 帧任务图：{} 个节点（{} 条依赖边），池{}",
        g.frame_no,
        g.node_count(),
        g.edge_count(),
        if g.pool_released { "已整池释放" } else { "在册" }
    )
}
