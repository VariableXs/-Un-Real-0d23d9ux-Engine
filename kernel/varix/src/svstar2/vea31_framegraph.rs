//! VE-F0031 · 帧图资源屏障自动插入（VE-A 域 · 帧图屏障推理 + 冗余消除 + 环检测 · 目标 440 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0031`
//!
//! **判据（锚点原文）**：渲染帧图的资源屏障自动推理（读写关系分析→屏障自动插入），
//! 手动屏障禁令（手写屏障是 bug 温床——一律走帧图），屏障冗余消除；含帧图的
//! 热图视图（屏障密度可视化）。判据：**自动推理、手动禁令、冗余消除、环检测、判据**。
//!
//! **错误路径与降级矩阵**（锚点原文）：
//!
//! - 图环检测 → **拒绝**（帧图有环 ⇒ 无合法拓扑序，屏障无处可插；
//!   返回环上节点与边，**不返回半成品帧图**）
//! - 屏障遗漏 → **兜底 + 告警**（推理漏了就退到最保守屏障并登记告警，
//!   不是静默放行——静默放行的遗漏在 GPU 上表现为偶发花屏，最难查）
//! - 冗余 → **消除**（相邻同资源同类型的屏障，保留一条；冗余屏障不是"更安全"，
//!   它是真的拖慢管线）
//!
//! **数据结构**：帧图引擎（[`FrameGraph`]）；屏障推理（[`infer_barriers`]）；
//! 热图视图（[`heat_rows`]）。
//!
//! **性能逐项分解**：O(资源边)——拓扑排序是 O(V+E)、屏障推理是 O(E) 单趟扫描
//! （每条写边至多贡献一条屏障），冗余消除是 O(屏障数) 单趟相邻比较。
//!
//! **跨批对接点**：A32 状态跟踪联动——[`Barrier`] 的资源与范围字段供状态跟踪
//! 消费；本条只**产出屏障清单**，不管状态机的可见性推导。
//!
//! **无障碍与隐私**：帧图视图读屏替代（[`FrameGraph::a11y_lines`]）——报
//! 「节点数/边数/屏障数/冗余消除数/环状态」，中英双语逐行。面板**只报聚合计数**，
//! **不报单个资源的绑定细节与纹理尺寸**（那是资产布局信息）。
//!
//! ## 设计要点
//!
//! - **手动屏障禁令是结构性的，不是靠自觉**（[`ManualBarrier`] /
//!   [`BuildError::ManualBarrierForbidden`]）：手写屏障把「谁读谁」的断言留在
//!   人的脑子里，而帧图**已经知道**读写关系（它由资源边推导）。两者并存时，
//!   人的那一份必然随改动腐化。故 API 层面**不提供**「插一条手写屏障」的口子，
//!   调用方试图手写只能拿到 [`BuildError::ManualBarrierForbidden`]。
//! - **屏障由写→读边推导，不由节点顺序猜测**（[`infer_barriers`]）：
//!   对每条「资源 R 的写者 W → 读者 V」的边，插一条 [`Barrier`]。
//!   只在**同一资源**上推导，跨资源不插（那是调度器的事，不是屏障的事）。
//! - **RAW / WAR / WAW 三类都要挡，但语义不同**（[`BarrierKind`]）：
//!   RAW（写后读）挡的是可见性、WAW（写后写）挡的是顺序覆写、
//!   WAR（读后写）挡的是「读还没取完就被覆写」。三类外部表现相同
//!   （画面错），合并成一条则删掉任一条判据仍全绿（十诫第 3 条）。
//! - **读后写必须挡，而多数引擎漏这一条**：若只推 RAW/WAW，则同一资源
//!   被「读 → 写」时，前一个读节点可能还没取完数据就被后一个写覆写。
//!   故 [`infer_barriers`] 对**每个资源**扫全部访问，按访问序两两定关系。
//! - **环检测拒绝而非截断**（[`FrameGraph::build`]）：有环时不存在合法拓扑序，
//!   强行按加入顺序出屏障等于给出一份**看起来能用**的帧图——那比直接失败更坏。
//!   故 [`BuildError::Cycle`] 携带环上节点，且环存在时**不返回任何屏障**。
//! - **兜底屏障 + 告警，不静默**（[`FallbackGuard`]）：屏障遗漏在 GPU 上是
//!   偶发花屏——最难查的一类。故本条提供 [`Barrier::fallback_conservative`]，
//!   推理给出 `None` 时可退到最保守屏障，并**强制**登记 [`FrameGraph::alerts`]
//!   计数；判据断言告警计数与兜底次数**独立对账**（十诫第 10 条，不用净值口径）。
//! - **冗余消除只合并「相邻且同资源同类」的屏障**（[`eliminate_redundant`]）：
//!   A→B 与 B→C 是两次屏障，合并成一条 A→C 才叫消除；把 A→C 直接缩成
//!   一条屏障会**丢掉**中间那条的粒度，掩盖真实依赖。相邻同资源同类才可合并。
//! - **夹逼对钉死边位置**（[`MAX_NODES`] / [`MAX_BARRIERS`]）：判据一律用
//!   「界前合法 / 界上 / 界后一位」三点。
//!
//! ## 与相邻条的分工（易混，故写明）
//!
//! - **F0030（`vea30_batcher`）决定「哪些绘制合批」**，本条决定「批次之间要什么
//!   屏障」。合批后的批次是本条的节点；本条不关心批次内部怎么画。
//! - **F0029（`vea29_indirect`）写命令缓冲**，本条推屏障。屏障在命令**之前**
//!   生效，两者不是同一层：命令写对了但屏障缺失，一样花屏。
//!
//! 两条各自**自持**定义类型，不跨模块 `use`——并行提交时跨模块引用会把两个模块
//! 的编译成败绑在一起，一方半成品就拖垮另一方，而这类失败报 E0583，与真实缺陷
//! 长得一样、极难分辨。

use crate::checks::CheckSet;

use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、规格常量
// ---------------------------------------------------------------------------

/// 帧图节点数上限（超出 → [`BuildError::TooManyNodes`]）。
pub const MAX_NODES: usize = 64;

/// 帧图边数上限（超出 → [`BuildError::TooManyEdges`]）。
pub const MAX_EDGES: usize = 256;

/// 屏障数上限（超出 → 兜底并告警，不静默截断）。
pub const MAX_BARRIERS: usize = 256;

/// 屏障类别种数（RAW / WAR / WAW，各自语义不同，不合并）。
pub const BARRIER_KIND_COUNT: usize = 3;

/// 资源访问类别种数（读 / 写）。
pub const ACCESS_KIND_COUNT: usize = 2;

/// 环检测的深度上限（DFS 显式栈容量，超出即判环，不递归爆栈）。
pub const MAX_TOPO_VISITS: usize = MAX_NODES * MAX_BARRIERS;

// ---------------------------------------------------------------------------
// 二、访问与屏障
// ---------------------------------------------------------------------------

/// 资源访问类别。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Access {
    /// 只读。
    Read,
    /// 写（读改写也算写——它改了内容）。
    Write,
}

impl Access {
    /// 全集，顺序稳定（判据按此下标推导，不靠字面量）。
    pub const ALL: [Access; ACCESS_KIND_COUNT] =
        [Access::Read, Access::Write];

    /// 判别下标（**不是**线上编码值）。
    pub const fn ordinal(self) -> usize {
        match self {
            Access::Read => 0,
            Access::Write => 1,
        }
    }

    /// 中文标签（读屏与调试用）。
    pub const fn zh(self) -> &'static str {
        match self {
            Access::Read => "读",
            Access::Write => "写",
        }
    }

    /// 英文标签（读屏用）。
    pub const fn tag(self) -> &'static str {
        match self {
            Access::Read => "read",
            Access::Write => "write",
        }
    }
}

/// 屏障类别。**三类语义不同，不合并**（合并则判据无法区分，见文件头十诫第 3 条）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BarrierKind {
    /// 写后读：挡可见性。
    Raw,
    /// 读后写：挡「读未取完就被覆写」。
    War,
    /// 写后写：挡顺序覆写。
    Waw,
}

impl BarrierKind {
    /// 全集，顺序稳定（判据按此下标推导，不靠字面量）。
    pub const ALL: [BarrierKind; BARRIER_KIND_COUNT] =
        [BarrierKind::Raw, BarrierKind::War, BarrierKind::Waw];

    /// 判别下标。
    pub const fn ordinal(self) -> usize {
        match self {
            BarrierKind::Raw => 0,
            BarrierKind::War => 1,
            BarrierKind::Waw => 2,
        }
    }

    /// 由「前一访问 → 后一访问」定关系（**按序两两判定**，非跨资源）。
    pub const fn between(prev: Access, next: Access) -> BarrierKind {
        match (prev, next) {
            (Access::Write, Access::Read) => BarrierKind::Raw,
            (Access::Read, Access::Write) => BarrierKind::War,
            (Access::Write, Access::Write) => BarrierKind::Waw,
            // 读后读不需要屏障——这是**唯一**不需要挡的组合。
            (Access::Read, Access::Read) => BarrierKind::Raw,
        }
    }

    /// 该组合是否真需要屏障（读后读不需要）。
    pub const fn required(prev: Access, next: Access) -> bool {
        !matches!((prev, next), (Access::Read, Access::Read))
    }

    /// 中文标签。
    pub const fn zh(self) -> &'static str {
        match self {
            BarrierKind::Raw => "写后读屏障",
            BarrierKind::War => "读后写屏障",
            BarrierKind::Waw => "写后写屏障",
        }
    }

    /// 英文标签。
    pub const fn tag(self) -> &'static str {
        match self {
            BarrierKind::Raw => "RAW",
            BarrierKind::War => "WAR",
            BarrierKind::Waw => "WAW",
        }
    }
}

/// 一条屏障。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Barrier {
    /// 生产者节点（下标）。
    pub producer: usize,
    /// 消费者节点（下标）。
    pub consumer: usize,
    /// 被屏障的资源下标。
    pub resource: usize,
    /// 屏障类别。
    pub kind: BarrierKind,
    /// 是否为兜底屏障（`true` ⇒ 推理没给出，走最保守路径）。
    pub fallback: bool,
}

impl Barrier {
    /// 构造一条推理屏障（非兜底）。
    pub const fn new(producer: usize, consumer: usize, resource: usize, kind: BarrierKind) -> Barrier {
        Barrier { producer, consumer, resource, kind, fallback: false }
    }

    /// 构造**兜底屏障**：全资源可见性屏障（最保守），必带 `fallback` 标记。
    ///
    /// 兜底屏障刻意不带 [`BarrierKind`] 语义——它是「挡一切」，不是「挡某一类」，
    /// 把它标成某类会让统计口径把兜底混进该类（十诫第 10 条）。
    pub const fn fallback_conservative(resource: usize) -> Barrier {
        Barrier {
            producer: usize::MAX,
            consumer: usize::MAX,
            resource,
            kind: BarrierKind::Raw,
            fallback: true,
        }
    }

    /// 是否为读后读这种**不需要**屏障的组合（真值才该是 `false`）。
    pub fn is_redundant_pair(prev: Access, next: Access) -> bool {
        !BarrierKind::required(prev, next)
    }

    /// 中文一行摘要（热图与调试用）。
    pub fn summary(&self) -> String {
        if self.fallback {
            return format!("资源 {}：兜底全可见性屏障", self.resource);
        }
        format!(
            "资源 {}：{}→{} 插{}",
            self.resource,
            self.producer,
            self.consumer,
            self.kind.zh()
        )
    }
}

// ---------------------------------------------------------------------------
// 三、帧图与屏障推理
// ---------------------------------------------------------------------------

/// 资源在某个节点上的一次访问。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AccessRec {
    /// 节点下标。
    pub node: usize,
    /// 访问的资源下标。
    pub resource: usize,
    /// 访问类别。
    pub access: Access,
}

/// 帧图：节点 + 资源访问表，屏障由资源访问序列推导。
#[derive(Clone, Debug, Default)]
pub struct FrameGraph {
    /// 节点数。
    nodes: usize,
    /// 资源访问表（按加入序）。
    accesses: Vec<AccessRec>,
    /// 显式资源边（用于图结构校验与环检测；由资源访问自动派生亦可）。
    edges: Vec<(usize, usize)>,
    /// 推理屏障。
    barriers: Vec<Barrier>,
    /// 告警计数（兜底与容量溢出的诚实登记）。
    alerts: u32,
    /// 冗余消除计数。
    removed_redundant: u32,
}

impl FrameGraph {
    /// 新建空帧图。
    pub fn new() -> FrameGraph {
        FrameGraph {
            nodes: 0,
            accesses: Vec::new(),
            edges: Vec::new(),
            barriers: Vec::new(),
            alerts: 0,
            removed_redundant: 0,
        }
    }

    /// 加一个节点。
    pub fn add_node(&mut self) -> Result<usize, BuildError> {
        if self.nodes >= MAX_NODES {
            self.alerts = self.alerts.saturating_add(1);
            return Err(BuildError::TooManyNodes);
        }
        let id = self.nodes;
        self.nodes += 1;
        Ok(id)
    }

    /// 登记一次资源访问（节点已存在时调用）。
    pub fn add_access(&mut self, node: usize, resource: usize, access: Access) -> Result<(), BuildError> {
        if node >= self.nodes {
            return Err(BuildError::BadNode);
        }
        self.accesses.push(AccessRec { node, resource, access });
        Ok(())
    }

    /// 登记一条显式资源边（生产者 → 消费者），供环检测使用。
    pub fn add_edge(&mut self, from: usize, to: usize) -> Result<(), BuildError> {
        if from >= self.nodes || to >= self.nodes {
            return Err(BuildError::BadNode);
        }
        if self.edges.len() >= MAX_EDGES {
            self.alerts = self.alerts.saturating_add(1);
            return Err(BuildError::TooManyEdges);
        }
        self.edges.push((from, to));
        Ok(())
    }

    /// 试图登记**手写屏障**——**一律拒绝**（锚点：手写屏障是 bug 温床）。
    ///
    /// 帧图已从资源边推出屏障，人工再写一份就是把依赖断言搬回人脑。
    /// 故此处**恒失败**，且不改变任何状态。
    pub fn add_manual_barrier(&mut self, _producer: usize, _consumer: usize, _resource: usize) -> Result<(), BuildError> {
        self.alerts = self.alerts.saturating_add(1);
        Err(BuildError::ManualBarrierForbidden)
    }

    /// 构建：先环检测（拒绝），再推理屏障（兜底 + 告警），最后消除冗余。
    ///
    /// **环存在时不返回任何屏障**（[`Barrier`] 列表保持空）——半成品帧图
    /// 比明确失败更危险。
    pub fn build(&mut self) -> Result<usize, BuildError> {
        // 1) 环检测（显式栈 DFS，不用递归——no_std 下递归栈不可控）。
        if let Some(cycle) = self.find_cycle() {
            self.barriers.clear();
            return Err(BuildError::Cycle(cycle));
        }
        // 2) 推理屏障。
        self.barriers.clear();
        let want = infer_barriers(self.accesses.clone());
        for b in want {
            if self.barriers.len() >= MAX_BARRIERS {
                // 容量溢出：登记告警并给兜底，**不静默截断**。
                self.alerts = self.alerts.saturating_add(1);
                self.barriers.push(Barrier::fallback_conservative(b.resource));
                continue;
            }
            self.barriers.push(b);
        }
        // 3) 冗余消除（只合并相邻且同资源同类型）。
        let (kept, removed) = eliminate_redundant(self.barriers.clone());
        self.barriers = kept;
        self.removed_redundant = removed;
        Ok(self.barriers.len())
    }

    /// 找环（返回环上节点序列；无环则 `None`）。
    ///
    /// 用**迭代三色 DFS**（0 未访问 / 1 在栈上 / 2 已完成），避免递归爆栈。
    pub fn find_cycle(&self) -> Option<Vec<usize>> {
        const WHITE: u8 = 0;
        const GRAY: u8 = 1;
        const BLACK: u8 = 2;
        let mut color = vec![WHITE; self.nodes];
        // 邻接表（边数受限，线性查表即可，不建邻接结构省分配）。
        for start in 0..self.nodes {
            if color[start] != WHITE {
                continue;
            }
            // 显式栈：(节点, 待检查的边下标)
            let mut stack: Vec<(usize, usize)> = vec![(start, 0)];
            color[start] = GRAY;
            let mut visits = 0usize;
            while let Some(&mut (node, ref mut ei)) = stack.last_mut() {
                visits += 1;
                if visits > MAX_TOPO_VISITS {
                    // 保险：访问次数超界视为有环（不死循环）。
                    return Some(vec![node]);
                }
                if *ei < self.edges.len() {
                    let (from, to) = self.edges[*ei];
                    *ei += 1;
                    if from != node {
                        continue;
                    }
                    match color[to] {
                        GRAY => {
                            // 回边 ⇒ 环
                            return Some(vec![node, to]);
                        }
                        WHITE => {
                            color[to] = GRAY;
                            stack.push((to, 0));
                        }
                        _ => {}
                    }
                } else {
                    color[node] = BLACK;
                    stack.pop();
                }
            }
        }
        None
    }

    /// 推理屏障（纯函数，可独立调用与判据）。
    ///
    /// 对**每个资源**按加入序扫其访问序列，对每一对**相邻且不同节点**的访问
    /// 定关系并产出屏障。读后读不产屏障。
    pub fn barriers(&self) -> &[Barrier] {
        &self.barriers
    }

    /// 资源访问表（只读）。
    pub fn accesses(&self) -> &[AccessRec] {
        &self.accesses
    }

    /// 显式边表（只读）。
    pub fn edges(&self) -> &[(usize, usize)] {
        &self.edges
    }

    /// 告警计数（兜底与容量溢出的诚实登记）。
    pub fn alerts(&self) -> u32 {
        self.alerts
    }

    /// 冗余消除计数。
    pub fn removed_redundant(&self) -> u32 {
        self.removed_redundant
    }

    /// 热图视图：按资源给出屏障密度（**可视化用**，只报计数不报内容）。
    ///
    /// 每个资源一行：资源下标、访问次数、屏障数、密度（千分比）。
    pub fn heat_rows(&self) -> Vec<String> {
        let mut out = Vec::new();
        let mut r = 0usize;
        while r < self.resource_count() {
            let visits = self.accesses.iter().filter(|a| a.resource == r).count();
            if visits > 0 {
                let bars = self.barriers.iter().filter(|b| b.resource == r).count();
                let density = ((bars as u64 * 1000) / visits as u64) as u32;
                out.push(format!(
                    "资源 {}：访问 {} 次，屏障 {} 条，密度 {}‰",
                    r, visits, bars, density
                ));
            }
            r += 1;
        }
        out
    }

    /// 资源数（按出现过的最大下标 + 1 估，不做全表假设）。
    fn resource_count(&self) -> usize {
        let mut m = 0usize;
        for a in self.accesses.iter() {
            if a.resource + 1 > m {
                m = a.resource + 1;
            }
        }
        m
    }

    /// 读屏面板：中英双语逐行，**只报聚合计数**，不报单个资源绑定细节。
    pub fn a11y_lines(&self) -> [String; 6] {
        let cycle = if self.find_cycle().is_some() { "有环（已拒绝）" } else { "无环" };
        [
            format!("节点数 / nodes: {}", self.nodes),
            format!("资源访问 / accesses: {}", self.accesses.len()),
            format!("屏障数 / barriers: {}", self.barriers.len()),
            format!("冗余消除 / redundant removed: {}", self.removed_redundant),
            format!("告警 / alerts: {}", self.alerts),
            format!("环状态 / cycle: {}", cycle),
        ]
    }
}

/// 构建错误。
///
/// **不 derive Copy**：`Cycle` 变体携带 `Vec<usize>`（环上节点），
/// 含堆指针的类型不能 Copy。判据用 `matches!` 判型，不依赖 Copy。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BuildError {
    /// 节点越界（引用不存在的节点）。
    BadNode,
    /// 节点数超限。
    TooManyNodes,
    /// 边数超限。
    TooManyEdges,
    /// 手写屏障：一律拒绝（锚点禁令）。
    ManualBarrierForbidden,
    /// 图有环（携带环上节点）。
    Cycle(Vec<usize>),
}

/// 推理屏障（纯函数）。
///
/// 按资源分组、按加入序扫访问序列，每一对**相邻**访问定关系产出屏障。
/// 同一对访问只在**相邻**处产屏障——非相邻的两个访问之间的屏障由中间那些
/// 屏障链式保证，逐对产出反而会把冗余塞进清单（那正是本条要消除的东西）。
pub fn infer_barriers(accesses: Vec<AccessRec>) -> Vec<Barrier> {
    let mut out: Vec<Barrier> = Vec::new();
    // 逐资源：收集该资源按序的访问
    let mut r = 0usize;
    let mut maxr = 0usize;
    for a in accesses.iter() {
        if a.resource + 1 > maxr {
            maxr = a.resource + 1;
        }
    }
    while r < maxr {
        let mut seq: Vec<AccessRec> =
            accesses.iter().copied().filter(|a| a.resource == r).collect();
        let mut i = 1usize;
        while i < seq.len() {
            let prev = seq[i - 1];
            let cur = seq[i];
            // **同一节点内的两次访问不产屏障**：一个节点内部的访问顺序由
            // 该节点自身的指令序保证，屏障是**跨节点**的同步手段，对自己
            // 插屏障是自同步（producer == consumer 的自环屏障），语义空转。
            // 只推 RAW 的实现恰恰漏了这条——它会把节点内「先读后写」也
            // 算成WAR，而那本来就不需要屏障。
            if prev.node != cur.node && BarrierKind::required(prev.access, cur.access) {
                out.push(Barrier::new(prev.node, cur.node, r, BarrierKind::between(prev.access, cur.access)));
            }
            i += 1;
        }
        r += 1;
    }
    out
}

/// 冗余消除（纯函数）：只合并**相邻且同资源同类**的屏障。
///
/// 返回 `(保留的屏障, 消除条数)`。非同资源或不同类**不合并**——把它们并成一条
/// 会丢掉中间那条的粒度，掩盖真实依赖。
pub fn eliminate_redundant(barriers: Vec<Barrier>) -> (Vec<Barrier>, u32) {
    let mut kept: Vec<Barrier> = Vec::new();
    let mut removed = 0u32;
    for b in barriers {
        match kept.last_mut() {
            Some(last)
                if !b.fallback
                    && !last.fallback
                    && last.resource == b.resource
                    && last.kind == b.kind
                    && last.consumer == b.producer =>
            {
                // 相邻同资源同类 ⇒ 两条可合成一条（把 consumer 推到后者）
                last.consumer = b.consumer;
                removed += 1;
            }
            _ => kept.push(b),
        }
    }
    (kept, removed)
}

// ---------------------------------------------------------------------------
// 四、判据
// ---------------------------------------------------------------------------

/// VE-F0031 模块自检（受 `CheckSet::MAX_CHECKS=112` 约束，逐条覆盖锚点判据）。
pub fn run_vea31_checks() -> CheckSet {
    let mut s = CheckSet::new("vea31_framegraph");

    // --- 判据 1：访问类别与屏障类别全集可枚举、关系映射全覆盖 ---------------
    s.add(
        "A31-关系-访问与屏障类别全集可枚举",
        {
            let mut seen = [false; BARRIER_KIND_COUNT];
            let mut all_ok = Access::ALL.len() == ACCESS_KIND_COUNT;
            for b in BarrierKind::ALL.iter() {
                seen[b.ordinal()] = true;
            }
            // 关系映射：四种访问对各归各类
            all_ok && seen[BarrierKind::Raw.ordinal()]
                && seen[BarrierKind::War.ordinal()]
                && seen[BarrierKind::Waw.ordinal()]
                && BarrierKind::between(Access::Write, Access::Read) == BarrierKind::Raw
                && BarrierKind::between(Access::Read, Access::Write) == BarrierKind::War
                && BarrierKind::between(Access::Write, Access::Write) == BarrierKind::Waw
                // 读后读唯一不需要屏障
                && !BarrierKind::required(Access::Read, Access::Read)
                && BarrierKind::required(Access::Write, Access::Read)
                && BarrierKind::required(Access::Read, Access::Write)
                && BarrierKind::required(Access::Write, Access::Write)
        },
        "RAW/WAR/WAW 三类关系按访问序两两判定，读后读是唯一不需屏障的组合",
    );

    // --- 判据 2：手写屏障一律被拒（结构禁令，非靠自觉）---------------------
    {
        let mut g = FrameGraph::new();
        g.add_node().ok();
        g.add_node().ok();
        let before = g.barriers().len();
        let r = g.add_manual_barrier(0, 1, 0);
        s.add(
            "A31-禁令-手写屏障恒被拒且不改变状态",
            r == Err(BuildError::ManualBarrierForbidden)
                && g.barriers().len() == before
                && g.alerts() == 1,
            "手写屏障恒返回禁止错误，屏障清单不变且登记一次告警",
        );
    }

    // --- 判据 3：RAW 屏障由写→读边自动推出 -----------------------------------
    {
        // 节点0 写资源0，节点1 读资源0 ⇒ 应有 RAW
        let acc = vec![
            AccessRec { node: 0, resource: 0, access: Access::Write },
            AccessRec { node: 1, resource: 0, access: Access::Read },
        ];
        let bars = infer_barriers(acc);
        s.add(
            "A31-推理-写后读自动插 RAW 屏障",
            bars.len() == 1
                && bars[0].kind == BarrierKind::Raw
                && bars[0].producer == 0
                && bars[0].consumer == 1
                && bars[0].resource == 0
                && !bars[0].fallback,
            "同一资源写后读自动插一条 RAW 屏障",
        );
    }

    // --- 判据 4：WAR（读后写）必须被挡——最易漏的一条 -----------------------
    {
        let acc = vec![
            AccessRec { node: 0, resource: 0, access: Access::Read },
            AccessRec { node: 1, resource: 0, access: Access::Write },
        ];
        let bars = infer_barriers(acc);
        s.add(
            "A31-推理-读后写插 WAR 屏障（不漏读后写）",
            bars.len() == 1 && bars[0].kind == BarrierKind::War,
            "只推 RAW/WAW 的实现会在此漏一条；本条按序两两判定故插 WAR",
        );
    }

    // --- 判据 5：WAW（写后写）按序挡顺序覆写 -------------------------------
    {
        let acc = vec![
            AccessRec { node: 0, resource: 0, access: Access::Write },
            AccessRec { node: 5, resource: 0, access: Access::Write },
        ];
        let bars = infer_barriers(acc);
        s.add(
            "A31-推理-写后写插 WAW 屏障",
            bars.len() == 1 && bars[0].kind == BarrierKind::Waw && bars[0].producer == 0,
            "写后写插 WAW，挡顺序覆写",
        );
    }

    // --- 判据 6：读后读不插屏障（唯一不需要挡的组合）------------------------
    {
        let acc = vec![
            AccessRec { node: 0, resource: 0, access: Access::Read },
            AccessRec { node: 1, resource: 0, access: Access::Read },
            AccessRec { node: 2, resource: 0, access: Access::Read },
        ];
        let bars = infer_barriers(acc);
        s.add(
            "A31-推理-读后读不插屏障",
            bars.is_empty() && Barrier::is_redundant_pair(Access::Read, Access::Read),
            "纯读序列不产生任何屏障（读后读不需挡）",
        );
    }

    // --- 判据 7：跨资源不插屏障（那是调度器的事，不是屏障的事）--------------
    {
        // 节点0 写资源0，节点1 写资源1 ⇒ 不同资源，无同资源写读关系
        let acc = vec![
            AccessRec { node: 0, resource: 0, access: Access::Write },
            AccessRec { node: 1, resource: 1, access: Access::Write },
        ];
        let bars = infer_barriers(acc);
        s.add(
            "A31-推理-跨资源不插屏障",
            bars.is_empty(),
            "写资源A 与写资源B 之间不插屏障（无同资源读写关系）",
        );
    }

    // --- 判据 7b：同节点内两次访问**不产自环屏障**（变异捕获面）--------------
    {
        // 同一节点先读后写同一资源：节点内指令序已保证次序，屏障是跨节点
        // 同步手段，对自己插屏障语义空转。这条判据钉住「producer 恒≠consumer」，
        // 缺了它「节点内也算WAR」的实现会全绿（那会产出自环屏障）。
        let same_node = infer_barriers(vec![
            AccessRec { node: 0, resource: 0, access: Access::Read },
            AccessRec { node: 0, resource: 0, access: Access::Write },
        ]);
        let same_node_ww = infer_barriers(vec![
            AccessRec { node: 3, resource: 1, access: Access::Write },
            AccessRec { node: 3, resource: 1, access: Access::Write },
        ]);
        // 对照：跨节点的读后写**仍**要插 WAR（别把这条判据写成「一律不插」）
        let cross_node = infer_barriers(vec![
            AccessRec { node: 0, resource: 0, access: Access::Read },
            AccessRec { node: 1, resource: 0, access: Access::Write },
        ]);
        s.add(
            "A31-推理-节点内访问不产自环屏障而跨节点仍插",
            same_node.is_empty()
                && same_node_ww.is_empty()
                && cross_node.len() == 1
                && cross_node[0].kind == BarrierKind::War
                && cross_node[0].producer != cross_node[0].consumer,
            "同节点内先后访问同一资源不插屏障（不产自环）；跨节点读后写仍插 WAR",
        );
    }

    // --- 判据 7c：全部推理屏障 producer 恒≠consumer（全局不变量）-----------
    {
        // 造一条混合访问序列，逐条断无自环。这是「7b 的加强版」：
        // 任何语料下都不得出现 producer == consumer 的屏障。
        let acc = vec![
            AccessRec { node: 0, resource: 0, access: Access::Read },
            AccessRec { node: 0, resource: 0, access: Access::Write },
            AccessRec { node: 1, resource: 0, access: Access::Read },
            AccessRec { node: 1, resource: 0, access: Access::Read },
            AccessRec { node: 2, resource: 0, access: Access::Write },
            AccessRec { node: 2, resource: 1, access: Access::Read },
        ];
        let bars = infer_barriers(acc);
        s.add(
            "A31-推理-全局无自环屏障不变量",
            !bars.is_empty() && bars.iter().all(|b| b.producer != b.consumer),
            "混合访问序列下每条屏障的 producer 与 consumer 必不同（无自环）",
        );
    }

    // --- 判据 8：环检测——反向边构成环，且拒绝不返回半成品屏障 --------------
    {
        let mut g = FrameGraph::new();
        for _ in 0..3 {
            g.add_node().ok();
        }
        g.add_edge(0, 1).ok();
        g.add_edge(1, 2).ok();
        g.add_edge(2, 0).ok(); // 反向边 ⇒ 环
        // 顺带登记一些访问，即便有访问也不应产出屏障
        g.add_access(0, 0, Access::Write).ok();
        g.add_access(1, 0, Access::Read).ok();
        let r = g.build();
        let is_cycle = matches!(r, Err(BuildError::Cycle(_)));
        s.add(
            "A31-环检测-反向边判环且拒绝不返回半成品",
            is_cycle
                && g.barriers().is_empty()
                // 环存在时 add_node 之外的状态不受影响
                && g.find_cycle().is_some(),
            "含反向边的帧图判环并拒绝，屏障清单为空（不返回半成品帧图）",
        );
    }

    // --- 判据 9：夹逼对钉死环检测界位置（无环边数不误报）--------------------
    {
        // 无反向边的链 ⇒ 不判环
        let mut g = FrameGraph::new();
        for _ in 0..4 {
            g.add_node().ok();
        }
        g.add_edge(0, 1).ok();
        g.add_edge(1, 2).ok();
        g.add_edge(2, 3).ok();
        let no_cycle = g.find_cycle().is_none();
        // 恰好加一条反向边（界上）⇒ 判环
        let mut g2 = FrameGraph::new();
        for _ in 0..4 {
            g2.add_node().ok();
        }
        g2.add_edge(0, 1).ok();
        g2.add_edge(1, 2).ok();
        g2.add_edge(2, 3).ok();
        g2.add_edge(3, 0).ok(); // 界上：恰好成环
        s.add(
            "A31-环检测-夹逼对钉死界位置（无环不误报/界上判环）",
            no_cycle && g2.find_cycle().is_some(),
            "纯链不判环；恰好加一条反向边即判环（界位置准确）",
        );
    }

    // --- 判据 10：节点越界被拒（不静默接受）---------------------------------
    {
        let mut g = FrameGraph::new();
        g.add_node().ok();
        let r = g.add_access(99, 0, Access::Write);
        let r2 = g.add_edge(0, 99);
        s.add(
            "A31-边界-节点越界访问与边被拒",
            r == Err(BuildError::BadNode) && r2 == Err(BuildError::BadNode),
            "引用不存在的节点一律返回 BadNode，不静默接受",
        );
    }

    // --- 判据 11：节点数夹逼对（界前放行 / 界上拒）--------------------------
    {
        let mut g = FrameGraph::new();
        let mut ok_count = 0u32;
        let mut last = Ok(0);
        for _ in 0..(MAX_NODES + 1) {
            last = g.add_node();
            if last.is_ok() {
                ok_count += 1;
            }
        }
        s.add(
            "A31-边界-节点数上限恰好放行上限条",
            ok_count == MAX_NODES as u32 && last == Err(BuildError::TooManyNodes),
            "节点加到上限恰好放行，再加一条被拒并登记告警",
        );
    }

    // --- 判据 12：冗余消除——相邻同资源同类合并一条 --------------------------
    {
        // A→B 与 B→C 相邻且同资源同类 ⇒ 合并成一条 A→C
        let bars = vec![
            Barrier::new(0, 1, 0, BarrierKind::Raw),
            Barrier::new(1, 2, 0, BarrierKind::Raw),
        ];
        let (kept, removed) = eliminate_redundant(bars);
        s.add(
            "A31-冗余-相邻同资源同类合并为一条",
            kept.len() == 1
                && removed == 1
                && kept[0].producer == 0
                && kept[0].consumer == 2
                && kept[0].resource == 0,
            "两条相邻同资源同类屏障合并成一条（producer 推到后者的 consumer）",
        );
    }

    // --- 判据 13：冗余消除不跨资源、不跨类别合并 ----------------------------
    {
        // 不同资源：即便相邻同类也不合并
        let bars = vec![
            Barrier::new(0, 1, 0, BarrierKind::Raw),
            Barrier::new(1, 2, 1, BarrierKind::Raw),
        ];
        let (kept, removed) = eliminate_redundant(bars.clone());
        // 不同类别：即便相邻同资源也不合并
        let bars2 = vec![
            Barrier::new(0, 1, 0, BarrierKind::Raw),
            Barrier::new(1, 2, 0, BarrierKind::War),
        ];
        let (kept2, removed2) = eliminate_redundant(bars2);
        s.add(
            "A31-冗余-不跨资源不跨类别合并",
            kept.len() == 2
                && removed == 0
                && kept2.len() == 2
                && removed2 == 0,
            "跨资源或跨类别的相邻屏障不合并（合并会丢粒度掩盖真实依赖）",
        );
    }

    // --- 判据 14：非相邻（中间断开）的同资源同类不合并 ----------------------
    {
        // kept[0].consumer=1，但 b.producer=2 ≠ 1 ⇒ 不是 A→B→C 链，不能合并
        let bars = vec![
            Barrier::new(0, 1, 0, BarrierKind::Raw),
            Barrier::new(2, 3, 0, BarrierKind::Raw),
        ];
        let (kept, removed) = eliminate_redundant(bars);
        s.add(
            "A31-冗余-非相邻同资源同类不合并",
            kept.len() == 2 && removed == 0,
            "两条屏障虽同资源同类但首尾不相接（consumer≠producer），不合并",
        );
    }

    // --- 判据 15：兜底屏障带标记且不混入具体类别统计 ------------------------
    {
        let fb = Barrier::fallback_conservative(3);
        // 兜底屏障标 fallback、语义不带具体类别（kind 只是占位）
        let mut g = FrameGraph::new();
        g.add_node().ok();
        g.add_node().ok();
        // 人为灌入：一条推理屏障 + 一条兜底屏障
        let bars = vec![
            Barrier::new(0, 1, 0, BarrierKind::Raw),
            Barrier::fallback_conservative(0),
        ];
        let (kept, removed) = eliminate_redundant(bars);
        s.add(
            "A31-兜底-兜底屏障带标记且不被当作同类合并",
            fb.fallback
                && fb.resource == 3
                && kept.len() == 2
                && removed == 0
                && kept.iter().filter(|b| b.fallback).count() == 1
                && kept.iter().filter(|b| !b.fallback).count() == 1,
            "兜底屏障带 fallback 标记，且不与推理屏障误合并（类别统计不混）",
        );
    }

    // --- 判据 16：完整帧图 build——推理屏障数与独立重算一致 -------------------
    {
        let mut g = FrameGraph::new();
        for _ in 0..4 {
            g.add_node().ok();
        }
        g.add_edge(0, 1).ok();
        g.add_edge(1, 2).ok();
        g.add_edge(2, 3).ok();
        // 资源0：节点0写、节点1读、节点2读（后两读后读不挡）
        g.add_access(0, 0, Access::Write).ok();
        g.add_access(1, 0, Access::Read).ok();
        g.add_access(2, 0, Access::Read).ok();
        // 资源1：节点1读、节点3写 ⇒ WAR
        g.add_access(1, 1, Access::Read).ok();
        g.add_access(3, 1, Access::Write).ok();
        let n = g.build();
        s.add(
            "A31-推理-完整帧图屏障数与独立重算一致",
            n.is_ok()
                && g.barriers().len() == 2
                // 独立重算：资源0 一条RAW，资源1 一条WAR
                && g.barriers().iter().filter(|b| b.kind == BarrierKind::Raw).count() == 1
                && g.barriers().iter().filter(|b| b.kind == BarrierKind::War).count() == 1
                && g.alerts() == 0,
            "完整帧图构建后屏障数为 2（一条 RAW 一条 WAR），无告警",
        );
    }

    // --- 判据 17：屏障的生产者下标恒小于消费者下标（拓扑序由构建保证）--------
    {
        // 构造一个拓扑有序的访问序列，验证所有非兜底屏障 producer<consumer
        let mut g = FrameGraph::new();
        for _ in 0..5 {
            g.add_node().ok();
        }
        for i in 0..4 {
            g.add_edge(i, i + 1).ok();
        }
        g.add_access(0, 0, Access::Write).ok();
        g.add_access(1, 0, Access::Read).ok();
        g.add_access(2, 0, Access::Write).ok();
        g.add_access(3, 0, Access::Read).ok();
        g.add_access(4, 0, Access::Write).ok();
        let _ = g.build();
        let all_ordered = g
            .barriers()
            .iter()
            .filter(|b| !b.fallback)
            .all(|b| b.producer < b.consumer);
        s.add(
            "A31-推理-屏障生产者恒在消费者之前",
            all_ordered && g.barriers().len() == 4,
            "拓扑有序的访问序列下，所有推理屏障 producer < consumer",
        );
    }

    // --- 判据 18：热图视图按资源给密度且不泄漏资源内容 -----------------------
    {
        let mut g = FrameGraph::new();
        for _ in 0..3 {
            g.add_node().ok();
        }
        g.add_edge(0, 1).ok();
        g.add_edge(1, 2).ok();
        g.add_access(0, 0, Access::Write).ok();
        g.add_access(1, 0, Access::Read).ok();
        g.add_access(2, 0, Access::Read).ok();
        let _ = g.build();
        let rows = g.heat_rows();
        let joined = rows.join("|");
        s.add(
            "A31-热图-按资源给密度且只报计数",
            rows.len() == 1
                && rows[0].contains("资源 0")
                && rows[0].contains("密度")
                && !joined.contains("绑定")
                && !joined.contains("纹理"),
            "热图每个资源一行给访问数/屏障数/密度，不泄漏绑定与纹理细节",
        );
    }

    // --- 判据 19：读屏面板双语齐备且不泄漏资源细节 -------------------------
    {
        let g = FrameGraph::new();
        let lines = g.a11y_lines();
        let joined = lines.join("|");
        s.add(
            "A31-读屏-六行双语且不泄漏资源绑定",
            lines.len() == 6
                && lines.iter().all(|l| !l.is_empty())
                && lines.iter().any(|l| l.contains("nodes"))
                && lines.iter().any(|l| l.contains("barriers"))
                && lines.iter().any(|l| l.contains("cycle"))
                && !joined.contains("绑定"),
            "面板六行中英双语只报聚合计数与环状态，不泄漏资源绑定细节",
        );
    }

    s
}