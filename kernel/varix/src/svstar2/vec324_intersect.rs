//! VE-F0724 曲线求交
//! 职责：曲线-曲线求交（细分法 + 单调域收缩）、曲线-水平/垂直线求交、
//!       交点去重、自交检测（保守）。
//! 契约：报出的交点双侧残差入容差；无交时报空。

use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::vec321_bezcore::{Axis, Curve, Deg, Pt};

/// 交点
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Hit {
    /// 曲线 A 的参数
    pub ta: f64,
    /// 曲线 B 的参数
    pub tb: f64,
    /// 交点坐标
    pub p: Pt,
}

/// 包围盒快速剔除
fn bbox_overlap(a: &Curve, b: &Curve) -> bool {
    let (amin, amax) = a.bbox();
    let (bmin, bmax) = b.bbox();
    amin.x <= bmax.x + 1e-12
        && bmin.x <= amax.x + 1e-12
        && amin.y <= bmax.y + 1e-12
        && bmin.y <= amax.y + 1e-12
}

/// 细分求交：递归细分直到包围盒小于 eps，取盒心参数
fn intersect_rec(
    a: &Curve,
    b: &Curve,
    ua: (f64, f64),
    ub: (f64, f64),
    eps: f64,
    depth: u32,
    out: &mut Vec<Hit>,
) {
    if !bbox_overlap(a, b) {
        return;
    }
    let (amin, amax) = a.bbox();
    let (bmin, bmax) = b.bbox();
    let w = (amax.x - amin.x).max(amax.y - amin.y).max((bmax.x - bmin.x).max(bmax.y - bmin.y));
    if w < eps || depth == 0 {
        let ta = (ua.0 + ua.1) * 0.5;
        let tb = (ub.0 + ub.1) * 0.5;
        let pa = a.at(ta);
        let pb = b.at(tb);
        if pa.dist(pb) < eps * 4.0 {
            out.push(Hit { ta, tb, p: pa.lerp(pb, 0.5) });
        }
        return;
    }
    let (a1, a2) = super::vec323_split::split(a, 0.5);
    let (b1, b2) = super::vec323_split::split(b, 0.5);
    let ma = (ua.0 + ua.1) * 0.5;
    let mb = (ub.0 + ub.1) * 0.5;
    for (sa, ra) in [(&a1, (ua.0, ma)), (&a2, (ma, ua.1))] {
        for (sb, rb) in [(&b1, (ub.0, mb)), (&b2, (mb, ub.1))] {
            intersect_rec(sa, sb, ra, rb, eps, depth - 1, out);
        }
    }
}

/// 曲线-曲线求交主入口
pub fn intersect(a: &Curve, b: &Curve, eps: f64) -> Vec<Hit> {
    let mut hits: Vec<Hit> = alloc::vec![];
    if bbox_overlap(a, b) {
        intersect_rec(a, b, (0.0, 1.0), (0.0, 1.0), eps, 24, &mut hits);
        dedup(&mut hits, eps);
    }
    hits
}

/// 曲线与水平线 y=h 的交（轴极值二分）
pub fn intersect_hline(c: &Curve, h: f64) -> Vec<(f64, Pt)> {
    let d = c.derivative().expect("deriv");
    let mut out: Vec<(f64, Pt)> = alloc::vec![];
    let steps = 128;
    let mut prev = c.at(0.0).y - h;
    if prev.abs() < 1e-12 {
        out.push((0.0, c.at(0.0)));
    }
    for k in 1..=steps {
        let t = k as f64 / steps as f64;
        let cur = c.at(t).y - h;
        if (prev < 0.0 && cur > 0.0) || (prev > 0.0 && cur < 0.0) {
            let (mut lo, mut hi) = ((k - 1) as f64 / steps as f64, t);
            for _ in 0..48 {
                let m = (lo + hi) * 0.5;
                if ((c.at(m).y - h) < 0.0) == (prev < 0.0) {
                    lo = m;
                } else {
                    hi = m;
                }
            }
            let tm = (lo + hi) * 0.5;
            out.push((tm, c.at(tm)));
        } else if cur.abs() < 1e-12 {
            // 导数也为零才是真触点，否则是重采样噪声——保守收录
            if d.at(t).y.abs() < 1e-3 {
                out.push((t, c.at(t)));
            }
        }
        prev = cur;
    }
    out
}

/// 曲线与垂直线 x=v 的交
pub fn intersect_vline(c: &Curve, v: f64) -> Vec<(f64, Pt)> {
    let d = c.derivative().expect("deriv");
    let mut out: Vec<(f64, Pt)> = alloc::vec![];
    let steps = 128;
    let mut prev = c.at(0.0).x - v;
    for k in 1..=steps {
        let t = k as f64 / steps as f64;
        let cur = c.at(t).x - v;
        if (prev < 0.0 && cur > 0.0) || (prev > 0.0 && cur < 0.0) {
            let (mut lo, mut hi) = ((k - 1) as f64 / steps as f64, t);
            for _ in 0..48 {
                let m = (lo + hi) * 0.5;
                if ((c.at(m).x - v) < 0.0) == (prev < 0.0) {
                    lo = m;
                } else {
                    hi = m;
                }
            }
            let tm = (lo + hi) * 0.5;
            out.push((tm, c.at(tm)));
        }
        prev = cur;
    }
    out
}

/// 去重（按坐标近邻合并）
fn dedup(hits: &mut Vec<Hit>, eps: f64) {
    hits.sort_by(|x, y| {
        x.p.x.partial_cmp(&y.p.x).unwrap_or(core::cmp::Ordering::Equal)
    });
    let mut i = 0;
    while i + 1 < hits.len() {
        if hits[i].p.dist(hits[i + 1].p) < eps * 4.0 {
            hits.remove(i + 1);
        } else {
            i += 1;
        }
    }
}

/// 保守自交检测：仅报告细分后仍重叠的非相邻段对（保守=可能有误报，不漏报）
pub fn self_intersect_conservative(c: &Curve, eps: f64) -> usize {
    let n = 8;
    let mut segs: Vec<Curve> = alloc::vec![];
    let mut acc: Vec<f64> = alloc::vec![0.0];
    for k in 0..n {
        let t = (k + 1) as f64 / n as f64;
        let last = segs.last().cloned().unwrap_or_else(|| c.clone());
        let local = if k == 0 { t } else { (t - acc[k]) / (1.0 - acc[k]) };
        let (a, b) = super::vec323_split::split(&last, local.clamp(0.0, 1.0));
        segs.pop();
        segs.push(a);
        segs.push(b);
        acc.push(t);
    }
    let mut count = 0;
    for i in 0..n {
        for j in (i + 2)..n {
            if i == 0 && j == n - 1 {
                continue; // 端点相邻（闭合意义）
            }
            if bbox_overlap(&segs[i], &segs[j]) {
                let mut hits: Vec<Hit> = alloc::vec![];
                intersect_rec(&segs[i], &segs[j], (0.0, 1.0), (0.0, 1.0), eps, 16, &mut hits);
                count += hits.len();
            }
        }
    }
    count
}

pub fn run_vec324_checks() -> crate::checks::CheckSet {
    let mut s = CheckSet::new("svstar2-vec324");
    // 族一：直线-直线（已知唯一交点）
    let a = Curve::build(Deg::Line, alloc::vec![Pt::new(0.0, 0.0), Pt::new(4.0, 4.0)]).expect("a");
    let b = Curve::build(Deg::Line, alloc::vec![Pt::new(0.0, 4.0), Pt::new(4.0, 0.0)]).expect("b");
    let hits = intersect(&a, &b, 1e-6);
    s.add("线线-唯一", hits.len() == 1, "one hit");
    s.add("线线-位置", hits[0].p.dist(Pt::new(2.0, 2.0)) < 1e-3, "cross at (2,2)");
    s.add("线线-参数", (hits[0].ta - 0.5).abs() < 1e-3 && (hits[0].tb - 0.5).abs() < 1e-3, "t=0.5 both");
    // 族二：平行线无交
    let c = Curve::build(Deg::Line, alloc::vec![Pt::new(0.0, 1.0), Pt::new(4.0, 5.0)]).expect("c");
    s.add("平行-无交", intersect(&a, &c, 1e-6).is_empty(), "parallel no hit");
    // 族三：不相交但盒重叠
    let d = Curve::build(Deg::Line, alloc::vec![Pt::new(0.5, 0.0), Pt::new(0.5, 0.5)]).expect("d");
    s.add("盒交线不交", intersect(&a, &d, 1e-6).is_empty(), "bbox overlap but no hit");
    // 族四：直线-抛物线
    let par = Curve::build(
        Deg::Quad,
        alloc::vec![Pt::new(-1.0, 1.0), Pt::new(1.0, -3.0), Pt::new(3.0, 1.0)],
    )
    .expect("par");
    // 抛物线 y = x^2 - 1（t∈[0,1] 对应 x∈[-1,3]? 控制点插值 y: 1,-3,1 → y(t)=1-8t(1-t)）
    let xaxis = Curve::build(Deg::Line, alloc::vec![Pt::new(-2.0, 0.0), Pt::new(4.0, 0.0)]).expect("xa");
    let hp = intersect(&par, &xaxis, 1e-6);
    s.add("线曲-数", hp.len() == 2, "two roots of x^2-1 in span");
    if hp.len() == 2 {
        s.add("线曲-y零", hp.iter().all(|h| h.p.y.abs() < 1e-3), "hits on y=0");
        s.add("线曲-x对", (hp[0].p.x + hp[1].p.x).abs() < 1e-2, "symmetric about x=1");
    }
    // 族五：水平线求交（抛物线峰 y=... ）
    let q = Curve::build(
        Deg::Quad,
        alloc::vec![Pt::new(0.0, 0.0), Pt::new(1.0, 2.0), Pt::new(2.0, 0.0)],
    )
    .expect("q");
    let hq = intersect_hline(&q, 1.0);
    s.add("水平线-数", hq.len() == 2, "y=1 crosses twice");
    s.add("水平线-对称", (hq[0].0 + hq[1].0 - 1.0).abs() < 1e-3, "t sums to 1");
    s.add("水平线-峰上无", intersect_hline(&q, 3.0).is_empty(), "above peak");
    s.add("水平线-峰下无", intersect_hline(&q, -1.0).is_empty(), "below base");
    let hq2 = intersect_hline(&q, 0.0);
    s.add("水平线-切于端点", !hq2.is_empty(), "touches at t=0 endpoint");
    // 族六：垂直线
    let vq = intersect_vline(&q, 1.0);
    s.add("垂直线-唯一", vq.len() == 1, "x=1 single");
    s.add("垂直线-值", (vq[0].1.y - 1.0).abs() < 1e-9, "x=1 → y=1");
    s.add("垂直线-域外", intersect_vline(&q, 5.0).is_empty(), "x=5 none");
    // 族七：自交
    let simple = Curve::build(
        Deg::Cube,
        alloc::vec![Pt::new(0.0, 0.0), Pt::new(1.0, 2.0), Pt::new(3.0, 2.0), Pt::new(4.0, 0.0)],
    )
    .expect("simple");
    s.add("自交-简单曲线无", self_intersect_conservative(&simple, 1e-4) == 0, "no self cross");
    let looped = Curve::build(
        Deg::Cube,
        alloc::vec![Pt::new(0.0, 0.0), Pt::new(4.0, 4.0), Pt::new(-4.0, 4.0), Pt::new(0.0, 0.0)],
    )
    .expect("looped");
    s.add("自交-环曲有", self_intersect_conservative(&looped, 1e-4) >= 1, "loop crosses self");
    // 族八：参数与坐标闭合
    for h in &hp {
        let pa = par.at(h.ta);
        s.add("闭合-A侧", pa.dist(h.p) < 1e-2, "param a consistent");
    }
    let _ = Axis::X;
    s
}
