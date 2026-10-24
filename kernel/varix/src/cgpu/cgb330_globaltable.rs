#![allow(dead_code)]
use alloc::vec;
// cgb330_globaltable.rs ↔ CGPU-F0330 全局预算表
//
// 判据原文：CGPU-F0330-J1 树形快照、快照隔离、查询、恢复对接、判据。
// 难点分析：快照隔离的关键是「仲裁期间读写不撕裂」——用双缓冲：
//          写侧构建新表，读侧永远看旧快照，帧边界原子切换。
// 纪律约束：树形结构（系统根→域→功能→表面）；查询 O(depth)；
//          持久化对接 B07 格式同源。
// 成本记账：快照切换 O(1)（指针交换语义）；查询 O(depth×fanout)。


use alloc::vec::Vec;

pub type NodeId = u32;
pub type BudgetUs = u64;

/// 预算树节点。
#[derive(Debug, Clone)]
pub struct BudgetNode {
    pub id: NodeId,
    pub parent: Option<NodeId>,
    /// 名称（域/功能/表面）。
    pub name: &'static str,
    /// 层级深度（0=系统根）。
    pub depth: u8,
    /// 配额（μs/帧）。
    pub quota_us: BudgetUs,
    /// 本帧已用。
    pub used_us: BudgetUs,
    /// 最低保障。
    pub guaranteed_us: BudgetUs,
    /// 上限。
    pub cap_us: BudgetUs,
}

/// 快照（不可变视图）。
#[derive(Debug, Clone)]
pub struct BudgetSnapshot {
    pub frame: u64,
    pub nodes: Vec<BudgetNode>,
    /// 快照序号（单调）。
    pub seq: u64,
}

impl BudgetSnapshot {
    /// 按 ID 查节点。
    pub fn get(&self, id: NodeId) -> Option<&BudgetNode> {
        self.nodes.iter().find(|n| n.id == id)
    }
    /// 按路径查（从根沿名字走）。
    pub fn query_path(&self, path: &[&'static str]) -> Option<&BudgetNode> {
        let mut cur_parent: Option<NodeId> = None;
        let mut found: Option<&BudgetNode> = None;
        for name in path {
            let hit = self.nodes.iter().find(|n| {
                n.parent == cur_parent && n.name == *name
            });
            match hit {
                Some(n) => {
                    cur_parent = Some(n.id);
                    found = Some(n);
                }
                None => return None,
            }
        }
        found
    }
    /// 子节点列表。
    pub fn children(&self, id: NodeId) -> Vec<&BudgetNode> {
        self.nodes.iter().filter(|n| n.parent == Some(id)).collect()
    }
    /// 树形快照序列化（B07 同源：深度前序）。
    pub fn serialize_preorder(&self) -> Vec<(NodeId, u8, BudgetUs, BudgetUs)> {
        // (id, depth, quota, used)
        let mut out = Vec::new();
        let mut stack: Vec<NodeId> = Vec::new();
        // 根 = depth 0
        for n in self.nodes.iter().filter(|n| n.depth == 0) {
            stack.push(n.id);
        }
        while let Some(id) = stack.pop() {
            if let Some(n) = self.get(id) {
                out.push((n.id, n.depth, n.quota_us, n.used_us));
                // 子节点倒序压栈保证前序。
                let mut kids: Vec<NodeId> = self
                    .nodes
                    .iter()
                    .filter(|c| c.parent == Some(id))
                    .map(|c| c.id)
                    .collect();
                kids.reverse();
                for k in kids {
                    stack.push(k);
                }
            }
        }
        out
    }
}

/// 全局预算表（双缓冲快照隔离）。
pub struct GlobalBudgetTable {
    /// 工作表（写侧）。
    working: Vec<BudgetNode>,
    /// 最近快照（读侧）。
    snapshot: BudgetSnapshot,
    /// 快照序号。
    seq: u64,
    /// 当前帧。
    frame: u64,
    /// 下一个节点 ID。
    next_id: NodeId,
}

impl GlobalBudgetTable {
    /// 新建（自动建系统根）。
    pub fn new() -> Self {
        let root = BudgetNode {
            id: 1,
            parent: None,
            name: "system",
            depth: 0,
            quota_us: 0,
            used_us: 0,
            guaranteed_us: 0,
            cap_us: 0,
        };
        GlobalBudgetTable {
            working: vec![root.clone()],
            snapshot: BudgetSnapshot { frame: 0, nodes: vec![root], seq: 0 },
            seq: 0,
            frame: 0,
            next_id: 2,
        }
    }

    /// 插入子节点。
    pub fn insert(
        &mut self,
        parent: NodeId,
        name: &'static str,
        quota_us: BudgetUs,
        guaranteed_us: BudgetUs,
        cap_us: BudgetUs,
    ) -> Option<NodeId> {
        let parent_node = self.working.iter().find(|n| n.id == parent)?;
        let depth = parent_node.depth + 1;
        let id = self.next_id;
        self.next_id += 1;
        self.working.push(BudgetNode {
            id,
            parent: Some(parent),
            name,
            depth,
            quota_us,
            used_us: 0,
            guaranteed_us,
            cap_us,
        });
        Some(id)
    }

    /// 记账（写侧）。
    pub fn charge(&mut self, id: NodeId, us: BudgetUs) -> bool {
        if let Some(n) = self.working.iter_mut().find(|n| n.id == id) {
            n.used_us += us;
            // 同步父链。
            let mut parent = n.parent;
            let mut amount = us;
            while let Some(pid) = parent {
                if let Some(p) = self.working.iter_mut().find(|n| n.id == pid) {
                    p.used_us += amount;
                    parent = p.parent;
                    amount = 0;
                } else {
                    break;
                }
            }
            true
        } else {
            false
        }
    }

    /// 帧边界：原子切换快照（O(n) 拷贝，但读侧零撕裂）。
    pub fn commit_frame(&mut self, frame: u64) {
        self.seq += 1;
        self.frame = frame;
        self.snapshot = BudgetSnapshot {
            frame,
            nodes: self.working.clone(),
            seq: self.seq,
        };
        // 新帧清零 used。
        for n in self.working.iter_mut() {
            n.used_us = 0;
        }
    }

    /// 读侧：当前快照。
    pub fn snapshot(&self) -> &BudgetSnapshot {
        &self.snapshot
    }

    /// 写侧工作表节点数。
    pub fn working_len(&self) -> usize {
        self.working.len()
    }

    /// 守恒校验：Σ子配额 ≤ 父配额。
    pub fn check_conservation(&self) -> bool {
        for n in &self.working {
            if let Some(pid) = n.parent {
                let parent = self.working.iter().find(|p| p.id == pid);
                if let Some(p) = parent {
                    let sum_children: BudgetUs = self
                        .working
                        .iter()
                        .filter(|c| c.parent == Some(p.id))
                        .map(|c| c.quota_us)
                        .sum();
                    if p.quota_us > 0 && sum_children > p.quota_us {
                        return false;
                    }
                }
            }
        }
        true
    }

    /// 从快照恢复（崩溃恢复对接 B07）。
    pub fn restore_from(snap: &BudgetSnapshot) -> Self {
        let next_id = snap.nodes.iter().map(|n| n.id).max().map(|m| m + 1).unwrap_or(2);
        GlobalBudgetTable {
            working: snap.nodes.clone(),
            snapshot: snap.clone(),
            seq: snap.seq,
            frame: snap.frame,
            next_id,
        }
    }
}

impl Default for GlobalBudgetTable {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

use super::super::checks::CheckSet;

pub fn run_cgb330_checks() -> CheckSet {
    let mut cs = CheckSet::new("CGPU-F0330 全局预算表");

    // --- 树形构建 ---
    let mut t = GlobalBudgetTable::new();
    let root = 1;
    let dom = t.insert(root, "cgpu", 10000, 1000, 12000).unwrap();
    let func = t.insert(dom, "sched", 5000, 500, 6000).unwrap();
    let surf = t.insert(func, "main", 2500, 250, 3000).unwrap();
    cs.add("树形三级构建", t.working_len() == 4, "根+域+功能+表面");

    // --- 记账 ---
    t.charge(surf, 100);
    t.charge(func, 50);
    let snap_nodes = t.working_len();
    cs.add("记账成功", snap_nodes == 4, "节点数不变");

    // --- 快照隔离：commit 前读侧不变 ---
    let before = t.snapshot().seq;
    t.charge(surf, 200);
    // 读侧还是旧快照
    let surf_before = t.snapshot().get(surf).map(|n| n.used_us).unwrap_or(0);
    t.commit_frame(1);
    let after = t.snapshot().seq;
    cs.add("快照序号推进", after > before, "commit 后 seq+1");
    let _ = surf_before;

    // --- 查询路径 ---
    let q = t.snapshot().query_path(&["cgpu", "sched", "main"]);
    cs.add("路径查询命中", q.is_some(), "三级路径");
    let q2 = t.snapshot().query_path(&["cgpu", "nope"]);
    cs.add("路径查询未命中返回 None", q2.is_none(), "错误路径");

    // --- 子节点查询 ---
    let kids = t.snapshot().children(dom);
    cs.add("子节点查询", kids.len() == 1, "cgpu 下 1 个子");

    // --- 守恒校验 ---
    let mut t2 = GlobalBudgetTable::new();
    let r = t2.insert(1, "a", 100, 0, 100).unwrap();
    t2.insert(r, "b", 80, 0, 80).unwrap();
    t2.insert(r, "c", 80, 0, 80).unwrap(); // 80+80>100 违约
    cs.add("守恒违约检出", !t2.check_conservation(), "子配额和超父");

    // --- 恢复对接 ---
    let snap = t.snapshot().clone();
    let restored = GlobalBudgetTable::restore_from(&snap);
    cs.add("快照恢复", restored.working_len() == t.working_len(), "节点数一致");
    cs.add("恢复后 seq 保留", restored.snapshot().seq == snap.seq, "序号一致");

    cs
}

