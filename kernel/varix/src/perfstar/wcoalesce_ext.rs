//! F046 写合并窗口自适应 · 深化件（AI-K1 深化批次三 · G-B-06）。
//!
//! | # | 主册原文 | 本件机制 |
//! | --- | --- | --- |
//! | 1 | 【设计细节】「三档阈值：请求 **<10/s 且脏页 <5% = 空闲**；**>200/s 或脏页 >15% = 重载**；迟滞驻留防抖；**档位切换写一行审计**」 | [`TierRule`] 条件表（数值进旋钮）+ [`TierAudit`] 切档审计行 |
//! | 2 | 【设计细节】「**窗口上限 8s 的依据=断电丢失窗口承诺**（用户可理解的边界）」 | [`LossWindow`] 断电丢失窗口估算（窗口 × 写入速率 = 最坏丢失字节） |
//! | 3 | 【验收判据】「**断电百次按三档各跑一遍全绿**」 | [`PowerCutDrill`] 分档百次演练账（每档独立计数，缺失一档就不算跑过） |
//! | 4 | 【验收判据】「**fsync 延迟 P99 <10ms 不受档位影响**」 | [`FsyncP99`] 分档独立 P99 账（三档各自达标才达标） |
//! | 5 | 【交互诊断】「**档位切换事件入诊断快照**」 | [`TierAudit::push_to`] 经 [`DiagSink`] 入 F174 |
//! | 6 | 【数据与存储】「档位判定依据：**每秒写请求数 + 脏页占比（F045 数据源共享）**」 | [`Feed`] F045 数据接收面（一处一事实：不由本域自算脏页） |
//! | 7 | 【状态与异常】「**B-702/B-703 判据在自适应模式下重测重录**」 | [`DrillLedger`] 重测记录（档位 × 轮次 × 结果） |

use crate::checks::CheckSet;
use crate::perfstar::perfkit::{DiagSev, DiagSink, KnobTable};

// ---------------------------------------------------------------------------
// 常量
// ---------------------------------------------------------------------------

/// 空闲档窗口 1s（主册【功能定义】）。
pub const WIN_IDLE_MS: u32 = 1_000;
/// 正常档窗口 5s。
pub const WIN_NORMAL_MS: u32 = 5_000;
/// 重载档窗口 8s（主册原文；依据=断电丢失窗口承诺）。
pub const WIN_HEAVY_MS: u32 = 8_000;
/// 空闲档判据：写请求 <10/s。
pub const IDLE_REQ_PER_SEC: u32 = 10;
/// 空闲档判据：脏页 <5%（千分 50）。
pub const IDLE_DIRTY_PERMILLE: u32 = 50;
/// 重载档判据：写请求 >200/s。
pub const HEAVY_REQ_PER_SEC: u32 = 200;
/// 重载档判据：脏页 >15%（千分 150）。
pub const HEAVY_DIRTY_PERMILLE: u32 = 150;
/// 迟滞驻留 5s（主册【状态与异常】「切档后至少驻留 5s」）。
pub const DWELL_MS: u32 = 5_000;
/// fsync P99 红线 10ms（主册【验收判据】）。
pub const FSYNC_P99_REDLINE_US: u32 = 10_000;
/// 断电演练目标轮次 100（主册「断电百次」）。
pub const DRILL_ROUNDS: u32 = 100;
/// 切档审计环容量。
pub const AUDIT_RING: usize = 32;

/// 三档（与主域档位枚举同义，本件独立定义避免跨件耦合）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tier {
    Idle,
    Normal,
    Heavy,
}

impl Tier {
    pub const fn window_ms(self) -> u32 {
        match self {
            Tier::Idle => WIN_IDLE_MS,
            Tier::Normal => WIN_NORMAL_MS,
            Tier::Heavy => WIN_HEAVY_MS,
        }
    }
    pub const fn name(self) -> &'static str {
        match self {
            Tier::Idle => "空闲",
            Tier::Normal => "正常",
            Tier::Heavy => "重载",
        }
    }
}

// ---------------------------------------------------------------------------
// 1. 三档条件表（数值进旋钮）
// ---------------------------------------------------------------------------

/// 三档切换条件（主册【设计细节】全表）。
#[derive(Clone, Copy, Debug)]
pub struct TierRule {
    pub idle_req_per_sec: u32,
    pub idle_dirty_permille: u32,
    pub heavy_req_per_sec: u32,
    pub heavy_dirty_permille: u32,
    pub dwell_ms: u32,
}

impl TierRule {
    pub const fn default_rule() -> Self {
        TierRule {
            idle_req_per_sec: IDLE_REQ_PER_SEC,
            idle_dirty_permille: IDLE_DIRTY_PERMILLE,
            heavy_req_per_sec: HEAVY_REQ_PER_SEC,
            heavy_dirty_permille: HEAVY_DIRTY_PERMILLE,
            dwell_ms: DWELL_MS,
        }
    }
    /// 裁定档位（主册条件逐条：空闲要**两个条件同时**成立；重载**任一**成立即可）。
    pub fn classify(&self, req_per_sec: u32, dirty_permille: u32) -> Tier {
        if req_per_sec > self.heavy_req_per_sec || dirty_permille > self.heavy_dirty_permille {
            return Tier::Heavy;
        }
        if req_per_sec < self.idle_req_per_sec && dirty_permille < self.idle_dirty_permille {
            return Tier::Idle;
        }
        Tier::Normal
    }
    /// 登记进旋钮清单（无隐藏魔法数）。
    pub fn register_knobs(&self, t: &mut KnobTable) {
        t.register("wc.win_idle_ms", WIN_IDLE_MS as i64, 100, 5_000, "ms");
        t.register("wc.win_normal_ms", WIN_NORMAL_MS as i64, 500, 8_000, "ms");
        t.register("wc.win_heavy_ms", WIN_HEAVY_MS as i64, 1_000, 8_000, "ms");
        t.register("wc.idle_req_per_sec", self.idle_req_per_sec as i64, 1, 100, "req/s");
        t.register("wc.idle_dirty_permille", self.idle_dirty_permille as i64, 1, 500, "permille");
        t.register("wc.heavy_req_per_sec", self.heavy_req_per_sec as i64, 20, 5_000, "req/s");
        t.register("wc.heavy_dirty_permille", self.heavy_dirty_permille as i64, 50, 900, "permille");
        t.register("wc.dwell_ms", self.dwell_ms as i64, 0, 30_000, "ms");
    }
}

// ---------------------------------------------------------------------------
// 2. F045 数据接收面（数据源共享，不自算第二份真相）
// ---------------------------------------------------------------------------

/// F045 共享数据源接收面：写请求速率 + 脏页占比。
///
/// 主册明确「档位判定依据：每秒写请求数 + 脏页占比（**F045 数据源共享**）」——
/// 本域不得自己再统计一份脏页（两处数字对不上 = 两处都不可信）。
#[derive(Clone, Copy, Debug)]
pub struct Feed {
    /// 最近一秒写请求数（由 F045/存储栈喂入）。
    pub req_per_sec: u32,
    /// 脏页占比千分（由 F045 喂入，本域不自算）。
    pub dirty_permille: u32,
    /// 是否收到过 F045 数据（未收到 = 判据不成立，不得拍脑袋定档）。
    pub fed: bool,
}

impl Feed {
    pub const fn new() -> Self {
        Feed { req_per_sec: 0, dirty_permille: 0, fed: false }
    }
    /// F045 侧调用：喂入共享数据。
    pub fn note(&mut self, req_per_sec: u32, dirty_permille: u32) {
        self.req_per_sec = req_per_sec;
        self.dirty_permille = dirty_permille.min(1_000);
        self.fed = true;
    }
    /// 无数据时保守判为正常档（既不激进合并也不放弃合并——可解释的中间态）。
    pub fn classify(&self, rule: &TierRule) -> Tier {
        if !self.fed {
            return Tier::Normal;
        }
        rule.classify(self.req_per_sec, self.dirty_permille)
    }
}

// ---------------------------------------------------------------------------
// 3. 切档审计（写一行审计 + 入诊断快照）
// ---------------------------------------------------------------------------

/// 一条切档审计行（主册「档位切换写一行审计」）。
#[derive(Clone, Copy, Debug)]
pub struct TierAudit {
    pub at_ms: u64,
    pub from: Tier,
    pub to: Tier,
    /// 切档时的写请求速率（证据）。
    pub req_per_sec: u32,
    /// 切档时的脏页千分（证据）。
    pub dirty_permille: u32,
    /// 驻留判定：距上次切档是否已满 5s（不满足的切档不应发生）。
    pub dwell_ok: bool,
}

/// 切档审计环（定长 32，覆盖最旧）+ 诊断快照投递。
pub struct TierAuditRing {
    ring: [Option<TierAudit>; AUDIT_RING],
    head: usize,
    filled: usize,
    /// 未满足驻留却被要求切档的次数（迟滞生效证据）。
    pub suppressed: u32,
    pub total: u32,
}

impl TierAuditRing {
    pub const fn new() -> Self {
        TierAuditRing { ring: [None; AUDIT_RING], head: 0, filled: 0, suppressed: 0, total: 0 }
    }
    /// 记录一次切档请求。`last_switch_ms` 为上次切档时刻（None = 从未切档）。
    pub fn push(
        &mut self,
        a: TierAudit,
        last_switch_ms: Option<u64>,
        dwell_ms: u32,
        sink: Option<&mut DiagSink>,
    ) -> bool {
        let dwell_ok = match last_switch_ms {
            None => true,
            Some(t) => a.at_ms.saturating_sub(t) >= dwell_ms as u64,
        };
        if !dwell_ok {
            self.suppressed += 1;
            return false;
        }
        self.ring[self.head] = Some(TierAudit { dwell_ok, ..a });
        self.head = (self.head + 1) % AUDIT_RING;
        self.filled = (self.filled + 1).min(AUDIT_RING);
        self.total += 1;
        if let Some(s) = sink {
            let sev = if a.to == Tier::Heavy { DiagSev::Info } else { DiagSev::Info };
            let msg: &[u8] = match a.to {
                Tier::Idle => b"tier -> idle",
                Tier::Normal => b"tier -> normal",
                Tier::Heavy => b"tier -> heavy",
            };
            s.push("F046", 1, a.at_ms, sev, a.req_per_sec as u64, a.dirty_permille as u64, msg);
        }
        true
    }
    /// 快照导出（时间升序）。
    pub fn snapshot(&self, out: &mut [TierAudit]) -> usize {
        let n = self.filled.min(out.len());
        let start = (self.head + AUDIT_RING - n) % AUDIT_RING;
        for i in 0..n {
            if let Some(a) = self.ring[(start + i) % AUDIT_RING] {
                out[i] = a;
            }
        }
        n
    }
    pub fn len(&self) -> usize {
        self.filled
    }
}

// ---------------------------------------------------------------------------
// 4. 断电丢失窗口（窗口上限 8s 的依据 = 用户可理解的边界）
// ---------------------------------------------------------------------------

/// 断电丢失窗口估算：最坏情况下丢失的字节数 = 窗口时长 × 写入速率。
///
/// 主册把「8s」的依据写成「断电丢失窗口承诺」——本件把这个承诺变成可算的数，
/// 用户能读懂：「最坏丢这么多，且这个数是我承诺过的」。
pub struct LossWindow;

impl LossWindow {
    /// 最坏丢失字节（窗口 × 每秒写入字节）。
    pub fn worst_case_bytes(window_ms: u32, bytes_per_sec: u64) -> u64 {
        (bytes_per_sec * window_ms as u64) / 1_000
    }
    /// 三档各自的最坏丢失（用户可理解的三档对比表）。
    pub fn by_tier(bytes_per_sec: u64) -> [(Tier, u64); 3] {
        [
            (Tier::Idle, Self::worst_case_bytes(WIN_IDLE_MS, bytes_per_sec)),
            (Tier::Normal, Self::worst_case_bytes(WIN_NORMAL_MS, bytes_per_sec)),
            (Tier::Heavy, Self::worst_case_bytes(WIN_HEAVY_MS, bytes_per_sec)),
        ]
    }
    /// 承诺是否成立：最坏丢失 ≤ 用户可接受的承诺值（默认 8s 档即上限）。
    pub fn within_promise(window_ms: u32, bytes_per_sec: u64, promise_bytes: u64) -> bool {
        Self::worst_case_bytes(window_ms, bytes_per_sec) <= promise_bytes
    }
}

// ---------------------------------------------------------------------------
// 5. 断电演练账（三档各 100 次）
// ---------------------------------------------------------------------------

/// 演练结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DrillResult {
    /// 自动恢复，零 fsck。
    Recovered,
    /// 需要 fsck 才恢复（判据要求：零 fsck）。
    NeededFsck,
    /// 数据损坏（最坏）。
    Corrupted,
}

/// 分档断电演练账（主册「断电百次按三档各跑一遍全绿」）。
#[derive(Clone, Copy, Debug)]
pub struct PowerCutDrill {
    pub rounds: [u32; 3],
    pub recovered: [u32; 3],
    pub needed_fsck: [u32; 3],
    pub corrupted: [u32; 3],
}

impl PowerCutDrill {
    pub const fn new() -> Self {
        PowerCutDrill { rounds: [0; 3], recovered: [0; 3], needed_fsck: [0; 3], corrupted: [0; 3] }
    }
    fn idx(t: Tier) -> usize {
        match t {
            Tier::Idle => 0,
            Tier::Normal => 1,
            Tier::Heavy => 2,
        }
    }
    pub fn record(&mut self, t: Tier, r: DrillResult) {
        let i = PowerCutDrill::idx(t);
        self.rounds[i] += 1;
        match r {
            DrillResult::Recovered => self.recovered[i] += 1,
            DrillResult::NeededFsck => self.needed_fsck[i] += 1,
            DrillResult::Corrupted => self.corrupted[i] += 1,
        }
    }
    /// 某档是否跑满 100 轮且全绿（零 fsck 零损坏）。
    pub fn tier_green(&self, t: Tier) -> bool {
        let i = PowerCutDrill::idx(t);
        self.rounds[i] >= DRILL_ROUNDS && self.needed_fsck[i] == 0 && self.corrupted[i] == 0
    }
    /// 三档全绿（判据达成）。缺一档跑过就不算——不粉饰。
    pub fn all_green(&self) -> bool {
        self.tier_green(Tier::Idle) && self.tier_green(Tier::Normal) && self.tier_green(Tier::Heavy)
    }
    /// 缺口说明（哪一档还没跑满——诚实报告，不让「大部分绿」冒充全绿）。
    pub fn gap(&self) -> Option<(Tier, u32)> {
        for (i, t) in [Tier::Idle, Tier::Normal, Tier::Heavy].iter().enumerate() {
            if self.rounds[i] < DRILL_ROUNDS {
                return Some((*t, DRILL_ROUNDS - self.rounds[i]));
            }
        }
        None
    }
}

/// 重测记录（B-702/B-703 在自适应模式下重测重录）。
#[derive(Clone, Copy, Debug)]
pub struct DrillLedger {
    /// 已完成的重测批次计数。
    pub reruns: u32,
    /// 每批结果是否全绿。
    pub last_green: bool,
}

impl DrillLedger {
    pub const fn new() -> Self {
        DrillLedger { reruns: 0, last_green: false }
    }
    pub fn record_rerun(&mut self, all_green: bool) {
        self.reruns += 1;
        self.last_green = all_green;
    }
}

// ---------------------------------------------------------------------------
// 6. fsync P99 分档账（不受档位影响）
// ---------------------------------------------------------------------------

/// fsync 延迟直方图（对数桶，与 F047 同款口径：50×1.25^b）。
///
/// 32 桶覆盖到 ~78ms（50×1.25^31），足以把 10ms 红线与「明显超线」分开——
/// 桶数不足会把 20ms 压进 <10ms 的上界桶里，P99 就会假性达标（此处吃过亏，
/// 记档：桶上界口径必须能区分红线两侧）。
pub const FSYNC_BUCKETS: usize = 32;

fn bucket_of(us: u32) -> usize {
    let mut b = 0usize;
    let mut lo = 50u32;
    while b + 1 < FSYNC_BUCKETS && us > lo {
        lo = lo + lo / 4;
        b += 1;
    }
    b
}

/// 分档 fsync P99 账（三档各自独立统计——判据「不受档位影响」要求分档看）。
#[derive(Clone, Copy, Debug)]
pub struct FsyncP99 {
    counts: [[u32; FSYNC_BUCKETS]; 3],
    total: [u32; 3],
}

impl FsyncP99 {
    pub const fn new() -> Self {
        FsyncP99 { counts: [[0; FSYNC_BUCKETS]; 3], total: [0; 3] }
    }
    pub fn record(&mut self, t: Tier, us: u32) {
        let i = PowerCutDrill::idx(t);
        self.counts[i][bucket_of(us)] += 1;
        self.total[i] += 1;
    }
    /// 某档 P99（取第 99 百分位所在桶的上界——桶口径，不假装精确值）。
    pub fn p99_upper_bound(&self, t: Tier) -> Option<u32> {
        let i = PowerCutDrill::idx(t);
        if self.total[i] == 0 {
            return None;
        }
        let target = ((self.total[i] as u64 * 99 + 99) / 100) as u32; // ceil(99%)
        let mut acc = 0u32;
        let mut lo = 50u32;
        for b in 0..FSYNC_BUCKETS {
            acc += self.counts[i][b];
            if acc >= target {
                return Some(lo);
            }
            lo = lo + lo / 4;
        }
        Some(lo)
    }
    /// 三档 P99 全部 <10ms（判据达成；任一档未跑过样本 = 不成立）。
    pub fn all_tiers_pass(&self) -> bool {
        [Tier::Idle, Tier::Normal, Tier::Heavy]
            .iter()
            .all(|t| matches!(self.p99_upper_bound(*t), Some(v) if v < FSYNC_P99_REDLINE_US))
    }
}

// ---------------------------------------------------------------------------
// 7. 交互诊断数据面（不重复造曲线）
// ---------------------------------------------------------------------------

/// 交互诊断面说明：主册【交互诊断】要求的「当前档位 + 最近 60 秒实际写入量
/// 曲线」中，60 秒曲线由主域 `wcoalesce` 的秒环提供（深化批次二已实装），
/// 本件**不重复实现第二份曲线**——只补主域没有的「档位标注与审计」面。
///
/// 零冗余纪律：同一份数据两个实现 = 两个真相，宁可不写也不重复。
pub struct DiagBinding;

impl DiagBinding {
    /// 当前档位的人话说明（诊断面板直接取用，不各自造句）。
    pub const fn tier_text(t: Tier) -> &'static str {
        match t {
            Tier::Idle => "空闲档：写入 1 秒内落盘，拔 U 盘即走",
            Tier::Normal => "正常档：写入 5 秒内合并后落盘",
            Tier::Heavy => "重载档：写入最多合并 8 秒后落盘（批量场景省写）",
        }
    }
    /// 该档位下的断电最坏丢失说明（窗口上限 8s 依据 = 用户可理解的边界）。
    pub const fn promise_text(t: Tier) -> &'static str {
        match t {
            Tier::Idle => "最坏丢失：1 秒写入量",
            Tier::Normal => "最坏丢失：5 秒写入量",
            Tier::Heavy => "最坏丢失：8 秒写入量（承诺上限，不会更久）",
        }
    }
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

pub fn run_checks() -> CheckSet {
    let mut cs = CheckSet::new("F046-wcoalesce-ext");
    // 1) 三档窗口（1s/5s/8s）+ 8s 上限即断电丢失窗口承诺。
    cs.add(
        "windows_1_5_8",
        Tier::Idle.window_ms() == 1_000 && Tier::Normal.window_ms() == 5_000 && Tier::Heavy.window_ms() == 8_000,
        "",
    );
    // 2) 条件表：空闲要两条同时成立，重载任一成立即可（主册原文口径）。
    let rule = TierRule::default_rule();
    cs.add(
        "classify_idle_needs_both",
        rule.classify(5, 20) == Tier::Idle
            && rule.classify(5, 80) == Tier::Normal // 脏页 8% ≥5% → 不是空闲
            && rule.classify(50, 20) == Tier::Normal, // 请求 50 ≥10 → 不是空闲
        "",
    );
    cs.add(
        "classify_heavy_needs_either",
        rule.classify(300, 0) == Tier::Heavy && rule.classify(5, 200) == Tier::Heavy,
        "",
    );
    // 3) 旋钮清单（无隐藏魔法数）。
    let mut kt = KnobTable::new();
    rule.register_knobs(&mut kt);
    cs.add(
        "knobs_registered",
        kt.len() == 8 && kt.get("wc.win_heavy_ms") == Some(8_000) && kt.get("wc.dwell_ms") == Some(5_000) && kt.all_in_range(),
        "",
    );
    // 4) F045 共享数据源：未喂数据 → 保守正常档（不拍脑袋）。
    let mut feed = Feed::new();
    cs.add("feed_unfed_is_normal", !feed.fed && feed.classify(&rule) == Tier::Normal, "");
    feed.note(300, 0);
    cs.add("feed_shared_from_f045", feed.fed && feed.classify(&rule) == Tier::Heavy, "");
    // 5) 切档审计：驻留未满 → 抑制并记录（迟滞生效）。
    let mut ring = TierAuditRing::new();
    let a1 = TierAudit { at_ms: 10_000, from: Tier::Normal, to: Tier::Heavy, req_per_sec: 300, dirty_permille: 0, dwell_ok: true };
    let ok1 = ring.push(a1, None, DWELL_MS, None);
    let a2 = TierAudit { at_ms: 12_000, from: Tier::Heavy, to: Tier::Idle, req_per_sec: 1, dirty_permille: 0, dwell_ok: true };
    let ok2 = ring.push(a2, Some(10_000), DWELL_MS, None); // 仅隔 2s < 5s
    let a3 = TierAudit { at_ms: 15_000, from: Tier::Heavy, to: Tier::Idle, req_per_sec: 1, dirty_permille: 0, dwell_ok: true };
    let ok3 = ring.push(a3, Some(10_000), DWELL_MS, None); // 隔 5s 满足
    cs.add("dwell_suppresses_switch", ok1 && !ok2 && ok3 && ring.suppressed == 1 && ring.total == 2, "");
    // 6) 审计行入诊断快照（交互诊断：档位切换事件可查）。
    let mut sink = DiagSink::new();
    let mut ring2 = TierAuditRing::new();
    ring2.push(
        TierAudit { at_ms: 1_000, from: Tier::Idle, to: Tier::Heavy, req_per_sec: 500, dirty_permille: 200, dwell_ok: true },
        None,
        DWELL_MS,
        Some(&mut sink),
    );
    cs.add("tier_switch_into_diag_snapshot", sink.count(DiagSev::Info) == 1, "");
    // 7) 断电丢失窗口可算（窗口 × 写入速率）。
    cs.add(
        "loss_window_computable",
        LossWindow::worst_case_bytes(8_000, 100 * 1_024 * 1024) == 800 * 1_024 * 1_024
            && LossWindow::worst_case_bytes(1_000, 100 * 1024 * 1024) == 100 * 1_024 * 1_024,
        "",
    );
    let by = LossWindow::by_tier(1_000_000);
    cs.add("loss_window_by_tier_ordered", by[0].1 < by[1].1 && by[1].1 < by[2].1, "");
    cs.add("loss_within_promise", LossWindow::within_promise(8_000, 1_000_000, 8_000_000), "");
    // 8) 断电演练：三档各 100 轮且零 fsck 零损坏才算全绿。
    let mut d = PowerCutDrill::new();
    for t in [Tier::Idle, Tier::Normal, Tier::Heavy] {
        for _ in 0..DRILL_ROUNDS {
            d.record(t, DrillResult::Recovered);
        }
    }
    cs.add("drill_all_tiers_green", d.all_green() && d.gap().is_none(), "");
    // 少一轮就不算绿（不粉饰「差一点」）
    let mut d2 = PowerCutDrill::new();
    for t in [Tier::Idle, Tier::Normal, Tier::Heavy] {
        for _ in 0..99 {
            d2.record(t, DrillResult::Recovered);
        }
    }
    cs.add("drill_gap_not_masked", !d2.all_green() && d2.gap() == Some((Tier::Idle, 1)), "");
    // 有 fsck 也不算绿
    let mut d3 = PowerCutDrill::new();
    for t in [Tier::Idle, Tier::Normal, Tier::Heavy] {
        for i in 0..DRILL_ROUNDS {
            d3.record(t, if i == 0 { DrillResult::NeededFsck } else { DrillResult::Recovered });
        }
    }
    cs.add("drill_fsck_fails", !d3.all_green(), "");
    // 9) fsync P99 分档：三档各自 <10ms。
    let mut f = FsyncP99::new();
    for t in [Tier::Idle, Tier::Normal, Tier::Heavy] {
        for _ in 0..100 {
            f.record(t, 2_000); // 2ms 落在低桶
        }
    }
    cs.add("fsync_p99_all_tiers_pass", f.all_tiers_pass(), "");
    // 重载档慢 → 不达标（判据是「不受档位影响」，重载档慢就是受影响）
    let mut f2 = FsyncP99::new();
    for t in [Tier::Idle, Tier::Normal] {
        for _ in 0..100 {
            f2.record(t, 2_000);
        }
    }
    for _ in 0..100 {
        f2.record(Tier::Heavy, 20_000);
    }
    cs.add("fsync_p99_heavy_slow_fails", !f2.all_tiers_pass(), "");
    // 未跑过样本的档 → 不成立（零样本 ≠ 达标）
    cs.add("fsync_p99_no_sample_not_pass", !FsyncP99::new().all_tiers_pass(), "");
    // 10) 重测记录（B-702/B-703 自适应模式重测重录）。
    let mut dl = DrillLedger::new();
    dl.record_rerun(true);
    dl.record_rerun(false);
    cs.add("rerun_ledger", dl.reruns == 2 && !dl.last_green, "");
    // 11) 交互诊断文案：三档各有用户能读懂的说明（不裸抛档位名）。
    cs.add(
        "diag_text_per_tier",
        DiagBinding::tier_text(Tier::Heavy) == "重载档：写入最多合并 8 秒后落盘（批量场景省写）"
            && DiagBinding::promise_text(Tier::Heavy) == "最坏丢失：8 秒写入量（承诺上限，不会更久）"
            && DiagBinding::tier_text(Tier::Idle).contains("拔 U 盘即走"),
        "",
    );
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bucket_boundaries_are_monotonic() {
        let mut last = 0usize;
        for us in [0u32, 50, 63, 100, 1_000, 10_000, 100_000, 5_000_000, u32::MAX] {
            let b = bucket_of(us);
            assert!(b >= last, "桶必须随耗时单调不减: {} -> {} < {}", us, b, last);
            last = b;
        }
        assert!(last < FSYNC_BUCKETS);
    }

    #[test]
    fn fsync_p99_is_upper_bound_not_exact() {
        let mut f = FsyncP99::new();
        for _ in 0..10 {
            f.record(Tier::Idle, 60);
        }
        let p = f.p99_upper_bound(Tier::Idle).unwrap();
        assert!(p >= 60, "桶上界必须 ≥ 样本值：{}", p);
    }

    #[test]
    fn audit_ring_covers_oldest_when_full() {
        let mut r = TierAuditRing::new();
        for i in 0..(AUDIT_RING + 5) {
            r.push(
                TierAudit { at_ms: i as u64, from: Tier::Normal, to: Tier::Heavy, req_per_sec: 0, dirty_permille: 0, dwell_ok: true },
                None,
                DWELL_MS,
                None,
            );
        }
        assert_eq!(r.len(), AUDIT_RING);
        let mut out = [TierAudit { at_ms: 0, from: Tier::Normal, to: Tier::Heavy, req_per_sec: 0, dirty_permille: 0, dwell_ok: true }; AUDIT_RING];
        let n = r.snapshot(&mut out);
        assert_eq!(n, AUDIT_RING);
        assert_eq!(out[0].at_ms, 5, "最旧 5 条被覆盖");
    }

    #[test]
    fn drill_reports_which_tier_is_missing() {
        let mut d = PowerCutDrill::new();
        for _ in 0..DRILL_ROUNDS {
            d.record(Tier::Idle, DrillResult::Recovered);
            d.record(Tier::Normal, DrillResult::Recovered);
        }
        assert_eq!(d.gap(), Some((Tier::Heavy, DRILL_ROUNDS)));
    }

    #[test]
    fn feed_clamps_dirty_permille() {
        let mut f = Feed::new();
        f.note(0, 5_000); // 脏页千分越界输入
        assert_eq!(f.dirty_permille, 1_000, "外部数据必须清洗（不信任输入）");
    }
}
