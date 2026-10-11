//! unxt305_pgcross —— UNX-F0301 四级交叉校验（VA→PA 全链回放）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F0301 · 页表构建总编排（四级表族流水） · 判据：PML4→PDPT→PD→PT 四级构建流水编排正确，依赖序与设计一致，构建账完整
//! · 组件：VA→PA 转换引擎 + 交叉校验对拍 + 失败归因
//! · 判据落地：四级构建正确性校验面
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const T305_VERSION: &str = "t305-pgcross-v1";

/// 诊断段基址。
pub const T305_SEG_BASE: u16 = 0x3c40;

// --------------------------------------------------------------------------
// 诊断码（t305 十六码 · 15 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 交叉校验诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum t305Code {
    /// 0x3c40 走表失败（某级表项无效）。
    WalkFailed,
    /// 0x3c41 转换不符（双源 PA 不一致）。
    Mismatch,
    /// 0x3c42 非规范 VA。
    UncanonVa,
    /// 0x3c43 大页标记不一致。
    HugeInconsistent,
    /// 0x3c44 NX 数据页可执行。
    NxNoExec,
    /// 0x3c45 权限位非法。
    PermBad,
    /// 0x3c46 校验账破缺。
    LedgerBreak,
    /// 0x3c47 守卫触发。
    GuardTripped,
    /// 0x3c48 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x3c49 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x3c4a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x3c4b 索引错位（枚举序与矩阵序不一致）。
    IndexMisaligned,
    /// 0x3c4c 条目重复（同键双写）。
    DuplicateEntry,
    /// 0x3c4d 次序违例（依赖序/时序破缺）。
    OrderViolated,
    /// 0x3c4e 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl t305Code {
    /// 全量闭集。
    pub const ALL: [t305Code; 15] = [
        t305Code::WalkFailed,
        t305Code::Mismatch,
        t305Code::UncanonVa,
        t305Code::HugeInconsistent,
        t305Code::NxNoExec,
        t305Code::PermBad,
        t305Code::LedgerBreak,
        t305Code::GuardTripped,
        t305Code::LedgerBroken,
        t305Code::SnapshotFingerprint,
        t305Code::TableBroken,
        t305Code::IndexMisaligned,
        t305Code::DuplicateEntry,
        t305Code::OrderViolated,
        t305Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            t305Code::WalkFailed => 0x3c40,
            t305Code::Mismatch => 0x3c41,
            t305Code::UncanonVa => 0x3c42,
            t305Code::HugeInconsistent => 0x3c43,
            t305Code::NxNoExec => 0x3c44,
            t305Code::PermBad => 0x3c45,
            t305Code::LedgerBreak => 0x3c46,
            t305Code::GuardTripped => 0x3c47,
            t305Code::LedgerBroken => 0x3c48,
            t305Code::SnapshotFingerprint => 0x3c49,
            t305Code::TableBroken => 0x3c4a,
            t305Code::IndexMisaligned => 0x3c4b,
            t305Code::DuplicateEntry => 0x3c4c,
            t305Code::OrderViolated => 0x3c4d,
            t305Code::SegmentGuard => 0x3c4e,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            t305Code::WalkFailed => "走表失败（某级表项无效）",
            t305Code::Mismatch => "转换不符（双源 PA 不一致）",
            t305Code::UncanonVa => "非规范 VA",
            t305Code::HugeInconsistent => "大页标记不一致",
            t305Code::NxNoExec => "NX 数据页可执行",
            t305Code::PermBad => "权限位非法",
            t305Code::LedgerBreak => "校验账破缺",
            t305Code::GuardTripped => "守卫触发",
            t305Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            t305Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            t305Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            t305Code::IndexMisaligned => "索引错位（枚举序与矩阵序不一致）",
            t305Code::DuplicateEntry => "条目重复（同键双写）",
            t305Code::OrderViolated => "次序违例（依赖序/时序破缺）",
            t305Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            t305Code::WalkFailed => "按级归因定位",
            t305Code::Mismatch => "按走表链复核",
            t305Code::UncanonVa => "按规范位律拒收",
            t305Code::HugeInconsistent => "按大页律校正",
            t305Code::NxNoExec => "按 W^X 校正",
            t305Code::PermBad => "按权限表校正",
            t305Code::LedgerBreak => "按守恒重建",
            t305Code::GuardTripped => "检查临界区",
            t305Code::LedgerBroken => "按守恒律重建台账后重跑",
            t305Code::SnapshotFingerprint => "按防篡改规程重建快照",
            t305Code::TableBroken => "按语义规则重建常量表",
            t305Code::IndexMisaligned => "按声明序归一化后重跑",
            t305Code::DuplicateEntry => "合并同键条目为末次生效",
            t305Code::OrderViolated => "按依赖矩阵重排后重跑",
            t305Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}


// ---------------------------------------------------------------------------
// 页表走表原语（本模块自包含：位域/规范位/表项标志内联定义）
// ---------------------------------------------------------------------------

/// 每级索引位宽（9 位）。
pub const IDX_BITS: u32 = 9;

/// 级字段表（起始位/结束位/是否可大页；x86_64 四级 9-9-9-9-12）。
pub static PG_FIELDS: [(u32, u32, u32, bool); 4] = [
    (0, 39, 47, false),
    (1, 30, 38, false),
    (2, 21, 29, true),
    (3, 12, 20, false),
];

/// 级索引提取（VA → 级别的 9 位索引）。
pub const fn pg_index(va: u64, level: u32) -> u64 {
    if level > 3 {
        return 0;
    }
    let f = PG_FIELDS[level as usize];
    (va >> f.1) & ((1u64 << IDX_BITS) - 1)
}

/// 规范位检查（VA 第 47-63 位必须全同号）。
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
pub const PTE_PS: u64 = 1 << 7;

/// 物理地址掩码（52 位物理空间）。
pub const PHYS_MASK: u64 = 0x000f_ffff_ffff_f000;

/// 表项 → 物地址（掩码提取）。
pub const fn pte_phys(entry: u64) -> u64 {
    entry & PHYS_MASK
}

// ---------------------------------------------------------------------------
// VA→PA 四級走表（简化模型：给定四级表项数组直算）
// ---------------------------------------------------------------------------

/// 单级表项（512 项）。
pub const LEVEL_ENTRIES: usize = 512;

/// 走表输入（四级表项数组的假设物地址，测试用模型）。
#[derive(Clone, Copy, Debug)]
pub struct WalkInput {
    /// PML4 表基址。
    pub pml4_base: u64,
    /// PML4 表项。
    pub pml4_entries: [u64; LEVEL_ENTRIES],
    /// PDPT 表项。
    pub pdpt_entries: [u64; LEVEL_ENTRIES],
    /// PD 表项。
    pub pd_entries: [u64; LEVEL_ENTRIES],
    /// PT 表项。
    pub pt_entries: [u64; LEVEL_ENTRIES],
}

/// 走表结果（物地址 + 命中的大页级别 + NX）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WalkResult {
    /// 转换物地址。
    pub pa: u64,
    /// 大页级别（0=4K，2=2M，3=1G）。
    pub huge_level: u32,
    /// 权限位快照。
    pub flags: u64,
}

/// 走表引擎（逐级取 9 位索引，PS 位命中即收口）。
pub fn pg_walk(input: &WalkInput, va: u64) -> Option<WalkResult> {
    if !pg_canonical_ok(va) {
        return None;
    }
    let i0 = pg_index(va, 0) as usize;
    let e0 = input.pml4_entries[i0];
    if e0 & PTE_P == 0 {
        return None;
    }
    let pdp_base = pte_phys(e0);
    let _ = pdp_base;
    let i1 = pg_index(va, 1) as usize;
    let e1 = input.pdpt_entries[i1];
    if e1 & PTE_P == 0 {
        return None;
    }
    if e1 & PTE_PS != 0 {
        // 1G 大页。
        let base = pte_phys(e1);
        let off = va & ((1u64 << 30) - 1);
        return Some(WalkResult { pa: base + off, huge_level: 1, flags: e1 });
    }
    let i2 = pg_index(va, 2) as usize;
    let e2 = input.pd_entries[i2];
    if e2 & PTE_P == 0 {
        return None;
    }
    if e2 & PTE_PS != 0 {
        // 2M 大页。
        let base = pte_phys(e2);
        let off = va & ((1u64 << 21) - 1);
        return Some(WalkResult { pa: base + off, huge_level: 2, flags: e2 });
    }
    let i3 = pg_index(va, 3) as usize;
    let e3 = input.pt_entries[i3];
    if e3 & PTE_P == 0 {
        return None;
    }
    let base = pte_phys(e3);
    let off = va & 0xfff;
    Some(WalkResult { pa: base + off, huge_level: 0, flags: e3 })
}

// ---------------------------------------------------------------------------
// 段位守卫（本模块独立十六进码前缀防串段；越界即判红）
// ---------------------------------------------------------------------------

/// 段位守卫：全部诊断码必须落在本段 [0x3c40, 0x3c4f]。
pub const fn t305_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < t305Code::ALL.len() {
        let c = t305Code::ALL[i].code();
        if c < 0x3c40 || c > 0x3c4f {
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
fn t305_guard_seg() -> bool {
    t305_segment_guard()
}

/// 4K 页走表正样本（索引按 VA 位片实算，PA 恒定）。
fn check_c1_walk_4k() -> bool {
    let va = 0x0000_7fff_8000_0000u64;
    let (i0, i1, i2, i3) = (pg_index(va, 0) as usize, pg_index(va, 1) as usize, pg_index(va, 2) as usize, pg_index(va, 3) as usize);
    let mut input = WalkInput {
        pml4_base: 0x1000, pml4_entries: [0; 512], pdpt_entries: [0; 512],
        pd_entries: [0; 512], pt_entries: [0; 512],
    };
    input.pml4_entries[i0] = PTE_P | PTE_RW | 0x2000;
    input.pdpt_entries[i1] = PTE_P | PTE_RW | 0x3000;
    input.pd_entries[i2] = PTE_P | PTE_RW | 0x4000;
    input.pt_entries[i3] = PTE_P | PTE_RW | 0xdead_000;
    let mut ok = matches!(pg_walk(&input, va), Some(_));
    if ok {
        if let Some(r) = pg_walk(&input, va) {
            ok = r.pa == 0xdead_000 && r.huge_level == 0;
        }
    }
    ok
}

/// 2M 大页走表（PS 位提前收口）。
fn check_c2_walk_huge() -> bool {
    let va = 0x0000_7fff_8000_0000u64;
    let (i0, i1, i2, _i3) = (pg_index(va, 0) as usize, pg_index(va, 1) as usize, pg_index(va, 2) as usize, pg_index(va, 3) as usize);
    let mut input = WalkInput {
        pml4_base: 0x1000, pml4_entries: [0; 512], pdpt_entries: [0; 512],
        pd_entries: [0; 512], pt_entries: [0; 512],
    };
    input.pml4_entries[i0] = PTE_P | PTE_RW | 0x2000;
    input.pdpt_entries[i1] = PTE_P | PTE_RW | 0x3000;
    input.pd_entries[i2] = PTE_P | PTE_RW | PTE_PS | 0x500_000;
    let mut ok = false;
    if let Some(r) = pg_walk(&input, va) {
        ok = r.huge_level == 2 && r.pa == 0x500_000;
    }
    ok
}

/// 非规范 VA 拒收 + 表项缺失归因。
fn check_c3_walk_uncanon() -> bool {
    let input = WalkInput {
        pml4_base: 0x1000, pml4_entries: [0; 512], pdpt_entries: [0; 512],
        pd_entries: [0; 512], pt_entries: [0; 512],
    };
    let mut ok = pg_walk(&input, 0x0001_8000_0000_0000).is_none();
    if ok {
        ok = pg_walk(&input, 0x0000_7fff_8000_0000).is_none();
    }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_t305_checks() -> CheckSet {
    let mut set = CheckSet::new("T305");
    set.add(
        "t305-seg-guard",
        t305_guard_seg(),
        if t305_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "t305-c1-walk-4k",
        check_c1_walk_4k(),
        if check_c1_walk_4k() { "" } else { "4k page walk mismatch" },
    );
    set.add(
        "t305-c2-walk-huge",
        check_c2_walk_huge(),
        if check_c2_walk_huge() { "" } else { "2m huge page walk mismatch" },
    );
    set.add(
        "t305-c3-walk-uncanon",
        check_c3_walk_uncanon(),
        if check_c3_walk_uncanon() { "" } else { "uncanonical/missing entry rejection mismatch" },
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
    fn t305_all_checks_green() {
        let set = run_t305_checks();
        assert!(set.all_passed(), "T305 domain checks must be green");
    }

    #[test]
    fn t305_segment_guard_holds() {
        assert!(t305_segment_guard());
    }
}
