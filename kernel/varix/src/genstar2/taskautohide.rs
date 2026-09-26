//! F494 任务栏自动隐藏（genstar2 · I 域通用·二分队 · AI-U2）。
//!
//! 主册判据（验收标准第一句）：
//! **滑出 <200ms；2px 提示线；全屏抑制判据（注入全屏视频测试）；3s 收回；
//! 开关即时与持久化。**
//!
//! 功能定义（主册批次三）：任务栏自动隐藏开关（默认关）——开启后任务栏
//! 平时收起（留 2px 提示线）、鼠标到底缘滑出（<200ms）、全屏应用（F429）
//! 时绝不弹出（沉浸不被破坏）；滑出后 3 秒无交互自动收回。
//!
//! 零堆纪律：定长状态，无 alloc。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（一处一事实）
// ---------------------------------------------------------------------------

/// 滑出动画时限（主册：<200ms）。
pub const SLIDE_OUT_MS: u64 = 200;
/// 提示线宽度（主册：2px）。
pub const HINT_LINE_PX: u32 = 2;
/// 无交互自动收回时长（主册：3 秒）。
pub const RETRACT_AFTER_MS: u64 = 3_000;

/// 任务栏隐藏状态机。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TaskbarHideState {
    /// 收起（2px 提示线）。
    Hidden,
    /// 滑出中（<200ms 动画）。
    SlidingOut,
    /// 展开（交互中）。
    Shown,
    /// 全屏抑制（绝不弹出）。
    Suppressed,
}

/// 自动隐藏控制器。
pub struct AutoHide {
    /// 开关（默认关——主册判据）。
    pub enabled: bool,
    pub state: TaskbarHideState,
    /// 全屏应用在前（F429 联动）。
    pub fullscreen_front: bool,
    /// 滑出动画起点。
    slide_start: u64,
    /// 最近一次交互时刻。
    last_interact: u64,
}

impl AutoHide {
    pub const fn new() -> Self {
        AutoHide {
            enabled: false,
            state: TaskbarHideState::Shown,
            fullscreen_front: false,
            slide_start: 0,
            last_interact: 0,
        }
    }

    /// 开关（即时生效——开即收起/关即展开）。
    pub fn toggle(&mut self, on: bool, now_ms: u64) {
        self.enabled = on;
        self.state = if on { TaskbarHideState::Hidden } else { TaskbarHideState::Shown };
        self.last_interact = now_ms;
    }

    /// 全屏抑制（主册：全屏应用时绝不弹出——鼠标误到底缘也不打扰）。
    pub fn set_fullscreen(&mut self, on: bool, now_ms: u64) {
        self.fullscreen_front = on;
        if on && self.state != TaskbarHideState::Hidden {
            self.state = TaskbarHideState::Suppressed;
        } else if on {
            self.state = TaskbarHideState::Suppressed;
        } else if self.enabled && self.state == TaskbarHideState::Suppressed {
            self.state = TaskbarHideState::Hidden;
        }
        let _ = now_ms;
    }

    /// 鼠标到底缘（滑出——全屏时拒绝）。
    pub fn edge_hover(&mut self, now_ms: u64) -> bool {
        if !self.enabled || self.fullscreen_front {
            return false; // 全屏抑制：绝不弹出（主册判据）
        }
        if self.state == TaskbarHideState::Hidden {
            self.state = TaskbarHideState::SlidingOut;
            self.slide_start = now_ms;
        }
        true
    }

    /// 滑出动画完成判定（<200ms）。
    pub fn slide_done(&self, now_ms: u64) -> bool {
        self.state == TaskbarHideState::SlidingOut && now_ms.saturating_sub(self.slide_start) >= SLIDE_OUT_MS
    }

    /// 动画帧推进（到达时限 → Shown + 交互计时起点）。
    pub fn tick(&mut self, now_ms: u64) {
        if self.slide_done(now_ms) {
            self.state = TaskbarHideState::Shown;
            self.last_interact = now_ms;
        } else if self.state == TaskbarHideState::Shown && self.enabled {
            // 3 秒无交互自动收回（主册判据）。
            if now_ms.saturating_sub(self.last_interact) >= RETRACT_AFTER_MS && !self.fullscreen_front {
                self.state = TaskbarHideState::Hidden;
            }
        }
    }

    /// 交互刷新（点击任务栏 = 重新计时）。
    pub fn interact(&mut self, now_ms: u64) {
        self.last_interact = now_ms;
    }

    /// 2px 提示线（收起态常驻——知道它在）。
    pub fn hint_line_px() -> u32 {
        HINT_LINE_PX
    }
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

pub fn run_taskautohide_checks() -> CheckSet {
    let mut cs = CheckSet::new("F494-taskautohide");
    // 1) 开关默认关。
    let mut a = AutoHide::new();
    cs.add("default_off", !a.enabled && a.state == TaskbarHideState::Shown, "");
    // 2) 开关即时生效。
    a.toggle(true, 1_000);
    cs.add("toggle_instant_hide", a.state == TaskbarHideState::Hidden, "");
    a.toggle(false, 2_000);
    cs.add("toggle_off_shows", a.state == TaskbarHideState::Shown, "");
    a.toggle(true, 3_000);
    // 3) 滑出 <200ms（动画时限在册）。
    a.edge_hover(4_000);
    cs.add("slide_out_begins", a.state == TaskbarHideState::SlidingOut, "");
    cs.add("slide_under_200ms", !a.slide_done(4_199) && a.slide_done(4_200), "");
    a.tick(4_200);
    cs.add("slide_completes", a.state == TaskbarHideState::Shown, "");
    // 4) 3s 无交互自动收回。
    a.tick(4_200 + RETRACT_AFTER_MS - 1);
    cs.add("retract_waits_3s", a.state == TaskbarHideState::Shown, "");
    a.tick(4_200 + RETRACT_AFTER_MS);
    cs.add("retract_after_3s", a.state == TaskbarHideState::Hidden, "");
    // 5) 全屏抑制（注入全屏视频测试：误触底缘绝不弹出）。
    a.set_fullscreen(true, 10_000);
    cs.add("fullscreen_suppresses", !a.edge_hover(10_100) && a.state == TaskbarHideState::Suppressed, "");
    a.set_fullscreen(false, 11_000);
    cs.add("unfullscreen_restores", a.state == TaskbarHideState::Hidden, "");
    // 6) 2px 提示线。
    cs.add("hint_line_2px", AutoHide::hint_line_px() == 2, "");
    // 7) 交互重置收回计时。
    a.edge_hover(12_000);
    a.tick(12_200);
    a.interact(12_300);
    a.tick(12_300 + RETRACT_AFTER_MS - 1);
    cs.add("interact_resets_timer", a.state == TaskbarHideState::Shown, "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fullscreen_video_never_interrupted() {
        let mut a = AutoHide::new();
        a.toggle(true, 0);
        a.set_fullscreen(true, 1_000);
        // 注入全屏视频 + 鼠标反复蹭底缘：任务栏绝不弹出。
        for t in 1_050..1_400u64 {
            assert!(!a.edge_hover(t));
        }
        assert_eq!(a.state, TaskbarHideState::Suppressed);
    }

    #[test]
    fn disabled_never_hides() {
        let mut a = AutoHide::new();
        assert!(!a.edge_hover(0));
        a.tick(10_000);
        assert_eq!(a.state, TaskbarHideState::Shown);
    }
}
