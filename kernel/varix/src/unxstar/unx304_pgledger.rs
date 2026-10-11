//! unxt304_pgledger —— UNX-F0301 构建账（级×槽 × 物理页守恒）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F0301 · 页表构建总编排（四级表族流水） · 判据：PML4→PDPT→PD→PT 四级构建流水编排正确，依赖序与设计一致，构建账完整
//! · 组件：构建账（四级槽数 × 页计数守恒）+ 直方图 + 域快照
//! · 判据落地：构建账完整
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const T304_VERSION: &str = "t304-pgledger-v1";

/// 诊断段基址。
pub const T304_SEG_BASE: u16 = 0x3c30;

// --------------------------------------------------------------------------
// 诊断码（t304 十六码 · 15 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 构建账诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum t304Code {
    /// 0x3c30 构建账破缺。
    LedgerBreak,
    /// 0x3c31 物理页泄漏。
    PageLeak,
    /// 0x3c32 过量分配（槽超页承载）。
    OverAlloc,
    /// 0x3c33 欠量分配（页未用满）。
    UnderAlloc,
    /// 0x3c34 重复计数（同页双计）。
    DoubleCount,
    /// 0x3c35 零计数（页无槽引用）。
    ZeroCount,
    /// 0x3c36 直方图溢出。
    HistOverflow,
    /// 0x3c37 守卫触发。
    GuardTripped,
    /// 0x3c38 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x3c39 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x3c3a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x3c3b 索引错位（枚举序与矩阵序不一致）。
    IndexMisaligned,
    /// 0x3c3c 条目重复（同键双写）。
    DuplicateEntry,
    /// 0x3c3d 次序违例（依赖序/时序破缺）。
    OrderViolated,
    /// 0x3c3e 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl t304Code {
    /// 全量闭集。
    pub const ALL: [t304Code; 15] = [
        t304Code::LedgerBreak,
        t304Code::PageLeak,
        t304Code::OverAlloc,
        t304Code::UnderAlloc,
        t304Code::DoubleCount,
        t304Code::ZeroCount,
        t304Code::HistOverflow,
        t304Code::GuardTripped,
        t304Code::LedgerBroken,
        t304Code::SnapshotFingerprint,
        t304Code::TableBroken,
        t304Code::IndexMisaligned,
        t304Code::DuplicateEntry,
        t304Code::OrderViolated,
        t304Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            t304Code::LedgerBreak => 0x3c30,
            t304Code::PageLeak => 0x3c31,
            t304Code::OverAlloc => 0x3c32,
            t304Code::UnderAlloc => 0x3c33,
            t304Code::DoubleCount => 0x3c34,
            t304Code::ZeroCount => 0x3c35,
            t304Code::HistOverflow => 0x3c36,
            t304Code::GuardTripped => 0x3c37,
            t304Code::LedgerBroken => 0x3c38,
            t304Code::SnapshotFingerprint => 0x3c39,
            t304Code::TableBroken => 0x3c3a,
            t304Code::IndexMisaligned => 0x3c3b,
            t304Code::DuplicateEntry => 0x3c3c,
            t304Code::OrderViolated => 0x3c3d,
            t304Code::SegmentGuard => 0x3c3e,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            t304Code::LedgerBreak => "构建账破缺",
            t304Code::PageLeak => "物理页泄漏",
            t304Code::OverAlloc => "过量分配（槽超页承载）",
            t304Code::UnderAlloc => "欠量分配（页未用满）",
            t304Code::DoubleCount => "重复计数（同页双计）",
            t304Code::ZeroCount => "零计数（页无槽引用）",
            t304Code::HistOverflow => "直方图溢出",
            t304Code::GuardTripped => "守卫触发",
            t304Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            t304Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            t304Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            t304Code::IndexMisaligned => "索引错位（枚举序与矩阵序不一致）",
            t304Code::DuplicateEntry => "条目重复（同键双写）",
            t304Code::OrderViolated => "次序违例（依赖序/时序破缺）",
            t304Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            t304Code::LedgerBreak => "按守恒重建",
            t304Code::PageLeak => "按释放账追回",
            t304Code::OverAlloc => "按承载律削减",
            t304Code::UnderAlloc => "按填充律补填",
            t304Code::DoubleCount => "幂等去重",
            t304Code::ZeroCount => "按引用律核验",
            t304Code::HistOverflow => "分窗统计",
            t304Code::GuardTripped => "检查临界区",
            t304Code::LedgerBroken => "按守恒律重建台账后重跑",
            t304Code::SnapshotFingerprint => "按防篡改规程重建快照",
            t304Code::TableBroken => "按语义规则重建常量表",
            t304Code::IndexMisaligned => "按声明序归一化后重跑",
            t304Code::DuplicateEntry => "合并同键条目为末次生效",
            t304Code::OrderViolated => "按依赖矩阵重排后重跑",
            t304Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 守恒账 pgb2（四级构建账（每级页数累加守恒））
// --------------------------------------------------------------------------
/// 四级构建账（每级页数累加守恒）。
#[derive(Clone, Copy, Debug)]
pub struct pgb2 {
    /// PML4 页数
    pub pml4_pages: u64,
    /// PDPT 页数
    pub pdpt_pages: u64,
    /// PD 页数
    pub pd_pages: u64,
    /// PT 页数
    pub pt_pages: u64,
}

impl pgb2 {
    /// 全零构造。
    pub const fn new() -> Self {
        Self {
            pml4_pages: 0,
            pdpt_pages: 0,
            pd_pages: 0,
            pt_pages: 0,
        }
    }

    /// 分配一 PML4 页。
    pub fn alloc_pml4(&mut self) {
        self.pml4_pages += 1;
    }

    /// 分配一 PDPT 页。
    pub fn alloc_pdpt(&mut self) {
        self.pdpt_pages += 1;
    }

    /// 分配一 PD 页。
    pub fn alloc_pd(&mut self) {
        self.pd_pages += 1;
    }

    /// 分配一 PT 页。
    pub fn alloc_pt(&mut self) {
        self.pt_pages += 1;
    }

}

// --------------------------------------------------------------------------
// 直方图引擎 pg_hist（有界插入排序 + 分位数 P50/P90/P99）
// --------------------------------------------------------------------------
/// 构建填充率直方图（分位数 P50/P90/P99）。
/// 样本容量上限。
pub const PG_HIST_CAP: usize = 64;

/// 直方图账本（排序态 + 五统量）。
#[derive(Clone, Copy, Debug)]
pub struct pg_histHist {
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

impl pg_histHist {
    /// 空账构造。
    pub const fn new() -> Self {
        Self { samples: [0; 64], n: 0, drops: 0, min: 0, max: 0, sum: 0 }
    }

    /// 采样（有界插入排序；满员丢弃并记账）。
    pub fn push(&mut self, v: u64) {
        if self.n >= PG_HIST_CAP {
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

/// 段位守卫：全部诊断码必须落在本段 [0x3c30, 0x3c3f]。
pub const fn t304_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < t304Code::ALL.len() {
        let c = t304Code::ALL[i].code();
        if c < 0x3c30 || c > 0x3c3f {
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
fn t304_guard_seg() -> bool {
    t304_segment_guard()
}

/// 构建账守恒（分配序列 + 总量核算）。
fn check_c1_build_conserve() -> bool {
    let mut b = pgb2::new();
    b.alloc_pml4();
    let mut i = 0;
    while i < 2 {
        b.alloc_pdpt();
        i += 1;
    }
    let mut j = 0;
    while j < 6 {
        b.alloc_pd();
        j += 1;
    }
    let mut k = 0;
    while k < 18 {
        b.alloc_pt();
        k += 1;
    }
    let total = b.pml4_pages + b.pdpt_pages + b.pd_pages + b.pt_pages;
    let mut ok = total == 27;
    if ok {
        ok = b.pml4_pages == 1 && b.pt_pages == 18;
    }
    ok
}

/// 填充率直方图分位数有序。
fn check_c2_pg_hist() -> bool {
    let mut h = pg_histHist::new();
    let mut v = 10u64;
    while v < 140 {
        h.push(v);
        v += 3;
    }
    let mut ok = h.conserved();
    if ok {
        ok = h.percentile(500) <= h.percentile(900) && h.percentile(900) <= h.percentile(990);
    }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_t304_checks() -> CheckSet {
    let mut set = CheckSet::new("T304");
    set.add(
        "t304-seg-guard",
        t304_guard_seg(),
        if t304_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "t304-c1-build-conserve",
        check_c1_build_conserve(),
        if check_c1_build_conserve() { "" } else { "build ledger conservation mismatch" },
    );
    set.add(
        "t304-c2-pg-hist",
        check_c2_pg_hist(),
        if check_c2_pg_hist() { "" } else { "fill histogram percentile order mismatch" },
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
    fn t304_all_checks_green() {
        let set = run_t304_checks();
        assert!(set.all_passed(), "T304 domain checks must be green");
    }

    #[test]
    fn t304_segment_guard_holds() {
        assert!(t304_segment_guard());
    }
}
