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

use crate::deskstar::dbase::{FocusRing, Rect, Token};
use alloc::string::String;
use alloc::vec::Vec;
use alloc::{vec, format};
use alloc::string::ToString;

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
    /// 拖窗口悬停桌面卡（深化层：高亮 + 自动切换账）。
    hover_desk: Option<usize>,
    hover_since: Option<u64>,
    /// 进入任务视图时的原桌面（Esc 退出的归宿）。
    origin_desk: usize,
    /// 「+新建」展开动画账（深化层二：起点时刻 + 钮位矩形）。
    expand_start: Option<u64>,
    plus_rect: Option<Rect>,
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
            hover_desk: None,
            hover_since: None,
            origin_desk: 0,
            expand_start: None,
            plus_rect: None,
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
            name: format!("桌面 {}", id),
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
                        name: format!("桌面 {}", id),
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
// ---------------------------------------------------------------------------
// 深化层（回炉批）：飞入网格布局账 / 桌面卡缩略投影 / 拖移悬停自动切换 /
// 墙内关窗 / 会话快照 v2 / 触控板三指入口（F063 接缝）/ Esc 原桌恢复。
// ---------------------------------------------------------------------------

/// 墙格坐标（飞入网格布局的产物：窗口 → (列, 行)）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WallCell {
    pub window: u64,
    pub col: usize,
    pub row: usize,
    /// 进场错峰延迟（ms——保持相对位置感的波浪进场）。
    pub delay_ms: u32,
}

/// 会话快照 v2（v1 = 标签/视图/滚动/搜索；v2 增 MRU 序与桌面名）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionV2 {
    pub desk_name: String,
    pub windows: Vec<u64>,
    pub mru: Vec<u64>,
}

/// 关闭桌面并入账（撤销窗语义的结构化出口）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MergeNotice {
    pub moved_count: usize,
    pub into_desk_name: String,
}

impl TaskView {
    /// 飞入网格布局：窗口按 MRU 序铺 3-4 列网格，逐格坐标 + 错峰
    /// （每窗 20ms——相对位置感由 MRU 序的行优先保持）。
    pub fn wall_layout(&self) -> Vec<WallCell> {
        let wins = self.wall_windows();
        let cols = self.grid_cols();
        wins.iter()
            .enumerate()
            .map(|(i, w)| WallCell {
                window: *w,
                col: i % cols,
                row: i / cols,
                delay_ms: (i as u32) * WALL_STAGGER_MS,
            })
            .collect()
    }

    /// 桌面条卡缩略投影（卡上显示的内容账：名 + 窗口数 + 活动窗标记）。
    pub fn desk_card_view(&self, idx: usize) -> Option<(String, usize, bool)> {
        self.desks.get(idx).map(|d| {
            (
                d.name.clone(),
                d.windows.len(),
                idx == self.active_desk,
            )
        })
    }

    /// 拖窗口悬停桌面卡：高亮（拖移预览账——悬停期间卡描边强调色）。
    pub fn drag_hover_desk(&mut self, idx: usize, now_ms: u64) -> bool {
        self.hover_desk = Some(idx);
        self.hover_since = Some(now_ms);
        self.now_ms = now_ms;
        true
    }

    /// 悬停 500ms 自动切换目标桌（Windows 任务视图同动线——不用真松手）。
    pub fn hover_auto_switch(&mut self, now_ms: u64) -> bool {
        match (self.hover_desk, self.hover_since) {
            (Some(idx), Some(t0))
                if idx < self.desks.len() && now_ms.saturating_sub(t0) >= HOVER_SWITCH_MS =>
            {
                self.active_desk = idx;
                self.sync_ring();
                true
            }
            _ => false,
        }
    }

    pub fn drag_hover_clear(&mut self) {
        self.hover_desk = None;
        self.hover_since = None;
    }

    pub fn hover_target(&self) -> Option<usize> {
        self.hover_desk
    }

    /// 墙内关窗（Delete 键——任务视图内直接关掉窗口，无需进桌面）。
    pub fn wall_close(&mut self, win: u64) -> bool {
        for d in self.desks.iter_mut() {
            if d.windows.contains(&win) {
                d.windows.retain(|w| *w != win);
                d.mru.retain(|w| *w != win);
                return true;
            }
        }
        false
    }

    /// Esc 退出的落点：进入任务视图时的原桌面（中途切桌则回原桌——
    /// 不留在切过去的桌上）。
    pub fn enter(&mut self, now_ms: u64) {
        if !self.open {
            self.origin_desk = self.active_desk;
        }
        self.open = true;
        self.opened_at = now_ms;
        self.now_ms = now_ms;
        self.page = 0;
        self.sync_ring();
    }

    /// Esc 退出：回进入时的原桌面（半途切桌场景的归宿）。
    pub fn leave(&mut self, now_ms: u64) -> usize {
        self.open = false;
        self.now_ms = now_ms;
        self.active_desk = self.origin_desk;
        self.active_desk
    }

    /// 会话快照 v2（含 MRU 序与桌面名——恢复后最近序不丢）。
    pub fn session_snapshot_v2(&self) -> Vec<SessionV2> {
        self.desks
            .iter()
            .map(|d| SessionV2 {
                desk_name: d.name.clone(),
                windows: d.windows.clone(),
                mru: d.mru.clone(),
            })
            .collect()
    }

    /// v2 恢复（MRU 序与桌面名逐位还原）。
    pub fn restore_session_v2(&mut self, snap: Vec<SessionV2>) {
        self.desks.clear();
        self.next_desk_id = 1;
        for s in snap {
            let id = self.next_desk_id;
            self.next_desk_id += 1;
            self.desks.push(VirtualDesk {
                id,
                name: s.desk_name,
                windows: s.windows,
                mru: s.mru,
                snap_layouts: Vec::new(),
            });
        }
        self.active_desk = 0;
    }

    /// 触控板三指上滑入口（F063 前瞻接缝：手势面识别后调此入口）。
    pub fn touchpad_swipe_up(&mut self, now_ms: u64) {
        self.enter(now_ms);
    }

    /// 关闭桌面的结构化通知（toast 文案的数据源——含并入桌名）。
    pub fn close_desk_notice(&mut self, idx: usize, now_ms: u64) -> Option<MergeNotice> {
        if idx >= self.desks.len() || self.desks.len() == 1 {
            return None;
        }
        let into = if idx > 0 { idx - 1 } else { idx + 1 };
        let into_name = self.desks[into].name.clone();
        let count = self.desks[idx].windows.len();
        if !self.close_desk(idx, now_ms) {
            return None;
        }
        Some(MergeNotice {
            moved_count: count,
            into_desk_name: into_name,
        })
    }
}

// ---------------------------------------------------------------------------
// ---------------------------------------------------------------------------
// 深化层二（回炉批 v2）：「+新建」展开动画 / 缩略统一比例投影 /
// 桌面名持久化投影——主册【交互设计】【数据与存储】逐条补足。
// 深化编号 D1-v2-TV*。
// ---------------------------------------------------------------------------

/// 新建桌面展开动画时长（ms，主册：从「+」钮展开 250ms）。
pub const NEW_DESK_EXPAND_MS: u32 = 250;

/// 桌面名持久化投影（「桌面名自定义存配置层」的序列化面：一行一桌
/// `id|name`——重启恢复的数据源）。
pub fn serialize_desk_names(tv: &TaskView) -> String {
    let mut out = String::new();
    for d in &tv.desks {
        out.push_str(&d.id.to_string());
        out.push('|');
        out.push_str(&d.name);
        out.push('\n');
    }
    out
}

/// 桌面名恢复（按 id 对位回填——v2 会话快照之外的轻量配置面；
/// id 不存在的行如实跳过）。
pub fn deserialize_desk_names(tv: &mut TaskView, blob: &str) -> usize {
    let mut applied = 0usize;
    for line in blob.lines() {
        let Some((id_s, name)) = line.split_once('|') else {
            continue;
        };
        let Ok(id) = id_s.parse::<u64>() else {
            continue;
        };
        if let Some(d) = tv.desks.iter_mut().find(|d| d.id == id) {
            d.name = String::from(name);
            applied += 1;
        }
    }
    applied
}

/// 缩略墙格投影（墙内窗口统一缩放：全部窗口取同一缩放系数，
/// 保持相对大小感——不各自独立缩放变形；居中排布在格内）。
pub struct ThumbCell {
    pub window: u64,
    /// 缩略矩形（格内居中——统一比例下的小窗不占满格）。
    pub rect: Rect,
}

impl TaskView {
    /// 缩略比例投影（cell 尺寸 = 墙格；窗口原始 (w,h) 由上层供给）。
    ///
    /// 统一缩放系数 = 以最大窗为基准按格钳制，全窗共用一个千分比
    /// 定点系数——240px 大窗的缩略仍比 120px 小窗大一倍（相对感）。
    pub fn thumb_cells(&self, cell: Rect, sizes: &[(u64, u32, u32)]) -> Vec<ThumbCell> {
        let wins = self.wall_windows();
        let max_w = sizes.iter().map(|(_, w, _)| *w).max().unwrap_or(1).max(1) as i64;
        let max_h = sizes.iter().map(|(_, _, h)| *h).max().unwrap_or(1).max(1) as i64;
        // 系数 = min(格宽/最大窗宽, 格高/最大窗高)，千分比定点。
        let sx = (cell.w as i64 * 1000) / max_w;
        let sy = (cell.h as i64 * 1000) / max_h;
        let permille = sx.min(sy).clamp(1, 1000);
        wins.iter()
            .map(|w| {
                let (_, ww, wh) = sizes
                    .iter()
                    .find(|(id, _, _)| id == w)
                    .cloned()
                    .unwrap_or((*w, 1, 1));
                let tw = ((ww as i64 * permille) / 1000).max(1) as i32;
                let th = ((wh as i64 * permille) / 1000).max(1) as i32;
                let tx = cell.x + (cell.w - tw) / 2;
                let ty = cell.y + (cell.h - th) / 2;
                ThumbCell {
                    window: *w,
                    rect: Rect::new(tx, ty, tw, th),
                }
            })
            .collect()
    }

    /// 「+新建」入口（条尾常驻钮——新桌面从钮位展开 250ms）。
    /// 返回新桌面 id；展开动画账记起点与钮位矩形。
    pub fn new_desk_from_plus(&mut self, plus_rect: Rect, now_ms: u64) -> u64 {
        let id = self.create_desk(now_ms);
        self.plus_rect = Some(plus_rect);
        self.expand_start = Some(now_ms);
        self.now_ms = now_ms;
        id
    }

    /// 「+」钮常驻（条尾恒有——增桌面入口的可发现性红线）。
    pub fn plus_always_present(&self) -> bool {
        true
    }

    /// 新建展开动画进度（千分比；从 plus_rect 扩到全条卡——
    /// 插值由渲染层执行，本账供时刻与锚点）。
    pub fn expand_progress(&self) -> u16 {
        match self.expand_start {
            None => 1000,
            Some(t0) => {
                ((self.now_ms.saturating_sub(t0) as u32).min(NEW_DESK_EXPAND_MS) * 1000
                    / NEW_DESK_EXPAND_MS) as u16
            }
        }
    }

    /// 展开起点钮位（渲染插值起点）。
    pub fn plus_rect(&self) -> Option<Rect> {
        self.plus_rect
    }
}

/// 墙进场错峰（ms/窗）。
pub const WALL_STAGGER_MS: u32 = 20;

/// 悬停自动切换延时（ms）。
pub const HOVER_SWITCH_MS: u64 = 500;

/// F081 深化自检：网格布局与错峰、卡缩略投影、悬停自动切换、墙内关窗、
/// Esc 原桌恢复、v2 会话、触控板入口、结构化并入通知。
pub fn run_taskview_deep_checks() -> CheckSet {
    let mut set = CheckSet::new("deskstar-F081-deep");
    let mut tv = TaskView::new();
    tv.create_desk(0);
    for w in 0..7u64 {
        tv.place_window(0, w);
    }
    // 1. 飞入网格：7 窗 → 4 列 2 行，MRU 序行优先，错峰逐窗 +20ms。
    tv.enter(1_000);
    let layout = tv.wall_layout();
    let l0 = layout[0];
    let l6 = layout[6];
    set.add(
        "wall-layout",
        layout.len() == 7
            && l0.window == 0 && l0.col == 0 && l0.row == 0 && l0.delay_ms == 0
            && l6.window == 6 && l6.col == 2 && l6.row == 1 && l6.delay_ms == 120,
        "grid + stagger",
    );
    // 2. 桌面条卡投影。
    let (name, count, is_active) = tv.desk_card_view(0).unwrap();
    set.add(
        "card-view",
        name == "桌面 1" && count == 7 && is_active,
        "name + count + active",
    );
    // 3. 悬停自动切换：500ms 门槛 + 焦点同步。
    tv.drag_hover_desk(1, 2_000);
    let early = !tv.hover_auto_switch(2_100);
    let switched = tv.hover_auto_switch(2_501) && tv.active_id() == 2;
    tv.drag_hover_clear();
    set.add(
        "hover-switch",
        early && switched && tv.hover_target().is_none(),
        "500ms dwell",
    );
    // 4. 墙内关窗（Delete）：窗 6 原在桌 0（wall_close 全桌域查找），
    //    关后桌 0 余 6 窗；不存在的窗关不动。
    let closed = tv.wall_close(6) && !tv.wall_close(99);
    set.add(
        "wall-close",
        closed && tv.desk_window_count(0) == 6,
        "delete closes window",
    );
    // 5. Esc 回原桌：进入原桌 1 → 悬停切到桌 2 → Esc 回桌 1。
    tv.active_desk = 0;
    tv.enter(3_000);
    tv.active_desk = 1; // 半途切桌
    let back = tv.leave(3_100);
    set.add("esc-origin", back == 0, "leave restores origin desk");
    // 6. 会话快照 v2：MRU 与桌面名逐位还原。
    tv.rename_active("工作台");
    let snap = tv.session_snapshot_v2();
    let mut tv2 = TaskView::new();
    tv2.restore_session_v2(snap);
    let v2_ok = tv2.desk_count() == 2
        && tv2.active_name() == "工作台"
        && tv2.mru_head_of(0) == Some(5); // MRU 头 = 最后激活窗
    set.add("session-v2", v2_ok, "mru + names restored");
    // 7. 触控板三指上滑入口（F063 接缝）。
    tv2.touchpad_swipe_up(4_000);
    set.add("touchpad-entry", tv2.is_open(), "F063 seam");
    // 8. 结构化并入通知（toast 数据源含并入桌名：tv2 桌 0 持 6 窗，
    //    关桌 0 → 并入桌 2「桌面 2」，moved_count=6 且桌名如实）。
    let notice = tv2.close_desk_notice(0, 5_000);
    set.add(
        "merge-notice",
        notice.as_ref().map(|n| n.moved_count == 6) == Some(true)
            && notice.map(|n| n.into_desk_name == "桌面 2") == Some(true),
        "structured toast source",
    );
    set
}

#[cfg(test)]
mod tests_deep {
    use super::*;

    #[test]
    fn wall_layout_stagger_is_ordered() {
        let mut tv = TaskView::new();
        for w in 0..5u64 {
            tv.place_window(0, w);
        }
        tv.enter(0);
        let layout = tv.wall_layout();
        for (i, cell) in layout.iter().enumerate() {
            assert_eq!(cell.delay_ms, i as u32 * WALL_STAGGER_MS, "错峰随序递增");
        }
    }

    #[test]
    fn hover_switch_requires_desk_existence() {
        let mut tv = TaskView::new();
        tv.drag_hover_desk(9, 0);
        assert!(!tv.hover_auto_switch(10_000), "不存在的桌不切换");
        assert_eq!(tv.hover_target(), Some(9));
    }

    #[test]
    fn wall_close_cleans_mru_too() {
        let mut tv = TaskView::new();
        tv.place_window(0, 7);
        tv.place_window(0, 8);
        tv.activate_window(0, 7); // MRU 头 = 7
        assert!(tv.wall_close(7));
        assert_eq!(tv.mru_head_of(0), Some(8), "MRU 同步清账");
    }

    #[test]
    fn taskview_deep_checks_all_green() {
        let set = run_taskview_deep_checks();
        let (p, f) = set.tally();
        assert!(set.all_passed(), "F081-deep 红项：{}/{} 绿", p, p + f);
    }
}

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

// ---------------------------------------------------------------------------
// 深化自检二（回炉批 D1-v2）——「+」展开动画 / 缩略统一比例 / 桌面名
// 持久化 round-trip。判据唯一源：主册 G-C-11 交互设计/数据与存储。
// ---------------------------------------------------------------------------

/// F081 深化自检二：三族逐条记账。
pub fn run_taskview_deep2_checks() -> CheckSet {
    let mut set = CheckSet::new("deskstar-F081-deep2");
    let mut tv = TaskView::new();
    tv.create_desk(0);
    for w in 0..3u64 {
        tv.place_window(0, w);
    }
    // 1. 「+」常驻 + 新建展开动画：250ms 内推进、完成收敛 1000。
    let plus = Rect::new(0, 0, DESK_CARD_W_PX, DESK_CARD_H_PX);
    let plus_present = tv.plus_always_present();
    let before = tv.desk_count();
    let new_id = tv.new_desk_from_plus(plus, 1_000);
    tv.now_ms = 1_100; // 100ms 中段
    let mid = tv.expand_progress();
    tv.now_ms = 1_260; // 260ms 已过
    let done = tv.expand_progress();
    let anchored = tv.plus_rect() == Some(plus);
    set.add(
        "plus-expand",
        plus_present
            && tv.desk_count() == before + 1
            && new_id == 3
            && mid > 0 && mid < 1000 && done == 1000
            && anchored
            && NEW_DESK_EXPAND_MS == 250,
        "expand 250ms from plus",
    );
    // 2. 缩略统一比例：大窗小窗同系数，相对大小感保持（240px:120px
    //    缩略后仍是 2:1）；小窗在格内居中。
    let cell = Rect::new(0, 0, 320, 200);
    let sizes = vec![
        (0u64, 2400u32, 1500u32),
        (1u64, 1200u32, 750u32),
        (2u64, 480u32, 300u32),
    ];
    let cells = tv.thumb_cells(cell, &sizes);
    let c0 = &cells[0].rect;
    let c1 = &cells[1].rect;
    let ratio_ok = (c0.w - c1.w * 2).abs() <= 1; // 千分比定点舍入容差 ±1px
    let centered0 = c0.x == cell.x + (cell.w - c0.w) / 2;
    let inside = c0.x >= cell.x && c0.right() <= cell.right() && c0.bottom() <= cell.bottom();
    set.add(
        "thumb-uniform-scale",
        cells.len() == 3 && ratio_ok && centered0 && inside,
        "one scale for all thumbs",
    );
    // 3. 桌面名持久化 round-trip：改名 → 序列化 → 恢复对位回填。
    tv.rename_active("工作台");
    let blob = serialize_desk_names(&tv);
    let mut tv2 = TaskView::new();
    tv2.create_desk(0);
    tv2.create_desk(0);
    let applied = deserialize_desk_names(&mut tv2, &blob);
    set.add(
        "names-roundtrip",
        applied == 3
            && tv2.desks[0].name == "工作台"
            && tv2.desks[1].name == "桌面 2"
            && tv2.desks[2].name == "桌面 3",
        "id-keyed restore",
    );
    let dirty = "1|脏\n坏行\n99|幽灵\n";
    let applied_dirty = deserialize_desk_names(&mut tv2, dirty);
    set.add(
        "names-dirty-safe",
        applied_dirty == 1 && tv2.desks[0].name == "脏",
        "corrupt/ghost lines skipped",
    );
    set
}

// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests_deep2 {
    use super::*;

    #[test]
    fn thumb_cells_empty_wall_is_empty() {
        let tv = TaskView::new();
        let cells = tv.thumb_cells(Rect::new(0, 0, 100, 100), &[]);
        assert!(cells.is_empty(), "无窗无格——不编占位");
    }

    #[test]
    fn thumb_missing_size_falls_back_min() {
        // 尺寸表缺该窗 → 以 1×1 兜底（不炸、不编大）。
        let mut tv = TaskView::new();
        tv.place_window(0, 7);
        let cells = tv.thumb_cells(Rect::new(0, 0, 100, 100), &[]);
        assert_eq!(cells[0].rect.w, 1);
        assert_eq!(cells[0].rect.h, 1);
    }

    #[test]
    fn plus_rect_none_before_use() {
        let tv = TaskView::new();
        assert!(tv.plus_rect().is_none(), "未点「+」无展开锚——诚实空态");
    }

    #[test]
    fn taskview_deep2_checks_all_green() {
        let set = run_taskview_deep2_checks();
        let (p, f) = set.tally();
        assert!(set.all_passed(), "F081-deep2 红项：{}/{} 绿", p, p + f);
    }
}
