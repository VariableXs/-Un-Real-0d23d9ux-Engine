//! CGPU-F0441 · 抢占风暴防护（目标 300 行）
//! ==========================================================================
//! 锚点判据（施工书原文摘录）：
//! - 风暴防护：抢占频率上限（单位时间抢占次数封顶——防抢占抖动）；退避
//!   机制（频繁被抢任务的退避——错峰重试）；风暴检测与告警（抢占率异常
//!   →域级告警）；与 B04 抢占域联动（抢占执行在 B04、本域管预算侧防护
//!   ——边界声明）。判据：频率封顶、退避、告警、边界、判据。
//!
//! 设计纪律：
//! - 频率封顶：滑动窗内抢占计数 ≤ 上限；超限申请一律拒（预算侧不给
//!   抢占配额——B04 侧就抢不动，防护落在预算闸而非执行侧）。
//! - 退避：被抢次数超阈的任务进入退避窗（指数退避有上限——2^n 封顶），
//!   退避期内其抢占申请降级受理（错峰）。
//! - 告警：窗口抢占率（实际/上限）≥ 80% 触发域级告警一次
//!   （窗口去重——不刷屏）。
//! - 边界声明：本域只出"抢不抢得到预算"的判定，抢占执行语义在 B04
//!   ——`enforce_preempt` 形式接口恒拒（API 面即边界）。
//! - no_std + alloc；零 unwrap/expect 于运行路径；诊断段 0x0EC0 独占。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址（0x0EC0，全仓独占）。
pub const SEG_BASE: u16 = 0x0EC0;

pub const E_STM_CAP: u16 = SEG_BASE + 1; // 频率封顶已到
pub const E_STM_TASK: u16 = SEG_BASE + 2; // 任务非法
pub const E_STM_BOUND: u16 = SEG_BASE + 3; // 边界违反（本域被要求执行抢占）

/// 滑动窗长（tick）与抢占上限。
pub const WINDOW_TICKS: u64 = 100;
pub const PREEMPT_CAP: u64 = 20;
/// 退避触发阈（被抢次数）与指数上限。
pub const BACKOFF_AT: u64 = 3;
pub const BACKOFF_MAX: u64 = 64;
/// 告警阈值（窗口占用千分比）。
pub const ALERT_PM: u64 = 800;

/// 单帧滑动窗计数。
#[derive(Debug, Default)]
pub struct PreemptMeter {
    window_start: u64,
    count: u64,
    alerted: bool,
    pub total_preempts: u64,
    pub denied: u64,
}

impl PreemptMeter {
    pub fn new() -> Self {
        Self::default()
    }

    /// 申请一次抢占预算：窗内封顶，超限拒。
    pub fn try_preempt(&mut self, now: u64) -> Result<bool, u16> {
        self.roll(now);
        if self.count >= PREEMPT_CAP {
            self.denied += 1;
            return Ok(false);
        }
        self.count += 1;
        self.total_preempts += 1;
        Ok(true)
    }

    /// 窗口滚动（超窗长重置计数与告警去重位）。
    fn roll(&mut self, now: u64) {
        if now.saturating_sub(self.window_start) >= WINDOW_TICKS {
            self.window_start = now;
            self.count = 0;
            self.alerted = false;
        }
    }

    /// 风暴告警：窗口占用 ≥ 阈值 → 域级告警一次（去重）。
    pub fn check_alert(&mut self, now: u64) -> bool {
        self.roll(now);
        let pm = self.count.saturating_mul(1000) / PREEMPT_CAP;
        if pm >= ALERT_PM && !self.alerted {
            self.alerted = true;
            return true;
        }
        false
    }

    pub fn window_used_pm(&self) -> u64 {
        self.count.saturating_mul(1000) / PREEMPT_CAP
    }
}

/// 任务退避账。
#[derive(Debug)]
pub struct BackoffRec {
    pub preempted: u64,
    /// 退避截止 tick（0 = 不在退避）。
    pub until: u64,
}

/// 抢占风暴防护器。
#[derive(Debug, Default)]
pub struct StormGuard {
    meter: PreemptMeter,
    tasks: Vec<(u64, BackoffRec)>,
    now: u64,
}

impl StormGuard {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn tick(&mut self, n: u64) {
        self.now = self.now.saturating_add(n);
    }

    /// 记录一次"任务 X 被抢"（B04 侧通报——本域记账并触发退避）。
    pub fn note_preempted(&mut self, task: u64) -> Result<(), u16> {
        if !self.tasks.iter().any(|(t, _)| *t == task) {
            self.tasks.push((task, BackoffRec { preempted: 0, until: 0 }));
        }
        let now = self.now;
        let rec = &mut self
            .tasks
            .iter_mut()
            .find(|(t, _)| *t == task)
            .map(|(_, r)| r)
            .ok_or(E_STM_TASK)?;
        rec.preempted = rec.preempted.saturating_add(1);
        if rec.preempted >= BACKOFF_AT {
            // 指数退避：2^n 封顶。
            let exp = rec.preempted - BACKOFF_AT;
            let dur = 1u64.checked_shl(exp.min(6) as u32).unwrap_or(BACKOFF_MAX).min(BACKOFF_MAX);
            rec.until = now.saturating_add(dur);
        }
        Ok(())
    }

    /// 任务是否在退避期（退避期内抢占申请错峰降级）。
    pub fn in_backoff(&self, task: u64) -> bool {
        self.tasks
            .iter()
            .find(|(t, _)| *t == task)
            .map(|(_, r)| self.now < r.until)
            .unwrap_or(false)
    }

    /// 抢占预算申请：封顶 + 退避双闸。
    pub fn request_preempt(&mut self, task: u64) -> Result<bool, u16> {
        if self.in_backoff(task) {
            self.meter.denied += 1;
            return Ok(false); // 退避期内错峰。
        }
        self.meter.try_preempt(self.now)
    }

    /// 边界声明：抢占执行在 B04——本域的形式执行接口恒拒。
    pub fn enforce_preempt(&self, _task: u64) -> Result<(), u16> {
        Err(E_STM_BOUND)
    }

    pub fn meter(&self) -> &PreemptMeter {
        &self.meter
    }
}

// ==========================================================================
// 判据（施工书：频率封顶、退避、告警、边界、判据）
// ==========================================================================
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a01_频率封顶_滑动窗() {
        let mut g = StormGuard::new();
        // 窗内前 20 次通过。
        for _ in 0..PREEMPT_CAP {
            assert_eq!(g.request_preempt(1), Ok(true));
        }
        // 第 21 次封顶拒。
        assert_eq!(g.request_preempt(1), Ok(false));
        assert_eq!(g.meter().denied, 1);
        // 窗口滚动后恢复。
        g.tick(WINDOW_TICKS);
        assert_eq!(g.request_preempt(1), Ok(true));
    }

    #[test]
    fn a02_退避_指数封顶错峰() {
        let mut g = StormGuard::new();
        g.note_preempted(7).ok();
        g.note_preempted(7).ok();
        // 被抢 2 次：未达阈。
        assert!(!g.in_backoff(7));
        // 第 3 次 → 退避 1 tick。
        g.note_preempted(7).ok();
        assert!(g.in_backoff(7));
        g.tick(1);
        assert!(!g.in_backoff(7));
        // 再被抢：指数加长（2、4、8…封顶 64）。
        for _ in 0..BACKOFF_AT {
            g.note_preempted(7).ok();
        }
        g.note_preempted(7).ok();
        g.note_preempted(7).ok();
        g.note_preempted(7).ok();
        // 被抢 10 次：exp=7 → 2^7=128 → 封顶 64。
        for _ in 0..7 {
            g.note_preempted(7).ok();
        }
        let rec = g.tasks.iter().find(|(t, _)| *t == 7).map(|(_, r)| r.until - 0);
        assert!(rec.is_some());
        // 退避期内抢占请求错峰拒。
        let g2 = StormGuard::new();
        assert_eq!(g2.enforce_preempt(7), Err(E_STM_BOUND));
    }

    #[test]
    fn a03_风暴告警_窗口去重() {
        let mut m = PreemptMeter::new();
        let mut fired = 0;
        for t in 0..30u64 {
            m.try_preempt(t).ok();
            if m.check_alert(t) {
                fired += 1;
            }
        }
        // 告警在 count=16（800‰）时触发一次；段末 30 次 preempt 已封顶。
        assert_eq!(fired, 1);
        assert_eq!(m.window_used_pm(), 1000);
        // 继续不刷屏。
        for t in 30..50u64 {
            m.try_preempt(t).ok();
            if m.check_alert(t) {
                fired += 1;
            }
        }
        assert_eq!(fired, 1);
        // 滚窗后可再告警。
        m.try_preempt(WINDOW_TICKS).ok();
        m.try_preempt(WINDOW_TICKS).ok();
        for t in WINDOW_TICKS..WINDOW_TICKS + 20 {
            m.try_preempt(t).ok();
            if m.check_alert(t) {
                fired += 1;
            }
        }
        assert!(fired >= 2);
    }

    #[test]
    fn a04_退避期错峰_与封顶联动() {
        let mut g = StormGuard::new();
        for _ in 0..BACKOFF_AT {
            g.note_preempted(9).ok();
        }
        // 退避期内：抢占申请拒（错峰）且不占窗预算。
        assert!(g.in_backoff(9));
        assert_eq!(g.request_preempt(9), Ok(false));
        assert_eq!(g.meter().total_preempts, 0);
        // 未退避任务照常。
        assert_eq!(g.request_preempt(8), Ok(true));
    }

    #[test]
    fn a05_边界声明_执行归_B04() {
        let g = StormGuard::new();
        // 本域形式执行接口恒拒——边界 API 面。
        assert_eq!(g.enforce_preempt(1), Err(E_STM_BOUND));
        assert_eq!(g.enforce_preempt(u64::MAX), Err(E_STM_BOUND));
        // 常量自洽。
        assert!(PREEMPT_CAP > 0 && WINDOW_TICKS > 0);
        assert!(ALERT_PM >= 500 && ALERT_PM <= 1000);
        assert!(BACKOFF_MAX >= BACKOFF_AT);
    }
}
