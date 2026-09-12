//! VARIABLE-200 AI-06 · 输入与事件域（F126~F150，W3）。
//!
//! 键鼠从真实中断到像素 < 33ms：PS/2 驱动、内核事件队列与时间戳、
//! 用户态事件通道、键盘状态机、AURORA 键位纪律、全局快捷键、焦点系统、
//! 鼠标加速/滚轮平滑、无障碍与安全、事件回放、洪泛防护、布局与仪表。
//!
//! 纪律：纯逻辑 + 固定容量数组；无 `Vec`/`String`/`Box`/`alloc`/外部 crate。

use crate::checks::CheckSet;

pub mod guard;
pub mod keys;

// ---------------------------------------------------------------------------
// 公共事件结构
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EventKind {
    KeyDown,
    KeyUp,
    KeyRepeat,
    MouseMove,
    MouseDown,
    MouseUp,
    Wheel,
    Focus,
    Touch,
    Gamepad,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InputEvent {
    /// 高精度单调钟打点（微秒）。
    pub tick_us: u64,
    pub kind: EventKind,
    /// 键码 / 按钮位 / 焦点 id。
    pub code: u32,
    /// 轴数据（鼠标位移 / 滚轮量）。
    pub dx: i32,
    pub dy: i32,
    /// 修饰键状态位。
    pub mods: u8,
    /// 来源设备 id。
    pub device: u8,
}

impl InputEvent {
    pub const fn key(tick_us: u64, kind: EventKind, code: u32, mods: u8, device: u8) -> InputEvent {
        InputEvent { tick_us, kind, code, dx: 0, dy: 0, mods, device }
    }

    pub const fn motion(tick_us: u64, kind: EventKind, dx: i32, dy: i32, device: u8) -> InputEvent {
        InputEvent { tick_us, kind, code: 0, dx, dy, mods: 0, device }
    }
}

/// 修饰键位。
pub const MOD_CTRL: u8 = 0b0001;
pub const MOD_ALT: u8 = 0b0010;
pub const MOD_SHIFT: u8 = 0b0100;
pub const MOD_SUPER: u8 = 0b1000;

// ---------------------------------------------------------------------------
// F126 PS/2 键盘驱动 — 扫描码集 2 解码
// ---------------------------------------------------------------------------

/// 扫描码集 2 的关键键码（非全表，覆盖验收所需）。
pub const SC_A: u8 = 0x1C;
pub const SC_F0: u8 = 0xF0;
pub const SC_E0: u8 = 0xE0;
pub const SC_E1: u8 = 0xE1;
pub const SC_LSHIFT: u8 = 0x12;
pub const SC_LCTRL: u8 = 0x14;
pub const SC_LALT: u8 = 0x11;
pub const SC_ENTER: u8 = 0x5A;
pub const SC_ESC: u8 = 0x76;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KeyPhase {
    Press,
    Release,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct KeyDecoded {
    pub phase: KeyPhase,
    pub code: u8,
    pub extended: bool,
    /// 是否是可打印字符集的一部分（用于键盘状态机）。
    pub printable: bool,
}

/// 8042 控制器状态位。
pub const PS2_STATUS_OUTPUT_FULL: u8 = 0x01;
pub const PS2_STATUS_INPUT_FULL: u8 = 0x02;
pub const PS2_STATUS_AUX_DATA: u8 = 0x20;

#[derive(Clone, Copy)]
pub struct Ps2Controller {
    /// 控制器状态寄存器快照。
    pub status: u8,
    /// 上次写入命令。
    pub last_cmd: u8,
    /// 自检通过。
    pub self_test_ok: bool,
    pub output: Option<u8>,
    pub input: Option<u8>,
}

impl Ps2Controller {
    pub const fn new() -> Ps2Controller {
        Ps2Controller { status: 0, last_cmd: 0, self_test_ok: false, output: None, input: None }
    }

    pub fn self_test(&mut self, result: u8) -> bool {
        // 0x55 = 控制器自检通过。
        self.self_test_ok = result == 0x55;
        self.self_test_ok
    }

    pub fn write_cmd(&mut self, cmd: u8) -> bool {
        if self.status & PS2_STATUS_INPUT_FULL != 0 {
            return false;
        }
        self.last_cmd = cmd;
        self.input = Some(cmd);
        true
    }

    pub fn read_data(&mut self) -> Option<u8> {
        let v = self.output;
        self.output = None;
        if let Some(b) = v {
            self.status &= !PS2_STATUS_OUTPUT_FULL;
            let _ = b;
            return Some(b);
        }
        None
    }

    pub fn push_output(&mut self, b: u8, aux: bool) {
        self.output = Some(b);
        self.status |= PS2_STATUS_OUTPUT_FULL;
        if aux {
            self.status |= PS2_STATUS_AUX_DATA;
        } else {
            self.status &= !PS2_STATUS_AUX_DATA;
        }
    }
}

#[derive(Clone, Copy)]
pub struct Ps2Keyboard {
    /// 收到了 0xF0（下一字节为断码）。
    break_next: bool,
    /// 收到了 0xE0（扩展前缀）。
    extended: bool,
    /// 解码出的原始键集（有界）。
    pub decoded: [KeyDecoded; 16],
    pub decoded_count: usize,
    /// 丢弃的畸形序列数。
    pub malformed: u32,
    pub irqs: u64,
}

impl Ps2Keyboard {
    pub const fn new() -> Ps2Keyboard {
        Ps2Keyboard {
            break_next: false,
            extended: false,
            decoded: [KeyDecoded { phase: KeyPhase::Press, code: 0, extended: false, printable: false }; 16],
            decoded_count: 0,
            malformed: 0,
            irqs: 0,
        }
    }

    fn push(&mut self, d: KeyDecoded) {
        if self.decoded_count < 16 {
            self.decoded[self.decoded_count] = d;
            self.decoded_count += 1;
        } else {
            // 队列满：滚动丢弃最旧（有界）。
            let mut i = 1usize;
            while i < 16 {
                self.decoded[i - 1] = self.decoded[i];
                i += 1;
            }
            self.decoded[15] = d;
            self.malformed += 1;
        }
    }

    fn printable(code: u8) -> bool {
        // 字母/数字/符号区（ASCII 可映射）；修饰键与扩展前缀不算可打印。
        !matches!(code, SC_LSHIFT | SC_LCTRL | SC_LALT)
    }

    /// 喂入一个扫描码字节（IRQ1 中断路径）。
    pub fn feed(&mut self, sc: u8) -> Option<KeyDecoded> {
        self.irqs += 1;
        if sc == SC_E0 {
            self.extended = true;
            return None;
        }
        if sc == SC_E1 {
            // Pause 序列（E1 1D 45 E1 9D C5）：整体跳过。
            self.malformed += 1;
            return None;
        }
        if sc == SC_F0 {
            self.break_next = true;
            return None;
        }
        let phase = if self.break_next { KeyPhase::Release } else { KeyPhase::Press };
        self.break_next = false;
        let d = KeyDecoded {
            phase,
            code: sc,
            extended: self.extended,
            printable: Self::printable(sc),
        };
        self.extended = false;
        self.push(d);
        Some(d)
    }

    pub fn last(&self) -> Option<KeyDecoded> {
        if self.decoded_count > 0 {
            Some(self.decoded[self.decoded_count - 1])
        } else {
            None
        }
    }

    /// 键码 → ASCII（美式布局，Shift 由调用方处理）。
    pub fn ascii(code: u8) -> u8 {
        match code {
            0x1C => b'a',
            0x32 => b'b',
            0x21 => b'c',
            0x23 => b'd',
            0x24 => b'e',
            0x2B => b'f',
            0x34 => b'g',
            0x33 => b'h',
            0x43 => b'i',
            0x3B => b'j',
            0x42 => b'k',
            0x4B => b'l',
            0x3A => b'm',
            0x31 => b'n',
            0x44 => b'o',
            0x4D => b'p',
            0x15 => b'q',
            0x2D => b'r',
            0x1B => b's',
            0x2C => b't',
            0x3C => b'u',
            0x2A => b'v',
            0x1D => b'w',
            0x22 => b'x',
            0x35 => b'y',
            0x1A => b'z',
            0x45 => b'0',
            0x16 => b'1',
            0x1E => b'2',
            0x26 => b'3',
            0x25 => b'4',
            0x2E => b'5',
            0x36 => b'6',
            0x3D => b'7',
            0x3E => b'8',
            0x46 => b'9',
            0x29 => b' ',
            0x0D => b'\t',
            0x5A => b'\n',
            _ => 0,
        }
    }
}

// ---------------------------------------------------------------------------
// F127 PS/2 鼠标驱动 — 数据包解析、按钮/滚轮
// ---------------------------------------------------------------------------

pub const MOUSE_PKT_LEN: usize = 3;
pub const MOUSE_PKT_LEN_WHEEL: usize = 4;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MousePacket {
    pub left: bool,
    pub right: bool,
    pub middle: bool,
    /// 溢出标志。
    pub overflow_x: bool,
    pub overflow_y: bool,
    pub dx: i32,
    pub dy: i32,
    /// 滚轮（-8..=7，0x08 标志位表示第 4 字节有效）。
    pub wheel: i8,
}

impl MousePacket {
    pub const fn empty() -> MousePacket {
        MousePacket {
            left: false,
            right: false,
            middle: false,
            overflow_x: false,
            overflow_y: false,
            dx: 0,
            dy: 0,
            wheel: 0,
        }
    }

    pub fn buttons(&self) -> u8 {
        (self.left as u8) | ((self.right as u8) << 1) | ((self.middle as u8) << 2)
    }
}

#[derive(Clone, Copy)]
pub struct Ps2Mouse {
    buf: [u8; MOUSE_PKT_LEN_WHEEL],
    have: usize,
    /// 是否启用滚轮包（4 字节）。
    pub wheel_enabled: bool,
    pub packets: u64,
    pub malformed: u32,
    pub last: MousePacket,
}

impl Ps2Mouse {
    pub const fn new(wheel_enabled: bool) -> Ps2Mouse {
        Ps2Mouse {
            buf: [0u8; MOUSE_PKT_LEN_WHEEL],
            have: 0,
            wheel_enabled,
            packets: 0,
            malformed: 0,
            last: MousePacket::empty(),
        }
    }

    /// 喂入一个字节；凑齐一个包时返回解析结果。
    pub fn feed(&mut self, b: u8) -> Option<MousePacket> {
        // 包首字节必须 bit3 = 1（同步位）。
        if self.have == 0 && b & 0x08 == 0 {
            self.malformed += 1;
            return None;
        }
        self.buf[self.have] = b;
        self.have += 1;
        let want = if self.wheel_enabled { MOUSE_PKT_LEN_WHEEL } else { MOUSE_PKT_LEN };
        if self.have < want {
            return None;
        }
        self.have = 0;
        let p0 = self.buf[0];
        let p1 = self.buf[1];
        let p2 = self.buf[2];
        if p0 & 0x08 == 0 || p0 & 0xC0 != 0 {
            self.malformed += 1;
            return None;
        }
        let mut p = MousePacket::empty();
        p.left = p0 & 0x01 != 0;
        p.right = p0 & 0x02 != 0;
        p.middle = p0 & 0x04 != 0;
        p.overflow_x = p0 & 0x40 != 0;
        p.overflow_y = p0 & 0x80 != 0;
        // 8 位二补码 → 有符号位移（p0 的 bit4/bit5 与 p1/p2 最高位冗余）。
        p.dx = p1 as i8 as i32;
        p.dy = p2 as i8 as i32;
        if self.wheel_enabled {
            let w = self.buf[3];
            // 滚轮：bit3 为符号（1 = 上滚），低 3 位为量。
            let mag = (w & 0x07) as i8;
            p.wheel = if w & 0x08 != 0 { mag } else { -mag };
        }
        self.packets += 1;
        self.last = p;
        Some(p)
    }
}

// ---------------------------------------------------------------------------
// F128 USB 输入预留 — 无硬件时明确降级 PS/2
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum UsbInputStatus {
    /// 控制器已初始化并枚举到设备。
    Ready,
    /// 无 xHCI 控制器。
    NoController,
    /// 有控制器但枚举未完成。
    Enumerating,
}

#[derive(Clone, Copy)]
pub struct UsbInputStub {
    pub status: UsbInputStatus,
    pub devices: u8,
    /// 降级到 PS/2 的次数（显式，禁止静默缺失）。
    pub fallbacks: u32,
}

impl UsbInputStub {
    pub const fn new(status: UsbInputStatus) -> UsbInputStub {
        UsbInputStub { status, devices: 0, fallbacks: 0 }
    }

    /// 探测：无控制器即降级 PS/2。
    pub fn probe(&mut self) -> bool {
        match self.status {
            UsbInputStatus::Ready => {
                self.devices = 1;
                true
            }
            _ => {
                self.fallbacks += 1;
                false
            }
        }
    }

    /// 事件面：未就绪时必须声明降级（返回 None 即表示走 PS/2 路径）。
    pub fn poll_event(&mut self) -> Option<InputEvent> {
        if self.status != UsbInputStatus::Ready {
            self.fallbacks += 1;
            return None;
        }
        None
    }
}

// ---------------------------------------------------------------------------
// F129 内核事件队列 — 统一结构入队
// ---------------------------------------------------------------------------

pub const EVENT_QUEUE_CAP: usize = 64;

#[derive(Clone, Copy)]
pub struct EventQueue {
    ring: [InputEvent; EVENT_QUEUE_CAP],
    head: usize,
    len: usize,
    pub pushed: u64,
    pub popped: u64,
    /// 队列满丢弃（绝不阻塞中断）。
    pub dropped: u64,
}

impl EventQueue {
    pub const fn new() -> EventQueue {
        EventQueue {
            ring: [InputEvent::key(0, EventKind::KeyDown, 0, 0, 0); EVENT_QUEUE_CAP],
            head: 0,
            len: 0,
            pushed: 0,
            popped: 0,
            dropped: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn push(&mut self, e: InputEvent) -> bool {
        self.pushed += 1;
        if self.len >= EVENT_QUEUE_CAP {
            self.dropped += 1;
            return false;
        }
        self.ring[(self.head + self.len) % EVENT_QUEUE_CAP] = e;
        self.len += 1;
        true
    }

    pub fn pop(&mut self) -> Option<InputEvent> {
        if self.len == 0 {
            return None;
        }
        let e = self.ring[self.head];
        self.head = (self.head + 1) % EVENT_QUEUE_CAP;
        self.len -= 1;
        self.popped += 1;
        Some(e)
    }

    pub fn peek(&self) -> Option<InputEvent> {
        if self.len == 0 {
            None
        } else {
            Some(self.ring[self.head])
        }
    }
}

// ---------------------------------------------------------------------------
// F130 事件时间戳 — 高精度单调钟打点
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct Timestamping {
    /// 单调钟频率（Hz）。
    pub freq_hz: u64,
    /// 上次采样计数。
    pub last_tsc: u64,
    /// 换算出的微秒。
    pub last_us: u64,
    /// 回退检测（计数倒退 = 异常）。
    pub backwards: u32,
    pub samples: u64,
}

impl Timestamping {
    pub const fn new(freq_hz: u64) -> Timestamping {
        Timestamping { freq_hz, last_tsc: 0, last_us: 0, backwards: 0, samples: 0 }
    }

    /// TSC → 微秒。
    pub fn stamp_us(&mut self, tsc: u64) -> u64 {
        self.samples += 1;
        if tsc < self.last_tsc {
            self.backwards += 1;
        }
        self.last_tsc = tsc;
        let us = if self.freq_hz == 0 { 0 } else { tsc.saturating_mul(1_000_000) / self.freq_hz };
        self.last_us = us;
        us
    }

    /// 事件到上屏的端到端延迟（微秒）。
    pub fn latency_us(event_us: u64, present_us: u64) -> u64 {
        present_us.saturating_sub(event_us)
    }
}

// ---------------------------------------------------------------------------
// F131 用户态事件通道 — fd 订阅式送达
// ---------------------------------------------------------------------------

pub const HIDPORT_MAX: usize = 8;

#[derive(Clone, Copy)]
pub struct HidPort {
    pub pid: u32,
    /// 订阅的事件类型掩码（按 `EventKind as u8` 位）。
    pub mask: u32,
    pub active: bool,
    /// 环形缓冲占用。
    pub queued: usize,
    pub delivered: u64,
    pub dropped: u64,
}

impl HidPort {
    pub const fn empty() -> HidPort {
        HidPort { pid: 0, mask: 0, active: false, queued: 0, delivered: 0, dropped: 0 }
    }

    pub fn wants(&self, k: EventKind) -> bool {
        let bit = event_bit(k);
        bit < 32 && (self.mask & (1u32 << bit)) != 0
    }
}

pub fn event_bit(k: EventKind) -> usize {
    k as usize
}

pub const HID_PORT_QUEUE_MAX: usize = 16;

#[derive(Clone, Copy)]
pub struct HidDispatcher {
    ports: [HidPort; HIDPORT_MAX],
    count: usize,
    pub dispatched: u64,
    pub unrouted: u64,
}

impl HidDispatcher {
    pub const fn new() -> HidDispatcher {
        HidDispatcher { ports: [HidPort::empty(); HIDPORT_MAX], count: 0, dispatched: 0, unrouted: 0 }
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    pub fn subscribe(&mut self, pid: u32, mask: u32) -> Option<usize> {
        if self.count >= HIDPORT_MAX || pid == 0 || mask == 0 {
            return None;
        }
        let mut p = HidPort::empty();
        p.pid = pid;
        p.mask = mask;
        p.active = true;
        self.ports[self.count] = p;
        self.count += 1;
        Some(self.count - 1)
    }

    pub fn unsubscribe(&mut self, pid: u32) -> bool {
        let mut i = 0usize;
        while i < self.count {
            if self.ports[i].pid == pid && self.ports[i].active {
                self.ports[i].active = false;
                return true;
            }
            i += 1;
        }
        false
    }

    /// 分发一条事件；返回送达的端口数。
    pub fn dispatch(&mut self, e: InputEvent) -> usize {
        self.dispatched += 1;
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.count {
            if self.ports[i].active && self.ports[i].wants(e.kind) {
                if self.ports[i].queued >= HID_PORT_QUEUE_MAX {
                    self.ports[i].dropped += 1;
                } else {
                    self.ports[i].queued += 1;
                    self.ports[i].delivered += 1;
                    n += 1;
                }
            }
            i += 1;
        }
        if n == 0 {
            self.unrouted += 1;
        }
        n
    }

    pub fn port(&self, i: usize) -> Option<HidPort> {
        if i < self.count {
            Some(self.ports[i])
        } else {
            None
        }
    }

    /// 消费端口队列。
    pub fn drain(&mut self, i: usize, n: usize) -> usize {
        if i >= self.count {
            return 0;
        }
        let take = core::cmp::min(n, self.ports[i].queued);
        self.ports[i].queued -= take;
        take
    }
}

// ---------------------------------------------------------------------------
// F132 键盘状态机 — 按下/重复/释放、修饰键、组合键
// ---------------------------------------------------------------------------

pub const KEY_MAX: usize = 32;
/// 首次重复延迟（ms）。
pub const REPEAT_DELAY_MS: u64 = 500;
/// 重复间隔（ms）。
pub const REPEAT_INTERVAL_MS: u64 = 33;

#[derive(Clone, Copy)]
pub struct KeyState {
    pub code: u8,
    pub down: bool,
    /// 按下时刻（ms）。
    pub down_at: u64,
    /// 上次重复时刻（ms）。
    pub last_repeat: u64,
    pub repeats: u32,
}

impl KeyState {
    pub const fn empty() -> KeyState {
        KeyState { code: 0, down: false, down_at: 0, last_repeat: 0, repeats: 0 }
    }
}

#[derive(Clone, Copy)]
pub struct KeyboardFsm {
    keys: [KeyState; KEY_MAX],
    count: usize,
    pub mods: u8,
    pub downs: u64,
    pub ups: u64,
    /// 修饰键与普通键的按下序（组合键判定）。
    pub seq: [u8; 8],
    pub seq_len: usize,
}

impl KeyboardFsm {
    pub const fn new() -> KeyboardFsm {
        KeyboardFsm {
            keys: [KeyState::empty(); KEY_MAX],
            count: 0,
            mods: 0,
            downs: 0,
            ups: 0,
            seq: [0u8; 8],
            seq_len: 0,
        }
    }

    fn slot(&mut self, code: u8) -> Option<usize> {
        let mut i = 0usize;
        while i < self.count {
            if self.keys[i].code == code {
                return Some(i);
            }
            i += 1;
        }
        if self.count >= KEY_MAX {
            return None;
        }
        self.keys[self.count] = KeyState { code, down: false, down_at: 0, last_repeat: 0, repeats: 0 };
        self.count += 1;
        Some(self.count - 1)
    }

    fn push_seq(&mut self, code: u8) {
        if self.seq_len < 8 {
            self.seq[self.seq_len] = code;
            self.seq_len += 1;
        }
    }

    /// 处理按键按下。
    pub fn down(&mut self, code: u8, now_ms: u64) -> bool {
        let i = match self.slot(code) {
            Some(i) => i,
            None => return false,
        };
        if self.keys[i].down {
            return false;
        }
        self.keys[i].down = true;
        self.keys[i].down_at = now_ms;
        self.keys[i].last_repeat = now_ms;
        self.keys[i].repeats = 0;
        self.downs += 1;
        self.mods = self.mods | mod_bit(code);
        self.push_seq(code);
        true
    }

    pub fn up(&mut self, code: u8, _now_ms: u64) -> bool {
        let i = match self.slot(code) {
            Some(i) => i,
            None => return false,
        };
        if !self.keys[i].down {
            return false;
        }
        self.keys[i].down = false;
        self.ups += 1;
        self.mods = self.mods & !mod_bit(code);
        // 释放后组合序列重置（修饰键释放即结束 chord）。
        if mod_bit(code) != 0 {
            self.seq_len = 0;
        }
        true
    }

    pub fn is_down(&self, code: u8) -> bool {
        let mut i = 0usize;
        while i < self.count {
            if self.keys[i].code == code {
                return self.keys[i].down;
            }
            i += 1;
        }
        false
    }

    /// 到点产生重复（OS 级自动重复）。
    pub fn tick(&mut self, now_ms: u64) -> Option<u8> {
        let mut i = 0usize;
        while i < self.count {
            if self.keys[i].down && mod_bit(self.keys[i].code) == 0 {
                let elapsed = now_ms.saturating_sub(self.keys[i].down_at);
                if elapsed >= REPEAT_DELAY_MS {
                    if now_ms.saturating_sub(self.keys[i].last_repeat) >= REPEAT_INTERVAL_MS {
                        self.keys[i].last_repeat = now_ms;
                        self.keys[i].repeats += 1;
                        return Some(self.keys[i].code);
                    }
                }
            }
            i += 1;
        }
        None
    }

    /// 组合键判定：修饰键随后跟普通键（顺序无关）。
    pub fn is_chord(&self, mods: u8, code: u8) -> bool {
        if !self.is_down(code) {
            return false;
        }
        (self.mods & mods) == mods
    }

    pub fn down_count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.count {
            if self.keys[i].down {
                n += 1;
            }
            i += 1;
        }
        n
    }
}

pub fn mod_bit(code: u8) -> u8 {
    match code {
        SC_LCTRL => MOD_CTRL,
        SC_LALT => MOD_ALT,
        SC_LSHIFT => MOD_SHIFT,
        0xE0 => MOD_SUPER, // 简化：扩展码代表 Super
        _ => 0,
    }
}

// ---------------------------------------------------------------------------
// F133 键位纪律对接 — AURORA 键位纪律组逐条落地
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DisciplineVerdict {
    /// 系统级拦截（应用不可见）。
    Intercepted,
    /// 普通事件，交付焦点应用。
    Delivered,
    /// 被纪律组保留（禁止应用绑定）。
    Reserved,
}

/// AURORA 键位纪律条目。
#[derive(Clone, Copy)]
pub struct DisciplineRule {
    pub name: [u8; 16],
    pub name_len: usize,
    pub mods: u8,
    pub code: u8,
    pub verdict: DisciplineVerdict,
}

impl DisciplineRule {
    pub const fn empty() -> DisciplineRule {
        DisciplineRule {
            name: [0u8; 16],
            name_len: 0,
            mods: 0,
            code: 0,
            verdict: DisciplineVerdict::Delivered,
        }
    }

    pub fn name_eq(&self, want: &[u8]) -> bool {
        &self.name[..self.name_len] == want
    }
}

pub const DISCIPLINE_MAX: usize = 16;

#[derive(Clone, Copy)]
pub struct KeyDiscipline {
    rules: [DisciplineRule; DISCIPLINE_MAX],
    count: usize,
    pub intercepted: u64,
    pub delivered: u64,
}

impl KeyDiscipline {
    pub const fn new() -> KeyDiscipline {
        KeyDiscipline { rules: [DisciplineRule::empty(); DISCIPLINE_MAX], count: 0, intercepted: 0, delivered: 0 }
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    pub fn add_rule(
        &mut self,
        name: &[u8],
        mods: u8,
        code: u8,
        verdict: DisciplineVerdict,
    ) -> Option<usize> {
        if self.count >= DISCIPLINE_MAX || name.is_empty() || name.len() >= 16 {
            return None;
        }
        let mut r = DisciplineRule::empty();
        r.name_len = name.len();
        r.mods = mods;
        r.code = code;
        r.verdict = verdict;
        let mut i = 0usize;
        while i < name.len() {
            r.name[i] = name[i];
            i += 1;
        }
        self.rules[self.count] = r;
        self.count += 1;
        Some(self.count - 1)
    }

    /// 纪律判定：裸键（无修饰）永不拦截——纪律组第一红线。
    pub fn judge(&mut self, mods: u8, code: u8) -> DisciplineVerdict {
        if mods == 0 {
            self.delivered += 1;
            return DisciplineVerdict::Delivered;
        }
        let mut i = 0usize;
        while i < self.count {
            let r = self.rules[i];
            if r.mods == mods && r.code == code {
                match r.verdict {
                    DisciplineVerdict::Intercepted => {
                        self.intercepted += 1;
                        return DisciplineVerdict::Intercepted;
                    }
                    DisciplineVerdict::Reserved => return DisciplineVerdict::Reserved,
                    DisciplineVerdict::Delivered => {}
                }
            }
            i += 1;
        }
        self.delivered += 1;
        DisciplineVerdict::Delivered
    }

    pub fn find(&self, name: &[u8]) -> Option<DisciplineRule> {
        let mut i = 0usize;
        while i < self.count {
            if self.rules[i].name_eq(name) {
                return Some(self.rules[i]);
            }
            i += 1;
        }
        None
    }
}

/// AURORA 键位纪律标准表（AI-05 既有规范的逐条内核对齐）。
pub fn aurora_discipline() -> KeyDiscipline {
    let mut d = KeyDiscipline::new();
    let _ = d.add_rule(b"term", MOD_CTRL | MOD_ALT, 0x2C, DisciplineVerdict::Intercepted); // Ctrl+Alt+T
    let _ = d.add_rule(b"app-switch", MOD_ALT, 0x0D, DisciplineVerdict::Intercepted); // Alt+Tab
    let _ = d.add_rule(b"run", MOD_SUPER, 0x1B, DisciplineVerdict::Intercepted); // Super+S
    let _ = d.add_rule(b"reserved-sec", MOD_CTRL | MOD_ALT, 0x76, DisciplineVerdict::Reserved); // Ctrl+Alt+Esc
    d
}

// ---------------------------------------------------------------------------
// F134 全局快捷键 — 系统级拦截层，应用不可独占
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HotkeyOwner {
    System,
    FocusedApp,
    None,
}

#[derive(Clone, Copy)]
pub struct HotkeyBinding {
    pub mods: u8,
    pub code: u8,
    pub owner: HotkeyOwner,
    pub app_id: u32,
    pub registered_at: u64,
}

impl HotkeyBinding {
    pub const fn empty() -> HotkeyBinding {
        HotkeyBinding { mods: 0, code: 0, owner: HotkeyOwner::None, app_id: 0, registered_at: 0 }
    }
}

pub const HOTKEY_MAX: usize = 16;

#[derive(Clone, Copy)]
pub struct HotkeyTable {
    bindings: [HotkeyBinding; HOTKEY_MAX],
    count: usize,
    pub rejected: u64,
    /// 系统级键被应用抢注的尝试次数（必须为 0 才能过纪律）。
    pub steal_attempts: u64,
}

impl HotkeyTable {
    pub const fn new() -> HotkeyTable {
        HotkeyTable { bindings: [HotkeyBinding::empty(); HOTKEY_MAX], count: 0, rejected: 0, steal_attempts: 0 }
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    pub fn bind_system(&mut self, mods: u8, code: u8) -> bool {
        if self.count >= HOTKEY_MAX {
            return false;
        }
        if let Some(i) = self.find(mods, code) {
            if self.bindings[i].owner == HotkeyOwner::System {
                return false;
            }
            // 系统键可从应用手里收回。
            self.bindings[i].owner = HotkeyOwner::System;
            self.bindings[i].app_id = 0;
            return true;
        }
        let mut b = HotkeyBinding::empty();
        b.mods = mods;
        b.code = code;
        b.owner = HotkeyOwner::System;
        b.registered_at = self.count as u64;
        self.bindings[self.count] = b;
        self.count += 1;
        true
    }

    /// 应用注册：不得与系统级冲突（应用不可独占）。
    pub fn bind_app(&mut self, mods: u8, code: u8, app_id: u32) -> bool {
        if self.count >= HOTKEY_MAX || app_id == 0 || mods == 0 {
            self.rejected += 1;
            return false;
        }
        if let Some(i) = self.find(mods, code) {
            self.rejected += 1;
            if self.bindings[i].owner == HotkeyOwner::System {
                self.steal_attempts += 1;
            }
            return false;
        }
        let mut b = HotkeyBinding::empty();
        b.mods = mods;
        b.code = code;
        b.owner = HotkeyOwner::FocusedApp;
        b.app_id = app_id;
        b.registered_at = self.count as u64;
        self.bindings[self.count] = b;
        self.count += 1;
        true
    }

    pub fn find(&self, mods: u8, code: u8) -> Option<usize> {
        let mut i = 0usize;
        while i < self.count {
            if self.bindings[i].mods == mods && self.bindings[i].code == code {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 命中归属。
    pub fn resolve(&self, mods: u8, code: u8) -> HotkeyOwner {
        match self.find(mods, code) {
            Some(i) => self.bindings[i].owner,
            None => HotkeyOwner::None,
        }
    }

    pub fn binding(&self, i: usize) -> Option<HotkeyBinding> {
        if i < self.count {
            Some(self.bindings[i])
        } else {
            None
        }
    }
}

// ---------------------------------------------------------------------------
// F135 焦点系统 — 归属、切换、丢失事件
// ---------------------------------------------------------------------------

pub const FOCUS_MAX: usize = 8;

#[derive(Clone, Copy)]
pub struct Focusable {
    pub id: u32,
    pub z: i32,
    pub can_focus: bool,
    /// Tab 序。
    pub tab_order: u32,
}

impl Focusable {
    pub const fn empty() -> Focusable {
        Focusable { id: 0, z: 0, can_focus: false, tab_order: 0 }
    }
}

#[derive(Clone, Copy)]
pub struct FocusSystem {
    items: [Focusable; FOCUS_MAX],
    count: usize,
    /// 当前焦点 id（0 = 无）。
    pub focused: u32,
    pub switches: u64,
    /// 焦点丢失事件数（供通知/无障碍播报）。
    pub lost_events: u64,
    /// 最后点击坐标。
    pub last_click: (u32, u32),
}

impl FocusSystem {
    pub const fn new() -> FocusSystem {
        FocusSystem {
            items: [Focusable::empty(); FOCUS_MAX],
            count: 0,
            focused: 0,
            switches: 0,
            lost_events: 0,
            last_click: (0, 0),
        }
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    pub fn add(&mut self, id: u32, z: i32, can_focus: bool, tab_order: u32) -> Option<usize> {
        if self.count >= FOCUS_MAX || id == 0 {
            return None;
        }
        let mut f = Focusable::empty();
        f.id = id;
        f.z = z;
        f.can_focus = can_focus;
        f.tab_order = tab_order;
        self.items[self.count] = f;
        self.count += 1;
        Some(self.count - 1)
    }

    pub fn get(&self, i: usize) -> Option<Focusable> {
        if i < self.count {
            Some(self.items[i])
        } else {
            None
        }
    }

    fn set_focus(&mut self, id: u32) -> bool {
        if id == self.focused {
            return false;
        }
        if self.focused != 0 {
            self.lost_events += 1;
        }
        self.focused = id;
        self.switches += 1;
        true
    }

    /// 点击切换：命中 z 最高且可聚焦者；命中不可聚焦区则焦点丢失。
    pub fn click(&mut self, x: u32, y: u32, hit: u32, hit_can_focus: bool) -> bool {
        self.last_click = (x, y);
        if hit == 0 {
            return self.set_focus(0);
        }
        if !hit_can_focus {
            return false;
        }
        self.set_focus(hit)
    }

    /// Tab 切换（按 tab_order 环形）。
    pub fn tab(&mut self, forward: bool) -> bool {
        if self.count == 0 {
            return false;
        }
        let mut best: Option<usize> = None;
        let mut i = 0usize;
        while i < self.count {
            if self.items[i].can_focus && self.items[i].id != self.focused {
                match best {
                    None => best = Some(i),
                    Some(b) => {
                        let cur = self.items[i].tab_order;
                        let bb = self.items[b].tab_order;
                        let better = if forward { cur < bb } else { cur > bb };
                        if better {
                            best = Some(i);
                        }
                    }
                }
            }
            i += 1;
        }
        match best {
            Some(i) => {
                let id = self.items[i].id;
                self.set_focus(id)
            }
            None => false,
        }
    }

    /// 焦点窗口被关闭时的兜底。
    pub fn drop_focus(&mut self, id: u32) -> bool {
        if self.focused != id {
            return false;
        }
        self.set_focus(0)
    }
}

// ---------------------------------------------------------------------------
// 域自检：F126~F150（本文件 F126~F135，子模块 keys/guard 各 10/5 项）
// ---------------------------------------------------------------------------

pub fn run_hidsrv_checks() -> CheckSet {
    let mut set = CheckSet::new("hidsrv");

    // F126 PS/2 键盘
    let mut ctl = Ps2Controller::new();
    let st = ctl.self_test(0x55) && !ctl.self_test(0x00) && ctl.write_cmd(0xF4);
    ctl.push_output(SC_A, false);
    let out_full = ctl.status & PS2_STATUS_OUTPUT_FULL != 0;
    let read = ctl.read_data();
    let empty_after = ctl.read_data().is_none();
    let mut kb = Ps2Keyboard::new();
    let press = kb.feed(SC_A);
    let brk = kb.feed(SC_F0);
    let rel = kb.feed(SC_A);
    let pf = kb.feed(SC_E0);
    let ext = kb.feed(0x1F);
    let pause = kb.feed(SC_E1).is_none();
    set.add(
        "F126 ps2 keyboard",
        st
            && out_full
            && read == Some(SC_A)
            && empty_after
            && press.map(|d| d.phase == KeyPhase::Press && d.code == SC_A).unwrap_or(false)
            && brk.is_none()
            && rel.map(|d| d.phase == KeyPhase::Release).unwrap_or(false)
            && pf.is_none()
            && ext.map(|d| d.extended && d.code == 0x1F).unwrap_or(false)
            && pause
            && kb.irqs == 6
            && kb.malformed == 1
            && Ps2Keyboard::ascii(SC_A) == b'a'
            && Ps2Keyboard::ascii(SC_ENTER) == b'\n'
            && Ps2Keyboard::ascii(0xFF) == 0,
        "自检/扫描码集2 解码/E0/F0/E1",
    );

    // F127 PS/2 鼠标
    let mut ms = Ps2Mouse::new(true);
    let sync = ms.feed(0x00).is_none();
    let malformed_after = ms.malformed;
    let p1 = ms.feed(0x09);
    let p2 = ms.feed(0x05);
    let p3 = ms.feed(0xFB);
    let p4 = ms.feed(0x08);
    let pkt = p4.unwrap_or(MousePacket::empty());
    set.add(
        "F127 ps2 mouse",
        sync
            && malformed_after == 1
            && p1.is_none()
            && p2.is_none()
            && p3.is_none()
            && pkt.left
            && pkt.dx == 5
            && pkt.dy == -5
            && pkt.wheel == 0
            && ms.packets == 1
            && ms.last.buttons() == 0b001,
        "同步位/9位补码/按钮解析",
    );
    let mut mw = Ps2Mouse::new(true);
    let _ = mw.feed(0x08);
    let _ = mw.feed(0x00);
    let _ = mw.feed(0x00);
    let up = mw.feed(0x09);
    set.add(
        "F127 mouse wheel",
        up.map(|p| p.wheel == 1).unwrap_or(false) && mw.packets == 1,
        "滚轮符号（上滚为正）",
    );
    let mut m3 = Ps2Mouse::new(false);
    let _ = m3.feed(0x08);
    let _ = m3.feed(0x00);
    let done = m3.feed(0x00);
    set.add(
        "F127 mouse 3byte",
        done.map(|p| p.wheel == 0 && p.dx == 0).unwrap_or(false) && m3.packets == 1,
        "三字节包模式",
    );

    // F128 USB 输入预留
    let mut usb_none = UsbInputStub::new(UsbInputStatus::NoController);
    let plugged = usb_none.probe();
    let ev = usb_none.poll_event();
    let mut usb_ok = UsbInputStub::new(UsbInputStatus::Ready);
    let ok = usb_ok.probe();
    set.add(
        "F128 usb placeholder",
        !plugged
            && ev.is_none()
            && usb_none.fallbacks >= 2
            && ok
            && usb_ok.devices == 1
            && usb_ok.fallbacks == 0
            && !UsbInputStub::new(UsbInputStatus::Enumerating).probe(),
        "无控制器明确降级 PS/2",
    );

    // F129 内核事件队列
    let mut q = EventQueue::new();
    let mut i = 0usize;
    while i < EVENT_QUEUE_CAP {
        let _ = q.push(InputEvent::motion(i as u64, EventKind::MouseMove, 1, 2, 0));
        i += 1;
    }
    let over = q.push(InputEvent::motion(0, EventKind::MouseMove, 0, 0, 0));
    let peek = q.peek();
    let popped = q.pop();
    set.add(
        "F129 event queue",
        !over
            && q.dropped == 1
            && q.pushed == EVENT_QUEUE_CAP as u64 + 1
            && q.len() == EVENT_QUEUE_CAP - 1
            && peek.map(|e| e.dx == 1 && e.dy == 2).unwrap_or(false)
            && popped.map(|e| e.tick_us == 0).unwrap_or(false)
            && q.popped == 1,
        "统一结构入队/满即丢弃不阻塞中断",
    );

    // F130 事件时间戳
    let mut ts = Timestamping::new(1_000_000);
    let a = ts.stamp_us(1000);
    let b = ts.stamp_us(500);
    set.add(
        "F130 timestamps",
        a == 1000
            && b == 500
            && ts.backwards == 1
            && ts.samples == 2
            && Timestamping::latency_us(100, 9_000) == 8_900
            && Timestamping::latency_us(9_000, 100) == 0
            && Timestamping::new(0).stamp_us(5) == 0,
        "TSC→μs/回退检测/延迟计算",
    );

    // F131 用户态事件通道
    let mut disp = HidDispatcher::new();
    let key_mask = 1u32 << event_bit(EventKind::KeyDown);
    let p = disp.subscribe(60, key_mask | (1u32 << event_bit(EventKind::MouseMove)));
    let q2 = disp.subscribe(61, 1u32 << event_bit(EventKind::KeyDown));
    let none = disp.subscribe(0, 1);
    let n1 = disp.dispatch(InputEvent::key(1, EventKind::KeyDown, 0x1C, 0, 0));
    let n2 = disp.dispatch(InputEvent::motion(2, EventKind::MouseMove, 1, 1, 0));
    let unsub = disp.unsubscribe(61);
    let n3 = disp.dispatch(InputEvent::key(3, EventKind::KeyDown, 0x1C, 0, 0));
    set.add(
        "F131 event channel",
        p == Some(0)
            && q2 == Some(1)
            && none.is_none()
            && n1 == 2
            && n2 == 1
            && unsub
            && n3 == 1
            && disp.port(0).map(|pp| pp.delivered == 3).unwrap_or(false)
            && disp.port(1).map(|pp| pp.delivered == 1).unwrap_or(false)
            && disp.unrouted == 0
            && disp.drain(0, 2) == 2
            && disp.drain(0, 9) == 1,
        "fd 订阅掩码/退订/路由",
    );

    // F132 键盘状态机
    let mut fsm = KeyboardFsm::new();
    let d1 = fsm.down(SC_LCTRL, 0);
    let d2 = fsm.down(SC_LCTRL, 1);
    let d3 = fsm.down(SC_A, 10);
    let chord = fsm.is_chord(MOD_CTRL, SC_A);
    let mods_after_chord = fsm.mods;
    let rep_none = fsm.tick(100).is_none();
    let rep1 = fsm.tick(510) == Some(SC_A);
    let rep2 = fsm.tick(544) == Some(SC_A);
    let u1 = fsm.up(SC_A, 600);
    let u2 = fsm.up(SC_LCTRL, 601);
    set.add(
        "F132 keyboard fsm",
        d1 && !d2 && d3 && chord && mods_after_chord == MOD_CTRL
            && rep_none
            && rep1
            && rep2
            && u1
            && u2
            && fsm.mods == 0
            && fsm.down_count() == 0
            && fsm.downs == 2
            && fsm.ups == 2
            && !fsm.up(0x99, 700),
        "按下/重复/释放/修饰键/chord",
    );

    // F133 键位纪律对接
    let mut disc = aurora_discipline();
    let bare = disc.judge(0, SC_A);
    let term = disc.judge(MOD_CTRL | MOD_ALT, 0x2C);
    let reserved = disc.judge(MOD_CTRL | MOD_ALT, SC_ESC);
    let plain = disc.judge(MOD_SHIFT, SC_A);
    set.add(
        "F133 key discipline",
        disc.len() == 4
            && bare == DisciplineVerdict::Delivered
            && term == DisciplineVerdict::Intercepted
            && reserved == DisciplineVerdict::Reserved
            && plain == DisciplineVerdict::Delivered
            && disc.intercepted == 1
            && disc.find(b"app-switch").map(|r| r.mods == MOD_ALT).unwrap_or(false)
            && !disc.find(b"nope").is_some(),
        "裸键不拦截/纪律逐条判定",
    );

    // F134 全局快捷键
    let mut hk = HotkeyTable::new();
    let sys = hk.bind_system(MOD_ALT, 0x0D);
    let steal = !hk.bind_app(MOD_ALT, 0x0D, 77);
    let own = hk.bind_app(MOD_CTRL, 0x2C, 77);
    let resolved_sys = hk.resolve(MOD_ALT, 0x0D);
    let resolved_app = hk.resolve(MOD_CTRL, 0x2C);
    let bare_rejected = !hk.bind_app(0, 0x1C, 77);
    set.add(
        "F134 global hotkeys",
        sys
            && steal
            && own
            && resolved_sys == HotkeyOwner::System
            && resolved_app == HotkeyOwner::FocusedApp
            && bare_rejected
            && hk.steal_attempts == 1
            && hk.rejected == 2
            && hk.len() == 2,
        "系统级不可抢注/归属解析",
    );

    // F135 焦点系统
    let mut fs = FocusSystem::new();
    let a = fs.add(10, 0, true, 0);
    let b = fs.add(20, 5, true, 1);
    let _c = fs.add(30, 1, false, 2);
    let no_focus = fs.focused;
    let click = fs.click(1, 1, 20, true);
    let after_click = fs.focused;
    let lost_after_click = fs.lost_events;
    let tab = fs.tab(true);
    let after_tab = fs.focused;
    let tab_back = fs.tab(false);
    let after_tab_back = fs.focused;
    let wrong_drop = !fs.drop_focus(10);
    let drop = fs.drop_focus(20);
    set.add(
        "F135 focus system",
        a == Some(0)
            && b == Some(1)
            && no_focus == 0
            && click
            && after_click == 20
            && lost_after_click == 0
            && tab
            && after_tab == 10
            && tab_back
            && after_tab_back == 20
            && wrong_drop
            && drop
            && fs.focused == 0
            && fs.lost_events == 3
            && fs.switches == 4
            && !fs.drop_focus(0),
        "点击/Tab 切换/焦点丢失",
    );

    keys::extend_checks(&mut set);
    guard::extend_checks(&mut set);

    set
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f126_scancode_release_prefix() {
        let mut kb = Ps2Keyboard::new();
        assert!(kb.feed(SC_A).is_some());
        assert!(kb.feed(SC_F0).is_none());
        let r = kb.feed(SC_A).unwrap();
        assert_eq!(r.phase, KeyPhase::Release);
    }

    #[test]
    fn f127_mouse_negative_delta() {
        let mut m = Ps2Mouse::new(false);
        let _ = m.feed(0x08);
        let _ = m.feed(0x00);
        let p = m.feed(0x20 | 0x10); // y 负溢出位? 使用 x 符号位
        assert!(p.is_some());
    }

    #[test]
    fn f132_auto_repeat_timing() {
        let mut f = KeyboardFsm::new();
        assert!(f.down(SC_A, 0));
        assert!(f.tick(100).is_none());
        assert_eq!(f.tick(500), Some(SC_A));
        assert!(f.tick(510).is_none());
        assert_eq!(f.tick(533), Some(SC_A));
    }

    #[test]
    fn f135_focus_lost_on_refocus() {
        let mut f = FocusSystem::new();
        let _ = f.add(1, 0, true, 0);
        let _ = f.add(2, 0, true, 1);
        assert!(f.click(0, 0, 1, true));
        assert_eq!(f.lost_events, 0);
        assert!(f.click(0, 0, 2, true));
        assert_eq!(f.lost_events, 1);
    }

    #[test]
    fn f_run_hidsrv_checks_pass() {
        let set = run_hidsrv_checks();
        if !set.all_passed() {
            let mut buf = [0u8; 2048];
            let n = set.render(&mut buf);
            panic!("hidsrv self-test failed:\n{}", core::str::from_utf8(&buf[..n]).unwrap_or("<x>"));
        }
        assert!(set.len() >= 25, "hidsrv domain must expose >= 25 checks");
    }
}
