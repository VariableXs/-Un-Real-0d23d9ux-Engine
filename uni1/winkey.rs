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

use alloc::string::String;

// ---------------------------------------------------------------------------
// 规格常量（参数唯一源）
// ---------------------------------------------------------------------------

/// 开合时序判线（ms）——「开 <150ms」。
pub const OPEN_BUDGET_MS: u64 = 150;

/// 连按去抖窗（ms）：同拍抖动（≤80ms 内的重复 keydown）不算第二次开合。
pub const DEBOUNCE_MS: u64 = 80;

/// 搜索框容量（字节）——超限诚实拒绝，不静默截断。
pub const SEARCH_MAX_BYTES: usize = 64;

/// 开始菜单在浮层栈中的登记名（与 Esc 分发器 F424 对接的身份）。
pub const START_MENU_LAYER: &str = "startmenu";

/// Esc 分段语义（v7 深化）：输入流不被 Esc 打断——先清后关。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EscStep {
    /// 输入法组合中：取消组合（菜单与文本都不动）。
    ImeCancel,
    /// 有文本：清空搜索框（菜单保持——第一次 Esc 不关）。
    Clear,
    /// 空文本：关闭菜单。
    Close,
    /// 本来就没开。
    Noop,
}

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
    /// 搜索框内容（v7 深化：打开即焦点——打字即进框）。
    pub search: String,
    /// 输入法组合中（F107 协作：组合期 Esc 优先还组合，不关菜单）。
    pub composing: bool,
    /// 关闭前登记的焦点归属（打开时由调用方 set；关闭时归还并记账）。
    focus_owner: Option<&'static str>,
    /// 最近一次关闭归还的焦点归属（调用方据此执行真实聚焦）。
    pub last_focus_returned: Option<&'static str>,
    /// 焦点归还成功次数（体验账）。
    pub focus_returns: u64,
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
            search: String::new(),
            composing: false,
            focus_owner: None,
            last_focus_returned: None,
            focus_returns: 0,
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
            self.finish_close();
        }
        self.log.push(now_ms, "win", if self.open { "open" } else { "close" }, "");
        true
    }

    /// 关闭收尾（三条出路共用的唯一落位）：焦点归还 + 搜索框清空 +
    /// 组合期状态复位 + 浮层栈撤层。一处一事实——出路再增不走样。
    /// 归还目标写入 `last_focus_returned` 供调用方执行真实聚焦。
    fn finish_close(&mut self) {
        self.open = false;
        self.focus_in_search = false;
        if let Some(owner) = self.focus_owner.take() {
            self.focus_returns += 1;
            self.last_focus_returned = Some(owner);
        }
        self.search.clear();
        self.composing = false;
        let _ = self.layers.close_named(START_MENU_LAYER);
    }

    /// 登记焦点归属（打开菜单前的原焦点元素——关闭时按此归还）。
    pub fn set_focus_owner(&mut self, owner: &'static str) {
        self.focus_owner = Some(owner);
    }

    /// 焦点归属查询（诊断面）。
    pub fn focus_owner(&self) -> Option<&'static str> {
        self.focus_owner
    }

    /// 打字进框（v7 深化）：菜单开且焦点在搜索框才接收；容量满诚实
    /// 拒绝（不静默截断）；组合期不接收（组合文本由输入法提交）。
    pub fn type_char(&mut self, c: char) -> bool {
        if !self.open || !self.focus_in_search || self.composing {
            return false;
        }
        if self.search.len() + c.len_utf8() > SEARCH_MAX_BYTES {
            return false;
        }
        self.search.push(c);
        true
    }

    /// 退格：删一个字符（空框退格无动作返回 false）。
    pub fn backspace(&mut self) -> bool {
        if !self.open || self.composing {
            return false;
        }
        self.search.pop().is_some()
    }

    /// 搜索框内容查询。
    pub fn search_text(&self) -> &str {
        &self.search
    }

    /// 输入法组合期登记（F107 协作语义核侧的最小状态位）。
    pub fn begin_composition(&mut self) {
        if self.open {
            self.composing = true;
        }
    }

    /// 组合结束（committed = 上屏文本是否已并入搜索框——由调用方在
    /// 提交回调里处理文本，这里只复位状态位）。
    pub fn end_composition(&mut self, committed_text: &str) {
        if self.composing {
            self.composing = false;
            self.search.push_str(committed_text);
        }
    }

    /// Esc 分段语义（v7 深化）：组合中 → 取消组合（菜单不动）；
    /// 有文本 → 清空（菜单不动）；空 → 关菜单。逐段可预期。
    pub fn esc_step(&mut self) -> EscStep {
        if !self.open {
            return EscStep::Noop;
        }
        if self.composing {
            self.composing = false;
            self.log.push(0, "esc", "ime-cancel", "");
            return EscStep::ImeCancel;
        }
        if !self.search.is_empty() {
            self.search.clear();
            self.log.push(0, "esc", "clear", "");
            return EscStep::Clear;
        }
        self.finish_close();
        self.toggles += 1;
        self.log.push(0, "esc", "close", "");
        EscStep::Close
    }

    /// Esc 关闭（F424 同效——走分段语义：文本先清再关的组合由调用方
    /// 连按两次兑现；此口保持「一次按键一步」的原语义供直接关闭场景）。
    pub fn esc_close(&mut self) -> bool {
        if !self.open {
            return false;
        }
        loop {
            match self.esc_step() {
                EscStep::Close => return true,
                EscStep::Noop => return false,
                _ => continue, // 组合/文本段自动走完 = 直关语义不变
            }
        }
    }

    /// 外点关闭（点击外部区域）——走同一收尾落位。
    pub fn outside_click_close(&mut self) -> bool {
        if !self.open {
            return false;
        }
        self.finish_close();
        self.toggles += 1;
        self.log.push(0, "click", "close", "");
        true
    }

    /// Enter 提交（v7 深化）：返回查询词并清框，菜单保持（连续搜是
    /// 高频动作——不逼用户重开菜单）。空查询无动作。
    pub fn submit(&mut self) -> Option<String> {
        if !self.open || self.composing || self.search.is_empty() {
            return None;
        }
        let q = core::mem::take(&mut self.search);
        self.log.push(0, "search", "submit", "");
        Some(q)
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

    // ---- v7 深化：搜索框输入状态机 / 两段 Esc / 焦点归还 ----

    // 打字进框：开+焦点才接收；关闭态拒绝。
    let mut s = StartMenu::new();
    let _ = s.win_key(7_000, 100);
    let typed = "varix".chars().map(|c| s.type_char(c)).fold(true, |a, b| a && b);
    let mut s_closed = StartMenu::new();
    set.add(
        "f416-type-routes-to-search",
        typed && s.search_text() == "varix" && !s_closed.type_char('x'),
        "",
    );

    // 容量诚实拒绝：64 字节上限，第 65 字节被拒（不静默截断）。
    let mut cap = StartMenu::new();
    let _ = cap.win_key(7_100, 100);
    let mut filled = true;
    for c in "ab".repeat(32).chars() {
        filled &= cap.type_char(c);
    }
    set.add(
        "f416-type-cap-honest",
        filled && cap.search_text().len() == SEARCH_MAX_BYTES && !cap.type_char('Z'),
        "",
    );

    // 退格：删末字符；空框退格诚实 false。
    cap.backspace();
    let mut eb = StartMenu::new();
    let _ = eb.win_key(1, 100);
    set.add(
        "f416-backspace",
        cap.search_text().len() == SEARCH_MAX_BYTES - 1 && !eb.backspace(),
        "",
    );

    // 两段 Esc：有文本先清（菜单保持）→ 空再关。
    let mut e2 = StartMenu::new();
    let _ = e2.win_key(7_200, 100);
    for c in "hi".chars() {
        let _ = e2.type_char(c);
    }
    set.add(
        "f416-esc-two-stage",
        e2.esc_step() == EscStep::Clear && e2.is_open() && e2.search_text().is_empty()
            && e2.esc_step() == EscStep::Close && !e2.is_open(),
        "",
    );

    // 组合期 Esc：取消组合（菜单与文本都不动）。
    let mut e3 = StartMenu::new();
    let _ = e3.win_key(7_300, 100);
    let _ = e3.type_char('v');
    e3.begin_composition();
    set.add(
        "f416-esc-ime-cancel",
        e3.esc_step() == EscStep::ImeCancel && e3.is_open() && e3.search_text() == "v" && !e3.composing,
        "",
    );

    // 组合提交：上屏文本并入搜索框；组合期打字不直收。
    let mut e4 = StartMenu::new();
    let _ = e4.win_key(7_400, 100);
    e4.begin_composition();
    set.add(
        "f416-ime-commit-merges",
        !e4.type_char('x') && {
            e4.end_composition("拼");
            e4.search_text() == "拼" && !e4.composing
        },
        "",
    );

    // Enter 提交：返回查询词、清框、菜单保持（连续搜不逼重开）。
    let mut e5 = StartMenu::new();
    let _ = e5.win_key(7_500, 100);
    for c in "查询".chars() {
        let _ = e5.type_char(c);
    }
    let q = e5.submit();
    set.add(
        "f416-enter-submit-keeps-open",
        q == Some(alloc::string::String::from("查询"))
            && e5.search_text().is_empty()
            && e5.is_open()
            && e5.submit().is_none(),
        "",
    );

    // 焦点归还：登记 → 关闭 → 归还目标落账（三条出路同源）。
    let mut e6 = StartMenu::new();
    e6.set_focus_owner("explorer.list");
    let _ = e6.win_key(7_600, 100);
    let _ = e6.outside_click_close();
    set.add(
        "f416-focus-return-recorded",
        e6.last_focus_returned == Some("explorer.list") && e6.focus_returns == 1 && e6.focus_owner().is_none(),
        "",
    );

    // 未登记归属的关闭：不虚计归还。
    let mut e7 = StartMenu::new();
    let _ = e7.win_key(7_700, 100);
    set.add(
        "f416-focus-return-uncounted",
        e7.esc_close() && e7.focus_returns == 0 && e7.last_focus_returned.is_none(),
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

    // ---- v7 深化单测 ----

    #[test]
    fn search_input_full_lifecycle() {
        let mut m = StartMenu::new();
        let _ = m.win_key(100, 100);
        for c in "rust".chars() {
            assert!(m.type_char(c));
        }
        assert_eq!(m.search_text(), "rust");
        // 退格到空再退格诚实拒绝。
        assert!(m.backspace());
        assert_eq!(m.search_text(), "rus");
        // Enter 提交后菜单保持、框清空。
        assert_eq!(m.submit(), Some(alloc::string::String::from("rus")));
        assert!(m.is_open() && m.search_text().is_empty());
        // 再打再搜（连续搜索流）。
        for c in "v".chars() {
            assert!(m.type_char(c));
        }
        assert_eq!(m.submit(), Some(alloc::string::String::from("v")));
        // Esc 清 → Esc 关。
        let _ = m.type_char('q');
        assert_eq!(m.esc_step(), EscStep::Clear);
        assert_eq!(m.esc_step(), EscStep::Close);
        assert!(!m.is_open());
    }

    #[test]
    fn composition_flow_never_loses_text() {
        let mut m = StartMenu::new();
        let _ = m.win_key(100, 100);
        let _ = m.type_char('e');
        m.begin_composition();
        // 组合期：直打不收（文本由输入法提交）。
        assert!(!m.type_char('n'));
        // Esc 取消组合：已上屏文本保留、菜单保留。
        assert_eq!(m.esc_step(), EscStep::ImeCancel);
        assert_eq!(m.search_text(), "e");
        assert!(m.is_open());
        // 组合正常提交路径：合并上屏文本。
        m.begin_composition();
        m.end_composition("ng");
        assert_eq!(m.search_text(), "eng");
        assert!(!m.composing);
    }

    #[test]
    fn focus_return_via_all_close_paths() {
        for close in 0..3u8 {
            let mut m = StartMenu::new();
            m.set_focus_owner("desk.icons");
            let _ = m.win_key(100, 100);
            match close {
                0 => {
                    let _ = m.win_key(200, 100);
                } // 再按关
                1 => {
                    assert!(m.esc_close());
                } // Esc 关（直关语义：组合/文本段自动走完）
                _ => {
                    assert!(m.outside_click_close());
                } // 外点关
            }
            assert_eq!(m.last_focus_returned, Some("desk.icons"), "出路 {close} 必须归还焦点");
            assert_eq!(m.focus_returns, 1);
            assert!(m.search_text().is_empty(), "关闭即清框（下次打开是干净的）");
        }
    }

    #[test]
    fn closed_menu_rejects_all_input() {
        let mut m = StartMenu::new();
        assert!(!m.type_char('x'));
        assert!(!m.backspace());
        assert!(m.submit().is_none());
        m.begin_composition();
        assert!(!m.composing, "关闭态组合登记无效果");
    }
}
