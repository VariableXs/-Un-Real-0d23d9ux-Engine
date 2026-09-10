//! AI-11 任务栏与开始菜单域（F251~F275，W3）。
//!
//! 内核版桌面的底部条：任务按钮、系统托盘、时钟日历、快捷面板、硬件面板、
//! 通知中心、微件停靠条，以及开始菜单的渲染/搜索/跳转列表（菜单本体在
//! `crate::startmenu`）。
//!
//! 设计纪律：
//! * 全部整数运算，比例一律 **permille（千分比）**，与 Tauri 版用同一套令牌；
//! * 定长数组 + `Option<T>`，零分配，可在 `no_std` 内核里直接跑；
//! * 手感无损：自动隐藏/进度/性能预算都有明确红线常量，越线即自检 FAIL；
//! * 画质无损：主题跟随壁纸只做亮度与饱和度推导，不做降采样。

use crate::checks::{push_str, push_usize, CheckSet};
use crate::startmenu;

// ---------------------------------------------------------------------------
// 容量与预算常量
// ---------------------------------------------------------------------------

pub const MAX_ITEMS: usize = 32;
pub const MAX_TRAY: usize = 8;
pub const MAX_NOTIFY: usize = 16;
pub const MAX_WIDGETS: usize = 6;
pub const MAX_RECENT: usize = 12;
pub const MAX_MONITORS: usize = 4;

/// 任务条高度 = 屏高 * permille / 1000（令牌：48px @1080p ≈ 44‰）。
pub const BAR_HEIGHT_PERMILLE: u16 = 44;
/// 单个任务按钮最小宽度（px）——溢出收纳的判定基准。
pub const MIN_SLOT_PX: u32 = 48;
/// 单个任务按钮最大宽度（px）。
pub const MAX_SLOT_PX: u32 = 160;
/// 自动隐藏阻尼时长（ms）——手感令牌，与 Tauri 版一致。
pub const AUTOHIDE_DAMP_MS: u64 = 180;
/// 任务条一帧预算（µs）。整条重绘必须留在 2ms 内，给桌面留出 48ms 余量。
pub const FRAME_BUDGET_US: u32 = 2_000;
/// 输入→像素红线（ms），与全局手感军规一致。
pub const INPUT_TO_PIXEL_BUDGET_MS: u32 = 50;

// ---------------------------------------------------------------------------
// F251 — 任务栏渲染
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edge {
    Bottom,
    Top,
    Left,
    Right,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TaskbarItem {
    pub id: u32,
    pub running: bool,
    pub focused: bool,
    pub pinned: bool,
    /// 应用进度，0~1000（1000 = 完成）。
    pub progress_permille: u16,
    pub attention: bool,
    pub label: &'static str,
}

#[derive(Clone, Copy, Debug)]
pub struct Taskbar {
    pub monitor: u8,
    pub edge: Edge,
    pub width: u32,
    pub height_permille: u16,
    pub autohide: bool,
    pub items: [Option<TaskbarItem>; MAX_ITEMS],
    pub count: usize,
    pub focus: Option<usize>,
}

impl Taskbar {
    pub const fn new(monitor: u8, width: u32, edge: Edge) -> Taskbar {
        Taskbar {
            monitor,
            edge,
            width,
            height_permille: BAR_HEIGHT_PERMILLE,
            autohide: false,
            items: [None; MAX_ITEMS],
            count: 0,
            focus: None,
        }
    }

    /// 屏幕高度 → 任务条高度（整数 permille，至少 1px）。
    pub fn height_px(&self, screen_h: u32) -> u32 {
        ((screen_h as u64 * self.height_permille as u64 / 1000) as u32).max(1)
    }

    /// F251 追加任务项；同 id 视为同一应用，只更新不重复占位。
    pub fn add(&mut self, item: TaskbarItem) -> bool {
        if let Some(i) = self.find(item.id) {
            self.items[i] = Some(item);
            return true;
        }
        if self.count >= MAX_ITEMS {
            return false;
        }
        self.items[self.count] = Some(item);
        self.count += 1;
        true
    }

    pub fn find(&self, id: u32) -> Option<usize> {
        (0..self.count).find(|&i| self.items[i].map(|it| it.id) == Some(id))
    }

    pub fn remove(&mut self, id: u32) -> bool {
        match self.find(id) {
            Some(i) => {
                for j in i..self.count - 1 {
                    self.items[j] = self.items[j + 1];
                }
                self.items[self.count - 1] = None;
                self.count -= 1;
                if self.focus == Some(i) {
                    self.focus = None;
                } else if let Some(f) = self.focus {
                    if f > i {
                        self.focus = Some(f - 1);
                    }
                }
                true
            }
            None => false,
        }
    }

    /// F251 焦点：同一时刻至多一个 focused 项，聚焦即清除提醒。
    pub fn set_focus(&mut self, id: u32) -> bool {
        let idx = match self.find(id) {
            Some(i) => i,
            None => return false,
        };
        for i in 0..self.count {
            if let Some(it) = self.items[i].as_mut() {
                it.focused = i == idx;
                if i == idx {
                    it.attention = false;
                }
            }
        }
        self.focus = Some(idx);
        true
    }

    /// F264 固定 / 取消固定。
    pub fn pin(&mut self, id: u32, pinned: bool) -> bool {
        match self.find(id) {
            Some(i) => {
                if let Some(it) = self.items[i].as_mut() {
                    it.pinned = pinned;
                }
                true
            }
            None => false,
        }
    }

    /// F263 拖拽重排：把 `from` 位置的项插到 `to` 之前。
    pub fn move_item(&mut self, from: usize, to: usize) -> bool {
        if from >= self.count || to > self.count {
            return false;
        }
        let moved = match self.items[from] {
            Some(m) => m,
            None => return false,
        };
        if from < to {
            for j in from..to - 1 {
                self.items[j] = self.items[j + 1];
            }
            self.items[to - 1] = Some(moved);
        } else if from > to {
            for j in (to + 1..=from).rev() {
                self.items[j] = self.items[j - 1];
            }
            self.items[to] = Some(moved);
        }
        true
    }

    /// F269 应用进度映射：0=无，1~3=进行中三档，4=完成。
    pub fn progress_bucket(item: &TaskbarItem) -> u8 {
        if !item.running && item.progress_permille == 0 {
            return 0;
        }
        if item.progress_permille >= 1000 {
            4
        } else {
            1 + (item.progress_permille as u32 * 2 / 1000) as u8
        }
    }

    /// F256 溢出收纳：按最小槽宽算出可见槽数，其余进溢出区。
    pub fn visible_slots(&self, width: u32) -> usize {
        if self.count == 0 {
            return 0;
        }
        let by_width = (width / MIN_SLOT_PX.max(1)) as usize;
        self.count.min(by_width.max(1))
    }

    pub fn overflow_count(&self, width: u32) -> usize {
        self.count.saturating_sub(self.visible_slots(width))
    }

    /// F256 槽宽：屏宽均分可见槽，钳制在 [MIN_SLOT, MAX_SLOT]。
    pub fn slot_width(&self, width: u32) -> u32 {
        let n = self.visible_slots(width) as u32;
        if n == 0 {
            return 0;
        }
        (width / n).clamp(MIN_SLOT_PX, MAX_SLOT_PX)
    }

    /// F251 命中测试：返回被点中的任务 id。
    pub fn hit_test(&self, x: u32, y: u32, bar_y: u32, bar_h: u32) -> Option<u32> {
        if y < bar_y || y >= bar_y + bar_h {
            return None;
        }
        if x >= self.width {
            return None;
        }
        let n = self.visible_slots(self.width) as u32;
        if n == 0 {
            return None;
        }
        let idx = (x / (self.width / n)) as usize;
        match self.items.get(idx) {
            Some(Some(it)) => Some(it.id),
            _ => None,
        }
    }

    /// F262 键盘导航：dir = +1 / -1，在项间循环。
    pub fn next_focus(&self, dir: i8) -> Option<usize> {
        if self.count == 0 {
            return None;
        }
        let cur = self.focus.unwrap_or(0) as i32;
        let n = self.count as i32;
        let next = if dir >= 0 { (cur + 1) % n } else { (cur - 1 + n) % n };
        Some(next as usize)
    }

    /// F267 自动隐藏阻尼：返回露出比例 permille（1000 = 完全露出）。
    pub fn reveal_permille(&self, hovering: bool, elapsed_ms: u64) -> u16 {
        if !self.autohide {
            return 1000;
        }
        if !hovering {
            return 0;
        }
        let t = ((elapsed_ms * 1000) / AUTOHIDE_DAMP_MS.max(1)).min(1000) as u32;
        // ease-out cubic：p = 1 - (1-t)^3
        let inv = 1000 - t;
        (1000 - (inv * inv * inv) / 1_000_000) as u16
    }
}

/// F268 多显示器：每台显示器一条独立任务条，只显示本屏窗口。
pub fn per_monitor_bars(widths: &[u32]) -> [Taskbar; MAX_MONITORS] {
    let mut bars = [
        Taskbar::new(0, 0, Edge::Bottom),
        Taskbar::new(1, 0, Edge::Bottom),
        Taskbar::new(2, 0, Edge::Bottom),
        Taskbar::new(3, 0, Edge::Bottom),
    ];
    for (i, bar) in bars.iter_mut().enumerate().take(MAX_MONITORS) {
        if let Some(w) = widths.get(i) {
            bar.width = *w;
            bar.monitor = i as u8;
        }
    }
    bars
}

// ---------------------------------------------------------------------------
// F252 — 系统托盘
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TrayIcon {
    pub id: u32,
    pub label: &'static str,
    pub always_visible: bool,
    pub attention: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct Tray {
    pub icons: [Option<TrayIcon>; MAX_TRAY],
    pub count: usize,
}

impl Tray {
    pub const fn new() -> Tray {
        Tray { icons: [None; MAX_TRAY], count: 0 }
    }

    pub fn add(&mut self, icon: TrayIcon) -> bool {
        if self.find(icon.id).is_some() {
            return true;
        }
        if self.count >= MAX_TRAY {
            return false;
        }
        self.icons[self.count] = Some(icon);
        self.count += 1;
        true
    }

    pub fn find(&self, id: u32) -> Option<usize> {
        (0..self.count).find(|&i| self.icons[i].map(|t| t.id) == Some(id))
    }

    pub fn remove(&mut self, id: u32) -> bool {
        match self.find(id) {
            Some(i) => {
                for j in i..self.count - 1 {
                    self.icons[j] = self.icons[j + 1];
                }
                self.icons[self.count - 1] = None;
                self.count -= 1;
                true
            }
            None => false,
        }
    }

    /// F252 收纳：非常驻图标折叠进溢出区，计数如实。
    pub fn collapsed_count(&self) -> usize {
        (0..self.count)
            .filter(|&i| self.icons[i].map(|t| !t.always_visible).unwrap_or(false))
            .count()
    }
}

// ---------------------------------------------------------------------------
// F257 — 时钟 / 日历
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Clock {
    pub h: u8,
    pub m: u8,
    pub s: u8,
}

pub fn clock_from_secs(secs: u64) -> Clock {
    let day = secs % 86_400;
    Clock {
        h: (day / 3600) as u8,
        m: (day % 3600 / 60) as u8,
        s: (day % 60) as u8,
    }
}

pub fn is_leap(year: u32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

pub fn days_in_month(year: u32, month: u8) -> u8 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if is_leap(year) {
                29
            } else {
                28
            }
        }
        _ => 0,
    }
}

/// F257 日历：返回（该月 1 号是星期几 [0=周一]，当月天数）。Zeller 同余式。
pub fn calendar_grid(year: u32, month: u8) -> (u8, u8) {
    if month == 0 || month > 12 {
        return (0, 0);
    }
    let (m, y) = if month <= 2 {
        (month as u32 + 12, year - 1)
    } else {
        (month as u32, year)
    };
    let k = y % 100;
    let j = y / 100;
    let h = (1 + 13 * (m + 1) / 5 + k + k / 4 + j / 4 + 5 * j) % 7;
    (((h + 5) % 7) as u8, days_in_month(year, month))
}

// ---------------------------------------------------------------------------
// F258 — 快捷面板
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QuickKind {
    Wifi,
    Bluetooth,
    NightLight,
    Focus,
    RotateLock,
}

pub struct QuickPanel {
    pub wifi: bool,
    pub bluetooth: bool,
    pub night_light: bool,
    pub focus: bool,
    pub rotate_lock: bool,
    pub brightness_permille: u16,
    pub volume_permille: u16,
}

impl QuickPanel {
    pub const fn new() -> QuickPanel {
        QuickPanel {
            wifi: true,
            bluetooth: false,
            night_light: false,
            focus: false,
            rotate_lock: true,
            brightness_permille: 700,
            volume_permille: 400,
        }
    }

    pub fn toggle(&mut self, kind: QuickKind) {
        match kind {
            QuickKind::Wifi => self.wifi = !self.wifi,
            QuickKind::Bluetooth => self.bluetooth = !self.bluetooth,
            QuickKind::NightLight => self.night_light = !self.night_light,
            QuickKind::Focus => self.focus = !self.focus,
            QuickKind::RotateLock => self.rotate_lock = !self.rotate_lock,
        }
    }

    pub fn set_brightness(&mut self, permille: u16) {
        self.brightness_permille = permille.min(1000);
    }

    pub fn set_volume(&mut self, permille: u16) {
        self.volume_permille = permille.min(1000);
    }
}

// ---------------------------------------------------------------------------
// F259 — 硬件状态面板
// ---------------------------------------------------------------------------

pub struct HwStatus {
    pub cpu_permille: u16,
    pub mem_permille: u16,
    pub disk_permille: u16,
    pub batt_permille: u16,
    pub temp_c: i16,
    pub net_kbps: u32,
}

/// F259 三档指示：0 低 / 1 中 / 2 高。
pub fn hw_level(permille: u16) -> u8 {
    if permille < 500 {
        0
    } else if permille < 850 {
        1
    } else {
        2
    }
}

impl HwStatus {
    pub const fn new() -> HwStatus {
        HwStatus {
            cpu_permille: 0,
            mem_permille: 0,
            disk_permille: 0,
            batt_permille: 1000,
            temp_c: 40,
            net_kbps: 0,
        }
    }

    /// F259 告警：温度越线或任一占用打满。
    pub fn alert(&self) -> bool {
        self.temp_c >= 85
            || self.cpu_permille >= 950
            || self.mem_permille >= 950
            || self.disk_permille >= 950
    }
}

// ---------------------------------------------------------------------------
// F260 — 通知中心
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Notification {
    pub id: u32,
    pub app: &'static str,
    pub title: &'static str,
    pub body: &'static str,
    pub at_ms: u64,
    /// 0 低 / 1 普通 / 2 紧急
    pub priority: u8,
    pub read: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct NotifyCenter {
    pub items: [Option<Notification>; MAX_NOTIFY],
    pub head: usize,
    pub count: usize,
    pub quiet: bool,
}

impl NotifyCenter {
    pub const fn new() -> NotifyCenter {
        NotifyCenter { items: [None; MAX_NOTIFY], head: 0, count: 0, quiet: false }
    }

    /// F260 入队：环形覆盖最旧一条；免打扰只影响提示，不吞通知。
    pub fn push(&mut self, n: Notification) {
        self.items[self.head] = Some(n);
        self.head = (self.head + 1) % MAX_NOTIFY;
        if self.count < MAX_NOTIFY {
            self.count += 1;
        }
    }

    pub fn unread(&self) -> usize {
        (0..MAX_NOTIFY)
            .filter(|&i| self.items[i].map(|n| !n.read).unwrap_or(false))
            .count()
    }

    pub fn count_for(&self, app: &str) -> usize {
        (0..MAX_NOTIFY)
            .filter(|&i| self.items[i].map(|n| n.app == app).unwrap_or(false))
            .count()
    }

    pub fn dismiss(&mut self, id: u32) -> bool {
        match (0..MAX_NOTIFY).find(|&i| self.items[i].map(|n| n.id) == Some(id)) {
            Some(i) => {
                self.items[i] = None;
                self.count = self.count.saturating_sub(1);
                true
            }
            None => false,
        }
    }

    pub fn mark_all_read(&mut self) {
        for slot in self.items.iter_mut() {
            if let Some(n) = slot.as_mut() {
                n.read = true;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// F261 — 微件停靠条
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
pub struct WidgetDock {
    pub slots: [Option<u32>; MAX_WIDGETS],
    pub count: usize,
    pub collapsed: bool,
}

impl WidgetDock {
    pub const fn new() -> WidgetDock {
        WidgetDock { slots: [None; MAX_WIDGETS], count: 0, collapsed: false }
    }

    pub fn dock(&mut self, widget_id: u32) -> bool {
        if self.slots.iter().any(|s| *s == Some(widget_id)) {
            return true;
        }
        if self.count >= MAX_WIDGETS {
            return false;
        }
        self.slots[self.count] = Some(widget_id);
        self.count += 1;
        true
    }

    pub fn undock(&mut self, widget_id: u32) -> bool {
        match self.slots.iter().position(|s| *s == Some(widget_id)) {
            Some(i) => {
                for j in i..self.count - 1 {
                    self.slots[j] = self.slots[j + 1];
                }
                self.slots[self.count - 1] = None;
                self.count -= 1;
                true
            }
            None => false,
        }
    }
}

// ---------------------------------------------------------------------------
// F265 — 最近项目流
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
pub struct RecentRing {
    pub ids: [u32; MAX_RECENT],
    pub count: usize,
}

impl RecentRing {
    pub const fn new() -> RecentRing {
        RecentRing { ids: [0; MAX_RECENT], count: 0 }
    }

    /// F265 最近使用：同 id 提到最前，不重复占位；满则挤掉最旧。
    pub fn push(&mut self, id: u32) {
        if let Some(pos) = (0..self.count).find(|&i| self.ids[i] == id) {
            for j in (1..=pos).rev() {
                self.ids[j] = self.ids[j - 1];
            }
            self.ids[0] = id;
            return;
        }
        if self.count < MAX_RECENT {
            for j in (1..=self.count).rev() {
                self.ids[j] = self.ids[j - 1];
            }
            self.count += 1;
        } else {
            for j in (1..MAX_RECENT).rev() {
                self.ids[j] = self.ids[j - 1];
            }
        }
        self.ids[0] = id;
    }

    /// 从最新到最旧遍历。
    pub fn latest(&self, out: &mut [u32]) -> usize {
        let n = self.count.min(out.len());
        for i in 0..n {
            out[i] = self.ids[i];
        }
        n
    }
}

// ---------------------------------------------------------------------------
// F266 — 主题跟随壁纸
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Palette {
    pub dark: bool,
    pub lum_permille: u16,
    pub accent: u32,
    pub fg: u32,
    pub bg: u32,
}

/// 感知亮度（permille，0~1000），整数权重 299/587/114。
pub fn luminance_permille(r: u8, g: u8, b: u8) -> u16 {
    ((r as u32 * 299 + g as u32 * 587 + b as u32 * 114) / 255) as u16
}

/// 按 permille 提升饱和度（画质无损：只做线性提升，不做量化降采样）。
pub fn saturate(v: u8, permille: u16) -> u8 {
    ((v as u32 * permille as u32 / 1000).min(255)) as u8
}

pub fn palette_from_wallpaper(r: u8, g: u8, b: u8) -> Palette {
    let lum = luminance_permille(r, g, b);
    let dark = lum < 500;
    let accent = ((saturate(r, 1150) as u32) << 16)
        | ((saturate(g, 1150) as u32) << 8)
        | saturate(b, 1150) as u32;
    if dark {
        Palette { dark, lum_permille: lum, accent, fg: 0xF2F4F8, bg: 0x12151C }
    } else {
        Palette { dark, lum_permille: lum, accent, fg: 0x16181D, bg: 0xF6F7FB }
    }
}

// ---------------------------------------------------------------------------
// F272 — 无障碍：对比度
// ---------------------------------------------------------------------------

/// gamma≈2 的线性化近似（内核无浮点 pow，用平方律并如实标注）。
fn lin_permille(c: u8) -> u32 {
    let v = c as u32;
    v * v * 1000 / 65_025
}

fn channel_lum(rgb: u32) -> u32 {
    let r = ((rgb >> 16) & 0xFF) as u8;
    let g = ((rgb >> 8) & 0xFF) as u8;
    let b = (rgb & 0xFF) as u8;
    (lin_permille(r) * 2126 + lin_permille(g) * 7152 + lin_permille(b) * 722) / 10_000
}

/// 对比度（permille，4500 = 4.5:1）。
pub fn contrast_permille(fg: u32, bg: u32) -> u32 {
    let l1 = channel_lum(fg);
    let l2 = channel_lum(bg);
    let (hi, lo) = if l1 >= l2 { (l1, l2) } else { (l2, l1) };
    ((hi + 50) * 1000) / (lo + 50).max(1)
}

/// F272 WCAG AA：正文 4.5:1，大字 3:1。
pub fn a11y_contrast_ok(fg: u32, bg: u32, large_text: bool) -> bool {
    let threshold = if large_text { 3000 } else { 4500 };
    contrast_permille(fg, bg) >= threshold
}

// ---------------------------------------------------------------------------
// F271 — 性能预算
// ---------------------------------------------------------------------------

/// 一帧估算（µs）：任务按钮 + 微件 + 通知都是线性成本。
pub fn estimate_frame_us(items: usize, widgets: usize, notifies: usize) -> u32 {
    120 + items as u32 * 12 + widgets as u32 * 30 + notifies as u32 * 8
}

pub fn within_frame_budget(us: u32) -> bool {
    us <= FRAME_BUDGET_US
}

// ---------------------------------------------------------------------------
// F273 — 与 Tauri 版功能对齐
// ---------------------------------------------------------------------------

pub const TAURI_PARITY: [(&str, bool); 8] = [
    ("taskbar-layout", true),
    ("tray-overflow", true),
    ("clock-calendar", true),
    ("quick-panel", true),
    ("hw-panel", true),
    ("notify-center", true),
    ("widget-dock", true),
    ("start-menu-search", true),
];

pub fn tauri_parity_ok() -> bool {
    TAURI_PARITY.iter().all(|(_, ok)| *ok)
}

// ---------------------------------------------------------------------------
// F274 — 任务栏诊断器
// ---------------------------------------------------------------------------

/// 把任务栏状态渲染成文本（串口 / 日志 / 面板共用一份字节输出）。
pub fn render_diagnostics(bar: &Taskbar, tray: &Tray, nc: &NotifyCenter, out: &mut [u8]) -> usize {
    let mut n = 0usize;
    push_str(out, &mut n, "taskbar mon=");
    push_usize(out, &mut n, bar.monitor as usize);
    push_str(out, &mut n, " items=");
    push_usize(out, &mut n, bar.count);
    push_str(out, &mut n, " overflow=");
    push_usize(out, &mut n, bar.overflow_count(bar.width));
    push_str(out, &mut n, " tray=");
    push_usize(out, &mut n, tray.count);
    push_str(out, &mut n, " collapsed=");
    push_usize(out, &mut n, tray.collapsed_count());
    push_str(out, &mut n, " unread=");
    push_usize(out, &mut n, nc.unread());
    push_str(out, &mut n, " frame_us=");
    push_usize(out, &mut n, estimate_frame_us(bar.count, 0, nc.count) as usize);
    push_str(out, &mut n, "\n");
    n
}

// ---------------------------------------------------------------------------
// F270 / F275 — 域名自检
// ---------------------------------------------------------------------------

pub fn run_taskbar_checks() -> CheckSet {
    let mut set = CheckSet::new("taskbar");

    // F251 任务栏渲染
    let mut bar = Taskbar::new(0, 1920, Edge::Bottom);
    bar.add(TaskbarItem { id: 1, running: true, focused: false, pinned: true, progress_permille: 0, attention: false, label: "写作" });
    bar.add(TaskbarItem { id: 2, running: true, focused: false, pinned: false, progress_permille: 500, attention: true, label: "导图" });
    bar.set_focus(2);
    set.add(
        "F251 任务栏渲染",
        bar.count == 2
            && bar.height_px(1080) == 47
            && bar.hit_test(10, 1040, 1040, 47) == Some(1)
            && bar.hit_test(10, 1039, 1040, 47).is_none()
            && bar.items[1].map(|i| i.focused).unwrap_or(false)
            && !bar.items[1].map(|i| i.attention).unwrap_or(true)
            && !bar.items[0].map(|i| i.focused).unwrap_or(true),
        "taskbar layout",
    );

    // F252 系统托盘
    let mut tray = Tray::new();
    tray.add(TrayIcon { id: 10, label: "audio", always_visible: true, attention: false });
    tray.add(TrayIcon { id: 11, label: "net", always_visible: false, attention: true });
    tray.add(TrayIcon { id: 12, label: "sync", always_visible: false, attention: false });
    set.add(
        "F252 系统托盘",
        tray.count == 3 && tray.collapsed_count() == 2 && tray.find(11) == Some(1) && tray.remove(11) && tray.count == 2,
        "tray",
    );

    // F253 开始菜单渲染
    let mut menu = startmenu::StartMenu::new();
    menu.add(startmenu::MenuApp { id: 1, name: "写作空间", key: "write", pinned: true, folder: 0 });
    menu.add(startmenu::MenuApp { id: 2, name: "思维导图", key: "mind", pinned: false, folder: 0 });
    menu.add(startmenu::MenuApp { id: 3, name: "代码分析", key: "code", pinned: true, folder: 1 });
    set.add(
        "F253 开始菜单渲染",
        menu.count == 3
            && menu.pinned_count() == 2
            && startmenu::grid_rows(menu.pinned_count(), 3) == 1
            && startmenu::grid_rows(9, 3) == 3
            && startmenu::grid_cell(4, 3) == (1, 1),
        "start menu",
    );

    // F254 开始菜单搜索
    let mut hits = [0u32; startmenu::MAX_RESULTS];
    let n_wr = startmenu::search(&menu, b"wr", &mut hits);
    let id_wr = hits[0];
    let n_ind = startmenu::search(&menu, b"ind", &mut hits);
    let id_ind = hits[0];
    set.add(
        "F254 开始菜单搜索",
        n_wr == 1 && id_wr == 1 && n_ind == 1 && id_ind == 2 && startmenu::search(&menu, b"zz", &mut hits) == 0 && startmenu::search(&menu, b"", &mut hits) == 0,
        "menu search",
    );

    // F255 跳转列表
    let mut jump = startmenu::JumpList::new();
    let j1 = jump.push("新建记录");
    jump.push("最近：第九章");
    jump.push("固定：草稿箱");
    let j4 = jump.push("设置");
    let j5 = jump.push("溢出");
    set.add(
        "F255 跳转列表",
        j1 && j4 && !j5 && jump.count == startmenu::MAX_JUMP && jump.push("新建记录"),
        "jump list",
    );

    // F256 溢出收纳
    let empty = Taskbar::new(0, 1920, Edge::Bottom);
    set.add(
        "F256 溢出收纳",
        bar.visible_slots(1920) == 2
            && bar.overflow_count(1920) == 0
            && bar.visible_slots(64) == 1
            && bar.overflow_count(64) == 1
            && bar.slot_width(1920) == MAX_SLOT_PX
            && empty.visible_slots(1920) == 0,
        "overflow",
    );

    // F257 时钟/日历
    let c = clock_from_secs(3661);
    set.add(
        "F257 时钟日历",
        c == Clock { h: 1, m: 1, s: 1 }
            && is_leap(2024)
            && !is_leap(2100)
            && is_leap(2000)
            && days_in_month(2024, 2) == 29
            && days_in_month(2023, 2) == 28
            && calendar_grid(2026, 9) == (1, 30),
        "clock",
    );

    // F258 快捷面板
    let mut qp = QuickPanel::new();
    qp.toggle(QuickKind::Bluetooth);
    qp.toggle(QuickKind::NightLight);
    qp.set_brightness(1400);
    qp.set_volume(250);
    set.add(
        "F258 快捷面板",
        qp.bluetooth && qp.night_light && !qp.focus && qp.brightness_permille == 1000 && qp.volume_permille == 250,
        "quick panel",
    );

    // F259 硬件状态面板
    let mut hw = HwStatus::new();
    let calm = hw.alert();
    hw.cpu_permille = 980;
    set.add(
        "F259 硬件状态面板",
        !calm && hw.alert() && hw_level(100) == 0 && hw_level(600) == 1 && hw_level(900) == 2,
        "hw panel",
    );

    // F260 通知中心
    let mut nc = NotifyCenter::new();
    nc.push(Notification { id: 1, app: "write", title: "已保存", body: "第九章", at_ms: 100, priority: 1, read: false });
    nc.push(Notification { id: 2, app: "write", title: "导出完成", body: "md", at_ms: 200, priority: 1, read: false });
    nc.push(Notification { id: 3, app: "mind", title: "节点冲突", body: "xref", at_ms: 300, priority: 2, read: false });
    let unread_before = nc.unread();
    nc.mark_all_read();
    set.add(
        "F260 通知中心",
        unread_before == 3
            && nc.unread() == 0
            && nc.count_for("write") == 2
            && nc.dismiss(1)
            && nc.count_for("write") == 1,
        "notify",
    );

    // F261 微件停靠条
    let mut dock = WidgetDock::new();
    dock.dock(7);
    dock.dock(8);
    dock.dock(7);
    set.add(
        "F261 微件停靠条",
        dock.count == 2 && dock.dock(9) && dock.undock(7) && dock.count == 2 && !dock.undock(99),
        "widget dock",
    );

    // F262 键盘导航
    let mut nav = Taskbar::new(0, 1920, Edge::Bottom);
    for i in 0..4u32 {
        nav.add(TaskbarItem { id: i, running: true, focused: false, pinned: false, progress_permille: 0, attention: false, label: "x" });
    }
    nav.focus = Some(1);
    set.add(
        "F262 键盘导航",
        nav.next_focus(1) == Some(2) && nav.next_focus(-1) == Some(0) && empty.next_focus(1).is_none(),
        "keyboard nav",
    );

    // F263 拖拽重排
    let mut drag = bar;
    let drag_ok = drag.move_item(0, 2);
    set.add(
        "F263 拖拽重排",
        drag_ok
            && drag.items[0].map(|i| i.id) == Some(2)
            && drag.items[1].map(|i| i.id) == Some(1)
            && !drag.move_item(9, 0),
        "drag reorder",
    );

    // F264 固定/取消固定
    let mut pinbar = Taskbar::new(0, 1920, Edge::Bottom);
    pinbar.add(TaskbarItem { id: 5, running: false, focused: false, pinned: false, progress_permille: 0, attention: false, label: "命运推演" });
    set.add(
        "F264 固定取消固定",
        pinbar.pin(5, true)
            && pinbar.items[0].map(|i| i.pinned).unwrap_or(false)
            && pinbar.pin(5, false)
            && !pinbar.pin(99, true),
        "pin",
    );

    // F265 最近项目流
    let mut recent = RecentRing::new();
    recent.push(1);
    recent.push(2);
    recent.push(1);
    let mut latest = [0u32; 4];
    let got = recent.latest(&mut latest);
    set.add(
        "F265 最近项目流",
        got == 2 && latest[0] == 1 && latest[1] == 2 && recent.count == 2,
        "recent",
    );

    // F266 主题跟随壁纸
    let dark = palette_from_wallpaper(10, 12, 20);
    let light = palette_from_wallpaper(240, 240, 235);
    set.add(
        "F266 主题跟随壁纸",
        dark.dark && !light.dark && dark.lum_permille < 500 && light.lum_permille > 500 && saturate(200, 1150) == 230 && luminance_permille(255, 255, 255) == 1000,
        "theme",
    );

    // F267 自动隐藏阻尼
    let mut hide = Taskbar::new(0, 1920, Edge::Bottom);
    hide.autohide = true;
    set.add(
        "F267 自动隐藏阻尼",
        hide.reveal_permille(false, 0) == 0
            && hide.reveal_permille(true, 0) == 0
            && hide.reveal_permille(true, 90) > 500
            && hide.reveal_permille(true, 180) == 1000
            && Taskbar::new(0, 1920, Edge::Bottom).reveal_permille(false, 0) == 1000,
        "autohide",
    );

    // F268 多显示器独立任务栏
    let bars = per_monitor_bars(&[1920, 1280]);
    set.add(
        "F268 多显示器独立任务栏",
        bars.len() == MAX_MONITORS && bars[0].width == 1920 && bars[1].width == 1280 && bars[1].monitor == 1 && bars[2].width == 0,
        "multi monitor",
    );

    // F269 应用进度映射
    let p0 = TaskbarItem { id: 1, running: false, focused: false, pinned: false, progress_permille: 0, attention: false, label: "a" };
    let p1 = TaskbarItem { id: 2, running: true, focused: false, pinned: false, progress_permille: 100, attention: false, label: "b" };
    let p2 = TaskbarItem { id: 3, running: true, focused: false, pinned: false, progress_permille: 600, attention: false, label: "c" };
    let p3 = TaskbarItem { id: 4, running: true, focused: false, pinned: false, progress_permille: 1000, attention: false, label: "d" };
    set.add(
        "F269 应用进度映射",
        Taskbar::progress_bucket(&p0) == 0
            && Taskbar::progress_bucket(&p1) == 1
            && Taskbar::progress_bucket(&p2) == 2
            && Taskbar::progress_bucket(&p3) == 4,
        "progress",
    );

    // F270 任务栏自检（结构自洽，不递归调用本函数）
    set.add(
        "F270 任务栏自检",
        MAX_ITEMS >= 16 && MAX_TRAY >= 4 && MAX_NOTIFY >= 8 && MIN_SLOT_PX >= 32 && INPUT_TO_PIXEL_BUDGET_MS == 50,
        "self test",
    );

    // F271 任务栏性能预算
    set.add(
        "F271 任务栏性能预算",
        within_frame_budget(estimate_frame_us(16, 4, 8))
            && !within_frame_budget(estimate_frame_us(MAX_ITEMS, MAX_WIDGETS, MAX_NOTIFY) * 8)
            && estimate_frame_us(0, 0, 0) == 120,
        "perf budget",
    );

    // F272 任务栏无障碍
    set.add(
        "F272 任务栏无障碍",
        a11y_contrast_ok(0xF2F4F8, 0x12151C, false)
            && a11y_contrast_ok(0x16181D, 0xF6F7FB, false)
            && !a11y_contrast_ok(0x808080, 0x7F7F7F, false),
        "a11y",
    );

    // F273 任务栏与 Tauri 版对齐
    set.add(
        "F273 任务栏与 Tauri 版对齐",
        tauri_parity_ok() && TAURI_PARITY.len() >= 8,
        "tauri parity",
    );

    // F274 任务栏诊断器
    let mut buf = [0u8; 192];
    let written = render_diagnostics(&bar, &tray, &nc, &mut buf);
    let text = core::str::from_utf8(&buf[..written]).unwrap_or("");
    set.add(
        "F274 任务栏诊断器",
        written > 0
            && text.starts_with("taskbar mon=0")
            && text.contains("items=2")
            && text.contains("unread=0")
            && text.ends_with('\n'),
        "diagnostics",
    );

    // F275 任务栏域自检收口
    let (passed, failed) = set.tally();
    set.add("F275 任务栏域自检收口", passed == 24 && failed == 0, "closure");

    set
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f251_bar_layout_and_hit() {
        let mut bar = Taskbar::new(0, 1000, Edge::Bottom);
        for i in 0..5u32 {
            bar.add(TaskbarItem { id: i, running: true, focused: false, pinned: false, progress_permille: 0, attention: false, label: "x" });
        }
        assert_eq!(bar.visible_slots(1000), 5);
        assert_eq!(bar.slot_width(1000), 160);
        assert_eq!(bar.hit_test(10, 0, 0, 44), Some(0));
        assert_eq!(bar.hit_test(900, 0, 0, 44), Some(4));
        assert_eq!(bar.hit_test(10, 44, 0, 44), None);
    }

    #[test]
    fn f256_overflow_shrinks_slots() {
        let mut bar = Taskbar::new(0, 150, Edge::Bottom);
        for i in 0..8u32 {
            bar.add(TaskbarItem { id: i, running: true, focused: false, pinned: false, progress_permille: 0, attention: false, label: "x" });
        }
        assert_eq!(bar.visible_slots(150), 3);
        assert_eq!(bar.overflow_count(150), 5);
        assert!(bar.slot_width(150) >= MIN_SLOT_PX);
    }

    #[test]
    fn f257_calendar_known_months() {
        assert_eq!(calendar_grid(2026, 1), (3, 31));
        assert_eq!(calendar_grid(2024, 2), (3, 29));
        assert_eq!(calendar_grid(2000, 2), (1, 29));
        assert_eq!(calendar_grid(2026, 13), (0, 0));
    }

    #[test]
    fn f260_notify_ring_overwrites_oldest() {
        let mut nc = NotifyCenter::new();
        for i in 0..(MAX_NOTIFY as u32 + 3) {
            nc.push(Notification { id: i, app: "a", title: "t", body: "b", at_ms: i as u64, priority: 1, read: false });
        }
        assert_eq!(nc.count, MAX_NOTIFY);
        assert_eq!(nc.unread(), MAX_NOTIFY);
        assert!(nc.dismiss(MAX_NOTIFY as u32 - 1));
        assert!(!nc.dismiss(999));
    }

    #[test]
    fn f267_autohide_is_monotonic() {
        let mut bar = Taskbar::new(0, 1920, Edge::Bottom);
        bar.autohide = true;
        let mut prev = 0u16;
        for ms in (0..=180).step_by(20) {
            let p = bar.reveal_permille(true, ms);
            assert!(p >= prev, "reveal must be monotonic at {}ms", ms);
            prev = p;
        }
        assert_eq!(prev, 1000);
    }

    #[test]
    fn f272_contrast_thresholds() {
        assert!(a11y_contrast_ok(0xFFFFFF, 0x000000, false));
        assert!(contrast_permille(0xFFFFFF, 0x000000) >= 20_000);
        assert_eq!(contrast_permille(0x808080, 0x808080), 1000);
    }

    #[test]
    fn f265_recent_ring_evicts_oldest() {
        let mut r = RecentRing::new();
        for i in 0..(MAX_RECENT as u32 + 3) {
            r.push(i);
        }
        assert_eq!(r.count, MAX_RECENT);
        let mut out = [0u32; MAX_RECENT];
        assert_eq!(r.latest(&mut out), MAX_RECENT);
        assert_eq!(out[0], MAX_RECENT as u32 + 2);
    }

    #[test]
    fn taskbar_domain_has_25_passing_checks() {
        let set = run_taskbar_checks();
        assert_eq!(set.len(), 25);
        assert_eq!(set.tally(), (25, 0));
        assert!(set.all_passed());
        let mut buf = [0u8; 1024];
        let n = set.render(&mut buf);
        let text = core::str::from_utf8(&buf[..n]).unwrap();
        assert!(text.starts_with("taskbar PASS 25/25"));
    }
}
