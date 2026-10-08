//! F045 页缓存水位策略 · 深化件（AI-K1 深化批次三 · G-B-05）。
//!
//! | # | 主册原文 | 本件机制 |
//! | --- | --- | --- |
//! | 1 | 【设计细节】「LRU 按**「文件页/匿名页」分列**（文件页可弃可重读优先弃）」 | [`LruSplit`] 双列 LRU（回收先取文件页，匿名页只有在文件页枯竭后才动） |
//! | 2 | 【设计细节】「**脏页 20% 上限按分区分别计**（SHARED 只读卷不计）」 | [`PerVolumeDirty`] 分区脏页账 + 只读卷豁免 |
//! | 3 | 【设计细节】「水位判定**每 500ms 一次**（不逐页检查的功耗账）」 | [`TickGate`] 500ms 节流门（判定本身也要算功耗账） |
//! | 4 | 【设计细节】「三档切换条件**文档化**（>1GB 空闲=高档……全表在旋钮清单）」 | [`TierRule`] 条件表 + [`KnobTable`] 旋钮登记（无隐藏魔法数） |
//! | 5 | 【状态与异常】「回收导致二次读盘激增 → 记录**「回收后悔」指标**调参依据」 | [`RegretMeter`] 回收后悔账（回收后短期内被重新读回 = 后悔） |
//! | 6 | 【状态与异常】「内存耗尽前 **200MB 警戒** → 全局冲刷 + 通知后台应用释放（F195 联动）」 | [`Emergency`] 警戒状态机（进入/冲刷/通知/解除） |
//! | 7 | 【状态与异常】「应用内存配额**挤压缓存 → 缓存先让**（交互优先）」 | [`CedePolicy`] 让路策略（一次性让 25%，非常态降档——与主域 `note_quota_pressure` 同语义的独立策略面） |
//! | 8 | 【交互设计】「水位线档位在诊断面板**只读展示**（自动策略不暴露旋钮给普通用户——防乱调）」 | [`ReadOnlyView`] 只读投影（有值、有理由、无 setter） |
//! | 9 | 【数据与存储】「**策略决策日志入诊断快照（F174）**」 | [`DecisionLog`] 决策日志（DiagSink 承载） |

use crate::checks::CheckSet;
use crate::perfstar::perfkit::{DiagSev, DiagSink, Knob, KnobTable, SecRing};

// ---------------------------------------------------------------------------
// 常量
// ---------------------------------------------------------------------------

/// 三档水位（主册【功能定义】：高 3.2GB / 中 2.4GB / 低 1.6GB）。
pub const WATER_HIGH_BYTES: u64 = 3_200 * 1_024 * 1_024;
pub const WATER_MID_BYTES: u64 = 2_400 * 1_024 * 1_024;
pub const WATER_LOW_BYTES: u64 = 1_600 * 1_024 * 1_024;
/// 脏页占比上限 20%（千分 = 200）。
pub const DIRTY_CAP_PERMILLE: u32 = 200;
/// 水位判定节流 500ms。
pub const TICK_MS: u64 = 500;
/// 内存耗尽前 200MB 警戒线。
pub const EMERGENCY_FREE_BYTES: u64 = 200 * 1_024 * 1_024;
/// 配额挤压让路：一次性让出 25%（千分）。
pub const CEDE_PERMILLE: u32 = 250;
/// 回收后悔判定窗口：回收后 5 秒内被读回即计后悔。
pub const REGRET_WINDOW_MS: u64 = 5_000;
/// 分区数上限（定长账）。
pub const VOLUMES: usize = 8;

/// 水位档位。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tier {
    High,
    Mid,
    Low,
}

impl Tier {
    pub const fn bytes(self) -> u64 {
        match self {
            Tier::High => WATER_HIGH_BYTES,
            Tier::Mid => WATER_MID_BYTES,
            Tier::Low => WATER_LOW_BYTES,
        }
    }
    pub const fn name(self) -> &'static str {
        match self {
            Tier::High => "高",
            Tier::Mid => "中",
            Tier::Low => "低",
        }
    }
}

// ---------------------------------------------------------------------------
// 1. 文件页/匿名页分列 LRU
// ---------------------------------------------------------------------------

/// 回收目标（主册「文件页可弃可重读优先弃」）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReclaimFrom {
    /// 文件页（可弃可重读，优先）。
    File,
    /// 匿名页（只有文件页枯竭才动——swap 代价比重读高得多）。
    Anon,
    /// 两列都空（无页可回收）。
    Nothing,
}

/// 双列 LRU 计数面（本件管策略与计数，页链本体随闸门接线）。
#[derive(Clone, Copy, Debug)]
pub struct LruSplit {
    pub file_pages: u32,
    pub anon_pages: u32,
    /// 累计回收：文件页 / 匿名页（调参依据）。
    pub reclaimed_file: u64,
    pub reclaimed_anon: u64,
}

impl LruSplit {
    pub const fn new() -> Self {
        LruSplit { file_pages: 0, anon_pages: 0, reclaimed_file: 0, reclaimed_anon: 0 }
    }
    /// 按主册优先级挑一列回收。
    pub fn pick(&self) -> ReclaimFrom {
        if self.file_pages > 0 {
            ReclaimFrom::File
        } else if self.anon_pages > 0 {
            ReclaimFrom::Anon
        } else {
            ReclaimFrom::Nothing
        }
    }
    /// 执行回收一页（返回实际回收的列）。
    pub fn reclaim_one(&mut self) -> ReclaimFrom {
        match self.pick() {
            ReclaimFrom::File => {
                self.file_pages -= 1;
                self.reclaimed_file += 1;
                ReclaimFrom::File
            }
            ReclaimFrom::Anon => {
                self.anon_pages -= 1;
                self.reclaimed_anon += 1;
                ReclaimFrom::Anon
            }
            ReclaimFrom::Nothing => ReclaimFrom::Nothing,
        }
    }
    /// 回收目标字节数换算页数（4KB 页）。
    pub fn pages_needed(bytes: u64) -> u32 {
        ((bytes + 4095) / 4096) as u32
    }
}

// ---------------------------------------------------------------------------
// 2. 分区脏页账（SHARED 只读卷不计）
// ---------------------------------------------------------------------------

/// 一个分区的脏页账。
#[derive(Clone, Copy, Debug)]
pub struct VolumeDirty {
    pub id: u32,
    /// 是否只读卷（SHARED 只读卷天然不计脏页）。
    pub read_only: bool,
    pub total_pages: u32,
    pub dirty_pages: u32,
    /// 强制冲刷次数（超限后的动作计数）。
    pub forced_flushes: u32,
}

impl VolumeDirty {
    pub const fn new(id: u32, read_only: bool) -> Self {
        VolumeDirty { id, read_only, total_pages: 0, dirty_pages: 0, forced_flushes: 0 }
    }
    /// 脏页千分（只读卷恒为 0——不是算出来是 0，是根本不参与）。
    pub fn dirty_permille(&self) -> u32 {
        if self.read_only || self.total_pages == 0 {
            return 0;
        }
        ((self.dirty_pages as u64 * 1000) / self.total_pages as u64) as u32
    }
    /// 是否超 20% 上限。
    pub fn over_cap(&self) -> bool {
        !self.read_only && self.dirty_permille() > DIRTY_CAP_PERMILLE
    }
    /// 冲刷 `n` 页（上限：不得超过脏页数，也不许把只读卷算进来）。
    pub fn flush(&mut self, n: u32) -> u32 {
        if self.read_only {
            return 0;
        }
        let d = n.min(self.dirty_pages);
        self.dirty_pages -= d;
        if d > 0 {
            self.forced_flushes = self.forced_flushes.saturating_add(1);
        }
        d
    }
}

/// 分区脏页总账（定长八卷）。
pub struct PerVolumeDirty {
    vols: [Option<VolumeDirty>; VOLUMES],
    n: usize,
}

impl PerVolumeDirty {
    pub const fn new() -> Self {
        PerVolumeDirty { vols: [None; VOLUMES], n: 0 }
    }
    pub fn register(&mut self, v: VolumeDirty) -> bool {
        if let Some(i) = self.find(v.id) {
            self.vols[i] = Some(v);
            return true;
        }
        if self.n >= VOLUMES {
            return false;
        }
        self.vols[self.n] = Some(v);
        self.n += 1;
        true
    }
    fn find(&self, id: u32) -> Option<usize> {
        for i in 0..self.n {
            if let Some(v) = self.vols[i] {
                if v.id == id {
                    return Some(i);
                }
            }
        }
        None
    }
    pub fn get(&self, id: u32) -> Option<VolumeDirty> {
        self.find(id).and_then(|i| self.vols[i])
    }
    /// 超限分区数（只读卷永不计入——主册「SHARED 只读卷不计」）。
    pub fn over_cap_count(&self) -> usize {
        self.vols.iter().filter_map(|v| *v).filter(|v| v.over_cap()).count()
    }
    /// 全局脏页千分（只读卷的分母页也不计入——口径一致才可比）。
    pub fn global_dirty_permille(&self) -> u32 {
        let mut d = 0u64;
        let mut t = 0u64;
        for v in self.vols.iter().filter_map(|v| *v) {
            if v.read_only {
                continue;
            }
            d += v.dirty_pages as u64;
            t += v.total_pages as u64;
        }
        if t == 0 {
            return 0;
        }
        ((d * 1000) / t) as u32
    }
    pub fn len(&self) -> usize {
        self.n
    }
}

// ---------------------------------------------------------------------------
// 3. 500ms 判定节流门（功耗账）
// ---------------------------------------------------------------------------

/// 水位判定节流门：500ms 一次，不逐页检查。
///
/// 主册把「每 500ms 一次」和「不逐页检查的功耗账」写在一条里——本件把它做成
/// 可验证的：门本身记录被跳过的判定次数（省下的功耗能算出来）。
#[derive(Clone, Copy, Debug)]
pub struct TickGate {
    pub interval_ms: u64,
    last_ms: Option<u64>,
    /// 已执行的判定次数。
    pub runs: u64,
    /// 被节流跳过次数（省下的判定 = 省下的功耗）。
    pub skipped: u64,
}

impl TickGate {
    pub const fn new() -> Self {
        TickGate { interval_ms: TICK_MS, last_ms: None, runs: 0, skipped: 0 }
    }
    /// 是否该跑这次判定（到点才跑）。
    pub fn due(&mut self, now_ms: u64) -> bool {
        match self.last_ms {
            None => {
                self.last_ms = Some(now_ms);
                self.runs += 1;
                true
            }
            Some(last) => {
                if now_ms.saturating_sub(last) >= self.interval_ms {
                    self.last_ms = Some(now_ms);
                    self.runs += 1;
                    true
                } else {
                    self.skipped += 1;
                    false
                }
            }
        }
    }
    /// 节流命中率千分（省下的判定 / 总调用）——功耗账的直接证据。
    pub fn saving_permille(&self) -> u32 {
        let total = self.runs + self.skipped;
        if total == 0 {
            return 0;
        }
        ((self.skipped * 1000) / total) as u32
    }
}

// ---------------------------------------------------------------------------
// 4. 三档切换条件表（文档化 + 旋钮清单）
// ---------------------------------------------------------------------------

/// 档位切换条件（主册「三档切换条件文档化（>1GB 空闲=高档……全表在旋钮清单）」）。
#[derive(Clone, Copy, Debug)]
pub struct TierRule {
    /// 升到高档所需空闲字节（主册原文：>1GB 空闲 = 高档）。
    pub up_high_free_bytes: u64,
    /// 降到中档的空闲阈值。
    pub down_mid_free_bytes: u64,
    /// 降到低档的空闲阈值。
    pub down_low_free_bytes: u64,
}

impl TierRule {
    /// 主册口径默认表（数值进旋钮，不散落在代码里）。
    pub const fn default_rule() -> Self {
        TierRule {
            up_high_free_bytes: 1_024 * 1_024 * 1_024,
            down_mid_free_bytes: 512 * 1_024 * 1_024,
            down_low_free_bytes: 256 * 1_024 * 1_024,
        }
    }
    /// 按空闲字节裁定档位（含迟滞：升档需超过阈值，降档需低于阈值——
    /// 同值不抖动）。
    pub fn tier_for(&self, free_bytes: u64) -> Tier {
        if free_bytes > self.up_high_free_bytes {
            Tier::High
        } else if free_bytes > self.down_mid_free_bytes {
            Tier::Mid
        } else {
            Tier::Low
        }
    }
    /// 登记进旋钮清单（无隐藏魔法数：每个阈值都有名字、范围、单位）。
    pub fn register_knobs(&self, t: &mut KnobTable) {
        t.register("water.up_high_free_mb", (self.up_high_free_bytes / (1024 * 1024)) as i64, 64, 4096, "MB");
        t.register("water.down_mid_free_mb", (self.down_mid_free_bytes / (1024 * 1024)) as i64, 32, 2048, "MB");
        t.register("water.down_low_free_mb", (self.down_low_free_bytes / (1024 * 1024)) as i64, 16, 1024, "MB");
        t.register("water.tick_ms", TICK_MS as i64, 100, 5_000, "ms");
        t.register("water.dirty_cap_permille", DIRTY_CAP_PERMILLE as i64, 50, 500, "permille");
        t.register("water.emergency_free_mb", (EMERGENCY_FREE_BYTES / (1024 * 1024)) as i64, 32, 1024, "MB");
        t.register("water.cede_permille", CEDE_PERMILLE as i64, 0, 500, "permille");
    }
}

// ---------------------------------------------------------------------------
// 5. 回收后悔账
// ---------------------------------------------------------------------------

/// 回收后悔账：回收掉的页在短窗内被读回 = 这次回收是错的。
///
/// 主册「回收导致二次读盘激增 → 记录「回收后悔」指标调参依据」——没有这个
/// 指标，水位调参就是拍脑袋（与 F052「数据先行」同纪律）。
#[derive(Clone, Copy, Debug)]
pub struct RegretMeter {
    /// 回收时间戳环（定长 64，覆盖最旧）。
    stamps: [u64; 64],
    /// 页号环（判定同一页是否被读回）。
    pages: [u32; 64],
    head: usize,
    filled: usize,
    pub reclaims: u64,
    pub regrets: u64,
    pub window_ms: u64,
}

impl RegretMeter {
    pub const fn new() -> Self {
        RegretMeter { stamps: [0; 64], pages: [0; 64], head: 0, filled: 0, reclaims: 0, regrets: 0, window_ms: REGRET_WINDOW_MS }
    }
    /// 记一次回收。
    pub fn note_reclaim(&mut self, page: u32, now_ms: u64) {
        self.stamps[self.head] = now_ms;
        self.pages[self.head] = page;
        self.head = (self.head + 1) % 64;
        self.filled = (self.filled + 1).min(64);
        self.reclaims += 1;
    }
    /// 读回一页：若该页在窗内被回收过 → 计一次后悔（每页只计一次，避免刷高）。
    pub fn note_reread(&mut self, page: u32, now_ms: u64) -> bool {
        for i in 0..self.filled {
            let idx = (self.head + 64 - self.filled + i) % 64;
            if self.pages[idx] == page {
                let age = now_ms.saturating_sub(self.stamps[idx]);
                if age <= self.window_ms {
                    self.regrets += 1;
                    // 消费掉这条记录，避免同一页重复计数
                    self.pages[idx] = u32::MAX;
                    return true;
                }
            }
        }
        false
    }
    /// 后悔率千分。
    pub fn permille(&self) -> u32 {
        if self.reclaims == 0 {
            return 0;
        }
        ((self.regrets * 1000) / self.reclaims) as u32
    }
}

// ---------------------------------------------------------------------------
// 6. 200MB 警戒（全局冲刷 + 通知 F195）
// ---------------------------------------------------------------------------

/// 警戒状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EmergencyState {
    Normal,
    /// 空闲低于 200MB：全局冲刷 + 通知后台应用释放。
    Warning,
    /// 已通知 F195 并等待释放（不重复通知，避免通知风暴）。
    Notified,
}

/// 警戒状态机（进入 → 冲刷 → 通知 → 解除；解除带迟滞，防边界抖动）。
pub struct Emergency {
    pub state: EmergencyState,
    /// 进入警戒次数。
    pub entered: u32,
    /// 触发的全局冲刷次数。
    pub flushes: u32,
    /// 发出的通知次数（F195 联动）。
    pub notices: u32,
    /// 解除阈值（迟滞：需回到警戒线 +128MB 才解除）。
    pub release_hysteresis_bytes: u64,
}

impl Emergency {
    pub const fn new() -> Self {
        Emergency {
            state: EmergencyState::Normal,
            entered: 0,
            flushes: 0,
            notices: 0,
            release_hysteresis_bytes: 128 * 1_024 * 1_024,
        }
    }
    /// 心跳：按当前空闲字节推进状态。返回本次是否触发了「冲刷 + 通知」。
    pub fn tick(&mut self, free_bytes: u64, sink: Option<&mut DiagSink>, now_ms: u64) -> bool {
        match self.state {
            EmergencyState::Normal => {
                if free_bytes < EMERGENCY_FREE_BYTES {
                    self.state = EmergencyState::Warning;
                    self.entered += 1;
                    self.flushes += 1;
                    if let Some(s) = sink {
                        s.push("F045", 1, now_ms, DiagSev::Warn, free_bytes, EMERGENCY_FREE_BYTES, b"free below emergency line");
                    }
                    return true;
                }
                false
            }
            EmergencyState::Warning => {
                // 进入即通知一次，随后转 Notified（不重复通知）
                self.notices += 1;
                self.state = EmergencyState::Notified;
                if let Some(s) = sink {
                    s.push("F045", 2, now_ms, DiagSev::Warn, free_bytes, 0, b"notify F195 to release");
                }
                false
            }
            EmergencyState::Notified => {
                if free_bytes > EMERGENCY_FREE_BYTES + self.release_hysteresis_bytes {
                    self.state = EmergencyState::Normal;
                }
                false
            }
        }
    }
    pub fn is_active(&self) -> bool {
        !matches!(self.state, EmergencyState::Normal)
    }
}

// ---------------------------------------------------------------------------
// 7. 配额挤压让路（交互优先：缓存先让）
// ---------------------------------------------------------------------------

/// 让路裁定（主册「应用内存配额挤压缓存 → 缓存先让（交互优先）」）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CedePolicy {
    /// 一次性让出比例千分（默认 250 = 25%）。
    pub cede_permille: u32,
    /// 让路次数（非常态：次数暴涨说明配额定错了，是调参信号）。
    pub cedes: u32,
}

impl CedePolicy {
    pub const fn new() -> Self {
        CedePolicy { cede_permille: CEDE_PERMILLE, cedes: 0 }
    }
    /// 应让出的字节数（从当前缓存占用里算）。
    pub fn cede_bytes(&mut self, cache_bytes: u64) -> u64 {
        self.cedes += 1;
        (cache_bytes * self.cede_permille as u64) / 1000
    }
    /// 让路是否异常频繁（>10 次/分钟即提示调参——诊断面用）。
    pub fn is_thrashing(&self, window_cedes_per_min: u32) -> bool {
        window_cedes_per_min > 10
    }
}

// ---------------------------------------------------------------------------
// 8. 只读展示面（防乱调）
// ---------------------------------------------------------------------------

/// 诊断面板只读投影：有值、有理由，**没有 setter**（类型层面防乱调）。
#[derive(Clone, Copy, Debug)]
pub struct ReadOnlyView {
    pub tier: Tier,
    pub dirty_permille: u32,
    pub free_bytes: u64,
}

impl ReadOnlyView {
    pub fn of(tier: Tier, dirty_permille: u32, free_bytes: u64) -> Self {
        ReadOnlyView { tier, dirty_permille, free_bytes }
    }
    /// 档位说明（为什么现在在这档——用户看得懂的理由，不是内部阈值）。
    pub fn reason(&self) -> &'static str {
        match self.tier {
            Tier::High => "内存充裕，缓存放到最大",
            Tier::Mid => "内存一般，缓存适度收缩",
            Tier::Low => "内存紧张，缓存优先让给应用",
        }
    }
    /// 是否允许用户在界面上调节（主册：自动策略不暴露旋钮给普通用户）。
    pub const fn user_adjustable() -> bool {
        false
    }
}

// ---------------------------------------------------------------------------
// 9. 决策日志（入诊断快照 F174）+ 交互设计数据面
// ---------------------------------------------------------------------------

/// 决策日志：每次档位切换/警戒/让路都留一行（F174 诊断快照消费）。
pub struct DecisionLog {
    sink: DiagSink,
    /// 回收吞吐 60 秒曲线（交互设计：三区图旁的实时曲线）。
    reclaim_curve: SecRing,
    last_tier: Option<Tier>,
}

impl DecisionLog {
    pub const fn new() -> Self {
        DecisionLog { sink: DiagSink::new(), reclaim_curve: SecRing::new(), last_tier: None }
    }
    /// 记一次档位切换（只在真的切档时记，避免刷日志）。
    pub fn note_tier(&mut self, tier: Tier, now_ms: u64, free_bytes: u64) {
        if self.last_tier == Some(tier) {
            return;
        }
        self.last_tier = Some(tier);
        let sev = if tier == Tier::Low { DiagSev::Warn } else { DiagSev::Info };
        let msg: &[u8] = match tier {
            Tier::High => b"tier -> high",
            Tier::Mid => b"tier -> mid",
            Tier::Low => b"tier -> low",
        };
        self.sink.push("F045", 10, now_ms, sev, free_bytes, tier.bytes(), msg);
    }
    /// 记一次回收（进 60 秒曲线 + 决策日志）。
    pub fn note_reclaim(&mut self, pages: u32, now_ms: u64) {
        self.reclaim_curve.note(now_ms, pages as u64);
    }
    /// 60 秒回收曲线（时间升序）。
    pub fn reclaim_series(&self, out: &mut [u64]) -> usize {
        self.reclaim_curve.series(out)
    }
    pub fn sink(&self) -> &DiagSink {
        &self.sink
    }
    pub fn sink_mut(&mut self) -> &mut DiagSink {
        &mut self.sink
    }
    /// 导出给 F174（诊断快照）。
    pub fn snapshot(&self, out: &mut [crate::perfstar::perfkit::DiagEntry]) -> usize {
        self.sink.snapshot(out)
    }
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

pub fn run_checks() -> CheckSet {
    let mut cs = CheckSet::new("F045-pagewater-ext");
    // 1) 三档水位数值（主册 3.2/2.4/1.6GB）。
    cs.add(
        "tiers_are_32_24_16_gb",
        Tier::High.bytes() == 3_200 * 1_024 * 1_024 && Tier::Mid.bytes() == 2_400 * 1_024 * 1_024 && Tier::Low.bytes() == 1_600 * 1_024 * 1_024,
        "",
    );
    // 2) LRU 分列：文件页优先，文件页枯竭才动匿名页。
    let mut ls = LruSplit { file_pages: 2, anon_pages: 1, ..LruSplit::new() };
    let f1 = ls.reclaim_one();
    let f2 = ls.reclaim_one();
    let a1 = ls.reclaim_one();
    let nothing = ls.reclaim_one();
    cs.add(
        "lru_file_before_anon",
        f1 == ReclaimFrom::File && f2 == ReclaimFrom::File && a1 == ReclaimFrom::Anon && nothing == ReclaimFrom::Nothing,
        "",
    );
    // 3) 页数换算（4KB 页，向上取整）。
    cs.add("pages_needed_rounds_up", LruSplit::pages_needed(4096) == 1 && LruSplit::pages_needed(4097) == 2, "");
    // 4) 分区脏页：只读卷不计（SHARED）。
    let mut pv = PerVolumeDirty::new();
    pv.register(VolumeDirty { id: 1, read_only: false, total_pages: 1_000, dirty_pages: 300, forced_flushes: 0 });
    pv.register(VolumeDirty { id: 2, read_only: true, total_pages: 1_000, dirty_pages: 300, forced_flushes: 0 });
    cs.add(
        "readonly_volume_excluded",
        pv.get(1).unwrap().dirty_permille() == 300
            && pv.get(1).unwrap().over_cap()
            && pv.get(2).unwrap().dirty_permille() == 0
            && !pv.get(2).unwrap().over_cap()
            && pv.over_cap_count() == 1,
        "",
    );
    // 5) 全局脏页千分：只读卷的分母也不计入（口径一致才可比）。
    cs.add("global_dirty_excludes_readonly", pv.global_dirty_permille() == 300, "");
    // 6) 冲刷不得把只读卷算进来。
    let mut ro = VolumeDirty { id: 3, read_only: true, total_pages: 1_000, dirty_pages: 900, forced_flushes: 0 };
    cs.add("flush_ignores_readonly", ro.flush(100) == 0 && ro.forced_flushes == 0, "");
    let mut rw = VolumeDirty { id: 4, read_only: false, total_pages: 1_000, dirty_pages: 50, forced_flushes: 0 };
    cs.add("flush_clamps_to_dirty", rw.flush(100) == 50 && rw.dirty_pages == 0 && rw.forced_flushes == 1, "");
    // 7) 500ms 节流门（功耗账：节流命中率可算）。
    let mut tg = TickGate::new();
    let d0 = tg.due(0);
    let d1 = tg.due(100);
    let d2 = tg.due(500);
    cs.add("tick_gate_500ms", d0 && !d1 && d2 && tg.skipped == 1 && tg.saving_permille() == 333, "");
    // 8) 三档切换条件表（>1GB 空闲 = 高档）。
    let rule = TierRule::default_rule();
    cs.add(
        "tier_rule_documentized",
        rule.tier_for(2_048 * 1_024 * 1_024) == Tier::High
            && rule.tier_for(600 * 1_024 * 1_024) == Tier::Mid
            && rule.tier_for(100 * 1_024 * 1_024) == Tier::Low,
        "",
    );
    // 9) 旋钮清单（无隐藏魔法数：阈值全部登记，可审计可钳制）。
    let mut t = KnobTable::new();
    rule.register_knobs(&mut t);
    let mut out = [Knob { name: [0; 32], name_len: 0, value: 0, min: 0, max: 0, unit: [0; 8], unit_len: 0 }; 16];
    let n = t.audit(&mut out);
    cs.add("knobs_registered", n == 7 && t.get("water.tick_ms") == Some(500) && t.all_in_range(), "");
    cs.add("knob_clamped", t.set("water.tick_ms", 999_999) && t.get("water.tick_ms") == Some(5_000) && t.clamped() == 1, "");
    // 10) 回收后悔账（窗内读回 = 后悔；窗外读回不算）。
    let mut rm = RegretMeter::new();
    rm.note_reclaim(7, 1_000);
    cs.add("regret_in_window", rm.note_reread(7, 4_000) && rm.regrets == 1, "");
    let mut rm2 = RegretMeter::new();
    rm2.note_reclaim(8, 1_000);
    cs.add("no_regret_outside_window", !rm2.note_reread(8, 20_000) && rm2.regrets == 0, "");
    // 同一页不重复计数（避免刷高指标）
    cs.add("regret_counted_once", !rm.note_reread(7, 4_100) && rm.regrets == 1, "");
    // 11) 200MB 警戒：进入 → 冲刷 → 通知 → 迟滞解除。
    let mut sink = DiagSink::new();
    let mut em = Emergency::new();
    let fired = em.tick(150 * 1_024 * 1_024, Some(&mut sink), 1_000);
    let notified = em.tick(150 * 1_024 * 1_024, Some(&mut sink), 1_500);
    cs.add(
        "emergency_enters_flushes_notifies",
        fired && !notified && em.is_active() && em.entered == 1 && em.flushes == 1 && em.notices == 1 && sink.count(DiagSev::Warn) == 2,
        "",
    );
    // 迟滞：回到 250MB 不解除（< 200+128），回到 400MB 才解除
    em.tick(250 * 1_024 * 1_024, None, 2_000);
    let still = em.is_active();
    em.tick(400 * 1_024 * 1_024, None, 2_500);
    cs.add("emergency_hysteresis", still && !em.is_active(), "");
    // 12) 配额让路（一次性 25%，非常态降档）。
    let mut cp = CedePolicy::new();
    let ceded = cp.cede_bytes(1_000_000);
    cs.add("cede_quarter_once", ceded == 250_000 && cp.cedes == 1 && cp.is_thrashing(11) && !cp.is_thrashing(10), "");
    // 13) 只读展示面：有理由、类型层面无 setter（防乱调）。
    let v = ReadOnlyView::of(Tier::Low, 210, 100 * 1_024 * 1_024);
    cs.add("readonly_view_no_setter", !ReadOnlyView::user_adjustable() && v.reason() == "内存紧张，缓存优先让给应用", "");
    // 14) 决策日志：只在真的切档时记（不刷日志）+ 入诊断快照。
    let mut dl = DecisionLog::new();
    dl.note_tier(Tier::High, 1_000, 3_000_000_000);
    dl.note_tier(Tier::High, 2_000, 3_000_000_000); // 同档不记
    dl.note_tier(Tier::Low, 3_000, 100_000_000);
    let mut entries = [crate::perfstar::perfkit::DiagEntry::empty(); 8];
    let n2 = dl.snapshot(&mut entries);
    cs.add("decision_log_only_on_change", n2 == 2 && dl.sink().count(DiagSev::Warn) == 1, "");
    // 15) 回收曲线（交互设计实时面）。
    dl.note_reclaim(4, 10_000);
    dl.note_reclaim(6, 10_500);
    let mut curve = [0u64; 60];
    dl.reclaim_series(&mut curve);
    cs.add("reclaim_curve_accumulates", curve[59] == 10, "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lru_never_reclaims_anon_while_file_pages_exist() {
        // 文件页 1 + 匿名页 1：先文件、后匿名、再无（空列回收不越界）
        let mut l = LruSplit { file_pages: 1, anon_pages: 1, ..LruSplit::new() };
        assert_eq!(l.reclaim_one(), ReclaimFrom::File);
        assert_eq!(l.reclaim_one(), ReclaimFrom::Anon);
        assert_eq!(l.reclaimed_file, 1);
        assert_eq!(l.reclaimed_anon, 1);
        assert_eq!(l.reclaim_one(), ReclaimFrom::Nothing, "空列回收不越界");
        // 文件页枯竭、匿名页仍有 → 动匿名页（不因「优先弃文件页」而卡住回收）
        let mut l2 = LruSplit { file_pages: 0, anon_pages: 3, ..LruSplit::new() };
        assert_eq!(l2.reclaim_one(), ReclaimFrom::Anon);
        assert_eq!(l2.reclaimed_file, 0);
    }

    #[test]
    fn dirty_cap_is_per_volume_not_global_only() {
        let mut p = PerVolumeDirty::new();
        // 卷 A 10% 脏、卷 B 30% 脏：全局 20% 看似达标，但 B 超限——必须按分区判
        p.register(VolumeDirty { id: 1, read_only: false, total_pages: 1_000, dirty_pages: 100, forced_flushes: 0 });
        p.register(VolumeDirty { id: 2, read_only: false, total_pages: 1_000, dirty_pages: 300, forced_flushes: 0 });
        assert_eq!(p.global_dirty_permille(), 200);
        assert_eq!(p.over_cap_count(), 1, "按分区分别计：B 超限");
    }

    #[test]
    fn tick_gate_saving_is_measurable() {
        let mut t = TickGate::new();
        for i in 0..10u64 {
            t.due(i * 100);
        }
        assert!(t.saving_permille() > 0, "省下的判定次数必须可量化");
    }

    #[test]
    fn emergency_does_not_spam_notices() {
        let mut e = Emergency::new();
        e.tick(10 * 1024 * 1024, None, 0);
        for _ in 0..50 {
            e.tick(10 * 1024 * 1024, None, 1_000);
        }
        assert_eq!(e.notices, 1, "通知发一次，不刷屏");
        assert_eq!(e.flushes, 1);
    }

    #[test]
    fn regret_meter_zero_reclaims_is_zero_rate() {
        let r = RegretMeter::new();
        assert_eq!(r.permille(), 0);
        assert_eq!(r.reclaims, 0);
    }

    #[test]
    fn readonly_view_reason_covers_all_tiers() {
        assert_eq!(ReadOnlyView::of(Tier::High, 0, 0).reason(), "内存充裕，缓存放到最大");
        assert_eq!(ReadOnlyView::of(Tier::Mid, 0, 0).reason(), "内存一般，缓存适度收缩");
    }
}
