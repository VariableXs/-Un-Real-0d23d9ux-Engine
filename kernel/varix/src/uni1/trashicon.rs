//! F415 回收站满空两态 · 完整设计（STAR I 主册 G-I-15）。
//!
//! **判据（主册）**：两态视觉切换即时（删入/清出 <1s）；角标数量；清空
//! 确认与不可逆提示；容量属性页；四菜单项功能。＋通12。
//!
//! 设计：回收站图标状态机——空（透明桶）/非空（纸团+角标）两态；删入/
//! 清出延迟账（<1s）；右键四菜单项（打开/清空/属性/固定到快速访问）；
//! 清空确认链（确认页显示件数与可释放空间 + 「不可还原」显式告知——
//! 全系统唯一一次不可逆删除）。v6 深化：满度档位（角标颜色三档——
//! <50%/50-80%/≥80% 配额）、批量还原（一次状态切换）、最新在前清单
//! （最近删除置顶——用户最先找的就是刚删的）、按大小降序视图、配额
//! 修改钳制、空桶清空诚实拒绝。

use crate::checks::CheckSet;

use alloc::vec::Vec;

/// 两态切换判线（ms）。
pub const STATE_SWITCH_BUDGET_MS: u64 = 1_000;

/// 右键四菜单项（顺序钉死）。
pub const TRASH_MENU: [&str; 4] = ["打开", "清空回收站", "属性", "固定到快速访问"];

/// 满度档位阈值（占配额百分比）。
pub const TIER_HALF_PCT: u64 = 50;
pub const TIER_HIGH_PCT: u64 = 80;

/// 回收站两态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrashState {
    Empty,
    Full,
}

/// 回收站语义核。
pub struct TrashIcon {
    pub state: TrashState,
    /// 角标数（= 件数）。
    pub badge: u32,
    /// 件目（id → bytes，供确认页与容量页；push 序 = 删除序）。
    pub items: Vec<(u64, u64)>,
    /// 最近一次状态切换耗时账。
    pub last_switch_ms: Option<u64>,
    pub over_budget: u64,
    /// 固定到快速访问账。
    pub pinned: bool,
    /// 清空执行账（确认后）。
    pub empties: u64,
    /// 配额（字节；属性页可改——钳制下限）。
    pub quota_bytes: u64,
}

impl TrashIcon {
    pub fn new() -> TrashIcon {
        TrashIcon {
            state: TrashState::Empty,
            badge: 0,
            items: Vec::new(),
            last_switch_ms: None,
            over_budget: 0,
            pinned: false,
            empties: 0,
            quota_bytes: 0,
        }
    }

    /// 满度档位（v6）：0=空 1=<50% 2=<80% 3=≥80%（角标颜色三档 +
    /// 空桶透明档）。配额未设（0）→ 非空一律高档（诚实保守）。
    pub fn fullness_tier(&self) -> u8 {
        if self.items.is_empty() {
            return 0;
        }
        if self.quota_bytes == 0 {
            return 3;
        }
        let used: u64 = self.items.iter().map(|(_, b)| b).sum();
        let pct = used * 100 / self.quota_bytes;
        if pct < TIER_HALF_PCT {
            1
        } else if pct < TIER_HIGH_PCT {
            2
        } else {
            3
        }
    }

    /// 删入（F414 三路同归的下游落位）。切换延迟注入记账。
    pub fn delete_in(&mut self, id: u64, bytes: u64, latency_ms: u64) {
        if !self.items.iter().any(|(i, _)| *i == id) {
            self.items.push((id, bytes));
        }
        self.apply_state(latency_ms);
    }

    /// 还原出站。
    pub fn restore_out(&mut self, id: u64, latency_ms: u64) -> bool {
        let before = self.items.len();
        self.items.retain(|(i, _)| *i != id);
        let removed = self.items.len() != before;
        if removed {
            self.apply_state(latency_ms);
        }
        removed
    }

    /// 批量还原（v6）：一次结算、一次状态切换（切换账只记一笔）。
    /// 返回实际还原件数（清单内不存在的 id 不虚计）。
    pub fn restore_many(&mut self, ids: &[u64], latency_ms: u64) -> usize {
        let before = self.items.len();
        self.items.retain(|(i, _)| !ids.contains(i));
        let removed = before - self.items.len();
        if removed > 0 {
            self.apply_state(latency_ms);
        }
        removed
    }

    /// 最新在前清单（v6）：最近删除置顶（渲染序——items 反转）。
    pub fn list_newest_first(&self) -> Vec<u64> {
        self.items.iter().rev().map(|(i, _)| *i).collect()
    }

    /// 按大小降序视图（v6）：占空间大户排前面（清理决策视图）。
    /// 平局按 id 升序（确定可复现）。
    pub fn list_by_size_desc(&self) -> Vec<u64> {
        let mut v: Vec<(u64, u64)> = self.items.clone();
        // 插入排序（不引 sort 依赖；n 小）。
        for i in 1..v.len() {
            let mut j = i;
            while j > 0 {
                let swap = v[j].1 > v[j - 1].1 || (v[j].1 == v[j - 1].1 && v[j].0 < v[j - 1].0);
                if swap {
                    v.swap(j - 1, j);
                    j -= 1;
                } else {
                    break;
                }
            }
        }
        v.into_iter().map(|(id, _)| id).collect()
    }

    /// 配额修改（v6）：属性页回写——下限钳制 1（0 语义是「未设」，由
    /// 初始化持有，不允许用户改没）。
    pub fn set_quota(&mut self, bytes: u64) {
        self.quota_bytes = bytes.max(1);
    }

    fn apply_state(&mut self, latency_ms: u64) {
        self.last_switch_ms = Some(latency_ms);
        if latency_ms > STATE_SWITCH_BUDGET_MS {
            self.over_budget += 1;
        }
        self.badge = self.items.len() as u32;
        self.state = if self.items.is_empty() { TrashState::Empty } else { TrashState::Full };
    }

    /// 清空确认页数据：件数与可释放空间（判据「显示件数与可释放空间」）。
    pub fn confirm_view(&self) -> (u32, u64) {
        (self.items.len() as u32, self.items.iter().map(|(_, b)| b).sum())
    }

    /// 清空执行：唯一不可逆动作——必须持确认凭据。
    pub fn empty_confirmed(&mut self, confirmed: bool, latency_ms: u64) -> bool {
        if !confirmed || self.items.is_empty() {
            return false;
        }
        self.items.clear();
        self.empties += 1;
        self.apply_state(latency_ms);
        true
    }

    /// 菜单项分发（四项功能）。
    pub fn menu_activate(&mut self, idx: usize) -> Option<&'static str> {
        match idx {
            0 => Some(TRASH_MENU[0]),                    // 打开（列表窗）
            1 => Some(TRASH_MENU[1]),                    // 清空（确认链由调用方走）
            2 => Some(TRASH_MENU[2]),                    // 属性（容量设置 F085）
            3 => {
                self.pinned = true;
                Some(TRASH_MENU[3])
            }
            _ => None,
        }
    }

    /// 容量属性页：占用字节与容量上限阈值判定（F085 联动）。
    pub fn capacity_page(&self, quota_bytes: u64) -> (u64, bool) {
        let used = self.items.iter().map(|(_, b)| b).sum();
        (used, used > quota_bytes)
    }
}

pub fn run_trashicon_checks() -> CheckSet {
    let mut set = CheckSet::new("uni1-F415");
    set.add(
        "f415-menu-const",
        TRASH_MENU == ["打开", "清空回收站", "属性", "固定到快速访问"],
        "",
    );
    set.add("f415-tier-thresholds", TIER_HALF_PCT == 50 && TIER_HIGH_PCT == 80, "");
    let mut t = TrashIcon::new();
    set.add("f415-initial-empty", t.state == TrashState::Empty && t.badge == 0, "");
    // 删入两态即时（<1s）。
    t.delete_in(1, 1_024, 200);
    t.delete_in(2, 2_048, 300);
    set.add(
        "f415-full-state-fast",
        t.state == TrashState::Full && t.badge == 2 && t.over_budget == 0,
        "",
    );
    // 确认页数据 + 清空确认链。
    set.add("f415-confirm-view", t.confirm_view() == (2, 3_072), "");
    set.add("f415-empty-needs-confirm", !t.empty_confirmed(false, 100), "");
    set.add(
        "f415-empty-confirmed",
        t.empty_confirmed(true, 250) && t.state == TrashState::Empty && t.badge == 0 && t.empties == 1,
        "",
    );
    // 空桶再清空：诚实拒绝（不可逆动作没有可执行对象）。
    set.add("f415-empty-on-empty-honest", !t.empty_confirmed(true, 50) && t.empties == 1, "");
    // 还原出站即时。
    t.delete_in(3, 512, 100);
    set.add(
        "f415-restore-out",
        t.restore_out(3, 180) && t.state == TrashState::Empty && t.over_budget == 0,
        "",
    );
    // 批量还原（v6）：一次切换账、缺席 id 不虚计。
    t.delete_in(10, 100, 50);
    t.delete_in(11, 200, 50);
    t.delete_in(12, 300, 50);
    set.add(
        "f415-restore-many",
        t.restore_many(&[10, 11, 99], 120) == 2 && t.badge == 1 && t.over_budget == 0,
        "",
    );
    // 最新在前（v6）：items push 序反转，12 在顶。
    t.delete_in(13, 400, 50);
    set.add("f415-newest-first", t.list_newest_first() == alloc::vec![13, 12], "");
    // 按大小降序（v6）：平局按 id 升序。
    let mut s = TrashIcon::new();
    s.delete_in(1, 500, 10);
    s.delete_in(2, 900, 10);
    s.delete_in(3, 500, 10);
    set.add("f415-size-desc", s.list_by_size_desc() == alloc::vec![2, 1, 3], "");
    // 满度档位（v6）：1KB/2KB 配额 → 60% 档 2；上调配额 → 档 1。
    let mut q = TrashIcon::new();
    q.set_quota(0);
    set.add("f415-quota-clamp", q.quota_bytes == 1, "0 被钳到 1——配额不允许改没");
    q.set_quota(5_000);
    q.delete_in(1, 3_000, 10);
    set.add("f415-tier-mid", q.fullness_tier() == 2, "60% → 档 2");
    q.set_quota(10_000);
    set.add("f415-tier-low", q.fullness_tier() == 1, "30% → 档 1");
    q.delete_in(2, 5_000, 10);
    set.add("f415-tier-high", q.fullness_tier() == 3, "80% → 档 3");
    // 未设配额非空保守高档。
    let mut n = TrashIcon::new();
    n.delete_in(1, 1, 10);
    set.add("f415-tier-unset-conservative", n.fullness_tier() == 3, "");
    // 四菜单项功能。
    set.add("f415-menu-open", t.menu_activate(0) == Some("打开"), "");
    set.add("f415-menu-empty", t.menu_activate(1) == Some("清空回收站"), "");
    set.add("f415-menu-props", t.menu_activate(2) == Some("属性"), "");
    set.add(
        "f415-menu-pin",
        t.menu_activate(3) == Some("固定到快速访问") && t.pinned,
        "",
    );
    set.add("f415-menu-bounds", t.menu_activate(4).is_none(), "");
    // 容量属性页（阈值判定）——先清场，从空桶起算（确认链语义内清）。
    set.add("f415-clear-for-capacity", t.empty_confirmed(true, 100), "");
    t.delete_in(4, 900, 100);
    let (used, over) = t.capacity_page(1_000);
    set.add("f415-capacity-page", used == 900 && !over, "");
    t.delete_in(5, 200, 100);
    set.add("f415-capacity-over", t.capacity_page(1_000) == (1_100, true), "");
    // 超时如实记账。
    t.delete_in(6, 1, 1_500);
    set.add("f415-switch-over-logged", t.over_budget == 1, "");
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duplicate_delete_in_keeps_single() {
        let mut t = TrashIcon::new();
        t.delete_in(9, 100, 50);
        t.delete_in(9, 100, 50);
        assert_eq!(t.badge, 1, "同 id 不重复计数");
    }

    #[test]
    fn restore_missing_id_false() {
        let mut t = TrashIcon::new();
        assert!(!t.restore_out(42, 50));
        assert!(t.last_switch_ms.is_none(), "无切换不记账");
    }

    #[test]
    fn restore_many_all_missing_noop() {
        let mut t = TrashIcon::new();
        t.delete_in(1, 10, 10);
        assert_eq!(t.restore_many(&[7, 8], 10), 0);
        assert!(t.last_switch_ms.is_some(), "此前删入已记过切换");
        assert_eq!(t.badge, 1, "全缺席不改变件数");
    }

    #[test]
    fn empty_trash_confirm_view_zero() {
        let t = TrashIcon::new();
        assert_eq!(t.confirm_view(), (0, 0), "空桶确认页件数与空间都为 0");
    }
}
