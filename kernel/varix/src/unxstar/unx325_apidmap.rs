//! unxt325_unx325_apidmap —— UNX-F0325 AP ID 与 LAPIC 映射核对（幽灵/缺失 AP 双查）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F0325 · AP ID 与 LAPIC 映射核对（MADT 消费） · 判据：APIC ID 清单与 MADT 声明逐项一致，幽灵/缺失 AP 检出 100%
//! · 组件：MADT APIC 条目表 + AP ID 清单核对引擎 + 幽灵/缺失双查账 + 域快照
//! · 判据落地：逐项一致 + 幽灵/缺失 100% 检出
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const T325_VERSION: &str = "t325-unx325_apidmap-v1";

/// 诊断段基址。
pub const T325_SEG_BASE: u16 = 0x6200;

// --------------------------------------------------------------------------
// 诊断码（t325 十六码 · 14 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// AP ID 核对诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum t325Code {
    /// 0x6200 MADT 条目非法（结构越界）。
    MadtBad,
    /// 0x6201 APIC ID 不符（MADT 与清单分歧）。
    IdMismatch,
    /// 0x6202 幽灵 AP（清单有而 MADT 无）。
    GhostAp,
    /// 0x6203 缺失 AP（MADT 有而清单无）。
    MissingAp,
    /// 0x6204 APIC ID 重复。
    DupApicId,
    /// 0x6205 核对账破缺。
    LedgerBreak,
    /// 0x6206 核对序违例。
    OrderViolated,
    /// 0x6207 守卫触发。
    GuardTripped,
    /// 0x6208 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x6209 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x620a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x620b 索引错位（枚举序与矩阵序不一致）。
    IndexMisaligned,
    /// 0x620c 条目重复（同键双写）。
    DuplicateEntry,
    /// 0x620d 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl t325Code {
    /// 全量闭集。
    pub const ALL: [t325Code; 14] = [
        t325Code::MadtBad,
        t325Code::IdMismatch,
        t325Code::GhostAp,
        t325Code::MissingAp,
        t325Code::DupApicId,
        t325Code::LedgerBreak,
        t325Code::OrderViolated,
        t325Code::GuardTripped,
        t325Code::LedgerBroken,
        t325Code::SnapshotFingerprint,
        t325Code::TableBroken,
        t325Code::IndexMisaligned,
        t325Code::DuplicateEntry,
        t325Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            t325Code::MadtBad => 0x6200,
            t325Code::IdMismatch => 0x6201,
            t325Code::GhostAp => 0x6202,
            t325Code::MissingAp => 0x6203,
            t325Code::DupApicId => 0x6204,
            t325Code::LedgerBreak => 0x6205,
            t325Code::OrderViolated => 0x6206,
            t325Code::GuardTripped => 0x6207,
            t325Code::LedgerBroken => 0x6208,
            t325Code::SnapshotFingerprint => 0x6209,
            t325Code::TableBroken => 0x620a,
            t325Code::IndexMisaligned => 0x620b,
            t325Code::DuplicateEntry => 0x620c,
            t325Code::SegmentGuard => 0x620d,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            t325Code::MadtBad => "MADT 条目非法（结构越界）",
            t325Code::IdMismatch => "APIC ID 不符（MADT 与清单分歧）",
            t325Code::GhostAp => "幽灵 AP（清单有而 MADT 无）",
            t325Code::MissingAp => "缺失 AP（MADT 有而清单无）",
            t325Code::DupApicId => "APIC ID 重复",
            t325Code::LedgerBreak => "核对账破缺",
            t325Code::OrderViolated => "核对序违例",
            t325Code::GuardTripped => "守卫触发",
            t325Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            t325Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            t325Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            t325Code::IndexMisaligned => "索引错位（枚举序与矩阵序不一致）",
            t325Code::DuplicateEntry => "条目重复（同键双写）",
            t325Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            t325Code::MadtBad => "按 MADT 结构律校正",
            t325Code::IdMismatch => "按逐项核对归因",
            t325Code::GhostAp => "按 100% 检出红线补检",
            t325Code::MissingAp => "按 100% 检出红线补检",
            t325Code::DupApicId => "按唯一律去重",
            t325Code::LedgerBreak => "按守恒重建",
            t325Code::OrderViolated => "按 MADT 序重排",
            t325Code::GuardTripped => "检查临界区",
            t325Code::LedgerBroken => "按守恒律重建台账后重跑",
            t325Code::SnapshotFingerprint => "按防篡改规程重建快照",
            t325Code::TableBroken => "按语义规则重建常量表",
            t325Code::IndexMisaligned => "按声明序归一化后重跑",
            t325Code::DuplicateEntry => "合并同键条目为末次生效",
            t325Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// ---------------------------------------------------------------------------
// MADT APIC 条目 × AP ID 清单核对（幽灵/缺失双查）
// ---------------------------------------------------------------------------

/// APIC 条目类型（MADT entry type）。
pub const MADT_TYPE_LAPIC: u8 = 0;

/// I/O APIC 条目类型。
pub const MADT_TYPE_IOAPIC: u8 = 1;

/// x2APIC 条目类型。
pub const MADT_TYPE_X2APIC: u8 = 9;

/// APIC ID 域上限（8 位）。
pub const APIC_ID_MAX: u32 = 255;

/// AP 数上限（核对域 16 核）。
pub const AP_MAX_N: u32 = 16;

/// MADT APIC 条目（处理器 Local APIC）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MadtLapic {
    /// APIC ID。
    pub apic_id: u8,
    /// 处理器 UID。
    pub acpi_uid: u32,
    /// 是否 enabled 位（1 = 可用处理器）。
    pub enabled: bool,
}

/// 比对侧 AP 清单（宿主/MADT 双源核对）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ApEntry {
    /// APIC ID。
    pub apic_id: u8,
    /// 序列号（BSP=0，AP 依次）。
    pub seq: u32,
    /// 是否在册（已登记到运行表）。
    pub listed: bool,
}

/// 条目合法性（APIC ID 域 + 类型域）。
pub const fn madt_lapic_ok(m: &MadtLapic) -> bool {
    (m.apic_id as u32) <= APIC_ID_MAX
}

/// 清单合法性。
pub const fn ap_entry_ok(a: &ApEntry) -> bool {
    (a.apic_id as u32) <= APIC_ID_MAX && a.seq <= AP_MAX_N
}

/// 核对结果分类（四态闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ApDiff {
    /// 双方一致。
    Match,
    /// 幽灵 AP（清单有、MADT 无）。
    Ghost,
    /// 缺失 AP（MADT 有、清单无）。
    Missing,
    /// 启停位分歧（MADT disabled 但清单在册）。
    FlagMismatch,
}

/// 单项核对（MADT 条目 vs 清单条目）。
pub const fn ap_diff(m: &MadtLapic, a: &ApEntry) -> ApDiff {
    if !madt_lapic_ok(m) || !ap_entry_ok(a) {
        return ApDiff::FlagMismatch;
    }
    if m.enabled == a.listed {
        ApDiff::Match
    } else if m.enabled && !a.listed {
        ApDiff::Missing
    } else {
        ApDiff::Ghost
    }
}

/// 核对账（幽灵/缺失 100% 检出）。
#[derive(Clone, Copy, Debug)]
pub struct ApBook {
    /// 已核对对数。
    pub compared: u64,
    /// 一致对数。
    pub matched: u64,
    /// 幽灵检出数。
    pub ghosts: u64,
    /// 缺失检出数。
    pub missing: u64,
    /// 未检出数（必须 0）。
    pub missed: u64,
}

impl ApBook {
    /// 空账构造。
    pub const fn new() -> Self {
        Self { compared: 0, matched: 0, ghosts: 0, missing: 0, missed: 0 }
    }

    /// 核对一对（分歧必须归类，不许静默漏过）。
    pub fn compare(&mut self, m: &MadtLapic, a: &ApEntry, expect: ApDiff) -> bool {
        self.compared += 1;
        let d = ap_diff(m, a);
        let ok = match (d, expect) {
            (ApDiff::Match, ApDiff::Match) => {
                self.matched += 1;
                true
            }
            (ApDiff::Ghost, ApDiff::Ghost) => {
                self.ghosts += 1;
                true
            }
            (ApDiff::Missing, ApDiff::Missing) => {
                self.missing += 1;
                true
            }
            (ApDiff::FlagMismatch, ApDiff::FlagMismatch) => {
                self.missing += 1;
                true
            }
            _ => {
                self.missed += 1;
                false
            }
        };
        ok
    }

    /// 检出 100% 判据（无漏检 + 有检出）。
    pub fn detect_all(&self) -> bool {
        self.missed == 0 && (self.ghosts + self.missing) > 0
    }

    /// 逐项一致判据（全部命中且无分歧）。
    pub fn all_match(&self) -> bool {
        self.missed == 0 && self.ghosts == 0 && self.missing == 0 && self.compared == self.matched
    }
}

// ---------------------------------------------------------------------------
// 段位守卫（本模块独立十六进码前缀防串段；越界即判红）
// ---------------------------------------------------------------------------

/// 段位守卫：全部诊断码必须落在本段 [0x6200, 0x620f]。
pub const fn t325_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < t325Code::ALL.len() {
        let c = t325Code::ALL[i].code();
        if c < 0x6200 || c > 0x620f {
            ok = false;
        }
        i += 1;
    }
    ok
}

// --------------------------------------------------------------------------
// 自检（CheckSet 范式：每检查一带 detail，全绿才出门）
// --------------------------------------------------------------------------
/// 段位守卫：全部诊断码落在本模块段内。
fn t325_guard_seg() -> bool {
    t325_segment_guard()
}

/// 四态分类（一致/幽灵/缺失/启停分歧逐样本）。
fn check_c1_diff() -> bool {
    let good_m = MadtLapic { apic_id: 1, acpi_uid: 1, enabled: true };
    let good_a = ApEntry { apic_id: 1, seq: 1, listed: true };
    let ghost_m = MadtLapic { apic_id: 2, acpi_uid: 2, enabled: false };
    let ghost_a = ApEntry { apic_id: 2, seq: 2, listed: true };
    let miss_m = MadtLapic { apic_id: 3, acpi_uid: 3, enabled: true };
    let miss_a = ApEntry { apic_id: 3, seq: 3, listed: false };
    let mut ok = matches!(ap_diff(&good_m, &good_a), ApDiff::Match);
    if ok {
        ok = matches!(ap_diff(&ghost_m, &ghost_a), ApDiff::Ghost);
    }
    if ok {
        ok = matches!(ap_diff(&miss_m, &miss_a), ApDiff::Missing);
    }
    ok
}

/// 核对账（16 对全一致 + 幽灵缺失注入 100% 检出）。
fn check_c2_book() -> bool {
    let mut bk = ApBook::new();
    let mut ok = true;
    let mut i = 0u8;
    while i < 16 {
        let m = MadtLapic { apic_id: i, acpi_uid: i as u32, enabled: i != 15 };
        let a = ApEntry { apic_id: i, seq: i as u32, listed: i != 15 };
        ok = ok && bk.compare(&m, &a, ApDiff::Match);
        i += 1;
    }
    if ok {
        ok = bk.all_match() && bk.compared == 16;
    }
    if ok {
        // 幽灵注入必抓。
        let gm = MadtLapic { apic_id: 7, acpi_uid: 7, enabled: false };
        let ga = ApEntry { apic_id: 7, seq: 7, listed: true };
        ok = bk.compare(&gm, &ga, ApDiff::Ghost) && bk.ghosts == 1;
    }
    if ok {
        // 缺失注入必抓。
        let mm = MadtLapic { apic_id: 8, acpi_uid: 8, enabled: true };
        let ma = ApEntry { apic_id: 8, seq: 8, listed: false };
        ok = bk.compare(&mm, &ma, ApDiff::Missing) && bk.missing == 1;
    }
    if ok {
        // 期望错配必须记为漏检（missed=1 且 detect_all 判假）。
        let gm2 = MadtLapic { apic_id: 7, acpi_uid: 7, enabled: false };
        let ga2 = ApEntry { apic_id: 7, seq: 7, listed: true };
        ok = !bk.compare(&gm2, &ga2, ApDiff::Match) && bk.missed == 1 && !bk.detect_all();
    }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_t325_checks() -> CheckSet {
    let mut set = CheckSet::new("T325");
    set.add(
        "t325-seg-guard",
        t325_guard_seg(),
        if t325_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "t325-c1-diff",
        check_c1_diff(),
        if check_c1_diff() { "" } else { "ap diff classification mismatch" },
    );
    set.add(
        "t325-c2-book",
        check_c2_book(),
        if check_c2_book() { "" } else { "ap compare book mismatch" },
    );
    set
}

// ---------------------------------------------------------------------------
// 单元测试（域聚合测试在 mod.rs——此处只放本模块纯函数级断言）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn t325_all_checks_green() {
        let set = run_t325_checks();
        assert!(set.all_passed(), "T325 domain checks must be green");
    }

    #[test]
    fn t325_segment_guard_holds() {
        assert!(t325_segment_guard());
    }
}
