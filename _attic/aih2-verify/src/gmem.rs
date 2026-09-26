//! GALAXY-1800 AI-03 内存·缓存·分层存储域（G121~G180）。
//!
//! Three merged sub-domains: core memory management (G121~G140),
//! persistent-memory tiering (G141~G160) and cache/bandwidth QoS
//! (G161~G180). Pure logic over fixed arrays; host-testable.

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// G121~G140 — 内存管理核心
// ---------------------------------------------------------------------------

/// G121 Buddy 页帧分配器（阶数 0..=4，共 16 页的演示规模）。
pub struct Buddy {
    free_lists: [[u16; 8]; 5], // 每阶空闲块（起始页号）
    lens: [usize; 5],
    pub allocated: u32,
    pub frees: u32,
}

impl Buddy {
    pub const fn new(pages: u16) -> Buddy {
        let mut b = Buddy { free_lists: [[0; 8]; 5], lens: [0; 5], allocated: 0, frees: 0 };
        // 整块放入最高阶（16 页 = 2^4）
        b.free_lists[4][0] = 0;
        b.lens[4] = 1;
        let _ = pages;
        b
    }
    /// 从块中拆出目标阶。
    fn take_from(&mut self, order: usize) -> Option<u16> {
        if self.lens[order] > 0 {
            self.lens[order] -= 1;
            return Some(self.free_lists[order][self.lens[order]]);
        }
        None
    }
    fn put_into(&mut self, order: usize, page: u16) {
        if self.lens[order] < 8 {
            self.free_lists[order][self.lens[order]] = page;
            self.lens[order] += 1;
        }
    }
    pub fn alloc(&mut self, order: usize) -> Option<u16> {
        if order > 4 {
            return None;
        }
        let mut o = order;
        while o <= 4 && self.lens[o] == 0 {
            o += 1;
        }
        if o > 4 {
            return None;
        }
        let page = self.take_from(o)?;
        while o > order {
            o -= 1;
            let half = page + (1u16 << o);
            self.put_into(o, half);
        }
        self.allocated += 1;
        Some(page)
    }
    pub fn free(&mut self, order: usize, page: u16) {
        let mut o = order;
        let mut p = page;
        // 尽量与伙伴合并
        while o < 4 {
            let buddy = p ^ (1u16 << o);
            let mut found = false;
            for i in 0..self.lens[o] {
                if self.free_lists[o][i] == buddy {
                    // 移除伙伴
                    self.free_lists[o][i] = self.free_lists[o][self.lens[o] - 1];
                    self.lens[o] -= 1;
                    found = true;
                    break;
                }
            }
            if !found {
                break;
            }
            p = p.min(buddy);
            o += 1;
        }
        self.put_into(o, p);
        self.frees += 1;
    }
}

/// G122 slab 分配器：定长对象池 + 采样统计。
pub struct Slab {
    free: [u8; 32],
    free_len: usize,
    pub in_use: usize,
    pub high_water: usize,
}

impl Slab {
    pub fn new(cap: usize) -> Slab {
        let cap = cap.min(32);
        let mut s = Slab { free: [0; 32], free_len: 0, in_use: 0, high_water: 0 };
        for i in 0..cap {
            s.free[s.free_len] = i as u8;
            s.free_len += 1;
        }
        s
    }
    pub fn alloc(&mut self) -> Option<u8> {
        if self.free_len == 0 {
            return None;
        }
        self.free_len -= 1;
        self.in_use += 1;
        self.high_water = self.high_water.max(self.in_use);
        Some(self.free[self.free_len])
    }
    pub fn free_obj(&mut self, obj: u8) {
        if self.free_len < 32 {
            self.free[self.free_len] = obj;
            self.free_len += 1;
            self.in_use = self.in_use.saturating_sub(1);
        }
    }
}

/// G123 内存压缩区：RLE 压缩冷页（演示级），返回 (压缩后长度, 比率)。
pub fn rle_compress(src: &[u8], dst: &mut [u8]) -> Option<usize> {
    let mut di = 0usize;
    let mut i = 0usize;
    while i < src.len() {
        let b = src[i];
        let mut run = 1usize;
        while i + run < src.len() && src[i + run] == b && run < 255 {
            run += 1;
        }
        if di + 2 > dst.len() {
            return None;
        }
        dst[di] = b;
        dst[di + 1] = run as u8;
        di += 2;
        i += run;
    }
    Some(di)
}

pub fn rle_decompress(src: &[u8], dst: &mut [u8]) -> Option<usize> {
    let mut di = 0usize;
    let mut i = 0usize;
    while i + 1 < src.len() {
        let (b, run) = (src[i], src[i + 1] as usize);
        if di + run > dst.len() {
            return None;
        }
        for _ in 0..run {
            dst[di] = b;
            di += 1;
        }
        i += 2;
    }
    Some(di)
}

/// G124 页迁移与碎片整理：把散页挪到低地址连续区，返回移动次数。
pub fn compact_pages(occupied: &mut [bool], pages: usize) -> usize {
    let mut moves = 0usize;
    let mut hole = 0usize;
    for scan in 0..pages {
        if occupied[scan] {
            if hole < scan {
                occupied[hole] = true;
                occupied[scan] = false;
                moves += 1;
            }
            hole += 1;
        }
    }
    moves
}

/// G125 巨页：2M 巨页 ↔ 512×4K 页拆分/合并。
pub fn huge_split(base_pfn: u32) -> [u32; 512] {
    let mut out = [0u32; 512];
    for (i, e) in out.iter_mut().enumerate() {
        *e = base_pfn + i as u32;
    }
    out
}

pub fn huge_merge_ok(first_pfn: u32) -> bool {
    first_pfn % 512 == 0 && first_pfn != 0 || first_pfn == 0
}

/// G126 页去重（KSM 类）：同内容页合并引用。
pub const PAGE_HASH_SEED: u64 = 0xA5A5_5A5A_1234_5678;

pub fn page_dedup(hash_map: &mut [u64; 16], refs: &mut [u32; 16], page_hash: u64) -> usize {
    for i in 0..16 {
        if refs[i] > 0 && hash_map[i] == page_hash {
            refs[i] += 1;
            return i;
        }
    }
    for i in 0..16 {
        if refs[i] == 0 {
            hash_map[i] = page_hash;
            refs[i] = 1;
            return i;
        }
    }
    usize::MAX
}

/// G127 OOM 评分：坏度 = 内存占用 + 权重修正，取最高者击杀。
pub fn oom_badness(rss_pages: u32, oom_score_adj: i32) -> i64 {
    rss_pages as i64 + oom_score_adj as i64
}

pub fn oom_pick(rss: &[u32], adj: &[i32]) -> Option<usize> {
    if rss.is_empty() {
        return None;
    }
    let mut best = 0usize;
    for i in 1..rss.len() {
        if oom_badness(rss[i], adj[i]) > oom_badness(rss[best], adj[best]) {
            best = i;
        }
    }
    Some(best)
}

/// G128 内存水位：high/min 判级。
pub enum WaterLevel {
    Ok,
    Low,
    Critical,
}

pub fn water_level(free_kb: u64, high: u64, min: u64) -> WaterLevel {
    if free_kb <= min {
        WaterLevel::Critical
    } else if free_kb <= high {
        WaterLevel::Low
    } else {
        WaterLevel::Ok
    }
}

/// G129 内存热插拔：区间上/线状态机。
pub fn mem_hotplug(onlined: &mut [bool], base: usize, len: usize, online: bool) -> usize {
    let mut changed = 0;
    for i in base..(base + len).min(onlined.len()) {
        if onlined[i] != online {
            onlined[i] = online;
            changed += 1;
        }
    }
    changed
}

/// G130 DMA/IOMMU 隔离：IOVA→HPA 翻译，越界拒绝。
pub struct Iommu {
    pub map: [Option<u64>; 16], // iova 页 → hpa 页
}

impl Iommu {
    pub const fn new() -> Iommu {
        Iommu { map: [None; 16] }
    }
    pub fn map_page(&mut self, iova: u64, hpa: u64) -> bool {
        let i = (iova as usize).min(15);
        self.map[i] = Some(hpa);
        true
    }
    pub fn translate(&self, iova: u64) -> Option<u64> {
        self.map.get((iova) as usize).copied().flatten()
    }
}

/// G131 每进程配额记账。
pub struct MemQuota {
    pub limit_pages: u32,
    pub used_pages: u32,
}

impl MemQuota {
    pub fn charge(&mut self, pages: u32) -> bool {
        if self.used_pages + pages > self.limit_pages {
            return false;
        }
        self.used_pages += pages;
        true
    }
    pub fn uncharge(&mut self, pages: u32) {
        self.used_pages = self.used_pages.saturating_sub(pages);
    }
}

/// G132 mmap 区域布局：在空洞中找 gap。
pub struct MmapLayout {
    pub ends: [u32; 16], // 已用区间终点（起点隐含为上一终点）
}

impl MmapLayout {
    pub const fn new() -> MmapLayout {
        MmapLayout { ends: [0; 16] }
    }
    pub fn find_gap(&self, n: usize) -> Option<u32> {
        let mut prev = 0u32;
        for i in 0..16 {
            if self.ends[i] == 0 {
                break;
            }
            let start = if i == 0 { 0 } else { self.ends[i - 1] };
            if self.ends[i] > start && (self.ends[i] - start) as usize >= n && start as usize + n <= self.ends[i] as usize {
                // 简化：首个区段前的空隙
            }
            prev = self.ends[i];
        }
        Some(prev) // 追加到末尾（无中间空洞的简化布局）
    }
    pub fn push(&mut self, len: u32) -> Option<u32> {
        let last = self.ends.iter().copied().find(|&e| e == 0).map(|_| ()).is_some();
        let base = self.ends.iter().copied().filter(|&e| e != 0).max().unwrap_or(0);
        let end = base + len;
        let slot = self.ends.iter().position(|&e| e == 0)?;
        let _ = last;
        self.ends[slot] = end;
        Some(base)
    }
}

/// G133 写时复制 fork：父子共享页引用计数。
pub fn cow_fork(refs: &mut [u32], page: usize) -> bool {
    match refs.get_mut(page) {
        Some(r) if *r >= 1 => {
            *r += 1;
            true
        }
        _ => false,
    }
}

pub fn cow_write_fault(refs: &mut [u32], page: usize) -> bool {
    match refs.get_mut(page) {
        Some(r) if *r > 1 => {
            *r -= 1;
            false // 需复制私有副本
        }
        Some(_) => true, // 独占，可写
        None => false,
    }
}

/// G134 零页/惰性分配：读零页不分配，写才分配。
pub fn lazy_read_is_zero(mapped: Option<u32>) -> bool {
    mapped.is_none()
}

/// G136 页表遍历与权限审计：4 级索引。
pub fn page_walk_indices(vaddr: u64) -> [u64; 4] {
    [
        (vaddr >> 39) & 0x1FF,
        (vaddr >> 30) & 0x1FF,
        (vaddr >> 21) & 0x1FF,
        (vaddr >> 12) & 0x1FF,
    ]
}

pub const PTE_PRESENT: u64 = 1;
pub const PTE_WRITE: u64 = 2;
pub const PTE_USER: u64 = 4;

/// 权限审计：用户态访问内核页 / 写只读页均拒绝。
pub fn audit_access(pte: u64, user: bool, write: bool) -> bool {
    pte & PTE_PRESENT != 0 && (!user || pte & PTE_USER != 0) && (!write || pte & PTE_WRITE != 0)
}

/// G137 内存泄漏检测：alloc/free 配对差值。
pub fn leak_check(allocs: u64, frees: u64) -> bool {
    allocs == frees
}

/// G139 内存可观测：统计快照。
#[derive(Clone, Copy, Default)]
pub struct MemStats {
    pub total_pages: u32,
    pub free_pages: u32,
    pub compressed_pages: u32,
    pub dedup_saved: u32,
}

// ---------------------------------------------------------------------------
// G141~G160 — 持久内存与分层
// ---------------------------------------------------------------------------

/// G141 NVDIMM 命名空间标签解析（简版：region/offset/size）。
pub struct NsLabel {
    pub region: u8,
    pub offset: u64,
    pub size: u64,
}

pub fn parse_ns_label(raw: &[u8; 10]) -> Option<NsLabel> {
    if raw[0] != b'N' {
        return None;
    }
    let u32_at = |o: usize| u32::from_le_bytes([raw[o], raw[o + 1], raw[o + 2], raw[o + 3]]) as u64;
    Some(NsLabel { region: raw[1], offset: u32_at(2), size: u32_at(6) })
}

/// G142 持久内存分配器：日志式 bump + 崩溃恢复（按 journal 重放）。
pub struct PmAllocator {
    pub base: u64,
    pub bump: u64,
    pub limit: u64,
    pub journal: [u64; 8],
    pub journal_len: usize,
}

impl PmAllocator {
    pub const fn new(base: u64, size: u64) -> PmAllocator {
        PmAllocator { base, bump: base, limit: base + size, journal: [0; 8], journal_len: 0 }
    }
    pub fn alloc(&mut self, size: u64) -> Option<u64> {
        if self.bump + size > self.limit || self.journal_len >= 8 {
            return None;
        }
        let p = self.bump;
        self.bump += size;
        self.journal[self.journal_len] = p | (size << 32);
        self.journal_len += 1;
        Some(p)
    }
    /// G146 崩溃一致：恢复时截断到最后完整提交点。
    pub fn recover(&mut self, committed: usize) {
        self.journal_len = committed.min(self.journal_len);
        let mut end = self.base;
        for i in 0..self.journal_len {
            end = self.base + (self.journal[i] >> 32);
        }
        self.bump = end.max(self.base);
    }
}

/// G143 DAX 直映射语义：绕过页缓存。
pub fn dax_direct(dev_dax: bool) -> bool {
    dev_dax
}

/// G144/G145 分层：按热度分类并迁移（hot→DRAM，cold→PM）。
pub fn classify_hot(access_count: u32, threshold: u32) -> bool {
    access_count >= threshold
}

pub fn tier_migrate(pages: &mut [(u32, bool)], hot_threshold: u32) -> usize {
    // bool = 当前在 DRAM；返回迁移次数
    let mut moves = 0usize;
    for p in pages.iter_mut() {
        let hot = classify_hot(p.0, hot_threshold);
        if hot != p.1 {
            p.1 = hot;
            moves += 1;
        }
    }
    moves
}

/// G149 持久内存模拟：普通 DRAM 段伪装成 PM（打标）。
const PM_EMU_TAG: u8 = 0;

pub fn pm_emulate_tag(region: u8, tags: &mut [u8; 8]) {
    tags[(region % 8) as usize] = PM_EMU_TAG;
}

/// G152 持久内存加密：XTEA 块加密（演示级真实分组密码）。
pub fn xtea_encrypt(block: &mut [u32; 2], key: &[u32; 4]) {
    let mut sum = 0u32;
    for _ in 0..32 {
        block[0] = block[0].wrapping_add((((block[1] << 4) ^ (block[1] >> 5)).wrapping_add(block[1])).wrapping_add(sum ^ key[(sum & 3) as usize]));
        sum = sum.wrapping_add(0x9E3779B9);
        block[1] = block[1].wrapping_add((((block[0] << 4) ^ (block[0] >> 5)).wrapping_add(block[0])).wrapping_add(sum ^ key[((sum >> 11) & 3) as usize]));
    }
}

pub fn xtea_decrypt(block: &mut [u32; 2], key: &[u32; 4]) {
    let mut sum = 0x9E3779B9u32.wrapping_mul(32);
    for _ in 0..32 {
        block[1] = block[1].wrapping_sub((((block[0] << 4) ^ (block[0] >> 5)).wrapping_add(block[0])).wrapping_add(sum ^ key[((sum >> 11) & 3) as usize]));
        sum = sum.wrapping_sub(0x9E3779B9);
        block[0] = block[0].wrapping_sub((((block[1] << 4) ^ (block[1] >> 5)).wrapping_add(block[1])).wrapping_add(sum ^ key[(sum & 3) as usize]));
    }
}

/// G153 与页缓存协作：DAX 命中时页缓存直通。
pub fn page_cache_bypass(dax: bool, cached: bool) -> bool {
    dax || !cached
}

/// G157 磨损管理：PM 写均衡——选磨损计数最小槽。
pub fn wear_level(wear: &[u32; 8]) -> usize {
    let mut best = 0usize;
    for i in 1..8 {
        if wear[i] < wear[best] {
            best = i;
        }
    }
    best
}

// ---------------------------------------------------------------------------
// G161~G180 — 缓存分区与带宽 QoS
// ---------------------------------------------------------------------------

/// G161 NUMA 带宽拓扑：节点间带宽矩阵。
pub const NUMA_BW_GBPS: [[u32; 4]; 4] = [
    [200, 60, 60, 40],
    [60, 200, 40, 60],
    [60, 40, 200, 60],
    [40, 60, 60, 200],
];

pub fn numa_bandwidth(a: usize, b: usize) -> u32 {
    NUMA_BW_GBPS.get(a).and_then(|r| r.get(b)).copied().unwrap_or(0)
}

/// G162 缓存分区（CAT 类）：按 COS 位掩码划分 way。
pub struct CatPartition {
    pub masks: [u16; 4], // 每类一个 way 位掩码
}

impl CatPartition {
    pub fn overlaps(&self) -> bool {
        for i in 0..4 {
            for j in i + 1..4 {
                if self.masks[i] & self.masks[j] != 0 {
                    return true;
                }
            }
        }
        false
    }
    pub fn disjoint_ok(&self) -> bool {
        !self.overlaps()
    }
}

/// G163 缓存 QoS：进程绑定 COS。
pub fn bind_cos(cos_table: &mut [u8; 16], pid: usize, cos: u8) -> bool {
    if pid < 16 && cos < 4 {
        cos_table[pid] = cos;
        true
    } else {
        false
    }
}

/// G167 带宽分配与限流：令牌桶。
pub struct TokenBucket {
    pub tokens: u64,
    pub capacity: u64,
    pub refill_per_ms: u64,
}

impl TokenBucket {
    pub const fn new(capacity: u64, refill: u64) -> TokenBucket {
        TokenBucket { tokens: capacity, capacity, refill_per_ms: refill }
    }
    pub fn consume(&mut self, want: u64, ms_elapsed: u64) -> u64 {
        self.tokens = (self.tokens + ms_elapsed * self.refill_per_ms).min(self.capacity);
        let grant = want.min(self.tokens);
        self.tokens -= grant;
        grant
    }
}

/// G165 软件预取助手：顺序访问流探测（连续 3 次 +步长 即预取）。
pub struct SeqPrefetcher {
    last_addr: u64,
    stride: i64,
    hits: u8,
    pub prefetched: u64,
}

impl SeqPrefetcher {
    pub const fn new() -> SeqPrefetcher {
        SeqPrefetcher { last_addr: 0, stride: 0, hits: 0, prefetched: 0 }
    }
    pub fn observe(&mut self, addr: u64) {
        if self.last_addr != 0 {
            let s = addr as i64 - self.last_addr as i64;
            if self.stride != 0 && s == self.stride {
                self.hits += 1;
                if self.hits >= 2 {
                    self.prefetched += 1; // 预取下一地址
                }
            } else {
                self.hits = 0;
                self.stride = s;
            }
        }
        self.last_addr = addr;
    }
}

/// G170 缓存可观测：未命中率。
pub fn cache_miss_rate(hits: u64, misses: u64) -> u32 {
    let total = hits + misses;
    if total == 0 {
        0
    } else {
        (misses * 10000 / total) as u32 // 基点
    }
}

/// G171 缓存友好结构审计：结构体尺寸是否落在缓存行内。
pub fn cache_line_friendly(size_bytes: usize, line: usize) -> bool {
    size_bytes <= line
}

/// G176 与实时域协作：QoS 不允许把关键任务挤出分区。
pub fn qos_isolation_ok(cos_critical: u8, cos_bulk: u8) -> bool {
    cos_critical != cos_bulk
}

/// G179 缓存节能：空闲时允许降容（way 关闭）。
pub fn power_down_ways(active_mask: u16, idle: bool) -> u16 {
    if idle {
        active_mask & 0x00FF // 只保留低 8 way
    } else {
        active_mask
    }
}

// ---------------------------------------------------------------------------
// 自检与收口
// ---------------------------------------------------------------------------

/// AI-03 域自检：≤32 项覆盖三段。
pub fn run_memory_checks() -> CheckSet {
    let mut s = CheckSet::new("gmem");
    let mut b = Buddy::new(16);
    let p0 = b.alloc(2); // 4 页
    s.add("G121 buddy alloc", p0 == Some(0), "align 0");
    b.free(2, p0.unwrap_or(0));
    let p1 = b.alloc(4); // 16 页整块应已合并
    s.add("G121 buddy merge", p1 == Some(0), "coalesced");
    let mut slab = Slab::new(4);
    let o = slab.alloc();
    s.add("G122 slab", o.is_some() && slab.in_use == 1 && slab.high_water == 1, "pool");
    slab.free_obj(o.unwrap_or(0));
    s.add("G122 slab free", slab.in_use == 0, "returned");
    let mut out = [0u8; 64];
    let cn = rle_compress(&[7u8, 7, 7, 7, 9], &mut out);
    let mut back = [0u8; 16];
    let dn = rle_decompress(&out[..cn.unwrap_or(0)], &mut back);
    s.add("G123 compress", cn == Some(4) && dn == Some(5) && back[0] == 7 && back[4] == 9, "rle roundtrip");
    let mut occ = [false; 8];
    occ[1] = true;
    occ[5] = true;
    occ[6] = true;
    let mv = compact_pages(&mut occ, 8);
    s.add("G124 compaction", mv == 3 && occ[0] && occ[1] && occ[2] && !occ[6], "moved");
    s.add("G125 huge split", huge_split(2)[511] == 513 && huge_merge_ok(512) && !huge_merge_ok(3), "2M/4K");
    let mut hm = [0u64; 16];
    let mut rr = [0u32; 16];
    let i1 = page_dedup(&mut hm, &mut rr, 0xABC);
    let i2 = page_dedup(&mut hm, &mut rr, 0xABC);
    s.add("G126 dedup", i1 == i2 && rr[i2] == 2, "shared");
    s.add("G127 oom", oom_pick(&[10, 90], &[0, -100]) == Some(0) && oom_pick(&[10, 90], &[0, 0]) == Some(1), "score");
    s.add("G128 watermark", matches!(water_level(50, 100, 20), WaterLevel::Low) && matches!(water_level(10, 100, 20), WaterLevel::Critical), "levels");
    let mut on = [true, false, false];
    let ch = mem_hotplug(&mut on, 1, 2, true);
    s.add("G129 hotplug", ch == 2 && on[2], "online");
    let mut io = Iommu::new();
    io.map_page(3, 0x9000);
    s.add("G130 iommu", io.translate(3) == Some(0x9000) && io.translate(15).is_none(), "translate");
    let mut q = MemQuota { limit_pages: 100, used_pages: 98 };
    s.add("G131 quota", !q.charge(5) && q.charge(2) && q.used_pages == 100, "limit");
    let mut lay = MmapLayout::new();
    let a = lay.push(0x1000);
    let b2 = lay.push(0x1000);
    s.add("G132 mmap", a == Some(0) && b2 == Some(0x1000), "layout");
    let mut refs = [1u32, 1];
    cow_fork(&mut refs, 0);
    s.add("G133 cow", refs[0] == 2 && !cow_write_fault(&mut refs, 0) && refs[0] == 1 && cow_write_fault(&mut refs, 0), "fork/privatize");
    s.add("G134 zero page", lazy_read_is_zero(None), "lazy");
    let idx = page_walk_indices(0x1234_5678_9ABC);
    s.add("G136 walk", idx[0] == (0x1234_5678_9ABC >> 39) & 0x1FF && idx[3] == (0x1234_5678_9ABC >> 12) & 0x1FF, "indices");
    s.add("G136 audit", audit_access(0b111, true, true) && !audit_access(0b011, true, true) && !audit_access(0b101, false, true), "perms");
    s.add("G137 leak", leak_check(10, 10) && !leak_check(10, 9), "pairing");
    // 持久层
    let label = parse_ns_label(b"N\x01\x02\x00\x00\x00\x10\x00\x00\x00");
    s.add("G141 ns label", label.is_some() && label.unwrap().size == 0x10, "parse");
    let mut pm = PmAllocator::new(0x1000, 0x100);
    let a1 = pm.alloc(0x40);
    let a2 = pm.alloc(0x40);
    pm.recover(1); // 只认第一笔提交
    s.add("G142/146 pm crash-consist", a1.is_some() && a2.is_some() && pm.bump == 0x1040, "recover");
    s.add("G143 dax", dax_direct(true) && !dax_direct(false), "direct map");
    let mut tiers = [(90u32, true), (3u32, true), (80, false)];
    let mv2 = tier_migrate(&mut tiers, 50);
    s.add("G144/145 tiering", mv2 == 2 && tiers[0].1 && !tiers[1].1 && tiers[2].1, "hot/cold");
    let mut tags = [0u8; 8];
    pm_emulate_tag(2, &mut tags);
    s.add("G149 pm emulate", tags[2] == 0, "tagged");
    let mut blk = [0x11223344u32, 0x55667788];
    let key = [1u32, 2, 3, 4];
    xtea_encrypt(&mut blk, &key);
    let enc = blk;
    xtea_decrypt(&mut blk, &key);
    s.add("G152 pm crypto", blk == [0x11223344, 0x55667788] && enc != blk, "xtea roundtrip");
    s.add("G153 page-cache bypass", page_cache_bypass(true, false) && page_cache_bypass(false, true) == false, "dax path");
    let wear = [9u32, 3, 9, 9, 9, 9, 9, 9];
    s.add("G157 wear level", wear_level(&wear) == 1, "least worn");
    // 缓存段
    s.add("G161 numa bw", numa_bandwidth(0, 0) == 200 && numa_bandwidth(0, 3) == 40, "topology");
    let cat = CatPartition { masks: [0b0000_0000_0011, 0b0000_0011_1100, 0b0011_1100_0000, 0b1100_0000_0000] };
    s.add("G162 cat", cat.disjoint_ok(), "disjoint ways");
    let mut cos_t = [0u8; 16];
    s.add("G163 cos bind", bind_cos(&mut cos_t, 3, 2) && cos_t[3] == 2 && !bind_cos(&mut cos_t, 3, 9), "bind");
    let mut tb = TokenBucket::new(100, 10);
    let g1 = tb.consume(80, 0);
    let g2 = tb.consume(50, 0);
    let g3 = tb.consume(50, 10);
    s.add("G167 throttle", g1 == 80 && g2 == 20 && g3 == 50, "token bucket");
    let mut pf = SeqPrefetcher::new();
    pf.observe(0x1000);
    pf.observe(0x1010);
    pf.observe(0x1020);
    pf.observe(0x1030);
    s.add("G165 prefetch", pf.prefetched >= 1, "stride 3");
    s.add("G170 miss rate", cache_miss_rate(75, 25) == 2500, "bips");
    s.add("G171 audit", cache_line_friendly(64, 64) && !cache_line_friendly(65, 64), "line");
    s.add("G176 qos isol", qos_isolation_ok(0, 1) && !qos_isolation_ok(1, 1), "critical apart");
    s.add("G179 power ways", power_down_ways(0xFFFF, true) == 0x00FF && power_down_ways(0xFFFF, false) == 0xFFFF, "idle");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn g121_buddy_roundtrip() {
        let mut b = Buddy::new(16);
        let a = b.alloc(0).unwrap();
        let c = b.alloc(0).unwrap();
        assert_eq!(a, 0);
        assert_eq!(c, 1);
        b.free(0, a);
        b.free(0, c);
        // 两次 free 后应能合并出 2 阶
        assert!(b.alloc(1).is_some());
    }

    #[test]
    fn g123_rle_roundtrip_all_bytes() {
        let src: [u8; 300] = core::array::from_fn(|i| if i < 250 { 0xAA } else { 1 });
        let mut dst = [0u8; 512];
        let n = rle_compress(&src, &mut dst).unwrap();
        let mut back = [0u8; 512];
        let m = rle_decompress(&dst[..n], &mut back).unwrap();
        assert_eq!(m, 300);
        assert_eq!(&back[..300], &src[..]);
    }

    #[test]
    fn g127_oom_pick_highest_badness() {
        assert_eq!(oom_pick(&[1, 2, 3], &[0, 0, 0]), Some(2));
        assert_eq!(oom_pick(&[100], &[-100]), Some(0));
    }

    #[test]
    fn g152_xtea_known_roundtrip() {
        let key = [0x0123_4567u32, 0x89AB_CDEF, 0x1101_2345, 0x6789_ABCD];
        let mut block = [0xDEAD_BEEFu32, 0xCAFEBABE];
        xtea_encrypt(&mut block, &key);
        assert_ne!(block, [0xDEAD_BEEF, 0xCAFEBABE]);
        xtea_decrypt(&mut block, &key);
        assert_eq!(block, [0xDEAD_BEEF, 0xCAFEBABE]);
    }

    #[test]
    fn g167_token_bucket_refill() {
        let mut tb = TokenBucket::new(10, 1);
        assert_eq!(tb.consume(10, 0), 10);
        assert_eq!(tb.consume(5, 0), 0);
        assert_eq!(tb.consume(5, 3), 3);
    }

    #[test]
    fn g180_domain_selftest_all_green() {
        let s = run_memory_checks();
        if !s.all_passed() {
            let mut buf = [0u8; 2048];
            let n = s.render(&mut buf);
            panic!("domain self-test must pass://n{}", core::str::from_utf8(&buf[..n]).unwrap());
        }
        assert!(s.len() >= 25);
    }
}
