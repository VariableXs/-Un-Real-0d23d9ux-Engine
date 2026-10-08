//! F405 Alt+F4 与桌面关机菜单 · 完整设计（STAR I 主册 G-I-05）。
//!
//! **判据（主册）**：焦点判定（窗/桌面/模态三场景）；桌面电源菜单三
//! 选项与默认值；三问联动；关闭语义与 × 一致性。＋通12。
//!
//! **设计要点（主册）**：Alt+F4 关闭当前窗口（焦点窗关闭语义与点 ×
//! 完全一致——F310 未保存三问照走）；焦点在桌面时 Alt+F4 弹电源菜单
//! （睡眠/关机/重启三选，默认关机，方向键切换 Enter 确认）；组合键在
//! 模态弹窗中无效（F384 陷阱优先）。
//!
//! 本模块是 Alt+F4 **分发语义核**：三场景焦点判定 → 三种结果（关窗/
//! 弹菜单/无动作），关窗必经 F310 三问钩子（快捷键不绕过安全网），
//! 电源菜单状态机（三选项、默认关机、方向键循环、Enter 确认、Esc 取消）。
//!
//! 时间注入式（毫秒戳），无外部依赖。

use crate::checks::CheckSet;

use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 规格常量（参数唯一源）
// ---------------------------------------------------------------------------

/// 电源菜单三选项（顺序钉死：睡眠/关机/重启——与 Windows 经典顺序一致）。
pub const POWER_MENU_ITEMS: [&str; 3] = ["睡眠", "关机", "重启"];

/// 默认选中项下标：关机（判据「默认关机」）。
pub const POWER_MENU_DEFAULT: usize = 1;

/// 电源动作确认倒计时（ms）——破坏性动作（关机/重启）必须按住确认，
/// 中途松手/取消即作废（F310 安全网精神在电源路径的同源落位）。
/// 睡眠可逆，不设倒计时。
pub const POWER_CONFIRM_MS: u64 = 2_000;

/// 关机/重启需要确认，睡眠即按即行（唯一登记点）。
pub fn needs_confirm(action: &str) -> bool {
    action == "关机" || action == "重启"
}

// ---------------------------------------------------------------------------
// v7 深化：多窗关闭链
// ---------------------------------------------------------------------------

/// 关闭链的一步。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChainStep {
    /// 该窗有脏数据——三问挂起（链头停住等裁决）。
    Ask(u64),
    /// 该窗干净/已裁决放行——正常关闭。
    Closing(u64),
    /// 链走完。
    Done,
}

/// 多窗关闭链状态机：Alt+F4 在多窗场景逐个关（**从栈顶开始**——
/// `begin` 的 ids 自底向上登记，链从末位（栈顶）弹出），每窗独立过
/// F310 三问；取消一次 = 余下全部放弃（不静默继续）。
pub struct CloseChain {
    pending: Vec<u64>,
    /// 三问挂起中的窗（Some = 链头停住）。
    pub active_ask: Option<u64>,
    pub asked: u64,
    pub closed: u64,
    /// 取消时被放弃的窗数（诚实账——取消不是无声无息）。
    pub abandoned: u64,
    pub done: bool,
}

impl CloseChain {
    pub fn begin(ids: &[u64]) -> CloseChain {
        CloseChain {
            pending: ids.to_vec(),
            active_ask: None,
            asked: 0,
            closed: 0,
            abandoned: 0,
            done: ids.is_empty(),
        }
    }

    pub fn remaining(&self) -> usize {
        self.pending.len() + if self.active_ask.is_some() { 1 } else { 0 }
    }

    /// 推进一步：弹出链头窗，脏 → 三问挂起；干净 → 关闭。
    pub fn step(&mut self, head_dirty: bool) -> ChainStep {
        if self.done {
            return ChainStep::Done;
        }
        let id = match self.pending.pop() {
            Some(id) => id,
            None => {
                self.done = true;
                return ChainStep::Done;
            }
        };
        if head_dirty {
            self.asked += 1;
            self.active_ask = Some(id);
            ChainStep::Ask(id)
        } else {
            self.closed += 1;
            if self.pending.is_empty() {
                self.done = true;
            }
            ChainStep::Closing(id)
        }
    }

    /// 三问裁决：放行 → 关闭该窗并继续；取消 → 余下全部放弃（链终止）。
    pub fn resolve_ask(&mut self, proceed: bool) -> ChainStep {
        let id = match self.active_ask.take() {
            Some(id) => id,
            None => return ChainStep::Done,
        };
        if proceed {
            self.closed += 1;
            if self.pending.is_empty() {
                self.done = true;
            }
            ChainStep::Closing(id)
        } else {
            self.abandoned += self.pending.len() as u64;
            self.pending.clear();
            self.done = true;
            ChainStep::Done
        }
    }
}

// ---------------------------------------------------------------------------
// v7 深化：电源动作确认倒计时
// ---------------------------------------------------------------------------

/// 确认倒计时状态机（按住确认语义的时序核）。
pub struct PowerConfirm {
    /// 确认中的动作（None = 空闲）。
    pub action: Option<&'static str>,
    pub started_at_ms: Option<u64>,
    pub cancelled: u64,
    pub fired: u64,
}

impl PowerConfirm {
    pub fn new() -> PowerConfirm {
        PowerConfirm { action: None, started_at_ms: None, cancelled: 0, fired: 0 }
    }

    /// 开始确认（菜单 Enter 后对需确认动作调用）。进行中重开 = 重新计时。
    pub fn begin(&mut self, action: &'static str, now_ms: u64) -> bool {
        if !needs_confirm(action) {
            return false;
        }
        self.action = Some(action);
        self.started_at_ms = Some(now_ms);
        true
    }

    /// 进度（千分比——0..=1000，供按住进度环渲染）。
    pub fn progress_permille(&self, now_ms: u64) -> u64 {
        match (self.action, self.started_at_ms) {
            (Some(_), Some(t0)) => {
                let elapsed = now_ms.saturating_sub(t0);
                (elapsed * 1000 / POWER_CONFIRM_MS).min(1000)
            }
            _ => 0,
        }
    }

    /// 是否已达确认线（按满 2s）。
    pub fn ready(&self, now_ms: u64) -> bool {
        self.progress_permille(now_ms) >= 1000
    }

    /// 提前松手/取消：作废（诚实计数，不静默复燃）。
    pub fn cancel(&mut self) -> bool {
        if self.action.is_some() {
            self.action = None;
            self.started_at_ms = None;
            self.cancelled += 1;
            true
        } else {
            false
        }
    }

    /// 按满确认线 → 触发动作（取走后清态）。
    pub fn take(&mut self, now_ms: u64) -> Option<&'static str> {
        if self.ready(now_ms) {
            let a = self.action.take();
            self.started_at_ms = None;
            if a.is_some() {
                self.fired += 1;
            }
            a
        } else {
            None
        }
    }
}

/// 焦点场景。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FocusScene {
    /// 焦点在普通窗口。
    Window,
    /// 焦点在桌面。
    Desktop,
    /// 焦点在模态弹窗（F384 陷阱优先——Alt+F4 在此无效）。
    Modal,
}

/// Alt+F4 的三种合法结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AltF4Result {
    /// 关窗（经 F310 三问钩子）。
    CloseWindow(u64),
    /// 弹出桌面电源菜单。
    PowerMenu,
    /// 无动作（模态陷阱/无焦点）。
    Noop,
}

/// 关窗请求的裁决（F310 三问钩子的返回）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CloseVerdict {
    /// 无脏数据直接关。
    Clean,
    /// 有未保存内容——三问已弹，等待用户三选一。
    AskPending,
    /// 用户选保存/放弃后允许关。
    Proceed,
    /// 用户取消——不关。
    Cancel,
}

/// 电源菜单状态机。
pub struct PowerMenu {
    pub open: bool,
    pub selected: usize,
    /// 打开次数/取消次数（体验账）。
    pub opened: u64,
    pub cancelled: u64,
}

impl PowerMenu {
    pub fn new() -> PowerMenu {
        PowerMenu { open: false, selected: POWER_MENU_DEFAULT, opened: 0, cancelled: 0 }
    }

    /// 打开：选中复位为默认（关机）——判据「默认关机」逐次兑现。
    pub fn open(&mut self) {
        self.open = true;
        self.selected = POWER_MENU_DEFAULT;
        self.opened += 1;
    }

    /// 方向键切换（下 = +1 循环，上 = -1 循环）。
    pub fn move_sel(&mut self, delta: i32) {
        if !self.open {
            return;
        }
        let n = POWER_MENU_ITEMS.len() as i32;
        self.selected = ((self.selected as i32 + delta).rem_euclid(n)) as usize;
    }

    /// Enter 确认：返回选中项并关菜单。
    pub fn confirm(&mut self) -> Option<&'static str> {
        if !self.open {
            return None;
        }
        self.open = false;
        Some(POWER_MENU_ITEMS[self.selected])
    }

    /// Esc 取消。
    pub fn cancel(&mut self) -> bool {
        if !self.open {
            return false;
        }
        self.open = false;
        self.cancelled += 1;
        true
    }
}

/// Alt+F4 分发器。
pub struct AltF4Dispatch {
    pub menu: PowerMenu,
    /// 三问钩子触发次数（关窗请求中带脏标记的数量——判据「三问联动」）。
    pub ask_triggered: u64,
    /// × 与 Alt+F4 走同一关窗口的次数（语义一致性账）。
    pub close_via_same_path: u64,
}

impl AltF4Dispatch {
    pub fn new() -> AltF4Dispatch {
        AltF4Dispatch { menu: PowerMenu::new(), ask_triggered: 0, close_via_same_path: 0 }
    }

    /// 分发 Alt+F4。
    ///
    /// - `dirty`：焦点窗是否有未保存内容（F310 三问的输入）；
    /// - `modal_open`：模态陷阱是否在顶（F384——陷阱优先，组合键无效）。
    pub fn alt_f4(&mut self, scene: FocusScene, focused_window: Option<u64>, dirty: bool, modal_open: bool) -> AltF4Result {
        // 模态陷阱优先于一切（F384）。
        if modal_open || scene == FocusScene::Modal {
            return AltF4Result::Noop;
        }
        match scene {
            FocusScene::Window => match focused_window {
                Some(id) => {
                    if dirty {
                        self.ask_triggered += 1;
                        return AltF4Result::CloseWindow(id); // 调用方收到后必须走三问
                    }
                    self.close_via_same_path += 1;
                    AltF4Result::CloseWindow(id)
                }
                None => AltF4Result::Noop,
            },
            FocusScene::Desktop => {
                self.menu.open();
                AltF4Result::PowerMenu
            }
            FocusScene::Modal => AltF4Result::Noop,
        }
    }

    /// 三问裁决回填（F310 三选一 → 关窗语义与 × 一致——同一记账路径）。
    pub fn resolve_ask(&mut self, verdict: CloseVerdict) -> bool {
        match verdict {
            CloseVerdict::Proceed | CloseVerdict::Clean => {
                self.close_via_same_path += 1;
                true
            }
            CloseVerdict::Cancel | CloseVerdict::AskPending => false,
        }
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F405 自检。
pub fn run_altf4_checks() -> CheckSet {
    let mut set = CheckSet::new("uni1-F405");

    // 常量钉死：三选项顺序与默认值。
    set.add(
        "f405-menu-const",
        POWER_MENU_ITEMS == ["睡眠", "关机", "重启"] && POWER_MENU_DEFAULT == 1,
        "",
    );

    // 场景一：焦点窗干净 → 关窗，与 × 同路径记账。
    let mut d = AltF4Dispatch::new();
    let r1 = d.alt_f4(FocusScene::Window, Some(7), false, false);
    set.add(
        "f405-window-clean-close",
        r1 == AltF4Result::CloseWindow(7) && d.close_via_same_path == 1 && d.ask_triggered == 0,
        "",
    );

    // 场景二：焦点窗脏 → 三问触发（快捷键不绕过安全网）。
    let r2 = d.alt_f4(FocusScene::Window, Some(7), true, false);
    set.add(
        "f405-dirty-triple-ask",
        r2 == AltF4Result::CloseWindow(7) && d.ask_triggered == 1 && d.close_via_same_path == 1,
        "",
    );
    // 用户取消 → 不关。
    set.add("f405-ask-cancel-no-close", !d.resolve_ask(CloseVerdict::Cancel) && d.close_via_same_path == 1, "");
    // 用户选保存 → 关，同一记账路径（与 × 一致性）。
    set.add("f405-ask-proceed-close", d.resolve_ask(CloseVerdict::Proceed) && d.close_via_same_path == 2, "");

    // 场景三：焦点桌面 → 电源菜单，默认选中关机。
    let r3 = d.alt_f4(FocusScene::Desktop, None, false, false);
    set.add(
        "f405-desktop-power-menu",
        r3 == AltF4Result::PowerMenu && d.menu.open && d.menu.selected == POWER_MENU_DEFAULT,
        "",
    );

    // 菜单键盘流：方向键循环 + Enter 确认 + Esc 取消。
    let mut m = PowerMenu::new();
    m.open();
    m.move_sel(1); // 关机 → 重启
    set.add("f405-menu-move-down", m.selected == 2, "");
    m.move_sel(1); // 重启 → 睡眠（循环）
    set.add("f405-menu-wrap", m.selected == 0, "");
    m.move_sel(-1); // 睡眠 → 重启? 0-1 → 2（循环到尾）
    set.add("f405-menu-wrap-up", m.selected == 2, "");
    set.add("f405-menu-confirm", m.confirm() == Some("重启") && !m.open, "");
    m.open();
    m.move_sel(-1); // 默认1 → 0 睡眠
    set.add("f405-menu-esc-cancel", m.cancel() && !m.open && m.cancelled == 1, "");
    set.add("f405-menu-confirm-closed-noop", m.confirm().is_none(), "");

    // 场景四：模态陷阱优先——组合键无效（F384）。
    let mut d2 = AltF4Dispatch::new();
    let r4 = d2.alt_f4(FocusScene::Modal, Some(9), false, true);
    let r5 = d2.alt_f4(FocusScene::Window, Some(9), false, true);
    set.add(
        "f405-modal-trap-priority",
        r4 == AltF4Result::Noop && r5 == AltF4Result::Noop && d2.menu.opened == 0,
        "",
    );

    // ---- v7 深化：多窗关闭链 / 电源确认倒计时 ----

    // 关闭链从栈顶开始（ids 自底向上登记，末位先关）：干净窗直关；
    // 脏窗三问挂起；放行后链继续。
    let mut ch = CloseChain::begin(&[1, 2, 3]);
    set.add(
        "f405-chain-top-first-clean",
        ch.step(false) == ChainStep::Closing(3) && ch.step(false) == ChainStep::Closing(2),
        "",
    );
    set.add("f405-chain-dirty-ask", ch.step(true) == ChainStep::Ask(1) && ch.asked == 1, "");
    set.add(
        "f405-chain-resume-after-proceed",
        ch.resolve_ask(true) == ChainStep::Closing(1) && ch.closed == 3 && ch.done,
        "",
    );

    // 取消一次 = 余下全放弃：取消计入诚实账（不无声继续）。
    let mut cx = CloseChain::begin(&[6, 7, 8, 9]);
    let _ = cx.step(false); // 9（栈顶）关
    let _ = cx.step(true); // 8 三问
    set.add(
        "f405-chain-cancel-abandons-rest",
        cx.resolve_ask(false) == ChainStep::Done && cx.abandoned == 2 && cx.closed == 1 && cx.done,
        "",
    );

    // 空链：begin 即 Done，step 永远 Done（不 panic）。
    let mut ce = CloseChain::begin(&[]);
    set.add(
        "f405-chain-empty-done",
        ce.done && ce.step(false) == ChainStep::Done && ce.resolve_ask(true) == ChainStep::Done,
        "",
    );

    // 确认倒计时：关机/重启需要（睡眠免），进度千分比 + 按满才触发。
    set.add(
        "f405-confirm-needs",
        needs_confirm("关机") && needs_confirm("重启") && !needs_confirm("睡眠"),
        "",
    );
    let mut pc = PowerConfirm::new();
    set.add("f405-confirm-sleep-no-hold", !pc.begin("睡眠", 1_000), "");
    set.add("f405-confirm-begin", pc.begin("关机", 1_000) && pc.progress_permille(1_000) == 0, "");
    set.add("f405-confirm-half", pc.progress_permille(2_000) == 500 && !pc.ready(2_000), "");
    set.add("f405-confirm-not-ready-take", pc.take(1_999).is_none(), "");
    set.add("f405-confirm-ready-take", pc.ready(3_000) && pc.take(3_000) == Some("关机") && pc.action.is_none() && pc.fired == 1, "");

    // 提前松手：作废且不静默复燃（取消计数）。
    let mut pz = PowerConfirm::new();
    let _ = pz.begin("重启", 5_000);
    set.add(
        "f405-confirm-cancel-honest",
        pz.cancel() && !pz.cancel() && pz.cancelled == 1 && pz.progress_permille(9_999) == 0,
        "",
    );

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn three_scenes_matrix() {
        let mut d = AltF4Dispatch::new();
        // 窗 → 关窗；桌面 → 菜单；模态 → 无动作。
        assert_eq!(d.alt_f4(FocusScene::Window, Some(1), false, false), AltF4Result::CloseWindow(1));
        assert_eq!(d.alt_f4(FocusScene::Desktop, None, false, false), AltF4Result::PowerMenu);
        assert_eq!(d.alt_f4(FocusScene::Modal, Some(1), false, true), AltF4Result::Noop);
        // 焦点窗缺失 → 无动作（不 panic）。
        assert_eq!(d.alt_f4(FocusScene::Window, None, false, false), AltF4Result::Noop);
    }

    #[test]
    fn menu_selection_cycle_both_directions() {
        let mut m = PowerMenu::new();
        m.open();
        // 默认关机；向下两次到重启再一次回睡眠。
        assert_eq!(m.selected, 1);
        m.move_sel(1);
        m.move_sel(1);
        assert_eq!(m.selected, 0);
        // 向上一次回重启（循环到尾）。
        m.move_sel(-1);
        assert_eq!(m.selected, 2);
        assert_eq!(m.confirm(), Some("重启"));
        assert!(!m.open);
        // 关闭后方向键与确认均无动作。
        m.move_sel(1);
        assert!(m.confirm().is_none());
    }

    #[test]
    fn dirty_window_never_closes_silently() {
        let mut d = AltF4Dispatch::new();
        let _ = d.alt_f4(FocusScene::Window, Some(3), true, false);
        assert_eq!(d.ask_triggered, 1);
        assert_eq!(d.close_via_same_path, 0, "三问未裁决前不得关");
        // 挂起态也不算关。
        assert!(!d.resolve_ask(CloseVerdict::AskPending));
        assert_eq!(d.close_via_same_path, 0);
    }

    // ---- v7 深化单测 ----

    #[test]
    fn close_chain_every_dirty_window_asks() {
        // 全脏链（ids 自底向上，从栈顶 12 开始关）：每个窗都过三问，无一绕过。
        let mut ch = CloseChain::begin(&[10, 11, 12]);
        assert_eq!(ch.step(true), ChainStep::Ask(12));
        assert_eq!(ch.resolve_ask(true), ChainStep::Closing(12));
        assert_eq!(ch.step(true), ChainStep::Ask(11));
        assert_eq!(ch.resolve_ask(true), ChainStep::Closing(11));
        assert_eq!(ch.step(true), ChainStep::Ask(10));
        assert_eq!(ch.resolve_ask(true), ChainStep::Closing(10));
        assert!(ch.done);
        assert_eq!((ch.asked, ch.closed, ch.abandoned), (3, 3, 0));
        // 链完后再 step / resolve 都是 Done（不 panic 不翻账）。
        assert_eq!(ch.step(false), ChainStep::Done);
        assert_eq!(ch.resolve_ask(true), ChainStep::Done);
    }

    #[test]
    fn confirm_countdown_full_lifecycle() {
        let mut pc = PowerConfirm::new();
        assert!(pc.begin("重启", 0));
        // 边界：恰满 2s 达线；差 1ms 不达。
        assert!(!pc.ready(POWER_CONFIRM_MS - 1));
        assert!(pc.ready(POWER_CONFIRM_MS));
        assert_eq!(pc.take(POWER_CONFIRM_MS), Some("重启"));
        // 触发后空态：进度归零、再 take 无动作。
        assert_eq!(pc.progress_permille(9_999), 0);
        assert!(pc.take(9_999).is_none());
        // 重开 = 重新计时（重置起点）。
        assert!(pc.begin("关机", 20_000));
        assert_eq!(pc.progress_permille(21_000), 500);
        assert!(pc.cancel());
        assert_eq!(pc.progress_permille(25_000), 0, "取消后不复燃");
    }
}
