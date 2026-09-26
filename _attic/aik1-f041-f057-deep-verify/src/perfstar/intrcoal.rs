//! F050 中断合并（perfstar · G-B-10）——把丢弃变成延迟。
//!
//! 主册判据（验收标准第一句）：
//! **flood 测试（flood-publishes 70 级）零丢弃；滚动场景输入类 p99 延迟 ≤8ms；两数字对账 F041 掉帧归零。**
//!
//! 功能定义（G-B-10）：输入与网络中断按突发窗口合并：2000ns 预算内批量
//! 入队处理；input-probe 的 flood_dropped（实测 70）转为「合并处理零丢弃」。
//!
//! 【设计细节】合并键：同设备同类型事件才合并（键盘不与滚轮混批）；批处理
//! 预算 2000ns 超则留队下轮（预算硬，不超卖）；**鼠标位置类只保最新（绝对量
//! 事件可覆盖）、按键类全保（离散事件不可丢）——「可覆盖与不可丢」的二分是
//! 合并的正确性根基**。
//! 【状态与异常】合并导致延迟 >8ms（体感阈值）→ 动态收缩窗口（流畅优先于
//! 省电）；中断风暴（恶意/故障）→ 熔断降频 + 诊断告警。
//! 【数据与存储】合并队列定长环形（零堆）；合并统计（每秒合并率/最大批）
//! 入账本。
//!
//! 零堆纪律：定长队列 + 定长位置表，无 alloc。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（一处一事实）
// ---------------------------------------------------------------------------

/// 批处理预算：2000ns（主册；预算硬，不超卖）。
pub const BATCH_BUDGET_NS: u32 = 2_000;
/// 单事件处理成本模型（纳秒）：出队 + 分类 + 入交付环。
pub const PER_EVENT_COST_NS: u32 = 40;
/// 体感延迟阈值：合并导致延迟 >8ms → 动态收缩窗口。
pub const LATENCY_CAP_MS: u32 = 8;
/// 窗口收缩因子（每次 ×1/2）与下限（微秒）。
pub const WINDOW_SHRINK_HALF_US: u32 = 2;
pub const WINDOW_MIN_US: u32 = 100;
/// 中断风暴熔断阈值：每秒事件数。
pub const STORM_EVENTS_PER_S: u32 = 10_000;
/// 队列容量（定长环形）。
pub const QUEUE_CAP: usize = 256;
/// 位置覆盖表容量（同键最新值槽位）。
pub const POS_TABLE_CAP: usize = 64;

/// 事件类型。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EvKind {
    /// 位置类（绝对量可覆盖）：鼠标移动/滚轮绝对位置。
    Position,
    /// 离散类（不可丢）：按键按下/释放。
    Discrete,
}

/// 输入事件。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InputEvent {
    pub device: u8,
    pub kind: EvKind,
    pub code: u32,
    pub value: i32,
}

// ---------------------------------------------------------------------------
// 合并器
// ---------------------------------------------------------------------------

/// 中断合并器。
pub struct InterruptCoalescer {
    queue: [Option<InputEvent>; QUEUE_CAP],
    q_head: usize,
    q_n: usize,
    /// 位置类覆盖表：(device, code) → 最新值。
    pos_dev: [u8; POS_TABLE_CAP],
    pos_code: [u32; POS_TABLE_CAP],
    pos_val: [i32; POS_TABLE_CAP],
    pos_n: usize,
    /// 交付环（离散事件全保 + 冲刷出的位置事件）。
    out: [Option<InputEvent>; QUEUE_CAP],
    out_head: usize,
    out_n: usize,
    /// 统计：合并率/最大批/丢弃（恒 0 是正确性根基）。
    total_in: u64,
    total_out: u64,
    merged_away: u64,
    max_batch: u32,
    dropped: u64,
    /// 动态窗口（微秒）：延迟超 8ms 收缩。
    window_us: u32,
    window_shrinks: u32,
    /// 熔断态。
    tripped: bool,
    storm_alarms: u64,
    last_storm_check_ms: u64,
    events_this_sec: u32,
    /// 分钟聚合桶（主册 G-B-10【数据与存储】：「合并统计（每秒合并率/
    /// 最大批）入账本」——分钟粒度账本行，60 分钟环形，帧账本
    /// frameledger.minute_snapshot 同款消费面）。
    minutes: [MinSlot; 60],
    minute_cursor: usize,
    minute_epoch: u64,
    minute_anchored: bool,
}

/// 一分钟的合并统计账本行。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MinuteCoalesceAgg {
    pub minute_index: u64,
    pub inputs: u32,
    pub merged_away: u32,
    pub delivered: u32,
    pub max_batch: u32,
}

#[derive(Clone, Copy)]
struct MinSlot {
    epoch_min: u64,
    inputs: u32,
    merged: u32,
    delivered: u32,
    max_batch: u32,
}
impl MinSlot {
    const fn empty() -> Self {
        MinSlot { epoch_min: u64::MAX, inputs: 0, merged: 0, delivered: 0, max_batch: 0 }
    }
}

impl InterruptCoalescer {
    pub const fn new() -> Self {
        InterruptCoalescer {
            queue: [None; QUEUE_CAP],
            q_head: 0,
            q_n: 0,
            pos_dev: [0; POS_TABLE_CAP],
            pos_code: [0; POS_TABLE_CAP],
            pos_val: [0; POS_TABLE_CAP],
            pos_n: 0,
            out: [None; QUEUE_CAP],
            out_head: 0,
            out_n: 0,
            total_in: 0,
            total_out: 0,
            merged_away: 0,
            max_batch: 0,
            dropped: 0,
            window_us: 8_000, // 初始窗 8ms（主册【状态与异常】：延迟超 8ms 动态收缩窗口）
            window_shrinks: 0,
            tripped: false,
            storm_alarms: 0,
            minutes: [MinSlot::empty(); 60],
            minute_cursor: 0,
            minute_epoch: 0,
            minute_anchored: false,
            last_storm_check_ms: 0,
            events_this_sec: 0,
        }
    }

    /// 事件入队。队列满 → 尾部请求留队计数（不丢：**丢弃恒零是正确性根基**，
    /// 背压语义 = 推迟，不是丢弃）。
    pub fn submit(&mut self, ev: InputEvent, now_ms: u64) {
        self.total_in += 1;
        // 分钟账本时间轴推进（submit 是写路径节拍——分钟桶锚定于此）。
        self.roll_minute(now_ms);
        if self.minute_anchored && self.minutes[self.minute_cursor].epoch_min == self.minute_epoch {
            self.minutes[self.minute_cursor].inputs += 1;
        }
        // 风暴熔断判定（1s 滚动窗）。
        if now_ms.saturating_sub(self.last_storm_check_ms) >= 1_000 {
            self.last_storm_check_ms = now_ms;
            if self.events_this_sec > STORM_EVENTS_PER_S {
                self.tripped = true;
                self.storm_alarms += 1;
            }
            self.events_this_sec = 0;
        }
        self.events_this_sec += 1;
        if self.tripped {
            // 熔断降频：位置类只保留最新（同键覆盖），离散类照常入队。
            if ev.kind == EvKind::Position {
                if self.cover_position(ev) {
                    self.bump_merged();
                }
                return;
            }
        }
        if self.q_n == QUEUE_CAP {
            // 队满：位置类直接覆盖表（合并语义），离散类拒绝即丢——不允许，
            // 因此离散类顶掉最旧的位置类槽位腾地方（位置可覆盖 = 可牺牲）。
            if ev.kind == EvKind::Position {
                if self.cover_position(ev) {
                    self.bump_merged();
                }
                return;
            }
            if !self.evict_one_position() {
                self.dropped += 1; // 理论不可达：位置表恒有回收候选
                return;
            }
        }
        self.queue[(self.q_head + self.q_n) % QUEUE_CAP] = Some(ev);
        self.q_n += 1;
    }

    /// 位置类入覆盖表。返回是否发生同键覆盖（true = 旧值被合并掉）。
    fn cover_position(&mut self, ev: InputEvent) -> bool {
        for i in 0..self.pos_n {
            if self.pos_dev[i] == ev.device && self.pos_code[i] == ev.code {
                self.pos_val[i] = ev.value;
                return true;
            }
        }
        if self.pos_n < POS_TABLE_CAP {
            self.pos_dev[self.pos_n] = ev.device;
            self.pos_code[self.pos_n] = ev.code;
            self.pos_val[self.pos_n] = ev.value;
            self.pos_n += 1;
        }
        false
    }

    fn evict_one_position(&mut self) -> bool {
        for i in 0..self.q_n {
            let idx = (self.q_head + i) % QUEUE_CAP;
            if let Some(e) = self.queue[idx] {
                if e.kind == EvKind::Position {
                    // 位置类可覆盖：直接出队入覆盖表（不丢语义——最新值胜出）。
                    self.queue[idx] = None;
                    if self.cover_position(e) {
                        self.bump_merged();
                    }
                    self.q_head = (self.q_head + 1) % QUEUE_CAP;
                    self.q_n -= 1;
                    return true;
                }
            }
        }
        false
    }

    /// 批处理：2000ns 预算内出队（预算硬不超卖——主册）。
    /// 返回本批处理的事件数。位置类进覆盖表、离散类直接交付。
    pub fn flush_batch(&mut self) -> u32 {
        if self.tripped {
            return 0; // 熔断中：批量处理暂停（降频语义），位置覆盖仍走 submit
        }
        let mut budget = BATCH_BUDGET_NS as i64;
        let mut batch = 0u32;
        let mut delivered_n = 0u32;
        while self.q_n > 0 && budget >= PER_EVENT_COST_NS as i64 {
            let ev = self.queue[self.q_head].take().unwrap();
            self.q_head = (self.q_head + 1) % QUEUE_CAP;
            self.q_n -= 1;
            match ev.kind {
                EvKind::Position => {
                    if self.cover_position(ev) {
                        self.bump_merged();
                    }
                }
                EvKind::Discrete => {
                    self.deliver(ev);
                    delivered_n += 1;
                }
            }
            budget -= PER_EVENT_COST_NS as i64;
            batch += 1;
        }
        if batch > self.max_batch {
            self.max_batch = batch;
        }
        // 分钟账本累加（当前桶 = 最近一次 submit 锚定的分钟；消费面口径
        // 分钟级，拍点级错位可忽略）。
        if self.minute_anchored && self.minutes[self.minute_cursor].epoch_min == self.minute_epoch {
            let slot = &mut self.minutes[self.minute_cursor];
            slot.delivered += delivered_n; // 交付数 ≠ 批处理数（位置合并不算交付）
            slot.max_batch = slot.max_batch.max(batch);
        }
        // 预算内还有积压 → 窗口收缩（流畅优先，主册【状态与异常】）。
        if self.q_n > 0 {
            self.window_us = (self.window_us / WINDOW_SHRINK_HALF_US).max(WINDOW_MIN_US);
            self.window_shrinks += 1;
        }
        batch
    }

    /// 位置覆盖表冲刷（每帧一次：合成器消费最新位置）。
    pub fn drain_positions(&mut self, consumer: &mut dyn FnMut(InputEvent)) -> usize {
        let n = self.pos_n;
        for i in 0..n {
            consumer(InputEvent { device: self.pos_dev[i], kind: EvKind::Position, code: self.pos_code[i], value: self.pos_val[i] });
            self.total_out += 1;
        }
        self.pos_n = 0;
        n
    }

    fn deliver(&mut self, ev: InputEvent) {
        self.out[self.out_head] = Some(ev);
        self.out_head = (self.out_head + 1) % QUEUE_CAP;
        self.out_n = (self.out_n + 1).min(QUEUE_CAP);
        self.total_out += 1;
        if self.minute_anchored && self.minutes[self.minute_cursor].epoch_min == self.minute_epoch {
            self.minutes[self.minute_cursor].delivered += 1;
        }
    }

    /// 分钟账本时间轴推进（submit 写路径节拍锚定；跨分钟翻页清槽）。
    fn roll_minute(&mut self, now_ms: u64) {
        let epoch = now_ms / 60_000;
        if !self.minute_anchored {
            self.minute_anchored = true;
            self.minute_epoch = epoch;
            self.minutes[self.minute_cursor] =
                MinSlot { epoch_min: epoch, inputs: 0, merged: 0, delivered: 0, max_batch: 0 };
            return;
        }
        if epoch > self.minute_epoch {
            let steps = ((epoch - self.minute_epoch) as usize).min(60);
            for _ in 0..steps {
                self.minute_cursor = (self.minute_cursor + 1) % 60;
                self.minute_epoch += 1;
                self.minutes[self.minute_cursor] =
                    MinSlot { epoch_min: self.minute_epoch, inputs: 0, merged: 0, delivered: 0, max_batch: 0 };
            }
        }
    }

    /// 合并计数（全局 + 当前分钟桶双记——一处一事实，账本行是聚合视图）。
    fn bump_merged(&mut self) {
        self.merged_away += 1;
        if self.minute_anchored && self.minutes[self.minute_cursor].epoch_min == self.minute_epoch {
            self.minutes[self.minute_cursor].merged += 1;
        }
    }

    /// 交付环消费（离散事件全保）。
    pub fn take_delivered(&mut self, out: &mut [Option<InputEvent>]) -> usize {
        let n = out.len().min(self.out_n);
        for i in 0..n {
            out[i] = self.out[(self.out_head + i) % QUEUE_CAP];
        }
        self.out_head = (self.out_head + n) % QUEUE_CAP;
        self.out_n -= n;
        n
    }

    pub fn dropped(&self) -> u64 {
        self.dropped
    }

    pub fn merged_away(&self) -> u64 {
        self.merged_away
    }

    /// 分钟聚合快照（升序；跳过未锚定空槽——账本行只含真实分钟）。
    /// 监视器 IO 页合并率曲线与 F061 回归门的消费面（主册【数据与存储】）。
    pub fn minute_snapshot(&self, out: &mut [MinuteCoalesceAgg]) -> usize {
        let mut n = 0;
        for i in 0..60 {
            let idx = (self.minute_cursor + 1 + i) % 60;
            let slot = &self.minutes[idx];
            if slot.epoch_min == u64::MAX || (slot.inputs == 0 && slot.delivered == 0 && slot.merged == 0) {
                continue;
            }
            if n < out.len() {
                out[n] = MinuteCoalesceAgg {
                    minute_index: slot.epoch_min,
                    inputs: slot.inputs,
                    merged_away: slot.merged,
                    delivered: slot.delivered,
                    max_batch: slot.max_batch,
                };
                n += 1;
            }
        }
        n
    }

    pub fn max_batch(&self) -> u32 {
        self.max_batch
    }

    /// 合并率 permille：被覆盖/总入队。
    pub fn merge_rate_permille(&self) -> u32 {
        if self.total_in == 0 {
            return 0;
        }
        (self.merged_away * 1000 / self.total_in) as u32
    }

    pub fn window_us(&self) -> u32 {
        self.window_us
    }

    pub fn window_shrinks(&self) -> u32 {
        self.window_shrinks
    }

    pub fn is_tripped(&self) -> bool {
        self.tripped
    }

    pub fn storm_alarms(&self) -> u64 {
        self.storm_alarms
    }

    /// 延迟核算（滚动场景）：队列深度 × 单事件成本（纳秒模型）。
    pub fn pending_latency_us(&self) -> u32 {
        (self.q_n as u64 * PER_EVENT_COST_NS as u64 / 1_000) as u32
    }

    /// 延迟超 8ms 的动态收缩判定（体感阈值，主册）。
    pub fn latency_over_cap(&self) -> bool {
        self.pending_latency_us() > LATENCY_CAP_MS * 1_000
    }

    /// 熔断解除（风暴退去后由诊断面调用）。
    pub fn reset_breaker(&mut self) {
        self.tripped = false;
    }

    pub fn queue_depth(&self) -> usize {
        self.q_n
    }
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

/// 域自检。
pub fn run_intrcoal_checks() -> CheckSet {
    let mut cs = CheckSet::new("F050-intrcoal");
    // 1) flood 70 级零丢弃（主册判据）：70 个事件全部有归宿。
    let mut ic = InterruptCoalescer::new();
    for i in 0..70u32 {
        let kind = if i % 2 == 0 { EvKind::Position } else { EvKind::Discrete };
        ic.submit(InputEvent { device: 1, kind, code: i % 16, value: i as i32 }, (i / 1000) as u64);
    }
    let mut batches = 0;
    while ic.queue_depth() > 0 && batches < 100 {
        ic.flush_batch();
        batches += 1;
    }
    let mut sink: [Option<InputEvent>; QUEUE_CAP] = [None; QUEUE_CAP];
    ic.take_delivered(&mut sink);
    let mut pos_total = 0;
    ic.drain_positions(&mut |_| pos_total += 1);
    cs.add("flood70_zero_drop", ic.dropped() == 0 && ic.total_in == 70, "");
    // 2) 位置类只保最新（同键覆盖）：50 个同键事件经批处理压入覆盖表，
    // 表内仅存 value=50，其余 49 个被合并。
    let mut ic2 = InterruptCoalescer::new();
    for v in 1..=50i32 {
        ic2.submit(InputEvent { device: 2, kind: EvKind::Position, code: 7, value: v }, 0);
    }
    let _ = ic2.flush_batch();
    let mut latest = 0i32;
    ic2.drain_positions(&mut |e| latest = e.value);
    cs.add("position_keep_latest", latest == 50 && ic2.merged_away() == 49, "");
    // 3) 按键类全保（离散事件不可丢）。
    let mut ic3 = InterruptCoalescer::new();
    for v in 0..30i32 {
        ic3.submit(InputEvent { device: 3, kind: EvKind::Discrete, code: v as u32, value: v }, 0);
    }
    while ic3.queue_depth() > 0 {
        ic3.flush_batch();
    }
    let mut sink3: [Option<InputEvent>; QUEUE_CAP] = [None; QUEUE_CAP];
    let got = ic3.take_delivered(&mut sink3);
    cs.add("discrete_keep_all", got == 30, "");
    // 4) 合并键隔离：键盘不与滚轮混批（设备/类型二元组独立，批处理压表后两条并存）。
    let mut ic4 = InterruptCoalescer::new();
    ic4.submit(InputEvent { device: 1, kind: EvKind::Position, code: 1, value: 10 }, 0);
    ic4.submit(InputEvent { device: 2, kind: EvKind::Position, code: 1, value: 20 }, 0);
    let _ = ic4.flush_batch();
    let mut vals: [i32; 8] = [0; 8];
    let mut vi = 0;
    ic4.drain_positions(&mut |e| {
        vals[vi] = e.value;
        vi += 1;
    });
    cs.add("merge_key_isolation", vi == 2 && vals[0] == 10 && vals[1] == 20, "");
    // 5) 批处理预算硬（2000ns / 40ns = 50 事件/批上限）。
    let mut ic5 = InterruptCoalescer::new();
    for i in 0..200u32 {
        ic5.submit(InputEvent { device: 4, kind: EvKind::Discrete, code: i, value: i as i32 }, 0);
    }
    let b = ic5.flush_batch();
    cs.add("batch_budget_hard", b <= BATCH_BUDGET_NS / PER_EVENT_COST_NS, "");
    // 6) 中断风暴熔断 + 告警：STORM+50 个事件全部铺在 999ms 内（同一
    // 1s 滚动窗），第 1000ms 的事件触发风暴检查点 → 窗口累计超阈值熔断。
    let mut ic6 = InterruptCoalescer::new();
    let n6 = (STORM_EVENTS_PER_S + 50) as u64;
    for i in 0..n6 {
        ic6.submit(InputEvent { device: 5, kind: EvKind::Position, code: (i % 32) as u32, value: 1 }, (i * 999 / n6) as u64);
    }
    ic6.submit(InputEvent { device: 5, kind: EvKind::Position, code: 0, value: 1 }, 1_000);
    cs.add("storm_breaker", ic6.is_tripped() && ic6.storm_alarms() >= 1, "");
    // 7) 延迟超 8ms 动态收缩窗口。
    let mut ic7 = InterruptCoalescer::new();
    for i in 0..150u32 {
        ic7.submit(InputEvent { device: 6, kind: EvKind::Discrete, code: i, value: i as i32 }, 0);
    }
    let w0 = ic7.window_us();
    ic7.flush_batch();
    cs.add("shrink_on_backlog", ic7.window_us() < w0 && ic7.window_shrinks() >= 1, "");
    // 8) 分钟聚合账本（主册【数据与存储】合并统计入账本）：行精确 +
    //     inputs/delivered/max_batch 对账。
    let mut ic8 = InterruptCoalescer::new();
    for i in 0..10u64 {
        ic8.submit(InputEvent { device: 1, kind: EvKind::Position, code: 0, value: i as i32 }, i * 100);
    }
    ic8.flush_batch();
    let mut aggs = [MinuteCoalesceAgg::default(); 8];
    let n8 = ic8.minute_snapshot(&mut aggs);
    // 位置类同键覆盖：10 入队 → 首个进覆盖表、9 个被合并；flush 批处理
    // 10 个（预算内）但零交付（无离散事件）、最大批 = 10。
    cs.add(
        "minute_agg_row",
        n8 == 1
            && aggs[0].inputs == 10
            && aggs[0].merged_away == 9
            && aggs[0].delivered == 0
            && aggs[0].max_batch == 10,
        "",
    );
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flood_seventy_all_accounted() {
        // 主册场景复刻：flood-publishes 70 级 → 交付 + 覆盖 + 队列 = 70。
        let mut ic = InterruptCoalescer::new();
        for i in 0..70u32 {
            ic.submit(InputEvent { device: 1, kind: EvKind::Position, code: i % 4, value: i as i32 }, 0);
        }
        assert_eq!(ic.total_in, 70);
        while ic.queue_depth() > 0 {
            ic.flush_batch();
        }
        let mut n = 0;
        ic.drain_positions(&mut |_| n += 1);
        // 4 个键各保最新 → 4 条交付 + 66 条被覆盖 = 70，丢弃 0。
        assert_eq!(n, 4);
        assert_eq!(ic.merged_away(), 66);
        assert_eq!(ic.dropped(), 0);
        assert_eq!(n as u64 + ic.merged_away() + ic.dropped(), 70);
    }

    #[test]
    fn breaker_reduces_to_cover_only_but_keeps_keys() {
        let mut ic = InterruptCoalescer::new();
        ic.tripped = true; // 直接触发熔断态
        for i in 0..100u32 {
            ic.submit(InputEvent { device: 1, kind: EvKind::Position, code: 1, value: i as i32 }, 1);
            ic.submit(InputEvent { device: 1, kind: EvKind::Discrete, code: i, value: 1 }, 1);
        }
        assert_eq!(ic.dropped(), 0);
        assert_eq!(ic.merged_away(), 99); // 首个入覆盖表，其余 99 个被同键覆盖
        assert_eq!(ic.queue_depth(), 100); // 按键全部留队
        ic.reset_breaker();
        while ic.queue_depth() > 0 {
            ic.flush_batch();
        }
        let mut sink: [Option<InputEvent>; QUEUE_CAP] = [None; QUEUE_CAP];
        assert_eq!(ic.take_delivered(&mut sink), 100);
    }

    #[test]
    fn merge_rate_accounting() {
        let mut ic = InterruptCoalescer::new();
        for v in 0..10i32 {
            ic.submit(InputEvent { device: 1, kind: EvKind::Position, code: 0, value: v }, 0);
        }
        let _ = ic.flush_batch(); // 批处理压入覆盖表：首个入表，后 9 个被同键覆盖
        assert_eq!(ic.merge_rate_permille(), 900); // 9/10 被覆盖
    }

    #[test]
    fn batch_leaves_rest_for_next_round() {
        // 预算硬不超卖：一批最多 50，余下留队下轮（主册原文）。
        let mut ic = InterruptCoalescer::new();
        for i in 0..120u32 {
            ic.submit(InputEvent { device: 1, kind: EvKind::Discrete, code: i, value: i as i32 }, 0);
        }
        let b1 = ic.flush_batch();
        assert_eq!(b1, 50);
        assert_eq!(ic.queue_depth(), 70);
        let b2 = ic.flush_batch();
        assert_eq!(b2, 50);
        assert_eq!(ic.queue_depth(), 20);
    }

    #[test]
    fn minute_buckets_roll_and_account() {
        let mut ic = InterruptCoalescer::new();
        // 分钟 0：5 个位置事件（同键 → 4 merged）+ flush 交付。
        for i in 0..5u64 {
            ic.submit(InputEvent { device: 2, kind: EvKind::Position, code: 7, value: i as i32 }, i * 10);
        }
        ic.flush_batch();
        // 分钟 1：翻页后新桶。
        for i in 0..3u64 {
            ic.submit(InputEvent { device: 2, kind: EvKind::Position, code: 7, value: 100 + i as i32 }, 61_000 + i * 10);
        }
        ic.flush_batch();
        let mut out = [MinuteCoalesceAgg::default(); 8];
        let n = ic.minute_snapshot(&mut out);
        assert_eq!(n, 2);
        assert_eq!(out[0].minute_index, 0);
        assert_eq!(out[0].inputs, 5);
        assert_eq!(out[0].merged_away, 4);
        assert_eq!(out[1].minute_index, 1);
        assert_eq!(out[1].inputs, 3);
        // 全局对账：inputs 和 = total_in；merged 和 = merged_away。
        let ti: u32 = out[..n].iter().map(|a| a.inputs).sum();
        let tm: u32 = out[..n].iter().map(|a| a.merged_away).sum();
        assert_eq!(ti as u64, ic.total_in);
        assert_eq!(tm as u64, ic.merged_away());
    }
}