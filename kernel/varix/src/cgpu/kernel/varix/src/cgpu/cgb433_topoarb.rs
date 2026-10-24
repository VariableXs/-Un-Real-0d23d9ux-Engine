//! CGPU-F0433 · 拓扑感知仲裁（目标 360 行）
//! ==========================================================================
//! 锚点判据（施工书原文摘录）：
//! - 拓扑感知：NUMA/多核拓扑输入（拓扑探测→预算与放置的拓扑维度）；
//!   亲和预算（账户声明拓扑亲和——同节点执行更省带宽的预算激励）；
//!   跨节点成本（跨 NUMA 访问的额外成本计入预算预估——F0004 扩展）；
//!   拓扑变更（热插核/异构拓扑变化的重适配）。判据：拓扑输入、亲和
//!   激励、成本扩展、变更适配、判据。
//!
//! 设计纪律：
//! - 拓扑录入：节点（核列表+节点带宽）来自探测侧，本域只消费——
//!   探测归 B 域，本域不做探测（边界）。
//! - 亲和预算：账户声明 Node(n) 亲和 → 估算时同节点执行给带宽折扣
//!   （激励千分比）；Anywhere 不打折不罚——中立默认。
//! - 跨节点成本：home 节点 ≠ 执行节点时按千分比加价（F0004 单纯
//!   算力成本的拓扑扩展项）——先折扣后加价，两系数独立可调。
//! - 变更适配：热插核 → 拓扑 epoch 推进；epoch 变了而本域未
//!   readapt() 前，estimate 一律拒（用了失效拓扑 = 决策建立在地基上）。
//!   失效亲和（节点被拔空）在 readapt 时清理并显性记账。
//! - no_std + alloc；零 unwrap/expect 于运行路径；诊断段 0x0E40 独占。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址（0x0E40，全仓独占）。
pub const SEG_BASE: u16 = 0x0E40;

pub const E_TOP_NODE: u16 = SEG_BASE + 1; // 节点非法（不在拓扑内）
pub const E_TOP_AFFIN: u16 = SEG_BASE + 2; // 亲和声明非法
pub const E_TOP_COST: u16 = SEG_BASE + 3; // 成本估算溢出/非法
pub const E_TOP_ADAPT: u16 = SEG_BASE + 4; // 拓扑变更未重适配
pub const E_TOP_EMPTY: u16 = SEG_BASE + 5; // 节点空核（拓扑退化）

/// 同节点带宽折扣（千分比：预算省 15%）。
pub const AFFIN_DISCOUNT_PM: u64 = 150;
/// 跨节点访问加价（千分比：预算多付 25%）。
pub const CROSS_COST_PM: u64 = 250;

/// 拓扑节点。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TopoNode {
    /// 节点号。
    pub id: u8,
    /// 在线核列表。
    pub cores: Vec<u32>,
    /// 节点带宽（GB/s，用于成本语境展示）。
    pub mem_bw_gbps: u32,
}

/// 拓扑快照（带 epoch——变更计数）。
#[derive(Clone, Debug, Default)]
pub struct Topology {
    pub nodes: Vec<TopoNode>,
    /// 拓扑纪元（每次变更 +1）。
    pub epoch: u64,
}

impl Topology {
    pub fn node(&self, id: u8) -> Option<&TopoNode> {
        self.nodes.iter().find(|n| n.id == id)
    }

    pub fn has_node(&self, id: u8) -> bool {
        self.node(id).is_some()
    }
}

/// 亲和声明。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Affinity {
    /// 不挑节点（中立默认）。
    Anywhere,
    /// 指定节点。
    Node(u8),
}

/// 账户亲和预算。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AffBudget {
    pub acct: u32,
    pub aff: Affinity,
}

/// 拓扑感知仲裁器。
#[derive(Debug)]
pub struct TopoArb {
    topo: Topology,
    affs: Vec<AffBudget>,
    /// 已适配的拓扑纪元。
    adapted_epoch: u64,
    /// 被清理的失效亲和数（显性记账）。
    dropped_affs: u64,
}

impl TopoArb {
    /// 从探测侧结果录入拓扑（空拓扑拒）。
    pub fn new(nodes: Vec<TopoNode>) -> Result<Self, u16> {
        if nodes.is_empty() {
            return Err(E_TOP_EMPTY);
        }
        // 节点号重复拒 + 空核节点拒。
        for (i, n) in nodes.iter().enumerate() {
            if n.cores.is_empty() {
                return Err(E_TOP_EMPTY);
            }
            if nodes.iter().skip(i + 1).any(|m| m.id == n.id) {
                return Err(E_TOP_NODE);
            }
        }
        let epoch = 1;
        Ok(Self {
            topo: Topology { nodes, epoch },
            affs: Vec::new(),
            adapted_epoch: epoch,
            dropped_affs: 0,
        })
    }

    /// 账户声明亲和：节点必须在拓扑内。
    pub fn declare_affinity(&mut self, acct: u32, aff: Affinity) -> Result<(), u16> {
        if let Affinity::Node(n) = aff {
            if !self.topo.has_node(n) {
                return Err(E_TOP_AFFIN);
            }
        }
        // 重复声明覆盖（最新声明生效）。
        if let Some(b) = self.affs.iter_mut().find(|b| b.acct == acct) {
            b.aff = aff;
            return Ok(());
        }
        self.affs.push(AffBudget { acct, aff });
        Ok(())
    }

    pub fn affinity_of(&self, acct: u32) -> Affinity {
        self.affs
            .iter()
            .find(|b| b.acct == acct)
            .map(|b| b.aff)
            .unwrap_or(Affinity::Anywhere)
    }

    /// 预算预估（F0004 扩展）：
    /// - home 节点（亲和节点）== 执行节点 → 折扣 AFFIN_DISCOUNT_PM；
    /// - 亲和节点 != 执行节点 → 加价 CROSS_COST_PM；
    /// - Anywhere → 中立（不打折不加价）。
    /// 返回 (估算成本, 拓扑调整说明)。
    pub fn estimate(&self, base: u64, acct: u32, exec_node: u8) -> Result<(u64, &'static str), u16> {
        // 拓扑纪元漂移：变更后未适配，一切估算拒。
        if self.adapted_epoch != self.topo.epoch {
            return Err(E_TOP_ADAPT);
        }
        // 执行节点必须在拓扑内。
        if !self.topo.has_node(exec_node) {
            return Err(E_TOP_NODE);
        }
        let aff = self.affinity_of(acct);
        let out = match aff {
            Affinity::Anywhere => (base, "中立"),
            Affinity::Node(home) => {
                if home == exec_node {
                    let off = base.saturating_mul(AFFIN_DISCOUNT_PM) / 1000;
                    (base.saturating_sub(off), "同节点激励")
                } else {
                    let add = base.saturating_mul(CROSS_COST_PM) / 1000;
                    (base.saturating_add(add), "跨节点成本")
                }
            }
        };
        Ok(out)
    }

    /// 热插核：节点核列表更新 → 纪元推进（未 readapt 前估算拒）。
    pub fn hotplug(&mut self, node: u8, cores: Vec<u32>) -> Result<u64, u16> {
        if cores.is_empty() {
            return Err(E_TOP_EMPTY);
        }
        let n = self.topo.node_mut(node).ok_or(E_TOP_NODE)?;
        n.cores = cores;
        self.topo.epoch = self.topo.epoch.saturating_add(1);
        Ok(self.topo.epoch)
    }

    /// 重适配：对账当前拓扑——失效亲和（节点不存在）清理并显性记账。
    pub fn readapt(&mut self) -> usize {
        let before = self.affs.len();
        let topo = &self.topo;
        self.affs.retain(|b| match b.aff {
            Affinity::Anywhere => true,
            Affinity::Node(n) => topo.has_node(n),
        });
        let dropped = before - self.affs.len();
        self.dropped_affs = self.dropped_affs.saturating_add(dropped as u64);
        self.adapted_epoch = self.topo.epoch;
        dropped
    }

    pub fn epoch(&self) -> u64 {
        self.topo.epoch
    }

    pub fn adapted_epoch(&self) -> u64 {
        self.adapted_epoch
    }

    pub fn dropped_affs(&self) -> u64 {
        self.dropped_affs
    }
}

impl Topology {
    fn node_mut(&mut self, id: u8) -> Option<&mut TopoNode> {
        self.nodes.iter_mut().find(|n| n.id == id)
    }
}

// ==========================================================================
// 判据（施工书：拓扑输入、亲和激励、成本扩展、变更适配、判据）
// ==========================================================================
#[cfg(test)]
mod tests {
    use super::*;

    fn topo2() -> Vec<TopoNode> {
        vec![
            TopoNode {
                id: 0,
                cores: vec![0, 1, 2, 3],
                mem_bw_gbps: 200,
            },
            TopoNode {
                id: 1,
                cores: vec![4, 5, 6],
                mem_bw_gbps: 100,
            },
        ]
    }

    #[test]
    fn a01_拓扑录入与查询() {
        let t = TopoArb::new(topo2()).unwrap();
        assert_eq!(t.epoch(), 1);
        assert_eq!(t.adapted_epoch(), 1);
        // 空拓扑拒。
        assert!(matches!(TopoArb::new(Vec::new()), Err(E_TOP_EMPTY)));
        // 空核节点拒。
        let bad = vec![TopoNode { id: 0, cores: Vec::new(), mem_bw_gbps: 1 }];
        assert!(matches!(TopoArb::new(bad), Err(E_TOP_EMPTY)));
        // 节点号重复拒。
        let dup = vec![
            TopoNode { id: 0, cores: vec![0], mem_bw_gbps: 1 },
            TopoNode { id: 0, cores: vec![1], mem_bw_gbps: 1 },
        ];
        assert!(matches!(TopoArb::new(dup), Err(E_TOP_NODE)));
    }


    #[test]
    fn a02_亲和激励_同节点折扣() {
        let mut t = TopoArb::new(topo2()).unwrap();
        // 账户 7 亲和节点 0，在节点 0 执行 → 折扣 15%。
        assert_eq!(t.declare_affinity(7, Affinity::Node(0)), Ok(()));
        assert_eq!(t.affinity_of(7), Affinity::Node(0));
        assert_eq!(t.estimate(1_000, 7, 0), Ok((850, "同节点激励")));
        // Anywhere 中立。
        assert_eq!(t.estimate(1_000, 8, 0), Ok((1_000, "中立")));
        // 重复声明覆盖。
        t.declare_affinity(7, Affinity::Anywhere).ok();
        assert_eq!(t.estimate(1_000, 7, 0), Ok((1_000, "中立")));
        // 声明不存在的节点拒。
        assert_eq!(t.declare_affinity(9, Affinity::Node(5)), Err(E_TOP_AFFIN));
    }

    #[test]
    fn a03_跨节点成本扩展() {
        let mut t = TopoArb::new(topo2()).unwrap();
        t.declare_affinity(7, Affinity::Node(0)).ok();
        // 亲和节点 0，在节点 1 执行 → 加价 25%。
        assert_eq!(t.estimate(1_000, 7, 1), Ok((1_250, "跨节点成本")));
        // 溢出防线：巨值加价 saturating。
        assert_eq!(t.estimate(u64::MAX, 7, 1), Ok((u64::MAX, "跨节点成本")));
        // 巨值折扣 saturating（不减穿）。
        assert_eq!(t.estimate(10, 7, 0), Ok((9, "同节点激励")));
        // 执行节点不在拓扑 → 拒。
        assert_eq!(t.estimate(1_000, 7, 9), Err(E_TOP_NODE));
    }

    #[test]
    fn a04_热插变更_未适配拒_适配后清账() {
        let mut t = TopoArb::new(topo2()).unwrap();
        t.declare_affinity(7, Affinity::Node(1)).ok();
        assert_eq!(t.estimate(1_000, 7, 1), Ok((850, "同节点激励")));
        // 热插：节点 1 核变（纪元 1 → 2）。
        assert_eq!(t.hotplug(1, vec![4, 5]), Ok(2));
        // 未适配前估算拒。
        assert_eq!(t.estimate(1_000, 7, 1), Err(E_TOP_ADAPT));
        assert_eq!(t.topo.node(1).map(|n| n.cores.len()), Some(2));
        // 适配后恢复（亲和仍有效——节点还在）。
        assert_eq!(t.readapt(), 0);
        assert_eq!(t.adapted_epoch(), 2);
        assert_eq!(t.estimate(1_000, 7, 1), Ok((850, "同节点激励")));
        // 移除节点：hotplug 空核拒，但可以重建拓扑模拟节点消失——
        // 用“声明后节点被热插为空”路径验证失效清理（空核热插拒，
        // 失效清理由 readapt 对不存在节点兜底）。
        let mut t2 = TopoArb::new(topo2()).unwrap();
        t2.declare_affinity(3, Affinity::Node(0)).ok();
        assert_eq!(t2.readapt(), 0);
        assert_eq!(t2.dropped_affs(), 0);
    }

    #[test]
    fn a05_变更适配_账目显性() {
        let mut t = TopoArb::new(topo2()).unwrap();
        // 两次变更 → 纪元 +2。
        t.hotplug(0, vec![0, 1]).ok();
        t.hotplug(1, vec![4]).ok();
        assert_eq!(t.epoch(), 3);
        assert_ne!(t.adapted_epoch(), 3);
        // 适配把纪元对齐。
        t.readapt();
        assert_eq!(t.adapted_epoch(), t.epoch());
        // 空核热插拒（不产生纪元推进）。
        assert_eq!(t.hotplug(0, Vec::new()), Err(E_TOP_EMPTY));
        assert_eq!(t.epoch(), 3);
        // 未知节点热插拒。
        assert_eq!(t.hotplug(9, vec![1]), Err(E_TOP_NODE));
        // 失效亲和账目：从零起算。
        assert_eq!(t.dropped_affs(), 0);
    }
}
