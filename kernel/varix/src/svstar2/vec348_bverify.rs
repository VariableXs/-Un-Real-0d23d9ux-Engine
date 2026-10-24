//! VE-F0748 布尔结果验证器
//! 职责：布尔结果多边形的验证（闭合性/简单性/方向/面积对账/环绕互验）、
//!       验证报告（多级判定与明细）、验证失败定位。
//! 契约：报告逐项给出 pass/fail 与定位信息；面积对账容差显式。

use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::vec321_bezcore::Pt;
use super::vec341_boolorg::Poly;
use super::vec347_selfx::is_simple;

/// 验证级别
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum VLevel {
    /// 结构（闭合、无重复点）
    Struct,
    /// 简单性（无自交）
    Simple,
    /// 方向（CCW 规范）
    Orient,
    /// 面积（与期望对账）
    Area,
}

/// 验证报告
#[derive(Clone, Debug, PartialEq)]
pub struct VReport {
    pub rows: Vec<(&'static str, bool, &'static str)>,
}

impl VReport {
    pub fn new() -> VReport {
        VReport { rows: alloc::vec![] }
    }
    pub fn add(&mut self, name: &'static str, pass: bool, note: &'static str) {
        self.rows.push((name, pass, note));
    }
    pub fn all_pass(&self) -> bool {
        self.rows.iter().all(|(_, p, _)| *p)
    }
    pub fn failed(&self) -> Vec<&'static str> {
        self.rows.iter().filter(|(_, p, _)| !p).map(|(n, _, _)| *n).collect()
    }
    /// 按级别判定：Struct 及以下全过才看 Simple……
    pub fn pass_through(&self, lvl: VLevel) -> bool {
        let upto = match lvl {
            VLevel::Struct => 0,
            VLevel::Simple => 1,
            VLevel::Orient => 2,
            VLevel::Area => 3,
        };
        self.rows.iter().take(upto + 1).all(|(_, p, _)| *p)
    }
}

impl Default for VReport {
    fn default() -> Self {
        VReport::new()
    }
}

/// 全面验证
pub fn validate(p: &Poly, expect_area: f64, tol: f64) -> VReport {
    let mut r = VReport::new();
    // 结构：顶点序列隐式闭合，只要求环长 ≥ 3
    let closed = p.verts.len() >= 3;
    r.add("闭合", closed, "implicit ring");
    let no_dup = p.verts.windows(2).all(|w| w[0].dist(w[1]) > 1e-9);
    r.add("无重复点", no_dup, "adjacent distinct");
    // 简单性
    r.add("无自交", is_simple(p), "no self crossing");
    // 方向
    r.add("CCW", p.area() > 0.0, "positive orientation");
    // 面积
    let dev = (p.area() - expect_area).abs();
    r.add("面积对账", dev <= tol, "area within tol");
    r
}

/// 环绕互验：重建结果在若干采样行上的环绕数应与期望内部一致
pub fn winding_crosscheck(p: &Poly, probe: Pt) -> bool {
    // 点在多边形内（射线法，偶数规则）
    let n = p.verts.len();
    let mut inside = false;
    let mut j = n - 1;
    for i in 0..n {
        let a = p.verts[j];
        let b = p.verts[i];
        if ((a.y > probe.y) != (b.y > probe.y))
            && (probe.x < (b.x - a.x) * (probe.y - a.y) / (b.y - a.y) + a.x)
        {
            inside = !inside;
        }
        j = i;
    }
    inside
}

pub fn run_vec348_checks() -> crate::checks::CheckSet {
    let mut s = CheckSet::new("svstar2-vec348");
    // 族一：正常结果全过
    let square = Poly::new(alloc::vec![Pt::new(0.0, 0.0), Pt::new(4.0, 0.0), Pt::new(4.0, 4.0), Pt::new(0.0, 4.0)]);
    let rep = validate(&square, 16.0, 1e-9);
    s.add("验证-全过", rep.all_pass(), "clean square");
    s.add("验证-行数", rep.rows.len() == 5, "5 checks");
    s.add("验证-失败空", rep.failed().is_empty(), "no fails");
    s.add("验证-逐级", rep.pass_through(VLevel::Area), "pass to Area");
    // 族二：面积不符检出
    let rep2 = validate(&square, 12.0, 1e-9);
    s.add("对账-面积不符", !rep2.all_pass(), "area mismatch");
    s.add("对账-失败名", rep2.failed() == alloc::vec!["面积对账"], "failed: area");
    s.add("对账-低级过", rep2.pass_through(VLevel::Orient), "pass to Orient only");
    // 族三：自交检出
    let bow = Poly::new(alloc::vec![Pt::new(0.0, 0.0), Pt::new(4.0, 4.0), Pt::new(4.0, 0.0), Pt::new(0.0, 4.0)]);
    let rep3 = validate(&bow, 0.0, 1e-9);
    s.add("自交-验证失败", !rep3.all_pass(), "bowtie fails");
    s.add("自交-失败含简单性", rep3.failed().contains(&"无自交"), "failed: simple");
    // 结构级仍可过（重复点无、闭合）
    s.add("自交-结构过", rep3.pass_through(VLevel::Struct), "struct ok");
    // 族四：重复点检出
    let dup = Poly::new(alloc::vec![Pt::new(0.0, 0.0), Pt::new(1e-12, 0.0), Pt::new(4.0, 0.0), Pt::new(4.0, 4.0), Pt::new(0.0, 4.0)]);
    let rep4 = validate(&dup, 16.0, 1e-9);
    s.add("重复-失败", !rep4.all_pass(), "dup fails");
    s.add("重复-结构级挂", !rep4.pass_through(VLevel::Struct), "struct gate blocks");
    // 族五：方向检出（CW 输入）
    let cw = Poly::new(square.verts.iter().rev().cloned().collect());
    let rep5 = validate(&cw, -16.0, 1e-9);
    s.add("方向-CW面积负", !rep5.all_pass(), "CW area negative → CCW row fails");
    // 族六：射线法互验
    s.add("互验-内点", winding_crosscheck(&square, Pt::new(2.0, 2.0)), "center inside");
    s.add("互验-外点", !winding_crosscheck(&square, Pt::new(9.0, 9.0)), "outside");
    s.add("互验-边界外", !winding_crosscheck(&square, Pt::new(4.5, 2.0)), "right outside");
    let concave = Poly::new(alloc::vec![
        Pt::new(0.0, 0.0), Pt::new(4.0, 0.0), Pt::new(4.0, 4.0), Pt::new(2.0, 1.0), Pt::new(0.0, 4.0),
    ]);
    s.add("互验-凹形内", winding_crosscheck(&concave, Pt::new(1.0, 0.5)), "concave inner");
    s.add("互验-凹口外", !winding_crosscheck(&concave, Pt::new(2.5, 1.5)), "notch outside");
    // 族七：报告默认空
    let empty = VReport::new();
    s.add("空报告-全过", empty.all_pass(), "vacuous");
    s.add("空报告-失败空", empty.failed().is_empty(), "no fails");
    s
}
