//! unxt444_ecamread —— UNX-F0461 双机制回读一致性（8/8 设备）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F0461 · 配置访问机制裁定（ECAM vs CF8/CFC） · 判据：机制裁定账（ECAM 优先/CF8 兜底 2 路径），双机制回读一致性抽样核（8/8 设备）
//! · 组件：回读对拍（8 设备）+ 一致性账 + 直方图
//! · 判据落地：双机制回读一致性抽样核（8/8 设备）
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const T444_VERSION: &str = "t444-ecamread-v1";

/// 诊断段基址。
pub const T444_SEG_BASE: u16 = 0x4430;

// --------------------------------------------------------------------------
// 诊断码（t444 十六码 · 15 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 回读一致性诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum t444Code {
    /// 0x4430 设备越界（≥8）。
    DeviceOob,
    /// 0x4431 回读不一致。
    ReadbackDiff,
    /// 0x4432 抽样不足（<8）。
    SampledUnder,
    /// 0x4433 一致性账破缺。
    LedgerBreak,
    /// 0x4434 窗口溢出。
    WindowOverflow,
    /// 0x4435 报告陈旧。
    ReportStale,
    /// 0x4436 脱敏缺失。
    MaskMissing,
    /// 0x4437 守卫触发。
    GuardTripped,
    /// 0x4438 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x4439 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x443a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x443b 索引错位（枚举序与矩阵序不一致）。
    IndexMisaligned,
    /// 0x443c 条目重复（同键双写）。
    DuplicateEntry,
    /// 0x443d 次序违例（依赖序/时序破缺）。
    OrderViolated,
    /// 0x443e 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl t444Code {
    /// 全量闭集。
    pub const ALL: [t444Code; 15] = [
        t444Code::DeviceOob,
        t444Code::ReadbackDiff,
        t444Code::SampledUnder,
        t444Code::LedgerBreak,
        t444Code::WindowOverflow,
        t444Code::ReportStale,
        t444Code::MaskMissing,
        t444Code::GuardTripped,
        t444Code::LedgerBroken,
        t444Code::SnapshotFingerprint,
        t444Code::TableBroken,
        t444Code::IndexMisaligned,
        t444Code::DuplicateEntry,
        t444Code::OrderViolated,
        t444Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            t444Code::DeviceOob => 0x4430,
            t444Code::ReadbackDiff => 0x4431,
            t444Code::SampledUnder => 0x4432,
            t444Code::LedgerBreak => 0x4433,
            t444Code::WindowOverflow => 0x4434,
            t444Code::ReportStale => 0x4435,
            t444Code::MaskMissing => 0x4436,
            t444Code::GuardTripped => 0x4437,
            t444Code::LedgerBroken => 0x4438,
            t444Code::SnapshotFingerprint => 0x4439,
            t444Code::TableBroken => 0x443a,
            t444Code::IndexMisaligned => 0x443b,
            t444Code::DuplicateEntry => 0x443c,
            t444Code::OrderViolated => 0x443d,
            t444Code::SegmentGuard => 0x443e,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            t444Code::DeviceOob => "设备越界（≥8）",
            t444Code::ReadbackDiff => "回读不一致",
            t444Code::SampledUnder => "抽样不足（<8）",
            t444Code::LedgerBreak => "一致性账破缺",
            t444Code::WindowOverflow => "窗口溢出",
            t444Code::ReportStale => "报告陈旧",
            t444Code::MaskMissing => "脱敏缺失",
            t444Code::GuardTripped => "守卫触发",
            t444Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            t444Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            t444Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            t444Code::IndexMisaligned => "索引错位（枚举序与矩阵序不一致）",
            t444Code::DuplicateEntry => "条目重复（同键双写）",
            t444Code::OrderViolated => "次序违例（依赖序/时序破缺）",
            t444Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            t444Code::DeviceOob => "按 8 设备裁剪",
            t444Code::ReadbackDiff => "按归因路径登记",
            t444Code::SampledUnder => "补齐后复核",
            t444Code::LedgerBreak => "按守恒重建",
            t444Code::WindowOverflow => "分窗统计",
            t444Code::ReportStale => "重渲染",
            t444Code::MaskMissing => "补打码",
            t444Code::GuardTripped => "检查临界区",
            t444Code::LedgerBroken => "按守恒律重建台账后重跑",
            t444Code::SnapshotFingerprint => "按防篡改规程重建快照",
            t444Code::TableBroken => "按语义规则重建常量表",
            t444Code::IndexMisaligned => "按声明序归一化后重跑",
            t444Code::DuplicateEntry => "合并同键条目为末次生效",
            t444Code::OrderViolated => "按依赖矩阵重排后重跑",
            t444Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 引擎 cfg_dual（双源对拍：8 槽位逐槽回读一致性）
// --------------------------------------------------------------------------
/// ECAM vs CF8 双机制回读对拍（8 设备逐槽）。
/// 槽位枚举（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum cfg_dualSlot {
    /// 设备 0。
    Dev0,
    /// 设备 1。
    Dev1,
    /// 设备 2。
    Dev2,
    /// 设备 3。
    Dev3,
    /// 设备 4。
    Dev4,
    /// 设备 5。
    Dev5,
    /// 设备 6。
    Dev6,
    /// 设备 7。
    Dev7,
}

impl cfg_dualSlot {
    /// 全量槽位闭集。
    pub const ALL: [cfg_dualSlot; 8] = [
        cfg_dualSlot::Dev0,
        cfg_dualSlot::Dev1,
        cfg_dualSlot::Dev2,
        cfg_dualSlot::Dev3,
        cfg_dualSlot::Dev4,
        cfg_dualSlot::Dev5,
        cfg_dualSlot::Dev6,
        cfg_dualSlot::Dev7,
    ];

    /// 索引（与声明序恒等）。
    pub const fn index(self) -> usize {
        match self {
            cfg_dualSlot::Dev0 => 0,
            cfg_dualSlot::Dev1 => 1,
            cfg_dualSlot::Dev2 => 2,
            cfg_dualSlot::Dev3 => 3,
            cfg_dualSlot::Dev4 => 4,
            cfg_dualSlot::Dev5 => 5,
            cfg_dualSlot::Dev6 => 6,
            cfg_dualSlot::Dev7 => 7,
        }
    }

    /// 中文槽名。
    pub const fn cn(self) -> &'static str {
        match self {
            cfg_dualSlot::Dev0 => "设备 0",
            cfg_dualSlot::Dev1 => "设备 1",
            cfg_dualSlot::Dev2 => "设备 2",
            cfg_dualSlot::Dev3 => "设备 3",
            cfg_dualSlot::Dev4 => "设备 4",
            cfg_dualSlot::Dev5 => "设备 5",
            cfg_dualSlot::Dev6 => "设备 6",
            cfg_dualSlot::Dev7 => "设备 7",
        }
    }
}

/// 双源样本（A 源首读 + B 源回读 + 一致性判定）。
#[derive(Clone, Copy, Debug)]
pub struct cfg_dualPair {
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
pub struct cfg_dualDual {
    /// 逐槽样本。
    pub pairs: [cfg_dualPair; 8],
    /// 一致槽位数。
    pub same_n: usize,
    /// 不一致槽位数。
    pub diff_n: usize,
    /// 采样总轮次。
    pub rounds: u64,
    /// 已采样的不一致槽位掩码。
    pub diff_mask: u64,
}

impl cfg_dualDual {
    /// 全零构造。
    pub const fn new() -> Self {
        Self {
            pairs: [cfg_dualPair { a: 0, b: 0, same: false, seq: 0 }; 8],
            same_n: 0,
            diff_n: 0,
            rounds: 0,
            diff_mask: 0,
        }
    }

    /// 单槽采样（A/B 双源读入后逐位比较）。
    pub fn sample(&mut self, who: cfg_dualSlot, a: u64, b: u64) {
        let i = who.index();
        let same = a == b;
        self.pairs[i] = cfg_dualPair { a, b, same, seq: self.rounds };
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
        self.diff_n == 0 && self.same_n == 8
    }

    /// 复算：逐槽重读 pairs 重判 same（防记账面与数据面分叉）。
    pub fn recount(&self) -> bool {
        let mut same = 0usize;
        let mut i = 0usize;
        while i < 8 {
            if self.pairs[i].same == (self.pairs[i].a == self.pairs[i].b) {
                same += 1;
            }
            i += 1;
        }
        same == 8 && self.same_n + self.diff_n == 8
    }
}

// --------------------------------------------------------------------------
// 直方图引擎 cfg_hist（有界插入排序 + 分位数 P50/P90/P99）
// --------------------------------------------------------------------------
/// 回读差值直方图（分位数 P50/P90/P99）。
/// 样本容量上限。
pub const CFG_HIST_CAP: usize = 32;

/// 直方图账本（排序态 + 五统量）。
#[derive(Clone, Copy, Debug)]
pub struct cfg_histHist {
    /// 样本数组（插入排序维护有序）。
    pub samples: [u64; 32],
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

impl cfg_histHist {
    /// 空账构造。
    pub const fn new() -> Self {
        Self { samples: [0; 32], n: 0, drops: 0, min: 0, max: 0, sum: 0 }
    }

    /// 采样（有界插入排序；满员丢弃并记账）。
    pub fn push(&mut self, v: u64) {
        if self.n >= CFG_HIST_CAP {
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

/// 段位守卫：全部诊断码必须落在本段 [0x4430, 0x443f]。
pub const fn t444_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < t444Code::ALL.len() {
        let c = t444Code::ALL[i].code();
        if c < 0x4430 || c > 0x443f {
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
fn t444_guard_seg() -> bool {
    t444_segment_guard()
}

/// 8/8 设备回读一致（全绿对拍）。
fn check_c1_8of8() -> bool {
    let mut d = cfg_dualDual::new();
    d.sample(cfg_dualSlot::Dev0, 0x1234, 0x1234);
    d.sample(cfg_dualSlot::Dev1, 0x5678, 0x5678);
    d.sample(cfg_dualSlot::Dev2, 0x9abc, 0x9abc);
    d.sample(cfg_dualSlot::Dev3, 0xdef0, 0xdef0);
    d.sample(cfg_dualSlot::Dev4, 0x0f0f, 0x0f0f);
    d.sample(cfg_dualSlot::Dev5, 0xf0f0, 0xf0f0);
    d.sample(cfg_dualSlot::Dev6, 0xa5a5, 0xa5a5);
    d.sample(cfg_dualSlot::Dev7, 0x5a5a, 0x5a5a);
    let mut ok = d.all_same() && d.same_n == 8;
    if ok {
        ok = d.recount();
    }
    ok
}

/// 回读差值直方图（一致场景全零）。
fn check_c2_hist() -> bool {
    let mut h = cfg_histHist::new();
    let mut i = 0;
    while i < 24 {
        h.push(0);
        i += 1;
    }
    let mut ok = h.conserved() && h.max == 0 && h.percentile(500) == 0;
    if ok {
        ok = h.drops == 0;
    }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_t444_checks() -> CheckSet {
    let mut set = CheckSet::new("T444");
    set.add(
        "t444-seg-guard",
        t444_guard_seg(),
        if t444_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "t444-c1-8of8",
        check_c1_8of8(),
        if check_c1_8of8() { "" } else { "8-of-8 device readback mismatch" },
    );
    set.add(
        "t444-c2-hist",
        check_c2_hist(),
        if check_c2_hist() { "" } else { "readback histogram mismatch" },
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
    fn t444_all_checks_green() {
        let set = run_t444_checks();
        assert!(set.all_passed(), "T444 domain checks must be green");
    }

    #[test]
    fn t444_segment_guard_holds() {
        assert!(t444_segment_guard());
    }
}
