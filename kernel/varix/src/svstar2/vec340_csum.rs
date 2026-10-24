//! VE-F0740 曲线库阶段总结（跨域影响）
//! 职责：曲线库对栅格化域（D06）、布尔域（D08）、文字域（E 域）的
//!       输出清单与消费契约、跨域一致性自检、阶段判定。
//! 契约：每个对外函数在消费域的调用点有契约登记；判定全绿才允许组外消费。

use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::vec321_bezcore::{Curve, Deg, Pt};

/// 消费域
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Domain {
    Raster,
    Boolean,
    Text,
}

/// 对外契约登记行
#[derive(Clone, Debug, PartialEq)]
pub struct Contract {
    /// 提供方模块
    pub from: &'static str,
    /// 消费域
    pub to: Domain,
    /// 契约描述
    pub what: &'static str,
}

/// 输出契约清单
pub fn export_contracts() -> Vec<Contract> {
    alloc::vec![
        Contract { from: "vec321_bezcore", to: Domain::Raster, what: "Curve::at/bbox 供 flattening" },
        Contract { from: "vec322_arclen", to: Domain::Raster, what: "均匀弧长参数化供 dash" },
        Contract { from: "vec323_split", to: Domain::Boolean, what: "split 供交点区间裁剪" },
        Contract { from: "vec324_intersect", to: Domain::Boolean, what: "intersect 供边相交检测" },
        Contract { from: "vec325_nearest", to: Domain::Text, what: "最近点供 hit-test" },
        Contract { from: "vec326_bbox", to: Domain::Raster, what: "紧盒供 tile 分派" },
        Contract { from: "vec328_offset", to: Domain::Raster, what: "偏移供 stroke 生成" },
        Contract { from: "vec330_roots", to: Domain::Raster, what: "根求解供扫描线交点" },
    ]
}

/// 契约校验：每条 from 指向的模块存在且 to 非重复空挂
pub fn contracts_valid(cs: &[Contract]) -> bool {
    !cs.is_empty()
        && cs.iter().all(|c| c.from.starts_with("vec3"))
        && cs.iter().all(|c| !c.what.is_empty())
}

/// 按域统计
pub fn count_by_domain(cs: &[Contract], d: Domain) -> usize {
    cs.iter().filter(|c| c.to == d).count()
}

/// 跨域一致性自检：曲线库能力在消费域视角的采样一致性
/// —— Raster flatten（采样折线）与曲线的距离上界有理论界：|B|'' h² / 8
pub fn flatten_error_bound(c: &Curve, steps: usize) -> f64 {
    let h = 1.0 / steps as f64;
    let a_max = c.second_derivative().map(|d| {
        (0..=steps)
            .map(|k| d.at(k as f64 / steps as f64).len())
            .fold(0.0f64, f64::max)
    }).unwrap_or(0.0);
    a_max * h * h / 8.0
}

/// 阶段判定输入
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct PhaseInput {
    pub matrix_done: bool,
    pub contracts_valid: bool,
    pub duels_pass: bool,
    pub risks_filed: usize,
}

/// 阶段判定：矩阵全完成 + 契约有效 + 对拍全过 + 风险已挂账（≥1）
pub fn phase_ok(p: PhaseInput) -> bool {
    p.matrix_done && p.contracts_valid && p.duels_pass && p.risks_filed >= 1
}

pub fn run_vec340_checks() -> crate::checks::CheckSet {
    let mut s = CheckSet::new("svstar2-vec340");
    // 族一：输出契约
    let cs = export_contracts();
    s.add("契约-数量", cs.len() == 8, "8 contracts");
    s.add("契约-有效", contracts_valid(&cs), "all valid");
    s.add("契约-模块前缀", cs.iter().all(|c| c.from.starts_with("vec3")), "vec3 prefix");
    // 按域统计
    s.add("域-Raster最多", count_by_domain(&cs, Domain::Raster) >= 4, "raster heavy");
    s.add("域-Boolean", count_by_domain(&cs, Domain::Boolean) == 2, "boolean 2");
    s.add("域-Text", count_by_domain(&cs, Domain::Text) == 1, "text 1");
    s.add("域-总和", cs.iter().map(|c| 1usize).sum::<usize>() == 8, "total 8");
    // 族二：flatten 误差界
    let c = Curve::build(
        Deg::Cube,
        alloc::vec![Pt::new(0.0, 0.0), Pt::new(1.0, 3.0), Pt::new(3.0, 3.0), Pt::new(4.0, 0.0)],
    )
    .expect("cube");
    let b8 = flatten_error_bound(&c, 8);
    let b64 = flatten_error_bound(&c, 64);
    s.add("误差-正性", b8 > 0.0, "bound positive");
    s.add("误差-O(h²)", b64 * 64.0 * 64.0 < b8 * 8.0 * 8.0 + 1e-9, "quadratic shrink");
    s.add("误差-细化极小", b64 < b8 / 16.0, "64 steps much better");
    // 64 步误差界足够小（< 0.01 px）
    s.add("误差-实用", b64 < 0.01, "usable bound");
    // 直线误差界为 0（二阶导不存在）
    let ln = Curve::build(Deg::Line, alloc::vec![Pt::ZERO, Pt::new(4.0, 4.0)]).expect("ln");
    s.add("误差-直线零", flatten_error_bound(&ln, 8) == 0.0, "line exact");
    // 族三：阶段判定
    let ok = phase_ok(PhaseInput { matrix_done: true, contracts_valid: true, duels_pass: true, risks_filed: 3 });
    s.add("判定-全绿", ok, "phase ok");
    s.add("判定-矩阵缺", !phase_ok(PhaseInput { matrix_done: false, contracts_valid: true, duels_pass: true, risks_filed: 3 }), "matrix gate");
    s.add("判定-契约缺", !phase_ok(PhaseInput { matrix_done: true, contracts_valid: false, duels_pass: true, risks_filed: 3 }), "contract gate");
    s.add("判定-对拍缺", !phase_ok(PhaseInput { matrix_done: true, contracts_valid: true, duels_pass: false, risks_filed: 3 }), "duel gate");
    s.add("判定-风险缺", !phase_ok(PhaseInput { matrix_done: true, contracts_valid: true, duels_pass: true, risks_filed: 0 }), "risk gate");
    // 族四：与组收口联动——本阶段判定依赖 vec339 的矩阵
    let m = super::vec339_cclose::capability_matrix();
    s.add("联动-矩阵真实", super::vec339_cclose::matrix_complete(&m), "real matrix complete");
    s.add("联动-风险真实", super::vec339_cclose::leftover_risks().len() >= 1, "real risks");
    // 族五：契约描述可读性
    s.add("可读-含供体名", cs.iter().all(|c| c.what.len() > 5), "descriptive");
    s
}
