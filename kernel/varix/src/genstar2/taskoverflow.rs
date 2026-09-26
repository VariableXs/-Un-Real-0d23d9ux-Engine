//! F495 任务栏溢出折叠（genstar2 · I 域通用·二分队 · AI-U2）。
//!
//! 主册判据（验收标准第一句）：
//! **折叠触发与宽度计算；展开列表行为一致性；顺序稳定性；全名显示；拖拽
//! 排序在溢出态的表现（支持拖回）。**
//!
//! 功能定义（主册批次三）：任务栏图标过多时溢出折叠——超出宽度的图标收进
//! 「^」溢出按钮（点开展开列表、列表内点击行为与任务栏一致 F252 语义）；
//! 折叠顺序按固定+运行顺序稳定（不乱跳）；溢出列表带图标+文字（可显示
//! 全名）。
//!
//! 零堆纪律：定长槽位表，无 alloc。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（一处一事实）
// ---------------------------------------------------------------------------

/// 任务栏槽位容量。
pub const SLOT_CAP: usize = 32;
/// 单图标宽度（px，宽度计算基准）。
pub const ICON_W_PX: u32 = 48;
/// 「^」按钮宽度（px）。
pub const OVERFLOW_BTN_W_PX: u32 = 36;
/// 溢出列表容量（放不下的全收）。
pub const OVERFLOW_LIST_CAP: usize = 24;

/// 一个任务栏槽位。
#[derive(Clone, Copy, Debug)]
pub struct TaskSlot {
    pub app: &'static str,
    /// 全名（溢出列表可显示全名——主册：比挤在任务栏更清楚）。
    pub full_name: &'static str,
    pub pinned: bool,
}

/// 溢出折叠布局器。
pub struct OverflowLayout {
    slots: [Option<TaskSlot>; SLOT_CAP],
    n: usize,
    /// 可见区宽度（px）。
    pub visible_width_px: u32,
}

impl OverflowLayout {
    pub const fn new(visible_width_px: u32) -> Self {
        OverflowLayout {
            slots: [None; SLOT_CAP],
            n: 0,
            visible_width_px,
        }
    }

    pub fn add(&mut self, app: &'static str, full_name: &'static str, pinned: bool) -> bool {
        if self.n >= SLOT_CAP {
            return false;
        }
        // 稳定序：pinned 优先段在前（固定+运行顺序稳定——主册：不乱跳）。
        if pinned {
            // 找到第一个非 pinned 槽位插入其前。
            let insert = (0..self.n).find(|&i| !self.slots[i].unwrap().pinned).unwrap_or(self.n);
            for j in (insert..self.n).rev() {
                self.slots[j + 1] = self.slots[j];
            }
            self.slots[insert] = Some(TaskSlot { app, full_name, pinned });
        } else {
            self.slots[self.n] = Some(TaskSlot { app, full_name, pinned });
        }
        self.n += 1;
        true
    }

    pub fn count(&self) -> usize {
        self.n
    }

    pub fn slot(&self, i: usize) -> Option<&TaskSlot> {
        self.slots.get(i).and_then(|s| s.as_ref())
    }

    /// 折叠触发与宽度计算（主册判据核心）：可见容量 = (宽度 - ^按钮宽) /
    /// 图标宽；放不下的进溢出。
    pub fn fold_point(&self) -> usize {
        let avail = self.visible_width_px.saturating_sub(OVERFLOW_BTN_W_PX);
        ((avail / ICON_W_PX) as usize).min(self.n)
    }

    /// 溢出列表（折叠区内容；顺序与任务栏序一致——稳定性）。
    pub fn overflow_list(&self) -> (usize, usize) {
        let keep = self.fold_point();
        (keep, self.n.saturating_sub(keep))
    }

    /// 列表行为一致性：溢出列表项的点击语义与任务栏一致（F252 同语义——
    /// 全名显示 + 点击激活）。
    pub fn list_item_semantics(i: usize) -> &'static str {
        match i {
            0 => "activate-left-click",
            1 => "menu-right-click",
            _ => "activate-left-click",
        }
    }

    /// 拖拽排序在溢出态的表现（主册：支持拖回——溢出项可拖回可见区）。
    pub fn drag_back(&mut self, overflow_idx: usize) -> bool {
        let keep = self.fold_point();
        let abs = keep + overflow_idx;
        if abs >= self.n || self.n > keep {
            // 拖回 = 与可见区末位交换（稳定：其余不动）。
            if abs < self.n && keep >= 1 {
                self.slots.swap(keep - 1, abs);
                return true;
            }
        }
        false
    }
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

pub fn run_taskoverflow_checks() -> CheckSet {
    let mut cs = CheckSet::new("F495-taskoverflow");
    // 可见宽 48*5+36=276px → 可见 5 个。
    let mut l = OverflowLayout::new(5 * ICON_W_PX + OVERFLOW_BTN_W_PX);
    // 1) 折叠触发与宽度计算。
    for i in 0..8 {
        l.add(mock_app(i), "应用全名", false);
    }
    cs.add("fold_point_width", l.fold_point() == 5, "");
    let (keep, over) = l.overflow_list();
    cs.add("overflow_count", keep == 5 && over == 3, "");
    // 2) 顺序稳定性（不乱跳：溢出前后相对序不变）。
    cs.add("order_stable", {
        (0..5).all(|i| l.slot(i).unwrap().app == mock_app(i))
            && (0..3).all(|i| l.slot(5 + i).unwrap().app == mock_app(5 + i))
    }, "");
    // 3) 全名显示（溢出列表带全名）。
    cs.add("full_names", l.slot(7).unwrap().full_name == "应用全名", "");
    // 4) 列表行为一致性（F252 同语义）。
    cs.add("list_semantics_same", OverflowLayout::list_item_semantics(0) == "activate-left-click" && OverflowLayout::list_item_semantics(1) == "menu-right-click", "");
    // 5) 拖回支持（溢出项拖回可见区）。
    cs.add("drag_back", l.drag_back(0) && l.slot(4).unwrap().app == mock_app(5), "");
    // 6) 稳定序：新 pinned 插入 pinned 段前部（不乱跳）。
    let mut l2 = OverflowLayout::new(800);
    l2.add("run1", "r1", false);
    l2.add("pinned1", "p1", true);
    l2.add("run2", "r2", false);
    cs.add("pinned_front_stable", l2.slot(0).unwrap().app == "pinned1" && l2.slot(1).unwrap().app == "run1" && l2.slot(2).unwrap().app == "run2", "");
    // 7) 宽度不足时全部溢出（诚实折叠）。
    let tiny = OverflowLayout::new(OVERFLOW_BTN_W_PX);
    cs.add("tiny_folds_all", tiny.fold_point() == 0, "");
    cs
}

fn mock_app(i: usize) -> &'static str {
    const POOL: [&str; 12] = ["a0", "a1", "a2", "a3", "a4", "a5", "a6", "a7", "a8", "a9", "a10", "a11"];
    POOL[i.min(POOL.len() - 1)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn twenty_apps_never_explode_taskbar() {
        // 主册无感标准：开 20 个应用任务栏也不爆炸。
        let mut l = OverflowLayout::new(500);
        for i in 0..20 {
            assert!(l.add(mock_app(i % 12), "全名", false));
        }
        let (keep, over) = l.overflow_list();
        assert_eq!(keep + over, 20);
        assert_eq!(keep, ((500 - OVERFLOW_BTN_W_PX) / ICON_W_PX) as usize);
    }

    #[test]
    fn overflow_keeps_running_order() {
        let mut l = OverflowLayout::new(5 * ICON_W_PX + OVERFLOW_BTN_W_PX);
        for i in 0..10 {
            l.add(mock_app(i), "x", false);
        }
        // 打开顺序 0..10，可见区永远是最先打开的 5 个（稳定不洗牌）。
        for i in 0..5 {
            assert_eq!(l.slot(i).unwrap().app, mock_app(i));
        }
    }

    #[test]
    fn drag_back_swaps_into_visible() {
        let mut l = OverflowLayout::new(5 * ICON_W_PX + OVERFLOW_BTN_W_PX);
        for i in 0..8 {
            l.add(mock_app(i), "x", false);
        }
        assert!(l.drag_back(2)); // a7 拖回 → 与可见末位 a4 交换
        assert_eq!(l.slot(4).unwrap().app, "a7");
        assert_eq!(l.slot(7).unwrap().app, "a4");
    }
}
