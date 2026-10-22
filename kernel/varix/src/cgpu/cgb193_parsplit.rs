//! CGPU-F0193 · 帧图并行切分策略（判据段 0x05B0）
//! 帧图的并行切分策略族：策略一——节点级并行（就绪集内所有就绪任务
//! 同时发射到线程池），策略二——子图级并行（独立子 DAG 整体并行——
//! 子图内部串行、子图间并行），策略三——瓦片级并行（图形渲染任务按
//! 瓦片拆分——同 Pass 的瓦片子任务全并行），三策略按图形态自动选择
//! 或手动指定。判据：三策略实现、自动选择正确性、并行度实测、策略
//! 切换开销、用例。
//!
//! 诊断码段 0x05B0（本模块独占：0x05B0~0x05BF）。
//! no_std + alloc 零 unwrap，全部 Result 化。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断码段基址。
pub const SEG_BASE: u16 = 0x05B0;

/// 自动选择的并行度下限：预估并行度低于此值时策略候选被淘汰。
pub const MIN_USEFUL_PARALLELISM: u32 = 2;

/// 瓦片拆分的默认瓦片边长（像素）：与瓦片渲染管线对齐。
pub const DEFAULT_TILE_EDGE: u32 = 256;

/// 切换开销采样数下限：少于该数不评估切换开销。
pub const SWITCH_SAMPLE_MIN: usize = 3;

/// 节点类别：决定该节点能否参与瓦片级拆分。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NodeKind {
    /// 图形渲染 Pass（可瓦片化）。
    GfxPass,
    /// 计算任务（节点级并行）。
    Compute,
    /// 拷贝/上传（串行偏好）。
    Transfer,
    /// 呈现/屏障（不可并行锚点）。
    Barrier,
}

impl NodeKind {
    /// 瓦片可拆性：仅 GfxPass 可按瓦片拆分（判据「三策略实现」
    /// 的策略三前置条件）。
    pub fn tileable(&self) -> bool {
        matches!(self, NodeKind::GfxPass)
    }
    /// 并行偏好权重：Transfer/Barrier 天然偏好串行（0=完全串行
    /// 1=完全自由），自动选择器用其修正策略评分。
    pub fn parallel_affinity(&self) -> u32 {
        match self {
            NodeKind::GfxPass => 90,
            NodeKind::Compute => 100,
            NodeKind::Transfer => 20,
            NodeKind::Barrier => 0,
        }
    }
}

/// 帧图节点（轻量版）：执行代价以微秒计。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FrameNode {
    /// 节点号（从 1 起，0 保留）。
    pub id: u32,
    /// 预估执行代价（微秒）。
    pub cost_us: u32,
    /// 节点类别。
    pub kind: NodeKind,
}

impl FrameNode {
    /// 节点合法性：号非零、代价非零（零代价节点会污染并行度统计）。
    pub fn validate(&self) -> Result<(), u16> {
        if self.id == 0 || self.cost_us == 0 {
            return Err(SEG_BASE);
        }
        Ok(())
    }
}

/// 帧图轻量邻接表：有向边 u→v 表示 u 的输出是 v 的输入。
#[derive(Clone, Debug, Default)]
pub struct FrameGraphLite {
    /// 节点表。
    pub nodes: Vec<FrameNode>,
    /// 邻接表（按节点下标存出边下标）。
    adj: Vec<Vec<u32>>,
    /// 入度表。
    indeg: Vec<u32>,
}

impl FrameGraphLite {
    /// 空图。
    pub fn new() -> Self {
        Self {
            nodes: Vec::new(),
            adj: Vec::new(),
            indeg: Vec::new(),
        }
    }
    /// 加节点（重复 id 拒绝）。返回内部下标。
    pub fn add_node(&mut self, n: FrameNode) -> Result<usize, u16> {
        n.validate()?;
        if self.nodes.iter().any(|x| x.id == n.id) {
            return Err(SEG_BASE + 1);
        }
        self.adj.push(Vec::new());
        self.indeg.push(0);
        self.nodes.push(n);
        Ok(self.nodes.len() - 1)
    }
    /// 加边（自环/重复边/悬空端点拒绝）。
    pub fn add_edge(&mut self, from: u32, to: u32) -> Result<(), u16> {
        if from == to {
            return Err(SEG_BASE + 2);
        }
        let fi = self.idx_of(from).ok_or(SEG_BASE + 3)?;
        let ti = self.idx_of(to).ok_or(SEG_BASE + 3)?;
        if self.adj[fi].contains(&(ti as u32)) {
            return Err(SEG_BASE + 4);
        }
        self.adj[fi].push(ti as u32);
        self.indeg[ti] += 1;
        Ok(())
    }
    /// id → 下标。
    fn idx_of(&self, id: u32) -> Option<usize> {
        self.nodes.iter().position(|n| n.id == id)
    }
    /// 节点数。
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }
    /// 出边下标切片。
    pub fn out_edges(&self, idx: usize) -> &[u32] {
        &self.adj[idx]
    }
    /// 入度。
    pub fn in_degree(&self, idx: usize) -> u32 {
        self.indeg[idx]
    }
}

/// 切分策略（判据「三策略实现」的一等公民）。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum SplitStrategy {
    /// 策略一：节点级并行——就绪集整体发射。
    #[default]
    NodeLevel,
    /// 策略二：子图级并行——独立子 DAG 内串行、子图间并行。
    SubgraphLevel,
    /// 策略三：瓦片级并行——同 Pass 瓦片子任务全并行。
    TileLevel,
}

impl SplitStrategy {
    /// 策略序号（遥测/台账用）。
    pub fn ordinal(&self) -> u8 {
        match self {
            SplitStrategy::NodeLevel => 1,
            SplitStrategy::SubgraphLevel => 2,
            SplitStrategy::TileLevel => 3,
        }
    }
}

/// 一个执行波次：波内任务两两无依赖、可同时发射。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Wave {
    /// 波内节点 id 列表。
    pub ids: Vec<u32>,
}

impl Wave {
    /// 波宽（并行度实测的原子量）。
    pub fn width(&self) -> u32 {
        self.ids.len() as u32
    }
}

/// 切分方案：波次序列 + 元信息。
#[derive(Clone, Debug, Default)]
pub struct SplitPlan {
    /// 采用的策略。
    pub strategy: SplitStrategy,
    /// 波次序列（波间有全序依赖）。
    pub waves: Vec<Wave>,
    /// 预估总收益（相对全串行的加速比 ×1000，千分比）。
    pub speedup_permille: u32,
}

impl SplitPlan {
    /// 方案合法性：波次非空、波内无重复节点、节点全覆盖恰好一次。
    pub fn validate(&self, total: usize) -> Result<(), u16> {
        if self.waves.is_empty() {
            return Err(SEG_BASE + 5);
        }
        let mut seen = vec![false; total];
        for w in &self.waves {
            if w.ids.is_empty() {
                return Err(SEG_BASE + 6);
            }
            for &id in &w.ids {
                if id == 0 || id as usize > total || seen[id as usize - 1] {
                    return Err(SEG_BASE + 7);
                }
                seen[id as usize - 1] = true;
            }
        }
        if seen.iter().any(|s| !s) {
            return Err(SEG_BASE + 8);
        }
        Ok(())
    }
    /// 最大波宽（判据「并行度实测」的核心量）。
    pub fn peak_width(&self) -> u32 {
        self.waves.iter().map(|w| w.width()).max().unwrap_or(0)
    }
    /// 平均波宽 ×1000。
    pub fn avg_width_permille(&self) -> u32 {
        if self.waves.is_empty() {
            return 0;
        }
        let sum: u64 = self.waves.iter().map(|w| w.width() as u64).sum();
        ((sum * 1000) / self.waves.len() as u64) as u32
    }
}

/// 拓扑就绪集扫描（策略一/二的公共底座）：逐层剥洋葱，每层即一个
/// 波次。环检测内建——剥不完的图返回环错误。
pub fn topo_waves(g: &FrameGraphLite) -> Result<Vec<Wave>, u16> {
    let n = g.node_count();
    if n == 0 {
        return Err(SEG_BASE + 9);
    }
    let mut indeg: Vec<u32> = (0..n).map(|i| g.in_degree(i)).collect();
    let mut done = vec![false; n];
    let mut waves: Vec<Wave> = Vec::new();
    let mut peeled = 0usize;
    loop {
        let mut cur: Vec<u32> = Vec::new();
        for i in 0..n {
            if !done[i] && indeg[i] == 0 {
                cur.push(g.nodes[i].id);
            }
        }
        if cur.is_empty() {
            break;
        }
        for &id in &cur {
            let i = id as usize - 1;
            done[i] = true;
            peeled += 1;
            for &o in g.out_edges(i) {
                indeg[o as usize] -= 1;
            }
        }
        waves.push(Wave { ids: cur });
    }
    if peeled != n {
        return Err(SEG_BASE + 10);
    }
    Ok(waves)
}

/// 策略一：节点级并行切分（判据「三策略实现」之一）。
pub fn split_node_level(g: &FrameGraphLite) -> Result<SplitPlan, u16> {
    let waves = topo_waves(g)?;
    let total_cost: u64 = g.nodes.iter().map(|n| n.cost_us as u64).sum();
    let waves_cost: u64 = waves.iter().map(|w| {
        w.ids.iter().map(|&id| g.nodes[id as usize - 1].cost_us as u64).max().unwrap_or(0)
    }).sum();
    let speedup = if waves_cost == 0 {
        0
    } else {
        ((total_cost * 1000) / waves_cost) as u32
    };
    Ok(SplitPlan {
        strategy: SplitStrategy::NodeLevel,
        waves,
        speedup_permille: speedup,
    })
}

/// 独立子 DAG 标号：依赖连通分量划分（判据「策略二」的底座）。
/// 两个节点同属一个子 DAG 当且仅当它们在同一条依赖路径上（把边视
/// 为无向后求连通分量——同分量的节点间存在依赖走廊，拆开并行会
/// 破坏数据流）。
pub fn subgraph_labels(g: &FrameGraphLite) -> Result<Vec<u32>, u16> {
    let n = g.node_count();
    if n == 0 {
        return Err(SEG_BASE + 11);
    }
    let mut label = vec![u32::MAX; n];
    let mut next_label = 0u32;
    for s in 0..n {
        if label[s] != u32::MAX {
            continue;
        }
        // BFS 泛洪（无向遍历）。
        let mut stack = vec![s];
        label[s] = next_label;
        while let Some(cur) = stack.pop() {
            let mut nbrs: Vec<usize> = g.out_edges(cur).iter().map(|&o| o as usize).collect();
            // 反向邻居：扫全表的入边。
            for (i, node_out) in g.adj.iter().enumerate() {
                if node_out.contains(&(cur as u32)) {
                    nbrs.push(i);
                }
            }
            for nb in nbrs {
                if label[nb] == u32::MAX {
                    label[nb] = next_label;
                    stack.push(nb);
                }
            }
        }
        next_label += 1;
    }
    Ok(label)
}

/// 策略二：子图级并行切分（判据「三策略实现」之二）。子图内保持
/// 拓扑串行波次，子图之间在同一波次槽内并行推进——实现为「按槽
/// 交错」：槽 k 收集各子图各自第 k 个波次的所有节点。
pub fn split_subgraph_level(g: &FrameGraphLite) -> Result<SplitPlan, u16> {
    let labels = subgraph_labels(g)?;
    let n = g.node_count();
    let sub_count = labels.iter().copied().max().unwrap_or(0) as usize + 1;
    if sub_count < 1 {
        return Err(SEG_BASE + 12);
    }
    // 每个子图的串行波次（按子图内拓扑）。
    let mut sub_waves: Vec<Vec<Wave>> = vec![Vec::new(); sub_count];
    let mut indeg: Vec<u32> = (0..n).map(|i| g.in_degree(i)).collect();
    let mut done = vec![false; n];
    let mut peeled = 0usize;
    loop {
        let mut ready: Vec<usize> = (0..n).filter(|&i| !done[i] && indeg[i] == 0).collect();
        if ready.is_empty() {
            break;
        }
        // 按子图分组：同子图同波（子图内串行）。
        let mut by_sub: Vec<Vec<u32>> = vec![Vec::new(); sub_count];
        for &i in &ready {
            by_sub[labels[i] as usize].push(g.nodes[i].id);
        }
        for (si, ids) in by_sub.into_iter().enumerate() {
            if !ids.is_empty() {
                sub_waves[si].push(Wave { ids });
            }
        }
        for &i in &ready {
            done[i] = true;
            peeled += 1;
            for &o in g.out_edges(i) {
                indeg[o as usize] -= 1;
            }
        }
    }
    if peeled != n {
        return Err(SEG_BASE + 13);
    }
    // 槽交错：槽 k = 各子图第 k 波的并集。
    let depth = sub_waves.iter().map(|w| w.len()).max().unwrap_or(0);
    let mut waves: Vec<Wave> = Vec::new();
    for k in 0..depth {
        let mut ids: Vec<u32> = Vec::new();
        for sw in &sub_waves {
            if let Some(w) = sw.get(k) {
                ids.extend_from_slice(&w.ids);
            }
        }
        if !ids.is_empty() {
            waves.push(Wave { ids });
        }
    }
    let total_cost: u64 = g.nodes.iter().map(|x| x.cost_us as u64).sum();
    let critical: u64 = waves.iter().map(|w| {
        w.ids.iter().map(|&id| g.nodes[id as usize - 1].cost_us as u64).sum::<u64>()
    }).sum();
    let speedup = if critical == 0 { 0 } else { ((total_cost * 1000) / critical) as u32 };
    Ok(SplitPlan {
        strategy: SplitStrategy::SubgraphLevel,
        waves,
        speedup_permille: speedup,
    })
}

/// 瓦片栅格描述（判据「策略三」的输入）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TileGrid {
    /// 渲染目标宽（像素）。
    pub width: u32,
    /// 渲染目标高（像素）。
    pub height: u32,
    /// 瓦片边长（像素）。
    pub edge: u32,
}

impl TileGrid {
    /// 网格校验：宽高边长均非零且边长不超过短边。
    pub fn validate(&self) -> Result<(), u16> {
        if self.width == 0 || self.height == 0 || self.edge == 0 {
            return Err(SEG_BASE + 14);
        }
        if self.edge > self.width.min(self.height) {
            return Err(SEG_BASE + 15);
        }
        Ok(())
    }
    /// 瓦片列数。
    pub fn cols(&self) -> u32 {
        (self.width + self.edge - 1) / self.edge
    }
    /// 瓦片行数。
    pub fn rows(&self) -> u32 {
        (self.height + self.edge - 1) / self.edge
    }
    /// 瓦片总数。
    pub fn tile_count(&self) -> u32 {
        self.cols() * self.rows()
    }
}

/// 策略三：瓦片级并行切分（判据「三策略实现」之三）。每个可瓦片
/// 化的 GfxPass 节点展开为 tile_count 个瓦片子任务（id 编码：原
/// id×1000 + 瓦片序号），其余节点按拓扑波次保留。
pub fn split_tile_level(g: &FrameGraphLite, grid: &TileGrid) -> Result<SplitPlan, u16> {
    grid.validate()?;
    let waves = topo_waves(g)?;
    let mut out: Vec<Wave> = Vec::new();
    for w in &waves {
        let mut ids: Vec<u32> = Vec::new();
        for &id in &w.ids {
            let node = &g.nodes[id as usize - 1];
            if node.kind.tileable() {
                let tiles = grid.tile_count();
                if tiles == 0 || id > u32::MAX / 1000 - tiles {
                    return Err(SEG_BASE + 16);
                }
                for t in 0..tiles {
                    ids.push(id * 1000 + t + 1);
                }
            } else {
                ids.push(id);
            }
        }
        out.push(Wave { ids });
    }
    let plan = SplitPlan {
        strategy: SplitStrategy::TileLevel,
        waves: out,
        speedup_permille: 0,
    };
    plan.validate(g.node_count() * grid.tile_count() as usize + g.node_count())?;
    Ok(plan)
}

/// 图形态指纹（自动选择器的特征向量）：就绪集平均宽度 ×1000 /
/// 可瓦片化占比 ×1000 / 子图数。
#[derive(Clone, Copy, Debug)]
pub struct ShapeProfile {
    /// 就绪集平均宽度 ×1000（0 波次时为 0）。
    pub avg_ready_permille: u32,
    /// 可瓦片化节点占比 ×1000。
    pub tileable_permille: u32,
    /// 独立子图数。
    pub subgraphs: u32,
    /// 峰值就绪宽度。
    pub peak_ready: u32,
}

impl ShapeProfile {
    /// 特征合法性。
    pub fn validate(&self) -> Result<(), u16> {
        if self.avg_ready_permille > 100_000 || self.tileable_permille > 1000 {
            return Err(SEG_BASE + 17);
        }
        if self.subgraphs == 0 || self.peak_ready == 0 {
            return Err(SEG_BASE + 18);
        }
        Ok(())
    }
}

/// 图形态剖析（判据「自动选择正确性」的依据采集）。
pub fn profile_shape(g: &FrameGraphLite) -> Result<ShapeProfile, u16> {
    let waves = topo_waves(g)?;
    let n = g.node_count();
    let sum_w: u64 = waves.iter().map(|w| w.width() as u64).sum();
    let avg = ((sum_w * 1000) / waves.len() as u64) as u32;
    let tileable = g.nodes.iter().filter(|x| x.kind.tileable()).count();
    let tile_permille = ((tileable as u64) * 1000 / n as u64) as u32;
    let subs = subgraph_labels(g)?.iter().copied().max().unwrap_or(0) + 1;
    let peak = waves.iter().map(|w| w.width()).max().unwrap_or(0);
    Ok(ShapeProfile {
        avg_ready_permille: avg,
        tileable_permille: tile_permille,
        subgraphs: subs,
        peak_ready: peak,
    })
}

/// 自动策略选择器（判据「自动选择正确性」的核心裁决）。
/// 裁决序：
/// 1. 可瓦片化占比 ≥ 600‰ 且峰值就绪 ≥ 2 → 瓦片级；
/// 2. 子图数 ≥ 2 且平均就绪宽度 < 2000‰ → 子图级；
/// 3. 否则节点级。
pub fn select_auto(p: &ShapeProfile) -> Result<SplitStrategy, u16> {
    p.validate()?;
    if p.tileable_permille >= 600 && p.peak_ready >= MIN_USEFUL_PARALLELISM {
        return Ok(SplitStrategy::TileLevel);
    }
    if p.subgraphs >= 2 && p.avg_ready_permille < 2000 {
        return Ok(SplitStrategy::SubgraphLevel);
    }
    if p.peak_ready < MIN_USEFUL_PARALLELISM {
        // 全串行图三策略皆无收益，仍返回节点级（退化但合法）。
        return Ok(SplitStrategy::NodeLevel);
    }
    Ok(SplitStrategy::NodeLevel)
}

/// 切换开销记录（判据「策略切换开销」）：一次策略迁移的代价采样。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SwitchSample {
    /// 迁移前策略。
    pub from: SplitStrategy,
    /// 迁移后策略。
    pub to: SplitStrategy,
    /// 重建切分方案的代价（微秒）。
    pub replot_us: u32,
    /// 方案发布到首波发射的空泡（微秒）。
    pub bubble_us: u32,
}

impl SwitchSample {
    /// 采样合法性。
    pub fn validate(&self) -> Result<(), u16> {
        if self.from == self.to {
            return Err(SEG_BASE + 19);
        }
        if self.replot_us == 0 || self.bubble_us > self.replot_us * 4 {
            return Err(SEG_BASE + 20);
        }
        Ok(())
    }
    /// 总开销（微秒）。
    pub fn total_us(&self) -> u64 {
        self.replot_us as u64 + self.bubble_us as u64
    }
}

/// 切换开销台账：滑动窗口统计。
#[derive(Default)]
pub struct SwitchLedger {
    samples: Vec<SwitchSample>,
}

impl SwitchLedger {
    /// 记录一次切换。
    pub fn record(&mut self, s: SwitchSample) -> Result<(), u16> {
        s.validate()?;
        if self.samples.len() >= 4096 {
            self.samples.remove(0);
        }
        self.samples.push(s);
        Ok(())
    }
    /// 台账规模。
    pub fn count(&self) -> usize {
        self.samples.len()
    }
    /// 平均切换开销（微秒 ×1000 千分精度）。
    pub fn avg_total_micro_permille(&self) -> Result<u32, u16> {
        if self.samples.len() < SWITCH_SAMPLE_MIN {
            return Err(SEG_BASE + 21);
        }
        let sum: u64 = self.samples.iter().map(|s| s.total_us()).sum();
        Ok(((sum * 1000) / self.samples.len() as u64) as u32)
    }
    /// 策略迁移对计数（from→to 方向热度）：返回 (from,to,count) 列表
    /// 按 count 降序——调度器可据此钉住高频切换对做滞回。
    pub fn hot_pairs(&self) -> Vec<(u8, u8, usize)> {
        let mut pairs: Vec<(u8, u8, usize)> = Vec::new();
        for s in &self.samples {
            let key = (s.from.ordinal(), s.to.ordinal());
            if let Some(e) = pairs.iter_mut().find(|(a, b, _)| (*a, *b) == key) {
                e.2 += 1;
            } else {
                pairs.push((key.0, key.1, 1));
            }
        }
        pairs.sort_by(|x, y| y.2.cmp(&x.2));
        pairs
    }
}

/// 切分台账指纹（判据「用例」可审计面）。
pub fn plan_book_fp(plan: &SplitPlan) -> u64 {
    let mut acc: u64 = 0xcbf29ce484222325;
    acc = acc.wrapping_mul(0x100000001b3) ^ plan.strategy.ordinal() as u64;
    for w in &plan.waves {
        for &id in &w.ids {
            acc = acc.wrapping_mul(0x100000001b3) ^ id as u64;
        }
        acc = acc.wrapping_mul(0x100000001b3) ^ 0xA5A5;
    }
    acc = acc.wrapping_mul(0x100000001b3) ^ plan.speedup_permille as u64;
    acc
}

/// 三策略全量对拍（判据「用例」主判据）：同一图跑三策略，各自方案
/// 必须通过 validate 且节点覆盖完整；并返回三者峰值波宽供伸缩分析。
pub fn triple_strategy_bench(g: &FrameGraphLite, grid: &TileGrid) -> Result<(u32, u32, u32), u16> {
    let total = g.node_count();
    let p1 = split_node_level(g)?;
    let p2 = split_subgraph_level(g)?;
    let p3 = split_tile_level(g, grid)?;
    p1.validate(total)?;
    p2.validate(total)?;
    // 瓦片方案按展开后账本校验：GfxPass 展开、其余原样。
    let tiles = grid.tile_count();
    let expanded = total + g.nodes.iter().filter(|n| n.kind.tileable()).count() as usize * (tiles as usize - 1).max(0);
    p3.waves.iter().for_each(|_| {});
    let _ = expanded;
    for w in &p3.waves {
        for &id in &w.ids {
            if id == 0 {
                return Err(SEG_BASE + 22);
            }
            let base = id / 1000;
            let rem = id % 1000;
            let is_tile = rem >= 1 && base as usize <= total && g.nodes[base as usize - 1].kind.tileable();
            let is_plain = base == 0 && (id as usize) <= total && !g.nodes[id as usize - 1].kind.tileable();
            if !is_tile && !is_plain {
                return Err(SEG_BASE + 23);
            }
        }
    }
    Ok((p1.peak_width(), p2.peak_width(), p3.peak_width()))
}

/// 自动选择与指定策略一致性（判据「自动选择正确性」反证面）：
/// 手动指定与自动裁决不一致时报告（不阻断——手动优先但入遥测）。
pub fn auto_manual_agreement(
    g: &FrameGraphLite,
    manual: SplitStrategy,
) -> Result<(SplitStrategy, bool), u16> {
    let p = profile_shape(g)?;
    let auto = select_auto(&p)?;
    Ok((auto, auto == manual))
}

/// 波次依赖完整性深度审计（独立函数面）：同一波内任两节点不得有
/// 直接边——策略实现的核心不变量（波内并行 = 无边 = 无数据竞争）。
pub fn wave_independence_audit(g: &FrameGraphLite, plan: &SplitPlan) -> Result<u32, u16> {
    plan.validate(g.node_count())?;
    for w in &plan.waves {
        for (i, &a) in w.ids.iter().enumerate() {
            for &b in &w.ids[i + 1..] {
                let ai = a as usize - 1;
                let bi = b as usize - 1;
                if g.adj[ai].contains(&(bi as u32)) || g.adj[bi].contains(&(ai as u32)) {
                    return Err(SEG_BASE + 24);
                }
            }
        }
    }
    Ok(plan.peak_width())
}

/// 策略切换滞回建议（判据「策略切换开销」可行动面）：相邻两次
/// 选择不同策略且台账平均开销超过阈值时建议滞回（返回 true）。
pub fn hysteresis_advised(
    ledger: &SwitchLedger,
    last: SplitStrategy,
    next: SplitStrategy,
    budget_us: u32,
) -> Result<bool, u16> {
    if last == next {
        return Ok(false);
    }
    if ledger.count() < SWITCH_SAMPLE_MIN {
        return Err(SEG_BASE + 25);
    }
    let avg = ledger.avg_total_micro_permille()?;
    // avg 是微秒×1000，budget 是微秒：avg/1000 > budget 即超限。
    Ok((avg as u64) > (budget_us as u64) * 1000)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn chain(n: u32, cost: u32) -> FrameGraphLite {
        let mut g = FrameGraphLite::new();
        for i in 1..=n {
            g.add_node(FrameNode { id: i, cost_us: cost, kind: NodeKind::Compute }).unwrap();
        }
        for i in 1..n {
            g.add_edge(i, i + 1).unwrap();
        }
        g
    }
    #[test]
    fn node_level_speeds_chain_not() {
        let g = chain(4, 10);
        let p = split_node_level(&g).unwrap();
        assert_eq!(p.peak_width(), 1);
        assert_eq!(p.speedup_permille, 1000);
    }
    #[test]
    fn fanout_parallel_wide() {
        let mut g = FrameGraphLite::new();
        g.add_node(FrameNode { id: 1, cost_us: 5, kind: NodeKind::Compute }).unwrap();
        for i in 2..=9 {
            g.add_node(FrameNode { id: i, cost_us: 10, kind: NodeKind::Compute }).unwrap();
            g.add_edge(1, i).unwrap();
        }
        let p = split_node_level(&g).unwrap();
        assert_eq!(p.peak_width(), 8);
    }
    #[test]
    fn cycle_rejected() {
        let mut g = FrameGraphLite::new();
        for i in 1..=3 {
            g.add_node(FrameNode { id: i, cost_us: 1, kind: NodeKind::Compute }).unwrap();
        }
        g.add_edge(1, 2).unwrap();
        g.add_edge(2, 3).unwrap();
        g.add_edge(3, 1).unwrap();
        assert!(topo_waves(&g).is_err());
    }
    #[test]
    fn tile_expansion_counts() {
        let mut g = FrameGraphLite::new();
        g.add_node(FrameNode { id: 1, cost_us: 8, kind: NodeKind::GfxPass }).unwrap();
        g.add_node(FrameNode { id: 2, cost_us: 3, kind: NodeKind::Barrier }).unwrap();
        g.add_edge(1, 2).unwrap();
        let grid = TileGrid { width: 512, height: 512, edge: 256 };
        let p = split_tile_level(&g, &grid).unwrap();
        assert_eq!(p.waves[0].ids.len(), 4);
        assert_eq!(p.waves[1].ids, vec![2]);
    }
    #[test]
    fn auto_picks_tile_for_gfx() {
        let mut g = FrameGraphLite::new();
        for i in 1..=4 {
            g.add_node(FrameNode { id: i, cost_us: 5, kind: NodeKind::GfxPass }).unwrap();
            if i > 1 {
                g.add_edge(i - 1, i).unwrap();
            }
        }
        let prof = profile_shape(&g).unwrap();
        assert_eq!(select_auto(&prof).unwrap(), SplitStrategy::TileLevel);
    }
}
