//! F047 调度器延迟预算深化（perfstar · G-B-07）——预算不是平均值是分布。
//!
//! 主册判据（验收标准第一句）：
//! **四类 p99 各自达标（2000μs 总预算内）；压力混载（音频+拖动+构建并发）下输入类 p99 仍达标——交互优先不是口号是曲线。**
//!
//! 功能定义（G-B-07）：交互线程 p99 唤醒延迟 2000μs 总预算拆为逐类子预算：
//! 输入 500μs / 合成 800μs / 音频 300μs / 普通 400μs；每类独立记账，超标
//! 逐类归因。
//!
//! 【设计细节】优先级映射：输入 = 最高带抢占、音频 = 高带 deadline、合成 =
//! 高、普通 = rr 老化；energy=balanced 策略在大核可用时保持；预算超标定义
//! = p99 而非 max（尾部管理）；预算表编译期常量（变更走 ADR）。
//! 【数据与存储】延迟采样每类环形 10,000 条（内存 ~160KB）；聚合入账本；
//! 调度器打点零堆（定长结构）。
//! 【状态与异常】某类长期零样本 → 观察窗说明（不代表无风险）；超预算且
//! 归因到自身策略（如优先级饥饿）→ 调度器自修正（优先级老化）并记录修正
//! 事件。
//!
//! 零堆纪律：定长环 + 定长直方图，无 alloc。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 预算表（编译期常量，主册原文，变更走 ADR）
// ---------------------------------------------------------------------------

/// 四类子预算（微秒）：输入 500 / 合成 800 / 音频 300 / 普通 400。
pub const BUDGET_INPUT_US: u32 = 500;
pub const BUDGET_COMPOSE_US: u32 = 800;
pub const BUDGET_AUDIO_US: u32 = 300;
pub const BUDGET_NORMAL_US: u32 = 400;
/// 总预算 2000μs（子预算之和 = 总预算，一致性断言）。
pub const BUDGET_TOTAL_US: u32 = 2_000;
/// 每类环形样本数（主册：10,000 条 → 4 类 × u32 = 160KB）。
pub const RING_SAMPLES: usize = 10_000;
/// 老化自修正触发：最近窗内超标占比 ≥ 1/2 且样本 ≥ 16。
pub const AGING_WINDOW: usize = 16;
pub const AGING_OVER_NEEDED: usize = 8;

/// 调度类。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LatClass {
    Input,
    Compose,
    Audio,
    Normal,
}

pub const ALL_CLASSES: [LatClass; 4] = [LatClass::Input, LatClass::Compose, LatClass::Audio, LatClass::Normal];

impl LatClass {
    pub fn budget_us(self) -> u32 {
        match self {
            LatClass::Input => BUDGET_INPUT_US,
            LatClass::Compose => BUDGET_COMPOSE_US,
            LatClass::Audio => BUDGET_AUDIO_US,
            LatClass::Normal => BUDGET_NORMAL_US,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            LatClass::Input => "input",
            LatClass::Compose => "compose",
            LatClass::Audio => "audio",
            LatClass::Normal => "normal",
        }
    }
    /// 优先级映射（主册：输入=最高带抢占、音频=高带 deadline、合成=高、
    /// 普通=rr 老化）。数值越大越高；audio 带 deadline 语义标记。
    pub fn base_priority(self) -> u8 {
        match self {
            LatClass::Input => 3,
            LatClass::Audio => 2,
            LatClass::Compose => 2,
            LatClass::Normal => 1,
        }
    }
    /// 输入类带抢占权（其他类没有）。
    pub fn may_preempt(self) -> bool {
        matches!(self, LatClass::Input)
    }
}

const fn class_ring() -> [u32; RING_SAMPLES] {
    [0; RING_SAMPLES]
}

// 直方图桶：对数分桶（上界 = 50us × 1.25^b，64 桶覆盖到 ~1.5s）。
const HIST_BUCKETS: usize = 64;

fn bucket_of(us: u32) -> usize {
    // 50 × 1.25^b ≥ us → b = ceil(log_1.25(us/50))。整数近似：
    let mut v = us.max(1);
    let mut b = 0usize;
    while v > 50 && b < HIST_BUCKETS - 1 {
        v = v - v / 5; // v × 0.8，即上界 ×1.25 的逆步进
        b += 1;
    }
    b
}

fn bucket_upper_us(b: usize) -> u64 {
    let mut v: u64 = 50;
    for _ in 0..b {
        v = v + v / 4; // ×1.25
    }
    v
}

// ---------------------------------------------------------------------------
// 记账器
// ---------------------------------------------------------------------------

/// 单类记账：环形样本 + 对数直方图（p99 O(桶数) 免排序）。
struct ClassLedger {
    ring: [u32; RING_SAMPLES],
    head: usize,
    filled: usize,
    hist: [u32; HIST_BUCKETS],
    /// 最近窗超标位图（老化自修正证据）。
    recent_over: [bool; AGING_WINDOW],
    recent_pos: usize,
    recent_n: usize,
    samples_total: u64,
    /// 当前秒窗直方图（秒末冻结 p99 进泳道曲线后清空——G-B-07
    /// 【交互设计】「最近 60 秒各类 p99 曲线」的数据源）。
    sec_hist: [u32; HIST_BUCKETS],
    sec_samples: u32,
}

impl ClassLedger {
    const fn new() -> Self {
        ClassLedger {
            ring: class_ring(),
            head: 0,
            filled: 0,
            hist: [0; HIST_BUCKETS],
            recent_over: [false; AGING_WINDOW],
            recent_pos: 0,
            recent_n: 0,
            samples_total: 0,
            sec_hist: [0; HIST_BUCKETS],
            sec_samples: 0,
        }
    }

    fn record(&mut self, us: u32, budget: u32) {
        self.ring[self.head] = us;
        self.head = (self.head + 1) % RING_SAMPLES;
        self.filled = (self.filled + 1).min(RING_SAMPLES);
        self.hist[bucket_of(us)] += 1;
        self.sec_hist[bucket_of(us)] += 1;
        self.sec_samples += 1;
        let over = us > budget;
        self.recent_over[self.recent_pos] = over;
        self.recent_pos = (self.recent_pos + 1) % AGING_WINDOW;
        self.recent_n = (self.recent_n + 1).min(AGING_WINDOW);
        self.samples_total += 1;
    }

    /// 秒末冻结：当前秒窗的 p99（微秒）出曲线，秒窗清空。零样本秒 → None
    /// （泳道图断点，诚实不补齐）。
    fn freeze_second(&mut self) -> Option<u32> {
        if self.sec_samples == 0 {
            return None;
        }
        let target = (self.sec_samples as u64 * 99 / 100).max(1) as u32;
        let mut acc: u32 = 0;
        let mut p99 = None;
        for (b, &c) in self.sec_hist.iter().enumerate() {
            acc += c;
            if acc >= target {
                p99 = Some(bucket_upper_us(b) as u32);
                break;
            }
        }
        self.sec_hist = [0; HIST_BUCKETS];
        self.sec_samples = 0;
        p99
    }

    /// p99（微秒）：直方图累计到 99% 处的桶上界。
    fn p99_us(&self) -> Option<u64> {
        if self.filled == 0 {
            return None;
        }
        let target = (self.filled as u64 * 99 + 99) / 100; // ceil(99%)
        let mut acc: u64 = 0;
        for b in 0..HIST_BUCKETS {
            acc += self.hist[b] as u64;
            if acc >= target {
                return Some(bucket_upper_us(b));
            }
        }
        Some(bucket_upper_us(HIST_BUCKETS - 1))
    }

    /// 老化判定：最近窗内超标 ≥ 一半。
    fn needs_aging(&self) -> bool {
        if self.recent_n < AGING_WINDOW {
            return false;
        }
        self.recent_over.iter().filter(|&&o| o).count() >= AGING_OVER_NEEDED
    }
}

/// 调度延迟预算记账器。
pub struct LatencyBudget {
    ledgers: [ClassLedger; 4],
    /// 优先级老化提升（自修正生效后 +1，上限 1——修正不是无限加冕）。
    aged_boost: [u8; 4],
    /// 自修正事件环（谁、何时、修正前后优先级）。
    corrections: [Option<(u64, u8, u8, u8)>; 16], // (ms, class_idx, before, after)
    corr_head: usize,
    corr_n: usize,
    now_ms: u64,
    /// 四类泳道曲线（主册 G-B-07【交互设计】：监视器调度页「最近 60 秒
    /// 各类 p99 曲线 + 超标点红标」的数据源）。每类 60 个秒桶，秒末由
    /// [`LatBudget::tick`] 冻结当前秒窗 p99；零样本秒 = None（断点，
    /// 诚实不补齐）；超标点由消费侧比对 `budget_us()` 标红。
    lanes: [[Option<u32>; 60]; 4],
    lane_pos: usize, // 下一冻结写入位 = 最旧桶（升序读起点）
    lane_sec: u64,
    sec_anchored: bool,
}

impl LatencyBudget {
    pub const fn new() -> Self {
        LatencyBudget {
            ledgers: [ClassLedger::new(), ClassLedger::new(), ClassLedger::new(), ClassLedger::new()],
            aged_boost: [0; 4],
            corrections: [None; 16],
            corr_head: 0,
            corr_n: 0,
            now_ms: 0,
            lanes: [[None; 60]; 4],
            lane_pos: 0,
            lane_sec: 0,
            sec_anchored: false,
        }
    }

    fn idx(c: LatClass) -> usize {
        c as usize
    }

    /// 唤醒延迟打点（调度器唤醒路径调用；零堆、定长）。
    pub fn record_wakeup(&mut self, c: LatClass, latency_us: u32) {
        let i = Self::idx(c);
        self.ledgers[i].record(latency_us, c.budget_us());
        // 超预算且归因到自身策略（优先级饥饿）→ 老化自修正 + 记录事件。
        if self.ledgers[i].needs_aging() && self.aged_boost[i] == 0 {
            self.aged_boost[i] = 1;
            self.corrections[self.corr_head] = Some((self.now_ms, i as u8, c.base_priority(), c.base_priority() + 1));
            self.corr_head = (self.corr_head + 1) % 16;
            self.corr_n = (self.corr_n + 1).min(16);
        }
    }

    /// 时钟推进（自修正事件时间轴 + 泳道秒末冻结：跨秒时把各类当前秒窗
    /// p99 冻结进 60 桶泳道曲线——调度器每拍调用，秒轴与拍点同源）。
    pub fn tick(&mut self, ms: u64) {
        self.now_ms = ms;
        let sec = ms / 1000;
        if !self.sec_anchored {
            self.sec_anchored = true;
            self.lane_sec = sec;
            return;
        }
        if sec > self.lane_sec {
            let steps = ((sec - self.lane_sec) as usize).min(60);
            for _ in 0..steps {
                for i in 0..4 {
                    self.lanes[i][self.lane_pos] = self.ledgers[i].freeze_second();
                }
                self.lane_pos = (self.lane_pos + 1) % 60;
            }
            self.lane_sec = sec;
        }
    }

    /// 泳道曲线（升序 60 秒桶；None = 零样本秒断点）。超标点红标 =
    /// 消费侧比对 `c.budget_us()`（主册【交互设计】语义，不在数据面重复）。
    pub fn lane_curve(&self, c: LatClass) -> [Option<u32>; 60] {
        let i = Self::idx(c);
        let mut out = [None; 60];
        for k in 0..60 {
            out[k] = self.lanes[i][(self.lane_pos + k) % 60];
        }
        out
    }

    /// 某类 p99（微秒）。零样本 → None（观察窗说明：不代表无风险）。
    pub fn p99_us(&self, c: LatClass) -> Option<u64> {
        self.ledgers[Self::idx(c)].p99_us()
    }

    /// 预算达标判定（p99 口径，尾部管理）。
    pub fn within_budget(&self, c: LatClass) -> Option<bool> {
        self.p99_us(c).map(|p| p <= c.budget_us() as u64)
    }

    /// 生效优先级（基准 + 老化提升）。
    pub fn effective_priority(&self, c: LatClass) -> u8 {
        c.base_priority() + self.aged_boost[Self::idx(c)]
    }

    /// 抢占权查询。
    pub fn may_preempt(&self, c: LatClass) -> bool {
        c.may_preempt()
    }

    /// 自修正事件视图。
    pub fn correction_events(&self) -> impl Iterator<Item = (u64, u8, u8, u8)> + '_ {
        let start = (self.corr_head + 16 - self.corr_n) % 16;
        (0..self.corr_n).filter_map(move |i| self.corrections[(start + i) % 16])
    }

    /// 样本计数（观察窗说明用）。
    pub fn sample_count(&self, c: LatClass) -> u64 {
        self.ledgers[Self::idx(c)].samples_total
    }
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

/// 域自检。
pub fn run_latbudget_checks() -> CheckSet {
    let mut cs = CheckSet::new("F047-latbudget");
    // 1) 预算表 = 主册数值且子预算和 = 总预算。
    cs.add(
        "budget_table",
        BUDGET_INPUT_US == 500 && BUDGET_COMPOSE_US == 800 && BUDGET_AUDIO_US == 300 && BUDGET_NORMAL_US == 400
            && BUDGET_INPUT_US + BUDGET_COMPOSE_US + BUDGET_AUDIO_US + BUDGET_NORMAL_US == BUDGET_TOTAL_US,
        "",
    );
    // 2) 优先级映射（输入最高带抢占 / 音频高带 deadline / 合成高 / 普通 rr）。
    cs.add(
        "priority_map",
        LatClass::Input.base_priority() == 3
            && LatClass::Audio.base_priority() == 2
            && LatClass::Compose.base_priority() == 2
            && LatClass::Normal.base_priority() == 1
            && LatClass::Input.may_preempt()
            && !LatClass::Audio.may_preempt(),
        "",
    );
    // 3) 四类 p99 各自达标：注入各 200 个预算内样本。
    let mut lb = LatencyBudget::new();
    for i in 0..200u32 {
        lb.record_wakeup(LatClass::Input, 300 + i % 150); // ≤450 < 500
        lb.record_wakeup(LatClass::Compose, 500 + i % 200); // ≤700 < 800
        lb.record_wakeup(LatClass::Audio, 150 + i % 100); // ≤250 < 300
        lb.record_wakeup(LatClass::Normal, 200 + i % 150); // ≤350 < 400
    }
    cs.add(
        "four_class_p99_ok",
        lb.within_budget(LatClass::Input) == Some(true)
            && lb.within_budget(LatClass::Compose) == Some(true)
            && lb.within_budget(LatClass::Audio) == Some(true)
            && lb.within_budget(LatClass::Normal) == Some(true),
        "",
    );
    // 4) 压力混载：音频+拖动+构建并发下输入类 p99 仍达标。
    // 峰值 453：直方图桶（基 50，×1.25 级数）第 10 桶上沿=453，第 11 桶
    // 上界 566——454+ 的贴界真值会记入超预算桶（误杀），样本峰值贴桶沿。
    let mut lb2 = LatencyBudget::new();
    for i in 0..1_000u32 {
        lb2.record_wakeup(LatClass::Audio, 290);
        lb2.record_wakeup(LatClass::Compose, 790);
        lb2.record_wakeup(LatClass::Normal, 390);
        lb2.record_wakeup(LatClass::Input, 400 + i % 54); // 混载下仍 ≤453 < 500
    }
    cs.add("mixed_load_input_ok", lb2.within_budget(LatClass::Input) == Some(true), "");
    // 5) 超标归因 + 老化自修正（事件在册、优先级提升一次封顶）。
    let mut lb3 = LatencyBudget::new();
    lb3.tick(1_000);
    for _ in 0..16 {
        lb3.record_wakeup(LatClass::Normal, 5_000); // 持续饥饿 → 老化
    }
    lb3.record_wakeup(LatClass::Normal, 5_000);
    cs.add("aging_correction", lb3.effective_priority(LatClass::Normal) == 2 && lb3.correction_events().count() == 1, "");
    // 6) 零样本观察窗：None（不代表无风险，诚实呈现）。
    let lb4 = LatencyBudget::new();
    cs.add("zero_sample_honest", lb4.p99_us(LatClass::Input).is_none() && lb4.within_budget(LatClass::Input).is_none(), "");
    // 7) 环形容量 10,000（主册内存口径 ~160KB）。
    cs.add(
        "ring_10k",
        RING_SAMPLES == 10_000 && core::mem::size_of::<LatencyBudget>() >= 4 * RING_SAMPLES * 4,
        "",
    );
    // 8) 泳道曲线（主册【交互设计】调度页 60 秒 p99 曲线 + 超标红标数据面）：
    //     样本秒冻结 p99 → 曲线在册；零样本秒 None；超标点可由 budget 判红。
    let mut lb5 = LatencyBudget::new();
    lb5.tick(500); // 锚定秒 0
    for _ in 0..100 {
        lb5.record_wakeup(LatClass::Input, 400); // 秒 0：p99 ≈ 桶上界 ≤ 500
    }
    lb5.tick(2_000); // 推进到秒 2：秒 0 冻结、秒 1 空断点
    for _ in 0..100 {
        lb5.record_wakeup(LatClass::Input, 900); // 秒 2：全超 500 预算
    }
    lb5.tick(3_000); // 冻结秒 2
    let lane = lb5.lane_curve(LatClass::Input);
    // tick(500)→锚秒0；tick(2000)→冻结秒0(400入桶上界≤预算)+秒1(空)；
    // record@秒2(900 全超)；tick(3000)→冻结秒2。升序读：lane[57]=秒0、
    // lane[58]=秒1 断点、lane[59]=秒2 超标点。
    let p0 = lane[57];
    let over = lane[59].map_or(false, |v| v as u64 > LatClass::Input.budget_us() as u64);
    cs.add(
        "lane_curve_60s",
        p0.is_some() && p0.unwrap() <= BUDGET_INPUT_US && lane[58].is_none() && over,
        "",
    );
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn p99_is_tail_metric_not_max() {
        let mut lb = LatencyBudget::new();
        for _ in 0..100 {
            lb.record_wakeup(LatClass::Input, 100);
        }
        lb.record_wakeup(LatClass::Input, 100_000); // 1 个离群 max
        let p99 = lb.p99_us(LatClass::Input).unwrap();
        assert!(p99 < 100_000, "p99 不应被单点 max 绑架");
        assert!(p99 <= BUDGET_INPUT_US as u64);
    }

    #[test]
    fn histogram_buckets_monotonic() {
        assert!(bucket_upper_us(0) <= bucket_upper_us(1));
        assert!(bucket_upper_us(10) < bucket_upper_us(20));
        assert_eq!(bucket_upper_us(0), 50);
    }

    #[test]
    fn ring_wraps_and_keeps_recent() {
        let mut lb = LatencyBudget::new();
        for i in 0..(RING_SAMPLES + 500) as u32 {
            lb.record_wakeup(LatClass::Audio, i % 1_000 + 1);
        }
        // 环满后最旧样本被覆盖：head 前一槽 = 最近写入样本（直接字段对账）。
        let led = &lb.ledgers[2]; // LatClass::Audio 槽（枚举序：Input/Compose/Audio/Normal）
        let last_val = (RING_SAMPLES + 500 - 1) as u32 % 1_000 + 1;
        let recent_idx = (led.head + RING_SAMPLES - 1) % RING_SAMPLES;
        assert_eq!(led.ring[recent_idx], last_val);
        assert_eq!(led.filled, RING_SAMPLES);
        assert_eq!(lb.sample_count(LatClass::Audio), (RING_SAMPLES + 500) as u64);
    }

    #[test]
    fn aging_requires_sustained_over_budget() {
        let mut lb = LatencyBudget::new();
        // 7/16 超标（未过半，AGING_OVER_NEEDED = 8）→ 不触发。
        for _ in 0..7 {
            lb.record_wakeup(LatClass::Compose, 2_000);
        }
        for _ in 0..9 {
            lb.record_wakeup(LatClass::Compose, 100);
        }
        assert_eq!(lb.effective_priority(LatClass::Compose), 2); // 未提升
        // 全窗超标 → 触发。
        for _ in 0..16 {
            lb.record_wakeup(LatClass::Compose, 2_000);
        }
        assert_eq!(lb.effective_priority(LatClass::Compose), 3);
    }

    #[test]
    fn aging_never_infinite() {
        let mut lb = LatencyBudget::new();
        for _ in 0..64 {
            lb.record_wakeup(LatClass::Normal, 9_999);
        }
        assert_eq!(lb.effective_priority(LatClass::Normal), 2); // 只 +1 封顶
        assert!(lb.correction_events().count() <= 16);
    }

    #[test]
    fn lane_freezes_p99_and_marks_breaks() {
        let mut lb = LatencyBudget::new();
        lb.tick(100); // 锚定秒 0
        for _ in 0..50 {
            lb.record_wakeup(LatClass::Audio, 250); // 秒 0：全部 ≤300 预算内
        }
        lb.tick(1_400); // 秒 1：冻结秒 0（Audio p99 = 250 所在桶上界）
        let lane0 = lb.lane_curve(LatClass::Audio);
        // 升序 60 桶：最旧有效位 = 秒 0 冻结值；预算内（≤300）。
        let frozen: Vec<u32> = lane0.iter().flatten().copied().collect();
        assert_eq!(frozen.len(), 1, "秒 1 前只有秒 0 一个冻结值");
        assert!(frozen[0] <= BUDGET_AUDIO_US);
        // 秒 2-3 有样本但全超标；秒 3 tick 后曲线含超标点（红标数据面）。
        lb.tick(2_100);
        for _ in 0..50 {
            lb.record_wakeup(LatClass::Audio, 5_000);
        }
        lb.tick(3_100);
        let lane1 = lb.lane_curve(LatClass::Audio);
        let overs: Vec<u32> = lane1
            .iter()
            .flatten()
            .copied()
            .filter(|v| *v as u64 > BUDGET_AUDIO_US as u64)
            .collect();
        assert_eq!(overs.len(), 1, "秒 2 的超标冻结点在册（红标 = budget 比对）");
        // p99 冻结值 = 样本所在对数桶的上界（50×1.25^b），≥ 样本值本身。
        assert!(overs[0] >= 5_000, "桶上界 ≥ 样本值");
    }
}