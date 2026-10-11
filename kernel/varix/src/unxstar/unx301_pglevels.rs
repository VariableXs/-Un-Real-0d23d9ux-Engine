//! unxt301_pglevels —— UNX-F0301 四级页表族（级别 × 位域语义）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F0301 · 页表构建总编排（四级表族流水） · 判据：PML4→PDPT→PD→PT 四级构建流水编排正确，依赖序与设计一致，构建账完整
//! · 组件：四级页表族表（PML4/PDPT/PD/PT × 位域）+ 位域守卫 + 级别索引对拍
//! · 判据落地：四级表族的语义底座
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const T301_VERSION: &str = "t301-pglevels-v1";

/// 诊断段基址。
pub const T301_SEG_BASE: u16 = 0x3c00;

// --------------------------------------------------------------------------
// 诊断码（t301 十六码 · 15 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 页表族诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum t301Code {
    /// 0x3c00 级别非法（0..3 之外）。
    LevelBad,
    /// 0x3c01 位域非法（起止位倒挂）。
    BitFieldBad,
    /// 0x3c02 位移重叠（邻级位域交叠）。
    ShiftOverlap,
    /// 0x3c03 规范位违例（VA 高位不全同号）。
    CanonBad,
    /// 0x3c04 页尺寸混用（同级混 4K/2M/1G）。
    HugeMixBad,
    /// 0x3c05 表项位非法（保留位置位）。
    EntryBitsBad,
    /// 0x3c06 NX 违规（可执行数据页）。
    NxViolate,
    /// 0x3c07 页表族表破坏。
    TableBroken,
    /// 0x3c08 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x3c09 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x3c0a 索引错位（枚举序与矩阵序不一致）。
    IndexMisaligned,
    /// 0x3c0b 条目重复（同键双写）。
    DuplicateEntry,
    /// 0x3c0c 次序违例（依赖序/时序破缺）。
    OrderViolated,
    /// 0x3c0d 守卫触发（重入/越限被拒）。
    GuardTripped,
    /// 0x3c0e 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl t301Code {
    /// 全量闭集。
    pub const ALL: [t301Code; 15] = [
        t301Code::LevelBad,
        t301Code::BitFieldBad,
        t301Code::ShiftOverlap,
        t301Code::CanonBad,
        t301Code::HugeMixBad,
        t301Code::EntryBitsBad,
        t301Code::NxViolate,
        t301Code::TableBroken,
        t301Code::LedgerBroken,
        t301Code::SnapshotFingerprint,
        t301Code::IndexMisaligned,
        t301Code::DuplicateEntry,
        t301Code::OrderViolated,
        t301Code::GuardTripped,
        t301Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            t301Code::LevelBad => 0x3c00,
            t301Code::BitFieldBad => 0x3c01,
            t301Code::ShiftOverlap => 0x3c02,
            t301Code::CanonBad => 0x3c03,
            t301Code::HugeMixBad => 0x3c04,
            t301Code::EntryBitsBad => 0x3c05,
            t301Code::NxViolate => 0x3c06,
            t301Code::TableBroken => 0x3c07,
            t301Code::LedgerBroken => 0x3c08,
            t301Code::SnapshotFingerprint => 0x3c09,
            t301Code::IndexMisaligned => 0x3c0a,
            t301Code::DuplicateEntry => 0x3c0b,
            t301Code::OrderViolated => 0x3c0c,
            t301Code::GuardTripped => 0x3c0d,
            t301Code::SegmentGuard => 0x3c0e,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            t301Code::LevelBad => "级别非法（0..3 之外）",
            t301Code::BitFieldBad => "位域非法（起止位倒挂）",
            t301Code::ShiftOverlap => "位移重叠（邻级位域交叠）",
            t301Code::CanonBad => "规范位违例（VA 高位不全同号）",
            t301Code::HugeMixBad => "页尺寸混用（同级混 4K/2M/1G）",
            t301Code::EntryBitsBad => "表项位非法（保留位置位）",
            t301Code::NxViolate => "NX 违规（可执行数据页）",
            t301Code::TableBroken => "页表族表破坏",
            t301Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            t301Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            t301Code::IndexMisaligned => "索引错位（枚举序与矩阵序不一致）",
            t301Code::DuplicateEntry => "条目重复（同键双写）",
            t301Code::OrderViolated => "次序违例（依赖序/时序破缺）",
            t301Code::GuardTripped => "守卫触发（重入/越限被拒）",
            t301Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            t301Code::LevelBad => "按四级族校正",
            t301Code::BitFieldBad => "按位域序重排",
            t301Code::ShiftOverlap => "按位移唯一律重排",
            t301Code::CanonBad => "按规范位律校正",
            t301Code::HugeMixBad => "按页尺寸一致律校正",
            t301Code::EntryBitsBad => "按表项位掩码校正",
            t301Code::NxViolate => "按 W^X 策略校正",
            t301Code::TableBroken => "按 x86_64 规格重建",
            t301Code::LedgerBroken => "按守恒律重建台账后重跑",
            t301Code::SnapshotFingerprint => "按防篡改规程重建快照",
            t301Code::IndexMisaligned => "按声明序归一化后重跑",
            t301Code::DuplicateEntry => "合并同键条目为末次生效",
            t301Code::OrderViolated => "按依赖矩阵重排后重跑",
            t301Code::GuardTripped => "检查临界区嵌套与阈值",
            t301Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 字符串键值映射 pglevels（4 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 四级页表族表（级别↔VA 位域逐行守卫）。
/// 行数。
pub const pglevels_COUNT: usize = 4;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static pglevels: [(&'static str, u64); 4] = [
    ("LVL_PML4", 0), // PML4 级（VA 47-39 位索引，9 位）
    ("LVL_PDPT", 1), // PDPT 级（VA 38-30 位索引，9 位）
    ("LVL_PD", 2), // PD 级（VA 29-21 位索引，9 位；可 2M 大页）
    ("LVL_PT", 3), // PT 级（VA 20-12 位索引，9 位；4K 页）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn pglevels_get(key: &str) -> Option<u64> {
    match key {
        "LVL_PML4" => Some(0),
        "LVL_PDPT" => Some(1),
        "LVL_PD" => Some(2),
        "LVL_PT" => Some(3),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn pglevels_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < pglevels_COUNT {
        let (k, v) = pglevels[i];
        let ok = match k {
            "LVL_PML4" => v == 0,
            "LVL_PDPT" => v == 1,
            "LVL_PD" => v == 2,
            "LVL_PT" => v == 3,
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
pub fn pglevels_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < pglevels_COUNT {
        s = s.wrapping_add(pglevels[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn pglevels_unique() -> bool {
    let mut i = 0usize;
    while i < pglevels_COUNT {
        let mut j = i + 1;
        while j < pglevels_COUNT {
            if pglevels[i].0 == pglevels[j].0 {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// ---------------------------------------------------------------------------
// 每级位域定义（x86_64 四级转换：9-9-9-9-12）
// ---------------------------------------------------------------------------

/// 级索引类型（0=PML4 .. 3=PT）。
pub type PgLevel = u32;

/// 每级索引位宽（9 位）。
pub const IDX_BITS: u32 = 9;

/// 每级槽数（512）。
pub const SLOTS_PER_LEVEL: u32 = 512;

/// 页内偏移位宽（12 位 = 4KiB）。
pub const OFFSET_BITS: u32 = 12;

/// 级字段表（起始位/结束位/是否可大页）。
pub static PG_FIELDS: [(PgLevel, u32, u32, bool); 4] = [
    (0, 39, 47, false),
    (1, 30, 38, false),
    (2, 21, 29, true),
    (3, 12, 20, false),
];

/// 级字段合法性（起止位邻接不重叠 + 覆盖 12..47 连续）。
pub const fn pg_fields_ok() -> bool {
    // 覆盖连续性：LVL_PT 起位=12，PML4 止位=47。
    if PG_FIELDS[3].1 != OFFSET_BITS || PG_FIELDS[0].2 != 47 {
        return false;
    }
    // 邻接不重叠：上级止位 + 1 == 下级起位（PT.20+1=PD.21，PD.29+1=PDPT.30，PDPT.38+1=PML4.39）。
    if PG_FIELDS[3].2 + 1 != PG_FIELDS[2].1 || PG_FIELDS[2].2 + 1 != PG_FIELDS[1].1 || PG_FIELDS[1].2 + 1 != PG_FIELDS[0].1 {
        return false;
    }
    true
}

/// 级索引提取（VA → 级别的 9 位索引）。
pub const fn pg_index(va: u64, level: PgLevel) -> u64 {
    if level > 3 {
        return 0;
    }
    let f = PG_FIELDS[level as usize];
    (va >> f.1) & ((1u64 << IDX_BITS) - 1)
}

/// 规范位检查（VA 第 47-63 位必须全同号：47 位为 0 则高位全 0，否则全 1）。
pub const fn pg_canonical_ok(va: u64) -> bool {
    let bit47 = (va >> 47) & 1;
    let upper = va >> 48;
    if bit47 == 0 {
        upper == 0
    } else {
        upper == 0xffff
    }
}

/// 表项标志位（x86_64 PTE 语义）。
pub const PTE_P: u64 = 1 << 0;
pub const PTE_RW: u64 = 1 << 1;
pub const PTE_US: u64 = 1 << 2;
pub const PTE_PWT: u64 = 1 << 3;
pub const PTE_PCD: u64 = 1 << 4;
pub const PTE_A: u64 = 1 << 5;
pub const PTE_D: u64 = 1 << 6;
pub const PTE_PS: u64 = 1 << 7;
pub const PTE_G: u64 = 1 << 8;
pub const PTE_NX: u64 = 1 << 63;

/// 物理地址掩码（52 位物理空间；高 12 位清零）。
pub const PHYS_MASK: u64 = 0x000f_ffff_ffff_f000;

/// 表项 → 物地址（掩码提取）。
pub const fn pte_phys(entry: u64) -> u64 {
    entry & PHYS_MASK
}

// ---------------------------------------------------------------------------
// 段位守卫（本模块独立十六进码前缀防串段；越界即判红）
// ---------------------------------------------------------------------------

/// 段位守卫：全部诊断码必须落在本段 [0x3c00, 0x3c0f]。
pub const fn t301_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < t301Code::ALL.len() {
        let c = t301Code::ALL[i].code();
        if c < 0x3c00 || c > 0x3c0f {
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
fn t301_guard_seg() -> bool {
    t301_segment_guard()
}

/// 表 pglevels 逐行守卫全绿（行数/守卫/唯一性）。
fn check_t301_pglevels_guard() -> bool {
    let mut ok = pglevels_COUNT == 4;
    if ok {
        ok = pglevels_scan() == 0;
    }
    if ok {
        ok = pglevels_unique();
    }
    ok
}

/// 级字段邻接律 + 规范位正负样本 + 索引提取对拍。
fn check_c2_fields() -> bool {
    let mut ok = pg_fields_ok();
    if ok {
        // 高半空间规范 VA（内核文本基址）的 PML4 索引为 511。
        ok = pg_index(0xffff_ffff_8000_0000, 0) == 511;
    }
    if ok {
        // 低半空间规范位：47 位为 0 → 高位全 0。
        ok = pg_canonical_ok(0x0000_5555_5555_5555) && pg_canonical_ok(0xffff_8000_0000_0000);
    }
    if ok {
        // 非规范负样本（48 位悬空）。
        ok = !pg_canonical_ok(0x0001_8000_0000_0000);
    }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_t301_checks() -> CheckSet {
    let mut set = CheckSet::new("T301");
    set.add(
        "t301-seg-guard",
        t301_guard_seg(),
        if t301_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "t301-c1-level-guard",
        check_t301_pglevels_guard(),
        if check_t301_pglevels_guard() { "" } else { "table pglevels guard mismatch" },
    );
    set.add(
        "t301-c2-fields",
        check_c2_fields(),
        if check_c2_fields() { "" } else { "page table field layout mismatch" },
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
    fn t301_all_checks_green() {
        let set = run_t301_checks();
        assert!(set.all_passed(), "T301 domain checks must be green");
    }

    #[test]
    fn t301_segment_guard_holds() {
        assert!(t301_segment_guard());
    }
}
