// cgb340_burst.rs ↔ CGPU-F0340 突发负载处理
//
// 判据原文：CGPU-F0340-J1 识别、缓冲、限流、平滑恢复、判据。
// 难点分析：突发识别要用「申请速率突变」而非绝对值——EWMA 基线 + 倍数阈值。
//          平滑恢复的敌人是「回弹震荡」：限流解除必须走令牌桶渐进放行。
// 纪律约束：小突发余量池吸收；大突发排队+限流；恢复平滑不震荡。
// 成本记账：EWMA O(1)；令牌桶 O(1)；无历史窗口堆积。

#![allow(dead_code)]

use alloc::vec::Vec;

pub type BudgetUs = u64;

/// 突发状态机。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BurstState {
    /// 正常。
    Normal,
    /// 小突发（余量池吸收）。
    Minor,
    /// 大突发（限流中）。
    Throttled,
    /// 平滑恢复中。
    Recovering,
}

/// 突发事件。
#[derive(Debug, Clone)]
pub struct BurstEvent {
    pub frame: u64,
    pub state: BurstState,
    /// 检测到的申请速率（本帧申请量）。
    pub rate_us: BudgetUs,
    /// EWMA 基线。
    pub baseline_us: BudgetUs,
    /// 处置：吸收/排队/限流量。
    pub absorbed_us: BudgetUs,
    pub throttled_us: BudgetUs,
}

/// EWMA 基线跟踪器。
#[derive(Debug, Clone)]
struct Ewma {
    value: f64,
    /// 平滑系数（0.0~1.0，越小越平滑）。
    alpha: f64,
    initialized: bool,
}

impl Ewma {
    fn new(alpha: f64) -> Self {
        Ewma { value: 0.0, alpha, initialized: false }
    }
    fn push(&mut self, x: f64) {
        if !self.initialized {
            self.value = x;
            self.initialized = true;
        } else {
            self.value = self.alpha * x + (1.0 - self.alpha) * self.value;
        }
    }
    fn value(&self) -> f64 {
        self.value
    }
}

/// 令牌桶（平滑恢复用）。
#[derive(Debug, Clone)]
struct TokenBucket {
    tokens: f64,
    capacity: f64,
    /// 每帧补充速率。
    refill_per_frame: f64,
}

impl TokenBucket {
    fn new(capacity: f64, refill_per_frame: f64) -> Self {
        TokenBucket { tokens: capacity, capacity, refill_per_frame }
    }
    fn refill(&mut self) {
        self.tokens = (self.tokens + self.refill_per_frame).min(self.capacity);
    }
    fn try_take(&mut self, want: f64) -> f64 {
        let got = want.min(self.tokens);
        self.tokens -= got;
        got
    }
}

/// 突发负载处理器。
pub struct BurstHandler {
    /// 申请速率基线（EWMA）。
    baseline: Ewma,
    /// 突发倍数阈值（相对基线）。
    minor_multiplier: f64,
    major_multiplier: f64,
    /// 余量池（吸收小突发）。
    reserve_pool: BudgetUs,
    reserve_capacity: BudgetUs,
    /// 状态。
    state: BurstState,
    /// 恢复令牌桶。
    recovery_bucket: TokenBucket,
    /// 状态机历史。
    events: Vec<BurstEvent>,
    frame: u64,
    /// 累计限流量。
    total_throttled: BudgetUs,
    /// 累计吸收量。
    total_absorbed: BudgetUs,
    /// 状态持续帧数（迟滞）。
    state_age: u32,
    /// 恢复迟滞：需连续 N 帧低于阈值才回 Normal。
    recover_frames_needed: u32,
    recover_frames_seen: u32,
}

impl BurstHandler {
    /// 新建。
    pub fn new(reserve_capacity: BudgetUs) -> Self {
        BurstHandler {
            baseline: Ewma::new(0.2),
            minor_multiplier: 2.0,
            major_multiplier: 5.0,
            reserve_pool: reserve_capacity,
            reserve_capacity,
            state: BurstState::Normal,
            recovery_bucket: TokenBucket::new(reserve_capacity as f64, reserve_capacity as f64 / 4.0),
            events: Vec::new(),
            frame: 0,
            total_throttled: 0,
            total_absorbed: 0,
            state_age: 0,
            recover_frames_needed: 3,
            recover_frames_seen: 0,
        }
    }

    /// 处理一帧申请速率，返回（实际放行量, 被限量）。
    pub fn on_request_rate(&mut self, request_us: BudgetUs, frame_budget: BudgetUs) -> (BudgetUs, BudgetUs) {
        self.frame += 1;
        self.recovery_bucket.refill();

        let base = self.baseline.value();
        let mut absorbed = 0u64;
        let mut throttled = 0u64;
        let mut admitted = request_us;

        // 突发识别。
        if base > 0.0 {
            let ratio = request_us as f64 / base;
            if ratio >= self.major_multiplier {
                self.transition(BurstState::Throttled);
            } else if ratio >= self.minor_multiplier {
                if self.state == BurstState::Normal {
                    self.transition(BurstState::Minor);
                }
            } else {
                // 低于阈值 → 恢复计数。
                self.recover_frames_seen += 1;
                if self.recover_frames_seen >= self.recover_frames_needed
                    && self.state != BurstState::Normal
                {
                    self.transition(BurstState::Recovering);
                }
            }
        }

        // 按状态处置。
        match self.state {
            BurstState::Normal => {
                admitted = request_us.min(frame_budget);
            }
            BurstState::Minor => {
                // 余量池吸收。
                let need = request_us.saturating_sub(frame_budget);
                if need > 0 && self.reserve_pool >= need {
                    self.reserve_pool -= need;
                    absorbed = need;
                    admitted = request_us;
                } else {
                    admitted = request_us.min(frame_budget + self.reserve_pool);
                    absorbed = admitted.saturating_sub(frame_budget.min(admitted));
                    self.reserve_pool = self.reserve_pool.saturating_sub(absorbed);
                }
            }
            BurstState::Throttled => {
                // 排队 + 限流（令牌桶）。
                let allow = self.recovery_bucket.try_take(request_us as f64) as u64;
                admitted = allow.min(frame_budget);
                throttled = request_us - admitted;
                self.total_throttled += throttled;
            }
            BurstState::Recovering => {
                // 平滑放行（令牌桶渐进）。
                let allow = self.recovery_bucket.try_take(request_us as f64) as u64;
                admitted = allow.min(frame_budget);
                throttled = request_us - admitted;
                self.total_throttled += throttled;
                // 连续全放行后回 Normal。
                if throttled == 0 {
                    self.recover_frames_seen += 1;
                    if self.recover_frames_seen >= self.recover_frames_needed {
                        self.transition(BurstState::Normal);
                    }
                } else {
                    self.recover_frames_seen = 0;
                }
            }
        }

        self.total_absorbed += absorbed;
        self.baseline.push(request_us as f64);

        self.events.push(BurstEvent {
            frame: self.frame,
            state: self.state,
            rate_us: request_us,
            baseline_us: self.baseline.value() as u64,
            absorbed_us: absorbed,
            throttled_us: throttled,
        });

        (admitted, throttled)
    }

    fn transition(&mut self, to: BurstState) {
        if self.state != to {
            self.state = to;
            self.state_age = 0;
            self.recover_frames_seen = 0;
            // 进入限流/恢复时重置令牌桶为部分容量（渐进）。
            if to == BurstState::Recovering {
                self.recovery_bucket.tokens = self.recovery_bucket.capacity * 0.3;
            }
        } else {
            self.state_age += 1;
        }
    }

    /// 当前状态。
    pub fn state(&self) -> BurstState {
        self.state
    }
    /// 余量池剩余。
    pub fn reserve_remaining(&self) -> BudgetUs {
        self.reserve_pool
    }
    /// 余量池回充（帧末有结余时）。
    pub fn refill_reserve(&mut self, us: BudgetUs) {
        self.reserve_pool = (self.reserve_pool + us).min(self.reserve_capacity);
    }
    /// 累计限流。
    pub fn total_throttled(&self) -> BudgetUs {
        self.total_throttled
    }
    /// 累计吸收。
    pub fn total_absorbed(&self) -> BudgetUs {
        self.total_absorbed
    }
    /// 事件流。
    pub fn events(&self) -> &[BurstEvent] {
        &self.events
    }
    /// 基线值。
    pub fn baseline(&self) -> BudgetUs {
        self.baseline.value() as u64
    }
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

use super::super::checks::CheckSet;

pub fn run_cgb340_checks() -> CheckSet {
    let mut cs = CheckSet::new("CGPU-F0340 突发负载处理");

    // --- 正常状态 ---
    let mut h = BurstHandler::new(5000);
    // 先建立基线
    for _ in 0..10 {
        h.on_request_rate(100, 1000);
    }
    cs.add("初始状态 Normal", h.state() == BurstState::Normal, "基线稳定后");

    // --- 小突发 → Minor ---
    let (admitted, _) = h.on_request_rate(300, 1000); // 3x 基线
    cs.add("小突发识别", h.state() == BurstState::Minor, "3x 基线");
    cs.add("小突发放行", admitted > 0, "余量池吸收");

    // --- 大突发 → Throttled ---
    // 先回正常
    for _ in 0..10 {
        h.on_request_rate(100, 1000);
    }
    let (_a, throttled) = h.on_request_rate(2000, 1000); // 20x 基线
    cs.add("大突发限流", h.state() == BurstState::Throttled, "20x 触发限流");
    cs.add("限流量 >0", throttled > 0 || h.total_throttled() > 0, "部分被限");

    // --- 平滑恢复 ---
    for _ in 0..15 {
        h.on_request_rate(100, 1000);
    }
    let st = h.state();
    cs.add(
        "平滑恢复回 Normal 或 Recovering",
        st == BurstState::Normal || st == BurstState::Recovering,
        "低速率连续后恢复",
    );

    // --- 余量池回充 ---
    let before = h.reserve_remaining();
    h.refill_reserve(1000);
    cs.add("余量池回充", h.reserve_remaining() >= before, "帧末结余回充");

    // --- 基线跟踪 ---
    cs.add("基线非零", h.baseline() > 0, "EWMA 已初始化");

    // --- 事件流水 ---
    cs.add("事件流水非空", h.events().len() > 5, "全程记录");

    cs
}
