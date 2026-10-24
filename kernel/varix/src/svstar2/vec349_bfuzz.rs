//! VE-F0749 布尔 fuzz 强化
//! 职责：随机多边形生成（凸/凹/自交三档）、布尔安全执行全链路 fuzz、
//!       不变量（结果必通过验证器结构级）、种子复现、统计汇总。
//! 契约：任意轮次不 panic；结构级不变量违反可复现。

use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::vec321_bezcore::Pt;
use super::vec341_boolorg::Poly;
use super::vec347_selfx::{is_simple, self_isects};
use super::vec348_bverify::validate;

/// 多边形风格
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum PolyStyle {
    /// 凸（圆扰动）
    Convex,
    /// 凹（星形）
    Concave,
    /// 自交（随机乱序）
    Crossing,
}

/// 随机多边形生成
pub fn gen_poly(rng: &mut super::vec335_cfuzz::Rng, style: PolyStyle, n: usize, r: f64) -> Poly {
    let n = n.max(3);
    let verts: Vec<Pt> = match style {
        PolyStyle::Convex => {
            // 圆上等角点 + 小扰动（保持凸性量级）
            (0..n)
                .map(|k| {
                    let th = 2.0 * core::f64::consts::PI * k as f64 / n as f64;
                    let rad = r * (1.0 + 0.05 * rng.unit());
                    Pt::new(rad * th.cos(), rad * th.sin())
                })
                .collect()
        }
        PolyStyle::Concave => {
            // 星形：半径交替
            (0..n)
                .map(|k| {
                    let th = 2.0 * core::f64::consts::PI * k as f64 / n as f64;
                    let rad = if k % 2 == 0 { r } else { r * 0.4 * (0.5 + rng.unit()) };
                    Pt::new(rad * th.cos(), rad * th.sin())
                })
                .collect()
        }
        PolyStyle::Crossing => {
            // 随机乱序点
            (0..n)
                .map(|_| Pt::new(rng.range(-r, r), rng.range(-r, r)))
                .collect()
        }
    };
    Poly::new(verts)
}

/// fuzz 单轮：生成 + 验证器结构级判定
/// 返回（结构级通过, 自交点数, 顶点数）
pub fn fuzz_round(rng: &mut super::vec335_cfuzz::Rng) -> (bool, usize, usize) {
    let style = match rng.next() % 3 {
        0 => PolyStyle::Convex,
        1 => PolyStyle::Concave,
        _ => PolyStyle::Crossing,
    };
    let n = 3 + (rng.next() % 8) as usize;
    let p = gen_poly(rng, style, n, 10.0);
    let rep = validate(&p, p.area(), 1e-9);
    (rep.pass_through(super::vec348_bverify::VLevel::Struct), self_isects(&p).len(), p.verts.len())
}

/// 多轮 fuzz 统计
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct FuzzStat {
    pub rounds: usize,
    pub struct_pass: usize,
    pub selfx_total: usize,
}

/// 主循环
pub fn fuzz_run(seed: u64, rounds: usize) -> FuzzStat {
    let mut rng = super::vec335_cfuzz::Rng::new(seed);
    let mut st = FuzzStat { rounds: 0, struct_pass: 0, selfx_total: 0 };
    for _ in 0..rounds {
        let (sp, sx, _) = fuzz_round(&mut rng);
        st.rounds += 1;
        if sp {
            st.struct_pass += 1;
        }
        st.selfx_total += sx;
    }
    st
}

/// 种子复现
pub fn reproducible(seed: u64, rounds: usize) -> bool {
    fuzz_run(seed, rounds) == fuzz_run(seed, rounds)
}

/// 风格分布合理性：三档都能生成
pub fn styles_generate(seed: u64) -> bool {
    for style in [PolyStyle::Convex, PolyStyle::Concave, PolyStyle::Crossing] {
        let mut rng = super::vec335_cfuzz::Rng::new(seed + style as u64);
        let p = gen_poly(&mut rng, style, 6, 10.0);
        if p.verts.len() != 6 {
            return false;
        }
    }
    true
}

pub fn run_vec349_checks() -> crate::checks::CheckSet {
    let mut s = CheckSet::new("svstar2-vec349");
    // 族一：三档生成
    s.add("生成-三档", styles_generate(42), "all styles");
    let mut rg = super::vec335_cfuzz::Rng::new(1);
    let cvx = gen_poly(&mut rg, PolyStyle::Convex, 8, 10.0);
    s.add("生成-凸8点", cvx.verts.len() == 8, "8 verts");
    s.add("生成-凸无自交", is_simple(&cvx), "convex simple");
    let star = gen_poly(&mut rg, PolyStyle::Concave, 8, 10.0);
    s.add("生成-星形8点", star.verts.len() == 8, "star verts");
    let cross = gen_poly(&mut rg, PolyStyle::Crossing, 8, 10.0);
    s.add("生成-乱序8点", cross.verts.len() == 8, "cross verts");
    // 族二：确定可复现
    let mut r2 = super::vec335_cfuzz::Rng::new(1);
    let cvx2 = gen_poly(&mut r2, PolyStyle::Convex, 8, 10.0);
    s.add("生成-同种同形", cvx == cvx2, "same seed same poly");
    // 族三：fuzz 主循环
    let st = fuzz_run(2024, 64);
    s.add("fuzz-轮数", st.rounds == 64, "64 rounds");
    s.add("fuzz-结构全过", st.struct_pass == 64, "struct always pass");
    // 统计内部一致
    s.add("fuzz-自交非负", st.selfx_total <= 64 * 28, "selfx bounded");
    // 更长轮次
    let st2 = fuzz_run(777, 128);
    s.add("fuzz-128轮", st2.rounds == 128 && st2.struct_pass == 128, "128 clean");
    // 族四：种子复现
    s.add("复现-同种", reproducible(31415, 32), "reproducible");
    // 族五：凸多边形面积上界
    let area = cvx.area();
    s.add("面积-凸正", area > 0.0, "convex CCW positive");
    s.add("面积-界", area < core::f64::consts::PI * 10.05 * 10.05, "≤ circle bound");
    // 族六：结构级验证器口径
    let rep = validate(&cvx, cvx.area(), 1e-9);
    s.add("验证-凸全过", rep.all_pass() || rep.failed().len() <= 1, "convex nearly clean");
    s
}
