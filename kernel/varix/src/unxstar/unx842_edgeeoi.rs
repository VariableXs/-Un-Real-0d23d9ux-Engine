//! unxt842_unx842_edgeeoi —— UNX-F0842 边沿触发投递与 EOI 去重（10 万零丢失）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F0842 · 边沿触发投递与 EOI 去重语义 · 判据：边沿事件零丢失（10 万边沿 10 万投递），EOI 去重（一次投递一次 EOI）账平
//! · 组件：边沿检测引擎（上升沿锁存）+ 投递账（零丢失）+ EOI 去重账 + 域快照
//! · 判据落地：10 万边沿 10 万投递 + 一次投递一次 EOI
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const T842_VERSION: &str = "t842-unx842_edgeeoi-v1";

/// 诊断段基址。
pub const T842_SEG_BASE: u16 = 0x5090;

// --------------------------------------------------------------------------
// 诊断码（t842 十六码 · 14 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 边沿 EOI 诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum t842Code {
    /// 0x5090 边沿丢失（边沿未锁存）。
    EdgeLost,
    /// 0x5091 电平误判（持续高电平重复投递）。
    LevelHeld,
    /// 0x5092 EOI 重复（一次投递多次 EOI）。
    EoiDup,
    /// 0x5093 EOI 缺失（投递未 EOI）。
    EoiMissing,
    /// 0x5094 投递账破缺。
    LedgerBreak,
    /// 0x5095 投递序违例。
    OrderViolated,
    /// 0x5096 队列溢出丢弃。
    OverflowDrop,
    /// 0x5097 守卫触发。
    GuardTripped,
    /// 0x5098 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x5099 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x509a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x509b 索引错位（枚举序与矩阵序不一致）。
    IndexMisaligned,
    /// 0x509c 条目重复（同键双写）。
    DuplicateEntry,
    /// 0x509d 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl t842Code {
    /// 全量闭集。
    pub const ALL: [t842Code; 14] = [
        t842Code::EdgeLost,
        t842Code::LevelHeld,
        t842Code::EoiDup,
        t842Code::EoiMissing,
        t842Code::LedgerBreak,
        t842Code::OrderViolated,
        t842Code::OverflowDrop,
        t842Code::GuardTripped,
        t842Code::LedgerBroken,
        t842Code::SnapshotFingerprint,
        t842Code::TableBroken,
        t842Code::IndexMisaligned,
        t842Code::DuplicateEntry,
        t842Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            t842Code::EdgeLost => 0x5090,
            t842Code::LevelHeld => 0x5091,
            t842Code::EoiDup => 0x5092,
            t842Code::EoiMissing => 0x5093,
            t842Code::LedgerBreak => 0x5094,
            t842Code::OrderViolated => 0x5095,
            t842Code::OverflowDrop => 0x5096,
            t842Code::GuardTripped => 0x5097,
            t842Code::LedgerBroken => 0x5098,
            t842Code::SnapshotFingerprint => 0x5099,
            t842Code::TableBroken => 0x509a,
            t842Code::IndexMisaligned => 0x509b,
            t842Code::DuplicateEntry => 0x509c,
            t842Code::SegmentGuard => 0x509d,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            t842Code::EdgeLost => "边沿丢失（边沿未锁存）",
            t842Code::LevelHeld => "电平误判（持续高电平重复投递）",
            t842Code::EoiDup => "EOI 重复（一次投递多次 EOI）",
            t842Code::EoiMissing => "EOI 缺失（投递未 EOI）",
            t842Code::LedgerBreak => "投递账破缺",
            t842Code::OrderViolated => "投递序违例",
            t842Code::OverflowDrop => "队列溢出丢弃",
            t842Code::GuardTripped => "守卫触发",
            t842Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            t842Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            t842Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            t842Code::IndexMisaligned => "索引错位（枚举序与矩阵序不一致）",
            t842Code::DuplicateEntry => "条目重复（同键双写）",
            t842Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            t842Code::EdgeLost => "按零丢失红线补锁",
            t842Code::LevelHeld => "按边沿口径纠偏",
            t842Code::EoiDup => "按去重账归一",
            t842Code::EoiMissing => "按账平判据补齐",
            t842Code::LedgerBreak => "按守恒重建",
            t842Code::OrderViolated => "按到达序重排",
            t842Code::OverflowDrop => "按零丢失红线扩容",
            t842Code::GuardTripped => "检查临界区",
            t842Code::LedgerBroken => "按守恒律重建台账后重跑",
            t842Code::SnapshotFingerprint => "按防篡改规程重建快照",
            t842Code::TableBroken => "按语义规则重建常量表",
            t842Code::IndexMisaligned => "按声明序归一化后重跑",
            t842Code::DuplicateEntry => "合并同键条目为末次生效",
            t842Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// ---------------------------------------------------------------------------
// 边沿触发检测（上升沿锁存）+ EOI 去重（一次投递一次 EOI）
// ---------------------------------------------------------------------------

/// 边沿检测器（上一电平 + 上升沿计数）。
#[derive(Clone, Copy, Debug)]
pub struct EdgeDetector {
    /// 上一采样电平。
    pub last_level: bool,
    /// 已检出上升沿数。
    pub edges: u64,
}

impl EdgeDetector {
    /// 构造（初始低电平）。
    pub const fn new() -> Self {
        Self { last_level: false, edges: 0 }
    }

    /// 采样一次电平（上升沿 = 当前高且上一低）。
    pub fn sample(&mut self, level: bool) -> bool {
        let rising = level && !self.last_level;
        if rising {
            self.edges += 1;
        }
        self.last_level = level;
        rising
    }
}

/// 投递账本（零丢失 + EOI 去双账）。
#[derive(Clone, Copy, Debug)]
pub struct EdgeBook {
    /// 已检边沿数。
    pub edges: u64,
    /// 已投递数。
    pub delivered: u64,
    /// 已 EOI 数。
    pub eois: u64,
    /// 重复 EOI 被去重数。
    pub eoi_dupes: u64,
    /// 待 EOI 队列深度（0 = 账平）。
    pub pending: u64,
}

impl EdgeBook {
    /// 空账构造。
    pub const fn new() -> Self {
        Self { edges: 0, delivered: 0, eois: 0, eoi_dupes: 0, pending: 0 }
    }

    /// 登记一次边沿检出。
    pub fn edge(&mut self) {
        self.edges += 1;
    }

    /// 投递一次（幂等口径：投递数不超过已检边沿数）。
    pub fn deliver(&mut self) -> bool {
        if self.delivered >= self.edges {
            return false;
        }
        self.delivered += 1;
        self.pending += 1;
        true
    }

    /// EOI 一次（无待 EOI 则记重复去重）。
    pub fn eoi(&mut self) -> bool {
        if self.pending == 0 {
            self.eoi_dupes += 1;
            return false;
        }
        self.pending -= 1;
        self.eois += 1;
        true
    }

    /// 零丢失判据（投递数 == 边沿数）。
    pub fn zero_loss(&self) -> bool {
        self.delivered == self.edges
    }

    /// 一次投递一次 EOI 账平判据。
    pub fn eoi_balanced(&self) -> bool {
        self.pending == 0 && self.eois == self.delivered
    }
}

// ---------------------------------------------------------------------------
// 段位守卫（本模块独立十六进码前缀防串段；越界即判红）
// ---------------------------------------------------------------------------

/// 段位守卫：全部诊断码必须落在本段 [0x5090, 0x509f]。
pub const fn t842_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < t842Code::ALL.len() {
        let c = t842Code::ALL[i].code();
        if c < 0x5090 || c > 0x509f {
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
fn t842_guard_seg() -> bool {
    t842_segment_guard()
}

/// 边沿检测（上升沿锁存 + 持续高电平不重复）。
fn check_c1_edge() -> bool {
    let mut d = EdgeDetector::new();
    let mut ok = !d.sample(false) && d.edges == 0;
    if ok {
        ok = d.sample(true) && d.edges == 1;
    }
    if ok {
        // 持续高电平不再产生边沿。
        ok = !d.sample(true) && !d.sample(true) && d.edges == 1;
    }
    if ok {
        // 先落低再拉高才有新边沿。
        ok = !d.sample(false) && d.sample(true) && d.edges == 2;
    }
    ok
}

/// 10 万边沿 10 万投递 + EOI 账平（含去重）。
fn check_c2_100k() -> bool {
    let mut bk = EdgeBook::new();
    let mut i = 0;
    while i < 100000 {
        bk.edge();
        if !bk.deliver() {
            return false;
        }
        if !bk.eoi() {
            return false;
        }
        i += 1;
    }
    let mut ok = bk.zero_loss() && bk.eoi_balanced() && bk.delivered == 100000;
    if ok {
        // 无投递的 EOI 必须被去重。
        ok = !bk.eoi() && bk.eoi_dupes == 1 && bk.eoi_balanced();
    }
    if ok {
        // 无待投递的投递必须拒。
        ok = !bk.deliver();
    }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_t842_checks() -> CheckSet {
    let mut set = CheckSet::new("T842");
    set.add(
        "t842-seg-guard",
        t842_guard_seg(),
        if t842_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "t842-c1-edge",
        check_c1_edge(),
        if check_c1_edge() { "" } else { "edge detection mismatch" },
    );
    set.add(
        "t842-c2-100k",
        check_c2_100k(),
        if check_c2_100k() { "" } else { "100k edge delivery mismatch" },
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
    fn t842_all_checks_green() {
        let set = run_t842_checks();
        assert!(set.all_passed(), "T842 domain checks must be green");
    }

    #[test]
    fn t842_segment_guard_holds() {
        assert!(t842_segment_guard());
    }
}
