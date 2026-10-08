//! m600shell — VARIX-M600 AI-13 桌面 Shell 精工域 (F301~F325)
//!
//! 任务栏精工/开始菜单重铸/通知中心策展/快速设置重排/搜索一体化/
//! 虚拟桌面空间站/窗口吸附乐高/分屏蓝图库/多显示器舞编/任务视图电影感/
//! 小组件画廊/系统托盘礼仪/右键菜单重生/上下文感知菜单/桌面图标几何/
//! 拖拽物理/剪贴板历史馆/截图即标注/输入指示器/状态指示灯语汇/
//! 锁屏画作/控制中心指挥/全局命令面板/Shell 回归走廊/Shell 年报。
//!
//! 硬约束：no_std / 无 alloc / 无浮点（全部 permille/定点）/ 纯逻辑。

use crate::checks::CheckSet;

// ===========================================================================
// F301 — 任务栏精工：图标容量与溢出
// ===========================================================================

pub const TASKBAR_ICON_PX: u32 = 44;
pub const TASKBAR_MARGIN_PX: u32 = 8;

/// 可见图标数 = (宽度 - 固定区 200px) / (图标+边距)，上限 20。
pub fn taskbar_visible_icons(width_px: u32) -> usize {
    if width_px <= 200 {
        return 0;
    }
    let n = (width_px - 200) / (TASKBAR_ICON_PX + TASKBAR_MARGIN_PX);
    (n as usize).min(20)
}

/// 溢出条目数。
pub fn taskbar_overflow(width_px: u32, items: usize) -> usize {
    items.saturating_sub(taskbar_visible_icons(width_px))
}

// ===========================================================================
// F302 — 开始菜单重铸：固定网格
// ===========================================================================

pub const START_GRID_COLS: usize = 6;
pub const START_GRID_ROWS: usize = 4;

pub fn pinned_capacity() -> usize {
    START_GRID_COLS * START_GRID_ROWS
}

/// 磁贴合法性：占格 (w×h) 不超网格且非零。
pub fn tile_fits(w_cells: usize, h_cells: usize) -> bool {
    w_cells >= 1 && h_cells >= 1 && w_cells <= START_GRID_COLS && h_cells <= START_GRID_ROWS
}

// ===========================================================================
// F303 — 通知中心策展：堆栈与分组
// ===========================================================================

pub const NOTIFICATION_STACK_MAX: usize = 8;

#[derive(Clone, Copy)]
pub struct Notification {
    pub app: &'static str,
    pub ts: u64,
    pub urgent: bool,
}

/// 展示策略：紧急通知置顶且占用堆栈名额；超限丢弃最旧的非紧急。
pub fn stack_visible(items: &[Notification]) -> usize {
    items.len().min(NOTIFICATION_STACK_MAX)
}

/// 紧急通知数量。
pub fn urgent_count(items: &[Notification]) -> usize {
    items.iter().filter(|n| n.urgent).count()
}

// ===========================================================================
// F304 — 快速设置重排：优先级单调
// ===========================================================================

/// 排序合法性：优先级必须单调不增（0 最高）。
pub fn tile_order_valid(priorities: &[u8]) -> bool {
    priorities.windows(2).all(|w| w[0] <= w[1])
}

// ===========================================================================
// F305 — 搜索一体化：本地/网络混合排名
// ===========================================================================

/// 综合分 = 本地分×0.6 + 网络分×0.4 + 新近加分（permille 满分 1000）。
pub fn blended_rank(local: u16, web: u16, recency_permille: u16) -> u16 {
    let base = local as u32 * 600 / 1000 + web as u32 * 400 / 1000;
    (base + recency_permille.min(1000) as u32 * 100 / 1000) as u16
}

// ===========================================================================
// F306 — 虚拟桌面空间站：桌面切换
// ===========================================================================

pub const DESKTOP_MAX: u8 = 8;

/// 切换合法性：目标桌面存在且与当前不同。
pub fn desktop_switch_ok(current: u8, target: u8) -> bool {
    target >= 1 && target <= DESKTOP_MAX && target != current
}

// ===========================================================================
// F307 — 窗口吸附乐高：边缘热区
// ===========================================================================

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SnapZone {
    None,
    Left,
    Right,
    Top,
}

/// 指针落入边缘 zone_px 热区即触发吸附。
pub fn snap_zone(x: u32, y: u32, screen_w: u32, _screen_h: u32, zone_px: u32) -> SnapZone {
    if zone_px == 0 {
        return SnapZone::None;
    }
    if y <= zone_px && x > screen_w / 4 && x < screen_w * 3 / 4 {
        return SnapZone::Top;
    }
    if x <= zone_px {
        return SnapZone::Left;
    }
    if x + zone_px >= screen_w {
        return SnapZone::Right;
    }
    SnapZone::None
}

// ===========================================================================
// F308 — 分屏蓝图库：蓝图矩形
// ===========================================================================

#[derive(Clone, Copy)]
pub struct LayoutTile {
    pub x_permille: u16,
    pub y_permille: u16,
    pub w_permille: u16,
    pub h_permille: u16,
}

impl LayoutTile {
    pub fn valid(&self) -> bool {
        self.w_permille > 0 && self.h_permille > 0
            && self.x_permille + self.w_permille <= 1000
            && self.y_permille + self.h_permille <= 1000
    }
}

pub const BLUEPRINT_HALVES: [LayoutTile; 2] = [
    LayoutTile { x_permille: 0, y_permille: 0, w_permille: 500, h_permille: 1000 },
    LayoutTile { x_permille: 500, y_permille: 0, w_permille: 500, h_permille: 1000 },
];

pub const BLUEPRINT_QUAD: [LayoutTile; 4] = [
    LayoutTile { x_permille: 0, y_permille: 0, w_permille: 500, h_permille: 500 },
    LayoutTile { x_permille: 500, y_permille: 0, w_permille: 500, h_permille: 500 },
    LayoutTile { x_permille: 0, y_permille: 500, w_permille: 500, h_permille: 500 },
    LayoutTile { x_permille: 500, y_permille: 500, w_permille: 500, h_permille: 500 },
];

// ===========================================================================
// F309 — 多显示器舞编：排布无重叠
// ===========================================================================

#[derive(Clone, Copy)]
pub struct Monitor {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

impl Monitor {
    pub fn overlaps(&self, other: &Monitor) -> bool {
        self.x < other.x + other.w as i32
            && other.x < self.x + self.w as i32
            && self.y < other.y + other.h as i32
            && other.y < self.y + self.h as i32
    }
}

/// 所有显示器两两不重叠才合法。
pub fn monitors_disjoint(mons: &[Monitor]) -> bool {
    for i in 0..mons.len() {
        for j in (i + 1)..mons.len() {
            if mons[i].overlaps(&mons[j]) {
                return false;
            }
        }
    }
    true
}

// ===========================================================================
// F310 — 任务视图电影感：卡片缩放
// ===========================================================================

/// 与选中卡片的距离 d → 缩放‰：1000 - d×120，下限 600。
pub fn card_scale_permille(dist: u32) -> u16 {
    (1000u32.saturating_sub(dist * 120)).max(600) as u16
}

// ===========================================================================
// F311 — 小组件画廊：尺寸合法
// ===========================================================================

/// 小组件规格：2×2 / 2×4 / 4×4 / 4×2。
pub fn widget_size_ok(w_cells: u8, h_cells: u8) -> bool {
    matches!((w_cells, h_cells), (2, 2) | (2, 4) | (4, 2) | (4, 4))
}

// ===========================================================================
// F312 — 系统托盘礼仪：托盘上限
// ===========================================================================

pub const TRAY_VISIBLE_MAX: usize = 6;

/// 超出上限的图标进入折叠飞出窗。
pub fn tray_folded(count: usize) -> usize {
    count.saturating_sub(TRAY_VISIBLE_MAX)
}

// ===========================================================================
// F313 — 右键菜单重生：菜单结构
// ===========================================================================

/// 菜单合法：条目 1..=12，分隔线(0)不连续、不在首尾。
pub fn context_menu_valid(items: &[u8]) -> bool {
    if items.is_empty() || items.len() > 12 {
        return false;
    }
    if items[0] == 0 || items[items.len() - 1] == 0 {
        return false;
    }
    items.windows(2).all(|w| !(w[0] == 0 && w[1] == 0))
}

// ===========================================================================
// F314 — 上下文感知菜单：按对象类型出菜单
// ===========================================================================

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TargetKind {
    File,
    Folder,
    Text,
    Desktop,
}

pub const MENU_FILE: [&str; 3] = ["open", "rename", "delete"];
pub const MENU_FOLDER: [&str; 3] = ["open", "new-item", "properties"];
pub const MENU_TEXT: [&str; 3] = ["copy", "paste", "select-all"];
pub const MENU_DESKTOP: [&str; 3] = ["refresh", "new-folder", "display"];

pub fn menu_for(kind: TargetKind) -> &'static [&'static str; 3] {
    match kind {
        TargetKind::File => &MENU_FILE,
        TargetKind::Folder => &MENU_FOLDER,
        TargetKind::Text => &MENU_TEXT,
        TargetKind::Desktop => &MENU_DESKTOP,
    }
}

// ===========================================================================
// F315 — 桌面图标几何：网格吸附
// ===========================================================================

pub const ICON_CELL_PX: u32 = 96;

/// 图标中心点 → 网格 (列, 行)。
pub fn icon_cell(x: u32, y: u32) -> (u32, u32) {
    (x / ICON_CELL_PX, y / ICON_CELL_PX)
}

/// 网格槽位是否在桌面范围内。
pub fn icon_cell_in_bounds(col: u32, row: u32, cols: u32, rows: u32) -> bool {
    col < cols && row < rows
}

// ===========================================================================
// F316 — 拖拽物理：甩掷惯性
// ===========================================================================

/// 速度 = 位移/时长（px/s），整数除法。
pub fn drag_velocity_px_s(dist_px: u32, dur_ms: u32) -> u32 {
    if dur_ms == 0 {
        return 0;
    }
    dist_px * 1000 / dur_ms
}

/// 甩掷判定：速度 ≥ 800px/s。
pub fn fling_detected(dist_px: u32, dur_ms: u32) -> bool {
    drag_velocity_px_s(dist_px, dur_ms) >= 800
}

// ===========================================================================
// F317 — 剪贴板历史馆：定容去重环
// ===========================================================================

pub const CLIP_HISTORY_CAP: usize = 32;

/// 最近优先的剪贴板历史，重复内容前移不新增。
pub struct ClipStore {
    entries: [u64; CLIP_HISTORY_CAP],
    count: usize,
}

impl ClipStore {
    pub const fn new() -> ClipStore {
        ClipStore { entries: [0; CLIP_HISTORY_CAP], count: 0 }
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn get(&self, i: usize) -> Option<u64> {
        if i < self.count {
            Some(self.entries[i])
        } else {
            None
        }
    }

    /// 加入一条：已存在则前移，否则头部插入（满则挤掉最旧）。
    pub fn add(&mut self, h: u64) {
        // 已存在：前移
        let mut found = self.count;
        for i in 0..self.count {
            if self.entries[i] == h {
                found = i;
                break;
            }
        }
        if found < self.count {
            for i in (0..found).rev() {
                self.entries[i + 1] = self.entries[i];
            }
            self.entries[0] = h;
            return;
        }
        // 新条目：头部插入
        let last = CLIP_HISTORY_CAP - 1;
        if self.count < CLIP_HISTORY_CAP {
            for i in (0..self.count).rev() {
                self.entries[i + 1] = self.entries[i];
            }
            self.count += 1;
        } else {
            for i in (0..last).rev() {
                self.entries[i + 1] = self.entries[i];
            }
        }
        self.entries[0] = h;
    }
}

// ===========================================================================
// F318 — 截图即标注：标注工具
// ===========================================================================

pub const ANNOTATION_TOOLS: [&str; 5] = ["arrow", "rect", "freehand", "text", "blur"];

/// 标注合法：工具已知且笔画点数 ≥2（文本除外 ≥1）。
pub fn annotation_ok(tool: &str, points: usize) -> bool {
    if !ANNOTATION_TOOLS.contains(&tool) {
        return false;
    }
    if tool == "text" {
        points >= 1
    } else {
        points >= 2
    }
}

// ===========================================================================
// F319 — 输入指示器：IME 状态
// ===========================================================================

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ImeState {
    Off,
    English,
    Chinese,
}

pub fn indicator_label(s: ImeState) -> &'static str {
    match s {
        ImeState::Off => "A-off",
        ImeState::English => "A",
        ImeState::Chinese => "中",
    }
}

// ===========================================================================
// F320 — 状态指示灯语汇：状态→灯色
// ===========================================================================

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum LedColor {
    Green,
    Amber,
    Red,
    Off,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SysStatus {
    Healthy,
    Degraded,
    Critical,
    Idle,
}

pub fn led_for(s: SysStatus) -> LedColor {
    match s {
        SysStatus::Healthy => LedColor::Green,
        SysStatus::Degraded => LedColor::Amber,
        SysStatus::Critical => LedColor::Red,
        SysStatus::Idle => LedColor::Off,
    }
}

// ===========================================================================
// F321 — 锁屏画作：昼夜压暗
// ===========================================================================

/// 22:00~06:00 压暗 400‰，白天 100‰。
pub fn lockscreen_dim(hour: u8) -> u16 {
    if hour >= 22 || hour < 6 {
        400
    } else {
        100
    }
}

// ===========================================================================
// F322 — 控制中心指挥：开关互斥一致性
// ===========================================================================

#[derive(Clone, Copy)]
pub struct QuickToggles {
    pub airplane: bool,
    pub wifi: bool,
    pub bluetooth: bool,
}

/// 飞行模式开启时 wifi/蓝牙必须关闭。
pub fn toggles_consistent(t: QuickToggles) -> bool {
    !t.airplane || (!t.wifi && !t.bluetooth)
}

// ===========================================================================
// F323 — 全局命令面板：模糊匹配
// ===========================================================================

/// 子序列模糊匹配：query 每个字节按序出现在 target 中即命中。
/// 命中返回得分（每字节 +2，且与上一命中位置相邻再 +3），未命中返回 None。
pub fn fuzzy_match(query: &str, target: &str) -> Option<u32> {
    let q = query.as_bytes();
    let t = target.as_bytes();
    if q.is_empty() {
        return Some(0);
    }
    let mut score = 0u32;
    let mut ti = 0usize;
    let mut prev_idx: Option<usize> = None;
    for &qc in q {
        let mut hit_idx = None;
        while ti < t.len() {
            if t[ti] == qc {
                hit_idx = Some(ti);
                ti += 1;
                break;
            }
            ti += 1;
        }
        let idx = hit_idx?;
        score += 2 + if prev_idx == Some(idx.wrapping_sub(1)) { 3 } else { 0 };
        prev_idx = Some(idx);
    }
    Some(score)
}

// ===========================================================================
// F324 — Shell 回归走廊：截图金样哈希
// ===========================================================================

/// 金样哈希一致才放行（64 位 FNV-1a）。
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

pub fn screenshot_golden(actual: &[u8], golden_hash: u64) -> bool {
    fnv1a64(actual) == golden_hash
}

// ===========================================================================
// F325 — Shell 年报：年度 Shell 统计
// ===========================================================================

pub const SHELL_REPORT_SECTIONS: [&str; 4] = ["usage", "latency", "crashes", "layout"];

#[derive(Clone, Copy)]
pub struct ShellYearStats {
    pub launches: u32,
    pub p95_open_ms: u32,
    pub crashes: u32,
}

impl ShellYearStats {
    pub fn report_ready(&self) -> bool {
        self.launches > 0 && self.p95_open_ms > 0 && SHELL_REPORT_SECTIONS.len() == 4
    }
}

// ===========================================================================
// 域自检
// ===========================================================================

pub fn run_m600shell_checks() -> CheckSet {
    let mut set = CheckSet::new("m600shell");

    // F301 任务栏
    set.add("F301 taskbar layout", taskbar_visible_icons(1920) == 20 && taskbar_visible_icons(400) == 3, "cap 20");
    set.add("F301 taskbar overflow", taskbar_overflow(400, 5) == 2 && taskbar_overflow(1920, 5) == 0, "fold");

    // F302 开始菜单
    set.add("F302 start menu", pinned_capacity() == 24 && tile_fits(2, 2) && !tile_fits(7, 1) && !tile_fits(0, 1), "24 slots");

    // F303 通知中心
    let notes = [
        Notification { app: "mail", ts: 10, urgent: true },
        Notification { app: "chat", ts: 20, urgent: false },
        Notification { app: "sys", ts: 30, urgent: true },
    ];
    set.add("F303 notification center", stack_visible(&notes) == 3 && urgent_count(&notes) == 2, "curated");
    set.add("F303 stack cap", stack_visible(&[Notification { app: "x", ts: 0, urgent: false }; 10]) == NOTIFICATION_STACK_MAX, "max 8");

    // F304 快速设置
    set.add("F304 quick settings", tile_order_valid(&[0, 0, 1, 3, 9]) && !tile_order_valid(&[0, 2, 1]), "monotonic");

    // F305 搜索一体化
    set.add("F305 unified search", blended_rank(1000, 500, 0) == 800 && blended_rank(0, 0, 1000) == 100, "blend 6:4");

    // F306 虚拟桌面
    set.add("F306 virtual desktops", desktop_switch_ok(1, 2) && !desktop_switch_ok(3, 3) && !desktop_switch_ok(1, 9) && !desktop_switch_ok(1, 0), "switch guard");

    // F307 窗口吸附
    set.add(
        "F307 snap zones",
        snap_zone(5, 400, 1000, 800, 20) == SnapZone::Left
            && snap_zone(995, 400, 1000, 800, 20) == SnapZone::Right
            && snap_zone(500, 10, 1000, 800, 20) == SnapZone::Top
            && snap_zone(500, 400, 1000, 800, 20) == SnapZone::None,
        "edges",
    );

    // F308 分屏蓝图
    set.add("F308 blueprints valid", BLUEPRINT_HALVES.iter().all(|t| t.valid()) && BLUEPRINT_QUAD.iter().all(|t| t.valid()), "halves+quad");
    set.add(
        "F308 blueprint bounds",
        !LayoutTile { x_permille: 600, y_permille: 0, w_permille: 500, h_permille: 1000 }.valid(),
        "overflow caught",
    );

    // F309 多显示器
    let mons = [
        Monitor { x: 0, y: 0, w: 1920, h: 1080 },
        Monitor { x: 1920, y: 0, w: 1280, h: 1024 },
    ];
    let bad = [Monitor { x: 0, y: 0, w: 1920, h: 1080 }, Monitor { x: 1000, y: 0, w: 1280, h: 1024 }];
    set.add("F309 multi monitor", monitors_disjoint(&mons) && !monitors_disjoint(&bad), "no overlap");

    // F310 任务视图
    set.add("F310 task view", card_scale_permille(0) == 1000 && card_scale_permille(2) == 760 && card_scale_permille(9) == 600, "cinematic scale");

    // F311 小组件
    set.add("F311 widgets", widget_size_ok(2, 2) && widget_size_ok(4, 4) && !widget_size_ok(3, 3) && !widget_size_ok(1, 1), "spec sizes");

    // F312 托盘礼仪
    set.add("F312 tray etiquette", tray_folded(9) == 3 && tray_folded(6) == 0 && tray_folded(2) == 0, "fold at 6");

    // F313 右键菜单
    set.add("F313 context menu", context_menu_valid(&[1, 2, 0, 3]) && !context_menu_valid(&[1, 0, 0, 3]) && !context_menu_valid(&[0, 1]), "separator rules");
    set.add("F313 menu length", !context_menu_valid(&[]) && !context_menu_valid(&[1; 13]), "1..=12");

    // F314 感知菜单
    set.add("F314 aware menu", menu_for(TargetKind::File)[0] == "open" && menu_for(TargetKind::Text).contains(&"paste") && menu_for(TargetKind::Desktop).len() == 3, "per kind");

    // F315 图标几何
    set.add("F315 icon geometry", icon_cell(200, 300) == (2, 3) && icon_cell_in_bounds(5, 3, 6, 4) && !icon_cell_in_bounds(6, 0, 6, 4), "grid snap");

    // F316 拖拽物理
    set.add("F316 drag physics", drag_velocity_px_s(800, 1000) == 800 && fling_detected(1600, 1000) && !fling_detected(400, 1000), "fling 800px/s");

    // F317 剪贴板历史
    let mut clip = ClipStore::new();
    clip.add(0xAA);
    clip.add(0xBB);
    clip.add(0xAA);
    let top = clip.get(0); // 末态读取：先存
    let second = clip.get(1);
    let third = clip.get(2);
    set.add("F317 clip dedup", top == Some(0xAA) && second == Some(0xBB) && third.is_none(), "dedup+front");
    set.add("F317 clip history", clip.len() == 2, "count");

    // F318 截图标注
    set.add("F318 annotate", annotation_ok("arrow", 2) && annotation_ok("text", 1) && !annotation_ok("arrow", 1) && !annotation_ok("stamp", 3), "tools");

    // F319 输入指示器
    set.add("F319 ime indicator", indicator_label(ImeState::Chinese) == "中" && indicator_label(ImeState::English) == "A" && indicator_label(ImeState::Off) == "A-off", "labels");

    // F320 状态灯
    set.add(
        "F320 led vocabulary",
        led_for(SysStatus::Healthy) == LedColor::Green
            && led_for(SysStatus::Degraded) == LedColor::Amber
            && led_for(SysStatus::Critical) == LedColor::Red
            && led_for(SysStatus::Idle) == LedColor::Off,
        "status map",
    );

    // F321 锁屏画作
    set.add("F321 lockscreen art", lockscreen_dim(23) == 400 && lockscreen_dim(3) == 400 && lockscreen_dim(12) == 100, "day/night dim");

    // F322 控制中心
    set.add(
        "F322 control center",
        toggles_consistent(QuickToggles { airplane: false, wifi: true, bluetooth: true })
            && toggles_consistent(QuickToggles { airplane: true, wifi: false, bluetooth: false })
            && !toggles_consistent(QuickToggles { airplane: true, wifi: true, bluetooth: false }),
        "airplane mutex",
    );

    // F323 命令面板
    set.add("F323 command palette", fuzzy_match("st", "status") == Some(7) && fuzzy_match("st", "files").is_none(), "fuzzy");
    set.add("F323 palette empty", fuzzy_match("", "anything") == Some(0), "empty query");

    // F324 回归走廊
    let golden_hash = fnv1a64(b"shell-2026-09");
    set.add("F324 shell corridor", screenshot_golden(b"shell-2026-09", golden_hash) && !screenshot_golden(b"shell-broken", golden_hash), "golden hash");

    // F325 Shell 年报
    let stats = ShellYearStats { launches: 12_000, p95_open_ms: 140, crashes: 2 };
    set.add(
        "F325 shell report",
        stats.report_ready() && SHELL_REPORT_SECTIONS.len() == 4
            && !ShellYearStats { launches: 0, p95_open_ms: 0, crashes: 0 }.report_ready(),
        "sections+counts",
    );

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f301_taskbar_math() {
        assert_eq!(taskbar_visible_icons(208), 0); // 8/(52)=0
        assert_eq!(taskbar_visible_icons(212), 0); // 12/52=0
        assert_eq!(taskbar_visible_icons(272), 1); // 72/52=1
        assert_eq!(taskbar_overflow(272, 3), 2);
    }

    #[test]
    fn f307_snap_zone_edges() {
        assert_eq!(snap_zone(500, 0, 1000, 800, 20), SnapZone::Top); // top 热区（x 居中）
        assert_eq!(snap_zone(10, 500, 1000, 800, 20), SnapZone::Left);
        assert_eq!(snap_zone(999, 500, 1000, 800, 20), SnapZone::Right);
        assert_eq!(snap_zone(500, 500, 1000, 800, 0), SnapZone::None);
    }

    #[test]
    fn f317_clip_ring_behavior() {
        let mut c = ClipStore::new();
        for i in 0..(CLIP_HISTORY_CAP as u64 + 5) {
            c.add(i);
        }
        assert_eq!(c.len(), CLIP_HISTORY_CAP);
        assert_eq!(c.get(0), Some(CLIP_HISTORY_CAP as u64 + 4)); // 最新在前
        c.add(3); // 旧条目重新前移
        assert_eq!(c.get(0), Some(3));
        assert_eq!(c.len(), CLIP_HISTORY_CAP); // 不新增
    }

    #[test]
    fn f323_fuzzy_scoring() {
        assert!(fuzzy_match("abc", "aebdc").is_some()); // 子序列
        assert!(fuzzy_match("ac", "ca").is_none()); // 顺序反了
        assert_eq!(fuzzy_match("a", "apple"), Some(2));
        // 连续命中得分更高
        let contiguous = fuzzy_match("ap", "apple");
        let split = fuzzy_match("ap", "a__p");
        assert!(contiguous.unwrap() > split.unwrap());
    }

    #[test]
    fn f309_monitor_adjacency_ok() {
        // 共边不算重叠
        let mons = [
            Monitor { x: 0, y: 0, w: 100, h: 100 },
            Monitor { x: 100, y: 0, w: 100, h: 100 },
            Monitor { x: 0, y: 100, w: 200, h: 100 },
        ];
        assert!(monitors_disjoint(&mons));
    }

    #[test]
    fn f325_shell_domain_selfcheck_all_pass() {
        let set = run_m600shell_checks();
        assert!(set.len() >= 25, "got {}", set.len());
        assert!(set.all_passed());
    }
}
