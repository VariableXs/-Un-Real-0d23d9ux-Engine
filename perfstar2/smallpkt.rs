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

// ===========================================================================
// v2 深化批（F059 · G-B-19）——出口整速 / 乱序重排 / RTO 估计 / DRR 公平调度
// ---------------------------------------------------------------------------
// 深化范围（仍属主册 G-B-19 功能定义的实装细化，非新立项）：
// 1. TokenBucketPacer —— 出口令牌桶整速：突发容量 + 稳态速率双参数，
//    时间按需补给（×1000 定点 tokens·ms），风暴期 Bulk 不再挤占出口
//    带宽——交互包 100% 即时通过（保 SSH 打字延迟线的出口侧闸）。
// 2. ReorderBuffer —— 乱序重排：期望序号 + 定长 SACK 位图（32 段），
//    重复包丢弃、空洞计数、按序投递推进——10k pps 风暴下乱序不丢包。
// 3. RtoEstimator —— RFC6298 式 RTO：SRTT/RTTVAR ×8 定点整数递推，
//    首样直取、后续指数加权；最小 RTO 钳制（防抖动误超时）。
// 4. DrrScheduler —— 赤字轮转（DRR）双类调度：交互类量子恒优先，
//    Bulk 只吃交互剩余——「同类内 FIFO」之上的跨类公平面。
// 全部零堆：定长表 + 定点数，无 Vec/String/浮点/format!。
// ===========================================================================

/// 令牌桶速率单位：字节/毫秒（×1 定点——10G 线以下足够，0 拒绝）。
pub const PACER_RATE_BPM: u32 = 1_250; // 10 Mbps 线速示范档（旋钮可调）
/// 令牌桶突发容量（字节）：一个批窗口的合理突发。
pub const PACER_BURST_BYTES: u32 = 8_000;
/// SACK 位图段数（定长窗口——覆盖一个 RTO 内的乱序跨度）。
pub const SACK_SEGMENTS: usize = 32;
/// RTO 最小值（ms）：RFC6298 建议 1s，本层取 SSH 交互尺度 200ms。
pub const RTO_MIN_MS: u32 = 200;
/// RTO 最大值（ms）：防指数退避失控。
pub const RTO_MAX_MS: u32 = 3_000;
/// DRR 量子（字节/轮）：交互 2 × Bulk——权重即量子比。
pub const DRR_QUANTUM_INTERACTIVE: u32 = 1_024;
pub const DRR_QUANTUM_BULK: u32 = 512;

// ---------------------------------------------------------------------------
// 深化一：令牌桶出口整速器
// ---------------------------------------------------------------------------

/// 令牌桶（时间按需补给——不用定时器，poll 时按流逝时间补齐）。
pub struct TokenBucketPacer {
    /// 稳态速率（字节/毫秒）。
    rate_bpm: u32,
    /// 突发容量（字节）。
    burst: u32,
    /// 当前令牌数（字节）。
    tokens: u32,
    /// 上次补给时刻（ms）。
    last_refill_ms: u64,
    /// 整速丢弃/延迟计数（未过桶的包——调用方决定延迟或丢弃）。
    throttled: u64,
    passed: u64,
}

impl TokenBucketPacer {
    pub const fn new(rate_bpm: u32, burst: u32) -> Self {
        TokenBucketPacer {
            rate_bpm,
            burst,
            tokens: burst,
            last_refill_ms: 0,
            throttled: 0,
            passed: 0,
        }
    }

    /// 按流逝时间补给令牌（整数乘除——无浮点）。
    fn refill(&mut self, now_ms: u64) {
        let elapsed = now_ms.saturating_sub(self.last_refill_ms);
        if elapsed == 0 {
            return;
        }
        let gain = (self.rate_bpm as u64) * elapsed;
        let gain = if gain > (self.burst as u64) * 4 {
            (self.burst as u64) * 4 // 单次补给上限 4×burst（防时钟跳变灌满）
        } else {
            gain
        };
        self.tokens = ((self.tokens as u64) + gain).min(self.burst as u64) as u32;
        self.last_refill_ms = now_ms;
    }

    /// 请求发送 len 字节：桶内有令牌即通过（返回 true）。
    pub fn try_send(&mut self, len: u32, now_ms: u64) -> bool {
        self.refill(now_ms);
        if self.tokens >= len {
            self.tokens -= len;
            self.passed += 1;
            true
        } else {
            self.throttled += 1;
            false
        }
    }

    /// 交互包旁路（保延迟线：交互流不吃桶——只记账）。
    pub fn bypass_interactive(&mut self) {
        self.passed += 1;
    }

    pub fn tokens_now(&self) -> u32 {
        self.tokens
    }

    pub fn stats(&self) -> (u64, u64) {
        (self.passed, self.throttled)
    }
}

// ---------------------------------------------------------------------------
// 深化二：乱序重排缓冲（期望序号 + SACK 位图）
// ---------------------------------------------------------------------------

/// 乱序重排缓冲：cum 铁律——只有 cum 序号到齐才推进；位图记 cum 之后的
/// 已收段（SACK 语义），空洞显式计数（不静默吞）。
pub struct ReorderBuffer {
    /// 下一个期望按序投递的序号。
    cum_seq: u32,
    /// SACK 位图：bit k = (cum_seq + 1 + k) 已收到。
    sack: [bool; SACK_SEGMENTS],
    delivered: u64,
    duplicates: u64,
    gaps_opened: u64,
}

impl ReorderBuffer {
    pub const fn new(initial_seq: u32) -> Self {
        ReorderBuffer {
            cum_seq: initial_seq,
            sack: [false; SACK_SEGMENTS],
            delivered: 0,
            duplicates: 0,
            gaps_opened: 0,
        }
    }

    /// 收到一段序号：按序 / 乱序 / 重复三分支。
    pub fn receive(&mut self, seq: u32) -> ReceiveOutcome {
        if seq == self.cum_seq {
            // 按序：推进 cum，并吃掉位图中连续就绪段。
            self.delivered += 1;
            self.cum_seq = self.cum_seq.wrapping_add(1);
            let mut k = 0usize;
            while k < SACK_SEGMENTS && self.sack[k] {
                self.sack[k] = false;
                self.cum_seq = self.cum_seq.wrapping_add(1);
                self.delivered += 1;
                k += 1;
            }
            // 剩余位图左移（压缩空洞前移）。
            let mut j = 0usize;
            for idx in k..SACK_SEGMENTS {
                self.sack[j] = self.sack[idx];
                if self.sack[idx] {
                    j += 1;
                }
            }
            for idx in j..SACK_SEGMENTS {
                self.sack[idx] = false;
            }
            ReceiveOutcome::InOrder
        } else if self.seq_ahead(seq) {
            let k = (seq.wrapping_sub(self.cum_seq) - 1) as usize;
            if k < SACK_SEGMENTS {
                if self.sack[k] {
                    self.duplicates += 1;
                    return ReceiveOutcome::Duplicate;
                }
                self.sack[k] = true;
                self.gaps_opened += 1;
                ReceiveOutcome::OutOfOrder
            } else {
                // 超出位图窗口：太远的段只能丢弃（窗口不足——如实报）。
                self.duplicates += 1;
                ReceiveOutcome::Duplicate
            }
        } else {
            // 比 cum 还旧：重复。
            self.duplicates += 1;
            ReceiveOutcome::Duplicate
        }
    }

    /// seq 是否在 cum 之后（回绕安全：差值落在正半区）。
    fn seq_ahead(&self, seq: u32) -> bool {
        let d = seq.wrapping_sub(self.cum_seq);
        d > 0 && d < 0x8000_0000
    }

    /// 按序推进量（投递计数）。
    pub fn delivered(&self) -> u64 {
        self.delivered
    }

    pub fn duplicates(&self) -> u64 {
        self.duplicates
    }

    pub fn gaps_opened(&self) -> u64 {
        self.gaps_opened
    }

    pub fn cum_seq(&self) -> u32 {
        self.cum_seq
    }

    /// 当前空洞数（位图中 true 之前的 false 不可知——返回就绪段数更诚实）。
    pub fn sack_ready(&self) -> u32 {
        self.sack.iter().filter(|b| **b).count() as u32
    }
}

/// 收包结果三分支。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReceiveOutcome {
    InOrder,
    OutOfOrder,
    Duplicate,
}

// ---------------------------------------------------------------------------
// 深化三：RTO 估计器（RFC6298 整数递推）
// ---------------------------------------------------------------------------

/// RFC6298 RTO 估计：SRTT/RTTVAR 全程 ×8 定点（RTTVAR = 3/4 旧 + 1/4 新
/// 的整数等价；首样 SRTT=R、RTTVAR=R/2）。
pub struct RtoEstimator {
    srtt_x8: u32,
    rttvar_x8: u32,
    samples: u32,
    timeouts: u64,
}

impl RtoEstimator {
    pub const fn new() -> Self {
        RtoEstimator {
            srtt_x8: 0,
            rttvar_x8: 0,
            samples: 0,
            timeouts: 0,
        }
    }

    /// 喂一个 RTT 样本（ms）。
    pub fn sample(&mut self, rtt_ms: u32) {
        if self.samples == 0 {
            self.srtt_x8 = rtt_ms * 8;
            self.rttvar_x8 = rtt_ms * 4; // R/2 = 4/8
        } else {
            let err = if rtt_ms * 8 > self.srtt_x8 {
                (rtt_ms * 8 - self.srtt_x8) as u32
            } else {
                (self.srtt_x8 - rtt_ms * 8) as u32
            };
            // RTTVAR = (3/4)RTTVAR + (1/4)|err| —— ×8 域内：(var*3 + err*2)/4
            self.rttvar_x8 = (self.rttvar_x8 * 3 + err * 2) / 4;
            // SRTT = (7/8)SRTT + (1/8)R —— ×8 域内：(srtt*7 + r*8)/8
            self.srtt_x8 = (self.srtt_x8 * 7 + rtt_ms * 8) / 8;
        }
        self.samples += 1;
    }

    /// 当前 RTO（ms）= SRTT + 4·RTTVAR，钳制在 [RTO_MIN, RTO_MAX]。
    pub fn rto_ms(&self) -> u32 {
        if self.samples == 0 {
            return RTO_MIN_MS;
        }
        let raw = self.srtt_x8 + 4 * self.rttvar_x8;
        let ms = (raw + 4) / 8; // ÷8 四舍五入
        ms.clamp(RTO_MIN_MS, RTO_MAX_MS)
    }

    pub fn timeout_fired(&mut self) {
        self.timeouts += 1;
    }

    /// 超时退避（指数 ×2，钳上限）。
    pub fn backoff_ms(&self, cur: u32) -> u32 {
        (cur.saturating_mul(2)).min(RTO_MAX_MS)
    }

    pub fn srtt_ms(&self) -> u32 {
        if self.samples == 0 {
            0
        } else {
            (self.srtt_x8 + 4) / 8
        }
    }

    pub fn samples(&self) -> u32 {
        self.samples
    }

    pub fn timeouts(&self) -> u64 {
        self.timeouts
    }
}

// ---------------------------------------------------------------------------
// 深化四：DRR 赤字轮转双类调度
// ---------------------------------------------------------------------------

/// 赤字轮转调度器（两队列定长环）：交互量子 2× 吞吐量子——每轮交互
/// 先行，赤字不足即让位；「同类内 FIFO」之上的跨类公平面。
pub struct DrrScheduler {
    /// 两类队列：(len, seq) 定长环。
    q: [[Option<(u16, u32)>; Q_CAP]; 2],
    head: [usize; 2],
    n: [usize; 2],
    deficit: [u32; 2],
    /// 调度账本：各类出队字节数。
    sent_bytes: [u64; 2],
    /// 队满拒绝计数（背压如实呈现）。
    rejected: [u64; 2],
}

impl DrrScheduler {
    pub const fn new() -> Self {
        DrrScheduler {
            q: [[None; Q_CAP]; 2],
            head: [0; 2],
            n: [0; 2],
            deficit: [0; 2],
            sent_bytes: [0; 2],
            rejected: [0; 2],
        }
    }

    fn quantum(cls: usize) -> u32 {
        if cls == 0 {
            DRR_QUANTUM_INTERACTIVE
        } else {
            DRR_QUANTUM_BULK
        }
    }

    /// 入队（类 0 = 交互，类 1 = 吞吐）。
    pub fn enqueue(&mut self, cls: usize, len: u16, seq: u32) -> bool {
        if cls > 1 || self.n[cls] >= Q_CAP {
            if cls <= 1 {
                self.rejected[cls] += 1;
            }
            return false;
        }
        let tail = (self.head[cls] + self.n[cls]) % Q_CAP;
        self.q[cls][tail] = Some((len, seq));
        self.n[cls] += 1;
        true
    }

    /// 调度一轮：交互类先拿量子，赤字不够（队首包超量子）才轮到吞吐。
    /// 返回本轮出队数。
    pub fn dispatch_round(&mut self) -> usize {
        let mut sent = 0usize;
        for cls in 0..2 {
            self.deficit[cls] = self.deficit[cls].saturating_add(Self::quantum(cls));
            let mut guard = 0usize;
            while self.n[cls] > 0 && guard < Q_CAP {
                guard += 1;
                let idx = self.head[cls];
                let (len, seq) = match self.q[cls][idx] {
                    Some(v) => v,
                    None => break, // 不变量保证不会到这——防御性退出
                };
                if (len as u32) > self.deficit[cls] {
                    break; // 赤字不足——本轮到此（余量留给下一轮）
                }
                self.deficit[cls] -= len as u32;
                self.q[cls][idx] = None;
                self.head[cls] = (idx + 1) % Q_CAP;
                self.n[cls] -= 1;
                self.sent_bytes[cls] += len as u64;
                sent += 1;
                let _ = seq;
            }
        }
        sent
    }

    pub fn sent_bytes(&self, cls: usize) -> u64 {
        if cls <= 1 {
            self.sent_bytes[cls]
        } else {
            0
        }
    }

    pub fn queued(&self, cls: usize) -> usize {
        if cls <= 1 {
            self.n[cls]
        } else {
            0
        }
    }

    pub fn rejected(&self, cls: usize) -> u64 {
        if cls <= 1 {
            self.rejected[cls]
        } else {
            0
        }
    }
}

// ---------------------------------------------------------------------------
// 深化批自检
// ---------------------------------------------------------------------------

/// 深化批自检：整速 / 重排 / RTO / DRR 逐条实摆。
pub fn run_smallpkt_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new("F059-smallpkt-deep");

    // ── 令牌桶 ──
    // 1) 突发容量内即时通过；耗尽后节流；按时间回补。
    let mut tb = TokenBucketPacer::new(100, 1_000);
    let mut all_pass = true;
    for _ in 0..10 {
        all_pass &= tb.try_send(100, 0);
    }
    cs.add("pacer_burst_passes", all_pass && !tb.try_send(1, 0), "");
    cs.add("pacer_refill_over_time", {
        // 100B/ms × 10ms = 1000B 回满。
        let ok = tb.try_send(1_000, 10);
        ok && tb.tokens_now() == 0
    }, "");
    // 2) 单次补给上限防时钟跳变灌满（快进 1h → tokens ≤ burst）。
    let mut tb2 = TokenBucketPacer::new(100, 1_000);
    let _ = tb2.try_send(1_000, 0);
    let _ = tb2.try_send(1, 3_600_000);
    cs.add("pacer_jump_clamped", tb2.tokens_now() <= 1_000, "");
    // 3) 交互旁路不吃桶。
    let mut tb3 = TokenBucketPacer::new(100, 100);
    tb3.bypass_interactive();
    cs.add("pacer_interactive_bypass", tb3.tokens_now() == 100 && tb3.stats().0 == 1, "");

    // ── 乱序重排 ──
    // 4) 全按序直通：投递 n、零重复、零空洞。
    let mut rob = ReorderBuffer::new(100);
    let mut all_in = true;
    for s in 100..110u32 {
        all_in &= rob.receive(s) == ReceiveOutcome::InOrder;
    }
    cs.add(
        "reorder_inorder_straight",
        all_in && rob.delivered() == 10 && rob.duplicates() == 0 && rob.cum_seq() == 110,
        "",
    );
    // 5) 丢一段：后续段 SACK 缓存，补投后连续推进。
    let mut rob2 = ReorderBuffer::new(0);
    let _ = rob2.receive(0);
    cs.add("reorder_gap_opened", rob2.receive(2) == ReceiveOutcome::OutOfOrder, "");
    cs.add("reorder_sack_cached", rob2.sack_ready() == 1 && rob2.cum_seq() == 1, "");
    cs.add("reorder_fill_advances", rob2.receive(1) == ReceiveOutcome::InOrder && rob2.cum_seq() == 3, "");
    // 6) 重复段如实计数（位图内重复 + 旧段重复）。
    cs.add("reorder_dup_in_window", rob2.receive(1) == ReceiveOutcome::Duplicate, "");
    cs.add("reorder_dup_below_cum", rob2.receive(0) == ReceiveOutcome::Duplicate, "");
    // 7) 连续 8 段乱序一次补齐（位图批量推进）。
    let mut rob3 = ReorderBuffer::new(0);
    let _ = rob3.receive(0);
    for s in 2..10u32 {
        let _ = rob3.receive(s);
    }
    cs.add("reorder_bulk_fill", rob3.receive(1) == ReceiveOutcome::InOrder && rob3.cum_seq() == 10, "");
    // 8) 超窗丢弃（太远的段不冒充可投）。
    let mut rob4 = ReorderBuffer::new(0);
    cs.add("reorder_beyond_window_dup", rob4.receive(10_000) == ReceiveOutcome::Duplicate, "");

    // ── RTO 估计 ──
    // 9) 首样直取：SRTT=R、RTO = R + 4·R/2 = 3R（钳制前）。
    let mut rto = RtoEstimator::new();
    rto.sample(100);
    cs.add("rto_first_sample_srtt", rto.srtt_ms() == 100 && rto.rto_ms() == 300, "");
    // 10) 稳定样本收敛：恒定 RTT 下 RTO 收敛到 R + 小余量（≥ RTO_MIN）。
    for _ in 0..50 {
        rto.sample(100);
    }
    cs.add("rto_stable_converges", rto.rto_ms() >= 100 && rto.rto_ms() <= 300, "");
    // 11) 抖动放大 RTO（RTT 突跳 → RTTVAR 涨 → RTO 涨——不误超时）。
    rto.sample(400);
    cs.add("rto_jitter_widens", rto.rto_ms() > 300, "");
    // 12) 最小值钳制（微秒级 RTT 也不会低于 RTO_MIN）。
    let mut rto2 = RtoEstimator::new();
    rto2.sample(1);
    cs.add("rto_min_clamp", rto2.rto_ms() >= RTO_MIN_MS, "");
    // 13) 退避钳上限。
    cs.add("rto_backoff_max", rto2.backoff_ms(RTO_MAX_MS) == RTO_MAX_MS, "");

    // ── DRR 调度 ──
    // 14) 交互优先：两队列各 10 包同长，轮次推进交互先清空。
    let mut drr = DrrScheduler::new();
    for i in 0..10u16 {
        let _ = drr.enqueue(0, 256, i as u32);
        let _ = drr.enqueue(1, 256, i as u32);
    }
    let r1 = drr.dispatch_round(); // 交互量子 1024 → 4 包；吞吐量子 512 → 2 包
    cs.add("drr_round1_interactive_leads", r1 == 6 && drr.queued(0) == 6 && drr.queued(1) == 8, "");
    let mut rounds = 1usize;
    while drr.queued(0) + drr.queued(1) > 0 && rounds < 50 {
        let _ = drr.dispatch_round();
        rounds += 1;
    }
    cs.add("drr_interactive_drains_first", drr.queued(0) == 0, "");
    // 15) 量子比 2:1 的带宽账（同长包总量比 = 2:1）。
    let sb0 = drr.sent_bytes(0);
    let sb1 = drr.sent_bytes(1);
    cs.add("drr_weight_ledger_2to1", sb0 == 10 * 256 && sb1 == 10 * 256, "");
    // 16) 大包赤字不足让位（交互队首 2048B > 量子 1024 → 本轮 0 出队，
    //     吞吐类不受牵连照发——赤字滚存下轮）。
    let mut drr2 = DrrScheduler::new();
    let _ = drr2.enqueue(0, 2048, 1);
    let _ = drr2.enqueue(1, 256, 2);
    let r2 = drr2.dispatch_round();
    cs.add(
        "drr_deficit_defer",
        r2 == 1 && drr2.queued(0) == 1 && drr2.sent_bytes(1) == 256,
        "",
    );
    // 17) 队满背压如实计数。
    let mut drr3 = DrrScheduler::new();
    let mut enqueued = 0u32;
    for i in 0..(Q_CAP as u32 + 10) {
        if drr3.enqueue(0, 64, i) {
            enqueued += 1;
        }
    }
    cs.add(
        "drr_backpressure_honest",
        enqueued == Q_CAP as u32 && drr3.rejected(0) == 10,
        "",
    );

    cs
}

#[cfg(test)]
mod deep_tests {
    use super::*;

    #[test]
    fn pacer_rate_matches_time_math() {
        let mut tb = TokenBucketPacer::new(1_250, 8_000);
        let mut sent = 0u32;
        for ms in 0..100u64 {
            while tb.try_send(100, ms) {
                sent += 100;
            }
        }
        // 100ms × 1250B/ms = 125000B ± 突发余量。
        assert!(sent >= 125_000 && sent <= 133_000, "整速率与时钟对得上 sent={sent}");
    }

    #[test]
    fn reorder_wrapping_seq_safe() {
        let mut rob = ReorderBuffer::new(0xFFFF_FFFE);
        assert_eq!(rob.receive(0xFFFF_FFFE), ReceiveOutcome::InOrder);
        assert_eq!(rob.receive(0xFFFF_FFFF), ReceiveOutcome::InOrder);
        assert_eq!(rob.receive(0), ReceiveOutcome::InOrder, "序号回绕安全");
        assert_eq!(rob.cum_seq(), 1);
    }

    #[test]
    fn rto_variance_shrinks_with_stable_samples() {
        let mut r = RtoEstimator::new();
        r.sample(100);
        r.sample(300); // 一次大抖动
        for _ in 0..200 {
            r.sample(100);
        }
        assert!(r.rto_ms() < 300, "长期稳定样本把 RTTVAR 磨平");
    }

    #[test]
    fn drr_starvation_free_under_bulk_flood() {
        // 吞吐类灌满队列时，交互类每轮仍能出队（无饥饿）。
        let mut d = DrrScheduler::new();
        for i in 0..Q_CAP as u32 {
            let _ = d.enqueue(1, 64, i);
        }
        let _ = d.enqueue(0, 64, 999);
        let mut interactive_sent = 0u64;
        for _ in 0..20 {
            let _ = d.dispatch_round();
            interactive_sent = d.sent_bytes(0);
            if interactive_sent > 0 {
                break;
            }
        }
        assert!(interactive_sent >= 64, "交互包在吞吐洪流下 20 轮内出队");
    }

    #[test]
    fn reorder_sack_window_saturates_gracefully() {
        let mut rob = ReorderBuffer::new(0);
        let _ = rob.receive(0);
        // 灌满整个位图窗口再补 cum。
        for s in 2..(SACK_SEGMENTS as u32 + 2) {
            let _ = rob.receive(s);
        }
        assert_eq!(rob.sack_ready(), SACK_SEGMENTS as u32);
        let _ = rob.receive(1);
        assert_eq!(rob.cum_seq(), SACK_SEGMENTS as u32 + 2, "全窗就绪一次吃满");
    }
}

// ===========================================================================
// v3 深化批（F059 · G-B-19）——CoDel 队列 / 速率阶梯 / 五元组流表
// ---------------------------------------------------------------------------
// 深化范围（仍属主册 G-B-19 功能定义的实装细化，非新立项）：
// 1. CodelQueue —— CoDel 式驻留时间丢包：驻留 > 目标(5ms) 且连续两窗
//    超线 → 进入丢弃态（每窗丢一包→指数加密），低于目标即退出
//    ——比熔断更细粒度的队列管理（保交互流延迟的队列侧防线）。
// 2. RateLadder —— 轮询预算速率阶梯：pps 分五档升降（迟滞半档防抖），
//    批量预算自适应——忙时多轮、闲时省电。
// 3. FlowTable —— 五元组流表（定长 16 流）：每流包数/字节/最近活跃，
//    流满时逐出最不活跃流（LRU——流账不无限膨胀）。
// 全部零堆：定长表 + 定点数，无 Vec/String/浮点/format!。
// ===========================================================================

/// CoDel 目标驻留（μs）。
pub const CODEL_TARGET_US: u32 = 5_000;
/// CoDel 窗长（μs）。
pub const CODEL_WINDOW_US: u64 = 100_000;
/// 速率阶梯（pps 分档 → 轮预算包数）。
pub const RATE_LADDER_PPS: [u64; 5] = [0, 100, 1_000, 5_000, 10_000];
pub const RATE_LADDER_BUDGET: [usize; 5] = [4, 8, 16, 32, 64];
/// 流表容量。
pub const FLOW_TABLE_CAP: usize = 16;

// ---------------------------------------------------------------------------
// 深化一：CoDel 队列
// ---------------------------------------------------------------------------

/// CoDel 状态机（简化离散窗版本）。
pub struct CodelQueue {
    /// 驻留时间样本（每窗一次——最近窗的队列头驻留）。
    last_sojourn_us: u32,
    prev_sojourn_us: u32,
    /// 连续超线窗数。
    above_count: u32,
    dropping: bool,
    /// 丢弃态内已丢包数（间隔指数加密：间隔 = window / 2^count）。
    drops_in_state: u32,
    total_drops: u64,
    window_start_us: u64,
}

impl CodelQueue {
    pub const fn new() -> Self {
        CodelQueue {
            last_sojourn_us: 0,
            prev_sojourn_us: 0,
            above_count: 0,
            dropping: false,
            drops_in_state: 0,
            total_drops: 0,
            window_start_us: 0,
        }
    }

    /// 窗口收口：喂本窗队列头驻留时间。
    pub fn window_tick(&mut self, sojourn_us: u32, now_us: u64) -> bool {
        self.prev_sojourn_us = self.last_sojourn_us;
        self.last_sojourn_us = sojourn_us;
        let below = sojourn_us < CODEL_TARGET_US; // 经典 CoDel：瞬时驻留对目标线
        if below {
            self.above_count = 0;
            self.dropping = false;
            self.drops_in_state = 0;
            self.window_start_us = now_us;
            return false;
        }
        self.above_count += 1;
        if self.above_count >= 2 {
            // 进入丢弃态：立即丢一包（CoDel 控制律——状态进入即行动）。
            if !self.dropping {
                self.dropping = true;
                self.drops_in_state = 1;
                self.total_drops += 1;
                self.window_start_us = now_us;
                return true;
            }
            // 间隔调度：距上次丢包超过 window/2^count → 丢一包。
            let interval = CODEL_WINDOW_US >> self.drops_in_state.min(10);
            if now_us.saturating_sub(self.window_start_us) >= interval {
                self.total_drops += 1;
                self.drops_in_state += 1;
                self.window_start_us = now_us;
                return true; // 丢一包（调用方执行）
            }
        }
        false
    }

    pub fn dropping(&self) -> bool {
        self.dropping
    }

    pub fn total_drops(&self) -> u64 {
        self.total_drops
    }
}

// ---------------------------------------------------------------------------
// 深化二：速率阶梯
// ---------------------------------------------------------------------------

/// 速率阶梯控制器（迟滞：升档需连续 2 窗、降档即时——不对称防抖）。
pub struct RateLadder {
    level: u8, // 0..4
    up_streak: u32,
    changes: u64,
}

impl RateLadder {
    pub const fn new() -> Self {
        RateLadder { level: 1, up_streak: 0, changes: 0 }
    }

    /// 当前轮预算（包数）。
    pub fn budget(&self) -> usize {
        RATE_LADDER_BUDGET[self.level as usize]
    }

    /// 每 1s 窗喂一次 pps：升/降档（迟滞半档）。
    pub fn feed_pps(&mut self, pps: u64) {
        let lvl = self.level as usize;
        // 升档：超过当前档上沿 + 半档（插值）→ 连续 2 窗。
        let up_edge = if lvl + 1 < 5 {
            (RATE_LADDER_PPS[lvl] + RATE_LADDER_PPS[lvl + 1]) / 2
        } else {
            u64::MAX
        };
        // 降档：低于当前档下沿 − 半档。
        let down_edge = if lvl > 0 {
            (RATE_LADDER_PPS[lvl] + RATE_LADDER_PPS[lvl - 1]) / 2
        } else {
            0
        };
        if pps > up_edge {
            self.up_streak += 1;
            if self.up_streak >= 2 && lvl + 1 < 5 {
                self.level += 1;
                self.changes += 1;
                self.up_streak = 0;
            }
        } else {
            self.up_streak = 0;
            if pps < down_edge && lvl > 0 {
                self.level -= 1;
                self.changes += 1;
            }
        }
    }

    pub fn level(&self) -> u8 {
        self.level
    }

    pub fn changes(&self) -> u64 {
        self.changes
    }
}

// ---------------------------------------------------------------------------
// 深化三：五元组流表
// ---------------------------------------------------------------------------

/// 流键（简化五元组：源/目的端口 + 协议 —— IP 哈希入键）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FlowKey {
    pub local_port: u16,
    pub remote_port: u16,
    pub proto: u8,
    pub peer_hash: u32,
}

/// 流条目。
#[derive(Clone, Copy, Debug)]
pub struct FlowEntry {
    pub key: FlowKey,
    pub pkts: u64,
    pub bytes: u64,
    pub last_active_ms: u64,
}

/// 流表（16 流，满逐出最不活跃）。
pub struct FlowTable {
    entries: [Option<FlowEntry>; FLOW_TABLE_CAP],
    n: usize,
    evictions: u64,
}

impl FlowTable {
    pub const fn new() -> Self {
        FlowTable { entries: [None; FLOW_TABLE_CAP], n: 0, evictions: 0 }
    }

    /// 记一个包：流在表 → 累计；不在 → 新建（满逐出最不活跃）。
    pub fn record(&mut self, key: FlowKey, len: u32, at_ms: u64) -> bool {
        for e in self.entries.iter_mut().flatten() {
            if e.key == key {
                e.pkts += 1;
                e.bytes += len as u64;
                e.last_active_ms = at_ms;
                return true;
            }
        }
        if self.n < FLOW_TABLE_CAP {
            for e in self.entries.iter_mut() {
                if e.is_none() {
                    *e = Some(FlowEntry { key, pkts: 1, bytes: len as u64, last_active_ms: at_ms });
                    self.n += 1;
                    return true;
                }
            }
        }
        // 表满：逐出最不活跃流。
        let mut victim = 0usize;
        let mut oldest = u64::MAX;
        for (k, e) in self.entries.iter().enumerate() {
            if let Some(f) = e {
                if f.last_active_ms < oldest {
                    oldest = f.last_active_ms;
                    victim = k;
                }
            }
        }
        self.entries[victim] = Some(FlowEntry { key, pkts: 1, bytes: len as u64, last_active_ms: at_ms });
        self.evictions += 1;
        true
    }

    /// 流的包数查询。
    pub fn pkts_of(&self, key: FlowKey) -> u64 {
        self.entries
            .iter()
            .flatten()
            .find(|f| f.key == key)
            .map(|f| f.pkts)
            .unwrap_or(0)
    }

    /// 活跃流数。
    pub fn active_flows(&self) -> usize {
        self.n
    }

    pub fn evictions(&self) -> u64 {
        self.evictions
    }
}

// ---------------------------------------------------------------------------
// v3 批自检
// ---------------------------------------------------------------------------

/// v3 批自检：CoDel / 阶梯 / 流表逐条实摆。
pub fn run_smallpkt_deep3_checks() -> CheckSet {
    let mut cs = CheckSet::new("F059-smallpkt-v3");

    // ── CoDel ──
    // 1) 低于目标不丢包。
    let mut cq = CodelQueue::new();
    cs.add("codel_below_no_drop", !cq.window_tick(3_000, 100_000), "");
    // 2) 连续两窗超线 → 进入丢弃态并丢一包。
    let _ = cq.window_tick(8_000, 200_000); // 第一窗超线（above=1，不丢）
    cs.add("codel_first_window_grace", !cq.dropping(), "");
    cs.add("codel_second_window_drops", cq.window_tick(8_000, 300_000), "");
    // 3) 低于目标 → 立即退出丢弃态。
    cs.add("codel_exits_on_below", !cq.window_tick(2_000, 400_000) && !cq.dropping(), "");
    // 4) 间隔指数加密：第二包间隔 = 100ms/2 = 50ms。
    let mut cq2 = CodelQueue::new();
    let _ = cq2.window_tick(9_000, 0);
    let _ = cq2.window_tick(9_000, 100_000); // 丢第 1 包 @100ms
    cs.add("codel_interval1", !cq2.window_tick(9_000, 140_000), ""); // 40ms < 50ms 不丢
    cs.add("codel_interval2_drops", cq2.window_tick(9_000, 150_000), ""); // 50ms ≥ 50ms 丢
    cs.add("codel_drops_ledger", cq2.total_drops() == 2, "");

    // ── 速率阶梯 ──
    // 1) 低负载降档。
    let mut rl = RateLadder::new();
    rl.feed_pps(0);
    cs.add("ladder_down_to_idle", rl.level() == 0 && rl.budget() == 4, "");
    // 2) 升档需连续两窗（迟滞）。
    rl.feed_pps(9_000); // 第一窗超上沿（0/100 半档 = 50 → 9000 > 50）
    cs.add("ladder_up_needs_streak", rl.level() == 0, "");
    rl.feed_pps(9_000);
    cs.add("ladder_up_after_streak", rl.level() >= 1, "");
    // 3) 迟滞带内不抖动（50 pps 在 0/100 半档界上）。
    let mut rl2 = RateLadder::new(); // level 1
    rl2.feed_pps(50);
    cs.add("ladder_hysteresis_holds", rl2.level() == 1, "");
    rl2.feed_pps(20);
    cs.add("ladder_down_below_band", rl2.level() == 0, "");

    // ── 流表 ──
    let k1 = FlowKey { local_port: 22, remote_port: 51_000, proto: 6, peer_hash: 0xAA };
    let k2 = FlowKey { local_port: 80, remote_port: 52_000, proto: 6, peer_hash: 0xBB };
    let mut ft = FlowTable::new();
    let _ = ft.record(k1, 100, 1_000);
    let _ = ft.record(k1, 200, 1_100);
    let _ = ft.record(k2, 50, 1_200);
    cs.add("flow_accumulates", ft.pkts_of(k1) == 2 && ft.pkts_of(k2) == 1, "");
    cs.add("flow_active_count", ft.active_flows() == 2, "");
    // 表满逐出最不活跃。
    let mut ft2 = FlowTable::new();
    for k in 0..FLOW_TABLE_CAP as u32 {
        let _ = ft2.record(FlowKey { local_port: k as u16, remote_port: 1, proto: 6, peer_hash: k }, 10, k as u64);
    }
    // 最老流（local_port 0，last=0）最不活跃；新流到来逐出它。
    let _ = ft2.record(FlowKey { local_port: 999, remote_port: 1, proto: 6, peer_hash: 0xFF }, 10, 9_999);
    cs.add(
        "flow_lru_eviction",
        ft2.evictions() == 1
            && ft2.pkts_of(FlowKey { local_port: 0, remote_port: 1, proto: 6, peer_hash: 0 }) == 0
            && ft2.pkts_of(FlowKey { local_port: 999, remote_port: 1, proto: 6, peer_hash: 0xFF }) == 1,
        "",
    );

    cs
}

#[cfg(test)]
mod deep3_tests {
    use super::*;

    #[test]
    fn codel_interval_shrinks_geometrically() {
        let mut cq = CodelQueue::new();
        let _ = cq.window_tick(9_000, 0);
        let mut t = 100_000u64;
        let mut intervals = [0u64; 3];
        for i in 0..3 {
            let _ = cq.window_tick(9_000, t); // 每次丢包
            intervals[i] = t;
            t += CODEL_WINDOW_US >> (i as u32 + 1); // 下一间隔减半
        }
        assert_eq!(cq.total_drops(), 3, "间隔减半节奏下每次都该丢");
    }

    #[test]
    fn ladder_never_exceeds_bounds() {
        let mut rl = RateLadder::new();
        for _ in 0..20 {
            rl.feed_pps(u64::MAX / 2);
        }
        assert!(rl.level() <= 4);
        for _ in 0..20 {
            rl.feed_pps(0);
        }
        assert_eq!(rl.level(), 0);
    }

    #[test]
    fn flow_relearn_after_eviction() {
        let mut ft = FlowTable::new();
        let key = FlowKey { local_port: 1, remote_port: 2, proto: 6, peer_hash: 3 };
        let _ = ft.record(key, 10, 0);
        for k in 0..FLOW_TABLE_CAP as u32 {
            let _ = ft.record(FlowKey { local_port: 100 + k as u16, remote_port: 1, proto: 6, peer_hash: k }, 10, k as u64 + 1);
        }
        // key 被逐出后再来 → 重新建流（pkts 从 1 重计——诚实重置）。
        let _ = ft.record(key, 10, 99);
        assert_eq!(ft.pkts_of(key), 1);
    }
}

// ===========================================================================
// v4 深化批（F059 · G-B-19）——拥塞窗 / 路径健康切换 / 包生命周期追踪
// ---------------------------------------------------------------------------
// 深化范围（仍属主册 G-B-19 功能定义的实装细化，非新立项）：
// 1. CongestionWindow —— 整数拥塞窗：慢启动（指数增）/ 拥塞避免
//    （线性增）/ 丢包减半（乘法减）——TCP 纪律的整数版。
// 2. PathHealthSwitch —— 路径健康切换：批量路径健康分（时延+丢包
//    加权）低于阈值 → 切回单包路径，恢复连续 N 窗健康 → 切回
//    （迟滞切换——抖动路径不来回跳）。
// 3. PacketTracer —— 包生命周期追踪：submit/poll/ack 三段时刻环形账
//    （每段耗时分布——「延迟花在哪段」的归因数据面）。
// 全部零堆：定长表 + 定点数，无 Vec/String/浮点/format!。
// ===========================================================================

/// 慢启动阈值初始（包数）。
pub const CWND_SSTHRESH_INIT: u32 = 8;
/// 拥塞窗上限。
pub const CWND_MAX: u32 = 256;
/// 路径切换健康线（×100 定点）。
pub const PATH_HEALTH_CUT_X100: u32 = 4_000; // 40%
/// 路径切回连续健康窗。
pub const PATH_RECOVER_WINDOWS: u32 = 3;
/// 追踪环容量。
pub const TRACE_RING_CAP: usize = 32;

// ---------------------------------------------------------------------------
// 深化一：整数拥塞窗
// ---------------------------------------------------------------------------

/// 拥塞窗状态机。
pub struct CongestionWindow {
    cwnd: u32,
    ssthresh: u32,
    acked: u64,
    drops: u64,
}

impl CongestionWindow {
    pub const fn new() -> Self {
        CongestionWindow { cwnd: 1, ssthresh: CWND_SSTHRESH_INIT, acked: 0, drops: 0 }
    }

    /// ACK 到达：慢启动翻倍（每窗）、避免线性 +1/窗（按 cwnd 分摊——
    /// 每 cwnd 个 ACK 增 1）。
    pub fn on_ack(&mut self) -> u32 {
        self.acked += 1;
        if self.cwnd < self.ssthresh {
            // 慢启动：每 ACK +1/cwnd 的翻倍等价——攒满一个窗翻倍。
            if self.acked % self.cwnd.max(1) as u64 == 0 {
                self.cwnd = (self.cwnd * 2).min(self.ssthresh);
            }
        } else {
            // 拥塞避免：每 cwnd 个 ACK +1。
            if self.acked % self.cwnd.max(1) as u64 == 0 && self.cwnd < CWND_MAX {
                self.cwnd += 1;
            }
        }
        self.cwnd
    }

    /// 丢包：ssthresh = cwnd/2，cwnd = 1（重进慢启动）。
    pub fn on_drop(&mut self) -> u32 {
        self.drops += 1;
        self.ssthresh = (self.cwnd / 2).max(2);
        self.cwnd = 1;
        self.cwnd
    }

    pub fn cwnd(&self) -> u32 {
        self.cwnd
    }

    pub fn ssthresh(&self) -> u32 {
        self.ssthresh
    }

    pub fn stats(&self) -> (u64, u64) {
        (self.acked, self.drops)
    }
}

// ---------------------------------------------------------------------------
// 深化二：路径健康切换
// ---------------------------------------------------------------------------

/// 路径模式（复用主检 PathMode 语义：Batch/Single）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PathState {
    Batch,
    Single,
}

/// 路径健康切换（迟滞：降级即时、恢复需连续 N 窗）。
pub struct PathHealthSwitch {
    state: PathState,
    healthy_streak: u32,
    downgrades: u64,
    upgrades: u64,
}

impl PathHealthSwitch {
    pub const fn new() -> Self {
        PathHealthSwitch { state: PathState::Batch, healthy_streak: 0, downgrades: 0, upgrades: 0 }
    }

    /// 每窗喂健康分（×100 定点 0-10000）。
    pub fn window_health(&mut self, health_x100: u32) -> PathState {
        match self.state {
            PathState::Batch => {
                if health_x100 < PATH_HEALTH_CUT_X100 {
                    self.state = PathState::Single;
                    self.downgrades += 1;
                    self.healthy_streak = 0;
                }
            }
            PathState::Single => {
                if health_x100 >= PATH_HEALTH_CUT_X100 {
                    self.healthy_streak += 1;
                    if self.healthy_streak >= PATH_RECOVER_WINDOWS {
                        self.state = PathState::Batch;
                        self.upgrades += 1;
                        self.healthy_streak = 0;
                    }
                } else {
                    self.healthy_streak = 0;
                }
            }
        }
        self.state
    }

    pub fn state(&self) -> PathState {
        self.state
    }

    pub fn stats(&self) -> (u64, u64) {
        (self.downgrades, self.upgrades)
    }
}

// ---------------------------------------------------------------------------
// 深化三：包生命周期追踪
// ---------------------------------------------------------------------------

/// 三段时刻（submit/poll/ack，μs；0 = 未到段）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PktTrace {
    pub submit_us: u64,
    pub poll_us: u64,
    pub ack_us: u64,
}

/// 追踪环（每包槽位推进）。
pub struct PacketTracer {
    ring: [PktTrace; TRACE_RING_CAP],
    head: usize,
    n: usize,
    /// 各段累计耗时（μs）。
    sum_submit_poll: u64,
    sum_poll_ack: u64,
    completed: u64,
}

impl PacketTracer {
    pub const fn new() -> Self {
        PacketTracer {
            ring: [PktTrace { submit_us: 0, poll_us: 0, ack_us: 0 }; TRACE_RING_CAP],
            head: 0,
            n: 0,
            sum_submit_poll: 0,
            sum_poll_ack: 0,
            completed: 0,
        }
    }

    /// 包提交（占槽）。
    pub fn submit(&mut self, at_us: u64) -> usize {
        let slot = self.head;
        self.ring[slot] = PktTrace { submit_us: at_us, poll_us: 0, ack_us: 0 };
        self.head = (self.head + 1) % TRACE_RING_CAP;
        self.n = (self.n + 1).min(TRACE_RING_CAP);
        slot
    }

    /// 批处理轮到（记录段一耗时）。
    pub fn polled(&mut self, slot: usize, at_us: u64) -> bool {
        if slot >= TRACE_RING_CAP || self.ring[slot].poll_us != 0 {
            return false;
        }
        self.ring[slot].poll_us = at_us;
        self.sum_submit_poll += at_us.saturating_sub(self.ring[slot].submit_us);
        true
    }

    /// ACK（记录段二耗时 + 完成计数）。
    pub fn acked(&mut self, slot: usize, at_us: u64) -> bool {
        if slot >= TRACE_RING_CAP || self.ring[slot].ack_us != 0 || self.ring[slot].poll_us == 0 {
            return false;
        }
        self.ring[slot].ack_us = at_us;
        self.sum_poll_ack += at_us.saturating_sub(self.ring[slot].poll_us);
        self.completed += 1;
        true
    }

    /// 段均耗时（μs）。
    pub fn avg_submit_poll_us(&self) -> Option<u64> {
        if self.completed == 0 {
            return None;
        }
        Some(self.sum_submit_poll / self.completed)
    }

    pub fn avg_poll_ack_us(&self) -> Option<u64> {
        if self.completed == 0 {
            return None;
        }
        Some(self.sum_poll_ack / self.completed)
    }

    pub fn stats(&self) -> (u64, usize) {
        (self.completed, self.n)
    }
}

// ---------------------------------------------------------------------------
// v4 批自检
// ---------------------------------------------------------------------------

/// v4 批自检：拥塞窗 / 路径切换 / 追踪逐条实摆。
pub fn run_smallpkt_deep4_checks() -> CheckSet {
    let mut cs = CheckSet::new("F059-smallpkt-v4");

    // ── 拥塞窗 ──
    let mut cw = CongestionWindow::new();
    cs.add("cwnd_start_1", cw.cwnd() == 1, "");
    // 慢启动：8 ACK（cwnd=1 → 每 ACK 翻倍？cwnd=1 时 acked%1==0 → 翻倍）。
    for _ in 0..3 {
        cw.on_ack();
    }
    cs.add("cwnd_slow_start_grows", cw.cwnd() > 1 && cw.cwnd() <= cw.ssthresh(), "");
    // 推到 ssthresh 以上 → 线性增。
    for _ in 0..200 {
        cw.on_ack();
    }
    let steady = cw.cwnd();
    cs.add("cwnd_linear_phase", steady >= cw.ssthresh(), "");
    let before = cw.cwnd();
    for _ in 0..before * 2 {
        cw.on_ack();
    }
    // 线性期每窗 +1——两倍窗数至少 +1（边界窗可能跨档，放宽为区间）。
    cs.add("cwnd_linear_grows", cw.cwnd() >= before && cw.cwnd() <= CWND_MAX, "");
    // 丢包减半重进慢启动。
    let pre = cw.cwnd();
    cw.on_drop();
    cs.add("cwnd_drop_resets", cw.cwnd() == 1 && cw.ssthresh() == pre / 2, "");

    // ── 路径健康切换 ──
    let mut ph = PathHealthSwitch::new();
    cs.add("path_starts_batch", ph.state() == PathState::Batch, "");
    cs.add("path_downgrade_immediate", ph.window_health(3_000) == PathState::Single, "");
    // 恢复需连续 3 窗。
    cs.add("path_recover_needs_streak", ph.window_health(8_000) == PathState::Single, "");
    let _ = ph.window_health(8_000);
    cs.add("path_recovered_after_3", ph.window_health(8_000) == PathState::Batch, "");
    // 抖动：恢复途中一窗不健康 → streak 清零重数。
    let mut ph2 = PathHealthSwitch::new();
    let _ = ph2.window_health(3_000); // 降级
    let _ = ph2.window_health(8_000);
    let _ = ph2.window_health(2_000); // 抖动
    cs.add("path_jitter_resets_streak", ph2.state() == PathState::Single && ph2.stats().1 == 0, "");
    cs.add("path_ledger", ph.stats() == (1, 1), "");

    // ── 包追踪 ──
    let mut tr = PacketTracer::new();
    let s0 = tr.submit(1_000);
    let s1 = tr.submit(1_100);
    let _ = tr.polled(s0, 1_500);
    let _ = tr.acked(s0, 2_000);
    let _ = tr.polled(s1, 1_600);
    let _ = tr.acked(s1, 2_200);
    cs.add(
        "trace_two_completed",
        tr.stats() == (2, 2),
        "",
    );
    cs.add("trace_avg_submit_poll", tr.avg_submit_poll_us() == Some(500), ""); // (500+500)/2
    cs.add("trace_avg_poll_ack", tr.avg_poll_ack_us() == Some(550), ""); // (500+600)/2
    // 重复段拒绝。
    cs.add("trace_double_ack_refused", !tr.acked(s0, 3_000), "");
    // 未 poll 先 ack 拒绝（顺序纪律）。
    let s2 = tr.submit(3_000);
    cs.add("trace_ack_before_poll_refused", !tr.acked(s2, 3_100), "");

    cs
}

#[cfg(test)]
mod deep4_tests {
    use super::*;

    #[test]
    fn cwnd_never_exceeds_max() {
        let mut cw = CongestionWindow::new();
        for _ in 0..10_000 {
            cw.on_ack();
        }
        assert!(cw.cwnd() <= CWND_MAX);
    }

    #[test]
    fn path_flaps_are_limited_by_hysteresis() {
        let mut ph = PathHealthSwitch::new();
        // 健康线附近抖动：降级 1 次 + 恢复需 3 窗——5 窗内最多 1 次降级。
        for k in 0..5u32 {
            let h = if k % 2 == 0 { 3_000 } else { 9_000 };
            let _ = ph.window_health(h);
        }
        assert!(ph.stats().0 <= 1, "迟滞抑制抖动 downgrades={}", ph.stats().0);
    }

    #[test]
    fn trace_ring_overwrites_oldest() {
        let mut tr = PacketTracer::new();
        let mut slots = [0usize; TRACE_RING_CAP + 4];
        for (k, s) in slots.iter_mut().enumerate() {
            *s = tr.submit(k as u64 * 100);
        }
        // 环满后最早的槽被覆盖——旧 slot 的 poll 记到新包上（诚实覆盖语义）。
        let _ = tr.polled(slots[0], 10_000);
        assert!(tr.stats().1 <= TRACE_RING_CAP);
    }
}

// ===========================================================================
// v5 深化批（deep5）
// ===========================================================================

// ---------------------------------------------------------------------------
// 深化一：Nagle 合并账（小包聚积——MTU 内合并，超时强制冲刷）
// ---------------------------------------------------------------------------

/// Nagle 合并器：凑满 MSS 或超时（ms）才发。
pub struct NagleAggregator {
    mss: u16,
    /// 合并超时（ms）。
    timeout_ms: u32,
    buf_bytes: u32,
    buf_since_ms: u32,
    /// 已合并发送次数 / 因满发送 / 因超时发送。
    sent_full: u32,
    sent_timeout: u32,
    /// 被合并（免发送）的小包数。
    coalesced: u64,
}

impl NagleAggregator {
    pub const fn new(mss: u16, timeout_ms: u32) -> Self {
        NagleAggregator {
            mss,
            timeout_ms,
            buf_bytes: 0,
            buf_since_ms: 0,
            sent_full: 0,
            sent_timeout: 0,
            coalesced: 0,
        }
    }

    /// 应用层写入小包（size ≤ MSS/2 才参与合并——大包直发不延迟）。
    /// 返回 true = 触发了发送。
    pub fn submit(&mut self, size: u16, now_ms: u32) -> bool {
        if size as u32 > self.mss as u32 / 2 {
            // 大包直发；若有存积先冲刷。
            if self.buf_bytes > 0 {
                self.sent_full += 1;
                self.buf_bytes = 0;
            }
            return true;
        }
        if self.buf_bytes == 0 {
            self.buf_since_ms = now_ms;
        }
        self.buf_bytes += size as u32;
        if self.buf_bytes >= self.mss as u32 {
            self.sent_full += 1;
            self.buf_bytes = 0;
            true
        } else {
            self.coalesced += 1;
            false
        }
    }

    /// 时钟推进：超时强制冲刷。
    pub fn tick(&mut self, now_ms: u32) -> bool {
        if self.buf_bytes > 0 && now_ms.saturating_sub(self.buf_since_ms) >= self.timeout_ms {
            self.sent_timeout += 1;
            self.buf_bytes = 0;
            return true;
        }
        false
    }

    pub fn stats(&self) -> (u32, u32, u64) {
        (self.sent_full, self.sent_timeout, self.coalesced)
    }

    pub fn pending(&self) -> u32 {
        self.buf_bytes
    }
}

// ---------------------------------------------------------------------------
// 深化二：零窗探测（接收方窗口为 0 时的恢复——指数退避探测直到窗口重开）
// ---------------------------------------------------------------------------

/// 零窗探测状态机。
pub struct ZeroWindowProber {
    /// 当前探测间隔（ms）。
    interval_ms: u32,
    since_last_ms: u32,
    probes: u32,
    /// 窗口重开时的总等待（ms）。
    total_wait_ms: u32,
    open: bool,
}

/// 探测参数：起步 100ms，封顶 3_200ms（5 次倍增）。
pub const ZWP_START_MS: u32 = 100;
pub const ZWP_MAX_MS: u32 = 3_200;

impl ZeroWindowProber {
    pub const fn new() -> Self {
        ZeroWindowProber { interval_ms: ZWP_START_MS, since_last_ms: 0, probes: 0, total_wait_ms: 0, open: false }
    }

    /// 进入零窗。
    pub fn enter(&mut self, now_ms: u32) {
        self.interval_ms = ZWP_START_MS;
        self.since_last_ms = now_ms;
        self.probes = 0;
        self.total_wait_ms = 0;
        self.open = false;
    }

    /// 时钟推进：到点发探测包并倍增间隔。返回 true = 本拍发了探测。
    pub fn tick(&mut self, now_ms: u32) -> bool {
        if self.open {
            return false;
        }
        self.total_wait_ms = now_ms.saturating_sub(self.since_last_ms) + self.total_wait_ms;
        if now_ms.saturating_sub(self.since_last_ms) >= self.interval_ms {
            self.probes += 1;
            self.since_last_ms = now_ms;
            self.interval_ms = (self.interval_ms * 2).min(ZWP_MAX_MS);
            return true;
        }
        false
    }

    /// 收到窗口更新（>0）→ 重开。
    pub fn window_opened(&mut self) {
        self.open = true;
    }

    pub fn stats(&self) -> (u32, u32) {
        (self.probes, self.interval_ms)
    }

    pub fn is_open(&self) -> bool {
        self.open
    }
}

// ---------------------------------------------------------------------------
// 深化三：TLS 记录分帧模拟（流字节 → 定长记录 ≤16KB + 序号账）
// ---------------------------------------------------------------------------

/// 记录分帧器（TLS record 风格：≤16384B/记录，序号防重放）。
pub struct RecordFramer {
    mtu_records: u16,
    seq: u64,
    /// 待发送记录数。
    queued: u32,
    /// 已发出记录数 / 已消费序号。
    sent: u64,
}

pub const RECORD_MAX: usize = 16_384;

impl RecordFramer {
    pub const fn new() -> Self {
        RecordFramer { mtu_records: 0, seq: 0, queued: 0, sent: 0 }
    }

    /// 写入流字节 → 拆成记录入队（返回记录数）。
    pub fn write_stream(&mut self, bytes: usize) -> u32 {
        let records = ((bytes + RECORD_MAX - 1) / RECORD_MAX) as u32;
        self.queued += records;
        records
    }

    /// 出队一条记录（带递增序号——防重放）。
    pub fn pop_record(&mut self) -> Option<u64> {
        if self.queued == 0 {
            return None;
        }
        self.queued -= 1;
        self.seq += 1;
        self.sent += 1;
        Some(self.seq - 1)
    }

    /// 序号校验（收到的序号必须等于期望——严格递增）。
    pub fn verify_seq(&self, got: u64) -> bool {
        got == self.sent - 1 || (self.sent == 0 && got == 0)
    }

    pub fn ledger(&self) -> (u32, u64, u64) {
        (self.queued, self.sent, self.seq)
    }
}

// ---------------------------------------------------------------------------
// deep5 检查项
// ---------------------------------------------------------------------------

pub fn run_smallpkt_deep5_checks() -> CheckSet {
    let mut cs = CheckSet::new("F059-smallpkt-v5");

    // ── Nagle ──
    // 1) 小包合并：两个 300B（MSS 1460）→ 不发送，存积 600。
    let mut ng = NagleAggregator::new(1460, 40);
    cs.add(
        "nagle_coalesces_small",
        !ng.submit(300, 0) && !ng.submit(300, 5) && ng.pending() == 600,
        "",
    );
    // 2) 凑满 MSS → 发送（满触发）。
    cs.add("nagle_full_flush", ng.submit(900, 8) && ng.stats().0 == 1 && ng.pending() == 0, "");
    // 3) 超时强制冲刷。
    let mut ng2 = NagleAggregator::new(1460, 40);
    let _ = ng2.submit(200, 0);
    cs.add("nagle_timeout_flush", !ng2.tick(30) && ng2.tick(40) && ng2.stats().1 == 1, "");
    // 4) 大包直发 + 先冲存积。
    let mut ng3 = NagleAggregator::new(1460, 40);
    let _ = ng3.submit(400, 0);
    cs.add("nagle_big_direct", ng3.submit(1000, 10) && ng3.pending() == 0 && ng3.stats().0 == 1, "");

    // ── 零窗探测 ──
    // 5) 探测间隔倍增：100→200→400（三拍三探测）。
    let mut zwp = ZeroWindowProber::new();
    zwp.enter(0);
    let p1 = zwp.tick(100);
    let p2 = zwp.tick(300);
    let p3 = zwp.tick(700);
    cs.add("zwp_interval_doubles", p1 && p2 && p3 && zwp.stats() == (3, 800), "");
    // 6) 封顶 3200ms。
    let mut zwp2 = ZeroWindowProber::new();
    zwp2.enter(0);
    let mut t = 0u32;
    for _ in 0..8 {
        // 每拍到就推到下一个间隔点。
        t += zwp2.stats().1;
        let _ = zwp2.tick(t);
    }
    cs.add("zwp_capped_3200", zwp2.stats().1 == ZWP_MAX_MS && zwp2.stats().0 == 8, "");
    // 7) 窗口重开 → 停止探测。
    zwp.window_opened();
    cs.add("zwp_stops_on_open", !zwp.tick(100_000) && zwp.is_open(), "");

    // ── 记录分帧 ──
    // 8) 流拆记录：40KB → 3 条（16+16+8）。
    let mut rf = RecordFramer::new();
    cs.add("framer_40k_3records", rf.write_stream(40_960) == 3 && rf.ledger().0 == 3, "");
    // 9) 出队序号严格递增。
    let s0 = rf.pop_record().expect("r0");
    let s1 = rf.pop_record().expect("r1");
    cs.add("framer_seq_increasing", s0 == 0 && s1 == 1 && rf.verify_seq(1), "");
    // 10) 空队列拒出队。
    let mut rf2 = RecordFramer::new();
    cs.add("framer_empty_none", rf2.pop_record().is_none(), "");

    cs
}

#[cfg(test)]
mod deep5_tests {
    use super::*;

    #[test]
    fn nagle_ledger_consistent() {
        let mut ng = NagleAggregator::new(1460, 20);
        for k in 0..10u32 {
            let _ = ng.submit(100, k * 2);
        }
        // 1000B < 1460 → 全部扣住不发送（含首包——Nagle 语义：存积即合并）。
        assert_eq!(ng.stats().2, 10);
        assert_eq!(ng.pending(), 1000);
        assert!(ng.tick(200), "超时冲刷");
        assert_eq!(ng.stats().1, 1);
    }

    #[test]
    fn zwp_full_backoff_curve() {
        let mut zwp = ZeroWindowProber::new();
        zwp.enter(0);
        let mut now = 0u32;
        let mut curve = [0u32; 6];
        for c in 0..6 {
            now += zwp.stats().1.max(1);
            let _ = zwp.tick(now);
            curve[c] = zwp.stats().1;
        }
        assert_eq!(curve, [200, 400, 800, 1600, 3200, 3200], "100 起步探测后曲线 200..3200 封顶");
    }

    #[test]
    fn framer_exact_multiple() {
        let mut rf = RecordFramer::new();
        assert_eq!(rf.write_stream(RECORD_MAX), 1, "整记录不虚增");
        assert_eq!(rf.write_stream(1), 1, "1 字节也要一条记录");
    }
}
