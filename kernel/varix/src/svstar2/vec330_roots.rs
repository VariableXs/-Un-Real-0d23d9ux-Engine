//! VE-F0730 曲线根求解
//! 职责：曲线上标量方程 f(t)=0 的根（单调区间二分 + 切线修正）、
//!       多项式形式根（Bernstein 凸组合二分）、根计数与隔离。
//! 契约：根残差入容差；根数不超过阶数；区间外不报根。

use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::vec321_bezcore::{Curve, Deg, Pt};

/// 曲线某轴减常数后的根（即曲线与水平/垂直线的参数解）
pub fn roots_axis(c: &Curve, axis: super::vec321_bezcore::Axis, val: f64) -> Vec<f64> {
    let d = match c.derivative() {
        Some(d) => d,
        None => return alloc::vec![],
    };
    let f = |t: f64| axis.get(c.at(t)) - val;
    let mut out: Vec<f64> = alloc::vec![];
    // 检查端点
    if f(0.0).abs() < 1e-12 {
        out.push(0.0);
    }
    // 扫描符号变化 + 二分
    let steps = 256;
    let mut prev = f(0.0);
    for k in 1..=steps {
        let t = k as f64 / steps as f64;
        let cur = f(t);
        if (prev < 0.0 && cur > 0.0) || (prev > 0.0 && cur < 0.0) {
            let mut lo = (k - 1) as f64 / steps as f64;
            let mut hi = t;
            for _ in 0..60 {
                let m = (lo + hi) * 0.5;
                if (f(m) < 0.0) == (prev < 0.0) {
                    lo = m;
                } else {
                    hi = m;
                }
            }
            out.push((lo + hi) * 0.5);
        } else if cur.abs() < 1e-12 {
            // 触点：导数同号过零才算切点，保守收录一次
            let dv = axis.get(d.at(t));
            if dv.abs() < 1e-6 {
                out.push(t);
            }
        }
        prev = cur;
    }
    if f(1.0).abs() < 1e-12 && !out.contains(&1.0) {
        out.push(1.0);
    }
    out
}

/// Bernstein 多项式系数直接二分求根（f(t) = Σ c_i B_i(t)）
pub fn roots_bernstein(coeffs: &[f64]) -> Vec<f64> {
    let n = coeffs.len() - 1;
    let eval = |t: f64| -> f64 {
        let mut acc = 0.0;
        for i in 0..=n {
            acc += coeffs[i] * super::vec321_bezcore::bernstein(n, i, t);
        }
        acc
    };
    let mut out: Vec<f64> = alloc::vec![];
    if eval(0.0).abs() < 1e-12 {
        out.push(0.0);
    }
    let steps = 512;
    let mut prev = eval(0.0);
    for k in 1..=steps {
        let t = k as f64 / steps as f64;
        let cur = eval(t);
        if (prev < 0.0 && cur > 0.0) || (prev > 0.0 && cur < 0.0) {
            let mut lo = (k - 1) as f64 / steps as f64;
            let mut hi = t;
            for _ in 0..60 {
                let m = (lo + hi) * 0.5;
                if (eval(m) < 0.0) == (prev < 0.0) {
                    lo = m;
                } else {
                    hi = m;
                }
            }
            out.push((lo + hi) * 0.5);
        }
        prev = cur;
    }
    if eval(1.0).abs() < 1e-12 && !out.contains(&1.0) {
        out.push(1.0);
    }
    out
}

/// 根残差检查
pub fn residual(c: &Curve, axis: super::vec321_bezcore::Axis, val: f64, t: f64) -> f64 {
    (axis.get(c.at(t)) - val).abs()
}

/// 根去重（近邻合并）
pub fn dedup_roots(roots: &mut Vec<f64>, tol: f64) {
    roots.sort_by(|a, b| a.partial_cmp(b).unwrap_or(core::cmp::Ordering::Equal));
    let mut i = 0;
    while i + 1 < roots.len() {
        if roots[i + 1] - roots[i] < tol {
            roots.remove(i + 1);
        } else {
            i += 1;
        }
    }
}

pub fn run_vec330_checks() -> crate::checks::CheckSet {
    use super::vec321_bezcore::Axis;
    let mut s = CheckSet::new("svstar2-vec330");
    // 族一：抛物线与 y=0（2 根）、y=1（2 根）、y=3（0 根）
    let par = Curve::build(
        Deg::Quad,
        alloc::vec![Pt::new(0.0, 0.0), Pt::new(1.0, 2.0), Pt::new(2.0, 0.0)],
    )
    .expect("par");
    // y(t) = 4t(1-t)，y=0 → t∈{0,1}
    let r0 = roots_axis(&par, Axis::Y, 0.0);
    s.add("根-基线数", !r0.is_empty(), "y=0 has roots");
    s.add("根-基线残差", r0.iter().all(|&t| residual(&par, Axis::Y, 0.0, t) < 1e-9), "residual small");
    // y=1 → 4t(1-t)=1 → t=0.5（单根，切点）
    let r1 = roots_axis(&par, Axis::Y, 1.0);
    s.add("根-切点存在", !r1.is_empty(), "tangent root found");
    if !r1.is_empty() {
        s.add("根-切点位置", (r1[0] - 0.5).abs() < 1e-3, "t=0.5");
        s.add("根-切点残差", residual(&par, Axis::Y, 1.0, r1[0]) < 1e-6, "tangent residual");
    }
    // y=3（峰 1 以上）无根
    s.add("根-域外无", roots_axis(&par, Axis::Y, 3.0).is_empty(), "above peak none");
    // x = 1 → 单根 t=0.5
    let rx = roots_axis(&par, Axis::X, 1.0);
    s.add("根-x单", rx.len() == 1, "x=1 single");
    s.add("根-x位置", (rx[0] - 0.5).abs() < 1e-6, "t=0.5");
    // 族二：Bernstein 系数直接求根
    // 4t(1-t) 的 Bernstein 系数（n=2）：c0=0, c1=2, c2=0
    let rb = roots_bernstein(&alloc::vec![0.0, 2.0, 0.0]);
    s.add("Bern-端根", rb.contains(&0.0) && rb.contains(&1.0), "roots at 0 and 1");
    // t²-0.25 的 Bernstein（升阶到 n=2）：roots t=±0.5 → [0.5] only
    // f(t)=B(t) with c = [-0.25, -0.25, 0.75]: f(0)=-0.25, f(1)=0.5? c2=0.75 → f(1)=0.75。
    // roots: f(0.5) = -0.25*0.25 + -0.25*0.5 + 0.75*0.25 = -0.0625-0.125+0.1875 = 0 ✓
    let rc = roots_bernstein(&alloc::vec![-0.25, -0.25, 0.75]);
    s.add("Bern-内根", rc.len() == 1 && (rc[0] - 0.5).abs() < 1e-6, "root at 0.5");
    // 无根：全正系数
    s.add("Bern-无根", roots_bernstein(&alloc::vec![1.0, 2.0, 3.0]).is_empty(), "all positive no root");
    // 三次：t³-1 → 根 t=1（端点）
    // Bernstein n=3: f = Σ c_i B_i; c = [-1, -1, -1, 0] → f(1) = 0
    let rd = roots_bernstein(&alloc::vec![-1.0, -1.0, -1.0, 0.0]);
    s.add("Bern-端点根", rd.contains(&1.0), "root at t=1");
    // 族三：去重
    let mut dup = alloc::vec![0.5, 0.5000001, 0.7];
    dedup_roots(&mut dup, 1e-3);
    s.add("去重-合并", dup.len() == 2 && (dup[0] - 0.5).abs() < 1e-9, "merge near");
    s.add("去重-保序", dup[0] < dup[1], "sorted");
    // 族四：根数上界（二次最多 2 根）
    let r_all = roots_axis(&par, Axis::Y, 0.5);
    s.add("根数-上界", r_all.len() <= 2, "quad ≤ 2 roots");
    // 族五：三次曲线 x 轴三根验证
    // x(t) 单调（0..4），x=2 → 1 根
    let c3 = Curve::build(
        Deg::Cube,
        alloc::vec![Pt::new(0.0, 0.0), Pt::new(1.0, 3.0), Pt::new(3.0, 3.0), Pt::new(4.0, 0.0)],
    )
    .expect("c3");
    let rcx = roots_axis(&c3, Axis::X, 2.0);
    s.add("立方-x中点", rcx.len() == 1 && (rcx[0] - 0.5).abs() < 1e-6, "x=2 at t=0.5");
    // y=2.25（峰）→ 切点 t=0.5
    let rcy = roots_axis(&c3, Axis::Y, 2.25);
    if !rcy.is_empty() {
        s.add("立方-峰切点", rcy.iter().any(|&t| (t - 0.5).abs() < 1e-3), "peak tangent");
    }
    s.add("立方-峰有根", !rcy.is_empty(), "y=2.25 reached");
    // 族六：残差函数
    s.add("残差-构造", (residual(&par, Axis::X, 1.0, 0.5) - 0.0).abs() < 1e-9, "x(0.5)=1");
    s
}
