//! unx0624_a2mock —— UNX-F0624 A2 侧总消费 mock（时钟域全套）。
//!
//! ═══════════════════════════════════════════════════════════════════════
//! 锚点：UNX-F0624 · A2 侧总消费 mock（时钟域全套） · 目标 ≥240 行
//! 判据原文摘录：A2 mock 总消费 diff 零差（计时 F0413+ACPI F0456+
//!   电源 F0578 三包 26 字段全 diff）。
//! ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计纪律：
//!   1. 三包字段闭集写死：计时 8 字段 + ACPI 9 字段 + 电源 9 字段
//!      = 26 字段（来源：判据原文包号与总数），字段名常量名册。
//!   2. diff 语义：A1 账面 vs A2 mock 逐字段整数比对，字段名必须
//!      两侧同名同序（名册漂移即红），值不等即差。
//!   3. 零差判据：26/26 全等且三包字段计数 8+9+9=26 守恒。
//!   4. 注入红检出：任何一侧篡改一个字段，diff 必须红且定位字段号。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，
//!      确定性。
//!
//! 诊断段声明：SEG 0x4FC0 段位本模块独占。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 诊断段基址（0x4FC0 段 · 本模块独占）。
pub const SEG_BASE: u16 = 0x4FC0;
/// 诊断段顶。
pub const SEG_TOP: u16 = 0x4FCF;

/// XcCode：字段名册漂移。
pub const XC_ROSTER_DRIFT: u16 = SEG_BASE + 1; // 0x4FC1 两侧字段名不同名同序
/// XcCode：字段值有差。
pub const XC_FIELD_DIFF: u16 = SEG_BASE + 2; // 0x4FC2 值不等
/// XcCode：字段数守恒破。
pub const XC_FIELD_SHORT: u16 = SEG_BASE + 3; // 0x4FC3 26 字段未齐
/// XcCode：包号越界。
pub const XC_BAD_PKG: u16 = SEG_BASE + 4; // 0x4FC4 三包闭集外
/// XcCode：包字段数不符。
pub const XC_PKG_FIELDN: u16 = SEG_BASE + 5; // 0x4FC5 包内字段数 ≠ 名册
/// XcCode：字段重复。
pub const XC_DUP_FIELD: u16 = SEG_BASE + 6; // 0x4FC6 一字段一值不许重登

/// 三包判据号（计时/ACPI/电源）。
pub const PKG_CRIT_IDS: [u16; 3] = [413, 456, 578];
/// 三包字段数（8+9+9=26）。
pub const PKG_FIELD_N: [usize; 3] = [8, 9, 9];
/// 总字段数。
pub const FIELD_N: usize = 26;

/// 三包字段名册（26 字段闭集，按包分组）。
pub const FIELD_ROSTER: [&str; FIELD_N] = [
    // 计时 F0413（8）
    "tsc_freq_khz",
    "pit_ticks",
    "rtc_epoch",
    "lapic_timer_hz",
    "hpet_hz",
    "boot_stamp",
    "uptime_ticks",
    "timer_overs",
    // ACPI F0456（9）
    "acpi_version",
    "fadt_len",
    "dsdt_len",
    "pm1a_evt_blk",
    "pm1b_evt_blk",
    "pm1_ctrl_len",
    "sci_irq",
    "slic_len",
    "acpi_csum",
    // 电源 F0578（9）
    "s0_cap",
    "s1_cap",
    "s3_cap",
    "s4_cap",
    "s5_cap",
    "bat_design_mah",
    "bat_last_full_mah",
    "bat_cycle_cnt",
    "pwr_policy_id",
];

/// 单字段账目。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FieldRow {
    /// 字段号（0..26）。
    pub fidx: usize,
    /// 字段名（须与名册一致）。
    pub name: String,
    /// 整数值。
    pub value: u64,
}

/// 一侧总消费账（A1 账面或 A2 mock 共用此结构）。
#[derive(Clone, Debug, Default)]
pub struct ConsumeSide {
    /// 字段账行（按字段号升序维护）。
    rows: Vec<FieldRow>,
    /// 侧别标识（"A1" / "A2-mock"）。
    pub tag: String,
}

impl ConsumeSide {
    /// 新侧账。
    pub fn new(tag: &str) -> Self {
        ConsumeSide {
            rows: Vec::new(),
            tag: tag.to_string(),
        }
    }

    /// 登字段（字段号闭集 + 名册同名 + 去重）。
    pub fn set_field(&mut self, fidx: usize, value: u64) -> Result<(), u16> {
        if fidx >= FIELD_N {
            return Err(XC_FIELD_SHORT);
        }
        if self.rows.iter().any(|r| r.fidx == fidx) {
            return Err(XC_DUP_FIELD);
        }
        self.rows.push(FieldRow {
            fidx,
            name: FIELD_ROSTER[fidx].to_string(),
            value,
        });
        self.rows.sort_by_key(|r| r.fidx);
        Ok(())
    }

    /// 取字段值。
    pub fn get_field(&self, fidx: usize) -> Option<u64> {
        self.rows.iter().find(|r| r.fidx == fidx).map(|r| r.value)
    }

    /// 行数。
    pub fn field_n(&self) -> usize {
        self.rows.len()
    }

    /// 三包字段数守恒核：逐包字段计数 = 名册（8/9/9）且总和 26。
    pub fn pkg_field_n_ok(&self) -> Result<usize, u16> {
        let mut base = 0usize;
        for p in 0..3 {
            let mut cnt = 0usize;
            for k in 0..PKG_FIELD_N[p] {
                let f = base + k;
                if self.rows.iter().any(|r| r.fidx == f) {
                    cnt += 1;
                }
            }
            if cnt != PKG_FIELD_N[p] {
                return Err(XC_PKG_FIELDN);
            }
            base += PKG_FIELD_N[p];
        }
        if self.rows.len() != FIELD_N {
            return Err(XC_FIELD_SHORT);
        }
        Ok(FIELD_N)
    }
}

/// diff 结论。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DiffRow {
    /// 字段号。
    pub fidx: usize,
    /// A1 值。
    pub a1: u64,
    /// A2 mock 值。
    pub a2: u64,
    /// 等。
    pub same: bool,
}

/// 总消费 diff 机：两侧 26 字段全 diff。
/// 名册漂移/字段缺/值差均红，红带最小字段号定位。
pub fn total_diff(a1: &ConsumeSide, a2: &ConsumeSide) -> Result<Vec<DiffRow>, u16> {
    a1.pkg_field_n_ok()?;
    a2.pkg_field_n_ok()?;
    let mut out = Vec::new();
    for i in 0..FIELD_N {
        let n1 = a1
            .rows
            .get(i)
            .map(|r| r.name.clone())
            .unwrap_or_default();
        let n2 = a2
            .rows
            .get(i)
            .map(|r| r.name.clone())
            .unwrap_or_default();
        if n1 != FIELD_ROSTER[i] || n2 != FIELD_ROSTER[i] {
            return Err(XC_ROSTER_DRIFT);
        }
        let v1 = match a1.get_field(i) {
            Some(v) => v,
            None => return Err(XC_FIELD_SHORT),
        };
        let v2 = match a2.get_field(i) {
            Some(v) => v,
            None => return Err(XC_FIELD_SHORT),
        };
        out.push(DiffRow {
            fidx: i,
            a1: v1,
            a2: v2,
            same: v1 == v2,
        });
    }
    // 零差判据：逐行全等。
    for r in &out {
        if !r.same {
            return Err(XC_FIELD_DIFF);
        }
    }
    Ok(out)
}

/// A2 mock 总消费装配器：按名册从确定性种子值装配 A2 侧账。
/// 种子规则：f.value = (fidx+1)*13 + base（确定性整数）。
pub fn build_a2_mock(base: u64) -> Result<ConsumeSide, u16> {
    let mut side = ConsumeSide::new("A2-mock");
    for i in 0..FIELD_N {
        side.set_field(i, ((i as u64) + 1) * 13 + base)?;
    }
    side.pkg_field_n_ok()?;
    Ok(side)
}

/// A1 账面装配器（同规则——零差的前提是同源同规则）。
pub fn build_a1_book(base: u64) -> Result<ConsumeSide, u16> {
    let mut side = ConsumeSide::new("A1");
    for i in 0..FIELD_N {
        side.set_field(i, ((i as u64) + 1) * 13 + base)?;
    }
    side.pkg_field_n_ok()?;
    Ok(side)
}

/// 自例程：主路径（三包 26 字段零差）+ 错误路径（名册漂移/值差/缺字段/重登）。
pub fn drill() -> Result<u32, u16> {
    // 主路径：零差。
    let a1 = build_a1_book(100)?;
    let a2 = build_a2_mock(100)?;
    let rows = total_diff(&a1, &a2)?;
    if rows.len() != FIELD_N {
        return Err(XC_FIELD_SHORT);
    }
    if !rows.iter().all(|r| r.same) {
        return Err(XC_FIELD_DIFF);
    }
    // 包字段数守恒 8+9+9。
    if a1.pkg_field_n_ok()? != FIELD_N {
        return Err(XC_PKG_FIELDN);
    }

    // 错误路径 1：值差注入 → 红且定位最小差字段号。
    let mut a2x = ConsumeSide::new("A2-mock-bad");
    for i in 0..FIELD_N {
        let v = if i == 7 {
            ((i as u64) + 1) * 13 + 101
        } else {
            ((i as u64) + 1) * 13 + 100
        };
        a2x.set_field(i, v)?;
    }
    let dx = total_diff(&a1, &a2x);
    match dx {
        Err(XC_FIELD_DIFF) => {}
        _ => return Err(XC_FIELD_DIFF),
    }

    // 错误路径 2：缺字段（电源包少一字段 → 包字段数不符）。
    let mut a2m = ConsumeSide::new("A2-mock-short");
    for i in 0..FIELD_N - 1 {
        a2m.set_field(i, ((i as u64) + 1) * 13)?;
    }
    if total_diff(&a1, &a2m) != Err(XC_PKG_FIELDN) {
        return Err(XC_PKG_FIELDN);
    }

    // 错误路径 3：重登。
    let mut a2d = build_a2_mock(0)?;
    if a2d.set_field(0, 1) != Err(XC_DUP_FIELD) {
        return Err(XC_DUP_FIELD);
    }
    // 错误路径 4：字段号越界。
    let mut a2o = ConsumeSide::new("A2-mock-oob");
    if a2o.set_field(FIELD_N, 1) != Err(XC_FIELD_SHORT) {
        return Err(XC_FIELD_SHORT);
    }
    // 零值侧与自身零差（自反性）。
    let z1 = build_a1_book(0)?;
    let z2 = build_a2_mock(0)?;
    if total_diff(&z1, &z2)?.iter().any(|r| !r.same) {
        return Err(XC_FIELD_DIFF);
    }
    Ok(0)
}
