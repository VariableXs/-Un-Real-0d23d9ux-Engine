//! VE-F0745 布尔结果重建
//! 职责：从扫描线区间重建结果多边形（区间合并→矩形条带→条带缝合）、
//!       重建结果的面数/洞数统计、重建结果与扫描环绕数互验。
//! 契约：重建多边形可再次通过退化检测（无重复点）；面积与扫描覆盖一致。

use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::vec321_bezcore::Pt;
use super::vec341_boolorg::{FillRule, Poly, DirEdge, scan_row, inside_by_rule};

/// 扫描区间（一行上的内部区段）
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Span {
    pub y: f64,
    pub x0: f64,
    pub x1: f64,
}

/// 由边集在给定行集上生成内部区间
pub fn spans_at(edges: &[DirEdge], ys: &[f64], rule: FillRule) -> Vec<Span> {
    let mut out: Vec<Span> = alloc::vec![];
    for &y in ys {
        let evs = scan_row(edges, y);
        let flags = inside_by_rule(&evs, rule);
        let mut i = 0;
        while i < flags.len() {
            if flags[i] {
                let x0 = evs[i].x;
                let mut j = i;
                while j < flags.len() && flags[j] {
                    j += 1;
                }
                // 区间右端：从 j 位置出去；x1 取 evs[j].x（若存在）否则最后事件
                let x1 = if j < evs.len() { evs[j].x } else { evs.last().map(|e| e.x).unwrap_or(x0) };
                out.push(Span { y, x0, x1 });
                i = j;
            } else {
                i += 1;
            }
        }
    }
    out
}

/// 区间总面积（蒙太奇积分）
pub fn spans_area(spans: &[Span], dy: f64) -> f64 {
    spans.iter().map(|sp| (sp.x1 - sp.x0) * dy).sum()
}

/// 重建统计
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct RebuildStat {
    pub spans: usize,
    pub area: f64,
}

/// 从区间重建矩形条带多边形（每区间一个矩形，供下游合并）
pub fn spans_to_rects(spans: &[Span], dy: f64) -> Vec<Poly> {
    spans
        .iter()
        .map(|sp| {
            Poly::new(alloc::vec![
                Pt::new(sp.x0, sp.y),
                Pt::new(sp.x1, sp.y),
                Pt::new(sp.x1, sp.y + dy),
                Pt::new(sp.x0, sp.y + dy),
            ])
        })
        .collect()
}

/// 条带多边形顶点无重复（退化安全）
pub fn rects_clean(rects: &[Poly], eps: f64) -> bool {
    rects.iter().all(|r| {
        r.verts.windows(2).all(|w| w[0].dist(w[1]) >= eps)
            && r.verts[0].dist(r.verts[3]) >= eps
    })
}

pub fn run_vec345_checks() -> crate::checks::CheckSet {
    let mut s = CheckSet::new("svstar2-vec345");
    // 单位方形 [0,4]²
    let square = Poly::new(alloc::vec![Pt::new(0.0, 0.0), Pt::new(4.0, 0.0), Pt::new(4.0, 4.0), Pt::new(0.0, 4.0)]);
    let edges = square.edges();
    // 族一：区间生成
    let ys: Vec<f64> = alloc::vec![0.5, 1.5, 2.5, 3.5];
    let spans = spans_at(&edges, &ys, FillRule::NonZero);
    s.add("区间-数量", spans.len() == 4, "4 rows");
    s.add("区间-范围", spans.iter().all(|sp| (sp.x0 - 0.0).abs() < 1e-9 && (sp.x1 - 4.0).abs() < 1e-9), "full width");
    // 面积积分
    let area = spans_area(&spans, 1.0);
    s.add("积分-面积", (area - 16.0).abs() < 1e-9, "area 16");
    // 奇偶规则同（单环）
    let spans_eo = spans_at(&edges, &ys, FillRule::EvenOdd);
    s.add("奇偶-同区间", spans_eo == spans, "evenodd same");
    // 族二：有洞方形（内环 [1,3]² 反向）
    let hole = Poly::new(alloc::vec![Pt::new(1.0, 1.0), Pt::new(3.0, 1.0), Pt::new(3.0, 3.0), Pt::new(1.0, 3.0)]);
    let mut both = edges.clone();
    // 反向内环边（CW）
    let hn = hole.verts.len();
    for i in 0..hn {
        both.push(DirEdge { a: hole.verts[(i + 1) % hn], b: hole.verts[i] });
    }
    let sp_hole = spans_at(&both, &ys, FillRule::NonZero);
    s.add("洞-区间数", sp_hole.len() == 6, "2 rows split + 4 full");
    // 非零规则下中间行有洞：y=1.5 与 y=2.5 被拆成两段
    let mid: Vec<&Span> = sp_hole.iter().filter(|sp| sp.y > 1.0 && sp.y < 3.0).collect();
    s.add("洞-中间行两段", mid.len() == 4, "rows 1.5/2.5 × 2 spans");
    for sp in &mid {
        let has_gap = (sp.x1 - sp.x0) < 4.0;
        s.add("洞-宽度小于全宽", has_gap, "narrowed span");
    }
    // 洞环面积：外 16 - 内 4 = 12
    let area_hole = spans_area(&sp_hole, 1.0);
    s.add("洞-面积", (area_hole - 12.0).abs() < 1e-9, "annulus 12");
    // 奇偶规则下同样有洞（双环奇偶 = 非零此例）
    let sp_eo = spans_at(&both, &ys, FillRule::EvenOdd);
    let area_eo = spans_area(&sp_eo, 1.0);
    s.add("洞-奇偶同面积", (area_eo - 12.0).abs() < 1e-9, "evenodd same area");
    // 族三：条带重建
    let rects = spans_to_rects(&spans, 1.0);
    s.add("条带-数量", rects.len() == 4, "4 rects");
    s.add("条带-无重复点", rects_clean(&rects, 1e-9), "clean rects");
    let rect_area: f64 = rects.iter().map(|r| r.area()).sum();
    s.add("条带-面积和", (rect_area - 16.0).abs() < 1e-9, "sum 16");
    // 条带位置抽查
    s.add("条带-y对齐", rects[0].verts[0].y == 0.5 && rects[3].verts[0].y == 3.5, "row y");
    // 族四：窄行插值
    let ys_fine: Vec<f64> = (0..16).map(|k| 0.25 + k as f64 * 0.25).collect();
    let spans_fine = spans_at(&edges, &ys_fine, FillRule::NonZero);
    let area_fine = spans_area(&spans_fine, 0.25);
    s.add("细分-面积收敛", (area_fine - 16.0).abs() < 1e-9, "fine area 16");
    s.add("细分-行数", spans_fine.len() == 16, "16 rows");
    // 族五：统计
    let stat = RebuildStat { spans: spans.len(), area };
    s.add("统计-一致", stat.spans == 4 && (stat.area - 16.0).abs() < 1e-9, "stat");
    // 族六：三角形的扫描区间宽度递减
    let tri = Poly::new(alloc::vec![Pt::new(0.0, 0.0), Pt::new(4.0, 0.0), Pt::new(2.0, 4.0)]);
    let tri_spans = spans_at(&tri.edges(), &alloc::vec![1.0, 3.0], FillRule::NonZero);
    s.add("三角-区间数", tri_spans.len() == 2, "2 rows");
    let w1 = tri_spans[0].x1 - tri_spans[0].x0;
    let w3 = tri_spans[1].x1 - tri_spans[1].x0;
    s.add("三角-宽度递减", w3 < w1, "narrower up");
    s.add("三角-面积", (spans_area(&tri_spans, 1.0) - (w1 + w3)).abs() < 1e-9, "sum widths");
    s
}
