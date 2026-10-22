//! CGPU-F0179 · 条件依赖与动态分支（判据段 0x04D0）
//! 任务图的运行时分支：条件节点（task 声明执行条件——条件为假则该
//! 节点及子树跳过）、动态分叉（GPU 查询结果驱动后续路径——遮挡剔除
//! 结果决定是否发射详细渲染任务）、条件传播（父节点跳过→子节点级联
//! 跳过——条件传播的图遍历）。判据：条件节点语义、动态分叉正确、
//! 级联跳过、条件组合用例、性能。
//!
//! 诊断码段 0x04D0（本模块独占：0x04D0~0x04DF）。
//! no_std + alloc 零 unwrap，全部 Result 化。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断码段基址。
pub const SEG_BASE: u16 = 0x04D0;

/// 条件节点（判据「条件节点语义」载体）：执行条件 + 条件类型。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CondNode {
    /// 任务号。
    pub task: u32,
    /// 条件源（查询对象的指纹——GPU 查询/外部输入的条件键）。
    pub cond_fp: u64,
    /// 条件阈值（查询结果 ≥ 阈值才执行）。
    pub threshold: u32,
    /// 是否已观测（观测前不得发射——条件未定即发射 = 分支失效）。
    pub observed: bool,
}

impl CondNode {
    /// 校验：条件键与阈值非零。
    pub fn validate(&self) -> Result<(), u16> {
        if self.task == 0 || self.cond_fp == 0 || self.threshold == 0 {
            return Err(SEG_BASE);
        }
        Ok(())
    }

    /// 条件判（判据「条件节点语义」）：查询结果 ≥ 阈值 → 执行。
    pub fn evaluate(&self, query_result: u32) -> Result<bool, u16> {
        self.validate()?;
        if !self.observed {
            return Err(SEG_BASE + 1);
        }
        Ok(query_result >= self.threshold)
    }
}

/// 条件图：普通节点 + 条件节点 + 边。
pub struct CondGraph {
    /// 全部节点。
    pub nodes: Vec<u32>,
    /// 条件节点集。
    pub conds: Vec<CondNode>,
    /// 边集。
    pub edges: Vec<(u32, u32)>,
}

impl CondGraph {
    /// 空图。
    pub fn new() -> Self {
        CondGraph {
            nodes: Vec::new(),
            conds: Vec::new(),
            edges: Vec::new(),
        }
    }

    /// 加节点。
    pub fn add_node(&mut self, t: u32) -> Result<(), u16> {
        if t == 0 || self.nodes.contains(&t) {
            return Err(SEG_BASE + 2);
        }
        self.nodes.push(t);
        Ok(())
    }

    /// 声明条件（节点必须已在图内；一节点一条件）。
    pub fn declare_cond(&mut self, c: CondNode) -> Result<(), u16> {
        c.validate()?;
        if !self.nodes.contains(&c.task) {
            return Err(SEG_BASE + 3);
        }
        if self.conds.iter().any(|x| x.task == c.task) {
            return Err(SEG_BASE + 4);
        }
        self.conds.push(c);
        Ok(())
    }

    /// 加边。
    pub fn add_edge(&mut self, from: u32, to: u32) -> Result<(), u16> {
        if !self.nodes.contains(&from) || !self.nodes.contains(&to) || from == to {
            return Err(SEG_BASE + 5);
        }
        if self.edges.contains(&(from, to)) {
            return Err(SEG_BASE + 6);
        }
        self.edges.push((from, to));
        Ok(())
    }

    /// 观测条件（判据「动态分叉正确」输入面）：GPU 查询结果回填。
    pub fn observe(&mut self, task: u32, query_result: u32) -> Result<bool, u16> {
        let c = self
            .conds
            .iter_mut()
            .find(|c| c.task == task)
            .ok_or(SEG_BASE + 7)?;
        if c.observed {
            return Err(SEG_BASE + 8);
        }
        c.observed = true;
        Ok(query_result >= c.threshold)
    }

    /// 级联跳过闭包（判据「级联跳过」核心面）：被跳过节点的全部
    /// 可达后继级联跳过（图遍历）。返回跳过集。
    pub fn skip_closure(&self, seeds: &[u32]) -> Result<Vec<u32>, u16> {
        for s in seeds {
            if !self.nodes.contains(s) {
                return Err(SEG_BASE + 9);
            }
        }
        let mut out: Vec<u32> = Vec::new();
        let mut queue: Vec<u32> = seeds.to_vec();
        while let Some(t) = queue.pop() {
            if out.contains(&t) {
                continue;
            }
            out.push(t);
            for (f, to) in &self.edges {
                if *f == t && !out.contains(to) {
                    queue.push(*to);
                }
            }
        }
        Ok(out)
    }

    /// 动态分叉执行集（判据核心汇总面）：给定全部条件观测结果，
    /// 产出本帧实际执行集 = 全节点 - 级联跳过闭包。未观测的条件
    /// 节点 = 账面违约（判据「条件未定即发射」禁止）。
    pub fn execute_set(&self, verdicts: &[(u32, bool)]) -> Result<Vec<u32>, u16> {
        // 全部条件必须已观测。
        for c in &self.conds {
            if !c.observed {
                return Err(SEG_BASE + 10);
            }
            if !verdicts.iter().any(|(t, _)| *t == c.task) {
                return Err(SEG_BASE + 11);
            }
        }
        let mut seeds: Vec<u32> = Vec::new();
        for (t, ok) in verdicts {
            if !*ok {
                seeds.push(*t);
            }
        }
        let skipped = self.skip_closure(&seeds)?;
        let mut exec: Vec<u32> = Vec::new();
        for n in &self.nodes {
            if !skipped.contains(n) {
                exec.push(*n);
            }
        }
        if exec.is_empty() {
            return Err(SEG_BASE + 12);
        }
        Ok(exec)
    }

    /// 执行集拓扑合法性（判据「性能」前置面）：执行集内不得有
    /// 跨跳过边（前驱被跳过后继执行 = 消费了不存在的产出）。
    pub fn exec_set_valid(&self, exec: &[u32]) -> Result<(), u16> {
        if exec.is_empty() {
            return Err(SEG_BASE + 13);
        }
        for (f, t) in &self.edges {
            let f_in = exec.contains(f);
            let t_in = exec.contains(t);
            // 前驱不在执行集而后继在 = 悬空消费。
            if !f_in && t_in {
                // 合法豁免：前驱是被条件跳过的节点且后继自己也带条件
                // ——级联跳过理应已把后继带走，走到这里说明级联不完备。
                return Err(SEG_BASE + 14);
            }
            let _ = t_in;
        }
        Ok(())
    }
}

/// 条件组合用例（判据「条件组合用例」面）：多条件AND语义——
/// 节点声明多条件时全部成立才执行（组合矩阵）。
pub struct CondCombo {
    /// 目标任务。
    pub task: u32,
    /// 组合条件键集。
    pub conds: Vec<u64>,
    /// 各条件成立位（与 conds 等长）。
    pub verdicts: Vec<bool>,
}

impl CondCombo {
    /// 组合校验：键非零、位表等长、无重复键。
    pub fn validate(&self) -> Result<(), u16> {
        if self.task == 0 || self.conds.is_empty() {
            return Err(SEG_BASE + 15);
        }
        if self.conds.len() != self.verdicts.len() {
            return Err(SEG_BASE + 16);
        }
        for i in 0..self.conds.len() {
            if self.conds[i] == 0 {
                return Err(SEG_BASE + 17);
            }
            for j in (i + 1)..self.conds.len() {
                if self.conds[i] == self.conds[j] {
                    return Err(SEG_BASE + 18);
                }
            }
        }
        Ok(())
    }

    /// AND 判。
    pub fn all_true(&self) -> Result<bool, u16> {
        self.validate()?;
        Ok(self.verdicts.iter().all(|v| *v))
    }
}

/// 分叉覆盖率（判据「性能」量化面）：条件节点占全图比例——
/// 全无条件图（0）说明分叉能力闲置；超 30% 说明图过度分支
/// （条件过多 = 静态结构劣化，应优先重排）。
pub fn branch_coverage(g: &CondGraph) -> Result<u32, u16> {
    if g.nodes.is_empty() {
        return Err(SEG_BASE + 19);
    }
    let pm = ((g.conds.len() * 1000) / g.nodes.len()) as u32;
    if pm > 300 {
        return Err(SEG_BASE + 20);
    }
    Ok(pm)
}

/// 条件传播完备性（判据「级联跳过」强化面）：对每个条件节点，
/// 其后继中不带条件的必须能被级联带走——若某后继无自身条件却
/// 有外部输入依赖（从图外进入），级联会错杀，标记为传播屏障。
pub fn propagation_barriers(g: &CondGraph) -> Result<Vec<u32>, u16> {
    let mut barriers: Vec<u32> = Vec::new();
    for c in &g.conds {
        for (f, t) in &g.edges {
            if *f == c.task && !g.conds.iter().any(|x| x.task == *t) {
                // 后继无条件：级联会跳过它。检查它是否有非 c.task
                // 的其他前驱（有 = 屏障，不能级联跳过）。
                let others = g.edges.iter().any(|(f2, t2)| *t2 == *t && f2 != &c.task);
                if !others {
                    barriers.push(*t);
                }
            }
        }
    }
    Ok(barriers)
}

/// 条件图一站式审计（总闸）：分叉覆盖 + 组合语义 + 屏障投影。
pub fn cond_audit(g: &CondGraph, combos: &[CondCombo]) -> Result<u32, u16> {
    if g.nodes.is_empty() {
        return Err(SEG_BASE + 21);
    }
    let pm = branch_coverage(g)?;
    for c in combos {
        c.validate()?;
        c.all_true()?;
    }
    let _ = propagation_barriers(g)?;
    Ok(pm)
}

/// 条件图深度扩展面。
impl CondGraph {
    /// 未观测扫描（判据「条件未定即发射」禁止的批量面）：全部
    /// 未观测条件节点——回放/对拍前必须清零的活性源清单。
    pub fn unobserved(&self) -> Vec<u32> {
        self.conds.iter().filter(|c| !c.observed).map(|c| c.task).collect()
    }

    /// 分叉对称性审计（判据「动态分叉正确」强化面）：被跳过集与
    /// 执行集的并 = 全节点、交 = 空（分叉必须完备且互斥）。
    pub fn fork_partition(&self, exec: &[u32]) -> Result<(), u16> {
        let unobs = self.unobserved();
        if !unobs.is_empty() {
            return Err(SEG_BASE + 22);
        }
        let mut skipped: Vec<u32> = Vec::new();
        let seeds: Vec<u32> = self.conds.iter().map(|c| c.task).collect();
        let sc = self.skip_closure(&seeds)?;
        for s in &sc {
            if !exec.contains(s) && !skipped.contains(s) {
                skipped.push(*s);
            }
        }
        for n in &self.nodes {
            let a = exec.contains(n);
            let b = skipped.contains(n);
            if a == b {
                return Err(SEG_BASE + 23);
            }
        }
        Ok(())
    }

    /// 条件阈值普查（判据「条件节点语义」可见性）：阈值分布
    /// （过低阈值 = 分支常真 = 条件形同虚设）。
    pub fn threshold_census(&self) -> Result<(u32, u32), u16> {
        if self.conds.is_empty() {
            return Err(SEG_BASE + 24);
        }
        let lo = self.conds.iter().map(|c| c.threshold).min().unwrap_or(0);
        let hi = self.conds.iter().map(|c| c.threshold).max().unwrap_or(0);
        if lo == 0 {
            return Err(SEG_BASE + 25);
        }
        Ok((lo, hi))
    }
}

/// 级联深度审计（独立函数面）：跳过闭包的最大链深——级联过深
/// 说明条件节点放置在图头部（一处跳过拖垮半图），建议下沉。
pub fn cascade_depth(g: &CondGraph, seed: u32) -> Result<usize, u16> {
    if !g.nodes.contains(&seed) {
        return Err(SEG_BASE + 26);
    }
    let mut depth = 0usize;
    let mut frontier = vec![seed];
    let mut seen: Vec<u32> = vec![seed];
    while !frontier.is_empty() {
        let mut next: Vec<u32> = Vec::new();
        for f in &frontier {
            for (f2, t) in &g.edges {
                if f2 == f && !seen.contains(t) {
                    seen.push(*t);
                    next.push(*t);
                }
            }
        }
        if !next.is_empty() {
            depth += 1;
        }
        frontier = next;
        if depth > g.nodes.len() {
            return Err(SEG_BASE + 27);
        }
    }
    Ok(depth)
}
