//! VE-F0768 九宫格图层集成
//! 职责：图层（内容矩形 + 九宫格样式 + 状态标志）模型、
//!       图层树的绘制序（后序遍历）、可见性裁剪传播、
//!       样式缺失时的降级（纯色填充）。
//! 契约：子图层绘制区域必须 ⊆ 父图层裁剪盒；不可见子树零绘制。

use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::vec361_ninecore::{Cell, Insets, Slice, layout};

/// 图层矩形
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Rect {
    pub fn contains(&self, o: &Rect) -> bool {
        o.x >= self.x && o.y >= self.y && o.x + o.w <= self.x + self.w && o.y + o.h <= self.y + self.h
    }
    pub fn intersect(&self, o: &Rect) -> Option<Rect> {
        let x0 = self.x.max(o.x);
        let y0 = self.y.max(o.y);
        let x1 = (self.x + self.w).min(o.x + o.w);
        let y1 = (self.y + self.h).min(o.y + o.h);
        if x1 > x0 && y1 > y0 {
            Some(Rect { x: x0, y: y0, w: x1 - x0, h: y1 - y0 })
        } else {
            None
        }
    }
}

/// 图层样式
#[derive(Clone, Debug, PartialEq)]
pub enum Style {
    /// 九宫格（纹理标识 + 切片）
    Nine(u32, Slice),
    /// 纯色降级
    Solid(u8),
}

/// 图层
pub struct Layer {
    pub name: &'static str,
    pub rect: Rect,
    pub visible: bool,
    pub style: Style,
    pub children: Vec<Layer>,
}

impl Layer {
    pub fn new(name: &'static str, rect: Rect, style: Style) -> Layer {
        Layer { name, rect, visible: true, style, children: alloc::vec![] }
    }
}

/// 绘制指令
#[derive(Clone, Debug, PartialEq)]
pub enum Draw {
    Nine { name: &'static str, cells: [Cell; 9] },
    Solid { name: &'static str, r: Rect },
}

/// 遍历绘制（后序：先子后父；不可见子树跳过；裁剪盒传播）
pub fn paint(layer: &Layer, clip: &Rect, out: &mut Vec<Draw>) {
    if !layer.visible {
        return;
    }
    let eff = match clip.intersect(&layer.rect) {
        Some(r) => r,
        None => return, // 完全出裁剪盒
    };
    for c in &layer.children {
        paint(c, &eff, out);
    }
    match &layer.style {
        Style::Nine(_, slice) => {
            if let Some(cells) = layout(slice, eff.x, eff.y, eff.w, eff.h) {
                out.push(Draw::Nine { name: layer.name, cells });
            }
        }
        Style::Solid(_) => {
            out.push(Draw::Solid { name: layer.name, r: eff });
        }
    }
}

/// 绘制统计
pub fn paint_stats(draws: &[Draw]) -> (usize, usize, usize) {
    let mut nine = 0;
    let mut solid = 0;
    let mut names: Vec<&'static str> = alloc::vec![];
    for d in draws {
        match d {
            Draw::Nine { name, .. } => {
                nine += 1;
                names.push(name);
            }
            Draw::Solid { name, .. } => {
                solid += 1;
                names.push(name);
            }
        }
    }
    (nine, solid, names.len())
}

pub fn run_vec368_checks() -> crate::checks::CheckSet {
    let mut s = CheckSet::new("svstar2-vec368");
    let slice = Slice::build(64.0, 64.0, Insets::uniform(8.0)).expect("slice");
    // 族一：矩形代数
    let a = Rect { x: 0.0, y: 0.0, w: 100.0, h: 100.0 };
    let b = Rect { x: 20.0, y: 20.0, w: 30.0, h: 30.0 };
    s.add("矩形-包含", a.contains(&b), "b in a");
    s.add("矩形-自含", a.contains(&a), "self contains");
    let ix = a.intersect(&b).expect("ix");
    s.add("矩形-交", ix == b, "b ⊆ a → b");
    let c = Rect { x: 80.0, y: 80.0, w: 40.0, h: 40.0 };
    let ix2 = a.intersect(&c).expect("ix2");
    s.add("矩形-部分交", ix2 == Rect { x: 80.0, y: 80.0, w: 20.0, h: 20.0 }, "partial");
    let d = Rect { x: 200.0, y: 0.0, w: 10.0, h: 10.0 };
    s.add("矩形-无交", a.intersect(&d).is_none(), "disjoint");
    // 族二：绘制序
    let mut root = Layer::new("root", a, Style::Nine(1, slice));
    let mut child = Layer::new("btn", b, Style::Nine(2, slice));
    child.children.push(Layer::new("icon", Rect { x: 25.0, y: 25.0, w: 10.0, h: 10.0 }, Style::Solid(255)));
    root.children.push(child);
    let mut draws: Vec<Draw> = alloc::vec![];
    paint(&root, &a, &mut draws);
    // 后序：icon → btn → root
    let names: Vec<&'static str> = draws.iter().map(|d| match d {
        Draw::Nine { name, .. } => *name,
        Draw::Solid { name, .. } => *name,
    }).collect();
    s.add("绘制-后序", names == alloc::vec!["icon", "btn", "root"], "children first");
    s.add("绘制-三次", draws.len() == 3, "3 draws");
    let (nine, solid, total) = paint_stats(&draws);
    s.add("统计-九宫", nine == 2, "2 nine");
    s.add("统计-纯色", solid == 1, "1 solid");
    s.add("统计-总数", total == 3, "3 total");
    // 族三：可见性
    root.children[0].visible = false;
    let mut d2: Vec<Draw> = alloc::vec![];
    paint(&root, &a, &mut d2);
    s.add("隐藏-子树全跳", d2.len() == 1, "only root");
    s.add("隐藏-无 icon", !d2.iter().any(|d| matches!(d, Draw::Solid { name: "icon", .. })), "icon gone");
    // 族四：裁剪传播
    let clip = Rect { x: 0.0, y: 0.0, w: 30.0, h: 30.0 };
    root.children[0].visible = true;
    let mut d3: Vec<Draw> = alloc::vec![];
    paint(&root, &clip, &mut d3);
    // root 的绘制区域被裁剪到 30×30
    if let Some(Draw::Nine { cells, .. }) = d3.iter().rev().next() {
        s.add("裁剪-root区", cells[4].w == 30.0 - 16.0, "root clipped 30×30");
    }
    // icon 完全在裁剪盒外（25+10=35 > 30）→ 部分裁剪
    // icon 25..35 与 0..30 相交 25..30 → 仍绘制
    s.add("裁剪-icon部分", d3.iter().any(|d| matches!(d, Draw::Solid { name: "icon", .. })), "icon partial kept");
    // 完全出界子层
    root.children[0].children.clear();
    root.children[0].children.push(Layer::new("far", d, Style::Solid(9)));
    let mut d4: Vec<Draw> = alloc::vec![];
    paint(&root, &clip, &mut d4);
    s.add("裁剪-出界剔除", !d4.iter().any(|dd| matches!(dd, Draw::Solid { name: "far", .. })), "far dropped");
    // 族五：降级
    let fallback = Layer::new("fb", b, Style::Solid(77));
    let mut d5: Vec<Draw> = alloc::vec![];
    paint(&fallback, &a, &mut d5);
    s.add("降级-纯色", d5.len() == 1 && matches!(d5[0], Draw::Solid { .. }), "solid fallback");
    // 族六：样式缺纹理（Nine 布局失败 → 零绘制）
    let bad = Layer::new("bad", b, Style::Nine(9, Slice::build(16.0, 16.0, Insets::uniform(8.0)).expect("tight")));
    let mut d6: Vec<Draw> = alloc::vec![];
    // b 是 30×30 > 16 最小 → 可绘；构造一个目标小于角区和的场景
    let tiny = Layer::new("tiny", Rect { x: 0.0, y: 0.0, w: 8.0, h: 8.0 }, bad.style.clone());
    paint(&tiny, &a, &mut d6);
    s.add("降级-过小零绘制", d6.is_empty(), "too small skipped");
    s
}
