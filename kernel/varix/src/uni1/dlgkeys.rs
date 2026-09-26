//! F434 对话框控件键位 · 完整设计（STAR I 主册 G-I-34）。
//!
//! **判据（主册）**：四键行为矩阵（三类控件×四键）；焦点陷阱联动；热键
//! 功能性（虽无下划线仍生效）；默认按钮判定（F207 判据复用）。＋通12。
//!
//! 设计：对话框键盘核——四键 = Tab/空格/方向键/Enter（Esc 走 F207 取消
//! 语义转发）；三类控件（按钮/复选/单选组）×四键行为矩阵；焦点陷阱
//! （Tab 循环不出对话框）；按钮热键字母（无下划线仍生效）；默认按钮
//! 判定（F207：可安全默认者才默认）。

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

    /// 焦点陷阱：Tab/Shift+Tab 循环（永不出对话框）。
    fn trap_check(&mut self) {
        if self.focus >= self.ctrls.len() {
            self.focus = 0;
        }
    }

    pub fn tab(&mut self, shift: bool) {
        let n = self.ctrls.len();
        if n == 0 {
            return;
        }
        self.focus = if shift {
            (self.focus + n - 1) % n
        } else {
            (self.focus + 1) % n
        };
    }

    /// 空格：按下按钮（触发）/勾选复选（翻转）/单选组内选中。
    pub fn space(&mut self) -> Option<&'static str> {
        let c = &mut self.ctrls[self.focus];
        match c.kind {
            CtrlKind::Button => Some(c.label),
            CtrlKind::Checkbox => {
                c.checked = !c.checked;
                Some(c.label)
            }
            CtrlKind::RadioInGroup => {
                c.checked = true;
                Some(c.label)
            }
        }
    }

    /// 方向键：单选组内切换（把选中移到组内相邻项）；其他控件忽略。
    pub fn arrow(&mut self, delta: i32) -> bool {
        let n = self.ctrls.len();
        let cur_kind = self.ctrls[self.focus].kind;
        if cur_kind != CtrlKind::RadioInGroup {
            return false;
        }
        // 组内相邻（同 kind 的相邻块内移动——本核以「连续 RadioInGroup
        // 块」为一组）。
        let mut i = self.focus as i32;
        loop {
            i = (i + delta).rem_euclid(n as i32);
            if self.ctrls[i as usize].kind == CtrlKind::RadioInGroup {
                // 组边界：跨出连续块则停在本组端点。
                let step_back = (i - delta).rem_euclid(n as i32) as usize;
                if step_back != self.focus
                    && self.ctrls[step_back].kind == CtrlKind::RadioInGroup
                    && !self.adjacent(step_back, i as usize)
                {
                    break;
                }
                // 取消组内其它选中，选中此项。
                for (j, c) in self.ctrls.iter_mut().enumerate() {
                    if c.kind == CtrlKind::RadioInGroup {
                        c.checked = j == i as usize;
                    }
                }
                self.focus = i as usize;
                return true;
            }
        }
        false
    }

    fn adjacent(&self, a: usize, b: usize) -> bool {
        let n = self.ctrls.len();
        (a + n - 1) % n == b || (a + 1) % n == b
    }

    /// Enter：默认按钮（F207 复用：无默认 → 无动作）。
    pub fn enter(&mut self) -> Option<&'static str> {
        self.ctrls
            .iter()
            .find(|c| c.is_default && c.kind == CtrlKind::Button)
            .map(|c| c.label)
    }

    /// 热键字母直达（无下划线仍生效）：Alt 无关——按字母触发。
    pub fn hotkey_press(&mut self, ch: u8) -> Option<&'static str> {
        let idx = self.ctrls.iter().position(|c| c.hotkey == ch && c.kind == CtrlKind::Button)?;
        self.focus = idx;
        Some(self.ctrls[idx].label)
    }
}

pub fn run_dlgkeys_checks() -> CheckSet {
    let mut set = CheckSet::new("uni1-F434");
    let ctrls = alloc::vec![
        DlgCtrl { kind: CtrlKind::Checkbox, label: "记住选择", hotkey: 0, checked: false, is_default: false },
        DlgCtrl { kind: CtrlKind::RadioInGroup, label: "每天", hotkey: 0, checked: true, is_default: false },
        DlgCtrl { kind: CtrlKind::RadioInGroup, label: "每周", hotkey: 0, checked: false, is_default: false },
        DlgCtrl { kind: CtrlKind::RadioInGroup, label: "每月", hotkey: 0, checked: false, is_default: false },
        DlgCtrl { kind: CtrlKind::Button, label: "确定", hotkey: b'O', checked: false, is_default: true },
        DlgCtrl { kind: CtrlKind::Button, label: "取消", hotkey: b'C', checked: false, is_default: false },
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
    // Enter 默认按钮。
    set.add("f434-enter-default", d.enter() == Some("确定"), "");
    // 热键（无下划线仍生效）。
    set.add(
        "f434-hotkey-func",
        d.hotkey_press(b'C') == Some("取消") && d.focus == 5,
        "",
    );
    set.add("f434-hotkey-miss", d.hotkey_press(b'Z').is_none(), "");
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
            DlgCtrl { kind: CtrlKind::Button, label: "好", hotkey: 0, checked: false, is_default: false },
        ]);
        // F207 复用：没有可安全默认者 → Enter 无动作。
        let mut d2 = DlgKeys::new(alloc::vec![
            DlgCtrl { kind: CtrlKind::Button, label: "好", hotkey: 0, checked: false, is_default: false },
        ]);
        assert!(d.enter().is_none() && d2.enter().is_none());
    }
}
