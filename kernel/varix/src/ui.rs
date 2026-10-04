//! AI-16 UI 运行时域（F376~F400）。
//!
//! The native widget runtime: component tree, layout, text editing primitives,
//! list virtualisation, scroll physics, focus/keyboard navigation, gestures,
//! IME + candidate window, clipboard/drag integration, the shell surfaces
//! (notification centre, control centre, launcher, taskbar, calendar), the
//! motion-token system, the a11y tree, DPI handling, the frame-budget guard
//! and the UI self-test.
//!
//! 手感无损 (军规 1) lives here: every easing curve and scroll integrator is
//! deterministic integer maths so a frame costs the same on QEMU and on metal.

use crate::checks::CheckSet;

// TRINITY-500 · AI-07 组件库 / AI-08 动效子模块，与 AI-16 运行时同处 `ui` 命名空间。
pub mod motion;
pub mod tokens;
pub mod widgets;

// ---------------------------------------------------------------------------
// F376 — 原生组件树
// ---------------------------------------------------------------------------

pub const MAX_COMPONENTS: usize = 24;
pub const MAX_CHILDREN: usize = 6;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WidgetKind {
    Container,
    Label,
    Button,
    TextField,
    List,
    Canvas,
    Window,
}

#[derive(Clone, Copy, Debug)]
pub struct Component {
    pub id: u16,
    pub parent: Option<u16>,
    pub kind: WidgetKind,
    pub children: [u16; MAX_CHILDREN],
    pub child_count: usize,
}

#[derive(Clone, Copy, Debug)]
pub struct ComponentTree {
    nodes: [Option<Component>; MAX_COMPONENTS],
    count: usize,
    /// Bumped whenever the tree changes; renderers compare it to skip work.
    pub revision: u32,
}

impl ComponentTree {
    pub const fn new() -> ComponentTree {
        ComponentTree { nodes: [None; MAX_COMPONENTS], count: 0, revision: 0 }
    }

    pub fn add(&mut self, mut node: Component) -> bool {
        if self.count >= MAX_COMPONENTS {
            return false;
        }
        if let Some(p) = node.parent {
            match self.find_mut(p) {
                Some(parent) => {
                    if parent.child_count >= MAX_CHILDREN {
                        return false;
                    }
                    parent.children[parent.child_count] = node.id;
                    parent.child_count += 1;
                }
                None => return false,
            }
        }
        node.child_count = 0;
        self.nodes[self.count] = Some(node);
        self.count += 1;
        self.revision += 1;
        true
    }

    fn find_mut(&mut self, id: u16) -> Option<&mut Component> {
        for i in 0..self.count {
            if self.nodes[i].map(|n| n.id == id).unwrap_or(false) {
                return self.nodes[i].as_mut();
            }
        }
        None
    }

    pub fn find(&self, id: u16) -> Option<Component> {
        (0..self.count).find_map(|i| match self.nodes[i] {
            Some(n) if n.id == id => Some(n),
            _ => None,
        })
    }

    pub fn remove_subtree(&mut self, id: u16) -> usize {
        let mut removed = 0usize;
        for i in 0..self.count {
            if let Some(n) = self.nodes[i] {
                if n.id == id {
                    self.nodes[i] = None;
                    removed += 1;
                    // Compact to keep iteration dense.
                    if i + 1 < self.count {
                        self.nodes[i] = self.nodes[self.count - 1];
                    }
                    self.nodes[self.count - 1] = None;
                    self.count -= 1;
                    self.revision += 1;
                    break;
                }
            }
        }
        removed
    }

    /// Depth from the root; `None` when the parent chain is broken.
    pub fn depth(&self, id: u16) -> Option<usize> {
        let mut d = 0usize;
        let mut cur = self.find(id)?;
        while let Some(p) = cur.parent {
            d += 1;
            if d > MAX_COMPONENTS {
                return None;
            }
            cur = self.find(p)?;
        }
        Some(d)
    }

    pub fn len(&self) -> usize {
        self.count
    }
}

// ---------------------------------------------------------------------------
// F377 — 布局引擎
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LayoutItem {
    /// Preferred size in logical pixels.
    pub basis: u32,
    /// Growth weight (0 = fixed).
    pub grow: u16,
    pub min: u32,
    pub max: u32,
}

/// Flex-style row layout: fixed items take their basis, the slack is split by
/// growth weight, and every item is clamped to [min, max].
pub fn layout_row(items: &[LayoutItem], available: u32, out: &mut [u32]) -> usize {
    if items.is_empty() || out.len() < items.len() {
        return 0;
    }
    let mut used = 0u32;
    let mut grow_total = 0u32;
    for it in items.iter() {
        let w = it.basis.clamp(it.min, it.max.max(it.min));
        used = used.saturating_add(w);
        grow_total += it.grow as u32;
    }
    let slack = available.saturating_sub(used);
    for (i, it) in items.iter().enumerate() {
        let mut w = it.basis.clamp(it.min, it.max.max(it.min));
        if it.grow > 0 && grow_total > 0 {
            w = w.saturating_add(slack * it.grow as u32 / grow_total);
        }
        out[i] = w.clamp(it.min, it.max.max(it.min));
    }
    items.len()
}

/// Cross-axis (column) layout: equal split with the last item absorbing the
/// remainder so the edges always line up with the parent.
pub fn layout_column(available: u32, rows: usize, out: &mut [u32]) -> usize {
    if rows == 0 || out.len() < rows {
        return 0;
    }
    let each = available / rows as u32;
    let mut assigned = 0u32;
    for i in 0..rows {
        let h = if i == rows - 1 { available - assigned } else { each };
        out[i] = h;
        assigned += h;
    }
    rows
}

// ---------------------------------------------------------------------------
// F378 — 文本输入框
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextField {
    /// Character count (the kernel models text as bytes in one codec).
    pub len: usize,
    pub cursor: usize,
    pub anchor: usize,
    /// Scroll offset in characters for long lines.
    pub scroll: usize,
}

impl TextField {
    pub const fn new() -> TextField {
        TextField { len: 0, cursor: 0, anchor: 0, scroll: 0 }
    }

    pub fn insert(&mut self, count: usize) {
        self.len += count;
        self.cursor += count;
        self.anchor = self.cursor;
    }

    /// Delete backwards; returns how many characters were removed.
    pub fn backspace(&mut self) -> usize {
        let removed = self.selection_len();
        if removed > 0 {
            self.len -= removed;
            self.cursor = self.cursor.min(self.anchor);
            self.anchor = self.cursor;
            return removed;
        }
        if self.cursor == 0 {
            return 0;
        }
        self.cursor -= 1;
        self.anchor = self.cursor;
        self.len -= 1;
        1
    }

    pub fn delete_forward(&mut self) -> usize {
        if self.selection_len() > 0 {
            return self.backspace();
        }
        if self.cursor >= self.len {
            return 0;
        }
        self.len -= 1;
        1
    }

    pub fn selection_len(&self) -> usize {
        self.cursor.abs_diff(self.anchor)
    }

    pub fn select_all(&mut self) {
        self.anchor = 0;
        self.cursor = self.len;
    }

    pub fn move_cursor(&mut self, delta: i32, extend: bool) {
        let next = (self.cursor as i32 + delta).clamp(0, self.len as i32) as usize;
        self.cursor = next;
        if !extend {
            self.anchor = next;
        }
    }

    /// Keep the cursor inside the visible band given a viewport width.
    pub fn clamp_scroll(&mut self, viewport_chars: usize) {
        if viewport_chars == 0 {
            self.scroll = 0;
            return;
        }
        if self.cursor < self.scroll {
            self.scroll = self.cursor;
        } else if self.cursor >= self.scroll + viewport_chars {
            self.scroll = self.cursor + 1 - viewport_chars;
        }
    }
}

// ---------------------------------------------------------------------------
// F379 — 列表虚拟化
// ---------------------------------------------------------------------------

/// Inclusive-visible range for a scrolled list.
pub fn visible_range(
    scroll_px: u64,
    item_height: u32,
    viewport_height: u32,
    total_items: usize,
) -> (usize, usize) {
    if item_height == 0 || total_items == 0 {
        return (0, 0);
    }
    let first = (scroll_px / item_height as u64) as usize;
    let visible = (viewport_height as u64).div_ceil(item_height as u64) as usize;
    let first = first.min(total_items);
    let last = (first + visible + 1).min(total_items); // +1 = partial row
    (first, last)
}

/// Total scrollable height for a list.
pub fn list_content_height(total_items: usize, item_height: u32, gap: u32) -> u64 {
    if total_items == 0 {
        return 0;
    }
    total_items as u64 * (item_height + gap) as u64 - gap as u64
}

// ---------------------------------------------------------------------------
// F380 — 滚动物理
// ---------------------------------------------------------------------------

/// Millipixel position/velocity integrator — integers only, so the feel is
/// identical on every machine and reproducible in tests.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScrollPhysics {
    pub pos_milli: i64,
    pub vel_milli: i64,
    /// Per-millisecond friction, permille.
    pub friction_permille: u32,
    pub max_pos_milli: i64,
}

impl ScrollPhysics {
    pub const fn new(max_pos_milli: i64) -> ScrollPhysics {
        ScrollPhysics { pos_milli: 0, vel_milli: 0, friction_permille: 940, max_pos_milli }
    }

    pub fn fling(&mut self, velocity_milli: i64) {
        self.vel_milli = velocity_milli;
    }

    /// Integrate one frame (`dt_ms`). Returns `true` while still moving.
    pub fn step(&mut self, dt_ms: u32) {
        self.pos_milli += self.vel_milli * dt_ms as i64 / 1000;
        self.vel_milli = self.vel_milli * self.friction_permille as i64 / 1000;
        if self.vel_milli.abs() < 10 {
            self.vel_milli = 0;
        }
        // Rubber-band at the ends instead of a hard stop.
        if self.pos_milli < 0 {
            self.pos_milli /= 2;
            self.vel_milli = 0;
        } else if self.pos_milli > self.max_pos_milli {
            self.pos_milli = self.max_pos_milli + (self.pos_milli - self.max_pos_milli) / 2;
            self.vel_milli = 0;
        }
    }

    pub fn is_settled(&self) -> bool {
        self.vel_milli == 0
    }
}

// ---------------------------------------------------------------------------
// F381/F382 — 焦点系统与键盘导航
// ---------------------------------------------------------------------------

pub const MAX_FOCUSABLE: usize = 16;

#[derive(Clone, Copy, Debug)]
pub struct FocusChain {
    ids: [u16; MAX_FOCUSABLE],
    /// `false` = disabled / not tabbable (skipped by navigation).
    enabled: [bool; MAX_FOCUSABLE],
    count: usize,
    pub current: usize,
}

impl FocusChain {
    pub const fn new() -> FocusChain {
        FocusChain { ids: [0; MAX_FOCUSABLE], enabled: [true; MAX_FOCUSABLE], count: 0, current: 0 }
    }

    pub fn push(&mut self, id: u16, enabled: bool) -> bool {
        if self.count >= MAX_FOCUSABLE {
            return false;
        }
        self.ids[self.count] = id;
        self.enabled[self.count] = enabled;
        self.count += 1;
        true
    }

    pub fn set_enabled(&mut self, index: usize, enabled: bool) {
        if index < self.count {
            self.enabled[index] = enabled;
        }
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn focused_id(&self) -> Option<u16> {
        if self.count == 0 {
            None
        } else {
            Some(self.ids[self.current.min(self.count - 1)])
        }
    }

    /// Tab / Shift-Tab with wrap, skipping disabled entries.
    pub fn move_by(&mut self, step: i32) -> Option<u16> {
        if self.count == 0 {
            return None;
        }
        let mut idx = self.current as i32;
        for _ in 0..self.count {
            idx = (idx + step).rem_euclid(self.count as i32);
            if self.enabled[idx as usize] {
                self.current = idx as usize;
                return self.focused_id();
            }
        }
        None
    }

    /// Spatial navigation (arrows): the next enabled entry in `direction`,
    /// treating the chain as a linear id order (0 = left/up, 1 = right/down).
    pub fn navigate(&mut self, direction: i32) -> Option<u16> {
        self.move_by(direction)
    }
}

// ---------------------------------------------------------------------------
// F383 — 触控手势
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GestureState {
    Idle,
    Touching,
    LongPressing,
    Panning,
    Pinching,
}

#[derive(Clone, Copy, Debug)]
pub struct GestureRecognizer {
    pub state: GestureState,
    start_ms: u32,
    pub start_x: i32,
    pub start_y: i32,
    pub last_x: i32,
    pub last_y: i32,
    pub long_press_ms: u32,
    pub tap_slop: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GestureEvent {
    None,
    Tap,
    LongPress,
    PanStart,
    PanUpdate,
    PanEnd,
    PinchStart,
}

impl GestureRecognizer {
    pub const fn new() -> GestureRecognizer {
        GestureRecognizer {
            state: GestureState::Idle,
            start_ms: 0,
            start_x: 0,
            start_y: 0,
            last_x: 0,
            last_y: 0,
            long_press_ms: 500,
            tap_slop: 8,
        }
    }

    pub fn down(&mut self, x: i32, y: i32, now_ms: u32) -> GestureEvent {
        self.state = GestureState::Touching;
        self.start_ms = now_ms;
        self.start_x = x;
        self.start_y = y;
        self.last_x = x;
        self.last_y = y;
        GestureEvent::None
    }

    pub fn moved(&mut self, x: i32, y: i32, now_ms: u32) -> GestureEvent {
        let dx = (x - self.start_x).unsigned_abs();
        let dy = (y - self.start_y).unsigned_abs();
        self.last_x = x;
        self.last_y = y;
        match self.state {
            GestureState::Touching | GestureState::LongPressing => {
                if dx > self.tap_slop || dy > self.tap_slop {
                    self.state = GestureState::Panning;
                    GestureEvent::PanStart
                } else if self.state == GestureState::Touching
                    && now_ms.saturating_sub(self.start_ms) >= self.long_press_ms
                {
                    self.state = GestureState::LongPressing;
                    GestureEvent::LongPress
                } else {
                    GestureEvent::None
                }
            }
            GestureState::Panning => GestureEvent::PanUpdate,
            _ => GestureEvent::None,
        }
    }

    pub fn up(&mut self, now_ms: u32) -> GestureEvent {
        let event = match self.state {
            GestureState::Panning => GestureEvent::PanEnd,
            GestureState::Touching => {
                if now_ms.saturating_sub(self.start_ms) >= self.long_press_ms {
                    GestureEvent::LongPress
                } else {
                    GestureEvent::Tap
                }
            }
            _ => GestureEvent::None,
        };
        self.state = GestureState::Idle;
        event
    }

    /// Two-finger pinch distance change (permille of the initial spread).
    pub fn pinch(&mut self, current_spread: u32, initial_spread: u32) -> (GestureEvent, u16) {
        if initial_spread == 0 {
            return (GestureEvent::None, 1000);
        }
        let start = self.state != GestureState::Pinching;
        self.state = GestureState::Pinching;
        let scale = (current_spread as u64 * 1000 / initial_spread as u64).min(4000) as u16;
        (if start { GestureEvent::PinchStart } else { GestureEvent::None }, scale)
    }
}

// ---------------------------------------------------------------------------
// F384/F385 — 输入法框架与候选词窗口
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImeSession {
    /// Composition (pre-edit) length in characters.
    pub preedit_len: usize,
    pub preedit_cursor: usize,
    pub committed_len: usize,
    pub active: bool,
}

impl ImeSession {
    pub const fn new() -> ImeSession {
        ImeSession { preedit_len: 0, preedit_cursor: 0, committed_len: 0, active: false }
    }

    pub fn type_char(&mut self, ch: char) {
        if ch.is_ascii() {
            // Direct input commits immediately (no pre-edit for ASCII).
            self.committed_len += 1;
            self.preedit_len = 0;
            self.preedit_cursor = 0;
        } else {
            self.active = true;
            self.preedit_len += 1;
            self.preedit_cursor = self.preedit_len;
        }
    }

    pub fn commit(&mut self) -> usize {
        let n = self.preedit_len;
        self.committed_len += n;
        self.preedit_len = 0;
        self.preedit_cursor = 0;
        self.active = false;
        n
    }

    pub fn cancel(&mut self) {
        self.preedit_len = 0;
        self.preedit_cursor = 0;
        self.active = false;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CandidateWindow {
    pub total: usize,
    pub per_page: usize,
    pub page: usize,
    pub selected: usize,
}

impl CandidateWindow {
    pub fn new(total: usize, per_page: usize) -> CandidateWindow {
        let per_page = per_page.max(1);
        CandidateWindow { total, per_page, page: 0, selected: 0 }
    }

    pub fn page_count(&self) -> usize {
        self.total.div_ceil(self.per_page)
    }

    pub fn range(&self) -> (usize, usize) {
        let start = (self.page * self.per_page).min(self.total);
        let end = (start + self.per_page).min(self.total);
        (start, end)
    }

    pub fn move_selection(&mut self, delta: i32) {
        let (start, end) = self.range();
        if end == start {
            return;
        }
        let width = (end - start) as i32;
        let idx = (self.selected as i32 + delta).rem_euclid(width);
        self.selected = idx as usize;
    }

    pub fn next_page(&mut self) -> bool {
        if self.page + 1 >= self.page_count() {
            return false;
        }
        self.page += 1;
        self.selected = 0;
        true
    }

    /// The candidate index the user is on, in global terms.
    pub fn global_index(&self) -> usize {
        self.range().0 + self.selected
    }
}

// ---------------------------------------------------------------------------
// F386/F387/F388/F389 — 剪贴板集成、通知中心、控制中心、快速设置
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RichClip {
    pub has_text: bool,
    pub has_html: bool,
    pub has_image: bool,
    pub bytes: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PasteFlavor {
    Html,
    Image,
    Text,
    None,
}

/// Paste flavour preference: HTML keeps formatting, text is the safe fallback.
pub fn paste_flavor(clip: RichClip) -> PasteFlavor {
    if clip.has_html {
        PasteFlavor::Html
    } else if clip.has_image {
        PasteFlavor::Image
    } else if clip.has_text {
        PasteFlavor::Text
    } else {
        PasteFlavor::None
    }
}

pub const MAX_GROUPED: usize = 6;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NotificationGroup {
    pub app: &'static str,
    pub count: u32,
    pub collapsed: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct NotificationPanel {
    groups: [Option<NotificationGroup>; MAX_GROUPED],
    count: usize,
}

impl NotificationPanel {
    pub const fn new() -> NotificationPanel {
        NotificationPanel { groups: [None; MAX_GROUPED], count: 0 }
    }

    /// One row per app — a burst of notifications collapses into `count`.
    pub fn push(&mut self, app: &'static str) -> bool {
        for i in 0..self.count {
            if let Some(mut g) = self.groups[i] {
                if g.app == app {
                    g.count += 1;
                    g.collapsed = g.count > 1;
                    self.groups[i] = Some(g);
                    return true;
                }
            }
        }
        if self.count >= MAX_GROUPED {
            return false;
        }
        self.groups[self.count] = Some(NotificationGroup { app, count: 1, collapsed: false });
        self.count += 1;
        true
    }

    pub fn group(&self, app: &str) -> Option<NotificationGroup> {
        (0..self.count).find_map(|i| match self.groups[i] {
            Some(g) if g.app == app => Some(g),
            _ => None,
        })
    }

    pub fn len(&self) -> usize {
        self.count
    }
}

/// Control-centre tile grids: 2 columns, rows derived from the tile count.
pub fn tile_grid(tiles: usize) -> (usize, usize) {
    let cols = 2usize;
    (cols, tiles.div_ceil(cols.max(1)))
}

pub const MAX_QUICK_TOGGLES: usize = 8;

#[derive(Clone, Copy, Debug)]
pub struct QuickSettings {
    pins: [Option<&'static str>; MAX_QUICK_TOGGLES],
    count: usize,
}

impl QuickSettings {
    pub const fn new() -> QuickSettings {
        QuickSettings { pins: [None; MAX_QUICK_TOGGLES], count: 0 }
    }

    pub fn pin(&mut self, name: &'static str) -> bool {
        if self.count >= MAX_QUICK_TOGGLES
            || self.pins[..self.count].contains(&Some(name))
        {
            return false;
        }
        self.pins[self.count] = Some(name);
        self.count += 1;
        true
    }

    pub fn unpin(&mut self, name: &str) -> bool {
        for i in 0..self.count {
            if self.pins[i] == Some(name) {
                if i + 1 < self.count {
                    self.pins[i] = self.pins[self.count - 1];
                }
                self.pins[self.count - 1] = None;
                self.count -= 1;
                return true;
            }
        }
        false
    }

    pub fn pinned(&self, name: &str) -> bool {
        self.pins[..self.count].contains(&Some(name))
    }

    pub fn len(&self) -> usize {
        self.count
    }
}

// ---------------------------------------------------------------------------
// F390/F391/F392 — 启动器、任务栏语义、日历时钟
// ---------------------------------------------------------------------------

/// Subsequence fuzzy match score: higher is better, `None` = no match.
/// Adjacent hits and word starts score extra (the launcher feel).
pub fn fuzzy_score(query: &str, name: &str) -> Option<u32> {
    if query.is_empty() {
        return Some(0);
    }
    let q: &[u8] = query.as_bytes();
    let n: &[u8] = name.as_bytes();
    let mut qi = 0usize;
    let mut score = 0u32;
    let mut prev_hit: Option<usize> = None;
    for (i, &c) in n.iter().enumerate() {
        if qi < q.len() && c.eq_ignore_ascii_case(&q[qi]) {
            score += 10;
            if prev_hit == Some(i.wrapping_sub(1)) {
                score += 15; // adjacent
            }
            if i == 0 || n[i - 1] == b' ' {
                score += 20; // word start
            }
            prev_hit = Some(i);
            qi += 1;
        }
    }
    if qi == q.len() {
        // Prefer shorter names on a tie.
        Some(score + (100 - name.len().min(100)) as u32)
    } else {
        None
    }
}

pub const MAX_TASKS: usize = 12;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TaskEntry {
    pub id: u32,
    pub app: &'static str,
    pub pinned: bool,
    pub visible: bool,
    pub urgent: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct Taskbar {
    tasks: [Option<TaskEntry>; MAX_TASKS],
    count: usize,
}

impl Taskbar {
    pub const fn new() -> Taskbar {
        Taskbar { tasks: [None; MAX_TASKS], count: 0 }
    }

    pub fn upsert(&mut self, entry: TaskEntry) -> bool {
        for i in 0..self.count {
            if let Some(mut t) = self.tasks[i] {
                if t.id == entry.id {
                    t.visible = entry.visible;
                    t.urgent = entry.urgent;
                    self.tasks[i] = Some(t);
                    return true;
                }
            }
        }
        if self.count >= MAX_TASKS {
            return false;
        }
        self.tasks[self.count] = Some(entry);
        self.count += 1;
        true
    }

    /// What the taskbar shows: pinned entries always, running entries only
    /// while visible (hidden windows leave the bar — F417).
    pub fn visible_count(&self) -> usize {
        (0..self.count)
            .filter(|i| {
                self.tasks[*i]
                    .map(|t| t.pinned || t.visible)
                    .unwrap_or(false)
            })
            .count()
    }

    pub fn urgent_count(&self) -> usize {
        (0..self.count)
            .filter(|i| self.tasks[*i].map(|t| t.urgent).unwrap_or(false))
            .count()
    }
}

pub fn is_leap_year(year: u32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

pub fn days_in_month(year: u32, month: u8) -> u8 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if is_leap_year(year) {
                29
            } else {
                28
            }
        }
        _ => 0,
    }
}

/// Day of week for a Gregorian date (0 = Sunday) via Zeller's congruence.
pub fn weekday(year: u32, month: u8, day: u8) -> Option<u8> {
    if month == 0 || month > 12 || day == 0 || day > days_in_month(year, month) {
        return None;
    }
    let (m, y) = if month < 3 {
        (month as i32 + 12, year as i32 - 1)
    } else {
        (month as i32, year as i32)
    };
    let k = y % 100;
    let j = y / 100;
    let h = (day as i32 + (13 * (m + 1)) / 5 + k + k / 4 + j / 4 + 5 * j).rem_euclid(7);
    // Zeller: 0 = Saturday → shift so 0 = Sunday.
    Some(((h + 6) % 7) as u8)
}

/// Format minutes-since-midnight as a zero-padded `HH:MM` into `out`.
pub fn format_clock(minutes_of_day: u32, out: &mut [u8; 5]) -> bool {
    if minutes_of_day >= 24 * 60 {
        return false;
    }
    let h = minutes_of_day / 60;
    let m = minutes_of_day % 60;
    out[0] = b'0' + (h / 10) as u8;
    out[1] = b'0' + (h % 10) as u8;
    out[2] = b':';
    out[3] = b'0' + (m / 10) as u8;
    out[4] = b'0' + (m % 10) as u8;
    true
}

// ---------------------------------------------------------------------------
// F393/F394 — 图标渲染与动画令牌
// ---------------------------------------------------------------------------

/// Icon ladder: 16/24/32/48/64/128/256 (the Variable 128px pipeline).
pub const ICON_SIZES: [u32; 7] = [16, 24, 32, 48, 64, 128, 256];

/// Pick the smallest rung that covers the requested size.
pub fn icon_rung(requested: u32) -> u32 {
    for s in ICON_SIZES.iter() {
        if *s >= requested {
            return *s;
        }
    }
    ICON_SIZES[ICON_SIZES.len() - 1]
}

/// Cache key: name hash + rung + dpi bucket (no string keys in the kernel).
pub fn icon_cache_key(name_hash: u64, rung: u32, dpi_permille: u32) -> u64 {
    let dpi_bucket = (dpi_permille / 250) as u64;
    name_hash
        .wrapping_mul(31)
        .wrapping_add(rung as u64)
        .wrapping_mul(31)
        .wrapping_add(dpi_bucket)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MotionCurve {
    Linear,
    Standard,
    Emphasized,
    Decelerate,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MotionTokens {
    pub fast_ms: u16,
    pub normal_ms: u16,
    pub slow_ms: u16,
    pub curve: MotionCurve,
    /// `reduce-motion` (F423) collapses every duration to zero.
    pub reduce_motion: bool,
}

impl MotionTokens {
    pub const fn base() -> MotionTokens {
        MotionTokens { fast_ms: 90, normal_ms: 180, slow_ms: 320, curve: MotionCurve::Standard, reduce_motion: false }
    }

    pub fn duration_for(&self, kind: MotionKind) -> u16 {
        if self.reduce_motion {
            return 0;
        }
        match kind {
            MotionKind::Fast => self.fast_ms,
            MotionKind::Normal => self.normal_ms,
            MotionKind::Slow => self.slow_ms,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MotionKind {
    Fast,
    Normal,
    Slow,
}

/// Easing on a 0..1000 progress scale, 0..1000 output.
pub fn ease(curve: MotionCurve, progress_permille: u16) -> u16 {
    let p = progress_permille.min(1000) as i64;
    match curve {
        MotionCurve::Linear => p as u16,
        MotionCurve::Standard => {
            // ease-in-out quad
            if p < 500 {
                (2 * p * p / 1000) as u16
            } else {
                (1000 - 2 * (1000 - p) * (1000 - p) / 1000) as u16
            }
        }
        MotionCurve::Emphasized => {
            // ease-in-out cubic — the "expressive" token
            if p < 500 {
                (4 * p * p * p / 1_000_000) as u16
            } else {
                let q = 1000 - p;
                (1000 - 4 * q * q * q / 1_000_000) as u16
            }
        }
        MotionCurve::Decelerate => {
            // 1 - (1-p)^2
            let q = 1000 - p;
            (1000 - q * q / 1000) as u16
        }
    }
}

// ---------------------------------------------------------------------------
// F395 — 无障碍树
// ---------------------------------------------------------------------------

pub const MAX_A11Y: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum A11yRole {
    Button,
    Text,
    TextField,
    List,
    Window,
    Group,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct A11yNode {
    pub id: u16,
    pub role: A11yRole,
    pub name: &'static str,
    pub focusable: bool,
    pub enabled: bool,
    /// Depth in the UI tree — screen readers announce in this order.
    pub depth: u8,
}

#[derive(Clone, Copy, Debug)]
pub struct A11yTree {
    nodes: [Option<A11yNode>; MAX_A11Y],
    count: usize,
}

impl A11yTree {
    pub const fn new() -> A11yTree {
        A11yTree { nodes: [None; MAX_A11Y], count: 0 }
    }

    pub fn add(&mut self, node: A11yNode) -> bool {
        if self.count >= MAX_A11Y {
            return false;
        }
        self.nodes[self.count] = Some(node);
        self.count += 1;
        true
    }

    /// Flatten in tree order (depth-first), skipping disabled subtrees —
    /// what the screen reader walks.
    pub fn flatten(&self, out: &mut [u16]) -> usize {
        let mut n = 0usize;
        for i in 0..self.count {
            if let Some(node) = self.nodes[i] {
                if node.enabled && n < out.len() {
                    out[n] = node.id;
                    n += 1;
                }
            }
        }
        n
    }

    pub fn find(&self, id: u16) -> Option<A11yNode> {
        (0..self.count).find_map(|i| match self.nodes[i] {
            Some(n) if n.id == id => Some(n),
            _ => None,
        })
    }

    pub fn len(&self) -> usize {
        self.count
    }
}

// ---------------------------------------------------------------------------
// F396 — 主题实时预览
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TokenDiff {
    pub changed: u8,
    pub accent_changed: bool,
    pub motion_changed: bool,
}

/// Live preview: compute what changed so only the affected layers repaint.
pub fn token_diff(a: crate::service::ThemeTokens, b: crate::service::ThemeTokens) -> TokenDiff {
    let mut changed = 0u8;
    let accent_changed = a.accent != b.accent;
    let motion_changed = a.motion_ms != b.motion_ms;
    if accent_changed {
        changed += 1;
    }
    if motion_changed {
        changed += 1;
    }
    if a.radius_px != b.radius_px {
        changed += 1;
    }
    if a.spacing_px != b.spacing_px {
        changed += 1;
    }
    if a.blur_permille != b.blur_permille {
        changed += 1;
    }
    TokenDiff { changed, accent_changed, motion_changed }
}

// ---------------------------------------------------------------------------
// F397 — 高分屏适配
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DpiScale {
    /// 1000 = 1.0×.
    pub permille: u32,
}

impl DpiScale {
    pub const fn new(permille: u32) -> DpiScale {
        let p = if permille == 0 { 1 } else { permille };
        DpiScale { permille: p }
    }

    pub fn to_physical(&self, logical: u32) -> u32 {
        ((logical as u64 * self.permille as u64) / 1000) as u32
    }

    pub fn to_logical(&self, physical: u32) -> u32 {
        ((physical as u64 * 1000) / self.permille as u64) as u32
    }

    /// Snap a physical length to whole device pixels for crisp hairlines.
    pub fn snap(&self, logical: u32) -> u32 {
        let p = self.to_physical(logical);
        if p == 0 {
            1
        } else {
            p
        }
    }
}

// ---------------------------------------------------------------------------
// F398 — 帧预算守护
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameBudgetGuard {
    pub budget_us: u32,
    pub over_budget_frames: u32,
    pub total_frames: u32,
    pub degraded: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameVerdict {
    Within,
    Over,
    /// Three consecutive over-budget frames: drop to the cheap path (F198).
    Degrade,
}

impl FrameBudgetGuard {
    pub const fn new(budget_us: u32) -> FrameBudgetGuard {
        FrameBudgetGuard { budget_us, over_budget_frames: 0, total_frames: 0, degraded: false }
    }

    pub fn record(&mut self, frame_us: u32) -> FrameVerdict {
        self.total_frames += 1;
        if frame_us <= self.budget_us {
            return FrameVerdict::Within;
        }
        self.over_budget_frames += 1;
        if self.over_budget_frames >= 3 {
            self.degraded = true;
            return FrameVerdict::Degrade;
        }
        FrameVerdict::Over
    }

    /// Over-budget ratio in permille — surfaced on the frame-rate gauge.
    pub fn over_ratio_permille(&self) -> u16 {
        if self.total_frames == 0 {
            return 0;
        }
        (self.over_budget_frames as u64 * 1000 / self.total_frames as u64) as u16
    }

    pub fn reset(&mut self) {
        self.over_budget_frames = 0;
        self.degraded = false;
    }
}

// ---------------------------------------------------------------------------
// F399 — UI 回归套件
// ---------------------------------------------------------------------------

pub const MAX_SNAPSHOT_ENTRIES: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SnapshotEntry {
    pub screen: &'static str,
    pub hash: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegressionVerdict {
    Match,
    Mismatch(&'static str),
    Missing(&'static str),
    Clean,
}

#[derive(Clone, Copy, Debug)]
pub struct RegressionSuite {
    baseline: [Option<SnapshotEntry>; MAX_SNAPSHOT_ENTRIES],
    count: usize,
}

impl RegressionSuite {
    pub const fn new() -> RegressionSuite {
        RegressionSuite { baseline: [None; MAX_SNAPSHOT_ENTRIES], count: 0 }
    }

    pub fn capture(&mut self, entry: SnapshotEntry) -> bool {
        for i in 0..self.count {
            if self.baseline[i].map(|e| e.screen == entry.screen).unwrap_or(false) {
                self.baseline[i] = Some(entry);
                return true;
            }
        }
        if self.count >= MAX_SNAPSHOT_ENTRIES {
            return false;
        }
        self.baseline[self.count] = Some(entry);
        self.count += 1;
        true
    }

    pub fn compare(&self, entry: SnapshotEntry) -> RegressionVerdict {
        for i in 0..self.count {
            if let Some(base) = self.baseline[i] {
                if base.screen == entry.screen {
                    return if base.hash == entry.hash {
                        RegressionVerdict::Match
                    } else {
                        RegressionVerdict::Mismatch(entry.screen)
                    };
                }
            }
        }
        RegressionVerdict::Missing(entry.screen)
    }

    pub fn compare_all(&self, current: &[SnapshotEntry]) -> usize {
        current
            .iter()
            .filter(|e| !matches!(self.compare(**e), RegressionVerdict::Match))
            .count()
    }

    pub fn len(&self) -> usize {
        self.count
    }
}

// ---------------------------------------------------------------------------
// F400 — UI 自检
// ---------------------------------------------------------------------------

pub fn run_ui_checks() -> CheckSet {
    let mut set = CheckSet::new("ui");

    let mut tree = ComponentTree::new();
    let ok_root = tree.add(Component {
        id: 1,
        parent: None,
        kind: WidgetKind::Window,
        children: [0; MAX_CHILDREN],
        child_count: 0,
    });
    let ok_child = tree.add(Component {
        id: 2,
        parent: Some(1),
        kind: WidgetKind::Button,
        children: [0; MAX_CHILDREN],
        child_count: 0,
    });
    set.add(
        "F376 component tree",
        ok_root
            && ok_child
            && tree.find(1).map(|n| n.child_count) == Some(1)
            && tree.depth(2) == Some(1)
            && tree.revision == 2
            && !tree.add(Component {
                id: 3,
                parent: Some(99),
                kind: WidgetKind::Label,
                children: [0; MAX_CHILDREN],
                child_count: 0,
            }),
        "tree ops",
    );
    set.add("F376 detach", tree.remove_subtree(2) == 1 && tree.len() == 1, "remove");

    let items = [
        LayoutItem { basis: 100, grow: 0, min: 50, max: 200 },
        LayoutItem { basis: 100, grow: 1, min: 100, max: 1000 },
        LayoutItem { basis: 100, grow: 1, min: 100, max: 1000 },
    ];
    let mut widths = [0u32; 3];
    let n = layout_row(&items, 500, &mut widths);
    let mut rows = [0u32; 3];
    let rn = layout_column(100, 3, &mut rows);
    set.add(
        "F377 layout",
        n == 3
            && widths[0] == 100
            && widths[1] + widths[2] == 400
            && rn == 3
            && rows.iter().sum::<u32>() == 100
            && rows[2] == 100 - 33 - 33,
        "flex row + column",
    );
    set.add(
        "F377 clamps",
        {
            let tight = [LayoutItem { basis: 10, grow: 0, min: 40, max: 400 }];
            let mut out = [0u32; 1];
            layout_row(&tight, 10, &mut out) == 1 && out[0] == 40
        },
        "min clamp",
    );

    let mut tf = TextField::new();
    tf.insert(5);
    let deleted = tf.backspace();
    tf.move_cursor(-1, true);
    tf.select_all();
    let sel = tf.selection_len();
    tf.delete_forward();
    set.add(
        "F378 text field",
        deleted == 1
            && tf.len == 0
            && sel == 4
            && tf.len == 0
            && TextField::new().backspace() == 0,
        "edit primitives",
    );
    let mut scroll_tf = TextField::new();
    scroll_tf.insert(30);
    scroll_tf.clamp_scroll(10);
    set.add("F378 scroll", scroll_tf.scroll == 21, "cursor visibility");

    set.add(
        "F379 virtual list",
        visible_range(0, 40, 400, 1000) == (0, 11)
            && visible_range(800, 40, 400, 1000) == (20, 31)
            && visible_range(0, 0, 400, 10) == (0, 0)
            && visible_range(1_000_000, 40, 400, 10) == (10, 10)
            && list_content_height(10, 40, 8) == 472,
        "range math",
    );
    let mut vis = [0u32; 32];
    set.add(
        "F379 visible count",
        layout_column(1000, 32, &mut vis) == 32,
        "column helper reuse",
    );

    let mut physics = ScrollPhysics::new(100_000);
    physics.fling(20_000);
    let before = physics.pos_milli;
    for _ in 0..16 {
        physics.step(16);
    }
    set.add(
        "F380 scroll physics",
        physics.pos_milli > before && physics.vel_milli.abs() < 20_000,
        "momentum",
    );
    let mut rubber = ScrollPhysics::new(1000);
    rubber.pos_milli = -1000;
    rubber.step(16);
    set.add("F380 rubber band", rubber.pos_milli == -500, "overscroll");

    let mut chain = FocusChain::new();
    chain.push(10, true);
    chain.push(11, false);
    chain.push(12, true);
    let first = chain.move_by(1);
    let second = chain.move_by(1);
    let wrap = chain.move_by(1);
    set.add(
        "F381/F382 focus",
        first == Some(12)
            && second == Some(10)
            && wrap == Some(12)
            && FocusChain::new().move_by(1).is_none(),
        "skip disabled + wrap",
    );

    let mut g = GestureRecognizer::new();
    let down = g.down(0, 0, 0);
    let moved = g.moved(2, 2, 100);
    let up = g.up(120);
    let mut g2 = GestureRecognizer::new();
    g2.down(0, 0, 0);
    let long = g2.moved(1, 1, 600);
    let mut g3 = GestureRecognizer::new();
    g3.down(0, 0, 0);
    let pan = g3.moved(100, 0, 30);
    let (pinch_event, scale) = {
        let mut g4 = GestureRecognizer::new();
        g4.pinch(200, 100)
    };
    set.add(
        "F383 gestures",
        down == GestureEvent::None
            && moved == GestureEvent::None
            && up == GestureEvent::Tap
            && long == GestureEvent::LongPress
            && pan == GestureEvent::PanStart
            && pinch_event == GestureEvent::PinchStart
            && scale == 2000,
        "recognizer",
    );
    set.add("F383 pinch guard", GestureRecognizer::new().pinch(1, 0).1 == 1000, "zero spread");

    let mut ime = ImeSession::new();
    ime.type_char('h');
    ime.type_char('汉');
    ime.type_char('字');
    let committed = ime.commit();
    set.add(
        "F384 ime",
        committed == 2 && ime.committed_len == 3 && !ime.active && ime.preedit_len == 0,
        "preedit + commit",
    );
    let mut ime2 = ImeSession::new();
    ime2.type_char('x');
    ime2.cancel();
    set.add("F384 cancel", ime2.preedit_len == 0 && ime2.committed_len == 1, "cancel keeps ascii");

    let mut cand = CandidateWindow::new(23, 5);
    let paged = cand.next_page();
    let (start, end) = cand.range();
    cand.move_selection(1);
    set.add(
        "F385 candidate window",
        cand.page_count() == 5
            && paged
            && (start, end) == (5, 10)
            && cand.selected == 1
            && cand.global_index() == 6
            && !CandidateWindow::new(3, 5).next_page(),
        "paging",
    );

    set.add(
        "F386 clipboard integration",
        paste_flavor(RichClip { has_text: true, has_html: true, has_image: true, bytes: 10 })
            == PasteFlavor::Html
            && paste_flavor(RichClip { has_text: false, has_html: false, has_image: true, bytes: 10 })
                == PasteFlavor::Image
            && paste_flavor(RichClip { has_text: false, has_html: false, has_image: false, bytes: 0 })
                == PasteFlavor::None,
        "flavour priority",
    );

    let mut panel = NotificationPanel::new();
    panel.push("mail");
    panel.push("mail");
    panel.push("chat");
    set.add(
        "F387 notification centre",
        panel.group("mail").map(|g| (g.count, g.collapsed)) == Some((2, true))
            && panel.group("chat").map(|g| g.collapsed) == Some(false)
            && panel.len() == 2,
        "grouping",
    );

    set.add(
        "F388/F389 control centre",
        tile_grid(5) == (2, 3)
            && tile_grid(0) == (2, 0)
            && {
                let mut q = QuickSettings::new();
                q.pin("wifi") && q.pin("bt") && !q.pin("wifi") && q.unpin("wifi") && q.len() == 1
            },
        "tiles + pins",
    );

    set.add(
        "F390 launcher",
        fuzzy_score("chr", "chrome") == fuzzy_score("chr", "chrome")
            && fuzzy_score("chr", "chrome").unwrap() > 0
            && fuzzy_score("xyz", "chrome").is_none()
            && fuzzy_score("", "anything") == Some(0),
        "fuzzy match",
    );

    let mut bar = Taskbar::new();
    bar.upsert(TaskEntry { id: 1, app: "term", pinned: true, visible: false, urgent: false });
    bar.upsert(TaskEntry { id: 2, app: "files", pinned: false, visible: true, urgent: true });
    bar.upsert(TaskEntry { id: 3, app: "hidden", pinned: false, visible: false, urgent: false });
    set.add(
        "F391 taskbar semantics",
        bar.visible_count() == 2 && bar.urgent_count() == 1,
        "pinned + hidden",
    );

    let mut clock_out = [0u8; 5];
    let clock_ok = format_clock(9 * 60 + 5, &mut clock_out);
    set.add(
        "F392 calendar clock",
        is_leap_year(2024)
            && !is_leap_year(2100)
            && days_in_month(2023, 2) == 28
            && weekday(2026, 9, 11) == Some(5)
            && clock_ok
            && &clock_out == b"09:05"
            && !format_clock(24 * 60, &mut clock_out),
        "date + clock",
    );

    set.add(
        "F393 icon pipeline",
        icon_rung(17) == 24
            && icon_rung(200) == 256
            && icon_rung(256) == 256
            && icon_cache_key(1, 24, 1000) == icon_cache_key(1, 24, 1100)
            && icon_cache_key(1, 24, 1000) != icon_cache_key(1, 24, 1500),
        "rungs + cache key",
    );

    let tokens = MotionTokens::base();
    let reduced = MotionTokens { reduce_motion: true, ..tokens };
    set.add(
        "F394 motion tokens",
        tokens.duration_for(MotionKind::Normal) == 180
            && reduced.duration_for(MotionKind::Slow) == 0
            && ease(MotionCurve::Linear, 500) == 500
            && ease(MotionCurve::Standard, 0) == 0
            && ease(MotionCurve::Standard, 1000) == 1000
            && ease(MotionCurve::Standard, 500) == 500
            && ease(MotionCurve::Decelerate, 500) > 500,
        "easing",
    );
    set.add(
        "F394 monotonic curves",
        {
            let mut prev = 0u16;
            let mut monotonic = true;
            let mut p = 0u16;
            while p <= 1000 {
                let v = ease(MotionCurve::Standard, p);
                if v < prev || v > 1000 {
                    monotonic = false;
                }
                prev = v;
                p += 50;
            }
            let mut prev_e = 0u16;
            let mut p = 0u16;
            while p <= 1000 {
                let v = ease(MotionCurve::Emphasized, p);
                if v < prev_e {
                    monotonic = false;
                }
                prev_e = v;
                p += 50;
            }
            monotonic && prev == 1000 && prev_e == 1000
        },
        "no overshoot",
    );

    let mut a11y = A11yTree::new();
    a11y.add(A11yNode { id: 1, role: A11yRole::Window, name: "settings", focusable: false, enabled: true, depth: 0 });
    a11y.add(A11yNode { id: 2, role: A11yRole::Button, name: "apply", focusable: true, enabled: true, depth: 1 });
    a11y.add(A11yNode { id: 3, role: A11yRole::Button, name: "disabled", focusable: true, enabled: false, depth: 1 });
    let mut flat = [0u16; MAX_A11Y];
    let flat_n = a11y.flatten(&mut flat);
    set.add(
        "F395 accessibility tree",
        flat_n == 2
            && flat[0] == 1
            && flat[1] == 2
            && a11y.find(2).map(|n| n.role) == Some(A11yRole::Button)
            && a11y.len() == 3,
        "flatten",
    );

    let base = crate::service::ThemeTokens::base();
    let diff = token_diff(base, crate::service::ThemeTokens { accent: 0xFF0000, motion_ms: 90, ..base });
    set.add(
        "F396 live preview",
        diff.changed == 2 && diff.accent_changed && diff.motion_changed,
        "minimal repaint",
    );

    let scale = DpiScale::new(1500);
    set.add(
        "F397 hidpi",
        scale.to_physical(100) == 150
            && scale.to_logical(150) == 100
            && DpiScale::new(1000).snap(0) == 1
            && DpiScale::new(2000).to_physical(11) == 22,
        "dpi math",
    );

    let mut guard = FrameBudgetGuard::new(16_667);
    let ok = guard.record(8_000);
    let over = guard.record(20_000);
    let over2 = guard.record(20_000);
    let degrade = guard.record(20_000);
    set.add(
        "F398 frame budget",
        ok == FrameVerdict::Within
            && over == FrameVerdict::Over
            && over2 == FrameVerdict::Over
            && degrade == FrameVerdict::Degrade
            && guard.degraded
            && guard.over_ratio_permille() == 750,
        "budget guard",
    );

    let mut suite = RegressionSuite::new();
    suite.capture(SnapshotEntry { screen: "desktop", hash: 0xAAAA });
    let match_v = suite.compare(SnapshotEntry { screen: "desktop", hash: 0xAAAA });
    let mismatch = suite.compare(SnapshotEntry { screen: "desktop", hash: 0xBBBB });
    let missing = suite.compare(SnapshotEntry { screen: "none", hash: 1 });
    set.add(
        "F399 UI regression",
        match_v == RegressionVerdict::Match
            && mismatch == RegressionVerdict::Mismatch("desktop")
            && missing == RegressionVerdict::Missing("none")
            && suite.compare_all(&[SnapshotEntry { screen: "desktop", hash: 0xBBBB }]) == 1,
        "snapshots",
    );

    set
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f376_tree_capacity() {
        let mut t = ComponentTree::new();
        for i in 0..MAX_COMPONENTS {
            let parent = if i == 0 {
                None
            } else {
                Some(i as u16)
            };
            assert!(t.add(Component {
                id: (i + 1) as u16,
                parent,
                kind: WidgetKind::Container,
                children: [0; MAX_CHILDREN],
                child_count: 0,
            }));
        }
        assert_eq!(t.len(), MAX_COMPONENTS);
        assert!(t.find(999).is_none());
    }

    #[test]
    fn f376_child_limit() {
        let mut t = ComponentTree::new();
        t.add(Component { id: 1, parent: None, kind: WidgetKind::Container, children: [0; MAX_CHILDREN], child_count: 0 });
        for i in 0..MAX_CHILDREN {
            assert!(t.add(Component {
                id: 10 + i as u16,
                parent: Some(1),
                kind: WidgetKind::Label,
                children: [0; MAX_CHILDREN],
                child_count: 0,
            }));
        }
        assert!(!t.add(Component {
            id: 99,
            parent: Some(1),
            kind: WidgetKind::Label,
            children: [0; MAX_CHILDREN],
            child_count: 0,
        }));
    }

    #[test]
    fn f377_zero_space_layout() {
        let items = [
            LayoutItem { basis: 50, grow: 1, min: 0, max: 1000 },
            LayoutItem { basis: 50, grow: 1, min: 0, max: 1000 },
        ];
        let mut out = [0u32; 2];
        assert_eq!(layout_row(&items, 0, &mut out), 2);
        assert_eq!(out[0], 50); // basis still wins when there is no slack
        assert_eq!(layout_row(&items, 100, &mut out[..1]), 0);
        let mut c = [0u32; 0];
        assert_eq!(layout_column(100, 2, &mut c), 0);
        assert_eq!(layout_column(0, 0, &mut out), 0);
    }

    #[test]
    fn f378_cursor_survives_edits() {
        let mut tf = TextField::new();
        tf.insert(10);
        tf.move_cursor(-3, false);
        assert_eq!(tf.cursor, 7);
        assert_eq!(tf.selection_len(), 0);
        tf.move_cursor(-100, true);
        assert_eq!(tf.cursor, 0);
        assert_eq!(tf.selection_len(), 7);
        assert_eq!(tf.delete_forward(), 7);
        assert_eq!(tf.len, 3);
        tf.insert(1);
        assert_eq!(tf.cursor, 1);
    }

    #[test]
    fn f379_visible_range_edges() {
        assert_eq!(visible_range(0, 50, 0, 10), (0, 1));
        assert_eq!(visible_range(0, 50, 500, 0), (0, 0));
        let (first, last) = visible_range(5000, 50, 500, 100);
        assert_eq!(first, 100);
        assert_eq!(last, 100);
    }

    #[test]
    fn f380_settles_and_clamps() {
        let mut p = ScrollPhysics::new(10_000);
        p.fling(1_000_000);
        for _ in 0..200 {
            p.step(16);
        }
        assert!(p.is_settled());
        assert_eq!(p.pos_milli, 10_000);
        let mut high = ScrollPhysics::new(100);
        high.pos_milli = 1_000_000;
        high.step(16);
        assert!(high.pos_milli > 100 && high.pos_milli < 1_000_000);
    }

    #[test]
    fn f384_ascii_bypasses_ime() {
        let mut s = ImeSession::new();
        for c in "abc".chars() {
            s.type_char(c);
        }
        assert!(!s.active);
        assert_eq!(s.committed_len, 3);
        assert_eq!(s.commit(), 0);
    }

    #[test]
    fn f385_candidate_edges() {
        let mut c = CandidateWindow::new(0, 0);
        assert_eq!(c.page_count(), 0);
        assert_eq!(c.range(), (0, 0));
        c.move_selection(1); // no-op, no panic
        assert!(!c.next_page());
        let mut full = CandidateWindow::new(4, 4);
        full.move_selection(-1);
        assert_eq!(full.selected, 3); // wraps inside the page
    }

    #[test]
    fn f392_known_dates() {
        assert_eq!(weekday(2000, 1, 1), Some(6)); // Saturday
        assert_eq!(weekday(2026, 1, 1), Some(4)); // Thursday
        assert_eq!(weekday(2026, 13, 1), None);
        assert_eq!(weekday(2026, 2, 30), None);
        assert_eq!(days_in_month(2000, 2), 29);
    }

    #[test]
    fn f393_cache_key_buckets() {
        assert_eq!(icon_cache_key(7, 32, 1000), icon_cache_key(7, 32, 1100));
        assert_ne!(icon_cache_key(7, 32, 1000), icon_cache_key(7, 48, 1000));
        assert_eq!(icon_rung(0), 16);
        assert_eq!(icon_rung(1000), 256);
    }

    #[test]
    fn f394_ease_is_monotonic_and_bounded() {
        for curve in [
            MotionCurve::Linear,
            MotionCurve::Standard,
            MotionCurve::Emphasized,
            MotionCurve::Decelerate,
        ] {
            let mut prev = 0u16;
            for p in (0..=1000).step_by(25) {
                let v = ease(curve, p);
                assert!(v >= prev, "{:?} regressed at {}", curve, p);
                assert!(v <= 1000);
                prev = v;
            }
            assert_eq!(ease(curve, 0), 0);
            assert_eq!(ease(curve, 1000), 1000);
        }
    }

    #[test]
    fn f395_flatten_capacity() {
        let mut t = A11yTree::new();
        for i in 0..MAX_A11Y {
            assert!(t.add(A11yNode {
                id: i as u16,
                role: A11yRole::Text,
                name: "n",
                focusable: true,
                enabled: true,
                depth: 1,
            }));
        }
        let mut out = [0u16; 4];
        assert_eq!(t.flatten(&mut out), 4);
        assert_eq!(t.flatten(&mut []), 0);
    }

    #[test]
    fn f398_degrade_and_reset() {
        let mut g = FrameBudgetGuard::new(1000);
        assert_eq!(g.record(2000), FrameVerdict::Over);
        assert_eq!(g.record(500), FrameVerdict::Within);
        assert!(!g.degraded);
        g.record(2000);
        g.record(2000);
        assert!(g.degraded);
        g.reset();
        assert!(!g.degraded);
        assert_eq!(FrameBudgetGuard::new(1000).over_ratio_permille(), 0);
    }

    #[test]
    fn f399_suite_capacity() {
        let mut s = RegressionSuite::new();
        for i in 0..MAX_SNAPSHOT_ENTRIES {
            let name: &'static str = ["a", "b", "c", "d", "e", "f", "g", "h"][i];
            assert!(s.capture(SnapshotEntry { screen: name, hash: i as u64 }));
        }
        assert!(!s.capture(SnapshotEntry { screen: "z", hash: 0 }));
        assert!(s.capture(SnapshotEntry { screen: "a", hash: 42 })); // update
        assert_eq!(s.compare(SnapshotEntry { screen: "a", hash: 42 }), RegressionVerdict::Match);
    }

    #[test]
    fn f400_self_test_passes() {
        let set = run_ui_checks();
        if !set.all_passed() {
            let mut buf = [0u8; 1024];
            let n = set.render(&mut buf);
            panic!("ui self-test:\n{}", core::str::from_utf8(&buf[..n]).unwrap());
        }
        assert!(set.len() >= 24);
    }
}
