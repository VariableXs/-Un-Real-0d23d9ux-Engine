//! VE-F0732 圆锥曲线支持（椭圆弧）
//! 职责：椭圆弧的贝塞尔近似（参数角分段）、椭圆几何量（离心率/焦点）、
//!       参数点与隐式方程互验、圆整化（圆为特例）。
//! 契约：近似弧的隐式残差（x²/a²+y²/b²-1）入容差；端点角度准确。

use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::vec321_bezcore::{Curve, Deg, Pt};

/// 椭圆参数化点
pub fn ellipse_pt(a: f64, b: f64, theta: f64) -> Pt {
    Pt::new(a * theta.cos(), b * theta.sin())
}

/// 单象限椭圆弧的四段三次贝塞尔控制点
const K_QUAD: f64 = 0.5522847498307936;

/// 椭圆单象限弧（θ ∈ [0, π/2]），a 半长轴 b 半短轴
pub fn ellipse_quarter(a: f64, b: f64) -> Curve {
    let ka = a * K_QUAD;
    let kb = b * K_QUAD;
    Curve::build(
        Deg::Cube,
        alloc::vec![
            Pt::new(a, 0.0),
            Pt::new(a, kb),
            Pt::new(ka, b),
            Pt::new(0.0, b),
        ],
    )
    .expect("quarter")
}

/// 椭圆整弧：四象限拼接（CCW）
pub fn ellipse_full(a: f64, b: f64) -> Vec<Curve> {
    let q = ellipse_quarter(a, b);
    let rot90 = |c: &Curve| -> Curve {
        Curve::build(
            c.deg,
            c.ctrl.iter().map(|p| Pt::new(-p.y, p.x)).collect(),
        )
        .expect("rot")
    };
    let mut out = alloc::vec![q.clone()];
    for _ in 0..3 {
        out.push(rot90(out.last().expect("last")));
    }
    out
}

/// 离心率
pub fn eccentricity(a: f64, b: f64) -> f64 {
    let (a, b) = if a >= b { (a, b) } else { (b, a) };
    (1.0 - (b * b) / (a * a)).sqrt()
}

/// 焦距 c = sqrt(a²-b²)（a ≥ b）
pub fn focal(a: f64, b: f64) -> f64 {
    let (a, b) = if a >= b { (a, b) } else { (b, a) };
    (a * a - b * b).sqrt()
}

/// 隐式残差 x²/a² + y²/b² - 1
pub fn implicit_resid(p: Pt, a: f64, b: f64) -> f64 {
    p.x * p.x / (a * a) + p.y * p.y / (b * b) - 1.0
}

/// 近似弧最大隐式残差
pub fn max_resid(c: &Curve, a: f64, b: f64, samples: usize) -> f64 {
    let n = samples.max(8);
    (0..=n)
        .map(|k| implicit_resid(c.at(k as f64 / n as f64), a, b).abs())
        .fold(0.0f64, f64::max)
}

/// 参数角对应的弧上近似参数（按角度采样映射到 t，供外部分段）
pub fn quarter_param(theta: f64) -> f64 {
    // 单象限均匀角参数映射 t ∈ [0,1]（近似线性——象限内角参数与 t 近似成比例）
    (theta / (core::f64::consts::FRAC_PI_2)).clamp(0.0, 1.0)
}

pub fn run_vec332_checks() -> crate::checks::CheckSet {
    let mut s = CheckSet::new("svstar2-vec332");
    // 族一：参数化点
    let p0 = ellipse_pt(3.0, 2.0, 0.0);
    s.add("参数-θ0", p0 == Pt::new(3.0, 0.0), "theta 0 → (a,0)");
    let p90 = ellipse_pt(3.0, 2.0, core::f64::consts::FRAC_PI_2);
    s.add("参数-θ90", p90.dist(Pt::new(0.0, 2.0)) < 1e-12, "theta 90 → (0,b)");
    let p45 = ellipse_pt(3.0, 2.0, core::f64::consts::FRAC_PI_4);
    s.add("参数-θ45", (p45.x - 3.0f64 * core::f64::consts::FRAC_1_SQRT_2).abs() < 1e-12, "x = a·cos45");
    // 族二：隐式残差——真参数点残差为零
    s.add("隐式-真点零", implicit_resid(p45, 3.0, 2.0).abs() < 1e-12, "exact point resid 0");
    s.add("隐式-内点负", implicit_resid(Pt::ZERO, 3.0, 2.0) == -1.0, "center → -1");
    s.add("隐式-外点正", implicit_resid(Pt::new(5.0, 0.0), 3.0, 2.0) > 0.0, "outside → +");
    // 族三：单象限弧近似残差
    let q = ellipse_quarter(3.0, 2.0);
    s.add("象限-端点", q.at(0.0).dist(Pt::new(3.0, 0.0)) < 1e-12 && q.at(1.0).dist(Pt::new(0.0, 2.0)) < 1e-12, "endpoints");
    let mr = max_resid(&q, 3.0, 2.0, 64);
    s.add("象限-残差小", mr < 5e-4, "max resid < 5e-4");
    // 圆特例（a=b）残差更小
    let qc = ellipse_quarter(2.0, 2.0);
    s.add("圆-残差", max_resid(&qc, 2.0, 2.0, 64) < 3e-4, "circle resid");
    // 族四：整弧四段
    let full = ellipse_full(3.0, 2.0);
    s.add("整弧-四段", full.len() == 4, "4 quarters");
    // 拼接连续性
    let mut joint_err = 0.0f64;
    for w in full.windows(2) {
        joint_err = joint_err.max(w[0].at(1.0).dist(w[1].at(0.0)));
    }
    s.add("整弧-接合", joint_err < 1e-12, "joints continuous");
    // 首尾闭合
    s.add("整弧-闭合", full[3].at(1.0).dist(full[0].at(0.0)) < 1e-12, "closed loop");
    // 全弧隐式残差
    let mrf = full.iter().map(|c| max_resid(c, 3.0, 2.0, 32)).fold(0.0f64, f64::max);
    s.add("整弧-残差", mrf < 5e-4, "whole resid");
    // 族五：几何量
    s.add("离心-圆零", eccentricity(2.0, 2.0).abs() < 1e-12, "circle e=0");
    let e = eccentricity(3.0, 2.0);
    // e = sqrt(1 - 4/9) = sqrt(5)/3 ≈ 0.745
    s.add("离心-值", (e - 5.0f64.sqrt() / 3.0).abs() < 1e-12, "e = √5/3");
    s.add("离心-对称", (eccentricity(2.0, 3.0) - e).abs() < 1e-12, "axis order free");
    let c_f = focal(3.0, 2.0);
    s.add("焦距-值", (c_f - 5.0f64.sqrt()).abs() < 1e-12, "c = √5");
    s.add("焦距-圆零", focal(2.0, 2.0).abs() < 1e-12, "circle c=0");
    // 焦点在椭圆上？c < a
    s.add("焦点-域内", c_f < 3.0, "c < a");
    // 族六：参数映射
    s.add("映射-端点", quarter_param(0.0) == 0.0 && quarter_param(core::f64::consts::FRAC_PI_2) == 1.0, "0..1");
    s.add("映射-越界夹取", quarter_param(2.0) == 1.0 && quarter_param(-1.0) == 0.0, "clamped");
    s
}
