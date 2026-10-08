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

/// 单窗标签上限（v7 深化）：超限诚实拒绝（CapFull），不静默挤掉旧标签。
pub const TAB_CAP: usize = 32;

/// 每窗最近路径环容量（访问史——可回放最近到过哪，满则淘汰最旧）。
pub const RECENT_CAP: usize = 8;

/// 打开模式。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpenMode {
    /// 同一窗口开新标签（默认）。
    SameWindowTab,
    /// 总是新窗口。
    AlwaysNewWindow,
}

/// 一个资源管理器窗口：id + 标签页清单 + 最近路径环。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExpWindow {
    pub id: u64,
    pub tabs: Vec<&'static str>,
    /// 当前活动标签下标。
    pub active: usize,
    /// 最近访问路径环（容量 RECENT_CAP，满则淘汰最旧——访问史不是
    /// 标签集，关闭标签不清史）。
    pub recent: Vec<&'static str>,
}

/// 开标签结果（v7 深化：去重跳转 / 容量诚实拒绝的完整出口）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TabOpen {
    /// 新开标签（窗 id, 标签下标）。
    Opened(u64, usize),
    /// 同路径已开 → 跳转既有标签（不重复开）。
    Jumped(u64, usize),
    /// 标签容量满——诚实拒绝。
    CapFull,
    /// 窗不存在。
    NoWindow,
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
                        self.windows.push(ExpWindow { id, tabs: Vec::new(), active: 0, recent: Vec::new() });
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
                self.windows.push(ExpWindow { id, tabs: alloc::vec!["此机"], active: 0, recent: Vec::new() });
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

    /// 开指定标签（v7 深化）：同路径已开 → 跳转既有标签（去重，
    /// 「此机」反复按不堆标签）；容量满 → 诚实拒绝；窗不存在 → NoWindow。
    /// 成功开标签时记入该窗最近路径环。
    pub fn open_tab(&mut self, win_id: u64, path: &'static str) -> TabOpen {
        let exists = self.windows.iter().any(|w| w.id == win_id);
        if !exists {
            return TabOpen::NoWindow;
        }
        let w = self.windows.iter_mut().find(|w| w.id == win_id).unwrap();
        if let Some(i) = w.tabs.iter().position(|t| *t == path) {
            w.active = i;
            return TabOpen::Jumped(win_id, i);
        }
        if w.tabs.len() >= TAB_CAP {
            return TabOpen::CapFull;
        }
        w.tabs.push(path);
        w.active = w.tabs.len() - 1;
        w.recent.push(path);
        if w.recent.len() > RECENT_CAP {
            w.recent.remove(0);
        }
        TabOpen::Opened(win_id, w.active)
    }

    /// 关指定标签（v7 深化）：激活权移交右邻（视觉延续——刚关的标签
    /// 右边通常是用户接下来要看的）；右邻越界 → 左邻；末标签关闭 =
    /// 关窗（返回 window_closed = true）。非法下标返回 None。
    pub fn close_tab(&mut self, win_id: u64, idx: usize) -> Option<(u64, usize, bool)> {
        let w = self.windows.iter_mut().find(|w| w.id == win_id)?;
        if idx >= w.tabs.len() {
            return None;
        }
        w.tabs.remove(idx);
        if w.tabs.is_empty() {
            let id = w.id;
            self.windows.retain(|x| x.id != id);
            return Some((id, 0, true));
        }
        let new_active = if idx < w.tabs.len() { idx } else { idx - 1 };
        w.active = new_active;
        Some((win_id, new_active, false))
    }

    /// 最近访问路径快照（可回放）。
    pub fn recent_of(&self, win_id: u64) -> &[&'static str] {
        self.windows.iter().find(|w| w.id == win_id).map(|w| w.recent.as_slice()).unwrap_or(&[])
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

    // ---- v7 深化：标签生命周期 / 去重跳转 / 最近路径环 ----

    // 去重跳转：同路径已开 → 激活既有标签，不堆重复标签。
    let mut t = ExploreHot::new();
    let (tw, _) = t.hotkey_press(None);
    let o1 = t.open_tab(tw, "D:/资料");
    let o2 = t.open_tab(tw, "C:/下载");
    let o3 = t.open_tab(tw, "D:/资料");
    set.add(
        "f404-tab-dedupe-jump",
        o1 == TabOpen::Opened(tw, 1)
            && o2 == TabOpen::Opened(tw, 2)
            && o3 == TabOpen::Jumped(tw, 1)
            && t.windows[0].tabs.len() == 3
            && t.windows[0].active == 1,
        "",
    );

    // 容量诚实拒绝：32 上限，第 33 个不同路径 → CapFull（窗口还在、
    // 标签不多不少）。
    let mut c = ExploreHot::new();
    let (cw, _) = c.hotkey_press(None);
    // 热键预置了 1 个「此机」标签；31 条全异路径填满。
    const CAP_PATHS: [&str; 31] = [
        "c0", "c1", "c2", "c3", "c4", "c5", "c6", "c7", "c8", "c9",
        "c10", "c11", "c12", "c13", "c14", "c15", "c16", "c17", "c18", "c19",
        "c20", "c21", "c22", "c23", "c24", "c25", "c26", "c27", "c28", "c29",
        "c30",
    ];
    let filled = CAP_PATHS.iter().all(|p| matches!(c.open_tab(cw, p), TabOpen::Opened(_, _)));
    set.add(
        "f404-tab-cap-honest",
        filled && c.open_tab(cw, "D:/overflow") == TabOpen::CapFull && c.windows[0].tabs.len() == TAB_CAP,
        "",
    );

    // 关标签激活右邻：关 idx1 → 原 idx2 右移为 idx1 且激活。
    let mut r = ExploreHot::new();
    let (rw, _) = r.hotkey_press(None);
    let _ = r.open_tab(rw, "A1");
    let _ = r.open_tab(rw, "A2");
    let _ = r.open_tab(rw, "A3");
    let act = r.close_tab(rw, 1);
    set.add(
        "f404-close-right-neighbor",
        act == Some((rw, 1, false))
            && r.windows[0].tabs == alloc::vec!["此机", "A2", "A3"]
            && r.windows[0].active == 1,
        "",
    );

    // 关末位标签 → 左邻接管；关到只剩一个再关 → 关窗。
    let act2 = r.close_tab(rw, 1); // 关 "A3"（末位）
    set.add(
        "f404-close-left-fallback",
        act2 == Some((rw, 1, false)) && r.windows[0].active == 1 && r.windows[0].tabs.len() == 2,
        "",
    );
    let _ = r.close_tab(rw, 1);
    let act3 = r.close_tab(rw, 0);
    set.add(
        "f404-last-tab-closes-window",
        act3 == Some((rw, 0, true)) && r.window_count() == 0,
        "",
    );

    // 非法下标与幽灵窗：诚实返回 None / NoWindow。
    set.add(
        "f404-tab-invalid-inputs",
        r.close_tab(rw, 0).is_none() && r.open_tab(rw, "X") == TabOpen::NoWindow,
        "",
    );

    // 最近路径环：容量 8 滘汰最旧；关标签不清访问史。
    let mut h = ExploreHot::new();
    let (hw, _) = h.hotkey_press(None);
    for p in ["P1", "P2", "P3", "P4", "P5", "P6", "P7", "P8", "P9"] {
        let _ = h.open_tab(hw, p);
        // 去重会让重复路径变跳转——这里路径全异，逐个开满。
        if h.windows[0].tabs.len() >= TAB_CAP {
            break;
        }
    }
    // 开了 9 个（含「此机」共 10 标签 < 32），环里应只剩最近 8 条。
    let rec = h.recent_of(hw);
    set.add(
        "f404-recent-ring-evicts",
        rec.len() == 8 && rec[0] == "P2" && rec[7] == "P9",
        "",
    );

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

    // ---- v7 深化单测 ----

    #[test]
    fn dedupe_jump_never_stacks_tabs() {
        let mut e = ExploreHot::new();
        let (w, _) = e.hotkey_press(None);
        for _ in 0..5 {
            let _ = e.open_tab(w, "S:/共享");
        }
        assert_eq!(e.windows[0].tabs, alloc::vec!["此机", "S:/共享"], "同路径只开一次");
        assert_eq!(e.windows[0].active, 1);
        // 访问史也只记首次入环（跳转不重复记）。
        assert_eq!(e.recent_of(w), &["S:/共享"]);
    }

    #[test]
    fn close_tab_activation_chain() {
        let mut e = ExploreHot::new();
        let (w, _) = e.hotkey_press(None);
        for p in ["T1", "T2", "T3", "T4"] {
            let _ = e.open_tab(w, p);
        }
        // 5 个标签：关最左（idx0）→ 右邻顶上。
        assert_eq!(e.close_tab(w, 0), Some((w, 0, false)));
        assert_eq!(e.windows[0].active, 0);
        // 关末位（idx3，共 4 个）→ 左邻接管。
        assert_eq!(e.close_tab(w, 3), Some((w, 2, false)));
        assert_eq!(e.windows[0].active, 2);
        // 关中位（idx1）→ 右邻顶上。
        assert_eq!(e.close_tab(w, 1), Some((w, 1, false)));
        // 剩 2 个：关末位 → 左邻接管；再关最后一个 → 关窗。
        assert_eq!(e.close_tab(w, 1), Some((w, 0, false)));
        assert_eq!(e.close_tab(w, 0), Some((w, 0, true)));
        assert_eq!(e.window_count(), 0);
    }

    #[test]
    fn tab_cap_is_honest_ceiling() {
        const PATHS: [&str; 40] = [
            "p0", "p1", "p2", "p3", "p4", "p5", "p6", "p7", "p8", "p9",
            "p10", "p11", "p12", "p13", "p14", "p15", "p16", "p17", "p18", "p19",
            "p20", "p21", "p22", "p23", "p24", "p25", "p26", "p27", "p28", "p29",
            "p30", "p31", "p32", "p33", "p34", "p35", "p36", "p37", "p38", "p39",
        ];
        let mut e = ExploreHot::new();
        let (w, _) = e.hotkey_press(None);
        for p in PATHS {
            match e.open_tab(w, p) {
                TabOpen::Opened(_, _) => {}
                TabOpen::CapFull => {
                    // 32 上限达成为止——其后每个新路径都诚实拒绝。
                    assert_eq!(e.windows[0].tabs.len(), TAB_CAP);
                    for q in PATHS.iter() {
                        if !e.windows[0].tabs.contains(q) {
                            assert_eq!(e.open_tab(w, q), TabOpen::CapFull, "{q} 必须被拒");
                        }
                    }
                    return;
                }
                other => panic!("中途不应出现 {other:?}"),
            }
        }
        panic!("40 条路径不足以触顶——TAB_CAP 被改动？");
    }
}
