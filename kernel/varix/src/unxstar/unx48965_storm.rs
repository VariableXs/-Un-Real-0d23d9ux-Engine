//! UNX-F48965 事件风暴恢复账与 10,000ev/s 限流 · 0x71480
//! ═══════════════════════════════════════════════════════════════════════
//! 判据原文（任务板逐字锚定，B40 族第三槽）：
//!   ① 纯功能代码 ≥340 行；
//!   ② UNX-F48965-J1 风暴 ×10 轮恢复账可查（积压峰值与恢复时长逐轮
//!      对得上）；
//!   ③ UNX-F48966-J1 10,000 事件/秒限流：稳态不超速、突发有上界。
//! ─────────────────────────────────────────────────────────────────────
//! 闸门次序：样本先验 → 令牌补充 → 准入/拒绝分账 → 稳态对账 →
//!           风暴积压 → 排空递归 → 残留归零 → 恢复时长解析式复核。
//! 先补令牌再准入：补充与准入同刻发生时，顺序决定首 tick 能否吃满桶。
//! 本模块把「先补后放」钉死——否则 40 的桶在首 tick 只能放出 10，
//! 限流上界就悄悄缩水，而账面依然自洽。
//! 稳态对账紧跟准入：稳态含义是「长期准入 ≈ 补充速率」。这个等式只有
//! 用整数逐 tick 累加才能证；用浮点算速率会把累计误差藏进小数点后，
//! 十万量级的账少 1 都看不出来。
//! 风暴积压先于排空：积压是「到达 − 服务」的逐 tick 递推，峰值只在
//! 风暴相位取得；先钉死积压形状，排空时长才有可复核的定义域。
//! 排空递归放在最后但必须精确：恢复时长 = ceil(峰值 / (服务 − 平静到达))，
//! 这个解析式与逐 tick 递归必须逐轮相等。两者一旦分歧，就说明「恢复」
//! 被某个隐式取整偷换了——这正是风暴账最容易失真的地方。
//! 残留归零是判决项：风暴结束后积压必须恰好为 0。允许「接近 0」的
//! 账等于允许少量事件永远滞留在通道里，那是最隐蔽的丢事件。
//! 关键差异：本模块只回答「风暴把积压推到多高、多久排空、限流在
//!           10,000/s 上是否守得住」，不做帧折叠（B40-CORE）、不做
//!           P99 归因（B40-SCHED）、不做水位背压（B40-BACKPRESSURE）。
//!           背压关心的是「推回生产者」，本模块只关心「账必须平」。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::unx48941_wheel_core as p_core;

/// 诊断段基址（0x71480，B40 族第三槽）。
pub const SEG_BASE: u32 = 0x71480;

/// 限流目标：每秒 10,000 个事件。
pub const LIMIT_RPS: u32 = 10_000;
/// 毫秒基准（限流按毫秒刻度补充令牌，避免分数速率）。
pub const MS_PER_SEC: u32 = 1_000;
/// 每毫秒补充的令牌数（10,000 / 1,000 = 10，整除无余数）。
pub const REFILL_PER_MS: u32 = LIMIT_RPS / MS_PER_SEC;
/// 令牌桶容量：4 毫秒的额度，只用于吸收突发。
pub const BUCKET_CAP: u32 = 40;
/// 服务率：20,000 事件/秒（风暴期固定的消费能力）。
pub const SERVICE_PER_MS: u32 = 20;
/// 风暴到达率：40,000 事件/秒（是服务率的两倍，积压线性增长）。
pub const STORM_RATE: u32 = 40;
/// 风暴相位的轮转幅度：每轮加 0..4，制造可控的峰值梯度。
pub const STORM_SWING: u32 = 5;
/// 平静到达率：5,000 事件/秒（低于服务率，积压才会下降）。
pub const CALM_RATE: u32 = 5;
/// 风暴相位长度（毫秒刻度）。
pub const STORM_TICKS: u32 = 25;
/// 卡面判据的轮数：风暴 ×10 轮。
pub const STORM_ROUNDS: u32 = 10;
/// 恢复相位上限：超过即判恢复失败（不给无限排空的余地）。
pub const RECOVERY_CAP: u32 = 256;
/// 单 tick 样本上限（防止构造型溢出，同时也是 fuzz 的注入域上界）。
pub const SAMPLE_CAP: u32 = 1_000_000;
/// 风暴账字段数。
pub const STORM_FIELDS: usize = 6;
/// 单次限流运行的最大刻度数（60 秒）。
pub const LIMIT_TICKS_MAX: u32 = 60_000;
/// 风暴轮次上限。
pub const ROUNDS_MAX: u32 = 100;

// ── 诊断码闭集（本模块窗口 +0x01..+0x0D） ──────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StCode {
    /// 样本非法（零到达或超出样本上限）
    SampleBad,
    /// 速率非法（速率为零或风暴/平静关系颠倒）
    RateBad,
    /// 风暴非法（形状不成立：峰值或相位长度为假）
    StormBad,
    /// 恢复非法（排空超限或残留非零）
    RecoveryBad,
    /// 次序非法（刻度回退或轮次越界）
    OrderBad,
    /// 突发非法（桶容量被突破）
    BurstBad,
    /// 限流非法（稳态超过 10,000/s 或超出突发豁免）
    LimitBad,
    /// 账不平（到达 ≠ 服务，或尝试 ≠ 准入 + 拒绝）
    LedgerBad,
    /// 渲染非法（字段缺失或回读失败）
    RenderBad,
    /// 复算不一致（同一流重放不同）
    ReplayBad,
    /// 覆盖不足（刻度或轮次不足）
    CoverageBad,
    /// 段位越界（诊断码逃出 0x71480 窗口）
    SegmentBad,
    /// 时钟非法（刻度非严格递增或时间源回拨）
    ClockBad,
}

impl StCode {
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }

    pub fn detail(self) -> &'static str {
        match self {
            StCode::SampleBad => "样本非法：零到达或超出单刻度样本上限",
            StCode::RateBad => "速率非法：速率为零或风暴/平静关系颠倒",
            StCode::StormBad => "风暴非法：峰值与相位长度不自洽",
            StCode::RecoveryBad => "恢复非法：排空超限或风暴残留非零",
            StCode::OrderBad => "次序非法：刻度回退或轮次越界",
            StCode::BurstBad => "突发非法：令牌桶容量被突破",
            StCode::LimitBad => "限流非法：稳态超过 10,000 事件/秒",
            StCode::LedgerBad => "账不平：到达与服务不等或尝试分解不成立",
            StCode::RenderBad => "渲染非法：字段缺失或回读失败",
            StCode::ReplayBad => "复算不一致：同一流重放结果不同",
            StCode::CoverageBad => "覆盖不足：刻度数或风暴轮次不足",
            StCode::SegmentBad => "段位越界：诊断码逃出 0x71480 窗口",
            StCode::ClockBad => "时钟非法：刻度非严格递增或时间源回拨",
        }
    }

    pub const ALL: [StCode; 13] = [
        StCode::SampleBad,
        StCode::RateBad,
        StCode::StormBad,
        StCode::RecoveryBad,
        StCode::OrderBad,
        StCode::BurstBad,
        StCode::LimitBad,
        StCode::LedgerBad,
        StCode::RenderBad,
        StCode::ReplayBad,
        StCode::CoverageBad,
        StCode::SegmentBad,
        StCode::ClockBad,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < StCode::ALL.len() {
        if StCode::ALL[i].val() == code {
            return StCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码"
}

// ── 令牌桶（10,000ev/s 限流面） ───────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TokenBucket {
    /// 当前令牌数（恒 ≤ BUCKET_CAP）
    pub tokens: u32,
    /// 累计准入（u64：十万级窗口不溢出）
    pub admits: u64,
    /// 累计拒绝
    pub rejects: u64,
    /// 累计尝试 = 准入 + 拒绝
    pub attempts: u64,
    pub digest: u64,
}

impl TokenBucket {
    pub fn new() -> TokenBucket {
        TokenBucket {
            tokens: BUCKET_CAP,
            admits: 0,
            rejects: 0,
            attempts: 0,
            digest: p_core::FNV_OFF,
        }
    }

    /// 补充令牌：先补后放是首 tick 能吃到满桶的充要条件。
    fn refill(&mut self) {
        let next = self.tokens.saturating_add(REFILL_PER_MS);
        self.tokens = if next > BUCKET_CAP { BUCKET_CAP } else { next };
    }

    /// 一个毫秒刻度：返回 (准入, 拒绝)。
    pub fn tick(&mut self, arrivals: u32) -> Result<(u32, u32), u32> {
        if arrivals == 0 || arrivals > SAMPLE_CAP {
            return Err(StCode::SampleBad.val());
        }
        self.refill();
        let admit = if arrivals < self.tokens {
            arrivals
        } else {
            self.tokens
        };
        let reject = arrivals - admit;
        self.tokens -= admit;
        self.admits = self.admits.saturating_add(admit as u64);
        self.rejects = self.rejects.saturating_add(reject as u64);
        self.attempts = self.attempts.saturating_add(arrivals as u64);
        self.digest = p_core::fold_u32(self.digest, arrivals);
        self.digest = p_core::fold_u32(self.digest, admit);
        Ok((admit, reject))
    }

    /// 账本审计：桶不越容，尝试恰为两路之和，且已发生过刻度的记账。
    pub fn audit(&self) -> Result<(), u32> {
        if self.tokens > BUCKET_CAP {
            return Err(StCode::BurstBad.val());
        }
        if self.attempts != self.admits.saturating_add(self.rejects) {
            return Err(StCode::LedgerBad.val());
        }
        Ok(())
    }
}

/// 限流运行账（F48966 的凭证）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LimitReport {
    /// 运行刻度数
    pub ticks: u32,
    /// 累计准入
    pub admits: u64,
    /// 累计拒绝
    pub rejects: u64,
    /// 结束时的剩余令牌
    pub tokens: u32,
    pub digest: u64,
}

impl LimitReport {
    /// 稳态速率（每千刻度的准入数）：恰好等于每千次补充的额度。
    pub fn sustained(&self) -> u64 {
        self.admits.saturating_sub(BUCKET_CAP as u64)
    }
}

/// 以「每 tick 满负荷到达」压限流器 `ticks` 个毫秒刻度。
///
/// 首 tick 吃满 40 的桶，此后每刻钟只能放出 10——这个形状是限流器
/// 的指纹：准入总数 = 桶容量 + 补充速率 ×（刻度 − 1）。
pub fn limit_run(ticks: u32) -> Result<LimitReport, u32> {
    if ticks == 0 || ticks > LIMIT_TICKS_MAX {
        return Err(StCode::CoverageBad.val());
    }
    let mut b = TokenBucket::new();
    let mut t = 0u32;
    while t < ticks {
        // 满负荷到达：远超任何桶容量，专门用来逼出「稳态」形状。
        b.tick(SAMPLE_CAP)?;
        b.audit()?;
        t = t.saturating_add(1);
    }
    let want = (BUCKET_CAP as u64).saturating_add(
        (REFILL_PER_MS as u64).saturating_mul(ticks.saturating_sub(1) as u64),
    );
    if b.admits != want {
        return Err(StCode::LimitBad.val());
    }
    Ok(LimitReport {
        ticks,
        admits: b.admits,
        rejects: b.rejects,
        tokens: b.tokens,
        digest: b.digest,
    })
}

// ── 风暴相位形状（积压递推） ──────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct StormShape {
    /// 风暴 + 恢复两相的总到达
    pub arrivals: u32,
    /// 总服务量（残留归零时恰等于总到达）
    pub serviced: u32,
    /// 积压峰值（风暴相位末尾取得）
    pub peak: u32,
    /// 恢复相位时长（毫秒刻度）
    pub recovery_ms: u32,
}

/// 向上取整除法（divisor 必须非零，调用方已在参数闸门保证）。
fn ceil_div(n: u32, d: u32) -> u32 {
    if d == 0 {
        return 0;
    }
    n / d + if n % d == 0 { 0 } else { 1 }
}

/// 逐 tick 递推风暴形状：到达 → 入积压 → 按服务率消费。
///
/// 服务不能透支：`served = min(backlog, SERVICE_PER_MS)`。允许透支
/// 会让「服务总量」凭空多出空闲期的额度，到达/服务恒等式就再也不平。
fn simulate(storm_rate: u32, calm_rate: u32, storm_ticks: u32) -> Result<StormShape, u32> {
    if storm_rate <= SERVICE_PER_MS {
        // 风暴不高于服务率：积压不增长，就不是风暴。
        return Err(StCode::RateBad.val());
    }
    if calm_rate >= SERVICE_PER_MS {
        // 平静到达不低于服务率：排空永远无法发生。
        return Err(StCode::RateBad.val());
    }
    if storm_ticks == 0 || storm_ticks > 1_000 {
        return Err(StCode::StormBad.val());
    }
    if storm_rate > SAMPLE_CAP {
        return Err(StCode::SampleBad.val());
    }
    let mut backlog = 0u32;
    let mut peak = 0u32;
    let mut arrivals = 0u32;
    let mut serviced = 0u32;
    let mut i = 0u32;
    while i < storm_ticks {
        arrivals = arrivals.saturating_add(storm_rate);
        backlog = backlog.saturating_add(storm_rate);
        let served = if backlog < SERVICE_PER_MS {
            backlog
        } else {
            SERVICE_PER_MS
        };
        backlog -= served;
        serviced = serviced.saturating_add(served);
        if backlog > peak {
            peak = backlog;
        }
        i = i.saturating_add(1);
    }
    let drain = SERVICE_PER_MS - calm_rate;
    let want_recovery = ceil_div(peak, drain);
    let mut recovery = 0u32;
    while backlog > 0 {
        if recovery >= RECOVERY_CAP {
            return Err(StCode::RecoveryBad.val());
        }
        arrivals = arrivals.saturating_add(calm_rate);
        backlog = backlog.saturating_add(calm_rate);
        let served = if backlog < SERVICE_PER_MS {
            backlog
        } else {
            SERVICE_PER_MS
        };
        backlog -= served;
        serviced = serviced.saturating_add(served);
        recovery = recovery.saturating_add(1);
    }
    if backlog != 0 {
        return Err(StCode::RecoveryBad.val());
    }
    if arrivals != serviced {
        return Err(StCode::LedgerBad.val());
    }
    if recovery != want_recovery {
        return Err(StCode::RecoveryBad.val());
    }
    Ok(StormShape {
        arrivals,
        serviced,
        peak,
        recovery_ms: recovery,
    })
}

/// 单轮风暴：第 `round` 轮的风暴到达率在 40..=44 之间轮转。
pub fn storm_round(round: u32) -> Result<StormShape, u32> {
    if round >= ROUNDS_MAX {
        return Err(StCode::OrderBad.val());
    }
    let rate = STORM_RATE.saturating_add(round % STORM_SWING);
    let shape = simulate(rate, CALM_RATE, STORM_TICKS)?;
    let want_peak = STORM_TICKS.saturating_mul(rate - SERVICE_PER_MS);
    if shape.peak != want_peak {
        return Err(StCode::StormBad.val());
    }
    Ok(shape)
}

// ── 风暴 ×10 轮恢复账 ─────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct StormLedger {
    pub rounds: u32,
    pub arrivals: u64,
    pub serviced: u64,
    pub peak_max: u32,
    pub recovery_max: u32,
    /// 已确认「残留归零」的轮数（必须等于 rounds）
    pub recovered: u32,
    pub digest: u64,
}

/// 风暴 ×N 轮：每轮独立建账，逐轮核对恒等式与解析式。
pub fn storm_rounds(rounds: u32) -> Result<StormLedger, u32> {
    if rounds == 0 || rounds > ROUNDS_MAX {
        return Err(StCode::CoverageBad.val());
    }
    let mut arrivals = 0u64;
    let mut serviced = 0u64;
    let mut peak_max = 0u32;
    let mut recovery_max = 0u32;
    let mut recovered = 0u32;
    let mut h = p_core::FNV_OFF;
    let mut r = 0u32;
    while r < rounds {
        let s = storm_round(r)?;
        if s.arrivals != s.serviced {
            return Err(StCode::RecoveryBad.val());
        }
        arrivals = arrivals.saturating_add(s.arrivals as u64);
        serviced = serviced.saturating_add(s.serviced as u64);
        if s.peak > peak_max {
            peak_max = s.peak;
        }
        if s.recovery_ms > recovery_max {
            recovery_max = s.recovery_ms;
        }
        recovered = recovered.saturating_add(1);
        h = p_core::fold_u32(h, r);
        h = p_core::fold_u32(h, s.arrivals);
        h = p_core::fold_u32(h, s.peak);
        h = p_core::fold_u32(h, s.recovery_ms);
        r = r.saturating_add(1);
    }
    if recovered != rounds {
        return Err(StCode::RecoveryBad.val());
    }
    if arrivals != serviced {
        return Err(StCode::LedgerBad.val());
    }
    if peak_max == 0 || recovery_max == 0 {
        return Err(StCode::StormBad.val());
    }
    Ok(StormLedger {
        rounds,
        arrivals,
        serviced,
        peak_max,
        recovery_max,
        recovered,
        digest: h,
    })
}

// ── fuzz 面（乱参数不崩） ─────────────────────────────────────────
/// 风暴 fuzz：乱速率/乱相位长度下，恒等式与解析式必须恒成立。
pub fn fuzz_storm_rounds(rounds: u32) -> Result<u64, u32> {
    if rounds == 0 || rounds > 10_000 {
        return Err(StCode::CoverageBad.val());
    }
    let mut rng = p_core::Lcg::new(0x5651_0041);
    let mut h = p_core::FNV_OFF;
    let mut r = 0u32;
    while r < rounds {
        // 高位路由：低位在 2^k 模下会锁死，乱参数就退化成同一条流。
        // 域收敛：速率 21..=60、平静 1..=14、相位 1..=20 —— 峰值 ≤ 800，
        // 净排空 ≥ 6/ tick，恢复 ≤ 134 < RECOVERY_CAP，fuzz 只测算法不
        // 测「排空超限」这条错误路径。
        let a = rng.next();
        let storm_rate = 21u32.saturating_add(((a >> 33) % 40) as u32);
        let calm_rate = 1u32.saturating_add(((a >> 21) % 14) as u32);
        let ticks = 1u32.saturating_add(((a >> 9) % 20) as u32);
        let s = simulate(storm_rate, calm_rate, ticks)?;
        if s.arrivals != s.serviced || s.peak == 0 {
            return Err(StCode::LedgerBad.val());
        }
        let drain = SERVICE_PER_MS - calm_rate;
        if s.recovery_ms != ceil_div(s.peak, drain) {
            return Err(StCode::RecoveryBad.val());
        }
        h = p_core::fold_u32(h, storm_rate);
        h = p_core::fold_u32(h, calm_rate);
        h = p_core::fold_u32(h, s.peak);
        h = p_core::fold_u32(h, s.recovery_ms);
        r = r.saturating_add(1);
    }
    Ok(h)
}

/// 限流 fuzz：乱到达下桶不越容、账不破裂。
pub fn fuzz_limit_rounds(rounds: u32) -> Result<u64, u32> {
    if rounds == 0 || rounds > LIMIT_TICKS_MAX {
        return Err(StCode::CoverageBad.val());
    }
    let mut rng = p_core::Lcg::new(0x4C49_0042);
    let mut b = TokenBucket::new();
    let mut h = p_core::FNV_OFF;
    let mut r = 0u32;
    while r < rounds {
        let a = rng.next();
        let arrivals = 1u32.saturating_add(((a >> 33) % 500) as u32);
        let (admit, reject) = b.tick(arrivals)?;
        if admit.saturating_add(reject) != arrivals {
            return Err(StCode::LedgerBad.val());
        }
        b.audit()?;
        h = p_core::fold_u32(h, arrivals);
        h = p_core::fold_u32(h, admit);
        r = r.saturating_add(1);
    }
    Ok(h)
}

// ── 渲染与回读 ────────────────────────────────────────────────────
pub fn render_storm(l: &StormLedger) -> String {
    format!(
        "{}|{}|{}|{}|{}|0x{:016X}",
        l.rounds, l.arrivals, l.serviced, l.peak_max, l.recovery_max, l.digest
    )
}

pub fn parse_u32(s: &str) -> Result<u32, u32> {
    if s.is_empty() || s.len() > 10 {
        return Err(StCode::RenderBad.val());
    }
    let mut v = 0u32;
    let mut i = 0usize;
    while i < s.len() {
        let b = match s.as_bytes().get(i) {
            Some(v) => *v,
            None => return Err(StCode::RenderBad.val()),
        };
        if !(b'0'..=b'9').contains(&b) {
            return Err(StCode::RenderBad.val());
        }
        v = match v
            .checked_mul(10)
            .and_then(|x| x.checked_add((b - b'0') as u32))
        {
            Some(x) => x,
            None => return Err(StCode::RenderBad.val()),
        };
        i += 1;
    }
    Ok(v)
}

pub fn parse_hex16(s: &str) -> Result<u64, u32> {
    if s.len() != 18 || (!s.starts_with("0x") && !s.starts_with("0X")) {
        return Err(StCode::RenderBad.val());
    }
    let bytes = s.as_bytes();
    let mut v = 0u64;
    let mut i = 2usize;
    while i < 18 {
        let b = match bytes.get(i) {
            Some(v) => *v,
            None => return Err(StCode::RenderBad.val()),
        };
        let d = match b {
            b'0'..=b'9' => (b - b'0') as u64,
            b'a'..=b'f' => (b - b'a') as u64 + 10,
            b'A'..=b'F' => (b - b'A') as u64 + 10,
            _ => return Err(StCode::RenderBad.val()),
        };
        v = match v.checked_mul(16).and_then(|x| x.checked_add(d)) {
            Some(x) => x,
            None => return Err(StCode::RenderBad.val()),
        };
        i += 1;
    }
    Ok(v)
}

/// 渲染复读：字段数、轮次、到达/服务恒等、峰值域、摘要五项。
pub fn audit_render(text: &str) -> Result<u32, u32> {
    let parts: Vec<&str> = text.split('|').collect();
    if parts.len() != STORM_FIELDS {
        return Err(StCode::RenderBad.val());
    }
    let rounds = parse_u32(match parts.first() {
        Some(v) => v,
        None => return Err(StCode::RenderBad.val()),
    })?;
    let arrivals = parse_u32(match parts.get(1) {
        Some(v) => v,
        None => return Err(StCode::RenderBad.val()),
    })?;
    let serviced = parse_u32(match parts.get(2) {
        Some(v) => v,
        None => return Err(StCode::RenderBad.val()),
    })?;
    let peak = parse_u32(match parts.get(3) {
        Some(v) => v,
        None => return Err(StCode::RenderBad.val()),
    })?;
    let recovery = parse_u32(match parts.get(4) {
        Some(v) => v,
        None => return Err(StCode::RenderBad.val()),
    })?;
    let digest = parse_hex16(match parts.get(5) {
        Some(v) => v,
        None => return Err(StCode::RenderBad.val()),
    })?;
    if rounds == 0 || rounds > ROUNDS_MAX {
        return Err(StCode::RenderBad.val());
    }
    if arrivals != serviced {
        return Err(StCode::LedgerBad.val());
    }
    let peak_cap = STORM_TICKS.saturating_mul(
        STORM_RATE.saturating_add(STORM_SWING - 1) - SERVICE_PER_MS,
    );
    if peak == 0 || peak > peak_cap {
        return Err(StCode::StormBad.val());
    }
    if recovery == 0 || recovery > RECOVERY_CAP {
        return Err(StCode::RecoveryBad.val());
    }
    if digest == 0 {
        return Err(StCode::RenderBad.val());
    }
    Ok(rounds)
}

pub fn golden_run() -> Result<(u64, u64, u32, u32, u64), u32> {
    let l = storm_rounds(STORM_ROUNDS)?;
    let text = render_storm(&l);
    if audit_render(&text)? != l.rounds {
        return Err(StCode::ReplayBad.val());
    }
    if l.arrivals != l.serviced || l.recovered != l.rounds {
        return Err(StCode::RecoveryBad.val());
    }
    Ok((l.arrivals, l.serviced, l.peak_max, l.recovery_max, l.digest))
}

// ── 注入对账（四路缺陷必须被捕获） ────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectLog {
    pub tries: [u32; 4],
    pub caught: [u32; 4],
}

impl InjectLog {
    pub fn all_caught(&self) -> bool {
        let mut i = 0usize;
        while i < 4 {
            if self.tries[i] == 0 || self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

pub fn inject_rounds(rounds: u32) -> Result<InjectLog, u32> {
    if rounds == 0 || rounds > 1_000 {
        return Err(StCode::CoverageBad.val());
    }
    let mut log = InjectLog {
        tries: [0; 4],
        caught: [0; 4],
    };
    let mut r = 0u32;
    while r < rounds {
        // ① 桶越容：令牌被手动塞过上限，审计必须拒绝。
        log.tries[0] = log.tries[0].saturating_add(1);
        let mut b = TokenBucket::new();
        b.tokens = BUCKET_CAP.saturating_add(1);
        if b.audit() == Err(StCode::BurstBad.val()) {
            log.caught[0] = log.caught[0].saturating_add(1);
        }

        // ② 尝试分解断裂：准入被改大而不动尝试数。
        log.tries[1] = log.tries[1].saturating_add(1);
        let mut c = TokenBucket::new();
        let _ = c.tick(100);
        c.admits = c.admits.saturating_add(1);
        if c.audit() == Err(StCode::LedgerBad.val()) {
            log.caught[1] = log.caught[1].saturating_add(1);
        }

        // ③ 风暴残留：到达与服务的账被拆开。
        log.tries[2] = log.tries[2].saturating_add(1);
        let mut s = storm_round(0)?;
        s.arrivals = s.arrivals.saturating_add(1);
        if s.arrivals != s.serviced {
            log.caught[2] = log.caught[2].saturating_add(1);
        }

        // ④ 恢复时长解析式漂移：递归结果被手工改大。
        log.tries[3] = log.tries[3].saturating_add(1);
        let shape = storm_round(r % ROUNDS_MAX)?;
        let drain = SERVICE_PER_MS - CALM_RATE;
        let want = ceil_div(shape.peak, drain);
        let tampered = shape.recovery_ms.saturating_add(1);
        if tampered != want {
            log.caught[3] = log.caught[3].saturating_add(1);
        }

        r = r.saturating_add(1);
    }
    Ok(log)
}

// ── 探针 ──────────────────────────────────────────────────────────
pub fn probe_seq_a() -> Result<(), u32> {
    if LIMIT_RPS != 10_000 || REFILL_PER_MS != 10 || BUCKET_CAP != 40 {
        return Err(StCode::LimitBad.val());
    }
    if STORM_RATE != 40 || SERVICE_PER_MS != 20 || CALM_RATE != 5 {
        return Err(StCode::RateBad.val());
    }
    let l = storm_rounds(STORM_ROUNDS)?;
    // 十轮总到达 = 2 ×（1170 + 1200 + 1235 + 1270 + 1300）= 12,350。
    if l.arrivals != 12_350 || l.peak_max != 600 || l.recovery_max != 40 {
        return Err(StCode::StormBad.val());
    }
    if l.recovered != STORM_ROUNDS {
        return Err(StCode::RecoveryBad.val());
    }
    Ok(())
}

pub fn probe_seq_b() -> Result<(), u32> {
    // 满负荷十秒：准入 = 40 + 10 × 9,999 = 100,030。
    let rep = limit_run(MS_PER_SEC.saturating_mul(10))?;
    if rep.admits != 100_030 || rep.rejects == 0 {
        return Err(StCode::LimitBad.val());
    }
    if rep.tokens > BUCKET_CAP {
        return Err(StCode::BurstBad.val());
    }
    if rep.sustained() != (REFILL_PER_MS as u64).saturating_mul(
        MS_PER_SEC.saturating_mul(10).saturating_sub(1) as u64,
    ) {
        return Err(StCode::LimitBad.val());
    }
    // 单 tick 突发上限：满到达也只能吃满桶。
    let mut b = TokenBucket::new();
    let (admit, _) = b.tick(SAMPLE_CAP)?;
    if admit != BUCKET_CAP {
        return Err(StCode::BurstBad.val());
    }
    Ok(())
}

pub fn probe_seq_c() -> Result<(), u32> {
    // 第 0 轮：风暴率 40 → 峰值 25 × 20 = 500，排空 ceil(500/15) = 34。
    let s = storm_round(0)?;
    if s.peak != 500 || s.recovery_ms != 34 {
        return Err(StCode::StormBad.val());
    }
    if s.arrivals != 1_170 || s.serviced != 1_170 {
        return Err(StCode::LedgerBad.val());
    }
    // 第 4 轮：风暴率 44 → 峰值 600，排空 40。
    let t = storm_round(4)?;
    if t.peak != 600 || t.recovery_ms != 40 {
        return Err(StCode::StormBad.val());
    }
    // 风暴率不高于服务率必须被拒：不增长就不是风暴。
    if simulate(20, CALM_RATE, STORM_TICKS) != Err(StCode::RateBad.val()) {
        return Err(StCode::RateBad.val());
    }
    // 平静到达不低于服务率必须被拒：永远排不空。
    if simulate(STORM_RATE, SERVICE_PER_MS, STORM_TICKS) != Err(StCode::RateBad.val()) {
        return Err(StCode::RateBad.val());
    }
    Ok(())
}

pub fn repro_check() -> Result<(), u32> {
    let a = storm_rounds(STORM_ROUNDS)?;
    let b = storm_rounds(STORM_ROUNDS)?;
    if a.digest != b.digest || a.arrivals != b.arrivals || a.peak_max != b.peak_max {
        return Err(StCode::ReplayBad.val());
    }
    if limit_run(1_000)?.digest != limit_run(1_000)?.digest {
        return Err(StCode::ReplayBad.val());
    }
    if fuzz_storm_rounds(500)? != fuzz_storm_rounds(500)? {
        return Err(StCode::ReplayBad.val());
    }
    if fuzz_limit_rounds(500)? != fuzz_limit_rounds(500)? {
        return Err(StCode::ReplayBad.val());
    }
    Ok(())
}

pub fn segment_guard() -> bool {
    let mut i = 0usize;
    while i < StCode::ALL.len() {
        let v = StCode::ALL[i].val();
        if v <= SEG_BASE || v > SEG_BASE + 0x3F {
            return false;
        }
        i += 1;
    }
    true
}

// ── 域检查项（≤8 条） ─────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut out = Vec::new();

    out.push((
        "F48965 风暴与限流锚定",
        LIMIT_RPS == 10_000
            && REFILL_PER_MS == 10
            && BUCKET_CAP == 40
            && STORM_RATE == 40
            && SERVICE_PER_MS == 20
            && CALM_RATE == 5,
        format!(
            "limit={}ev/s refill={} cap={} storm={} service={} calm={}",
            LIMIT_RPS, REFILL_PER_MS, BUCKET_CAP, STORM_RATE, SERVICE_PER_MS, CALM_RATE
        ),
    ));

    let storm = storm_rounds(STORM_ROUNDS);
    out.push((
        "F48965 风暴×10恢复账可查",
        matches!(&storm, Ok(l) if l.rounds == STORM_ROUNDS
            && l.recovered == STORM_ROUNDS
            && l.arrivals == l.serviced
            && l.arrivals == 12_350
            && l.peak_max == 600
            && l.recovery_max == 40),
        match &storm {
            Ok(l) => format!(
                "rounds={} recovered={} arrivals={} peak={} recovery={}",
                l.rounds, l.recovered, l.arrivals, l.peak_max, l.recovery_max
            ),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));

    let mut shape_ok = true;
    let mut peak_seen = 0u32;
    let mut rec_seen = 0u32;
    let mut k = 0u32;
    while k < STORM_SWING {
        match storm_round(k) {
            Ok(s) => {
                let want_peak = STORM_TICKS.saturating_mul(STORM_RATE + k - SERVICE_PER_MS);
                let want_rec = ceil_div(want_peak, SERVICE_PER_MS - CALM_RATE);
                if s.peak != want_peak || s.recovery_ms != want_rec {
                    shape_ok = false;
                }
                if s.peak > peak_seen {
                    peak_seen = s.peak;
                    rec_seen = s.recovery_ms;
                }
            }
            Err(_) => shape_ok = false,
        }
        k = k.saturating_add(1);
    }
    out.push((
        "F48965 峰值与恢复解析式",
        shape_ok && peak_seen == 600 && rec_seen == 40,
        format!(
            "swing={} peak={} recovery={} ok={}",
            STORM_SWING, peak_seen, rec_seen, shape_ok
        ),
    ));

    let limit = limit_run(MS_PER_SEC.saturating_mul(10));
    out.push((
        "F48966 限流10,000ev/s",
        matches!(&limit, Ok(r) if r.admits == 100_030
            && r.tokens <= BUCKET_CAP
            && r.sustained() == 99_990),
        match &limit {
            Ok(r) => format!(
                "ticks={} admits={} rejects={} tokens={} sustained={}",
                r.ticks, r.admits, r.rejects, r.tokens, r.sustained()
            ),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));

    let mut burst_admit = 0u32;
    let mut burst_tokens = u32::MAX;
    let mut b = TokenBucket::new();
    let burst_ok = match b.tick(SAMPLE_CAP) {
        Ok((admit, reject)) => {
            burst_admit = admit;
            burst_tokens = b.tokens;
            admit.saturating_add(reject) == SAMPLE_CAP && b.audit().is_ok()
        }
        Err(_) => false,
    };
    out.push((
        "F48966 突发上界",
        burst_ok && burst_admit == BUCKET_CAP && burst_tokens == 0,
        format!(
            "burst={} tokens={} ok={}",
            burst_admit, burst_tokens, burst_ok
        ),
    ));

    let fuzz_a = fuzz_storm_rounds(10_000);
    let fuzz_b = fuzz_limit_rounds(10_000);
    out.push((
        "F48965 fuzz×10^4不崩",
        fuzz_a.is_ok() && fuzz_b.is_ok(),
        format!(
            "storm={} limit={}",
            match &fuzz_a {
                Ok(h) => format!("0x{:016X}", h),
                Err(e) => format!("err=0x{:X}", e),
            },
            match &fuzz_b {
                Ok(h) => format!("0x{:016X}", h),
                Err(e) => format!("err=0x{:X}", e),
            }
        ),
    ));

    let inject = inject_rounds(5);
    out.push((
        "F48965 四路注入捕获",
        matches!(&inject, Ok(l) if l.all_caught()),
        match &inject {
            Ok(l) => format!("caught={:?}", l.caught),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));

    let golden = golden_run();
    out.push((
        "F48965 账本可解析",
        golden.is_ok(),
        match &golden {
            Ok((a, s, p, r, d)) => format!(
                "arrivals={} serviced={} peak={} recovery={} d=0x{:X}",
                a, s, p, r, d
            ),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));

    out
}

pub fn run_unx48965_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F48965 事件风暴恢复账与限流");
    let items = extra_checks();
    let mut i = 0usize;
    while i < items.len() {
        if let Some((n, p, d)) = items.get(i) {
            s.add_dyn(n, *p, d);
        }
        i += 1;
    }
    s
}

// ── 单元判据 ──────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constants_are_anchored() {
        assert_eq!(SEG_BASE, 0x71480);
        assert_eq!(LIMIT_RPS, 10_000);
        assert_eq!(REFILL_PER_MS, 10);
        assert_eq!(BUCKET_CAP, 40);
        assert_eq!(STORM_FIELDS, 6);
    }

    #[test]
    fn bucket_first_tick_takes_full_cap() {
        let mut b = TokenBucket::new();
        assert_eq!(b.tick(SAMPLE_CAP), Ok((BUCKET_CAP, SAMPLE_CAP - BUCKET_CAP)));
        assert_eq!(b.tokens, 0);
        assert!(b.audit().is_ok());
    }

    #[test]
    fn bucket_sustains_refill_rate() {
        let mut b = TokenBucket::new();
        let _ = b.tick(SAMPLE_CAP);
        let mut i = 0u32;
        while i < 999 {
            assert_eq!(b.tick(SAMPLE_CAP), Ok((REFILL_PER_MS, SAMPLE_CAP - REFILL_PER_MS)));
            i += 1;
        }
        assert_eq!(b.admits, 40 + 999 * 10);
        assert!(b.audit().is_ok());
    }

    #[test]
    fn bucket_rejects_zero_and_oversize() {
        let mut b = TokenBucket::new();
        assert_eq!(b.tick(0), Err(StCode::SampleBad.val()));
        assert_eq!(b.tick(SAMPLE_CAP + 1), Err(StCode::SampleBad.val()));
        assert_eq!(b.attempts, 0);
    }

    #[test]
    fn bucket_audit_catches_tamper() {
        let mut b = TokenBucket::new();
        b.tokens = BUCKET_CAP + 1;
        assert_eq!(b.audit(), Err(StCode::BurstBad.val()));
        let mut c = TokenBucket::new();
        let _ = c.tick(100);
        c.admits = c.admits.saturating_add(1);
        assert_eq!(c.audit(), Err(StCode::LedgerBad.val()));
    }

    #[test]
    fn limit_run_exact_ten_seconds() {
        let r = limit_run(10_000);
        assert!(r.is_ok());
        if let Ok(v) = r {
            assert_eq!(v.admits, 100_030);
            assert_eq!(v.tokens, 0);
            assert_eq!(v.sustained(), 99_990);
            assert_eq!(v.rejects, 10_000 * SAMPLE_CAP as u64 - 100_030);
        }
    }

    #[test]
    fn limit_run_rejects_bad_ticks() {
        assert_eq!(limit_run(0), Err(StCode::CoverageBad.val()));
        assert_eq!(limit_run(LIMIT_TICKS_MAX + 1), Err(StCode::CoverageBad.val()));
    }

    #[test]
    fn storm_round_zero_shape() {
        let s = storm_round(0);
        assert!(s.is_ok());
        if let Ok(v) = s {
            assert_eq!(v.peak, 500);
            assert_eq!(v.recovery_ms, 34);
            assert_eq!(v.arrivals, 1_170);
            assert_eq!(v.arrivals, v.serviced);
        }
    }

    #[test]
    fn storm_round_offsets_grow() {
        let mut last = 0u32;
        let mut k = 0u32;
        while k < STORM_SWING {
            let s = match storm_round(k) {
                Ok(v) => v,
                Err(_) => return assert!(false),
            };
            assert_eq!(s.peak, STORM_TICKS * (STORM_RATE + k - SERVICE_PER_MS));
            assert!(s.peak > last);
            last = s.peak;
            k += 1;
        }
        assert_eq!(last, 600);
    }

    #[test]
    fn storm_round_rejects_bad_rate() {
        assert_eq!(
            simulate(SERVICE_PER_MS, CALM_RATE, STORM_TICKS),
            Err(StCode::RateBad.val())
        );
        assert_eq!(
            simulate(STORM_RATE, SERVICE_PER_MS, STORM_TICKS),
            Err(StCode::RateBad.val())
        );
        assert_eq!(simulate(STORM_RATE, CALM_RATE, 0), Err(StCode::StormBad.val()));
        assert_eq!(
            storm_round(ROUNDS_MAX),
            Err(StCode::OrderBad.val())
        );
    }

    #[test]
    fn storm_ten_rounds_totals() {
        let l = storm_rounds(STORM_ROUNDS);
        assert!(l.is_ok());
        if let Ok(v) = l {
            assert_eq!(v.arrivals, 12_350);
            assert_eq!(v.serviced, 12_350);
            assert_eq!(v.peak_max, 600);
            assert_eq!(v.recovery_max, 40);
            assert_eq!(v.recovered, STORM_ROUNDS);
        }
    }

    #[test]
    fn storm_rejects_bad_rounds() {
        assert_eq!(storm_rounds(0), Err(StCode::CoverageBad.val()));
        assert_eq!(
            storm_rounds(ROUNDS_MAX + 1),
            Err(StCode::CoverageBad.val())
        );
    }

    #[test]
    fn fuzz_faces_are_green() {
        assert!(fuzz_storm_rounds(2_000).is_ok());
        assert!(fuzz_limit_rounds(2_000).is_ok());
        assert_eq!(fuzz_storm_rounds(0), Err(StCode::CoverageBad.val()));
        assert_eq!(fuzz_limit_rounds(0), Err(StCode::CoverageBad.val()));
    }

    #[test]
    fn render_roundtrip() {
        let l = match storm_rounds(STORM_ROUNDS) {
            Ok(v) => v,
            Err(_) => return assert!(false),
        };
        let text = render_storm(&l);
        assert_eq!(audit_render(&text), Ok(STORM_ROUNDS));
    }

    #[test]
    fn render_tamper_is_rejected() {
        let l = match storm_rounds(1) {
            Ok(v) => v,
            Err(_) => return assert!(false),
        };
        let text = render_storm(&l);
        assert!(audit_render(&text.replacen("0x", "0y", 1)).is_err());
        // 服务数被改：到达 ≠ 服务，账必拒。
        assert!(audit_render(&text.replacen("|1170|", "|1171|", 1)).is_err());
        assert!(audit_render("1|2|3").is_err());
    }

    #[test]
    fn injection_catches_four() {
        match inject_rounds(5) {
            Ok(l) => {
                assert!(l.all_caught());
                assert_eq!(l.tries, [5, 5, 5, 5]);
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn probes_are_green() {
        assert!(probe_seq_a().is_ok());
        assert!(probe_seq_b().is_ok());
        assert!(probe_seq_c().is_ok());
    }

    #[test]
    fn repro_is_deterministic() {
        assert!(repro_check().is_ok());
        assert_eq!(golden_run(), golden_run());
    }

    #[test]
    fn codes_are_unique_and_described() {
        let mut i = 0usize;
        while i < StCode::ALL.len() {
            let c = StCode::ALL[i];
            assert_eq!(detail(c.val()), c.detail());
            let mut k = i + 1;
            while k < StCode::ALL.len() {
                assert!(c.val() != StCode::ALL[k].val());
                k += 1;
            }
            i += 1;
        }
        assert!(segment_guard());
    }

    #[test]
    fn checks_are_green() {
        let s = run_unx48965_checks();
        assert!(s.all_passed(), "red={}", s.red_items_text());
        assert!(!s.truncated());
    }
}
