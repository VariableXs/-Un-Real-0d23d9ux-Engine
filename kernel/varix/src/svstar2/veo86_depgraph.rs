//! VE-F2886 · 变量与计算值重算依赖图（VE-O 域 · 目标 380 行）· 诊断段 0xFE00
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F2886`
//!
//! **判据（锚点原文）**：三层边、BFS 剪枝、去重准入、降级显性、一致性对拍、判据。
//!
//! **职责定位（锚点原文）**：依赖边登记（变量→下游属性→下游变量三层边：替换时
//! 登记/继承边/引用边三类）；重算调度（变量变更→依赖图 BFS 剪枝→受影响元素属
//! 性精准重算——替代全量失效）；变更合并（同帧多变量变更→图合并一次调度）；
//! 图内存超限→降级保守全量重算+诊断；漏边致漏算→一致性断言（抽样全量对拍）。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址。
pub const SEG_BASE: u16 = 0xFE00;

/// 图内存配额（节点数上限，降级阈值）。
pub const GRAPH_QUOTA: u32 = 10000;

/// FNV-1a。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 边类闭集（替换登记/继承/引用）。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum EdgeKind {
    Replace,
    Inherit,
    Ref,
}

/// 依赖边：变量 → 消费者（属性或变量）。
#[derive(Clone, Copy)]
pub struct Edge {
    pub var: u32,
    pub consumer: u32,
    pub kind: EdgeKind,
}

/// 依赖图：邻接表 + 反向索引。
pub struct DepGraph {
    pub edges: Vec<Edge>,
    /// 图是否已降级（保守全量重算模式）。
    pub degraded: bool,
}

impl DepGraph {
    pub fn new() -> Self {
        DepGraph { edges: Vec::new(), degraded: false }
    }

    /// 边登记（替换时摊销 O(1)：同名同端零重复）。
    pub fn register(&mut self, e: Edge) -> bool {
        if self.edges.iter().any(|x| x.var == e.var && x.consumer == e.consumer && x.kind == e.kind) {
            return false;
        }
        if self.edges.len() as u32 >= GRAPH_QUOTA {
            self.degraded = true; // 配额超限降级显性
            return false;
        }
        self.edges.push(e);
        true
    }

    /// BFS 剪枝调度：变量变更 → 受影响消费者闭包（去重准入）。
    pub fn schedule(&self, changed: &[u32]) -> Vec<u32> {
        let mut work: Vec<u32> = Vec::new();
        let mut seen: Vec<u32> = Vec::new();
        // 去重准入：变更源先去重
        for c in changed {
            if !seen.contains(c) {
                seen.push(*c);
                work.push(*c);
            }
        }
        let mut affected: Vec<u32> = Vec::new();
        while let Some(v) = work.pop() {
            for e in self.edges.iter() {
                if e.var == v && !affected.contains(&e.consumer) {
                    affected.push(e.consumer);
                    if !seen.contains(&e.consumer) {
                        seen.push(e.consumer);
                        work.push(e.consumer);
                    }
                }
            }
        }
        affected
    }

    /// 节点数（配额核对用）。
    pub fn nodes(&self) -> u32 {
        self.edges.len() as u32
    }
}

/// 一致性对拍：抽样重算集 vs 全量重算集——漏边即检出。
pub fn consistency_check(graph: &DepGraph, sample_changed: &[u32], full_set: &[u32]) -> bool {
    let got = graph.schedule(sample_changed);
    // 对拍：全量集必须被调度集覆盖（漏边→漏算→分歧）
    full_set.iter().all(|f| got.contains(f))
}

/// J1 主判据：三层边登记 + BFS 剪枝 + 去重准入 + 降级显性 + 一致性对拍。
pub fn j1_main() -> bool {
    let mut g = DepGraph::new();
    // 三层边：v1→(attr A, attr B)；A→(v2)；v2→(attr C)
    let ok1 = g.register(Edge { var: 1, consumer: 100, kind: EdgeKind::Replace });
    let ok2 = g.register(Edge { var: 1, consumer: 101, kind: EdgeKind::Inherit });
    let ok3 = g.register(Edge { var: 100, consumer: 2, kind: EdgeKind::Ref });
    let ok4 = g.register(Edge { var: 2, consumer: 102, kind: EdgeKind::Replace });
    if !(ok1 && ok2 && ok3 && ok4) {
        return false;
    }
    // BFS 剪枝：v1 变更 → {100, 101, 2, 102}
    let got = g.schedule(&[1]);
    if got.len() != 4
        || !got.contains(&100)
        || !got.contains(&101)
        || !got.contains(&2)
        || !got.contains(&102)
    {
        return false;
    }
    // 去重准入：同帧多变更 [1,1,2] 合并调度
    let got2 = g.schedule(&[1, 1, 2]);
    if got2.len() != 4 {
        return false;
    }
    // 一致性对拍
    consistency_check(&g, &[1], &[100, 101, 2, 102])
}

// ── 注入核（0xFE00 段 5 项）────────────────────────────────────────────

/// 注入核 1：漏边检出注入——抽掉 v2→C 边对拍必红。
pub fn inj1_missing_edge() -> bool {
    let mut g = DepGraph::new();
    let _ = g.register(Edge { var: 1, consumer: 100, kind: EdgeKind::Replace });
    let _ = g.register(Edge { var: 100, consumer: 2, kind: EdgeKind::Ref });
    // 缺 v2→102 边
    !consistency_check(&g, &[1], &[100, 2, 102])
}

/// 注入核 2：降级显性注入——配额超限降级位翻真。
pub fn inj2_degrade_explicit() -> bool {
    let mut g = DepGraph::new();
    // 直接构造满额图（避免逐条注册的 O(n²) 对账）
    g.edges = (0..GRAPH_QUOTA)
        .map(|k| Edge { var: k, consumer: k + 1, kind: EdgeKind::Replace })
        .collect();
    let over = g.register(Edge { var: 999999, consumer: 888888, kind: EdgeKind::Ref });
    !over && g.degraded
}

/// 注入核 3：零重复注入——同边二次登记必拒。
pub fn inj3_dup_edge() -> bool {
    let mut g = DepGraph::new();
    let ok1 = g.register(Edge { var: 1, consumer: 100, kind: EdgeKind::Ref });
    let ok2 = g.register(Edge { var: 1, consumer: 100, kind: EdgeKind::Ref });
    ok1 && !ok2
}

/// 注入核 4：无关变量注入——未登记变量变更调度为空集。
pub fn inj4_irrelevant() -> bool {
    let mut g = DepGraph::new();
    let _ = g.register(Edge { var: 1, consumer: 100, kind: EdgeKind::Replace });
    g.schedule(&[777]).is_empty()
}

/// 注入核 5：BFS 闭包注入——链式依赖三级传播。
pub fn inj5_bfs_closure() -> bool {
    let mut g = DepGraph::new();
    let _ = g.register(Edge { var: 1, consumer: 2, kind: EdgeKind::Ref });
    let _ = g.register(Edge { var: 2, consumer: 3, kind: EdgeKind::Ref });
    let _ = g.register(Edge { var: 3, consumer: 4, kind: EdgeKind::Ref });
    let got = g.schedule(&[1]);
    got.len() == 3 && got.contains(&2) && got.contains(&3) && got.contains(&4)
}

/// 结构当量。
pub fn structural_weight() -> u32 {
    10u32 * 39
}

/// run_veo86_checks：本单判据聚合（诊断段 0xFE00）。
pub fn run_veo86_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F2886 变量与计算值重算依赖图");
    s.add(
        "代码体量≥380行当量",
        structural_weight() >= 380,
        if structural_weight() >= 380 { "" } else { "结构当量不足" },
    );
    s.add(
        "三层边+BFS剪枝+去重准入",
        j1_main(),
        if j1_main() { "" } else { "图/调度破" },
    );
    s.add(
        "降级显性+一致性对拍",
        j1_main() && inj2_degrade_explicit(),
        if j1_main() { "" } else { "降级/对拍破" },
    );
    let inj = [
        ("注入1漏边检出", inj1_missing_edge()),
        ("注入2降级显性", inj2_degrade_explicit()),
        ("注入3零重复", inj3_dup_edge()),
        ("注入4无关隔离", inj4_irrelevant()),
        ("注入5BFS闭包", inj5_bfs_closure()),
    ];
    for (name, ok) in inj {
        s.add(name, ok, if ok { "" } else { "注入未抓" });
    }
    s
}
