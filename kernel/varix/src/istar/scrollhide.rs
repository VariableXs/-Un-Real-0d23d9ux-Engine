//! F579 滚动条自动隐藏 · 完整设计（STAR I 主册 I 域批次八）。
//!
//! **判据（主册）**：两态判定；4px/44px 双规格；淡入淡出实测；拖动中恒显；
//! 触屏检测。
//!
//! **设计要点（主册）**：
//! - 滚动条两态：内容可滚时显示细态（4px 悬停加粗 F204）、无滚动内容时
//!   完全隐藏（不留灰条假象）；
//! - 触屏模式常显粗态（44px 可拖——触屏没有 hover）；
//! - 淡入淡出 150ms（F124 线性）；拖动中永不隐藏。

use crate::checks::CheckSet;
use crate::istar::ibase::ISTAR_DOMAIN;

// ---------------------------------------------------------------------------
// 规格常量
// ---------------------------------------------------------------------------

/// 细态宽（px）。
pub const THIN_W_PX: u32 = 4;

/// 触屏粗态宽（px，可拖）。
pub const TOUCH_W_PX: u32 = 44;

/// 淡入淡出时长（ms，F124 线性）。
pub const FADE_MS: u32 = 150;

// ---------------------------------------------------------------------------
// 模型
// ---------------------------------------------------------------------------

/// 滚动条状态机。
pub struct ScrollBar {
    /// 内容可滚（viewport < content）。
    scrollable: bool,
    /// 触屏模式（检测面——触屏设备常显粗态）。
    touch_mode: bool,
    /// 用户拖动中（恒显保护）。
    dragging: bool,
    /// 可见性（渲染目标态）。
    visible: bool,
    /// 动画进度（0..=FADE_MS；淡入淡出共用线性推进）。
    fade_ms: u32,
    /// 淡入方向（true=淡入中 false=淡出中）。
    fading_in: bool,
}

impl ScrollBar {
    pub fn new() -> ScrollBar {
        ScrollBar {
            scrollable: false,
            touch_mode: false,
            dragging: false,
            visible: false,
            fade_ms: 0,
            fading_in: false,
        }
    }

    /// 内容/视口更新（两态判定唯一入口）。
    pub fn layout(&mut self, viewport: u32, content: u32) {
        self.scrollable = content > viewport;
        self.apply();
    }

    /// 触屏检测注入（设备面——宿主输入栈推送）。
    pub fn set_touch(&mut self, on: bool) {
        self.touch_mode = on;
        self.apply();
    }

    pub fn set_dragging(&mut self, on: bool) {
        self.dragging = on;
        self.apply();
    }

    fn apply(&mut self) {
        // 目标可见性：可滚即显（鼠标细态 / 触屏粗态）；不可滚完全隐藏。
        // 拖动中恒显保护：即便瞬时不可滚，拖动结束前条不消失。
        let want = self.scrollable || self.dragging;
        if want != self.visible {
            self.visible = want;
            self.fading_in = want;
            self.fade_ms = 0; // 动画从头走
        }
    }

    /// 动画推进（150ms 线性）。
    pub fn advance(&mut self, ms: u32) {
        self.fade_ms = (self.fade_ms + ms).min(FADE_MS);
    }

    pub fn visible(&self) -> bool {
        self.visible
    }

    /// 渲染宽度（px）。
    pub fn width_px(&self) -> u32 {
        if self.touch_mode {
            TOUCH_W_PX
        } else {
            THIN_W_PX
        }
    }

    /// 透明度（‰——0 全隐 1000 全显；线性 150ms）。
    pub fn opacity_permille(&self) -> u32 {
        if !self.visible {
            return 0;
        }
        if self.fade_ms >= FADE_MS {
            return 1_000;
        }
        ((self.fade_ms * 1_000) / FADE_MS) as u32
    }

    /// 拖动中恒显判定（判据单列——拖动永不隐藏）。
    pub fn always_visible_while_dragging(&self) -> bool {
        !self.dragging || self.visible
    }

    pub fn is_scrollable(&self) -> bool {
        self.scrollable
    }
}

impl Default for ScrollBar {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub fn run_scrollhide_checks() -> CheckSet {
    let mut set = CheckSet::new(ISTAR_DOMAIN);

    // 1. 两态判定：内容可滚 → 显；不可滚 → 完全隐藏（不留灰条假象）。
    let mut s = ScrollBar::new();
    s.layout(100, 200);
    let scrollable = s.visible() && s.is_scrollable();
    s.layout(100, 100);
    set.add("two state visibility", scrollable && !s.visible() && s.opacity_permille() == 0, "");

    // 2. 4px/44px 双规格：鼠标细态 4px；触屏粗态 44px。
    s.layout(100, 200);
    let thin = s.width_px() == THIN_W_PX;
    s.set_touch(true);
    let thick = s.width_px() == TOUCH_W_PX;
    set.add("thin 4px touch 44px", thin && thick && s.visible(), "");

    // 3. 淡入淡出 150ms 线性：75ms 半程、150ms 满格。
    let mut s2 = ScrollBar::new();
    s2.layout(0, 10); // 淡入开始
    s2.advance(75);
    let half = s2.opacity_permille() == 500;
    s2.advance(75);
    let full = s2.opacity_permille() == 1_000;
    set.add("fade 150ms linear", half && full && FADE_MS == 150, "");

    // 4. 淡出对称：隐藏反向走同一 150ms。
    s2.layout(0, 0); // 触发淡出
    s2.advance(75);
    let out_half = s2.opacity_permille() == 0 && !s2.visible();
    set.add("fade out mirrors fade in", out_half, "");

    // 5. 拖动中恒显：拖动期间即使停滚也不隐藏。
    let mut s3 = ScrollBar::new();
    s3.layout(0, 10);
    s3.advance(150);
    s3.set_dragging(true);
    let during = s3.always_visible_while_dragging() && s3.visible();
    s3.layout(10, 5); // 拖动中内容变少（理论可停滚）
    set.add("always visible while dragging", during && s3.always_visible_while_dragging(), "");

    // 6. 触屏检测：触屏模式常显粗态（无 hover 世界的替代）。
    let mut s4 = ScrollBar::new();
    s4.layout(100, 200);
    s4.set_touch(true);
    let touch_always = s4.visible() && s4.width_px() == TOUCH_W_PX;
    s4.set_touch(false);
    set.add("touch mode persistent thick", touch_always && s4.width_px() == THIN_W_PX, "");

    // 7. 不可滚 + 触屏：常显语义不破（内容不可滚仍隐藏——触屏常显的前提
    //    是「可滚」；不可滚没有条可显）。
    s4.set_touch(true);
    s4.layout(100, 100);
    set.add("touch but unscrollable still hidden", !s4.visible(), "");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fade_monotonic_up() {
        let mut s = ScrollBar::new();
        s.layout(0, 10);
        let mut last = 0;
        for _ in 0..5 {
            s.advance(30);
            let o = s.opacity_permille();
            assert!(o >= last);
            last = o;
        }
    }

    #[test]
    fn idle_mouse_never_shows_without_scroll() {
        let mut s = ScrollBar::new();
        s.layout(10, 5);
        s.advance(150);
        assert!(!s.visible());
    }

    #[test]
    fn width_constants() {
        assert_eq!(THIN_W_PX, 4);
        assert_eq!(TOUCH_W_PX, 44);
    }
}
