//! VE-F0747 布尔自交输入处理
//! 职责：自交多边形的检测（折线段互交）、自交点分裂预处理、
//!       处理后环绕数合法性（无悬空边）、规范化输出。
//! 契约：简单多边形不误报；自交多边形检出后分裂点全部入顶点表。

use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::vec321_bezcore::Pt;
use super::vec341_boolorg::Poly;

/// 线段交点（参数域内）
fn seg_isect(a1: Pt, b1: Pt, a2: Pt, b2: Pt) -> Option<(f64, f64, Pt)> {
    let d1 = b1.sub(a1);
    let d2 = b2.sub(a2);
    let den = d1.cross(d2);
    if den.abs() < 1e-15 {
        return None;
    }
    let t = a2.sub(a1).cross(d2) / den;
    let u = a2.sub(a1).cross(d1) / den;
    if t > 1e-9 && t < 1.0 - 1e-9 && u > 1e-9 && u < 1.0 - 1e-9 {
        Some((t, u, a1.add(d1.mul(t))))
    } else {
        None
    }
}

/// 自交检测：返回交点列表（排除相邻边共享端点）
pub fn self_isects(p: &Poly) -> Vec<Pt> {
    let n = p.verts.len();
    let mut out: Vec<Pt> = alloc::vec![];
    for i in 0..n {
        for j in (i + 1)..n {
            // 相邻边跳过（含首尾相邻）
            if j == i + 1 || (i == 0 && j == n - 1) {
                continue;
            }
            if let Some((_, _, q)) = seg_isect(p.verts[i], p.verts[(i + 1) % n], p.verts[j], p.verts[(j + 1) % n]) {
                // 去重
                if !out.iter().any(|o| o.dist(q) < 1e-9) {
                    out.push(q);
                }
            }
        }
    }
    out
}

/// 简单性判定
pub fn is_simple(p: &Poly) -> bool {
    self_isects(p).is_empty()
}

/// 分裂预处理：把自交点按 t 序插入边中（保序，产出顶点数增加的多边形）
pub fn split_at_isects(p: &Poly) -> (Poly, usize) {
    let isects = self_isects(p);
    let n = p.verts.len();
    let mut verts: Vec<Pt> = alloc::vec![];
    for i in 0..n {
        verts.push(p.verts[i]);
        let a = p.verts[i];
        let b = p.verts[(i + 1) % n];
        let mut ts: Vec<(f64, Pt)> = alloc::vec![];
        for j in (i + 1)..n {
            if j == i + 1 || (i == 0 && j == n - 1) {
                continue;
            }
            if let Some((t, _, q)) = seg_isect(a, b, p.verts[j], p.verts[(j + 1) % n]) {
                ts.push((t, q));
            }
        }
        ts.sort_by(|x, y| x.0.partial_cmp(&y.0).unwrap_or(core::cmp::Ordering::Equal));
        for (_, q) in ts {
            if verts.last().map(|l| l.dist(q) > 1e-9).unwrap_or(true) {
                verts.push(q);
            }
        }
    }
    (Poly::new(verts), isects.len())
}

/// 规范化输出：分裂后 CCW
pub fn normalize(p: &Poly) -> Poly {
    p.normalize_ccw()
}

pub fn run_vec347_checks() -> crate::checks::CheckSet {
    let mut s = CheckSet::new("svstar2-vec347");
    // 族一：简单多边形
    let square = Poly::new(alloc::vec![Pt::new(0.0, 0.0), Pt::new(4.0, 0.0), Pt::new(4.0, 4.0), Pt::new(0.0, 4.0)]);
    s.add("简单-方形", is_simple(&square), "square simple");
    let tri = Poly::new(alloc::vec![Pt::new(0.0, 0.0), Pt::new(2.0, 0.0), Pt::new(1.0, 2.0)]);
    s.add("简单-三角", is_simple(&tri), "tri simple");
    // 凹多边形仍简单
    let concave = Poly::new(alloc::vec![
        Pt::new(0.0, 0.0), Pt::new(4.0, 0.0), Pt::new(4.0, 4.0), Pt::new(2.0, 1.0), Pt::new(0.0, 4.0),
    ]);
    s.add("简单-凹形", is_simple(&concave), "concave simple");
    // 族二：自交多边形（蝴蝶结）
    let bow = Poly::new(alloc::vec![Pt::new(0.0, 0.0), Pt::new(4.0, 4.0), Pt::new(4.0, 0.0), Pt::new(0.0, 4.0)]);
    let hits = self_isects(&bow);
    s.add("自交-检出", hits.len() == 1, "bowtie 1 isect");
    s.add("自交-位置", hits[0].dist(Pt::new(2.0, 2.0)) < 1e-9, "at center");
    s.add("自交-不简单", !is_simple(&bow), "not simple");
    // 族三：分裂预处理
    let (split, k) = split_at_isects(&bow);
    s.add("分裂-交点数", k == 1, "1 isect");
    s.add("分裂-顶点增加", split.verts.len() == 5, "4+1 verts");
    s.add("分裂-交点入表", split.verts.iter().any(|v| v.dist(Pt::new(2.0, 2.0)) < 1e-9), "isect in verts");
    s.add("分裂-无重复", split.verts.windows(2).all(|w| w[0].dist(w[1]) > 1e-9), "no adjacent dup");
    // 分裂后边不再穿越（分段的边对仍可能共享交点端点，但 seg_isect 内部区间排除端点）
    s.add("分裂-CCW规范化", normalize(&split).area() > 0.0, "normalized");
    // 分裂后自交检测仍报 1（交点端点相触不算新穿越）——正确语义：端点接触被排除
    s.add("分裂-端点豁免", {
        // 蝴蝶结在 (2,2) 有两条边端点相接，seg_isect 排除端点 → 无新交
        let h2 = self_isects(&split);
        h2.is_empty()
    }, "endpoint touch excluded");
    // 族四：多次自交
    let spiral = Poly::new(alloc::vec![
        Pt::new(0.0, 0.0), Pt::new(6.0, 0.0), Pt::new(6.0, 6.0), Pt::new(1.0, 6.0),
        Pt::new(1.0, 1.0), Pt::new(5.0, 1.0), Pt::new(5.0, 5.0), Pt::new(2.0, 5.0),
    ]);
    let _ = &spiral;
    // 螺旋是简单多边形（嵌套但不自交）
    s.add("螺旋-简单", is_simple(&spiral), "spiral simple");
    // 真双交：八字加横
    let multi = Poly::new(alloc::vec![
        Pt::new(0.0, 0.0), Pt::new(4.0, 4.0), Pt::new(2.0, 0.0), Pt::new(0.0, 4.0),
    ]);
    let hm = self_isects(&multi);
    s.add("多交-检出", hm.len() == 2, "figure-8 2 isects");
    let (split_m, km) = split_at_isects(&multi);
    s.add("多交-分裂", km == 2 && split_m.verts.len() == 6, "2 isects split");
    // 族五：seg_isect 平行无解
    s.add("平行-无解", seg_isect(Pt::ZERO, Pt::new(4.0, 0.0), Pt::new(0.0, 1.0), Pt::new(4.0, 1.0)).is_none(), "parallel none");
    // 共线重叠无唯一解
    s.add("共线-无解", seg_isect(Pt::ZERO, Pt::new(4.0, 0.0), Pt::new(1.0, 0.0), Pt::new(5.0, 0.0)).is_none(), "collinear none");
    // 正常相交
    s.add("相交-有解", seg_isect(Pt::ZERO, Pt::new(4.0, 4.0), Pt::new(0.0, 4.0), Pt::new(4.0, 0.0)).is_some(), "cross");
    // 族六：T 形触碰（端点在边上）不算自交
    let tee = Poly::new(alloc::vec![
        Pt::new(0.0, 0.0), Pt::new(4.0, 0.0), Pt::new(4.0, 4.0), Pt::new(2.0, 2.0), Pt::new(0.0, 4.0),
    ]);
    s.add("触碰-豁免", is_simple(&tee) || !self_isects(&tee).iter().any(|h| h.dist(Pt::new(2.0, 2.0)) < 1e-9), "T-touch excluded");
    s
}
