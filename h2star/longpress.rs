//! F280 触屏长按右键 · 完整设计（STAR I 主册 H 域）。
//!
//! **判据（主册）**：长按 500ms±50ms 触发；涟漪反馈与菜单位置判据；
//! 拖拽衔接；44px 命中区审计（关键交互点清单）；双通道（触屏+鼠标）
//! 行为一致性。
//!
//! **设计要点（主册）**：触屏场景长按 500ms=右键（按下即出现视觉涟漪
//! 反馈、松开弹菜单——菜单出现在手指位置上方 48px 防手指遮挡），拖动
//! 长按项可直接拖拽文件；触屏命中目标放大到 44×44px 最小触区。
//!
//! 实装：长按状态机（按下→涟漪即时→500ms 达标→松开弹菜单；提前松开
//! =普通点击、拖动=拖拽衔接）；菜单位置计算（手指位置上方 48px）；
//! 命中区扩展器（视觉尺寸不变、命中区 ≥44px——审计清单直读）；双通道
//! 语义表（同一目标触屏/鼠标动作一致）。

use crate::checks::CheckSet;

use alloc::string::String;
use alloc::vec::Vec;

/// 长按触发时长（ms，判据 500ms±50ms——判定以 500 为准，±50 是工艺容差）。
pub const LONG_PRESS_MS: u64 = 500;
/// 涟漪反馈出现时刻（按下即出——0ms）。
pub const RIPPLE_AT_MS: u64 = 0;
/// 菜单相对手指的上移量（px，防手指遮挡）。
pub const MENU_LIFT_PX: i32 = 48;
/// 最小触区边长（px）。
pub const MIN_HIT_PX: i32 = 44;

/// 长按会话状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PressPhase {
    /// 未开始。
    Idle,
    /// 按下（涟漪已出，计时中）。
    Pressing,
    /// 长按达标（松开即弹菜单）。
    Armed,
    /// 转入拖拽（长按后移动）。
    Dragging,
    /// 菜单已弹。
    MenuShown,
    /// 普通点击（提前松开）。
    Tapped,
}

/// 长按会话。
pub struct LongPress {
    pub phase: PressPhase,
    pub down_x: i32,
    pub down_y: i32,
}

impl LongPress {
    pub fn press(x: i32, y: i32) -> LongPress {
        LongPress { phase: PressPhase::Pressing, down_x: x, down_y: y }
    }

    /// 松开：按住时长决定走向——达标弹菜单（位置=手指上方 48px），
    /// 未达标=普通点击。
    pub fn release(&mut self, held_ms: u64) -> Option<(PressPhase, i32, i32)> {
        match self.phase {
            PressPhase::Pressing => {
                if held_ms >= LONG_PRESS_MS {
                    self.phase = PressPhase::MenuShown;
                    Some((PressPhase::MenuShown, self.down_x, self.down_y - MENU_LIFT_PX))
                } else {
                    self.phase = PressPhase::Tapped;
                    Some((PressPhase::Tapped, self.down_x, self.down_y))
                }
            }
            PressPhase::Armed => {
                self.phase = PressPhase::MenuShown;
                Some((PressPhase::MenuShown, self.down_x, self.down_y - MENU_LIFT_PX))
            }
            _ => None,
        }
    }

    /// 达标后移动 → 转拖拽（拖拽衔接判据：长按项直接拖走）。
    pub fn begin_drag(&mut self) -> bool {
        if self.phase == PressPhase::Armed || self.phase == PressPhase::Pressing {
            self.phase = PressPhase::Dragging;
            true
        } else {
            false
        }
    }

    /// 计时推进（held_ms 注入——达标置 Armed，涟漪已在按下时刻出现）。
    pub fn tick(&mut self, held_ms: u64) -> bool {
        if self.phase == PressPhase::Pressing && held_ms >= LONG_PRESS_MS {
            self.phase = PressPhase::Armed;
            true
        } else {
            false
        }
    }
}

/// 菜单位置：手指位置上方 48px（防遮挡判据的唯一计算点）。
pub fn menu_anchor(finger_x: i32, finger_y: i32) -> (i32, i32) {
    (finger_x, finger_y - MENU_LIFT_PX)
}

/// 命中区扩展：视觉尺寸 (w,h) 不变，命中区扩到 ≥44×44px。
/// 返回命中区宽高（审计清单直读——扩展只在命中层，不改视觉布局）。
pub fn hit_box(visual_w: i32, visual_h: i32) -> (i32, i32) {
    (visual_w.max(MIN_HIT_PX), visual_h.max(MIN_HIT_PX))
}

/// 双通道语义表：同一目标在触屏/鼠标两通道路由到同一动作。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChannelAction {
    OpenMenu,
    Activate,
    StartDrag,
}

pub fn resolve_action(touch: Option<ChannelAction>, mouse: Option<ChannelAction>) -> bool {
    match (touch, mouse) {
        (Some(t), Some(m)) => t == m,
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub fn run_longpress_checks() -> CheckSet {
    let mut set = CheckSet::new("h2-F280");
    // 长按 500ms±50ms：450ms 不触发、500ms 触发、550ms 触发。
    let mut lp = LongPress::press(100, 200);
    let early = lp.release(450);
    set.add(
        "F280 450 not enough",
        early.unwrap().0 == PressPhase::Tapped,
        "too short = tap",
    );
    let mut lp2 = LongPress::press(100, 200);
    let armed = lp2.tick(500);
    let fired = lp2.release(510);
    set.add(
        "F280 500 fires",
        armed && fired.unwrap().0 == PressPhase::MenuShown,
        "500ms±50",
    );
    // 涟漪即时：按下时刻即 Pressing（涟漪渲染层读 Pressing 态出涟漪，
    // RIPPLE_AT_MS=0 为常量钉死）。
    let lp3 = LongPress::press(0, 0);
    set.add(
        "F280 ripple instant",
        lp3.phase == PressPhase::Pressing && RIPPLE_AT_MS == 0,
        "feedback on down",
    );
    // 菜单位置：手指上方 48px。
    let anchor = menu_anchor(320, 640);
    set.add(
        "F280 menu above finger",
        anchor == (320, 640 - MENU_LIFT_PX) && MENU_LIFT_PX == 48,
        "anti-occlusion",
    );
    // 拖拽衔接：长按达标后移动 → 拖拽。
    let mut lp4 = LongPress::press(50, 50);
    let _ = lp4.tick(500);
    let drag_ok = lp4.begin_drag();
    set.add("F280 drag handoff", drag_ok && lp4.phase == PressPhase::Dragging, "armed→drag");
    // 44px 命中区审计：小目标扩到 44，大目标不动。
    let (w1, h1) = hit_box(24, 24);
    let (w2, h2) = hit_box(60, 30);
    set.add(
        "F280 44px hit",
        w1 == MIN_HIT_PX && h1 == MIN_HIT_PX && w2 == 60 && h2 == 44,
        "expand only",
    );
    // 双通道一致性。
    set.add(
        "F280 dual channel",
        resolve_action(Some(ChannelAction::OpenMenu), Some(ChannelAction::OpenMenu))
            && !resolve_action(Some(ChannelAction::Activate), Some(ChannelAction::OpenMenu)),
        "same semantics",
    );
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f280_press_flow() {
        let set = run_longpress_checks();
        let (p, f) = set.tally();
        assert!(set.all_passed(), "F280 自检红 {f}/{p}");
    }

    #[test]
    fn armed_release_still_menu() {
        // Armed 态松手即使超时也弹菜单（状态机无死角）。
        let mut lp = LongPress::press(1, 1);
        let _ = lp.tick(500);
        let out = lp.release(2000).unwrap();
        assert_eq!(out.0, PressPhase::MenuShown);
    }
}
