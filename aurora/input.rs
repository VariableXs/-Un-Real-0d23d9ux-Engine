//! AURORA-1000 AI-07 · 输入子系统全栈（A151~A175，W1）
//!
//! 输入即手感：键盘矩阵 / 鼠标 / 触控板手势 / 游戏手柄 / HID / 事件泵
//! 全栈 + 低延迟事件分发。全部为纯逻辑 + 固定容量数组（no_std，无分配），
//! 主机测试套件（`cargo ktest`）可在无硬件情况下跑完整域自检。
//!
//! 设计要点：扫描码→键值映射表（US 布局 Set1 翻译，含 Shift/Caps 状态机）、
//! 修饰键状态寄存器、按键去抖、HID 报告描述符最小解析、鼠标增量累积→绝对坐标
//! 边界钳制、滚轮累积、触控板手势状态机、手柄位掩码、事件环形队列（[Event;64]
//! 满则丢最旧并计数）、焦点路由分发、按住重复节流、单调低延迟时间戳。

use crate::checks::CheckSet;

// ===========================================================================
// 公共类型
// ===========================================================================

/// 逻辑键值（与具体扫描码集解耦）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum KeySym {
    None = 0,
    A, B, C, D, E, F, G, H, I, J, K, L, M, N, O, P, Q, R, S, T, U, V, W, X, Y, Z,
    D0, D1, D2, D3, D4, D5, D6, D7, D8, D9,
    Enter, Esc, Backspace, Tab, Space,
    Minus, Equal, Lbracket, Rbracket, Backslash,
    Semicolon, Quote, Backquote, Comma, Dot, Slash,
    CapsLock, LShift, RShift, LCtrl, RCtrl, LAlt, RAlt, LMeta, RMeta,
    F1, F2, F3, F4, F5, F6, F7, F8, F9, F10, F11, F12,
    Up, Down, Left, Right,
}

/// 输入事件种类。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputKind {
    KeyDown,
    KeyUp,
    MouseMove,
    MouseBtn,
    Wheel,
    Touch,
    Gamepad,
    Gesture,
    Hotkey,
}

/// 一条输入事件（纯值，可固定容量存放）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Event {
    pub kind: InputKind,
    pub code: u16,
    pub x: i32,
    pub y: i32,
    pub value: i16,
    pub device: u8,
    /// 单调时间戳（低延迟计数）。
    pub stamp: u64,
}

impl Event {
    pub const fn none() -> Event {
        Event {
            kind: InputKind::KeyDown,
            code: 0,
            x: 0,
            y: 0,
            value: 0,
            device: 0,
            stamp: 0,
        }
    }
}

/// 输入设备类型。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum DeviceKind {
    Keyboard = 0,
    Mouse = 1,
    Touchpad = 2,
    Gamepad = 3,
    Hid = 4,
}

// ===========================================================================
// A151 — 键盘驱动与扫描码（PS/2 Set1 翻译，含 make/break）
// ===========================================================================

/// Set1 扫描码 → 键值映射表（make code，不含 0xE0 扩展前缀）。
pub const SCAN_ENTRIES: [(u8, KeySym); 73] = [
    (0x01, KeySym::Esc),
    (0x02, KeySym::D1),
    (0x03, KeySym::D2),
    (0x04, KeySym::D3),
    (0x05, KeySym::D4),
    (0x06, KeySym::D5),
    (0x07, KeySym::D6),
    (0x08, KeySym::D7),
    (0x09, KeySym::D8),
    (0x0A, KeySym::D9),
    (0x0B, KeySym::D0),
    (0x0C, KeySym::Minus),
    (0x0D, KeySym::Equal),
    (0x0E, KeySym::Backspace),
    (0x0F, KeySym::Tab),
    (0x10, KeySym::Q),
    (0x11, KeySym::W),
    (0x12, KeySym::E),
    (0x13, KeySym::R),
    (0x14, KeySym::T),
    (0x15, KeySym::Y),
    (0x16, KeySym::U),
    (0x17, KeySym::I),
    (0x18, KeySym::O),
    (0x19, KeySym::P),
    (0x1A, KeySym::Lbracket),
    (0x1B, KeySym::Rbracket),
    (0x1C, KeySym::Enter),
    (0x1D, KeySym::LCtrl),
    (0x1E, KeySym::A),
    (0x1F, KeySym::S),
    (0x20, KeySym::D),
    (0x21, KeySym::F),
    (0x22, KeySym::G),
    (0x23, KeySym::H),
    (0x24, KeySym::J),
    (0x25, KeySym::K),
    (0x26, KeySym::L),
    (0x27, KeySym::Semicolon),
    (0x28, KeySym::Quote),
    (0x29, KeySym::Backquote),
    (0x2A, KeySym::LShift),
    (0x2B, KeySym::Backslash),
    (0x2C, KeySym::Z),
    (0x2D, KeySym::X),
    (0x2E, KeySym::C),
    (0x2F, KeySym::V),
    (0x30, KeySym::B),
    (0x31, KeySym::N),
    (0x32, KeySym::M),
    (0x33, KeySym::Comma),
    (0x34, KeySym::Dot),
    (0x35, KeySym::Slash),
    (0x36, KeySym::RShift),
    (0x38, KeySym::LAlt),
    (0x39, KeySym::Space),
    (0x3A, KeySym::CapsLock),
    (0x3B, KeySym::F1),
    (0x3C, KeySym::F2),
    (0x3D, KeySym::F3),
    (0x3E, KeySym::F4),
    (0x3F, KeySym::F5),
    (0x40, KeySym::F6),
    (0x41, KeySym::F7),
    (0x42, KeySym::F8),
    (0x43, KeySym::F9),
    (0x44, KeySym::F10),
    (0x45, KeySym::F11),
    (0x46, KeySym::F12),
    (0x48, KeySym::Up),
    (0x4B, KeySym::Left),
    (0x4D, KeySym::Right),
    (0x50, KeySym::Down),
];

/// Set1 断码 = 通码 | 0x80。
pub fn is_break(code: u8) -> bool {
    code & 0x80 != 0
}

/// 将单个 Set1 扫描码翻译为键值（自动剥离断码位）。未知码返回 `None`。
pub fn decode_scancode(code: u8) -> Option<KeySym> {
    let c = code & 0x7F;
    let mut i = 0;
    while i < SCAN_ENTRIES.len() {
        if SCAN_ENTRIES[i].0 == c {
            return Some(SCAN_ENTRIES[i].1);
        }
        i += 1;
    }
    None
}

/// 解码为 `(键值, 是否按下)`，同时处理 make/break。
pub fn decode_key_event(code: u8) -> Option<(KeySym, bool)> {
    let ks = decode_scancode(code)?;
    Some((ks, !is_break(code)))
}

// ===========================================================================
// A152 — 键位映射表（US 布局，Shift/Caps 状态机）
// ===========================================================================

/// 单条键位：基础字符、Shift 字符、是否字母（决定 Caps 是否翻转）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeymapEntry {
    pub key: KeySym,
    pub normal: u8,
    pub shifted: u8,
    pub letter: bool,
}

/// US 布局字符映射表（字母 + 数字 + 符号 + 控制键占位）。
pub const KEYMAP: [KeymapEntry; 53] = [
    KeymapEntry { key: KeySym::A, normal: b'a', shifted: b'A', letter: true },
    KeymapEntry { key: KeySym::B, normal: b'b', shifted: b'B', letter: true },
    KeymapEntry { key: KeySym::C, normal: b'c', shifted: b'C', letter: true },
    KeymapEntry { key: KeySym::D, normal: b'd', shifted: b'D', letter: true },
    KeymapEntry { key: KeySym::E, normal: b'e', shifted: b'E', letter: true },
    KeymapEntry { key: KeySym::F, normal: b'f', shifted: b'F', letter: true },
    KeymapEntry { key: KeySym::G, normal: b'g', shifted: b'G', letter: true },
    KeymapEntry { key: KeySym::H, normal: b'h', shifted: b'H', letter: true },
    KeymapEntry { key: KeySym::I, normal: b'i', shifted: b'I', letter: true },
    KeymapEntry { key: KeySym::J, normal: b'j', shifted: b'J', letter: true },
    KeymapEntry { key: KeySym::K, normal: b'k', shifted: b'K', letter: true },
    KeymapEntry { key: KeySym::L, normal: b'l', shifted: b'L', letter: true },
    KeymapEntry { key: KeySym::M, normal: b'm', shifted: b'M', letter: true },
    KeymapEntry { key: KeySym::N, normal: b'n', shifted: b'N', letter: true },
    KeymapEntry { key: KeySym::O, normal: b'o', shifted: b'O', letter: true },
    KeymapEntry { key: KeySym::P, normal: b'p', shifted: b'P', letter: true },
    KeymapEntry { key: KeySym::Q, normal: b'q', shifted: b'Q', letter: true },
    KeymapEntry { key: KeySym::R, normal: b'r', shifted: b'R', letter: true },
    KeymapEntry { key: KeySym::S, normal: b's', shifted: b'S', letter: true },
    KeymapEntry { key: KeySym::T, normal: b't', shifted: b'T', letter: true },
    KeymapEntry { key: KeySym::U, normal: b'u', shifted: b'U', letter: true },
    KeymapEntry { key: KeySym::V, normal: b'v', shifted: b'V', letter: true },
    KeymapEntry { key: KeySym::W, normal: b'w', shifted: b'W', letter: true },
    KeymapEntry { key: KeySym::X, normal: b'x', shifted: b'X', letter: true },
    KeymapEntry { key: KeySym::Y, normal: b'y', shifted: b'Y', letter: true },
    KeymapEntry { key: KeySym::Z, normal: b'z', shifted: b'Z', letter: true },
    KeymapEntry { key: KeySym::D0, normal: b'0', shifted: b')', letter: false },
    KeymapEntry { key: KeySym::D1, normal: b'1', shifted: b'!', letter: false },
    KeymapEntry { key: KeySym::D2, normal: b'2', shifted: b'@', letter: false },
    KeymapEntry { key: KeySym::D3, normal: b'3', shifted: b'#', letter: false },
    KeymapEntry { key: KeySym::D4, normal: b'4', shifted: b'$', letter: false },
    KeymapEntry { key: KeySym::D5, normal: b'5', shifted: b'%', letter: false },
    KeymapEntry { key: KeySym::D6, normal: b'6', shifted: b'^', letter: false },
    KeymapEntry { key: KeySym::D7, normal: b'7', shifted: b'&', letter: false },
    KeymapEntry { key: KeySym::D8, normal: b'8', shifted: b'*', letter: false },
    KeymapEntry { key: KeySym::D9, normal: b'9', shifted: b'(', letter: false },
    KeymapEntry { key: KeySym::Minus, normal: b'-', shifted: b'_', letter: false },
    KeymapEntry { key: KeySym::Equal, normal: b'=', shifted: b'+', letter: false },
    KeymapEntry { key: KeySym::Lbracket, normal: b'[', shifted: b']', letter: false },
    KeymapEntry { key: KeySym::Rbracket, normal: b']', shifted: b'}', letter: false },
    KeymapEntry { key: KeySym::Backslash, normal: b'\\', shifted: b'|', letter: false },
    KeymapEntry { key: KeySym::Semicolon, normal: b';', shifted: b':', letter: false },
    KeymapEntry { key: KeySym::Quote, normal: b'\'', shifted: b'"', letter: false },
    KeymapEntry { key: KeySym::Backquote, normal: b'`', shifted: b'~', letter: false },
    KeymapEntry { key: KeySym::Comma, normal: b',', shifted: b'<', letter: false },
    KeymapEntry { key: KeySym::Dot, normal: b'.', shifted: b'>', letter: false },
    KeymapEntry { key: KeySym::Slash, normal: b'/', shifted: b'?', letter: false },
    KeymapEntry { key: KeySym::Space, normal: b' ', shifted: b' ', letter: false },
    KeymapEntry { key: KeySym::Backspace, normal: 0, shifted: 0, letter: false },
    KeymapEntry { key: KeySym::Tab, normal: 0, shifted: 0, letter: false },
    KeymapEntry { key: KeySym::Enter, normal: 0, shifted: 0, letter: false },
    KeymapEntry { key: KeySym::Esc, normal: 0, shifted: 0, letter: false },
    KeymapEntry { key: KeySym::CapsLock, normal: 0, shifted: 0, letter: false },
];

/// 按 Shift / Caps 状态机得出最终 ASCII。
pub fn char_for(key: KeySym, shift: bool, caps: bool) -> u8 {
    let mut i = 0;
    while i < KEYMAP.len() {
        let e = KEYMAP[i];
        if e.key == key {
            if e.letter {
                // 字母：Shift 与 Caps 互为反相（XOR）。
                let upper = shift != caps;
                return if upper { e.shifted } else { e.normal };
            }
            return if shift { e.shifted } else { e.normal };
        }
        i += 1;
    }
    0
}

// ===========================================================================
// A153 — 鼠标驱动（增量累积 → 绝对坐标，边界钳制 + 滚轮累积）
// ===========================================================================

/// 将 i64 钳制进 [lo, hi] 后再转回 i32，避免溢出。
fn clamp_i32(v: i64, lo: i32, hi: i32) -> i32 {
    if v < lo as i64 {
        lo
    } else if v > hi as i64 {
        hi
    } else {
        v as i32
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MouseState {
    pub x: i32,
    pub y: i32,
    pub wheel: i32,
    pub buttons: u8,
}

impl MouseState {
    pub const fn new() -> MouseState {
        MouseState { x: 0, y: 0, wheel: 0, buttons: 0 }
    }

    /// 累积相对位移并钳制到 [0, w]×[0, h]，滚轮单独累加。
    pub fn apply(&mut self, dx: i16, dy: i16, wheel: i8, w: i32, h: i32) {
        let nx = self.x as i64 + dx as i64;
        self.x = clamp_i32(nx, 0, w);
        let ny = self.y as i64 + dy as i64;
        self.y = clamp_i32(ny, 0, h);
        self.wheel = self.wheel.saturating_add(wheel as i32);
    }
}

// ===========================================================================
// A154 — 触控板驱动（多指原始坐标跟踪）
// ===========================================================================

pub const MAX_FINGERS: usize = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TouchPoint {
    pub id: u8,
    pub x: i32,
    pub y: i32,
    pub down: bool,
}

impl TouchPoint {
    pub const fn zero() -> TouchPoint {
        TouchPoint { id: 0, x: 0, y: 0, down: false }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct TouchpadState {
    points: [TouchPoint; MAX_FINGERS],
    count: usize,
}

impl TouchpadState {
    pub const fn new() -> TouchpadState {
        TouchpadState { points: [TouchPoint::zero(); MAX_FINGERS], count: 0 }
    }

    /// 上/下更新某根手指；超过 4 指时忽略新增，落下时移除。
    pub fn update(&mut self, id: u8, x: i32, y: i32, down: bool) {
        if down {
            let mut i = 0;
            while i < self.count {
                if self.points[i].id == id {
                    self.points[i] = TouchPoint { id, x, y, down: true };
                    return;
                }
                i += 1;
            }
            if self.count < MAX_FINGERS {
                self.points[self.count] = TouchPoint { id, x, y, down: true };
                self.count += 1;
            }
        } else {
            let mut i = 0;
            while i < self.count {
                if self.points[i].id == id {
                    let mut j = i;
                    while j < self.count - 1 {
                        self.points[j] = self.points[j + 1];
                        j += 1;
                    }
                    self.count -= 1;
                    return;
                }
                i += 1;
            }
        }
    }

    pub fn get(&self, i: usize) -> Option<TouchPoint> {
        if i < self.count {
            Some(self.points[i])
        } else {
            None
        }
    }

    pub fn finger_count(&self) -> usize {
        self.count
    }
}

// ===========================================================================
// A155 — 游戏手柄支持（A/B/X/Y/D-pad 位掩码）
// ===========================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum GamepadButton {
    A = 0,
    B = 1,
    X = 2,
    Y = 3,
    Lb = 4,
    Rb = 5,
    Start = 6,
    Select = 7,
    Dup = 8,
    Ddn = 9,
    Dleft = 10,
    Dright = 11,
    L3 = 12,
    R3 = 13,
}

impl GamepadButton {
    pub fn mask(self) -> u16 {
        1u16 << (self as u8)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Gamepad {
    pub buttons: u16,
    pub lx: i16,
    pub ly: i16,
    pub rx: i16,
    pub ry: i16,
}

impl Gamepad {
    pub const fn new() -> Gamepad {
        Gamepad { buttons: 0, lx: 0, ly: 0, rx: 0, ry: 0 }
    }

    pub fn press(&mut self, b: GamepadButton) {
        self.buttons |= b.mask();
    }

    pub fn release(&mut self, b: GamepadButton) {
        self.buttons &= !b.mask();
    }

    pub fn pressed(self, b: GamepadButton) -> bool {
        self.buttons & b.mask() != 0
    }

    /// 方向键向量（屏幕上：上 = -1）。
    pub fn dpad(&self) -> (i8, i8) {
        let x = if self.pressed(GamepadButton::Dleft) {
            -1
        } else if self.pressed(GamepadButton::Dright) {
            1
        } else {
            0
        };
        let y = if self.pressed(GamepadButton::Dup) {
            -1
        } else if self.pressed(GamepadButton::Ddn) {
            1
        } else {
            0
        };
        (x, y)
    }
}

// ===========================================================================
// A156 — HID 报告解析（usage page / button count 抽取）
// ===========================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HidInfo {
    pub usage_page: u16,
    pub usage: u16,
    pub button_count: u8,
    pub report_id: u8,
}

/// 最小 HID 报告描述符解析：抽取 Usage Page(0x05)、Usage(0x09)、
/// Report Count(0x95)、Report ID(0x85)。找不到 Usage Page 则失败。
pub fn parse_hid_report_descriptor(bytes: &[u8]) -> Option<HidInfo> {
    if bytes.len() < 2 {
        return None;
    }
    let mut info = HidInfo { usage_page: 0, usage: 0, button_count: 0, report_id: 0 };
    let mut found_page = false;
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        match b {
            0x05 => {
                // 首 个 Usage Page 生效（后续 collections 的 page 不覆盖主 page）。
                if !found_page && i + 1 < bytes.len() {
                    info.usage_page = bytes[i + 1] as u16;
                    found_page = true;
                }
                i += 2;
            }
            0x09 => {
                if i + 1 < bytes.len() {
                    info.usage = bytes[i + 1] as u16;
                }
                i += 2;
            }
            0x95 => {
                if i + 1 < bytes.len() {
                    info.button_count = bytes[i + 1];
                }
                i += 2;
            }
            0x85 => {
                if i + 1 < bytes.len() {
                    info.report_id = bytes[i + 1];
                }
                i += 2;
            }
            _ => {
                i += 1;
            }
        }
    }
    if found_page {
        Some(info)
    } else {
        None
    }
}

// ===========================================================================
// A157 — 输入事件泵（环形队列 [Event;64]，满丢最旧 + 计数）
// ===========================================================================

pub const EVENT_CAP: usize = 64;

#[derive(Clone, Copy, Debug)]
pub struct EventPump {
    buf: [Option<Event>; EVENT_CAP],
    head: usize,
    count: usize,
    dropped: u64,
}

impl EventPump {
    pub const fn new() -> EventPump {
        EventPump {
            buf: [None; EVENT_CAP],
            head: 0,
            count: 0,
            dropped: 0,
        }
    }

    /// 入队；满时丢弃最旧并计数，新事件写入队尾。
    pub fn push(&mut self, e: Event) {
        if self.count == EVENT_CAP {
            self.buf[self.head] = None;
            self.head = (self.head + 1) % EVENT_CAP;
            self.dropped = self.dropped.saturating_add(1);
        }
        let tail = (self.head + self.count) % EVENT_CAP;
        self.buf[tail] = Some(e);
        if self.count < EVENT_CAP {
            self.count += 1;
        }
    }

    /// 出队（最旧）。
    pub fn pop(&mut self) -> Option<Event> {
        if self.count == 0 {
            return None;
        }
        let e = self.buf[self.head].take();
        self.head = (self.head + 1) % EVENT_CAP;
        self.count -= 1;
        e
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_full(&self) -> bool {
        self.count == EVENT_CAP
    }

    pub fn dropped(&self) -> u64 {
        self.dropped
    }

    pub fn peek(&self, i: usize) -> Option<Event> {
        if i < self.count {
            self.buf[(self.head + i) % EVENT_CAP]
        } else {
            None
        }
    }
}

// ===========================================================================
// A158 — 事件分发与订阅（焦点路由）
// ===========================================================================

pub const MAX_SUBS: usize = 16;

/// 焦点路由：事件按焦点窗口 id 派发（始终返回焦点窗口）。
pub fn route_focus(_e: Event, focus_window: u32) -> u32 {
    focus_window
}

#[derive(Clone, Copy, Debug)]
pub struct SubscriberTable {
    wins: [u32; MAX_SUBS],
    count: usize,
}

impl SubscriberTable {
    pub const fn new() -> SubscriberTable {
        SubscriberTable { wins: [0; MAX_SUBS], count: 0 }
    }

    pub fn subscribe(&mut self, w: u32) -> bool {
        if self.count >= MAX_SUBS {
            return false;
        }
        let mut i = 0;
        while i < self.count {
            if self.wins[i] == w {
                return true;
            }
            i += 1;
        }
        self.wins[self.count] = w;
        self.count += 1;
        true
    }

    pub fn unsubscribe(&mut self, w: u32) -> bool {
        let mut i = 0;
        while i < self.count {
            if self.wins[i] == w {
                let mut j = i;
                while j < self.count - 1 {
                    self.wins[j] = self.wins[j + 1];
                    j += 1;
                }
                self.count -= 1;
                return true;
            }
            i += 1;
        }
        false
    }

    pub fn delivers(&self, w: u32) -> bool {
        let mut i = 0;
        while i < self.count {
            if self.wins[i] == w {
                return true;
            }
            i += 1;
        }
        false
    }

    pub fn len(&self) -> usize {
        self.count
    }
}

// ===========================================================================
// A159 — 按键重复率（按住重复节流）
// ===========================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RepeatState {
    last_key: u16,
    held_ms: u32,
    next_at: u32,
    active: bool,
}

impl RepeatState {
    pub const fn new() -> RepeatState {
        RepeatState { last_key: 0, held_ms: 0, next_at: 0, active: false }
    }

    /// 每帧调用：按下超过 delay 后首次触发，之后每 rate 触发一次。
    /// 返回 `true` 表示应再发一次重复事件。
    pub fn tick(&mut self, key: u16, pressed: bool, dt_ms: u32, delay_ms: u32, rate_ms: u32) -> bool {
        if !pressed {
            self.active = false;
            self.last_key = 0;
            return false;
        }
        if !self.active || self.last_key != key {
            self.active = true;
            self.last_key = key;
            self.held_ms = 0;
            self.next_at = delay_ms;
            return false;
        }
        self.held_ms = self.held_ms.saturating_add(dt_ms);
        if self.held_ms >= self.next_at {
            self.next_at = self.next_at.saturating_add(rate_ms);
            return true;
        }
        false
    }
}

// ===========================================================================
// A160 — 组合键与快捷键（修饰键状态寄存器）
// ===========================================================================

/// 修饰键状态寄存器（ctrl/alt/shift/meta）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ModState {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub meta: bool,
}

impl ModState {
    pub const fn new() -> ModState {
        ModState { ctrl: false, alt: false, shift: false, meta: false }
    }

    /// 根据按下/抬起更新对应修饰位（左/右合并）。
    pub fn update(&mut self, key: KeySym, pressed: bool) {
        match key {
            KeySym::LCtrl | KeySym::RCtrl => self.ctrl = pressed,
            KeySym::LAlt | KeySym::RAlt => self.alt = pressed,
            KeySym::LShift | KeySym::RShift => self.shift = pressed,
            KeySym::LMeta | KeySym::RMeta => self.meta = pressed,
            _ => {}
        }
    }

    pub fn any(&self) -> bool {
        self.ctrl || self.alt || self.shift || self.meta
    }
}

/// 修饰键 + 主键完全一致才算命中组合键。
pub fn match_hotkey(combo_mods: ModState, combo_key: KeySym, cur: ModState, key: KeySym) -> bool {
    cur.ctrl == combo_mods.ctrl
        && cur.alt == combo_mods.alt
        && cur.shift == combo_mods.shift
        && cur.meta == combo_mods.meta
        && key == combo_key
}

// ===========================================================================
// A161 — 输入设备热插拔（注册 / 拔出 / 上下线）
// ===========================================================================

pub const MAX_DEVICES: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InputDevice {
    pub id: u8,
    pub kind: DeviceKind,
    pub online: bool,
    pub name: &'static str,
}

#[derive(Clone, Copy, Debug)]
pub struct DeviceRegistry {
    devs: [Option<InputDevice>; MAX_DEVICES],
    count: usize,
}

impl DeviceRegistry {
    pub const fn new() -> DeviceRegistry {
        DeviceRegistry { devs: [None; MAX_DEVICES], count: 0 }
    }

    pub fn plug(&mut self, d: InputDevice) -> bool {
        if self.count >= MAX_DEVICES {
            return false;
        }
        let mut i = 0;
        while i < self.count {
            if let Some(ex) = self.devs[i] {
                if ex.id == d.id {
                    self.devs[i] = Some(d);
                    return true;
                }
            }
            i += 1;
        }
        self.devs[self.count] = Some(d);
        self.count += 1;
        true
    }

    pub fn unplug(&mut self, id: u8) -> bool {
        let mut i = 0;
        while i < self.count {
            if let Some(ex) = self.devs[i] {
                if ex.id == id {
                    let mut j = i;
                    while j < self.count - 1 {
                        self.devs[j] = self.devs[j + 1];
                        j += 1;
                    }
                    self.count -= 1;
                    return true;
                }
            }
            i += 1;
        }
        false
    }

    pub fn set_online(&mut self, id: u8, online: bool) -> bool {
        let mut i = 0;
        while i < self.count {
            if let Some(mut ex) = self.devs[i] {
                if ex.id == id {
                    ex.online = online;
                    self.devs[i] = Some(ex);
                    return true;
                }
            }
            i += 1;
        }
        false
    }

    /// 统计某类「在线」设备数。
    pub fn count_kind(&self, kind: DeviceKind) -> usize {
        let mut n = 0;
        let mut i = 0;
        while i < self.count {
            if let Some(ex) = self.devs[i] {
                if ex.kind == kind && ex.online {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn get(&self, i: usize) -> Option<InputDevice> {
        if i < self.count {
            self.devs[i]
        } else {
            None
        }
    }
}

// ===========================================================================
// A162 — 输入延迟预算（单调时间戳 + 预算）
// ===========================================================================

/// 目标：单事件端到端 ≤ 8ms。
pub const INPUT_LATENCY_BUDGET_US: u32 = 8000;

/// 单调计数器（低延迟时间戳来源）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Monotonic {
    pub t: u64,
}

impl Monotonic {
    pub const fn new() -> Monotonic {
        Monotonic { t: 0 }
    }

    pub fn tick(&mut self) -> u64 {
        self.t = self.t.wrapping_add(1);
        self.t
    }
}

pub fn latency_ok(measured_us: u32, budget_us: u32) -> bool {
    measured_us <= budget_us
}

// ===========================================================================
// A163 — 输入设备列表（按类枚举，A161 之上）
// ===========================================================================

/// 返回某类在线设备 id 列表（固定容量），返回实际写入数。
pub fn list_devices(reg: &DeviceRegistry, kind: DeviceKind, out: &mut [u8]) -> usize {
    let mut n = 0;
    let mut i = 0;
    while i < reg.len() {
        if let Some(d) = reg.get(i) {
            if d.kind == kind && d.online && n < out.len() {
                out[n] = d.id;
                n += 1;
            }
        }
        i += 1;
    }
    n
}

// ===========================================================================
// A164 — 输入模糊测试（边界 / 非法输入防护）
// ===========================================================================

/// 事件合法性（保留设备 id 之外均为合法）。用作 fuzz 守卫。
pub fn validate_event(e: Event) -> bool {
    e.device != 0xFF
}

/// 将任意字节流当作扫描码喂入事件泵；返回处理字节数（不恐慌）。
pub fn fuzz_feed(pump: &mut EventPump, bytes: &[u8], stamp: u64) -> u32 {
    let mut n = 0u32;
    let mut i = 0;
    while i < bytes.len() {
        let code = bytes[i];
        if let Some(ks) = decode_scancode(code) {
            let brk = is_break(code);
            pump.push(Event {
                kind: if brk { InputKind::KeyUp } else { InputKind::KeyDown },
                code: ks as u16,
                x: 0,
                y: 0,
                value: 0,
                device: 0,
                stamp,
            });
        }
        n = n.saturating_add(1);
        i += 1;
    }
    n
}

// ===========================================================================
// A165 — 输入录制与回放
// ===========================================================================

pub const RECORD_CAP: usize = 128;

#[derive(Clone, Copy, Debug)]
pub struct InputRecorder {
    buf: [Event; RECORD_CAP],
    head: usize,
    count: usize,
}

impl InputRecorder {
    pub const fn new() -> InputRecorder {
        InputRecorder { buf: [Event::none(); RECORD_CAP], head: 0, count: 0 }
    }

    pub fn record(&mut self, e: Event) {
        if self.count == RECORD_CAP {
            self.buf[self.head] = e;
            self.head = (self.head + 1) % RECORD_CAP;
        } else {
            let t = (self.head + self.count) % RECORD_CAP;
            self.buf[t] = e;
            self.count += 1;
        }
    }

    pub fn len(&self) -> usize {
        self.count
    }

    /// 将录制内容重放进事件泵，返回注入条数。
    pub fn replay(&self, pump: &mut EventPump) -> usize {
        let mut n = 0;
        let mut i = 0;
        while i < self.count {
            pump.push(self.buf[(self.head + i) % RECORD_CAP]);
            n += 1;
            i += 1;
        }
        n
    }
}

// ===========================================================================
// A166 — 输入无障碍（粘滞键 + 慢速键）
// ===========================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AccessState {
    pub sticky: ModState,
    pub sticky_on: bool,
}

impl AccessState {
    pub const fn new() -> AccessState {
        AccessState { sticky: ModState::new(), sticky_on: false }
    }

    /// 粘滞键：每次按下修饰键切换其锁存状态。返回当前粘滞修饰态。
    pub fn sticky_update(&mut self, key: KeySym, pressed: bool) -> ModState {
        let is_mod = matches_modifier(key);
        if is_mod && pressed {
            self.sticky_on = !self.sticky_on;
            self.sticky.update(key, self.sticky_on);
        }
        self.sticky
    }
}

fn matches_modifier(key: KeySym) -> bool {
    matches_mod(key)
}

fn matches_mod(key: KeySym) -> bool {
    key == KeySym::LCtrl
        || key == KeySym::RCtrl
        || key == KeySym::LAlt
        || key == KeySym::RAlt
        || key == KeySym::LShift
        || key == KeySym::RShift
        || key == KeySym::LMeta
        || key == KeySym::RMeta
}

/// 慢速键：两次按键间隔须 ≥ min_ms。
pub fn slow_key_ok(last_ms: u64, now_ms: u64, min_ms: u64) -> bool {
    now_ms.saturating_sub(last_ms) >= min_ms
}

// ===========================================================================
// A167 — 输入与手势协作（触控板手势状态机）
// ===========================================================================

const GESTURE_MOVE_THRESH: i32 = 8;
const GESTURE_SWIPE_THRESH: i32 = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gesture {
    None,
    Scroll(i16, i16),
    Tap2,
    Swipe3Up,
}

#[derive(Clone, Copy, Debug)]
pub struct GestureState {
    last_count: usize,
    start: [TouchPoint; MAX_FINGERS],
    moved: bool,
    swipe_fired: bool,
    last: Gesture,
}

impl GestureState {
    pub const fn new() -> GestureState {
        GestureState {
            last_count: 0,
            start: [TouchPoint::zero(); MAX_FINGERS],
            moved: false,
            swipe_fired: false,
            last: Gesture::None,
        }
    }

    /// 依据触控板当前快照推进手势状态机。
    pub fn update(&mut self, pad: &TouchpadState) -> Gesture {
        let cur = pad.finger_count();
        if cur != self.last_count {
            let mut i = 0;
            while i < MAX_FINGERS {
                self.start[i] = pad.get(i).unwrap_or(TouchPoint::zero());
                i += 1;
            }
            self.moved = false;
            self.swipe_fired = false;
        }

        let g = if cur == 2 && self.last_count == 2 {
            let mut total = 0i32;
            let mut si = 0;
            while si < 2 {
                let s = self.start[si];
                let c = pad.get(si).unwrap_or(s);
                total += (c.x - s.x).abs() + (c.y - s.y).abs();
                si += 1;
            }
            if total > GESTURE_MOVE_THRESH {
                self.moved = true;
                let mut sx = 0i32;
                let mut sy = 0i32;
                let mut ci = 0;
                while ci < 2 {
                    let c = pad.get(ci).unwrap_or(self.start[ci]);
                    sx += c.x;
                    sy += c.y;
                    ci += 1;
                }
                let avg_x = sx / 2;
                let avg_y = sy / 2;
                let start_x = (self.start[0].x + self.start[1].x) / 2;
                let start_y = (self.start[0].y + self.start[1].y) / 2;
                Gesture::Scroll((avg_x - start_x) as i16, (avg_y - start_y) as i16)
            } else {
                Gesture::None
            }
        } else if cur == 0 && self.last_count == 2 {
            if !self.moved {
                Gesture::Tap2
            } else {
                Gesture::None
            }
        } else if cur == 3 {
            let mut ay_start = 0i32;
            let mut ay_cur = 0i32;
            let mut fi = 0;
            while fi < 3 {
                ay_start += self.start[fi].y as i32;
                ay_cur += pad.get(fi).map(|t| t.y).unwrap_or(self.start[fi].y) as i32;
                fi += 1;
            }
            let d = ay_start - ay_cur; // 上移为正
            if d > GESTURE_SWIPE_THRESH && !self.swipe_fired {
                self.swipe_fired = true;
                Gesture::Swipe3Up
            } else {
                Gesture::None
            }
        } else {
            Gesture::None
        };

        self.last_count = cur;
        self.last = g;
        g
    }

    pub fn last(&self) -> Gesture {
        self.last
    }
}

// ===========================================================================
// A168 — 输入自检收口（核心不变量）
// ===========================================================================

pub fn input_core_invariant() -> bool {
    let mut letters_ok = true;
    let mut li = 0;
    let letters = [KeySym::A, KeySym::M, KeySym::Z];
    while li < letters.len() {
        if char_for(letters[li], false, false) == 0 {
            letters_ok = false;
        }
        li += 1;
    }
    let mut m = MouseState::new();
    m.apply(1000, 1000, 0, 100, 100);
    let clamp_ok = m.x == 100 && m.y == 100;
    let mut pump = EventPump::new();
    pump.push(Event::none());
    let queue_ok = pump.len() == 1 && EVENT_CAP == 64;
    SCAN_ENTRIES.len() > 0 && letters_ok && clamp_ok && queue_ok
}

// ===========================================================================
// A169 — 输入子系统全栈自检（聚合器，被 A175 收口入口复用）
// ===========================================================================

pub fn input_subsystem_check() -> CheckSet {
    run_ainput_checks()
}

// ===========================================================================
// A170 — 输入子系统全栈性能预算
// ===========================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PerfBudget {
    pub budget_us: u32,
}

impl PerfBudget {
    pub const fn new(budget_us: u32) -> PerfBudget {
        PerfBudget { budget_us }
    }

    pub fn within(&self, measured: u32) -> bool {
        measured <= self.budget_us
    }

    /// 0..100 性能分（越接近预算越差）。
    pub fn score(&self, measured: u32) -> u8 {
        if self.budget_us == 0 {
            return 0;
        }
        let used = measured.min(self.budget_us);
        let left = self.budget_us - used;
        ((left * 100) / self.budget_us) as u8
    }
}

// ===========================================================================
// A171 — 输入子系统全栈可观测（指标累加）
// ===========================================================================

#[derive(Clone, Copy, Debug)]
pub struct InputMetrics {
    pub events: u64,
    pub drops: u64,
    pub sum_latency_us: u64,
    pub samples: u32,
}

impl InputMetrics {
    pub const fn new() -> InputMetrics {
        InputMetrics { events: 0, drops: 0, sum_latency_us: 0, samples: 0 }
    }

    pub fn record_event(&mut self, latency_us: u32) {
        self.events = self.events.saturating_add(1);
        self.sum_latency_us = self.sum_latency_us.saturating_add(latency_us as u64);
        self.samples = self.samples.saturating_add(1);
    }

    pub fn record_drop(&mut self) {
        self.drops = self.drops.saturating_add(1);
    }

    pub fn avg_latency_us(&self) -> u32 {
        if self.samples == 0 {
            0
        } else {
            (self.sum_latency_us / self.samples as u64) as u32
        }
    }
}

pub fn observe_event(m: &mut InputMetrics, latency_us: u32) {
    m.record_event(latency_us);
}

// ===========================================================================
// A172 — 输入子系统全栈模糊测试
// ===========================================================================

/// 对任意字节流跑模糊投喂并返回（处理数, 丢棄数），永不恐慌。
pub fn input_fuzz_run(pump: &mut EventPump, bytes: &[u8]) -> (u32, u64) {
    let n = fuzz_feed(pump, bytes, 0);
    (n, pump.dropped())
}

// ===========================================================================
// A173 — 输入子系统全栈文档（功能索引）
// ===========================================================================

pub const INPUT_FEATURES: [&'static str; 25] = [
    "A151 keyboard scancode",
    "A152 keymap US layout",
    "A153 mouse accumulation",
    "A154 touchpad tracking",
    "A155 gamepad buttons",
    "A156 hid descriptor parse",
    "A157 event pump ring",
    "A158 event dispatch focus",
    "A159 key repeat throttle",
    "A160 hotkey modifiers",
    "A161 device hotplug",
    "A162 latency budget",
    "A163 device list",
    "A164 input fuzz guard",
    "A165 record replay",
    "A166 accessibility",
    "A167 gesture state machine",
    "A168 core invariant",
    "A169 subsystem self-check",
    "A170 perf budget",
    "A171 observability",
    "A172 fuzz",
    "A173 docs",
    "A174 degrade chain",
    "A175 domain self-test",
];

pub fn feature_count() -> usize {
    INPUT_FEATURES.len()
}

pub fn feature_name(i: usize) -> Option<&'static str> {
    if i < INPUT_FEATURES.len() {
        Some(INPUT_FEATURES[i])
    } else {
        None
    }
}

// ===========================================================================
// A174 — 输入子系统全栈降级链
// ===========================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DegradeLevel {
    Full = 0,
    Fallback = 1,
    Disabled = 2,
}

/// 设备不健康时按类型降级：键盘/鼠标可软降级，其余直接停用。
pub fn degrade_level(kind: DeviceKind, healthy: bool) -> DegradeLevel {
    if healthy {
        DegradeLevel::Full
    } else {
        match kind {
            DeviceKind::Keyboard | DeviceKind::Mouse => DegradeLevel::Fallback,
            _ => DegradeLevel::Disabled,
        }
    }
}

pub fn fallback_hint(kind: DeviceKind) -> &'static str {
    match kind {
        DeviceKind::Keyboard => "on-screen keyboard",
        DeviceKind::Mouse => "keyboard navigation",
        _ => "ignore input",
    }
}

// ===========================================================================
// A175 — 输入子系统全栈域自检收口
// ===========================================================================

/// 域自检入口：构建并填好 A151~A175 全部检查项。
pub fn run_ainput_checks() -> CheckSet {
    let mut set = CheckSet::new("aurora-input");

    // A151 — 扫描码翻译（含 make/break）
    set.add(
        "A151 scancode",
        decode_scancode(0x1E) == Some(KeySym::A)
            && decode_scancode(0x39) == Some(KeySym::Space)
            && decode_scancode(0x48) == Some(KeySym::Up)
            && is_break(0x81),
        "set1 translate",
    );
    // A151 — 非法扫描码拒绝
    set.add("A151 unknown", decode_scancode(0x57) == None && decode_scancode(0xFF) == None, "reject");

    // A152 — 键位映射（Shift/Caps）
    set.add(
        "A152 keymap",
        char_for(KeySym::A, false, false) == b'a'
            && char_for(KeySym::A, true, false) == b'A'
            && char_for(KeySym::A, false, true) == b'A'
            && char_for(KeySym::A, true, true) == b'a'
            && char_for(KeySym::D1, true, false) == b'!'
            && char_for(KeySym::Space, false, false) == b' '
            && char_for(KeySym::Enter, false, false) == 0,
        "shift/caps",
    );

    // A153 — 鼠标累积 + 钳制 + 滚轮
    let mut m = MouseState::new();
    m.apply(1000, 1000, 5, 100, 100);
    let mut m2 = MouseState::new();
    m2.apply(-1000, -1000, 0, 100, 100);
    set.add(
        "A153 mouse",
        m.x == 100 && m.y == 100 && m.wheel == 5 && m2.x == 0 && m2.y == 0,
        "clamp + wheel",
    );

    // A154 — 触控板跟踪
    let mut pad = TouchpadState::new();
    pad.update(0, 10, 10, true);
    pad.update(1, 20, 10, true);
    let track_ok = pad.finger_count() == 2 && pad.get(0).unwrap().x == 10;
    pad.update(0, 0, 0, false);
    let lift_ok = pad.finger_count() == 1;
    set.add("A154 touchpad", track_ok && lift_ok, "multi-touch");

    // A155 — 手柄位掩码
    let mut gp = Gamepad::new();
    gp.press(GamepadButton::A);
    gp.press(GamepadButton::Y);
    gp.press(GamepadButton::Dup);
    let gp_ok = gp.pressed(GamepadButton::A)
        && gp.pressed(GamepadButton::Y)
        && !gp.pressed(GamepadButton::B)
        && gp.dpad() == (0, -1)
        && GamepadButton::A.mask() == 1
        && GamepadButton::Y.mask() == 1 << 3;
    set.add("A155 gamepad", gp_ok, "button mask");

    // A156 — HID 解析
    let desc = [
        0x05u8, 0x01, 0x09, 0x02, 0xA1, 0x01, 0x05, 0x09, 0x19, 0x01, 0x29, 0x03, 0x15, 0x00, 0x25,
        0x01, 0x95, 0x03, 0x75, 0x01,
    ];
    let hid_ok = match parse_hid_report_descriptor(&desc) {
        Some(i) => i.usage_page == 1 && i.usage == 2 && i.button_count == 3,
        None => false,
    };
    set.add(
        "A156 hid parse",
        hid_ok && parse_hid_report_descriptor(&[0u8; 1]) == None,
        "usage page/buttons",
    );

    // A157 — 事件泵环形队列（满丢最旧）
    let mut pump = EventPump::new();
    let mut k = 0;
    while k < EVENT_CAP {
        pump.push(Event::none());
        k += 1;
    }
    let full = pump.is_full() && pump.len() == EVENT_CAP;
    pump.push(Event::none());
    let drop_ok = pump.dropped() == 1 && pump.len() == EVENT_CAP;
    let pop_ok = pump.pop().is_some();
    set.add("A157 event pump", full && drop_ok && pop_ok, "ring drop-oldest");

    // A158 — 分发与订阅
    let mut sub = SubscriberTable::new();
    let sub_ok = sub.subscribe(1)
        && sub.subscribe(2)
        && sub.delivers(1)
        && !sub.delivers(9)
        && sub.unsubscribe(1)
        && !sub.delivers(1);
    let route_ok = route_focus(Event::none(), 42) == 42;
    set.add("A158 dispatch", sub_ok && route_ok, "focus routing");

    // A159 — 重复节流
    let mut rep = RepeatState::new();
    let first = rep.tick(0x1E, true, 0, 500, 100);
    let a = rep.tick(0x1E, true, 200, 500, 100);
    let b = rep.tick(0x1E, true, 400, 500, 100);
    let c = rep.tick(0x1E, true, 100, 500, 100);
    let rel = rep.tick(0x1E, false, 0, 500, 100);
    set.add(
        "A159 repeat",
        !first && !a && b && c && !rel,
        "throttle",
    );

    // A160 — 组合键 + 修饰键
    let mut mods = ModState::new();
    mods.update(KeySym::LCtrl, true);
    mods.update(KeySym::LShift, true);
    let hot = match_hotkey(
        ModState { ctrl: true, alt: false, shift: false, meta: false },
        KeySym::K,
        mods,
        KeySym::K,
    );
    let hot2 = match_hotkey(
        ModState { ctrl: true, alt: false, shift: true, meta: false },
        KeySym::K,
        mods,
        KeySym::K,
    );
    set.add("A160 hotkey", mods.ctrl && mods.shift && !hot && hot2, "mods + combo");

    // A161 — 热插拔
    let mut reg = DeviceRegistry::new();
    let plug1 = reg.plug(InputDevice { id: 1, kind: DeviceKind::Keyboard, online: true, name: "kbd" });
    let plug2 = reg.plug(InputDevice { id: 2, kind: DeviceKind::Mouse, online: true, name: "mouse" });
    let unplug = reg.unplug(2);
    let online_ok = reg.set_online(1, false) && !reg.set_online(9, true);
    set.add(
        "A161 hotplug",
        plug1 && plug2 && unplug && online_ok
            && reg.count_kind(DeviceKind::Keyboard) == 0
            && reg.count_kind(DeviceKind::Mouse) == 0
            && reg.len() == 1,
        "plug/unplug/online",
    );

    // A162 — 延迟预算 + 单调
    let mut mono = Monotonic::new();
    let t1 = mono.tick();
    let t2 = mono.tick();
    set.add(
        "A162 latency",
        latency_ok(5000, INPUT_LATENCY_BUDGET_US)
            && !latency_ok(9000, INPUT_LATENCY_BUDGET_US)
            && t2 == t1 + 1,
        "monotonic + budget",
    );

    // A163 — 设备列表
    let mut reg3 = DeviceRegistry::new();
    reg3.plug(InputDevice { id: 1, kind: DeviceKind::Keyboard, online: true, name: "k" });
    reg3.plug(InputDevice { id: 2, kind: DeviceKind::Keyboard, online: false, name: "k2" });
    let mut ids = [0u8; 8];
    let listed = list_devices(&reg3, DeviceKind::Keyboard, &mut ids);
    set.add(
        "A163 device list",
        reg3.count_kind(DeviceKind::Keyboard) == 1 && listed == 1 && ids[0] == 1,
        "online enumeration",
    );

    // A164 — 模糊测试守卫
    let mut fp = EventPump::new();
    let fed = fuzz_feed(&mut fp, &[0x1E, 0x39, 0xFF, 0x81], 0);
    let valid = validate_event(Event { kind: InputKind::KeyDown, code: 0, x: 0, y: 0, value: 0, device: 0, stamp: 0 });
    set.add("A164 fuzz", fed == 4 && fp.len() == 3 && valid, "guard");

    // A165 — 录制回放
    let mut rec = InputRecorder::new();
    let mut ri = 0;
    while ri < 5 {
        rec.record(Event { kind: InputKind::KeyDown, code: ri as u16, x: 0, y: 0, value: 0, device: 0, stamp: 0 });
        ri += 1;
    }
    let mut rp = EventPump::new();
    let replayed = rec.replay(&mut rp);
    set.add("A165 recorder", rec.len() == 5 && replayed == 5 && rp.len() == 5, "record/replay");

    // A166 — 无障碍
    let mut acc = AccessState::new();
    let m1 = acc.sticky_update(KeySym::LShift, true);
    let m2 = acc.sticky_update(KeySym::LShift, true);
    let slow = slow_key_ok(0, 100, 50) && !slow_key_ok(0, 30, 50);
    set.add("A166 a11y", m1.shift && !m2.shift && slow, "sticky + slow");

    // A167 — 手势
    let mut gpad = TouchpadState::new();
    let mut gst = GestureState::new();
    gpad.update(0, 10, 10, true);
    gpad.update(1, 20, 10, true);
    let _ = gst.update(&gpad);
    gpad.update(0, 40, 10, true);
    gpad.update(1, 50, 10, true);
    let gr = gst.update(&gpad);
    let scroll_ok = match gr {
        Gesture::Scroll(_, _) => true,
        _ => false,
    };
    set.add("A167 gesture", scroll_ok, "two-finger scroll");

    // A168 — 核心不变量
    set.add("A168 core invariant", input_core_invariant(), "invariants");

    // A169 — 子系统自检聚合
    set.add(
        "A169 subsystem",
        feature_count() == 25 && input_core_invariant(),
        "aggregate",
    );

    // A170 — 性能预算
    let pb = PerfBudget::new(8000);
    set.add(
        "A170 perf budget",
        pb.within(4000) && !pb.within(9000) && pb.score(0) == 100,
        "within + score",
    );

    // A171 — 可观测
    let mut met = InputMetrics::new();
    met.record_event(2000);
    met.record_event(6000);
    met.record_drop();
    set.add(
        "A171 metrics",
        met.events == 2 && met.drops == 1 && met.avg_latency_us() == 4000,
        "avg latency",
    );

    // A172 — 模糊测试
    let mut fz = EventPump::new();
    let (fn2, _) = input_fuzz_run(&mut fz, &[0x1E, 0x30, 0x81]);
    set.add("A172 fuzz", fn2 == 3 && fz.len() == 3, "stream");

    // A173 — 文档
    set.add(
        "A173 docs",
        feature_count() == 25 && feature_name(0).map(|s| s.len() > 0).unwrap_or(false),
        "feature doc",
    );

    // A174 — 降级链
    set.add(
        "A174 degrade",
        degrade_level(DeviceKind::Keyboard, true) == DegradeLevel::Full
            && degrade_level(DeviceKind::Keyboard, false) == DegradeLevel::Fallback
            && degrade_level(DeviceKind::Gamepad, false) == DegradeLevel::Disabled,
        "fallback chain",
    );

    // A175 — 域自检收口
    set.add("A175 domain self-test", input_core_invariant() && set.len() >= 25, "wrap");

    set
}

// ===========================================================================
// 单元测试
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a151_scancode_decode() {
        assert_eq!(decode_scancode(0x1E), Some(KeySym::A));
        assert_eq!(decode_scancode(0x39), Some(KeySym::Space));
        assert_eq!(decode_scancode(0x48), Some(KeySym::Up));
        assert_eq!(decode_scancode(0x3B), Some(KeySym::F1));
        assert_eq!(decode_scancode(0x3A), Some(KeySym::CapsLock));
    }

    #[test]
    fn a151_break_bit_ignored() {
        // 0x81 == Esc 的断码；剥离后应得 Esc。
        assert!(is_break(0x81));
        assert_eq!(decode_scancode(0x81), Some(KeySym::Esc));
        assert_eq!(decode_key_event(0x81), Some((KeySym::Esc, false)));
        assert_eq!(decode_key_event(0x1E), Some((KeySym::A, true)));
    }

    #[test]
    fn a151_unknown_scancode_none() {
        assert_eq!(decode_scancode(0x57), None);
        assert_eq!(decode_scancode(0xFF), None);
        assert_eq!(decode_scancode(0x00), None);
    }

    #[test]
    fn a152_letter_shift_caps() {
        assert_eq!(char_for(KeySym::A, false, false), b'a');
        assert_eq!(char_for(KeySym::A, true, false), b'A');
        assert_eq!(char_for(KeySym::A, false, true), b'A');
        assert_eq!(char_for(KeySym::A, true, true), b'a');
        assert_eq!(char_for(KeySym::Z, true, false), b'Z');
        assert_eq!(char_for(KeySym::M, false, true), b'M');
    }

    #[test]
    fn a152_digit_and_symbol_shift() {
        assert_eq!(char_for(KeySym::D1, false, false), b'1');
        assert_eq!(char_for(KeySym::D1, true, false), b'!');
        assert_eq!(char_for(KeySym::D9, true, false), b'(');
        assert_eq!(char_for(KeySym::Minus, true, false), b'_');
        assert_eq!(char_for(KeySym::Slash, true, false), b'?');
        assert_eq!(char_for(KeySym::Space, true, true), b' ');
    }

    #[test]
    fn a152_control_keys_no_char() {
        assert_eq!(char_for(KeySym::Enter, false, false), 0);
        assert_eq!(char_for(KeySym::Backspace, true, false), 0);
        assert_eq!(char_for(KeySym::Esc, false, true), 0);
    }

    #[test]
    fn a153_mouse_clamp() {
        let mut m = MouseState::new();
        m.apply(1000, 1000, 5, 100, 100);
        assert_eq!(m.x, 100);
        assert_eq!(m.y, 100);
        assert_eq!(m.wheel, 5);
    }

    #[test]
    fn a153_mouse_negative_and_wheel() {
        let mut m = MouseState::new();
        m.apply(-1000, -1000, -3, 100, 100);
        assert_eq!(m.x, 0);
        assert_eq!(m.y, 0);
        assert_eq!(m.wheel, -3);
        m.apply(50, 50, 2, 100, 100);
        assert_eq!(m.x, 50);
        assert_eq!(m.wheel, -1);
    }

    #[test]
    fn a154_touchpad_track_and_lift() {
        let mut pad = TouchpadState::new();
        pad.update(0, 10, 10, true);
        pad.update(1, 20, 10, true);
        pad.update(2, 30, 10, true);
        assert_eq!(pad.finger_count(), 3);
        assert_eq!(pad.get(1).unwrap().x, 20);
        pad.update(1, 25, 12, true);
        assert_eq!(pad.get(1).unwrap().x, 25);
        pad.update(0, 0, 0, false);
        assert_eq!(pad.finger_count(), 2);
        // 超出 4 指忽略
        pad.update(3, 1, 1, true);
        pad.update(4, 2, 2, true);
        pad.update(5, 3, 3, true);
        assert_eq!(pad.finger_count(), 4);
        assert!(pad.get(4).is_none());
    }

    #[test]
    fn a155_gamepad_press_and_dpad() {
        let mut gp = Gamepad::new();
        gp.press(GamepadButton::A);
        gp.press(GamepadButton::B);
        gp.release(GamepadButton::B);
        assert!(gp.pressed(GamepadButton::A));
        assert!(!gp.pressed(GamepadButton::B));
        gp.press(GamepadButton::Dleft);
        gp.press(GamepadButton::Dup);
        assert_eq!(gp.dpad(), (-1, -1));
        gp.release(GamepadButton::Dleft);
        assert_eq!(gp.dpad(), (0, -1));
    }

    #[test]
    fn a155_gamepad_mask_bits() {
        assert_eq!(GamepadButton::A.mask(), 1 << 0);
        assert_eq!(GamepadButton::X.mask(), 1 << 2);
        assert_eq!(GamepadButton::Y.mask(), 1 << 3);
        assert_eq!(GamepadButton::Dup.mask(), 1 << 8);
    }

    #[test]
    fn a156_hid_parse() {
        let desc = [
            0x05u8, 0x01, 0x09, 0x02, 0xA1, 0x01, 0x05, 0x09, 0x19, 0x01, 0x29, 0x03, 0x15, 0x00,
            0x25, 0x01, 0x95, 0x03, 0x75, 0x01,
        ];
        let info = parse_hid_report_descriptor(&desc).expect("hid");
        assert_eq!(info.usage_page, 1);
        assert_eq!(info.usage, 2);
        assert_eq!(info.button_count, 3);
        let short = parse_hid_report_descriptor(&[0x09u8, 0x02]);
        assert!(short.is_none());
    }

    #[test]
    fn a156_hid_no_page() {
        let d = [0x09u8, 0x02, 0x95, 0x04];
        assert!(parse_hid_report_descriptor(&d).is_none());
    }

    #[test]
    fn a157_queue_full_drops_oldest() {
        let mut pump = EventPump::new();
        let mut i = 0;
        while i < EVENT_CAP {
            pump.push(Event { kind: InputKind::KeyDown, code: i as u16, x: 0, y: 0, value: 0, device: 0, stamp: 0 });
            i += 1;
        }
        assert!(pump.is_full());
        assert_eq!(pump.len(), EVENT_CAP);
        pump.push(Event { kind: InputKind::KeyDown, code: 0xF001, x: 0, y: 0, value: 0, device: 0, stamp: 0 });
        assert_eq!(pump.dropped(), 1);
        assert_eq!(pump.len(), EVENT_CAP);
        // 最旧被丢弃：peek(0) 是刚写入的新事件
        assert_eq!(pump.peek(0).unwrap().code, 0xF001);
    }

    #[test]
    fn a157_queue_pop_order() {
        let mut pump = EventPump::new();
        pump.push(Event { kind: InputKind::KeyDown, code: 1, x: 0, y: 0, value: 0, device: 0, stamp: 0 });
        pump.push(Event { kind: InputKind::KeyDown, code: 2, x: 0, y: 0, value: 0, device: 0, stamp: 0 });
        assert_eq!(pump.pop().unwrap().code, 1);
        assert_eq!(pump.pop().unwrap().code, 2);
        assert!(pump.pop().is_none());
    }

    #[test]
    fn a158_route_focus() {
        let e = Event::none();
        assert_eq!(route_focus(e, 7), 7);
        assert_eq!(route_focus(e, 0xFFFF), 0xFFFF);
    }

    #[test]
    fn a158_subscribers() {
        let mut s = SubscriberTable::new();
        assert!(s.subscribe(1));
        assert!(s.subscribe(2));
        assert!(s.delivers(1));
        assert!(!s.delivers(9));
        assert!(s.subscribe(2)); // 重复订阅仍成功
        assert!(s.unsubscribe(1));
        assert!(!s.delivers(1));
        assert!(!s.unsubscribe(1));
        // 超出容量拒绝
        let mut full = SubscriberTable::new();
        let mut ok = true;
        let mut w = 0;
        while w < MAX_SUBS + 2 {
            if !full.subscribe(w as u32) && w < MAX_SUBS {
                ok = false;
            }
            w += 1;
        }
        assert!(ok);
        assert_eq!(full.len(), MAX_SUBS);
    }

    #[test]
    fn a159_repeat_after_delay() {
        let mut r = RepeatState::new();
        assert!(!r.tick(0x1E, true, 0, 500, 100)); // 首次按下不重复
        assert!(!r.tick(0x1E, true, 200, 500, 100)); // 200 < 500
        assert!(r.tick(0x1E, true, 400, 500, 100)); // 累计 600 >= 500
        assert!(r.tick(0x1E, true, 100, 500, 100)); // 累计 700 >= 600
        assert!(!r.tick(0x1E, false, 0, 500, 100)); // 松开
        assert!(!r.tick(0x1E, true, 0, 500, 100)); // 重新按下不立即重复
    }

    #[test]
    fn a159_repeat_switch_key_resets() {
        let mut r = RepeatState::new();
        r.tick(0x1E, true, 0, 500, 100);
        r.tick(0x1E, true, 600, 500, 100);
        // 换键后需重新等待 delay
        assert!(!r.tick(0x2F, true, 100, 500, 100));
        assert!(!r.tick(0x2F, true, 300, 500, 100));
    }

    #[test]
    fn a160_hotkey_match() {
        let mut m = ModState::new();
        m.update(KeySym::LCtrl, true);
        let combo = ModState { ctrl: true, alt: false, shift: false, meta: false };
        assert!(match_hotkey(combo, KeySym::K, m, KeySym::K));
        assert!(!match_hotkey(combo, KeySym::C, m, KeySym::K));
        m.update(KeySym::LShift, true);
        assert!(!match_hotkey(combo, KeySym::K, m, KeySym::K)); // 多了 shift
    }

    #[test]
    fn a160_modstate_update() {
        let mut m = ModState::new();
        m.update(KeySym::LCtrl, true);
        m.update(KeySym::RAlt, true);
        assert!(m.ctrl && m.alt && !m.shift && !m.meta);
        m.update(KeySym::LCtrl, false);
        assert!(!m.ctrl);
        assert!(m.any());
    }

    #[test]
    fn a161_device_plug_unplug() {
        let mut reg = DeviceRegistry::new();
        assert!(reg.plug(InputDevice { id: 1, kind: DeviceKind::Keyboard, online: true, name: "k" }));
        assert!(reg.plug(InputDevice { id: 2, kind: DeviceKind::Mouse, online: true, name: "m" }));
        assert!(reg.unplug(2));
        assert!(!reg.unplug(2));
        assert!(reg.set_online(1, false));
        assert!(!reg.set_online(9, true));
        assert_eq!(reg.count_kind(DeviceKind::Keyboard), 0);
        assert!(reg.set_online(1, true));
        assert_eq!(reg.count_kind(DeviceKind::Keyboard), 1);
        assert_eq!(reg.count_kind(DeviceKind::Mouse), 0);
        // 重复 plug 同 id 替换
        assert!(reg.plug(InputDevice { id: 1, kind: DeviceKind::Keyboard, online: true, name: "k2" }));
        assert_eq!(reg.len(), 1);
    }

    #[test]
    fn a161_device_registry_capacity() {
        let mut reg = DeviceRegistry::new();
        let mut ok = true;
        let mut id = 0u8;
        while id < (MAX_DEVICES as u8) + 4 {
            if !reg.plug(InputDevice { id, kind: DeviceKind::Hid, online: true, name: "h" }) && id < MAX_DEVICES as u8 {
                ok = false;
            }
            id = id.wrapping_add(1);
        }
        assert!(ok);
        assert_eq!(reg.len(), MAX_DEVICES);
    }

    #[test]
    fn a162_latency_budget() {
        assert!(latency_ok(5000, INPUT_LATENCY_BUDGET_US));
        assert!(!latency_ok(9000, INPUT_LATENCY_BUDGET_US));
        let mut mono = Monotonic::new();
        let t1 = mono.tick();
        let t2 = mono.tick();
        assert_eq!(t2, t1 + 1);
    }

    #[test]
    fn a163_device_list_online_only() {
        let mut reg = DeviceRegistry::new();
        reg.plug(InputDevice { id: 1, kind: DeviceKind::Keyboard, online: true, name: "k" });
        reg.plug(InputDevice { id: 2, kind: DeviceKind::Keyboard, online: false, name: "k2" });
        assert_eq!(reg.count_kind(DeviceKind::Keyboard), 1);
        let mut ids = [0u8; 4];
        let n = list_devices(&reg, DeviceKind::Keyboard, &mut ids);
        assert_eq!(n, 1);
        assert_eq!(ids[0], 1);
        let mut empty = [0u8; 2];
        assert_eq!(list_devices(&reg, DeviceKind::Mouse, &mut empty), 0);
    }

    #[test]
    fn a164_fuzz_feed() {
        let mut pump = EventPump::new();
        let n = fuzz_feed(&mut pump, &[0x1E, 0x39, 0xFF, 0x81], 0);
        assert_eq!(n, 4);
        assert_eq!(pump.len(), 3); // 0xFF 不产生事件
        assert!(validate_event(Event::none()));
        assert!(!validate_event(Event { device: 0xFF, ..Event::none() }));
    }

    #[test]
    fn a165_recorder_replay() {
        let mut rec = InputRecorder::new();
        let mut i = 0;
        while i < 5 {
            rec.record(Event { kind: InputKind::KeyDown, code: i as u16, x: 0, y: 0, value: 0, device: 0, stamp: 0 });
            i += 1;
        }
        assert_eq!(rec.len(), 5);
        let mut pump = EventPump::new();
        let n = rec.replay(&mut pump);
        assert_eq!(n, 5);
        assert_eq!(pump.len(), 5);
        assert_eq!(pump.pop().unwrap().code, 0);
    }

    #[test]
    fn a165_recorder_overflow_ring() {
        let mut rec = InputRecorder::new();
        let mut i = 0u16;
        while i < (RECORD_CAP as u16) + 8 {
            rec.record(Event { kind: InputKind::KeyDown, code: i, x: 0, y: 0, value: 0, device: 0, stamp: 0 });
            i += 1;
        }
        assert_eq!(rec.len(), RECORD_CAP);
        let mut pump = EventPump::new();
        let n = rec.replay(&mut pump);
        assert_eq!(n, RECORD_CAP);
    }

    #[test]
    fn a166_sticky_keys() {
        let mut acc = AccessState::new();
        let m1 = acc.sticky_update(KeySym::LShift, true);
        assert!(m1.shift);
        let m2 = acc.sticky_update(KeySym::LShift, true);
        assert!(!m2.shift);
        // 非修饰键不影响
        let m3 = acc.sticky_update(KeySym::A, true);
        assert!(!m3.shift);
    }

    #[test]
    fn a166_slow_keys() {
        assert!(slow_key_ok(0, 100, 50));
        assert!(!slow_key_ok(0, 30, 50));
        assert!(slow_key_ok(1000, 1000 + 50, 50));
    }

    #[test]
    fn a167_gesture_scroll() {
        let mut pad = TouchpadState::new();
        let mut g = GestureState::new();
        pad.update(0, 10, 10, true);
        pad.update(1, 20, 10, true);
        let _ = g.update(&pad);
        pad.update(0, 40, 10, true);
        pad.update(1, 50, 10, true);
        let gr = g.update(&pad);
        let is_scroll = match gr {
            Gesture::Scroll(_, _) => true,
            _ => false,
        };
        assert!(is_scroll);
    }

    #[test]
    fn a167_gesture_tap2() {
        let mut pad = TouchpadState::new();
        let mut g = GestureState::new();
        pad.update(0, 10, 10, true);
        pad.update(1, 20, 10, true);
        let _ = g.update(&pad);
        // 不移动直接抬起
        pad.update(0, 10, 10, false);
        pad.update(1, 20, 10, false);
        let gr = g.update(&pad);
        let is_tap = match gr {
            Gesture::Tap2 => true,
            _ => false,
        };
        assert!(is_tap);
    }

    #[test]
    fn a167_gesture_swipe3up() {
        let mut pad = TouchpadState::new();
        let mut g = GestureState::new();
        pad.update(0, 10, 100, true);
        pad.update(1, 20, 100, true);
        pad.update(2, 30, 100, true);
        let _ = g.update(&pad);
        // 整体上移
        pad.update(0, 10, 80, true);
        pad.update(1, 20, 80, true);
        pad.update(2, 30, 80, true);
        let gr = g.update(&pad);
        let is_swipe = match gr {
            Gesture::Swipe3Up => true,
            _ => false,
        };
        assert!(is_swipe);
    }

    #[test]
    fn a167_gesture_interrupted() {
        let mut pad = TouchpadState::new();
        let mut g = GestureState::new();
        pad.update(0, 10, 10, true);
        pad.update(1, 20, 10, true);
        let _ = g.update(&pad);
        // 中断：只剩一指
        pad.update(1, 20, 10, false);
        let gr = g.update(&pad);
        assert_eq!(gr, Gesture::None);
    }

    #[test]
    fn a168_core_invariant() {
        assert!(input_core_invariant());
    }

    #[test]
    fn a169_feature_index_25() {
        assert_eq!(feature_count(), 25);
        assert!(feature_name(0).is_some());
        assert!(feature_name(24).is_some());
        assert!(feature_name(25).is_none());
    }

    #[test]
    fn a170_perf_budget() {
        let pb = PerfBudget::new(8000);
        assert!(pb.within(4000));
        assert!(!pb.within(9000));
        assert_eq!(pb.score(0), 100);
        assert_eq!(pb.score(8000), 0);
        assert_eq!(pb.score(4000), 50);
    }

    #[test]
    fn a171_metrics_avg() {
        let mut met = InputMetrics::new();
        met.record_event(2000);
        met.record_event(6000);
        met.record_drop();
        assert_eq!(met.events, 2);
        assert_eq!(met.drops, 1);
        assert_eq!(met.avg_latency_us(), 4000);
        observe_event(&mut met, 0);
        assert_eq!(met.avg_latency_us(), 2666); // (2000+6000+0)/3
    }

    #[test]
    fn a172_fuzz_run() {
        let mut pump = EventPump::new();
        let (n, d) = input_fuzz_run(&mut pump, &[0x1E, 0x30, 0x81, 0xFF]);
        assert_eq!(n, 4);
        assert_eq!(pump.len(), 3);
        assert_eq!(d, 0);
    }

    #[test]
    fn a173_docs_nonempty() {
        let mut all_nonempty = true;
        let mut i = 0;
        while i < feature_count() {
            if let Some(s) = feature_name(i) {
                if s.is_empty() {
                    all_nonempty = false;
                }
            } else {
                all_nonempty = false;
            }
            i += 1;
        }
        assert!(all_nonempty);
    }

    #[test]
    fn a174_degrade_levels() {
        assert_eq!(degrade_level(DeviceKind::Keyboard, true), DegradeLevel::Full);
        assert_eq!(degrade_level(DeviceKind::Keyboard, false), DegradeLevel::Fallback);
        assert_eq!(degrade_level(DeviceKind::Mouse, false), DegradeLevel::Fallback);
        assert_eq!(degrade_level(DeviceKind::Gamepad, false), DegradeLevel::Disabled);
        assert_eq!(fallback_hint(DeviceKind::Keyboard), "on-screen keyboard");
    }

    #[test]
    fn a175_domain_self_test() {
        let set = run_ainput_checks();
        assert!(set.all_passed());
        assert!(set.len() >= 25);
        let (passed, failed) = set.tally();
        assert_eq!(failed, 0);
        assert!(passed >= 25);
    }
}
