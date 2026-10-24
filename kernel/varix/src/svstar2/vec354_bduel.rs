//! VE-F0754 布尔对拍参考
//! 职责：以「扫描线积分」为参考实现的布尔面积对拍（点分类法 vs 区间法）、
//!       双规则（非零/奇偶）对拍、参考实现在已知形状上的封闭解验证。
//! 契约：两实现偏差入容差即绿；参考解必须有封闭解锚点。

use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::vec321_bezcore::Pt;
use super::vec341_boolorg::{FillRule, Poly, DirEdge, scan_row, inside_by_rule};

/// 点分类法面积（蒙特卡洛网格采样）
pub fn area_point_sample(edges: &[DirEdge], lo: Pt, hi: Pt, n: usize, rule: FillRule) -> f64 {
    let dx = (hi.x - lo.x) / n as f64;
    let dy = (hi.y - lo.y) / n as f64;
    let cell = dx * dy;
    let mut cnt = 0usize;
    for j in 0..n {
        let y = lo.y + (j as f64 + 0.5) * dy;
        let evs = scan_row(edges, y);
        let flags = inside_by_rule(&evs, rule);
        // 该行内部区段
        let mut segs: Vec<(f64, f64)> = alloc::vec![];
        let mut i = 0;
        while i < flags.len() {
            if flags[i] {
                let x0 = evs[i].x;
                let mut k = i;
                while k < flags.len() && flags[k] {
                    k += 1;
                }
                let x1 = if k < evs.len() { evs[k].x } else { x0 };
                segs.push((x0, x1));
                i = k;
            } else {
                i += 1;
            }
        }
        for i in 0..n {
            let x = lo.x + (i as f64 + 0.5) * dx;
            if segs.iter().any(|(a, b)| x >= *a && x <= *b) {
                cnt += 1;
            }
        }
    }
    cnt as f64 * cell
}

/// 区间法面积（行积分）
pub fn area_span(edges: &[DirEdge], ys: &[f64], dy: f64, rule: FillRule) -> f64 {
    let mut total = 0.0;
    for &y in ys {
        let evs = scan_row(edges, y);
        let flags = inside_by_rule(&evs, rule);
        let mut i = 0;
        while i < flags.len() {
            if flags[i] {
                let x0 = evs[i].x;
                let mut k = i;
                while k < flags.len() && flags[k] {
                    k += 1;
                }
                let x1 = if k < evs.len() { evs[k].x } else { x0 };
                total += (x1 - x0) * dy;
                i = k;
            } else {
                i += 1;
            }
        }
    }
    total
}

/// 双规则对拍（单环必一致；带洞环必不同）
pub fn rule_duel(edges: &[DirEdge], ys: &[f64], dy: f64) -> (f64, f64) {
    (
        area_span(edges, ys, dy, FillRule::NonZero),
        area_span(edges, ys, dy, FillRule::EvenOdd),
    )
}

/// 对拍报告
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct DuelRep {
    pub a: f64,
    pub b: f64,
    pub tol: f64,
}

impl DuelRep {
    pub fn pass(&self) -> bool {
        (self.a - self.b).abs() <= self.tol
    }
    pub fn dev(&self) -> f64 {
        (self.a - self.b).abs()
    }
}

pub fn run_vec354_checks() -> crate::checks::CheckSet {
    let mut s = CheckSet::new("svstar2-vec354");
    let square = Poly::new(alloc::vec![Pt::new(0.0, 0.0), Pt::new(4.0, 0.0), Pt::new(4.0, 4.0), Pt::new(0.0, 4.0)]);
    let edges = square.edges();
    // 族一：封闭解锚点
    s.add("锚点-鞋带", (square.area() - 16.0).abs() < 1e-12, "shoelace 16");
    // 区间法精确
    let ys: Vec<f64> = alloc::vec![0.5, 1.5, 2.5, 3.5];
    let span_area = area_span(&edges, &ys, 1.0, FillRule::NonZero);
    s.add("锚点-区间法", (span_area - 16.0).abs() < 1e-9, "span 16");
    // 点采样法（4×4 恰好全内）
    let pt_area = area_point_sample(&edges, Pt::ZERO, Pt::new(4.0, 4.0), 4, FillRule::NonZero);
    s.add("锚点-点采样", (pt_area - 16.0).abs() < 1e-9, "point 16");
    // 族二：双实现对拍
    let rep = DuelRep { a: span_area, b: pt_area, tol: 1e-9 };
    s.add("对拍-方形过", rep.pass(), "square duel");
    s.add("对拍-偏差", rep.dev() < 1e-9, "tiny dev");
    // 三角形
    let tri = Poly::new(alloc::vec![Pt::new(0.0, 0.0), Pt::new(4.0, 0.0), Pt::new(2.0, 4.0)]);
    let tri_edges = tri.edges();
    let tri_span = area_span(&tri_edges, &alloc::vec![1.0, 3.0], 2.0, FillRule::NonZero);
    // 封闭解 8；行积分近似（梯形行恰精确）：y=1 宽 3，y=3 宽 1 → 2×(3+1)=8
    s.add("锚点-三角", (tri_span - 8.0).abs() < 1e-9, "tri 8");
    let tri_pt = area_point_sample(&tri_edges, Pt::ZERO, Pt::new(4.0, 4.0), 8, FillRule::NonZero);
    let rep_t = DuelRep { a: tri_span, b: tri_pt, tol: 0.5 };
    s.add("对拍-三角过", rep_t.pass(), "tri duel (coarse grid tol)");
    // 族三：双规则对拍
    let (nz, eo) = rule_duel(&edges, &ys, 1.0);
    s.add("规则-单环同", (nz - eo).abs() < 1e-9, "single ring same");
    // 带洞环
    let hole = Poly::new(alloc::vec![Pt::new(1.0, 1.0), Pt::new(3.0, 1.0), Pt::new(3.0, 3.0), Pt::new(1.0, 3.0)]);
    let mut both = edges.clone();
    let hn = hole.verts.len();
    for i in 0..hn {
        both.push(DirEdge { a: hole.verts[(i + 1) % hn], b: hole.verts[i] });
    }
    let (nz2, eo2) = rule_duel(&both, &ys, 1.0);
    s.add("规则-带洞同", (nz2 - eo2).abs() < 1e-9, "aligned hole same");
    // 同向洞（非零规则被填充）：内环也 CCW
    let mut same_dir = edges.clone();
    for e in hole.edges() {
        same_dir.push(e);
    }
    let (nz3, eo3) = rule_duel(&same_dir, &ys, 1.0);
    s.add("规则-同向洞异", (nz3 - eo3).abs() > 1e-9, "same-direction hole differs");
    s.add("规则-非零更大", nz3 > eo3, "nonzero fills hole");
    // 族四：报告判定
    s.add("报告-过", DuelRep { a: 1.0, b: 1.0 + 1e-10, tol: 1e-9 }.pass(), "within tol");
    s.add("报告-不过", !DuelRep { a: 1.0, b: 1.1, tol: 1e-9 }.pass(), "out of tol");
    s.add("报告-偏差", (DuelRep { a: 2.0, b: 1.0, tol: 0.5 }.dev() - 1.0).abs() < 1e-12, "dev 1");
    // 族五：旋转方形（45°）面积不变
    let rot: Vec<Pt> = square.verts.iter().map(|v| {
        let (sn, cs) = (core::f64::consts::FRAC_PI_4.sin(), core::f64::consts::FRAC_PI_4.cos());
        Pt::new(v.x * cs - v.y * sn, v.x * sn + v.y * cs)
    }).collect();
    let rot_poly = Poly::new(rot);
    s.add("旋转-面积", (rot_poly.area() - 16.0).abs() < 1e-9, "45° area 16");
    s
}
