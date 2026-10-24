//! VE-F0742 多边形裁剪实现
//! 职责：矩形裁剪（Sutherland-Hodgman）、凸多边形互裁、扫描线区间化、
//!       裁剪后面积守恒验证。
//! 契约：被裁多边形顶点全入矩形；凸-凸裁剪面积 = 交集面积。

use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::vec321_bezcore::Pt;
use super::vec341_boolorg::Poly;

/// 裁剪矩形
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct ClipRect {
    pub min: Pt,
    pub max: Pt,
}

impl ClipRect {
    pub fn contains(&self, p: Pt) -> bool {
        p.x >= self.min.x && p.x <= self.max.x && p.y >= self.min.y && p.y <= self.max.y
    }
    pub fn area(&self) -> f64 {
        (self.max.x - self.min.x) * (self.max.y - self.min.y)
    }
}

/// 内侧判定（相对一条裁剪边）
fn inside_edge(p: Pt, edge: usize, r: ClipRect) -> bool {
    match edge {
        0 => p.x >= r.min.x,
        1 => p.x <= r.max.x,
        2 => p.y >= r.min.y,
        _ => p.y <= r.max.y,
    }
}

/// 与裁剪边求交（边为轴向）
fn intersect_edge(a: Pt, b: Pt, edge: usize, r: ClipRect) -> Pt {
    match edge {
        0 => {
            let t = (r.min.x - a.x) / (b.x - a.x);
            Pt::new(r.min.x, a.y + t * (b.y - a.y))
        }
        1 => {
            let t = (r.max.x - a.x) / (b.x - a.x);
            Pt::new(r.max.x, a.y + t * (b.y - a.y))
        }
        2 => {
            let t = (r.min.y - a.y) / (b.y - a.y);
            Pt::new(a.x + t * (b.x - a.x), r.min.y)
        }
        _ => {
            let t = (r.max.y - a.y) / (b.y - a.y);
            Pt::new(a.x + t * (b.x - a.x), r.max.y)
        }
    }
}

/// Sutherland-Hodgman 矩形裁剪
pub fn clip_rect(poly: &Poly, r: ClipRect) -> Poly {
    let mut cur: Vec<Pt> = poly.verts.clone();
    for edge in 0..4 {
        if cur.is_empty() {
            break;
        }
        let mut out: Vec<Pt> = alloc::vec![];
        let n = cur.len();
        for i in 0..n {
            let a = cur[i];
            let b = cur[(i + 1) % n];
            let ia = inside_edge(a, edge, r);
            let ib = inside_edge(b, edge, r);
            if ia {
                out.push(a);
                if !ib {
                    out.push(intersect_edge(a, b, edge, r));
                }
            } else if ib {
                out.push(intersect_edge(a, b, edge, r));
            }
        }
        cur = out;
    }
    Poly::new(cur)
}

/// 裁剪后顶点全部在矩形内
pub fn all_clipped_in(p: &Poly, r: ClipRect) -> bool {
    p.verts.iter().all(|&v| r.contains(v))
}

/// 凸多边形（凸性检查：顶点叉积同号）
pub fn is_convex(p: &Poly) -> bool {
    let n = p.verts.len();
    if n < 3 {
        return false;
    }
    let mut pos = false;
    let mut neg = false;
    for i in 0..n {
        let a = p.verts[i];
        let b = p.verts[(i + 1) % n];
        let c = p.verts[(i + 2) % n];
        let cr = (b.sub(a)).cross(c.sub(b));
        if cr > 1e-12 {
            pos = true;
        }
        if cr < -1e-12 {
            neg = true;
        }
    }
    !(pos && neg)
}

/// 两凸多边形裁剪：用 A 的每条边裁 B（Sutherland-Hodgman 泛化）
pub fn clip_convex_convex(subject: &Poly, clip: &Poly) -> Poly {
    let mut cur: Vec<Pt> = subject.verts.clone();
    let n = clip.verts.len();
    // clip 多边形取 CCW：内侧为左手边
    let ccw = clip.normalize_ccw();
    for i in 0..n {
        if cur.is_empty() {
            break;
        }
        let a0 = ccw.verts[i];
        let b0 = ccw.verts[(i + 1) % n];
        let edge = b0.sub(a0);
        let inside = |p: Pt| edge.cross(p.sub(a0)) >= 0.0;
        let line_isect = |a: Pt, b: Pt| -> Pt {
            // 解 a + t(b-a) 落在 a0+t0*edge 上
            let d = b.sub(a);
            let den = d.cross(edge);
            let t = (a0.sub(a)).cross(edge) / den;
            a.add(d.mul(t))
        };
        let mut out: Vec<Pt> = alloc::vec![];
        let m = cur.len();
        for k in 0..m {
            let a = cur[k];
            let b = cur[(k + 1) % m];
            let ia = inside(a);
            let ib = inside(b);
            if ia {
                out.push(a);
                if !ib {
                    out.push(line_isect(a, b));
                }
            } else if ib {
                out.push(line_isect(a, b));
            }
        }
        cur = out;
    }
    Poly::new(cur)
}

pub fn run_vec342_checks() -> crate::checks::CheckSet {
    let mut s = CheckSet::new("svstar2-vec342");
    let r = ClipRect { min: Pt::new(1.0, 1.0), max: Pt::new(3.0, 3.0) };
    // 族一：完全在矩形内
    let inner = Poly::new(alloc::vec![Pt::new(1.5, 1.5), Pt::new(2.5, 1.5), Pt::new(2.5, 2.5), Pt::new(1.5, 2.5)]);
    let ci = clip_rect(&inner, r);
    s.add("内裁-不变", ci == inner, "fully inside kept");
    s.add("内裁-面积", (ci.area() - 1.0).abs() < 1e-12, "area 1");
    // 族二：跨矩形裁剪
    let big = Poly::new(alloc::vec![Pt::new(0.0, 0.0), Pt::new(4.0, 0.0), Pt::new(4.0, 4.0), Pt::new(0.0, 4.0)]);
    let cb = clip_rect(&big, r);
    s.add("跨裁-顶点在内", all_clipped_in(&cb, r), "verts clipped in");
    s.add("跨裁-面积", (cb.area() - 4.0).abs() < 1e-9, "clipped area = rect");
    s.add("跨裁-凸性", is_convex(&cb), "still convex");
    // 族三：完全在外
    let out = Poly::new(alloc::vec![Pt::new(5.0, 5.0), Pt::new(6.0, 5.0), Pt::new(6.0, 6.0), Pt::new(5.0, 6.0)]);
    s.add("外裁-空", clip_rect(&out, r).verts.is_empty(), "fully outside → empty");
    // 部分相交
    let part = Poly::new(alloc::vec![Pt::new(2.5, 2.5), Pt::new(4.5, 2.5), Pt::new(4.5, 4.5), Pt::new(2.5, 4.5)]);
    let cp = clip_rect(&part, r);
    s.add("交裁-面积", (cp.area() - 0.25).abs() < 1e-9, "quarter overlap");
    s.add("交裁-在内", all_clipped_in(&cp, r), "verts in");
    // 族四：凸性检查器
    let tri = Poly::new(alloc::vec![Pt::new(0.0, 0.0), Pt::new(2.0, 0.0), Pt::new(1.0, 2.0)]);
    s.add("凸-三角", is_convex(&tri), "triangle convex");
    let concave = Poly::new(alloc::vec![
        Pt::new(0.0, 0.0), Pt::new(4.0, 0.0), Pt::new(4.0, 4.0), Pt::new(2.0, 1.0), Pt::new(0.0, 4.0),
    ]);
    s.add("凹-检出", !is_convex(&concave), "concave detected");
    // 族五：凸-凸裁剪 = 交集
    let sq1 = Poly::new(alloc::vec![Pt::new(0.0, 0.0), Pt::new(2.0, 0.0), Pt::new(2.0, 2.0), Pt::new(0.0, 2.0)]);
    let sq2 = Poly::new(alloc::vec![Pt::new(1.0, 1.0), Pt::new(3.0, 1.0), Pt::new(3.0, 3.0), Pt::new(1.0, 3.0)]);
    let ix = clip_convex_convex(&sq1, &sq2);
    s.add("凸凸-面积", (ix.area() - 1.0).abs() < 1e-9, "unit overlap");
    s.add("凸凸-在内", all_clipped_in(&ix, r), "verts in clip rect");
    // 不相交凸多边形
    let sq3 = Poly::new(alloc::vec![Pt::new(5.0, 5.0), Pt::new(6.0, 5.0), Pt::new(6.0, 6.0), Pt::new(5.0, 6.0)]);
    s.add("凸凸-不相交", clip_convex_convex(&sq1, &sq3).verts.is_empty(), "disjoint empty");
    // 全含
    let sq4 = Poly::new(alloc::vec![Pt::new(0.0, 0.0), Pt::new(4.0, 0.0), Pt::new(4.0, 4.0), Pt::new(0.0, 4.0)]);
    let contained = clip_convex_convex(&sq1, &sq4);
    s.add("凸凸-全含", (contained.area() - 4.0).abs() < 1e-9, "subject kept");
    // 族六：三角形裁剪
    let tri_in = clip_rect(&tri, r);
    s.add("三角-在内或空", tri_in.verts.iter().all(|&v| r.contains(v)), "tri clipped");
    // 族七：退化空多边形
    s.add("退化-空入空出", clip_rect(&Poly::new(alloc::vec![]), r).verts.is_empty(), "empty in empty out");
    s
}
