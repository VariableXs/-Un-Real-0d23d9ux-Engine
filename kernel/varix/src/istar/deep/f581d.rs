//! 深化层 · F581 Tab 进框全选（回炉补深主册【设计要点】未实装机制）。
//!
//! 补深三条（判据唯一源：主册 F581 节）：
//! ①「全选生命周期状态机」——Tab 进入→全选态；打字=替换并退出全选
//!   态；方向键=取消全选改定位；点击=按点击位置定位；Shift+Tab 回向
//!   进入同语义——事件→迁移全链计账（基础件只有单步语义，无状态机）；
//! ②「三框一致性对账」——表单框/地址栏（F265 Ctrl+L）/搜索框（F416）
//!   跑同一事件脚本得同一终态（同一状态机实例的语义分派）；
//! ③「F206 焦点环协同」——全选态下焦点环仍显示，失焦才收环。

use crate::checks::CheckSet;
use crate::istar::ibase::ISTAR_DOMAIN;
use crate::istar::tabselect::{FocusVia, SelectState, TextField};

// ---------------------------------------------------------------------------
// 全选生命周期状态机
// ---------------------------------------------------------------------------

/// 选择生命周期事件（状态机唯一入口）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelEvent {
    /// Tab 进框。
    TabIn,
    /// Shift+Tab 回向进框（与 Tab 同语义）。
    ShiftTabIn,
    /// 热键直跳（地址栏 Ctrl+L 同语义）。
    HotkeyIn,
    /// 点击进框/框内点击（按位定位）。
    ClickAt(usize),
    /// 打字。
    Type(char),
    /// 方向键定位（左右/Home/End 统一落点）。
    Arrow(usize),
    /// 失焦。
    Blur,
}

/// 事件结果（三框一致性对账的对账面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelOutcome {
    /// 进入全选态。
    SelectAll,
    /// 全选被替换（打字速填流）。
    Replace,
    /// 光标定位（取消全选或直落）。
    CaretMove,
    /// 无焦点忽略。
    Ignored,
}

/// 全选生命周期状态机（包住基础件 TextField——不重写选择/替换本体）。
pub struct SelectLifecycle {
    pub field: TextField,
    ring_on: bool,
    all_entries: u32,
    replaces: u32,
    cancels: u32,
}

impl SelectLifecycle {
    pub fn new(text: &str) -> SelectLifecycle {
        SelectLifecycle {
            field: TextField::new(text),
            ring_on: false,
            all_entries: 0,
            replaces: 0,
            cancels: 0,
        }
    }

    pub fn apply(&mut self, ev: SelEvent) -> SelOutcome {
        match ev {
            SelEvent::TabIn | SelEvent::ShiftTabIn | SelEvent::HotkeyIn => {
                // 三入口同语义（回向/热键统一走基础件 Tab 口）。
                let _ = self.field.focus(FocusVia::Tab);
                self.ring_on = true;
                if self.field.state == SelectState::All {
                    self.all_entries += 1;
                    SelOutcome::SelectAll
                } else {
                    SelOutcome::CaretMove // 空文本诚实落光标
                }
            }
            SelEvent::ClickAt(pos) => {
                if self.field.state == SelectState::All {
                    self.cancels += 1; // 全选态点击 = 取消改定位
                }
                if !self.field.focused {
                    let _ = self.field.focus(FocusVia::Click);
                    self.ring_on = true;
                }
                self.field.click_at(pos);
                SelOutcome::CaretMove
            }
            SelEvent::Type(c) => {
                if !self.field.focused {
                    return SelOutcome::Ignored;
                }
                let replacing = self.field.state == SelectState::All;
                self.field.type_char(c);
                if replacing {
                    self.replaces += 1;
                    SelOutcome::Replace
                } else {
                    SelOutcome::CaretMove
                }
            }
            SelEvent::Arrow(to) => {
                if !self.field.focused {
                    return SelOutcome::Ignored;
                }
                if self.field.state == SelectState::All {
                    self.cancels += 1; // 方向键 = 取消全选改定位
                }
                self.field.arrow_navigate(to);
                SelOutcome::CaretMove
            }
            SelEvent::Blur => {
                self.field.blur();
                self.ring_on = false;
                SelOutcome::Ignored
            }
        }
    }

    /// F206 协同：有焦点即有环（全选态环仍在——环不因全选消失）。
    pub fn ring_visible(&self) -> bool {
        self.ring_on && self.field.focused
    }

    pub fn all_entries(&self) -> u32 {
        self.all_entries
    }

    pub fn replaces(&self) -> u32 {
        self.replaces
    }

    pub fn cancels(&self) -> u32 {
        self.cancels
    }
}

fn state_kind(s: &SelectState) -> u8 {
    match s {
        SelectState::All => 0,
        SelectState::Caret(_) => 1,
        SelectState::Range(..) => 2,
    }
}

/// 三框一致性对账：同脚本跑出的两台状态机终态同源
/// （选择态种别 + 焦点 + 全链计账逐一相符）。
pub fn same_derivation(a: &SelectLifecycle, b: &SelectLifecycle) -> bool {
    state_kind(&a.field.state) == state_kind(&b.field.state)
        && a.field.focused == b.field.focused
        && a.all_entries == b.all_entries
        && a.replaces == b.replaces
        && a.cancels == b.cancels
}

// ---------------------------------------------------------------------------
// 深化自检
// ---------------------------------------------------------------------------

/// 一致性对账脚本（三框同跑——事件序单一源）。
fn consistency_script(lc: &mut SelectLifecycle) {
    let _ = lc.apply(SelEvent::TabIn);
    let _ = lc.apply(SelEvent::Type('v'));
    let _ = lc.apply(SelEvent::Type('x'));
    let _ = lc.apply(SelEvent::Arrow(1));
    let _ = lc.apply(SelEvent::ClickAt(0));
    let _ = lc.apply(SelEvent::Blur);
}

pub fn run_f581_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new(ISTAR_DOMAIN);

    // 1) Tab 进入 → 全选态 + 环亮（状态机入口账）。
    let mut lc = SelectLifecycle::new("http://varix.local");
    let o1 = lc.apply(SelEvent::TabIn);
    cs.add(
        "tab in selects all",
        o1 == SelOutcome::SelectAll && lc.all_entries() == 1 && lc.ring_visible(),
        "",
    );

    // 2) 打字 = 替换并退出全选态（速填流闭环）。
    let o2 = lc.apply(SelEvent::Type('h'));
    cs.add(
        "type replaces and exits all",
        o2 == SelOutcome::Replace
            && lc.replaces() == 1
            && lc.field.text == "h"
            && !matches!(lc.field.state, SelectState::All),
        "",
    );

    // 3) 方向键 = 取消全选改光标定位。
    let mut lc2 = SelectLifecycle::new("搜索关键词");
    let _ = lc2.apply(SelEvent::TabIn);
    let o3 = lc2.apply(SelEvent::Arrow(3));
    cs.add(
        "arrow cancels to caret",
        o3 == SelOutcome::CaretMove
            && lc2.cancels() == 1
            && matches!(lc2.field.state, SelectState::Caret(3)),
        "",
    );

    // 4) 点击 = 按点击位置定位（含全选态下点击取消）。
    let mut lc3 = SelectLifecycle::new("已有内容");
    let o4a = lc3.apply(SelEvent::ClickAt(2));
    let o4b = lc3.apply(SelEvent::TabIn);
    let o4c = lc3.apply(SelEvent::ClickAt(4));
    cs.add(
        "click positions caret",
        o4a == SelOutcome::CaretMove
            && o4b == SelOutcome::SelectAll
            && o4c == SelOutcome::CaretMove
            && matches!(lc3.field.state, SelectState::Caret(4))
            && lc3.cancels() == 1,
        "",
    );

    // 5) Shift+Tab 回向进入同语义（全选）。
    let mut lc4 = SelectLifecycle::new("varix.local");
    let o5 = lc4.apply(SelEvent::ShiftTabIn);
    cs.add(
        "shift tab backward same semantics",
        o5 == SelOutcome::SelectAll && lc4.all_entries() == 1,
        "",
    );

    // 6) 三框一致性：表单框/地址栏/搜索框同脚本 → 终态同源。
    let mut form = SelectLifecycle::new("用户名预填");
    let mut addr = SelectLifecycle::new("varix.local/首页");
    let mut search = SelectLifecycle::new("上轮搜索词");
    consistency_script(&mut form);
    consistency_script(&mut addr);
    consistency_script(&mut search);
    cs.add(
        "three surfaces same derivation",
        same_derivation(&form, &addr)
            && same_derivation(&addr, &search)
            && same_derivation(&form, &search),
        "",
    );

    // 7) F206 焦点环协同：全选态环亮、失焦收环。
    let mut lc5 = SelectLifecycle::new("文本");
    let _ = lc5.apply(SelEvent::TabIn);
    let ring_all = lc5.ring_visible();
    let _ = lc5.apply(SelEvent::Blur);
    cs.add("f206 ring during select all", ring_all && !lc5.ring_visible(), "");

    // 8) 无焦点打字忽略（事件链不越权——Ignored 诚实回报）。
    let mut lc6 = SelectLifecycle::new("abc");
    let o8 = lc6.apply(SelEvent::Type('x'));
    cs.add(
        "typing without focus ignored",
        o8 == SelOutcome::Ignored && lc6.field.text == "abc",
        "",
    );

    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shift_tab_full_flow_replace() {
        let mut lc = SelectLifecycle::new("old");
        let _ = lc.apply(SelEvent::ShiftTabIn);
        let _ = lc.apply(SelEvent::Type('n'));
        assert_eq!(lc.field.text, "n");
        assert_eq!(lc.replaces(), 1);
    }

    #[test]
    fn arrow_clamps_beyond_length() {
        let mut lc = SelectLifecycle::new("abc");
        let _ = lc.apply(SelEvent::TabIn);
        let _ = lc.apply(SelEvent::Arrow(99));
        assert!(matches!(lc.field.state, SelectState::Caret(3)));
    }

    #[test]
    fn divergent_scripts_break_derivation() {
        let mut a = SelectLifecycle::new("xyz");
        let mut b = SelectLifecycle::new("xyz");
        let _ = a.apply(SelEvent::TabIn);
        let _ = b.apply(SelEvent::TabIn);
        let _ = a.apply(SelEvent::Type('1'));
        let _ = b.apply(SelEvent::Arrow(0)); // 不同脚本 → 终态分叉
        assert!(!same_derivation(&a, &b));
    }
}
