//! VE-F0728 曲线偏移（Offset Curve）
//! 职责：法向偏移、固定距离近似偏移（采样重拟合）、偏移自交检测
//!       （凹侧距离超曲率半径时发生尖点）、双侧偏移带。
//! 契约：凸侧偏移点严格落在近似曲线上（局部线性段精确）；
//!       小偏移量下采样偏差入容差。

use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::vec321_bezcore::{Curve, Deg, Pt};

/// 法向（速度向量逆时针旋转 90°）
pub fn normal(c: &Curve, t: f64) -> Option<Pt> {
    let v = c.velocity(t)?;
    let l = v.len();
    if l < 1e-14 {
        return None;
    }
    Some(Pt::new(-v.y / l, v.x / l))
}

/// 单点偏移：c(t) + d·n(t)
pub fn offset_point(c: &Curve, t: f64, d: f64) -> Option<Pt> {
    normal(c, t).map(|n| c.at(t).add(n.mul(d)))
}

/// 均匀采样近似偏移曲线（返回采样点列，供渲染或重拟合）
pub fn offset_samples(c: &Curve, d: f64, samples: usize) -> Vec<Pt> {
    let n = samples.max(2);
    let mut out: Vec<Pt> = alloc::vec![];
    for k in 0..n {
        let t = k as f64 / (n - 1) as f64;
        if let Some(p) = offset_point(c, t, d) {
            out.push(p);
        }
    }
    out
}

/// 偏移近似误差：偏移采样点重新作为「目标点」，度量近似折线到真偏移曲线的距离上界
/// 这里用「弦中点到真偏移曲线的偏差」来度量折线逼近损失
pub fn offset_polyline_err(c: &Curve, d: f64, samples: usize) -> f64 {
    let pts = offset_samples(c, d, samples);
    let n = pts.len();
    let mut worst = 0.0f64;
    for k in 0..(n - 1) {
        let tm = (k as f64 + 0.5) / (n - 1) as f64;
        if let Some(ptrue) = offset_point(c, tm, d) {
            let chord_mid = pts[k].lerp(pts[k + 1], 0.5);
            worst = worst.max(ptrue.dist(chord_mid));
        }
    }
    worst
}

/// 曲率半径 ρ(t) = |v|³ / |v × a|（v/a 分别为一二阶导）
pub fn curvature_radius(c: &Curve, t: f64) -> Option<f64> {
    let v = c.velocity(t)?;
    let a = c.second_derivative()?.at(t);
    let cr = v.cross(a).abs();
    let lv = v.len();
    if cr < 1e-14 {
        return None; // 直线段：曲率为零
    }
    Some(lv * lv * lv / cr)
}

/// 偏移安全距离：小于全程最小曲率半径则凹侧无尖点
pub fn safe_offset(c: &Curve, probes: usize) -> f64 {
    let n = probes.max(4);
    let mut rmin = f64::INFINITY;
    for k in 0..=n {
        let t = k as f64 / n as f64;
        if let Some(r) = curvature_radius(c, t) {
            rmin = rmin.min(r);
        }
    }
    rmin
}

/// 双侧偏移带（左右点对）
pub fn offset_band(c: &Curve, d: f64, samples: usize) -> Vec<(Pt, Pt)> {
    let n = samples.max(2);
    let mut out: Vec<(Pt, Pt)> = alloc::vec![];
    for k in 0..n {
        let t = k as f64 / (n - 1) as f64;
        let p = c.at(t);
        if let Some(nv) = normal(c, t) {
            out.push((p.add(nv.mul(d)), p.sub(nv.mul(d))));
        } else {
            out.push((p, p));
        }
    }
    out
}

pub fn run_vec328_checks() -> crate::checks::CheckSet {
    let mut s = CheckSet::new("svstar2-vec328");
    // 族一：直线偏移精确
    let ln = Curve::build(Deg::Line, alloc::vec![Pt::new(0.0, 0.0), Pt::new(10.0, 0.0)]).expect("ln");
    let n0 = normal(&ln, 0.5).expect("n");
    s.add("法向-向上", n0 == Pt::new(0.0, 1.0), "left normal of +x is +y");
    let op = offset_point(&ln, 0.3, 2.0).expect("op");
    s.add("直线偏移-精确", op.dist(Pt::new(3.0, 2.0)) < 1e-12, "exact offset");
    let band = offset_band(&ln, 2.0, 8);
    s.add("带-对称", band[4].0.dist(Pt::new(0.0, 2.0).add(Pt::new(10.0 * 4.0 / 7.0, 0.0))) < 1e-9, "band +side");
    // 族二：抛物线小偏移误差有界
    let par = Curve::build(
        Deg::Quad,
        alloc::vec![Pt::new(0.0, 0.0), Pt::new(1.0, 2.0), Pt::new(2.0, 0.0)],
    )
    .expect("par");
    let err_coarse = offset_polyline_err(&par, 0.05, 8);
    let err_fine = offset_polyline_err(&par, 0.05, 64);
    s.add("偏移-正性", err_coarse > 0.0, "polyline error > 0");
    s.add("偏移-细化收敛", err_fine < err_coarse / 4.0, "8x samples → ≥4x better");
    s.add("偏移-小偏移小误差", err_fine < 1e-3, "fine err tiny");
    // 族三：偏移点在法向上（偏移向量与速度正交）
    for k in 1..10 {
        let t = k as f64 / 10.0;
        let p0 = par.at(t);
        let po = offset_point(&par, t, 0.5).expect("po");
        let v = par.velocity(t).expect("v");
        let off = po.sub(p0);
        s.add("正交-正交性", off.dot(v).abs() < 1e-9, "offset ⊥ velocity");
        s.add("正交-长度", (off.len() - 0.5).abs() < 1e-9, "|offset| = d");
    }
    // 族四：凸侧/凹侧方向
    // 抛物线开口向下（峰向上），法向 n=(-vy,vx)/|v|：t=0.5 处 v=(2,0) → n=(0,1) 指向峰外（凸侧）
    let up = offset_point(&par, 0.5, 0.1).expect("up");
    let down = offset_point(&par, 0.5, -0.1).expect("down");
    s.add("凸凹-上侧高", up.y > par.at(0.5).y && down.y < par.at(0.5).y, "+d is up at mid");
    // 族五：曲率半径
    let rmid = curvature_radius(&par, 0.5).expect("r");
    // 抛物线 y=2x(1-x) 在 x=1 处 y''=-4 → ρ = (1+0)^1.5 / 4 = 0.25
    s.add("曲率-峰半径", (rmid - 0.25).abs() < 1e-9, "rho = 0.25 at peak");
    let rend = curvature_radius(&par, 0.0).expect("r0");
    s.add("曲率-端半径", (rend - 1.25f64.powf(1.5) / 4.0).abs() < 1e-9, "rho at t=0");
    let rln = curvature_radius(&ln, 0.5);
    s.add("曲率-直线无", rln.is_none(), "line: no curvature");
    // 族六：安全偏移
    let safe = safe_offset(&par, 64);
    s.add("安全-最小半径", (safe - 0.25).abs() < 1e-6, "min rho 0.25");
    s.add("安全-超限判断", safe < 0.5, "0.5 offset exceeds rho → cusp risk");
    // 族七：采样完整性
    let pts = offset_samples(&par, 0.3, 32);
    s.add("采样-数量", pts.len() == 32, "32 samples");
    s.add("采样-首末", {
        let f = offset_point(&par, 0.0, 0.3).expect("f");
        let l = offset_point(&par, 1.0, 0.3).expect("l");
        pts[0].dist(f) < 1e-12 && pts[31].dist(l) < 1e-12
    }, "endpoints included");
    // 族八：偏移点严格在真偏移轨迹上（构造式定义自证）
    let t_check = 0.73;
    let po = offset_point(&par, t_check, 0.4).expect("po");
    let expect = par.at(t_check).add(normal(&par, t_check).expect("n").mul(0.4));
    s.add("定义-一致", po == expect, "point == base + d·n");
    s
}
