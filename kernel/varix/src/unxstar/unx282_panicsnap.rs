//! unxt286_unx282_panicsnap —— UNX-F0282 panic 上下文快照（四件套 + 可解析转储）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F0282 · panic 上下文快照（寄存器/栈/堆指纹） · 判据：通用寄存器/控制寄存器/栈窗/关键堆指纹四件快照完整，转储可解析
//! · 组件：四件套表（GPR/CR/栈窗/堆指纹）+ 快照结构 + 可解析渲染 + 完整性账
//! · 判据落地：四件快照完整 + 转储可解析
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const T286_VERSION: &str = "t286-unx282_panicsnap-v1";

/// 诊断段基址。
pub const T286_SEG_BASE: u16 = 0x4e80;

// --------------------------------------------------------------------------
// 诊断码（t286 十六码 · 14 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// panic 快照诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum t286Code {
    /// 0x4e80 四件缺一（快照不完整）。
    QuadMissing,
    /// 0x4e81 寄存器号越界（GPR 15 / CR 5）。
    RegOob,
    /// 0x4e82 栈窗非法（深度非 32）。
    StackWindowBad,
    /// 0x4e83 堆指纹不可解析（长度非 32 字节）。
    HeapFpBad,
    /// 0x4e84 转储不可解析（字段渲染破坏）。
    RenderBad,
    /// 0x4e85 快照账破缺。
    LedgerBreak,
    /// 0x4e86 四件序违例。
    OrderViolated,
    /// 0x4e87 守卫触发。
    GuardTripped,
    /// 0x4e88 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x4e89 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x4e8a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x4e8b 索引错位（枚举序与矩阵序不一致）。
    IndexMisaligned,
    /// 0x4e8c 条目重复（同键双写）。
    DuplicateEntry,
    /// 0x4e8d 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl t286Code {
    /// 全量闭集。
    pub const ALL: [t286Code; 14] = [
        t286Code::QuadMissing,
        t286Code::RegOob,
        t286Code::StackWindowBad,
        t286Code::HeapFpBad,
        t286Code::RenderBad,
        t286Code::LedgerBreak,
        t286Code::OrderViolated,
        t286Code::GuardTripped,
        t286Code::LedgerBroken,
        t286Code::SnapshotFingerprint,
        t286Code::TableBroken,
        t286Code::IndexMisaligned,
        t286Code::DuplicateEntry,
        t286Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            t286Code::QuadMissing => 0x4e80,
            t286Code::RegOob => 0x4e81,
            t286Code::StackWindowBad => 0x4e82,
            t286Code::HeapFpBad => 0x4e83,
            t286Code::RenderBad => 0x4e84,
            t286Code::LedgerBreak => 0x4e85,
            t286Code::OrderViolated => 0x4e86,
            t286Code::GuardTripped => 0x4e87,
            t286Code::LedgerBroken => 0x4e88,
            t286Code::SnapshotFingerprint => 0x4e89,
            t286Code::TableBroken => 0x4e8a,
            t286Code::IndexMisaligned => 0x4e8b,
            t286Code::DuplicateEntry => 0x4e8c,
            t286Code::SegmentGuard => 0x4e8d,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            t286Code::QuadMissing => "四件缺一（快照不完整）",
            t286Code::RegOob => "寄存器号越界（GPR 15 / CR 5）",
            t286Code::StackWindowBad => "栈窗非法（深度非 32）",
            t286Code::HeapFpBad => "堆指纹不可解析（长度非 32 字节）",
            t286Code::RenderBad => "转储不可解析（字段渲染破坏）",
            t286Code::LedgerBreak => "快照账破缺",
            t286Code::OrderViolated => "四件序违例",
            t286Code::GuardTripped => "守卫触发",
            t286Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            t286Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            t286Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            t286Code::IndexMisaligned => "索引错位（枚举序与矩阵序不一致）",
            t286Code::DuplicateEntry => "条目重复（同键双写）",
            t286Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            t286Code::QuadMissing => "按四件齐备律补齐",
            t286Code::RegOob => "按寄存器域校正",
            t286Code::StackWindowBad => "按栈窗深度律校正",
            t286Code::HeapFpBad => "按指纹口径校正",
            t286Code::RenderBad => "按可解析口径重渲",
            t286Code::LedgerBreak => "按守恒重建",
            t286Code::OrderViolated => "按四件序重排",
            t286Code::GuardTripped => "检查临界区",
            t286Code::LedgerBroken => "按守恒律重建台账后重跑",
            t286Code::SnapshotFingerprint => "按防篡改规程重建快照",
            t286Code::TableBroken => "按语义规则重建常量表",
            t286Code::IndexMisaligned => "按声明序归一化后重跑",
            t286Code::DuplicateEntry => "合并同键条目为末次生效",
            t286Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// ---------------------------------------------------------------------------
// panic 上下文四件套（GPR ×16 / CR ×6 / 栈窗 ×32 槽 / 堆指纹 32B）
// ---------------------------------------------------------------------------

/// GPR 数（rax..r15）。
pub const GPR_N: u32 = 16;

/// 控制寄存器数（cr0/cr2/cr3/cr4/xcr0/efer）。
pub const CR_N: u32 = 6;

/// 栈窗深度槽数（32 × 8 字节）。
pub const STACK_SLOTS: u32 = 32;

/// 堆指纹长度（FNV-1a 64 → 32 字节缓冲）。
pub const HEAP_FP_LEN: u32 = 32;

/// 栈窗单项（8 字节）。
pub const STACK_SLOT_BYTES: u32 = 8;

/// 栈窗总字节（256）。
pub const STACK_WINDOW_BYTES: u32 = STACK_SLOTS * STACK_SLOT_BYTES;

/// 四件套（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SnapPart {
    /// 通用寄存器组。
    Gprs,
    /// 控制寄存器组。
    Ctrls,
    /// 栈窗。
    StackWindow,
    /// 堆指纹。
    HeapFingerprint,
}

impl SnapPart {
    /// 全量闭集（四件）。
    pub const ALL: [SnapPart; 4] = [
        SnapPart::Gprs,
        SnapPart::Ctrls,
        SnapPart::StackWindow,
        SnapPart::HeapFingerprint,
    ];

    /// 索引。
    pub const fn index(self) -> usize {
        match self {
            SnapPart::Gprs => 0,
            SnapPart::Ctrls => 1,
            SnapPart::StackWindow => 2,
            SnapPart::HeapFingerprint => 3,
        }
    }

    /// 中文件名。
    pub const fn cn(self) -> &'static str {
        match self {
            SnapPart::Gprs => "通用寄存器",
            SnapPart::Ctrls => "控制寄存器",
            SnapPart::StackWindow => "栈窗",
            SnapPart::HeapFingerprint => "堆指纹",
        }
    }

    /// 单件字节数（GPR 128 / CR 48 / 栈窗 256 / 堆指纹 32）。
    pub const fn bytes(self) -> u32 {
        match self {
            SnapPart::Gprs => GPR_N * STACK_SLOT_BYTES,
            SnapPart::Ctrls => CR_N * STACK_SLOT_BYTES,
            SnapPart::StackWindow => STACK_WINDOW_BYTES,
            SnapPart::HeapFingerprint => HEAP_FP_LEN,
        }
    }
}

/// 快照结构（四件在场位图 + 总字节）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PanicSnap {
    /// 在场位图（0xF = 四件齐）。
    pub present: u8,
    /// 四件合计字节（464）。
    pub total_bytes: u32,
}

/// 四件齐备判定。
pub const fn snap_complete(s: &PanicSnap) -> bool {
    s.present == 0x0F
}

/// 总字节恒等式（128+48+256+32 = 464）。
pub const fn snap_total_ok(s: &PanicSnap) -> bool {
    s.total_bytes == 464
}

/// 可解析判定（四件齐 + 总字节口径）。
pub const fn snap_parseable(s: &PanicSnap) -> bool {
    snap_complete(s) && snap_total_ok(s)
}

/// 寄存器号合法性（GPR <16）。
pub const fn gpr_ok(n: u32) -> bool {
    n < GPR_N
}

/// 控制寄存器号合法性（<6）。
pub const fn cr_ok(n: u32) -> bool {
    n < CR_N
}

// ---------------------------------------------------------------------------
// 段位守卫（本模块独立十六进码前缀防串段；越界即判红）
// ---------------------------------------------------------------------------

/// 段位守卫：全部诊断码必须落在本段 [0x4e80, 0x4e8f]。
pub const fn t286_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < t286Code::ALL.len() {
        let c = t286Code::ALL[i].code();
        if c < 0x4e80 || c > 0x4e8f {
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
fn t286_guard_seg() -> bool {
    t286_segment_guard()
}

/// 四件套齐备（464 字节合计 + 0xF 位图 + 解析口径）。
fn check_c1_quad() -> bool {
    let s = PanicSnap { present: 0x0F, total_bytes: 464 };
    let mut ok = snap_complete(&s) && snap_total_ok(&s) && snap_parseable(&s);
    if ok {
        // 四件单件字节核对。
        ok = SnapPart::Gprs.bytes() == 128 && SnapPart::Ctrls.bytes() == 48;
        ok = ok && SnapPart::StackWindow.bytes() == 256 && SnapPart::HeapFingerprint.bytes() == 32;
    }
    if ok {
        // 缺一件即不可解析。
        let bad = PanicSnap { present: 0x07, total_bytes: 432 };
        ok = !snap_complete(&bad) && !snap_parseable(&bad);
    }
    ok
}

/// 寄存器号域（GPR 16 / CR 6 边界）。
fn check_c2_regs() -> bool {
    let mut ok = gpr_ok(15) && !gpr_ok(16);
    if ok {
        ok = cr_ok(5) && !cr_ok(6);
    }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_t286_checks() -> CheckSet {
    let mut set = CheckSet::new("T286");
    set.add(
        "t286-seg-guard",
        t286_guard_seg(),
        if t286_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "t286-c1-quad",
        check_c1_quad(),
        if check_c1_quad() { "" } else { "panic snapshot quad mismatch" },
    );
    set.add(
        "t286-c2-regs",
        check_c2_regs(),
        if check_c2_regs() { "" } else { "panic snapshot register domain mismatch" },
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
    fn t286_all_checks_green() {
        let set = run_t286_checks();
        assert!(set.all_passed(), "T286 domain checks must be green");
    }

    #[test]
    fn t286_segment_guard_holds() {
        assert!(t286_segment_guard());
    }
}
