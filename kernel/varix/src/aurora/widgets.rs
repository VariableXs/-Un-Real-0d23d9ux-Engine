//! AURORA-1000 AI-13 · UI 组件库（A301~A325，W2）
//!
//! 纯逻辑 UI 组件模型：统一组件树、状态机、矩形布局 pass、键盘可达与
//! 读屏角色语义。所有组件用固定容量数组 + `usize` 计数实现，`no_std`、
//! 无 `alloc`、无 `unsafe`、无宏、无泛型魔法。每个组件都是纯函数或可
//! 变结构，主机测试套件不依赖任何硬件即可完整验收本域。

use crate::checks::CheckSet;

// ===========================================================================
// 公共常量与基础类型
// ===========================================================================

/// 文本定长缓冲容量（字节）。
pub const MAX_TEXT: usize = 32;
/// 组件树容量。
pub const MAX_NODES: usize = 32;

/// 键盘扫描码（沿用 ASCII 控制码约定，便于无依赖测试）。
pub const KEY_TAB: u8 = 9;
pub const KEY_ENTER: u8 = 13;
pub const KEY_SPACE: u8 = 32;
pub const KEY_LEFT: u8 = 37;
pub const KEY_UP: u8 = 38;
pub const KEY_RIGHT: u8 = 39;
pub const KEY_DOWN: u8 = 40;

/// 滑块值域上界（含）。
pub const SLIDER_MAX: u8 = 255;
/// 进度条值域上界（含）。
pub const PROGRESS_MAX: u8 = 100;

/// 一个矩形（以左上角为原点，向右下延伸）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

/// 定长文本缓冲：`[u8; 32]` + 长度计数。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextBuf {
    pub bytes: [u8; MAX_TEXT],
    pub len: usize,
}

impl TextBuf {
    pub const fn new() -> TextBuf {
        TextBuf { bytes: [0u8; MAX_TEXT], len: 0 }
    }

    /// 从 ASCII 字节切片填充（超出容量部分截断）。
    pub fn from_ascii(s: &[u8]) -> TextBuf {
        let mut t = TextBuf::new();
        let mut i = 0usize;
        while i < s.len() && i < MAX_TEXT {
            t.bytes[i] = s[i];
            i += 1;
        }
        t.len = i;
        t
    }

    /// 追加一个字节；满容量返回 `false`。
    pub fn push(&mut self, b: u8) -> bool {
        if self.len >= MAX_TEXT {
            return false;
        }
        self.bytes[self.len] = b;
        self.len += 1;
        true
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes[..self.len]
    }
}

/// 组件视觉/交互状态机。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WState {
    Normal,
    Hover,
    Pressed,
    Disabled,
    Focused,
}

/// 读屏角色（无障碍语义标签）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Button,
    TextInput,
    List,
    Table,
    Dialog,
    Menu,
    Slider,
    ProgressBar,
    Tab,
    Selector,
    Checkbox,
    Radio,
    Tooltip,
    ScrollBar,
    Unknown,
}

/// 统一组件树节点（父子索引 + 矩形 + 角色 + 状态 + 值）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Node {
    pub role: Role,
    pub state: WState,
    pub rect: Rect,
    /// 父节点索引；`-1` 表示根。
    pub parent: i16,
    pub label: TextBuf,
    /// 通用载荷：滑块/进度条值、选择器高亮项等。
    pub value: u16,
}

impl Node {
    pub const fn new(role: Role) -> Node {
        Node {
            role,
            state: WState::Normal,
            rect: Rect { x: 0, y: 0, w: 0, h: 0 },
            parent: -1,
            label: TextBuf::new(),
            value: 0,
        }
    }
}

/// 状态机输入事件。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UiEvent {
    Hover,
    Unhover,
    Press,
    Release,
    Focus,
    Blur,
    Disable,
    Enable,
}

/// 点是否落在矩形内。
fn point_in(r: Rect, px: i32, py: i32) -> bool {
    px >= r.x && px < r.x + r.w as i32 && py >= r.y && py < r.y + r.h as i32
}

// ===========================================================================
// A301 按钮组件
// ===========================================================================

/// 命中测试：在 `state != Disabled` 且坐标落在矩形内时返回 `true`。
/// 禁用按钮不响应任何指针点击（边界防护）。
pub fn button_press(state: WState, rect: Rect, px: i32, py: i32) -> bool {
    if state == WState::Disabled {
        return false;
    }
    point_in(rect, px, py)
}

// ===========================================================================
// A302 按钮键盘触发
// ===========================================================================

/// 键盘激活：空格(32)或回车(13)触发；禁用按钮不响应。
pub fn button_key_activate(state: WState, key: u8) -> bool {
    if state == WState::Disabled {
        return false;
    }
    key == KEY_SPACE || key == KEY_ENTER
}

// ===========================================================================
// A303 文本输入框
// ===========================================================================

/// 输入框模型：文本缓冲 + 光标 + 选区。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InputModel {
    pub text: TextBuf,
    /// 光标位置 `0..=len`。
    pub cursor: usize,
    pub sel_start: usize,
    pub sel_end: usize,
}

impl InputModel {
    pub const fn new() -> InputModel {
        InputModel {
            text: TextBuf::new(),
            cursor: 0,
            sel_start: 0,
            sel_end: 0,
        }
    }
}

/// 删除当前选区（若有）。返回是否删除了内容。
fn delete_selection(im: &mut InputModel) -> bool {
    if im.sel_end <= im.sel_start {
        return false;
    }
    let a = im.sel_start;
    let b = im.sel_end;
    let removed = b - a;
    let mut i = a;
    while i + removed < im.text.len {
        im.text.bytes[i] = im.text.bytes[i + removed];
        i += 1;
    }
    im.text.len -= removed;
    im.cursor = a;
    im.sel_start = a;
    im.sel_end = a;
    true
}

/// 在光标处插入一个字符；先清空选区。容量满（32）时钳制返回 `false`。
pub fn input_insert(im: &mut InputModel, b: u8) -> bool {
    delete_selection(im);
    if im.text.len >= MAX_TEXT {
        return false;
    }
    let mut i = im.text.len;
    while i > im.cursor {
        im.text.bytes[i] = im.text.bytes[i - 1];
        i -= 1;
    }
    im.text.bytes[im.cursor] = b;
    im.text.len += 1;
    im.cursor += 1;
    im.sel_start = im.cursor;
    im.sel_end = im.cursor;
    true
}

// ===========================================================================
// A304 输入框光标移动
// ===========================================================================

/// 移动光标 `delta`（可为负）。越界钳制到 `[0, len]` 并清除选区。
pub fn input_move_cursor(im: &mut InputModel, delta: i32) {
    let c = im.cursor as i32 + delta;
    if c < 0 {
        im.cursor = 0;
    } else if c as usize > im.text.len {
        im.cursor = im.text.len;
    } else {
        im.cursor = c as usize;
    }
    im.sel_start = im.cursor;
    im.sel_end = im.cursor;
}

// ===========================================================================
// A305 输入框删除与选区
// ===========================================================================

/// 退格：先删选区，否则删除光标前一个字符；空文本/光标在首返回 `false`。
pub fn input_backspace(im: &mut InputModel) -> bool {
    if delete_selection(im) {
        return true;
    }
    if im.cursor == 0 {
        return false;
    }
    let mut i = im.cursor - 1;
    while i + 1 < im.text.len {
        im.text.bytes[i] = im.text.bytes[i + 1];
        i += 1;
    }
    im.text.len -= 1;
    im.cursor -= 1;
    im.sel_start = im.cursor;
    im.sel_end = im.cursor;
    true
}

/// 设置选区 `[start, end)`（自动排序并钳制到文本长度）。
pub fn input_set_selection(im: &mut InputModel, start: usize, end: usize) {
    let s = start.min(end).min(im.text.len);
    let e = end.max(start).min(im.text.len);
    im.sel_start = s;
    im.sel_end = e;
}

// ===========================================================================
// A306 列表单选
// ===========================================================================

/// 列表模型：总数、选中项、滚动顶、视口高度（行数）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ListModel {
    pub total: usize,
    pub selected: usize,
    pub scroll_top: usize,
    pub view: usize,
}

/// 选取单项（越界返回 `false`），并同步滚动窗口。
pub fn list_select(l: &mut ListModel, idx: usize) -> bool {
    if idx >= l.total {
        return false;
    }
    l.selected = idx;
    l.scroll_top = clamp_scroll_top(l);
    true
}

fn clamp_scroll_top(l: &ListModel) -> usize {
    if l.view == 0 || l.total <= l.view {
        return 0;
    }
    let mut top = l.scroll_top;
    if l.selected < top {
        top = l.selected;
    } else if l.selected >= top + l.view {
        top = l.selected - l.view + 1;
    }
    if top > l.total - l.view {
        top = l.total - l.view;
    }
    top
}

// ===========================================================================
// A307 列表可见窗口
// ===========================================================================

/// 计算可见区 `[top, end)`（依据选中项与视口高度做滚动钳制）。
/// 空列表返回 `(0, 0)`。
pub fn list_visible_window(l: &ListModel) -> (usize, usize) {
    let top = clamp_scroll_top(l);
    let end = (top + l.view).min(l.total);
    (top, end)
}

// ===========================================================================
// A308 表格单元格命中
// ===========================================================================

/// 表格单元格命中：按等宽等高的网格分配。`(usize::MAX, usize::MAX)`
/// 表示行/列为空或点落在表格外。
pub fn table_cell_hit(rows: usize, cols: usize, rect: Rect, px: i32, py: i32) -> (usize, usize) {
    if rows == 0 || cols == 0 || !point_in(rect, px, py) {
        return (usize::MAX, usize::MAX);
    }
    let cw = rect.w / cols as u32;
    let ch = rect.h / rows as u32;
    if cw == 0 || ch == 0 {
        return (usize::MAX, usize::MAX);
    }
    let col = ((px - rect.x) / cw as i32) as usize;
    let row = ((py - rect.y) / ch as i32) as usize;
    (row.min(rows - 1), col.min(cols - 1))
}

// ===========================================================================
// A309 表格单元选择
// ===========================================================================

/// 选择单元格（行, 列）。越界由调用方保证范围，这里直接记录。
pub fn table_select_cell(sel: &mut (usize, usize), row: usize, col: usize) {
    *sel = (row, col);
}

// ===========================================================================
// A310 对话框与模态
// ===========================================================================

/// 对话框：模态标志 + 开启标志。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Dialog {
    pub modal: bool,
    pub open: bool,
}

/// 模态且开启时阻塞底层输入（`true`）。
pub fn dialog_blocks(d: Dialog) -> bool {
    d.modal && d.open
}

// ===========================================================================
// A311 下拉菜单弹出/收起
// ===========================================================================

/// 菜单：开启标志 + 选中项 + 项数。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Menu {
    pub open: bool,
    pub selected: usize,
    pub items: usize,
}

/// 切换菜单弹出/收起；弹出时高亮项归零。
pub fn menu_toggle(m: &mut Menu) {
    m.open = !m.open;
    if m.open {
        m.selected = 0;
    }
}

// ===========================================================================
// A312 菜单项选择
// ===========================================================================

/// 选择菜单项；菜单未开启或越界返回 `false`。
pub fn menu_select_item(m: &mut Menu, idx: usize) -> bool {
    if !m.open || idx >= m.items {
        return false;
    }
    m.selected = idx;
    true
}

// ===========================================================================
// A313 上下文菜单
// ===========================================================================

/// 在屏幕内计算上下文菜单弹出矩形（防止超出屏幕边界）。
pub fn context_menu_rect(screen: Rect, at_x: i32, at_y: i32, w: u32, h: u32) -> Rect {
    let mut x = at_x;
    let mut y = at_y;
    if x + w as i32 > screen.x + screen.w as i32 {
        x = screen.x + screen.w as i32 - w as i32;
    }
    if x < screen.x {
        x = screen.x;
    }
    if y + h as i32 > screen.y + screen.h as i32 {
        y = screen.y + screen.h as i32 - h as i32;
    }
    if y < screen.y {
        y = screen.y;
    }
    Rect { x, y, w, h }
}

// ===========================================================================
// A314 滑块值映射
// ===========================================================================

/// 设置滑块值，钳制到 `0..=255`。
pub fn slider_set(v: u16) -> u8 {
    if v > SLIDER_MAX as u16 {
        SLIDER_MAX
    } else {
        v as u8
    }
}

// ===========================================================================
// A315 滑块步进
// ===========================================================================

/// 方向键步进：上/右增 `step`，下/左减 `step`，全程钳制到 `0..=255`。
pub fn slider_step(value: u8, key: u8, step: u8) -> u8 {
    match key {
        KEY_UP | KEY_RIGHT => value.saturating_add(step).min(SLIDER_MAX),
        KEY_DOWN | KEY_LEFT => value.saturating_sub(step),
        _ => value,
    }
}

// ===========================================================================
// A316 进度条
// ===========================================================================

/// 设置进度值，钳制到 `0..=100`。
pub fn progress_set(v: u16) -> u8 {
    if v > PROGRESS_MAX as u16 {
        PROGRESS_MAX
    } else {
        v as u8
    }
}

// ===========================================================================
// A317 标签页切换
// ===========================================================================

/// 标签页栏：页数和当前激活页。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TabBar {
    pub count: usize,
    pub active: usize,
}

/// 切换到指定页；越界返回 `false`。
pub fn tab_select(t: &mut TabBar, idx: usize) -> bool {
    if idx >= t.count {
        return false;
    }
    t.active = idx;
    true
}

// ===========================================================================
// A318 标签页 Ctrl+Tab 循环
// ===========================================================================

/// 环形切换页：`shift=true` 反向。空栏返回 `false`。
pub fn tab_cycle(t: &mut TabBar, shift: bool) -> bool {
    if t.count == 0 {
        return false;
    }
    t.active = if shift {
        (t.active + t.count - 1) % t.count
    } else {
        (t.active + 1) % t.count
    };
    true
}

// ===========================================================================
// A319 下拉选择器展开
// ===========================================================================

/// 选择器：展开标志 + 高亮项 + 选项数。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Selector {
    pub expanded: bool,
    pub highlight: usize,
    pub options: usize,
}

/// 展开下拉列表；高亮项越界则归零。
pub fn selector_expand(s: &mut Selector) {
    s.expanded = true;
    if s.highlight >= s.options {
        s.highlight = 0;
    }
}

/// 收起下拉列表。
pub fn selector_collapse(s: &mut Selector) {
    s.expanded = false;
}

// ===========================================================================
// A320 选择器键盘导航
// ===========================================================================

/// 键盘导航：上/下环形移动高亮；回车/空格确认（保留高亮）。
/// 未展开或空列表返回 `false`。
pub fn selector_nav(s: &mut Selector, key: u8) -> bool {
    if !s.expanded || s.options == 0 {
        return false;
    }
    match key {
        KEY_DOWN => {
            s.highlight = (s.highlight + 1) % s.options;
            true
        }
        KEY_UP => {
            s.highlight = (s.highlight + s.options - 1) % s.options;
            true
        }
        KEY_ENTER | KEY_SPACE => true,
        _ => false,
    }
}

// ===========================================================================
// A321 组件状态机
// ===========================================================================

/// 状态转移：Disabled 吞掉一切事件（除 Enable）；其余按交互规则转移。
pub fn state_transition(s: WState, e: UiEvent) -> WState {
    match (s, e) {
        (_, UiEvent::Disable) => WState::Disabled,
        (WState::Disabled, UiEvent::Enable) => WState::Normal,
        (WState::Disabled, _) => WState::Disabled,
        (WState::Focused, UiEvent::Press) => WState::Pressed,
        (_, UiEvent::Hover) => WState::Hover,
        (_, UiEvent::Unhover) => WState::Normal,
        (_, UiEvent::Press) => WState::Pressed,
        (WState::Pressed, UiEvent::Release) => WState::Normal,
        (WState::Hover, UiEvent::Release) => WState::Hover,
        (WState::Pressed, UiEvent::Focus) => WState::Focused,
        (WState::Hover, UiEvent::Focus) => WState::Focused,
        (WState::Focused, UiEvent::Release) => WState::Focused,
        (WState::Focused, UiEvent::Blur) => WState::Normal,
        (WState::Normal, UiEvent::Focus) => WState::Focused,
        (WState::Hover, UiEvent::Blur) => WState::Normal,
        (WState::Normal, UiEvent::Blur) => WState::Normal,
        // 其余组合（如 Release/Enable 在 Normal/Hover 态）保持原状态。
        (st, _) => st,
    }
}

// ===========================================================================
// A322 组件树布局 pass
// ===========================================================================

/// 布局 pass：从父矩形纵向均分 `count` 个子矩形，写入 `children` 并设置
/// 父索引。返回实际布置的子节点数（钳制到 `MAX_NODES`/非空）。
pub fn layout_children(
    parent_rect: Rect,
    parent_index: i16,
    children: &mut [Node; MAX_NODES],
    count: usize,
) -> usize {
    let c = count.min(MAX_NODES);
    if c == 0 {
        return 0;
    }
    let ch = parent_rect.h / c as u32;
    let mut i = 0usize;
    while i < c {
        children[i].rect = Rect {
            x: parent_rect.x,
            y: parent_rect.y + (i as i32) * ch as i32,
            w: parent_rect.w,
            h: ch,
        };
        children[i].parent = parent_index;
        i += 1;
    }
    c
}

// ===========================================================================
// A323 键盘遍历环
// ===========================================================================

/// Tab 顺序环形遍历：返回下一个聚焦索引。`count == 0` 时安全返回 0。
pub fn focus_cycle(current: usize, count: usize, backward: bool) -> usize {
    if count == 0 {
        return 0;
    }
    if backward {
        (current + count - 1) % count
    } else {
        (current + 1) % count
    }
}

// ===========================================================================
// A324 读屏角色标签
// ===========================================================================

/// 将角色映射为读屏可用的 ARIA 风格标签串。
pub fn role_label(r: Role) -> &'static str {
    match r {
        Role::Button => "button",
        Role::TextInput => "textbox",
        Role::List => "list",
        Role::Table => "grid",
        Role::Dialog => "dialog",
        Role::Menu => "menu",
        Role::Slider => "slider",
        Role::ProgressBar => "progressbar",
        Role::Tab => "tab",
        Role::Selector => "combobox",
        Role::Checkbox => "checkbox",
        Role::Radio => "radio",
        Role::Tooltip => "tooltip",
        Role::ScrollBar => "scrollbar",
        Role::Unknown => "unknown",
    }
}

// ===========================================================================
// A325 UI 组件库域自检收口
// ===========================================================================

/// 域自检：纯不变量，必须全部为真方可通过 `cargo ktest`。
pub fn run_widgets_checks() -> CheckSet {
    let mut set = CheckSet::new("aurora-widgets");

    // A301 按钮命中 + 禁用不响应
    let r = Rect { x: 0, y: 0, w: 100, h: 40 };
    set.add(
        "A301 button hit",
        button_press(WState::Normal, r, 50, 20) && !button_press(WState::Normal, r, -5, -5),
        "hit test",
    );
    set.add(
        "A301 disabled no press",
        !button_press(WState::Disabled, r, 50, 20),
        "disabled ignored",
    );

    // A302 按钮键盘触发
    set.add(
        "A302 key activate",
        button_key_activate(WState::Normal, KEY_SPACE)
            && button_key_activate(WState::Normal, KEY_ENTER)
            && !button_key_activate(WState::Normal, KEY_LEFT)
            && !button_key_activate(WState::Disabled, KEY_SPACE),
        "space/enter",
    );

    // A303 输入框插入 + 容量钳制
    let mut im = InputModel::new();
    let mut ok = true;
    for _ in 0..MAX_TEXT {
        ok = ok && input_insert(&mut im, b'x');
    }
    let full = im.text.len == MAX_TEXT;
    let overflow = !input_insert(&mut im, b'x') && im.text.len == MAX_TEXT;
    set.add("A303 insert build", ok && full, "fill 32");
    set.add("A303 capacity clamp", overflow, "no overflow");

    // A304 光标移动越界钳制
    let mut im = InputModel::new();
    input_insert(&mut im, b'a');
    input_insert(&mut im, b'b');
    input_move_cursor(&mut im, -100);
    let at0 = im.cursor == 0;
    input_move_cursor(&mut im, 100);
    let at_end = im.cursor == im.text.len;
    set.add("A304 cursor clamp", at0 && at_end, "bounds");

    // A305 退格删除 + 选区删除
    let mut im = InputModel::new();
    for b in [b'a', b'b', b'c', b'd'] {
        input_insert(&mut im, b);
    }
    let _ = input_backspace(&mut im); // d 删除 -> abc
    let after_bs = im.text.as_bytes() == b"abc" && im.cursor == 3;
    input_set_selection(&mut im, 1, 3); // 选中 "bc"
    let _ = input_backspace(&mut im); // -> a
    let after_sel = im.text.as_bytes() == b"a" && im.cursor == 1;
    set.add("A305 backspace", after_bs && after_sel, "edit");

    // A306 列表单选越界
    let mut l = ListModel { total: 4, selected: 0, scroll_top: 0, view: 2 };
    let ok_sel = list_select(&mut l, 2) && l.selected == 2;
    let bad_sel = !list_select(&mut l, 9);
    set.add("A306 list select", ok_sel && bad_sel, "single");

    // A307 列表可见窗口（空 + 滚动钳制）
    let empty = ListModel { total: 0, selected: 0, scroll_top: 0, view: 5 };
    let ew = list_visible_window(&empty);
    let big = ListModel { total: 10, selected: 9, scroll_top: 0, view: 3 };
    let bw = list_visible_window(&big);
    set.add("A307 visible window", ew == (0, 0) && bw == (7, 10), "scroll clamp");

    // A308 表格命中 / 越界
    let tr = Rect { x: 0, y: 0, w: 300, h: 400 };
    let inside = table_cell_hit(4, 3, tr, 150, 250) == (2, 1);
    let outside = table_cell_hit(4, 3, tr, -5, -5) == (usize::MAX, usize::MAX);
    let zero = table_cell_hit(0, 3, tr, 10, 10) == (usize::MAX, usize::MAX);
    set.add("A308 table hit", inside && outside && zero, "cell hit");

    // A309 表格单元选择
    let mut cell = (0usize, 0usize);
    table_select_cell(&mut cell, 3, 2);
    set.add("A309 table select", cell == (3, 2), "cell select");

    // A310 对话框模态阻塞
    set.add(
        "A310 dialog modal",
        dialog_blocks(Dialog { modal: true, open: true })
            && !dialog_blocks(Dialog { modal: false, open: true })
            && !dialog_blocks(Dialog { modal: true, open: false }),
        "modal block",
    );

    // A311 菜单弹出/收起
    let mut m = Menu { open: false, selected: 3, items: 2 };
    menu_toggle(&mut m);
    let opened = m.open && m.selected == 0;
    menu_toggle(&mut m);
    let closed = !m.open;
    set.add("A311 menu toggle", opened && closed, "popup");

    // A312 菜单项选择（关闭时拒绝）
    let mut m = Menu { open: false, selected: 0, items: 3 };
    let closed_rej = !menu_select_item(&mut m, 1);
    m.open = true;
    let opened_ok = menu_select_item(&mut m, 2) && m.selected == 2;
    set.add("A312 menu select", closed_rej && opened_ok, "item select");

    // A313 上下文菜单边界钳制
    let screen = Rect { x: 0, y: 0, w: 800, h: 600 };
    let cm = context_menu_rect(screen, 780, 580, 200, 100);
    set.add(
        "A313 context menu",
        cm.x == 600 && cm.y == 500 && cm.w == 200 && cm.h == 100,
        "clamp to screen",
    );

    // A314 滑块值映射钳制
    set.add(
        "A314 slider set",
        slider_set(100) == 100 && slider_set(300) == SLIDER_MAX && slider_set(0) == 0,
        "0..=255",
    );

    // A315 滑块步进
    set.add(
        "A315 slider step",
        slider_step(10, KEY_UP, 5) == 15
            && slider_step(250, KEY_UP, 10) == SLIDER_MAX
            && slider_step(0, KEY_LEFT, 5) == 0
            && slider_step(10, KEY_TAB, 5) == 10,
        "step clamp",
    );

    // A316 进度条钳制
    set.add(
        "A316 progress",
        progress_set(50) == 50 && progress_set(150) == PROGRESS_MAX && progress_set(0) == 0,
        "0..=100",
    );

    // A317 标签页切换
    let mut t = TabBar { count: 4, active: 0 };
    let ok_tab = tab_select(&mut t, 2) && t.active == 2;
    let bad_tab = !tab_select(&mut t, 9);
    set.add("A317 tab select", ok_tab && bad_tab, "switch");

    // A318 标签页 Ctrl+Tab 循环
    let mut t = TabBar { count: 3, active: 0 };
    let _ = tab_cycle(&mut t, false);
    let fwd = t.active == 1;
    let mut t2 = TabBar { count: 3, active: 0 };
    let _ = tab_cycle(&mut t2, true);
    let back = t2.active == 2;
    let mut t3 = TabBar { count: 0, active: 0 };
    let empty = !tab_cycle(&mut t3, false) && t3.active == 0;
    set.add("A318 tab cycle", fwd && back && empty, "ring");

    // A319 选择器展开
    let mut s = Selector { expanded: false, highlight: 5, options: 3 };
    selector_expand(&mut s);
    set.add(
        "A319 selector expand",
        s.expanded && s.highlight == 0,
        "expand reset",
    );

    // A320 选择器键盘导航
    let mut s = Selector { expanded: true, highlight: 0, options: 3 };
    let down = selector_nav(&mut s, KEY_DOWN) && s.highlight == 1;
    let up = selector_nav(&mut s, KEY_UP) && s.highlight == 0;
    let mut sc = Selector { expanded: false, highlight: 0, options: 3 };
    let closed = !selector_nav(&mut sc, KEY_DOWN);
    set.add("A320 selector nav", down && up && closed, "nav ring");

    // A321 状态机
    set.add(
        "A321 state machine",
        state_transition(WState::Normal, UiEvent::Press) == WState::Pressed
            && state_transition(WState::Pressed, UiEvent::Release) == WState::Normal
            && state_transition(WState::Disabled, UiEvent::Press) == WState::Disabled
            && state_transition(WState::Normal, UiEvent::Disable) == WState::Disabled
            && state_transition(WState::Disabled, UiEvent::Enable) == WState::Normal,
        "transitions",
    );

    // A322 组件树布局
    let parent = Rect { x: 0, y: 0, w: 100, h: 200 };
    let mut kids = [Node::new(Role::Button); MAX_NODES];
    let placed = layout_children(parent, -1, &mut kids, 4);
    set.add(
        "A322 layout pass",
        placed == 4 && kids[3].rect.y == 150 && kids[0].rect.h == 50 && kids[0].parent == -1,
        "alloc children",
    );

    // A323 键盘遍历环
    set.add(
        "A323 focus ring",
        focus_cycle(0, 3, false) == 1
            && focus_cycle(2, 3, true) == 1
            && focus_cycle(0, 0, false) == 0,
        "tab order",
    );

    // A324 读屏角色标签
    set.add(
        "A324 role label",
        role_label(Role::Button) == "button"
            && role_label(Role::Slider) == "slider"
            && role_label(Role::Dialog) == "dialog",
        "a11y role",
    );

    // A325 自检收口登记
    set.add(
        "A325 self-check registry",
        set.domain == "aurora-widgets" && set.len() >= 25,
        "domain tag",
    );

    set
}

// ===========================================================================
// 单元测试
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a301_button_hit_and_disabled() {
        let r = Rect { x: 10, y: 10, w: 80, h: 30 };
        assert!(button_press(WState::Normal, r, 50, 20));
        assert!(!button_press(WState::Normal, r, 0, 0));
        assert!(!button_press(WState::Disabled, r, 50, 20));
    }

    #[test]
    fn a302_button_keyboard() {
        assert!(button_key_activate(WState::Normal, KEY_SPACE));
        assert!(button_key_activate(WState::Normal, KEY_ENTER));
        assert!(!button_key_activate(WState::Normal, KEY_TAB));
        assert!(!button_key_activate(WState::Disabled, KEY_ENTER));
    }

    #[test]
    fn a303_input_insert_and_capacity() {
        let mut im = InputModel::new();
        for _ in 0..MAX_TEXT {
            assert!(input_insert(&mut im, b'z'));
        }
        assert_eq!(im.text.len, MAX_TEXT);
        assert!(!input_insert(&mut im, b'z'));
        assert_eq!(im.text.len, MAX_TEXT);
    }

    #[test]
    fn a303_input_insert_middle() {
        let mut im = InputModel::new();
        input_insert(&mut im, b'a');
        input_insert(&mut im, b'b'); // "ab" cursor=2
        input_move_cursor(&mut im, -2); // cursor=0
        assert!(input_insert(&mut im, b'X'));
        assert_eq!(im.text.as_bytes(), b"Xab");
        assert_eq!(im.cursor, 1);
    }

    #[test]
    fn a304_cursor_move_clamp() {
        let mut im = InputModel::new();
        input_insert(&mut im, b'h');
        input_insert(&mut im, b'i');
        input_move_cursor(&mut im, -99);
        assert_eq!(im.cursor, 0);
        input_move_cursor(&mut im, 99);
        assert_eq!(im.cursor, im.text.len);
    }

    #[test]
    fn a305_backspace_and_selection() {
        let mut im = InputModel::new();
        for b in [b'a', b'b', b'c'] {
            input_insert(&mut im, b);
        }
        assert!(input_backspace(&mut im));
        assert_eq!(im.text.as_bytes(), b"ab");
        assert_eq!(im.cursor, 2);
        // 空选区 + 光标在首：不能再删
        input_move_cursor(&mut im, -99);
        assert!(!input_backspace(&mut im));
        // 选区删除
        let mut im2 = InputModel::new();
        for b in [b'a', b'b', b'c', b'd'] {
            input_insert(&mut im2, b);
        }
        input_set_selection(&mut im2, 1, 3);
        assert!(input_backspace(&mut im2));
        assert_eq!(im2.text.as_bytes(), b"ad");
    }

    #[test]
    fn a306_list_single_select() {
        let mut l = ListModel { total: 5, selected: 0, scroll_top: 0, view: 3 };
        assert!(list_select(&mut l, 4));
        assert_eq!(l.selected, 4);
        assert!(!list_select(&mut l, 5));
    }

    #[test]
    fn a307_list_visible_window() {
        let empty = ListModel { total: 0, selected: 0, scroll_top: 0, view: 4 };
        assert_eq!(list_visible_window(&empty), (0, 0));
        let mut l = ListModel { total: 12, selected: 0, scroll_top: 0, view: 4 };
        assert_eq!(list_visible_window(&l), (0, 4));
        l.selected = 11;
        assert_eq!(list_visible_window(&l), (8, 12));
    }

    #[test]
    fn a308_table_cell_hit() {
        let tr = Rect { x: 0, y: 0, w: 300, h: 400 };
        assert_eq!(table_cell_hit(4, 3, tr, 150, 250), (2, 1));
        assert_eq!(table_cell_hit(4, 3, tr, -1, -1), (usize::MAX, usize::MAX));
        assert_eq!(table_cell_hit(0, 3, tr, 1, 1), (usize::MAX, usize::MAX));
    }

    #[test]
    fn a309_table_select_cell() {
        let mut cell = (0usize, 0usize);
        table_select_cell(&mut cell, 5, 7);
        assert_eq!(cell, (5, 7));
    }

    #[test]
    fn a310_dialog_modal_block() {
        assert!(dialog_blocks(Dialog { modal: true, open: true }));
        assert!(!dialog_blocks(Dialog { modal: false, open: true }));
        assert!(!dialog_blocks(Dialog { modal: true, open: false }));
    }

    #[test]
    fn a311_menu_toggle() {
        let mut m = Menu { open: false, selected: 2, items: 3 };
        menu_toggle(&mut m);
        assert!(m.open && m.selected == 0);
        menu_toggle(&mut m);
        assert!(!m.open);
    }

    #[test]
    fn a312_menu_select_item() {
        let mut m = Menu { open: false, selected: 0, items: 3 };
        assert!(!menu_select_item(&mut m, 1));
        m.open = true;
        assert!(menu_select_item(&mut m, 2));
        assert_eq!(m.selected, 2);
        assert!(!menu_select_item(&mut m, 9));
    }

    #[test]
    fn a313_context_menu_clamp() {
        let screen = Rect { x: 0, y: 0, w: 800, h: 600 };
        let cm = context_menu_rect(screen, 700, 560, 200, 100);
        assert_eq!(cm, Rect { x: 600, y: 500, w: 200, h: 100 });
        let cm2 = context_menu_rect(screen, 10, 10, 200, 100);
        assert_eq!(cm2, Rect { x: 10, y: 10, w: 200, h: 100 });
    }

    #[test]
    fn a314_slider_set_clamp() {
        assert_eq!(slider_set(0), 0);
        assert_eq!(slider_set(128), 128);
        assert_eq!(slider_set(255), 255);
        assert_eq!(slider_set(1000), SLIDER_MAX);
    }

    #[test]
    fn a315_slider_step() {
        assert_eq!(slider_step(10, KEY_UP, 5), 15);
        assert_eq!(slider_step(250, KEY_UP, 10), SLIDER_MAX);
        assert_eq!(slider_step(0, KEY_LEFT, 5), 0);
        assert_eq!(slider_step(20, KEY_RIGHT, 1), 21);
        assert_eq!(slider_step(20, KEY_TAB, 5), 20);
    }

    #[test]
    fn a316_progress_clamp() {
        assert_eq!(progress_set(0), 0);
        assert_eq!(progress_set(75), 75);
        assert_eq!(progress_set(255), PROGRESS_MAX);
    }

    #[test]
    fn a317_tab_select() {
        let mut t = TabBar { count: 4, active: 0 };
        assert!(tab_select(&mut t, 3));
        assert_eq!(t.active, 3);
        assert!(!tab_select(&mut t, 4));
    }

    #[test]
    fn a318_tab_cycle() {
        let mut t = TabBar { count: 3, active: 0 };
        assert!(tab_cycle(&mut t, false));
        assert_eq!(t.active, 1);
        assert!(tab_cycle(&mut t, false));
        assert_eq!(t.active, 2);
        assert!(tab_cycle(&mut t, false)); // 2 -> 0 wrap
        assert_eq!(t.active, 0);
        assert!(tab_cycle(&mut t, true)); // 0 -> 2 wrap back
        assert_eq!(t.active, 2);
        let mut e = TabBar { count: 0, active: 0 };
        assert!(!tab_cycle(&mut e, false));
    }

    #[test]
    fn a319_selector_expand() {
        let mut s = Selector { expanded: false, highlight: 9, options: 2 };
        selector_expand(&mut s);
        assert!(s.expanded);
        assert_eq!(s.highlight, 0);
        selector_collapse(&mut s);
        assert!(!s.expanded);
    }

    #[test]
    fn a320_selector_nav() {
        let mut s = Selector { expanded: true, highlight: 0, options: 3 };
        assert!(selector_nav(&mut s, KEY_DOWN));
        assert_eq!(s.highlight, 1);
        assert!(selector_nav(&mut s, KEY_DOWN));
        assert_eq!(s.highlight, 2);
        assert!(selector_nav(&mut s, KEY_DOWN)); // wrap -> 0
        assert_eq!(s.highlight, 0);
        assert!(selector_nav(&mut s, KEY_UP)); // 0 -> 2 wrap
        assert_eq!(s.highlight, 2);
        let mut closed = Selector { expanded: false, highlight: 0, options: 3 };
        assert!(!selector_nav(&mut closed, KEY_DOWN));
    }

    #[test]
    fn a321_state_transitions() {
        assert_eq!(state_transition(WState::Normal, UiEvent::Hover), WState::Hover);
        assert_eq!(state_transition(WState::Hover, UiEvent::Press), WState::Pressed);
        assert_eq!(state_transition(WState::Pressed, UiEvent::Release), WState::Normal);
        assert_eq!(state_transition(WState::Normal, UiEvent::Focus), WState::Focused);
        assert_eq!(state_transition(WState::Focused, UiEvent::Blur), WState::Normal);
        assert_eq!(state_transition(WState::Disabled, UiEvent::Press), WState::Disabled);
        assert_eq!(state_transition(WState::Disabled, UiEvent::Enable), WState::Normal);
    }

    #[test]
    fn a322_layout_children() {
        let parent = Rect { x: 10, y: 20, w: 120, h: 240 };
        let mut kids = [Node::new(Role::List); MAX_NODES];
        let n = layout_children(parent, 7, &mut kids, 4);
        assert_eq!(n, 4);
        assert_eq!(kids[0].rect, Rect { x: 10, y: 20, w: 120, h: 60 });
        assert_eq!(kids[3].rect.y, 20 + 180);
        assert_eq!(kids[0].parent, 7);
        // 空布局返回 0
        let mut none = [Node::new(Role::List); MAX_NODES];
        assert_eq!(layout_children(parent, 0, &mut none, 0), 0);
    }

    #[test]
    fn a323_focus_ring() {
        assert_eq!(focus_cycle(0, 3, false), 1);
        assert_eq!(focus_cycle(2, 3, false), 0);
        assert_eq!(focus_cycle(0, 3, true), 2);
        assert_eq!(focus_cycle(0, 0, false), 0);
    }

    #[test]
    fn a324_role_labels() {
        assert_eq!(role_label(Role::Button), "button");
        assert_eq!(role_label(Role::TextInput), "textbox");
        assert_eq!(role_label(Role::List), "list");
        assert_eq!(role_label(Role::Table), "grid");
        assert_eq!(role_label(Role::Slider), "slider");
        assert_eq!(role_label(Role::ProgressBar), "progressbar");
        assert_eq!(role_label(Role::Selector), "combobox");
        assert!(role_label(Role::Unknown).len() > 0);
    }

    #[test]
    fn a325_self_check_runs() {
        let set = run_widgets_checks();
for i in 0..set.len() { if let Some(c) = set.get(i) { if !c.passed { println!("DIAG a325 fail: {} : {}", c.name, c.detail); } } }
        assert_eq!(set.domain, "aurora-widgets");
        assert!(set.len() >= 25);
        assert!(set.all_passed(), "all widget checks must pass");
        let (passed, failed) = set.tally();
        assert_eq!(failed, 0, "no failed checks");
        assert_eq!(passed, set.len());
    }

    #[test]
    fn a325_textbuf_from_ascii() {
        let t = TextBuf::from_ascii(b"hello");
        assert_eq!(t.as_bytes(), b"hello");
        assert_eq!(t.len, 5);
        let long = TextBuf::from_ascii(b"this string is way longer than thirty-two bytes for sure");
        assert_eq!(long.len, MAX_TEXT);
    }
}
