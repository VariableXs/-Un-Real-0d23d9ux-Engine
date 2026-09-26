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
