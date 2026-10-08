//! F553 多选拖影计数 · 完整设计（STAR I 主册 I 域批次七）。
//!
//! **判据（主册）**：计数准确（7/20 项实测）；加选实时；单拖无徽标；
//! 徽标几何（右下 12px 偏移）；与 F255/F262 拖影体系一致。
//!
//! **设计要点（主册）**：
//! - 拖影显示代表性图标 + 右下角数字徽标（「×7」）——拖几个心里有数；
//! - 单个文件无徽标（干净）；
//! - 徽标数字实时反映当前选择（拖动中按 Ctrl 加选时数字跟着变）；
//! - 徽标几何：右下 12px 偏移，小而清楚不遮拖影。
//!
//! 与 F255（拖影视觉）/F262（拖动会话）的一致性：本模块只管「计数与徽标」，
//! 拖影本体与拖动会话由接缝注入（选择集变化回调），不反向造编译依赖。

use crate::checks::CheckSet;
use crate::istar::ibase::ISTAR_DOMAIN;

// ---------------------------------------------------------------------------
// 规格常量
// ---------------------------------------------------------------------------

/// 徽标右下偏移（px，主册定档）。
pub const BADGE_OFFSET_PX: u32 = 12;

/// 单拖阈值：选择数 ≤ 1 无徽标。
pub const BADGE_MIN_COUNT: usize = 2;

// ---------------------------------------------------------------------------
// 拖影计数器
// ---------------------------------------------------------------------------

/// 多选拖影计数器（挂在 F262 拖动会话上，选择集变化即刷新）。
pub struct DragBadge {
    /// 当前选择（文件 id 集——去重保持插入序）。
    selected: [u64; 256],
    sel_len: usize,
    /// 拖动会话是否在途（F262 接缝注入）。
    dragging: bool,
    /// 拖动中发生的加选/减选次数（实时性对账）。
    live_updates: usize,
}

impl DragBadge {
    pub fn new() -> DragBadge {
        DragBadge {
            selected: [0; 256],
            sel_len: 0,
            dragging: false,
            live_updates: 0,
        }
    }

    /// 加选（Ctrl 点选 / 框选增量；重复 id 幂等）。
    pub fn select(&mut self, id: u64) {
        if !self.selected[..self.sel_len].contains(&id) && self.sel_len < 256 {
            self.selected[self.sel_len] = id;
            self.sel_len += 1;
            if self.dragging {
                self.live_updates += 1;
            }
        }
    }

    /// 减选（Ctrl 再点 / Esc 取消单个）。
    pub fn deselect(&mut self, id: u64) {
        if let Some(pos) = self.selected[..self.sel_len].iter().position(|&x| x == id) {
            self.selected.copy_within(pos + 1..self.sel_len, pos);
            self.sel_len -= 1;
            if self.dragging {
                self.live_updates += 1;
            }
        }
    }

    /// 清空选择。
    pub fn clear(&mut self) {
        self.sel_len = 0;
        if self.dragging {
            self.live_updates += 1;
        }
    }

    /// 拖动开始/结束（F262 会话接缝）。
    pub fn set_dragging(&mut self, on: bool) {
        self.dragging = on;
    }

    /// 当前计数。
    pub fn count(&self) -> usize {
        self.sel_len
    }

    /// 徽标可见性：拖动中且计数 ≥ 2（单拖干净无徽标）。
    pub fn badge_visible(&self) -> bool {
        self.dragging && self.sel_len >= BADGE_MIN_COUNT
    }

    /// 徽标文案「×N」。
    pub fn badge_text(&self) -> &'static str {
        // 文案渲染在 UI 层做数字格式化；内核侧返回模板（× 前缀唯一源）。
        "×"
    }

    /// 徽标几何（相对拖影图标右下角的偏移）。
    pub fn badge_offset(&self) -> (u32, u32) {
        (BADGE_OFFSET_PX, BADGE_OFFSET_PX)
    }

    /// 拖动中加/减选实时刷新次数（对账面）。
    pub fn live_update_count(&self) -> usize {
        self.live_updates
    }
}

impl Default for DragBadge {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub fn run_dragbadge_checks() -> CheckSet {
    let mut set = CheckSet::new(ISTAR_DOMAIN);

    // 1. 计数准确：7 项选择 → ×7；20 项 → ×20（主册实测口径同参）。
    let mut b = DragBadge::new();
    b.set_dragging(true);
    for i in 1..=7u64 {
        b.select(i);
    }
    let seven = b.count() == 7 && b.badge_visible();
    for i in 8..=20u64 {
        b.select(i);
    }
    set.add("count 7 then 20 accurate", seven && b.count() == 20, "");

    // 2. 加选实时：拖动中加选数字跟着变（live_updates 随每次加/减选递增）。
    b.select(21);
    let live = b.count() == 21 && b.live_update_count() == 21; // 前 20 次加选 + 本次
    set.add("ctrl add updates live while dragging", live, "");

    // 3. 单拖无徽标：计数 1 → 徽标不可见（干净）。
    let mut b2 = DragBadge::new();
    b2.set_dragging(true);
    b2.select(100);
    set.add("single drag no badge", !b2.badge_visible() && b2.count() == 1, "");

    // 4. 徽标几何：右下 12px 偏移（x=y=12）。
    let b3 = DragBadge::new();
    set.add(
        "badge offset 12px bottom-right",
        b3.badge_offset() == (BADGE_OFFSET_PX, BADGE_OFFSET_PX),
        "",
    );

    // 5. 未拖动时有选择也不出徽标（徽标只属于拖动会话）。
    let mut b4 = DragBadge::new();
    b4.select(1);
    b4.select(2);
    set.add("badge only in drag session", !b4.badge_visible(), "");

    // 6. 减选实时：拖动中减到 1 → 徽标即时消失。
    let mut b5 = DragBadge::new();
    b5.set_dragging(true);
    b5.select(1);
    b5.select(2);
    b5.deselect(2);
    set.add("deselect to one hides badge", b5.count() == 1 && !b5.badge_visible(), "");

    // 7. 重复加选幂等（连点同项不虚增计数）。
    let mut b6 = DragBadge::new();
    b6.select(9);
    b6.select(9);
    b6.select(9);
    set.add("duplicate select idempotent", b6.count() == 1, "");

    // 8. 清空归零（松手/取消路径）。
    b6.clear();
    set.add("clear zeroes selection", b6.count() == 0 && !b6.badge_visible(), "");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mixed_add_remove_live() {
        let mut b = DragBadge::new();
        b.set_dragging(true);
        for i in 1..=5u64 {
            b.select(i);
        }
        b.deselect(3);
        assert_eq!(b.count(), 4);
        assert_eq!(b.live_update_count(), 6); // 5 加 + 1 减
    }

    #[test]
    fn clear_while_dragging_counts_update() {
        let mut b = DragBadge::new();
        b.set_dragging(true);
        b.select(1);
        b.clear();
        assert_eq!(b.live_update_count(), 2);
        assert!(!b.badge_visible());
    }

    #[test]
    fn badge_text_is_times_prefix() {
        let b = DragBadge::new();
        assert_eq!(b.badge_text(), "×");
    }
}
