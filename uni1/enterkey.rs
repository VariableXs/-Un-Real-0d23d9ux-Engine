//! F410 Enter 打开与 Ctrl+Enter 新窗 · 完整设计（STAR I 主册 G-I-10）。
//!
//! **判据（主册）**：四键行为矩阵（文件/目录×四键）；首字母跳转；默认
//! 方式与 F257 联动；新窗独立会话（F266 独立栈）。＋通12。
//!
//! 设计：文件列表键位核——四键 = Enter/Ctrl+Enter/Alt+Enter（转发
//! F412 语义）/菜单键（转发 F433）；文件×目录两形态行为矩阵；首字母
//! 跳转（循环命中）；新窗独立导航栈（F266 注入口：栈由调用方持有，
//! 此处钉「新窗必须领新栈」的规则并记账）。v6 深化：条目名升级 String
//! 支撑改名、改名校验（非法字符/空名/重名人话归因）、跳转账（命中/
//! 脱靶分记）、会话对账审计（窗口数≠栈数可机检暴露）、Alt+Enter 焦点
//! 回归账、Ctrl+Enter 保持选中。

use crate::checks::CheckSet;

use alloc::string::String;
use alloc::vec::Vec;

/// 文件名禁用字符（Windows 同构——跨平台一律拦）。
pub const NAME_FORBIDDEN: [char; 9] = ['*', '?', '"', '<', '>', '|', ':', '/', '\\'];

/// 列表项形态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ItemKind {
    File,
    Dir,
}

/// 一项（v6：名字升级 String——支撑改名语义）。
#[derive(Clone, Debug)]
pub struct ListItem {
    pub name: String,
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
    /// 跳转账：首字母命中/脱靶分记（v6）。
    pub letter_jumps: u64,
    pub letter_misses: u64,
    /// Alt+Enter 焦点回归账（v6——属性页关闭必须还焦点给列表）。
    pub prop_opens: u64,
    pub prop_focus_returned: u64,
    pub prop_focus_lost: u64,
}

impl FileListKeys {
    pub fn new(items: Vec<ListItem>) -> FileListKeys {
        FileListKeys {
            items,
            selected: 0,
            new_windows: 0,
            new_stacks: 0,
            letter_jumps: 0,
            letter_misses: 0,
            prop_opens: 0,
            prop_focus_returned: 0,
            prop_focus_lost: 0,
        }
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
            (ListKey::AltEnter, _) => {
                self.prop_opens += 1;
                ListAction::Properties(pid)
            }
            (ListKey::MenuKey, _) => ListAction::CtxMenu(pid),
        }
    }

    /// 首字母跳转：从当前选中位向后循环命中（Windows 同构）。账分记。
    pub fn jump_letter(&mut self, ch: u8) -> bool {
        let n = self.items.len();
        if n == 0 {
            self.letter_misses += 1;
            return false;
        }
        for step in 1..=n {
            let i = (self.selected + step) % n;
            if self.items[i].name.as_bytes().first() == Some(&ch) {
                self.selected = i;
                self.letter_jumps += 1;
                return true;
            }
        }
        self.letter_misses += 1;
        false
    }

    /// 新窗-新栈对账（独立会话判据：两数必须相等）。
    pub fn sessions_reconciled(&self) -> bool {
        self.new_windows == self.new_stacks
    }

    /// 会话对账审计（v6）：账面被外部污染时能机检暴露（非恒真——
    /// 显式比较两账，错一个都返回 Err 带差异）。
    pub fn audit_sessions(&self) -> Result<(), (u64, u64)> {
        if self.new_windows == self.new_stacks {
            Ok(())
        } else {
            Err((self.new_windows, self.new_stacks))
        }
    }

    /// 改名（v6）：校验三关——空名/非法字符/重名，人话归因。
    pub fn rename(&mut self, idx: usize, new_name: &str) -> Result<(), &'static str> {
        let trimmed = new_name.trim();
        if trimmed.is_empty() {
            return Err("名字不能为空——输入一个名字再确认");
        }
        if let Some(bad) = trimmed.chars().find(|c| NAME_FORBIDDEN.contains(c)) {
            let _ = bad;
            return Err("名字含非法字符 * ? \" < > | : / \\——换一个不含这些符号的名字");
        }
        if self.items.iter().enumerate().any(|(i, it)| i != idx && it.name == trimmed) {
            return Err("已有同名项——换一个不同的名字");
        }
        match self.items.get_mut(idx) {
            Some(it) => {
                it.name = String::from(trimmed);
                Ok(())
            }
            None => Err("目标不存在——列表可能已刷新"),
        }
    }

    /// Alt+Enter 属性页关闭（v6）：焦点回归账（回归/丢失分开记）。
    pub fn properties_closed(&mut self, focus_returned: bool) -> bool {
        if self.prop_opens == 0 {
            return false; // 没开过属性页，无账可结。
        }
        if focus_returned {
            self.prop_focus_returned += 1;
        } else {
            self.prop_focus_lost += 1;
        }
        true
    }

    pub fn items(&self) -> &[ListItem] {
        &self.items
    }
}

pub fn run_enterkey_checks() -> CheckSet {
    let mut set = CheckSet::new("uni1-F410");
    let mk = |name: &str, kind: ItemKind| ListItem { name: String::from(name), kind };
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
    // 跳转账：命中 3 / 脱靶 1（v6 分记账）。
    set.add("f410-letter-ledger", l.letter_jumps == 3 && l.letter_misses == 1, "");
    // Ctrl+Enter 后选中保持（新窗不打断浏览位）。
    let sel_before = l.selected;
    let _ = l.press(ListKey::CtrlEnter);
    set.add("f410-ctrl-enter-keeps-selection", l.selected == sel_before, "");
    // Alt+Enter 焦点回归账：回归/丢失分开记；没开过时拒绝结账。
    let mut z = FileListKeys::new(alloc::vec![mk("a", ItemKind::File)]);
    set.add("f410-focus-close-without-open", !z.properties_closed(true), "");
    let _ = z.press(ListKey::AltEnter);
    set.add(
        "f410-focus-ledger",
        z.properties_closed(true) && z.properties_closed(false)
            && z.prop_focus_returned == 1
            && z.prop_focus_lost == 1,
        "",
    );
    // 改名三关（v6）。
    set.add(
        "f410-rename-ok",
        l.rename(2, "delta.txt").is_ok() && l.items[2].name == "delta.txt",
        "",
    );
    set.add(
        "f410-rename-invalid-chars",
        matches!(l.rename(2, "a*b"), Err(_)) && l.items[2].name == "delta.txt",
        "",
    );
    set.add(
        "f410-rename-empty",
        matches!(l.rename(2, "   "), Err(_)) && l.items[2].name == "delta.txt",
        "",
    );
    set.add(
        "f410-rename-dup",
        matches!(l.rename(2, "alpha.txt"), Err(_)) && l.items[2].name == "delta.txt",
        "",
    );
    // 改名后跳转跟随新名（docs→data：'d' 仍命中同位）。
    set.add(
        "f410-rename-then-jump",
        l.rename(1, "data").is_ok() && l.jump_letter(b'd') && l.selected == 1,
        "",
    );
    // 会话对账审计：污染账面可机检暴露（非恒真）。
    let mut bad = FileListKeys::new(alloc::vec![mk("x", ItemKind::File)]);
    let _ = bad.press(ListKey::CtrlEnter);
    bad.new_stacks = 0; // 模拟栈表丢失（下游 bug 注入点）。
    set.add(
        "f410-session-mismatch-audit",
        bad.audit_sessions() == Err((1, 0)) && !bad.sessions_reconciled(),
        "",
    );
    // 空表安全。
    let mut e = FileListKeys::new(alloc::vec![]);
    set.add(
        "f410-empty-noop",
        e.press(ListKey::Enter) == ListAction::Noop && !e.jump_letter(b'a'),
        "",
    );
    set.add("f410-empty-letter-missed", e.letter_misses == 1, "");
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list() -> FileListKeys {
        FileListKeys::new(alloc::vec![
            ListItem { name: String::from("report.docx"), kind: ItemKind::File },
            ListItem { name: String::from("archive"), kind: ItemKind::Dir },
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
            ListItem { name: String::from("b1"), kind: ItemKind::File },
            ListItem { name: String::from("a1"), kind: ItemKind::File },
            ListItem { name: String::from("b2"), kind: ItemKind::File },
        ]);
        l.selected = 0;
        assert!(l.jump_letter(b'b'));
        assert_eq!(l.selected, 2, "从当前位置向后找，跳过 a1");
        assert!(l.jump_letter(b'b'));
        assert_eq!(l.selected, 0, "循环回环");
        assert_eq!(l.letter_jumps, 2);
    }

    #[test]
    fn rename_rejects_each_class() {
        let mut l = list();
        assert!(l.rename(0, "").is_err(), "空名拒");
        assert!(l.rename(0, "x:y").is_err(), "冒号拒");
        assert!(l.rename(0, "archive").is_err(), "重名拒");
        assert!(l.rename(9, "ok").is_err(), "越界拒");
        assert!(l.rename(0, "报告.docx").is_ok(), "合法名收");
        assert_eq!(l.items[0].name, "报告.docx");
    }

    #[test]
    fn audit_clean_when_untouched() {
        let l = list();
        assert!(l.audit_sessions().is_ok());
    }
}
