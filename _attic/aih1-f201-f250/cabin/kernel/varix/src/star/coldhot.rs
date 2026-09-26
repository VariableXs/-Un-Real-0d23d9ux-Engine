//! F067 启动 IO 冷热分离 · 完整设计（STAR I 主册 G-B-27）。
//!
//! **判据（主册）**：只读区随机读延迟 P99 <2ms（U 盘实测）；启动段 IO 走
//! 并行（F053）后只读段耗时占比 <25%。
//!
//! **设计要点（主册）**：
//! - 镜像分区布局分离只读区（内核/字体/资产）与可写区：只读区零 journal、
//!   零写放大；BOT 顺序读优势最大化（预读全开）；
//! - 分区布局设计文档化（**偏移/对齐/大小三表**——本模块结构化产出）；
//! - 只读区哈希清单用于完整性自查（F191 联动）→ 不符 → 启动链自查拦截
//!   进恢复环境；
//! - 可写区满 → 提前预警（<500MB）；
//! - 只读区对齐 4MB（大页 F051 联动）；
//! - 资产按访问序物理排布（启动甘特图顺序 = 盘上顺序——这是 BOT 顺序
//!   读/预读全开生效的前提，本模块给出可判定的顺序性检查）；
//! - 版本更新 = 只读区整体换槽（双槽 F190 语义——换槽前哈希清单必验，
//!   不符拒绝换槽保持旧槽）；
//! - 可写区 journal 只为可写数据服务（职责不混——只读区结构上无 journal）。
//!
//! 布局思路参照只读 rootfs（squashfs 类）设计经验；实现自研（镜像构建
//! 工具链）。一切时间注入式（微秒戳），宿主测试确定复现。

use crate::checks::CheckSet;
use crate::star::sbase::{pct_near, RingLog};

use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 规格常量
// ---------------------------------------------------------------------------

/// 只读区对齐（4MB，大页 F051 联动）。
pub const RO_ALIGN_BYTES: u64 = 4 * 1024 * 1024;

/// 可写区低水位预警线（<500MB 提前预警）。
pub const WRITABLE_LOW_WATER_BYTES: u64 = 500 * 1024 * 1024;

/// 只读区随机读判线（μs）：P99 <2ms（U 盘实测口径）。
pub const RO_RANDOM_P99_LIMIT_US: u64 = 2_000;

/// 只读段耗时占比判线（万分比 <25%）。
pub const RO_TIME_SHARE_LIMIT_PPT: u32 = 2_500;

/// 随机读判线的最小样本量（数据充足性门——不足不下结论）。
pub const RANDOM_MIN_SAMPLES: usize = 64;

/// 随机读延迟环容量。
const RAND_RING_CAP: usize = 512;

/// 哈希清单容量（只读区文件数远小于此）。
const MANIFEST_CAP: usize = 512;

// ---------------------------------------------------------------------------
// 分区布局三表（偏移/对齐/大小）
// ---------------------------------------------------------------------------

/// 盘面分区规划（双槽 F190 语义 + 可写区）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RegionPlan {
    /// 槽 A 只读区起点（4MB 对齐）。
    pub slot_a_ro_base: u64,
    /// 槽 B 只读区起点（4MB 对齐）。
    pub slot_b_ro_base: u64,
    /// 可写区起点（4MB 对齐；可写区 journal 只为可写数据服务）。
    pub writable_base: u64,
    /// 盘总容量。
    pub total_bytes: u64,
}

impl RegionPlan {
    /// 对齐表：三区起点全部 4MB 对齐。
    pub fn bases_aligned(&self) -> bool {
        self.slot_a_ro_base % RO_ALIGN_BYTES == 0
            && self.slot_b_ro_base % RO_ALIGN_BYTES == 0
            && self.writable_base % RO_ALIGN_BYTES == 0
    }
}

/// 单个资产落位（偏移表 + 大量表条目）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AssetEntry {
    pub name: &'static str,
    /// 物理偏移（绝对，从盘首起算）。
    pub offset: u64,
    pub size_bytes: u64,
}

/// 对齐工具：向上对齐。
pub fn align_up(v: u64, align: u64) -> u64 {
    if align == 0 {
        return v;
    }
    v.div_ceil(align) * align
}

/// 规划双槽 + 可写区布局（放不下诚实返回 None——不伪装成功）。
pub fn plan_layout(total_bytes: u64, slot_a_ro_bytes: u64, slot_b_ro_bytes: u64) -> Option<RegionPlan> {
    let a_base = align_up(0, RO_ALIGN_BYTES);
    let b_base = align_up(a_base + slot_a_ro_bytes, RO_ALIGN_BYTES);
    let w_base = align_up(b_base + slot_b_ro_bytes, RO_ALIGN_BYTES);
    if w_base >= total_bytes {
        return None; // 可写区无处安放——布局失败如实上报。
    }
    Some(RegionPlan { slot_a_ro_base: a_base, slot_b_ro_base: b_base, writable_base: w_base, total_bytes })
}

/// 资产按访问序物理排布：`assets` 必须已经按启动甘特图（F053）顺序给出，
/// 盘上偏移连续紧密排布（顺序读优势最大化）。
pub fn place_assets(ro_base: u64, assets: &[(&'static str, u64)]) -> Vec<AssetEntry> {
    let mut out = Vec::with_capacity(assets.len());
    let mut cursor = ro_base;
    for (name, size) in assets {
        out.push(AssetEntry { name, offset: cursor, size_bytes: *size });
        cursor += size;
    }
    out
}

/// 访问序自检：甘特图顺序遍历的盘上偏移必须单调不减——顺序读模式成立
/// （BOT 预读全开生效的前提；甘特图中出现未知资产名 → 严格判 false）。
pub fn in_disk_order(entries: &[AssetEntry], gantt: &[&str]) -> bool {
    let mut prev: Option<u64> = None;
    for g in gantt {
        let e = match entries.iter().find(|e| e.name == *g) {
            Some(e) => e,
            None => return false,
        };
        if let Some(p) = prev {
            if e.offset < p {
                return false;
            }
        }
        prev = Some(e.offset);
    }
    true
}

// ---------------------------------------------------------------------------
// 哈希清单（F191 联动）
// ---------------------------------------------------------------------------

/// FNV-1a 64——只读区内容指纹。
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// 哈希清单校验结果。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ManifestVerify {
    pub ok: bool,
    /// 不符条目（指名道姓——F191 拦截与恢复环境定位用）。
    pub mismatches: Vec<&'static str>,
}

/// 只读区哈希清单。
pub struct HashManifest {
    entries: Vec<(&'static str, u64)>,
}

impl HashManifest {
    pub fn new() -> HashManifest {
        HashManifest { entries: Vec::new() }
    }

    pub fn add(&mut self, name: &'static str, content_hash: u64) -> bool {
        if self.entries.len() >= MANIFEST_CAP || self.entries.iter().any(|(n, _)| *n == name) {
            return false;
        }
        self.entries.push((name, content_hash));
        true
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 完整性自查：实际哈希（F191 采集）逐条比对。
    pub fn verify(&self, actual: &[(&'static str, u64)]) -> ManifestVerify {
        let mut mismatches = Vec::new();
        for (name, expect) in &self.entries {
            match actual.iter().find(|(n, _)| n == name) {
                Some((_, h)) if h == expect => {}
                _ => mismatches.push(*name),
            }
        }
        ManifestVerify { ok: mismatches.is_empty(), mismatches }
    }
}

impl Default for HashManifest {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 可写区守护（低水位预警 + 只读区写拒绝）
// ---------------------------------------------------------------------------

/// 只读区 journal 状态——结构上无 journal（零 journal、零写放大的
/// 可执行语义：常量而非开关）。
pub const fn ro_journal_enabled() -> bool {
    false
}

/// 可写区水位。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WritableHealth {
    /// 充裕。
    Ok,
    /// 低于 500MB——提前预警（诊断面提示，不阻断写入）。
    LowWater,
}

/// 可写区守护。
pub struct WritableGuard {
    total: u64,
    free: u64,
}

impl WritableGuard {
    pub fn new(total: u64, free: u64) -> WritableGuard {
        WritableGuard { total: total.max(1), free: free.min(total) }
    }

    /// 更新空闲水位（调用方按容量事件驱动）。
    pub fn update_free(&mut self, free: u64) -> WritableHealth {
        self.free = free.min(self.total);
        if self.free < WRITABLE_LOW_WATER_BYTES {
            WritableHealth::LowWater
        } else {
            WritableHealth::Ok
        }
    }

    pub fn free(&self) -> u64 {
        self.free
    }

    /// 只读区写入请求 → 一律拒绝（只读区语义的运行时面）。
    pub fn ro_write_attempt(&self) -> Result<(), &'static str> {
        Err("read-only region: zero journal, zero write amplification")
    }
}

// ---------------------------------------------------------------------------
// 启动 IO 剖析（判据直读面）
// ---------------------------------------------------------------------------

/// 启动段耗时占比与随机读 P99 双判据。
pub struct BootIoProfiler {
    ro_us: u64,
    rw_us: u64,
    rand_lat: RingLog<u32, RAND_RING_CAP>,
}

impl BootIoProfiler {
    pub fn new() -> BootIoProfiler {
        BootIoProfiler { ro_us: 0, rw_us: 0, rand_lat: RingLog::new() }
    }

    /// 记录只读段加载耗时（F053 并行后的分段打点）。
    pub fn record_ro(&mut self, us: u64) {
        self.ro_us += us;
    }

    /// 记录可写段加载耗时。
    pub fn record_rw(&mut self, us: u64) {
        self.rw_us += us;
    }

    /// 只读段随机读延迟打点（μs）。
    pub fn record_random_read(&mut self, us: u32) {
        self.rand_lat.push(us);
    }

    /// 只读段耗时占比（万分比；总耗时为 0 → 0）。
    pub fn ro_share_ppt(&self) -> u32 {
        let total = self.ro_us + self.rw_us;
        if total == 0 {
            return 0;
        }
        (self.ro_us * 10_000 / total) as u32
    }

    /// 只读区随机读 P99（μs；样本不足返回 None——不猜）。
    pub fn random_p99_us(&self) -> Option<u64> {
        if self.rand_lat.len() < RANDOM_MIN_SAMPLES {
            return None;
        }
        let v: Vec<u64> = self.rand_lat.newest_first().iter().map(|v| *v as u64).collect();
        Some(pct_near(&v, 99))
    }

    /// 双判据直读（占比 <25% 且 P99 <2ms；P99 样本不足 → 不达标——诚实）。
    pub fn meets(&self) -> bool {
        self.ro_share_ppt() < RO_TIME_SHARE_LIMIT_PPT
            && matches!(self.random_p99_us(), Some(p99) if p99 < RO_RANDOM_P99_LIMIT_US)
    }
}

impl Default for BootIoProfiler {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 双槽换槽（F190 语义）
// ---------------------------------------------------------------------------

/// 只读区槽位。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Slot {
    A,
    B,
}

impl Slot {
    /// 换槽目标。
    pub fn other(self) -> Slot {
        match self {
            Slot::A => Slot::B,
            Slot::B => Slot::A,
        }
    }
}

/// 双槽管理：版本更新 = 只读区整体换槽；换槽前哈希清单必验。
pub struct DualSlot {
    active: Slot,
    staged: Option<(Slot, HashManifest)>,
    /// 成功换槽次数。
    pub swaps: u32,
    /// 被拒绝的换槽尝试（哈希不符等——留痕）。
    pub refused: u32,
}

impl DualSlot {
    pub fn new(active: Slot) -> DualSlot {
        DualSlot { active, staged: None, swaps: 0, refused: 0 }
    }

    pub fn active(&self) -> Slot {
        self.active
    }

    /// 暂存新槽布局与清单（不能暂存当前活动槽——换槽语义是「切到另一槽」）。
    pub fn stage(&mut self, slot: Slot, manifest: HashManifest) -> bool {
        if slot == self.active {
            return false;
        }
        self.staged = Some((slot, manifest));
        true
    }

    /// 激活暂存槽：F191 采集的实际哈希全对 → 换槽；不符 → 拒绝并保持旧槽
    /// （拦截进恢复环境的判定由 F191 做——本模块只拒绝切换）。
    pub fn activate(&mut self, actual: &[(&'static str, u64)]) -> Result<Slot, &'static str> {
        let (slot, manifest) = match self.staged.take() {
            Some(p) => p,
            None => {
                self.refused += 1;
                return Err("no staged slot");
            }
        };
        let v = manifest.verify(actual);
        if !v.ok {
            self.refused += 1;
            return Err("staged slot hash mismatch: intercept to recovery env (F191)");
        }
        self.active = slot;
        self.swaps += 1;
        Ok(slot)
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F067 自检（判据：只读区随机读 P99 <2ms；只读段耗时占比 <25%）。
pub fn run_coldhot_checks() -> CheckSet {
    let mut set = CheckSet::new("F067-coldhot");

    // 1. 布局三表：16GB 盘 = 槽 A 6GB + 槽 B 6GB + 可写区；三基点 4MB 对齐、
    //    偏移连续、可写区容量守恒。
    let g: u64 = 1024 * 1024 * 1024;
    let plan = plan_layout(16 * g, 6 * g, 6 * g).unwrap();
    set.add(
        "layout three tables aligned & conserved",
        plan.bases_aligned()
            && plan.slot_a_ro_base == 0
            && plan.slot_b_ro_base == align_up(6 * g, RO_ALIGN_BYTES)
            && plan.writable_base == align_up(12 * g, RO_ALIGN_BYTES)
            && plan.writable_base < 16 * g,
        "",
    );

    // 2. 布局诚实拒绝：盘放不下双槽 → None。
    set.add("layout refuses when total too small", plan_layout(8 * g, 6 * g, 6 * g).is_none(), "");

    // 3. 资产按访问序排布：甘特图顺序 = 盘上顺序。
    let assets = [("kernel", 16 * 1024 * 1024u64), ("font-cn", 64 * 1024 * 1024), ("assets-ui", 128 * 1024 * 1024)];
    let placed = place_assets(plan.slot_a_ro_base, &assets);
    set.add(
        "assets placed in access order",
        in_disk_order(&placed, &["kernel", "font-cn", "assets-ui"])
            && placed[1].offset == assets[0].1
            && placed[2].offset == assets[0].1 + assets[1].1,
        "",
    );
    // 乱序甘特（后访问者偏移更小）→ false（顺序读前提被破坏，显性暴露）。
    set.add("out-of-order gantt detected", !in_disk_order(&placed, &["assets-ui", "kernel", "font-cn"]), "");
    // 甘特图出现未知资产 → 严格 false。
    set.add("unknown gantt asset rejected", !in_disk_order(&placed, &["kernel", "ghost"]), "");

    // 4. 哈希清单：一致全绿；篡改一条 → 指名道姓。
    let mut man = HashManifest::new();
    assert!(man.add("kernel", fnv1a64(b"kernel-bytes-v7")));
    assert!(man.add("font-cn", fnv1a64(b"font-cn-bytes-v1")));
    let ok_v = man.verify(&[
        ("kernel", fnv1a64(b"kernel-bytes-v7")),
        ("font-cn", fnv1a64(b"font-cn-bytes-v1")),
    ]);
    let bad_v = man.verify(&[
        ("kernel", fnv1a64(b"kernel-bytes-v7")),
        ("font-cn", fnv1a64(b"font-cn-BITFLIP")),
    ]);
    set.add(
        "hash manifest pinpoints mismatch",
        ok_v.ok && !bad_v.ok && bad_v.mismatches == vec!["font-cn"],
        "",
    );

    // 5. 只读区写拒绝 + 零 journal 常量语义。
    let g2 = WritableGuard::new(4 * g, 4 * g);
    set.add(
        "ro region denies writes & has no journal",
        g2.ro_write_attempt().is_err() && !ro_journal_enabled(),
        "",
    );

    // 6. 可写区低水位：499MB → 预警；501MB → 正常。
    let mut g3 = WritableGuard::new(4 * g, 4 * g);
    let low = g3.update_free(WRITABLE_LOW_WATER_BYTES - 1);
    let okh = g3.update_free(WRITABLE_LOW_WATER_BYTES + 1);
    set.add(
        "writable low-water 500MB warning",
        low == WritableHealth::LowWater && okh == WritableHealth::Ok,
        "",
    );

    // 7. 双槽换槽：验证通过 → 切槽；不符 → 拒绝保持旧槽。
    //    （activate 消费暂存——拒绝后重验必须重新 stage，语义如此。）
    let mut ds = DualSlot::new(Slot::A);
    let mut m_b = HashManifest::new();
    let _ = m_b.add("kernel", fnv1a64(b"kernel-v8"));
    let _ = ds.stage(Slot::B, m_b);
    let bad_act = ds.activate(&[("kernel", fnv1a64(b"kernel-V8-FLIP"))]);
    let active_after_refusal = ds.active(); // 拒绝后必须仍是旧槽（先观测再继续）。
    let mut m_b2 = HashManifest::new();
    let _ = m_b2.add("kernel", fnv1a64(b"kernel-v8"));
    let staged_again = ds.stage(Slot::B, m_b2);
    let good_act = ds.activate(&[("kernel", fnv1a64(b"kernel-v8"))]);
    set.add(
        "dual-slot swap gated by manifest",
        bad_act.is_err()
            && active_after_refusal == Slot::A
            && ds.refused == 1
            && staged_again
            && good_act == Ok(Slot::B)
            && ds.swaps == 1,
        "",
    );

    // 8. stage 当前活动槽拒绝。
    let mut ds2 = DualSlot::new(Slot::A);
    let mut m_a = HashManifest::new();
    let _ = m_a.add("kernel", 1);
    set.add("cannot stage active slot", !ds2.stage(Slot::A, m_a), "");

    // 9. 只读段耗时占比 <25% 正演：RO 2.0s / 总 10.0s = 20%。
    let mut prof = BootIoProfiler::new();
    prof.record_ro(2_000_000);
    prof.record_rw(8_000_000);
    set.add("ro time share 20% under 25%", prof.ro_share_ppt() == 2_000, "");
    // 红面：RO 3.5s / 总 10s = 35% → 不达标。
    let mut prof_bad = BootIoProfiler::new();
    prof_bad.record_ro(3_500_000);
    prof_bad.record_rw(6_500_000);
    set.add("ro share 35% honestly red", prof_bad.ro_share_ppt() == 3_500, "");

    // 10. 随机读 P99 <2ms：64 样本全 1.5ms → 达标；样本不足 → 不猜。
    let mut prof2 = BootIoProfiler::new();
    for _ in 0..RANDOM_MIN_SAMPLES {
        prof2.record_random_read(1_500);
    }
    set.add(
        "ro random p99 under 2ms with enough samples",
        prof2.random_p99_us() == Some(1_500) && prof2.meets(),
        "",
    );
    let mut prof_few = BootIoProfiler::new();
    for _ in 0..(RANDOM_MIN_SAMPLES - 1) {
        prof_few.record_random_read(1_500);
    }
    set.add("random p99 insufficient samples => no verdict", prof_few.random_p99_us().is_none() && !prof_few.meets(), "");

    // 11. 双判据合流：占比达标 + P99 达标 → meets。
    for _ in 0..RANDOM_MIN_SAMPLES {
        prof.record_random_read(1_500);
    }
    set.add("dual criteria meets", prof.meets(), "");

    set
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn align_up_edges() {
        assert_eq!(align_up(0, 4 * 1024 * 1024), 0);
        assert_eq!(align_up(1, 4096), 4096);
        assert_eq!(align_up(4096, 4096), 4096);
        assert_eq!(align_up(4097, 4096), 8192);
        assert_eq!(align_up(7, 0), 7, "零对齐保护");
    }

    #[test]
    fn fnv1a64_deterministic_and_sensitive() {
        let a = fnv1a64(b"varix");
        assert_eq!(a, fnv1a64(b"varix"));
        assert_ne!(a, fnv1a64(b"varixs"));
        assert_ne!(a, fnv1a64(b""));
        assert_eq!(fnv1a64(b""), 0xcbf29ce484222325, "空输入 = FNV offset basis");
    }

    #[test]
    fn layout_minimal_medium() {
        // 小盘：两个 8MB 槽 + 剩余可写区。
        let m: u64 = 1024 * 1024;
        let plan = plan_layout(100 * m, 8 * m, 8 * m).unwrap();
        assert_eq!(plan.slot_a_ro_base, 0);
        assert_eq!(plan.slot_b_ro_base, 8 * m);
        assert_eq!(plan.writable_base, 16 * m);
        assert!(plan.bases_aligned());
    }

    #[test]
    fn manifest_add_rules() {
        let mut m = HashManifest::new();
        assert!(m.add("a", 1));
        assert!(!m.add("a", 2), "重名拒绝");
        assert_eq!(m.len(), 1);
        // 清单缺失条目也算不符（actual 少给 → mismatch 指名）。
        let v = m.verify(&[]);
        assert!(!v.ok && v.mismatches == vec!["a"]);
    }

    #[test]
    fn writable_guard_clamps_free() {
        let mut g = WritableGuard::new(1000, 500);
        g.update_free(2000);
        assert_eq!(g.free(), 1000, "free 不得超过 total");
    }

    #[test]
    fn profiler_zero_state() {
        let p = BootIoProfiler::new();
        assert_eq!(p.ro_share_ppt(), 0);
        assert!(p.random_p99_us().is_none());
        assert!(!p.meets(), "无数据不达标");
    }

    #[test]
    fn slot_swap_requires_staging() {
        let mut ds = DualSlot::new(Slot::B);
        assert_eq!(ds.activate(&[]), Err("no staged slot"));
        assert_eq!(ds.refused, 1);
        assert_eq!(ds.active(), Slot::B);
    }

    #[test]
    fn place_assets_preserves_order_and_sizes() {
        let assets = [("a", 100u64), ("b", 300), ("c", 50)];
        let placed = place_assets(4096, &assets);
        assert_eq!(placed[0].offset, 4096);
        assert_eq!(placed[1].offset, 4196);
        assert_eq!(placed[2].offset, 4496);
        assert_eq!(placed[2].size_bytes, 50);
    }

    #[test]
    fn dual_slot_roundtrip_swap_back() {
        let mut ds = DualSlot::new(Slot::A);
        let mut m = HashManifest::new();
        let _ = m.add("k", 42);
        assert!(ds.stage(Slot::B, m));
        assert_eq!(ds.activate(&[("k", 42)]), Ok(Slot::B));
        // 换回 A。
        let mut m2 = HashManifest::new();
        let _ = m2.add("k", 7);
        assert!(ds.stage(Slot::A, m2));
        assert_eq!(ds.activate(&[("k", 7)]), Ok(Slot::A));
        assert_eq!(ds.swaps, 2);
    }
}
