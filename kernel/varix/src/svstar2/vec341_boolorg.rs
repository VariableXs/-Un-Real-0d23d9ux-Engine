//! VE-F0741 布尔运算架构总纲
//! 职责：多边形布尔（并/交/差/异或）的阶段管线定义、有向边记号模型、
//!       环绕数规则与奇偶规则双档、能力登记表。
//! 契约：记号模型输入→扫描→环绕计数→分类输出四段，段间契约显式。

use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::vec321_bezcore::Pt;

/// 布尔运算种类
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum BoolOp {
    Union,
    Inter,
    Diff,
    Xor,
}

/// 环绕填充规则
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum FillRule {
    NonZero,
    EvenOdd,
}

/// 有向边（多边形边界段）
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct DirEdge {
    pub a: Pt,
    pub b: Pt,
}

impl DirEdge {
    pub fn new(a: Pt, b: Pt) -> DirEdge {
        DirEdge { a, b }
    }
    /// 边方向：CCW 为正
    pub fn signed_area2(&self) -> f64 {
        self.a.cross(self.b)
    }
    /// 与水平扫描线 y 的交点 x（若跨行）
    pub fn scan_x(&self, y: f64) -> Option<f64> {
        let (y0, y1) = (self.a.y.min(self.b.y), self.a.y.max(self.b.y));
        if y < y0 || y >= y1 {
            return None;
        }
        let t = (y - self.a.y) / (self.b.y - self.a.y);
        Some(self.a.x + t * (self.b.x - self.a.x))
    }
}

/// 多边形（顶点序列，隐式闭合）
#[derive(Clone, Debug, PartialEq)]
pub struct Poly {
    pub verts: Vec<Pt>,
}

impl Poly {
    pub fn new(verts: Vec<Pt>) -> Poly {
        Poly { verts }
    }
    /// 有向面积（鞋带公式；CCW 为正）
    pub fn area(&self) -> f64 {
        let n = self.verts.len();
        if n < 3 {
            return 0.0;
        }
        let mut a = 0.0;
        for i in 0..n {
            let j = (i + 1) % n;
            a += self.verts[i].cross(self.verts[j]);
        }
        a * 0.5
    }
    pub fn is_ccw(&self) -> bool {
        self.area() > 0.0
    }
    /// 规范化：统一为 CCW
    pub fn normalize_ccw(&self) -> Poly {
        if self.is_ccw() {
            self.clone()
        } else {
            Poly::new(self.verts.iter().rev().cloned().collect())
        }
    }
    pub fn edges(&self) -> Vec<DirEdge> {
        let n = self.verts.len();
        (0..n).map(|i| DirEdge::new(self.verts[i], self.verts[(i + 1) % n])).collect()
    }
}

/// 扫描线事件（边在 y 处的交点）
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct ScanEvent {
    pub x: f64,
    pub winding: i32,
}

/// 对一组边做 y 扫描，收集交点事件
pub fn scan_row(edges: &[DirEdge], y: f64) -> Vec<ScanEvent> {
    let mut evs: Vec<ScanEvent> = alloc::vec![];
    for e in edges {
        if let Some(x) = e.scan_x(y) {
            let up = e.b.y > e.a.y;
            evs.push(ScanEvent { x, winding: if up { 1 } else { -1 } });
        }
    }
    evs.sort_by(|a, b| a.x.partial_cmp(&b.x).unwrap_or(core::cmp::Ordering::Equal));
    evs
}

/// 环绕数规则下的内部判定
pub fn inside_by_rule(evs: &[ScanEvent], rule: FillRule) -> Vec<bool> {
    let mut out: Vec<bool> = alloc::vec![];
    let mut wind = 0i32;
    for i in 0..evs.len() {
        // 在事件 i 与 i+1 之间判定
        wind += evs[i].winding;
        let inside = match rule {
            FillRule::NonZero => wind != 0,
            FillRule::EvenOdd => wind % 2 != 0,
        };
        out.push(inside);
    }
    out
}

/// 阶段管线登记
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Stage {
    Tokenize,
    Sweep,
    Winding,
    Classify,
}

pub const STAGES: [Stage; 4] = [Stage::Tokenize, Stage::Sweep, Stage::Winding, Stage::Classify];

pub fn run_vec341_checks() -> crate::checks::CheckSet {
    let mut s = CheckSet::new("svstar2-vec341");
    // 族一：多边形面积与方向
    let square = Poly::new(alloc::vec![Pt::new(0.0, 0.0), Pt::new(4.0, 0.0), Pt::new(4.0, 4.0), Pt::new(0.0, 4.0)]);
    s.add("面积-单位方", (square.area() - 16.0).abs() < 1e-12, "area 16");
    s.add("方向-CCW", square.is_ccw(), "ccw");
    let cw = Poly::new(square.verts.iter().rev().cloned().collect());
    s.add("方向-CW", !cw.is_ccw(), "cw reversed");
    s.add("规范化-稳定", cw.normalize_ccw() == square, "normalize");
    // 退化：两点/一点面积 0
    s.add("面积-线段零", Poly::new(alloc::vec![Pt::ZERO, Pt::new(1.0, 1.0)]).area() == 0.0, "segment");
    s.add("面积-空零", Poly::new(alloc::vec![]).area() == 0.0, "empty");
    // 族二：有向边扫描
    let e_up = DirEdge::new(Pt::new(1.0, 0.0), Pt::new(3.0, 4.0));
    s.add("扫描-上行有交", e_up.scan_x(2.0).is_some(), "up edge hits y=2");
    s.add("扫描-交点x", (e_up.scan_x(2.0).expect("x") - 2.0).abs() < 1e-12, "x=2 at y=2");
    s.add("扫描-行下方无", e_up.scan_x(-1.0).is_none(), "below");
    s.add("扫描-行上方无", e_up.scan_x(4.0).is_none(), "at top exclusive");
    let e_dn = DirEdge::new(Pt::new(3.0, 4.0), Pt::new(1.0, 0.0));
    s.add("扫描-下行有交", e_dn.scan_x(2.0).is_some(), "down edge");
    // 族三：扫描事件与环绕
    let edges = square.edges();
    let evs = scan_row(&edges, 2.0);
    s.add("事件-两交点", evs.len() == 2, "square row → 2 hits");
    s.add("事件-有序", evs[0].x < evs[1].x, "sorted by x");
    s.add("事件-方向对", evs[0].winding == 1 && evs[1].winding == -1, "+1 then -1");
    // 非零规则：两交点之间在内
    let inside = inside_by_rule(&evs, FillRule::NonZero);
    s.add("环绕-区间内", inside == alloc::vec![true], "inside between");
    // 奇偶规则同结果（单环）
    let inside2 = inside_by_rule(&evs, FillRule::EvenOdd);
    s.add("奇偶-同判", inside2 == alloc::vec![true], "evenodd same");
    // 族四：双环（有洞）非零 vs 奇偶
    // 外方 CCW + 内方 CW（洞）同向交点：+1,+1,-1,-1
    let hole_evs = alloc::vec![
        ScanEvent { x: 0.0, winding: 1 },
        ScanEvent { x: 1.0, winding: 1 },
        ScanEvent { x: 3.0, winding: -1 },
        ScanEvent { x: 4.0, winding: -1 },
    ];
    let nz = inside_by_rule(&hole_evs, FillRule::NonZero);
    s.add("洞-非零规则", nz == alloc::vec![true, true, false], "nonzero hole inside");
    let eo = inside_by_rule(&hole_evs, FillRule::EvenOdd);
    s.add("洞-奇偶规则", eo == alloc::vec![true, false, true], "evenodd annulus");
    // 族五：四阶段登记
    s.add("阶段-四段", STAGES.len() == 4, "4 stages");
    s.add("阶段-有序", STAGES[0] == Stage::Tokenize && STAGES[3] == Stage::Classify, "order");
    // 族六：运算种类
    s.add("运算-四种", [BoolOp::Union, BoolOp::Inter, BoolOp::Diff, BoolOp::Xor].len() == 4, "4 ops");
    s
}
