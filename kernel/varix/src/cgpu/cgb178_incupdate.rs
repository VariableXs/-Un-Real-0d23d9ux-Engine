//! CGPU-F0178 · 依赖图增量更新（判据段 0x04C0）
//! 运行时图变更：图构建后运行中插入/删除节点（增量拓扑排序——只
//! 重排受影响子图而非全图）、边动态增删（依赖关系运行时变化——如
//! 异步加载完成后才可用的资源节点）、增量正确性（增量更新后的执行序
//! 与全量重建一致）。判据：增量插入/删除正确、执行序等价断言、性能
//! （增量 vs 全量提速 ≥5 倍）、边界（增量后产生环检测）、回归。
//!
//! 诊断码段 0x04C0（本模块独占：0x04C0~0x04CF）。
//! no_std + alloc 零 unwrap，全部 Result 化。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断码段基址。
pub const SEG_BASE: u16 = 0x04C0;

/// 增量图（判据载体）：节点 + 边 + 脏区标记（受影响子图）。
pub struct IncGraph {
    /// 节点集。
    pub nodes: Vec<u32>,
    /// 边集 (from, to)。
    pub edges: Vec<(u32, u32)>,
    /// 脏区：增量操作触碰过的节点（增量重排只扫这里）。
    pub dirty: Vec<u32>,
    /// 增量操作次数（性能对比的计数源）。
    pub ops: u64,
}

impl IncGraph {
    /// 空图。
    pub fn new() -> Self {
        IncGraph {
            nodes: Vec::new(),
            edges: Vec::new(),
            dirty: Vec::new(),
            ops: 0,
        }
    }

    /// 加节点（初始构建用，不记脏区）。
    pub fn add_node(&mut self, t: u32) -> Result<(), u16> {
        if t == 0 {
            return Err(SEG_BASE);
        }
        if self.nodes.contains(&t) {
            return Err(SEG_BASE + 1);
        }
        self.nodes.push(t);
        Ok(())
    }

    /// 加边（初始构建用）。
    pub fn add_edge(&mut self, from: u32, to: u32) -> Result<(), u16> {
        if !self.nodes.contains(&from) || !self.nodes.contains(&to) {
            return Err(SEG_BASE + 2);
        }
        if from == to || self.edges.contains(&(from, to)) {
            return Err(SEG_BASE + 3);
        }
        self.edges.push((from, to));
        Ok(())
    }

    /// 增量插入节点（判据「增量插入正确」核心面）：新节点挂入
    /// 现有边、脏区记录新节点与直接邻域——不扫全图。
    pub fn insert_node(&mut self, t: u32, deps: &[u32], feeders: &[u32]) -> Result<(), u16> {
        if t == 0 || self.nodes.contains(&t) {
            return Err(SEG_BASE + 4);
        }
        for d in deps {
            if !self.nodes.contains(d) {
                return Err(SEG_BASE + 5);
            }
            if *d == t {
                return Err(SEG_BASE + 6);
            }
        }
        for f in feeders {
            if !self.nodes.contains(f) || *f == t {
                return Err(SEG_BASE + 7);
            }
        }
        self.nodes.push(t);
        for d in deps {
            self.edges.push((*d, t));
        }
        for f in feeders {
            self.edges.push((t, *f));
        }
        self.dirty.push(t);
        for d in deps {
            self.dirty.push(*d);
        }
        for f in feeders {
            self.dirty.push(*f);
        }
        self.ops += 1;
        Ok(())
    }

    /// 增量删除节点（判据「增量删除正确」核心面）：节点及其全部
    /// 关联边删除、脏区记录其邻域（桥接边自动消失——语义由调用方
    /// 以重连边补偿，此处账面只做删除）。
    pub fn remove_node(&mut self, t: u32) -> Result<usize, u16> {
        if !self.nodes.contains(&t) {
            return Err(SEG_BASE + 8);
        }
        self.nodes.retain(|x| *x != t);
        let before = self.edges.len();
        self.edges.retain(|(f, to)| *f != t && *to != t);
        let removed = before - self.edges.len();
        for (f, to) in &self.edges {
            if *f != t {
                self.dirty.push(*f);
            }
            if *to != t {
                self.dirty.push(*to);
            }
        }
        self.dirty.retain(|x| self.nodes.contains(x));
        self.ops += 1;
        Ok(removed)
    }

    /// 增量加边（判据「边动态增删」面）：加后立即环检测——增量后
    /// 产生环是硬边界（判据「边界」），回滚该边。
    pub fn inc_add_edge(&mut self, from: u32, to: u32) -> Result<(), u16> {
        self.add_edge(from, to)?;
        self.dirty.push(from);
        self.dirty.push(to);
        self.ops += 1;
        if self.has_cycle() {
            self.edges.retain(|(f, t)| !(*f == from && *t == to));
            self.ops += 1;
            return Err(SEG_BASE + 9);
        }
        Ok(())
    }

    /// 增量删边。
    pub fn inc_remove_edge(&mut self, from: u32, to: u32) -> Result<(), u16> {
        let before = self.edges.len();
        self.edges.retain(|(f, t)| !(*f == from && *t == to));
        if self.edges.len() == before {
            return Err(SEG_BASE + 10);
        }
        self.dirty.push(from);
        self.dirty.push(to);
        self.ops += 1;
        Ok(())
    }

    /// 环检测（DFS 三色法）。
    pub fn has_cycle(&self) -> bool {
        // 0=白 1=灰 2=黑
        let mut color: Vec<(u32, u8)> = self.nodes.iter().map(|n| (*n, 0u8)).collect();
        for start in &self.nodes {
            if color.iter().find(|(n, _)| n == start).map(|(_, c)| *c) != Some(0) {
                continue;
            }
            let mut stack: Vec<(u32, usize)> = Vec::new();
            stack.push((*start, 0usize));
            while let Some((cur, ei)) = stack.pop() {
                let ci = color.iter_mut().find(|(n, _)| *n == cur).map(|(_, c)| c);
                match ci {
                    Some(c) if *c == 0 => *c = 1,
                    _ => {}
                }
                let out: Vec<u32> = self
                    .edges
                    .iter()
                    .filter(|(f, _)| *f == cur)
                    .map(|(_, t)| *t)
                    .collect();
                let mut descended = false;
                let mut i = ei;
                while i < out.len() {
                    let nxt = out[i];
                    let st = color.iter().find(|(n, _)| *n == nxt).map(|(_, c)| *c);
                    if st == Some(1) {
                        return true; // 灰边 = 环
                    }
                    if st == Some(0) {
                        stack.push((cur, i + 1));
                        stack.push((nxt, 0));
                        descended = true;
                        break;
                    }
                    i += 1;
                }
                if !descended {
                    if let Some(c) = color.iter_mut().find(|(n, _)| *n == cur) {
                        c.1 = 2;
                    }
                }
            }
        }
        false
    }

    /// 全量拓扑序（正确性基准）。
    pub fn full_topo(&self) -> Result<Vec<u32>, u16> {
        let n = self.nodes.len();
        if n == 0 {
            return Err(SEG_BASE + 11);
        }
        let idx = |t: u32| self.nodes.iter().position(|x| *x == t);
        let mut indeg: Vec<u32> = vec![0; n];
        for (f, t) in &self.edges {
            indeg[idx(*t).ok_or(SEG_BASE + 12)?] += 1;
        }
        let mut ready: Vec<u32> = Vec::new();
        for nd in &self.nodes {
            if indeg[idx(*nd).ok_or(SEG_BASE + 12)?] == 0 {
                ready.push(*nd);
            }
        }
        let mut order: Vec<u32> = Vec::new();
        while let Some(t) = ready.first().copied() {
            ready.remove(0);
            order.push(t);
            for (f, to) in &self.edges {
                if *f == t {
                    let j = idx(*to).ok_or(SEG_BASE + 12)?;
                    indeg[j] -= 1;
                    if indeg[j] == 0 {
                        ready.push(*to);
                    }
                }
            }
        }
        if order.len() != n {
            return Err(SEG_BASE + 13); // 环
        }
        Ok(order)
    }

    /// 增量拓扑（判据「只重排受影响子图」核心面）：脏区局部重排——
    /// 非脏节点保持原相对序，脏节点按局部 Kahn 插入。返回新全序 +
    /// 实际扫描节点数（性能证据）。
    pub fn inc_topo(&self, prev: &[u32]) -> Result<(Vec<u32>, usize), u16> {
        if self.has_cycle() {
            return Err(SEG_BASE + 14);
        }
        if prev.len() != self.nodes.len() {
            return Err(SEG_BASE + 15);
        }
        let clean: Vec<u32> = prev
            .iter()
            .filter(|t| !self.dirty.contains(t) && self.nodes.contains(t))
            .copied()
            .collect();
        let dirty: Vec<u32> = self
            .nodes
            .iter()
            .filter(|t| self.dirty.contains(t))
            .copied()
            .collect();
        // 非脏节点的相对序保持；脏节点按拓扑约束二分插入。
        let mut out: Vec<u32> = clean.clone();
        for d in &dirty {
            // 找插入位：必须在全部前驱之后、全部后继之前。
            let mut lo = 0usize;
            let mut hi = out.len();
            for (f, t) in &self.edges {
                if *t == *d {
                    if let Some(p) = out.iter().position(|x| x == f) {
                        if p + 1 > lo {
                            lo = p + 1;
                        }
                    }
                }
                if *f == *d {
                    if let Some(p) = out.iter().position(|x| x == t) {
                        if p < hi {
                            hi = p;
                        }
                    }
                }
            }
            if lo > hi {
                return Err(SEG_BASE + 16);
            }
            out.insert(lo, *d);
        }
        // 全序合法性校验。
        for (f, t) in &self.edges {
            let pf = out.iter().position(|x| x == f).ok_or(SEG_BASE + 17)?;
            let pt = out.iter().position(|x| x == t).ok_or(SEG_BASE + 17)?;
            if pf >= pt {
                return Err(SEG_BASE + 18);
            }
        }
        let scanned = clean.len() + dirty.len();
        Ok((out, scanned))
    }

    /// 执行序等价断言（判据「执行序等价」核心面）：增量序与全量
    /// 重建序的拓扑约束集一致——逐边前驱先于后继。
    pub fn equivalence_assert(&self, inc_order: &[u32]) -> Result<(), u16> {
        let full = self.full_topo()?;
        if full.len() != inc_order.len() {
            return Err(SEG_BASE + 19);
        }
        for (f, t) in &self.edges {
            let pf = inc_order.iter().position(|x| x == f).ok_or(SEG_BASE + 20)?;
            let pt = inc_order.iter().position(|x| x == t).ok_or(SEG_BASE + 20)?;
            if pf >= pt {
                return Err(SEG_BASE + 21);
            }
        }
        Ok(())
    }

    /// 清脏区（帧收尾）。
    pub fn clear_dirty(&mut self) {
        self.dirty.clear();
    }
}

/// 性能对比（判据「增量 vs 全量提速 ≥5 倍」量化面）：扫描节点比
/// ——全量扫描 n，增量扫描 s，加速比 = n / s ≥ 5（脏区占比
/// ≤ 20% 时）。这是账面加速比（比较的是扫描量，非墙钟）。
pub fn speedup_ratio(total_nodes: usize, scanned: usize) -> Result<u32, u16> {
    if total_nodes == 0 || scanned == 0 || scanned > total_nodes {
        return Err(SEG_BASE + 22);
    }
    let ratio_x100 = (total_nodes * 100) / scanned;
    if ratio_x100 < 500 {
        return Err(SEG_BASE + 23);
    }
    Ok(ratio_x100 as u32)
}

/// 脏区占比审计（判据「增量正确性」边界面）：脏区占比超 20% 时
/// 增量不如全量，应降级为全量重建（策略面不是错误面）。
pub fn dirty_share(total_nodes: usize, dirty: usize) -> Result<bool, u16> {
    if total_nodes == 0 || dirty > total_nodes {
        return Err(SEG_BASE + 24);
    }
    Ok((dirty * 1000) / total_nodes <= 200)
}

/// 增量回归演练（判据「回归」一站式）：插入 → 等价断言 → 删除 →
/// 等价断言 → 增量加边成环 → 回滚验证，五步全过才绿。
pub fn inc_regression(g: &mut IncGraph, probe: u32) -> Result<(), u16> {
    let prev = g.full_topo()?;
    g.insert_node(probe, &[prev[0]], &[prev[prev.len() - 1]])?;
    let (order, scanned) = g.inc_topo(&prev)?;
    g.equivalence_assert(&order)?;
    speedup_ratio(g.nodes.len(), scanned)?;
    g.remove_node(probe)?;
    let (order2, _) = g.inc_topo(&prev)?;
    g.equivalence_assert(&order2)?;
    g.clear_dirty();
    Ok(())
}

/// 增量图深度扩展面。
impl IncGraph {
    /// 脏区去重压缩（判据「只重排受影响子图」卫生面）：脏区必须
    /// 去重——重复脏标会让增量扫描虚胖（性能证据失真）。
    pub fn dirty_dedup(&mut self) -> usize {
        let mut seen: Vec<u32> = Vec::new();
        for d in &self.dirty {
            if !seen.contains(d) {
                seen.push(*d);
            }
        }
        let removed = self.dirty.len() - seen.len();
        self.dirty = seen;
        removed
    }

    /// 增量删除的桥接补偿（判据「增量删除正确」强化面）：删除
    /// 节点后其前驱与后继之间补直连边（保持可达语义）。
    pub fn remove_with_bridging(&mut self, t: u32) -> Result<usize, u16> {
        if !self.nodes.contains(&t) {
            return Err(SEG_BASE + 25);
        }
        let preds: Vec<u32> = self
            .edges
            .iter()
            .filter(|(_, to)| *to == t)
            .map(|(f, _)| *f)
            .collect();
        let succs: Vec<u32> = self
            .edges
            .iter()
            .filter(|(f, _)| *f == t)
            .map(|(_, to)| *to)
            .collect();
        let removed = self.remove_node(t)?;
        for p in &preds {
            for s in &succs {
                if p != s
                    && self.nodes.contains(p)
                    && self.nodes.contains(s)
                    && !self.edges.contains(&(*p, *s))
                {
                    self.edges.push((*p, *s));
                }
            }
        }
        if self.has_cycle() {
            return Err(SEG_BASE + 26);
        }
        Ok(removed)
    }

    /// 操作密度（判据「性能」量化面）：每节点平均增量操作数。
    pub fn ops_per_node(&self) -> Result<u32, u16> {
        if self.nodes.is_empty() || self.ops == 0 {
            return Err(SEG_BASE + 27);
        }
        Ok(((self.ops * 100) / self.nodes.len() as u64) as u32)
    }
}

/// 增量等价深化（独立函数面）：增量序与全量序的「可达关系等价」
/// ——两序允许平手不同，但每条边的先后关系必须一致。
pub fn reach_equivalence(g: &IncGraph, inc: &[u32]) -> Result<(), u16> {
    let full = g.full_topo()?;
    for (f, t) in &g.edges {
        let a1 = inc.iter().position(|x| x == f).ok_or(SEG_BASE + 28)?;
        let b1 = inc.iter().position(|x| x == t).ok_or(SEG_BASE + 28)?;
        let a2 = full.iter().position(|x| x == f).ok_or(SEG_BASE + 28)?;
        let b2 = full.iter().position(|x| x == t).ok_or(SEG_BASE + 28)?;
        let ord1 = a1 < b1;
        let ord2 = a2 < b2;
        if ord1 != ord2 {
            return Err(SEG_BASE + 29);
        }
    }
    Ok(())
}
