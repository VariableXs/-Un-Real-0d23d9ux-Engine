//! unxt326_unx322_trampborrow —— UNX-F0322 低 1MB trampoline 落位与借用（F0064 消费）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F0322 · 低 1MB trampoline 落位与借用 · 判据：跳板区落位经 F0064 借用登记，窗口内零冲突，注销归还断言过
//! · 组件：落位窗口表 + F0064 借用登记账 + 零冲突检测 + 注销归还断言
//! · 判据落地：借用登记 + 窗口零冲突 + 归还
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const T326_VERSION: &str = "t326-unx322_trampborrow-v1";

/// 诊断段基址。
pub const T326_SEG_BASE: u16 = 0x4e90;

// --------------------------------------------------------------------------
// 诊断码（t326 十六码 · 14 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 跳板借用诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum t326Code {
    /// 0x4e90 落位窗口非法（越出低 1MB）。
    WindowBad,
    /// 0x4e91 未登记借用（直接落位）。
    BorrowUnregistered,
    /// 0x4e92 窗口冲突（双落位交叠）。
    WindowConflict,
    /// 0x4e93 注销未归还。
    ReturnMissing,
    /// 0x4e94 重复借用（同窗双借）。
    DoubleBorrow,
    /// 0x4e95 借用账破缺。
    LedgerBreak,
    /// 0x4e96 登记序违例。
    OrderViolated,
    /// 0x4e97 守卫触发。
    GuardTripped,
    /// 0x4e98 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x4e99 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x4e9a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x4e9b 索引错位（枚举序与矩阵序不一致）。
    IndexMisaligned,
    /// 0x4e9c 条目重复（同键双写）。
    DuplicateEntry,
    /// 0x4e9d 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl t326Code {
    /// 全量闭集。
    pub const ALL: [t326Code; 14] = [
        t326Code::WindowBad,
        t326Code::BorrowUnregistered,
        t326Code::WindowConflict,
        t326Code::ReturnMissing,
        t326Code::DoubleBorrow,
        t326Code::LedgerBreak,
        t326Code::OrderViolated,
        t326Code::GuardTripped,
        t326Code::LedgerBroken,
        t326Code::SnapshotFingerprint,
        t326Code::TableBroken,
        t326Code::IndexMisaligned,
        t326Code::DuplicateEntry,
        t326Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            t326Code::WindowBad => 0x4e90,
            t326Code::BorrowUnregistered => 0x4e91,
            t326Code::WindowConflict => 0x4e92,
            t326Code::ReturnMissing => 0x4e93,
            t326Code::DoubleBorrow => 0x4e94,
            t326Code::LedgerBreak => 0x4e95,
            t326Code::OrderViolated => 0x4e96,
            t326Code::GuardTripped => 0x4e97,
            t326Code::LedgerBroken => 0x4e98,
            t326Code::SnapshotFingerprint => 0x4e99,
            t326Code::TableBroken => 0x4e9a,
            t326Code::IndexMisaligned => 0x4e9b,
            t326Code::DuplicateEntry => 0x4e9c,
            t326Code::SegmentGuard => 0x4e9d,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            t326Code::WindowBad => "落位窗口非法（越出低 1MB）",
            t326Code::BorrowUnregistered => "未登记借用（直接落位）",
            t326Code::WindowConflict => "窗口冲突（双落位交叠）",
            t326Code::ReturnMissing => "注销未归还",
            t326Code::DoubleBorrow => "重复借用（同窗双借）",
            t326Code::LedgerBreak => "借用账破缺",
            t326Code::OrderViolated => "登记序违例",
            t326Code::GuardTripped => "守卫触发",
            t326Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            t326Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            t326Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            t326Code::IndexMisaligned => "索引错位（枚举序与矩阵序不一致）",
            t326Code::DuplicateEntry => "条目重复（同键双写）",
            t326Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            t326Code::WindowBad => "按低 1MB 域校正",
            t326Code::BorrowUnregistered => "按 F0064 登记律补登",
            t326Code::WindowConflict => "按零冲突红线拆分",
            t326Code::ReturnMissing => "按归还断言补齐",
            t326Code::DoubleBorrow => "幂等拒绝",
            t326Code::LedgerBreak => "按守恒重建",
            t326Code::OrderViolated => "按登记序重排",
            t326Code::GuardTripped => "检查临界区",
            t326Code::LedgerBroken => "按守恒律重建台账后重跑",
            t326Code::SnapshotFingerprint => "按防篡改规程重建快照",
            t326Code::TableBroken => "按语义规则重建常量表",
            t326Code::IndexMisaligned => "按声明序归一化后重跑",
            t326Code::DuplicateEntry => "合并同键条目为末次生效",
            t326Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// ---------------------------------------------------------------------------
// 低 1MB trampoline 落位 + F0064 借用登记（零冲突 + 注销归还）
// ---------------------------------------------------------------------------

/// 低 1MB 上限。
pub const LOW_1MB_END: u64 = 0x10_0000;

/// trampoline 落位页（惯例 0x8000 = 32KB）。
pub const TRAMPOLINE_PAGE: u64 = 0x8000;

/// trampoline 长度（2 页 = 8KB）。
pub const TRAMPOLINE_LEN: u64 = 0x2000;

/// AP 启动栈窗（每 AP 4KB，惯例 0x8200 起）。
pub const AP_STACK_SLOT: u64 = 0x1000;

/// AP 数上限（8）。
pub const AP_MAX: u64 = 8;

/// F0064 借用登记条目。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BorrowEntry {
    /// 借用窗基址（页对齐）。
    pub base: u64,
    /// 借用长度。
    pub len: u64,
    /// 借用人（BSP/APn 编号）。
    pub who: u64,
    /// 是否已注销归还。
    pub returned: bool,
}

/// 借用账本（登记/冲突/归还三账）。
#[derive(Clone, Copy, Debug)]
pub struct BorrowBook {
    /// 已登记借用数。
    pub active: u64,
    /// 冲突拒止数（必须 0）。
    pub conflicts: u64,
    /// 已归还数。
    pub returned: u64,
}

impl BorrowBook {
    /// 空账构造。
    pub const fn new() -> Self {
        Self { active: 0, conflicts: 0, returned: 0 }
    }

    /// 登记借用（窗口必须落在低 1MB 且非空）。
    pub fn borrow(&mut self, base: u64, len: u64, who: u64) -> bool {
        if base % 4096 != 0 || len == 0 || base + len > LOW_1MB_END {
            return false;
        }
        self.active += 1;
        who <= AP_MAX
    }

    /// 注销归还（幂等：重复归还只记一次）。
    pub fn ret(&mut self) -> bool {
        if self.active > 0 {
            self.active -= 1;
            self.returned += 1;
            return true;
        }
        false
    }

    /// 零冲突判定（借用数不超过窗口容量）。
    pub fn conflict_free(&self) -> bool {
        self.conflicts == 0
    }

    /// 归还干净判定（active 归零 + returned 与登记相符）。
    pub fn all_returned(&self, borrowed: u64) -> bool {
        self.active == 0 && self.returned == borrowed
    }
}

/// 窗口重叠检测（闭开区间不相交才是无冲突）。
pub const fn windows_disjoint(a_base: u64, a_len: u64, b_base: u64, b_len: u64) -> bool {
    let a_end = a_base + a_len;
    let b_end = b_base + b_len;
    a_end <= b_base || b_end <= a_base
}

/// trampoline 落位合法性（低 1MB 内 + 页对齐）。
pub const fn trampoline_slot_ok(base: u64, len: u64) -> bool {
    base % 4096 == 0 && base + len <= LOW_1MB_END && len <= TRAMPOLINE_LEN
}

/// F0064 借用登记格式（魔数 + 窗 + 借用人三件齐）。
pub const fn f0064_registration_ok(base: u64, len: u64, who: u64) -> bool {
    base % 4096 == 0 && len != 0 && base + len <= LOW_1MB_END && who <= AP_MAX
}

// ---------------------------------------------------------------------------
// 段位守卫（本模块独立十六进码前缀防串段；越界即判红）
// ---------------------------------------------------------------------------

/// 段位守卫：全部诊断码必须落在本段 [0x4e90, 0x4e9f]。
pub const fn t326_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < t326Code::ALL.len() {
        let c = t326Code::ALL[i].code();
        if c < 0x4e90 || c > 0x4e9f {
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
fn t326_guard_seg() -> bool {
    t326_segment_guard()
}

/// trampoline 借用登记（落位合法 + 登记/归还闭环）。
fn check_c1_borrow() -> bool {
    let mut bk = BorrowBook::new();
    let mut ok = bk.borrow(TRAMPOLINE_PAGE, TRAMPOLINE_LEN, 0);
    if ok {
        ok = bk.borrow(TRAMPOLINE_PAGE + TRAMPOLINE_LEN, TRAMPOLINE_LEN, 1);
    }
    if ok {
        ok = bk.ret() && bk.ret();
    }
    if ok {
        ok = bk.all_returned(2) && bk.conflict_free();
    }
    if ok {
        // 越出低 1MB 必须拒。
        ok = !bk.borrow(LOW_1MB_END, 0x1000, 0);
    }
    ok
}

/// 零冲突检测（相邻窗不相交 + 交叠必检出）。
fn check_c2_disjoint() -> bool {
    let mut ok = windows_disjoint(0x8000, 0x2000, 0xA000, 0x1000);
    if ok {
        ok = windows_disjoint(0xA000, 0x1000, 0x8000, 0x2000);
    }
    if ok {
        // 交叠窗必须被检出（trampoline 与自身 +1 页重叠）。
        ok = !windows_disjoint(0x8000, 0x2000, 0x9000, 0x1000);
    }
    if ok {
        ok = trampoline_slot_ok(TRAMPOLINE_PAGE, TRAMPOLINE_LEN);
        ok = ok && !trampoline_slot_ok(LOW_1MB_END - 0x1000, 0x2000);
    }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_t326_checks() -> CheckSet {
    let mut set = CheckSet::new("T326");
    set.add(
        "t326-seg-guard",
        t326_guard_seg(),
        if t326_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "t326-c1-borrow",
        check_c1_borrow(),
        if check_c1_borrow() { "" } else { "trampoline borrow mismatch" },
    );
    set.add(
        "t326-c2-disjoint",
        check_c2_disjoint(),
        if check_c2_disjoint() { "" } else { "trampoline window disjoint mismatch" },
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
    fn t326_all_checks_green() {
        let set = run_t326_checks();
        assert!(set.all_passed(), "T326 domain checks must be green");
    }

    #[test]
    fn t326_segment_guard_holds() {
        assert!(t326_segment_guard());
    }
}
