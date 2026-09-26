//! m600perf — VARIX-M600 AI-05 性能观测域 (F101~F125)
//!
//! 全链路时间轴/火焰图直播/预算告警台/慢查询追踪器/帧成本显微镜/
//! 输入延迟示波器/唤醒风暴雷达/IO 黑匣子/网络路径解剖/合成器帧账本/
//! 功耗归因器/热节流叙事/指标超市/时间旅行调试/基线漂移检测/
//! 回归猎手/用户旅程计时器/资源家谱/采样守护者/剖面插桩开关/
//! 每域仪表公约/观测降噪器/巡检机器人/性能周报生成器/观测年报。
//!
//! 硬约束：no_std / 无 alloc / 无浮点（全部 permille/定点）/ 纯逻辑。

use crate::checks::CheckSet;

// ===========================================================================
// F101 — 全链路时间轴：跨子系统 span 进账
// ===========================================================================

pub const PERF_TIMELINE_CAP: usize = 8;

/// 一段 span：start <= end 由 `push` 守护。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PerfSpan {
    pub start_ns: u64,
    pub end_ns: u64,
}

#[derive(Clone, Copy, Debug)]
pub struct PerfTimeline {
    spans: [PerfSpan; PERF_TIMELINE_CAP],
    len: usize,
}

impl PerfTimeline {
    pub const fn new() -> PerfTimeline {
        PerfTimeline {
            spans: [PerfSpan { start_ns: 0, end_ns: 0 }; PERF_TIMELINE_CAP],
            len: 0,
        }
    }

    /// 记录一段 span。倒置区间或账满（≥ 8 段）拒绝并返回 false。
    pub fn push(&mut self, start_ns: u64, end_ns: u64) -> bool {
        if end_ns < start_ns || self.len >= PERF_TIMELINE_CAP {
            return false;
        }
        self.spans[self.len] = PerfSpan { start_ns, end_ns };
        self.len += 1;
        true
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn span_ns(&self, i: usize) -> u64 {
        if i < self.len {
            self.spans[i].end_ns - self.spans[i].start_ns
        } else {
            0
        }
    }

    pub fn total_ns(&self) -> u64 {
        let mut sum = 0u64;
        let mut i = 0usize;
        while i < self.len {
            sum += self.span_ns(i);
            i += 1;
        }
        sum
    }
}

// ===========================================================================
// F102 — 火焰图直播：调用栈深度直播，enter/exit 必须配平
// ===========================================================================

pub const PERF_FLAME_MAX_DEPTH: usize = 8;

#[derive(Clone, Copy, Debug)]
pub struct FlameStack {
    frames: [u32; PERF_FLAME_MAX_DEPTH],
    depth: usize,
}

impl FlameStack {
    pub const fn new() -> FlameStack {
        FlameStack { frames: [0; PERF_FLAME_MAX_DEPTH], depth: 0 }
    }

    /// 压入一帧。深度到顶（≥ 8）拒绝。
    pub fn enter(&mut self, frame_id: u32) -> bool {
        if self.depth >= PERF_FLAME_MAX_DEPTH {
            return false;
        }
        self.frames[self.depth] = frame_id;
        self.depth += 1;
        true
    }

    /// 弹出一帧。空栈返回 false。
    pub fn exit(&mut self) -> bool {
        if self.depth == 0 {
            return false;
        }
        self.depth -= 1;
        true
    }

    pub fn depth(&self) -> usize {
        self.depth
    }

    pub fn top(&self) -> Option<u32> {
        if self.depth == 0 {
            None
        } else {
            Some(self.frames[self.depth - 1])
        }
    }

    pub fn balanced(&self) -> bool {
        self.depth == 0
    }
}

// ===========================================================================
// F103 — 预算告警台：帧耗时越线三态
// ===========================================================================

pub const PERF_FRAME_BUDGET_US: u32 = 16666;
pub const PERF_BUDGET_WARN_PERMILLE: u32 = 800;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PerfBudgetState {
    Ok,
    Warn,
    Alert,
}

/// 80% 预算转 Warn，100% 及以上 Alert（交叉相乘避免截断）。
pub fn perf_budget_state(frame_us: u32) -> PerfBudgetState {
    if frame_us * 1000 >= PERF_FRAME_BUDGET_US * 1000 {
        PerfBudgetState::Alert
    } else if frame_us * 1000 >= PERF_FRAME_BUDGET_US * PERF_BUDGET_WARN_PERMILLE {
        PerfBudgetState::Warn
    } else {
        PerfBudgetState::Ok
    }
}

// ===========================================================================
// F104 — 慢查询追踪器：超阈查询入册，id 去重 + 容量上限
// ===========================================================================

pub const PERF_SLOW_QUERY_CAP: usize = 8;
pub const PERF_SLOW_QUERY_US: u32 = 100_000;

#[derive(Clone, Copy, Debug)]
pub struct SlowQueryLog {
    ids: [u32; PERF_SLOW_QUERY_CAP],
    costs: [u32; PERF_SLOW_QUERY_CAP],
    len: usize,
}

impl SlowQueryLog {
    pub const fn new() -> SlowQueryLog {
        SlowQueryLog {
            ids: [0; PERF_SLOW_QUERY_CAP],
            costs: [0; PERF_SLOW_QUERY_CAP],
            len: 0,
        }
    }

    /// 只有达到慢阈值的查询才入册；同 id 去重、账满拒绝，返回是否入册。
    pub fn record(&mut self, id: u32, cost_us: u32) -> bool {
        if cost_us < PERF_SLOW_QUERY_US || self.len >= PERF_SLOW_QUERY_CAP {
            return false;
        }
        let mut i = 0usize;
        while i < self.len {
            if self.ids[i] == id {
                return false;
            }
            i += 1;
        }
        self.ids[self.len] = id;
        self.costs[self.len] = cost_us;
        self.len += 1;
        true
    }

    pub fn len(&self) -> usize {
        self.len
    }

    /// 最慢一条的耗时（空表返回 0）。
    pub fn slowest_us(&self) -> u32 {
        let mut best = 0u32;
        let mut i = 0usize;
        while i < self.len {
            if self.costs[i] > best {
                best = self.costs[i];
            }
            i += 1;
        }
        best
    }
}

// ===========================================================================
// F105 — 帧成本显微镜：cpu + gpu + io = total 恒等式
// ===========================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PerfFrameCost {
    pub total_us: u32,
    pub cpu_us: u32,
    pub gpu_us: u32,
    pub io_us: u32,
}

impl PerfFrameCost {
    pub fn consistent(&self) -> bool {
        self.cpu_us + self.gpu_us + self.io_us == self.total_us
    }

    /// 最大成本分量（三分量手写比较，避免 Ord）。
    pub fn biggest_component_us(&self) -> u32 {
        let mut best = self.cpu_us;
        if self.gpu_us > best {
            best = self.gpu_us;
        }
        if self.io_us > best {
            best = self.io_us;
        }
        best
    }
}

// ===========================================================================
// F106 — 输入延迟示波器：采样定容，超预算占比 permille
// ===========================================================================

pub const PERF_LATENCY_SAMPLES: usize = 16;
pub const PERF_LATENCY_BUDGET_MS: u32 = 50;

#[derive(Clone, Copy, Debug)]
pub struct PerfLatencyScope {
    samples: [u32; PERF_LATENCY_SAMPLES],
    len: usize,
}

impl PerfLatencyScope {
    pub const fn new() -> PerfLatencyScope {
        PerfLatencyScope { samples: [0; PERF_LATENCY_SAMPLES], len: 0 }
    }

    /// 采一个样。样本满（≥ 16）拒绝。
    pub fn record(&mut self, latency_ms: u32) -> bool {
        if self.len >= PERF_LATENCY_SAMPLES {
            return false;
        }
        self.samples[self.len] = latency_ms;
        self.len += 1;
        true
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn max_ms(&self) -> u32 {
        let mut best = 0u32;
        let mut i = 0usize;
        while i < self.len {
            if self.samples[i] > best {
                best = self.samples[i];
            }
            i += 1;
        }
        best
    }

    /// 达到/超过预算的样本占比（‰）。空表返回 0。
    pub fn over_budget_permille(&self) -> u32 {
        if self.len == 0 {
            return 0;
        }
        let mut over = 0u32;
        let mut i = 0usize;
        while i < self.len {
            if self.samples[i] >= PERF_LATENCY_BUDGET_MS {
                over += 1;
            }
            i += 1;
        }
        over * 1000 / self.len as u32
    }
}

// ===========================================================================
// F107 — 唤醒风暴雷达：每秒唤醒次数超阈即风暴
// ===========================================================================

pub const PERF_WAKE_STORM_PER_SEC: u32 = 200;
pub const PERF_WAKE_MIN_WINDOW_MS: u32 = 100;

#[derive(Clone, Copy, Debug, Default)]
pub struct PerfWakeRadar {
    pub wakes: u32,
    pub window_ms: u32,
}

/// 观察窗足够长（≥ 100ms）且折算每秒唤醒 ≥ 200 次 → 风暴。
pub fn perf_wake_storm(r: &PerfWakeRadar) -> bool {
    r.window_ms >= PERF_WAKE_MIN_WINDOW_MS && r.wakes * 1000 >= r.window_ms * PERF_WAKE_STORM_PER_SEC
}

// ===========================================================================
// F108 — IO 黑匣子：定容环形事件流，覆盖旧事件必须记账
// ===========================================================================

pub const PERF_BLACKBOX_CAP: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PerfIoKind {
    Read,
    Write,
    Sync,
}

#[derive(Clone, Copy, Debug)]
pub struct PerfIoBlackBox {
    seqs: [u64; PERF_BLACKBOX_CAP],
    kinds: [PerfIoKind; PERF_BLACKBOX_CAP],
    latencies: [u32; PERF_BLACKBOX_CAP],
    head: usize,
    len: usize,
    pub overwritten: u64,
}

impl PerfIoBlackBox {
    pub const fn new() -> PerfIoBlackBox {
        PerfIoBlackBox {
            seqs: [0; PERF_BLACKBOX_CAP],
            kinds: [PerfIoKind::Read; PERF_BLACKBOX_CAP],
            latencies: [0; PERF_BLACKBOX_CAP],
            head: 0,
            len: 0,
            overwritten: 0,
        }
    }

    /// 写入一条事件；环满后覆盖最旧事件并累计 overwritten。
    pub fn push(&mut self, seq: u64, kind: PerfIoKind, latency_us: u32) {
        if self.len < PERF_BLACKBOX_CAP {
            self.len += 1;
        } else {
            self.overwritten += 1;
        }
        self.seqs[self.head] = seq;
        self.kinds[self.head] = kind;
        self.latencies[self.head] = latency_us;
        self.head = (self.head + 1) % PERF_BLACKBOX_CAP;
    }

    pub fn kind_at(&self, i: usize) -> PerfIoKind {
        self.kinds[i % PERF_BLACKBOX_CAP]
    }

    pub fn seq_at(&self, i: usize) -> u64 {
        self.seqs[i % PERF_BLACKBOX_CAP]
    }

    pub fn latency_at(&self, i: usize) -> u32 {
        self.latencies[i % PERF_BLACKBOX_CAP]
    }
}

// ===========================================================================
// F109 — 网络路径解剖：逐跳 RTT 账本，定位最差一跳
// ===========================================================================

pub const PERF_NET_MAX_HOPS: usize = 6;

#[derive(Clone, Copy, Debug)]
pub struct PerfNetPath {
    rtts: [u32; PERF_NET_MAX_HOPS],
    len: usize,
}

impl PerfNetPath {
    pub const fn new() -> PerfNetPath {
        PerfNetPath { rtts: [0; PERF_NET_MAX_HOPS], len: 0 }
    }

    /// 加一跳。满（≥ 6 跳）拒绝。
    pub fn add_hop(&mut self, rtt_us: u32) -> bool {
        if self.len >= PERF_NET_MAX_HOPS {
            return false;
        }
        self.rtts[self.len] = rtt_us;
        self.len += 1;
        true
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn total_rtt_us(&self) -> u32 {
        let mut sum = 0u32;
        let mut i = 0usize;
        while i < self.len {
            sum += self.rtts[i];
            i += 1;
        }
        sum
    }

    pub fn worst_hop_us(&self) -> u32 {
        let mut best = 0u32;
        let mut i = 0usize;
        while i < self.len {
            if self.rtts[i] > best {
                best = self.rtts[i];
            }
            i += 1;
        }
        best
    }

    /// 最差一跳占总 RTT 的比例（‰）。总 RTT 为 0 返回 0。
    pub fn worst_share_permille(&self) -> u32 {
        let total = self.total_rtt_us();
        if total == 0 {
            0
        } else {
            self.worst_hop_us() * 1000 / total
        }
    }
}

// ===========================================================================
// F110 — 合成器帧账本：呈现 vs 错帧，总账有上限
// ===========================================================================

pub const PERF_FRAME_LEDGER_CAP: u32 = 4096;

#[derive(Clone, Copy, Debug, Default)]
pub struct PerfFrameLedger {
    pub presented: u32,
    pub missed: u32,
}

impl PerfFrameLedger {
    /// 记一帧呈现。总账满（≥ 4096）拒绝。
    pub fn record_presented(&mut self) -> bool {
        if self.presented + self.missed >= PERF_FRAME_LEDGER_CAP {
            return false;
        }
        self.presented += 1;
        true
    }

    /// 记一帧错失。总账满（≥ 4096）拒绝。
    pub fn record_missed(&mut self) -> bool {
        if self.presented + self.missed >= PERF_FRAME_LEDGER_CAP {
            return false;
        }
        self.missed += 1;
        true
    }

    pub fn total(&self) -> u32 {
        self.presented + self.missed
    }

    /// 错帧率（‰）。空账返回 0。
    pub fn miss_permille(&self) -> u32 {
        let total = self.total();
        if total == 0 {
            0
        } else {
            self.missed * 1000 / total
        }
    }
}

// ===========================================================================
// F111 — 功耗归因器：各来源 permille 相加必须恰为 1000，id 去重
// ===========================================================================

pub const PERF_ATTR_CAP: usize = 8;
pub const PERF_ATTR_TOTAL_PERMILLE: u32 = 1000;

#[derive(Clone, Copy, Debug)]
pub struct PerfAttribution {
    ids: [u32; PERF_ATTR_CAP],
    permilles: [u32; PERF_ATTR_CAP],
    len: usize,
}

impl PerfAttribution {
    pub const fn new() -> PerfAttribution {
        PerfAttribution {
            ids: [0; PERF_ATTR_CAP],
            permilles: [0; PERF_ATTR_CAP],
            len: 0,
        }
    }

    /// 归因一个来源。同 id 去重、份额溢出（合计 > 1000‰）或账满拒绝。
    pub fn add(&mut self, id: u32, permille: u32) -> bool {
        if permille > PERF_ATTR_TOTAL_PERMILLE || self.len >= PERF_ATTR_CAP {
            return false;
        }
        let mut sum = permille;
        let mut i = 0usize;
        while i < self.len {
            if self.ids[i] == id {
                return false;
            }
            sum += self.permilles[i];
            i += 1;
        }
        if sum > PERF_ATTR_TOTAL_PERMILLE {
            return false;
        }
        self.ids[self.len] = id;
        self.permilles[self.len] = permille;
        self.len += 1;
        true
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn total_permille(&self) -> u32 {
        let mut sum = 0u32;
        let mut i = 0usize;
        while i < self.len {
            sum += self.permilles[i];
            i += 1;
        }
        sum
    }

    /// 归因完备：合计恰好 1000‰。
    pub fn complete(&self) -> bool {
        self.total_permille() == PERF_ATTR_TOTAL_PERMILLE
    }
}

// ===========================================================================
// F112 — 热节流叙事：节流等级 → 频率 permille 阶梯
// ===========================================================================

pub const PERF_THROTTLE_LEVELS: u32 = 5;
pub const PERF_THROTTLE_FREQ_STEP: u32 = 150;
pub const PERF_THROTTLE_FREQ_FLOOR: u32 = 400;

/// level 0 → 1000‰，每级降 150‰，4 级封底 400‰；超界等级同样封底。
pub fn perf_throttle_freq_permille(level: u32) -> u32 {
    if level >= PERF_THROTTLE_LEVELS {
        return PERF_THROTTLE_FREQ_FLOOR;
    }
    let cut = level * PERF_THROTTLE_FREQ_STEP;
    let freq = 1000 - cut;
    if freq < PERF_THROTTLE_FREQ_FLOOR {
        PERF_THROTTLE_FREQ_FLOOR
    } else {
        freq
    }
}

// ===========================================================================
// F113 — 指标超市：指标发布 id 去重（覆盖更新）+ 容量上限
// ===========================================================================

pub const PERF_METRIC_CAP: usize = 16;

#[derive(Clone, Copy, Debug)]
pub struct PerfMetricMarket {
    ids: [u32; PERF_METRIC_CAP],
    values: [u64; PERF_METRIC_CAP],
    len: usize,
}

impl PerfMetricMarket {
    pub const fn new() -> PerfMetricMarket {
        PerfMetricMarket {
            ids: [0; PERF_METRIC_CAP],
            values: [0; PERF_METRIC_CAP],
            len: 0,
        }
    }

    /// 发布一个指标：已存在同 id 则覆盖更新（去重），否则占新槽；槽满拒绝。
    pub fn publish(&mut self, id: u32, value: u64) -> bool {
        let mut i = 0usize;
        while i < self.len {
            if self.ids[i] == id {
                self.values[i] = value;
                return true;
            }
            i += 1;
        }
        if self.len >= PERF_METRIC_CAP {
            return false;
        }
        self.ids[self.len] = id;
        self.values[self.len] = value;
        self.len += 1;
        true
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn lookup(&self, id: u32) -> Option<u64> {
        let mut i = 0usize;
        while i < self.len {
            if self.ids[i] == id {
                return Some(self.values[i]);
            }
            i += 1;
        }
        None
    }
}

// ===========================================================================
// F114 — 时间旅行调试：事件日志严格递增，可回放到任意历史序号
// ===========================================================================

pub const PERF_TT_CAP: usize = 8;

#[derive(Clone, Copy, Debug)]
pub struct PerfTimeTravel {
    seqs: [u64; PERF_TT_CAP],
    states: [u32; PERF_TT_CAP],
    len: usize,
}

impl PerfTimeTravel {
    pub const fn new() -> PerfTimeTravel {
        PerfTimeTravel {
            seqs: [0; PERF_TT_CAP],
            states: [0; PERF_TT_CAP],
            len: 0,
        }
    }

    /// 追加一条状态。seq 必须严格大于上一条；账满拒绝。
    pub fn record(&mut self, seq: u64, state: u32) -> bool {
        if self.len >= PERF_TT_CAP {
            return false;
        }
        if self.len > 0 && seq <= self.seqs[self.len - 1] {
            return false;
        }
        self.seqs[self.len] = seq;
        self.states[self.len] = state;
        self.len += 1;
        true
    }

    pub fn len(&self) -> usize {
        self.len
    }

    /// 该序号是否存在于日志（能否回放）。
    pub fn rewind_ok(&self, seq: u64) -> bool {
        let mut i = 0usize;
        while i < self.len {
            if self.seqs[i] == seq {
                return true;
            }
            i += 1;
        }
        false
    }

    pub fn state_at(&self, seq: u64) -> Option<u32> {
        let mut i = 0usize;
        while i < self.len {
            if self.seqs[i] == seq {
                return Some(self.states[i]);
            }
            i += 1;
        }
        None
    }
}

// ===========================================================================
// F115 — 基线漂移检测：相对基线的偏离 permille
// ===========================================================================

pub const PERF_DRIFT_ALERT_PERMILLE: u32 = 150;

/// 双向偏离（|current - baseline| / baseline，‰）。基线为 0 无法归一，返回 0。
pub fn perf_drift_permille(baseline: u32, current: u32) -> u32 {
    if baseline == 0 {
        return 0;
    }
    if current >= baseline {
        (current - baseline) * 1000 / baseline
    } else {
        (baseline - current) * 1000 / baseline
    }
}

pub fn perf_drifted(baseline: u32, current: u32) -> bool {
    baseline != 0 && perf_drift_permille(baseline, current) >= PERF_DRIFT_ALERT_PERMILLE
}

// ===========================================================================
// F116 — 回归猎手：样本足够且劣化 ≥ 10% 即回归
// ===========================================================================

pub const PERF_REGRESSION_PERMILLE: u32 = 100;
pub const PERF_REGRESSION_MIN_SAMPLES: u32 = 30;

pub fn perf_regression(baseline: u32, current: u32, samples: u32) -> bool {
    samples >= PERF_REGRESSION_MIN_SAMPLES && current * 1000 >= baseline * (1000 + PERF_REGRESSION_PERMILLE)
}

// ===========================================================================
// F117 — 用户旅程计时器：阶段里程碑严格递增
// ===========================================================================

pub const PERF_JOURNEY_STAGES: usize = 4;

#[derive(Clone, Copy, Debug)]
pub struct PerfJourney {
    ends_us: [u64; PERF_JOURNEY_STAGES],
    len: usize,
}

impl PerfJourney {
    pub const fn new() -> PerfJourney {
        PerfJourney { ends_us: [0; PERF_JOURNEY_STAGES], len: 0 }
    }

    /// 打点一个阶段终点。必须严格大于上一打点；阶段满（≥ 4）拒绝。
    pub fn mark_stage(&mut self, end_us: u64) -> bool {
        if self.len >= PERF_JOURNEY_STAGES {
            return false;
        }
        if self.len > 0 && end_us <= self.ends_us[self.len - 1] {
            return false;
        }
        self.ends_us[self.len] = end_us;
        self.len += 1;
        true
    }

    pub fn len(&self) -> usize {
        self.len
    }

    /// 旅程总耗时 = 最后一个里程碑（空旅程返回 0）。
    pub fn total_us(&self) -> u64 {
        if self.len == 0 {
            0
        } else {
            self.ends_us[self.len - 1]
        }
    }
}

// ===========================================================================
// F118 — 资源家谱：parent 指向合法、不得自指、深度受限
// ===========================================================================

pub const PERF_GENEALOGY_MAX_DEPTH: u32 = 6;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PerfResource {
    pub id: u32,
    pub parent: u32,
    pub depth: u32,
}

/// id 0 保留；parent 0 表示根；不得自指；深度不得超过上限。
pub fn perf_lineage_ok(r: PerfResource) -> bool {
    if r.id == 0 {
        return false;
    }
    if r.parent != 0 && r.parent == r.id {
        return false;
    }
    if r.parent != 0 && r.depth == 0 {
        return false;
    }
    r.depth <= PERF_GENEALOGY_MAX_DEPTH
}

// ===========================================================================
// F119 — 采样守护者：采样间隔必须落在 [10ms, 1000ms]
// ===========================================================================

pub const PERF_SAMPLE_MIN_MS: u32 = 10;
pub const PERF_SAMPLE_MAX_MS: u32 = 1000;

/// 非法间隔夹回边界。
pub fn perf_clamp_interval(ms: u32) -> u32 {
    if ms < PERF_SAMPLE_MIN_MS {
        PERF_SAMPLE_MIN_MS
    } else if ms > PERF_SAMPLE_MAX_MS {
        PERF_SAMPLE_MAX_MS
    } else {
        ms
    }
}

pub fn perf_interval_ok(ms: u32) -> bool {
    ms >= PERF_SAMPLE_MIN_MS && ms <= PERF_SAMPLE_MAX_MS
}

// ===========================================================================
// F120 — 剖面插桩开关：按子系统位图开关
// ===========================================================================

pub const PERF_PROFILE_SUBSYSTEMS: u32 = 8;

#[derive(Clone, Copy, Debug, Default)]
pub struct PerfProfileMask {
    pub bits: u32,
}

impl PerfProfileMask {
    /// 打开一个子系统的插桩。子系统号越界（≥ 8）拒绝。
    pub fn enable(&mut self, subsystem: u32) -> bool {
        if subsystem >= PERF_PROFILE_SUBSYSTEMS {
            return false;
        }
        self.bits |= 1u32 << subsystem;
        true
    }

    /// 关闭一个子系统的插桩。越界拒绝。
    pub fn disable(&mut self, subsystem: u32) -> bool {
        if subsystem >= PERF_PROFILE_SUBSYSTEMS {
            return false;
        }
        self.bits &= !(1u32 << subsystem);
        true
    }

    pub fn is_enabled(&self, subsystem: u32) -> bool {
        subsystem < PERF_PROFILE_SUBSYSTEMS && (self.bits & (1u32 << subsystem)) != 0
    }

    pub fn enabled_count(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0u32;
        while i < PERF_PROFILE_SUBSYSTEMS {
            if (self.bits & (1u32 << i)) != 0 {
                n += 1;
            }
            i += 1;
        }
        n
    }
}

// ===========================================================================
// F121 — 每域仪表公约：每域必须上报同一组仪表（4 项全覆盖）
// ===========================================================================

pub const PERF_GAUGE_NAMES: [&str; 4] = ["fps", "latency", "cpu", "mem"];
pub const PERF_GAUGE_MASK_FULL: u32 = 0b1111;

/// 仪表覆盖位图必须恰好等于公约全集（多报少报都不合格）。
pub fn perf_gauge_mask_complete(mask: u32) -> bool {
    mask == PERF_GAUGE_MASK_FULL
}

pub fn perf_convention_ok(gauges_reported: u32) -> bool {
    gauges_reported == PERF_GAUGE_NAMES.len() as u32
}

// ===========================================================================
// F122 — 观测降噪器：连续相同读数抑制
// ===========================================================================

#[derive(Clone, Copy, Debug, Default)]
pub struct PerfNoiseReducer {
    last: Option<u32>,
    pub accepted: u64,
    pub suppressed: u64,
}

impl PerfNoiseReducer {
    /// 喂入一个读数。与上一读数相同则抑制（返回 false），否则放行。
    pub fn feed(&mut self, value: u32) -> bool {
        match self.last {
            Some(prev) if prev == value => {
                self.suppressed += 1;
                false
            }
            _ => {
                self.accepted += 1;
                self.last = Some(value);
                true
            }
        }
    }
}

// ===========================================================================
// F123 — 巡检机器人：轮次记账 + 问题清单（id 去重 + 容量上限）
// ===========================================================================

pub const PERF_PATROL_ISSUE_CAP: usize = 8;

#[derive(Clone, Copy, Debug)]
pub struct PerfPatrolBot {
    pub rounds: u32,
    issues: [u32; PERF_PATROL_ISSUE_CAP],
    issue_len: usize,
}

impl PerfPatrolBot {
    pub const fn new() -> PerfPatrolBot {
        PerfPatrolBot {
            rounds: 0,
            issues: [0; PERF_PATROL_ISSUE_CAP],
            issue_len: 0,
        }
    }

    pub fn run_round(&mut self) -> u32 {
        self.rounds += 1;
        self.rounds
    }

    /// 登记一个问题。同 id 去重、清单满（≥ 8）拒绝。
    pub fn record_issue(&mut self, id: u32) -> bool {
        let mut i = 0usize;
        while i < self.issue_len {
            if self.issues[i] == id {
                return false;
            }
            i += 1;
        }
        if self.issue_len >= PERF_PATROL_ISSUE_CAP {
            return false;
        }
        self.issues[self.issue_len] = id;
        self.issue_len += 1;
        true
    }

    /// 结案一个问题（原地压缩）。不存在返回 false。
    pub fn resolve_issue(&mut self, id: u32) -> bool {
        let mut i = 0usize;
        while i < self.issue_len {
            if self.issues[i] == id {
                let mut w = i;
                while w + 1 < self.issue_len {
                    self.issues[w] = self.issues[w + 1];
                    w += 1;
                }
                self.issue_len -= 1;
                return true;
            }
            i += 1;
        }
        false
    }

    pub fn open_issues(&self) -> usize {
        self.issue_len
    }
}

// ===========================================================================
// F124 — 性能周报生成器：周报章节完备性
// ===========================================================================

pub const PERF_WEEKLY_SECTIONS: [&str; 5] =
    ["timeline", "budget", "regressions", "patrol", "attribution"];

pub fn perf_weekly_complete(sections_filled: u32) -> bool {
    sections_filled >= PERF_WEEKLY_SECTIONS.len() as u32
}

// ===========================================================================
// F125 — 观测年报：全年 52 周覆盖 + 周报链完备
// ===========================================================================

pub const PERF_ANNUAL_WEEKS: u32 = 52;

pub fn perf_annual_complete(weeks_covered: u32, weekly_chain_ok: bool) -> bool {
    weekly_chain_ok && weeks_covered == PERF_ANNUAL_WEEKS
}

// ===========================================================================
// 域自检
// ===========================================================================

pub fn run_m600perf_checks() -> CheckSet {
    let mut set = CheckSet::new("m600perf");

    // F101 全链路时间轴
    let mut tl = PerfTimeline::new();
    let first = tl.push(10, 20);
    let len_after_first = tl.len();
    let second = tl.push(20, 35);
    set.add(
        "F101 timeline record",
        first && len_after_first == 1 && second && tl.len() == 2 && tl.total_ns() == 25,
        "spans sum",
    );
    let inverted = tl.push(40, 39);
    let len_after_bad = tl.len();
    set.add("F101 timeline rejects inverted", !inverted && len_after_bad == 2, "end < start");

    // F102 火焰图直播
    let mut flame = FlameStack::new();
    let e1 = flame.enter(100);
    let e2 = flame.enter(200);
    let depth_mid = flame.depth();
    let top_mid = flame.top();
    let x1 = flame.exit();
    let x2 = flame.exit();
    let x3 = flame.exit();
    set.add(
        "F102 flame stack",
        e1 && e2 && depth_mid == 2 && top_mid == Some(200) && x1 && x2 && !x3 && flame.balanced(),
        "enter/exit paired",
    );
    let mut deep = FlameStack::new();
    let mut d = 0usize;
    while d < PERF_FLAME_MAX_DEPTH {
        deep.enter(d as u32);
        d += 1;
    }
    let over = deep.enter(99);
    set.add("F102 flame capped", !over && deep.depth() == PERF_FLAME_MAX_DEPTH, "depth cap");

    // F103 预算告警台
    set.add(
        "F103 budget states",
        perf_budget_state(8000) == PerfBudgetState::Ok
            && perf_budget_state(14000) == PerfBudgetState::Warn
            && perf_budget_state(17000) == PerfBudgetState::Alert,
        "ok/warn/alert",
    );
    set.add("F103 budget edge", perf_budget_state(PERF_FRAME_BUDGET_US) == PerfBudgetState::Alert, "at budget");

    // F104 慢查询追踪器
    let mut slow = SlowQueryLog::new();
    let r1 = slow.record(7, 150_000);
    let dup = slow.record(7, 200_000);
    let fast = slow.record(9, 50_000);
    let r2 = slow.record(11, 120_000);
    set.add(
        "F104 slow query log",
        r1 && !dup && !fast && r2 && slow.len() == 2 && slow.slowest_us() == 150_000,
        "threshold + dedup",
    );
    let mut capped = SlowQueryLog::new();
    let mut c = 0u32;
    while c < PERF_SLOW_QUERY_CAP as u32 {
        capped.record(100 + c, PERF_SLOW_QUERY_US);
        c += 1;
    }
    let over = capped.record(999, PERF_SLOW_QUERY_US);
    set.add("F104 slow query capped", !over && capped.len() == PERF_SLOW_QUERY_CAP, "cap rejects");

    // F105 帧成本显微镜
    let cost = PerfFrameCost { total_us: 100, cpu_us: 60, gpu_us: 30, io_us: 10 };
    let broken = PerfFrameCost { total_us: 100, cpu_us: 60, gpu_us: 30, io_us: 20 };
    set.add(
        "F105 frame cost identity",
        cost.consistent() && !broken.consistent() && cost.biggest_component_us() == 60,
        "cpu+gpu+io=total",
    );

    // F106 输入延迟示波器
    let mut scope = PerfLatencyScope::new();
    scope.record(40);
    scope.record(60);
    scope.record(30);
    scope.record(70);
    let scope_max = scope.max_ms();
    let scope_over = scope.over_budget_permille();
    set.add("F106 latency scope", scope.len() == 4 && scope_max == 70 && scope_over == 500, "2/4 over budget");

    // F107 唤醒风暴雷达
    set.add(
        "F107 wake storm",
        perf_wake_storm(&PerfWakeRadar { wakes: 25, window_ms: 100 })
            && !perf_wake_storm(&PerfWakeRadar { wakes: 10, window_ms: 100 })
            && !perf_wake_storm(&PerfWakeRadar { wakes: 25, window_ms: 50 }),
        "rate + window guard",
    );

    // F108 IO 黑匣子
    let mut bb = PerfIoBlackBox::new();
    bb.push(1, PerfIoKind::Read, 100);
    bb.push(2, PerfIoKind::Write, 200);
    bb.push(3, PerfIoKind::Sync, 300);
    let bb_kind2 = bb.kind_at(1);
    let bb_lat3 = bb.latency_at(2);
    set.add(
        "F108 blackbox record",
        bb.seq_at(0) == 1 && bb_kind2 == PerfIoKind::Write && bb_lat3 == 300 && bb.overwritten == 0,
        "three events kept",
    );
    let mut wrap = PerfIoBlackBox::new();
    let mut s = 1u64;
    while s <= 9 {
        wrap.push(s, PerfIoKind::Read, 10);
        s += 1;
    }
    let wrap_seq8 = wrap.seq_at(7);
    let wrap_seq0 = wrap.seq_at(8);
    set.add(
        "F108 blackbox wrap",
        wrap.overwritten == 1 && wrap_seq8 == 8 && wrap_seq0 == 9,
        "oldest overwritten once",
    );

    // F109 网络路径解剖
    let mut path = PerfNetPath::new();
    path.add_hop(100);
    path.add_hop(250);
    path.add_hop(50);
    let path_total = path.total_rtt_us();
    let path_worst = path.worst_hop_us();
    let path_share = path.worst_share_permille();
    set.add(
        "F109 net path",
        path.len() == 3 && path_total == 400 && path_worst == 250 && path_share == 625,
        "worst hop share",
    );
    let mut long_path = PerfNetPath::new();
    let mut h = 0usize;
    while h < PERF_NET_MAX_HOPS {
        long_path.add_hop(10);
        h += 1;
    }
    let hop_over = long_path.add_hop(10);
    set.add("F109 net path capped", !hop_over && long_path.len() == PERF_NET_MAX_HOPS, "hop cap");

    // F110 合成器帧账本
    let mut fl = PerfFrameLedger::default();
    let mut p = 0u32;
    while p < 90 {
        fl.record_presented();
        p += 1;
    }
    let mut m = 0u32;
    while m < 10 {
        fl.record_missed();
        m += 1;
    }
    let fl_total = fl.total();
    let fl_miss = fl.miss_permille();
    set.add("F110 frame ledger", fl_total == 100 && fl_miss == 100, "10% missed");

    // F111 功耗归因器
    let mut attr = PerfAttribution::new();
    let a1 = attr.add(1, 400);
    let dup = attr.add(1, 100);
    let overflow = attr.add(2, 700);
    let a2 = attr.add(2, 350);
    let a3 = attr.add(3, 250);
    let over_after_full = attr.add(4, 1);
    set.add(
        "F111 attribution rules",
        a1 && !dup && !overflow && a2 && a3 && !over_after_full && attr.complete(),
        "dedup + sum 1000",
    );

    // F112 热节流叙事
    set.add(
        "F112 throttle ladder",
        perf_throttle_freq_permille(0) == 1000
            && perf_throttle_freq_permille(2) == 700
            && perf_throttle_freq_permille(4) == 400
            && perf_throttle_freq_permille(9) == 400,
        "level to freq",
    );
    set.add(
        "F112 throttle monotonic",
        perf_throttle_freq_permille(1) > perf_throttle_freq_permille(2),
        "hotter means slower",
    );

    // F113 指标超市
    let mut market = PerfMetricMarket::new();
    let p1 = market.publish(1, 100);
    let p1_update = market.publish(1, 200);
    let len_after_update = market.len();
    let v1 = market.lookup(1);
    let p2 = market.publish(2, 50);
    set.add(
        "F113 metric market",
        p1 && p1_update && len_after_update == 1 && v1 == Some(200) && p2 && market.len() == 2,
        "dedup updates in place",
    );
    let mut full_market = PerfMetricMarket::new();
    let mut mi = 0u32;
    while mi < PERF_METRIC_CAP as u32 {
        full_market.publish(mi, mi as u64);
        mi += 1;
    }
    let market_over = full_market.publish(999, 1);
    set.add("F113 market capped", !market_over && full_market.len() == PERF_METRIC_CAP, "cap rejects");

    // F114 时间旅行调试
    let mut tt = PerfTimeTravel::new();
    let t1 = tt.record(1, 100);
    let t_dup = tt.record(1, 150);
    let t2 = tt.record(3, 200);
    let t_back = tt.record(2, 250);
    let rw1 = tt.rewind_ok(1);
    let rw3 = tt.rewind_ok(3);
    let rw2 = tt.rewind_ok(2);
    let st3 = tt.state_at(3);
    set.add(
        "F114 time travel",
        t1 && !t_dup && t2 && !t_back && rw1 && rw3 && !rw2 && st3 == Some(200),
        "strict seq + rewind",
    );

    // F115 基线漂移检测
    set.add(
        "F115 drift math",
        perf_drift_permille(100, 120) == 200 && perf_drift_permille(120, 100) == 166,
        "both directions",
    );
    set.add(
        "F115 drift alert",
        perf_drifted(100, 120) && !perf_drifted(100, 110) && !perf_drifted(0, 5),
        "threshold + zero baseline",
    );

    // F116 回归猎手
    set.add(
        "F116 regression",
        perf_regression(100, 115, 30)
            && !perf_regression(100, 105, 30)
            && !perf_regression(100, 115, 10),
        "10% worse + samples",
    );

    // F117 用户旅程计时器
    let mut journey = PerfJourney::new();
    let j1 = journey.mark_stage(100);
    let j_flat = journey.mark_stage(100);
    let j2 = journey.mark_stage(300);
    let journey_total = journey.total_us();
    set.add(
        "F117 journey",
        j1 && !j_flat && j2 && journey.len() == 2 && journey_total == 300,
        "milestones increase",
    );

    // F118 资源家谱
    set.add(
        "F118 lineage rules",
        perf_lineage_ok(PerfResource { id: 3, parent: 0, depth: 0 })
            && perf_lineage_ok(PerfResource { id: 5, parent: 3, depth: 2 })
            && !perf_lineage_ok(PerfResource { id: 7, parent: 7, depth: 1 })
            && !perf_lineage_ok(PerfResource { id: 8, parent: 2, depth: 7 })
            && !perf_lineage_ok(PerfResource { id: 0, parent: 0, depth: 0 }),
        "root/child/self/deep/id0",
    );

    // F119 采样守护者
    set.add(
        "F119 sampling clamp",
        perf_clamp_interval(5) == 10 && perf_clamp_interval(500) == 500 && perf_clamp_interval(2000) == 1000,
        "clamped to window",
    );
    set.add(
        "F119 sampling ok",
        perf_interval_ok(10) && perf_interval_ok(1000) && !perf_interval_ok(9) && !perf_interval_ok(1001),
        "bounds inclusive",
    );

    // F120 剖面插桩开关
    let mut mask = PerfProfileMask::default();
    let en0 = mask.enable(0);
    let en7 = mask.enable(7);
    let en_bad = mask.enable(8);
    let count_two = mask.enabled_count();
    let has7 = mask.is_enabled(7);
    mask.disable(0);
    let count_after = mask.enabled_count();
    set.add(
        "F120 profile mask",
        en0 && en7 && !en_bad && count_two == 2 && has7 && count_after == 1,
        "toggle + bounds",
    );

    // F121 每域仪表公约
    set.add(
        "F121 gauge convention",
        PERF_GAUGE_NAMES.len() == 4
            && perf_gauge_mask_complete(0b1111)
            && !perf_gauge_mask_complete(0b0111)
            && !perf_gauge_mask_complete(0b10001)
            && perf_convention_ok(4)
            && !perf_convention_ok(3),
        "exact coverage",
    );

    // F122 观测降噪器
    let mut nr = PerfNoiseReducer::default();
    let f1 = nr.feed(5);
    let f2 = nr.feed(5);
    let f3 = nr.feed(6);
    set.add(
        "F122 noise reducer",
        f1 && !f2 && f3 && nr.accepted == 2 && nr.suppressed == 1,
        "duplicate suppressed",
    );

    // F123 巡检机器人
    let mut bot = PerfPatrolBot::new();
    let round1 = bot.run_round();
    let i1 = bot.record_issue(11);
    let i_dup = bot.record_issue(11);
    let i2 = bot.record_issue(12);
    let open_two = bot.open_issues();
    let res = bot.resolve_issue(11);
    let res_again = bot.resolve_issue(11);
    set.add(
        "F123 patrol bot",
        round1 == 1 && i1 && !i_dup && i2 && open_two == 2 && res && !res_again && bot.open_issues() == 1,
        "dedup + resolve",
    );
    let mut full_bot = PerfPatrolBot::new();
    let mut bi = 0u32;
    while bi < PERF_PATROL_ISSUE_CAP as u32 {
        full_bot.record_issue(100 + bi);
        bi += 1;
    }
    let bot_over = full_bot.record_issue(999);
    set.add("F123 patrol capped", !bot_over && full_bot.open_issues() == PERF_PATROL_ISSUE_CAP, "cap rejects");

    // F124 性能周报生成器
    set.add(
        "F124 weekly report",
        PERF_WEEKLY_SECTIONS.len() == 5 && perf_weekly_complete(5) && !perf_weekly_complete(4),
        "sections complete",
    );

    // F125 观测年报
    set.add(
        "F125 annual report",
        perf_annual_complete(52, true) && !perf_annual_complete(51, true) && !perf_annual_complete(52, false),
        "52 weeks + chain",
    );

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f101_timeline_math() {
        let mut tl = PerfTimeline::new();
        assert!(tl.push(0, 100));
        assert!(tl.push(100, 250));
        assert_eq!(tl.span_ns(0), 100);
        assert_eq!(tl.span_ns(1), 150);
        assert_eq!(tl.total_ns(), 250);
        assert!(!tl.push(300, 299));
        assert_eq!(tl.len(), 2);
    }

    #[test]
    fn f104_slow_query_dedup_cap() {
        let mut log = SlowQueryLog::new();
        assert!(log.record(1, 200_000));
        assert!(!log.record(1, 300_000)); // id 去重
        assert!(!log.record(2, 99_999)); // 未达阈值
        let mut i = 0u32;
        while log.len() < PERF_SLOW_QUERY_CAP {
            assert!(log.record(10 + i, PERF_SLOW_QUERY_US));
            i += 1;
        }
        assert!(!log.record(999, PERF_SLOW_QUERY_US)); // 容量上限
        assert_eq!(log.len(), PERF_SLOW_QUERY_CAP);
    }

    #[test]
    fn f110_ledger_never_overfills() {
        let mut fl = PerfFrameLedger::default();
        let mut i = 0u32;
        while i < PERF_FRAME_LEDGER_CAP {
            assert!(fl.record_presented());
            i += 1;
        }
        assert!(!fl.record_presented());
        assert!(!fl.record_missed());
        assert_eq!(fl.total(), PERF_FRAME_LEDGER_CAP);
        assert_eq!(fl.miss_permille(), 0);
    }

    #[test]
    fn f113_market_dedup_updates_in_place() {
        let mut m = PerfMetricMarket::new();
        assert!(m.publish(5, 1));
        assert!(m.publish(5, 2));
        assert!(m.publish(6, 3));
        assert_eq!(m.len(), 2);
        assert_eq!(m.lookup(5), Some(2));
        assert_eq!(m.lookup(6), Some(3));
        assert_eq!(m.lookup(7), None);
    }

    #[test]
    fn f122_noise_suppression_counts() {
        let mut nr = PerfNoiseReducer::default();
        assert!(nr.feed(1));
        assert!(nr.feed(2));
        assert!(!nr.feed(2));
        assert!(!nr.feed(2));
        assert!(nr.feed(3));
        assert_eq!(nr.accepted, 3);
        assert_eq!(nr.suppressed, 2);
    }

    #[test]
    fn f125_perf_selfcheck_all_pass() {
        let set = run_m600perf_checks();
        assert!(set.len() >= 25, "got {}", set.len());
        assert!(set.all_passed());
    }
}
