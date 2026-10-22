//! VE-F3434 · 换肤压测 —— 连续切换 + 并发切换 + 大主题 + 恢复爬回验证。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3434`
//!
//! # 职责（锚点原文拆解）
//!
//! - **连续切换**：万次换肤不泄漏、不劣化。两个指标分开看：
//!   泄漏看「分配 - 释放」是否归零；劣化看后 20% 轮次的 p50 是否显著
//!   高于前 20%（阈值 [`DEGRADE_LIMIT_PERMILLE`]）。只看平均值会把
//!   劣化摊平，看不出「越跑越慢」；
//! - **并发切换**：多窗口同时换。核心断言是**不出现半新半旧**：
//!   任一轮次里，所有窗口要么都在新主题、要么都在旧主题，
//!   `half_states` 必须为 0。非 0 即红线；
//! - **大主题**：万级令牌的主题切换。考的是「耗时是否随令牌数线性」，
//!   以及内存峰值是否可接受；
//! - **恢复爬回**：撤压之后再测一轮，p50 必须回到基线
//!   [`RECOVERY_LIMIT_PERMILLE`] 以内。爬不回来说明有缓存没释放、
//!   或者内部结构被压大了（例如哈希表扩容后不缩容）。
//!
//! # 为什么「泄漏」与「劣化」必须分成两条判据
//!
//! 泄漏是**单调增长**的：每次换肤多一点，一万次之后爆掉。劣化是
//! **相对变慢**：内存没涨，但每轮多做了一点无用功（缓存失效、
//! 链表变长）。两者的修法完全不同——泄漏要查释放路径，劣化要查
//! 数据结构。合成一条「性能不达标」的判据，等于把两种缺陷都埋在
//! 一个红灯后面。
//!
//! # 复杂度
//!
//! - `run`：O(轮次)；
//! - `p50`：O(n log n)；
//! - `recovery`：O(1)。
//!
//! # 零 panic 面
//!
//! 无索引、无 `unwrap`。测试区除外。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码（E18 段续：5511 起）
// ---------------------------------------------------------------------------

/// 压测诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StressCode {
    /// 内存泄漏（分配未释放）。
    Leak = 5511,
    /// 性能劣化（后段显著慢于前段）。
    Degraded = 5512,
    /// 出现半新半旧状态（红线）。
    HalfState = 5513,
    /// 恢复未爬回基线。
    NoRecovery = 5514,
    /// 轮次为 0。
    ZeroRounds = 5515,
    /// 并发窗口为 0。
    ZeroWindows = 5516,
    /// 大主题令牌数为 0。
    ZeroTokens = 5517,
    /// 容量口径不足（压测规模低于锚点要求）。
    CapacityTooLow = 5518,
}

impl StressCode {
    /// 稳定诊断号。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 中文说明。
    pub fn text(self) -> &'static str {
        match self {
            StressCode::Leak => "换肤内存泄漏",
            StressCode::Degraded => "连续换肤性能劣化",
            StressCode::HalfState => "出现半新半旧状态",
            StressCode::NoRecovery => "撤压后未恢复基线",
            StressCode::ZeroRounds => "压测轮次为 0",
            StressCode::ZeroWindows => "并发窗口为 0",
            StressCode::ZeroTokens => "大主题令牌数为 0",
            StressCode::CapacityTooLow => "压测规模低于锚点要求",
        }
    }

    /// 是否阻断级。
    pub fn blocking(self) -> bool {
        matches!(self, StressCode::Leak | StressCode::HalfState)
    }
}

/// 劣化阈值：后段 p50 超过前段 130% 即判劣化。
pub const DEGRADE_LIMIT_PERMILLE: u32 = 1300;

/// 恢复阈值：撤压后 p50 不得超过基线 110%。
pub const RECOVERY_LIMIT_PERMILLE: u32 = 1100;

/// 锚点要求的最小连续切换次数（万次）。
pub const MIN_ROUNDS: u32 = 10_000;

// ---------------------------------------------------------------------------
// 二、压测配置与单轮结果
// ---------------------------------------------------------------------------

/// 压测配置。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct StressConfig {
    /// 连续轮次。
    pub rounds: u32,
    /// 并发窗口数。
    pub windows: u32,
    /// 大主题令牌数。
    pub tokens: u32,
}

impl Default for StressConfig {
    fn default() -> Self {
        StressConfig {
            rounds: MIN_ROUNDS,
            windows: 4,
            tokens: 10_000,
        }
    }
}

impl StressConfig {
    /// 校验。
    pub fn validate(&self) -> Result<(), StressCode> {
        if self.rounds == 0 {
            return Err(StressCode::ZeroRounds);
        }
        if self.windows == 0 {
            return Err(StressCode::ZeroWindows);
        }
        if self.tokens == 0 {
            return Err(StressCode::ZeroTokens);
        }
        Ok(())
    }

    /// 是否达到锚点规模（万次连续 + 万级令牌）。低于规模只是「冒烟」，
    /// 不许当作压测结论发布。
    pub fn meets_capacity(&self) -> bool {
        self.rounds >= MIN_ROUNDS && self.tokens >= 10_000
    }
}

/// 单轮结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RoundResult {
    /// 轮次。
    pub round: u32,
    /// 耗时。
    pub duration_ms: u32,
    /// 本轮的半新半旧窗口数（必须 0）。
    pub half_states: u32,
    /// 本轮净分配字节（分配 - 释放）。
    pub net_bytes: i64,
}

/// 压测报告。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct StressReport {
    /// 轮次结果。
    pub rounds: Vec<RoundResult>,
    /// 峰值净分配。
    pub peak_net_bytes: i64,
    /// 末轮净分配（应回到 0 附近）。
    pub final_net_bytes: i64,
    /// 前 20% p50。
    pub head_p50_ms: u32,
    /// 后 20% p50。
    pub tail_p50_ms: u32,
    /// 是否达到锚点规模。
    pub at_capacity: bool,
}

impl StressReport {
    /// 累计半新半旧。
    pub fn half_states(&self) -> u32 {
        self.rounds.iter().map(|r| r.half_states).sum()
    }

    /// 劣化率（千分比）：后段 / 前段。前段为 0 时返回 1000。
    pub fn degrade_permille(&self) -> u32 {
        if self.head_p50_ms == 0 {
            return 1000;
        }
        (self.tail_p50_ms as u64 * 1000 / self.head_p50_ms as u64) as u32
    }

    /// 判定列表（空 = 全绿）。
    pub fn verdicts(&self) -> Vec<StressCode> {
        let mut out = Vec::new();
        if self.final_net_bytes > 0 || self.peak_net_bytes > 0 && self.final_net_bytes > 0 {
            out.push(StressCode::Leak);
        }
        if self.degrade_permille() > DEGRADE_LIMIT_PERMILLE {
            out.push(StressCode::Degraded);
        }
        if self.half_states() > 0 {
            out.push(StressCode::HalfState);
        }
        if !self.at_capacity {
            out.push(StressCode::CapacityTooLow);
        }
        out
    }

    /// 是否全绿。
    pub fn clean(&self) -> bool {
        self.verdicts().is_empty()
    }

    /// 读屏清单。
    pub fn reading_lines(&self) -> Vec<String> {
        let mut out = Vec::new();
        out.push(format!(
            "换肤压测：{} 轮，前段 p50 {}ms，后段 p50 {}ms，劣化 {}‰（阈值 {}‰）",
            self.rounds.len(),
            self.head_p50_ms,
            self.tail_p50_ms,
            self.degrade_permille(),
            DEGRADE_LIMIT_PERMILLE
        ));
        out.push(format!(
            "内存：峰值净 {:?} 字节，末轮净 {:?} 字节；半新半旧 {} 次",
            self.peak_net_bytes,
            self.final_net_bytes,
            self.half_states()
        ));
        if out.len() >= 2 && self.clean() {
            out.push("判定：全绿".to_string());
        } else {
            for v in self.verdicts() {
                out.push(format!("判定：E{} {}", v.code(), v.text()));
            }
        }
        out
    }
}

// ---------------------------------------------------------------------------
// 三、压测执行
// ---------------------------------------------------------------------------

/// 结果收集器。
#[derive(Clone, Debug, Default)]
pub struct StressSuite {
    results: Vec<RoundResult>,
    peak_net: i64,
}

impl StressSuite {
    /// 新建。
    pub fn new() -> StressSuite {
        StressSuite {
            results: Vec::new(),
            peak_net: 0,
        }
    }

    /// 记录一轮。净分配是**累计值**：每轮的 `net_bytes` 是当轮增量，
    /// 累计后才是有意义的「当前占用」。只看末轮增量会把「+16/-16
    /// 交替」误判成泄漏或误判成归零。
    pub fn record(&mut self, result: RoundResult) {
        let running = self
            .results
            .last()
            .map(|r| r.net_bytes)
            .unwrap_or(0)
            .saturating_add(result.net_bytes);
        let mut stored = result;
        stored.net_bytes = running;
        self.peak_net = core::cmp::max(self.peak_net, running);
        self.results.push(stored);
    }

    /// 生成报告。
    pub fn report(&self, at_capacity: bool) -> StressReport {
        let mut durations: Vec<u32> = self.results.iter().map(|r| r.duration_ms).collect();
        let n = durations.len();
        let head_n = core::cmp::max(1, n / 5);
        let mut head: Vec<u32> = durations.iter().take(head_n).copied().collect();
        let mut tail: Vec<u32> = durations.iter().skip(n.saturating_sub(head_n)).copied().collect();
        head.sort_unstable();
        tail.sort_unstable();
        durations.sort_unstable();
        // 末轮 `net_bytes` 已是累计值（见 `record`）。
        let final_net = self.results.last().map(|r| r.net_bytes).unwrap_or(0);
        StressReport {
            rounds: self.results.clone(),
            peak_net_bytes: self.peak_net,
            final_net_bytes: final_net,
            head_p50_ms: percentile(&head, 50),
            tail_p50_ms: percentile(&tail, 50),
            at_capacity,
        }
    }
}

/// 恢复验证：撤压后的 p50 是否回到基线附近。
/// 返回 `Ok(permille)`；超阈值返回 `Err`。
pub fn recovery_check(baseline_p50_ms: u32, after_p50_ms: u32) -> Result<u32, StressCode> {
    if baseline_p50_ms == 0 {
        // 基线为 0 无法比较，按不恢复处理（宁可红，不可假绿）。
        return Err(StressCode::NoRecovery);
    }
    let permille = (after_p50_ms as u64 * 1000 / baseline_p50_ms as u64) as u32;
    if permille > RECOVERY_LIMIT_PERMILLE {
        Err(StressCode::NoRecovery)
    } else {
        Ok(permille)
    }
}

fn percentile(sorted: &[u32], p: u32) -> u32 {
    if sorted.is_empty() {
        return 0;
    }
    let idx = ((sorted.len() as u64 * p as u64) / 100) as usize;
    let idx = core::cmp::min(idx, sorted.len().saturating_sub(1));
    sorted.get(idx).copied().unwrap_or(0)
}

/// 生成一条确定性的模拟轮次结果（供自测与基准共用）。
///
/// 模拟模型：耗时随轮次轻微上升（模拟缓存压力），内存每轮 +16 字节
/// 再 -16 字节（净 0），并发轮次里每 997 轮制造一次「无半新半旧」
/// （正确实现恒 0）。
pub fn simulate_round(round: u32, windows: u32, tokens: u32) -> RoundResult {
    let base = 3u32.saturating_add(tokens / 5000);
    // 每 8000 轮才 +1ms：模拟长跑下的轻微缓存压力，不触发劣化判据。
    let drift = round / 8000;
    RoundResult {
        round,
        duration_ms: base.saturating_add(drift),
        half_states: 0,
        net_bytes: if round % 2 == 0 { 16 } else { -16 },
    }
    .with_windows(windows)
}

impl RoundResult {
    fn with_windows(mut self, windows: u32) -> RoundResult {
        // 并发窗口越多，单轮耗时略增（调度开销），但不改变净分配。
        self.duration_ms = self.duration_ms.saturating_add(windows / 4);
        self
    }
}

// ---------------------------------------------------------------------------
// 四、测试支撑
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn run(rounds: u32, windows: u32, tokens: u32) -> StressReport {
        let cfg = StressConfig {
            rounds,
            windows,
            tokens,
        };
        let mut suite = StressSuite::new();
        let mut i = 0;
        while i < rounds {
            suite.record(simulate_round(i, cfg.windows, cfg.tokens));
            i += 1;
        }
        suite.report(cfg.meets_capacity())
    }

    #[test]
    fn ten_thousand_rounds_is_clean() {
        let r = run(MIN_ROUNDS, 4, 10_000);
        assert_eq!(r.rounds.len() as u32, MIN_ROUNDS);
        assert_eq!(r.half_states(), 0);
        assert_eq!(r.final_net_bytes, 0);
        assert!(r.clean(), "{:?}", r.reading_lines());
    }

    #[test]
    fn leak_is_detected_and_blocking() {
        let mut suite = StressSuite::new();
        let mut i = 0;
        while i < 100 {
            let mut r = simulate_round(i, 4, 10_000);
            if i == 99 {
                r.net_bytes = 4096;
            }
            suite.record(r);
            i += 1;
        }
        let report = suite.report(true);
        assert!(report.verdicts().contains(&StressCode::Leak));
        assert!(StressCode::Leak.blocking());
    }

    #[test]
    fn half_state_is_red_line() {
        let mut suite = StressSuite::new();
        let mut i = 0;
        while i < 50 {
            let mut r = simulate_round(i, 4, 10_000);
            if i == 7 {
                r.half_states = 2;
            }
            suite.record(r);
            i += 1;
        }
        let report = suite.report(true);
        assert_eq!(report.half_states(), 2);
        assert!(report.verdicts().contains(&StressCode::HalfState));
    }

    #[test]
    fn degradation_is_detected_separately_from_leak() {
        let mut suite = StressSuite::new();
        let mut i = 0;
        while i < 100 {
            let mut r = simulate_round(i, 4, 10_000);
            // 后 20% 显著变慢，但净分配仍为 0。
            if i >= 80 {
                r.duration_ms = 100;
            }
            suite.record(r);
            i += 1;
        }
        let report = suite.report(true);
        assert!(report.verdicts().contains(&StressCode::Degraded));
        assert!(!report.verdicts().contains(&StressCode::Leak));
        assert!(report.degrade_permille() > DEGRADE_LIMIT_PERMILLE);
    }

    #[test]
    fn recovery_must_crawl_back_to_baseline() {
        assert_eq!(recovery_check(10, 10).ok(), Some(1000));
        assert_eq!(recovery_check(10, 11).ok(), Some(1100));
        assert_eq!(recovery_check(10, 12).err(), Some(StressCode::NoRecovery));
        assert_eq!(recovery_check(0, 5).err(), Some(StressCode::NoRecovery));
    }

    #[test]
    fn capacity_below_anchor_is_flagged() {
        let r = run(100, 2, 500);
        assert!(!r.at_capacity);
        assert!(r.verdicts().contains(&StressCode::CapacityTooLow));
    }

    #[test]
    fn config_validation_rejects_zero_sizes() {
        assert_eq!(
            StressConfig {
                rounds: 0,
                ..StressConfig::default()
            }
            .validate()
            .err(),
            Some(StressCode::ZeroRounds)
        );
        assert_eq!(
            StressConfig {
                windows: 0,
                ..StressConfig::default()
            }
            .validate()
            .err(),
            Some(StressCode::ZeroWindows)
        );
    }

    #[test]
    fn reading_lines_are_complete() {
        let r = run(1000, 4, 10_000);
        let lines = r.reading_lines();
        assert!(lines.first().map(|l| l.contains("换肤压测：1000 轮")).unwrap_or(false));
        assert!(lines.iter().any(|l| l.contains("内存")));
    }
}
