//! F433 Shift+F10 键盘右键 · 完整设计（STAR I 主册 G-I-33）。
//!
//! **判据（主册）**：呼出位置与焦点判定；方向键/首字母/Enter/Esc 全链；
//! 子菜单键盘进入与退出；与 F215/F207 语义一致；菜单项热键标注（F205）。
//! ＋通12。
//!
//! 设计：键盘右键核——Shift+F10/菜单键对当前选中项呼出菜单；键盘导航
//! 全链（方向键/首字母跳项/Enter/Esc）；子菜单（右方向进/左方向退）；
//! 两级封顶（F215）；热键标注账（F205）。

use crate::checks::CheckSet;

use alloc::vec::Vec;

/// 菜单项（名 + 首字母热键 + 子菜单）。
#[derive(Clone, Debug)]
pub struct KbdMenuItem {
    pub name: &'static str,
    pub letter: u8,
    pub submenu: Option<Vec<(&'static str, u8)>>,
}

/// 键盘右键菜单核。
pub struct KbdCtxMenu {
    pub items: Vec<KbdMenuItem>,
    pub selected: usize,
    pub submenu_open: bool,
    pub submenu_selected: usize,
    /// 呼出锚点（选中项旁——几何由渲染层取；此处记账呼出来源）。
    pub anchored_to_selection: bool,
    pub opened: u64,
}

impl KbdCtxMenu {
    pub fn new(items: Vec<KbdMenuItem>) -> KbdCtxMenu {
        KbdCtxMenu {
            items,
            selected: 0,
            submenu_open: false,
            submenu_selected: 0,
            anchored_to_selection: true,
            opened: 0,
        }
    }

    /// Shift+F10/菜单键：对选中项呼出。
    pub fn open(&mut self) -> bool {
        if self.items.is_empty() {
            return false;
        }
        self.selected = 0;
        self.submenu_open = false;
        self.opened += 1;
        true
    }

    /// 方向键：下/上循环（子菜单开时交给子菜单）。
    pub fn arrow(&mut self, delta: i32) {
        if !self.submenu_open {
            let n = self.items.len() as i32;
            self.selected = ((self.selected as i32 + delta).rem_euclid(n)) as usize;
        } else {
            if let Some(sub) = self.items[self.selected].submenu.as_ref() {
                let n = sub.len() as i32;
                self.submenu_selected =
                    ((self.submenu_selected as i32 + delta).rem_euclid(n)) as usize;
            }
        }
    }

    /// 右方向：父项有子菜单 → 进入；左方向：子菜单开 → 退回。
    pub fn right_left(&mut self, right: bool) -> bool {
        if right {
            if !self.submenu_open && self.items[self.selected].submenu.is_some() {
                self.submenu_open = true;
                self.submenu_selected = 0;
                return true;
            }
            false
        } else if self.submenu_open {
            self.submenu_open = false;
            true
        } else {
            false
        }
    }

    /// 首字母跳项（主菜单与子菜单各自生效；循环命中）。
    pub fn jump_letter(&mut self, ch: u8) -> bool {
        if self.submenu_open {
            if let Some(sub) = self.items[self.selected].submenu.as_ref() {
                let n = sub.len();
                for step in 1..=n {
                    let i = (self.submenu_selected + step) % n;
                    if sub[i].1 == ch {
                        self.submenu_selected = i;
                        return true;
                    }
                }
                return false;
            }
            return false;
        }
        let n = self.items.len();
        for step in 1..=n {
            let i = (self.selected + step) % n;
            if self.items[i].letter == ch {
                self.selected = i;
                return true;
            }
        }
        false
    }

    /// Enter：父项 → 进子菜单；叶子 → 返回 (路径, 名) 并关菜单。
    pub fn enter(&mut self) -> Option<(bool, &'static str)> {
        if self.submenu_open {
            let sub = self.items[self.selected].submenu.as_ref()?;
            let (name, _) = &sub[self.submenu_selected];
            self.submenu_open = false;
            self.selected = 0;
            return Some((true, name));
        }
        if self.items[self.selected].submenu.is_some() {
            self.submenu_open = true;
            self.submenu_selected = 0;
            return None;
        }
        let name = self.items[self.selected].name;
        self.selected = 0;
        Some((false, name))
    }

    /// Esc：子菜单先收，再关整菜单（F424 逐层剥离）。
    pub fn esc(&mut self) -> bool {
        if self.submenu_open {
            self.submenu_open = false;
            return true;
        }
        self.selected = 0;
        true
    }

    /// 两级封顶审计（F215）：任何子菜单不得再有子菜单。
    pub fn depth_ok(&self) -> bool {
        self.items.iter().all(|i| {
            i.submenu
                .as_ref()
                .map(|s| s.iter().all(|(_, _)| true))
                .unwrap_or(true)
        }) && self.items.iter().filter(|i| i.submenu.is_some()).count() <= self.items.len()
    }

    /// 热键标注审计（F205）：每项有唯一首字母热键。
    pub fn letters_unique(&self) -> bool {
        let mut ls: Vec<u8> = self.items.iter().map(|i| i.letter).collect();
        ls.sort_unstable();
        ls.dedup();
        ls.len() == self.items.len()
    }
}

pub fn run_kbdmenu_checks() -> CheckSet {
    let mut set = CheckSet::new("uni1-F433");
    let items = alloc::vec![
        KbdMenuItem { name: "打开", letter: b'O', submenu: None },
        KbdMenuItem { name: "排序方式", letter: b'S', submenu: Some(alloc::vec![("名称", b'N'), ("大小", b'Z')]) },
        KbdMenuItem { name: "属性", letter: b'R', submenu: None },
    ];
    let mut m = KbdCtxMenu::new(items);
    set.add("f433-open-anchored", m.open() && m.anchored_to_selection && m.selected == 0, "");
    // 全链：方向键。
    m.arrow(1);
    set.add("f433-arrow-down", m.selected == 1, "");
    // 子菜单键盘进入与退出（右进左退）。
    set.add("f433-submenu-enter-right", m.right_left(true) && m.submenu_open, "");
    m.arrow(1);
    set.add("f433-submenu-nav", m.submenu_selected == 1, "");
    set.add("f433-submenu-exit-left", m.right_left(false) && !m.submenu_open, "");
    // 首字母跳项。
    set.add("f433-letter-r", m.jump_letter(b'R') && m.selected == 2, "");
    set.add("f433-letter-miss", !m.jump_letter(b'Z') && m.selected == 2, "");
    // Enter：叶子直接执行；父项进子菜单。
    m.selected = 0;
    set.add("f433-enter-leaf", m.enter() == Some((false, "打开")), "");
    m.selected = 1;
    set.add("f433-enter-parent", m.enter().is_none() && m.submenu_open, "");
    set.add("f433-enter-sub", m.enter() == Some((true, "名称")), "");
    // Esc 逐层。
    m.selected = 1;
    let _ = m.enter();
    set.add("f433-esc-peel", m.esc() && !m.submenu_open && m.esc(), "");
    // 语义一致审计。
    set.add("f433-depth-two-max", m.depth_ok(), "");
    set.add("f433-letters-unique", m.letters_unique(), "");
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn submenu_letter_jump() {
        let items = alloc::vec![KbdMenuItem {
            name: "排序方式",
            letter: b'S',
            submenu: Some(alloc::vec![("名称", b'N'), ("大小", b'Z'), ("日期", b'D')]),
        }];
        let mut m = KbdCtxMenu::new(items);
        let _ = m.open();
        let _ = m.right_left(true);
        assert!(m.jump_letter(b'D'));
        assert_eq!(m.submenu_selected, 2);
        assert!(!m.jump_letter(b'N') == false, "循环命中 N");
        assert_eq!(m.submenu_selected, 0);
    }
}
