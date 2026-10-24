//! VE-F0630 混合模式对拍总集（VE-D 域 · 目标 360 行）
//!
//! **锚点**：`VE-STAR-II · 总纲与施工书.md#VE-F0630`
//!
//! **判据（锚点原文逐条）**：
//! - **金标准唯一**：参考实现（oracle 直译规范公式）是唯一基准；
//! - **色卡全覆盖**：每模式关键区域（零/半/一/溢出/极端背景/渐变边界）标准色卡；
//! - **1 LSB 判据**：全组沿用 F0622 确立的预算；
//! - **三归因**：公式实现错 / 精度策略错 / 路径实现错分诊；
//! - **判据**：金标准只跑对拍；失败限期修。

use crate::checks::CheckSet;
use super::ved22_separable::{SepMode, eval_scalar, oracle, DivZeroLog, LSB_BUDGET, SWATCH_PROBES};

/// 色卡矩阵：SWATCH 基础探针 × 极端背景扩展（零/一/渐变边界/溢出对）
pub fn swatch_matrix() -> alloc::vec::Vec<(f32, f32)> {
    let base: alloc::vec::Vec<(f32, f32)> = SWATCH_PROBES.iter().copied().collect();
    let extremes = alloc::vec![
        (0.0, 0.0),   // 双零
        (1.0, 1.0),   // 双一
        (0.0, 1.0),   // 全黑底全亮源
        (1.0, 0.0),   // 全白底全黑源
        (0.5, 0.0),   // 渐变边界 lo
        (0.5, 1.0),   // 渐变边界 hi
        (0.999, 0.001), // 溢出对
    ];
    let mut all = base;
    all.extend(extremes);
    all
}

/// 偏差档位判定：≤1 LSB 过 / 否则红
pub fn within_lsb(diff: f32) -> bool {
    diff <= LSB_BUDGET / 255.0
}

/// 三归因分诊（失败时定位到哪一类错）
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fault {
    /// 公式实现错：oracle 自身与规范漂移（复审规范原文）
    FormulaImpl,
    /// 精度策略错：公式对但舍入/钳制纪律破坏
    Precision,
    /// 路径实现错：单路（SIMD/GPU）特有偏差
    PathImpl,
}

impl Fault {
    pub fn label(self) -> &'static str {
        match self {
            Fault::FormulaImpl => "公式实现错",
            Fault::Precision => "精度策略错",
            Fault::PathImpl => "路径实现错",
        }
    }
}

/// 归因分诊：偏差模式 → 故障类别
/// - 全路一致偏差（标量也对不上 oracle）→ 公式或精度；
/// - 标量对 oracle 但偏差超 1 LSB 的常量性 → 精度（系统性偏移）；
/// - 仅批量/单点特有 → 路径。
pub fn triage(scalar_off: bool, simd_only_off: bool, systematic_shift: bool) -> Fault {
    if simd_only_off {
        Fault::PathImpl
    } else if scalar_off && systematic_shift {
        Fault::Precision
    } else {
        Fault::FormulaImpl
    }
}

/// 单模式全色卡对拍报告
#[derive(Clone, Debug, PartialEq)]
pub struct XCheckReport {
    pub mode: SepMode,
    pub cases: usize,
    pub fails: usize,
    /// 首个失败色卡（cb, cs, got, want）
    pub first_fail: Option<(f32, f32, f32, f32)>,
}

/// 标量路（实现路）对金标准全量对拍
pub fn xcheck_scalar(mode: SepMode) -> XCheckReport {
    let mut log = DivZeroLog::new();
    let mut rep = XCheckReport { mode, cases: 0, fails: 0, first_fail: None };
    for &(cb, cs) in &swatch_matrix() {
        rep.cases += 1;
        let got = eval_scalar(mode, cb, cs, &mut log);
        let want = oracle(mode, cb, cs);
        if !within_lsb((got - want).abs()) {
            rep.fails += 1;
            if rep.first_fail.is_none() {
                rep.first_fail = Some((cb, cs, got, want));
            }
        }
    }
    rep
}

pub fn run_vec230_checks() -> crate::checks::CheckSet {
    let mut s = CheckSet::new("svstar2-vec230");
    // 族一：色卡全覆盖
    let mat = swatch_matrix();
    s.add("对拍-色卡数", mat.len() == SWATCH_PROBES.len() + 7, "base+extreme");
    s.add("对拍-双零双一", mat.contains(&(0.0, 0.0)) && mat.contains(&(1.0, 1.0)), "corners");
    s.add("对拍-渐变边界", mat.contains(&(0.5, 0.0)) && mat.contains(&(0.5, 1.0)), "gradient edges");
    s.add("对拍-溢出对", mat.contains(&(0.999, 0.001)), "overflow pair");
    // 族二：1 LSB 判据
    s.add("对拍-预算1LSB", LSB_BUDGET == 1.0, "budget");
    s.add("对拍-域内过", within_lsb(1.0 / 255.0), "within");
    s.add("对拍-域外拒", !within_lsb(2.0 / 255.0), "beyond");
    // 族三：金标准唯一（标量路对 oracle 全模式全色卡零失败）
    let mut total_cases = 0usize;
    let mut total_fails = 0usize;
    for m in SepMode::all() {
        let rep = xcheck_scalar(m);
        total_cases += rep.cases;
        total_fails += rep.fails;
    }
    s.add("对拍-全模式零失败", total_fails == 0, "scalar vs oracle clean");
    s.add("对拍-全量跑过", total_cases == SepMode::all().len() * mat.len(), "full sweep");
    // 族四：报告结构
    let rep = xcheck_scalar(SepMode::Multiply);
    s.add("对拍-报告用例数", rep.cases == mat.len(), "report cases");
    s.add("对拍-报告零失败", rep.fails == 0 && rep.first_fail.is_none(), "clean report");
    // 族五：三归因分诊
    s.add("归因-SIMD特有→路径", triage(false, true, false) == Fault::PathImpl, "path fault");
    s.add("归因-系统偏移→精度", triage(true, false, true) == Fault::Precision, "precision fault");
    s.add("归因-其余→公式", triage(true, false, false) == Fault::FormulaImpl, "formula fault");
    s.add("归因-标签", Fault::PathImpl.label() == "路径实现错", "label");
    // 族六：oracle 单调性抽检（multiply 单调不增、screen 单调不减）
    let mut mono_ok = true;
    let mut prev_mul = 1.0f32;
    let mut prev_scr = 0.0f32;
    for i in 0..=10 {
        let v = i as f32 / 10.0;
        let m = oracle(SepMode::Multiply, v, v);
        let sc = oracle(SepMode::Screen, v, v);
        if m > prev_mul + 1e-6 || sc < prev_scr - 1e-6 {
            mono_ok = false;
        }
        prev_mul = m;
        prev_scr = sc;
    }
    s.add("对拍-oracle单调", mono_ok, "multiply↓ screen↑");
    // 族七：金标准幂等（重复求值逐位一致）
    let a = oracle(SepMode::Overlay, 0.37, 0.63);
    let b = oracle(SepMode::Overlay, 0.37, 0.63);
    s.add("对拍-oracle幂等", a == b, "deterministic");
    s
}
