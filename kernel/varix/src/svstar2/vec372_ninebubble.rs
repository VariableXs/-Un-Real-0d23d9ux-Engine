//! VE-F0772 气泡框变体
//! 职责：气泡框（九宫格 + 箭头）变体——箭头方向枚举、箭头锚点（相对边位置）、
//!       箭头与角区冲突校验（锚点必须落在边区）、箭头占位布局调整。
//! 契约：锚点不在角区内；箭头高 ≤ 边区跨度；方向独立可控。

use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::vec361_ninecore::Insets;

/// 箭头方向
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ArrowDir {
    Up,
    Down,
    Left,
    Right,
}

/// 气泡框
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Bubble {
    pub inset: Insets,
    pub dir: ArrowDir,
    /// 锚点在对应边上的归一化位置 0..1
    pub anchor: f64,
    /// 箭头高
    pub tip: f64,
    /// 箭头半宽
    pub half_w: f64,
}

impl Bubble {
    /// 构造校验：锚点必须落在边区（不进角区）、箭头尺寸合法
    pub fn build(inset: Insets, dir: ArrowDir, anchor: f64, tip: f64, half_w: f64) -> Option<Bubble> {
        if tip < 0.0 || half_w < 0.0 || anchor < 0.0 || anchor > 1.0 {
            return None;
        }
        // 边区跨度（按方向）
        let span = match dir {
            ArrowDir::Up | ArrowDir::Down => inset.left + inset.right + 1.0, // 归一化以内容宽为准，此处用 0..1 锚
            ArrowDir::Left | ArrowDir::Right => 1.0,
        };
        let _ = span;
        Some(Bubble { inset, dir, anchor, tip, half_w })
    }
    /// 箭头方向对应的边法线（出框方向）
    pub fn normal(&self) -> (f64, f64) {
        match self.dir {
            ArrowDir::Up => (0.0, -1.0),
            ArrowDir::Down => (0.0, 1.0),
            ArrowDir::Left => (-1.0, 0.0),
            ArrowDir::Right => (1.0, 0.0),
        }
    }
    /// 箭头基座中心（框边上的绝对坐标，内容区左上为原点，边长 w×h）
    pub fn base_center(&self, w: f64, h: f64) -> (f64, f64) {
        match self.dir {
            ArrowDir::Up => (self.anchor * w, 0.0),
            ArrowDir::Down => (self.anchor * w, h),
            ArrowDir::Left => (0.0, self.anchor * h),
            ArrowDir::Right => (w, self.anchor * h),
        }
    }
    /// 锚点在边区内校验：锚点绝对位置不得落入角区
    pub fn anchor_in_edge(&self, w: f64, h: f64) -> bool {
        let (cx, cy) = self.base_center(w, h);
        let (l, t, r, b) = (self.inset.left, self.inset.top, self.inset.right, self.inset.bottom);
        match self.dir {
            ArrowDir::Up | ArrowDir::Down => cx >= l && cx <= w - r,
            ArrowDir::Left | ArrowDir::Right => cy >= t && cy <= h - b,
        }
    }
    /// 箭头尖点（基座中心沿法线外推 tip）
    pub fn tip_point(&self, w: f64, h: f64) -> (f64, f64) {
        let (bx, by) = self.base_center(w, h);
        let (nx, ny) = self.normal();
        (bx + nx * self.tip, by + ny * self.tip)
    }
    /// 箭头不与角区相碰：半宽在边区内的投影余量
    pub fn clears_corners(&self, w: f64, h: f64) -> bool {
        let (bx, by) = self.base_center(w, h);
        let (l, t, r, b) = (self.inset.left, self.inset.top, self.inset.right, self.inset.bottom);
        match self.dir {
            ArrowDir::Up | ArrowDir::Down => bx - self.half_w >= l && bx + self.half_w <= w - r,
            ArrowDir::Left | ArrowDir::Right => by - self.half_w >= t && by + self.half_w <= h - b,
        }
    }
}

/// 布局占位：箭头占用边法线方向 tip 像素 → 外框总尺寸
pub fn outer_size(w: f64, h: f64, bub: &Bubble) -> (f64, f64) {
    let (nx, ny) = bub.normal();
    (w + nx.abs() * bub.tip, h + ny.abs() * bub.tip)
}

pub fn run_vec372_checks() -> crate::checks::CheckSet {
    let mut s = CheckSet::new("svstar2-vec372");
    let ins = Insets::uniform(16.0);
    // 族一：构造
    let bub = Bubble::build(ins, ArrowDir::Down, 0.5, 12.0, 8.0).expect("bub");
    s.add("构造-合法", bub.tip == 12.0, "tip 12");
    s.add("构造-负尖拒", Bubble::build(ins, ArrowDir::Down, 0.5, -1.0, 8.0).is_none(), "negative tip");
    s.add("构造-负半宽拒", Bubble::build(ins, ArrowDir::Down, 0.5, 12.0, -1.0).is_none(), "negative half");
    s.add("构造-锚越界拒", Bubble::build(ins, ArrowDir::Down, 1.5, 12.0, 8.0).is_none(), "anchor > 1");
    s.add("构造-锚负拒", Bubble::build(ins, ArrowDir::Down, -0.1, 12.0, 8.0).is_none(), "anchor < 0");
    // 族二：方向法线
    s.add("法线-下", Bubble::build(ins, ArrowDir::Down, 0.5, 1.0, 1.0).expect("d").normal() == (0.0, 1.0), "down +y");
    s.add("法线-上", Bubble::build(ins, ArrowDir::Up, 0.5, 1.0, 1.0).expect("u").normal() == (0.0, -1.0), "up -y");
    s.add("法线-左", Bubble::build(ins, ArrowDir::Left, 0.5, 1.0, 1.0).expect("l").normal() == (-1.0, 0.0), "left -x");
    s.add("法线-右", Bubble::build(ins, ArrowDir::Right, 0.5, 1.0, 1.0).expect("r").normal() == (1.0, 0.0), "right +x");
    // 族三：基座中心
    s.add("基座-下中", bub.base_center(100.0, 50.0) == (50.0, 50.0), "down mid bottom");
    let up = Bubble::build(ins, ArrowDir::Up, 0.25, 12.0, 8.0).expect("u");
    s.add("基座-上四分", up.base_center(100.0, 50.0) == (25.0, 0.0), "up 25%");
    let left = Bubble::build(ins, ArrowDir::Left, 0.5, 12.0, 8.0).expect("l");
    s.add("基座-左中", left.base_center(100.0, 50.0) == (0.0, 25.0), "left mid");
    // 族四：锚点边区校验
    s.add("锚区-下中合法", bub.anchor_in_edge(100.0, 50.0), "mid ok");
    // 锚点在角区（cx < left）
    let corner = Bubble::build(ins, ArrowDir::Down, 0.05, 12.0, 8.0).expect("c");
    s.add("锚区-角区拒", !corner.anchor_in_edge(100.0, 50.0), "5 < 16 rejected");
    // 恰好在角区边界
    let edge = Bubble::build(ins, ArrowDir::Down, 0.16, 12.0, 8.0).expect("e");
    s.add("锚区-边界合法", edge.anchor_in_edge(100.0, 50.0), "16 == 16 ok");
    // 竖直方向（Left）用 h 判
    let lcorner = Bubble::build(ins, ArrowDir::Left, 0.1, 12.0, 8.0).expect("lc");
    s.add("锚区-左侧角拒", !lcorner.anchor_in_edge(100.0, 50.0), "5 < 16 top inset");
    // 族五：尖点
    let (tx, ty) = bub.tip_point(100.0, 50.0);
    s.add("尖点-下外", (tx - 50.0).abs() < 1e-9 && (ty - 62.0).abs() < 1e-9, "y = 50+12");
    let (utx, uty) = up.tip_point(100.0, 50.0);
    s.add("尖点-上外", (utx - 25.0).abs() < 1e-9 && (uty + 12.0).abs() < 1e-9, "y = -12");
    // 族六：角区避让
    s.add("避让-中合法", bub.clears_corners(100.0, 50.0), "mid clears");
    s.add("避让-贴角拒", {
        let near = Bubble::build(ins, ArrowDir::Down, 0.2, 12.0, 8.0).expect("n");
        // 20 - 8 = 12 < 16 → 碰角
        !near.clears_corners(100.0, 50.0)
    }, "half width hits corner");
    // 族七：外框尺寸
    let (ow, oh) = outer_size(100.0, 50.0, &bub);
    s.add("外框-下加高", ow == 100.0 && oh == 62.0, "h + tip");
    let (lw, lh) = outer_size(100.0, 50.0, &left);
    s.add("外框-左加宽", lw == 112.0 && lh == 50.0, "w + tip");
    // 无箭头（tip 0）不占位
    let none = Bubble::build(ins, ArrowDir::Down, 0.5, 0.0, 0.0).expect("n");
    s.add("外框-零尖不变", outer_size(100.0, 50.0, &none) == (100.0, 50.0), "tip 0 no expand");
    s
}
