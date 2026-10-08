//! F414 拖拽进回收站 · 完整设计（STAR I 主册 G-I-14）。
//!
//! **判据（主册）**：语义一致性（拖拽/Delete/右键删除三路同归）；高亮
//! 反馈；还原路径；拖放判定区域；误拖撤销（F202 联动）。＋通12。
//!
//! 设计：三路删除统一入口核——拖入判定区（图标判定盒 + 高亮态）进站，
//! 与 Delete/右键删除同归同语义（同进站、同可还原）；高亮反馈状态机
//! （悬停进判定区 → 高亮+放大 1.05 → 松手进站/离开复原）；误拖撤销
//! 钩子（F202 撤销链记账）。
//!
//! v5 纵深：守恒口径修正（重复 id 不虚计进站账——物料守恒严格成立）；
//! 容量上限整批原子拒绝（部分进站=半截状态，禁止）；永久清空带确认位
//! 并入守恒账；多步撤销（undo_last_n）。

use crate::checks::CheckSet;

use alloc::vec::Vec;

/// 高亮放大倍率（1.05，唯一登记点）。
pub const HIGHLIGHT_SCALE_PERMILLE: u32 = 1_050;

/// 默认回收站容量（条）。
pub const TRASH_CAP_DEFAULT: u64 = 64;

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
    /// 进站账（三路同账——同归判据；只计实际新增，重复 id 不虚计）。
    pub trashed_by_path: [u64; 3],
    /// 进站条目（可还原——还原路径判据）。
    pub items: Vec<u64>,
    /// 撤销账（F202 联动：误拖撤销成功次数）。
    pub undo_count: u64,
    /// 还原账（回收站拖出/右键还原）。
    pub restore_count: u64,
    /// 容量上限（条）——满时整批原子拒绝。
    pub cap: u64,
    /// 永久清空账（守恒出口之一）。
    pub emptied: u64,
    /// 满容量整批拒绝账。
    pub rejected_batches: u64,
    /// 最近一批实际新增条数（守恒口径的可审计值）。
    pub last_batch_added: u64,
}

impl DragTrash {
    pub fn new(box_: HitBox) -> DragTrash {
        DragTrash {
            box_,
            highlight: false,
            trashed_by_path: [0; 3],
            items: Vec::new(),
            undo_count: 0,
            restore_count: 0,
            cap: TRASH_CAP_DEFAULT,
            emptied: 0,
            rejected_batches: 0,
            last_batch_added: 0,
        }
    }

    /// 拖拽悬停反馈：进判定区 → 高亮 + 1.05；离开 → 复原。
    pub fn drag_hover(&mut self, px: i32, py: i32) -> bool {
        self.highlight = self.box_.contains(px, py);
        self.highlight
    }

    /// 统一进站口：三路同归（拖拽路径必须先 hover 高亮才允许松手进站）。
    /// 重复 id 不重复进站、不虚计账；容量满时整批原子拒绝。
    pub fn trash(&mut self, path: DeletePath, ids: &[u64], drop_confirmed: bool) -> bool {
        if path == DeletePath::DragDrop {
            if !drop_confirmed || !self.highlight {
                self.highlight = false; // 松手未进站/未确认 → 复原
                return false;
            }
        }
        let mut new_ids: Vec<u64> = Vec::new();
        for id in ids {
            // 去重双查：不在站 + 不在本批已收（批内重复也不虚计）。
            if !self.items.contains(id) && !new_ids.contains(id) {
                new_ids.push(*id);
            }
        }
        if self.items.len() as u64 + new_ids.len() as u64 > self.cap {
            self.rejected_batches += 1;
            self.highlight = false;
            return false; // 整批拒绝（原子性：要么全进要么全不进）
        }
        for id in &new_ids {
            self.items.push(*id);
        }
        self.trashed_by_path[path as usize] += new_ids.len() as u64;
        self.last_batch_added = new_ids.len() as u64;
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

    /// 误拖撤销（F202 链）：进站后立即撤回一步。
    pub fn undo_last(&mut self) -> bool {
        match self.items.pop() {
            Some(_) => {
                self.undo_count += 1;
                true
            }
            None => false,
        }
    }

    /// 多步撤销：连撤 n 步（返回实际撤掉条数）。
    pub fn undo_last_n(&mut self, n: usize) -> u64 {
        let mut popped = 0u64;
        for _ in 0..n {
            if !self.undo_last() {
                break;
            }
            popped += 1;
        }
        popped
    }

    /// 永久清空：必须带确认位（破坏性操作不裸跑）；并入守恒账。
    pub fn empty(&mut self, confirmed: bool) -> usize {
        if !confirmed {
            return 0;
        }
        let n = self.items.len();
        self.emptied += n as u64;
        self.items.clear();
        n
    }

    /// 三路同归审计：三路计数都走同一 items 容器（结构性一致：
    /// 进站总数 = 在站数 + 撤销数 + 还原数 + 清空数——物料守恒）。
    pub fn all_paths_same_sink(&self) -> bool {
        self.trashed_by_path.iter().sum::<u64>()
            == self.items.len() as u64 + self.undo_count + self.restore_count + self.emptied
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
    // v5：重复 id 不虚计（守恒口径——同批 [x,x] 只进一个、账 +1）。
    let mut d = DragTrash::new(HitBox { x: 0, y: 0, w: 10, h: 10 });
    d.trash(DeletePath::DeleteKey, &[7, 7], false);
    set.add(
        "f414-dup-no-inflate",
        d.last_batch_added == 1 && d.items == alloc::vec![7] && d.all_paths_same_sink(),
        "",
    );
    // v5：容量原子性——满后整批拒（部分进站 = 半截状态，禁止）。
    let mut c = DragTrash::new(HitBox { x: 0, y: 0, w: 10, h: 10 });
    c.cap = 3;
    let _ = c.trash(DeletePath::DeleteKey, &[1], false);
    set.add(
        "f414-cap-atomic-reject",
        !c.trash(DeletePath::DeleteKey, &[2, 3, 4], false)
            && c.items == alloc::vec![1]
            && c.rejected_batches == 1
            && c.all_paths_same_sink(),
        "",
    );
    // v5：腾位后同批可进。
    let _ = c.undo_last();
    set.add(
        "f414-cap-freed-retry",
        c.trash(DeletePath::DeleteKey, &[2, 3, 4], false) && c.items.len() == 3,
        "",
    );
    // v5：永久清空——无确认不动手；确认后清空并入守恒账。
    set.add("f414-empty-needs-confirm", c.empty(false) == 0 && c.items.len() == 3, "");
    set.add(
        "f414-empty-confirmed",
        c.empty(true) == 3 && c.items.is_empty() && c.emptied == 3 && c.all_paths_same_sink(),
        "",
    );
    // v5：多步撤销（F202 链连续回退）。
    let mut m = DragTrash::new(HitBox { x: 0, y: 0, w: 10, h: 10 });
    let _ = m.trash(DeletePath::DeleteKey, &[1, 2, 3], false);
    set.add(
        "f414-undo-multi",
        m.undo_last_n(2) == 2 && m.items == alloc::vec![1] && m.undo_count == 2,
        "",
    );
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
        assert_eq!(t.trashed_by_path[1], 1, "账只计实际新增");
        assert!(t.all_paths_same_sink(), "守恒不破");
    }

    #[test]
    fn undo_empty_is_false() {
        let mut t = DragTrash::new(HitBox { x: 0, y: 0, w: 10, h: 10 });
        assert!(!t.undo_last());
        assert_eq!(t.undo_last_n(5), 0, "空站多步撤销也是 0");
        assert_eq!(t.undo_count, 0);
    }

    #[test]
    fn default_cap_is_64() {
        let mut t = DragTrash::new(HitBox { x: 0, y: 0, w: 10, h: 10 });
        let ids: Vec<u64> = (0..64u64).collect();
        assert!(t.trash(DeletePath::DeleteKey, &ids, false));
        assert!(!t.trash(DeletePath::DeleteKey, &[100], false), "第 65 条被拒");
        assert_eq!(t.rejected_batches, 1);
    }
}
