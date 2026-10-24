//! VE-F0725 最近点查询
//! 职责：点到曲线的最近点（粗采样 + 局部三分搜索精化）、距离函数、
//!       距离平方（免开方）、多点批量查询。
//! 契约：返回点在曲线上（回代一致）；局部极小不劣于全局粗采样下界。

use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::vec321_bezcore::{Curve, Deg, Pt};

/// 最近点查询器
pub struct Nearest {
    /// 粗采样数
    pub coarse: usize,
    /// 三分搜索轮数
    pub iters: u32,
}

impl Nearest {
    pub fn new(coarse: usize, iters: u32) -> Nearest {
        Nearest { coarse: coarse.max(8), iters: iters.max(8) }
    }

    /// 距离平方（不最后开方，保精度）
    pub fn dist2(&self, c: &Curve, p: Pt) -> (f64, f64, Pt) {
        // 粗采样定界
        let n = self.coarse;
        let mut best_t = 0.0f64;
        let mut best_d2 = f64::INFINITY;
        for k in 0..=n {
            let t = k as f64 / n as f64;
            let d2 = c.at(t).sub(p).len2();
            if d2 < best_d2 {
                best_d2 = d2;
                best_t = t;
            }
        }
        // 三分搜索精化 [best_t-1/n, best_t+1/n]
        let span = 1.0 / n as f64;
        let mut lo = (best_t - span).max(0.0);
        let mut hi = (best_t + span).min(1.0);
        for _ in 0..self.iters {
            let m1 = lo + (hi - lo) / 3.0;
            let m2 = hi - (hi - lo) / 3.0;
            let d1 = c.at(m1).sub(p).len2();
            let d2 = c.at(m2).sub(p).len2();
            if d1 < d2 {
                hi = m2;
            } else {
                lo = m1;
            }
        }
        let t = (lo + hi) * 0.5;
        let q = c.at(t);
        (q.sub(p).len2(), t, q)
    }

    /// 距离
    pub fn dist(&self, c: &Curve, p: Pt) -> f64 {
        self.dist2(c, p).0.sqrt()
    }

    /// 批量查询
    pub fn batch(&self, c: &Curve, ps: &[Pt]) -> Vec<f64> {
        ps.iter().map(|&p| self.dist(c, p)).collect()
    }

    /// 距离场剖面：均匀 t 采样距离（供可视与 fuzz 对照）
    pub fn profile(&self, c: &Curve, p: Pt, samples: usize) -> Vec<f64> {
        (0..samples)
            .map(|k| {
                let t = k as f64 / (samples - 1).max(1) as f64;
                c.at(t).dist(p)
            })
            .collect()
    }
}

/// 距离场的最小值（对照用暴力）
pub fn min_dist_bruteforce(c: &Curve, p: Pt, n: usize) -> f64 {
    (0..=n).map(|k| c.at(k as f64 / n as f64).dist(p)).fold(f64::INFINITY, f64::min)
}

pub fn run_vec325_checks() -> crate::checks::CheckSet {
    let mut s = CheckSet::new("svstar2-vec325");
    let q = Nearest::new(64, 48);
    let c = Curve::build(
        Deg::Cube,
        alloc::vec![Pt::new(0.0, 0.0), Pt::new(1.0, 3.0), Pt::new(3.0, 3.0), Pt::new(4.0, 0.0)],
    )
    .expect("cube");
    // 族一：点在端点上
    let (d2, t, pt) = q.dist2(&c, Pt::new(0.0, 0.0));
    s.add("端点-距离零", d2 < 1e-16, "d=0 at endpoint");
    s.add("端点-参数", t < 1e-3, "t≈0");
    s.add("端点-回代", pt.dist(c.at(t)) < 1e-12, "on-curve");
    // 族二：中垂线点最近为峰
    let (_, t2, _) = q.dist2(&c, Pt::new(2.0, 5.0));
    // 对称曲线峰在 t=0.5 附近（x=2 处）
    s.add("对称峰-参数", (t2 - 0.5).abs() < 0.01, "t≈0.5");
    // 族三：最近距离不劣于粗采样下界
    let probe = Pt::new(2.0, -1.0);
    let (d2b, _, _) = q.dist2(&c, probe);
    let lower = min_dist_bruteforce(&c, probe, 64);
    s.add("下界-不劣", d2b <= lower + 1e-9, "refined <= brute");
    s.add("下界-开方一致", (d2b.sqrt() - q.dist(&c, probe)).abs() < 1e-12, "sqrt consistent");
    // 族四：曲线点自身距离为零
    let onp = c.at(0.37);
    let (d2c, tc, _) = q.dist2(&c, onp);
    s.add("曲上点-零", d2c < 1e-14, "d≈0");
    s.add("曲上点-参数闭合", c.at(tc).dist(onp) < 1e-6, "param roundtrip");
    // 族五：批量
    let batch = q.batch(&c, &[Pt::ZERO, probe, Pt::new(4.0, 0.0)]);
    s.add("批量-长度", batch.len() == 3, "3 results");
    s.add("批量-端点零", batch[0] < 1e-9 && batch[2] < 1e-9, "endpoints zero");
    s.add("批量-有序对照", batch[1] > 0.5, "probe far");
    // 族六：距离场剖面性质
    let prof = q.profile(&c, probe, 16);
    s.add("剖面-长度", prof.len() == 16, "16 samples");
    s.add("剖面-非负", prof.iter().all(|&d| d >= 0.0), "non-negative");
    s.add("剖面-端点", (prof[0] - probe.dist(c.at(0.0))).abs() < 1e-12, "profile[0]");
    s.add("剖面-最小", prof.iter().cloned().fold(f64::INFINITY, f64::min) <= d2b.sqrt() + 1e-9, "profile min ≈ best");
    // 族七：二次曲线专项
    let par = Curve::build(
        Deg::Quad,
        alloc::vec![Pt::new(0.0, 0.0), Pt::new(1.0, 2.0), Pt::new(2.0, 0.0)],
    )
    .expect("par");
    let (_, tp, _) = q.dist2(&par, Pt::new(1.0, 5.0));
    s.add("抛物-峰上点", (tp - 0.5).abs() < 0.01, "above peak → t=0.5");
    let (_, tp2, _) = q.dist2(&par, Pt::new(-3.0, 0.0));
    s.add("抛物-域外左", tp2 < 0.02, "far left → clamped t≈0");
    let (_, tp3, _) = q.dist2(&par, Pt::new(5.0, 0.0));
    s.add("抛物-域外右", tp3 > 0.98, "far right → clamped t≈1");
    // 族八：线性曲线最近点是垂足或端点
    let ln = Curve::build(Deg::Line, alloc::vec![Pt::new(0.0, 0.0), Pt::new(10.0, 0.0)]).expect("ln");
    let (_, tfoot, pf) = q.dist2(&ln, Pt::new(3.0, 4.0));
    s.add("线性-垂足", (tfoot - 0.3).abs() < 1e-6, "foot at t=0.3");
    s.add("线性-距离", (pf.y + 4.0).abs() < 1e-6 || (pf.dist(Pt::new(3.0, 0.0)) < 1e-6), "foot on line");
    let (_, tclamp, _) = q.dist2(&ln, Pt::new(-5.0, 1.0));
    s.add("线性-端点夹取", tclamp < 1e-6, "left clamp");
    s
}
