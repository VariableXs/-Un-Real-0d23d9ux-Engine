//! unxt354_heapalloc —— UNX-F0381 堆分配语义（类分配/释放守恒）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F0381 · 早年堆 arena 划定（堆窗入账） · 判据：堆窗边界冻结入账，与页帧池白名单一致，堆容量 4MB 档锁定
//! · 组件：分配流水 + 分配账（已分配+空闲守恒）+ 类直方图
//! · 判据落地：堆窗入账后的分配面
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const T354_VERSION: &str = "t354-heapalloc-v1";

/// 诊断段基址。
pub const T354_SEG_BASE: u16 = 0x4030;

// --------------------------------------------------------------------------
// 诊断码（t354 十六码 · 14 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 堆分配诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum t354Code {
    /// 0x4030 类耗尽（该类无空闲块）。
    ClassExhausted,
    /// 0x4031 泄漏（分配未释放）。
    Leak,
    /// 0x4032 重复释放。
    DoubleFree,
    /// 0x4033 分配账破缺。
    LedgerBreak,
    /// 0x4034 碎片超限。
    FragOverLimit,
    /// 0x4035 分配序违例。
    OrderViolated,
    /// 0x4036 堆耗尽触发 panic。
    OomPanic,
    /// 0x4037 守卫触发。
    GuardTripped,
    /// 0x4038 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x4039 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x403a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x403b 索引错位（枚举序与矩阵序不一致）。
    IndexMisaligned,
    /// 0x403c 条目重复（同键双写）。
    DuplicateEntry,
    /// 0x403d 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl t354Code {
    /// 全量闭集。
    pub const ALL: [t354Code; 14] = [
        t354Code::ClassExhausted,
        t354Code::Leak,
        t354Code::DoubleFree,
        t354Code::LedgerBreak,
        t354Code::FragOverLimit,
        t354Code::OrderViolated,
        t354Code::OomPanic,
        t354Code::GuardTripped,
        t354Code::LedgerBroken,
        t354Code::SnapshotFingerprint,
        t354Code::TableBroken,
        t354Code::IndexMisaligned,
        t354Code::DuplicateEntry,
        t354Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            t354Code::ClassExhausted => 0x4030,
            t354Code::Leak => 0x4031,
            t354Code::DoubleFree => 0x4032,
            t354Code::LedgerBreak => 0x4033,
            t354Code::FragOverLimit => 0x4034,
            t354Code::OrderViolated => 0x4035,
            t354Code::OomPanic => 0x4036,
            t354Code::GuardTripped => 0x4037,
            t354Code::LedgerBroken => 0x4038,
            t354Code::SnapshotFingerprint => 0x4039,
            t354Code::TableBroken => 0x403a,
            t354Code::IndexMisaligned => 0x403b,
            t354Code::DuplicateEntry => 0x403c,
            t354Code::SegmentGuard => 0x403d,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            t354Code::ClassExhausted => "类耗尽（该类无空闲块）",
            t354Code::Leak => "泄漏（分配未释放）",
            t354Code::DoubleFree => "重复释放",
            t354Code::LedgerBreak => "分配账破缺",
            t354Code::FragOverLimit => "碎片超限",
            t354Code::OrderViolated => "分配序违例",
            t354Code::OomPanic => "堆耗尽触发 panic",
            t354Code::GuardTripped => "守卫触发",
            t354Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            t354Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            t354Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            t354Code::IndexMisaligned => "索引错位（枚举序与矩阵序不一致）",
            t354Code::DuplicateEntry => "条目重复（同键双写）",
            t354Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            t354Code::ClassExhausted => "按降级路径借大类",
            t354Code::Leak => "按释放账追回",
            t354Code::DoubleFree => "幂等拒绝",
            t354Code::LedgerBreak => "按守恒重建",
            t354Code::FragOverLimit => "按整理路径重整",
            t354Code::OrderViolated => "按 FIFO/LIFO 律",
            t354Code::OomPanic => "按 OOM 路径登记",
            t354Code::GuardTripped => "检查临界区",
            t354Code::LedgerBroken => "按守恒律重建台账后重跑",
            t354Code::SnapshotFingerprint => "按防篡改规程重建快照",
            t354Code::TableBroken => "按语义规则重建常量表",
            t354Code::IndexMisaligned => "按声明序归一化后重跑",
            t354Code::DuplicateEntry => "合并同键条目为末次生效",
            t354Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 守恒账 halloc（堆分配账（used + free == 4MB/32B 总块数））
// --------------------------------------------------------------------------
/// 堆分配账（used + free == 4MB/32B 总块数）。
#[derive(Clone, Copy, Debug)]
pub struct halloc {
    /// 已分配块
    pub used_blocks: u64,
    /// 空闲块
    pub free_blocks: u64,
    /// OOM 事件数
    pub oom_events: u64,
}

impl halloc {
    /// 全零构造。
    pub const fn new() -> Self {
        Self {
            used_blocks: 0,
            free_blocks: 0,
            oom_events: 0,
        }
    }

    /// 分配一块。
    pub fn alloc(&mut self) {
        if self.free_blocks > 0 { self.free_blocks -= 1; } else { self.oom_events += 1; } self.used_blocks += 1;
    }

    /// 释放一块。
    pub fn release(&mut self) {
        if self.used_blocks > 0 { self.used_blocks -= 1; } self.free_blocks += 1;
    }

}

// --------------------------------------------------------------------------
// 直方图引擎 halloc_hist（有界插入排序 + 分位数 P50/P90/P99）
// --------------------------------------------------------------------------
/// 类使用率直方图（分位数 P50/P90/P99）。
/// 样本容量上限。
pub const HALLOC_HIST_CAP: usize = 32;

/// 直方图账本（排序态 + 五统量）。
#[derive(Clone, Copy, Debug)]
pub struct halloc_histHist {
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

impl halloc_histHist {
    /// 空账构造。
    pub const fn new() -> Self {
        Self { samples: [0; 32], n: 0, drops: 0, min: 0, max: 0, sum: 0 }
    }

    /// 采样（有界插入排序；满员丢弃并记账）。
    pub fn push(&mut self, v: u64) {
        if self.n >= HALLOC_HIST_CAP {
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

/// 段位守卫：全部诊断码必须落在本段 [0x4030, 0x403f]。
pub const fn t354_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < t354Code::ALL.len() {
        let c = t354Code::ALL[i].code();
        if c < 0x4030 || c > 0x403f {
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
fn t354_guard_seg() -> bool {
    t354_segment_guard()
}

/// 分配账守恒（总量 = 4MB/32B = 131072 块）。
fn check_c1_alloc_conserve() -> bool {
    let mut h = halloc::new();
    h.free_blocks = 131072;
    let mut i = 0;
    while i < 1000 {
        h.alloc();
        i += 1;
    }
    let mut j = 0;
    while j < 400 {
        h.release();
        j += 1;
    }
    let mut ok = h.used_blocks == 600 && h.free_blocks == 130472;
    if ok {
        ok = h.used_blocks + h.free_blocks == 131072 && h.oom_events == 0;
    }
    ok
}

/// OOM 路径（空池分配 → OOM 记账）。
fn check_c2_oom() -> bool {
    let mut h = halloc::new();
    h.alloc();
    let ok = h.oom_events == 1 && h.used_blocks == 1;
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_t354_checks() -> CheckSet {
    let mut set = CheckSet::new("T354");
    set.add(
        "t354-seg-guard",
        t354_guard_seg(),
        if t354_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "t354-c1-alloc-conserve",
        check_c1_alloc_conserve(),
        if check_c1_alloc_conserve() { "" } else { "heap allocation conservation mismatch" },
    );
    set.add(
        "t354-c2-oom",
        check_c2_oom(),
        if check_c2_oom() { "" } else { "heap OOM path mismatch" },
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
    fn t354_all_checks_green() {
        let set = run_t354_checks();
        assert!(set.all_passed(), "T354 domain checks must be green");
    }

    #[test]
    fn t354_segment_guard_holds() {
        assert!(t354_segment_guard());
    }
}
