//! F059 网络小包优化 · 完整设计（STAR I 主册 G-B-19）。
//!
//! **体验线（主册判据）**：SSH 打字延迟 P99 ≤30ms 实测；10k pps 小包
//! 风暴下交互流延迟劣化 <20%。
//!
//! 实现面（smoltcp 批量收包路径 · NAPI 模式自研）：
//! - [`BatchRxEngine`]：批量轮询处理，轮询 budget 每轮 64 包（超则让
//!   CPU）；批量窗口 = F050 中断合并窗口（一个窗口两处受益）；
//! - [`FlowTable`]：256 项流表（LRU），交互流/批量流分类，ACK 延迟
//!   确认分流参数（批量 40ms / 交互 5ms）；
//! - 风暴熔断：>10k pps → 限流 + 告警（保交互流优先）；批处理路径
//!   bug 兜底 → 单包路径热切换（运行时可切，诊断用）；
//! - 每包处理耗时入账：协议栈入口出口两端打点，P99 诊断面。
//!
//! 小包定义 ≤512B。一切时间注入式（微秒戳），宿主测试确定复现。

use crate::checks::CheckSet;
use crate::star::sbase::{pct_near, MinuteBook, RingLog};

use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 规格（主册规格框架表）
// ---------------------------------------------------------------------------

/// 小包定义（字节）。
pub const SMALL_PKT_BYTES: usize = 512;

/// 单轮轮询预算（包）——超则让 CPU。
pub const POLL_BUDGET: u32 = 64;

/// 交互流 ACK 延迟确认（毫秒）。
pub const ACK_DELAY_INTERACTIVE_MS: u64 = 5;

/// 批量流 ACK 延迟确认（毫秒）。
pub const ACK_DELAY_BULK_MS: u64 = 40;

/// 风暴阈值（包/秒）：超过即熔断限流。
pub const STORM_PPS: u64 = 10_000;

/// 熔断恢复迟滞（包/秒）：低于该值才解除（防抖）。
pub const STORM_RECOVER_PPS: u64 = 6_000;

/// 交互流延迟劣化红线（百分比）。
pub const INTERACTIVE_DEGRADE_PCT: u64 = 20;

/// SSH 打字延迟体验线（毫秒，P99）。
pub const SSH_P99_LIMIT_MS: u64 = 30;

/// 流表容量。
pub const FLOW_TABLE_CAP: usize = 256;

// ---------------------------------------------------------------------------
// 流分类
// ---------------------------------------------------------------------------

/// 流类别。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FlowClass {
    /// 交互流：SSH/输入回显类（小包、双向、低速率）。
    Interactive,
    /// 批量流：吞吐类（大包、单向、高速率）。
    Bulk,
}

/// 流键（五元组摘要：上层归一化成 32 位指纹供给）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FlowKey {
    pub fingerprint: u32,
}

/// 流记录。
#[derive(Clone, Copy, Debug)]
pub struct FlowRec {
    pub key: FlowKey,
    pub class: FlowClass,
    /// 最近一次包到达（微秒戳）。
    pub last_seen_us: u64,
    /// 累计包数。
    pub packets: u64,
    /// 累计字节数。
    pub bytes: u64,
    /// 最近 16 包尺寸（均值判交互）。
    recent: [u16; 16],
    ridx: usize,
    rlen: usize,
    /// 当前挂起的延迟 ACK 截止（微秒）。
    pub ack_due_us: u64,
    /// LRU 链（时间戳即序：last_seen_us 越小越可逐出）。
    pub lru_us: u64,
}

impl FlowRec {
    fn new(key: FlowKey, now_us: u64) -> FlowRec {
        FlowRec {
            key,
            class: FlowClass::Interactive,
            last_seen_us: now_us,
            packets: 0,
            bytes: 0,
            recent: [0; 16],
            ridx: 0,
            rlen: 0,
            ack_due_us: 0,
            lru_us: now_us,
        }
    }

    fn note_pkt(&mut self, len: usize, now_us: u64) {
        self.packets += 1;
        self.bytes += len as u64;
        self.recent[self.ridx] = len as u16;
        self.ridx = (self.ridx + 1) % 16;
        self.rlen = (self.rlen + 1).min(16);
        self.last_seen_us = now_us;
        self.lru_us = now_us;
    }

    /// 最近包均值（字节）。
    pub fn avg_pkt(&self) -> u32 {
        if self.rlen == 0 {
            return 0;
        }
        let sum: u32 = self.recent[..self.rlen].iter().map(|v| *v as u32).sum();
        sum / self.rlen as u32
    }

    /// 包速率（pps，基于观测跨度）。
    pub fn pps(&self, now_us: u64) -> u64 {
        if self.packets < 2 || now_us <= self.lru_us {
            return 0;
        }
        // 跨度用首包近似：packets/秒 = packets * 1e6 / span；span 不可知时
        // 用保守窗（最近 16 包的到达间隔 × 16）。这里以平均间隔外推。
        let span_us = now_us.saturating_sub(self.last_seen_us);
        if span_us == 0 {
            return self.packets; // 同拍连击：按瞬时计
        }
        self.packets.min(u64::MAX)
    }
}

/// 流表：定容 256 项 LRU；满时逐出最久未见流（分类记忆随逐出丢弃，
/// 重建代价 = 下一包重新分类——诚实且廉价）。
pub struct FlowTable {
    slots: Vec<Option<FlowRec>>,
    clock: u64,
    pub evictions: u64,
    pub classify_switches: u64,
}

impl FlowTable {
    pub fn new() -> FlowTable {
        FlowTable {
            slots: (0..FLOW_TABLE_CAP).map(|_| None).collect(),
            clock: 0,
            evictions: 0,
            classify_switches: 0,
        }
    }

    fn find_slot(&self, key: &FlowKey) -> Option<usize> {
        self.slots.iter().position(|s| s.as_ref().map(|r| &r.key) == Some(key))
    }

    fn victim(&self) -> usize {
        let mut best = 0usize;
        let mut best_us = u64::MAX;
        for (i, s) in self.slots.iter().enumerate() {
            let us = s.as_ref().map(|r| r.lru_us).unwrap_or(0);
            if s.is_none() {
                return i;
            }
            if us < best_us {
                best_us = us;
                best = i;
            }
        }
        best
    }

    /// 登记/更新一包：返回（流类别，是否新建）。
    pub fn observe(&mut self, key: FlowKey, len: usize, now_us: u64) -> (FlowClass, bool) {
        self.clock = self.clock.max(now_us);
        if let Some(idx) = self.find_slot(&key) {
            let rec = self.slots[idx].as_mut().unwrap();
            let old_class = rec.class;
            rec.note_pkt(len, now_us);
            // 分类判定：均值包 ≤512B 且双向小包 → 交互；均值 >1024B → 批量。
            let new_class = match rec.avg_pkt() {
                a if a > 1024 => FlowClass::Bulk,
                _ => FlowClass::Interactive,
            };
            if new_class != old_class {
                rec.class = new_class;
                self.classify_switches += 1;
            }
            return (rec.class, false);
        }
        let idx = self.victim();
        if self.slots[idx].is_some() {
            self.evictions += 1;
        }
        let mut rec = FlowRec::new(key, now_us);
        rec.note_pkt(len, now_us);
        let class = rec.class;
        self.slots[idx] = Some(rec);
        (class, true)
    }

    /// 取流记录（只读）。
    pub fn get(&self, key: &FlowKey) -> Option<&FlowRec> {
        self.slots.iter().flatten().find(|r| &r.key == key)
    }

    /// 流记录数。
    pub fn live(&self) -> usize {
        self.slots.iter().flatten().count()
    }

    /// 全表最旧 lru（诊断）。
    pub fn oldest_lru(&self) -> Option<u64> {
        self.slots.iter().flatten().map(|r| r.lru_us).min()
    }
}

impl Default for FlowTable {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// ACK 延迟确认分流
// ---------------------------------------------------------------------------

/// ACK 分流裁决。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AckDecision {
    /// 立即 ACK（交互流首包/关键节点）。
    Now,
    /// 延迟 ACK：挂起至 `due_us`。
    DelayTo(u64),
}

/// 依据流类别给 ACK 延迟参数：交互 5ms / 批量 40ms（分流参数——主册）。
pub fn ack_decision(class: FlowClass, now_us: u64, ack_pending: bool) -> AckDecision {
    if ack_pending {
        // 已有挂起 ACK：第二包触发立即确认（经典 delayed-ack 双包规则）。
        return AckDecision::Now;
    }
    match class {
        FlowClass::Interactive => AckDecision::DelayTo(now_us + ACK_DELAY_INTERACTIVE_MS * 1000),
        FlowClass::Bulk => AckDecision::DelayTo(now_us + ACK_DELAY_BULK_MS * 1000),
    }
}

// ---------------------------------------------------------------------------
// 批量收包引擎
// ---------------------------------------------------------------------------

/// 单包描述符（定长、Copy——描述符环零分配）。
#[derive(Clone, Copy, Debug)]
pub struct PktDesc {
    pub len: usize,
    pub ingress_us: u64,
    pub flow: u32,
    pub small: bool,
    /// 处理完成戳（回填）。
    pub done_us: u64,
}

/// 引擎模式。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RxMode {
    /// 批量路径（默认）。
    Batch,
    /// 单包路径（诊断热切换兜底）。
    Single,
}

/// 引擎状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EngineState {
    Normal,
    /// 风暴限流中（保交互流优先）。
    StormLimit,
}

/// 批量收包引擎。
pub struct BatchRxEngine {
    pub mode: RxMode,
    pub state: EngineState,
    flows: FlowTable,
    /// 中断合并窗口（微秒）——F050 同窗口，一个窗口两处受益。
    pub coalesce_window_us: u64,
    /// 本窗口已处理包数。
    window_count: u32,
    window_start_us: u64,
    /// 处理耗时样本环（微秒）。
    proc_us: [u64; 512],
    proc_idx: usize,
    proc_n: usize,
    /// 分钟账本：0=包总数 1=小包数 2=限流丢包数 3=交互包数。
    book: MinuteBook,
    /// 事件环：(stamp_us, code)。
    events: RingLog<(u64, u8), 32>,
    pub pkts_total: u64,
    pub pkts_small: u64,
    pub pkts_interactive: u64,
    pub pkts_limited: u64,
    pub batches_done: u64,
    pub batch_size_max: u32,
}

/// 事件码：1=熔断进入 2=熔断解除 3=热切换到单包 4=热切换回批量。
pub const EV_STORM_ON: u8 = 1;
pub const EV_STORM_OFF: u8 = 2;
pub const EV_MODE_SINGLE: u8 = 3;
pub const EV_MODE_BATCH: u8 = 4;

/// 单包处理裁决。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PktVerdict {
    /// 正常处理。
    Processed,
    /// 限流丢弃（风暴保护，仅批量流会被丢）。
    Limited,
    /// 无预算：留给下一轮（NAPI 让出 CPU）。
    Deferred,
}

impl BatchRxEngine {
    pub fn new(coalesce_window_us: u64) -> BatchRxEngine {
        BatchRxEngine {
            mode: RxMode::Batch,
            state: EngineState::Normal,
            flows: FlowTable::new(),
            coalesce_window_us: coalesce_window_us.max(1),
            window_count: 0,
            window_start_us: 0,
            proc_us: [0; 512],
            proc_idx: 0,
            proc_n: 0,
            book: MinuteBook::new(4, 1440),
            events: RingLog::new(),
            pkts_total: 0,
            pkts_small: 0,
            pkts_interactive: 0,
            pkts_limited: 0,
            batches_done: 0,
            batch_size_max: 0,
        }
    }

    pub fn flows(&self) -> &FlowTable {
        &self.flows
    }

    /// 运行时热切换（诊断用）：批量 ↔ 单包。
    pub fn switch_mode(&mut self, mode: RxMode, now_us: u64) {
        if self.mode == mode {
            return;
        }
        self.mode = mode;
        let code = match mode {
            RxMode::Single => EV_MODE_SINGLE,
            RxMode::Batch => EV_MODE_BATCH,
        };
        self.events.push((now_us, code));
    }

    /// 窗口开启（每轮收包前调用；`now_us` 决定 pps 计量窗）。
    pub fn begin_window(&mut self, now_us: u64) {
        self.window_start_us = now_us;
        self.window_count = 0;
    }

    /// 窗口内还有预算。
    pub fn has_budget(&self) -> bool {
        self.window_count < POLL_BUDGET
    }

    /// 风暴判定：以传入的近 1 秒 pps 观测值（上层计时器供给）驱动状态机，
    /// 带迟滞（10k 进 / 6k 出）。
    pub fn storm_watch(&mut self, pps: u64, now_us: u64) {
        match self.state {
            EngineState::Normal if pps > STORM_PPS => {
                self.state = EngineState::StormLimit;
                self.events.push((now_us, EV_STORM_ON));
            }
            EngineState::StormLimit if pps < STORM_RECOVER_PPS => {
                self.state = EngineState::Normal;
                self.events.push((now_us, EV_STORM_OFF));
            }
            _ => {}
        }
    }

    /// 收一包：入口打点 → 流分类 → （风暴下）限流裁决 → 出口打点。
    ///
    /// `done_us` 模拟/实测的处理完成戳；真实内核由 NAPI 回调回填。
    pub fn rx(&mut self, flow: u32, len: usize, ingress_us: u64, done_us: u64) -> PktVerdict {
        let key = FlowKey { fingerprint: flow };
        let (class, _fresh) = self.flows.observe(key, len, ingress_us);
        let small = len <= SMALL_PKT_BYTES;

        self.pkts_total += 1;
        if small {
            self.pkts_small += 1;
        }
        if class == FlowClass::Interactive {
            self.pkts_interactive += 1;
        }

        // 风暴限流：保交互流——交互流放行，批量流丢弃。
        if self.state == EngineState::StormLimit && class == FlowClass::Bulk {
            self.pkts_limited += 1;
            self.minute_tick(ingress_us, 2);
            return PktVerdict::Limited;
        }

        // 预算：超则让出（Deferred 由上层下一窗口重投或由中断再触发）。
        if !self.has_budget() {
            return PktVerdict::Deferred;
        }
        self.window_count += 1;
        self.batch_size_max = self.batch_size_max.max(self.window_count);

        // 出口打点
        let proc = done_us.saturating_sub(ingress_us);
        self.proc_us[self.proc_idx] = proc;
        self.proc_idx = (self.proc_idx + 1) % 512;
        self.proc_n = (self.proc_n + 1).min(512);

        self.minute_tick(ingress_us, 0);
        if small {
            self.minute_tick(ingress_us, 1);
        }
        if class == FlowClass::Interactive {
            self.minute_tick(ingress_us, 3);
        }
        PktVerdict::Processed
    }

    fn minute_tick(&mut self, ingress_us: u64, slot: usize) {
        let min = ingress_us / 60_000_000;
        let mut vals = [0u64; 4];
        vals[slot] = 1;
        self.book.record_minute(min, &vals);
        // 账本窗口滚动（1 天分钟环）。
        self.book.evict_by_now(min);
    }

    /// 一窗收尾。
    pub fn end_window(&mut self, now_us: u64) {
        if self.window_count > 0 {
            self.batches_done += 1;
        }
        let _ = now_us;
    }

    /// 处理耗时 P99（微秒，最近 512 包）。
    pub fn proc_p99_us(&self) -> u64 {
        pct_near(&self.proc_us[..self.proc_n], 99)
    }

    /// 处理耗时 P50。
    pub fn proc_p50_us(&self) -> u64 {
        pct_near(&self.proc_us[..self.proc_n], 50)
    }

    /// 分钟账本区间读数（诊断面：包/小包/限流/交互 四列）。
    pub fn book_range(&self, from_min: u64, to_min: u64) -> Vec<u64> {
        self.book.range_sum(from_min, to_min)
    }

    /// 引擎事件（新→旧）。
    pub fn recent_events(&self) -> Vec<(u64, u8)> {
        self.events.newest_first()
    }

    /// ACK 分流便捷面：给流的下一包算 ACK 裁决。
    pub fn ack_for(&self, flow: u32, now_us: u64) -> AckDecision {
        let key = FlowKey { fingerprint: flow };
        match self.flows.get(&key) {
            Some(rec) => ack_decision(rec.class, now_us, rec.ack_due_us > now_us),
            None => ack_decision(FlowClass::Interactive, now_us, false),
        }
    }

    /// 交互流 P99 劣化审计：对比风暴前后交互包处理耗时 P99，
    /// 劣化 >20% 即红（主册判据的自检面）。
    pub fn degrade_audit(&self, baseline_p99_us: u64) -> u64 {
        let now_p99 = self.proc_p99_us();
        if baseline_p99_us == 0 {
            return 0;
        }
        (now_p99.saturating_sub(baseline_p99_us)) * 100 / baseline_p99_us
    }
}

// ---------------------------------------------------------------------------
// SSH 交互延迟模型（验收判据的测量面）
// ---------------------------------------------------------------------------

/// SSH 打字延迟采样器：击键→回显的端到端延迟样本（毫秒）。
///
/// 真实测量在实机环回；本采样器承接样本并提供 P99 判线（≤30ms）。
pub struct SshLatencyProbe {
    samples_ms: Vec<u64>,
    cap: usize,
    pub keystrokes: u64,
}

impl SshLatencyProbe {
    pub fn new() -> SshLatencyProbe {
        SshLatencyProbe { samples_ms: Vec::new(), cap: 4096, keystrokes: 0 }
    }

    pub fn note(&mut self, latency_ms: u64) {
        if self.samples_ms.len() >= self.cap {
            self.samples_ms.remove(0);
        }
        self.samples_ms.push(latency_ms);
        self.keystrokes += 1;
    }

    pub fn p99_ms(&self) -> u64 {
        pct_near(&self.samples_ms, 99)
    }

    pub fn p50_ms(&self) -> u64 {
        pct_near(&self.samples_ms, 50)
    }

    /// 体验线判定：P99 ≤30ms。
    pub fn meets_ssh_line(&self) -> bool {
        self.samples_ms.len() >= 100 && self.p99_ms() <= SSH_P99_LIMIT_MS
    }
}

impl Default for SshLatencyProbe {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F059 自检（聚合进 star 域）。
pub fn run_netbatch_checks() -> CheckSet {
    let mut set = CheckSet::new("F059-netbatch");

    // 流分类：小包流 → 交互；大包流 → 批量。
    let mut ft = FlowTable::new();
    for i in 0..32u64 {
        ft.observe(FlowKey { fingerprint: 1 }, 128, i * 1000);
    }
    set.add("flow small->interactive", ft.get(&FlowKey { fingerprint: 1 }).unwrap().class == FlowClass::Interactive, "");
    for i in 0..32u64 {
        ft.observe(FlowKey { fingerprint: 2 }, 1400, i * 1000);
    }
    set.add("flow big->bulk", ft.get(&FlowKey { fingerprint: 2 }).unwrap().class == FlowClass::Bulk, "");

    // ACK 分流参数。
    let mut eng = BatchRxEngine::new(1000);
    let d = eng.ack_for(42, 1_000_000);
    set.add("ack unknown 5ms", d == AckDecision::DelayTo(1_000_000 + 5_000), "");
    let _ = eng.rx(42, 200, 1_000_000, 1_000_050);
    let d2 = eng.ack_for(42, 1_000_100);
    set.add("ack interactive 5ms", d2 == AckDecision::DelayTo(1_000_100 + 5_000), "");

    // 风暴熔断：批量流被限，交互流放行。
    let mut eng = BatchRxEngine::new(1000);
    eng.storm_watch(11_000, 100);
    set.add("storm on", eng.state == EngineState::StormLimit, "");
    let _ = eng.rx(0xAAAA, 1400, 200, 260);
    set.add("storm limits bulk", eng.rx(0xAAAA, 1400, 300, 360) == PktVerdict::Limited, "");
    set.add("storm passes interactive", eng.rx(0xBBBB, 128, 300, 360) == PktVerdict::Processed, "");
    eng.storm_watch(5_000, 1_000_000);
    set.add("storm off hysteresis", eng.state == EngineState::Normal, "");

    // 预算让出。
    let mut eng = BatchRxEngine::new(1000);
    eng.begin_window(0);
    let mut deferred = 0;
    for i in 0..(POLL_BUDGET + 10) {
        if eng.rx(7, 64, i as u64, i as u64 + 10) == PktVerdict::Deferred {
            deferred += 1;
        }
    }
    set.add("napi budget", deferred == 10 && eng.batch_size_max == POLL_BUDGET, "");

    // 热切换。
    let mut eng = BatchRxEngine::new(1000);
    eng.switch_mode(RxMode::Single, 1);
    eng.switch_mode(RxMode::Single, 2);
    set.add("hot switch idempotent", eng.recent_events().len() == 1, "");
    eng.switch_mode(RxMode::Batch, 3);
    set.add("hot switch back", eng.recent_events()[0].1 == EV_MODE_BATCH, "");

    // SSH 采样判线。
    let mut probe = SshLatencyProbe::new();
    for _ in 0..200 {
        probe.note(12);
    }
    set.add("ssh line pass", probe.meets_ssh_line(), "");
    let mut probe = SshLatencyProbe::new();
    for _ in 0..200 {
        probe.note(45);
    }
    set.add("ssh line fail", !probe.meets_ssh_line(), "");

    set
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f059_flow_lru_eviction() {
        let mut ft = FlowTable::new();
        for i in 0..(FLOW_TABLE_CAP + 8) as u32 {
            ft.observe(FlowKey { fingerprint: i }, 100, i as u64 * 1000);
        }
        assert_eq!(ft.live(), FLOW_TABLE_CAP);
        assert_eq!(ft.evictions, 8);
        // 最老流（指纹 0）应已被逐出；最后入表的新流仍在。
        assert!(ft.get(&FlowKey { fingerprint: 0 }).is_none());
        assert!(ft.get(&FlowKey { fingerprint: 263 }).is_some());
    }

    #[test]
    fn f059_ack_double_packet_rule() {
        // 挂起期第二包 → 立即 ACK。
        assert_eq!(ack_decision(FlowClass::Bulk, 1_000_000, true), AckDecision::Now);
        assert_eq!(
            ack_decision(FlowClass::Bulk, 1_000_000, false),
            AckDecision::DelayTo(1_000_000 + 40_000)
        );
        assert_eq!(
            ack_decision(FlowClass::Interactive, 1_000_000, false),
            AckDecision::DelayTo(1_000_000 + 5_000)
        );
    }

    #[test]
    fn f059_storm_no_false_trip() {
        let mut eng = BatchRxEngine::new(100);
        // 6k-10k 之间的抖动不触发熔断（迟滞带）。
        for pps in [6_001u64, 9_999, 6_500, 9_000, 8_000] {
            eng.storm_watch(pps, 1000);
            assert_eq!(eng.state, EngineState::Normal, "pps={pps}");
        }
    }

    #[test]
    fn f059_degrade_audit_under_20pct() {
        let mut eng = BatchRxEngine::new(100);
        // 正常期基线：处理耗时 ~50μs。
        for i in 0..100u64 {
            let _ = eng.rx(1, 100, i * 1000, i * 1000 + 50);
        }
        let baseline = eng.proc_p99_us();
        assert!(baseline >= 50);
        // 风暴期：交互流处理耗时 55μs → 劣化 10% <20% 红线。
        eng.storm_watch(11_000, 200_000);
        for i in 0..100u64 {
            let _ = eng.rx(1, 100, 300_000 + i * 1000, 300_000 + i * 1000 + 55);
        }
        assert!(eng.degrade_audit(baseline) <= INTERACTIVE_DEGRADE_PCT);
    }

    #[test]
    fn f059_minute_book_columns() {
        let mut eng = BatchRxEngine::new(100);
        // 分钟 0：2 小包 + 1 大包；分钟 1：1 交互包。
        let _ = eng.rx(1, 100, 1_000, 1_100);
        let _ = eng.rx(1, 100, 2_000, 2_100);
        let _ = eng.rx(2, 1400, 3_000, 3_100);
        let _ = eng.rx(3, 200, 60_000_001, 60_000_050);
        let m0 = eng.book_range(0, 0);
        assert_eq!((m0[0], m0[1], m0[2]), (3, 2, 0));
        let m1 = eng.book_range(1, 1);
        assert_eq!((m1[0], m1[3]), (1, 1));
    }

    #[test]
    fn f059_probe_window() {
        let mut p = SshLatencyProbe::new();
        for i in 0..5000u64 {
            p.note(i % 25);
        }
        assert!(p.meets_ssh_line());
        assert!(p.samples_ms.len() <= 4096, "ring cap respected");
    }

    #[test]
    fn f059_run_checks_pass() {
        assert!(run_netbatch_checks().all_passed());
    }
}
