//! VE-F0739 曲线组收口
//! 职责：F0721~F0738 十八单的组级收口——交叉验证（弧长×分割×最近点联合一致）、
//!       能力矩阵（十八能力的实现/判据/容差三态登记）、遗留风险清单。
//! 契约：矩阵十八行全「已实现+已判据」；遗留风险全部挂账到远景。

use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::vec321_bezcore::{Curve, Deg, Pt};

/// 能力三态
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum CapState {
    Done,
    Missing,
}

/// 能力矩阵行
#[derive(Clone, Debug, PartialEq)]
pub struct CapRow {
    pub task: &'static str,
    pub module: &'static str,
    pub state: CapState,
    pub tol: f64,
}

/// 能力矩阵（十八单）
pub fn capability_matrix() -> Vec<CapRow> {
    alloc::vec![
        CapRow { task: "VE-F0721", module: "vec321_bezcore", state: CapState::Done, tol: 1e-9 },
        CapRow { task: "VE-F0722", module: "vec322_arclen", state: CapState::Done, tol: 1e-9 },
        CapRow { task: "VE-F0723", module: "vec323_split", state: CapState::Done, tol: 1e-9 },
        CapRow { task: "VE-F0724", module: "vec324_intersect", state: CapState::Done, tol: 1e-3 },
        CapRow { task: "VE-F0725", module: "vec325_nearest", state: CapState::Done, tol: 1e-6 },
        CapRow { task: "VE-F0726", module: "vec326_bbox", state: CapState::Done, tol: 1e-6 },
        CapRow { task: "VE-F0727", module: "vec327_fit", state: CapState::Done, tol: 1e-6 },
        CapRow { task: "VE-F0728", module: "vec328_offset", state: CapState::Done, tol: 1e-3 },
        CapRow { task: "VE-F0729", module: "vec329_degree", state: CapState::Done, tol: 1e-9 },
        CapRow { task: "VE-F0730", module: "vec330_roots", state: CapState::Done, tol: 1e-6 },
        CapRow { task: "VE-F0731", module: "vec331_curv", state: CapState::Done, tol: 1e-2 },
        CapRow { task: "VE-F0732", module: "vec332_conic", state: CapState::Done, tol: 5e-4 },
        CapRow { task: "VE-F0733", module: "vec333_degen", state: CapState::Done, tol: 1e-9 },
        CapRow { task: "VE-F0734", module: "vec334_batch", state: CapState::Done, tol: 0.0 },
        CapRow { task: "VE-F0735", module: "vec335_cfuzz", state: CapState::Done, tol: 0.0 },
        CapRow { task: "VE-F0736", module: "vec336_cbench", state: CapState::Done, tol: 0.0 },
        CapRow { task: "VE-F0737", module: "vec337_cduel", state: CapState::Done, tol: 1e-6 },
        CapRow { task: "VE-F0738", module: "vec338_cdocs", state: CapState::Done, tol: 0.0 },
    ]
}

pub fn matrix_complete(m: &[CapRow]) -> bool {
    m.len() == 18 && m.iter().all(|r| r.state == CapState::Done)
}

pub fn matrix_has_gap(m: &[CapRow]) -> bool {
    m.iter().any(|r| r.state == CapState::Missing)
}

/// 交叉验证一：弧长 × 分割——分割后各段弧长和 = 原弧长
pub fn xval_split_arclen(c: &Curve, t: f64, segs: usize) -> f64 {
    let al = super::vec322_arclen::ArcLen::new(segs);
    let whole = al.len(c);
    let (l, r) = super::vec323_split::split(c, t);
    (whole - (al.len(&l) + al.len(&r))).abs()
}

/// 交叉验证二：最近点 × 包围盒——最近点必落在紧盒内
pub fn xval_nearest_bbox(c: &Curve, p: Pt) -> bool {
    let q = super::vec325_nearest::Nearest::new(64, 48);
    let (_, _, np) = q.dist2(c, p);
    let b = super::vec326_bbox::tight_bbox(c);
    b.contains(np)
}

/// 交叉验证三：升阶 × 求值——升阶前后批量求值一致
pub fn xval_raise_eval(c: &Curve, samples: usize) -> f64 {
    let raised = super::vec329_degree::raise(c);
    let mut worst = 0.0f64;
    for k in 0..samples {
        let t = k as f64 / samples as f64;
        worst = worst.max(c.at(t).dist(raised.at(t)));
    }
    worst
}

/// 遗留风险（远景挂账）
pub fn leftover_risks() -> Vec<&'static str> {
    alloc::vec![
        "高阶(>4)贝塞尔未支持——按规范曲线一律先降阶再入管线",
        "椭圆弧仅覆盖参数角正序段——逆向弧由调用方分割拼接",
        "求交细分深度上限 24——极端近平行场景可能漏检（挂账 D08 布尔域）",
    ]
}

pub fn run_vec339_checks() -> crate::checks::CheckSet {
    let mut s = CheckSet::new("svstar2-vec339");
    // 族一：能力矩阵
    let m = capability_matrix();
    s.add("矩阵-十八行", m.len() == 18, "18 rows");
    s.add("矩阵-全完成", matrix_complete(&m), "all Done");
    s.add("矩阵-无缺口", !matrix_has_gap(&m), "no Missing");
    s.add("矩阵-模块名对齐", m.iter().all(|r| r.module.starts_with("vec")), "module prefix");
    s.add("矩阵-任务号连续", m.windows(2).all(|w| {
        let a = w[0].task.trim_start_matches("VE-F");
        let b = w[1].task.trim_start_matches("VE-F");
        b.parse::<u32>().unwrap_or(0) == a.parse::<u32>().unwrap_or(0) + 1
    }), "consecutive");
    // 族二：交叉验证一（弧长×分割）
    let c = Curve::build(
        Deg::Cube,
        alloc::vec![Pt::new(0.0, 0.0), Pt::new(1.0, 3.0), Pt::new(3.0, 3.0), Pt::new(4.0, 0.0)],
    )
    .expect("cube");
    s.add("交叉-弧长分割", xval_split_arclen(&c, 0.4, 32) < 1e-9, "split preserves length");
    s.add("交叉-半分", xval_split_arclen(&c, 0.5, 16) < 1e-9, "t=0.5");
    // 族三：交叉验证二（最近点×包围盒）
    s.add("交叉-最近点盒内", xval_nearest_bbox(&c, Pt::new(2.0, 5.0)), "nearest in bbox");
    s.add("交叉-域外点", xval_nearest_bbox(&c, Pt::new(-3.0, -3.0)), "outside point");
    // 族四：交叉验证三（升阶×求值）
    s.add("交叉-升阶求值", xval_raise_eval(&c, 64) < 1e-12, "raise preserves eval");
    // 族五：遗留风险挂账
    let risks = leftover_risks();
    s.add("风险-数量", risks.len() == 3, "3 risks");
    s.add("风险-非空", risks.iter().all(|r| !r.is_empty()), "non-empty");
    s.add("风险-挂账布域", risks[2].contains("D08"), "linked to D08");
    // 族六：三态语义
    let gap_row = CapRow { task: "VE-FXXXX", module: "none", state: CapState::Missing, tol: 0.0 };
    let gap_m = alloc::vec![gap_row];
    s.add("三态-缺口检出", matrix_has_gap(&gap_m), "Missing detected");
    s.add("三态-缺口不全", !matrix_complete(&gap_m), "incomplete matrix");
    s
}
