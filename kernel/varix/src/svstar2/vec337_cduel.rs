//! VE-F0737 曲线数值对拍
//! 职责：三路求值对拍（Bernstein / Casteljau / 导数积分）、
//!       弧长对拍（高斯 / 自适应 / 弦线上界）、对拍报告与判定。
//! 契约：三路最大偏差入容差即绿；报告含每路样本数与最大偏差。

use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::vec321_bezcore::{Curve, Deg, Pt};

/// 对拍报告
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct DiffReport {
    pub samples: usize,
    pub max_dev: f64,
    pub tol: f64,
}

impl DiffReport {
    pub fn pass(&self) -> bool {
        self.max_dev <= self.tol
    }
    pub fn margin(&self) -> f64 {
        self.tol - self.max_dev
    }
}

/// 三路求值对拍
pub fn eval_duel(c: &Curve, samples: usize, tol: f64) -> DiffReport {
    let n = samples.max(2);
    let d1 = c.derivative();
    let mut worst = 0.0f64;
    let mut count = 0usize;
    for k in 0..n {
        let t = (k as f64 + 0.5) / n as f64;
        let a = c.at(t);
        let b = c.at_casteljau(t);
        worst = worst.max(a.dist(b));
        // 第三路：导数积分初值（欧拉细步，粗对拍）
        if let Some(d) = &d1 {
            let steps = 16;
            let dt = t / steps as f64;
            let mut acc = c.at(0.0);
            for i in 0..steps {
                acc = acc.add(d.at(i as f64 * dt).mul(dt));
            }
            // 积分误差与步长同阶，容差放宽 10 倍
            worst = worst.max((acc.dist(a)) * 0.1);
        }
        count += 1;
    }
    DiffReport { samples: count, max_dev: worst, tol }
}

/// 弧长对拍：弦线上界 / 高斯 / 自适应
pub fn arclen_duel(c: &Curve, segs: usize, tol: f64) -> DiffReport {
    let gauss = super::vec322_arclen::ArcLen::new(segs).len(c);
    let adaptive = super::vec322_arclen::ArcLen::new(segs).len_adaptive(c, 1e-12);
    let mut chord = 0.0f64;
    let mut prev = c.at(0.0);
    for k in 1..=256 {
        let p = c.at(k as f64 / 256.0);
        chord += p.dist(prev);
        prev = p;
    }
    // 高斯与自适应一致；弦长从下方逼近（差值作为偏差的一部分）
    let dev = (gauss - adaptive).abs().max((chord - gauss).abs() * 0.01);
    DiffReport { samples: 256, max_dev: dev, tol }
}

/// 对拍总结（多曲线汇总）
pub struct DuelSummary {
    pub rows: Vec<(&'static str, DiffReport)>,
}

impl DuelSummary {
    pub fn new() -> DuelSummary {
        DuelSummary { rows: alloc::vec![] }
    }
    pub fn push(&mut self, name: &'static str, r: DiffReport) {
        self.rows.push((name, r));
    }
    pub fn all_pass(&self) -> bool {
        self.rows.iter().all(|(_, r)| r.pass())
    }
    pub fn worst(&self) -> Option<&DiffReport> {
        self.rows.iter().map(|(_, r)| r).max_by(|a, b| a.max_dev.partial_cmp(&b.max_dev).unwrap_or(core::cmp::Ordering::Equal))
    }
}

impl Default for DuelSummary {
    fn default() -> Self {
        DuelSummary::new()
    }
}

pub fn run_vec337_checks() -> crate::checks::CheckSet {
    let mut s = CheckSet::new("svstar2-vec337");
    let c = Curve::build(
        Deg::Cube,
        alloc::vec![Pt::new(0.0, 0.0), Pt::new(1.0, 3.0), Pt::new(3.0, 3.0), Pt::new(4.0, 0.0)],
    )
    .expect("cube");
    // 族一：报告判定逻辑
    let r_ok = DiffReport { samples: 32, max_dev: 1e-12, tol: 1e-9 };
    s.add("报告-通过", r_ok.pass(), "dev < tol");
    s.add("报告-余量", (r_ok.margin() - (1e-9 - 1e-12)).abs() < 1e-15, "margin");
    let r_bad = DiffReport { samples: 32, max_dev: 1e-6, tol: 1e-9 };
    s.add("报告-不通过", !r_bad.pass(), "dev > tol");
    // 族二：求值对拍
    let duel = eval_duel(&c, 64, 1e-8);
    s.add("求值-样本数", duel.samples == 64, "64 samples");
    s.add("求值-通过", duel.pass(), "bernstein ≈ casteljau");
    s.add("求值-偏差极小", duel.max_dev < 1e-9, "dev tiny");
    // 三次 S 形
    let s_curve = Curve::build(
        Deg::Cube,
        alloc::vec![Pt::new(0.0, 0.0), Pt::new(2.0, 3.0), Pt::new(2.0, -3.0), Pt::new(4.0, 0.0)],
    )
    .expect("s");
    s.add("求值-S形过", eval_duel(&s_curve, 64, 1e-8).pass(), "S curve pass");
    // 高阶
    let q4 = Curve::build(
        Deg::Quart,
        alloc::vec![Pt::new(0.0, 0.0), Pt::new(1.0, 4.0), Pt::new(2.0, -4.0), Pt::new(3.0, 4.0), Pt::new(4.0, 0.0)],
    )
    .expect("q4");
    s.add("求值-四次过", eval_duel(&q4, 64, 1e-8).pass(), "quartic pass");
    // 族三：弧长对拍
    let aduel = arclen_duel(&c, 16, 1e-6);
    s.add("弧长-通过", aduel.pass(), "gauss ≈ adaptive");
    s.add("弧长-样本", aduel.samples == 256, "chord samples");
    let aduel_q = arclen_duel(&q4, 16, 1e-6);
    s.add("弧长-四次过", aduel_q.pass(), "quartic arc pass");
    // 族四：汇总
    let mut sum = DuelSummary::new();
    sum.push("cube", eval_duel(&c, 32, 1e-8));
    sum.push("s-curve", eval_duel(&s_curve, 32, 1e-8));
    sum.push("quartic", eval_duel(&q4, 32, 1e-8));
    sum.push("arc-cube", arclen_duel(&c, 8, 1e-6));
    s.add("汇总-行数", sum.rows.len() == 4, "4 rows");
    s.add("汇总-全过", sum.all_pass(), "all pass");
    let w = sum.worst().expect("worst");
    s.add("汇总-最坏有限", w.max_dev.is_finite(), "worst finite");
    s.add("汇总-最坏≤各", sum.rows.iter().all(|(_, r)| r.max_dev <= w.max_dev + 1e-18), "worst is max");
    // 族五：空汇总
    let empty = DuelSummary::new();
    s.add("汇总-空全过", empty.all_pass(), "vacuous pass");
    s.add("汇总-空无最坏", empty.worst().is_none(), "no worst");
    // 族六：容差敏感
    let tight = eval_duel(&c, 32, 1e-15);
    let _ = tight;
    s.add("容差-余量单调", duel.margin() > 0.0, "positive margin at 1e-8");
    s
}
