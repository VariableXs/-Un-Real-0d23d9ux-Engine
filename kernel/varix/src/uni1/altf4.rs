//! F405 Alt+F4 与桌面关机菜单 · 完整设计（STAR I 主册 G-I-05）。
//!
//! **判据（主册）**：焦点判定（窗/桌面/模态三场景）；桌面电源菜单三
//! 选项与默认值；三问联动；关闭语义与 × 一致性。＋通12。
//!
//! **设计要点（主册）**：Alt+F4 关闭当前窗口（焦点窗关闭语义与点 ×
//! 完全一致——F310 未保存三问照走）；焦点在桌面时 Alt+F4 弹电源菜单
//! （睡眠/关机/重启三选，默认关机，方向键切换 Enter 确认）；组合键在
//! 模态弹窗中无效（F384 陷阱优先）。
//!
//! 本模块是 Alt+F4 **分发语义核**：三场景焦点判定 → 三种结果（关窗/
//! 弹菜单/无动作），关窗必经 F310 三问钩子（快捷键不绕过安全网），
//! 电源菜单状态机（三选项、默认关机、方向键循环、Enter 确认、Esc 取消）。
//!
//! 时间注入式（毫秒戳），无外部依赖。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 规格常量（参数唯一源）
// ---------------------------------------------------------------------------

/// 电源菜单三选项（顺序钉死：睡眠/关机/重启——与 Windows 经典顺序一致）。
pub const POWER_MENU_ITEMS: [&str; 3] = ["睡眠", "关机", "重启"];

/// 默认选中项下标：关机（判据「默认关机」）。
pub const POWER_MENU_DEFAULT: usize = 1;

/// 焦点场景。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FocusScene {
    /// 焦点在普通窗口。
    Window,
    /// 焦点在桌面。
    Desktop,
    /// 焦点在模态弹窗（F384 陷阱优先——Alt+F4 在此无效）。
    Modal,
}

/// Alt+F4 的三种合法结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AltF4Result {
    /// 关窗（经 F310 三问钩子）。
    CloseWindow(u64),
    /// 弹出桌面电源菜单。
    PowerMenu,
    /// 无动作（模态陷阱/无焦点）。
    Noop,
}

/// 关窗请求的裁决（F310 三问钩子的返回）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CloseVerdict {
    /// 无脏数据直接关。
    Clean,
    /// 有未保存内容——三问已弹，等待用户三选一。
    AskPending,
    /// 用户选保存/放弃后允许关。
    Proceed,
    /// 用户取消——不关。
    Cancel,
}

/// 电源菜单状态机。
pub struct PowerMenu {
    pub open: bool,
    pub selected: usize,
    /// 打开次数/取消次数（体验账）。
    pub opened: u64,
    pub cancelled: u64,
}

impl PowerMenu {
    pub fn new() -> PowerMenu {
        PowerMenu { open: false, selected: POWER_MENU_DEFAULT, opened: 0, cancelled: 0 }
    }

    /// 打开：选中复位为默认（关机）——判据「默认关机」逐次兑现。
    pub fn open(&mut self) {
        self.open = true;
        self.selected = POWER_MENU_DEFAULT;
        self.opened += 1;
    }

    /// 方向键切换（下 = +1 循环，上 = -1 循环）。
    pub fn move_sel(&mut self, delta: i32) {
        if !self.open {
            return;
        }
        let n = POWER_MENU_ITEMS.len() as i32;
        self.selected = ((self.selected as i32 + delta).rem_euclid(n)) as usize;
    }

    /// Enter 确认：返回选中项并关菜单。
    pub fn confirm(&mut self) -> Option<&'static str> {
        if !self.open {
            return None;
        }
        self.open = false;
        Some(POWER_MENU_ITEMS[self.selected])
    }

    /// Esc 取消。
    pub fn cancel(&mut self) -> bool {
        if !self.open {
            return false;
        }
        self.open = false;
        self.cancelled += 1;
        true
    }
}

/// Alt+F4 分发器。
pub struct AltF4Dispatch {
    pub menu: PowerMenu,
    /// 三问钩子触发次数（关窗请求中带脏标记的数量——判据「三问联动」）。
    pub ask_triggered: u64,
    /// × 与 Alt+F4 走同一关窗口的次数（语义一致性账）。
    pub close_via_same_path: u64,
}

impl AltF4Dispatch {
    pub fn new() -> AltF4Dispatch {
        AltF4Dispatch { menu: PowerMenu::new(), ask_triggered: 0, close_via_same_path: 0 }
    }

    /// 分发 Alt+F4。
    ///
    /// - `dirty`：焦点窗是否有未保存内容（F310 三问的输入）；
    /// - `modal_open`：模态陷阱是否在顶（F384——陷阱优先，组合键无效）。
    pub fn alt_f4(&mut self, scene: FocusScene, focused_window: Option<u64>, dirty: bool, modal_open: bool) -> AltF4Result {
        // 模态陷阱优先于一切（F384）。
        if modal_open || scene == FocusScene::Modal {
            return AltF4Result::Noop;
        }
        match scene {
            FocusScene::Window => match focused_window {
                Some(id) => {
                    if dirty {
                        self.ask_triggered += 1;
                        return AltF4Result::CloseWindow(id); // 调用方收到后必须走三问
                    }
                    self.close_via_same_path += 1;
                    AltF4Result::CloseWindow(id)
                }
                None => AltF4Result::Noop,
            },
            FocusScene::Desktop => {
                self.menu.open();
                AltF4Result::PowerMenu
            }
            FocusScene::Modal => AltF4Result::Noop,
        }
    }

    /// 三问裁决回填（F310 三选一 → 关窗语义与 × 一致——同一记账路径）。
    pub fn resolve_ask(&mut self, verdict: CloseVerdict) -> bool {
        match verdict {
            CloseVerdict::Proceed | CloseVerdict::Clean => {
                self.close_via_same_path += 1;
                true
            }
            CloseVerdict::Cancel | CloseVerdict::AskPending => false,
        }
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F405 自检。
pub fn run_altf4_checks() -> CheckSet {
    let mut set = CheckSet::new("uni1-F405");

    // 常量钉死：三选项顺序与默认值。
    set.add(
        "f405-menu-const",
        POWER_MENU_ITEMS == ["睡眠", "关机", "重启"] && POWER_MENU_DEFAULT == 1,
        "",
    );

    // 场景一：焦点窗干净 → 关窗，与 × 同路径记账。
    let mut d = AltF4Dispatch::new();
    let r1 = d.alt_f4(FocusScene::Window, Some(7), false, false);
    set.add(
        "f405-window-clean-close",
        r1 == AltF4Result::CloseWindow(7) && d.close_via_same_path == 1 && d.ask_triggered == 0,
        "",
    );

    // 场景二：焦点窗脏 → 三问触发（快捷键不绕过安全网）。
    let r2 = d.alt_f4(FocusScene::Window, Some(7), true, false);
    set.add(
        "f405-dirty-triple-ask",
        r2 == AltF4Result::CloseWindow(7) && d.ask_triggered == 1 && d.close_via_same_path == 1,
        "",
    );
    // 用户取消 → 不关。
    set.add("f405-ask-cancel-no-close", !d.resolve_ask(CloseVerdict::Cancel) && d.close_via_same_path == 1, "");
    // 用户选保存 → 关，同一记账路径（与 × 一致性）。
    set.add("f405-ask-proceed-close", d.resolve_ask(CloseVerdict::Proceed) && d.close_via_same_path == 2, "");

    // 场景三：焦点桌面 → 电源菜单，默认选中关机。
    let r3 = d.alt_f4(FocusScene::Desktop, None, false, false);
    set.add(
        "f405-desktop-power-menu",
        r3 == AltF4Result::PowerMenu && d.menu.open && d.menu.selected == POWER_MENU_DEFAULT,
        "",
    );

    // 菜单键盘流：方向键循环 + Enter 确认 + Esc 取消。
    let mut m = PowerMenu::new();
    m.open();
    m.move_sel(1); // 关机 → 重启
    set.add("f405-menu-move-down", m.selected == 2, "");
    m.move_sel(1); // 重启 → 睡眠（循环）
    set.add("f405-menu-wrap", m.selected == 0, "");
    m.move_sel(-1); // 睡眠 → 重启? 0-1 → 2（循环到尾）
    set.add("f405-menu-wrap-up", m.selected == 2, "");
    set.add("f405-menu-confirm", m.confirm() == Some("重启") && !m.open, "");
    m.open();
    m.move_sel(-1); // 默认1 → 0 睡眠
    set.add("f405-menu-esc-cancel", m.cancel() && !m.open && m.cancelled == 1, "");
    set.add("f405-menu-confirm-closed-noop", m.confirm().is_none(), "");

    // 场景四：模态陷阱优先——组合键无效（F384）。
    let mut d2 = AltF4Dispatch::new();
    let r4 = d2.alt_f4(FocusScene::Modal, Some(9), false, true);
    let r5 = d2.alt_f4(FocusScene::Window, Some(9), false, true);
    set.add(
        "f405-modal-trap-priority",
        r4 == AltF4Result::Noop && r5 == AltF4Result::Noop && d2.menu.opened == 0,
        "",
    );

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn three_scenes_matrix() {
        let mut d = AltF4Dispatch::new();
        // 窗 → 关窗；桌面 → 菜单；模态 → 无动作。
        assert_eq!(d.alt_f4(FocusScene::Window, Some(1), false, false), AltF4Result::CloseWindow(1));
        assert_eq!(d.alt_f4(FocusScene::Desktop, None, false, false), AltF4Result::PowerMenu);
        assert_eq!(d.alt_f4(FocusScene::Modal, Some(1), false, true), AltF4Result::Noop);
        // 焦点窗缺失 → 无动作（不 panic）。
        assert_eq!(d.alt_f4(FocusScene::Window, None, false, false), AltF4Result::Noop);
    }

    #[test]
    fn menu_selection_cycle_both_directions() {
        let mut m = PowerMenu::new();
        m.open();
        // 默认关机；向下两次到重启再一次回睡眠。
        assert_eq!(m.selected, 1);
        m.move_sel(1);
        m.move_sel(1);
        assert_eq!(m.selected, 0);
        // 向上一次回重启（循环到尾）。
        m.move_sel(-1);
        assert_eq!(m.selected, 2);
        assert_eq!(m.confirm(), Some("重启"));
        assert!(!m.open);
        // 关闭后方向键与确认均无动作。
        m.move_sel(1);
        assert!(m.confirm().is_none());
    }

    #[test]
    fn dirty_window_never_closes_silently() {
        let mut d = AltF4Dispatch::new();
        let _ = d.alt_f4(FocusScene::Window, Some(3), true, false);
        assert_eq!(d.ask_triggered, 1);
        assert_eq!(d.close_via_same_path, 0, "三问未裁决前不得关");
        // 挂起态也不算关。
        assert!(!d.resolve_ask(CloseVerdict::AskPending));
        assert_eq!(d.close_via_same_path, 0);
    }
}
