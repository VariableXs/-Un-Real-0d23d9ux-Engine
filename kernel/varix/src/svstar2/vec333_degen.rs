//! VE-F0733 曲线数值稳定性（退化检测）
//! 职责：退化分类（零长边/共线/重叠控制点/控制点漂移）、重参数化建议、
//!       条件数估计（控制点多边形面积比）、鲁棒求值（Casteljau 回退）。
//! 契约：正常曲线不误报退化；已知退化样本全部分类正确。

use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::vec321_bezcore::{Curve, Deg, Pt};

/// 退化类型
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Degen {
    None,
    /// 控制点全重合（点曲线）
    Point,
    /// 全部控制点共线（可降阶为折线）
    Collinear,
    /// 相邻控制点重合
    DupCtrl,
    /// 控制点多边形自交（缠绕异常）
    SelfWrap,
}

/// 退化检测
pub fn detect(c: &Curve) -> Degen {
    let n = c.ctrl.len();
    // 全重合
    if c.ctrl.iter().all(|p| p.dist(c.ctrl[0]) < 1e-12) {
        return Degen::Point;
    }
    // 相邻重合
    for w in c.ctrl.windows(2) {
        if w[0].dist(w[1]) < 1e-12 {
            return Degen::DupCtrl;
        }
    }
    // 共线：所有点对基线 (c0 → c_{n-1}) 的叉积为零
    let base = c.ctrl[n - 1].sub(c.ctrl[0]);
    if base.len2() > 1e-24 {
        let collinear = c.ctrl.iter().all(|p| p.sub(c.ctrl[0]).cross(base).abs() < 1e-9);
        if collinear {
            return Degen::Collinear;
        }
    }
    // 控制点多边形有向面积（鞋带公式）为 0 → 共线（重复检测）或自交近似
    let mut area2 = 0.0;
    for i in 0..n {
        let j = (i + 1) % n;
        area2 += c.ctrl[i].cross(c.ctrl[j]);
    }
    if area2.abs() < 1e-12 {
        return Degen::Collinear;
    }
    Degen::None
}

/// 条件数估计：控制点多边形面积的平方 / 首尾距的四次方（越小越病态）
pub fn condition(c: &Curve) -> f64 {
    let n = c.ctrl.len();
    let base = c.ctrl[n - 1].dist(c.ctrl[0]);
    if base < 1e-12 {
        return f64::INFINITY;
    }
    let mut area2 = 0.0f64;
    for i in 0..n {
        let j = (i + 1) % n;
        area2 += c.ctrl[i].cross(c.ctrl[j]);
    }
    (area2 * area2) / (base * base * base * base)
}

/// 鲁棒求值：Bernstein 与 Casteljau 双路径，不一致超阈值回退 Casteljau
pub fn robust_eval(c: &Curve, t: f64) -> Pt {
    let b = c.at(t);
    let ca = c.at_casteljau(t);
    if b.dist(ca) < 1e-9 {
        b
    } else {
        ca
    }
}

/// 重参数化建议：弧长不均匀度（最大段弦长/最小段弦长）
pub fn reparam_hint(c: &Curve, samples: usize) -> f64 {
    let n = samples.max(4);
    let mut lens: Vec<f64> = alloc::vec![];
    let mut prev = c.at(0.0);
    for k in 1..=n {
        let t = k as f64 / n as f64;
        let p = c.at(t);
        lens.push(p.dist(prev));
        prev = p;
    }
    let lmin = lens.iter().cloned().fold(f64::INFINITY, f64::min);
    let lmax = lens.iter().cloned().fold(0.0f64, f64::max);
    if lmin < 1e-15 {
        f64::INFINITY
    } else {
        lmax / lmin
    }
}

pub fn run_vec333_checks() -> crate::checks::CheckSet {
    let mut s = CheckSet::new("svstar2-vec333");
    // 族一：正常曲线不误报
    let ok = Curve::build(
        Deg::Cube,
        alloc::vec![Pt::new(0.0, 0.0), Pt::new(1.0, 3.0), Pt::new(3.0, 3.0), Pt::new(4.0, 0.0)],
    )
    .expect("ok");
    s.add("正常-无退化", detect(&ok) == Degen::None, "clean curve");
    // 族二：点曲线
    let dot = Curve::build(Deg::Cube, alloc::vec![Pt::new(2.0, 3.0); 4]).expect("dot");
    s.add("点曲线-检出", detect(&dot) == Degen::Point, "point degeneracy");
    s.add("点曲线-条件无穷", condition(&dot).is_infinite(), "inf condition");
    // 族三：共线
    let line3 = Curve::build(
        Deg::Cube,
        alloc::vec![Pt::new(0.0, 0.0), Pt::new(1.0, 1.0), Pt::new(2.0, 2.0), Pt::new(3.0, 3.0)],
    )
    .expect("line3");
    s.add("共线-检出", detect(&line3) == Degen::Collinear, "collinear");
    // 抛物线（控制点不共线）正常
    let par = Curve::build(
        Deg::Quad,
        alloc::vec![Pt::new(0.0, 0.0), Pt::new(1.0, 2.0), Pt::new(2.0, 0.0)],
    )
    .expect("par");
    s.add("抛物-无退化", detect(&par) == Degen::None, "parabola clean");
    // 族四：相邻重合
    let dup = Curve::build(
        Deg::Cube,
        alloc::vec![Pt::new(0.0, 0.0), Pt::new(1.0, 1.0), Pt::new(1.0, 1.0), Pt::new(4.0, 0.0)],
    )
    .expect("dup");
    s.add("重合-检出", detect(&dup) == Degen::DupCtrl, "dup ctrl");
    // 族五：条件数
    let cond_ok = condition(&ok);
    let cond_par = condition(&par);
    s.add("条件-有限", cond_ok.is_finite() && cond_par.is_finite(), "finite");
    // 好曲线条件数远小于 1（面积大、跨度大）
    s.add("条件-良好曲线小", cond_ok < 1.0, "well-conditioned");
    // 接近共线的病态曲线条件数更小（面积≈0）
    let near_line = Curve::build(
        Deg::Cube,
        alloc::vec![Pt::new(0.0, 0.0), Pt::new(1.0, 0.001), Pt::new(2.0, -0.001), Pt::new(3.0, 0.0)],
    )
    .expect("near");
    s.add("病态-条件极小", condition(&near_line) < cond_ok / 100.0, "ill-conditioned tiny area");
    // 共线曲线条件数为 0
    s.add("共线-条件零", condition(&line3) == 0.0, "zero area");
    // 族六：鲁棒求值
    for t in [0.0, 0.25, 0.5, 0.75, 1.0] {
        let rb = robust_eval(&ok, t);
        s.add("鲁棒-正常路径", rb.dist(ok.at(t)) < 1e-12, "normal path");
    }
    // 病态大 t 也不崩
    let rb = robust_eval(&near_line, 0.5);
    s.add("鲁棒-病态可算", rb.x.is_finite() && rb.y.is_finite(), "finite result");
    // 族七：重参数化提示
    let hint_line = reparam_hint(&ln_helper(), 64);
    s.add("提示-直线均匀", hint_line < 1.0 + 1e-9, "line uniform");
    let hint_ok = reparam_hint(&ok, 64);
    s.add("提示-正常有限", hint_ok.is_finite() && hint_ok > 0.0, "finite hint");
    // 严重不均匀的曲线（控制点挤一端）
    let skew = Curve::build(
        Deg::Cube,
        alloc::vec![Pt::new(0.0, 0.0), Pt::new(0.001, 0.001), Pt::new(0.002, 3.0), Pt::new(10.0, 0.0)],
    )
    .expect("skew");
    let hint_skew = reparam_hint(&skew, 64);
    s.add("提示-偏斜更大", hint_skew > hint_ok, "skew worse");
    // 退化检测在鲁棒求值路径上的兜底：点曲线求值恒返回该点
    s.add("点曲线-求值", robust_eval(&dot, 0.7).dist(Pt::new(2.0, 3.0)) < 1e-12, "dot eval");
    s
}

fn ln_helper() -> Curve {
    Curve::build(Deg::Line, alloc::vec![Pt::ZERO, Pt::new(4.0, 4.0)]).expect("ln")
}
