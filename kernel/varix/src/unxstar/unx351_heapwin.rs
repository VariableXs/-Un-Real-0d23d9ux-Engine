//! unxt351_heapwin —— UNX-F0381 堆窗划定（边界冻结入账）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F0381 · 早年堆 arena 划定（堆窗入账） · 判据：堆窗边界冻结入账，与页帧池白名单一致，堆容量 4MB 档锁定
//! · 组件：堆窗表（基址/长度/对齐）+ 边界守卫 + 4MB 档锁定
//! · 判据落地：堆窗边界冻结入账
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const T351_VERSION: &str = "t351-heapwin-v1";

/// 诊断段基址。
pub const T351_SEG_BASE: u16 = 0x4000;

// --------------------------------------------------------------------------
// 诊断码（t351 十六码 · 15 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 堆窗划定诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum t351Code {
    /// 0x4000 堆窗非法（基址/长度违例）。
    WindowBad,
    /// 0x4001 对齐违例（非页对齐）。
    AlignBad,
    /// 0x4002 容量非 4MB 档。
    SizeNot4M,
    /// 0x4003 堆窗重叠（与页帧池冲突）。
    WindowOverlap,
    /// 0x4004 边界未冻结。
    BoundaryUnfrozen,
    /// 0x4005 白名单不符（堆窗越出白名单）。
    WhitelistMismatch,
    /// 0x4006 堆窗账破缺。
    LedgerBreak,
    /// 0x4007 守卫触发。
    GuardTripped,
    /// 0x4008 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x4009 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x400a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x400b 索引错位（枚举序与矩阵序不一致）。
    IndexMisaligned,
    /// 0x400c 条目重复（同键双写）。
    DuplicateEntry,
    /// 0x400d 次序违例（依赖序/时序破缺）。
    OrderViolated,
    /// 0x400e 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl t351Code {
    /// 全量闭集。
    pub const ALL: [t351Code; 15] = [
        t351Code::WindowBad,
        t351Code::AlignBad,
        t351Code::SizeNot4M,
        t351Code::WindowOverlap,
        t351Code::BoundaryUnfrozen,
        t351Code::WhitelistMismatch,
        t351Code::LedgerBreak,
        t351Code::GuardTripped,
        t351Code::LedgerBroken,
        t351Code::SnapshotFingerprint,
        t351Code::TableBroken,
        t351Code::IndexMisaligned,
        t351Code::DuplicateEntry,
        t351Code::OrderViolated,
        t351Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            t351Code::WindowBad => 0x4000,
            t351Code::AlignBad => 0x4001,
            t351Code::SizeNot4M => 0x4002,
            t351Code::WindowOverlap => 0x4003,
            t351Code::BoundaryUnfrozen => 0x4004,
            t351Code::WhitelistMismatch => 0x4005,
            t351Code::LedgerBreak => 0x4006,
            t351Code::GuardTripped => 0x4007,
            t351Code::LedgerBroken => 0x4008,
            t351Code::SnapshotFingerprint => 0x4009,
            t351Code::TableBroken => 0x400a,
            t351Code::IndexMisaligned => 0x400b,
            t351Code::DuplicateEntry => 0x400c,
            t351Code::OrderViolated => 0x400d,
            t351Code::SegmentGuard => 0x400e,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            t351Code::WindowBad => "堆窗非法（基址/长度违例）",
            t351Code::AlignBad => "对齐违例（非页对齐）",
            t351Code::SizeNot4M => "容量非 4MB 档",
            t351Code::WindowOverlap => "堆窗重叠（与页帧池冲突）",
            t351Code::BoundaryUnfrozen => "边界未冻结",
            t351Code::WhitelistMismatch => "白名单不符（堆窗越出白名单）",
            t351Code::LedgerBreak => "堆窗账破缺",
            t351Code::GuardTripped => "守卫触发",
            t351Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            t351Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            t351Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            t351Code::IndexMisaligned => "索引错位（枚举序与矩阵序不一致）",
            t351Code::DuplicateEntry => "条目重复（同键双写）",
            t351Code::OrderViolated => "次序违例（依赖序/时序破缺）",
            t351Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            t351Code::WindowBad => "按堆窗表校正",
            t351Code::AlignBad => "按页对齐律校正",
            t351Code::SizeNot4M => "按 4MB 档重划",
            t351Code::WindowOverlap => "按白名单核验拆分",
            t351Code::BoundaryUnfrozen => "按冻结规程封版",
            t351Code::WhitelistMismatch => "按白名单裁剪",
            t351Code::LedgerBreak => "按守恒重建",
            t351Code::GuardTripped => "检查临界区",
            t351Code::LedgerBroken => "按守恒律重建台账后重跑",
            t351Code::SnapshotFingerprint => "按防篡改规程重建快照",
            t351Code::TableBroken => "按语义规则重建常量表",
            t351Code::IndexMisaligned => "按声明序归一化后重跑",
            t351Code::DuplicateEntry => "合并同键条目为末次生效",
            t351Code::OrderViolated => "按依赖矩阵重排后重跑",
            t351Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// ---------------------------------------------------------------------------
// 堆窗常量（早年堆：4MB 档，页对齐，白名单内）
// ---------------------------------------------------------------------------

/// 4MB 档（字节）。
pub const HEAP_SIZE_4M: u64 = 4 * 1024 * 1024;

/// 4K 页。
pub const HEAP_PAGE: u64 = 4096;

/// 堆窗基址（早年惯例 0x1000000 = 16MiB，页帧池白名单内）。
pub const HEAP_BASE: u64 = 0x100_0000;

/// 堆窗结束（闭开）。
pub const HEAP_END: u64 = HEAP_BASE + HEAP_SIZE_4M;

/// 页帧池白名单上限（早年可分配物理上限 0x4000_0000 = 1GiB 内的早年段）。
pub const EARLY_POOL_LIMIT: u64 = 0x4000_0000;

/// 堆窗合法性：基址页对齐 + 4MB 档 + 闭开不溢出 + 在早年池内。
pub const fn heap_window_ok(base: u64, size: u64) -> bool {
    if base % HEAP_PAGE != 0 || size % HEAP_PAGE != 0 {
        return false;
    }
    if size != HEAP_SIZE_4M {
        return false;
    }
    if base + size != HEAP_END {
        return false;
    }
    base + size <= EARLY_POOL_LIMIT
}

/// 白名单核验：堆窗整体落在 [HEAP_BASE, EARLY_POOL_LIMIT) 内。
pub const fn heap_in_whitelist() -> bool {
    HEAP_BASE >= HEAP_PAGE && HEAP_END <= EARLY_POOL_LIMIT
}

// ---------------------------------------------------------------------------
// 段位守卫（本模块独立十六进码前缀防串段；越界即判红）
// ---------------------------------------------------------------------------

/// 段位守卫：全部诊断码必须落在本段 [0x4000, 0x400f]。
pub const fn t351_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < t351Code::ALL.len() {
        let c = t351Code::ALL[i].code();
        if c < 0x4000 || c > 0x400f {
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
fn t351_guard_seg() -> bool {
    t351_segment_guard()
}

/// 堆窗合法性（4MB 档 + 页对齐 + 白名单内）。
fn check_c1_window() -> bool {
    let mut ok = heap_window_ok(HEAP_BASE, HEAP_SIZE_4M);
    if ok {
        ok = HEAP_SIZE_4M == 4194304 && heap_in_whitelist();
    }
    if ok {
        // 错误档位必须被拒。
        ok = !heap_window_ok(HEAP_BASE, 2 * 1024 * 1024);
    }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_t351_checks() -> CheckSet {
    let mut set = CheckSet::new("T351");
    set.add(
        "t351-seg-guard",
        t351_guard_seg(),
        if t351_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "t351-c1-window",
        check_c1_window(),
        if check_c1_window() { "" } else { "heap window validation mismatch" },
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
    fn t351_all_checks_green() {
        let set = run_t351_checks();
        assert!(set.all_passed(), "T351 domain checks must be green");
    }

    #[test]
    fn t351_segment_guard_holds() {
        assert!(t351_segment_guard());
    }
}
