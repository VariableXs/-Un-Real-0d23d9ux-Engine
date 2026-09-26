//! F416 Win 键开合开始菜单 · 完整设计（STAR I 主册 G-I-16）。
//!
//! **判据（主册）**：开合时序（<150ms）；焦点落点；打字冲突零丢失；
//! Esc/外点关闭；连按稳定性。＋通12。
//!
//! **设计要点（主册）**：Win 键单击=开/关开始菜单（再按即关、Esc 同效
//! F424）；Win 键不与任何输入冲突（打字中按 Win 立即响应候选窗让位
//! F107）；开始菜单打开时焦点落搜索框（直接打字即搜 F071——打开菜单
//! 的下一步 80% 是找东西，直接给）；点击外部区域关闭。
//!
//! 本模块是开合**状态机**：开/关/焦点落点/打字冲突账（零丢失计数）/
//! 连按去抖（同拍连按只算一次开合——稳定性判据）/外点关闭。菜单本体
//! 与搜索属 F071/F417。
//!
//! 时间注入式（毫秒戳），无外部依赖。

use crate::checks::CheckSet;
use crate::uni1::ubase::{LayerStack, LayerTier, RingLog};

// ---------------------------------------------------------------------------
// 规格常量（参数唯一源）
// ---------------------------------------------------------------------------

/// 开合时序判线（ms）——「开 <150ms」。
pub const OPEN_BUDGET_MS: u64 = 150;

/// 连按去抖窗（ms）：同拍抖动（≤80ms 内的重复 keydown）不算第二次开合。
pub const DEBOUNCE_MS: u64 = 80;

/// 开始菜单在浮层栈中的登记名（与 Esc 分发器 F424 对接的身份）。
pub const START_MENU_LAYER: &str = "startmenu";

/// 开始菜单状态机。
pub struct StartMenu {
    pub open: bool,
    /// 焦点落点：打开即搜索框（判据「焦点落点」）。
    pub focus_in_search: bool,
    /// 打字冲突零丢失账：菜单打开/关闭瞬间被吞的键（必须恒 0）。
    pub typing_lost: u64,
    /// 最近一次开/关动作耗时（超预算如实计数）。
    pub last_toggle_ms: Option<u64>,
    pub over_budget: u64,
    /// 上次 Win 键时刻（去抖窗）。
    last_win_at_ms: Option<u64>,
    /// 开合次数（体验账）。
    pub toggles: u64,
    /// 体验日志（可回放最近开合事件）。
    pub log: RingLog,
    /// 浮层栈（与 F424 对接：菜单作为一层 Popup 登记）。
    pub layers: LayerStack,
}

impl StartMenu {
    pub fn new() -> StartMenu {
        StartMenu {
            open: false,
            focus_in_search: false,
            typing_lost: 0,
            last_toggle_ms: None,
            over_budget: 0,
            last_win_at_ms: None,
            toggles: 0,
            log: RingLog::new(32),
            layers: LayerStack::new(),
        }
    }

    /// Win 键按下：去抖后开/关（<150ms 判线的实测值由调用方注入）。
    pub fn win_key(&mut self, now_ms: u64, latency_ms: u64) -> bool {
        // 连按去抖：DEBOUNCE_MS 内的重复按下忽略（稳定性判据）。
        if let Some(t) = self.last_win_at_ms {
            if now_ms.saturating_sub(t) <= DEBOUNCE_MS {
                return false; // 抖动，不动作（不吞状态）。
            }
        }
        self.last_win_at_ms = Some(now_ms);
        self.toggles += 1;
        self.last_toggle_ms = Some(latency_ms);
        if latency_ms > OPEN_BUDGET_MS {
            self.over_budget += 1;
        }
        self.open = !self.open;
        if self.open {
            // 打开：焦点直接落搜索框（F071 直达——不打字先定位）。
            self.focus_in_search = true;
            self.layers.open(LayerTier::Popup, START_MENU_LAYER);
        } else {
            self.focus_in_search = false;
            let _ = self.layers.close_named(START_MENU_LAYER);
        }
        self.log.push(now_ms, "win", if self.open { "open" } else { "close" }, "");
        true
    }

    /// Esc 关闭（F424 同效——走浮层栈语义）。
    pub fn esc_close(&mut self) -> bool {
        if !self.open {
            return false;
        }
        let peeled = self.layers.close_named(START_MENU_LAYER);
        if peeled {
            self.open = false;
            self.focus_in_search = false;
            self.toggles += 1;
            self.log.push(0, "esc", "close", "");
        }
        peeled
    }

    /// 外点关闭（点击外部区域）。
    pub fn outside_click_close(&mut self) -> bool {
        if !self.open {
            return false;
        }
        let peeled = self.layers.close_named(START_MENU_LAYER);
        if peeled {
            self.open = false;
            self.focus_in_search = false;
            self.toggles += 1;
            self.log.push(0, "click", "close", "");
        }
        peeled
    }

    /// 打字冲突账：菜单开合瞬间每个键击都必须送达（搜索框或原输入处）。
    /// 调用方逐键上报「该键最终去向」，任何丢失在此记账——判据
    /// 「打字冲突零丢失」的硬账本。
    pub fn report_keystroke(&mut self, delivered: bool) {
        if !delivered {
            self.typing_lost += 1;
        }
    }

    /// 与输入法协作（F107 候选窗让位）：打开菜单时候选窗让位事件数。
    /// 让位 = 菜单接管焦点，候选窗收起——不算打字丢失。
    pub fn ime_yield_on_open(&self) -> bool {
        self.open && self.focus_in_search
    }

    pub fn is_open(&self) -> bool {
        self.open
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F416 自检。
pub fn run_winkey_checks() -> CheckSet {
    let mut set = CheckSet::new("uni1-F416");

    // 开 <150ms：120ms 达标。
    let mut m = StartMenu::new();
    set.add(
        "f416-open-under-150",
        m.win_key(1_000, 120) && m.is_open() && m.focus_in_search && m.over_budget == 0,
        "",
    );

    // 再按即关：状态翻回、焦点归还、浮层栈清空。
    set.add(
        "f416-toggle-close",
        m.win_key(1_400, 60) && !m.is_open() && !m.focus_in_search && m.layers.is_empty(),
        "",
    );

    // 连按稳定性：80ms 去抖窗内抖动不算第二次开合。
    let mut m2 = StartMenu::new();
    let _ = m2.win_key(2_000, 100);
    let jitter = m2.win_key(2_060, 100); // 60ms 后抖动
    set.add(
        "f416-debounce",
        !jitter && m2.is_open() && m2.toggles == 1,
        "",
    );
    let real = m2.win_key(2_200, 100); // 200ms 后真按 = 关
    set.add("f416-real-second-press", real && !m2.is_open() && m2.toggles == 2, "");

    // Esc 同效关闭。
    let mut m3 = StartMenu::new();
    let _ = m3.win_key(3_000, 100);
    set.add(
        "f416-esc-close",
        m3.esc_close() && !m3.is_open() && !m3.esc_close(),
        "",
    );

    // 外点关闭。
    let mut m4 = StartMenu::new();
    let _ = m4.win_key(4_000, 100);
    set.add(
        "f416-outside-click-close",
        m4.outside_click_close() && !m4.is_open() && !m4.outside_click_close(),
        "",
    );

    // 打字冲突零丢失：全程逐键送达账恒零。
    let mut m5 = StartMenu::new();
    for i in 0..20 {
        let _ = m5.win_key(5_000 + i * 200, 100);
        // 每次开合瞬间各有一个键击，全部送达。
        m5.report_keystroke(true);
    }
    set.add("f416-typing-zero-lost", m5.typing_lost == 0 && m5.toggles == 20, "");

    // 超预算诚实记账：180ms 超线计数。
    let mut m6 = StartMenu::new();
    let _ = m6.win_key(6_000, 180);
    set.add("f416-over-budget-logged", m6.over_budget == 1 && m6.last_toggle_ms == Some(180), "");

    // 输入法让位：打开态候选窗让位成立。
    set.add("f416-ime-yield", m6.ime_yield_on_open(), "");

    // 体验日志可回放：开/关事件成对。
    let snap = m5.log.snapshot();
    set.add(
        "f416-log-pairs",
        snap.len() == 20 && snap.iter().filter(|e| e.what == "open").count() == 10,
        "",
    );

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_close_cycle_with_focus() {
        let mut m = StartMenu::new();
        assert!(m.win_key(100, 120));
        assert!(m.is_open());
        assert!(m.focus_in_search, "打开即搜索框焦点（80% 下一步是找东西）");
        assert!(m.ime_yield_on_open());
        assert!(m.win_key(500, 80));
        assert!(!m.is_open());
        assert!(!m.focus_in_search);
        assert!(m.layers.is_empty(), "菜单层必须从浮层栈撤干净");
    }

    #[test]
    fn rapid_jitter_stable() {
        let mut m = StartMenu::new();
        assert!(m.win_key(1_000, 100));
        // 10ms/30ms/70ms 的机械抖动全部忽略。
        for dt in [10, 30, 70] {
            assert!(!m.win_key(1_000 + dt, 100), "去抖窗内抖动不得触发");
        }
        assert!(m.is_open(), "抖动不改变状态");
        assert_eq!(m.toggles, 1);
        // 去抖窗外真按有效。
        assert!(m.win_key(1_200, 100));
        assert!(!m.is_open());
    }

    #[test]
    fn three_close_paths_all_work() {
        // 再按 / Esc / 外点 —— 三条出路各自独立可用。
        let mut m = StartMenu::new();
        let _ = m.win_key(100, 100);
        assert!(m.win_key(200, 80) && !m.is_open());

        let mut m2 = StartMenu::new();
        let _ = m2.win_key(100, 100);
        assert!(m2.esc_close() && !m2.is_open());

        let mut m3 = StartMenu::new();
        let _ = m3.win_key(100, 100);
        assert!(m3.outside_click_close() && !m3.is_open());
    }

    #[test]
    fn budget_honest_accounting() {
        let mut m = StartMenu::new();
        assert!(m.win_key(100, OPEN_BUDGET_MS)); // 恰达线
        assert_eq!(m.over_budget, 0);
        assert!(m.win_key(1_000, OPEN_BUDGET_MS + 1));
        assert_eq!(m.over_budget, 1);
    }

    #[test]
    fn typing_never_lost_during_toggles() {
        let mut m = StartMenu::new();
        // 开合 50 次期间模拟 50 个键击全部送达。
        for i in 0..50u64 {
            let _ = m.win_key(i * 300, 100);
            m.report_keystroke(true);
        }
        assert_eq!(m.typing_lost, 0, "打字冲突零丢失是硬判据");
        // 一次丢失即记账（红线可见）。
        m.report_keystroke(false);
        assert_eq!(m.typing_lost, 1);
    }
}
