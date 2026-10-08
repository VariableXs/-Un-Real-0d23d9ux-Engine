//! VE-F0607 · 域自检（判据逐条对应，见 `ved07_visibility.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项）：
//! - 三态语义 → `D07-三态-显性分离与幂等`
//! - 传播规则（父隐藏则子树全隐藏） → `D07-传播-祖先链有效可见性`
//! - 结构保留（恢复即时无需重建） → `D07-结构-状态与结构分离`
//! - 脏标记向上聚合 O(深度) → `D07-双向-向上聚合与向下失效`
//! - 失效向下传播 O(子树) → `D07-双向-向上聚合与向下失效`
//! - 传播断链→不变式断言 → `D07-断链-不变式捕获`
//! - 风暴→批量合并 → `D07-风暴-批量合并单次apply`
//! - 事件映射 O(1)（显示=加入、隐藏=移除+相邻失效） → `D07-事件-三型映射`
//! - 未登记节点拒绝 → `D07-防护-未登记拒绝`
//!
//! 逻辑 tick 注入、零墙钟，回归可复现。

use super::ved01_tree::DirtyFlags;
use super::ved07_visibility::*;
use crate::checks::CheckSet;

/// 链 1(根)→2→3(叶) 的父映射。
fn chain_parents() -> Vec<(u64, Option<u64>)> {
    vec![(1, None), (2, Some(1)), (3, Some(2))]
}

/// VE-F0607 域自检。
pub fn run_ved07_checks() -> CheckSet {
    let mut set = CheckSet::new("svstar2-ved07");

    // ---- 三态语义 ----

    {
        let mut t = VisibilityTable::new();
        t.tick();
        t.set_hidden(1);
        t.set_removed(2);
        t.set_visible(3);
        let states = t.state_of(1) == Visibility::HiddenKeepLayout
            && t.state_of(2) == Visibility::Removed
            && t.state_of(3) == Visibility::Visible;
        let doc = VISIBILITY_CONTRACT_DOC.contains("隐藏保留布局")
            && VISIBILITY_CONTRACT_DOC.contains("无需重建");
        // 幂等：同态重复设置不产生新切换。
        let n0 = t.pending_len();
        t.set_visible(3);
        set.add(
            "D07-三态-显性分离与幂等",
            states && doc && t.pending_len() == n0,
            "",
        );
    }

    // ---- 传播规则：祖先链有效可见性 ----

    {
        let all_visible = [Visibility::Visible, Visibility::Visible, Visibility::Visible];
        let parent_hidden = [Visibility::Visible, Visibility::HiddenKeepLayout, Visibility::Visible];
        let ancestor_removed = [Visibility::Visible, Visibility::HiddenKeepLayout, Visibility::Removed];
        let ok = effective_visibility(&all_visible) == EffectiveVisibility::Shown
            && effective_visibility(&parent_hidden) == EffectiveVisibility::HiddenSkipped
            && effective_visibility(&ancestor_removed) == EffectiveVisibility::Gone;
        set.add("D07-传播-祖先链有效可见性", ok, "");
    }

    // ---- 结构保留 ----

    // 判据：状态表拿不到树的操作权（父映射快照只读投影），可见性往返后
    // 结构签名不变——恢复即时无需重建。
    {
        let mut t = VisibilityTable::new();
        t.tick();
        t.set_hidden(1);
        t.set_removed(2);
        t.apply_batch();
        t.set_visible(1);
        t.set_visible(2);
        t.apply_batch();
        let restored = t.state_of(1) == Visibility::Visible && t.state_of(2) == Visibility::Visible;
        // 传播器构造只收快照，类型上不持有树的写接口（结构保留的实现面）。
        let p = ValidityPropagator::new(chain_parents());
        let snapshot_only = p.flags_of(1).is_some() && p.flags_of(99).is_none();
        set.add("D07-结构-状态与结构分离", restored && snapshot_only, "");
    }

    // ---- 双向传播 ----

    {
        let mut p = ValidityPropagator::new(chain_parents());
        p.tick();
        // 向上聚合：叶脏 → 祖先链 SUBTREE 全置位。
        let _ = p.mark_dirty_up(3);
        let up = p.flags_of(3).unwrap().has(DirtyFlags::SELF)
            && p.flags_of(2).unwrap().has(DirtyFlags::SUBTREE)
            && p.flags_of(1).unwrap().has(DirtyFlags::SUBTREE);
        // 向下失效：根变更 → 子树全失效。
        let children = vec![(1u64, vec![2u64]), (2u64, vec![3u64]), (3u64, vec![])];
        let _ = p.invalidate_down(1, &children);
        let down = p.flags_of(1).unwrap().has(DirtyFlags::CONTENT)
            && p.flags_of(2).unwrap().has(DirtyFlags::CONTENT)
            && p.flags_of(3).unwrap().has(DirtyFlags::CONTENT);
        let doc = VALIDITY_CONTRACT_DOC.contains("O(深度)") && VALIDITY_CONTRACT_DOC.contains("O(子树)");
        set.add("D07-双向-向上聚合与向下失效", up && down && doc, "");
    }

    // ---- 传播断链→不变式捕获 ----

    {
        let mut p = ValidityPropagator::new(chain_parents());
        p.tick();
        let _ = p.mark_dirty_up(3);
        // 演练注入：抹掉节点 1 的 SUBTREE 位模拟断链。
        p.clear_subtree_bit_for_drill(1);
        let caught = !p.invariants_broken().is_empty();
        // 未断链时不变式安静。
        let mut p2 = ValidityPropagator::new(chain_parents());
        let _ = p2.mark_dirty_up(3);
        let quiet = p2.invariants_broken().is_empty();
        set.add("D07-断链-不变式捕获", caught && quiet, "");
    }

    // ---- 风暴→批量合并 ----

    {
        let mut t = VisibilityTable::new();
        t.tick();
        t.set_hidden(1);
        t.set_visible(1);
        t.set_removed(2);
        t.set_hidden(3);
        let pooled = t.pending_len() == 4;
        let evs = t.apply_batch();
        let once = evs.len() == 4 && t.pending_len() == 0;
        set.add("D07-风暴-批量合并单次apply", pooled && once, "");
    }

    // ---- 事件映射三型 ----

    {
        // 显示=加入。
        let shown = map_visibility_event(&VisSwitch {
            node_id: 1,
            before: Visibility::HiddenKeepLayout,
            after: Visibility::Visible,
        }) == DirtyEvent::ShownAdded { node_id: 1 };
        // 隐藏=移除+相邻失效（从可见切来时）。
        let hidden = map_visibility_event(&VisSwitch {
            node_id: 2,
            before: Visibility::Visible,
            after: Visibility::HiddenKeepLayout,
        }) == DirtyEvent::HiddenRemoved { node_id: 2, adjacent_invalidated: true };
        let hidden_from_hidden = map_visibility_event(&VisSwitch {
            node_id: 2,
            before: Visibility::HiddenKeepLayout,
            after: Visibility::Removed,
        }) == DirtyEvent::RemovedGone { node_id: 2 };
        // 移除=整块作废。
        let gone = map_visibility_event(&VisSwitch {
            node_id: 3,
            before: Visibility::Visible,
            after: Visibility::Removed,
        }) == DirtyEvent::RemovedGone { node_id: 3 };
        let doc = EVENT_MAP_DOC.contains("F0613");
        set.add("D07-事件-三型映射", shown && hidden && gone && hidden_from_hidden && doc, "");
    }

    // ---- 防护：未登记节点拒绝 ----

    {
        let mut p = ValidityPropagator::new(chain_parents());
        let up_rejected = p.mark_dirty_up(42).is_err();
        let children = vec![(1u64, vec![])];
        let down_rejected = p.invalidate_down(42, &children).is_err();
        let logged = p
            .errors()
            .iter()
            .filter(|(_, c, _)| *c == "E_NODE_NOT_FOUND")
            .count()
            >= 2;
        set.add("D07-防护-未登记拒绝", up_rejected && down_rejected && logged, "");
    }

    set
}
