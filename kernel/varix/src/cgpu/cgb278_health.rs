#![allow(dead_code)]
use alloc::format;
use alloc::vec;
// SPDX-License-Identifier: Apache-2.0
// Varix Kernel — CGPU · cgb278_health
// CGPU-F0278 · 调度器健康指标（B08 组 · 四组指标 + 九组红绿灯 + 阈值化）
//
// 锚点：CGPU-F0278
// 判据原文：队列深度与利用率（每队列水位/线程忙碌率）；
//          抢占频率与代价（抢占引发的重复执行耗时）；
//          检测器健康汇聚（B06 检测耗时/误报率/重启计数+B07 积压/丢弃计数）；
//          九组健康红绿灯（绿/黄/红阈值化——一眼看全域）。
//
// 难点分析：
//   1. 红绿灯的阈值设计是「体验守护」的关键——太敏感会频繁误报，
//      太迟钝会漏掉真问题。三档阈值（绿/黄/红）需要可调。
//   2. 九组覆盖全域：不能只看 CPU，还要看队列/内存/死锁/持久化。
//   3. 「一眼看全域」要求九组灯的语义一致：绿=正常、黄=注意、红=异常。
//
// 纪律约束：
//   - no_std + alloc 三件套
//   - match 臂字符串尾必须加逗号


extern crate alloc;

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::checks::CheckSet;

// ============================================================================
// 一、红绿灯状态
// ============================================================================

/// 灯色。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LightColor {
    /// 正常。
    Green,
    /// 注意。
    Yellow,
    /// 异常。
    Red,
}

impl LightColor {
    /// 灯色符号。
    pub fn symbol(&self) -> &'static str {
        match self {
            LightColor::Green => "●",
            LightColor::Yellow => "◐",
            LightColor::Red => "○",
        }
    }
}

/// 阈值配置（三档）。
#[derive(Clone, Copy, Debug)]
pub struct Threshold {
    /// 黄灯阈值。
    pub yellow: f64,
    /// 红灯阈值。
    pub red: f64,
    /// 是否反转（低值为异常）。
    pub inverted: bool,
}

impl Threshold {
    /// 正常阈值（低值好）。
    pub fn normal(yellow: f64, red: f64) -> Self {
        Self { yellow, red, inverted: false }
    }

    /// 反转阈值（高值好）。
    pub fn inverted(yellow: f64, red: f64) -> Self {
        Self { yellow, red, inverted: true }
    }

    /// 判灯。
    pub fn evaluate(&self, value: f64) -> LightColor {
        if self.inverted {
            if value < self.red {
                LightColor::Red
            } else if value < self.yellow {
                LightColor::Yellow
            } else {
                LightColor::Green
            }
        } else {
            if value >= self.red {
                LightColor::Red
            } else if value >= self.yellow {
                LightColor::Yellow
            } else {
                LightColor::Green
            }
        }
    }
}

// ============================================================================
// 二、九组健康指标
// ============================================================================

/// 健康组 ID。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum HealthGroup {
    /// 队列深度。
    QueueDepth,
    /// 线程利用率。
    ThreadUtil,
    /// 抢占频率。
    PreemptRate,
    /// 抢占代价。
    PreemptCost,
    /// 死锁检测耗时。
    DeadlockDetect,
    /// 死锁误报率。
    DeadlockFalsePos,
    /// 持久化积压。
    PersistBacklog,
    /// 持久化丢弃。
    PersistDrop,
    /// 帧时间。
    FrameTime,
}

impl HealthGroup {
    /// 组名。
    pub fn name(&self) -> &'static str {
        match self {
            HealthGroup::QueueDepth => "队列深度",
            HealthGroup::ThreadUtil => "线程利用率",
            HealthGroup::PreemptRate => "抢占频率",
            HealthGroup::PreemptCost => "抢占代价",
            HealthGroup::DeadlockDetect => "死锁检测耗时",
            HealthGroup::DeadlockFalsePos => "死锁误报率",
            HealthGroup::PersistBacklog => "持久化积压",
            HealthGroup::PersistDrop => "持久化丢弃",
            HealthGroup::FrameTime => "帧时间",
        }
    }

    /// 全部九组。
    pub fn all() -> Vec<HealthGroup> {
        vec![
            HealthGroup::QueueDepth,
            HealthGroup::ThreadUtil,
            HealthGroup::PreemptRate,
            HealthGroup::PreemptCost,
            HealthGroup::DeadlockDetect,
            HealthGroup::DeadlockFalsePos,
            HealthGroup::PersistBacklog,
            HealthGroup::PersistDrop,
            HealthGroup::FrameTime,
        ]
    }

    /// 默认阈值。
    pub fn default_threshold(&self) -> Threshold {
        match self {
            HealthGroup::QueueDepth => Threshold::normal(50.0, 90.0),
            HealthGroup::ThreadUtil => Threshold::normal(70.0, 95.0),
            HealthGroup::PreemptRate => Threshold::normal(0.1, 0.3),
            HealthGroup::PreemptCost => Threshold::normal(500.0, 2000.0),
            HealthGroup::DeadlockDetect => Threshold::normal(100.0, 500.0),
            HealthGroup::DeadlockFalsePos => Threshold::normal(0.05, 0.15),
            HealthGroup::PersistBacklog => Threshold::normal(100.0, 500.0),
            HealthGroup::PersistDrop => Threshold::normal(1.0, 10.0),
            HealthGroup::FrameTime => Threshold::normal(11_000.0, 12_500.0),
        }
    }
}

/// 健康状态。
#[derive(Clone, Debug)]
pub struct HealthStatus {
    /// 组灯色。
    pub lights: Vec<(HealthGroup, LightColor)>,
    /// 组当前值。
    pub values: Vec<(HealthGroup, f64)>,
}

impl HealthStatus {
    /// 是否全域正常。
    pub fn all_green(&self) -> bool {
        self.lights.iter().all(|&(_, c)| c == LightColor::Green)
    }

    /// 红灯组列表。
    pub fn red_groups(&self) -> Vec<HealthGroup> {
        self.lights
            .iter()
            .filter(|&&(_, c)| c == LightColor::Red)
            .map(|&(g, _)| g)
            .collect()
    }

    /// 人话摘要。
    pub fn summary(&self) -> String {
        let green = self.lights.iter().filter(|&&(_, c)| c == LightColor::Green).count();
        let yellow = self.lights.iter().filter(|&&(_, c)| c == LightColor::Yellow).count();
        let red = self.lights.iter().filter(|&&(_, c)| c == LightColor::Red).count();
        alloc::format!("绿{} 黄{} 红{}", green, yellow, red)
    }
}

/// 健康监控器。
pub struct HealthMonitor {
    /// 阈值表。
    thresholds: Vec<(HealthGroup, Threshold)>,
}

impl HealthMonitor {
    /// 构造（默认阈值）。
    pub fn new() -> Self {
        Self {
            thresholds: HealthGroup::all()
                .iter()
                .map(|&g| (g, g.default_threshold()))
                .collect(),
        }
    }

    /// 设置自定义阈值。
    pub fn set_threshold(&mut self, group: HealthGroup, threshold: Threshold) {
        if let Some(entry) = self.thresholds.iter_mut().find(|(g, _)| *g == group) {
            entry.1 = threshold;
        }
    }

    /// 评估健康状态。
    pub fn evaluate(&self, values: &[(HealthGroup, f64)]) -> HealthStatus {
        let mut lights = Vec::new();
        let mut values_out = Vec::new();
        for &(g, v) in values {
            let threshold = self
                .thresholds
                .iter()
                .find(|(tg, _)| *tg == g)
                .map(|(_, t)| *t)
                .unwrap_or_else(|| g.default_threshold());
            lights.push((g, threshold.evaluate(v)));
            values_out.push((g, v));
        }
        HealthStatus {
            lights,
            values: values_out,
        }
    }

    /// 一键全绿检查。
    pub fn is_healthy(&self, values: &[(HealthGroup, f64)]) -> bool {
        self.evaluate(values).all_green()
    }
}

impl Default for HealthMonitor {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// 三、判据：F0278 三条（四组指标、红绿灯、阈值化）
// ============================================================================

/// F0278 判据集。
pub fn run_f0278_checks() -> CheckSet {
    let mut cs = CheckSet::new("CGPU-F0278");

    // --- 判据 1：九组健康指标齐备 ---
    let all_groups = HealthGroup::all();
    cs.add(
        "九组健康指标",
        all_groups.len() == 9,
        alloc::format!("{:?}", all_groups.iter().map(|g| g.name()).collect::<Vec<_>>()).leak(),
    );

    // --- 判据 2：红绿灯阈值化 ---
    let monitor = HealthMonitor::new();
    let healthy_values: Vec<(HealthGroup, f64)> = vec![
        (HealthGroup::QueueDepth, 10.0),
        (HealthGroup::ThreadUtil, 50.0),
        (HealthGroup::PreemptRate, 0.02),
        (HealthGroup::PreemptCost, 100.0),
        (HealthGroup::DeadlockDetect, 50.0),
        (HealthGroup::DeadlockFalsePos, 0.01),
        (HealthGroup::PersistBacklog, 20.0),
        (HealthGroup::PersistDrop, 0.0),
        (HealthGroup::FrameTime, 10_000.0),
    ];
    let healthy = monitor.evaluate(&healthy_values);
    let all_green_ok = healthy.all_green();

    let warning_values: Vec<(HealthGroup, f64)> = vec![
        (HealthGroup::QueueDepth, 60.0),
        (HealthGroup::ThreadUtil, 75.0),
    ];
    let warning = monitor.evaluate(&warning_values);
    let has_yellow = warning.lights.iter().any(|&(_, c)| c == LightColor::Yellow);

    let critical_values: Vec<(HealthGroup, f64)> = vec![
        (HealthGroup::QueueDepth, 95.0),
        (HealthGroup::ThreadUtil, 98.0),
    ];
    let critical = monitor.evaluate(&critical_values);
    let has_red = critical.lights.iter().any(|&(_, c)| c == LightColor::Red);

    cs.add(
        "红绿灯三档阈值化",
        all_green_ok && has_yellow && has_red,
        "全绿/黄灯/红灯三档正确",
    );

    // --- 判据 3：一眼看全域 ---
    let summary = healthy.summary();
    cs.add(
        "全域摘要",
        summary.contains("绿9") && summary.contains("红0"),
        alloc::format!("{}", summary).leak(),
    );

    // --- 附加：红灯组列表 ---
    let red_list = critical.red_groups();
    cs.add(
        "红灯组列表",
        !red_list.is_empty(),
        alloc::format!("{:?}", red_list.iter().map(|g| g.name()).collect::<Vec<_>>()).leak(),
    );

    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f0278_all_pass() {
        let cs = run_f0278_checks();
        assert!(cs.all_green(), "F0278 判据未全绿: {}", cs.red_items_text());
    }

    #[test]
    fn threshold_normal() {
        let t = Threshold::normal(50.0, 90.0);
        assert_eq!(t.evaluate(10.0), LightColor::Green);
        assert_eq!(t.evaluate(60.0), LightColor::Yellow);
        assert_eq!(t.evaluate(95.0), LightColor::Red);
    }

    #[test]
    fn threshold_inverted() {
        let t = Threshold::inverted(50.0, 20.0);
        assert_eq!(t.evaluate(80.0), LightColor::Green);
        assert_eq!(t.evaluate(30.0), LightColor::Yellow);
        assert_eq!(t.evaluate(10.0), LightColor::Red);
    }

    #[test]
    fn nine_groups_all_present() {
        assert_eq!(HealthGroup::all().len(), 9);
    }
}
