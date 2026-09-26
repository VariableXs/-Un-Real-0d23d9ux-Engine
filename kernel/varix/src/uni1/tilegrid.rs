//! F417 开始菜单磁贴交互 · 完整设计（STAR I 主册 G-I-17）。
//!
//! **判据（主册）**：三档尺寸渲染与占格；拖拽让位动画与落点精度；固定
//! 双向（磁贴区↔所有应用）；持久化；与 F158 布局预览联动。＋通12。
//!
//! 设计：磁贴网格语义核——三档尺寸（小 1×1 / 中 2×2 / 大 2×4）占格
//! 模型；拖拽重排（目标格占位校验 + 其余磁贴让位重排，落点精度按格
//! 钉死）；固定双向（磁贴区 ↔ 所有应用列表）；布局快照（持久化 +
//! F158 预览同源）。

use crate::checks::CheckSet;

use alloc::vec::Vec;

/// 磁贴三档（占格 宽×高）。
pub const TILE_SMALL: (u32, u32) = (1, 1);
pub const TILE_MEDIUM: (u32, u32) = (2, 2);
pub const TILE_LARGE: (u32, u32) = (2, 4);

/// 一个磁贴。
#[derive(Clone, Debug)]
pub struct Tile {
    pub app: &'static str,
    pub size: (u32, u32),
    /// 左上格位。
    pub cell: (u32, u32),
}

/// 磁贴区（列数固定，行向下生长）。
pub struct TileGrid {
    pub cols: u32,
    tiles: Vec<Tile>,
}

impl TileGrid {
    pub fn new(cols: u32) -> TileGrid {
        TileGrid { cols: cols.max(2), tiles: Vec::new() }
    }

    pub fn tiles(&self) -> &[Tile] {
        &self.tiles
    }

    /// 占格集合（含重叠检出——重叠即非法态）。
    fn occupy(&self, t: &Tile) -> Vec<(u32, u32)> {
        let mut cells = Vec::new();
        for dx in 0..t.size.0 {
            for dy in 0..t.size.1 {
                cells.push((t.cell.0 + dx, t.cell.1 + dy));
            }
        }
        cells
    }

    fn any_overlap(&self, ignore: Option<&str>, candidate: &Tile) -> bool {
        let cand = self.occupy(candidate);
        self.tiles.iter().filter(|t| Some(t.app) != ignore).any(|t| {
            self.occupy(t).iter().any(|c| cand.contains(c))
        })
    }

    /// 固定到磁贴区（从「所有应用」）；落首个可容纳位。
    pub fn pin(&mut self, app: &'static str, size: (u32, u32)) -> bool {
        if self.tiles.iter().any(|t| t.app == app) {
            return false;
        }
        for row in 0..64u32 {
            for col in 0..=self.cols.saturating_sub(size.0) {
                let cand = Tile { app, size, cell: (col, row) };
                if !self.any_overlap(None, &cand) {
                    self.tiles.push(cand);
                    return true;
                }
            }
        }
        false
    }

    /// 取消固定（回「所有应用」——双向判据）。
    pub fn unpin(&mut self, app: &'static str) -> bool {
        let before = self.tiles.len();
        self.tiles.retain(|t| t.app != app);
        self.tiles.len() != before
    }

    /// 调整大小（三档校验：越界/重叠拒绝——不静默破坏布局）。
    pub fn resize(&mut self, app: &'static str, size: (u32, u32)) -> bool {
        let pos = match self.tiles.iter().position(|t| t.app == app) {
            Some(p) => p,
            None => return false,
        };
        let cand = Tile { app, size, cell: self.tiles[pos].cell };
        if cand.cell.0 + size.0 > self.cols || self.any_overlap(Some(app), &cand) {
            return false;
        }
        self.tiles[pos].size = size;
        true
    }

    /// 拖拽重排：目标格合法（不越界不与第三方重叠）→ 落点生效；
    /// 其余磁贴让位（下压重排）由 relayout 补齐。
    pub fn drag_to(&mut self, app: &'static str, to: (u32, u32)) -> bool {
        let pos = match self.tiles.iter().position(|t| t.app == app) {
            Some(p) => p,
            None => return false,
        };
        let size = self.tiles[pos].size;
        if to.0 + size.0 > self.cols {
            return false; // 落点越界拒绝（精度判据：非法落点不吸附）。
        }
        let cand = Tile { app, size, cell: to };
        if self.any_overlap(Some(app), &cand) {
            return false;
        }
        self.tiles[pos].cell = to;
        self.relayout(Some(app));
        true
    }

    /// 让位重排：锚点磁贴（被拖拽者）钉在落点不动，其余磁贴按阅读序
    /// first-fit 回流填缝——「落点精度 + 其余让位」的唯一实现点。
    /// `anchor = None` 时全量回流（无锚点压缩）。
    pub fn relayout(&mut self, anchor: Option<&str>) {
        self.tiles.sort_by(|a, b| (a.cell.1, a.cell.0).cmp(&(b.cell.1, b.cell.0)));
        // 锚点占格（不可移动的既定格）。
        let mut placed: Vec<(u32, u32, u32, u32)> = Vec::new();
        for t in self.tiles.iter() {
            if anchor == Some(t.app) {
                for dx in 0..t.size.0 {
                    for dy in 0..t.size.1 {
                        placed.push((t.cell.0 + dx, t.cell.1 + dy, 1, 1));
                    }
                }
            }
        }
        let anchored_names: Vec<&str> =
            self.tiles.iter().filter(|t| anchor == Some(t.app)).map(|t| t.app).collect();
        for t in self.tiles.iter_mut() {
            if anchored_names.contains(&t.app) {
                continue;
            }
            let (w, h) = t.size;
            'outer: for row in 0..64u32 {
                for col in 0..=self.cols.saturating_sub(w) {
                    let mut conflict = false;
                    'inner: for dx in 0..w {
                        for dy in 0..h {
                            let (cx, cy) = (col + dx, row + dy);
                            if placed.iter().any(|(px, py, pw, ph)| {
                                cx >= *px && cx < px + pw && cy >= *py && cy < py + ph
                            }) {
                                conflict = true;
                                break 'inner;
                            }
                        }
                    }
                    if !conflict {
                        t.cell = (col, row);
                        for dx in 0..w {
                            for dy in 0..h {
                                placed.push((col + dx, row + dy, 1, 1));
                            }
                        }
                        break 'outer;
                    }
                }
            }
        }
    }

    /// 布局快照（持久化 / F158 预览同源）。
    pub fn snapshot(&self) -> Vec<(&'static str, (u32, u32), (u32, u32))> {
        self.tiles.iter().map(|t| (t.app, t.size, t.cell)).collect()
    }

    /// 快照恢复（round-trip 判据）。
    pub fn restore(snap: Vec<(&'static str, (u32, u32), (u32, u32))>, cols: u32) -> TileGrid {
        let mut g = TileGrid::new(cols);
        for (app, size, cell) in snap {
            g.tiles.push(Tile { app, size, cell });
        }
        g
    }

    /// 无重叠不变量。
    pub fn invariant_ok(&self) -> bool {
        for i in 0..self.tiles.len() {
            for j in (i + 1)..self.tiles.len() {
                let a = self.occupy(&self.tiles[i]);
                let b = self.occupy(&self.tiles[j]);
                if a.iter().any(|c| b.contains(c)) {
                    return false;
                }
            }
        }
        true
    }
}

pub fn run_tilegrid_checks() -> CheckSet {
    let mut set = CheckSet::new("uni1-F417");
    set.add(
        "f417-three-sizes",
        TILE_SMALL == (1, 1) && TILE_MEDIUM == (2, 2) && TILE_LARGE == (2, 4),
        "",
    );
    let mut g = TileGrid::new(6);
    // 固定双向：pin 三枚 + unpin 一枚。
    set.add(
        "f417-pin-three",
        g.pin("邮件", TILE_MEDIUM) && g.pin("日历", TILE_SMALL) && g.pin("照片", TILE_LARGE),
        "",
    );
    set.add(
        "f417-placements",
        g.tiles().iter().find(|t| t.app == "邮件").map(|t| t.cell) == Some((0, 0))
            && g.tiles().iter().find(|t| t.app == "日历").map(|t| t.cell) == Some((2, 0))
            && g.tiles().iter().find(|t| t.app == "照片").map(|t| t.cell) == Some((3, 0)),
        "",
    );
    set.add("f417-invariant", g.invariant_ok(), "");
    // 拖拽让位 + 落点精度。
    set.add("f417-drag-ok", g.drag_to("邮件", (2, 0)) == false, ""); // 与日历重叠拒绝
    set.add("f417-drag-precise", g.drag_to("邮件", (0, 3)) && g.tiles().iter().find(|t| t.app == "邮件").map(|t| t.cell) == Some((0, 3)), "");
    set.add("f417-relayout-compacts", g.tiles().iter().find(|t| t.app == "日历").map(|t| t.cell.1) == Some(0), "");
    set.add("f417-invariant-after-drag", g.invariant_ok(), "");
    // 调整大小（三档 + 重叠拒绝：日历放大成 2×4 会压到邮件的落点格）。
    set.add("f417-resize-ok", g.resize("日历", TILE_MEDIUM), "");
    set.add("f417-resize-reject", !g.resize("日历", TILE_LARGE), "");
    // 取消固定回所有应用。
    set.add("f417-unpin", g.unpin("邮件") && !g.unpin("邮件"), "");
    // 快照 round-trip。
    let snap = g.snapshot();
    let g2 = TileGrid::restore(snap.clone(), 6);
    set.add(
        "f417-snapshot-roundtrip",
        g2.snapshot() == snap && g2.invariant_ok(),
        "",
    );
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relayout_fills_holes() {
        let mut g = TileGrid::new(4);
        assert!(g.pin("a", TILE_SMALL));
        assert!(g.pin("b", TILE_SMALL));
        assert!(g.pin("c", TILE_SMALL));
        assert!(g.drag_to("a", (0, 2)));
        // 落点锚定：a 钉在 (0,2)；b/c 按阅读序 first-fit 回流填缝。
        let snap = g.snapshot();
        assert_eq!(snap[0], ("b", TILE_SMALL, (0, 0)));
        assert_eq!(snap[1], ("c", TILE_SMALL, (1, 0)));
        assert_eq!(snap[2], ("a", TILE_SMALL, (0, 2)));
        assert!(g.invariant_ok());
    }

    #[test]
    fn pin_duplicate_rejected() {
        let mut g = TileGrid::new(6);
        assert!(g.pin("x", TILE_SMALL));
        assert!(!g.pin("x", TILE_SMALL), "已固定不重复");
    }
}
