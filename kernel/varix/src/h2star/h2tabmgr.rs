//! H2 标签页管理 · 深化批次四（F271 渲染与几何侧——标签条宽度
//! 自适应、溢出滚动、拖拽重排、关闭焦点转移、全状态恢复）。
//!
//! **承接判据**（主册 H 域正文，一处一事实）：
//! - **F271 资源管理器多标签页**：收缩阈值与 Tooltip（标签宽度
//!   三档：完整标题→文件名→图标——阈值由条数与条宽算出）、拖出
//!   成窗与拖回成标签（几何在 extabs，本层管重排与焦点）、重启
//!   恢复 10 标签全状态（含滚动位——恢复账含视口偏移）；
//! - **十四章状态机**：关闭中间标签的焦点转移有明确规则（关谁的
//!   邻居谁接管——右邻优先、无右邻取左邻），关当前标签焦点不落
//!   空（「焦点丢在宇宙里」的标签条版）；
//! - **顺序稳定**：拖拽重排即模型重排（构造序=渲染序纪律同源），
//!   非法位置（越界/原位）原样拒绝留账。
//!
//! 时间纪律：无时钟；几何纯函数。

use crate::checks::CheckSet;

use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 标签模型与宽度三档
// ---------------------------------------------------------------------------

/// 标签宽度三档（F271 收缩阈值判据）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TabWidth {
    /// 完整标题（含路径尾段）。
    Full,
    /// 仅文件名。
    Name,
    /// 图标（正方形）。
    Icon,
}

/// 一只标签：标题 + 所辖滚动位（恢复账字段）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tab {
    pub title: String,
    /// 该标签的视口滚动偏移（恢复全状态判据的字段）。
    pub scroll_offset: u32,
    /// 是否钉选（钉选标签不收缩为 Icon）。
    pub pinned: bool,
}

/// 宽度档计算：条总宽 / 标签数 → 每签宽 → 档。
/// 阈值：≥160px Full；≥44px Name；否则 Icon（钉选标签最低 Name
/// ——钉了就给名字，图标认不出哪只是钉选）。
pub fn width_tier(count: usize, bar_w: u32, pinned_present: bool) -> TabWidth {
    let per = if count == 0 { bar_w } else { bar_w / count as u32 };
    if per >= 160 {
        TabWidth::Full
    } else if per >= 44 {
        TabWidth::Name
    } else if pinned_present {
        TabWidth::Name
    } else {
        TabWidth::Icon
    }
}

// ---------------------------------------------------------------------------
// 标签账（顺序 = 渲染序）
// ---------------------------------------------------------------------------

/// 关闭焦点转移规则（一处一事实）：右邻优先、无右邻取左邻。
pub fn close_focus_rule(count: usize, closing: usize) -> Option<usize> {
    if count == 0 || closing >= count {
        return None;
    }
    if count == 1 {
        return None; // 最后一签关掉——焦点交给桌面（调用方处理）
    }
    if closing + 1 < count {
        Some(closing + 1) // 右邻
    } else {
        Some(closing - 1) // 无右邻 → 左邻
    }
}

/// 标签管理器：顺序即渲染序；重排走模型。
pub struct TabManager {
    pub tabs: Vec<Tab>,
    pub active: usize,
    /// 被拒重排账（非法位置——审计可见）。
    pub rejected_moves: usize,
}

impl TabManager {
    pub fn new() -> TabManager {
        TabManager { tabs: Vec::new(), active: 0, rejected_moves: 0 }
    }

    /// 打开（追加在当前标签右侧——浏览器与资源管理器共有的心智：
    /// 新标签从「我现在在哪」长出来，不是甩到队尾）。
    pub fn open(&mut self, title: &str, scroll_offset: u32) -> usize {
        let idx = (self.active + 1).min(self.tabs.len());
        self.tabs.insert(
            idx,
            Tab { title: title.into(), scroll_offset, pinned: false },
        );
        self.active = idx;
        idx
    }

    /// 关闭：焦点转移按规则；返回焦点去向（None = 全关）。
    pub fn close(&mut self, idx: usize) -> Option<usize> {
        let focus = close_focus_rule(self.tabs.len(), idx);
        self.tabs.remove(idx);
        match focus {
            Some(f) => {
                // 移除点左侧的关闭使右邻左移一位。
                self.active = if idx < f { f - 1 } else { f };
                if self.active >= self.tabs.len() {
                    self.active = self.tabs.len().saturating_sub(1);
                }
                Some(self.active)
            }
            None => {
                self.tabs.clear();
                self.active = 0;
                None
            }
        }
    }

    /// 拖拽重排：from → to，越界/原位拒绝留账。
    pub fn move_tab(&mut self, from: usize, to: usize) -> bool {
        if from >= self.tabs.len() || to >= self.tabs.len() || from == to {
            self.rejected_moves += 1;
            return false;
        }
        let t = self.tabs.remove(from);
        self.tabs.insert(to, t);
        // 活动标签跟随其内容移动（焦点不因重排跳签）。
        if self.active == from {
            self.active = to;
        }
        true
    }

    /// 恢复：整表回填（重启恢复 10 标签全状态——含滚动位与活动签）。
    pub fn restore(&mut self, tabs: Vec<Tab>, active: usize) -> bool {
        if tabs.is_empty() || active >= tabs.len() {
            return false;
        }
        self.tabs = tabs;
        self.active = active;
        true
    }

    /// 溢出滚动：活动签保证可见（最小平移——与 h2list 焦点跟随同谱）。
    /// 返回条滚动偏移。
    pub fn scroll_to_active(&self, bar_w: u32, tab_w: u32) -> u32 {
        let tab_w = tab_w.max(1);
        let count = self.tabs.len();
        let total = (count as u32 * tab_w).saturating_sub(bar_w);
        let left = self.active as u32 * tab_w;
        let right = left + tab_w;
        // 用当前活动签位置推导（无状态滚动——纯函数口径）。
        let visible = bar_w.min(count as u32 * tab_w);
        if right > visible {
            (right - visible).min(total)
        } else {
            0
        }
    }

    pub fn len(&self) -> usize {
        self.tabs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tabs.is_empty()
    }
}

// ---------------------------------------------------------------------------
// 自检（判据逐条钉死）
// ---------------------------------------------------------------------------

pub fn run_h2tabmgr_checks() -> CheckSet {
    let mut set = CheckSet::new("h2-h2tabmgr");
    // 宽度三档：阈值边界逐档钉死。
    set.add(
        "h2tabmgr width tiers",
        width_tier(4, 800, false) == TabWidth::Full
            && width_tier(10, 800, false) == TabWidth::Name
            && width_tier(30, 800, false) == TabWidth::Icon,
        "160/44 thresholds",
    );
    // 钉选在场不收缩为 Icon。
    set.add(
        "h2tabmgr pinned floor",
        width_tier(30, 800, true) == TabWidth::Name,
        "pinned gets a name",
    );
    // 打开：从活动签右侧长出，活动随迁。
    let mut tm = TabManager::new();
    tm.open("首页", 0);
    tm.open("报告", 120);
    tm.active = 0;
    tm.open("新建", 0);
    set.add(
        "h2tabmgr open beside active",
        tm.len() == 3 && tm.tabs[1].title == "新建" && tm.active == 1,
        "grows from here",
    );
    // 关闭中间签：右邻接管；焦点不落空。
    tm.active = 0;
    let focus = tm.close(0);
    set.add(
        "h2tabmgr close right neighbor",
        focus == Some(0) && tm.tabs[0].title == "新建" && tm.len() == 2,
        "right neighbor takes over",
    );
    // 关最后一签（无右邻）：左邻接管。
    tm.active = tm.len() - 1;
    let last = tm.len() - 1;
    let focus2 = tm.close(last);
    set.add(
        "h2tabmgr close left neighbor",
        focus2 == Some(0) && tm.len() == 1,
        "left fallback",
    );
    // 拖拽重排：合法移动 + 活动签跟随；越界/原位拒绝留账。
    tm.open("末签", 0);
    tm.active = 0;
    set.add(
        "h2tabmgr reorder ok",
        tm.move_tab(0, 1) && tm.active == 1 && tm.tabs[1].title == "新建",
        "active follows content",
    );
    let bad1 = tm.move_tab(0, 99);
    let bad2 = tm.move_tab(1, 1);
    set.add(
        "h2tabmgr reorder reject",
        !bad1 && !bad2 && tm.rejected_moves == 2,
        "illegal moves audited",
    );
    // 恢复：10 签全状态（含滚动位）回填。
    let snap: Vec<Tab> = (0..10)
        .map(|i| Tab { title: alloc::format!("签{i}"), scroll_offset: i as u32 * 40, pinned: false })
        .collect();
    let mut rt = TabManager::new();
    set.add(
        "h2tabmgr restore full",
        rt.restore(snap.clone(), 7)
            && rt.len() == 10
            && rt.active == 7
            && rt.tabs[3].scroll_offset == 120,
        "10 tabs + scroll offsets",
    );
    set.add(
        "h2tabmgr restore reject",
        !rt.restore(snap, 10) && !rt.restore(vec![], 0),
        "bad snapshot refused",
    );
    // 溢出滚动：活动签超出可视带 → 最小平移钳到内容尾。
    let mut sm = TabManager::new();
    for i in 0..10 {
        sm.open(&alloc::format!("t{i}"), 0);
    }
    sm.active = 9;
    let off = sm.scroll_to_active(400, 80);
    set.add(
        "h2tabmgr overflow scroll",
        off == 400 && sm.scroll_to_active(4000, 80) == 0,
        "min shift + no scroll when fits",
    );
    // 焦点规则表：中间关右邻、尾关左邻、单签关空。
    set.add(
        "h2tabmgr focus rule",
        close_focus_rule(3, 0) == Some(1)
            && close_focus_rule(3, 2) == Some(1)
            && close_focus_rule(1, 0).is_none()
            && close_focus_rule(3, 3).is_none(),
        "rule table exact",
    );
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn h2tabmgr_all_green() {
        let set = run_h2tabmgr_checks();
        assert!(set.all_passed(), "h2tabmgr 自检有红项");
        assert!(!set.truncated(), "h2tabmgr 自检溢出");
    }

    #[test]
    fn close_never_loses_focus_midway() {
        // 20 签逐个关：每次关闭后活动签恒合法（焦点不落空不变式）。
        let mut tm = TabManager::new();
        for i in 0..20 {
            tm.open(&alloc::format!("t{i}"), 0);
        }
        while tm.len() > 1 {
            let before = tm.len();
            let f = tm.close(tm.active);
            assert!(f.is_some() && (f.unwrap() as usize) < tm.len());
            assert_eq!(tm.len(), before - 1);
        }
        assert!(tm.close(0).is_none());
        assert!(tm.is_empty());
    }
}
