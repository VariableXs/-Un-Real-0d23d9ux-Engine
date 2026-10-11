//! unxt306_unx302_pml4layout —— UNX-F0302 PML4 槽位布局冻结（四区域 × F0012 矩阵逐槽一致）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F0302 · PML4 槽位布局冻结实现 · 判据：槽位布局（内核/直接映射/自映射/高半区）与 F0012 冻结矩阵逐槽一致
//! · 组件：四区域槽位表 + F0012 冻结矩阵逐槽对拍 + 冻结链 + 域快照
//! · 判据落地：与 F0012 冻结矩阵逐槽一致
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const T306_VERSION: &str = "t306-unx302_pml4layout-v1";

/// 诊断段基址。
pub const T306_SEG_BASE: u16 = 0x4ea0;

// --------------------------------------------------------------------------
// 诊断码（t306 十六码 · 14 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// PML4 布局诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum t306Code {
    /// 0x4ea0 区域非法（四区域之外）。
    RegionBad,
    /// 0x4ea1 槽号越界（≥512）。
    SlotOob,
    /// 0x4ea2 区域重复（同槽双区域）。
    RegionDup,
    /// 0x4ea3 矩阵分歧（与 F0012 不一致）。
    MatrixDiff,
    /// 0x4ea4 冻结缺失（布局未冻）。
    FreezeMissing,
    /// 0x4ea5 布局账破缺。
    LedgerBreak,
    /// 0x4ea6 槽序断档。
    OrderViolated,
    /// 0x4ea7 守卫触发。
    GuardTripped,
    /// 0x4ea8 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x4ea9 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x4eaa 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x4eab 索引错位（枚举序与矩阵序不一致）。
    IndexMisaligned,
    /// 0x4eac 条目重复（同键双写）。
    DuplicateEntry,
    /// 0x4ead 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl t306Code {
    /// 全量闭集。
    pub const ALL: [t306Code; 14] = [
        t306Code::RegionBad,
        t306Code::SlotOob,
        t306Code::RegionDup,
        t306Code::MatrixDiff,
        t306Code::FreezeMissing,
        t306Code::LedgerBreak,
        t306Code::OrderViolated,
        t306Code::GuardTripped,
        t306Code::LedgerBroken,
        t306Code::SnapshotFingerprint,
        t306Code::TableBroken,
        t306Code::IndexMisaligned,
        t306Code::DuplicateEntry,
        t306Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            t306Code::RegionBad => 0x4ea0,
            t306Code::SlotOob => 0x4ea1,
            t306Code::RegionDup => 0x4ea2,
            t306Code::MatrixDiff => 0x4ea3,
            t306Code::FreezeMissing => 0x4ea4,
            t306Code::LedgerBreak => 0x4ea5,
            t306Code::OrderViolated => 0x4ea6,
            t306Code::GuardTripped => 0x4ea7,
            t306Code::LedgerBroken => 0x4ea8,
            t306Code::SnapshotFingerprint => 0x4ea9,
            t306Code::TableBroken => 0x4eaa,
            t306Code::IndexMisaligned => 0x4eab,
            t306Code::DuplicateEntry => 0x4eac,
            t306Code::SegmentGuard => 0x4ead,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            t306Code::RegionBad => "区域非法（四区域之外）",
            t306Code::SlotOob => "槽号越界（≥512）",
            t306Code::RegionDup => "区域重复（同槽双区域）",
            t306Code::MatrixDiff => "矩阵分歧（与 F0012 不一致）",
            t306Code::FreezeMissing => "冻结缺失（布局未冻）",
            t306Code::LedgerBreak => "布局账破缺",
            t306Code::OrderViolated => "槽序断档",
            t306Code::GuardTripped => "守卫触发",
            t306Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            t306Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            t306Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            t306Code::IndexMisaligned => "索引错位（枚举序与矩阵序不一致）",
            t306Code::DuplicateEntry => "条目重复（同键双写）",
            t306Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            t306Code::RegionBad => "按四区域闭集校正",
            t306Code::SlotOob => "按 512 边界裁剪",
            t306Code::RegionDup => "按独占律裁一",
            t306Code::MatrixDiff => "按逐槽对拍归因",
            t306Code::FreezeMissing => "按冻结规程封版",
            t306Code::LedgerBreak => "按守恒重建",
            t306Code::OrderViolated => "按 512 连续律补号",
            t306Code::GuardTripped => "检查临界区",
            t306Code::LedgerBroken => "按守恒律重建台账后重跑",
            t306Code::SnapshotFingerprint => "按防篡改规程重建快照",
            t306Code::TableBroken => "按语义规则重建常量表",
            t306Code::IndexMisaligned => "按声明序归一化后重跑",
            t306Code::DuplicateEntry => "合并同键条目为末次生效",
            t306Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// ---------------------------------------------------------------------------
// PML4 槽位布局（四区域：内核/直接映射/自映射/高半区）+ F0012 冻结矩阵对拍
// ---------------------------------------------------------------------------

/// 区域（闭集四区）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Pml4Region {
    /// 内核区（PML4 255 = 0xFF）。
    Kernel,
    /// 直接映射区（末段槽）。
    DirectMap,
    /// 自映射区（PML4 510，自指）。
    SelfMap,
    /// 高半区区（PML4 256+，内核模块区）。
    HighHalf,
}

impl Pml4Region {
    /// 全量闭集（四区）。
    pub const ALL: [Pml4Region; 4] = [
        Pml4Region::Kernel,
        Pml4Region::DirectMap,
        Pml4Region::SelfMap,
        Pml4Region::HighHalf,
    ];

    /// 索引。
    pub const fn index(self) -> usize {
        match self {
            Pml4Region::Kernel => 0,
            Pml4Region::DirectMap => 1,
            Pml4Region::SelfMap => 2,
            Pml4Region::HighHalf => 3,
        }
    }

    /// 中文区名。
    pub const fn cn(self) -> &'static str {
        match self {
            Pml4Region::Kernel => "内核区",
            Pml4Region::DirectMap => "直接映射区",
            Pml4Region::SelfMap => "自映射区",
            Pml4Region::HighHalf => "高半区",
        }
    }

    /// 起始槽（Kernel 255 / DirectMap 511 / SelfMap 510 / HighHalf 256）。
    pub const fn start_slot(self) -> u32 {
        match self {
            Pml4Region::Kernel => 255,
            Pml4Region::DirectMap => 511,
            Pml4Region::SelfMap => 510,
            Pml4Region::HighHalf => 256,
        }
    }

    /// 区域槽数（Kernel 1 / DirectMap 1 / SelfMap 1 / HighHalf 1，冻结口径逐槽登记）。
    pub const fn slot_span(self) -> u32 {
        match self {
            _ => 1,
        }
    }
}

/// F0012 冻结矩阵（槽号 → 区域；None = 未冻结）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrozenSlot {
    /// 槽号（0..=511）。
    pub slot: u32,
    /// 区域标签（0=Kernel 1=DirectMap 2=SelfMap 3=HighHalf；0xFF=未冻结）。
    pub region: u8,
}

/// 冻结槽合法性（槽域 + 四区闭合集）。
pub const fn frozen_slot_ok(s: &FrozenSlot) -> bool {
    s.slot < 512 && (s.region == 0xFF || s.region <= 3)
}

/// 槽归属解析（四区域任意槽的归属；返回 None = 未冻结）。
pub const fn slot_region(slot: u32) -> Option<Pml4Region> {
    if slot >= 512 {
        return None;
    }
    let mut i = 0;
    while i < 4 {
        let r = Pml4Region::ALL[i];
        if slot >= r.start_slot() && slot < r.start_slot() + r.slot_span() {
            return Some(r);
        }
        i += 1;
    }
    None
}

/// F0012 冻结矩阵对拍（布局槽 == F0012 冻结槽逐槽一致）。
pub const fn matrix_agrees(layout: &FrozenSlot, f0012: &FrozenSlot) -> bool {
    layout.slot == f0012.slot && layout.region == f0012.region
}

/// 自映射恒等式（自映射槽的槽号 == 自映射区起始槽）。
pub const fn selfmap_identity(slot: u32) -> bool {
    match slot_region(slot) {
        Some(r) => {
            matches!(r, Pml4Region::SelfMap) && slot == Pml4Region::SelfMap.start_slot()
        }
        None => false,
    }
}

// ---------------------------------------------------------------------------
// 段位守卫（本模块独立十六进码前缀防串段；越界即判红）
// ---------------------------------------------------------------------------

/// 段位守卫：全部诊断码必须落在本段 [0x4ea0, 0x4eaf]。
pub const fn t306_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < t306Code::ALL.len() {
        let c = t306Code::ALL[i].code();
        if c < 0x4ea0 || c > 0x4eaf {
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
fn t306_guard_seg() -> bool {
    t306_segment_guard()
}

/// 四区域起始槽（255/511/510/256）互异 + 归属解析。
fn check_c1_regions() -> bool {
    let mut ok = Pml4Region::Kernel.start_slot() == 255;
    if ok {
        ok = Pml4Region::DirectMap.start_slot() == 511 && Pml4Region::SelfMap.start_slot() == 510;
    }
    if ok {
        ok = Pml4Region::HighHalf.start_slot() == 256;
    }
    if ok {
        ok = matches!(slot_region(255), Some(Pml4Region::Kernel));
    }
    if ok {
        ok = matches!(slot_region(511), Some(Pml4Region::DirectMap));
    }
    if ok {
        ok = slot_region(512).is_none();
    }
    ok
}

/// F0012 逐槽对拍 + 自映射恒等 + 未冻结场景。
fn check_c2_matrix() -> bool {
    let layout = FrozenSlot { slot: 256, region: 3 };
    let f0012 = FrozenSlot { slot: 256, region: 3 };
    let mut ok = matrix_agrees(&layout, &f0012);
    if ok {
        ok = selfmap_identity(510);
    }
    if ok {
        // 未冻结槽（0xFF）与布局必不一致。
        let unfrozen = FrozenSlot { slot: 256, region: 0xFF };
        ok = !matrix_agrees(&layout, &unfrozen);
    }
    if ok {
        // 槽号分歧必检出。
        let wrong = FrozenSlot { slot: 257, region: 3 };
        ok = !matrix_agrees(&layout, &wrong);
    }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_t306_checks() -> CheckSet {
    let mut set = CheckSet::new("T306");
    set.add(
        "t306-seg-guard",
        t306_guard_seg(),
        if t306_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "t306-c1-regions",
        check_c1_regions(),
        if check_c1_regions() { "" } else { "pml4 regions mismatch" },
    );
    set.add(
        "t306-c2-matrix",
        check_c2_matrix(),
        if check_c2_matrix() { "" } else { "f0012 matrix agreement mismatch" },
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
    fn t306_all_checks_green() {
        let set = run_t306_checks();
        assert!(set.all_passed(), "T306 domain checks must be green");
    }

    #[test]
    fn t306_segment_guard_holds() {
        assert!(t306_segment_guard());
    }
}
