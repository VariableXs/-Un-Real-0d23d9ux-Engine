//! VE-F0731 曲率与曲率极值
//! 职责：有符号曲率 κ(t)、曲率半径、曲率极值定位（|κ| 峰值二分扫描）、
//!       拐点检测（κ 变号）、总转角。
//! 契约：圆的曲率恒 1/r；直线曲率恒 0；拐点数不超过阶数。

use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::vec321_bezcore::{Curve, Deg, Pt};

/// 有符号曲率 κ(t) = (v × a) / |v|³
pub fn curvature(c: &Curve, t: f64) -> Option<f64> {
    let v = c.velocity(t)?;
    let lv = v.len();
    if lv < 1e-14 {
        return None;
    }
    let a = c.second_derivative()?.at(t);
    Some(v.cross(a) / (lv * lv * lv))
}

/// 曲率半径（无符号）
pub fn radius(c: &Curve, t: f64) -> Option<f64> {
    curvature(c, t).map(|k| 1.0 / k.abs())
}

/// 拐点：曲率变号位置（|κ| 谷为 0 处）
pub fn inflections(c: &Curve, probes: usize) -> Vec<f64> {
    let n = probes.max(16);
    let mut out: Vec<f64> = alloc::vec![];
    let mut prev = curvature(c, 0.0);
    for k in 1..=n {
        let t = k as f64 / n as f64;
        let cur = curvature(c, t);
        if let (Some(p), Some(q)) = (prev, cur) {
            if (p < 0.0 && q > 0.0) || (p > 0.0 && q < 0.0) {
                // 二分收缩 κ=0
                let (mut lo, mut hi) = ((k - 1) as f64 / n as f64, t);
                for _ in 0..48 {
                    let m = (lo + hi) * 0.5;
                    let km = curvature(c, m).unwrap_or(0.0);
                    if (km < 0.0) == (p < 0.0) {
                        lo = m;
                    } else {
                        hi = m;
                    }
                }
                out.push((lo + hi) * 0.5);
            }
        }
        prev = cur;
    }
    out
}

/// |κ| 极大值位置（细扫描 + 局部三分）
pub fn max_curvature(c: &Curve, probes: usize) -> (f64, f64) {
    let n = probes.max(8);
    let mut best_t = 0.0;
    let mut best_k = 0.0f64;
    for k in 0..=n {
        let t = k as f64 / n as f64;
        if let Some(kk) = curvature(c, t) {
            let a = kk.abs();
            if a > best_k {
                best_k = a;
                best_t = t;
            }
        }
    }
    // 局部三分精化
    let span = 1.0 / n as f64;
    let mut lo = (best_t - span).max(0.0);
    let mut hi = (best_t + span).min(1.0);
    for _ in 0..48 {
        let m1 = lo + (hi - lo) / 3.0;
        let m2 = hi - (hi - lo) / 3.0;
        let k1 = curvature(c, m1).map(|k| k.abs()).unwrap_or(0.0);
        let k2 = curvature(c, m2).map(|k| k.abs()).unwrap_or(0.0);
        if k1 < k2 {
            hi = m2;
        } else {
            lo = m1;
        }
    }
    let t = (lo + hi) * 0.5;
    let k = curvature(c, t).map(|k| k.abs()).unwrap_or(best_k);
    (t, k.max(best_k))
}

/// 总转角：切向角变化的累加（绝对值）
pub fn total_turning(c: &Curve, probes: usize) -> f64 {
    let n = probes.max(8);
    let ang = |t: f64| -> f64 { c.velocity(t).map(|v| v.y.atan2(v.x)).unwrap_or(0.0) };
    let mut sum = 0.0f64;
    let mut prev = ang(0.0);
    for k in 1..=n {
        let t = k as f64 / n as f64;
        let cur = ang(t);
        let mut d = cur - prev;
        while d > core::f64::consts::PI {
            d -= 2.0 * core::f64::consts::PI;
        }
        while d < -core::f64::consts::PI {
            d += 2.0 * core::f64::consts::PI;
        }
        sum += d.abs();
        prev = cur;
    }
    sum
}

/// 构造近似圆弧的四段三次贝塞尔（单象限，magic k = 0.5522847498…）
pub fn circle_arc(r: f64) -> Curve {
    let kk = 0.5522847498307936f64;
    Curve::build(
        Deg::Cube,
        alloc::vec![
            Pt::new(r, 0.0),
            Pt::new(r, r * kk),
            Pt::new(r * kk, r),
            Pt::new(0.0, r),
        ],
    )
    .expect("arc")
}

pub fn run_vec331_checks() -> crate::checks::CheckSet {
    let mut s = CheckSet::new("svstar2-vec331");
    // 族一：直线曲率为零
    let ln = Curve::build(Deg::Line, alloc::vec![Pt::ZERO, Pt::new(5.0, 0.0)]).expect("ln");
    s.add("直线-曲率零", curvature(&ln, 0.5).is_none(), "line no curvature");
    // 直线二阶导不存在（Line derivative → None）→ curvature None
    // 族二：圆弧曲率恒 1/r
    let r = 2.0f64;
    let arc = circle_arc(r);
    let mut ks: Vec<f64> = alloc::vec![];
    for k in 0..=10 {
        let t = k as f64 / 10.0;
        ks.push(curvature(&arc, t).expect("k"));
    }
    s.add("圆弧-符号", ks.iter().all(|&k| k > 0.0), "consistent sign");
    s.add("圆弧-接近1/r", ks.iter().all(|&k| (k - 1.0 / r).abs() < 0.02), "κ ≈ 1/r");
    let rad = radius(&arc, 0.5).expect("r");
    s.add("圆弧-半径", (rad - r).abs() < 0.05, "ρ ≈ r");
    // 族三：抛物线曲率峰在顶点
    let par = Curve::build(
        Deg::Quad,
        alloc::vec![Pt::new(0.0, 0.0), Pt::new(1.0, 2.0), Pt::new(2.0, 0.0)],
    )
    .expect("par");
    let (tmax, kmax) = max_curvature(&par, 64);
    s.add("抛物-峰位", (tmax - 0.5).abs() < 0.02, "peak at apex");
    s.add("抛物-峰值", (kmax - 4.0).abs() < 0.05, "κ_max = 4");
    // 族四：拐点
    // S 形三次：控制点交替上/下 → 有拐点
    let s_curve = Curve::build(
        Deg::Cube,
        alloc::vec![Pt::new(0.0, 0.0), Pt::new(2.0, 3.0), Pt::new(2.0, -3.0), Pt::new(4.0, 0.0)],
    )
    .expect("s");
    let inf = inflections(&s_curve, 128);
    s.add("S形-有拐点", !inf.is_empty(), "has inflection");
    s.add("S形-拐点居中", inf.iter().all(|&t| (t - 0.5).abs() < 0.15), "near mid");
    // 圆弧无拐点
    s.add("圆弧-无拐点", inflections(&arc, 128).is_empty(), "no inflection");
    // 族五：总转角
    // 四分之一圆弧切向转角 = π/2
    let turn = total_turning(&arc, 64);
    s.add("转角-四分圆", (turn - core::f64::consts::FRAC_PI_2).abs() < 0.05, "quarter turn");
    // 直线总转角 = 0
    s.add("转角-直线零", total_turning(&ln, 32).abs() < 1e-9, "line no turn");
    // 族六：曲率对称性
    let kp0 = curvature(&par, 0.25).expect("k0");
    let kp1 = curvature(&par, 0.75).expect("k1");
    s.add("抛物-对称", (kp0 - kp1).abs() < 1e-9, "symmetric curvature");
    // 族七：半径与曲率互逆
    for t in [0.2, 0.5, 0.8] {
        let k = curvature(&par, t).expect("k");
        let rr = radius(&par, t).expect("r");
        s.add("互逆-校验", (1.0 / k.abs() - rr).abs() < 1e-9, "ρ = 1/|κ|");
    }
    s
}
