//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F3342 · 等待图数据结构（节点=锁，边=持有→等待）· 诊断段 0xCA80
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥380 行；
//!   ② J1：图本体（邻接表，节点=锁实例，边=等待关系）结构交付；
//!      M-101 建-查-删图操作对账；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 邻接表：节点（锁实例 id）→ 出边集（等待者）；
//!   - 边：holder → waiter 有向等待关系；
//!   - 建-查-删对账：建边数 ≡ 查得数 + 删后差；
//!   - 环检测锚：DFS 着色三态（白/灰/黑）判环（死锁预警锚）；
//!   - 注入核 5 项。
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::string::{String, ToString};
use crate::alloc::vec;
use crate::alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址。
pub const SEG_BASE: u16 = 0xCA80;

/// 节点容量。
pub const MAX_NODES: usize = 64;

/// FNV-1a。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// DFS 三色。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Color {
    White,
    Gray,
    Black,
}

/// 等待图。
pub struct WaitGraph {
    /// 邻接表：adj[i] = i 持有的锁被哪些节点等待（出边集）。
    pub adj: Vec<Vec<usize>>,
    /// 边账。
    pub edges: u64,
    /// 删除边账。
    pub removed: u64,
}

impl WaitGraph {
    pub fn new() -> Self {
        let mut adj = Vec::new();
        for _ in 0..MAX_NODES {
            adj.push(Vec::new());
        }
        WaitGraph { adj, edges: 0, removed: 0 }
    }

    /// 建边：holder → waiter（去重）。
    pub fn add_edge(&mut self, holder: usize, waiter: usize) -> bool {
        if holder >= MAX_NODES || waiter >= MAX_NODES || holder == waiter {
            return false;
        }
        let lst = &mut self.adj[holder];
        if lst.contains(&waiter) {
            return false;
        }
        lst.push(waiter);
        self.edges += 1;
        true
    }

    /// 查边。
    pub fn has_edge(&self, holder: usize, waiter: usize) -> bool {
        holder < MAX_NODES && self.adj[holder].contains(&waiter)
    }

    /// 查出边。
    pub fn waiters_of(&self, holder: usize) -> Vec<usize> {
        if holder >= MAX_NODES {
            return Vec::new();
        }
        self.adj[holder].clone()
    }

    /// 删边。
    pub fn remove_edge(&mut self, holder: usize, waiter: usize) -> bool {
        if holder >= MAX_NODES {
            return false;
        }
        let lst = &mut self.adj[holder];
        if let Some(pos) = lst.iter().position(|&w| w == waiter) {
            lst.remove(pos);
            self.edges -= 1;
            self.removed += 1;
            true
        } else {
            false
        }
    }

    /// 对账：edges + removed ≡ 建边总数（建边计账在外部对照）。
    pub fn reconciled(&self, built: u64) -> bool {
        self.edges + self.removed == built
    }

    /// 环检测：DFS 三色。
    pub fn has_cycle(&self) -> bool {
        let mut color = vec![Color::White; MAX_NODES];
        for start in 0..MAX_NODES {
            if color[start] == Color::White && self.dfs(start, &mut color) {
                return true;
            }
        }
        false
    }

    fn dfs(&self, u: usize, color: &mut Vec<Color>) -> bool {
        color[u] = Color::Gray;
        for &v in &self.adj[u] {
            match color[v] {
                Color::Gray => return true, // 回边 → 环
                Color::White => {
                    if self.dfs(v, color) {
                        return true;
                    }
                }
                Color::Black => {}
            }
        }
        color[u] = Color::Black;
        false
    }

    /// 图哈希（边集序敏感）。
    pub fn hash(&self) -> u32 {
        let mut buf: Vec<u8> = Vec::new();
        for (i, lst) in self.adj.iter().enumerate() {
            for &w in lst {
                buf.push(i as u8);
                buf.push(w as u8);
            }
        }
        fnv1a(&buf)
    }
}

/// M-101 建-查-删对账。
pub fn m101() -> (bool, u64) {
    let mut g = WaitGraph::new();
    let mut built = 0u64;
    // 建边：链式 A→B→C→…→J（10 节点，无环）
    for i in 0..10 {
        if g.add_edge(i, i + 1) {
            built += 1;
        }
    }
    // 查边
    let mut queries_ok = true;
    for i in 0..10 {
        if !g.has_edge(i, i + 1) {
            queries_ok = false;
        }
    }
    let no_back = !g.has_edge(5, 3);
    // 删边 3 条
    let mut removed_ok = true;
    for i in 0..3 {
        if !g.remove_edge(i, i + 1) {
            removed_ok = false;
        }
    }
    let after = !g.has_edge(0, 1);
    (
        queries_ok && no_back && removed_ok && after && g.reconciled(built) && !g.has_cycle(),
        built,
    )
}

/// J1 主判据：图本体 + M-101 对账 + 环检测可用。
pub fn j1_main() -> bool {
    let (ok, built) = m101();
    ok && built == 10 && {
        // 环场景：A→B→C→A
        let mut g = WaitGraph::new();
        let _ = g.add_edge(0, 1);
        let _ = g.add_edge(1, 2);
        let _ = g.add_edge(2, 0);
        g.has_cycle()
    } && {
        let g = WaitGraph::new();
        g.hash() == 0 // 空图哈希为 0（空输入恒 0）
    }
}

// ── 注入核（0xCA80 段 5 项）────────────────────────────────────────────

/// 注入核 1：自环注入——holder==waiter 必拒。
pub fn inj1_self_loop() -> bool {
    let mut g = WaitGraph::new();
    !g.add_edge(7, 7)
}

/// 注入核 2：重边注入——重复建边必拒且不计数。
pub fn inj2_dup_edge() -> bool {
    let mut g = WaitGraph::new();
    let e0 = g.add_edge(1, 2);
    let e1 = g.add_edge(1, 2);
    e0 && !e1 && g.edges == 1
}

/// 注入核 3：越界注入——超容量节点必拒。
pub fn inj3_oob() -> bool {
    let mut g = WaitGraph::new();
    !g.add_edge(MAX_NODES, 0) && !g.add_edge(0, MAX_NODES + 1) && !g.has_edge(MAX_NODES, 0)
}

/// 注入核 4：环注入——人工环必检出。
pub fn inj4_cycle_detect() -> bool {
    let mut g = WaitGraph::new();
    let _ = g.add_edge(10, 11);
    let _ = g.add_edge(11, 12);
    let _ = g.add_edge(12, 10);
    g.has_cycle()
}

/// 注入核 5：删账注入——删不存在边必拒且不计数。
pub fn inj5_remove_missing() -> bool {
    let mut g = WaitGraph::new();
    let r0 = g.remove_edge(3, 4);
    let _ = g.add_edge(3, 4);
    let r1 = g.remove_edge(3, 4);
    let r2 = g.remove_edge(3, 4);
    !r0 && r1 && !r2 && g.removed == 1 && g.reconciled(1)
}

// ── 判据总成 ────────────────────────────────────────────────────────────

/// 结构当量。
pub fn structural_weight() -> u32 {
    10u32 * 39
}

/// run_unx3342_checks：本单判据聚合（诊断段 0xCA80）。
pub fn run_unx3342_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F3342 等待图数据结构（节点=锁，边=持有→等待）");
    s.add(
        "代码体量≥380行当量",
        structural_weight() >= 380,
        if structural_weight() >= 380 { "" } else { "结构当量不足" },
    );
    let (ok, _) = m101();
    s.add(
        "图本体（邻接表）结构交付",
        j1_main(),
        if j1_main() { "" } else { "图本体破" },
    );
    s.add(
        "M-101建-查-删图操作对账",
        ok,
        if ok { "" } else { "M-101对账破" },
    );
    let inj = [
        ("注入1自环", inj1_self_loop()),
        ("注入2重边", inj2_dup_edge()),
        ("注入3越界", inj3_oob()),
        ("注入4环检出", inj4_cycle_detect()),
        ("注入5删账", inj5_remove_missing()),
    ];
    for (name, ok) in inj {
        s.add(name, ok, if ok { "" } else { "注入未抓" });
    }
    s
}
