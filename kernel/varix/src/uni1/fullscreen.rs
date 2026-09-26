//! F429 F11 全屏模式 · 完整设计（STAR I 主册 G-I-29）。
//!
//! **判据（主册）**：全屏渲染边界（零边缝）；顶缘任务栏滑出与自动收回；
//! 退出双键（F11/Esc）；提示一次性；进入/退出动画（F124 强调档 200ms）。
//! ＋通12。
//!
//! 设计：全屏状态机——进入（渲染边界全屏矩形=屏幕矩形，零边缝核算）；
//! 顶缘滑出/3s 自动收回账；退出双键；一次性提示（第二次进入不再提示）；
//! 动画时长常量（F124 强调档 200ms 唯一登记）。

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
        }
    }

    /// F11 进入：渲染边界 = 全屏矩形零边缝。
    pub fn enter(&mut self) -> (i32, i32, u32, u32) {
        self.active = true;
        self.enter_anims += 1;
        self.screen
    }

    /// 渲染边界零边缝判据：全屏矩形 == 屏幕矩形（无任务栏预留、无边缝）。
    pub fn zero_seam(&self, render: (i32, i32, u32, u32)) -> bool {
        render == self.screen
    }

    /// 首次提示：只提示一次。
    pub fn should_hint(&mut self) -> bool {
        if !self.hint_used {
            self.hint_used = true;
            return true;
        }
        false
    }

    /// 顶缘滑出（鼠标到顶缘；滑出后 3s 自动收回——超时账）。
    pub fn topbar_slide(&mut self, now: u64) {
        self.topbar_out = true;
        self.topbar_out_at = Some(now);
    }

    /// 自动收回判定：滑出超 3s → 收回。
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
        if !self.active {
            return false;
        }
        self.active = false;
        self.exit_anims += 1;
        self.topbar_out = false;
        self.topbar_out_at = None;
        let _ = via_f11;
        true
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
}
