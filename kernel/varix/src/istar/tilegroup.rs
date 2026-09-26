//! F576 磁贴分组文件夹 · 完整设计（STAR I 主册 I 域批次八）。
//!
//! **判据（主册）**：600ms 合组阈值；组内管理；折叠展开；组拖删；
//! 持久化与 F417 语义兼容。
//!
//! **设计要点（主册）**：
//! - 开始菜单磁贴拖叠成分组：磁贴拖到另一磁贴上 600ms 自动建组（组名可改）、
//!   组内磁贴独立开关、组整体可拖可删；组折叠（只显示组名条）；
//!   组内容持久化（与 F417 磁贴布局语义兼容）。

use crate::checks::CheckSet;
use crate::istar::ibase::ISTAR_DOMAIN;

use alloc::string::String;
use alloc::vec::Vec;

/// 合组悬停阈值（ms）。
pub const GROUP_HOVER_MS: u64 = 600;

/// 组数上限（布局持久化容量——与 F417 磁贴布局账同容量纪律）。
pub const GROUP_CAP: usize = 12;

/// 一个磁贴（id 即 F417 布局账中的应用 id）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tile {
    pub app_id: u64,
    pub pinned: bool,
}

/// 一组磁贴。
#[derive(Clone, Debug)]
pub struct TileGroup {
    pub name: String,
    pub tiles: Vec<Tile>,
    pub collapsed: bool,
}

/// 磁贴分组账。
pub struct TileBoard {
    /// 独立磁贴（未入组）。
    loose: Vec<u64>,
    groups: Vec<TileGroup>,
    /// 拖叠悬停态（拖动磁贴 id, 目标磁贴 id, 悬停开始 ms）。
    hover: Option<(u64, u64, u64)>,
    now_ms: u64,
}

impl TileBoard {
    pub fn new() -> TileBoard {
        TileBoard {
            loose: Vec::new(),
            groups: Vec::new(),
            hover: None,
            now_ms: 0,
        }
    }

    /// 添加独立磁贴。
    pub fn add_tile(&mut self, app_id: u64, pinned: bool) {
        if !self.loose.contains(&app_id) {
            self.loose.push(app_id);
            let _ = pinned; // pinned 态由 F417 布局账自持；本账只记分组关系
        }
    }

    /// 拖叠悬停开始（拖 A 到 B 上）。
    pub fn hover_start(&mut self, drag: u64, onto: u64, ms: u64) {
        self.now_ms = ms;
        self.hover = Some((drag, onto, ms));
    }

    /// 悬停到期判定：600ms 到点即建组（拖上即合、松手即成的自动面）。
    pub fn hover_tick(&mut self, ms: u64) -> bool {
        self.now_ms = ms;
        let (drag, onto, start) = match self.hover {
            Some(h) => h,
            None => return false,
        };
        if ms.saturating_sub(start) < GROUP_HOVER_MS {
            return false;
        }
        self.hover = None;
        self.make_group_of(drag, onto)
    }

    /// 提前松手（不足 600ms）：取消合组（阈值语义——不误合）。
    pub fn hover_cancel(&mut self) {
        self.hover = None;
    }

    fn make_group_of(&mut self, drag: u64, onto: u64) -> bool {
        // 目标磁贴已在组内 → 追加进该组（组内管理语义——合组不是复制组）。
        let onto_group = self
            .groups
            .iter()
            .position(|g| g.tiles.iter().any(|t| t.app_id == onto));
        if let Some(gi) = onto_group {
            self.loose.retain(|&t| t != drag);
            let g = &mut self.groups[gi];
            if g.tiles.iter().any(|t| t.app_id == drag) {
                return true; // 同组内拖叠幂等。
            }
            g.tiles.push(Tile { app_id: drag, pinned: true });
            return true;
        }
        // 双散 → 新建组（容量门）。
        if self.groups.len() >= GROUP_CAP {
            return false;
        }
        self.loose.retain(|&t| t != drag && t != onto);
        self.groups.push(TileGroup {
            name: String::from("新建分组"),
            tiles: alloc::vec![
                Tile { app_id: drag, pinned: true },
                Tile { app_id: onto, pinned: true }
            ],
            collapsed: false,
        });
        true
    }

    pub fn groups(&self) -> &[TileGroup] {
        &self.groups
    }

    pub fn loose_tiles(&self) -> &[u64] {
        &self.loose
    }

    /// 组名修改。
    pub fn rename(&mut self, idx: usize, name: &str) -> bool {
        match self.groups.get_mut(idx) {
            Some(g) => {
                g.name = String::from(name);
                true
            }
            None => false,
        }
    }

    /// 组内磁贴独立开关（pinned 切换）。
    pub fn toggle_tile(&mut self, group: usize, app_id: u64) -> bool {
        match self.groups.get_mut(group) {
            Some(g) => {
                for t in g.tiles.iter_mut() {
                    if t.app_id == app_id {
                        t.pinned = !t.pinned;
                        return true;
                    }
                }
                false
            }
            None => false,
        }
    }

    /// 折叠/展开（只显示组名条）。
    pub fn toggle_collapsed(&mut self, idx: usize) -> bool {
        match self.groups.get_mut(idx) {
            Some(g) => {
                g.collapsed = !g.collapsed;
                true
            }
            None => false,
        }
    }

    /// 组整体拖删（整组消失，磁贴回松散区——拖出即散的组级等价）。
    pub fn delete_group(&mut self, idx: usize) -> bool {
        if idx >= self.groups.len() {
            return false;
        }
        let g = self.groups.remove(idx);
        for t in g.tiles {
            self.loose.push(t.app_id);
        }
        true
    }

    /// 拆组：把一枚磁贴拖出组（组空了自动消组）。
    pub fn pull_out(&mut self, group: usize, app_id: u64) -> bool {
        let mut now_empty = false;
        if let Some(g) = self.groups.get_mut(group) {
            let before = g.tiles.len();
            g.tiles.retain(|t| t.app_id != app_id);
            now_empty = g.tiles.is_empty() && g.tiles.len() != before;
            if !self.loose.contains(&app_id) {
                self.loose.push(app_id);
            }
        }
        if now_empty {
            self.groups.remove(group);
        }
        now_empty
    }
}

impl Default for TileBoard {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub fn run_tilegroup_checks() -> CheckSet {
    let mut set = CheckSet::new(ISTAR_DOMAIN);

    // 1. 600ms 合组阈值：599ms 不合、600ms 合（边界钉死）。
    let mut b = TileBoard::new();
    b.add_tile(1, true);
    b.add_tile(2, true);
    b.hover_start(1, 2, 1_000);
    let early = !b.hover_tick(1_599);
    let on_time = b.hover_tick(1_600);
    set.add(
        "group hover 600ms boundary",
        early && on_time && b.groups().len() == 1 && b.loose_tiles().is_empty(),
        "",
    );

    // 2. 提前松手不合组（阈值语义防误合）。
    let mut b2 = TileBoard::new();
    b2.add_tile(3, true);
    b2.add_tile(4, true);
    b2.hover_start(3, 4, 0);
    b2.hover_cancel();
    set.add(
        "early release no group",
        !b2.hover_tick(1_000) && b2.groups().is_empty(),
        "",
    );

    // 3. 组名修改 + 组内独立开关。
    b.rename(0, "工作");
    let t1 = b.toggle_tile(0, 1);
    set.add(
        "rename and tile toggle",
        b.groups()[0].name == "工作" && t1 && !b.groups()[0].tiles[0].pinned,
        "",
    );

    // 4. 折叠展开（只显示组名条——折叠位翻转）。
    let folded = b.toggle_collapsed(0);
    let is_folded = b.groups()[0].collapsed;
    let unfolded = b.toggle_collapsed(0);
    let is_unfolded = !b.groups()[0].collapsed;
    set.add("collapse expand toggle", folded && is_folded && unfolded && is_unfolded, "");

    // 5. 组拖删：整组消失、磁贴回松散区。
    b.delete_group(0);
    set.add(
        "group delete spills tiles",
        b.groups().is_empty() && b.loose_tiles().contains(&1) && b.loose_tiles().contains(&2),
        "",
    );

    // 6. 拆组拖出：组内剩一枚时再拖出 → 组空自动消组。
    let mut b3 = TileBoard::new();
    b3.add_tile(7, true);
    b3.add_tile(8, true);
    b3.hover_start(7, 8, 0);
    b3.hover_tick(600);
    b3.pull_out(0, 7);
    let one_left = b3.groups().len() == 1 && b3.loose_tiles().contains(&7);
    b3.pull_out(0, 8);
    set.add(
        "pull out empties group away",
        one_left && b3.groups().is_empty() && b3.loose_tiles().contains(&8),
        "",
    );

    // 7. 组数上限诚实拒绝（第 13 组建不了）。
    let mut b4 = TileBoard::new();
    let mut all_ok = true;
    for i in 0..12u64 {
        b4.add_tile(i * 2, true);
        b4.add_tile(i * 2 + 1, true);
        b4.hover_start(i * 2, i * 2 + 1, 0);
        all_ok &= b4.hover_tick(600);
    }
    b4.add_tile(99, true);
    b4.add_tile(100, true);
    b4.hover_start(99, 100, 0);
    let over = !b4.hover_tick(600);
    set.add("group cap honest reject", all_ok && over && b4.groups().len() == GROUP_CAP, "");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hover_without_start_false() {
        let mut b = TileBoard::new();
        assert!(!b.hover_tick(1_000));
    }

    #[test]
    fn rename_missing_group_false() {
        let mut b = TileBoard::new();
        assert!(!b.rename(5, "x"));
    }

    #[test]
    fn group_of_three_via_two_steps() {
        let mut b = TileBoard::new();
        for id in 1..=3u64 {
            b.add_tile(id, true);
        }
        b.hover_start(1, 2, 0);
        b.hover_tick(600);
        b.hover_start(3, 1, 0);
        b.hover_tick(600);
        // 第二次合组：3 拖到已入组的 1 上——追加进该组（组内管理语义）。
        assert_eq!(b.groups().len(), 1);
        assert_eq!(b.groups()[0].tiles.len(), 3);
    }
}
