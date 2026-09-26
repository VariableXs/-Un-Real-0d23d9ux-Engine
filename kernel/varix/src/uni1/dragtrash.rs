//! F414 拖拽进回收站 · 完整设计（STAR I 主册 G-I-14）。
//!
//! **判据（主册）**：语义一致性（拖拽/Delete/右键删除三路同归）；高亮
//! 反馈；还原路径；拖放判定区域；误拖撤销（F202 联动）。＋通12。
//!
//! 设计：三路删除统一入口核——拖入判定区（图标判定盒 + 高亮态）进站，
//! 与 Delete/右键删除同归同语义（同进站、同可还原）；高亮反馈状态机
//! （悬停进判定区 → 高亮+放大 1.05 → 松手进站/离开复原）；误拖撤销
//! 钩子（F202 撤销链记账）。

use crate::checks::CheckSet;

use alloc::vec::Vec;

/// 高亮放大倍率（1.05，唯一登记点）。
pub const HIGHLIGHT_SCALE_PERMILLE: u32 = 1_050;

/// 回收站图标判定盒（px）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HitBox {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl HitBox {
    pub fn contains(&self, px: i32, py: i32) -> bool {
        px >= self.x && px < self.x + self.w && py >= self.y && py < self.y + self.h
    }
}

/// 删除三路。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeletePath {
    DragDrop,
    DeleteKey,
    CtxMenu,
}

/// 拖拽进回收站语义核。
pub struct DragTrash {
    pub box_: HitBox,
    /// 悬停高亮态。
    pub highlight: bool,
    /// 进站账（三路同账——同归判据）。
    pub trashed_by_path: [u64; 3],
    /// 进站条目（可还原——还原路径判据）。
    pub items: Vec<u64>,
    /// 撤销账（F202 联动：误拖撤销成功次数）。
    pub undo_count: u64,
    /// 还原账（回收站拖出/右键还原）。
    pub restore_count: u64,
}

impl DragTrash {
    pub fn new(box_: HitBox) -> DragTrash {
        DragTrash { box_, highlight: false, trashed_by_path: [0; 3], items: Vec::new(), undo_count: 0, restore_count: 0 }
    }

    /// 拖拽悬停反馈：进判定区 → 高亮 + 1.05；离开 → 复原。
    pub fn drag_hover(&mut self, px: i32, py: i32) -> bool {
        self.highlight = self.box_.contains(px, py);
        self.highlight
    }

    /// 统一进站口：三路同归（拖拽路径必须先 hover 高亮才允许松手进站）。
    pub fn trash(&mut self, path: DeletePath, ids: &[u64], drop_confirmed: bool) -> bool {
        if path == DeletePath::DragDrop {
            if !drop_confirmed || !self.highlight {
                self.highlight = false; // 松手未进站/未确认 → 复原
                return false;
            }
        }
        for id in ids {
            if !self.items.contains(id) {
                self.items.push(*id);
            }
        }
        self.trashed_by_path[path as usize] += ids.len() as u64;
        self.highlight = false;
        true
    }

    /// 还原（回收站拖出/右键还原——后悔药不因路径不同失效）。
    pub fn restore(&mut self, id: u64) -> bool {
        let before = self.items.len();
        self.items.retain(|i| *i != id);
        let removed = self.items.len() != before;
        if removed {
            self.restore_count += 1;
        }
        removed
    }

    /// 误拖撤销（F202 链）：进站后立即撤回。
    pub fn undo_last(&mut self) -> bool {
        match self.items.pop() {
            Some(_) => {
                self.undo_count += 1;
                true
            }
            None => false,
        }
    }

    /// 三路同归审计：三路计数都走同一 items 容器（结构性一致：
    /// 进站总数 = 在站数 + 撤销数 + 还原数——物料守恒）。
    pub fn all_paths_same_sink(&self) -> bool {
        self.trashed_by_path.iter().sum::<u64>()
            == self.items.len() as u64 + self.undo_count + self.restore_count
    }
}

pub fn run_dragtrash_checks() -> CheckSet {
    let mut set = CheckSet::new("uni1-F414");
    let mut t = DragTrash::new(HitBox { x: 800, y: 500, w: 64, h: 64 });
    // 判定区命中与高亮。
    set.add("f414-hover-in", t.drag_hover(830, 530), "");
    set.add("f414-hover-out", !t.drag_hover(100, 100) && !t.highlight, "");
    // 三路同归：三路删除同进 items 容器。
    t.drag_hover(830, 530);
    set.add("f414-drag-in", t.trash(DeletePath::DragDrop, &[1, 2], true), "");
    set.add("f414-delkey-in", t.trash(DeletePath::DeleteKey, &[3], false), "");
    set.add("f414-ctx-in", t.trash(DeletePath::CtxMenu, &[4], false), "");
    set.add(
        "f414-three-paths-same-sink",
        t.trashed_by_path == [2, 1, 1] && t.items == alloc::vec![1, 2, 3, 4] && t.all_paths_same_sink(),
        "",
    );
    // 未高亮松手 = 不进站（拖放判定区域判据）。
    t.drag_hover(10, 10);
    set.add(
        "f414-drop-outside-rejected",
        !t.trash(DeletePath::DragDrop, &[9], true) && !t.items.contains(&9),
        "",
    );
    // 还原路径 + 误拖撤销。
    set.add("f414-restore", t.restore(2) && !t.items.contains(&2), "");
    set.add("f414-undo", t.undo_last() && t.undo_count == 1 && !t.items.contains(&4), "");
    set.add("f414-ledger-consistent", t.all_paths_same_sink(), "");
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duplicate_ids_not_double_counted() {
        let mut t = DragTrash::new(HitBox { x: 0, y: 0, w: 10, h: 10 });
        t.trash(DeletePath::DeleteKey, &[7, 7], false);
        assert_eq!(t.items, alloc::vec![7], "同 id 不重复进站");
    }

    #[test]
    fn undo_empty_is_false() {
        let mut t = DragTrash::new(HitBox { x: 0, y: 0, w: 10, h: 10 });
        assert!(!t.undo_last());
        assert_eq!(t.undo_count, 0);
    }
}
