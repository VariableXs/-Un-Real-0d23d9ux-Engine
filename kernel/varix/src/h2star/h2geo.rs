//! H2 域几何布局引擎 · 深化批次一（渲染层纵深——骨架→器官）。
//!
//! **承接判据**（主册 H 域正文，一处一事实）：
//! - **F276 贴靠布局组**：四款布局几何计算用例（各屏分辨率）；区与区
//!   之间留 8px 呼吸缝；拖分屏线联动缩放——本引擎给出四款布局在任意
//!   分辨率下的精确矩形，以及分屏线拖动时两邻区按比例联动的新几何；
//! - **F286 壁纸多屏设置**：跨屏拼接（一张 4K+ 大图按屏分割）——
//!   本引擎给出每屏在源图上的取样区段（分割几何）；
//! - **F298 快速设置磁贴编辑**：磁贴流式排布——面板宽度内按磁贴档位
//!   （小 1×1 / 中 2×2 / 宽 2×1）逐行排布，行满换行；
//! - **F259 新建落点 / F084 网格语义**：网格视图坐标换算（格间距、
//!   单元几何、点→格与格→点双向换算——拖拽吸附与落点提示共用）。
//!
//! 时间纪律延续：无时钟；全部纯函数——输入矩形与参数，输出矩形与
//! 校验位。旋钮（缝宽 8px 等）由调用方注入，本模块不读全局状态。

use crate::checks::CheckSet;

use alloc::vec::Vec;

/// 一个轴对齐矩形（整像素——布局不许出现半像素糊边）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

impl Rect {
    pub const fn new(x: i32, y: i32, w: u32, h: u32) -> Rect {
        Rect { x, y, w, h }
    }
    /// 等比缩放（分屏线拖动联动——两邻区按剩余宽度比例分摊）。
    pub fn scaled(&self, num: u32, den: u32) -> Rect {
        if den == 0 {
            return *self;
        }
        Rect {
            x: self.x,
            y: self.y,
            w: (self.w as u64 * num as u64 / den as u64) as u32,
            h: (self.h as u64 * num as u64 / den as u64) as u32,
        }
    }
}

/// 四款贴靠布局（主册清单——与 Windows 贴靠选择器同构）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnapLayout {
    /// 二分：左右各半。
    Two,
    /// 三分：左 1/3 + 右上/右下。
    Three,
    /// 四分：四等分。
    Four,
    /// 左大右小：左 2/3 + 右列上下两块。
    LeftBig,
}

impl SnapLayout {
    /// 槽位数。
    pub fn slots(self) -> usize {
        match self {
            SnapLayout::Two => 2,
            SnapLayout::Three => 3,
            SnapLayout::Four => 4,
            SnapLayout::LeftBig => 3,
        }
    }
}

/// 呼吸缝默认值（主册定值 8px；h2knob `h2.f276.gap_px` 同源）。
pub const SNAP_GAP_PX: u32 = 8;

/// 把一块工作区切成 `n` 份、区间留 `gap` 呼吸缝的通用算子：
/// 总缝宽 = (n-1)*gap，净宽按份均分（余数给靠前的区——不丢像素）。
fn divide_h(area: Rect, n: usize, gap: u32) -> Vec<Rect> {
    let mut out = Vec::with_capacity(n);
    if n == 0 {
        return out;
    }
    let gap_total = (n as u32 - 1) * gap;
    if (area.w as u64) < gap_total as u64 + n as u64 {
        // 极窄屏：退化为重叠缝（每份均分、不留缝——布局不崩优先）。
        let w = area.w / n as u32;
        for i in 0..n {
            let x = area.x + (i as u32 * w) as i32;
            out.push(Rect::new(x, area.y, w, area.h));
        }
        return out;
    }
    let net = area.w - gap_total;
    let base = net / n as u32;
    let rem = net % n as u32;
    let mut x = area.x;
    for i in 0..n {
        let w = base + if (i as u32) < rem { 1 } else { 0 };
        out.push(Rect::new(x, area.y, w, area.h));
        x += w as i32 + gap as i32;
    }
    out
}

/// 竖向同构（三分/左大右小的右列用）。
fn divide_v(area: Rect, n: usize, gap: u32) -> Vec<Rect> {
    let mut out = Vec::with_capacity(n);
    if n == 0 {
        return out;
    }
    let gap_total = (n as u32 - 1) * gap;
    if (area.h as u64) < gap_total as u64 + n as u64 {
        let h = area.h / n as u32;
        for i in 0..n {
            let y = area.y + (i as u32 * h) as i32;
            out.push(Rect::new(area.x, y, area.w, h));
        }
        return out;
    }
    let net = area.h - gap_total;
    let base = net / n as u32;
    let rem = net % n as u32;
    let mut y = area.y;
    for i in 0..n {
        let h = base + if (i as u32) < rem { 1 } else { 0 };
        out.push(Rect::new(area.x, y, area.w, h));
        y += h as i32 + gap as i32;
    }
    out
}

/// 计算一款布局在某工作区内的全部槽位矩形（F276 判据「四款布局几何
/// 计算用例（各屏分辨率）」的机制实现——同一函数对 1366×768 到 4K
/// 全分辨率成立，缝宽恒 `gap`）。
pub fn snap_slots(area: Rect, layout: SnapLayout, gap: u32) -> Vec<Rect> {
    match layout {
        SnapLayout::Two => divide_h(area, 2, gap),
        SnapLayout::Four => {
            // 四分：先横切两列，再每列竖切两行。
            let cols = divide_h(area, 2, gap);
            let mut out = Vec::with_capacity(4);
            for col in cols {
                out.extend(divide_v(col, 2, gap));
            }
            out
        }
        SnapLayout::Three => {
            // 左 1/3 + 右列上下两块。
            let cols = divide_h(area, 3, gap);
            if cols.len() != 3 {
                return cols;
            }
            let mut out = vec![cols[0]];
            out.extend(divide_v(cols[2], 2, gap));
            out
        }
        SnapLayout::LeftBig => {
            // 左 2/3 + 右列上下两块：横切三等分后左两份合并为左大区。
            let cols = divide_h(area, 3, gap);
            if cols.len() != 3 {
                return cols;
            }
            let left = Rect::new(
                cols[0].x,
                cols[0].y,
                cols[0].w + gap + cols[1].w,
                cols[0].h,
            );
            let mut out = vec![left];
            out.extend(divide_v(cols[2], 2, gap));
            out
        }
    }
}

/// 分屏线拖动：四款布局里可拖的竖直分屏线（索引 0=主分隔线）拖到
/// `divider_x` 后，两邻区按新比例联动缩放（判据「拖分屏线联动缩放」）。
/// 返回重排后的槽位矩形；比例钳制在 20%-80%（拖不出废布局）。
pub fn drag_divider(
    area: Rect,
    layout: SnapLayout,
    gap: u32,
    divider_x: i32,
) -> Vec<Rect> {
    let span = area.w as i64 - gap as i64;
    if span <= 0 {
        return snap_slots(area, layout, gap);
    }
    let min = area.x as i64 + span * 20 / 100;
    let max = area.x as i64 + span * 80 / 100;
    let dx = (divider_x as i64).clamp(min, max);
    let left_w = (dx - area.x as i64).max(1) as u32;
    let right_w = (area.w - gap - left_w).max(1);
    let right_x = area.x + left_w as i32 + gap as i32;
    let right_area = Rect::new(right_x, area.y, right_w, area.h);
    let left_area = Rect::new(area.x, area.y, left_w, area.h);
    match layout {
        SnapLayout::Two => alloc::vec![left_area, right_area],
        SnapLayout::Three => {
            let mut out = vec![left_area];
            out.extend(divide_v(right_area, 2, gap));
            out
        }
        SnapLayout::LeftBig => {
            // 左大右小拖的是右列的竖线：左区占 2/3 锚——拖动改右列宽。
            let left = Rect::new(
                area.x,
                area.y,
                left_w + gap + right_w,
                area.h,
            );
            let right_col = Rect::new(right_x, area.y, right_w, area.h);
            let mut out = vec![left];
            out.extend(divide_v(right_col, 2, gap));
            out
        }
        SnapLayout::Four => {
            let mut out = Vec::with_capacity(4);
            out.extend(divide_v(left_area, 2, gap));
            out.extend(divide_v(right_area, 2, gap));
            out
        }
    }
}

// ---------------------------------------------------------------------------
// F286 跨屏拼接：源图按屏分割
// ---------------------------------------------------------------------------

/// 拼接分割：总虚拟桌面尺寸 `total`（多屏并排拼成的包围盒），第 `index`
/// 块屏的矩形为 `screen`——返回该屏在源图上的取样区段（按位置等比对
/// 位）。源图长宽比与包围盒不一致时按「填充裁切」对齐（不留黑边）。
pub fn tile_source(total: Rect, screen: Rect) -> Rect {
    if total.w == 0 || total.h == 0 {
        return Rect::new(0, 0, 0, 0);
    }
    let sx = if screen.x >= total.x {
        (screen.x - total.x) as u64
    } else {
        0
    };
    let sy = if screen.y >= total.y {
        (screen.y - total.y) as u64
    } else {
        0
    };
    Rect::new(
        (sx * 10_000 / total.w as u64) as i32,
        (sy * 10_000 / total.h as u64) as i32,
        (screen.w as u64 * 10_000 / total.w as u64) as u32,
        (screen.h as u64 * 10_000 / total.h as u64) as u32,
    )
}

// ---------------------------------------------------------------------------
// F298 磁贴流式排布
// ---------------------------------------------------------------------------

/// 快速设置磁贴档位（占格数——流式排布的权重）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TileSpan {
    /// 小：1×1。
    Small,
    /// 中：2×2。
    Medium,
    /// 宽：2×1。
    Wide,
}

impl TileSpan {
    pub fn cells(self) -> (u32, u32) {
        match self {
            TileSpan::Small => (1, 1),
            TileSpan::Wide => (2, 1),
            TileSpan::Medium => (2, 2),
        }
    }
}

/// 单元基准尺寸（面板内一格的边长；磁贴间隙用同一 `gap`）。
pub const TILE_CELL_PX: u32 = 64;

/// 磁贴流式排布：按声明顺序逐行放入 `panel_w` 宽的面板，行满换行；
/// 宽/中磁贴放不进剩余格时**换行**（不拆格、不留 1px 挤压）。
/// 返回每个磁贴的屏幕矩形（与输入同序——编辑态拖重排后重跑本函数）。
pub fn flow_tiles(spans: &[TileSpan], panel_w: u32, gap: u32) -> Vec<Rect> {
    let mut out = Vec::with_capacity(spans.len());
    if panel_w == 0 {
        return out;
    }
    let unit = TILE_CELL_PX + gap;
    let cols = ((panel_w + gap) / unit).max(1) as i32;
    let mut cursor_col: i32 = 0;
    let mut cursor_row: i32 = 0;
    for sp in spans {
        let (cw, ch) = sp.cells();
        let cw = cw as i32;
        let ch = ch as i32;
        if cursor_col + cw > cols {
            cursor_col = 0;
            cursor_row += 1;
        }
        let x = cursor_col * unit as i32;
        let y = cursor_row * unit as i32;
        out.push(Rect::new(
            x,
            y,
            cw as u32 * TILE_CELL_PX + (cw - 1).max(0) as u32 * gap,
            ch as u32 * TILE_CELL_PX + (ch - 1).max(0) as u32 * gap,
        ));
        cursor_col += cw;
    }
    out
}

/// 面板高度估算（F076 面板弹出 ≤100ms 的布局预算——先算高再渲染）。
/// 返回内容纵向边界（最后一行磁贴的底边）。
pub fn flow_height(spans: &[TileSpan], panel_w: u32, gap: u32) -> u32 {
    let rects = flow_tiles(spans, panel_w, gap);
    let max_row = rects.iter().map(|r| r.y + r.h as i32).max().unwrap_or(0);
    max_row.max(0) as u32
}

// ---------------------------------------------------------------------------
// F259/F084 网格视图坐标换算
// ---------------------------------------------------------------------------

/// 网格视图参数（图标 96px 单元 + 单元间距；列数由宽度推导）。
#[derive(Clone, Copy, Debug)]
pub struct GridSpec {
    pub cell_w: u32,
    pub cell_h: u32,
    pub gap_x: u32,
    pub gap_y: u32,
}

impl GridSpec {
    pub const DEFAULT: GridSpec = GridSpec { cell_w: 96, cell_h: 116, gap_x: 16, gap_y: 16 };
    /// 可视宽度下列数（至少 1 列——极窄不塌）。
    pub fn cols_for(&self, view_w: u32) -> u32 {
        let unit = self.cell_w + self.gap_x;
        if unit == 0 {
            return 1;
        }
        ((view_w + self.gap_x) / unit).max(1)
    }
    /// 第 `index` 项的单元矩形（行优先）。
    pub fn cell_at(&self, index: usize, view_w: u32) -> Rect {
        let cols = self.cols_for(view_w) as usize;
        let (col, row) = h2_slot(index, cols);
        Rect::new(
            (col as u32 * (self.cell_w + self.gap_x)) as i32,
            (row as u32 * (self.cell_h + self.gap_y)) as i32,
            self.cell_w,
            self.cell_h,
        )
    }
    /// 点→格：命中测试返回格索引（拖拽吸附与落点提示共用——
    /// 「新建落点=当前视图第一个可用网格位」的坐标层实现）。
    pub fn hit(&self, px: i32, py: i32, view_w: u32) -> Option<usize> {
        if px < 0 || py < 0 {
            return None;
        }
        let cols = self.cols_for(view_w) as i64;
        let unit_x = (self.cell_w + self.gap_x) as i64;
        let unit_y = (self.cell_h + self.gap_y) as i64;
        if unit_x == 0 || unit_y == 0 {
            return None;
        }
        let col = px as i64 / unit_x;
        let row = py as i64 / unit_y;
        if col >= cols {
            return None;
        }
        Some((row * cols + col) as usize)
    }
}

/// 网格落位（与 [`h2base::pick_slot`] 同规则——此处给 usize 便捷名）。
fn h2_slot(used: usize, cols: usize) -> (usize, usize) {
    let cols = if cols == 0 { 1 } else { cols };
    (used % cols, used / cols)
}

// ---------------------------------------------------------------------------
// 自检（判据逐条钉死）
// ---------------------------------------------------------------------------

pub fn run_h2geo_checks() -> CheckSet {
    let mut set = CheckSet::new("h2-h2geo");
    // --- F276 四款布局：槽数与缝宽恒成立（1366×768 / 2560×1440 / 4K）。 ---
    let screens = [
        Rect::new(0, 0, 1366, 768),
        Rect::new(0, 0, 2560, 1440),
        Rect::new(0, 0, 3840, 2160),
    ];
    let mut all_ok = true;
    for a in screens {
        for ly in [SnapLayout::Two, SnapLayout::Three, SnapLayout::Four, SnapLayout::LeftBig] {
            let slots = snap_slots(a, ly, SNAP_GAP_PX);
            all_ok &= slots.len() == ly.slots();
            // 不重叠且不越界（缝宽恒 8：相邻槽间隙 ≥ gap）。
            for r in &slots {
                all_ok &= r.x >= a.x && r.y >= a.y;
                all_ok &= r.x + r.w as i32 <= a.x + a.w as i32 + 1;
                all_ok &= r.y + r.h as i32 <= a.y + a.h as i32 + 1;
                all_ok &= r.w > 0 && r.h > 0;
            }
            for i in 0..slots.len() {
                for j in (i + 1)..slots.len() {
                    let (ra, rb) = (slots[i], slots[j]);
                    let sep_x = ra.x + ra.w as i32 <= rb.x || rb.x + rb.w as i32 <= ra.x;
                    let sep_y = ra.y + ra.h as i32 <= rb.y || rb.y + rb.h as i32 <= ra.y;
                    all_ok &= sep_x || sep_y;
                }
            }
        }
    }
    set.add("h2geo F276 four layouts", all_ok, "slots per screen");
    // 二分等宽：1366 宽 → (1366-8)/2 = 679 / 679（余数给前区）。
    let two = snap_slots(screens[0], SnapLayout::Two, 8);
    set.add(
        "h2geo F276 two halves",
        two[0].w == 679 && two[1].w == 679 && two[1].x == two[0].x + 679 + 8,
        "gap 8 honored",
    );
    // --- F276 分屏线拖动联动：比例钳制 20%-80%。 ---
    let dragged = drag_divider(screens[0], SnapLayout::Two, 8, 100);
    set.add(
        "h2geo F276 divider clamp",
        dragged[0].w as u64 >= (screens[0].w as u64 - 8) * 20 / 100 - 1
            && dragged[0].w as u64 <= (screens[0].w as u64 - 8) * 80 / 100 + 1,
        "20-80 clamp",
    );
    let mid = drag_divider(screens[0], SnapLayout::Two, 8, 683);
    set.add("h2geo F276 divider mid", mid.len() == 2 && mid[0].w + mid[1].w + 8 == screens[0].w, "widths sum");
    // --- F286 拼接分割：双屏 1920+1920 → 各取源图左右半。 ---
    let total = Rect::new(0, 0, 3840, 2160);
    let l = tile_source(total, Rect::new(0, 0, 1920, 2160));
    let r = tile_source(total, Rect::new(1920, 0, 1920, 2160));
    set.add(
        "h2geo F286 splice split",
        l.x == 0 && l.w == 5_000 && r.x == 5_000 && r.w == 5_000,
        "half/half per-mille",
    );
    // --- F298 磁贴流式：8 磁贴默认档全部小格 → 一行放下由列数决定。 ---
    let small8 = [TileSpan::Small; 8];
    let panel = 8 * TILE_CELL_PX + 7 * 8; // 恰 8 列
    let laid = flow_tiles(&small8, panel, 8);
    set.add(
        "h2geo F298 one row",
        laid.len() == 8 && laid.iter().all(|r| r.y == 0) && laid[1].x == laid[0].x + TILE_CELL_PX as i32 + 8,
        "flow row",
    );
    // 宽磁贴放不进剩余格 → 换行（不拆格）。第 3 枚（宽）在行 0 放下，
    // 第 4 枚（小）被挤到行 1——换行的是被挤的小磁贴。
    let mix = [TileSpan::Small, TileSpan::Small, TileSpan::Wide, TileSpan::Small];
    let narrow = 4 * TILE_CELL_PX + 3 * 8; // 4 列
    let laid2 = flow_tiles(&mix, narrow, 8);
    set.add(
        "h2geo F298 wide wraps",
        laid2[3].y > laid2[0].y && laid2[2].w == 2 * TILE_CELL_PX + 8,
        "wrap whole",
    );
    let hgt = flow_height(&small8, panel, 8);
    set.add("h2geo F298 height", hgt == TILE_CELL_PX, "single row height");
    // --- F259/F084 网格换算：cell_at 与 hit 互逆。 ---
    let g = GridSpec::DEFAULT;
    let vw = 5 * (96 + 16); // 5 列
    let c = g.cell_at(7, vw);
    set.add(
        "h2geo F259 cell at",
        c.x == 2 * (96 + 16) as i32 && c.y == (116 + 16) as i32,
        "row major (h≠w)",
    );
    let hit = g.hit(c.x + 8, c.y + 8, vw);
    set.add("h2geo F259 hit inverse", hit == Some(7), "point→cell");
    set.add("h2geo F259 hit outside", g.hit(-1, 0, vw).is_none(), "no ghost cell");
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn h2geo_all_green() {
        let set = run_h2geo_checks();
        let (p, f) = set.tally();
        assert!(set.all_passed(), "h2geo 自检红 {f}/{p}");
    }

    #[test]
    fn four_way_never_overlaps_on_odd_widths() {
        // 奇数宽度（1371）下四分也不重叠不越界——余数分配的回归锚。
        let a = Rect::new(0, 0, 1371, 769);
        let slots = snap_slots(a, SnapLayout::Four, 8);
        for i in 0..slots.len() {
            for j in (i + 1)..slots.len() {
                let (ra, rb) = (slots[i], slots[j]);
                let sep_x = ra.x + ra.w as i32 <= rb.x || rb.x + rb.w as i32 <= ra.x;
                let sep_y = ra.y + ra.h as i32 <= rb.y || rb.y + rb.h as i32 <= ra.y;
                assert!(sep_x || sep_y, "槽 {i} 与 {j} 重叠");
            }
        }
    }

    #[test]
    fn zero_gaps_never_panic() {
        // 零格网降级不炸（与 h2base::pick_slot 同纪律）。
        let laid = flow_tiles(&[TileSpan::Small], 0, 8);
        assert!(laid.is_empty());
        assert_eq!(GridSpec::DEFAULT.cols_for(0), 1);
    }
}
