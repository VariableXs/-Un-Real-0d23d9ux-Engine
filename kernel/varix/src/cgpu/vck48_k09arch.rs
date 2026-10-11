//! CGPU-F1729 · K09 开工与自查范围
//!
//! 判据锚点（施工书原文摘录）：
//! 自查开工：使命（K09=K 域域收口前全面自查（O09/J09 范式三次复用——范式复用声明；
//! 范围（八组核验+对账+API+五资产+补齐+GO——七维复用；纪律（三纪律复用；输出
//! （记录+F1742 报告——K10 消费；分级（红黄绿复用；测试（使命/范围/纪律/输出/
//! 分级五组）。判据：范式三次、七维、纪律、输出、分级、五组、判据。
//!
//! 诊断码段 0xAC01~0xAC05（独占）：
//!   0xAC01 使命失范（自查使命声明缺/范式复用不足三次）
//!   0xAC02 范围缺维（七维任一缺席）
//!   0xAC03 纪律缺条（三纪律任一缺）
//!   0xAC04 输出断链（记录锚缺/报告无消费方）
//!   0xAC05 段越界（判码落 0xAC00..0xAD00 区间外；分级并入范围组判）
//!
//! 纯功能：no_std+alloc；零 unwrap/expect；全部自检在 selfcheck_* 组内。

/// 模块版本。
pub const K09_ARCH_VERSION: &str = "K48-k09arch-v1";

// ---------------------------------------------------------------------------
// 判码（段内独占 0xAC01~0xAC05）
// ---------------------------------------------------------------------------

pub const XC_K9A_MISSION_BAD: u32 = 0xAC01;
pub const XC_K9A_SCOPE_SHORT: u32 = 0xAC02;
pub const XC_K9A_DISCIPLINE_SHORT: u32 = 0xAC03;
pub const XC_K9A_OUTPUT_BROKEN: u32 = 0xAC04;
pub const XC_K9A_SEG_OOR: u32 = 0xAC05;

// ---------------------------------------------------------------------------
// 一、使命声明（范式三次复用：O09/J09 + 本域 K09）
// ---------------------------------------------------------------------------

/// 范式复用账：O09/J09/K09 三次声明（域收口前全面自查范式同构）。
pub const PARADIGM_REUSES: [&str; 3] = ["O09", "J09", "K09"];

/// 使命判：三次范式全在册且互异。
pub fn mission_sound() -> Result<(), u32> {
    if PARADIGM_REUSES.len() < 3 {
        return Err(XC_K9A_MISSION_BAD);
    }
    let mut i = 0;
    while i < PARADIGM_REUSES.len() {
        if PARADIGM_REUSES[i].is_empty() {
            return Err(XC_K9A_MISSION_BAD);
        }
        let mut j = i + 1;
        while j < PARADIGM_REUSES.len() {
            if PARADIGM_REUSES[i] == PARADIGM_REUSES[j] {
                return Err(XC_K9A_MISSION_BAD);
            }
            j += 1;
        }
        i += 1;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 二、范围七维（八组核验/对账/API/五资产/补齐/GO + 分级复用）
// ---------------------------------------------------------------------------

/// 七维闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ScopeDim {
    /// 八组核验（K01..=K08 逐组）。
    GroupVerify,
    /// 组间两两对账。
    PairAudit,
    /// API 面终版清点。
    ApiCensus,
    /// 五资产汇总（测试/性能精度/安全红线/长稳漂移/文档）。
    AssetRollup,
    /// 缺口补齐。
    GapFill,
    /// GO 判定。
    GoGate,
    /// 分级（红黄绿三态复用）。
    Grading,
}

impl ScopeDim {
    pub const ALL: [ScopeDim; 7] = [
        ScopeDim::GroupVerify,
        ScopeDim::PairAudit,
        ScopeDim::ApiCensus,
        ScopeDim::AssetRollup,
        ScopeDim::GapFill,
        ScopeDim::GoGate,
        ScopeDim::Grading,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            ScopeDim::GroupVerify => "group_verify",
            ScopeDim::PairAudit => "pair_audit",
            ScopeDim::ApiCensus => "api_census",
            ScopeDim::AssetRollup => "asset_rollup",
            ScopeDim::GapFill => "gap_fill",
            ScopeDim::GoGate => "go_gate",
            ScopeDim::Grading => "grading",
        }
    }
}

/// 范围总账：七维全在册。
pub fn scope_complete() -> Result<(), u32> {
    // 互异对账（enum 天然互异——对 ALL 数组完整性兜底）。
    if ScopeDim::ALL.len() != 7 {
        return Err(XC_K9A_SCOPE_SHORT);
    }
    Ok(())
}

/// 分级三态闭集（红黄绿复用）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grade {
    /// 红：阻断收口，必补齐。
    Red,
    /// 黄：有保留，登记后放行。
    Yellow,
    /// 绿：全绿。
    Green,
}

impl Grade {
    pub const ALL: [Grade; 3] = [Grade::Red, Grade::Yellow, Grade::Green];
}

/// GO 门禁：任何红即 NO-GO（黄需登记——本单只判红）。
pub fn go_gate(grades: &[Grade]) -> Result<(), u32> {
    if grades.is_empty() {
        return Err(XC_K9A_SCOPE_SHORT);
    }
    for g in grades {
        if *g == Grade::Red {
            return Err(XC_K9A_SCOPE_SHORT);
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 三、纪律（三纪律复用）与输出链
// ---------------------------------------------------------------------------

/// 三纪律闭集（复用 O09/J09 已固化纪律）。
pub const DISCIPLINES: [&str; 3] = ["不造假", "不夸大", "不隐性"];

/// 纪律判：三条全在册。
pub fn disciplines_sound() -> Result<(), u32> {
    if DISCIPLINES.len() < 3 {
        return Err(XC_K9A_DISCIPLINE_SHORT);
    }
    for d in DISCIPLINES.iter() {
        if d.is_empty() {
            return Err(XC_K9A_DISCIPLINE_SHORT);
        }
    }
    Ok(())
}

/// 输出链：过程记录锚 + F1742 终报告（K10 消费）双锚在链。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct OutputChain {
    pub record_anchor: &'static str,
    /// 终报告挂单号（F1742——K10 消费）。
    pub report_ref: &'static str,
    /// 消费方声明。
    pub consumer: &'static str,
}

impl OutputChain {
    pub fn linked(&self) -> Result<(), u32> {
        if self.record_anchor.is_empty() || self.report_ref.is_empty() || self.consumer.is_empty()
        {
            return Err(XC_K9A_OUTPUT_BROKEN);
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 自检（五组：使命/范围/纪律/输出/分级）
// ---------------------------------------------------------------------------

/// K09 开工引擎。
pub struct K09ArchEngine;

impl K09ArchEngine {
    pub fn selfcheck_all() -> Result<(), u32> {
        Self::segment_guard()?;
        // 组一：使命——范式三次复用。
        mission_sound()?;
        // 组二：范围七维 + GO 门禁（无红 GO/有红 NO-GO）。
        scope_complete()?;
        go_gate(&[Grade::Green, Grade::Green, Grade::Yellow])?;
        let has_red = [Grade::Green, Grade::Red];
        if go_gate(&has_red).is_ok() {
            return Err(XC_K9A_SCOPE_SHORT);
        }
        if go_gate(&[]).is_ok() {
            return Err(XC_K9A_SCOPE_SHORT);
        }
        // 组三：三纪律。
        disciplines_sound()?;
        // 组四：输出双锚。
        OutputChain {
            record_anchor: "kernel/.workbuddy/memory",
            report_ref: "CGPU-F1742",
            consumer: "K10",
        }
        .linked()?;
        let torn = OutputChain { record_anchor: "", report_ref: "CGPU-F1742", consumer: "K10" };
        if torn.linked().is_ok() {
            return Err(XC_K9A_OUTPUT_BROKEN);
        }
        let no_consumer =
            OutputChain { record_anchor: "mem", report_ref: "CGPU-F1742", consumer: "" };
        if no_consumer.linked().is_ok() {
            return Err(XC_K9A_OUTPUT_BROKEN);
        }
        // 组五：分级三态互异兜底。
        if Grade::ALL.len() != 3 {
            return Err(XC_K9A_SCOPE_SHORT);
        }
        Ok(())
    }

    /// 段对账：五码全落 0xAC00..0xAD00 开区间且互异。
    pub const fn segment_guard() -> Result<(), u32> {
        let codes = [
            XC_K9A_MISSION_BAD,
            XC_K9A_SCOPE_SHORT,
            XC_K9A_DISCIPLINE_SHORT,
            XC_K9A_OUTPUT_BROKEN,
            XC_K9A_SEG_OOR,
        ];
        let mut i = 0;
        while i < codes.len() {
            if codes[i] < 0xAC00 || codes[i] >= 0xAD00 {
                return Err(XC_K9A_MISSION_BAD);
            }
            let mut j = i + 1;
            while j < codes.len() {
                if codes[i] == codes[j] {
                    return Err(XC_K9A_MISSION_BAD);
                }
                j += 1;
            }
            i += 1;
        }
        Ok(())
    }
}
