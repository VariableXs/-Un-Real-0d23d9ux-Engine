//! CGPU-F1744 · K09 组收口双签
//!
//! 判据锚点（施工书原文摘录）：
//! 组收口：接口总账（K09 组 15 项接口——自查工具 API 5 函数冻结 v1）；核验汇总
//! （八组+对账 10 对+API 74 清点——总数字）；五资产确认；补齐（三数字）；预备报告
//! （GO 确认）；遗留移交（K10）；双签（K10 就绪）。判据：接口总账、核验数字、五资产、
//! GO、移交、双签、六组、判据。
//!
//! 诊断码段 0xBB01~0xBB05（独占）：
//!   0xBB01 接口总账缺项（15 项不足/自查工具 API 5 失封）
//!   0xBB02 核验数字失账（八组/10 对/74 清点总数字失配）
//!   0xBB03 五资产失认（资产面任一未确认）
//!   0xBB04 移交失显（K10 遗留移交面缺）
//!   0xBB05 段越界（判码落 0xBB00..0xBC00 区间外；GO/双签并入报告组判）
//!
//! 纯功能：no_std+alloc；零 unwrap/expect；全部自检在 selfcheck_* 组内。

/// 模块版本。
pub const K09_CLOSE_VERSION: &str = "K63-k09close-v1";

// ---------------------------------------------------------------------------
// 判码（段内独占 0xBB01~0xBB05）
// ---------------------------------------------------------------------------

pub const XC_K9C_LEDGER_SHORT: u32 = 0xBB01;
pub const XC_K9C_NUMBERS_MISMATCH: u32 = 0xBB02;
pub const XC_K9C_ASSET_UNCONFIRMED: u32 = 0xBB03;
pub const XC_K9C_HANDOVER_HIDDEN: u32 = 0xBB04;
pub const XC_K9C_SEG_OOR: u32 = 0xBB05;

// ---------------------------------------------------------------------------
// 一、K09 组 15 项接口总账（自查工具 API 5 函数冻结 v1）
// ---------------------------------------------------------------------------

/// K09 组 15 项接口清单。
pub const K09_APIS: [&str; 15] = [
    "k09_mission_declare",
    "k09_scope_seven",
    "k09_grading_gate",
    "k09_pair_audit10",
    "k09_api_census74",
    "k09_test_rollup",
    "k09_perf_rollup",
    "k09_sec_rollup",
    "k09_soak_register",
    "k09_gap_fill",
    "k09_ninedim_matrix",
    "k09_prescore20",
    "k09_go_report",
    "k09_rehearsal",
    "k09_close_dualsign",
];

/// 自查工具 API 5 函数（冻结面）。
pub const AUDIT_TOOL_APIS: [&str; 5] =
    ["selfcheck_all", "segment_guard", "ledger_fingerprint", "readback_gate", "census_verify"];

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

/// 全清单指纹（15+5 串接）。
pub fn k09_ledger_fingerprint() -> u64 {
    let mut buf = [0u8; 512];
    let mut n = 0usize;
    let mut pass = 0;
    while pass < 2 {
        let list: &[&str] = if pass == 0 { &K09_APIS } else { &AUDIT_TOOL_APIS };
        let mut i = 0;
        while i < list.len() {
            let b = list[i].as_bytes();
            let mut j = 0;
            while j < b.len() {
                buf[n] = b[j];
                n += 1;
                j += 1;
            }
            buf[n] = b'|';
            n += 1;
            i += 1;
        }
        pass += 1;
    }
    fnv64(&buf[..n])
}

/// 接口总账冻结判：15+5 恰值 + 指纹复算对拍。
pub fn k09_ledger_frozen(fp: u64) -> Result<(), u32> {
    if K09_APIS.len() != 15 || AUDIT_TOOL_APIS.len() != 5 {
        return Err(XC_K9C_LEDGER_SHORT);
    }
    for a in K09_APIS.iter() {
        if a.is_empty() {
            return Err(XC_K9C_LEDGER_SHORT);
        }
    }
    if fp != k09_ledger_fingerprint() {
        return Err(XC_K9C_LEDGER_SHORT);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 二、核验汇总总数字（八组 / 10 对 / 74 清点）
// ---------------------------------------------------------------------------

/// 核验总数字恰值对账。
pub const VERIFY_GROUPS: u16 = 8;
pub const VERIFY_PAIRS: u16 = 10;
pub const VERIFY_API_TOTAL: u16 = 74;

pub fn verify_numbers_audit(groups: u16, pairs: u16, api_total: u16) -> Result<(), u32> {
    if groups != VERIFY_GROUPS || pairs != VERIFY_PAIRS || api_total != VERIFY_API_TOTAL {
        return Err(XC_K9C_NUMBERS_MISMATCH);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 三、五资产确认与补齐三数字
// ---------------------------------------------------------------------------

/// 五资产确认账（测试/性能精度/安全红线/长稳漂移/文档）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AssetConfirms {
    pub tests: bool,
    pub perf_precision: bool,
    pub sec_redlines: bool,
    pub soak_drift: bool,
    pub docs: bool,
}

impl AssetConfirms {
    pub fn all_confirmed(&self) -> Result<(), u32> {
        if !self.tests || !self.perf_precision || !self.sec_redlines || !self.soak_drift
            || !self.docs
        {
            return Err(XC_K9C_ASSET_UNCONFIRMED);
        }
        Ok(())
    }
}

/// 补齐三数字（三来源补齐计数：核验带条件/对账发现/资产缺口）。
pub const FILLED_CONDITIONAL: u16 = 1;
pub const FILLED_FINDINGS: u16 = 1;
pub const FILLED_GAPS: u16 = 1;

pub fn fill_numbers_audit(c: u16, f: u16, g: u16) -> Result<(), u32> {
    if c != FILLED_CONDITIONAL || f != FILLED_FINDINGS || g != FILLED_GAPS {
        return Err(XC_K9C_NUMBERS_MISMATCH);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 四、GO 确认 + K10 移交 + 双签
// ---------------------------------------------------------------------------

/// K10 移交面。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct K10Handover {
    pub go_confirmed: bool,
    /// 移交清单在册（终审面 F1745+）。
    pub terminal_anchors: &'static str,
    pub consumer_declared: bool,
}

impl K10Handover {
    pub fn sound(&self) -> Result<(), u32> {
        if !self.go_confirmed || self.terminal_anchors.is_empty() || !self.consumer_declared {
            return Err(XC_K9C_HANDOVER_HIDDEN);
        }
        Ok(())
    }
}

/// 双签（K10 就绪三态——复用 F1742 口径）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DualSign {
    pub eng: bool,
    pub qa: bool,
    pub k10_ready: bool,
}

impl DualSign {
    pub fn effective(&self) -> Result<(), u32> {
        if !self.eng || !self.qa || !self.k10_ready {
            return Err(XC_K9C_HANDOVER_HIDDEN);
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 自检（六组：总账/核验数字/五资产/补齐/移交/双签）
// ---------------------------------------------------------------------------

/// K09 收口引擎。
pub struct K09CloseEngine;

impl K09CloseEngine {
    pub fn selfcheck_all() -> Result<(), u32> {
        Self::segment_guard()?;
        // 组一：接口总账冻结 + 反例（指纹失配拒）。
        k09_ledger_frozen(k09_ledger_fingerprint())?;
        if k09_ledger_frozen(k09_ledger_fingerprint() ^ 1).is_ok() {
            return Err(XC_K9C_LEDGER_SHORT);
        }
        // 组二：核验总数字 + 反例。
        verify_numbers_audit(8, 10, 74)?;
        if verify_numbers_audit(7, 10, 74).is_ok() {
            return Err(XC_K9C_NUMBERS_MISMATCH);
        }
        // 组三：五资产确认 + 反例。
        AssetConfirms { tests: true, perf_precision: true, sec_redlines: true, soak_drift: true, docs: true }
            .all_confirmed()?;
        let torn = AssetConfirms { tests: true, perf_precision: false, sec_redlines: true, soak_drift: true, docs: true };
        if torn.all_confirmed().is_ok() {
            return Err(XC_K9C_ASSET_UNCONFIRMED);
        }
        // 组四：补齐三数字 + 反例。
        fill_numbers_audit(1, 1, 1)?;
        if fill_numbers_audit(1, 0, 1).is_ok() {
            return Err(XC_K9C_NUMBERS_MISMATCH);
        }
        // 组五：K10 移交 + 反例。
        K10Handover {
            go_confirmed: true,
            terminal_anchors: "F1745-F1760",
            consumer_declared: true,
        }
        .sound()?;
        let nogo = K10Handover {
            go_confirmed: false,
            terminal_anchors: "F1745-F1760",
            consumer_declared: true,
        };
        if nogo.sound().is_ok() {
            return Err(XC_K9C_HANDOVER_HIDDEN);
        }
        // 组六：双签 + 反例。
        DualSign { eng: true, qa: true, k10_ready: true }.effective()?;
        if (DualSign { eng: true, qa: true, k10_ready: false }).effective().is_ok() {
            return Err(XC_K9C_HANDOVER_HIDDEN);
        }
        Ok(())
    }

    /// 段对账：五码全落 0xBB00..0xBC00 开区间且互异。
    pub const fn segment_guard() -> Result<(), u32> {
        let codes = [
            XC_K9C_LEDGER_SHORT,
            XC_K9C_NUMBERS_MISMATCH,
            XC_K9C_ASSET_UNCONFIRMED,
            XC_K9C_HANDOVER_HIDDEN,
            XC_K9C_SEG_OOR,
        ];
        let mut i = 0;
        while i < codes.len() {
            if codes[i] < 0xBB00 || codes[i] >= 0xBC00 {
                return Err(XC_K9C_LEDGER_SHORT);
            }
            let mut j = i + 1;
            while j < codes.len() {
                if codes[i] == codes[j] {
                    return Err(XC_K9C_LEDGER_SHORT);
                }
                j += 1;
            }
            i += 1;
        }
        Ok(())
    }
}
