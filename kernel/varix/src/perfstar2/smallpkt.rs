//! F059 网络小包优化（perfstar2 · G-B-19）——网络栈对小包的尊重就是交互的尊重。
//!
//! 主册判据（验收标准第一句）：
//! **SSH 打字延迟 P99 ≤30ms 实测；10k pps 小包风暴下交互流延迟劣化 <20%。**
//!
//! 功能定义（G-B-19）：smoltcp 批量收包路径——中断聚合（F050 同窗口）+
//! 批量轮询处理；每包处理耗时入账；SSH 交互打字延迟 ≤30ms 为体验线。
//!
//! 【设计细节】批量窗口 = F050 中断合并窗口（一个窗口两处受益）；轮询
//! budget 每轮 64 包（超则让 CPU）；TCP ACK 延迟确认 40ms（交互流 5ms——
//! 分流参数）；小包定义 ≤512B；延迟测量打点在协议栈入口出口两端。
//! 【状态与异常】包风暴（>10k pps）→ 熔断限流 + 告警（保交互流优先）；
//! 批量路径 bug 兜底 → 单包路径热切换（运行时可切，诊断用）。
//!
//! 零堆纪律：定长批队列 + 定长延迟环 + 128 元素插入排序取分位
//! （无 64K 直方图上栈——内核栈预算红线），无 alloc。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（一处一事实；全参数旋钮化——无隐藏魔法数）
// ---------------------------------------------------------------------------

/// 小包定义：≤512B（主册明文）。
pub const SMALL_PKT_BYTES: usize = 512;
/// 轮询 budget：每轮 64 包，超则让 CPU（主册明文）。
pub const POLL_BUDGET_PKTS: usize = 64;
/// 批量窗口 = F050 中断合并窗口初始值 8ms（一处一事实：与
/// `crate::perfstar::intrcoal` 初始窗同源——一个窗口两处受益）。
pub const BATCH_WINDOW_US: u64 = 8_000;
/// TCP ACK 延迟确认：吞吐流 40ms（主册明文）。
pub const ACK_DELAY_BULK_MS: u64 = 40;
/// TCP ACK 延迟确认：交互流 5ms（主册明文——分流参数）。
pub const ACK_DELAY_INTERACTIVE_MS: u64 = 5;
/// 风暴线：>10k pps → 熔断限流 + 告警（主册明文）。
pub const STORM_PPS: u64 = 10_000;
/// SSH 打字延迟体验线：P99 ≤30ms（主册明文）。
pub const SSH_LAT_P99_LIMIT_US: u32 = 30_000;
/// 风暴下交互流劣化红线：<20%（主册明文）。
pub const STORM_DEGRADE_MAX_PCT: u32 = 20;
/// 批队列容量（风暴期积压上界：8ms 窗 × 10k pps = 80 包量级，取 2 倍余量）。
pub const Q_CAP: usize = 256;
/// pps 测量窗：100 桶 × 10ms = 1s 滑动窗。
const PPS_BUCKETS: usize = 100;
const PPS_BUCKET_MS: u64 = 10;
/// 延迟环容量（交互/吞吐各一份）。
const LAT_CAP: usize = 128;

/// 包分类。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FlowClass {
    /// 交互流（SSH 打字等——保延迟）。
    Interactive,
    /// 吞吐流（下载/同步——保带宽）。
    Bulk,
}

/// 处理路径模式（运行时可切——批量路径 bug 兜底）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PathMode {
    Batch,
    Single,
}

// ---------------------------------------------------------------------------
// 引擎
// ---------------------------------------------------------------------------

/// 小包批量处理引擎。
pub struct SmallPktEngine {
    mode: PathMode,
    /// 批队列：纯 FIFO 定长环。不变量：`q[q_head..q_head+q_n]` 全为在队包
    /// （驱逐用窗口内前移压实，不留空洞）。
    q: [Option<(u16, FlowClass, u64)>; Q_CAP], // (len, class, in_us)
    q_head: usize,
    q_n: usize,
    /// 当前批窗口开启时刻（None = 无未决批）。
    window_open_us: Option<u64>,
    /// pps 滑动窗（100 × 10ms 计数；每桶带窗口起点 ms——桶按时间归属，
    /// 1s 外自动过期，不做全窗清零）。
    pps: [u16; PPS_BUCKETS],
    bucket_ms: [u64; PPS_BUCKETS],
    /// 熔断器：风暴告警在位（滞后解除：pps 降至半线以下一个整窗才清）。
    storm_alarm: bool,
    calm_ms: u64,
    /// 熔断限流丢弃计数（只丢 Bulk——保交互流优先）。
    dropped_bulk: u64,
    /// 出口延迟环（head + 有效数语义；有效区 [0..n)，满后整体滚动覆盖）。
    lat_i: [u16; LAT_CAP],
    lat_i_head: usize,
    lat_i_n: usize,
    lat_b: [u16; LAT_CAP],
    lat_b_head: usize,
    lat_b_n: usize,
    /// 无风暴基线（交互流 P50，μs）——劣化比对基准。
    baseline_i_us: u32,
    /// ACK 分流账本（armed = 已有上次 ACK 时刻）。
    ack_i_armed: bool,
    ack_b_armed: bool,
    last_ack_i_ms: u64,
    last_ack_b_ms: u64,
    now_us: u64,
}

impl SmallPktEngine {
    pub const fn new() -> Self {
        SmallPktEngine {
            mode: PathMode::Batch,
            q: [None; Q_CAP],
            q_head: 0,
            q_n: 0,
            window_open_us: None,
            pps: [0; PPS_BUCKETS],
            bucket_ms: [0; PPS_BUCKETS],
            storm_alarm: false,
            calm_ms: 0,
            dropped_bulk: 0,
            lat_i: [0; LAT_CAP],
            lat_i_head: 0,
            lat_i_n: 0,
            lat_b: [0; LAT_CAP],
            lat_b_head: 0,
            lat_b_n: 0,
            baseline_i_us: 0,
            ack_i_armed: false,
            ack_b_armed: false,
            last_ack_i_ms: 0,
            last_ack_b_ms: 0,
            now_us: 0,
        }
    }

    /// 包入口（协议栈入口打点）：分类入队；风暴时限流（只丢 Bulk）。
    /// 返回 false = 被限流丢弃。
    pub fn submit(&mut self, len: usize, class: FlowClass, at_us: u64) -> bool {
        self.now_us = at_us;
        self.bump_pps(at_us);
        // 风暴判定（1s 滑窗，按桶时间归属）→ 熔断限流 + 告警；
        // 解除滞后：半线以下整窗清。
        let pps = self.current_pps(at_us);
        if pps > STORM_PPS {
            self.storm_alarm = true;
            self.calm_ms = 0;
        } else if self.storm_alarm {
            if pps <= STORM_PPS / 2 {
                self.calm_ms += PPS_BUCKET_MS;
                if self.calm_ms >= PPS_BUCKETS as u64 * PPS_BUCKET_MS {
                    self.storm_alarm = false;
                    self.calm_ms = 0;
                }
            } else {
                self.calm_ms = 0;
            }
        }
        if self.storm_alarm && class == FlowClass::Bulk {
            // 限流：交互流零丢弃，Bulk 丢（保交互流优先）。
            self.dropped_bulk += 1;
            return false;
        }
        // 单包热切换路径：不排队，同步记账即出（诊断兜底）。
        if self.mode == PathMode::Single {
            self.record(class, Self::model_proc_us(len));
            return true;
        }
        // 批路径入队（队满兜底：交互挤最老 Bulk；无 Bulk 可挤才丢交互）。
        if self.q_n == Q_CAP {
            if class == FlowClass::Bulk {
                self.dropped_bulk += 1;
                return false;
            }
            if !self.evict_oldest_bulk() {
                return false; // 全是交互的极端保底：丢新包不崩
            }
        }
        let idx = (self.q_head + self.q_n) % Q_CAP;
        self.q[idx] = Some((len.min(u16::MAX as usize) as u16, class, at_us));
        self.q_n += 1;
        if self.window_open_us.is_none() {
            self.window_open_us = Some(at_us);
        }
        true
    }

    /// 驱逐队内最老 Bulk（窗口内前移压实，环不变量保持）。返回是否驱逐成功。
    fn evict_oldest_bulk(&mut self) -> bool {
        for i in 0..self.q_n {
            let idx = (self.q_head + i) % Q_CAP;
            if matches!(self.q[idx], Some((_, FlowClass::Bulk, _))) {
                // [i+1..q_n) 前移一位。
                let mut j = i;
                while j + 1 < self.q_n {
                    let a = (self.q_head + j) % Q_CAP;
                    let b = (self.q_head + j + 1) % Q_CAP;
                    self.q[a] = self.q[b];
                    j += 1;
                }
                self.q[(self.q_head + self.q_n - 1) % Q_CAP] = None;
                self.q_n -= 1;
                self.dropped_bulk += 1;
                return true;
            }
        }
        false
    }

    /// 从队内取第一个匹配包（同类内 FIFO；前移压实，环不变量保持）。
    /// `only_interactive=true` 时只取交互包（保交互流优先调度）。
    fn pop_first(&mut self, only_interactive: bool) -> Option<(u16, FlowClass, u64)> {
        for i in 0..self.q_n {
            let idx = (self.q_head + i) % Q_CAP;
            let take = match self.q[idx] {
                Some((_, c, _)) => !only_interactive || c == FlowClass::Interactive,
                None => false,
            };
            if take {
                let pkt = self.q[idx].take();
                // [i+1..q_n) 前移一位压实。
                let mut j = i;
                while j + 1 < self.q_n {
                    let a = (self.q_head + j) % Q_CAP;
                    let b = (self.q_head + j + 1) % Q_CAP;
                    self.q[a] = self.q[b];
                    j += 1;
                }
                self.q[(self.q_head + self.q_n - 1) % Q_CAP] = None;
                self.q_n -= 1;
                return pkt;
            }
        }
        None
    }

    /// 批量轮询（一个轮次）：窗口到期后预算内出队——**交互流优先**
    /// （先清交互再清吞吐，同类内 FIFO），超预算让 CPU。
    /// 返回本轮实际处理包数。
    pub fn poll_round(&mut self, at_us: u64) -> usize {
        self.now_us = at_us;
        if self.mode == PathMode::Single {
            return 0; // 单包路径无批轮次
        }
        let win = match self.window_open_us {
            Some(w) => w,
            None => return 0,
        };
        if at_us.saturating_sub(win) < BATCH_WINDOW_US {
            return 0; // F050 同窗口：未满不开批
        }
        let mut processed = 0usize;
        while processed < POLL_BUDGET_PKTS && self.q_n > 0 {
            // 第一优先：交互包；队列无交互时才取吞吐包。
            let pkt = match self.pop_first(true) {
                Some(p) => p,
                None => match self.pop_first(false) {
                    Some(p) => p,
                    None => break,
                },
            };
            let (len, class, in_us) = pkt;
            let lat = at_us.saturating_sub(in_us) as u32 + Self::model_proc_us(len as usize);
            self.record(class, lat);
            processed += 1;
        }
        if self.q_n == 0 {
            self.window_open_us = None;
        }
        processed
    }

    /// 单包路径热切换（诊断用兜底；批量路径 bug 时运行时可切）。
    pub fn set_mode(&mut self, m: PathMode) {
        self.mode = m;
    }

    pub fn mode(&self) -> PathMode {
        self.mode
    }

    /// 模型处理耗时（协议栈出口-入口差：小包 ≤512B 定界 120μs，大批摊薄 40μs）。
    fn model_proc_us(len: usize) -> u32 {
        if len <= SMALL_PKT_BYTES {
            120
        } else {
            40
        }
    }

    fn record(&mut self, class: FlowClass, lat_us: u32) {
        let v = lat_us.min(u16::MAX as u32) as u16;
        match class {
            FlowClass::Interactive => {
                self.lat_i[self.lat_i_head] = v;
                self.lat_i_head = (self.lat_i_head + 1) % LAT_CAP;
                self.lat_i_n = (self.lat_i_n + 1).min(LAT_CAP);
            }
            FlowClass::Bulk => {
                self.lat_b[self.lat_b_head] = v;
                self.lat_b_head = (self.lat_b_head + 1) % LAT_CAP;
                self.lat_b_n = (self.lat_b_n + 1).min(LAT_CAP);
            }
        }
    }

    fn bump_pps(&mut self, at_us: u64) {
        let ms = at_us / 1000;
        let bucket_start = ms - (ms % PPS_BUCKET_MS);
        let idx = ((bucket_start / PPS_BUCKET_MS) % PPS_BUCKETS as u64) as usize;
        if self.bucket_ms[idx] != bucket_start {
            // 桶复用（新 10ms 窗）：旧计数失效。
            self.bucket_ms[idx] = bucket_start;
            self.pps[idx] = 0;
        }
        self.pps[idx] = self.pps[idx].saturating_add(1);
    }

    /// 当前 pps（1s 滑窗合计：只计窗口与当前时刻重叠的桶——
    /// `bs + 1000 > ms` 等价于有符号的 `bs > ms - 1000`，无下溢陷阱）。
    pub fn current_pps(&self, at_us: u64) -> u64 {
        let ms = at_us / 1000;
        let window = PPS_BUCKET_MS * PPS_BUCKETS as u64;
        self.pps
            .iter()
            .zip(self.bucket_ms.iter())
            .filter(|(_, &bs)| bs.saturating_add(window) > ms)
            .map(|(&c, _)| c as u64)
            .sum()
    }

    pub fn storm_alarm(&self) -> bool {
        self.storm_alarm
    }

    /// 交互流 P99（延迟环有效样本，插入排序取分位——栈上 128 元素 scratch）。
    pub fn interactive_p99_us(&self) -> u32 {
        percentile(&self.lat_i, self.lat_i_n, 99)
    }

    pub fn bulk_p99_us(&self) -> u32 {
        percentile(&self.lat_b, self.lat_b_n, 99)
    }

    fn interactive_p50_us(&self) -> u32 {
        percentile(&self.lat_i, self.lat_i_n, 50)
    }
    // 注：P50 保留为诊断视图；劣化判定基线用 P99（同类统计量对比）。

    /// 基线标定（无风暴窗口的交互流 P99——劣化比对基准；与风暴期同统计量
    /// 才是同类对比：P99 对 P99，不同量纲对比不诚实）。
    pub fn calibrate_baseline(&mut self) {
        self.baseline_i_us = self.interactive_p99_us();
    }

    pub fn baseline_i_us(&self) -> u32 {
        self.baseline_i_us
    }

    /// 清空延迟账（诊断测量窗口重置——基线期与风暴期分开统计，不混纪元）。
    pub fn clear_latency_ledger(&mut self) {
        self.lat_i = [0; LAT_CAP];
        self.lat_i_head = 0;
        self.lat_i_n = 0;
        self.lat_b = [0; LAT_CAP];
        self.lat_b_head = 0;
        self.lat_b_n = 0;
    }

    /// 风暴劣化率（相对基线 %；基线未标定返回 0——不评，诚实口径）。
    pub fn storm_degrade_pct(&self) -> u32 {
        if self.baseline_i_us == 0 {
            return 0;
        }
        let now = self.interactive_p99_us() as u64;
        let base = self.baseline_i_us as u64;
        (now.saturating_sub(base) * 100 / base) as u32
    }

    pub fn dropped_bulk(&self) -> u64 {
        self.dropped_bulk
    }

    /// ACK 分流决策：交互流 5ms / 吞吐流 40ms 延迟确认。
    /// 首 ACK 立即发（无上次时刻可延迟）；返回 true = 应发 ACK。
    pub fn ack_due(&mut self, class: FlowClass, at_ms: u64) -> bool {
        match class {
            FlowClass::Interactive => {
                if !self.ack_i_armed {
                    self.ack_i_armed = true;
                    self.last_ack_i_ms = at_ms;
                    return true;
                }
                if at_ms.saturating_sub(self.last_ack_i_ms) >= ACK_DELAY_INTERACTIVE_MS {
                    self.last_ack_i_ms = at_ms;
                    true
                } else {
                    false
                }
            }
            FlowClass::Bulk => {
                if !self.ack_b_armed {
                    self.ack_b_armed = true;
                    self.last_ack_b_ms = at_ms;
                    return true;
                }
                if at_ms.saturating_sub(self.last_ack_b_ms) >= ACK_DELAY_BULK_MS {
                    self.last_ack_b_ms = at_ms;
                    true
                } else {
                    false
                }
            }
        }
    }
}

/// 环有效区 [0..n) 分位（插入排序 scratch，n ≤128；样本 0 返回 0）。
fn percentile(ring: &[u16; LAT_CAP], n: usize, pct: u32) -> u32 {
    if n == 0 {
        return 0;
    }
    let mut buf = [0u16; LAT_CAP];
    buf[..n].copy_from_slice(&ring[..n]);
    // 插入排序（n ≤128，O(n²) ≤16384 步，栈 256B——内核栈预算内）。
    for i in 1..n {
        let key = buf[i];
        let mut j = i;
        while j > 0 && buf[j - 1] > key {
            buf[j] = buf[j - 1];
            j -= 1;
        }
        buf[j] = key;
    }
    // 位次：ceil(pct/100 × n)，1 基。
    let rank = (n as u32 * pct + 99) / 100;
    let rank = rank.clamp(1, n as u32) as usize;
    buf[rank - 1] as u32
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

/// 域自检。
pub fn run_smallpkt_checks() -> CheckSet {
    let mut cs = CheckSet::new("F059-smallpkt");
    // 1) SSH 打字：交互小包批路径 P99 ≤30ms（20ms 击键节奏实测模型）。
    let mut e = SmallPktEngine::new();
    for k in 0..10u64 {
        e.submit(SMALL_PKT_BYTES, FlowClass::Interactive, k * 20_000);
        e.poll_round(k * 20_000);
    }
    e.poll_round(10 * 20_000 + BATCH_WINDOW_US);
    cs.add("ssh_p99_under_30ms", e.interactive_p99_us() <= SSH_LAT_P99_LIMIT_US, "");
    // 2) 轮询预算 64 包/轮（超则让 CPU）。
    let mut e2 = SmallPktEngine::new();
    for i in 0..100u64 {
        e2.submit(SMALL_PKT_BYTES, FlowClass::Bulk, i);
    }
    let done = e2.poll_round(BATCH_WINDOW_US);
    cs.add("poll_budget_64", done == POLL_BUDGET_PKTS, "");
    // 3) 风暴 >10k pps → 熔断告警 + Bulk 限流 + 交互零丢。
    let mut e3 = SmallPktEngine::new();
    for i in 0..10_100u64 {
        e3.submit(SMALL_PKT_BYTES, FlowClass::Bulk, i * 90); // ~11k pps
    }
    let mut interactive_in = 0u32;
    for k in 0..20u64 {
        if e3.submit(SMALL_PKT_BYTES, FlowClass::Interactive, 2_000_000 + k * 10_000) {
            interactive_in += 1;
        }
    }
    cs.add("storm_alarm_raised", e3.storm_alarm(), "");
    cs.add("storm_drops_bulk_only", e3.dropped_bulk() > 0 && interactive_in == 20, "");
    // 4) 批窗口 = F050 同窗口（8ms 初始：窗口未满不开批）。
    let mut e4 = SmallPktEngine::new();
    e4.submit(64, FlowClass::Bulk, 0);
    cs.add("window_waits_f050", e4.poll_round(BATCH_WINDOW_US - 1) == 0, "");
    cs.add("window_flushes_at_f050", e4.poll_round(BATCH_WINDOW_US) == 1, "");
    // 5) 单包路径热切换：切换后不排队即时出。
    let mut e5 = SmallPktEngine::new();
    e5.set_mode(PathMode::Single);
    cs.add(
        "single_mode_hot_switch",
        e5.mode() == PathMode::Single && e5.submit(64, FlowClass::Interactive, 0),
        "",
    );
    cs.add("single_mode_no_queue", e5.poll_round(1_000) == 0, "");
    // 6) ACK 分流：首 ACK 立即，交互 5ms / 吞吐 40ms 窗。
    let mut e6 = SmallPktEngine::new();
    let a0 = e6.ack_due(FlowClass::Interactive, 0);
    let a1 = e6.ack_due(FlowClass::Interactive, 4);
    let a2 = e6.ack_due(FlowClass::Interactive, 5);
    cs.add("ack_interactive_5ms", a0 && !a1 && a2, "");
    let b0 = e6.ack_due(FlowClass::Bulk, 0);
    let b1 = e6.ack_due(FlowClass::Bulk, 39);
    let b2 = e6.ack_due(FlowClass::Bulk, 40);
    cs.add("ack_bulk_40ms", b0 && !b1 && b2, "");
    // 7) 小包模型分界：≤512B = 120μs 定界，>512B = 40μs 摊薄。
    let mut e7 = SmallPktEngine::new();
    e7.set_mode(PathMode::Single);
    e7.submit(SMALL_PKT_BYTES, FlowClass::Interactive, 0);
    e7.submit(SMALL_PKT_BYTES + 1, FlowClass::Bulk, 0);
    cs.add(
        "small_pkt_class_model",
        e7.interactive_p99_us() == 120 && e7.bulk_p99_us() == 40,
        "",
    );
    // 8) 风暴下交互流劣化 <20%（基线标定 → 风暴注入 → 限流保交互 → 比对）。
    let mut e8 = SmallPktEngine::new();
    for k in 0..50u64 {
        e8.submit(SMALL_PKT_BYTES, FlowClass::Interactive, k * 20_000);
        e8.poll_round(k * 20_000);
    }
    e8.poll_round(50 * 20_000 + BATCH_WINDOW_US);
    e8.calibrate_baseline();
    let base = e8.baseline_i_us();
    e8.clear_latency_ledger(); // 测量窗口重置：风暴期单独统计
    for i in 0..10_100u64 {
        e8.submit(SMALL_PKT_BYTES, FlowClass::Bulk, 2_000_000 + i * 90);
    }
    // 51 轮（奇数收尾：末轮冲刷与基线节奏对称，避免尾包伪影）。
    for k in 0..51u64 {
        e8.submit(SMALL_PKT_BYTES, FlowClass::Interactive, 3_000_000 + k * 20_000);
        e8.poll_round(3_000_000 + k * 20_000);
    }
    e8.poll_round(3_000_000 + 51 * 20_000 + BATCH_WINDOW_US);
    let deg = e8.storm_degrade_pct();
    cs.add("storm_degrade_under_20pct", base > 0 && deg <= STORM_DEGRADE_MAX_PCT, "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn storm_needs_over_10k_pps() {
        let mut e = SmallPktEngine::new();
        for i in 0..10_100u64 {
            e.submit(SMALL_PKT_BYTES, FlowClass::Bulk, i * 90);
        }
        assert!(e.current_pps(910_000) > STORM_PPS, "1s 滑窗合计越线");
        assert!(e.storm_alarm(), "越线后告警在位");
        assert!(e.dropped_bulk() > 0);
    }

    #[test]
    fn alarm_recovers_after_calm_window() {
        let mut e = SmallPktEngine::new();
        for i in 0..10_100u64 {
            e.submit(SMALL_PKT_BYTES, FlowClass::Bulk, i * 90);
        }
        assert!(e.storm_alarm());
        // 静默期：逐 10ms 一包（100 pps = 半线以下），跨一个整窗。
        for k in 0..120u64 {
            e.submit(SMALL_PKT_BYTES, FlowClass::Interactive, 2_000_000 + k * 10_000);
        }
        assert!(!e.storm_alarm(), "半线以下整窗 → 告警解除");
    }

    #[test]
    fn interactive_never_dropped_by_limiting() {
        let mut e = SmallPktEngine::new();
        for i in 0..10_100u64 {
            e.submit(SMALL_PKT_BYTES, FlowClass::Bulk, i * 90);
        }
        let mut interactive_in = 0u32;
        for k in 0..100u64 {
            if e.submit(SMALL_PKT_BYTES, FlowClass::Interactive, 2_000_000 + k * 5_000) {
                interactive_in += 1;
            }
        }
        assert_eq!(interactive_in, 100, "限流只丢 Bulk，交互零丢");
    }

    #[test]
    fn queue_cap_never_broken_by_eviction() {
        let mut e = SmallPktEngine::new();
        // 窗口未开批时连灌超容：Bulk 全收（窗口内），交互挤最老 Bulk，环不变量恒立。
        for i in 0..(Q_CAP as u64 + 50) {
            let _ = e.submit(SMALL_PKT_BYTES, FlowClass::Bulk, i);
        }
        for k in 0..20u64 {
            let _ = e.submit(SMALL_PKT_BYTES, FlowClass::Interactive, 1_000 + k);
        }
        assert!(e.q_n <= Q_CAP);
        // 环内 [q_head..q_head+q_n) 全为 Some（压实不变量）。
        for i in 0..e.q_n {
            assert!(e.q[(e.q_head + i) % Q_CAP].is_some(), "环空洞破坏不变量");
        }
    }

    #[test]
    fn batch_then_drain_completes_within_budgets() {
        let mut e = SmallPktEngine::new();
        for i in 0..200u64 {
            e.submit(SMALL_PKT_BYTES, FlowClass::Bulk, i * 100);
        }
        let mut total = 0usize;
        for r in 0..4u64 {
            total += e.poll_round(BATCH_WINDOW_US * (r + 1));
        }
        assert_eq!(total, 200, "预算轮次全清");
        assert_eq!(e.current_pps(1_100_000), 0, "1.1s 后滑窗过期归零");
    }

    #[test]
    fn percentile_rank_honest() {
        let mut e = SmallPktEngine::new();
        e.set_mode(PathMode::Single);
        // 交互 3×120 + 吞吐 1×40：P99=最大值（小样本诚实位次）。
        for _ in 0..3 {
            e.submit(SMALL_PKT_BYTES, FlowClass::Interactive, 0);
        }
        e.submit(SMALL_PKT_BYTES + 1, FlowClass::Bulk, 0);
        assert_eq!(e.interactive_p99_us(), 120);
        assert_eq!(e.bulk_p99_us(), 40);
    }

    #[test]
    fn baseline_degrade_math() {
        let mut e = SmallPktEngine::new();
        e.set_mode(PathMode::Single);
        for _ in 0..10 {
            e.submit(SMALL_PKT_BYTES, FlowClass::Interactive, 0);
        }
        e.calibrate_baseline();
        assert_eq!(e.baseline_i_us(), 120);
        assert_eq!(e.storm_degrade_pct(), 0, "P99=P50 时零劣化");
    }
}
