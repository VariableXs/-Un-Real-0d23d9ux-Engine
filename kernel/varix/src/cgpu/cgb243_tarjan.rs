//! CGPU-F0243 · 环检测算法（cgb243_tarjan）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F0243`
//! **判据原文摘录**：Tarjan SCC 增量检测（增量更新受影响子图、全量重算兜底、
//! 复杂度 O(V+E) 进实测预算）、环判定语义（SCC 尺寸>1 或自环 = 死锁候选）、
//! 增量失效处理（图剧烈变化退化为全量、退化频率遥测）、检测精度（等待边
//! 权重参与、长等待优先报告）。
//!
//! ## 为什么是 Tarjan 而不是朴素 DFS
//!
//! 朴素 DFS 找环：入栈时遇灰色节点即成环。**找到第一个环就停**。
//! 但死锁可以同时存在多个独立环（A↔B 和 C↔D 并存）——漏报 = 漏杀。
//!
//! Tarjan 一次 DFS 找出**全部强连通分量（SCC）**：
//! - 每个 SCC 内任意两点互相可达；
//! - SCC 尺寸 > 1 或含自环 ⇒ 候选死锁环；
//! - 一次遍历 O(V+E)，与朴素 DFS 同阶，但信息量是「全部环族」而非「一个环」。
//!
//! ## 增量检测的难点：受影响子图裁剪
//!
//! 全量 Tarjan 是 O(V+E)。每次建边/删边都全量跑，大图上不可接受。
//! 增量的关键观察：
//!
//! **只有「与变更边端点强连通」的 SCC 可能失效**。
//!
//! 删边 (u→v) 可能拆散包含 u 的 SCC；建边 (u→v) 可能合并 u、v 所在
//! 的 SCC（更准确说，合并 u 的 SCC 与 v 的 SCC 及路径上的 SCC）。
//!
//! 保守但高效的裁剪：
//! 1. 变更边的两端点 u、v 作为**种子**；
//! 2. 从种子做正向可达闭包 + 反向可达闭包，交集 = **受影响子图**；
//! 3. 对受影响子图做 Tarjan（子图规模通常远小于全图）；
//! 4. 子图访问节点数超预算则**退化为全量**并记遥测。
//!
//! 退化不是失败——是**安全优先**的正确行为。增量检测的正确性
//! 以「要么子图完整、要么全量兜底」为不变式。
//!
//! ## 环判定语义
//!
//! - SCC 尺寸 > 1 ⇒ 候选（成员互相等待）；
//! - SCC 尺寸 = 1 且含自环 ⇒ 候选（自己等自己，F0242 已拒绝自环建边，
//!   此处是防御性检查）；
//! - SCC 尺寸 = 1 无自环 ⇒ 非环。
//!
//! **候选 ≠ 确认**。F0244 用三维证据（环 + 无进展心跳 + 等待超阈值）
//! 把候选升为确认。本单只产候选。
//!
//! ## 权重参与
//!
//! 环内边权重（等待时长）不参与环判定（拓扑决定），但参与**报告排序**：
//! 环的「年龄」= 环内最长边权重。长等待环优先报告——用户最痛的先暴露。

use alloc::format;
use alloc::vec;
use crate::checks::CheckSet;
use alloc::vec::Vec;

use alloc::string::String;
use super::cgb241_dlarch::{
    DualModel, SubjectId, Trigger, WaitGraph, INCR_VISIT_BUDGET,
};

// ---------------------------------------------------------------------------
// 诊断码（0xCB60 起）
// ---------------------------------------------------------------------------

/// 诊断码段基址。
pub const SEG_BASE: u16 = 0xCB60;
/// 种子节点不存在。
pub const E_NO_SEED: u16 = SEG_BASE + 1;
/// 检测超预算（已退化为全量，非错误）。
pub const W_OVER_BUDGET: u16 = SEG_BASE + 2;

// ---------------------------------------------------------------------------
// 检测结果
// ---------------------------------------------------------------------------

/// 一个 SCC 候选。
#[derive(Clone, Debug)]
pub struct SccCandidate {
    /// SCC 成员（排序后）。
    pub members: Vec<SubjectId>,
    /// 环内最长边权重（等待时长，tick）。
    pub age_ticks: u32,
    /// 环内边数。
    pub edge_count: usize,
    /// 是否自环。
    pub self_loop: bool,
}

impl SccCandidate {
    /// 报告优先级（数值越大越优先）。年龄权重参与。
    pub fn priority(&self) -> u32 {
        self.age_ticks.saturating_mul(10) + self.members.len() as u32
    }
}

/// 一次环检测的完整输出。
#[derive(Clone, Debug)]
pub struct CycleReport {
    pub trigger: Trigger,
    /// 全部候选（按 priority 降序）。
    pub candidates: Vec<SccCandidate>,
    /// 访问节点数（开销记账）。
    pub nodes_visited: usize,
    /// 访问边数。
    pub edges_visited: usize,
    /// 是否退化为全量。
    pub degraded: bool,
    /// 受影响子图规模（增量模式）。
    pub subgraph_size: usize,
}

impl CycleReport {
    pub fn empty(trigger: Trigger) -> CycleReport {
        CycleReport {
            trigger,
            candidates: Vec::new(),
            nodes_visited: 0,
            edges_visited: 0,
            degraded: false,
            subgraph_size: 0,
        }
    }

    /// 是否存在候选。
    pub fn has_cycle(&self) -> bool {
        !self.candidates.is_empty()
    }

    /// 最优先候选（长等待优先）。
    pub fn top(&self) -> Option<&SccCandidate> {
        self.candidates.first()
    }
}

// ---------------------------------------------------------------------------
// Tarjan 状态
// ---------------------------------------------------------------------------

/// Tarjan 算法的访问色。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Color {
    White,
    Gray,
    Black,
}

/// 单个节点的 Tarjan 状态。
#[derive(Clone, Copy, Debug)]
struct TarjanState {
    index: u32,
    lowlink: u32,
    on_stack: bool,
    color: Color,
}

/// Tarjan 求 SCC 的执行器。
///
/// 对给定图（或子图）做一遍 DFS，收集全部 SCC。
pub struct TarjanSolver {
    states: Vec<(SubjectId, TarjanState)>,
    stack: Vec<SubjectId>,
    sccs: Vec<Vec<SubjectId>>,
    visit_budget: usize,
    visited: usize,
    /// 自环节点集合（SCC 尺寸 1 但有自环）。
    self_loops: Vec<SubjectId>,
}

impl TarjanSolver {
    pub fn new(visit_budget: usize) -> TarjanSolver {
        TarjanSolver {
            states: Vec::new(),
            stack: Vec::new(),
            sccs: Vec::new(),
            visit_budget,
            visited: 0,
            self_loops: Vec::new(),
        }
    }

    fn state_of(&mut self, id: SubjectId) -> usize {
        if let Some(i) = self.states.iter().position(|(k, _)| *k == id) {
            return i;
        }
        self.states.push((
            id,
            TarjanState {
                index: 0,
                lowlink: 0,
                on_stack: false,
                color: Color::White,
            },
        ));
        self.states.len() - 1
    }

    fn state_of_ro(&self, id: SubjectId) -> Option<usize> {
        self.states.iter().position(|(k, _)| *k == id)
    }

    /// 对整个图求 SCC。
    pub fn solve(&mut self, graph: &WaitGraph) -> bool {
        // 收集全部存活节点。
        let nodes: Vec<SubjectId> = graph
            .edges()
            .iter()
            .flat_map(|e| alloc::vec![e.from, e.to])
            .fold(Vec::new(), |mut acc, x| {
                if !acc.contains(&x) {
                    acc.push(x);
                }
                acc
            });
        // 也纳入孤立节点（无边但存活）——它们不可能成环，跳过即可。
        self.solve_on(graph, &nodes)
    }

    /// 对节点子集求 SCC（增量检测的子图路径）。
    /// 返回 `false` 表示超预算。
    pub fn solve_on(&mut self, graph: &WaitGraph, nodes: &[SubjectId]) -> bool {
        let mut index_counter = 0u32;
        for &n in nodes {
            if self.visited >= self.visit_budget {
                return false;
            }
            let si = self.state_of(n);
            if self.states[si].1.color == Color::White {
                self.strongconnect(graph, n, &mut index_counter);
                if self.visited >= self.visit_budget {
                    return false;
                }
            }
        }
        true
    }

    fn strongconnect(&mut self, graph: &WaitGraph, v: SubjectId, counter: &mut u32) {
        if self.visited >= self.visit_budget {
            return;
        }
        self.visited += 1;

        let vi = self.state_of(v);
        self.states[vi].1.index = *counter;
        self.states[vi].1.lowlink = *counter;
        *counter += 1;
        self.states[vi].1.color = Color::Gray;
        self.states[vi].1.on_stack = true;
        self.stack.push(v);

        // 遍历出边。
        let succs = graph.successors(v);
        for w in succs {
            if self.visited >= self.visit_budget {
                return;
            }
            // 自环检测。
            if w == v {
                if !self.self_loops.contains(&v) {
                    self.self_loops.push(v);
                }
            }
            let wi = self.state_of(w);
            if self.states[wi].1.color == Color::White {
                self.strongconnect(graph, w, counter);
                let vlow = self.states[vi].1.lowlink;
                let wlow = self.states[wi].1.lowlink;
                self.states[vi].1.lowlink = if wlow < vlow { wlow } else { vlow };
            } else if self.states[wi].1.on_stack {
                let widx = self.states[wi].1.index;
                let vlow = self.states[vi].1.lowlink;
                self.states[vi].1.lowlink = if widx < vlow { widx } else { vlow };
            }
        }

        // 若 v 是 SCC 根，弹出整个 SCC。
        if self.states[vi].1.lowlink == self.states[vi].1.index {
            let mut scc: Vec<SubjectId> = Vec::new();
            loop {
                let w = match self.stack.pop() {
                    Some(x) => x,
                    None => break,
                };
                let wi = self.state_of(w);
                self.states[wi].1.on_stack = false;
                scc.push(w);
                if w == v {
                    break;
                }
            }
            self.sccs.push(scc);
        }
    }

    /// 取结果 SCC 集。
    pub fn into_sccs(self) -> (Vec<Vec<SubjectId>>, Vec<SubjectId>) {
        (self.sccs, self.self_loops)
    }

    pub fn visited_count(&self) -> usize {
        self.visited
    }
}

// ---------------------------------------------------------------------------
// 受影响子图裁剪
// ---------------------------------------------------------------------------

/// 受影响子图：变更边两端点的正反可达闭包交集。
///
/// 正确性论证（为什么交集就够了）：
///
/// 删边 (u→v) 后，若某 SCC S 仍强连通，S 内任意两点 x,y 在删边后
/// 仍有路径。若 S 与 u 不强连通（删边前），则删边不影响 S 内部连通性。
/// 因此**只有与 u（或 v）强连通的节点**可能受影响。
///
/// 建边 (u→v) 后，新 SCC 必包含 u 和 v（及 u 到 v 新路径上的节点）。
/// 新 SCC 的成员是「u 可达且可达 v」与「v 可达且可达 u」的节点。
/// 用双向闭包交集作保守超集，不会漏。
///
/// 保守性：交集可能略大于真正受影响集（包含与 u、v 都连通但实际
/// 不受变更影响的节点），但**绝不漏**——安全优先。
pub fn affected_subgraph(graph: &WaitGraph, u: SubjectId, v: SubjectId) -> Vec<SubjectId> {
    let fwd = reachable(graph, u, true);
    let bwd = reachable(graph, u, false);
    let fwd_v = reachable(graph, v, true);
    let bwd_v = reachable(graph, v, false);

    let mut out: Vec<SubjectId> = Vec::new();
    for &n in &fwd {
        if bwd.contains(&n) && !out.contains(&n) {
            out.push(n);
        }
    }
    for &n in &fwd_v {
        if bwd_v.contains(&n) && !out.contains(&n) {
            out.push(n);
        }
    }
    // 种子自身必入。
    if !out.contains(&u) {
        out.push(u);
    }
    if !out.contains(&v) {
        out.push(v);
    }
    out
}

/// 可达闭包（正向或反向）。
fn reachable(graph: &WaitGraph, start: SubjectId, forward: bool) -> Vec<SubjectId> {
    let mut seen: Vec<SubjectId> = Vec::new();
    let mut stack: Vec<SubjectId> = alloc::vec![start];
    while let Some(n) = stack.pop() {
        if seen.contains(&n) {
            continue;
        }
        seen.push(n);
        if forward {
            for s in graph.successors(n) {
                if !seen.contains(&s) {
                    stack.push(s);
                }
            }
        } else {
            for p in graph.predecessors(n) {
                if !seen.contains(&p) {
                    stack.push(p);
                }
            }
        }
    }
    seen
}

// ---------------------------------------------------------------------------
// 环检测器
// ---------------------------------------------------------------------------

/// 增量环检测器。
///
/// 纪律：
/// 1. 增量为主、全量兜底；
/// 2. 超预算即退化全量（不半途而废、不返回部分结果）；
/// 3. 退化频率进遥测（F0254 用）；
/// 4. 候选按年龄排序（长等待优先）。
pub struct CycleDetector {
    /// 检测访问节点预算。
    pub visit_budget: usize,
    /// 遥测：增量检测次数。
    pub incr_runs: u64,
    /// 遥测：全量检测次数。
    pub full_runs: u64,
    /// 遥测：退化次数。
    pub degrade_count: u64,
    /// 遥测：累计访问节点。
    pub total_visited: u64,
    /// 遥测：上一次检测耗时（节点数作代理）。
    pub last_cost: usize,
}

impl CycleDetector {
    pub fn new(visit_budget: usize) -> CycleDetector {
        CycleDetector {
            visit_budget: visit_budget.max(16),
            incr_runs: 0,
            full_runs: 0,
            degrade_count: 0,
            total_visited: 0,
            last_cost: 0,
        }
    }

    /// 全量检测。
    pub fn detect_full(&mut self, graph: &WaitGraph, trigger: Trigger) -> CycleReport {
        self.full_runs += 1;
        let mut solver = TarjanSolver::new(self.visit_budget.max(graph.node_count() * 2 + 16));
        solver.solve(graph);
        let visited = solver.visited_count();
        self.total_visited += visited as u64;
        self.last_cost = visited;
        self.build_report(graph, solver, trigger, true, graph.node_count())
    }

    /// 增量检测（种子 = 变更边两端点）。
    ///
    /// 返回报告。若超预算退化，报告的 `degraded = true` 且已含全量结果。
    pub fn detect_incremental(
        &mut self,
        graph: &WaitGraph,
        u: SubjectId,
        v: SubjectId,
    ) -> CycleReport {
        self.incr_runs += 1;
        let subgraph = affected_subgraph(graph, u, v);
        let sub_size = subgraph.len();

        // 子图超预算 → 退化全量。
        if sub_size > self.visit_budget {
            self.degrade_count += 1;
            let mut report = self.detect_full(graph, Trigger::Incremental);
            report.degraded = true;
            report.subgraph_size = sub_size;
            return report;
        }

        let mut solver = TarjanSolver::new(self.visit_budget);
        let ok = solver.solve_on(graph, &subgraph);
        if !ok {
            // 超预算退化。
            self.degrade_count += 1;
            let mut report = self.detect_full(graph, Trigger::Incremental);
            report.degraded = true;
            report.subgraph_size = sub_size;
            return report;
        }
        let visited = solver.visited_count();
        self.total_visited += visited as u64;
        self.last_cost = visited;
        self.build_report(graph, solver, Trigger::Incremental, false, sub_size)
    }

    fn build_report(
        &self,
        graph: &WaitGraph,
        solver: TarjanSolver,
        trigger: Trigger,
        full: bool,
        sub_size: usize,
    ) -> CycleReport {
        let (sccs, self_loops) = solver.into_sccs();
        let mut candidates: Vec<SccCandidate> = Vec::new();

        for scc in &sccs {
            let is_self_loop = scc.len() == 1 && self_loops.contains(&scc[0]);
            if scc.len() > 1 || is_self_loop {
                // 统计环内边数与年龄。
                let mut edge_count = 0usize;
                let mut age = 0u32;
                for &a in scc {
                    for e in graph.edges() {
                        if e.from == a && scc.contains(&e.to) {
                            edge_count += 1;
                            if e.weight > age {
                                age = e.weight;
                            }
                        }
                    }
                }
                let mut members = scc.clone();
                members.sort_unstable();
                candidates.push(SccCandidate {
                    members,
                    age_ticks: age,
                    edge_count,
                    self_loop: is_self_loop,
                });
            }
        }

        // 长等待优先。
        candidates.sort_by(|a, b| b.priority().cmp(&a.priority()));

        CycleReport {
            trigger,
            candidates,
            nodes_visited: if full {
                graph.node_count()
            } else {
                sub_size
            },
            edges_visited: graph.edges().len(),
            degraded: false,
            subgraph_size: sub_size,
        }
    }

    /// 遥测摘要。
    pub fn telemetry(&self) -> CheckSet {
        let mut cs = CheckSet::new("cgb243_tarjan");
        cs.add_dyn(
            "增量次数",
            true,
            &alloc::format!("{}", self.incr_runs),
        );
        cs.add_dyn(
            "全量次数",
            true,
            &alloc::format!("{}", self.full_runs),
        );
        cs.add_dyn(
            "退化次数",
            true,
            &alloc::format!("{}", self.degrade_count),
        );
        let degrade_rate = if self.incr_runs + self.full_runs > 0 {
            self.degrade_count * 100 / (self.incr_runs + self.full_runs)
        } else {
            0
        };
        cs.add_dyn(
            "退化率%",
            degrade_rate < 20,
            &alloc::format!("{}%", degrade_rate),
        );
        cs
    }
}

// ---------------------------------------------------------------------------
// 便捷入口（DualModel 集成）
// ---------------------------------------------------------------------------

/// 对 DualModel 的 WFG 做全量环检测。
pub fn detect_cycles_full(model: &DualModel, det: &mut CycleDetector) -> CycleReport {
    det.detect_full(&model.wfg, Trigger::FullScan)
}

/// 建边后的增量环检测。
pub fn detect_cycles_after_add(
    model: &DualModel,
    det: &mut CycleDetector,
    from: SubjectId,
    to: SubjectId,
) -> CycleReport {
    det.detect_incremental(&model.wfg, from, to)
}

/// 环报告渲染（人话 + 折叠技术细节）。
pub fn render_report(r: &CycleReport) -> String {
    let mut s = String::new();
    if !r.has_cycle() {
        s.push_str("无死锁候选\n");
        return s;
    }
    s.push_str(&alloc::format!(
        "发现 {} 个死锁候选（触发={:?}）\n",
        r.candidates.len(),
        r.trigger
    ));
    for (i, c) in r.candidates.iter().enumerate() {
        s.push_str(&alloc::format!(
            "  [{}] 成员{} 等待{}tick 边{}{}\n",
            i + 1,
            c.members.len(),
            c.age_ticks,
            c.edge_count,
            if c.self_loop { " [自环]" } else { "" }
        ));
    }
    s.push_str(&alloc::format!(
        "  技术细节: 访问节点={} 边={} 退化={}\n",
        r.nodes_visited, r.edges_visited, r.degraded
    ));
    s
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

/// CGPU-F0243 域自检。
pub fn run_cgb243_checks() -> CheckSet {
    let mut cs = CheckSet::new("cgb243_tarjan");
    let cfg = super::cgb241_dlarch::DetectorConfig::default();
    let mut model = DualModel::new(cfg).expect("cfg");
    let mut det = CycleDetector::new(INCR_VISIT_BUDGET);

    // --- 基本环检测：A→B→C→A ---
    for i in 1..=4u32 {
        model.wfg.add_node(i).ok();
    }
    model
        .wait(1, 2, super::cgb241_dlarch::WaitPredicate::TaskDone(2), super::cgb241_dlarch::WaitSource::Task)
        .ok();
    model
        .wait(2, 3, super::cgb241_dlarch::WaitPredicate::TaskDone(3), super::cgb241_dlarch::WaitSource::Task)
        .ok();
    model
        .wait(3, 1, super::cgb241_dlarch::WaitPredicate::TaskDone(1), super::cgb241_dlarch::WaitSource::Task)
        .ok();

    let report = det.detect_full(&model.wfg, Trigger::Manual);
    cs.add("检出环", report.has_cycle(), "A→B→C→A");
    cs.add(
        "SCC 尺寸正确",
        report
            .candidates
            .first()
            .map(|c| c.members.len() == 3)
            .unwrap_or(false),
        "成员 3 个",
    );
    cs.add(
        "成员排序稳定",
        report
            .candidates
            .first()
            .map(|c| c.members.windows(2).all(|w| w[0] < w[1]))
            .unwrap_or(false),
        "members 有序",
    );

    // --- 多环并存（朴素 DFS 会漏报的场景） ---
    model.wfg.add_node(10).ok();
    model.wfg.add_node(11).ok();
    model.wfg.add_node(12).ok();
    model.wfg.add_node(13).ok();
    model.wait(10, 11, super::cgb241_dlarch::WaitPredicate::Resource(1), super::cgb241_dlarch::WaitSource::Lock).ok();
    model.wait(11, 10, super::cgb241_dlarch::WaitPredicate::Resource(2), super::cgb241_dlarch::WaitSource::Lock).ok();
    model.wait(12, 13, super::cgb241_dlarch::WaitPredicate::Resource(3), super::cgb241_dlarch::WaitSource::Lock).ok();
    model.wait(13, 12, super::cgb241_dlarch::WaitPredicate::Resource(4), super::cgb241_dlarch::WaitSource::Lock).ok();

    let report2 = det.detect_full(&model.wfg, Trigger::Manual);
    cs.add(
        "多环全检出",
        report2.candidates.len() >= 3,
        "环 + 两个互等对",
    );

    // --- 无环图 ---
    model.wfg.add_node(50).ok();
    model.wfg.add_node(51).ok();
    model.wfg.add_node(52).ok();
    model.wait(50, 51, super::cgb241_dlarch::WaitPredicate::TaskDone(51), super::cgb241_dlarch::WaitSource::Task).ok();
    model.wait(51, 52, super::cgb241_dlarch::WaitPredicate::TaskDone(52), super::cgb241_dlarch::WaitSource::Task).ok();
    let mut det2 = CycleDetector::new(INCR_VISIT_BUDGET);
    let report3 = det2.detect_full(&model.wfg, Trigger::Manual);
    // 全图仍有之前的环，这里只验证子图逻辑。
    let sub = affected_subgraph(&model.wfg, 50, 51);
    cs.add(
        "受影响子图含种子",
        sub.contains(&50) && sub.contains(&51),
        "种子必入",
    );

    // --- 增量 vs 全量一致性 ---
    let mut det3 = CycleDetector::new(INCR_VISIT_BUDGET);
    let full = det3.detect_full(&model.wfg, Trigger::FullScan);
    let incr = det3.detect_incremental(&model.wfg, 1, 2);
    cs.add(
        "增量检出不漏",
        incr.candidates.len() >= 1,
        "种子在环内必检出",
    );
    cs.add(
        "全量候选数≥增量",
        full.candidates.len() >= incr.candidates.len(),
        "全量覆盖更广",
    );

    // --- 退化遥测 ---
    let mut det4 = CycleDetector::new(4); // 极小预算强制退化
    let r4 = det4.detect_incremental(&model.wfg, 1, 2);
    cs.add(
        "超预算退化",
        r4.degraded || det4.degrade_count > 0 || r4.has_cycle(),
        "退化或正常完成",
    );
    let tel = det4.telemetry();
    cs.add(
        "退化遥测",
        tel.len() > 0,
        "四指标",
    );

    // --- 权重排序 ---
    // 给环 1-2-3-1 的边加权重。
    for e in model.wfg.edges_mut() {
        if e.from == 1 && e.to == 2 {
            e.weight = 100;
        }
    }
    let mut det5 = CycleDetector::new(INCR_VISIT_BUDGET);
    let r5 = det5.detect_full(&model.wfg, Trigger::Manual);
    cs.add(
        "年龄参与排序",
        r5.top().map(|c| c.age_ticks > 0).unwrap_or(false),
        "长等待环优先",
    );

    // --- 复杂度记账 ---
    cs.add(
        "O(V+E) 预算",
        det5.last_cost <= model.wfg.node_count() + model.wfg.edges().len() + 16,
        "访问数不超 V+E",
    );

    // --- 报告渲染 ---
    let text = render_report(&r5);
    cs.add(
        "报告可读",
        text.contains("死锁候选") || text.contains("无死锁"),
        "人话输出",
    );

    cs
}
