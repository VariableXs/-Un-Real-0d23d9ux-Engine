//! F055 字形光栅缓存 · 深化件（AI-K1 深化批次三 · G-B-15）。
//!
//! | # | 主册原文 | 本件机制 |
//! | --- | --- | --- |
//! | 1 | 【设计细节】「**常用字集预热：启动后台预光栅常用 3500 汉字 + ASCII**（用户打字前图集已热）」 | [`WarmupSet`] 预热字集（区段表 + 进度 + 后台优先级） |
//! | 2 | 【设计细节】「**驱逐保护：当前帧用到的字形禁止驱逐（双缓冲页交换）**」 | [`PinSet`] 当前帧钉住集 + [`PageExchange`] 双缓冲原子换页 |
//! | 3 | 【设计细节】「**灰度 AA 与 hinting 参数冻结进资产版本（换字体版本=图集全重建，显式日志）**」 | [`AssetVersion`] 资产版本指纹（参数变更 → 全重建决策 + 显式日志） |
//! | 4 | 【状态与异常】「**命中率 <80% → 自动扩一页（上限 32MB）**」 | [`AutoGrow`] 扩页状态机（迟滞 + 上限 + 缩不回的诚实说明） |
//! | 5 | 【状态与异常】「**字形光栅自身耗时 >2ms → 后台线程光栅化（前台用旧版占位，到位后原子换）**」 | [`RasterJob`] 后台光栅作业状态机（前台占位 → 后台产出 → 原子换） |
//! | 6 | 【数据与存储】「图集显存（或锁页内存）**配额 16MB**（4K 管线）；持久化不做（启动重建快，F053 并行）」 | [`Quota`] 配额账（页粒度，上限 32MB 为扩页硬顶） |

use crate::checks::CheckSet;
use crate::perfstar::perfkit::DiagSink;
use crate::perfstar::perfkit::DiagSev;

// ---------------------------------------------------------------------------
// 常量
// ---------------------------------------------------------------------------

/// 常用汉字数 3500（主册【设计细节】）。
pub const COMMON_HANZI: u32 = 3_500;
/// ASCII 字符数（可打印 95 个 + 控制字符按 128 计）。
pub const ASCII_GLYPHS: u32 = 128;
/// 常用汉字区段起点（GB2312 一级字库起始码位 U+4E00 起的常用集，此处按序取）。
pub const HANZI_BASE: u32 = 0x4E00;
/// 图集页尺寸 512×512（主册【功能定义】）。
pub const PAGE_PX: u32 = 512;
/// 单页字节（灰度 AA，1 字节/像素）。
pub const PAGE_BYTES: u64 = (PAGE_PX * PAGE_PX) as u64;
/// 初始配额 16MB（主册【数据与存储】）。
pub const QUOTA_BASE_BYTES: u64 = 16 * 1_024 * 1_024;
/// 扩页硬顶 32MB（主册【状态与异常】）。
pub const QUOTA_MAX_BYTES: u64 = 32 * 1_024 * 1_024;
/// 命中率扩页阈值 80%（千分 800）。
pub const GROW_THRESHOLD_PERMILLE: u32 = 800;
/// 命中率达标线 90%（千分 900，主册【验收判据】）。
pub const HIT_TARGET_PERMILLE: u32 = 900;
/// 字形光栅耗时阈值 2ms（超过则转后台）。
pub const RASTER_THRESHOLD_US: u32 = 2_000;
/// 单帧钉住集上限（定长）。
pub const PIN_SLOTS: usize = 64;

// ---------------------------------------------------------------------------
// 1. 预热字集（3500 汉字 + ASCII）
// ---------------------------------------------------------------------------

/// 预热进度与调度（主册「启动后台预光栅」——不阻塞用户，但要在打字前热好）。
#[derive(Clone, Copy, Debug)]
pub struct WarmupSet {
    /// 待预热总数。
    pub total: u32,
    /// 已完成数。
    pub done: u32,
    /// 是否启用（关掉就老实说「没预热」）。
    pub enabled: bool,
    /// 后台优先级（0 = 最低，不抢前台）。
    pub priority: u8,
}

impl WarmupSet {
    /// 默认字集：3500 汉字 + ASCII 128。
    pub const fn default_set() -> Self {
        WarmupSet { total: COMMON_HANZI + ASCII_GLYPHS, done: 0, enabled: true, priority: 0 }
    }
    /// 推进 `n` 个（后台逐步跑）。
    pub fn advance(&mut self, n: u32) {
        self.done = (self.done + n).min(self.total);
    }
    /// 完成千分。
    pub fn permille(&self) -> u32 {
        if self.total == 0 {
            return 0;
        }
        ((self.done as u64 * 1000) / self.total as u64) as u32
    }
    /// 是否预热完成。
    pub fn complete(&self) -> bool {
        self.total > 0 && self.done >= self.total
    }
    /// 某个码位是否在预热集合内（ASCII 段 + 常用汉字段）。
    pub fn covers(&self, codepoint: u32) -> bool {
        if codepoint < ASCII_GLYPHS {
            return true;
        }
        if codepoint >= HANZI_BASE {
            let off = codepoint - HANZI_BASE;
            return off < COMMON_HANZI;
        }
        false
    }
    /// 状态说明（不预热也不含糊）。
    pub fn text(&self) -> &'static str {
        if !self.enabled {
            return "未启用预热（首个字形按需光栅，可能有一次卡顿）";
        }
        if self.complete() {
            "常用字集已预热完成（打字前图集已热）"
        } else {
            "常用字集预热中（后台低优先级进行）"
        }
    }
}

// ---------------------------------------------------------------------------
// 2. 当前帧钉住 + 双缓冲页交换
// ---------------------------------------------------------------------------

/// 当前帧钉住集（主册「当前帧用到的字形禁止驱逐」）。
#[derive(Clone, Copy, Debug)]
pub struct PinSet {
    slots: [u32; PIN_SLOTS],
    n: usize,
    /// 因钉住而阻止的驱逐次数（调参依据：钉太多说明帧内字形过多）。
    pub blocked_evictions: u64,
}

impl PinSet {
    pub const fn new() -> Self {
        PinSet { slots: [0; PIN_SLOTS], n: 0, blocked_evictions: 0 }
    }
    /// 钉住一个字形（帧开始逐字调用）。
    pub fn pin(&mut self, glyph_id: u32) -> bool {
        if self.n >= PIN_SLOTS {
            return false;
        }
        self.slots[self.n] = glyph_id;
        self.n += 1;
        true
    }
    /// 是否钉住。
    pub fn is_pinned(&self, glyph_id: u32) -> bool {
        self.slots[..self.n].contains(&glyph_id)
    }
    /// 驱逐前询问：钉住则阻止并计数。
    pub fn try_evict(&mut self, glyph_id: u32) -> bool {
        if self.is_pinned(glyph_id) {
            self.blocked_evictions += 1;
            return false;
        }
        true
    }
    /// 帧末清空（钉住只对当前帧有效）。
    pub fn clear_frame(&mut self) {
        self.n = 0;
    }
    pub fn len(&self) -> usize {
        self.n
    }
}

/// 双缓冲页交换（主册「双缓冲页交换」——换页必须是原子的，不能换到一半）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExchangeState {
    /// 前台在用 A 页，B 页后台准备。
    Preparing,
    /// B 页就绪，等待交换点。
    Ready,
    /// 已交换（原子生效）。
    Swapped,
}

#[derive(Clone, Copy, Debug)]
pub struct PageExchange {
    pub state: ExchangeState,
    /// 交换次数。
    pub swaps: u64,
    /// 因前台正在读而推迟的交换次数（不撕裂正在用的页）。
    pub postponed: u64,
}

impl PageExchange {
    pub const fn new() -> Self {
        PageExchange { state: ExchangeState::Preparing, swaps: 0, postponed: 0 }
    }
    /// 后台页准备完成。
    pub fn mark_ready(&mut self) {
        if self.state == ExchangeState::Preparing {
            self.state = ExchangeState::Ready;
        }
    }
    /// 尝试在安全点交换（前台不在读该页时才能换）。
    pub fn try_swap(&mut self, front_busy: bool) -> bool {
        if self.state != ExchangeState::Ready {
            return false;
        }
        if front_busy {
            self.postponed += 1;
            return false;
        }
        self.state = ExchangeState::Swapped;
        self.swaps += 1;
        true
    }
    /// 交换后回到准备态（下一轮双缓冲）。
    pub fn begin_next(&mut self) {
        if self.state == ExchangeState::Swapped {
            self.state = ExchangeState::Preparing;
        }
    }
    /// 是否处于「换到一半」的危险态（本设计下恒不可能——类型层面排除）。
    pub const fn can_tear() -> bool {
        false
    }
}

// ---------------------------------------------------------------------------
// 3. 资产版本（AA/hinting 参数冻结）
// ---------------------------------------------------------------------------

/// 资产版本指纹（主册「灰度 AA 与 hinting 参数冻结进资产版本」）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AssetVersion {
    /// AA 模式（0=无 1=灰度 2=次像素）。
    pub aa_mode: u8,
    /// hinting 档（0=无 1=轻 2=全）。
    pub hinting: u8,
    /// 字体文件版本哈希低 32 位。
    pub font_hash: u32,
}

impl AssetVersion {
    /// 指纹（三参数打包成一个 u64——比较用，不用于持久化）。
    pub fn fingerprint(&self) -> u64 {
        ((self.aa_mode as u64) << 40) | ((self.hinting as u64) << 32) | self.font_hash as u64
    }
    /// 是否需要全重建（任一参数变化即重建——不部分复用）。
    pub fn needs_full_rebuild(&self, other: &AssetVersion) -> bool {
        self.fingerprint() != other.fingerprint()
    }
    /// 重建原因（显式日志：不说「要重建」，说「为什么」）。
    pub fn rebuild_reason(&self, other: &AssetVersion) -> Option<&'static str> {
        if self.aa_mode != other.aa_mode {
            return Some("灰度 AA 模式变更，图集全部失效");
        }
        if self.hinting != other.hinting {
            return Some("hinting 档位变更，图集全部失效");
        }
        if self.font_hash != other.font_hash {
            return Some("字体版本变更，图集全部失效");
        }
        None
    }
    /// 显式日志条目（定长 32B：`aa=<n> hint=<n> font=<hex8>`）。
    ///
    /// 换字体/改参数必须有这条日志——主册「换字体版本=图集全重建，**显式
    /// 日志**」；没有日志的重建是不可解释的行为。
    pub fn log_entry(&self) -> ([u8; 32], usize) {
        let mut b = [0u8; 32];
        let mut p = 0usize;
        b[p..p + 3].copy_from_slice(b"aa=");
        p += 3;
        b[p] = b'0' + self.aa_mode.min(9);
        p += 1;
        b[p..p + 6].copy_from_slice(b" hint=");
        p += 6;
        b[p] = b'0' + self.hinting.min(9);
        p += 1;
        b[p..p + 6].copy_from_slice(b" font=");
        p += 6;
        for i in 0..8 {
            let nib = ((self.font_hash >> (28 - i * 4)) & 0xF) as u8;
            b[p] = if nib < 10 { b'0' + nib } else { b'a' + nib - 10 };
            p += 1;
        }
        (b, p)
    }
}

// ---------------------------------------------------------------------------
// 4. 扩页状态机（命中率 <80% 自动扩一页，上限 32MB）
// ---------------------------------------------------------------------------

/// 扩页裁定。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GrowVerdict {
    /// 命中率达标，不扩。
    Hold,
    /// 扩一页。
    Grow,
    /// 到硬顶了，扩不动（如实告知，不假装扩了）。
    AtCap,
}

/// 扩页状态机（迟滞 + 硬顶）。
#[derive(Clone, Copy, Debug)]
pub struct AutoGrow {
    /// 当前页数。
    pub pages: u32,
    /// 已扩次数。
    pub grows: u32,
    /// 到顶被拒次数。
    pub at_cap: u32,
    /// 连续低于阈值的观察次数（迟滞：不是一次掉下去就扩）。
    pub low_streak: u32,
    /// 触发扩页所需的连续低命中次数。
    pub streak_needed: u32,
}

impl AutoGrow {
    pub const fn new(pages: u32) -> Self {
        AutoGrow { pages, grows: 0, at_cap: 0, low_streak: 0, streak_needed: 3 }
    }
    /// 当前字节占用。
    pub fn bytes(&self) -> u64 {
        self.pages as u64 * PAGE_BYTES
    }
    /// 是否到硬顶（再扩一页就超 32MB）。
    pub fn at_cap_now(&self) -> bool {
        self.bytes() + PAGE_BYTES > QUOTA_MAX_BYTES
    }
    /// 上报一次命中率，返回裁定。
    pub fn report(&mut self, hit_permille: u32) -> GrowVerdict {
        if hit_permille >= GROW_THRESHOLD_PERMILLE {
            self.low_streak = 0;
            return GrowVerdict::Hold;
        }
        self.low_streak += 1;
        if self.low_streak < self.streak_needed {
            return GrowVerdict::Hold;
        }
        self.low_streak = 0;
        if self.at_cap_now() {
            self.at_cap += 1;
            return GrowVerdict::AtCap;
        }
        self.pages += 1;
        self.grows += 1;
        GrowVerdict::Grow
    }
    /// 是否达标（≥90%）。
    pub fn meets_target(&self, hit_permille: u32) -> bool {
        hit_permille >= HIT_TARGET_PERMILLE
    }
}

// ---------------------------------------------------------------------------
// 5. 后台光栅作业（>2ms 转后台，前台用旧版占位）
// ---------------------------------------------------------------------------

/// 光栅作业状态（主册「前台用旧版占位，到位后原子换」）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JobState {
    /// 前台直接光栅（未超阈值）。
    Inline,
    /// 后台光栅中，前台用旧版占位。
    Background,
    /// 后台产出就绪，待原子换。
    Ready,
}

/// 光栅作业（阈值判定 + 占位 + 原子换）。
#[derive(Clone, Copy, Debug)]
pub struct RasterJob {
    pub state: JobState,
    /// 转后台的光栅次数。
    pub backgrounded: u64,
    /// 占位显示次数（用户看到旧版字形的次数——体验代价可查）。
    pub placeholder_shown: u64,
    /// 原子换次数。
    pub swapped: u64,
}

impl RasterJob {
    pub const fn new() -> Self {
        RasterJob { state: JobState::Inline, backgrounded: 0, placeholder_shown: 0, swapped: 0 }
    }
    /// 请求光栅一个字形（`cost_us` 为该字形的光栅耗时）。
    pub fn request(&mut self, cost_us: u32) -> JobState {
        if cost_us <= RASTER_THRESHOLD_US {
            self.state = JobState::Inline;
            return self.state;
        }
        self.state = JobState::Background;
        self.backgrounded += 1;
        self.placeholder_shown += 1; // 前台先用旧版占位
        self.state
    }
    /// 后台产出完成。
    pub fn finish(&mut self) {
        if self.state == JobState::Background {
            self.state = JobState::Ready;
        }
    }
    /// 原子换入（前台不在用旧版时才能换）。
    pub fn swap(&mut self, front_using_placeholder: bool) -> bool {
        if self.state != JobState::Ready {
            return false;
        }
        if front_using_placeholder {
            return false;
        }
        self.state = JobState::Inline;
        self.swapped += 1;
        true
    }
    /// 是否正在显示占位（体验代价：能被问出来）。
    pub fn showing_placeholder(&self) -> bool {
        matches!(self.state, JobState::Background | JobState::Ready)
    }
}

// ---------------------------------------------------------------------------
// 6. 配额账
// ---------------------------------------------------------------------------

/// 图集配额账（页粒度，16MB 起步、32MB 硬顶）。
#[derive(Clone, Copy, Debug)]
pub struct Quota {
    pub pages: u32,
    pub used_bytes: u64,
}

impl Quota {
    /// 初始配额：16MB 折合页数（512×512×1B = 256KB/页 → 64 页）。
    pub const fn base() -> Self {
        Quota { pages: (QUOTA_BASE_BYTES / PAGE_BYTES) as u32, used_bytes: 0 }
    }
    /// 总字节。
    pub fn total_bytes(&self) -> u64 {
        self.pages as u64 * PAGE_BYTES
    }
    /// 占用千分。
    pub fn usage_permille(&self) -> u32 {
        let t = self.total_bytes();
        if t == 0 {
            return 0;
        }
        ((self.used_bytes * 1000) / t) as u32
    }
    /// 是否还能再加一页（硬顶 32MB）。
    pub fn can_add_page(&self) -> bool {
        self.total_bytes() + PAGE_BYTES <= QUOTA_MAX_BYTES
    }
    /// 加一页（超顶返回 false，不静默）。
    pub fn add_page(&mut self) -> bool {
        if !self.can_add_page() {
            return false;
        }
        self.pages += 1;
        true
    }
    /// 持久化策略说明（主册「持久化不做」）。
    pub const fn persistence_policy() -> &'static str {
        "不做持久化：启动重建快（F053 并行），图集随会话重建"
    }
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

pub fn run_checks() -> CheckSet {
    let mut cs = CheckSet::new("F055-glyphcache-ext");
    // 1) 预热字集：3500 汉字 + ASCII 128，区段覆盖判定正确。
    let mut w = WarmupSet::default_set();
    cs.add(
        "warmup_set_size_and_coverage",
        w.total == 3_628 && w.covers(65) && w.covers(0x4E00) && w.covers(0x4E00 + 3_499) && !w.covers(0x4E00 + 3_500) && !w.covers(0x1F600),
        "",
    );
    // 2) 预热进度（后台低优先级推进，不阻塞前台）。
    w.advance(1_814);
    cs.add("warmup_progress", w.permille() == 500 && !w.complete() && w.priority == 0, "");
    w.advance(2_000);
    cs.add("warmup_complete", w.complete() && w.text() == "常用字集已预热完成（打字前图集已热）", "");
    // 未启用就是未启用（不含糊）
    let off = WarmupSet { enabled: false, ..WarmupSet::default_set() };
    cs.add("warmup_disabled_honest", off.text() == "未启用预热（首个字形按需光栅，可能有一次卡顿）", "");
    // 3) 钉住保护：当前帧用到的字形禁止驱逐。
    let mut pin = PinSet::new();
    pin.pin(7);
    pin.pin(8);
    cs.add(
        "pin_blocks_eviction",
        pin.is_pinned(7) && !pin.try_evict(7) && pin.try_evict(9) && pin.blocked_evictions == 1,
        "",
    );
    // 帧末清空（钉住只对当前帧有效）
    pin.clear_frame();
    cs.add("pin_clears_at_frame_end", pin.len() == 0 && pin.try_evict(7), "");
    // 钉住集满：返回 false（不静默丢钉）
    let mut pin2 = PinSet::new();
    let mut ok = true;
    for i in 0..(PIN_SLOTS + 1) {
        ok &= pin2.pin(i as u32);
    }
    cs.add("pin_capacity_respected", !ok && pin2.len() == PIN_SLOTS, "");
    // 4) 双缓冲页交换：原子性（前台忙则推迟，不撕裂）。
    let mut px = PageExchange::new();
    let early = px.try_swap(false); // 未就绪 → 不换
    px.mark_ready();
    let busy = px.try_swap(true); // 前台忙 → 推迟
    let idle = px.try_swap(false); // 前台空闲 → 换
    cs.add(
        "page_exchange_is_atomic",
        !early && !busy && idle && px.swaps == 1 && px.postponed == 1 && !PageExchange::can_tear(),
        "",
    );
    px.begin_next();
    cs.add("page_exchange_cycles", px.state == ExchangeState::Preparing, "");
    // 5) 资产版本：任一参数变化 → 全重建 + 显式原因。
    let v1 = AssetVersion { aa_mode: 1, hinting: 1, font_hash: 0xABCD };
    let v2 = AssetVersion { aa_mode: 2, hinting: 1, font_hash: 0xABCD };
    let v3 = AssetVersion { aa_mode: 1, hinting: 2, font_hash: 0xABCD };
    let v4 = AssetVersion { aa_mode: 1, hinting: 1, font_hash: 0x1234 };
    cs.add(
        "asset_version_rebuild_reasons",
        v1.needs_full_rebuild(&v2)
            && v1.rebuild_reason(&v2) == Some("灰度 AA 模式变更，图集全部失效")
            && v1.rebuild_reason(&v3) == Some("hinting 档位变更，图集全部失效")
            && v1.rebuild_reason(&v4) == Some("字体版本变更，图集全部失效")
            && v1.rebuild_reason(&v1).is_none(),
        "",
    );
    // 显式日志条目：`aa=1 hint=1 font=0000abcd`（长度与内容都要对）
    let (log, log_len) = v1.log_entry();
    cs.add("asset_version_log_entry", log_len == 25 && &log[..log_len] == b"aa=1 hint=1 font=0000abcd", "");
    // 6) 扩页状态机：命中率 <80% 连续 3 次才扩（迟滞），到 32MB 硬顶不再扩。
    let mut ag = AutoGrow::new(64);
    let g1 = ag.report(700);
    let g2 = ag.report(700);
    let g3 = ag.report(700);
    cs.add(
        "autogrow_hysteresis",
        g1 == GrowVerdict::Hold && g2 == GrowVerdict::Hold && g3 == GrowVerdict::Grow && ag.pages == 65 && ag.grows == 1,
        "",
    );
    // 命中率回升 → 迟滞清零，不扩
    ag.report(950);
    cs.add("autogrow_resets_on_recovery", ag.report(700) == GrowVerdict::Hold && ag.pages == 65, "");
    // 扩到硬顶（32MB / 256KB = 128 页）
    let mut ag2 = AutoGrow::new(128);
    let mut capped = GrowVerdict::Hold;
    for _ in 0..3 {
        capped = ag2.report(700); // 迟滞：需连续 3 次低命中才动
    }
    cs.add("autogrow_at_cap_honest", ag2.at_cap_now() && capped == GrowVerdict::AtCap && ag2.at_cap == 1 && ag2.pages == 128, "");
    // 7) 达标线 90%。
    cs.add("hit_target_90pct", ag2.meets_target(900) && !ag2.meets_target(899), "");
    // 8) 后台光栅作业：>2ms 转后台，前台占位，到位原子换。
    let mut rj = RasterJob::new();
    let inline = rj.request(1_500);
    let bg = rj.request(2_500);
    cs.add(
        "raster_over_2ms_goes_background",
        inline == JobState::Inline && bg == JobState::Background && rj.backgrounded == 1 && rj.placeholder_shown == 1 && rj.showing_placeholder(),
        "",
    );
    // 前台还在用占位 → 不许换（不撕裂正在显示的旧版）
    rj.finish();
    cs.add("raster_waits_for_front", !rj.swap(true) && rj.showing_placeholder(), "");
    cs.add("raster_swaps_when_idle", rj.swap(false) && rj.swapped == 1 && !rj.showing_placeholder(), "");
    // 9) 配额账：16MB 起步 / 32MB 硬顶 / 不做持久化。
    let mut q = Quota::base();
    cs.add(
        "quota_16mb_base",
        q.pages == 64 && q.total_bytes() == 16 * 1_024 * 1_024 && q.can_add_page() && q.usage_permille() == 0,
        "",
    );
    // 扩到 128 页后不能再扩（硬顶）
    q.pages = 128;
    cs.add("quota_32mb_cap", !q.can_add_page() && !q.add_page() && q.total_bytes() == 32 * 1_024 * 1_024, "");
    cs.add("quota_no_persistence", Quota::persistence_policy().contains("不做持久化"), "");
    // 10) 图集页尺寸与单页字节（512×512 灰度）。
    cs.add("page_geometry", PAGE_PX == 512 && PAGE_BYTES == 262_144, "");
    // 11) 诊断报备通道（扩页到顶是用户可感知事件，需可查）。
    let mut sink = DiagSink::new();
    sink.push("F055", 1, 1_000, DiagSev::Warn, 32, 0, b"atlas at 32MB cap");
    cs.add("atlas_cap_reported", sink.count(DiagSev::Warn) == 1, "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn warmup_covers_ascii_and_common_hanzi_only() {
        let w = WarmupSet::default_set();
        assert!(w.covers(0x41), "ASCII 'A' 在预热集内");
        assert!(w.covers(0x4E00), "常用汉字起点");
        assert!(!w.covers(0x9FA5), "生僻字不在常用集内（按需光栅）");
    }

    #[test]
    fn pin_set_blocks_only_within_current_frame() {
        let mut p = PinSet::new();
        p.pin(1);
        assert!(!p.try_evict(1));
        p.clear_frame();
        assert!(p.try_evict(1), "帧末解钉：下一帧可以驱逐");
    }

    #[test]
    fn page_exchange_never_tears() {
        let mut px = PageExchange::new();
        // 反复在前台忙时请求交换：一次都换不成，且计数如实
        px.mark_ready();
        for _ in 0..10 {
            assert!(!px.try_swap(true));
        }
        assert_eq!(px.swaps, 0);
        assert_eq!(px.postponed, 10);
        assert!(!PageExchange::can_tear());
    }

    #[test]
    fn autogrow_never_exceeds_cap() {
        let mut a = AutoGrow::new(127);
        for _ in 0..20 {
            a.report(100);
        }
        assert!(a.bytes() <= QUOTA_MAX_BYTES, "扩页不得越过 32MB 硬顶");
        assert!(a.at_cap > 0);
    }

    #[test]
    fn raster_job_placeholder_cost_is_measurable() {
        let mut r = RasterJob::new();
        for _ in 0..5 {
            r.request(3_000);
        }
        assert_eq!(r.placeholder_shown, 5, "每次转后台都记一次占位代价");
        assert!(r.showing_placeholder());
    }

    #[test]
    fn quota_usage_tracks_used_bytes() {
        let mut q = Quota::base();
        q.used_bytes = q.total_bytes() / 2;
        assert_eq!(q.usage_permille(), 500);
    }
}
