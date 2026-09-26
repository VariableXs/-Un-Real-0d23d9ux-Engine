//! H2 窗口编排 · 深化批次四（z 序层带 + 激活链 + 多屏 z 序保持——
//! F276/F277/F278 窗口面的编排深化）。
//!
//! **承接判据**（主册 H 域正文，一处一事实）：
//! - **F277 多显示器任务栏**：焦点屏判定（焦点切到副屏 200ms 内）
//!   ——本层产出焦点链事件（激活→焦点屏变更→任务栏按钮归属），
//!   multitb 的归属策略消费此事件流；
//! - **F278 投影/显示模式**：黑屏恢复路径——仅第二屏模式下主屏
//!   交互入口保留：z 序层带保证「交互入口浮在黑屏层之上」是结构
//!   可表达的（专用层带，不靠坐标巧合）；
//! - **F226 精神（H1 车道，经 F277 锚落位）**：私自置顶审计——
//!   置顶只能经 `raise_pinned` 通道（参数带授权票根），无票根的
//!   置顶请求在编排层被拒并留账（越界审计=0 的机制保证）；
//! - **F082 车道（经 F277 锚）**：Alt+Tab 遍历序 = z 序顶层带内
//!   按最近激活时间排序——切换器不做自己的 z 序，只读本层账。
//!
//! 时间纪律：激活时刻由调用方注入；层带升降是纯函数。

use crate::checks::CheckSet;

use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 层带模型
// ---------------------------------------------------------------------------

/// z 序层带（四带固定——私设第五带即缺陷）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Band {
    /// 最底：桌面壁纸/小组件。
    Desktop,
    /// 常规窗口。
    Normal,
    /// 系统交互入口（F278 仅第二屏黑屏模式下主屏保留的入口——
    /// 恒在 Normal 之上，保证可交互）。
    SystemEntry,
    /// 浮层（菜单/弹窗/拖影）。
    Floating,
}

/// 一扇受管窗口：id + 层带 + 所属屏 + 最近激活时刻（毫秒戳）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ManagedWin {
    pub id: u64,
    pub band: Band,
    pub screen: u8,
    /// 最近激活时刻（Alt+Tab 序的数据源）。
    pub last_active_ms: u64,
}

/// z 序账：窗口集合 + 层带内稳定排序（同层带按最近激活降序）。
pub struct ZOrder {
    wins: Vec<ManagedWin>,
    /// 拒绝的越权置顶（审计账——私自置顶=0 的对账面）。
    pub rejected_pins: Vec<u64>,
}

impl ZOrder {
    pub fn new() -> ZOrder {
        ZOrder { wins: Vec::new(), rejected_pins: Vec::new() }
    }

    pub fn len(&self) -> usize {
        self.wins.len()
    }

    pub fn is_empty(&self) -> bool {
        self.wins.is_empty()
    }

    /// 登记（层带必选，无默认层带——新窗口不进 Normal 就显式说）。
    pub fn admit(&mut self, w: ManagedWin) {
        self.wins.push(w);
    }

    /// 移除（结束清算车道——h2appctl Teardown 消费）。
    pub fn remove(&mut self, id: u64) -> bool {
        let before = self.wins.len();
        self.wins.retain(|w| w.id != id);
        self.wins.len() != before
    }

    /// 置顶（授权票根制）：`ticket` 必须匹配窗口 id 的派生票根
    /// （票根 = id ^ 0x5A5A——演示用确定性派生，实现侧换接安全
    /// 票根源）。无票根 → 拒绝留账，z 序不动。
    pub fn raise_pinned(&mut self, id: u64, ticket: u64) -> bool {
        let expect = id ^ 0x5A5A;
        if ticket != expect {
            self.rejected_pins.push(id);
            return false;
        }
        match self.wins.iter_mut().find(|w| w.id == id) {
            Some(w) => {
                w.band = Band::Floating;
                true
            }
            None => false,
        }
    }

    /// 遍历序（z 序语义）：层带升序（Desktop 最先画）、同带按最近
    /// 激活降序（后激活在上）。Alt+Tab 直接取顶层带这一段。
    pub fn paint_order(&self) -> Vec<u64> {
        let mut ws = self.wins.clone();
        ws.sort_by(|a, b| a.band.cmp(&b.band).then(b.last_active_ms.cmp(&a.last_active_ms)));
        ws.iter().map(|w| w.id).collect()
    }

    /// Alt+Tab 序：仅 SystemEntry+Normal+Floating 带，最近激活优先
    /// （桌面层不带参——切不进壁纸）。
    pub fn alt_tab_order(&self) -> Vec<u64> {
        let mut ws: Vec<ManagedWin> = self
            .wins
            .iter()
            .filter(|w| w.band != Band::Desktop)
            .cloned()
            .collect();
        ws.sort_by(|a, b| b.last_active_ms.cmp(&a.last_active_ms));
        ws.iter().map(|w| w.id).collect()
    }

    /// 某屏的窗口 ids（F277 按钮归属判定输入）。
    pub fn on_screen(&self, screen: u8) -> Vec<u64> {
        let mut ids: Vec<u64> =
            self.wins.iter().filter(|w| w.screen == screen).map(|w| w.id).collect();
        ids.sort_unstable();
        ids
    }

    /// 激活：更新最近激活时刻（z 序与 Alt+Tab 序随之变化）。
    pub fn activate(&mut self, id: u64, now_ms: u64) -> bool {
        match self.wins.iter_mut().find(|w| w.id == id) {
            Some(w) => {
                w.last_active_ms = now_ms;
                true
            }
            None => false,
        }
    }
}

// ---------------------------------------------------------------------------
// 焦点链（F277 焦点屏判定的事件源）
// ---------------------------------------------------------------------------

/// 焦点事件。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FocusEvent {
    /// 窗口激活（点击/Alt+Tab/程序请求）。
    Activate { win: u64, screen: u8 },
    /// 焦点丢失（窗口销毁/最小化——焦点还给次顶窗）。
    Lost { win: u64 },
}

/// 焦点屏判定预算（F277 判据：焦点切到副屏 200ms 内——判定时延
/// 预算常量，超线由消费方计账）。
pub const FOCUS_SWITCH_BUDGET_MS: u32 = 200;

/// 焦点链：单点焦点 + 事件流（任务栏/切换器/编排三方同读一账）。
pub struct FocusChain {
    /// 当前焦点窗口与所在屏。
    pub focused: Option<(u64, u8)>,
    pub events: Vec<FocusEvent>,
}

impl FocusChain {
    pub fn new() -> FocusChain {
        FocusChain { focused: None, events: Vec::new() }
    }

    /// 激活事件：焦点变更才记（同窗同屏重复激活不刷事件——账干净）。
    pub fn activate(&mut self, win: u64, screen: u8) -> bool {
        if self.focused == Some((win, screen)) {
            return false;
        }
        self.focused = Some((win, screen));
        self.events.push(FocusEvent::Activate { win, screen });
        true
    }

    /// 焦点丢失：次顶窗接管（由调用方传入接管者——本层只记账）。
    pub fn lost(&mut self, win: u64, heir: Option<(u64, u8)>) {
        if self.focused.map(|(f, _)| f) == Some(win) {
            self.focused = heir;
            self.events.push(FocusEvent::Lost { win });
        }
    }

    /// 当前焦点屏（F277 焦点屏判定的唯一出口）。
    pub fn focus_screen(&self) -> Option<u8> {
        self.focused.map(|(_, s)| s)
    }
}

// ---------------------------------------------------------------------------
// 自检（判据逐条钉死）
// ---------------------------------------------------------------------------

pub fn run_h2zorder_checks() -> CheckSet {
    let mut set = CheckSet::new("h2-h2zorder");
    // 层带序：Desktop 最先画、Floating 最后画；同带最近激活在上。
    let mut z = ZOrder::new();
    z.admit(ManagedWin { id: 1, band: Band::Normal, screen: 0, last_active_ms: 100 });
    z.admit(ManagedWin { id: 2, band: Band::Floating, screen: 0, last_active_ms: 90 });
    z.admit(ManagedWin { id: 3, band: Band::Normal, screen: 1, last_active_ms: 300 });
    z.admit(ManagedWin { id: 4, band: Band::Desktop, screen: 0, last_active_ms: 50 });
    set.add(
        "h2zorder band order",
        z.paint_order() == vec![4, 3, 1, 2],
        "desktop→normal(recent first)→floating",
    );
    // Alt+Tab 序：桌面层带除外，纯最近激活序。
    set.add(
        "h2zorder alt-tab",
        z.alt_tab_order() == vec![3, 1, 2] && !z.alt_tab_order().contains(&4),
        "desktop not switchable",
    );
    // 激活改写两序：1 触碰后成为常规带最新、全局最近激活。
    z.activate(1, 500);
    set.add(
        "h2zorder activate reorders",
        z.paint_order() == vec![4, 1, 3, 2] && z.alt_tab_order()[0] == 1,
        "recent wins both orders",
    );
    // 私自置顶拦截：无票根拒 + 留账 + z 序不动；带票根放行。
    let ok_pin = z.raise_pinned(1, 0);
    set.add(
        "h2zorder pin audited",
        !ok_pin && z.rejected_pins == vec![1] && z.paint_order()[3] != 1,
        "no ticket no raise",
    );
    set.add("h2zorder pin with ticket", z.raise_pinned(1, 1 ^ 0x5A5A), "ticketed raise ok");
    // 移除 + 按屏查询（F277 归属输入）。
    set.add(
        "h2zorder screen query",
        z.on_screen(1) == vec![3] && z.on_screen(0) == vec![1, 2, 4] && z.remove(4),
        "per-screen ids sorted",
    );
    set.add("h2zorder remove honest", !z.remove(999), "missing id honest");
    // 焦点链：变更才记事件、重复激活不刷、焦点屏出口、丢失 heir 接管。
    let mut fc = FocusChain::new();
    let e1 = fc.activate(3, 1);
    let e2 = fc.activate(3, 1);
    set.add(
        "h2zorder focus dedupe",
        e1 && !e2 && fc.events.len() == 1 && fc.focus_screen() == Some(1),
        "same target no event",
    );
    fc.activate(1, 0);
    fc.lost(1, Some((3, 1)));
    set.add(
        "h2zorder focus heir",
        fc.focus_screen() == Some(1) && matches!(fc.events[2], FocusEvent::Lost { win: 1 }),
        "heir takes over",
    );
    set.add(
        "h2zorder focus budget",
        FOCUS_SWITCH_BUDGET_MS == 200,
        "200ms line",
    );
    // F278 黑屏路径：仅第二屏模式下主屏 SystemEntry 带恒可交互
    // （结构表达：入口带高于 Normal，黑屏只是 Normal 带无内容）。
    let mut z2 = ZOrder::new();
    z2.admit(ManagedWin { id: 10, band: Band::SystemEntry, screen: 0, last_active_ms: 10 });
    z2.admit(ManagedWin { id: 11, band: Band::Normal, screen: 0, last_active_ms: 20 });
    set.add(
        "h2zorder black-screen entry",
        z2.paint_order()[1] == 10,
        "entry above normal",
    );
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn h2zorder_all_green() {
        let set = run_h2zorder_checks();
        assert!(set.all_passed(), "h2zorder 自检有红项");
        assert!(!set.truncated(), "h2zorder 自检溢出");
    }

    #[test]
    fn pin_audits_never_silent() {
        // 越权置顶 50 次：每次都被拒且都留账（审计账不丢事件）。
        let mut z = ZOrder::new();
        z.admit(ManagedWin { id: 7, band: Band::Normal, screen: 0, last_active_ms: 0 });
        for _ in 0..50 {
            assert!(!z.raise_pinned(7, 0));
        }
        assert_eq!(z.rejected_pins.len(), 50);
        assert_eq!(z.wins[0].band, Band::Normal);
    }

    #[test]
    fn alt_tab_skips_empty_gracefully() {
        // 只有桌面层带：Alt+Tab 空序（不假装有窗可切）。
        let mut z = ZOrder::new();
        z.admit(ManagedWin { id: 1, band: Band::Desktop, screen: 0, last_active_ms: 0 });
        assert!(z.alt_tab_order().is_empty());
    }
}
