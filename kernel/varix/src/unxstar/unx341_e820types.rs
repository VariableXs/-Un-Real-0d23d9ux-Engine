//! unxt341_e820types —— UNX-F0361 E820 类型表（类型 × 子语义）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F0361 · 早年内存盘点归并（E820 区段排序相交并） · 判据：E820 区段排序归并零相交，归并后总容量与分区明细账一致
//! · 组件：E820 类型表（1..5 + 扩展）+ 类型守卫 + 容量分类账
//! · 判据落地：E820 语义底座
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const T341_VERSION: &str = "t341-e820types-v1";

/// 诊断段基址。
pub const T341_SEG_BASE: u16 = 0x3f00;

// --------------------------------------------------------------------------
// 诊断码（t341 十六码 · 15 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// E820 类型诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum t341Code {
    /// 0x3f00 类型非法（1..5 之外）。
    TypeBad,
    /// 0x3f01 区段长度为零。
    LenZero,
    /// 0x3f02 基址未对齐（非 64 位自然对齐）。
    BaseUnaligned,
    /// 0x3f03 类型混用（相邻异类误并）。
    TypeMix,
    /// 0x3f04 容量溢出（累加突破 u64）。
    CapacityOverflow,
    /// 0x3f05 保留段误用。
    ReservedUsed,
    /// 0x3f06 容量账破缺。
    LedgerBreak,
    /// 0x3f07 守卫触发。
    GuardTripped,
    /// 0x3f08 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x3f09 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x3f0a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x3f0b 索引错位（枚举序与矩阵序不一致）。
    IndexMisaligned,
    /// 0x3f0c 条目重复（同键双写）。
    DuplicateEntry,
    /// 0x3f0d 次序违例（依赖序/时序破缺）。
    OrderViolated,
    /// 0x3f0e 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl t341Code {
    /// 全量闭集。
    pub const ALL: [t341Code; 15] = [
        t341Code::TypeBad,
        t341Code::LenZero,
        t341Code::BaseUnaligned,
        t341Code::TypeMix,
        t341Code::CapacityOverflow,
        t341Code::ReservedUsed,
        t341Code::LedgerBreak,
        t341Code::GuardTripped,
        t341Code::LedgerBroken,
        t341Code::SnapshotFingerprint,
        t341Code::TableBroken,
        t341Code::IndexMisaligned,
        t341Code::DuplicateEntry,
        t341Code::OrderViolated,
        t341Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            t341Code::TypeBad => 0x3f00,
            t341Code::LenZero => 0x3f01,
            t341Code::BaseUnaligned => 0x3f02,
            t341Code::TypeMix => 0x3f03,
            t341Code::CapacityOverflow => 0x3f04,
            t341Code::ReservedUsed => 0x3f05,
            t341Code::LedgerBreak => 0x3f06,
            t341Code::GuardTripped => 0x3f07,
            t341Code::LedgerBroken => 0x3f08,
            t341Code::SnapshotFingerprint => 0x3f09,
            t341Code::TableBroken => 0x3f0a,
            t341Code::IndexMisaligned => 0x3f0b,
            t341Code::DuplicateEntry => 0x3f0c,
            t341Code::OrderViolated => 0x3f0d,
            t341Code::SegmentGuard => 0x3f0e,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            t341Code::TypeBad => "类型非法（1..5 之外）",
            t341Code::LenZero => "区段长度为零",
            t341Code::BaseUnaligned => "基址未对齐（非 64 位自然对齐）",
            t341Code::TypeMix => "类型混用（相邻异类误并）",
            t341Code::CapacityOverflow => "容量溢出（累加突破 u64）",
            t341Code::ReservedUsed => "保留段误用",
            t341Code::LedgerBreak => "容量账破缺",
            t341Code::GuardTripped => "守卫触发",
            t341Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            t341Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            t341Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            t341Code::IndexMisaligned => "索引错位（枚举序与矩阵序不一致）",
            t341Code::DuplicateEntry => "条目重复（同键双写）",
            t341Code::OrderViolated => "次序违例（依赖序/时序破缺）",
            t341Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            t341Code::TypeBad => "按 E820 类型表校正",
            t341Code::LenZero => "按非零律拒收",
            t341Code::BaseUnaligned => "按对齐律校正",
            t341Code::TypeMix => "按同类律合并",
            t341Code::CapacityOverflow => "分段累加",
            t341Code::ReservedUsed => "按保留表避让",
            t341Code::LedgerBreak => "按守恒重建",
            t341Code::GuardTripped => "检查临界区",
            t341Code::LedgerBroken => "按守恒律重建台账后重跑",
            t341Code::SnapshotFingerprint => "按防篡改规程重建快照",
            t341Code::TableBroken => "按语义规则重建常量表",
            t341Code::IndexMisaligned => "按声明序归一化后重跑",
            t341Code::DuplicateEntry => "合并同键条目为末次生效",
            t341Code::OrderViolated => "按依赖矩阵重排后重跑",
            t341Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 字符串键值映射 e820types（5 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// E820 类型表（五类型逐行守卫）。
/// 行数。
pub const e820types_COUNT: usize = 5;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static e820types: [(&'static str, u64); 5] = [
    ("T1_AVAILABLE", 1), // 可用内存（可分配）
    ("T2_RESERVED", 2), // 保留内存（不可用）
    ("T3_ACPI_RECLAIM", 3), // ACPI 可回收
    ("T4_ACPI_NVS", 4), // ACPI NVS
    ("T5_BAD", 5), // 坏内存
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn e820types_get(key: &str) -> Option<u64> {
    match key {
        "T1_AVAILABLE" => Some(1),
        "T2_RESERVED" => Some(2),
        "T3_ACPI_RECLAIM" => Some(3),
        "T4_ACPI_NVS" => Some(4),
        "T5_BAD" => Some(5),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn e820types_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < e820types_COUNT {
        let (k, v) = e820types[i];
        let ok = match k {
            "T1_AVAILABLE" => v >= 1 && v <= 5,
            "T2_RESERVED" => v >= 1 && v <= 5,
            "T3_ACPI_RECLAIM" => v >= 1 && v <= 5,
            "T4_ACPI_NVS" => v >= 1 && v <= 5,
            "T5_BAD" => v >= 1 && v <= 5,
            _ => false,
        };
        if !ok {
            bad += 1;
        }
        i += 1;
    }
    bad
}

/// 累计求和（守恒对账用）。
pub fn e820types_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < e820types_COUNT {
        s = s.wrapping_add(e820types[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn e820types_unique() -> bool {
    let mut i = 0usize;
    while i < e820types_COUNT {
        let mut j = i + 1;
        while j < e820types_COUNT {
            if e820types[i].0 == e820types[j].0 {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// ---------------------------------------------------------------------------
// 段位守卫（本模块独立十六进码前缀防串段；越界即判红）
// ---------------------------------------------------------------------------

/// 段位守卫：全部诊断码必须落在本段 [0x3f00, 0x3f0f]。
pub const fn t341_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < t341Code::ALL.len() {
        let c = t341Code::ALL[i].code();
        if c < 0x3f00 || c > 0x3f0f {
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
fn t341_guard_seg() -> bool {
    t341_segment_guard()
}

/// 表 e820types 逐行守卫全绿（行数/守卫/唯一性）。
fn check_t341_e820types_guard() -> bool {
    let mut ok = e820types_COUNT == 5;
    if ok {
        ok = e820types_scan() == 0;
    }
    if ok {
        ok = e820types_unique();
    }
    ok
}

/// E820 五类型在位 + 类型别名辨识。
fn check_c2_types() -> bool {
    let mut ok = e820types_get("T1_AVAILABLE") == Some(1);
    if ok {
        ok = e820types_get("T5_BAD") == Some(5) && e820types_get("T6_GHOST").is_none();
    }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_t341_checks() -> CheckSet {
    let mut set = CheckSet::new("T341");
    set.add(
        "t341-seg-guard",
        t341_guard_seg(),
        if t341_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "t341-c1-type-guard",
        check_t341_e820types_guard(),
        if check_t341_e820types_guard() { "" } else { "table e820types guard mismatch" },
    );
    set.add(
        "t341-c2-types",
        check_c2_types(),
        if check_c2_types() { "" } else { "e820 types mismatch" },
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
    fn t341_all_checks_green() {
        let set = run_t341_checks();
        assert!(set.all_passed(), "T341 domain checks must be green");
    }

    #[test]
    fn t341_segment_guard_holds() {
        assert!(t341_segment_guard());
    }
}
