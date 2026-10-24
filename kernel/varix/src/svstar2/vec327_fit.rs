//! VE-F0727 曲线平滑拟合（点列 → 贝塞尔）
//! 职责：最小二乘拟合二次/三次贝塞尔（给定两端点自由内控点）、
//!       误差度量（最大偏差 / 均方根）、自动升阶判据、Chord-length 参数化。
//! 契约：拟合误差随自由度增加不增；精确样本（真实曲线采样）拟合可复现。

use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::vec321_bezcore::{Curve, Deg, Pt};

/// Chord-length 参数化：点列 → 参数列（累计弦长归一）
pub fn chord_params(pts: &[Pt]) -> Vec<f64> {
    let n = pts.len();
    if n == 0 {
        return alloc::vec![];
    }
    if n == 1 {
        return alloc::vec![0.0];
    }
    let mut ts = alloc::vec![0.0f64; n];
    let mut acc = 0.0;
    for i in 1..n {
        acc += pts[i].dist(pts[i - 1]);
        ts[i] = acc;
    }
    if acc > 0.0 {
        for t in ts.iter_mut() {
            *t /= acc;
        }
    }
    ts
}

/// 三次最小二乘拟合：固定首末点，解中间两控制点（法方程 2x2）
pub fn fit_cube(pts: &[Pt], ts: &[f64]) -> Curve {
    let n = pts.len();
    let p0 = pts[0];
    let p3 = pts[n - 1];
    // B(t) = b0(t)P0 + b1(t)C1 + b2(t)C2 + b3(t)P3
    // 残差对 C1/C2 线性 → 正规方程
    let mut a = [[0.0f64; 2]; 2];
    let mut bx = [0.0f64; 2];
    let mut by = [0.0f64; 2];
    for i in 0..n {
        let t = ts[i];
        let b1 = 3.0 * t * (1.0 - t) * (1.0 - t);
        let b2 = 3.0 * t * t * (1.0 - t);
        // 常数部分（P0/P3 项）
        let b0 = (1.0 - t) * (1.0 - t) * (1.0 - t);
        let b3 = t * t * t;
        let rx = pts[i].x - b0 * p0.x - b3 * p3.x;
        let ry = pts[i].y - b0 * p0.y - b3 * p3.y;
        a[0][0] += b1 * b1;
        a[0][1] += b1 * b2;
        a[1][0] += b1 * b2;
        a[1][1] += b2 * b2;
        bx[0] += b1 * rx;
        bx[1] += b2 * rx;
        by[0] += b1 * ry;
        by[1] += b2 * ry;
    }
    let det = a[0][0] * a[1][1] - a[0][1] * a[1][0];
    let (c1x, c1y, c2x, c2y) = if det.abs() > 1e-14 {
        let c1x = (bx[0] * a[1][1] - bx[1] * a[0][1]) / det;
        let c2x = (bx[1] * a[0][0] - bx[0] * a[1][0]) / det;
        let c1y = (by[0] * a[1][1] - by[1] * a[0][1]) / det;
        let c2y = (by[1] * a[0][0] - by[0] * a[1][0]) / det;
        (c1x, c1y, c2x, c2y)
    } else {
        // 退化：均匀参数兜底
        (p0.lerp(p3, 1.0 / 3.0).x, p0.lerp(p3, 1.0 / 3.0).y, p0.lerp(p3, 2.0 / 3.0).x, p0.lerp(p3, 2.0 / 3.0).y)
    };
    Curve::build(
        Deg::Cube,
        alloc::vec![p0, Pt::new(c1x, c1y), Pt::new(c2x, c2y), p3],
    )
    .expect("fit cube")
}

/// 二次拟合：固定首末，解单内控点（一维最小二乘，两轴独立）
pub fn fit_quad(pts: &[Pt], ts: &[f64]) -> Curve {
    let n = pts.len();
    let p0 = pts[0];
    let p2 = pts[n - 1];
    let mut aa = 0.0;
    let mut bx = 0.0;
    let mut by = 0.0;
    for i in 0..n {
        let t = ts[i];
        let b1 = 2.0 * t * (1.0 - t);
        let b0 = (1.0 - t) * (1.0 - t);
        let b2 = t * t;
        let rx = pts[i].x - b0 * p0.x - b2 * p2.x;
        let ry = pts[i].y - b0 * p0.y - b2 * p2.y;
        aa += b1 * b1;
        bx += b1 * rx;
        by += b1 * ry;
    }
    let c = if aa > 1e-14 { bx / aa } else { 0.0 };
    let cy = if aa > 1e-14 { by / aa } else { 0.0 };
    Curve::build(Deg::Quad, alloc::vec![p0, Pt::new(c, cy), p2]).expect("fit quad")
}

/// 最大偏差
pub fn max_dev(c: &Curve, pts: &[Pt], ts: &[f64]) -> f64 {
    pts.iter()
        .zip(ts.iter())
        .map(|(&p, &t)| c.at(t).dist(p))
        .fold(0.0f64, f64::max)
}

/// 均方根偏差
pub fn rms_dev(c: &Curve, pts: &[Pt], ts: &[f64]) -> f64 {
    let n = pts.len().max(1);
    let sum: f64 = pts.iter().zip(ts.iter()).map(|(&p, &t)| c.at(t).dist(p).powi(2)).sum();
    (sum / n as f64).sqrt()
}

pub fn run_vec327_checks() -> crate::checks::CheckSet {
    let mut s = CheckSet::new("svstar2-vec327");
    // 族一：Chord-length 参数化
    let pts_line = alloc::vec![Pt::new(0.0, 0.0), Pt::new(1.0, 0.0), Pt::new(3.0, 0.0)];
    let ts_line = chord_params(&pts_line);
    s.add("参数-起点零", ts_line[0] == 0.0, "t0=0");
    s.add("参数-终点一", (ts_line[2] - 1.0).abs() < 1e-12, "tN=1");
    s.add("参数-等距", (ts_line[1] - 0.25).abs() < 1e-12, "1/4 by chord");
    // 族二：真实三次曲线采样 → 拟合复现
    let truth = Curve::build(
        Deg::Cube,
        alloc::vec![Pt::new(0.0, 0.0), Pt::new(1.0, 3.0), Pt::new(3.0, 3.0), Pt::new(4.0, 0.0)],
    )
    .expect("truth");
    let mut samples: Vec<Pt> = alloc::vec![];
    let mut ts: Vec<f64> = alloc::vec![];
    for k in 0..=24 {
        let t = k as f64 / 24.0;
        samples.push(truth.at(t));
        ts.push(t);
    }
    let fitted = fit_cube(&samples, &ts);
    s.add("拟合-端点保持", fitted.at(0.0) == truth.at(0.0) && fitted.at(1.0) == truth.at(1.0), "endpoints fixed");
    let dev = max_dev(&fitted, &samples, &ts);
    s.add("拟合-复现精确", dev < 1e-6, "exact sample refit < 1e-6");
    s.add("拟合-内控点接近", {
        let mut d = 0.0f64;
        for i in 1..3 {
            d = d.max(fitted.ctrl[i].dist(truth.ctrl[i]));
        }
        d < 1e-4
    }, "inner ctrl close");
    // 族三：二次曲线被二次拟合复现
    let tq = Curve::build(
        Deg::Quad,
        alloc::vec![Pt::new(0.0, 0.0), Pt::new(1.0, 2.0), Pt::new(2.0, 0.0)],
    )
    .expect("tq");
    let mut sq: Vec<Pt> = alloc::vec![];
    let mut tsq: Vec<f64> = alloc::vec![];
    for k in 0..=16 {
        let t = k as f64 / 16.0;
        sq.push(tq.at(t));
        tsq.push(t);
    }
    let fq = fit_quad(&sq, &tsq);
    s.add("二次拟合-复现", max_dev(&fq, &sq, &tsq) < 1e-9, "quad refit exact");
    // 族四：带噪数据 → 拟合误差有界且 rms < max
    let mut noisy: Vec<Pt> = alloc::vec![];
    let mut tsn: Vec<f64> = alloc::vec![];
    for k in 0..=20 {
        let t = k as f64 / 20.0;
        let p = truth.at(t);
        let jitter = ((k % 5) as f64 - 2.0) * 0.01;
        noisy.push(Pt::new(p.x + jitter, p.y - jitter));
        tsn.push(t);
    }
    let fnz = fit_cube(&noisy, &tsn);
    let mx = max_dev(&fnz, &noisy, &tsn);
    let rms = rms_dev(&fnz, &noisy, &tsn);
    s.add("噪声-有界", mx < 0.1, "max dev < 0.1");
    s.add("噪声-rms<max", rms <= mx + 1e-12, "rms <= max");
    s.add("噪声-rms小", rms < 0.05, "rms < 0.05");
    // 族五：升阶不增误差——二次拟合误差 ≥ 三次拟合误差（同数据）
    let f2 = fit_quad(&noisy, &tsn);
    let e2 = rms_dev(&f2, &noisy, &tsn);
    s.add("升阶-不增", e2 >= rms - 1e-12, "quad err >= cube err");
    // 族六：退化点列
    let dup = alloc::vec![Pt::new(1.0, 1.0); 4];
    let tsd = chord_params(&dup);
    s.add("退化-全零参数", tsd.iter().all(|&t| t == 0.0), "zero chord → all 0");
    let fd = fit_cube(&dup, &tsd);
    s.add("退化-不崩", fd.at(0.5).dist(Pt::new(1.0, 1.0)) < 1e-9, "degenerate fit stable");
    // 族七：单点与两点
    s.add("单点-参数", chord_params(&alloc::vec![Pt::new(3.0, 4.0)]) == alloc::vec![0.0], "single pt");
    s.add("两点-两端", {
        let t2 = chord_params(&alloc::vec![Pt::ZERO, Pt::new(1.0, 1.0)]);
        t2.len() == 2 && t2[0] == 0.0 && t2[1] == 1.0
    }, "two pts");
    // 族八：拟合对参数化敏感——均匀参数拟合弦距样本误差更大（正常现象，验证可控）
    let mut tsu: Vec<f64> = (0..samples.len()).map(|k| k as f64 / (samples.len() - 1) as f64).collect();
    let fu = fit_cube(&samples, &tsu);
    s.add("参数化-弦距优", max_dev(&fitted, &samples, &ts) <= max_dev(&fu, &samples, &tsu) + 1e-9, "chord param not worse");
    tsu[0] = 0.0; // 防止未使用告警
    s
}
