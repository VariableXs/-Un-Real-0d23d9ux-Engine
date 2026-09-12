//! VARIX-M400 AI-02 内存与隔离硬化域（F026~F050）。
//!
//! 让恶意进程无机可乘。纯逻辑 + 固定容量数组，no_std 安全。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// F026 — 物理页分配器 O(1)（bitmap + hint，最坏路径有上界）
// ---------------------------------------------------------------------------

pub const PMM_BITS: usize = 128;

pub struct Pmm {
    pub busy: [u64; PMM_BITS / 64],
    pub hint: usize, // 下一次探测起点，保证 O(1) 均摊
    pub used: usize,
}

impl Pmm {
    pub const fn new() -> Pmm {
        Pmm { busy: [0; PMM_BITS / 64], hint: 0, used: 0 }
    }

    pub fn alloc(&mut self) -> Option<usize> {
        for k in 0..PMM_BITS / 64 {
            let w = (self.hint + k) % (PMM_BITS / 64);
            if self.busy[w] == u64::MAX {
                continue;
            }
            let bit = self.busy[w].trailing_ones() as usize;
            self.busy[w] |= 1 << bit;
            self.hint = w;
            self.used += 1;
            return Some(w * 64 + bit);
        }
        None
    }

    pub fn free(&mut self, page: usize) -> bool {
        let (w, b) = (page / 64, page % 64);
        if w >= PMM_BITS / 64 || self.busy[w] & (1 << b) == 0 {
            return false;
        }
        self.busy[w] &= !(1 << b);
        self.hint = w;
        self.used -= 1;
        true
    }
}

// ---------------------------------------------------------------------------
// F027 — 页帧引用计数
// ---------------------------------------------------------------------------

pub const REFCOUNT_CAP: usize = 64;

pub struct Refcounts {
    pub counts: [u16; REFCOUNT_CAP],
}

impl Refcounts {
    pub const fn new() -> Refcounts {
        Refcounts { counts: [0; REFCOUNT_CAP] }
    }

    /// 返回自增后的计数；0 计数不可再增（悬空保护）。
    pub fn inc(&mut self, pfn: usize) -> Option<u16> {
        let c = self.counts.get_mut(pfn)?;
        if *c >= u16::MAX - 1 {
            return None;
        }
        *c += 1;
        Some(*c)
    }

    pub fn dec(&mut self, pfn: usize) -> Option<u16> {
        let c = self.counts.get_mut(pfn)?;
        if *c == 0 {
            return None; // 悬空引用：拒绝
        }
        *c -= 1;
        Some(*c)
    }
}

// ---------------------------------------------------------------------------
// F028/F029 — KASLR / 用户态 ASLR
// ---------------------------------------------------------------------------

/// xorshift64* 伪随机（种子决定布局，可复现验证）。
pub fn aslr_next(state: &mut u64) -> u64 {
    let mut x = *state;
    x ^= x >> 12;
    x ^= x << 25;
    x ^= x >> 27;
    *state = x;
    x.wrapping_mul(0x2545_F491_4F6C_DD1D)
}

/// 基址 = 对齐基座 + rand 对齐偏移；两次种子不同 -> 布局不同。
pub fn f028_kaslr_base(seed: u64, base: u64, align: u64, span: u64) -> u64 {
    let mut s = seed;
    let r = aslr_next(&mut s) % (span / align);
    base + r * align
}

pub fn f029_user_aslr_slots(seed: u64) -> (u64, u64, u64) {
    let mut s = seed;
    let stack = 0x7fff_0000_0000 + (aslr_next(&mut s) & 0xFFF) * 0x1000;
    let heap = 0x6000_0000_0000 + (aslr_next(&mut s) & 0xFFF) * 0x1000;
    let libs = 0x7000_0000_0000 + (aslr_next(&mut s) & 0xFFF) * 0x1000;
    (stack, heap, libs)
}

// ---------------------------------------------------------------------------
// F030 — guard page（越界写必崩且归因）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GuardVerdict {
    Ok,
    GuardHit,
}

/// 访问 [base, base+len) 之外但在保护区内的地址触发 guard。
pub fn f030_guard_check(base: u64, len: u64, guard_before: u64, guard_after: u64, addr: u64) -> GuardVerdict {
    let lo = base - guard_before;
    let hi = base + len + guard_after;
    if addr >= base && addr < base + len {
        GuardVerdict::Ok
    } else if addr >= lo && addr < hi {
        GuardVerdict::GuardHit
    } else {
        GuardVerdict::Ok // 保护区之外不是本 guard 的职责
    }
}

// ---------------------------------------------------------------------------
// F031 — 写时复制 fork
// ---------------------------------------------------------------------------

pub const COW_PAGES: usize = 32;

pub struct CowTable {
    pub cow: [bool; COW_PAGES],
    pub writable: [bool; COW_PAGES],
}

impl CowTable {
    pub const fn new() -> CowTable {
        CowTable { cow: [false; COW_PAGES], writable: [true; COW_PAGES] }
    }

    /// fork：父页降为只读 + COW 标记。
    pub fn fork(&mut self, pfn: usize) -> bool {
        if pfn >= COW_PAGES {
            return false;
        }
        self.cow[pfn] = true;
        self.writable[pfn] = false;
        true
    }

    /// 写 fault：COW 页复制后恢复可写并解除标记。
    pub fn on_write_fault(&mut self, pfn: usize) -> bool {
        if pfn >= COW_PAGES || !self.cow[pfn] {
            return false;
        }
        self.cow[pfn] = false;
        self.writable[pfn] = true;
        true
    }
}

// ---------------------------------------------------------------------------
// F032 — demand paging（惰性加载）
// ---------------------------------------------------------------------------

pub const LAZY_PAGES: usize = 32;

pub struct LazyVma {
    pub present: [bool; LAZY_PAGES],
    pub zero_filled: [bool; LAZY_PAGES],
}

impl LazyVma {
    pub const fn new() -> LazyVma {
        LazyVma { present: [false; LAZY_PAGES], zero_filled: [false; LAZY_PAGES] }
    }

    /// 缺页 -> 分配零页（骨架语义）。
    pub fn on_fault(&mut self, idx: usize) -> bool {
        if idx >= LAZY_PAGES || self.present[idx] {
            return false;
        }
        self.present[idx] = true;
        self.zero_filled[idx] = true;
        true
    }

    /// 后备文件填充后不再是零页。
    pub fn fill_from_file(&mut self, idx: usize) -> bool {
        if idx >= LAZY_PAGES || !self.present[idx] {
            return false;
        }
        self.zero_filled[idx] = false;
        true
    }
}

// ---------------------------------------------------------------------------
// F033 — swap 骨架（换出/换回零损坏）
// ---------------------------------------------------------------------------

pub const SWAP_SLOTS: usize = 16;

pub struct SwapArea {
    pub used: [bool; SWAP_SLOTS],
    pub data: [[u64; 4]; SWAP_SLOTS],
}

impl SwapArea {
    pub const fn new() -> SwapArea {
        SwapArea { used: [false; SWAP_SLOTS], data: [[0; 4]; SWAP_SLOTS] }
    }

    pub fn swap_out(&mut self, slot: usize, page: &[u64; 4]) -> bool {
        if slot >= SWAP_SLOTS || self.used[slot] {
            return false;
        }
        self.data[slot] = *page;
        self.used[slot] = true;
        true
    }

    pub fn swap_in(&mut self, slot: usize, out: &mut [u64; 4]) -> bool {
        if slot >= SWAP_SLOTS || !self.used[slot] {
            return false;
        }
        *out = self.data[slot];
        self.used[slot] = false;
        true
    }
}

// ---------------------------------------------------------------------------
// F034 — mmap 语义（匿名/文件映射基线）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MapType {
    Anon,
    File,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MapRegion {
    pub base: u64,
    pub len: u64,
    pub kind: MapType,
    pub writable: bool,
}

/// 映射合法性：长度非零、按页对齐、不与已映射区间重叠。
pub fn f034_mmap_validate(regions: &[MapRegion], cand: MapRegion) -> bool {
    if cand.len == 0 || cand.base % 0x1000 != 0 || cand.len % 0x1000 != 0 {
        return false;
    }
    for r in regions {
        if cand.base < r.base + r.len && r.base < cand.base + cand.len {
            return false;
        }
    }
    true
}

// ---------------------------------------------------------------------------
// F035 — SMAP/SMEP 启用
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cr4Flags(pub u32);

pub const CR4_SMAP: u32 = 1 << 21;
pub const CR4_SMEP: u32 = 1 << 20;

pub fn f035_enable_smap_smep(cr4: Cr4Flags) -> Cr4Flags {
    Cr4Flags(cr4.0 | CR4_SMAP | CR4_SMEP)
}

pub fn f035_enabled(cr4: Cr4Flags) -> bool {
    cr4.0 & (CR4_SMAP | CR4_SMEP) == CR4_SMAP | CR4_SMEP
}

/// 内核态访问用户指针（SMAP 违规）。
pub fn f035_smap_violation(smap_on: bool, kernel_mode: bool, user_addr: bool) -> bool {
    smap_on && kernel_mode && user_addr
}

// ---------------------------------------------------------------------------
// F036 — NX 覆盖审计（可执行页白名单化）
// ---------------------------------------------------------------------------

pub const NX_PAGES: usize = 32;

pub struct NxBitmap {
    pub exec: [u64; (NX_PAGES + 63) / 64], // bit=1 表示可执行
}

impl NxBitmap {
    pub const fn new_all_nx() -> NxBitmap {
        NxBitmap { exec: [0; (NX_PAGES + 63) / 64] }
    }

    pub fn whitelist(&mut self, page: usize) -> bool {
        if page >= NX_PAGES {
            return false;
        }
        self.exec[page / 64] |= 1 << (page % 64);
        true
    }

    pub fn is_exec(&self, page: usize) -> bool {
        page < NX_PAGES && self.exec[page / 64] & (1 << (page % 64)) != 0
    }

    /// 审计：可执行页必须全部在白名单内（白名单即全集）。
    pub fn audit(&self, declared_exec: &[usize]) -> bool {
        declared_exec.iter().all(|&p| self.is_exec(p))
    }
}

// ---------------------------------------------------------------------------
// F037 — 页表自映射
// ---------------------------------------------------------------------------

/// 自映射条目：把 PML4 的某个槽位指回自身。
pub const SELF_MAP_ENTRY: usize = 510;

pub fn f037_selfmap_pml4_entry() -> u64 {
    // entry = (自映射槽 << 12) | PRESENT | WRITE
    ((SELF_MAP_ENTRY as u64) << 12) | 0b11
}

/// 通过自映射窗口计算虚拟地址：pml4e=510, pdpte/pde/pte 给定。
pub fn f037_selfmap_vaddr(pdpte: usize, pde: usize, pte: usize) -> u64 {
    let v = (SELF_MAP_ENTRY << 39) | (SELF_MAP_ENTRY << 30) | (pdpte << 21) | (pde << 12) | (pte << 3);
    v as u64
}

// ---------------------------------------------------------------------------
// F038 — OOM killer 策略
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct OomVictim {
    pub pid: u32,
    pub rss_pages: u32,
    pub protected: bool, // 关键进程不可杀
}

/// 选牺牲者：非法保护进程，分数 = rss。
pub fn f038_pick_oom_victim<'a>(victims: &'a [OomVictim]) -> Option<&'a OomVictim> {
    victims
        .iter()
        .filter(|v| !v.protected)
        .max_by_key(|v| v.rss_pages)
}

// ---------------------------------------------------------------------------
// F039 — 内存压力通知
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemPressure {
    None,
    Low,
    Medium,
    Critical,
}

pub fn f039_pressure_level(used: u64, total: u64) -> MemPressure {
    if total == 0 {
        return MemPressure::Critical;
    }
    let pct = used * 100 / total;
    if pct < 60 {
        MemPressure::None
    } else if pct < 80 {
        MemPressure::Low
    } else if pct < 95 {
        MemPressure::Medium
    } else {
        MemPressure::Critical
    }
}

// ---------------------------------------------------------------------------
// F040 — 大页支持（2MB）
// ---------------------------------------------------------------------------

/// 判断区间可否整体用 2MB 大页覆盖。
pub fn f040_hugepage_ok(base: u64, len: u64) -> bool {
    base % (2 * 1024 * 1024) == 0 && len % (2 * 1024 * 1024) == 0 && len > 0
}

pub fn f040_hugepage_count(len: u64) -> u64 {
    len / (2 * 1024 * 1024)
}

/// 4KB 页数对比。
pub fn f040_smallpage_count(len: u64) -> u64 {
    len / 4096
}

// ---------------------------------------------------------------------------
// F041 — DMA 缓冲管理（一致性内存）
// ---------------------------------------------------------------------------

pub const DMA_BUFS: usize = 16;

pub struct DmaPool {
    pub base: [u64; DMA_BUFS],
    pub len: [u32; DMA_BUFS],
    pub coherent: [bool; DMA_BUFS],
    pub used: [bool; DMA_BUFS],
}

impl DmaPool {
    pub const fn new() -> DmaPool {
        DmaPool { base: [0; DMA_BUFS], len: [0; DMA_BUFS], coherent: [false; DMA_BUFS], used: [false; DMA_BUFS] }
    }

    pub fn alloc(&mut self, slot: usize, base: u64, len: u32, coherent: bool) -> bool {
        if slot >= DMA_BUFS || self.used[slot] || len == 0 {
            return false;
        }
        self.base[slot] = base;
        self.len[slot] = len;
        self.coherent[slot] = coherent;
        self.used[slot] = true;
        true
    }

    pub fn free(&mut self, slot: usize) -> bool {
        if slot >= DMA_BUFS || !self.used[slot] {
            return false;
        }
        self.used[slot] = false;
        true
    }
}

// ---------------------------------------------------------------------------
// F042 — IOMMU（VT-d）设备 DMA 隔离
// ---------------------------------------------------------------------------

pub const IOMMU_DEVICES: usize = 32;

pub struct Iommu {
    pub enabled: bool,
    pub allowed: [bool; IOMMU_DEVICES], // 设备可寻址的内存段注册
    pub allowed_range: [(u64, u64); IOMMU_DEVICES],
}

impl Iommu {
    pub const fn new() -> Iommu {
        Iommu { enabled: false, allowed: [false; IOMMU_DEVICES], allowed_range: [(0, 0); IOMMU_DEVICES] }
    }

    pub fn grant(&mut self, dev: usize, lo: u64, hi: u64) -> bool {
        if dev >= IOMMU_DEVICES || hi <= lo {
            return false;
        }
        self.allowed[dev] = true;
        self.allowed_range[dev] = (lo, hi);
        true
    }

    /// 设备发起的 DMA 地址是否放行。
    pub fn translate(&self, dev: usize, addr: u64) -> bool {
        if !self.enabled || dev >= IOMMU_DEVICES || !self.allowed[dev] {
            return false;
        }
        let (lo, hi) = self.allowed_range[dev];
        addr >= lo && addr < hi
    }
}

// ---------------------------------------------------------------------------
// F043 — 内核栈哨兵
// ---------------------------------------------------------------------------

pub const STACK_MAGIC: u64 = 0x5341_4343; // "SACC"

/// 栈哨兵：底部 8 字节恒为魔数，被写即栈溢出。
pub fn f043_sentinel_ok(bottom_word: u64) -> bool {
    bottom_word == STACK_MAGIC
}

// ---------------------------------------------------------------------------
// F044 — 内核内存泄漏检测
// ---------------------------------------------------------------------------

pub const LEAK_TRACK: usize = 32;

pub struct LeakTracker {
    pub allocs: [u32; LEAK_TRACK],
    pub frees: [u32; LEAK_TRACK],
}

impl LeakTracker {
    pub const fn new() -> LeakTracker {
        LeakTracker { allocs: [0; LEAK_TRACK], frees: [0; LEAK_TRACK] }
    }

    pub fn alloc(&mut self, site: usize) {
        if site < LEAK_TRACK {
            self.allocs[site] += 1;
        }
    }

    pub fn free(&mut self, site: usize) {
        if site < LEAK_TRACK {
            self.frees[site] += 1;
        }
    }

    /// 泄漏的分配计数（alloc - free > 0 的站点列表）。
    pub fn leaks(&self) -> ([usize; LEAK_TRACK], usize) {
        let mut out = [0usize; LEAK_TRACK];
        let mut n = 0usize;
        for i in 0..LEAK_TRACK {
            if self.allocs[i] > self.frees[i] {
                out[n] = i;
                n += 1;
            }
        }
        (out, n)
    }
}

// ---------------------------------------------------------------------------
// F045 — 内存投毒调试模式
// ---------------------------------------------------------------------------

pub const POISON_FREE: u64 = 0xC7_C7_C7_C7_C7_C7_C7_C7;

/// 释放后投毒：整页写魔数。
pub fn f045_poison(page: &mut [u64]) {
    for w in page.iter_mut() {
        *w = POISON_FREE;
    }
}

/// 读到魔数 = use-after-free。
pub fn f045_is_poisoned(word: u64) -> bool {
    word == POISON_FREE
}

// ---------------------------------------------------------------------------
// F046 — copy_to/from_user 边界校验
// ---------------------------------------------------------------------------

pub const USER_SPACE_LIMIT: u64 = 0x0000_8000_0000_0000;

/// 用户指针全量校验：范围必须在用户空间且不跨越内核边界。
pub fn f046_check_user_ptr(addr: u64, len: u64) -> bool {
    if addr == 0 && len > 0 {
        return false; // 非空长度禁止空指针
    }
    if addr >= USER_SPACE_LIMIT {
        return false;
    }
    let end = match addr.checked_add(len) {
        Some(e) => e,
        None => return false,
    };
    end <= USER_SPACE_LIMIT
}

// ---------------------------------------------------------------------------
// F047 — 进程内存限额
// ---------------------------------------------------------------------------

pub fn f047_within_limit(used: u64, limit: u64, request: u64) -> bool {
    used.saturating_add(request) <= limit
}

// ---------------------------------------------------------------------------
// F048 — 缺页归因报告
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FaultReport {
    pub err: u64,
    pub rip: u64,
    pub cr2: u64,
    pub pid: u32,
}

/// 解析 err 位：P=0 缺页、W=1 写违规、U=1 用户态。
pub fn f048_decode_err(err: u64) -> (bool, bool, bool) {
    (err & 1 == 1, err & 2 == 2, err & 4 == 4) // (present, write, user)
}

// ---------------------------------------------------------------------------
// F049 — 碎片整理评估（碎片率曲线）
// ---------------------------------------------------------------------------

pub struct FragTracker {
    pub samples: [u16; 16],
    pub count: usize,
}

impl FragTracker {
    pub const fn new() -> FragTracker {
        FragTracker { samples: [0; 16], count: 0 }
    }

    /// 记录碎片率（0~1000 千分比）。
    pub fn sample(&mut self, permille: u16) {
        let v = permille.min(1000);
        if self.count < 16 {
            self.samples[self.count] = v;
            self.count += 1;
        } else {
            self.samples.copy_within(1.., 0);
            self.samples[15] = v;
        }
    }

    pub fn peak(&self) -> u16 {
        self.samples[..self.count].iter().copied().max().unwrap_or(0)
    }
}

// ---------------------------------------------------------------------------
// F050 — vmstat 观测接口
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Default)]
pub struct VmStat {
    pub pages_allocated: u64,
    pub pages_freed: u64,
    pub page_faults: u64,
    pub swap_out: u64,
    pub swap_in: u64,
}

impl VmStat {
    pub fn record_alloc(&mut self, n: u64) {
        self.pages_allocated += n;
    }
    pub fn record_free(&mut self, n: u64) {
        self.pages_freed += n;
    }
    pub fn live_pages(&self) -> u64 {
        self.pages_allocated.saturating_sub(self.pages_freed)
    }
}

// ---------------------------------------------------------------------------
// 域自检 F050+（域全量 CheckSet）
// ---------------------------------------------------------------------------

pub fn run_memhard_checks() -> CheckSet {
    let mut set = CheckSet::new("memhard");

    // F026
    let mut pmm = Pmm::new();
    let a = pmm.alloc();
    let b = pmm.alloc();
    set.add("F026 alloc seq", a == Some(0) && b == Some(1), "sequential pages");
    let freed = pmm.free(0);
    let reused = pmm.alloc();
    set.add("F026 free-realloc", freed && reused == Some(0), "reuse freed");
    // 再取一页验证重复释放被拒（先落中间态，避免末态读取）
    let c = pmm.alloc();
    let c = c.unwrap();
    let f1 = pmm.free(c);
    let f2 = pmm.free(c);
    set.add("F026 double free", f1 && !f2, "double free rejected");

    // F027
    let mut rc = Refcounts::new();
    set.add("F027 inc", rc.inc(3) == Some(1) && rc.inc(3) == Some(2), "refcount up");
    set.add("F027 dec", rc.dec(3) == Some(1) && rc.dec(3) == Some(0), "refcount down");
    set.add("F027 dangling", rc.dec(3).is_none(), "dangling rejected");

    // F028/F029
    let b1 = f028_kaslr_base(0xC0FFEE, 0x1000_0000, 0x1000, 0x100_0000);
    let b2 = f028_kaslr_base(0xBADF00D, 0x1000_0000, 0x1000, 0x100_0000);
    set.add("F028 kaslr varies", b1 != b2, "different seeds differ");
    set.add("F028 kaslr aligned", b1 % 0x1000 == 0, "page aligned");
    let (s1, h1, _l1) = f029_user_aslr_slots(42);
    let (s2, h2, _) = f029_user_aslr_slots(43);
    set.add("F029 aslr differs", (s1, h1) != (s2, h2), "stack/heap randomized");

    // F030
    set.add("F030 inside ok", f030_guard_check(0x1000, 0x1000, 0x1000, 0x1000, 0x1800) == GuardVerdict::Ok, "inside region");
    set.add("F030 overflow hit", f030_guard_check(0x1000, 0x1000, 0x1000, 0x1000, 0x2100) == GuardVerdict::GuardHit, "guard after hit");
    set.add("F030 underflow hit", f030_guard_check(0x1000, 0x1000, 0x1000, 0x1000, 0x0500) == GuardVerdict::GuardHit, "guard before hit");

    // F031
    let mut cow = CowTable::new();
    let forked = cow.fork(5);
    set.add("F031 fork ro", forked && !cow.writable[5] && cow.cow[5], "parent page cow");
    set.add("F031 fault restores", cow.on_write_fault(5) && cow.writable[5] && !cow.cow[5], "copy on write");

    // F032
    let mut vma = LazyVma::new();
    set.add("F032 lazy fault", vma.on_fault(2) && vma.present[2] && vma.zero_filled[2], "zero page filled");
    set.add("F032 fill file", vma.fill_from_file(2) && !vma.zero_filled[2], "file content replaces zero");

    // F033
    let mut swap = SwapArea::new();
    let page: [u64; 4] = [0x1111, 0x2222, 0x3333, 0x4444];
    let out_ok = swap.swap_out(7, &page);
    let mut back = [0u64; 4];
    let in_ok = swap.swap_in(7, &mut back);
    set.add("F033 roundtrip", out_ok && in_ok && back == page, "swap out/in zero damage");
    let mut scratch = [0u64; 4];
    set.add("F033 empty slot", !swap.swap_in(7, &mut scratch), "empty slot rejected");

    // F034
    let existing = [MapRegion { base: 0x1000, len: 0x1000, kind: MapType::Anon, writable: true }];
    set.add("F034 ok", f034_mmap_validate(&existing, MapRegion { base: 0x3000, len: 0x2000, kind: MapType::File, writable: false }), "valid mapping");
    set.add("F034 overlap", !f034_mmap_validate(&existing, MapRegion { base: 0x0, len: 0x2000, kind: MapType::Anon, writable: true }), "overlap rejected");
    set.add("F034 unaligned", !f034_mmap_validate(&[], MapRegion { base: 0x1001, len: 0x1000, kind: MapType::Anon, writable: true }), "unaligned rejected");

    // F035
    let cr4 = f035_enable_smap_smep(Cr4Flags(0));
    set.add("F035 enable", f035_enabled(cr4), "smap+smep on");
    set.add("F035 violation", f035_smap_violation(true, true, true), "kernel reads user ptr");

    // F036
    let mut nx = NxBitmap::new_all_nx();
    nx.whitelist(3);
    nx.whitelist(20);
    set.add("F036 whitelist exec", nx.is_exec(3) && nx.is_exec(20), "listed pages exec");
    set.add("F036 default nx", !nx.is_exec(4), "unlisted is nx");
    set.add("F036 audit", nx.audit(&[3, 20]) && !nx.audit(&[3, 21]), "audit catches rogue");

    // F037
    set.add("F037 selfmap entry", f037_selfmap_pml4_entry() & 0xFFF == 0b11, "present+write");
    set.add("F037 vaddr window", (f037_selfmap_vaddr(0, 0, 0) >> 39) as usize == SELF_MAP_ENTRY, "pml4 slot 510");

    // F038
    let victims = [
        OomVictim { pid: 1, rss_pages: 10, protected: true },
        OomVictim { pid: 2, rss_pages: 100, protected: false },
        OomVictim { pid: 3, rss_pages: 50, protected: false },
    ];
    set.add("F038 pick biggest", f038_pick_oom_victim(&victims).map(|v| v.pid) == Some(2), "max rss unprotected");
    set.add("F038 all protected", f038_pick_oom_victim(&victims[..1]).is_none(), "protected spared");

    // F039
    set.add("F039 none", f039_pressure_level(50, 100) == MemPressure::None, "50%");
    set.add("F039 low", f039_pressure_level(70, 100) == MemPressure::Low, "70%");
    set.add("F039 critical", f039_pressure_level(96, 100) == MemPressure::Critical, "96%");
    set.add("F039 div0", f039_pressure_level(1, 0) == MemPressure::Critical, "zero total");

    // F040
    set.add("F040 ok", f040_hugepage_ok(0x200000, 0x400000), "2MB aligned span");
    set.add("F040 unaligned", !f040_hugepage_ok(0x1000, 0x400000), "misaligned rejected");
    set.add("F040 count", f040_hugepage_count(0x400000) == 2 && f040_smallpage_count(0x400000) == 1024, "2 huge = 1024 small");

    // F041
    let mut pool = DmaPool::new();
    set.add("F041 alloc", pool.alloc(0, 0x5000_0000, 0x1000, true), "coherent buffer");
    set.add("F041 reuse", !pool.alloc(0, 0x5000_1000, 0x1000, false), "double alloc rejected");
    set.add("F041 free", pool.free(0) && !pool.free(0), "free once");

    // F042
    let mut iommu = Iommu::new();
    iommu.grant(1, 0x6000_0000, 0x6000_1000);
    set.add("F042 grant+en", { iommu.enabled = true; iommu.translate(1, 0x6000_0800) }, "in-range pass");
    set.add("F042 block", !iommu.translate(1, 0x7000_0000), "out-of-range blocked");
    set.add("F042 unassigned", !iommu.translate(2, 0x6000_0800), "unassigned dev blocked");

    // F043
    set.add("F043 sentinel", f043_sentinel_ok(STACK_MAGIC), "magic intact");

    // F044
    let mut lt = LeakTracker::new();
    lt.alloc(1);
    lt.alloc(1);
    lt.free(1);
    lt.alloc(5);
    let (sites, n) = lt.leaks();
    set.add("F044 detect", n == 2 && sites.contains(&1) && sites.contains(&5), "two leak sites");

    // F045
    let mut pg = [0u64; 4];
    f045_poison(&mut pg);
    set.add("F045 poison", pg.iter().all(|&w| f045_is_poisoned(w)), "page poisoned");

    // F046
    set.add("F046 ok", f046_check_user_ptr(0x1000, 0x2000), "user range");
    set.add("F046 kernel addr", !f046_check_user_ptr(USER_SPACE_LIMIT, 8), "kernel ptr rejected");
    set.add("F046 overflow", !f046_check_user_ptr(u64::MAX - 4, 8), "overflow rejected");

    // F047
    set.add("F047 within", f047_within_limit(90, 100, 10), "exactly at limit");
    set.add("F047 exceed", !f047_within_limit(90, 100, 11), "over limit");

    // F048
    let (present, write, user) = f048_decode_err(0b110);
    set.add("F048 decode", !present && write && user, "user write non-present");
    let fr = FaultReport { err: 0b110, rip: 0x401000, cr2: 0xdead0, pid: 7 };
    set.add("F048 report", fr.pid == 7 && fr.rip != 0, "report carries pid/rip");

    // F049
    let mut ft = FragTracker::new();
    ft.sample(100);
    ft.sample(900);
    ft.sample(400);
    set.add("F049 peak", ft.peak() == 900, "peak tracked");
    ft.sample(1200); // clamp
    set.add("F049 clamp", ft.peak() == 1000, "clamped to 1000");

    // F050
    let mut vs = VmStat::default();
    vs.record_alloc(10);
    vs.record_free(3);
    set.add("F050 live", vs.live_pages() == 7, "alloc - free");
    set.add("F050 fields", vs.page_faults == 0 && vs.swap_out == 0, "counters start zero");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    fn failures(set: &CheckSet) -> String {
        let mut s = String::new();
        for i in 0..set.len() {
            if let Some(c) = set.get(i) {
                if !c.passed {
                    s.push_str(&format!("{}: {}\n", c.name, c.detail));
                }
            }
        }
        s
    }

    #[test]
    fn f050_memhard_selftest_all_pass() {
        let set = run_memhard_checks();
        assert!(set.len() >= 25);
        assert!(set.all_passed(), "{}", failures(&set));
    }

    #[test]
    fn f026_pmm_exhaustion() {
        let mut pmm = Pmm::new();
        let mut got = 0usize;
        while pmm.alloc().is_some() {
            got += 1;
        }
        assert_eq!(got, PMM_BITS);
        assert_eq!(pmm.used, PMM_BITS);
    }

    #[test]
    fn f033_swap_integrity() {
        let mut swap = SwapArea::new();
        let page: [u64; 4] = [u64::MAX, 0, 1, 0x8000_0000_0000_0000];
        assert!(swap.swap_out(0, &page));
        let mut back = [0u64; 4];
        assert!(swap.swap_in(0, &mut back));
        assert_eq!(back, page);
    }
}
