//! F434 对话框控件键位 · 完整设计（STAR I 主册 G-I-34）。
//!
//! **判据（主册）**：四键行为矩阵（三类控件×四键）；焦点陷阱联动；热键
//! 功能性（虽无下划线仍生效）；默认按钮判定（F207 判据复用）。＋通12。
//!
//! 设计：对话框键盘核——四键 = Tab/空格/方向键/Enter（Esc 走 F207 取消
//! 语义转发）；三类控件（按钮/复选/单选组）×四键行为矩阵；焦点陷阱
//! （Tab 循环不出对话框）；按钮热键字母（无下划线仍生效）；默认按钮
//! 判定（F207：可安全默认者才默认）。v6 深化：单选组隔离修正（两组
//! 单选互不越界——互斥只在组内生效）、方向键组端停住（不环绕到别的
//! 组）、空格选中单选同样组内互斥、禁用控件 Tab/Shift+Tab 双向跳过
//! （焦点永不卡死——除非全员禁用）、禁用控件热键不响应。

use crate::checks::CheckSet;

use alloc::vec::Vec;

/// 控件类型。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CtrlKind {
    Button,
    Checkbox,
    RadioInGroup,
}

/// 控件。
#[derive(Clone, Debug)]
pub struct DlgCtrl {
    pub kind: CtrlKind,
    pub label: &'static str,
    /// 按钮热键字母（0 = 无）。
    pub hotkey: u8,
    pub checked: bool,
    pub is_default: bool,
    /// 禁用位（v6：Tab 跳过 + 热键不响应）。
    pub disabled: bool,
}

/// 四键。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DlgKey {
    Tab,
    ShiftTab,
    Space,
    Enter,
    Arrow(i32),
}

/// 对话框键盘核（焦点陷阱内）。
pub struct DlgKeys {
    pub ctrls: Vec<DlgCtrl>,
    pub focus: usize,
}

impl DlgKeys {
    pub fn new(ctrls: Vec<DlgCtrl>) -> DlgKeys {
        let mut d = DlgKeys { ctrls, focus: 0 };
        d.trap_check();
        d
    }

    /// 焦点陷阱：Tab/Shift+Tab 循环（永不出对话框）。禁用控件跳过；
    /// 全员禁用时焦点原地不动（不 panic、不越界）。
    fn trap_check(&mut self) {
        if self.focus >= self.ctrls.len() {
            self.focus = 0;
        }
    }

    fn focusable(&self, i: usize) -> bool {
        !self.ctrls[i].disabled
    }

    pub fn tab(&mut self, shift: bool) {
        let n = self.ctrls.len();
        if n == 0 {
            return;
        }
        let step: i32 = if shift { -1 } else { 1 };
        let mut i = self.focus as i32;
        for _ in 0..n {
            i = (i + step).rem_euclid(n as i32);
            if self.focusable(i as usize) {
                self.focus = i as usize;
                return;
            }
        }
        // 全员禁用：焦点原地（诚实不假装移动）。
    }

    /// 空格：按下按钮（触发）/勾选复选（翻转）/单选组内选中（互斥——
    /// v6：同组其它项取消选中）。禁用控件一律不响应。
    pub fn space(&mut self) -> Option<&'static str> {
        if self.ctrls[self.focus].disabled {
            return None;
        }
        let n = self.ctrls.len();
        match self.ctrls[self.focus].kind {
            CtrlKind::Button => Some(self.ctrls[self.focus].label),
            CtrlKind::Checkbox => {
                let c = &mut self.ctrls[self.focus];
                c.checked = !c.checked;
                Some(c.label)
            }
            CtrlKind::RadioInGroup => {
                // 组边界：以 focus 为锚的连续块（v6 组隔离）。
                let mut lo = self.focus;
                while lo > 0 && self.ctrls[lo - 1].kind == CtrlKind::RadioInGroup {
                    lo -= 1;
                }
                let mut hi = self.focus;
                while hi + 1 < n && self.ctrls[hi + 1].kind == CtrlKind::RadioInGroup {
                    hi += 1;
                }
                for j in lo..=hi {
                    self.ctrls[j].checked = j == self.focus;
                }
                Some(self.ctrls[self.focus].label)
            }
        }
    }

    /// 方向键：单选组内切换（v6 修正——只在 focus 所属的连续组内移动，
    /// 组端停住不环绕；组与组之间互不越界）。其他控件忽略。
    pub fn arrow(&mut self, delta: i32) -> bool {
        let n = self.ctrls.len();
        if n == 0 || self.ctrls[self.focus].kind != CtrlKind::RadioInGroup {
            return false;
        }
        let mut lo = self.focus;
        while lo > 0 && self.ctrls[lo - 1].kind == CtrlKind::RadioInGroup {
            lo -= 1;
        }
        let mut hi = self.focus;
        while hi + 1 < n && self.ctrls[hi + 1].kind == CtrlKind::RadioInGroup {
            hi += 1;
        }
        let target = self.focus as i32 + delta;
        if target < lo as i32 || target > hi as i32 {
            return false; // 组端点：停住（不环绕到别的组）。
        }
        let t = target as usize;
        for j in lo..=hi {
            self.ctrls[j].checked = j == t;
        }
        self.focus = t;
        true
    }

    /// Enter：默认按钮（F207 复用：无默认 → 无动作；禁用的默认钮 →
    /// 无动作——禁用优先于默认，v6）。
    pub fn enter(&mut self) -> Option<&'static str> {
        self.ctrls
            .iter()
            .find(|c| c.is_default && c.kind == CtrlKind::Button && !c.disabled)
            .map(|c| c.label)
    }

    /// 热键字母直达（无下划线仍生效）：禁用控件不响应（v6）。
    pub fn hotkey_press(&mut self, ch: u8) -> Option<&'static str> {
        let idx = self
            .ctrls
            .iter()
            .position(|c| c.hotkey == ch && c.kind == CtrlKind::Button && !c.disabled)?;
        self.focus = idx;
        Some(self.ctrls[idx].label)
    }
}

pub fn run_dlgkeys_checks() -> CheckSet {
    let mut set = CheckSet::new("uni1-F434");
    let ctrls = alloc::vec![
        DlgCtrl { kind: CtrlKind::Checkbox, label: "记住选择", hotkey: 0, checked: false, is_default: false, disabled: false },
        DlgCtrl { kind: CtrlKind::RadioInGroup, label: "每天", hotkey: 0, checked: true, is_default: false, disabled: false },
        DlgCtrl { kind: CtrlKind::RadioInGroup, label: "每周", hotkey: 0, checked: false, is_default: false, disabled: false },
        DlgCtrl { kind: CtrlKind::RadioInGroup, label: "每月", hotkey: 0, checked: false, is_default: false, disabled: false },
        DlgCtrl { kind: CtrlKind::Button, label: "确定", hotkey: b'O', checked: false, is_default: true, disabled: false },
        DlgCtrl { kind: CtrlKind::Button, label: "取消", hotkey: b'C', checked: false, is_default: false, disabled: false },
    ];
    let mut d = DlgKeys::new(ctrls);
    // 四键矩阵 × 复选。
    set.add("f434-tab-cycle", { d.tab(false); d.focus == 1 && { d.tab(true); d.focus == 0 } }, "");
    set.add(
        "f434-checkbox-space",
        { d.space(); d.ctrls[0].checked },
        "",
    );
    set.add("f434-checkbox-untoggle", { d.space(); !d.ctrls[0].checked }, "");
    // 单选组：方向键切换 + 组内互斥。
    d.tab(false); // → 1
    set.add("f434-radio-arrow", d.arrow(1) && d.ctrls[2].checked && !d.ctrls[1].checked, "");
    set.add("f434-radio-arrow-back", d.arrow(-1) && d.ctrls[1].checked && !d.ctrls[2].checked, "");
    // 方向键对复选/按钮无效。
    d.tab(true);
    set.add("f434-arrow-ignored-on-checkbox", !d.arrow(1), "");
    // 组端停住（v6）：从「每天」向左是组头——再向左无动作（不环绕）。
    d.tab(false); // → 1 每天（组头）
    set.add("f434-radio-group-head-stop", !d.arrow(-1) && d.focus == 1, "");
    d.arrow(2); // → 3 每月（组尾）
    set.add("f434-radio-group-tail-stop", !d.arrow(1) && d.focus == 3, "");
    // 两组隔离（v6）：第二组（隔一个按钮）的互斥不影响第一组。
    let two = alloc::vec![
        DlgCtrl { kind: CtrlKind::RadioInGroup, label: "甲", hotkey: 0, checked: true, is_default: false, disabled: false },
        DlgCtrl { kind: CtrlKind::RadioInGroup, label: "乙", hotkey: 0, checked: false, is_default: false, disabled: false },
        DlgCtrl { kind: CtrlKind::Button, label: "分隔", hotkey: 0, checked: false, is_default: false, disabled: false },
        DlgCtrl { kind: CtrlKind::RadioInGroup, label: "丙", hotkey: 0, checked: true, is_default: false, disabled: false },
        DlgCtrl { kind: CtrlKind::RadioInGroup, label: "丁", hotkey: 0, checked: false, is_default: false, disabled: false },
    ];
    let mut g = DlgKeys::new(two);
    g.focus = 3; // 第二组头
    set.add(
        "f434-two-groups-isolated",
        g.arrow(1) && g.ctrls[4].checked && !g.ctrls[3].checked
            && g.ctrls[0].checked && !g.ctrls[1].checked,
        "第一组选中未被波及",
    );
    set.add("f434-no-cross-group", !g.arrow(1), "组尾停住——不越到按钮/别的组");
    // 空格选单选：组内互斥（v6 补全——与方向键同规矩）。
    g.focus = 3;
    let _ = g.space();
    set.add(
        "f434-space-radio-exclusive",
        g.ctrls[3].checked && !g.ctrls[4].checked && g.ctrls[0].checked,
        "",
    );
    // Enter 默认按钮。
    d.focus = 0;
    set.add("f434-enter-default", d.enter() == Some("确定"), "");
    // 热键（无下划线仍生效）。
    set.add(
        "f434-hotkey-func",
        d.hotkey_press(b'C') == Some("取消") && d.focus == 5,
        "",
    );
    set.add("f434-hotkey-miss", d.hotkey_press(b'Z').is_none(), "");
    // 禁用控件（v6）：Tab 双向跳过、热键不响应、默认钮禁用 → Enter 无动作。
    let dis = alloc::vec![
        DlgCtrl { kind: CtrlKind::Checkbox, label: "可勾", hotkey: 0, checked: false, is_default: false, disabled: false },
        DlgCtrl { kind: CtrlKind::Button, label: "禁用钮", hotkey: b'X', checked: false, is_default: true, disabled: true },
        DlgCtrl { kind: CtrlKind::Button, label: "备用钮", hotkey: b'Y', checked: false, is_default: false, disabled: false },
    ];
    let mut q = DlgKeys::new(dis);
    q.focus = 0;
    q.tab(false); // 1 禁用 → 跳到 2
    set.add("f434-disabled-skip-tab", q.focus == 2, "");
    q.tab(true); // 2 → 1 禁用跳过 → 0
    set.add("f434-disabled-skip-shift-tab", q.focus == 0, "");
    set.add(
        "f434-disabled-hotkey-silent",
        q.hotkey_press(b'X').is_none() && q.focus == 0,
        "",
    );
    set.add("f434-disabled-default-no-enter", q.enter().is_none(), "");
    set.add("f434-enabled-hotkey-still-works", q.hotkey_press(b'Y') == Some("备用钮"), "");
    // 焦点陷阱：末尾 Tab 回头。
    d.focus = 5;
    d.tab(false);
    set.add("f434-trap-wrap", d.focus == 0, "");
    d.focus = 0;
    d.tab(true);
    set.add("f434-trap-wrap-back", d.focus == 5, "");
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_default_button_noop() {
        let mut d = DlgKeys::new(alloc::vec![
            DlgCtrl { kind: CtrlKind::Button, label: "好", hotkey: 0, checked: false, is_default: false, disabled: false },
        ]);
        // F207 复用：没有可安全默认者 → Enter 无动作。
        let mut d2 = DlgKeys::new(alloc::vec![
            DlgCtrl { kind: CtrlKind::Button, label: "好", hotkey: 0, checked: false, is_default: false, disabled: false },
        ]);
        assert!(d.enter().is_none() && d2.enter().is_none());
    }

    #[test]
    fn all_disabled_focus_stays() {
        let mut d = DlgKeys::new(alloc::vec![
            DlgCtrl { kind: CtrlKind::Button, label: "a", hotkey: 0, checked: false, is_default: false, disabled: true },
            DlgCtrl { kind: CtrlKind::Button, label: "b", hotkey: 0, checked: false, is_default: false, disabled: true },
        ]);
        d.tab(false);
        assert_eq!(d.focus, 0, "全员禁用：焦点原地不假装移动");
        assert!(d.space().is_none(), "禁用控件空格不响应");
    }

    #[test]
    fn radio_space_does_not_cross_group() {
        let mut d = DlgKeys::new(alloc::vec![
            DlgCtrl { kind: CtrlKind::RadioInGroup, label: "一", hotkey: 0, checked: true, is_default: false, disabled: false },
            DlgCtrl { kind: CtrlKind::RadioInGroup, label: "二", hotkey: 0, checked: false, is_default: false, disabled: false },
            DlgCtrl { kind: CtrlKind::Checkbox, label: "隔", hotkey: 0, checked: false, is_default: false, disabled: false },
            DlgCtrl { kind: CtrlKind::RadioInGroup, label: "三", hotkey: 0, checked: false, is_default: false, disabled: false },
        ]);
        d.focus = 3;
        let _ = d.space();
        assert!(d.ctrls[3].checked);
        assert!(d.ctrls[0].checked, "第一组互斥不被第二组选中破坏");
    }
}
