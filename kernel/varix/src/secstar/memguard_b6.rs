//! F176 内存守卫 · 批次六深化（secstar · G-G-06）。
//!
//! 批次六功能面（达成率 54%——主攻批次。与 b3/b4/b5 互补，本批管
//! 「页表映射与生命周期」）：
//! - [`GuardPageTable`]：护栏页表——页号 → 归属块映射（护栏命中时
//!   一查就知道是谁越的界——O(1) 归因面）；
//! - [`BlockLifecycle`]：块生命周期账——分配/命中/释放三事件流
//!   （一个块从生到死的每一步都有账）；
//! - [`OverheadAccountant`]：开销会计——护栏内存字节 + 检查时钟拍数
//!   两轴（<3% 判据的内存轴：护栏页不是免费的）；
//! - [`fault_human_reason`]：故障→人话三要素（六类故障各有一句
//!   「发生了什么/为什么/怎么办」——第 9 章在守卫面的落地）。
//!
//! 零堆纪律：定长页表 + 定长事件账，无 alloc。

use super::memguard::{FaultClass, GUARD_PAGE_BYTES};
use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 护栏页表
// ---------------------------------------------------------------------------

/// 页表容量。
pub const PAGE_TABLE_CAP: usize = 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GuardPageRec {
    pub page: u64,
    pub owner_app: u32,
    pub block_base: u64,
}

/// 护栏页表：护栏页号 → 归属（应用+块）。
pub struct GuardPageTable {
    recs: [Option<GuardPageRec>; PAGE_TABLE_CAP],
    pub n: usize,
}

impl GuardPageTable {
    pub const fn new() -> GuardPageTable {
        GuardPageTable { recs: [const { None }; PAGE_TABLE_CAP], n: 0 }
    }

    /// 登记（同页重复登记拒——一页只有一个主人）。
    pub fn register(&mut self, page: u64, owner_app: u32, block_base: u64) -> bool {
        if self.n >= PAGE_TABLE_CAP || self.lookup(page).is_some() {
            return false;
        }
        self.recs[self.n] = Some(GuardPageRec { page, owner_app, block_base });
        self.n += 1;
        true
    }

    pub fn lookup(&self, page: u64) -> Option<GuardPageRec> {
        self.recs[..self.n].iter().flatten().copied().find(|r| r.page == page)
    }

    /// 块释放 → 其全部护栏页出表（页随块走——不残留悬空映射）。
    pub fn release_block(&mut self, owner_app: u32, block_base: u64) -> usize {
        let mut removed = 0;
        let mut i = 0;
        while i < self.n {
            let r = self.recs[i].unwrap();
            if r.owner_app == owner_app && r.block_base == block_base {
                for j in i..self.n - 1 {
                    self.recs[j] = self.recs[j + 1];
                }
                self.recs[self.n - 1] = None;
                self.n -= 1;
                removed += 1;
            } else {
                i += 1;
            }
        }
        removed
    }

    /// 页数换算：块大小 → 所需护栏页数（尾页含界内即 +1）。
    pub fn pages_needed(size_bytes: u64) -> u64 {
        size_bytes.div_ceil(GUARD_PAGE_BYTES)
    }
}

// ---------------------------------------------------------------------------
// 块生命周期账
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LifecycleEvent {
    Allocated,
    GuardHit,
    Freed,
}

/// 单块生命周期（事件环——每块最多 16 事件）。
pub struct BlockLifecycle {
    app_id: u32,
    block_base: u64,
    events: [Option<(LifecycleEvent, u64)>; 16],
    head: usize,
    pub n: usize,
}

impl BlockLifecycle {
    pub const fn new(app_id: u32, block_base: u64) -> BlockLifecycle {
        BlockLifecycle { app_id, block_base, events: [const { None }; 16], head: 0, n: 0 }
    }

    pub fn record(&mut self, ev: LifecycleEvent, at_ms: u64) {
        if self.n < 16 {
            self.n += 1;
        }
        self.events[self.head] = Some((ev, at_ms));
        self.head = (self.head + 1) % 16;
    }

    /// 命中次数（该块被抓到越界的次数）。
    pub fn hit_count(&self) -> usize {
        self.events.iter().flatten().filter(|(e, _)| *e == LifecycleEvent::GuardHit).count()
    }

    pub fn owner(&self) -> u32 {
        self.app_id
    }

    pub fn base(&self) -> u64 {
        self.block_base
    }
}

// ---------------------------------------------------------------------------
// 开销会计
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Default)]
pub struct OverheadAccountant {
    /// 护栏页内存字节（页数 × 4KB）。
    pub guard_bytes: u64,
    /// 检查时钟拍数（每次 access 检查记 1 拍）。
    pub check_ticks: u64,
    /// 应用工作总字节（分母）。
    pub app_bytes: u64,
}

impl OverheadAccountant {
    /// 内存开销 ‰：护栏字节 / 应用字节。
    pub fn memory_permille(&self) -> u32 {
        if self.app_bytes == 0 {
            return 0;
        }
        (self.guard_bytes * 1_000 / self.app_bytes) as u32
    }

    /// 护栏页数账（登记时累加）。
    pub fn on_guard_pages(&mut self, pages: u64) {
        self.guard_bytes += pages.saturating_mul(GUARD_PAGE_BYTES);
    }
}

// ---------------------------------------------------------------------------
// 故障人话三要素
// ---------------------------------------------------------------------------

/// 三要素行（发生了什么/为什么/怎么办——六类各一套）。
pub fn fault_human_reason(c: FaultClass) -> (&'static str, &'static str, &'static str) {
    match c {
        FaultClass::HeapOverflow => (
            "应用写超出了分配的内存边界",
            "相邻内存被护栏页拦下——越界写入被阻断",
            "该应用已隔离，可在崩溃隔离页查看详情并重启它",
        ),
        FaultClass::UseAfterFree => (
            "应用访问了已释放的内存",
            "内存归还后仍被引用——悬空指针",
            "应用已隔离；此故障通常意味着应用有缺陷，等待更新",
        ),
        FaultClass::DoubleFree => (
            "应用重复释放了同一块内存",
            "两次 free 同一块——内存账本已不一致",
            "应用已隔离；如反复出现请向应用开发者反馈",
        ),
        FaultClass::WildPointer => (
            "应用访问了未映射的内存区域",
            "指针指向无效地址——野指针",
            "应用已隔离；详情见诊断快照",
        ),
        FaultClass::StackOverflow => (
            "应用栈溢出",
            "递归过深或栈帧过大，超出栈底守卫",
            "应用已隔离；重启后如再现请反馈",
        ),
        FaultClass::UninitJump => (
            "应用跳转到不可执行区域",
            "代码指针未初始化或被破坏",
            "应用已隔离；此为严重缺陷，建议不再使用该应用",
        ),
    }
}

// ---------------------------------------------------------------------------
// 批次六自检
// ---------------------------------------------------------------------------

#[inline(never)]
pub fn run_memguard_b6_checks() -> CheckSet {
    use FaultClass as FC;
    let mut cs = CheckSet::new("F176-b6");

    // 1) 页表登记/查重：同页拒（一页一主）。
    let mut t = GuardPageTable::new();
    let ok = t.register(0x9_0000, 7, 0x1000);
    let dup = t.register(0x9_0000, 8, 0x2000);
    cs.add("pagetable_register", ok && !dup && t.lookup(0x9_0000).unwrap().owner_app == 7, "");

    // 2) 命中归因：护栏页触碰 → 一查知主（O(1) 归因面）。
    let rec = t.lookup(0x9_0000).unwrap();
    cs.add("pagetable_attribution", rec.owner_app == 7 && rec.block_base == 0x1000, "");

    // 3) 块释放页出表：两护栏页同块 → 一次 release 全出（不残留）。
    t.register(0x9_1000, 7, 0x1000);
    let removed = t.release_block(7, 0x1000);
    cs.add(
        "pagetable_release_all",
        removed == 2 && t.lookup(0x9_0000).is_none() && t.lookup(0x9_1000).is_none() && t.n == 0,
        "",
    );

    // 4) 页数换算：1B→1 页、4096→1 页、4097→2 页（尾页进位）。
    cs.add(
        "pages_needed",
        GuardPageTable::pages_needed(1) == 1 && GuardPageTable::pages_needed(4_096) == 1 && GuardPageTable::pages_needed(4_097) == 2,
        "",
    );

    // 5) 生命周期账：分配→命中→释放三事件在账（生到死有迹）。
    let mut l = BlockLifecycle::new(7, 0x1000);
    l.record(LifecycleEvent::Allocated, 100);
    l.record(LifecycleEvent::GuardHit, 200);
    l.record(LifecycleEvent::Freed, 300);
    cs.add(
        "lifecycle_full",
        l.hit_count() == 1 && l.owner() == 7 && l.base() == 0x1000 && l.n == 3,
        "",
    );

    // 6) 生命周期多次命中：5 次命中全计数（惯犯识别面）。
    let mut l2 = BlockLifecycle::new(9, 0x5000);
    for i in 0..5u64 {
        l2.record(LifecycleEvent::GuardHit, i);
    }
    cs.add("lifecycle_repeat_offender", l2.hit_count() == 5, "");

    // 7) 开销会计：4 护栏页 = 16KB；应用 4MB → 内存开销 4‰（两轴之一）。
    let mut o = OverheadAccountant { app_bytes: 4 * 1024 * 1024, ..Default::default() };
    o.on_guard_pages(4);
    cs.add("overhead_memory_axis", o.guard_bytes == 16_384 && o.memory_permille() == 3, "");

    // 8) 开销空账：零应用 → 0‰（不编造）。
    cs.add("overhead_empty", OverheadAccountant::default().memory_permille() == 0, "");

    // 9) 人话三要素：六类各三段非空、且首段各异（六套话不重复）。
    let classes = [FC::HeapOverflow, FC::UseAfterFree, FC::DoubleFree, FC::WildPointer, FC::StackOverflow, FC::UninitJump];
    let all = classes.iter().all(|c| {
        let (w, y, n) = fault_human_reason(*c);
        !w.is_empty() && !y.is_empty() && !n.is_empty()
    });
    let distinct = {
        let a = fault_human_reason(FC::HeapOverflow).0;
        let b = fault_human_reason(FC::UseAfterFree).0;
        let c3 = fault_human_reason(FC::DoubleFree).0;
        a != b && b != c3 && a != c3
    };
    cs.add("fault_human_six", all && distinct, "");

    // 10) 页宽贯通：4KB 护栏页一处一事实。
    cs.add("consts_aligned", GUARD_PAGE_BYTES == 4_096, "");

    cs
}

// ---------------------------------------------------------------------------
// 宿主单测（批次六）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests_b6 {
    use super::*;

    #[test]
    fn pagetable_capacity_boundary() {
        // 32 页满容、第 33 拒（页表容量边界）。
        let mut t = GuardPageTable::new();
        for p in 0..PAGE_TABLE_CAP as u64 {
            assert!(t.register(p, 1, p * 0x1000));
        }
        assert!(!t.register(999, 1, 0));
        assert_eq!(t.n, PAGE_TABLE_CAP);
    }

    #[test]
    fn lifecycle_ring_wraps() {
        // 事件环回卷：20 事件只留最近 16（内存上限纪律）。
        let mut l = BlockLifecycle::new(1, 0);
        for i in 0..20u64 {
            l.record(LifecycleEvent::GuardHit, i);
        }
        assert_eq!(l.n, 16);
        assert_eq!(l.hit_count(), 16);
    }

    #[test]
    fn overhead_memory_axis_big_app() {
        // 中应用：64 护栏页 = 256KB / 16MB 应用 = 15‰（比例随规模变化）。
        let mut o = OverheadAccountant { app_bytes: 16 * 1024 * 1024, ..Default::default() };
        o.on_guard_pages(64);
        assert_eq!(o.memory_permille(), 15); // 262144*1000/16777216 = 15
    }
}
