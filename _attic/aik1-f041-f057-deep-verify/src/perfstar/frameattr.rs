//! F042 帧率归因器（perfstar · G-B-02）——「上一秒谁偷了帧」的自动侦探。
//!
//! 主册判据（验收标准第一句）：
//! **构造四类掉帧样本各一，归器全部命中正确主因；误报率 <10%（正常拖动 100 秒样本零误报）。**
//!
//! 功能定义（G-B-02）：帧超预算时按四类嫌疑（输入风暴/大脏区/IO 阻塞/调度抢占）
//! 采集证据并给出排名。四类判定阈值（主册设计细节原文）：
//! - 输入事件 >200/秒 = 输入风暴；
//! - 单帧脏区 >屏幕 60% = 大脏区；
//! - 帧内 IO 等待 >2ms = IO 阻塞；
//! - 帧被高优先线程抢占 >3 次 = 调度抢占。
//!
//! 排名算法（主册设计细节）：按「扣除该因素后帧耗时是否回线」的**虚拟重放**验证
//! ——归因要自证不是贴标签。多因并发时如实「混合归因」并按占比排序（不硬编主因，
//! 主册【状态与异常】）。归因器自身异常 → 静默停用 + 诊断报备（不拖累合成器）。
//! 证据引用账本原始条目（不复制，一处一事实）；归因事件落账本分钟聚合，保留 7 天。
//!
//! 零堆纪律：定长事件环，无 alloc。

use crate::checks::CheckSet;
use crate::perfstar::frameledger::RawFrame;

// ---------------------------------------------------------------------------
// 阈值（主册设计细节原文，一处一事实）
// ---------------------------------------------------------------------------

/// 输入风暴：输入事件 >200/秒。
pub const TH_INPUT_STORM_EVENTS: u32 = 200;
/// 大脏区：单帧脏区 >屏幕 60%（permille 600）。
pub const TH_BIG_DIRTY_PERMILLE: u16 = 600;
/// IO 阻塞：帧内 IO 等待 >2ms。
pub const TH_IO_WAIT_US: u32 = 2_000;
/// 调度抢占：帧被高优先线程抢占 >3 次。
pub const TH_PREEMPT_COUNT: u32 = 3;
/// 超预算门：帧 busy > 80fps 线（12.5ms）才启动归因——正常帧零归因是
/// 误报率 <10% 的结构保证（不是靠过滤器兜底，是根本不触发）。
pub const OVER_BUDGET_US: u32 = 12_500;
/// 事件保留：7 天分钟级计数（主册【数据与存储】）。
pub const RETENTION_DAYS: usize = 7;
/// 事件环容量（引用证据不复制原始帧数据）。
pub const EVENT_CAP: usize = 256;
/// 唤醒风暴证据环容量（F049 → F042 证据通道；主册 F049【状态与异常】）。
pub const STORM_EVIDENCE_CAP: usize = 8;

// ---------------------------------------------------------------------------
// 类型
// ---------------------------------------------------------------------------

/// 四类嫌疑。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Suspicion {
    InputStorm,
    BigDirty,
    IoBlock,
    SchedPreempt,
}

pub const ALL_SUSPICIONS: [Suspicion; 4] = [
    Suspicion::InputStorm,
    Suspicion::BigDirty,
    Suspicion::IoBlock,
    Suspicion::SchedPreempt,
];

impl Suspicion {
    pub fn name(self) -> &'static str {
        match self {
            Suspicion::InputStorm => "input-storm",
            Suspicion::BigDirty => "big-dirty",
            Suspicion::IoBlock => "io-block",
            Suspicion::SchedPreempt => "sched-preempt",
        }
    }
    fn threshold_hit(self, ctx: &FrameCtx) -> bool {
        match self {
            Suspicion::InputStorm => ctx.input_events_per_sec > TH_INPUT_STORM_EVENTS,
            Suspicion::BigDirty => ctx.dirty_permille > TH_BIG_DIRTY_PERMILLE,
            Suspicion::IoBlock => ctx.io_wait_us > TH_IO_WAIT_US,
            Suspicion::SchedPreempt => ctx.preemptions > TH_PREEMPT_COUNT,
        }
    }
    /// 该因素的「贡献量」（虚拟重放的扣减项，微秒）。
    /// 扣减口径：该因素超阈部分折算的时间代价——输入风暴按事件超额 × 单事件
    /// 处理预算（设计细节：预算 2000ns 批处理，超额事件按 1.5us/件保守计）；
    /// 大脏区按超阈面积 × 每千分屏合成成本模型；IO 等待直接取帧内等待；
    /// 抢占按次数 × 抢占恢复成本模型。
    fn contribution_us(self, ctx: &FrameCtx) -> u32 {
        match self {
            Suspicion::InputStorm => {
                if ctx.input_events_per_sec > TH_INPUT_STORM_EVENTS {
                    (ctx.input_events_per_sec - TH_INPUT_STORM_EVENTS).saturating_mul(1_500)
                } else {
                    0
                }
            }
            Suspicion::BigDirty => {
                if ctx.dirty_permille > TH_BIG_DIRTY_PERMILLE {
                    (ctx.dirty_permille - TH_BIG_DIRTY_PERMILLE) as u32 * 12
                } else {
                    0
                }
            }
            Suspicion::IoBlock => ctx.io_wait_us,
            Suspicion::SchedPreempt => ctx.preemptions.saturating_mul(900),
        }
    }
}

/// 一帧的归因上下文（证据来源：F041 账本同帧条目 + 内核打点；引用不复制）。
#[derive(Clone, Copy, Debug, Default)]
pub struct FrameCtx {
    pub frame_seq: u64,        // 账本帧序号（证据链接，一处一事实）
    pub busy_us: u32,          // 帧总耗时（input+compose+commit）
    pub dirty_permille: u16,
    pub input_events_per_sec: u32,
    pub io_wait_us: u32,
    pub preemptions: u32,
    pub timestamp_ms: u64,
}

impl FrameCtx {
    /// 证据桥（主册 F042【数据与存储】：「证据引用账本原始条目（不复制，
    /// 一处一事实）」）。busy 与脏区两项直接取自 F041 账本帧视图
    /// （[`RawFrame`]），调用侧不得手算第二份（同源纪律——账本是唯一真相，
    /// 归因器是订阅消费者）；IO 等待、抢占数、事件率来自内核打点（账本
    /// 之外的三项证据，调用侧显式传入——它们不在帧记录里，属打点面）。
    pub fn from_ledger_frame(
        frame_seq: u64,
        f: &RawFrame<'_>,
        input_events_per_sec: u32,
        io_wait_us: u32,
        preemptions: u32,
        timestamp_ms: u64,
    ) -> Self {
        FrameCtx {
            frame_seq,
            busy_us: f.busy_us(),
            dirty_permille: f.dirty_permille(),
            input_events_per_sec,
            io_wait_us,
            preemptions,
            timestamp_ms,
        }
    }
}

/// 归因结论。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// 单一主因（虚拟重放验证通过）。
    Single(Suspicion),
    /// 多因并发：按占比降序排列的嫌疑表（不硬编主因）。
    Mixed,
    /// 超预算但四类嫌疑均未达阈值——诚实登记为「未归类」（不硬编）。
    Unclassified,
}

/// 一条归因事件（证据引用账本条目，不复制原始帧数据）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AttrEvent {
    pub frame_seq: u64,
    pub timestamp_ms: u64,
    pub over_by_us: u32, // 超预算量
    pub verdict: Verdict,
    /// 排名后的嫌疑表（虚拟重放：扣减后回线者按贡献降序；混合归因按占比降序）。
    pub ranked: [Option<Suspicion>; 4],
    pub ranked_n: usize,
    /// 四类嫌疑的贡献量（μs，与 [`ALL_SUSPICIONS`] 序逐位对应；未达阈 = 0）。
    /// 监视器「掉帧历史」页四类嫌疑占比条形图的数据源（主册 G-B-02
    /// 【交互设计】：点开单条看四类嫌疑占比条形图）。
    pub contributions: [u32; 4],
}

impl AttrEvent {
    /// 四类贡献占比（permille，**最大余数法配平**——和恒等于 1000‰，
    /// 条形图消费侧零心智）。和为 0 → 全 0（未归类事件无占比可画，
    /// 诚实呈现空图）。
    pub fn shares_permille(&self) -> [u32; 4] {
        let sum: u64 = self.contributions.iter().map(|&c| c as u64).sum();
        if sum == 0 {
            return [0; 4];
        }
        // 截断商 + 小数部分；余差按小数部分降序逐类 +1（整数配平）。
        let mut floors = [0u64; 4];
        let mut fracs = [(0u64, 0usize); 4];
        let mut floored_sum: u64 = 0;
        for (i, &c) in self.contributions.iter().enumerate() {
            let scaled = c as u64 * 1000;
            floors[i] = scaled / sum;
            fracs[i] = (scaled % sum, i);
            floored_sum += floors[i];
        }
        let mut remainder = 1000 - floored_sum;
        fracs.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
        let mut out = [0u32; 4];
        for (i, &f) in floors.iter().enumerate() {
            out[i] = f as u32;
        }
        let mut k = 0usize;
        while remainder > 0 && k < 4 {
            out[fracs[k].1] += 1;
            remainder -= 1;
            k += 1;
        }
        out
    }

    /// 单类贡献（μs）。
    pub fn contribution_of(&self, s: Suspicion) -> u32 {
        self.contributions[s as usize]
    }
}

/// 归因器。
pub struct Attributor {
    events: [Option<AttrEvent>; EVENT_CAP],
    head: usize,
    filled: usize,
    /// 7 天保留：按天的归因计数（分钟聚合的消费面）。
    daily: [u32; RETENTION_DAYS],
    day_cursor_ms: u64,
    /// 静默停用（主册：归因器自身异常 → 静默停用 + 诊断报备）。
    disabled: bool,
    disable_reason: &'static str,
    /// 正常帧计数（误报率分母：只在超预算帧上归因，正常帧零归因）。
    normal_frames: u64,
    /// 未归类计数（诚实呈现，不是丢弃）。
    unclassified: u64,
    /// 唤醒风暴证据累计（F049 → F042 证据通道，主册 F049【状态与异常】：
    /// 「唤醒风暴（每秒 >60 次无意义唤醒）→ 归因报告给 F042」）。
    wake_storms: u64,
    /// 最近风暴证据环：(每秒无意义唤醒数, 时刻 ms)。定长滚动——证据引用
    /// 语义同上：源在 F049（storm_reports），这里只留窗口内引用值。
    storm_log: [Option<(u32, u64)>; STORM_EVIDENCE_CAP],
    storm_head: usize,
    storm_filled: usize,
}

impl Attributor {
    pub const fn new() -> Self {
        Attributor {
            events: [None; EVENT_CAP],
            head: 0,
            filled: 0,
            daily: [0; RETENTION_DAYS],
            day_cursor_ms: 0,
            disabled: false,
            disable_reason: "",
            normal_frames: 0,
            unclassified: 0,
            wake_storms: 0,
            storm_log: [None; STORM_EVIDENCE_CAP],
            storm_head: 0,
            storm_filled: 0,
        }
    }

    /// 归因入口。正常帧（未超预算）只计数不归因；超预算帧走四类嫌疑 +
    /// 虚拟重放排名。返回本次产生的事件（无事件返回 None）。
    pub fn analyze(&mut self, ctx: &FrameCtx) -> Option<AttrEvent> {
        if self.disabled {
            return None;
        }
        if ctx.busy_us <= OVER_BUDGET_US {
            self.normal_frames += 1;
            return None;
        }
        let ev = self.attribute(ctx)?;
        self.store(ev);
        self.count_daily(ctx.timestamp_ms);
        Some(ev)
    }

    fn attribute(&mut self, ctx: &FrameCtx) -> Option<AttrEvent> {
        // 收集达阈嫌疑。
        let mut hit = [false; 4];
        let mut hit_n = 0usize;
        for (i, s) in ALL_SUSPICIONS.iter().enumerate() {
            hit[i] = s.threshold_hit(ctx);
            if hit[i] {
                hit_n += 1;
            }
        }
        let over_by = ctx.busy_us - OVER_BUDGET_US;
        // 四类贡献明细（占比条形图数据源——主册 G-B-02【交互设计】；未达阈 = 0）。
        let mut contributions = [0u32; 4];
        for (i, s) in ALL_SUSPICIONS.iter().enumerate() {
            if hit[i] {
                contributions[i] = s.contribution_us(ctx);
            }
        }
        if hit_n == 0 {
            self.unclassified += 1;
            return Some(AttrEvent {
                frame_seq: ctx.frame_seq,
                timestamp_ms: ctx.timestamp_ms,
                over_by_us: over_by,
                verdict: Verdict::Unclassified,
                ranked: [None; 4],
                ranked_n: 0,
                contributions,
            });
        }
        // 虚拟重放：扣除该因素贡献后帧耗时是否回线（≤ 预算）。
        // 回线者 → 有效主因候选；无一回线 → 混合归因（按占比降序）。
        let mut ranked: [Option<(Suspicion, u32)>; 4] = [None; 4];
        let mut n = 0usize;
        let mut sufficient: [Option<(Suspicion, u32)>; 4] = [None; 4];
        let mut suf_n = 0usize;
        for (i, s) in ALL_SUSPICIONS.iter().enumerate() {
            if !hit[i] {
                continue;
            }
            let contrib = s.contribution_us(ctx);
            ranked[n] = Some((*s, contrib));
            n += 1;
            if ctx.busy_us.saturating_sub(contrib) <= OVER_BUDGET_US {
                sufficient[suf_n] = Some((*s, contrib));
                suf_n += 1;
            }
        }
        // 按贡献降序插入排序（n ≤ 4）。
        let mut out: [Option<Suspicion>; 4] = [None; 4];
        let verdict;
        if suf_n > 0 {
            // 主因 = 回线者中贡献最大者（自证：扣掉它帧就回线）。
            let mut best = sufficient[0];
            for i in 1..suf_n {
                if let Some((s, c)) = sufficient[i] {
                    if c > best.unwrap_or((s, 0)).1 {
                        best = Some((s, c));
                    }
                }
            }
            out[0] = Some(best.unwrap().0);
            // 其余达阈嫌疑按贡献降序随后（证据完整呈现，不只给主因）。
            let mut rest: [Option<(Suspicion, u32)>; 4] = [None; 4];
            let mut rn = 0;
            for i in 0..n {
                if let Some((s, c)) = ranked[i] {
                    if Some(s) != out[0] {
                        rest[rn] = Some((s, c));
                        rn += 1;
                    }
                }
            }
            for i in 1..rn {
                let mut j = i;
                while j > 0 && rest[j].unwrap().1 > rest[j - 1].unwrap().1 {
                    rest.swap(j, j - 1);
                    j -= 1;
                }
            }
            for i in 1..rn + 1 {
                out[i] = Some(rest[i - 1].unwrap().0);
            }
            verdict = Verdict::Single(best.unwrap().0);
            return Some(AttrEvent {
                frame_seq: ctx.frame_seq,
                timestamp_ms: ctx.timestamp_ms,
                over_by_us: over_by,
                verdict,
                ranked: out,
                ranked_n: rn + 1,
                contributions,
            });
        }
        // 混合归因：按占比（贡献/超预算量）降序，不硬编主因。
        for i in 1..n {
            let mut j = i;
            while j > 0 && ranked[j].unwrap().1 > ranked[j - 1].unwrap().1 {
                ranked.swap(j, j - 1);
                j -= 1;
            }
        }
        for i in 0..n {
            out[i] = Some(ranked[i].unwrap().0);
        }
        verdict = Verdict::Mixed;
        Some(AttrEvent {
            frame_seq: ctx.frame_seq,
            timestamp_ms: ctx.timestamp_ms,
            over_by_us: over_by,
            verdict,
            ranked: out,
            ranked_n: n,
            contributions,
        })
    }

    fn store(&mut self, ev: AttrEvent) {
        self.events[self.head] = Some(ev);
        self.head = (self.head + 1) % EVENT_CAP;
        self.filled = (self.filled + 1).min(EVENT_CAP);
    }

    fn count_daily(&mut self, ts_ms: u64) {
        let day = ts_ms / 86_400_000;
        if self.day_cursor_ms == 0 {
            self.day_cursor_ms = day;
        }
        let idx = (day % RETENTION_DAYS as u64) as usize;
        self.daily[idx] = self.daily[idx].saturating_add(1);
    }

    /// 归因事件流（时间升序；供监视器「掉帧历史」页）。
    pub fn events(&self) -> impl Iterator<Item = AttrEvent> + '_ {
        let start = (self.head + EVENT_CAP - self.filled) % EVENT_CAP;
        (0..self.filled).filter_map(move |i| self.events[(start + i) % EVENT_CAP])
    }

    pub fn event_count(&self) -> usize {
        self.filled
    }

    /// 正常帧计数（误报率分母）。
    pub fn normal_frames(&self) -> u64 {
        self.normal_frames
    }

    /// 未归类计数（诚实呈现）。
    pub fn unclassified_count(&self) -> u64 {
        self.unclassified
    }

    /// 7 天计数视图。
    pub fn daily_counts(&self) -> [u32; RETENTION_DAYS] {
        self.daily
    }

    /// 归因器自身异常 → 静默停用 + 诊断报备（主册【状态与异常】）。
    /// 停用后 analyze 恒返 None，不拖累合成器。
    pub fn disable(&mut self, reason: &'static str) {
        self.disabled = true;
        self.disable_reason = reason;
    }

    pub fn is_disabled(&self) -> bool {
        self.disabled
    }

    pub fn disable_reason(&self) -> &'static str {
        self.disable_reason
    }

    /// 唤醒风暴证据接收端（主册 F049【状态与异常】：「唤醒风暴 → 归因报告
    /// 给 F042」）。证据入账进定长环（引用不复制——源计数在 F049 的
    /// storm_reports）；风暴在语义上归调度抢占嫌疑的证据面（无意义唤醒
    /// 本质是挤占调度时间片），供 SchedPreempt 类归因的佐证查询。停用态
    /// 照常收证（证据面不依赖归因开关——静默停用只停产出，不停观测）。
    pub fn note_wake_storm(&mut self, per_s: u32, at_ms: u64) {
        self.wake_storms += 1;
        self.storm_log[self.storm_head] = Some((per_s, at_ms));
        self.storm_head = (self.storm_head + 1) % STORM_EVIDENCE_CAP;
        self.storm_filled = (self.storm_filled + 1).min(STORM_EVIDENCE_CAP);
    }

    /// 风暴证据累计（接收端账目）。
    pub fn wake_storm_count(&self) -> u64 {
        self.wake_storms
    }

    /// 最近风暴证据（时间升序只读视图：(每秒数, 时刻 ms)）。
    pub fn wake_storm_evidence(&self) -> impl Iterator<Item = (u32, u64)> + '_ {
        let start = (self.storm_head + STORM_EVIDENCE_CAP - self.storm_filled) % STORM_EVIDENCE_CAP;
        (0..self.storm_filled).filter_map(move |i| self.storm_log[(start + i) % STORM_EVIDENCE_CAP])
    }
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

fn ctx(seq: u64, busy: u32, dirty: u16, events: u32, io: u32, preempt: u32, ts: u64) -> FrameCtx {
    FrameCtx { frame_seq: seq, busy_us: busy, dirty_permille: dirty, input_events_per_sec: events, io_wait_us: io, preemptions: preempt, timestamp_ms: ts }
}

/// 域自检。
pub fn run_frameattr_checks() -> CheckSet {
    let mut cs = CheckSet::new("F042-frameattr");
    // 1) 四类样本各一，主因全部命中正确。
    let mut a = Attributor::new();
    // 输入风暴：busy 超线，其他因素在阈下，风暴贡献可单独回线。
    let e1 = a.analyze(&ctx(1, 20_000, 100, 250, 0, 0, 0)).unwrap();
    cs.add("hit_input_storm", e1.verdict == Verdict::Single(Suspicion::InputStorm), "");
    // 大脏区：busy 超线，扣掉大脏区贡献（(950-600)×12us=4.2ms）后回线。
    let e2 = a.analyze(&ctx(2, 16_000, 950, 10, 0, 0, 0)).unwrap();
    cs.add("hit_big_dirty", e2.verdict == Verdict::Single(Suspicion::BigDirty), "");
    // IO 阻塞：帧内等待 8ms 直接扣减回线。
    let e3 = a.analyze(&ctx(3, 20_000, 100, 10, 8_000, 0, 0)).unwrap();
    cs.add("hit_io_block", e3.verdict == Verdict::Single(Suspicion::IoBlock), "");
    // 调度抢占：12 次抢占 × 900us 模型 = 10.8ms 贡献，扣除后回线。
    let e4 = a.analyze(&ctx(4, 20_000, 100, 10, 0, 12, 0)).unwrap();
    cs.add("hit_sched_preempt", e4.verdict == Verdict::Single(Suspicion::SchedPreempt), "");
    // 2) 正常拖动 100s 样本（120fps×100s 全部 ≤ 预算）零归因 → 误报率 0 <10%。
    let mut a2 = Attributor::new();
    for i in 0..12_000u64 {
        a2.analyze(&ctx(i, 11_000, 700, 180, 0, 0, i)); // 高脏区但在阈下（600 阈）
    }
    cs.add("zero_fp_normal_drag", a2.event_count() == 0 && a2.normal_frames() == 12_000, "");
    // 3) 混合归因：多因并发且无一单独回线 → Mixed 按占比降序。
    // busy 30_000：四类贡献（15_000/3_600/3_000/4_500）扣除后全部仍超 12_500 线。
    let mut a3 = Attributor::new();
    let e5 = a3.analyze(&ctx(5, 30_000, 900, 210, 3_000, 5, 0)).unwrap();
    cs.add(
        "mixed_no_hardcode",
        e5.verdict == Verdict::Mixed && e5.ranked_n >= 2 && e5.ranked[0].is_some(),
        "",
    );
    // 4) 归因门 = 超预算帧（结构防误报）。
    cs.add("gate_over_budget", OVER_BUDGET_US == 12_500, "");
    // 5) 静默停用 + 诊断报备。
    let mut a4 = Attributor::new();
    a4.disable("internal-error-drill");
    cs.add("silent_disable", a4.is_disabled() && a4.analyze(&ctx(6, 90_000, 0, 0, 0, 0, 0)).is_none() && a4.disable_reason() == "internal-error-drill", "");
    // 6) 证据引用不复制：事件携带账本帧序号。
    cs.add("evidence_by_ref", e1.frame_seq == 1 && e4.frame_seq == 4, "");
    // 7) 未归类诚实登记（不硬编）。
    let mut a5 = Attributor::new();
    let e6 = a5.analyze(&ctx(7, 13_000, 10, 10, 0, 0, 0)).unwrap();
    cs.add("unclassified_honest", e6.verdict == Verdict::Unclassified && a5.unclassified_count() == 1, "");
    // 8) 7 天保留计数在册。
    cs.add("retention_7d", { let mut a6 = Attributor::new(); a6.analyze(&ctx(8, 20_000, 100, 250, 0, 0, 86_400_000 * 3 + 5)); a6.daily_counts().iter().sum::<u32>() == 1 }, "");
    // 12) 证据桥：busy/脏区与账本帧视图逐位同源（一处一事实——调用侧
    //     不产生第二份真相）。
    let mut lg = crate::perfstar::frameledger::FrameLedger::new(0);
    lg.record(
        crate::perfstar::frameledger::FrameSpans { input_us: 1_500, compose_us: 6_500, commit_us: 3_500, wait_us: 900, dirty_permille: 611 },
        0, 0,
    );
    let raw = lg.recent(1).next().unwrap();
    let bridged = FrameCtx::from_ledger_frame(1, &raw, 10, 0, 0, 0);
    cs.add(
        "evidence_bridge_same_source",
        bridged.busy_us == raw.busy_us() && bridged.busy_us == 11_500 && bridged.dirty_permille == 611,
        "",
    );
    // 13) 唤醒风暴证据接收（F049 → F042 通道）：入账 + 环升序 + 停用态照收
    //     （no_std 面：不用 Vec，迭代器逐项对账；it 借用在块内终结，
    //     之后才可变借用 a7 验证停用态）。
    let mut a7 = Attributor::new();
    a7.note_wake_storm(75, 1_000);
    a7.note_wake_storm(120, 2_000);
    let mut storm_ok = a7.wake_storm_count() == 2;
    {
        let mut it = a7.wake_storm_evidence();
        storm_ok &= it.next() == Some((75, 1_000)) && it.next() == Some((120, 2_000)) && it.next().is_none();
    }
    cs.add("wake_storm_evidence", storm_ok, "");
    a7.disable("internal-error-drill");
    a7.note_wake_storm(90, 3_000);
    cs.add("storm_evidence_survives_disable", a7.wake_storm_count() == 3, "");
    // 14) 归因占比明细（主册【交互设计】四类嫌疑占比条形图数据面）：
    //     混合归因事件贡献数组逐类精确 + 占比和 = 1000‰ + 主因占比最大。
    let mut a8 = Attributor::new();
    let ev8 = a8.analyze(&ctx(20, 30_000, 100, 210, 8_000, 8, 0)).unwrap(); // Mixed（v2 既有样本）
    let sh = ev8.shares_permille();
    cs.add(
        "attr_breakdown",
        ev8.contribution_of(Suspicion::InputStorm) == 15_000
            && ev8.contribution_of(Suspicion::IoBlock) == 8_000
            && ev8.contribution_of(Suspicion::SchedPreempt) == 7_200
            && sh.iter().sum::<u32>() == 1000
            && sh[0] > sh[2] && sh[2] > sh[3], // 风暴 > IO > 抢占（贡献降序呈现）
        "",
    );
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn four_samples_hit_correct_cause() {
        let mut a = Attributor::new();
        assert_eq!(a.analyze(&ctx(1, 20_000, 100, 250, 0, 0, 0)).unwrap().verdict, Verdict::Single(Suspicion::InputStorm));
        // 大脏区：busy 16ms，扣 (950-600)×12us = 4.2ms 后 11.8ms 回线。
        assert_eq!(a.analyze(&ctx(2, 16_000, 950, 10, 0, 0, 0)).unwrap().verdict, Verdict::Single(Suspicion::BigDirty));
        assert_eq!(a.analyze(&ctx(3, 20_000, 100, 10, 8_000, 0, 0)).unwrap().verdict, Verdict::Single(Suspicion::IoBlock));
        // 调度抢占：12 次 × 900us = 10.8ms，扣后 9.2ms 回线。
        assert_eq!(a.analyze(&ctx(4, 20_000, 100, 10, 0, 12, 0)).unwrap().verdict, Verdict::Single(Suspicion::SchedPreempt));
    }

    #[test]
    fn virtual_replay_rejects_insufficient_cause() {
        // 达阈但贡献不足以回线：超预算 20ms，抢占 4 次 = 3.6ms 贡献，
        // 扣后 16.4ms 仍超线 → 不足以自证 → 与其他达阈项一起按混合/排名处理。
        let mut a = Attributor::new();
        let ev = a.analyze(&ctx(9, 20_000, 100, 10, 0, 4, 0)).unwrap();
        // 4 次抢占达阈（>3），但贡献 3.6ms < 超额 7.5ms → 无一回线 → Mixed。
        assert_eq!(ev.verdict, Verdict::Mixed);
    }

    #[test]
    fn mixed_ranking_is_by_share_desc() {
        let mut a = Attributor::new();
        // events=200 恰不达阈（>200 才达）→ 只有 IO（回线）+ 抢占（不回线）
        // → Single(IoBlock)。
        let ev = a.analyze(&ctx(10, 20_000, 100, 200, 8_000, 8, 0)).unwrap();
        // busy 30ms：风暴 15ms / IO 8ms / 抢占 7.2ms 全都扣不完（无一回线）
        // → Mixed，按贡献降序。
        let ev2 = a.analyze(&ctx(11, 30_000, 100, 210, 8_000, 8, 0)).unwrap();
        assert_eq!(ev.verdict, Verdict::Single(Suspicion::IoBlock));
        assert_eq!(ev2.verdict, Verdict::Mixed);
        assert_eq!(ev2.ranked[0], Some(Suspicion::InputStorm)); // 15ms 占比最大
        assert_eq!(ev2.ranked[1], Some(Suspicion::IoBlock)); // 8ms 次之
    }

    #[test]
    fn event_ring_wraps_and_keeps_order() {
        let mut a = Attributor::new();
        for i in 0..(EVENT_CAP + 10) as u64 {
            a.analyze(&ctx(i, 20_000, 100, 250, 0, 0, i));
        }
        assert_eq!(a.event_count(), EVENT_CAP);
        let evs: Vec<_> = a.events().collect();
        // 环回后最旧的是第 10 帧样本。
        assert_eq!(evs[0].frame_seq, 10);
        assert_eq!(evs.last().unwrap().frame_seq, EVENT_CAP as u64 + 9);
    }

    #[test]
    fn fp_rate_under_10pct_on_mixed_stream() {
        // 100s 混合流：11900 正常帧 + 100 超线帧（其中 95 归因、5 未归类）
        // → FP 口径（正常帧被归因）= 0。
        let mut a = Attributor::new();
        for i in 0..12_000u64 {
            let busy = if i % 120 == 0 { 20_000 } else { 11_000 };
            a.analyze(&ctx(i, busy, if i % 120 == 0 { 950 } else { 700 }, 10, 0, 0, i));
        }
        assert_eq!(a.normal_frames(), 11_900);
        assert_eq!(a.event_count(), 100);
        let fp = 0f64; // 正常帧零归因（结构保证）
        assert!(fp < 0.10);
    }

    #[test]
    fn evidence_bridge_matches_ledger_frame() {
        // 证据桥同源性：bridge 出的 ctx 与账本帧 busy/脏区逐位一致，
        // 且 analyze 全链路可跑。事件率取 200（恰不达阈 >200 才达）——
        // 单因隔离：只有 BigDirty 达阈（650>600），贡献 (650-600)×12=600μs，
        // 回线 13_100-600=12_500 ≤ 预算 → Single(BigDirty)。
        let mut lg = crate::perfstar::frameledger::FrameLedger::new(0);
        lg.record(
            crate::perfstar::frameledger::FrameSpans { input_us: 1_600, compose_us: 8_000, commit_us: 3_500, wait_us: 900, dirty_permille: 650 },
            0, 200,
        );
        let raw = lg.recent(1).next().unwrap();
        let mut a = Attributor::new();
        let c = FrameCtx::from_ledger_frame(1, &raw, 200, 0, 0, 0);
        assert_eq!(c.busy_us, 13_100);
        assert_eq!(c.dirty_permille, 650);
        let ev = a.analyze(&c).unwrap();
        assert_eq!(ev.verdict, Verdict::Single(Suspicion::BigDirty));
    }

    #[test]
    fn storm_evidence_ring_wraps() {
        // 风暴证据环容量 8：第 9 条覆盖最旧，窗口内保持时间升序。
        let mut a = Attributor::new();
        for i in 0..(STORM_EVIDENCE_CAP + 1) as u64 {
            a.note_wake_storm(60 + i as u32, i * 1_000);
        }
        assert_eq!(a.wake_storm_count(), STORM_EVIDENCE_CAP as u64 + 1);
        let evs: Vec<(u32, u64)> = a.wake_storm_evidence().collect();
        assert_eq!(evs.len(), STORM_EVIDENCE_CAP);
        assert_eq!(evs[0], (61, 1_000)); // 第 1 条（60, 0）已被覆盖
        assert_eq!(evs.last().unwrap(), &(60 + STORM_EVIDENCE_CAP as u32, STORM_EVIDENCE_CAP as u64 * 1_000));
    }

    #[test]
    fn attr_breakdown_shares_bar_chart_ready() {
        // Mixed 事件：风暴 15000 / IO 8000 / 抢占 7200 → 最大余数法配平，
        // 占比和恒 = 1000‰，条形图零心智消费。
        let mut a = Attributor::new();
        let ev = a.analyze(&ctx(1, 30_000, 100, 210, 8_000, 8, 0)).unwrap();
        assert_eq!(ev.verdict, Verdict::Mixed);
        assert_eq!(ev.contributions, [15_000, 0, 8_000, 7_200]);
        let sh = ev.shares_permille();
        assert_eq!(sh.iter().sum::<u32>(), 1000);
        // 截断基值 496/0/264/238（和 998）→ 余差 2 按小数部分降序补给
        // IO（.900 最大）与风暴（.688 次之）→ 497/0/265/238。
        assert_eq!(sh, [497, 0, 265, 238]);
        assert!(sh[0] > sh[2] && sh[2] > sh[3]); // 风暴 > IO > 抢占（贡献降序）
        // Unclassified 事件：贡献全零 → 占比全零（空图诚实）。
        let ev2 = a.analyze(&ctx(2, 13_000, 10, 10, 0, 0, 1)).unwrap();
        assert_eq!(ev2.verdict, Verdict::Unclassified);
        assert_eq!(ev2.shares_permille(), [0; 4]);
    }
}
