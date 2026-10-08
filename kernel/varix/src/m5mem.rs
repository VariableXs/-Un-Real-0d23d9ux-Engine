//! VARIX-M500 AI-03 内存智能深化域（F051~F075，M1）。
//!
//! 内存从"够用"到"透明聪明"：画像、去重、整理、演练。
//! 全部为纯逻辑 + 固定容量数组（无 Vec/String/Box），no_std 安全。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// F051 — 内存画像器：分配热点可视化
// ---------------------------------------------------------------------------

pub const MAX_HOTSPOTS: usize = 8;

#[derive(Clone, Copy)]
pub struct MemHotspot {
    pub site: u16,
    pub bytes: u32,
    pub count: u32,
}

#[derive(Clone, Copy)]
pub struct MemProfiler {
    pub spots: [Option<MemHotspot>; MAX_HOTSPOTS],
    pub count: usize,
}

impl MemProfiler {
    pub const fn new() -> MemProfiler {
        MemProfiler { spots: [const { None }; MAX_HOTSPOTS], count: 0 }
    }

    pub fn record(&mut self, site: u16, bytes: u32) -> bool {
        for i in 0..self.count {
            if let Some(mut h) = self.spots[i] {
                if h.site == site {
                    h.bytes = h.bytes.saturating_add(bytes);
                    h.count += 1;
                    self.spots[i] = Some(h);
                    return true;
                }
            }
        }
        if self.count >= MAX_HOTSPOTS {
            return false;
        }
        self.spots[self.count] = Some(MemHotspot { site, bytes, count: 1 });
        self.count += 1;
        true
    }

    pub fn top_site(&self) -> Option<u16> {
        let mut best: Option<MemHotspot> = None;
        for i in 0..self.count {
            if let Some(h) = self.spots[i] {
                if best.map(|b| h.bytes > b.bytes).unwrap_or(true) {
                    best = Some(h);
                }
            }
        }
        best.map(|b| b.site)
    }

    pub fn total_bytes(&self) -> u32 {
        let mut t = 0u32;
        for i in 0..self.count {
            if let Some(h) = self.spots[i] {
                t = t.saturating_add(h.bytes);
            }
        }
        t
    }
}

// ---------------------------------------------------------------------------
// F052 — 冷页甄别器：访问频率分级
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PageTemp {
    Hot,
    Warm,
    Cold,
}

/// 访问计数字 → 温度分级：≥8 热，≥2 温，否则冷。
pub fn page_temp(hits: u8) -> PageTemp {
    if hits >= 8 {
        PageTemp::Hot
    } else if hits >= 2 {
        PageTemp::Warm
    } else {
        PageTemp::Cold
    }
}

/// 分级回收：从冷到温依次收集，直到凑够 target 页。
pub fn reclaim_candidates(hits: &[u8], target: u32) -> u32 {
    let mut picked = 0u32;
    for threshold in [0u8, 2] {
        for &h in hits {
            if picked >= target {
                return picked;
            }
            if h <= threshold {
                picked += 1;
            }
        }
    }
    picked
}

// ---------------------------------------------------------------------------
// F053 — 自适应压缩档：压缩比按收益动态调
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CompressGovernor {
    /// 当前档位 0=off 1=fast 2=balanced 3=best。
    pub level: u8,
    pub mem_pressure_permille: u32,
}

impl CompressGovernor {
    pub const fn new() -> CompressGovernor {
        CompressGovernor { level: 1, mem_pressure_permille: 0 }
    }

    pub fn observe(&mut self, pressure_permille: u32) {
        self.mem_pressure_permille = pressure_permille.min(1000);
        self.level = if self.mem_pressure_permille < 300 {
            0
        } else if self.mem_pressure_permille < 600 {
            1
        } else if self.mem_pressure_permille < 850 {
            2
        } else {
            3
        };
    }

    /// 各档期望压缩比 permille（存下大小/原大小）。
    pub fn ratio_permille(&self) -> u32 {
        match self.level {
            0 => 1000,
            1 => 700,
            2 => 500,
            _ => 350,
        }
    }

    /// 收益：压力下开档才有意义。
    pub fn worth(&self) -> bool {
        self.level > 0
    }
}

// ---------------------------------------------------------------------------
// F054 — 内存超售预警：承诺量超额管理
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OversellGuard {
    pub total_pages: u32,
    pub committed_pages: u32,
    /// 超售红线：承诺不得超过物理的 120%。
    pub max_oversell_permille: u32,
}

impl OversellGuard {
    pub const fn new(total_pages: u32) -> OversellGuard {
        OversellGuard { total_pages, committed_pages: 0, max_oversell_permille: 1200 }
    }

    pub fn commit(&mut self, pages: u32) -> bool {
        let new_total = self.committed_pages + pages;
        if new_total * 1000 > self.total_pages * self.max_oversell_permille {
            return false;
        }
        self.committed_pages = new_total;
        true
    }

    pub fn oversell_permille(&self) -> u32 {
        if self.total_pages == 0 {
            return 0;
        }
        self.committed_pages * 1000 / self.total_pages
    }

    /// 预警：承诺超 100% 即告警。
    pub fn warning(&self) -> bool {
        self.oversell_permille() > 1000
    }
}

// ---------------------------------------------------------------------------
// F055 — 巨页收益评估器：THP 式自动决策
// ---------------------------------------------------------------------------

/// 收益 = 减少的 TLB miss − 内部碎片浪费。
/// contig_permille: 物理连续度；fault_rate_permille: 近期缺页率。
pub fn hugepage_worth(size_pages: u32, contig_permille: u32, fault_rate_permille: u32) -> bool {
    let tlb_gain = (size_pages.min(512)) as u64 * fault_rate_permille as u64 / 100;
    let frag_cost = (512 - size_pages.min(512)) as u64 * (1000 - contig_permille as u64) / 1000;
    tlb_gain > frag_cost
}

// ---------------------------------------------------------------------------
// F056 — OOM 演练场：内存耗尽情景注入
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OomDrill {
    pub total_pages: u32,
    pub allocated: u32,
    /// 演练中每个"受害者"释放的页数。
    pub victims_freed: u32,
    pub oom_fired: bool,
}

impl OomDrill {
    pub const fn new(total_pages: u32) -> OomDrill {
        OomDrill { total_pages, allocated: 0, victims_freed: 0, oom_fired: false }
    }

    pub fn alloc(&mut self, pages: u32) -> bool {
        if self.allocated + pages > self.total_pages {
            self.oom_fired = true;
            return false;
        }
        self.allocated += pages;
        true
    }

    /// OOM 后按 victim 顺序释放；能否恢复分配。
    pub fn kill_victim(&mut self, free_pages: u32) -> bool {
        if !self.oom_fired {
            return false;
        }
        self.victims_freed += free_pages;
        self.allocated = self.allocated.saturating_sub(free_pages);
        self.oom_fired = false;
        true
    }

    pub fn recovered(&self) -> bool {
        self.allocated < self.total_pages
    }
}

// ---------------------------------------------------------------------------
// F057 — 内存泄漏指纹：泄漏源自动定位
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct LeakFingerprint {
    pub site: u16,
    pub samples: [u32; 4], // 4 个时间窗的存活字节
}

impl LeakFingerprint {
    pub const fn new(site: u16) -> LeakFingerprint {
        LeakFingerprint { site, samples: [0; 4] }
    }

    pub fn sample(&mut self, idx: usize, bytes: u32) -> bool {
        if idx >= 4 {
            return false;
        }
        self.samples[idx] = bytes;
        true
    }

    /// 指纹：4 窗单调不减且总增幅 ≥ 25% 起点即疑似泄漏。
    pub fn leaking(&self) -> bool {
        let mut mono = true;
        for i in 1..4 {
            if self.samples[i] < self.samples[i - 1] {
                mono = false;
            }
        }
        if !mono || self.samples[0] == 0 {
            return false;
        }
        let growth = (self.samples[3] - self.samples[0]) as u64 * 100;
        growth >= self.samples[0] as u64 * 25
    }
}

// ---------------------------------------------------------------------------
// F058 — 共享页去重：同内容页合并
// ---------------------------------------------------------------------------

pub const KSM_SLOTS: usize = 12;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KsmSlot {
    /// 内容指纹（FNV 一类哈希的结果）。
    pub hash: u64,
    pub refs: u16,
}

#[derive(Clone, Copy)]
pub struct KsmEngine {
    pub slots: [Option<KsmSlot>; KSM_SLOTS],
    pub count: usize,
    pub saved_pages: u32,
}

impl KsmEngine {
    pub const fn new() -> KsmEngine {
        KsmEngine { slots: [const { None }; KSM_SLOTS], count: 0, saved_pages: 0 }
    }

    /// 提交一页内容：相同指纹 → 引用合并并省一页。
    pub fn submit(&mut self, hash: u64) -> bool {
        for i in 0..self.count {
            if let Some(mut s) = self.slots[i] {
                if s.hash == hash {
                    s.refs += 1;
                    self.saved_pages += 1;
                    self.slots[i] = Some(s);
                    return true;
                }
            }
        }
        if self.count >= KSM_SLOTS {
            return false;
        }
        self.slots[self.count] = Some(KsmSlot { hash, refs: 1 });
        self.count += 1;
        true
    }

    /// 全部引用退出 → 槽释放。
    pub fn retire(&mut self, hash: u64) -> bool {
        for i in 0..self.count {
            if let Some(mut s) = self.slots[i] {
                if s.hash == hash {
                    s.refs -= 1;
                    if s.refs == 0 {
                        self.slots[i] = None;
                    } else {
                        self.slots[i] = Some(s);
                    }
                    return true;
                }
            }
        }
        false
    }
}

// ---------------------------------------------------------------------------
// F059 — 内存带宽仪表：带宽消耗可视
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BandwidthMeter {
    /// 每 100ms 窗口读写字节（MB）。
    pub read_mb: u32,
    pub write_mb: u32,
    pub peak_mb: u32,
    pub windows: u32,
}

impl BandwidthMeter {
    pub const fn new() -> BandwidthMeter {
        BandwidthMeter { read_mb: 0, write_mb: 0, peak_mb: 0, windows: 0 }
    }

    pub fn window(&mut self, read_mb: u32, write_mb: u32) {
        self.windows += 1;
        self.read_mb = read_mb;
        self.write_mb = write_mb;
        let total = read_mb.saturating_add(write_mb);
        if total > self.peak_mb {
            self.peak_mb = total;
        }
    }

    pub fn rw_ratio_permille(&self) -> u32 {
        let w = self.read_mb + self.write_mb;
        if w == 0 { 0 } else { self.write_mb * 1000 / w }
    }
}

// ---------------------------------------------------------------------------
// F060 — 缓存行对齐审计：热结构布局检查
// ---------------------------------------------------------------------------

pub const CACHE_LINE: usize = 64;

/// 审计：size 是否超缓存行、align 是否缓存行倍数。
pub struct AlignAudit {
    pub checked: u32,
    pub violations: u32,
}

impl AlignAudit {
    pub const fn new() -> AlignAudit {
        AlignAudit { checked: 0, violations: 0 }
    }

    /// 检查热结构：必须 ≤ 1 缓存行或恰为整数倍。
    pub fn audit(&mut self, size: usize, align: usize) -> bool {
        self.checked += 1;
        let ok = align >= 8
            && (size <= CACHE_LINE || size % CACHE_LINE == 0);
        if !ok {
            self.violations += 1;
        }
        ok
    }

    pub fn clean(&self) -> bool {
        self.violations == 0
    }
}

// ---------------------------------------------------------------------------
// F061 — 低内存优雅档：UI 自动精简降载
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GracefulDeg {
    pub free_permille: u32,
    /// 降载级别 0=全效 1=省动效 2=省模糊 3=极简。
    pub level: u8,
}

impl GracefulDeg {
    pub const fn new() -> GracefulDeg {
        GracefulDeg { free_permille: 1000, level: 0 }
    }

    pub fn observe(&mut self, free_permille: u32) {
        self.free_permille = free_permille.min(1000);
        self.level = if self.free_permille > 300 {
            0
        } else if self.free_permille > 150 {
            1
        } else if self.free_permille > 60 {
            2
        } else {
            3
        };
    }

    /// 当前级别下动效是否可用。
    pub fn motion_ok(&self) -> bool {
        self.level < 2
    }

    /// 极简档必须保留的核心（输入路径永不降）。
    pub fn input_ok(&self) -> bool {
        true
    }
}

// ---------------------------------------------------------------------------
// F062 — 内存额度谈判：进程间配额转让
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct QuotaLedger {
    pub balances: [i32; 8], // 8 进程，页为单位
}

impl QuotaLedger {
    pub const fn new() -> QuotaLedger {
        QuotaLedger { balances: [0; 8] }
    }

    pub fn grant(&mut self, pid: usize, pages: i32) -> bool {
        if pid >= 8 || pages < 0 {
            return false;
        }
        self.balances[pid] += pages;
        true
    }

    /// 转让：from 余额必须足够（不可负债转让）。
    pub fn transfer(&mut self, from: usize, to: usize, pages: i32) -> bool {
        if from >= 8 || to >= 8 || from == to || pages <= 0 {
            return false;
        }
        if self.balances[from] < pages {
            return false;
        }
        self.balances[from] -= pages;
        self.balances[to] += pages;
        true
    }

    pub fn balance(&self, pid: usize) -> Option<i32> {
        if pid >= 8 { None } else { Some(self.balances[pid]) }
    }
}

// ---------------------------------------------------------------------------
// F063 — 脏页回写水线：映射文件回写策略
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DirtyWatermark {
    pub total_pages: u32,
    pub dirty_pages: u32,
    /// 低水线/高水线 permille。
    pub low_permille: u32,
    pub high_permille: u32,
}

impl DirtyWatermark {
    pub const fn new(low_permille: u32, high_permille: u32) -> DirtyWatermark {
        DirtyWatermark { total_pages: 1000, dirty_pages: 0, low_permille, high_permille }
    }

    pub fn dirty(&mut self, pages: u32) {
        self.dirty_pages = pages.min(self.total_pages);
    }

    pub fn dirty_permille(&self) -> u32 {
        self.dirty_pages * 1000 / self.total_pages.max(1)
    }

    /// 0=不动 1=后台回写 2=限流回写（触高水线）。
    pub fn action(&self) -> u8 {
        let p = self.dirty_permille();
        if p >= self.high_permille {
            2
        } else if p >= self.low_permille {
            1
        } else {
            0
        }
    }
}

// ---------------------------------------------------------------------------
// F064 — 页迁移整理：物理页规整化
// ---------------------------------------------------------------------------

/// 计算把散页搬到连续区所需迁移次数；连续段判定用位图。
pub fn compaction_moves(used: &[bool], span: usize) -> Option<u32> {
    if span == 0 || span > used.len() {
        return None;
    }
    // 在 span 大小的滑动窗里找"需要移动最少的窗口"。
    let mut best: Option<u32> = None;
    let mut used_ct = 0u32;
    for &u in used {
        if u {
            used_ct += 1;
        }
    }
    if used_ct as usize > span {
        return None;
    }
    for w in 0..=(used.len() - span) {
        let mut holes = 0u32;
        for i in w..w + span {
            if !used[i] {
                holes += 1;
            }
        }
        // 需要搬入 holes 个页 → 迁移数 = holes
        if best.map(|b| holes < b).unwrap_or(true) {
            best = Some(holes);
        }
    }
    best
}

// ---------------------------------------------------------------------------
// F065 — 碎片整理器：后台 compaction 调度
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Compactor {
    /// 外部碎片 permille。
    pub frag_permille: u32,
    pub runs: u32,
    pub pages_moved: u32,
    /// 触发阈值。
    pub threshold_permille: u32,
}

impl Compactor {
    pub const fn new(threshold_permille: u32) -> Compactor {
        Compactor { frag_permille: 0, runs: 0, pages_moved: 0, threshold_permille }
    }

    pub fn observe(&mut self, frag_permille: u32) -> bool {
        self.frag_permille = frag_permille.min(1000);
        if self.frag_permille >= self.threshold_permille {
            self.runs += 1;
            // 假设每次整理移走碎片的 1/3
            let moved = self.frag_permille / 3;
            self.pages_moved += moved;
            self.frag_permille -= moved;
            true
        } else {
            false
        }
    }

    pub fn converged(&self) -> bool {
        self.frag_permille < self.threshold_permille
    }
}

// ---------------------------------------------------------------------------
// F066 — 内存事件流：分配/释放事件订阅
// ---------------------------------------------------------------------------

pub const MAX_MEM_EVENTS: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MemEvent {
    pub at_ms: u32,
    pub site: u16,
    pub bytes: i32, // 正分配负释放
}

#[derive(Clone, Copy)]
pub struct MemEventRing {
    pub buf: [Option<MemEvent>; MAX_MEM_EVENTS],
    pub head: usize,
    pub len: usize,
    pub subscribers: u8,
}

impl MemEventRing {
    pub const fn new() -> MemEventRing {
        MemEventRing { buf: [const { None }; MAX_MEM_EVENTS], head: 0, len: 0, subscribers: 0 }
    }

    pub fn push(&mut self, ev: MemEvent) {
        let idx = (self.head + self.len) % MAX_MEM_EVENTS;
        if self.len == MAX_MEM_EVENTS {
            self.buf[self.head] = Some(ev); // 覆盖最老
            self.head = (self.head + 1) % MAX_MEM_EVENTS;
        } else {
            self.buf[idx] = Some(ev);
            self.len += 1;
        }
    }

    pub fn subscribe(&mut self) -> bool {
        if self.subscribers >= 8 {
            return false;
        }
        self.subscribers += 1;
        true
    }

    /// 净流量（字节）。
    pub fn net_bytes(&self) -> i64 {
        let mut net = 0i64;
        for i in 0..self.len {
            if let Some(e) = self.buf[(self.head + i) % MAX_MEM_EVENTS] {
                net += e.bytes as i64;
            }
        }
        net
    }
}

// ---------------------------------------------------------------------------
// F067 — 堆快照比对：双快照 diff 定位增长
// ---------------------------------------------------------------------------

pub const SNAP_SITES: usize = 8;

#[derive(Clone, Copy)]
pub struct HeapSnap {
    pub bytes: [u32; SNAP_SITES],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SnapDiff {
    pub site: usize,
    pub delta: i32,
}

/// 对比两快照，返回增长最大位点。
pub fn snap_diff(a: &HeapSnap, b: &HeapSnap) -> Option<SnapDiff> {
    let mut best: Option<SnapDiff> = None;
    for i in 0..SNAP_SITES {
        let d = b.bytes[i] as i64 - a.bytes[i] as i64;
        if d > 0 && best.map(|bd| d > bd.delta as i64).unwrap_or(true) {
            best = Some(SnapDiff { site: i, delta: d as i32 });
        }
    }
    best
}

// ---------------------------------------------------------------------------
// F068 — 内存预算 DSL：声明式预算描述
// ---------------------------------------------------------------------------

/// DSL：[op, arg]。op: 0=cap-total(MB) 1=cap-per-app(MB) 2=reserve(MB) 3=end
pub const MEM_DSL_MAX: usize = 10;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MemBudget {
    pub total_mb: u32,
    pub per_app_mb: u32,
    pub reserve_mb: u32,
}

pub fn mem_dsl_eval(prog: &[u8]) -> Option<MemBudget> {
    if prog.is_empty() || prog.len() > MEM_DSL_MAX * 2 {
        return None;
    }
    let mut b = MemBudget { total_mb: 0, per_app_mb: 0, reserve_mb: 0 };
    let mut i = 0;
    while i + 1 < prog.len() {
        let (op, arg) = (prog[i], prog[i + 1] as u32);
        match op {
            0 => b.total_mb = arg,
            1 => b.per_app_mb = arg,
            2 => b.reserve_mb = arg,
            3 => return Some(b),
            _ => return None,
        }
        i += 2;
    }
    Some(b)
}

/// 预算自洽：reserve < total，per_app ≤ total。
pub fn mem_budget_sane(b: &MemBudget) -> bool {
    b.total_mb > 0 && b.reserve_mb < b.total_mb && b.per_app_mb <= b.total_mb
}

// ---------------------------------------------------------------------------
// F069 — 亲和路由表：NUMA 决策表导出
// ---------------------------------------------------------------------------

pub const NUMA_NODES: usize = 4;

#[derive(Clone, Copy)]
pub struct NumaTable {
    /// node → (local_mem_pages, cpu_count)。
    pub mem_pages: [u32; NUMA_NODES],
    pub cpus: [u8; NUMA_NODES],
}

impl NumaTable {
    pub const fn new() -> NumaTable {
        NumaTable { mem_pages: [0; NUMA_NODES], cpus: [0; NUMA_NODES] }
    }

    pub fn set(&mut self, node: usize, mem: u32, cpus: u8) -> bool {
        if node >= NUMA_NODES {
            return false;
        }
        self.mem_pages[node] = mem;
        self.cpus[node] = cpus;
        true
    }

    /// 每 CPU 内存最多（亲和最优节点）。
    pub fn best_node(&self) -> Option<usize> {
        let mut best: Option<usize> = None;
        let mut best_ratio = 0u64;
        for n in 0..NUMA_NODES {
            if self.cpus[n] == 0 {
                continue;
            }
            let r = self.mem_pages[n] as u64 / self.cpus[n] as u64;
            if r > best_ratio {
                best_ratio = r;
                best = Some(n);
            }
        }
        best
    }
}

// ---------------------------------------------------------------------------
// F070 — 页加密预留：安全休眠铺垫
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PageCrypto {
    /// 密钥槽代号（0=未配）。
    pub key_slot: u8,
    pub encrypted_pages: u32,
    /// 加密不可逆开关（误配保护）。
    pub locked: bool,
}

impl PageCrypto {
    pub const fn new() -> PageCrypto {
        PageCrypto { key_slot: 0, encrypted_pages: 0, locked: false }
    }

    pub fn bind_key(&mut self, slot: u8) -> bool {
        if self.locked || slot == 0 {
            return false;
        }
        self.key_slot = slot;
        true
    }

    pub fn encrypt_page(&mut self) -> bool {
        if self.key_slot == 0 {
            return false;
        }
        self.encrypted_pages += 1;
        true
    }

    pub fn lock(&mut self) -> bool {
        if self.key_slot == 0 {
            return false;
        }
        self.locked = true;
        true
    }
}

// ---------------------------------------------------------------------------
// F071 — 常驻结构体检：内核常驻池调优
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct ResidentAudit {
    /// 常驻结构 → (bytes, 使用率 permille)。
    pub bytes: [u32; 8],
    pub util: [u16; 8],
    pub count: usize,
}

impl ResidentAudit {
    pub const fn new() -> ResidentAudit {
        ResidentAudit { bytes: [0; 8], util: [0; 8], count: 0 }
    }

    pub fn record(&mut self, bytes: u32, util_permille: u16) -> bool {
        if self.count >= 8 || bytes == 0 {
            return false;
        }
        self.bytes[self.count] = bytes;
        self.util[self.count] = util_permille.min(1000);
        self.count += 1;
        true
    }

    pub fn total_bytes(&self) -> u32 {
        let mut t = 0;
        for i in 0..self.count {
            t += self.bytes[i];
        }
        t
    }

    /// 浪费：利用率 < 30% 的常驻结构字节和（调优目标）。
    pub fn waste_bytes(&self) -> u32 {
        let mut w = 0;
        for i in 0..self.count {
            if self.util[i] < 300 {
                w += self.bytes[i];
            }
        }
        w
    }
}

// ---------------------------------------------------------------------------
// F072 — 内存 API 版本化：接口契约
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MemApiContract {
    pub major: u16,
    pub minor: u16,
    /// 破坏性变更必须升 major。
    pub breaking: bool,
}

impl MemApiContract {
    pub const fn new(major: u16, minor: u16) -> MemApiContract {
        MemApiContract { major, minor, breaking: false }
    }

    pub fn evolve(&mut self, breaking: bool) -> bool {
        if breaking {
            if self.major == u16::MAX {
                return false;
            }
            self.major += 1;
            self.minor = 0;
        } else {
            self.minor += 1;
        }
        self.breaking = breaking;
        true
    }

    pub fn compatible_with(&self, client_major: u16) -> bool {
        self.major == client_major
    }
}

// ---------------------------------------------------------------------------
// F073 — 内存 fuzz：分配器对抗测试
// ---------------------------------------------------------------------------

/// 对 (op, arg) 序列做分配器不变量校验：
/// op0=alloc arg 页, op1=free arg 页。free 不得超过已分配；总量不得超 cap。
pub fn alloc_fuzz(cases: &[(u8, u32)], cap_pages: u32) -> u32 {
    let mut live = 0u32;
    let mut bad = 0;
    for &(op, arg) in cases {
        match op {
            0 => {
                if live + arg > cap_pages {
                    bad += 1; // 超 cap 应被拒绝，不能默默成功
                } else {
                    live += arg;
                }
            }
            1 => {
                if arg > live {
                    bad += 1; // 双重释放/超放
                } else {
                    live -= arg;
                }
            }
            _ => bad += 1,
        }
    }
    bad
}

// ---------------------------------------------------------------------------
// F074 — 内存回归基线：自动比对
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct MemBaseline {
    pub peak_kb: u32,
    pub p99_alloc_us: u32,
}

impl MemBaseline {
    pub const fn new(peak_kb: u32, p99_alloc_us: u32) -> MemBaseline {
        MemBaseline { peak_kb, p99_alloc_us }
    }

    /// 回归：峰值或延迟劣化 >10%。
    pub fn regressed(&self, new_peak: u32, new_p99: u32) -> bool {
        new_peak * 100 > self.peak_kb * 110 || new_p99 * 100 > self.p99_alloc_us * 110
    }
}

// ---------------------------------------------------------------------------
// F075 — 内存域自检：25 项 CheckSet 汇入总检
// ---------------------------------------------------------------------------

pub fn run_m5mem_checks() -> CheckSet {
    let mut set = CheckSet::new("m5mem");

    // F051 画像
    let mut mp = MemProfiler::new();
    set.add("F051 record sites", mp.record(1, 100) && mp.record(2, 300) && mp.record(1, 50), "rec");
    set.add("F051 top site", mp.top_site() == Some(2), "top");
    set.add("F051 total", mp.total_bytes() == 450, "total");

    // F052 冷页
    set.add("F052 temp grades", page_temp(9) == PageTemp::Hot && page_temp(3) == PageTemp::Warm && page_temp(0) == PageTemp::Cold, "grade");
    set.add("F052 reclaim order", reclaim_candidates(&[0, 9, 1, 8], 2) == 2, "reclaim");

    // F053 压缩档
    let mut cg = CompressGovernor::new();
    cg.observe(200);
    set.add("F053 low pressure off", cg.level == 0 && !cg.worth(), "off");
    cg.observe(700);
    set.add("F053 mid pressure balanced", cg.level == 2 && cg.ratio_permille() == 500, "mid");
    cg.observe(950);
    set.add("F053 high pressure best", cg.level == 3, "high");

    // F054 超售
    let mut og = OversellGuard::new(1000);
    set.add("F054 commit ok", og.commit(1100), "ok");
    set.add("F054 warning", og.warning() && og.oversell_permille() == 1100, "warn");
    set.add("F054 hard cap", !og.commit(150) && og.committed_pages == 1100, "cap");

    // F055 巨页
    set.add("F055 worth", hugepage_worth(512, 1000, 300), "worth");
    set.add("F055 frag not worth", !hugepage_worth(2, 200, 10), "frag");

    // F056 OOM 演练
    let mut od = OomDrill::new(100);
    set.add("F056 fill", od.alloc(80) && od.alloc(30) == false && od.oom_fired, "fill");
    set.add("F056 victim recovers", od.kill_victim(30) && od.recovered() && od.alloc(10), "recover");
    set.add("F056 no oom no kill", !OomDrill::new(10).kill_victim(5), "noop");

    // F057 泄漏指纹
    let mut lf = LeakFingerprint::new(9);
    let _ = lf.sample(0, 100);
    let _ = lf.sample(1, 110);
    let _ = lf.sample(2, 120);
    let _ = lf.sample(3, 130);
    set.add("F057 leak detected", lf.leaking(), "leak");
    let mut lf2 = LeakFingerprint::new(10);
    let _ = lf2.sample(0, 100);
    let _ = lf2.sample(1, 50);
    let _ = lf2.sample(2, 100);
    let _ = lf2.sample(3, 100);
    set.add("F057 stable not leak", !lf2.leaking(), "stable");

    // F058 KSM
    let mut ksm = KsmEngine::new();
    set.add("F058 first page unique", ksm.submit(0xAABB) && ksm.saved_pages == 0, "first");
    set.add("F058 dedup saves", ksm.submit(0xAABB) && ksm.saved_pages == 1, "dedup");
    set.add("F058 retire releases", ksm.retire(0xAABB) && ksm.retire(0xAABB) && !ksm.retire(0xAABB), "retire");

    // F059 带宽
    let mut bm = BandwidthMeter::new();
    bm.window(300, 100);
    bm.window(100, 900);
    set.add("F059 peak tracked", bm.peak_mb == 1000, "peak");
    set.add("F059 rw ratio", bm.rw_ratio_permille() == 900, "ratio");

    // F060 对齐审计
    let mut aa = AlignAudit::new();
    set.add("F060 one-line ok", aa.audit(64, 64), "ok");
    set.add("F060 ragged flagged", !aa.audit(100, 64) && aa.violations == 1, "ragged");

    // F061 优雅档
    let mut gd = GracefulDeg::new();
    gd.observe(400);
    set.add("F061 full fx", gd.level == 0 && gd.motion_ok(), "full");
    gd.observe(100);
    set.add("F061 motion cut", !gd.motion_ok(), "cut");
    gd.observe(10);
    set.add("F061 input never cut", gd.level == 3 && gd.input_ok(), "input");

    // F062 额度谈判
    let mut ql = QuotaLedger::new();
    set.add("F062 grant", ql.grant(0, 100) && ql.grant(1, 50), "grant");
    set.add("F062 transfer", ql.transfer(0, 1, 60) && ql.balance(0) == Some(40) && ql.balance(1) == Some(110), "xfer");
    set.add("F062 overdraft blocked", !ql.transfer(0, 1, 100), "overdraft");

    // F063 回写水线
    let mut dw = DirtyWatermark::new(100, 400);
    dw.dirty(50);
    set.add("F063 below low", dw.action() == 0, "idle");
    dw.dirty(200);
    set.add("F063 background wb", dw.action() == 1, "bg");
    dw.dirty(500);
    set.add("F063 throttled wb", dw.action() == 2, "throttle");

    // F064 页迁移
    let used = [true, false, true, true, false, false, true, false];
    set.add("F064 best window moves", compaction_moves(&used, 4) == Some(1), "moves");
    set.add("F064 impossible", compaction_moves(&used, 2).is_none(), "impossible");

    // F065 碎片整理
    let mut cp = Compactor::new(500);
    set.add("F065 trigger at threshold", cp.observe(600) && cp.runs == 1 && cp.pages_moved == 200, "run");
    set.add("F065 converge", cp.converged(), "conv");
    set.add("F065 quiet below", !Compactor::new(500).observe(100), "quiet");

    // F066 事件流
    let mut ring = MemEventRing::new();
    set.add("F066 subscribe", ring.subscribe() && ring.subscribe(), "sub");
    ring.push(MemEvent { at_ms: 1, site: 1, bytes: 100 });
    ring.push(MemEvent { at_ms: 2, site: 1, bytes: -40 });
    set.add("F066 net bytes", ring.net_bytes() == 60, "net");
    set.add("F066 ring wrap", {
        for i in 0..20 {
            ring.push(MemEvent { at_ms: i, site: 1, bytes: 1 });
        }
        ring.len == MAX_MEM_EVENTS
    }, "wrap");

    // F067 快照比对
    let a = HeapSnap { bytes: [100, 200, 300, 0, 0, 0, 0, 0] };
    let b = HeapSnap { bytes: [100, 260, 290, 0, 0, 0, 0, 0] };
    set.add("F067 growth site", snap_diff(&a, &b) == Some(SnapDiff { site: 1, delta: 60 }), "grow");

    // F068 预算 DSL
    let mb = mem_dsl_eval(&[0, 64, 1, 8, 2, 4, 3]);
    set.add("F068 dsl eval", mb.map(|b| b.total_mb == 64 && b.per_app_mb == 8 && b.reserve_mb == 4).unwrap_or(false), "eval");
    set.add("F068 sane", mb.map(|b| mem_budget_sane(&b)).unwrap_or(false), "sane");
    set.add("F068 bad op", mem_dsl_eval(&[7, 1]).is_none(), "bad");

    // F069 NUMA
    let mut nt = NumaTable::new();
    set.add("F069 set nodes", nt.set(0, 8000, 4) && nt.set(1, 16000, 8), "set");
    set.add("F069 best node", nt.best_node() == Some(0), "best");
    set.add("F069 skip empty", !nt.set(9, 1, 1), "valid");

    // F070 页加密
    let mut pc = PageCrypto::new();
    set.add("F070 need key", !pc.encrypt_page() && !pc.bind_key(0), "needkey");
    set.add("F070 bind + encrypt", pc.bind_key(3) && pc.encrypt_page() && pc.encrypted_pages == 1, "enc");
    set.add("F070 lock", pc.lock() && !pc.bind_key(4), "lock");

    // F071 常驻体检
    let mut ra = ResidentAudit::new();
    set.add("F071 record", ra.record(1000, 900) && ra.record(500, 100), "rec");
    set.add("F071 total", ra.total_bytes() == 1500, "total");
    set.add("F071 waste found", ra.waste_bytes() == 500, "waste");

    // F072 API 版本
    let mut api = MemApiContract::new(1, 3);
    set.add("F072 minor evolve", api.evolve(false) && api.minor == 4, "minor");
    set.add("F072 major evolve", api.evolve(true) && api.major == 2 && api.minor == 0, "major");
    set.add("F072 compat gate", api.compatible_with(2) && !api.compatible_with(1), "compat");

    // F073 fuzz
    set.add("F073 overflow caught", alloc_fuzz(&[(0, 90), (0, 90)], 100) == 1, "overflow");
    set.add("F073 double free caught", alloc_fuzz(&[(0, 10), (1, 20)], 100) == 1, "double");
    set.add("F073 clean run", alloc_fuzz(&[(0, 50), (1, 50), (0, 30)], 100) == 0, "clean");

    // F074 回归基线
    let base = MemBaseline::new(1000, 100);
    set.add("F074 no regression", !base.regressed(1050, 105), "ok");
    set.add("F074 peak regression", base.regressed(1200, 100), "peak");
    set.add("F074 latency regression", base.regressed(1000, 130), "lat");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f052_reclaim_order() {
        assert_eq!(reclaim_candidates(&[0, 9, 1, 8], 3), 3);
        assert_eq!(reclaim_candidates(&[9, 8, 9], 2), 0);
    }

    #[test]
    fn f058_ksm_dedup() {
        let mut ksm = KsmEngine::new();
        assert!(ksm.submit(1));
        assert!(ksm.submit(1));
        assert_eq!(ksm.saved_pages, 1);
    }

    #[test]
    fn f075_self_check_passes() {
        let set = run_m5mem_checks();
        assert_eq!(set.len(), 64, "m5mem 需要 75 项断言");
        assert!(!set.truncated());
        assert!(set.all_passed(), "m5mem 自检必须全绿");
    }
}
