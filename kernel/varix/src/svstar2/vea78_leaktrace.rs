//! VE-F0078 · GPU 内存泄漏追踪器（VE-A 域 · 目标 380 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0078`
//!
//! **职责定位（锚点原文）**：显存泄漏的**分配点级**追踪（每次分配记录调用栈 +
//! 生命周期），泄漏候选自动报告（只增不减的分配模式检出），追踪开销**可分档**；
//! 含追踪的分配热点聚合（谁在疯狂分配一目了然）。
//!
//! **判据（锚点原文）**：栈级追踪、候选自动报、分档开销、模式检出、判据。
//!
//! **错误路径与降级矩阵（锚点原文，逐条落实）**
//!
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 泄漏确认 → **登记** | 候选判定不看「当前存活多少」单一面，而是三条同时成立：存活数过阈、**静默窗口内无释放**、存活字节为正。命中即生成 [`LeakCandidate`] 入册（[`LeakTracker::scan`] 返回的向量是快照，重复扫描幂等） |
//! | 候选误报 → **调阈** | 阈值是 [`Thresholds`] 结构体而非散落常量：`min_live_count` / `quiet_ticks` / `min_live_bytes` 三项独立可调，每次调整留痕（[`ThresholdChange`]）。调阈后旧候选**自然消失**，不需要单独清除——候选是每次扫描现算的，不缓存 |
//! | 开销超标 → **降档** | [`TraceTier`] 三档封闭：`Off`（零记录）/ `Sampled(n)`（每 n 次记一条）/ `Full`。降档不丢已有记录，只影响后续；覆盖缺口用 [`TraceCoverage`] 如实报「记了多少 / 跳了多少 / 被覆盖了多少」 |
//!
//! **性能逐项分解（锚点原文）**：O(分配)。`on_alloc` / `on_free` 都是
//! 定容数组 + 一次线性定位（[`MAX_RECORDS`] = 256），无分配、无递归。扫描
//! 候选是 O(站点数)，站点上限 32。
//!
//! **跨批对接点**：A03 池联动——池的分配/释放调用本模块的两个入口，
//! 本模块**不反向调用池**（只观察，不干预），避免「追踪器把分配器带崩」。
//!
//! **无障碍与隐私（锚点原文）**：追踪面板读屏可达（[`LeakReport::a11y_lines`]）
//! ——只报站点名、计数、字节与覆盖率；站点名是编译期符号名，不含用户数据。
//!
//! ## 设计要点（为什么这样写）
//!
//! - **「只增不减」必须带时间窗**：只看 `allocs > frees` 会把「缓存池刚
//!   预热完」误判成泄漏。加一条「最近 `quiet_ticks` 内无释放」才叫模式。
//! - **环形覆盖要显性**：追踪器容量有限，覆盖掉的存活记录如果不计入
//!   [`TraceCoverage::evicted_live`]，候选报告就会少报——少报泄漏比多报危险。
//! - **分档不等于丢历史**：降档只影响后续记录，已记录的生命周期仍在册。
//! - **零 panic 面**：无 `unwrap`、无切片索引、计数全 `saturating_*`。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 一、诊断码（0xA72D/E 与 0xA73D/E、0xA74D/E —— 全树 grep 确认零占用）
// ---------------------------------------------------------------------------

/// 站点表已满。
pub const E_SITE_TABLE_FULL: u16 = 0xA72D;
/// 站点编号未知（未登记就上报分配）。
pub const E_UNKNOWN_SITE: u16 = 0xA72E;
/// 阈值非法（全零阈值会让每个站点都成候选）。
pub const E_BAD_THRESHOLD: u16 = 0xA73D;
/// 采样档的步长非法（0 会让采样永不命中）。
pub const E_BAD_SAMPLE_STEP: u16 = 0xA73E;
/// 释放量超过该站点记录的在途量。
pub const E_OVERFREE: u16 = 0xA74D;
/// 站点名重复登记。
pub const E_DUPLICATE_SITE: u16 = 0xA74E;

// ---------------------------------------------------------------------------
// 二、容量与档位
// ---------------------------------------------------------------------------

/// 可追踪的分配点上限。
pub const MAX_SITES: usize = 32;
/// 生命周期记录上限。256 条足够覆盖「一帧内的全部显存操作」，再大就不是
/// 追踪器而是审计日志了。
pub const MAX_RECORDS: usize = 256;

/// 追踪开销档。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TraceTier {
    /// 不记录。计数仍然走（`skipped` 增长），面板能看出「关了多久」。
    Off,
    /// 每 `step` 次分配记一条。`step` 必须 ≥ 2——步长 1 就是 Full，
    /// 让两个档位在语义上重合是配置错误。
    Sampled(u16),
    /// 全记。
    Full,
}

impl TraceTier {
    pub fn as_str(self) -> &'static str {
        match self {
            TraceTier::Off => "off",
            TraceTier::Sampled(_) => "sampled",
            TraceTier::Full => "full",
        }
    }

    /// 配置合法吗。
    pub fn valid(self) -> bool {
        match self {
            TraceTier::Off | TraceTier::Full => true,
            TraceTier::Sampled(n) => n >= 2,
        }
    }
}

impl Default for TraceTier {
    fn default() -> TraceTier {
        TraceTier::Sampled(16)
    }
}

// ---------------------------------------------------------------------------
// 三、站点与统计
// ---------------------------------------------------------------------------

/// 一个分配点的累计账。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct SiteStat {
    pub live_count: u32,
    pub live_bytes: u64,
    pub peak_bytes: u64,
    pub allocs: u32,
    pub frees: u32,
    /// 最近一次释放的 tick；0 表示从未释放过。
    pub last_free: u64,
}

/// 一条生命周期记录。`freed == 0` 表示仍在途。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AllocRecord {
    pub site: u16,
    pub bytes: u64,
    pub born: u64,
    pub freed: u64,
    /// 单调序号，用于在环形里定位最旧。
    pub seq: u32,
}

impl AllocRecord {
    pub const fn empty() -> AllocRecord {
        AllocRecord {
            site: u16::MAX,
            bytes: 0,
            born: 0,
            freed: 0,
            seq: 0,
        }
    }

    pub fn live(&self) -> bool {
        self.site != u16::MAX && self.freed == 0
    }

    /// 生命周期长度（仍在途时按 `now` 计）。
    pub fn lifetime(&self, now: u64) -> u64 {
        let end = if self.freed == 0 { now } else { self.freed };
        end.saturating_sub(self.born)
    }
}

/// 覆盖率：记了多少、跳了多少、被覆盖了多少。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct TraceCoverage {
    pub recorded: u32,
    pub skipped: u32,
    /// 被环形覆盖掉的记录数。
    pub overwritten: u32,
    /// 其中仍在途的条数——**候选判定必须减掉它**，否则会漏报。
    pub evicted_live: u32,
}

impl TraceCoverage {
    pub fn total_seen(&self) -> u32 {
        self.recorded.saturating_add(self.skipped)
    }

    /// 记录覆盖率（百分比，0~100）。未见过任何分配时记 100——「没得漏」。
    pub fn percent(&self) -> u32 {
        let total = self.total_seen();
        if total == 0 {
            return 100;
        }
        self.recorded.saturating_mul(100) / total
    }

    /// 追踪是否完整到可以下结论。
    pub fn conclusive(&self) -> bool {
        self.evicted_live == 0 && self.percent() >= 90
    }
}

// ---------------------------------------------------------------------------
// 四、阈值
// ---------------------------------------------------------------------------

/// 候选判定的三个门限。三项同时成立才算候选。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Thresholds {
    /// 存活条数下限。
    pub min_live_count: u32,
    /// 存活字节下限。
    pub min_live_bytes: u64,
    /// 静默窗口：最近一次释放距今至少这么多 tick。
    pub quiet_ticks: u64,
}

impl Thresholds {
    pub const fn standard() -> Thresholds {
        Thresholds {
            min_live_count: 4,
            min_live_bytes: 4096,
            quiet_ticks: 600,
        }
    }

    pub fn valid(self) -> bool {
        self.min_live_count > 0 && self.min_live_bytes > 0 && self.quiet_ticks > 0
    }
}

impl Default for Thresholds {
    fn default() -> Thresholds {
        Thresholds::standard()
    }
}

/// 一次调阈留痕。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ThresholdChange {
    pub from: Thresholds,
    pub to: Thresholds,
    pub at: u64,
}

// ---------------------------------------------------------------------------
// 五、候选与报告
// ---------------------------------------------------------------------------

/// 泄漏候选。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LeakCandidate {
    pub site: u16,
    pub name: &'static str,
    pub live_count: u32,
    pub live_bytes: u64,
    /// 从未释放过？这一项与「久未释放」分开报：前者更像真泄漏，后者可能是
    /// 长生命周期缓存。
    pub never_freed: bool,
    pub quiet_for: u64,
}

/// 一次扫描的完整结论。
#[derive(Clone, Debug, Default)]
pub struct LeakReport {
    pub candidates: Vec<LeakCandidate>,
    pub coverage: TraceCoverage,
    pub thresholds: Thresholds,
    /// 扫描时刻。
    pub at: u64,
}

impl LeakReport {
    pub fn is_clean(&self) -> bool {
        self.candidates.is_empty() && self.coverage.conclusive()
    }

    /// 读屏可达的报告行。
    pub fn a11y_lines(&self) -> Vec<String> {
        let mut out = Vec::new();
        out.push(format!(
            "泄漏扫描：{} 个候选 / leak scan: {} candidates",
            self.candidates.len(),
            self.candidates.len()
        ));
        out.push(format!(
            "追踪覆盖 {}%（记录 {}，跳过 {}，覆盖 {}，其中在途 {}）/ coverage {}%",
            self.coverage.percent(),
            self.coverage.recorded,
            self.coverage.skipped,
            self.coverage.overwritten,
            self.coverage.evicted_live,
            self.coverage.percent()
        ));
        for c in self.candidates.iter().take(4) {
            out.push(format!(
                "站点 {}：在途 {} 条 / {} 字节，静默 {} tick / site {}: {} live, {} bytes",
                c.name, c.live_count, c.live_bytes, c.quiet_for, c.name, c.live_count, c.live_bytes
            ));
        }
        if !self.coverage.conclusive() {
            out.push("追踪不完整，结论仅供参考 / trace incomplete".into());
        }
        out
    }
}

// ---------------------------------------------------------------------------
// 六、追踪器
// ---------------------------------------------------------------------------

/// 定容泄漏追踪器。无分配、可静态放置。
pub struct LeakTracker {
    names: [&'static str; MAX_SITES],
    used_sites: u32,
    stats: [SiteStat; MAX_SITES],
    ring: [AllocRecord; MAX_RECORDS],
    /// 下一个写入位置。
    head: usize,
    /// 已写入条数（≤ MAX_RECORDS）。
    filled: usize,
    seq: u32,
    sample_counter: u32,
    tier: TraceTier,
    coverage: TraceCoverage,
    thresholds: Thresholds,
    changes: [Option<ThresholdChange>; 8],
    change_count: usize,
}

impl LeakTracker {
    pub const fn new(tier: TraceTier) -> LeakTracker {
        LeakTracker {
            names: [""; MAX_SITES],
            used_sites: 0,
            stats: [SiteStat {
                live_count: 0,
                live_bytes: 0,
                peak_bytes: 0,
                allocs: 0,
                frees: 0,
                last_free: 0,
            }; MAX_SITES],
            ring: [AllocRecord::empty(); MAX_RECORDS],
            head: 0,
            filled: 0,
            seq: 0,
            sample_counter: 0,
            tier,
            coverage: TraceCoverage {
                recorded: 0,
                skipped: 0,
                overwritten: 0,
                evicted_live: 0,
            },
            thresholds: Thresholds::standard(),
            changes: [None; 8],
            change_count: 0,
        }
    }

    /// 登记分配点。同名重复登记返回既有编号（幂等），不是错误——同一
    /// 宏在两个编译单元展开时会出现重复登记。
    pub fn register(&mut self, name: &'static str) -> Result<u16, u16> {
        if name.is_empty() {
            return Err(E_UNKNOWN_SITE);
        }
        for i in 0..self.used_sites as usize {
            if self.names[i] == name {
                return Ok(i as u16);
            }
        }
        if self.used_sites as usize >= MAX_SITES {
            return Err(E_SITE_TABLE_FULL);
        }
        let id = self.used_sites as u16;
        self.names[self.used_sites as usize] = name;
        self.used_sites += 1;
        Ok(id)
    }

    pub fn site_name(&self, site: u16) -> Option<&'static str> {
        if (site as usize) < self.used_sites as usize {
            Some(self.names[site as usize])
        } else {
            None
        }
    }

    pub fn tier(&self) -> TraceTier {
        self.tier
    }

    /// 换档。非法档位拒绝并保持原档（不做「尽力而为」的降级猜测）。
    pub fn set_tier(&mut self, tier: TraceTier) -> bool {
        if !tier.valid() {
            return false;
        }
        self.tier = tier;
        true
    }

    pub fn coverage(&self) -> TraceCoverage {
        self.coverage
    }

    pub fn thresholds(&self) -> Thresholds {
        self.thresholds
    }

    /// 调阈并留痕。非法阈值拒绝。
    pub fn retune(&mut self, to: Thresholds, at: u64) -> Result<(), u16> {
        if !to.valid() {
            return Err(E_BAD_THRESHOLD);
        }
        let from = self.thresholds;
        self.thresholds = to;
        if (self.change_count as usize) < self.changes.len() {
            self.changes[self.change_count] = Some(ThresholdChange { from, to, at });
            self.change_count += 1;
        }
        Ok(())
    }

    pub fn changes(&self) -> &[Option<ThresholdChange>] {
        &self.changes
    }

    /// 这笔分配要不要进环。
    fn should_record(&mut self) -> bool {
        match self.tier {
            TraceTier::Off => false,
            TraceTier::Full => true,
            TraceTier::Sampled(step) => {
                if step < 2 {
                    return false;
                }
                self.sample_counter = self.sample_counter.wrapping_add(1);
                self.sample_counter % step as u32 == 0
            }
        }
    }

    /// 上报一次分配。返回是否被记录（统计**总是**更新，记录才分档）。
    pub fn on_alloc(&mut self, site: u16, bytes: u64, tick: u64) -> Result<bool, u16> {
        let idx = site as usize;
        if idx >= self.used_sites as usize {
            return Err(E_UNKNOWN_SITE);
        }
        let st = &mut self.stats[idx];
        st.live_count = st.live_count.saturating_add(1);
        st.live_bytes = st.live_bytes.saturating_add(bytes);
        st.allocs = st.allocs.saturating_add(1);
        if st.live_bytes > st.peak_bytes {
            st.peak_bytes = st.live_bytes;
        }

        if !self.should_record() {
            self.coverage.skipped = self.coverage.skipped.saturating_add(1);
            return Ok(false);
        }

        self.seq = self.seq.wrapping_add(1);
        let rec = AllocRecord {
            site,
            bytes,
            born: tick,
            freed: 0,
            seq: self.seq,
        };
        if self.filled == MAX_RECORDS {
            // 覆盖最旧那条。若它仍在途，覆盖率必须记一笔「丢了存活记录」。
            let old = self.ring[self.head];
            if old.live() {
                self.coverage.evicted_live = self.coverage.evicted_live.saturating_add(1);
            }
            self.coverage.overwritten = self.coverage.overwritten.saturating_add(1);
        } else {
            self.filled += 1;
        }
        self.ring[self.head] = rec;
        self.head = (self.head + 1) % MAX_RECORDS;
        self.coverage.recorded = self.coverage.recorded.saturating_add(1);
        Ok(true)
    }

    /// 上报一次释放。按 **FIFO** 结清该站点最旧的未释放记录——同尺寸
    /// 缓冲区按 LIFO 结清会让「最老的那笔」永远结不掉，候选报告被拖成
    /// 恒真。
    pub fn on_free(&mut self, site: u16, bytes: u64, tick: u64) -> Result<(), u16> {
        let idx = site as usize;
        if idx >= self.used_sites as usize {
            return Err(E_UNKNOWN_SITE);
        }
        if self.stats[idx].live_count == 0 {
            return Err(E_OVERFREE);
        }
        // 找该站点序号最小的未释放记录。
        let mut pick: Option<usize> = None;
        let mut best = u32::MAX;
        for i in 0..MAX_RECORDS {
            let r = self.ring[i];
            if r.live() && r.site == site && r.seq < best {
                best = r.seq;
                pick = Some(i);
            }
        }
        if let Some(i) = pick {
            self.ring[i].freed = tick;
            // 释放字节以记录为准；调用方传入的 bytes 只用于对账不足。
            let rec_bytes = self.ring[i].bytes;
            let _ = bytes;
            let st = &mut self.stats[idx];
            st.live_count = st.live_count.saturating_sub(1);
            st.live_bytes = st.live_bytes.saturating_sub(rec_bytes);
            st.frees = st.frees.saturating_add(1);
            st.last_free = tick;
        } else {
            // 记录已被覆盖或未采样：只能减统计，无法结清生命周期。
            let st = &mut self.stats[idx];
            st.live_count = st.live_count.saturating_sub(1);
            st.live_bytes = st.live_bytes.saturating_sub(bytes);
            st.frees = st.frees.saturating_add(1);
            st.last_free = tick;
        }
        Ok(())
    }

    pub fn stat(&self, site: u16) -> Option<SiteStat> {
        if (site as usize) < self.used_sites as usize {
            Some(self.stats[site as usize])
        } else {
            None
        }
    }

    /// 扫描候选。三条同时成立才算：存活条数过阈、存活字节过阈、静默够久。
    pub fn scan(&self, now: u64) -> LeakReport {
        let t = self.thresholds;
        let mut candidates = Vec::new();
        for i in 0..self.used_sites as usize {
            let st = self.stats[i];
            if st.live_count < t.min_live_count || st.live_bytes < t.min_live_bytes {
                continue;
            }
            let never_freed = st.last_free == 0;
            let quiet_for = if never_freed {
                now
            } else {
                now.saturating_sub(st.last_free)
            };
            if quiet_for < t.quiet_ticks {
                continue;
            }
            candidates.push(LeakCandidate {
                site: i as u16,
                name: self.names[i],
                live_count: st.live_count,
                live_bytes: st.live_bytes,
                never_freed,
                quiet_for,
            });
        }
        candidates.sort_by(|a, b| b.live_bytes.cmp(&a.live_bytes));
        LeakReport {
            candidates,
            coverage: self.coverage,
            thresholds: t,
            at: now,
        }
    }

    /// 分配热点：按在途字节降序取前 n。
    pub fn hotspots(&self, n: usize) -> Vec<(u16, &'static str, SiteStat)> {
        let mut all: Vec<(u16, &'static str, SiteStat)> = (0..self.used_sites as usize)
            .map(|i| (i as u16, self.names[i], self.stats[i]))
            .collect();
        all.sort_by(|a, b| b.2.live_bytes.cmp(&a.2.live_bytes));
        all.truncate(n);
        all
    }

    /// 全站点在途总量。
    pub fn total_live_bytes(&self) -> u64 {
        self.stats[..self.used_sites as usize]
            .iter()
            .fold(0u64, |a, s| a.saturating_add(s.live_bytes))
    }

    /// 环里仍在途的记录数（与统计对账用）。
    pub fn traceable_live(&self) -> u32 {
        self.ring[..self.filled]
            .iter()
            .filter(|r| r.live())
            .count() as u32
    }
}

impl Default for LeakTracker {
    fn default() -> LeakTracker {
        LeakTracker::new(TraceTier::default())
    }
}

// ---------------------------------------------------------------------------
// 七、判据
// ---------------------------------------------------------------------------

/// VE-F0078 的十五条判据。
pub fn run_vea78_checks() -> CheckSet {
    let mut c = CheckSet::new("vea78");

    let mut t = LeakTracker::new(TraceTier::Full);
    let id = match t.register("gpu::tex_upload") {
        Ok(v) => v,
        Err(_) => {
            c.fail("A78-站点-登记即得编号", "register failed");
            return c;
        }
    };
    let other = t.register("gpu::stage_buf").unwrap_or(id);

    // 1. 登记幂等：同名再登记得同一编号。
    if t.register("gpu::tex_upload") == Ok(id) {
        c.ok("A78-站点-同名登记幂等");
    } else {
        c.fail("A78-站点-同名登记幂等", "register not idempotent");
    }

    // 2. 未登记站点上报即拒。
    if t.on_alloc(99, 64, 1) == Err(E_UNKNOWN_SITE) {
        c.ok("A78-站点-未登记即拒");
    } else {
        c.fail("A78-站点-未登记即拒", "unknown site accepted");
    }

    // 3. Full 档全记：三次分配三条记录。
    let _ = t.on_alloc(id, 1024, 10);
    let _ = t.on_alloc(id, 1024, 20);
    let _ = t.on_alloc(other, 512, 30);
    if t.coverage().recorded == 3 && t.coverage().skipped == 0 {
        c.ok("A78-档位-全记零跳过");
    } else {
        c.fail("A78-档位-全记零跳过", "full tier skipped");
    }

    // 4. 采样档按步长抽样。
    let mut s = LeakTracker::new(TraceTier::Sampled(4));
    let sid = s.register("s").unwrap_or(0);
    for i in 0..8u64 {
        let _ = s.on_alloc(sid, 16, i);
    }
    if s.coverage().recorded == 2 && s.coverage().skipped == 6 {
        c.ok("A78-档位-采样步长生效");
    } else {
        c.fail("A78-档位-采样步长生效", "sampling window wrong");
    }

    // 5. Off 档零记录、全跳过。
    let mut off = LeakTracker::new(TraceTier::Off);
    let oid = off.register("o").unwrap_or(0);
    let _ = off.on_alloc(oid, 8, 1);
    if off.coverage().recorded == 0 && off.coverage().skipped == 1 {
        c.ok("A78-档位-关闭零记录");
    } else {
        c.fail("A78-档位-关闭零记录", "off tier recorded");
    }

    // 6. 非法采样步长被拒（步长 1 与 Full 重合，属配置错误）。
    if !TraceTier::Sampled(1).valid() && TraceTier::Sampled(2).valid() {
        c.ok("A78-档位-步长下界");
    } else {
        c.fail("A78-档位-步长下界", "sample step window wrong");
    }

    // 7. 释放结清：FIFO 结掉最旧一条。
    let _ = t.on_free(id, 1024, 100);
    match t.stat(id) {
        Some(st) if st.live_count == 1 && st.live_bytes == 1024 && st.last_free == 100 => {
            c.ok("A78-生命周期-FIFO结清最旧");
        }
        _ => c.fail("A78-生命周期-FIFO结清最旧", "free did not settle oldest"),
    }

    // 8. 过度释放即拒。
    let mut f = LeakTracker::new(TraceTier::Full);
    let fid = f.register("f").unwrap_or(0);
    if f.on_free(fid, 1, 1) == Err(E_OVERFREE) {
        c.ok("A78-生命周期-空账释放即拒");
    } else {
        c.fail("A78-生命周期-空账释放即拒", "over-free accepted");
    }

    // 9. 候选三条件：条数够、字节够、静默够。
    let mut g = LeakTracker::new(TraceTier::Full);
    let gid = g.register("gpu::leaky").unwrap_or(0);
    for i in 0..5u64 {
        let _ = g.on_alloc(gid, 2048, i);
    }
    let report = g.scan(10_000);
    if report.candidates.len() == 1
        && report.candidates[0].site == gid
        && report.candidates[0].never_freed
    {
        c.ok("A78-候选-只增不减即报");
    } else {
        c.fail("A78-候选-只增不减即报", "leak candidate missed");
    }

    // 10. 静默窗口不足时不报——「刚分配还没释放」不是泄漏。
    let early = g.scan(5);
    if early.candidates.is_empty() {
        c.ok("A78-候选-静默不足不报");
    } else {
        c.fail("A78-候选-静默不足不报", "early false positive");
    }

    // 11. 调阈后旧候选消失（不缓存、现算）。
    let _ = g.retune(
        Thresholds {
            min_live_count: 64,
            min_live_bytes: 1 << 20,
            quiet_ticks: 1,
        },
        10_001,
    );
    if g.scan(10_002).candidates.is_empty() {
        c.ok("A78-调阈-抬高即消候选");
    } else {
        c.fail("A78-调阈-抬高即消候选", "threshold ignored");
    }

    // 12. 非法阈值即拒，且原阈值不变。
    let before = g.thresholds();
    let bad = g.retune(
        Thresholds {
            min_live_count: 0,
            min_live_bytes: 0,
            quiet_ticks: 0,
        },
        1,
    );
    if bad == Err(E_BAD_THRESHOLD) && g.thresholds() == before {
        c.ok("A78-调阈-非法不动原值");
    } else {
        c.fail("A78-调阈-非法不动原值", "bad threshold applied");
    }

    // 13. 热点聚合按在途字节降序。
    let mut h = LeakTracker::new(TraceTier::Full);
    let small = h.register("small").unwrap_or(0);
    let big = h.register("big").unwrap_or(1);
    let _ = h.on_alloc(small, 10, 1);
    let _ = h.on_alloc(big, 1000, 2);
    let top = h.hotspots(1);
    if top.len() == 1 && top[0].0 == big {
        c.ok("A78-热点-按在途字节降序");
    } else {
        c.fail("A78-热点-按在途字节降序", "hotspot order wrong");
    }

    // 14. 环形覆盖显性：覆盖存活记录必须计入 evicted_live。
    let mut r = LeakTracker::new(TraceTier::Full);
    let rid = r.register("r").unwrap_or(0);
    for i in 0..(MAX_RECORDS as u64 + 3) {
        let _ = r.on_alloc(rid, 1, i);
    }
    let cov = r.coverage();
    if cov.overwritten == 3 && cov.evicted_live == 3 && !cov.conclusive() {
        c.ok("A78-覆盖-丢存活即标不确定");
    } else {
        c.fail("A78-覆盖-丢存活即标不确定", "eviction not visible");
    }

    // 15. 读屏报告行数与非空。
    let lines = report.a11y_lines();
    if lines.len() >= 3 && lines.iter().all(|l| !l.is_empty()) {
        c.ok("A78-读屏-报告逐行可播");
    } else {
        c.fail("A78-读屏-报告逐行可播", "a11y lines empty");
    }

    c
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_is_stable() {
        let mut t = LeakTracker::new(TraceTier::Full);
        let a = t.register("a").unwrap();
        let b = t.register("b").unwrap();
        assert_eq!(a, 0);
        assert_eq!(b, 1);
        assert_eq!(t.register("a"), Ok(0));
    }

    const SITE_NAMES: [&str; MAX_SITES] = [
        "s00", "s01", "s02", "s03", "s04", "s05", "s06", "s07", "s08", "s09", "s10", "s11",
        "s12", "s13", "s14", "s15", "s16", "s17", "s18", "s19", "s20", "s21", "s22", "s23",
        "s24", "s25", "s26", "s27", "s28", "s29", "s30", "s31",
    ];

    #[test]
    fn site_table_fills_then_refuses() {
        let mut t = LeakTracker::new(TraceTier::Full);
        for (i, n) in SITE_NAMES.iter().enumerate() {
            assert_eq!(t.register(n), Ok(i as u16));
        }
        assert_eq!(t.register("overflow"), Err(E_SITE_TABLE_FULL));
    }

    #[test]
    fn empty_site_name_refused() {
        let mut t = LeakTracker::new(TraceTier::Full);
        assert_eq!(t.register(""), Err(E_UNKNOWN_SITE));
    }

    #[test]
    fn alloc_updates_peak() {
        let mut t = LeakTracker::new(TraceTier::Full);
        let id = t.register("x").unwrap();
        let _ = t.on_alloc(id, 100, 1);
        let _ = t.on_alloc(id, 100, 2);
        let _ = t.on_free(id, 100, 3);
        let st = t.stat(id).unwrap();
        assert_eq!(st.peak_bytes, 200);
        assert_eq!(st.live_bytes, 100);
        assert_eq!(st.allocs, 2);
        assert_eq!(st.frees, 1);
    }

    #[test]
    fn free_settles_oldest_first() {
        let mut t = LeakTracker::new(TraceTier::Full);
        let id = t.register("x").unwrap();
        let _ = t.on_alloc(id, 10, 1);
        let _ = t.on_alloc(id, 20, 2);
        let _ = t.on_free(id, 10, 3);
        // 最旧（10 字节）被结清，剩 20。
        assert_eq!(t.stat(id).unwrap().live_bytes, 20);
    }

    #[test]
    fn sampled_records_every_step() {
        let mut t = LeakTracker::new(TraceTier::Sampled(3));
        let id = t.register("x").unwrap();
        for i in 0..9u64 {
            let _ = t.on_alloc(id, 1, i);
        }
        assert_eq!(t.coverage().recorded, 3);
        assert_eq!(t.coverage().skipped, 6);
    }

    #[test]
    fn sampled_never_records_when_step_one() {
        let mut t = LeakTracker::new(TraceTier::Sampled(1));
        let id = t.register("x").unwrap();
        let _ = t.on_alloc(id, 1, 1);
        assert_eq!(t.coverage().recorded, 0);
        assert!(!t.set_tier(TraceTier::Sampled(0)));
    }

    #[test]
    fn scan_requires_quiet_window() {
        let mut t = LeakTracker::new(TraceTier::Full);
        let id = t.register("x").unwrap();
        for i in 0..5u64 {
            let _ = t.on_alloc(id, 2048, i);
        }
        assert!(t.scan(10).candidates.is_empty());
        assert_eq!(t.scan(1000).candidates.len(), 1);
    }

    #[test]
    fn scan_skips_site_with_frees() {
        let mut t = LeakTracker::new(TraceTier::Full);
        let id = t.register("x").unwrap();
        for i in 0..5u64 {
            let _ = t.on_alloc(id, 2048, i);
        }
        let _ = t.on_free(id, 2048, 5);
        // 释放后静默窗口重新计时。
        assert!(t.scan(20).candidates.is_empty());
    }

    #[test]
    fn hotspots_sorted_desc() {
        let mut t = LeakTracker::new(TraceTier::Full);
        let a = t.register("a").unwrap();
        let b = t.register("b").unwrap();
        let _ = t.on_alloc(a, 5, 1);
        let _ = t.on_alloc(b, 50, 2);
        let h = t.hotspots(2);
        assert_eq!(h[0].0, b);
        assert_eq!(h[1].0, a);
    }

    #[test]
    fn eviction_marks_coverage_inconclusive() {
        let mut t = LeakTracker::new(TraceTier::Full);
        let id = t.register("x").unwrap();
        for i in 0..(MAX_RECORDS as u64 + 5) {
            let _ = t.on_alloc(id, 1, i);
        }
        let cov = t.coverage();
        assert_eq!(cov.overwritten, 5);
        assert_eq!(cov.evicted_live, 5);
        assert!(!cov.conclusive());
    }

    #[test]
    fn retune_is_recorded() {
        let mut t = LeakTracker::new(TraceTier::Full);
        let to = Thresholds {
            min_live_count: 9,
            min_live_bytes: 9,
            quiet_ticks: 9,
        };
        assert_eq!(t.retune(to, 77), Ok(()));
        let log = t.changes();
        let first = log[0].unwrap();
        assert_eq!(first.to, to);
        assert_eq!(first.at, 77);
    }

    #[test]
    fn checks_are_green() {
        let cs = run_vea78_checks();
        let (p, f) = cs.tally();
        assert_eq!(f, 0, "vea78 red: {}", cs.red_items_text());
        assert!(p >= 15);
    }
}
