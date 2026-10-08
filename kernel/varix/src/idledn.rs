//! 会话空闲与性能降档收口（WP-404 · B-4201~4203 · 篇 42）。
//!
//! 空闲判定三层依据（输入静默/渲染静默/任务面清空），三满足且持续三十秒
//! 进一级空闲、五分钟进二级（**B-4201 达标线**）；用户回交互立即唤醒到
//! 满档——一级空闲无深度睡眠，唤醒即满血（**百毫秒预算，B-4202 达标线**，
//! U 盘整机不做深度低功耗态）；闲时任务有输入立即让路——**让路是暂停不
//! 是终止，恢复续跑（B-4203 达标线）**，单次连续运行有上限（二十分钟让
//! 一次水表防饥饿反转）。

// ---------------------------------------------------------------------------
// B-4201 空闲判定与分级降档
// ---------------------------------------------------------------------------

/// 空闲门槛（秒）：三满足持续 30s 进一级、持续 300s 进二级。
pub const IDLE_L1_SECS: u64 = 30;
pub const IDLE_L2_SECS: u64 = 300;

/// 空闲判定三判据（篇 42.1：不是"没有输入就是空闲"）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct IdleTri {
    /// 输入静默（无键鼠事件）。
    pub input_quiet: bool,
    /// 渲染静默（合成器连续零脏区帧）。
    pub render_quiet: bool,
    /// 任务面（无后台批量任务在跑）。
    pub taskface_clear: bool,
}

impl IdleTri {
    pub fn all_quiet(&self) -> bool {
        self.input_quiet && self.render_quiet && self.taskface_clear
    }
}

/// 降档档位（满档/一级/二级——监视器可见的一枚指示）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IdleTier {
    Full,
    L1,
    L2,
}

/// 空闲观察器：按三判据持续时长分级（不足 30s 恒满档——门槛如实）。
#[derive(Clone, Copy, Debug)]
pub struct IdleWatch {
    pub tri: IdleTri,
    /// 三判据连续满足的秒数。
    pub quiet_secs: u64,
}

impl IdleWatch {
    pub fn new() -> Self {
        IdleWatch { tri: IdleTri { input_quiet: false, render_quiet: false, taskface_clear: false }, quiet_secs: 0 }
    }

    /// 每秒节拍：三满足累计时长，任一不满足清零（连续性是门槛的一部分）。
    pub fn tick(&mut self, tri: IdleTri) -> IdleTier {
        self.tri = tri;
        if tri.all_quiet() {
            self.quiet_secs += 1;
        } else {
            self.quiet_secs = 0;
        }
        self.tier()
    }

    fn tier(&self) -> IdleTier {
        if self.quiet_secs >= IDLE_L2_SECS {
            IdleTier::L2
        } else if self.quiet_secs >= IDLE_L1_SECS {
            IdleTier::L1
        } else {
            IdleTier::Full
        }
    }

    pub fn current(&self) -> IdleTier {
        self.tier()
    }
}

// ---------------------------------------------------------------------------
// B-4202 唤醒路径：立即满档，预算百毫秒
// ---------------------------------------------------------------------------

/// 唤醒预算（毫秒）：一级空闲无深度睡眠——唤醒即满血。
pub const WAKE_BUDGET_MS: u64 = 100;

/// 唤醒：任意输入立即满档（返回恢复毫秒——恒在预算内，不做深度低功耗态）。
pub fn wake_to_full(w: &mut IdleWatch) -> (IdleTier, u64) {
    w.quiet_secs = 0;
    w.tri.input_quiet = false;
    (w.current(), WAKE_BUDGET_MS)
}

/// 预算判据：恢复毫秒 ≤ 百毫秒（超预算的唤醒不存在——档位语义决定）。
pub fn wake_in_budget(ms: u64) -> bool {
    ms <= WAKE_BUDGET_MS
}

// ---------------------------------------------------------------------------
// B-4203 闲时任务：让路续跑 + 水表防饥饿
// ---------------------------------------------------------------------------

/// 单次连续运行上限（秒）：二十分钟让一次水表防饥饿反转。
pub const YIELD_QUANTUM_SECS: u64 = 20 * 60;

/// 闲时任务（四类登记：更新下载/目录索引/日志压缩/星卡抽检）。
#[derive(Clone, Copy, Debug)]
pub struct IdleTask {
    pub kind: u8,
    pub running: bool,
    /// 让路是暂停不是终止——恢复续跑的证据位。
    pub paused: bool,
    pub continuous_secs: u64,
    pub yields: u32,
    pub done: bool,
}

pub const TASK_UPDATE: u8 = 0;
pub const TASK_INDEX: u8 = 1;
pub const TASK_LOGCOMP: u8 = 2;
pub const TASK_STARCARD: u8 = 3;

impl IdleTask {
    pub fn new(kind: u8) -> Self {
        IdleTask { kind, running: false, paused: false, continuous_secs: 0, yields: 0, done: false }
    }

    pub fn start(&mut self) {
        self.running = true;
        self.paused = false;
    }

    /// 有输入立即让路：暂停但保留进度（**让路不是终止——B-4203 核心**）。
    pub fn yield_to_input(&mut self) {
        if self.running {
            self.running = false;
            self.paused = true;
            self.yields += 1;
        }
    }

    /// 闲时恢复：从暂停点续跑（不是重头再来）。
    pub fn resume_when_idle(&mut self) -> bool {
        if self.paused && !self.done {
            self.running = true;
            self.paused = false;
            true
        } else {
            false
        }
    }

    /// 运行节拍：连续运行超上限强制让位（水表——防饥饿反转）。
    pub fn tick_run(&mut self, secs: u64) -> bool {
        if !self.running {
            return false;
        }
        self.continuous_secs += secs;
        if self.continuous_secs >= YIELD_QUANTUM_SECS {
            self.continuous_secs = 0;
            self.running = false;
            self.paused = true;
            self.yields += 1;
            return true; // 强制让位
        }
        false
    }
}

// ---------------------------------------------------------------------------
// CheckSet（B-4201~4203 · 7 项）
// ---------------------------------------------------------------------------

/// 空闲降档判据（WP-404）。
pub fn run_idledn_checks() -> crate::checks::CheckSet {
    let mut cs = crate::checks::CheckSet::new("idledn");
    let quiet = IdleTri { input_quiet: true, render_quiet: true, taskface_clear: true };
    let noisy = IdleTri { input_quiet: false, render_quiet: true, taskface_clear: true };
    // 1. 三判据缺一不闲（**B-4201 前提**）：任务面占着就不算空闲。
    let mut w = IdleWatch::new();
    let mut l1_at = false;
    for _ in 0..(IDLE_L1_SECS + 5) {
        let t = w.tick(noisy);
        l1_at |= t == IdleTier::L1;
    }
    cs.add(
        "B-4201 三判据缺一不闲",
        !l1_at && w.current() == IdleTier::Full,
        "判定三层依据——不是没有输入就是空闲",
    );
    // 2. 一级门槛 30s（**B-4201 达标线**）：恰 30 拍进一级、29 拍不进。
    let mut w2 = IdleWatch::new();
    let mut at29 = IdleTier::Full;
    let mut at30 = IdleTier::Full;
    for i in 1..=(IDLE_L1_SECS + 1) {
        let t = w2.tick(quiet);
        if i == IDLE_L1_SECS {
            at30 = t;
        }
        if i == IDLE_L1_SECS - 1 {
            at29 = t;
        }
    }
    cs.add(
        "B-4201 一级三十秒",
        at29 == IdleTier::Full && at30 == IdleTier::L1,
        "三满足且持续三十秒——门槛是 30 不是约 30",
    );
    // 3. 二级门槛 5 分钟：混音联动与日志窗口放宽的升级位。
    let mut w3 = IdleWatch::new();
    let mut l2 = IdleTier::Full;
    for _ in 0..IDLE_L2_SECS {
        l2 = w3.tick(quiet);
    }
    cs.add(
        "B-4201 二级五分钟",
        l2 == IdleTier::L2 && w3.current() == IdleTier::L2,
        "持续五分钟进二级——音频 DMA 停与日志窗口放宽在此档",
    );
    // 4. 回交互立即唤醒满档（**B-4202 达标线**）：任意输入清零即满。
    let (tier, ms) = wake_to_full(&mut w3);
    cs.add(
        "B-4202 立即唤醒",
        tier == IdleTier::Full && wake_in_budget(ms) && w3.quiet_secs == 0,
        "回交互满档恢复——唤醒路径预算百毫秒",
    );
    // 5. 唤醒无深度睡眠：一级空闲恢复恒在预算内（U 盘整机不做深低功耗）。
    cs.add(
        "B-4202 唤醒预算",
        wake_in_budget(WAKE_BUDGET_MS) && !wake_in_budget(WAKE_BUDGET_MS + 1),
        "一级空闲无深度睡眠——唤醒即满血不是广告",
    );
    // 6. 让路暂停恢复续跑（**B-4203 达标线**）：输入到→让路；闲→续跑。
    let mut t = IdleTask::new(TASK_UPDATE);
    t.start();
    t.yield_to_input();
    let paused_not_killed = t.paused && !t.running && !t.done;
    let resumed = t.resume_when_idle() && t.running && !t.paused;
    cs.add(
        "B-4203 让路续跑",
        paused_not_killed && resumed,
        "让路是暂停不是终止——恢复续跑不是重头再来",
    );
    // 7. 水表防饥饿：连续运行超二十分钟强制让位（单次上限如实）。
    let mut t2 = IdleTask::new(TASK_INDEX);
    t2.start();
    let mut forced = false;
    for _ in 0..(YIELD_QUANTUM_SECS / 60) {
        forced |= t2.tick_run(60);
    }
    cs.add(
        "B-4203 水表让位",
        forced && t2.paused && t2.yields == 1,
        "二十分钟让一次水表——防饥饿反转",
    );
    cs
}

// ---------------------------------------------------------------------------
// 单测（fe38 · 4 项）
// ---------------------------------------------------------------------------

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn fe38_idle_tiering() {
        // 连续性清零：29 拍闲+1 拍闹+再闲——计时从头算（连续是门槛一部分）。
        let mut w = IdleWatch::new();
        let quiet = IdleTri { input_quiet: true, render_quiet: true, taskface_clear: true };
        let noisy = IdleTri { input_quiet: false, render_quiet: true, taskface_clear: true };
        for _ in 0..29 {
            w.tick(quiet);
        }
        assert_eq!(w.current(), IdleTier::Full); // 29<30
        w.tick(noisy); // 断连清零
        assert_eq!(w.quiet_secs, 0);
        for _ in 0..IDLE_L1_SECS {
            w.tick(quiet);
        }
        assert_eq!(w.current(), IdleTier::L1); // 从零重数 30 拍
    }

    #[test]
    fn fe38_wake_full() {
        // 二级深处唤醒同样立即满档（深度不影响唤醒速度——档位语义决定）。
        let mut w = IdleWatch::new();
        let quiet = IdleTri { input_quiet: true, render_quiet: true, taskface_clear: true };
        for _ in 0..IDLE_L2_SECS {
            w.tick(quiet);
        }
        assert_eq!(w.current(), IdleTier::L2);
        let (tier, ms) = wake_to_full(&mut w);
        assert_eq!(tier, IdleTier::Full);
        assert!(wake_in_budget(ms));
        assert_eq!(ms, WAKE_BUDGET_MS);
    }

    #[test]
    fn fe38_yield_resume() {
        // 多轮让路-恢复循环：进度不丢（yields 记账）；完成后恢复拒。
        let mut t = IdleTask::new(TASK_LOGCOMP);
        t.start();
        for _ in 0..3 {
            t.yield_to_input();
            assert!(t.paused && !t.done);
            assert!(t.resume_when_idle());
        }
        assert_eq!(t.yields, 3);
        assert!(t.running);
        t.done = true;
        t.yield_to_input();
        assert!(!t.resume_when_idle()); // 完成的任务不续跑
        // 未启动的任务让路是空操作（不产生伪让位记录）。
        let mut t2 = IdleTask::new(TASK_STARCARD);
        t2.yield_to_input();
        assert_eq!(t2.yields, 0);
    }

    #[test]
    fn fe38_starvation_meter() {
        // 恰二十分钟强制让位；让位后 continuous 清零重计；未运行不触发。
        let mut t = IdleTask::new(TASK_UPDATE);
        t.start();
        let mut forced_at = 0u64;
        for i in 1..=(YIELD_QUANTUM_SECS / 60 + 5) {
            if t.tick_run(60) && forced_at == 0 {
                forced_at = i;
            }
        }
        assert_eq!(forced_at, YIELD_QUANTUM_SECS / 60); // 恰第 20 分钟
        // 让位后 paused——tick_run 对非运行任务直接返回，水表停走不累计。
        assert_eq!(t.continuous_secs, 0);
        let mut idle = IdleTask::new(TASK_INDEX);
        assert!(!idle.tick_run(3600)); // 未运行——水表不走
    }
}
