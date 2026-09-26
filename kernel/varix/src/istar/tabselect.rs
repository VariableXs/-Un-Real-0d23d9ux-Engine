//! F581 Tab 进框全选 · 完整设计（STAR I 主册 I 域批次八）。
//!
//! **判据（主册）**：Tab 全选触发；打字替换；方向键/点击取消全选；
//! 三处一致；与 F206 焦点环协同。
//!
//! **设计要点（主册）**：
//! - Tab 进入文本输入框时自动全选现有内容（直接打字即替换——表单速填流）；
//! - 方向键/点击则取消全选改光标定位；
//! - 地址栏（F265 Ctrl+L）与搜索框（F416）同语义——三处一致。

use crate::checks::CheckSet;
use crate::istar::ibase::ISTAR_DOMAIN;

use alloc::string::String;

// ---------------------------------------------------------------------------
// 模型
// ---------------------------------------------------------------------------

/// 焦点来源（决定进入框时的初始选择态）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FocusVia {
    /// Tab 进框 → 全选。
    Tab,
    /// 点击 → 光标定位（不全选）。
    Click,
    /// 热键直跳（地址栏 Ctrl+L 同 Tab 语义——三处一致面）。
    Hotkey,
}

/// 选择态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelectState {
    /// 全选（Tab 进框初态）。
    All,
    /// 光标在 offset 位（点击/方向键落点）。
    Caret(usize),
    /// 区间选择（Shift+方向键扩展——判据外延的最小闭环）。
    Range(usize, usize),
}

/// 文本框选择状态机。
pub struct TextField {
    pub text: String,
    pub state: SelectState,
    /// 焦点环显示位（F206 协同：有焦点即有环）。
    pub focused: bool,
}

impl TextField {
    pub fn new(text: &str) -> TextField {
        TextField {
            text: String::from(text),
            state: SelectState::Caret(0),
            focused: false,
        }
    }

    /// 获得焦点。
    pub fn focus(&mut self, via: FocusVia) -> SelectState {
        self.focused = true;
        self.state = match via {
            FocusVia::Tab | FocusVia::Hotkey => {
                if self.text.is_empty() {
                    SelectState::Caret(0)
                } else {
                    SelectState::All
                }
            }
            FocusVia::Click => SelectState::Caret(0),
        };
        self.state
    }

    pub fn blur(&mut self) {
        self.focused = false;
    }

    /// 打字：全选态下替换全部；光标态下插入。
    pub fn type_char(&mut self, c: char) {
        if !self.focused {
            return;
        }
        match self.state {
            SelectState::All => {
                self.text.clear();
                self.text.push(c);
                self.state = SelectState::Caret(1);
            }
            SelectState::Caret(pos) => {
                let pos = pos.min(self.text.len());
                self.text.insert(pos, c);
                self.state = SelectState::Caret(pos + c.len_utf8());
            }
            SelectState::Range(a, b) => {
                let (lo, hi) = (a.min(b), b.max(a));
                let hi = hi.min(self.text.len());
                let mut s = alloc::string::String::new();
                s.push(c);
                self.text.replace_range(lo..hi, &s);
                self.state = SelectState::Caret(lo + 1);
            }
        }
    }

    /// 方向键：取消全选改光标定位（Home/End/左右统一落此口）。
    pub fn arrow_navigate(&mut self, to: usize) {
        let to = to.min(self.text.chars().count());
        self.state = SelectState::Caret(to);
    }

    /// 点击定位：同方向键语义（取消全选）。
    pub fn click_at(&mut self, pos: usize) {
        self.arrow_navigate(pos);
    }

    /// 判定：Tab 全选触发 + 打字替换（判据组合断言）。
    pub fn tab_select_active(&self) -> bool {
        self.focused && self.state == SelectState::All
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub fn run_tabselect_checks() -> CheckSet {
    let mut set = CheckSet::new(ISTAR_DOMAIN);

    // 1. Tab 全选触发：进框即 All；空文本落光标（全选无意义）。
    let mut f = TextField::new("http://varix.local");
    f.focus(FocusVia::Tab);
    let tab_all = f.tab_select_active();
    let mut empty = TextField::new("");
    empty.focus(FocusVia::Tab);
    set.add(
        "tab selects all",
        tab_all && matches!(empty.state, SelectState::Caret(0)),
        "",
    );

    // 2. 打字替换：全选态打字覆盖原文。
    f.type_char('h');
    set.add(
        "typing replaces all",
        f.text == "h" && matches!(f.state, SelectState::Caret(1)),
        "",
    );

    // 3. 方向键/点击取消全选改光标定位。
    let mut f2 = TextField::new("搜索关键词");
    f2.focus(FocusVia::Tab);
    let was_all = f2.tab_select_active();
    f2.arrow_navigate(3);
    let arrow_ok = matches!(f2.state, SelectState::Caret(3)) && was_all;
    f2.click_at(1);
    set.add(
        "arrow and click cancel selection",
        arrow_ok && matches!(f2.state, SelectState::Caret(1)),
        "",
    );

    // 4. 点击进框不全选（Click via → 光标定位）。
    let mut f3 = TextField::new("已有内容");
    f3.focus(FocusVia::Click);
    set.add(
        "click focus places caret",
        f3.focused && matches!(f3.state, SelectState::Caret(0)),
        "",
    );

    // 5. 三处一致：地址栏（Ctrl+L 热键）/搜索框/表单框同一语义源
    //    （Hotkey 与 Tab 同样全选）。
    let mut a = TextField::new("varix.local");
    a.focus(FocusVia::Hotkey);
    let mut s = TextField::new("varix 搜索");
    s.focus(FocusVia::Tab);
    let mut m = TextField::new("用户名");
    m.focus(FocusVia::Tab);
    set.add(
        "three surfaces same semantics",
        a.tab_select_active() && s.tab_select_active() && m.tab_select_active(),
        "",
    );

    // 6. 与 F206 焦点环协同：有焦点即环在（焦点环不因全选消失）。
    f2.blur();
    let blurred = !f2.focused;
    f2.focus(FocusVia::Tab);
    set.add(
        "focus ring follows focus",
        blurred && f2.focused && f2.tab_select_active(),
        "",
    );

    // 7. 光标态打字为插入（非全选路径不误伤原文）。
    let mut f4 = TextField::new("abc");
    f4.focus(FocusVia::Click);
    f4.arrow_navigate(1);
    f4.type_char('X');
    set.add(
        "caret typing inserts",
        f4.text == "aXbc" && matches!(f4.state, SelectState::Caret(2)),
        "",
    );

    // 8. 区间选择打字替换区间（Shift 扩展闭环）。
    let mut f5 = TextField::new("abcdef");
    f5.focus(FocusVia::Click);
    f5.state = SelectState::Range(1, 4);
    f5.type_char('Z');
    set.add(
        "range typing replaces range",
        f5.text == "aZef",
        "",
    );

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typing_without_focus_noop() {
        let mut f = TextField::new("abc");
        f.type_char('X');
        assert_eq!(f.text, "abc");
    }

    #[test]
    fn all_select_on_empty_then_type_inserts() {
        let mut f = TextField::new("");
        f.focus(FocusVia::Tab);
        f.type_char('h');
        f.type_char('i');
        assert_eq!(f.text, "hi");
    }

    #[test]
    fn arrow_clamps_to_length() {
        let mut f = TextField::new("abc");
        f.focus(FocusVia::Click);
        f.arrow_navigate(99);
        assert!(matches!(f.state, SelectState::Caret(3)));
    }
}
