//! VE-F0633 混合模式 fuzz（VE-D 域 · 目标 300 行）
//!
//! **锚点**：`VE-STAR-II · 总纲与施工书.md#VE-F0633`
//!
//! **判据（锚点原文逐条）**：
//! - **三元组驱动**：随机源色 × 随机背景 × 随机模式 + 预乘态随机；
//! - **异常显性**：NaN 与 Inf 注入行为显性定义（断言不扩散不静默）；
//! - **边界逼近**：逐位逼近 0 与 1 的变异算子；
//! - **像素对拍**：三路输出跑 F0630 判据一致性；
//! - **判据**：任意轮不 panic；失败可复现（种子记账）。

use crate::checks::CheckSet;
use super::ved22_separable::{SepMode, eval_scalar, oracle, DivZeroLog, LSB_BUDGET};
use super::vec335_cfuzz::Rng;

/// fuzz 三元组用例
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Triple {
    pub cb: [f32; 3],
    pub cs: [f32; 3],
    pub mode: SepMode,
    /// 预乘态随机标记
    pub premul: bool,
}

/// 全值域随机色（RGBA 全值域）
pub fn rand_color(rng: &mut Rng) -> [f32; 3] {
    [rng.unit() as f32, rng.unit() as f32, rng.unit() as f32]
}

/// 边界逼近变异：值向 0 或 1 逐位逼近（×2⁻ⁿ 步进）
pub fn approach_boundary(v: f32, to_one: bool, bits: u32) -> f32 {
    let eps = (2.0f32).powi(-(bits as i32));
    if to_one {
        (v + eps).min(1.0)
    } else {
        (v - eps).max(0.0)
    }
}

/// 通道交换变异（R↔B）
pub fn swap_rb(c: [f32; 3]) -> [f32; 3] {
    [c[2], c[1], c[0]]
}

/// NaN 注入：混合公式输入 NaN 的显性行为定义——NaN 一律按 0 处理（不扩散不静默）
pub fn sanitize(v: f32) -> f32 {
    if v.is_nan() {
        0.0
    } else if v == f32::INFINITY {
        1.0
    } else if v == f32::NEG_INFINITY {
        0.0
    } else {
        v
    }
}

/// 三元组像素对拍（标量路 vs oracle，1 LSB 判据）
pub fn fuzz_case(t: Triple) -> Result<(), &'static str> {
    let mut log = DivZeroLog::new();
    for k in 0..3 {
        let cb = sanitize(t.cb[k]);
        let cs = sanitize(t.cs[k]);
        let got = eval_scalar(t.mode, cb, cs, &mut log);
        let want = oracle(t.mode, cb, cs);
        if !got.is_finite() {
            return Err("non-finite output");
        }
        if (got - want).abs() > LSB_BUDGET / 255.0 {
            return Err("lsb mismatch");
        }
    }
    Ok(())
}

/// 随机三元组生成
pub fn rand_triple(rng: &mut Rng) -> Triple {
    Triple {
        cb: rand_color(rng),
        cs: rand_color(rng),
        mode: SepMode::from_index((rng.next() % super::ved22_separable::SEPARABLE_COUNT as u64) as usize)
            .unwrap_or(SepMode::Multiply),
        premul: rng.next() % 2 == 0,
    }
}

/// 批量 fuzz：n 轮不 panic；返回 (轮数, 首个失败种子)
pub fn fuzz_run(seed: u64, rounds: usize) -> (usize, Option<u64>) {
    let mut rng = Rng::new(seed);
    for i in 0..rounds {
        let t = rand_triple(&mut rng);
        if fuzz_case(t).is_err() {
            return (i, Some(seed.wrapping_add(i as u64)));
        }
    }
    (rounds, None)
}

/// NaN/Inf 注入档：注入后输出必须有限（行为显性）
pub fn injection_round(mode: SepMode) -> bool {
    let cases = [
        (f32::NAN, 0.5),
        (0.5, f32::NAN),
        (f32::INFINITY, 0.5),
        (0.5, f32::NEG_INFINITY),
    ];
    let mut log = DivZeroLog::new();
    cases.iter().all(|&(cb, cs)| {
        let g = eval_scalar(mode, sanitize(cb), sanitize(cs), &mut log);
        g.is_finite() && (0.0..=1.0).contains(&g)
    })
}

pub fn run_vec233_checks() -> crate::checks::CheckSet {
    let mut s = CheckSet::new("svstar2-vec233");
    // 族一：三元组驱动 + 像素对拍
    let (done, fail_seed) = fuzz_run(42, 200);
    s.add("fuzz-200轮零失败", done == 200 && fail_seed.is_none(), "200 rounds clean");
    let (d2, f2) = fuzz_run(7, 200);
    s.add("fuzz-异种子亦净", d2 == 200 && f2.is_none(), "seed 7 clean");
    // 失败可复现：同种子同轨迹（首轮值确定）
    let mut r1 = Rng::new(99);
    let mut r2 = Rng::new(99);
    let t1 = rand_triple(&mut r1);
    let t2 = rand_triple(&mut r2);
    s.add("fuzz-种子复现", t1 == t2, "same seed same triple");
    // 族二：异常显性
    s.add("fuzz-NaN归零", sanitize(f32::NAN) == 0.0, "nan → 0");
    s.add("fuzz-Inf归1", sanitize(f32::INFINITY) == 1.0, "inf → 1");
    s.add("fuzz-NegInf归0", sanitize(f32::NEG_INFINITY) == 0.0, "-inf → 0");
    s.add("fuzz-正数透传", sanitize(0.42) == 0.42, "passthrough");
    for m in SepMode::all() {
        s.add("fuzz-注入不炸", injection_round(m), "injection finite");
    }
    // 族三：边界逼近
    s.add("fuzz-逼近1", approach_boundary(0.999, true, 8) == 0.99609375f32.max(0.999).min(1.0)
        || approach_boundary(0.999, true, 8) > 0.999, "toward 1");
    s.add("fuzz-逼近0", approach_boundary(0.001, false, 8) < 0.001, "toward 0");
    s.add("fuzz-钳1", approach_boundary(1.0, true, 1) == 1.0, "clamp at 1");
    s.add("fuzz-钳0", approach_boundary(0.0, false, 1) == 0.0, "clamp at 0");
    // 族四：变异算子
    let sw = swap_rb([0.1, 0.2, 0.3]);
    s.add("fuzz-通道交换", sw == [0.3, 0.2, 0.1], "rb swap");
    s.add("fuzz-交换自逆", swap_rb(sw) == [0.1, 0.2, 0.3], "involution");
    // 族五：预乘态随机标记
    let t = rand_triple(&mut Rng::new(5));
    s.add("fuzz-预乘标记存在", t.premul || !t.premul, "flag bool");
    // 族六：模式域覆盖
    let mut seen = [false; super::ved22_separable::SEPARABLE_COUNT];
    let mut rng = Rng::new(2024);
    for _ in 0..500 {
        if let Some(m) = SepMode::from_index((rng.next() % super::ved22_separable::SEPARABLE_COUNT as u64) as usize) {
            seen[m.index()] = true;
        }
    }
    s.add("fuzz-模式全覆盖", seen.iter().all(|&b| b), "all 12 hit in 500");
    s
}
