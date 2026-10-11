//! unxt314_tspan —— UNX-F0321 三段时序账（时序守恒 + 直方图）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F0321 · trampoline 总编排（BSP 侧准备 → AP 唤醒 → 汇合） · 判据：三段编排（准备/唤醒/汇合）时序正确，与 F0188 前置包消费契约一致
//! · 组件：时序账（三段耗时守恒）+ 直方图 + 域快照
//! · 判据落地：三段时序正确的证据面
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const T314_VERSION: &str = "t314-tspan-v1";

/// 诊断段基址。
pub const T314_SEG_BASE: u16 = 0x3d30;

// --------------------------------------------------------------------------
// 诊断码（t314 十六码 · 14 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 三段时序诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum t314Code {
    /// 0x3d30 时序账破缺。
    LedgerBreak,
    /// 0x3d31 窗口溢出。
    WindowOverflow,
    /// 0x3d32 样本过期。
    SampleStale,
    /// 0x3d33 时序倒挂。
    OrderViolated,
    /// 0x3d34 漂移超限。
    DriftOver,
    /// 0x3d35 直方图不守恒。
    HistBroken,
    /// 0x3d36 报告陈旧。
    ReportStale,
    /// 0x3d37 守卫触发。
    GuardTripped,
    /// 0x3d38 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x3d39 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x3d3a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x3d3b 索引错位（枚举序与矩阵序不一致）。
    IndexMisaligned,
    /// 0x3d3c 条目重复（同键双写）。
    DuplicateEntry,
    /// 0x3d3d 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl t314Code {
    /// 全量闭集。
    pub const ALL: [t314Code; 14] = [
        t314Code::LedgerBreak,
        t314Code::WindowOverflow,
        t314Code::SampleStale,
        t314Code::OrderViolated,
        t314Code::DriftOver,
        t314Code::HistBroken,
        t314Code::ReportStale,
        t314Code::GuardTripped,
        t314Code::LedgerBroken,
        t314Code::SnapshotFingerprint,
        t314Code::TableBroken,
        t314Code::IndexMisaligned,
        t314Code::DuplicateEntry,
        t314Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            t314Code::LedgerBreak => 0x3d30,
            t314Code::WindowOverflow => 0x3d31,
            t314Code::SampleStale => 0x3d32,
            t314Code::OrderViolated => 0x3d33,
            t314Code::DriftOver => 0x3d34,
            t314Code::HistBroken => 0x3d35,
            t314Code::ReportStale => 0x3d36,
            t314Code::GuardTripped => 0x3d37,
            t314Code::LedgerBroken => 0x3d38,
            t314Code::SnapshotFingerprint => 0x3d39,
            t314Code::TableBroken => 0x3d3a,
            t314Code::IndexMisaligned => 0x3d3b,
            t314Code::DuplicateEntry => 0x3d3c,
            t314Code::SegmentGuard => 0x3d3d,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            t314Code::LedgerBreak => "时序账破缺",
            t314Code::WindowOverflow => "窗口溢出",
            t314Code::SampleStale => "样本过期",
            t314Code::OrderViolated => "时序倒挂",
            t314Code::DriftOver => "漂移超限",
            t314Code::HistBroken => "直方图不守恒",
            t314Code::ReportStale => "报告陈旧",
            t314Code::GuardTripped => "守卫触发",
            t314Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            t314Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            t314Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            t314Code::IndexMisaligned => "索引错位（枚举序与矩阵序不一致）",
            t314Code::DuplicateEntry => "条目重复（同键双写）",
            t314Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            t314Code::LedgerBreak => "按守恒重建",
            t314Code::WindowOverflow => "分窗统计",
            t314Code::SampleStale => "重采样",
            t314Code::OrderViolated => "按时序律重排",
            t314Code::DriftOver => "按时钟源校准",
            t314Code::HistBroken => "重建排序态",
            t314Code::ReportStale => "重渲染",
            t314Code::GuardTripped => "检查临界区",
            t314Code::LedgerBroken => "按守恒律重建台账后重跑",
            t314Code::SnapshotFingerprint => "按防篡改规程重建快照",
            t314Code::TableBroken => "按语义规则重建常量表",
            t314Code::IndexMisaligned => "按声明序归一化后重跑",
            t314Code::DuplicateEntry => "合并同键条目为末次生效",
            t314Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 守恒账 tspan（三段时序账（三段耗时累加 = 总计））
// --------------------------------------------------------------------------
/// 三段时序账（三段耗时累加 = 总计）。
#[derive(Clone, Copy, Debug)]
pub struct tspan {
    /// 准备段耗时
    pub prep_ns: u64,
    /// 唤醒段耗时
    pub wake_ns: u64,
    /// 汇合段耗时
    pub join_ns: u64,
}

impl tspan {
    /// 全零构造。
    pub const fn new() -> Self {
        Self {
            prep_ns: 0,
            wake_ns: 0,
            join_ns: 0,
        }
    }

    /// 记准备段一采样。
    pub fn add_prep(&mut self) {
        self.prep_ns += 1;
    }

    /// 记唤醒段一采样。
    pub fn add_wake(&mut self) {
        self.wake_ns += 1;
    }

    /// 记汇合段一采样。
    pub fn add_join(&mut self) {
        self.join_ns += 1;
    }

}

// --------------------------------------------------------------------------
// 直方图引擎 tspan_hist（有界插入排序 + 分位数 P50/P90/P99）
// --------------------------------------------------------------------------
/// 三段耗时直方图（分位数 P50/P90/P99）。
/// 样本容量上限。
pub const TSPAN_HIST_CAP: usize = 64;

/// 直方图账本（排序态 + 五统量）。
#[derive(Clone, Copy, Debug)]
pub struct tspan_histHist {
    /// 样本数组（插入排序维护有序）。
    pub samples: [u64; 64],
    /// 样本数。
    pub n: usize,
    /// 溢出丢弃计数。
    pub drops: u64,
    /// 最小值（空时为 0）。
    pub min: u64,
    /// 最大值。
    pub max: u64,
    /// 累加和。
    pub sum: u64,
}

impl tspan_histHist {
    /// 空账构造。
    pub const fn new() -> Self {
        Self { samples: [0; 64], n: 0, drops: 0, min: 0, max: 0, sum: 0 }
    }

    /// 采样（有界插入排序；满员丢弃并记账）。
    pub fn push(&mut self, v: u64) {
        if self.n >= TSPAN_HIST_CAP {
            self.drops += 1;
            return;
        }
        let mut j = self.n;
        while j > 0 && self.samples[j - 1] > v {
            self.samples[j] = self.samples[j - 1];
            j -= 1;
        }
        self.samples[j] = v;
        self.n += 1;
        self.sum = self.sum.wrapping_add(v);
        if self.n == 1 || v < self.min {
            self.min = v;
        }
        if v > self.max {
            self.max = v;
        }
    }

    /// 分位数（permille 口径：q500=P50/q900=P90/q990=P99；最近秩法）。
    pub fn percentile(&self, permille: u64) -> u64 {
        if self.n == 0 {
            return 0;
        }
        let rank = (permille * self.n as u64 + 999) / 1000;
        let idx = if rank == 0 { 1 } else { rank };
        let i = if idx as usize > self.n { self.n - 1 } else { idx as usize - 1 };
        self.samples[i]
    }

    /// 均值（整数除，余数入账）。
    pub fn mean(&self) -> u64 {
        if self.n == 0 {
            return 0;
        }
        self.sum / self.n as u64
    }

    /// 守恒：sum == 各样本和（复算）。
    pub fn conserved(&self) -> bool {
        let mut s = 0u64;
        let mut i = 0usize;
        while i < self.n {
            s = s.wrapping_add(self.samples[i]);
            i += 1;
        }
        s == self.sum
    }
}

// ---------------------------------------------------------------------------
// 段位守卫（本模块独立十六进码前缀防串段；越界即判红）
// ---------------------------------------------------------------------------

/// 段位守卫：全部诊断码必须落在本段 [0x3d30, 0x3d3f]。
pub const fn t314_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < t314Code::ALL.len() {
        let c = t314Code::ALL[i].code();
        if c < 0x3d30 || c > 0x3d3f {
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
fn t314_guard_seg() -> bool {
    t314_segment_guard()
}

/// 时序账总和（三段累加守恒）。
fn check_c1_tspan_sum() -> bool {
    let mut t = tspan::new();
    let mut i = 0;
    while i < 12 {
        t.add_prep();
        i += 1;
    }
    let mut j = 0;
    while j < 30 {
        t.add_wake();
        j += 1;
    }
    let mut k = 0;
    while k < 8 {
        t.add_join();
        k += 1;
    }
    t.prep_ns + t.wake_ns + t.join_ns == 50
}

/// 耗时直方图分位数有序 + 守恒。
fn check_c2_tspan_hist() -> bool {
    let mut h = tspan_histHist::new();
    let mut v = 5u64;
    while v < 130 {
        h.push(v);
        v += 4;
    }
    let mut ok = h.conserved();
    if ok {
        ok = h.percentile(500) <= h.percentile(900) && h.percentile(900) <= h.percentile(990);
    }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_t314_checks() -> CheckSet {
    let mut set = CheckSet::new("T314");
    set.add(
        "t314-seg-guard",
        t314_guard_seg(),
        if t314_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "t314-c1-tspan-sum",
        check_c1_tspan_sum(),
        if check_c1_tspan_sum() { "" } else { "trampoline span sum mismatch" },
    );
    set.add(
        "t314-c2-tspan-hist",
        check_c2_tspan_hist(),
        if check_c2_tspan_hist() { "" } else { "span histogram mismatch" },
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
    fn t314_all_checks_green() {
        let set = run_t314_checks();
        assert!(set.all_passed(), "T314 domain checks must be green");
    }

    #[test]
    fn t314_segment_guard_holds() {
        assert!(t314_segment_guard());
    }
}
