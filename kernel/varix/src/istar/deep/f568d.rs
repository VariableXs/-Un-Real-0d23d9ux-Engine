//! 深化层 · F568 任务栏中键关闭（回炉补深主册【设计要点】未实装机制）。
//!
//! 补深四条（判据唯一源：主册 F568 节）：
//! ①**两层命中判定器**——中键事件的路由几何：缩略图浮层带（y < 浮层
//!   高）→ 单窗层；图标区按槽宽取槽（槽号 < 应用数才有效）；其余落空
//!   （落空不误路由——误触第一道闸）；
//! ②**未保存拦截状态机**——逐窗三问队列：窗 A 问询中窗 B 挂起、
//!   逐窗裁决落关闭账、点「取消」全链终止（问询中 + 挂起全释放，
//!   无一关闭）；
//! ③**误触防护评估器**——关闭路径裁决唯一源：零未保存窗 → 直接关；
//!   任一未保存窗 → 强制三问（风险账一分不让）；
//! ④**开关关闭时的静默路径**——事件被静默吞没（计吞没账、不动关闭
//!   账、不惊动任何窗——非中键用户零感知）。

use crate::checks::CheckSet;
use crate::istar::ibase::ISTAR_DOMAIN;
use crate::istar::midclose::{DirtyState, MidClose};

use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// ① 两层命中判定器
// ---------------------------------------------------------------------------

/// 缩略图浮层带高度（px——浮层压在图标区上方）。
pub const THUMB_STRIP_H: u32 = 160;
/// 图标槽宽（px——每应用一槽）。
pub const SLOT_W: u32 = 48;

/// 中键命中层（槽号即应用序）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MidLayer {
    /// 缩略图浮层带：命中第 n 槽（具体窗 id 由缩略图列定）。
    ThumbStrip(usize),
    /// 图标区：命中第 n 槽 = 关该应用全部窗口。
    IconSlot(usize),
}

/// 命中路由：y 在浮层带 → 单窗层；否则按槽号落图标区；越槽落空 None。
pub fn route(x: u32, y: u32, app_count: usize) -> Option<MidLayer> {
    if y < THUMB_STRIP_H {
        return Some(MidLayer::ThumbStrip((x / SLOT_W) as usize));
    }
    let slot = (x / SLOT_W) as usize;
    if slot < app_count {
        Some(MidLayer::IconSlot(slot))
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// ② 未保存拦截状态机（逐窗三问队列）
// ---------------------------------------------------------------------------

/// 三问队列：首窗出队问询，余窗挂起；逐窗裁决或全链取消。
pub struct PromptChain {
    current: Option<u64>,
    pending: Vec<u64>,
    resolved_close: Vec<u64>,
}

impl PromptChain {
    /// 开始链：首窗上问，余窗挂起（空窗链 = 立即完结）。
    pub fn begin(ids: &[u64]) -> PromptChain {
        let mut it = ids.iter().copied();
        let first = it.next();
        PromptChain { current: first, pending: it.collect(), resolved_close: Vec::new() }
    }

    /// 当前问询中的窗。
    pub fn current_id(&self) -> Option<u64> {
        self.current
    }

    /// 挂起窗数（问询期间不弹、不关、不催）。
    pub fn suspended_count(&self) -> usize {
        self.pending.len()
    }

    /// 当前窗三问裁决：close = true 落关闭账；下一窗上问。返回链是否仍在途。
    pub fn resolve(&mut self, close: bool) -> bool {
        let cur = match self.current {
            Some(c) => c,
            None => return false,
        };
        if close {
            self.resolved_close.push(cur);
        }
        self.current = if self.pending.is_empty() { None } else { Some(self.pending.remove(0)) };
        self.current.is_some()
    }

    /// 取消：全链终止——问询中 + 挂起全释放，无一关闭。返回释放窗数。
    pub fn cancel(&mut self) -> usize {
        let mut freed = 0usize;
        if self.current.is_some() {
            freed += 1;
        }
        freed += self.pending.len();
        self.current = None;
        self.pending.clear();
        freed
    }

    pub fn finished(&self) -> bool {
        self.current.is_none()
    }

    pub fn closed_ids(&self) -> &[u64] {
        &self.resolved_close
    }
}

// ---------------------------------------------------------------------------
// ③ 误触防护评估器
// ---------------------------------------------------------------------------

/// 关闭路径裁决。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClosePath {
    /// 零未保存窗——直接关（风险账为零）。
    Direct,
    /// 任一未保存窗——强制三问（风险账一分不让）。
    ThreeQuestions,
}

/// 防护裁决唯一源：未保存窗数 → 关闭路径。
pub fn guard_path(unsaved_windows: usize) -> ClosePath {
    if unsaved_windows == 0 {
        ClosePath::Direct
    } else {
        ClosePath::ThreeQuestions
    }
}

// ---------------------------------------------------------------------------
// ④ 开关关闭时的静默路径
// ---------------------------------------------------------------------------

/// 静默路径账：开关关 → 中键事件吞没计数（不动任何别的账）。
pub struct SilentPath {
    swallowed: u32,
}

impl SilentPath {
    pub fn new() -> SilentPath {
        SilentPath { swallowed: 0 }
    }

    /// 中键事件进入：开关关 → 吞没（true）；开关开 → 交正常路径（false）。
    pub fn feed(&mut self, m: &MidClose) -> bool {
        if !m.enabled() {
            self.swallowed += 1;
            true
        } else {
            false
        }
    }

    pub fn swallowed(&self) -> u32 {
        self.swallowed
    }
}

impl Default for SilentPath {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 深化自检
// ---------------------------------------------------------------------------

pub fn run_f568_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new(ISTAR_DOMAIN);

    // 1) 浮层带命中：y < 160 → 单窗层，槽号按 48px 取。
    cs.add("thumb strip routes thumb layer", route(100, 80, 3) == Some(MidLayer::ThumbStrip(2)), "");

    // 2) 图标区命中：y ≥ 160 且槽号 < 应用数 → 整组层。
    cs.add("icon zone routes icon layer", route(100, 300, 3) == Some(MidLayer::IconSlot(2)), "");

    // 3) 落空不误路由：越槽 / 空应用区一律 None（误触第一道闸）。
    cs.add(
        "miss beyond slots routes none",
        route(200, 300, 3).is_none() && route(100, 300, 0).is_none(),
        "",
    );

    // 4) 防护裁决边界：0 未保存 → 直关；1 未保存 → 三问。
    cs.add(
        "guard path boundary",
        guard_path(0) == ClosePath::Direct && guard_path(1) == ClosePath::ThreeQuestions,
        "",
    );

    // 5) 直关路径实证：全净应用图标中键 → 一杆关全部（基础层语义不破）。
    let mut m = MidClose::new();
    m.add_win("终端", 1, DirtyState::Clean);
    m.add_win("终端", 2, DirtyState::Clean);
    let direct = matches!(m.middle_click_icon("终端"), Ok(ref ids) if ids.len() == 2);
    cs.add("clean app closes all directly", direct, "");

    // 6) 三问队列：三窗入链 → 首窗上问、两窗挂起。
    let mut ch = PromptChain::begin(&[11, 12, 13]);
    let queued = ch.current_id() == Some(11) && ch.suspended_count() == 2;

    // 7) 逐窗裁决：三窗全裁「关」→ 链完结、关闭账 11/12/13。
    let mut guard = 0;
    while ch.current_id().is_some() && guard < 8 {
        ch.resolve(true);
        guard += 1;
    }
    cs.add(
        "chain resolves through all",
        queued && ch.finished() && ch.closed_ids() == &[11u64, 12, 13][..],
        "",
    );

    // 8) 取消全链终止：裁掉一窗（留窗）后取消 → 释放问询中 + 挂起共 2，
    //    关闭账空（无一关闭）。
    let mut ch2 = PromptChain::begin(&[4, 5, 6]);
    ch2.resolve(false);
    let freed = ch2.cancel();
    cs.add(
        "cancel terminates whole chain",
        freed == 2 && ch2.finished() && ch2.closed_ids().is_empty(),
        "",
    );

    // 9) 链路贯通基础层：未保存应用拒直关 → 三问逐窗裁「不保存关」→
    //    force_close 落基础层关闭账（after-three-questions 来源全对齐）。
    let mut m2 = MidClose::new();
    m2.add_win("报告", 11, DirtyState::Unsaved);
    m2.add_win("报告", 12, DirtyState::Unsaved);
    m2.add_win("报告", 13, DirtyState::Unsaved);
    let refused = m2.middle_click_icon("报告").is_err();
    let mut ch3 = PromptChain::begin(&[11, 12, 13]);
    let mut landed = 0;
    while ch3.current_id().is_some() {
        let id = match ch3.current_id() {
            Some(i) => i,
            None => break,
        };
        if m2.force_close_after_prompt("报告", id) {
            landed += 1;
        }
        ch3.resolve(true);
    }
    cs.add(
        "chain lands force close ledger",
        refused && landed == 3 && m2.closed_log().len() == 3
            && m2.closed_log().iter().all(|(_, s)| *s == "after-three-questions"),
        "",
    );

    // 10) 静默路径：开关关 → 事件吞没记账、关闭账不动、直关照拒。
    let mut m3 = MidClose::new();
    m3.add_win("画图", 5, DirtyState::Clean);
    m3.set_enabled(false);
    let mut sp = SilentPath::new();
    let eaten = sp.feed(&m3);
    cs.add(
        "off switch silent path",
        eaten && sp.swallowed() == 1 && m3.closed_log().is_empty()
            && m3.middle_click_icon("画图").is_err(),
        "",
    );

    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_chain_finishes_immediately() {
        let mut ch = PromptChain::begin(&[]);
        assert!(ch.finished());
        assert_eq!(ch.current_id(), None);
        assert_eq!(ch.cancel(), 0);
    }

    #[test]
    fn route_boundary_at_strip_edge() {
        // y = 160 恰在浮层带外（含头不含尾）→ 图标区。
        assert_eq!(route(48, THUMB_STRIP_H, 2), Some(MidLayer::IconSlot(1)));
        assert_eq!(route(48, THUMB_STRIP_H - 1, 2), Some(MidLayer::ThumbStrip(1)));
    }

    #[test]
    fn resolve_false_keeps_window_open() {
        let mut ch = PromptChain::begin(&[7, 8]);
        ch.resolve(false); // 窗 7 裁「留」
        assert_eq!(ch.closed_ids().len(), 0);
        assert_eq!(ch.current_id(), Some(8));
        ch.resolve(true);
        assert_eq!(ch.closed_ids(), &[8u64][..]);
    }
}
