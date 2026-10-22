//! cgb163_toposort.rs —— CGPU-F0163 · 拓扑排序与关键路径
//!
//! 帧任务图（F0161/F0162 产物）的调度前处理：
//! - Kahn 拓扑排序（队列间并行分层的分层序——同层任务可并行）；
//! - 环检测（有环图拒绝调度，环路径显性报出）；
//! - 关键路径（最长路径 = 帧下界，关键路径上的任务不可延迟）；
//! - 层内优先级细化（同层按 prio 与截止时间排序）。
//! 诊断码段 0x0420（本席 WB1022 分配表，全域唯一）。
//! no_std + alloc 零 unwrap、零 panic 路径。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断码段基址。
pub const SEG_BASE: u16 = 0x0420;

/// 任务成本模型（关键路径原料）。
#[derive(Clone, Copy, Debug)]
pub struct TaskCost {
    /// 任务号（1..n 连续）。
    pub task: u32,
    /// 预估耗时（微秒）。
    pub cost_us: u32,
}

impl TaskCost {
    /// 自检。
    pub fn validate(&self) -> Result<(), u16> {
        if self.task == 0 || self.cost_us == 0 {
            return Err(SEG_BASE + 1);
        }
        Ok(())
    }
}

/// 依赖边（调度向：from 先于 to）。
#[derive(Clone, Copy, Debug)]
pub struct DepEdge {
    /// 前任务。
    pub from: u32,
    /// 后任务。
    pub to: u32,
}

impl DepEdge {
    /// 自检。
    pub fn validate(&self) -> Result<(), u16> {
        if self.from == 0 || self.to == 0 || self.from == self.to {
            return Err(SEG_BASE + 2);
        }
        Ok(())
    }
}

/// 拓扑结果。
#[derive(Clone, Debug)]
pub struct TopoResult {
    /// 分层序：每层任务号集（同层可并行）。
    pub layers: Vec<Vec<u32>>,
    /// 关键路径（任务号序）。
    pub critical: Vec<u32>,
    /// 关键路径总耗时（微秒）。
    pub critical_us: u64,
    /// 是否有环（false = 无环才有效）。
    pub cyclic: bool,
}

impl TopoResult {
    /// 总层数。
    pub fn depth(&self) -> usize {
        self.layers.len()
    }

    /// 任务总数（分层覆盖数）。
    pub fn covered(&self) -> usize {
        self.layers.iter().map(|l| l.len()).sum()
    }

    /// 自检：无环时任务全覆盖。
    pub fn validate(&self, n_tasks: usize) -> Result<(), u16> {
        if self.cyclic {
            return Err(SEG_BASE + 3);
        }
        if self.covered() != n_tasks {
            return Err(SEG_BASE + 4);
        }
        if self.critical_us == 0 || self.critical.is_empty() {
            return Err(SEG_BASE + 5);
        }
        Ok(())
    }
}

/// Kahn 拓扑 + 关键路径（判据核心）：
/// - Kahn 分层（入度归零一波进一层）；
/// - 任一层推进不动 = 有环（cyclic 置位，环检测天然内嵌）；
/// - 关键路径 = DAG 最长路（成本取 cost_us，DP 逐层松弛）。
pub fn topo_sort(tasks: &[TaskCost], deps: &[DepEdge]) -> Result<TopoResult, u16> {
    if tasks.is_empty() {
        return Err(SEG_BASE + 6);
    }
    for t in tasks {
        t.validate()?;
    }
    for d in deps {
        d.validate()?;
    }
    let n = tasks.len() as u32;
    let mut idx: Vec<u32> = Vec::new();
    for (i, t) in tasks.iter().enumerate() {
        if t.task as usize != i + 1 {
            return Err(SEG_BASE + 7);
        }
        idx.push(t.task);
    }
    let _ = idx;
    // 入度表。
    let mut indeg = vec![0u32; (n + 1) as usize];
    let mut adj: Vec<Vec<u32>> = vec![Vec::new(); (n + 1) as usize];
    for d in deps {
        if d.from > n || d.to > n {
            return Err(SEG_BASE + 8);
        }
        adj[d.from as usize].push(d.to);
        indeg[d.to as usize] += 1;
    }
    // Kahn。
    let mut layers: Vec<Vec<u32>> = Vec::new();
    let mut frontier: Vec<u32> = Vec::new();
    for t in 1..=n {
        if indeg[t as usize] == 0 {
            frontier.push(t);
        }
    }
    let mut done = 0usize;
    let cyclic = frontier.is_empty();
    while !frontier.is_empty() {
        let mut next: Vec<u32> = Vec::new();
        for t in &frontier {
            done += 1;
            for &m in &adj[*t as usize] {
                indeg[m as usize] -= 1;
                if indeg[m as usize] == 0 {
                    next.push(m);
                }
            }
        }
        layers.push(frontier);
        frontier = next;
    }
    if done < n as usize {
        return Ok(TopoResult {
            layers,
            critical: Vec::new(),
            critical_us: 0,
            cyclic: true,
        });
    }
    let _ = cyclic;
    // 关键路径 DP（判据「关键路径」）：按层序松弛 dist 与前驱。
    let mut dist = vec![0u64; (n + 1) as usize];
    let mut prev = vec![0u32; (n + 1) as usize];
    for layer in &layers {
        for &t in layer {
            for &m in &adj[t as usize] {
                let w = tasks[(t - 1) as usize].cost_us as u64;
                if dist[t as usize] + w > dist[m as usize] {
                    dist[m as usize] = dist[t as usize] + w;
                    prev[m as usize] = t;
                }
            }
        }
    }
    // 终点 = dist 最大者；回溯关键路径。
    let mut end = 1u32;
    for t in 1..=n {
        if dist[t as usize] > dist[end as usize] {
            end = t;
        }
    }
    let critical_us = dist[end as usize];
    let mut critical = Vec::new();
    let mut cur = end;
    while cur != 0 {
        critical.push(cur);
        cur = prev[cur as usize];
    }
    critical.reverse();
    if critical_us == 0 {
        return Err(SEG_BASE + 9);
    }
    Ok(TopoResult {
        layers,
        critical,
        critical_us,
        cyclic: false,
    })
}

/// 环路径显性报告（判据「环检测」细化）：cyclic 时给出环上
/// 任务号集（入度未清者）。
pub fn cycle_members(tasks: &[TaskCost], deps: &[DepEdge]) -> Result<Vec<u32>, u16> {
    let r = topo_sort(tasks, deps)?;
    if !r.cyclic {
        return Ok(Vec::new());
    }
    let n = tasks.len() as u32;
    let mut indeg = vec![0u32; (n + 1) as usize];
    for d in deps {
        indeg[d.to as usize] += 1;
    }
    let mut on_cycle = Vec::new();
    for t in 1..=n {
        if indeg[t as usize] > 0 {
            on_cycle.push(t);
        }
    }
    if on_cycle.is_empty() {
        return Err(SEG_BASE + 10);
    }
    Ok(on_cycle)
}

/// 层内细化排序（判据「层内优先级细化」）：同层按 (prio,
/// deadline_us, task) 升序——prio 小者优先、同优先级截止早者
/// 先行、再同按任务号稳定序。
pub fn refine_layer(
    layer: &[u32],
    prio_of: impl Fn(u32) -> u16,
    deadline_of: impl Fn(u32) -> u32,
) -> Result<Vec<u32>, u16> {
    if layer.is_empty() {
        return Err(SEG_BASE + 11);
    }
    let mut keyed: Vec<(u16, u32, u32)> = Vec::new();
    for &t in layer {
        keyed.push((prio_of(t), deadline_of(t), t));
    }
    keyed.sort_unstable();
    Ok(keyed.into_iter().map(|(_, _, t)| t).collect())
}

/// 并行度统计（判据支撑）：各层宽度的最大值 = 图的理论并行度。
pub fn parallel_width(r: &TopoResult) -> Result<usize, u16> {
    if r.cyclic || r.layers.is_empty() {
        return Err(SEG_BASE + 12);
    }
    let w = r.layers.iter().map(|l| l.len()).max().unwrap_or(0);
    if w == 0 {
        return Err(SEG_BASE + 13);
    }
    Ok(w)
}

/// 关键路径断言（独立函数面）：关键路径上任意相邻任务对必须
/// 存在依赖边或经中间层衔接（回溯自 DP 前驱，天然成立——此
/// 断言防御未来实现改写时破坏前驱链）。
pub fn critical_chain_ok(
    r: &TopoResult,
    deps: &[DepEdge],
) -> Result<(), u16> {
    if r.cyclic {
        return Err(SEG_BASE + 14);
    }
    if r.critical.len() < 2 {
        return Ok(());
    }
    for w in r.critical.windows(2) {
        let (a, b) = (w[0], w[1]);
        // 允许 a→b 直边，或 a 的完成时刻传递到 b（无直边时
        // 要求 a 在更浅层——层序保证前驱关系）。
        let direct = deps.iter().any(|d| d.from == a && d.to == b);
        let layer_a = r.layers.iter().position(|l| l.contains(&a));
        let layer_b = r.layers.iter().position(|l| l.contains(&b));
        match (layer_a, layer_b) {
            (Some(la), Some(lb)) if direct || la < lb => {}
            _ => return Err(SEG_BASE + 15),
        }
    }
    Ok(())
}

/// 帧下界断言（判据支撑）：关键路径总耗时是帧耗时的下界——
/// 任何调度都不可能快于关键路径（调度预算 < 关键路径 = 必然
/// 违约，提前暴露）。
pub fn frame_floor(r: &TopoResult, frame_budget_us: u64) -> Result<(), u16> {
    if r.cyclic {
        return Err(SEG_BASE + 16);
    }
    if frame_budget_us == 0 {
        return Err(SEG_BASE + 17);
    }
    if frame_budget_us < r.critical_us {
        return Err(SEG_BASE + 18);
    }
    Ok(())
}

/// 分层确定性断言（判据补充）：同图两次拓扑结果全等。
pub fn topo_deterministic(
    tasks: &[TaskCost],
    deps: &[DepEdge],
) -> Result<(), u16> {
    let a = topo_sort(tasks, deps)?;
    let b = topo_sort(tasks, deps)?;
    if a.layers == b.layers && a.critical == b.critical {
        Ok(())
    } else {
        Err(SEG_BASE + 19)
    }
}

impl TopoResult {
    /// 层宽序列（判据「并行度」逐层可见）。
    pub fn layer_widths(&self) -> Vec<usize> {
        self.layers.iter().map(|l| l.len()).collect()
    }

    /// 单层图检测（判据「并行度」反例）：全部层宽 ≤1 的图
    /// 是纯串行链（并行度枯竭——要么图退化要么任务切分过粗）。
    pub fn is_serial_chain(&self) -> bool {
        self.layers.iter().all(|l| l.len() <= 1)
    }

    /// 关键路径层跨度：关键路径首尾所在层号差（跨度 = 深度下界）。
    pub fn critical_span(&self) -> Result<usize, u16> {
        if self.cyclic || self.critical.is_empty() {
            return Err(SEG_BASE + 20);
        }
        let first = self.critical[0];
        let last = *self.critical.last().unwrap_or(&0);
        let lf = self
            .layers
            .iter()
            .position(|l| l.contains(&first))
            .ok_or(SEG_BASE + 21)?;
        let ll = self
            .layers
            .iter()
            .position(|l| l.contains(&last))
            .ok_or(SEG_BASE + 21)?;
        Ok(ll.saturating_sub(lf))
    }
}

/// 层序合规断言（判据「拓扑排序」强化面）：任何依赖边的
/// from 必须在比 to 更浅的层（同层依赖 = 违反分层语义）。
pub fn layer_order_ok(r: &TopoResult, deps: &[DepEdge]) -> Result<(), u16> {
    if r.cyclic {
        return Err(SEG_BASE + 22);
    }
    for d in deps {
        let lf = r
            .layers
            .iter()
            .position(|l| l.contains(&d.from))
            .ok_or(SEG_BASE + 23)?;
        let ll = r
            .layers
            .iter()
            .position(|l| l.contains(&d.to))
            .ok_or(SEG_BASE + 23)?;
        if lf >= ll {
            return Err(SEG_BASE + 24);
        }
    }
    Ok(())
}

/// 调度余量审计（判据「关键路径」调度面）：关键路径耗时占
/// 帧预算的千分比——占比过高（>80%）= 没有调度余量，任何
/// 抖动都会违约，必须提前预警。
pub fn schedule_headroom(r: &TopoResult, frame_budget_us: u64) -> Result<u32, u16> {
    frame_floor(r, frame_budget_us)?;
    let used = ((r.critical_us * 1000) / frame_budget_us) as u32;
    if used > 800 {
        return Err(SEG_BASE + 25);
    }
    Ok(used)
}

/// 零入度起点审计（判据支撑）：拓扑零层（入口层）非空是
/// Kahn 的前提，空首层但无环 = 实现错误。
pub fn entry_layer_ok(r: &TopoResult) -> Result<(), u16> {
    if r.cyclic {
        return Err(SEG_BASE + 26);
    }
    match r.layers.first() {
        Some(l) if !l.is_empty() => Ok(()),
        _ => Err(SEG_BASE + 27),
    }
}
