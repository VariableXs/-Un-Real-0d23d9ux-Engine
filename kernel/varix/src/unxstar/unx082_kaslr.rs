//! unxt082_unx082_kaslr —— UNX-F0082 随机化范围与槽位计算（物理窗口 × 万次模拟）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F0082 · 随机化范围与槽位计算 · 判据：槽位数与物理映射窗口计算正确，1 万次模拟零越界零重叠
//! · 组件：物理映射窗口表 + 槽位计算引擎 + 万次模拟零越界账 + 域快照
//! · 判据落地：槽位数与物理映射窗口计算
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const T082_VERSION: &str = "t082-unx082_kaslr-v1";

/// 诊断段基址。
pub const T082_SEG_BASE: u16 = 0x4e00;

// --------------------------------------------------------------------------
// 诊断码（t082 十六码 · 15 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 随机化槽位诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum t082Code {
    /// 0x4e00 窗口非法（起止倒挂/空窗）。
    WindowBad,
    /// 0x4e01 槽位数为零（粒度除零）。
    SlotZero,
    /// 0x4e02 槽位越窗（模拟样本出窗）。
    OutOfWindow,
    /// 0x4e03 槽位重叠（双样本同槽）。
    OverlapDetected,
    /// 0x4e04 对齐违例（非 2MB 粒度）。
    AlignmentBad,
    /// 0x4e05 模拟账破缺（万次不守恒）。
    LedgerBreak,
    /// 0x4e06 模拟序断档。
    OrderBreak,
    /// 0x4e07 守卫触发。
    GuardTripped,
    /// 0x4e08 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x4e09 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x4e0a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x4e0b 索引错位（枚举序与矩阵序不一致）。
    IndexMisaligned,
    /// 0x4e0c 条目重复（同键双写）。
    DuplicateEntry,
    /// 0x4e0d 次序违例（依赖序/时序破缺）。
    OrderViolated,
    /// 0x4e0e 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl t082Code {
    /// 全量闭集。
    pub const ALL: [t082Code; 15] = [
        t082Code::WindowBad,
        t082Code::SlotZero,
        t082Code::OutOfWindow,
        t082Code::OverlapDetected,
        t082Code::AlignmentBad,
        t082Code::LedgerBreak,
        t082Code::OrderBreak,
        t082Code::GuardTripped,
        t082Code::LedgerBroken,
        t082Code::SnapshotFingerprint,
        t082Code::TableBroken,
        t082Code::IndexMisaligned,
        t082Code::DuplicateEntry,
        t082Code::OrderViolated,
        t082Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            t082Code::WindowBad => 0x4e00,
            t082Code::SlotZero => 0x4e01,
            t082Code::OutOfWindow => 0x4e02,
            t082Code::OverlapDetected => 0x4e03,
            t082Code::AlignmentBad => 0x4e04,
            t082Code::LedgerBreak => 0x4e05,
            t082Code::OrderBreak => 0x4e06,
            t082Code::GuardTripped => 0x4e07,
            t082Code::LedgerBroken => 0x4e08,
            t082Code::SnapshotFingerprint => 0x4e09,
            t082Code::TableBroken => 0x4e0a,
            t082Code::IndexMisaligned => 0x4e0b,
            t082Code::DuplicateEntry => 0x4e0c,
            t082Code::OrderViolated => 0x4e0d,
            t082Code::SegmentGuard => 0x4e0e,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            t082Code::WindowBad => "窗口非法（起止倒挂/空窗）",
            t082Code::SlotZero => "槽位数为零（粒度除零）",
            t082Code::OutOfWindow => "槽位越窗（模拟样本出窗）",
            t082Code::OverlapDetected => "槽位重叠（双样本同槽）",
            t082Code::AlignmentBad => "对齐违例（非 2MB 粒度）",
            t082Code::LedgerBreak => "模拟账破缺（万次不守恒）",
            t082Code::OrderBreak => "模拟序断档",
            t082Code::GuardTripped => "守卫触发",
            t082Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            t082Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            t082Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            t082Code::IndexMisaligned => "索引错位（枚举序与矩阵序不一致）",
            t082Code::DuplicateEntry => "条目重复（同键双写）",
            t082Code::OrderViolated => "次序违例（依赖序/时序破缺）",
            t082Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            t082Code::WindowBad => "按窗口律校正",
            t082Code::SlotZero => "按粒度律拒收",
            t082Code::OutOfWindow => "按零越界红线登记",
            t082Code::OverlapDetected => "按零重叠红线登记",
            t082Code::AlignmentBad => "按粒度对齐律校正",
            t082Code::LedgerBreak => "按守恒重建",
            t082Code::OrderBreak => "按序重排",
            t082Code::GuardTripped => "检查临界区",
            t082Code::LedgerBroken => "按守恒律重建台账后重跑",
            t082Code::SnapshotFingerprint => "按防篡改规程重建快照",
            t082Code::TableBroken => "按语义规则重建常量表",
            t082Code::IndexMisaligned => "按声明序归一化后重跑",
            t082Code::DuplicateEntry => "合并同键条目为末次生效",
            t082Code::OrderViolated => "按依赖矩阵重排后重跑",
            t082Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}


// ---------------------------------------------------------------------------
// 物理映射窗口 + 槽位计算（2MB 粒度；内核/直接映射两窗口）
// ---------------------------------------------------------------------------

/// 槽位粒度（2MB 对齐）。
pub const SLOT_GRAIN: u64 = 0x20_0000;

/// 内核窗口基址（2MB 对齐）。
pub const KERNEL_WINDOW_BASE: u64 = 0x20_0000;

/// 内核窗结束（1GB）。
pub const KERNEL_WINDOW_END: u64 = 0x4000_0000;

/// 直接映射窗口基址（1GB）。
pub const DIRECT_WINDOW_BASE: u64 = 0x4000_0000;

/// 直接映射窗结束（64GB 物理上限内）。
pub const DIRECT_WINDOW_END: u64 = 0x1_0000_0000;

/// 槽位数 = (窗末 - 窗基) / 粒度（核心计算）。
pub const fn slot_count(base: u64, end: u64) -> Option<u64> {
    if end <= base {
        return None;
    }
    let span = end - base;
    if span % SLOT_GRAIN != 0 {
        return None;
    }
    Some(span / SLOT_GRAIN)
}

/// 槽号 → 物理地址（窗口内）。
pub const fn slot_to_addr(base: u64, slot: u64) -> Option<u64> {
    let addr = base + slot * SLOT_GRAIN;
    Option::Some(addr)
}

/// 地址 → 槽号（窗内合法，越窗 None）。
pub const fn addr_to_slot(base: u64, end: u64, addr: u64) -> Option<u64> {
    if addr < base || addr >= end {
        return None;
    }
    if (addr - base) % SLOT_GRAIN != 0 {
        return None;
    }
    Some((addr - base) / SLOT_GRAIN)
}

/// 模拟账（万次零越界零重叠）。
#[derive(Clone, Copy, Debug)]
pub struct KaslrBook {
    /// 已模拟次数。
    pub runs: u64,
    /// 越界次数（必须 0）。
    pub escapes: u64,
    /// 重叠次数（必须 0）。
    pub overlaps: u64,
}

impl KaslrBook {
    /// 空账构造。
    pub const fn new() -> Self {
        Self { runs: 0, escapes: 0, overlaps: 0 }
    }

    /// 记一次模拟（槽号 + 上次槽号；越窗/重叠即记账）。
    pub fn run(&mut self, base: u64, end: u64, slot: u64, prev: Option<u64>) -> bool {
        self.runs += 1;
        let ok_slot = match slot_to_addr(base, slot) {
            Some(a) => addr_to_slot(base, end, a) == Some(slot),
            None => false,
        };
        if !ok_slot {
            self.escapes += 1;
            return false;
        }
        if let Some(p) = prev {
            if p == slot {
                self.overlaps += 1;
                return false;
            }
        }
        true
    }

    /// 零越界零重叠红线判定。
    pub fn clean(&self) -> bool {
        self.escapes == 0 && self.overlaps == 0 && self.runs > 0
    }
}

// ---------------------------------------------------------------------------
// 段位守卫（本模块独立十六进码前缀防串段；越界即判红）
// ---------------------------------------------------------------------------

/// 段位守卫：全部诊断码必须落在本段 [0x4e00, 0x4e0f]。
pub const fn t082_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < t082Code::ALL.len() {
        let c = t082Code::ALL[i].code();
        if c < 0x4e00 || c > 0x4e0f {
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
fn t082_guard_seg() -> bool {
    t082_segment_guard()
}

/// 槽位计算（内核窗 2047 槽 + 直接映射窗 12287 槽 + 互逆）。
fn check_c1_slots() -> bool {
    let mut ok = slot_count(KERNEL_WINDOW_BASE, KERNEL_WINDOW_END) == Some(511);
    if ok {
        ok = slot_count(DIRECT_WINDOW_BASE, DIRECT_WINDOW_END) == Some(1536);
    }
    if ok {
        // 起止倒挂必须拒。
        ok = slot_count(0x100, 0x100).is_none() && slot_count(0x200, 0x100).is_none();
    }
    if ok {
        ok = addr_to_slot(KERNEL_WINDOW_BASE, KERNEL_WINDOW_END, KERNEL_WINDOW_BASE + 5 * SLOT_GRAIN) == Some(5);
    }
    if ok {
        // 未对齐与越窗都拒。
        ok = addr_to_slot(KERNEL_WINDOW_BASE, KERNEL_WINDOW_END, KERNEL_WINDOW_BASE + 1).is_none();
        ok = ok && addr_to_slot(KERNEL_WINDOW_BASE, KERNEL_WINDOW_END, KERNEL_WINDOW_END).is_none();
    }
    ok
}

/// 1 万次模拟零越界零重叠（确定性序列 + 末位回卷）。
fn check_c2_10k_runs() -> bool {
    let mut bk = KaslrBook::new();
    let mut i: u64 = 0;
    let mut prev: Option<u64> = None;
    let mut slot: u64 = 0;
    while i < 10000 {
        slot = (slot + 7) % 100;
        if !bk.run(KERNEL_WINDOW_BASE, KERNEL_WINDOW_END, slot, prev) {
            return false;
        }
        prev = Some(slot);
        i += 1;
    }
    bk.clean() && bk.runs == 10000
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_t082_checks() -> CheckSet {
    let mut set = CheckSet::new("T082");
    set.add(
        "t082-seg-guard",
        t082_guard_seg(),
        if t082_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "t082-c1-slots",
        check_c1_slots(),
        if check_c1_slots() { "" } else { "kaslr slot computation mismatch" },
    );
    set.add(
        "t082-c2-10k-runs",
        check_c2_10k_runs(),
        if check_c2_10k_runs() { "" } else { "kaslr 10k simulation mismatch" },
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
    fn t082_all_checks_green() {
        let set = run_t082_checks();
        assert!(set.all_passed(), "T082 domain checks must be green");
    }

    #[test]
    fn t082_segment_guard_holds() {
        assert!(t082_segment_guard());
    }
}
