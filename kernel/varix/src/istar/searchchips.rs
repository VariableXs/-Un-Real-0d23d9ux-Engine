//! F577 搜索结果过滤片 · 完整设计（STAR I 主册 I 域批次八）。
//!
//! **判据（主册）**：三片过滤正确性；计数准确；即时性（<50ms 本地筛）；
//! 键盘可达；会话内记忆。
//!
//! **设计要点（主册）**：
//! - 搜索结果页顶部过滤片三枚（应用/文件/设置 chips）：点击只看该类
//!   （片高亮态）；计数徽标（各类结果数即时可见）；键盘可达（Tab 到片行、
//!   方向键切换）；过滤态记忆（本次搜索会话内保持）；清过滤一键回全量。

use crate::checks::CheckSet;
use crate::istar::ibase::ISTAR_DOMAIN;

// ---------------------------------------------------------------------------
// 规格常量
// ---------------------------------------------------------------------------

/// 本地筛即时性红线（ms）。
pub const FILTER_BUDGET_MS: u64 = 50;

/// 结果三类（枚举即三片）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResKind {
    App,
    File,
    Setting,
}

/// 三片全量枚举（键盘左右循环的序——片行顺序唯一源）。
pub const CHIP_ORDER: [ResKind; 3] = [ResKind::App, ResKind::File, ResKind::Setting];

// ---------------------------------------------------------------------------
// 模型
// ---------------------------------------------------------------------------

/// 一条搜索结果。
#[derive(Clone, Copy, Debug)]
pub struct ResItem {
    pub id: u64,
    pub kind: ResKind,
}

/// 过滤片引擎（会话内账——清会话即复位）。
pub struct SearchChips {
    items: [Option<ResItem>; 256],
    len: usize,
    /// 当前选中的片（None = 全量）。
    active: Option<ResKind>,
    /// 最近一次过滤耗时（红线对账）。
    last_ms: u64,
    /// 键盘焦点片位（Tab 到片行后的方向键游标）。
    cursor: usize,
}

impl SearchChips {
    pub fn new() -> SearchChips {
        SearchChips {
            items: [(); 256].map(|_| None),
            len: 0,
            active: None,
            last_ms: 0,
            cursor: 0,
        }
    }

    /// 结果集注入（新搜索 = 会话重置：片选与计数全清——「过滤态记忆」
    /// 是同一搜索会话内保持，跨搜索不串）。
    pub fn new_search(&mut self, items: &[ResItem]) {
        self.len = 0;
        for it in items {
            if self.len < 256 {
                self.items[self.len] = Some(*it);
                self.len += 1;
            }
        }
        self.active = None;
    }

    /// 计数徽标（某类结果数——即时可见的账）。
    pub fn count_of(&self, kind: ResKind) -> usize {
        self.items[..self.len]
            .iter()
            .flatten()
            .filter(|i| i.kind == kind)
            .count()
    }

    /// 点片：只看该类（再点同一片 = 清过滤回全量）。
    pub fn pick(&mut self, kind: ResKind) {
        self.active = if self.active == Some(kind) { None } else { Some(kind) };
    }

    /// 清过滤一键回全量。
    pub fn clear_filter(&mut self) {
        self.active = None;
    }

    /// 过滤视图（本地筛——不重搜，内存内筛）。
    pub fn view(&mut self, elapsed_ms: u64) -> alloc::vec::Vec<u64> {
        self.last_ms = elapsed_ms;
        self.items[..self.len]
            .iter()
            .flatten()
            .filter(|i| self.active.map(|k| i.kind == k).unwrap_or(true))
            .map(|i| i.id)
            .collect()
    }

    /// 即时性红线（<50ms 本地筛）。
    pub fn within_budget(&self) -> bool {
        self.last_ms < FILTER_BUDGET_MS
    }

    /// 键盘可达：方向键在片行循环移动游标。
    pub fn chip_next(&mut self) {
        self.cursor = (self.cursor + 1) % CHIP_ORDER.len();
    }

    pub fn chip_prev(&mut self) {
        self.cursor = (self.cursor + CHIP_ORDER.len() - 1) % CHIP_ORDER.len();
    }

    /// Enter 激活游标所在片。
    pub fn chip_activate(&mut self) -> ResKind {
        let k = CHIP_ORDER[self.cursor];
        self.pick(k);
        k
    }

    pub fn active(&self) -> Option<ResKind> {
        self.active
    }

    pub fn cursor_kind(&self) -> ResKind {
        CHIP_ORDER[self.cursor]
    }
}

impl Default for SearchChips {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub fn run_searchchips_checks() -> CheckSet {
    let mut set = CheckSet::new(ISTAR_DOMAIN);

    let mk = |id: u64, k: ResKind| ResItem { id, kind: k };

    // 1. 三片过滤正确性：点应用只看应用；再点回全量。
    let mut c = SearchChips::new();
    c.new_search(&[mk(1, ResKind::App), mk(2, ResKind::File), mk(3, ResKind::App), mk(4, ResKind::Setting)]);
    c.pick(ResKind::App);
    let apps = c.view(10);
    c.pick(ResKind::App); // 再点同片清过滤
    let all = c.view(10);
    set.add(
        "chip filter and toggle off",
        apps == alloc::vec![1u64, 3] && all.len() == 4 && c.active().is_none(),
        "",
    );

    // 2. 计数准确：应用 2 / 文件 1 / 设置 1。
    set.add(
        "counts accurate",
        c.count_of(ResKind::App) == 2
            && c.count_of(ResKind::File) == 1
            && c.count_of(ResKind::Setting) == 1,
        "",
    );

    // 3. 即时性 <50ms：49ms 过、50ms 拒（判据是小于）。
    c.view(49);
    let under = c.within_budget();
    c.view(50);
    set.add("budget strictly under 50ms", under && !c.within_budget(), "");

    // 4. 键盘可达：方向键循环 + Enter 激活。
    let mut c2 = SearchChips::new();
    c2.new_search(&[mk(1, ResKind::File), mk(2, ResKind::Setting)]);
    c2.chip_next();
    c2.chip_next();
    let activated = c2.chip_activate();
    set.add(
        "keyboard chips",
        c2.cursor_kind() == ResKind::Setting && activated == ResKind::Setting && c2.active() == Some(ResKind::Setting),
        "",
    );

    // 5. 方向键循环回卷：设置片右移回应用片。
    c2.chip_next();
    set.add("chip order wraps", c2.cursor_kind() == ResKind::App, "");
    c2.chip_prev();
    set.add("prev goes back", c2.cursor_kind() == ResKind::Setting, "");

    // 6. 会话内记忆：同一次搜索里片选保持；新搜索重置。
    c2.clear_filter();
    c2.pick(ResKind::File);
    let kept = c2.active() == Some(ResKind::File);
    c2.new_search(&[mk(9, ResKind::App)]);
    set.add(
        "session memory and reset",
        kept && c2.active().is_none() && c2.view(1) == alloc::vec![9u64],
        "",
    );

    // 7. 清过滤一键回全量。
    c2.pick(ResKind::App);
    c2.clear_filter();
    set.add("clear filter one key", c2.active().is_none(), "");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_result_set_views_empty() {
        let mut c = SearchChips::new();
        c.new_search(&[]);
        assert!(c.view(1).is_empty());
        assert_eq!(c.count_of(ResKind::App), 0);
    }

    #[test]
    fn overflow_results_truncated_honestly() {
        let mut c = SearchChips::new();
        let items: alloc::vec::Vec<ResItem> = (0..300)
            .map(|i| ResItem { id: i as u64, kind: ResKind::File })
            .collect();
        c.new_search(&items);
        assert_eq!(c.view(1).len(), 256); // 容量截断如实呈现
    }

    #[test]
    fn chip_activate_toggles_same_kind() {
        let mut c = SearchChips::new();
        c.new_search(&[]);
        c.chip_activate();
        assert!(c.active().is_some());
        c.chip_activate();
        assert!(c.active().is_none());
    }
}
