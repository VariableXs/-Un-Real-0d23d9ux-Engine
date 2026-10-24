#![allow(dead_code)]
use alloc::format;
use alloc::vec;
// SPDX-License-Identifier: Apache-2.0
// Varix Kernel — CGPU · cgb262_diffaudit
// CGPU-F0262 · 图 diff 与审计（B07 组 · 三维差异 + 审计查询 + 进总线）
//
// 锚点：CGPU-F0262
// 判据原文：图快照 diff（两快照拓扑/参数/账目三维差异报告）；
//          审计查询（按节点/资源/时间窗查历史——谁改了什么何时）；
//          diff 报告进体验日志总线。
//
// 难点分析：
//   1. 三维 diff 必须独立可查——「拓扑变了但账目没变」和「账目变了但拓扑没变」
//      是完全不同的诊断线索。
//   2. 审计查询的索引设计：按节点/资源/时间窗三轴，需要倒排索引而非全表扫描。
//   3. 「进总线」要求 diff 结果有统一序列化格式（与 13 章体验日志同构）。
//
// 纪律约束：
//   - no_std + alloc 三件套
//   - match 臂字符串尾必须加逗号


extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use super::cgb257_persistarch::{GraphSnapshot, LogicalTime, NodeId, ResId};
use crate::checks::CheckSet;

// ============================================================================
// 一、三维差异报告
// ============================================================================

/// 拓扑差异。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TopoDiff {
    /// 新增节点。
    pub nodes_added: Vec<NodeId>,
    /// 删除节点。
    pub nodes_removed: Vec<NodeId>,
    /// 新增边 (from, to, kind)。
    pub edges_added: Vec<(NodeId, NodeId, u8)>,
    /// 删除边。
    pub edges_removed: Vec<(NodeId, NodeId, u8)>,
}

impl TopoDiff {
    /// 是否有差异。
    pub fn has_changes(&self) -> bool {
        !self.nodes_added.is_empty()
            || !self.nodes_removed.is_empty()
            || !self.edges_added.is_empty()
            || !self.edges_removed.is_empty()
    }

    /// 差异摘要行数。
    pub fn summary_lines(&self) -> usize {
        self.nodes_added.len()
            + self.nodes_removed.len()
            + self.edges_added.len()
            + self.edges_removed.len()
    }
}

/// 参数差异。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ParamDiff {
    /// 变更列表：(NodeId 或 0 表示全局, key, 旧值描述, 新值描述)。
    pub changes: Vec<(NodeId, String, String, String)>,
}

impl ParamDiff {
    /// 是否有差异。
    pub fn has_changes(&self) -> bool {
        !self.changes.is_empty()
    }
}

/// 账目差异。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AccountDiff {
    /// 新增持有的 (ResId, SubjectId)。
    pub holdings_added: Vec<(ResId, u32)>,
    /// 释放的持有。
    pub holdings_removed: Vec<(ResId, u32)>,
    /// 新增等待。
    pub waitings_added: Vec<(ResId, u32)>,
    /// 等待结束。
    pub waitings_removed: Vec<(ResId, u32)>,
    /// 平衡变化。
    pub balance_delta: i64,
}

impl AccountDiff {
    /// 是否有差异。
    pub fn has_changes(&self) -> bool {
        !self.holdings_added.is_empty()
            || !self.holdings_removed.is_empty()
            || !self.waitings_added.is_empty()
            || !self.waitings_removed.is_empty()
            || self.balance_delta != 0
    }
}

/// 三维差异报告。
#[derive(Clone, Debug, Default)]
pub struct SnapshotDiff {
    /// 拓扑差异。
    pub topo: TopoDiff,
    /// 参数差异。
    pub params: ParamDiff,
    /// 账目差异。
    pub accounts: AccountDiff,
    /// 生成时刻。
    pub generated_at: LogicalTime,
}

impl SnapshotDiff {
    /// 是否有任何差异。
    pub fn has_any_changes(&self) -> bool {
        self.topo.has_changes() || self.params.has_changes() || self.accounts.has_changes()
    }

    /// 变化维度数（0~3）。
    pub fn changed_dimensions(&self) -> u32 {
        let mut n = 0;
        if self.topo.has_changes() {
            n += 1;
        }
        if self.params.has_changes() {
            n += 1;
        }
        if self.accounts.has_changes() {
            n += 1;
        }
        n
    }

    /// 生成人话摘要（进体验日志总线）。
    pub fn to_bus_message(&self) -> String {
        let mut parts = Vec::new();
        if self.topo.has_changes() {
            parts.push(alloc::format!(
                "拓扑变化(+{}节点/-{}节点/+{}边/-{}边)",
                self.topo.nodes_added.len(),
                self.topo.nodes_removed.len(),
                self.topo.edges_added.len(),
                self.topo.edges_removed.len()
            ));
        }
        if self.params.has_changes() {
            parts.push(alloc::format!("参数变化({}项)", self.params.changes.len()));
        }
        if self.accounts.has_changes() {
            parts.push(alloc::format!(
                "账目变化(+{}持有/-{}持有/+{}等待/-{}等待, 平衡{:+})",
                self.accounts.holdings_added.len(),
                self.accounts.holdings_removed.len(),
                self.accounts.waitings_added.len(),
                self.accounts.waitings_removed.len(),
                self.accounts.balance_delta
            ));
        }
        if parts.is_empty() {
            "无变化".to_string()
        } else {
            parts.join("; ")
        }
    }
}

/// Diff 引擎。
pub struct DiffEngine;

impl DiffEngine {
    /// 计算两个快照的三维差异。
    pub fn diff(old: &GraphSnapshot, new: &GraphSnapshot) -> SnapshotDiff {
        // 拓扑差异。
        let old_nodes: alloc::collections::BTreeSet<NodeId> =
            old.topology.nodes.iter().copied().collect();
        let new_nodes: alloc::collections::BTreeSet<NodeId> =
            new.topology.nodes.iter().copied().collect();
        let nodes_added: Vec<NodeId> = new_nodes.difference(&old_nodes).copied().collect();
        let nodes_removed: Vec<NodeId> = old_nodes.difference(&new_nodes).copied().collect();

        let old_edges: alloc::collections::BTreeSet<(NodeId, NodeId, u8)> =
            old.topology.edges.iter().copied().collect();
        let new_edges: alloc::collections::BTreeSet<(NodeId, NodeId, u8)> =
            new.topology.edges.iter().copied().collect();
        let edges_added: Vec<(NodeId, NodeId, u8)> = new_edges.difference(&old_edges).copied().collect();
        let edges_removed: Vec<(NodeId, NodeId, u8)> = old_edges.difference(&new_edges).copied().collect();

        // 参数差异。
        let mut param_changes = Vec::new();
        // 全局参数。
        let old_global: BTreeMap<&str, &super::cgb257_persistarch::ParamValue> =
            old.params.global_params.iter().map(|e| (e.key.as_str(), &e.value)).collect();
        let new_global: BTreeMap<&str, &super::cgb257_persistarch::ParamValue> =
            new.params.global_params.iter().map(|e| (e.key.as_str(), &e.value)).collect();
        for (key, new_val) in &new_global {
            match old_global.get(key) {
                Some(old_val) if *old_val == *new_val => {}
                Some(old_val) => {
                    param_changes.push((0, key.to_string(), format_param(old_val), format_param(new_val)));
                }
                None => {
                    param_changes.push((0, key.to_string(), "(absent)".to_string(), format_param(new_val)));
                }
            }
        }
        for (key, old_val) in &old_global {
            if !new_global.contains_key(key) {
                param_changes.push((0, key.to_string(), format_param(old_val), "(removed)".to_string()));
            }
        }
        // 节点参数。
        for (nid, new_list) in &new.params.node_params {
            let old_list = old.params.node_params.get(nid);
            let old_map: BTreeMap<&str, &super::cgb257_persistarch::ParamValue> = old_list
                .map(|l| l.iter().map(|e| (e.key.as_str(), &e.value)).collect())
                .unwrap_or_default();
            let new_map: BTreeMap<&str, &super::cgb257_persistarch::ParamValue> =
                new_list.iter().map(|e| (e.key.as_str(), &e.value)).collect();
            for (key, new_val) in &new_map {
                match old_map.get(key) {
                    Some(old_val) if *old_val == *new_val => {}
                    Some(old_val) => {
                        param_changes.push((*nid, key.to_string(), format_param(old_val), format_param(new_val)));
                    }
                    None => {
                        param_changes.push((*nid, key.to_string(), "(absent)".to_string(), format_param(new_val)));
                    }
                }
            }
        }

        // 账目差异。
        let old_hold: alloc::collections::BTreeSet<(ResId, u32)> =
            old.accounts.holdings.iter().copied().collect();
        let new_hold: alloc::collections::BTreeSet<(ResId, u32)> =
            new.accounts.holdings.iter().copied().collect();
        let holdings_added: Vec<(ResId, u32)> = new_hold.difference(&old_hold).copied().collect();
        let holdings_removed: Vec<(ResId, u32)> = old_hold.difference(&new_hold).copied().collect();

        let old_wait: alloc::collections::BTreeSet<(ResId, u32)> =
            old.accounts.waitings.iter().copied().collect();
        let new_wait: alloc::collections::BTreeSet<(ResId, u32)> =
            new.accounts.waitings.iter().copied().collect();
        let waitings_added: Vec<(ResId, u32)> = new_wait.difference(&old_wait).copied().collect();
        let waitings_removed: Vec<(ResId, u32)> = old_wait.difference(&new_wait).copied().collect();

        SnapshotDiff {
            topo: TopoDiff {
                nodes_added,
                nodes_removed,
                edges_added,
                edges_removed,
            },
            params: ParamDiff {
                changes: param_changes,
            },
            accounts: AccountDiff {
                holdings_added,
                holdings_removed,
                waitings_added,
                waitings_removed,
                balance_delta: new.accounts.balance - old.accounts.balance,
            },
            generated_at: new.created_at,
        }
    }
}

/// 格式化参数值。
fn format_param(v: &super::cgb257_persistarch::ParamValue) -> String {
    use super::cgb257_persistarch::ParamValue;
    match v {
        ParamValue::Int(n) => alloc::format!("{}", n),
        ParamValue::Float(f) => alloc::format!("{:.6}", f),
        ParamValue::Bool(b) => alloc::format!("{}", b),
        ParamValue::Str(s) => alloc::format!("\"{}\"", s),
    }
}

// ============================================================================
// 二、审计查询（三轴倒排索引）
// ============================================================================

/// 审计条目。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuditEntry {
    /// 逻辑时间。
    pub time: LogicalTime,
    /// 操作者（SubjectId）。
    pub actor: u32,
    /// 目标类型。
    pub target_kind: AuditTarget,
    /// 目标 ID。
    pub target_id: u32,
    /// 操作描述。
    pub action: String,
}

/// 审计目标类型。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AuditTarget {
    /// 节点。
    Node,
    /// 资源。
    Resource,
    /// 图。
    Graph,
}

/// 审计索引（三轴）。
pub struct AuditIndex {
    /// 全量条目。
    entries: Vec<AuditEntry>,
    /// 按节点倒排。
    by_node: BTreeMap<NodeId, Vec<usize>>,
    /// 按资源倒排。
    by_resource: BTreeMap<ResId, Vec<usize>>,
    /// 按时间排序索引。
    by_time: Vec<usize>,
}

impl AuditIndex {
    /// 构造。
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            by_node: BTreeMap::new(),
            by_resource: BTreeMap::new(),
            by_time: Vec::new(),
        }
    }

    /// 记录审计条目。
    pub fn record(&mut self, entry: AuditEntry) {
        let idx = self.entries.len();
        match entry.target_kind {
            AuditTarget::Node => {
                self.by_node.entry(entry.target_id).or_default().push(idx);
            }
            AuditTarget::Resource => {
                self.by_resource.entry(entry.target_id).or_default().push(idx);
            }
            AuditTarget::Graph => {}
        }
        self.by_time.push(idx);
        self.entries.push(entry);
        // 保持时间序。
        self.by_time.sort_by(|&a, &b| self.entries[a].time.cmp(&self.entries[b].time));
    }

    /// 按节点查询。
    pub fn query_by_node(&self, node: NodeId) -> Vec<&AuditEntry> {
        self.by_node
            .get(&node)
            .map(|idxs| idxs.iter().map(|&i| &self.entries[i]).collect())
            .unwrap_or_default()
    }

    /// 按资源查询。
    pub fn query_by_resource(&self, res: ResId) -> Vec<&AuditEntry> {
        self.by_resource
            .get(&res)
            .map(|idxs| idxs.iter().map(|&i| &self.entries[i]).collect())
            .unwrap_or_default()
    }

    /// 按时间窗查询。
    pub fn query_by_time_window(&self, from: LogicalTime, to: LogicalTime) -> Vec<&AuditEntry> {
        self.by_time
            .iter()
            .map(|&i| &self.entries[i])
            .filter(|e| e.time >= from && e.time <= to)
            .collect()
    }

    /// 组合查询（时间窗 × 节点）。
    pub fn query_combined(
        &self,
        from: LogicalTime,
        to: LogicalTime,
        node: Option<NodeId>,
        resource: Option<ResId>,
    ) -> Vec<&AuditEntry> {
        self.by_time
            .iter()
            .map(|&i| &self.entries[i])
            .filter(|e| {
                if e.time < from || e.time > to {
                    return false;
                }
                if let Some(n) = node {
                    if e.target_kind != AuditTarget::Node || e.target_id != n {
                        return false;
                    }
                }
                if let Some(r) = resource {
                    if e.target_kind != AuditTarget::Resource || e.target_id != r {
                        return false;
                    }
                }
                true
            })
            .collect()
    }

    /// 条目总数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl Default for AuditIndex {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// 三、判据：F0262 三条（三维 diff、审计查询、进总线）
// ============================================================================

/// F0262 判据集。
pub fn run_f0262_checks() -> CheckSet {
    let mut cs = CheckSet::new("CGPU-F0262");

    use super::cgb257_persistarch::{
        AccountLayer, EventStreamLayer, ParamLayer, ParamValue, TopologyLayer,
    };

    // 构造两个快照（old → new 有三维变化）。
    let old_snap = GraphSnapshot::new(
        TopologyLayer::new(1, vec![1, 2], vec![(1, 2, 0)]),
        {
            let mut p = ParamLayer::new();
            p.set_node_param(1, "prio", ParamValue::Int(5));
            p
        },
        AccountLayer::new(vec![(10, 1)], vec![]),
        EventStreamLayer::new(vec![]),
    );
    let new_snap = GraphSnapshot::new(
        TopologyLayer::new(1, vec![1, 2, 3], vec![(1, 2, 0), (2, 3, 1)]),
        {
            let mut p = ParamLayer::new();
            p.set_node_param(1, "prio", ParamValue::Int(9));
            p.set_node_param(3, "new_key", ParamValue::Bool(true));
            p
        },
        AccountLayer::new(vec![(10, 1), (11, 3)], vec![(12, 2)]),
        EventStreamLayer::new(vec![]),
    );

    // --- 判据 1：三维 diff ---
    let diff = DiffEngine::diff(&old_snap, &new_snap);
    let topo_ok = diff.topo.nodes_added.contains(&3) && diff.topo.edges_added.len() == 1;
    let param_ok = diff.params.changes.len() >= 2;
    let account_ok = diff.accounts.holdings_added.len() == 1 && diff.accounts.waitings_added.len() == 1;
    cs.add(
        "三维 diff 各自独立可查",
        topo_ok && param_ok && account_ok,
        alloc::format!(
            "拓扑+1节点+1边, 参数{}变更, 账目+1持有+1等待",
            diff.params.changes.len()
        ).leak(),
    );

    // --- 判据 2：审计查询 ---
    let mut audit = AuditIndex::new();
    audit.record(AuditEntry {
        time: 100,
        actor: 1,
        target_kind: AuditTarget::Node,
        target_id: 3,
        action: "add node".to_string(),
    });
    audit.record(AuditEntry {
        time: 200,
        actor: 2,
        target_kind: AuditTarget::Resource,
        target_id: 11,
        action: "allocate".to_string(),
    });
    audit.record(AuditEntry {
        time: 300,
        actor: 1,
        target_kind: AuditTarget::Node,
        target_id: 3,
        action: "set param".to_string(),
    });
    let by_node = audit.query_by_node(3);
    let by_res = audit.query_by_resource(11);
    let by_window = audit.query_by_time_window(150, 250);
    cs.add(
        "三轴审计查询",
        by_node.len() == 2 && by_res.len() == 1 && by_window.len() == 1,
        "节点3查到2条/资源11查到1条/时间窗查到1条",
    );

    // --- 判据 3：进总线 ---
    let msg = diff.to_bus_message();
    let bus_ok = msg.contains("拓扑") && msg.contains("参数") && msg.contains("账目");
    cs.add(
        "diff 报告进总线",
        bus_ok,
        alloc::format!("总线消息: {}", msg).leak(),
    );

    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f0262_all_pass() {
        let cs = run_f0262_checks();
        assert!(cs.all_green(), "F0262 判据未全绿: {}", cs.red_items_text());
    }

    #[test]
    fn diff_no_change() {
        use super::super::cgb257_persistarch::*;
        let snap = GraphSnapshot::new(
            TopologyLayer::new(1, vec![1], vec![]),
            ParamLayer::new(),
            AccountLayer::new(vec![], vec![]),
            EventStreamLayer::new(vec![]),
        );
        let d = DiffEngine::diff(&snap, &snap);
        assert!(!d.has_any_changes());
        assert_eq!(d.changed_dimensions(), 0);
    }
}
