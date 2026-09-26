//! F330 通知中心分组与批量清除 · 完整设计（AI-H3 · 主册 G-H 区段）。
//!
//! **判据（主册）**：分组折叠行为；组清/全清/撤销三用例；钉选上限与置
//! 顶逻辑；时间规则边界（59 分钟/61 分钟）；清除后徽标同步（任务栏角标）。
//!
//! **设计要点**：通知按应用分组折叠（默认展开最近一组）、组头一键清空
//! 该组、顶部「全部清除」带撤销（5 秒内可恢复）；置顶钉选上限 3 条；时
//! 间显示规则（1 小时内相对时间、更早绝对时间）。

use crate::checks::CheckSet;

use alloc::string::String;
use alloc::vec::Vec;

/// 钉选上限。
pub const PIN_CAP: usize = 3;

/// 撤销窗口（ms）。
pub const UNDO_WINDOW_MS: u64 = 5000;

/// 一条通知。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Notice {
    pub id: u64,
    pub app: &'static str,
    pub text: String,
    pub at_ms: u64,
    pub pinned: bool,
}

/// 通知分组账（按应用分组——组序按最近活动降序，确定）。
pub struct NoticeCenter {
    items: Vec<Notice>,
    /// 已展开的组（默认展开最近一组——空账即无展开）。
    pub expanded: Option<String>,
    /// 任务栏角标数（清除后同步——判据载体）。
    pub badge: u64,
}

impl NoticeCenter {
    pub fn new() -> NoticeCenter {
        NoticeCenter { items: Vec::new(), expanded: None, badge: 0 }
    }

    /// 入账一条通知（默认展开切到该组——「最近一组」语义）。
    pub fn push(&mut self, id: u64, app: &'static str, text: &str, at_ms: u64) {
        self.items.push(Notice {
            id,
            app,
            text: String::from(text),
            at_ms,
            pinned: false,
        });
        self.expanded = Some(String::from(app));
        self.sync_badge();
    }

    /// 组视图（组名 + 该组通知 id 清单；组序按组内最新 at_ms 降序）。
    pub fn groups(&self) -> Vec<(String, Vec<u64>)> {
        let mut apps: Vec<&str> = Vec::new();
        for n in &self.items {
            if !apps.contains(&n.app) {
                apps.push(n.app);
            }
        }
        let mut groups: Vec<(String, u64, Vec<u64>)> = apps
            .iter()
            .map(|a| {
                let mut ids: Vec<u64> =
                    self.items.iter().filter(|n| n.app == *a).map(|n| n.id).collect();
                let latest = self
                    .items
                    .iter()
                    .filter(|n| n.app == *a)
                    .map(|n| n.at_ms)
                    .max()
                    .unwrap_or(0);
                ids.sort();
                (String::from(*a), latest, ids)
            })
            .collect();
        groups.sort_by(|x, y| y.1.cmp(&x.1));
        groups.into_iter().map(|(a, _, ids)| (a, ids)).collect()
    }

    /// 组清：清空该组（钉选保留——钉住的不被组清冲走）。
    pub fn clear_group(&mut self, app: &str) -> Vec<Notice> {
        let removed: Vec<Notice> =
            self.items.iter().filter(|n| n.app == app && !n.pinned).cloned().collect();
        self.items.retain(|n| !(n.app == app && !n.pinned));
        self.sync_badge();
        removed
    }

    /// 全清（带撤销——返回被清清单供 5 秒窗口恢复；钉选保留）。
    pub fn clear_all(&mut self) -> Vec<Notice> {
        let removed: Vec<Notice> = self.items.iter().filter(|n| !n.pinned).cloned().collect();
        self.items.retain(|n| n.pinned);
        self.sync_badge();
        removed
    }

    /// 撤销（5 秒窗口内——恢复清单并同步徽标）。
    pub fn undo(&mut self, removed: Vec<Notice>, now_ms: u64, cleared_at_ms: u64) -> bool {
        if now_ms.saturating_sub(cleared_at_ms) > UNDO_WINDOW_MS {
            return false;
        }
        for n in removed {
            if !self.items.iter().any(|x| x.id == n.id) {
                self.items.push(n);
            }
        }
        self.sync_badge();
        true
    }

    /// 钉选（上限 3——超出拒绝，不静默）。
    pub fn pin(&mut self, id: u64) -> bool {
        let pinned_count = self.items.iter().filter(|n| n.pinned).count();
        match self.items.iter_mut().find(|n| n.id == id) {
            Some(n) if pinned_count < PIN_CAP => {
                n.pinned = true;
                true
            }
            _ => false,
        }
    }

    /// 置顶序：钉选恒在未钉选前（组内）。
    pub fn pinned_first(&self) -> Vec<u64> {
        let mut pinned: Vec<u64> = self.items.iter().filter(|n| n.pinned).map(|n| n.id).collect();
        let rest: Vec<u64> = self.items.iter().filter(|n| !n.pinned).map(|n| n.id).collect();
        pinned.extend(rest);
        pinned
    }

    /// 时间规则：59 分钟 → 相对；61 分钟 → 绝对（边界判线载体）。
    pub fn time_label(at_ms: u64, now_ms: u64) -> String {
        let mins = (now_ms.saturating_sub(at_ms)) / 60_000;
        if mins < 60 {
            alloc::format!("{} 分钟前", mins.max(0).max(if now_ms >= at_ms { mins } else { 0 }))
        } else {
            alloc::format!("{}", at_ms)
        }
    }

    fn sync_badge(&mut self) {
        self.badge = self.items.len() as u64;
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

impl Default for NoticeCenter {
    fn default() -> NoticeCenter {
        NoticeCenter::new()
    }
}

/// F330 自检。
pub fn run_ntfgrp_checks() -> CheckSet {
    let mut set = CheckSet::new("F330-ntfgrp");

    let mut c = NoticeCenter::new();
    c.push(1, "邮件", "新邮件 A", 100);
    c.push(2, "邮件", "新邮件 B", 200);
    c.push(3, "日程", "会议提醒", 150);
    c.push(4, "日历", "日程通知", 300);

    // 1. 分组折叠行为：按应用分组、最近组排前、默认展开最近组。
    let g = c.groups();
    set.add(
        "groups ordered by latest",
        g.len() == 3
            && g[0].0 == "日历"
            && g[1].0 == "邮件"
            && g[1].1 == [1, 2]
            && c.expanded.as_deref() == Some("日历"),
        "",
    );

    // 2. 组清：清空邮件组，其他组不动；徽标同步。
    let removed = c.clear_group("邮件");
    set.add(
        "group clear syncs badge",
        removed.len() == 2 && c.len() == 2 && c.badge == 2 && !c.groups().iter().any(|x| x.0 == "邮件"),
        "",
    );

    // 3. 全清 + 撤销（5 秒内）三用例：全清 → 撤销恢复 → 徽标同步。
    let mut c = NoticeCenter::new();
    c.push(1, "邮件", "A", 0);
    c.push(2, "日程", "B", 10);
    c.pin(2);
    let removed = c.clear_all();
    set.add(
        "clear all keeps pinned",
        c.len() == 1 && removed.len() == 1 && c.badge == 1,
        "",
    );
    set.add("undo within window", c.undo(removed, 4000, 0) && c.badge == 2, "");

    // 4. 撤销超窗拒绝（5 秒外——清错的不可救但状态诚实）。
    let removed = c.clear_all();
    set.add("undo past window refused", !c.undo(removed, 6000, 0) && c.badge == 1, "");

    // 5. 钉选上限 3：第 4 枚拒绝。
    let mut c = NoticeCenter::new();
    for i in 1..=4u64 {
        c.push(i, "测试", "t", i);
    }
    let p1 = c.pin(1);
    let p2 = c.pin(2);
    let p3 = c.pin(3);
    let p4 = c.pin(4);
    set.add(
        "pin cap three",
        p1 && p2 && p3 && !p4 && c.pinned_first()[0] == 1 && c.pinned_first().last() == Some(&4),
        "",
    );

    // 6. 时间规则边界：59 分钟相对、61 分钟绝对。
    set.add(
        "time rule boundary",
        NoticeCenter::time_label(0, 59 * 60_000).contains("59 分钟前")
            && NoticeCenter::time_label(0, 61 * 60_000) == "0",
        "",
    );

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_center_safe() {
        let c = NoticeCenter::new();
        assert!(c.groups().is_empty() && c.is_empty() && c.badge == 0);
    }

    #[test]
    fn group_clear_keeps_pinned() {
        let mut c = NoticeCenter::new();
        c.push(1, "a", "x", 0);
        c.push(2, "a", "y", 1);
        let _ = c.pin(1);
        let removed = c.clear_group("a");
        assert_eq!(removed.len(), 1);
        assert_eq!(c.len(), 1);
        assert!(c.items[0].pinned);
    }

    #[test]
    fn badge_syncs_on_push() {
        let mut c = NoticeCenter::new();
        c.push(1, "a", "x", 0);
        assert_eq!(c.badge, 1);
    }
}

// ---------------------------------------------------------------------------
// 深化层二 · F330 时间边界账/徽标同步账/撤销窗口边界账
// ---------------------------------------------------------------------------

/// 通知时间显示边界账（判据「59 分钟相对 / 更早绝对」的边界实测）：
/// 59 分钟 → 相对；60 分钟起 → 绝对（时间戳）；未来时刻钳 0 不出负。
pub fn time_boundary_audit() -> bool {
    let now: u64 = 100 * 60 * 1000;
    let at_59 = now - 59 * 60 * 1000;
    let at_60 = now - 60 * 60 * 1000;
    let at_61 = now - 61 * 60 * 1000;
    let l59 = NoticeCenter::time_label(at_59, now);
    let l60 = NoticeCenter::time_label(at_60, now);
    let l61 = NoticeCenter::time_label(at_61, now);
    let future = NoticeCenter::time_label(now + 5_000, now);
    l59 == "59 分钟前" && !l60.contains("分钟前") && !l61.contains("分钟前")
        && future == "0 分钟前"
}

/// 徽标同步账（判据「清除后徽标同步（任务栏角标）」的深化）：角标 =
/// 在场通知数——组清/全清/撤销三路操作后角标必须与账面一致
/// （sync_badge 的三路对账，一处不同步即缺陷）。
pub fn badge_sync_audit(center: &NoticeCenter) -> bool {
    // 徽标是私有账——经 len() 对读（len 即账面在场数，sync_badge 同源）。
    center.badge == center.len() as u64
}

/// 深化层二自检（时间边界 / 徽标三路同步 / 撤销窗口边界 / 钉选上限）。
pub fn run_ntfgrp_deep2_checks() -> CheckSet {
    let mut set = CheckSet::new("F330-deep2");

    // 1. 时间规则边界：59 分钟相对、60/61 分钟绝对、未来钳 0。
    set.add("time boundary 59/60/61", time_boundary_audit(), "");

    // 2. 徽标同步·组清：清一组 → 角标降到剩余数。
    let mut nc = NoticeCenter::new();
    nc.push(1, "邮件", "新邮件 A", 0);
    nc.push(2, "邮件", "新邮件 B", 1_000);
    nc.push(3, "日历", "会议提醒", 2_000);
    let ok_before = badge_sync_audit(&nc);
    let _ = nc.clear_group("邮件");
    set.add(
        "badge sync after group clear",
        ok_before && nc.len() == 1 && badge_sync_audit(&nc),
        "",
    );

    // 3. 徽标同步·全清+撤销：全清归零 → 5s 内撤销 → 角标回涨。
    let mut nc2 = NoticeCenter::new();
    nc2.push(1, "邮件", "A", 0);
    nc2.push(2, "日历", "B", 1_000);
    let removed = nc2.clear_all();
    let zero_ok = nc2.len() == 0 && badge_sync_audit(&nc2);
    let undone = nc2.undo(removed, 3_000, 2_000);
    set.add(
        "badge sync clear all then undo",
        zero_ok && undone && nc2.len() == 2 && badge_sync_audit(&nc2),
        "",
    );

    // 4. 撤销窗口边界：恰 5s 内可撤（<5000ms），超窗拒绝。
    let mut nc3 = NoticeCenter::new();
    nc3.push(1, "邮件", "A", 0);
    let removed3 = nc3.clear_all();
    let in_window = nc3.undo(removed3, 4_999, 0);
    set.add("undo inside 5s window", in_window && nc3.len() == 1, "");
    let mut nc4 = NoticeCenter::new();
    nc4.push(1, "邮件", "A", 0);
    let removed4 = nc4.clear_all();
    let out_window = nc4.undo(removed4, 5_001, 0);
    set.add("undo outside window rejected", !out_window && nc4.len() == 0, "");

    // 5. 钉选上限深化：第 4 枚钉选拒绝（上限 3）；pinned_first 是「钉选
    //    恒在前」的排序面——前三位恰为被钉的三枚。
    let mut nc5 = NoticeCenter::new();
    for id in 1..=4u64 {
        nc5.push(id, "任务", "待办", id * 1_000);
    }
    let p1 = nc5.pin(1);
    let p2 = nc5.pin(2);
    let p3 = nc5.pin(3);
    let p4 = nc5.pin(4);
    let order = nc5.pinned_first();
    set.add(
        "pin cap three",
        p1 && p2 && p3 && !p4 && order.len() == 4 && order.starts_with(&[1u64, 2, 3]),
        "",
    );

    // 6. 分组折叠：默认展开最近一组（组序按最新时间）——组头顺序账。
    let mut nc6 = NoticeCenter::new();
    nc6.push(1, "邮件", "早的", 0);
    nc6.push(2, "日历", "晚的", 9_000);
    let groups = nc6.groups();
    set.add(
        "groups newest first",
        groups.len() == 2 && groups[0].0 == "日历" && groups[1].0 == "邮件",
        "",
    );

    set
}

#[cfg(test)]
mod deep2_tests {
    use super::*;

    #[test]
    fn time_label_boundary_60_is_absolute() {
        let now = 100 * 60 * 1000;
        let l = NoticeCenter::time_label(now - 60 * 60 * 1000, now);
        assert!(!l.contains("分钟前"), "60 分钟整起转绝对时间");
    }

    #[test]
    fn undo_empty_window_noop() {
        let mut nc = NoticeCenter::new();
        // 空清单撤销：窗口内语义成功但账面零变化（no-op 成功，不伪造红）。
        assert!(nc.undo(Vec::new(), 1_000, 0) && nc.len() == 0);
    }

    #[test]
    fn badge_sync_on_fresh_center() {
        let nc = NoticeCenter::new();
        assert!(badge_sync_audit(&nc));
    }
}
