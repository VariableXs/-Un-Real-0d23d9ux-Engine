//! F176 内存守卫 · 批次四深化（secstar · G-G-06）。
//!
//! 批次四功能面（与批次三互补：批次三管「隔离与金丝雀」，本批管
//! 「巡检、模式与重放」）：
//! - [`GuardWalker`]：护栏页巡检器——一次巡 N 页、命中页号报告
//!   （PROT_NONE 巡检面的批量路径：逐页试触太慢，批量页表读一拍出账）；
//! - [`AllocPattern`]：分配模式统计——六级 slab 各档计数/峰值/当前
//!   （分配画像：异常暴涨的档位一眼可见）；
//! - [`FaultReplay`]：故障重放账——FaultClass × 应用位图矩阵
//!   （六类故障在哪些应用发生过——回归测试的目标清单来源）；
//! - [`ExemptAudit`]：豁免审计——豁免命中率/窗外照拦数（豁免白名单
//!   不是黑洞：命中了什么、拦了什么都有账）。
//!
//! 零堆纪律：定长计数器阵 + 位图，无 alloc。

use super::memguard::{FAULT_LOG_CAP, FaultClass};
use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 护栏页巡检器
// ---------------------------------------------------------------------------

/// 单次巡检页数上限。
pub const WALK_BATCH: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WalkReport {
    /// 巡检起始页号。
    pub start_page: u64,
    /// 实际巡的页数（≤ WALK_BATCH——边界诚实）。
    pub walked: usize,
    /// 命中的护栏页数（本批）。
    pub guard_hits: usize,
}

/// 页表读取注入口（真实页表由底层回填——本层只驱动巡检节奏）。
pub trait PageTableProbe {
    /// 该页是否为护栏页（PROT_NONE）。
    fn is_guard(&self, page: u64) -> bool;
}

/// 全部非护栏的替身（巡检零命中的空转路径）。
pub struct NoGuardProbe;
impl PageTableProbe for NoGuardProbe {
    fn is_guard(&self, _page: u64) -> bool {
        false
    }
}

/// 巡检一批：从 start_page 起最多 WALK_BATCH 页。
pub fn walk_guard_pages(probe: &dyn PageTableProbe, start_page: u64, max_pages: usize) -> WalkReport {
    let walked = max_pages.min(WALK_BATCH);
    let mut hits = 0;
    for i in 0..walked {
        if probe.is_guard(start_page + i as u64) {
            hits += 1;
        }
    }
    WalkReport { start_page, walked, guard_hits: hits }
}

// ---------------------------------------------------------------------------
// 分配模式统计
// ---------------------------------------------------------------------------

/// 六档计数器（与批次三 SIZE_CLASS_EDGES 同尺）。
pub const SIZE_CLASSES: usize = 6;

#[derive(Clone, Copy, Debug, Default)]
pub struct AllocPattern {
    counts: [u64; SIZE_CLASSES],
    peaks: [u64; SIZE_CLASSES],
    current: [u64; SIZE_CLASSES],
}

impl AllocPattern {
    pub const fn new() -> AllocPattern {
        AllocPattern { counts: [0; SIZE_CLASSES], peaks: [0; SIZE_CLASSES], current: [0; SIZE_CLASSES] }
    }

    pub fn on_alloc(&mut self, class: usize) {
        if class >= SIZE_CLASSES {
            return;
        }
        self.counts[class] += 1;
        self.current[class] += 1;
        if self.current[class] > self.peaks[class] {
            self.peaks[class] = self.current[class];
        }
    }

    pub fn on_free(&mut self, class: usize) {
        if class >= SIZE_CLASSES {
            return;
        }
        self.current[class] = self.current[class].saturating_sub(1);
    }

    pub fn count(&self, class: usize) -> u64 {
        self.counts.get(class).copied().unwrap_or(0)
    }

    pub fn peak(&self, class: usize) -> u64 {
        self.peaks.get(class).copied().unwrap_or(0)
    }

    /// 异常暴涨：某档当前值 > 峰值阈值 → 黄标（画像面）。
    pub fn spike(&self, class: usize, threshold: u64) -> bool {
        self.current.get(class).copied().unwrap_or(0) > threshold
    }
}

// ---------------------------------------------------------------------------
// 故障重放账（FaultClass × 应用位图）
// ---------------------------------------------------------------------------

/// 应用位宽（16 应用——重放目标清单的粒度）。
pub const FAULT_APP_BITS: usize = 16;

/// 六类 × 16 应用位图账。
#[derive(Clone, Copy)]
pub struct FaultReplay {
    /// bit i = 应用 i 出过该类故障。
    matrix: [u16; 6],
    total: [u32; 6],
}

impl FaultReplay {
    pub const fn new() -> FaultReplay {
        FaultReplay { matrix: [0; 6], total: [0; 6] }
    }

    fn class_idx(c: FaultClass) -> usize {
        match c {
            FaultClass::HeapOverflow => 0,
            FaultClass::UseAfterFree => 1,
            FaultClass::DoubleFree => 2,
            FaultClass::WildPointer => 3,
            FaultClass::StackOverflow => 4,
            FaultClass::UninitJump => 5,
        }
    }

    /// 记一次故障（app_bit 0-15）。
    pub fn record(&mut self, c: FaultClass, app_bit: usize) {
        if app_bit >= FAULT_APP_BITS {
            return;
        }
        let i = Self::class_idx(c);
        self.matrix[i] |= 1 << app_bit;
        self.total[i] += 1;
    }

    /// 哪些应用出过该类故障（位图）。
    pub fn apps_of(&self, c: FaultClass) -> u16 {
        self.matrix[Self::class_idx(c)]
    }

    /// 该类故障总数。
    pub fn total_of(&self, c: FaultClass) -> u32 {
        self.total[Self::class_idx(c)]
    }

    /// 六类全谱覆盖判定（主册六类样本全捕获的回归面：每类至少一例）。
    pub fn all_six_covered(&self) -> bool {
        self.total.iter().all(|t| *t > 0)
    }
}

// ---------------------------------------------------------------------------
// 豁免审计
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Default)]
pub struct ExemptAudit {
    /// 豁免命中数（窗内放行）。
    pub hits: u64,
    /// 窗外照拦数（豁免不是免死金牌）。
    pub blocked_outside_window: u64,
    /// 豁免请求总数。
    pub requests: u64,
}

impl ExemptAudit {
    pub fn on_exempt_hit(&mut self) {
        self.requests += 1;
        self.hits += 1;
    }

    pub fn on_exempt_miss(&mut self) {
        self.requests += 1;
    }

    pub fn on_blocked_outside(&mut self) {
        self.blocked_outside_window += 1;
    }

    /// 命中率 ‰（零请求 → 0 不编造）。
    pub fn hit_rate_permille(&self) -> u32 {
        if self.requests == 0 {
            return 0;
        }
        (self.hits * 1_000 / self.requests) as u32
    }
}

// ---------------------------------------------------------------------------
// 批次四自检
// ---------------------------------------------------------------------------

#[inline(never)]
pub fn run_memguard_b4_checks() -> CheckSet {
    let mut cs = CheckSet::new("F176-b4");

    // 1) 巡检零命中：无护栏页 → walked 满额 hits=0（空转路径诚实）。
    let r0 = walk_guard_pages(&NoGuardProbe, 100, 16);
    cs.add("walk_zero_hits", r0.walked == 16 && r0.guard_hits == 0, "");

    // 2) 巡检命中：替身探测页 103/104 是护栏 → 恰 2 命中（批量面）。
    struct TwoGuards;
    impl PageTableProbe for TwoGuards {
        fn is_guard(&self, page: u64) -> bool {
            page == 103 || page == 104
        }
    }
    let r2 = walk_guard_pages(&TwoGuards, 100, 16);
    cs.add("walk_two_hits", r2.guard_hits == 2 && r2.walked == 16, "");

    // 3) 巡检截断：max_pages=5 → walked=5（批量上限内诚实缩）。
    let r3 = walk_guard_pages(&NoGuardProbe, 0, 5);
    cs.add("walk_clamped", r3.walked == 5, "");

    // 4) 分配画像：档 2 计 3 次/峰值 3 → 释放 1 → 当前 2（三轴全对）。
    let mut p = AllocPattern::new();
    p.on_alloc(2);
    p.on_alloc(2);
    p.on_alloc(2);
    let peak_ok = p.peak(2) == 3;
    p.on_free(2);
    cs.add("alloc_pattern_axes", p.count(2) == 3 && peak_ok && p.spike(2, 1), "");

    // 5) 画像越界档位不炸：class=9 无事（分级面诚实忽略）。
    let mut p2 = AllocPattern::new();
    p2.on_alloc(9);
    p2.on_free(9);
    cs.add("alloc_class_bounded", p2.count(9) == 0 && p2.peak(9) == 0 && !p2.spike(9, 0), "");

    // 6) 故障重放矩阵：UAF 在 app3/app7 → 位图 0b10001000（目标清单）。
    let mut m = FaultReplay::new();
    m.record(FaultClass::UseAfterFree, 3);
    m.record(FaultClass::UseAfterFree, 7);
    cs.add(
        "replay_matrix_bitmap",
        m.apps_of(FaultClass::UseAfterFree) == 0b1000_1000 && m.total_of(FaultClass::UseAfterFree) == 2,
        "",
    );

    // 7) 重放全谱：六类各一例 → all_six_covered（主册六类的回归面）。
    let mut m2 = FaultReplay::new();
    m2.record(FaultClass::HeapOverflow, 0);
    m2.record(FaultClass::UseAfterFree, 1);
    m2.record(FaultClass::DoubleFree, 2);
    m2.record(FaultClass::WildPointer, 3);
    m2.record(FaultClass::StackOverflow, 4);
    m2.record(FaultClass::UninitJump, 5);
    cs.add("replay_all_six", m2.all_six_covered() && !FaultReplay::new().all_six_covered(), "");

    // 8) 重放越界 app bit 不炸：bit 16 忽略（粒度上限）。
    let mut m3 = FaultReplay::new();
    m3.record(FaultClass::DoubleFree, 16);
    cs.add("replay_bit_bounded", m3.total_of(FaultClass::DoubleFree) == 0, "");

    // 9) 豁免审计：8 请求 6 命中 → 750‰（命中率面）。
    let mut a = ExemptAudit::default();
    for _ in 0..6 {
        a.on_exempt_hit();
    }
    for _ in 0..2 {
        a.on_exempt_miss();
    }
    a.on_blocked_outside();
    cs.add(
        "exempt_audit_rate",
        a.hit_rate_permille() == 750 && a.hits == 6 && a.blocked_outside_window == 1,
        "",
    );

    // 10) 豁免零请求诚实：0 → 0‰（不编造）。
    cs.add("exempt_audit_zero", ExemptAudit::default().hit_rate_permille() == 0, "");

    // 11) 故障日志容量贯通：FAULT_LOG_CAP 64 一处一事实。
    cs.add("fault_log_cap", FAULT_LOG_CAP == 64, "");

    // 12) 巡检批量常量：单批 ≤16 页（巡检节奏面在册）。
    cs.add("walk_batch_const", WALK_BATCH == 16, "");

    cs
}

// ---------------------------------------------------------------------------
// 宿主单测（批次四）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests_b4 {
    use super::*;

    #[test]
    fn alloc_pattern_multi_class_isolated() {
        // 多档隔离：档 0 暴涨不影响档 5 的画像（互不串扰）。
        let mut p = AllocPattern::new();
        for _ in 0..10 {
            p.on_alloc(0);
        }
        p.on_alloc(5);
        assert!(p.spike(0, 5));
        assert!(!p.spike(5, 5));
        assert_eq!(p.peak(5), 1);
    }

    #[test]
    fn replay_matrix_dense() {
        // 单类打满 16 应用：位图全 1、总数 16（位宽饱和不炸）。
        let mut m = FaultReplay::new();
        for bit in 0..FAULT_APP_BITS {
            m.record(FaultClass::WildPointer, bit);
        }
        assert_eq!(m.apps_of(FaultClass::WildPointer), 0xFFFF);
        assert_eq!(m.total_of(FaultClass::WildPointer), 16);
    }

    #[test]
    fn walk_reports_page_identity() {
        // 巡检报告带起始页号（定位面：命中在哪个页段可追溯）。
        struct All;
        impl PageTableProbe for All {
            fn is_guard(&self, _p: u64) -> bool {
                true
            }
        }
        let r = walk_guard_pages(&All, 0x9_0000, 8);
        assert_eq!((r.start_page, r.walked, r.guard_hits), (0x9_0000, 8, 8));
    }
}
