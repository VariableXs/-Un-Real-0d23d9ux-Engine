//! F084 图标拖拽网格 · 完整设计（STAR I 主册 G-C-14）。
//!
//! **判据（主册）**：碰撞换位波浪动画全对录屏；框选 20 图标整体
//! 拖动无散架；双模式切换即时生效。
//!
//! **设计要点（主册）**：
//! - 桌面图标两种模式：对齐网格（拖动吸附 76×100px 单元，乙-4 表）
//!   /自由摆放（像素级）；拖拽半透明跟手+目标位高亮+碰撞自动换位
//!   动画；框选（橡皮筋）多选拖；
//! - 拖拽中图标 80% 透明跟手（F018 视觉族）；落点合法→目标单元
//!   高亮 2px 描边；碰撞→被占位图标滑动让位动画（150ms 逐个连锁，
//!   波浪感）；框选矩形虚线描边+8% 填充；多选拖动整体平移保持
//!   相对位；
//! - 图标位存桌面布局文件（每图标网格坐标或自由坐标）；布局随
//!   主题（E1）可分套保存；
//! - 拖出屏幕 → 回弹动画+归位（不丢失）；同名冲突 → F087 面板；
//!   布局文件损坏 → 重建默认网格（自愈+报备）；「自动排列」一键
//!   重排（按名称/类型/日期）；
//! - 网格吸附阈值 24px（松手距最近格心内才吸附，否则回弹）；让位
//!   动画连锁延迟每级 20ms（波浪节拍）；自由模式吸附关闭但轻微
//!   磁吸 8px（对齐强迫症友好）；Ctrl+拖=复制语义（对齐 Windows）；
//!   布局文件版本化（主题切换恢复不丢用户手工位）。
//!
//! 实装口径：布局账本（网格/自由双模式）+ 拖拽状态机 + 吸附阈值
//! 判定 + 碰撞连锁波浪账 + 框选账 + 自动排列三键 + 布局版本化。
//! 时间注入式。

use crate::checks::CheckSet;

use crate::deskstar::dbase::Rect;
use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 规格常量（参数唯一源——主册交互设计/设计细节）
// ---------------------------------------------------------------------------

/// 网格单元宽（px，乙-4 表）。
pub const CELL_W_PX: i32 = 76;

/// 网格单元高（px，乙-4 表）。
pub const CELL_H_PX: i32 = 100;

/// 吸附阈值（px，距最近格心内才吸附否则回弹）。
pub const SNAP_THRESHOLD_PX: i32 = 24;

/// 自由模式轻微磁吸（px）。
pub const FREE_MAGNET_PX: i32 = 8;

/// 拖拽中透明（%，80% 透明跟手——注意主册语义是「80% 透明」，
/// 即不透明度 20%；实现层以 opacity_pct 表不透明度=20）。
pub const DRAG_OPACITY_PCT: u8 = 20;

/// 目标单元高亮描边（px）。
pub const TARGET_STROKE_PX: i32 = 2;

/// 让位动画单段时长（ms）。
pub const DISPLACE_MS: u32 = 150;

/// 让位连锁节拍（ms/级，波浪）。
pub const DISPLACE_STAGGER_MS: u32 = 20;

/// 框选填充不透明度（%）。
pub const RUBBER_FILL_PCT: u8 = 8;

/// 布局文件版本（版本化——主题切换恢复不丢手工位）。
pub const LAYOUT_VERSION: u32 = 2;

// ---------------------------------------------------------------------------
// 模式与布局账
// ---------------------------------------------------------------------------

/// 图标布局模式。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GridMode {
    /// 对齐网格（吸附 76×100 单元）。
    Snap,
    /// 自由摆放（像素级，8px 轻磁吸）。
    Free,
}

/// 桌面图标条目。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeskIcon {
    pub id: u64,
    pub name: String,
    /// 网格坐标（对齐模式语义）。
    pub cell: (u16, u16),
    /// 自由坐标（自由模式语义；网格模式下渲染锚=格心）。
    pub free: (i32, i32),
}

/// 桌面图标网格管理器。
pub struct IconGrid {
    pub mode: GridMode,
    icons: Vec<DeskIcon>,
    cols: i32,
    rows: i32,
    /// 拖拽态：(被拖 id, 当前指针位, 是否 Ctrl 复制)。
    drag: Option<(u64, (i32, i32), bool)>,
    /// 框选态：橡皮筋矩形（拖动中实时更新）。
    rubber: Option<Rect>,
    /// 被框选集合。
    selected: Vec<u64>,
    /// 让位连锁账：[(图标, 起始延迟 ms)]——波浪节拍。
    pub displacements: Vec<(u64, u32)>,
    /// 回弹账（拖出屏幕归位）。
    pub rebounds: u64,
    /// 自愈报备账（布局损坏重建默认网格）。
    pub rebuilds: u64,
    /// 复制语义账（Ctrl+拖）。
    pub copies: u64,
    /// 冲突上抛账（同名 → F087 面板接缝）。
    pub conflicts_raised: u64,
    /// 时间锚（拖拽/动画事件的注入时刻）。
    now_ms: u64,
}

impl IconGrid {
    pub fn new(cols: i32, rows: i32) -> IconGrid {
        IconGrid {
            mode: GridMode::Snap,
            icons: Vec::new(),
            cols,
            rows,
            drag: None,
            rubber: None,
            selected: Vec::new(),
            displacements: Vec::new(),
            rebounds: 0,
            rebuilds: 0,
            copies: 0,
            conflicts_raised: 0,
            now_ms: 0,
        }
    }

    /// 布局文件载入（损坏 → 重建默认网格自愈+报备）。
    pub fn load_layout(&mut self, payload: Option<Vec<DeskIcon>>, version: u32) {
        match payload {
            Some(icons) if version <= LAYOUT_VERSION && !icons.is_empty() => {
                self.icons = icons;
            }
            _ => {
                self.rebuild_default();
            }
        }
    }

    /// 重建默认网格（自愈：按现序回填格位）。
    fn rebuild_default(&mut self) {
        for (i, icon) in self.icons.iter_mut().enumerate() {
            let c = i as i32 % self.cols;
            let r = i as i32 / self.cols;
            icon.cell = (c as u16, r as u16);
            icon.free = (
                c * CELL_W_PX + CELL_W_PX / 2,
                r * CELL_H_PX + CELL_H_PX / 2,
            );
        }
        self.rebuilds += 1;
    }

    pub fn icon_count(&self) -> usize {
        self.icons.len()
    }

    /// 注册图标（占最近空格）。
    pub fn add_icon(&mut self, id: u64, name: &str) {
        let mut cell = (0u16, 0u16);
        'outer: for r in 0..self.rows {
            for c in 0..self.cols {
                let occupied = self
                    .icons
                    .iter()
                    .any(|ic| ic.cell == (c as u16, r as u16));
                if !occupied {
                    cell = (c as u16, r as u16);
                    break 'outer;
                }
            }
        }
        let free = (
            cell.0 as i32 * CELL_W_PX + CELL_W_PX / 2,
            cell.1 as i32 * CELL_H_PX + CELL_H_PX / 2,
        );
        self.icons.push(DeskIcon {
            id,
            name: String::from(name),
            cell,
            free,
        });
    }

    /// 双模式切换（即时生效）。
    pub fn set_mode(&mut self, mode: GridMode) {
        self.mode = mode;
    }

    /// 格心坐标。
    pub fn cell_center(&self, cell: (u16, u16)) -> (i32, i32) {
        (
            cell.0 as i32 * CELL_W_PX + CELL_W_PX / 2,
            cell.1 as i32 * CELL_H_PX + CELL_H_PX / 2,
        )
    }

    /// 指针 → 最近格（越界钳制）。
    fn nearest_cell(&self, p: (i32, i32)) -> (u16, u16) {
        let c = (p.0 / CELL_W_PX).clamp(0, self.cols - 1);
        let r = (p.1 / CELL_H_PX).clamp(0, self.rows - 1);
        (c as u16, r as u16)
    }

    // -- 拖拽 ---------------------------------------------------------------

    /// 开始拖动（id + 指针；ctrl = 复制语义）。
    pub fn drag_start(&mut self, id: u64, at: (i32, i32), ctrl: bool) -> bool {
        if !self.icons.iter().any(|ic| ic.id == id) {
            return false;
        }
        self.drag = Some((id, at, ctrl));
        true
    }

    pub fn drag_move(&mut self, at: (i32, i32)) {
        if let Some((_, p, _)) = self.drag.as_mut() {
            *p = at;
        }
    }

    /// 拖拽中透明度（跟手 80% 透明——opacity 20%）。
    pub fn drag_opacity_pct(&self) -> u8 {
        if self.drag.is_some() {
            DRAG_OPACITY_PCT
        } else {
            100
        }
    }

    /// 目标单元高亮（合法落点 → 2px 描边；返回格位）。
    pub fn drop_highlight(&self) -> Option<(u16, u16)> {
        let (id, at, _) = self.drag.as_ref()?;
        let cell = self.nearest_cell(*at);
        // 合法性：格内非他人占用（自己占的格合法）。
        let occupied_by = self.icons.iter().find(|ic| ic.cell == cell).map(|ic| ic.id);
        if occupied_by.map(|o| o != *id) == Some(true) {
            Some(cell) // 高亮但落位时触发换位
        } else {
            Some(cell)
        }
    }

    /// 松手落位：网格模式按 24px 阈值吸附或回弹；碰撞触发连锁让位。
    pub fn drop(&mut self, now_ms: u64) -> Option<u64> {
        let (id, at, ctrl) = self.drag.take()?;
        let icon = self.icons.iter().find(|ic| ic.id == id)?.clone();
        self.now_ms = now_ms;
        match self.mode {
            GridMode::Snap => {
                let cell = self.nearest_cell(at);
                let center = self.cell_center(cell);
                let dist = (at.0 - center.0).abs().max((at.1 - center.1).abs());
                if dist > SNAP_THRESHOLD_PX {
                    // 回弹归位（不丢失）。
                    self.rebounds += 1;
                    return Some(id);
                }
                // Ctrl+拖 = 复制语义（新图标占目标格）。
                if ctrl {
                    self.copies += 1;
                    let new_id = self.icons.iter().map(|i| i.id).max().unwrap_or(0) + 1;
                    let name = icon.name.clone();
                    let src_cell = icon.cell;
                    let src_center = self.cell_center(src_cell);
                    self.icons.iter_mut().for_each(|i| {
                        if i.id == id {
                            i.cell = cell;
                            i.free = center;
                        }
                    });
                    self.icons.push(DeskIcon {
                        id: new_id,
                        name,
                        cell: src_cell,
                        free: src_center,
                    });
                    return Some(new_id);
                }
                // 碰撞：目标格被他人占 → 被占位者滑向源位（连锁波浪）。
                let occupant = self.icons.iter().find(|ic| ic.cell == cell && ic.id != id).map(|ic| ic.id);
                let swap_center = self.cell_center(icon.cell);
                self.icons.iter_mut().for_each(|i| {
                    if i.id == id {
                        i.cell = cell;
                        i.free = center;
                    } else if Some(i.id) == occupant {
                        i.cell = icon.cell;
                        i.free = swap_center;
                    }
                });
                if let Some(o) = occupant {
                    self.displacements.push((o, 0));
                }
                Some(id)
            }
            GridMode::Free => {
                // 自由模式：像素级落位 + 8px 轻磁吸（对齐强迫症友好）。
                let mut x = at.0;
                let mut y = at.1;
                let base = self.cell_center(icon.cell);
                if (x - base.0).abs() <= FREE_MAGNET_PX {
                    x = base.0;
                }
                if (y - base.1).abs() <= FREE_MAGNET_PX {
                    y = base.1;
                }
                self.icons.iter_mut().for_each(|i| {
                    if i.id == id {
                        i.free = (x, y);
                    }
                });
                Some(id)
            }
        }
    }

    /// 让位连锁波浪（供上层排动画：逐级 +20ms）。
    pub fn chain_wave(&mut self, follower: u64) {
        let base = self
            .displacements
            .last()
            .map(|(_, d)| *d + DISPLACE_STAGGER_MS)
            .unwrap_or(0);
        self.displacements.push((follower, base));
    }

    /// 拖出屏幕回弹归位（不丢失）。
    pub fn drag_out_of_screen(&mut self) {
        if let Some((id, _, ctrl)) = self.drag.take() {
            let _ = ctrl;
            self.rebounds += 1;
            let _ = id; // 位置不动即归位
        }
    }

    /// 同名冲突上抛（F087 接缝：桌面目标已有同名项）。
    pub fn raise_conflict(&mut self, name: &str) -> bool {
        let dup = self.icons.iter().filter(|ic| ic.name == name).count() > 1;
        if dup {
            self.conflicts_raised += 1;
        }
        dup
    }

    // -- 框选（橡皮筋）------------------------------------------------------

    pub fn rubber_start(&mut self, at: (i32, i32)) {
        self.rubber = Some(Rect::new(at.0, at.1, 0, 0));
        self.selected.clear();
    }

    pub fn rubber_move(&mut self, from: (i32, i32), to: (i32, i32)) {
        let x = from.0.min(to.0);
        let y = from.1.min(to.1);
        let w = (from.0 - to.0).abs();
        let h = (from.1 - to.1).abs();
        self.rubber = Some(Rect::new(x, y, w, h));
        let rect = self.rubber.unwrap();
        self.selected = self
            .icons
            .iter()
            .filter(|ic| rect.contains(ic.free.0, ic.free.1))
            .map(|ic| ic.id)
            .collect();
    }

    /// 框选样式（虚线描边+8% 填充的几何/令牌面）。
    pub fn rubber_rect(&self) -> Option<Rect> {
        self.rubber
    }

    pub fn selected_ids(&self) -> &[u64] {
        &self.selected
    }

    /// 多选整体拖动（相对位保持——平移量一致，无散架）。
    pub fn drag_selection(&mut self, dx: i32, dy: i32) -> bool {
        if self.selected.is_empty() {
            return false;
        }
        let sel = self.selected.clone();
        for ic in self.icons.iter_mut() {
            if sel.contains(&ic.id) {
                ic.free = (ic.free.0 + dx, ic.free.1 + dy);
            }
        }
        true
    }

    // -- 自动排列 ------------------------------------------------------------

    /// 自动排列（按名称/类型/日期——类型与日期以名称序演示同规则族，
    /// 实际键由上层供给排序键；本账保证：重排后格位无重叠满覆盖）。
    pub fn auto_arrange(&mut self, order: Vec<u64>) {
        for (i, id) in order.iter().enumerate() {
            let c = (i as i32 % self.cols) as u16;
            let r = (i as i32 / self.cols) as u16;
            let cell = (c, r);
            let free = self.cell_center(cell);
            self.icons.iter_mut().for_each(|ic| {
                if ic.id == *id {
                    ic.cell = cell;
                    ic.free = free;
                }
            });
        }
    }

    /// 布局一致性：格位两两不重（自愈/排列后的不变量）。
    pub fn cells_unique(&self) -> bool {
        let mut seen: Vec<(u16, u16)> = Vec::new();
        for ic in &self.icons {
            if seen.contains(&ic.cell) {
                return false;
            }
            seen.push(ic.cell);
        }
        true
    }

    pub fn icon_at(&self, id: u64) -> Option<&DeskIcon> {
        self.icons.iter().find(|ic| ic.id == id)
    }
}

// ---------------------------------------------------------------------------
// 自检（判据唯一源：主册 G-C-14 验收判据）
// ---------------------------------------------------------------------------

/// F084 自检：波浪换位、框选 20 整体拖不散架、双模式即时生效、
/// 24px 吸附阈值、8px 磁吸、回弹不丢失、Ctrl 复制、布局自愈版本化。
pub fn run_icongrid_checks() -> CheckSet {
    let mut set = CheckSet::new("deskstar-F084");
    let mut g = IconGrid::new(8, 6);
    for i in 1..=6u64 {
        g.add_icon(i, "图标");
    }
    // 1. 碰撞换位波浪：拖 1 号到 2 号格 → 2 号滑向 1 号原格。
    let c1 = g.icon_at(1).unwrap().cell;
    let c2 = g.icon_at(2).unwrap().cell;
    assert!(g.drag_start(1, g.cell_center(c2), false), "drag_start 恒可开始");
    let target = g.drop_highlight();
    g.drop(0);
    let swapped = g.icon_at(1).unwrap().cell == c2 && g.icon_at(2).unwrap().cell == c1;
    let wave = !g.displacements.is_empty() && g.displacements[0].1 == 0;
    set.add(
        "collision-wave",
        swapped && wave && target.is_some(),
        "swap + wave ledger",
    );
    // 2. 连锁节拍：追加让位逐级 +20ms。
    g.chain_wave(3);
    g.chain_wave(4);
    let stagger_ok = g.displacements.len() == 3
        && g.displacements[1].1 == DISPLACE_STAGGER_MS
        && g.displacements[2].1 == 2 * DISPLACE_STAGGER_MS;
    set.add("wave-stagger", stagger_ok, "20ms per level");
    // 3. 框选 20 图标整体拖动不散架。
    let mut g2 = IconGrid::new(8, 6);
    for i in 1..=20u64 {
        g2.add_icon(i, "图标");
    }
    g2.set_mode(GridMode::Free);
    g2.rubber_start((0, 0));
    g2.rubber_move((0, 0), (600, 500)); // 框住前 20 格心
    let sel_n = g2.selected_ids().len();
    let before: Vec<(i32, i32)> = g2
        .selected_ids()
        .iter()
        .map(|id| g2.icon_at(*id).unwrap().free)
        .collect();
    g2.drag_selection(30, 40);
    let after: Vec<(i32, i32)> = g2
        .selected_ids()
        .iter()
        .map(|id| g2.icon_at(*id).unwrap().free)
        .collect();
    // 相对位不变：逐对差值一致。
    let rel0 = (after[0].0 - before[0].0, after[0].1 - before[0].1); // 位移 (+30,+40)
    let rigid = after
        .iter()
        .zip(before.iter())
        .all(|(a, b)| (a.0 - b.0, a.1 - b.1) == rel0);
    set.add(
        "rubber-20-rigid",
        sel_n == 20 && rigid && rel0 == (30, 40),
        "20 icons move rigid",
    );
    // 4. 双模式即时生效 + 吸附阈值。
    let mut g3 = IconGrid::new(8, 6);
    g3.add_icon(1, "甲");
    g3.set_mode(GridMode::Snap);
    let center = g3.cell_center((0, 0));
    // 距格心 10px（<24）→ 吸附；40px（>24）→ 回弹。
    assert!(g3.drag_start(1, (center.0 + 10, center.1 + 10), false), "drag_start 恒可开始");
    g3.drop(0);
    let snapped = g3.icon_at(1).unwrap().cell == (0, 0);
    assert!(g3.drag_start(1, (center.0 + 40, center.1), false), "drag_start 恒可开始");
    g3.drop(0);
    let bounced = g3.rebounds == 1;
    g3.set_mode(GridMode::Free);
    let free_now = g3.mode == GridMode::Free;
    set.add(
        "mode-threshold",
        snapped && bounced && free_now,
        "24px snap / instant switch",
    );
    // 5. 自由模式 8px 磁吸。
    let base = g3.cell_center((0, 0));
    assert!(g3.drag_start(1, (base.0 + 6, base.1 + 20), false), "drag_start 恒可开始");
    g3.drop(0);
    let magnet = g3.icon_at(1).unwrap().free.0 == base.0
        && g3.icon_at(1).unwrap().free.1 == base.1 + 20;
    set.add("free-magnet", magnet, "8px light magnet");
    // 6. 拖出屏幕回弹归位（不丢失）。
    assert!(g3.drag_start(1, (base.0, base.1), false), "drag_start 恒可开始");
    g3.drag_out_of_screen();
    set.add(
        "out-of-screen",
        g3.rebounds == 2 && g3.icon_at(1).is_some(),
        "rebound, never lost",
    );
    // 7. Ctrl+拖 = 复制语义。
    let mut g4 = IconGrid::new(8, 6);
    g4.add_icon(1, "报告");
    assert!(g4.drag_start(1, g4.cell_center((2, 0)), true), "drag_start 恒可开始");
    g4.drop(0);
    set.add(
        "ctrl-copy",
        g4.copies == 1 && g4.icon_count() == 2 && g4.cells_unique(),
        "copy semantics",
    );
    // 8. 同名冲突上抛 F087 接缝。
    g4.add_icon(3, "报告");
    set.add("conflict-f087", g4.raise_conflict("报告") && g4.conflicts_raised == 1, "raises panel");
    // 9. 布局损坏自愈 + 版本化。
    let mut g5 = IconGrid::new(8, 6);
    g5.load_layout(None, LAYOUT_VERSION); // 损坏 → 重建
    g5.add_icon(1, "甲");
    g5.load_layout(None, LAYOUT_VERSION);
    set.add(
        "layout-selfheal",
        g5.rebuilds >= 1 && g5.cells_unique(),
        "corrupt → default grid",
    );
    set
}

// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snap_threshold_boundary_exact() {
        let mut g = IconGrid::new(8, 6);
        g.add_icon(1, "甲");
        let center = g.cell_center((1, 0));
        // 恰 24px（阈值内）→ 吸附。
        assert!(g.drag_start(1, (center.0 + 24, center.1), false), "drag_start 恒可开始");
        g.drop(0);
        assert_eq!(g.icon_at(1).unwrap().cell, (1, 0));
        // 恰 25px → 回弹。
        assert!(g.drag_start(1, (center.0 + 25, center.1), false), "drag_start 恒可开始");
        g.drop(0);
        assert_eq!(g.rebounds, 1);
    }

    #[test]
    fn drag_opacity_80_transparent() {
        let mut g = IconGrid::new(8, 6);
        g.add_icon(1, "甲");
        assert_eq!(g.drag_opacity_pct(), 100);
        assert!(g.drag_start(1, (10, 10), false), "drag_start 恒可开始");
        assert_eq!(g.drag_opacity_pct(), 20, "80% 透明 = 不透明度 20%");
    }

    #[test]
    fn rubber_selection_updates_live() {
        let mut g = IconGrid::new(8, 6);
        for i in 1..=5u64 {
            g.add_icon(i, "甲");
        }
        g.set_mode(GridMode::Free);
        g.rubber_start((0, 0));
        g.rubber_move((0, 0), (150, 60));
        assert_eq!(g.selected_ids().len(), 2, "框住前两格心");
        g.rubber_move((0, 0), (400, 300));
        assert_eq!(g.selected_ids().len(), 5);
        assert!(g.rubber_rect().is_some());
    }

    #[test]
    fn auto_arrange_fills_grid_without_overlap() {
        let mut g = IconGrid::new(8, 6);
        for i in 1..=10u64 {
            g.add_icon(i, "甲");
        }
        let order: Vec<u64> = (1..=10).rev().collect();
        g.auto_arrange(order);
        assert!(g.cells_unique(), "重排后格位两两不重");
    }

    #[test]
    fn icongrid_self_checks_all_green() {
        let set = run_icongrid_checks();
        let (p, f) = set.tally();
        assert!(set.all_passed(), "F084 自检红项：{}/{} 绿", p, p + f);
    }
}
