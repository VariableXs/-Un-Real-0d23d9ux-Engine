//! CGPU-F1743 · 预备演练
//!
//! 判据锚点（施工书原文摘录）：
//! 预备演练：流程彩排（K10 流程干跑（五要素预生成——彩排复用；三查；正式走查
//! （K 域用户旅程（开发者接入旅程/用户能力旅程——旅程走查执行——执行记录；报告
//! （附录；测试（五要素/三查/走查/报告四组）。判据：五要素、三查、走查、报告、
//! 四组、判据。
//!
//! 诊断码段 0xBA01~0xBA05（独占）：
//!   0xBA01 五要素缺件（K10 流程干跑五要素任一缺）
//!   0xBA02 三查失守（彩排三查任一未过）
//!   0xBA03 走查缺程（双旅程任一未执行/步序断）
//!   0xBA04 报告缺附（执行记录未入附录）
//!   0xBA05 段越界（判码落 0xBA00..0xBB00 区间外）
//!
//! 纯功能：no_std+alloc；零 unwrap/expect；全部自检在 selfcheck_* 组内。

/// 模块版本。
pub const REHEARSAL_VERSION: &str = "K62-rehearsal-v1";

// ---------------------------------------------------------------------------
// 判码（段内独占 0xBA01~0xBA05）
// ---------------------------------------------------------------------------

pub const XC_RH_ELEMENTS_SHORT: u32 = 0xBA01;
pub const XC_RH_TRIPLE_FAIL: u32 = 0xBA02;
pub const XC_RH_WALK_MISSING: u32 = 0xBA03;
pub const XC_RH_APPENDIX_NOEVID: u32 = 0xBA04;
pub const XC_RH_SEG_OOR: u32 = 0xBA05;

// ---------------------------------------------------------------------------
// 一、K10 流程干跑（五要素预生成）
// ---------------------------------------------------------------------------

/// 五要素闭集（K10 收口流程输入）。
pub const FIVE_ELEMENTS: [&str; 5] = [
    "interface_ledger",
    "theme_map",
    "contract_pack",
    "asset_rollups",
    "verdict_draft",
];

/// 五要素预生成判：全非空互异（干跑预生成完成）。
pub fn five_elements_audit() -> Result<(), u32> {
    if FIVE_ELEMENTS.len() != 5 {
        return Err(XC_RH_ELEMENTS_SHORT);
    }
    let mut i = 0;
    while i < 5 {
        if FIVE_ELEMENTS[i].is_empty() {
            return Err(XC_RH_ELEMENTS_SHORT);
        }
        let mut j = i + 1;
        while j < 5 {
            if FIVE_ELEMENTS[i] == FIVE_ELEMENTS[j] {
                return Err(XC_RH_ELEMENTS_SHORT);
            }
            j += 1;
        }
        i += 1;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 二、彩排三查
// ---------------------------------------------------------------------------

/// 三查闭集（彩排质量闸）。
pub const TRIPLE_CHECKS: [&str; 3] = ["要素齐", "顺序对", "消费方可读"];

/// 三查记录。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RehearsalTriple {
    pub checks: [bool; 3],
}

impl RehearsalTriple {
    pub fn sound(&self) -> Result<(), u32> {
        for ok in self.checks.iter() {
            if !ok {
                return Err(XC_RH_TRIPLE_FAIL);
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 三、双旅程走查（开发者接入旅程 / 用户能力旅程）
// ---------------------------------------------------------------------------

/// 旅程步：步名/序号/执行标志。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct WalkStep {
    pub name: &'static str,
    pub ord: u8,
    pub executed: bool,
}

impl WalkStep {
    pub fn sound(&self) -> Result<(), u32> {
        if self.name.is_empty() || !self.executed {
            return Err(XC_RH_WALK_MISSING);
        }
        Ok(())
    }
}

/// 双旅程走查（每程四步单调执行）。
pub const DEV_JOURNEY: [WalkStep; 4] = [
    WalkStep { name: "discover_api", ord: 0, executed: true },
    WalkStep { name: "bind_adapter", ord: 1, executed: true },
    WalkStep { name: "run_sample", ord: 2, executed: true },
    WalkStep { name: "read_trace", ord: 3, executed: true },
];

pub const USER_JOURNEY: [WalkStep; 4] = [
    WalkStep { name: "open_panel", ord: 0, executed: true },
    WalkStep { name: "toggle_capability", ord: 1, executed: true },
    WalkStep { name: "read_explain", ord: 2, executed: true },
    WalkStep { name: "see_fallback", ord: 3, executed: true },
];

/// 旅程走查判：步序单调 + 全执行。
pub fn journey_audit(steps: &[WalkStep; 4]) -> Result<(), u32> {
    let mut i = 0;
    while i < 4 {
        steps[i].sound()?;
        if i + 1 < 4 && steps[i + 1].ord != steps[i].ord + 1 {
            return Err(XC_RH_WALK_MISSING);
        }
        i += 1;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 四、报告附录（执行记录入册）
// ---------------------------------------------------------------------------

/// 附录账：走查记录指纹（非零）+ 记录条数（双旅程 8 步）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AppendixLedger {
    pub walk_fingerprint: u64,
    pub record_count: u16,
}

/// FNV-1a 64。
pub const fn fnv64(buf: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    let mut i = 0;
    while i < buf.len() {
        hash ^= buf[i] as u64;
        hash = hash.wrapping_mul(0x100000001b3);
        i += 1;
    }
    hash
}

impl AppendixLedger {
    pub fn sound(&self) -> Result<(), u32> {
        if self.walk_fingerprint == 0 {
            return Err(XC_RH_APPENDIX_NOEVID);
        }
        if self.record_count != 8 {
            return Err(XC_RH_APPENDIX_NOEVID);
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 自检（四组：五要素/三查/走查/报告）
// ---------------------------------------------------------------------------

/// 预备演练引擎。
pub struct RehearsalEngine;

impl RehearsalEngine {
    pub fn selfcheck_all() -> Result<(), u32> {
        Self::segment_guard()?;
        // 组一：五要素预生成。
        five_elements_audit()?;
        // 组二：三查 + 反例（缺查拒）。
        RehearsalTriple { checks: [true, true, true] }.sound()?;
        if (RehearsalTriple { checks: [true, false, true] }).sound().is_ok() {
            return Err(XC_RH_TRIPLE_FAIL);
        }
        // 组三：双旅程走查 + 反例（断序拒/未执行拒）。
        journey_audit(&DEV_JOURNEY)?;
        journey_audit(&USER_JOURNEY)?;
        let mut torn = DEV_JOURNEY;
        torn[2].executed = false;
        if journey_audit(&torn).is_ok() {
            return Err(XC_RH_WALK_MISSING);
        }
        // 组四：报告附录 + 反例（零指纹/条数失平拒）。
        let fp = fnv64(b"rehearsal-walk-8steps");
        AppendixLedger { walk_fingerprint: fp, record_count: 8 }.sound()?;
        if (AppendixLedger { walk_fingerprint: 0, record_count: 8 }).sound().is_ok() {
            return Err(XC_RH_APPENDIX_NOEVID);
        }
        if (AppendixLedger { walk_fingerprint: fp, record_count: 7 }).sound().is_ok() {
            return Err(XC_RH_APPENDIX_NOEVID);
        }
        Ok(())
    }

    /// 段对账：五码全落 0xBA00..0xBB00 开区间且互异。
    pub const fn segment_guard() -> Result<(), u32> {
        let codes = [
            XC_RH_ELEMENTS_SHORT,
            XC_RH_TRIPLE_FAIL,
            XC_RH_WALK_MISSING,
            XC_RH_APPENDIX_NOEVID,
            XC_RH_SEG_OOR,
        ];
        let mut i = 0;
        while i < codes.len() {
            if codes[i] < 0xBA00 || codes[i] >= 0xBB00 {
                return Err(XC_RH_ELEMENTS_SHORT);
            }
            let mut j = i + 1;
            while j < codes.len() {
                if codes[i] == codes[j] {
                    return Err(XC_RH_ELEMENTS_SHORT);
                }
                j += 1;
            }
            i += 1;
        }
        Ok(())
    }
}
