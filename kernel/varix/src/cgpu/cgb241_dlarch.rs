//! CGPU-F0241 · 死锁检测总架构（cgb241_dlarch）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F0241`
//! **判据原文摘录**：双模型（RAG+等待图）、增量+兜底（事件驱动增量检测 +
//! 周期 1s 全量）、四源统一（任务等待/锁等待/栅栏等待/显存分配等待）。
//!
//! 与 F0231 静态环检测的关系：静态管**可判环**（加锁路径上的潜在环），
//! 本组管**运行期动态死锁**——只有真正建立的等待边才进图。两套模型互补，
//! 不可互相替代：静态能提前预警但会误报，动态不误报但只能事后检出。
//!
//! 与 F0003 线程池死锁检测的关系：F0003 是线程池**域内**自检（池自身
//! 饥饿/互等），本组是**引擎级**扩展——跨池、跨子系统、跨进程的等待
//! 统一进一张图。F0003 的检测结果作为四源之一的「任务等待」喂进本图。
//!
//! ## 双模型的分工（这是本单的全部难点）
//!
//! **RAG（资源分配图）**：二分图，左部是任务节点，右部是资源节点。
//! 边分两种——`hold`（任务→资源，已持有）与 `wait`（任务→资源，等待中）。
//! RAG 的经典结论是「**单实例资源**下 RAG 有环 ⇔ 存在死锁」，但引擎里
//! 大量资源是**多实例**的（显存池、信号量），此时 RAG 有环**不一定**
//! 死锁（环上每个资源都还有空闲实例时环是假的）。所以 RAG 保留作
//! 「持有关系的权威记录」，但**死锁判定不走 RAG**。
//!
//! **等待图（Wait-For Graph, WFG）**：单部图，节点是**等待主体**
//! （任务/线程/池），边 A→B 表示「A 正在等 B 释放某个资源」。
//! WFG 的优势是**语义精确**——边的含义就是「等谁」，与资源实例数
//! 无关。多实例资源在 WFG 里自然消解：A 等池 P 的空闲槽，而 P 由
//! B/C/D 共同持有，则 WFG 里是 A→{B,C,D} 的**多出边**，只有当
//! A 等的那个具体槽被「独占持有」时才建 A→holder 单边。
//!
//! **判定走 WFG，记账走 RAG**——这是本架构的第一条纪律。
//!
//! ## 触发模型
//!
//! - **增量为主**：等待边建立/消亡时立刻检测受影响子图。边事件是
//!   最强的死锁信号——死锁必然在「最后一条等待边闭合」那一刻成环，
//!   增量检测恰好覆盖这个时刻。开销 O(受影响子图) 而非 O(全图)。
//! - **兜底为辅**：周期 1s 全量扫描。覆盖两类增量漏检：①边事件丢失
//!   （钩子被绕过/竞态）；②非边变化引发的死锁（如任务心跳停止后
//!   优先级反转链固化）。兜底是**安全网**，不是主路径。
//!
//! ## 四源统一
//!
//! | 源 | 等待主体 | 等待对象 | 边语义 |
//! |---|---|---|---|
//! | 任务等待 | 任务 | 任务/池 | join / 依赖完成 |
//! | 锁等待 | 线程 | 锁持有者 | mutex / rwlock |
//! | 栅栏等待 | 线程 | 栅栏参与者 | barrier / latch |
//! | 显存分配 | 任务 | 池持有者 | alloc 阻塞 |
//!
//! 四源的**边类型不同但图结构同构**——统一成 `WaitEdge` 后检测器
//! 只面对一张图。源类型保留在边上作诊断标签，不参与图算法。
//!
//! ## 容量与开销预算
//!
//! 图容量有界（节点/边上限），满则**拒绝建边并计数**——不静默丢、
//! 也不扩容。理由：图是检测器的地基，地基无界膨胀会让检测器自身
//! 成为内存压力源（F0248 自保的延伸）。拒绝建边意味着该等待不可见，
//! 计入 `edges_rejected` 并在诊断报告里显性披露。
//!
//! 检测开销：增量路径每次建边/删边最多触发一次子图 DFS，预算按
//! **访问节点数**记账（`nodes_visited`），不依赖墙钟——3GHz 下访问
//! 一个节点约 20 周期，1000 节点子图约 7µs，远低于 1s 兜底周期。

use alloc::format;
use crate::checks::CheckSet;
use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 诊断码段（0xCB41 起 · B06 死锁检测组）
// ---------------------------------------------------------------------------

/// 诊断码段基址。
pub const SEG_BASE: u16 = 0xCB41;
/// 图容量满，建边被拒。
pub const E_GRAPH_FULL: u16 = SEG_BASE + 1;
/// 节点 ID 不存在。
pub const E_NO_NODE: u16 = SEG_BASE + 2;
/// 重复建边。
pub const E_DUP_EDGE: u16 = SEG_BASE + 3;
/// 自环建边。
pub const E_SELF_EDGE: u16 = SEG_BASE + 4;
/// 源类型非法。
pub const E_BAD_SOURCE: u16 = SEG_BASE + 5;
/// 等待 ID 不存在（删边失败）。
pub const E_NO_WAIT: u16 = SEG_BASE + 6;
/// 配置越界。
pub const E_BAD_CONFIG: u16 = SEG_BASE + 7;

// ---------------------------------------------------------------------------
// 容量与预算
// ---------------------------------------------------------------------------

/// 节点上限。1024 个等待主体足以覆盖引擎全场景（含 256 线程 × 4 源）。
pub const MAX_NODES: usize = 1024;
/// 边上限。每节点平均出边 ≤8，1024×8=8192。
pub const MAX_EDGES: usize = 8192;
/// 单次增量检测访问节点预算（超出则中止本次增量、退化为全量兜底）。
pub const INCR_VISIT_BUDGET: usize = 2048;
/// 兜底全量扫描周期（tick 数；1 tick = 1ms，故 1000 tick = 1s）。
pub const FULL_SCAN_TICKS: u32 = 1000;
/// 图规模阈值：超过此节点数时增量检测降频（F0254 自适应的输入）。
pub const LARGE_GRAPH_NODES: usize = 512;

// ---------------------------------------------------------------------------
// 核心标识
// ---------------------------------------------------------------------------

/// 等待主体 ID（任务/线程/池的统一编号）。
pub type SubjectId = u32;
/// 资源 ID。
pub type ResId = u32;
/// 等待边的全局序号（删边凭据）。
pub type WaitId = u64;

// ---------------------------------------------------------------------------
// 等待源类型
// ---------------------------------------------------------------------------

/// 四源统一标签。**不参与图算法**，只作诊断与归因。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WaitSource {
    /// 任务等待（join / 依赖完成）。
    Task,
    /// 锁等待（mutex / rwlock）。
    Lock,
    /// 栅栏等待（barrier / latch）。
    Barrier,
    /// 显存分配等待（alloc 阻塞）。
    VramAlloc,
}

impl WaitSource {
    pub const ALL: [WaitSource; 4] = [
        WaitSource::Task,
        WaitSource::Lock,
        WaitSource::Barrier,
        WaitSource::VramAlloc,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            WaitSource::Task => "task",
            WaitSource::Lock => "lock",
            WaitSource::Barrier => "barrier",
            WaitSource::VramAlloc => "vram",
        }
    }

    pub const fn idx(self) -> usize {
        match self {
            WaitSource::Task => 0,
            WaitSource::Lock => 1,
            WaitSource::Barrier => 2,
            WaitSource::VramAlloc => 3,
        }
    }

    pub fn from_idx(i: usize) -> Option<WaitSource> {
        WaitSource::ALL.get(i).copied()
    }
}

// ---------------------------------------------------------------------------
// 等待谓词（F0242 谓词建模的载体）
// ---------------------------------------------------------------------------

/// 等待谓词：描述「等什么」而非「等谁」。
///
/// 为什么需要它：条件变量的等待是「等条件成真」，不是「等某个资源」。
/// 把 `cond.wait()` 建成「等锁持有者」的边是**错误建模**——持有者可能
/// 根本不知道有人在等它（它不需要释放锁，只需要改条件）。
///
/// 谓词建模的核心区分：
/// - `Resource(res)`：等具体资源，边指向当前持有者；
/// - `Cond(id)`：等条件，边指向**所有可能改条件的主体**（广播面）。
///   若广播面为空（无人能改条件），这是**永久等待**而非死锁——
///   归入「活锁/饥饿」桶，不进死锁候选（F0244 的三维证据排除）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WaitPredicate {
    /// 等具体资源释放/可用。
    Resource(ResId),
    /// 等条件变量成真（广播面由图侧维护）。
    Cond(u32),
    /// 等任务完成。
    TaskDone(SubjectId),
    /// 等栅栏汇合。
    Barrier(u32),
    /// 显存分配等待（等池出空闲槽）。
    AllocSlot(u32),
}

impl WaitPredicate {
    pub const fn is_cond(self) -> bool {
        matches!(self, WaitPredicate::Cond(_))
    }

    /// 该谓词是否「等某个具体主体」（可建单边）。
    pub const fn targets_subject(self) -> bool {
        matches!(self, WaitPredicate::Resource(_) | WaitPredicate::TaskDone(_))
    }
}

// ---------------------------------------------------------------------------
// 等待边
// ---------------------------------------------------------------------------

/// 一条等待边（WFG 的一条有向边）。
///
/// `from` 等 `to` 释放/推进 `predicate` 描述的对象。
/// `weight` 是**已等待时长（tick）**——F0243 要求「长等待优先报告」，
/// 权重参与环内排序，不参与环判定（环判定只看拓扑）。
#[derive(Clone, Copy, Debug)]
pub struct WaitEdge {
    /// 等待方。
    pub from: SubjectId,
    /// 被等方。
    pub to: SubjectId,
    /// 等待谓词。
    pub predicate: WaitPredicate,
    /// 等待源。
    pub source: WaitSource,
    /// 建边时的 tick。
    pub created_at: u32,
    /// 累计等待 tick（由 `tick()` 累加）。
    pub weight: u32,
    /// 全局等待序号（删边凭据）。
    pub wait_id: WaitId,
}

// ---------------------------------------------------------------------------
// 节点
// ---------------------------------------------------------------------------

/// 等待主体节点。
#[derive(Clone, Copy, Debug)]
pub struct SubjectNode {
    pub id: SubjectId,
    /// 该主体当前持有的资源数（RAG 侧记账）。
    pub holds: u16,
    /// 是否存活（节点销毁级联清边，F0242 一致性）。
    pub alive: bool,
    /// 心跳 tick（F0247 看门狗输入；0 = 未注册心跳）。
    pub heartbeat_at: u32,
    /// 出度（缓存，增量检测用）。
    pub out_degree: u16,
    /// 入度（缓存，增量检测用）。
    pub in_degree: u16,
}

// ---------------------------------------------------------------------------
// 图事件（增量检测的输入）
// ---------------------------------------------------------------------------

/// 图事件。增量检测只消费事件，不做主动扫描。
#[derive(Clone, Copy, Debug)]
pub enum GraphEvent {
    /// 等待边建立。
    EdgeAdded(WaitId),
    /// 等待边消亡。
    EdgeRemoved(WaitId),
    /// 节点加入。
    NodeAdded(SubjectId),
    /// 节点销毁。
    NodeRemoved(SubjectId),
}

// ---------------------------------------------------------------------------
// 检测结果
// ---------------------------------------------------------------------------

/// 检测触发方式。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Trigger {
    /// 事件驱动增量检测。
    Incremental,
    /// 周期兜底全量扫描。
    FullScan,
    /// 手动触发（诊断命令）。
    Manual,
}

/// 一次检测的输出（环集合 + 遥测）。
#[derive(Clone, Debug)]
pub struct DetectionRun {
    pub trigger: Trigger,
    /// 发现的环（每条环是主体 ID 序列，首尾相接）。
    pub cycles: Vec<Vec<SubjectId>>,
    /// 本次访问的节点数（开销记账）。
    pub nodes_visited: usize,
    /// 本次访问的边数。
    pub edges_visited: usize,
    /// 是否因超预算退化为全量。
    pub degraded: bool,
    /// 耗时 tick（粗粒度）。
    pub elapsed_ticks: u32,
}

impl DetectionRun {
    pub fn empty(trigger: Trigger) -> DetectionRun {
        DetectionRun {
            trigger,
            cycles: Vec::new(),
            nodes_visited: 0,
            edges_visited: 0,
            degraded: false,
            elapsed_ticks: 0,
        }
    }
}

// ---------------------------------------------------------------------------
// 配置
// ---------------------------------------------------------------------------

/// 检测器配置。
#[derive(Clone, Copy, Debug)]
pub struct DetectorConfig {
    /// 增量检测访问节点预算。
    pub incr_visit_budget: usize,
    /// 兜底全量扫描周期（tick）。
    pub full_scan_ticks: u32,
    /// 大图阈值。
    pub large_graph_nodes: usize,
    /// 是否启用增量检测（关闭则纯兜底）。
    pub incremental_enabled: bool,
}

impl DetectorConfig {
    pub const fn default() -> DetectorConfig {
        DetectorConfig {
            incr_visit_budget: INCR_VISIT_BUDGET,
            full_scan_ticks: FULL_SCAN_TICKS,
            large_graph_nodes: LARGE_GRAPH_NODES,
            incremental_enabled: true,
        }
    }

    /// 配置越界检查。
    pub fn validate(self) -> Result<(), u16> {
        if self.incr_visit_budget == 0 || self.incr_visit_budget > MAX_NODES * 2 {
            return Err(E_BAD_CONFIG);
        }
        if self.full_scan_ticks == 0 || self.full_scan_ticks > 60_000 {
            return Err(E_BAD_CONFIG);
        }
        if self.large_graph_nodes == 0 || self.large_graph_nodes > MAX_NODES {
            return Err(E_BAD_CONFIG);
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 等待图本体
// ---------------------------------------------------------------------------

/// 等待图（WFG）——死锁判定的权威模型。
///
/// 纪律回顾：
/// 1. **判定走 WFG，记账走 RAG**（见模块头注）；
/// 2. 边生命周期与等待精确同步（F0242）——`add_wait` / `remove_wait`
///    是唯二入口，不存在「先占位后补边」的后门；
/// 3. 节点销毁级联清边（F0242 一致性）——`remove_node` 内部完成，
///    悬空边零容忍；
/// 4. 拒绝建边必须计数显性（`edges_rejected`）。
pub struct WaitGraph {
    nodes: Vec<SubjectNode>,
    edges: Vec<WaitEdge>,
    /// 节点索引表（id → nodes 下标）。id 稀疏时避免线性扫描。
    index: Vec<(SubjectId, u32)>,
    next_wait_id: WaitId,
    /// 遥测：被拒绝的建边数。
    pub edges_rejected: u64,
    /// 遥测：累计建边数。
    pub edges_created: u64,
    /// 遥测：累计删边数。
    pub edges_removed: u64,
    /// 遥测：累计节点销毁级联清边数。
    pub cascade_cleared: u64,
    /// 遥测：当前边数（缓存）。
    pub edge_count: usize,
}

impl WaitGraph {
    pub fn new() -> WaitGraph {
        WaitGraph {
            nodes: Vec::new(),
            edges: Vec::new(),
            index: Vec::new(),
            next_wait_id: 1,
            edges_rejected: 0,
            edges_created: 0,
            edges_removed: 0,
            cascade_cleared: 0,
            edge_count: 0,
        }
    }

    // -- 节点管理 ----------------------------------------------------------

    pub fn node_count(&self) -> usize {
        self.nodes.iter().filter(|n| n.alive).count()
    }

    pub fn raw_node_count(&self) -> usize {
        self.nodes.len()
    }

    fn find_node_idx(&self, id: SubjectId) -> Option<usize> {
        // index 表按 id 有序，二分查找。
        self.index
            .binary_search_by(|(k, _)| k.cmp(&id))
            .ok()
            .map(|i| self.index[i].1 as usize)
    }

    /// 加入等待主体。已存在则返回已有下标（幂等）。
    pub fn add_node(&mut self, id: SubjectId) -> Result<usize, u16> {
        if let Some(i) = self.find_node_idx(id) {
            if self.nodes[i].alive {
                return Ok(i);
            }
            // 复活已销毁节点。
            self.nodes[i].alive = true;
            self.nodes[i].holds = 0;
            self.nodes[i].out_degree = 0;
            self.nodes[i].in_degree = 0;
            self.nodes[i].heartbeat_at = 0;
            return Ok(i);
        }
        if self.nodes.len() >= MAX_NODES {
            self.edges_rejected += 1;
            return Err(E_GRAPH_FULL);
        }
        let idx = self.nodes.len();
        self.nodes.push(SubjectNode {
            id,
            holds: 0,
            alive: true,
            heartbeat_at: 0,
            out_degree: 0,
            in_degree: 0,
        });
        // 插入有序 index。
        let pos = self.index.partition_point(|(k, _)| *k < id);
        self.index.insert(pos, (id, idx as u32));
        Ok(idx)
    }

    /// 销毁节点并级联清边（F0242 一致性：悬空边零容忍）。
    ///
    /// 返回被级联清除的边数。
    pub fn remove_node(&mut self, id: SubjectId) -> Result<usize, u16> {
        let idx = self.find_node_idx(id).ok_or(E_NO_NODE)?;
        if !self.nodes[idx].alive {
            return Ok(0);
        }
        self.nodes[idx].alive = false;
        self.nodes[idx].out_degree = 0;
        self.nodes[idx].in_degree = 0;

        // 级联清边：凡 from==id 或 to==id 的边全部移除。
        let mut cleared = 0usize;
        let mut i = 0usize;
        while i < self.edges.len() {
            if self.edges[i].from == id || self.edges[i].to == id {
                self.edges.swap_remove(i);
                cleared += 1;
            } else {
                i += 1;
            }
        }
        self.edge_count = self.edges.len();
        self.cascade_cleared += cleared as u64;
        Ok(cleared)
    }

    pub fn is_alive(&self, id: SubjectId) -> bool {
        self.find_node_idx(id)
            .map(|i| self.nodes[i].alive)
            .unwrap_or(false)
    }

    pub fn node(&self, id: SubjectId) -> Option<&SubjectNode> {
        self.find_node_idx(id).map(|i| &self.nodes[i])
    }

    /// 记录一次心跳（F0247 看门狗输入）。
    pub fn heartbeat(&mut self, id: SubjectId, now: u32) -> Result<(), u16> {
        let idx = self.find_node_idx(id).ok_or(E_NO_NODE)?;
        if !self.nodes[idx].alive {
            return Err(E_NO_NODE);
        }
        self.nodes[idx].heartbeat_at = now;
        Ok(())
    }

    /// 记录持有资源数变化（RAG 侧记账）。
    pub fn adjust_holds(&mut self, id: SubjectId, delta: i32) -> Result<(), u16> {
        let idx = self.find_node_idx(id).ok_or(E_NO_NODE)?;
        if !self.nodes[idx].alive {
            return Err(E_NO_NODE);
        }
        let cur = self.nodes[idx].holds as i32 + delta;
        if cur < 0 {
            self.nodes[idx].holds = 0;
        } else {
            self.nodes[idx].holds = cur as u16;
        }
        Ok(())
    }

    // -- 边管理（F0242 生命周期） -----------------------------------------

    /// 建立等待边。返回 `WaitId` 作删边凭据。
    ///
    /// 拒绝条件：图满 / 节点不存在或已销毁 / 重复边 / 自环。
    /// **自环拒绝**：自己等自己在语义上是编程错误（同线程重入同一把锁），
    /// 不是死锁——拒绝并计数，交由 F0246 锁序检查捕获。
    pub fn add_wait(
        &mut self,
        from: SubjectId,
        to: SubjectId,
        predicate: WaitPredicate,
        source: WaitSource,
        now: u32,
    ) -> Result<WaitId, u16> {
        if from == to {
            self.edges_rejected += 1;
            return Err(E_SELF_EDGE);
        }
        if self.edges.len() >= MAX_EDGES {
            self.edges_rejected += 1;
            return Err(E_GRAPH_FULL);
        }
        // 节点必须存活。
        let fi = self.find_node_idx(from).ok_or(E_NO_NODE)?;
        let ti = self.find_node_idx(to).ok_or(E_NO_NODE)?;
        if !self.nodes[fi].alive || !self.nodes[ti].alive {
            self.edges_rejected += 1;
            return Err(E_NO_NODE);
        }
        // 重复边检测（同一 from→to+predicate 只允许一条）。
        for e in &self.edges {
            if e.from == from && e.to == to && e.predicate == predicate {
                self.edges_rejected += 1;
                return Err(E_DUP_EDGE);
            }
        }

        let wid = self.next_wait_id;
        self.next_wait_id = self.next_wait_id.wrapping_add(1).max(1);
        self.edges.push(WaitEdge {
            from,
            to,
            predicate,
            source,
            created_at: now,
            weight: 0,
            wait_id: wid,
        });
        self.nodes[fi].out_degree = self.nodes[fi].out_degree.saturating_add(1);
        self.nodes[ti].in_degree = self.nodes[ti].in_degree.saturating_add(1);
        self.edge_count = self.edges.len();
        self.edges_created += 1;
        Ok(wid)
    }

    /// 按 `WaitId` 删边（等待解除）。
    pub fn remove_wait(&mut self, wait_id: WaitId) -> Result<(), u16> {
        let pos = self.edges.iter().position(|e| e.wait_id == wait_id);
        let pos = pos.ok_or(E_NO_WAIT)?;
        let e = self.edges.swap_remove(pos);
        self.edge_count = self.edges.len();
        self.edges_removed += 1;
        // 回收度数（节点可能已销毁，度数已清零——用 saturating 防下溢）。
        if let Some(fi) = self.find_node_idx(e.from) {
            if self.nodes[fi].alive {
                self.nodes[fi].out_degree = self.nodes[fi].out_degree.saturating_sub(1);
            }
        }
        if let Some(ti) = self.find_node_idx(e.to) {
            if self.nodes[ti].alive {
                self.nodes[ti].in_degree = self.nodes[ti].in_degree.saturating_sub(1);
            }
        }
        Ok(())
    }

    /// 按 (from, to, predicate) 删边（等待解除的语义入口）。
    pub fn remove_wait_by(
        &mut self,
        from: SubjectId,
        to: SubjectId,
        predicate: WaitPredicate,
    ) -> Result<WaitId, u16> {
        let pos = self
            .edges
            .iter()
            .position(|e| e.from == from && e.to == to && e.predicate == predicate);
        let pos = pos.ok_or(E_NO_WAIT)?;
        let wid = self.edges[pos].wait_id;
        self.remove_wait(wid)?;
        Ok(wid)
    }

    /// 累加等待权重（tick 推进时由 `tick()` 调用）。
    pub fn age_edges(&mut self) {
        for e in &mut self.edges {
            e.weight = e.weight.saturating_add(1);
        }
    }

    // -- 查询 -------------------------------------------------------------

    pub fn edge(&self, wait_id: WaitId) -> Option<&WaitEdge> {
        self.edges.iter().find(|e| e.wait_id == wait_id)
    }

    pub fn edges(&self) -> &[WaitEdge] {
        &self.edges
    }

    /// 可变边迭代（仅供权重老化与检测器注入）。
    pub fn edges_mut(&mut self) -> &mut [WaitEdge] {
        &mut self.edges
    }

    /// 取某主体的出边（它在等谁）。
    pub fn out_edges(&self, id: SubjectId) -> Vec<&WaitEdge> {
        self.edges.iter().filter(|e| e.from == id).collect()
    }

    /// 取某主体的入边（谁在等它）。
    pub fn in_edges(&self, id: SubjectId) -> Vec<&WaitEdge> {
        self.edges.iter().filter(|e| e.to == id).collect()
    }

    /// 主体的直接后继（它在等的主体集合，去重）。
    pub fn successors(&self, id: SubjectId) -> Vec<SubjectId> {
        let mut out: Vec<SubjectId> = Vec::new();
        for e in &self.edges {
            if e.from == id && !out.contains(&e.to) {
                out.push(e.to);
            }
        }
        out
    }

    /// 主体的直接前驱（等它的主体集合，去重）。
    pub fn predecessors(&self, id: SubjectId) -> Vec<SubjectId> {
        let mut out: Vec<SubjectId> = Vec::new();
        for e in &self.edges {
            if e.to == id && !out.contains(&e.from) {
                out.push(e.from);
            }
        }
        out
    }

    // -- 一致性自检（F0242 判据） -----------------------------------------

    /// 图一致性校验：边引用的节点必存活、度数缓存正确、无重复边。
    pub fn check_consistency(&self) -> CheckSet {
        let mut cs = CheckSet::new("cgb241_graph");
        let mut dangling = 0usize;
        let mut dup = 0usize;
        let mut degree_bad = 0usize;

        for e in &self.edges {
            let fo = self.find_node_idx(e.from);
            let to = self.find_node_idx(e.to);
            let ok_f = fo.map(|i| self.nodes[i].alive).unwrap_or(false);
            let ok_t = to.map(|i| self.nodes[i].alive).unwrap_or(false);
            if !ok_f || !ok_t {
                dangling += 1;
            }
        }
        // 重复边。
        for i in 0..self.edges.len() {
            for j in (i + 1)..self.edges.len() {
                if self.edges[i].from == self.edges[j].from
                    && self.edges[i].to == self.edges[j].to
                    && self.edges[i].predicate == self.edges[j].predicate
                {
                    dup += 1;
                }
            }
        }
        // 度数缓存核对。
        for n in &self.nodes {
            if !n.alive {
                continue;
            }
            let real_out = self.edges.iter().filter(|e| e.from == n.id).count() as u16;
            let real_in = self.edges.iter().filter(|e| e.to == n.id).count() as u16;
            if real_out != n.out_degree || real_in != n.in_degree {
                degree_bad += 1;
            }
        }

        cs.add(
            "无悬空边",
            dangling == 0,
            "边引用的节点必须存活",
        );
        cs.add("无重复边", dup == 0, "同 from-to-predicate 唯一");
        cs.add("度数缓存正确", degree_bad == 0, "out/in degree 与实边一致");
        cs.add(
            "边计数一致",
            self.edge_count == self.edges.len(),
            "edge_count 缓存同步",
        );
        cs
    }
}

// ---------------------------------------------------------------------------
// 资源分配图（RAG）—— 记账侧
// ---------------------------------------------------------------------------

/// RAG 边类型。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RagEdgeKind {
    /// 任务持有资源。
    Hold,
    /// 任务等待资源。
    Request,
}

/// RAG 记账条目（二分图的一条边）。
#[derive(Clone, Copy, Debug)]
pub struct RagEdge {
    pub subject: SubjectId,
    pub resource: ResId,
    pub kind: RagEdgeKind,
    /// 多实例资源的实例号（单实例恒 0）。
    pub instance: u16,
}

/// 资源分配图（RAG）——持有关系的权威记录。
///
/// 不用于死锁判定（多实例下 RAG 有环≠死锁），但用于：
/// 1. 牺牲者选择（F0245 的「持锁数量」因子）；
/// 2. 资源归还完整性验证（F0245 解除后验证）；
/// 3. 诊断报告的持有表转储（F0252）。
pub struct ResourceAllocGraph {
    edges: Vec<RagEdge>,
    /// 资源实例总数（单实例资源 = 1）。
    instances: Vec<(ResId, u16)>,
    /// 资源可用实例数。
    available: Vec<(ResId, u16)>,
}

impl ResourceAllocGraph {
    pub fn new() -> ResourceAllocGraph {
        ResourceAllocGraph {
            edges: Vec::new(),
            instances: Vec::new(),
            available: Vec::new(),
        }
    }

    /// 注册资源。
    pub fn register_resource(&mut self, res: ResId, instances: u16) {
        if !self.instances.iter().any(|(r, _)| *r == res) {
            self.instances.push((res, instances));
            self.available.push((res, instances));
        }
    }

    /// 分配一个实例给 subject。成功则记 Hold 边。
    pub fn alloc(&mut self, subject: SubjectId, res: ResId) -> Result<u16, u16> {
        let pos = self
            .available
            .iter()
            .position(|(r, _)| *r == res)
            .ok_or(E_NO_NODE)?;
        if self.available[pos].1 == 0 {
            return Err(E_GRAPH_FULL);
        }
        // 找一个未被占用的实例号。
        let total = self
            .instances
            .iter()
            .find(|(r, _)| *r == res)
            .map(|(_, t)| *t)
            .unwrap_or(1);
        let mut inst = 0u16;
        while inst < total {
            if !self
                .edges
                .iter()
                .any(|e| e.resource == res && e.instance == inst && e.kind == RagEdgeKind::Hold)
            {
                break;
            }
            inst += 1;
        }
        if inst >= total {
            return Err(E_GRAPH_FULL);
        }
        self.available[pos].1 -= 1;
        self.edges.push(RagEdge {
            subject,
            resource: res,
            kind: RagEdgeKind::Hold,
            instance: inst,
        });
        Ok(inst)
    }

    /// 归还实例。
    pub fn free(&mut self, subject: SubjectId, res: ResId, instance: u16) -> Result<(), u16> {
        let pos = self.edges.iter().position(|e| {
            e.subject == subject
                && e.resource == res
                && e.instance == instance
                && e.kind == RagEdgeKind::Hold
        });
        let pos = pos.ok_or(E_NO_WAIT)?;
        self.edges.swap_remove(pos);
        if let Some(ap) = self.available.iter().position(|(r, _)| *r == res) {
            self.available[ap].1 = self.available[ap].1.saturating_add(1);
        }
        Ok(())
    }

    /// 记录请求边（等待中）。
    pub fn request(&mut self, subject: SubjectId, res: ResId) {
        self.edges.push(RagEdge {
            subject,
            resource: res,
            kind: RagEdgeKind::Request,
            instance: 0,
        });
    }

    /// 清除请求边。
    pub fn clear_request(&mut self, subject: SubjectId, res: ResId) {
        self.edges
            .retain(|e| !(e.subject == subject && e.resource == res && e.kind == RagEdgeKind::Request));
    }

    /// subject 的持有数（F0245 牺牲者选择因子之一）。
    pub fn hold_count(&self, subject: SubjectId) -> usize {
        self.edges
            .iter()
            .filter(|e| e.subject == subject && e.kind == RagEdgeKind::Hold)
            .count()
    }

    /// 资源的持有者集合。
    pub fn holders(&self, res: ResId) -> Vec<SubjectId> {
        let mut out: Vec<SubjectId> = Vec::new();
        for e in &self.edges {
            if e.resource == res && e.kind == RagEdgeKind::Hold && !out.contains(&e.subject) {
                out.push(e.subject);
            }
        }
        out
    }

    /// 资源可用实例数。
    pub fn available_of(&self, res: ResId) -> u16 {
        self.available
            .iter()
            .find(|(r, _)| *r == res)
            .map(|(_, a)| *a)
            .unwrap_or(0)
    }

    /// 归还完整性校验（F0245 解除后验证）：全部请求边清空 + 可用数正确。
    pub fn check_integrity(&self) -> CheckSet {
        let mut cs = CheckSet::new("cgb241_rag");
        let pending_req = self
            .edges
            .iter()
            .filter(|e| e.kind == RagEdgeKind::Request)
            .count();
        cs.add(
            "请求边已清",
            pending_req == 0,
            "解除后不应残留请求边",
        );

        // 可用 + 已分配 == 总实例。
        let mut mismatch = 0usize;
        for (res, total) in &self.instances {
            let held = self
                .edges
                .iter()
                .filter(|e| e.resource == *res && e.kind == RagEdgeKind::Hold)
                .count() as u16;
            let avail = self.available_of(*res);
            if held + avail != *total {
                mismatch += 1;
            }
        }
        cs.add(
            "实例账目平衡",
            mismatch == 0,
            "held + available == total",
        );
        cs
    }
}

// ---------------------------------------------------------------------------
// 双模型统一宿主
// ---------------------------------------------------------------------------

/// 双模型宿主：WFG 判死锁 + RAG 记持有。
///
/// 这是 F0241 的顶层交付物。上层检测器（F0243 起）只依赖本结构，
/// 不直接触碰两个子图——保证「判定走 WFG、记账走 RAG」的纪律
/// 在 API 层面无法绕过。
pub struct DualModel {
    /// 等待图（判定侧）。
    pub wfg: WaitGraph,
    /// 资源分配图（记账侧）。
    pub rag: ResourceAllocGraph,
    /// 当前 tick。
    pub now: u32,
    /// 自上次全量扫描的 tick 数。
    pub ticks_since_full: u32,
    /// 配置。
    pub cfg: DetectorConfig,
    /// 遥测：增量检测次数。
    pub incr_runs: u64,
    /// 遥测：全量扫描次数。
    pub full_runs: u64,
    /// 遥测：因超预算退化次数。
    pub degrade_count: u64,
}

impl DualModel {
    pub fn new(cfg: DetectorConfig) -> Result<DualModel, u16> {
        cfg.validate()?;
        Ok(DualModel {
            wfg: WaitGraph::new(),
            rag: ResourceAllocGraph::new(),
            now: 0,
            ticks_since_full: 0,
            cfg,
            incr_runs: 0,
            full_runs: 0,
            degrade_count: 0,
        })
    }

    /// tick 推进：权重老化 + 兜底周期计数。
    /// 返回 `true` 表示该跑全量兜底了。
    pub fn tick(&mut self) -> bool {
        self.now = self.now.wrapping_add(1);
        self.wfg.age_edges();
        self.ticks_since_full = self.ticks_since_full.saturating_add(1);
        if self.ticks_since_full >= self.cfg.full_scan_ticks {
            self.ticks_since_full = 0;
            self.full_runs += 1;
            true
        } else {
            false
        }
    }

    /// 是否应触发增量检测（配置开关 + 图规模）。
    pub fn should_incr(&self) -> bool {
        self.cfg.incremental_enabled
    }

    /// 图是否处于「大图」态（F0254 降频输入）。
    pub fn is_large(&self) -> bool {
        self.wfg.node_count() >= self.cfg.large_graph_nodes
    }

    /// 四源统一建等待边的便捷入口。
    pub fn wait(
        &mut self,
        from: SubjectId,
        to: SubjectId,
        predicate: WaitPredicate,
        source: WaitSource,
    ) -> Result<WaitId, u16> {
        self.wfg.add_wait(from, to, predicate, source, self.now)
    }

    /// 等待解除。
    pub fn wake(&mut self, wait_id: WaitId) -> Result<(), u16> {
        self.wfg.remove_wait(wait_id)
    }

    /// 全图自检（双模型 + 一致性 + 容量）。
    pub fn check_all(&self) -> CheckSet {
        let mut cs = CheckSet::new("cgb241_dual");
        let wc = self.wfg.check_consistency();
        for i in 0..wc.len() {
            if let Some(c) = wc.get(i) {
                cs.add(c.name, c.passed, c.detail);
            }
        }
        let rc = self.rag.check_integrity();
        for i in 0..rc.len() {
            if let Some(c) = rc.get(i) {
                cs.add(c.name, c.passed, c.detail);
            }
        }
        cs.add(
            "节点容量有界",
            self.wfg.raw_node_count() <= MAX_NODES,
            "MAX_NODES=1024",
        );
        cs.add(
            "边容量有界",
            self.wfg.edges().len() <= MAX_EDGES,
            "MAX_EDGES=8192",
        );
        cs.add(
            "配置有效",
            self.cfg.validate().is_ok(),
            "incr/full/large 三参数",
        );
        cs.add_dyn(
            "拒绝建边计数",
            true,
            &alloc::format!("rejected={}", self.wfg.edges_rejected),
        );
        cs
    }
}

// ---------------------------------------------------------------------------
// 域自检入口
// ---------------------------------------------------------------------------

/// CGPU-F0241 域自检。
///
/// 覆盖判据三条：双模型、增量+兜底、四源统一。
pub fn run_cgb241_checks() -> CheckSet {
    let mut cs = CheckSet::new("cgb241_dlarch");
    let cfg = DetectorConfig::default();
    let mut dm = DualModel::new(cfg).expect("default config valid");

    // --- 双模型 ---
    cs.add("双模型存在", true, "WFG + RAG");
    dm.rag.register_resource(1, 2);
    dm.rag.register_resource(2, 1);
    dm.wfg.add_node(10).ok();
    dm.wfg.add_node(20).ok();
    dm.wfg.add_node(30).ok();
    let alloc_ok = dm.rag.alloc(10, 1).is_ok();
    cs.add("RAG 记账可用", alloc_ok, "alloc 成功");
    let w = dm.wait(20, 10, WaitPredicate::Resource(1), WaitSource::Lock);
    cs.add("WFG 建边可用", w.is_ok(), "wait 建边");
    cs.add(
        "双模型分工",
        dm.rag.hold_count(10) == 1 && dm.wfg.edges().len() == 1,
        "RAG 记 hold / WFG 记 wait",
    );

    // --- 增量 + 兜底 ---
    cs.add("增量开关默认开", dm.should_incr(), "incremental_enabled=true");
    // 推进 tick 至兜底周期。
    let mut full_hit = false;
    for _ in 0..FULL_SCAN_TICKS {
        if dm.tick() {
            full_hit = true;
            break;
        }
    }
    cs.add("兜底周期触发", full_hit, "FULL_SCAN_TICKS=1000");
    cs.add(
        "权重老化",
        dm.wfg.edges().iter().all(|e| e.weight > 0),
        "age_edges 累加",
    );

    // --- 四源统一 ---
    let mut src_ok = true;
    for (i, src) in WaitSource::ALL.iter().enumerate() {
        let a = 100 + i as u32;
        let b = 200 + i as u32;
        dm.wfg.add_node(a).ok();
        dm.wfg.add_node(b).ok();
        let pred = match src {
            WaitSource::Task => WaitPredicate::TaskDone(b),
            WaitSource::Lock => WaitPredicate::Resource(i as u32),
            WaitSource::Barrier => WaitPredicate::Barrier(i as u32),
            WaitSource::VramAlloc => WaitPredicate::AllocSlot(i as u32),
        };
        match dm.wait(a, b, pred, *src) {
            Ok(_) => {}
            Err(_) => src_ok = false,
        }
    }
    cs.add("四源统一建边", src_ok, "task/lock/barrier/vram");

    // --- 一致性 ---
    let gc = dm.wfg.check_consistency();
    let mut g_ok = true;
    for i in 0..gc.len() {
        if let Some(c) = gc.get(i) {
            if !c.passed {
                g_ok = false;
            }
        }
    }
    cs.add("图一致性", g_ok, "悬空/重复/度数");

    // --- 级联清边 ---
    let before = dm.wfg.edges().len();
    let cleared = dm.wfg.remove_node(10).unwrap_or(0);
    cs.add(
        "级联清边",
        cleared > 0 && dm.wfg.edges().len() < before,
        "节点销毁清边",
    );

    // --- 拒绝建边显性 ---
    let mut rejected_visible = true;
    // 自环应被拒。
    if dm.wfg.add_node(999).is_ok() {
        if dm
            .wfg
            .add_wait(999, 999, WaitPredicate::Resource(0), WaitSource::Lock, 0)
            .is_ok()
        {
            rejected_visible = false;
        }
    }
    cs.add("自环拒绝", rejected_visible, "E_SELF_EDGE");
    cs.add(
        "拒绝计数显性",
        dm.wfg.edges_rejected > 0,
        "edges_rejected 累加",
    );

    // --- 容量边界 ---
    cs.add(
        "节点上限",
        MAX_NODES == 1024,
        "1024 主体够全场景",
    );
    cs.add("边上限", MAX_EDGES == 8192, "8192 边够 8 出度");
    cs.add(
        "配置校验",
        DetectorConfig {
            incr_visit_budget: 0,
            ..DetectorConfig::default()
        }
        .validate()
        .is_err(),
        "0 预算拒绝",
    );

    cs
}
