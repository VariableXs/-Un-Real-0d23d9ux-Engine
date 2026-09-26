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
}

/// 全局 Esc 分发器。
pub struct EscDispatcher {
    pub stack: LayerStack,
    /// 响应超预算次数（诚实记账）。
    pub over_budget: u64,
    pub press_count: u64,
    /// 体验日志：每次 Esc 一条（第十三章纪律——事件可回放）。
    pub log: RingLog,
}

impl EscDispatcher {
    pub fn new() -> EscDispatcher {
        EscDispatcher {
            stack: LayerStack::new(),
            over_budget: 0,
            press_count: 0,
            log: RingLog::new(64),
        }
    }

    /// 开层（各功能把浮层/面板/对话框登记进来；Desktop 不登记——
    /// 它是空栈语义）。
    pub fn open_layer(&mut self, tier: LayerTier, name: &'static str) {
        self.stack.open(tier, name);
    }

    /// 按 Esc：按一次关一层（从最浮开始）；空栈 = 无动作。
    pub fn press_esc(&mut self, latency_ms: u64) -> EscOutcome {
        self.press_count += 1;
        if latency_ms > ESC_BUDGET_MS {
            self.over_budget += 1;
        }
        let closed = self.stack.close_top();
        let outcome = match closed {
            Some((tier, name)) => {
                let semantics = ESC_SEMANTICS
                    .iter()
                    .find(|(t, _, _)| *t == tier)
                    .map(|(_, _, s)| *s)
                    .unwrap_or("");
                EscOutcome { closed: Some((tier, name)), latency_ms, semantics }
            }
            None => EscOutcome { closed: None, latency_ms, semantics: "无动作（桌面态）" },
        };
        let verdict = if latency_ms > ESC_BUDGET_MS { "slow" } else { "" };
        self.log.push(0, "esc", if outcome.closed.is_some() { "peel" } else { "noop" }, verdict);
        outcome
    }

    /// 外点关闭指定层（浮层出路清单的另一出口——不改 Esc 语义）。
    pub fn outside_click_close(&mut self, name: &'static str) -> bool {
        self.stack.close_named(name)
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
}
