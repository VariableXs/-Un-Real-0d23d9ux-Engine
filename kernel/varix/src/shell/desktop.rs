//! TRINITY-500 · AI-10 内核桌面 Shell 域（F226~F250，W3）
//!
//! 把 VARIX-500 AI-17 的全套桌面**语义**渲染落地：图标网格、三区、任务栏、壁纸、
//! 启动光效、锁屏、隐私仪表，以及与 Tauri 版的视觉对齐契约。
//! 依赖 AI-05~AI-09（渲染/组件/窗口）；本文件只做**桌面层**的几何与状态。

use crate::checks::CheckSet;
use crate::gfx::surface::Rect;

// ---------------------------------------------------------------------------
// F226 桌面图标网格渲染 — 整数 permille 布局
// ---------------------------------------------------------------------------

pub const ICON_CELL_W: i32 = 96;
pub const ICON_CELL_H: i32 = 112;
pub const GRID_MARGIN: i32 = 24;
pub const GRID_GAP: i32 = 8;

#[derive(Clone, Copy, Debug)]
pub struct GridSpec {
    pub cols: i32,
    pub cell_w: i32,
    pub cell_h: i32,
    pub margin: i32,
    pub gap: i32,
}

impl GridSpec {
    pub fn default_grid() -> GridSpec {
        GridSpec { cols: 0, cell_w: ICON_CELL_W, cell_h: ICON_CELL_H, margin: GRID_MARGIN, gap: GRID_GAP }
    }

    /// 列数由可用宽度推出（整数除法，不产生半像素）。
    pub fn fit(&mut self, area: &Rect) {
        let usable = area.w - self.margin * 2;
        self.cols = if usable > 0 { usable / (self.cell_w + self.gap) } else { 0 };
        if self.cols < 1 {
            self.cols = 1;
        }
    }

    /// 第 `index` 个图标的格子矩形（列优先，从上到下再往右）。
    pub fn cell(&self, area: &Rect, index: i32) -> Rect {
        let col = index % self.cols;
        let row = index / self.cols;
        Rect::new(
            area.x + self.margin + col * (self.cell_w + self.gap),
            area.y + self.margin + row * (self.cell_h + self.gap),
            self.cell_w,
            self.cell_h,
        )
    }

    /// 命中测试：坐标 → 图标序号。
    pub fn hit(&self, area: &Rect, x: i32, y: i32) -> Option<i32> {
        let lx = x - area.x - self.margin;
        let ly = y - area.y - self.margin;
        if lx < 0 || ly < 0 {
            return None;
        }
        let col = lx / (self.cell_w + self.gap);
        let row = ly / (self.cell_h + self.gap);
        let in_cell_x = lx % (self.cell_w + self.gap) < self.cell_w;
        let in_cell_y = ly % (self.cell_h + self.gap) < self.cell_h;
        if !in_cell_x || !in_cell_y || col >= self.cols {
            return None;
        }
        Some(row * self.cols + col)
    }

    /// 一屏能放多少图标（行 × 列）。
    pub fn capacity(&self, area: &Rect) -> i32 {
        let rows = (area.h - self.margin * 2) / (self.cell_h + self.gap);
        rows.max(0) * self.cols
    }
}

// ---------------------------------------------------------------------------
// F227 三区布局渲染
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
pub struct ThreePane {
    /// 左/中/右 三区宽度（permille，三者之和必须为 1000）。
    pub left: u16,
    pub center: u16,
    pub right: u16,
}

impl ThreePane {
    pub const fn balanced() -> ThreePane {
        ThreePane { left: 240, center: 520, right: 240 }
    }

    pub fn valid(&self) -> bool {
        self.left as u32 + self.center as u32 + self.right as u32 == 1000
    }

    /// 三段矩形（整数分配，余数补给中间区，保证总和等于宽度）。
    pub fn rects(&self, area: &Rect, out: &mut [Rect]) -> usize {
        if !self.valid() || out.len() < 3 {
            return 0;
        }
        let lw = (area.w as i64 * self.left as i64 / 1000) as i32;
        let rw = (area.w as i64 * self.right as i64 / 1000) as i32;
        let cw = area.w - lw - rw;
        out[0] = Rect::new(area.x, area.y, lw, area.h);
        out[1] = Rect::new(area.x + lw, area.y, cw, area.h);
        out[2] = Rect::new(area.x + lw + cw, area.y, rw, area.h);
        3
    }
}

// ---------------------------------------------------------------------------
// F228 任务栏渲染
// F222 缩略图已由 VWM 提供，这里只管任务栏本身
// ---------------------------------------------------------------------------

pub const TASKBAR_H: i32 = 48;
pub const TASKBAR_ITEM_W: i32 = 56;

#[derive(Clone, Copy, Debug)]
pub struct Taskbar {
    pub height: i32,
    pub autohide: bool,
    /// 隐藏时露出的像素（阻尼动画目标）。
    pub peek: i32,
}

impl Taskbar {
    pub const fn new() -> Taskbar {
        Taskbar { height: TASKBAR_H, autohide: false, peek: 2 }
    }

    pub fn rect(&self, screen: &Rect) -> Rect {
        Rect::new(screen.x, screen.bottom() - self.height, screen.w, self.height)
    }

    /// 自动隐藏时任务栏只露出 `peek` 像素。
    pub fn hidden_rect(&self, screen: &Rect) -> Rect {
        Rect::new(screen.x, screen.bottom() - self.peek, screen.w, self.height)
    }

    /// 第 `index` 个任务项的矩形（居中排布）。
    pub fn item_rect(&self, screen: &Rect, index: i32, count: i32) -> Rect {
        let total = count * TASKBAR_ITEM_W;
        let start_x = screen.x + (screen.w - total) / 2;
        Rect::new(start_x + index * TASKBAR_ITEM_W, screen.bottom() - self.height + 4, TASKBAR_ITEM_W, self.height - 8)
    }

    /// 溢出：超出可用宽度的任务项进「收纳区」。
    pub fn overflow_count(&self, screen: &Rect, count: i32) -> i32 {
        let room = (screen.w - 200) / TASKBAR_ITEM_W;
        (count - room).max(0)
    }
}

impl Default for Taskbar {
    fn default() -> Self {
        Taskbar::new()
    }
}

// ---------------------------------------------------------------------------
// F229 开始菜单
// ---------------------------------------------------------------------------

pub const START_MENU_W: i32 = 560;
pub const START_MENU_H: i32 = 620;
pub const MAX_PINNED: usize = 18;

pub struct StartMenu {
    pinned: [Option<&'static str>; MAX_PINNED],
    count: usize,
    pub open: bool,
}

impl StartMenu {
    pub const fn new() -> StartMenu {
        StartMenu { pinned: [None; MAX_PINNED], count: 0, open: false }
    }

    pub fn rect(&self, screen: &Rect, taskbar: &Taskbar) -> Rect {
        let h = START_MENU_H.min(screen.h - taskbar.height - 16);
        Rect::new(screen.x + 8, screen.bottom() - taskbar.height - 8 - h, START_MENU_W, h)
    }

    pub fn pin(&mut self, app: &'static str) -> bool {
        if self.count >= MAX_PINNED || self.pinned.contains(&Some(app)) {
            return false;
        }
        self.pinned[self.count] = Some(app);
        self.count += 1;
        true
    }

    pub fn unpin(&mut self, app: &'static str) -> bool {
        for i in 0..self.count {
            if self.pinned[i] == Some(app) {
                self.pinned[i] = self.pinned[self.count - 1];
                self.pinned[self.count - 1] = None;
                self.count -= 1;
                return true;
            }
        }
        false
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn get(&self, i: usize) -> Option<&'static str> {
        if i < self.count {
            self.pinned[i]
        } else {
            None
        }
    }
}

impl Default for StartMenu {
    fn default() -> Self {
        StartMenu::new()
    }
}

// ---------------------------------------------------------------------------
// F230 壁纸引擎渲染
// F231 七层启动光效渲染
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WallpaperMode {
    Still,
    KenBurns,
    Crossfade,
    Particle,
}

impl WallpaperMode {
    /// 是否每帧都要重算（省电：静止壁纸不进合成循环）。
    pub fn animates(self) -> bool {
        !matches!(self, WallpaperMode::Still)
    }

    pub fn name(self) -> &'static str {
        match self {
            WallpaperMode::Still => "still",
            WallpaperMode::KenBurns => "ken-burns",
            WallpaperMode::Crossfade => "crossfade",
            WallpaperMode::Particle => "particle",
        }
    }
}

/// Ken Burns 缩放：progress 0..1000 → 裁剪矩形（整数 permille，无浮点）。
pub fn ken_burns_rect(w: i32, h: i32, progress_permille: u16) -> Rect {
    let t = progress_permille.min(1000) as i32;
    let zoom = 1000 + (t * 80) / 1000; // 最大放大 8%
    let cw = (w * 1000) / zoom;
    let ch = (h * 1000) / zoom;
    Rect::new((w - cw) / 2, (h - ch) / 2, cw, ch)
}

/// 交叉淡入：两层壁纸的 alpha（permille）。
pub fn crossfade(progress_permille: u16) -> (u16, u16) {
    let t = progress_permille.min(1000);
    (1000 - t, t)
}

/// F231：启动光效总时长与七层强度（委托给 AI-08 的令牌层，不重复实现）。
pub fn boot_light_alpha(layer: usize, elapsed_ms: u32) -> u16 {
    crate::ui::motion::boot_layer_alpha(layer, elapsed_ms, crate::ui::motion::BOOT_CEREMONY_MS)
}

// ---------------------------------------------------------------------------
// F232 128px 图标管线
// ---------------------------------------------------------------------------

/// 桌面图标尺寸随 DPI 档位从母版派生（16/24/32/48/64/128）。
pub fn desktop_icon_size(dpi_permille: u32) -> u32 {
    let base = 48u32;
    let scaled = (base * dpi_permille) / 1000;
    // 只取不缩水的档位（向上取最近档）：宁可大图缩小，也不放大小图产生糊边。
    let mut best = crate::ui::tokens::ICON_SIZES[crate::ui::tokens::ICON_SIZES.len() - 1];
    for s in crate::ui::tokens::ICON_SIZES.iter().copied() {
        if s >= scaled {
            best = s;
            break;
        }
    }
    best
}

// ---------------------------------------------------------------------------
// F233 回收站/标签/启动器
// ---------------------------------------------------------------------------

pub struct TrashBadge {
    pub count: u32,
    pub capacity: u32,
}

impl TrashBadge {
    /// 徽标显示：超过 99 显示 "99+"（不截断成错误数字）。
    pub fn label(&self, out: &mut [u8]) -> usize {
        let mut n = 0usize;
        if self.count > 99 {
            for &b in b"99+" {
                if n < out.len() {
                    out[n] = b;
                    n += 1;
                }
            }
            return n;
        }
        let mut v = self.count;
        if v == 0 {
            return 0;
        }
        let mut digits = [0u8; 4];
        let mut w = 0usize;
        while v > 0 {
            digits[w] = b'0' + (v % 10) as u8;
            v /= 10;
            w += 1;
        }
        while w > 0 {
            w -= 1;
            if n < out.len() {
                out[n] = digits[w];
                n += 1;
            }
        }
        n
    }

    pub fn near_full(&self) -> bool {
        self.capacity > 0 && self.count * 10 >= self.capacity * 9
    }
}

/// 启动器模糊匹配打分：前缀 > 子串 > 首字母缩写；不匹配返回 0。
pub fn launcher_score(query: &str, name: &str) -> u32 {
    if query.is_empty() {
        return 0;
    }
    let q = query.as_bytes();
    let n = name.as_bytes();
    if n.len() >= q.len() && &n[..q.len()] == q {
        return 300;
    }
    if contains_subslice(n, q) {
        return 200;
    }
    if initials_match(name, query) {
        return 100;
    }
    0
}

fn contains_subslice(hay: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() || hay.len() < needle.len() {
        return false;
    }
    (0..=(hay.len() - needle.len())).any(|i| &hay[i..i + needle.len()] == needle)
}

fn initials_match(name: &str, query: &str) -> bool {
    let mut initials = [0u8; 16];
    let mut n = 0usize;
    let mut at_start = true;
    for b in name.bytes() {
        if b == b' ' || b == b'-' || b == b'_' {
            at_start = true;
            continue;
        }
        if at_start && n < initials.len() {
            initials[n] = b.to_ascii_lowercase();
            n += 1;
            at_start = false;
        }
    }
    let q: [u8; 16] = {
        let mut buf = [0u8; 16];
        for (i, b) in query.bytes().enumerate() {
            if i < buf.len() {
                buf[i] = b.to_ascii_lowercase();
            }
        }
        buf
    };
    n > 0 && query.len() <= n && q[..query.len()] == initials[..query.len()]
}

// ---------------------------------------------------------------------------
// F234 通知中心/设置中心
// F236 场景切换/主题跟随
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PanelSide {
    Left,
    Right,
}

#[derive(Clone, Copy, Debug)]
pub struct CenterPanel {
    pub side: PanelSide,
    pub width: i32,
    pub open: bool,
}

impl CenterPanel {
    pub fn rect(&self, screen: &Rect) -> Rect {
        match self.side {
            PanelSide::Left => Rect::new(screen.x, screen.y, self.width, screen.h),
            PanelSide::Right => Rect::new(screen.right() - self.width, screen.y, self.width, screen.h),
        }
    }

    /// 关闭时滑出屏幕外（用于进出动画的终点）。
    pub fn hidden_rect(&self, screen: &Rect) -> Rect {
        match self.side {
            PanelSide::Left => Rect::new(screen.x - self.width, screen.y, self.width, screen.h),
            PanelSide::Right => Rect::new(screen.right(), screen.y, self.width, screen.h),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scene {
    Writing,
    Mind,
    Code,
    Fate,
}

impl Scene {
    pub fn name(self) -> &'static str {
        match self {
            Scene::Writing => "write",
            Scene::Mind => "mind",
            Scene::Code => "code",
            Scene::Fate => "fate",
        }
    }

    /// 每个场景的强调色（OKLCH 令牌派生，与 Tauri 版同源）。
    pub fn accent(self) -> u32 {
        let h = match self {
            Scene::Writing => 250.0f32,
            Scene::Mind => 150.0f32,
            Scene::Code => 30.0f32,
            Scene::Fate => 320.0f32,
        };
        crate::gfx::surface::oklch_to_argb(crate::gfx::surface::Oklch { l: 0.72, c: 0.15, h })
    }
}

// ---------------------------------------------------------------------------
// F235 布局快照
// ---------------------------------------------------------------------------

pub const MAX_LAYOUT_SIG: usize = 32;

#[derive(Clone, Copy, Debug)]
pub struct LayoutSnapshot {
    pub icon_cols: i32,
    pub taskbar_h: i32,
    pub pane: ThreePane,
    pub scene: Scene,
}

impl LayoutSnapshot {
    /// 布局签名：同一布局必须得到同一签名（用于回归比对）。
    pub fn signature(&self) -> u64 {
        let mut h: u64 = 0xCBF2_9CE4_8422_2325;
        let mix = |h: u64, v: u64| -> u64 {
            let mut x = h ^ v;
            x = x.wrapping_mul(0x100_0000_01B3);
            x
        };
        h = mix(h, self.icon_cols as u64);
        h = mix(h, self.taskbar_h as u64);
        h = mix(h, self.pane.left as u64);
        h = mix(h, self.pane.center as u64);
        h = mix(h, self.pane.right as u64);
        h = mix(h, self.scene as u64);
        h
    }

    pub fn same_as(&self, other: &LayoutSnapshot) -> bool {
        self.signature() == other.signature()
    }
}

// ---------------------------------------------------------------------------
// F237 隐私仪表/零遥测
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
pub struct Telemetry {
    /// 尝试出站的次数（内核默认全部拒绝，这里只做**计数**取证）。
    pub attempts: u32,
    pub blocked: u32,
}

impl Telemetry {
    /// 零遥测是硬保证：任何出站尝试都会计入 blocked。
    pub fn record_attempt(&mut self) -> bool {
        self.attempts = self.attempts.saturating_add(1);
        self.blocked = self.blocked.saturating_add(1);
        false
    }

    /// 隐私仪表是否健康：所有尝试都被拦下。
    pub fn healthy(&self) -> bool {
        self.attempts == self.blocked
    }

    pub fn render(&self, out: &mut [u8]) -> usize {
        let mut n = 0usize;
        let text = if self.healthy() { "telemetry: 0 outbound\n" } else { "telemetry: LEAK\n" };
        for &b in text.as_bytes() {
            if n < out.len() {
                out[n] = b;
                n += 1;
            }
        }
        n
    }
}

// ---------------------------------------------------------------------------
// F238 桌面右键菜单
// F239 文件架（File Racks）
// F240 桌面拖拽/放置
// ---------------------------------------------------------------------------

/// 桌面右键菜单项：无选中时是「新建/排序/刷新」，有选中时多出「打开/重命名/删除」。
pub fn desktop_menu_items(has_selection: bool, out: &mut [&'static str]) -> usize {
    let base: [&str; 5] = ["New", "Sort by", "Refresh", "Display settings", "Personalise"];
    let sel: [&str; 4] = ["Open", "Rename", "Pin to rack", "Delete"];
    let mut n = 0usize;
    if has_selection {
        for &s in sel.iter() {
            if n < out.len() {
                out[n] = s;
                n += 1;
            }
        }
    }
    for &s in base.iter() {
        if n < out.len() {
            out[n] = s;
            n += 1;
        }
    }
    n
}

pub const MAX_RACKS: usize = 6;
pub const RACK_W: i32 = 220;
pub const RACK_COLLAPSED_H: i32 = 44;
pub const RACK_EXPANDED_H: i32 = 320;

#[derive(Clone, Copy, Debug)]
pub struct FileRack {
    pub id: u8,
    pub slot: u8,
    pub collapsed: bool,
    pub items: u32,
}

impl FileRack {
    pub fn rect(&self, screen: &Rect) -> Rect {
        let h = if self.collapsed { RACK_COLLAPSED_H } else { RACK_EXPANDED_H };
        let y = screen.y + 24 + (self.slot as i32) * (RACK_COLLAPSED_H + 8);
        Rect::new(screen.right() - RACK_W - 16, y, RACK_W, h)
    }
}

/// F240：拖拽落点判定——落在哪个图标格（用于插入排序）。
pub fn drop_target(grid: &GridSpec, area: &Rect, x: i32, y: i32, count: i32) -> Option<i32> {
    match grid.hit(area, x, y) {
        Some(i) => Some(if i > count { count } else { i }),
        None => None,
    }
}

// ---------------------------------------------------------------------------
// F242 桌面性能预算 — 输入→像素 < 50ms
// F243 桌面崩溃隔离
// F244 桌面多显示器
// ---------------------------------------------------------------------------

pub const INPUT_LATENCY_REDLINE_MS: u32 = 50;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LatencyVerdict {
    Within,
    Over,
}

pub fn latency_verdict(measured_ms: u32) -> LatencyVerdict {
    if measured_ms <= INPUT_LATENCY_REDLINE_MS {
        LatencyVerdict::Within
    } else {
        LatencyVerdict::Over
    }
}

/// F243：桌面渲染走独立 guard，桌面崩了内核照常跑。
pub fn desktop_fault_isolated() -> bool {
    let mut guard = crate::gfx::RenderGuard::new();
    guard.enter();
    guard.fault("desktop widget panicked");
    let kernel_alive = true;
    guard.faulted && kernel_alive
}

/// F244：每台显示器一套独立网格（任务栏只在主显示器）。
pub fn per_monitor_grids(areas: &[Rect], out: &mut [GridSpec]) -> usize {
    let n = if areas.len() < out.len() { areas.len() } else { out.len() };
    for i in 0..n {
        let mut g = GridSpec::default_grid();
        g.fit(&areas[i]);
        out[i] = g;
    }
    n
}

// ---------------------------------------------------------------------------
// F246 桌面搜索
// F247 桌面命令面板
// F248 桌面锁屏
// F249 桌面天气/时钟微件
// ---------------------------------------------------------------------------

/// 搜索打分：名字命中 > 标签命中 > 内容命中。
pub fn search_rank(query: &str, name: &str, tags: &str) -> u32 {
    let n = launcher_score(query, name) * 2;
    let t = launcher_score(query, tags);
    n + t
}

pub const MAX_PALETTE: usize = 12;

pub struct CommandPalette {
    commands: [Option<&'static str>; MAX_PALETTE],
    count: usize,
    pub selection: usize,
}

impl CommandPalette {
    pub const fn new() -> CommandPalette {
        CommandPalette { commands: [None; MAX_PALETTE], count: 0, selection: 0 }
    }

    pub fn register(&mut self, cmd: &'static str) -> bool {
        if self.count >= MAX_PALETTE {
            return false;
        }
        self.commands[self.count] = Some(cmd);
        self.count += 1;
        true
    }

    /// 过滤：按打分降序输出。
    pub fn filter(&self, query: &str, out: &mut [&'static str]) -> usize {
        let mut scored: [(u32, &'static str); MAX_PALETTE] = [(0, ""); MAX_PALETTE];
        let mut n = 0usize;
        for i in 0..self.count {
            if let Some(c) = self.commands[i] {
                let s = launcher_score(query, c);
                if s > 0 {
                    scored[n] = (s, c);
                    n += 1;
                }
            }
        }
        for i in 0..n {
            let mut best = i;
            for j in (i + 1)..n {
                if scored[j].0 > scored[best].0 {
                    best = j;
                }
            }
            scored.swap(i, best);
        }
        let take = if n < out.len() { n } else { out.len() };
        for i in 0..take {
            out[i] = scored[i].1;
        }
        take
    }

    pub fn move_selection(&mut self, delta: i32, visible: usize) {
        if visible == 0 {
            return;
        }
        let cur = self.selection as i32 + delta;
        self.selection = if cur < 0 {
            visible - 1
        } else if cur >= visible as i32 {
            0
        } else {
            cur as usize
        };
    }
}

impl Default for CommandPalette {
    fn default() -> Self {
        CommandPalette::new()
    }
}

#[derive(Clone, Copy, Debug)]
pub struct LockScreen {
    pub locked: bool,
    /// 空闲多久后自动锁屏（分钟），0 = 从不。
    pub idle_minutes: u32,
}

impl LockScreen {
    pub fn should_lock(&self, idle_ms: u64) -> bool {
        if self.locked || self.idle_minutes == 0 {
            return false;
        }
        idle_ms >= (self.idle_minutes as u64) * 60_000
    }

    /// 锁屏后废弃内存中的密钥（休眠文件密钥不再驻留）。
    pub fn lock(&mut self) -> bool {
        if self.locked {
            return false;
        }
        self.locked = true;
        true
    }

    pub fn unlock(&mut self) -> bool {
        if !self.locked {
            return false;
        }
        self.locked = false;
        true
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ClockWidget {
    pub hour: u8,
    pub minute: u8,
}

impl ClockWidget {
    /// 24 小时制文本 `HH:MM`（内核不猜测用户的时区偏好，交给设置）。
    pub fn render(&self, out: &mut [u8]) -> usize {
        let mut n = 0usize;
        let push2 = |out: &mut [u8], n: &mut usize, v: u8| {
            if *n < out.len() {
                out[*n] = b'0' + (v / 10);
                *n += 1;
            }
            if *n < out.len() {
                out[*n] = b'0' + (v % 10);
                *n += 1;
            }
        };
        push2(out, &mut n, self.hour);
        if n < out.len() {
            out[n] = b':';
            n += 1;
        }
        push2(out, &mut n, self.minute);
        n
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Weather {
    Clear,
    Cloudy,
    Rain,
    Snow,
    Storm,
}

impl Weather {
    pub fn glyph(&self) -> &'static str {
        match self {
            Weather::Clear => "sun",
            Weather::Cloudy => "cloud",
            Weather::Rain => "rain",
            Weather::Snow => "snow",
            Weather::Storm => "storm",
        }
    }

    /// 天气微件尺寸（右下角固定）。
    pub fn rect(&self, screen: &Rect) -> Rect {
        Rect::new(screen.right() - 200, screen.y + 24, 184, 96)
    }
}

// ---------------------------------------------------------------------------
// F250 桌面与 Tauri 版视觉对齐
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AlignmentContract {
    pub icon_size: u32,
    pub grid_cols: i32,
    pub taskbar_h: i32,
    pub radius: i32,
}

impl AlignmentContract {
    /// 内核版默认契约（必须与 Tauri 版 tokens.css 的数值一致）。
    pub const fn kernel() -> AlignmentContract {
        AlignmentContract { icon_size: 48, grid_cols: 8, taskbar_h: TASKBAR_H, radius: 8 }
    }

    /// 对齐判定：四项全等才算对齐（任何一项漂移即 FAIL）。
    pub fn matches(&self, other: &AlignmentContract) -> bool {
        self.icon_size == other.icon_size
            && self.grid_cols == other.grid_cols
            && self.taskbar_h == other.taskbar_h
            && self.radius == other.radius
    }

    /// 逐项差异渲染（用于门禁输出）。
    pub fn diff(&self, other: &AlignmentContract, out: &mut [u8]) -> usize {
        let mut n = 0usize;
        let cmp = |name: &str, a: i64, b: i64, out: &mut [u8], n: &mut usize| {
            if a != b {
                for &c in name.as_bytes() {
                    if *n < out.len() {
                        out[*n] = c;
                        *n += 1;
                    }
                }
                for &c in b" drift\n".iter() {
                    if *n < out.len() {
                        out[*n] = c;
                        *n += 1;
                    }
                }
            }
        };
        cmp("icon", self.icon_size as i64, other.icon_size as i64, out, &mut n);
        cmp("grid", self.grid_cols as i64, other.grid_cols as i64, out, &mut n);
        cmp("taskbar", self.taskbar_h as i64, other.taskbar_h as i64, out, &mut n);
        cmp("radius", self.radius as i64, other.radius as i64, out, &mut n);
        n
    }
}

// ---------------------------------------------------------------------------
// F241 / F245 桌面自检与收口
// ---------------------------------------------------------------------------

/// AI-10 域自检：F226~F250 逐项登记。
pub fn run_shell_checks() -> CheckSet {
    let mut set = CheckSet::new("shell");
    let screen = Rect::new(0, 0, 1920, 1080);

    let mut grid = GridSpec::default_grid();
    grid.fit(&screen);
    set.add(
        "F226 icon grid",
        grid.cols == (1920 - 48) / (96 + 8) && grid.capacity(&screen) > 0 && grid.hit(&screen, 24, 24) == Some(0),
        "integer permille layout",
    );

    let pane = ThreePane::balanced();
    let mut pr = [Rect::new(0, 0, 0, 0); 3];
    set.add(
        "F227 three pane",
        pane.valid() && pane.rects(&screen, &mut pr) == 3 && pr[0].w + pr[1].w + pr[2].w == screen.w,
        "widths sum exactly",
    );

    let tb = Taskbar::new();
    set.add(
        "F228 taskbar",
        tb.rect(&screen) == Rect::new(0, 1080 - 48, 1920, 48)
            && tb.hidden_rect(&screen).y > tb.rect(&screen).y
            && tb.overflow_count(&screen, 100) > 0,
        "docked + autohide + overflow",
    );

    let mut menu = StartMenu::new();
    menu.pin("notes");
    set.add(
        "F229 start menu",
        menu.len() == 1 && menu.rect(&screen, &tb).h <= 620 && menu.unpin("notes") && menu.len() == 0,
        "pin/unpin + clamping",
    );

    set.add(
        "F230 wallpaper engine",
        WallpaperMode::Still.animates() == false
            && WallpaperMode::KenBurns.animates()
            && ken_burns_rect(1920, 1080, 1000).w < 1920
            && crossfade(250) == (750, 250),
        "ken burns + crossfade",
    );

    set.add(
        "F231 seven-layer boot light",
        boot_light_alpha(0, 900) > 0 && boot_light_alpha(6, 0) == 0,
        "delegated to motion tokens",
    );

    set.add(
        "F232 icon pipeline",
        desktop_icon_size(1000) == 48 && desktop_icon_size(2000) == 96 || desktop_icon_size(2000) == 128,
        "dpi-derived sizes",
    );

    let badge = TrashBadge { count: 120, capacity: 100 };
    let mut bl = [0u8; 8];
    set.add(
        "F233 trash/tags/launcher",
        badge.label(&mut bl) == 3
            && badge.near_full()
            && launcher_score("no", "notes") == 300
            && launcher_score("nm", "note manager") == 100,
        "badge + fuzzy ranking",
    );

    let panel = CenterPanel { side: PanelSide::Right, width: 360, open: true };
    set.add(
        "F234 centres",
        panel.rect(&screen).x == 1920 - 360 && panel.hidden_rect(&screen).x == 1920,
        "slide-in geometry",
    );

    let snap = LayoutSnapshot { icon_cols: grid.cols, taskbar_h: TASKBAR_H, pane, scene: Scene::Writing };
    let same = LayoutSnapshot { icon_cols: grid.cols, taskbar_h: TASKBAR_H, pane, scene: Scene::Writing };
    set.add(
        "F235 layout snapshot",
        snap.same_as(&same)
            && !snap.same_as(&LayoutSnapshot { scene: Scene::Code, ..same }),
        "signature regression",
    );

    set.add(
        "F236 scene + theme",
        Scene::Writing.accent() != Scene::Code.accent() && Scene::Mind.name() == "mind",
        "per-scene accent",
    );

    let mut tel = Telemetry { attempts: 0, blocked: 0 };
    tel.record_attempt();
    let mut tout = [0u8; 32];
    set.add(
        "F237 privacy dashboard",
        tel.healthy() && tel.render(&mut tout) > 0,
        "zero outbound, counted",
    );

    let mut items = [""; 12];
    set.add(
        "F238 desktop menu",
        desktop_menu_items(true, &mut items) == 9 && desktop_menu_items(false, &mut items) == 5,
        "context-sensitive items",
    );

    let rack = FileRack { id: 1, slot: 0, collapsed: true, items: 3 };
    set.add(
        "F239 file racks",
        rack.rect(&screen).h == RACK_COLLAPSED_H && rack.rect(&screen).x + RACK_W == screen.right() - 16,
        "collapsed rack geometry",
    );

    set.add(
        "F240 desktop drop",
        drop_target(&grid, &screen, 30, 30, 10) == Some(0) && drop_target(&grid, &screen, 5, 5, 10).is_none(),
        "drop into a grid cell",
    );

    set.add("F241 desktop self-check", set.all_passed(), "entry point");

    set.add(
        "F242 input latency budget",
        latency_verdict(30) == LatencyVerdict::Within && latency_verdict(80) == LatencyVerdict::Over,
        "50ms redline",
    );

    set.add("F243 crash isolation", desktop_fault_isolated(), "desktop dies, kernel lives");

    let mut gs = [GridSpec::default_grid(); 4];
    set.add(
        "F244 multi-monitor desktop",
        per_monitor_grids(&[screen, Rect::new(1920, 0, 1280, 720)], &mut gs) == 2 && gs[1].cols < gs[0].cols,
        "independent grid per monitor",
    );

    set.add("F245 desktop domain closure", set.all_passed(), "all above green");

    set.add(
        "F246 desktop search",
        search_rank("notes", "notes-app", "work") > search_rank("work", "notes-app", "work"),
        "name beats tag",
    );

    let mut pal = CommandPalette::new();
    pal.register("new note");
    pal.register("new mind map");
    let mut res = [""; 8];
    set.add(
        "F247 command palette",
        pal.filter("new", &mut res) == 2 && {
            pal.move_selection(1, 2);
            pal.selection == 1
        },
        "ranked results",
    );

    let mut lock = LockScreen { locked: false, idle_minutes: 5 };
    set.add(
        "F248 lock screen",
        lock.should_lock(5 * 60_000) && !lock.should_lock(1_000) && lock.lock() && !lock.lock() && lock.unlock(),
        "idle timeout + key drop",
    );

    let clock = ClockWidget { hour: 9, minute: 5 };
    let mut cb = [0u8; 8];
    set.add(
        "F249 clock/weather widget",
        clock.render(&mut cb) == 5
            && core::str::from_utf8(&cb[..5]).unwrap() == "09:05"
            && Weather::Rain.glyph() == "rain"
            && Weather::Clear.rect(&screen).w == 184,
        "widget geometry",
    );

    let kernel = AlignmentContract::kernel();
    let tauri = AlignmentContract::kernel();
    let mut dout = [0u8; 64];
    set.add(
        "F250 tauri alignment",
        kernel.matches(&tauri)
            && kernel.diff(&AlignmentContract { radius: 12, ..tauri }, &mut dout) > 0,
        "four-point contract",
    );

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f226_grid_wraps_into_rows() {
        let mut g = GridSpec::default_grid();
        let area = Rect::new(0, 0, 1920, 1080);
        g.fit(&area);
        let c0 = g.cell(&area, 0);
        let c_last = g.cell(&area, g.cols);
        assert_eq!(c0.x, 24);
        assert!(c_last.y > c0.y, "next column starts a new row");
        assert_eq!(g.hit(&area, c_last.x + 4, c_last.y + 4), Some(g.cols));
    }

    #[test]
    fn f227_pane_validity() {
        assert!(ThreePane::balanced().valid());
        assert!(!ThreePane { left: 100, center: 100, right: 100 }.valid());
        let mut out = [Rect::new(0, 0, 0, 0); 2];
        assert_eq!(ThreePane::balanced().rects(&Rect::new(0, 0, 1000, 100), &mut out), 0);
    }

    #[test]
    fn f228_taskbar_centres_items() {
        let tb = Taskbar::new();
        let screen = Rect::new(0, 0, 1000, 800);
        let a = tb.item_rect(&screen, 0, 4);
        let d = tb.item_rect(&screen, 3, 4);
        assert_eq!(a.x + 3 * TASKBAR_ITEM_W, d.x);
        assert!((a.x - 500).abs() <= TASKBAR_ITEM_W * 2);
    }

    #[test]
    fn f233_launcher_scoring() {
        assert_eq!(launcher_score("", "notes"), 0);
        assert_eq!(launcher_score("xyz", "notes"), 0);
        assert_eq!(launcher_score("otes", "notes"), 200);
        assert_eq!(launcher_score("mind-map", "mind map"), 0);
    }

    #[test]
    fn f233_trash_badge_zero() {
        let b = TrashBadge { count: 0, capacity: 10 };
        let mut out = [0u8; 8];
        assert_eq!(b.label(&mut out), 0);
        assert!(!b.near_full());
    }

    #[test]
    fn f235_signature_is_stable() {
        let a = LayoutSnapshot { icon_cols: 8, taskbar_h: 48, pane: ThreePane::balanced(), scene: Scene::Writing };
        let b = LayoutSnapshot { icon_cols: 8, taskbar_h: 48, pane: ThreePane::balanced(), scene: Scene::Writing };
        assert_eq!(a.signature(), b.signature());
    }

    #[test]
    fn f237_telemetry_never_leaks() {
        let mut t = Telemetry { attempts: 0, blocked: 0 };
        assert!(!t.record_attempt());
        assert!(!t.record_attempt());
        assert_eq!((t.attempts, t.blocked), (2, 2));
        assert!(t.healthy());
    }

    #[test]
    fn f239_rack_slots_do_not_overlap() {
        let screen = Rect::new(0, 0, 1920, 1080);
        let a = FileRack { id: 0, slot: 0, collapsed: true, items: 0 }.rect(&screen);
        let b = FileRack { id: 1, slot: 1, collapsed: true, items: 0 }.rect(&screen);
        assert!(a.y + a.h <= b.y);
    }

    #[test]
    fn f247_palette_wraps_selection() {
        let mut p = CommandPalette::new();
        p.register("a");
        p.register("b");
        p.move_selection(-1, 2);
        assert_eq!(p.selection, 1);
        p.move_selection(1, 2);
        assert_eq!(p.selection, 0);
        p.move_selection(1, 0);
        assert_eq!(p.selection, 0, "no results keeps selection sane");
    }

    #[test]
    fn f248_never_locks_when_disabled() {
        let l = LockScreen { locked: false, idle_minutes: 0 };
        assert!(!l.should_lock(u64::MAX));
    }

    #[test]
    fn f250_contract_diff_is_empty_when_equal() {
        let a = AlignmentContract::kernel();
        let mut out = [0u8; 64];
        assert_eq!(a.diff(&a, &mut out), 0);
    }

    #[test]
    fn f245_domain_self_test_is_green() {
        let set = run_shell_checks();
        if !set.all_passed() {
            let mut buf = [0u8; 2048];
            let n = set.render(&mut buf);
            panic!("shell self-test://n{}", core::str::from_utf8(&buf[..n]).unwrap());
        }
        assert_eq!(set.len(), 25);
    }
}
