//! VE-F0729 曲线升降阶（Degree Raising / Lowering）
//! 职责：升阶（精确，可逆）、降阶（最佳逼近最小二乘 + 误差报告）、
//!       升降阶往返误差度量。
//! 契约：升阶后轨迹逐点不变；先升后降误差 ≤ 直接降阶误差。

use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::vec321_bezcore::{Curve, Deg, Pt};

/// 升阶：n 阶 → n+1 阶（控制点数 +1），轨迹精确不变
/// C'_i = (i/(n+1)) C_{i-1} + (1 - i/(n+1)) C_i，边界 C'_0 = C_0, C'_{n+1} = C_n
pub fn raise(c: &Curve) -> Curve {
    let n = c.ctrl.len() - 1;
    let mut out: Vec<Pt> = alloc::vec![c.ctrl[0]];
    for i in 1..=n {
        let k = i as f64 / (n + 1) as f64;
        out.push(c.ctrl[i - 1].mul(k).add(c.ctrl[i].mul(1.0 - k)));
    }
    out.push(c.ctrl[n]);
    let deg = match out.len() {
        2 => Deg::Line,
        3 => Deg::Quad,
        4 => Deg::Cube,
        _ => Deg::Quart,
    };
    Curve::build(deg, out).expect("raise")
}

/// 降阶：n 阶 → n-1 阶最佳逼近（最小二乘），返回（曲线，最大轨迹误差）
pub fn lower(c: &Curve) -> (Curve, f64) {
    let n = c.ctrl.len();
    if n <= 2 {
        return (c.clone(), 0.0);
    }
    // 目标：找 m = n-1 个控制点 Q 使 ∫|B_n(t) - B_{m}(t)|² dt 最小。
    // 以 Bernstein 基 Gram 矩阵在 [0,1] 上的离散化近似（256 采样）。
    let m = n - 1;
    let samples = 256;
    let mut a = [[0.0f64; 5]; 5]; // m ≤ 4 足够（Quart→Cube）
    let mut bx = [0.0f64; 5];
    let mut by = [0.0f64; 5];
    for k in 0..samples {
        let t = (k as f64 + 0.5) / samples as f64;
        let p = c.at(t);
        for j in 0..m {
            let bj = bernstein_j(m, j, t);
            for i in 0..m {
                a[i][j] += bj * bernstein_j(m, i, t);
            }
            bx[j] += bj * p.x;
            by[j] += bj * p.y;
        }
    }
    // 解 m×m 线性系统（高斯消元）
    let mut aug_x = [[0.0f64; 6]; 5];
    let mut aug_y = [[0.0f64; 6]; 5];
    for i in 0..m {
        for j in 0..m {
            aug_x[i][j] = a[i][j];
            aug_y[i][j] = a[i][j];
        }
        aug_x[i][m] = bx[i];
        aug_y[i][m] = by[i];
    }
    let qx = solve(&mut aug_x, m);
    let qy = solve(&mut aug_y, m);
    let qs: Vec<Pt> = (0..m).map(|i| Pt::new(qx[i], qy[i])).collect();
    let deg = match m {
        2 => Deg::Line,
        3 => Deg::Quad,
        _ => Deg::Cube,
    };
    let out = Curve::build(deg, qs).expect("lower");
    // 误差：轨迹最大偏差
    let mut worst = 0.0f64;
    for k in 0..=samples {
        let t = k as f64 / samples as f64;
        worst = worst.max(out.at(t).dist(c.at(t)));
    }
    (out, worst)
}

fn bernstein_j(m: usize, j: usize, t: f64) -> f64 {
    super::vec321_bezcore::bernstein(m - 1, j, t)
}

/// 高斯消元解线性系统（增广矩阵，m ≤ 5）
fn solve(aug: &mut [[f64; 6]; 5], m: usize) -> [f64; 5] {
    for col in 0..m {
        // 选主元
        let mut piv = col;
        for r in col..m {
            if aug[r][col].abs() > aug[piv][col].abs() {
                piv = r;
            }
        }
        aug.swap(col, piv);
        let d = aug[col][col];
        if d.abs() < 1e-18 {
            continue;
        }
        for r in 0..m {
            if r != col {
                let f = aug[r][col] / d;
                for cc in col..=m {
                    aug[r][cc] -= f * aug[col][cc];
                }
            }
        }
    }
    let mut out = [0.0f64; 5];
    for i in 0..m {
        out[i] = if aug[i][i].abs() >= 1e-18 {
            aug[i][m] / aug[i][i]
        } else {
            0.0
        };
    }
    out
}

/// 轨迹最大偏差
pub fn trace_err(a: &Curve, b: &Curve) -> f64 {
    let mut w = 0.0f64;
    for k in 0..=256 {
        let t = k as f64 / 256.0;
        w = w.max(a.at(t).dist(b.at(t)));
    }
    w
}

pub fn run_vec329_checks() -> crate::checks::CheckSet {
    let mut s = CheckSet::new("svstar2-vec329");
    let c3 = Curve::build(
        Deg::Cube,
        alloc::vec![Pt::new(0.0, 0.0), Pt::new(1.0, 3.0), Pt::new(3.0, 3.0), Pt::new(4.0, 0.0)],
    )
    .expect("c3");
    // 族一：升阶精确性
    let c4 = raise(&c3);
    s.add("升阶-控制点数", c4.ctrl.len() == 5, "4→5");
    s.add("升阶-端点", c4.ctrl[0] == c3.ctrl[0] && *c4.ctrl.last().expect("l") == c3.ctrl[3], "endpoints kept");
    s.add("升阶-轨迹不变", trace_err(&c3, &c4) < 1e-12, "trace exact");
    let c5 = raise(&c4);
    s.add("升阶-可链", c5.ctrl.len() == 6 && trace_err(&c3, &c5) < 1e-12, "double raise");
    s.add("升阶-端点仍稳", c5.ctrl[0] == c3.ctrl[0] && *c5.ctrl.last().expect("l") == c3.ctrl[3], "endpoints kept 2");
    // 族二：二次升三次
    let q = Curve::build(
        Deg::Quad,
        alloc::vec![Pt::new(0.0, 0.0), Pt::new(1.0, 2.0), Pt::new(2.0, 0.0)],
    )
    .expect("q");
    let q3 = raise(&q);
    s.add("升阶-二次", q3.deg == Deg::Cube && q3.ctrl.len() == 4, "quad→cube");
    s.add("升阶-二次轨迹", trace_err(&q, &q3) < 1e-12, "quad trace");
    // 二次中点 y=1：升阶后中点仍 1
    s.add("升阶-中点", (q3.at(0.5).y - 1.0).abs() < 1e-12, "mid y=1");
    // 族三：线性升阶
    let ln = Curve::build(Deg::Line, alloc::vec![Pt::new(0.0, 0.0), Pt::new(4.0, 4.0)]).expect("ln");
    let lq = raise(&ln);
    s.add("升阶-线性", lq.deg == Deg::Quad && trace_err(&ln, &lq) < 1e-12, "line→quad");
    // 族四：降阶——直线降阶无损
    let (ld, el) = lower(&lq);
    s.add("降阶-直线无损", el < 1e-12 && ld.deg == Deg::Line, "line lower exact");
    // 族五：二次降为线性（抛物线不可无损，误差 > 0）
    let (qd, eq) = lower(&q);
    s.add("降阶-抛物有损", eq > 0.01, "parabola cannot linearize");
    s.add("降阶-低阶", qd.deg == Deg::Line, "quad→line");
    // 族六：先升后降优于直接降阶
    let (_, e_direct) = lower(&c3);
    let (c3r, e_round) = lower(&c4); // 升阶再降
    s.add("往返-不劣", e_round <= e_direct + 1e-9, "raise-then-lower ≤ direct");
    s.add("往返-残差小", e_round < 0.2, "roundtrip bounded");
    s.add("往返-曲线有效", c3r.ctrl.len() == 3, "lowered to quad");
    // 族七：降阶结果的端点保持（最小二乘不强制端点，但退化情况应接近）
    // 验证降阶曲线轨迹接近原轨迹（数值一致性）
    s.add("降阶-轨迹近", trace_err(&c3, &c3r) < 0.25, "trace close");
    // 族八：solve 正确性（2x2 已知解）
    let mut aug = [[0.0f64; 6]; 5];
    aug[0][0] = 2.0;
    aug[0][2] = 4.0;
    aug[1][1] = 1.0;
    aug[1][2] = 3.0;
    let r = solve(&mut aug, 2);
    s.add("求解-2x2", r[0] == 2.0 && r[1] == 3.0, "x=2,y=3");
    let mut aug3 = [[0.0f64; 6]; 5];
    aug3[0] = [1.0, 1.0, 1.0, 6.0, 0.0, 0.0];
    aug3[1] = [0.0, 2.0, 5.0, -4.0, 0.0, 0.0];
    aug3[2] = [2.0, 5.0, -1.0, 27.0, 0.0, 0.0];
    let r3 = solve(&mut aug3, 3);
    s.add("求解-3x3", (r3[0] - 5.0).abs() < 1e-9 && (r3[1] - 3.0).abs() < 1e-9 && (r3[2] + 2.0).abs() < 1e-9, "x=5,y=3,z=-2");
    // 族九：退化单点曲线升阶
    let dot = Curve::build(Deg::Cube, alloc::vec![Pt::new(2.0, 3.0); 4]).expect("dot");
    let dot4 = raise(&dot);
    s.add("退化-点升阶", trace_err(&dot, &dot4) < 1e-12, "dot raise");
    s
}
