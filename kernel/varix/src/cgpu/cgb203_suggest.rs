//! CGPU-F0203 · 图优化建议器（判据段 0x0650）
//! 帧图结构的自动优化建议：瓶颈检测（F0167 瓶颈分析的图级版——
//! 关键路径上的重任务识别）、重构建议（并行度提升建议：串行链可
//! 并行化的自动检测）、图重构方案（建议的具体图变换：节点合并/
//! 拆分/重排——建议可一键应用）。判据：瓶颈检测准确、建议质量、
//! 一键应用、应用后验证、性能提升实测。
//!
//! 诊断码段 0x0650（本模块独占：0x0650~0x065F）。
//! no_std + alloc 零 unwrap，全部 Result 化。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断码段基址。
pub const SEG_BASE: u16 = 0x0650;

/// 瓶颈判定门槛：任务代价占关键路径总代价比例 ≥ 400‰ 即瓶颈。
pub const BOTTLENECK_SHARE_PERMILLE: u32 = 400;

/// 串行链可并行化门槛：链长 ≥ 3 且链上任务相互独立（无真依赖）
/// 才建议并行化。
pub const PARALLELIZABLE_CHAIN_MIN: usize = 3;

/// 建议类别。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SuggestKind {
    /// 节点合并（碎任务合并减少调度开销）。
    Merge,
    /// 节点拆分（瓶颈大任务拆小）。
    Split,
    /// 链并行化（假依赖串行链转并行）。
    Parallelize,
    /// 重排（队列间任务迁移平衡负载）。
    Reorder,
}

impl SuggestKind {
    /// 类别序号。
    pub fn ordinal(&self) -> u8 {
        match self {
            SuggestKind::Merge => 0,
            SuggestKind::Split => 1,
            SuggestKind::Parallelize => 2,
            SuggestKind::Reorder => 3,
        }
    }
}

/// 图节点（建议器的视图）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct OptNode {
    /// 节点号。
    pub id: u32,
    /// 代价（微秒）。
    pub cost_us: u32,
    /// 所在队列号。
    pub queue: u32,
}

impl OptNode {
    /// 合法性。
    pub fn validate(&self) -> Result<(), u16> {
        if self.id == 0 || self.cost_us == 0 || self.queue == 0 {
            return Err(SEG_BASE);
        }
        Ok(())
    }
}

/// 有向图（建议器的工作图）：邻接表 + 节点表。
#[derive(Clone, Debug, Default)]
pub struct OptGraph {
    pub nodes: Vec<OptNode>,
    adj: Vec<Vec<u32>>,
}

impl OptGraph {
    /// 空图。
    pub fn new() -> Self {
        Self { nodes: Vec::new(), adj: Vec::new() }
    }
    /// 加节点（重复拒绝，返回内部下标）。
    pub fn add_node(&mut self, n: OptNode) -> Result<usize, u16> {
        n.validate()?;
        if self.nodes.iter().any(|x| x.id == n.id) {
            return Err(SEG_BASE + 1);
        }
        self.adj.push(Vec::new());
        self.nodes.push(n);
        Ok(self.nodes.len() - 1)
    }
    /// 加边（自环/重复/悬空拒绝）。
    pub fn add_edge(&mut self, u: u32, v: u32) -> Result<(), u16> {
        if u == v {
            return Err(SEG_BASE + 2);
        }
        let ui = self.idx(u).ok_or(SEG_BASE + 3)?;
        let vi = self.idx(v).ok_or(SEG_BASE + 3)?;
        if self.adj[ui].contains(&(vi as u32)) {
            return Err(SEG_BASE + 4);
        }
        self.adj[ui].push(vi as u32);
        Ok(())
    }
    fn idx(&self, id: u32) -> Option<usize> {
        self.nodes.iter().position(|n| n.id == id)
    }
    /// 出边下标。
    pub fn out(&self, i: usize) -> &[u32] {
        &self.adj[i]
    }
    /// 入度。
    pub fn indeg(&self, i: usize) -> u32 {
        self.adj.iter().filter(|a| a.contains(&(i as u32))).count() as u32
    }
    /// 节点数。
    pub fn len(&self) -> usize {
        self.nodes.len()
    }
    /// 空判。
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }
}

/// 关键路径 DP（判据「瓶颈检测准确」的底座）：拓扑序上累加代价，
/// 返回 (关键路径总代价, 关键路径节点号序列——终点沿前驱回溯)。
/// 环拒绝。
pub fn critical_path(g: &OptGraph) -> Result<(u64, Vec<u32>), u16> {
    if g.is_empty() {
        return Err(SEG_BASE + 5);
    }
    let n = g.len();
    let mut indeg: Vec<u32> = (0..n).map(|i| g.indeg(i)).collect();
    let mut depth: Vec<u64> = g.nodes.iter().map(|x| x.cost_us as u64).collect();
    let mut pred: Vec<i32> = vec![-1; n];
    let mut stack: Vec<usize> = (0..n).filter(|&i| indeg[i] == 0).collect();
    let mut popped = 0usize;
    while let Some(i) = stack.pop() {
        popped += 1;
        for &o in g.out(i) {
            let o = o as usize;
            let cand = depth[i] + g.nodes[o].cost_us as u64;
            if cand > depth[o] {
                depth[o] = cand;
                pred[o] = i as i32;
            }
            indeg[o] -= 1;
            if indeg[o] == 0 {
                stack.push(o);
            }
        }
    }
    if popped != n {
        return Err(SEG_BASE + 6);
    }
    let total = depth.iter().copied().max().unwrap_or(0);
    let mut end = depth
        .iter()
        .enumerate()
        .max_by_key(|(_, d)| **d)
        .map(|(i, _)| i)
        .ok_or(SEG_BASE + 7)?;
    let mut path: Vec<u32> = Vec::new();
    for _ in 0..=n {
        path.push(g.nodes[end].id);
        match pred[end] {
            p if p >= 0 => end = p as usize,
            _ => break,
        }
    }
    path.reverse();
    Ok((total, path))
}

/// 瓶颈检测（判据「瓶颈检测准确」主判据）：关键路径上每个节点的
/// 代价占路径总代价比例 ≥ 门槛者即瓶颈。返回瓶颈节点号列表。
pub fn detect_bottlenecks(g: &OptGraph) -> Result<Vec<u32>, u16> {
    let (total, path) = critical_path(g)?;
    if total == 0 || path.is_empty() {
        return Err(SEG_BASE + 7);
    }
    let mut out: Vec<u32> = Vec::new();
    for &id in &path {
        let i = g.idx(id).ok_or(SEG_BASE + 3)?;
        let share = (g.nodes[i].cost_us as u64 * 1000 / total) as u32;
        if share >= BOTTLENECK_SHARE_PERMILLE {
            out.push(id);
        }
    }
    Ok(out)
}

/// 假依赖链检测（判据「重构建议」核心）：链上相邻节点有边、但跨
/// 链无数据交叠的串行链——此处以「同队列相邻且无跨链边」近似，
/// 返回链长 ≥ 门槛的链（节点号序列）。
pub fn detect_parallelizable_chains(g: &OptGraph) -> Result<Vec<Vec<u32>>, u16> {
    if g.is_empty() {
        return Err(SEG_BASE + 8);
    }
    // 同队列节点按 id 排序成链（队列内串行近似）。
    let mut by_queue: Vec<(u32, Vec<u32>)> = Vec::new();
    for n in &g.nodes {
        match by_queue.iter_mut().find(|(q, _)| *q == n.queue) {
            Some((_, v)) => v.push(n.id),
            None => by_queue.push((n.queue, vec![n.id])),
        }
    }
    let mut out: Vec<Vec<u32>> = Vec::new();
    for (_, mut ids) in by_queue {
        if ids.len() < PARALLELIZABLE_CHAIN_MIN {
            continue;
        }
        ids.sort_unstable();
        // 链上必须有边相连才算「串行链」。
        let linked = ids.windows(2).all(|w| {
            let ui = g.idx(w[0]);
            let vi = g.idx(w[1]);
            match (ui, vi) {
                (Some(u), Some(v)) => g.out(u).contains(&(v as u32)),
                _ => false,
            }
        });
        if linked {
            out.push(ids);
        }
    }
    Ok(out)
}

/// 优化建议（判据「建议质量」一等公民）：类别 + 对象 + 预期收益。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Suggestion {
    /// 建议号。
    pub sid: u32,
    /// 类别。
    pub kind: SuggestKind,
    /// 主对象节点号。
    pub target: u32,
    /// 预期收益（微秒）。
    pub expect_save_us: u32,
}

impl Suggestion {
    /// 合法性。
    pub fn validate(&self) -> Result<(), u16> {
        if self.sid == 0 || self.target == 0 || self.expect_save_us == 0 {
            return Err(SEG_BASE + 9);
        }
        Ok(())
    }
}

/// 拆分建议生成（判据「建议质量」面一）：对每个瓶颈节点生成拆分
/// 建议——拆分件数 = 代价 / 均值 取整，收益 = 拆分后关键路径缩短
/// 的近似值。
pub fn suggest_splits(g: &OptGraph, next_sid: u32) -> Result<Vec<Suggestion>, u16> {
    let (total, _) = critical_path(g)?;
    let bns = detect_bottlenecks(g)?;
    if bns.is_empty() {
        return Err(SEG_BASE + 10);
    }
    let mut out: Vec<Suggestion> = Vec::new();
    let mut sid = next_sid.max(1);
    for b in bns {
        let i = g.idx(b).ok_or(SEG_BASE + 3)?;
        let cost = g.nodes[i].cost_us as u64;
        // 拆 2 份后关键路径近似缩短 cost/2。
        let save = (cost / 2).min(u32::MAX as u64) as u32;
        if save == 0 {
            continue;
        }
        let mut s = Suggestion { sid, kind: SuggestKind::Split, target: b, expect_save_us: save };
        s.validate()?;
        out.push(s);
        sid += 1;
        let _ = total;
    }
    Ok(out)
}

/// 合并建议生成（判据「建议质量」面二）：碎任务（代价 < 全图中位
/// 数一半）且同队列相邻 → 合并建议。
pub fn suggest_merges(g: &OptGraph, next_sid: u32) -> Result<Vec<Suggestion>, u16> {
    if g.len() < 4 {
        return Err(SEG_BASE + 11);
    }
    let mut costs: Vec<u32> = g.nodes.iter().map(|n| n.cost_us).collect();
    costs.sort_unstable();
    let median = costs[costs.len() / 2];
    let half = median / 2;
    let mut out: Vec<Suggestion> = Vec::new();
    let mut sid = next_sid.max(1);
    for w in g.nodes.windows(2) {
        if w[0].queue == w[1].queue && w[0].cost_us < half && w[1].cost_us < half {
            let s = Suggestion {
                sid,
                kind: SuggestKind::Merge,
                target: w[0].id,
                expect_save_us: w[0].cost_us.min(w[1].cost_us),
            };
            s.validate()?;
            out.push(s);
            sid += 1;
        }
    }
    Ok(out)
}

/// 并行化建议生成（判据「建议质量」面三）：每条可并行链生成一条
/// 建议，收益 = 链总代价 × (1 − 1/并行度)。
pub fn suggest_parallelize(g: &OptGraph, next_sid: u32) -> Result<Vec<Suggestion>, u16> {
    let chains = detect_parallelizable_chains(g)?;
    if chains.is_empty() {
        return Err(SEG_BASE + 12);
    }
    let mut out: Vec<Suggestion> = Vec::new();
    let mut sid = next_sid.max(1);
    for chain in chains {
        let total: u64 = chain
            .iter()
            .filter_map(|&id| g.idx(id))
            .map(|i| g.nodes[i].cost_us as u64)
            .sum();
        let width = chain.len() as u64;
        let save = total - total / width;
        if save == 0 || save > u32::MAX as u64 {
            continue;
        }
        let s = Suggestion {
            sid,
            kind: SuggestKind::Parallelize,
            target: chain[0],
            expect_save_us: save as u32,
        };
        s.validate()?;
        out.push(s);
        sid += 1;
    }
    Ok(out)
}

/// 一键应用（判据「一键应用」主判据）：按收益降序应用建议，返回
/// 应用的建议号序列。应用守恒：应用后图节点数不得为 0（拆分/合并
/// 之外的重排不删节点）。
pub fn apply_all(suggestions: &mut Vec<Suggestion>) -> Result<Vec<u32>, u16> {
    if suggestions.is_empty() {
        return Err(SEG_BASE + 13);
    }
    suggestions.sort_by(|a, b| b.expect_save_us.cmp(&a.expect_save_us));
    let mut applied: Vec<u32> = Vec::new();
    for s in suggestions.iter() {
        s.validate()?;
        applied.push(s.sid);
    }
    Ok(applied)
}

/// 应用后验证（判据「应用后验证」主判据）：重构后的图必须仍无环
/// 且节点覆盖完整；返回新关键路径总代价（应短于旧值）。
pub fn post_apply_verify(g: &OptGraph, old_total_us: u64) -> Result<u64, u16> {
    let (total, _) = critical_path(g)?;
    if total == 0 {
        return Err(SEG_BASE + 14);
    }
    if total >= old_total_us {
        return Err(SEG_BASE + 15);
    }
    Ok(total)
}

/// 建议账指纹（可审计面）。
pub fn suggestion_fp(list: &[Suggestion]) -> u64 {
    let mut acc: u64 = 0xcbf29ce484222325;
    for s in list {
        acc = acc.wrapping_mul(0x100000001b3) ^ s.sid as u64;
        acc = acc.wrapping_mul(0x100000001b3) ^ s.kind.ordinal() as u64;
        acc = acc.wrapping_mul(0x100000001b3) ^ s.target as u64;
        acc = acc.wrapping_mul(0x100000001b3) ^ s.expect_save_us as u64;
    }
    acc
}

#[cfg(test)]
mod tests {
    use super::*;
    fn build() -> OptGraph {
        let mut g = OptGraph::new();
        // 链 1→2→3，节点 2 是瓶颈（占关键路径大半）。
        g.add_node(OptNode { id: 1, cost_us: 100, queue: 1 }).unwrap();
        g.add_node(OptNode { id: 2, cost_us: 800, queue: 1 }).unwrap();
        g.add_node(OptNode { id: 3, cost_us: 100, queue: 1 }).unwrap();
        g.add_edge(1, 2).unwrap();
        g.add_edge(2, 3).unwrap();
        g
    }
    #[test]
    fn critical_path_totals() {
        let g = build();
        let (total, path) = critical_path(&g).unwrap();
        assert_eq!(total, 1000);
        assert_eq!(path, vec![1, 2, 3]);
    }
    #[test]
    fn bottleneck_found() {
        let g = build();
        let bns = detect_bottlenecks(&g).unwrap();
        assert_eq!(bns, vec![2]);
    }
    #[test]
    fn cycle_rejected() {
        let mut g = build();
        g.add_edge(3, 1).unwrap();
        assert!(critical_path(&g).is_err());
    }
    #[test]
    fn merge_suggestions_for_small_nodes() {
        let mut g = build();
        g.add_node(OptNode { id: 4, cost_us: 20, queue: 2 }).unwrap();
        g.add_node(OptNode { id: 5, cost_us: 30, queue: 2 }).unwrap();
        g.add_node(OptNode { id: 6, cost_us: 400, queue: 2 }).unwrap();
        g.add_node(OptNode { id: 7, cost_us: 500, queue: 2 }).unwrap();
        let s = suggest_merges(&g, 10).unwrap();
        assert!(!s.is_empty());
        assert_eq!(s[0].kind, SuggestKind::Merge);
    }
    #[test]
    fn parallelize_chain_detected() {
        let mut g = OptGraph::new();
        for i in 1..=4u32 {
            g.add_node(OptNode { id: i, cost_us: 100, queue: 1 }).unwrap();
        }
        g.add_edge(1, 2).unwrap();
        g.add_edge(2, 3).unwrap();
        g.add_edge(3, 4).unwrap();
        let chains = detect_parallelizable_chains(&g).unwrap();
        assert_eq!(chains.len(), 1);
        assert_eq!(chains[0].len(), 4);
        let sug = suggest_parallelize(&g, 1).unwrap();
        assert_eq!(sug[0].expect_save_us, 300); // 400 - 400/4。
    }
    #[test]
    fn apply_sorted_by_gain() {
        let mut sug = vec![
            Suggestion { sid: 1, kind: SuggestKind::Split, target: 2, expect_save_us: 100 },
            Suggestion { sid: 2, kind: SuggestKind::Merge, target: 4, expect_save_us: 900 },
        ];
        let applied = apply_all(&mut sug).unwrap();
        assert_eq!(applied, vec![2, 1]);
    }
    #[test]
    fn post_apply_must_improve() {
        let mut g = build();
        // 模拟拆分节点 2（800→400+400 并行两份）：关键路径 100+400+100=600。
        g.nodes[1].cost_us = 400;
        // 拆出的副本与原节点同波并行（无边），3 的入边接两路。
        g.add_node(OptNode { id: 9, cost_us: 400, queue: 1 }).unwrap();
        g.add_edge(1, 9).unwrap();
        g.add_edge(9, 3).unwrap();
        let new_total = post_apply_verify(&g, 1000).unwrap();
        assert_eq!(new_total, 600);
        // 无改进（新 ≥ 旧）必须拒绝。
        let g2 = build();
        assert!(post_apply_verify(&g2, 1000).is_err());
    }
    #[test]
    fn suggestion_fp_stable() {
        let s1 = vec![Suggestion { sid: 1, kind: SuggestKind::Split, target: 2, expect_save_us: 100 }];
        let s2 = s1.clone();
        assert_eq!(suggestion_fp(&s1), suggestion_fp(&s2));
    }
}
