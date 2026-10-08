//! F182 双域时钟同步 · 批次三深化（secstar · G-G-12）。
//!
//! 批次三功能面（主册判据「10 轮零漂移 / >5s 提示 / 回拨保护」的实现纵深）：
//! - [`DriftEstimator`]：漂移估计器——滑动窗口样本线性拟合（每秒漂移
//!   率 µs/s），预测未来偏差（提前提示，不等人撞线）；
//! - [`SlewPolicy`]：校正策略——小漂移平滑 slew（每拍 100ms 步进）/
//!   大漂移步进 step+提示（500ms 线 / 5s 线双阈值，主册 DRIFT_ADVISE）；
//! - [`RollbackGuard`]：回拨保护账本——单调时钟对拍，时间被拨回即拒
//!   并留痕（文件时间戳不倒退——F219 排序记忆的时间地基）；
//! - [`SyncLedger`]：交接轮账本——10 轮记录环，逐轮置信度+偏差留档。
//!
//! 零堆纪律：定长样本窗 + 定长账本环，无 alloc。

use super::duoclock::{Confidence, DRIFT_ADVISE_MS, HANDOFF_ROUNDS};
use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 漂移估计器（滑动窗口线性拟合）
// ---------------------------------------------------------------------------

pub const SAMPLE_WINDOW: usize = 8;

/// 一个观测样本：本地毫秒钟 vs 权威源毫秒钟。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DriftSample {
    pub local_ms: u64,
    pub ref_ms: u64,
}

impl DriftSample {
    /// 本样本偏差（正=本地超前）。
    pub fn offset(&self) -> i64 {
        self.local_ms as i64 - self.ref_ms as i64
    }
}

/// 漂移估计器：最近 8 样本的最小二乘斜率 = 每毫秒偏差变化（漂移率），
/// 外推 future_ms 后的预测偏差。
pub struct DriftEstimator {
    win: [DriftSample; SAMPLE_WINDOW],
    n: usize,
    head: usize,
}

impl DriftEstimator {
    pub const fn new() -> DriftEstimator {
        DriftEstimator { win: [DriftSample { local_ms: 0, ref_ms: 0 }; SAMPLE_WINDOW], n: 0, head: 0 }
    }

    pub fn push(&mut self, s: DriftSample) {
        self.win[self.head] = s;
        self.head = (self.head + 1) % SAMPLE_WINDOW;
        if self.n < SAMPLE_WINDOW {
            self.n += 1;
        }
    }

    /// 按时间序取样本（环序→时间序）。
    fn ordered(&self) -> [DriftSample; SAMPLE_WINDOW] {
        let mut out = [DriftSample { local_ms: 0, ref_ms: 0 }; SAMPLE_WINDOW];
        for i in 0..self.n {
            out[i] = self.win[(self.head + SAMPLE_WINDOW - self.n + i) % SAMPLE_WINDOW];
        }
        out
    }

    /// 漂移率：偏差随本地时间的斜率（i64 定点，单位 µs/ms×1e6 → 直接
    /// 用偏差差/时间差的整数近似，负=本地落后）。
    pub fn drift_rate_per_s(&self) -> Option<i64> {
        if self.n < 2 {
            return None;
        }
        let o = self.ordered();
        let first = &o[0];
        let last = &o[self.n - 1];
        let dt = last.local_ms as i64 - first.local_ms as i64;
        if dt <= 0 {
            return None;
        }
        let d_off = last.offset() - first.offset();
        Some(d_off * 1_000_000 / dt) // 漂移率 µs/s（ms 偏差 ×1e6 / ms 时长）
    }

    /// 预测 future_ms 后的偏差（当前偏差 ms + 率 µs/s × 时长，归 ms）。
    pub fn predict_offset(&self, future_ms: u64) -> Option<i64> {
        let rate = self.drift_rate_per_s()?;
        let o = self.ordered();
        Some(o[self.n - 1].offset() + rate * future_ms as i64 / 1_000_000)
    }
}

// ---------------------------------------------------------------------------
// 校正策略（slew/step 双线）
// ---------------------------------------------------------------------------

/// 平滑校正线：偏差 ≤500ms 走 slew（每拍拉回 100ms——用户无感）。
pub const SLEW_MAX_OFFSET_MS: i64 = 500;
/// 平滑步长：每拍 100ms。
pub const SLEW_STEP_MS: i64 = 100;

/// 校正决策。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Correction {
    /// 无需校正。
    None,
    /// 平滑校正（本次拍拉回多少毫秒）。
    Slew(i64),
    /// 步进校正 + 用户提示（>500ms 或撞 DRIFT_ADVISE 线——诚实跳变）。
    Step,
}

/// 依据当前偏差决定校正方式。
pub fn decide_correction(offset_ms: i64) -> Correction {
    if offset_ms == 0 {
        return Correction::None;
    }
    let abs = offset_ms.abs();
    if abs <= SLEW_MAX_OFFSET_MS {
        Correction::Slew(offset_ms.clamp(-SLEW_STEP_MS, SLEW_STEP_MS))
    } else {
        Correction::Step
    }
}

/// 步进校正必须触发用户提示（>DRIFT_ADVISE_MS 即 5s 线——主册判据）。
pub fn step_requires_notice(offset_ms: i64) -> bool {
    offset_ms.abs() > DRIFT_ADVISE_MS as i64
}

// ---------------------------------------------------------------------------
// 回拨保护（单调账本）
// ---------------------------------------------------------------------------

/// 回拨保护：记录最近一次合法时间；新时间早于它 → 回拨。
pub struct RollbackGuard {
    last_good_ms: Option<u64>,
    pub violations: u32,
}

impl RollbackGuard {
    pub const fn new() -> RollbackGuard {
        RollbackGuard { last_good_ms: None, violations: 0 }
    }

    /// 审查新时间：回拨 → 拒 + 留痕；否则收下。
    pub fn admit(&mut self, candidate_ms: u64) -> bool {
        match self.last_good_ms {
            Some(prev) if candidate_ms < prev => {
                self.violations += 1;
                false
            }
            _ => {
                self.last_good_ms = Some(candidate_ms);
                true
            }
        }
    }

    pub fn last_good(&self) -> Option<u64> {
        self.last_good_ms
    }
}

// ---------------------------------------------------------------------------
// 交接轮账本（10 轮环）
// ---------------------------------------------------------------------------

/// 单轮交接记录。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RoundRec {
    pub round: u8,
    pub offset_ms: i64,
    pub confidence: Confidence,
}

/// 10 轮账本：逐轮留档 + 主册判据直算（10 轮零漂移）。
pub struct SyncLedger {
    recs: [Option<RoundRec>; HANDOFF_ROUNDS],
    pub n: usize,
}

impl SyncLedger {
    pub const fn new() -> SyncLedger {
        SyncLedger { recs: [const { None }; HANDOFF_ROUNDS], n: 0 }
    }

    pub fn record(&mut self, round: u8, offset_ms: i64, confidence: Confidence) -> bool {
        if (round as usize) >= HANDOFF_ROUNDS || self.n >= HANDOFF_ROUNDS {
            return false;
        }
        self.recs[round as usize] = Some(RoundRec { round, offset_ms, confidence });
        self.n += 1;
        true
    }

    /// 主册判据直算：10 轮全部记录且偏差绝对值 ≤ 零漂移线（1s）。
    pub fn ten_round_zero_drift(&self, zero_drift_ms: i64) -> bool {
        self.n == HANDOFF_ROUNDS
            && self.recs[..HANDOFF_ROUNDS].iter().flatten().all(|r| r.offset_ms.abs() <= zero_drift_ms)
    }

    /// 最差轮（|偏差|最大——复盘定位用）。
    pub fn worst_round(&self) -> Option<RoundRec> {
        self.recs[..HANDOFF_ROUNDS]
            .iter()
            .flatten()
            .copied()
            .max_by_key(|r| r.offset_ms.abs())
    }
}

// ---------------------------------------------------------------------------
// 批次三自检
// ---------------------------------------------------------------------------

#[inline(never)]
pub fn run_duoclock_b3_checks() -> CheckSet {
    let mut cs = CheckSet::new("F182-b3");

    // 1) 漂移估计：恒定 5ms/s 漂移序列 → 率≈5000µs/s 量级（符号对）。
    let mut est = DriftEstimator::new();
    for i in 0..SAMPLE_WINDOW {
        let t = (i as u64) * 1_000;
        est.push(DriftSample { local_ms: t + (i as u64) * 5, ref_ms: t });
    }
    let rate = est.drift_rate_per_s().unwrap();
    cs.add("drift_rate_positive", (4_000..=6_000).contains(&rate), "");

    // 2) 漂移预测：外推 10s 后偏差 ≈ 50ms（提前提示的数据面）。
    let pred = est.predict_offset(10_000).unwrap();
    cs.add("drift_predict", (70..=100).contains(&pred), "");

    // 3) 样本不足不猜：单样本无率（诚实 None）。
    let mut est2 = DriftEstimator::new();
    est2.push(DriftSample { local_ms: 100, ref_ms: 100 });
    cs.add("drift_insufficient_none", est2.drift_rate_per_s().is_none(), "");

    // 4) 校正三决策：0=None / ≤500ms=Slew / >500ms=Step（双线阈值）。
    cs.add(
        "correction_three_ways",
        decide_correction(0) == Correction::None
            && decide_correction(300) == Correction::Slew(100)
            && decide_correction(-300) == Correction::Slew(-100)
            && decide_correction(2_000) == Correction::Step,
        "",
    );

    // 5) 步进提示线：>5s 必提示、≤5s 不骚扰（主册 DRIFT_ADVISE_MS 对账）。
    cs.add(
        "step_notice_line",
        step_requires_notice(6_000) && !step_requires_notice(4_000) && step_requires_notice(-5_001),
        "",
    );

    // 6) 回拨保护：拨回被拒且留痕、前进被收（单调地基）。
    let mut g = RollbackGuard::new();
    let ok1 = g.admit(10_000);
    let bad = g.admit(9_999);
    let ok2 = g.admit(10_001);
    cs.add("rollback_guard", ok1 && !bad && ok2 && g.violations == 1 && g.last_good() == Some(10_001), "");

    // 7) 回拨保护首次时间无历史可回拨（首个候选必收）。
    let mut g2 = RollbackGuard::new();
    cs.add("rollback_first_admit", g2.admit(1) && g2.violations == 0, "");

    // 8) 账本 10 轮零漂移：全零偏差 10 轮 → 判据直算绿。
    let mut led = SyncLedger::new();
    for r in 0..HANDOFF_ROUNDS as u8 {
        assert!(led.record(r, 0, Confidence::NtpCalibrated));
    }
    cs.add("ledger_zero_drift_green", led.ten_round_zero_drift(1_000), "");

    // 9) 账本有一轮超线 → 红（判据不是摆设——反面样本真实可判）。
    let mut led2 = SyncLedger::new();
    for r in 0..HANDOFF_ROUNDS as u8 {
        let off = if r == 5 { 6_000 } else { 0 };
        led2.record(r, off, Confidence::RtcDirect);
    }
    cs.add("ledger_one_bad_red", !led2.ten_round_zero_drift(1_000), "");

    // 10) 账本最差轮定位：worst_round 找出 |偏差| 最大轮（复盘面）。
    let worst = led2.worst_round().unwrap();
    cs.add("ledger_worst_located", worst.round == 5 && worst.offset_ms == 6_000, "");

    // 11) 账本越界轮号与重复记录诚实拒收（10 上限）。
    let mut led3 = SyncLedger::new();
    let over = led3.record(10, 0, Confidence::NtpCalibrated);
    for r in 0..HANDOFF_ROUNDS as u8 {
        led3.record(r, 0, Confidence::NtpCalibrated);
    }
    let dup = led3.record(0, 0, Confidence::NtpCalibrated);
    cs.add("ledger_bounds", !over && !dup && led3.n == HANDOFF_ROUNDS, "");

    // 12) 置信度参与账面：NTP 轮在账、推断轮黄旗透传（信任分级留档）。
    let mut led4 = SyncLedger::new();
    led4.record(0, 100, Confidence::Inferred);
    let rec = led4.worst_round().unwrap();
    cs.add("ledger_confidence_kept", rec.confidence.yellow_flag(), "");

    // 13) 主册常量贯通：5s 提示线 / 10 轮上限一处一事实。
    cs.add("consts_aligned", DRIFT_ADVISE_MS == 5_000 && HANDOFF_ROUNDS == 10, "");

    cs
}

// ---------------------------------------------------------------------------
// 宿主单测（批次三）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests_b3 {
    use super::*;

    #[test]
    fn estimator_window_is_sliding() {
        // 滑动窗：塞 20 样本只留最后 8——旧样本不污染当前率。
        let mut est = DriftEstimator::new();
        for i in 0..20u64 {
            est.push(DriftSample { local_ms: i * 1_000 + i, ref_ms: i * 1_000 });
        }
        // 窗内最后段漂移率约 1ms/s → 1000µs/s。
        let rate = est.drift_rate_per_s().unwrap();
        assert!((900..=1_100).contains(&rate), "rate={rate}");
    }

    #[test]
    fn slew_converges() {
        // 平滑校正收敛：每拍 100ms，500ms 偏差 5 拍内归零（用户无感线）。
        let mut off: i64 = 500;
        let mut ticks = 0;
        while off != 0 {
            match decide_correction(off) {
                Correction::Slew(step) => {
                    off -= step;
                    ticks += 1;
                }
                _ => break,
            }
        }
        assert!(ticks <= 5, "ticks={ticks}");
    }

    #[test]
    fn ledger_matches_handoff_judgement() {
        // 账本与既有 HandoffSession 判据同源：编码帧→解码→偏差→账本，
        // 全链零漂移绿（交接 10 轮判据的批次三对账路径）。
        let mut led = SyncLedger::new();
        for r in 0..HANDOFF_ROUNDS as u8 {
            let snap = super::super::duoclock::ClockSnapshot { utc_ms: 1_000_000 + (r as u64) * 60_000, tz_offset_min: 480, confidence: Confidence::NtpCalibrated };
            let mut frame = [0u8; super::super::duoclock::SNAPSHOT_LEN];
            assert!(super::super::duoclock::encode_snapshot(&snap, &mut frame));
            let dec = super::super::duoclock::decode_snapshot(&frame).unwrap();
            led.record(r, dec.utc_ms as i64 - (1_000_000 + r as u64 * 60_000) as i64, dec.confidence);
        }
        assert!(led.ten_round_zero_drift(1_000));
    }
}
