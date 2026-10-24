//! VE-F0761 九宫格核心模型
//! 职责：九宫格切片定义（四条内边距 → 九区）、尺寸不变性（角固定/边拉伸/中心双拉伸）、
//!       最小纹理尺寸校验、切片域坐标映射。
//! 契约：角区尺寸 = 内边距恒定；边区一维伸缩；中心区二维伸缩；内边距和 ≤ 纹理尺寸。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 四向内边距
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Insets {
    pub left: f64,
    pub top: f64,
    pub right: f64,
    pub bottom: f64,
}

impl Insets {
    pub fn uniform(v: f64) -> Insets {
        Insets { left: v, top: v, right: v, bottom: v }
    }
    pub fn sum_h(&self) -> f64 {
        self.left + self.right
    }
    pub fn sum_v(&self) -> f64 {
        self.top + self.bottom
    }
}

/// 九宫格切片
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Slice {
    /// 纹理尺寸
    pub tex_w: f64,
    pub tex_h: f64,
    pub inset: Insets,
}

impl Slice {
    /// 构造校验：内边距和 < 纹理尺寸
    pub fn build(tex_w: f64, tex_h: f64, inset: Insets) -> Option<Slice> {
        if tex_w <= 0.0 || tex_h <= 0.0 {
            return None;
        }
        if inset.sum_h() >= tex_w || inset.sum_v() >= tex_h {
            return None;
        }
        Some(Slice { tex_w, tex_h, inset })
    }
    /// 中心区尺寸
    pub fn center_w(&self) -> f64 {
        self.tex_w - self.inset.sum_h()
    }
    pub fn center_h(&self) -> f64 {
        self.tex_h - self.inset.sum_v()
    }
}

/// 目标区域布局结果（九区矩形，纹理空间 → 屏幕空间）
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Cell {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// 布局：把九宫格画到 (x, y, w, h)
pub fn layout(s: &Slice, x: f64, y: f64, w: f64, h: f64) -> Option<[Cell; 9]> {
    if w < s.inset.sum_h() || h < s.inset.sum_v() {
        return None; // 目标小于角区之和
    }
    let (l, t, r, b) = (s.inset.left, s.inset.top, s.inset.right, s.inset.bottom);
    let cw = w - l - r;
    let ch = h - t - b;
    let mk = |cx: f64, cy: f64, ww: f64, hh: f64| Cell { x: cx, y: cy, w: ww, h: hh };
    Some([
        mk(x, y, l, t),             // 0 左上角
        mk(x + l, y, cw, t),        // 1 上边
        mk(x + l + cw, y, r, t),    // 2 右上角
        mk(x, y + t, l, ch),        // 3 左边
        mk(x + l, y + t, cw, ch),   // 4 中心
        mk(x + l + cw, y + t, r, ch), // 5 右边
        mk(x, y + t + ch, l, b),    // 6 左下角
        mk(x + l, y + t + ch, cw, b), // 7 下边
        mk(x + l + cw, y + t + ch, r, b), // 8 右下角
    ])
}

/// 角区尺寸不变性：布局后角区 wh 与内边距一致
pub fn corners_fixed(s: &Slice, cells: &[Cell; 9]) -> bool {
    let (l, t, r, b) = (s.inset.left, s.inset.top, s.inset.right, s.inset.bottom);
    cells[0].w == l && cells[0].h == t && cells[2].w == r && cells[2].h == t
        && cells[6].w == l && cells[6].h == b && cells[8].w == r && cells[8].h == b
}

/// 九区覆盖完整性：面积和 = w×h
pub fn covers(cells: &[Cell; 9], w: f64, h: f64) -> bool {
    let area: f64 = cells.iter().map(|c| c.w * c.h).sum();
    (area - w * h).abs() < 1e-9
}

pub fn run_vec361_checks() -> crate::checks::CheckSet {
    let mut s = CheckSet::new("svstar2-vec361");
    let ins = Insets { left: 8.0, top: 8.0, right: 8.0, bottom: 8.0 };
    let sl = Slice::build(64.0, 64.0, ins).expect("slice");
    // 族一：构造校验
    s.add("构造-合法", sl.center_w() == 48.0 && sl.center_h() == 48.0, "center 48");
    s.add("构造-零尺寸拒", Slice::build(0.0, 64.0, ins).is_none(), "zero tex");
    s.add("构造-边距过大拒", Slice::build(16.0, 64.0, ins).is_none(), "inset ≥ tex");
    s.add("构造-边距恰好拒", Slice::build(16.0, 16.0, ins).is_none(), "sum == tex rejected");
    // 边距和
    s.add("边距-水平和", ins.sum_h() == 16.0, "l+r");
    s.add("边距-垂直和", ins.sum_v() == 16.0, "t+b");
    s.add("边距-均匀", Insets::uniform(5.0).sum_h() == 10.0, "uniform");
    // 族二：布局
    let cells = layout(&sl, 0.0, 0.0, 100.0, 80.0).expect("cells");
    s.add("布局-九区", cells.len() == 9, "9 cells");
    s.add("布局-覆盖", covers(&cells, 100.0, 80.0), "area sum");
    s.add("布局-角固定", corners_fixed(&sl, &cells), "corners 8×8");
    // 中心区伸缩
    s.add("布局-中心宽", cells[4].w == 84.0, "center w = 100-16");
    s.add("布局-中心高", cells[4].h == 64.0, "center h = 80-16");
    // 边区一维
    s.add("布局-上边横伸", cells[1].w == 84.0 && cells[1].h == 8.0, "top edge");
    s.add("布局-左边纵伸", cells[3].w == 8.0 && cells[3].h == 64.0, "left edge");
    // 族三：最小尺寸
    s.add("布局-过小拒", layout(&sl, 0.0, 0.0, 16.0, 80.0).is_none(), "w < l+r");
    s.add("布局-恰好最小", layout(&sl, 0.0, 0.0, 16.0, 16.0).is_some(), "w == l+r ok");
    let min_cells = layout(&sl, 0.0, 0.0, 16.0, 16.0).expect("min");
    s.add("布局-最小中心零", min_cells[4].w == 0.0 && min_cells[4].h == 0.0, "degenerate center");
    s.add("布局-最小覆盖", covers(&min_cells, 16.0, 16.0), "still covers");
    // 族四：非对称边距
    let asym = Slice::build(100.0, 100.0, Insets { left: 10.0, top: 20.0, right: 30.0, bottom: 5.0 }).expect("asym");
    let ac = layout(&asym, 0.0, 0.0, 200.0, 150.0).expect("ac");
    s.add("非对称-角", ac[0].w == 10.0 && ac[0].h == 20.0, "TL");
    s.add("非对称-右角", ac[2].w == 30.0 && ac[2].h == 20.0, "TR");
    s.add("非对称-下角", ac[6].w == 10.0 && ac[6].h == 5.0, "BL");
    s.add("非对称-中心", ac[4].w == 160.0 && ac[4].h == 125.0, "center");
    s.add("非对称-覆盖", covers(&ac, 200.0, 150.0), "covers");
    // 族五：偏移布局
    let off = layout(&sl, 10.0, 20.0, 100.0, 80.0).expect("off");
    s.add("偏移-原点", off[0].x == 10.0 && off[0].y == 20.0, "offset origin");
    s.add("偏移-右下", off[8].x == 10.0 + 92.0 && off[8].y == 20.0 + 72.0, "BR");
    s
}
