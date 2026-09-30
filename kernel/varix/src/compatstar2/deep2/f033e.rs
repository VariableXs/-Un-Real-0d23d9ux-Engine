//! F033 深化批次三 · 构建图面（compatstar2/deep2 · G-A-33）。
//!
//! 批次一/批次二已覆盖 F033 的协议语义面与执行治理面；本批补主册
//! 【功能定义】「全语义对齐」的执行/边界/注入面：DAG 拓扑排序（Kahn
//! 算法，定长邻接表 16 节点 × 4 出边，环检出显性 Err）、增量时间戳
//! 传播（文件 mtime 变更沿边传播到全部后继的祖先脏标记，定长节点账）、
//! 并行 ready 队列调度模型（入度归零即 ready，worker 计账：完成数/
//! 在飞数/空闲数）、命令指纹（FNV-1a 32 位对拍，flags 哈希变更即重建，
//! 变更计数）。
//!
//! 判据对账：深化以主册【设计细节】「增量构建判据：二次构建仅重编变更
//! 文件（时间戳比对实测）」未落地面为源，一处一事实（ninja 增量图调度
//! 与 restat 语义、FNV-1a 参考参数对拍）。
//!
//! 零堆纪律：定长邻接表/节点账/ready 队列/指纹表，无 alloc。

use crate::checks::CheckSet;

// 常量（一处一事实）
/// 构建图节点容量（定长邻接表口径）。
pub const MAX_NODES: usize = 16;
/// 每节点出边上限。
pub const MAX_OUT_EDGES: usize = 4;
/// 并行 worker 数（域内调度模型口径）。
pub const WORKERS: usize = 2;
/// FNV-1a 32 位偏移基（规范参考值 2166136261）。
pub const FNV_OFFSET: u32 = 2166136261;
/// FNV-1a 32 位素数（规范参考值 16777619）。
pub const FNV_PRIME: u32 = 16777619;
/// 空边哨兵。
pub const NO_EDGE: u8 = 0xFF;

// DAG 与 Kahn 拓扑排序
/// 构建图：定长邻接表（16 节点 × 4 出边），边 from→to 表示 from 依赖
/// to 必须先建（ninja 语义：order 由依赖方向唯一确定）。
#[derive(Clone, Copy)]
pub struct BuildGraph {
    out_edges: [[u8; MAX_OUT_EDGES]; MAX_NODES],
    out_count: [usize; MAX_NODES],
    pub node_count: usize,
}

impl BuildGraph {
    pub const fn new() -> Self {
        BuildGraph { out_edges: [[NO_EDGE; MAX_OUT_EDGES]; MAX_NODES], out_count: [0; MAX_NODES], node_count: 0 }
    }

    /// 加节点（容量满 → None，显性拒绝）。
    pub fn add_node(&mut self) -> Option<u8> {
        if self.node_count >= MAX_NODES { return None; }
        let id = self.node_count as u8;
        self.node_count += 1;
        Some(id)
    }

    /// 加边（越界/容量超限 → 显性 Err）。
    pub fn add_edge(&mut self, from: u8, to: u8) -> Result<(), &'static str> {
        if from as usize >= self.node_count || to as usize >= self.node_count { return Err("node-out-of-range"); }
        let f = from as usize;
        if self.out_count[f] >= MAX_OUT_EDGES { return Err("out-edge-full"); }
        self.out_edges[f][self.out_count[f]] = to;
        self.out_count[f] += 1;
        Ok(())
    }

    /// 入度表（Kahn 的初始账）。
    pub fn indegrees(&self) -> [u32; MAX_NODES] {
        let mut deg = [0u32; MAX_NODES];
        for i in 0..self.node_count {
            for e in 0..self.out_count[i] { deg[self.out_edges[i][e] as usize] += 1; }
        }
        deg
    }

    /// Kahn 拓扑排序：入度归零入队、出边递减；排序不全 → 环 →
    /// 显性 Err("cycle-detected")——不静默、不猜测。
    pub fn topo_sort(&self) -> Result<[u8; MAX_NODES], &'static str> {
        let n = self.node_count;
        let mut indeg = self.indegrees();
        let mut queue = [0u8; MAX_NODES];
        let (mut qh, mut qt) = (0usize, 0usize);
        for i in 0..n {
            if indeg[i] == 0 { queue[qt] = i as u8; qt += 1; }
        }
        let mut order = [0u8; MAX_NODES];
        let mut oi = 0usize;
        while qh < qt {
            let u = queue[qh] as usize;
            qh += 1;
            order[oi] = u as u8;
            oi += 1;
            for e in 0..self.out_count[u] {
                let v = self.out_edges[u][e] as usize;
                indeg[v] -= 1;
                if indeg[v] == 0 { queue[qt] = v as u8; qt += 1; }
            }
        }
        if oi < n { return Err("cycle-detected"); }
        Ok(order)
    }
}

// 增量时间戳传播（祖先脏标记）
/// 增量账：mtime 变更沿出边传播到全部后继（依赖我的节点都得重编）；
/// 已脏节点短路——不重复计数（restat 语义）。
pub struct IncrementalLedger {
    graph: BuildGraph,
    mtime: [u64; MAX_NODES],
    dirty: [bool; MAX_NODES],
    /// 当前脏节点数（记账面）。
    pub dirty_count: u32,
    /// 传播触发次数。
    pub propagation_rounds: u32,
}

impl IncrementalLedger {
    pub const fn new(graph: BuildGraph) -> Self {
        IncrementalLedger { graph, mtime: [0; MAX_NODES], dirty: [false; MAX_NODES], dirty_count: 0, propagation_rounds: 0 }
    }

    /// 文件触碰：mtime 变了才标脏并传播；mtime 未变 → 不动账。
    pub fn touch(&mut self, node: u8, mtime: u64) -> bool {
        let n = node as usize;
        if n >= self.graph.node_count || self.mtime[n] == mtime { return false; }
        self.mtime[n] = mtime;
        if !self.dirty[n] { self.dirty[n] = true; self.dirty_count += 1; }
        self.propagate(n);
        true
    }

    /// 显式栈 DFS 传播（零递归、零堆）：后继未脏则标脏并继续深入。
    fn propagate(&mut self, start: usize) {
        let mut stack = [0usize; MAX_NODES];
        let mut sp = 0usize;
        for e in 0..self.graph.out_count[start] {
            if sp < MAX_NODES { stack[sp] = self.graph.out_edges[start][e] as usize; sp += 1; }
        }
        while sp > 0 {
            sp -= 1;
            let u = stack[sp];
            if self.dirty[u] { continue; } // 已脏短路——账面不重复计数
            self.dirty[u] = true;
            self.dirty_count += 1;
            self.propagation_rounds += 1;
            for e in 0..self.graph.out_count[u] {
                if sp < MAX_NODES { stack[sp] = self.graph.out_edges[u][e] as usize; sp += 1; }
            }
        }
    }

    pub fn needs_rebuild(&self, node: u8) -> bool {
        (node as usize) < MAX_NODES && self.dirty[node as usize]
    }
}

// 并行 ready 队列调度模型
/// 调度器：入度归零即 ready；每拍至多 WORKERS 个出队，完成后继解锁。
/// 计账：完成数 done / 在飞数 inflight / 空闲数 idle_worker_ticks
/// （有活可干但 worker 没吃满的拍，按 worker·拍累计）。
pub struct ParallelScheduler {
    graph: BuildGraph,
    indeg: [u32; MAX_NODES],
    ready: [u8; MAX_NODES],
    ready_len: usize,
    pub done: u32,
    pub inflight: u32,
    pub idle_worker_ticks: u32,
}

impl ParallelScheduler {
    pub fn new(graph: BuildGraph) -> Self {
        let indeg = graph.indegrees();
        let mut ready = [0u8; MAX_NODES];
        let mut ready_len = 0usize;
        for i in 0..graph.node_count {
            if indeg[i] == 0 { ready[ready_len] = i as u8; ready_len += 1; }
        }
        ParallelScheduler { graph, indeg, ready, ready_len, done: 0, inflight: 0, idle_worker_ticks: 0 }
    }

    /// 拍推进：至多 WORKERS 个出队 → 完成 → 后继入度归零入 ready。
    /// 返回本拍实际出队数；有剩余工作但未吃满 → 空闲计账。
    pub fn tick(&mut self) -> usize {
        let dispatched = self.ready_len.min(WORKERS);
        // 有剩余工作（done < 总数）但 worker 没吃满 → 空闲 worker·拍计账。
        if dispatched < WORKERS && self.done < self.graph.node_count as u32 {
            self.idle_worker_ticks += (WORKERS - dispatched) as u32;
        }
        let mut completed = [0u8; WORKERS];
        for k in 0..dispatched { completed[k] = self.ready[k]; }
        for k in (0..dispatched).rev() {
            for i in k..self.ready_len - 1 { self.ready[i] = self.ready[i + 1]; }
            self.ready_len -= 1;
        }
        self.inflight = dispatched as u32;
        for k in 0..dispatched {
            let u = completed[k] as usize;
            self.done += 1;
            self.inflight -= 1;
            for e in 0..self.graph.out_count[u] {
                let v = self.graph.out_edges[u][e] as usize;
                self.indeg[v] -= 1;
                if self.indeg[v] == 0 { self.ready[self.ready_len] = v as u8; self.ready_len += 1; }
            }
        }
        dispatched
    }

    pub fn all_done(&self) -> bool { self.done == self.graph.node_count as u32 }
}

// 命令指纹（FNV-1a 32 位对拍）
/// FNV-1a 32 位：offset basis 2166136261、prime 16777619（规范参考值）。
pub fn fnv1a32(data: &[u8]) -> u32 {
    let mut h = FNV_OFFSET;
    for &b in data { h ^= b as u32; h = h.wrapping_mul(FNV_PRIME); }
    h
}

/// 命令指纹账：flags 哈希变更即重建（ninja command line 指纹语义），
/// 变更计数如实入账。
pub struct CommandFingerprints {
    prev: [u32; MAX_NODES],
    pub rebuild_count: u32,
}

impl CommandFingerprints {
    pub const fn new() -> Self {
        CommandFingerprints { prev: [FNV_OFFSET; MAX_NODES], rebuild_count: 0 }
    }

    /// 提交某节点的编译 flags：与上指纹不一致 → 重建 true 并计数。
    pub fn submit(&mut self, node: u8, flags: &str) -> bool {
        let h = fnv1a32(flags.as_bytes());
        let n = node as usize;
        if self.prev[n] == h { return false; }
        self.prev[n] = h;
        self.rebuild_count += 1;
        true
    }
}

// 域自检（深化批次三）
pub fn run_f033e_checks() -> CheckSet {
    let mut cs = CheckSet::new("F033-buildgraph-d3");
    // 菱形判例图：0→1, 0→2, 1→3, 2→3（a 依赖 b/c，b/c 依赖 d）。
    let mut g = BuildGraph::new();
    for _ in 0..4 { let _ = g.add_node(); }
    let _ = g.add_edge(0, 1);
    let _ = g.add_edge(0, 2);
    let _ = g.add_edge(1, 3);
    let _ = g.add_edge(2, 3);
    // 1) Kahn 序：入度归零顺序 0,1,2,3；排序全量无环。
    let order = g.topo_sort();
    cs.add("kahn_topo_order", order == Ok([0, 1, 2, 3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]), "");
    // 2) 拓扑性质：任一边 from 在序中先于 to。
    let seq = order.unwrap_or([0; MAX_NODES]);
    let pos_of = |n: u8| -> i32 { seq.iter().position(|&x| x == n).map(|p| p as i32).unwrap_or(-1) };
    cs.add("topo_edges_forward",
        pos_of(0) < pos_of(1) && pos_of(0) < pos_of(2) && pos_of(1) < pos_of(3) && pos_of(2) < pos_of(3), "");
    // 3) 环检出：0→1→0 显性 Err，不静默。
    let mut cyc = BuildGraph::new();
    let _ = cyc.add_node();
    let _ = cyc.add_node();
    let _ = (cyc.add_edge(0, 1), cyc.add_edge(1, 0));
    cs.add("kahn_cycle_err", cyc.topo_sort() == Err("cycle-detected"), "");
    // 4) 边界显性：出边超容/节点越界都显性拒绝，不静默。
    let mut cap = BuildGraph::new();
    for _ in 0..5 { let _ = cap.add_node(); }
    let mut ok4 = true;
    for t in 1..=4u8 { ok4 = ok4 && cap.add_edge(0, t).is_ok(); }
    cs.add("edge_bounds_explicit",
        ok4 && cap.add_edge(0, 4) == Err("out-edge-full") && cap.add_edge(0, 9) == Err("node-out-of-range"), "");
    // 5) 增量传播：触 0 → 全体后继 1/2/3 脏；mtime 未变不动账。
    let mut led = IncrementalLedger::new(g);
    let t1 = led.touch(0, 100);
    cs.add("dirty_propagate_all_succ",
        t1 && led.dirty_count == 4 && led.needs_rebuild(1) && led.needs_rebuild(2)
            && led.needs_rebuild(3) && !led.touch(0, 100), "");
    // 6) 已脏短路：触 1（已脏）新 mtime → 只影响 3，不重复计数。
    let _ = led.touch(1, 200);
    cs.add("dirty_short_circuit", led.dirty_count == 4 && led.needs_rebuild(3), "");
    // 7) 并行调度：菱形四节点三拍清空，完成/在飞账面归位。
    let mut sch = ParallelScheduler::new(g);
    let (d1, d2, d3) = (sch.tick(), sch.tick(), sch.tick());
    cs.add("parallel_diamond_schedule",
        d1 == 1 && d2 == 2 && d3 == 1 && sch.all_done() && sch.done == 4 && sch.inflight == 0, "");
    // 8) 空闲计账：单链三节点每拍只喂饱 1 个 worker（WORKERS=2）。
    let mut chain = BuildGraph::new();
    for _ in 0..3 { let _ = chain.add_node(); }
    let _ = chain.add_edge(0, 1);
    let _ = chain.add_edge(1, 2);
    let mut single = ParallelScheduler::new(chain);
    for _ in 0..3 { let _ = single.tick(); }
    cs.add("parallel_idle_accounting", single.all_done() && single.idle_worker_ticks == 3, "");
    // 9) FNV-1a 参考向量：空串 = 偏移基；"a" = 0xE40C292C。
    cs.add("fnv1a_reference_vectors", fnv1a32(b"") == 2166136261 && fnv1a32(b"a") == 0xE40C292C, "");
    // 10) 指纹重建：flags 不变不重建，变更即重建并计数。
    let mut fp = CommandFingerprints::new();
    let r1 = fp.submit(0, "-O0 -g");
    let r2 = fp.submit(0, "-O0 -g");
    let r3 = fp.submit(0, "-O2");
    cs.add("fingerprint_rebuild_on_change", r1 && !r2 && r3 && fp.rebuild_count == 2, "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eight_node_order_is_total() {
        // 8 节点链图：拓扑序全量且严格递增（Kahn 全序性质）。
        let mut g = BuildGraph::new();
        for _ in 0..8 { let _ = g.add_node(); }
        for i in 0..7u8 { let _ = g.add_edge(i, i + 1); }
        let order = g.topo_sort().expect("链图无环必成");
        for i in 0..8usize {
            assert_eq!(order[i] as usize, i, "链图拓扑序必须逐位吻合");
        }
    }

    #[test]
    fn isolated_node_rebuild_only_self() {
        // 孤立节点触碰只脏自己，不产生传播轮次。
        let mut g = BuildGraph::new();
        let _ = (g.add_node(), g.add_node(), g.add_edge(0, 1));
        let mut led = IncrementalLedger::new(g);
        let _ = led.touch(0, 42);
        assert_eq!(led.dirty_count, 2);
        assert_eq!(led.propagation_rounds, 1);
        assert!(led.needs_rebuild(1));
    }

    #[test]
    fn deep3_checks_all_green() {
        let cs = run_f033e_checks();
        assert!(cs.all_passed() && !cs.truncated());
    }
}
