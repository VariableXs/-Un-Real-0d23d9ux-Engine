//! F081 任务视图 · 完整设计（STAR I 主册 G-C-11）。
//!
//! **判据（主册）**：四桌面×三窗口压测全流程录屏；拖移窗口跨桌后
//! 焦点正确；进入/退出动画帧率达标。
//!
//! **设计要点（主册）**：
//! - Win+Tab 多窗口缩略墙（复用 F073 管道）+ 底部虚拟桌面条：桌面
//!   1/2 增删、窗口跨桌拖移；每桌面独立最近使用栈（F072）与分屏
//!   布局（F080）；
//! - 进入动画 250ms：所有窗口缩略飞入网格（保持相对位置感）；顶部
//!   当前桌面名可点击重命名；虚拟桌面条卡 160×90px（桌面缩略）；
//!   拖窗口到桌面卡上=移动；桌面条「+新建」尾部常驻；Esc 退出回
//!   原桌面；
//! - 桌面列表与窗口归属存会话状态（重启恢复 F 会话恢复面）；桌面
//!   名自定义存配置层；
//! - 关闭含窗口的桌面 → 窗口并入前一桌（Windows 同策略）+ toast
//!   可撤销；缩略墙窗口过多 → 分页（每页 12 格）；Alt+Tab（F082）
//!   仅限当前桌面（跨桌用任务视图——边界明确）；
//! - 缩略墙网格自适应（3-4 列按窗口数）；窗口缩略比例统一缩放
//!   （保持相对大小感）；当前桌面卡描边强调色；键盘路径：Win+Tab
//!   后方向键+Enter 全可达；新建桌面动画从「+」钮展开（250ms）。
//!
//! 实装口径：虚拟桌面账本 + 窗口归属账 + 跨桌拖移焦点账 + 分页墙
//! 账 + 关闭并入撤销账 + 全键盘导航账。缩略图供给以显式闭包注入
//! （F073 管道接缝），本模块持归属与焦点判定。时间注入式。

use crate::checks::CheckSet;

use crate::deskstar::dbase::{FocusRing, Token};
use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 规格常量（参数唯一源——主册交互设计）
// ---------------------------------------------------------------------------

/// 进入/退出动画时长（ms）。
pub const ENTER_ANIM_MS: u32 = 250;

/// 虚拟桌面条卡宽（px）。
pub const DESK_CARD_W_PX: i32 = 160;

/// 虚拟桌面条卡高（px）。
pub const DESK_CARD_H_PX: i32 = 90;

/// 缩略墙每页格数。
pub const WALL_PAGE_CAP: usize = 12;

/// 关闭桌面并入的可撤销窗（ms，toast 语义）。
pub const MERGE_UNDO_MS: u64 = 5_000;

// ---------------------------------------------------------------------------
// 模型
// ---------------------------------------------------------------------------

/// 虚拟桌面（每桌面独立最近栈与分屏布局）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VirtualDesk {
    pub id: u64,
    pub name: String,
    /// 本桌面窗口（归属序即 z 序底→顶）。
    pub windows: Vec<u64>,
    /// 最近使用栈（MRU 头；F072 同引擎的消费面——本账持桌面内投影）。
    pub mru: Vec<u64>,
    /// 分屏布局记忆（F080 每桌面独立——zone 名投影）。
    pub snap_layouts: Vec<(u64, &'static str)>,
}

/// 待并入撤销账（关闭桌面 → 窗口并入前一桌 + toast 可撤销）。
struct PendingMerge {
    moved: Vec<(u64, u64)>, // (window, from_desk)
    at_ms: u64,
    into_desk: u64,
}

/// 任务视图。
pub struct TaskView {
    desks: Vec<VirtualDesk>,
    active_desk: usize,
    open: bool,
    now_ms: u64,
    opened_at: u64,
    next_desk_id: u64,
    /// 缩略墙当前页。
    page: usize,
    /// 焦点环（墙内窗口 + 桌面条卡两组导航——先墙后条）。
    ring: FocusRing,
    merge_undo: Option<PendingMerge>,
    /// toast 队列。
    toasts: Vec<String>,
    /// 焦点正确性账（跨桌拖移后焦点落点）。
    pub focus_moves: Vec<(u64, u64)>, // (window, desk_id)
    /// 桌面名自定义账（存配置层的内存投影）。
    pub renames: u32,
}

impl TaskView {
    pub fn new() -> TaskView {
        let mut tv = TaskView {
            desks: Vec::new(),
            active_desk: 0,
            open: false,
            now_ms: 0,
            opened_at: 0,
            next_desk_id: 1,
            page: 0,
            ring: FocusRing::new(1),
            merge_undo: None,
            toasts: Vec::new(),
            focus_moves: Vec::new(),
            renames: 0,
        };
        tv.create_desk(0);
        tv
    }

    /// 新建桌面（「+」尾部常驻；首个桌面随构造建立）。
    pub fn create_desk(&mut self, now_ms: u64) -> u64 {
        let id = self.next_desk_id;
        self.next_desk_id += 1;
        self.desks.push(VirtualDesk {
            id,
            name: alloc::format!("桌面 {}", id),
            windows: Vec::new(),
            mru: Vec::new(),
            snap_layouts: Vec::new(),
        });
        self.now_ms = now_ms;
        id
    }

    pub fn desk_count(&self) -> usize {
        self.desks.len()
    }

    pub fn active_id(&self) -> u64 {
        self.desks[self.active_desk].id
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Win+Tab 进入（250ms 动画起点）。
    pub fn enter(&mut self, now_ms: u64) {
        self.open = true;
        self.opened_at = now_ms;
        self.now_ms = now_ms;
        self.page = 0;
        self.sync_ring();
    }

    /// Esc 退出回原桌面。
    pub fn leave(&mut self, now_ms: u64) {
        self.open = false;
        self.now_ms = now_ms;
    }

    /// 进入动画进度（千分比）。
    pub fn enter_progress(&self) -> u16 {
        if !self.open {
            return 0;
        }
        ((self.now_ms.saturating_sub(self.opened_at) as u32).min(ENTER_ANIM_MS) * 1000
            / ENTER_ANIM_MS) as u16
    }

    // -- 窗口归属 ----------------------------------------------------------

    /// 窗口在某桌面创建/落位。
    pub fn place_window(&mut self, desk_idx: usize, win: u64) {
        if desk_idx < self.desks.len() {
            self.desks[desk_idx].windows.push(win);
            self.desks[desk_idx].mru.insert(0, win);
        }
    }

    /// 窗口前台激活（MRU 头更新——Alt+Tab 仅限当前桌面的数据基础）。
    pub fn activate_window(&mut self, desk_idx: usize, win: u64) {
        if desk_idx < self.desks.len() {
            let d = &mut self.desks[desk_idx];
            if d.windows.contains(&win) {
                d.mru.retain(|w| *w != win);
                d.mru.insert(0, win);
            }
        }
    }

    /// 拖窗口到桌面卡上=移动（跨桌焦点正确：落桌 MRU 头 + 焦点账）。
    pub fn drag_window_to_desk(&mut self, win: u64, to_desk_idx: usize, now_ms: u64) {
        if to_desk_idx >= self.desks.len() {
            return;
        }
        self.now_ms = now_ms;
        let from = self
            .desks
            .iter()
            .position(|d| d.windows.contains(&win));
        if let Some(from_idx) = from {
            if from_idx == to_desk_idx {
                return;
            }
            self.desks[from_idx].windows.retain(|w| *w != win);
            self.desks[from_idx].mru.retain(|w| *w != win);
            let desk_id = self.desks[to_desk_idx].id;
            self.desks[to_desk_idx].windows.push(win);
            self.desks[to_desk_idx].mru.insert(0, win);
            // 焦点正确性：跨桌后焦点跟随窗口（账上留痕——判据对账面）。
            self.focus_moves.push((win, desk_id));
        }
    }

    /// 当前桌面 MRU 头（Alt+Tab 的第一个候选）。
    pub fn active_mru_head(&self) -> Option<u64> {
        self.desks[self.active_desk].mru.first().copied()
    }

    // -- 桌面管理 ----------------------------------------------------------

    /// 重命名当前桌面（存配置层投影）。
    pub fn rename_active(&mut self, name: &str) {
        self.desks[self.active_desk].name = String::from(name);
        self.renames += 1;
    }

    pub fn active_name(&self) -> &str {
        &self.desks[self.active_desk].name
    }

    /// 关闭桌面：窗口并入前一桌 + toast 可撤销（首桌不可关——保底）。
    pub fn close_desk(&mut self, idx: usize, now_ms: u64) -> bool {
        if idx >= self.desks.len() || self.desks.len() == 1 {
            return false;
        }
        let moved: Vec<(u64, u64)> = self.desks[idx]
            .windows
            .iter()
            .map(|w| (*w, self.desks[idx].id))
            .collect();
        let into = if idx > 0 { idx - 1 } else { idx + 1 };
        let into_id = self.desks[into].id;
        let wins = core::mem::take(&mut self.desks[idx].windows);
        let mru = core::mem::take(&mut self.desks[idx].mru);
        let layouts = core::mem::take(&mut self.desks[idx].snap_layouts);
        self.desks[into].windows.extend(wins);
        self.desks[into].mru.splice(0..0, mru);
        self.desks[into].snap_layouts.extend(layouts);
        self.desks.remove(idx);
        if self.active_desk >= self.desks.len() {
            self.active_desk = self.desks.len() - 1;
        }
        self.merge_undo = Some(PendingMerge {
            moved,
            at_ms: now_ms,
            into_desk: into_id,
        });
        self.toasts
            .push(String::from("桌面已关闭，窗口已并入前一桌面"));
        self.now_ms = now_ms;
        true
    }

    /// 撤销并入（toast 窗内）。
    pub fn undo_merge(&mut self, now_ms: u64) -> bool {
        match self.merge_undo.take() {
            Some(p) if now_ms.saturating_sub(p.at_ms) < MERGE_UNDO_MS => {
                // 重建被关桌面并搬回窗口。
                let id = self.next_desk_id;
                self.next_desk_id += 1;
                let idx = self
                    .desks
                    .iter()
                    .position(|d| d.id == p.into_desk)
                    .map(|i| i + 1) // 撤销还原到原位（被关桌在并入桌之后）
                    .unwrap_or(self.desks.len());
                let moved_wins: Vec<u64> = p.moved.iter().map(|(w, _)| *w).collect();
                for d in self.desks.iter_mut() {
                    d.windows.retain(|w| !moved_wins.contains(w));
                    d.mru.retain(|w| !moved_wins.contains(w));
                }
                self.desks.insert(
                    idx,
                    VirtualDesk {
                        id,
                        name: alloc::format!("桌面 {}", id),
                        windows: moved_wins,
                        mru: p.moved.iter().map(|(w, _)| *w).collect(),
                        snap_layouts: Vec::new(),
                    },
                );
                true
            }
            Some(_) => false, // 窗后到达：已真并入，如实回绝
            None => false,
        }
    }

    pub fn undo_pending(&self) -> bool {
        self.merge_undo.is_some()
    }

    pub fn pop_toast(&mut self) -> Option<String> {
        if self.toasts.is_empty() {
            None
        } else {
            Some(self.toasts.remove(0))
        }
    }

    // -- 缩略墙 ------------------------------------------------------------

    /// 当前桌面墙内窗口（分页：每页 12 格）。
    pub fn wall_windows(&self) -> Vec<u64> {
        let wins = &self.desks[self.active_desk].windows;
        let start = self.page * WALL_PAGE_CAP;
        wins.iter().skip(start).take(WALL_PAGE_CAP).copied().collect()
    }

    pub fn wall_pages(&self) -> usize {
        (self.desks[self.active_desk].windows.len() + WALL_PAGE_CAP - 1) / WALL_PAGE_CAP
    }

    pub fn next_page(&mut self) {
        if self.page + 1 < self.wall_pages() {
            self.page += 1;
        }
    }

    /// 网格列数自适应（3-4 列按窗口数）。
    pub fn grid_cols(&self) -> usize {
        let n = self.wall_windows().len();
        if n <= 6 {
            3
        } else {
            4
        }
    }

    /// 当前桌面卡描边强调色（其余中性——令牌面）。
    pub fn desk_card_token(&self, idx: usize) -> Token {
        if idx == self.active_desk {
            Token::Accent
        } else {
            Token::Off
        }
    }

    // -- 键盘路径 ----------------------------------------------------------

    fn sync_ring(&mut self) {
        let wall = self.wall_windows().len();
        let cards = self.desks.len();
        self.ring = FocusRing::new(wall + cards);
    }

    /// 方向键移动焦点（墙内→桌面条全可达）。
    pub fn key_move(&mut self, forward: bool) -> usize {
        self.ring.arrow(forward)
    }

    /// Enter 激活焦点项（墙内=切窗口前台；条卡=切桌面）。
    pub fn key_activate(&mut self) -> Option<u64> {
        let idx = self.ring.activate();
        let wall = self.wall_windows().len();
        if idx < wall {
            let win = self.wall_windows()[idx];
            self.activate_window(self.active_desk, win);
            Some(win)
        } else {
            let card = idx - wall;
            if card < self.desks.len() {
                self.active_desk = card;
            }
            None
        }
    }

    pub fn focus_index(&self) -> usize {
        self.ring.index()
    }
}

// ---------------------------------------------------------------------------
// 自检（判据唯一源：主册 G-C-11 验收判据）
// ---------------------------------------------------------------------------

/// F081 自检：四桌面×三窗口压测、跨桌拖移焦点正确、关闭并入+撤销、
/// 分页墙、全键盘路径、重命名、动画账、Alt+Tab 桌面边界。
pub fn run_taskview_checks() -> CheckSet {
    let mut set = CheckSet::new("deskstar-F081");
    let mut tv = TaskView::new();
    // 1. 四桌面×三窗口压测：建到 4 桌、每桌放 3 窗。
    while tv.desk_count() < 4 {
        tv.create_desk(0);
    }
    for d in 0..4usize {
        for w in 0..3u64 {
            tv.place_window(d, d as u64 * 100 + w);
        }
    }
    set.add(
        "stress-4x3",
        tv.desk_count() == 4
            && tv.desks_iter_all().iter().all(|n| *n == 3),
        "4 desks × 3 windows",
    );
    // 2. 跨桌拖移焦点正确：拖窗后焦点账在落桌 MRU 头。
    tv.enter(1_000);
    let win = 0; // 桌面 1 的窗口
    tv.drag_window_to_desk(win, 3, 1_100);
    let head_ok = tv.mru_head_of(3) == Some(win);
    let moved_out = tv.mru_head_of(0) != Some(win);
    set.add(
        "cross-desk-focus",
        head_ok && moved_out && !tv.focus_moves.is_empty(),
        "focus follows window",
    );
    // 3. 关闭含窗桌面 → 并入前一桌 + toast 可撤销（窗内撤/窗后拒）。
    tv.close_desk(3, 2_000);
    // 桌 4 在检 2 被拖入 1 窗（4 窗）——并入桌 3 后 3+4=7。
    let merged = tv.desk_count() == 3 && tv.desk_window_count(2) == 7;
    let toast = tv.pop_toast().is_some();
    let undone = tv.undo_merge(2_500);
    let restored = tv.desk_count() == 4;
    tv.close_desk(3, 3_000);
    tv.tick_undo_window(3_000 + MERGE_UNDO_MS);
    let late = !tv.undo_merge(3_000 + MERGE_UNDO_MS + 1);
    set.add(
        "close-merge-undo",
        merged && toast && undone && restored && late,
        "merge + undo 5s",
    );
    // 4. 缩略墙分页（每页 12 格）与网格自适应。
    let mut tv2 = TaskView::new();
    for w in 0..15u64 {
        tv2.place_window(0, w);
    }
    tv2.enter(4_000);
    let pages = tv2.wall_pages();
    tv2.next_page();
    let page2 = tv2.wall_windows().len();
    let cols_small = tv2.grid_cols(); // 第 2 页 3 窗 → 3 列
    set.add(
        "wall-paging",
        pages == 2 && page2 == 3 && cols_small == 3,
        "12 per page, 3-4 cols",
    );
    // 5. 键盘路径全可达：方向键+Enter 切窗、切桌面。
    let mut tv3 = TaskView::new();
    tv3.create_desk(0);
    tv3.place_window(0, 10);
    tv3.place_window(0, 11);
    tv3.place_window(1, 12);
    tv3.enter(5_000);
    let picked = tv3.key_activate(); // 焦点起点=墙首位 → 窗 10
    let desk_switched = {
        tv3.key_move(true); // 窗 11
        tv3.key_move(true); // 桌面条卡 1
        tv3.key_move(true); // 桌面条卡 2
        tv3.key_activate();
        tv3.active_id() == 2
    };
    set.add(
        "keyboard-path",
        picked == Some(10) && desk_switched,
        "arrows + enter",
    );
    // 6. 重命名 + 桌面卡强调色（当前活动桌在 1 号位——键盘测试切过去的）。
    tv3.rename_active("工作台");
    set.add(
        "rename-card",
        tv3.active_name() == "工作台" && tv3.desk_card_token(1) == Token::Accent,
        "custom name + accent",
    );
    // 7. 动画账：250ms 进度收敛。
    tv3.leave(5_900); // 上检开着——重进才重置动画锚
    tv3.enter(6_000);
    tv3.now_ms = 6_000 + (ENTER_ANIM_MS as u64) / 2;
    let mid = tv3.enter_progress();
    tv3.now_ms = 6_000 + ENTER_ANIM_MS as u64;
    let end = tv3.enter_progress();
    set.add("enter-anim", mid > 0 && mid < 1000 && end == 1000, "250ms");
    // 8. Alt+Tab 边界：MRU 只投影当前桌面（跨桌用任务视图）。
    let head_d1 = tv3.mru_head_of(0);
    let head_d2 = tv3.mru_head_of(1);
    set.add(
        "alttab-boundary",
        head_d1.is_some() && head_d2.is_some() && head_d1 != head_d2,
        "per-desk MRU",
    );
    set
}

// 测试/自检辅助（只读投影——不进内核热路径）。
impl TaskView {
    pub fn desks_iter_all(&self) -> Vec<usize> {
        self.desks.iter().map(|d| d.windows.len()).collect()
    }

    pub fn desk_window_count(&self, idx: usize) -> usize {
        self.desks.get(idx).map(|d| d.windows.len()).unwrap_or(0)
    }

    pub fn mru_head_of(&self, idx: usize) -> Option<u64> {
        self.desks.get(idx).and_then(|d| d.mru.first().copied())
    }

    /// 撤销窗驱动（宿主滴答调用——到期真并入）。
    pub fn tick_undo_window(&mut self, now_ms: u64) {
        if let Some(p) = &self.merge_undo {
            if now_ms.saturating_sub(p.at_ms) >= MERGE_UNDO_MS {
                self.merge_undo = None;
            }
        }
        self.now_ms = now_ms;
    }
}

// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_desk_cannot_be_closed_below_one() {
        let mut tv = TaskView::new();
        assert!(!tv.close_desk(0, 0), "仅一桌不可关（保底桌面）");
        tv.create_desk(0);
        assert!(tv.close_desk(1, 0));
        assert_eq!(tv.desk_count(), 1);
    }

    #[test]
    fn drag_to_same_desk_noop() {
        let mut tv = TaskView::new();
        tv.place_window(0, 7);
        tv.drag_window_to_desk(7, 0, 0);
        assert_eq!(tv.desk_window_count(0), 1, "同桌拖移零动作");
        assert!(tv.focus_moves.is_empty());
    }

    #[test]
    fn merge_undo_restores_desk_with_windows() {
        let mut tv = TaskView::new();
        tv.create_desk(0);
        tv.place_window(0, 1);
        tv.place_window(0, 2);
        tv.place_window(1, 3);
        tv.close_desk(1, 1_000);
        assert_eq!(tv.desk_window_count(0), 3);
        assert!(tv.undo_merge(1_200));
        assert_eq!(tv.desk_count(), 2);
        assert_eq!(tv.desk_window_count(1), 1, "窗口搬回新桌");
        assert_eq!(tv.desk_window_count(0), 2);
    }

    #[test]
    fn wall_caps_at_twelve_per_page() {
        let mut tv = TaskView::new();
        for w in 0..30u64 {
            tv.place_window(0, w);
        }
        tv.enter(0);
        assert_eq!(tv.wall_pages(), 3);
        assert_eq!(tv.wall_windows().len(), 12);
    }

    #[test]
    fn grid_cols_adapts() {
        let mut tv = TaskView::new();
        for w in 0..8u64 {
            tv.place_window(0, w);
        }
        tv.enter(0);
        assert_eq!(tv.grid_cols(), 4, "7-12 窗 → 4 列");
    }

    #[test]
    fn taskview_self_checks_all_green() {
        let set = run_taskview_checks();
        let (p, f) = set.tally();
        assert!(set.all_passed(), "F081 自检红项：{}/{} 绿", p, p + f);
    }
}
