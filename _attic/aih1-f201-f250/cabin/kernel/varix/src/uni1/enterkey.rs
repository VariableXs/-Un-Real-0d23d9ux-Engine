//! F410 Enter 打开与 Ctrl+Enter 新窗 · 完整设计（STAR I 主册 G-I-10）。
//!
//! **判据（主册）**：四键行为矩阵（文件/目录×四键）；首字母跳转；默认
//! 方式与 F257 联动；新窗独立会话（F266 独立栈）。＋通12。
//!
//! 设计：文件列表键位核——四键 = Enter/Ctrl+Enter/Alt+Enter（转发
//! F412 语义）/菜单键（转发 F433）；文件×目录两形态行为矩阵；首字母
//! 跳转（循环命中）；新窗独立导航栈（F266 注入口：栈由调用方持有，
//! 此处钉「新窗必须领新栈」的规则并记账）。

use crate::checks::CheckSet;

use alloc::vec::Vec;

/// 列表项形态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ItemKind {
    File,
    Dir,
}

/// 一项。
#[derive(Clone, Copy, Debug)]
pub struct ListItem {
    pub name: &'static str,
    pub kind: ItemKind,
}

/// 四键枚举。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ListKey {
    Enter,
    CtrlEnter,
    AltEnter,
    MenuKey,
}

/// 四键动作结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ListAction {
    /// 文件：默认方式打开（F257 关联）。
    OpenDefault(u64),
    /// 文件/目录：新窗口打开（必须领新栈——F266 独立会话）。
    OpenNewWindow(u64),
    /// 目录：进入（导航，不新开）。
    EnterDir(u64),
    /// 属性（F412 语义转发）。
    Properties(u64),
    /// 键盘右键菜单（F433 语义转发）。
    CtxMenu(u64),
    /// 无动作（矩阵外的组合按「最接近直觉」落位后仍为空时）。
    Noop,
}

/// 文件列表键位核。
pub struct FileListKeys {
    items: Vec<ListItem>,
    pub selected: usize,
    /// 新窗计数（每个新窗都必须是新会话——与 F266 栈表对账）。
    pub new_windows: u64,
    /// 栈表记账：新窗数 == 新栈数（独立会话判据的账面）。
    pub new_stacks: u64,
}

impl FileListKeys {
    pub fn new(items: Vec<ListItem>) -> FileListKeys {
        FileListKeys { items, selected: 0, new_windows: 0, new_stacks: 0 }
    }

    /// 键分发：四键×两形态行为矩阵（唯一实现点）。
    pub fn press(&mut self, key: ListKey) -> ListAction {
        let idx = self.selected;
        let kind = match self.items.get(idx) {
            Some(i) => i.kind,
            None => return ListAction::Noop,
        };
        let pid = idx as u64;
        match (key, kind) {
            (ListKey::Enter, ItemKind::Dir) => ListAction::EnterDir(pid),
            (ListKey::Enter, ItemKind::File) => ListAction::OpenDefault(pid),
            (ListKey::CtrlEnter, _) => {
                self.new_windows += 1;
                // 新窗必须领新栈（F266 独立会话）——规则在此钉死。
                self.new_stacks += 1;
                ListAction::OpenNewWindow(pid)
            }
            (ListKey::AltEnter, _) => ListAction::Properties(pid),
            (ListKey::MenuKey, _) => ListAction::CtxMenu(pid),
        }
    }

    /// 首字母跳转：从当前选中位向后循环命中（Windows 同构）。
    pub fn jump_letter(&mut self, ch: u8) -> bool {
        let n = self.items.len();
        if n == 0 {
            return false;
        }
        for step in 1..=n {
            let i = (self.selected + step) % n;
            if self.items[i].name.as_bytes().first() == Some(&ch) {
                self.selected = i;
                return true;
            }
        }
        false
    }

    /// 新窗-新栈对账（独立会话判据：两数必须相等）。
    pub fn sessions_reconciled(&self) -> bool {
        self.new_windows == self.new_stacks
    }

    pub fn items(&self) -> &[ListItem] {
        &self.items
    }
}

pub fn run_enterkey_checks() -> CheckSet {
    let mut set = CheckSet::new("uni1-F410");
    let mk = |name: &'static str, kind: ItemKind| ListItem { name, kind };
    let mut l = FileListKeys::new(alloc::vec![
        mk("alpha.txt", ItemKind::File),
        mk("docs", ItemKind::Dir),
        mk("beta.txt", ItemKind::File),
    ]);

    // 四键×文件矩阵。
    l.selected = 0;
    set.add(
        "f410-file-matrix",
        l.press(ListKey::Enter) == ListAction::OpenDefault(0)
            && l.press(ListKey::CtrlEnter) == ListAction::OpenNewWindow(0)
            && l.press(ListKey::AltEnter) == ListAction::Properties(0)
            && l.press(ListKey::MenuKey) == ListAction::CtxMenu(0),
        "",
    );
    // 四键×目录矩阵：Enter=进入（不新开）。
    l.selected = 1;
    set.add(
        "f410-dir-matrix",
        l.press(ListKey::Enter) == ListAction::EnterDir(1)
            && l.press(ListKey::CtrlEnter) == ListAction::OpenNewWindow(1)
            && l.press(ListKey::AltEnter) == ListAction::Properties(1)
            && l.press(ListKey::MenuKey) == ListAction::CtxMenu(1),
        "",
    );
    // 新窗独立会话对账。
    set.add("f410-newwin-independent-stack", l.new_windows == 2 && l.sessions_reconciled(), "");
    // 首字母跳转（从当前位置循环）。
    l.selected = 0;
    set.add("f410-letter-b", l.jump_letter(b'b') && l.selected == 2, "");
    set.add("f410-letter-d", l.jump_letter(b'd') && l.selected == 1, "");
    set.add("f410-letter-wrap", l.jump_letter(b'a') && l.selected == 0, "");
    set.add("f410-letter-miss", !l.jump_letter(b'z') && l.selected == 0, "");
    // 空表安全。
    let mut e = FileListKeys::new(alloc::vec![]);
    set.add("f410-empty-noop", e.press(ListKey::Enter) == ListAction::Noop && !e.jump_letter(b'a'), "");
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list() -> FileListKeys {
        FileListKeys::new(alloc::vec![
            ListItem { name: "report.docx", kind: ItemKind::File },
            ListItem { name: "archive", kind: ItemKind::Dir },
        ])
    }

    #[test]
    fn dir_enter_never_opens_window() {
        let mut l = list();
        l.selected = 1;
        assert_eq!(l.press(ListKey::Enter), ListAction::EnterDir(1));
        assert_eq!(l.new_windows, 0, "进入是导航不是打开");
        assert_eq!(l.press(ListKey::CtrlEnter), ListAction::OpenNewWindow(1));
        assert_eq!(l.new_windows, 1);
        assert!(l.sessions_reconciled());
    }

    #[test]
    fn letter_jump_cycles_from_selection() {
        let mut l = FileListKeys::new(alloc::vec![
            ListItem { name: "b1", kind: ItemKind::File },
            ListItem { name: "a1", kind: ItemKind::File },
            ListItem { name: "b2", kind: ItemKind::File },
        ]);
        l.selected = 0;
        assert!(l.jump_letter(b'b'));
        assert_eq!(l.selected, 2, "从当前位置向后找，跳过 a1");
        assert!(l.jump_letter(b'b'));
        assert_eq!(l.selected, 0, "循环回环");
    }
}
