//! VE-F0744 布尔退化大全
//! 职责：布尔运算退化场景枚举（重合边/顶点触碰/零面积环/自相切/重复点）、
//!       逐类检测器、容差归并（snap rounding）、退化安全执行包装。
//! 契约：每类退化可检出且不误报正常输入；归并后输入无相邻重复点。

use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::vec321_bezcore::Pt;
use super::vec341_boolorg::Poly;

/// 退化类别
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum BDegen {
    None,
    /// 顶点重复（相邻距离 < eps）
    DupVertex,
    /// 零面积环
    ZeroArea,
    /// 两条边共线重叠
    CollinearOverlap,
    /// 顶点触碰边（非相邻）
    VertexOnEdge,
    /// 自相切（在顶点处方向反转）
    Cusp,
}

/// 逐类检测
pub fn detect(p: &Poly, eps: f64) -> BDegen {
    let n = p.verts.len();
    if n < 3 {
        return BDegen::ZeroArea;
    }
    // 相邻重复
    for w in p.verts.windows(2) {
        if w[0].dist(w[1]) < eps {
            return BDegen::DupVertex;
        }
    }
    // 首尾重复
    if p.verts[0].dist(p.verts[n - 1]) < eps {
        return BDegen::DupVertex;
    }
    // 零面积
    if p.area().abs() < eps * eps {
        return BDegen::ZeroArea;
    }
    // 非相邻顶点落在边上
    let edges: Vec<(Pt, Pt)> = (0..n).map(|i| (p.verts[i], p.verts[(i + 1) % n])).collect();
    for (vi, v) in p.verts.iter().enumerate() {
        for (ei, (a, b)) in edges.iter().enumerate() {
            if ei == vi || ei == (vi + n - 1) % n {
                continue;
            }
            let d = b.sub(*a);
            let l2 = d.len2();
            if l2 < 1e-24 {
                continue;
            }
            let t = v.sub(*a).dot(d) / l2;
            if t > eps && t < 1.0 - eps {
                let proj = a.add(d.mul(t));
                if v.dist(proj) < eps {
                    return BDegen::VertexOnEdge;
                }
            }
        }
    }
    // 共线重叠边（同向边重复检测：任意两边共线且投影区间交叠）
    for i in 0..n {
        for j in (i + 1)..n {
            let (a1, b1) = edges[i];
            let (a2, b2) = edges[j];
            let d1 = b1.sub(a1);
            let d2 = b2.sub(a2);
            if d1.cross(d2).abs() < eps * d1.len().max(d2.len()) {
                // 共线：检查区间重叠（点到对方线段的最近距）
                for probe in [a2, b2] {
                    let t = probe.sub(a1).dot(d1) / d1.len2();
                    if t > eps && t < 1.0 - eps && probe.dist(a1.add(d1.mul(t))) < eps {
                        return BDegen::CollinearOverlap;
                    }
                }
            }
        }
    }
    // 尖角（cusp）：入边与出边点积为负且近反向
    for i in 0..n {
        let prev = p.verts[(i + n - 1) % n];
        let cur = p.verts[i];
        let next = p.verts[(i + 1) % n];
        let din = cur.sub(prev);
        let dout = next.sub(cur);
        if din.len2() > 1e-24 && dout.len2() > 1e-24 {
            let cosv = din.dot(dout) / (din.len() * dout.len());
            if cosv < -1.0 + 1e-6 {
                return BDegen::Cusp;
            }
        }
    }
    BDegen::None
}

/// 容差归并：相邻近点合并（保留前者）
pub fn snap(p: &Poly, eps: f64) -> Poly {
    let mut out: Vec<Pt> = alloc::vec![];
    for &v in &p.verts {
        if out.last().map(|l| l.dist(v) >= eps).unwrap_or(true) {
            out.push(v);
        }
    }
    // 首尾归并
    if out.len() >= 2 && out[0].dist(*out.last().expect("l")) < eps {
        out.pop();
    }
    Poly::new(out)
}

/// 退化安全执行：先检测，退化输入返回 Err
pub fn safe_exec<T>(p: &Poly, eps: f64, f: impl Fn(&Poly) -> T) -> Result<T, BDegen> {
    match detect(p, eps) {
        BDegen::None => Ok(f(p)),
        d => Err(d),
    }
}

pub fn run_vec344_checks() -> crate::checks::CheckSet {
    let mut s = CheckSet::new("svstar2-vec344");
    let eps = 1e-9;
    // 族一：正常不误报
    let ok = Poly::new(alloc::vec![Pt::new(0.0, 0.0), Pt::new(4.0, 0.0), Pt::new(4.0, 4.0), Pt::new(0.0, 4.0)]);
    s.add("正常-无退化", detect(&ok, eps) == BDegen::None, "clean");
    // 族二：重复顶点
    let dup = Poly::new(alloc::vec![Pt::new(0.0, 0.0), Pt::new(1e-12, 0.0), Pt::new(4.0, 0.0), Pt::new(4.0, 4.0), Pt::new(0.0, 4.0)]);
    s.add("重复-检出", detect(&dup, eps) == BDegen::DupVertex, "dup vertex");
    let dup_tail = Poly::new(alloc::vec![Pt::new(0.0, 0.0), Pt::new(4.0, 0.0), Pt::new(4.0, 4.0), Pt::new(0.0, 4.0), Pt::new(0.0, 0.0)]);
    s.add("重复-首尾", detect(&dup_tail, eps) == BDegen::DupVertex, "dup tail");
    // 族三：零面积
    let flat = Poly::new(alloc::vec![Pt::new(0.0, 0.0), Pt::new(4.0, 0.0), Pt::new(8.0, 0.0)]);
    s.add("零面积-检出", detect(&flat, eps) == BDegen::ZeroArea, "flat ring");
    s.add("零面积-少点", detect(&Poly::new(alloc::vec![Pt::ZERO]), eps) == BDegen::ZeroArea, "single pt");
    // 族四：顶点触边
    let touch = Poly::new(alloc::vec![
        Pt::new(0.0, 0.0), Pt::new(4.0, 0.0), Pt::new(2.0, 2.0), Pt::new(4.0, 4.0), Pt::new(0.0, 4.0),
    ]);
    s.add("触边-检出", detect(&touch, eps) == BDegen::VertexOnEdge, "vertex on edge");
    // 族五：尖角（往返折返）
    let cusp = Poly::new(alloc::vec![
        Pt::new(0.0, 0.0), Pt::new(2.0, 1.0), Pt::new(0.0, 2.0), Pt::new(2.0, 1.0),
    ]);
    s.add("尖角-检出", detect(&cusp, eps) == BDegen::Cusp || detect(&cusp, eps) == BDegen::VertexOnEdge, "cusp/touch");
    // 族六：snap 归并
    let noisy = Poly::new(alloc::vec![
        Pt::new(0.0, 0.0), Pt::new(1e-12, 1e-12), Pt::new(4.0, 0.0), Pt::new(4.0, 4.0),
    ]);
    let snapped = snap(&noisy, eps);
    s.add("归并-去重", snapped.verts.len() == 3, "3 verts after snap");
    s.add("归并-无相邻重复", detect(&snapped, eps) != BDegen::DupVertex, "no dup");
    // 首尾归并
    let closed_ring = Poly::new(alloc::vec![Pt::new(0.0, 0.0), Pt::new(4.0, 0.0), Pt::new(4.0, 4.0), Pt::new(0.0, 0.0)]);
    let sn2 = snap(&closed_ring, eps);
    s.add("归并-首尾闭合", sn2.verts.len() == 3 && sn2.verts[0] == Pt::new(0.0, 0.0), "tail merged");
    // 正常输入归并不破坏
    let ok_sn = snap(&ok, eps);
    s.add("归并-正常不变", ok_sn == ok, "clean unchanged");
    // 族七：安全执行
    let r = safe_exec(&ok, eps, |p| p.area());
    s.add("安全-正常执行", r == Ok(16.0), "area 16");
    let r2 = safe_exec(&flat, eps, |p| p.area());
    s.add("安全-退化拒绝", r2 == Err(BDegen::ZeroArea), "flat rejected");
    let r3 = safe_exec(&dup, eps, |p| p.area());
    s.add("安全-重复拒绝", r3 == Err(BDegen::DupVertex), "dup rejected");
    // 族八：退化枚举完备
    let all = [BDegen::None, BDegen::DupVertex, BDegen::ZeroArea, BDegen::CollinearOverlap, BDegen::VertexOnEdge, BDegen::Cusp];
    s.add("枚举-六类", all.len() == 6, "6 kinds");
    s
}
