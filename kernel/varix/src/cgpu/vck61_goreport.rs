//! CGPU-F1742 · K 域预备报告（GO/NO-GO）
//!
//! 判据锚点（施工书原文摘录）：
//! 预备报告：八节结构复用；GO 五条件复用逐条判定；NO-GO 纪律复用；双签（K10
//! 就绪；测试（八节/五条件/纪律/双签四组）。判据：八节、五条件、纪律、双签、四组、判据。
//!
//! 诊断码段 0xB901~0xB905（独占）：
//!   0xB901 八节缺节（报告八节任一缺席）
//!   0xB902 五条件失判（GO 五条件任一未逐条判定）
//!   0xB903 纪律失守（NO-GO 纪律未挂/GO 依据不足）
//!   0xB904 双签缺席（工程/质量任一未签）
//!   0xB905 段越界（判码落 0xB900..0xBA00 区间外）
//!
//! 纯功能：no_std+alloc；零 unwrap/expect；全部自检在 selfcheck_* 组内。

/// 模块版本。
pub const GO_REPORT_VERSION: &str = "K61-goreport-v1";

// ---------------------------------------------------------------------------
// 判码（段内独占 0xB901~0xB905）
// ---------------------------------------------------------------------------

pub const XC_GR_SECTION_SHORT: u32 = 0xB901;
pub const XC_GR_COND_UNJUDGED: u32 = 0xB902;
pub const XC_GR_DISCIPLINE_LOST: u32 = 0xB903;
pub const XC_GR_SIGN_MISSING: u32 = 0xB904;
pub const XC_GR_SEG_OOR: u32 = 0xB905;

// ---------------------------------------------------------------------------
// 一、八节结构（复用惯例闭集）
// ---------------------------------------------------------------------------

/// 八节闭集。
pub const EIGHT_SECTIONS: [&str; 8] = [
    "mission",
    "scope_seven",
    "disciplines",
    "verifications",
    "assets",
    "gaps",
    "risks_prescore",
    "go_verdict",
];

/// 八节齐判：全非空互异。
pub fn sections_audit() -> Result<(), u32> {
    if EIGHT_SECTIONS.len() != 8 {
        return Err(XC_GR_SECTION_SHORT);
    }
    let mut i = 0;
    while i < 8 {
        if EIGHT_SECTIONS[i].is_empty() {
            return Err(XC_GR_SECTION_SHORT);
        }
        let mut j = i + 1;
        while j < 8 {
            if EIGHT_SECTIONS[i] == EIGHT_SECTIONS[j] {
                return Err(XC_GR_SECTION_SHORT);
            }
            j += 1;
        }
        i += 1;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 二、GO 五条件（逐条判定）
// ---------------------------------------------------------------------------

/// GO 五条件闭集（复用惯例）。
pub const GO_CONDITIONS: [&str; 5] = [
    "eight_groups_green",
    "api_74_frozen",
    "assets_complete",
    "gaps_dispositioned",
    "redline_zero_open",
];

/// 单条件判定：名 + 判定态（过=真）+ 证据锚。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CondVerdict {
    pub name: &'static str,
    pub passed: bool,
    pub evidence: &'static str,
}

impl CondVerdict {
    pub fn sound(&self) -> Result<(), u32> {
        if self.name.is_empty() || self.evidence.is_empty() {
            return Err(XC_GR_COND_UNJUDGED);
        }
        Ok(())
    }
}

/// 五条件逐条判定（在册账：全过——GO 前提）。
pub const COND_VERDICTS: [CondVerdict; 5] = [
    CondVerdict { name: GO_CONDITIONS[0], passed: true, evidence: "F1730-F1732#green" },
    CondVerdict { name: GO_CONDITIONS[1], passed: true, evidence: "F1734#74" },
    CondVerdict { name: GO_CONDITIONS[2], passed: true, evidence: "F1735-F1737#assets" },
    CondVerdict { name: GO_CONDITIONS[3], passed: true, evidence: "F1739#fills" },
    CondVerdict { name: GO_CONDITIONS[4], passed: true, evidence: "F1737#redlines" },
];

/// 五条件判：五条全判、全在册、全过、同源。
pub fn go_conditions_audit() -> Result<(), u32> {
    if COND_VERDICTS.len() != 5 {
        return Err(XC_GR_COND_UNJUDGED);
    }
    let mut i = 0;
    while i < 5 {
        let v = &COND_VERDICTS[i];
        v.sound()?;
        if !v.passed {
            return Err(XC_GR_DISCIPLINE_LOST);
        }
        // 同源对拍（与闭集同序同名）。
        if GO_CONDITIONS[i] != v.name {
            return Err(XC_GR_COND_UNJUDGED);
        }
        i += 1;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 三、NO-GO 纪律与双签
// ---------------------------------------------------------------------------

/// NO-GO 纪律：任一条件不过即 NO-GO（不得带病 GO）。
pub fn no_go_discipline(conds: &[CondVerdict]) -> Result<bool, u32> {
    for c in conds {
        c.sound()?;
        if !c.passed {
            // NO-GO 触发——纪律本身生效（返回假=不可 GO）。
            return Ok(false);
        }
    }
    Ok(true)
}

/// 双签：工程 + 质量 + K10 就绪三态（K10 消费面就绪）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DualSignK10 {
    pub eng_signed: bool,
    pub qa_signed: bool,
    pub k10_ready: bool,
}

impl DualSignK10 {
    pub fn effective(&self) -> Result<(), u32> {
        if !self.eng_signed || !self.qa_signed || !self.k10_ready {
            return Err(XC_GR_SIGN_MISSING);
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 自检（四组：八节/五条件/纪律/双签）
// ---------------------------------------------------------------------------

/// GO 报告引擎。
pub struct GoReportEngine;

impl GoReportEngine {
    pub fn selfcheck_all() -> Result<(), u32> {
        Self::segment_guard()?;
        // 组一：八节结构。
        sections_audit()?;
        // 组二：五条件逐条判定 + 反例（证据缺/失配拒）。
        go_conditions_audit()?;
        let noev = CondVerdict { name: GO_CONDITIONS[0], passed: true, evidence: "" };
        if noev.sound().is_ok() {
            return Err(XC_GR_COND_UNJUDGED);
        }
        // 组三：NO-GO 纪律——一条不过即不可 GO + 全过则可 GO。
        let mut mixed: [CondVerdict; 5] = [
            COND_VERDICTS[0], COND_VERDICTS[1], COND_VERDICTS[2], COND_VERDICTS[3],
            COND_VERDICTS[4],
        ];
        mixed[2] = CondVerdict { name: GO_CONDITIONS[2], passed: false, evidence: "缺口" };
        if no_go_discipline(&mixed)? {
            return Err(XC_GR_DISCIPLINE_LOST);
        }
        if !no_go_discipline(&COND_VERDICTS)? {
            return Err(XC_GR_DISCIPLINE_LOST);
        }
        // 组四：双签 + K10 就绪 + 反例（单签拒）。
        DualSignK10 { eng_signed: true, qa_signed: true, k10_ready: true }.effective()?;
        if (DualSignK10 { eng_signed: true, qa_signed: false, k10_ready: true })
            .effective()
            .is_ok()
        {
            return Err(XC_GR_SIGN_MISSING);
        }
        Ok(())
    }

    /// 段对账：五码全落 0xB900..0xBA00 开区间且互异。
    pub const fn segment_guard() -> Result<(), u32> {
        let codes = [
            XC_GR_SECTION_SHORT,
            XC_GR_COND_UNJUDGED,
            XC_GR_DISCIPLINE_LOST,
            XC_GR_SIGN_MISSING,
            XC_GR_SEG_OOR,
        ];
        let mut i = 0;
        while i < codes.len() {
            if codes[i] < 0xB900 || codes[i] >= 0xBA00 {
                return Err(XC_GR_SECTION_SHORT);
            }
            let mut j = i + 1;
            while j < codes.len() {
                if codes[i] == codes[j] {
                    return Err(XC_GR_SECTION_SHORT);
                }
                j += 1;
            }
            i += 1;
        }
        Ok(())
    }
}
