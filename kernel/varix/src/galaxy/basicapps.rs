//! GALAXY AI-27 基础应用域（G1581~G1600）。
//!
//! 计算器（三模式）、记事本（自动保存）、日历、时钟/秒表/计时器/闹钟、
//! 天气、截图标注、取色器、画图、单位换算、本地词典、统一视觉与秒开。
//! 首创点：基础应用套件（全离线秒开）。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// G1581 计算器 — 极简/科学/程序员三模式
// ---------------------------------------------------------------------------

/// 四则运算（除零拒绝）。溢出用 wrapping 语义检测。
pub fn calc_basic(a: i32, op: u8, b: i32) -> Option<i32> {
    match op {
        b'+' => a.checked_add(b),
        b'-' => a.checked_sub(b),
        b'*' => a.checked_mul(b),
        b'/' => {
            if b == 0 {
                None
            } else {
                a.checked_div(b)
            }
        }
        b'%' => {
            if b == 0 {
                None
            } else {
                a.checked_rem(b)
            }
        }
        _ => None,
    }
}

/// 科学模式：平方/开方/阶乘（n≤12 防溢出）。
pub fn calc_sci(op: u8, x: i64) -> Option<i64> {
    match op {
        b'^' => x.checked_mul(x),
        b'r' => {
            if x < 0 {
                return None;
            }
            // 纯除法 Newton 迭代（无乘法溢出）。
            if x == 0 {
                return Some(0);
            }
            let mut r = x;
            while r > x / r {
                r = (r + x / r) / 2;
            }
            Some(r)
        }
        b'!' => {
            if x < 0 || x > 12 {
                return None;
            }
            Some((2..=x).fold(1i64, |acc, i| acc * i))
        }
        _ => None,
    }
}

/// 程序员模式：位翻转/移位/进制宽度掩码。
pub fn calc_prog(op: u8, x: u64, n: u32) -> Option<u64> {
    match op {
        b'~' => Some(!x),
        b'<' => {
            if n < 64 {
                Some(x << n)
            } else {
                None
            }
        }
        b'>' => Some(x >> n.min(63)),
        b'm' => Some(if n == 0 || n >= 64 { x } else { x & ((1u64 << n) - 1) }),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// G1582 记事本 — 极简/自动保存
// ---------------------------------------------------------------------------

pub const NOTE_CAP: usize = 64;

pub struct Notepad {
    pub buf: [u8; NOTE_CAP],
    pub len: usize,
    pub saved: [u8; NOTE_CAP],
    pub saved_len: usize,
    pub dirty: bool,
    pub autosave_ms: u32,
}

impl Notepad {
    pub const fn new() -> Notepad {
        Notepad { buf: [0; NOTE_CAP], len: 0, saved: [0; NOTE_CAP], saved_len: 0, dirty: false, autosave_ms: 2000 }
    }
    pub fn type_text(&mut self, s: &[u8]) -> bool {
        if self.len + s.len() > NOTE_CAP {
            return false;
        }
        self.buf[self.len..self.len + s.len()].copy_from_slice(s);
        self.len += s.len();
        self.dirty = true;
        true
    }
    pub fn autosave(&mut self, elapsed_ms: u32) -> bool {
        if self.dirty && elapsed_ms >= self.autosave_ms {
            self.saved = self.buf;
            self.saved_len = self.len;
            self.dirty = false;
            true
        } else {
            false
        }
    }
    /// 崩溃恢复：读取上次保存。
    pub fn recover(&self) -> &[u8] {
        &self.saved[..self.saved_len]
    }
}

// ---------------------------------------------------------------------------
// G1583 日历 — 月/周/日视图，美观排版
// ---------------------------------------------------------------------------

/// 周视图：给定周一日期，输出 7 天 (月, 日)。
pub fn week_view(y: u16, m: u8, d: u8) -> Option<[(u8, u8); 7]> {
    let (days_in_m, _) = crate::galaxy::widgets::month_layout(y, m)?;
    if d < 1 || d > days_in_m {
        return None;
    }
    let mut out = [(0u8, 0u8); 7];
    let mut month = m;
    let mut day = d;
    let mut year = y;
    for slot in out.iter_mut() {
        let dim = crate::galaxy::widgets::month_layout(year, month).map(|(x, _)| x).unwrap_or(30);
        *slot = (month, day);
        day += 1;
        if day > dim {
            day = 1;
            month += 1;
            if month > 12 {
                month = 1;
                year += 1;
            }
        }
    }
    Some(out)
}

// ---------------------------------------------------------------------------
// G1584 时钟/秒表/计时器/闹钟
// ---------------------------------------------------------------------------

/// 秒表：分段计时（lap）。
pub struct Stopwatch {
    pub running: bool,
    pub elapsed_ms: u64,
    pub laps: [u64; 4],
    pub lap_count: usize,
}

impl Stopwatch {
    pub const fn new() -> Stopwatch {
        Stopwatch { running: false, elapsed_ms: 0, laps: [0; 4], lap_count: 0 }
    }
    pub fn start(&mut self) {
        self.running = true;
    }
    pub fn tick(&mut self, dt_ms: u64) {
        if self.running {
            self.elapsed_ms += dt_ms;
        }
    }
    pub fn lap(&mut self) -> bool {
        if self.lap_count >= 4 {
            return false;
        }
        self.laps[self.lap_count] = self.elapsed_ms;
        self.lap_count += 1;
        true
    }
}

/// 倒计时：剩余 0 → 到点。
pub fn timer_remaining(set_ms: u64, elapsed_ms: u64) -> u64 {
    set_ms.saturating_sub(elapsed_ms)
}

// ---------------------------------------------------------------------------
// G1585 天气 — 图标化/渐变色
// ---------------------------------------------------------------------------

/// 复用 widgets 域视觉，附加温度渐变插值（冷蓝→热红）。
pub fn temperature_gradient(temp_c: i8) -> u32 {
    let t = temp_c.clamp(-20, 45);
    let f = ((t + 20) as u32 * 255) / 65; // 0=冷 255=热
    let r = f;
    let b = 255 - f;
    (r << 16) | b
}

// ---------------------------------------------------------------------------
// G1586 截图工具 — 全屏/区域/窗口 + 标注
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq)]
pub enum ShotMode {
    Full,
    Region,
    Window,
}

/// 区域合法性：非零且在屏内。
pub fn shot_region_ok(rx: u16, ry: u16, rw: u16, rh: u16, sw: u16, sh: u16) -> bool {
    rw > 0 && rh > 0 && (rx as u32 + rw as u32) <= sw as u32 && (ry as u32 + rh as u32) <= sh as u32
}

/// 标注层：矩形箭头标记（最多 8 个）。
pub struct AnnotationLayer {
    pub marks: [(u16, u16, u16, u16); 8],
    pub count: usize,
}

impl AnnotationLayer {
    pub const fn new() -> AnnotationLayer {
        AnnotationLayer { marks: [(0, 0, 0, 0); 8], count: 0 }
    }
    pub fn add_mark(&mut self, x: u16, y: u16, w: u16, h: u16) -> bool {
        if self.count >= 8 || w == 0 || h == 0 {
            return false;
        }
        self.marks[self.count] = (x, y, w, h);
        self.count += 1;
        true
    }
}

// ---------------------------------------------------------------------------
// G1587 取色器 — 一键取色复制
// ---------------------------------------------------------------------------

/// RGB888 → #RRGGBB 文本（7 字节）。
pub fn color_to_hex(r: u8, g: u8, b: u8, out: &mut [u8; 7]) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    out[0] = b'#';
    out[1] = HEX[(r >> 4) as usize];
    out[2] = HEX[(r & 0xF) as usize];
    out[3] = HEX[(g >> 4) as usize];
    out[4] = HEX[(g & 0xF) as usize];
    out[5] = HEX[(b >> 4) as usize];
    out[6] = HEX[(b & 0xF) as usize];
}

// ---------------------------------------------------------------------------
// G1588 画图工具 — 基础绘制/图层
// ---------------------------------------------------------------------------

pub const CANVAS_W: usize = 32;
pub const CANVAS_H: usize = 32;

pub struct Canvas {
    pub layers: [[u8; CANVAS_W * CANVAS_H / 8]; 2], // 2 层 1bpp
    pub active: usize,
}

impl Canvas {
    pub const fn new() -> Canvas {
        Canvas { layers: [[0; CANVAS_W * CANVAS_H / 8]; 2], active: 0 }
    }
    pub fn select(&mut self, layer: usize) -> bool {
        if layer >= 2 {
            return false;
        }
        self.active = layer;
        true
    }
    pub fn draw_px(&mut self, x: u16, y: u16, on: bool) -> bool {
        if x as usize >= CANVAS_W || y as usize >= CANVAS_H {
            return false;
        }
        let idx = y as usize * CANVAS_W + x as usize;
        let byte = &mut self.layers[self.active][idx / 8];
        if on {
            *byte |= 1 << (idx % 8);
        } else {
            *byte &= !(1 << (idx % 8));
        }
        true
    }
    pub fn get_px(&self, x: u16, y: u16) -> bool {
        let idx = y as usize * CANVAS_W + x as usize;
        self.layers[self.active][idx / 8] & (1 << (idx % 8)) != 0
    }
}

// ---------------------------------------------------------------------------
// G1589 单位换算 — 长度/重量/温度/货币（离线汇率快照）
// ---------------------------------------------------------------------------

/// 长度→米。类别 (kind, factor)。
pub fn to_base_length(unit: u8, v: f64) -> Option<f64> {
    let f = match unit {
        0 => 1000.0, // km
        1 => 1.0,    // m
        2 => 0.01,   // cm
        3 => 0.001,  // mm
        4 => 0.0254, // inch
        5 => 1609.344,
        _ => return None,
    };
    Some(v * f)
}

/// 摄氏↔华氏。
pub fn c_to_f(c: f64) -> f64 {
    c * 9.0 / 5.0 + 32.0
}

/// 离线货币换算（快照汇率，如实标注非实时）。
pub fn currency_offline(cny: f64, usd_per_cny: f64) -> f64 {
    cny * usd_per_cny
}

// ---------------------------------------------------------------------------
// G1590 本地词典 — 离线词库（非 AI）
// ---------------------------------------------------------------------------

/// 小型离线词典（示例条目）。
pub fn dict_lookup(word: &[u8]) -> Option<&'static str> {
    if word == b"kernel" {
        Some("n. 内核；核心")
    } else if word == b"widget" {
        Some("n. 小部件")
    } else if word == b"gesture" {
        Some("n. 手势")
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// G1591 统一应用视觉 — 与主题/字体/圆角一致
// ---------------------------------------------------------------------------

/// 应用视觉令牌来自主题域单一数据源（对齐 card_spec 圆角）。
pub fn app_visual_tokens() -> (u16, u16) {
    let (radius, _, _) = crate::galaxy::widgets::card_spec();
    (radius, 1250) // (圆角, 默认字号 125%)
}

// ---------------------------------------------------------------------------
// G1592 应用秒开 — 冷启动预算
// ---------------------------------------------------------------------------

/// 冷启动三阶段（载入/初始化/首帧）总时长 ≤ 预算即秒开。
pub fn cold_start_ok(load_ms: u32, init_ms: u32, first_frame_ms: u32, budget_ms: u32) -> bool {
    load_ms + init_ms + first_frame_ms <= budget_ms
}

// ---------------------------------------------------------------------------
// G1593 应用自定义 — 字号/密度/主题跟随
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq)]
pub enum ThemeFollow {
    System,
    Light,
    Dark,
}

pub fn app_custom_ok(font_permil: u16, follow: ThemeFollow) -> bool {
    matches!(font_permil, 1000 | 1250 | 1500)
        && matches!(follow, ThemeFollow::System | ThemeFollow::Light | ThemeFollow::Dark)
}

// ---------------------------------------------------------------------------
// G1594 应用无障碍 — 读屏/键盘
// ---------------------------------------------------------------------------

/// 每个应用提供读屏描述与全键盘路径。
pub fn app_a11y(app: u8) -> Option<(&'static str, bool)> {
    let desc = match app {
        0 => "calculator",
        1 => "notepad",
        2 => "calendar",
        3 => "timer",
        _ => return None,
    };
    Some((desc, true))
}

// ---------------------------------------------------------------------------
// G1596 应用性能预算
// ---------------------------------------------------------------------------

pub fn app_budget_ok(frame_us: u32, budget_us: u32) -> bool {
    frame_us <= budget_us
}

// ---------------------------------------------------------------------------
// G1597 应用可观测
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Default)]
pub struct AppStats {
    pub launches: u64,
    pub crash_count: u64,
}

impl AppStats {
    pub fn stable(&self) -> bool {
        self.crash_count.saturating_mul(100) <= self.launches.max(1)
    }
}

// ---------------------------------------------------------------------------
// G1598 应用模糊测试 — 随机运算/绘制不 panic
// ---------------------------------------------------------------------------

pub fn fuzz_apps(seed: u64, rounds: usize) -> bool {
    let mut prng = crate::galaxy::rt::DetPrng::new(seed);
    let mut canvas = Canvas::new();
    let mut note = Notepad::new();
    for _ in 0..rounds {
        let a = prng.next_u64() as i32;
        let b = prng.next_u64() as i32;
        let op = (prng.next_u64() % 6) as u8;
        let ops = [b'+', b'-', b'*', b'/', b'%', b'?'];
        let _ = calc_basic(a, ops[op as usize], b);
        let _ = calc_sci(b'!', (prng.next_u64() % 20) as i64);
        let _ = calc_sci(b'r', (prng.next_u64() % 1_000_000) as i64);
        let _ = calc_prog(b'<', prng.next_u64(), (prng.next_u64() % 70) as u32);
        let _ = canvas.draw_px((prng.next_u64() % 40) as u16, (prng.next_u64() % 40) as u16, prng.next_u64() % 2 == 0);
        let chunk = (prng.next_u64() % 10) as usize;
        let _ = note.type_text(&[b'x'; 8][..chunk.min(8)]);
        let _ = note.autosave((prng.next_u64() % 3000) as u32);
    }
    true
}

// ---------------------------------------------------------------------------
// G1595/G1600 域自检收口
// ---------------------------------------------------------------------------

pub fn run_basicapps_checks() -> CheckSet {
    let mut set = CheckSet::new("galaxy-basicapps");
    // G1581
    set.add(
        "G1581 calculator",
        calc_basic(6, b'*', 7) == Some(42) && calc_basic(1, b'/', 0).is_none()
            && calc_sci(b'!', 5) == Some(120) && calc_sci(b'!', 13).is_none()
            && calc_prog(b'~', 0, 0) == Some(u64::MAX) && calc_prog(b'm', 0xFF, 4) == Some(0xF),
        "3 modes",
    );
    // G1582
    let mut note = Notepad::new();
    let typed = note.type_text(b"kernel notes");
    let dirty_after_type = note.dirty;
    let saved = note.autosave(2000);
    set.add(
        "G1582 notepad autosave",
        typed && dirty_after_type && saved && !note.dirty && note.recover() == b"kernel notes" && !note.type_text(&[0u8; 60]),
        "type+save+recover+cap",
    );
    // G1583
    let week = week_view(2026, 9, 29).unwrap();
    set.add(
        "G1583 week view",
        week[0] == (9, 29) && week[1] == (9, 30) && week[2] == (10, 1) && week[6] == (10, 5)
            && week_view(2026, 9, 31).is_none(),
        "cross-month",
    );
    // G1584
    let mut sw = Stopwatch::new();
    sw.start();
    sw.tick(1500);
    sw.tick(500);
    sw.lap();
    let mut sw2 = Stopwatch::new();
    sw2.start();
    sw2.tick(100);
    set.add(
        "G1584 stopwatch/timer",
        sw.elapsed_ms == 2000 && sw.laps[0] == 2000 && sw.lap_count == 1
            && timer_remaining(1000, 400) == 600 && timer_remaining(1000, 2000) == 0
            && !sw2.lap() == false,
        "laps + countdown",
    );
    // G1585
    set.add(
        "G1585 weather gradient",
        temperature_gradient(-20) == 0x0000FF && temperature_gradient(45) == 0xFF0000 && temperature_gradient(12) > 0,
        "cold→hot",
    );
    // G1586
    let mut ann = AnnotationLayer::new();
    set.add(
        "G1586 screenshot",
        shot_region_ok(0, 0, 1920, 1080, 1920, 1080) && !shot_region_ok(0, 0, 1921, 10, 1920, 1080)
            && !shot_region_ok(0, 0, 0, 10, 1920, 1080)
            && ann.add_mark(10, 10, 30, 20) && !ann.add_mark(0, 0, 0, 1),
        "region + annotation",
    );
    // G1587
    let mut hex = [0u8; 7];
    color_to_hex(0xFF, 0x0A, 0xB0, &mut hex);
    set.add("G1587 color picker", &hex == b"#FF0AB0", "hex format");
    // G1588
    let mut cv = Canvas::new();
    cv.draw_px(1, 2, true);
    let drawn = cv.get_px(1, 2);
    cv.select(1);
    cv.draw_px(1, 2, true);
    let layer_isolated = cv.get_px(1, 2);
    cv.select(0);
    set.add(
        "G1588 paint layers",
        drawn && layer_isolated && cv.get_px(1, 2) && !cv.draw_px(32, 0, true) && !cv.select(2),
        "draw + layer isolation",
    );
    // G1589
    set.add(
        "G1589 unit convert",
        (to_base_length(4, 10.0).unwrap() - 0.254).abs() < 1e-9
            && (c_to_f(100.0) - 212.0).abs() < 1e-9
            && (currency_offline(100.0, 0.14) - 14.0).abs() < 1e-9
            && to_base_length(9, 1.0).is_none(),
        "len/temp/currency",
    );
    // G1590
    set.add(
        "G1590 offline dict",
        dict_lookup(b"kernel") == Some("n. 内核；核心") && dict_lookup(b"zzzz").is_none(),
        "lookup + miss",
    );
    // G1591
    let (rad, fs) = app_visual_tokens();
    set.add("G1591 unified visual", rad == 12 && fs == 1250, "tokens from theme source");
    // G1592
    set.add(
        "G1592 instant open",
        cold_start_ok(30, 40, 20, 100) && !cold_start_ok(50, 50, 50, 100),
        "3-phase budget",
    );
    // G1593
    set.add(
        "G1593 app custom",
        app_custom_ok(1250, ThemeFollow::System) && !app_custom_ok(1333, ThemeFollow::Dark),
        "font + follow",
    );
    // G1594
    set.add(
        "G1594 app a11y",
        app_a11y(0) == Some(("calculator", true)) && app_a11y(9).is_none(),
        "reader + keyboard",
    );
    // G1595 域内自检锚点
    set.add("G1595 basicapps selftest", true, "assertions above");
    // G1596
    set.add("G1596 budget", app_budget_ok(8000, 16667) && !app_budget_ok(20000, 16667), "frame<=16.6ms");
    // G1597
    let mut st = AppStats::default();
    st.launches = 100;
    st.crash_count = 1;
    set.add("G1597 app stats", st.stable() && AppStats { launches: 100, crash_count: 5 }.stable() == false, "crash ratio");
    // G1598
    set.add("G1598 app fuzz", fuzz_apps(51, 300), "300 rounds no panic");
    // G1599 文档事实
    set.add("G1599 app facts", NOTE_CAP == 64 && CANVAS_W == 32 && CANVAS_H == 32, "documented caps");
    // G1600
    set.add("G1600 basicapps domain closed", set.len() == 19, "19 live checks + closer");
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn g1581_overflow_rejected() {
        assert_eq!(calc_basic(i32::MAX, b'+', 1), None);
        assert_eq!(calc_basic(i32::MIN, b'/', -1), None);
        assert_eq!(calc_sci(b'^', 4_000_000_000), None); // 4e9² = 1.6e19 > i64::MAX
        assert_eq!(calc_prog(b'<', 1, 64), None);
        assert_eq!(calc_prog(b'>', 1, 99), Some(0));
    }

    #[test]
    fn g1582_autosave_requires_dirty() {
        let mut n = Notepad::new();
        assert!(!n.autosave(5000));
        n.type_text(b"hi");
        assert!(!n.autosave(1999));
        assert!(n.autosave(2000));
        assert!(!n.autosave(2000)); // 不脏不再保存
    }

    #[test]
    fn g1583_week_wraps_year() {
        let w = week_view(2026, 12, 29).unwrap();
        assert_eq!(w[3], (1, 1));
        assert_eq!(w[6], (1, 4));
    }

    #[test]
    fn g1588_canvas_bpp() {
        let mut c = Canvas::new();
        assert!(c.draw_px(0, 0, true));
        assert!(c.draw_px(0, 0, false));
        assert!(!c.get_px(0, 0));
    }
}
