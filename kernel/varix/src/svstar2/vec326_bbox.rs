//! VE-F0726 曲线精确包围盒
//! 职责：基于导数零点解析定位的紧致包围盒（非控制点凸包近似）、
//!       包围盒面积、平移/缩放协变、粗盒（凸包）与紧盒双档。
//! 契约：紧盒四边均被曲线上某点触及（无死边）；粗盒必包含紧盒。

use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::vec321_bezcore::{Axis, Curve, Deg, Pt};

/// 包围盒
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Box2 {
    pub min: Pt,
    pub max: Pt,
}

impl Box2 {
    pub fn w(&self) -> f64 {
        self.max.x - self.min.x
    }
    pub fn h(&self) -> f64 {
        self.max.y - self.min.y
    }
    pub fn area(&self) -> f64 {
        self.w() * self.h()
    }
    pub fn contains(&self, p: Pt) -> bool {
        p.x >= self.min.x - 1e-12 && p.x <= self.max.x + 1e-12 && p.y >= self.min.y - 1e-12 && p.y <= self.max.y + 1e-12
    }
    pub fn translated(&self, d: Pt) -> Box2 {
        Box2 { min: self.min.add(d), max: self.max.add(d) }
    }
    pub fn scaled(&self, k: f64) -> Box2 {
        Box2 { min: self.min.mul(k), max: self.max.mul(k) }
    }
    pub fn union(a: Box2, b: Box2) -> Box2 {
        Box2 {
            min: Pt::new(a.min.x.min(b.min.x), a.min.y.min(b.min.y)),
            max: Pt::new(a.max.x.max(b.max.x), a.max.y.max(b.max.y)),
        }
    }
}

/// 紧致包围盒：导数零点处取值 + 端点
pub fn tight_bbox(c: &Curve) -> Box2 {
    let mut min = Pt::new(f64::INFINITY, f64::INFINITY);
    let mut max = Pt::new(f64::NEG_INFINITY, f64::NEG_INFINITY);
    for axis in [Axis::X, Axis::Y] {
        for t in c.axis_extrema(axis) {
            let p = c.at(t);
            min = Pt::new(min.x.min(p.x), min.y.min(p.y));
            max = Pt::new(max.x.max(p.x), max.y.max(p.y));
        }
    }
    Box2 { min, max }
}

/// 粗包围盒：控制点凸包（必包含曲线）
pub fn coarse_bbox(c: &Curve) -> Box2 {
    let mut min = Pt::new(f64::INFINITY, f64::INFINITY);
    let mut max = Pt::new(f64::NEG_INFINITY, f64::NEG_INFINITY);
    for p in &c.ctrl {
        min = Pt::new(min.x.min(p.x), min.y.min(p.y));
        max = Pt::new(max.x.max(p.x), max.y.max(p.y));
    }
    Box2 { min, max }
}

/// 边紧致性：每条边上是否存在曲线点触及（容差内）
fn edge_tight(c: &Curve, b: Box2, tol: f64) -> [bool; 4] {
    let n = 256;
    let mut touches = [false; 4];
    for k in 0..=n {
        let p = c.at(k as f64 / n as f64);
        if (p.x - b.min.x).abs() < tol {
            touches[0] = true;
        }
        if (p.x - b.max.x).abs() < tol {
            touches[1] = true;
        }
        if (p.y - b.min.y).abs() < tol {
            touches[2] = true;
        }
        if (p.y - b.max.y).abs() < tol {
            touches[3] = true;
        }
    }
    touches
}

pub fn run_vec326_checks() -> crate::checks::CheckSet {
    let mut s = CheckSet::new("svstar2-vec326");
    // 族一：对称立方曲线紧盒
    let c = Curve::build(
        Deg::Cube,
        alloc::vec![Pt::new(0.0, 0.0), Pt::new(1.0, 3.0), Pt::new(3.0, 3.0), Pt::new(4.0, 0.0)],
    )
    .expect("cube");
    let t = tight_bbox(&c);
    s.add("紧盒-底", t.min.y.abs() < 1e-9, "y min 0");
    s.add("紧盒-顶", (t.max.y - 2.25).abs() < 1e-6, "y max 2.25");
    s.add("紧盒-左", t.min.x.abs() < 1e-9, "x min 0");
    s.add("紧盒-右", (t.max.x - 4.0).abs() < 1e-9, "x max 4");
    // 粗盒（控制点 y 上到 3）必大于紧盒
    let g = coarse_bbox(&c);
    s.add("粗盒-包含紧盒", g.contains(t.min) && g.contains(t.max), "coarse ⊇ tight");
    s.add("粗盒-更松", g.max.y > t.max.y + 0.5, "coarse y=3 > 2.25");
    s.add("面积-有序", t.area() < g.area(), "tight area < coarse");
    // 族二：边紧致（每边被触及）
    let touch = edge_tight(&c, t, 1e-6);
    s.add("边紧-左", touch[0], "left edge touched");
    s.add("边紧-右", touch[1], "right edge touched");
    s.add("边紧-下", touch[2], "bottom edge touched");
    s.add("边紧-上", touch[3], "top edge touched");
    // 族三：紧盒内部曲线上点全在内
    let mut all_in = true;
    for k in 0..=64 {
        if !t.contains(c.at(k as f64 / 64.0)) {
            all_in = false;
        }
    }
    s.add("包含-全部曲线点", all_in, "curve ⊆ tight box");
    // 族四：协变性
    let d = Pt::new(10.0, -7.0);
    let c2 = Curve::build(Deg::Cube, c.ctrl.iter().map(|p| p.add(d)).collect()).expect("c2");
    let t2 = tight_bbox(&c2);
    s.add("平移-协变", t2.min.dist(t.min.add(d)) < 1e-9 && t2.max.dist(t.max.add(d)) < 1e-9, "translate covariant");
    let c3 = Curve::build(Deg::Cube, c.ctrl.iter().map(|p| p.mul(2.0)).collect()).expect("c3");
    let t3 = tight_bbox(&c3);
    s.add("缩放-协变", t3.min.dist(t.min.mul(2.0)) < 1e-9 && t3.max.dist(t.max.mul(2.0)) < 1e-9, "scale covariant");
    // 族五：盒代数
    let b1 = Box2 { min: Pt::new(0.0, 0.0), max: Pt::new(2.0, 2.0) };
    let b2 = Box2 { min: Pt::new(1.0, 1.0), max: Pt::new(3.0, 3.0) };
    let u = Box2::union(b1, b2);
    s.add("并-极值", u.min == Pt::new(0.0, 0.0) && u.max == Pt::new(3.0, 3.0), "union bounds");
    s.add("含-内点", b1.contains(Pt::new(1.0, 1.0)), "contains inner");
    s.add("含-边界", b1.contains(Pt::new(2.0, 0.0)), "contains boundary");
    s.add("不含-域外", !b1.contains(Pt::new(2.1, 0.0)), "excludes outside");
    s.add("宽高面积", (b1.w() - 2.0).abs() < 1e-12 && (b1.area() - 4.0).abs() < 1e-12, "w/h/area");
    // 族六：单调盒（无内部极值）退化为端点盒
    let ln = Curve::build(Deg::Line, alloc::vec![Pt::new(1.0, 2.0), Pt::new(5.0, 6.0)]).expect("ln");
    let tl = tight_bbox(&ln);
    s.add("线性-端点盒", tl.min == Pt::new(1.0, 2.0) && tl.max == Pt::new(5.0, 6.0), "line bbox = endpoints");
    // 族七：退化曲线（全控制点重合）
    let dot = Curve::build(Deg::Cube, alloc::vec![Pt::new(2.0, 3.0); 4]).expect("dot");
    let td = tight_bbox(&dot);
    s.add("点曲线-零面积", td.area() < 1e-18, "degenerate dot");
    s.add("点曲线-位置", td.min == Pt::new(2.0, 3.0), "dot position");
    // 族八：多段盒合并
    let (l, r) = super::vec323_split::split(&c, 0.5);
    let tl_ = tight_bbox(&l);
    let tr = tight_bbox(&r);
    let tu = Box2::union(tl_, tr);
    s.add("分段-并覆盖", tu.contains(t.min) && tu.contains(t.max), "split union ⊇ whole");
    s
}
