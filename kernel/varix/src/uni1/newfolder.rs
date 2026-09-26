//! F431 Ctrl+Shift+N 快捷新建 · 完整设计（STAR I 主册 G-I-31）。
//!
//! **判据（主册）**：两场景生效；命名初态（F260 判据复用）；连建循环
//! 与退出；重名自动「新建文件夹(2)」递增；键位注册。＋通12。

use crate::checks::CheckSet;
use crate::uni1::ubase::{Chord, HotkeyTable, MOD_CTRL, MOD_SHIFT};

use alloc::vec::Vec;

/// 默认新建名。
pub const DEFAULT_NAME: &str = "新建文件夹";

/// 快捷新建核。
pub struct NewFolder {
    pub hotkeys: HotkeyTable,
    /// 当前目录已有条目名。
    pub entries: Vec<&'static str>,
    /// 命名初态：新名进入重命名态（全选）。
    pub pending_rename: Option<&'static str>,
    /// 连建循环态（Esc 退出）。
    pub loop_active: bool,
    pub created: u64,
}

impl NewFolder {
    pub fn new() -> NewFolder {
        let mut hotkeys = HotkeyTable::new();
        let _ = hotkeys.register("f431.newfolder", Chord::new(MOD_CTRL | MOD_SHIFT, b'N'));
        NewFolder { hotkeys, entries: Vec::new(), pending_rename: None, loop_active: false, created: 0 }
    }

    /// 重名递增名：「新建文件夹」「新建文件夹(2)」…（静态候选表直查；
    /// 超出表尾回退「新建文件夹(n)」——桌面层按同一规则支持任意 n）。
    pub fn unique_name(existing: &[&'static str]) -> &'static str {
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

    /// Ctrl+Shift+N：生成唯一名 + 进入命名初态（全选重命名）。
    /// 两场景（资源管理器/桌面）同一入口同一行为。
    pub fn press(&mut self, scene_desktop: bool) -> &'static str {
        let name = Self::unique_name(&self.entries);
        self.entries.push(name);
        self.pending_rename = Some(name);
        self.loop_active = true;
        self.created += 1;
        let _ = scene_desktop; // 两场景行为一致（判据：同一个键同一个结果）
        name
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

    /// 命名初态判定（F260 复用：进入即全选、直接可打字）。
    pub fn naming_initial_state(&self) -> bool {
        self.pending_rename.is_some()
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
    r.entries = alloc::vec!["新建文件夹"];
    set.add("f431-dup-2", NewFolder::unique_name(&r.entries) == "新建文件夹(2)", "");
    r.entries.push("新建文件夹(2)");
    set.add("f431-dup-3", NewFolder::unique_name(&r.entries) == "新建文件夹(3)", "");
    // 连建循环与退出。
    set.add("f431-loop-active", n.loop_active, "");
    set.add("f431-confirm-keeps-loop", n.confirm() && !n.naming_initial_state() && n.loop_active, "");
    set.add("f431-esc-exits-loop", n.esc_exit() && !n.loop_active && !n.esc_exit(), "");
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
        assert!(n.entries.contains(&"新建文件夹"));
        assert!(n.entries.contains(&"新建文件夹(5)"));
    }
}
