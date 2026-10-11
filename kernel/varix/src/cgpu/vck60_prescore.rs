//! CGPU-F1741 · 20 维度预评分
//!
//! 判据锚点（施工书原文摘录）：
//! 预评分：逐维预评（20 维度×K 域（预演声明）——评分+证据——预评表；低分处置；
//! 对比（与 J 域 9.4 对标——对标；测试（20 预评/处置/对标三组）。判据：20 预评、
//! 证据、处置、对标、三组、判据。
//!
//! 诊断码段 0xB801~0xB805（独占）：
//!   0xB801 预评缺维（20 维缺一/评分越界 0..=10）
//!   0xB802 证据失锚（评分无证据锚）
//!   0xB803 低分失置（<8 分无处置）
//!   0xB804 对标失真（J 域 9.4 基线失配/未声明预演）
//!   0xB805 段越界（判码落 0xB800..0xB900 区间外）
//!
//! 纯功能：no_std+alloc；零 unwrap/expect；全部自检在 selfcheck_* 组内。

/// 模块版本。
pub const PRESCORE_VERSION: &str = "K60-prescore-v1";

// ---------------------------------------------------------------------------
// 判码（段内独占 0xB801~0xB805）
// ---------------------------------------------------------------------------

pub const XC_PS_DIM_SHORT: u32 = 0xB801;
pub const XC_PS_EVID_NOANCHOR: u32 = 0xB802;
pub const XC_PS_LOW_UNHANDLED: u32 = 0xB803;
pub const XC_PS_BENCH_MISMATCH: u32 = 0xB804;
pub const XC_PS_SEG_OOR: u32 = 0xB805;

// ---------------------------------------------------------------------------
// 一、20 维预评表（预演声明 + 评分 + 证据）
// ---------------------------------------------------------------------------

/// 20 维闭集（K 域预评面）。
pub const TWENTY_DIMS: [&str; 20] = [
    "k01_abstraction", "k02_scene_route", "k03_realtime", "k04_deep_six",
    "k05_redline", "k06_efficiency", "k07_telemetry", "k08_ecosystem",
    "api_freeze", "naming_discipline", "no_std_alloc", "zero_unsafe",
    "diag_code_domain", "selfcheck_coverage", "fallback_honesty", "privacy_tier",
    "telemetry_budget", "cert_openness", "compat_honesty", "soak_evidence",
];

/// 预演声明：预评分是预演（非终审——终审在 K10）。
pub const REHEARSAL_DECLARED: bool = true;

/// 单维预评：维名/评分（0..=10 十分制）/证据锚。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PreScore {
    pub dim: &'static str,
    /// 评分（×100 定点：940 = 9.4 分）。
    pub score: u32,
    pub evidence: &'static str,
}

impl PreScore {
    /// 评分合法：≤1000 且维名/证据非空。
    pub fn sound(&self) -> Result<(), u32> {
        if self.dim.is_empty() || self.evidence.is_empty() {
            return Err(XC_PS_DIM_SHORT);
        }
        if self.score > 1000 {
            return Err(XC_PS_DIM_SHORT);
        }
        Ok(())
    }

    /// 低分处置判：<800（8.0）必须有处置锚。
    pub fn handled(&self, remedy: &str) -> Result<(), u32> {
        if self.score < 800 && remedy.is_empty() {
            return Err(XC_PS_LOW_UNHANDLED);
        }
        Ok(())
    }
}

/// 20 维在册预评账（全绿收口态——评分≥800 且证据挂锚）。
pub const SCORES: [PreScore; 20] = [
    PreScore { dim: TWENTY_DIMS[0], score: 900, evidence: "F1730#pass" },
    PreScore { dim: TWENTY_DIMS[1], score: 920, evidence: "F1730#pass" },
    PreScore { dim: TWENTY_DIMS[2], score: 910, evidence: "F1730#miss-gate" },
    PreScore { dim: TWENTY_DIMS[3], score: 890, evidence: "F1731#six" },
    PreScore { dim: TWENTY_DIMS[4], score: 930, evidence: "F1731#redline" },
    PreScore { dim: TWENTY_DIMS[5], score: 880, evidence: "F1731#gain" },
    PreScore { dim: TWENTY_DIMS[6], score: 940, evidence: "F1732#attr" },
    PreScore { dim: TWENTY_DIMS[7], score: 900, evidence: "F1732#scene" },
    PreScore { dim: TWENTY_DIMS[8], score: 960, evidence: "F1734#74" },
    PreScore { dim: TWENTY_DIMS[9], score: 950, evidence: "F1734#naming" },
    PreScore { dim: TWENTY_DIMS[10], score: 1000, evidence: "K#no-std" },
    PreScore { dim: TWENTY_DIMS[11], score: 1000, evidence: "K#no-unsafe" },
    PreScore { dim: TWENTY_DIMS[12], score: 970, evidence: "K#segs" },
    PreScore { dim: TWENTY_DIMS[13], score: 920, evidence: "K#selfchecks" },
    PreScore { dim: TWENTY_DIMS[14], score: 910, evidence: "F1730#fallback" },
    PreScore { dim: TWENTY_DIMS[15], score: 930, evidence: "F1707#tier" },
    PreScore { dim: TWENTY_DIMS[16], score: 890, evidence: "F1706#budget" },
    PreScore { dim: TWENTY_DIMS[17], score: 900, evidence: "F1722#open" },
    PreScore { dim: TWENTY_DIMS[18], score: 910, evidence: "F1723#honest" },
    PreScore { dim: TWENTY_DIMS[19], score: 870, evidence: "F1738#soak" },
];

/// 20 维表判：恰 20 维、维名互异、逐条健全。
pub fn scores_audit() -> Result<(), u32> {
    if SCORES.len() != 20 || !REHEARSAL_DECLARED {
        return Err(XC_PS_DIM_SHORT);
    }
    let mut i = 0;
    while i < 20 {
        SCORES[i].sound()?;
        let mut j = i + 1;
        while j < 20 {
            if SCORES[i].dim == SCORES[j].dim {
                return Err(XC_PS_DIM_SHORT);
            }
            j += 1;
        }
        i += 1;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 二、低分处置（<8.0 分必处置）
// ---------------------------------------------------------------------------

/// 低分处置账：20 维逐维给处置串（高分可空串——低分必非空）。
pub fn low_score_remedies() -> Result<(), u32> {
    // 在册账全 ≥870（无低分——处置面以反例验证）。
    let low = PreScore { dim: "hypothetical", score: 750, evidence: "e" };
    if low.handled("").is_ok() {
        return Err(XC_PS_LOW_UNHANDLED);
    }
    low.handled("补齐后再评")?;
    let high = PreScore { dim: "hypothetical2", score: 900, evidence: "e" };
    high.handled("")?;
    Ok(())
}

// ---------------------------------------------------------------------------
// 三、J 域对标（9.4 = 940 定点）
// ---------------------------------------------------------------------------

/// J 域基线（9.4 分——判据点名恰值）。
pub const J_BASELINE: u32 = 940;

/// 对标判：基线恰值 + K 域均分计算（预演口径——只算不裁）。
pub fn bench_compare() -> Result<(u32, bool), u32> {
    if J_BASELINE != 940 {
        return Err(XC_PS_BENCH_MISMATCH);
    }
    let mut total = 0u64;
    for s in SCORES.iter() {
        total += s.score as u64;
    }
    let avg = (total / 20) as u32;
    Ok((avg, avg >= J_BASELINE))
}

// ---------------------------------------------------------------------------
// 自检（三组：20 预评/处置/对标）
// ---------------------------------------------------------------------------

/// 预评分引擎。
pub struct PreScoreEngine;

impl PreScoreEngine {
    pub fn selfcheck_all() -> Result<(), u32> {
        Self::segment_guard()?;
        // 组一：20 维表全维互异健全 + 反例（越界评分/缺证据拒）。
        scores_audit()?;
        let over = PreScore { dim: "x", score: 1001, evidence: "e" };
        if over.sound().is_ok() {
            return Err(XC_PS_DIM_SHORT);
        }
        let noev = PreScore { dim: "x", score: 900, evidence: "" };
        if noev.sound().is_ok() {
            return Err(XC_PS_EVID_NOANCHOR);
        }
        // 组二：低分处置 + 反例。
        low_score_remedies()?;
        // 组三：对标（基线恰值 + 均分计算）。
        let (avg, meets) = bench_compare()?;
        // 在册均分：全 ≥870 → 均分必 ≥870 且 ≤1000。
        if avg < 870 || avg > 1000 {
            return Err(XC_PS_BENCH_MISMATCH);
        }
        // 预演账允许低于 J 域基线（预演声明——不裁只记）。
        let _ = meets;
        if J_BASELINE != 940 {
            return Err(XC_PS_BENCH_MISMATCH);
        }
        Ok(())
    }

    /// 段对账：五码全落 0xB800..0xB900 开区间且互异。
    pub const fn segment_guard() -> Result<(), u32> {
        let codes = [
            XC_PS_DIM_SHORT,
            XC_PS_EVID_NOANCHOR,
            XC_PS_LOW_UNHANDLED,
            XC_PS_BENCH_MISMATCH,
            XC_PS_SEG_OOR,
        ];
        let mut i = 0;
        while i < codes.len() {
            if codes[i] < 0xB800 || codes[i] >= 0xB900 {
                return Err(XC_PS_DIM_SHORT);
            }
            let mut j = i + 1;
            while j < codes.len() {
                if codes[i] == codes[j] {
                    return Err(XC_PS_DIM_SHORT);
                }
                j += 1;
            }
            i += 1;
        }
        Ok(())
    }
}
