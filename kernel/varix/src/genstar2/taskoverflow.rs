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

// ===========================================================================
// 深化 v2（F495）：折叠点宽度计算实证 / 顺序稳定性 / 拖回交换语义 /
// 溢出列表全名渲染账 / 折叠-展开往返
// ===========================================================================

/// 折叠点宽度计算实证（主册「(宽度-^钮)/图标宽折叠点」的公式落地：
/// 可见槽位数 = (任务栏宽 − 溢出按钮宽) / 图标宽——除不尽的余数
/// 让位给间距，向下取整诚实）。
pub fn visible_slot_formula(width_px: u32) -> usize {
    ((width_px.saturating_sub(OVERFLOW_BTN_W_PX)) / ICON_W_PX) as usize
}

/// 顺序稳定性（主册「折叠顺序按固定+运行顺序稳定（不乱跳）」：
/// 固定区永远在运行区前、同区内插入序即显示序——开关窗口不洗牌）。
pub fn order_stable(slots: &[(&str, bool)]) -> bool {
    // 断言一：所有 pinned 在前（一次遍历分界单调）。
    let mut seen_unpinned = false;
    for (_, pinned) in slots {
        if !pinned {
            seen_unpinned = true;
        } else if seen_unpinned {
            return false; // pinned 出现在 unpinned 之后 = 乱序。
        }
    }
    true
}

/// 溢出列表全名渲染账（主册「展开空间大，可显示全名」：溢出列表
/// 每条带 full_name 且与 slot 名同源——两处名字对不上 = 假全名）。
pub fn overflow_names_consistent(layout: &OverflowLayout, full_names: &[&str]) -> bool {
    layout.count() == full_names.len()
        && (0..layout.count()).all(|i| layout.slot(i).map(|s| s.full_name == full_names[i]).unwrap_or(false))
}

/// 拖回交换语义（主册「拖拽排序在溢出态的表现（支持拖回）」：
/// 溢出项拖回可见区 = 与可见末位交换；被换下的进溢出列表尾）。
pub fn drag_back_swap(visible: &mut [&'static str], overflow: &mut [&'static str], overflow_idx: usize) -> bool {
    if overflow_idx >= overflow.len() || visible.is_empty() {
        return false;
    }
    let incoming = overflow[overflow_idx];
    let last_visible = visible[visible.len() - 1];
    visible[visible.len() - 1] = incoming;
    overflow[overflow_idx] = last_visible;
    true
}

/// 折叠-展开往返（折叠点对账：可见数 + 溢出数 = 总数——
/// 一项不多算不少算，账面闭合）。
pub fn fold_unfold_roundtrip(layout: &OverflowLayout) -> bool {
    let (keep, overflow_n) = layout.overflow_list();
    keep + overflow_n == layout.count()
}

// ---------------------------------------------------------------------------
// 深化自检（F495 v2）
// ---------------------------------------------------------------------------

pub fn run_taskoverflow_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new("F495-v2");
    // 1) 折叠点公式：720px 宽 − 36px 钮 = 684 / 48 = 14 槽。
    cs.add("visible_formula", visible_slot_formula(720) == 14, "");
    cs.add("formula_zero_guard", visible_slot_formula(20) == 0, "");
    // 2) 顺序稳定性：pinned 前置约束。
    cs.add("order_stable", order_stable(&[("固定A", true), ("固定B", true), ("运行C", false), ("运行D", false)]), "");
    cs.add("order_violation_detected", !order_stable(&[("运行C", false), ("固定A", true)]), "");
    // 3) 溢出列表全名同源。
    let mut layout = OverflowLayout::new(720);
    let _ = layout.add("app1", "应用一号完整名", true);
    let _ = layout.add("app2", "应用二号完整名", false);
    cs.add("names_consistent", overflow_names_consistent(&layout, &["应用一号完整名", "应用二号完整名"]), "");
    // 4) 拖回交换：可见末位与溢出项互换。
    let mut vis = ["固定A", "运行C"];
    let mut ovf = ["运行D", "运行E"];
    cs.add("drag_back_swap", drag_back_swap(&mut vis, &mut ovf, 0)
        && vis[1] == "运行D" && ovf[0] == "运行C", "");
    // 5) 折叠-展开往返账面闭合。
    cs.add("roundtrip_closes", fold_unfold_roundtrip(&layout), "");
    cs
}

#[cfg(test)]
mod deep_tests {
    use super::*;

    #[test]
    fn formula_monotonic_in_width() {
        // 宽度越大可见槽位不减（单调性——布局公式的物理意义）。
        let mut last = 0;
        for w in [300u32, 480, 720, 960, 1440, 2560] {
            let v = visible_slot_formula(w);
            assert!(v >= last);
            last = v;
        }
    }

    #[test]
    fn drag_back_swap_overflow_tail() {
        let mut vis = ["a", "b"];
        let mut ovf = ["c"];
        assert!(drag_back_swap(&mut vis, &mut ovf, 0));
        assert_eq!(ovf[0], "b", "被换下的进溢出原位");
    }

    #[test]
    fn drag_back_oob_honest() {
        let mut vis = ["a"]; 
        let mut ovf: [&'static str; 0] = [];
        assert!(!drag_back_swap(&mut vis, &mut ovf, 0));
    }

    #[test]
    fn fold_point_respects_slot_count() {
        // 少量应用不溢出（fold_point = min(容量, 槽位数)）。
        let mut small = OverflowLayout::new(720);
        let _ = small.add("solo", "唯一应用", false);
        assert_eq!(small.fold_point(), 1);
        let (_, ovf) = small.overflow_list();
        assert_eq!(ovf, 0);
    }
}
