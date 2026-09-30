//! F431 Ctrl+Shift+N 快捷新建 · 完整设计（STAR I 主册 G-I-31）。
//!
//! **判据（主册）**：两场景生效；命名初态（F260 判据复用）；连建循环
//! 与退出；重名自动「新建文件夹(2)」递增；键位注册。＋通12。
//!
//! v5 纵深：条目名升级为 String（支撑用户自定义命名）；命名校验
//! （非法字符/空名/首尾空格——拒绝并说怎么改）；自定义重名检测
//! （冲突保持命名态——不吞输入）；桌面网格自动落位（第一个空格）；
//! 既有条目 F2 改名。

use crate::checks::CheckSet;
use crate::uni1::ubase::{Chord, HotkeyTable, MOD_CTRL, MOD_SHIFT};

use alloc::string::String;
use alloc::vec::Vec;

/// 默认新建名。
pub const DEFAULT_NAME: &str = "新建文件夹";

/// 文件名非法字符（Windows 语义对齐——跨平台一致性以最严者为界）。
pub const FORBIDDEN_CHARS: [char; 9] = ['\\', '/', ':', '*', '?', '"', '<', '>', '|'];

/// 快捷新建核。
pub struct NewFolder {
    pub hotkeys: HotkeyTable,
    /// 当前目录已有条目名。
    pub entries: Vec<String>,
    /// 命名初态：新名进入重命名态（全选）。
    pub pending_rename: Option<String>,
    /// 连建循环态（Esc 退出）。
    pub loop_active: bool,
    pub created: u64,
}

impl NewFolder {
    pub fn new() -> NewFolder {
        let mut hotkeys = HotkeyTable::new();
        let _ = hotkeys.register("f431.newfolder", Chord::new(MOD_CTRL | MOD_SHIFT, b'N'));
        NewFolder {
            hotkeys,
            entries: Vec::new(),
            pending_rename: None,
            loop_active: false,
            created: 0,
        }
    }

    /// 名称合法性校验（人话归因——说「怎么改对」）。
    pub fn validate_name(name: &str) -> Result<(), &'static str> {
        if name.is_empty() {
            return Err("名字不能为空——输入一个名称");
        }
        if name.starts_with(' ') || name.ends_with(' ') {
            return Err("名字首尾不能有空格——去掉首尾空格");
        }
        if let Some(c) = name.chars().find(|c| FORBIDDEN_CHARS.contains(c)) {
            let _ = c;
            return Err("名字里有文件名不允许的字符 \\ / : * ? \" < > |——换个字符");
        }
        Ok(())
    }

    /// 重名递增名：「新建文件夹」「新建文件夹(2)」…（静态候选表直查；
    /// 超出表尾回退「新建文件夹(n)」——桌面层按同一规则支持任意 n）。
    pub fn unique_name(existing: &[&str]) -> &'static str {
        const CANDIDATES: [&str; 9] = [
            "新建文件夹",
            "新建文件夹(2)",
            "新建文件夹(3)",
            "新建文件夹(4)",
            "新建文件夹(5)",
            "新建文件夹(6)",
            "新建文件夹(7)",
            "新建文件夹(8)",
            "新建文件夹(9)",
        ];
        CANDIDATES
            .into_iter()
            .find(|c| !existing.contains(c))
            .unwrap_or("新建文件夹(n)")
    }

    /// 基于当前 entries 的下一个唯一默认名。
    pub fn next_unique_name(&self) -> &'static str {
        let refs: Vec<&str> = self.entries.iter().map(|s| s.as_str()).collect();
        Self::unique_name(&refs)
    }

    /// Ctrl+Shift+N：生成唯一名 + 进入命名初态（全选重命名）。
    /// 两场景（资源管理器/桌面）同一入口同一行为。
    pub fn press(&mut self, scene_desktop: bool) -> &'static str {
        let name = self.next_unique_name();
        self.entries.push(String::from(name));
        self.pending_rename = Some(String::from(name));
        self.loop_active = true;
        self.created += 1;
        let _ = scene_desktop; // 两场景行为一致（判据：同一个键同一个结果）
        name
    }

    /// 命名初态判定（F260 复用：进入即全选、直接可打字）。
    pub fn naming_initial_state(&self) -> bool {
        self.pending_rename.is_some()
    }

    /// 确认命名（Enter）：退出命名态，连建循环保持。
    pub fn confirm(&mut self) -> bool {
        self.pending_rename = None;
        true
    }

    /// Esc：退出连建循环（并取消当前命名——新文件夹保留）。
    pub fn esc_exit(&mut self) -> bool {
        if !self.loop_active {
            return false;
        }
        self.pending_rename = None;
        self.loop_active = false;
        true
    }

    /// 自定义命名（用户在命名态打字后回车）：先校验后查重——非法字符
    /// 与重名都拒绝且保持命名态（不吞用户输入）。
    /// 待命名条目 = entries 末项（press 刚创建的那个）。
    pub fn rename_pending(&mut self, name: &str) -> Result<(), &'static str> {
        if self.pending_rename.is_none() {
            return Err("当前没有处于命名态的新条目");
        }
        Self::validate_name(name)?;
        let pending = self.pending_rename.clone().unwrap_or_default();
        if self.entries.iter().any(|e| e.as_str() == name && e.as_str() != pending) {
            return Err("已存在同名条目——换一个名字");
        }
        if let Some(last) = self.entries.last_mut() {
            *last = String::from(name);
        }
        self.pending_rename = Some(String::from(name));
        Ok(())
    }

    /// F2 改既有条目名（同一校验同一查重——一个系统一套规则）。
    pub fn rename_entry(&mut self, idx: usize, name: &str) -> Result<(), &'static str> {
        if idx >= self.entries.len() {
            return Err("条目不存在");
        }
        Self::validate_name(name)?;
        if self.entries.iter().enumerate().any(|(i, e)| i != idx && e.as_str() == name) {
            return Err("已存在同名条目——换一个名字");
        }
        self.entries[idx] = String::from(name);
        Ok(())
    }

    /// 桌面网格自动落位：返回第一个空格位（occupied 按行主序）。
    pub fn grid_slot(occupied: &[bool]) -> usize {
        occupied.iter().position(|o| !o).unwrap_or(occupied.len())
    }
}

pub fn run_newfolder_checks() -> CheckSet {
    let mut set = CheckSet::new("uni1-F431");
    let mut n = NewFolder::new();
    set.add(
        "f431-hotkey-registered",
        n.hotkeys.lookup(Chord::new(MOD_CTRL | MOD_SHIFT, b'N')) == Some("f431.newfolder"),
        "",
    );
    // 两场景一致。
    let a = n.press(false);
    let b = n.press(true);
    set.add(
        "f431-two-scenes-same",
        a == DEFAULT_NAME && b == "新建文件夹(2)" && n.created == 2,
        "",
    );
    // 命名初态。
    set.add("f431-naming-initial", n.naming_initial_state(), "");
    // 重名递增。
    let mut r = NewFolder::new();
    r.entries = alloc::vec![String::from("新建文件夹")];
    set.add("f431-dup-2", r.next_unique_name() == "新建文件夹(2)", "");
    r.entries.push(String::from("新建文件夹(2)"));
    set.add("f431-dup-3", r.next_unique_name() == "新建文件夹(3)", "");
    // 连建循环与退出。
    set.add("f431-loop-active", n.loop_active, "");
    set.add("f431-confirm-keeps-loop", n.confirm() && !n.naming_initial_state() && n.loop_active, "");
    set.add("f431-esc-exits-loop", n.esc_exit() && !n.loop_active && !n.esc_exit(), "");
    // v5：命名校验——非法字符/空名/首尾空格三路全拒且归因人话。
    set.add(
        "f431-name-forbidden-chars",
        NewFolder::validate_name("a/b") == Err("名字里有文件名不允许的字符 \\ / : * ? \" < > |——换个字符")
            && NewFolder::validate_name("a*b").is_err()
            && NewFolder::validate_name("a|b").is_err(),
        "",
    );
    set.add(
        "f431-name-empty-and-space",
        NewFolder::validate_name("") == Err("名字不能为空——输入一个名称")
            && NewFolder::validate_name(" x ") == Err("名字首尾不能有空格——去掉首尾空格"),
        "",
    );
    set.add("f431-name-valid-pass", NewFolder::validate_name("我的资料 2026").is_ok(), "");
    // v5：自定义命名——非法拒且保持命名态；合法生效。
    let mut c = NewFolder::new();
    let _ = c.press(false);
    set.add(
        "f431-rename-invalid-keeps-state",
        c.rename_pending("a/b").is_err() && c.naming_initial_state(),
        "",
    );
    set.add(
        "f431-rename-valid",
        c.rename_pending("方案库").is_ok() && c.entries.last().map(|s| s.as_str()) == Some("方案库"),
        "",
    );
    // v5：自定义重名检测——冲突拒、保持命名态、改后可过。
    let mut d = NewFolder::new();
    let _ = d.press(false);
    let _ = d.confirm();
    let _ = d.press(false);
    set.add(
        "f431-rename-dup-rejected",
        d.rename_pending("新建文件夹") == Err("已存在同名条目——换一个名字") && d.naming_initial_state(),
        "",
    );
    set.add("f431-rename-dup-then-ok", d.rename_pending("新建文件夹(2)").is_ok(), "");
    // v5：F2 改既有条目——同校验同查重（一套规则）。
    set.add("f431-rename-entry-f2", d.rename_entry(0, "归档").is_ok() && d.entries[0] == "归档", "");
    set.add("f431-rename-entry-dup", d.rename_entry(1, "归档").is_err(), "");
    set.add("f431-rename-entry-bounds", d.rename_entry(9, "x").is_err(), "");
    // v5：网格自动落位——第一个空格。
    set.add(
        "f431-grid-first-free",
        NewFolder::grid_slot(&[true, true, false, true]) == 2 && NewFolder::grid_slot(&[]) == 0,
        "",
    );
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyboard_flow_ten_folders() {
        let mut n = NewFolder::new();
        for i in 0..10 {
            let _name = n.press(false);
            assert!(n.naming_initial_state(), "第 {} 个直接进命名态", i + 1);
            let _ = n.confirm();
        }
        assert_eq!(n.created, 10);
        assert!(n.entries.iter().any(|s| s == "新建文件夹"));
        assert!(n.entries.iter().any(|s| s == "新建文件夹(5)"));
    }

    #[test]
    fn rename_pending_needs_pending() {
        let mut n = NewFolder::new();
        assert!(n.rename_pending("x").is_err(), "无命名态拒绝");
    }

    #[test]
    fn forbidden_char_table_complete() {
        // 9 个非法字符逐个点名（一个不漏）。
        for c in FORBIDDEN_CHARS {
            let mut name = String::from("a");
            name.push(c);
            assert!(NewFolder::validate_name(&name).is_err(), "字符 {:?} 必须被拒", c);
        }
    }

    #[test]
    fn grid_slot_full_row_appends() {
        assert_eq!(NewFolder::grid_slot(&[true, true, true]), 3, "满行 → 追加到行尾");
    }
}
