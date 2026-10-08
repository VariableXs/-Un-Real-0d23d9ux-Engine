//! CGPU-F0163 · 拓扑排序与关键路径（CGPU-B 域 · 批次 B01 · 目标 400 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F0163`
//!
//! **判据（锚点原文）**：排序正确性（有向无环验证）、关键路径与理论对拍、
//! 并行发射吞吐、环检测（有环即报错含环路径）、万节点性能。
//!
//! **职责定位（锚点原文）**：任务图的执行序推导：**Kahn 拓扑排序**（入度
//! 归零发射——并行发射所有就绪任务）、**关键路径计算**（最长路径=帧延迟
//! 下限，关键路径上的任务优先发射）、**关键路径标注**（关键任务标记——
//! 预算分配时优先保障）。
//!
//! # 一、为什么拓扑排序产出「波次」而不是「一条序」
//!
//! 排序的目的是发射：入度归零的任务**现在就能跑**，没有理由等前面的任务
//! 排完。故 Kahn 的每轮归零集是一个**波次**（[`LaunchPlan::waves`]）——
//! 同波任务互不依赖，可全并行发射；波次数=关键路径长度下限，最大波宽=
//! 并行发射吞吐上限。线性拓扑序只是波次的拼接副本——把并行信息压扁成
//! 序，等于把调度器最需要的信息扔掉（并行发射吞吐判据直接断言波宽）。
//!
//! # 二、关键路径为什么「=帧延迟下限」且用预算做权重
//!
//! 无论发射多并行，最后一波结束的时刻不可能早于最长依赖链的预算总和——
//! 这条链就是**帧延迟的下限**（关键路径与理论对拍判据的物理含义）。权重
//! 用节点的预算标签（`budget_us`）：预算是调度器对耗时的估计，沿估计最
//! 长的链回溯出的路径就是关键路径。关键路径上的任务**优先发射、预算优先
//! 保障**——它晚一微秒，整帧晚一微秒；非关键任务有松弛量（slack），晚一
//! 点不伤帧。
//!
//! # 三、环为什么「报错且给出环上路径」而不是静默跳过
//!
//! 有环图不可拓扑排序——被跳过的节点永远不发射，渲染静默缺一块；被丢弃
//! 的报错让调用方以为图是好的。故环检出（[`cycle_nodes`]）**列出环上全部
//! 节点**：F0161 的边三闸拦自环与重复边，本条拦跨任务环——两级防线之间
//! 的环（A→B→C→A）只有全图扫描能抓。报错必须可定位：拿到环节点清单，
//! 修图的人才知道从哪里断开。
//!
//! # 四、与相邻条的分工
//!
//! F0161 提供图与边闸；F0162 提供自动建边（本条的消费入口可直接吃其
//! `to_builder_edges` 产物）；F0164+ 的实际发射器消费本条的波次与标注。
//! 本条只管「**序怎么排、链在哪、环怎么报**」，不执行发射本身。
//!
//! **性能（锚点原文）**：Kahn O(节点+边)；关键路径 DP O(节点+边)；万节点
//! 链式与星形判据压测覆盖。
//!
//! # 五、线性序与松弛量——两个衍生消费面的契约
//!
//! **线性拓扑序**（[`topo_order`]）：波次的拼接副本。有些消费者（日志、
//! 逐项回放）确实只要一条序——给它们显性 API，免得各自手搓拼接出错。
//! 拼接顺序固定为波次序×波内出现序，同一个 [`LaunchPlan`] 拼出的线性序
//! 恒等（确定性账面，跨帧可对拍）。线性序只是**投影**：并行信息在投影
//! 里不可见，调度器仍然只认波次。
//!
//! **松弛量**（[`slacks`]）：非关键任务的「最晚可拖延量」——锚点原文
//! 「非关键任务有松弛量（slack），晚一点不伤帧」的可机检化。整数推导：
//! - `earliest_finish(v) = dist[v]`（含自身预算的最长前驱链——已在关键
//!   路径 DP 算出）；
//! - `tail[v] = bud[v] + max(tail[后继])`（v 到汇的最长链，倒序同款
//!   DP——拓扑序倒着松弛一遍）；
//! - `slack(v) = critical_total - tail[v] + bud[v] - dist[v]`（帧总预算
//!   减去「v 之前被迫等的最长链」再减「v 之后还要走的最长链（不含
//!   v）」——剩下的就是 v 可以自由支配的拖延量）。
//!
//! 自洽性三验（判据钉死）：关键链末端节点 slack=0；关键路径上任一节点
//! slack=0（定义使然）；短支中间节点 slack=关键链与含己最长链之差
//! （菱形语料手算 450 对拍）。全零松弛任务集**可能大于单条关键路径**
//! （并列关键链）——消费方要「全部零松弛任务」时拿 [`slacks`] 自己过滤，
//! [`LaunchPlan::critical_set`] 恒为关键路径本身（单链语义，预算保障的
//! 最小集合）。

use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use crate::svstar2::vcb01_framegraph as fg;

// ---------------------------------------------------------------------------
// 一、常量与错误码
// ---------------------------------------------------------------------------

/// 本项版本。
pub const TOPOPATH_VERSION: &str = "CB03-topopath-v1";

/// 图含环（不可拓扑排序——报错携带环上节点）。
pub const E_GRAPH_CYCLE: &str = "E_GRAPH_CYCLE";

/// 图为空（空图无发射计划——显性拒绝）。
pub const E_TOPO_EMPTY: &str = "E_TOPO_EMPTY";

// ---------------------------------------------------------------------------
// 二、环检测（Kahn 残留 = 环成员）
// ---------------------------------------------------------------------------

/// 环上节点清单（有环即报错**含环路径**——锚点判据）。
///
/// Kahn 归零扫描后入度仍大于零的节点必然在环上（或依赖环）——返回其
/// 句柄全集（升序）。无环图返回空表。
pub fn cycle_nodes(g: &fg::FrameGraph) -> Vec<u32> {
    let n = g.node_count();
    let mut indeg = vec![0usize; n];
    for e in g.edges_of().iter() {
        let (f, t) = (e.from as usize, e.to as usize);
        if f < n && t < n {
            indeg[t] += 1;
        }
    }
    let mut queue: Vec<usize> = Vec::new();
    for (i, d) in indeg.iter().enumerate() {
        if *d == 0 {
            queue.push(i);
        }
    }
    let mut qi = 0usize;
    while qi < queue.len() {
        let cur = queue[qi];
        qi += 1;
        for e in g.edges_of().iter() {
            if e.from as usize == cur {
                let t = e.to as usize;
                indeg[t] -= 1;
                if indeg[t] == 0 {
                    queue.push(t);
                }
            }
        }
    }
    let mut stuck: Vec<u32> = Vec::new();
    for (i, d) in indeg.iter().enumerate() {
        if *d > 0 {
            stuck.push(i as u32);
        }
    }
    stuck
}

// ---------------------------------------------------------------------------
// 三、Kahn 分层拓扑排序（波次=并行发射集）
// ---------------------------------------------------------------------------

/// 发射计划：波次集 + 关键路径标注（锚点数据结构）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LaunchPlan {
    /// 波次集：同波任务互不依赖，可全并行发射（波次序=执行轮次序）。
    pub waves: Vec<Vec<u32>>,
    /// 关键路径（节点句柄按执行序——帧延迟下限链）。
    pub critical_path: Vec<u32>,
    /// 帧延迟下限（关键路径预算总和，微秒）。
    pub critical_total_us: u64,
    /// 关键任务标记集（预算分配优先保障的节点）。
    pub critical_set: Vec<u32>,
}

impl LaunchPlan {
    /// 波次数（=关键路径长度下限的波次度量）。
    pub fn wave_count(&self) -> usize {
        self.waves.len()
    }

    /// 最大波宽（单波最大并行度——并行发射吞吐上限）。
    pub fn max_parallel(&self) -> usize {
        self.waves.iter().map(|w| w.len()).max().unwrap_or(0)
    }

    /// 是否关键任务（标注查询——预算保障的判定入口）。
    pub fn is_critical(&self, h: u32) -> bool {
        self.critical_set.contains(&h)
    }

    /// 总节点数（波次覆盖对账）。
    pub fn total_nodes(&self) -> usize {
        self.waves.iter().map(|w| w.len()).sum()
    }
}

/// Kahn 分层拓扑排序 + 关键路径标注（执行序推导主入口）。
///
/// 空图拒；有环拒（错误信息走 [`cycle_nodes`] 由调用方取证）。
pub fn plan(g: &fg::FrameGraph) -> Result<LaunchPlan, &'static str> {
    let n = g.node_count();
    if n == 0 {
        return Err(E_TOPO_EMPTY);
    }
    // 入度表 + 邻接（按节点分桶——O(节点+边)）。
    let mut indeg = vec![0usize; n];
    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); n];
    for e in g.edges_of().iter() {
        let (f, t) = (e.from as usize, e.to as usize);
        if f < n && t < n {
            adj[f].push(t);
            indeg[t] += 1;
        }
    }
    // Kahn 波次扫描。
    let mut waves: Vec<Vec<u32>> = Vec::new();
    let mut frontier: Vec<usize> = Vec::new();
    for (i, d) in indeg.iter().enumerate() {
        if *d == 0 {
            frontier.push(i);
        }
    }
    let mut covered = 0usize;
    while !frontier.is_empty() {
        let mut wave: Vec<u32> = Vec::new();
        let mut next: Vec<usize> = Vec::new();
        for &cur in frontier.iter() {
            wave.push(cur as u32);
            covered += 1;
        }
        for &cur in frontier.iter() {
            for &nxt in adj[cur].iter() {
                indeg[nxt] -= 1;
                if indeg[nxt] == 0 {
                    next.push(nxt);
                }
            }
        }
        waves.push(wave);
        frontier = next;
    }
    if covered < n {
        // 有环：判据要求报错含环路径——错误码固定，环清单走 cycle_nodes。
        return Err(E_GRAPH_CYCLE);
    }
    // 关键路径 DP（按预算权重）。
    // 平坦拓扑序 = 波次拼接（波次天然合法拓扑序：后波节点的前驱全在前波）。
    // 逐点「先收自己的预算，再向邻接出边松弛」——dist[v] 一旦定稿就是
    // 「从源到 v 的最长链预算和」，后继只增不减地继承（MAX 松弛不改自身）。
    let mut order: Vec<usize> = Vec::new();
    for w in waves.iter() {
        for &x in w.iter() {
            order.push(x as usize);
        }
    }
    // 预算标签先收集（判据侧可对拍：critical_total == Σbud(critical_path)）。
    let mut bud = vec![0u64; n];
    for (i, item) in bud.iter_mut().enumerate() {
        *item = g.get_node(i as u32).map_or(0u64, |s| s.budget_us as u64);
    }
    let mut dist = vec![0u64; n];
    let mut prev: Vec<Option<usize>> = vec![None; n];
    for &v in order.iter() {
        dist[v] += bud[v];
        for &nxt in adj[v].iter() {
            if dist[v] > dist[nxt] {
                dist[nxt] = dist[v];
                prev[nxt] = Some(v);
            }
        }
    }
    // 帧延迟下限=所有节点 dist 最大值；回溯关键路径。
    let mut best = 0usize;
    for (i, &d) in dist.iter().enumerate() {
        if d > dist[best] {
            best = i;
        }
    }
    let critical_total_us = dist[best];
    let mut path_rev: Vec<u32> = Vec::new();
    let mut cur = Some(best);
    while let Some(c) = cur {
        path_rev.push(c as u32);
        cur = prev[c];
    }
    path_rev.reverse();
    let critical_path = path_rev;
    // 关键集：回溯路径上的节点全部标注。
    let critical_set: Vec<u32> = critical_path.clone();
    Ok(LaunchPlan {
        waves,
        critical_path,
        critical_total_us,
        critical_set,
    })
}

// ---------------------------------------------------------------------------
// 四、读屏替述
// ---------------------------------------------------------------------------

/// 发射计划读屏单行（波次/并行度/关键路径；无资源明细）。
pub fn screen_line_plan(p: &LaunchPlan) -> String {
    format!(
        "发射计划：{} 波，最大并行 {}，关键路径 {} 节点，帧延迟下限 {} 微秒",
        p.wave_count(),
        p.max_parallel(),
        p.critical_path.len(),
        p.critical_total_us
    )
}

// ---------------------------------------------------------------------------
// 五、线性拓扑序（波次拼接的显性投影）
// ---------------------------------------------------------------------------

/// 线性拓扑序（波次拼接副本——确定性投影，跨帧可对拍）。
///
/// 拼接顺序固定：波次序 × 波内出现序。对同一 [`LaunchPlan`] 恒等；
/// 对同一图两次 `plan` 的计划若 [`PartialEq::eq`] 则线性序恒等。
/// 调度器发射仍只认波次——线性序只服务日志/回放/逐项核账类消费者。
pub fn topo_order(p: &LaunchPlan) -> Vec<u32> {
    let mut order: Vec<u32> = Vec::with_capacity(p.total_nodes());
    for w in p.waves.iter() {
        for &x in w.iter() {
            order.push(x);
        }
    }
    order
}

// ---------------------------------------------------------------------------
// 六、松弛量（非关键任务的最晚可拖延量——整数推导）
// ---------------------------------------------------------------------------

/// 松弛量表（句柄升序与图节点序一致；单位微秒）。
///
/// `slack(v) = critical_total - tail[v] + bud[v] - dist[v]`：
/// - `dist[v]`：最早完成时刻（含自身预算的最长前驱链——plan 内部已算）；
/// - `tail[v]`：v 到汇的最长链（含自身预算，倒序 DP）；
/// - 关键路径节点 slack 恒 0；零松弛集可能大于单条关键路径（并列链）。
///
/// 入参计划必须产自同一图（句柄有效域一致）——跨图混用是调用方错误，
/// 边界对不上时节点按预算 0 处理（不 panic，判据面有对拍兜底）。
pub fn slacks(g: &fg::FrameGraph, p: &LaunchPlan) -> Vec<u64> {
    let n = g.node_count();
    // 预算表（与 plan 内部同口径：越界/缺失按 0）。
    let mut bud = vec![0u64; n];
    for (i, item) in bud.iter_mut().enumerate() {
        *item = g.get_node(i as u32).map_or(0u64, |s| s.budget_us as u64);
    }
    // 邻接（正序出边）+ 反邻接（入边）。
    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut radj: Vec<Vec<usize>> = vec![Vec::new(); n];
    for e in g.edges_of().iter() {
        let (f, t) = (e.from as usize, e.to as usize);
        if f < n && t < n {
            adj[f].push(t);
            radj[t].push(f);
        }
    }
    // tail DP：沿线性拓扑序**倒序**松弛——tail[v] = bud[v] + max(tail[后继])。
    let order = topo_order(p);
    let mut tail = vec![0u64; n];
    for &h in order.iter().rev() {
        let v = h as usize;
        let mut best_succ = 0u64;
        for &s in adj[v].iter() {
            if tail[s] > best_succ {
                best_succ = tail[s];
            }
        }
        tail[v] = bud[v] + best_succ;
    }
    // dist DP：沿线性拓扑序**正序**重算（plan 内部的 dist 不出接口——
    // 在此独立重算，判据面可以对拍两张表的互洽性）。
    let mut dist = vec![0u64; n];
    for &h in order.iter() {
        let v = h as usize;
        let mut best_prev = 0u64;
        for &q in radj[v].iter() {
            if dist[q] > best_prev {
                best_prev = dist[q];
            }
        }
        dist[v] = best_prev + bud[v];
    }
    // slack = critical_total - tail + bud - dist（零浮点，饱和减法防下溢）。
    let total = p.critical_total_us;
    let mut out = vec![0u64; n];
    for (i, item) in out.iter_mut().enumerate() {
        let lhs = total.saturating_sub(tail[i]);
        let rhs = dist[i].saturating_sub(bud[i]);
        *item = lhs.saturating_sub(rhs);
    }
    out
}

/// 零松弛任务集（并列关键链全量——预算优先保障的完整集合）。
///
/// 升序返回；至少含关键路径全部节点（关键任务 slack 恒 0）。
pub fn zero_slack_set(g: &fg::FrameGraph, p: &LaunchPlan) -> Vec<u32> {
    let sk = slacks(g, p);
    let mut out: Vec<u32> = Vec::new();
    for (i, &s) in sk.iter().enumerate() {
        if s == 0 {
            out.push(i as u32);
        }
    }
    out
}

/// 松弛量读屏单行（关键路径保障 / 最松任务余量——人话可对账）。
pub fn screen_line_slack(g: &fg::FrameGraph, p: &LaunchPlan) -> String {
    let sk = slacks(g, p);
    let mut max_slack = 0u64;
    let mut zero_count = 0usize;
    for &s in sk.iter() {
        if s > max_slack {
            max_slack = s;
        }
        if s == 0 {
            zero_count += 1;
        }
    }
    format!(
        "松弛量：零松弛 {} 任务（含关键路径 {}），最大可拖延 {} 微秒",
        zero_count,
        p.critical_path.len(),
        max_slack
    )
}
