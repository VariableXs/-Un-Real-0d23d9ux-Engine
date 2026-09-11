//! AURORA-1000 AI-11 · 桌面 shell 与任务栏（A251~A275，W2）
//!
//! 桌面壳（桌面图标/壁纸/任务栏/系统托盘/开始菜单/快捷方式/右键菜单/
//! 无障碍）。所有策略都是纯函数或固定容量结构，便于主机测试套件在无
//! 图形硬件的情况下完整演练整个域；目标端只在最边缘处提供坐标与点击。
//!
//! 约束（与 power.rs 一致）：no_std、只使用 core、固定容量数组 + usize
//! 计数、名称用定长字节数组 `[u8; 32] + len`。禁止 unsafe / 宏 / 泛型 /
//! Vec / String / Box / format! / dyn。

use crate::checks::CheckSet;

// ===========================================================================
// 基础类型：定长名称、几何、命中测试
// ===========================================================================

/// 名称定长容量（字节）。
pub const NAME_CAP: usize = 32;

/// 名称：定长字节数组 + 长度，避免任何分配器依赖。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Name {
    pub bytes: [u8; NAME_CAP],
    pub len: u8,
}

impl Name {
    pub const fn new() -> Name {
        Name { bytes: [0u8; NAME_CAP], len: 0 }
    }

    /// 从字符串字面量构造（超长截断到 NAME_CAP）。
    pub fn from_str(s: &str) -> Name {
        let mut n = Name::new();
        let mut i = 0usize;
        while i < s.len() && i < NAME_CAP {
            n.bytes[i] = s.as_bytes()[i];
            i += 1;
        }
        n.len = i as u8;
        n
    }

    /// 安全转 str（仅写入合法 UTF-8 字节）。
    pub fn as_str<'a>(&'a self) -> &'a str {
        core::str::from_utf8(&self.bytes[..self.len as usize]).unwrap_or("")
    }

    pub fn eq(&self, other: &Name) -> bool {
        if self.len != other.len {
            return false;
        }
        let l = self.len as usize;
        let mut i = 0;
        while i < l {
            if self.bytes[i] != other.bytes[i] {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 前缀过滤（开始菜单搜索用）。
    pub fn starts_with(&self, prefix: &Name) -> bool {
        if prefix.len as usize > self.len as usize {
            return false;
        }
        let l = prefix.len as usize;
        let mut i = 0;
        while i < l {
            if self.bytes[i] != prefix.bytes[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 二维点（屏幕坐标，可为负）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

/// 轴对齐矩形。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

/// 点是否落在矩形内（边界内开区间）。
pub fn point_in_rect(p: Point, r: Rect) -> bool {
    p.x >= r.x
        && p.x < r.x + r.w as i32
        && p.y >= r.y
        && p.y < r.y + r.h as i32
}

// ===========================================================================
// A251 桌面图标布局
// ===========================================================================

pub const MAX_ICONS: usize = 48;
pub const GRID_COLS: usize = 8;
pub const GRID_ROWS: usize = 6;
pub const ICON_W: u32 = 96;
pub const ICON_H: u32 = 96;
pub const ICON_PAD: u32 = 24;

/// 单个桌面图标：标签、网格单元、应用 id。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Icon {
    pub label: Name,
    pub cell: u8,
    pub app_id: u16,
}

/// 桌面图标表：固定容量 + 计数。
#[derive(Clone, Copy, Debug)]
pub struct DesktopIcons {
    icons: [Icon; MAX_ICONS],
    count: usize,
}

impl DesktopIcons {
    pub const fn new() -> DesktopIcons {
        DesktopIcons {
            icons: [Icon { label: Name::new(), cell: 0, app_id: 0 }; MAX_ICONS],
            count: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn get(&self, i: usize) -> Option<Icon> {
        if i < self.count {
            Some(self.icons[i])
        } else {
            None
        }
    }

    /// 网格坐标分配：找到第一个空闲单元并放置。
    pub fn allocate(&mut self, label: Name, app_id: u16) -> Option<usize> {
        if self.count >= MAX_ICONS {
            return None;
        }
        let mut used = [false; MAX_ICONS];
        let mut k = 0;
        while k < self.count {
            used[self.icons[k].cell as usize] = true;
            k += 1;
        }
        let mut cell = 0usize;
        while cell < MAX_ICONS && used[cell] {
            cell += 1;
        }
        if cell >= MAX_ICONS {
            return None;
        }
        self.icons[self.count] = Icon { label, cell: cell as u8, app_id };
        self.count += 1;
        Some(self.count - 1)
    }

    /// 单元 -> 屏幕矩形。
    pub fn rect_of(&self, i: usize) -> Option<Rect> {
        let ic = self.get(i)?;
        let cell = ic.cell as usize;
        let col = cell % GRID_COLS;
        let row = cell / GRID_COLS;
        Some(Rect {
            x: (col as u32 * (ICON_W + ICON_PAD)) as i32,
            y: (row as u32 * (ICON_H + ICON_PAD)) as i32,
            w: ICON_W,
            h: ICON_H,
        })
    }

    /// 命中测试：返回最上层命中的图标索引。
    pub fn hit(&self, p: Point) -> Option<usize> {
        let mut i = 0;
        while i < self.count {
            if let Some(r) = self.rect_of(i) {
                if point_in_rect(p, r) {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 拖拽换位：交换两个图标的网格单元。
    pub fn drag_swap(&mut self, a: usize, b: usize) -> bool {
        if a >= self.count || b >= self.count || a == b {
            return false;
        }
        let ca = self.icons[a].cell;
        let cb = self.icons[b].cell;
        self.icons[a].cell = cb;
        self.icons[b].cell = ca;
        true
    }

    /// 按单元查找图标索引。
    pub fn by_cell(&self, cell: u8) -> Option<usize> {
        let mut i = 0;
        while i < self.count {
            if self.icons[i].cell == cell {
                return Some(i);
            }
            i += 1;
        }
        None
    }
}

/// A251 对齐吸附：把任意点吸附到最近的网格单元（行列夹紧在网格内）。
pub fn snap_to_grid(p: Point) -> (u8, u8) {
    let stride = (ICON_W + ICON_PAD) as i32;
    if stride <= 0 {
        return (0, 0);
    }
    let col = (p.x / stride).clamp(0, GRID_COLS as i32 - 1);
    let row = (p.y / stride).clamp(0, GRID_ROWS as i32 - 1);
    (col as u8, row as u8)
}

/// 双击状态：被点中的图标 + 时间戳。
#[derive(Clone, Copy, Debug)]
pub struct ClickState {
    pub icon: Option<u8>,
    pub t_ms: u32,
}

/// A251 双击打开判定：时间窗内两次命中同一图标。
pub fn detect_double_click(prev: ClickState, cur: ClickState, window_ms: u32) -> bool {
    match (prev.icon, cur.icon) {
        (Some(a), Some(b)) => {
            a == b && cur.t_ms >= prev.t_ms && (cur.t_ms - prev.t_ms) <= window_ms
        }
        _ => false,
    }
}

// ===========================================================================
// A252 壁纸渲染
// ===========================================================================

pub const WALLPAPER_MAX_TILES: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WallpaperMode {
    Center,
    Stretch,
    Tile,
}

/// A252 壁纸矩形计算：居中 / 拉伸 / 平铺，结果写入 out，返回使用的矩形数。
pub fn wallpaper_rects(
    mode: WallpaperMode,
    screen: Rect,
    img_w: u32,
    img_h: u32,
    out: &mut [Rect; WALLPAPER_MAX_TILES],
) -> usize {
    match mode {
        WallpaperMode::Center => {
            let x = screen.x + (screen.w.saturating_sub(img_w)) as i32 / 2;
            let y = screen.y + (screen.h.saturating_sub(img_h)) as i32 / 2;
            out[0] = Rect { x, y, w: img_w, h: img_h };
            1
        }
        WallpaperMode::Stretch => {
            out[0] = screen;
            1
        }
        WallpaperMode::Tile => {
            let cw = img_w.max(1);
            let ch = img_h.max(1);
            let cols = (screen.w + cw - 1) / cw;
            let rows = (screen.h + ch - 1) / ch;
            let mut n = 0usize;
            let mut r = 0u32;
            while r < rows && n < WALLPAPER_MAX_TILES {
                let mut c = 0u32;
                while c < cols && n < WALLPAPER_MAX_TILES {
                    out[n] = Rect {
                        x: screen.x + (c * cw) as i32,
                        y: screen.y + (r * ch) as i32,
                        w: cw,
                        h: ch,
                    };
                    n += 1;
                    c += 1;
                }
                r += 1;
            }
            n
        }
    }
}

// ===========================================================================
// A253 任务栏 / A254 窗口按钮组
// ===========================================================================

pub const MAX_TASKBUTTONS: usize = 16;
pub const TASKBTN_W: u32 = 160;
pub const TASKBTN_H: u32 = 40;
pub const TASKBAR_X: i32 = 0;
pub const TASKBAR_H: u32 = 48;
pub const TASKBAR_OVERFLOW_AT: usize = 12;
pub const TASKBAR_RENDER_COST_PER_BTN_MS: u32 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WinButton {
    pub label: Name,
    pub win_id: u16,
    pub active: bool,
}

/// 任务栏：固定容量窗口按钮 + 锁定/自动隐藏/激活态。
#[derive(Clone, Copy, Debug)]
pub struct Taskbar {
    buttons: [WinButton; MAX_TASKBUTTONS],
    count: usize,
    pub locked: bool,
    pub autohide: bool,
    pub active_index: Option<u8>,
}

impl Taskbar {
    pub const fn new() -> Taskbar {
        Taskbar {
            buttons: [WinButton { label: Name::new(), win_id: 0, active: false };
                MAX_TASKBUTTONS],
            count: 0,
            locked: false,
            autohide: false,
            active_index: None,
        }
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn get(&self, i: usize) -> Option<WinButton> {
        if i < self.count {
            Some(self.buttons[i])
        } else {
            None
        }
    }

    pub fn index_of(&self, win_id: u16) -> Option<u8> {
        let mut i = 0;
        while i < self.count {
            if self.buttons[i].win_id == win_id {
                return Some(i as u8);
            }
            i += 1;
        }
        None
    }

    /// A254 窗口按钮注册。
    pub fn register(&mut self, b: WinButton) -> bool {
        if self.count >= MAX_TASKBUTTONS {
            return false;
        }
        self.buttons[self.count] = b;
        self.count += 1;
        true
    }

    /// A254 窗口按钮移除（保持顺序，修正激活态索引）。
    pub fn remove(&mut self, win_id: u16) -> bool {
        let mut i = 0;
        while i < self.count {
            if self.buttons[i].win_id == win_id {
                let mut j = i;
                while j + 1 < self.count {
                    self.buttons[j] = self.buttons[j + 1];
                    j += 1;
                }
                self.count -= 1;
                match self.active_index {
                    Some(a) if a as usize == i => self.active_index = None,
                    Some(a) if a as usize > i => self.active_index = Some(a - 1),
                    _ => {}
                }
                return true;
            }
            i += 1;
        }
        false
    }

    /// A254 激活态：仅一个按钮 active。
    pub fn activate(&mut self, win_id: u16) -> bool {
        let mut i = 0;
        while i < self.count {
            self.buttons[i].active = self.buttons[i].win_id == win_id;
            i += 1;
        }
        self.active_index = self.index_of(win_id);
        self.active_index.is_some()
    }

    /// A253 点击命中：返回命中的按钮索引。
    pub fn hit(&self, p: Point) -> Option<usize> {
        let mut i = 0;
        while i < self.count {
            let x = TASKBAR_X + (i as u32 * TASKBTN_W) as i32;
            let r = Rect { x, y: 0, w: TASKBTN_W, h: TASKBTN_H };
            if point_in_rect(p, r) {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 溢出折叠计数：超出 TASKBAR_OVERFLOW_AT 的部分折叠进「溢出」按钮。
    pub fn overflow_count(&self) -> usize {
        if self.count > TASKBAR_OVERFLOW_AT {
            self.count - TASKBAR_OVERFLOW_AT
        } else {
            0
        }
    }

    pub fn visible_count(&self) -> usize {
        self.count.min(TASKBAR_OVERFLOW_AT)
    }

    /// A259 任务栏自定义：锁定 / 自动隐藏。
    pub fn set_locked(&mut self, locked: bool) {
        self.locked = locked;
    }
    pub fn set_autohide(&mut self, autohide: bool) {
        self.autohide = autohide;
    }

    /// A264 任务栏性能预算：帧时间 + 每按钮绘制成本是否越界。
    pub fn within_budget(&self, frame_ms: u32, budget_ms: u32) -> bool {
        let cost = self.count as u32 * TASKBAR_RENDER_COST_PER_BTN_MS;
        frame_ms.saturating_add(cost) <= budget_ms
    }

    /// A265 任务栏无障碍：tab 顺序（按钮索引序列）。
    pub fn tab_order(&self, out: &mut [u8; MAX_TASKBUTTONS]) -> usize {
        let mut n = 0usize;
        let mut i = 0;
        while i < self.count && n < MAX_TASKBUTTONS {
            out[n] = i as u8;
            n += 1;
            i += 1;
        }
        n
    }
}

/// A260 任务栏多屏：偶数屏任务栏在底部，奇数屏在顶部。
pub fn taskbar_screen_rect(screen: usize, screen_w: u32, screen_h: u32) -> Rect {
    let y = if screen % 2 == 0 {
        screen_h as i32 - TASKBAR_H as i32
    } else {
        0
    };
    Rect { x: 0, y, w: screen_w, h: TASKBAR_H }
}

// ===========================================================================
// A255 系统托盘 / A261 托盘通知角标
// ===========================================================================

pub const MAX_TRAY: usize = 16;
pub const TRAY_ICON_W: u32 = 24;
pub const TRAY_ICON_H: u32 = 24;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TrayItem {
    pub label: Name,
    pub icon_id: u16,
    pub badge: u8,
}

#[derive(Clone, Copy, Debug)]
pub struct Tray {
    items: [TrayItem; MAX_TRAY],
    count: usize,
}

impl Tray {
    pub const fn new() -> Tray {
        Tray {
            items: [TrayItem { label: Name::new(), icon_id: 0, badge: 0 }; MAX_TRAY],
            count: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn get(&self, i: usize) -> Option<TrayItem> {
        if i < self.count {
            Some(self.items[i])
        } else {
            None
        }
    }

    pub fn register(&mut self, it: TrayItem) -> bool {
        if self.count >= MAX_TRAY {
            return false;
        }
        self.items[self.count] = it;
        self.count += 1;
        true
    }

    pub fn remove(&mut self, icon_id: u16) -> bool {
        let mut i = 0;
        while i < self.count {
            if self.items[i].icon_id == icon_id {
                let mut j = i;
                while j + 1 < self.count {
                    self.items[j] = self.items[j + 1];
                    j += 1;
                }
                self.count -= 1;
                return true;
            }
            i += 1;
        }
        false
    }

    /// A255 点击事件：给定托盘区域基点，命中第 i 个图标。
    pub fn hit(&self, p: Point, base_x: i32, base_y: i32) -> Option<usize> {
        let mut i = 0;
        while i < self.count {
            let x = base_x + (i as u32 * TRAY_ICON_W) as i32;
            let r = Rect { x, y: base_y, w: TRAY_ICON_W, h: TRAY_ICON_H };
            if point_in_rect(p, r) {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// A261 托盘通知角标：累计所有角标数。
    pub fn total_badge(&self) -> usize {
        let mut sum = 0usize;
        let mut i = 0;
        while i < self.count {
            sum += self.items[i].badge as usize;
            i += 1;
        }
        sum
    }

    pub fn has_badge(&self) -> bool {
        self.total_badge() > 0
    }

    /// A274 降级链：清空所有角标（重度降级时用）。
    pub fn clear_badges(&mut self) {
        let mut i = 0;
        while i < self.count {
            self.items[i].badge = 0;
            i += 1;
        }
    }
}

// ===========================================================================
// A256 开始菜单 / A263 桌面搜索入口
// ===========================================================================

pub const MAX_START_ITEMS: usize = 32;
pub const START_PAGE_SIZE: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StartItem {
    pub label: Name,
    pub app_id: u16,
}

#[derive(Clone, Copy, Debug)]
pub struct StartMenu {
    items: [StartItem; MAX_START_ITEMS],
    count: usize,
}

impl StartMenu {
    pub const fn new() -> StartMenu {
        StartMenu {
            items: [StartItem { label: Name::new(), app_id: 0 }; MAX_START_ITEMS],
            count: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn get(&self, i: usize) -> Option<StartItem> {
        if i < self.count {
            Some(self.items[i])
        } else {
            None
        }
    }

    pub fn add(&mut self, it: StartItem) -> bool {
        if self.count >= MAX_START_ITEMS {
            return false;
        }
        self.items[self.count] = it;
        self.count += 1;
        true
    }

    fn filtered_count(&self, filter: &Name) -> usize {
        let mut c = 0usize;
        let mut i = 0;
        while i < self.count {
            if filter.len == 0 || self.items[i].label.starts_with(filter) {
                c += 1;
            }
            i += 1;
        }
        c
    }

    /// A256 字符过滤搜索 + 分页：命中的原始索引写入 out，返回本页数量。
    pub fn query(
        &self,
        filter: &Name,
        page: usize,
        page_size: usize,
        out: &mut [usize; MAX_START_ITEMS],
    ) -> usize {
        let ps = if page_size == 0 { 1 } else { page_size };
        let mut matches = [0usize; MAX_START_ITEMS];
        let mut mcnt = 0usize;
        let mut i = 0;
        while i < self.count && mcnt < MAX_START_ITEMS {
            if filter.len == 0 || self.items[i].label.starts_with(filter) {
                matches[mcnt] = i;
                mcnt += 1;
            }
            i += 1;
        }
        let start = page * ps;
        let mut n = 0usize;
        let mut k = start;
        while k < mcnt && k < start + ps && n < MAX_START_ITEMS {
            out[n] = matches[k];
            n += 1;
            k += 1;
        }
        n
    }

    pub fn page_count(&self, filter: &Name, page_size: usize) -> usize {
        let ps = if page_size == 0 { 1 } else { page_size };
        let total = self.filtered_count(filter);
        if total == 0 {
            0
        } else {
            (total + ps - 1) / ps
        }
    }
}

/// A263 桌面搜索入口：直接走开始菜单的首页过滤。
pub fn desktop_search(
    menu: &StartMenu,
    query: &Name,
    out: &mut [usize; MAX_START_ITEMS],
) -> usize {
    menu.query(query, 0, START_PAGE_SIZE, out)
}

// ===========================================================================
// A257 应用快捷方式
// ===========================================================================

/// A257 快捷方式：目标 + 参数 + 工作目录，全部定长记录。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shortcut {
    pub target: Name,
    pub args: Name,
    pub cwd: Name,
    pub valid: bool,
}

pub fn shortcut_make(target: Name, args: Name, cwd: Name) -> Shortcut {
    Shortcut {
        target,
        args,
        cwd,
        valid: target.len > 0,
    }
}

/// A257 解析：返回 (目标, 参数, 工作目录)。
pub fn shortcut_resolve(sc: &Shortcut) -> (Name, Name, Name) {
    (sc.target, sc.args, sc.cwd)
}

pub fn shortcut_valid(sc: &Shortcut) -> bool {
    sc.valid && sc.target.len > 0
}

// ===========================================================================
// A258 桌面右键菜单
// ===========================================================================

pub const MAX_MENU_ITEMS: usize = 16;
pub const SUBMENU_NONE: u8 = 0xFF;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuAction {
    Open,
    Refresh,
    Sort,
    Settings,
    Submenu,
    Disabled,
    Separator,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MenuItem {
    pub label: Name,
    pub action: MenuAction,
    pub enabled: bool,
    pub submenu: u8,
}

#[derive(Clone, Copy, Debug)]
pub struct ContextMenu {
    items: [MenuItem; MAX_MENU_ITEMS],
    count: usize,
    pub anchor: Point,
    pub cursor: u8,
}

impl ContextMenu {
    pub const fn new() -> ContextMenu {
        ContextMenu {
            items: [MenuItem {
                label: Name::new(),
                action: MenuAction::Separator,
                enabled: true,
                submenu: SUBMENU_NONE,
            }; MAX_MENU_ITEMS],
            count: 0,
            anchor: Point { x: 0, y: 0 },
            cursor: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn get(&self, i: usize) -> Option<MenuItem> {
        if i < self.count {
            Some(self.items[i])
        } else {
            None
        }
    }

    pub fn add(&mut self, it: MenuItem) -> bool {
        if self.count >= MAX_MENU_ITEMS {
            return false;
        }
        self.items[self.count] = it;
        self.count += 1;
        true
    }

    /// A258 位置弹出：在 anchor 处打开并把光标移到首个可选项。
    pub fn popup_at(&mut self, anchor: Point) {
        self.anchor = anchor;
        self.cursor = 0;
    }

    fn is_selectable(&self, i: usize) -> bool {
        match self.get(i) {
            Some(it) => it.enabled && it.action != MenuAction::Separator,
            None => false,
        }
    }

    /// A258 键盘下移：跳过禁用项与分隔符。
    pub fn move_down(&mut self) -> bool {
        if self.count == 0 {
            return false;
        }
        let mut i = (self.cursor as usize + 1) % self.count;
        let start = self.cursor as usize;
        loop {
            if self.is_selectable(i) {
                self.cursor = i as u8;
                return true;
            }
            if i == start {
                break;
            }
            i = (i + 1) % self.count;
        }
        false
    }

    /// A258 键盘上移：跳过禁用项与分隔符。
    pub fn move_up(&mut self) -> bool {
        if self.count == 0 {
            return false;
        }
        let mut i = (self.cursor as usize + self.count - 1) % self.count;
        let start = self.cursor as usize;
        loop {
            if self.is_selectable(i) {
                self.cursor = i as u8;
                return true;
            }
            if i == start {
                break;
            }
            i = (i + self.count - 1) % self.count;
        }
        false
    }

    /// A258 回车触发：禁用项不可触发。
    pub fn activate(&self) -> Option<usize> {
        let c = self.cursor as usize;
        if self.is_selectable(c) {
            Some(c)
        } else {
            None
        }
    }

    /// A258 子菜单层级：进入当前项指向的子菜单。
    pub fn enter_submenu(&mut self) -> Option<usize> {
        let c = self.cursor as usize;
        if let Some(it) = self.get(c) {
            if it.submenu != SUBMENU_NONE && it.submenu as usize != c {
                self.cursor = it.submenu;
                return Some(it.submenu as usize);
            }
        }
        None
    }

    /// A258 无障碍：可遍历项计数。
    pub fn traversable_count(&self) -> usize {
        let mut n = 0;
        let mut i = 0;
        while i < self.count {
            if self.is_selectable(i) {
                n += 1;
            }
            i += 1;
        }
        n
    }
}

// ===========================================================================
// A262 桌面文件夹 / A266 桌面省电 / A267 桌面一致性校验 / A270 性能预算
// A271 可观测 / A272 模糊测试 / A273 文档 / A274 降级链
// ===========================================================================

/// A262 桌面文件夹：拼接 "desktop/<name>" 路径（定长）。
pub fn desktop_folder_path(name: &Name, out: &mut Name) -> bool {
    let prefix = b"desktop/";
    if name.len as usize + prefix.len() > NAME_CAP {
        return false;
    }
    let mut i = 0;
    while i < prefix.len() {
        out.bytes[i] = prefix[i];
        i += 1;
    }
    let mut j = 0;
    while j < name.len as usize {
        out.bytes[prefix.len() + j] = name.bytes[j];
        j += 1;
    }
    out.len = (prefix.len() + name.len as usize) as u8;
    true
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DegradeLevel {
    None,
    Mild,
    Heavy,
}

/// 桌面聚合状态。
#[derive(Clone, Copy, Debug)]
pub struct Desktop {
    pub icons: DesktopIcons,
    pub taskbar: Taskbar,
    pub tray: Tray,
    pub start: StartMenu,
    pub menu: ContextMenu,
    pub wallpaper: WallpaperMode,
    pub dim_idle_ms: u32,
    pub perf_frame_ms: u32,
}

impl Desktop {
    pub const fn new() -> Desktop {
        Desktop {
            icons: DesktopIcons::new(),
            taskbar: Taskbar::new(),
            tray: Tray::new(),
            start: StartMenu::new(),
            menu: ContextMenu::new(),
            wallpaper: WallpaperMode::Center,
            dim_idle_ms: 30_000,
            perf_frame_ms: 4,
        }
    }
}

/// A266 桌面省电：空闲超过阈值则进入变暗。
pub fn desktop_dim_due(idle_ms: u32, d: &Desktop) -> bool {
    idle_ms >= d.dim_idle_ms
}

/// A267 桌面一致性校验：表不满、单元唯一、至多一个激活按钮、壁纸有效、省电阈值>0。
pub fn desktop_consistency(d: &Desktop) -> bool {
    if d.icons.len() > MAX_ICONS
        || d.taskbar.len() > MAX_TASKBUTTONS
        || d.tray.len() > MAX_TRAY
        || d.start.len() > MAX_START_ITEMS
        || d.menu.len() > MAX_MENU_ITEMS
    {
        return false;
    }
    // 图标单元唯一且合法
    let mut used = [false; MAX_ICONS];
    let mut i = 0;
    while i < d.icons.len() {
        if let Some(ic) = d.icons.get(i) {
            if ic.cell as usize >= MAX_ICONS || used[ic.cell as usize] {
                return false;
            }
            used[ic.cell as usize] = true;
        }
        i += 1;
    }
    // 至多一个激活按钮
    let mut active = 0usize;
    let mut j = 0;
    while j < d.taskbar.len() {
        if let Some(b) = d.taskbar.get(j) {
            if b.active {
                active += 1;
            }
        }
        j += 1;
    }
    if active > 1 {
        return false;
    }
    if d.dim_idle_ms == 0 {
        return false;
    }
    true
}

/// A270 桌面性能预算：当前帧时间不超预算。
pub fn desktop_within_perf(d: &Desktop, budget_ms: u32) -> bool {
    d.perf_frame_ms <= budget_ms
}

/// A271 桌面 shell 与任务栏可观测：把关键计数渲染成文本行。
pub fn desktop_metrics(d: &Desktop, out: &mut [u8], n: &mut usize) {
    use crate::checks::{push_str, push_usize};
    push_str(out, n, "desktop icons=");
    push_usize(out, n, d.icons.len());
    push_str(out, n, " taskbar=");
    push_usize(out, n, d.taskbar.len());
    push_str(out, n, " tray=");
    push_usize(out, n, d.tray.len());
    push_str(out, n, " start=");
    push_usize(out, n, d.start.len());
    push_str(out, n, " menu=");
    push_usize(out, n, d.menu.len());
    push_str(out, n, "\n");
}

/// A273 桌面 shell 与任务栏文档：渲染一行能力说明。
pub fn desktop_doc(out: &mut [u8], n: &mut usize) {
    use crate::checks::push_str;
    push_str(
        out,
        n,
        "AURORA-1000 AI-11 desktop shell: icons wallpaper taskbar tray startmenu shortcuts contextmenu a11y",
    );
}

/// A272 桌面 shell 与任务栏模糊测试：以种子驱动布局，校验命中自反不变式。
pub fn desktop_fuzz(seed: u32) -> bool {
    let mut d = Desktop::new();
    let n = (seed % MAX_ICONS as u32) as usize + 1; // 1..=48
    let mut i = 0;
    while i < n {
        let mut lbl = Name::new();
        lbl.bytes[0] = b'A' + (i as u8 % 26);
        lbl.len = 1;
        d.icons.allocate(lbl, (i + 1) as u16);
        i += 1;
    }
    let mut ok = true;
    let mut k = 0;
    while k < d.icons.len() {
        if let Some(r) = d.icons.rect_of(k) {
            let c = Point {
                x: r.x + (r.w / 2) as i32,
                y: r.y + (r.h / 2) as i32,
            };
            if d.icons.hit(c) != Some(k) {
                ok = false;
            }
        }
        k += 1;
    }
    ok
}

/// A274 桌面 shell 与任务栏降级链：按级别降低视觉/功耗开销，最终仍须一致。
pub fn desktop_degrade(level: DegradeLevel, d: &mut Desktop) -> bool {
    match level {
        DegradeLevel::None => {
            d.wallpaper = WallpaperMode::Center;
        }
        DegradeLevel::Mild => {
            d.taskbar.autohide = false;
            d.dim_idle_ms = 15_000;
            d.wallpaper = WallpaperMode::Center;
            d.perf_frame_ms = d.perf_frame_ms.saturating_add(2);
        }
        DegradeLevel::Heavy => {
            d.wallpaper = WallpaperMode::Center;
            d.dim_idle_ms = 5_000;
            d.tray.clear_badges();
            d.perf_frame_ms = d.perf_frame_ms.saturating_add(6);
        }
    }
    desktop_consistency(d)
}

// ===========================================================================
// A268 桌面自检收口 / A269 桌面 shell 与任务栏自检 / A275 域自检收口
// ===========================================================================

/// 收集域核心自检项（≥25 项全真）。
fn collect_desktop_checks(set: &mut CheckSet, _d: &Desktop) {
    // ---- A251 桌面图标布局 ----
    let mut grid = DesktopIcons::new();
    let mut i = 0;
    while i < MAX_ICONS {
        let mut lbl = Name::new();
        lbl.bytes[0] = b'I';
        lbl.len = 1;
        grid.allocate(lbl, (i + 1) as u16);
        i += 1;
    }
    let over = grid.allocate(Name::from_str("x"), 0);
    set.add("A251 icon capacity", grid.len() == MAX_ICONS && over.is_none(), "48 cap");

    let mut used = [false; MAX_ICONS];
    let mut dup = false;
    let mut k = 0;
    while k < grid.len() {
        if let Some(ic) = grid.get(k) {
            if used[ic.cell as usize] {
                dup = true;
            } else {
                used[ic.cell as usize] = true;
            }
        }
        k += 1;
    }
    set.add("A251 cells unique", !dup, "no collision");

    let mut hit_ok = true;
    let mut k = 0;
    while k < grid.len() {
        if let Some(r) = grid.rect_of(k) {
            let c = Point {
                x: r.x + (r.w / 2) as i32,
                y: r.y + (r.h / 2) as i32,
            };
            if grid.hit(c) != Some(k) {
                hit_ok = false;
            }
        }
        k += 1;
    }
    set.add("A251 hit self", hit_ok, "hit test");

    let before_a = grid.get(0).map(|x| x.cell);
    let before_b = grid.get(1).map(|x| x.cell);
    let swapped = grid.drag_swap(0, 1);
    let after_a = grid.get(0).map(|x| x.cell);
    let after_b = grid.get(1).map(|x| x.cell);
    set.add(
        "A251 drag swap",
        swapped && before_a == after_b && before_b == after_a,
        "swap",
    );

    let dc = detect_double_click(
        ClickState { icon: Some(3), t_ms: 1000 },
        ClickState { icon: Some(3), t_ms: 1250 },
        400,
    );
    let dc_bad = detect_double_click(
        ClickState { icon: Some(3), t_ms: 1000 },
        ClickState { icon: Some(4), t_ms: 1200 },
        400,
    );
    set.add("A251 double click", dc && !dc_bad, "dblclick");

    // ---- A252 壁纸渲染 ----
    let screen = Rect { x: 0, y: 0, w: 1920, h: 1080 };
    let mut tb = [Rect { x: 0, y: 0, w: 0, h: 0 }; WALLPAPER_MAX_TILES];
    let n_center = wallpaper_rects(WallpaperMode::Center, screen, 640, 480, &mut tb);
    set.add(
        "A252 center",
        n_center == 1 && tb[0].x == (1920 - 640) / 2 && tb[0].y == (1080 - 480) / 2,
        "center",
    );
    let n_stretch = wallpaper_rects(WallpaperMode::Stretch, screen, 640, 480, &mut tb);
    set.add("A252 stretch", n_stretch == 1 && tb[0] == screen, "stretch");
    let n_tile = wallpaper_rects(WallpaperMode::Tile, screen, 100, 100, &mut tb);
    set.add("A252 tile", n_tile > 1 && n_tile <= WALLPAPER_MAX_TILES, "tile");

    // ---- A253 任务栏 ----
    let mut tb2 = Taskbar::new();
    let mut reg_ok = true;
    let mut i = 0;
    while i < MAX_TASKBUTTONS {
        let mut l = Name::new();
        l.bytes[0] = b'B';
        l.len = 1;
        if !tb2.register(WinButton { label: l, win_id: i as u16, active: false }) {
            reg_ok = false;
        }
        i += 1;
    }
    let cap_fail = tb2.register(WinButton { label: Name::new(), win_id: 99, active: false });
    set.add(
        "A253 register/cap",
        reg_ok && tb2.len() == MAX_TASKBUTTONS && !cap_fail,
        "taskbar cap",
    );
    let hb = tb2.hit(Point {
        x: (2 * TASKBTN_W + TASKBTN_W / 2) as i32,
        y: TASKBTN_H as i32 / 2,
    });
    set.add("A253 hit", hb == Some(2), "taskbar hit");
    let mut tb3 = Taskbar::new();
    let mut i = 0;
    while i < 14 {
        let mut l = Name::new();
        l.bytes[0] = b'T';
        l.len = 1;
        tb3.register(WinButton { label: l, win_id: i as u16, active: false });
        i += 1;
    }
    set.add(
        "A253 overflow",
        tb3.overflow_count() == 2 && tb3.visible_count() == 12,
        "overflow fold",
    );

    // ---- A254 窗口按钮组 ----
    let mut tb4 = Taskbar::new();
    let mut l = Name::new();
    l.bytes[0] = b'A';
    l.len = 1;
    tb4.register(WinButton { label: l, win_id: 5, active: false });
    tb4.register(WinButton { label: l, win_id: 6, active: false });
    let act = tb4.activate(6);
    let mut active_count = 0usize;
    let mut i = 0;
    while i < tb4.len() {
        if let Some(b) = tb4.get(i) {
            if b.active {
                active_count += 1;
            }
        }
        i += 1;
    }
    set.add(
        "A254 activate",
        act && active_count == 1 && tb4.active_index == Some(1),
        "single active",
    );

    // ---- A255 系统托盘 ----
    let mut tray = Tray::new();
    let mut reg_ok = true;
    let mut i = 0;
    while i < MAX_TRAY {
        let mut l = Name::new();
        l.bytes[0] = b'X';
        l.len = 1;
        if !tray.register(TrayItem { label: l, icon_id: i as u16, badge: 0 }) {
            reg_ok = false;
        }
        i += 1;
    }
    let cap_fail = tray.register(TrayItem { label: Name::new(), icon_id: 99, badge: 0 });
    set.add("A255 register/cap", reg_ok && tray.len() == MAX_TRAY && !cap_fail, "tray cap");
    let hit = tray.hit(
        Point {
            x: (3 * TRAY_ICON_W + TRAY_ICON_W / 2) as i32,
            y: TRAY_ICON_H as i32 / 2,
        },
        0,
        0,
    );
    set.add("A255 hit", hit == Some(3), "tray hit");

    // ---- A256 开始菜单 ----
    let mut menu = StartMenu::new();
    let apps: [&[u8]; 8] = [
        &b"app_a"[..],
        &b"app_b"[..],
        &b"browser"[..],
        &b"calc"[..],
        &b"clock"[..],
        &b"cmd"[..],
        &b"editor"[..],
        &b"explorer"[..],
    ];
    let mut added = true;
    let mut i = 0;
    while i < 8 {
        let mut l = Name::new();
        let s = apps[i];
        let mut j = 0;
        while j < s.len() {
            l.bytes[j] = s[j];
            j += 1;
        }
        l.len = s.len() as u8;
        if !menu.add(StartItem { label: l, app_id: (i + 1) as u16 }) {
            added = false;
        }
        i += 1;
    }
    let mut filt = Name::new();
    filt.bytes[0] = b'c';
    filt.len = 1;
    let mut out = [0usize; MAX_START_ITEMS];
    let m = menu.query(&filt, 0, START_PAGE_SIZE, &mut out);
    let mut m_ok = m == 3;
    let mut k = 0;
    while k < m {
        if let Some(it) = menu.get(out[k]) {
            if !it.label.starts_with(&filt) {
                m_ok = false;
            }
        }
        k += 1;
    }
    set.add("A256 query prefix", added && m_ok, "filter");

    // ---- A257 应用快捷方式 ----
    let sc = shortcut_make(Name::from_str("notepad"), Name::from_str("--foo"), Name::from_str("/home"));
    let sc_bad = shortcut_make(Name::new(), Name::new(), Name::new());
    set.add(
        "A257 shortcut valid",
        shortcut_valid(&sc) && !shortcut_valid(&sc_bad),
        "shortcut",
    );

    // ---- A258 桌面右键菜单 ----
    let mut m = ContextMenu::new();
    m.add(MenuItem { label: Name::from_str("Open"), action: MenuAction::Open, enabled: true, submenu: SUBMENU_NONE });
    m.add(MenuItem { label: Name::from_str("X"), action: MenuAction::Disabled, enabled: false, submenu: SUBMENU_NONE });
    m.add(MenuItem { label: Name::from_str("Settings"), action: MenuAction::Settings, enabled: true, submenu: SUBMENU_NONE });
    let moved = m.move_down();
    set.add("A258 nav skip", moved && m.cursor == 2, "skip disabled");
    m.cursor = 1;
    let act = m.activate();
    set.add("A258 activate blocked", act.is_none(), "disabled blocked");

    // ---- A259 任务栏自定义 ----
    let mut t = Taskbar::new();
    t.set_locked(true);
    t.set_autohide(true);
    set.add("A259 lock/autohide", t.locked && t.autohide, "custom");

    // ---- A260 任务栏多屏 ----
    let r0 = taskbar_screen_rect(0, 1920, 1080);
    let r1 = taskbar_screen_rect(1, 1920, 1080);
    set.add(
        "A260 screen rect",
        r0.y == 1080 - TASKBAR_H as i32 && r1.y == 0 && r0.w == 1920,
        "multiscreen",
    );

    // ---- A261 托盘通知角标 ----
    let mut tr = Tray::new();
    tr.register(TrayItem { label: Name::from_str("a"), icon_id: 1, badge: 3 });
    tr.register(TrayItem { label: Name::from_str("b"), icon_id: 2, badge: 5 });
    set.add("A261 badge", tr.total_badge() == 8 && tr.has_badge(), "badge sum");

    // ---- A262 桌面文件夹 ----
    let nm = Name::from_str("apps");
    let mut outp = Name::new();
    let ok = desktop_folder_path(&nm, &mut outp);
    let want = Name::from_str("desktop/apps");
    set.add("A262 folder", ok && outp == want, "folder path");

    // ---- A263 桌面搜索入口 ----
    let mut sm = StartMenu::new();
    let sapps: [&[u8]; 3] = [&b"find"[..], &b"files"[..], &b"help"[..]];
    let mut i = 0;
    while i < 3 {
        let mut l = Name::new();
        let s = sapps[i];
        let mut j = 0;
        while j < s.len() {
            l.bytes[j] = s[j];
            j += 1;
        }
        l.len = s.len() as u8;
        sm.add(StartItem { label: l, app_id: i as u16 });
        i += 1;
    }
    let mut q = Name::new();
    q.bytes[0] = b'f';
    q.len = 1;
    let mut out = [0usize; MAX_START_ITEMS];
    let n = desktop_search(&sm, &q, &mut out);
    set.add("A263 search", n == 2, "search entry");

    // ---- A264 任务栏性能预算 ----
    let mut tb5 = Taskbar::new();
    let mut i = 0;
    while i < 12 {
        let mut l = Name::new();
        l.bytes[0] = b'W';
        l.len = 1;
        tb5.register(WinButton { label: l, win_id: i as u16, active: false });
        i += 1;
    }
    let within = tb5.within_budget(2, 40);
    let over = tb5.within_budget(5, 10);
    set.add("A264 budget", within && !over, "perf budget");

    // ---- A265 任务栏无障碍 ----
    let mut tb6 = Taskbar::new();
    let mut i = 0;
    while i < 5 {
        let mut l = Name::new();
        l.bytes[0] = b'Q';
        l.len = 1;
        tb6.register(WinButton { label: l, win_id: i as u16, active: false });
        i += 1;
    }
    let mut ord = [0u8; MAX_TASKBUTTONS];
    let cnt = tb6.tab_order(&mut ord);
    set.add("A265 tab order", cnt == 5 && ord[0] == 0 && ord[4] == 4, "tab order");

    // ---- A266 桌面省电 ----
    let dd = Desktop::new();
    let dim = desktop_dim_due(31_000, &dd) && !desktop_dim_due(1_000, &dd);
    set.add("A266 dim", dim, "power save");

    // ---- A267 桌面一致性校验（填充态） ----
    let mut pd = Desktop::new();
    let mut l = Name::new();
    l.bytes[0] = b'P';
    l.len = 1;
    pd.icons.allocate(l, 1);
    pd.taskbar.register(WinButton { label: l, win_id: 1, active: false });
    pd.taskbar.register(WinButton { label: l, win_id: 2, active: false });
    pd.taskbar.activate(2);
    pd.tray.register(TrayItem { label: l, icon_id: 1, badge: 0 });
    pd.start.add(StartItem { label: l, app_id: 1 });
    pd.menu.add(MenuItem { label: l, action: MenuAction::Open, enabled: true, submenu: SUBMENU_NONE });
    set.add("A267 consistency", desktop_consistency(&pd), "consistent");

    // ---- A272 模糊测试 ----
    set.add(
        "A272 fuzz",
        desktop_fuzz(0) && desktop_fuzz(12_345) && desktop_fuzz(999),
        "fuzz ok",
    );
}

/// A268 桌面自检收口：最终一致性 + 容量 + 可观测校验。
pub fn desktop_finalize_checks(d: &Desktop, set: &mut CheckSet) {
    set.add(
        "A268 capacity",
        d.icons.len() <= MAX_ICONS
            && d.taskbar.len() <= MAX_TASKBUTTONS
            && d.tray.len() <= MAX_TRAY
            && d.start.len() <= MAX_START_ITEMS
            && d.menu.len() <= MAX_MENU_ITEMS,
        "no overflow",
    );
    set.add("A268 consistency", desktop_consistency(d), "default consistent");
    let mut buf = [0u8; 128];
    let mut n = 0usize;
    desktop_metrics(d, &mut buf, &mut n);
    set.add("A271 metrics", n > 0, "metrics rendered");
}

/// A269 桌面 shell 与任务栏自检：构建默认桌面并产出完整 CheckSet。
pub fn desktop_selfcheck() -> CheckSet {
    let d = Desktop::new();
    let mut set = CheckSet::new("aurora-desktop");
    collect_desktop_checks(&mut set, &d);
    desktop_finalize_checks(&d, &mut set);
    set
}

/// A275 桌面 shell 与任务栏域自检收口（必做项，对外入口）。
pub fn run_desktop_checks() -> CheckSet {
    desktop_selfcheck()
}

// ===========================================================================
// 单元测试
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a251_capacity_full() {
        let mut g = DesktopIcons::new();
        for i in 0..MAX_ICONS {
            let mut l = Name::new();
            l.bytes[0] = b'I';
            l.len = 1;
            assert!(g.allocate(l, i as u16).is_some());
        }
        // 表满：第 49 个应失败
        assert!(g.allocate(Name::from_str("x"), 0).is_none());
        assert_eq!(g.len(), MAX_ICONS);
    }

    #[test]
    fn a251_cells_unique_and_layout() {
        let mut g = DesktopIcons::new();
        for i in 0..MAX_ICONS {
            let mut l = Name::new();
            l.bytes[0] = b'L';
            l.len = 1;
            g.allocate(l, i as u16);
        }
        let mut seen = [false; MAX_ICONS];
        for i in 0..MAX_ICONS {
            let cell = g.get(i).unwrap().cell as usize;
            assert!(!seen[cell]);
            seen[cell] = true;
        }
        // 单元 0 应在左上角
        let r = g.rect_of(0).unwrap();
        assert_eq!((r.x, r.y), (0, 0));
        assert_eq!((r.w, r.h), (ICON_W, ICON_H));
    }

    #[test]
    fn a251_hit_and_double_click() {
        let mut g = DesktopIcons::new();
        let mut l = Name::new();
        l.bytes[0] = b'H';
        l.len = 1;
        g.allocate(l, 1);
        let r = g.rect_of(0).unwrap();
        let center = Point { x: r.x + 10, y: r.y + 10 };
        assert_eq!(g.hit(center), Some(0));
        // 空白处命中 None
        assert_eq!(g.hit(Point { x: -50, y: -50 }), None);

        let dc = detect_double_click(
            ClickState { icon: Some(0), t_ms: 0 },
            ClickState { icon: Some(0), t_ms: 300 },
            400,
        );
        assert!(dc);
        let dc_bad = detect_double_click(
            ClickState { icon: Some(0), t_ms: 0 },
            ClickState { icon: Some(0), t_ms: 900 },
            400,
        );
        assert!(!dc_bad);
    }

    #[test]
    fn a251_drag_swap_and_snap() {
        let mut g = DesktopIcons::new();
        let mut a = Name::new();
        a.bytes[0] = b'A';
        a.len = 1;
        let mut b = Name::new();
        b.bytes[0] = b'B';
        b.len = 1;
        let i0 = g.allocate(a, 1).unwrap();
        let i1 = g.allocate(b, 2).unwrap();
        let c0 = g.get(i0).unwrap().cell;
        let c1 = g.get(i1).unwrap().cell;
        assert!(g.drag_swap(i0, i1));
        assert_eq!(g.get(i0).unwrap().cell, c1);
        assert_eq!(g.get(i1).unwrap().cell, c0);
        // 越界交换失败
        assert!(!g.drag_swap(0, 99));

        let s = snap_to_grid(Point { x: 0, y: 0 });
        assert_eq!(s, (0, 0));
        let s2 = snap_to_grid(Point { x: 1000, y: 1000 });
        assert_eq!(s2, ((GRID_COLS - 1) as u8, (GRID_ROWS - 1) as u8));
    }

    #[test]
    fn a252_wallpaper_modes() {
        let screen = Rect { x: 0, y: 0, w: 800, h: 600 };
        let mut out = [Rect { x: 0, y: 0, w: 0, h: 0 }; WALLPAPER_MAX_TILES];
        let n = wallpaper_rects(WallpaperMode::Center, screen, 200, 100, &mut out);
        assert_eq!(n, 1);
        assert_eq!(out[0].x, 300);
        assert_eq!(out[0].y, 250);
        let n = wallpaper_rects(WallpaperMode::Stretch, screen, 200, 100, &mut out);
        assert_eq!(n, 1);
        assert_eq!(out[0], screen);
        let n = wallpaper_rects(WallpaperMode::Tile, screen, 50, 50, &mut out);
        assert!(n > 1 && n <= WALLPAPER_MAX_TILES);
    }

    #[test]
    fn a253_taskbar_register_hit_overflow() {
        let mut t = Taskbar::new();
        for i in 0..MAX_TASKBUTTONS {
            let mut l = Name::new();
            l.bytes[0] = b'T';
            l.len = 1;
            assert!(t.register(WinButton { label: l, win_id: i as u16, active: false }));
        }
        assert!(!t.register(WinButton { label: Name::new(), win_id: 99, active: false }));
        // 越界坐标
        assert_eq!(t.hit(Point { x: -1, y: -1 }), None);
        let h = t.hit(Point { x: (1 * TASKBTN_W + 5) as i32, y: 5 });
        assert_eq!(h, Some(1));
        // 溢出折叠
        let mut t2 = Taskbar::new();
        for i in 0..14 {
            let mut l = Name::new();
            l.bytes[0] = b'T';
            l.len = 1;
            t2.register(WinButton { label: l, win_id: i as u16, active: false });
        }
        assert_eq!(t2.overflow_count(), 2);
        assert_eq!(t2.visible_count(), 12);
    }

    #[test]
    fn a254_activate_and_remove() {
        let mut t = Taskbar::new();
        let mut l = Name::new();
        l.bytes[0] = b'W';
        l.len = 1;
        t.register(WinButton { label: l, win_id: 10, active: false });
        t.register(WinButton { label: l, win_id: 20, active: false });
        assert!(t.activate(20));
        assert_eq!(t.active_index, Some(1));
        assert!(t.get(1).unwrap().active);
        assert!(!t.get(0).unwrap().active);
        assert!(t.remove(10));
        assert_eq!(t.len(), 1);
        // 移除的是非激活窗（索引 0），激活窗（原索引 1）前移为 Some(0)。
        assert_eq!(t.active_index, Some(0));
        assert!(t.get(0).unwrap().active);
        assert!(!t.remove(999));
    }

    #[test]
    fn a255_tray_register_hit_remove() {
        let mut tray = Tray::new();
        for i in 0..MAX_TRAY {
            let mut l = Name::new();
            l.bytes[0] = b'X';
            l.len = 1;
            assert!(tray.register(TrayItem { label: l, icon_id: i as u16, badge: i as u8 }));
        }
        assert!(!tray.register(TrayItem { label: Name::new(), icon_id: 99, badge: 0 }));
        // 空托盘命中 None
        let mut empty = Tray::new();
        assert_eq!(empty.hit(Point { x: 5, y: 5 }, 0, 0), None);
        // 命中第 4 个
        let h = tray.hit(Point { x: (4 * TRAY_ICON_W + 4) as i32, y: 4 }, 0, 0);
        assert_eq!(h, Some(4));
        assert!(tray.remove(3));
        assert_eq!(tray.len(), MAX_TRAY - 1);
    }

    #[test]
    fn a256_startmenu_query_pagination() {
        let mut m = StartMenu::new();
        let names: [&[u8]; 5] = [&b"cat"[..], &b"dog"[..], &b"cobra"[..], &b"door"[..], &b"code"[..]];
        for (i, s) in names.iter().enumerate() {
            let mut l = Name::new();
            let mut j = 0;
            while j < s.len() {
                l.bytes[j] = s[j];
                j += 1;
            }
            l.len = s.len() as u8;
            assert!(m.add(StartItem { label: l, app_id: i as u16 }));
        }
        // 前缀 "c" -> cat, cobra, code = 3
        let mut filt = Name::new();
        filt.bytes[0] = b'c';
        filt.len = 1;
        let mut out = [0usize; MAX_START_ITEMS];
        let n = m.query(&filt, 0, START_PAGE_SIZE, &mut out);
        assert_eq!(n, 3);
        // 空查询返回全部
        let mut out2 = [0usize; MAX_START_ITEMS];
        let all = m.query(&Name::new(), 0, 2, &mut out2);
        assert_eq!(all, 2);
        assert_eq!(m.page_count(&Name::new(), 2), 3);
        // 越界分页返回 0
        assert_eq!(m.query(&Name::new(), 99, 2, &mut out2), 0);
    }

    #[test]
    fn a257_shortcut_valid() {
        let ok = shortcut_make(Name::from_str("prog"), Name::from_str("-a"), Name::from_str("/x"));
        assert!(shortcut_valid(&ok));
        let (t, _a, c) = shortcut_resolve(&ok);
        assert_eq!(t.as_str(), "prog");
        assert_eq!(c.as_str(), "/x");
        let bad = shortcut_make(Name::new(), Name::new(), Name::new());
        assert!(!shortcut_valid(&bad));
    }

    #[test]
    fn a258_menu_nav_and_submenu() {
        let mut m = ContextMenu::new();
        assert_eq!(m.move_down(), false); // 空菜单
        assert_eq!(m.activate(), None);
        m.add(MenuItem { label: Name::from_str("Open"), action: MenuAction::Open, enabled: true, submenu: SUBMENU_NONE });
        m.add(MenuItem { label: Name::from_str("X"), action: MenuAction::Disabled, enabled: false, submenu: SUBMENU_NONE });
        m.add(MenuItem { label: Name::from_str("More"), action: MenuAction::Submenu, enabled: true, submenu: 3 });
        m.add(MenuItem { label: Name::from_str("Sub"), action: MenuAction::Settings, enabled: true, submenu: SUBMENU_NONE });
        // 从 0 下移：跳过禁用项(1) -> 2
        assert!(m.move_down());
        assert_eq!(m.cursor, 2);
        assert_eq!(m.enter_submenu(), Some(3));
        // 禁用项不可触发
        m.cursor = 1;
        assert_eq!(m.activate(), None);
        // 可遍历计数 = 3（跳过禁用 与 分隔符）
        assert_eq!(m.traversable_count(), 3);
    }

    #[test]
    fn a259_lock_and_autohide() {
        let mut t = Taskbar::new();
        t.set_locked(true);
        t.set_autohide(true);
        assert!(t.locked && t.autohide);
        t.set_locked(false);
        assert!(!t.locked);
    }

    #[test]
    fn a260_multiscreen_rect() {
        let r0 = taskbar_screen_rect(0, 1024, 768);
        assert_eq!(r0.y, 768 - TASKBAR_H as i32);
        let r1 = taskbar_screen_rect(1, 1024, 768);
        assert_eq!(r1.y, 0);
        assert_eq!(r0.w, 1024);
    }

    #[test]
    fn a261_badge_sum() {
        let mut t = Tray::new();
        t.register(TrayItem { label: Name::from_str("a"), icon_id: 1, badge: 2 });
        t.register(TrayItem { label: Name::from_str("b"), icon_id: 2, badge: 7 });
        assert_eq!(t.total_badge(), 9);
        assert!(t.has_badge());
        t.clear_badges();
        assert_eq!(t.total_badge(), 0);
    }

    #[test]
    fn a262_folder_path() {
        let mut out = Name::new();
        assert!(desktop_folder_path(&Name::from_str("docs"), &mut out));
        assert_eq!(out.as_str(), "desktop/docs");
        // 超长名称失败
        let mut long = Name::new();
        let mut i = 0;
        while i < NAME_CAP {
            long.bytes[i] = b'x';
            i += 1;
        }
        long.len = NAME_CAP as u8;
        assert!(!desktop_folder_path(&long, &mut out));
    }

    #[test]
    fn a263_search_entry() {
        let mut m = StartMenu::new();
        let names: [&[u8]; 3] = [&b"find"[..], &b"files"[..], &b"help"[..]];
        for (i, s) in names.iter().enumerate() {
            let mut l = Name::new();
            let mut j = 0;
            while j < s.len() {
                l.bytes[j] = s[j];
                j += 1;
            }
            l.len = s.len() as u8;
            m.add(StartItem { label: l, app_id: i as u16 });
        }
        let mut q = Name::new();
        q.bytes[0] = b'f';
        q.len = 1;
        let mut out = [0usize; MAX_START_ITEMS];
        assert_eq!(desktop_search(&m, &q, &mut out), 2);
        // 空查询返回 0 项（首页）
        assert_eq!(desktop_search(&m, &Name::new(), &mut out), 3);
    }

    #[test]
    fn a264_perf_budget() {
        let mut t = Taskbar::new();
        for i in 0..10 {
            let mut l = Name::new();
            l.bytes[0] = b'W';
            l.len = 1;
            t.register(WinButton { label: l, win_id: i as u16, active: false });
        }
        assert!(t.within_budget(2, 40));
        assert!(!t.within_budget(5, 10));
    }

    #[test]
    fn a265_tab_order() {
        let mut t = Taskbar::new();
        for i in 0..4 {
            let mut l = Name::new();
            l.bytes[0] = b'Q';
            l.len = 1;
            t.register(WinButton { label: l, win_id: i as u16, active: false });
        }
        let mut ord = [0u8; MAX_TASKBUTTONS];
        let n = t.tab_order(&mut ord);
        assert_eq!(n, 4);
        assert_eq!(ord[0], 0);
        assert_eq!(ord[3], 3);
    }

    #[test]
    fn a266_dim_due() {
        let d = Desktop::new();
        assert!(desktop_dim_due(30_000, &d));
        assert!(!desktop_dim_due(100, &d));
    }

    #[test]
    fn a267_consistency() {
        let d = Desktop::new();
        assert!(desktop_consistency(&d));
        let mut pd = Desktop::new();
        let mut l = Name::new();
        l.bytes[0] = b'P';
        l.len = 1;
        pd.icons.allocate(l, 1);
        pd.taskbar.register(WinButton { label: l, win_id: 1, active: true });
        pd.taskbar.register(WinButton { label: l, win_id: 2, active: true }); // 两个 active
        assert!(!desktop_consistency(&pd));
    }

    #[test]
    fn a270_perf_frame() {
        let d = Desktop::new();
        assert!(desktop_within_perf(&d, 16));
        assert!(!desktop_within_perf(&d, 2));
    }

    #[test]
    fn a271_metrics_render() {
        let d = Desktop::new();
        let mut buf = [0u8; 128];
        let mut n = 0usize;
        desktop_metrics(&d, &mut buf, &mut n);
        assert!(n > 0);
        let text = core::str::from_utf8(&buf[..n]).unwrap();
        assert!(text.contains("icons=0"));
    }

    #[test]
    fn a272_fuzz_invariant() {
        assert!(desktop_fuzz(0));
        assert!(desktop_fuzz(47));
        assert!(desktop_fuzz(31_415));
    }

    #[test]
    fn a273_doc_render() {
        let mut buf = [0u8; 256];
        let mut n = 0usize;
        desktop_doc(&mut buf, &mut n);
        assert!(n > 0);
        let text = core::str::from_utf8(&buf[..n]).unwrap();
        assert!(text.contains("AI-11"));
    }

    #[test]
    fn a274_degrade_chain() {
        let mut d = Desktop::new();
        assert!(desktop_degrade(DegradeLevel::Mild, &mut d));
        assert_eq!(d.dim_idle_ms, 15_000);
        assert!(desktop_degrade(DegradeLevel::Heavy, &mut d));
        assert_eq!(d.dim_idle_ms, 5_000);
        assert_eq!(d.tray.total_badge(), 0);
        assert!(desktop_consistency(&d));
    }

    #[test]
    fn a268_finalize_checks() {
        let d = Desktop::new();
        let mut set = CheckSet::new("aurora-desktop");
        desktop_finalize_checks(&d, &mut set);
        assert_eq!(set.len(), 3);
        assert!(set.all_passed());
    }

    #[test]
    fn a251_to_a275_selfcheck_all_pass() {
        let set = run_desktop_checks();
        assert_eq!(set.domain, "aurora-desktop");
        assert!(!set.truncated());
        let (passed, failed) = set.tally();
        assert_eq!(failed, 0);
        assert!(passed >= 25);
        assert!(set.all_passed());
    }
}
