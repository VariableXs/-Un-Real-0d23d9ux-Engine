//! CGPU-F0177 · 依赖边权重与发射序细化（判据段 0x04B0）
//! 拓扑排序的发射序精细化：关键路径优先发射（F0163 基础上深化：
//! 关键路径动态重算——前序任务完成时间影响后续关键路径）、边权重
//! 表达（依赖不只是 0/1——数据量大小/传输成本/同步成本的加权边）、
//! 发射窗口（就绪集内按窗口分组发射——每窗口一次 GPU 提交减少
//! 提交次数）。判据：动态关键路径、加权发射序、窗口分组收益、
//! 对拍 F0163 基线、性能。
//!
//! 诊断码段 0x04B0（本模块独占：0x04B0~0x04BF）。
//! no_std + alloc 零 unwrap，全部 Result 化。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断码段基址。
pub const SEG_BASE: u16 = 0x04B0;

/// 加权依赖边（判据「边权重表达」载体）：依赖不是 0/1 而是成本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct WEdge {
    /// 前置任务。
    pub from: u32,
    /// 后继任务。
    pub to: u32,
    /// 数据量成本（传输字节折微秒）。
    pub xfer_us: u32,
    /// 同步成本（围栏等待折微秒）。
    pub sync_us: u32,
}

impl WEdge {
    /// 边校验：自环拒、成本在界内（单边 ≤ 1ms）。
    pub fn validate(&self) -> Result<(), u16> {
        if self.from == self.to {
            return Err(SEG_BASE);
        }
        if self.xfer_us > 1_000 || self.sync_us > 1_000 {
            return Err(SEG_BASE + 1);
        }
        Ok(())
    }

    /// 边总成本（微秒）。
    pub fn cost_us(&self) -> u64 {
        self.xfer_us as u64 + self.sync_us as u64
    }
}

/// 加权任务（发射序视角）：任务自身执行耗时。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct WTask {
    /// 任务号。
    pub task: u32,
    /// 自身执行耗时（微秒）。
    pub self_us: u32,
}

impl WTask {
    /// 校验：任务号非零、耗时非零（零耗时任务不占发射窗口）。
    pub fn validate(&self) -> Result<(), u16> {
        if self.task == 0 || self.self_us == 0 {
            return Err(SEG_BASE + 2);
        }
        Ok(())
    }
}

/// 加权图。
pub struct WGraph {
    /// 任务集。
    pub tasks: Vec<WTask>,
    /// 加权边集。
    pub edges: Vec<WEdge>,
}

impl WGraph {
    /// 空图。
    pub fn new() -> Self {
        WGraph {
            tasks: Vec::new(),
            edges: Vec::new(),
        }
    }

    /// 加任务（重复拒）。
    pub fn add_task(&mut self, t: WTask) -> Result<(), u16> {
        t.validate()?;
        if self.tasks.iter().any(|x| x.task == t.task) {
            return Err(SEG_BASE + 3);
        }
        self.tasks.push(t);
        Ok(())
    }

    /// 加边（端点必须在图内、重复拒）。
    pub fn add_edge(&mut self, e: WEdge) -> Result<(), u16> {
        e.validate()?;
        if !self.tasks.iter().any(|t| t.task == e.from)
            || !self.tasks.iter().any(|t| t.task == e.to)
        {
            return Err(SEG_BASE + 4);
        }
        if self.edges.iter().any(|x| x.from == e.from && x.to == e.to) {
            return Err(SEG_BASE + 5);
        }
        self.edges.push(e);
        Ok(())
    }

    /// 任务耗时查询。
    pub fn self_us(&self, task: u32) -> Result<u64, u16> {
        self.tasks
            .iter()
            .find(|t| t.task == task)
            .map(|t| t.self_us as u64)
            .ok_or(SEG_BASE + 6)
    }

    /// 静态关键路径（判据「关键路径」静态面）：入度归零拓扑 +
    /// 最早完成时间 DP——finish[t] = self + max(边成本 + finish[前驱])。
    /// 有环返回 Err。
    pub fn critical_path_static(&self) -> Result<(u64, Vec<u32>), u16> {
        let n = self.tasks.len();
        if n == 0 {
            return Err(SEG_BASE + 7);
        }
        let idx = |t: u32| self.tasks.iter().position(|x| x.task == t);
        let mut indeg: Vec<u32> = vec![0; n];
        for e in &self.edges {
            indeg[idx(e.to).ok_or(SEG_BASE + 6)?] += 1;
        }
        let mut ready: Vec<u32> = Vec::new();
        for t in &self.tasks {
            if indeg[idx(t.task).ok_or(SEG_BASE + 6)?] == 0 {
                ready.push(t.task);
            }
        }
        let mut finish: Vec<(u32, u64)> = Vec::new();
        let mut processed = 0usize;
        while let Some(t) = ready.first().copied() {
            ready.remove(0);
            processed += 1;
            let fi = self.self_us(t)?;
            let mut f = fi;
            for e in &self.edges {
                if e.to == t {
                    let pf = finish
                        .iter()
                        .find(|(p, _)| *p == e.from)
                        .map(|(_, v)| *v)
                        .ok_or(SEG_BASE + 8)?;
                    f = f.max(pf + e.cost_us() + fi);
                }
            }
            finish.push((t, f));
            for e in &self.edges {
                if e.from == t {
                    let j = idx(e.to).ok_or(SEG_BASE + 6)?;
                    indeg[j] -= 1;
                    if indeg[j] == 0 {
                        ready.push(e.to);
                    }
                }
            }
        }
        if processed != n {
            return Err(SEG_BASE + 9); // 有环
        }
        let best = finish.iter().map(|(_, v)| *v).max().unwrap_or(0);
        if best == 0 {
            return Err(SEG_BASE + 10);
        }
        Ok((best, finish.iter().map(|(t, _)| *t).collect()))
    }

    /// 动态关键路径重算（判据「动态关键路径」核心面）：给定前序
    /// 任务的观测完成时刻（覆盖静态估计），重算后继关键路径——
    /// 完成时间晚于静态估计的前驱会拉伸后继路径。
    pub fn critical_path_dynamic(
        &self,
        observed: &[(u32, u64)],
    ) -> Result<(u64, Vec<u32>), u16> {
        let (base, _) = self.critical_path_static()?;
        let mut stretch: u64 = 0;
        for (t, obs) in observed {
            let fi = self.self_us(*t)?;
            // 观测完成时刻若晚于最早开始 + 自耗时，则超额部分计入。
            let static_start = base.saturating_sub(fi);
            if *obs > static_start + fi {
                stretch = stretch.max(obs - (static_start + fi));
            }
        }
        if stretch > base {
            return Err(SEG_BASE + 11);
        }
        let (_, order) = self.critical_path_static()?;
        Ok((base + stretch, order))
    }
}

/// 发射窗口分组器（判据「窗口分组收益」核心面）：就绪集按窗口
/// 上限分组——每窗口一次 GPU 提交。窗口上限 = 提交数红线。
pub const WINDOW_CAP: usize = 8;

/// 发射序（判据「加权发射序」产出物）：序位 + 任务号 + 所属窗口。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LaunchSlot {
    /// 序位（0 起）。
    pub rank: u32,
    /// 任务号。
    pub task: u32,
    /// 窗口号。
    pub window: u32,
}

/// 加权发射序计算（判据「加权发射序」核心面）：就绪集内按
/// 「后继边总成本降序」（后继等待成本大者先发——减少全图等待）
/// 排序，再按 WINDOW_CAP 切窗。有环拒。
pub fn weighted_launch_order(g: &WGraph) -> Result<Vec<LaunchSlot>, u16> {
    let (total, mut topo) = g.critical_path_static()?;
    let _ = total;
    let mut succ_cost: Vec<(u32, u64)> = Vec::new();
    for t in &topo {
        let c: u64 = g
            .edges
            .iter()
            .filter(|e| e.from == *t)
            .map(|e| e.cost_us())
            .sum();
        succ_cost.push((*t, c));
    }
    // 拓扑约束下的稳定排序：先拓扑位次、后继成本降序做层内细化
    // ——逐位扫描，对无依赖冲突的相邻对按成本交换改进。
    let n = topo.len();
    for i in 0..n {
        for j in 0..n - 1 {
            let a = topo[j];
            let b = topo[j + 1];
            // a 在 b 前的约束：b 不依赖 a。
            let b_dep_a = g.edges.iter().any(|e| e.from == a && e.to == b);
            if b_dep_a {
                continue;
            }
            let ca = succ_cost.iter().find(|(t, _)| *t == a).map(|(_, c)| *c).unwrap_or(0);
            let cb = succ_cost.iter().find(|(t, _)| *t == b).map(|(_, c)| *c).unwrap_or(0);
            // a 前驱完成约束：a 依赖 b 时不可交换。
            let a_dep_b = g.edges.iter().any(|e| e.from == b && e.to == a);
            if !a_dep_b && cb > ca {
                topo.swap(j, j + 1);
            }
        }
    }
    let mut out: Vec<LaunchSlot> = Vec::new();
    for (i, t) in topo.iter().enumerate() {
        out.push(LaunchSlot {
            rank: i as u32,
            task: *t,
            window: (i / WINDOW_CAP) as u32,
        });
    }
    Ok(out)
}

/// 窗口分组收益（判据「窗口分组收益」量化面）：提交次数从任务数
/// 收敛到窗口数，提交节省 = 任务数 - 窗口数。
pub fn window_gain(slots: &[LaunchSlot]) -> Result<(usize, usize, i64), u16> {
    if slots.is_empty() {
        return Err(SEG_BASE + 12);
    }
    let mut wins: Vec<u32> = Vec::new();
    for s in slots {
        if !wins.contains(&s.window) {
            wins.push(s.window);
        }
    }
    let before = slots.len();
    let after = wins.len();
    if after > before {
        return Err(SEG_BASE + 13);
    }
    // 窗口容量校验：除末窗外每窗必须满 WINDOW_CAP。
    for w in 0..after - 1 {
        let cnt = slots.iter().filter(|s| s.window == w as u32).count();
        if cnt != WINDOW_CAP {
            return Err(SEG_BASE + 14);
        }
    }
    Ok((before, after, (before - after) as i64))
}

/// 对拍 F0163 基线（判据「对拍 F0163 基线」面）：加权发射序必须
/// 是合法拓扑序（逐边前驱先于后继）——细化不破坏拓扑语义。
pub fn topo_equivalence(g: &WGraph, order: &[LaunchSlot]) -> Result<(), u16> {
    let n = g.tasks.len();
    if order.len() != n {
        return Err(SEG_BASE + 15);
    }
    for e in &g.edges {
        let pa = order
            .iter()
            .position(|s| s.task == e.from)
            .ok_or(SEG_BASE + 16)?;
        let pb = order
            .iter()
            .position(|s| s.task == e.to)
            .ok_or(SEG_BASE + 16)?;
        if pa >= pb {
            return Err(SEG_BASE + 17);
        }
    }
    Ok(())
}

/// 发射序确定性（判据「性能」回归面）：同图两次计算序全等
/// （排序过程必须稳定，不允许迭代次序泄漏）。
pub fn launch_order_stable(g: &WGraph) -> Result<(), u16> {
    let a = weighted_launch_order(g)?;
    let b = weighted_launch_order(g)?;
    if a.len() != b.len() {
        return Err(SEG_BASE + 18);
    }
    for i in 0..a.len() {
        if a[i].task != b[i].task {
            return Err(SEG_BASE + 19);
        }
    }
    Ok(())
}

/// 发射序审计总闸（一站式收口）：加权序 + 窗收益 + 拓扑等价 + 稳定。
pub fn launch_audit(g: &WGraph) -> Result<(u64, i64), u16> {
    let slots = weighted_launch_order(g)?;
    topo_equivalence(g, &slots)?;
    launch_order_stable(g)?;
    let (_, _, wgain) = window_gain(&slots)?;
    let (crit, _) = g.critical_path_static()?;
    Ok((crit, wgain))
}

/// 加权图深度扩展面。
impl WGraph {
    /// 边成本普查（判据「边权重表达」可见性）：传输/同步成本的
    /// 总账与峰值边。
    pub fn cost_census(&self) -> Result<(u64, u64, u64), u16> {
        if self.edges.is_empty() {
            return Err(SEG_BASE + 20);
        }
        let xf: u64 = self.edges.iter().map(|e| e.xfer_us as u64).sum();
        let sy: u64 = self.edges.iter().map(|e| e.sync_us as u64).sum();
        let peak = self.edges.iter().map(|e| e.cost_us()).max().unwrap_or(0);
        Ok((xf, sy, peak))
    }

    /// 重边成本扫描（判据「加权发射序」优化面）：同步成本占比
    /// 超 50% = 图过度同步（发射被围栏切碎），提示合并窗口。
    pub fn sync_heavy(&self) -> Result<bool, u16> {
        let (xf, sy, _) = self.cost_census()?;
        if xf + sy == 0 {
            return Err(SEG_BASE + 21);
        }
        Ok(sy * 2 > xf + sy)
    }

    /// 入度零起点审计（判据「性能」前置）：无零入度任务的图
    /// 必有环（静态关键路径的前提被破坏）。
    pub fn entry_ok(&self) -> Result<(), u16> {
        let n = self.tasks.len();
        if n == 0 {
            return Err(SEG_BASE + 22);
        }
        let mut indeg: Vec<u32> = vec![0; n];
        for e in &self.edges {
            let j = self
                .tasks
                .iter()
                .position(|t| t.task == e.to)
                .ok_or(SEG_BASE + 6)?;
            indeg[j] += 1;
        }
        if !indeg.iter().any(|d| *d == 0) {
            return Err(SEG_BASE + 23);
        }
        Ok(())
    }
}

/// 窗口深度扩展（独立函数面）：窗口均衡审计——各窗任务总耗时
/// 方差有界（窗间失衡 = 某些提交过重，GPU 空转窗口出现）。
pub fn window_balance(slots: &[LaunchSlot], g: &WGraph) -> Result<u32, u16> {
    if slots.is_empty() {
        return Err(SEG_BASE + 24);
    }
    let mut wins: Vec<u32> = Vec::new();
    for s in slots {
        if !wins.contains(&s.window) {
            wins.push(s.window);
        }
    }
    let mut costs: Vec<u64> = Vec::new();
    for w in &wins {
        let c: u64 = slots
            .iter()
            .filter(|s| s.window == *w)
            .map(|s| g.self_us(s.task).unwrap_or(0))
            .sum();
        if c == 0 {
            return Err(SEG_BASE + 25);
        }
        costs.push(c);
    }
    let max = *costs.iter().max().unwrap_or(&0);
    let min = *costs.iter().min().unwrap_or(&0);
    if min == 0 {
        return Err(SEG_BASE + 26);
    }
    Ok(((min * 1000) / max) as u32)
}
