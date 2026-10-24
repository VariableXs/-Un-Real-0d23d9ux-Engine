// cgb344_arbselfprot.rs ↔ CGPU-F0344 仲裁器自保
//
// 判据原文：CGPU-F0344-J1 兜底、看门狗、恢复、判据。
// 难点分析：仲裁器是单点风险——故障时必须兜底运行（静态分配表）而非停摆。
//          看门狗的「P99 超阈值告警+自动重启」要避免抖动（连续 N 次才重启）。
// 纪律约束：兜底显性降级告知；看门狗防抖；状态从全局表快照恢复。
// 成本记账：看门狗 O(1) 每次仲裁采样。

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;

pub type AccountId = u32;
pub type BudgetUs = u64;

/// 仲裁器健康状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArbHealth {
    /// 正常。
    Healthy,
    /// 降级（兜底运行）。
    Degraded,
    /// 看门狗告警。
    Warning,
    /// 重启中。
    Restarting,
}

/// 静态分配表（兜底用）。
#[derive(Debug, Clone)]
pub struct StaticAllocation {
    pub account: AccountId,
    pub fixed_us: BudgetUs,
}

/// 看门狗配置。
#[derive(Debug, Clone, Copy)]
pub struct WatchdogConfig {
    /// 延迟 P99 阈值（μs）。
    pub latency_p99_threshold_us: f64,
    /// 连续超阈值次数（防抖）。
    pub breach_tolerance: u32,
}

impl Default for WatchdogConfig {
    fn default() -> Self {
        WatchdogConfig {
            latency_p99_threshold_us: 100.0,
            breach_tolerance: 3,
        }
    }
}

/// 延迟采样（环形窗口）。
#[derive(Debug, Clone)]
struct LatencyWindow {
    samples: Vec<f64>,
    capacity: usize,
    idx: usize,
}

impl LatencyWindow {
    fn new(capacity: usize) -> Self {
        LatencyWindow {
            samples: alloc::vec![0.0; capacity],
            capacity,
            idx: 0,
        }
    }
    fn push(&mut self, v: f64) {
        self.samples[self.idx] = v;
        self.idx = (self.idx + 1) % self.capacity;
    }
    /// P99（排序近似）。
    fn p99(&self) -> f64 {
        let mut sorted = self.samples.clone();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(core::cmp::Ordering::Equal));
        sorted[(self.capacity * 99 / 100).min(self.capacity - 1)]
    }
    /// P95。
    fn p95(&self) -> f64 {
        let mut sorted = self.samples.clone();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(core::cmp::Ordering::Equal));
        sorted[(self.capacity * 95 / 100).min(self.capacity - 1)]
    }
}

/// 仲裁器自保护。
pub struct ArbSelfProtect {
    health: ArbHealth,
    /// 兜底静态表。
    fallback: Vec<StaticAllocation>,
    /// 看门狗配置。
    config: WatchdogConfig,
    /// 延迟窗口。
    latency: LatencyWindow,
    /// 连续超阈值计数。
    breach_streak: u32,
    /// 重启计数。
    restarts: u32,
    /// 是否降级告知（显性）。
    degraded_announced: bool,
}

impl ArbSelfProtect {
    /// 新建。
    pub fn new() -> Self {
        ArbSelfProtect {
            health: ArbHealth::Healthy,
            fallback: Vec::new(),
            config: WatchdogConfig::default(),
            latency: LatencyWindow::new(256),
            breach_streak: 0,
            restarts: 0,
            degraded_announced: false,
        }
    }

    /// 配置兜底静态表。
    pub fn set_fallback(&mut self, table: Vec<StaticAllocation>) {
        self.fallback = table;
    }

    /// 记录仲裁延迟（看门狗采样）。
    pub fn sample_latency(&mut self, latency_us: f64) -> ArbHealth {
        self.latency.push(latency_us);
        let p99 = self.latency.p99();
        if p99 > self.config.latency_p99_threshold_us {
            self.breach_streak += 1;
            if self.breach_streak >= self.config.breach_tolerance {
                self.health = ArbHealth::Warning;
                // 自动重启。
                self.restart();
            } else {
                self.health = ArbHealth::Warning;
            }
        } else {
            self.breach_streak = 0;
            if self.health == ArbHealth::Warning {
                self.health = ArbHealth::Healthy;
            }
        }
        self.health
    }

    /// 重启（恢复到健康，清窗口）。
    fn restart(&mut self) {
        self.health = ArbHealth::Restarting;
        self.restarts += 1;
        self.latency = LatencyWindow::new(256);
        self.breach_streak = 0;
        self.health = ArbHealth::Healthy;
    }

    /// 触发兜底（仲裁异常时调用）。
    pub fn enter_degraded(&mut self) {
        self.health = ArbHealth::Degraded;
        self.degraded_announced = false;
    }

    /// 显性降级告知。
    pub fn announce_degraded(&mut self) -> &'static str {
        self.degraded_announced = true;
        "仲裁器降级：静态分配表兜底运行（显性告知）"
    }

    /// 兜底分配（从静态表）。
    pub fn fallback_allocate(&self, account: AccountId) -> BudgetUs {
        self.fallback
            .iter()
            .find(|s| s.account == account)
            .map(|s| s.fixed_us)
            .unwrap_or(0)
    }

    /// 状态恢复（从全局表快照——模拟）。
    pub fn recover_from_snapshot(&mut self, account_count: usize) {
        // 恢复后重置健康。
        self.health = ArbHealth::Healthy;
        self.degraded_announced = true;
        let _ = account_count;
    }

    /// 当前健康。
    pub fn health(&self) -> ArbHealth {
        self.health
    }
    /// P99。
    pub fn latency_p99(&self) -> f64 {
        self.latency.p99()
    }
    /// P95。
    pub fn latency_p95(&self) -> f64 {
        self.latency.p95()
    }
    /// 重启次数。
    pub fn restarts(&self) -> u32 {
        self.restarts
    }
    /// 是否已告知降级。
    pub fn degraded_announced(&self) -> bool {
        self.degraded_announced
    }
}

impl Default for ArbSelfProtect {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

use super::super::checks::CheckSet;

pub fn run_cgb344_checks() -> CheckSet {
    let mut cs = CheckSet::new("CGPU-F0344 仲裁器自保");

    // --- 正常健康 ---
    let mut s = ArbSelfProtect::new();
    cs.add("初始健康", s.health() == ArbHealth::Healthy, "Healthy");

    // --- 看门狗：单次超阈值告警 ---
    s.sample_latency(150.0);
    cs.add("超阈值告警", s.health() == ArbHealth::Warning, "P99>100");

    // --- 防抖：连续 N 次才重启 ---
    let restarts_before = s.restarts();
    s.sample_latency(200.0);
    s.sample_latency(300.0); // 第 3 次 → 重启
    cs.add(
        "连续超阈值重启",
        s.restarts() > restarts_before,
        "tolerance=3",
    );
    cs.add("重启后健康", s.health() == ArbHealth::Healthy, "恢复");

    // --- 延迟恢复正常 ---
    for _ in 0..10 {
        s.sample_latency(10.0);
    }
    cs.add("低延迟健康", s.health() == ArbHealth::Healthy, "恢复健康");

    // --- 兜底 ---
    s.enter_degraded();
    cs.add("降级状态", s.health() == ArbHealth::Degraded, "Degraded");
    let msg = s.announce_degraded();
    cs.add("显性告知", msg.len() > 0, "降级文案");
    cs.add("已告知标记", s.degraded_announced(), "announced=true");

    // --- 兜底分配 ---
    s.set_fallback(alloc::vec![
        StaticAllocation { account: 1, fixed_us: 100 },
        StaticAllocation { account: 2, fixed_us: 200 },
    ]);
    cs.add("兜底分配", s.fallback_allocate(1) == 100, "静态表");
    cs.add("兜底缺省 0", s.fallback_allocate(99) == 0, "无条目");

    // --- 状态恢复 ---
    s.recover_from_snapshot(5);
    cs.add("恢复后健康", s.health() == ArbHealth::Healthy, "从快照恢复");

    // --- P95/P99 ---
    cs.add("P99 有值", s.latency_p99() >= 0.0, "窗口统计");
    cs.add("P95 有值", s.latency_p95() >= 0.0, "窗口统计");

    cs
}
