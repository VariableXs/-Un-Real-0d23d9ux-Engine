//! VE-F0723 曲线分割（de Casteljau 分割）
//! 职责：任意 t 处把曲线一分为二、双侧子曲线控制点显式给出、
//!       多段链式分割、分割可逆性验证（子曲线升阶重组原曲线）。
//! 契约：子曲线在分割点的值一致；两子曲线并的轨迹与原曲线逐点重合。

use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::vec321_bezcore::{Curve, Deg, Pt};

/// 在 t 处分割，返回（左半，右半）
pub fn split(c: &Curve, t: f64) -> (Curve, Curve) {
    let n = c.ctrl.len();
    let mut tmp: Vec<Pt> = c.ctrl.clone();
    // 左半的第 i 层首点 / 右半的第 i 层尾点
    let mut left: Vec<Pt> = alloc::vec![tmp[0]];
    let mut right: Vec<Pt> = alloc::vec![tmp[n - 1]];
    for r in 1..n {
        for i in 0..(n - r) {
            tmp[i] = tmp[i].lerp(tmp[i + 1], t);
        }
        left.push(tmp[0]);
        right.push(tmp[n - 1 - r]);
    }
    right.reverse();
    (
        Curve::build(c.deg, left).expect("left"),
        Curve::build(c.deg, right).expect("right"),
    )
}

/// 链式分割：给定切割参数升序序列，返回各段
pub fn split_chain(c: &Curve, cuts: &[f64]) -> Vec<Curve> {
    let mut segs = alloc::vec![c.clone()];
    let mut done: Vec<f64> = alloc::vec![];
    for &t in cuts {
        // 把 t 映射到最后一段的局部参数
        let last = segs.last().expect("seg").clone();
        let span = 1.0 - done.last().copied().unwrap_or(0.0);
        let local = if span > 0.0 { (t - done.last().copied().unwrap_or(0.0)) / span } else { 1.0 };
        let (a, b) = split(&last, local.clamp(0.0, 1.0));
        segs.pop();
        segs.push(a);
        segs.push(b);
        done.push(t);
    }
    segs
}

/// 两子曲线并的轨迹与原曲线逐点最大偏差
pub fn split_trace_error(c: &Curve, t: f64, samples: usize) -> f64 {
    let (l, r) = split(c, t);
    let n = samples.max(2);
    let mut max_d = 0.0f64;
    for k in 0..n {
        let u = k as f64 / (n - 1) as f64;
        let p;
        if u <= t && t > 0.0 {
            p = l.at(u / t);
        } else {
            let den = 1.0 - t;
            p = if den > 0.0 { r.at((u - t) / den) } else { r.at(0.0) };
        }
        max_d = max_d.max(p.dist(c.at(u)));
    }
    max_d
}

pub fn run_vec323_checks() -> crate::checks::CheckSet {
    let mut s = CheckSet::new("svstar2-vec323");
    let c = Curve::build(
        Deg::Cube,
        alloc::vec![Pt::new(0.0, 0.0), Pt::new(1.0, 3.0), Pt::new(3.0, 3.0), Pt::new(4.0, 0.0)],
    )
    .expect("cube");
    // 族一：端点与分割点性质
    let (l, r) = split(&c, 0.5);
    s.add("分割-控制点数", l.ctrl.len() == 4 && r.ctrl.len() == 4, "same degree");
    s.add("分割-左端点", l.at(0.0) == c.ctrl[0], "left start");
    s.add("分割-右端点", r.at(1.0) == c.ctrl[3], "right end");
    s.add("分割-接合点", l.at(1.0).dist(r.at(0.0)) < 1e-12, "joint continuous");
    s.add("分割-接合值", l.at(1.0).dist(c.at(0.5)) < 1e-12, "joint value");
    // 族二：轨迹重合
    s.add("轨迹-半分", split_trace_error(&c, 0.5, 64) < 1e-9, "trace match t=0.5");
    s.add("轨迹-三分位", split_trace_error(&c, 1.0 / 3.0, 64) < 1e-9, "trace match t=1/3");
    s.add("轨迹-偏分位", split_trace_error(&c, 0.87, 64) < 1e-9, "trace match t=0.87");
    // 族三：端点分割退化
    let (l0, r0) = split(&c, 0.0);
    s.add("退化-t0左", l0.at(0.0) == c.at(0.0), "t=0 left is point");
    s.add("退化-t0右", r0.trace_eq(&c), "t=0 right == whole");
    let (l1, r1) = split(&c, 1.0);
    s.add("退化-t1左", l1.trace_eq(&c), "t=1 left == whole");
    s.add("退化-t1右", r1.at(1.0) == c.at(1.0), "t=1 right is point");
    // 族四：对称性——分割两次回到原曲线（先 0.5 再右半再 0.5）
    let (lm, rm) = split(&c, 0.5);
    let (_, rmm) = split(&rm, 0.5);
    let mut max_d = 0.0f64;
    for k in 0..=16 {
        let t = k as f64 / 16.0;
        max_d = max_d.max(rmm.at(t).dist(c.at(0.5 + 0.5 * t)));
    }
    s.add("复合-右半右半", max_d < 1e-9, "nested split");
    let (lmm, _) = split(&lm, 0.5);
    let mut max_d2 = 0.0f64;
    for k in 0..=16 {
        let t = k as f64 / 16.0;
        max_d2 = max_d2.max(lmm.at(t).dist(c.at(0.5 * t)));
    }
    s.add("复合-左半左半", max_d2 < 1e-9, "nested left");
    // 族五：链式分割
    let chain = split_chain(&c, &[0.25, 0.5, 0.75]);
    s.add("链-段数", chain.len() == 4, "3 cuts → 4 segs");
    let mut joint_err = 0.0f64;
    for w in chain.windows(2) {
        joint_err = joint_err.max(w[0].at(1.0).dist(w[1].at(0.0)));
    }
    s.add("链-接合连续", joint_err < 1e-9, "chain C0");
    // 链整体覆盖：采样映射回原曲线
    let knots: Vec<f64> = alloc::vec![0.0, 0.25, 0.5, 0.75, 1.0];
    let mut cover_err = 0.0f64;
    for (i, seg) in chain.iter().enumerate() {
        for k in 0..=8 {
            let t = k as f64 / 8.0;
            let u = knots[i] + (knots[i + 1] - knots[i]) * t;
            cover_err = cover_err.max(seg.at(t).dist(c.at(u)));
        }
    }
    s.add("链-覆盖一致", cover_err < 1e-9, "chain covers whole");
    // 族六：二次与线性
    let q = Curve::build(Deg::Quad, alloc::vec![Pt::new(0.0, 0.0), Pt::new(2.0, 2.0), Pt::new(4.0, 0.0)]).expect("q");
    s.add("二次-轨迹", split_trace_error(&q, 0.4, 32) < 1e-9, "quad trace");
    let ln = Curve::build(Deg::Line, alloc::vec![Pt::new(0.0, 0.0), Pt::new(4.0, 4.0)]).expect("ln");
    let (ll, lr) = split(&ln, 0.5);
    s.add("线性-中点", ll.at(1.0).dist(Pt::new(2.0, 2.0)) < 1e-12, "line mid");
    s.add("线性-轨迹", split_trace_error(&ln, 0.5, 16) < 1e-12, "line trace");
    // 族七：平移不变性——整体平移后分割结果同步平移
    let shift = Pt::new(10.0, -5.0);
    let c2 = Curve::build(
        Deg::Cube,
        c.ctrl.iter().map(|p| p.add(shift)).collect(),
    )
    .expect("c2");
    let (l2, _) = split(&c2, 0.3);
    let (l1s, _) = split(&c, 0.3);
    s.add("平移-同步", l2.at(0.7).dist(l1s.at(0.7).add(shift)) < 1e-12, "translate invariance");
    s
}

impl Curve {
    /// 与另一曲线逐点一致（同采样数）
    fn trace_eq(&self, o: &Curve) -> bool {
        for k in 0..=16 {
            let t = k as f64 / 16.0;
            if self.at(t).dist(o.at(t)) > 1e-9 {
                return false;
            }
        }
        true
    }
}
