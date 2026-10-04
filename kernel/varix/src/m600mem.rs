//! m600mem — VARIX-M600 AI-02 内存与算力域 (F026~F050)
//!
//! 页粒度画像/工作集预言机/压缩交换总线/大页自适应/内存配额市场/
//! NUMA 感知分配/零拷贝总线/泄漏哨兵/碎片整形器/内存热图直播/
//! OOM 陪审团/缓存亲和调度/算力令牌池/异构卸载网关/向量加速接口/
//! 能效感知调度/实时池隔离/内存快照差异器/泳道预算/微堆复用器/
//! 页表走查优化/共享内存账本/内存防火墙/压力自适应回退/算力年报。
//!
//! 硬约束：no_std / 无 alloc / 无浮点（全部 permille/定点）/ 纯逻辑。

use crate::checks::CheckSet;

// ===========================================================================
// F026 — 页粒度画像：同一请求按 4K/2M 两种粒度的页数画像
// ===========================================================================

pub const MEM_PAGE_SIZE: u32 = 4096;
pub const MEM_HUGE_PAGE_SIZE: u32 = 2 * 1024 * 1024;

/// 向上取整页数（const fn，手写算术，不用 Ord）。
pub const fn mem_pages_needed(size_bytes: u64, granularity: u32) -> u64 {
    (size_bytes + granularity as u64 - 1) / granularity as u64
}

/// 一份大小在两种页粒度下的画像。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MemGranularity {
    pub pages_4k: u64,
    pub pages_2m: u64,
}

pub fn mem_granularity_profile(size_bytes: u64) -> MemGranularity {
    MemGranularity {
        pages_4k: mem_pages_needed(size_bytes, MEM_PAGE_SIZE),
        pages_2m: mem_pages_needed(size_bytes, MEM_HUGE_PAGE_SIZE),
    }
}

// ===========================================================================
// F027 — 工作集预言机：最近 8 页窗口内即预测命中
// ===========================================================================

pub const WORKSET_WINDOW: usize = 8;

#[derive(Clone, Copy, Debug)]
pub struct MemWorkingSet {
    buf: [u64; WORKSET_WINDOW],
    len: usize,
    head: usize,
}

impl MemWorkingSet {
    pub const fn new() -> MemWorkingSet {
        MemWorkingSet { buf: [0; WORKSET_WINDOW], len: 0, head: 0 }
    }

    pub fn observe(&mut self, page: u64) {
        self.buf[self.head] = page;
        self.head = (self.head + 1) % WORKSET_WINDOW;
        if self.len < WORKSET_WINDOW {
            self.len += 1;
        }
    }

    /// 页落在最近窗口内 → 预言命中。
    pub fn contains(&self, page: u64) -> bool {
        let mut i = 0usize;
        while i < self.len {
            if self.buf[i] == page {
                return true;
            }
            i += 1;
        }
        false
    }

    /// 窗口内去重后的工作集大小。
    pub fn distinct(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let mut seen = false;
            let mut j = 0usize;
            while j < i {
                if self.buf[j] == self.buf[i] {
                    seen = true;
                }
                j += 1;
            }
            if !seen {
                n += 1;
            }
            i += 1;
        }
        n
    }
}

// ===========================================================================
// F028 — 压缩交换总线：换出页压缩存放，槽满拒绝并记账
// ===========================================================================

pub const MEM_SWAP_SLOTS: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MemSwapSlot {
    pub orig_pages: u32,
    pub comp_pages: u32,
}

/// 压缩节省比 permille。
pub fn mem_swap_saved_permille(orig: u32, comp: u32) -> u32 {
    if orig == 0 || comp > orig {
        0
    } else {
        (orig - comp) * 1000 / orig
    }
}

#[derive(Clone, Copy, Debug)]
pub struct MemSwapBus {
    pub slots: [Option<MemSwapSlot>; MEM_SWAP_SLOTS],
    pub used: usize,
    pub dropped: u32,
}

impl MemSwapBus {
    pub const fn new() -> MemSwapBus {
        MemSwapBus { slots: [None; MEM_SWAP_SLOTS], used: 0, dropped: 0 }
    }

    /// 入槽：槽满拒绝（记账 dropped），压缩后反而变大拒绝。
    pub fn push(&mut self, orig_pages: u32, comp_pages: u32) -> bool {
        if orig_pages == 0 || comp_pages > orig_pages {
            return false;
        }
        if self.used >= MEM_SWAP_SLOTS {
            self.dropped += 1;
            return false;
        }
        let mut i = 0usize;
        while i < MEM_SWAP_SLOTS {
            if self.slots[i].is_none() {
                self.slots[i] = Some(MemSwapSlot { orig_pages, comp_pages });
                self.used += 1;
                return true;
            }
            i += 1;
        }
        false
    }

    /// 回收一个槽。
    pub fn reclaim(&mut self, index: usize) -> bool {
        if index < MEM_SWAP_SLOTS && self.slots[index].is_some() {
            self.slots[index] = None;
            self.used -= 1;
            true
        } else {
            false
        }
    }
}

// ===========================================================================
// F029 — 大页自适应：连续缺页达 16 次即提升大页并归零重计
// ===========================================================================

pub const HUGEPAGE_FAULT_THRESHOLD: u32 = 16;

#[derive(Clone, Copy, Debug, Default)]
pub struct MemHugeTracker {
    pub faults: u32,
    pub promotions: u32,
}

impl MemHugeTracker {
    /// 记一次缺页；达到阈值返回 true（提升发生，计数归零）。
    pub fn on_fault(&mut self) -> bool {
        self.faults += 1;
        if self.faults >= HUGEPAGE_FAULT_THRESHOLD {
            self.faults = 0;
            self.promotions += 1;
            true
        } else {
            false
        }
    }
}

// ===========================================================================
// F030 — 内存配额市场：按用量定价，超发自然受限
// ===========================================================================

#[derive(Clone, Copy, Debug)]
pub struct MemQuotaMarket {
    pub limit: u32,
    pub used: u32,
}

impl MemQuotaMarket {
    pub const fn new(limit: u32) -> MemQuotaMarket {
        MemQuotaMarket { limit, used: 0 }
    }

    /// 申请 n 页，返回实际批准数（剩余不足按剩余批）。
    pub fn claim(&mut self, want: u32) -> u32 {
        let free = self.limit - self.used;
        let granted = if want > free { free } else { want };
        self.used += granted;
        granted
    }

    /// 占用比即价格 permille。
    pub fn price_permille(&self) -> u32 {
        if self.limit == 0 {
            0
        } else {
            self.used * 1000 / self.limit
        }
    }
}

// ===========================================================================
// F031 — NUMA 感知分配：距离最近者优先，同距看余量，全空回退
// ===========================================================================

pub const NUMA_NONE: u8 = 255;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NumaNode {
    pub id: u8,
    pub free_pages: u32,
    pub dist_to_local: u8,
}

/// 在有页的节点里挑 (距离升序, 余量降序) 最优者；全部无页返回 NUMA_NONE。
pub fn numa_pick(nodes: &[NumaNode]) -> u8 {
    if nodes.is_empty() {
        return NUMA_NONE;
    }
    let mut best: Option<usize> = None;
    let mut i = 0usize;
    while i < nodes.len() {
        if nodes[i].free_pages > 0 {
            match best {
                None => best = Some(i),
                Some(b) => {
                    let take = nodes[i].dist_to_local < nodes[b].dist_to_local
                        || (nodes[i].dist_to_local == nodes[b].dist_to_local
                            && nodes[i].free_pages > nodes[b].free_pages);
                    if take {
                        best = Some(i);
                    }
                }
            }
        }
        i += 1;
    }
    match best {
        Some(b) => nodes[b].id,
        None => NUMA_NONE,
    }
}

// ===========================================================================
// F032 — 零拷贝总线：页对齐整页传输 + 引用计数放行
// ===========================================================================

pub const MEM_ZC_ALIGN: u64 = 4096;

/// 零拷贝三条件：非空、地址页对齐、长度整页。
pub fn zero_copy_ok(addr: u64, len: u32) -> bool {
    len > 0 && addr % MEM_ZC_ALIGN == 0 && len % MEM_PAGE_SIZE == 0
}

#[derive(Clone, Copy, Debug, Default)]
pub struct MemZeroRef {
    pub refs: u32,
}

impl MemZeroRef {
    pub const REF_CAP: u32 = 4096;

    /// 挂一路引用；超过引用上限拒绝。
    pub fn acquire(&mut self) -> bool {
        if self.refs >= Self::REF_CAP {
            return false;
        }
        self.refs += 1;
        true
    }

    /// 摘一路引用；返回 true 表示这是最后一路（可以真正释放）。
    pub fn release(&mut self) -> bool {
        if self.refs == 0 {
            return false;
        }
        self.refs -= 1;
        self.refs == 0
    }
}

// ===========================================================================
// F033 — 泄漏哨兵：存活计数越线即嫌疑
// ===========================================================================

pub const LEAK_SUSPECT_LIVE: u32 = 64;

#[derive(Clone, Copy, Debug, Default)]
pub struct LeakSentinel {
    pub allocs: u64,
    pub frees: u64,
    pub live: u32,
    pub high_water: u32,
}

impl LeakSentinel {
    pub fn on_alloc(&mut self) {
        self.allocs += 1;
        self.live += 1;
        if self.live > self.high_water {
            self.high_water = self.live;
        }
    }

    pub fn on_free(&mut self) {
        if self.live > 0 {
            self.live -= 1;
            self.frees += 1;
        }
    }

    pub fn suspicious(&self) -> bool {
        self.live >= LEAK_SUSPECT_LIVE
    }
}

// ===========================================================================
// F034 — 碎片整形器：请求圆整到 2 的幂 + 碎片率度量
// ===========================================================================

/// 圆整到不小于 n 的最小 2 的幂（const fn，手写循环）。
pub const fn mem_round_pow2(n: u32) -> u32 {
    if n <= 1 {
        return 1;
    }
    let mut p = 1u32;
    while p < n {
        p *= 2;
    }
    p
}

/// 碎片率 = (total - usable) / total，permille。
pub fn frag_permille(usable: u32, total: u32) -> u32 {
    if total == 0 || usable > total {
        0
    } else {
        (total - usable) * 1000 / total
    }
}

// ===========================================================================
// F035 — 内存热图直播：按地址段 4 桶直播触摸热度
// ===========================================================================

pub const MEM_HEAT_BUCKETS: usize = 4;
/// 每桶覆盖 1024 个地址单位。
pub const MEM_HEAT_BUCKET_SPAN: u64 = 1024;

pub fn mem_heat_bucket(addr: u64) -> usize {
    let b = (addr / MEM_HEAT_BUCKET_SPAN) as usize;
    if b >= MEM_HEAT_BUCKETS {
        MEM_HEAT_BUCKETS - 1
    } else {
        b
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct MemHeatmap {
    pub buckets: [u32; MEM_HEAT_BUCKETS],
    pub touches: u64,
}

impl MemHeatmap {
    pub fn touch(&mut self, addr: u64) {
        let b = mem_heat_bucket(addr);
        self.buckets[b] += 1;
        self.touches += 1;
    }

    /// 最热桶（并列取编号小者；全空回 0）。
    pub fn hottest(&self) -> usize {
        let mut best = 0usize;
        let mut i = 1usize;
        while i < MEM_HEAT_BUCKETS {
            if self.buckets[i] > self.buckets[best] {
                best = i;
            }
            i += 1;
        }
        best
    }
}

// ===========================================================================
// F036 — OOM 陪审团：5 席多数决 + 按 RSS 选受害者
// ===========================================================================

pub const OOM_JURORS: usize = 5;
pub const OOM_KILL_MAJORITY: usize = 3;

/// kill 票 ≥ 3 即处决。
pub fn oom_verdict(kill_votes: &[bool; OOM_JURORS]) -> bool {
    let mut kills = 0usize;
    let mut i = 0usize;
    while i < OOM_JURORS {
        if kill_votes[i] {
            kills += 1;
        }
        i += 1;
    }
    kills >= OOM_KILL_MAJORITY
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MemVictim {
    pub pid: u32,
    pub rss_pages: u64,
}

/// 挑 RSS 更大者作受害者；并列取先者。
pub fn oom_pick(a: MemVictim, b: MemVictim) -> u32 {
    if a.rss_pages >= b.rss_pages {
        a.pid
    } else {
        b.pid
    }
}

// ===========================================================================
// F037 — 缓存亲和调度：迁移收益必须盖过迁移惩罚
// ===========================================================================

pub const MIGRATE_PENALTY_PERMILLE: u32 = 100;
pub const MIGRATE_PENALTY_CAP_PERMILLE: u32 = 1000;

/// 迁移惩罚随次数累加，封顶 1000‰。
pub fn migrate_penalty_permille(migrations: u32) -> u32 {
    let p = migrations.saturating_mul(MIGRATE_PENALTY_PERMILLE);
    if p > MIGRATE_PENALTY_CAP_PERMILLE {
        MIGRATE_PENALTY_CAP_PERMILLE
    } else {
        p
    }
}

/// 迁移收益严格大于当前累计惩罚才值得迁。
pub fn should_migrate(benefit_permille: u32, migrations: u32) -> bool {
    benefit_permille > migrate_penalty_permille(migrations)
}

// ===========================================================================
// F038 — 算力令牌池：满容量 64，取用与回填都封顶
// ===========================================================================

pub const TOKEN_POOL_CAP: u32 = 64;

#[derive(Clone, Copy, Debug)]
pub struct MemTokenPool {
    pub tokens: u32,
}

impl MemTokenPool {
    pub const fn new() -> MemTokenPool {
        MemTokenPool { tokens: TOKEN_POOL_CAP }
    }

    /// 申请 n 个令牌，返回实际发放数。
    pub fn take(&mut self, want: u32) -> u32 {
        let granted = if want > self.tokens { self.tokens } else { want };
        self.tokens -= granted;
        granted
    }

    /// 回填令牌，封顶 TOKEN_POOL_CAP。
    pub fn refill(&mut self, n: u32) -> u32 {
        let free = TOKEN_POOL_CAP - self.tokens;
        let added = if n > free { free } else { n };
        self.tokens += added;
        added
    }
}

// ===========================================================================
// F039 — 异构卸载网关：工作量够大且加速器空闲才卸载
// ===========================================================================

pub const OFFLOAD_MIN_WORK: u32 = 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OffloadTarget {
    Accel,
    Cpu,
}

pub fn offload_target(work_items: u32, accel_free: bool) -> OffloadTarget {
    if work_items >= OFFLOAD_MIN_WORK && accel_free {
        OffloadTarget::Accel
    } else {
        OffloadTarget::Cpu
    }
}

// ===========================================================================
// F040 — 向量加速接口：16 通道批处理与有效占用率
// ===========================================================================

pub const VEC_LANES: u32 = 16;

/// 批次数 = ceil(n / 16)。
pub fn vec_batches(n: u32) -> u32 {
    (n + VEC_LANES - 1) / VEC_LANES
}

/// 不足一整批的尾量。
pub fn vec_tail(n: u32) -> u32 {
    n % VEC_LANES
}

/// 实际利用率 = n / (批数 * 16)，permille。
pub fn vec_efficiency_permille(n: u32) -> u32 {
    let total_slots = vec_batches(n) * VEC_LANES;
    if n == 0 || total_slots == 0 {
        0
    } else {
        n * 1000 / total_slots
    }
}

// ===========================================================================
// F041 — 能效感知调度：按负载挑最小够用的频档，能耗按频率平方计
// ===========================================================================

pub const PSTEPS_PERMILLE: [u32; 4] = [250, 500, 750, 1000];

/// 挑最小能满足负载的频档；都满足不了顶格 1000‰。
pub const fn pick_pstate(work_permille: u32) -> u32 {
    let mut i = 0usize;
    while i < PSTEPS_PERMILLE.len() {
        if PSTEPS_PERMILLE[i] >= work_permille {
            return PSTEPS_PERMILLE[i];
        }
        i += 1;
    }
    1000
}

/// 能耗 ∝ f²，定点：permille² / 1000。
pub const fn pstate_energy_permille(step_permille: u32) -> u32 {
    step_permille * step_permille / 1000
}

// ===========================================================================
// F042 — 实时池隔离：RT 保留页 RT 专用，非 RT 一律拒收
// ===========================================================================

#[derive(Clone, Copy, Debug)]
pub struct RtPool {
    pub reserved: u32,
    pub used: u32,
}

impl RtPool {
    pub const fn new(reserved: u32) -> RtPool {
        RtPool { reserved, used: 0 }
    }

    /// RT 侧分配：只在保留额度内。
    pub fn rt_alloc(&mut self) -> bool {
        if self.used < self.reserved {
            self.used += 1;
            true
        } else {
            false
        }
    }

    pub fn rt_free(&mut self) -> bool {
        if self.used > 0 {
            self.used -= 1;
            true
        } else {
            false
        }
    }

    /// 非 RT 请求：永远 0（隔离铁律）。
    pub fn nonrt_admit(&self, want: u32) -> u32 {
        let _ = want;
        0
    }
}

// ===========================================================================
// F043 — 内存快照差异器：等长快照逐项比对
// ===========================================================================

/// 等长返回变化项数；长度不等返回 None（快照非法）。
pub fn snap_diff(a: &[u64], b: &[u64]) -> Option<usize> {
    if a.len() != b.len() {
        return None;
    }
    let mut changed = 0usize;
    let mut i = 0usize;
    while i < a.len() {
        if a[i] != b[i] {
            changed += 1;
        }
        i += 1;
    }
    Some(changed)
}

// ===========================================================================
// F044 — 泳道预算：每泳道限额，超限拒绝且账不动
// ===========================================================================

pub const MEM_LANES: usize = 4;

#[derive(Clone, Copy, Debug)]
pub struct LaneBudget {
    pub budget: [u32; MEM_LANES],
    pub spent: [u32; MEM_LANES],
}

impl LaneBudget {
    pub const fn new(budget: [u32; MEM_LANES]) -> LaneBudget {
        LaneBudget { budget, spent: [0; MEM_LANES] }
    }

    /// 记账一笔；超额拒绝且不产生任何改动。
    pub fn charge(&mut self, lane: usize, n: u32) -> bool {
        if lane >= MEM_LANES {
            return false;
        }
        if self.spent[lane] + n > self.budget[lane] {
            return false;
        }
        self.spent[lane] += n;
        true
    }

    pub fn remaining(&self, lane: usize) -> u32 {
        if lane >= MEM_LANES {
            0
        } else {
            self.budget[lane] - self.spent[lane]
        }
    }
}

// ===========================================================================
// F045 — 微堆复用器：请求按尺寸归入固定档位
// ===========================================================================

pub const HEAP_CLASSES: [u32; 4] = [16, 32, 64, 128];

/// 0 与超档返回 None；其余归入最小可容纳档。
pub const fn mem_size_class_of(n: u32) -> Option<u32> {
    if n == 0 {
        None
    } else if n <= HEAP_CLASSES[0] {
        Some(HEAP_CLASSES[0])
    } else if n <= HEAP_CLASSES[1] {
        Some(HEAP_CLASSES[1])
    } else if n <= HEAP_CLASSES[2] {
        Some(HEAP_CLASSES[2])
    } else if n <= HEAP_CLASSES[3] {
        Some(HEAP_CLASSES[3])
    } else {
        None
    }
}

// ===========================================================================
// F046 — 页表走查优化：映射粒度决定走查深度 + TLB 命中率
// ===========================================================================

pub const PTW_LEVELS_4K: u32 = 4;
pub const PTW_LEVELS_2M: u32 = 3;
pub const PTW_LEVELS_1G: u32 = 2;

pub fn walk_levels(page_size: u64) -> u32 {
    if page_size >= 1 << 30 {
        PTW_LEVELS_1G
    } else if page_size >= 1 << 21 {
        PTW_LEVELS_2M
    } else {
        PTW_LEVELS_4K
    }
}

pub fn tlb_hit_permille(hits: u32, misses: u32) -> u32 {
    let total = hits + misses;
    if total == 0 {
        0
    } else {
        hits * 1000 / total
    }
}

// ===========================================================================
// F047 — 共享内存账本：定容段表，注册去重，挂接计数
// ===========================================================================

pub const MEM_SHM_SEGS: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MemShmSeg {
    pub key: u32,
    pub refs: u32,
}

#[derive(Clone, Copy, Debug)]
pub struct MemShmLedger {
    pub segs: [Option<MemShmSeg>; MEM_SHM_SEGS],
    pub count: usize,
    pub dup_rejected: u32,
}

impl MemShmLedger {
    pub const fn new() -> MemShmLedger {
        MemShmLedger { segs: [None; MEM_SHM_SEGS], count: 0, dup_rejected: 0 }
    }

    fn find(&self, key: u32) -> Option<usize> {
        let mut i = 0usize;
        while i < MEM_SHM_SEGS {
            if let Some(s) = self.segs[i] {
                if s.key == key {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 注册新段：同 key 去重拒绝（记账），表满拒绝。
    pub fn register(&mut self, key: u32) -> bool {
        if self.find(key).is_some() {
            self.dup_rejected += 1;
            return false;
        }
        let mut i = 0usize;
        while i < MEM_SHM_SEGS {
            if self.segs[i].is_none() {
                self.segs[i] = Some(MemShmSeg { key, refs: 0 });
                self.count += 1;
                return true;
            }
            i += 1;
        }
        false
    }

    /// 挂接一路引用；段不存在返回 false。
    pub fn attach(&mut self, key: u32) -> bool {
        match self.find(key) {
            Some(i) => {
                if let Some(s) = self.segs[i].as_mut() {
                    s.refs += 1;
                }
                true
            }
            None => false,
        }
    }

    /// 摘一路引用。
    pub fn detach(&mut self, key: u32) -> bool {
        match self.find(key) {
            Some(i) => {
                if let Some(s) = self.segs[i].as_mut() {
                    if s.refs > 0 {
                        s.refs -= 1;
                        return true;
                    }
                }
                false
            }
            None => false,
        }
    }

    pub fn seg_refs(&self, key: u32) -> Option<u32> {
        match self.find(key) {
            Some(i) => self.segs[i].map(|s| s.refs),
            None => None,
        }
    }
}

// ===========================================================================
// F048 — 内存防火墙：守卫区间，重叠拒绝，区间内访问封锁
// ===========================================================================

pub const MEM_GUARD_MAX: usize = 4;

#[derive(Clone, Copy, Debug)]
pub struct MemFirewall {
    pub lo: [u64; MEM_GUARD_MAX],
    pub hi: [u64; MEM_GUARD_MAX],
    pub count: usize,
    pub overlaps: u32,
}

impl MemFirewall {
    pub const fn new() -> MemFirewall {
        MemFirewall { lo: [0; MEM_GUARD_MAX], hi: [0; MEM_GUARD_MAX], count: 0, overlaps: 0 }
    }

    fn overlaps(lo1: u64, hi1: u64, lo2: u64, hi2: u64) -> bool {
        lo1 < hi2 && lo2 < hi1
    }

    /// 新增守卫：区间非法、表满、与既有区间重叠都拒绝。
    pub fn add_guard(&mut self, lo: u64, hi: u64) -> bool {
        if hi <= lo {
            return false;
        }
        let mut i = 0usize;
        while i < self.count {
            if Self::overlaps(lo, hi, self.lo[i], self.hi[i]) {
                self.overlaps += 1;
                return false;
            }
            i += 1;
        }
        if self.count >= MEM_GUARD_MAX {
            return false;
        }
        self.lo[self.count] = lo;
        self.hi[self.count] = hi;
        self.count += 1;
        true
    }

    /// 地址落在任一守卫区间内即被封锁。
    pub fn blocked(&self, addr: u64) -> bool {
        let mut i = 0usize;
        while i < self.count {
            if addr >= self.lo[i] && addr < self.hi[i] {
                return true;
            }
            i += 1;
        }
        false
    }
}

// ===========================================================================
// F049 — 压力自适应回退：四级压力台阶，级越高回收越多
// ===========================================================================

pub const STAGE1_PERMILLE: u32 = 500;
pub const STAGE2_PERMILLE: u32 = 700;
pub const STAGE3_PERMILLE: u32 = 900;
/// 每升一级追加回收 150‰。
pub const STAGE_RECLAIM_STEP_PERMILLE: u32 = 150;

pub const fn fallback_stage(pressure_permille: u32) -> u32 {
    if pressure_permille >= STAGE3_PERMILLE {
        3
    } else if pressure_permille >= STAGE2_PERMILLE {
        2
    } else if pressure_permille >= STAGE1_PERMILLE {
        1
    } else {
        0
    }
}

pub const fn stage_reclaim_permille(stage: u32) -> u32 {
    stage * STAGE_RECLAIM_STEP_PERMILLE
}

// ===========================================================================
// F050 — 算力年报：章节完备性 + 年度总量账
// ===========================================================================

pub const MEM_COMPUTE_SECTIONS: [&str; 5] =
    ["granularity", "workingset", "swap", "quota", "offload"];

#[derive(Clone, Copy, Debug, Default)]
pub struct MemComputeYear {
    pub pages_profiled: u64,
    pub tokens_granted: u64,
    pub offloads: u64,
}

impl MemComputeYear {
    pub fn record_profile(&mut self, pages: u64) {
        self.pages_profiled += pages;
    }
    pub fn record_tokens(&mut self, n: u32) {
        self.tokens_granted += n as u64;
    }
    pub fn record_offload(&mut self) {
        self.offloads += 1;
    }
}

pub fn compute_report_complete(sections_filled: u32) -> bool {
    sections_filled >= MEM_COMPUTE_SECTIONS.len() as u32
}

// ===========================================================================
// 域自检
// ===========================================================================

pub fn run_m600mem_checks() -> CheckSet {
    let mut set = CheckSet::new("m600mem");

    // F026 页粒度画像
    let big = mem_granularity_profile(8 * 1024 * 1024);
    set.add(
        "F026 granularity profile",
        big.pages_4k == 2048 && big.pages_2m == 4,
        "8MiB in 4K/2M pages",
    );
    set.add(
        "F026 page ceil",
        mem_pages_needed(1, MEM_PAGE_SIZE) == 1
            && mem_pages_needed(MEM_PAGE_SIZE as u64, MEM_PAGE_SIZE) == 1
            && mem_pages_needed(MEM_PAGE_SIZE as u64 + 1, MEM_PAGE_SIZE) == 2,
        "ceil rounding",
    );
    set.add(
        "F026 huge ceil",
        mem_pages_needed(3 * MEM_HUGE_PAGE_SIZE as u64, MEM_HUGE_PAGE_SIZE) == 3,
        "huge granularity",
    );

    // F027 工作集预言机
    let mut ws = MemWorkingSet::new();
    ws.observe(10);
    ws.observe(11);
    ws.observe(10);
    let ws_distinct = ws.distinct();
    let ws_hit = ws.contains(10);
    let ws_miss = ws.contains(99);
    set.add(
        "F027 workingset dedup",
        ws_distinct == 2 && ws_hit && !ws_miss,
        "window membership",
    );
    let mut i = 1u64;
    while i <= 8 {
        ws.observe(i);
        i += 1;
    }
    let six_still = ws.contains(6);
    ws.observe(9);
    let old_gone = ws.contains(6);
    let fresh_in = ws.contains(9);
    set.add(
        "F027 workingset ring evict",
        six_still && old_gone && fresh_in && ws.distinct() == 8,
        "8-deep ring",
    );

    // F028 压缩交换总线
    let mut bus = MemSwapBus::new();
    let first_ok = bus.push(8, 2);
    let saved = mem_swap_saved_permille(8, 2);
    set.add("F028 swap saved ratio", first_ok && saved == 750, "8->2 saves 750 permille");
    let bad_push = bus.push(4, 6);
    set.add("F028 swap rejects growth", !bad_push && bus.used == 1, "comp > orig refused");
    let mut fill = 1usize;
    while fill < MEM_SWAP_SLOTS {
        bus.push(2, 1);
        fill += 1;
    }
    let used_full = bus.used;
    let ninth = bus.push(2, 1);
    let dropped_after = bus.dropped;
    set.add(
        "F028 swap slots capped",
        used_full == MEM_SWAP_SLOTS && !ninth && dropped_after == 1,
        "cap 8 with drop bookkeeping",
    );
    let reclaimed = bus.reclaim(0);
    let used_after = bus.used;
    set.add("F028 swap reclaim", reclaimed && used_after == MEM_SWAP_SLOTS - 1, "slot freed");

    // F029 大页自适应
    let mut ht = MemHugeTracker::default();
    let mut promoted_early = 0;
    let mut k = 0;
    while k < 15 {
        if ht.on_fault() {
            promoted_early += 1;
        }
        k += 1;
    }
    let faults_before = ht.faults;
    set.add(
        "F029 hugepage patience",
        promoted_early == 0 && faults_before == 15,
        "15 faults stay put",
    );
    let p16 = ht.on_fault();
    let faults_after = ht.faults;
    set.add(
        "F029 hugepage promote",
        p16 && faults_after == 0 && ht.promotions == 1,
        "16th fault promotes and resets",
    );

    // F030 内存配额市场
    let mut market = MemQuotaMarket::new(100);
    let g1 = market.claim(60);
    let price_mid = market.price_permille();
    set.add("F030 quota grant and price", g1 == 60 && price_mid == 600, "price tracks usage");
    let g2 = market.claim(50);
    let g3 = market.claim(1);
    let price_full = market.price_permille();
    set.add(
        "F030 quota scarce market",
        g2 == 40 && g3 == 0 && price_full == 1000,
        "partial grant then dry",
    );

    // F031 NUMA 感知分配
    let local = [NumaNode { id: 1, free_pages: 100, dist_to_local: 0 }];
    set.add("F031 numa local first", numa_pick(&local) == 1, "home node serves");
    let far = [
        NumaNode { id: 1, free_pages: 0, dist_to_local: 0 },
        NumaNode { id: 2, free_pages: 50, dist_to_local: 1 },
        NumaNode { id: 3, free_pages: 500, dist_to_local: 2 },
    ];
    set.add(
        "F031 numa nearest with pages",
        numa_pick(&far) == 2,
        "distance beats capacity",
    );
    let tie = [
        NumaNode { id: 2, free_pages: 50, dist_to_local: 1 },
        NumaNode { id: 3, free_pages: 500, dist_to_local: 1 },
    ];
    let starved = [
        NumaNode { id: 1, free_pages: 0, dist_to_local: 0 },
        NumaNode { id: 2, free_pages: 0, dist_to_local: 1 },
    ];
    set.add(
        "F031 numa tie and starvation",
        numa_pick(&tie) == 3 && numa_pick(&starved) == NUMA_NONE,
        "more free wins ties; all dry bails",
    );

    // F032 零拷贝总线
    set.add(
        "F032 zero copy gate",
        zero_copy_ok(4096, 8192) && !zero_copy_ok(4097, 8192) && !zero_copy_ok(4096, 100),
        "aligned full pages only",
    );
    let mut zr = MemZeroRef::default();
    zr.acquire();
    zr.acquire();
    let refs_two = zr.refs;
    let rel_partial = zr.release();
    let refs_one = zr.refs;
    set.add(
        "F032 zero copy refs hold",
        refs_two == 2 && !rel_partial && refs_one == 1,
        "release keeps page alive",
    );
    let rel_last = zr.release();
    set.add("F032 zero copy refs free", rel_last && zr.refs == 0, "last release frees");

    // F033 泄漏哨兵
    let mut sent = LeakSentinel::default();
    let mut n = 0;
    while n < 10 {
        sent.on_alloc();
        n += 1;
    }
    let mut m = 0;
    while m < 10 {
        sent.on_free();
        m += 1;
    }
    let steady_live = sent.live;
    let steady_hw = sent.high_water;
    set.add(
        "F033 leak balanced",
        steady_live == 0 && steady_hw == 10 && !sent.suspicious(),
        "alloc/free in balance",
    );
    let mut p = 0;
    while p < 70 {
        sent.on_alloc();
        p += 1;
    }
    let grown_live = sent.live;
    set.add(
        "F033 leak caught",
        grown_live == 70 && sent.suspicious() && sent.high_water == 70,
        "live above 64 flags",
    );
    let mut sent2 = LeakSentinel::default();
    let mut q = 0;
    while q < 65 {
        sent2.on_alloc();
        q += 1;
    }
    sent2.on_free();
    sent2.on_free();
    let boundary_live = sent2.live;
    set.add("F033 leak boundary", boundary_live == 63 && !sent2.suspicious(), "63 lives below line");

    // F034 碎片整形器
    set.add(
        "F034 pow2 rounding",
        mem_round_pow2(1) == 1 && mem_round_pow2(5) == 8 && mem_round_pow2(8) == 8
            && mem_round_pow2(9) == 16,
        "smallest fitting power",
    );
    set.add(
        "F034 frag permille",
        frag_permille(700, 1000) == 300 && frag_permille(1000, 1000) == 0,
        "hole ratio",
    );

    // F035 内存热图直播
    let mut heat = MemHeatmap::default();
    heat.touch(0);
    heat.touch(100);
    heat.touch(900);
    heat.touch(1000);
    heat.touch(4000);
    heat.touch(4096);
    let b = heat.buckets;
    let hot1 = heat.hottest();
    set.add(
        "F035 heat buckets",
        b[0] == 4 && b[3] == 2 && hot1 == 0 && heat.touches == 6,
        "addr spans mapped",
    );
    heat.touch(4000);
    heat.touch(4000);
    heat.touch(4000);
    let hot2 = heat.hottest();
    let b3 = heat.buckets[3];
    set.add("F035 heat reshifts", hot2 == 3 && b3 == 5, "hotspot moves live");

    // F036 OOM 陪审团
    let kill3 = [true, true, true, false, false];
    let kill2 = [true, true, false, false, false];
    set.add(
        "F036 oom majority",
        oom_verdict(&kill3) && !oom_verdict(&kill2),
        "3 of 5 required",
    );
    let v1 = MemVictim { pid: 1, rss_pages: 900 };
    let v2 = MemVictim { pid: 2, rss_pages: 100 };
    let v7 = MemVictim { pid: 7, rss_pages: 50 };
    let v8 = MemVictim { pid: 8, rss_pages: 50 };
    set.add(
        "F036 oom victim pick",
        oom_pick(v1, v2) == 1 && oom_pick(v2, v1) == 1 && oom_pick(v7, v8) == 7,
        "biggest rss, ties to first",
    );

    // F037 缓存亲和调度
    set.add(
        "F037 migrate penalty curve",
        migrate_penalty_permille(0) == 0 && migrate_penalty_permille(1) == 100
            && migrate_penalty_permille(15) == 1000,
        "grows then capped",
    );
    set.add(
        "F037 migrate decision",
        should_migrate(150, 1) && !should_migrate(50, 1) && !should_migrate(999, 15),
        "benefit must beat penalty",
    );

    // F038 算力令牌池
    let mut pool = MemTokenPool::new();
    let granted = pool.take(100);
    let tokens_after_take = pool.tokens;
    set.add(
        "F038 token drain",
        granted == 64 && tokens_after_take == 0,
        "cap-limited grant",
    );
    let added_small = pool.refill(10);
    let tokens_mid = pool.tokens;
    let added_big = pool.refill(100);
    let tokens_end = pool.tokens;
    set.add(
        "F038 token refill cap",
        added_small == 10 && tokens_mid == 10 && added_big == 54 && tokens_end == TOKEN_POOL_CAP,
        "refill clamped to cap",
    );

    // F039 异构卸载网关
    set.add(
        "F039 offload gate",
        offload_target(16, true) == OffloadTarget::Cpu
            && offload_target(64, true) == OffloadTarget::Accel
            && offload_target(64, false) == OffloadTarget::Cpu,
        "big work and idle accel",
    );
    set.add(
        "F039 offload boundary",
        offload_target(OFFLOAD_MIN_WORK, true) == OffloadTarget::Accel,
        "exact threshold offloads",
    );

    // F040 向量加速接口
    set.add(
        "F040 vec batches",
        vec_batches(40) == 3 && vec_tail(40) == 8 && vec_batches(16) == 1 && vec_tail(16) == 0,
        "16 lanes per batch",
    );
    set.add(
        "F040 vec efficiency",
        vec_efficiency_permille(40) == 833 && vec_efficiency_permille(16) == 1000
            && vec_efficiency_permille(0) == 0,
        "utilization permille",
    );

    // F041 能效感知调度
    set.add(
        "F041 pstate pick",
        pick_pstate(100) == 250 && pick_pstate(300) == 500 && pick_pstate(1000) == 1000
            && pick_pstate(999) == 1000,
        "smallest sufficient step",
    );
    set.add(
        "F041 pstate energy quadratic",
        pstate_energy_permille(1000) == 1000 && pstate_energy_permille(500) == 250
            && pstate_energy_permille(750) == 562 && pstate_energy_permille(250) == 62,
        "f squared in fixed point",
    );

    // F042 实时池隔离
    let mut rtp = RtPool::new(4);
    let a1 = rtp.rt_alloc();
    let a2 = rtp.rt_alloc();
    let a3 = rtp.rt_alloc();
    let a4 = rtp.rt_alloc();
    let a5 = rtp.rt_alloc();
    let rtp_used = rtp.used;
    set.add(
        "F042 rt pool capped",
        a1 && a2 && a3 && a4 && !a5 && rtp_used == 4,
        "reserved quota exact",
    );
    let nonrt = rtp.nonrt_admit(2);
    let freed = rtp.rt_free();
    let rtp_after = rtp.used;
    set.add(
        "F042 rt pool isolation",
        nonrt == 0 && freed && rtp_after == 3,
        "non-rt rejected, rt freed",
    );

    // F043 内存快照差异器
    let base = [1u64, 2, 3, 4];
    let drifted = [1u64, 9, 3, 9];
    let same = [1u64, 2, 3, 4];
    set.add(
        "F043 snap diff counts",
        snap_diff(&base, &drifted) == Some(2) && snap_diff(&base, &same) == Some(0),
        "changed entries tallied",
    );
    set.add(
        "F043 snap length guard",
        snap_diff(&base, &[1, 2, 3]).is_none(),
        "unequal snapshots illegal",
    );

    // F044 泳道预算
    let mut lb = LaneBudget::new([10, 20, 30, 40]);
    let ok1 = lb.charge(0, 6);
    let spent1 = lb.spent[0];
    let denied = lb.charge(0, 6);
    let spent_denied = lb.spent[0];
    let ok2 = lb.charge(0, 4);
    let spent2 = lb.spent[0];
    set.add(
        "F044 lane budget overdraft",
        ok1 && spent1 == 6 && !denied && spent_denied == 6 && ok2 && spent2 == 10,
        "denied charge leaves ledger intact",
    );
    set.add(
        "F044 lane budget remaining",
        lb.remaining(0) == 0 && lb.remaining(1) == 20 && lb.remaining(9) == 0,
        "rest untouched",
    );

    // F045 微堆复用器
    set.add(
        "F045 size classes",
        mem_size_class_of(1) == Some(16) && mem_size_class_of(17) == Some(32)
            && mem_size_class_of(128) == Some(128),
        "smallest fitting class",
    );
    set.add(
        "F045 size class rejects",
        mem_size_class_of(0).is_none() && mem_size_class_of(129).is_none(),
        "zero and oversize refused",
    );

    // F046 页表走查优化
    set.add(
        "F046 walk levels",
        walk_levels(4096) == 4 && walk_levels(1 << 21) == 3 && walk_levels(1 << 30) == 2,
        "bigger pages fewer hops",
    );
    set.add(
        "F046 tlb hit rate",
        tlb_hit_permille(90, 10) == 900 && tlb_hit_permille(0, 0) == 0,
        "permille safe",
    );

    // F047 共享内存账本
    let mut shm = MemShmLedger::new();
    let reg1 = shm.register(7);
    let reg_dup = shm.register(7);
    let dup_after = shm.dup_rejected;
    set.add(
        "F047 shm register dedupe",
        reg1 && !reg_dup && dup_after == 1,
        "same key refused once booked",
    );
    let at1 = shm.attach(7);
    let at2 = shm.attach(7);
    let refs2 = shm.seg_refs(7);
    let det = shm.detach(7);
    let refs1 = shm.seg_refs(7);
    let ghost = shm.attach(9);
    set.add(
        "F047 shm attach lineage",
        at1 && at2 && refs2 == Some(2) && det && refs1 == Some(1) && !ghost,
        "refcount per key",
    );

    // F048 内存防火墙
    let mut fw = MemFirewall::new();
    let g1 = fw.add_guard(100, 200);
    let blk_mid = fw.blocked(150);
    let blk_end = fw.blocked(200);
    let blk_before = fw.blocked(99);
    set.add(
        "F048 guard blocks range",
        g1 && blk_mid && !blk_end && !blk_before,
        "half-open interval",
    );
    let g_overlap = fw.add_guard(150, 250);
    let overlaps_after = fw.overlaps;
    let g_touch = fw.add_guard(200, 300);
    let count_mid = fw.count;
    set.add(
        "F048 guard overlap",
        !g_overlap && overlaps_after == 1 && g_touch && count_mid == 2,
        "touching edges allowed",
    );
    fw.add_guard(0, 50);
    fw.add_guard(300, 400);
    let count_full = fw.count;
    let g_full = fw.add_guard(500, 600);
    let count_still = fw.count;
    set.add(
        "F048 guard capacity",
        count_full == 4 && !g_full && count_still == 4,
        "max 4 guards",
    );

    // F049 压力自适应回退
    set.add(
        "F049 stage ladder",
        fallback_stage(499) == 0 && fallback_stage(500) == 1 && fallback_stage(699) == 1
            && fallback_stage(700) == 2 && fallback_stage(900) == 3,
        "thresholds exact",
    );
    set.add(
        "F049 stage reclaim",
        stage_reclaim_permille(0) == 0 && stage_reclaim_permille(3) == 450,
        "150 permille per stage",
    );

    // F050 算力年报
    let mut year = MemComputeYear::default();
    year.record_profile(2048);
    year.record_tokens(64);
    year.record_tokens(10);
    year.record_offload();
    year.record_offload();
    year.record_offload();
    set.add(
        "F050 compute yearbook",
        MEM_COMPUTE_SECTIONS.len() == 5 && year.pages_profiled == 2048
            && year.tokens_granted == 74 && year.offloads == 3,
        "yearly totals accounted",
    );
    set.add(
        "F050 compute report sections",
        compute_report_complete(5) && !compute_report_complete(4),
        "five sections required",
    );

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f026_granularity_profile() {
        let p = mem_granularity_profile(8 * 1024 * 1024);
        assert_eq!(p.pages_4k, 2048);
        assert_eq!(p.pages_2m, 4);
        assert_eq!(mem_pages_needed(1, MEM_PAGE_SIZE), 1);
        assert_eq!(mem_pages_needed(4097, MEM_PAGE_SIZE), 2);
    }

    #[test]
    fn f029_hugepage_promotion_cycle() {
        let mut t = MemHugeTracker::default();
        let mut promoted = 0;
        let mut i = 0;
        while i < 15 {
            if t.on_fault() {
                promoted += 1;
            }
            i += 1;
        }
        assert_eq!(promoted, 0);
        assert_eq!(t.faults, 15);
        assert!(t.on_fault());
        assert_eq!(t.faults, 0);
        assert_eq!(t.promotions, 1);
    }

    #[test]
    fn f033_leak_boundary() {
        let mut s = LeakSentinel::default();
        let mut i = 0;
        while i < 65 {
            s.on_alloc();
            i += 1;
        }
        assert!(s.suspicious());
        assert_eq!(s.high_water, 65);
        s.on_free();
        s.on_free();
        assert_eq!(s.live, 63);
        assert!(!s.suspicious());
    }

    #[test]
    fn f038_token_pool_cap() {
        let mut p = MemTokenPool::new();
        assert_eq!(p.take(100), 64);
        assert_eq!(p.tokens, 0);
        p.refill(10);
        assert_eq!(p.tokens, 10);
        p.refill(TOKEN_POOL_CAP);
        assert_eq!(p.tokens, TOKEN_POOL_CAP);
    }

    #[test]
    fn f044_lane_never_overdrafts() {
        let mut lb = LaneBudget::new([10, 10, 10, 10]);
        assert!(lb.charge(0, 6));
        assert_eq!(lb.spent[0], 6);
        assert!(!lb.charge(0, 6));
        assert_eq!(lb.spent[0], 6);
        assert!(lb.charge(0, 4));
        assert_eq!(lb.spent[0], 10);
        assert!(!lb.charge(0, 1));
    }

    #[test]
    fn m600mem_selfcheck_all_pass() {
        let set = run_m600mem_checks();
        assert!(set.len() >= 25, "got {}", set.len());
        assert!(set.all_passed());
    }
}
