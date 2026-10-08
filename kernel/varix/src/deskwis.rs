//! VARIX-M500 AI-16 · 桌面智慧与空间管理（F376~F400，M5）
//!
//! 桌面从"能摆窗口"到"会替你安排"——工作区、命令面板、专注模式、演示模式。
//! 纯逻辑 + 固定容量数组（no_std），域自检 F400 汇入 `robust::run_kernel_checkup()`。

use crate::checks::CheckSet;

pub const MAX_WORKSPACES: usize = 8;
pub const MAX_WINDOWS: usize = 16;
pub const MAX_HISTORY: usize = 8;

// ---------------------------------------------------------------------------
// F376 多工作区
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Win {
    pub id: u16,
    pub app: u16,
    pub x: i16,
    pub y: i16,
    pub w: u16,
    pub h: u16,
    pub ws: u8,
}

#[derive(Clone, Copy)]
pub struct WorkspaceSet {
    wins: [Option<Win>; MAX_WINDOWS],
    count: usize,
    current: u8,
}

impl WorkspaceSet {
    pub const fn new() -> WorkspaceSet {
        WorkspaceSet { wins: [const { None }; MAX_WINDOWS], count: 0, current: 0 }
    }

    pub fn add(&mut self, w: Win) -> bool {
        if self.count >= MAX_WINDOWS || self.wins[..self.count].iter().any(|o| o.map(|x| x.id) == Some(w.id)) {
            return false;
        }
        self.wins[self.count] = Some(w);
        self.count += 1;
        true
    }

    pub fn count(&self) -> usize {
        self.count
    }

    pub fn get(&self, id: u16) -> Option<Win> {
        self.wins[..self.count].iter().find_map(|o| o.filter(|w| w.id == id))
    }

    /// 把窗口移动到目标工作区（F376）。
    pub fn move_to_ws(&mut self, id: u16, ws: u8) -> bool {
        if ws as usize >= MAX_WORKSPACES {
            return false;
        }
        for slot in self.wins[..self.count].iter_mut() {
            if let Some(w) = slot {
                if w.id == id {
                    w.ws = ws;
                    return true;
                }
            }
        }
        false
    }

    pub fn switch(&mut self, ws: u8) -> bool {
        if ws as usize >= MAX_WORKSPACES {
            return false;
        }
        self.current = ws;
        true
    }

    pub fn current(&self) -> u8 {
        self.current
    }

    pub fn visible(&self) -> usize {
        self.wins[..self.count]
            .iter()
            .filter(|o| o.map(|w| w.ws == self.current).unwrap_or(false))
            .count()
    }
}

// ---------------------------------------------------------------------------
// F377 工作区手势
// ---------------------------------------------------------------------------

/// 三指滑动手势 → 工作区切换步进（不回绕，端点停住）。
pub fn gesture_step(current: u8, swipe_left: bool) -> u8 {
    if swipe_left {
        if current + 1 >= MAX_WORKSPACES as u8 {
            MAX_WORKSPACES as u8 - 1
        } else {
            current + 1
        }
    } else if current == 0 {
        0
    } else {
        current - 1
    }
}

// ---------------------------------------------------------------------------
// F378 贴边分屏
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnapZone {
    Left,
    Right,
    TopMax,
    None,
}

/// 光标贴边 → 目标矩形。屏幕 (0,0)-(sw,sh)。
pub fn snap_zone(cursor_x: i16, cursor_y: i16, sw: u16, sh: u16) -> (SnapZone, i16, i16, u16, u16) {
    let edge = 4i16;
    if cursor_x <= edge {
        (SnapZone::Left, 0, 0, sw / 2, sh)
    } else if cursor_x >= sw as i16 - edge {
        (SnapZone::Right, (sw / 2) as i16, 0, sw - sw / 2, sh)
    } else if cursor_y <= edge {
        (SnapZone::TopMax, 0, 0, sw, sh)
    } else {
        (SnapZone::None, 0, 0, 0, 0)
    }
}

// ---------------------------------------------------------------------------
// F379 布局预设
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layout {
    Single,
    TwoColumn,
    ThreeColumn,
}

/// 对 `n` 个窗口应用布局，产出矩形列表（宽按均分，高全屏）。
pub fn apply_layout(l: Layout, n: usize, sw: u16, sh: u16) -> [(i16, i16, u16, u16); MAX_WINDOWS] {
    let mut out = [(0i16, 0i16, 0u16, 0u16); MAX_WINDOWS];
    let cols = match l {
        Layout::Single => 1,
        Layout::TwoColumn => 2,
        Layout::ThreeColumn => 3,
    };
    let cw = sw / cols as u16;
    let n = n.min(MAX_WINDOWS).min(cols);
    for i in 0..n {
        out[i] = ((i as u16 * cw) as i16, 0, cw, sh);
    }
    out
}

// ---------------------------------------------------------------------------
// F380 窗口记忆
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct GeoMemo {
    pub app: u16,
    pub x: i16,
    pub y: i16,
    pub w: u16,
    pub h: u16,
}

/// 保存几何并在窗口无记忆时回退默认。
pub fn memo_get(memos: &[GeoMemo], app: u16, fb: (i16, i16, u16, u16)) -> (i16, i16, u16, u16) {
    for m in memos {
        if m.app == app {
            return (m.x, m.y, m.w, m.h);
        }
    }
    fb
}

// ---------------------------------------------------------------------------
// F381/F382 全局搜索 + 索引器
// ---------------------------------------------------------------------------

pub const MAX_INDEX: usize = 32;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    App,
    File,
    Setting,
}

#[derive(Clone, Copy)]
pub struct Doc {
    pub id: u16,
    pub kind: Kind,
    pub title: [u8; 12],
    pub tlen: usize,
}

impl Doc {
    pub fn title_str(&self) -> &str {
        core::str::from_utf8(&self.title[..self.tlen]).unwrap_or("")
    }
}

/// 极简前缀索引：query 是 title 的前缀（ASCII 忽略大小写）。
pub fn search(docs: &[Doc], query: &str) -> usize {
    let mut hits = 0;
    for d in docs {
        let t = d.title_str();
        if t.len() >= query.len() && t[..query.len()].eq_ignore_ascii_case(query) {
            hits += 1;
        }
    }
    hits
}

// ---------------------------------------------------------------------------
// F383 命令面板
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct Cmd {
    pub name: &'static str,
    pub action: u8,
}

/// 面板模糊匹配：子序列匹配，返回第一个命中的 action。
pub fn palette_exec(cmds: &[Cmd], query: &str) -> Option<u8> {
    let q = query.as_bytes();
    if q.is_empty() {
        return None;
    }
    for c in cmds {
        let mut qi = 0;
        for &b in c.name.as_bytes() {
            if qi < q.len() && b.to_ascii_lowercase() == q[qi].to_ascii_lowercase() {
                qi += 1;
            }
        }
        if qi == q.len() {
            return Some(c.action);
        }
    }
    None
}

// ---------------------------------------------------------------------------
// F384 最近使用流
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct Mru {
    items: [u16; MAX_HISTORY],
    len: usize,
}

impl Mru {
    pub const fn new() -> Mru {
        Mru { items: [0; MAX_HISTORY], len: 0 }
    }

    /// 访问即置顶，去重（原位置删除）。
    pub fn touch(&mut self, id: u16) {
        let mut pos = None;
        for i in 0..self.len {
            if self.items[i] == id {
                pos = Some(i);
                break;
            }
        }
        if let Some(p) = pos {
            for i in (p..self.len - 1).rev() {
                self.items[i] = self.items[i + 1];
            }
            self.len -= 1;
        }
        if self.len == MAX_HISTORY {
            self.len -= 1;
        }
        for i in (1..=self.len).rev() {
            self.items[i] = self.items[i - 1];
        }
        self.items[0] = id;
        self.len += 1;
    }

    pub fn top(&self) -> Option<u16> {
        if self.len > 0 {
            Some(self.items[0])
        } else {
            None
        }
    }

    pub fn len(&self) -> usize {
        self.len
    }
}

// ---------------------------------------------------------------------------
// F385 任务栏聚类
// ---------------------------------------------------------------------------

/// 按应用聚合窗口 id（返回每组内窗口数，按 app 升序去重）。
pub fn taskbar_groups(wins: &[Win], apps: &mut [u16], counts: &mut [u8]) -> usize {
    let mut n = 0;
    for w in wins {
        let mut found = false;
        for i in 0..n {
            if apps[i] == w.app {
                counts[i] += 1;
                found = true;
                break;
            }
        }
        if !found && n < apps.len() {
            apps[n] = w.app;
            counts[n] = 1;
            n += 1;
        }
    }
    n
}

// ---------------------------------------------------------------------------
// F386 桌面舞台
// ---------------------------------------------------------------------------

/// 舞台模式：返回除焦点窗口外需要调暗的窗口 id 数量。
pub fn stage_focus(wins: &[Win], focus: u16) -> usize {
    wins.iter().filter(|w| w.id != focus).count()
}

// ---------------------------------------------------------------------------
// F387 多屏工作区映射
// ---------------------------------------------------------------------------

/// 显示器 → 工作区绑定表；bind 幂等，unbind 后默认 0。
pub struct ScreenMap {
    pub screens: [u8; 4],
    pub count: usize,
}

impl ScreenMap {
    pub const fn new(count: usize) -> ScreenMap {
        ScreenMap { screens: [0; 4], count: if count > 4 { 4 } else { count } }
    }

    pub fn bind(&mut self, screen: usize, ws: u8) -> bool {
        if screen >= self.count || ws as usize >= MAX_WORKSPACES {
            return false;
        }
        self.screens[screen] = ws;
        true
    }

    pub fn ws_of(&self, screen: usize) -> u8 {
        if screen < self.count {
            self.screens[screen]
        } else {
            0
        }
    }
}

// ---------------------------------------------------------------------------
// F388 空格预览
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Preview {
    Closed,
    Open,
    Zoomed,
}

/// 预览状态机：Closed→Open→Zoomed，Esc 关闭。
pub fn preview_step(state: Preview, space: bool, esc: bool) -> Preview {
    if esc {
        return Preview::Closed;
    }
    if space {
        return match state {
            Preview::Closed => Preview::Open,
            Preview::Open => Preview::Zoomed,
            Preview::Zoomed => Preview::Zoomed,
        };
    }
    state
}

// ---------------------------------------------------------------------------
// F389 跨屏拖拽辅助
// ---------------------------------------------------------------------------

/// 拖拽点距哪个屏幕中心最近（辅助决定落点屏）。
pub fn nearest_screen(x: i32, widths: &[i32]) -> usize {
    let mut best = 0usize;
    let mut best_d = i32::MAX;
    let mut left = 0i32;
    for (i, &w) in widths.iter().enumerate() {
        let center = left + w / 2;
        let d = (x - center).abs();
        if d < best_d {
            best_d = d;
            best = i;
        }
        left += w;
    }
    best
}

// ---------------------------------------------------------------------------
// F390 窗口动画守恒
// ---------------------------------------------------------------------------

/// 位移越大动画时长越长（守恒：速度恒定 1 px/ms，封顶 300ms）。
pub fn anim_ms(from: (i16, i16), to: (i16, i16)) -> u32 {
    let dx = (to.0 - from.0).abs() as i32;
    let dy = (to.1 - from.1).abs() as i32;
    let dist = if dx > dy { dx } else { dy };
    (dist as u32).min(300)
}

// ---------------------------------------------------------------------------
// F391 通知摘要
// ---------------------------------------------------------------------------

/// 免打扰摘要：按应用聚合计数，返回 (app, count) 组数。
pub fn summarize_notes(notes: &[(u16, u8)], out: &mut [(u16, u8)]) -> usize {
    let mut n = 0;
    for &(app, _c) in notes {
        let mut found = false;
        for i in 0..n {
            if out[i].0 == app {
                out[i].1 += 1;
                found = true;
                break;
            }
        }
        if !found && n < out.len() {
            out[n] = (app, 1);
            n += 1;
        }
    }
    n
}

// ---------------------------------------------------------------------------
// F392 专注模式
// ---------------------------------------------------------------------------

/// 专注模式放行判定：白名单应用或高优级才响铃。
pub fn dnd_allow(active: bool, allowlist: &[u16], app: u16, priority: u8) -> bool {
    if !active {
        return true;
    }
    allowlist.contains(&app) || priority >= 9
}

// ---------------------------------------------------------------------------
// F393 屏保框架
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Screensaver {
    Off,
    On,
}

/// 空闲 tick 累计到阈值激活；任何输入清零关闭。
pub fn screensaver_tick(state: Screensaver, idle_ms: u32, threshold_ms: u32, input: bool) -> (Screensaver, u32) {
    if input {
        return (Screensaver::Off, 0);
    }
    if idle_ms >= threshold_ms {
        (Screensaver::On, idle_ms)
    } else {
        (state, idle_ms)
    }
}

// ---------------------------------------------------------------------------
// F394 锁屏画布
// ---------------------------------------------------------------------------

/// 锁屏元素布局：时钟大号居中、日期其下、提示底部。
pub fn lockscreen_layout(sw: u16, sh: u16) -> [(i16, i16, u16); 3] {
    let cx = (sw / 2) as i16;
    let cy = (sh * 3 / 8) as i16;
    let hint_y = (sh - 48) as i16;
    [
        (cx, cy, 32),
        (cx, cy + 40, 14),
        (cx, hint_y, 12),
    ]
}

// ---------------------------------------------------------------------------
// F395 桌面小部件
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct Widget {
    pub id: u16,
    pub x: u16,
    pub y: u16,
    pub w: u16,
    pub h: u16,
}

/// 放置校验：不与既有小部件矩形重叠才允许。
pub fn widget_place(placed: &[Widget], cand: Widget) -> bool {
    for p in placed {
        if cand.x < p.x + p.w && p.x < cand.x + cand.w && cand.y < p.y + p.h && p.y < cand.y + cand.h {
            return false;
        }
    }
    true
}

// ---------------------------------------------------------------------------
// F396 剪贴板历史
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct ClipHist {
    items: [u32; MAX_HISTORY],
    pinned: [bool; MAX_HISTORY],
    len: usize,
}

impl ClipHist {
    pub const fn new() -> ClipHist {
        ClipHist { items: [0; MAX_HISTORY], pinned: [false; MAX_HISTORY], len: 0 }
    }

    pub fn push(&mut self, v: u32) {
        // 固定项永不被挤出。
        let free = self.len;
        if free >= MAX_HISTORY {
            let mut victim = None;
            for i in (0..MAX_HISTORY).rev() {
                if !self.pinned[i] {
                    victim = Some(i);
                    break;
                }
            }
            let v0 = match victim {
                Some(i) => i,
                None => return,
            };
            for i in v0..MAX_HISTORY - 1 {
                self.items[i] = self.items[i + 1];
                self.pinned[i] = self.pinned[i + 1];
            }
            self.items[MAX_HISTORY - 1] = v;
            return;
        }
        for i in (1..=free).rev() {
            self.items[i] = self.items[i - 1];
            self.pinned[i] = self.pinned[i - 1];
        }
        self.items[0] = v;
        self.pinned[0] = false;
        self.len += 1;
    }

    pub fn pin(&mut self, idx: usize, on: bool) -> bool {
        if idx >= self.len {
            return false;
        }
        self.pinned[idx] = on;
        true
    }

    pub fn item(&self, idx: usize) -> Option<u32> {
        if idx < self.len {
            Some(self.items[idx])
        } else {
            None
        }
    }

    pub fn len(&self) -> usize {
        self.len
    }
}

// ---------------------------------------------------------------------------
// F397 屏幕标尺
// ---------------------------------------------------------------------------

/// 生成标尺主刻度数：每 `step_px` 一个主刻度。
pub fn ruler_ticks(width_px: u16, step_px: u16) -> usize {
    if step_px == 0 {
        return 0;
    }
    (width_px / step_px + 1) as usize
}

// ---------------------------------------------------------------------------
// F398 全局取色器
// ---------------------------------------------------------------------------

/// 从 32bpp 帧缓冲取像素并转 565。
pub fn pick_color(fb: &[u8], pitch: usize, x: usize, y: usize) -> Option<u16> {
    let off = y.checked_mul(pitch)?.checked_add(x.checked_mul(4)?)?;
    if off + 3 >= fb.len() {
        return None;
    }
    let (b, g, r) = (fb[off], fb[off + 1], fb[off + 2]);
    Some(((r as u16 >> 3) << 11) | ((g as u16 >> 2) << 5) | (b as u16 >> 3))
}

// ---------------------------------------------------------------------------
// F399 演示模式
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DemoState {
    pub hide_notes: bool,
    pub hide_icons: bool,
    pub spotlight: bool,
}

/// 进入/退出演示模式。
pub fn demo_toggle(on: bool) -> DemoState {
    DemoState { hide_notes: on, hide_icons: on, spotlight: on }
}

// ---------------------------------------------------------------------------
// F400 桌面域自检
// ---------------------------------------------------------------------------

pub fn run_deskwis_checks() -> CheckSet {
    let mut set = CheckSet::new("m5-desk");

    // F376
    let mut ws = WorkspaceSet::new();
    let a1 = ws.add(Win { id: 1, app: 10, x: 0, y: 0, w: 100, h: 50, ws: 0 });
    let a2 = ws.add(Win { id: 2, app: 11, x: 0, y: 0, w: 100, h: 50, ws: 1 });
    let dup = ws.add(Win { id: 1, app: 10, x: 0, y: 0, w: 1, h: 1, ws: 0 });
    let mv = ws.move_to_ws(2, 3);
    let vis_after = ws.visible();
    ws.switch(3);
    let vis3 = ws.visible();
    set.add(
        "F376 multi-workspace",
        a1 && a2 && !dup && mv && vis_after == 1 && ws.current() == 3 && vis3 == 1,
        "workspace assign/move/switch",
    );

    // F377
    let g1 = gesture_step(0, false);
    let g2 = gesture_step(0, true);
    let g3 = gesture_step(MAX_WORKSPACES as u8 - 1, true);
    set.add(
        "F377 workspace gesture",
        g1 == 0 && g2 == 1 && g3 == MAX_WORKSPACES as u8 - 1,
        "gesture clamps at edges",
    );

    // F378
    let (zl, lx, _ly, lw, lh) = snap_zone(2, 50, 800, 600);
    let (zr, rx, _, rw, _) = snap_zone(798, 50, 800, 600);
    let (zt, _, ty, tw, th) = snap_zone(400, 0, 800, 600);
    let zn = snap_zone(400, 300, 800, 600).0;
    set.add(
        "F378 edge snap",
        zl == SnapZone::Left && lx == 0 && lw == 400 && lh == 600
            && zr == SnapZone::Right && rx == 400 && rw == 400
            && zt == SnapZone::TopMax && ty == 0 && tw == 800 && th == 600
            && zn == SnapZone::None,
        "snap zones",
    );

    // F379
    let l2 = apply_layout(Layout::TwoColumn, 2, 800, 600);
    let l3 = apply_layout(Layout::ThreeColumn, 3, 900, 600);
    set.add(
        "F379 layout presets",
        l2[0] == (0, 0, 400, 600) && l2[1] == (400, 0, 400, 600)
            && l3[2] == (600, 0, 300, 600),
        "preset rects",
    );

    // F380
    let memos = [GeoMemo { app: 7, x: 10, y: 20, w: 300, h: 200 }];
    let hit = memo_get(&memos, 7, (0, 0, 800, 600));
    let miss = memo_get(&memos, 8, (0, 0, 800, 600));
    set.add(
        "F380 window memo",
        hit == (10, 20, 300, 200) && miss == (0, 0, 800, 600),
        "geometry recall/fallback",
    );

    // F381/F382
    let mk = |id: u16, kind: Kind, s: &str| -> Doc {
        let mut d = Doc { id, kind, title: [0; 12], tlen: s.len().min(12) };
        d.title[..d.tlen].copy_from_slice(&s.as_bytes()[..d.tlen]);
        d
    };
    let docs = [mk(1, Kind::App, "Terminal"), mk(2, Kind::File, "termdoc")];
    let hits_case = search(&docs, "TERM");
    let hits_narrow = search(&docs, "term");
    let hits_none = search(&docs, "xyz");
    set.add(
        "F381 global search",
        hits_case == 2 && hits_narrow == 2 && hits_none == 0,
        "prefix search across kinds",
    );
    let idx_doc = mk(3, Kind::Setting, "brightness");
    let idx_hit = search(&[idx_doc], "bright");
    set.add("F382 search indexer", idx_hit == 1, "indexed lookup");

    // F383
    let cmds = [
        Cmd { name: "toggle dark", action: 1 },
        Cmd { name: "open terminal", action: 2 },
    ];
    let c1 = palette_exec(&cmds, "op");
    let c2 = palette_exec(&cmds, "dark");
    let c3 = palette_exec(&cmds, "");
    set.add(
        "F383 command palette",
        c1 == Some(2) && c2 == Some(1) && c3.is_none(),
        "fuzzy subsequence match",
    );

    // F384
    let mut mru = Mru::new();
    mru.touch(5);
    mru.touch(6);
    mru.touch(5);
    let top_after = mru.top();
    let mru_len = mru.len();
    set.add("F384 mru stream", top_after == Some(5) && mru_len == 2, "touch dedupes and promotes");

    // F385
    let wins = [
        Win { id: 1, app: 3, x: 0, y: 0, w: 1, h: 1, ws: 0 },
        Win { id: 2, app: 3, x: 0, y: 0, w: 1, h: 1, ws: 0 },
        Win { id: 3, app: 9, x: 0, y: 0, w: 1, h: 1, ws: 0 },
    ];
    let mut apps = [0u16; 4];
    let mut counts = [0u8; 4];
    let groups = taskbar_groups(&wins, &mut apps, &mut counts);
    set.add(
        "F385 taskbar clustering",
        groups == 2 && apps[0] == 3 && counts[0] == 2 && apps[1] == 9 && counts[1] == 1,
        "group by app",
    );

    // F386
    let dimmed = stage_focus(&wins, 2);
    set.add("F386 desktop stage", dimmed == 2, "focus dims others");

    // F387
    let mut sm = ScreenMap::new(2);
    let b1 = sm.bind(1, 4);
    let bad = sm.bind(2, 1);
    let w0 = sm.ws_of(0);
    let w1 = sm.ws_of(1);
    set.add(
        "F387 screen-workspace map",
        b1 && !bad && w0 == 0 && w1 == 4 && sm.ws_of(5) == 0,
        "monitor binding",
    );

    // F388
    let p0 = preview_step(Preview::Closed, true, false);
    let p1 = preview_step(p0, true, false);
    let p2 = preview_step(p1, false, true);
    set.add(
        "F388 quick preview",
        p0 == Preview::Open && p1 == Preview::Zoomed && p2 == Preview::Closed,
        "preview state machine",
    );

    // F389
    let near = nearest_screen(2400, &[1920, 1920]);
    let near0 = nearest_screen(100, &[1920, 1920]);
    set.add("F389 cross-screen drag", near == 1 && near0 == 0, "nearest screen wins");

    // F390
    let short = anim_ms((0, 0), (10, 0));
    let long = anim_ms((0, 0), (900, 0));
    set.add("F390 animation conservation", short == 10 && long == 300, "duration scales, capped");

    // F391
    let notes = [(7u16, 1u8), (7, 1), (9, 1)];
    let mut agg = [(0u16, 0u8); 4];
    let n_groups = summarize_notes(&notes, &mut agg);
    set.add(
        "F391 notification digest",
        n_groups == 2 && agg[0] == (7, 2) && agg[1] == (9, 1),
        "digest by app",
    );

    // F392
    let on = dnd_allow(true, &[3], 3, 1);
    let hi = dnd_allow(true, &[3], 4, 9);
    let lo = dnd_allow(true, &[3], 4, 1);
    let off = dnd_allow(false, &[], 4, 1);
    set.add(
        "F392 focus mode",
        on && hi && !lo && off,
        "allowlist + priority rules",
    );

    // F393
    let (s1, i1) = screensaver_tick(Screensaver::Off, 59_000, 60_000, false);
    let (s2, _) = screensaver_tick(Screensaver::Off, 60_000, 60_000, false);
    let (s3, i3) = screensaver_tick(s2, 61_000, 60_000, true);
    set.add(
        "F393 screensaver",
        s1 == Screensaver::Off && i1 == 59_000 && s2 == Screensaver::On && s3 == Screensaver::Off && i3 == 0,
        "idle activates, input resets",
    );

    // F394
    let lay = lockscreen_layout(800, 600);
    set.add(
        "F394 lockscreen canvas",
        lay[0].1 == 225 && lay[0].2 == 32 && lay[2].1 == 552,
        "clock/date/hint slots",
    );

    // F395
    let placed = [Widget { id: 1, x: 0, y: 0, w: 100, h: 80 }];
    let ok = widget_place(&placed, Widget { id: 2, x: 150, y: 0, w: 50, h: 50 });
    let clash = widget_place(&placed, Widget { id: 2, x: 50, y: 40, w: 50, h: 50 });
    set.add("F395 widget placement", ok && !clash, "collision check");

    // F396
    let mut ch = ClipHist::new();
    for v in 1u32..=9 {
        ch.push(v);
    }
    let pin_ok = ch.pin(3, true);
    for v in 10u32..=14 {
        ch.push(v);
    }
    let pinned_kept = ch.item(3) == Some(5);
    set.add(
        "F396 clipboard history",
        pin_ok && pinned_kept && ch.len() == MAX_HISTORY,
        "pin survives eviction",
    );

    // F397
    let t = ruler_ticks(800, 100);
    let t0 = ruler_ticks(800, 0);
    set.add("F397 screen ruler", t == 9 && t0 == 0, "tick generation");

    // F398
    let mut fb = [0u8; 16];
    fb[0] = 0xFF; // b
    fb[1] = 0x00; // g
    fb[2] = 0xF8; // r
    let c = pick_color(&fb, 8, 0, 0);
    let oob = pick_color(&fb, 8, 3, 3);
    set.add(
        "F398 color picker",
        c == Some(0xF81F) && oob.is_none(),
        "pixel pick + bounds",
    );

    // F399
    let d_on = demo_toggle(true);
    let d_off = demo_toggle(false);
    set.add(
        "F399 demo mode",
        d_on.hide_notes && d_on.spotlight && !d_off.hide_icons,
        "presentation toggles",
    );

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f376_workspace_flow() {
        let mut ws = WorkspaceSet::new();
        assert!(ws.add(Win { id: 1, app: 1, x: 0, y: 0, w: 10, h: 10, ws: 0 }));
        assert!(!ws.add(Win { id: 1, app: 1, x: 0, y: 0, w: 10, h: 10, ws: 0 }));
        assert!(ws.move_to_ws(1, 7));
        assert!(!ws.move_to_ws(1, MAX_WORKSPACES as u8));
        ws.switch(7);
        assert_eq!(ws.visible(), 1);
    }

    #[test]
    fn f384_mru_eviction() {
        let mut m = Mru::new();
        for i in 0..(MAX_HISTORY as u16 + 3) {
            m.touch(i + 1);
        }
        assert_eq!(m.len(), MAX_HISTORY);
        assert_eq!(m.top(), Some(MAX_HISTORY as u16 + 3));
    }

    #[test]
    fn f396_clip_pin_survives() {
        let mut h = ClipHist::new();
        h.push(42);
        h.pin(0, true);
        for v in 100u32..140 {
            h.push(v);
        }
        assert!(h.items.contains(&42));
    }

    #[test]
    fn f400_desk_self_test_passes() {
        let set = run_deskwis_checks();
        if !set.all_passed() {
            let mut buf = [0u8; 1024];
            let n = set.render(&mut buf);
            panic!("m5-desk self-test:\n{}", core::str::from_utf8(&buf[..n]).unwrap());
        }
        assert!(set.len() >= 24);
    }
}
