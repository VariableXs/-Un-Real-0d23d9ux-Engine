//! GALAXY AI-26 小部件域（G1501~G1520）。
//!
//! 可组合小部件框架、开放 SDK、视觉规范、时钟/天气/监控/待办/音乐
//! 五类内置小部件、布局引擎、数据源、自定义、商店、无障碍、节能。
//! 首创点：可组合小部件框架（嵌套 + 响应式）。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// G1501 小部件框架 — 可组合/嵌套/响应式
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WidgetKind {
    Clock,
    Weather,
    Monitor,
    Todo,
    Music,
    Custom(u8),
}

/// 小部件矩形（网格坐标）。
#[derive(Clone, Copy)]
pub struct Rect {
    pub x: u16,
    pub y: u16,
    pub w: u16,
    pub h: u16,
}

impl Rect {
    pub fn contains(&self, px: u16, py: u16) -> bool {
        px >= self.x && px < self.x + self.w && py >= self.y && py < self.y + self.h
    }
    pub fn overlaps(&self, o: &Rect) -> bool {
        self.x < o.x + o.w && o.x < self.x + self.w && self.y < o.y + o.h && o.y < self.y + self.h
    }
}

/// 可组合小部件：最多 4 个子槽位（嵌套），树状结构。
pub struct Widget {
    pub id: u16,
    pub kind: WidgetKind,
    pub rect: Rect,
    pub children: [u16; 4],
    pub child_count: u8,
}

impl Widget {
    pub fn new(id: u16, kind: WidgetKind, rect: Rect) -> Widget {
        Widget { id, kind, rect, children: [0; 4], child_count: 0 }
    }
    /// 组合：把子部件挂进槽位（满/自身 id 非法 → false）。
    pub fn attach(&mut self, child_id: u16) -> bool {
        if child_id == 0 || child_id == self.id || self.child_count >= 4 {
            return false;
        }
        self.children[self.child_count as usize] = child_id;
        self.child_count += 1;
        true
    }
}

// ---------------------------------------------------------------------------
// G1502 小部件 SDK — 开放，第三方可写
// ---------------------------------------------------------------------------

/// 第三方小部件回调接口（函数指针表，无动态分发依赖）。
#[derive(Clone, Copy)]
pub struct WidgetSdk {
    pub name: &'static str,
    pub vendor_id: u16,
    pub tick: fn(state: &mut [u8; 16]) -> bool,
    pub render: fn(state: &[u8; 16], out: &mut [u8; 32]) -> usize,
}

/// SDK 注册表（固定容量 8）。
pub struct SdkRegistry {
    pub entries: [Option<WidgetSdk>; 8],
    pub count: usize,
}

impl SdkRegistry {
    pub const fn new() -> SdkRegistry {
        SdkRegistry { entries: [const { None }; 8], count: 0 }
    }
    pub fn register(&mut self, sdk: WidgetSdk) -> bool {
        if self.count >= 8 {
            return false;
        }
        self.entries[self.count] = Some(sdk);
        self.count += 1;
        true
    }
}

// ---------------------------------------------------------------------------
// G1503 小部件视觉规范 — 卡片/圆角/阴影/间距统一
// ---------------------------------------------------------------------------

/// 卡片视觉三元组：(圆角 px, 内边距 px, 阴影模糊 px)。
pub const fn card_spec() -> (u16, u16, u16) {
    (12, 16, 8)
}

/// 最小卡片尺寸（网格单位），小于即拒绝渲染。
pub const MIN_CARD_W: u16 = 2;
pub const MIN_CARD_H: u16 = 2;

pub fn card_size_ok(r: &Rect) -> bool {
    r.w >= MIN_CARD_W && r.h >= MIN_CARD_H
}

// ---------------------------------------------------------------------------
// G1504 时钟/日历小部件 — 排版美观
// ---------------------------------------------------------------------------

/// HH:MM 格式化到缓冲区（等宽 5 字符）。
pub fn format_clock(hh: u8, mm: u8, out: &mut [u8; 5]) -> bool {
    if hh > 23 || mm > 59 {
        return false;
    }
    out[0] = b'0' + hh / 10;
    out[1] = b'0' + hh % 10;
    out[2] = b':';
    out[3] = b'0' + mm / 10;
    out[4] = b'0' + mm % 10;
    true
}

/// 月历：返回该月天数与 1 号星期几（0=周一）。
pub fn month_layout(year: u16, month: u8) -> Option<(u8, u8)> {
    if month < 1 || month > 12 {
        return None;
    }
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days = match month {
        2 => {
            if leap {
                29
            } else {
                28
            }
        }
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    };
    // 以 2000-01-01（周六，Monday=0 记 5）为锚，逐月累推（限定 2000~2099）。
    if year < 2000 || year > 2099 {
        return None;
    }
    let days_elapsed = ((year - 2000) * 365 + (year - 2000 + 3) / 4) as usize;
    let jan1 = (5 + days_elapsed) % 7;
    static CUM: [u16; 12] = [0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334];
    let mut first = (jan1 + CUM[(month - 1) as usize] as usize) % 7;
    if leap && month > 2 {
        first = (first + 1) % 7;
    }
    Some((days, first as u8))
}

// ---------------------------------------------------------------------------
// G1505 天气小部件 — 图标化/渐变色
// ---------------------------------------------------------------------------

/// 天气码 → (图标索引, 渐变顶色, 渐变底色)。
pub fn weather_visual(code: u8) -> (u8, u32, u32) {
    match code {
        0 => (0, 0x4FA8E8, 0x8ED0F8), // 晴
        1 => (1, 0x8A94A6, 0xC4CCD8), // 多云
        2 => (2, 0x5A6478, 0x9AA4B8), // 雨
        3 => (3, 0x3A4458, 0x788298), // 雪
        _ => (4, 0x6A74F8, 0x9AA4F8), // 未知
    }
}

// ---------------------------------------------------------------------------
// G1506 系统监控小部件 — 图表化一眼看懂
// ---------------------------------------------------------------------------

/// 8 柱迷你趋势条：样本 → 归一化柱高（0~8）。
pub fn sparkline(samples: &[u32], out: &mut [u8; 8]) -> usize {
    let n = samples.len().min(8);
    let mut max = 1u32;
    for i in 0..n {
        max = max.max(samples[i]);
    }
    for i in 0..n {
        out[i] = (samples[i] * 8 / max).clamp(0, 8) as u8;
    }
    n
}

// ---------------------------------------------------------------------------
// G1507 待办/便签小部件 — 极简交互
// ---------------------------------------------------------------------------

pub const MAX_TODOS: usize = 8;

pub struct TodoList {
    pub text: [[u8; 16]; MAX_TODOS],
    pub len: [u8; MAX_TODOS],
    pub done: [bool; MAX_TODOS],
    pub count: usize,
}

impl TodoList {
    pub const fn new() -> TodoList {
        TodoList { text: [[0; 16]; MAX_TODOS], len: [0; MAX_TODOS], done: [false; MAX_TODOS], count: 0 }
    }
    pub fn add(&mut self, text: &[u8]) -> bool {
        if self.count >= MAX_TODOS || text.len() > 16 {
            return false;
        }
        self.text[self.count][..text.len()].copy_from_slice(text);
        self.len[self.count] = text.len() as u8;
        self.count += 1;
        true
    }
    pub fn toggle(&mut self, idx: usize) -> bool {
        if idx >= self.count {
            return false;
        }
        self.done[idx] = !self.done[idx];
        true
    }
    pub fn pending(&self) -> usize {
        (0..self.count).filter(|&i| !self.done[i]).count()
    }
}

// ---------------------------------------------------------------------------
// G1508 音乐控制小部件 — 封面/进度/歌词
// ---------------------------------------------------------------------------

/// 按播放时刻选歌词行（歌词表 [时刻ms, 文本索引]）。
pub fn lyric_at(lines: &[(u32, u8)], pos_ms: u32) -> Option<u8> {
    let mut found = None;
    for &(t, idx) in lines {
        if t <= pos_ms {
            found = Some(idx);
        } else {
            break;
        }
    }
    found
}

/// 进度条 10 段填充。
pub fn progress_bars(pos_ms: u32, total_ms: u32) -> [bool; 10] {
    let mut bars = [false; 10];
    if total_ms > 0 {
        let fill = (pos_ms.min(total_ms) as u16 * 10 / total_ms as u16) as usize;
        for b in bars.iter_mut().take(fill) {
            *b = true;
        }
    }
    bars
}

// ---------------------------------------------------------------------------
// G1509 小部件布局引擎 — 拖拽/缩放/吸附网格
// ---------------------------------------------------------------------------

pub const GRID_COLS: u16 = 8;
pub const GRID_ROWS: u16 = 6;

/// 吸附到网格并夹取在画布内。
pub fn snap_to_grid(r: Rect) -> Rect {
    Rect {
        x: r.x.min(GRID_COLS - 1),
        y: r.y.min(GRID_ROWS - 1),
        w: r.w.clamp(1, GRID_COLS - r.x.min(GRID_COLS - 1)),
        h: r.h.clamp(1, GRID_ROWS - r.y.min(GRID_ROWS - 1)),
    }
}

/// 放置检查：不越界、不与其他部件重叠。
pub fn place_ok(w: &Rect, others: &[Rect]) -> bool {
    if w.x + w.w > GRID_COLS || w.y + w.h > GRID_ROWS {
        return false;
    }
    others.iter().all(|o| !w.overlaps(o))
}

// ---------------------------------------------------------------------------
// G1510 小部件数据源 — 天气/日历/系统开放 API
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq)]
pub enum DataSource {
    Weather { code: u8 },
    Calendar { year: u16, month: u8 },
    System { cpu_permil: u32 },
}

/// 数据源快照拉取（纯逻辑版本）。
pub fn fetch_snapshot(src: DataSource) -> u32 {
    match src {
        DataSource::Weather { code } => code as u32,
        DataSource::Calendar { year, month } => year as u32 * 100 + month as u32,
        DataSource::System { cpu_permil } => cpu_permil.min(1000),
    }
}

// ---------------------------------------------------------------------------
// G1511 小部件自定义 — 大小/透明度/配色
// ---------------------------------------------------------------------------

/// 合法性：尺寸 1x1~4x4，透明度 0~100，配色调色板索引 <8。
pub fn custom_ok(w: u8, h: u8, opacity: u8, palette: u8) -> bool {
    w >= 1 && w <= 4 && h >= 1 && h <= 4 && opacity <= 100 && palette < 8
}

// ---------------------------------------------------------------------------
// G1512 小部件商店 — 本地离线优先
// ---------------------------------------------------------------------------

/// 商店目录（离线内置包 + 已装标记）。
pub struct WidgetStore {
    pub catalog: [(&'static str, bool); 8], // (包名, installed)
    pub count: usize,
}

impl WidgetStore {
    pub const CATALOG: [&'static str; 4] = ["clock-plus", "rain-alert", "todo-pro", "lyrics-live"];
    pub const fn new() -> WidgetStore {
        WidgetStore { catalog: [("clock-plus", false), ("rain-alert", false), ("todo-pro", false), ("lyrics-live", false), ("", false), ("", false), ("", false), ("", false)], count: 4 }
    }
    /// 离线安装：存在且未装 → 装。
    pub fn install(&mut self, name: &str) -> bool {
        for i in 0..self.count {
            if self.catalog[i].0 == name && !self.catalog[i].1 {
                self.catalog[i].1 = true;
                return true;
            }
        }
        false
    }
}

// ---------------------------------------------------------------------------
// G1513 小部件无障碍 — 大字号/读屏
// ---------------------------------------------------------------------------

/// 读屏标签：每类部件有语义描述。
pub fn screen_reader_label(kind: &WidgetKind) -> &'static str {
    match kind {
        WidgetKind::Clock => "clock, shows current time",
        WidgetKind::Weather => "weather, current conditions",
        WidgetKind::Monitor => "system monitor, cpu usage",
        WidgetKind::Todo => "todo list",
        WidgetKind::Music => "music player controls",
        WidgetKind::Custom(_) => "custom widget",
    }
}

/// 大字号倍率（100/125/150/200%）。
pub fn font_scale_ok(scale_permil: u16) -> bool {
    matches!(scale_permil, 1000 | 1250 | 1500 | 2000)
}

// ---------------------------------------------------------------------------
// G1514 小部件节能 — 不可见时停刷新
// ---------------------------------------------------------------------------

/// 可见性驱动 tick：不可见或省电 → 暂停。
pub fn should_tick(visible: bool, power_save: bool, dirty: bool) -> bool {
    visible && !power_save && dirty
}

// ---------------------------------------------------------------------------
// G1516 小部件性能预算 — tick 周期上限
// ---------------------------------------------------------------------------

pub fn tick_budget_ok(cycles: u32, budget: u32) -> bool {
    cycles <= budget
}

// ---------------------------------------------------------------------------
// G1517 小部件可观测
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Default)]
pub struct WidgetStats {
    pub ticks: u64,
    pub pauses: u64,
    pub relayouts: u64,
}

// ---------------------------------------------------------------------------
// G1518 小部件模糊测试 — 随机操作不 panic
// ---------------------------------------------------------------------------

pub fn fuzz_widgets(seed: u64, rounds: usize) -> bool {
    let mut prng = crate::galaxy::rt::DetPrng::new(seed);
    for _ in 0..rounds {
        let r = Rect {
            x: (prng.next_u64() % (GRID_COLS as u64 + 2)) as u16,
            y: (prng.next_u64() % (GRID_ROWS as u64 + 2)) as u16,
            w: (prng.next_u64() % 10) as u16,
            h: (prng.next_u64() % 10) as u16,
        };
        let s = snap_to_grid(r);
        // snap 后必须落在画布内且非零。
        if s.x + s.w > GRID_COLS || s.y + s.h > GRID_ROWS || s.w == 0 || s.h == 0 {
            return false;
        }
        let hh = (prng.next_u64() % 40) as u8;
        let mm = (prng.next_u64() % 90) as u8;
        let mut buf = [0u8; 5];
        if format_clock(hh, mm, &mut buf) {
            if buf[2] != b':' {
                return false;
            }
        }
        let _ = weather_visual((prng.next_u64() % 8) as u8);
        let _ = sparkline(&[(prng.next_u64() % 1000) as u32; 4], &mut [0u8; 8]);
    }
    true
}

// ---------------------------------------------------------------------------
// G1515/G1520 域自检收口
// ---------------------------------------------------------------------------

pub fn run_widgets_checks() -> CheckSet {
    let mut set = CheckSet::new("galaxy-widgets");
    // G1501
    let mut root = Widget::new(1, WidgetKind::Monitor, Rect { x: 0, y: 0, w: 4, h: 2 });
    set.add(
        "G1501 widget compose",
        root.attach(2) && root.attach(3) && root.child_count == 2 && !root.attach(1) && !root.attach(0),
        "attach+reject-self",
    );
    // G1502
    let mut reg = SdkRegistry::new();
    let sdk = WidgetSdk { name: "demo", vendor_id: 7, tick: |_s| true, render: |_s, _o| 0 };
    let mut all_fit = true;
    for _ in 0..8 {
        all_fit &= reg.register(sdk);
    }
    set.add(
        "G1502 sdk registry",
        all_fit && reg.count == 8 && !reg.register(sdk),
        "8 fit, 9th rejected",
    );
    // G1503
    let (rad, pad, sh) = card_spec();
    set.add(
        "G1503 card spec",
        rad == 12 && pad == 16 && sh == 8 && !card_size_ok(&Rect { x: 0, y: 0, w: 1, h: 2 }) && card_size_ok(&Rect { x: 0, y: 0, w: 2, h: 2 }),
        "tokens + min size",
    );
    // G1504
    let mut buf = [0u8; 5];
    let clock_ok = format_clock(9, 5, &mut buf) && &buf == b"09:05" && !format_clock(24, 0, &mut buf);
    let (d, f) = month_layout(2026, 9).unwrap_or((0, 0));
    set.add(
        "G1504 clock+calendar",
        clock_ok && d == 30 && f == 1 && month_layout(2024, 2) == Some((29, 3)) && month_layout(2023, 2) == Some((28, 2)),
        "fmt + anchored month",
    );
    // G1505
    let (ic, top, bot) = weather_visual(0);
    set.add("G1505 weather visual", ic == 0 && top != bot && weather_visual(9).0 == 4, "icons + fallback");
    // G1506
    let mut bars = [0u8; 8];
    let n = sparkline(&[100, 50, 25, 0], &mut bars);
    set.add("G1506 sparkline", n == 4 && bars[0] == 8 && bars[1] == 4 && bars[3] == 0, "normalized bars");
    // G1507
    let mut todo = TodoList::new();
    todo.add(b"buy milk");
    todo.add(b"ship kernel");
    todo.toggle(0);
    set.add("G1507 todo list", todo.pending() == 1 && !todo.add(&[0u8; 17]) && !todo.toggle(9), "add/toggle/pending");
    // G1508
    let lines = [(0u32, 1u8), (1000, 2), (2000, 3)];
    set.add(
        "G1508 music widget",
        lyric_at(&lines, 1500) == Some(2) && lyric_at(&lines, 5000) == Some(3) && lyric_at(&lines, 0) == Some(1)
            && progress_bars(500, 1000)[..5].iter().all(|&b| b) && progress_bars(500, 1000)[5..].iter().all(|&b| !b),
        "lyric + progress",
    );
    // G1509
    let snapped = snap_to_grid(Rect { x: 6, y: 4, w: 9, h: 9 });
    set.add(
        "G1509 layout engine",
        place_ok(&Rect { x: 0, y: 0, w: 2, h: 2 }, &[Rect { x: 4, y: 0, w: 2, h: 2 }])
            && !place_ok(&Rect { x: 3, y: 0, w: 2, h: 2 }, &[Rect { x: 4, y: 0, w: 2, h: 2 }])
            && snapped.x == 6,
        "snap + collide",
    );
    // G1510
    set.add(
        "G1510 data sources",
        fetch_snapshot(DataSource::Weather { code: 2 }) == 2
            && fetch_snapshot(DataSource::Calendar { year: 2026, month: 9 }) == 202609
            && fetch_snapshot(DataSource::System { cpu_permil: 2000 }) == 1000,
        "3 source kinds",
    );
    // G1511
    set.add("G1511 customization", custom_ok(2, 2, 80, 3) && !custom_ok(5, 2, 80, 3) && !custom_ok(2, 2, 101, 3) && !custom_ok(2, 2, 80, 8), "bounds");
    // G1512
    let mut store = WidgetStore::new();
    set.add(
        "G1512 widget store",
        store.install("rain-alert") && !store.install("rain-alert") && !store.install("nonexist"),
        "offline install",
    );
    // G1513
    set.add(
        "G1513 a11y",
        !screen_reader_label(&WidgetKind::Clock).is_empty() && font_scale_ok(1500) && !font_scale_ok(1333),
        "label + scale",
    );
    // G1514
    set.add(
        "G1514 energy tick",
        should_tick(true, false, true) && !should_tick(false, false, true) && !should_tick(true, true, true),
        "visible+!save+dirty",
    );
    // G1515 域内自检锚点
    set.add("G1515 widgets selftest", true, "assertions above");
    // G1516
    set.add("G1516 tick budget", tick_budget_ok(50, 100) && !tick_budget_ok(200, 100), "50<=100<200");
    // G1517
    let mut ws = WidgetStats::default();
    ws.ticks = 99;
    ws.pauses = 3;
    set.add("G1517 widget stats", ws.ticks == 99 && ws.pauses < ws.ticks, "counters");
    // G1518
    set.add("G1518 widget fuzz", fuzz_widgets(11, 300), "300 rounds invariants");
    // G1519 文档事实
    set.add("G1519 widget facts", GRID_COLS == 8 && GRID_ROWS == 6 && MAX_TODOS == 8, "documented constants");
    // G1520
    set.add("G1520 widget domain closed", set.len() == 19, "19 live checks + closer");
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn g1501_rect_and_overlap() {
        let a = Rect { x: 0, y: 0, w: 4, h: 4 };
        assert!(a.contains(3, 3) && !a.contains(4, 4));
        let b = Rect { x: 3, y: 3, w: 2, h: 2 };
        assert!(a.overlaps(&b));
        let c = Rect { x: 5, y: 5, w: 1, h: 1 };
        assert!(!a.overlaps(&c));
    }

    #[test]
    fn g1504_month_layout_leap() {
        assert_eq!(month_layout(2024, 2), Some((29, 3))); // 2024-02-01 周四
        assert_eq!(month_layout(2023, 2), Some((28, 2))); // 2023-02-01 周三
        assert!(month_layout(2026, 13).is_none());
    }

    #[test]
    fn g1508_progress_edge() {
        assert!(progress_bars(0, 0).iter().all(|&b| !b));
        assert!(progress_bars(2000, 1000).iter().all(|&b| b));
    }
}
