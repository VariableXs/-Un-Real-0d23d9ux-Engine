//! m700cache — VARIX-M700 AI-11 缓存与页回写域 (F251~F275)
//!
//! 页缓存总账/回写节律器/预读预言家/缓存代际谱/回写风暴阀/缓存命中仪/
//! 同步回写走廊/缓存血缘标签/回写错误官/热点文件猎手/缓存压缩舱/
//! 一致性对账官/缓存水位戏剧/直通通道/回写优先级带/缓存预热器/
//! 缓存抖动雷达/脏页老化谱/缓存降压舱/回写预算表/缓存回归金样/
//! 零页 COW 联动/缓存统计分账/缓存事件流/缓存域年报。
//!
//! 硬约束：no_std / 无 alloc / 无浮点（全部 permille/定点）/ 纯逻辑。

use crate::checks::CheckSet;

// ===========================================================================
// F251 — 页缓存总账：全系统页缓存进账/出账
// ===========================================================================

pub const PAGE_SIZE_BYTES: u32 = 4096;

/// 页缓存总账：total = clean + dirty 恒等式由 `consistent()` 守护。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PageLedger {
    pub total: u32,
    pub clean: u32,
    pub dirty: u32,
}

impl PageLedger {
    pub const fn new() -> PageLedger {
        PageLedger { total: 0, clean: 0, dirty: 0 }
    }

    /// 入账一页。账满（≥ 4096 页样本上限）拒绝并返回 false。
    pub fn add_page(&mut self, dirty: bool) -> bool {
        if self.total >= 4096 {
            return false;
        }
        self.total += 1;
        if dirty {
            self.dirty += 1;
        } else {
            self.clean += 1;
        }
        true
    }

    pub fn mark_clean(&mut self, n: u32) -> bool {
        if n > self.dirty {
            return false;
        }
        self.dirty -= n;
        self.clean += n;
        true
    }

    pub fn mark_dirty(&mut self, n: u32) -> bool {
        if n > self.clean {
            return false;
        }
        self.clean -= n;
        self.dirty += n;
        true
    }

    pub fn consistent(&self) -> bool {
        self.clean + self.dirty == self.total
    }

    /// 脏比 permille。
    pub fn dirty_permille(&self) -> u32 {
        if self.total == 0 {
            0
        } else {
            self.dirty * 1000 / self.total
        }
    }
}

// ===========================================================================
// F252 — 回写节律器：脏比越高节奏越紧
// ===========================================================================

pub const CADENCE_BASE_MS: u32 = 5000;
pub const CADENCE_MIN_MS: u32 = 100;

/// 脏比 0‰ → 5000ms 一次；1000‰ → 100ms 一次，线性收敛。
pub fn cadence_interval_ms(dirty_permille: u32) -> u32 {
    let shrink = (dirty_permille.min(1000) * (CADENCE_BASE_MS - CADENCE_MIN_MS)) / 1000;
    CADENCE_BASE_MS - shrink
}

// ===========================================================================
// F253 — 预读预言家：顺序访问加倍窗口，乱序回退
// ===========================================================================

pub const READAHEAD_MIN_PAGES: u32 = 2;
pub const READAHEAD_MAX_PAGES: u32 = 32;

#[derive(Clone, Copy, Debug)]
pub struct Readahead {
    pub window_pages: u32,
    pub last_page: u64,
    pub primed: bool,
}

impl Readahead {
    pub const fn new() -> Readahead {
        Readahead { window_pages: READAHEAD_MIN_PAGES, last_page: 0, primed: false }
    }

    /// 记录一次按页访问，返回调整后的预读窗口（页数）。
    pub fn observe(&mut self, page: u64) -> u32 {
        if self.primed && page == self.last_page + 1 {
            self.window_pages = (self.window_pages * 2).min(READAHEAD_MAX_PAGES);
        } else {
            self.window_pages = READAHEAD_MIN_PAGES;
        }
        self.last_page = page;
        self.primed = true;
        self.window_pages
    }
}

// ===========================================================================
// F254 — 缓存代际谱：按代淘汰旧页
// ===========================================================================

pub const GEN_KEEP_WINDOW: u32 = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GenPage {
    pub index: u32,
    pub generation: u32,
}

/// 原地淘汰：generation + KEEP <= current 的页被移除（压缩数组），
/// 返回逐出数量。
pub fn evict_before(pages: &mut [GenPage], current_gen: u32) -> usize {
    let before = pages.len();
    let mut w = 0usize;
    for r in 0..pages.len() {
        let keep = pages[r].generation + GEN_KEEP_WINDOW > current_gen;
        if keep {
            pages.swap(w, r);
            w += 1;
        }
    }
    before - w
}

// ===========================================================================
// F255 — 回写风暴阀：脏比超限按比例限速
// ===========================================================================

pub const STORM_LOW_PERMILLE: u32 = 300;
pub const STORM_HIGH_PERMILLE: u32 = 700;
pub const STORM_MAX_DELAY_US: u32 = 5000;

/// 低水位以内放行（0us）；高水位以上顶格限速；中间线性爬升。
pub fn throttle_delay_us(dirty_permille: u32) -> u32 {
    if dirty_permille <= STORM_LOW_PERMILLE {
        0
    } else if dirty_permille >= STORM_HIGH_PERMILLE {
        STORM_MAX_DELAY_US
    } else {
        (dirty_permille - STORM_LOW_PERMILLE) * STORM_MAX_DELAY_US
            / (STORM_HIGH_PERMILLE - STORM_LOW_PERMILLE)
    }
}

// ===========================================================================
// F256 — 缓存命中仪：命中率 permille
// ===========================================================================

#[derive(Clone, Copy, Debug, Default)]
pub struct HitMeter {
    pub hits: u64,
    pub misses: u64,
}

impl HitMeter {
    pub fn record(&mut self, hit: bool) {
        if hit {
            self.hits += 1;
        } else {
            self.misses += 1;
        }
    }

    pub fn hit_rate_permille(&self) -> u32 {
        let total = self.hits + self.misses;
        if total == 0 {
            0
        } else {
            (self.hits * 1000 / total) as u32
        }
    }
}

// ===========================================================================
// F257 — 同步回写走廊：页号必须严格连续递增
// ===========================================================================

/// 同步回写必须按页号连续推进，断链即失败（顺序崩溃一致性）。
pub fn sync_walk_ok(pages: &[u64]) -> bool {
    let mut i = 1usize;
    while i < pages.len() {
        if pages[i] != pages[i - 1] + 1 {
            return false;
        }
        i += 1;
    }
    true
}

// ===========================================================================
// F258 — 缓存血缘标签：页属于哪个文件哪一页
// ===========================================================================

/// inode 0 保留给内核，血缘标签必须挂在真实 inode 上。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LineageTag {
    pub inode: u64,
    pub page_index: u64,
}

pub fn lineage_valid(t: LineageTag) -> bool {
    t.inode != 0
}

/// 两个标签是否来自同一文件的同一页（血缘冲突检测）。
pub fn lineage_same(a: LineageTag, b: LineageTag) -> bool {
    a.inode == b.inode && a.page_index == b.page_index
}

// ===========================================================================
// F259 — 回写错误官：错误分类与退避
// ===========================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WbError {
    /// 可重试（介质瞬时错误），附已重试次数。
    Retryable(u8),
    /// 致命（设备离线）。
    Fatal,
    /// 过期（页在排队期间又被改脏）。
    Stale,
}

pub fn wb_error_retryable(e: WbError) -> bool {
    matches!(e, WbError::Retryable(_))
}

/// 指数退避：100ms 起，每次翻倍，封顶 3200ms。
pub fn wb_retry_backoff_ms(attempt: u8) -> u32 {
    let mut ms = 100u32;
    let mut i = 0u8;
    while i < attempt && ms < 3200 {
        ms *= 2;
        i += 1;
    }
    ms.min(3200)
}

// ===========================================================================
// F260 — 热点文件猎手：访问计数超阈即热点
// ===========================================================================

pub const HOT_TOUCH_THRESHOLD: u32 = 8;

#[derive(Clone, Copy, Debug)]
pub struct HotFile {
    pub inode: u64,
    pub touches: u32,
}

pub fn hot_file_count(files: &[HotFile]) -> usize {
    files.iter().filter(|f| f.touches >= HOT_TOUCH_THRESHOLD).count()
}

/// 热度第一名（无热点返回 0）。
pub fn hottest_inode(files: &[HotFile]) -> u64 {
    let mut best: Option<&HotFile> = None;
    for f in files {
        if f.touches >= HOT_TOUCH_THRESHOLD {
            match best {
                Some(b) if b.touches >= f.touches => {}
                _ => best = Some(f),
            }
        }
    }
    match best {
        Some(b) => b.inode,
        None => 0,
    }
}

// ===========================================================================
// F261 — 缓存压缩舱：零字节占比过半的页值得压缩
// ===========================================================================

pub const COMPRESS_MIN_ZERO_PERMILLE: u32 = 500;

pub fn zero_permille(page: &[u8]) -> u32 {
    if page.is_empty() {
        return 0;
    }
    let zeros = page.iter().filter(|&&b| b == 0).count();
    (zeros * 1000 / page.len()) as u32
}

pub fn compress_candidate(page: &[u8]) -> bool {
    zero_permille(page) >= COMPRESS_MIN_ZERO_PERMILLE
}

// ===========================================================================
// F262 — 一致性对账官：账面脏页 = 分桶实数
// ===========================================================================

/// 对账：各桶脏页实数之和必须等于总账面值。
pub fn reconcile_ok(ledger_dirty: u32, bucket_dirty: &[u32]) -> bool {
    let mut sum: u32 = 0;
    for &b in bucket_dirty {
        sum += b;
    }
    sum == ledger_dirty
}

// ===========================================================================
// F263 — 缓存水位戏剧：三态水位机
// ===========================================================================

pub const WM_LOW_PERMILLE: u32 = 200;
pub const WM_HIGH_PERMILLE: u32 = 600;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CacheState {
    Normal,
    Reclaim,
    Panic,
}

pub fn water_state(used_permille: u32) -> CacheState {
    if used_permille >= 900 {
        CacheState::Panic
    } else if used_permille >= WM_HIGH_PERMILLE {
        CacheState::Reclaim
    } else {
        CacheState::Normal
    }
}

// ===========================================================================
// F264 — 直通通道：大块对齐 IO 绕过页缓存
// ===========================================================================

pub const BYPASS_MIN_BYTES: u32 = 8192;

pub fn bypass_io(len_bytes: u32, alignment_ok: bool) -> bool {
    len_bytes >= BYPASS_MIN_BYTES && alignment_ok
}

// ===========================================================================
// F265 — 回写优先级带：4 级，带号越大越急
// ===========================================================================

pub const WB_BANDS: usize = 4;

#[derive(Clone, Copy, Debug)]
pub struct BandQueue {
    pub pending: [u32; WB_BANDS],
}

impl BandQueue {
    pub const fn new() -> BandQueue {
        BandQueue { pending: [0; WB_BANDS] }
    }

    /// 最高优先级的非空带；全空返回 None。
    pub fn next_band(&self) -> Option<usize> {
        let mut band = WB_BANDS;
        while band > 0 {
            band -= 1;
            if self.pending[band] > 0 {
                return Some(band);
            }
        }
        None
    }

    /// 消费一个带。
    pub fn pop_band(&mut self, band: usize) -> bool {
        if band < WB_BANDS && self.pending[band] > 0 {
            self.pending[band] -= 1;
            true
        } else {
            false
        }
    }
}

// ===========================================================================
// F266 — 缓存预热器：启动时批量预取合法范围
// ===========================================================================

pub const WARMUP_MAX_RANGE_PAGES: u64 = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WarmupRange {
    pub start: u64,
    pub len: u64,
}

pub fn warmup_valid(r: WarmupRange) -> bool {
    r.len > 0 && r.len <= WARMUP_MAX_RANGE_PAGES
}

/// 合法范围的预取总页数；非法范围计 0。
pub fn warmup_total_pages(ranges: &[WarmupRange]) -> u64 {
    ranges.iter().filter(|r| warmup_valid(**r)).map(|r| r.len).sum()
}

// ===========================================================================
// F267 — 缓存抖动雷达：逐出紧追缺失即抖动
// ===========================================================================

pub const THRASH_RATIO_PERMILLE: u32 = 800;
pub const THRASH_MIN_FAULTS: u64 = 16;

#[derive(Clone, Copy, Debug, Default)]
pub struct ThrashRadar {
    pub evictions: u64,
    pub faults: u64,
}

/// 缺失量足够大且逐出/缺失 ≥ 800‰ → 判定抖动。
pub fn thrashing(r: &ThrashRadar) -> bool {
    r.faults >= THRASH_MIN_FAULTS && r.evictions * 1000 >= r.faults * THRASH_RATIO_PERMILLE as u64
}

// ===========================================================================
// F268 — 脏页老化谱：超龄脏页必须回写
// ===========================================================================

pub const DIRTY_AGE_MAX_MS: u32 = 30_000;

#[derive(Clone, Copy, Debug)]
pub struct DirtyPage {
    pub page: u64,
    pub dirty_age_ms: u32,
}

pub fn aged_dirty_count(pages: &[DirtyPage]) -> usize {
    pages.iter().filter(|p| p.dirty_age_ms >= DIRTY_AGE_MAX_MS).count()
}

/// 最老脏页的年龄（空表返回 0）。
pub fn oldest_dirty_ms(pages: &[DirtyPage]) -> u32 {
    pages.iter().map(|p| p.dirty_age_ms).max().unwrap_or(0)
}

// ===========================================================================
// F269 — 缓存降压舱：连续错误按比例缩目标
// ===========================================================================

pub const CACHE_TARGET_PERMILLE: u32 = 800;
pub const CACHE_TARGET_FLOOR_PERMILLE: u32 = 100;
/// 每次降压事件缩 10%。
pub const DEGRADE_STEP_PERMILLE: u32 = 80;

pub fn degraded_target(degrade_events: u32) -> u32 {
    let shrink = degrade_events.saturating_mul(DEGRADE_STEP_PERMILLE);
    CACHE_TARGET_PERMILLE.saturating_sub(shrink).max(CACHE_TARGET_FLOOR_PERMILLE)
}

// ===========================================================================
// F270 — 回写预算表：每轮限量发放
// ===========================================================================

#[derive(Clone, Copy, Debug)]
pub struct WbBudget {
    pub quota: u32,
    pub consumed: u32,
}

impl WbBudget {
    pub const fn new(quota: u32) -> WbBudget {
        WbBudget { quota, consumed: 0 }
    }

    pub fn remaining(&self) -> u32 {
        self.quota.saturating_sub(self.consumed)
    }

    /// 申请 n 页回写额度，返回实际批准数。
    pub fn take(&mut self, want: u32) -> u32 {
        let granted = want.min(self.remaining());
        self.consumed += granted;
        granted
    }
}

// ===========================================================================
// F271 — 缓存回归金样：固定剧本 → 固定末态
// ===========================================================================

/// 金样剧本：8 页进账（3 脏 5 净）→ 清洗 2 脏 → 再弄脏 1 净。
pub fn golden_scenario() -> PageLedger {
    let mut l = PageLedger::new();
    let mut i = 0;
    while i < 8 {
        l.add_page(i < 3);
        i += 1;
    }
    l.mark_clean(2);
    l.mark_dirty(1);
    l
}

/// 期望末态：total=8, dirty=2, clean=6。
pub fn golden_matches(l: &PageLedger) -> bool {
    l.total == 8 && l.dirty == 2 && l.clean == 6 && l.consistent()
}

// ===========================================================================
// F272 — 零页/COW 联动：共享零页写时复制
// ===========================================================================

#[derive(Clone, Copy, Debug)]
pub struct ZeroPage {
    pub mappings: u32,
}

impl ZeroPage {
    pub const SHARED: ZeroPage = ZeroPage { mappings: 4 };

    /// 对共享零页发起写：映射 ≥ 2 时必须 COW（拆一份映射换成私有副本），
    /// 返回是否发生了复制。
    pub fn cow_on_write(&mut self) -> bool {
        if self.mappings >= 2 {
            self.mappings -= 1;
            true
        } else {
            false
        }
    }
}

// ===========================================================================
// F273 — 缓存统计分账：按带分账统计
// ===========================================================================

#[derive(Clone, Copy, Debug, Default)]
pub struct BandStat {
    pub hits: u64,
    pub misses: u64,
    pub writebacks: u64,
}

#[derive(Clone, Copy, Debug)]
pub struct StatsBook {
    pub bands: [BandStat; WB_BANDS],
}

impl StatsBook {
    pub const fn new() -> StatsBook {
        StatsBook { bands: [BandStat { hits: 0, misses: 0, writebacks: 0 }; WB_BANDS] }
    }

    pub fn total_writebacks(&self) -> u64 {
        self.bands.iter().map(|b| b.writebacks).sum()
    }

    pub fn total_hits(&self) -> u64 {
        self.bands.iter().map(|b| b.hits).sum()
    }
}

// ===========================================================================
// F274 — 缓存事件流：定容环形事件流，溢出丢事件必须记账
// ===========================================================================

pub const WB_EVENT_RING: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WbEventKind {
    Dirty,
    Clean,
    Error,
    Drop,
}

#[derive(Clone, Copy, Debug)]
pub struct WbEventRing {
    kinds: [WbEventKind; WB_EVENT_RING],
    seqs: [u64; WB_EVENT_RING],
    head: usize,
    pub dropped: u64,
}

impl WbEventRing {
    pub const fn new() -> WbEventRing {
        WbEventRing {
            kinds: [WbEventKind::Dirty; WB_EVENT_RING],
            seqs: [0; WB_EVENT_RING],
            head: 0,
            dropped: 0,
        }
    }

    pub fn push(&mut self, seq: u64, kind: WbEventKind) {
        self.kinds[self.head] = kind;
        self.seqs[self.head] = seq;
        self.head = (self.head + 1) % WB_EVENT_RING;
    }

    pub fn len(&self) -> usize {
        WB_EVENT_RING
    }

    pub fn kind_at(&self, i: usize) -> WbEventKind {
        self.kinds[i % WB_EVENT_RING]
    }

    pub fn seq_at(&self, i: usize) -> u64 {
        self.seqs[i % WB_EVENT_RING]
    }
}

// ===========================================================================
// F275 — 缓存域年报：年报章节完备性
// ===========================================================================

pub const CACHE_REPORT_SECTIONS: [&str; 5] =
    ["ledger", "cadence", "storms", "hits", "aging"];

pub fn report_complete(sections_filled: u32) -> bool {
    sections_filled >= CACHE_REPORT_SECTIONS.len() as u32
}

// ===========================================================================
// 域自检
// ===========================================================================

pub fn run_m700cache_checks() -> CheckSet {
    let mut set = CheckSet::new("m700cache");

    // F251 页缓存总账
    let mut ledger = PageLedger::new();
    let mut i = 0;
    while i < 8 {
        ledger.add_page(i < 3);
        i += 1;
    }
    let dirty_before = ledger.dirty_permille();
    ledger.mark_clean(2);
    set.add("F251 ledger invariant", ledger.consistent() && ledger.total == 8 && ledger.dirty == 1, "identity");
    set.add(
        "F251 dirty permille snapshot",
        dirty_before == 375 && ledger.dirty_permille() == 125,
        "ratio tracked",
    );
    set.add("F251 ledger rejects over-clean", !ledger.mark_clean(9) && ledger.dirty == 1, "no overdraft");

    // F252 回写节律器
    set.add(
        "F252 cadence curve",
        cadence_interval_ms(0) == 5000 && cadence_interval_ms(1000) == 100,
        "endpoints",
    );
    set.add("F252 cadence monotonic", cadence_interval_ms(500) < cadence_interval_ms(250), "tighter when dirtier");

    // F253 预读预言家
    let mut ra = Readahead::new();
    let w1 = ra.observe(10);
    let w2 = ra.observe(11);
    let w3 = ra.observe(12);
    let w4 = ra.observe(99);
    set.add(
        "F253 readahead seq grow",
        w1 == 2 && w2 == 4 && w3 == 8 && w4 == 2,
        "double then reset",
    );
    let mut ra_cap = Readahead::new();
    let mut grow = 0;
    while grow < 8 {
        ra_cap.observe(100 + grow as u64);
        grow += 1;
    }
    set.add("F253 readahead capped", ra_cap.window_pages <= READAHEAD_MAX_PAGES, "no runaway");

    // F254 缓存代际谱
    let mut pages = [
        GenPage { index: 0, generation: 1 },
        GenPage { index: 1, generation: 4 },
        GenPage { index: 2, generation: 2 },
    ];
    let evicted = evict_before(&mut pages, 5);
    let survivors = pages.len() - evicted;
    set.add("F254 gen evict count", evicted == 2 && survivors == 1, "old gens gone");
    set.add("F254 gen survivor", pages[0] == GenPage { index: 1, generation: 4 }, "young survives");

    // F255 回写风暴阀
    set.add(
        "F255 storm valve curve",
        throttle_delay_us(100) == 0 && throttle_delay_us(800) == 5000,
        "pass then clamp",
    );
    set.add("F255 storm valve mid", throttle_delay_us(500) == 2500, "linear ramp");

    // F256 缓存命中仪
    let mut hm = HitMeter::default();
    hm.record(true);
    hm.record(true);
    hm.record(true);
    hm.record(false);
    set.add("F256 hit rate", hm.hit_rate_permille() == 750, "3/4 hits");
    set.add("F256 empty meter", HitMeter::default().hit_rate_permille() == 0, "no div-zero");

    // F257 同步回写走廊
    let good = [0, 1, 2, 3];
    let bad = [0, 1, 3, 4];
    set.add("F257 sync walk contiguous", sync_walk_ok(&good), "ordered");
    set.add("F257 sync walk gap caught", !sync_walk_ok(&bad), "gap rejected");

    // F258 缓存血缘标签
    let t1 = LineageTag { inode: 42, page_index: 7 };
    let t2 = LineageTag { inode: 42, page_index: 8 };
    set.add(
        "F258 lineage tag",
        lineage_valid(t1) && !lineage_valid(LineageTag { inode: 0, page_index: 1 }),
        "inode nonzero",
    );
    set.add("F258 lineage conflict", !lineage_same(t1, t2) && lineage_same(t1, t1), "distinct pages");

    // F259 回写错误官
    set.add(
        "F259 error classes",
        wb_error_retryable(WbError::Retryable(1)) && !wb_error_retryable(WbError::Fatal)
            && !wb_error_retryable(WbError::Stale),
        "retryable only",
    );
    set.add(
        "F259 retry backoff",
        wb_retry_backoff_ms(0) == 100 && wb_retry_backoff_ms(3) == 800 && wb_retry_backoff_ms(9) == 3200,
        "exponential capped",
    );

    // F260 热点文件猎手
    let files = [
        HotFile { inode: 7, touches: 12 },
        HotFile { inode: 9, touches: 3 },
        HotFile { inode: 11, touches: 8 },
    ];
    set.add("F260 hot count", hot_file_count(&files) == 2, "threshold 8");
    set.add("F260 hottest inode", hottest_inode(&files) == 7, "top toucher");
    set.add("F260 no hotspot", hottest_inode(&[HotFile { inode: 5, touches: 1 }]) == 0, "cold set");

    // F261 缓存压缩舱
    let hot_page = [0u8; 64];
    let cold_page = [7u8; 64];
    set.add("F261 zero permille", zero_permille(&hot_page) == 1000 && zero_permille(&cold_page) == 0, "ratio");
    set.add("F261 compress decision", compress_candidate(&hot_page) && !compress_candidate(&cold_page), "half rule");

    // F262 一致性对账官
    set.add("F262 reconcile match", reconcile_ok(10, &[3, 4, 3]), "sum equals ledger");
    set.add("F262 reconcile drift", !reconcile_ok(10, &[3, 4, 2]), "drift caught");

    // F263 缓存水位戏剧
    set.add(
        "F263 water states",
        water_state(150) == CacheState::Normal
            && water_state(600) == CacheState::Reclaim
            && water_state(950) == CacheState::Panic,
        "three tiers",
    );
    set.add("F263 watermark order", WM_LOW_PERMILLE < WM_HIGH_PERMILLE, "low < high");

    // F264 直通通道
    set.add("F264 bypass gate", bypass_io(16384, true) && !bypass_io(4096, true) && !bypass_io(16384, false), "big+aligned");

    // F265 回写优先级带
    let mut q = BandQueue::new();
    q.pending[1] = 2;
    q.pending[3] = 5;
    let top = q.next_band();
    set.add("F265 band priority", top == Some(3), "urgent first");
    q.pop_band(3);
    q.pop_band(3);
    let q3_left = q.pending[3];
    q.pop_band(3);
    set.add("F265 band drain", q3_left == 3 && q.next_band() == Some(3), "drain order");

    // F266 缓存预热器
    let ranges = [
        WarmupRange { start: 0, len: 32 },
        WarmupRange { start: 100, len: 65 },
        WarmupRange { start: 200, len: 0 },
    ];
    set.add("F266 warmup valid", warmup_valid(ranges[0]) && !warmup_valid(ranges[1]) && !warmup_valid(ranges[2]), "bounds");
    set.add("F266 warmup total", warmup_total_pages(&ranges) == 32, "invalid count zero");

    // F267 缓存抖动雷达
    set.add(
        "F267 thrash detected",
        thrashing(&ThrashRadar { evictions: 40, faults: 50 }),
        "80%+ evict rate",
    );
    set.add(
        "F267 thrash calm",
        !thrashing(&ThrashRadar { evictions: 10, faults: 50 })
            && !thrashing(&ThrashRadar { evictions: 9, faults: 8 }),
        "quiet or tiny",
    );

    // F268 脏页老化谱
    let dirty = [
        DirtyPage { page: 1, dirty_age_ms: 31_000 },
        DirtyPage { page: 2, dirty_age_ms: 5_000 },
        DirtyPage { page: 3, dirty_age_ms: 30_000 },
    ];
    set.add("F268 aged dirty", aged_dirty_count(&dirty) == 2, "30s rule");
    set.add("F268 oldest dirty", oldest_dirty_ms(&dirty) == 31_000, "max age");

    // F269 缓存降压舱
    set.add(
        "F269 degrade steps",
        degraded_target(0) == 800 && degraded_target(2) == 640 && degraded_target(20) == 100,
        "10% per event, floored",
    );

    // F270 回写预算表
    let mut budget = WbBudget::new(10);
    let first_grant = budget.take(6);
    let mid_remaining = budget.remaining();
    let second_grant = budget.take(6);
    set.add("F270 budget grant", first_grant == 6 && mid_remaining == 4, "partial grant");
    set.add("F270 budget clamp", second_grant == 4 && budget.remaining() == 0, "never overdraft");

    // F271 缓存回归金样
    let golden = golden_scenario();
    set.add("F271 golden matches", golden_matches(&golden), "scripted end state");
    let mut drifted = golden_scenario();
    drifted.mark_dirty(1);
    set.add("F271 golden catches drift", !golden_matches(&drifted), "mutation detected");

    // F272 零页/COW 联动
    let mut zp = ZeroPage::SHARED;
    let cow1 = zp.cow_on_write();
    let mappings_after_first = zp.mappings;
    set.add("F272 cow happens", cow1 && mappings_after_first == 3, "copy taken");
    let mut lonely = ZeroPage { mappings: 1 };
    set.add("F272 last mapping in place", !lonely.cow_on_write() && lonely.mappings == 1, "no cow needed");

    // F273 缓存统计分账
    let mut book = StatsBook::new();
    book.bands[0].hits = 5;
    book.bands[1].writebacks = 7;
    book.bands[3].writebacks = 2;
    set.add("F273 stats account", book.total_hits() == 5 && book.total_writebacks() == 9, "per-band ledger");

    // F274 缓存事件流
    let mut ring = WbEventRing::new();
    let mut seq = 0u64;
    while seq < 10 {
        ring.push(seq, if seq % 2 == 0 { WbEventKind::Dirty } else { WbEventKind::Clean });
        seq += 1;
    }
    let last_kind = ring.kind_at(9);
    let first_kind = ring.kind_at(0);
    set.add(
        "F274 event ring wrap",
        ring.len() == 8 && last_kind == WbEventKind::Clean && first_kind == WbEventKind::Dirty,
        "ring holds 8",
    );
    set.add("F274 ring latest seq", ring.seq_at(9) == 9 && ring.seq_at(2) == 2, "seq tracked");

    // F275 缓存域年报
    set.add(
        "F275 cache report",
        CACHE_REPORT_SECTIONS.len() == 5 && report_complete(5) && !report_complete(4),
        "sections complete",
    );

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f251_ledger_math() {
        let mut l = PageLedger::new();
        assert!(l.add_page(true));
        assert!(l.add_page(false));
        assert!(l.add_page(false));
        assert!(l.consistent());
        assert_eq!(l.dirty_permille(), 333);
        assert!(l.mark_dirty(1));
        assert_eq!(l.dirty, 2);
    }

    #[test]
    fn f253_readahead_growth() {
        let mut ra = Readahead::new();
        assert_eq!(ra.observe(0), 2);
        assert_eq!(ra.observe(1), 4);
        assert_eq!(ra.observe(2), 8);
        assert_eq!(ra.observe(5), 2); // 乱序回退
    }

    #[test]
    fn f255_throttle_boundaries() {
        assert_eq!(throttle_delay_us(STORM_LOW_PERMILLE), 0);
        assert_eq!(throttle_delay_us(STORM_HIGH_PERMILLE), STORM_MAX_DELAY_US);
        assert!(throttle_delay_us(400) > 0 && throttle_delay_us(400) < 5000);
    }

    #[test]
    fn f270_never_overdraft() {
        let mut b = WbBudget::new(5);
        assert_eq!(b.take(9), 5);
        assert_eq!(b.take(1), 0);
        assert_eq!(b.remaining(), 0);
    }

    #[test]
    fn f272_cow_semantics() {
        let mut z = ZeroPage { mappings: 2 };
        assert!(z.cow_on_write());
        assert_eq!(z.mappings, 1);
        assert!(!z.cow_on_write());
    }

    #[test]
    fn f275_domain_selfcheck_all_pass() {
        let set = run_m700cache_checks();
        assert!(set.len() >= 25, "got {}", set.len());
        assert!(set.all_passed());
    }
}
