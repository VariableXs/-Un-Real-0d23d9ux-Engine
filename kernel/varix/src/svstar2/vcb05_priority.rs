//! CGPU-F0165 · 优先级发射与抢占点（CGPU-B 域 · 批次 B01 · 目标 380 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F0165`
//!
//! **判据（锚点原文）**：三档发射序、抢占点检查、让位延迟 P95、饥饿防护
//! （F0090 老化复用）、遥测。
//!
//! **职责定位（锚点原文）**：任务的优先级发射：**三档**（实时帧/交互/后
//! 台）与 F0002 线程池的优先级队列对接，**发射顺序**（高优先级的就绪任务
//! 先发射——同优先级 FIFO），**抢占点标记**（协作式让位点——长任务每
//! 2ms 检查抢占请求 A 域 F0063 联动）。
//!
//! # 一、三档为什么是「封闭枚举+映射表」而不是裸优先级数
//!
//! F0002 线程池按优先级队列调度，但 GPU 任务的优先级语义只有三档——
//! 裸 u8 会把 256 种数值漏进调度器当 256 种语义。三档封闭
//! （[`Tier`]）：`tier_of` 把 F0161 的 `priority` 字段（0 最高）映射到
//! 档位——映射表钉死（0=实时帧，1=交互，2 及以上=后台），判据侧独立
//! 重算全表对拍。调度器只见三档，档内 FIFO——同档任务先来先发射，
//! 不再比数值（数值的意义在入队那一刻已经用完）。
//!
//! # 二、抢占点为什么是「协作式检查」而不是「随时打断」
//!
//! GPU 任务不是可随时撕开的——命令编码到一半撕掉就是坏帧。协作式让位
//! ：长任务自己按 [`PREEMPT_CHECK_INTERVAL_MS`]
//! 步长切分片，每个分片边界是一个**抢占点**——点上检查抢占请求
//! （F0063 联动），有请求就让出（返回控制权给调度器），没有就继续下一
//! 片。代价：请求的响应延迟取决于距下一个检查点还有多远——上界恰等于
//! 检查步长 2ms。让位延迟样本入遥测，P95 是「协作式代价」的可机检化
//! （[`p95`]）——超过步长即说明检查点漏标，判据抓。
//!
//! # 三、饥饿防护：后台任务的老化升档
//!
//! 三档严格序的必然副作用：后台档被实时流持续插队，可能永远轮不上——
//! 卡顿没人看见但功能死掉。老化复用（F0090 联动）：任务每经历一轮发射
//! 而未轮上，等待计数+1；计数满 [`AGING_ROUNDS`] 即**升一档**（后台→交
//! 互→实时帧），升档后与原生同档任务同 FIFO 位次——不是插队到档首，
//! 老化给的是公平不是特权。
//!
//! # 四、与相邻条的分工
//!
//! F0163 的波次给出「现在能跑谁」；本条给出「先跑谁」——波内就绪任务
//! 按档入 [`LaunchQueue`]，drain 顺序即发射序。F0166 执行引擎消费本条
//! 的发射序与抢占点标记。本条不做执行、不做等待（那是 F0166/F0164）。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、常量与版本
// ---------------------------------------------------------------------------

/// 本项版本。
pub const PRIORITY_VERSION: &str = "CB05-priority-v1";

/// 抢占检查步长（微秒当量毫秒值——长任务每 2ms 检查抢占请求）。
pub const PREEMPT_CHECK_INTERVAL_MS: u32 = 2;

/// 老化升档阈值（连续等待轮数满 8 升一档——F0090 老化复用联动）。
pub const AGING_ROUNDS: u32 = 8;

/// P95 分位（0.95 的千分比定点——950/1000）。
pub const P95_PERMILLE: u32 = 950;

// ---------------------------------------------------------------------------
// 二、三档封闭（实时帧/交互/后台）
// ---------------------------------------------------------------------------

/// 发射档位（三档闭集——加档必须改 tier_of 映射表+判据全表）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Tier {
    /// 实时帧（最高档——本帧不发射就掉帧）。
    Realtime = 0,
    /// 交互（中档——用户操作链路上的任务）。
    Interactive = 1,
    /// 后台（最低档——预生成/预热类，可延迟）。
    Background = 2,
}

impl Tier {
    /// 三档全集（顺序即优先级序）。
    pub const fn all() -> [Tier; 3] {
        [Tier::Realtime, Tier::Interactive, Tier::Background]
    }

    /// 档短码。
    pub const fn tag(self) -> &'static str {
        match self {
            Tier::Realtime => "rt",
            Tier::Interactive => "ia",
            Tier::Background => "bg",
        }
    }

    /// 档中文名（读屏可达）。
    pub const fn zh(self) -> &'static str {
        match self {
            Tier::Realtime => "实时帧",
            Tier::Interactive => "交互",
            Tier::Background => "后台",
        }
    }

    /// 档位序数（0 最高）。
    pub const fn rank(self) -> u8 {
        match self {
            Tier::Realtime => 0,
            Tier::Interactive => 1,
            Tier::Background => 2,
        }
    }
}

/// F0161 `priority`（u8，0 最高）到三档的映射表（钉死——判据全表对拍）。
///
/// 0=实时帧；1=交互；2 及以上=后台。表外无第四种语义。
pub const fn tier_of(priority: u8) -> Tier {
    match priority {
        0 => Tier::Realtime,
        1 => Tier::Interactive,
        _ => Tier::Background,
    }
}

// ---------------------------------------------------------------------------
// 三、发射队列（三档优先级 + 同档 FIFO）
// ---------------------------------------------------------------------------

/// 就绪任务（发射队列的成员）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReadyTask {
    /// 任务句柄（帧内节点句柄——F0161）。
    pub node: u32,
    /// 所属档位（入队时由 `tier_of` 定档；老化可升档）。
    pub tier: Tier,
    /// 入队序（同档 FIFO 的次序凭据——单调递增）。
    pub seq: u64,
}

/// 三档优先级发射队列。
///
/// 不变量：drain 只从最高非空档的**队首**出（高优先级先发射——同档
/// FIFO）；seq 单调递增且同档内入队序即出队序。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LaunchQueue {
    /// 三档就绪链（下标=档位序数）。
    lanes: [Vec<ReadyTask>; 3],
    /// 全局入队序号（FIFO 凭据）。
    next_seq: u64,
    /// 累计入队数（遥测）。
    pub enqueued: u32,
    /// 累计发射数（遥测）。
    pub launched: u32,
}

impl LaunchQueue {
    /// 空队列。
    pub fn new() -> Self {
        Self {
            lanes: [Vec::new(), Vec::new(), Vec::new()],
            next_seq: 0,
            enqueued: 0,
            launched: 0,
        }
    }

    /// 入队（按档入 lane，seq 分配 FIFO 凭据）。
    pub fn push(&mut self, node: u32, tier: Tier) {
        let seq = self.next_seq;
        self.next_seq += 1;
        self.lanes[tier.rank() as usize].push(ReadyTask { node, tier, seq });
        self.enqueued += 1;
    }

    /// 发射下一个：最高非空档的队首。空队列返回 None。
    pub fn drain_next(&mut self) -> Option<ReadyTask> {
        for lane in self.lanes.iter_mut() {
            if !lane.is_empty() {
                let t = lane.remove(0);
                self.launched += 1;
                return Some(t);
            }
        }
        None
    }

    /// 某档待发射数。
    pub fn len_by(&self, tier: Tier) -> usize {
        self.lanes[tier.rank() as usize].len()
    }

    /// 全队列待发射数。
    pub fn pending(&self) -> usize {
        self.lanes.iter().map(|l| l.len()).sum()
    }

    /// 老化升档（F0090 老化复用）：把某任务从当前档搬到高一档的**队尾**
    /// （公平不是特权——升档不插队）。返回是否发生升档。
    pub fn promote(&mut self, node: u32) -> bool {
        for r in 0..3usize {
            if let Some(pos) = self.lanes[r].iter().position(|t| t.node == node) {
                if r == 0 {
                    return false; // 已是实时帧——无档可升
                }
                let mut t = self.lanes[r].remove(pos);
                t.tier = Tier::all()[r - 1];
                t.seq = self.next_seq;
                self.next_seq += 1;
                self.lanes[r - 1].push(t);
                return true;
            }
        }
        false
    }

    /// 队列读屏单行（三档计数+累计遥测）。
    pub fn screen_line(&self) -> String {
        format!(
            "发射队列：实时帧 {} / 交互 {} / 后台 {}，累计入队 {} 发射 {}",
            self.len_by(Tier::Realtime),
            self.len_by(Tier::Interactive),
            self.len_by(Tier::Background),
            self.enqueued,
            self.launched
        )
    }
}

// ---------------------------------------------------------------------------
// 四、饥饿防护（老化计数器）
// ---------------------------------------------------------------------------

/// 老化追踪器：记录每个未发射任务经历的完整发射轮数。
///
/// 每轮发射结束调用 [`AgingTracker::tick_round`]：凡在队任务等待+1；
/// 计数满 [`AGING_ROUNDS`] 的任务名单返回——由调用方对其执行
/// [`LaunchQueue::promote`]，升档后计数清零重新起算。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AgingTracker {
    /// 任务等待轮数（node → rounds）。
    waits: Vec<(u32, u32)>,
    /// 累计升档次数（遥测）。
    pub promotions: u32,
}

impl AgingTracker {
    /// 空追踪器。
    pub fn new() -> Self {
        Self::default()
    }

    /// 登记追踪任务（重复登记幂等）。
    pub fn watch(&mut self, node: u32) {
        if !self.waits.iter().any(|(n, _)| *n == node) {
            self.waits.push((node, 0));
        }
    }

    /// 一轮发射结束：所有仍在追踪的任务等待+1；返回满阈值的升档名单
    /// （升档由调用方执行后调 [`AgingTracker::reset`] 清计数）。
    pub fn tick_round(&mut self) -> Vec<u32> {
        let mut due: Vec<u32> = Vec::new();
        for (n, r) in self.waits.iter_mut() {
            *r += 1;
            if *r == AGING_ROUNDS {
                due.push(*n);
            }
        }
        due
    }

    /// 升档完成清计数（重新起算）。
    pub fn reset(&mut self, node: u32) {
        for (n, r) in self.waits.iter_mut() {
            if *n == node {
                *r = 0;
            }
        }
        self.promotions += 1;
    }

    /// 任务当前等待轮数（未追踪返回 0）。
    pub fn rounds(&self, node: u32) -> u32 {
        self.waits.iter().find(|(n, _)| *n == node).map_or(0, |(_, r)| *r)
    }
}

// ---------------------------------------------------------------------------
// 五、抢占点（协作式让位）
// ---------------------------------------------------------------------------

/// 一次长任务的分片执行报告。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SliceReport {
    /// 实际运行时长（毫秒——让位或跑满的终点）。
    pub ran_ms: u32,
    /// 是否发生让位（false=跑满全程无抢占）。
    pub preempted: bool,
    /// 让位发生时刻（毫秒；未让位为 ran_ms）。
    pub yield_at_ms: u32,
    /// 检查点总数（每 PREEMPT_CHECK_INTERVAL_MS 一个——可机检步长纪律）。
    pub checkpoints: u32,
    /// 让位延迟（毫秒——请求发出到实际让出；未让位为 0）。
    pub yield_latency_ms: u32,
}

/// 协作式抢占监视：把 `budget_ms` 的长任务切成 2ms 分片。
///
/// 每片边界检查 `requested`（F0063 抢占请求旗标）：有请求即让位——
/// 让位延迟=请求时刻到检查点的距离（≤ [`PREEMPT_CHECK_INTERVAL_MS`]，
/// 请求在片内任意时刻发出，最坏等满整片）。
pub fn run_slices(budget_ms: u32, requested: bool, request_at_ms: u32) -> SliceReport {
    let step = PREEMPT_CHECK_INTERVAL_MS;
    let mut ran = 0u32;
    let mut checkpoints = 0u32;
    while ran < budget_ms {
        let slice_end = (ran + step).min(budget_ms);
        ran = slice_end;
        checkpoints += 1;
        // 检查点：请求已发出且尚未让位 → 让出。
        if requested && ran >= request_at_ms {
            return SliceReport {
                ran_ms: ran,
                preempted: true,
                yield_at_ms: ran,
                checkpoints,
                yield_latency_ms: ran - request_at_ms,
            };
        }
    }
    SliceReport {
        ran_ms: ran,
        preempted: false,
        yield_at_ms: ran,
        checkpoints,
        yield_latency_ms: 0,
    }
}

/// 让位延迟 P95（整数千分比分位——样本升序取第 ceil(0.95·n) 个）。
///
/// 空样本返回 0。锚点「让位延迟 P95」的可机检化：P95 ≤
/// [`PREEMPT_CHECK_INTERVAL_MS`] 即协作式代价达标（判据对拍）。
pub fn p95(samples: &Vec<u32>) -> u32 {
    if samples.is_empty() {
        return 0;
    }
    let mut s = samples.clone();
    s.sort_unstable();
    // index = ceil(0.95 * n) - 1 = (950*n + 999) / 1000 - 1（整数推导）。
    let n = s.len() as u32;
    let idx = ((P95_PERMILLE * n + 999) / 1000) - 1;
    s[idx as usize]
}

// ---------------------------------------------------------------------------
// 六、遥测账本
// ---------------------------------------------------------------------------

/// 优先级发射遥测（计数器全集——入 F0017 注册表的域）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PriorityTelemetry {
    /// 三档发射计数（下标=档位序数）。
    pub launched_by_tier: [u32; 3],
    /// 让位次数。
    pub preemptions: u32,
    /// 让位延迟样本（毫秒——P95 的原料）。
    pub yield_latencies: Vec<u32>,
    /// 老化升档次数。
    pub promotions: u32,
    /// 检查点总数（步长纪律的覆盖账）。
    pub checkpoints: u32,
}

impl PriorityTelemetry {
    /// 记一次发射。
    pub fn note_launch(&mut self, tier: Tier) {
        self.launched_by_tier[tier.rank() as usize] += 1;
    }

    /// 记一次让位（含延迟样本与检查点账）。
    pub fn note_preempt(&mut self, rep: &SliceReport) {
        if rep.preempted {
            self.preemptions += 1;
            self.yield_latencies.push(rep.yield_latency_ms);
        }
        self.checkpoints += rep.checkpoints;
    }

    /// 让位延迟 P95（原料即账——独立重算见判据）。
    pub fn yield_p95(&self) -> u32 {
        p95(&self.yield_latencies)
    }

    /// 遥测守恒：三档发射和+未发射数=总入队数（判据对账口径）。
    pub fn launched_total(&self) -> u32 {
        self.launched_by_tier.iter().sum()
    }

    /// 遥测读屏单行。
    pub fn screen_line(&self) -> String {
        format!(
            "优先级遥测：发射 {}（实时帧 {}/交互 {}/后台 {}），让位 {}，P95 {}ms，升档 {}",
            self.launched_total(),
            self.launched_by_tier[0],
            self.launched_by_tier[1],
            self.launched_by_tier[2],
            self.preemptions,
            self.yield_p95(),
            self.promotions
        )
    }
}

// ---------------------------------------------------------------------------
// 七、发射驱动（F0163 波次 → 本条队列的桥）
// ---------------------------------------------------------------------------

/// 把一个波次的就绪任务按 `priority` 映射三档入队。
///
/// `priorities` 与波内任务句柄一一对应（调用方从 F0161 NodeSpec 取）。
/// 返回入队数。
pub fn feed_wave(q: &mut LaunchQueue, wave: &[u32], priorities: &[u8]) -> usize {
    let mut n = 0usize;
    for (i, &h) in wave.iter().enumerate() {
        let pr = priorities.get(i).copied().unwrap_or(2);
        q.push(h, tier_of(pr));
        n += 1;
    }
    n
}
