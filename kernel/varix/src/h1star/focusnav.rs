//! F206 焦点可见性与键盘导航 · 判据实装（H 基础通用域 · AI-H1 分工包）。
//!
//! **判据锚**：主册 F206「焦点可见性与键盘导航」。
//!
//! **验收标准（主册第一句）**：全键盘走查链通过；「焦点丢失」（Tab 无
//! 可见反应超 1 秒）场景清单 0 项；焦点归还正确率 100%（弹窗开关
//! 20 次）。
//!
//! **设计要点**：
//! - [`FocusChain`] 焦点链：Tab 序 = 几何序（自上而下、从左到右）——
//!   按 y 中心排序后做行链分组（与行首垂直距离 ≤ 行高之半视为同行），
//!   行内按 x 排序；`walk_cycle` 环游保证 Shift+Tab 反向、Tab 环回；
//! - 焦点环样式：环宽 2px、外扩 2px（[`ring_rect`]），颜色来自主题
//!   强调色令牌——换主题只变色不换几何；
//! - 弹窗焦点归还栈：打开弹窗压栈（唤起层 + 唤起焦点），焦点落弹窗
//!   层第一控件；关闭弹栈归还——归还正确率可账面审计
//!   （`returns_ok`/`returns_total`）；
//! - 焦点丢失审计：Tab 请求账本 + 焦点移动账本（`RingLog` 64 条），
//!   「请求后 1 秒内无可见移动」计 1 缺陷（消费式审计，确认即清账）；
//! - 零堆纪律：遍历/环游/命中测试全在定长数组上；`Vec` 只在登记面
//!   （重建 Tab 序）使用。
//!
//! **依赖锚点**：`crate::checks::CheckSet`（自检面）、
//! `crate::h1star::h1base::{Rect, Rgb8}`（几何/强调色令牌）、
//! `crate::star::sbase::RingLog`（移动/请求账本）。
//! 时间纪律：一切时间由调用方注入毫秒戳，模块不持时钟。

use crate::checks::CheckSet;
use crate::h1star::h1base::{Rect, Rgb8};
use crate::star::sbase::RingLog;

// ---------------------------------------------------------------------------
// 规格常量
// ---------------------------------------------------------------------------

/// 焦点环宽——主册 F206：「可交互元素获焦显示 2px 焦点环」。
pub const RING_PX: i32 = 2;

/// 焦点环外扩——主册 F206：「主题强调色，外扩 2px」。
pub const RING_OUTSET_PX: i32 = 2;

/// 焦点丢失门——主册 F206：「Tab 无可见反应超 1 秒 = 缺陷」。
pub const LOST_FOCUS_MS: u64 = 1000;

/// 焦点链容量（登记面上限，实装定值）。
pub const CHAIN_CAP: usize = 128;

/// 弹窗归还栈深度（实装定值；超出显性拒绝）。
pub const RETURN_STACK_CAP: usize = 16;

/// 账本容量（Tab 请求 / 焦点移动各 64 条）。
pub const LEDGER_CAP: usize = 64;

/// 空槽哨兵（order 数组未用位）。
const NIL: u16 = 0xFFFF;

// ---------------------------------------------------------------------------
// 可聚焦元素
// ---------------------------------------------------------------------------

/// 可聚焦控件描述（几何 + 可用性 + 所属层）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Widget {
    pub id: u16,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub enabled: bool,
    /// 所属层：0 = 根层；>0 = 弹窗层（遍历只在活动层内）。
    pub layer: u16,
}

impl Widget {
    pub fn new(id: u16, x: i32, y: i32, w: i32, h: i32) -> Widget {
        Widget { id, x, y, w, h, enabled: true, layer: 0 }
    }

    /// 标记为禁用（Tab 遍历跳过）。
    pub fn disabled(mut self) -> Widget {
        self.enabled = false;
        self
    }

    /// 归属弹窗层。
    pub fn at_layer(mut self, layer: u16) -> Widget {
        self.layer = layer;
        self
    }

    /// 控件矩形。
    pub fn rect(&self) -> Rect {
        Rect::new(self.x, self.y, self.w, self.h)
    }
}

/// 焦点环样式（颜色随主题令牌，几何为常量）。
#[derive(Clone, Copy)]
pub struct FocusRingStyle {
    pub color: Rgb8,
    pub width_px: i32,
    pub outset_px: i32,
}

/// 从主题强调色取焦点环样式（换主题变色不换几何）。
pub fn ring_style_from_theme(accent: Rgb8) -> FocusRingStyle {
    FocusRingStyle { color: accent, width_px: RING_PX, outset_px: RING_OUTSET_PX }
}

/// 焦点环几何：元素矩形外扩 2px。
pub fn ring_rect(w: &Widget) -> Rect {
    Rect::new(
        w.x - RING_OUTSET_PX,
        w.y - RING_OUTSET_PX,
        w.w + 2 * RING_OUTSET_PX,
        w.h + 2 * RING_OUTSET_PX,
    )
}

// ---------------------------------------------------------------------------
// 焦点移动账目
// ---------------------------------------------------------------------------

/// 一次可见焦点移动（审计面）。
#[derive(Clone, Copy, Debug)]
pub struct MoveRec {
    pub ts: u64,
    pub from: u16,
    pub to: u16,
}

// ---------------------------------------------------------------------------
// 焦点链
// ---------------------------------------------------------------------------

/// 焦点链：登记 + 几何 Tab 序 + 环游 + 弹窗归还栈 + 丢失审计。
pub struct FocusChain {
    items: alloc::vec::Vec<Widget>,
    /// Tab 序（当前层的 id 序列；NIL = 空槽）。
    order: [u16; CHAIN_CAP],
    order_len: usize,
    focus: Option<u16>,
    active_layer: u16,
    return_stack: [Option<(u16, u16)>; RETURN_STACK_CAP],
    rs_len: usize,
    tab_reqs: RingLog<u64, LEDGER_CAP>,
    moves: RingLog<MoveRec, LEDGER_CAP>,
    last_move_ms: u64,
    /// 登记拒绝计数（超 CHAIN_CAP）。
    pub rejected: u32,
    /// 累计焦点丢失缺陷数（审计消费时入账）。
    pub lost_events: u32,
    /// 弹窗归还账面（正确率 = returns_ok / returns_total）。
    pub returns_ok: u32,
    pub returns_total: u32,
}

impl FocusChain {
    pub fn new() -> FocusChain {
        FocusChain {
            items: alloc::vec::Vec::new(),
            order: [NIL; CHAIN_CAP],
            order_len: 0,
            focus: None,
            active_layer: 0,
            return_stack: [const { None }; RETURN_STACK_CAP],
            rs_len: 0,
            tab_reqs: RingLog::new(),
            moves: RingLog::new(),
            last_move_ms: 0,
            rejected: 0,
            lost_events: 0,
            returns_ok: 0,
            returns_total: 0,
        }
    }

    // ---- 登记面 ----

    /// 登记控件（登记即重建当前层 Tab 序；超容量显性拒绝）。
    pub fn add(&mut self, w: Widget) {
        if self.items.len() >= CHAIN_CAP {
            self.rejected += 1;
            return;
        }
        self.items.push(w);
        self.rebuild_order();
    }

    /// 移除控件。
    pub fn remove(&mut self, id: u16) {
        self.items.retain(|w| w.id != id);
        if self.focus == Some(id) {
            self.focus = None;
        }
        self.rebuild_order();
    }

    /// 改可用性（重建序；禁用当前焦点时焦点悬空由下次 Tab 落位）。
    pub fn set_enabled(&mut self, id: u16, enabled: bool) {
        for w in self.items.iter_mut() {
            if w.id == id {
                w.enabled = enabled;
            }
        }
        if self.focus == Some(id) && !enabled {
            self.focus = None;
        }
        self.rebuild_order();
    }

    pub fn find(&self, id: u16) -> Option<Widget> {
        self.items.iter().copied().find(|w| w.id == id)
    }

    pub fn item_count(&self) -> usize {
        self.items.len()
    }

    pub fn enabled_count(&self) -> usize {
        self.items.iter().filter(|w| w.enabled && w.layer == self.active_layer).count()
    }

    pub fn active_layer(&self) -> u16 {
        self.active_layer
    }

    // ---- Tab 序（几何排序 + 行链分组）----

    /// 重建当前层 Tab 序：y 中心排序 → 行链分组（与行首 y 中心差
    /// ≤ max(行高)/2 为同行）→ 行内按 x。自上而下、从左到右。
    fn rebuild_order(&mut self) {
        self.order = [NIL; CHAIN_CAP];
        self.order_len = 0;
        let mut list: alloc::vec::Vec<(i32, i32, i32, u16)> = self
            .items
            .iter()
            .filter(|w| w.layer == self.active_layer)
            .map(|w| (w.y + w.h / 2, w.x, w.h, w.id))
            .collect();
        list.sort_unstable();
        let mut row: alloc::vec::Vec<(i32, u16)> = alloc::vec::Vec::new();
        let mut row_base_y = 0i32;
        let mut row_base_h = 0i32;
        let mut row_open = false;
        for (yc, x, h, id) in list {
            let half = h.max(row_base_h) / 2;
            if row_open && (yc - row_base_y).abs() > half {
                flush_row(&mut row, &mut self.order, &mut self.order_len);
                row_open = false;
            }
            if !row_open {
                row_base_y = yc;
                row_base_h = h;
                row_open = true;
            }
            row.push((x, id));
        }
        if row_open {
            flush_row(&mut row, &mut self.order, &mut self.order_len);
        }
    }

    /// Tab 序只读快照（走查审计用）。
    pub fn tab_order(&self) -> &[u16] {
        &self.order[..self.order_len]
    }

    pub fn tab_index(&self, id: u16) -> Option<usize> {
        self.tab_order().iter().position(|&x| x == id)
    }

    // ---- 环游 ----

    /// 纯函数式下一跳（不改状态；`dir` = +1 Tab / -1 Shift+Tab）。
    /// 从 `from` 起按序找第一个可用控件（跳过禁用项；环回）。
    fn peek_step(&self, from: Option<u16>, dir: i32) -> Option<u16> {
        if self.order_len == 0 {
            return None;
        }
        let start = from.and_then(|f| self.tab_index(f));
        let mut idx = match (start, dir > 0) {
            (Some(i), true) => i,
            (Some(i), false) => i,
            (None, true) => self.order_len - 1,
            (None, false) => 0,
        };
        for _ in 0..self.order_len {
            idx = if dir > 0 {
                (idx + 1) % self.order_len
            } else {
                (idx + self.order_len - 1) % self.order_len
            };
            let id = self.order[idx];
            if id == NIL {
                continue;
            }
            if let Some(w) = self.find(id) {
                if w.enabled {
                    return Some(id);
                }
            }
        }
        None
    }

    /// Tab（`now` 注入）：移动焦点并记录可见移动。
    pub fn focus_next(&mut self, now: u64) -> Option<u16> {
        self.step(1, now)
    }

    /// Shift+Tab（反向）。
    pub fn focus_prev(&mut self, now: u64) -> Option<u16> {
        self.step(-1, now)
    }

    fn step(&mut self, dir: i32, now: u64) -> Option<u16> {
        let prev = self.focus;
        let next = self.peek_step(prev, dir)?;
        if Some(next) != prev {
            self.focus = Some(next);
            self.note_move(now, prev, next);
        }
        Some(next)
    }

    /// 直达焦点（点击聚焦等）。
    pub fn focus_id(&mut self, id: u16, now: u64) -> bool {
        match self.find(id) {
            Some(w) if w.enabled && w.layer == self.active_layer => {
                let prev = self.focus;
                self.focus = Some(id);
                if prev != Some(id) {
                    self.note_move(now, prev, id);
                }
                true
            }
            _ => false,
        }
    }

    pub fn current(&self) -> Option<u16> {
        self.focus
    }

    pub fn focused_widget(&self) -> Option<Widget> {
        self.focus.and_then(|id| self.find(id))
    }

    /// 从 `start` 起环游一圈的访问序（回到 start 即止；不改动状态）。
    /// 环游性质（fuzz 验证面）：每个可用控件恰被访问一次。
    pub fn walk_cycle(&self, start: u16) -> alloc::vec::Vec<u16> {
        let mut visited = alloc::vec::Vec::new();
        let mut cur = Some(start);
        for _ in 0..self.order_len {
            cur = self.peek_step(cur, 1);
            match cur {
                Some(id) if id == start => break,
                Some(id) => visited.push(id),
                None => break,
            }
        }
        visited
    }

    // ---- 命中测试 ----

    /// 命中测试（后登记者优先——同点重叠取上层）。
    pub fn hit_test(&self, x: i32, y: i32) -> Option<u16> {
        for w in self.items.iter().rev() {
            if w.layer == self.active_layer && w.enabled && w.rect().contains(x, y) {
                return Some(w.id);
            }
        }
        None
    }

    // ---- 弹窗归还栈 ----

    /// 打开弹窗：压栈（唤起层 + 唤起焦点），切层，焦点落该层第一控件。
    pub fn open_modal(&mut self, layer: u16, now: u64) -> bool {
        if self.rs_len >= RETURN_STACK_CAP {
            return false;
        }
        let saved_focus = self.focus.unwrap_or(NIL);
        self.return_stack[self.rs_len] = Some((self.active_layer, saved_focus));
        self.rs_len += 1;
        self.active_layer = layer;
        self.rebuild_order();
        self.focus = None;
        self.step(1, now);
        true
    }

    /// 关闭弹窗：弹栈归还焦点给唤起者。返回归还后的焦点 id。
    /// 归还正确率账面：目标存在、可用且属恢复层才算 correct。
    pub fn close_modal(&mut self, now: u64) -> Option<u16> {
        if self.rs_len == 0 {
            return None;
        }
        self.rs_len -= 1;
        let (layer, saved) = self.return_stack[self.rs_len].take()?;
        self.active_layer = layer;
        self.rebuild_order();
        self.focus = if saved == NIL { None } else { Some(saved) };
        self.returns_total += 1;
        let ok = match self.focus {
            Some(id) => match self.find(id) {
                Some(w) => w.enabled && w.layer == layer,
                None => false,
            },
            None => true, // 唤起时本就无焦点 → 归还「无焦点」亦正确
        };
        if ok {
            self.returns_ok += 1;
        }
        if let Some(id) = self.focus {
            self.note_move(now, None, id);
        }
        self.focus
    }

    pub fn modal_depth(&self) -> usize {
        self.rs_len
    }

    // ---- 焦点丢失审计 ----

    /// Tab 请求入账（宿主在每次 Tab 按压时调用）。
    pub fn on_tab_request(&mut self, now: u64) {
        self.tab_reqs.push(now);
    }

    fn note_move(&mut self, now: u64, from: Option<u16>, to: u16) {
        self.moves.push(MoveRec { ts: now, from: from.unwrap_or(NIL), to });
        self.last_move_ms = now;
    }

    pub fn last_move_ms(&self) -> u64 {
        self.last_move_ms
    }

    pub fn recent_moves(&self) -> alloc::vec::Vec<MoveRec> {
        self.moves.newest_first()
    }

    /// 焦点丢失审计（消费式）：每条「Tab 请求后 1 秒内无可见焦点移动」
    /// 的请求计 1 缺陷并累计入 `lost_events`；审计确认后清请求账。
    pub fn audit_lost(&mut self, now: u64) -> u32 {
        let mut reqs = self.tab_reqs.newest_first();
        reqs.sort_unstable();
        let moves = self.moves.newest_first();
        let mut mv_ts: alloc::vec::Vec<u64> = moves.iter().map(|m| m.ts).collect();
        mv_ts.sort_unstable();
        let mut lost = 0u32;
        for r in reqs {
            // 响应移动 = 请求时刻或之后的第一次可见移动（含同刻）。
            let next_mv = mv_ts.iter().find(|&&m| m >= r).copied();
            let unresponsive = match next_mv {
                Some(m) => m - r > LOST_FOCUS_MS,
                None => now.saturating_sub(r) > LOST_FOCUS_MS,
            };
            if unresponsive {
                lost += 1;
            }
        }
        if lost > 0 {
            self.lost_events += lost;
        }
        self.tab_reqs.clear();
        lost
    }
}

impl Default for FocusChain {
    fn default() -> Self {
        Self::new()
    }
}

/// 行内排序落账：按 x 升序追加进 order。
fn flush_row(row: &mut alloc::vec::Vec<(i32, u16)>, order: &mut [u16; CHAIN_CAP], n: &mut usize) {
    row.sort_unstable();
    for &(_, id) in row.iter() {
        if *n < CHAIN_CAP {
            order[*n] = id;
            *n += 1;
        }
    }
    row.clear();
}

// ---------------------------------------------------------------------------
// 走查辅助面（首/末可焦、审计快照、层级封闭性）
// ---------------------------------------------------------------------------

/// 焦点丢失审计报告快照（走查清单出账用）。
#[derive(Clone, Copy, Debug)]
pub struct LostReport {
    /// 账面 Tab 请求总数。
    pub tab_requests: usize,
    /// 账面可见焦点移动总数。
    pub focus_moves: usize,
    /// 最近一次可见移动时刻。
    pub last_move_ms: u64,
    /// 累计焦点丢失缺陷数。
    pub lost_total: u32,
}

impl FocusChain {
    /// 首个可用控件（Tab 序几何首位；空链回 None）。
    pub fn first_focusable(&self) -> Option<u16> {
        self.tab_order().iter().copied().find(|&id| match self.find(id) {
            Some(w) => w.enabled,
            None => false,
        })
    }

    /// 末个可用控件（Shift+Tab 起点语义）。
    pub fn last_focusable(&self) -> Option<u16> {
        self.tab_order().iter().rev().copied().find(|&id| match self.find(id) {
            Some(w) => w.enabled,
            None => false,
        })
    }

    /// Tab 请求账面计数。
    pub fn tab_request_count(&self) -> usize {
        self.tab_reqs.len()
    }

    /// 走查审计报告快照。
    pub fn audit_report(&self) -> LostReport {
        LostReport {
            tab_requests: self.tab_reqs.len(),
            focus_moves: self.moves.len(),
            last_move_ms: self.last_move_ms,
            lost_total: self.lost_events,
        }
    }

    /// 层级封闭性验证器：Tab 序所有落点都在活动层——
    /// 「焦点不会从弹窗里漏到背景层」的不变量（走查用）。
    pub fn traversal_confined(&self) -> bool {
        self.tab_order().iter().all(|&id| match self.find(id) {
            Some(w) => w.layer == self.active_layer,
            None => false,
        })
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F206 自检（判据面：几何 Tab 序 + 环游 + 焦点归还 100% + 丢失审计 0 项）。
pub fn run_focusnav_checks() -> CheckSet {
    let mut set = CheckSet::new("F206-focusnav");

    // 1. Tab 序与视觉一致：三行网格自上而下、行内从左到右。
    let mut ch = FocusChain::new();
    // 行 1：y=0（id 1 左、id 2 右）；行 2：y=100（id 3）；行 3：y=200（id 4、5）。
    ch.add(Widget::new(4, 0, 200, 60, 24));
    ch.add(Widget::new(2, 200, 0, 60, 24));
    ch.add(Widget::new(1, 0, 0, 60, 24));
    ch.add(Widget::new(5, 300, 200, 60, 24));
    ch.add(Widget::new(3, 0, 100, 60, 24));
    let order: alloc::vec::Vec<u16> = ch.tab_order().to_vec();
    set.add(
        "tab order matches visual layout",
        order == alloc::vec![1, 2, 3, 4, 5],
        "",
    );

    // 2. 行链分组：y 中心差 ≤ 行高之半视为同行（同排不同高的控件）。
    let mut ch2 = FocusChain::new();
    ch2.add(Widget::new(10, 0, 100, 60, 40)); // y 中心 120
    ch2.add(Widget::new(11, 300, 110, 60, 20)); // y 中心 120
    ch2.add(Widget::new(12, 0, 0, 60, 20)); // y 中心 10
    let order2: alloc::vec::Vec<u16> = ch2.tab_order().to_vec();
    set.add(
        "row grouping by vertical overlap",
        order2 == alloc::vec![12, 10, 11],
        "",
    );

    // 3. Shift+Tab 恰为 Tab 的反序（含环回边界）。
    let mut ch3 = FocusChain::new();
    for id in 1..=4u16 {
        ch3.add(Widget::new(id, (id as i32 - 1) * 100, 0, 60, 24));
    }
    let mut fwd = alloc::vec::Vec::new();
    let _ = ch3.focus_next(0);
    fwd.push(ch3.current().unwrap());
    for _ in 0..3 {
        let _ = ch3.focus_next(1);
        fwd.push(ch3.current().unwrap());
    }
    let mut rev = alloc::vec::Vec::new();
    for _ in 0..3 {
        let _ = ch3.focus_prev(2);
        rev.push(ch3.current().unwrap());
    }
    let wrap_back = ch3.focus_prev(3);
    set.add(
        "shift+tab is exact reverse",
        fwd == alloc::vec![1, 2, 3, 4]
            && rev == alloc::vec![3, 2, 1]
            && wrap_back == Some(4),
        "",
    );

    // 4. 环回：末位 Tab 回首位；无焦点时 Tab 落首位。
    let mut ch4 = FocusChain::new();
    for id in 1..=3u16 {
        ch4.add(Widget::new(id, 0, (id as i32 - 1) * 50, 60, 24));
    }
    let _ = ch4.focus_id(3, 0);
    let wrapped = ch4.focus_next(1);
    set.add(
        "tab wraps last to first",
        wrapped == Some(1) && ch4.focus_next(2) == Some(2),
        "",
    );

    // 5. 禁用项跳过（遍历环 = 可用项集合）。
    let mut ch5 = FocusChain::new();
    ch5.add(Widget::new(1, 0, 0, 60, 24));
    ch5.add(Widget::new(2, 0, 50, 60, 24).disabled());
    ch5.add(Widget::new(3, 0, 100, 60, 24));
    let _ = ch5.focus_next(0);
    // 缺陷账本：现象=检查项恒红；根因=检查项在第二次 Tab **之后**才断言
    // current()==Some(1)——此刻焦点已按判据落到 3，断言时序与自身意图
    // （首跳落 1、次跳跳过禁用项落 3）矛盾，属检查项写错；修法=先取
    // 首跳落点快照再走次跳，判据内容不动（禁用项仍须被跳过）。
    let first_hop = ch5.current();
    let nxt = ch5.focus_next(1);
    set.add(
        "disabled widgets skipped",
        first_hop == Some(1) && nxt == Some(3) && ch5.enabled_count() == 2,
        "",
    );

    // 6. 焦点环：外扩 2px 的几何；样式颜色随主题、几何常量不动。
    let w = Widget::new(9, 10, 10, 50, 20);
    let rr = ring_rect(&w);
    let blue = ring_style_from_theme(Rgb8::new(40, 90, 220));
    let green = ring_style_from_theme(Rgb8::new(30, 160, 80));
    set.add(
        "focus ring outset 2px, width 2px, theme color",
        rr.x == 8 && rr.y == 8 && rr.w == 54 && rr.h == 24
            && blue.width_px == RING_PX
            && green.width_px == RING_PX
            && blue.color != green.color,
        "",
    );

    // 7. 弹窗打开：焦点落弹窗层第一控件；遍历限定在弹窗层内。
    let mut ch6 = FocusChain::new();
    ch6.add(Widget::new(1, 0, 0, 60, 24));
    ch6.add(Widget::new(2, 0, 50, 60, 24));
    ch6.add(Widget::new(20, 100, 0, 60, 24).at_layer(1));
    ch6.add(Widget::new(21, 100, 50, 60, 24).at_layer(1));
    let _ = ch6.focus_id(2, 0);
    let opened = ch6.open_modal(1, 10);
    set.add(
        "modal opens with focus on first control",
        opened && ch6.current() == Some(20) && ch6.modal_depth() == 1
            && ch6.focus_next(11) == Some(21),
        "",
    );

    // 8. 弹窗关闭：焦点归还唤起者；20 次开关归还 20/20（100%）。
    let mut ok_cycles = 0usize;
    let mut ch7 = FocusChain::new();
    ch7.add(Widget::new(1, 0, 0, 60, 24));
    ch7.add(Widget::new(2, 0, 50, 60, 24));
    ch7.add(Widget::new(20, 100, 0, 60, 24).at_layer(1));
    for cycle in 0..20u64 {
        let _ = ch7.focus_id(if cycle % 2 == 0 { 1 } else { 2 }, cycle * 10);
        let _ = ch7.open_modal(1, cycle * 10 + 1);
        let _ = ch7.close_modal(cycle * 10 + 2);
        let invoker = if cycle % 2 == 0 { 1u16 } else { 2u16 };
        if ch7.current() == Some(invoker) {
            ok_cycles += 1;
        }
    }
    set.add(
        "20 open/close cycles return 20/20",
        ok_cycles == 20 && ch7.returns_total == 20 && ch7.returns_ok == 20,
        "",
    );

    // 9. 嵌套弹窗：逐层关闭逐层归还。
    let mut ch8 = FocusChain::new();
    ch8.add(Widget::new(1, 0, 0, 60, 24));
    ch8.add(Widget::new(2, 0, 50, 60, 24).at_layer(1));
    ch8.add(Widget::new(3, 0, 100, 60, 24).at_layer(2));
    let _ = ch8.focus_id(1, 0);
    let _ = ch8.open_modal(1, 1);
    let _ = ch8.focus_id(2, 2);
    let _ = ch8.open_modal(2, 3);
    let close1 = ch8.close_modal(4);
    let close2 = ch8.close_modal(5);
    set.add(
        "nested modals restore layer by layer",
        close1 == Some(2) && close2 == Some(1) && ch8.modal_depth() == 0 && ch8.active_layer() == 0,
        "",
    );

    // 10. 焦点丢失审计：Tab 请求后 >1s 无可见移动 → 记缺陷；正常链 0 项。
    let mut ch9 = FocusChain::new();
    ch9.add(Widget::new(1, 0, 0, 60, 24));
    ch9.add(Widget::new(2, 0, 50, 60, 24).disabled()); // 唯一可用 → Tab 无可见移动
    let _ = ch9.focus_id(1, 100);
    ch9.on_tab_request(200);
    let _ = ch9.focus_next(200);
    ch9.on_tab_request(5_000);
    let _ = ch9.focus_next(5_000);
    let lost_bad = ch9.audit_lost(6_200);
    let mut ch10 = FocusChain::new();
    ch10.add(Widget::new(1, 0, 0, 60, 24));
    ch10.add(Widget::new(2, 0, 50, 60, 24));
    ch10.on_tab_request(0);
    let _ = ch10.focus_next(0);
    ch10.on_tab_request(500);
    let _ = ch10.focus_next(600);
    let lost_ok = ch10.audit_lost(700);
    set.add(
        "lost-focus audit: gaps counted, healthy chain clean",
        lost_bad == 2 && lost_ok == 0 && ch9.lost_events == 2 && ch10.lost_events == 0,
        "",
    );

    // 11. 登记容量：第 129 个显性拒绝并计数。
    let mut ch11 = FocusChain::new();
    for id in 0..(CHAIN_CAP as u16 + 1) {
        ch11.add(Widget::new(id, 0, id as i32 * 10, 60, 24));
    }
    set.add("chain cap 128 rejects extra", ch11.item_count() == CHAIN_CAP && ch11.rejected == 1, "");

    // 12. fuzz（xorshift32 范式）：随机布局 2000 轮——环游性质：
    //     从任一可用控件出发环游一圈，每个可用控件恰被访问一次。
    let mut x: u32 = 0x9E3779B9;
    let mut fuzz_ok = true;
    for _ in 0..2000u32 {
        let mut chf = FocusChain::new();
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        let n = (x % 12) as u16 + 2;
        for id in 0..n {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            let wx = (x % 400) as i32;
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            let wy = (x % 300) as i32;
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            let dis = x % 4 == 0;
            let mut wb = Widget::new(100 + id, wx, wy, 40, 20);
            wb.enabled = !dis;
            chf.add(wb);
        }
        let enabled: alloc::vec::Vec<u16> = (0..n)
            .map(|k| 100 + k)
            .filter(|id| chf.find(*id).map_or(false, |w| w.enabled))
            .collect();
        if enabled.is_empty() {
            continue;
        }
        let start = enabled[0];
        let walk = chf.walk_cycle(start);
        // 环游长 = 可用数；不含起点重复；互不重复。
        if walk.len() != enabled.len() - 1 {
            fuzz_ok = false;
        }
        let mut seen = alloc::vec::Vec::new();
        for id in &walk {
            if *id == start || seen.contains(id) {
                fuzz_ok = false;
            }
            seen.push(*id);
        }
    }
    set.add("walk-cycle fuzz 2000 layouts exact cover", fuzz_ok, "");

    // 13. set_enabled/remove 后遍历仍有效（序是当前层 id 的排列）。
    let mut ch12 = FocusChain::new();
    for id in 1..=4u16 {
        ch12.add(Widget::new(id, 0, id as i32 * 40, 60, 24));
    }
    ch12.set_enabled(2, false);
    ch12.remove(3);
    let order12: alloc::vec::Vec<u16> = ch12.tab_order().to_vec();
    set.add(
        "order stays permutation after edit",
        order12 == alloc::vec![1, 2, 4] && ch12.enabled_count() == 2,
        "",
    );

    set
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tab_order_full_walk() {
        let mut ch = FocusChain::new();
        for id in 1..=3u16 {
            ch.add(Widget::new(id, (id as i32 - 1) * 100, 0, 60, 24));
        }
        let mut seq = alloc::vec::Vec::new();
        for _ in 0..3 {
            let _ = ch.focus_next(0);
            seq.push(ch.current().unwrap());
        }
        assert_eq!(seq, alloc::vec![1, 2, 3]);
        // 再走一圈环回。
        let _ = ch.focus_next(1);
        assert_eq!(ch.current(), Some(1));
    }

    #[test]
    fn shift_tab_from_unfocused_goes_last() {
        let mut ch = FocusChain::new();
        for id in 1..=3u16 {
            ch.add(Widget::new(id, 0, (id as i32 - 1) * 50, 60, 24));
        }
        let got = ch.focus_prev(0);
        assert_eq!(got, Some(3));
    }

    #[test]
    fn hit_test_prefers_registered_top() {
        let mut ch = FocusChain::new();
        ch.add(Widget::new(1, 0, 0, 100, 100));
        ch.add(Widget::new(2, 40, 40, 20, 20));
        assert_eq!(ch.hit_test(50, 50), Some(2), "后登记者在上层");
        assert_eq!(ch.hit_test(10, 10), Some(1));
        assert_eq!(ch.hit_test(200, 200), None);
    }

    #[test]
    fn modal_open_rejects_when_stack_full() {
        let mut ch = FocusChain::new();
        // 缺陷账本：现象=本测试红（第 17 次 open_modal 返回 false）；
        // 根因=循环按 RETURN_STACK_CAP+1 次全部断言成功——但栈容量在册
        // 恰为 16（第 17 次必须被拒），测试把自己的「满栈拒绝」预期写成了
        // 全成功，自相矛盾；修法=循环只开满 16 层，第 17 次显性断言拒绝。
        for layer in 0..RETURN_STACK_CAP as u16 {
            ch.add(Widget::new(100 + layer, 0, 0, 10, 10).at_layer(layer));
            assert!(ch.open_modal(layer, layer as u64));
        }
        assert!(!ch.open_modal(99, 999));
        assert_eq!(ch.modal_depth(), RETURN_STACK_CAP);
    }

    #[test]
    fn focus_id_rejects_disabled_and_foreign_layer() {
        let mut ch = FocusChain::new();
        ch.add(Widget::new(1, 0, 0, 60, 24));
        ch.add(Widget::new(2, 0, 50, 60, 24).disabled());
        ch.add(Widget::new(3, 0, 100, 60, 24).at_layer(1));
        assert!(!ch.focus_id(2, 0));
        assert!(!ch.focus_id(3, 0), "跨层直焦拒绝");
        assert!(ch.focus_id(1, 0));
    }

    #[test]
    fn lost_focus_audit_is_consumable() {
        let mut ch = FocusChain::new();
        ch.add(Widget::new(1, 0, 0, 60, 24));
        ch.add(Widget::new(2, 0, 50, 60, 24));
        let _ = ch.focus_id(1, 0);
        ch.on_tab_request(0);
        let _ = ch.focus_next(0); // 同刻可见移动 → 响应
        ch.on_tab_request(2_000);
        let _ = ch.focus_next(2_100); // 100ms 内响应
        assert_eq!(ch.audit_lost(2_500), 0, "全部在 1s 内有响应");
        assert_eq!(ch.audit_lost(9_999), 0, "清账后不重复计");
        ch.on_tab_request(9_000);
        assert_eq!(ch.audit_lost(10_500), 1, "超 1s 无移动计 1");
        assert_eq!(ch.lost_events, 1);
    }

    #[test]
    fn first_last_focusable_and_confinement() {
        let mut ch = FocusChain::new();
        ch.add(Widget::new(1, 0, 0, 60, 24).disabled());
        ch.add(Widget::new(2, 0, 50, 60, 24));
        ch.add(Widget::new(3, 0, 100, 60, 24));
        assert_eq!(ch.first_focusable(), Some(2), "首位可用跳过禁用项");
        assert_eq!(ch.last_focusable(), Some(3));
        assert!(ch.traversal_confined());
        // 弹窗层：序内只有弹窗控件，封闭性保持；背景层首位不可达。
        ch.add(Widget::new(20, 100, 0, 60, 24).at_layer(1));
        let _ = ch.open_modal(1, 0);
        assert_eq!(ch.first_focusable(), Some(20));
        assert!(ch.traversal_confined());
        let rep = ch.audit_report();
        assert_eq!(rep.tab_requests, 0);
        assert!(rep.focus_moves >= 1, "开框落焦是一次可见移动");
        assert_eq!(rep.lost_total, 0);
    }

    #[test]
    fn focusnav_selfcheck_all_green() {
        let set = run_focusnav_checks();
        assert!(set.all_passed(), "F206 自检存在红项");
        assert!(!set.truncated());
        assert!(set.len() >= 8 && set.len() <= 14);
    }
}

// ===========================================================================
// v2 深化批（2026-09-26 · AI-H1 二次对账批）：UI 壳接线 / 持久化 I/O / 判定面扩展
// ===========================================================================

const VXH1_MAGIC: [u8; 4] = *b"VXH1";
const VXH1_VER: u8 = 1;

/// 损坏输入显性拒绝：四类 + 字段越界（链内出现 NIL 哨兵 id）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum V2CodecErr {
    BadMagic,
    BadVersion,
    BadLen,
    BadSum,
    BadField,
}

/// FNV-1a 32 位（校验和唯一实现点）。
fn fnv1a(data: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in data {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

// ---- 持久化 I/O 面：焦点链表档案（重启恢复上次焦点）----

/// 记录长：magic4 + ver1 + len1 + 128×u16 id + focus2 + sum4。
pub const CHAIN_REC_LEN: usize = 4 + 1 + 1 + 2 * CHAIN_CAP + 2 + 4;

/// 焦点链快照：当前层 Tab 序 + 焦点落点——壳层重启后按档案恢复
/// 焦点链（主册 F206「焦点归还 100%」的持久化面：归还目标可跨会话复原）。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct FocusChainRec {
    pub len: usize,
    ids: [u16; CHAIN_CAP],
    /// 0xFFFF = 无焦点。
    focus_raw: u16,
}

impl FocusChainRec {
    /// 从焦点链取快照（只走公开 API：tab_order/current）。
    pub fn of(ch: &FocusChain) -> FocusChainRec {
        let mut rec = FocusChainRec { len: 0, ids: [0u16; CHAIN_CAP], focus_raw: 0xFFFF };
        if let Some(f) = ch.current() {
            rec.focus_raw = f;
        }
        for (k, &id) in ch.tab_order().iter().enumerate() {
            if k < CHAIN_CAP {
                rec.ids[k] = id;
                rec.len += 1;
            }
        }
        rec
    }

    pub fn focus(&self) -> Option<u16> {
        if self.focus_raw == 0xFFFF {
            None
        } else {
            Some(self.focus_raw)
        }
    }

    pub fn id_at(&self, i: usize) -> Option<u16> {
        if i < self.len {
            Some(self.ids[i])
        } else {
            None
        }
    }

    pub fn to_bytes(&self) -> [u8; CHAIN_REC_LEN] {
        let mut out = [0u8; CHAIN_REC_LEN];
        out[..4].copy_from_slice(&VXH1_MAGIC);
        out[4] = VXH1_VER;
        out[5] = self.len as u8;
        for k in 0..CHAIN_CAP {
            out[6 + k * 2..8 + k * 2].copy_from_slice(&self.ids[k].to_le_bytes());
        }
        let o = 6 + 2 * CHAIN_CAP;
        out[o..o + 2].copy_from_slice(&self.focus_raw.to_le_bytes());
        let sum = fnv1a(&out[..o + 2]).to_le_bytes();
        out[o + 2..o + 6].copy_from_slice(&sum);
        out
    }

    pub fn from_bytes(b: &[u8]) -> Result<FocusChainRec, V2CodecErr> {
        if b.len() != CHAIN_REC_LEN {
            return Err(V2CodecErr::BadLen);
        }
        let mut mg = [0u8; 4];
        mg.copy_from_slice(&b[..4]);
        if mg != VXH1_MAGIC {
            return Err(V2CodecErr::BadMagic);
        }
        if b[4] != VXH1_VER {
            return Err(V2CodecErr::BadVersion);
        }
        if b[5] as usize > CHAIN_CAP {
            return Err(V2CodecErr::BadLen);
        }
        let o = 6 + 2 * CHAIN_CAP;
        let mut sum = [0u8; 4];
        sum.copy_from_slice(&b[o + 2..o + 6]);
        if fnv1a(&b[..o + 2]) != u32::from_le_bytes(sum) {
            return Err(V2CodecErr::BadSum);
        }
        let len = b[5] as usize;
        let mut ids = [0u16; CHAIN_CAP];
        for k in 0..len {
            let mut v = [0u8; 2];
            v.copy_from_slice(&b[6 + k * 2..8 + k * 2]);
            let id = u16::from_le_bytes(v);
            if id == NIL {
                return Err(V2CodecErr::BadField);
            }
            ids[k] = id;
        }
        let mut f = [0u8; 2];
        f.copy_from_slice(&b[o..o + 2]);
        Ok(FocusChainRec { len, ids, focus_raw: u16::from_le_bytes(f) })
    }
}

// ---- UI 壳接线面：焦点环绘制清单 + 环带命中 ----

/// 绘制图元：几何 + 颜色索引（0 = 主题强调色令牌，1 = 控件底色令牌）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct V2Prim {
    pub rect: Rect,
    pub color_idx: u8,
}

/// 焦点环绘制清单（定长 2 图元）：0 = 外扩 2px 的环矩形（令牌色），
/// 1 = 控件本体。换主题变色不换几何（F206 + F151 联动）。
pub fn ring_draw_items(w: &Widget) -> [V2Prim; 2] {
    [
        V2Prim { rect: ring_rect(w), color_idx: 0 },
        V2Prim { rect: w.rect(), color_idx: 1 },
    ]
}

/// 环带命中：点落在环带内（环矩形内、控件矩形外）——壳层用于点击
/// 不透传判定（点在焦点环上不激活控件本体）。
pub fn ring_hit(w: &Widget, x: i32, y: i32) -> bool {
    ring_rect(w).contains(x, y) && !w.rect().contains(x, y)
}

/// F206 v2 自检（首条恒为持久化 round-trip）。
pub fn run_focusnav_v2_checks() -> CheckSet {
    let mut set = CheckSet::new("F206-focusnav-v2");

    // 1. 持久化 round-trip：三控件链 + 焦点 → 编码解码逐位还原
    //    （验主册 F206 焦点归还判据的档案面：重启恢复上次焦点）。
    let mut ch = FocusChain::new();
    for id in 1..=3u16 {
        ch.add(Widget::new(id, 0, id as i32 * 40, 60, 24));
    }
    let _ = ch.focus_id(2, 0);
    let rec = FocusChainRec::of(&ch);
    let bytes = rec.to_bytes();
    let ok_rt = match FocusChainRec::from_bytes(&bytes) {
        Ok(r) => r.len == 3 && r.focus() == Some(2) && r.id_at(0) == Some(1) && r.id_at(2) == Some(3),
        Err(_) => false,
    };
    set.add("v2 persist roundtrip focus chain", ok_rt, "");

    // 2. 损坏拒绝四类 + 字段越界（链内混入 NIL 哨兵）。
    let mut bad1 = bytes;
    bad1[0] = b'X';
    let mut bad2 = bytes;
    bad2[4] = 9;
    let mut bad3 = bytes;
    bad3[6] = 0xFF;
    bad3[7] = 0xFF; // 首个 id 置 NIL 哨兵（重算 sum 使只坏字段）
    let o3 = 6 + 2 * CHAIN_CAP;
    let s3 = fnv1a(&bad3[..o3 + 2]);
    bad3[o3 + 2..o3 + 6].copy_from_slice(&s3.to_le_bytes());
    set.add(
        "v2 persist rejects corrupt chains",
        FocusChainRec::from_bytes(&bad1) == Err(V2CodecErr::BadMagic)
            && FocusChainRec::from_bytes(&bad2) == Err(V2CodecErr::BadVersion)
            && FocusChainRec::from_bytes(&bad3) == Err(V2CodecErr::BadField)
            && FocusChainRec::from_bytes(&bytes[..bytes.len() - 1]) == Err(V2CodecErr::BadLen),
        "",
    );

    // 3. 空链 + 无焦点 round-trip（0xFFFF 哨兵双向还原）。
    let empty = FocusChainRec::of(&FocusChain::new());
    let ok_empty = match FocusChainRec::from_bytes(&empty.to_bytes()) {
        Ok(r) => r.len == 0 && r.focus().is_none(),
        Err(_) => false,
    };
    set.add("v2 empty chain roundtrip no focus", ok_empty, "");

    // 4. 焦点环几何：环矩形外扩 2px；环带命中在环上为真、控件内为假
    //    （验主册 F206「2px 焦点环、外扩 2px」的壳层落点）。
    let w = Widget::new(9, 10, 10, 50, 20);
    let items = ring_draw_items(&w);
    set.add(
        "v2 focus ring draw & hit band",
        items[0].rect == Rect::new(8, 8, 54, 24) && items[0].color_idx == 0
            && items[1].rect == w.rect()
            && ring_hit(&w, 8, 8)
            && !ring_hit(&w, 10, 10),
        "",
    );

    // 5. 容量在册：len 字段超 CHAIN_CAP 按 BadLen 拒（档案不越容量红线）。
    let mut over = bytes;
    over[5] = (CHAIN_CAP + 1) as u8;
    set.add(
        "v2 chain len beyond cap rejected",
        FocusChainRec::from_bytes(&over) == Err(V2CodecErr::BadLen),
        "",
    );

    set
}

#[cfg(test)]
mod tests_v2 {
    use super::*;

    #[test]
    fn chain_rec_codec_exact() {
        let mut ch = FocusChain::new();
        ch.add(Widget::new(7, 0, 0, 30, 20));
        let rec = FocusChainRec::of(&ch);
        assert_eq!(rec.to_bytes().len(), CHAIN_REC_LEN);
        assert_eq!(FocusChainRec::from_bytes(&rec.to_bytes()).unwrap().len, 1);
    }

    #[test]
    fn ring_hit_corners_only() {
        let w = Widget::new(1, 0, 0, 10, 10);
        assert!(ring_hit(&w, -1, 5), "外扩带上");
        assert!(!ring_hit(&w, 20, 20), "环外");
    }

    #[test]
    fn v2_selfcheck_all_green() {
        let set = run_focusnav_v2_checks();
        assert!(set.all_passed(), "F206 v2 自检存在红项");
        assert!(!set.truncated());
        assert!((4..=6).contains(&set.len()));
    }
}
