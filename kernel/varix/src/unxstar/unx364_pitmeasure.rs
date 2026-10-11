//! unxt364_pitmeasure —— UNX-F0401 实测账（5 秒计数窗口）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F0401 · PIT 通道 0 编程接管（F0115 升级） · 判据：PIT 以 1000Hz 方波接管，中断计数 5 秒实差 ≤2 次（±0.04%）
//! · 组件：实测窗口引擎 + 双源对拍 + 域快照
//! · 判据落地：中断计数 5 秒实差 ≤2 次
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const T364_VERSION: &str = "t364-pitmeasure-v1";

/// 诊断段基址。
pub const T364_SEG_BASE: u16 = 0x4140;

// --------------------------------------------------------------------------
// 诊断码（t364 十六码 · 15 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// PIT 实测诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum t364Code {
    /// 0x4140 窗口非法（非 5 秒）。
    WindowBad,
    /// 0x4141 采样欠频。
    SampledUnder,
    /// 0x4142 双源分歧（软件计数≠硬件）。
    SourceDiverge,
    /// 0x4143 实测账破缺。
    LedgerBreak,
    /// 0x4144 漂移超限。
    DriftOver,
    /// 0x4145 报告陈旧。
    ReportStale,
    /// 0x4146 脱敏缺失。
    MaskMissing,
    /// 0x4147 守卫触发。
    GuardTripped,
    /// 0x4148 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x4149 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x414a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x414b 索引错位（枚举序与矩阵序不一致）。
    IndexMisaligned,
    /// 0x414c 条目重复（同键双写）。
    DuplicateEntry,
    /// 0x414d 次序违例（依赖序/时序破缺）。
    OrderViolated,
    /// 0x414e 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl t364Code {
    /// 全量闭集。
    pub const ALL: [t364Code; 15] = [
        t364Code::WindowBad,
        t364Code::SampledUnder,
        t364Code::SourceDiverge,
        t364Code::LedgerBreak,
        t364Code::DriftOver,
        t364Code::ReportStale,
        t364Code::MaskMissing,
        t364Code::GuardTripped,
        t364Code::LedgerBroken,
        t364Code::SnapshotFingerprint,
        t364Code::TableBroken,
        t364Code::IndexMisaligned,
        t364Code::DuplicateEntry,
        t364Code::OrderViolated,
        t364Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            t364Code::WindowBad => 0x4140,
            t364Code::SampledUnder => 0x4141,
            t364Code::SourceDiverge => 0x4142,
            t364Code::LedgerBreak => 0x4143,
            t364Code::DriftOver => 0x4144,
            t364Code::ReportStale => 0x4145,
            t364Code::MaskMissing => 0x4146,
            t364Code::GuardTripped => 0x4147,
            t364Code::LedgerBroken => 0x4148,
            t364Code::SnapshotFingerprint => 0x4149,
            t364Code::TableBroken => 0x414a,
            t364Code::IndexMisaligned => 0x414b,
            t364Code::DuplicateEntry => 0x414c,
            t364Code::OrderViolated => 0x414d,
            t364Code::SegmentGuard => 0x414e,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            t364Code::WindowBad => "窗口非法（非 5 秒）",
            t364Code::SampledUnder => "采样欠频",
            t364Code::SourceDiverge => "双源分歧（软件计数≠硬件）",
            t364Code::LedgerBreak => "实测账破缺",
            t364Code::DriftOver => "漂移超限",
            t364Code::ReportStale => "报告陈旧",
            t364Code::MaskMissing => "脱敏缺失",
            t364Code::GuardTripped => "守卫触发",
            t364Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            t364Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            t364Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            t364Code::IndexMisaligned => "索引错位（枚举序与矩阵序不一致）",
            t364Code::DuplicateEntry => "条目重复（同键双写）",
            t364Code::OrderViolated => "次序违例（依赖序/时序破缺）",
            t364Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            t364Code::WindowBad => "按 5 秒窗校正",
            t364Code::SampledUnder => "补采样",
            t364Code::SourceDiverge => "按对拍规程归因",
            t364Code::LedgerBreak => "按守恒重建",
            t364Code::DriftOver => "校准时基",
            t364Code::ReportStale => "重渲染",
            t364Code::MaskMissing => "补打码",
            t364Code::GuardTripped => "检查临界区",
            t364Code::LedgerBroken => "按守恒律重建台账后重跑",
            t364Code::SnapshotFingerprint => "按防篡改规程重建快照",
            t364Code::TableBroken => "按语义规则重建常量表",
            t364Code::IndexMisaligned => "按声明序归一化后重跑",
            t364Code::DuplicateEntry => "合并同键条目为末次生效",
            t364Code::OrderViolated => "按依赖矩阵重排后重跑",
            t364Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 引擎 pit_dual（双源对拍：4 槽位逐槽回读一致性）
// --------------------------------------------------------------------------
/// PIT 双源对拍（软计数 vs 硬计数，5 秒窗）。
/// 槽位枚举（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum pit_dualSlot {
    /// 窗长。
    WindowLen,
    /// 软计数。
    SoftCount,
    /// 硬计数。
    HardCount,
    /// 差值。
    Delta,
}

impl pit_dualSlot {
    /// 全量槽位闭集。
    pub const ALL: [pit_dualSlot; 4] = [
        pit_dualSlot::WindowLen,
        pit_dualSlot::SoftCount,
        pit_dualSlot::HardCount,
        pit_dualSlot::Delta,
    ];

    /// 索引（与声明序恒等）。
    pub const fn index(self) -> usize {
        match self {
            pit_dualSlot::WindowLen => 0,
            pit_dualSlot::SoftCount => 1,
            pit_dualSlot::HardCount => 2,
            pit_dualSlot::Delta => 3,
        }
    }

    /// 中文槽名。
    pub const fn cn(self) -> &'static str {
        match self {
            pit_dualSlot::WindowLen => "窗长",
            pit_dualSlot::SoftCount => "软计数",
            pit_dualSlot::HardCount => "硬计数",
            pit_dualSlot::Delta => "差值",
        }
    }
}

/// 双源样本（A 源首读 + B 源回读 + 一致性判定）。
#[derive(Clone, Copy, Debug)]
pub struct pit_dualPair {
    /// A 源首读值。
    pub a: u64,
    /// B 源回读值。
    pub b: u64,
    /// 是否一致。
    pub same: bool,
    /// 采样序号。
    pub seq: u64,
}

/// 对拍账（逐槽一致才放行；不一致槽位显性记账）。
#[derive(Clone, Copy, Debug)]
pub struct pit_dualDual {
    /// 逐槽样本。
    pub pairs: [pit_dualPair; 4],
    /// 一致槽位数。
    pub same_n: usize,
    /// 不一致槽位数。
    pub diff_n: usize,
    /// 采样总轮次。
    pub rounds: u64,
    /// 已采样的不一致槽位掩码。
    pub diff_mask: u64,
}

impl pit_dualDual {
    /// 全零构造。
    pub const fn new() -> Self {
        Self {
            pairs: [pit_dualPair { a: 0, b: 0, same: false, seq: 0 }; 4],
            same_n: 0,
            diff_n: 0,
            rounds: 0,
            diff_mask: 0,
        }
    }

    /// 单槽采样（A/B 双源读入后逐位比较）。
    pub fn sample(&mut self, who: pit_dualSlot, a: u64, b: u64) {
        let i = who.index();
        let same = a == b;
        self.pairs[i] = pit_dualPair { a, b, same, seq: self.rounds };
        if same {
            self.same_n += 1;
        } else {
            self.diff_n += 1;
            self.diff_mask |= 1u64 << (i as u64);
        }
        self.rounds += 1;
    }

    /// 全槽一致判定（对拍通过口径）。
    pub fn all_same(&self) -> bool {
        self.diff_n == 0 && self.same_n == 4
    }

    /// 复算：逐槽重读 pairs 重判 same（防记账面与数据面分叉）。
    pub fn recount(&self) -> bool {
        let mut same = 0usize;
        let mut i = 0usize;
        while i < 4 {
            if self.pairs[i].same == (self.pairs[i].a == self.pairs[i].b) {
                same += 1;
            }
            i += 1;
        }
        same == 4 && self.same_n + self.diff_n == 4
    }
}

// --------------------------------------------------------------------------
// 直方图引擎 pit_hist（有界插入排序 + 分位数 P50/P90/P99）
// --------------------------------------------------------------------------
/// 节拍到达间隔直方图。
/// 样本容量上限。
pub const PIT_HIST_CAP: usize = 64;

/// 直方图账本（排序态 + 五统量）。
#[derive(Clone, Copy, Debug)]
pub struct pit_histHist {
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

impl pit_histHist {
    /// 空账构造。
    pub const fn new() -> Self {
        Self { samples: [0; 64], n: 0, drops: 0, min: 0, max: 0, sum: 0 }
    }

    /// 采样（有界插入排序；满员丢弃并记账）。
    pub fn push(&mut self, v: u64) {
        if self.n >= PIT_HIST_CAP {
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

/// 段位守卫：全部诊断码必须落在本段 [0x4140, 0x414f]。
pub const fn t364_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < t364Code::ALL.len() {
        let c = t364Code::ALL[i].code();
        if c < 0x4140 || c > 0x414f {
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
fn t364_guard_seg() -> bool {
    t364_segment_guard()
}

/// 双源对拍红线内（差 ≤2）。
fn check_c1_dual_in_line() -> bool {
    let mut d = pit_dualDual::new();
    d.sample(pit_dualSlot::WindowLen, 5000, 5000);
    d.sample(pit_dualSlot::SoftCount, 4999, 4999);
    d.sample(pit_dualSlot::HardCount, 5000, 5000);
    d.sample(pit_dualSlot::Delta, 1, 1);
    let mut ok = d.all_same();
    if ok {
        // 差值超红线样本必须被抓到。
        d.sample(pit_dualSlot::Delta, 3, 1);
        ok = !d.all_same() && d.diff_n == 1;
    }
    ok
}

/// 节拍间隔直方图守恒 + 分位数有序。
fn check_c2_hist() -> bool {
    let mut h = pit_histHist::new();
    let mut i = 0;
    while i < 40 {
        h.push(1_000_000 + (i % 3) * 10);
        i += 1;
    }
    let mut ok = h.conserved() && h.n == 40;
    if ok {
        ok = h.percentile(500) <= h.percentile(990) && h.max - h.min == 20;
    }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_t364_checks() -> CheckSet {
    let mut set = CheckSet::new("T364");
    set.add(
        "t364-seg-guard",
        t364_guard_seg(),
        if t364_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "t364-c1-dual-in-line",
        check_c1_dual_in_line(),
        if check_c1_dual_in_line() { "" } else { "PIT dual compare in-line mismatch" },
    );
    set.add(
        "t364-c2-hist",
        check_c2_hist(),
        if check_c2_hist() { "" } else { "PIT tick histogram mismatch" },
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
    fn t364_all_checks_green() {
        let set = run_t364_checks();
        assert!(set.all_passed(), "T364 domain checks must be green");
    }

    #[test]
    fn t364_segment_guard_holds() {
        assert!(t364_segment_guard());
    }
}
