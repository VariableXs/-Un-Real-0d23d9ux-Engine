//! VARIABLE-200 AI-06 · 输入域指针/无障碍/安全面（F136~F145）。
//!
//! 纯逻辑 + 固定容量数组，无分配。

use crate::checks::CheckSet;
use crate::hidsrv::{InputEvent, EventKind, MOD_ALT, MOD_CTRL, SC_A, SC_ENTER, SC_ESC};

// ---------------------------------------------------------------------------
// F136 鼠标加速曲线 — 默认与 AURORA Windows 版手感一致
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AccelProfile {
    /// 原始（无加速，电竞模式）。
    Raw,
    /// 线性 2×（固定增益）。
    Linear,
    /// Windows 默认"提高指针精确度"曲线。
    WindowsDefault,
}

/// 曲线增益上限（Q8）。
pub const ACCEL_GAIN_MAX_Q8: i64 = 2048;

/// 位移 → 指针位移。
pub fn accelerate(dx: i32, profile: AccelProfile) -> i32 {
    match profile {
        AccelProfile::Raw => dx,
        AccelProfile::Linear => dx.saturating_mul(2),
        AccelProfile::WindowsDefault => {
            let ad = (dx as i64).abs();
            let gain_q8 = 256 + (ad * ad * 256) / 4096;
            let out = (dx as i64) * gain_q8 / 256;
            out.clamp(-ACCEL_GAIN_MAX_Q8, ACCEL_GAIN_MAX_Q8) as i32
        }
    }
}

/// 加速曲线在对称性上的自洽：正负位移增益一致。
pub fn accel_symmetric(dx: i32, profile: AccelProfile) -> bool {
    accelerate(dx, profile) == -accelerate(-dx, profile)
}

#[derive(Clone, Copy)]
pub struct PointerState {
    pub profile: AccelProfile,
    /// 累积原始位移（未加速）。
    pub raw_dx: i64,
    pub raw_dy: i64,
    /// 指针位置（加速后）。
    pub x: i32,
    pub y: i32,
    pub samples: u64,
}

impl PointerState {
    pub const fn new(profile: AccelProfile) -> PointerState {
        PointerState { profile, raw_dx: 0, raw_dy: 0, x: 0, y: 0, samples: 0 }
    }

    pub fn feed(&mut self, dx: i32, dy: i32) {
        self.raw_dx += dx as i64;
        self.raw_dy += dy as i64;
        self.x = self.x.saturating_add(accelerate(dx, self.profile));
        self.y = self.y.saturating_add(accelerate(dy, self.profile));
        self.samples += 1;
    }

    pub fn set_profile(&mut self, p: AccelProfile) -> bool {
        if self.profile == p {
            return false;
        }
        self.profile = p;
        true
    }
}

// ---------------------------------------------------------------------------
// F137 滚轮平滑 — 行/像素模式 + 惯性令牌
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WheelMode {
    /// 每格一行。
    Line,
    /// 每格 N 像素（平滑滚动）。
    Pixel,
}

/// 滚轮令牌（可热配置，与主题令牌同源）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct WheelTokens {
    /// 每格行数。
    pub lines_per_notch: u8,
    /// 每行像素数。
    pub pixels_per_line: u8,
    /// 惯性衰减千分比（1000 = 无衰减）。
    pub inertia_permille: usize,
}

impl WheelTokens {
    pub const fn default_ws() -> WheelTokens {
        WheelTokens { lines_per_notch: 3, pixels_per_line: 16, inertia_permille: 820 }
    }

    pub fn valid(&self) -> bool {
        self.lines_per_notch >= 1
            && self.lines_per_notch <= 10
            && self.pixels_per_line >= 1
            && self.pixels_per_line <= 64
            && self.inertia_permille <= 1000
    }
}

#[derive(Clone, Copy)]
pub struct WheelSmoother {
    pub mode: WheelMode,
    pub tokens: WheelTokens,
    /// 待释放的累计像素（Q8）。
    pending_q8: i32,
    /// 上滚/下滚计数（符号分开统计，便于手感走查）。
    pub up_notches: u64,
    pub down_notches: u64,
}

impl WheelSmoother {
    pub const fn new(mode: WheelMode, tokens: WheelTokens) -> WheelSmoother {
        WheelSmoother { mode, tokens, pending_q8: 0, up_notches: 0, down_notches: 0 }
    }

    /// 收到一个滚轮格。
    pub fn feed(&mut self, notches: i8) {
        if notches > 0 {
            self.up_notches += notches as u64;
        } else {
            self.down_notches += (-(notches as i32)) as u64;
        }
        let amount = match self.mode {
            WheelMode::Line => notches as i32 * self.tokens.lines_per_notch as i32,
            WheelMode::Pixel => {
                notches as i32 * self.tokens.lines_per_notch as i32 * self.tokens.pixels_per_line as i32
            }
        };
        self.pending_q8 = self.pending_q8.saturating_add(amount * 256);
    }

    /// 每帧释放：带惯性地吐出像素量。
    pub fn tick(&mut self) -> i32 {
        let out = self.pending_q8 / 256;
        self.pending_q8 -= out * 256;
        // 惯性衰减：剩余量按令牌衰减。
        self.pending_q8 = self.pending_q8 * self.tokens.inertia_permille as i32 / 1000;
        out
    }

    pub fn pending(&self) -> i32 {
        self.pending_q8 / 256
    }

    /// 行模式一次释放总行数。
    pub fn drain_total(&mut self, budget: usize) -> i32 {
        let mut total = 0i32;
        let mut i = 0usize;
        while i < budget {
            let v = self.tick();
            if v == 0 {
                break;
            }
            total += v;
            i += 1;
        }
        total
    }
}

// ---------------------------------------------------------------------------
// F138 触控预留 — 多点触控事件结构占位
// ---------------------------------------------------------------------------

pub const TOUCH_MAX_POINTS: usize = 5;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TouchPoint {
    pub id: u8,
    pub x: i32,
    pub y: i32,
    /// 压力（0..=1024）。
    pub pressure: u16,
}

impl TouchPoint {
    pub const fn new(id: u8, x: i32, y: i32, pressure: u16) -> TouchPoint {
        TouchPoint { id, x, y, pressure }
    }
}

#[derive(Clone, Copy)]
pub struct TouchFrame {
    pub points: [TouchPoint; TOUCH_MAX_POINTS],
    pub count: usize,
    pub tick_us: u64,
}

impl TouchFrame {
    pub const fn new() -> TouchFrame {
        TouchFrame { points: [TouchPoint::new(0, 0, 0, 0); TOUCH_MAX_POINTS], count: 0, tick_us: 0 }
    }

    pub fn add(&mut self, p: TouchPoint) -> bool {
        if self.count >= TOUCH_MAX_POINTS {
            return false;
        }
        self.points[self.count] = p;
        self.count += 1;
        true
    }

    /// 与上一帧比较，识别简单手势（捏合方向）。
    pub fn pinch_scale_permille(&self, prev: &TouchFrame) -> usize {
        if self.count != 2 || prev.count != 2 {
            return 1000;
        }
        let d0 = dist2(&prev.points[0], &prev.points[1]) as u64;
        let d1 = dist2(&self.points[0], &self.points[1]) as u64;
        if d0 == 0 {
            return 1000;
        }
        (isqrt(d1) as usize) * 1000 / (isqrt(d0) as usize).max(1)
    }
}

/// 整数平方根（Newton 迭代，no_std 无浮点）。
pub fn isqrt(n: u64) -> u64 {
    if n == 0 {
        return 0;
    }
    let mut x = n;
    let mut y = (x + 1) / 2;
    while y < x {
        x = y;
        y = (x + n / x) / 2;
    }
    x
}

fn dist2(a: &TouchPoint, b: &TouchPoint) -> i64 {
    let dx = (a.x - b.x) as i64;
    let dy = (a.y - b.y) as i64;
    dx * dx + dy * dy
}

/// 触控能力占位：当前平台无触控（显式降级，禁止静默）。
pub const fn touch_supported() -> bool {
    false
}

// ---------------------------------------------------------------------------
// F139 手柄预留 — HID 游戏手柄事件类型占位
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GamepadState {
    /// 轴：LX, LY, RX, RY（-32768..=32767）。
    pub axes: [i16; 4],
    /// 按钮位。
    pub buttons: u16,
    /// 十字键位。
    pub dpad: u8,
    pub connected: bool,
}

impl GamepadState {
    pub const fn new() -> GamepadState {
        GamepadState { axes: [0i16; 4], buttons: 0, dpad: 0, connected: false }
    }

    pub fn connect(&mut self) -> bool {
        if self.connected {
            return false;
        }
        self.connected = true;
        true
    }

    pub fn disconnect(&mut self) -> bool {
        if !self.connected {
            return false;
        }
        self.connected = false;
        self.axes = [0i16; 4];
        self.buttons = 0;
        self.dpad = 0;
        true
    }

    /// 死区处理：小于阈值的轴归零（防漂移）。
    pub fn axis_with_deadzone(&self, i: usize, deadzone: i16) -> i16 {
        if i >= 4 {
            return 0;
        }
        let v = self.axes[i];
        if v.abs() <= deadzone {
            0
        } else {
            v
        }
    }

    /// 转成内核事件（占位路径，未连接返回 None）。
    pub fn to_event(&self, tick_us: u64) -> Option<InputEvent> {
        if !self.connected {
            return None;
        }
        Some(InputEvent {
            tick_us,
            kind: EventKind::Gamepad,
            code: self.buttons as u32,
            dx: self.axes[0] as i32,
            dy: self.axes[1] as i32,
            mods: self.dpad,
            device: 0,
        })
    }
}

// ---------------------------------------------------------------------------
// F140 输入延迟仪表 — 事件到上屏全链路打点（< 33ms 硬红线）
// ---------------------------------------------------------------------------

pub const LATENCY_RING: usize = 64;
/// 硬红线：33ms。
pub const LATENCY_REDLINE_US: u64 = 33_000;

#[derive(Clone, Copy)]
pub struct LatencyMeter {
    ring: [u32; LATENCY_RING],
    wr: usize,
    pub filled: usize,
    pub samples: u64,
    pub over_redline: u64,
    /// 分阶段：ISR→队列、队列→分发、分发→上屏。
    pub stage_us: [u32; 3],
}

impl LatencyMeter {
    pub const fn new() -> LatencyMeter {
        LatencyMeter {
            ring: [0u32; LATENCY_RING],
            wr: 0,
            filled: 0,
            samples: 0,
            over_redline: 0,
            stage_us: [0u32; 3],
        }
    }

    /// 记录一次全链路延迟（微秒）。
    pub fn record(&mut self, isr_us: u32, dispatch_us: u32, present_us: u32) -> u32 {
        let total = isr_us.saturating_add(dispatch_us).saturating_add(present_us);
        self.stage_us = [isr_us, dispatch_us, present_us];
        self.ring[self.wr] = total;
        self.wr = (self.wr + 1) % LATENCY_RING;
        if self.filled < LATENCY_RING {
            self.filled += 1;
        }
        self.samples += 1;
        if total as u64 > LATENCY_REDLINE_US {
            self.over_redline += 1;
        }
        total
    }

    pub fn percentile_permille(&self, permille: usize) -> u32 {
        if self.filled == 0 {
            return 0;
        }
        let mut a = [0u32; LATENCY_RING];
        let mut i = 0usize;
        while i < self.filled {
            a[i] = self.ring[i];
            i += 1;
        }
        let mut k = 1usize;
        while k < self.filled {
            let key = a[k];
            let mut j = k;
            while j > 0 && a[j - 1] > key {
                a[j] = a[j - 1];
                j -= 1;
            }
            a[j] = key;
            k += 1;
        }
        let idx = core::cmp::min(self.filled * permille / 1000, self.filled - 1);
        a[idx]
    }

    pub fn p99(&self) -> u32 {
        self.percentile_permille(990)
    }

    /// 手感红线判定。
    pub fn within_redline(&self) -> bool {
        (self.p99() as u64) < LATENCY_REDLINE_US
    }

    /// 全链路是否小于 Windows 版现值（基准微秒）。
    pub fn not_worse_than(&self, baseline_us: u64) -> bool {
        self.p99() as u64 <= baseline_us
    }
}

// ---------------------------------------------------------------------------
// F141 输入无障碍 — 粘滞键、按键重复速率
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RepeatSettings {
    pub delay_ms: u64,
    pub rate_per_sec: u8,
}

impl RepeatSettings {
    pub const fn default_ws() -> RepeatSettings {
        RepeatSettings { delay_ms: 500, rate_per_sec: 30 }
    }

    pub fn interval_ms(&self) -> u64 {
        if self.rate_per_sec == 0 {
            return u64::MAX;
        }
        1000 / self.rate_per_sec as u64
    }

    pub fn valid(&self) -> bool {
        self.delay_ms >= 100 && self.delay_ms <= 5000 && self.rate_per_sec >= 1 && self.rate_per_sec <= 60
    }
}

#[derive(Clone, Copy)]
pub struct StickyKeys {
    pub enabled: bool,
    /// 已锁存的修饰键位。
    latched: u8,
    /// 按下中的修饰键（未锁存）。
    held: u8,
    /// 连续按两次修饰键的计数（触发锁存）。
    pub latches: u32,
    pub releases: u32,
}

impl StickyKeys {
    pub const fn new(enabled: bool) -> StickyKeys {
        StickyKeys { enabled, latched: 0, held: 0, latches: 0, releases: 0 }
    }

    /// 修饰键按下：启用粘滞时锁存而非按住。
    pub fn mod_down(&mut self, bit: u8) -> bool {
        if !self.enabled || bit == 0 {
            self.held |= bit;
            return false;
        }
        self.latched |= bit;
        self.latches += 1;
        true
    }

    /// 普通键按下后消费锁存（一次性）。
    pub fn consume(&mut self) -> u8 {
        let m = self.latched;
        self.latched = 0;
        self.releases += 1;
        m
    }

    pub fn effective_mods(&self) -> u8 {
        self.held | self.latched
    }
}

// ---------------------------------------------------------------------------
// F142 输入安全 — 特权键防注入
// ---------------------------------------------------------------------------

/// 特权组合（Ctrl+Alt+Del 等价物）：应用可见性为零。
pub fn is_privileged(mods: u8, code: u8) -> bool {
    mods == (MOD_CTRL | MOD_ALT) && (code == SC_ESC || code == 0x5A)
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InputSource {
    /// 真实硬件中断。
    Hardware,
    /// 用户态注入（合成事件）。
    Synthetic,
}

#[derive(Clone, Copy)]
pub struct InjectionGuard {
    pub source: InputSource,
    /// 特权键上的注入尝试（必须被阻断）。
    pub blocked: u64,
    /// 放行的合成事件。
    pub allowed: u64,
}

impl InjectionGuard {
    pub const fn new() -> InjectionGuard {
        InjectionGuard { source: InputSource::Hardware, blocked: 0, allowed: 0 }
    }

    /// 校验一个事件是否可被投递。
    pub fn admit(&mut self, source: InputSource, mods: u8, code: u8) -> bool {
        if source == InputSource::Synthetic && is_privileged(mods, code) {
            self.blocked += 1;
            return false;
        }
        self.allowed += 1;
        true
    }

    /// 系统级组合在应用层必须不可见。
    pub fn app_visible(mods: u8, code: u8) -> bool {
        !is_privileged(mods, code)
    }
}

// ---------------------------------------------------------------------------
// F143 事件回放 — 事件序列录制与回放（回归测试利器）
// ---------------------------------------------------------------------------

pub const REPLAY_CAP: usize = 32;

#[derive(Clone, Copy)]
pub struct Replayer {
    events: [InputEvent; REPLAY_CAP],
    len: usize,
    pos: usize,
    pub recording: bool,
    pub replays: u32,
    pub overflow: u64,
}

impl Replayer {
    pub const fn new() -> Replayer {
        Replayer {
            events: [InputEvent::key(0, EventKind::KeyDown, 0, 0, 0); REPLAY_CAP],
            len: 0,
            pos: 0,
            recording: false,
            replays: 0,
            overflow: 0,
        }
    }

    pub fn start_record(&mut self) -> bool {
        if self.recording {
            return false;
        }
        self.recording = true;
        self.len = 0;
        self.pos = 0;
        true
    }

    pub fn record(&mut self, e: InputEvent) -> bool {
        if !self.recording {
            return false;
        }
        if self.len >= REPLAY_CAP {
            self.overflow += 1;
            return false;
        }
        self.events[self.len] = e;
        self.len += 1;
        true
    }

    pub fn stop_record(&mut self) -> usize {
        self.recording = false;
        self.len
    }

    pub fn start_replay(&mut self) -> bool {
        if self.len == 0 {
            return false;
        }
        self.pos = 0;
        self.replays += 1;
        true
    }

    pub fn next(&mut self) -> Option<InputEvent> {
        if self.pos >= self.len {
            return None;
        }
        let e = self.events[self.pos];
        self.pos += 1;
        Some(e)
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn get(&self, i: usize) -> Option<InputEvent> {
        if i < self.len {
            Some(self.events[i])
        } else {
            None
        }
    }
}

// ---------------------------------------------------------------------------
// F144 输入洪泛防护 — 单源速率限制
// ---------------------------------------------------------------------------

pub const SOURCE_MAX: usize = 4;

#[derive(Clone, Copy)]
pub struct SourceBucket {
    pub device: u8,
    pub window_start_us: u64,
    pub count: u32,
    pub limited: u64,
}

impl SourceBucket {
    pub const fn empty() -> SourceBucket {
        SourceBucket { device: 0, window_start_us: 0, count: 0, limited: 0 }
    }
}

#[derive(Clone, Copy)]
pub struct FloodGuard {
    buckets: [SourceBucket; SOURCE_MAX],
    count: usize,
    /// 每窗口允许的事件上限。
    pub limit: u32,
    /// 窗口长度（微秒）。
    pub window_us: u64,
    pub total_limited: u64,
}

impl FloodGuard {
    pub const fn new(limit: u32, window_us: u64) -> FloodGuard {
        FloodGuard { buckets: [SourceBucket::empty(); SOURCE_MAX], count: 0, limit, window_us, total_limited: 0 }
    }

    fn bucket(&mut self, device: u8) -> Option<usize> {
        let mut i = 0usize;
        while i < self.count {
            if self.buckets[i].device == device {
                return Some(i);
            }
            i += 1;
        }
        if self.count >= SOURCE_MAX {
            return None;
        }
        self.buckets[self.count].device = device;
        self.count += 1;
        Some(self.count - 1)
    }

    /// 准入判定：超限即拒绝并计数（防单源饿死 UI 线程）。
    pub fn admit(&mut self, device: u8, now_us: u64) -> bool {
        let i = match self.bucket(device) {
            Some(i) => i,
            None => {
                self.total_limited += 1;
                return false;
            }
        };
        if now_us.saturating_sub(self.buckets[i].window_start_us) >= self.window_us {
            self.buckets[i].window_start_us = now_us;
            self.buckets[i].count = 0;
        }
        if self.buckets[i].count >= self.limit {
            self.buckets[i].limited += 1;
            self.total_limited += 1;
            return false;
        }
        self.buckets[i].count += 1;
        true
    }

    pub fn limited_for(&self, device: u8) -> u64 {
        let mut i = 0usize;
        while i < self.count {
            if self.buckets[i].device == device {
                return self.buckets[i].limited;
            }
            i += 1;
        }
        0
    }
}

// ---------------------------------------------------------------------------
// F145 键盘布局 — 布局表机制 + 中文输入法接口预留
// ---------------------------------------------------------------------------

pub const LAYOUT_MAX: usize = 32;

#[derive(Clone, Copy)]
pub struct LayoutEntry {
    pub code: u8,
    pub plain: u8,
    pub shifted: u8,
    pub altgr: u8,
}

impl LayoutEntry {
    pub const fn new(code: u8, plain: u8, shifted: u8, altgr: u8) -> LayoutEntry {
        LayoutEntry { code, plain, shifted, altgr }
    }
}

#[derive(Clone, Copy)]
pub struct KeyLayout {
    entries: [LayoutEntry; LAYOUT_MAX],
    count: usize,
    pub name: [u8; 8],
    pub name_len: usize,
    pub ime_active: bool,
}

impl KeyLayout {
    pub const fn new() -> KeyLayout {
        KeyLayout {
            entries: [LayoutEntry::new(0, 0, 0, 0); LAYOUT_MAX],
            count: 0,
            name: [0u8; 8],
            name_len: 0,
            ime_active: false,
        }
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    pub fn set_name(&mut self, n: &[u8]) -> bool {
        if n.is_empty() || n.len() > 8 {
            return false;
        }
        self.name_len = n.len();
        let mut i = 0usize;
        while i < n.len() {
            self.name[i] = n[i];
            i += 1;
        }
        true
    }

    pub fn add(&mut self, e: LayoutEntry) -> Option<usize> {
        if self.count >= LAYOUT_MAX || e.code == 0 {
            return None;
        }
        let mut i = 0usize;
        while i < self.count {
            if self.entries[i].code == e.code {
                return None;
            }
            i += 1;
        }
        self.entries[self.count] = e;
        self.count += 1;
        Some(self.count - 1)
    }

    /// 翻译：code + 修饰 → 字符。IME 激活时映射到占位（组合态）。
    pub fn translate(&self, code: u8, shift: bool, altgr: bool) -> u8 {
        let mut i = 0usize;
        while i < self.count {
            let e = self.entries[i];
            if e.code == code {
                if self.ime_active {
                    return 0xFF; // 组合中：交给 IME
                }
                if altgr {
                    return e.altgr;
                }
                return if shift { e.shifted } else { e.plain };
            }
            i += 1;
        }
        0
    }

    pub fn set_ime(&mut self, active: bool) -> bool {
        if self.ime_active == active {
            return false;
        }
        self.ime_active = active;
        true
    }
}

/// 美式布局表。
pub fn us_layout() -> KeyLayout {
    let mut l = KeyLayout::new();
    let _ = l.set_name(b"us");
    let _ = l.add(LayoutEntry::new(SC_A, b'a', b'A', b'a'));
    let _ = l.add(LayoutEntry::new(0x16, b'1', b'!', b'1'));
    let _ = l.add(LayoutEntry::new(SC_ENTER, b'\n', b'\n', b'\n'));
    let _ = l.add(LayoutEntry::new(0x29, b' ', b' ', b' '));
    l
}

// ---------------------------------------------------------------------------
// 自检扩展（F136~F145 共 10 项）
// ---------------------------------------------------------------------------

pub fn extend_checks(set: &mut CheckSet) {
    // F136 鼠标加速曲线
    let raw = accelerate(32, AccelProfile::Raw);
    let lin = accelerate(32, AccelProfile::Linear);
    let win1 = accelerate(1, AccelProfile::WindowsDefault);
    let win32 = accelerate(32, AccelProfile::WindowsDefault);
    let win64 = accelerate(64, AccelProfile::WindowsDefault);
    let mut ptr = PointerState::new(AccelProfile::WindowsDefault);
    ptr.feed(32, -32);
    let p2 = ptr.set_profile(AccelProfile::Raw);
    let same = !ptr.set_profile(AccelProfile::Raw);
    set.add(
        "F136 mouse accel",
        raw == 32
            && lin == 64
            && win1 == 1
            && win32 == 40
            && win64 == 128
            && win32 > raw
            && accel_symmetric(32, AccelProfile::WindowsDefault)
            && accel_symmetric(7, AccelProfile::Linear)
            && ptr.x == 40
            && ptr.y == -40
            && ptr.raw_dy == -32
            && p2
            && same
            && ptr.samples == 1,
        "原始/线性/Windows 曲线与对称性",
    );

    // F137 滚轮平滑
    let tokens = WheelTokens::default_ws();
    let mut ws = WheelSmoother::new(WheelMode::Line, tokens);
    ws.feed(1);
    let total = ws.drain_total(64);
    let mut wp = WheelSmoother::new(WheelMode::Pixel, tokens);
    wp.feed(-1);
    let pt = wp.drain_total(16);
    let mut wup = WheelSmoother::new(WheelMode::Line, tokens);
    wup.feed(2);
    let _ = wup.drain_total(64);
    let _ = wup.feed(-3);
    let _ = wup.drain_total(64);
    set.add(
        "F137 wheel smoothing",
        tokens.valid()
            && total == tokens.lines_per_notch as i32
            && ws.pending() == 0
            && pt == -(tokens.lines_per_notch as i32 * tokens.pixels_per_line as i32)
            && wp.down_notches == 1
            && wup.up_notches == 2
            && wup.down_notches == 3
            && !WheelTokens { lines_per_notch: 0, ..tokens }.valid()
            && WheelTokens { inertia_permille: 1000, ..tokens }.valid(),
        "行/像素模式/惯性令牌/上下计数",
    );

    // F138 触控预留
    let mut tf = TouchFrame::new();
    let _ = tf.add(TouchPoint::new(0, 0, 0, 512));
    let _ = tf.add(TouchPoint::new(1, 30, 40, 512));
    let mut tf2 = TouchFrame::new();
    let _ = tf2.add(TouchPoint::new(0, 0, 0, 512));
    let _ = tf2.add(TouchPoint::new(1, 60, 80, 512));
    let scale = tf2.pinch_scale_permille(&tf);
    let mut full = TouchFrame::new();
    let mut i = 0usize;
    let mut added = 0usize;
    while i < TOUCH_MAX_POINTS + 2 {
        if full.add(TouchPoint::new(i as u8, 0, 0, 0)) {
            added += 1;
        }
        i += 1;
    }
    set.add(
        "F138 touch placeholder",
        tf.count == 2
            && tf2.count == 2
            && scale == 2000
            && added == TOUCH_MAX_POINTS
            && !touch_supported()
            && TouchFrame::new().pinch_scale_permille(&tf) == 1000,
        "多点结构/捏合比例/能力显式降级",
    );

    // F139 手柄预留
    let mut gp = GamepadState::new();
    let before = gp.to_event(0);
    let conn = gp.connect();
    let dup = !gp.connect();
    gp.axes[0] = 100;
    gp.axes[1] = -20000;
    gp.buttons = 0b101;
    let ev = gp.to_event(7);
    let dz = gp.axis_with_deadzone(0, 200);
    let dz1 = gp.axis_with_deadzone(1, 200);
    let disc = gp.disconnect();
    set.add(
        "F139 gamepad placeholder",
        before.is_none()
            && conn
            && dup
            && ev.map(|e| e.kind == EventKind::Gamepad && e.dx == 100 && e.dy == -20000 && e.code == 5).unwrap_or(false)
            && dz == 0
            && dz1 == -20000
            && disc
            && gp.to_event(8).is_none()
            && !gp.disconnect(),
        "连接/死区/事件转换/断连",
    );

    // F140 输入延迟仪表
    let mut lm = LatencyMeter::new();
    let mut i = 0usize;
    while i < LATENCY_RING {
        let _ = lm.record(1000, 2000, 3000);
        i += 1;
    }
    let p99 = lm.p99();
    let mut bad = LatencyMeter::new();
    let mut k = 0usize;
    while k < LATENCY_RING {
        let _ = bad.record(20_000, 20_000, 10_000);
        k += 1;
    }
    set.add(
        "F140 latency meter",
        lm.filled == LATENCY_RING
            && lm.samples == LATENCY_RING as u64
            && lm.stage_us == [1000, 2000, 3000]
            && p99 == 6000
            && lm.within_redline()
            && lm.not_worse_than(6000)
            && !lm.not_worse_than(5999)
            && bad.p99() == 50_000
            && !bad.within_redline()
            && bad.over_redline == LATENCY_RING as u64
            && LatencyMeter::new().p99() == 0,
        "全链路打点/p99/33ms 硬红线",
    );

    // F141 输入无障碍
    let rs = RepeatSettings::default_ws();
    let rs2 = RepeatSettings { delay_ms: 250, rate_per_sec: 40 };
    let sk = {
        let mut s = StickyKeys::new(true);
        let latched = s.mod_down(MOD_CTRL);
        let used = s.consume();
        let off = StickyKeys::new(false);
        latched && used == MOD_CTRL && s.releases == 1 && s.latches == 1 && s.effective_mods() == 0
            && off.enabled == false
            && !s.mod_down(0)
    };
    set.add(
        "F141 accessibility",
        rs.valid()
            && rs2.valid()
            && rs.interval_ms() == 33
            && rs2.interval_ms() == 25
            && RepeatSettings { rate_per_sec: 0, ..rs }.interval_ms() == u64::MAX
            && !RepeatSettings { delay_ms: 50, ..rs }.valid()
            && sk,
        "粘滞键锁存与消费/重复速率",
    );

    // F142 输入安全
    let mut ig = InjectionGuard::new();
    let block = !ig.admit(InputSource::Synthetic, MOD_CTRL | MOD_ALT, SC_ESC);
    let hw_ok = ig.admit(InputSource::Hardware, MOD_CTRL | MOD_ALT, SC_ESC);
    let normal = ig.admit(InputSource::Synthetic, MOD_CTRL, SC_A);
    set.add(
        "F142 input security",
        is_privileged(MOD_CTRL | MOD_ALT, SC_ESC)
            && is_privileged(MOD_CTRL | MOD_ALT, 0x5A)
            && !is_privileged(MOD_CTRL, SC_ESC)
            && !is_privileged(MOD_CTRL | MOD_ALT, SC_A)
            && block
            && hw_ok
            && normal
            && ig.blocked == 1
            && ig.allowed == 2
            && !InjectionGuard::app_visible(MOD_CTRL | MOD_ALT, SC_ESC)
            && InjectionGuard::app_visible(0, SC_A),
        "特权键阻断注入/应用不可见",
    );

    // F143 事件回放
    let mut rp = Replayer::new();
    let not_rec = !rp.record(InputEvent::key(1, EventKind::KeyDown, SC_A as u32, 0, 0));
    let started = rp.start_record();
    let _ = rp.record(InputEvent::key(1, EventKind::KeyDown, SC_A as u32, 0, 0));
    let _ = rp.record(InputEvent::key(2, EventKind::KeyUp, SC_A as u32, 0, 0));
    let n = rp.stop_record();
    let replay_ok = {
        let mut ok = rp.start_replay();
        let a1 = rp.next();
        let a2 = rp.next();
        let a3 = rp.next();
        let mut ok2 = rp.start_replay();
        let b1 = rp.next();
        ok = ok && ok2 && a1 == b1 && a2.is_some() && a3.is_none();
        ok2 = ok2 && a1.is_some();
        ok && ok2
    };
    set.add(
        "F143 event replay",
        not_rec
            && started
            && n == 2
            && rp.len() == 2
            && replay_ok
            && rp.replays == 2
            && rp.get(1).map(|e| e.kind == EventKind::KeyUp).unwrap_or(false)
            && !Replayer::new().start_replay(),
        "录制/回放可复现/空序列拒绝",
    );

    // F144 输入洪泛防护
    let mut fg = FloodGuard::new(8, 1000);
    let mut allowed = 0usize;
    let mut i = 0usize;
    while i < 12 {
        if fg.admit(1, i as u64) {
            allowed += 1;
        }
        i += 1;
    }
    let next_window = fg.admit(1, 2000);
    let other = fg.admit(2, 2000);
    set.add(
        "F144 flood guard",
        allowed == 8
            && fg.limited_for(1) == 4
            && fg.total_limited == 4
            && next_window
            && other
            && fg.admit(2, 2000)
            && fg.limited_for(1) == 4,
        "单源限流/窗口滚动/多源互不影响",
    );

    // F145 键盘布局
    let mut kb = us_layout();
    let plain = kb.translate(SC_A, false, false);
    let shifted = kb.translate(SC_A, true, false);
    let enter = kb.translate(SC_ENTER, false, false);
    let one = kb.translate(0x16, true, false);
    let unknown = kb.translate(0x77, false, false);
    let ime_on = kb.set_ime(true);
    let composing = kb.translate(SC_A, false, false);
    let dup = kb.add(LayoutEntry::new(SC_A, b'x', b'X', b'x')).is_none();
    set.add(
        "F145 keyboard layout",
        kb.len() == 4
            && kb.name_len == 2
            && plain == b'a'
            && shifted == b'A'
            && enter == b'\n'
            && one == b'!'
            && unknown == 0
            && ime_on
            && composing == 0xFF
            && dup
            && !kb.set_ime(true),
        "布局表/Shift 变体/IME 组合态预留",
    );
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f136_windows_curve_is_monotonic_gain() {
        let g1 = accelerate(8, AccelProfile::WindowsDefault);
        let g2 = accelerate(16, AccelProfile::WindowsDefault);
        let g3 = accelerate(64, AccelProfile::WindowsDefault);
        assert!(g1 >= 8 && g2 >= 16 && g3 >= 64);
        assert_eq!(accelerate(0, AccelProfile::WindowsDefault), 0);
        assert!(accel_symmetric(64, AccelProfile::WindowsDefault));
    }

    #[test]
    fn f137_pixel_mode_scrolls_further_than_line() {
        let t = WheelTokens::default_ws();
        let mut a = WheelSmoother::new(WheelMode::Line, t);
        let mut b = WheelSmoother::new(WheelMode::Pixel, t);
        a.feed(1);
        b.feed(1);
        assert!(b.drain_total(32) > a.drain_total(32));
    }

    #[test]
    fn f140_redline_is_strict() {
        let mut m = LatencyMeter::new();
        let _ = m.record(10_000, 10_000, 10_000);
        assert!(m.within_redline());
        let _ = m.record(11_000, 11_000, 11_001);
        assert!(!m.within_redline());
    }

    #[test]
    fn f143_replay_is_deterministic() {
        let mut r = Replayer::new();
        assert!(r.start_record());
        for i in 0..5u64 {
            assert!(r.record(InputEvent::key(i, EventKind::KeyDown, SC_A as u32, 0, 0)));
        }
        assert_eq!(r.stop_record(), 5);
        assert!(r.start_replay());
        let mut seq = [0u64; 5];
        let mut i = 0usize;
        while let Some(e) = r.next() {
            seq[i] = e.tick_us;
            i += 1;
        }
        assert_eq!(seq, [0, 1, 2, 3, 4]);
    }

    #[test]
    fn f144_flood_guard_resets_window() {
        let mut g = FloodGuard::new(3, 100);
        assert!(g.admit(1, 0));
        assert!(g.admit(1, 1));
        assert!(g.admit(1, 2));
        assert!(!g.admit(1, 3));
        assert!(g.admit(1, 100));
    }
}
