//! F408 Win+X 快捷菜单 · 完整设计（STAR I 主册 G-I-08）。
//!
//! **判据（主册）**：九项清单与落地页对照表；首字母快捷；子菜单二级；
//! 菜单键位注册；各项打开时长 <1s。＋通12。
//!
//! **设计要点（主册）**：Win+X（或右键开始按钮）呼出电源用户菜单九项：
//! 任务管理器/设置/资源管理器/终端（F095）/终端（管理员语义——特权
//! 任务入口）/磁盘管理/设备页（F290）/存储页（F394）/关机或注销
//! （子菜单）——进阶用户的一站式跳板，全部直达对应页不中转；键盘可达
//! （X 后按首字母直达，与 Windows 同构）。
//!
//! 本模块是菜单**语义核**：九项清单与落地页对照（一处一事实钉死）、
//! 首字母索引、子菜单二级结构、打开时长记账。菜单渲染属界面层。
//!
//! 时间注入式（毫秒戳），无外部依赖。

use crate::checks::CheckSet;
use crate::uni1::ubase::{Chord, HotkeyTable, MOD_WIN};

use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 规格常量（参数唯一源）
// ---------------------------------------------------------------------------

/// 各项打开时长判线（ms）——「各项打开时长 <1s」。
pub const OPEN_BUDGET_MS: u64 = 1_000;

/// 子菜单（关机或注销）二级项。
pub const SHUTDOWN_SUBMENU: [&str; 3] = ["关机", "重启", "注销"];

/// 九项清单 + 落地页对照表（一处一事实：唯一登记点）。
/// 结构：（显示名, 首字母索引键, 落地页标识, 是否子菜单父项）。
pub const WINX_ITEMS: [(&str, u8, &str, bool); 9] = [
    ("任务管理器", b'R', "f402.taskmgr", false), // Windows 同构：T 被终端占，任务管理器用 R（与 Win+X 传统一致）
    ("设置", b'S', "f407.settings", false),
    ("资源管理器", b'W', "f404.explore", false), // Windows 同构：E 被事件查看器位让给资源管理器的 W 位? 保持 VARIX 对照表：W
    ("终端", b'T', "f095.terminal", false),
    ("终端（管理员）", b'A', "f095.terminal.admin", false),
    ("磁盘管理", b'K', "f438.disks", false),
    ("设备页", b'D', "f290.devices", false),
    ("存储页", b'O', "f394.storage", false),
    ("关机或注销", b'U', "f405.power", true),
];

// ---------------------------------------------------------------------------
// 菜单状态机
// ---------------------------------------------------------------------------

/// Win+X 菜单状态。
pub struct WinXMenu {
    pub open: bool,
    pub selected: usize,
    /// 子菜单展开态（仅对子菜单父项有效）。
    pub submenu_open: bool,
    pub submenu_selected: usize,
    /// 打开次数/超预算次数（记账）。
    pub opened: u64,
    pub over_budget: u64,
    /// 各项最近打开耗时（体验账，升序注入由调用方保证）。
    pub last_open_ms: Vec<u64>,
    /// 键位注册位（F244）。
    pub hotkeys: HotkeyTable,
}

impl WinXMenu {
    pub fn new() -> WinXMenu {
        let mut hotkeys = HotkeyTable::new();
        let _ = hotkeys.register("f408.winx", Chord::new(MOD_WIN, b'X'));
        WinXMenu {
            open: false,
            selected: 0,
            submenu_open: false,
            submenu_selected: 0,
            opened: 0,
            over_budget: 0,
            last_open_ms: Vec::new(),
            hotkeys,
        }
    }

    /// 打开（Win+X 或右键开始按钮）：选中复位首项。
    pub fn open_menu(&mut self, latency_ms: u64) {
        self.open = true;
        self.selected = 0;
        self.submenu_open = false;
        self.opened += 1;
        self.last_open_ms.push(latency_ms);
        if latency_ms > OPEN_BUDGET_MS {
            self.over_budget += 1;
        }
    }

    /// 方向键导航（上/下循环；不开时无动作）。
    pub fn move_sel(&mut self, delta: i32) {
        if !self.open || self.submenu_open {
            return;
        }
        let n = WINX_ITEMS.len() as i32;
        self.selected = ((self.selected as i32 + delta).rem_euclid(n)) as usize;
    }

    /// 首字母直达（判据「首字母快捷」）：命中唯一项则选中它；
    /// 未命中返回 false（不乱跳）。
    pub fn jump_letter(&mut self, ch: u8) -> bool {
        if !self.open {
            return false;
        }
        match WINX_ITEMS.iter().position(|(_, key, _, _)| *key == ch) {
            Some(i) => {
                self.selected = i;
                true
            }
            None => false,
        }
    }

    /// Enter：子菜单父项 → 展开二级；普通项 → 返回落地页并关菜单。
    pub fn activate(&mut self) -> Option<&'static str> {
        if !self.open {
            return None;
        }
        let (_, _, page, is_parent) = WINX_ITEMS[self.selected];
        if is_parent {
            self.submenu_open = true;
            self.submenu_selected = 0;
            return None;
        }
        self.open = false;
        Some(page)
    }

    /// 子菜单方向键与确认（关机或注销三选）。
    pub fn submenu_move(&mut self, delta: i32) {
        if !self.submenu_open {
            return;
        }
        let n = SHUTDOWN_SUBMENU.len() as i32;
        self.submenu_selected = ((self.submenu_selected as i32 + delta).rem_euclid(n)) as usize;
    }

    /// 子菜单确认：返回落地页 + 所选项。
    pub fn submenu_confirm(&mut self) -> Option<(&'static str, &'static str)> {
        if !self.submenu_open {
            return None;
        }
        self.submenu_open = false;
        self.open = false;
        Some(("f405.power", SHUTDOWN_SUBMENU[self.submenu_selected]))
    }

    /// 子菜单 Esc：先收子菜单（F424 逐层剥离）。
    pub fn submenu_cancel(&mut self) -> bool {
        if self.submenu_open {
            self.submenu_open = false;
            true
        } else {
            false
        }
    }

    /// Esc 关整菜单（子菜单未开时）。
    pub fn cancel(&mut self) -> bool {
        if self.open && !self.submenu_open {
            self.open = false;
            true
        } else {
            false
        }
    }

    pub fn is_open(&self) -> bool {
        self.open
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F408 自检。
pub fn run_winxmenu_checks() -> CheckSet {
    let mut set = CheckSet::new("uni1-F408");

    // 九项清单：数目、无重复落地页、无重复首字母、恰一个子菜单父项。
    let mut pages: Vec<&str> = Vec::new();
    let mut letters: Vec<u8> = Vec::new();
    let mut parents = 0;
    for (name, key, page, is_parent) in WINX_ITEMS {
        let _ = name;
        pages.push(page);
        letters.push(key);
        if is_parent {
            parents += 1;
        }
    }
    let dup = |v: &Vec<&str>| {
        let mut c = v.clone();
        c.sort_unstable();
        c.dedup();
        c.len() != v.len()
    };
    let dupl = {
        let mut c = letters.clone();
        c.sort_unstable();
        c.dedup();
        c.len() != letters.len()
    };
    set.add(
        "f408-nine-items-table",
        WINX_ITEMS.len() == 9 && !dup(&pages) && !dupl && parents == 1,
        "",
    );

    // 键位注册（F244）。
    let mut m = WinXMenu::new();
    set.add(
        "f408-hotkey-registered",
        m.hotkeys.lookup(Chord::new(MOD_WIN, b'X')) == Some("f408.winx"),
        "",
    );

    // 打开 <1s：500ms 达标；1.2s 超线如实计数。
    m.open_menu(500);
    set.add("f408-open-under-1s", m.is_open() && m.over_budget == 0 && m.last_open_ms[0] == 500, "");
    m.cancel();
    m.open_menu(1_200);
    set.add("f408-open-over-logged", m.over_budget == 1, "");

    // 首字母直达：S → 设置；T → 终端；未命中键不乱跳。
    set.add("f408-letter-s", m.jump_letter(b'S') && m.selected == 1, "");
    set.add("f408-letter-t", m.jump_letter(b'T') && m.selected == 3, "");
    set.add("f408-letter-miss", !m.jump_letter(b'Z') && m.selected == 3, "");

    // 普通项 Enter：直达落地页并关菜单（不中转）。
    let page = m.activate();
    set.add("f408-activate-terminal", page == Some("f095.terminal") && !m.is_open(), "");

    // 子菜单：U 父项 → 展开二级 → 下选 → 确认。
    let mut m2 = WinXMenu::new();
    m2.open_menu(400);
    m2.jump_letter(b'U');
    set.add("f408-submenu-parent-noop", m2.activate().is_none() && m2.submenu_open, "");
    m2.submenu_move(1); // 关机 → 重启
    set.add(
        "f408-submenu-confirm",
        m2.submenu_confirm() == Some(("f405.power", "重启")) && !m2.is_open(),
        "",
    );

    // 子菜单 Esc：先收子菜单再收菜单（F424 逐层剥离纪律）。
    let mut m3 = WinXMenu::new();
    m3.open_menu(400);
    m3.jump_letter(b'U');
    let _ = m3.activate();
    set.add(
        "f408-submenu-esc-peel",
        m3.submenu_cancel() && m3.is_open() && !m3.submenu_open && m3.cancel() && !m3.is_open(),
        "",
    );

    // 未开菜单：一切键位无动作。
    let mut m4 = WinXMenu::new();
    set.add(
        "f408-closed-noop",
        !m4.jump_letter(b'S') && m4.activate().is_none() && !m4.cancel() && m4.submenu_confirm().is_none(),
        "",
    );

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nine_items_landing_table_pinned() {
        // 对照表钉死：名→页 逐项核对（判据「九项清单与落地页对照表」）。
        let expect: [(&str, &str); 9] = [
            ("任务管理器", "f402.taskmgr"),
            ("设置", "f407.settings"),
            ("资源管理器", "f404.explore"),
            ("终端", "f095.terminal"),
            ("终端（管理员）", "f095.terminal.admin"),
            ("磁盘管理", "f438.disks"),
            ("设备页", "f290.devices"),
            ("存储页", "f394.storage"),
            ("关机或注销", "f405.power"),
        ];
        for (i, (name, page)) in expect.iter().enumerate() {
            assert_eq!(WINX_ITEMS[i].0, *name);
            assert_eq!(WINX_ITEMS[i].2, *page);
        }
    }

    #[test]
    fn arrows_cycle_nine_items() {
        let mut m = WinXMenu::new();
        m.open_menu(400);
        for _ in 0..9 {
            m.move_sel(1);
        }
        assert_eq!(m.selected, 0, "下键九次回环");
        m.move_sel(-1);
        assert_eq!(m.selected, 8, "上键循环到尾");
    }

    #[test]
    fn letter_jump_all_nine_unique() {
        let mut m = WinXMenu::new();
        m.open_menu(400);
        for (i, (_, key, _, _)) in WINX_ITEMS.iter().enumerate() {
            assert!(m.jump_letter(*key), "首字母 {key} 必须命中");
            assert_eq!(m.selected, i);
            // 复位到 0 再试下一个。
            m.selected = 0;
        }
    }

    #[test]
    fn submenu_peel_order_matches_f424() {
        let mut m = WinXMenu::new();
        m.open_menu(300);
        m.jump_letter(b'U');
        assert!(m.activate().is_none(), "父项 Enter 只展开");
        // 子菜单导航循环。
        m.submenu_move(1);
        m.submenu_move(1);
        m.submenu_move(1);
        assert_eq!(m.submenu_selected, 0, "子菜单三项循环");
        assert_eq!(m.submenu_confirm().unwrap().1, "关机");
    }

    #[test]
    fn open_budget_honest() {
        let mut m = WinXMenu::new();
        m.open_menu(OPEN_BUDGET_MS); // 恰达线 = 达标
        assert_eq!(m.over_budget, 0);
        m.cancel();
        m.open_menu(OPEN_BUDGET_MS + 1);
        assert_eq!(m.over_budget, 1);
    }
}
