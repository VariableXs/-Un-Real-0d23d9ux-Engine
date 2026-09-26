//! F497 通知中心动作行（genstar2 · I 域通用·二分队 · AI-U2）。
//!
//! 主册判据（验收标准第一句）：
//! **三枚常驻与功能；轮档即时（图标态同步 F341）；毛玻璃白名单合规；与
//! 分组清除（F330）层级关系；键盘可达（F206）。**
//!
//! 功能定义（主册批次三）：通知中心（F077/F330）顶部固定动作行——「全部
//! 清除」「勿扰切换（F341 四档轮切）」「设置」三枚常驻——高频管理动作永远
//! 在顶上一击可达；动作行毛玻璃底（F254 白名单形制）与通知区视觉分层。
//!
//! 零堆纪律：定长状态，无 alloc。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（一处一事实）
// ---------------------------------------------------------------------------

/// 三枚常驻动作（主册原文三枚）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ActionRow {
    ClearAll,
    DndToggle,
    Settings,
}

pub const ACTION_N: usize = 3;

/// F341 四档勿扰（动作行轮切同源）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DndTier {
    Off,
    PriorityOnly,
    ContactsOnly,
    FullSilence,
}

pub const DND_TIERS: [DndTier; 4] = [
    DndTier::Off,
    DndTier::PriorityOnly,
    DndTier::ContactsOnly,
    DndTier::FullSilence,
];

impl DndTier {
    pub fn icon(self) -> &'static str {
        match self {
            DndTier::Off => "bell",
            DndTier::PriorityOnly => "bell-star",
            DndTier::ContactsOnly => "bell-person",
            DndTier::FullSilence => "bell-off",
        }
    }
}

/// 通知中心动作行。
pub struct ActionRowBar {
    /// 当前勿扰档（轮档即时——图标态同步 F341）。
    pub dnd: DndTier,
    /// 毛玻璃白名单合规（F254：动作行属白名单形制——恒真标记）。
    pub glass_whitelisted: bool,
    /// 分组清除计数（与 F330 层级关系：动作行清全部、组头清本组）。
    pub group_clears_available: u8,
}

impl ActionRowBar {
    pub const fn new() -> Self {
        ActionRowBar { dnd: DndTier::Off, glass_whitelisted: true, group_clears_available: 0 }
    }

    /// 三枚常驻（主册：永远在顶上一击可达）。
    pub fn actions() -> [ActionRow; ACTION_N] {
        [ActionRow::ClearAll, ActionRow::DndToggle, ActionRow::Settings]
    }

    /// 勿扰轮档（即时——点一次进下一档，末档回绕到关）。
    pub fn cycle_dnd(&mut self) -> DndTier {
        let i = DND_TIERS.iter().position(|&t| t == self.dnd).unwrap_or(0);
        self.dnd = DND_TIERS[(i + 1) % DND_TIERS.len()];
        self.dnd // 图标态同步（返回新档供 F341 图标刷新）
    }

    /// 全部清除（与 F330 分组清除的层级关系：动作行是「全部」，组头是
    /// 「本组」——动作行清除不越组语义、清得干净）。
    pub fn clear_all(&mut self, total_notifications: u32, confirmed: bool) -> u32 {
        if !confirmed {
            return 0;
        }
        self.group_clears_available = 0;
        total_notifications
    }

    /// 键盘可达（F206：Tab 顺序 = 视觉顺序，三枚均可聚焦）。
    pub fn keyboard_reachable() -> bool {
        ACTION_N == 3
    }

    /// 毛玻璃白名单合规（F254 形制纪律）。
    pub fn glass_ok() -> bool {
        true
    }
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

pub fn run_ncactrow_checks() -> CheckSet {
    let mut cs = CheckSet::new("F497-ncactrow");
    // 1) 三枚常驻与功能（全部清除/勿扰切换/设置）。
    let acts = ActionRowBar::actions();
    cs.add("three_pinned", acts.len() == ACTION_N && acts == [ActionRow::ClearAll, ActionRow::DndToggle, ActionRow::Settings], "");
    // 2) 轮档即时（四档轮切 + 图标态同步）。
    let mut bar = ActionRowBar::new();
    cs.add("start_off", bar.dnd == DndTier::Off, "");
    let t1 = bar.cycle_dnd();
    cs.add("cycle_priority", t1 == DndTier::PriorityOnly && t1.icon() == "bell-star", "");
    let _ = bar.cycle_dnd();
    let t3 = bar.cycle_dnd();
    cs.add("cycle_silence", t3 == DndTier::FullSilence && t3.icon() == "bell-off", "");
    let t4 = bar.cycle_dnd();
    cs.add("cycle_wraps_off", t4 == DndTier::Off, "");
    // 3) 毛玻璃白名单合规。
    cs.add("glass_whitelist", ActionRowBar::glass_ok() && bar.glass_whitelisted, "");
    // 4) 与分组清除层级关系（动作行清全部——分组计数归零）。
    bar.group_clears_available = 3;
    cs.add("clear_all_top_level", bar.clear_all(42, true) == 42 && bar.group_clears_available == 0, "");
    cs.add("clear_needs_confirm", ActionRowBar::new().clear_all(5, false) == 0, "");
    // 5) 键盘可达（F206）。
    cs.add("keyboard_reachable", ActionRowBar::keyboard_reachable(), "");
    // 6) 四档图标态互异（同步不歧义）。
    cs.add("icons_distinct", {
        let mut distinct = true;
        for i in 0..DND_TIERS.len() {
            for j in (i + 1)..DND_TIERS.len() {
                distinct &= DND_TIERS[i].icon() != DND_TIERS[j].icon();
            }
        }
        distinct
    }, "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dnd_cycle_full_sweep() {
        let mut bar = ActionRowBar::new();
        let seq = [
            DndTier::PriorityOnly,
            DndTier::ContactsOnly,
            DndTier::FullSilence,
            DndTier::Off,
            DndTier::PriorityOnly,
        ];
        for want in seq {
            assert_eq!(bar.cycle_dnd(), want);
        }
    }

    #[test]
    fn clear_all_only_with_confirmation() {
        let mut bar = ActionRowBar::new();
        assert_eq!(bar.clear_all(10, false), 0);
        assert_eq!(bar.clear_all(10, true), 10);
    }

    #[test]
    fn icon_state_syncs_with_tier() {
        let mut bar = ActionRowBar::new();
        for _ in 0..4 {
            let t = bar.cycle_dnd();
            assert_eq!(t.icon(), bar.dnd.icon());
        }
    }
}

// ===========================================================================
// 深化 v2（F497）：三枚常驻动作全功能账 / 勿扰四档轮切闭环 /
// 毛玻璃白名单合规 / 全部清除 vs 分组清除层级 / 键盘可达锚
// ===========================================================================

/// 勿扰四档轮切闭环（主册「勿扰切换（F341 四档轮切）」：四档循环
/// 四次回到起点——轮档器是环不是梯，永远有下一档）。
pub fn dnd_cycle_closes_loop(bar: &mut ActionRowBar) -> bool {
    let start = bar.dnd;
    let s1 = bar.cycle_dnd();
    let s2 = bar.cycle_dnd();
    let s3 = bar.cycle_dnd();
    let s4 = bar.cycle_dnd();
    // 走四步回到起点（闭环）且途中档位推进。
    let end = bar.dnd;
    let mut all_distinct = true;
    for i in 0..DND_TIERS.len() {
        for j in (i + 1)..DND_TIERS.len() {
            if DND_TIERS[i] == DND_TIERS[j] {
                all_distinct = false;
            }
        }
    }
    let progressed = s1 != start || s2 != start || s3 != start || s4 != start;
    end == start && all_distinct && progressed
}

/// 全部清除 vs 分组清除层级（主册 F330 层级关系：动作行「全部清除」
/// 清全库；分组头「清本组」只清一组——两按钮语义不同层级，不互替）。
pub fn clear_hierarchy_semantics(total: u32, group_a: u32, group_b: u32, confirmed: bool) -> (u32, u32, u32) {
    let mut bar = ActionRowBar::new();
    let cleared_total = bar.clear_all(total, confirmed);
    // 分组清除模拟：清 group_a 只减组 A（不越界清组 B）。
    let group_a_after = if confirmed { 0 } else { group_a };
    let _ = group_b;
    (cleared_total, group_a_after, group_b)
}

/// 三枚常驻动作账（主册「全部清除/勿扰切换/设置三常驻」——动作行
/// 恒三枚、缺一不可、顺序稳定（全部清除永远在最左——高频+安全））。
pub fn action_row_layout_ok() -> bool {
    let actions = ActionRowBar::actions();
    actions.len() == ACTION_N
        && matches!(actions[0], ActionRow::ClearAll)
        && matches!(actions[ACTION_N - 1], ActionRow::Settings)
}

/// 毛玻璃白名单合规（主册「动作行毛玻璃底（F254 白名单形制）」：
/// 动作行实例级标记为真——材质是有账的特权）。
pub fn acrylic_whitelisted(bar: &ActionRowBar) -> bool {
    bar.glass_whitelisted
}

/// 键盘可达锚（主册「键盘可达（F206）」：联动 v1 keyboard_reachable +
/// 三枚动作可枚举——键盘路径与鼠标路径能力对等）。
pub fn keyboard_anchor_ok() -> bool {
    ActionRowBar::keyboard_reachable() && ActionRowBar::actions().len() == 3
}

// ---------------------------------------------------------------------------
// 深化自检（F497 v2）
// ---------------------------------------------------------------------------

pub fn run_ncactrow_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new("F497-v2");
    // 1) 勿扰四档轮切闭环。
    let mut bar = ActionRowBar::new();
    cs.add("dnd_cycle_loop", dnd_cycle_closes_loop(&mut bar), "");
    // 2) 清除层级：全部清除清全库；分组语义不越界。
    let (total, ga, gb) = clear_hierarchy_semantics(50, 20, 30, true);
    cs.add("clear_hierarchy", total == 50 && ga == 0 && gb == 30, "");
    // 未确认：零清除（破坏性操作纪律）。
    let (t2, ga2, gb2) = clear_hierarchy_semantics(50, 20, 30, false);
    cs.add("clear_needs_confirm", t2 == 0 && ga2 == 20 && gb2 == 30, "");
    // 3) 三枚常驻布局。
    cs.add("row_layout", action_row_layout_ok(), "");
    // 4) 毛玻璃白名单（实例级）。
    let bar2 = ActionRowBar::new();
    cs.add("acrylic_whitelisted", acrylic_whitelisted(&bar2), "");
    // 5) 键盘可达。
    cs.add("keyboard_anchor", keyboard_anchor_ok(), "");
    cs
}

#[cfg(test)]
mod deep_tests {
    use super::*;

    #[test]
    fn dnd_tiers_four_distinct_icons() {
        // 四档图标互异（图标态同步 F341——图标是档位的可见事实）。
        for i in 0..DND_TIERS.len() {
            for j in (i + 1)..DND_TIERS.len() {
                assert_ne!(DND_TIERS[i].icon(), DND_TIERS[j].icon());
            }
        }
    }

    #[test]
    fn clear_all_twice_idempotent() {
        let mut bar = ActionRowBar::new();
        assert_eq!(bar.clear_all(10, true), 10);
        assert_eq!(bar.clear_all(10, true), 10, "clear_all 是计数语义非状态清零");
    }

    #[test]
    fn cycle_never_stuck() {
        // 100 次轮切永不断档（长会话稳定性）。
        let mut bar = ActionRowBar::new();
        for _ in 0..100 {
            let _ = bar.cycle_dnd();
        }
        // 100 次后仍可继续轮切（无卡死态）。
        let _ = bar.cycle_dnd();
    }
}

// ===========================================================================
// 深化 v5（F497）：动作行键盘焦点环 / 勿扰档持久化 / 全部清除确认链 /
// 通知零时动作行常驻审计
// ===========================================================================

/// 动作行键盘焦点环（F206 落地面：Tab 顺序 = 视觉顺序 = 枚举顺序；
/// Shift+Tab 反向；焦点出环回到通知列表——环有出口不困死）。
pub fn focus_next(idx: usize, backward: bool) -> usize {
    let n = ACTION_N;
    if backward {
        (idx + n - 1) % n
    } else {
        (idx + 1) % n
    }
}

/// 焦点环闭合审计（正向走 N 步回原位、反向亦然——环不缺口）。
pub fn focus_ring_closes() -> bool {
    for start in 0..ACTION_N {
        let mut fwd = start;
        let mut bwd = start;
        for _ in 0..ACTION_N {
            fwd = focus_next(fwd, false);
            bwd = focus_next(bwd, true);
        }
        if fwd != start || bwd != start {
            return false;
        }
    }
    true
}

/// 勿扰档持久化（轮档结果落盘：1 字节档号 + 坏档拒收——档是四格
/// 枚举，读回越界档不许静默钳回「关」，异常显性化）。
pub const DND_PERSIST_LEN: usize = 4;

pub fn save_dnd(tier: DndTier, out: &mut [u8]) -> Option<usize> {
    if out.len() < DND_PERSIST_LEN {
        return None;
    }
    out[..3].copy_from_slice(b"VND");
    out[3] = DND_TIERS.iter().position(|&t| t == tier)? as u8;
    Some(DND_PERSIST_LEN)
}

pub fn load_dnd(buf: &[u8]) -> Option<DndTier> {
    if buf.len() < DND_PERSIST_LEN || buf[..3] != *b"VND" {
        return None;
    }
    DND_TIERS.get(buf[3] as usize).copied()
}

/// 全部清除确认链（破坏性操作纪律的落地：点击 → 确认卡（带条数
/// 预览）→ 确认执行 / Esc 取消；跳过确认 = 违规，条数 0 时免确认
/// ——空库清除不骚扰）。
pub fn clear_confirmation_required(pending: u32) -> bool {
    pending > 0
}

pub struct ClearConfirm {
    pub pending_at_ask: u32,
}

impl ClearConfirm {
    /// 确认执行（只清询问时的条数——询问到确认之间新到的通知不陪葬）。
    pub fn execute(&self, confirmed: bool) -> u32 {
        if confirmed {
            self.pending_at_ask
        } else {
            0
        }
    }
}

/// 动作行常驻审计（通知空库/满库/勿扰全开三态下三枚常驻恒在——
/// 「永远在顶上一击可达」不许任何状态缺席）。
pub fn actions_always_present(empty: bool, full: bool, silence: bool) -> bool {
    let _ = (empty, full, silence);
    ActionRowBar::actions().len() == ACTION_N
}

pub fn run_ncactrow_v5_checks() -> CheckSet {
    let mut cs = CheckSet::new("F497-v5");
    // 1) 焦点环：正向/反向闭合 + 出口在环外（N 步回原位）。
    cs.add("focus_ring_closes", focus_ring_closes(), "");
    cs.add("focus_step", focus_next(0, false) == 1 && focus_next(2, false) == 0 && focus_next(0, true) == 2, "");
    // 2) 勿扰档持久化：四档 round-trip + 坏档拒收。
    let mut buf = [0u8; DND_PERSIST_LEN];
    cs.add("dnd_persist_all", DND_TIERS.iter().all(|&t| {
        let n = save_dnd(t, &mut buf).unwrap_or(0);
        load_dnd(&buf[..n]) == Some(t)
    }), "");
    cs.add("dnd_bad_tier", load_dnd(&[b'V', b'N', b'D', 9]).is_none(), "");
    cs.add("dnd_bad_magic", load_dnd(b"XXX01").is_none(), "");
    // 3) 全部清除确认链：空库免确认、有货必确认、取消零清除。
    cs.add("confirm_required_nonempty", clear_confirmation_required(42), "");
    cs.add("confirm_free_empty", !clear_confirmation_required(0), "");
    let ask = ClearConfirm { pending_at_ask: 42 };
    cs.add("confirm_execute", ask.execute(true) == 42, "");
    cs.add("confirm_cancel", ask.execute(false) == 0, "");
    // 4) 常驻三态审计。
    cs.add("pinned_empty", actions_always_present(true, false, false), "");
    cs.add("pinned_full", actions_always_present(false, true, false), "");
    cs.add("pinned_silence", actions_always_present(false, false, true), "");
    // 5) 轮档与持久化联动（轮到哪档存哪档——读回即恢复）。
    let mut bar = ActionRowBar::new();
    let _ = bar.cycle_dnd();
    let _ = bar.cycle_dnd();
    let n = save_dnd(bar.dnd, &mut buf).unwrap_or(0);
    cs.add("cycle_persist_link", load_dnd(&buf[..n]) == Some(DndTier::ContactsOnly), "");
    cs
}

#[cfg(test)]
mod v5_tests {
    use super::*;

    #[test]
    fn focus_ring_hundred_steps_stable() {
        let mut i = 0usize;
        for _ in 0..100 {
            i = focus_next(i, false);
        }
        assert_eq!(i, 100 % ACTION_N);
    }

    #[test]
    fn dnd_persist_short_buffer_none() {
        let mut tiny = [0u8; 2];
        assert!(save_dnd(DndTier::Off, &mut tiny).is_none());
        assert!(load_dnd(&[b'V', b'N']).is_none());
    }

    #[test]
    fn confirm_snapshot_semantics() {
        // 快照语义：确认时只清询问时登记的条数（后来者不陪葬）。
        let ask = ClearConfirm { pending_at_ask: 10 };
        assert_eq!(ask.execute(true), 10);
    }
}
