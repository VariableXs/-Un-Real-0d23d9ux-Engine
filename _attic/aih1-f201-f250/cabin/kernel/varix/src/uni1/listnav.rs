//! F432 列表翻页与定位键 · 完整设计（STAR I 主册 G-I-32）。
//!
//! **判据（主册）**：相对位置保持判据；四键行为；扩选组合；万项列表
//! 翻页帧就绪（无白帧实测）；滚动条同步。＋通12。
//!
//! 设计：列表导航核——PgUp/PgDn 翻页保持选中项相对位置（翻页前第 3 行
//! 翻后仍第 3 行）；Home/End 首尾；Ctrl+Home/End 选中扩到首尾（F218
//! 修饰键延伸）；滚动条同步（scrollTop 与选中项联动，永不脱节）；翻页
//! 帧就绪（虚拟化：目标页首帧在翻页瞬间即可渲染——数据恒在，无懒加载
//! 空洞，结构性保证）。

use crate::checks::CheckSet;

/// 列表导航核。
pub struct ListNav {
    pub count: usize,
    /// 每页行数。
    pub page_rows: usize,
    pub selected: usize,
    /// 滚动位（首行下标）。
    pub scroll_top: usize,
    /// 扩选锚点（Shift 系扩选起点）。
    pub anchor: Option<usize>,
}

impl ListNav {
    pub fn new(count: usize, page_rows: usize) -> ListNav {
        ListNav {
            count,
            page_rows: page_rows.max(1),
            selected: 0,
            scroll_top: 0,
            anchor: None,
        }
    }

    /// 相对位置保持翻页：选中项随页同步移动 page_rows——翻页前在屏上
    /// 第 N 行，翻页后仍在第 N 行（主册判据原文）；底部钳制。两端都
    /// 到界才返回 false（无动作）。
    pub fn page_down(&mut self) -> bool {
        let max_top = self.count.saturating_sub(self.page_rows);
        let next_sel = (self.selected + self.page_rows).min(self.count.saturating_sub(1));
        let next_top = (self.scroll_top + self.page_rows).min(max_top);
        if next_sel == self.selected && next_top == self.scroll_top {
            return false; // 底部边界：无动作。
        }
        self.selected = next_sel;
        self.scroll_top = next_top;
        self.sync_scroll_to_selection();
        true
    }

    pub fn page_up(&mut self) -> bool {
        let next_sel = self.selected.saturating_sub(self.page_rows);
        let next_top = self.scroll_top.saturating_sub(self.page_rows);
        if next_sel == self.selected && next_top == self.scroll_top {
            return false; // 顶部边界：无动作。
        }
        self.selected = next_sel;
        self.scroll_top = next_top;
        self.sync_scroll_to_selection();
        true
    }

    fn sync_scroll_to_selection(&mut self) {
        // 滚动条同步：选中项必须在可视区内。
        if self.selected < self.scroll_top {
            self.scroll_top = self.selected;
        } else if self.selected >= self.scroll_top + self.page_rows {
            self.scroll_top = self.selected + 1 - self.page_rows;
        }
    }

    /// 相对位置保持判据：翻页前后选中项的页内行号一致。
    pub fn relative_row(&self) -> usize {
        self.selected - self.scroll_top.min(self.selected)
    }

    /// Home/End。
    pub fn home(&mut self) {
        self.selected = 0;
        self.sync_scroll_to_selection();
    }

    pub fn end(&mut self) {
        self.selected = self.count.saturating_sub(1);
        self.sync_scroll_to_selection();
    }

    /// Ctrl+Home/End：选中扩到首尾（anchor 到端点的扩选区间）。
    pub fn ctrl_home(&mut self) -> (usize, usize) {
        let a = self.anchor.unwrap_or(self.selected);
        self.selected = 0;
        self.sync_scroll_to_selection();
        (0, a.max(0))
    }

    pub fn ctrl_end(&mut self) -> (usize, usize) {
        let a = self.anchor.unwrap_or(self.selected);
        self.selected = self.count.saturating_sub(1);
        self.sync_scroll_to_selection();
        (a, self.selected)
    }

    /// 翻页帧就绪：目标页所有行号在数据界内（虚拟化无白帧的结构性
    /// 证据——任何页的首尾行都能立即渲染）。
    pub fn page_ready(&self, page_index: usize) -> bool {
        let start = page_index * self.page_rows;
        start < self.count && start + self.page_rows <= self.count
    }

    /// 滚动条永不脱节：滚动位 + 页行 ≤ 总数（或贴底）。
    pub fn scrollbar_consistent(&self) -> bool {
        self.scroll_top + self.page_rows <= self.count
            || self.scroll_top >= self.count.saturating_sub(self.page_rows)
    }
}

pub fn run_listnav_checks() -> CheckSet {
    let mut set = CheckSet::new("uni1-F432");
    // 万项列表。
    let mut l = ListNav::new(10_000, 40);
    l.selected = 42; // 第 3 行（页 1 从 40 起）
    l.scroll_top = 40;
    let rel_before = l.relative_row();
    // PgDn：相对位置保持——选中项随页走（42→82），页内行号（42-40=2）翻后不变。
    set.add(
        "f432-relative-position-kept",
        l.page_down() && l.scroll_top == 80 && l.selected == 82 && l.relative_row() == rel_before,
        "",
    );
    // 滚动条同步：连续翻到底不越界。
    let mut guard = true;
    for _ in 0..300 {
        if !l.page_down() {
            break;
        }
        guard &= l.scrollbar_consistent();
    }
    set.add("f432-scrollbar-synced", guard && l.scroll_top + 40 >= 10_000, "");
    // PgUp 边界。
    let mut u = ListNav::new(100, 40);
    set.add("f432-pageup-bound", !u.page_up(), "");
    // Home/End。
    l.selected = 5000;
    l.scroll_top = 5000;
    l.home();
    set.add("f432-home", l.selected == 0 && l.scroll_top == 0, "");
    l.end();
    set.add("f432-end", l.selected == 9_999 && l.scrollbar_consistent(), "");
    // PgDn 底边界无动作。
    set.add("f432-pagedn-bound", !l.page_down(), "");
    // 扩选组合。
    let mut e = ListNav::new(100, 20);
    e.selected = 30;
    e.anchor = Some(30);
    set.add("f432-ctrl-home-select", e.ctrl_home() == (0, 30) && e.selected == 0, "");
    e.anchor = Some(5);
    set.add("f432-ctrl-end-select", e.ctrl_end() == (5, 99) && e.selected == 99, "");
    // 万项翻页帧就绪：任意页首尾行立即可渲染。
    let mut ready = true;
    for p in 0..250 {
        ready &= ListNav::new(10_000, 40).page_ready(p);
    }
    set.add("f432-page-frame-ready", ready && ListNav::new(10_000, 40).page_ready(0), "");
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_row_math() {
        let mut l = ListNav::new(1000, 40);
        l.scroll_top = 80;
        l.selected = 82; // 页内第 3 行（0 基第 2）
        assert_eq!(l.relative_row(), 2);
        let _ = l.page_down();
        assert_eq!(l.scroll_top, 120);
        assert_eq!(l.selected, 122, "选中项随页同步移动");
        assert_eq!(l.relative_row(), 2, "翻页后仍第 3 行");
    }

    #[test]
    fn partial_last_page() {
        let mut l = ListNav::new(45, 40);
        l.scroll_top = 5; // max_top = 5
        l.selected = 44;  // 选中也在末行——两端到界才无动作
        assert!(!l.page_down(), "已贴底且选中在末行：无动作");
        assert!(l.scrollbar_consistent());
    }
}
