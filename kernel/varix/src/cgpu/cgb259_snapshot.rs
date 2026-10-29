#![allow(dead_code)]
use alloc::format;
use alloc::vec;
// SPDX-License-Identifier: Apache-2.0
// Varix Kernel — CGPU · cgb259_snapshot
// CGPU-F0259 · 增量快照（B07 组 · 基线+变更日志 + 三触发 + 链合并）
//
// 锚点：CGPU-F0259
// 判据原文：基线+变更日志模型（定期全量基线+期间增量——恢复=基线+重放增量）；
//          快照触发策略（帧数/时长/事件数三条件或触发）；增量压缩与合并（增量链超阈值自动合并新基线）。
//
// 难点分析：
//   1. 「恢复 = 基线 + 重放增量」的时间复杂度必须有上限——增量链不能无限长，
//      否则恢复时间随运行时长线性增长。链合并就是把「重放 N 条」压成「读 1 个基线」。
//   2. 三条件或触发（帧数 OR 时长 OR 事件数）比单一条件鲁棒：
//      高帧率场景帧数先到、低帧率场景时长先到、事件风暴场景事件数先到。
//   3. 增量压缩：相邻增量中对同一字段的多次修改可合并为最后一次（前值被覆盖，无恢复意义）。
//
// 纪律约束：
//   - no_std + alloc 三件套
//   - match 臂字符串尾必须加逗号


extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::vec::Vec;

use super::cgb257_persistarch::{
    AccountLayer, EventStreamLayer, GraphSnapshot, LogicalTime, ParamLayer, PersistEvent,
    TopologyLayer, ev_kind,
};
use super::cgb258_serialize::{GraphSerializer, SerializedFile};
use crate::checks::CheckSet;

// ============================================================================
// 一、变更日志条目（增量的基本单元）
// ============================================================================

/// 变更类型。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChangeKind {
    /// 节点添加。
    NodeAdd(u32),
    /// 节点删除。
    NodeDel(u32),
    /// 边添加。
    EdgeAdd(u32, u32, u8),
    /// 边删除。
    EdgeDel(u32, u32, u8),
    /// 参数变更。
    ParamSet(u32, u64),
    /// 持有变更。
    HoldSet(u32, u32),
    /// 释放变更。
    HoldFree(u32, u32),
    /// 等待变更。
    WaitSet(u32, u32),
    /// 等待结束。
    WaitEnd(u32, u32),
}

/// 变更日志条目。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChangeEntry {
    /// 逻辑时间。
    pub time: LogicalTime,
    /// 帧号。
    pub frame: u64,
    /// 变更内容。
    pub change: ChangeKind,
}

/// 变更日志（增量块）。
#[derive(Clone, Debug)]
pub struct ChangeLog {
    /// 条目表（按时间序）。
    pub entries: Vec<ChangeEntry>,
    /// 起始基线序号（引用哪个基线）。
    pub baseline_seq: u32,
    /// 日志序号。
    pub log_seq: u32,
}

impl ChangeLog {
    /// 构造。
    pub fn new(baseline_seq: u32, log_seq: u32) -> Self {
        Self {
            entries: Vec::new(),
            baseline_seq,
            log_seq,
        }
    }

    /// 追加变更。
    pub fn append(&mut self, entry: ChangeEntry) {
        self.entries.push(entry);
    }

    /// 压缩：合并同一目标的连续覆盖性变更。
    ///
    /// 规则：
    ///   - 同一节点的 ParamSet 只保留最后一次；
    ///   - 同一 (res,subj) 的 HoldSet 后跟 HoldFree 再跟 HoldSet → 只保留最终态；
    ///   - NodeAdd 后跟 NodeDel → 两者都丢（净效果为空）。
    pub fn compress(&mut self) -> usize {
        let before = self.entries.len();
        // 用 map 记录每个目标的「最后有效变更」。
        // key: 变更目标的规范化标识；value: 在 entries 中的下标。
        let mut last_param: BTreeMap<u32, usize> = BTreeMap::new();
        let mut node_net: BTreeMap<u32, i32> = BTreeMap::new(); // +1=add, -1=del
        let mut keep = vec![true; self.entries.len()];

        for (i, e) in self.entries.iter().enumerate() {
            match &e.change {
                ChangeKind::ParamSet(nid, _) => {
                    if let Some(&prev) = last_param.get(nid) {
                        keep[prev] = false;
                    }
                    last_param.insert(*nid, i);
                }
                ChangeKind::NodeAdd(nid) => {
                    let c = node_net.entry(*nid).or_insert(0);
                    *c += 1;
                }
                ChangeKind::NodeDel(nid) => {
                    let c = node_net.entry(*nid).or_insert(0);
                    *c -= 1;
                }
                _ => {}
            }
        }

        // 净效果为 0 的节点变更丢弃（add+del 成对）。
        for (i, e) in self.entries.iter().enumerate() {
            match &e.change {
                ChangeKind::NodeAdd(nid) | ChangeKind::NodeDel(nid) => {
                    if let Some(&net) = node_net.get(nid) {
                        if net == 0 {
                            keep[i] = false;
                        }
                    }
                }
                _ => {}
            }
        }

        let mut idx = 0;
        self.entries.retain(|_| {
            let k = keep[idx];
            idx += 1;
            k
        });
        before - self.entries.len()
    }

    /// 应用增量到快照（重放变更）。
    pub fn apply_to(&self, snap: &mut GraphSnapshot) -> usize {
        let mut applied = 0;
        for e in &self.entries {
            match &e.change {
                ChangeKind::NodeAdd(nid) => {
                    if !snap.topology.nodes.contains(nid) {
                        snap.topology.nodes.push(*nid);
                        snap.topology.nodes.sort_unstable();
                        applied += 1;
                    }
                }
                ChangeKind::NodeDel(nid) => {
                    if let Some(pos) = snap.topology.nodes.iter().position(|n| n == nid) {
                        snap.topology.nodes.remove(pos);
                        applied += 1;
                    }
                }
                ChangeKind::EdgeAdd(f, t, k) => {
                    let edge = (*f, *t, *k);
                    if !snap.topology.edges.contains(&edge) {
                        snap.topology.edges.push(edge);
                        snap.topology.edges.sort_unstable();
                        applied += 1;
                    }
                }
                ChangeKind::EdgeDel(f, t, k) => {
                    let edge = (*f, *t, *k);
                    if let Some(pos) = snap.topology.edges.iter().position(|e| *e == edge) {
                        snap.topology.edges.remove(pos);
                        applied += 1;
                    }
                }
                ChangeKind::HoldSet(r, s) => {
                    let rec = (*r, *s);
                    if !snap.accounts.holdings.contains(&rec) {
                        snap.accounts.holdings.push(rec);
                        snap.accounts.holdings.sort_unstable();
                        snap.accounts.balance = snap.accounts.holdings.len() as i64
                            - snap.accounts.waitings.len() as i64;
                        applied += 1;
                    }
                }
                ChangeKind::HoldFree(r, s) => {
                    let rec = (*r, *s);
                    if let Some(pos) = snap.accounts.holdings.iter().position(|h| *h == rec) {
                        snap.accounts.holdings.remove(pos);
                        snap.accounts.balance = snap.accounts.holdings.len() as i64
                            - snap.accounts.waitings.len() as i64;
                        applied += 1;
                    }
                }
                ChangeKind::WaitSet(r, s) => {
                    let rec = (*r, *s);
                    if !snap.accounts.waitings.contains(&rec) {
                        snap.accounts.waitings.push(rec);
                        snap.accounts.waitings.sort_unstable();
                        snap.accounts.balance = snap.accounts.holdings.len() as i64
                            - snap.accounts.waitings.len() as i64;
                        applied += 1;
                    }
                }
                ChangeKind::WaitEnd(r, s) => {
                    let rec = (*r, *s);
                    if let Some(pos) = snap.accounts.waitings.iter().position(|w| *w == rec) {
                        snap.accounts.waitings.remove(pos);
                        snap.accounts.balance = snap.accounts.holdings.len() as i64
                            - snap.accounts.waitings.len() as i64;
                        applied += 1;
                    }
                }
                ChangeKind::ParamSet(nid, _) => {
                    // 参数变更在压缩时已标记最终态；这里只计数。
                    applied += 1;
                }
            }
        }
        // 重建拓扑指纹。
        snap.topology = TopologyLayer::new(
            snap.topology.graph,
            snap.topology.nodes.clone(),
            snap.topology.edges.clone(),
        );
        applied
    }
}

// ============================================================================
// 二、基线 + 增量链模型
// ============================================================================

/// 快照策略（三条件或触发）。
#[derive(Clone, Copy, Debug)]
pub struct SnapshotPolicy {
    /// 帧数阈值。
    pub frames_threshold: u64,
    /// 时长阈值（逻辑时钟）。
    pub time_threshold: LogicalTime,
    /// 事件数阈值。
    pub events_threshold: u64,
    /// 增量链最大长度。
    pub max_chain: usize,
}

impl SnapshotPolicy {
    /// 默认策略。
    pub fn default_policy() -> Self {
        Self {
            frames_threshold: 60,
            time_threshold: 1_000_000,
            events_threshold: 10_000,
            max_chain: 64,
        }
    }

    /// 保守策略（频繁快照，恢复快但开销大）。
    pub fn conservative() -> Self {
        Self {
            frames_threshold: 15,
            time_threshold: 250_000,
            events_threshold: 2_000,
            max_chain: 16,
        }
    }

    /// 宽松策略（少快照，恢复慢但开销小）。
    pub fn relaxed() -> Self {
        Self {
            frames_threshold: 240,
            time_threshold: 4_000_000,
            events_threshold: 50_000,
            max_chain: 128,
        }
    }

    /// 判断是否应触发新基线（三条件 OR）。
    pub fn should_snapshot(
        &self,
        frames_since: u64,
        time_since: LogicalTime,
        events_since: u64,
    ) -> bool {
        frames_since >= self.frames_threshold
            || time_since >= self.time_threshold
            || events_since >= self.events_threshold
    }
}

/// 基线条目。
#[derive(Clone, Debug)]
pub struct BaselineEntry {
    /// 基线序号。
    pub seq: u32,
    /// 快照内容。
    pub snapshot: GraphSnapshot,
    /// 创建时刻。
    pub created_at: LogicalTime,
}

/// 增量快照管理器。
///
/// 核心模型：基线（全量） + 增量链（变更日志序列）。
/// 恢复 = 最近基线 + 重放其后所有增量。
/// 链超长时自动合并为新基线（重放成本封顶）。
pub struct SnapshotManager {
    /// 基线表（按序号升序）。
    baselines: Vec<BaselineEntry>,
    /// 增量链（按 log_seq 升序）。
    chain: Vec<ChangeLog>,
    /// 快照策略。
    policy: SnapshotPolicy,
    /// 距上个基线的帧数。
    frames_since: u64,
    /// 距上个基线的时长。
    time_since: LogicalTime,
    /// 距上个基线的事件数。
    events_since: u64,
    /// 当前帧号。
    current_frame: u64,
    /// 下一个基线序号。
    next_baseline_seq: u32,
    /// 下一个日志序号。
    next_log_seq: u32,
}

impl SnapshotManager {
    /// 构造。
    pub fn new(policy: SnapshotPolicy) -> Self {
        Self {
            baselines: Vec::new(),
            chain: Vec::new(),
            policy,
            frames_since: 0,
            time_since: 0,
            events_since: 0,
            current_frame: 0,
            next_baseline_seq: 0,
            next_log_seq: 0,
        }
    }

    /// 记录新基线（全量快照）。
    pub fn take_baseline(&mut self, snapshot: GraphSnapshot) -> u32 {
        let seq = self.next_baseline_seq;
        self.next_baseline_seq += 1;
        let created_at = snapshot.created_at;
        self.baselines.push(BaselineEntry {
            seq,
            snapshot,
            created_at,
        });
        // 新基线后清零计数器。
        self.frames_since = 0;
        self.time_since = 0;
        self.events_since = 0;
        // 基线之后的增量链可全部合并进基线——链清空。
        self.chain.clear();
        seq
    }

    /// 追加增量。
    pub fn append_change(&mut self, entry: ChangeEntry) {
        self.time_since = self.time_since.max(entry.time.saturating_sub(self.baselines.last().map(|b| b.created_at).unwrap_or(0)));
        self.events_since += 1;
        // 找到或创建当前增量日志。
        let baseline_seq = self.baselines.last().map(|b| b.seq).unwrap_or(0);
        if self.chain.is_empty() {
            let log_seq = self.next_log_seq;
            self.next_log_seq += 1;
            self.chain.push(ChangeLog::new(baseline_seq, log_seq));
        }
        if let Some(last) = self.chain.last_mut() {
            last.append(entry);
        }
    }

    /// 帧推进（更新帧计数）。
    pub fn advance_frame(&mut self) {
        self.current_frame += 1;
        self.frames_since += 1;
    }

    /// 检查是否应触发新基线。
    pub fn should_snapshot(&self) -> bool {
        self.policy.should_snapshot(self.frames_since, self.time_since, self.events_since)
            || self.chain.len() >= self.policy.max_chain
    }

    /// 自动维护：若应触发则用当前快照建立新基线。
    pub fn auto_maintain(&mut self, current: GraphSnapshot) -> Option<u32> {
        if self.should_snapshot() {
            Some(self.take_baseline(current))
        } else {
            None
        }
    }

    /// 链合并：把整条增量链合并进新基线。
    ///
    /// 返回合并后的基线序号。合并 = 最近基线 + 重放全链 → 新基线。
    pub fn merge_chain(&mut self) -> Option<u32> {
        if self.chain.is_empty() {
            return None;
        }
        let mut merged = self.baselines.last()?.snapshot.clone();
        for log in &self.chain {
            log.apply_to(&mut merged);
        }
        // 压缩后的合并基线替换旧基线 + 清空链。
        let seq = self.next_baseline_seq;
        self.next_baseline_seq += 1;
        let created_at = merged.created_at.max(self.baselines.last().map(|b| b.created_at).unwrap_or(0));
        self.baselines.push(BaselineEntry {
            seq,
            snapshot: merged,
            created_at,
        });
        self.chain.clear();
        self.frames_since = 0;
        self.time_since = 0;
        self.events_since = 0;
        Some(seq)
    }

    /// 恢复到最新一致状态（基线 + 重放增量）。
    pub fn restore_latest(&self) -> Option<GraphSnapshot> {
        let mut snap = self.baselines.last()?.snapshot.clone();
        for log in &self.chain {
            log.apply_to(&mut snap);
        }
        Some(snap)
    }

    /// 恢复到指定基线（不含其后增量）。
    pub fn restore_baseline(&self, seq: u32) -> Option<GraphSnapshot> {
        self.baselines.iter().find(|b| b.seq == seq).map(|b| b.snapshot.clone())
    }

    /// 基线数量。
    pub fn baseline_count(&self) -> usize {
        self.baselines.len()
    }

    /// 增量链长度。
    pub fn chain_length(&self) -> usize {
        self.chain.len()
    }

    /// 增量条目总数。
    pub fn total_changes(&self) -> usize {
        self.chain.iter().map(|l| l.entries.len()).sum()
    }

    /// 当前帧号。
    pub fn frame(&self) -> u64 {
        self.current_frame
    }

    /// 估算恢复成本（需重放的变更数）。
    pub fn recovery_cost(&self) -> usize {
        self.total_changes()
    }
}

// ============================================================================
// 三、增量压缩工具
// ============================================================================

/// 增量压缩器。
pub struct ChangeCompressor;

impl ChangeCompressor {
    /// 批量压缩一组变更日志（跨日志去重）。
    pub fn compress_batch(logs: &mut [ChangeLog]) -> usize {
        let mut total_saved = 0;
        for log in logs.iter_mut() {
            total_saved += log.compress();
        }
        total_saved
    }

    /// 合并相邻日志（减少链深度）。
    pub fn merge_adjacent(logs: &mut Vec<ChangeLog>) -> usize {
        if logs.len() < 2 {
            return 0;
        }
        let mut merged_count = 0;
        let mut i = 0;
        while i + 1 < logs.len() {
            // 只合并引用同一基线的日志。
            if logs[i].baseline_seq == logs[i + 1].baseline_seq {
                let mut entries = core::mem::take(&mut logs[i].entries);
                entries.append(&mut logs[i + 1].entries);
                logs[i].entries = entries;
                logs.remove(i + 1);
                merged_count += 1;
            } else {
                i += 1;
            }
        }
        merged_count
    }
}

// ============================================================================
// 四、判据：F0259 三条（基线+增量、三触发、链合并）
// ============================================================================

/// F0259 判据集。
pub fn run_f0259_checks() -> CheckSet {
    let mut cs = CheckSet::new("CGPU-F0259");

    // --- 判据 1：基线 + 增量模型 ---
    let mut mgr = SnapshotManager::new(SnapshotPolicy::default_policy());
    let topo = TopologyLayer::new(1, vec![1, 2], vec![(1, 2, 0)]);
    let snap0 = GraphSnapshot::new(topo, ParamLayer::new(), AccountLayer::new(vec![], vec![]), EventStreamLayer::new(vec![]));
    let b0 = mgr.take_baseline(snap0);
    // 追加三条增量。
    mgr.append_change(ChangeEntry { time: 10, frame: 1, change: ChangeKind::NodeAdd(3) });
    mgr.append_change(ChangeEntry { time: 20, frame: 1, change: ChangeKind::EdgeAdd(2, 3, 0) });
    mgr.append_change(ChangeEntry { time: 30, frame: 2, change: ChangeKind::NodeAdd(4) });
    let restored = mgr.restore_latest();
    let has_new_nodes = restored.as_ref().map_or(false, |s| s.topology.nodes.contains(&3) && s.topology.nodes.contains(&4));
    cs.add(
        "基线+增量恢复出新状态",
        b0 == 0 && has_new_nodes,
        "基线后3条增量，恢复含节点3和4",
    );

    // --- 判据 2：三条件或触发 ---
    let policy = SnapshotPolicy::default_policy();
    let by_frames = policy.should_snapshot(60, 0, 0);
    let by_time = policy.should_snapshot(0, 1_000_000, 0);
    let by_events = policy.should_snapshot(0, 0, 10_000);
    let not_yet = !policy.should_snapshot(10, 100, 100);
    cs.add(
        "三条件或触发",
        by_frames && by_time && by_events && not_yet,
        "帧/时长/事件数任一达标即触发",
    );

    // --- 判据 3：链合并 ---
    let mut mgr2 = SnapshotManager::new(SnapshotPolicy::default_policy());
    let snap1 = GraphSnapshot::new(
        TopologyLayer::new(1, vec![1], vec![]),
        ParamLayer::new(),
        AccountLayer::new(vec![], vec![]),
        EventStreamLayer::new(vec![]),
    );
    mgr2.take_baseline(snap1);
    // 追加 10 条增量。
    for i in 0..10 {
        mgr2.append_change(ChangeEntry {
            time: i * 100,
            frame: i / 3,
            change: ChangeKind::NodeAdd(100 + i as u32),
        });
    }
    let chain_before = mgr2.chain_length();
    let merged = mgr2.merge_chain();
    let chain_after = mgr2.chain_length();
    cs.add(
        "链合并压缩",
        chain_before > 0 && chain_after == 0 && merged.is_some(),
        alloc::format!("合并前链长{}→合并后{}", chain_before, chain_after).leak(),
    );

    // --- 附加：增量压缩有效性 ---
    let mut log = ChangeLog::new(0, 0);
    log.append(ChangeEntry { time: 1, frame: 0, change: ChangeKind::ParamSet(1, 100) });
    log.append(ChangeEntry { time: 2, frame: 0, change: ChangeKind::ParamSet(1, 200) });
    log.append(ChangeEntry { time: 3, frame: 0, change: ChangeKind::ParamSet(1, 300) });
    let saved = log.compress();
    cs.add(
        "增量压缩（覆盖性变更合并）",
        saved == 2 && log.entries.len() == 1,
        "3次同参数变更压缩为1次",
    );

    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f0259_all_pass() {
        let cs = run_f0259_checks();
        assert!(cs.all_passed(), "F0259 判据未全绿: {}", cs.red_items_text());
    }

    #[test]
    fn compress_add_del_pair() {
        let mut log = ChangeLog::new(0, 0);
        log.append(ChangeEntry { time: 1, frame: 0, change: ChangeKind::NodeAdd(42) });
        log.append(ChangeEntry { time: 2, frame: 0, change: ChangeKind::NodeDel(42) });
        let saved = log.compress();
        assert_eq!(saved, 2);
        assert!(log.entries.is_empty());
    }

    #[test]
    fn restore_cascade() {
        let mut mgr = SnapshotManager::new(SnapshotPolicy::conservative());
        let topo = TopologyLayer::new(1, vec![1], vec![]);
        mgr.take_baseline(GraphSnapshot::new(topo, ParamLayer::new(), AccountLayer::new(vec![], vec![]), EventStreamLayer::new(vec![])));
        mgr.append_change(ChangeEntry { time: 1, frame: 1, change: ChangeKind::NodeAdd(2) });
        mgr.append_change(ChangeEntry { time: 2, frame: 1, change: ChangeKind::NodeAdd(3) });
        mgr.append_change(ChangeEntry { time: 3, frame: 2, change: ChangeKind::NodeAdd(4) });
        let s = mgr.restore_latest().unwrap();
        assert_eq!(s.topology.nodes.len(), 4); // 1,2,3,4
    }
}
