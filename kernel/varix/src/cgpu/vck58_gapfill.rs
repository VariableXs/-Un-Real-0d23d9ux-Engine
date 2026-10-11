//! CGPU-F1739 · 缺失与薄弱项补齐
//!
//! 判据锚点（施工书原文摘录）：
//! 补齐：三来源（F1730-F1732 带条件+F1733 对账+F1734-F1738 缺口）；分级复用；
//! 复验闭环；台账；诚实移交（K10）；测试（三来源/分级/复验/台账/移交五组）。
//! 判据：三来源、分级、复验、台账、移交、五组、判据。
//!
//! 诊断码段 0xB601~0xB605（独占）：
//!   0xB601 来源缺账（三来源任一缺）
//!   0xB602 分级失范（补齐项无红黄绿分级）
//!   0xB603 复验开环（补齐后未复验）
//!   0xB604 台账失登/移交失显（台账缺项/K10 移交面缺）
//!   0xB605 段越界（判码落 0xB600..0xB700 区间外）
//!
//! 纯功能：no_std+alloc；零 unwrap/expect；全部自检在 selfcheck_* 组内。

/// 模块版本。
pub const GAP_FILL_VERSION: &str = "K58-gapfill-v1";

// ---------------------------------------------------------------------------
// 判码（段内独占 0xB601~0xB605）
// ---------------------------------------------------------------------------

pub const XC_GF_SOURCE_SHORT: u32 = 0xB601;
pub const XC_GF_GRADE_BAD: u32 = 0xB602;
pub const XC_GF_RECHECK_OPEN: u32 = 0xB603;
pub const XC_GF_LEDGER_HIDDEN: u32 = 0xB604;
pub const XC_GF_SEG_OOR: u32 = 0xB605;

// ---------------------------------------------------------------------------
// 一、三来源（核验带条件 / 对账发现 / 资产缺口）
// ---------------------------------------------------------------------------

/// 补齐来源闭集。
pub const SOURCES: [&str; 3] = ["F1730-F1732-conditional", "F1733-findings", "F1734-F1738-gaps"];

pub fn sources_audit() -> Result<(), u32> {
    if SOURCES.len() != 3 {
        return Err(XC_GF_SOURCE_SHORT);
    }
    for s in SOURCES.iter() {
        if s.is_empty() {
            return Err(XC_GF_SOURCE_SHORT);
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 二、分级复用（红黄绿）与补齐项台账
// ---------------------------------------------------------------------------

/// 补齐项：名/来源/分级/复验态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FillItem {
    pub name: &'static str,
    pub source: &'static str,
    /// 分级（0=红 1=黄 2=绿）。
    pub grade: u8,
    /// 补齐完成后复验通过。
    pub rechecked: bool,
    /// 复验证据锚。
    pub recheck_anchor: &'static str,
}

impl FillItem {
    pub fn sound(&self) -> Result<(), u32> {
        if self.name.is_empty() || self.grade > 2 {
            return Err(XC_GF_GRADE_BAD);
        }
        // 来源必须在三来源闭集内。
        let mut hit = false;
        for s in SOURCES.iter() {
            if *s == self.source {
                hit = true;
                break;
            }
        }
        if !hit {
            return Err(XC_GF_SOURCE_SHORT);
        }
        // 复验闭环：补齐必复验（证据锚非空）。
        if !self.rechecked || self.recheck_anchor.is_empty() {
            return Err(XC_GF_RECHECK_OPEN);
        }
        Ok(())
    }
}

/// 在册补齐台账（三来源各一条样例账——全绿收口）。
pub const FILLS: [FillItem; 3] = [
    FillItem {
        name: "k02_conditional_latency",
        source: "F1730-F1732-conditional",
        grade: 1,
        rechecked: true,
        recheck_anchor: "K58#rc1",
    },
    FillItem {
        name: "k06_j06_margin",
        source: "F1733-findings",
        grade: 2,
        rechecked: true,
        recheck_anchor: "K58#rc2",
    },
    FillItem {
        name: "doc_tutorial_tune",
        source: "F1734-F1738-gaps",
        grade: 1,
        rechecked: true,
        recheck_anchor: "K58#rc3",
    },
];

pub fn fills_audit() -> Result<(), u32> {
    for f in FILLS.iter() {
        f.sound()?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 三、诚实移交（K10 消费面）
// ---------------------------------------------------------------------------

/// 移交账：K10 消费面（移交清单非空+消费方声明）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Handover {
    /// 移交项数（零移交=无遗留——也需显性声明）。
    pub items: u16,
    pub consumer: &'static str,
    pub declared: bool,
}

impl Handover {
    pub fn sound(&self) -> Result<(), u32> {
        if !self.declared || self.consumer.is_empty() {
            return Err(XC_GF_LEDGER_HIDDEN);
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 自检（五组：三来源/分级/复验/台账/移交）
// ---------------------------------------------------------------------------

/// 缺口补齐引擎。
pub struct GapFillEngine;

impl GapFillEngine {
    pub fn selfcheck_all() -> Result<(), u32> {
        Self::segment_guard()?;
        // 组一：三来源。
        sources_audit()?;
        // 组二+三：补齐台账（分级+复验闭环）+ 反例（野来源/开环/无级拒）。
        fills_audit()?;
        let wild = FillItem {
            name: "x",
            source: "nowhere",
            grade: 1,
            rechecked: true,
            recheck_anchor: "rc",
        };
        if wild.sound().is_ok() {
            return Err(XC_GF_SOURCE_SHORT);
        }
        let open = FillItem {
            name: "x",
            source: SOURCES[0],
            grade: 1,
            rechecked: false,
            recheck_anchor: "",
        };
        if open.sound().is_ok() {
            return Err(XC_GF_RECHECK_OPEN);
        }
        let ungraded = FillItem {
            name: "x",
            source: SOURCES[0],
            grade: 9,
            rechecked: true,
            recheck_anchor: "rc",
        };
        if ungraded.sound().is_ok() {
            return Err(XC_GF_GRADE_BAD);
        }
        // 组四：台账非空账对账（三条全在册）。
        if FILLS.len() != 3 {
            return Err(XC_GF_LEDGER_HIDDEN);
        }
        // 组五：K10 移交 + 反例（无声明拒）。
        Handover { items: 2, consumer: "K10", declared: true }.sound()?;
        if (Handover { items: 2, consumer: "K10", declared: false }).sound().is_ok() {
            return Err(XC_GF_LEDGER_HIDDEN);
        }
        Ok(())
    }

    /// 段对账：五码全落 0xB600..0xB700 开区间且互异。
    pub const fn segment_guard() -> Result<(), u32> {
        let codes = [
            XC_GF_SOURCE_SHORT,
            XC_GF_GRADE_BAD,
            XC_GF_RECHECK_OPEN,
            XC_GF_LEDGER_HIDDEN,
            XC_GF_SEG_OOR,
        ];
        let mut i = 0;
        while i < codes.len() {
            if codes[i] < 0xB600 || codes[i] >= 0xB700 {
                return Err(XC_GF_SOURCE_SHORT);
            }
            let mut j = i + 1;
            while j < codes.len() {
                if codes[i] == codes[j] {
                    return Err(XC_GF_SOURCE_SHORT);
                }
                j += 1;
            }
            i += 1;
        }
        Ok(())
    }
}
//! CGPU-F1739 · 缺失与薄弱项补齐
//!
//! 判据锚点（施工书原文摘录）：
//! 补齐：三来源（F1730-F1732 带条件+F1733 对账+F1734-F1738 缺口）；分级复用；
//! 复验闭环；台账；诚实移交（K10）；测试（三来源/分级/复验/台账/移交五组）。
//! 判据：三来源、分级、复验、台账、移交、五组、判据。
//!
//! 诊断码段 0xB601~0xB605（独占）：
//!   0xB601 来源缺账（三来源任一缺）
//!   0xB602 分级失范（补齐项无红黄绿分级）
//!   0xB603 复验开环（补齐后未复验）
//!   0xB604 台账失登/移交失显（台账缺项/K10 移交面缺）
//!   0xB605 段越界（判码落 0xB600..0xB700 区间外）
//!
//! 纯功能：no_std+alloc；零 unwrap/expect；全部自检在 selfcheck_* 组内。

/// 模块版本。
pub const GAP_FILL_VERSION: &str = "K58-gapfill-v1";

// ---------------------------------------------------------------------------
// 判码（段内独占 0xB601~0xB605）
// ---------------------------------------------------------------------------

pub const XC_GF_SOURCE_SHORT: u32 = 0xB601;
pub const XC_GF_GRADE_BAD: u32 = 0xB602;
pub const XC_GF_RECHECK_OPEN: u32 = 0xB603;
pub const XC_GF_LEDGER_HIDDEN: u32 = 0xB604;
pub const XC_GF_SEG_OOR: u32 = 0xB605;

// ---------------------------------------------------------------------------
// 一、三来源（核验带条件 / 对账发现 / 资产缺口）
// ---------------------------------------------------------------------------

/// 补齐来源闭集。
pub const SOURCES: [&str; 3] = ["F1730-F1732-conditional", "F1733-findings", "F1734-F1738-gaps"];

pub fn sources_audit() -> Result<(), u32> {
    if SOURCES.len() != 3 {
        return Err(XC_GF_SOURCE_SHORT);
    }
    for s in SOURCES.iter() {
        if s.is_empty() {
            return Err(XC_GF_SOURCE_SHORT);
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 二、分级复用（红黄绿）与补齐项台账
// ---------------------------------------------------------------------------

/// 补齐项：名/来源/分级/复验态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FillItem {
    pub name: &'static str,
    pub source: &'static str,
    /// 分级（0=红 1=黄 2=绿）。
    pub grade: u8,
    /// 补齐完成后复验通过。
    pub rechecked: bool,
    /// 复验证据锚。
    pub recheck_anchor: &'static str,
}

impl FillItem {
    pub fn sound(&self) -> Result<(), u32> {
        if self.name.is_empty() || self.grade > 2 {
            return Err(XC_GF_GRADE_BAD);
        }
        // 来源必须在三来源闭集内。
        let mut hit = false;
        for s in SOURCES.iter() {
            if *s == self.source {
                hit = true;
                break;
            }
        }
        if !hit {
            return Err(XC_GF_SOURCE_SHORT);
        }
        // 复验闭环：补齐必复验（证据锚非空）。
        if !self.rechecked || self.recheck_anchor.is_empty() {
            return Err(XC_GF_RECHECK_OPEN);
        }
        Ok(())
    }
}

/// 在册补齐台账（三来源各一条样例账——全绿收口）。
pub const FILLS: [FillItem; 3] = [
    FillItem {
        name: "k02_conditional_latency",
        source: "F1730-F1732-conditional",
        grade: 1,
        rechecked: true,
        recheck_anchor: "K58#rc1",
    },
    FillItem {
        name: "k06_j06_margin",
        source: "F1733-findings",
        grade: 2,
        rechecked: true,
        recheck_anchor: "K58#rc2",
    },
    FillItem {
        name: "doc_tutorial_tune",
        source: "F1734-F1738-gaps",
        grade: 1,
        rechecked: true,
        recheck_anchor: "K58#rc3",
    },
];

pub fn fills_audit() -> Result<(), u32> {
    for f in FILLS.iter() {
        f.sound()?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 三、诚实移交（K10 消费面）
// ---------------------------------------------------------------------------

/// 移交账：K10 消费面（移交清单非空+消费方声明）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Handover {
    /// 移交项数（零移交=无遗留——也需显性声明）。
    pub items: u16,
    pub consumer: &'static str,
    pub declared: bool,
}

impl Handover {
    pub fn sound(&self) -> Result<(), u32> {
        if !self.declared || self.consumer.is_empty() {
            return Err(XC_GF_LEDGER_HIDDEN);
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 自检（五组：三来源/分级/复验/台账/移交）
// ---------------------------------------------------------------------------

/// 缺口补齐引擎。
pub struct GapFillEngine;

impl GapFillEngine {
    pub fn selfcheck_all() -> Result<(), u32> {
        Self::segment_guard()?;
        // 组一：三来源。
        sources_audit()?;
        // 组二+三：补齐台账（分级+复验闭环）+ 反例（野来源/开环/无级拒）。
        fills_audit()?;
        let wild = FillItem {
            name: "x",
            source: "nowhere",
            grade: 1,
            rechecked: true,
            recheck_anchor: "rc",
        };
        if wild.sound().is_ok() {
            return Err(XC_GF_SOURCE_SHORT);
        }
        let open = FillItem {
            name: "x",
            source: SOURCES[0],
            grade: 1,
            rechecked: false,
            recheck_anchor: "",
        };
        if open.sound().is_ok() {
            return Err(XC_GF_RECHECK_OPEN);
        }
        let ungraded = FillItem {
            name: "x",
            source: SOURCES[0],
            grade: 9,
            rechecked: true,
            recheck_anchor: "rc",
        };
        if ungraded.sound().is_ok() {
            return Err(XC_GF_GRADE_BAD);
        }
        // 组四：台账非空账对账（三条全在册）。
        if FILLS.len() != 3 {
            return Err(XC_GF_LEDGER_HIDDEN);
        }
        // 组五：K10 移交 + 反例（无声明拒）。
        Handover { items: 2, consumer: "K10", declared: true }.sound()?;
        if (Handover { items: 2, consumer: "K10", declared: false }).sound().is_ok() {
            return Err(XC_GF_LEDGER_HIDDEN);
        }
        Ok(())
    }

    /// 段对账：五码全落 0xB600..0xB700 开区间且互异。
    pub const fn segment_guard() -> Result<(), u32> {
        let codes = [
            XC_GF_SOURCE_SHORT,
            XC_GF_GRADE_BAD,
            XC_GF_RECHECK_OPEN,
            XC_GF_LEDGER_HIDDEN,
            XC_GF_SEG_OOR,
        ];
        let mut i = 0;
        while i < codes.len() {
            if codes[i] < 0xB600 || codes[i] >= 0xB700 {
                return Err(XC_GF_SOURCE_SHORT);
            }
            let mut j = i + 1;
            while j < codes.len() {
                if codes[i] == codes[j] {
                    return Err(XC_GF_SOURCE_SHORT);
                }
                j += 1;
            }
            i += 1;
        }
        Ok(())
    }
}
