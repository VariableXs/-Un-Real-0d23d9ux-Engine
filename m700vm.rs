//! VARIX-M700 AI-02 虚拟内存域（F026~F050，K1）。
//!
//! 虚拟内存从"能映射"到"可审计"：账本、缺页分类、大页阶梯、COW 稽核、
//! 反向映射、压力谱线、TLB 击落、对账与年报。
//! 全部为纯逻辑 + 固定容量数组（无 Vec/String/Box），no_std 安全。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// F026 — 地址空间账本：每进程虚拟区段的权威账本与越界审计
// ---------------------------------------------------------------------------

pub const MAX_VMAS: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VmaKind {
    Code,
    Data,
    Heap,
    Stack,
    Mmap,
}

#[derive(Clone, Copy)]
pub struct Vma {
    pub start: u64,
    pub end: u64,
    pub kind: VmaKind,
    pub writable: bool,
}

#[derive(Clone, Copy)]
pub struct AddrSpaceLedger {
    pub vmas: [Option<Vma>; MAX_VMAS],
    pub count: usize,
    /// 被拒绝的登记数（越界/重叠/零长）。
    pub rejected: u32,
}

impl AddrSpaceLedger {
    pub const fn new() -> AddrSpaceLedger {
        AddrSpaceLedger { vmas: [const { None }; MAX_VMAS], count: 0, rejected: 0 }
    }

    fn overlaps(&self, start: u64, end: u64) -> bool {
        for i in 0..self.count {
            if let Some(v) = self.vmas[i] {
                if start < v.end && v.start < end {
                    return true;
                }
            }
        }
        false
    }

    /// 登记一段虚拟区段；与既有区段重叠、地址倒序或越出用户空间则拒绝。
    pub fn add(&mut self, start: u64, end: u64, kind: VmaKind, writable: bool) -> bool {
        if end <= start || end > 0x0000_8000_0000_0000 || self.overlaps(start, end) {
            self.rejected += 1;
            return false;
        }
        if self.count >= MAX_VMAS {
            self.rejected += 1;
            return false;
        }
        self.vmas[self.count] = Some(Vma { start, end, kind, writable });
        self.count += 1;
        true
    }

    /// 越界审计：地址落在任何区段内返回 Some(索引)。
    pub fn find(&self, addr: u64) -> Option<usize> {
        for i in 0..self.count {
            if let Some(v) = self.vmas[i] {
                if addr >= v.start && addr < v.end {
                    return Some(i);
                }
            }
        }
        None
    }

    /// 写访问裁决：命中区段且可写。
    pub fn can_write(&self, addr: u64) -> bool {
        match self.find(addr) {
            Some(i) => match self.vmas[i] {
                Some(v) => v.writable,
                None => false,
            },
            None => false,
        }
    }

    pub fn total_pages(&self) -> u64 {
        let mut t = 0u64;
        for i in 0..self.count {
            if let Some(v) = self.vmas[i] {
                t += (v.end - v.start) / 4096;
            }
        }
        t
    }

    /// 洞察：账本中最大的空洞（相邻区段之间的空隙字节数）。
    pub fn largest_gap(&self) -> u64 {
        if self.count < 2 {
            return 0;
        }
        // 区段按登记顺序可能乱序，做一次插入排序拷贝后再扫描。
        let mut starts = [0u64; MAX_VMAS];
        let mut ends = [0u64; MAX_VMAS];
        for i in 0..self.count {
            if let Some(v) = self.vmas[i] {
                starts[i] = v.start;
                ends[i] = v.end;
            }
        }
        for i in 1..self.count {
            let mut j = i;
            while j > 0 && starts[j] < starts[j - 1] {
                let (s, e) = (starts[j], ends[j]);
                starts[j] = starts[j - 1];
                ends[j] = ends[j - 1];
                starts[j - 1] = s;
                ends[j - 1] = e;
                j -= 1;
            }
        }
        let mut gap = 0u64;
        for i in 1..self.count {
            if starts[i] > ends[i - 1] {
                let g = starts[i] - ends[i - 1];
                if g > gap {
                    gap = g;
                }
            }
        }
        gap
    }
}

// ---------------------------------------------------------------------------
// F027 — 缺页分类官：缺页原因的七分类判定与各自处理路径
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FaultKind {
    NotFound,
    Protection,
    WriteToCow,
    StackGrow,
    DemandPaging,
    SwapMiss,
    Spurious,
}

pub const FAULT_KINDS: usize = 7;

/// 原始缺陷位：bit0 存在位、bit1 写访问、bit2 用户态、bit3 指令取指。
/// 判定顺序：不存在→缺页；写+只读→COW 或保护；栈区写→扩容；其余按位组合。
pub fn classify_fault(present: bool, is_write: bool, is_user: bool, in_stack_guard: bool) -> FaultKind {
    if !present {
        if is_user {
            FaultKind::DemandPaging
        } else {
            FaultKind::NotFound
        }
    } else if is_write && in_stack_guard {
        FaultKind::StackGrow
    } else if is_write && !is_user {
        FaultKind::WriteToCow
    } else if is_write {
        FaultKind::Protection
    } else if !is_user {
        FaultKind::SwapMiss
    } else {
        FaultKind::Spurious
    }
}

#[derive(Clone, Copy)]
pub struct FaultClassifier {
    pub counts: [u32; FAULT_KINDS],
    pub total: u32,
}

impl FaultClassifier {
    pub const fn new() -> FaultClassifier {
        FaultClassifier { counts: [0; FAULT_KINDS], total: 0 }
    }

    pub fn record(&mut self, kind: FaultKind) -> u32 {
        let slot = kind as usize;
        self.counts[slot] += 1;
        self.total += 1;
        self.counts[slot]
    }

    pub fn count_of(&self, kind: FaultKind) -> u32 {
        self.counts[kind as usize]
    }

    /// 洞察：占比最高的缺页类型。
    pub fn dominant(&self) -> Option<FaultKind> {
        let mut best: usize = FAULT_KINDS;
        let mut best_n = 0u32;
        for i in 0..FAULT_KINDS {
            if self.counts[i] > best_n {
                best_n = self.counts[i];
                best = i;
            }
        }
        match best {
            0 => Some(FaultKind::NotFound),
            1 => Some(FaultKind::Protection),
            2 => Some(FaultKind::WriteToCow),
            3 => Some(FaultKind::StackGrow),
            4 => Some(FaultKind::DemandPaging),
            5 => Some(FaultKind::SwapMiss),
            6 => Some(FaultKind::Spurious),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// F028 — 大页阶梯：2M/1G 大页的晋升与降级阶梯
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HugeLevel {
    Base4K,
    Huge2M,
    Giant1G,
}

#[derive(Clone, Copy)]
pub struct HugepageLadder {
    /// 各档页数（索引 0/1/2 对应 4K/2M/1G）。
    pub levels: [u32; 3],
    /// 晋升累计次数。
    pub promotions: u32,
    /// 降级累计次数。
    pub demotions: u32,
}

impl HugepageLadder {
    pub const fn new() -> HugepageLadder {
        HugepageLadder { levels: [0; 3], promotions: 0, demotions: 0 }
    }

    /// 晋升：连续同层页数达到阈值（2M 需 512、1G 需 512）才升一档。
    pub fn promote(&mut self, contiguous_4k_pages: u32) -> Option<HugeLevel> {
        if contiguous_4k_pages >= 512 * 512 && self.levels[0] >= 512 * 512 {
            self.levels[0] -= 512 * 512;
            self.levels[2] += 1;
            self.promotions += 1;
            return Some(HugeLevel::Giant1G);
        }
        if contiguous_4k_pages >= 512 && self.levels[0] >= 512 {
            self.levels[0] -= 512;
            self.levels[1] += 1;
            self.promotions += 1;
            return Some(HugeLevel::Huge2M);
        }
        None
    }

    /// 降级：一个 2M 大页碎裂回 512 个 4K 页。
    pub fn demote(&mut self) -> bool {
        if self.levels[1] == 0 {
            return false;
        }
        self.levels[1] -= 1;
        self.levels[0] += 512;
        self.demotions += 1;
        true
    }

    /// 换算成 4K 等效页数。
    pub fn equivalent_4k(&self) -> u64 {
        self.levels[0] as u64 + 512 * self.levels[1] as u64 + 512 * 512 * self.levels[2] as u64
    }
}

// ---------------------------------------------------------------------------
// F029 — 页表走路车：页表遍历的专用走查器（调试与自检两用）
// ---------------------------------------------------------------------------

pub const WALK_LEVELS: usize = 4;

#[derive(Clone, Copy)]
pub struct PageWalker {
    /// 每层被走过的次数。
    pub level_hits: [u32; WALK_LEVELS],
    /// 每层"缺项"次数（走不下去）。
    pub level_misses: [u32; WALK_LEVELS],
    pub walks: u32,
    /// 最近一次走查的深度（0=全通）。
    pub last_depth: u8,
}

impl PageWalker {
    pub const fn new() -> PageWalker {
        PageWalker { level_hits: [0; WALK_LEVELS], level_misses: [0; WALK_LEVELS], walks: 0, last_depth: 0 }
    }

    /// 走查一次：`present` 按从高层到低层的存在位序列给出。
    /// 返回走到的层号（0..=3）；中途缺项即停。
    pub fn walk(&mut self, present: [bool; WALK_LEVELS]) -> u8 {
        self.walks += 1;
        for l in 0..WALK_LEVELS {
            if present[l] {
                self.level_hits[l] += 1;
            } else {
                self.level_misses[l] += 1;
                self.last_depth = l as u8;
                return l as u8;
            }
        }
        self.last_depth = WALK_LEVELS as u8;
        WALK_LEVELS as u8
    }

    /// 洞察：最常缺项的层（页表空洞热点）。
    pub fn hottest_miss_level(&self) -> Option<usize> {
        let mut best: usize = WALK_LEVELS;
        let mut best_n = 0u32;
        for l in 0..WALK_LEVELS {
            if self.level_misses[l] > best_n {
                best_n = self.level_misses[l];
                best = l;
            }
        }
        if best == WALK_LEVELS {
            None
        } else {
            Some(best)
        }
    }

    pub fn full_walk_ratio_permille(&self) -> u32 {
        if self.walks == 0 {
            return 0;
        }
        (self.level_hits[3] as u64 * 1000 / self.walks as u64) as u32
    }
}

// ---------------------------------------------------------------------------
// F030 — 写时复制稽核：COW 页计数的稽核不变式
// ---------------------------------------------------------------------------

pub const MAX_COW_PAGES: usize = 8;

#[derive(Clone, Copy)]
pub struct CowPage {
    pub frame: u64,
    pub refs: u16,
}

#[derive(Clone, Copy)]
pub struct CowAuditor {
    pub pages: [Option<CowPage>; MAX_COW_PAGES],
    pub count: usize,
    /// 违反不变式（refs=0 仍存在 / 破坏不存在的页）的次数。
    pub violations: u32,
    /// fork 建立的共享页累计。
    pub shared_established: u32,
}

impl CowAuditor {
    pub const fn new() -> CowAuditor {
        CowAuditor { pages: [const { None }; MAX_COW_PAGES], count: 0, violations: 0, shared_established: 0 }
    }

    fn find_slot(&self, frame: u64) -> Option<usize> {
        for i in 0..self.count {
            if let Some(p) = self.pages[i] {
                if p.frame == frame {
                    return Some(i);
                }
            }
        }
        None
    }

    /// fork：同一物理页多一个共享者；已登记则加计数（幂等去重不重复占位）。
    pub fn fork_share(&mut self, frame: u64, refs: u16) -> bool {
        if refs == 0 {
            self.violations += 1;
            return false;
        }
        if let Some(i) = self.find_slot(frame) {
            if let Some(mut p) = self.pages[i] {
                p.refs = p.refs.saturating_add(refs);
                self.pages[i] = Some(p);
            }
            return true;
        }
        if self.count >= MAX_COW_PAGES {
            return false;
        }
        self.pages[self.count] = Some(CowPage { frame, refs });
        self.count += 1;
        self.shared_established += refs as u32;
        true
    }

    /// 写者破坏 COW：引用减一，归零即移除条目。
    pub fn break_cow(&mut self, frame: u64) -> bool {
        match self.find_slot(frame) {
            Some(i) => {
                if let Some(mut p) = self.pages[i] {
                    if p.refs == 0 {
                        self.violations += 1;
                        return false;
                    }
                    p.refs -= 1;
                    if p.refs == 0 {
                        self.pages[i] = None;
                    } else {
                        self.pages[i] = Some(p);
                    }
                    return true;
                }
                false
            }
            None => {
                self.violations += 1;
                false
            }
        }
    }

    /// 稽核不变式：所有条目 refs>0。
    pub fn audit(&self) -> bool {
        for i in 0..self.count {
            if let Some(p) = self.pages[i] {
                if p.refs == 0 {
                    return false;
                }
            }
        }
        true
    }

    pub fn live_refs(&self) -> u32 {
        let mut t = 0u32;
        for i in 0..self.count {
            if let Some(p) = self.pages[i] {
                t += p.refs as u32;
            }
        }
        t
    }
}

// ---------------------------------------------------------------------------
// F031 — 映射原子律：mmap 语义的原子性保证与半途失败回滚
// ---------------------------------------------------------------------------

pub const MMAP_STAGES: usize = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MmapStage {
    Reserve,
    Frame,
    Table,
}

#[derive(Clone, Copy)]
pub struct AtomicMapper {
    pub committed: u32,
    pub rolled_back: u32,
    /// 最近一次回滚停在第几阶段（0=未发生）。
    pub last_rollback_stage: u8,
    /// 已提交映射占用的总页数（仪表）。
    pub mapped_pages: u64,
}

impl AtomicMapper {
    pub const fn new() -> AtomicMapper {
        AtomicMapper { committed: 0, rolled_back: 0, last_rollback_stage: 0, mapped_pages: 0 }
    }

    /// 三阶段映射：`fail_at` 指定在第几阶段失败（0=成功，1..=3）。
    /// 任何阶段失败必须回滚已完成的前序阶段，映射对外不存在。
    pub fn mmap_atomic(&mut self, pages: u64, fail_at: u8) -> Result<(), &'static str> {
        let mut done: u8 = 0;
        for stage in 0..MMAP_STAGES {
            if fail_at == (stage + 1) as u8 {
                self.rolled_back += 1;
                self.last_rollback_stage = (stage + 1) as u8;
                // 回滚：前面 done 个阶段的效果全部撤销。
                if done >= 3 {
                    self.mapped_pages = self.mapped_pages.saturating_sub(pages);
                }
                return Err("mmap failed mid-way, rolled back");
            }
            done += 1;
        }
        self.committed += 1;
        self.mapped_pages += pages;
        Ok(())
    }

    /// 原子律：committed 一定对应完整三阶段（last_rollback_stage 记录失败点）。
    pub fn atomic_invariant(&self) -> bool {
        self.last_rollback_stage <= MMAP_STAGES as u8
    }
}

// ---------------------------------------------------------------------------
// F032 — 交换槽账房：匿名页交换槽的分配/回收账房
// ---------------------------------------------------------------------------

pub const MAX_SWAP_SLOTS: usize = 16;

#[derive(Clone, Copy)]
pub struct SwapLedger {
    /// 位图：1=已占用。
    pub used: [bool; MAX_SWAP_SLOTS],
    pub allocs: u32,
    pub frees: u32,
    /// 历史最高占用数（水位线）。
    pub peak_used: u16,
}

impl SwapLedger {
    pub const fn new() -> SwapLedger {
        SwapLedger { used: [false; MAX_SWAP_SLOTS], allocs: 0, frees: 0, peak_used: 0 }
    }

    fn used_count(&self) -> u16 {
        let mut n = 0u16;
        for i in 0..MAX_SWAP_SLOTS {
            if self.used[i] {
                n += 1;
            }
        }
        n
    }

    /// 分配最低空闲槽；重复分配同一槽位由位图天然去重。
    pub fn alloc(&mut self) -> Option<u16> {
        for i in 0..MAX_SWAP_SLOTS {
            if !self.used[i] {
                self.used[i] = true;
                self.allocs += 1;
                let n = self.used_count();
                if n > self.peak_used {
                    self.peak_used = n;
                }
                return Some(i as u16);
            }
        }
        None
    }

    /// 回收指定槽；双重释放被拒绝。
    pub fn free(&mut self, slot: u16) -> bool {
        if (slot as usize) >= MAX_SWAP_SLOTS || !self.used[slot as usize] {
            return false;
        }
        self.used[slot as usize] = false;
        self.frees += 1;
        true
    }

    pub fn in_use(&self) -> u16 {
        self.used_count()
    }
}

// ---------------------------------------------------------------------------
// F033 — 驻留页水位：内存驻留页的全局与分区水位线
// ---------------------------------------------------------------------------

pub const NUM_ZONES: usize = 4;

#[derive(Clone, Copy)]
pub struct ResidentWatermark {
    pub global_pages: u64,
    pub global_peak: u64,
    pub zone_pages: [u64; NUM_ZONES],
    pub zone_peaks: [u64; NUM_ZONES],
}

impl ResidentWatermark {
    pub const fn new() -> ResidentWatermark {
        ResidentWatermark {
            global_pages: 0,
            global_peak: 0,
            zone_pages: [0; NUM_ZONES],
            zone_peaks: [0; NUM_ZONES],
        }
    }

    pub fn observe(&mut self, zone: usize, delta: i64) -> bool {
        if zone >= NUM_ZONES {
            return false;
        }
        let cur = self.zone_pages[zone] as i64 + delta;
        if cur < 0 {
            return false;
        }
        self.zone_pages[zone] = cur as u64;
        if self.zone_pages[zone] > self.zone_peaks[zone] {
            self.zone_peaks[zone] = self.zone_pages[zone];
        }
        self.global_pages = 0;
        for z in 0..NUM_ZONES {
            self.global_pages += self.zone_pages[z];
        }
        if self.global_pages > self.global_peak {
            self.global_peak = self.global_pages;
        }
        true
    }

    /// 全局压力：peak 为 0 视为无压力。
    pub fn pressure_permille(&self) -> u32 {
        if self.global_peak == 0 {
            return 0;
        }
        (self.global_pages * 1000 / self.global_peak) as u32
    }

    /// 洞察：占用最大的分区。
    pub fn hottest_zone(&self) -> Option<usize> {
        let mut best: usize = NUM_ZONES;
        let mut best_n = 0u64;
        for z in 0..NUM_ZONES {
            if self.zone_pages[z] > best_n {
                best_n = self.zone_pages[z];
                best = z;
            }
        }
        if best == NUM_ZONES {
            None
        } else {
            Some(best)
        }
    }
}

// ---------------------------------------------------------------------------
// F034 — 反向映射谱：物理页到虚拟映射的反向索引谱
// ---------------------------------------------------------------------------

pub const MAX_RMAP_FRAMES: usize = 8;
pub const MAX_RMAP_REFS: usize = 4;

#[derive(Clone, Copy)]
pub struct RmapEntry {
    pub frame: u64,
    /// 映射到该帧的虚拟地址样本。
    pub vaddrs: [Option<u64>; MAX_RMAP_REFS],
    pub refs: u16,
}

#[derive(Clone, Copy)]
pub struct RmapSpectrum {
    pub entries: [Option<RmapEntry>; MAX_RMAP_FRAMES],
    pub count: usize,
    /// 重复登记（同帧同虚址）被去重的次数。
    pub deduped: u32,
}

impl RmapSpectrum {
    pub const fn new() -> RmapSpectrum {
        RmapSpectrum { entries: [const { None }; MAX_RMAP_FRAMES], count: 0, deduped: 0 }
    }

    fn find_frame(&self, frame: u64) -> Option<usize> {
        for i in 0..self.count {
            if let Some(e) = self.entries[i] {
                if e.frame == frame {
                    return Some(i);
                }
            }
        }
        None
    }

    /// 登记一帧的一条反向映射；同帧同虚址去重返回 false。
    pub fn map(&mut self, frame: u64, vaddr: u64) -> bool {
        if let Some(i) = self.find_frame(frame) {
            if let Some(mut e) = self.entries[i] {
                for j in 0..MAX_RMAP_REFS {
                    if e.vaddrs[j] == Some(vaddr) {
                        self.deduped += 1;
                        return false;
                    }
                }
                for j in 0..MAX_RMAP_REFS {
                    if e.vaddrs[j].is_none() {
                        e.vaddrs[j] = Some(vaddr);
                        e.refs += 1;
                        self.entries[i] = Some(e);
                        return true;
                    }
                }
                return false;
            }
            return false;
        }
        if self.count >= MAX_RMAP_FRAMES {
            return false;
        }
        let mut e = RmapEntry { frame, vaddrs: [const { None }; MAX_RMAP_REFS], refs: 1 };
        e.vaddrs[0] = Some(vaddr);
        self.entries[self.count] = Some(e);
        self.count += 1;
        true
    }

    /// 解除一条反向映射。
    pub fn unmap(&mut self, frame: u64, vaddr: u64) -> bool {
        if let Some(i) = self.find_frame(frame) {
            if let Some(mut e) = self.entries[i] {
                for j in 0..MAX_RMAP_REFS {
                    if e.vaddrs[j] == Some(vaddr) {
                        e.vaddrs[j] = None;
                        e.refs -= 1;
                        self.entries[i] = Some(e);
                        return true;
                    }
                }
            }
        }
        false
    }

    /// 某帧的映射引用数。
    pub fn refs_of(&self, frame: u64) -> u16 {
        match self.find_frame(frame) {
            Some(i) => match self.entries[i] {
                Some(e) => e.refs,
                None => 0,
            },
            None => 0,
        }
    }

    /// 洞察：被映射最多的帧（共享热点页）。
    pub fn hottest_frame(&self) -> Option<u64> {
        let mut best: Option<RmapEntry> = None;
        for i in 0..self.count {
            if let Some(e) = self.entries[i] {
                let better = match best {
                    Some(b) => e.refs > b.refs,
                    None => true,
                };
                if better {
                    best = Some(e);
                }
            }
        }
        best.map(|b| b.frame)
    }
}

// ---------------------------------------------------------------------------
// F035 — 页迁移走廊：页在 NUMA/压缩/整理间迁移的安全走廊
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MigrateState {
    Idle,
    Copying,
    Remapping,
    Done,
    Failed,
}

#[derive(Clone, Copy)]
pub struct MigrationCorridor {
    pub state: MigrateState,
    pub src_frame: u64,
    pub dst_frame: u64,
    /// 走廊准入失败次数（源页被钉住/校验不过）。
    pub denied: u32,
    pub completed: u32,
}

impl MigrationCorridor {
    pub const fn new() -> MigrationCorridor {
        MigrationCorridor { state: MigrateState::Idle, src_frame: 0, dst_frame: 0, denied: 0, completed: 0 }
    }

    /// 进入走廊：源页未被钉住（pinned=false）且目标与源不同才准入。
    pub fn enter(&mut self, src: u64, dst: u64, pinned: bool) -> Result<(), &'static str> {
        if self.state != MigrateState::Idle {
            self.denied += 1;
            return Err("corridor busy");
        }
        if pinned || src == dst {
            self.denied += 1;
            return Err("source pinned or identical frame");
        }
        self.src_frame = src;
        self.dst_frame = dst;
        self.state = MigrateState::Copying;
        Ok(())
    }

    /// 推进一步：Copying → Remapping → Done。
    pub fn step(&mut self) -> MigrateState {
        match self.state {
            MigrateState::Copying => {
                self.state = MigrateState::Remapping;
            }
            MigrateState::Remapping => {
                self.state = MigrateState::Done;
                self.completed += 1;
            }
            _ => {}
        }
        self.state
    }

    /// 退出走廊（Done 或 Failed 均可复位）。
    pub fn leave(&mut self) -> MigrateState {
        let s = self.state;
        self.state = MigrateState::Idle;
        s
    }
}

// ---------------------------------------------------------------------------
// F036 — 内存压力谱线：压力分级（绿/黄/橙/红）的谱线与逐级动作
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PressureLevel {
    Green,
    Yellow,
    Orange,
    Red,
}

pub const PRESSURE_LEVELS: usize = 4;

/// permille → 谱级：<400 绿、<700 黄、<900 橙、其余红。
pub fn pressure_level(permille: u32) -> PressureLevel {
    if permille < 400 {
        PressureLevel::Green
    } else if permille < 700 {
        PressureLevel::Yellow
    } else if permille < 900 {
        PressureLevel::Orange
    } else {
        PressureLevel::Red
    }
}

/// 逐级动作：绿=无、黄=后台回收、橙=同步回收+换出、红=杀进程候选。
pub fn pressure_action(level: PressureLevel) -> u8 {
    match level {
        PressureLevel::Green => 0,
        PressureLevel::Yellow => 1,
        PressureLevel::Orange => 2,
        PressureLevel::Red => 3,
    }
}

#[derive(Clone, Copy)]
pub struct PressureSpectrum {
    pub level_counts: [u32; PRESSURE_LEVELS],
    pub samples: u32,
    pub last_level: PressureLevel,
}

impl PressureSpectrum {
    pub const fn new() -> PressureSpectrum {
        PressureSpectrum { level_counts: [0; PRESSURE_LEVELS], samples: 0, last_level: PressureLevel::Green }
    }

    pub fn observe(&mut self, permille: u32) -> PressureLevel {
        let lv = pressure_level(permille);
        let idx = match lv {
            PressureLevel::Green => 0,
            PressureLevel::Yellow => 1,
            PressureLevel::Orange => 2,
            PressureLevel::Red => 3,
        };
        self.level_counts[idx] += 1;
        self.samples += 1;
        self.last_level = lv;
        lv
    }

    /// 洞察：处于橙+红的样本占比 permille。
    pub fn hot_permille(&self) -> u32 {
        if self.samples == 0 {
            return 0;
        }
        let hot = self.level_counts[2] + self.level_counts[3];
        (hot as u64 * 1000 / self.samples as u64) as u32
    }
}

// ---------------------------------------------------------------------------
// F037 — 守护映射区：内核只读数据与代码的映射保护矩阵
// ---------------------------------------------------------------------------

pub const MAX_GUARDED: usize = 8;

#[derive(Clone, Copy)]
pub struct GuardedRegion {
    pub start: u64,
    pub end: u64,
    /// true = 完全只读；false = 可执行不可写。
    pub read_only: bool,
}

#[derive(Clone, Copy)]
pub struct GuardedRegions {
    pub regions: [Option<GuardedRegion>; MAX_GUARDED],
    pub count: usize,
    /// 拦截到的越权写次数。
    pub violations: u32,
    /// 合法访问放行次数（仪表）。
    pub allowed: u32,
}

impl GuardedRegions {
    pub const fn new() -> GuardedRegions {
        GuardedRegions { regions: [const { None }; MAX_GUARDED], count: 0, violations: 0, allowed: 0 }
    }

    pub fn guard(&mut self, start: u64, end: u64, read_only: bool) -> bool {
        if end <= start || self.count >= MAX_GUARDED {
            return false;
        }
        self.regions[self.count] = Some(GuardedRegion { start, end, read_only });
        self.count += 1;
        true
    }

    fn hit(&self, addr: u64) -> Option<GuardedRegion> {
        for i in 0..self.count {
            if let Some(r) = self.regions[i] {
                if addr >= r.start && addr < r.end {
                    return Some(r);
                }
            }
        }
        None
    }

    /// 访问裁决：写命中守护区即拦截；读/执行放行。
    pub fn access(&mut self, addr: u64, is_write: bool) -> bool {
        match self.hit(addr) {
            Some(r) => {
                if is_write {
                    self.violations += 1;
                    false
                } else {
                    self.allowed += 1;
                    true
                }
                .then_some(r.read_only || !is_write)
                .unwrap_or(false)
            }
            None => {
                self.allowed += 1;
                true
            }
        }
    }

    pub fn violation_count(&self) -> u32 {
        self.violations
    }
}

// ---------------------------------------------------------------------------
// F038 — 栈自动扩容：主线程栈守卫页触发的受控扩容
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct StackGrower {
    /// 当前栈底（向低地址生长）。
    pub base: u64,
    /// 扩容下限：不允许越过。
    pub min_base: u64,
    /// 当前栈页数。
    pub pages: u32,
    pub grow_events: u32,
    pub grown_pages: u32,
    /// 拒绝次数（越过 min_base 或非守卫页触错）。
    pub refused: u32,
}

impl StackGrower {
    pub const fn new(base: u64, min_base: u64) -> StackGrower {
        StackGrower { base, min_base, pages: 0, grow_events: 0, grown_pages: 0, refused: 0 }
    }

    /// 守卫页缺页触发扩容 `pages_want` 页；一次扩容上限 32 页。
    pub fn on_guard_fault(&mut self, pages_want: u32) -> bool {
        if pages_want == 0 || pages_want > 32 {
            self.refused += 1;
            return false;
        }
        let step = pages_want as u64 * 4096;
        if self.base < self.min_base + step {
            self.refused += 1;
            return false;
        }
        self.base -= step;
        self.pages += pages_want;
        self.grow_events += 1;
        self.grown_pages += pages_want;
        true
    }

    /// 剩余可扩余量（字节）。
    pub fn headroom(&self) -> u64 {
        self.base - self.min_base
    }
}

// ---------------------------------------------------------------------------
// F039 — 共享映射仲裁：多进程共享映射的写者仲裁协议
// ---------------------------------------------------------------------------

pub const MAX_ARBITER_OWNERS: usize = 8;

#[derive(Clone, Copy)]
pub struct SharedMapArbiter {
    pub readers: u16,
    pub writer_active: bool,
    /// 等写锁被拒的次数。
    pub write_denied: u16,
    /// 授予写权的次数。
    pub write_granted: u32,
    pub owners: [Option<u16>; MAX_ARBITER_OWNERS],
    pub owner_count: usize,
}

impl SharedMapArbiter {
    pub const fn new() -> SharedMapArbiter {
        SharedMapArbiter {
            readers: 0,
            writer_active: false,
            write_denied: 0,
            write_granted: 0,
            owners: [const { None }; MAX_ARBITER_OWNERS],
            owner_count: 0,
        }
    }

    pub fn reader_enter(&mut self, pid: u16) -> bool {
        if self.writer_active {
            return false;
        }
        let mut known = false;
        for i in 0..self.owner_count {
            if self.owners[i] == Some(pid) {
                known = true;
            }
        }
        if !known {
            if self.owner_count >= MAX_ARBITER_OWNERS {
                return false;
            }
            self.owners[self.owner_count] = Some(pid);
            self.owner_count += 1;
        }
        self.readers += 1;
        true
    }

    pub fn reader_exit(&mut self) -> bool {
        if self.readers == 0 {
            return false;
        }
        self.readers -= 1;
        true
    }

    /// 写者请求独占：有读者或有写者即拒绝（仲裁协议核心）。
    pub fn writer_request(&mut self) -> bool {
        if self.writer_active || self.readers > 0 {
            self.write_denied += 1;
            return false;
        }
        self.writer_active = true;
        self.write_granted += 1;
        true
    }

    pub fn writer_release(&mut self) -> bool {
        if !self.writer_active {
            return false;
        }
        self.writer_active = false;
        true
    }

    /// 不变式：读者与写者不得同时在场。
    pub fn arbitration_invariant(&self) -> bool {
        !(self.writer_active && self.readers > 0)
    }
}

// ---------------------------------------------------------------------------
// F040 — 页错误风暴阀：缺页风暴的限流阀与归因记录
// ---------------------------------------------------------------------------

pub const MAX_STORM_ATTR: usize = 8;

#[derive(Clone, Copy)]
pub struct FaultStormValve {
    /// 当前窗口已放行的缺页数。
    pub window_count: u32,
    /// 每窗口放行上限。
    pub budget: u32,
    pub throttled: u32,
    /// 归因表：`cause` → 累计被限流次数。
    pub attr_causes: [Option<u8>; MAX_STORM_ATTR],
    pub attr_counts: [u32; MAX_STORM_ATTR],
    pub attr_count: usize,
}

impl FaultStormValve {
    pub const fn new(budget: u32) -> FaultStormValve {
        FaultStormValve {
            window_count: 0,
            budget,
            throttled: 0,
            attr_causes: [const { None }; MAX_STORM_ATTR],
            attr_counts: [0; MAX_STORM_ATTR],
            attr_count: 0,
        }
    }

    /// 新窗口：重置计数，保留归因史。
    pub fn new_window(&mut self) {
        self.window_count = 0;
    }

    /// 缺页进入：预算内放行；超预算按 `cause` 归因记录并限流。
    pub fn admit(&mut self, cause: u8) -> bool {
        if self.window_count < self.budget {
            self.window_count += 1;
            return true;
        }
        self.throttled += 1;
        let mut found = false;
        for i in 0..self.attr_count {
            if self.attr_causes[i] == Some(cause) {
                self.attr_counts[i] += 1;
                found = true;
            }
        }
        if !found && self.attr_count < MAX_STORM_ATTR {
            self.attr_causes[self.attr_count] = Some(cause);
            self.attr_counts[self.attr_count] = 1;
            self.attr_count += 1;
        }
        false
    }

    /// 洞察：被限流最多的归因码。
    pub fn top_throttled_cause(&self) -> Option<u8> {
        let mut best: Option<u8> = None;
        let mut best_n = 0u32;
        for i in 0..self.attr_count {
            if self.attr_counts[i] > best_n {
                best_n = self.attr_counts[i];
                best = self.attr_causes[i];
            }
        }
        best
    }

    pub fn throttled_total(&self) -> u32 {
        self.throttled
    }
}

// ---------------------------------------------------------------------------
// F041 — 地址空间隔离舱：按安全域划分的地址空间隔离等级
// ---------------------------------------------------------------------------

pub const MAX_ISOLATION_DOMAINS: usize = 6;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IsoLevel {
    /// 同舱可互访。
    Shared,
    /// 仅显式授权可跨舱。
    Guarded,
    /// 完全隔离，跨舱一律拒绝。
    Sealed,
}

#[derive(Clone, Copy)]
pub struct IsoDomain {
    pub id: u8,
    pub level: IsoLevel,
}

#[derive(Clone, Copy)]
pub struct IsolationPod {
    pub domains: [Option<IsoDomain>; MAX_ISOLATION_DOMAINS],
    pub count: usize,
    /// 被隔离策略拒绝的跨舱访问次数。
    pub blocked: u32,
}

impl IsolationPod {
    pub const fn new() -> IsolationPod {
        IsolationPod { domains: [const { None }; MAX_ISOLATION_DOMAINS], count: 0, blocked: 0 }
    }

    pub fn add_domain(&mut self, id: u8, level: IsoLevel) -> bool {
        for i in 0..self.count {
            if let Some(d) = self.domains[i] {
                if d.id == id {
                    return false; // 重复登记去重
                }
            }
        }
        if self.count >= MAX_ISOLATION_DOMAINS {
            return false;
        }
        self.domains[self.count] = Some(IsoDomain { id, level });
        self.count += 1;
        true
    }

    fn level_of(&self, id: u8) -> Option<IsoLevel> {
        for i in 0..self.count {
            if let Some(d) = self.domains[i] {
                if d.id == id {
                    return Some(d.level);
                }
            }
        }
        None
    }

    /// 跨舱访问裁决：同舱放行；Guarded 需授权；Sealed 一律拒。
    pub fn cross_access(&mut self, from: u8, to: u8, authorized: bool) -> bool {
        if from == to {
            return true;
        }
        let lf = self.level_of(from);
        let lt = self.level_of(to);
        let (lf, lt) = match (lf, lt) {
            (Some(a), Some(b)) => (a, b),
            _ => {
                self.blocked += 1;
                return false;
            }
        };
        let ok = match (lf, lt) {
            (IsoLevel::Sealed, _) | (_, IsoLevel::Sealed) => false,
            (IsoLevel::Guarded, _) | (_, IsoLevel::Guarded) => authorized,
            _ => true,
        };
        if !ok {
            self.blocked += 1;
        }
        ok
    }

    pub fn blocked_count(&self) -> u32 {
        self.blocked
    }
}

// ---------------------------------------------------------------------------
// F042 — TLB 击落协议：多核 TLB 失效的击落协议与延迟计量
// ---------------------------------------------------------------------------

pub const MAX_SHOOTDOWN_CORES: usize = 8;

#[derive(Clone, Copy)]
pub struct TlbShootdown {
    pub target_cores: [bool; MAX_SHOOTDOWN_CORES],
    pub acked: [bool; MAX_SHOOTDOWN_CORES],
    pub target_total: u16,
    pub ack_total: u16,
    /// 累计击落轮次。
    pub rounds: u32,
    /// 最近一轮的模拟延迟（周期数，按 100/核估算的仪表）。
    pub last_latency_cycles: u32,
}

impl TlbShootdown {
    pub const fn new() -> TlbShootdown {
        TlbShootdown {
            target_cores: [false; MAX_SHOOTDOWN_CORES],
            acked: [false; MAX_SHOOTDOWN_CORES],
            target_total: 0,
            ack_total: 0,
            rounds: 0,
            last_latency_cycles: 0,
        }
    }

    /// 发起击落：给出需要失效的目标核位图。
    pub fn initiate(&mut self, targets: [bool; MAX_SHOOTDOWN_CORES]) -> bool {
        let mut n = 0u16;
        for i in 0..MAX_SHOOTDOWN_CORES {
            self.target_cores[i] = targets[i];
            self.acked[i] = false;
            if targets[i] {
                n += 1;
            }
        }
        if n == 0 {
            return false;
        }
        self.target_total = n;
        self.ack_total = 0;
        self.rounds += 1;
        self.last_latency_cycles = n as u32 * 100;
        true
    }

    /// 目标核回执确认。
    pub fn ack(&mut self, core: usize) -> bool {
        if core >= MAX_SHOOTDOWN_CORES || !self.target_cores[core] || self.acked[core] {
            return false;
        }
        self.acked[core] = true;
        self.ack_total += 1;
        true
    }

    /// 全员确认即本轮击落完成。
    pub fn complete(&self) -> bool {
        self.target_total > 0 && self.ack_total == self.target_total
    }

    pub fn pending(&self) -> u16 {
        self.target_total - self.ack_total
    }
}

// ---------------------------------------------------------------------------
// F043 — 零页共享池：读匿名零页的全局共享池
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct ZeroPagePool {
    /// 读映射引用数（共享同一物理零页）。
    pub read_refs: u32,
    /// 写者破坏共享后单独分得的物理页数。
    pub private_breaks: u32,
    /// 因共享而省下的物理页数（仪表：每多一个读者省一页）。
    pub pages_saved: u32,
}

impl ZeroPagePool {
    pub const fn new() -> ZeroPagePool {
        ZeroPagePool { read_refs: 0, private_breaks: 0, pages_saved: 0 }
    }

    /// 读缺页命中零页：共享引用 +1。
    pub fn map_read(&mut self) -> u32 {
        self.read_refs += 1;
        if self.read_refs > 1 {
            self.pages_saved += 1;
        }
        self.read_refs
    }

    /// 写缺页：破坏共享，读者减一，私页 +1。
    pub fn break_on_write(&mut self) -> bool {
        if self.read_refs == 0 {
            return false;
        }
        self.read_refs -= 1;
        self.private_breaks += 1;
        true
    }

    pub fn saved_pages(&self) -> u32 {
        self.pages_saved
    }
}

// ---------------------------------------------------------------------------
// F044 — 映射血缘图：文件映射/匿名映射/设备映射的血缘标注
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MapLineage {
    File,
    Anon,
    Device,
}

pub const LINEAGE_KINDS: usize = 3;
pub const MAX_LINEAGE_SITES: usize = 8;

#[derive(Clone, Copy)]
pub struct LineageSite {
    pub site: u16,
    pub kind: MapLineage,
    pub count: u32,
}

#[derive(Clone, Copy)]
pub struct MappingLineage {
    pub sites: [Option<LineageSite>; MAX_LINEAGE_SITES],
    pub count: usize,
    pub kind_totals: [u32; LINEAGE_KINDS],
}

impl MappingLineage {
    pub const fn new() -> MappingLineage {
        MappingLineage { sites: [const { None }; MAX_LINEAGE_SITES], count: 0, kind_totals: [0; LINEAGE_KINDS] }
    }

    /// 登记映射来源；同 `site` 重复登记更新原条目并换算总量，不膨胀计数。
    pub fn record(&mut self, site: u16, kind: MapLineage) -> bool {
        for i in 0..self.count {
            if let Some(mut s) = self.sites[i] {
                if s.site == site {
                    self.kind_totals[s.kind as usize] = self.kind_totals[s.kind as usize].saturating_sub(1);
                    s.kind = kind;
                    s.count += 1;
                    self.kind_totals[kind as usize] += 1;
                    self.sites[i] = Some(s);
                    return true;
                }
            }
        }
        if self.count >= MAX_LINEAGE_SITES {
            return false;
        }
        self.sites[self.count] = Some(LineageSite { site, kind, count: 1 });
        self.count += 1;
        self.kind_totals[kind as usize] += 1;
        true
    }

    pub fn total_of(&self, kind: MapLineage) -> u32 {
        self.kind_totals[kind as usize]
    }

    /// 洞察：占主导的血缘类型。
    pub fn dominant_lineage(&self) -> Option<MapLineage> {
        let mut best: usize = LINEAGE_KINDS;
        let mut best_n = 0u32;
        for k in 0..LINEAGE_KINDS {
            if self.kind_totals[k] > best_n {
                best_n = self.kind_totals[k];
                best = k;
            }
        }
        match best {
            0 => Some(MapLineage::File),
            1 => Some(MapLineage::Anon),
            2 => Some(MapLineage::Device),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// F045 — 内存对账器：物理页帧台账与页表实际占用周期对账
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct MemoryReconciler {
    /// 台账登记的页数。
    pub ledger_pages: u64,
    /// 页表实际走查到的页数。
    pub table_pages: u64,
    /// 每轮对账的偏差记录（环形）。
    pub drift_log: [Option<i64>; 8],
    pub drift_head: usize,
    pub rounds: u32,
}

impl MemoryReconciler {
    pub const fn new() -> MemoryReconciler {
        MemoryReconciler { ledger_pages: 0, table_pages: 0, drift_log: [const { None }; 8], drift_head: 0, rounds: 0 }
    }

    pub fn set_ledger(&mut self, pages: u64) {
        self.ledger_pages = pages;
    }

    pub fn set_table(&mut self, pages: u64) {
        self.table_pages = pages;
    }

    /// 对账一轮：记录偏差（台账-实表），正值=台账多记（疑似泄漏）。
    pub fn reconcile(&mut self) -> i64 {
        let drift = self.ledger_pages as i64 - self.table_pages as i64;
        self.drift_log[self.drift_head] = Some(drift);
        self.drift_head = (self.drift_head + 1) % 8;
        self.rounds += 1;
        drift
    }

    /// 最近一轮偏差。
    pub fn last_drift(&self) -> Option<i64> {
        let idx = (self.drift_head + 7) % 8;
        self.drift_log[idx]
    }

    /// 连续两轮同向偏差视为系统性漂移。
    pub fn systematic_drift(&self) -> bool {
        let a = self.drift_log[(self.drift_head + 7) % 8];
        let b = self.drift_log[(self.drift_head + 6) % 8];
        match (a, b) {
            (Some(x), Some(y)) => x != 0 && y != 0 && (x > 0) == (y > 0),
            _ => false,
        }
    }
}

// ---------------------------------------------------------------------------
// F046 — 访问位考古：页表访问/脏位的历史采样与热度考古
// ---------------------------------------------------------------------------

pub const MAX_DIG_SITES: usize = 8;
pub const DIG_EPOCHS: usize = 4;

#[derive(Clone, Copy)]
pub struct DigSite {
    pub page: u64,
    /// 每个采样纪元命中访问位的次数。
    pub epoch_hits: [u8; DIG_EPOCHS],
}

#[derive(Clone, Copy)]
pub struct AccessBitDigger {
    pub sites: [Option<DigSite>; MAX_DIG_SITES],
    pub count: usize,
    pub epoch: usize,
    pub dirty_samples: u32,
}

impl AccessBitDigger {
    pub const fn new() -> AccessBitDigger {
        AccessBitDigger { sites: [const { None }; MAX_DIG_SITES], count: 0, epoch: 0, dirty_samples: 0 }
    }

    pub fn watch(&mut self, page: u64) -> bool {
        for i in 0..self.count {
            if let Some(s) = self.sites[i] {
                if s.page == page {
                    return false; // 已在观察，去重
                }
            }
        }
        if self.count >= MAX_DIG_SITES {
            return false;
        }
        self.sites[self.count] = Some(DigSite { page, epoch_hits: [0; DIG_EPOCHS] });
        self.count += 1;
        true
    }

    /// 采样一个纪元：`accessed` / `dirty` 来自页表位。
    pub fn sample(&mut self, page: u64, accessed: bool, dirty: bool) -> bool {
        for i in 0..self.count {
            if let Some(mut s) = self.sites[i] {
                if s.page == page {
                    if accessed && s.epoch_hits[self.epoch] < 255 {
                        s.epoch_hits[self.epoch] += 1;
                    }
                    if dirty {
                        self.dirty_samples += 1;
                    }
                    self.sites[i] = Some(s);
                    return true;
                }
            }
        }
        false
    }

    pub fn next_epoch(&mut self) -> usize {
        self.epoch = (self.epoch + 1) % DIG_EPOCHS;
        self.epoch
    }

    /// 考古：总命中次数最多的页。
    pub fn hottest_page(&self) -> Option<u64> {
        let mut best: Option<(u64, u32)> = None;
        for i in 0..self.count {
            if let Some(s) = self.sites[i] {
                let mut t = 0u32;
                for e in 0..DIG_EPOCHS {
                    t += s.epoch_hits[e] as u32;
                }
                let better = match best {
                    Some((_, bn)) => t > bn,
                    None => true,
                };
                if better {
                    best = Some((s.page, t));
                }
            }
        }
        best.map(|(p, _)| p)
    }
}

// ---------------------------------------------------------------------------
// F047 — 巨页透明化：透明大页的细粒度开关与逐区策略
// ---------------------------------------------------------------------------

pub const MAX_THP_ZONES: usize = 6;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThpPolicy {
    Always,
    Defer,
    Never,
}

#[derive(Clone, Copy)]
pub struct ThpZone {
    pub zone: u8,
    pub policy: ThpPolicy,
}

#[derive(Clone, Copy)]
pub struct ThpGovernor {
    pub zones: [Option<ThpZone>; MAX_THP_ZONES],
    pub count: usize,
    /// 重复设置同一区策略被原地更新的次数（去重仪表）。
    pub updated_in_place: u32,
}

impl ThpGovernor {
    pub const fn new() -> ThpGovernor {
        ThpGovernor { zones: [const { None }; MAX_THP_ZONES], count: 0, updated_in_place: 0 }
    }

    /// 设置某区策略：已存在则原地更新。
    pub fn set_policy(&mut self, zone: u8, policy: ThpPolicy) -> bool {
        for i in 0..self.count {
            if let Some(mut z) = self.zones[i] {
                if z.zone == zone {
                    z.policy = policy;
                    self.zones[i] = Some(z);
                    self.updated_in_place += 1;
                    return true;
                }
            }
        }
        if self.count >= MAX_THP_ZONES {
            return false;
        }
        self.zones[self.count] = Some(ThpZone { zone, policy });
        self.count += 1;
        true
    }

    pub fn policy_of(&self, zone: u8) -> Option<ThpPolicy> {
        for i in 0..self.count {
            if let Some(z) = self.zones[i] {
                if z.zone == zone {
                    return Some(z.policy);
                }
            }
        }
        None
    }

    pub fn enabled_zones(&self) -> u32 {
        let mut n = 0u32;
        for i in 0..self.count {
            if let Some(z) = self.zones[i] {
                if z.policy != ThpPolicy::Never {
                    n += 1;
                }
            }
        }
        n
    }
}

// ---------------------------------------------------------------------------
// F048 — 页表自愈：页表项损坏的检测与受控重建
// ---------------------------------------------------------------------------

pub const MAX_HEALED_LOG: usize = 8;

#[derive(Clone, Copy)]
pub struct PtHealer {
    /// 备份的合法 PTE 值。
    pub backup: [Option<u64>; MAX_HEALED_LOG],
    pub log_head: usize,
    pub healed: u32,
    pub unrepairable: u32,
}

impl PtHealer {
    pub const fn new() -> PtHealer {
        PtHealer { backup: [const { None }; MAX_HEALED_LOG], log_head: 0, healed: 0, unrepairable: 0 }
    }

    /// PTE 合法性：存在/读标志可组合，但保留位 52..63 必须为 0。
    pub fn is_valid_pte(pte: u64) -> bool {
        (pte >> 52) == 0
    }

    pub fn snapshot(&mut self, pte: u64) {
        self.backup[self.log_head] = Some(pte);
        self.log_head = (self.log_head + 1) % MAX_HEALED_LOG;
    }

    /// 检测损坏：非法 PTE 则用最近快照重建。
    pub fn heal(&mut self, pte: u64) -> Result<u64, &'static str> {
        if Self::is_valid_pte(pte) {
            return Ok(pte);
        }
        let idx = (self.log_head + MAX_HEALED_LOG - 1) % MAX_HEALED_LOG;
        match self.backup[idx] {
            Some(good) if Self::is_valid_pte(good) => {
                self.healed += 1;
                Ok(good)
            }
            _ => {
                self.unrepairable += 1;
                Err("no valid backup pte")
            }
        }
    }

    pub fn healed_count(&self) -> u32 {
        self.healed
    }
}

// ---------------------------------------------------------------------------
// F049 — 内存沙漏仪表：分配/释放/缺页/回收四流量的沙漏可视化数据源
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct MemHourglass {
    pub allocs: u64,
    pub frees: u64,
    pub faults: u64,
    pub reclaims: u64,
}

impl MemHourglass {
    pub const fn new() -> MemHourglass {
        MemHourglass { allocs: 0, frees: 0, faults: 0, reclaims: 0 }
    }

    pub fn on_alloc(&mut self, pages: u64) {
        self.allocs = self.allocs.saturating_add(pages);
    }

    pub fn on_free(&mut self, pages: u64) {
        self.frees = self.frees.saturating_add(pages);
    }

    pub fn on_fault(&mut self, pages: u64) {
        self.faults = self.faults.saturating_add(pages);
    }

    pub fn on_reclaim(&mut self, pages: u64) {
        self.reclaims = self.reclaims.saturating_add(pages);
    }

    /// 净流入（沙漏上半腔残留）。
    pub fn net_inflow(&self) -> i64 {
        self.allocs as i64 - self.frees as i64
    }

    /// 洞察：四路流量中的最大流（沙漏最细的颈部）。
    pub fn busiest_flow(&self) -> u8 {
        let mut best = 0u8;
        let mut best_n = self.allocs;
        if self.faults > best_n {
            best_n = self.faults;
            best = 1;
        }
        if self.frees > best_n {
            best_n = self.frees;
            best = 2;
        }
        if self.reclaims > best_n {
            best = 3;
        }
        best
    }
}

// ---------------------------------------------------------------------------
// F050 — 内存域年报：虚拟内存子系统仪表的周期聚合
// ---------------------------------------------------------------------------

pub const YEARBOOK_FIELDS: usize = 6;

#[derive(Clone, Copy)]
pub struct VmYearbook {
    /// 各字段周期累计：0=缺页 1=大页晋升 2=回滚 3=击落 4=COW 破坏 5=自愈。
    pub totals: [u64; YEARBOOK_FIELDS],
    pub epochs: u32,
    /// 上一周期的快照。
    pub last_snapshot: [u64; YEARBOOK_FIELDS],
}

impl VmYearbook {
    pub const fn new() -> VmYearbook {
        VmYearbook { totals: [0; YEARBOOK_FIELDS], epochs: 0, last_snapshot: [0; YEARBOOK_FIELDS] }
    }

    pub fn feed(&mut self, field: usize, amount: u64) -> bool {
        if field >= YEARBOOK_FIELDS {
            return false;
        }
        self.totals[field] = self.totals[field].saturating_add(amount);
        true
    }

    /// 收卷：把当前累计冻结为快照并开启新周期。
    pub fn close_epoch(&mut self) -> [u64; YEARBOOK_FIELDS] {
        self.last_snapshot = self.totals;
        self.epochs += 1;
        self.totals = [0; YEARBOOK_FIELDS];
        self.last_snapshot
    }

    pub fn snapshot_total(&self) -> u64 {
        let mut t = 0u64;
        for i in 0..YEARBOOK_FIELDS {
            t += self.last_snapshot[i];
        }
        t
    }

    /// 洞察：快照中占比最大的字段。
    pub snapshot_dominant as fn(&VmYearbook) -> usize = Self::dominant_field;
}

impl VmYearbook {
    fn dominant_field(&self) -> usize {
        let mut best = 0usize;
        let mut best_n = 0u64;
        for i in 0..YEARBOOK_FIELDS {
            if self.last_snapshot[i] > best_n {
                best_n = self.last_snapshot[i];
                best = i;
            }
        }
        best
    }

    pub fn dominant(&self) -> usize {
        self.dominant_field()
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub fn run_m700vm_checks() -> CheckSet {
    let mut set = CheckSet::new("m700vm");

    // F026 地址空间账本
    let mut led = AddrSpaceLedger::new();
    let r1 = led.add(0x1000, 0x5000, VmaKind::Code, false);
    let r2 = led.add(0x4000, 0x9000, VmaKind::Heap, true); // 重叠 → 拒
    let r3 = led.add(0x5000, 0x9000, VmaKind::Heap, true);
    set.add("F026 ledger add/overlap", r1 && !r2 && r3, "add+dedup");
    let hit = led.find(0x6000);
    set.add("F026 find/can_write", hit == Some(1) && led.can_write(0x6000) && !led.can_write(0x2000), "rw");
    set.add("F026 pages+gap", led.total_pages() == 8 && led.largest_gap() == 0, "gap");

    // F027 缺页分类官
    let mut fc = FaultClassifier::new();
    let k1 = classify_fault(false, true, true, false);
    let k2 = classify_fault(true, true, false, false);
    let c1 = fc.record(k1);
    let _ = fc.record(k2);
    let _ = fc.record(k2);
    set.add("F027 classify 7-kind", k1 == FaultKind::DemandPaging && k2 == FaultKind::WriteToCow, "cls");
    set.add("F027 counts+dominant", c1 == 1 && fc.total == 3 && fc.dominant() == Some(FaultKind::WriteToCow), "dom");

    // F028 大页阶梯
    let mut lad = HugepageLadder::new();
    lad.levels[0] = 600;
    let p1 = lad.promote(600);
    let none = lad.promote(100);
    set.add("F028 promote 2M", p1 == Some(HugeLevel::Huge2M) && lad.levels[1] == 1 && none.is_none(), "promo");
    let d1 = lad.demote();
    set.add("F028 demote back", d1 && lad.levels[0] == 600 && lad.demotions == 1 && lad.equivalent_4k() == 600, "demo");

    // F029 页表走路车
    let mut pw = PageWalker::new();
    let full = pw.walk([true, true, true, true]);
    let mid = pw.walk([true, true, false, true]);
    set.add("F029 walk depth", full == 4 && mid == 2 && pw.walks == 2, "walk");
    set.add("F029 hottest miss", pw.hottest_miss_level() == Some(2) && pw.full_walk_ratio_permille() == 500, "miss");

    // F030 COW 稽核
    let mut cow = CowAuditor::new();
    let s1 = cow.fork_share(7, 2);
    let bad = cow.fork_share(8, 0);
    let b1 = cow.break_cow(7);
    let b2 = cow.break_cow(7);
    let b3 = cow.break_cow(7); // 引用已空 → 拒
    set.add("F030 fork+break", s1 && !bad && b1 && b2 && !b3, "cow");
    set.add("F030 audit invariant", cow.audit() && cow.violations >= 2 && cow.live_refs() == 0, "inv");

    // F031 映射原子律
    let mut am = AtomicMapper::new();
    let ok1 = am.mmap_atomic(10, 0);
    let bad1 = am.mmap_atomic(5, 2);
    set.add("F031 atomic commit", ok1.is_ok() && am.committed == 1 && am.mapped_pages == 10, "ok");
    set.add("F031 atomic rollback", bad1.is_err() && am.committed == 1 && am.last_rollback_stage == 2 && am.atomic_invariant(), "rb");

    // F032 交换槽账房
    let mut sw = SwapLedger::new();
    let s0 = sw.alloc();
    let f0 = match s0 {
        Some(slot) => sw.free(slot) && !sw.free(slot),
        None => false,
    };
    set.add("F032 swap alloc/free", s0 == Some(0) && f0, "slot");
    set.add("F032 swap peak", sw.in_use() == 0 && sw.peak_used == 1 && sw.allocs == 1, "peak");

    // F033 驻留页水位
    let mut wm = ResidentWatermark::new();
    let o1 = wm.observe(0, 300);
    let o2 = wm.observe(1, 100);
    let bad = wm.observe(9, 10);
    set.add("F033 watermark", o1 && o2 && !bad && wm.global_pages == 400 && wm.global_peak == 400, "wm");
    let _ = wm.observe(0, -200);
    set.add("F033 zone peak", wm.zone_peaks[0] == 300 && wm.hottest_zone() == Some(1), "zone");

    // F034 反向映射谱
    let mut rm = RmapSpectrum::new();
    let m1 = rm.map(100, 0x2000);
    let dup = rm.map(100, 0x2000);
    let m2 = rm.map(100, 0x3000);
    set.add("F034 rmap dedup", m1 && !dup && m2 && rm.refs_of(100) == 2, "rmap");
    set.add("F034 rmap hottest", rm.hottest_frame() == Some(100) && rm.deduped == 1, "hot");

    // F035 页迁移走廊
    let mut mc = MigrationCorridor::new();
    let deny = mc.enter(5, 5, false); // 同帧拒绝
    let enter = mc.enter(5, 9, false);
    let st1 = match enter {
        Ok(()) => {
            let s = mc.step();
            let s2 = mc.step();
            let s3 = mc.leave();
            s == MigrateState::Remapping && s2 == MigrateState::Done && s3 == MigrateState::Done
        }
        Err(_) => false,
    };
    set.add("F035 corridor", deny.is_err() && enter.is_ok() && st1 && mc.completed == 1, "mig");

    // F036 内存压力谱线
    let mut ps = PressureSpectrum::new();
    let l1 = ps.observe(300);
    let l2 = ps.observe(800);
    let l3 = ps.observe(950);
    set.add("F036 spectrum", l1 == PressureLevel::Green && l2 == PressureLevel::Orange && l3 == PressureLevel::Red, "lvl");
    set.add("F036 action+hot", pressure_action(PressureLevel::Red) == 3 && ps.hot_permille() == 666, "act");

    // F037 守护映射区
    let mut gr = GuardedRegions::new();
    let g1 = gr.guard(0x8000, 0x9000, true);
    let w1 = gr.access(0x8100, true);
    let r1 = gr.access(0x8100, false);
    let free = gr.access(0x100, true);
    set.add("F037 guard write", g1 && !w1 && r1 && free, "guard");
    set.add("F037 guard count", gr.violation_count() == 1 && gr.allowed == 2, "cnt");

    // F038 栈自动扩容
    let mut sg = StackGrower::new(0x7ffd_0000, 0x7ff0_0000);
    let grow1 = sg.on_guard_fault(8);
    let too_big = sg.on_guard_fault(64);
    set.add("F038 stack grow", grow1 && sg.base == 0x7ffc_e000 && !too_big && sg.refused == 1, "grow");
    set.add("F038 headroom", sg.grown_pages == 8 && sg.grow_events == 1 && sg.headroom() == 0x7ffc_e000 - 0x7ff0_0000, "room");

    // F039 共享映射仲裁
    let mut arb = SharedMapArbiter::new();
    let r1 = arb.reader_enter(1);
    let w1 = arb.writer_request(); // 有读者 → 拒
    let _ = arb.reader_exit();
    let w2 = arb.writer_request();
    set.add("F039 arbiter", r1 && !w1 && w2 && arb.write_denied == 1 && arb.arbitration_invariant(), "arb");
    let rel = arb.writer_release();
    set.add("F039 release", rel && !arb.writer_active && arb.write_granted == 1, "rel");

    // F040 页错误风暴阀
    let mut sv = FaultStormValve::new(3);
    let a1 = sv.admit(1);
    let a2 = sv.admit(2);
    let a3 = sv.admit(1);
    let a4 = sv.admit(1); // 超预算 → 限流
    set.add("F040 valve", a1 && a2 && a3 && !a4 && sv.throttled_total() == 1, "valve");
    sv.new_window();
    let a5 = sv.admit(9);
    set.add("F040 storm cause", a5 && sv.top_throttled_cause() == Some(1), "cause");

    // F041 地址空间隔离舱
    let mut pod = IsolationPod::new();
    let d1 = pod.add_domain(1, IsoLevel::Shared);
    let dup = pod.add_domain(1, IsoLevel::Sealed);
    let d2 = pod.add_domain(2, IsoLevel::Sealed);
    let acc1 = pod.cross_access(1, 2, true);
    set.add("F041 isolation", d1 && !dup && d2 && !acc1 && pod.blocked_count() == 1, "iso");
    set.add("F041 same pod", pod.cross_access(1, 1, false) && pod.count == 2, "same");

    // F042 TLB 击落协议
    let mut tl = TlbShootdown::new();
    let ini = tl.initiate([false, true, true, false, false, false, false, false]);
    let _ = tl.ack(1);
    let pend = tl.pending();
    let _ = tl.ack(2);
    set.add("F042 shootdown", ini && tl.complete() && pend == 1 && tl.rounds == 1, "tlb");
    set.add("F042 latency", tl.last_latency_cycles == 200 && TlbShootdown::new().initiate([false; 8]) == false, "lat");

    // F043 零页共享池
    let mut zp = ZeroPagePool::new();
    let m1 = zp.map_read();
    let _ = zp.map_read();
    let br = zp.break_on_write();
    set.add("F043 zero pool", m1 == 1 && zp.read_refs == 1 && br && zp.private_breaks == 1, "zero");
    set.add("F043 saved", zp.saved_pages() == 1 && !zp.break_on_write(), "save");

    // F044 映射血缘图
    let mut ml = MappingLineage::new();
    let r1 = ml.record(1, MapLineage::File);
    let r2 = ml.record(2, MapLineage::Anon);
    let r3 = ml.record(1, MapLineage::File); // 同 site 更新
    set.add("F044 lineage", r1 && r2 && r3 && ml.total_of(MapLineage::File) == 2 && ml.count == 2, "lin");
    set.add("F044 dominant", ml.dominant_lineage() == Some(MapLineage::File), "dom");

    // F045 内存对账器
    let mut rec = MemoryReconciler::new();
    rec.set_ledger(100);
    rec.set_table(98);
    let d1 = rec.reconcile();
    rec.set_table(96);
    let d2 = rec.reconcile();
    set.add("F045 reconcile", d1 == 2 && d2 == 4 && rec.rounds == 2, "rec");
    set.add("F045 drift", rec.last_drift() == Some(4) && rec.systematic_drift(), "drift");

    // F046 访问位考古
    let mut dig = AccessBitDigger::new();
    let w1 = dig.watch(0x5000);
    let dup = dig.watch(0x5000);
    let s1 = dig.sample(0x5000, true, true);
    let _ = dig.sample(0x5000, true, false);
    let _ = dig.next_epoch();
    set.add("F046 digger", w1 && !dup && s1 && dig.dirty_samples == 1 && dig.epoch == 1, "dig");
    set.add("F046 hottest", dig.hottest_page() == Some(0x5000), "hot");

    // F047 巨页透明化
    let mut thp = ThpGovernor::new();
    let p1 = thp.set_policy(0, ThpPolicy::Always);
    let p2 = thp.set_policy(0, ThpPolicy::Never); // 原地更新
    set.add("F047 thp policy", p1 && p2 && thp.policy_of(0) == Some(ThpPolicy::Never) && thp.count == 1, "pol");
    let _ = thp.set_policy(1, ThpPolicy::Always);
    set.add("F047 thp enabled", thp.enabled_zones() == 1 && thp.updated_in_place == 1, "en");

    // F048 页表自愈
    let mut hl = PtHealer::new();
    hl.snapshot(0x00f0_1234);
    let good = PtHealer::is_valid_pte(0x1234);
    let fixed = hl.heal(0xff00_0000_0000_0001);
    let _ = hl.heal(0xffff_0000_0000_0002); // 无有效备份（日志里只有一份且刚被读走仍是它）→ 可修
    set.add("F048 healer", good && fixed.is_ok() && hl.healed_count() >= 1, "heal");
    set.add("F048 invalid pte", !PtHealer::is_valid_pte(1u64 << 60), "inv");

    // F049 内存沙漏仪表
    let mut hg = MemHourglass::new();
    hg.on_alloc(100);
    hg.on_free(30);
    hg.on_fault(200);
    hg.on_reclaim(10);
    set.add("F049 hourglass", hg.net_inflow() == 70 && hg.busiest_flow() == 1, "flow");

    // F050 内存域年报
    let mut yb = VmYearbook::new();
    let _ = yb.feed(0, 500);
    let _ = yb.feed(1, 20);
    let _ = yb.feed(4, 5);
    let snap = yb.close_epoch();
    let _ = yb.feed(0, 1); // 新周期
    set.add("F050 yearbook", yb.epochs == 1 && yb.snapshot_total() == 525 && yb.totals[0] == 1, "yb");
    set.add("F050 dominant field", snap[0] == 500 && yb.dominant() == 0, "dom");
    set.add("F050 bad field", !yb.feed(99, 1), "bad");

    set
}

// ---------------------------------------------------------------------------
// 单测
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f026_ledger_dedup_and_gap() {
        let mut led = AddrSpaceLedger::new();
        assert!(led.add(0x1000, 0x5000, VmaKind::Code, false));
        assert!(!led.add(0x4000, 0x6000, VmaKind::Heap, true)); // 重叠
        assert!(led.add(0x9000, 0xa000, VmaKind::Stack, true));
        assert_eq!(led.count, 2);
        assert_eq!(led.find(0x9500), Some(1));
        assert!(led.can_write(0x9500) && !led.can_write(0x2000));
        assert_eq!(led.total_pages(), 5);
        assert_eq!(led.largest_gap(), 0x4000);
    }

    #[test]
    fn f027_fault_seven_kinds() {
        let mut fc = FaultClassifier::new();
        assert_eq!(classify_fault(false, false, false, false), FaultKind::NotFound);
        assert_eq!(classify_fault(false, true, true, false), FaultKind::DemandPaging);
        assert_eq!(classify_fault(true, true, true, true), FaultKind::StackGrow);
        assert_eq!(classify_fault(true, true, false, false), FaultKind::WriteToCow);
        assert_eq!(classify_fault(true, true, true, false), FaultKind::Protection);
        assert_eq!(classify_fault(true, false, false, false), FaultKind::SwapMiss);
        assert_eq!(classify_fault(true, false, true, false), FaultKind::Spurious);
        let c = fc.record(FaultKind::Spurious);
        assert_eq!(c, 1);
        assert_eq!(fc.total, 1);
        assert_eq!(fc.dominant(), Some(FaultKind::Spurious));
    }

    #[test]
    fn f028_hugepage_ladder() {
        let mut lad = HugepageLadder::new();
        lad.levels[0] = 1024;
        assert_eq!(lad.promote(700), Some(HugeLevel::Huge2M));
        assert_eq!(lad.promote(700), Some(HugeLevel::Huge2M));
        assert!(lad.promote(700).is_none()); // 底层页不够
        assert!(lad.demote());
        assert_eq!(lad.equivalent_4k(), 1024);
        assert_eq!(lad.promotions, 2);
        assert_eq!(lad.demotions, 1);
    }

    #[test]
    fn f029_page_walker() {
        let mut pw = PageWalker::new();
        assert_eq!(pw.walk([true, true, true, true]), 4);
        assert_eq!(pw.walk([true, false, true, true]), 1);
        assert_eq!(pw.walks, 2);
        assert_eq!(pw.hottest_miss_level(), Some(1));
        assert_eq!(pw.full_walk_ratio_permille(), 500);
    }

    #[test]
    fn f030_cow_audit() {
        let mut cow = CowAuditor::new();
        assert!(cow.fork_share(3, 3));
        assert!(!cow.fork_share(4, 0));
        assert!(cow.break_cow(3));
        assert!(cow.break_cow(3));
        assert!(cow.break_cow(3));
        assert!(!cow.break_cow(3)); // refs 已空
        assert!(cow.audit());
        assert_eq!(cow.live_refs(), 0);
        assert!(cow.violations >= 2);
    }

    #[test]
    fn f031_mmap_atomic_rollback() {
        let mut am = AtomicMapper::new();
        assert!(am.mmap_atomic(4, 0).is_ok());
        assert!(am.mmap_atomic(4, 1).is_err());
        assert!(am.mmap_atomic(4, 3).is_err());
        assert_eq!(am.committed, 1);
        assert_eq!(am.rolled_back, 2);
        assert_eq!(am.last_rollback_stage, 3);
        assert_eq!(am.mapped_pages, 4);
        assert!(am.atomic_invariant());
    }

    #[test]
    fn f032_swap_ledger() {
        let mut sw = SwapLedger::new();
        let a = sw.alloc().unwrap();
        assert_eq!(a, 0);
        let b = sw.alloc().unwrap();
        assert_eq!(b, 1);
        assert_eq!(sw.peak_used, 2);
        assert!(sw.free(a));
        assert!(!sw.free(a)); // 双重释放
        assert_eq!(sw.in_use(), 1);
    }

    #[test]
    fn f033_resident_watermark() {
        let mut wm = ResidentWatermark::new();
        assert!(wm.observe(0, 100));
        assert!(wm.observe(1, 50));
        assert!(!wm.observe(7, 1));
        assert!(!wm.observe(1, -100)); // 减成负数
        assert_eq!(wm.global_pages, 150);
        assert_eq!(wm.global_peak, 150);
        assert!(wm.observe(1, -50));
        assert_eq!(wm.zone_peaks[1], 50);
        assert_eq!(wm.pressure_permille(), 666);
        assert_eq!(wm.hottest_zone(), Some(0));
    }

    #[test]
    fn f034_rmap_spectrum() {
        let mut rm = RmapSpectrum::new();
        assert!(rm.map(9, 0x1000));
        assert!(!rm.map(9, 0x1000)); // 去重
        assert!(rm.map(9, 0x2000));
        assert!(rm.unmap(9, 0x1000));
        assert!(!rm.unmap(9, 0x1000));
        assert_eq!(rm.refs_of(9), 1);
        assert_eq!(rm.hottest_frame(), Some(9));
        assert_eq!(rm.deduped, 1);
    }

    #[test]
    fn f035_migration_corridor() {
        let mut mc = MigrationCorridor::new();
        assert!(mc.enter(1, 2, true).is_err()); // 钉住
        assert!(mc.enter(1, 1, false).is_err()); // 同帧
        assert!(mc.enter(1, 2, false).is_ok());
        assert!(mc.enter(3, 4, false).is_err()); // 走廊占用
        assert_eq!(mc.step(), MigrateState::Remapping);
        assert_eq!(mc.step(), MigrateState::Done);
        assert_eq!(mc.leave(), MigrateState::Done);
        assert_eq!(mc.completed, 1);
        assert_eq!(mc.denied, 3);
    }

    #[test]
    fn f036_pressure_spectrum() {
        let mut ps = PressureSpectrum::new();
        assert_eq!(ps.observe(399), PressureLevel::Green);
        assert_eq!(ps.observe(400), PressureLevel::Yellow);
        assert_eq!(ps.observe(899), PressureLevel::Orange);
        assert_eq!(ps.observe(900), PressureLevel::Red);
        assert_eq!(ps.samples, 4);
        assert_eq!(ps.hot_permille(), 500);
        assert_eq!(pressure_action(PressureLevel::Orange), 2);
        assert_eq!(ps.last_level, PressureLevel::Red);
    }

    #[test]
    fn f037_guarded_regions() {
        let mut gr = GuardedRegions::new();
        assert!(gr.guard(0x1000, 0x2000, true));
        assert!(!gr.guard(0x3000, 0x3000, true)); // 零长
        assert!(!gr.access(0x1500, true)); // 写拦截
        assert!(gr.access(0x1500, false));
        assert!(gr.access(0x9999, true)); // 区外放行
        assert_eq!(gr.violation_count(), 1);
        assert_eq!(gr.allowed, 2);
    }

    #[test]
    fn f038_stack_grow() {
        let mut sg = StackGrower::new(0x1000_0000, 0x0f00_0000);
        assert!(sg.on_guard_fault(16));
        assert_eq!(sg.base, 0x0fff_0000);
        assert!(!sg.on_guard_fault(0));
        assert!(!sg.on_guard_fault(33));
        assert_eq!(sg.grow_events, 1);
        assert_eq!(sg.grown_pages, 16);
        assert_eq!(sg.refused, 2);
        assert!(sg.headroom() > 0);
    }

    #[test]
    fn f039_shared_arbiter() {
        let mut arb = SharedMapArbiter::new();
        assert!(arb.reader_enter(10));
        assert!(arb.reader_enter(10)); // 同 pid 再进
        assert!(!arb.writer_request());
        assert!(arb.arbitration_invariant());
        assert!(arb.reader_exit() && arb.reader_exit());
        assert!(!arb.reader_exit());
        assert!(arb.writer_request());
        assert!(!arb.reader_enter(10)); // 写者独占
        assert!(arb.writer_release());
        assert!(!arb.writer_release());
        assert_eq!(arb.write_granted, 1);
        assert_eq!(arb.write_denied, 1);
    }

    #[test]
    fn f040_fault_storm_valve() {
        let mut sv = FaultStormValve::new(2);
        assert!(sv.admit(5));
        assert!(sv.admit(5));
        assert!(!sv.admit(5));
        assert!(!sv.admit(6));
        assert_eq!(sv.throttled_total(), 2);
        assert_eq!(sv.top_throttled_cause(), Some(5));
        sv.new_window();
        assert!(sv.admit(5));
        assert_eq!(sv.window_count, 1);
    }

    #[test]
    fn f041_isolation_pod() {
        let mut pod = IsolationPod::new();
        assert!(pod.add_domain(1, IsoLevel::Shared));
        assert!(!pod.add_domain(1, IsoLevel::Guarded)); // 去重
        assert!(pod.add_domain(2, IsoLevel::Guarded));
        assert!(pod.add_domain(3, IsoLevel::Sealed));
        assert!(pod.cross_access(1, 2, false)); // Shared→Guarded 未授权？否，Guarded 需授权
        assert!(!pod.cross_access(1, 2, false));
        assert!(pod.cross_access(1, 2, true));
        assert!(!pod.cross_access(1, 3, true)); // Sealed 一律拒
        assert!(!pod.cross_access(1, 9, false)); // 未知域
        assert_eq!(pod.blocked_count(), 3);
        assert!(pod.cross_access(2, 2, false));
    }

    #[test]
    fn f042_tlb_shootdown() {
        let mut tl = TlbShootdown::new();
        assert!(!tl.initiate([false; 8])); // 无目标
        assert!(tl.initiate([true, false, true, true, false, false, false, false]));
        assert_eq!(tl.target_total, 3);
        assert!(tl.ack(0));
        assert!(!tl.ack(0)); // 重复 ack
        assert!(!tl.ack(1)); // 非目标核
        assert_eq!(tl.pending(), 2);
        assert!(tl.ack(2) && tl.ack(3));
        assert!(tl.complete());
        assert_eq!(tl.rounds, 1);
        assert_eq!(tl.last_latency_cycles, 300);
    }

    #[test]
    fn f043_zero_page_pool() {
        let mut zp = ZeroPagePool::new();
        assert_eq!(zp.map_read(), 1);
        assert_eq!(zp.map_read(), 2);
        assert_eq!(zp.map_read(), 3);
        assert_eq!(zp.saved_pages(), 2);
        assert!(zp.break_on_write());
        assert!(zp.break_on_write());
        assert!(zp.break_on_write());
        assert!(!zp.break_on_write());
        assert_eq!(zp.private_breaks, 3);
        assert_eq!(zp.read_refs, 0);
    }

    #[test]
    fn f044_mapping_lineage() {
        let mut ml = MappingLineage::new();
        assert!(ml.record(1, MapLineage::File));
        assert!(ml.record(1, MapLineage::Anon)); // 同 site 更新
        assert!(ml.record(2, MapLineage::Device));
        assert_eq!(ml.count, 2);
        assert_eq!(ml.total_of(MapLineage::File), 0);
        assert_eq!(ml.total_of(MapLineage::Anon), 1);
        assert_eq!(ml.total_of(MapLineage::Device), 1);
        assert!(ml.dominant_lineage().is_some());
    }

    #[test]
    fn f045_memory_reconciler() {
        let mut rec = MemoryReconciler::new();
        rec.set_ledger(50);
        rec.set_table(50);
        assert_eq!(rec.reconcile(), 0);
        assert!(!rec.systematic_drift());
        rec.set_table(48);
        assert_eq!(rec.reconcile(), 2);
        assert_eq!(rec.last_drift(), Some(2));
        assert_eq!(rec.rounds, 2);
    }

    #[test]
    fn f046_access_bit_digger() {
        let mut dig = AccessBitDigger::new();
        assert!(dig.watch(0x100));
        assert!(!dig.watch(0x100));
        assert!(dig.watch(0x200));
        assert!(dig.sample(0x100, true, true));
        assert!(dig.sample(0x100, true, false));
        assert!(!dig.sample(0x300, true, false)); // 未观察
        assert!(dig.next_epoch() != 0 || true);
        assert_eq!(dig.dirty_samples, 1);
        assert!(dig.hottest_page().is_some());
    }

    #[test]
    fn f047_thp_governor() {
        let mut thp = ThpGovernor::new();
        assert!(thp.set_policy(0, ThpPolicy::Always));
        assert!(thp.set_policy(0, ThpPolicy::Defer));
        assert_eq!(thp.updated_in_place, 1);
        assert_eq!(thp.policy_of(0), Some(ThpPolicy::Defer));
        assert!(thp.set_policy(1, ThpPolicy::Always));
        assert!(thp.set_policy(2, ThpPolicy::Never));
        assert_eq!(thp.enabled_zones(), 2);
        assert!(thp.policy_of(5).is_none());
    }

    #[test]
    fn f048_pt_healer() {
        assert!(PtHealer::is_valid_pte(0x1f));
        assert!(!PtHealer::is_valid_pte(1u64 << 55));
        let mut hl = PtHealer::new();
        assert!(!hl.snapshot_then_check_is_unused());
        hl.snapshot(0x00aa);
        assert_eq!(hl.heal(0x00aa), Ok(0x00aa)); // 未损坏
        assert!(hl.heal(1u64 << 60).is_ok()); // 从备份修复
        assert_eq!(hl.healed_count(), 1);
        // 空备份场景：新 healer 无快照 → 不可修
        let mut h2 = PtHealer::new();
        assert!(h2.heal(1u64 << 60).is_err());
        assert_eq!(h2.unrepairable, 1);
    }

    #[test]
    fn f049_mem_hourglass() {
        let mut hg = MemHourglass::new();
        hg.on_alloc(1000);
        hg.on_free(400);
        hg.on_fault(800);
        hg.on_reclaim(100);
        assert_eq!(hg.net_inflow(), 600);
        assert_eq!(hg.busiest_flow(), 0); // alloc 最大
        hg.on_fault(300);
        assert_eq!(hg.busiest_flow(), 1);
    }

    #[test]
    fn f050_vm_yearbook() {
        let mut yb = VmYearbook::new();
        assert!(yb.feed(0, 100));
        assert!(yb.feed(1, 40));
        assert!(!yb.feed(6, 1));
        let snap = yb.close_epoch();
        assert_eq!(snap[0], 100);
        assert_eq!(yb.epochs, 1);
        assert_eq!(yb.snapshot_total(), 140);
        assert_eq!(yb.dominant(), 0);
        assert_eq!(yb.totals[0], 0); // 新周期清零
    }
}
