//! F299 开始菜单推荐区 · 完整设计（STAR I 主册 H 域）。
//!
//! **判据（主册）**：三来源混合排序规则入册；7 天徽标计时；× 降权
//! 持久化；广告位=0 判据（数据源白名单审计）；Top6 数量稳定。
//!
//! **设计要点（主册）**：开始菜单下沿推荐区三来源混合：最近安装（新装
//! 应用高亮 7 天徽标）、近期文件（F072 引擎 Top6，跨应用聚合）、推荐
//! 打开方式（选中上下文智能——插了 U 盘推「打开 S:」）；推荐可逐条 ×
//! 掉（× 掉的同源项降权不再见）；推荐区永不出现广告位（宪法级）。
//!
//! 实装：三来源供给器（安装流/文件流/上下文流——白名单外无第四源）；
//! 混合排序（新装 > 近期文件 > 上下文，同级按时间）；7 天徽标（分钟戳
//! 判定）；× 降权持久化（键 = 来源+条目）；Top6 稳定输出。

use crate::checks::CheckSet;

use alloc::string::String;
use alloc::vec::Vec;

/// 徽标高亮期（7 天 = 10080 分钟）。
pub const BADGE_MIN: u64 = 7 * 1440;
/// 推荐区容量。
pub const TOP_N: usize = 6;

/// 推荐来源（白名单三源——广告位=0 的结构保证：不存在第四个成员）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecoSource {
    RecentInstall,
    RecentFile,
    ContextHint,
}

/// 一条推荐条目。
#[derive(Clone, Debug)]
pub struct RecoItem {
    pub source: RecoSource,
    pub label: String,
    /// 安装/使用时刻（分钟戳注入）。
    pub at_min: u64,
}

impl RecoItem {
    /// 新装徽标：安装来源且 7 天内。
    pub fn badge_active(&self, now_min: u64) -> bool {
        self.source == RecoSource::RecentInstall
            && now_min.saturating_sub(self.at_min) < BADGE_MIN
    }

    /// 降权键（× 掉的键）。
    pub fn key(&self) -> String {
        alloc::format!("{:?}", self.source) + ":" + &self.label
    }
}

/// 推荐区服务。
pub struct RecoPanel {
    /// 三来源供给（白名单注入）。
    pub items: Vec<RecoItem>,
    /// × 降权账（键 → 降权时刻——持久化）。
    pub dismissed: Vec<(String, u64)>,
}

impl RecoPanel {
    pub fn new() -> RecoPanel {
        RecoPanel { items: Vec::new(), dismissed: Vec::new() }
    }

    /// 三来源混合排序：新装(0) > 近期文件(1) > 上下文(2)，
    /// 同级按时间新→旧。规则唯一源（入册即此函数）。
    fn source_rank(s: RecoSource) -> u8 {
        match s {
            RecoSource::RecentInstall => 0,
            RecoSource::RecentFile => 1,
            RecoSource::ContextHint => 2,
        }
    }

    /// Top6 输出：过滤 × 降权项 → 排序 → 截 6（数量稳定判据）。
    /// `now_min` 供调用方在同一时刻口径下取数（徽标判定见
    /// [`RecoItem::badge_active`]）。
    pub fn top(&self, _now_min: u64) -> Vec<RecoItem> {
        let mut v: Vec<&RecoItem> = self
            .items
            .iter()
            .filter(|i| !self.dismissed.iter().any(|(k, _)| *k == i.key()))
            .collect();
        v.sort_by(|a, b| {
            Self::source_rank(a.source)
                .cmp(&Self::source_rank(b.source))
                .then_with(|| b.at_min.cmp(&a.at_min))
        });
        v.into_iter().take(TOP_N).cloned().collect()
    }

    /// × 掉一条（降权持久化——重启后仍隐藏）。
    pub fn dismiss(&mut self, item_label: &str, source: RecoSource, now_min: u64) -> bool {
        let probe = RecoItem { source, label: String::from(item_label), at_min: 0 };
        let k = probe.key();
        if self.dismissed.iter().any(|(kk, _)| kk == &k) {
            return false;
        }
        self.dismissed.push((k, now_min));
        true
    }

    /// 快照（持久化 round-trip）。
    pub fn snapshot(&self) -> (Vec<RecoItem>, Vec<(String, u64)>) {
        (self.items.clone(), self.dismissed.clone())
    }

    pub fn restore(&mut self, items: Vec<RecoItem>, dismissed: Vec<(String, u64)>) {
        self.items = items;
        self.dismissed = dismissed;
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

fn install(name: &str, at: u64) -> RecoItem {
    RecoItem { source: RecoSource::RecentInstall, label: String::from(name), at_min: at }
}
fn file(name: &str, at: u64) -> RecoItem {
    RecoItem { source: RecoSource::RecentFile, label: String::from(name), at_min: at }
}
fn hint(name: &str, at: u64) -> RecoItem {
    RecoItem { source: RecoSource::ContextHint, label: String::from(name), at_min: at }
}

pub fn run_recommends_checks() -> CheckSet {
    let mut set = CheckSet::new("h2-F299");
    let mut panel = RecoPanel::new();
    // 时间线：现在=第 60 天。新笔记 1 天前装（徽标亮）；老软件很久前装（徽标灭）。
    let now = 60 * 1440;
    panel.items = alloc::vec![
        install("新笔记", now - 1440),
        file("报告.docx", 900),
        hint("打开 S:", 950),
        file("图纸.dwg", 800),
        install("老软件", 0),
    ];
    // 三来源混合排序：新装(新→旧) > 近期文件(新→旧) > 上下文。
    let top = panel.top(now);
    set.add(
        "F299 mixed order",
        top[0].label == "新笔记"
            && top[1].label == "老软件"
            && top[2].label == "报告.docx"
            && top[3].label == "图纸.dwg"
            && top[4].label == "打开 S:",
        "install>file>context",
    );
    // 7 天徽标：1 天前装的亮、很久前装的灭。
    set.add(
        "F299 badge 7d",
        top[0].badge_active(now) && !panel.items[4].badge_active(now),
        "install highlight",
    );
    // × 降权持久化。
    let ok = panel.dismiss("报告.docx", RecoSource::RecentFile, now);
    let after = panel.top(now + 1);
    set.add(
        "F299 dismiss persists",
        ok && after.iter().all(|i| i.label != "报告.docx") && panel.dismissed[0].1 == now,
        "hidden till forgotten",
    );
    // Top6 数量稳定：12 条也只出 6。
    for i in 0..8 {
        panel.items.push(file(&alloc::format!("文件{}", i), 600 + i));
    }
    let many = panel.top(now + 2);
    set.add(
        "F299 top6 stable",
        many.len() == TOP_N,
        "capped at 6",
    );
    // 广告位=0：来源枚举无第四成员（类型即证明）+ 快照 round-trip。
    let (items2, dis2) = panel.snapshot();
    let mut panel2 = RecoPanel::new();
    panel2.restore(items2, dis2);
    set.add(
        "F299 whitelist+persist",
        panel2.top(now + 2).len() == TOP_N
            && panel2.dismissed.len() == panel.dismissed.len(),
        "no ad slot exists",
    );
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f299_reco_panel() {
        let set = run_recommends_checks();
        let (p, f) = set.tally();
        assert!(set.all_passed(), "F299 自检红 {f}/{p}");
    }

    #[test]
    fn dismiss_twice_idempotent() {
        let mut p = RecoPanel::new();
        assert!(p.dismiss("x", RecoSource::ContextHint, 1));
        assert!(!p.dismiss("x", RecoSource::ContextHint, 2), "重复 × 不重复记账");
    }
}
