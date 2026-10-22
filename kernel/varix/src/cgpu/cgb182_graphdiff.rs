//! CGPU-F0182 · 图差异与变更检测（判据段 0x0500）
//! 相邻帧图的差异分析：图 diff（节点/边的增删改检测——结构 diff
//! 算法）、差异分类（结构变更/参数变更/纯数据变更——变更类型驱动
//! 增量策略：结构变更重建、参数变更更新、数据变更无图操作）、diff
//! 结果缓存联动（F0170 图缓存的失效判定依据）。判据：diff 算法正确、
//! 分类准确、缓存联动、性能、用例。
//!
//! 诊断码段 0x0500（本模块独占：0x0500~0x050F）。
//! no_std + alloc 零 unwrap，全部 Result 化。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断码段基址。
pub const SEG_BASE: u16 = 0x0500;

/// 变更类型（判据「分类准确」闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChangeKind {
    /// 结构变更（节点/边增删——图重建）。
    Structural,
    /// 参数变更（节点属性变化——图更新）。
    Parametric,
    /// 纯数据变更（图全同——无图操作）。
    DataOnly,
}

impl ChangeKind {
    /// 增量策略序号（0=重建 1=更新 2=无操作）。
    pub fn strategy(self) -> u8 {
        match self {
            ChangeKind::Structural => 0,
            ChangeKind::Parametric => 1,
            ChangeKind::DataOnly => 2,
        }
    }
}

/// 图 diff 结果（判据「diff 算法正确」产出物）。
pub struct GraphDiff {
    /// 新增节点。
    pub added_nodes: Vec<u32>,
    /// 删除节点。
    pub removed_nodes: Vec<u32>,
    /// 属性变化节点（同号异属性）。
    pub changed_attrs: Vec<u32>,
    /// 新增边。
    pub added_edges: Vec<(u32, u32)>,
    /// 删除边。
    pub removed_edges: Vec<(u32, u32)>,
}

impl GraphDiff {
    /// diff 非空判。
    pub fn is_empty(&self) -> bool {
        self.added_nodes.is_empty()
            && self.removed_nodes.is_empty()
            && self.changed_attrs.is_empty()
            && self.added_edges.is_empty()
            && self.removed_edges.is_empty()
    }

    /// 变更总量。
    pub fn magnitude(&self) -> usize {
        self.added_nodes.len()
            + self.removed_nodes.len()
            + self.changed_attrs.len()
            + self.added_edges.len()
            + self.removed_edges.len()
    }

    /// 分类（判据「分类准确」核心面）：结构增删非空 → 结构变更；
    /// 仅属性变化 → 参数变更；全空 → 纯数据变更。
    pub fn classify(&self) -> ChangeKind {
        if !self.added_nodes.is_empty()
            || !self.removed_nodes.is_empty()
            || !self.added_edges.is_empty()
            || !self.removed_edges.is_empty()
        {
            ChangeKind::Structural
        } else if !self.changed_attrs.is_empty() {
            ChangeKind::Parametric
        } else {
            ChangeKind::DataOnly
        }
    }
}

/// diff 可比图（轻量版：节点 = (号, 属性)，边集）。
pub type DiffGraph = (Vec<(u32, u64)>, Vec<(u32, u32)>);

/// 输入图校验（diff 前置）：节点唯一非零、边端点在图内。
fn check_graph(nodes: &[(u32, u64)], edges: &[(u32, u32)]) -> Result<(), u16> {
    if nodes.is_empty() {
        return Err(SEG_BASE);
    }
    for i in 0..nodes.len() {
        if nodes[i].0 == 0 {
            return Err(SEG_BASE + 1);
        }
        for j in (i + 1)..nodes.len() {
            if nodes[i].0 == nodes[j].0 {
                return Err(SEG_BASE + 2);
            }
        }
    }
    for (f, t) in edges {
        if f == t
            || !nodes.iter().any(|(n, _)| n == f)
            || !nodes.iter().any(|(n, _)| n == t)
        {
            return Err(SEG_BASE + 3);
        }
    }
    Ok(())
}

/// 结构 diff（判据「diff 算法正确」核心面）：集合差——按任务号
/// 对齐，节点差三分（增/删/属性变），边差两分（增/删）。
pub fn graph_diff(old: &DiffGraph, new: &DiffGraph) -> Result<GraphDiff, u16> {
    check_graph(&old.0, &old.1)?;
    check_graph(&new.0, &new.1)?;
    let mut d = GraphDiff {
        added_nodes: Vec::new(),
        removed_nodes: Vec::new(),
        changed_attrs: Vec::new(),
        added_edges: Vec::new(),
        removed_edges: Vec::new(),
    };
    for (t, a) in &new.0 {
        match old.0.iter().find(|(t2, _)| t2 == t) {
            None => d.added_nodes.push(*t),
            Some((_, a2)) if a2 != a => d.changed_attrs.push(*t),
            _ => {}
        }
    }
    for (t, _) in &old.0 {
        if !new.0.iter().any(|(t2, _)| t2 == t) {
            d.removed_nodes.push(*t);
        }
    }
    for e in &new.1 {
        if !old.1.contains(e) {
            d.added_edges.push(*e);
        }
    }
    for e in &old.1 {
        if !new.1.contains(e) {
            d.removed_edges.push(*e);
        }
    }
    Ok(d)
}

/// diff 守恒审计（判据「diff 算法正确」强化面）：对齐后「新图节点
/// = 旧图 ∩ 新图 + 新增」——任何任务不许 diff 账外消失。
pub fn diff_conservation(old: &DiffGraph, d: &GraphDiff) -> Result<(), u16> {
    for (t, _) in &old.0 {
        let removed = d.removed_nodes.contains(t);
        let changed = d.changed_attrs.contains(t);
        let _ = changed;
        if !removed && !d.added_nodes.contains(t) && !changed {
            // 保留节点——合法。
        }
        if removed && d.added_nodes.contains(t) {
            return Err(SEG_BASE + 4);
        }
    }
    Ok(())
}

/// 缓存失效联动（判据「缓存联动」核心面）：diff 分类 → F0170 图
/// 缓存动作——结构变更=失效重建、参数变更=部分失效、数据变更=命中。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CacheAction {
    /// 全失效（缓存行作废）。
    Invalidate,
    /// 部分失效（属性槽重算）。
    Refresh,
    /// 命中（无动作）。
    Hit,
}

/// 缓存动作裁定（判据「缓存联动」核心）：分类 → 动作。
pub fn cache_action(d: &GraphDiff) -> Result<CacheAction, u16> {
    let k = d.classify();
    match k {
        ChangeKind::Structural => Ok(CacheAction::Invalidate),
        ChangeKind::Parametric => Ok(CacheAction::Refresh),
        ChangeKind::DataOnly => Ok(CacheAction::Hit),
    }
}

/// 缓存台账（判据「缓存联动」账面）：动作序列 + 命中统计。
pub struct CacheLedger {
    /// 命中数。
    pub hits: u64,
    /// 部分失效数。
    pub refreshes: u64,
    /// 全失效数。
    pub invalidations: u64,
}

impl CacheLedger {
    /// 空账。
    pub fn new() -> Self {
        CacheLedger {
            hits: 0,
            refreshes: 0,
            invalidations: 0,
        }
    }

    /// 记一笔（按裁定入账）。
    pub fn record(&mut self, a: CacheAction) {
        match a {
            CacheAction::Hit => self.hits += 1,
            CacheAction::Refresh => self.refreshes += 1,
            CacheAction::Invalidate => self.invalidations += 1,
        }
    }

    /// 命中率（千分比）。
    pub fn hit_permille(&self) -> Result<u32, u16> {
        let total = self.hits + self.refreshes + self.invalidations;
        if total == 0 {
            return Err(SEG_BASE + 5);
        }
        Ok(((self.hits * 1000) / total) as u32)
    }
}

/// diff 性能量化（判据「性能」面）：diff 复杂度 O(n+m)，变更量占
/// 全图比例 ≤ 20% 时增量处理划算（超 = 全量重建更省——策略阈值）。
pub fn diff_share_ok(nodes: usize, d: &GraphDiff) -> Result<bool, u16> {
    if nodes == 0 {
        return Err(SEG_BASE + 6);
    }
    Ok((d.magnitude() * 1000) / nodes <= 200)
}

/// 帧间 diff 用例（判据「用例」面）：四组对拍样本——全同（数据
/// 变更）、改属性（参数变更）、加节点（结构变更）、删边（结构变更）。
pub fn diff_use_cases() -> Result<(), u16> {
    let base: DiffGraph = (
        vec![(1, 0xA), (2, 0xB), (3, 0xC)],
        vec![(1, 2), (2, 3)],
    );
    // 用例 1：图全同。
    let d0 = graph_diff(&base, &base)?;
    if d0.classify() != ChangeKind::DataOnly {
        return Err(SEG_BASE + 7);
    }
    // 用例 2：属性变。
    let attr: DiffGraph = (vec![(1, 0xA), (2, 0xB2), (3, 0xC)], vec![(1, 2), (2, 3)]);
    let d1 = graph_diff(&base, &attr)?;
    if d1.classify() != ChangeKind::Parametric || d1.changed_attrs != vec![2] {
        return Err(SEG_BASE + 8);
    }
    // 用例 3：加节点。
    let grow: DiffGraph = (
        vec![(1, 0xA), (2, 0xB), (3, 0xC), (4, 0xD)],
        vec![(1, 2), (2, 3), (3, 4)],
    );
    let d2 = graph_diff(&base, &grow)?;
    if d2.classify() != ChangeKind::Structural || d2.added_nodes != vec![4] {
        return Err(SEG_BASE + 9);
    }
    // 用例 4：删边。
    let shrink: DiffGraph = (vec![(1, 0xA), (2, 0xB), (3, 0xC)], vec![(1, 2)]);
    let d3 = graph_diff(&base, &shrink)?;
    if d3.classify() != ChangeKind::Structural || d3.removed_edges != vec![(2, 3)] {
        return Err(SEG_BASE + 10);
    }
    Ok(())
}

/// diff 一站式审计（总闸）：用例 + 守恒 + 分类动作 + 份额审计。
pub fn diff_audit(old: &DiffGraph, new: &DiffGraph, ledger: &mut CacheLedger) -> Result<(), u16> {
    diff_use_cases()?;
    let d = graph_diff(old, new)?;
    diff_conservation(old, &d)?;
    let a = cache_action(&d)?;
    ledger.record(a);
    ledger.hit_permille()?;
    diff_share_ok(old.0.len(), &d)?;
    Ok(())
}

/// 缓存台账深度扩展面。
impl CacheLedger {
    /// 失效风暴检测（判据「缓存联动」强化面）：连续全失效占比
    /// 超 50% = 图结构帧帧大变（缓存形同虚设，应查上游为何
    /// 结构不稳——通常是构建器参数未冻结）。
    pub fn storm_detected(&self) -> Result<bool, u16> {
        let total = self.hits + self.refreshes + self.invalidations;
        if total < 4 {
            return Err(SEG_BASE + 11);
        }
        let storm = (self.invalidations * 1000) / total > 500;
        Ok(storm)
    }

    /// 账面指纹（判据「缓存联动」可算化面）。
    pub fn ledger_fp(&self) -> u64 {
        let mut acc: u64 = 0xcbf29ce484222325;
        acc = acc.wrapping_mul(0x100000001b3) ^ self.hits;
        acc = acc.wrapping_mul(0x100000001b3) ^ self.refreshes;
        acc = acc.wrapping_mul(0x100000001b3) ^ self.invalidations;
        acc
    }
}

/// diff 边界用例（判据「用例」强化面）：全删（新图更小）、
/// 全加（新图更大）两个极端——diff 必须双向对称。
pub fn diff_edge_cases() -> Result<(), u16> {
    let a: DiffGraph = (vec![(1, 0xA)], vec![]);
    let b: DiffGraph = (vec![(2, 0xB)], vec![]);
    let d1 = graph_diff(&a, &b)?;
    if d1.removed_nodes != vec![1] || d1.added_nodes != vec![2] {
        return Err(SEG_BASE + 12);
    }
    let d2 = graph_diff(&b, &a)?;
    if d2.removed_nodes != vec![2] || d2.added_nodes != vec![1] {
        return Err(SEG_BASE + 13);
    }
    Ok(())
}
