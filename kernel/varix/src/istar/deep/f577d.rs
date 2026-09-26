//! 深化层 · F577 搜索结果过滤片（回炉补深主册【设计要点】未实装机制）。
//!
//! 补深四条（判据唯一源：主册 F577 节）：
//! ①「过滤不重搜（本地筛 <50ms）」的 **本地过滤器引擎**——结果集一次
//!   扫描分三桶（计数 + 分桶视图）：过滤视图从桶直取（只重排不重扫），
//!   派生纯度对账（每个 id 三桶中恰出现一次）；
//! ②「键盘可达（Tab 到片行、方向键切换）」的 **键盘导航状态机**——
//!   Tab 进出片行、方向键循环游标、Enter 激活游标片、Esc 清过滤
//!   （诚实回执：真清了才报真）；
//! ③「计数徽标（各类结果数即时可见）」的 **计数守恒对账**——三桶计数
//!   之和 == 结果总数，逐类与基础件 count_of 一致（徽标账不丢项）；
//! ④「过滤态记忆（本次搜索会话内保持）」的 **会话记忆语义规则**——
//!   精化搜索（新词以旧词为前缀）保留过滤态（新结果注入后按原片重
//!   选）；全新搜索重置（与基础件 new_search 的会话重置对齐）。

use crate::checks::CheckSet;
use crate::istar::ibase::ISTAR_DOMAIN;
use crate::istar::searchchips::{CHIP_ORDER, FILTER_BUDGET_MS, ResItem, ResKind, SearchChips};

use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 本地过滤器引擎（分桶派生）
// ---------------------------------------------------------------------------

/// 类 → 桶位（桶序与 CHIP_ORDER 一致）。
fn kind_slot(k: ResKind) -> usize {
    match k {
        ResKind::App => 0,
        ResKind::File => 1,
        ResKind::Setting => 2,
    }
}

/// 分桶账：一次扫全量产出三桶（计数 + 保序 id 列表）。过滤视图从桶
/// 直取——过滤不重搜（只重排不重扫）的机制面。
pub struct Buckets {
    counts: [usize; 3],
    ids: [Vec<u64>; 3],
}

impl Buckets {
    /// 一次扫描派生全部分桶。
    pub fn derive(items: &[ResItem]) -> Buckets {
        let mut b = Buckets {
            counts: [0; 3],
            ids: [Vec::new(), Vec::new(), Vec::new()],
        };
        for it in items {
            let k = kind_slot(it.kind);
            b.counts[k] += 1;
            b.ids[k].push(it.id);
        }
        b
    }

    /// 某类计数（徽标取数口）。
    pub fn count(&self, kind: ResKind) -> usize {
        self.counts[kind_slot(kind)]
    }

    /// 某类过滤视图（保序——与全量扫描序一致）。
    pub fn view(&self, kind: ResKind) -> &[u64] {
        &self.ids[kind_slot(kind)]
    }

    /// 三桶之和（计数守恒对账面）。
    pub fn total(&self) -> usize {
        self.counts[0] + self.counts[1] + self.counts[2]
    }
}

// ---------------------------------------------------------------------------
// 键盘导航状态机
// ---------------------------------------------------------------------------

/// 焦点两态：结果列表 ↔ 片行（Tab 切换）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NavFocus {
    Results,
    Chips,
}

/// 键盘导航状态机：Tab 进出片行；方向键在片行内循环（游标不越界）；
/// Enter 激活游标片（拨动基础件过滤）；Esc 清过滤。
pub struct KeyNav {
    focus: NavFocus,
    cursor: usize,
}

impl KeyNav {
    pub fn new() -> KeyNav {
        KeyNav { focus: NavFocus::Results, cursor: 0 }
    }

    pub fn focus(&self) -> NavFocus {
        self.focus
    }

    pub fn cursor_kind(&self) -> ResKind {
        CHIP_ORDER[self.cursor]
    }

    /// Tab：结果行 ↔ 片行互切（游标位置保留——回来还在原片）。
    pub fn tab(&mut self) {
        self.focus = match self.focus {
            NavFocus::Results => NavFocus::Chips,
            NavFocus::Chips => NavFocus::Results,
        };
    }

    /// 右方向键：片行内右移循环；结果行内无操作。
    pub fn right(&mut self) {
        if self.focus == NavFocus::Chips {
            self.cursor = (self.cursor + 1) % CHIP_ORDER.len();
        }
    }

    /// 左方向键：片行内左移循环；结果行内无操作。
    pub fn left(&mut self) {
        if self.focus == NavFocus::Chips {
            self.cursor = (self.cursor + CHIP_ORDER.len() - 1) % CHIP_ORDER.len();
        }
    }

    /// Enter：片行内激活游标片（拨基础件过滤）；结果行内无操作（None）。
    pub fn enter(&mut self, chips: &mut SearchChips) -> Option<ResKind> {
        match self.focus {
            NavFocus::Chips => {
                let k = CHIP_ORDER[self.cursor];
                chips.pick(k);
                Some(k)
            }
            NavFocus::Results => None,
        }
    }

    /// Esc：清过滤（回执诚实——真有过滤被清才报真）。
    pub fn esc(&mut self, chips: &mut SearchChips) -> bool {
        let had = chips.active().is_some();
        chips.clear_filter();
        had
    }
}

// ---------------------------------------------------------------------------
// 会话记忆语义规则
// ---------------------------------------------------------------------------

/// 会话动作：精化搜索保留过滤态；全新搜索重置。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionAction {
    Preserve,
    Reset,
}

/// 会话记忆语义：新词以旧词为前缀（同一会话内继续打字精化）→ 保留
/// 过滤态；全新词或旧词为空（首次搜索）→ 重置。
pub fn session_rule(old_query: &str, new_query: &str) -> SessionAction {
    if !old_query.is_empty() && new_query.len() >= old_query.len() && new_query.starts_with(old_query)
    {
        SessionAction::Preserve
    } else {
        SessionAction::Reset
    }
}

// ---------------------------------------------------------------------------
// 深化自检
// ---------------------------------------------------------------------------

pub fn run_f577_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new(ISTAR_DOMAIN);

    let mk = |id: u64, k: ResKind| ResItem { id, kind: k };
    let items = [
        mk(1, ResKind::App),
        mk(2, ResKind::File),
        mk(3, ResKind::App),
        mk(4, ResKind::Setting),
        mk(5, ResKind::File),
    ];

    // 1) 分桶引擎：一次扫全量分三桶，桶计数与基础件 count_of 逐类一致。
    let buckets = Buckets::derive(&items);
    let mut chips = SearchChips::new();
    chips.new_search(&items);
    let counts_match =
        (0..3).all(|k| buckets.count(CHIP_ORDER[k]) == chips.count_of(CHIP_ORDER[k]));
    cs.add("bucket counts match base", counts_match, "");

    // 2) 计数守恒：三桶之和 == 结果总数（徽标账不丢项）。
    cs.add("bucket counts conserved", buckets.total() == items.len(), "");

    // 3) 过滤视图派生 == 基础件过滤视图（本地筛同结论——只重排不重扫）。
    chips.pick(ResKind::App);
    let base_view = chips.view(1);
    cs.add(
        "derived view equals base view",
        buckets.view(ResKind::App) == base_view.as_slice(),
        "",
    );

    // 4) 派生纯度：每个结果 id 在三桶中恰出现一次（多漏即账坏）。
    let mut pure = true;
    for it in items.iter() {
        let seen = buckets
            .view(ResKind::App)
            .iter()
            .chain(buckets.view(ResKind::File).iter())
            .chain(buckets.view(ResKind::Setting).iter())
            .filter(|id| **id == it.id)
            .count();
        if seen != 1 {
            pure = false;
        }
    }
    cs.add("bucket purity one place each", pure, "");

    // 5) 即时性红线沿用基础常量（深化不放宽）：本地筛预算仍 <50ms。
    chips.view(FILTER_BUDGET_MS - 1);
    let under = chips.within_budget();
    chips.view(FILTER_BUDGET_MS);
    cs.add(
        "filter budget unchanged",
        FILTER_BUDGET_MS == 50 && under && !chips.within_budget(),
        "",
    );

    // 6) 键盘导航全路径：Tab 进片行 → 方向键到位 → Enter 激活对应片 →
    //    Tab 回结果行（Enter 无操作）。
    let mut chips2 = SearchChips::new();
    chips2.new_search(&items);
    let mut nav = KeyNav::new();
    nav.tab();
    let in_chips = nav.focus() == NavFocus::Chips;
    nav.right();
    nav.right();
    let at_setting = nav.cursor_kind() == ResKind::Setting;
    let got = nav.enter(&mut chips2);
    let filtered = chips2.active() == Some(ResKind::Setting) && chips2.view(1) == alloc::vec![4u64];
    nav.tab();
    let back_results = nav.focus() == NavFocus::Results && nav.enter(&mut chips2).is_none();
    cs.add("keyboard nav full path",
        in_chips && at_setting && got == Some(ResKind::Setting) && filtered && back_results, "");

    // 7) 方向键双向回卷：应用片三记右移回卷到应用片；左移逐片退回。
    let mut nav2 = KeyNav::new();
    nav2.tab();
    nav2.right();
    nav2.right();
    nav2.right();
    let wrapped = nav2.cursor_kind() == ResKind::App;
    nav2.left();
    let back_to_setting = nav2.cursor_kind() == ResKind::Setting;
    nav2.left();
    nav2.left();
    let to_app = nav2.cursor_kind() == ResKind::App;
    cs.add("arrow wrap both ways", wrapped && back_to_setting && to_app, "");

    // 8) Esc 清过滤诚实回执：有过滤清掉报真；无过滤再按报假。
    let mut chips3 = SearchChips::new();
    chips3.new_search(&items);
    chips3.pick(ResKind::File);
    let mut nav3 = KeyNav::new();
    let cleared = nav3.esc(&mut chips3);
    let noop = !nav3.esc(&mut chips3);
    cs.add(
        "esc clears with honest receipt",
        cleared && noop && chips3.active().is_none(),
        "",
    );

    // 9) 会话记忆语义：精化（前缀延续）保留；全新词重置；首次搜索重置。
    let refine = session_rule("应用", "应用商店") == SessionAction::Preserve;
    let fresh = session_rule("应用", "天气") == SessionAction::Reset;
    let blank = session_rule("", "天气") == SessionAction::Reset;
    cs.add("session memory semantics", refine && fresh && blank, "");

    // 10) 保留路径落位：精化后新结果注入（基础件会话重置）→ 深化账按
    //     原片重选，视图仍单类保序。
    let mut chips4 = SearchChips::new();
    chips4.new_search(&items);
    chips4.pick(ResKind::File);
    let new_items = [mk(9, ResKind::File), mk(10, ResKind::App), mk(11, ResKind::File)];
    chips4.new_search(&new_items); // 基础件会话重置
    if session_rule("应用", "应用商店") == SessionAction::Preserve {
        chips4.pick(ResKind::File); // 深化账：按原片重选
    }
    cs.add("preserve reapply filtered view",
        chips4.active() == Some(ResKind::File) && chips4.view(1) == alloc::vec![9u64, 11u64], "");

    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derive_empty_set() {
        let b = Buckets::derive(&[]);
        assert_eq!(b.total(), 0);
        assert!(b.view(ResKind::App).is_empty());
    }

    #[test]
    fn nav_enter_in_results_noop() {
        let mut nav = KeyNav::new();
        let mut chips = SearchChips::new();
        assert!(nav.enter(&mut chips).is_none());
        assert!(chips.active().is_none());
    }

    #[test]
    fn session_equal_query_preserves() {
        assert_eq!(session_rule("ab", "ab"), SessionAction::Preserve);
    }
}
