//! F271 资源管理器多标签页 · 完整设计（STAR I 主册 H 域）。
//!
//! **判据（主册）**：五组快捷键用例；拖出成窗与拖回成标签；独立栈/
//! 记忆验证；收缩阈值与 Tooltip。
//!
//! **设计要点（主册）**：Ctrl+T 新标签、Ctrl+W 关标签、Ctrl+Tab 轮换、
//! 中键点击目录=在新标签打开（中键开新窗语义保留给任务栏 F252）——
//! 键位全对齐浏览器肌肉记忆；标签可拖重排、拖出成独立窗口；每标签
//! 独立历史栈（F266）与视图记忆（F219）；标签栏空间不足时收缩为图标
//! （悬停 Tooltip 显示全路径）。
//!
//! 实装：标签容器（五组键位路由 + 拖重排 + 拖出/拖回事件）；每标签
//! 独立 F266 导航栈实例（隔离判据的结构保证）；视图记忆随标签走；
//! 收缩布局器（宽度预算 → 图标态判定 + Tooltip 数据）。

use crate::checks::CheckSet;
use crate::h2star::navstack::NavStack;

use alloc::string::String;
use alloc::vec::Vec;

/// 单个标签。
pub struct Tab {
    pub id: u32,
    pub title: String,
    /// 独立历史栈（F266——每标签各记各的）。
    pub nav: NavStack,
    /// 视图记忆（F219 口：排序/视图/滚动位）。
    pub view_mem: (String, String, u32),
}

/// 标签栏。
pub struct TabBar {
    tabs: Vec<Tab>,
    active: usize,
    next_id: u32,
}

/// 五组快捷键（唯一源——路由器按此对号）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TabKey {
    NewTab,      // Ctrl+T
    CloseTab,    // Ctrl+W
    CycleNext,   // Ctrl+Tab
    CyclePrev,   // Ctrl+Shift+Tab
    MiddleClick, // 中键=新标签开目录
}

impl TabBar {
    pub fn new(start: &str) -> TabBar {
        let mut bar = TabBar { tabs: Vec::new(), active: 0, next_id: 1 };
        bar.open_at(start, start);
        bar
    }

    /// 新标签（Ctrl+T：默认位起栈）。
    pub fn new_tab(&mut self) -> u32 {
        self.open_at("vx:/", "此电脑")
    }

    /// 中键：新标签打开目录（不是新窗——F252 语义保留给任务栏）。
    pub fn open_at(&mut self, path: &str, title: &str) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        self.tabs.push(Tab {
            id,
            title: String::from(title),
            nav: NavStack::new(path),
            view_mem: (String::from("名称"), String::from("详情"), 0),
        });
        self.active = self.tabs.len() - 1;
        id
    }

    /// 关标签；最后一个标签关掉=关窗（返回 None 语义由调用方处理）。
    pub fn close(&mut self, id: u32) -> bool {
        let before = self.tabs.len();
        self.tabs.retain(|t| t.id != id);
        if self.tabs.len() != before {
            if self.active >= self.tabs.len() {
                self.active = self.tabs.len().saturating_sub(1);
            }
            true
        } else {
            false
        }
    }

    /// 键位路由（五组快捷键判据的统一入口）。
    pub fn key(&mut self, k: TabKey) -> Option<u32> {
        match k {
            TabKey::NewTab | TabKey::MiddleClick => Some(self.new_tab()),
            TabKey::CloseTab => {
                let id = self.active_id();
                if self.close(id) {
                    self.active_id_opt()
                } else {
                    None
                }
            }
            TabKey::CycleNext => {
                if !self.tabs.is_empty() {
                    self.active = (self.active + 1) % self.tabs.len();
                }
                self.active_id_opt()
            }
            TabKey::CyclePrev => {
                if !self.tabs.is_empty() {
                    self.active = (self.active + self.tabs.len() - 1) % self.tabs.len();
                }
                self.active_id_opt()
            }
        }
    }

    /// 拖重排：把 from 位标签拖到 to 位。
    pub fn reorder(&mut self, from: usize, to: usize) -> bool {
        if from >= self.tabs.len() || to >= self.tabs.len() {
            return false;
        }
        let t = self.tabs.remove(from);
        self.tabs.insert(to, t);
        self.active = to;
        true
    }

    /// 拖出成独立窗口：摘除标签返回其数据（调用方开新窗）。
    pub fn tear_out(&mut self, id: u32) -> Option<Tab> {
        let pos = self.tabs.iter().position(|t| t.id == id)?;
        let tab = self.tabs.remove(pos);
        if self.active >= self.tabs.len() {
            self.active = self.tabs.len().saturating_sub(1);
        }
        Some(tab)
    }

    /// 拖回成标签（从独立窗口拖回）。
    pub fn tear_in(&mut self, tab: Tab) -> u32 {
        self.tabs.push(tab);
        self.active = self.tabs.len() - 1;
        self.tabs[self.tabs.len() - 1].id
    }

    pub fn active_id_opt(&self) -> Option<u32> {
        self.tabs.get(self.active).map(|t| t.id)
    }

    pub fn active_id(&self) -> u32 {
        self.active_id_opt().unwrap_or(0)
    }

    pub fn count(&self) -> usize {
        self.tabs.len()
    }

    /// 收缩布局：每标签预算宽 `per_tab`，总预算 `bar_width`——放不下时
    /// 非活动标签收缩为图标态（Tooltip 数据=全路径标题）。
    // -----------------------------------------------------------------
    // 深化批次二：拖出成窗阈值 + 拖回成标签命中（纯几何——坐标注入）
    // -----------------------------------------------------------------

    /// 拖出成窗阈值（px）：标签拖到标签栏下边缘之下超过此值 → 脱离
    /// 成独立窗（Windows 手感同源；不足此值松手=弹回原位）。
    pub const DRAG_OUT_PX: u32 = 30;
    /// 标签栏拖回命中带高（px）：拖回点落在此带内 → 重新停靠。
    pub const DOCK_BAND_PX: u32 = 24;

    /// 拖出判定：`below_bar_px` = 拖拽点在标签栏下边缘之下的距离。
    /// 超阈值 → 脱离；不足 → 松手弹回（拖拽可放弃并复原——十四章程）。
    pub fn drag_out(&self, below_bar_px: u32) -> bool {
        below_bar_px >= Self::DRAG_OUT_PX
    }

    /// 拖回判定：`x` 在 `bar_w` 横向范围内且 `y_from_bar_top` 落在
    /// 命中带内 → 重新停靠为标签（拖回成标签判据的几何面）。
    pub fn drag_back(&self, bar_w: u32, x: u32, y_from_bar_top: u32) -> bool {
        x <= bar_w && y_from_bar_top < Self::DOCK_BAND_PX
    }

    pub fn shrink_layout(&self, bar_width: u32, per_tab: u32) -> Vec<(u32, bool, String)> {
        let fits = self.tabs.len() as u64 * per_tab as u64 <= bar_width as u64;
        self.tabs
            .iter()
            .enumerate()
            .map(|(i, t)| (t.id, !(fits || i == self.active), t.title.clone()))
            .collect()
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub fn run_extabs_checks() -> CheckSet {
    let mut set = CheckSet::new("h2-F271");
    let mut bar = TabBar::new("vx:/甲");
    // 五组快捷键。
    let _ = bar.key(TabKey::NewTab);
    let c1 = bar.count();
    let _ = bar.key(TabKey::CycleNext);
    let _ = bar.key(TabKey::CyclePrev);
    let closed = bar.key(TabKey::CloseTab);
    let _ = bar.key(TabKey::MiddleClick);
    set.add(
        "F271 five keys",
        c1 == 2 && closed.is_some() && bar.count() == 2,
        "T/W/Tab/ShiftTab/Middle",
    );
    // 独立栈：甲标签导航不影响乙标签。
    let id_a = bar.tabs[0].id;
    let id_b = bar.tabs[1].id;
    let _ = bar.tabs[0].nav.navigate("vx:/甲/深");
    set.add(
        "F271 stacks isolated",
        bar.tabs[0].nav.state().can_back && !bar.tabs[1].nav.state().can_back,
        "per-tab history",
    );
    let _ = (id_a, id_b);
    // 视图记忆随标签走。
    bar.tabs[0].view_mem = (String::from("日期"), String::from("大图标"), 320);
    set.add(
        "F271 view mem",
        bar.tabs[0].view_mem.0 == "日期" && bar.tabs[0].view_mem.2 == 320,
        "F219 fields",
    );
    // 拖重排。
    set.add("F271 reorder", bar.reorder(1, 0) && bar.active == 0, "drag sort");
    set.add("F271 reorder reject", !bar.reorder(0, 9), "honest fail");
    // 拖出成窗与拖回成标签。
    let n_before = bar.count();
    let id = bar.tabs[0].id;
    let torn = bar.tear_out(id).unwrap();
    let back = bar.tear_in(torn);
    set.add(
        "F271 tear out/in",
        bar.count() == n_before && bar.active_id() == back,
        "window↔tab",
    );
    // 收缩阈值与 Tooltip：窄栏非活动标签进图标态。
    let lay = bar.shrink_layout(200, 120);
    let shrunk = lay.iter().filter(|(_, icon, _)| *icon).count();
    set.add(
        "F271 shrink+tooltip",
        shrunk == lay.len().saturating_sub(1) && lay.iter().all(|(_, _, t)| !t.is_empty()),
        "icon mode + title",
    );
    // --- 深化批次二：拖出成窗阈值 + 拖回成标签命中。 ---
    // 拖出判定：标签拖到标签栏下方 ≥30px → 脱离成独立窗。
    set.add(
        "F271 drag out threshold",
        bar.drag_out(30) && !bar.drag_out(29),
        "30px below bar detaches",
    );
    // 拖回判定：拖回点落在标签栏（宽 800）命中带内 → 重新停靠。
    set.add(
        "F271 drag back dock",
        bar.drag_back(800, 400, 12) && !bar.drag_back(800, 400, 40) && !bar.drag_back(800, 900, 12),
        "in-band re-dock",
    );
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f271_tab_keys_green() {
        let set = run_extabs_checks();
        let (p, f) = set.tally();
        assert!(set.all_passed(), "F271 自检红 {f}/{p}");
    }

    #[test]
    fn last_close_returns_gracefully() {
        let mut bar = TabBar::new("vx:/");
        let id = bar.active_id();
        assert!(bar.close(id) && bar.count() == 0, "最后一个标签可关——关窗由调用方接手");
    }
}
