//! VE-F0746 布尔性能优化
//! 职责：扫描事件的空间索引（按行桶）、边筛选（bbox 预剔除）、
//!       成本模型（事件数驱动的扫描复杂度）、批量布尔吞吐账本。
//! 契约：预剔除后结果与全量扫描一致（事件集不变）。

use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::vec321_bezcore::Pt;
use super::vec341_boolorg::{DirEdge, FillRule, Poly, scan_row, inside_by_rule, ScanEvent};

/// 行桶索引：y 区间 → 边序号列表
pub struct RowIndex {
    pub y0: f64,
    pub dy: f64,
    pub rows: usize,
    pub buckets: Vec<Vec<usize>>,
}

impl RowIndex {
    pub fn build(edges: &[DirEdge], y0: f64, y1: f64, rows: usize) -> RowIndex {
        let rows = rows.max(1);
        let dy = (y1 - y0) / rows as f64;
        let mut buckets: Vec<Vec<usize>> = alloc::vec![alloc::vec![]; rows];
        for (i, e) in edges.iter().enumerate() {
            let (a, b) = (e.a.y.min(e.b.y), e.a.y.max(e.b.y));
            let r0 = (((a - y0) / dy).floor().max(0.0)) as usize;
            let r1 = ((((b - y0) / dy).floor() as usize) + 1).min(rows);
            for r in r0..r1 {
                buckets[r].push(i);
            }
        }
        RowIndex { y0, dy, rows, buckets }
    }
    /// 查询某行涉及的边
    pub fn row_edges(&self, y: f64) -> &[usize] {
        let r = (((y - self.y0) / self.dy).floor().max(0.0)) as usize;
        if r >= self.rows {
            &[]
        } else {
            &self.buckets[r]
        }
    }
}

/// bbox 预剔除：只保留与 y 行相交的边
pub fn filter_edges(edges: &[DirEdge], y: f64) -> Vec<usize> {
    edges
        .iter()
        .enumerate()
        .filter(|(_, e)| {
            let (a, b) = (e.a.y.min(e.b.y), e.a.y.max(e.b.y));
            y >= a && y < b
        })
        .map(|(i, _)| i)
        .collect()
}

/// 成本模型：总事件数（扫描复杂度的直接度量）
pub fn cost_events(edges: &[DirEdge], ys: &[f64]) -> usize {
    ys.iter().map(|&y| scan_row(edges, y).len()).sum()
}

/// 带索引的扫描：利用 row_edges 缩小边集
pub fn scan_indexed(idx: &RowIndex, edges: &[DirEdge], y: f64) -> Vec<ScanEvent> {
    let ids = idx.row_edges(y);
    let sub: Vec<DirEdge> = ids.iter().map(|&i| edges[i]).collect();
    scan_row(&sub, y)
}

/// 批量吞吐账本
#[derive(Clone, Debug, PartialEq)]
pub struct Throughput {
    pub rows: usize,
    pub events: usize,
    pub polys: usize,
}

impl Throughput {
    pub fn events_per_row(&self) -> f64 {
        if self.rows == 0 {
            0.0
        } else {
            self.events as f64 / self.rows as f64
        }
    }
    pub fn record(edges: &[DirEdge], ys: &[f64], polys: usize) -> Throughput {
        Throughput { rows: ys.len(), events: cost_events(edges, ys), polys }
    }
}

pub fn run_vec346_checks() -> crate::checks::CheckSet {
    let mut s = CheckSet::new("svstar2-vec346");
    let square = Poly::new(alloc::vec![Pt::new(0.0, 0.0), Pt::new(4.0, 0.0), Pt::new(4.0, 4.0), Pt::new(0.0, 4.0)]);
    let edges = square.edges();
    let ys: Vec<f64> = alloc::vec![0.5, 1.5, 2.5, 3.5];
    // 族一：行桶索引
    let idx = RowIndex::build(&edges, 0.0, 4.0, 4);
    s.add("索引-行数", idx.rows == 4, "4 rows");
    s.add("索引-行高", (idx.dy - 1.0).abs() < 1e-12, "dy 1");
    // 每行两条边（方形每行左右各一）
    for &y in &ys {
        let ids = idx.row_edges(y);
        s.add("索引-每行两边", ids.len() == 2, "row hits");
    }
    // 域外行
    s.add("索引-域外空", idx.row_edges(10.0).is_empty(), "outside empty");
    // 族二：预剔除
    let f = filter_edges(&edges, 2.0);
    s.add("剔除-数量", f.len() == 2, "2 edges at y=2");
    // 剔除后事件集一致
    let e_full = scan_row(&edges, 2.0);
    let sub: Vec<DirEdge> = f.iter().map(|&i| edges[i]).collect();
    let e_sub = scan_row(&sub, 2.0);
    s.add("剔除-事件一致", e_full == e_sub, "same events");
    // 域外剔除
    s.add("剔除-域外空", filter_edges(&edges, 10.0).is_empty(), "outside none");
    // 族三：带索引扫描与全量一致
    for &y in &ys {
        let a = scan_row(&edges, y);
        let b = scan_indexed(&idx, &edges, y);
        s.add("索引-一致", a == b, "indexed == full");
    }
    // 环绕判定一致
    let flags_full = inside_by_rule(&scan_row(&edges, 1.5), FillRule::NonZero);
    let flags_idx = inside_by_rule(&scan_indexed(&idx, &edges, 1.5), FillRule::NonZero);
    s.add("索引-判定一致", flags_full == flags_idx, "same inside");
    // 族四：成本模型
    let cost = cost_events(&edges, &ys);
    s.add("成本-事件数", cost == 8, "4 rows × 2 events");
    let big = Poly::new((0..8).map(|k| Pt::new(k as f64, (k % 2) as f64 * 2.0)).collect());
    let cost_big = cost_events(&big.edges(), &ys);
    s.add("成本-多边形更多", cost_big > cost, "more edges more events");
    // 族五：吞吐账本
    let tp = Throughput::record(&edges, &ys, 1);
    s.add("账本-行数", tp.rows == 4, "rows");
    s.add("账本-事件", tp.events == 8, "events");
    s.add("账本-均摊", (tp.events_per_row() - 2.0).abs() < 1e-12, "2 per row");
    s.add("账本-多边形数", tp.polys == 1, "1 poly");
    let tp0 = Throughput { rows: 0, events: 0, polys: 0 };
    s.add("账本-零行均摊", tp0.events_per_row() == 0.0, "no div0");
    // 族六：索引构建的边重复计数（跨度行覆盖）
    let tall = DirEdge::new(Pt::new(1.0, 0.0), Pt::new(1.0, 4.0));
    let idx2 = RowIndex::build(&alloc::vec![tall], 0.0, 4.0, 4);
    s.add("索引-长边全行", idx2.buckets.iter().all(|b| b.len() == 1), "tall edge in all rows");
    let short = DirEdge::new(Pt::new(1.0, 0.0), Pt::new(1.0, 1.0));
    let idx3 = RowIndex::build(&alloc::vec![short], 0.0, 4.0, 4);
    s.add("索引-短边单行", idx3.buckets[0].len() == 1 && idx3.buckets[1].is_empty(), "short edge one row");
    s
}
