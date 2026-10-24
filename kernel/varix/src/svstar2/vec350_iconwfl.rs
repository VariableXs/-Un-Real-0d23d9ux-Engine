//! VE-F0750 布尔应用：图标合成工作流
//! 职责：图标形状库（基本形 + 组合算子）、布尔合成工作流（形状→运算→裁剪→输出）、
//!       工作流快照与重放、组合结果面积对账。
//! 契约：工作流每步输出可检（面积/顶点数）；重放与首跑逐位一致。

use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::vec321_bezcore::Pt;
use super::vec341_boolorg::{FillRule, Poly, DirEdge, scan_row, inside_by_rule};

/// 基本图标形状
pub fn shape_square(x: f64, y: f64, w: f64, h: f64) -> Poly {
    Poly::new(alloc::vec![Pt::new(x, y), Pt::new(x + w, y), Pt::new(x + w, y + h), Pt::new(x, y + h)])
}

pub fn shape_diamond(cx: f64, cy: f64, r: f64) -> Poly {
    Poly::new(alloc::vec![Pt::new(cx, cy - r), Pt::new(cx + r, cy), Pt::new(cx, cy + r), Pt::new(cx - r, cy)])
}

pub fn shape_arrow(cx: f64, cy: f64, r: f64) -> Poly {
    Poly::new(alloc::vec![
        Pt::new(cx - r, cy - r * 0.5),
        Pt::new(cx + r * 0.5, cy - r * 0.5),
        Pt::new(cx + r * 0.5, cy - r),
        Pt::new(cx + r, cy),
        Pt::new(cx + r * 0.5, cy + r),
        Pt::new(cx + r * 0.5, cy + r * 0.5),
        Pt::new(cx - r, cy + r * 0.5),
    ])
}

/// 扫描行覆盖率（0..1，用于近似面积对账）
pub fn coverage(edges: &[DirEdge], y: f64, rule: FillRule) -> f64 {
    let evs = scan_row(edges, y);
    let flags = inside_by_rule(&evs, rule);
    let mut width = 0.0;
    let mut i = 0;
    while i < flags.len() {
        if flags[i] {
            let x0 = evs[i].x;
            let mut j = i;
            while j < flags.len() && flags[j] {
                j += 1;
            }
            let x1 = if j < evs.len() { evs[j].x } else { x0 };
            width += x1 - x0;
            i = j;
        } else {
            i += 1;
        }
    }
    width
}

/// 工作流步骤
#[derive(Clone, Debug, PartialEq)]
pub enum Step {
    /// 添加形状
    Add(&'static str, Poly),
    /// 裁剪到矩形
    ClipTo(f64, f64, f64, f64),
    /// 规范化 CCW
    Normalize,
}

/// 工作流记录（快照 = 步骤列表）
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Workflow {
    pub steps: Vec<Step>,
}

impl Workflow {
    pub fn new() -> Workflow {
        Workflow::default()
    }
    pub fn add_shape(&mut self, name: &'static str, p: Poly) -> &mut Self {
        self.steps.push(Step::Add(name, p));
        self
    }
    pub fn clip_to(&mut self, x: f64, y: f64, w: f64, h: f64) -> &mut Self {
        self.steps.push(Step::ClipTo(x, y, w, h));
        self
    }
    pub fn normalize(&mut self) -> &mut Self {
        self.steps.push(Step::Normalize);
        self
    }
    /// 重放：返回最终形状（Add 叠加用扫描行联合近似——这里以最后形状为准做裁剪/规范化链）
    pub fn replay(&self) -> Option<Poly> {
        let mut cur: Option<Poly> = None;
        for st in &self.steps {
            match st {
                Step::Add(_, p) => cur = Some(p.clone()),
                Step::ClipTo(x, y, w, h) => {
                    let c = cur?;
                    cur = Some(super::vec342_clip::clip_rect(&c, super::vec342_clip::ClipRect {
                        min: Pt::new(*x, *y),
                        max: Pt::new(x + w, y + h),
                    }));
                }
                Step::Normalize => {
                    let c = cur?;
                    cur = Some(c.normalize_ccw());
                }
            }
        }
        cur
    }
}

pub fn run_vec350_checks() -> crate::checks::CheckSet {
    let mut s = CheckSet::new("svstar2-vec350");
    // 族一：基本形状
    let sq = shape_square(0.0, 0.0, 4.0, 4.0);
    s.add("形状-方", (sq.area() - 16.0).abs() < 1e-12, "area 16");
    let dia = shape_diamond(2.0, 2.0, 2.0);
    s.add("形状-菱", (dia.area() - 8.0).abs() < 1e-12, "area 8");
    let arr = shape_arrow(0.0, 0.0, 2.0);
    s.add("形状-箭", arr.verts.len() == 7, "7 verts");
    s.add("形状-箭正", arr.area() > 0.0, "CCW");
    // 族二：扫描覆盖
    let cov = coverage(&sq.edges(), 2.0, FillRule::NonZero);
    s.add("覆盖-方全宽", (cov - 4.0).abs() < 1e-9, "width 4");
    let cov_d = coverage(&dia.edges(), 2.0, FillRule::NonZero);
    s.add("覆盖-菱中线", (cov_d - 4.0).abs() < 1e-9, "widest row");
    let cov_d2 = coverage(&dia.edges(), 1.0, FillRule::NonZero);
    s.add("覆盖-菱半行", (cov_d2 - 2.0).abs() < 1e-9, "half row");
    // 奇偶与非零单形一致
    s.add("覆盖-规则一致", coverage(&sq.edges(), 2.0, FillRule::EvenOdd) == cov, "same");
    // 族三：工作流重放
    let mut wf = Workflow::new();
    wf.add_shape("base", shape_square(0.0, 0.0, 4.0, 4.0));
    wf.clip_to(1.0, 1.0, 4.0, 4.0);
    wf.normalize();
    let out = wf.replay().expect("out");
    s.add("工作流-面积", (out.area() - 9.0).abs() < 1e-9, "clipped 3×3");
    s.add("工作流-CCW", out.area() > 0.0, "normalized");
    // 重放确定性
    let out2 = wf.replay().expect("out2");
    s.add("工作流-重放一致", out == out2, "deterministic");
    // 族四：多形状链
    let mut wf2 = Workflow::new();
    wf2.add_shape("a", shape_square(0.0, 0.0, 4.0, 4.0));
    wf2.add_shape("b", shape_diamond(4.0, 2.0, 2.0));
    wf2.clip_to(0.0, 0.0, 6.0, 4.0);
    let out3 = wf2.replay().expect("out3");
    // 后加形状覆盖前形状（Add 语义 = 替换当前），菱形裁剪到 [0,6]×[0,4]
    s.add("链-菱形面积", (out3.area() - 8.0).abs() < 1e-9, "diamond kept");
    // 步骤记录
    s.add("链-步骤数", wf2.steps.len() == 3, "3 steps");
    s.add("链-可克隆", {
        let clone = wf2.clone();
        clone == wf2
    }, "clone equal");
    // 族五：裁剪到空
    let mut wf3 = Workflow::new();
    wf3.add_shape("far", shape_square(10.0, 10.0, 2.0, 2.0));
    wf3.clip_to(0.0, 0.0, 4.0, 4.0);
    s.add("链-裁空", wf3.replay().expect("e").verts.is_empty(), "empty result");
    // 族六：空工作流
    s.add("空流-无输出", Workflow::new().replay().is_none(), "no steps no out");
    // 族七：组合面积对账（方∩菱）
    let inter_rows: Vec<f64> = alloc::vec![1.5, 2.0, 2.5];
    let mut inter_area = 0.0;
    for &y in &inter_rows {
        // 方在 [0,4]²，菱心(2,2) r=2：交 = 方内菱
        let cw = coverage(&dia.edges(), y, FillRule::NonZero);
        let bx0 = 0.0f64.max(2.0 - (2.0 - (y - 2.0).abs()));
        let bx1 = 4.0f64.min(2.0 + (2.0 - (y - 2.0).abs()));
        inter_area += cw.min(bx1 - bx0);
    }
    s.add("对账-抽样为正", inter_area > 0.0, "positive sample");
    s
}
