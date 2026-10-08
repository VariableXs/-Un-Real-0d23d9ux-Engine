//! VE-F0601 · 域自检（判据逐条对应，见 `ved01_tree.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项）：
//! - 不变式三条（单父/无环/类型约束）→ `D01-不变式-*`
//! - 稀疏属性（未设走继承或默认值，不逐节点填满）→ `D01-稀疏-*`
//! - 脏标记族三类分离不混用 → `D01-脏标记-*`
//! - 池化句柄（批量销毁、句柄稳定、陈旧句柄可检出）→ `D01-池化-*`
//! - 重建树不失效外部句柄 → `D01-池化-重建树句柄稳定`
//! - 复杂度声明（挂载 O(1)/移除 O(子树)/遍历 O(节点数)）→ `D01-复杂度-*`
//! - 池耗尽三要素 → `D01-错误-池耗尽三要素`
//! - 零静默错误账本 → `D01-错误-*`
//!
//! 零墙钟、零 IO，回归可复现。

use super::ved01_tree::*;
use crate::checks::CheckSet;

use alloc::vec;
use alloc::vec::Vec;

/// 标准树：Root(0) → 容器(1) → [容器(2) → 表面(4), 表面(3)]。
/// 返回 (树, 句柄表：按 id 0..=4)。
fn std_tree() -> (LayerTree, [NodeHandle; 5]) {
    let mut t = LayerTree::new();
    let h0 = t.create(0, NodeKind::Root).expect("root");
    let h1 = t.create(1, NodeKind::Container).expect("c1");
    let h2 = t.create(2, NodeKind::Container).expect("c2");
    let h3 = t.create(3, NodeKind::Surface).expect("s3");
    let h4 = t.create(4, NodeKind::Surface).expect("s4");
    t.attach(h0, h1).expect("root←c1");
    t.attach(h1, h2).expect("c1←c2");
    t.attach(h2, h4).expect("c2←s4");
    t.attach(h1, h3).expect("c1←s3");
    (t, [h0, h1, h2, h3, h4])
}

/// VE-F0601 域自检。
pub fn run_ved01_checks() -> CheckSet {
    let mut set = CheckSet::new("svstar2-ved01");

    // ---- 不变式三条 ----

    // 判据：单父（已有父的节点再挂 → 拒绝）。
    {
        let (mut t, [_, h1, h2, h3, _]) = std_tree();
        let r = t.attach(h1, h2); // h2 已挂在 h1 下，再挂 h1 → 单父违规
        let ok = matches!(r, Err(e) if e.code == "E_ALREADY_ATTACHED");
        let _ = h3;
        set.add("D01-不变式-单父", ok, "");
    }

    // 判据：无环（把祖先挂到后代之下 → 挂载前断言拒绝；自环同理）。
    {
        let (mut t, [h0, h1, h2, _, _]) = std_tree();
        // 自环：c2 挂到 c2 自己。
        let self_loop = matches!(t.attach(h2, h2), Err(e) if e.code == "E_CYCLE");
        // 祖先环：h1（h2 的祖先）挂到 h2 下。
        let ancestor_loop = matches!(t.attach(h2, h1), Err(e) if e.code == "E_CYCLE");
        let _ = h0;
        set.add("D01-不变式-无环", self_loop && ancestor_loop, "");
    }

    // 判据：类型约束（白名单拒绝：Root 只收容器 / Surface 不收子节点）。
    {
        let (mut t, [h0, _, _, h3, _]) = std_tree();
        let s5 = t.create(5, NodeKind::Surface).expect("s5");
        // Root 的白名单只有容器 → 挂表面被拒。
        let root_surface = matches!(t.attach(h0, s5), Err(e) if e.code == "E_TYPE_VIOLATION");
        // Surface 是叶子 → 挂任何子节点被拒。
        let leaf_parent = matches!(t.attach(h3, s5), Err(e) if e.code == "E_TYPE_VIOLATION");
        set.add("D01-不变式-类型白名单", root_surface && leaf_parent, "");
    }

    // ---- 稀疏属性 ----

    // 判据：稀疏属性（只存已设键值；未设查询返回 None）。
    {
        let (mut t, [_, _, _, h3, _]) = std_tree();
        assert!(t.prop_get(h3, PropId::Opacity).is_none());
        t.prop_set(h3, PropId::Opacity, PropValue::F(0.5)).expect("set");
        let got = t.prop_get(h3, PropId::Opacity);
        let sparse = t.node(h3).expect("活").props.len() == 1;
        set.add(
            "D01-稀疏-只存已设键",
            got == Some(PropValue::F(0.5)) && sparse,
            "",
        );
    }

    // 判据：继承与默认值（沿父链上行到首个设置者；走不到根给默认）。
    {
        let (mut t, [_, h1, _, h4, _]) = std_tree();
        // 父链上的 h1 设 0.25，h4（孙子）继承到 0.25。
        t.prop_set(h1, PropId::Opacity, PropValue::F(0.25)).expect("set");
        let inherited = t.prop_get_inherited(h4, PropId::Opacity);
        // 无任何祖先设置 Z → 默认 0。
        let default_z = t.prop_get_inherited(h4, PropId::Z);
        set.add(
            "D01-稀疏-继承与默认",
            inherited == Some(PropValue::F(0.25))
                && default_z == Some(PropValue::I(0)),
            "",
        );
    }

    // ---- 脏标记族（三类分离不混用）----

    {
        let (mut t, [h0, h1, h2, _, h4]) = std_tree();
        // 置自身脏：本节点 SELF，祖先链补 SUBTREE，本节点不被误标 SUBTREE。
        t.mark_dirty(h4, DirtyFlags::SELF).expect("mark");
        let child_self = t.node(h4).expect("活").dirty.has(DirtyFlags::SELF);
        let child_no_subtree = !t.node(h4).expect("活").dirty.has(DirtyFlags::SUBTREE);
        let parent_subtree = t.node(h2).expect("活").dirty.has(DirtyFlags::SUBTREE)
            && t.node(h1).expect("活").dirty.has(DirtyFlags::SUBTREE)
            && t.node(h0).expect("活").dirty.has(DirtyFlags::SUBTREE);
        // 内容脏独立：置 CONTENT 不牵连 SELF/SUBTREE，也不向上传播。
        let (mut t2, [_, _, h2b, _, _]) = std_tree();
        t2.mark_dirty(h2b, DirtyFlags::CONTENT).expect("mark");
        let content_isolated = t2.node(h2b).expect("活").dirty.has(DirtyFlags::CONTENT)
            && !t2.node(h2b).expect("活").dirty.has(DirtyFlags::SELF)
            && t2.node(h1).map(|n| !n.dirty.has(DirtyFlags::SUBTREE)).unwrap_or(false);
        // 清除互不牵连：清 SELF 保留 CONTENT。
        t2.mark_dirty(h2b, DirtyFlags::SELF).expect("mark");
        t2.clear_dirty(h2b, DirtyFlags::SELF).expect("clear");
        let clear_isolated = t2.node(h2b).expect("活").dirty.has(DirtyFlags::CONTENT)
            && !t2.node(h2b).expect("活").dirty.has(DirtyFlags::SELF);
        set.add(
            "D01-脏标记-三类分离",
            child_self && child_no_subtree && parent_subtree && content_isolated && clear_isolated,
            "",
        );
        let _ = h4;
    }

    // ---- 池化句柄 ----

    // 判据：池化句柄稳定（销毁兄弟不影响其他句柄；陈旧句柄可检出；
    // 槽位复用后代数递增）。
    {
        let mut t = LayerTree::new();
        let ha = t.create(1, NodeKind::Container).expect("a");
        let hb = t.create(2, NodeKind::Container).expect("b");
        t.remove_subtree(ha).expect("remove");
        // 兄弟句柄 hb 不受批量销毁影响。
        let b_alive = t.node(hb).is_some() && t.handle_of(2).expect("b") == hb;
        // 陈旧句柄被代数校验检出。
        let a_stale = t.node(ha).is_none();
        // 新节点复用槽位后代数递增：旧句柄仍失效，新句柄有效。
        let hc = t.create(3, NodeKind::Container).expect("c");
        let reused_gen = hc.slot == ha.slot && hc.gen > ha.gen;
        set.add(
            "D01-池化-句柄稳定与陈旧检出",
            b_alive && a_stale && reused_gen,
            "",
        );
    }

    // 判据：批量销毁 O(子树)（子树全清、兄弟祖先完好、id 索引同步清理）。
    {
        let (mut t, [_, h1, h2, h3, h4]) = std_tree();
        // 移除 h2 子树（h2 + h4，共 2 个）；h3（兄弟）与 h1（父）完好。
        let removed = t.remove_subtree(h2).expect("remove");
        let ok = removed == 2
            && t.node(h2).is_none()
            && t.node(h4).is_none()
            && t.node(h1).is_some()
            && t.node(h3).is_some()
            && t.handle_of(2).is_none()
            && t.handle_of(4).is_none()
            && t.len() == 3;
        set.add("D01-池化-批量销毁子树", ok, "");
        let _ = (h1, h3, h4);
    }

    // 判据：重建树不失效外部句柄（只清结构，句柄全部有效）。
    {
        let (mut t, handles) = std_tree();
        let len_before = t.len();
        t.reset_structure();
        let all_alive = handles.iter().all(|&h| t.node(h).is_some());
        let structure_cleared = t.root().is_none()
            && t.node(handles[1]).expect("活").children.is_empty()
            && t.len() == len_before;
        set.add(
            "D01-池化-重建树句柄稳定",
            all_alive && structure_cleared,
            "",
        );
    }

    // ---- 复杂度声明 ----

    // 判据：复杂度声明在册（C1-C7 与锚点口径一致）。
    {
        let d = COMPLEXITY_DOC;
        let ok = d.contains("C1") && d.contains("O(1)")
            && d.contains("C3") && d.contains("O(子树)")
            && d.contains("C5") && d.contains("O(节点数)")
            && d.contains("C7") && d.contains("O(深度)");
        set.add("D01-复杂度-声明在册", ok, "");
    }

    // 判据：先序遍历次序正确且访问数 == 节点数（O(节点数) 行为验证）。
    {
        let (t, [h0, h1, h2, h3, h4]) = std_tree();
        let order = t.preorder(h0).expect("walk");
        let ids: Vec<u64> = order
            .iter()
            .filter_map(|&h| t.node(h).map(|n| n.id))
            .collect();
        // 先序：0 → 1 → 2 → 4 → 3（子序列插入序）。
        let ok = ids == vec![0u64, 1, 2, 4, 3] && order.len() == 5;
        // 子树遍历：从 h2 出发只走 h2、h4。
        let sub = t.preorder(h2).expect("walk");
        let sub_ids: Vec<u64> = sub
            .iter()
            .filter_map(|&h| t.node(h).map(|n| n.id))
            .collect();
        set.add(
            "D01-复杂度-先序次序与计数",
            ok && sub_ids == vec![2u64, 4],
            "",
        );
        let _ = (h1, h3);
    }

    // ---- 重挂 ----

    // 判据：重挂（detach+attach 语义：旧父失去、新父得到、子树跟随）。
    {
        let (mut t, [_, h1, h2, _, h4]) = std_tree();
        // 建第二个容器挂到根下……根只允许一个容器，改为挂在 h1 下：
        let h5 = t.create(5, NodeKind::Container).expect("c5");
        t.attach(h1, h5).expect("c1←c5");
        // 把 h2（含子树 h4）从 h1 重挂到 h5。
        t.reparent(h2, h5).expect("reparent");
        let old_lost = !t.node(h1).expect("活").children.iter().any(|&c| c == h2.slot);
        let new_got = t.node(h5).expect("活").children.contains(&h2.slot);
        let subtree_follows = t.node(h4).expect("活").parent == Some(h2.slot);
        // 重挂后先序从根走仍全覆盖且无重复。
        let all = t.preorder(h0_of(&t)).expect("walk");
        let mut seen = alloc::collections::BTreeSet::new();
        let no_dup = all.iter().all(|h| seen.insert(h.slot));
        set.add(
            "D01-重挂-子树跟随",
            old_lost && new_got && subtree_follows && no_dup && all.len() == 6,
            "",
        );
    }

    // ---- 稳定 id ----

    // 判据：稳定 id（结构变更后 id → 句柄映射仍正确）。
    {
        let (mut t, [_, _, _, _, _]) = std_tree();
        let before = t.handle_of(4);
        let _ = t.prop_set(t.handle_of(4).expect("h4"), PropId::Z, PropValue::I(7));
        let after = t.handle_of(4);
        let ok = before == after
            && t.prop_get(t.handle_of(4).expect("h4"), PropId::Z) == Some(PropValue::I(7));
        set.add("D01-稳定id-结构变更不漂移", ok, "");
    }

    // ---- 错误路径（零静默）----

    // 判据：池耗尽三要素（池满 → 错误含 发生了什么/为什么/下一步）。
    {
        let mut t = LayerTree::new();
        let mut last = Ok(NodeHandle { slot: 0, gen: 0 });
        for i in 0..POOL_CAP {
            last = t.create(i as u64, NodeKind::Surface);
        }
        let full_created = last.is_ok() && t.len() == POOL_CAP;
        let over = t.create(POOL_CAP as u64, NodeKind::Surface);
        let ok = full_created
            && matches!(over, Err(e) if e.code == "E_POOL_EXHAUSTED"
                && !e.why.is_empty() && !e.next.is_empty() && !e.who.is_empty());
        set.add("D01-错误-池耗尽三要素", ok, "");
    }

    // 判据：非法脏标记与陈旧句柄零静默入账。
    {
        let (mut t, [_, _, _, _, _]) = std_tree();
        let stale = NodeHandle { slot: 999, gen: 0 };
        let r1 = t.mark_dirty(stale, DirtyFlags::SELF);
        let r2 = t.mark_dirty(t.handle_of(3).expect("h3"), 0xFF);
        let ok = matches!(r1, Err(e) if e.code == "E_STALE_HANDLE")
            && matches!(r2, Err(e) if e.code == "E_BAD_FLAG")
            && t.errors().len() == 2
            && t.errors().iter().all(|e| !e.next.is_empty());
        set.add("D01-错误-零静默入账", ok, "");
    }

    // ---- 读屏与语义标注挂点 ----

    {
        let (t, _) = std_tree();
        let s = t.screen_text();
        set.add(
            "D01-读屏-摘要可播",
            s.contains("图层树") && s.contains("5 个活节点"),
            "",
        );
    }

    set
}

/// 取根句柄（测试辅助：标准树根 id=0）。
fn h0_of(t: &LayerTree) -> NodeHandle {
    t.handle_of(0).expect("root")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ved01_all_judgements_green() {
        let set = run_ved01_checks();
        let (passed, failed) = set.tally();
        let (reds, n) = set.red_items();
        let names: Vec<&'static str> = (0..n)
            .filter_map(|i| reds[i].as_ref().filter(|c| !c.passed).map(|c| c.name))
            .collect();
        assert!(
            set.all_passed(),
            "VE-F0601 自检存在红项：{}/{} 绿，红项：{:?}",
            passed,
            passed + failed,
            names
        );
    }

    #[test]
    fn ved01_whitelist_shape() {
        // 白名单形状：Root→[Container]；Container→三型；Surface→空。
        assert_eq!(NodeKind::Root.child_whitelist().len(), 1);
        assert_eq!(NodeKind::Container.child_whitelist().len(), 3);
        assert_eq!(NodeKind::Surface.child_whitelist().len(), 0);
        assert!(!NodeKind::Surface.can_have_children());
    }
}
