//! NOVA-UI · src/features/uikit 控件基类 → 内核移植（第四批：F216/F436 依赖件）。
//!
//! 覆盖 Z-02 Fluent 控件规格的全部交互原语，几何与状态机与 src 一致：
//! 按钮（32dp / min-w 120 / hover 抬升 / 按压下压 / 焦点环）、菜单项（36dp，紧凑 31dp）、
//! 复选框（20dp）、文本输入（32dp）、分隔线（1dp）、触控命中红线（44dp）。
//! 全部几何经 nova4k::dp 做 4K 等比换算；命中测试纯整数。
//!
//! 纯逻辑 + 固定容量：no_std / 仅 core，无分配、无 unsafe。

use crate::checks::CheckSet;
use crate::designsys::nova4k::{dp, CTL_BTN, CTL_BTN_MINW, CTL_CHECKBOX, CTL_DIVIDER, CTL_INPUT, CTL_MENU_ITEM, CTL_MENU_ITEM_COMPACT, CTL_PAD_BTN, CTL_TOUCH};

// ===========================================================================
// 1. 控件状态（Z-05/U-09 状态规范）
// ===========================================================================
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CtrlState {
    Idle,
    Hover,
    Pressed,
    /// 键盘焦点（:focus-visible 语义——仅键盘导航出现）
    Focus,
    Disabled,
}

/// U-09 光影语义：hover 抬升 / active 下压（elev 档位索引，0-5）。
pub fn elevation(state: CtrlState) -> u32 {
    match state {
        CtrlState::Hover => 1,  // --elev-hover: var(--elev-1)
        CtrlState::Pressed => 0, // 下压：贴底
        CtrlState::Focus => 1,
        _ => 0,
    }
}

/// Z-05 焦点环：双线（内 2dp 画布色 + 外 2dp 强调色），仅 Focus 态。
pub const FOCUS_RING_INNER_DP: u32 = 2;
pub const FOCUS_RING_OUTER_DP: u32 = 2;

/// V-77 焦点环三样式。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FocusRingStyle {
    /// system = 双线（现状）
    System,
    /// box = 2px accent 外框
    Box,
    /// underline = 底部 2px accent
    Underline,
}

// ===========================================================================
// 2. 按钮（Z-02：32dp 高 / min-w 120dp / pad 4dp）
// ===========================================================================
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ButtonSpec {
    pub w_dp: u32,
    pub h_dp: u32,
    pub min_w_dp: u32,
    pub pad_dp: u32,
}

pub const BUTTON: ButtonSpec =
    ButtonSpec { w_dp: CTL_BTN, h_dp: CTL_BTN, min_w_dp: CTL_BTN_MINW, pad_dp: CTL_PAD_BTN };

impl ButtonSpec {
    /// 实际宽度：内容宽 + 2×pad，钳制 min_w。
    pub const fn width_for_content(&self, content_w_dp: u32) -> u32 {
        let w = content_w_dp + 2 * self.pad_dp;
        if w < self.min_w_dp {
            self.min_w_dp
        } else {
            w
        }
    }
    /// 触控命中区（≥44dp 红线）：几何不足时命中区向外扩展。
    pub const fn hit_height(&self) -> u32 {
        if self.h_dp >= CTL_TOUCH {
            self.h_dp
        } else {
            CTL_TOUCH
        }
    }
    /// 圆角：控件档 8dp。
    pub const fn radius_dp(&self) -> u32 {
        8
    }
}

// ===========================================================================
// 3. 菜单项（36dp；紧凑密度 31dp，V-74 -15%）
// ===========================================================================
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MenuItemSpec {
    pub h_dp: u32,
    pub compact: bool,
}

pub const MENU_ITEM: MenuItemSpec = MenuItemSpec { h_dp: CTL_MENU_ITEM, compact: false };
pub const MENU_ITEM_COMPACT: MenuItemSpec = MenuItemSpec { h_dp: CTL_MENU_ITEM_COMPACT, compact: true };

impl MenuItemSpec {
    /// 菜单面板高 = n 项 × 行高 + 上下各 4dp 内边距。
    pub const fn panel_height(&self, n_items: u32) -> u32 {
        n_items * self.h_dp + 2 * 4
    }
}

// ===========================================================================
// 4. 复选框 / 输入框 / 分隔线
// ===========================================================================
pub const CHECKBOX_DP: u32 = CTL_CHECKBOX; // 20
pub const INPUT_H_DP: u32 = CTL_INPUT; // 32
pub const DIVIDER_H_DP: u32 = CTL_DIVIDER; // 1

/// 复选框选中态：勾选标记内缩 4dp。
pub const CHECK_MARK_INSET_DP: u32 = 4;

// ===========================================================================
// 5. 命中测试（纯整数，4K 物理坐标）
// ===========================================================================
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

impl Rect {
    pub const fn contains(&self, px: u32, py: u32) -> bool {
        px >= self.x && px < self.x + self.w && py >= self.y && py < self.y + self.h
    }
}

/// 命中区扩展：控件几何 < 触控红线时按中心扩展到红线（返回扩展后的命中矩形）。
pub fn touch_hit_rect(r: Rect, scale_milli: u32) -> Rect {
    let need = dp(CTL_TOUCH, scale_milli);
    if r.w >= need && r.h >= need {
        return r;
    }
    let cx = r.x + r.w / 2;
    let cy = r.y + r.h / 2;
    let w = if r.w >= need { r.w } else { need };
    let h = if r.h >= need { r.h } else { need };
    Rect {
        x: cx.saturating_sub(w / 2),
        y: cy.saturating_sub(h / 2),
        w,
        h,
    }
}

/// 圆角矩形内点判定（整数近似：四角 1/2 半径平方判据，const 可用）。
pub const fn rounded_contains(r: Rect, radius: u32, px: u32, py: u32) -> bool {
    if !r.contains(px, py) {
        return false;
    }
    let rr = radius as i64;
    let pxl = px as i64;
    let pyl = py as i64;
    let x0 = r.x as i64;
    let y0 = r.y as i64;
    let x1 = r.x as i64 + r.w as i64;
    let y1 = r.y as i64 + r.h as i64;
    // 左上角
    if pxl < x0 + rr && pyl < y0 + rr {
        let dx = pxl - (x0 + rr);
        let dy = pyl - (y0 + rr);
        if dx * dx + dy * dy > rr * rr {
            return false;
        }
    }
    // 右上角
    if pxl > x1 - rr && pyl < y0 + rr {
        let dx = pxl - (x1 - rr);
        let dy = pyl - (y0 + rr);
        if dx * dx + dy * dy > rr * rr {
            return false;
        }
    }
    // 左下角
    if pxl < x0 + rr && pyl > y1 - rr {
        let dx = pxl - (x0 + rr);
        let dy = pyl - (y1 - rr);
        if dx * dx + dy * dy > rr * rr {
            return false;
        }
    }
    // 右下角
    if pxl > x1 - rr && pyl > y1 - rr {
        let dx = pxl - (x1 - rr);
        let dy = pyl - (y1 - rr);
        if dx * dx + dy * dy > rr * rr {
            return false;
        }
    }
    true
}

// ===========================================================================
// 6. CheckSet 自检
// ===========================================================================
pub fn checks(cs: &mut CheckSet) {
    fn g(cs: &mut CheckSet, n: &str, ok: bool) {
        cs.check(n, ok);
    }

    g(cs, "novaui-button-spec", BUTTON.h_dp == 32 && BUTTON.min_w_dp == 120 && BUTTON.pad_dp == 4);
    g(cs, "novaui-btn-width", BUTTON.width_for_content(50) == 120 && BUTTON.width_for_content(200) == 208);
    g(cs, "novaui-btn-touch", BUTTON.hit_height() == 44);
    g(cs, "novaui-menu-36", MENU_ITEM.h_dp == 36 && MENU_ITEM_COMPACT.h_dp == 31);
    g(cs, "novaui-menu-panel", MENU_ITEM.panel_height(6) == 224 && MENU_ITEM_COMPACT.panel_height(6) == 194);
    g(cs, "novaui-ctrl-sizes", CHECKBOX_DP == 20 && INPUT_H_DP == 32 && DIVIDER_H_DP == 1);
    g(cs, "novaui-elev", elevation(CtrlState::Hover) == 1 && elevation(CtrlState::Pressed) == 0);
    g(cs, "novaui-focus-ring", FOCUS_RING_INNER_DP == 2 && FOCUS_RING_OUTER_DP == 2);

    // 命中测试（1x 基准）
    let btn = Rect { x: 100, y: 100, w: 32, h: 32 };
    g(cs, "novaui-hit-in", btn.contains(115, 115) && !btn.contains(200, 115));
    let hit = touch_hit_rect(btn, 1000);
    g(cs, "novaui-hit-expand", hit.w == 44 && hit.h == 44 && hit.x == 94 && hit.y == 94);
    // 4K：触控红线 = 88px；64px 按钮几何仍需扩展到 88
    let btn4k = Rect { x: 100, y: 100, w: 64, h: 64 };
    let hit4k = touch_hit_rect(btn4k, 2000);
    g(cs, "novaui-hit-4k-touch88", hit4k.w == 88 && hit4k.h == 88 && hit4k.x == 88 && hit4k.y == 88);
    // 96px 几何 ≥ 88 红线 → 不扩展
    let big4k = Rect { x: 0, y: 0, w: 96, h: 96 };
    g(cs, "novaui-hit-4k-noexpand", touch_hit_rect(big4k, 2000) == big4k);

    // 圆角内点：中心接受、角弧外拒绝、直边接受
    let card = Rect { x: 0, y: 0, w: 100, h: 100 };
    g(cs, "novaui-round-center", rounded_contains(card, 8, 50, 50));
    g(cs, "novaui-round-corner-out", !rounded_contains(card, 8, 1, 1));
    g(cs, "novaui-round-corner-in", rounded_contains(card, 8, 6, 6)); // (2²+2²)=8 ≤ 64 圆弧内
    g(cs, "novaui-round-edge", rounded_contains(card, 8, 50, 0));
}

// ===========================================================================
// 7. 单元测试（宿主机 std 下运行）
// ===========================================================================
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn button_matches_z02() {
        assert_eq!(BUTTON.width_for_content(0), 120);
        assert_eq!(BUTTON.width_for_content(500), 508);
        assert_eq!(BUTTON.hit_height(), 44);
        assert_eq!(BUTTON.radius_dp(), 8);
    }

    #[test]
    fn menu_heights() {
        assert_eq!(MENU_ITEM.panel_height(5), 188);
        assert_eq!(MENU_ITEM_COMPACT.panel_height(5), 163);
    }

    #[test]
    fn hit_testing_1x_and_4k() {
        let r = Rect { x: 10, y: 10, w: 20, h: 20 };
        let hit = touch_hit_rect(r, 1000);
        assert_eq!((hit.x, hit.y, hit.w, hit.h), (0, 0, 44, 44));
        // 4K：80px < 88px 红线 → 中心扩展到 88
        let r4 = Rect { x: 10, y: 10, w: 80, h: 80 };
        let hit4 = touch_hit_rect(r4, 2000);
        assert_eq!((hit4.x, hit4.y, hit4.w, hit4.h), (6, 6, 88, 88));
    }

    #[test]
    fn rounded_corners() {
        let r = Rect { x: 0, y: 0, w: 64, h: 64 };
        assert!(rounded_contains(r, 16, 32, 32));
        assert!(!rounded_contains(r, 16, 0, 0));
        assert!(rounded_contains(r, 16, 16, 16)); // 圆心处
        assert!(!rounded_contains(r, 16, 3, 3));  // 角落圆弧外
    }
}
