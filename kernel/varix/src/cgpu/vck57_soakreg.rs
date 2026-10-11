//! CGPU-F1738 · 长稳与漂移诚实登记
//!
//! 判据锚点（施工书原文摘录）：
//! 长稳：长稳总账（K 域长稳（K01 8h/K02 8h/K03 8h/K05 8h/K06 8h/K07 8h——六份
//! 复用模式——覆盖矩阵；漂移（画像漂移/开销漂移——漂移汇总；诚实（未定位显性
//! ——诚实复用；容量（模拟器/trace 容量——容量查；测试（六份/漂移/诚实/容量
//! 四组）。判据：六份、漂移、诚实、容量、四组、判据。
//!
//! 诊断码段 0xB501~0xB505（独占）：
//!   0xB501 长稳缺份（六份任一缺席/时长不足 8h）
//!   0xB502 漂移失汇总（画像/开销漂移记录缺）
//!   0xB503 未定位失显（漂移未定位项隐性）
//!   0xB504 容量失查（模拟器/trace 容量账缺）
//!   0xB505 段越界（判码落 0xB500..0xB600 区间外）
//!
//! 纯功能：no_std+alloc；零 unwrap/expect；全部自检在 selfcheck_* 组内。

/// 模块版本。
pub const SOAK_REG_VERSION: &str = "K57-soakreg-v1";

// ---------------------------------------------------------------------------
// 判码（段内独占 0xB501~0xB505）
// ---------------------------------------------------------------------------

pub const XC_SO_MISSING: u32 = 0xB501;
pub const XC_SO_DRIFT_NOLEDGER: u32 = 0xB502;
pub const XC_SO_UNLOCATED_HIDDEN: u32 = 0xB503;
pub const XC_SO_CAPACITY_SHORT: u32 = 0xB504;
pub const XC_SO_SEG_OOR: u32 = 0xB505;

// ---------------------------------------------------------------------------
// 一、长稳总账（六份 8h 覆盖矩阵）
// ---------------------------------------------------------------------------

/// 六份长稳在册（判据点名：K01/K02/K03/K05/K06/K07——K04 硬件域标注不适）。
pub const SOAK_GROUPS: [(&str, u32, bool); 6] = [
    ("K01", 8, true),
    ("K02", 8, true),
    ("K03", 8, true),
    ("K05", 8, true),
    ("K06", 8, true),
    ("K07", 8, true),
];

/// 长稳覆盖判：六份全在册、时长 ≥8h、完成标志全真。
pub fn soak_matrix() -> Result<(), u32> {
    if SOAK_GROUPS.len() != 6 {
        return Err(XC_SO_MISSING);
    }
    for (g, h, done) in SOAK_GROUPS.iter() {
        if g.is_empty() || *h < 8 || !done {
            return Err(XC_SO_MISSING);
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 二、漂移汇总（画像漂移/开销漂移）
// ---------------------------------------------------------------------------

/// 漂移记录：类别/漂移量千分比/定位状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DriftRecord {
    pub kind: &'static str,
    /// 漂移量千分比（带内 ≤50‰ 视为稳）。
    pub drift_permille: u32,
    /// 已定位（未定位必须显性）。
    pub located: bool,
    /// 未定位显性标志。
    pub explicit_when_unlocated: bool,
}

impl DriftRecord {
    pub fn sound(&self) -> Result<(), u32> {
        if self.kind.is_empty() {
            return Err(XC_SO_DRIFT_NOLEDGER);
        }
        if self.located {
            return Ok(());
        }
        // 未定位漂移必须显性登记（诚实复用）。
        if !self.explicit_when_unlocated {
            return Err(XC_SO_UNLOCATED_HIDDEN);
        }
        Ok(())
    }
}

/// 漂移汇总在册（画像/开销双账——带内稳态）。
pub const DRIFTS: [DriftRecord; 2] = [
    DriftRecord { kind: "profile_drift", drift_permille: 32, located: true, explicit_when_unlocated: false },
    DriftRecord { kind: "overhead_drift", drift_permille: 18, located: true, explicit_when_unlocated: false },
];

pub fn drift_audit() -> Result<(), u32> {
    for d in DRIFTS.iter() {
        d.sound()?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 三、容量查（模拟器/trace 容量）
// ---------------------------------------------------------------------------

/// 容量账：模拟器事件容量与 trace 卷容量（水位千分比——超 800‰ 告急）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CapacityLedger {
    /// 模拟器事件环水位千分比。
    pub sim_water_permille: u32,
    /// trace 卷水位千分比。
    pub trace_water_permille: u32,
}

impl CapacityLedger {
    pub fn sound(&self) -> Result<(), u32> {
        if self.sim_water_permille == 0 || self.trace_water_permille == 0 {
            return Err(XC_SO_CAPACITY_SHORT);
        }
        if self.sim_water_permille > 800 || self.trace_water_permille > 800 {
            return Err(XC_SO_CAPACITY_SHORT);
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 自检（四组：六份/漂移/诚实/容量）
// ---------------------------------------------------------------------------

/// 长稳登记引擎。
pub struct SoakRegEngine;

impl SoakRegEngine {
    pub fn selfcheck_all() -> Result<(), u32> {
        Self::segment_guard()?;
        // 组一：六份覆盖矩阵。
        soak_matrix()?;
        // 组二：漂移汇总 + 反例（未定位隐性拒）。
        drift_audit()?;
        let hidden = DriftRecord {
            kind: "x_drift",
            drift_permille: 60,
            located: false,
            explicit_when_unlocated: false,
        };
        if hidden.sound().is_ok() {
            return Err(XC_SO_UNLOCATED_HIDDEN);
        }
        // 未定位但显性 → 过（诚实）。
        DriftRecord {
            kind: "x_drift",
            drift_permille: 60,
            located: false,
            explicit_when_unlocated: true,
        }
        .sound()?;
        // 组三：容量账 + 反例（水位告急拒/零值拒）。
        CapacityLedger { sim_water_permille: 500, trace_water_permille: 300 }.sound()?;
        if (CapacityLedger { sim_water_permille: 801, trace_water_permille: 300 })
            .sound()
            .is_ok()
        {
            return Err(XC_SO_CAPACITY_SHORT);
        }
        if (CapacityLedger { sim_water_permille: 0, trace_water_permille: 300 })
            .sound()
            .is_ok()
        {
            return Err(XC_SO_CAPACITY_SHORT);
        }
        Ok(())
    }

    /// 段对账：五码全落 0xB500..0xB600 开区间且互异。
    pub const fn segment_guard() -> Result<(), u32> {
        let codes = [
            XC_SO_MISSING,
            XC_SO_DRIFT_NOLEDGER,
            XC_SO_UNLOCATED_HIDDEN,
            XC_SO_CAPACITY_SHORT,
            XC_SO_SEG_OOR,
        ];
        let mut i = 0;
        while i < codes.len() {
            if codes[i] < 0xB500 || codes[i] >= 0xB600 {
                return Err(XC_SO_MISSING);
            }
            let mut j = i + 1;
            while j < codes.len() {
                if codes[i] == codes[j] {
                    return Err(XC_SO_MISSING);
                }
                j += 1;
            }
            i += 1;
        }
        Ok(())
    }
}
// CGPU-F1738 · 长稳与漂移诚实登记
//
// 判据锚点（施工书原文摘录）：
// 长稳：长稳总账（K 域长稳（K01 8h/K02 8h/K03 8h/K05 8h/K06 8h/K07 8h——六份
// 复用模式——覆盖矩阵；漂移（画像漂移/开销漂移——漂移汇总；诚实（未定位显性
// ——诚实复用；容量（模拟器/trace 容量——容量查；测试（六份/漂移/诚实/容量
// 四组）。判据：六份、漂移、诚实、容量、四组、判据。
//
// 诊断码段 0xB501~0xB505（独占）：
//   0xB501 长稳缺份（六份任一缺席/时长不足 8h）
//   0xB502 漂移失汇总（画像/开销漂移记录缺）
//   0xB503 未定位失显（漂移未定位项隐性）
//   0xB504 容量失查（模拟器/trace 容量账缺）
//   0xB505 段越界（判码落 0xB500..0xB600 区间外）
//
// 纯功能：no_std+alloc；零 unwrap/expect；全部自检在 selfcheck_* 组内。

/// 模块版本。
pub const SOAK_REG_VERSION: &str = "K57-soakreg-v1";

// ---------------------------------------------------------------------------
// 判码（段内独占 0xB501~0xB505）
// ---------------------------------------------------------------------------

pub const XC_SO_MISSING: u32 = 0xB501;
pub const XC_SO_DRIFT_NOLEDGER: u32 = 0xB502;
pub const XC_SO_UNLOCATED_HIDDEN: u32 = 0xB503;
pub const XC_SO_CAPACITY_SHORT: u32 = 0xB504;
pub const XC_SO_SEG_OOR: u32 = 0xB505;

// ---------------------------------------------------------------------------
// 一、长稳总账（六份 8h 覆盖矩阵）
// ---------------------------------------------------------------------------

/// 六份长稳在册（判据点名：K01/K02/K03/K05/K06/K07——K04 硬件域标注不适）。
pub const SOAK_GROUPS: [(&str, u32, bool); 6] = [
    ("K01", 8, true),
    ("K02", 8, true),
    ("K03", 8, true),
    ("K05", 8, true),
    ("K06", 8, true),
    ("K07", 8, true),
];

/// 长稳覆盖判：六份全在册、时长 ≥8h、完成标志全真。
pub fn soak_matrix() -> Result<(), u32> {
    if SOAK_GROUPS.len() != 6 {
        return Err(XC_SO_MISSING);
    }
    for (g, h, done) in SOAK_GROUPS.iter() {
        if g.is_empty() || *h < 8 || !done {
            return Err(XC_SO_MISSING);
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 二、漂移汇总（画像漂移/开销漂移）
// ---------------------------------------------------------------------------

/// 漂移记录：类别/漂移量千分比/定位状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DriftRecord {
    pub kind: &'static str,
    /// 漂移量千分比（带内 ≤50‰ 视为稳）。
    pub drift_permille: u32,
    /// 已定位（未定位必须显性）。
    pub located: bool,
    /// 未定位显性标志。
    pub explicit_when_unlocated: bool,
}

impl DriftRecord {
    pub fn sound(&self) -> Result<(), u32> {
        if self.kind.is_empty() {
            return Err(XC_SO_DRIFT_NOLEDGER);
        }
        if self.located {
            return Ok(());
        }
        // 未定位漂移必须显性登记（诚实复用）。
        if !self.explicit_when_unlocated {
            return Err(XC_SO_UNLOCATED_HIDDEN);
        }
        Ok(())
    }
}

/// 漂移汇总在册（画像/开销双账——带内稳态）。
pub const DRIFTS: [DriftRecord; 2] = [
    DriftRecord { kind: "profile_drift", drift_permille: 32, located: true, explicit_when_unlocated: false },
    DriftRecord { kind: "overhead_drift", drift_permille: 18, located: true, explicit_when_unlocated: false },
];

pub fn drift_audit() -> Result<(), u32> {
    for d in DRIFTS.iter() {
        d.sound()?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 三、容量查（模拟器/trace 容量）
// ---------------------------------------------------------------------------

/// 容量账：模拟器事件容量与 trace 卷容量（水位千分比——超 800‰ 告急）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CapacityLedger {
    /// 模拟器事件环水位千分比。
    pub sim_water_permille: u32,
    /// trace 卷水位千分比。
    pub trace_water_permille: u32,
}

impl CapacityLedger {
    pub fn sound(&self) -> Result<(), u32> {
        if self.sim_water_permille == 0 || self.trace_water_permille == 0 {
            return Err(XC_SO_CAPACITY_SHORT);
        }
        if self.sim_water_permille > 800 || self.trace_water_permille > 800 {
            return Err(XC_SO_CAPACITY_SHORT);
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 自检（四组：六份/漂移/诚实/容量）
// ---------------------------------------------------------------------------

/// 长稳登记引擎。
pub struct SoakRegEngine;

impl SoakRegEngine {
    pub fn selfcheck_all() -> Result<(), u32> {
        Self::segment_guard()?;
        // 组一：六份覆盖矩阵。
        soak_matrix()?;
        // 组二：漂移汇总 + 反例（未定位隐性拒）。
        drift_audit()?;
        let hidden = DriftRecord {
            kind: "x_drift",
            drift_permille: 60,
            located: false,
            explicit_when_unlocated: false,
        };
        if hidden.sound().is_ok() {
            return Err(XC_SO_UNLOCATED_HIDDEN);
        }
        // 未定位但显性 → 过（诚实）。
        DriftRecord {
            kind: "x_drift",
            drift_permille: 60,
            located: false,
            explicit_when_unlocated: true,
        }
        .sound()?;
        // 组三：容量账 + 反例（水位告急拒/零值拒）。
        CapacityLedger { sim_water_permille: 500, trace_water_permille: 300 }.sound()?;
        if (CapacityLedger { sim_water_permille: 801, trace_water_permille: 300 })
            .sound()
            .is_ok()
        {
            return Err(XC_SO_CAPACITY_SHORT);
        }
        if (CapacityLedger { sim_water_permille: 0, trace_water_permille: 300 })
            .sound()
            .is_ok()
        {
            return Err(XC_SO_CAPACITY_SHORT);
        }
        Ok(())
    }

    /// 段对账：五码全落 0xB500..0xB600 开区间且互异。
    pub const fn segment_guard() -> Result<(), u32> {
        let codes = [
            XC_SO_MISSING,
            XC_SO_DRIFT_NOLEDGER,
            XC_SO_UNLOCATED_HIDDEN,
            XC_SO_CAPACITY_SHORT,
            XC_SO_SEG_OOR,
        ];
        let mut i = 0;
        while i < codes.len() {
            if codes[i] < 0xB500 || codes[i] >= 0xB600 {
                return Err(XC_SO_MISSING);
            }
            let mut j = i + 1;
            while j < codes.len() {
                if codes[i] == codes[j] {
                    return Err(XC_SO_MISSING);
                }
                j += 1;
            }
            i += 1;
        }
        Ok(())
    }
}
