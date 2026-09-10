//! TRINITY-500 · AI-07 UI 组件库域（F151~F175，W2）
//!
//! 依赖 AI-05（合成）、AI-06（文本）、AI-08（令牌）。
//! 纪律：所有交互判定为**纯整数**运算（命中/滚动/布局），保证 QEMU 与真机手感一致；
//! 组件自检（F170）逐项登记在 `run_widget_checks()`。

use crate::checks::CheckSet;
use crate::gfx::surface::Rect;

// ---------------------------------------------------------------------------
// F151 窗口框架 — 标题栏/红绿灯
// ---------------------------------------------------------------------------

pub const TITLE_BAR_H: i32 = 32;
pub const TRAFFIC_LIGHT_W: i32 = 12;
pub const TRAFFIC_LIGHT_GAP: i32 = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrafficLight {
    None = 0,
    Close = 1,
    Minimise = 2,
    Maximise = 3,
}

impl TrafficLight {
    pub fn from_x(local_x: i32, mirrored: bool) -> TrafficLight {
        let idx = local_x / (TRAFFIC_LIGHT_W + TRAFFIC_LIGHT_GAP);
        let idx = if mirrored { 2 - idx } else { idx };
        match idx {
            0 => TrafficLight::Close,
            1 => TrafficLight::Minimise,
            2 => TrafficLight::Maximise,
            _ => TrafficLight::None,
        }
    }

    /// 红绿灯默认排在左侧（可变），命中判定必须跟着排布走。
    pub fn origin_x(&self, w: i32, mirrored: bool, index: i32) -> i32 {
        let slot = TRAFFIC_LIGHT_W + TRAFFIC_LIGHT_GAP;
        if mirrored {
            w - 12 - (2 - index) * slot
        } else {
            12 + index * slot
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct WindowFrame {
    pub rect: Rect,
    pub resizable: bool,
    pub focused: bool,
}

impl WindowFrame {
    pub fn title_bar(&self) -> Rect {
        Rect::new(self.rect.x, self.rect.y, self.rect.w, TITLE_BAR_H)
    }

    /// 命中标题栏（拖动窗口）；命中边框 4px（缩放窗口）。
    pub fn hit_zone(&self, x: i32, y: i32) -> WindowZone {
        if !point_in(&self.rect, x, y) {
            return WindowZone::Outside;
        }
        if point_in(&self.title_bar(), x, y) {
            return WindowZone::TitleBar;
        }
        let border = 4;
        let in_x = x >= self.rect.x + border && x < self.rect.right() - border;
        let in_y = y >= self.rect.y + border && y < self.rect.bottom() - border;
        if self.resizable && (!in_x || !in_y) {
            return WindowZone::Border;
        }
        WindowZone::Client
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowZone {
    Outside,
    TitleBar,
    Border,
    Client,
}

pub fn point_in(r: &Rect, x: i32, y: i32) -> bool {
    x >= r.x && y >= r.y && x < r.right() && y < r.bottom()
}

// ---------------------------------------------------------------------------
// F152 按钮/图标按钮
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ButtonState {
    Normal,
    Hover,
    Pressed,
    Disabled,
}

impl ButtonState {
    /// 状态迁移表：禁用态不接受任何输入。
    pub fn transition(self, enabled: bool, hover: bool, pressed: bool) -> ButtonState {
        if !enabled {
            return ButtonState::Disabled;
        }
        if pressed {
            ButtonState::Pressed
        } else if hover {
            ButtonState::Hover
        } else {
            ButtonState::Normal
        }
    }

    /// 各状态的底色透明度（permille）——统一的按压反馈。
    pub fn fill_alpha(self) -> u16 {
        match self {
            ButtonState::Normal => 0,
            ButtonState::Hover => 80,
            ButtonState::Pressed => 160,
            ButtonState::Disabled => 40,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Button {
    pub rect: Rect,
    pub icon_only: bool,
    pub state: ButtonState,
}

impl Button {
    /// 图标按钮的最小热区 32×32（触控可达性军规）。
    pub fn min_hotspot(icon_only: bool) -> i32 {
        if icon_only {
            32
        } else {
            24
        }
    }

    pub fn hit(&self, x: i32, y: i32) -> bool {
        point_in(&self.rect, x, y) && self.state != ButtonState::Disabled
    }
}

// ---------------------------------------------------------------------------
// F153 文本输入框
// ---------------------------------------------------------------------------

pub const INPUT_MAX: usize = 128;

pub struct Input {
    buf: [u8; INPUT_MAX],
    len: usize,
    pub cursor: usize,
    pub selection: (usize, usize),
    pub password: bool,
}

impl Input {
    pub const fn new() -> Input {
        Input { buf: [0u8; INPUT_MAX], len: 0, cursor: 0, selection: (0, 0), password: false }
    }

    pub fn text(&self) -> &str {
        core::str::from_utf8(&self.buf[..self.len]).unwrap_or("")
    }

    /// 插入一个 UTF-8 字符（光标在中间时也能插）。
    pub fn insert(&mut self, ch: char) -> bool {
        let mut enc = [0u8; 4];
        let s = ch.encode_utf8(&mut enc);
        let n = s.len();
        if self.len + n > INPUT_MAX {
            return false;
        }
        self.buf[self.cursor..].rotate_right(n);
        self.buf[self.cursor..self.cursor + n].copy_from_slice(s.as_bytes());
        self.len += n;
        self.cursor += n;
        true
    }

    /// 退格：删除光标前一个字符（按 UTF-8 边界回退）。
    pub fn backspace(&mut self) -> bool {
        if self.cursor == 0 {
            return false;
        }
        let mut start = self.cursor - 1;
        while start > 0 && (self.buf[start] & 0xC0) == 0x80 {
            start -= 1;
        }
        self.buf[start..self.len].rotate_left(self.cursor - start);
        self.len -= self.cursor - start;
        self.cursor = start;
        true
    }

    /// 显示串：密码框返回等长圆点（不返回真值）。
    pub fn display(&self, out: &mut [u8]) -> usize {
        if self.password {
            let dots = self.text().chars().count();
            let n = dots.min(out.len());
            for b in out.iter_mut().take(n) {
                *b = b'*';
            }
            return n;
        }
        let n = self.len.min(out.len());
        out[..n].copy_from_slice(&self.buf[..n]);
        n
    }
}

impl Default for Input {
    fn default() -> Self {
        Input::new()
    }
}

// ---------------------------------------------------------------------------
// F154 滚动条/滚动容器
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
pub struct ScrollBar {
    pub track: i32,
    pub viewport: i32,
    pub content: i32,
    pub offset: i32,
}

impl ScrollBar {
    pub fn max_offset(&self) -> i32 {
        (self.content - self.viewport).max(0)
    }

    pub fn clamp(&mut self) {
        let max = self.max_offset();
        if self.offset < 0 {
            self.offset = 0;
        }
        if self.offset > max {
            self.offset = max;
        }
    }

    /// 滚动条滑块：内容不足一屏时隐藏（返回空矩形）。
    pub fn thumb(&self) -> Rect {
        if self.content <= self.viewport || self.viewport <= 0 {
            return Rect::new(0, 0, 0, 0);
        }
        let len = (self.viewport * self.track) / self.content;
        let len = len.max(16);
        let travel = self.track - len;
        let pos = if self.max_offset() == 0 {
            0
        } else {
            (self.offset * travel) / self.max_offset()
        };
        Rect::new(0, pos, 4, len)
    }

    /// 惯性滚动：每帧衰减 12%（整数运算，避免浮点漂移）。
    pub fn fling(&mut self, velocity: i32) {
        self.offset += velocity;
        self.clamp();
    }

    pub fn decay(v: i32) -> i32 {
        (v * 88) / 100
    }
}

// ---------------------------------------------------------------------------
// F155 列表/树形列表
// ---------------------------------------------------------------------------

pub const MAX_TREE: usize = 32;
pub const MAX_DEPTH: usize = 4;

#[derive(Clone, Copy, Debug)]
pub struct TreeNode {
    pub label: &'static str,
    pub depth: u8,
    pub expanded: bool,
    pub has_children: bool,
}

/// 展平为可见行（折叠节点的子树不出现）。
pub fn flatten_visible(nodes: &[TreeNode], out: &mut [usize]) -> usize {
    let mut n = 0usize;
    let mut hide_below: Option<u8> = None;
    for (i, node) in nodes.iter().enumerate() {
        if let Some(d) = hide_below {
            if node.depth > d {
                continue;
            }
            hide_below = None;
        }
        if n < out.len() {
            out[n] = i;
            n += 1;
        }
        if node.has_children && !node.expanded {
            hide_below = Some(node.depth);
        }
    }
    n
}

pub fn indent_px(depth: u8, step: i32) -> i32 {
    (depth.min(MAX_DEPTH as u8) as i32) * step
}

// ---------------------------------------------------------------------------
// F156 标签页
// ---------------------------------------------------------------------------

pub const MAX_TABS: usize = 8;

pub struct Tabs {
    labels: [Option<&'static str>; MAX_TABS],
    count: usize,
    pub active: usize,
}

impl Tabs {
    pub const fn new() -> Tabs {
        Tabs { labels: [None; MAX_TABS], count: 0, active: 0 }
    }

    pub fn add(&mut self, label: &'static str) -> bool {
        if self.count >= MAX_TABS {
            return false;
        }
        self.labels[self.count] = Some(label);
        self.count += 1;
        true
    }

    pub fn len(&self) -> usize {
        self.count
    }

    /// 命中第几个标签（等宽布局）。
    pub fn hit(&self, x: i32, width: i32) -> Option<usize> {
        if self.count == 0 || width <= 0 {
            return None;
        }
        let w = width / self.count as i32;
        let idx = x / w;
        if idx >= 0 && (idx as usize) < self.count {
            Some(idx as usize)
        } else {
            None
        }
    }

    pub fn select(&mut self, i: usize) -> bool {
        if i >= self.count {
            return false;
        }
        self.active = i;
        true
    }
}

impl Default for Tabs {
    fn default() -> Self {
        Tabs::new()
    }
}

// ---------------------------------------------------------------------------
// F157 右键菜单/上下文菜单
// ---------------------------------------------------------------------------

pub const MAX_MENU_ITEMS: usize = 12;
pub const MENU_ITEM_H: i32 = 28;

pub struct ContextMenu {
    items: [Option<&'static str>; MAX_MENU_ITEMS],
    count: usize,
    pub open: bool,
    pub x: i32,
    pub y: i32,
    pub active: usize,
}

impl ContextMenu {
    pub const fn new() -> ContextMenu {
        ContextMenu { items: [None; MAX_MENU_ITEMS], count: 0, open: false, x: 0, y: 0, active: 0 }
    }

    pub fn set_items(&mut self, items: &[&'static str]) {
        self.count = 0;
        self.active = 0;
        for it in items.iter() {
            if self.count < MAX_MENU_ITEMS {
                self.items[self.count] = Some(it);
                self.count += 1;
            }
        }
    }

    pub fn open_at(&mut self, x: i32, y: i32) {
        self.open = self.count > 0;
        self.x = x;
        self.y = y;
        self.active = 0;
    }

    pub fn close(&mut self) {
        self.open = false;
    }

    pub fn height(&self) -> i32 {
        self.count as i32 * MENU_ITEM_H
    }

    /// 命中项（相对菜单原点）。
    pub fn hit(&self, x: i32, y: i32) -> Option<usize> {
        if !self.open {
            return None;
        }
        let local_y = y - self.y;
        if x < self.x || local_y < 0 || local_y >= self.height() {
            return None;
        }
        Some((local_y / MENU_ITEM_H) as usize)
    }

    pub fn move_active(&mut self, delta: i32) {
        if self.count == 0 {
            return;
        }
        let cur = self.active as i32 + delta;
        if cur < 0 {
            self.active = self.count - 1;
        } else if cur >= self.count as i32 {
            self.active = 0;
        } else {
            self.active = cur as usize;
        }
    }
}

impl Default for ContextMenu {
    fn default() -> Self {
        ContextMenu::new()
    }
}

// ---------------------------------------------------------------------------
// F158 弹窗/模态
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
pub struct Modal {
    pub rect: Rect,
    pub modal: bool,
    pub open: bool,
}

impl Modal {
    /// 模态开启时，矩形外的输入一律被吞掉（不穿透到下层窗口）。
    pub fn blocks(&self, x: i32, y: i32) -> bool {
        self.open && self.modal && !point_in(&self.rect, x, y)
    }

    /// 遮罩透明度（permille）。
    pub fn scrim_alpha(&self) -> u16 {
        if self.open && self.modal {
            400
        } else {
            0
        }
    }
}

// ---------------------------------------------------------------------------
// F159 通知横幅
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BannerLevel {
    Info,
    Warning,
    Error,
}

impl BannerLevel {
    pub fn default_timeout_ms(&self) -> u32 {
        match self {
            BannerLevel::Info => 4_000,
            BannerLevel::Warning => 6_000,
            // 错误不自动消失：必须让用户看见（如实，不替用户做决定）。
            BannerLevel::Error => 0,
        }
    }
}

pub fn banner_expired(level: BannerLevel, shown_ms: u32) -> bool {
    let t = level.default_timeout_ms();
    t != 0 && shown_ms >= t
}

// ---------------------------------------------------------------------------
// F160 下拉/选择器
// ---------------------------------------------------------------------------

pub const MAX_OPTIONS: usize = 16;

pub struct Select {
    options: [Option<&'static str>; MAX_OPTIONS],
    count: usize,
    pub selected: usize,
    pub open: bool,
}

impl Select {
    pub const fn new() -> Select {
        Select { options: [None; MAX_OPTIONS], count: 0, selected: 0, open: false }
    }

    pub fn add(&mut self, option: &'static str) -> bool {
        if self.count >= MAX_OPTIONS {
            return false;
        }
        self.options[self.count] = Some(option);
        self.count += 1;
        true
    }

    pub fn pick(&mut self, i: usize) -> bool {
        if i >= self.count {
            return false;
        }
        self.selected = i;
        self.open = false;
        true
    }

    pub fn value(&self) -> &'static str {
        self.options[self.selected].unwrap_or("")
    }

    /// 展开后的行命中（行高 28）。
    pub fn hit_row(&self, local_y: i32) -> Option<usize> {
        if !self.open || local_y < 0 {
            return None;
        }
        let i = (local_y / 28) as usize;
        if i < self.count {
            Some(i)
        } else {
            None
        }
    }
}

impl Default for Select {
    fn default() -> Self {
        Select::new()
    }
}

// ---------------------------------------------------------------------------
// F161 滑块/开关
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
pub struct Slider {
    pub min: i32,
    pub max: i32,
    pub value: i32,
    pub width: i32,
}

impl Slider {
    /// 位置 → 值的整数换算（全程 permille，避免浮点）。
    pub fn value_at(&self, x: i32) -> i32 {
        if self.width <= 0 {
            return self.min;
        }
        let t = ((x * 1000) / self.width).clamp(0, 1000);
        self.min + ((self.max - self.min) * t) / 1000
    }

    /// 值 → 滑块中心的 x。
    pub fn knob_x(&self) -> i32 {
        if self.max <= self.min {
            return 0;
        }
        ((self.value - self.min) * self.width) / (self.max - self.min)
    }

    /// 键盘步进：步长 = 量程的 1%（至少 1）。
    pub fn step(&mut self, delta: i32) {
        let step = ((self.max - self.min) / 100).max(1);
        self.value = (self.value + delta * step).clamp(self.min, self.max);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Toggle {
    pub on: bool,
}

impl Toggle {
    /// 开关动画时长（毫秒），reduce-motion 下为 0。
    pub fn anim_ms(&self, reduced: bool) -> u32 {
        if reduced {
            0
        } else {
            160
        }
    }
}

// ---------------------------------------------------------------------------
// F162 进度条/加载态
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
pub struct Progress {
    pub permille: u16,
    pub indeterminate: bool,
}

impl Progress {
    /// 未知进度必须显示不确定态——不允许把「不知道」伪装成 50%。
    pub fn fill_permille(&self) -> u16 {
        if self.indeterminate {
            0
        } else {
            self.permille.min(1000)
        }
    }

    /// 不确定态的循环相位（0..1000），由时间驱动。
    pub fn phase(elapsed_ms: u32) -> u16 {
        ((elapsed_ms % 1200) * 1000 / 1200) as u16
    }
}

// ---------------------------------------------------------------------------
// F163 工具栏/侧栏
// ---------------------------------------------------------------------------

pub const MAX_TOOLBAR: usize = 12;

pub struct Toolbar {
    items: [Option<&'static str>; MAX_TOOLBAR],
    count: usize,
    pub active: usize,
}

impl Toolbar {
    pub const fn new() -> Toolbar {
        Toolbar { items: [None; MAX_TOOLBAR], count: 0, active: 0 }
    }

    pub fn add(&mut self, item: &'static str) -> bool {
        if self.count >= MAX_TOOLBAR {
            return false;
        }
        self.items[self.count] = Some(item);
        self.count += 1;
        true
    }

    /// 命中（等宽 40px）。
    pub fn hit(&self, x: i32) -> Option<usize> {
        let i = x / 40;
        if i >= 0 && (i as usize) < self.count {
            Some(i as usize)
        } else {
            None
        }
    }

    pub fn activate(&mut self, i: usize) -> bool {
        if i >= self.count {
            return false;
        }
        self.active = i;
        true
    }
}

impl Default for Toolbar {
    fn default() -> Self {
        Toolbar::new()
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Sidebar {
    pub width: i32,
    pub collapsed: bool,
    pub min_width: i32,
}

impl Sidebar {
    pub fn effective_width(&self) -> i32 {
        if self.collapsed {
            0
        } else {
            self.width.max(self.min_width)
        }
    }
}

// ---------------------------------------------------------------------------
// F164 表格
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
pub struct Table {
    pub rows: usize,
    pub cols: usize,
    pub cell_w: i32,
    pub cell_h: i32,
    pub header_h: i32,
}

impl Table {
    /// 命中单元格；命中表头返回 `Some((row = usize::MAX, col))`。
    pub fn hit(&self, x: i32, y: i32) -> Option<(usize, usize)> {
        if y < 0 {
            return None;
        }
        if y < self.header_h {
            let col = (x / self.cell_w) as usize;
            return if col < self.cols { Some((usize::MAX, col)) } else { None };
        }
        let row = ((y - self.header_h) / self.cell_h) as usize;
        let col = (x / self.cell_w) as usize;
        if row < self.rows && col < self.cols {
            Some((row, col))
        } else {
            None
        }
    }
}

// ---------------------------------------------------------------------------
// F165 拖拽与放置
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DragPhase {
    Idle,
    Armed,
    Dragging,
    Dropped,
    Cancelled,
}

#[derive(Clone, Copy, Debug)]
pub struct DragState {
    pub phase: DragPhase,
    pub start_x: i32,
    pub start_y: i32,
    pub payload: u32,
}

/// 超过阈值才算拖拽（避免点击被误判为拖动）。
pub const DRAG_THRESHOLD: i32 = 4;

impl DragState {
    pub const fn new() -> DragState {
        DragState { phase: DragPhase::Idle, start_x: 0, start_y: 0, payload: 0 }
    }

    pub fn press(&mut self, x: i32, y: i32, payload: u32) {
        self.phase = DragPhase::Armed;
        self.start_x = x;
        self.start_y = y;
        self.payload = payload;
    }

    pub fn move_to(&mut self, x: i32, y: i32) {
        if self.phase != DragPhase::Armed {
            return;
        }
        if (x - self.start_x).abs() >= DRAG_THRESHOLD || (y - self.start_y).abs() >= DRAG_THRESHOLD {
            self.phase = DragPhase::Dragging;
        }
    }

    pub fn drop(&mut self) -> Option<u32> {
        if self.phase == DragPhase::Dragging {
            self.phase = DragPhase::Dropped;
            Some(self.payload)
        } else {
            self.phase = DragPhase::Idle;
            None
        }
    }

    /// Esc 取消拖拽。
    pub fn cancel(&mut self) {
        if matches!(self.phase, DragPhase::Armed | DragPhase::Dragging) {
            self.phase = DragPhase::Cancelled;
        }
    }
}

impl Default for DragState {
    fn default() -> Self {
        DragState::new()
    }
}

// ---------------------------------------------------------------------------
// F166 焦点环/键盘导航
// ---------------------------------------------------------------------------

pub const MAX_FOCUS: usize = 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FocusKind {
    /// 键盘可见焦点环（Tab 导航时出现）。
    Ring,
    /// 指针交互不画环（避免"鼠标点了就出现蓝框"）。
    None,
}

pub struct FocusRing {
    order: [u16; MAX_FOCUS],
    count: usize,
    pub index: usize,
    pub kind: FocusKind,
}

impl FocusRing {
    pub const fn new() -> FocusRing {
        FocusRing { order: [0; MAX_FOCUS], count: 0, index: 0, kind: FocusKind::None }
    }

    pub fn register(&mut self, id: u16) -> bool {
        if self.count >= MAX_FOCUS {
            return false;
        }
        self.order[self.count] = id;
        self.count += 1;
        true
    }

    /// Tab / Shift+Tab 环形导航。
    pub fn advance(&mut self, forward: bool) -> Option<u16> {
        if self.count == 0 {
            return None;
        }
        self.kind = FocusKind::Ring;
        if forward {
            self.index = (self.index + 1) % self.count;
        } else if self.index == 0 {
            self.index = self.count - 1;
        } else {
            self.index -= 1;
        }
        Some(self.order[self.index])
    }

    pub fn focused(&self) -> Option<u16> {
        if self.count == 0 {
            None
        } else {
            Some(self.order[self.index])
        }
    }
}

impl Default for FocusRing {
    fn default() -> Self {
        FocusRing::new()
    }
}

// ---------------------------------------------------------------------------
// F167 无障碍语义 — 等价 aria 标注
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Button,
    Textbox,
    Checkbox,
    Slider,
    Tab,
    MenuItem,
    Dialog,
    ListItem,
    Banner,
    ProgressBar,
}

impl Role {
    /// 等价 ARIA role 字符串（用于与 Tauri 版对齐审计）。
    pub fn aria(&self) -> &'static str {
        match self {
            Role::Button => "button",
            Role::Textbox => "textbox",
            Role::Checkbox => "checkbox",
            Role::Slider => "slider",
            Role::Tab => "tab",
            Role::MenuItem => "menuitem",
            Role::Dialog => "dialog",
            Role::ListItem => "listitem",
            Role::Banner => "status",
            Role::ProgressBar => "progressbar",
        }
    }

    /// 哪些角色必须有可读名称（aria-label 等价物）。
    pub fn requires_name(&self) -> bool {
        !matches!(self, Role::Banner)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct A11yNode {
    pub role: Role,
    pub label: &'static str,
    pub disabled: bool,
    pub value_permille: u16,
}

impl A11yNode {
    /// 无障碍树节点是否合规：需名的必须有名。
    pub fn valid(&self) -> bool {
        !self.role.requires_name() || !self.label.is_empty()
    }
}

// ---------------------------------------------------------------------------
// F168 主题令牌（tokens 对齐）
// F169 组件状态机
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
pub struct ThemeTokens {
    pub bg: u32,
    pub fg: u32,
    pub accent: u32,
    pub radius: i32,
    pub gap: i32,
}

impl ThemeTokens {
    /// 默认暗色令牌（与 AI-08 的视觉令牌同源）。
    pub const fn dark() -> ThemeTokens {
        ThemeTokens { bg: 0xFF0B_0D12, fg: 0xFFE8_EAF0, accent: 0xFF7A_A2F7, radius: 8, gap: 8 }
    }

    /// 对比度必须达 WCAG AA（4.5:1）才允许出厂。
    pub fn contrast_ok(&self) -> bool {
        crate::gfx::surface::contrast_ratio(self.fg, self.bg) >= 4.5
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WidgetState {
    Normal,
    Hover,
    Pressed,
    Focused,
    Disabled,
}

/// 组件状态机：合法迁移表（不允许从 Disabled 直接跳到 Pressed）。
pub fn state_transition(from: WidgetState, event: WidgetEvent) -> WidgetState {
    match (from, event) {
        (WidgetState::Disabled, _) => WidgetState::Disabled,
        (_, WidgetEvent::Disable) => WidgetState::Disabled,
        (_, WidgetEvent::Enable) => WidgetState::Normal,
        (_, WidgetEvent::PointerEnter) => WidgetState::Hover,
        (WidgetState::Pressed, WidgetEvent::PointerLeave) => WidgetState::Normal,
        (_, WidgetEvent::PointerLeave) => WidgetState::Normal,
        (_, WidgetEvent::Press) => WidgetState::Pressed,
        (WidgetState::Pressed, WidgetEvent::Release) => WidgetState::Hover,
        (_, WidgetEvent::Release) => WidgetState::Normal,
        (_, WidgetEvent::Focus) => WidgetState::Focused,
        (WidgetState::Focused, WidgetEvent::Blur) => WidgetState::Normal,
        (_, WidgetEvent::Blur) => WidgetState::Normal,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WidgetEvent {
    PointerEnter,
    PointerLeave,
    Press,
    Release,
    Focus,
    Blur,
    Enable,
    Disable,
}

// ---------------------------------------------------------------------------
// F171 组件热区命中测试
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WidgetKind {
    Window,
    Button,
    Input,
    Scroll,
    List,
    Tabs,
    Menu,
    Modal,
    Banner,
    Select,
    Slider,
    Toggle,
    Progress,
    Toolbar,
    Table,
}

impl WidgetKind {
    pub fn name(self) -> &'static str {
        match self {
            WidgetKind::Window => "window",
            WidgetKind::Button => "button",
            WidgetKind::Input => "input",
            WidgetKind::Scroll => "scroll",
            WidgetKind::List => "list",
            WidgetKind::Tabs => "tabs",
            WidgetKind::Menu => "menu",
            WidgetKind::Modal => "modal",
            WidgetKind::Banner => "banner",
            WidgetKind::Select => "select",
            WidgetKind::Slider => "slider",
            WidgetKind::Toggle => "toggle",
            WidgetKind::Progress => "progress",
            WidgetKind::Toolbar => "toolbar",
            WidgetKind::Table => "table",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Widget {
    pub id: u16,
    pub kind: WidgetKind,
    pub rect: Rect,
    pub enabled: bool,
    pub visible: bool,
}

/// 顶层优先命中：从后往前扫（后面的画在上面）。
pub fn hit_test(widgets: &[Widget], x: i32, y: i32) -> Option<Widget> {
    widgets
        .iter()
        .rev()
        .find(|w| w.visible && w.enabled && point_in(&w.rect, x, y))
        .copied()
}

// ---------------------------------------------------------------------------
// F172 组件进入/退出动效
// F173 组件性能预算
// ---------------------------------------------------------------------------

/// 进入/退出曲线：进入 180ms 淡入 + 4px 上移，退出 120ms 淡出。
pub fn enter_exit(entering: bool, elapsed_ms: u32, reduced: bool) -> (u16, i32) {
    if reduced {
        return (if entering { 1000 } else { 0 }, 0);
    }
    let total = if entering { 180 } else { 120 };
    let t = (elapsed_ms.min(total) * 1000) / total;
    let alpha = if entering { t } else { 1000 - t };
    let offset = if entering { (4 * (1000 - t) as i32) / 1000 } else { 0 };
    (alpha as u16, offset)
}

pub const WIDGET_BUDGET_US: u32 = 2_000;

/// 组件树每帧预算 2ms（渲染 4ms + 合成 10ms ≈ 16ms）。
pub fn widget_budget_ok(widgets: u32, us_per_widget: u32) -> bool {
    (widgets as u64) * (us_per_widget as u64) <= WIDGET_BUDGET_US as u64
}

// ---------------------------------------------------------------------------
// F174 组件用法文档
// ---------------------------------------------------------------------------

/// 每个组件的用法一句话（仓库内可查，避免文档漂移）。
pub fn usage(kind: WidgetKind) -> &'static str {
    match kind {
        WidgetKind::Window => "WindowFrame::hit_zone() 决定拖动/缩放/客户区",
        WidgetKind::Button => "Button::hit() + ButtonState::transition() 驱动按压反馈",
        WidgetKind::Input => "Input::insert/backspace 维持 UTF-8 边界；password 只显 *",
        WidgetKind::Scroll => "ScrollBar::thumb() 内容不足一屏时返回空矩形",
        WidgetKind::List => "flatten_visible() 折叠子树后再布局",
        WidgetKind::Tabs => "Tabs::hit(x, width) 等宽命中，select() 切页",
        WidgetKind::Menu => "ContextMenu::open_at() 后 hit() 返回行号",
        WidgetKind::Modal => "Modal::blocks() 吞掉矩形外的输入",
        WidgetKind::Banner => "banner_expired() 对 Error 永不超时",
        WidgetKind::Select => "Select::hit_row() 用 28px 行高展开命中",
        WidgetKind::Slider => "Slider::value_at() 全程 permille 整数换算",
        WidgetKind::Toggle => "Toggle::anim_ms(reduced) 尊重 reduce-motion",
        WidgetKind::Progress => "indeterminate 时 fill_permille() 返回 0，不伪造",
        WidgetKind::Toolbar => "Toolbar::hit(x) 等宽 40px",
        WidgetKind::Table => "Table::hit() 命中表头时 row = usize::MAX",
    }
}

// ---------------------------------------------------------------------------
// F170 / F175 组件自检与收口
// ---------------------------------------------------------------------------

/// AI-07 域自检：F151~F175 逐项登记。
pub fn run_widget_checks() -> CheckSet {
    let mut set = CheckSet::new("widget");

    let wf = WindowFrame { rect: Rect::new(0, 0, 200, 120), resizable: true, focused: true };
    set.add(
        "F151 window frame",
        wf.hit_zone(10, 10) == WindowZone::TitleBar
            && wf.hit_zone(10, 60) == WindowZone::Client
            && wf.hit_zone(0, 60) == WindowZone::Border
            && wf.hit_zone(-1, 0) == WindowZone::Outside
            && TrafficLight::from_x(0, false) == TrafficLight::Close,
        "title bar + border + traffic lights",
    );

    set.add(
        "F152 button",
        ButtonState::Normal.transition(true, true, false) == ButtonState::Hover
            && ButtonState::Normal.transition(true, false, true) == ButtonState::Pressed
            && ButtonState::Normal.transition(false, false, false) == ButtonState::Disabled
            && Button::min_hotspot(true) == 32,
        "state machine + hotspot",
    );

    let mut input = Input::new();
    input.insert('a');
    input.insert('中');
    input.backspace();
    let mut disp = [0u8; 16];
    let dn = input.display(&mut disp);
    set.add(
        "F153 text input",
        input.text() == "a" && dn == 1 && {
            input.password = true;
            input.display(&mut disp) == 1 && disp[0] == b'*'
        },
        "utf8 boundaries + password masking",
    );

    let mut sb = ScrollBar { track: 100, viewport: 50, content: 200, offset: 0 };
    sb.offset = 999;
    sb.clamp();
    set.add(
        "F154 scrollbar",
        sb.offset == 150 && !sb.thumb().is_empty() && ScrollBar { content: 40, ..sb }.thumb().is_empty(),
        "clamp + thumb visibility",
    );

    let tree = [
        TreeNode { label: "a", depth: 0, expanded: true, has_children: true },
        TreeNode { label: "b", depth: 1, expanded: false, has_children: false },
        TreeNode { label: "c", depth: 0, expanded: false, has_children: false },
    ];
    let mut vis = [0usize; 8];
    set.add(
        "F155 tree list",
        flatten_visible(&tree, &mut vis) == 3 && indent_px(2, 16) == 32,
        "flatten + indent",
    );

    let mut tabs = Tabs::new();
    tabs.add("one");
    tabs.add("two");
    set.add("F156 tabs", tabs.hit(150, 300) == Some(1) && tabs.select(1) && tabs.active == 1, "equal-width hit");

    let mut menu = ContextMenu::new();
    menu.set_items(&["cut", "copy"]);
    menu.open_at(10, 10);
    set.add(
        "F157 context menu",
        menu.open && menu.hit(20, 40) == Some(1) && menu.hit(5, 40).is_none() && {
            menu.move_active(1);
            menu.active == 1
        },
        "open + hit + keyboard",
    );

    let modal = Modal { rect: Rect::new(0, 0, 100, 100), modal: true, open: true };
    set.add(
        "F158 modal",
        modal.blocks(200, 200) && !modal.blocks(10, 10) && modal.scrim_alpha() == 400,
        "input capture",
    );

    set.add(
        "F159 banner",
        banner_expired(BannerLevel::Info, 5_000) && !banner_expired(BannerLevel::Error, u32::MAX),
        "error never auto-dismisses",
    );

    let mut sel = Select::new();
    sel.add("x");
    sel.add("y");
    sel.open = true;
    let second_row = sel.hit_row(30);
    let out_of_range = sel.hit_row(28 * 5);
    set.add(
        "F160 select",
        second_row == Some(1) && out_of_range.is_none() && sel.pick(1) && !sel.open && sel.value() == "y",
        "row hit + pick",
    );

    let slider = Slider { min: 0, max: 100, value: 0, width: 200 };
    set.add(
        "F161 slider/toggle",
        slider.value_at(100) == 50 && Slider { value: 50, ..slider }.knob_x() == 100,
        "permille conversion",
    );

    let prog = Progress { permille: 500, indeterminate: true };
    set.add(
        "F162 progress",
        prog.fill_permille() == 0 && Progress { indeterminate: false, ..prog }.fill_permille() == 500,
        "indeterminate is honest",
    );

    let mut tb = Toolbar::new();
    tb.add("a");
    let sb2 = Sidebar { width: 10, collapsed: true, min_width: 200 };
    set.add(
        "F163 toolbar/sidebar",
        tb.hit(10) == Some(0) && tb.activate(0) && sb2.effective_width() == 0,
        "hit + collapse",
    );

    let table = Table { rows: 3, cols: 2, cell_w: 50, cell_h: 20, header_h: 20 };
    set.add(
        "F164 table",
        table.hit(60, 5) == Some((usize::MAX, 1)) && table.hit(10, 30) == Some((0, 0)) && table.hit(10, 500).is_none(),
        "header vs cell",
    );

    let mut drag = DragState::new();
    drag.press(0, 0, 7);
    drag.move_to(1, 1);
    let no_drop = drag.drop();
    drag.press(0, 0, 7);
    drag.move_to(20, 0);
    set.add(
        "F165 drag and drop",
        no_drop.is_none() && drag.phase == DragPhase::Dragging && drag.drop() == Some(7),
        "threshold before dragging",
    );

    let mut ring = FocusRing::new();
    ring.register(10);
    ring.register(20);
    set.add(
        "F166 focus ring",
        ring.advance(true) == Some(20) && ring.kind == FocusKind::Ring && ring.advance(false) == Some(10),
        "tab order",
    );

    let node = A11yNode { role: Role::Button, label: "save", disabled: false, value_permille: 0 };
    set.add(
        "F167 a11y semantics",
        node.valid() && Role::Button.aria() == "button" && !A11yNode { label: "", ..node }.valid(),
        "aria equivalence",
    );

    set.add("F168 theme tokens", ThemeTokens::dark().contrast_ok(), "WCAG AA contrast");

    set.add(
        "F169 widget state machine",
        state_transition(WidgetState::Normal, WidgetEvent::Press) == WidgetState::Pressed
            && state_transition(WidgetState::Disabled, WidgetEvent::Press) == WidgetState::Disabled
            && state_transition(WidgetState::Pressed, WidgetEvent::Release) == WidgetState::Hover,
        "legal transitions",
    );

    set.add("F170 widget self-check", set.all_passed(), "entry point");

    let ws = [
        Widget { id: 1, kind: WidgetKind::Window, rect: Rect::new(0, 0, 100, 100), enabled: true, visible: true },
        Widget { id: 2, kind: WidgetKind::Button, rect: Rect::new(10, 10, 20, 20), enabled: true, visible: true },
    ];
    set.add(
        "F171 hit testing",
        hit_test(&ws, 15, 15).map(|w| w.id) == Some(2) && hit_test(&ws, 90, 90).map(|w| w.id) == Some(1),
        "topmost wins",
    );

    set.add(
        "F172 enter/exit motion",
        enter_exit(true, 90, false).0 == 500 && enter_exit(true, 200, false).0 == 1000 && enter_exit(true, 0, true).0 == 1000,
        "curve honours reduce-motion",
    );

    set.add(
        "F173 widget perf budget",
        widget_budget_ok(100, 20) && !widget_budget_ok(200, 20),
        "2ms per frame",
    );

    set.add("F174 usage docs", !usage(WidgetKind::Table).is_empty() && usage(WidgetKind::Modal).contains("blocks"), "in-repo docs");

    set.add("F175 widget domain closure", set.all_passed(), "all above green");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f151_traffic_light_mirrors() {
        assert_eq!(TrafficLight::from_x(0, true), TrafficLight::Maximise);
        assert_eq!(TrafficLight::from_x(0, false), TrafficLight::Close);
        let tl = TrafficLight::Close;
        assert!(tl.origin_x(100, false, 0) < tl.origin_x(100, true, 0));
    }

    #[test]
    fn f153_input_handles_multibyte() {
        let mut i = Input::new();
        assert!(i.insert('中'));
        assert!(i.insert('文'));
        assert_eq!(i.text(), "中文");
        assert!(i.backspace());
        assert_eq!(i.text(), "中");
        assert!(i.backspace());
        assert_eq!(i.text(), "");
        assert!(!i.backspace());
    }

    #[test]
    fn f153_input_respects_capacity() {
        let mut i = Input::new();
        for _ in 0..INPUT_MAX + 10 {
            i.insert('a');
        }
        assert_eq!(i.text().len(), INPUT_MAX);
    }

    #[test]
    fn f154_scroll_decay_converges() {
        let mut v = 1000;
        let mut steps = 0;
        while v != 0 && steps < 200 {
            v = ScrollBar::decay(v);
            steps += 1;
        }
        assert!(steps < 100, "inertia must settle");
    }

    #[test]
    fn f155_collapsed_subtree_hidden() {
        let tree = [
            TreeNode { label: "root", depth: 0, expanded: false, has_children: true },
            TreeNode { label: "kid", depth: 1, expanded: false, has_children: false },
        ];
        let mut out = [0usize; 8];
        assert_eq!(flatten_visible(&tree, &mut out), 1);
    }

    #[test]
    fn f161_slider_bounds() {
        let s = Slider { min: 10, max: 20, value: 10, width: 0 };
        assert_eq!(s.value_at(50), 10, "zero width cannot divide");
        let mut s2 = Slider { min: 0, max: 1000, value: 0, width: 100 };
        s2.step(1);
        assert_eq!(s2.value, 10);
    }

    #[test]
    fn f165_drag_cancel() {
        let mut d = DragState::new();
        d.press(0, 0, 1);
        d.move_to(50, 50);
        d.cancel();
        assert_eq!(d.phase, DragPhase::Cancelled);
        assert!(d.drop().is_none());
    }

    #[test]
    fn f167_all_roles_have_aria() {
        let roles = [Role::Button, Role::Textbox, Role::Checkbox, Role::Slider, Role::Tab, Role::MenuItem, Role::Dialog, Role::ListItem, Role::Banner, Role::ProgressBar];
        for r in roles.iter() {
            assert!(!r.aria().is_empty());
        }
    }

    #[test]
    fn f175_domain_self_test_is_green() {
        let set = run_widget_checks();
        assert_eq!(set.len(), 25);
        assert!(set.all_passed(), "widget domain self-test must pass");
    }
}
