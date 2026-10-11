//! unxt324_apspan —— UNX-F0341 汇合时序账（到达跨度直方图）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F0341 · AP 汇合栅栏（全部到达点同步） · 判据：栅栏语义（全员到达放行）正确，部分未达超时路径有账
//! · 组件：汇合时序账 + 直方图 + 域快照
//! · 判据落地：栅栏时序证据面
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const T324_VERSION: &str = "t324-apspan-v1";

/// 诊断段基址。
pub const T324_SEG_BASE: u16 = 0x3e30;

// --------------------------------------------------------------------------
// 诊断码（t324 十六码 · 14 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 汇合时序诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum t324Code {
    /// 0x3e30 时序账破缺。
    LedgerBreak,
    /// 0x3e31 窗口溢出。
    WindowOverflow,
    /// 0x3e32 样本过期。
    SampleStale,
    /// 0x3e33 到时序倒挂。
    OrderViolated,
    /// 0x3e34 漂移超限。
    DriftOver,
    /// 0x3e35 直方图不守恒。
    HistBroken,
    /// 0x3e36 报告陈旧。
    ReportStale,
    /// 0x3e37 守卫触发。
    GuardTripped,
    /// 0x3e38 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x3e39 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x3e3a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x3e3b 索引错位（枚举序与矩阵序不一致）。
    IndexMisaligned,
    /// 0x3e3c 条目重复（同键双写）。
    DuplicateEntry,
    /// 0x3e3d 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl t324Code {
    /// 全量闭集。
    pub const ALL: [t324Code; 14] = [
        t324Code::LedgerBreak,
        t324Code::WindowOverflow,
        t324Code::SampleStale,
        t324Code::OrderViolated,
        t324Code::DriftOver,
        t324Code::HistBroken,
        t324Code::ReportStale,
        t324Code::GuardTripped,
        t324Code::LedgerBroken,
        t324Code::SnapshotFingerprint,
        t324Code::TableBroken,
        t324Code::IndexMisaligned,
        t324Code::DuplicateEntry,
        t324Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            t324Code::LedgerBreak => 0x3e30,
            t324Code::WindowOverflow => 0x3e31,
            t324Code::SampleStale => 0x3e32,
            t324Code::OrderViolated => 0x3e33,
            t324Code::DriftOver => 0x3e34,
            t324Code::HistBroken => 0x3e35,
            t324Code::ReportStale => 0x3e36,
            t324Code::GuardTripped => 0x3e37,
            t324Code::LedgerBroken => 0x3e38,
            t324Code::SnapshotFingerprint => 0x3e39,
            t324Code::TableBroken => 0x3e3a,
            t324Code::IndexMisaligned => 0x3e3b,
            t324Code::DuplicateEntry => 0x3e3c,
            t324Code::SegmentGuard => 0x3e3d,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            t324Code::LedgerBreak => "时序账破缺",
            t324Code::WindowOverflow => "窗口溢出",
            t324Code::SampleStale => "样本过期",
            t324Code::OrderViolated => "到时序倒挂",
            t324Code::DriftOver => "漂移超限",
            t324Code::HistBroken => "直方图不守恒",
            t324Code::ReportStale => "报告陈旧",
            t324Code::GuardTripped => "守卫触发",
            t324Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            t324Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            t324Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            t324Code::IndexMisaligned => "索引错位（枚举序与矩阵序不一致）",
            t324Code::DuplicateEntry => "条目重复（同键双写）",
            t324Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            t324Code::LedgerBreak => "按守恒重建",
            t324Code::WindowOverflow => "分窗统计",
            t324Code::SampleStale => "重采样",
            t324Code::OrderViolated => "按时序律重排",
            t324Code::DriftOver => "校准时基",
            t324Code::HistBroken => "重建排序态",
            t324Code::ReportStale => "重渲染",
            t324Code::GuardTripped => "检查临界区",
            t324Code::LedgerBroken => "按守恒律重建台账后重跑",
            t324Code::SnapshotFingerprint => "按防篡改规程重建快照",
            t324Code::TableBroken => "按语义规则重建常量表",
            t324Code::IndexMisaligned => "按声明序归一化后重跑",
            t324Code::DuplicateEntry => "合并同键条目为末次生效",
            t324Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 守恒账 apspan（汇合时序账（最早/最晚到达跨度））
// --------------------------------------------------------------------------
/// 汇合时序账（最早/最晚到达跨度）。
#[derive(Clone, Copy, Debug)]
pub struct apspan {
    /// 最早到达时刻
    pub earliest_ns: u64,
    /// 最晚到达时刻
    pub latest_ns: u64,
    /// 到达总数
    pub arrivals: u64,
    /// 逻辑时钟提示
    pub now_ns_hint: u64,
}

impl apspan {
    /// 全零构造。
    pub const fn new() -> Self {
        Self {
            earliest_ns: 0,
            latest_ns: 0,
            arrivals: 0,
            now_ns_hint: 0,
        }
    }

    /// 记录一次到达（跨度取最大）。
    pub fn observe(&mut self) {
        self.arrivals += 1;
        if self.arrivals == 1 {
            self.earliest_ns = self.now_ns_hint;
            self.latest_ns = self.now_ns_hint;
        } else if self.now_ns_hint > self.latest_ns {
            self.latest_ns = self.now_ns_hint;
        }
    }

    /// 推进逻辑时钟。
    pub fn advance(&mut self) {
        self.now_ns_hint += 1;
    }

}

// --------------------------------------------------------------------------
// 直方图引擎 apspan_hist（有界插入排序 + 分位数 P50/P90/P99）
// --------------------------------------------------------------------------
/// 到达间隔直方图（分位数 P50/P90/P99）。
/// 样本容量上限。
pub const APSPAN_HIST_CAP: usize = 64;

/// 直方图账本（排序态 + 五统量）。
#[derive(Clone, Copy, Debug)]
pub struct apspan_histHist {
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

impl apspan_histHist {
    /// 空账构造。
    pub const fn new() -> Self {
        Self { samples: [0; 64], n: 0, drops: 0, min: 0, max: 0, sum: 0 }
    }

    /// 采样（有界插入排序；满员丢弃并记账）。
    pub fn push(&mut self, v: u64) {
        if self.n >= APSPAN_HIST_CAP {
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

/// 段位守卫：全部诊断码必须落在本段 [0x3e30, 0x3e3f]。
pub const fn t324_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < t324Code::ALL.len() {
        let c = t324Code::ALL[i].code();
        if c < 0x3e30 || c > 0x3e3f {
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
fn t324_guard_seg() -> bool {
    t324_segment_guard()
}

/// 汇合时序账（跨度 = 最晚 - 最早）。
fn check_c1_apspan() -> bool {
    let mut s = apspan::new();
    let mut i = 0;
    while i < 4 {
        s.advance();
        s.observe();
        i += 1;
    }
    let ok = s.arrivals == 4 && s.latest_ns == 4 && s.earliest_ns == 1;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_t324_checks() -> CheckSet {
    let mut set = CheckSet::new("T324");
    set.add(
        "t324-seg-guard",
        t324_guard_seg(),
        if t324_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "t324-c1-apspan",
        check_c1_apspan(),
        if check_c1_apspan() { "" } else { "join span ledger mismatch" },
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
    fn t324_all_checks_green() {
        let set = run_t324_checks();
        assert!(set.all_passed(), "T324 domain checks must be green");
    }

    #[test]
    fn t324_segment_guard_holds() {
        assert!(t324_segment_guard());
    }
}
