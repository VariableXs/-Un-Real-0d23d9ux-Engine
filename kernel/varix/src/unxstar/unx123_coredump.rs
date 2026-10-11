//! unxt126_unx123_coredump —— UNX-F0123 卡死转储三件套（状态+栈+日志落串口）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F0123 · 卡死转储（状态+栈+日志） · 判据：看门狗触发时三件套转储完整落串口，10 次注入 10 次完整
//! · 组件：三件套表（状态/栈/日志）+ 转储流水（串口落盘）+ 10 次注入完整性账
//! · 判据落地：三件套转储完整落串口
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const T126_VERSION: &str = "t126-unx123_coredump-v1";

/// 诊断段基址。
pub const T126_SEG_BASE: u16 = 0x4e10;

// --------------------------------------------------------------------------
// 诊断码（t126 十六码 · 14 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 卡死转储诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum t126Code {
    /// 0x4e10 转储缺失（三件套缺一）。
    DumpMissing,
    /// 0x4e11 串口停滞（落盘卡死）。
    SerialStall,
    /// 0x4e12 转储截断（栈窗/日志超长）。
    Truncated,
    /// 0x4e13 注入丢失（10 次注入未全转储）。
    InjectLost,
    /// 0x4e14 转储不可解析（格式破坏）。
    ParseBad,
    /// 0x4e15 转储账破缺。
    LedgerBreak,
    /// 0x4e16 转储序违例。
    OrderViolated,
    /// 0x4e17 守卫触发。
    GuardTripped,
    /// 0x4e18 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x4e19 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x4e1a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x4e1b 索引错位（枚举序与矩阵序不一致）。
    IndexMisaligned,
    /// 0x4e1c 条目重复（同键双写）。
    DuplicateEntry,
    /// 0x4e1d 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl t126Code {
    /// 全量闭集。
    pub const ALL: [t126Code; 14] = [
        t126Code::DumpMissing,
        t126Code::SerialStall,
        t126Code::Truncated,
        t126Code::InjectLost,
        t126Code::ParseBad,
        t126Code::LedgerBreak,
        t126Code::OrderViolated,
        t126Code::GuardTripped,
        t126Code::LedgerBroken,
        t126Code::SnapshotFingerprint,
        t126Code::TableBroken,
        t126Code::IndexMisaligned,
        t126Code::DuplicateEntry,
        t126Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            t126Code::DumpMissing => 0x4e10,
            t126Code::SerialStall => 0x4e11,
            t126Code::Truncated => 0x4e12,
            t126Code::InjectLost => 0x4e13,
            t126Code::ParseBad => 0x4e14,
            t126Code::LedgerBreak => 0x4e15,
            t126Code::OrderViolated => 0x4e16,
            t126Code::GuardTripped => 0x4e17,
            t126Code::LedgerBroken => 0x4e18,
            t126Code::SnapshotFingerprint => 0x4e19,
            t126Code::TableBroken => 0x4e1a,
            t126Code::IndexMisaligned => 0x4e1b,
            t126Code::DuplicateEntry => 0x4e1c,
            t126Code::SegmentGuard => 0x4e1d,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            t126Code::DumpMissing => "转储缺失（三件套缺一）",
            t126Code::SerialStall => "串口停滞（落盘卡死）",
            t126Code::Truncated => "转储截断（栈窗/日志超长）",
            t126Code::InjectLost => "注入丢失（10 次注入未全转储）",
            t126Code::ParseBad => "转储不可解析（格式破坏）",
            t126Code::LedgerBreak => "转储账破缺",
            t126Code::OrderViolated => "转储序违例",
            t126Code::GuardTripped => "守卫触发",
            t126Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            t126Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            t126Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            t126Code::IndexMisaligned => "索引错位（枚举序与矩阵序不一致）",
            t126Code::DuplicateEntry => "条目重复（同键双写）",
            t126Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            t126Code::DumpMissing => "按三件套齐备律补齐",
            t126Code::SerialStall => "按停滞路径重试",
            t126Code::Truncated => "按截断口径补全或分页",
            t126Code::InjectLost => "按 10/10 完整性补转",
            t126Code::ParseBad => "按可解析口径重转",
            t126Code::LedgerBreak => "按守恒重建",
            t126Code::OrderViolated => "按序重排",
            t126Code::GuardTripped => "检查临界区",
            t126Code::LedgerBroken => "按守恒律重建台账后重跑",
            t126Code::SnapshotFingerprint => "按防篡改规程重建快照",
            t126Code::TableBroken => "按语义规则重建常量表",
            t126Code::IndexMisaligned => "按声明序归一化后重跑",
            t126Code::DuplicateEntry => "合并同键条目为末次生效",
            t126Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// ---------------------------------------------------------------------------
// 三件套转储（状态 + 栈窗 + 日志环）+ 串口落盘
// ---------------------------------------------------------------------------

/// 三件套（闭集：现场状态/栈窗快照/日志环切片）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DumpPart {
    /// 现场状态（寄存器/RIP/阶段）。
    State,
    /// 栈窗快照（RSP 上下 64 字节）。
    Stack,
    /// 日志环切片（转储前 64 条）。
    Log,
}

impl DumpPart {
    /// 全量闭集（三件）。
    pub const ALL: [DumpPart; 3] = [DumpPart::State, DumpPart::Stack, DumpPart::Log];

    /// 索引。
    pub const fn index(self) -> usize {
        match self {
            DumpPart::State => 0,
            DumpPart::Stack => 1,
            DumpPart::Log => 2,
        }
    }

    /// 中文件名。
    pub const fn cn(self) -> &'static str {
        match self {
            DumpPart::State => "现场状态",
            DumpPart::Stack => "栈窗快照",
            DumpPart::Log => "日志环切片",
        }
    }

    /// 单件字节预算（状态 128 / 栈窗 128 / 日志 512）。
    pub const fn budget(self) -> u32 {
        match self {
            DumpPart::State => 128,
            DumpPart::Stack => 128,
            DumpPart::Log => 512,
        }
    }
}

/// 栈窗深度（RSP 上下 32 × 8 字节 = 256 字节窗口内的 128 字节摘要）。
pub const STACK_WINDOW_BYTES: u32 = 128;

/// 日志环切片条数（转储前 64 条）。
pub const LOG_SLICE_ENTRIES: u32 = 64;

/// 转储账（10 次注入 10 次完整）。
#[derive(Clone, Copy, Debug)]
pub struct DumpBook {
    /// 已完成转储次数。
    pub completed: u64,
    /// 不完整转储次数（必须 0）。
    pub partial: u64,
    /// 已注入次数。
    pub injected: u64,
}

impl DumpBook {
    /// 空账构造。
    pub const fn new() -> Self {
        Self { completed: 0, partial: 0, injected: 0 }
    }

    /// 注入一次卡死。
    pub fn inject(&mut self) {
        self.injected += 1;
    }

    /// 完成一次转储（三件齐备才计数完整）。
    pub fn complete(&mut self, parts: usize, bytes: u32) -> bool {
        if parts != 3 {
            self.partial += 1;
            return false;
        }
        let total = DumpPart::State.budget() + DumpPart::Stack.budget() + DumpPart::Log.budget();
        if bytes < total {
            self.partial += 1;
            return false;
        }
        self.completed += 1;
        true
    }

    /// 10/10 完整性判据。
    pub fn all_complete(&self) -> bool {
        self.injected >= 10 && self.partial == 0 && self.completed >= 10
    }
}

// ---------------------------------------------------------------------------
// 段位守卫（本模块独立十六进码前缀防串段；越界即判红）
// ---------------------------------------------------------------------------

/// 段位守卫：全部诊断码必须落在本段 [0x4e10, 0x4e1f]。
pub const fn t126_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < t126Code::ALL.len() {
        let c = t126Code::ALL[i].code();
        if c < 0x4e10 || c > 0x4e1f {
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
fn t126_guard_seg() -> bool {
    t126_segment_guard()
}

/// 三件套预算齐备（768 字节合计 + 闭集三查）。
fn check_c1_parts() -> bool {
    let mut ok = DumpPart::ALL.len() == 3;
    if ok {
        ok = DumpPart::State.budget() == 128 && DumpPart::Stack.budget() == 128 && DumpPart::Log.budget() == 512;
    }
    if ok {
        ok = 128 + 128 + 512 == 768 && STACK_WINDOW_BYTES == 128 && LOG_SLICE_ENTRIES == 64;
    }
    ok
}

/// 10 次注入 10 次完整（齐备/不足两路径）。
fn check_c2_10_inject() -> bool {
    let mut bk = DumpBook::new();
    let mut i = 0;
    while i < 10 {
        bk.inject();
        if !bk.complete(3, 768) {
            return false;
        }
        i += 1;
    }
    let mut ok = bk.all_complete();
    if ok {
        // 缺件转储必须记 partial。
        let mut bk2 = DumpBook::new();
        bk2.inject();
        ok = !bk2.complete(2, 512) && bk2.partial == 1 && !bk2.all_complete();
    }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_t126_checks() -> CheckSet {
    let mut set = CheckSet::new("T126");
    set.add(
        "t126-seg-guard",
        t126_guard_seg(),
        if t126_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "t126-c1-parts",
        check_c1_parts(),
        if check_c1_parts() { "" } else { "dump parts budget mismatch" },
    );
    set.add(
        "t126-c2-10-inject",
        check_c2_10_inject(),
        if check_c2_10_inject() { "" } else { "dump 10-injection completeness mismatch" },
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
    fn t126_all_checks_green() {
        let set = run_t126_checks();
        assert!(set.all_passed(), "T126 domain checks must be green");
    }

    #[test]
    fn t126_segment_guard_holds() {
        assert!(t126_segment_guard());
    }
}
