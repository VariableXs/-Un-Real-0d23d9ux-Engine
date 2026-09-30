//! F429 F11 全屏模式 · 完整设计（STAR I 主册 G-I-29）。
//!
//! **判据（主册）**：全屏渲染边界（零边缝）；顶缘任务栏滑出与自动收回；
//! 退出双键（F11/Esc）；提示一次性；进入/退出动画（F124 强调档 200ms）。
//! ＋通12。
//!
//! 设计：全屏状态机——进入（渲染边界全屏矩形=屏幕矩形，零边缝核算）；
//! 顶缘滑出/3s 自动收回账；退出双键；一次性提示（第二次进入不再提示）；
//! 动画时长常量（F124 强调档 200ms 唯一登记）。
//!
//! v5 纵深：多显示器（任意屏矩形进入，零边缝按生效矩形核）；模态浮层
//! 在开时拒绝进入（三要素人话）；进入前窗口矩形记忆 + 退出还原；
//! 动画打断账（200ms 内退出记打断但不跳变）；顶缘重滑出重置计时。

use crate::checks::CheckSet;

/// F124 强调档动画时长（ms）。
pub const ANIM_MS: u64 = 200;
/// 顶缘任务栏自动收回（ms）。
pub const TOPBAR_AUTOHIDE_MS: u64 = 3_000;
/// 全屏进入时提示显示时长（ms）——仅首次。
pub const HINT_MS: u64 = 3_000;

/// 全屏核。
pub struct Fullscreen {
    pub active: bool,
    /// 首次提示已用（一次性判据）。
    pub hint_used: bool,
    /// 顶缘滑出态与滑出时刻（自动收回账）。
    pub topbar_out: bool,
    pub topbar_out_at: Option<u64>,
    /// 动画账（进入/退出各 200ms）。
    pub enter_anims: u64,
    pub exit_anims: u64,
    /// 屏幕矩形（零边缝基准）。
    pub screen: (i32, i32, u32, u32),
    /// 当前生效全屏矩形（多显示器：可在任意屏进入）。
    active_rect: (i32, i32, u32, u32),
    /// 进入前的窗口矩形（退出还原）。
    pub window_before: Option<(i32, i32, u32, u32)>,
    /// 进入动画被打断账（200ms 内退出）。
    pub interrupted_anims: u64,
    enter_started_at: Option<u64>,
    /// 模态浮层在开（进入前置检查）。
    pub modal_open: bool,
}

impl Fullscreen {
    pub fn new(screen: (i32, i32, u32, u32)) -> Fullscreen {
        Fullscreen {
            active: false,
            hint_used: false,
            topbar_out: false,
            topbar_out_at: None,
            enter_anims: 0,
            exit_anims: 0,
            screen,
            active_rect: screen,
            window_before: None,
            interrupted_anims: 0,
            enter_started_at: None,
            modal_open: false,
        }
    }

    /// F11 进入：渲染边界 = 全屏矩形零边缝（主屏）。
    pub fn enter(&mut self) -> (i32, i32, u32, u32) {
        self.enter_on(self.screen)
    }

    /// 带时间戳进入（动画打断账基准）。
    pub fn enter_at(&mut self, now: u64) -> (i32, i32, u32, u32) {
        self.enter_started_at = Some(now);
        self.enter()
    }

    /// 指定屏进入（多显示器——零边缝按生效矩形核）。
    pub fn enter_on(&mut self, rect: (i32, i32, u32, u32)) -> (i32, i32, u32, u32) {
        self.active = true;
        self.active_rect = rect;
        self.enter_anims += 1;
        rect
    }

    /// 记忆进入前的窗口矩形（退出还原的数据来源）。
    pub fn enter_from(&mut self, window: (i32, i32, u32, u32)) -> (i32, i32, u32, u32) {
        self.window_before = Some(window);
        self.enter()
    }

    /// 进入前置检查：模态浮层在开 → 拒绝（先处理再全屏）。
    pub fn try_enter(&mut self) -> Result<(i32, i32, u32, u32), &'static str> {
        if self.modal_open {
            return Err("有模态浮层打开——先处理再进全屏");
        }
        Ok(self.enter())
    }

    /// 渲染边界零边缝判据：全屏矩形 == 生效矩形（无任务栏预留、无边缝）。
    pub fn zero_seam(&self, render: (i32, i32, u32, u32)) -> bool {
        render == self.active_rect
    }

    /// 首次提示：只提示一次。
    pub fn should_hint(&mut self) -> bool {
        if !self.hint_used {
            self.hint_used = true;
            return true;
        }
        false
    }

    /// 顶缘滑出（鼠标到顶缘；重滑出 → 计时基准刷新）。
    pub fn topbar_slide(&mut self, now: u64) {
        self.topbar_out = true;
        self.topbar_out_at = Some(now);
    }

    /// 自动收回判定：自最近一次滑出超 3s → 收回。
    pub fn topbar_tick(&mut self, now: u64) -> bool {
        if let Some(t) = self.topbar_out_at {
            if self.topbar_out && now - t >= TOPBAR_AUTOHIDE_MS {
                self.topbar_out = false;
                self.topbar_out_at = None;
                return true; // 收回执行
            }
        }
        false
    }

    /// 退出双键：F11 或 Esc（都合法；Esc 经 F424 语义——全屏为面板层）。
    pub fn exit(&mut self, via_f11: bool) -> bool {
        self.exit_at(u64::MAX, via_f11)
    }

    /// 带时间戳退出：200ms 动画窗内退出记打断（但不跳变——干净退出）。
    pub fn exit_at(&mut self, now: u64, via_f11: bool) -> bool {
        if !self.active {
            return false;
        }
        if let Some(t) = self.enter_started_at {
            if now.saturating_sub(t) < ANIM_MS {
                self.interrupted_anims += 1;
            }
        }
        self.active = false;
        self.exit_anims += 1;
        self.topbar_out = false;
        self.topbar_out_at = None;
        let _ = via_f11;
        true
    }

    /// 退出后还原窗口矩形（一次性取出——取完即 None）。
    pub fn restore_window(&mut self) -> Option<(i32, i32, u32, u32)> {
        self.window_before.take()
    }

    /// 动画时长判据（强调档 200ms 双向对称）。
    pub fn anim_symmetric(&self) -> bool {
        self.enter_anims > 0 && self.exit_anims > 0 && ANIM_MS == 200
    }
}

pub fn run_fullscreen_checks() -> CheckSet {
    let mut set = CheckSet::new("uni1-F429");
    set.add(
        "f429-consts",
        ANIM_MS == 200 && TOPBAR_AUTOHIDE_MS == 3_000 && HINT_MS == 3_000,
        "",
    );
    let screen = (0, 0, 2560, 1440);
    let mut f = Fullscreen::new(screen);
    // 进入：零边缝。
    let render = f.enter();
    set.add("f429-zero-seam", f.active && f.zero_seam(render), "");
    // 首次提示一次性。
    set.add(
        "f429-hint-once",
        f.should_hint() && !f.should_hint() && !f.should_hint(),
        "",
    );
    // 顶缘滑出 + 3s 自动收回。
    f.topbar_slide(10_000);
    set.add("f429-topbar-out", f.topbar_out, "");
    set.add("f429-topbar-keep-before-3s", !f.topbar_tick(12_000) && f.topbar_out, "");
    set.add("f429-topbar-autohide-3s", f.topbar_tick(13_000) && !f.topbar_out, "");
    // 退出双键（F11 与 Esc 都合法）。
    set.add("f429-exit-f11", f.exit(true) && !f.active, "");
    f.active = true;
    set.add("f429-exit-esc", f.exit(false) && !f.active, "");
    // 未全屏退出无动作。
    set.add("f429-exit-noop-idle", !f.exit(true), "");
    // 动画对称（200ms 双向）。
    set.add("f429-anim-symmetric", f.anim_symmetric(), "");
    // v5：多显示器——副屏矩形进入同样零边缝。
    let mut m = Fullscreen::new(screen);
    let sub = (-1920i32, 0i32, 1920u32, 1080u32);
    let r = m.enter_on(sub);
    set.add("f429-secondary-monitor-seam", m.active && m.zero_seam(r), "");
    set.add(
        "f429-secondary-rejects-main-rect",
        !m.zero_seam(screen),
        "",
    );
    // v5：模态在开 → 拒绝进入（人话出口）。
    let mut g = Fullscreen::new((0, 0, 1920, 1080));
    g.modal_open = true;
    set.add(
        "f429-modal-block",
        g.try_enter() == Err("有模态浮层打开——先处理再进全屏") && !g.active,
        "",
    );
    // v5：窗口矩形记忆——全屏进 → 退 → 原矩形一次性还原。
    g.modal_open = false;
    let _ = g.enter_from((100, 100, 800, 600));
    set.add(
        "f429-exit-restores-window",
        g.exit(true) && g.restore_window() == Some((100, 100, 800, 600)) && g.restore_window().is_none(),
        "",
    );
    // v5：动画打断账——200ms 内退出记打断（干净退出，不跳变）。
    let mut t = Fullscreen::new((0, 0, 1920, 1080));
    let _ = t.enter_at(1_000);
    set.add(
        "f429-anim-interrupt-accounted",
        t.exit_at(1_100, false) && !t.active && t.interrupted_anims == 1,
        "",
    );
    // v5：过了动画窗再退 → 不算打断。
    let mut t2 = Fullscreen::new((0, 0, 1920, 1080));
    let _ = t2.enter_at(1_000);
    set.add(
        "f429-anim-completed-clean",
        t2.exit_at(1_300, true) && t2.interrupted_anims == 0,
        "",
    );
    // v5：顶缘重滑出 → 计时基准刷新（3s 从最后一次滑出算）。
    let mut b = Fullscreen::new((0, 0, 1920, 1080));
    let _ = b.enter();
    b.topbar_slide(1_000);
    b.topbar_slide(2_500);
    set.add("f429-topbar-reslide-resets", !b.topbar_tick(5_000) && b.topbar_out, "");
    set.add("f429-topbar-autohide-after-reslide", b.topbar_tick(5_500) && !b.topbar_out, "");
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_boundary_never_smaller() {
        let mut f = Fullscreen::new((0, 0, 1920, 1080));
        let r = f.enter();
        // 假如渲染层给了带任务栏预留的矩形 → 零边缝判据必须抓住。
        assert!(!f.zero_seam((0, 0, 1920, 1056)), "任务栏预留 = 边缝缺陷");
        assert!(f.zero_seam(r));
    }

    #[test]
    fn modal_gate_clears_then_allows() {
        let mut f = Fullscreen::new((0, 0, 1920, 1080));
        f.modal_open = true;
        assert!(f.try_enter().is_err());
        f.modal_open = false;
        assert!(f.try_enter().is_ok());
        assert!(f.active);
    }

    #[test]
    fn exit_without_enter_at_has_no_interrupt() {
        let mut f = Fullscreen::new((0, 0, 1920, 1080));
        let _ = f.enter();
        assert!(f.exit(true));
        assert_eq!(f.interrupted_anims, 0, "旧口径退出不计打断（向后兼容）");
    }
}
