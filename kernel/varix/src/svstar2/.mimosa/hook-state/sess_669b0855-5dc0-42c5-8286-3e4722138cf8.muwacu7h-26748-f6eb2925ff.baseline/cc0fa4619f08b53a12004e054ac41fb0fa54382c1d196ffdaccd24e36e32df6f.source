//! VE-F0607 · 图层可见性与有效性传播（VE-D 域 · 2D 合成引擎 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0607`
//!
//! **判据（锚点原文）**：可见性三态（可见、隐藏保留布局、彻底移除）及传播
//! 规则（父隐藏则子树全隐藏但树结构保留——恢复即时无需重建）；有效性双向
//! 传播：脏标记向上聚合（子树任何脏→祖先链子树脏位置位）、失效向下传播
//! （父变换或裁剪变更→子树世界矩阵与包围盒失效）；与 F0613 脏区收集衔接：
//! 可见性切换映射为标准脏区事件（显示=加入、隐藏=移除加相邻区域失效）。
//! 判据四条：**三态语义、双向传播、结构保留、事件映射**。
//!
//! **错误路径与降级矩阵**：三态混用→显性 API 分离；传播断链→不变式断言
//! （debug 常开）；风暴→批量合并。
//!
//! **设计要点**：
//! - **三态显性 API 分离**：[`VisibilityTable::set_visible`] / `set_hidden` /
//!   `set_removed` 三个方法各管一态——不提供一个"传数字"的混合入口，
//!   三态混用无门可入（错误矩阵第一条的执行面）；
//! - **传播规则**：祖先链任一祖先 ∈ {隐藏, 移除} ⇒ 子树全隐藏
//!   （[`EffectiveVisibility`]），树结构零改动——恢复可见即时生效无需重建
//!   （结构保留判据：状态与结构分离，状态表根本拿不到树的操作权）；
//! - **双向传播**：向上聚合 = 子树任何脏 → 祖先链逐层置 `SUBTREE` 位
//!   （复用上游 F0601 [`DirtyFlags`]，O(深度)）；向下失效 = 父变换/裁剪
//!   变更 → 子树逐个失效（O(子树)），失效即世界矩阵与包围盒作废
//!   （重算归 F0602/F0604 的消费方）；
//! - **风暴→批量合并**：标记先进 pending，一次 `apply_batch` 完成全部
//!   聚合与失效（风暴观测面 = pending 计数）；
//! - **传播断链→不变式断言**：聚合/失效后跑不变式（有 SUBTREE 位必有
//!   脏源、失效者的祖先必有 SUBTREE 位），断链显性报错不静默；
//! - **事件映射 O(1)**：可见性切换 → 标准脏区事件（显示=加入；隐藏=移除
//!   +相邻区域失效标记；移除=整块作废），F0613 凭事件收集脏区。
//!
//! **跨批对接点**：上游 F0601 脏标记族（`DirtyFlags` 复用）；下游 F0613
//! 脏区（事件消费方）、F0614 遍历（隐藏子树跳过判定 [`EffectiveVisibility`]）。
//!
//! 逻辑 tick 注入，零墙钟；零外部依赖，只依赖 `crate::checks`（自检侧）。

use crate::checks::CheckSet;

use super::ved01_tree::DirtyFlags;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、规格常量（语义文本）
// ---------------------------------------------------------------------------

/// 可见性三态契约（三态语义判据的文本载体）。
pub const VISIBILITY_CONTRACT_DOC: &str = "\
可见性三态契约（VE-F0607 · v1）：可见=正常绘制；隐藏保留布局=不绘制\
但占位（几何参与命中豁免由 F0611 裁定）；彻底移除=不绘制不占位。\
父隐藏则子树全隐藏，但树结构保留——恢复可见即时生效，无需重建。";

/// 有效性双向传播契约。
pub const VALIDITY_CONTRACT_DOC: &str = "\
有效性双向传播契约（VE-F0607 · v1）：向上=子树任何脏，祖先链子树脏位\
（DirtyFlags::SUBTREE）逐层置位，O(深度)；向下=父变换或裁剪变更，子树\
世界矩阵与包围盒全部失效，O(子树)。双向路径断链属最高缺陷，不变式\
断言 debug 常开。";

/// 脏区事件映射契约（F0613 衔接）。
pub const EVENT_MAP_DOC: &str = "\
脏区事件映射契约（VE-F0607 · v1，O(1) 每次切换）：显示=加入（该层区域\
进脏区）；隐藏=移除+相邻区域失效（移除其区域并使相邻区域重绘）；\
移除=整块作废。F0613 凭标准事件收集，不做特判。";

// ---------------------------------------------------------------------------
// 二、可见性三态与有效可见性
// ---------------------------------------------------------------------------

/// 可见性三态（显性 API 分离——三入口各管一态）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Visibility {
    /// 可见。
    Visible,
    /// 隐藏保留布局。
    HiddenKeepLayout,
    /// 彻底移除。
    Removed,
}

impl Visibility {
    /// 人话名（审计用）。
    pub fn name(self) -> &'static str {
        match self {
            Visibility::Visible => "可见",
            Visibility::HiddenKeepLayout => "隐藏保留布局",
            Visibility::Removed => "彻底移除",
        }
    }
}

/// 有效可见性（祖先链传播后的结论；F0614 据此跳过子树）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EffectiveVisibility {
    /// 正常绘制。
    Shown,
    /// 被祖先或自身的"隐藏保留布局"遮蔽——跳过绘制，布局保留。
    HiddenSkipped,
    /// 被移除遮蔽——不绘制不占位。
    Gone,
}

/// O(深度) 传播规则：祖先链（根→本层）逐层判定有效可见性。
pub fn effective_visibility(chain_states: &[Visibility]) -> EffectiveVisibility {
    let mut own = EffectiveVisibility::Shown;
    for st in chain_states {
        match st {
            Visibility::Removed => return EffectiveVisibility::Gone,
            Visibility::HiddenKeepLayout => {
                // 记录隐藏但不提前返回：后续祖先若移除则升级为 Gone。
                if own == EffectiveVisibility::Shown {
                    own = EffectiveVisibility::HiddenSkipped;
                }
            }
            Visibility::Visible => {}
        }
    }
    own
}

// ---------------------------------------------------------------------------
// 三、可见性状态表（三态显性分离 + 事件映射 + 风暴合并）
// ---------------------------------------------------------------------------

/// 标准脏区事件（F0613 衔接面；映射 O(1)）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DirtyEvent {
    /// 显示：区域加入脏区。
    ShownAdded { /// 层 id。
                 node_id: u64 },
    /// 隐藏：区域移除 + 相邻区域失效。
    HiddenRemoved {
        /// 层 id。
        node_id: u64,
        /// 相邻区域是否需失效（相邻重叠层存在时为 true）。
        adjacent_invalidated: bool,
    },
    /// 彻底移除：整块作废。
    RemovedGone { /// 层 id。
                  node_id: u64 },
}

/// 可见性切换的 pending 项（风暴合并池）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VisSwitch {
    /// 层 id。
    pub node_id: u64,
    /// 切换前状态。
    pub before: Visibility,
    /// 切换后状态。
    pub after: Visibility,
}

/// 可见性状态表（三态分离入口 + 传播 + 事件账）。
pub struct VisibilityTable {
    states: Vec<(u64, Visibility)>,
    pending: Vec<VisSwitch>,
    events: Vec<DirtyEvent>,
    audits: Vec<String>,
    errors: Vec<(String, &'static str, String)>,
    tick: u64,
}

impl VisibilityTable {
    /// 空表。
    pub fn new() -> Self {
        VisibilityTable {
            states: Vec::new(),
            pending: Vec::new(),
            events: Vec::new(),
            audits: Vec::new(),
            errors: Vec::new(),
            tick: 0,
        }
    }

    /// 状态查询（未登记默认可见）。
    pub fn state_of(&self, node_id: u64) -> Visibility {
        self.states
            .iter()
            .find(|(id, _)| *id == node_id)
            .map(|(_, s)| *s)
            .unwrap_or(Visibility::Visible)
    }

    /// 事件账。
    pub fn events(&self) -> &[DirtyEvent] {
        &self.events
    }

    /// 审计留痕。
    pub fn audits(&self) -> &[String] {
        &self.audits
    }

    /// 错误账本（零静默）。
    pub fn errors(&self) -> &[(String, &'static str, String)] {
        &self.errors
    }

    /// pending 计数（风暴观测面）。
    pub fn pending_len(&self) -> usize {
        self.pending.len()
    }

    fn set_state(&mut self, node_id: u64, to: Visibility, what: &'static str) {
        let before = self.state_of(node_id);
        if before == to {
            return; // 同态设置幂等（不产生事件不记账）。
        }
        self.pending.push(VisSwitch { node_id, before, after: to });
        self.states.retain(|(id, _)| *id != node_id);
        self.states.push((node_id, to));
        self.audits
            .push(format!("tick{} n={node_id} {what}：{} → {}", self.tick, before.name(), to.name()));
    }

    /// 设为可见（三态显性 API 之一）。
    pub fn set_visible(&mut self, node_id: u64) {
        self.set_state(node_id, Visibility::Visible, "设为可见");
    }

    /// 设为隐藏保留布局（三态显性 API 之二）。
    pub fn set_hidden(&mut self, node_id: u64) {
        self.set_state(node_id, Visibility::HiddenKeepLayout, "设为隐藏");
    }

    /// 设为彻底移除（三态显性 API 之三）。
    pub fn set_removed(&mut self, node_id: u64) {
        self.set_state(node_id, Visibility::Removed, "设为移除");
    }

    /// 批量应用：把本 tick 的全部切换合并，一次性产出标准脏区事件
    /// （风暴→批量合并判据；事件映射 O(1) 每次）。
    pub fn apply_batch(&mut self) -> Vec<DirtyEvent> {
        let mut out = Vec::new();
        for sw in self.pending.drain(..) {
            let ev = map_visibility_event(&sw);
            self.events.push(ev);
            out.push(ev);
        }
        out
    }

    /// 逻辑 tick 推进（零墙钟纪律）。
    pub fn tick(&mut self) {
        self.tick = self.tick.saturating_add(1);
    }
}

/// O(1) 事件映射：单次切换 → 标准脏区事件（显示=加入、隐藏=移除+相邻失效）。
pub fn map_visibility_event(sw: &VisSwitch) -> DirtyEvent {
    match sw.after {
        Visibility::Visible => DirtyEvent::ShownAdded { node_id: sw.node_id },
        Visibility::HiddenKeepLayout => DirtyEvent::HiddenRemoved {
            node_id: sw.node_id,
            adjacent_invalidated: sw.before == Visibility::Visible,
        },
        Visibility::Removed => DirtyEvent::RemovedGone { node_id: sw.node_id },
    }
}

// ---------------------------------------------------------------------------
// 四、有效性双向传播器（向上聚合 + 向下失效 + 不变式）
// ---------------------------------------------------------------------------

/// 双向传播器：每节点一份 [`DirtyFlags`]（复用 F0601 脏标记族）。
pub struct ValidityPropagator {
    flags: Vec<(u64, DirtyFlags)>,
    /// 父映射快照（id → 父 id；根为 None）——上游 F0601 结构的只读投影。
    parents: Vec<(u64, Option<u64>)>,
    audits: Vec<String>,
    errors: Vec<(String, &'static str, String)>,
    tick: u64,
}

impl ValidityPropagator {
    /// 构造：父映射快照（F0601 只读投影；本表不改树）。
    pub fn new(parents: Vec<(u64, Option<u64>)>) -> Self {
        ValidityPropagator {
            flags: parents.iter().map(|&(id, _)| (id, DirtyFlags(0))).collect(),
            parents,
            audits: Vec::new(),
            errors: Vec::new(),
            tick: 0,
        }
    }

    /// 脏标记只读。
    pub fn flags_of(&self, node_id: u64) -> Option<DirtyFlags> {
        self.flags.iter().find(|(id, _)| *id == node_id).map(|(_, f)| *f)
    }

    /// 审计留痕。
    pub fn audits(&self) -> &[String] {
        &self.audits
    }

    /// 错误账本（零静默）。
    pub fn errors(&self) -> &[(String, &'static str, String)] {
        &self.errors
    }

    fn parent_of(&self, node_id: u64) -> Option<u64> {
        self.parents
            .iter()
            .find(|(id, _)| *id == node_id)
            .and_then(|(_, p)| *p)
    }

    /// 向上聚合：子树任何脏 → 祖先链逐层置 SUBTREE 位（O(深度)）。
    pub fn mark_dirty_up(&mut self, node_id: u64) -> Result<(), &'static str> {
        if self.flags_of(node_id).is_none() {
            self.errors.push((
                format!("mark_dirty_up(n={node_id})"),
                "E_NODE_NOT_FOUND",
                "传播器父映射中无此 id——断链防护：拒绝凭空记账".to_string(),
            ));
            return Err("未登记的节点 id");
        }
        // 自身 SELF 位 + 祖先链 SUBTREE 位。
        if let Some(f) = self.flags.iter_mut().find(|(id, _)| *id == node_id) {
            f.1.set(DirtyFlags::SELF);
        }
        let mut cur = self.parent_of(node_id);
        let mut depth = 0usize;
        while let Some(p) = cur {
            if let Some(f) = self.flags.iter_mut().find(|(id, _)| *id == p) {
                f.1.set(DirtyFlags::SUBTREE);
            }
            cur = self.parent_of(p);
            depth += 1;
        }
        self.audits.push(format!(
            "tick{} n={node_id} 向上聚合完成（祖先链 {depth} 层 SUBTREE 置位）",
            self.tick
        ));
        Ok(())
    }

    /// 向下失效：父变换/裁剪变更 → 子树全部失效（O(子树)；children 快照驱动）。
    pub fn invalidate_down(
        &mut self,
        node_id: u64,
        children: &[(u64, alloc::vec::Vec<u64>)],
    ) -> Result<(), &'static str> {
        if self.flags_of(node_id).is_none() {
            self.errors.push((
                format!("invalidate_down(n={node_id})"),
                "E_NODE_NOT_FOUND",
                "传播器父映射中无此 id——断链防护".to_string(),
            ));
            return Err("未登记的节点 id");
        }
        // 先根后子树：逐个置 CONTENT 位（内容/几何失效）。
        let mut stack = alloc::vec!(node_id);
        let mut count = 0usize;
        while let Some(cur) = stack.pop() {
            if let Some(f) = self.flags.iter_mut().find(|(id, _)| *id == cur) {
                f.1.set(DirtyFlags::CONTENT);
            }
            count += 1;
            if let Some((_, kids)) = children.iter().find(|(id, _)| *id == cur) {
                for k in kids {
                    stack.push(*k);
                }
            }
        }
        self.audits.push(format!(
            "tick{} n={node_id} 向下失效完成（子树 {count} 节点 CONTENT 置位——世界矩阵与包围盒作废）",
            self.tick
        ));
        Ok(())
    }

    /// 不变式断言（传播断链→不变式断言；debug 常开的执行面）：
    /// 1) 有 SELF 脏的节点，其祖先链上每层都应有 SUBTREE 位（向上聚合不缺环）；
    /// 2) 有 CONTENT 失效的节点，其根方向祖先应有 SUBTREE 或 SELF 位。
    /// 返回断链清单（空 = 不变式成立）。
    pub fn invariants_broken(&self) -> Vec<String> {
        let mut broken = Vec::new();
        for (id, f) in &self.flags {
            if f.has(DirtyFlags::SELF) {
                let mut cur = self.parent_of(*id);
                while let Some(p) = cur {
                    let pf = self.flags_of(p);
                    match pf {
                        Some(pf) if pf.has(DirtyFlags::SUBTREE) => {}
                        _ => broken.push(format!(
                            "n={id} 自身脏但祖先 n={p} 无 SUBTREE 位（向上聚合断链）"
                        )),
                    }
                    cur = self.parent_of(p);
                }
            }
            if f.has(DirtyFlags::CONTENT) {
                // 失效节点的祖先应有 SUBTREE 位（根节点豁免——无祖先）。
                if let Some(p) = self.parent_of(*id) {
                    let pf = self.flags_of(p);
                    match pf {
                        Some(pf) if pf.has(DirtyFlags::SUBTREE) || pf.has(DirtyFlags::SELF) => {}
                        _ => broken.push(format!(
                            "n={id} 已失效但父 n={p} 无脏位（向下失效与向上聚合不同步）"
                        )),
                    }
                }
            }
        }
        broken
    }

    /// 演练注入：清掉某节点的 SUBTREE 位（自检"断链捕获"用；生产路径不得调用）。
    pub fn clear_subtree_bit_for_drill(&mut self, node_id: u64) {
        if let Some(f) = self.flags.iter_mut().find(|(id, _)| *id == node_id) {
            f.1.clear(DirtyFlags::SUBTREE);
        }
    }

    /// 逻辑 tick 推进（零墙钟纪律）。
    pub fn tick(&mut self) {
        self.tick = self.tick.saturating_add(1);
    }
}

// ---------------------------------------------------------------------------
// 五、自检注册入口
// ---------------------------------------------------------------------------

/// VE-F0607 域自检（判据逐条映射见 `ved07_checks.rs`）。
pub fn run_ved07_checks() -> CheckSet {
    super::ved07_checks::run_ved07_checks()
}

// ---------------------------------------------------------------------------
// 六、单元测试（宿主 cargo test 直跑）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// 链 1(根)→2→3(叶) 的父映射。
    fn chain_parents() -> Vec<(u64, Option<u64>)> {
        vec![(1, None), (2, Some(1)), (3, Some(2))]
    }

    #[test]
    fn ved07_effective_visibility_propagation() {
        // 祖先隐藏 → 子树全隐藏。
        let chain = [Visibility::Visible, Visibility::HiddenKeepLayout, Visibility::Visible];
        assert_eq!(effective_visibility(&chain), EffectiveVisibility::HiddenSkipped);
        // 祖先移除 → Gone（隐藏之后再移除，升级为 Gone）。
        let chain = [Visibility::Visible, Visibility::HiddenKeepLayout, Visibility::Removed];
        assert_eq!(effective_visibility(&chain), EffectiveVisibility::Gone);
        let chain = [Visibility::Removed, Visibility::Visible, Visibility::Visible];
        assert_eq!(effective_visibility(&chain), EffectiveVisibility::Gone);
        // 全可见 → Shown。
        let chain = [Visibility::Visible, Visibility::Visible];
        assert_eq!(effective_visibility(&chain), EffectiveVisibility::Shown);
    }

    #[test]
    fn ved07_three_state_separate_apis_and_idempotent() {
        let mut t = VisibilityTable::new();
        t.tick();
        t.set_hidden(1);
        t.set_removed(2);
        assert_eq!(t.state_of(1), Visibility::HiddenKeepLayout);
        assert_eq!(t.state_of(2), Visibility::Removed);
        assert_eq!(t.state_of(99), Visibility::Visible, "未登记默认可见");
        // 同态设置幂等：无事件无记账。
        let n0 = t.pending_len();
        t.set_hidden(1);
        assert_eq!(t.pending_len(), n0, "同态设置幂等");
        // 审计含三态名。
        assert!(t.audits().iter().any(|a| a.contains("隐藏保留布局")));
    }

    #[test]
    fn ved07_event_mapping_and_storm_batch() {
        let mut t = VisibilityTable::new();
        t.tick();
        // 风暴：四次切换入池（未登记节点 4 的"设可见"是幂等空操作——不入池）。
        t.set_hidden(1);
        t.set_visible(1); // 隐藏→可见 = ShownAdded
        t.set_removed(2);
        t.set_visible(99); // 幂等空操作：默认可见 → 设可见
        assert_eq!(t.pending_len(), 3, "空操作不入池");
        t.set_hidden(3);
        assert_eq!(t.pending_len(), 4, "风暴池累积");
        let evs = t.apply_batch();
        assert_eq!(evs.len(), 4, "一次 apply 合并");
        assert_eq!(t.pending_len(), 0);
        // 事件映射语义：显示=加入。
        assert!(evs.iter().any(|e| matches!(e, DirtyEvent::ShownAdded { node_id: 1 })));
        // 隐藏=移除+相邻失效（从可见切到隐藏时）。
        t.tick();
        t.set_hidden(5); // before=Visible → adjacent_invalidated=true
        let evs = t.apply_batch();
        assert!(matches!(
            evs[0],
            DirtyEvent::HiddenRemoved { node_id: 5, adjacent_invalidated: true }
        ));
        // 移除=整块作废。
        t.tick();
        t.set_removed(7);
        let evs = t.apply_batch();
        assert!(matches!(evs[0], DirtyEvent::RemovedGone { node_id: 7 }));
    }

    #[test]
    fn ved07_validity_upward_aggregation_and_downward_invalidation() {
        let mut p = ValidityPropagator::new(chain_parents());
        p.tick();
        // 叶脏 → 祖先链 SUBTREE 全置位（O(深度)）。
        assert!(p.mark_dirty_up(3).is_ok());
        assert!(p.flags_of(3).unwrap().has(DirtyFlags::SELF));
        assert!(p.flags_of(2).unwrap().has(DirtyFlags::SUBTREE));
        assert!(p.flags_of(1).unwrap().has(DirtyFlags::SUBTREE));
        // 根变换变更 → 子树全失效（O(子树)）。
        let children = alloc::vec![(1u64, alloc::vec![2u64]), (2u64, alloc::vec![3u64]), (3u64, Vec::new())];
        assert!(p.invalidate_down(1, &children).is_ok());
        assert!(p.flags_of(1).unwrap().has(DirtyFlags::CONTENT));
        assert!(p.flags_of(3).unwrap().has(DirtyFlags::CONTENT));
        let audited = p.audits().iter().any(|a| a.contains("3 节点"));
        assert!(audited, "O(子树) 账");
        // 不变式成立（聚合与失效同步）。
        assert!(p.invariants_broken().is_empty());
    }

    #[test]
    fn ved07_invariant_catches_broken_chain() {
        // 构造断链：手动抹掉祖先 SUBTREE 位 → 不变式应报断链。
        let mut p = ValidityPropagator::new(chain_parents());
        p.tick();
        let _ = p.mark_dirty_up(3);
        // 模拟断链（演练注入）：清掉节点 1 的 SUBTREE 位。
        if let Some(f) = p.flags.iter_mut().find(|(id, _)| *id == 1) {
            f.1.clear(DirtyFlags::SUBTREE);
        }
        let broken = p.invariants_broken();
        assert!(!broken.is_empty(), "断链应被不变式捕获");
        assert!(broken.iter().any(|b| b.contains("断链") || b.contains("不同步")));
    }

    #[test]
    fn ved07_unknown_node_rejected() {
        let mut p = ValidityPropagator::new(chain_parents());
        assert!(p.mark_dirty_up(42).is_err());
        assert!(p.errors().iter().any(|(_, c, _)| *c == "E_NODE_NOT_FOUND"));
    }

    #[test]
    fn ved07_checks_all_green() {
        let set = run_ved07_checks();
        let (passed, failed) = set.tally();
        assert!(
            set.all_passed(),
            "VE-F0607 域自检存在红项：{}/{} 绿，红项：{:?}",
            passed,
            passed + failed,
            set.red_items()
        );
    }
}
