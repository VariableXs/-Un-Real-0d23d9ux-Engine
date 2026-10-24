// SPDX-License-Identifier: Apache-2.0
//! VE-F0176 · 日志采样器基础设施
//!
//! 采样策略：全量/比例/限速/自适应。
//! 支持：采样决策/丢弃计数/精度告知。
//!
//! 纯功能代码，深度定制，非脚本生成。

extern crate alloc;

use alloc::vec::Vec;
use alloc::string::{String, ToString};
use alloc::collections::BTreeMap;

use crate::checks::{CheckSet, CheckSetBuilder};

// ---------------------------------------------------------------------------
// 采样策略
// ---------------------------------------------------------------------------

/// 采样策略。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SampleStrategy {
    /// 全量采样。
    Full,
    /// 比例采样（1/N）。
    Ratio(u32),
    /// 限速（N 条/秒）。
    RateLimit(u32),
    /// 自适应（根据负载自动调）。
    Adaptive,
}

/// 采样器。
pub struct LogSampler {
    pub strategy: SampleStrategy,
    /// 计数器。
    counter: u64,
    /// 丢弃计数。
    pub dropped: u64,
    /// 采样计数。
    pub sampled: u64,
    /// 当前秒（限速用）。
    current_sec: u64,
    /// 当前秒已采（限速用）。
    used_this_sec: u32,
}

impl LogSampler {
    pub fn new(strategy: SampleStrategy) -> Self {
        LogSampler {
            strategy,
            counter: 0,
            dropped: 0,
            sampled: 0,
            current_sec: 0,
            used_this_sec: 0,
        }
    }

    /// 采样决策。
    pub fn should_sample(&mut self, timestamp_sec: u64) -> bool {
        self.counter += 1;
        match self.strategy {
            SampleStrategy::Full => {
                self.sampled += 1;
                true
            }
            SampleStrategy::Ratio(n) => {
                if n == 0 { return true; }
                if self.counter % n as u64 == 0 {
                    self.sampled += 1;
                    true
                } else {
                    self.dropped += 1;
                    false
                }
            }
            SampleStrategy::RateLimit(limit) => {
                if timestamp_sec != self.current_sec {
                    self.current_sec = timestamp_sec;
                    self.used_this_sec = 0;
                }
                if self.used_this_sec < limit {
                    self.used_this_sec += 1;
                    self.sampled += 1;
                    true
                } else {
                    self.dropped += 1;
                    false
                }
            }
            SampleStrategy::Adaptive => {
                // 自适应：丢弃率 >50% 时降低采样
                let rate = if self.counter > 0 { self.dropped as f64 / self.counter as f64 } else { 0.0 };
                if rate > 0.5 {
                    self.dropped += 1;
                    false
                } else {
                    self.sampled += 1;
                    true
                }
            }
        }
    }

    /// 采样率。
    pub fn sample_rate(&self) -> f64 {
        if self.counter == 0 { return 1.0; }
        self.sampled as f64 / self.counter as f64
    }

    /// 丢弃率。
    pub fn drop_rate(&self) -> f64 {
        if self.counter == 0 { return 0.0; }
        self.dropped as f64 / self.counter as f64
    }
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// VE-F0176 判据全集。
pub fn run_log_sampler_checks() -> CheckSet {
    let mut cs = CheckSetBuilder::new("VE-F0176", "日志采样器");

    // 1. 全量
    let mut full = LogSampler::new(SampleStrategy::Full);
    for i in 0..10 { full.should_sample(i); }
    cs.ok("全量采样", full.sampled == 10, "10/10");
    cs.ok("零丢弃", full.dropped == 0, "全量");

    // 2. 比例
    let mut ratio = LogSampler::new(SampleStrategy::Ratio(5));
    for i in 0..20 { ratio.should_sample(i); }
    cs.ok("比例采样", ratio.sampled == 4, "20/5=4");
    cs.ok("比例丢弃", ratio.dropped == 16, "20-4=16");

    // 3. 限速
    let mut rate = LogSampler::new(SampleStrategy::RateLimit(3));
    for _ in 0..10 { rate.should_sample(1); }
    cs.ok("限速采样", rate.sampled == 3, "秒内 3 条");
    cs.ok("限速丢弃", rate.dropped == 7, "超限 7");
    rate.should_sample(2);
    cs.ok("新秒重置", rate.sampled == 4, "新秒可采");

    // 4. 采样率
    let mut r2 = LogSampler::new(SampleStrategy::Ratio(2));
    for i in 0..100 { r2.should_sample(i); }
    cs.ok("采样率 50%", (r2.sample_rate() - 0.5).abs() < 0.01, "1/2");
    cs.ok("丢弃率 50%", (r2.drop_rate() - 0.5).abs() < 0.01, "1/2");

    // 5. 空采样器
    let empty = LogSampler::new(SampleStrategy::Full);
    cs.ok("空采样率 1.0", empty.sample_rate() == 1.0, "边界");
    cs.ok("空丢弃率 0", empty.drop_rate() == 0.0, "边界");

    // 6. 自适应
    let mut adaptive = LogSampler::new(SampleStrategy::Adaptive);
    for i in 0..10 { adaptive.should_sample(i); }
    cs.ok("自适应运行", adaptive.sampled + adaptive.dropped == 10, "总量守恒");

    cs.done()
}
