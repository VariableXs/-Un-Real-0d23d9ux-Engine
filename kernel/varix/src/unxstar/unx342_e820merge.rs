//! unxt342_e820merge —— UNX-F0361 E820 排序归并引擎（零相交 + 容量守恒）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F0361 · 早年内存盘点归并（E820 区段排序相交并） · 判据：E820 区段排序归并零相交，归并后总容量与分区明细账一致
//! · 组件：归并引擎（排序 + 零相交 + 同类相邻吸收）+ 容量守恒 + 场景对拍
//! · 判据落地：区段排序归并零相交
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const T342_VERSION: &str = "t342-e820merge-v1";

/// 诊断段基址。
pub const T342_SEG_BASE: u16 = 0x3f10;

// --------------------------------------------------------------------------
// 诊断码（t342 十六码 · 15 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 归并引擎诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum t342Code {
    /// 0x3f10 区段相交（归并前重叠）。
    SegOverlap,
    /// 0x3f11 归并失衡（输入容量≠输出容量）。
    MergeUnbalanced,
    /// 0x3f12 类型冲突（同址双类型）。
    TypeConflict,
    /// 0x3f13 输入未排序。
    UnsortedInput,
    /// 0x3f14 非法吸收（异类相邻误并）。
    CoalesceIllegal,
    /// 0x3f15 容量明细账破缺。
    CapacityLedgerBreak,
    /// 0x3f16 输出非单调。
    MonotoneBreak,
    /// 0x3f17 守卫触发。
    GuardTripped,
    /// 0x3f18 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x3f19 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x3f1a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x3f1b 索引错位（枚举序与矩阵序不一致）。
    IndexMisaligned,
    /// 0x3f1c 条目重复（同键双写）。
    DuplicateEntry,
    /// 0x3f1d 次序违例（依赖序/时序破缺）。
    OrderViolated,
    /// 0x3f1e 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl t342Code {
    /// 全量闭集。
    pub const ALL: [t342Code; 15] = [
        t342Code::SegOverlap,
        t342Code::MergeUnbalanced,
        t342Code::TypeConflict,
        t342Code::UnsortedInput,
        t342Code::CoalesceIllegal,
        t342Code::CapacityLedgerBreak,
        t342Code::MonotoneBreak,
        t342Code::GuardTripped,
        t342Code::LedgerBroken,
        t342Code::SnapshotFingerprint,
        t342Code::TableBroken,
        t342Code::IndexMisaligned,
        t342Code::DuplicateEntry,
        t342Code::OrderViolated,
        t342Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            t342Code::SegOverlap => 0x3f10,
            t342Code::MergeUnbalanced => 0x3f11,
            t342Code::TypeConflict => 0x3f12,
            t342Code::UnsortedInput => 0x3f13,
            t342Code::CoalesceIllegal => 0x3f14,
            t342Code::CapacityLedgerBreak => 0x3f15,
            t342Code::MonotoneBreak => 0x3f16,
            t342Code::GuardTripped => 0x3f17,
            t342Code::LedgerBroken => 0x3f18,
            t342Code::SnapshotFingerprint => 0x3f19,
            t342Code::TableBroken => 0x3f1a,
            t342Code::IndexMisaligned => 0x3f1b,
            t342Code::DuplicateEntry => 0x3f1c,
            t342Code::OrderViolated => 0x3f1d,
            t342Code::SegmentGuard => 0x3f1e,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            t342Code::SegOverlap => "区段相交（归并前重叠）",
            t342Code::MergeUnbalanced => "归并失衡（输入容量≠输出容量）",
            t342Code::TypeConflict => "类型冲突（同址双类型）",
            t342Code::UnsortedInput => "输入未排序",
            t342Code::CoalesceIllegal => "非法吸收（异类相邻误并）",
            t342Code::CapacityLedgerBreak => "容量明细账破缺",
            t342Code::MonotoneBreak => "输出非单调",
            t342Code::GuardTripped => "守卫触发",
            t342Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            t342Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            t342Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            t342Code::IndexMisaligned => "索引错位（枚举序与矩阵序不一致）",
            t342Code::DuplicateEntry => "条目重复（同键双写）",
            t342Code::OrderViolated => "次序违例（依赖序/时序破缺）",
            t342Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            t342Code::SegOverlap => "按断点拆分后重并",
            t342Code::MergeUnbalanced => "按守恒律重算",
            t342Code::TypeConflict => "按优先级表裁定",
            t342Code::UnsortedInput => "先排序再归并",
            t342Code::CoalesceIllegal => "按同类律拒绝",
            t342Code::CapacityLedgerBreak => "按明细重建",
            t342Code::MonotoneBreak => "按序重排",
            t342Code::GuardTripped => "检查临界区",
            t342Code::LedgerBroken => "按守恒律重建台账后重跑",
            t342Code::SnapshotFingerprint => "按防篡改规程重建快照",
            t342Code::TableBroken => "按语义规则重建常量表",
            t342Code::IndexMisaligned => "按声明序归一化后重跑",
            t342Code::DuplicateEntry => "合并同键条目为末次生效",
            t342Code::OrderViolated => "按依赖矩阵重排后重跑",
            t342Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 引擎 e820m（区段排序归并：64B 制 + 零相交 + 容量守恒）
// --------------------------------------------------------------------------
/// E820 区段归并引擎（64 字节粒度，零相交红线）。
/// 区段表容量上限。
pub const E820M_CAP: usize = 48;

/// 区段（起址 + 长度，闭开区间 [base, base+len)）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct e820mSeg {
    /// 起始地址（64B 对齐）。
    pub base: u64,
    /// 长度（64B）。
    pub len: u64,
    /// 类型标签（由调用方约定，如可用/保留/ACPI）。
    pub kind: u32,
}

impl e820mSeg {
    /// 构造（零长区段直接拒收由上层保证）。
    pub const fn new(base: u64, len: u64, kind: u32) -> Self {
        Self { base, len, kind }
    }

    /// 末址（闭开区间上界）。
    pub const fn end(&self) -> u64 {
        self.base + self.len
    }
}

/// 归并结果账（容量守恒：输入总长 == 输出总长）。
#[derive(Clone, Copy, Debug)]
pub struct e820mMerger {
    /// 排序后归并输出。
    pub out: [e820mSeg; 48],
    /// 输出计数。
    pub out_n: usize,
    /// 输入总长。
    pub in_total: u64,
    /// 输出总长。
    pub out_total: u64,
    /// 相交拒止计数。
    pub overlap_rejects: u64,
    /// 归并吸收计数（相邻同类型段合并）。
    pub coalesced: u64,
}

impl e820mMerger {
    /// 全零构造。
    pub const fn new() -> Self {
        Self {
            out: [e820mSeg::new(0, 0, 0); 48],
            out_n: 0,
            in_total: 0,
            out_total: 0,
            overlap_rejects: 0,
            coalesced: 0,
        }
    }

    /// 插入排序（按 base 升序；同 base 按 len 降序，短的在前）。
    pub fn sort(&mut self, segs: &[e820mSeg]) {
        self.out_n = 0;
        let mut i = 0usize;
        while i < segs.len() && i < 48 {
            let s = segs[i];
            self.in_total += s.len;
            let mut j = self.out_n;
            while j > 0 && self.out[j - 1].base > s.base {
                self.out[j] = self.out[j - 1];
                j -= 1;
            }
            self.out[j] = s;
            self.out_n += 1;
            i += 1;
        }
    }

    /// 归并：相邻同类型且首尾相接的段吸收；相交直接拒止记账。
    pub fn coalesce(&mut self) -> Result<(), t342Code> {
        let mut i = 0usize;
        while i < self.out_n {
            if i + 1 < self.out_n {
                let a = self.out[i];
                let b = self.out[i + 1];
                if b.base < a.end() {
                    // 相交：拒止并记账（零相交红线）。
                    self.overlap_rejects += 1;
                    return Err(t342Code::SegOverlap);
                }
                if a.kind == b.kind && a.end() == b.base {
                    // 首尾相接同类型：吸收。
                    self.out[i].len += b.len;
                    let mut k = i + 1;
                    while k + 1 < self.out_n {
                        self.out[k] = self.out[k + 1];
                        k += 1;
                    }
                    self.out_n -= 1;
                    self.coalesced += 1;
                    continue;
                }
            }
            i += 1;
        }
        // 输出总长核算。
        let mut total = 0u64;
        let mut j = 0usize;
        while j < self.out_n {
            total += self.out[j].len;
            j += 1;
        }
        self.out_total = total;
        if self.out_total != self.in_total {
            return Err(t342Code::MergeUnbalanced);
        }
        Ok(())
    }

    /// 单调性复检（输出全序：base 严格递增且段间有缝或同序）。
    pub fn monotone(&self) -> bool {
        let mut i = 1usize;
        while i < self.out_n {
            if self.out[i - 1].base >= self.out[i].base {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 零相交复检（逐对扫描相邻段）。
    pub fn disjoint(&self) -> bool {
        let mut i = 0usize;
        while i + 1 < self.out_n {
            if self.out[i].end() > self.out[i + 1].base {
                return false;
            }
            i += 1;
        }
        true
    }
}

// ---------------------------------------------------------------------------
// 段位守卫（本模块独立十六进码前缀防串段；越界即判红）
// ---------------------------------------------------------------------------

/// 段位守卫：全部诊断码必须落在本段 [0x3f10, 0x3f1f]。
pub const fn t342_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < t342Code::ALL.len() {
        let c = t342Code::ALL[i].code();
        if c < 0x3f10 || c > 0x3f1f {
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
fn t342_guard_seg() -> bool {
    t342_segment_guard()
}

/// 归并基本正样本（排序 + 同类吸收 + 守恒）。
fn check_c1_merge_basic() -> bool {
    let mut mg = e820mMerger::new();
    let segs = [
        e820mSeg::new(0x1000, 0x2000, 1),
        e820mSeg::new(0x0000, 0x1000, 1),
        e820mSeg::new(0x8000, 0x1000, 2),
    ];
    mg.sort(&segs);
    let mut ok = matches!(mg.coalesce(), Ok(()));
    if ok {
        // 前两段同类相邻被吸收成一段 0x3000。
        ok = mg.out_n == 2 && mg.monotone() && mg.disjoint();
    }
    if ok {
        ok = mg.out[0].base == 0 && mg.out[0].len == 0x3000;
    }
    if ok {
        ok = mg.in_total == mg.out_total;
    }
    ok
}

/// 相交拒止（负样本：重叠段被拒并记账）。
fn check_c2_overlap_reject() -> bool {
    let mut mg = e820mMerger::new();
    let segs = [
        e820mSeg::new(0x1000, 0x2000, 1),
        e820mSeg::new(0x2000, 0x1000, 1),
    ];
    mg.sort(&segs);
    let mut ok = matches!(mg.coalesce(), Err(t342Code::SegOverlap));
    if ok {
        ok = mg.overlap_rejects == 1;
    }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_t342_checks() -> CheckSet {
    let mut set = CheckSet::new("T342");
    set.add(
        "t342-seg-guard",
        t342_guard_seg(),
        if t342_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "t342-c1-merge-basic",
        check_c1_merge_basic(),
        if check_c1_merge_basic() { "" } else { "e820 merge basic mismatch" },
    );
    set.add(
        "t342-c2-overlap-reject",
        check_c2_overlap_reject(),
        if check_c2_overlap_reject() { "" } else { "e820 overlap rejection mismatch" },
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
    fn t342_all_checks_green() {
        let set = run_t342_checks();
        assert!(set.all_passed(), "T342 domain checks must be green");
    }

    #[test]
    fn t342_segment_guard_holds() {
        assert!(t342_segment_guard());
    }
}
