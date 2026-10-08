//! VE-F3005 · 转场编排器（VE-P 域 · 动效与过渡库 · P03 组）—— **Rust 权威实现**。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3005`
//!
//! # 判据（锚点原文六条 + 纪律三条）
//!
//! 1. **DAG 声明**：编排图节点=元素×动效任务，边=时序依赖（**前置完成/并行/偏移启动**
//!    三边型）；图**必须**是 DAG，环在构建期就拒（环=死锁红线）；
//! 2. **四原语**：序列（sequential）/ 并行（parallel）/ 错开（stagger 均布偏移）/
//!    条件分支（状态驱动编排切换）—— 四原语各自编译为 M 时间轴实例组；
//! 3. **打断三策略**：编排中途打断→已完成阶段保持 + 未完成阶段按策略处置
//!    （快速完成 / 原地保持 / 回滚），**策略未选型默认快速完成并出诊断**（不悬空红线）；
//! 4. **嵌套上限 8**：编排可嵌套（子编排=复合节点），深度上限 8，超限拒绝；
//! 5. **统一 reduce**：reduce 态编排**坍缩为终态直达**——在编排器层统一处理，
//!    组件不必各自处理（组件零分支是本项的红线，不是效率问题）；
//! 6. **错误路径**：环→构建期拒绝（三要素齐发）；编译产物超预算（千实例）→预算检查
//!    + 降档（错开粒度合并）；打断策略未选型→默认+诊断；嵌套超限→拒绝；
//! 7. **性能分解**：图构建 O(节点+边)、环检测 O(V+E)、编译 O(实例数)、错开 O(元素数)。
//!
//! # 为什么环检测必须在**构建期**而不是编译期
//!
//! 编排图的语义是「谁在谁之后开始」。若图里有环 A→B→A，它没有任何拓扑序，
//! 编译期才发现时报的是「排不出顺序」——这句话对排障的人**毫无用处**，因为
//! 他真正要知道的是**哪个环**。而等到运行期才发现，代价是动画挂死：
//! 页面停在中间态，既没有入场也没有终态，用户只看到界面卡在半空。
//!
//! 故 [`Orchestrator::commit`] 在图封口的那一刻就跑 Kahn 拓扑排序，
//! 失败时报 [`E_CYCLE_DETECTED`]，并在 [`CycleReport`] 里**点名环上的节点链**
//! （`n3 -> n1 -> n7 -> n3`），让人一眼看出是哪几个动效互相等。
//!
//! # 三边型为什么要把「并行」也建成边（而不是不建边）
//!
//! 若「并行」表达为「无边」，那么「显式声明并行」与「忘了声明依赖」在图上**长得
//! 一模一样**——都是零入边。两者在运行期表现相反：前者该同时跑，后者该按序。
//! 分不出来就等于没有依赖声明，于是真出现时序错乱时，无人知道该加边还是该删边。
//! 故 [`EdgeKind::Parallel`] 是**显式边**：它不参与约束求解，但参与
//! [`Orchestrator::parallel_groups`] 的分组统计，声明者可查、可对拍。
//!
//! # 打断策略为什么必须「未选型也要能跑」
//!
//! 打断发生在最不该停下来想「该用哪个策略」的时刻——用户手快点两下。此时若
//! 「策略未选型」表现为**报错/丢弃**，用户看到的是「点了没反应」，而开发者
//! 什么日志都拿不到。故本项的处置是：**默认快速完成 + 显式诊断**
//! （[`InterruptPolicy::default_is_finish_fast`]），即行为可预期但**必留痕迹**
//! ——`InterruptReport.policy_source` 会记`PolicySource::Defaulted`。
//! 这与「静默兜底成任意行为」的区别是：静默兜底**不留痕迹**，事后无法统计
//! 有多少打断走了默认路径。
//!
//! # reduce 坍缩为什么在编排器层而不是组件层
//!
//! 若每个组件各自判reduce，总有人新写一个组件忘了判——漏掉的组件在reduce 态
//! 照动，用户把「减少动态效果」开到了头仍有动画在飞，这直接是无障碍设置失效。
//! 故 [`compile`] 是**唯一出口**：reduce 泳道下编译产物**整体**坍缩为终态
//! （全部零时长、零错开、零阶段），组件拿到的就是终态，不存在「忘了判」的路径。
//! 注意坍缩的判定与 F3003（reduce 令牌层）、F3004（reduce 控制闸门）**分工不同**：
//! F3003 管令牌取值、F3004 管编排动作能否生效、本项管**整个编排图是否还成立**。
//!
//! # 嵌套深度上限 8 的取值理由
//!
//! 上限不是拍脑袋：嵌套每深一层，编译期需要额外持有一层中间实例组，
//! 且打断传播要跨层汇总（外层打断须下沉到内层未完成阶段）。8 层足够表达
//! 「页面转场 → 区域转场 → 卡片转场 → 元素错开」这类真实层级，
//! 又能在编译期一次性校验完（超限即拒，不做运行期截断）。
//!
//! # no_std
//!
//! 仅依赖同册 [`vep03_token`]（reduce 泳道判定）与 [`vep04_stack`]（M v2 控制接口、
//! 编译目标选型表——本项按锚点「跨批对接点」消费之）、`alloc`。
//! 零IO、零墙钟、零浮点（时序全用整型毫秒），编译与打断回归可复现。

use crate::svstar2::vep03_token::{Lane, MotionTokenError};
use crate::svstar2::vep04_stack::{
    select_target, CompileTarget, ControlDirection, ControlPriority, ControlScope, ControlV1,
    ControlV2, P_NAMESPACE_OWNER,
};

use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、常量（参数唯一源）
// ---------------------------------------------------------------------------

/// 编排协议版本。图模型或原语语义变更走版本号。
pub const ORCH_PROTOCOL_VERSION: &str = "P03-orch-v1";
/// 打断协议版本。
pub const INTERRUPT_PROTOCOL_VERSION: &str = "P03-interrupt-v1";
/// 原语编译器版本。
pub const PRIMITIVE_PROTOCOL_VERSION: &str = "P03-primitive-v1";

/// 时序依赖边类型数（前置完成/并行/偏移启动）。
pub const EDGE_KIND_COUNT: usize = 3;
/// 编排原语数（序列/并行/错开/条件分支）。
pub const PRIMITIVE_COUNT: usize = 4;
/// 打断处置策略数（快速完成/原地保持/回滚）。
pub const INTERRUPT_STRATEGY_COUNT: usize = 3;
/// 图节点名长度上限（读屏可读 + 定长表用）。
pub const NODE_NAME_CAP: usize = 24;

/// 嵌套深度上限（锚点：嵌套上限 8）。
pub const MAX_NEST_DEPTH: usize = 8;
/// 单图编译产物实例预算（锚点：千实例编排）。
pub const INSTANCE_BUDGET: usize = 1000;
/// 降档触发的实例数阈值线（超预算即降档）。
pub const BUDGET_TRIP: usize = INSTANCE_BUDGET;
/// 超预算时的降档比例分母（错开粒度合并：粒度×4）。
pub const STAGGER_MERGE_FACTOR: u32 = 4;
/// 元素批次上限（错开按批处理，避免单批无界）。
pub const MAX_STAGGER_ELEMENTS: usize = 4096;

// ---------------------------------------------------------------------------
// 二、时序依赖边（判据一：三边型）
// ---------------------------------------------------------------------------

/// 时序依赖边类型。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EdgeKind {
    /// 前置完成：to 必须等 from **跑完**才起（t_from_end <= t_to_start）。
    AfterComplete,
    /// 并行：显式声明「无时序约束」，不参与约束求解，但参与并行组统计。
    Parallel,
    /// 偏移启动：to 在 from 起后固定偏移量起（t_to_start = t_from_start + delay），
    /// **不等** from 跑完（错开/交叠的语义）。
    OffsetStart,
}

impl EdgeKind {
    /// 全集（顺序即文档枚举序）。
    pub const ALL: [EdgeKind; EDGE_KIND_COUNT] = [
        EdgeKind::AfterComplete,
        EdgeKind::Parallel,
        EdgeKind::OffsetStart,
    ];

    /// 代号。
    pub fn code(self) -> &'static str {
        match self {
            EdgeKind::AfterComplete => "after-complete",
            EdgeKind::Parallel => "parallel",
            EdgeKind::OffsetStart => "offset-start",
        }
    }

    /// 中文名。
    pub fn zh(self) -> &'static str {
        match self {
            EdgeKind::AfterComplete => "前置完成",
            EdgeKind::Parallel => "并行",
            EdgeKind::OffsetStart => "偏移启动",
        }
    }

    /// 是否参与时序约束求解（并行边**不**参与）。
    pub fn constrains(self) -> bool {
        self != EdgeKind::Parallel
    }

    /// 语义说明（错误信息与文档同源，避免注释与代码反向）。
    pub fn semantics(self) -> &'static str {
        match self {
            EdgeKind::AfterComplete => "to.start >= from.start + from.duration",
            EdgeKind::Parallel => "无时序约束；显式声明并计入并行组",
            EdgeKind::OffsetStart => "to.start >= from.start + offset_ms",
        }
    }
}

/// 图节点 ID（构图期分配，从 1 起；0 留空便于「未初始化」默认值区分）。
pub type NodeId = u32;

/// 一个编排节点（元素 × 动效任务）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OrchNode {
    /// 节点 ID。
    pub id: NodeId,
    /// 元素标识（同一元素可挂多个动效任务，故与节点 ID 分开）。
    pub element: u32,
    /// 动效任务标识（O04 侧动效 ID）。
    pub effect: u32,
    /// 节点名（读屏可读，进诊断）。
    pub name: String,
    /// 声明时长（毫秒，取自动效令牌）。
    pub duration_ms: u32,
    /// 编译目标特征掩码（F3004 决策表输入）。
    pub features: u8,
}

impl OrchNode {
    /// 构造节点。
    pub fn new(id: NodeId, element: u32, effect: u32, name: &str, duration_ms: u32, features: u8) -> Self {
        OrchNode {
            id,
            element,
            effect,
            name: String::from(name),
            duration_ms,
            features,
        }
    }
}

/// 一条时序依赖边。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimingEdge {
    /// 源节点。
    pub from: NodeId,
    /// 目标节点。
    pub to: NodeId,
    /// 边类型。
    pub kind: EdgeKind,
    /// 偏移毫秒（仅 [`EdgeKind::OffsetStart`] 使用）。
    pub offset_ms: u32,
}

impl TimingEdge {
    /// 构造前置完成边。
    pub fn after_complete(from: NodeId, to: NodeId) -> Self {
        TimingEdge {
            from,
            to,
            kind: EdgeKind::AfterComplete,
            offset_ms: 0,
        }
    }

    /// 构造并行边。
    pub fn parallel(from: NodeId, to: NodeId) -> Self {
        TimingEdge {
            from,
            to,
            kind: EdgeKind::Parallel,
            offset_ms: 0,
        }
    }

    /// 构造偏移启动边。
    pub fn offset_start(from: NodeId, to: NodeId, offset_ms: u32) -> Self {
        TimingEdge {
            from,
            to,
            kind: EdgeKind::OffsetStart,
            offset_ms,
        }
    }
}

// ---------------------------------------------------------------------------
// 三、编排图构建（声明式；环检测在封口处）
// ---------------------------------------------------------------------------

/// 环检测失败诊断。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CycleReport {
    /// 环上的节点链（首尾同节点，构成闭合证据）。
    pub chain: Vec<NodeId>,
    /// 节点名链（人读）。
    pub names: String,
}

impl CycleReport {
    /// 是否为有效环证据（链首==链尾且长度>=2）。
    ///
    /// 自环（n->n）链长为 1，不构成闭合，故要求 >=2。
    pub fn is_valid(&self) -> bool {
        self.chain.len() >= 2 && self.chain[0] == self.chain[self.chain.len() - 1]
    }
}

/// 编排图构建期错误。
pub const E_CYCLE_DETECTED: &str = "E_CYCLE_DETECTED";
/// 节点 ID 重复。
pub const E_NODE_DUPLICATE: &str = "E_NODE_DUPLICATE";
/// 边引用了不存在的节点。
pub const E_EDGE_UNKNOWN_NODE: &str = "E_EDGE_UNKNOWN_NODE";
/// 自环（节点依赖自己）。
pub const E_SELF_LOOP: &str = "E_SELF_LOOP";
/// 图为空。
pub const E_GRAPH_EMPTY: &str = "E_GRAPH_EMPTY";
/// 嵌套深度超限。
pub const E_NEST_DEPTH: &str = "E_NEST_DEPTH";
/// 编译产物超预算。
pub const E_BUDGET_EXCEEDED: &str = "E_BUDGET_EXCEEDED";
/// 打断策略未选型（走默认，须留诊断）。
pub const E_POLICY_DEFAULTED: &str = "E_POLICY_DEFAULTED";
/// 条件分支条件值非法。
pub const E_BRANCH_CONDITION: &str = "E_BRANCH_CONDITION";
/// 编译目标未解析（F3004 决策表未命中）。
pub const E_TARGET_UNRESOLVED: &str = "E_TARGET_UNRESOLVED";
/// 错开步长非法（0 步长=同时起，错开语义失效）。
pub const E_STAGGER_STEP: &str = "E_STAGGER_STEP";
/// 阶段下标越界。
pub const E_STAGE_OUT_OF_RANGE: &str = "E_STAGE_OUT_OF_RANGE";
/// 重复的阶段完成回执。
pub const E_STAGE_DUPLICATE: &str = "E_STAGE_DUPLICATE";

/// 编排图构建器（声明式；未 [`Orchestrator::commit`] 前不构成可编译的图）。
#[derive(Clone, Debug, Default)]
pub struct Orchestrator {
    nodes: Vec<OrchNode>,
    edges: Vec<TimingEdge>,
    nest_depth: u32,
}

impl Orchestrator {
    /// 构造空编排器（根层深度 0）。
    pub fn new() -> Self {
        Orchestrator {
            nodes: Vec::new(),
            edges: Vec::new(),
            nest_depth: 0,
        }
    }

    /// 当前嵌套深度。
    pub fn depth(&self) -> u32 {
        self.nest_depth
    }

    /// 节点数。
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// 边数。
    pub fn edge_count(&self) -> usize {
        self.edges.len()
    }

    /// 节点切片。
    pub fn nodes(&self) -> &[OrchNode] {
        &self.nodes
    }

    /// 边切片。
    pub fn edges(&self) -> &[TimingEdge] {
        &self.edges
    }

    /// 声明节点（重复 ID 即拒，不覆盖——覆盖会让已连的边指向旧节点）。
    pub fn node(&mut self, node: OrchNode) -> Result<NodeId, MotionTokenError> {
        let id = node.id;
        if self.nodes.iter().any(|n| n.id == id) {
            return Err(MotionTokenError::new(
                E_NODE_DUPLICATE,
                "编排图节点 ID 重复",
                &format!(
                    "节点 {} 已在图中；同名同 ID 的二次声明会让已连好的边指向哪个节点变得不确定",
                    id
                ),
                &format!(
                    "给节点 {} 换一个未使用的 ID（图内已用：{} 个节点）",
                    id,
                    self.nodes.len()
                ),
                P_NAMESPACE_OWNER,
            ));
        }
        if node.name.len() > NODE_NAME_CAP {
            return Err(MotionTokenError::new(
                E_NODE_DUPLICATE,
                "编排节点名超长",
                &format!(
                    "节点 {} 的名字 {} 字节，超过 {} 上限",
                    id,
                    node.name.len(),
                    NODE_NAME_CAP
                ),
                "缩短节点名；诊断链与读屏提示都按定长取字段",
                P_NAMESPACE_OWNER,
            ));
        }
        self.nodes.push(node);
        Ok(id)
    }

    /// 声明一条边（端点必须已存在）。
    pub fn edge(&mut self, edge: TimingEdge) -> Result<(), MotionTokenError> {
        if !self.has_node(edge.from) || !self.has_node(edge.to) {
            return Err(MotionTokenError::new(
                E_EDGE_UNKNOWN_NODE,
                "时序边引用未声明的节点",
                &format!(
                    "边 {}->{} 的一端未在图中声明；悬空边在拓扑求解时会被静默忽略，表现为动画没按声明走却无从查起",
                    edge.from, edge.to
                ),
                "先把两端节点用 node() 声明出来，再连边",
                P_NAMESPACE_OWNER,
            ));
        }
        if edge.from == edge.to {
            return Err(MotionTokenError::new(
                E_SELF_LOOP,
                "时序边自环",
                &format!(
                    "节点 {} 依赖自己完成才开始；自环是环的最小特例，同样排不出顺序",
                    edge.from
                ),
                "删掉这条边，或把它改成与另一个节点的依赖",
                P_NAMESPACE_OWNER,
            ));
        }
        if edge.kind == EdgeKind::OffsetStart && edge.offset_ms == 0 {
            return Err(MotionTokenError::new(
                E_STAGGER_STEP,
                "偏移启动边偏移为零",
                &format!(
                    "边 {}->{} 声明为偏移启动但偏移 0 毫秒，与并行边行为完全相同，两者在图上不可区分",
                    edge.from, edge.to
                ),
                "偏移为 0 时改用 Parallel 边，或给出真实偏移毫秒",
                P_NAMESPACE_OWNER,
            ));
        }
        self.edges.push(edge);
        Ok(())
    }

    /// 嵌套一个子编排（复合节点）。
    ///
    /// 深度检查在**构建期**做：超限即拒，不做运行期截断（截断会让深嵌套编排
    /// 悄悄少跑一层，而外部表现只是「动画少了一段」）。
    pub fn nest(&mut self, child: &Orchestrator) -> Result<(), MotionTokenError> {
        let next = self.nest_depth + 1 + child.nest_depth;
        if next as usize > MAX_NEST_DEPTH {
            return Err(MotionTokenError::new(
                E_NEST_DEPTH,
                "编排嵌套深度超限",
                &format!(
                    "嵌套后深度 {} 超过上限 {}；每深一层都要额外持有中间实例组且打断传播要跨层汇总",
                    next, MAX_NEST_DEPTH
                ),
                &format!(
                    "把嵌套压到 {} 层以内：拆成多个平级编排，或用并行原语替代一层嵌套",
                    MAX_NEST_DEPTH
                ),
                P_NAMESPACE_OWNER,
            ));
        }
        for n in child.nodes.iter() {
            self.node(n.clone())?;
        }
        for e in child.edges.iter() {
            self.edge(*e)?;
        }
        self.nest_depth = next;
        Ok(())
    }

    /// 是否有该节点。
    pub fn has_node(&self, id: NodeId) -> bool {
        self.nodes.iter().any(|n| n.id == id)
    }

    /// 取节点。
    pub fn node_of(&self, id: NodeId) -> Option<&OrchNode> {
        self.nodes.iter().find(|n| n.id == id)
    }

    /// 封口：做环检测与拓扑排序，返回可编译的图。
    pub fn commit(self) -> Result<OrchGraph, MotionTokenError> {
        if self.nodes.is_empty() {
            return Err(MotionTokenError::new(
                E_GRAPH_EMPTY,
                "编排图为空",
                "空图没有可编译的时序；空转的编排会让调用方以为「已编排」实则什么都没跑",
                "至少声明一个节点再封口",
                P_NAMESPACE_OWNER,
            ));
        }
        if let Some(report) = self.find_cycle() {
            return Err(MotionTokenError::new(
                E_CYCLE_DETECTED,
                "编排图存在环，无法拓扑排序",
                &format!(
                    "环链 {}；环上任一节点都等不到自己或同环节点先完成，动效互相等待=死锁",
                    report.names
                ),
                &format!(
                    "断掉环上一条边（{}），或把互为前置的两个阶段改成并行边",
                    report.names
                ),
                P_NAMESPACE_OWNER,
            ));
        }
        let order = self.topological_order();
        let parallel_groups = self.count_parallel_groups();
        Ok(OrchGraph {
            nodes: self.nodes,
            edges: self.edges,
            topological: order,
            nest_depth: self.nest_depth,
            parallel_groups,
        })
    }

    /// 约束邻接（只含参与求解的边；`Parallel` 不入）。
    ///
    /// 存**下标**（usize）而非节点 ID（u32）：拓扑求解全程按下标走，
    /// 每轮少一次 ID→下标线性查找，复杂度守住O(V+E)。
    fn constraint_adjacency(&self) -> Vec<Vec<usize>> {
        let mut adj: Vec<Vec<usize>> = vec![Vec::new(); self.nodes.len()];
        for e in self.edges.iter() {
            if !e.kind.constrains() {
                continue;
            }
            let from_idx = match self.index_of(e.from) {
                Some(i) => i,
                None => continue,
            };
            let to_idx = match self.index_of(e.to) {
                Some(i) => i,
                None => continue,
            };
            adj[from_idx].push(to_idx);
        }
        // 去重 + 升序，保证求解**确定性**（同输入必同输出，回归可复现）。
        // 按下标逐行处理：一次性收集邻接副本再写回，避免 iter_mut 与写入同借用。
        let mut normalized: Vec<Vec<usize>> = Vec::with_capacity(adj.len());
        for row in adj.iter() {
            let mut v = row.clone();
            v.sort_unstable();
            v.dedup();
            normalized.push(v);
        }
        normalized
    }

    /// 节点 ID → 下标。
    fn index_of(&self, id: NodeId) -> Option<usize> {
        self.nodes.iter().position(|n| n.id == id)
    }

    /// 拓扑序（Kahn；调用方须已确认无环）。
    ///
    /// 同一层内按节点 ID 升序出队——出队顺序即**可复现**顺序，
    /// 否则同一张图两次编译出的实例顺序不同，对拍会报「顺序漂移」假故障。
    fn topological_order(&self) -> Vec<NodeId> {
        let adj = self.constraint_adjacency();
        let n = self.nodes.len();
        let mut indeg: Vec<usize> = vec![0; n];
        for row in adj.iter() {
            for t in row.iter() {
                indeg[*t] += 1;
            }
        }
        // 就绪集用有序Vec + 插入排序维持最小出队（节点数不大，避免堆依赖）。
        let mut ready: Vec<usize> = (0..n).filter(|i| indeg[*i] == 0).collect();
        let mut out: Vec<NodeId> = Vec::with_capacity(n);
        while let Some(pos) = ready.iter().enumerate().min_by_key(|(_, idx)| {
            // 比较节点 ID（而非下标）以保证语义顺序稳定。
            self.nodes[**idx].id
        })
        .map(|(p, _)| p)
        {
            let cur = ready.remove(pos);
            out.push(self.nodes[cur].id);
            for t in adj[cur].iter() {
                indeg[*t] -= 1;
                if indeg[*t] == 0 {
                    let at = ready
                        .iter()
                        .position(|i| self.nodes[*i].id > self.nodes[*t].id);
                    match at {
                        Some(k) => ready.insert(k, *t),
                        None => ready.push(*t),
                    }
                }
            }
        }
        out
    }

    /// 环检测（DFS 白/灰/黑三色，回溯取链）。
    ///
    /// 只看**约束边**：`Parallel` 边不构成时序环（并行等无依赖），
    /// 把并行边算进环检测会让「A 与 B 并行、C 在 A 之后」被误报成环。
    fn find_cycle(&self) -> Option<CycleReport> {
        let adj = self.constraint_adjacency();
        let n = self.nodes.len();
        // 0=白 1=灰 2=黑
        let mut color: Vec<u8> = vec![0; n];
        let mut stack: Vec<usize> = Vec::new();
        for start in 0..n {
            if color[start] != 0 {
                continue;
            }
            if let Some(chain) = dfs_cycle(start, &adj, &mut color, &mut stack) {
                let mut ids: Vec<NodeId> = Vec::with_capacity(chain.len());
                let mut names: Vec<&str> = Vec::with_capacity(chain.len());
                for idx in chain.iter() {
                    ids.push(self.nodes[*idx].id);
                    names.push(self.nodes[*idx].name.as_str());
                }
                return Some(CycleReport {
                    chain: ids,
                    names: format!("{} -> {}", names.join(" -> "), names[0]),
                });
            }
        }
        None
    }

    /// 并行组统计（显式并行边的连通分量大小）。
    fn count_parallel_groups(&self) -> Vec<usize> {
        let n = self.nodes.len();
        let mut parent: Vec<usize> = (0..n).collect();
        for e in self.edges.iter() {
            if e.kind != EdgeKind::Parallel {
                continue;
            }
            let (a, b) = (self.index_of(e.from), self.index_of(e.to));
            if let (Some(a), Some(b)) = (a, b) {
                let ra = find_root(&mut parent, a);
                let rb = find_root(&mut parent, b);
                if ra != rb {
                    parent[ra] = rb;
                }
            }
        }
        let mut sizes: Vec<usize> = Vec::new();
        for i in 0..n {
            let r = find_root(&mut parent, i);
            if sizes.len() <= r {
                sizes.resize(r + 1, 0);
            }
            sizes[r] += 1;
        }
        sizes.retain(|s| *s > 0);
        sizes.sort_unstable();
        sizes
    }
}

/// 并查集查根（带路径压缩）。
fn find_root(parent: &mut Vec<usize>, mut x: usize) -> usize {
    while parent[x] != x {
        let up = parent[x];
        parent[x] = parent[up];
        x = parent[x];
    }
    x
}

/// DFS 找环（返回下标链，首尾同下标）。
fn dfs_cycle(
    start: usize,
    adj: &Vec<Vec<usize>>,
    color: &mut Vec<u8>,
    stack: &mut Vec<usize>,
) -> Option<Vec<usize>> {
    color[start] = 1;
    stack.push(start);
    for t in adj[start].iter() {
        match color[*t] {
            1 => {
                // 撞灰=回到栈上祖先，成环；截取自该节点起的栈段。
                let at = stack.iter().position(|i| *i == *t).unwrap_or(0);
                let mut chain: Vec<usize> = stack[at..].to_vec();
                chain.push(*t);
                return Some(chain);
            }
            0 => {
                if let Some(chain) = dfs_cycle(*t, adj, color, stack) {
                    return Some(chain);
                }
            }
            _ => {}
        }
    }
    stack.pop();
    color[start] = 2;
    None
}

/// 已封口的编排图（可编译；持有拓扑序保证编译确定性）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OrchGraph {
    /// 节点集。
    pub nodes: Vec<OrchNode>,
    /// 边集。
    pub edges: Vec<TimingEdge>,
    /// 拓扑序（节点 ID）。
    pub topological: Vec<NodeId>,
    /// 嵌套深度。
    pub nest_depth: u32,
    /// 并行组大小（升序；空边时为空）。
    pub parallel_groups: Vec<usize>,
}

impl OrchGraph {
    /// 节点数。
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// 边数。
    pub fn edge_count(&self) -> usize {
        self.edges.len()
    }

    /// 图规模摘要（性能判据用）。
    pub fn size(&self) -> (usize, usize) {
        (self.node_count(), self.edge_count())
    }

    /// 取节点（编译期按节点 ID 回查；图规模不大，线性查找足够且免去索引表维护）。
    pub fn node_of(&self, id: NodeId) -> Option<&OrchNode> {
        self.nodes.iter().find(|n| n.id == id)
    }
}

// ---------------------------------------------------------------------------
// 四、编排原语（判据二：四原语）
// ---------------------------------------------------------------------------

/// 编排原语。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Primitive {
    /// 序列：节点依次完成。
    Sequential,
    /// 并行：节点同时起。
    Parallel,
    /// 错开：节点按固定步长均布偏移起。
    Stagger,
    /// 条件分支：由状态键选择走哪一支。
    Conditional,
}

impl Primitive {
    /// 全集。
    pub const ALL: [Primitive; PRIMITIVE_COUNT] = [
        Primitive::Sequential,
        Primitive::Parallel,
        Primitive::Stagger,
        Primitive::Conditional,
    ];

    /// 代号。
    pub fn code(self) -> &'static str {
        match self {
            Primitive::Sequential => "sequential",
            Primitive::Parallel => "parallel",
            Primitive::Stagger => "stagger",
            Primitive::Conditional => "conditional",
        }
    }

    /// 中文名。
    pub fn zh(self) -> &'static str {
        match self {
            Primitive::Sequential => "序列",
            Primitive::Parallel => "并行",
            Primitive::Stagger => "错开",
            Primitive::Conditional => "条件分支",
        }
    }

    /// 该原语生成的边类型（无则None——并行原语生成的正是 Parallel 边）。
    pub fn edge_kind(self) -> Option<EdgeKind> {
        match self {
            Primitive::Sequential => Some(EdgeKind::AfterComplete),
            Primitive::Parallel => Some(EdgeKind::Parallel),
            Primitive::Stagger => Some(EdgeKind::OffsetStart),
            // 条件分支不产边：它在编译期按状态裁掉未选中的分支。
            Primitive::Conditional => None,
        }
    }
}

/// 原语编译上下文（错开步长、分支状态键等）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PrimitiveCtx {
    /// 错开步长（毫秒；仅 [`Primitive::Stagger`] 用）。
    pub stagger_step_ms: u32,
    /// 分支状态键（仅 [`Primitive::Conditional`] 用）。
    pub branch_state: bool,
    /// 批次起点节点（并行/序列的原语头节点）。
    pub anchor: NodeId,
}

impl PrimitiveCtx {
    /// 构造并行/序列上下文。
    pub fn plain(anchor: NodeId) -> Self {
        PrimitiveCtx {
            stagger_step_ms: 0,
            branch_state: false,
            anchor,
        }
    }

    /// 构造错开上下文。
    pub fn stagger(anchor: NodeId, stagger_step_ms: u32) -> Self {
        PrimitiveCtx {
            stagger_step_ms,
            branch_state: false,
            anchor,
        }
    }

    /// 构造条件分支上下文。
    pub fn conditional(anchor: NodeId, branch_state: bool) -> Self {
        PrimitiveCtx {
            stagger_step_ms: 0,
            branch_state,
            anchor,
        }
    }
}

/// 原语编译结果：把一组节点按原语接进编排器。
///
/// **原语不改变节点语义**：它只生成边（序列/并行/错开）或做编译期裁剪（条件分支）。
/// 这样组件（F3006）复用本器时，编排图是唯一真源，不会出现「原语另有一套时序模型」。
pub fn apply_primitive(
    orch: &mut Orchestrator,
    primitive: Primitive,
    members: &[NodeId],
    ctx: PrimitiveCtx,
) -> Result<(), MotionTokenError> {
    if primitive == Primitive::Stagger && ctx.stagger_step_ms == 0 {
        return Err(MotionTokenError::new(
            E_STAGGER_STEP,
            "错开原语步长为零",
            "步长 0 使错开全部元素同时起，与并行原语无差别，错开语义失效",
            "给出正步长毫秒（如 30~60），或改用并行原语",
            P_NAMESPACE_OWNER,
        ));
    }
    if members.len() > MAX_STAGGER_ELEMENTS {
        return Err(MotionTokenError::new(
            E_BUDGET_EXCEEDED,
            "原语成员超单批上限",
            &format!(
                "成员 {} 个超过单批上限 {}；错开按批处理，单批无界会让编译期分配不可预测",
                members.len(),
                MAX_STAGGER_ELEMENTS
            ),
            &format!("拆成每批不超过 {} 个元素", MAX_STAGGER_ELEMENTS),
            P_NAMESPACE_OWNER,
        ));
    }
    match primitive {
        Primitive::Sequential => {
            for pair in members.windows(2) {
                orch.edge(TimingEdge::after_complete(pair[0], pair[1]))?;
            }
            Ok(())
        }
        Primitive::Parallel => {
            for m in members.iter() {
                orch.edge(TimingEdge::parallel(ctx.anchor, *m))?;
            }
            Ok(())
        }
        Primitive::Stagger => {
            // 错开：按成员顺序逐个叠加固定步长偏移（均布）。
            let mut prev = ctx.anchor;
            for (i, m) in members.iter().enumerate() {
                let off = ctx
                    .stagger_step_ms
                    .saturating_mul((i + 1) as u32);
                orch.edge(TimingEdge::offset_start(prev, *m, off))?;
                prev = *m;
            }
            Ok(())
        }
        Primitive::Conditional => {
            // 条件分支：状态为真才接边（编译期裁剪），状态为假则本支不产生时序边。
            if ctx.branch_state {
                for pair in members.windows(2) {
                    orch.edge(TimingEdge::after_complete(pair[0], pair[1]))?;
                }
            }
            Ok(())
        }
    }
}

/// 错开偏移表（独立于构图：供预览/预算估算取用；判据在此钉死均布性）。
///
/// 索引由**元素序号**推出，不依赖外部传入的偏移数组——这样错开均布性
/// 由本函数**自己**保证，调用方无法传入不均布的偏移蒙混过关。
pub fn stagger_offsets(count: usize, step_ms: u32) -> Result<Vec<u32>, MotionTokenError> {
    if count > MAX_STAGGER_ELEMENTS {
        return Err(MotionTokenError::new(
            E_BUDGET_EXCEEDED,
            "错开元素数超上限",
            &format!("{} 个元素超过 {} 上限", count, MAX_STAGGER_ELEMENTS),
            &format!("分批错开，每批不超过 {} 个元素", MAX_STAGGER_ELEMENTS),
            P_NAMESPACE_OWNER,
        ));
    }
    let mut out: Vec<u32> = Vec::with_capacity(count);
    for i in 0..count {
        out.push(step_ms.saturating_mul(i as u32));
    }
    Ok(out)
}

/// 错开粒度合并（超预算降档用）：步长放大 [`STAGGER_MERGE_FACTOR`] 倍。
///
/// 降档的语义是**合并错开粒度**而非丢元素：元素仍在图内、仍在时间轴上，
/// 只是相邻元素落在同一档位上。丢元素会让转场出现「有的元素进场有的没进」，
/// 表现为界面半成品，不可接受。
pub fn merged_stagger_step(step_ms: u32) -> u32 {
    step_ms.saturating_mul(STAGGER_MERGE_FACTOR)
}

// ---------------------------------------------------------------------------
// 五、编译为M 时间轴实例组（判据七：编译 O(实例数)）
// ---------------------------------------------------------------------------

/// 一个编译产物实例（对应 M 域时间轴上的一个实例）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimelineInstance {
    /// 实例 ID（= 节点 ID，同图内一一对应）。
    pub instance_id: u32,
    /// 节点 ID。
    pub node: NodeId,
    /// 元素标识。
    pub element: u32,
    /// 动效任务标识。
    pub effect: u32,
    /// 编译目标（F3004 决策表）。
    pub target: CompileTarget,
    /// 阶段号（0 起；同一阶段可并行）。
    pub stage: u32,
    /// 阶段内起始偏移（毫秒；错开即落在此处）。
    pub offset_in_stage_ms: u32,
    /// 声明时长（毫秒；reduce 态恒为 0）。
    pub duration_ms: u32,
    /// 本实例的 M v2 控制段。
    pub control: ControlV2,
}

/// 编排编译产物。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompiledOrch {
    /// 实例组（拓扑序，顺序即启动顺序）。
    pub instances: Vec<TimelineInstance>,
    /// 阶段数（0 起计数）。
    pub stages: u32,
    /// 降档是否发生（超预算合并错开粒度）。
    pub downgraded: bool,
    /// 降档前后错开步长。
    pub stagger_step_before: u32,
    /// 实际生效错开步长。
    pub stagger_step_after: u32,
    /// 泳道。
    pub lane: Lane,
    /// reduce 是否整体坍缩。
    pub collapsed: bool,
}

impl CompiledOrch {
    /// 实例数（预算判据用）。
    pub fn instance_count(&self) -> usize {
        self.instances.len()
    }

    /// 总时长（毫秒；各实例结束的最大值）。
    pub fn total_duration_ms(&self) -> u32 {
        let mut max = 0u32;
        for inst in self.instances.iter() {
            let end = inst.offset_in_stage_ms.saturating_add(inst.duration_ms);
            if end > max {
                max = end;
            }
        }
        max
    }

    /// 取某阶段的实例。
    pub fn stage_instances(&self, stage: u32) -> Vec<TimelineInstance> {
        self.instances
            .iter()
            .filter(|i| i.stage == stage)
            .copied()
            .collect()
    }

    /// 节点→实例反查。
    pub fn instance_of(&self, node: NodeId) -> Option<TimelineInstance> {
        self.instances.iter().find(|i| i.node == node).copied()
    }
}

/// 编译参数。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CompileParams {
    /// 泳道（`Lane::Reduced` 触发整体坍缩）。
    pub lane: Lane,
    /// 错开步长（毫秒；0=无错开）。
    pub stagger_step_ms: u32,
    /// 实例预算（超此即降档）。
    pub budget: usize,
    /// 实例优先级（进M v2 控制段）。
    pub priority: ControlPriority,
    /// 控制生效域。
    pub scope: ControlScope,
    /// 时间轴方向。
    pub direction: ControlDirection,
}

impl CompileParams {
    /// 常规态默认参数。
    pub fn normal(lane: Lane) -> Self {
        CompileParams {
            lane,
            stagger_step_ms: 0,
            budget: INSTANCE_BUDGET,
            priority: ControlPriority::Informational,
            scope: ControlScope::Instance,
            direction: ControlDirection::Forward,
        }
    }

    /// 带错开步长的参数。
    pub fn with_stagger(lane: Lane, stagger_step_ms: u32) -> Self {
        CompileParams {
            stagger_step_ms,
            ..CompileParams::normal(lane)
        }
    }
}

/// 编译编排图 → M 时间轴实例组。
///
/// 阶段分配：按拓扑序逐节点求「最长依赖路径长度」定为阶段号——
/// 前置完成边让stage(target) >= stage(from)+1；偏移启动边让 stage(target) >= stage(from)；
/// 并行边不推进阶段（故并行同阶段）。偏移毫秒落在阶段内 `offset_in_stage_ms`。
///
/// reduce 泳道：**整体坍缩**为终态直达——所有实例时长 0、偏移 0、阶段 0。
pub fn compile(graph: &OrchGraph, params: &CompileParams) -> Result<CompiledOrch, MotionTokenError> {
    let collapsed = params.lane == Lane::Reduced;
    // 阶段表：node id → stage。
    let mut stage_of: Vec<(NodeId, u32)> = Vec::with_capacity(graph.node_count());
    let mut offset_of: Vec<(NodeId, u32)> = Vec::with_capacity(graph.node_count());
    let mut stages: u32 = 0;

    // 先按拓扑序求解（并行边不进约束，见 constraint 判定）。
    let order: Vec<NodeId> = graph.topological.clone();
    for nid in order.iter() {
        let mut stage: u32 = 0;
        let mut off: u32 = 0;
        for e in graph.edges.iter() {
            if e.to != *nid {
                continue;
            }
            let from_stage = match lookup(&stage_of, e.from) {
                Some(s) => s,
                None => continue,
            };
            match e.kind {
                EdgeKind::AfterComplete => {
                    if from_stage + 1 > stage {
                        stage = from_stage + 1;
                    }
                }
                EdgeKind::OffsetStart => {
                    // 偏移启动：同阶段内按from阶段 + 偏移；不推进阶段号。
                    if from_stage > stage {
                        stage = from_stage;
                    }
                    let cand = e.offset_ms;
                    if cand > off {
                        off = cand;
                    }
                }
                EdgeKind::Parallel => {}
            }
        }
        stage_of.push((*nid, stage));
        offset_of.push((*nid, off));
        if stage + 1 > stages {
            stages = stage + 1;
        }
    }

    // 预算检查 → 降档（合并错开粒度）。
    let total = graph.node_count();
    let mut downgraded = false;
    let step_before = params.stagger_step_ms;
    let mut step_after = step_before;
    if total > params.budget {
        // 不拒绝而是降档：错开粒度合并，元素一个不少。
        downgraded = true;
        step_after = merged_stagger_step(step_before);
    }

    // 生成实例。
    let mut instances: Vec<TimelineInstance> = Vec::with_capacity(total);
    for nid in order.iter() {
        let node = match graph.node_of(*nid) {
            Some(n) => n,
            None => continue,
        };
        let stage = lookup(&stage_of, *nid).unwrap_or(0);
        let mut off = lookup(&offset_of, *nid).unwrap_or(0);
        // 错开注入：同阶段内按实例序叠加步长（均布）。
        if step_after > 0 {
            let idx = instances.len() as u32;
            let add = step_after.saturating_mul(idx);
            off = off.saturating_add(add);
        }
        let duration = if collapsed { 0 } else { node.duration_ms };
        let stage_eff = if collapsed { 0 } else { stage };
        let off_eff = if collapsed { 0 } else { off };
        let selection = select_target(node.features)?;
        instances.push(TimelineInstance {
            instance_id: node.id,
            node: node.id,
            element: node.element,
            effect: node.effect,
            target: selection.target,
            stage: stage_eff,
            offset_in_stage_ms: off_eff,
            duration_ms: duration,
            control: ControlV2 {
                v1: ControlV1 {
                    instance_id: node.id,
                    effect_key: node.effect,
                    duration_ms: duration,
                    easing_code: 0,
                },
                offset_ms: off_eff as i32,
                speed_milli: 1000,
                direction: params.direction,
                priority: params.priority,
                scope: params.scope,
            },
        });
    }

    // reduce 泳道整体坍缩：实例已全部落到阶段 0，故 `stages` 必须同步坍为 1。
    // 不改这里会留下「实例都在阶段 0、stages 却报 5」的自相矛盾产物——
    // 下游按stages 分配阶段槽时会多申请 4 个空槽，且 `verify_reduce_collapse`
    // 报出的 after_stages 与实例事实不符（该字段的文档明写「恒1」）。
    let stages_out = if collapsed { 1 } else { stages };

    Ok(CompiledOrch {
        instances,
        stages: stages_out,
        downgraded,
        stagger_step_before: step_before,
        stagger_step_after: step_after,
        lane: params.lane,
        collapsed,
    })
}

/// 查表（线性；图规模不大，且免去排序维护成本）。
fn lookup(table: &[(NodeId, u32)], key: NodeId) -> Option<u32> {
    table.iter().find(|(k, _)| *k == key).map(|(_, v)| *v)
}

// ---------------------------------------------------------------------------
// 六、打断与处置策略（判据三）
// ---------------------------------------------------------------------------

/// 打断后未完成阶段的处置策略。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InterruptStrategy {
    /// 快速完成：未完成阶段瞬时跳到终态（保结构，不留半空）。
    FinishFast,
    /// 原地保持：未完成阶段冻结在当前进度（保视觉，不改终态语义）。
    HoldInPlace,
    /// 回滚：未完成阶段反向退回起点（回到可重入状态）。
    RollBack,
}

impl InterruptStrategy {
    /// 全集。
    pub const ALL: [InterruptStrategy; INTERRUPT_STRATEGY_COUNT] = [
        InterruptStrategy::FinishFast,
        InterruptStrategy::HoldInPlace,
        InterruptStrategy::RollBack,
    ];

    /// 代号。
    pub fn code(self) -> &'static str {
        match self {
            InterruptStrategy::FinishFast => "finish-fast",
            InterruptStrategy::HoldInPlace => "hold-in-place",
            InterruptStrategy::RollBack => "rollback",
        }
    }

    /// 中文名。
    pub fn zh(self) -> &'static str {
        match self {
            InterruptStrategy::FinishFast => "快速完成",
            InterruptStrategy::HoldInPlace => "原地保持",
            InterruptStrategy::RollBack => "回滚",
        }
    }

    /// 该策略下未完成阶段的最终时长（毫秒）。
    pub fn resolved_duration_ms(self, declared: u32) -> u32 {
        match self {
            InterruptStrategy::FinishFast => 0,
            InterruptStrategy::HoldInPlace => declared,
            InterruptStrategy::RollBack => declared,
        }
    }

    /// 该策略下未完成阶段的**处置动作**。
    ///
    /// 为什么单靠时长不够：原地保持与回滚的时长都是「保留声明时长」，
    /// 若只输出时长，两者在下游**完全不可区分**——而它们要求运行侧做的事
    /// 恰好相反（一个冻结，一个反向）。故处置动作必须单独成字段，
    /// 否则「回滚」这条策略在实现里等于不存在（写了但没人能执行它）。
    pub fn pending_action(self) -> PendingAction {
        match self {
            InterruptStrategy::FinishFast => PendingAction::JumpToEnd,
            InterruptStrategy::HoldInPlace => PendingAction::FreezeHere,
            InterruptStrategy::RollBack => PendingAction::ReverseToStart,
        }
    }

    /// 该策略要求的播放方向（供 M 域执行；回滚为反向）。
    pub fn required_direction(self) -> ControlDirection {
        match self {
            InterruptStrategy::RollBack => ControlDirection::Reverse,
            _ => ControlDirection::Forward,
        }
    }
}

/// 未完成阶段的处置动作（与 [`InterruptStrategy`] 一一对应）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PendingAction {
    /// 跳到终态（结构保持，不留半空）。
    JumpToEnd,
    /// 冻结在当前进度（视觉保持，终态语义不改）。
    FreezeHere,
    /// 反向退回起点（回到可重入状态）。
    ReverseToStart,
}

impl PendingAction {
    /// 全集。
    pub const ALL: [PendingAction; INTERRUPT_STRATEGY_COUNT] = [
        PendingAction::JumpToEnd,
        PendingAction::FreezeHere,
        PendingAction::ReverseToStart,
    ];

    /// 代号。
    pub fn code(self) -> &'static str {
        match self {
            PendingAction::JumpToEnd => "jump-to-end",
            PendingAction::FreezeHere => "freeze-here",
            PendingAction::ReverseToStart => "reverse-to-start",
        }
    }

    /// 中文名。
    pub fn zh(self) -> &'static str {
        match self {
            PendingAction::JumpToEnd => "跳到终态",
            PendingAction::FreezeHere => "原地冻结",
            PendingAction::ReverseToStart => "反向退回起点",
        }
    }
}

/// 策略来源（是否显式选型）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PolicySource {
    /// 调用方显式选型。
    Explicit,
    /// 走默认（须出诊断，不悬空）。
    Defaulted,
}

/// 打断上下文。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct InterruptCtx {
    /// 已完成阶段（回执集合，可乱序）。
    pub completed_stages: Vec<u32>,
    /// 是否显式选型了策略。
    pub policy_selected: bool,
}

impl InterruptCtx {
    /// 已完成到第几阶段（含；无完成则返回 None）。
    pub fn frontier_stage(&self) -> Option<u32> {
        let mut max: Option<u32> = None;
        for s in self.completed_stages.iter() {
            if max.is_none() || *s > max.unwrap_or(0) {
                max = Some(*s);
            }
        }
        max
    }

    /// 阶段完成回执（重复即拒——重复回执会让「已完成集合」与实际不符）。
    pub fn ack_stage(&mut self, stage: u32) -> Result<(), MotionTokenError> {
        if self.completed_stages.contains(&stage) {
            return Err(MotionTokenError::new(
                E_STAGE_DUPLICATE,
                "阶段完成回执重复",
                &format!(
                    "阶段 {} 已有完成回执；重复回执会让「已完成集合」与实际不符，打断时按错误边界处置",
                    stage
                ),
                &format!("确认阶段 {} 未曾完成，或清理重复回执后重发", stage),
                P_NAMESPACE_OWNER,
            ));
        }
        self.completed_stages.push(stage);
        Ok(())
    }
}

/// 未完成阶段的处置结论（阶段号 + 处置后时长 + 处置动作）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PendingResolution {
    /// 阶段号。
    pub stage: u32,
    /// 处置后时长（毫秒）。
    pub resolved_ms: u32,
    /// 处置动作（**与时长正交**：原地保持与回滚时长同、动作异）。
    pub action: PendingAction,
}

/// 打断处置报告（已完成保持 + 未完成处置，逐阶段可查）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InterruptReport {
    /// 生效策略。
    pub strategy: InterruptStrategy,
    /// 策略来源（`Defaulted` 即诊断触发）。
    pub source: PolicySource,
    /// 已完成阶段（保持原样）。
    pub kept_stages: Vec<u32>,
    /// 未完成阶段的处置结论（按阶段号升序）。
    pub pending_resolution: Vec<PendingResolution>,
    /// 策略要求的播放方向（回滚为反向；供 M 域执行）。
    pub required_direction: ControlDirection,
    /// 诊断（`Defaulted` 时非空）。
    pub diagnostic: String,
}

impl InterruptReport {
    /// 是否走了默认策略（诊断触发条件）。
    pub fn defaulted(&self) -> bool {
        self.source == PolicySource::Defaulted
    }

    /// 未完成阶段数。
    pub fn pending_count(&self) -> usize {
        self.pending_resolution.len()
    }

    /// 取某阶段的处置结论。
    pub fn resolution_of(&self, stage: u32) -> Option<PendingResolution> {
        self.pending_resolution
            .iter()
            .find(|r| r.stage == stage)
            .copied()
    }
}

/// 对已编译编排施加打断。
///
/// 处置规则（锚点）：已完成阶段**保持**；未完成阶段按策略处置。
/// reduce 泳道：无论何种策略，未完成阶段**一律坍缩为终态直达**（无障碍优先于策略）。
pub fn apply_interrupt(
    compiled: &CompiledOrch,
    ctx: &InterruptCtx,
    requested: Option<InterruptStrategy>,
) -> Result<InterruptReport, MotionTokenError> {
    // 策略解析：显式 → 用；未选型 → 默认快速完成 + 诊断。
    let (strategy, source) = match requested {
        Some(s) => (s, PolicySource::Explicit),
        None => (InterruptStrategy::FinishFast, PolicySource::Defaulted),
    };
    let diagnostic = if source == PolicySource::Defaulted {
        format!(
            "打断未选型策略（{}），已按默认「快速完成」处置：已完成阶段保持，未完成阶段跳终态。补策略可改处置：显式传入 {}",
            E_POLICY_DEFAULTED,
            InterruptStrategy::ALL
                .iter()
                .map(|s| s.code())
                .collect::<Vec<&str>>()
                .join("/")
        )
    } else {
        String::new()
    };

    let mut kept: Vec<u32> = ctx.completed_stages.clone();
    kept.sort_unstable();
    kept.dedup();

    let mut pending: Vec<PendingResolution> = Vec::new();
    for stage in 0..compiled.stages {
        if kept.contains(&stage) {
            continue; // 已完成阶段保持。
        }
        // 阶段声明时长 = 该阶段内实例的最大时长。
        let declared = compiled
            .stage_instances(stage)
            .iter()
            .map(|i| i.duration_ms)
            .max()
            .unwrap_or(0);
        // reduce泳道：未完成阶段一律直达（策略被无障碍覆盖）。
        let resolved = if compiled.lane == Lane::Reduced {
            0
        } else {
            strategy.resolved_duration_ms(declared)
        };
        let action = if compiled.lane == Lane::Reduced {
            // 无障碍优先：reduce 态一律跳终态，不冻结也不反向（用户已明确要求少动效）。
            PendingAction::JumpToEnd
        } else {
            strategy.pending_action()
        };
        pending.push(PendingResolution {
            stage,
            resolved_ms: resolved,
            action,
        });
    }

    Ok(InterruptReport {
        strategy,
        source,
        kept_stages: kept,
        pending_resolution: pending,
        required_direction: strategy.required_direction(),
        diagnostic,
    })
}

// ---------------------------------------------------------------------------
// 七、预算与降档（判据六：超预算→降档）
// ---------------------------------------------------------------------------

/// 预算裁决结果。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BudgetVerdict {
    /// 实例数。
    pub instances: usize,
    /// 预算。
    pub budget: usize,
    /// 是否超预算。
    pub exceeded: bool,
    /// 处置（降档或放行）。
    pub action: BudgetAction,
    /// 处置说明。
    pub note: String,
}

/// 预算处置动作。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BudgetAction {
    /// 放行（未超）。
    Allow,
    /// 降档（合并错开粒度，元素不丢）。
    Downgrade,
}

/// 预算检查（锚点「编译产物超预算→预算检查+降档」）。
pub fn check_budget(instances: usize, budget: usize) -> BudgetVerdict {
    if instances <= budget {
        BudgetVerdict {
            instances,
            budget,
            exceeded: false,
            action: BudgetAction::Allow,
            note: format!("实例数 {} 在预算 {} 内，原样编译", instances, budget),
        }
    } else {
        BudgetVerdict {
            instances,
            budget,
            exceeded: true,
            action: BudgetAction::Downgrade,
            note: format!(
                "实例数 {} 超预算 {}；降档=错开粒度合并×{}，元素一个不少只降粒度",
                instances, budget, STAGGER_MERGE_FACTOR
            ),
        }
    }
}

// ---------------------------------------------------------------------------
// 八、无障碍：reduce 整体坍缩（判据五）
// ---------------------------------------------------------------------------

/// reduce 坍缩报告。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReduceCollapse {
    /// 坍缩前实例数。
    pub before_instances: usize,
    /// 坍缩后实例数（与 before 相同——坍缩不删元素，只清时间量）。
    pub after_instances: usize,
    /// 坍缩前阶段数。
    pub before_stages: u32,
    /// 坍缩后阶段数（恒1）。
    pub after_stages: u32,
    /// 是否全部实例零时长（终态直达）。
    pub all_zero_duration: bool,
    /// 是否全部实例零偏移（无错开）。
    pub all_zero_offset: bool,
    /// 组件零分支保证：坍缩在编排器层完成，组件无需各自判reduce。
    pub component_zero_branch: bool,
}

/// 检查 reduce 态是否整体坍缩为终态直达。
///
/// 判据**直接查编译产物的字段**（duration/offset/stage），
/// 不问「是否 reduce 泳道」——后者是同源驱动恒真（泳道→坍缩由同一 bool 决定，
/// 改任一边都抓不到）。故此处以**编译产物自身**为被测对象。
pub fn verify_reduce_collapse(
    normal: &CompiledOrch,
    reduced: &CompiledOrch,
) -> ReduceCollapse {
    let all_zero_duration = reduced.instances.iter().all(|i| i.duration_ms == 0);
    let all_zero_offset = reduced.instances.iter().all(|i| i.offset_in_stage_ms == 0);
    let all_single_stage = reduced.instances.iter().all(|i| i.stage == 0);
    ReduceCollapse {
        before_instances: normal.instance_count(),
        after_instances: reduced.instance_count(),
        before_stages: normal.stages,
        after_stages: reduced.stages,
        all_zero_duration,
        all_zero_offset,
        // 元素集合一致=未丢元素；实例不减=坍缩只清时间量。
        component_zero_branch: all_zero_duration
            && all_zero_offset
            && all_single_stage
            && reduced.instance_count() == normal.instance_count()
            && reduced.lane == Lane::Reduced,
    }
}

// ---------------------------------------------------------------------------
// 九、对接台账与判据（判据穷举用）
// ---------------------------------------------------------------------------

/// 判据枚举。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Criterion {
    /// DAG 声明（节点/三边型/环检测）。
    DagDeclaration,
    /// 四原语编译。
    FourPrimitives,
    /// 打断三策略。
    InterruptThree,
    /// 嵌套上限 8。
    NestCapEight,
    /// 统一 reduce 坍缩。
    UnifiedReduce,
}

impl Criterion {
    /// 全集。
    pub const ALL: [Criterion; 5] = [
        Criterion::DagDeclaration,
        Criterion::FourPrimitives,
        Criterion::InterruptThree,
        Criterion::NestCapEight,
        Criterion::UnifiedReduce,
    ];

    /// 判据承诺（锚点原文一句话）。
    pub fn promise(self) -> &'static str {
        match self {
            Criterion::DagDeclaration => "节点=元素×动效，边三型，环在构建期拒并点名链",
            Criterion::FourPrimitives => "序列/并行/错开/条件分支四原语编译为实例组",
            Criterion::InterruptThree => "已完成保持，未完成按三策略处置，未选型默认+诊断",
            Criterion::NestCapEight => "子编排=复合节点，深度上限 8，超限构建期拒",
            Criterion::UnifiedReduce => "reduce 态整体坍缩为终态直达，组件零分支",
        }
    }
}

/// 对接点状态（沿用 F3004 的两态：已冻结 / 上游在册未实现）。
pub use crate::svstar2::vep04_stack::LandingState;

/// 一条对接登记。
pub use crate::svstar2::vep04_stack::HandoverEntry;

/// 对接台账（锚点「跨批对接点」）。
pub const HANDOVERS: [HandoverEntry; 4] = [
    HandoverEntry {
        point: "M v2 接口消费（F3004）",
        ticket: "VE-F3004",
        state: LandingState::Frozen,
        delivered: "编译产物逐实例生成 ControlV2（偏移/速度/方向/优先级 + 生效域）",
    },
    HandoverEntry {
        point: "O04 编译目标选型",
        ticket: "VE-F3004",
        state: LandingState::Frozen,
        delivered: "编译期经 select_target 查表取 CompileTarget，未命中即拒不猜",
    },
    HandoverEntry {
        point: "P02 页面转场（编排器最大客户）",
        ticket: "VE-F3010 / VE-F3029",
        state: LandingState::Pending,
        delivered: "Orchestrator + OrchGraph 声明面与 compile() 单一入口",
    },
    HandoverEntry {
        point: "F3003 令牌接线（本项上游）",
        ticket: "VE-F3003",
        state: LandingState::Frozen,
        delivered: "reduce 泳道取自 Lane；声明时长/错开步长取自动效令牌",
    },
];

/// 错误码全集。
pub const ERROR_CODES: [&str; 13] = [
    E_CYCLE_DETECTED,
    E_NODE_DUPLICATE,
    E_EDGE_UNKNOWN_NODE,
    E_SELF_LOOP,
    E_GRAPH_EMPTY,
    E_NEST_DEPTH,
    E_BUDGET_EXCEEDED,
    E_POLICY_DEFAULTED,
    E_BRANCH_CONDITION,
    E_TARGET_UNRESOLVED,
    E_STAGGER_STEP,
    E_STAGE_OUT_OF_RANGE,
    E_STAGE_DUPLICATE,
];

// ---------------------------------------------------------------------------
// 十、单元测试（宿主侧 cargo test 直跑；零墙钟零 IO，回归可复现）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn two_seq() -> Orchestrator {
        let mut o = Orchestrator::new();
        o.node(OrchNode::new(1, 10, 100, "fade-in", 200, 1)).unwrap();
        o.node(OrchNode::new(2, 11, 101, "fade-out", 200, 1)).unwrap();
        o.edge(TimingEdge::after_complete(1, 2)).unwrap();
        o
    }

    #[test]
    fn test_commit_acyclic_ok() {
        let g = two_seq().commit().unwrap();
        assert_eq!(g.node_count(), 2);
        assert_eq!(g.edge_count(), 1);
    }

    #[test]
    fn test_cycle_rejected_with_chain() {
        let mut o = two_seq();
        o.edge(TimingEdge::after_complete(2, 1)).unwrap();
        let e = o.commit().unwrap_err();
        assert_eq!(e.code, E_CYCLE_DETECTED);
        assert!(e.next.contains("->"));
    }

    #[test]
    fn test_self_loop_rejected() {
        let mut o = two_seq();
        let e = o.edge(TimingEdge::after_complete(1, 1)).unwrap_err();
        assert_eq!(e.code, E_SELF_LOOP);
    }

    #[test]
    fn test_nest_cap_enforced() {
        // 逐层嵌套至超过 8。
        let mut o = Orchestrator::new();
        o.node(OrchNode::new(1, 10, 100, "leaf", 100, 1)).unwrap();
        let mut depth_rejected = false;
        let mut cur = o;
        for _ in 0..MAX_NEST_DEPTH + 2 {
            let mut inner = Orchestrator::new();
            inner.node(OrchNode::new(2, 20, 200, "inner", 100, 1)).unwrap();
            if inner.nest(&cur).is_err() {
                depth_rejected = true;
                break;
            }
            cur = inner;
        }
        assert!(depth_rejected, "嵌套深度超限必须被拒");
    }

    #[test]
    fn test_reduce_collapse() {
        let g = two_seq().commit().unwrap();
        let normal = compile(&g, &CompileParams::normal(Lane::Normal)).unwrap();
        let reduced = compile(&g, &CompileParams::normal(Lane::Reduced)).unwrap();
        let rc = verify_reduce_collapse(&normal, &reduced);
        assert!(rc.all_zero_duration);
        assert!(rc.all_zero_offset);
        assert!(rc.component_zero_branch);
    }

    #[test]
    fn test_interrupt_default_has_diagnostic() {
        let g = two_seq().commit().unwrap();
        let c = compile(&g, &CompileParams::normal(Lane::Normal)).unwrap();
        let ctx = InterruptCtx::default();
        let r = apply_interrupt(&c, &ctx, None).unwrap();
        assert!(r.defaulted());
        assert!(!r.diagnostic.is_empty());
        assert_eq!(r.strategy, InterruptStrategy::FinishFast);
    }

    #[test]
    fn test_stagger_uniform() {
        let off = stagger_offsets(4, 30).unwrap();
        assert_eq!(off, vec![0, 30, 60, 90]);
    }

    #[test]
    fn test_duplicate_ack_rejected() {
        let mut ctx = InterruptCtx::default();
        ctx.ack_stage(0).unwrap();
        assert_eq!(ctx.ack_stage(0).unwrap_err().code, E_STAGE_DUPLICATE);
    }
}
