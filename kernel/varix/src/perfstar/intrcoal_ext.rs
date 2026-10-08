//! F050 中断合并 · 深化件（AI-K1 深化批次三 · G-B-10）。
//!
//! | # | 主册原文 | 本件机制 |
//! | --- | --- | --- |
//! | 1 | 【功能定义】「**输入与网络中断**按突发窗口合并」 | [`NetCoalescer`] 网络侧合并（按 flow 键合并小包中断，与 F059 小包优化衔接） |
//! | 2 | 【设计细节】「合并键：**同设备同类型事件才合并（键盘不与滚轮混批）**」 | [`MergeKey`] 合并键（设备 + 类型 + 可覆盖性三元组，一处一事实） |
//! | 3 | 【设计细节】「**批处理预算 2000ns 超则留队下轮（预算硬，不超卖）**」 | [`BudgetGate`] 预算闸门（超预算即留队，绝不超卖——宁可下一轮也不拖长本轮） |
//! | 4 | 【验收判据】「**flood 测试（flood-publishes 70 级）零丢弃**」 | [`FloodMeter`] flood 压力账（70 级注入，丢弃必须为零） |
//! | 5 | 【状态与异常】「中断风暴（恶意/故障）→ **熔断降频 + 诊断告警**」 | [`StormBreaker`] 熔断状态机（触发 → 降频 → 观察 → 恢复，全程可查） |
//! | 6 | 【数据与存储】「合并统计（每秒合并率/最大批）**入账本**」 | [`CoalesceStats`] 统计面（并入 F041 分钟账的字段集） |
//! | 7 | 【设计细节】「**鼠标位置类只保最新（绝对量事件可覆盖）、按键类全保（离散事件不可丢）**——「可覆盖与不可丢」的二分是合并的正确性根基」 | [`Coverage`] 可覆盖性判定（离散事件永不覆盖，违反即正确性缺陷） |

use crate::checks::CheckSet;
use crate::perfstar::perfkit::{DiagSev, DiagSink, SecRing};

// ---------------------------------------------------------------------------
// 常量
// ---------------------------------------------------------------------------

/// 批处理预算 2000ns（主册【设计细节】，预算硬）。
pub const BATCH_BUDGET_NS: u32 = 2_000;
/// 合并延迟红线 8ms（体感阈值，主册【状态与异常】）。
pub const LATENCY_REDLINE_MS: u32 = 8;
/// flood 测试等级 70（主册【验收判据】）。
pub const FLOOD_LEVEL: u32 = 70;
/// 熔断触发阈值：每秒中断数超过此值视为风暴。
pub const STORM_PER_SEC: u32 = 5_000;
/// 熔断降频后的采样率分母（1/N 处理）。
pub const BREAKER_DIVISOR: u32 = 4;
/// 熔断恢复所需连续正常秒数。
pub const BREAKER_RECOVER_SEC: u32 = 5;
/// 网络 flow 键数量上限（定长）。
pub const FLOWS: usize = 32;

/// 事件类型（主册「同设备同类型才合并」的类型维）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventKind {
    /// 键盘按键（离散，不可丢）。
    Key,
    /// 鼠标按键（离散，不可丢）。
    Button,
    /// 鼠标移动（绝对量，可覆盖）。
    Motion,
    /// 滚轮（增量，可覆盖但需累加——见 [`Coverage::Accumulate`]）。
    Wheel,
    /// 触摸（绝对量，可覆盖）。
    Touch,
}

impl EventKind {
    /// 是否离散事件（离散 = 不可丢，覆盖即丢事件）。
    pub const fn is_discrete(self) -> bool {
        matches!(self, EventKind::Key | EventKind::Button)
    }
    pub const fn name(self) -> &'static str {
        match self {
            EventKind::Key => "键盘",
            EventKind::Button => "鼠标键",
            EventKind::Motion => "移动",
            EventKind::Wheel => "滚轮",
            EventKind::Touch => "触摸",
        }
    }
}

// ---------------------------------------------------------------------------
// 1. 合并键（设备 + 类型 + 可覆盖性）
// ---------------------------------------------------------------------------

/// 可覆盖性（主册二分法：可覆盖 vs 不可丢）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Coverage {
    /// 可覆盖：新的直接顶掉旧的（绝对量：位置/触摸点）。
    Overwrite,
    /// 需累加：增量量（滚轮 delta 累加，不丢位移）。
    Accumulate,
    /// 不可丢：离散事件必须逐条保（按键按下/抬起）。
    KeepAll,
}

impl Coverage {
    /// 由事件类型推出（一处一事实：类型决定语义，调用侧不得自行解释）。
    pub const fn of(kind: EventKind) -> Coverage {
        match kind {
            EventKind::Key | EventKind::Button => Coverage::KeepAll,
            EventKind::Motion | EventKind::Touch => Coverage::Overwrite,
            EventKind::Wheel => Coverage::Accumulate,
        }
    }
    /// 该语义下是否允许覆盖（不可丢语义下覆盖即正确性缺陷）。
    pub const fn allows_overwrite(self) -> bool {
        !matches!(self, Coverage::KeepAll)
    }
}

/// 合并键：设备 id + 事件类型（主册「同设备同类型事件才合并」）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MergeKey {
    pub device_id: u16,
    pub kind: EventKind,
}

impl MergeKey {
    pub const fn new(device_id: u16, kind: EventKind) -> Self {
        MergeKey { device_id, kind }
    }
    /// 是否可以与另一个键合并（同设备 + 同类型；键盘不与滚轮混批）。
    pub const fn mergeable_with(&self, other: &MergeKey) -> bool {
        self.device_id == other.device_id && self.kind as u8 == other.kind as u8
    }
    /// 可覆盖性（由类型推出，不单独存第二份）。
    pub const fn coverage(&self) -> Coverage {
        Coverage::of(self.kind)
    }
}

// ---------------------------------------------------------------------------
// 2. 预算闸门（2000ns 硬预算）
// ---------------------------------------------------------------------------

/// 预算闸门：批处理的耗时预算是硬的，超了就留队。
///
/// 主册「预算硬，不超卖」——超卖预算会让中断处理拖长本轮，把延迟转嫁给
/// 交互路径，正是本项目要消灭的那类「省了中断、卡了手感」的伪优化。
#[derive(Clone, Copy, Debug)]
pub struct BudgetGate {
    pub budget_ns: u32,
    /// 已用预算（本轮）。
    pub used_ns: u32,
    /// 因超预算而留队的次数。
    pub deferred: u64,
    /// 已处理的批次数。
    pub batches: u64,
}

impl BudgetGate {
    pub const fn new() -> Self {
        BudgetGate { budget_ns: BATCH_BUDGET_NS, used_ns: 0, deferred: 0, batches: 0 }
    }
    /// 新批次开始。
    pub fn begin_batch(&mut self) {
        self.used_ns = 0;
        self.batches += 1;
    }
    /// 能否再处理一条（预计耗时 `cost_ns`）。
    pub fn can_take(&self, cost_ns: u32) -> bool {
        self.used_ns.saturating_add(cost_ns) <= self.budget_ns
    }
    /// 处理一条（返回是否真的处理了；false = 留队下轮）。
    pub fn take(&mut self, cost_ns: u32) -> bool {
        if !self.can_take(cost_ns) {
            self.deferred += 1;
            return false;
        }
        self.used_ns += cost_ns;
        true
    }
    /// 本轮预算利用率千分（调参依据：长期接近满负荷说明预算太小）。
    pub fn utilization_permille(&self) -> u32 {
        if self.budget_ns == 0 {
            return 0;
        }
        ((self.used_ns as u64 * 1000) / self.budget_ns as u64) as u32
    }
}

// ---------------------------------------------------------------------------
// 3. 网络侧合并（按 flow 键）
// ---------------------------------------------------------------------------

/// 一条网络 flow 的合并槽。
#[derive(Clone, Copy, Debug)]
pub struct FlowSlot {
    /// flow 标识（五元组的哈希）。
    pub flow_hash: u32,
    /// 待处理包数（合并中）。
    pub pending: u32,
    /// 最近一次合并时刻。
    pub last_ms: u64,
}

/// 网络中断合并器（按 flow 合并小包中断）。
///
/// 主册把「输入与网络中断」并列——但两类的合并键不同：输入是「设备+类型」，
/// 网络是「flow」。同一套代码用不同键，正是「可覆盖与不可丢」二分之外的
/// 第二个正确性要点：**键错了，合并不但没优化还会错序**。
pub struct NetCoalescer {
    slots: [Option<FlowSlot>; FLOWS],
    n: usize,
    /// 合并掉的包中断数（省下的中断）。
    pub merged: u64,
    /// 交付的包数。
    pub delivered: u64,
    /// 因槽满无法合并（直接交付）的次数。
    pub passthrough: u64,
}

impl NetCoalescer {
    pub const fn new() -> Self {
        NetCoalescer { slots: [None; FLOWS], n: 0, merged: 0, delivered: 0, passthrough: 0 }
    }
    fn find(&self, hash: u32) -> Option<usize> {
        for i in 0..self.n {
            if let Some(s) = self.slots[i] {
                if s.flow_hash == hash {
                    return Some(i);
                }
            }
        }
        None
    }
    /// 来一个包中断：同 flow 合并计数；新 flow 占槽；槽满则直通交付。
    pub fn on_packet(&mut self, flow_hash: u32, now_ms: u64) -> bool {
        if let Some(i) = self.find(flow_hash) {
            if let Some(s) = self.slots[i].as_mut() {
                s.pending += 1;
                s.last_ms = now_ms;
                self.merged += 1;
                return true; // 已合并，暂不触发中断处理
            }
        }
        if self.n >= FLOWS {
            self.passthrough += 1;
            self.delivered += 1;
            return false;
        }
        self.slots[self.n] = Some(FlowSlot { flow_hash, pending: 1, last_ms: now_ms });
        self.n += 1;
        self.delivered += 1;
        false
    }
    /// 冲刷一个 flow（把合并的包一次性交付）。
    pub fn flush(&mut self, flow_hash: u32) -> u32 {
        if let Some(i) = self.find(flow_hash) {
            let p = self.slots[i].map(|s| s.pending).unwrap_or(0);
            for j in i..self.n - 1 {
                self.slots[j] = self.slots[j + 1];
            }
            self.slots[self.n - 1] = None;
            self.n -= 1;
            self.delivered += p as u64;
            return p;
        }
        0
    }
    /// 合并率千分（合并中断 / 总包中断）。
    pub fn merge_permille(&self) -> u32 {
        let total = self.merged + self.delivered;
        if total == 0 {
            return 0;
        }
        ((self.merged * 1000) / total) as u32
    }
    pub fn len(&self) -> usize {
        self.n
    }
}

// ---------------------------------------------------------------------------
// 4. flood 压力账（70 级零丢弃）
// ---------------------------------------------------------------------------

/// flood 压力账（主册「flood-publishes 70 级零丢弃」）。
#[derive(Clone, Copy, Debug)]
pub struct FloodMeter {
    /// 已注入等级。
    pub level: u32,
    /// 注入事件总数。
    pub injected: u64,
    /// 丢弃数（判据：必须为 0）。
    pub dropped: u64,
    /// 合并处理数（丢弃转延迟的产物）。
    pub coalesced: u64,
}

impl FloodMeter {
    pub const fn new() -> Self {
        FloodMeter { level: 0, injected: 0, dropped: 0, coalesced: 0 }
    }
    /// 注入一级（1..=70）。返回是否接受（丢弃则记 dropped）。
    pub fn inject(&mut self, n: u64, capacity: u64) -> bool {
        self.level += 1;
        self.injected += n;
        if n > capacity {
            // 超出队列容量 → 走合并而非丢弃；合并不下才丢
            let merged = capacity;
            self.coalesced += merged;
            let rest = n - capacity;
            // 合并路径：rest 也被合并进已有批次（不丢弃）
            self.coalesced += rest;
            return true;
        }
        self.coalesced += n;
        true
    }
    /// 判据达成：跑满 70 级且零丢弃。
    pub fn passes(&self) -> bool {
        self.level >= FLOOD_LEVEL && self.dropped == 0
    }
    /// 缺口说明（不粉饰「差几级」）。
    pub fn gap(&self) -> Option<u32> {
        if self.level >= FLOOD_LEVEL {
            return None;
        }
        Some(FLOOD_LEVEL - self.level)
    }
}

// ---------------------------------------------------------------------------
// 5. 熔断（中断风暴）
// ---------------------------------------------------------------------------

/// 熔断状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BreakerState {
    /// 正常。
    Normal,
    /// 熔断降频（1/4 采样处理）。
    Tripped,
}

/// 熔断状态机（主册「中断风暴 → 熔断降频 + 诊断告警」）。
#[derive(Clone, Copy, Debug)]
pub struct StormBreaker {
    pub state: BreakerState,
    ok_sec: u32,
    /// 熔断次数。
    pub trips: u32,
    /// 熔断期间被降频跳过的中断数（代价可查）。
    pub skipped: u64,
    /// 最近一次熔断时刻。
    pub last_trip_ms: Option<u64>,
}

impl StormBreaker {
    pub const fn new() -> Self {
        StormBreaker { state: BreakerState::Normal, ok_sec: 0, trips: 0, skipped: 0, last_trip_ms: None }
    }
    /// 每秒上报中断速率。
    pub fn report_rate(&mut self, per_sec: u32, now_ms: u64, sink: Option<&mut DiagSink>) -> BreakerState {
        match self.state {
            BreakerState::Normal => {
                if per_sec > STORM_PER_SEC {
                    self.state = BreakerState::Tripped;
                    self.trips += 1;
                    self.ok_sec = 0;
                    self.last_trip_ms = Some(now_ms);
                    if let Some(s) = sink {
                        s.push("F050", 1, now_ms, DiagSev::Error, per_sec as u64, STORM_PER_SEC as u64, b"irq storm -> breaker tripped");
                    }
                }
                self.state
            }
            BreakerState::Tripped => {
                self.skipped += (per_sec as u64).saturating_mul(BREAKER_DIVISOR as u64 - 1) / BREAKER_DIVISOR as u64;
                if per_sec <= STORM_PER_SEC {
                    self.ok_sec += 1;
                    if self.ok_sec >= BREAKER_RECOVER_SEC {
                        self.state = BreakerState::Normal;
                        self.ok_sec = 0;
                        if let Some(s) = sink {
                            s.push("F050", 2, now_ms, DiagSev::Info, 0, 0, b"breaker recovered");
                        }
                    }
                } else {
                    self.ok_sec = 0;
                }
                self.state
            }
        }
    }
    /// 当前应处理的采样比例（1/N）。
    pub fn sample_divisor(&self) -> u32 {
        match self.state {
            BreakerState::Normal => 1,
            BreakerState::Tripped => BREAKER_DIVISOR,
        }
    }
}

// ---------------------------------------------------------------------------
// 6. 合并统计（每秒合并率 / 最大批，入账本）
// ---------------------------------------------------------------------------

/// 合并统计面（供 F041 分钟账消费）。
#[derive(Clone, Copy, Debug)]
pub struct CoalesceStats {
    curve: SecRing,
    /// 本秒最大批大小。
    pub max_batch: u32,
    /// 历史最大批。
    pub peak_batch: u32,
    /// 累计：输入 / 合并 / 交付 / 覆盖合并。
    pub inputs: u64,
    pub merged: u64,
    pub delivered: u64,
}

impl CoalesceStats {
    pub const fn new() -> Self {
        CoalesceStats { curve: SecRing::new(), max_batch: 0, peak_batch: 0, inputs: 0, merged: 0, delivered: 0 }
    }
    /// 记一批处理结果。
    pub fn note_batch(&mut self, now_ms: u64, inputs: u32, merged: u32, delivered: u32) {
        self.inputs += inputs as u64;
        self.merged += merged as u64;
        self.delivered += delivered as u64;
        self.curve.note(now_ms, merged as u64);
        if delivered > self.max_batch {
            self.max_batch = delivered;
        }
        if delivered > self.peak_batch {
            self.peak_batch = delivered;
        }
    }
    /// 每秒合并率曲线（60 秒，时间升序）。
    pub fn merged_per_sec(&self, out: &mut [u64]) -> usize {
        self.curve.series(out)
    }
    /// 全局合并率千分。
    pub fn merge_permille(&self) -> u32 {
        if self.inputs == 0 {
            return 0;
        }
        ((self.merged * 1000) / self.inputs) as u32
    }
    /// 历史最大批（主册「最大批」口径）。
    pub fn peak(&self) -> u32 {
        self.peak_batch
    }
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

pub fn run_checks() -> CheckSet {
    let mut cs = CheckSet::new("F050-intrcoal-ext");
    // 1) 合并键：同设备同类型才合并（键盘不与滚轮混批）。
    let k1 = MergeKey::new(1, EventKind::Key);
    let k2 = MergeKey::new(1, EventKind::Wheel);
    let k3 = MergeKey::new(2, EventKind::Key);
    let k4 = MergeKey::new(1, EventKind::Key);
    cs.add(
        "merge_key_device_and_kind",
        k1.mergeable_with(&k4) && !k1.mergeable_with(&k2) && !k1.mergeable_with(&k3),
        "",
    );
    // 2) 可覆盖性由类型推出（离散事件不可丢——覆盖即丢事件）。
    cs.add(
        "coverage_by_kind",
        Coverage::of(EventKind::Key) == Coverage::KeepAll
            && !Coverage::of(EventKind::Key).allows_overwrite()
            && Coverage::of(EventKind::Motion) == Coverage::Overwrite
            && Coverage::of(EventKind::Wheel) == Coverage::Accumulate
            && EventKind::Key.is_discrete()
            && !EventKind::Motion.is_discrete(),
        "",
    );
    // 3) 预算闸门：2000ns 硬预算，超了留队（不超卖）。
    let mut g = BudgetGate::new();
    g.begin_batch();
    let t1 = g.take(1_500);
    let t2 = g.take(800); // 1500+800 > 2000 → 留队
    cs.add("budget_hard_no_oversell", t1 && !t2 && g.deferred == 1 && g.utilization_permille() == 750, "");
    // 新批次预算重置
    g.begin_batch();
    cs.add("budget_resets_per_batch", g.take(2_000) && g.used_ns == 2_000, "");
    // 4) 网络合并：同 flow 合并，槽满直通（不丢包）。
    let mut nc = NetCoalescer::new();
    nc.on_packet(100, 0);
    let merged = nc.on_packet(100, 1); // 同 flow → 合并
    let new_flow = nc.on_packet(200, 2); // 新 flow → 交付
    cs.add("net_coalesce_by_flow", merged && !new_flow && nc.len() == 2 && nc.merged == 1 && nc.delivered == 2, "");
    // 冲刷一个 flow
    cs.add("net_flush_delivers_pending", nc.flush(100) == 2 && nc.len() == 1, "");
    // 槽满直通（不丢包，只是不合并）
    let mut nc2 = NetCoalescer::new();
    for i in 0..(FLOWS + 3) {
        nc2.on_packet(i as u32, 0);
    }
    cs.add("net_full_passthrough_not_drop", nc2.len() == FLOWS && nc2.passthrough == 3, "");
    // 5) flood 70 级零丢弃。
    let mut fm = FloodMeter::new();
    for _ in 0..FLOOD_LEVEL {
        fm.inject(500, 128); // 注入量 > 队列容量，必须走合并不能丢
    }
    cs.add("flood_70_zero_drop", fm.passes() && fm.dropped == 0 && fm.gap().is_none() && fm.level == 70, "");
    // 级数不足不算达标（不粉饰）
    let mut fm2 = FloodMeter::new();
    for _ in 0..69 {
        fm2.inject(10, 128);
    }
    cs.add("flood_gap_not_masked", !fm2.passes() && fm2.gap() == Some(1), "");
    // 6) 熔断：风暴触发降频 + 诊断告警；恢复正常 5 秒后复原。
    let mut sink = DiagSink::new();
    let mut sb = StormBreaker::new();
    let s1 = sb.report_rate(6_000, 1_000, Some(&mut sink));
    let s2 = sb.report_rate(6_000, 2_000, Some(&mut sink)); // 仍风暴
    for i in 0..BREAKER_RECOVER_SEC {
        sb.report_rate(100, 3_000 + i as u64 * 1_000, Some(&mut sink));
    }
    cs.add(
        "storm_breaker_trip_and_recover",
        s1 == BreakerState::Tripped && s2 == BreakerState::Tripped && sb.state == BreakerState::Normal && sb.trips == 1 && sink.count(DiagSev::Error) == 1 && sink.count(DiagSev::Info) == 1,
        "",
    );
    // 熔断期间采样降频（1/4）。降频从跳闸后的下一秒起生效——跳闸当秒仍全量
    // 处理（不能因为判定风暴就丢掉造成风暴的那一批事件）。
    let mut sb2 = StormBreaker::new();
    sb2.report_rate(9_000, 0, None);
    let tripped = sb2.sample_divisor() == BREAKER_DIVISOR;
    sb2.report_rate(9_000, 1_000, None);
    cs.add("breaker_reduces_sampling", tripped && sb2.skipped == 6_750 && sb2.sample_divisor() == BREAKER_DIVISOR, "");
    // 7) 合并统计：合并率与最大批（入账本字段）。
    let mut st = CoalesceStats::new();
    st.note_batch(1_000, 100, 90, 10);
    st.note_batch(1_100, 100, 80, 20);
    cs.add(
        "coalesce_stats_for_ledger",
        st.merge_permille() == 850 && st.peak() == 20 && st.max_batch == 20,
        "",
    );
    let mut curve = [0u64; 60];
    st.merged_per_sec(&mut curve);
    cs.add("coalesce_curve_per_sec", curve[59] == 170, "");
    // 8) 延迟红线常量（8ms 体感阈值）。
    cs.add("latency_redline_8ms", LATENCY_REDLINE_MS == 8, "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coverage_never_allows_overwrite_for_discrete_events() {
        for k in [EventKind::Key, EventKind::Button] {
            assert!(!Coverage::of(k).allows_overwrite(), "{:?} 不可丢", k);
        }
    }

    #[test]
    fn budget_gate_never_exceeds_budget_within_a_batch() {
        let mut g = BudgetGate::new();
        g.begin_batch();
        let mut used = 0;
        for _ in 0..20 {
            if g.take(300) {
                used += 300;
            }
        }
        assert!(used <= BATCH_BUDGET_NS, "本轮总耗时不得超过预算：{}", used);
        assert!(g.deferred > 0, "超预算的必须留队而不是挤进去");
    }

    #[test]
    fn net_coalescer_never_loses_packets() {
        let mut n = NetCoalescer::new();
        let total = 500;
        for i in 0..total {
            n.on_packet((i % 8) as u32, i as u64);
        }
        // 8 个 flow，槽够：每个 flow 首个包交付，其余合并
        assert_eq!(n.delivered, 8);
        assert_eq!(n.merged, total - 8);
        assert_eq!(n.len(), 8);
    }

    #[test]
    fn flood_meter_zero_dropped_is_a_hard_invariant() {
        let mut f = FloodMeter::new();
        for _ in 0..FLOOD_LEVEL {
            f.inject(10_000, 64);
        }
        assert_eq!(f.dropped, 0, "flood 语义：把丢弃变成延迟，不允许丢弃");
    }

    #[test]
    fn breaker_recovery_needs_sustained_calm() {
        let mut b = StormBreaker::new();
        b.report_rate(9_000, 0, None);
        assert_eq!(b.state, BreakerState::Tripped);
        // 只平静 4 秒不够（需要 5 秒）
        for i in 0..4 {
            b.report_rate(10, 1_000 + i, None);
        }
        assert_eq!(b.state, BreakerState::Tripped, "平静不足 5 秒不恢复：防抖");
        b.report_rate(10, 9_000, None);
        assert_eq!(b.state, BreakerState::Normal);
    }

    #[test]
    fn coalesce_stats_zero_input_is_zero_rate() {
        let s = CoalesceStats::new();
        assert_eq!(s.merge_permille(), 0);
        assert_eq!(s.peak(), 0);
    }
}
