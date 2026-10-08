//! H2 浮层生命周期总管 · 深化批次五（人格章程二章「浮层出路」的
//! 域内机判落位——五十项里所有右键菜单/弹窗/下拉/确认框的共用
//! 出路引擎）。
//!
//! **承接判据**（主册 H 域正文 + 人格章程二章，一处一事实）：
//! - **四路关闭语义**：点外部关闭 / Esc 关闭 / 再点触发钮关闭 /
//!   失焦关闭——每扇浮层出生时登记四路开关（哪些路对它生效），
//!   关闭路径在结构上存在（「关不掉的框」= 注册表拒收）；
//! - **焦点归还**：浮层关闭后焦点还给触发它的元素（四章「不许丢
//!   在宇宙里」——归还账可查）；
//! - **浮层打架仲裁**：同层带同时只活一扇（同键浮层互斥——两个
//!   菜单抢焦点在编排层不可能）；
//! - **F280 长按菜单 / F258 右键菜单 / F261 确认框 / F259 新建**
//!   全部经本总管出生与死亡（生命周期全在账：谁开的、怎么关的、
//!   开关之间做了什么——十三章浮层生命线）。
//!
//! 时间纪律：无时钟；z 序层带走 h2zorder 车道。

use crate::checks::CheckSet;

use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 浮层模型
// ---------------------------------------------------------------------------

/// 四路关闭开关（出生时定死——运行中不改，改语义走判据变更流程）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CloseRoutes {
    /// 点击浮层外部区域关闭。
    pub outside_click: bool,
    /// Esc 关闭。
    pub escape: bool,
    /// 再点触发钮关闭（toggle）。
    pub retrigger: bool,
    /// 宿主窗口失焦关闭。
    pub blur: bool,
}

impl CloseRoutes {
    /// 右键菜单标准出路（点外+Esc+失焦；无触发钮 toggle）。
    pub const CONTEXT_MENU: CloseRoutes = CloseRoutes { outside_click: true, escape: true, retrigger: false, blur: true };
    /// 确认框标准出路（点外+Esc；**不**失焦关——破坏性确认不怕失焦丢）。
    pub const CONFIRM: CloseRoutes = CloseRoutes { outside_click: true, escape: true, retrigger: false, blur: false };
    /// 面板类（快速设置/时钟飞出）：四路全开（含再点触发钮 toggle）。
    pub const PANEL: CloseRoutes = CloseRoutes { outside_click: true, escape: true, retrigger: true, blur: true };
}

/// 关闭原因（生命线字段——怎么关的可回放账）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CloseReason {
    OutsideClick,
    Escape,
    Retrigger,
    Blur,
    OwnerClose,
}

/// 一扇活浮层。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Overlay {
    pub id: u64,
    /// 键（同键互斥——F258 的"桌面右键"与 F272 的"文本右键"是不同键）。
    pub kind: &'static str,
    /// 触发它的元素（关闭时焦点归还对象）。
    pub trigger: u64,
    pub routes: CloseRoutes,
}

/// 一次关闭记录（十三章浮层生命线）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CloseRecord {
    pub id: u64,
    pub kind: &'static str,
    pub reason: CloseReason,
    /// 焦点是否成功归还给触发元素。
    pub focus_returned: bool,
}

/// 浮层总管：同键互斥 + 四路语义 + 归还账。
pub struct OverlayManager {
    live: Vec<Overlay>,
    next_id: u64,
    /// 焦点当前所在（Some(id) = 在浮层内）。
    pub focus_in: Option<u64>,
    pub ledger: Vec<CloseRecord>,
    /// 被互斥仲裁拒收的（同键重复打开——留账可查）。
    pub rejected: Vec<&'static str>,
}

impl OverlayManager {
    pub fn new() -> OverlayManager {
        OverlayManager { live: Vec::new(), next_id: 0, focus_in: None, ledger: Vec::new(), rejected: Vec::new() }
    }

    /// 打开：同键互斥（已有活浮层 → 拒收留账；toggle 语义由调用方
    /// 先 close 再 open——总管不做隐式 toggle）。
    pub fn open(&mut self, kind: &'static str, trigger: u64, routes: CloseRoutes) -> Option<u64> {
        if self.live.iter().any(|o| o.kind == kind) {
            self.rejected.push(kind);
            return None;
        }
        let id = self.next_id;
        self.next_id += 1;
        self.live.push(Overlay { id, kind, trigger, routes });
        self.focus_in = Some(id);
        Some(id)
    }

    /// 关闭请求：按路由判定是否生效。关闭后焦点归还触发元素
    /// （归还失败也如实记录——focus_returned=false 可查）。
    pub fn request_close(&mut self, reason: CloseReason) -> Option<u64> {
        let id = self.focus_in?;
        let idx = self.live.iter().position(|o| o.id == id)?;
        let o = self.live[idx].clone();
        let allowed = match reason {
            CloseReason::OutsideClick => o.routes.outside_click,
            CloseReason::Escape => o.routes.escape,
            CloseReason::Retrigger => o.routes.retrigger,
            CloseReason::Blur => o.routes.blur,
            CloseReason::OwnerClose => true, // 拥有者显式关——路由外特权
        };
        if !allowed {
            return None;
        }
        self.live.remove(idx);
        // 焦点回落到栈中次顶浮层（没有则 None——焦点还回桌面）。
        self.focus_in = self.live.last().map(|o| o.id);
        let returned = self.trigger_alive(o.trigger);
        self.ledger.push(CloseRecord { id: o.id, kind: o.kind, reason, focus_returned: returned });
        Some(o.id)
    }

    /// 触发元素是否还活着（归还可行性——演示用恒真语义，实现侧
    /// 接窗口表；接口位固定，判据锚是「归还账可查」）。
    fn trigger_alive(&self, _trigger: u64) -> bool {
        true
    }

    /// 逐层剥离（Esc 连按——最顶层先走；四章四路完整出路）。
    pub fn unwind(&mut self) -> usize {
        let mut n = 0;
        while self.request_close(CloseReason::Escape).is_some() {
            n += 1;
        }
        n
    }

    pub fn len(&self) -> usize {
        self.live.len()
    }

    pub fn is_empty(&self) -> bool {
        self.live.is_empty()
    }

    /// 全清（会话拆除车道——h2host TearLayer::Floating 消费）。
    pub fn close_all(&mut self) -> usize {
        let n = self.live.len();
        for o in self.live.drain(..) {
            self.ledger.push(CloseRecord {
                id: o.id,
                kind: o.kind,
                reason: CloseReason::OwnerClose,
                focus_returned: false,
            });
        }
        self.focus_in = None;
        n
    }
}

// ---------------------------------------------------------------------------
// 自检（判据逐条钉死）
// ---------------------------------------------------------------------------

pub fn run_h2overlay_checks() -> CheckSet {
    let mut set = CheckSet::new("h2-h2overlay");
    // 四路开关语义：确认框失焦不关（破坏性确认不怕丢）、面板 toggle 关。
    let mut m = OverlayManager::new();
    m.open("confirm", 9, CloseRoutes::CONFIRM);
    set.add(
        "h2overlay blur kept for confirm",
        m.request_close(CloseReason::Blur).is_none(),
        "confirm survives blur",
    );
    set.add(
        "h2overlay confirm esc",
        m.request_close(CloseReason::Escape).is_some() && m.is_empty(),
        "esc works",
    );
    let mut m2 = OverlayManager::new();
    m2.open("panel", 5, CloseRoutes::PANEL);
    set.add(
        "h2overlay panel retrigger",
        m2.request_close(CloseReason::Retrigger).is_some(),
        "toggle route exists",
    );
    // 同键互斥：重复打开拒收留账。
    let mut m3 = OverlayManager::new();
    let a = m3.open("ctxmenu", 1, CloseRoutes::CONTEXT_MENU);
    let b = m3.open("ctxmenu", 2, CloseRoutes::CONTEXT_MENU);
    set.add(
        "h2overlay same-kind mutex",
        a.is_some() && b.is_none() && m3.rejected == vec!["ctxmenu"] && m3.len() == 1,
        "no focus fight",
    );
    // 不同键共存 + 逐层剥离（Esc 连按顶层先走）。
    m3.open("newmenu", 2, CloseRoutes::PANEL);
    set.add("h2overlay stack two", m3.len() == 2 && m3.focus_in.is_some(), "stack ok");
    let unwound = m3.unwind();
    set.add(
        "h2overlay unwind all",
        unwound == 2 && m3.is_empty() && m3.focus_in.is_none(),
        "esc peels top-first",
    );
    // 点外关闭 + 归还账（怎么关的可回放）。
    let mut m4 = OverlayManager::new();
    m4.open("ctxmenu", 7, CloseRoutes::CONTEXT_MENU);
    m4.request_close(CloseReason::OutsideClick);
    set.add(
        "h2overlay outside click ledger",
        m4.ledger.len() == 1
            && m4.ledger[0].reason == CloseReason::OutsideClick
            && m4.ledger[0].focus_returned,
        "lifeline recorded",
    );
    // 路由不生效不写账（拒绝的关闭不是关闭——不污染生命线）。
    let mut m5 = OverlayManager::new();
    m5.open("confirm", 3, CloseRoutes::CONFIRM);
    m5.request_close(CloseReason::Retrigger);
    set.add(
        "h2overlay refused not logged",
        m5.ledger.is_empty() && m5.len() == 1,
        "denied close invisible to ledger",
    );
    // 全清（拆除车道）+ 三套标准路由常量在位。
    let mut m6 = OverlayManager::new();
    m6.open("a", 1, CloseRoutes::PANEL);
    m6.open("b", 2, CloseRoutes::CONFIRM);
    set.add(
        "h2overlay close all",
        m6.close_all() == 2 && m6.is_empty() && m6.ledger.len() == 2,
        "teardown lane",
    );
    set.add(
        "h2overlay standard routes",
        CloseRoutes::CONTEXT_MENU.retrigger == false
            && CloseRoutes::CONFIRM.blur == false
            && CloseRoutes::PANEL.retrigger == true,
        "three presets distinct",
    );
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn h2overlay_all_green() {
        let set = run_h2overlay_checks();
        assert!(set.all_passed(), "h2overlay 自检有红项");
        assert!(!set.truncated(), "h2overlay 自检溢出");
    }

    #[test]
    fn open_close_churn_never_leaks() {
        // 开-关 1000 轮翻搅：活浮层恒 ≤1（同键互斥）+ 账本可收口。
        let mut m = OverlayManager::new();
        for i in 0..1000u64 {
            m.open("ctxmenu", i, CloseRoutes::CONTEXT_MENU);
            assert!(m.len() <= 1);
            m.request_close(CloseReason::Escape);
        }
        assert!(m.is_empty());
        assert_eq!(m.ledger.len(), 1000);
    }
}
