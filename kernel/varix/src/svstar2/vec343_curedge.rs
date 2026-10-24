//! VE-F0743 曲线边布尔
//! 职责：曲线边与直边混合路径的布尔扫描、交点插入（t 参数与坐标双记账）、
//!       分段标记（保留/丢弃）、混合路径面积。
//! 契约：插入交点后路径闭合且无重叠参数段；面积与扫描线环绕数一致。

use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::vec321_bezcore::{Curve, Deg, Pt};
use super::vec341_boolorg::Poly;

/// 路径段
#[derive(Clone, Debug)]
pub enum Seg {
    /// 直线段
    Line(Pt, Pt),
    /// 曲线段（贝塞尔）
    Curved(Curve),
}

/// 混合路径
#[derive(Clone, Debug)]
pub struct MixedPath {
    pub segs: Vec<Seg>,
}

impl MixedPath {
    pub fn new(segs: Vec<Seg>) -> MixedPath {
        MixedPath { segs }
    }
    /// 起点
    pub fn start(&self) -> Option<Pt> {
        match self.segs.first()? {
            Seg::Line(a, _) => Some(*a),
            Seg::Curved(c) => c.ctrl.first().copied(),
        }
    }
    /// 终点
    pub fn end(&self) -> Option<Pt> {
        match self.segs.last()? {
            Seg::Line(_, b) => Some(*b),
            Seg::Curved(c) => c.ctrl.last().copied(),
        }
    }
    /// 闭合性
    pub fn closed(&self) -> bool {
        match (self.start(), self.end()) {
            (Some(a), Some(b)) => a.dist(b) < 1e-9,
            _ => false,
        }
    }
    /// 直边采样化（供折线近似面积）
    pub fn flatten(&self, per_curve: usize) -> Vec<Pt> {
        let mut out: Vec<Pt> = alloc::vec![];
        for seg in &self.segs {
            match seg {
                Seg::Line(a, b) => {
                    if out.last().map(|l| l.dist(*a) > 1e-12).unwrap_or(true) {
                        out.push(*a);
                    }
                    out.push(*b);
                }
                Seg::Curved(c) => {
                    let n = per_curve.max(2);
                    for k in 1..=n {
                        out.push(c.at(k as f64 / n as f64));
                    }
                }
            }
        }
        out
    }
    /// 折线近似有向面积
    pub fn area_flat(&self, per_curve: usize) -> f64 {
        let pts = self.flatten(per_curve);
        let n = pts.len();
        if n < 3 {
            return 0.0;
        }
        let mut a = 0.0;
        for i in 0..n {
            let j = (i + 1) % n;
            a += pts[i].cross(pts[j]);
        }
        a * 0.5
    }
}

/// 曲线段在 y=h 处的交点参数（复用根求解）
pub fn curve_hline_ts(c: &Curve, h: f64) -> Vec<f64> {
    super::vec330_roots::roots_axis(c, super::vec321_bezcore::Axis::Y, h)
}

/// 直线段与 y=h 的交点参数
pub fn line_hline_t(a: Pt, b: Pt, h: f64) -> Option<f64> {
    let (y0, y1) = (a.y.min(b.y), a.y.max(b.y));
    if h < y0 || h >= y1 || (b.y - a.y).abs() < 1e-15 {
        return None;
    }
    Some((h - a.y) / (b.y - a.y))
}

/// 全路径与 y=h 的交点（记录段序与参数）
pub fn path_hline_hits(p: &MixedPath, h: f64) -> Vec<(usize, f64, Pt)> {
    let mut out: Vec<(usize, f64, Pt)> = alloc::vec![];
    for (i, seg) in p.segs.iter().enumerate() {
        match seg {
            Seg::Line(a, b) => {
                if let Some(t) = line_hline_t(*a, *b, h) {
                    out.push((i, t, a.lerp(*b, t)));
                }
            }
            Seg::Curved(c) => {
                for t in curve_hline_ts(c, h) {
                    out.push((i, t, c.at(t)));
                }
            }
        }
    }
    out.sort_by(|a, b| a.2.x.partial_cmp(&b.2.x).unwrap_or(core::cmp::Ordering::Equal));
    out
}

/// 交点插入后的分段标记：按 x 交替内/外（偶数规则）
pub fn keep_flags(hits: &[Pt]) -> Vec<bool> {
    let mut flags: Vec<bool> = alloc::vec![];
    let mut inside = false;
    for _ in 0..hits.len() {
        flags.push(inside);
        inside = !inside;
    }
    flags
}

pub fn run_vec343_checks() -> crate::checks::CheckSet {
    let mut s = CheckSet::new("svstar2-vec343");
    // 族一：混合路径基本量
    // 圆角方形近似：底线 + 右侧抛物线 + 顶线 + 左侧抛物线
    let bcurve = Curve::build(
        Deg::Quad,
        alloc::vec![Pt::new(4.0, 0.0), Pt::new(6.0, 0.0), Pt::new(6.0, 2.0)],
    )
    .expect("bc");
    let path = MixedPath::new(alloc::vec![
        Seg::Line(Pt::new(0.0, 0.0), Pt::new(4.0, 0.0)),
        Seg::Curved(bcurve.clone()),
        Seg::Line(Pt::new(6.0, 2.0), Pt::new(0.0, 2.0)),
        Seg::Curved(
            Curve::build(
                Deg::Quad,
                alloc::vec![Pt::new(0.0, 2.0), Pt::new(-2.0, 2.0), Pt::new(-2.0, 0.0)],
            )
            .expect("lc"),
        ),
        Seg::Line(Pt::new(-2.0, 0.0), Pt::new(0.0, 0.0)),
    ]);
    s.add("路径-闭合", path.closed(), "closed path");
    s.add("路径-起点", path.start().expect("s") == Pt::new(0.0, 0.0), "start");
    s.add("路径-段数", path.segs.len() == 5, "5 segs");
    // 族二：折线化
    let pts = path.flatten(16);
    s.add("折线-数量", pts.len() == 1 + 16 + 16 + 1, "line pts + curve samples");
    let area = path.area_flat(16);
    s.add("面积-正", area > 0.0, "CCW positive");
    // 底面积 8×2=16 + 两侧圆角外扩近似 > 16
    s.add("面积-大于矩形", area > 16.0, "rounded > rect");
    s.add("面积-收敛", (path.area_flat(64) - path.area_flat(16)).abs() < 0.05, "refinement stable");
    // 族三：直边交点
    let hits = path_hline_hits(&path, 1.0);
    s.add("交点-四点", hits.len() == 4, "4 crossings at y=1");
    s.add("交点-有序x", hits.windows(2).all(|w| w[0].2.x <= w[1].2.x), "x sorted");
    // 曲线段交点：bcurve y(t)=2t(1-t)*0+... 实际上 bcurve 从(4,0)经(6,0)到(6,2)：y(t)=2t²，y=1 → t=√0.5
    let hts = curve_hline_ts(&bcurve, 1.0);
    s.add("曲交-唯一", hts.len() == 1, "single root");
    s.add("曲交-残差", (bcurve.at(hts[0]).y - 1.0).abs() < 1e-6, "residual");
    // 族四：直线交点参数
    let lt = line_hline_t(Pt::new(0.0, 0.0), Pt::new(4.0, 0.0), 1.0);
    s.add("线交-水平无", lt.is_none(), "horizontal no cross");
    let lt2 = line_hline_t(Pt::new(0.0, 0.0), Pt::new(0.0, 2.0), 1.0);
    s.add("线交-垂直中", lt2 == Some(0.5), "vertical mid");
    let lt3 = line_hline_t(Pt::new(0.0, 0.0), Pt::new(2.0, 4.0), 2.0);
    s.add("线交-斜边", lt3 == Some(0.5), "diagonal");
    // 族五：分段标记
    let hp: Vec<Pt> = hits.iter().map(|h| h.2).collect();
    let flags = keep_flags(&hp);
    s.add("标记-交替", flags == alloc::vec![false, true, false, true], "alternating");
    // 族六：顶路径 y=2 交点（顶线水平共线 → 只有两曲线 + 直线端点）
    let h2 = path_hline_hits(&path, 2.0);
    s.add("顶行-交点少", h2.len() <= 4, "top row bounded");
    // 族七：域外行无交
    s.add("域外-无交", path_hline_hits(&path, 10.0).is_empty(), "y=10 none");
    s.add("域外-负无交", path_hline_hits(&path, -1.0).is_empty(), "y=-1 none");
    // 族八：Poly 联动
    let poly = Poly::new(path.flatten(8));
    s.add("联动-面积同阶", (poly.area() - path.area_flat(8)).abs() < 1e-9, "poly == flat");
    s
}
