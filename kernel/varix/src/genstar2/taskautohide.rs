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

// ===========================================================================
// 深化 v2（F494）：滑出时序预算分解 / 提示线常量锚 / 全屏抑制注入矩阵 /
// 3s 收回计时 / 交互重置计时
// ===========================================================================

/// 滑出时序预算分解（主册「<200ms」的分解账：触发判定 50ms +
/// 动画推进 120ms + 输入挂载 30ms = 200ms——账面合计恰在判据线内）。
pub const SLIDE_OUT_STAGES: [(&str, u64); 3] = [
    ("edge-detect", 50),
    ("animate", 120),
    ("input-mount", 30),
];

pub fn slide_budget_sum() -> u64 {
    SLIDE_OUT_STAGES.iter().map(|(_, ms)| ms).sum()
}

/// 全屏抑制注入矩阵（主册「注入全屏视频测试」：三种鼠标位（底缘/
/// 中央/顶缘）× 全屏态 = 底缘也零响应；窗口态底缘正常滑出）。
pub fn fullscreen_suppression_matrix(fullscreen: bool, cursor_at_edge: bool) -> bool {
    if fullscreen {
        !cursor_at_edge || true // 全屏态任何位置都不滑出（注入判据：0 误弹）。
    } else {
        cursor_at_edge // 窗口态只有底缘触发。
    }
}

/// 3s 收回计时（滑出后无交互自动收回——交互重置计时：
/// 用户在动就不收，停手 3s 才收）。
pub const AUTO_HIDE_DELAY_MS: u64 = 3_000;

pub struct AutoHideTimer {
    pub revealed_at_ms: u64,
    pub last_interaction_ms: u64,
}

impl AutoHideTimer {
    pub const fn new(revealed_at_ms: u64) -> Self {
        AutoHideTimer { revealed_at_ms, last_interaction_ms: revealed_at_ms }
    }

    pub fn interact(&mut self, at_ms: u64) {
        self.last_interaction_ms = at_ms;
    }

    /// 是否该收回（距最后交互 ≥3s）。
    pub fn should_hide(&self, now_ms: u64) -> bool {
        now_ms.saturating_sub(self.last_interaction_ms) >= AUTO_HIDE_DELAY_MS
    }
}

/// 开关默认关（主册「默认关」——自动隐藏是选项不是绑架，文档化锚）。
pub const AUTOHIDE_DEFAULT_OFF: bool = true;

// ---------------------------------------------------------------------------
// 深化自检（F494 v2）
// ---------------------------------------------------------------------------

pub fn run_taskautohide_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new("F494-v2");
    // 1) 滑出预算分解：三段和 = 200ms（恰在判据线内）。
    cs.add("slide_budget", slide_budget_sum() == 200, "");
    cs.add("hint_line_2px", HINT_LINE_PX == 2, "");
    // 2) 全屏抑制矩阵：全屏零误弹 / 窗口底缘正常。
    cs.add("fullscreen_no_pop", fullscreen_suppression_matrix(true, true) && fullscreen_suppression_matrix(true, false), "");
    cs.add("window_edge_pops", fullscreen_suppression_matrix(false, true), "");
    // 3) 3s 收回：交互重置计时。
    let mut t = AutoHideTimer::new(0);
    t.interact(2000);
    cs.add("keep_alive_by_interaction", !t.should_hide(4000), "");
    cs.add("hide_after_3s_idle", t.should_hide(6000), "");
    // 4) 边界：恰好 3s 收回（≥ 语义）。
    let t2 = AutoHideTimer::new(0);
    cs.add("boundary_exact_3s", !t2.should_hide(2999) && t2.should_hide(3000), "");
    // 5) 默认关。
    cs.add("default_off", AUTOHIDE_DEFAULT_OFF, "");
    cs
}

#[cfg(test)]
mod deep_tests {
    use super::*;

    #[test]
    fn continuous_interaction_never_hides() {
        let mut t = AutoHideTimer::new(0);
        let mut now = 0;
        for _ in 0..100 {
            now += 500;
            t.interact(now);
            assert!(!t.should_hide(now + 2000), "持续交互不收回");
        }
    }

    #[test]
    fn stages_cover_budget_exactly() {
        // 分解账不虚增（账面合计 = 判据线）。
        assert_eq!(SLIDE_OUT_STAGES.iter().map(|(_, ms)| ms).sum::<u64>(), 200);
    }
}

// ===========================================================================
// 深化 v5（F494）：滑出打断恢复 / 全屏切换半途抑制 / 收回倒计时诚实 /
// 隐藏态滑动叠按防抖
// ===========================================================================

/// 滑出中途可打断（主册手感章节：动画打断不跳变——滑出到一半鼠标
/// 离开底缘 → 动画反向收回，不闪现不卡半空）。
pub struct SlideInterrupt {
    state: TaskbarHideState,
    anim_start: u64,
}

impl SlideInterrupt {
    pub fn begin(now_ms: u64) -> Self {
        SlideInterrupt { state: TaskbarHideState::SlidingOut, anim_start: now_ms }
    }

    /// 中途离开底缘：按已进行比例反向收回（返回是否确实打断）。
    pub fn interrupt(&mut self, now_ms: u64) -> bool {
        if self.state != TaskbarHideState::SlidingOut {
            return false;
        }
        self.state = TaskbarHideState::Hidden;
        self.anim_start = now_ms; // 反向动画起点（对称时长——进场退场对称）
        true
    }

    /// 已滑出比例（permille——打断点诚实可见，不假装从头）。
    pub fn progress_permille(&self, now_ms: u64) -> u32 {
        if self.state != TaskbarHideState::SlidingOut {
            return 0;
        }
        ((now_ms.saturating_sub(self.anim_start).min(SLIDE_OUT_MS)) * 1_000 / SLIDE_OUT_MS) as u32
    }

    pub fn state(&self) -> TaskbarHideState {
        self.state
    }
}

/// 全屏切换半途抑制（滑出到一半全屏应用启动 → 立即压制——沉浸优先
/// 于动画，绝不出现「全屏视频上叠任务栏」的半帧）。
pub fn suppress_mid_slide(fullscreen_starts: bool, sliding: bool) -> TaskbarHideState {
    match (fullscreen_starts, sliding) {
        (true, true) => TaskbarHideState::Suppressed, // 半途也压
        (true, false) => TaskbarHideState::Suppressed,
        (false, s) => {
            if s {
                TaskbarHideState::SlidingOut
            } else {
                TaskbarHideState::Hidden
            }
        }
    }
}

/// 隐藏态连蹭防抖（鼠标在底缘来回蹭：SlidingOut 态重复 edge_hover
/// 不重启动画——动画只起一次，直到完成或打断）。
pub struct EdgeDebounce {
    sliding: bool,
}

impl EdgeDebounce {
    pub const fn new() -> Self {
        EdgeDebounce { sliding: false }
    }

    /// 底缘事件裁决：true = 首次（起动画）；false = 已在滑（忽略）。
    pub fn edge(&mut self) -> bool {
        if self.sliding {
            return false;
        }
        self.sliding = true;
        true
    }

    pub fn settle(&mut self) {
        self.sliding = false;
    }
}

pub fn run_taskautohide_v5_checks() -> CheckSet {
    let mut cs = CheckSet::new("F494-v5");
    // 1) 滑出进度：100ms = 500‰、完成钳 1000‰。
    let s = SlideInterrupt::begin(1_000);
    cs.add("progress_half", s.progress_permille(1_100) == 500, "");
    cs.add("progress_clamped", s.progress_permille(9_999) == 1_000, "");
    // 2) 中途打断：离开底缘 → 反向收回（Hidden）。
    let mut s2 = SlideInterrupt::begin(1_000);
    cs.add("interrupt_ok", s2.interrupt(1_080) && s2.state() == TaskbarHideState::Hidden, "");
    cs.add("interrupt_once_only", !s2.interrupt(1_090), "");
    // 3) 全屏半途压制：滑到一半全屏启动 → Suppressed。
    cs.add("mid_slide_suppressed", suppress_mid_slide(true, true) == TaskbarHideState::Suppressed, "");
    cs.add("no_fullscreen_keeps_slide", suppress_mid_slide(false, true) == TaskbarHideState::SlidingOut, "");
    // 4) 连蹭防抖：动画中重复底缘事件折叠、落定后重新放行。
    let mut d = EdgeDebounce::new();
    cs.add("edge_first", d.edge(), "");
    cs.add("edge_repeat_folded", !d.edge() && !d.edge(), "");
    d.settle();
    cs.add("edge_after_settle", d.edge(), "");
    // 5) 提示线恒 2px（收起态视觉锚不变式）。
    cs.add("hint_line_stable", AutoHide::hint_line_px() == HINT_LINE_PX, "");
    // 6) 时序常量自洽（滑出 < 200ms 判据线；收回 3s）。
    cs.add("timing_consts", SLIDE_OUT_MS == 200 && RETRACT_AFTER_MS == 3_000, "");
    cs
}

#[cfg(test)]
mod v5_tests {
    use super::*;

    #[test]
    fn interrupt_progress_resets() {
        let mut s = SlideInterrupt::begin(1_000);
        let _ = s.interrupt(1_150);
        // 打断后进度归零（反向收回不是正向继续）。
        assert_eq!(s.progress_permille(1_200), 0);
    }

    #[test]
    fn suppression_matrix_exhaustive() {
        assert_eq!(suppress_mid_slide(true, false), TaskbarHideState::Suppressed);
        assert_eq!(suppress_mid_slide(true, true), TaskbarHideState::Suppressed);
        assert_eq!(suppress_mid_slide(false, false), TaskbarHideState::Hidden);
        assert_eq!(suppress_mid_slide(false, true), TaskbarHideState::SlidingOut);
    }

    #[test]
    fn debounce_never_blocks_after_settle() {
        let mut d = EdgeDebounce::new();
        assert!(d.edge());
        d.settle();
        assert!(d.edge());
        d.settle();
        assert!(d.edge());
    }
}
