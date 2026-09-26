//! F407 Win+I 设置快捷 · 完整设计（STAR I 主册 G-I-07）。
//!
//! **判据（主册）**：三场景（未开/已开/已开在搜索）行为；单例聚焦；
//! 搜索框焦点与光标就绪（直接可打字）；注册表登记。＋通12。
//!
//! **设计要点（主册）**：Win+I 直达设置中心：已在设置中心时按 Win+I
//! 聚焦搜索框（F301）——第二次按是「帮我找」不是「再开一个」；从其他
//! 应用按则聚焦已有设置窗（单例策略 F282）。
//!
//! 本模块是单例聚焦**语义核**：设置窗单例表（最多一窗）、三场景分发、
//! 搜索框焦点/光标就绪状态、聚焦延迟记账（「即时」判线 150ms——与
//! F416 开合时序同档，常量在此唯一登记）。注册表登记复用 ubase
//! HotkeyTable（F244 唯一落位）。
//!
//! 时间注入式（毫秒戳），无外部依赖。

use crate::checks::CheckSet;
use crate::uni1::ubase::{Chord, HotkeyTable, MOD_WIN};

// ---------------------------------------------------------------------------
// 规格常量（参数唯一源）
// ---------------------------------------------------------------------------

/// 聚焦/打开延迟判线（ms）——「即时」口径（与 F416 开合时序同档）。
pub const FOCUS_BUDGET_MS: u64 = 150;

/// 设置窗单例 ID（单例策略：永远只有这一个窗口身份）。
pub const SETTINGS_WINDOW_ID: u64 = 0xF707;

/// 设置窗三态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingsState {
    /// 未开。
    Closed,
    /// 已开（焦点在页面）。
    OpenFocusedPage,
    /// 已开（焦点已在搜索框）。
    OpenFocusedSearch,
}

/// Win+I 语义核。
pub struct SettingsHot {
    pub state: SettingsState,
    /// 搜索框光标就绪（可打字——判据「直接可打字」）。
    pub search_caret_ready: bool,
    /// 最近一次动作耗时账（超预算如实计数）。
    pub last_action_ms: Option<u64>,
    pub over_budget: u64,
    pub action_count: u64,
    /// 注册表登记位（F244）。
    pub hotkeys: HotkeyTable,
}

impl SettingsHot {
    pub fn new() -> SettingsHot {
        let mut hotkeys = HotkeyTable::new();
        let _ = hotkeys.register("f407.settings", Chord::new(MOD_WIN, b'I'));
        SettingsHot {
            state: SettingsState::Closed,
            search_caret_ready: false,
            last_action_ms: None,
            over_budget: 0,
            action_count: 0,
            hotkeys,
        }
    }

    /// 按 Win+I：三场景分发。返回本次动作的窗口 ID（单例恒定）。
    pub fn hotkey_press(&mut self, latency_ms: u64) -> u64 {
        self.action_count += 1;
        self.last_action_ms = Some(latency_ms);
        if latency_ms > FOCUS_BUDGET_MS {
            self.over_budget += 1;
        }
        match self.state {
            SettingsState::Closed => {
                // 未开 → 打开并聚焦搜索框（打开菜单的下一步 80% 是找东西
                // ——与 F416/F071 同一心智，搜索框直接就绪可打字）。
                self.state = SettingsState::OpenFocusedSearch;
                self.search_caret_ready = true;
            }
            SettingsState::OpenFocusedPage => {
                // 已开 → 聚焦搜索框（不是再开一个——单例聚焦）。
                self.state = SettingsState::OpenFocusedSearch;
                self.search_caret_ready = true;
            }
            SettingsState::OpenFocusedSearch => {
                // 已开在搜索 → 保持（焦点不动，光标就绪不动摇）。
                self.search_caret_ready = true;
            }
        }
        SETTINGS_WINDOW_ID
    }

    /// 用户点进某设置页：焦点离开搜索框（页面聚焦态）。
    pub fn focus_page(&mut self) {
        if self.state != SettingsState::Closed {
            self.state = SettingsState::OpenFocusedPage;
            self.search_caret_ready = false;
        }
    }

    /// 关窗。
    pub fn close(&mut self) {
        self.state = SettingsState::Closed;
        self.search_caret_ready = false;
    }

    pub fn is_open(&self) -> bool {
        self.state != SettingsState::Closed
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F407 自检。
pub fn run_sethot_checks() -> CheckSet {
    let mut set = CheckSet::new("uni1-F407");

    // 注册表登记（F244）。
    let mut s = SettingsHot::new();
    set.add(
        "f407-hotkey-registered",
        s.hotkeys.lookup(Chord::new(MOD_WIN, b'I')) == Some("f407.settings"),
        "",
    );

    // 场景一（未开）：打开 + 搜索框焦点 + 光标就绪（直接可打字）。
    let wid = s.hotkey_press(120);
    set.add(
        "f407-scene-closed-opens",
        s.state == SettingsState::OpenFocusedSearch
            && s.search_caret_ready
            && wid == SETTINGS_WINDOW_ID,
        "",
    );

    // 场景二（已开在页面）：聚焦搜索框，单例不重开。
    s.focus_page();
    set.add("f407-page-focus", s.state == SettingsState::OpenFocusedPage && !s.search_caret_ready, "");
    let wid2 = s.hotkey_press(130);
    set.add(
        "f407-scene-open-focuses-search",
        wid2 == SETTINGS_WINDOW_ID
            && s.state == SettingsState::OpenFocusedSearch
            && s.search_caret_ready,
        "",
    );

    // 场景三（已开在搜索）：保持——「第二次按是帮我找不是再开一个」。
    let wid3 = s.hotkey_press(140);
    set.add(
        "f407-scene-search-keeps",
        wid3 == SETTINGS_WINDOW_ID && s.state == SettingsState::OpenFocusedSearch && s.search_caret_ready,
        "",
    );

    // 单例：无论按几次，窗口身份唯一（无第二窗可造）。
    for _ in 0..5 {
        let w = s.hotkey_press(100);
        if w != SETTINGS_WINDOW_ID {
            set.add("f407-singleton", false, "non-singleton id");
            break;
        }
    }
    set.add("f407-singleton", s.action_count >= 8, "");

    // 延迟记账：150ms 内全绿；超线如实计数。
    set.add(
        "f407-latency-budget",
        s.over_budget == 0 && s.last_action_ms == Some(100),
        "",
    );
    s.hotkey_press(999);
    set.add("f407-latency-over-logged", s.over_budget == 1, "");

    // 关窗后回到未开态。
    s.close();
    set.add(
        "f407-close-then-reopen",
        !s.is_open() && {
            s.hotkey_press(120);
            s.state == SettingsState::OpenFocusedSearch
        },
        "",
    );

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn three_scenes_matrix() {
        let mut s = SettingsHot::new();
        // 未开 → 开+搜索。
        assert_eq!(s.hotkey_press(120), SETTINGS_WINDOW_ID);
        assert_eq!(s.state, SettingsState::OpenFocusedSearch);
        // 点进页面。
        s.focus_page();
        assert_eq!(s.state, SettingsState::OpenFocusedPage);
        // 已开 → 聚焦搜索（单例）。
        assert_eq!(s.hotkey_press(120), SETTINGS_WINDOW_ID);
        assert_eq!(s.state, SettingsState::OpenFocusedSearch);
        // 已开在搜索 → 保持。
        assert_eq!(s.hotkey_press(120), SETTINGS_WINDOW_ID);
        assert_eq!(s.state, SettingsState::OpenFocusedSearch);
    }

    #[test]
    fn singleton_never_second_window() {
        let mut s = SettingsHot::new();
        let ids: Vec<u64> = (0..10).map(|_| s.hotkey_press(100)).collect();
        assert!(ids.iter().all(|i| *i == SETTINGS_WINDOW_ID));
    }

    #[test]
    fn caret_ready_invariant() {
        let mut s = SettingsHot::new();
        // 搜索态光标永远就绪；页面态永远不就绪；关窗清空。
        s.hotkey_press(100);
        assert!(s.search_caret_ready);
        s.focus_page();
        assert!(!s.search_caret_ready);
        s.hotkey_press(100);
        assert!(s.search_caret_ready);
        s.close();
        assert!(!s.search_caret_ready);
    }

    #[test]
    fn latency_honest() {
        let mut s = SettingsHot::new();
        s.hotkey_press(FOCUS_BUDGET_MS); // 恰好达线 = 达标
        assert_eq!(s.over_budget, 0);
        s.hotkey_press(FOCUS_BUDGET_MS + 1);
        assert_eq!(s.over_budget, 1);
    }
}
