//! F404 Win+E 资源管理器快捷 · 完整设计（STAR I 主册 G-I-04）。
//!
//! **判据（主册）**：默认标签/窗口设置；此机页内容清单；连按行为；
//! 快捷键注册（F244）；冷启动打开时长（首开 <1.5s，F283 骨架先行）。
//! ＋通12。
//!
//! **设计要点（主册）**：Win+E 打开资源管理器——「同一窗口开新标签」
//! 为默认，设置可改「总是新窗口」；默认落点「此机」页（所有盘符 +
//! S: 共享卷 + 常用文件夹总览）；连续按 Win+E 逐个新标签。
//!
//! 本模块是键位到窗口动作的**语义核**：模式两态、单例窗口表、连按
//! 行为（默认逐标签/新窗模式逐窗）、冷启动预算记账（骨架先行两拍）。
//! 「此机」页内容清单在此钉死（所有盘符 + S: 共享卷 + 常用文件夹——
//! 比 Windows 默认「快速访问」更综合）。
//!
//! 时间注入式（毫秒戳），无外部依赖。

use crate::checks::CheckSet;
use crate::uni1::ubase::{Chord, HotkeyTable, MOD_WIN};

use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 规格常量（参数唯一源）
// ---------------------------------------------------------------------------

/// 冷启动首开判线（ms）——「首开 <1.5s」。
pub const COLD_OPEN_BUDGET_MS: u64 = 1_500;

/// 骨架先行第一拍上限（骨架可见，ms）——F283 纪律。
pub const SKELETON_AT_MS: u64 = 400;

/// 此机页内容清单（默认落点）——比「快速访问」更综合的三区：
/// 盘符区（调用方注入实际盘符）、共享卷固定项、常用文件夹固定项。
pub const THIS_PC_SHARED_VOLUME: &str = "S:";
pub const THIS_PC_FOLDERS: [&str; 4] = ["文档", "下载", "桌面", "图片"];

/// 打开模式。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpenMode {
    /// 同一窗口开新标签（默认）。
    SameWindowTab,
    /// 总是新窗口。
    AlwaysNewWindow,
}

/// 一个资源管理器窗口：id + 标签页清单。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExpWindow {
    pub id: u64,
    pub tabs: Vec<&'static str>,
    /// 当前活动标签下标。
    pub active: usize,
}

/// Win+E 语义核。
pub struct ExploreHot {
    pub mode: OpenMode,
    pub windows: Vec<ExpWindow>,
    next_id: u64,
    /// 首开耗时账（骨架拍 + 内容拍，注入式记账）。
    pub last_cold_ms: Option<u64>,
    pub cold_open_count: u64,
    pub cold_over_budget: u64,
    /// 注册表登记位（F244）。
    pub hotkeys: HotkeyTable,
}

impl ExploreHot {
    pub fn new() -> ExploreHot {
        let mut hotkeys = HotkeyTable::new();
        let _ = hotkeys.register("f404.explore", Chord::new(MOD_WIN, b'E'));
        ExploreHot {
            mode: OpenMode::SameWindowTab,
            windows: Vec::new(),
            next_id: 1,
            last_cold_ms: None,
            cold_open_count: 0,
            cold_over_budget: 0,
            hotkeys,
        }
    }

    pub fn set_mode(&mut self, mode: OpenMode) {
        self.mode = mode;
    }

    /// 此机页内容清单：盘符区（注入）+ 共享卷 + 常用文件夹。
    /// 顺序钉死：盘符（字母序）→ S: 共享卷 → 常用文件夹（固定序）。
    pub fn this_pc_entries(&self, drives: &[&'static str]) -> Vec<&'static str> {
        let mut v = Vec::new();
        let mut sorted = drives.to_vec();
        sorted.sort_unstable();
        v.extend(sorted);
        v.push(THIS_PC_SHARED_VOLUME);
        v.extend(THIS_PC_FOLDERS);
        v
    }

    /// 按 Win+E：默认模式——有窗口则在其上开新标签（落此机页），
    /// 无窗口则开新窗；新窗模式——每次都开新窗。
    /// `cold_ms` 为本次首开的实测冷启耗时（None = 热路径，不记账）。
    pub fn hotkey_press(&mut self, cold_ms: Option<u64>) -> (u64, usize) {
        let (win_id, tab_idx) = match self.mode {
            OpenMode::SameWindowTab => {
                let id = match self.windows.last() {
                    Some(w) => w.id,
                    None => {
                        let id = self.next_id;
                        self.next_id += 1;
                        self.windows.push(ExpWindow { id, tabs: Vec::new(), active: 0 });
                        id
                    }
                };
                let w = self.windows.iter_mut().find(|w| w.id == id).unwrap();
                w.tabs.push("此机");
                w.active = w.tabs.len() - 1;
                (id, w.active)
            }
            OpenMode::AlwaysNewWindow => {
                let id = self.next_id;
                self.next_id += 1;
                self.windows.push(ExpWindow { id, tabs: alloc::vec!["此机"], active: 0 });
                (id, 0)
            }
        };
        if let Some(ms) = cold_ms {
            self.cold_open_count += 1;
            self.last_cold_ms = Some(ms);
            if ms > COLD_OPEN_BUDGET_MS {
                self.cold_over_budget += 1;
            }
        }
        (win_id, tab_idx)
    }

    /// 骨架先行两拍：骨架应当在 SKELETON_AT_MS 前可见（判据 F283 联动）。
    pub fn skeleton_on_time(skeleton_seen_ms: u64) -> bool {
        skeleton_seen_ms <= SKELETON_AT_MS
    }

    /// 连按行为快照：默认模式 N 连按 = 1 窗 N 标签；新窗模式 = N 窗。
    /// 返回（窗数, 最近窗标签数）。
    pub fn repeat_press_shape(&self) -> (usize, usize) {
        (self.windows.len(), self.windows.last().map(|w| w.tabs.len()).unwrap_or(0))
    }

    /// 关窗（标签清空即窗亡——窗口表只记活的）。
    pub fn close_window(&mut self, id: u64) -> bool {
        let before = self.windows.len();
        self.windows.retain(|w| w.id != id);
        self.windows.len() != before
    }

    pub fn window_count(&self) -> usize {
        self.windows.len()
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F404 自检。
pub fn run_explorehot_checks() -> CheckSet {
    let mut set = CheckSet::new("uni1-F404");

    // 快捷键注册（F244）。
    let mut e = ExploreHot::new();
    set.add(
        "f404-hotkey-registered",
        e.hotkeys.lookup(Chord::new(MOD_WIN, b'E')) == Some("f404.explore") && e.hotkeys.entry_count() == 1,
        "",
    );

    // 默认落点此机页 + 内容清单钉死（盘符注入 → S: → 四常用）。
    let entries = e.this_pc_entries(&[&"D:", &"C:"]);
    set.add(
        "f404-this-pc-list",
        entries == alloc::vec!["C:", "D:", "S:", "文档", "下载", "桌面", "图片"],
        "",
    );

    // 默认模式：首按开窗，再按逐个新标签。
    let (w1, t1) = e.hotkey_press(Some(1_100));
    let (w2, t2) = e.hotkey_press(None);
    let (w3, t3) = e.hotkey_press(None);
    set.add(
        "f404-tab-mode-repeat",
        e.mode == OpenMode::SameWindowTab
            && w1 == w2 && w2 == w3
            && t1 == 0 && t2 == 1 && t3 == 2
            && e.window_count() == 1,
        "",
    );

    // 冷启动记账：1.1s 在预算内；超 1.5s 如实计数。
    set.add(
        "f404-cold-under-1500",
        e.last_cold_ms == Some(1_100) && e.cold_open_count == 1 && e.cold_over_budget == 0,
        "",
    );
    let _ = e.hotkey_press(Some(1_800));
    set.add("f404-cold-over-budget-logged", e.cold_over_budget == 1, "");

    // 新窗模式：连按 = 逐个新窗。
    let mut n = ExploreHot::new();
    n.set_mode(OpenMode::AlwaysNewWindow);
    let (a, _) = n.hotkey_press(None);
    let (b, _) = n.hotkey_press(None);
    let (c, _) = n.hotkey_press(None);
    let (wc, tc) = n.repeat_press_shape();
    set.add(
        "f404-window-mode-repeat",
        a != b && b != c && wc == 3 && tc == 1,
        "",
    );

    // 骨架先行两拍：400ms 内骨架可见。
    set.add("f404-skeleton-first", ExploreHot::skeleton_on_time(380) && !ExploreHot::skeleton_on_time(420), "");

    // 关窗后表干净。
    let mut m = ExploreHot::new();
    let (wid, _) = m.hotkey_press(None);
    set.add("f404-close-window", m.close_window(wid) && m.window_count() == 0 && !m.close_window(wid), "");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tab_mode_accumulates_tabs() {
        let mut e = ExploreHot::new();
        let (w, t) = e.hotkey_press(None);
        for i in 1..5 {
            let (w2, t2) = e.hotkey_press(None);
            assert_eq!(w2, w, "默认模式必须同一窗口");
            assert_eq!(t2, i);
        }
        assert_eq!(e.windows[0].tabs, alloc::vec!["此机", "此机", "此机", "此机", "此机"]);
        assert_eq!(e.windows[0].active, 4);
        let _ = t;
    }

    #[test]
    fn this_pc_order_is_pinned() {
        let e = ExploreHot::new();
        // 盘符乱序注入 → 输出字母序。
        let v = e.this_pc_entries(&[&"Z:", &"A:", &"M:"]);
        assert_eq!(v[0], "A:");
        assert_eq!(v[1], "M:");
        assert_eq!(v[2], "Z:");
        assert_eq!(v[3], "S:");
        assert_eq!(&v[4..], &THIS_PC_FOLDERS);
    }

    #[test]
    fn window_mode_independent_sessions() {
        let mut n = ExploreHot::new();
        n.set_mode(OpenMode::AlwaysNewWindow);
        let ids: Vec<u64> = (0..3).map(|_| n.hotkey_press(None).0).collect();
        assert_eq!(ids[0] != ids[1] && ids[1] != ids[2], true);
        assert_eq!(n.window_count(), 3);
        assert!(n.close_window(ids[1]));
        assert_eq!(n.window_count(), 2);
    }

    #[test]
    fn cold_budget_honest_accounting() {
        let mut e = ExploreHot::new();
        assert!(e.hotkey_press(Some(1_499)).0 > 0);
        assert_eq!(e.cold_over_budget, 0);
        assert!(e.hotkey_press(Some(1_501)).0 > 0);
        assert_eq!(e.cold_over_budget, 1);
        assert_eq!(COLD_OPEN_BUDGET_MS, 1_500);
    }
}
