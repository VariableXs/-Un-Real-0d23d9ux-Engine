//! CGPU-F1736 · 性能与精度资产汇总
//!
//! 判据锚点（施工书原文摘录）：
//! 性能精度：总账（开销（异构增量 0.1ms/调度 0.1ms——开销表；收益（加速比/能效
//! 20%——收益表；精度（预测/画像门禁——精度表；达标对照（未达标显性；长稳（8h
//! 系列——长稳清单；测试（开销/收益/精度/对照/长稳五组）。判据：开销表、收益表、
//! 精度表、对照、长稳、五组、判据。
//!
//! 诊断码段 0xB301~0xB305（独占）：
//!   0xB301 开销失账（增量超承诺/开销表缺项）
//!   0xB302 收益失账（加速比未记/能效<20%）
//!   0xB303 精度失守（预测/画像门禁记录缺）
//!   0xB304 对照失显（未达标未显性/长稳清单缺）
//!   0xB305 段越界（判码落 0xB300..0xB400 区间外）
//!
//! 纯功能：no_std+alloc；零 unwrap/expect；全部自检在 selfcheck_* 组内。

/// 模块版本。
pub const PERF_ASSETS_VERSION: &str = "K55-perfassets-v1";

// ---------------------------------------------------------------------------
// 判码（段内独占 0xB301~0xB305）
// ---------------------------------------------------------------------------

pub const XC_PF_OVERHEAD_OVER: u32 = 0xB301;
pub const XC_PF_GAIN_LOST: u32 = 0xB302;
pub const XC_PF_PRECISION_GAP: u32 = 0xB303;
pub const XC_PF_COMPARE_HIDDEN: u32 = 0xB304;
pub const XC_PF_SEG_OOR: u32 = 0xB305;

// ---------------------------------------------------------------------------
// 一、开销表（承诺：异构增量 0.1ms / 调度 0.1ms）
// ---------------------------------------------------------------------------

/// 开销承诺恰值（µs）。
pub const OVERHEAD_HET_PROMISE_US: u32 = 100;
pub const OVERHEAD_SCHED_PROMISE_US: u32 = 100;

/// 开销表回读：实测值 ≤ 承诺值且非零。
pub fn overhead_readback(het_us: u32, sched_us: u32) -> Result<(), u32> {
    if het_us == 0 || sched_us == 0 {
        return Err(XC_PF_OVERHEAD_OVER);
    }
    if het_us > OVERHEAD_HET_PROMISE_US {
        return Err(XC_PF_OVERHEAD_OVER);
    }
    if sched_us > OVERHEAD_SCHED_PROMISE_US {
        return Err(XC_PF_OVERHEAD_OVER);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 二、收益表（加速比记录 / 能效 20%）
// ---------------------------------------------------------------------------

/// 收益账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GainLedger {
    /// 推理加速比千分比（承诺 ≥1500‰=1.5x）。
    pub speedup_permille: u32,
    /// 能效增益千分比（承诺 ≥200‰=20%）。
    pub eff_permille: u32,
}

impl GainLedger {
    pub fn sound(&self) -> Result<(), u32> {
        if self.speedup_permille < 1500 {
            return Err(XC_PF_GAIN_LOST);
        }
        if self.eff_permille < 200 {
            return Err(XC_PF_GAIN_LOST);
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 三、精度表（预测/画像门禁）
// ---------------------------------------------------------------------------

/// 精度账：预测命中率 + 画像门禁（双记录在册）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PrecisionLedger {
    /// 预测命中率千分比（承诺 ≥800‰）。
    pub predict_permille: u32,
    /// 画像门禁过（偏差带内）。
    pub profile_gate_ok: bool,
    pub anchor: &'static str,
}

impl PrecisionLedger {
    pub fn sound(&self) -> Result<(), u32> {
        if self.predict_permille < 800 || !self.profile_gate_ok || self.anchor.is_empty() {
            return Err(XC_PF_PRECISION_GAP);
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 四、达标对照（未达标显性）与长稳清单（8h 系列）
// ---------------------------------------------------------------------------

/// 对照项：承诺值/实测值/达标态（未达标必须显性——隐性即失格）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CompareItem {
    pub name: &'static str,
    pub promised: u32,
    pub measured: u32,
    /// 未达标显性标志（measured<promised 时必须 explicit=true）。
    pub explicit_when_miss: bool,
}

impl CompareItem {
    pub fn sound(&self) -> Result<(), u32> {
        if self.name.is_empty() {
            return Err(XC_PF_COMPARE_HIDDEN);
        }
        if self.measured < self.promised && !self.explicit_when_miss {
            return Err(XC_PF_COMPARE_HIDDEN);
        }
        Ok(())
    }
}

/// 长稳清单：8h 系列三查（漂移/泄漏/守恒）。
pub const SOAK_8H_CHECKS: [&str; 3] = ["drift", "leak", "conservation"];

pub fn soak_check() -> Result<(), u32> {
    if SOAK_8H_CHECKS.len() != 3 {
        return Err(XC_PF_COMPARE_HIDDEN);
    }
    for c in SOAK_8H_CHECKS.iter() {
        if c.is_empty() {
            return Err(XC_PF_COMPARE_HIDDEN);
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 自检（五组：开销/收益/精度/对照/长稳）
// ---------------------------------------------------------------------------

/// 性能精度资产引擎。
pub struct PerfAssetsEngine;

impl PerfAssetsEngine {
    pub fn selfcheck_all() -> Result<(), u32> {
        Self::segment_guard()?;
        // 组一：开销恰值 + 反例（超标拒/零值拒）。
        overhead_readback(100, 100)?;
        if overhead_readback(101, 100).is_ok() {
            return Err(XC_PF_OVERHEAD_OVER);
        }
        if overhead_readback(0, 100).is_ok() {
            return Err(XC_PF_OVERHEAD_OVER);
        }
        // 组二：收益账 + 反例（加速比不足/能效不足拒）。
        GainLedger { speedup_permille: 1500, eff_permille: 200 }.sound()?;
        if (GainLedger { speedup_permille: 1499, eff_permille: 200 }).sound().is_ok() {
            return Err(XC_PF_GAIN_LOST);
        }
        if (GainLedger { speedup_permille: 1500, eff_permille: 199 }).sound().is_ok() {
            return Err(XC_PF_GAIN_LOST);
        }
        // 组三：精度账 + 反例。
        PrecisionLedger { predict_permille: 800, profile_gate_ok: true, anchor: "K#prec" }
            .sound()?;
        if (PrecisionLedger { predict_permille: 799, profile_gate_ok: true, anchor: "K#prec" })
            .sound()
            .is_ok()
        {
            return Err(XC_PF_PRECISION_GAP);
        }
        // 组四：达标对照——达标过 + 未达标显性过 + 未达标隐性拒。
        CompareItem { name: "eff_gain", promised: 200, measured: 200, explicit_when_miss: false }
            .sound()?;
        CompareItem { name: "sr_hit", promised: 800, measured: 795, explicit_when_miss: true }
            .sound()?;
        let hidden = CompareItem {
            name: "sr_hit",
            promised: 800,
            measured: 795,
            explicit_when_miss: false,
        };
        if hidden.sound().is_ok() {
            return Err(XC_PF_COMPARE_HIDDEN);
        }
        // 组五：长稳 8h 三查。
        soak_check()?;
        Ok(())
    }

    /// 段对账：五码全落 0xB300..0xB400 开区间且互异。
    pub const fn segment_guard() -> Result<(), u32> {
        let codes = [
            XC_PF_OVERHEAD_OVER,
            XC_PF_GAIN_LOST,
            XC_PF_PRECISION_GAP,
            XC_PF_COMPARE_HIDDEN,
            XC_PF_SEG_OOR,
        ];
        let mut i = 0;
        while i < codes.len() {
            if codes[i] < 0xB300 || codes[i] >= 0xB400 {
                return Err(XC_PF_OVERHEAD_OVER);
            }
            let mut j = i + 1;
            while j < codes.len() {
                if codes[i] == codes[j] {
                    return Err(XC_PF_OVERHEAD_OVER);
                }
                j += 1;
            }
            i += 1;
        }
        Ok(())
    }
}
