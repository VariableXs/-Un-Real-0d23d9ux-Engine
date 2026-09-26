//! F424 Esc 通用关闭语义 · 完整设计（STAR I 主册 G-I-24）。
//!
//! **判据（主册）**：语义层级表（四层优先级）；逐层剥离用例（浮层+
//! 面板+对话框叠三场景）；无副作用桌面态；响应 <100ms。＋通12。
//!
//! **设计要点（主册）**：Esc 的全局语义表（唯一且可预期）：关浮层
//! （菜单/Tooltip/预览 F339/符号面板 F313）→ 关非模态面板（快速设置/
//! 通知中心/日历飞出）→ 模态对话框取消（F207）→ 无动作（桌面态 Esc
//! 无副作用）；优先级从「最浮」到「最沉」逐层剥——按一次关一层，连按
//! 逐层退回；从不出现 Esc 一键把三层全关了或关错了层。
//!
//! 层级栈通用件在 [`ubase::LayerStack`]（一处一事实）；本模块是
//! **全局分发器**：把按键事件路由到四层语义表、统一 100ms 响应记账、
//! 维护「关闭的是哪层」的体验日志（视觉即时反馈的事件源）。
//!
//! 时间注入式（毫秒戳），无外部依赖。

use crate::checks::CheckSet;
use crate::uni1::ubase::{LayerStack, LayerTier, RingLog};

use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 规格常量（参数唯一源）
// ---------------------------------------------------------------------------

/// Esc 响应判线（ms）——「响应 <100ms」。
pub const ESC_BUDGET_MS: u64 = 100;

/// 四层语义表（唯一登记点，与 LayerTier 一一对应）。
/// （层级, 层类名, 该层 Esc 的语义）
pub const ESC_SEMANTICS: [(LayerTier, &str, &str); 4] = [
    (LayerTier::Popup, "浮层", "关浮层（菜单/Tooltip/预览/符号面板）"),
    (LayerTier::Panel, "非模态面板", "关面板（快速设置/通知中心/日历飞出）"),
    (LayerTier::Modal, "模态对话框", "取消（F207 语义）"),
    (LayerTier::Desktop, "桌面态", "无动作（无副作用）"),
];

/// 一次 Esc 分发结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EscOutcome {
    /// 关掉了哪层（None = 桌面态无动作）。
    pub closed: Option<(LayerTier, &'static str)>,
    /// 分发耗时（ms，调用方注入实测值——模块不虚构）。
    pub latency_ms: u64,
    /// 语义动作（人话，供体验日志与提示面）。
    pub semantics: &'static str,
    /// 本次是否取消了输入法组合（v7：组合期门命中）。
    pub ime_cancelled: bool,
}

/// 全局 Esc 分发器。
pub struct EscDispatcher {
    pub stack: LayerStack,
    /// 响应超预算次数（诚实记账）。
    pub over_budget: u64,
    pub press_count: u64,
    /// 体验日志：每次 Esc 一条（第十三章纪律——事件可回放）。
    pub log: RingLog,
    /// 输入法组合中（v7 深化）：组合期 Esc 优先取消组合——比任何浮层
    /// 都「浮」的一层（用户正在打字，误剥浮层会打断输入流）。
    pub composing: bool,
    pub ime_cancels: u64,
    /// 各层焦点归属登记（层名 → 关闭时应归还的焦点元素）。
    focus_registry: Vec<(&'static str, &'static str)>,
    /// 焦点归还次数（体验账——每次剥层归还一次）。
    pub focus_returns: u64,
    /// 最近一次归还的焦点归属（调用方据此执行真实聚焦）。
    pub last_focus_returned: Option<&'static str>,
}

impl EscDispatcher {
    pub fn new() -> EscDispatcher {
        EscDispatcher {
            stack: LayerStack::new(),
            over_budget: 0,
            press_count: 0,
            log: RingLog::new(64),
            composing: false,
            ime_cancels: 0,
            focus_registry: Vec::new(),
            focus_returns: 0,
            last_focus_returned: None,
        }
    }

    /// 开层（各功能把浮层/面板/对话框登记进来；Desktop 不登记——
    /// 它是空栈语义）。
    pub fn open_layer(&mut self, tier: LayerTier, name: &'static str) {
        self.stack.open(tier, name);
    }

    /// 开层并登记焦点归属（v7 深化）：该层关闭时焦点应归还给 owner
    /// ——键盘用户的焦点永不丢在宇宙里（第四章纪律的语义核落位）。
    pub fn open_layer_with_focus(&mut self, tier: LayerTier, name: &'static str, owner: &'static str) {
        self.stack.open(tier, name);
        self.focus_registry.push((name, owner));
    }

    /// 焦点归属查询（诊断面——层没关时归属还挂着）。
    pub fn focus_owner_of(&self, name: &str) -> Option<&'static str> {
        self.focus_registry.iter().find(|(n, _)| *n == name).map(|(_, o)| *o)
    }

    /// 输入法组合期登记/解除（v7 深化：组合是「比浮层更浮」的一层）。
    pub fn begin_composition(&mut self) {
        self.composing = true;
    }

    pub fn end_composition(&mut self) {
        self.composing = false;
    }

    /// 按 Esc：组合期优先取消组合（层不动）；否则按一次关一层
    /// （从最浮开始）；空栈 = 无动作。剥层时按登记归还焦点。
    pub fn press_esc(&mut self, latency_ms: u64) -> EscOutcome {
        self.press_count += 1;
        if latency_ms > ESC_BUDGET_MS {
            self.over_budget += 1;
        }
        if self.composing {
            self.composing = false;
            self.ime_cancels += 1;
            self.log.push(0, "esc", "ime-cancel", "");
            return EscOutcome {
                closed: None,
                latency_ms,
                semantics: "取消输入法组合（浮层不动）",
                ime_cancelled: true,
            };
        }
        let closed = self.stack.close_top();
        let outcome = match closed {
            Some((tier, name)) => {
                let semantics = ESC_SEMANTICS
                    .iter()
                    .find(|(t, _, _)| *t == tier)
                    .map(|(_, _, s)| *s)
                    .unwrap_or("");
                // 焦点归还：登记在案则归还并留账（第十四章——焦点不丢）。
                if let Some(pos) = self.focus_registry.iter().position(|(n, _)| *n == name) {
                    let (_, owner) = self.focus_registry.remove(pos);
                    self.focus_returns += 1;
                    self.last_focus_returned = Some(owner);
                }
                EscOutcome { closed: Some((tier, name)), latency_ms, semantics, ime_cancelled: false }
            }
            None => EscOutcome {
                closed: None,
                latency_ms,
                semantics: "无动作（桌面态）",
                ime_cancelled: false,
            },
        };
        let verdict = if latency_ms > ESC_BUDGET_MS { "slow" } else { "" };
        self.log.push(0, "esc", if outcome.closed.is_some() { "peel" } else { "noop" }, verdict);
        outcome
    }

    /// 外点关闭指定层（浮层出路清单的另一出口——不改 Esc 语义；
    /// 焦点归还与 Esc 同源——登记在案即归还）。
    pub fn outside_click_close(&mut self, name: &'static str) -> bool {
        let peeled = self.stack.close_named(name);
        if peeled {
            if let Some(pos) = self.focus_registry.iter().position(|(n, _)| *n == name) {
                let (_, owner) = self.focus_registry.remove(pos);
                self.focus_returns += 1;
                self.last_focus_returned = Some(owner);
            }
        }
        peeled
    }

    /// 不变量：栈内从底到顶 tier 递减（枚举序 Popup<Panel<Modal——
    /// 更浮的层在更上面；开发期断言面）。
    pub fn invariant_ok(&self) -> bool {
        let (tiers, _) = self.stack.as_slices();
        tiers.windows(2).all(|w| w[0] >= w[1])
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F424 自检。
pub fn run_escstack_checks() -> CheckSet {
    let mut set = CheckSet::new("uni1-F424");

    // 语义表：四层齐全且顺序 = 最浮到最沉。
    set.add(
        "f424-semantics-table",
        ESC_SEMANTICS.len() == 4
            && ESC_SEMANTICS[0].0 == LayerTier::Popup
            && ESC_SEMANTICS[1].0 == LayerTier::Panel
            && ESC_SEMANTICS[2].0 == LayerTier::Modal
            && ESC_SEMANTICS[3].0 == LayerTier::Desktop,
        "",
    );

    // 叠三场景逐层剥离：浮层 + 面板 + 对话框。
    let mut d = EscDispatcher::new();
    d.open_layer(LayerTier::Modal, "确认对话框");
    d.open_layer(LayerTier::Panel, "快速设置");
    d.open_layer(LayerTier::Popup, "右键菜单");
    let o1 = d.press_esc(40);
    let o2 = d.press_esc(35);
    let o3 = d.press_esc(30);
    set.add(
        "f424-peel-popup-then-panel-then-modal",
        o1.closed == Some((LayerTier::Popup, "右键菜单"))
            && o2.closed == Some((LayerTier::Panel, "快速设置"))
            && o3.closed == Some((LayerTier::Modal, "确认对话框")),
        "",
    );

    // 语义动作随层正确。
    set.add(
        "f424-semantics-per-tier",
        o1.semantics.contains("浮层") && o2.semantics.contains("面板") && o3.semantics.contains("取消"),
        "",
    );

    // 桌面态：无动作（无副作用）。
    let o4 = d.press_esc(20);
    set.add(
        "f424-desktop-noop",
        o4.closed.is_none() && o4.semantics.contains("无动作") && d.stack.depth() == 0,
        "",
    );

    // 不一键全关：三次按键才清三层（计数对拍）。
    set.add(
        "f424-one-layer-per-press",
        d.press_count == 4 && d.log.len() == 4,
        "",
    );

    // 响应 <100ms：达标零超线；120ms 如实计数。
    set.add("f424-latency-budget", d.over_budget == 0, "");
    let _ = d.press_esc(120);
    set.add("f424-latency-over-logged", d.over_budget == 1, "");

    // 外点关闭：只关目标层，Esc 语义不受污染。
    let mut d2 = EscDispatcher::new();
    d2.open_layer(LayerTier::Modal, "dlg");
    d2.open_layer(LayerTier::Popup, "menu");
    set.add(
        "f424-outside-close-targeted",
        d2.outside_click_close("menu") && d2.stack.top() == Some((LayerTier::Modal, "dlg")),
        "",
    );

    // 不变量：栈序自底向上不降（更浮在上）。
    let mut d3 = EscDispatcher::new();
    d3.open_layer(LayerTier::Modal, "a");
    d3.open_layer(LayerTier::Panel, "b");
    d3.open_layer(LayerTier::Popup, "c");
    set.add("f424-invariant-tier-order", d3.invariant_ok(), "");

    // ---- v7 深化：IME 组合期门 / 焦点归还登记表 ----

    // 组合期门：Esc 先取消组合（层不动、不计数剥层），再按才剥层。
    let mut g = EscDispatcher::new();
    g.open_layer(LayerTier::Popup, "menu");
    g.begin_composition();
    let o_g1 = g.press_esc(20);
    set.add(
        "f424-ime-gate-cancels-first",
        o_g1.ime_cancelled && o_g1.closed.is_none() && g.stack.depth() == 1 && g.ime_cancels == 1,
        "",
    );
    let o_g2 = g.press_esc(20);
    set.add(
        "f424-ime-gate-then-peels",
        !o_g2.ime_cancelled && o_g2.closed == Some((LayerTier::Popup, "menu")) && g.stack.is_empty(),
        "",
    );

    // 组合提交后 Esc 直剥层（门只在组合期生效）。
    let mut g2 = EscDispatcher::new();
    g2.open_layer(LayerTier::Popup, "menu2");
    g2.begin_composition();
    g2.end_composition();
    set.add(
        "f424-ime-gate-off-after-commit",
        g2.press_esc(20).closed == Some((LayerTier::Popup, "menu2")) && g2.ime_cancels == 0,
        "",
    );

    // 焦点归还：登记 → Esc 剥层 → 归还目标落账；未登记层不虚计。
    let mut f = EscDispatcher::new();
    f.open_layer_with_focus(LayerTier::Modal, "dlg", "doc.editor");
    f.open_layer(LayerTier::Popup, "tooltip"); // Tooltip 不接管焦点——无登记
    set.add("f424-focus-registry-lookup", f.focus_owner_of("dlg") == Some("doc.editor") && f.focus_owner_of("tooltip").is_none(), "");
    let _ = f.press_esc(20); // 剥 tooltip——无归还
    set.add("f424-focus-return-only-registered", f.focus_returns == 0 && f.last_focus_returned.is_none(), "");
    let _ = f.press_esc(20); // 剥 dlg——归还
    set.add(
        "f424-focus-return-on-peel",
        f.focus_returns == 1 && f.last_focus_returned == Some("doc.editor") && f.focus_owner_of("dlg").is_none(),
        "",
    );

    // 外点关闭同源归还：焦点不因出路不同而丢。
    let mut f2 = EscDispatcher::new();
    f2.open_layer_with_focus(LayerTier::Panel, "quickset", "taskbar.clock");
    set.add(
        "f424-focus-return-on-outside",
        f2.outside_click_close("quickset") && f2.last_focus_returned == Some("taskbar.clock") && f2.focus_returns == 1,
        "",
    );

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn peel_one_per_press_never_all() {
        let mut d = EscDispatcher::new();
        d.open_layer(LayerTier::Modal, "m");
        d.open_layer(LayerTier::Panel, "p");
        d.open_layer(LayerTier::Popup, "u");
        // 一键只关一层。
        assert_eq!(d.press_esc(10).closed.map(|(_, n)| n), Some("u"));
        assert_eq!(d.stack.depth(), 2, "Esc 不得越层");
        assert_eq!(d.press_esc(10).closed.map(|(_, n)| n), Some("p"));
        assert_eq!(d.stack.depth(), 1);
        assert_eq!(d.press_esc(10).closed.map(|(_, n)| n), Some("m"));
        // 第四按：桌面态无动作，无 panic 无翻状态。
        let o = d.press_esc(10);
        assert!(o.closed.is_none());
        assert_eq!(d.press_count, 4);
    }

    #[test]
    fn outside_click_never_breaks_esc() {
        let mut d = EscDispatcher::new();
        d.open_layer(LayerTier::Modal, "m");
        d.open_layer(LayerTier::Popup, "menu-a");
        d.open_layer(LayerTier::Popup, "menu-b");
        // 外点关 menu-a（中间层）——栈顶 menu-b 保留。
        assert!(d.outside_click_close("menu-a"));
        assert_eq!(d.stack.top().map(|(_, n)| n), Some("menu-b"));
        // Esc 继续按最浮语义走。
        assert_eq!(d.press_esc(10).closed.map(|(_, n)| n), Some("menu-b"));
        assert_eq!(d.press_esc(10).closed.map(|(_, n)| n), Some("m"));
    }

    #[test]
    fn latency_accounting_honest() {
        let mut d = EscDispatcher::new();
        d.open_layer(LayerTier::Popup, "x");
        assert!(d.press_esc(ESC_BUDGET_MS).latency_ms == ESC_BUDGET_MS);
        assert_eq!(d.over_budget, 0);
        d.open_layer(LayerTier::Popup, "y");
        d.press_esc(ESC_BUDGET_MS + 1);
        assert_eq!(d.over_budget, 1);
    }

    #[test]
    fn log_records_every_press() {
        let mut d = EscDispatcher::new();
        d.open_layer(LayerTier::Popup, "x");
        d.press_esc(10);
        d.press_esc(10); // 桌面态
        let snap = d.log.snapshot();
        assert_eq!(snap.len(), 2);
        assert_eq!(snap[0].what, "peel");
        assert_eq!(snap[1].what, "noop");
    }

    // ---- v7 深化单测 ----

    #[test]
    fn ime_gate_ordering_with_layers() {
        // 组合 + 三层叠：Esc 序 = 取消组合 → 浮层 → 面板 → 对话框 → 桌面。
        let mut d = EscDispatcher::new();
        d.open_layer(LayerTier::Modal, "dlg");
        d.open_layer(LayerTier::Panel, "panel");
        d.open_layer(LayerTier::Popup, "menu");
        d.begin_composition();
        assert!(d.press_esc(10).ime_cancelled);
        assert!(!d.press_esc(10).ime_cancelled);
        assert_eq!(d.press_esc(10).closed.map(|(_, n)| n), Some("panel"));
        assert_eq!(d.press_esc(10).closed.map(|(_, n)| n), Some("dlg"));
        let last = d.press_esc(10);
        assert!(last.closed.is_none() && !last.ime_cancelled && last.semantics.contains("无动作"));
        assert_eq!(d.ime_cancels, 1);
        assert_eq!(d.press_count, 5);
    }

    #[test]
    fn composition_can_toggle_repeatedly() {
        let mut d = EscDispatcher::new();
        d.begin_composition();
        assert!(d.press_esc(10).ime_cancelled);
        // 组合再开再取消——每次都走门。
        d.begin_composition();
        assert!(d.press_esc(10).ime_cancelled);
        assert_eq!(d.ime_cancels, 2);
        // 空栈 + 非组合：桌面态无动作。
        let o = d.press_esc(10);
        assert!(o.closed.is_none() && !o.ime_cancelled);
    }

    #[test]
    fn focus_never_lost_across_mixed_closes() {
        let mut d = EscDispatcher::new();
        d.open_layer_with_focus(LayerTier::Modal, "save-ask", "doc.editor");
        d.open_layer(LayerTier::Panel, "notify");
        d.open_layer_with_focus(LayerTier::Popup, "ctxmenu", "explorer.list");
        // 外点关 ctxmenu → 焦点回 explorer.list。
        assert!(d.outside_click_close("ctxmenu"));
        assert_eq!(d.last_focus_returned, Some("explorer.list"));
        // Esc 关 notify（无登记）→ 焦点账不动。
        d.press_esc(10);
        assert_eq!(d.focus_returns, 1);
        // Esc 关 save-ask → 焦点回 doc.editor。
        d.press_esc(10);
        assert_eq!(d.last_focus_returned, Some("doc.editor"));
        assert_eq!(d.focus_returns, 2);
        assert!(d.focus_registry.is_empty(), "登记表随层清空——无悬挂归属");
    }
}
