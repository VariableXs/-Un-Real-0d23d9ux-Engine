//! VE-F0757 布尔基准
//! 职责：布尔管线各阶段成本模型（记号/扫描/环绕/分类）、档位轮数、
//!       预算判定、基准入册行、多边形复杂度（边数）与成本的线性度验证。
//! 契约：成本随边数线性增长（扫描主导项 O(E)）；预算超界可判。

use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::vec321_bezcore::Pt;
use super::vec341_boolorg::{DirEdge, Poly, scan_row};

/// 阶段成本计数（伪时钟）
#[derive(Copy, Clone, Debug, PartialEq, Default)]
pub struct StageCost {
    pub tokenize: u64,
    pub sweep: u64,
    pub winding: u64,
    pub classify: u64,
}

impl StageCost {
    pub fn total(&self) -> u64 {
        self.tokenize + self.sweep + self.winding + self.classify
    }
}

/// 记号成本：边数
pub fn cost_tokenize(edges: &[DirEdge]) -> u64 {
    edges.len() as u64
}

/// 扫描成本：行数 × 每行事件扫描（边数 × 行数 上界）
pub fn cost_sweep(edges: &[DirEdge], rows: usize) -> u64 {
    edges.len() as u64 * rows as u64
}

/// 环绕成本：事件数累加
pub fn cost_winding(edges: &[DirEdge], ys: &[f64]) -> u64 {
    ys.iter().map(|&y| scan_row(edges, y).len() as u64).sum()
}

/// 分类成本 = 事件数
pub fn cost_classify(edges: &[DirEdge], ys: &[f64]) -> u64 {
    cost_winding(edges, ys)
}

/// 基准档位
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum BTier {
    Smoke,
    Standard,
    Deep,
}

impl BTier {
    pub fn rows(self) -> usize {
        match self {
            BTier::Smoke => 8,
            BTier::Standard => 64,
            BTier::Deep => 512,
        }
    }
}

/// 预算（事件数上限/行）
pub const BUDGET_EVENTS_PER_ROW: u64 = 64;

pub fn within_budget(edges: &[DirEdge], tier: BTier) -> bool {
    let ys: Vec<f64> = (0..tier.rows()).map(|k| k as f64 + 0.5).collect();
    cost_winding(edges, &ys) / tier.rows() as u64 <= BUDGET_EVENTS_PER_ROW
}

/// 基准行（入册）
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct BenchRow {
    pub name: &'static str,
    pub tier: BTier,
    pub events_per_row: f64,
}

pub fn run_bench(edges: &[DirEdge], tier: BTier, name: &'static str) -> BenchRow {
    let ys: Vec<f64> = (0..tier.rows()).map(|k| k as f64 + 0.5).collect();
    let ev = cost_winding(edges, &ys);
    BenchRow { name, tier, events_per_row: ev as f64 / tier.rows() as f64 }
}

/// 复杂正多边形（n 边）
pub fn ngon(n: usize, r: f64) -> Poly {
    let verts: Vec<Pt> = (0..n)
        .map(|k| {
            let th = 2.0 * core::f64::consts::PI * k as f64 / n as f64;
            Pt::new(r * th.cos(), r * th.sin())
        })
        .collect();
    Poly::new(verts)
}

pub fn run_vec357_checks() -> crate::checks::CheckSet {
    let mut s = CheckSet::new("svstar2-vec357");
    let square = Poly::new(alloc::vec![Pt::new(0.0, 0.0), Pt::new(4.0, 0.0), Pt::new(4.0, 4.0), Pt::new(0.0, 4.0)]);
    let edges = square.edges();
    // 族一：阶段成本
    s.add("成本-记号", cost_tokenize(&edges) == 4, "4 edges");
    s.add("成本-扫描", cost_sweep(&edges, 8) == 32, "4×8");
    let ys: Vec<f64> = alloc::vec![0.5, 1.5, 2.5, 3.5];
    s.add("成本-环绕", cost_winding(&edges, &ys) == 8, "4 rows × 2 ev");
    s.add("成本-分类", cost_classify(&edges, &ys) == 8, "= winding");
    let sc = StageCost { tokenize: 4, sweep: 32, winding: 8, classify: 8 };
    s.add("成本-总计", sc.total() == 52, "52");
    s.add("成本-默认零", StageCost::default().total() == 0, "zero default");
    // 族二：档位
    s.add("档位-行数", BTier::Smoke.rows() == 8 && BTier::Deep.rows() == 512, "rows");
    s.add("档位-有序", BTier::Smoke < BTier::Standard && BTier::Standard < BTier::Deep, "ordered");
    // 族三：预算
    s.add("预算-方过", within_budget(&edges, BTier::Deep), "2 ≤ 64");
    // 复杂多边形（256 边：每行事件最多 256）
    let big = ngon(256, 10.0);
    let big_edges = big.edges();
    s.add("预算-256边过", within_budget(&big_edges, BTier::Deep), "≤64 avg events/row");
    // 超界构造：4096 边
    let huge = ngon(4096, 10.0);
    s.add("预算-4096超界", !within_budget(&huge.edges(), BTier::Standard), "over budget");
    // 族四：线性度——事件/行随边数线性
    let e64 = ngon(64, 5.0).edges();
    let e256 = ngon(256, 5.0).edges();
    let b64 = run_bench(&e64, BTier::Deep, "n64");
    let b256 = run_bench(&e256, BTier::Deep, "n256");
    s.add("线性-比值", (b256.events_per_row / b64.events_per_row - 4.0).abs() < 0.5, "~4× for 4× edges");
    // 方形每行 2 事件
    let bsq = run_bench(&edges, BTier::Deep, "sq");
    s.add("线性-方形2", bsq.events_per_row == 2.0, "2 events/row");
    // 族五：入册行
    s.add("入册-名称", b64.name == "n64", "name kept");
    s.add("入册-档位", b64.tier == BTier::Deep, "tier kept");
    // 确定性
    s.add("确定-重跑", run_bench(&e64, BTier::Deep, "n64") == b64, "deterministic");
    // 族六：多边形生成
    let pent = ngon(5, 1.0);
    s.add("生成-5边", pent.verts.len() == 5, "5 verts");
    s.add("生成-凸", pent.area() > 0.0 && pent.area() < core::f64::consts::PI, "positive & < π");
    // 正五边形面积 = 5/2 r² sin(72°)
    let expect = 2.5 * (2.0 * core::f64::consts::PI / 5.0).sin();
    s.add("生成-面积公式", (pent.area() - expect).abs() < 1e-12, "regular pentagon area");
    s
}
