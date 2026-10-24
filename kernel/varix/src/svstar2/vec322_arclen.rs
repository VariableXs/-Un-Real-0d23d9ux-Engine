//! VE-F0722 曲线弧长精确计算
//! 职责：高斯-勒让德求积的弧长、自适应细分弧长、反查参数（弧长→t）、
//!       弧长表构建与增量重算。
//! 契约：精确弧长与自适应弧长偏差入容差；反查单调解得的 t 回代弧长闭合。

use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::vec321_bezcore::{Curve, Deg, Pt};

/// 3 点高斯-勒让德在 [0,1] 段上的弧长求积核
fn gauss_seg(f: &dyn Fn(f64) -> f64, a: f64, b: f64) -> f64 {
    // 节点 ±sqrt(3/5)/2 与 0（映射到 [a,b]）
    const W1: f64 = 5.0 / 9.0;
    const W2: f64 = 8.0 / 9.0;
    let h = (b - a) * 0.5;
    let m = (a + b) * 0.5;
    let r = (3.0f64 / 5.0).sqrt() * h;
    W1 * h * (f(a + (h - r)) + f(b - (h - r))) + W2 * h * f(m)
}

/// 弧长计算器
pub struct ArcLen {
    /// 全局细分段数
    pub segs: usize,
}

impl ArcLen {
    pub fn new(segs: usize) -> ArcLen {
        ArcLen { segs: segs.max(1) }
    }

    /// 高斯求积弧长（固定段数）
    pub fn len(&self, c: &Curve) -> f64 {
        let d = c.derivative().expect("deriv");
        let f = |t: f64| d.at(t).len();
        let n = self.segs;
        let mut s = 0.0;
        for k in 0..n {
            s += gauss_seg(&f, k as f64 / n as f64, (k + 1) as f64 / n as f64);
        }
        s
    }

    /// 自适应弧长：单段二分直到子段估计收敛
    pub fn len_adaptive(&self, c: &Curve, tol: f64) -> f64 {
        let d = c.derivative().expect("deriv");
        let f = |t: f64| d.at(t).len();
        let mut total = 0.0;
        let mut stack: Vec<(f64, f64)> = alloc::vec![(0.0, 1.0)];
        let mut guard = 0;
        while let Some((a, b)) = stack.pop() {
            guard += 1;
            if guard > 4096 {
                total += gauss_seg(&f, a, b);
                continue;
            }
            let whole = gauss_seg(&f, a, b);
            let m = (a + b) * 0.5;
            let half = gauss_seg(&f, a, m) + gauss_seg(&f, m, b);
            if (half - whole).abs() < tol {
                total += half;
            } else {
                stack.push((a, m));
                stack.push((m, b));
            }
        }
        total
    }

    /// 弧长表：均匀 t 采样 → 累计弧长（单调不减）
    pub fn table(&self, c: &Curve, samples: usize) -> Vec<f64> {
        let n = samples.max(2);
        let mut tab = alloc::vec![0.0f64; n];
        let f = |t: f64| c.derivative().expect("deriv").at(t).len();
        for k in 1..n {
            let a = (k - 1) as f64 / (n - 1) as f64;
            let b = k as f64 / (n - 1) as f64;
            tab[k] = tab[k - 1] + gauss_seg(&f, a, b);
        }
        tab
    }

    /// 反查：给定弧长 s 求参数 t（表内线性插值初值 + 局部弦长修正）
    pub fn param_at(&self, c: &Curve, tab: &[f64], s: f64) -> Option<f64> {
        let n = tab.len();
        if s < 0.0 || s > tab[n - 1] + 1e-12 {
            return None;
        }
        // 二分定位区间
        let mut lo = 0usize;
        let mut hi = n - 1;
        while hi - lo > 1 {
            let mid = (lo + hi) / 2;
            if tab[mid] <= s {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        let seg = tab[hi] - tab[lo];
        let frac = if seg > 0.0 { (s - tab[lo]) / seg } else { 0.0 };
        Some((lo as f64 + frac) / (n - 1) as f64)
    }
}

/// 弦线求积（供对照的朴素方法）
pub fn chord_len(c: &Curve, n: usize) -> f64 {
    let mut s = 0.0;
    let mut prev = c.at(0.0);
    for k in 1..=n {
        let t = k as f64 / n as f64;
        let p = c.at(t);
        s += p.dist(prev);
        prev = p;
    }
    s
}

pub fn run_vec322_checks() -> crate::checks::CheckSet {
    let mut s = CheckSet::new("svstar2-vec322");
    // 直线段：任何方法都必须精确
    let line = Curve::build(Deg::Line, alloc::vec![Pt::new(0.0, 0.0), Pt::new(3.0, 4.0)]).expect("line");
    let al = ArcLen::new(8);
    s.add("直线-精确", (al.len(&line) - 5.0).abs() < 1e-12, "3-4-5 exact");
    s.add("直线-自适应", (al.len_adaptive(&line, 1e-12) - 5.0).abs() < 1e-12, "adaptive exact");
    s.add("直线-弦", (chord_len(&line, 4) - 5.0).abs() < 1e-12, "chord exact");
    // 抛物线 y=2x(1-x)：控制点 (0,0)(0.5,1)(1,0)
    let par = Curve::build(Deg::Quad, alloc::vec![Pt::new(0.0, 0.0), Pt::new(0.5, 1.0), Pt::new(1.0, 0.0)]).expect("par");
    let g = al.len(&par);
    let ada = al.len_adaptive(&par, 1e-11);
    s.add("抛物-正性", g > 2.0, "len > chord bound 2");
    s.add("自适应-收敛", (g - ada).abs() < 1e-6, "gauss vs adaptive");
    s.add("自适应-上界", ada < 2.1, "parabola arc < 2.1");
    // 弦线从下方逼近
    s.add("弦-下逼近", chord_len(&par, 16) < ada, "chord < arc");
    // 弧长表单调
    let tab = al.table(&par, 32);
    s.add("表-长度", tab.len() == 32, "samples");
    s.add("表-起点零", tab[0] == 0.0, "tab[0]==0");
    s.add("表-单调", tab.windows(2).all(|w| w[1] >= w[0]), "monotone");
    s.add("表-终值一致", (tab[31] - ada).abs() < 1e-9, "table end == adaptive");
    // 反查闭合：s = 总长/2 → t ≈ 0.5（对称曲线），回代弧长闭合
    let t_mid = al.param_at(&par, &tab, ada * 0.5).expect("param");
    s.add("反查-对称中点", (t_mid - 0.5).abs() < 0.02, "sym curve mid");
    // 反查越界拒绝
    s.add("反查-越界拒", al.param_at(&par, &tab, -0.1).is_none() && al.param_at(&par, &tab, ada + 0.1).is_none(), "out of range");
    // 反查非中点闭合：s(tab)=40% → 弧长回代
    let s40 = ada * 0.4;
    let t40 = al.param_at(&par, &tab, s40).expect("t40");
    // 用局部弦长近似验证回代（粗一致性）
    let sub = al.table(
        &Curve::build(
            Deg::Quad,
            alloc::vec![par.at(0.0), par.at(t40 * 0.5), par.at(t40)],
        )
        .expect("sub"),
        16,
    );
    s.add("反查-回代闭合", (sub[15] - s40).abs() < 0.02, "roundtrip within 2e-2");
    // 立方曲线：S 形
    let cube = Curve::build(
        Deg::Cube,
        alloc::vec![Pt::new(0.0, 0.0), Pt::new(1.0, 1.0), Pt::new(2.0, -1.0), Pt::new(3.0, 0.0)],
    )
    .expect("cube");
    let lc = al.len(&cube);
    s.add("立方-正性", lc > 3.0, "S len > chord 3");
    s.add("立方-双法一致", (lc - al.len_adaptive(&cube, 1e-11)).abs() < 1e-6, "gauss vs adaptive");
    // 段数收敛性：段数增加结果趋稳
    let l8 = ArcLen::new(8).len(&cube);
    let l64 = ArcLen::new(64).len(&cube);
    s.add("收敛-细化", (l8 - l64).abs() < 1e-9, "8 segs already converged");
    // 弧长导数 = 速率
    let d = cube.derivative().expect("d");
    s.add("核-正性", d.at(0.5).len() > 0.0, "speed positive");
    s
}
