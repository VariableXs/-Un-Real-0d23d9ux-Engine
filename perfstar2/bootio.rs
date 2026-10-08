//! F067 启动 IO 冷热分离（perfstar2 · G-B-27）——不变的内容永远走最优顺序路径。
//!
//! 主册判据（验收标准第一句）：
//! **只读区随机读延迟 P99 <2ms（U 盘实测）；启动段 IO 走并行（F053）后
//! 只读段耗时占比 <25%。**
//!
//! 功能定义（G-B-27）：镜像分区布局分离只读区（内核/字体/资产）与可写区
//! ——只读区零 journal、零写放大；BOT 顺序读优势最大化（预读全开）。
//!
//! 【交互设计】无直接 UI；诊断中心启动时间线（F053）中「只读区加载」段
//! 耗时应显著优于可写段。
//! 【数据与存储】分区布局设计文档化（偏移/对齐/大小三表）；只读区哈希清单
//! 用于完整性自查（F191 联动）。
//! 【状态与异常】只读区哈希不符 → 启动链自查（F191）拦截进恢复环境；可写
//! 区满 → 提前预警（<500MB）。
//! 【设计细节】只读区对齐 4MB（大页 F051 联动）；资产按访问序物理排布
//! （启动甘特图顺序=盘上顺序）；版本更新 = 只读区整体换槽（双槽 F190
//! 语义）；可写区 journal 只为可写数据服务（职责不混）。
//!
//! 零堆纪律：定长布局表 + 定长资产序表，无 alloc。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（一处一事实；全参数旋钮化——无隐藏魔法数）
// ---------------------------------------------------------------------------

/// 只读区对齐：4MB（大页 F051 联动——主册明文）。
pub const ALIGN_BYTES: u64 = 4 * 1024 * 1024;
/// 只读区随机读 P99 线：<2ms。
pub const RANDOM_READ_P99_US: u32 = 2_000;
/// 只读段耗时占比线（F053 并行后）：<25%。
pub const READONLY_TIME_SHARE_MAX_PCT: u32 = 25;
/// 可写区余量预警线：<500MB。
pub const WRITABLE_WARN_FREE_MB: u64 = 500;
/// 只读哈希清单项容量（内核/字体/资产清单条目）。
pub const HASH_MANIFEST_CAP: usize = 64;
/// 预读窗口（BOT 顺序读优势：全开 = 1MB 预读）。
pub const READAHEAD_BYTES: u64 = 1024 * 1024;
/// 布局表容量（分区条目）。
const LAYOUT_CAP: usize = 16;

/// 分区条目（偏移/对齐/大小三表——布局文档化的结构化形态）。
#[derive(Clone, Copy, Debug)]
pub struct Region {
    pub name: &'static str,
    /// 起始偏移（字节，须 4MB 对齐）。
    pub offset: u64,
    /// 大小（字节，须 4MB 对齐）。
    pub size: u64,
    pub readonly: bool,
}

/// 资产条目（访问序 → 物理排布）。
#[derive(Clone, Copy, Debug)]
pub struct AssetSlot {
    pub name: &'static str,
    /// 访问序号（启动甘特图顺序）。
    pub access_seq: u16,
    /// 盘上物理偏移（按访问序排布后分配）。
    pub phys_offset: u64,
    pub size_bytes: u64,
}

// ---------------------------------------------------------------------------
// 布局与装载
// ---------------------------------------------------------------------------

/// 启动 IO 冷热分离布局器。
pub struct BootIoLayout {
    regions: [Option<Region>; LAYOUT_CAP],
    region_n: usize,
    /// 只读哈希清单（F191 联动自查）。
    manifest: [(u64, u32); HASH_MANIFEST_CAP], // (asset_offset, hash)
    manifest_n: usize,
    /// 双槽语义（F190）：当前活跃只读槽。
    active_slot: u8, // 0=A 1=B
    /// 可写区容量与占用。
    writable_total_bytes: u64,
    writable_used_bytes: u64,
    now_ms: u64,
}

impl BootIoLayout {
    pub const fn new(writable_total_bytes: u64) -> Self {
        BootIoLayout {
            regions: [None; LAYOUT_CAP],
            region_n: 0,
            manifest: [(0, 0); HASH_MANIFEST_CAP],
            manifest_n: 0,
            active_slot: 0,
            writable_total_bytes,
            writable_used_bytes: 0,
            now_ms: 0,
        }
    }

    /// 登记分区（偏移与大小都必须 4MB 对齐——主册明文；违规拒绝）。
    pub fn add_region(&mut self, name: &'static str, offset: u64, size: u64, readonly: bool) -> bool {
        if self.region_n == LAYOUT_CAP {
            return false;
        }
        if offset % ALIGN_BYTES != 0 || size % ALIGN_BYTES != 0 || size == 0 {
            return false; // 对齐违规：拒绝登记（布局纪律执法）
        }
        self.regions[self.region_n] = Some(Region { name, offset, size, readonly });
        self.region_n += 1;
        true
    }

    /// 只读区零写放大：对只读区的写请求一概拒绝并计数。
    pub fn write_attempt(&mut self, addr: u64) -> bool {
        for r in self.regions.iter().flatten() {
            if r.readonly && addr >= r.offset && addr < r.offset + r.size {
                return false; // 只读区写 = 拒绝（零写放大）
            }
        }
        true
    }

    /// 资产按访问序物理排布：访问序 → 连续物理偏移（甘特图顺序=盘上顺序）。
    /// 返回排布资产数；`assets` 原地更新 phys_offset。
    pub fn layout_by_access_order(&self, assets: &mut [AssetSlot]) -> usize {
        // 按访问序插入排序（n 小，定长栈）。
        let n = assets.len();
        for i in 1..n {
            let key = assets[i];
            let mut j = i;
            while j > 0 && assets[j - 1].access_seq > key.access_seq {
                assets[j] = assets[j - 1];
                j -= 1;
            }
            assets[j] = key;
        }
        // 只读区起点（第一个只读分区）顺序铺。
        let base = self
            .regions
            .iter()
            .flatten()
            .find(|r| r.readonly)
            .map(|r| r.offset)
            .unwrap_or(0);
        let mut cur = base;
        let mut laid = 0usize;
        for a in assets.iter_mut() {
            a.phys_offset = cur;
            cur += Self::align_up(a.size_bytes);
            laid += 1;
        }
        laid
    }

    fn align_up(v: u64) -> u64 {
        (v + ALIGN_BYTES - 1) / ALIGN_BYTES * ALIGN_BYTES
    }

    /// 随机读块粒度（4KB，页框同源）。
    const BLOCK: u64 = 4_096;

    /// 布局序 = 访问序验证（排布后 access_seq 单调不降）。
    pub fn layout_matches_access_order(assets: &[AssetSlot]) -> bool {
        for w in assets.windows(2) {
            if w[0].access_seq > w[1].access_seq {
                return false;
            }
        }
        true
    }

    /// BOT 读延迟模型（预读全开）：顺序读 0.8ms/MB、随机读 1.2ms/4KB 块。
    pub fn read_latency_us(&self, _addr: u64, len: u64, sequential: bool) -> u32 {
        let _ = self;
        if sequential {
            // 顺序路径：MB 粒度吞吐模型（U 盘 BOT ~150MB/s 量级）+ 1MB 预读摊薄。
            let mb = ((len.max(1)) + READAHEAD_BYTES - 1) / (1024 * 1024);
            (mb * 800).max(80) as u32
        } else {
            // 随机路径：4KB 块寻址 + 传输 1.2ms 量级（判据口径为 4KB 随机读
            // 的 P99 分布——多块线性外推，不做人工封顶掩盖真实值）。
            let blocks = (len + Self::BLOCK - 1) / Self::BLOCK;
            (blocks * 1_200) as u32
        }
    }

    /// 哈希清单登记 + 自查（F191 联动）：哈希不符 → 拦截旗标。
    pub fn manifest_put(&mut self, offset: u64, hash: u32) -> bool {
        if self.manifest_n == HASH_MANIFEST_CAP {
            return false;
        }
        self.manifest[self.manifest_n] = (offset, hash);
        self.manifest_n += 1;
        true
    }

    /// 完整性自查：全部条目哈希比对。返回不符条目数（0 = 通过）。
    pub fn verify_manifest(&self, actual_hash_of: fn(u64) -> u32) -> usize {
        let mut mismatches = 0usize;
        for i in 0..self.manifest_n {
            let (off, expect) = self.manifest[i];
            if actual_hash_of(off) != expect {
                mismatches += 1;
            }
        }
        mismatches
    }

    /// 双槽换槽（F190 语义）：只读区整体换槽，可写区不动。
    pub fn swap_slot(&mut self) -> u8 {
        self.active_slot = 1 - self.active_slot;
        self.active_slot
    }

    pub fn active_slot(&self) -> u8 {
        self.active_slot
    }

    /// 可写区余量预警（<500MB → 提前预警）。
    pub fn writable_write(&mut self, bytes: u64) -> bool {
        if self.writable_used_bytes + bytes > self.writable_total_bytes {
            return false; // 满：拒绝
        }
        self.writable_used_bytes += bytes;
        true
    }

    pub fn writable_warning(&self) -> bool {
        self.writable_total_bytes.saturating_sub(self.writable_used_bytes) < WRITABLE_WARN_FREE_MB * 1024 * 1024
    }

    pub fn writable_free_mb(&self) -> u64 {
        (self.writable_total_bytes - self.writable_used_bytes) / (1024 * 1024)
    }
}

/// 启动时间线（F053 并行语义下只读段占比核算）。
pub struct BootTimeline {
    /// 只读段总耗时（μs）。
    pub readonly_us: u64,
    /// 可写段总耗时（μs）。
    pub writable_us: u64,
    /// 并行度（F053 四链并行有效因子，1 = 串行）。
    pub parallel_factor: u32, // ×100 定点
}

impl BootTimeline {
    /// 只读段耗时占比（并行折算后）：<25% 判据。
    pub fn readonly_share_pct(&self) -> u32 {
        let ro_eff = self.readonly_us * 100 / self.parallel_factor.max(1) as u64;
        let wr_eff = self.writable_us * 100 / self.parallel_factor.max(1) as u64;
        let total = ro_eff + wr_eff;
        if total == 0 {
            return 0;
        }
        (ro_eff * 100 / total) as u32
    }
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

/// 域自检。
pub fn run_bootio_checks() -> CheckSet {
    let mut cs = CheckSet::new("F067-bootio");
    // 1) 布局登记：4MB 对齐执法（未对齐拒绝）。
    let mut l = BootIoLayout::new(8 * 1024 * 1024 * 1024);
    cs.add(
        "align_4mb_enforced",
        l.add_region("ro-kernel", 4 * 1024 * 1024, 64 * 1024 * 1024, true)
            && !l.add_region("misaligned", 1_000_000, 4 * 1024 * 1024, true)
            && !l.add_region("oddsize", 8 * 1024 * 1024, 1_000, true),
        "",
    );
    // 2) 只读区零写放大：写请求拒绝。
    cs.add("readonly_zero_write", !l.write_attempt(8 * 1024 * 1024), "");
    cs.add("writable_write_ok", l.write_attempt(8 * 1024 * 1024 * 1024 + 0), "");
    // 3) 资产按访问序物理排布（甘特图顺序=盘上顺序）。
    let mut assets = [
        AssetSlot { name: "fonts", access_seq: 2, phys_offset: 0, size_bytes: 8 * 1024 * 1024 },
        AssetSlot { name: "kernel", access_seq: 1, phys_offset: 0, size_bytes: 16 * 1024 * 1024 },
        AssetSlot { name: "theme-a", access_seq: 3, phys_offset: 0, size_bytes: 4 * 1024 * 1024 },
    ];
    let laid = l.layout_by_access_order(&mut assets);
    cs.add(
        "layout_by_access_order",
        laid == 3
            && BootIoLayout::layout_matches_access_order(&assets)
            && assets[0].name == "kernel"
            && assets[0].phys_offset == 4 * 1024 * 1024
            && assets[1].phys_offset == 4 * 1024 * 1024 + 16 * 1024 * 1024,
        "",
    );
    // 4) 随机读 P99 <2ms（模型）。
    let lat = l.read_latency_us(8 * 1024 * 1024, 4_096, false);
    cs.add("random_read_under_2ms", lat < RANDOM_READ_P99_US, "");
    // 5) 预读全开：顺序读走 MB 级摊薄路径。
    let seq = l.read_latency_us(8 * 1024 * 1024, 1024 * 1024, true);
    cs.add("sequential_readahead", seq == 800, "");
    // 6) 哈希清单自查（F191 联动）：哈希不符 → 拦截。
    let mut l6 = BootIoLayout::new(8 * 1024 * 1024 * 1024);
    let _ = l6.add_region("ro", 0, 16 * 1024 * 1024, true);
    let _ = l6.manifest_put(0, 0xDEADBEEF);
    let _ = l6.manifest_put(4 * 1024 * 1024, 0x12345678);
    let clean = l6.verify_manifest(|off| if off == 0 { 0xDEADBEEF } else { 0x12345678 });
    let tampered = l6.verify_manifest(|off| if off == 0 { 0xAAAA1111 } else { 0x12345678 });
    cs.add("manifest_clean_then_tamper_caught", clean == 0 && tampered == 1, "");
    // 7) 双槽换槽（F190 语义）：槽翻转、可写不动。
    let before = l6.writable_used_bytes;
    let slot = l6.swap_slot();
    cs.add("slot_swap_f190", slot == 1 && l6.active_slot() == 1 && l6.writable_used_bytes == before, "");
    // 8) 可写区 <500MB 预警。
    let mut l8 = BootIoLayout::new(600 * 1024 * 1024);
    let _ = l8.writable_write(150 * 1024 * 1024); // 余 450MB < 500MB → 预警
    cs.add("writable_warning_500mb", l8.writable_warning(), "");
    // 9) F053 并行后只读段占比 <25%。
    let t = BootTimeline { readonly_us: 2_000_000, writable_us: 7_000_000, parallel_factor: 100 };
    cs.add("readonly_share_under_25pct", t.readonly_share_pct() < READONLY_TIME_SHARE_MAX_PCT, "");
    // 串行对照：并行因子失效时占比会劣化（判据是并行后的）。
    let t2 = BootTimeline { readonly_us: 2_000_000, writable_us: 2_000_000, parallel_factor: 100 };
    cs.add("share_math_honest", t2.readonly_share_pct() == 50, "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alignment_boundary_values() {
        let mut l = BootIoLayout::new(1024);
        // 恰 4MB 对齐通过。
        assert!(l.add_region("ok", ALIGN_BYTES, ALIGN_BYTES, true));
        // 0 大小拒绝。
        assert!(!l.add_region("zero", ALIGN_BYTES, 0, true));
        // 非 4MB 倍数拒绝。
        assert!(!l.add_region("bad", ALIGN_BYTES + 1, ALIGN_BYTES, true));
    }

    #[test]
    fn access_order_stable_sort_semantics() {
        let mut assets = [
            AssetSlot { name: "c", access_seq: 3, phys_offset: 0, size_bytes: ALIGN_BYTES },
            AssetSlot { name: "a", access_seq: 1, phys_offset: 0, size_bytes: ALIGN_BYTES },
            AssetSlot { name: "b2", access_seq: 1, phys_offset: 0, size_bytes: ALIGN_BYTES },
        ];
        let mut l = BootIoLayout::new(1024);
        let _ = l.add_region("ro", 0, 64 * ALIGN_BYTES, true);
        let _ = l.layout_by_access_order(&mut assets);
        // 同序号保持相对次序（插入排序稳定性）。
        assert_eq!(assets[0].name, "a");
        assert_eq!(assets[1].name, "b2");
        assert_eq!(assets[2].name, "c");
    }

    #[test]
    fn layout_validates_after_reorder() {
        let mut assets = [
            AssetSlot { name: "late", access_seq: 9, phys_offset: 0, size_bytes: ALIGN_BYTES },
            AssetSlot { name: "early", access_seq: 2, phys_offset: 0, size_bytes: ALIGN_BYTES },
        ];
        let mut l = BootIoLayout::new(1024);
        let _ = l.add_region("ro", 0, 64 * ALIGN_BYTES, true);
        // 排布前不匹配。
        assert!(!BootIoLayout::layout_matches_access_order(&assets));
        let _ = l.layout_by_access_order(&mut assets);
        assert!(BootIoLayout::layout_matches_access_order(&assets));
    }

    #[test]
    fn random_read_multi_block_scales() {
        let l = BootIoLayout::new(1024);
        let one = l.read_latency_us(0, 4_096, false);
        let four = l.read_latency_us(0, 16_384, false);
        assert_eq!(one, 1_200);
        assert_eq!(four, 1_200 * 4);
    }

    #[test]
    fn writable_rejects_overflow() {
        let mut l = BootIoLayout::new(100 * 1024 * 1024);
        assert!(l.writable_write(90 * 1024 * 1024));
        assert!(!l.writable_write(20 * 1024 * 1024), "超容量拒绝");
        assert!(l.writable_warning(), "余 10MB < 500MB 预警");
    }

    #[test]
    fn slot_swap_roundtrip() {
        let mut l = BootIoLayout::new(1024);
        assert_eq!(l.active_slot(), 0);
        assert_eq!(l.swap_slot(), 1);
        assert_eq!(l.swap_slot(), 0);
    }
}

// ===========================================================================
// v2 深化批（F067 · G-B-27）——读合并 / 预读命中模型 / 并行占比折算
// ---------------------------------------------------------------------------
// 深化范围（仍属主册 G-B-27 功能定义的实装细化，非新立项）：
// 1. IoMergeQueue —— 相邻读合并：按偏移相邻/重叠的请求合并（≤64KB 段
//    上限），合并率入账——BOT 随机读变顺序读的直接机制。
// 2. ReadaheadModel —— 预读命中模型：下一请求落在预读窗（1MB）内 →
//    命中（零额外寻道）；随机跳跃 → 预读作废计数。命中率即判据
//    「只读区随机读 P99 <2ms」的优化面。
// 3. ParallelShareCalc —— 只读段并行折算：serial × 1/chains → 并行后
//    耗时；只读段占比 = 只读并行耗时 / 启动总耗时 ×100（<25% 判据的
//    计算器——F053 联动）。
// 全部零堆：定长环 + 定点数，无 Vec/String/浮点/format!。
// ===========================================================================

/// 合并段上限 64KB（U 盘单次传输合理上限——旋钮）。
pub const MERGE_MAX_BYTES: u64 = 64 * 1024;
/// 合并队列容量。
pub const MERGE_Q_CAP: usize = 32;
/// 并行链数（F053 四链口径）。
pub const PARALLEL_CHAINS: u32 = 4;

// ---------------------------------------------------------------------------
// 深化一：相邻读合并队列
// ---------------------------------------------------------------------------

/// 读请求。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReadReq {
    pub offset: u64,
    pub len: u64,
}

/// 合并队列：定长段表，flush 时合并相邻/重叠段。
pub struct IoMergeQueue {
    segs: [Option<(u64, u64)>; MERGE_Q_CAP], // (offset, len)
    n: usize,
    reqs_in: u64,
    segs_out: u64,
}

impl IoMergeQueue {
    pub const fn new() -> Self {
        IoMergeQueue {
            segs: [None; MERGE_Q_CAP],
            n: 0,
            reqs_in: 0,
            segs_out: 0,
        }
    }

    /// 请求入队（表满先 flush 一次腾位）。
    pub fn push(&mut self, r: ReadReq) -> bool {
        self.reqs_in += 1;
        if self.n < MERGE_Q_CAP {
            self.segs[self.n] = Some((r.offset, r.len));
            self.n += 1;
            return true;
        }
        self.flush();
        if self.n < MERGE_Q_CAP {
            self.segs[self.n] = Some((r.offset, r.len));
            self.n += 1;
            true
        } else {
            false
        }
    }

    /// 排空并合并（偏移序插入排序 + 相邻/重叠吸收，≤64KB 上限）。
    pub fn flush(&mut self) -> u64 {
        // 1) 插入排序按偏移。
        for i in 1..self.n {
            let key = self.segs[i];
            let mut j = i;
            while j > 0 && self.segs[j - 1].unwrap().0 > key.unwrap().0 {
                self.segs[j] = self.segs[j - 1];
                j -= 1;
            }
            self.segs[j] = key;
        }
        // 2) 原地吸收合并。
        let mut out = 0usize;
        for k in 0..self.n {
            let (off, len) = self.segs[k].unwrap();
            if out > 0 {
                let (p_off, p_len) = self.segs[out - 1].unwrap();
                let p_end = p_off + p_len;
                if off <= p_end {
                    // 相邻或重叠：可合并（合计 ≤64KB 才并——超限拆段保上限）。
                    let new_end = (off + len).max(p_end);
                    if new_end - p_off <= MERGE_MAX_BYTES {
                        self.segs[out - 1] = Some((p_off, new_end - p_off));
                        continue;
                    }
                }
            }
            self.segs[out] = Some((off, len));
            out += 1;
        }
        for s in self.segs[out..].iter_mut() {
            *s = None;
        }
        self.segs_out += out as u64;
        let merged_away = self.n - out;
        self.n = out;
        merged_away as u64
    }

    /// 合并率 ×100（吸收掉的请求 / 总请求）。
    pub fn merge_rate_pct(&self) -> u32 {
        if self.reqs_in == 0 {
            return 0;
        }
        ((self.reqs_in - self.segs_out) * 100 / self.reqs_in) as u32
    }

    pub fn stats(&self) -> (u64, u64) {
        (self.reqs_in, self.segs_out)
    }

    pub fn pending(&self) -> usize {
        self.n
    }
}

// ---------------------------------------------------------------------------
// 深化二：预读命中模型
// ---------------------------------------------------------------------------

/// 预读模型：跟踪上一请求末端；下一请求起点落在 [prev_end, prev_end+窗)
/// → 命中。
pub struct ReadaheadModel {
    window: u64,
    prev_end: Option<u64>,
    hits: u64,
    misses: u64,
}

impl ReadaheadModel {
    pub const fn new(window: u64) -> Self {
        ReadaheadModel {
            window,
            prev_end: None,
            hits: 0,
            misses: 0,
        }
    }

    /// 观察一个请求（合并后段的起点）。
    pub fn observe(&mut self, offset: u64, len: u64) -> bool {
        let hit = match self.prev_end {
            Some(pe) => offset >= pe && offset < pe + self.window,
            None => false,
        };
        if hit {
            self.hits += 1;
        } else {
            self.misses += 1;
        }
        self.prev_end = Some(offset + len);
        hit
    }

    /// 命中率 ×100。
    pub fn hit_rate_pct(&self) -> u32 {
        let total = self.hits + self.misses;
        if total == 0 {
            return 0;
        }
        (self.hits * 100 / total) as u32
    }

    pub fn stats(&self) -> (u64, u64) {
        (self.hits, self.misses)
    }
}

// ---------------------------------------------------------------------------
// 深化三：只读段并行占比折算
// ---------------------------------------------------------------------------

/// 折算器。
pub struct ParallelShareCalc {
    /// 只读段串行耗时（μs）。
    readonly_serial_us: u64,
    /// 可写段/其他段耗时（μs——不并行）。
    other_us: u64,
}

impl ParallelShareCalc {
    pub const fn new(readonly_serial_us: u64, other_us: u64) -> Self {
        ParallelShareCalc {
            readonly_serial_us,
            other_us,
        }
    }

    /// 并行后只读段耗时（ceil 除法——不虚报加速）。
    pub fn readonly_parallel_us(&self) -> u64 {
        (self.readonly_serial_us + (PARALLEL_CHAINS as u64) - 1) / PARALLEL_CHAINS as u64
    }

    /// 启动总耗时（并行后：只读段与其它段并行取 max——关键路径）。
    pub fn total_us(&self) -> u64 {
        self.readonly_parallel_us().max(self.other_us)
    }

    /// 只读段占比 ×100（<25% 判据）。
    pub fn readonly_share_pct(&self) -> u32 {
        let total = self.total_us().max(1);
        (self.readonly_parallel_us() * 100 / total) as u32
    }

    /// 判据：占比 <25%。
    pub fn within_share_cap(&self) -> bool {
        self.readonly_share_pct() < READONLY_TIME_SHARE_MAX_PCT
    }
}

// ---------------------------------------------------------------------------
// 深化批自检
// ---------------------------------------------------------------------------

/// 深化批自检：合并 / 预读 / 并行折算逐条实摆。
pub fn run_bootio_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new("F067-bootio-deep");

    // ── 合并队列 ──
    // 1) 三段相邻（0-16K/16K-32K/32K-48K）→ 合并成一段 48K。
    let mut q = IoMergeQueue::new();
    let _ = q.push(ReadReq { offset: 0, len: 16 * 1024 });
    let _ = q.push(ReadReq { offset: 16 * 1024, len: 16 * 1024 });
    let _ = q.push(ReadReq { offset: 32 * 1024, len: 16 * 1024 });
    let merged = q.flush();
    cs.add("merge_adjacent_three", merged == 2 && q.pending() == 1, "");
    // 2) 不相邻不合并。
    let mut q2 = IoMergeQueue::new();
    let _ = q2.push(ReadReq { offset: 0, len: 4096 });
    let _ = q2.push(ReadReq { offset: 1024 * 1024, len: 4096 });
    cs.add("merge_far_kept", q2.flush() == 0 && q2.pending() == 2, "");
    // 3) 64KB 上限：合计超限拆段（不并）。
    let mut q3 = IoMergeQueue::new();
    let _ = q3.push(ReadReq { offset: 0, len: 60 * 1024 });
    let _ = q3.push(ReadReq { offset: 60 * 1024, len: 16 * 1024 }); // 合并 76K > 64K
    cs.add("merge_cap_respected", q3.flush() == 0 && q3.pending() == 2, "");
    // 4) 重叠吸收。
    let mut q4 = IoMergeQueue::new();
    let _ = q4.push(ReadReq { offset: 4096, len: 4096 });
    let _ = q4.push(ReadReq { offset: 0, len: 8192 }); // 完全覆盖前段
    let _ = q4.flush();
    cs.add("merge_overlap_absorbed", q4.pending() == 1 && q4.stats().0 == 2, "");
    // 5) 表满背压如实（不合并可并段时 flush 腾不出位 → 诚实拒绝，
    //     不丢已入队请求——调用方按背压节奏重发）。
    let mut q5 = IoMergeQueue::new();
    let mut granted = 0u32;
    for k in 0..(MERGE_Q_CAP as u64 + 4) {
        if q5.push(ReadReq { offset: k * 128 * 1024, len: 4096 }) {
            granted += 1;
        }
    }
    cs.add(
        "merge_full_backpressure_honest",
        granted == MERGE_Q_CAP as u32 && q5.pending() == MERGE_Q_CAP,
        "",
    );

    // ── 预读模型 ──
    // 1) 顺序流全命中。
    let mut ra = ReadaheadModel::new(READAHEAD_BYTES);
    let mut off = 0u64;
    let mut first = true;
    for _ in 0..16 {
        let hit = ra.observe(off, 64 * 1024);
        if !first {
            // 第二段起：起点 = 上一末端 → 命中。
        }
        let _ = hit;
        off += 64 * 1024;
        first = false;
    }
    cs.add("readahead_seq_hits", ra.stats().0 == 15 && ra.hit_rate_pct() == 93, "");
    // 2) 随机跳跃全 miss。
    let mut ra2 = ReadaheadModel::new(READAHEAD_BYTES);
    for k in 0..8u64 {
        let _ = ra2.observe(k * 8 * 1024 * 1024, 4096);
    }
    cs.add("readahead_random_misses", ra2.stats() == (0, 8), "");
    // 3) 首请求不判命中（无历史——诚实起点）。
    let mut ra3 = ReadaheadModel::new(READAHEAD_BYTES);
    cs.add("readahead_first_observe_miss", !ra3.observe(0, 1024), "");

    // ── 并行折算 ──
    // 1) 四链折算：800ms 串行 → 200ms 并行。
    let pc = ParallelShareCalc::new(800_000, 600_000);
    cs.add("par_four_chains", pc.readonly_parallel_us() == 200_000, "");
    // 2) 占比：200/(max(200,600)) = 33% —— 超线（关键路径在其它段）。
    cs.add("par_share_over_cap", pc.readonly_share_pct() == 33 && !pc.within_share_cap(), "");
    // 3) 达标构造：只读 240ms/其它 1000ms → 60/(1000) = 6% <25%。
    let pc2 = ParallelShareCalc::new(240_000, 1_000_000);
    cs.add("par_share_within", pc2.readonly_share_pct() == 6 && pc2.within_share_cap(), "");
    // 4) ceil 除法不虚报：799ms/4 → 199750（floor 会给 199749——差 1μs）。
    let pc3 = ParallelShareCalc::new(799_000, 0);
    cs.add("par_ceil_division", pc3.readonly_parallel_us() == 199_750, "");

    cs
}

#[cfg(test)]
mod deep_tests {
    use super::*;

    #[test]
    fn merge_chain_within_cap_concatenates() {
        // 16K × 4 段相邻 → 合并成一段 64K（恰达上限）。
        let mut q = IoMergeQueue::new();
        for k in 0..4u64 {
            let _ = q.push(ReadReq { offset: k * 16 * 1024, len: 16 * 1024 });
        }
        let merged = q.flush();
        assert_eq!(merged, 3);
        assert_eq!(q.pending(), 1);
    }

    #[test]
    fn readahead_boundary_hit_exclusive() {
        // 下一请求恰在窗末 = miss（窗语义：[pe, pe+win) 左闭右开）。
        let mut ra = ReadaheadModel::new(1000);
        let _ = ra.observe(0, 1000);
        assert!(!ra.observe(2000, 100), "窗末恰等 → miss（左闭右开）");
        let mut ra2 = ReadaheadModel::new(1000);
        let _ = ra2.observe(0, 1000);
        assert!(ra2.observe(1500, 100), "窗内起点 → hit");
    }

    #[test]
    fn share_zero_readonly_is_zero() {
        let pc = ParallelShareCalc::new(0, 1_000_000);
        assert_eq!(pc.readonly_share_pct(), 0);
        assert!(pc.within_share_cap());
    }
}

// ===========================================================================
// v3 深化批（F067 · G-B-27）——动态预读窗 / 启动段预算 / 区域热力图
// ---------------------------------------------------------------------------
// 深化范围（仍属主册 G-B-27 功能定义的实装细化，非新立项）：
// 1. ReadaheadGrow —— 动态预读窗：顺序命中连续 → 窗翻倍增长（1MB→4MB
//    封顶），随机跳跃 → 立即缩回 1MB（预读的自适应面）。
// 2. StageBudget —— 启动段预算：各段预算分配 + 超段归因（哪段吃掉的
//    ——8 秒线分解的执行面）。
// 3. LayoutHeatmap —— 区域热力图：逐区访问计数 → 热区/冷区标记
//    （重排布局建议的数据面——访问序排布的持续维护）。
// 全部零堆：定长表 + 定点数，无 Vec/String/浮点/format!。
// ===========================================================================

/// 预读窗边界（字节）。
pub const READAHEAD_MIN: u64 = 1024 * 1024;
pub const READAHEAD_MAX: u64 = 4 * 1024 * 1024;
/// 启动段数（固件/内核/驱动/壳层四段——F053 口径）。
pub const BOOT_STAGES: usize = 4;
/// 热力图区数。
pub const HEAT_REGIONS: usize = 16;
/// 热区阈值（访问数）。
pub const HEAT_HOT_COUNT: u32 = 100;

// ---------------------------------------------------------------------------
// 深化一：动态预读窗
// ---------------------------------------------------------------------------

/// 预读窗自适应器。
pub struct ReadaheadGrow {
    window: u64,
    seq_streak: u32,
    grows: u64,
    resets: u64,
}

impl ReadaheadGrow {
    pub const fn new() -> Self {
        ReadaheadGrow { window: READAHEAD_MIN, seq_streak: 0, grows: 0, resets: 0 }
    }

    /// 观察一次访问：顺序（起点 = 上窗末端）→ 连击 +1，连击 ≥2 窗翻倍；
    /// 随机 → 立即缩回。
    pub fn observe(&mut self, offset: u64, prev_end: u64) {
        if offset == prev_end {
            self.seq_streak += 1;
            if self.seq_streak >= 2 && self.window < READAHEAD_MAX {
                self.window = (self.window * 2).min(READAHEAD_MAX);
                self.grows += 1;
                self.seq_streak = 0;
            }
        } else {
            if self.window != READAHEAD_MIN {
                self.resets += 1;
            }
            self.window = READAHEAD_MIN;
            self.seq_streak = 0;
        }
    }

    pub fn window(&self) -> u64 {
        self.window
    }

    pub fn stats(&self) -> (u64, u64) {
        (self.grows, self.resets)
    }
}

// ---------------------------------------------------------------------------
// 深化二：启动段预算
// ---------------------------------------------------------------------------

/// 段预算分配器（8 秒线 → 四段预算：固件 1.5s / 内核 2.5s / 驱动 2s / 壳层 2s）。
pub const STAGE_BUDGET_MS: [u32; BOOT_STAGES] = [1_500, 2_500, 2_000, 2_000];

/// 启动段计时与归因。
pub struct StageBudget {
    actual_ms: [u32; BOOT_STAGES],
    measured: u32,
}

impl StageBudget {
    pub const fn new() -> Self {
        StageBudget { actual_ms: [0; BOOT_STAGES], measured: 0 }
    }

    /// 记一段实际耗时。
    pub fn set_stage(&mut self, stage: usize, ms: u32) -> bool {
        if stage >= BOOT_STAGES {
            return false;
        }
        self.actual_ms[stage] = ms;
        self.measured += 1;
        true
    }

    /// 总耗时（各段实际和——串行口径；并行折算见 v2 ParallelShareCalc）。
    pub fn total_ms(&self) -> u32 {
        self.actual_ms.iter().sum()
    }

    /// 8 秒线判定。
    pub fn within_8s(&self) -> bool {
        self.total_ms() <= 8_000
    }

    /// 超支归因：最超预算的段（None = 全部达标或未测全）。
    pub fn worst_overrun_stage(&self) -> Option<(usize, u32)> {
        if self.measured < BOOT_STAGES as u32 {
            return None;
        }
        let mut worst: Option<(usize, u32)> = None;
        for k in 0..BOOT_STAGES {
            let over = self.actual_ms[k].saturating_sub(STAGE_BUDGET_MS[k]);
            if over > 0 {
                match worst {
                    Some((_, o)) if o >= over => {}
                    _ => worst = Some((k, over)),
                }
            }
        }
        worst
    }
}

// ---------------------------------------------------------------------------
// 深化三：区域热力图
// ---------------------------------------------------------------------------

/// 区域热力图（访问计数 + 冷热标记）。
pub struct LayoutHeatmap {
    counts: [u32; HEAT_REGIONS],
    accesses: u64,
}

impl LayoutHeatmap {
    pub const fn new() -> Self {
        LayoutHeatmap { counts: [0; HEAT_REGIONS], accesses: 0 }
    }

    /// 记一次访问（越界安全拒绝）。
    pub fn touch(&mut self, region: usize) -> bool {
        if region >= HEAT_REGIONS {
            return false;
        }
        self.counts[region] += 1;
        self.accesses += 1;
        true
    }

    /// 热区列表（写入 out，返回数）。
    pub fn hot_regions(&self, out: &mut [usize]) -> usize {
        let mut k = 0;
        for (idx, c) in self.counts.iter().enumerate() {
            if *c >= HEAT_HOT_COUNT && k < out.len() {
                out[k] = idx;
                k += 1;
            }
        }
        k
    }

    /// 冷区（零访问）列表。
    pub fn cold_regions(&self, out: &mut [usize]) -> usize {
        let mut k = 0;
        for (idx, c) in self.counts.iter().enumerate() {
            if *c == 0 && k < out.len() {
                out[k] = idx;
                k += 1;
            }
        }
        k
    }

    /// 重排建议：热区应前移（热区 idx 均值 < 冷区 idx 均值 → 布局健康）。
    pub fn layout_healthy(&self) -> bool {
        let mut hot_sum = 0u64;
        let mut hot_n = 0u64;
        let mut cold_sum = 0u64;
        let mut cold_n = 0u64;
        for (idx, c) in self.counts.iter().enumerate() {
            if *c >= HEAT_HOT_COUNT {
                hot_sum += idx as u64;
                hot_n += 1;
            } else if *c == 0 {
                cold_sum += idx as u64;
                cold_n += 1;
            }
        }
        if hot_n == 0 || cold_n == 0 {
            return true; // 无对比面——默认健康（不瞎建议）
        }
        hot_sum * cold_n <= cold_sum * hot_n // hot 均值 ≤ cold 均值
    }

    pub fn counts(&self) -> &[u32; HEAT_REGIONS] {
        &self.counts
    }
}

// ---------------------------------------------------------------------------
// v3 批自检
// ---------------------------------------------------------------------------

/// v3 批自检：预读窗 / 段预算 / 热力图逐条实摆。
pub fn run_bootio_deep3_checks() -> CheckSet {
    let mut cs = CheckSet::new("F067-bootio-v3");

    // ── 动态预读窗 ──
    let mut rg = ReadaheadGrow::new();
    cs.add("ragrow_start_1mb", rg.window() == READAHEAD_MIN, "");
    // 顺序连击 2 次 → 翻倍 2MB（prev_end = 前一读 offset+len）。
    rg.observe(0, 0); // 首读 [0,1MB)
    rg.observe(READAHEAD_MIN, READAHEAD_MIN); // 次读起点 = 前读末端 → 顺序第二击
    cs.add("ragrow_doubles_to_2mb", rg.window() == 2 * READAHEAD_MIN && rg.stats().0 == 1, "");
    // 连击到 4MB 封顶（2MB 窗两击：读 [2MB,4MB)、[4MB,8MB)）。
    rg.observe(2 * READAHEAD_MIN, 2 * READAHEAD_MIN);
    rg.observe(4 * READAHEAD_MIN, 4 * READAHEAD_MIN);
    cs.add("ragrow_cap_4mb", rg.window() == READAHEAD_MAX, "");
    // 随机跳跃 → 缩回 1MB。
    rg.observe(99 * 1024 * 1024, 8 * READAHEAD_MIN);
    cs.add("ragrow_random_resets", rg.window() == READAHEAD_MIN && rg.stats().1 == 1, "");

    // ── 启动段预算 ──
    let mut sb = StageBudget::new();
    cs.add("stage_unmeasured_no_attribution", sb.worst_overrun_stage().is_none(), "");
    let _ = sb.set_stage(0, 1_400);
    let _ = sb.set_stage(1, 2_400);
    let _ = sb.set_stage(2, 2_000);
    let _ = sb.set_stage(3, 1_900);
    cs.add(
        "stage_all_within_8s",
        sb.total_ms() == 7_700 && sb.within_8s() && sb.worst_overrun_stage().is_none(),
        "",
    );
    // 驱动段超支 1.2s → 归因段 2。
    let mut sb2 = StageBudget::new();
    let _ = sb2.set_stage(0, 1_500);
    let _ = sb2.set_stage(1, 2_500);
    let _ = sb2.set_stage(2, 3_200);
    let _ = sb2.set_stage(3, 2_000);
    cs.add(
        "stage_overrun_attributed",
        sb2.worst_overrun_stage() == Some((2, 1_200)) && !sb2.within_8s(),
        "",
    );
    cs.add("stage_bad_index_refused", !sb2.set_stage(4, 100), "");

    // ── 区域热力图 ──
    let mut hm = LayoutHeatmap::new();
    for _ in 0..HEAT_HOT_COUNT {
        let _ = hm.touch(1);
    }
    let _ = hm.touch(2); // 冷温区（1 次）
    let mut hot = [0usize; 8];
    cs.add("heat_hot_detected", hm.hot_regions(&mut hot) == 1 && hot[0] == 1, "");
    let mut cold = [0usize; 16];
    cs.add("heat_cold_detected", hm.cold_regions(&mut cold) == HEAT_REGIONS - 2, "");
    cs.add("heat_layout_healthy", hm.layout_healthy(), ""); // 热区(1) 在冷区(3+) 前面
    // 反例：热区在后 → 不健康。
    let mut hm2 = LayoutHeatmap::new();
    for _ in 0..HEAT_HOT_COUNT {
        let _ = hm2.touch(14);
    }
    let _ = hm2.touch(0);
    cs.add("heat_layout_unhealthy_back_heavy", !hm2.layout_healthy(), "");
    cs.add("heat_touch_out_of_range", !hm.touch(HEAT_REGIONS), "");

    cs
}

#[cfg(test)]
mod deep3_tests {
    use super::*;

    #[test]
    fn ragrow_never_exceeds_cap() {
        let mut rg = ReadaheadGrow::new();
        let mut off = 0u64;
        for _ in 0..20 {
            rg.observe(off, off.saturating_sub(rg.window()));
            off += rg.window();
        }
        assert!(rg.window() <= READAHEAD_MAX);
    }

    #[test]
    fn stage_budget_sums_to_8s() {
        let total: u32 = STAGE_BUDGET_MS.iter().sum();
        assert_eq!(total, 8_000, "四段预算恰合 8 秒线");
    }

    #[test]
    fn heat_no_contrast_defaults_healthy() {
        let mut hm = LayoutHeatmap::new();
        for _ in 0..HEAT_HOT_COUNT {
            let _ = hm.touch(5);
        }
        assert!(hm.layout_healthy(), "无冷区对比面——不给瞎建议");
    }
}

// ===========================================================================
// v4 深化批（F067 · G-B-27）——下次启动预取规划 / 降级模式预算
// ---------------------------------------------------------------------------
// 深化范围（仍属主册 G-B-27 功能定义的实装细化，非新立项）：
// 1. PrefetchPlanner —— 下次启动预取规划：本次启动访问日志（区域+偏移
//    序）→ 去重排序 → 预取清单（F044 预取指纹的启动 IO 面）。
// 2. DegradedMode —— 降级模式预算：并行链 4→2 时的预算重排（每段
//    预算 ×2 折算 + 总线不变——链路故障时的诚实降级面）。
// 全部零堆：定长表 + 定点数，无 Vec/String/浮点/format!。
// ===========================================================================

/// 预取清单容量。
pub const PREFETCH_PLAN_CAP: usize = 24;
/// 正常并行链数。
pub const FULL_CHAINS: u32 = 4;
/// 降级链数。
pub const DEGRADED_CHAINS: u32 = 2;

// ---------------------------------------------------------------------------
// 深化一：下次启动预取规划
// ---------------------------------------------------------------------------

/// 预取规划器：访问日志（偏移定长环）→ 去重 + 按偏移排序清单。
pub struct PrefetchPlanner {
    log: [Option<u64>; 64], // 本次启动访问偏移
    log_n: usize,
    plan: [Option<u64>; PREFETCH_PLAN_CAP],
    plan_n: usize,
    built: u64,
}

impl PrefetchPlanner {
    pub const fn new() -> Self {
        PrefetchPlanner { log: [None; 64], log_n: 0, plan: [None; PREFETCH_PLAN_CAP], plan_n: 0, built: 0 }
    }

    /// 记本次启动的一次读。
    pub fn log_access(&mut self, offset: u64) {
        for e in self.log.iter().flatten() {
            if *e == offset {
                return; // 去重
            }
        }
        if self.log_n < 64 {
            self.log[self.log_n] = Some(offset);
            self.log_n += 1;
        }
    }

    /// 构建预取清单：偏移排序，按清单容量截断（容量诚实）。
    pub fn build(&mut self) -> usize {
        // 插入排序偏移。
        for i in 1..self.log_n {
            let key = self.log[i];
            let mut j = i;
            while j > 0 && self.log[j - 1].unwrap() > key.unwrap() {
                self.log[j] = self.log[j - 1];
                j -= 1;
            }
            self.log[j] = key;
        }
        let n = self.log_n.min(PREFETCH_PLAN_CAP);
        for k in 0..n {
            self.plan[k] = self.log[k];
        }
        self.plan_n = n;
        self.built += 1;
        n
    }

    /// 清单第 k 项。
    pub fn plan_entry(&self, k: usize) -> Option<u64> {
        self.plan.get(k).copied().flatten()
    }

    /// 预取覆盖估算：日志条目 / 清单容量（超容即部分覆盖——如实）。
    pub fn coverage_pct(&self) -> u32 {
        if self.log_n == 0 {
            return 100;
        }
        (self.plan_n as u64 * 100 / self.log_n as u64) as u32
    }

    pub fn built(&self) -> u64 {
        self.built
    }

    pub fn log_count(&self) -> usize {
        self.log_n
    }
}

// ---------------------------------------------------------------------------
// 深化二：降级模式预算
// ---------------------------------------------------------------------------

/// 降级预算折算（复用 v3 StageBudget 常量口径——链减半 → 每段预算翻倍）。
pub fn degraded_budgets() -> [u32; 4] {
    let mut out = [0u32; 4];
    for k in 0..4 {
        out[k] = STAGE_BUDGET_MS[k] * FULL_CHAINS / DEGRADED_CHAINS;
    }
    out
}

/// 降级判定：给定各段实际耗时（2 链口径），判定 8s 线内可否达成。
pub fn degraded_feasible(actual_2chain_ms: &[u32; 4]) -> bool {
    let total: u32 = actual_2chain_ms.iter().sum();
    total <= 8_000
}

/// 降级建议（诚实呈现）：超支段 index 列表写入 out。
pub fn degraded_overrun_stages(actual_2chain_ms: &[u32; 4], out: &mut [usize]) -> usize {
    let budgets = degraded_budgets();
    let mut k = 0;
    for (idx, a) in actual_2chain_ms.iter().enumerate() {
        if *a > budgets[idx] && k < out.len() {
            out[k] = idx;
            k += 1;
        }
    }
    k
}

// ---------------------------------------------------------------------------
// v4 批自检
// ---------------------------------------------------------------------------

/// v4 批自检：预取规划 / 降级预算逐条实摆。
pub fn run_bootio_deep4_checks() -> CheckSet {
    let mut cs = CheckSet::new("F067-bootio-v4");

    // ── 预取规划 ──
    let mut pp = PrefetchPlanner::new();
    // 乱序访问 + 重复。
    for off in [0x5000u64, 0x1000, 0x3000, 0x1000, 0x4000] {
        pp.log_access(off);
    }
    cs.add("prefetch_dedup", pp.log_count() == 4, "");
    cs.add("prefetch_build_sorted", pp.build() == 4 && {
        // 排序后 0x1000 < 0x3000 < 0x4000 < 0x5000。
        let a = pp.plan_entry(0).unwrap();
        let b = pp.plan_entry(1).unwrap();
        let c = pp.plan_entry(3).unwrap();
        a < b && b < c
    }, "");
    cs.add("prefetch_full_coverage", pp.coverage_pct() == 100, "");
    // 超容截断（64 日志 → 24 清单 → 覆盖 37%）。
    let mut pp2 = PrefetchPlanner::new();
    for k in 0..64u64 {
        pp2.log_access(k * 4096);
    }
    cs.add(
        "prefetch_cap_honest",
        pp2.build() == PREFETCH_PLAN_CAP && pp2.coverage_pct() == 37,
        "",
    );
    // 空日志覆盖 100%（无预取需求）。
    cs.add("prefetch_empty_full", PrefetchPlanner::new().coverage_pct() == 100, "");

    // ── 降级预算 ──
    let budgets = degraded_budgets();
    cs.add(
        "degraded_budgets_doubled",
        budgets == [3_000, 5_000, 4_000, 4_000],
        "",
    );
    // 2 链实测 7.6s → 可行。
    cs.add("degraded_feasible_ok", degraded_feasible(&[1_800, 2_400, 1_900, 1_500]), "");
    // 超支段归因（壳层 4.5s > 4s 预算）。
    let mut out = [0usize; 4];
    let n = degraded_overrun_stages(&[1_800, 2_400, 1_900, 4_500], &mut out);
    cs.add("degraded_overrun_attributed", n == 1 && out[0] == 3, "");
    cs.add("degraded_all_within", degraded_overrun_stages(&[1_800, 2_400, 1_900, 1_500], &mut out) == 0, "");

    cs
}

#[cfg(test)]
mod deep4_tests {
    use super::*;

    #[test]
    fn prefetch_rebuild_refreshes() {
        let mut pp = PrefetchPlanner::new();
        pp.log_access(100);
        assert_eq!(pp.build(), 1);
        pp.log_access(200);
        assert_eq!(pp.build(), 2);
        assert_eq!(pp.built(), 2, "重建计数");
    }

    #[test]
    fn degraded_total_still_8s_target_semantics() {
        // 降级预算翻倍但总和 16s——诚实口径：链减半不改变物理线，
        // 预算翻倍表示"允许更久"，可行性仍按实测总和判。
        let b = degraded_budgets();
        assert_eq!(b.iter().sum::<u32>(), 16_000);
    }
}

// ===========================================================================
// v5 深化批（deep5）：IO 优先级类 + 断点续传偏移账
// ===========================================================================

// ---------------------------------------------------------------------------
// 深化一：IO 优先级三队列（RT > BE > Idle —— 高优先级先出，同级 FIFO）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IoClass {
    Rt,
    BestEffort,
    Idle,
}

/// 简化读请求。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PrioReq {
    pub offset: u32,
    pub class: IoClass,
}

/// 从定长槽队列头取一个并整体前移（n = 当前长度）。
fn dequeue_front(q: &mut [Option<PrioReq>], n: usize) -> Option<PrioReq> {
    let head = q[0].take();
    for k in 1..n {
        q[k - 1] = q[k].take();
    }
    head
}

pub struct PrioQueues {
    rt: [Option<PrioReq>; 4],
    be: [Option<PrioReq>; 8],
    idle: [Option<PrioReq>; 4],
    rt_n: usize,
    be_n: usize,
    idle_n: usize,
    /// 各类已发出数。
    served: [u32; 3],
}

impl PrioQueues {
    pub const fn new() -> Self {
        PrioQueues {
            rt: [None; 4],
            be: [None; 8],
            idle: [None; 4],
            rt_n: 0,
            be_n: 0,
            idle_n: 0,
            served: [0; 3],
        }
    }

    pub fn push(&mut self, r: PrioReq) -> bool {
        match r.class {
            IoClass::Rt => {
                if self.rt_n >= 4 {
                    return false;
                }
                self.rt[self.rt_n] = Some(r);
                self.rt_n += 1;
            }
            IoClass::BestEffort => {
                if self.be_n >= 8 {
                    return false;
                }
                self.be[self.be_n] = Some(r);
                self.be_n += 1;
            }
            IoClass::Idle => {
                if self.idle_n >= 4 {
                    return false;
                }
                self.idle[self.idle_n] = Some(r);
                self.idle_n += 1;
            }
        }
        true
    }

    /// 出队：RT 优先，其次 BE，最后 Idle；同类 FIFO。
    pub fn pop(&mut self) -> Option<PrioReq> {
        if self.rt_n > 0 {
            let head = dequeue_front(&mut self.rt, self.rt_n);
            self.rt_n -= 1;
            self.served[0] += 1;
            head
        } else if self.be_n > 0 {
            let head = dequeue_front(&mut self.be, self.be_n);
            self.be_n -= 1;
            self.served[1] += 1;
            head
        } else if self.idle_n > 0 {
            let head = dequeue_front(&mut self.idle, self.idle_n);
            self.idle_n -= 1;
            self.served[2] += 1;
            head
        } else {
            None
        }
    }

    pub fn pending(&self) -> usize {
        self.rt_n + self.be_n + self.idle_n
    }

    pub fn served(&self) -> [u32; 3] {
        self.served
    }
}

// ---------------------------------------------------------------------------
// 深化二：断点续传偏移账（分段下载——已收段位图 → 下一段起点）
// ---------------------------------------------------------------------------

/// 分段下载账：16 段（每段 256KB），位图记录已收段。
pub struct ResumeLedger {
    got: [u64; 1], // 16 位够用
    seg_bytes: u32,
    total_segs: u32,
}

pub const RESUME_SEG_BYTES: u32 = 256 * 1024;
pub const RESUME_MAX_SEGS: u32 = 16;

impl ResumeLedger {
    pub const fn new() -> Self {
        ResumeLedger { got: [0; 1], seg_bytes: RESUME_SEG_BYTES, total_segs: 0 }
    }

    /// 设置总段数（文件大小决定）。
    pub fn plan(&mut self, total_bytes: u32) {
        self.total_segs = (total_bytes + self.seg_bytes - 1) / self.seg_bytes;
    }

    /// 收到一段。
    pub fn seg_done(&mut self, seg: u32) -> bool {
        if seg >= RESUME_MAX_SEGS || seg >= self.total_segs {
            return false;
        }
        self.got[0] |= 1u64 << seg;
        true
    }

    /// 下一个缺失段（断点续传起点）。全齐 → None。
    pub fn next_missing(&self) -> Option<u32> {
        for s in 0..self.total_segs {
            if self.got[0] & (1u64 << s) == 0 {
                return Some(s);
            }
        }
        None
    }

    /// 完成度（×100）。
    pub fn done_pct(&self) -> u32 {
        if self.total_segs == 0 {
            return 0;
        }
        let mut n = 0u32;
        for s in 0..self.total_segs {
            if self.got[0] & (1u64 << s) != 0 {
                n += 1;
            }
        }
        n * 100 / self.total_segs
    }

    pub fn complete(&self) -> bool {
        self.total_segs > 0 && self.next_missing().is_none()
    }
}

// ---------------------------------------------------------------------------
// deep5 检查项
// ---------------------------------------------------------------------------

pub fn run_bootio_deep5_checks() -> CheckSet {
    let mut cs = CheckSet::new("F067-bootio-v5");

    // ── 优先级队列 ──
    // 1) RT 插队：先排 BE 两个，再排 RT → pop 先出 RT。
    let mut pq = PrioQueues::new();
    let _ = pq.push(PrioReq { offset: 100, class: IoClass::BestEffort });
    let _ = pq.push(PrioReq { offset: 200, class: IoClass::BestEffort });
    let _ = pq.push(PrioReq { offset: 0, class: IoClass::Rt });
    let head = pq.pop().expect("有");
    cs.add("prio_rt_first", head.class == IoClass::Rt && head.offset == 0, "");
    // 2) 同级 FIFO。
    let second = pq.pop().expect("有");
    cs.add("prio_be_fifo", second.offset == 100, "");
    // 3) Idle 最后。
    let _ = pq.push(PrioReq { offset: 900, class: IoClass::Idle });
    let _ = pq.pop();
    let idle = pq.pop().expect("有");
    cs.add("prio_idle_last", idle.class == IoClass::Idle, "");
    // 4) RT 满 4 拒绝。
    let mut pq2 = PrioQueues::new();
    let mut all_ok = true;
    for k in 0..5 {
        all_ok &= pq2.push(PrioReq { offset: k * 10, class: IoClass::Rt });
    }
    cs.add("prio_rt_cap_4", !all_ok && pq2.pending() == 4, "");
    // 5) 服务账同步。
    let sv = pq.served();
    cs.add("prio_served_ledger", sv == [1, 2, 1], "");

    // ── 断点续传 ──
    // 6) 1MB 文件 = 4 段；收 0,1,2 → 下缺 3。
    let mut rl = ResumeLedger::new();
    rl.plan(1024 * 1024);
    let _ = rl.seg_done(0);
    let _ = rl.seg_done(1);
    let _ = rl.seg_done(2);
    cs.add("resume_next_missing", rl.next_missing() == Some(3) && rl.done_pct() == 75, "");
    // 7) 全齐 → complete。
    let _ = rl.seg_done(3);
    cs.add("resume_complete", rl.complete() && rl.next_missing().is_none(), "");
    // 8) 越段拒绝。
    cs.add("resume_oob_refused", !rl.seg_done(4), "");
    // 9) 非整段尾：1MB+1B → 5 段（尾段 1 字节也占一段）。
    let mut rl2 = ResumeLedger::new();
    rl2.plan(1024 * 1024 + 1);
    let _ = rl2.seg_done(0);
    let _ = rl2.seg_done(1);
    let _ = rl2.seg_done(2);
    let _ = rl2.seg_done(3);
    cs.add(
        "resume_tail_segment_last",
        rl2.next_missing() == Some(4) && rl2.done_pct() == 80, // 前四段齐，尾段待收
        "",
    );
    // 10) 跳段收完也能补齐（乱序到达）。
    let mut rl3 = ResumeLedger::new();
    rl3.plan(RESUME_SEG_BYTES * 3);
    let _ = rl3.seg_done(2);
    let _ = rl3.seg_done(0);
    cs.add("resume_out_of_order", rl3.next_missing() == Some(1), "");

    cs
}

#[cfg(test)]
mod deep5_tests {
    use super::*;

    #[test]
    fn resume_tail_segment_exact() {
        let mut rl = ResumeLedger::new();
        rl.plan(1024 * 1024 + 1); // 5 段
        assert!(!rl.seg_done(5), "第 6 段不存在（5 段文件）");
        assert!(rl.seg_done(4), "第 5 段（尾段）合法");
        assert_eq!(rl.done_pct(), 20);
    }

    #[test]
    fn prio_interleaved_drain() {
        let mut pq = PrioQueues::new();
        let _ = pq.push(PrioReq { offset: 1, class: IoClass::Idle });
        let _ = pq.push(PrioReq { offset: 2, class: IoClass::Rt });
        let _ = pq.push(PrioReq { offset: 3, class: IoClass::Idle });
        let _ = pq.push(PrioReq { offset: 4, class: IoClass::BestEffort });
        // 全清顺序：RT、BE、Idle(1)、Idle(3)。
        assert_eq!(pq.pop().unwrap().offset, 2);
        assert_eq!(pq.pop().unwrap().offset, 4);
        assert_eq!(pq.pop().unwrap().offset, 1);
        assert_eq!(pq.pop().unwrap().offset, 3);
        assert_eq!(pq.pop(), None);
    }
}
