//! F184 热插拔体验 · 批次五深化（secstar · G-G-14）。
//!
//! 批次五功能面（与 b3「全链状态机」、b4「多卷治理」互补，本批管
//! 「图标语言与自动节奏」）：
//! - [`icon_for_state`]：图标状态映射——卷态 → 图标色/动画标记
//!   （绿=可拔 / 转=冲刷中 / 红=脏 / 灰=离线——一眼可读的语言）；
//! - [`AutoFlushTimer`]：自动冲刷计时——写入停止 N 秒后自动冲刷
//!   （用户忘点弹出也不丢数据——自动保存纪律的卷面版）；
//! - [`EjectPolicyLog`]：弹出策略日志——拒/合并/通过三计数
//!   （弹出策略不是黑箱：三种结局各有账）；
//! - [`stale_toast`]：过期 toast 清理——卷已拔则 toast 撤回（浮层
//!   不指向不存在的卷——幽灵引用清零）。
//!
//! 零堆纪律：定长计数 + 状态字段，无 alloc。

use super::hotplug::VolState;
use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 图标状态映射
// ---------------------------------------------------------------------------

/// 图标视觉标记。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IconSpec {
    /// 色：0=灰 1=绿 2=黄(转) 3=红。
    pub color: u8,
    /// 动画：转圈中。
    pub spinning: bool,
}

pub fn icon_for_state(s: VolState) -> IconSpec {
    match s {
        VolState::Absent | VolState::Debouncing => IconSpec { color: 0, spinning: false },
        VolState::Present => IconSpec { color: 1, spinning: false },
        VolState::Flushing => IconSpec { color: 2, spinning: true },
        VolState::SafeToEject => IconSpec { color: 1, spinning: false },
        VolState::Blocked => IconSpec { color: 3, spinning: false },
        VolState::Removed => IconSpec { color: 0, spinning: false },
        VolState::RemovedDirty => IconSpec { color: 3, spinning: false },
    }
}

// ---------------------------------------------------------------------------
// 自动冲刷计时
// ---------------------------------------------------------------------------

/// 写入静默多久后自动冲刷（ms——5s 无写入即冲）。
pub const AUTO_FLUSH_SILENCE_MS: u64 = 5_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AutoFlushTimer {
    pub last_write_ms: u64,
    pub flushing: bool,
}

impl AutoFlushTimer {
    pub const fn new() -> AutoFlushTimer {
        AutoFlushTimer { last_write_ms: 0, flushing: false }
    }

    pub fn on_write(&mut self, at_ms: u64) {
        self.last_write_ms = at_ms;
        self.flushing = false; // 新写入打断冲刷态（有新活不白冲）
    }

    /// 时间一拍：静默满 5s → 自动冲刷（一次性触发）。
    pub fn tick(&mut self, now_ms: u64) -> bool {
        if self.flushing {
            return false;
        }
        if now_ms.saturating_sub(self.last_write_ms) >= AUTO_FLUSH_SILENCE_MS {
            self.flushing = true;
            return true;
        }
        false
    }
}

// ---------------------------------------------------------------------------
// 弹出策略日志
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Default)]
pub struct EjectPolicyLog {
    pub denied: u32,
    pub merged: u32,
    pub passed: u32,
}

impl EjectPolicyLog {
    pub const fn new() -> EjectPolicyLog {
        EjectPolicyLog { denied: 0, merged: 0, passed: 0 }
    }

    pub fn on_denied(&mut self) {
        self.denied += 1;
    }

    pub fn on_merged(&mut self) {
        self.merged += 1;
    }

    pub fn on_passed(&mut self) {
        self.passed += 1;
    }

    /// 总请求数（三计数守恒——策略不丢请求）。
    pub fn total(&self) -> u32 {
        self.denied + self.merged + self.passed
    }

    /// 守恒自检：总请求 = 三计数和（日志完整性）。
    pub fn conserves(&self, observed_total: u32) -> bool {
        self.total() == observed_total
    }
}

// ---------------------------------------------------------------------------
// 过期 toast 清理
// ---------------------------------------------------------------------------

/// toast 是否指向已拔出的卷（幽灵引用判定）。
pub fn stale_toast(toast_drive_alive: bool) -> bool {
    !toast_drive_alive
}

/// 清理策略：卷不在 → toast 立即撤回（不指向不存在的东西）。
pub fn should_withdraw(toast_drive_alive: bool, toast_visible: bool) -> bool {
    toast_visible && stale_toast(toast_drive_alive)
}

// ---------------------------------------------------------------------------
// 批次五自检
// ---------------------------------------------------------------------------

#[inline(never)]
pub fn run_hotplug_b5_checks() -> CheckSet {
    let mut cs = CheckSet::new("F184-b5");

    // 1) 图标映射：七态各自色/动画（视觉语言逐态核对）。
    cs.add(
        "icon_map_states",
        icon_for_state(VolState::SafeToEject) == super::hotplug_b5::IconSpec { color: 1, spinning: false }
            && icon_for_state(VolState::Flushing).spinning
            && icon_for_state(VolState::Blocked).color == 3
            && icon_for_state(VolState::RemovedDirty).color == 3
            && icon_for_state(VolState::Absent).color == 0,
        "",
    );

    // 2) 自动冲刷：静默满 5s 触发一次、冲刷中不重复（一次性）。
    let mut t = AutoFlushTimer::new();
    t.on_write(1_000);
    let not_yet = !t.tick(5_999);
    let fired = t.tick(6_001);
    let again = !t.tick(9_999);
    cs.add("auto_flush_once", not_yet && fired && again && t.flushing, "");

    // 3) 新写入打断冲刷态：冲刷中来写入 → 回到非冲刷（有新活不白冲）。
    t.on_write(10_000);
    cs.add("auto_flush_write_resets", !t.flushing, "");

    // 4) 策略日志守恒：拒 2 合并 3 通过 5 → 总 10 恒等（完整性）。
    let mut log = EjectPolicyLog::new();
    for _ in 0..2 {
        log.on_denied();
    }
    for _ in 0..3 {
        log.on_merged();
    }
    for _ in 0..5 {
        log.on_passed();
    }
    cs.add("policy_log_conserves", log.total() == 10 && log.conserves(10) && !log.conserves(9), "");

    // 5) 空日志守恒：0 = 0（空账也是账）。
    cs.add("policy_log_empty", EjectPolicyLog::new().conserves(0) && !EjectPolicyLog::new().conserves(1), "");

    // 6) 幽灵 toast：卷已拔 → 撤回；卷在 → 留；不可见 → 无事（三态）。
    cs.add(
        "stale_toast",
        should_withdraw(false, true) && !should_withdraw(true, true) && !should_withdraw(false, false),
        "",
    );

    // 7) 常量贯通：自动冲刷 5s 一处一事实。
    cs.add("consts_aligned", AUTO_FLUSH_SILENCE_MS == 5_000, "");

    cs
}

// ---------------------------------------------------------------------------
// 宿主单测（批次五）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests_b5 {
    use super::*;

    #[test]
    fn auto_flush_cycle_repeats() {
        // 周期性：写→冲→写→冲（长挂载多轮自动冲刷）。
        let mut t = AutoFlushTimer::new();
        t.on_write(0);
        assert!(t.tick(5_000));
        t.on_write(6_000);
        assert!(t.tick(11_000));
        assert!(t.flushing);
    }

    #[test]
    fn icon_language_distinct_per_state() {
        // 视觉语言：相邻态不共样式（冲刷中必转圈、可拔必静止绿）。
        let flushing = icon_for_state(VolState::Flushing);
        let safe = icon_for_state(VolState::SafeToEject);
        assert_ne!(flushing, safe);
        assert!(flushing.spinning && !safe.spinning);
    }
}
