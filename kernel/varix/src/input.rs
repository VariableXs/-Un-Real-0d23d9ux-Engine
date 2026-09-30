//! AI-07 · 输入与 HID 域（F151~F175）.
//!
//! From a PS/2 scan code to a pixel changing colour. The route is: transport
//! (PS/2 or xHCI) → HID report → key code → the key table the *focused plane*
//! selects → a command. Each stage is a pure function over data, which matters
//! because input is the one path where a wrong lookup is felt immediately.

use core::sync::atomic::{AtomicU64, Ordering};

// ---------------------------------------------------------------------------
// F152 — scan codes (Set 1, the make/break codes a PS/2 controller sends)
// ---------------------------------------------------------------------------

pub const SC_ESC: u8 = 0x01;
pub const SC_BACKSPACE: u8 = 0x0E;
pub const SC_ENTER: u8 = 0x1C;
pub const SC_LCTRL: u8 = 0x1D;
pub const SC_LSHIFT: u8 = 0x2A;
pub const SC_RSHIFT: u8 = 0x36;
pub const SC_LALT: u8 = 0x38;
pub const SC_CAPSLOCK: u8 = 0x3A;
pub const SC_SPACE: u8 = 0x39;
pub const SC_EXTENDED: u8 = 0xE0;
pub const SC_RELEASE: u8 = 0x80;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct KeyCode(pub u8);

impl KeyCode {
    pub const A: KeyCode = KeyCode(0x1E);
    pub const Z: KeyCode = KeyCode(0x2C);
    pub const N1: KeyCode = KeyCode(0x02);
    pub const N0: KeyCode = KeyCode(0x0B);
    pub const F1: KeyCode = KeyCode(0x3B);
    pub const F12: KeyCode = KeyCode(0x58);
    pub const ESC: KeyCode = KeyCode(0x01);
    pub const SPACE: KeyCode = KeyCode(0x39);
    pub const ENTER: KeyCode = KeyCode(0x1C);
    pub const BACKSPACE: KeyCode = KeyCode(0x0E);
    pub const TAB: KeyCode = KeyCode(0x0F);
    pub const UNKNOWN: KeyCode = KeyCode(0);

    pub fn valid(self) -> bool {
        self.0 != 0
    }

    /// ASCII for a key in the US layout, unshifted. The layout engine (F153)
    /// maps this through the active table, so this is the base layer only.
    pub fn ascii_unshifted(self) -> Option<u8> {
        let c = self.0;
        match c {
            // Set 1: 0x02..=0x0A are '1'..'9', 0x0B is '0'.
            0x02..=0x0A => Some(b'1' + (c - 0x02)),
            0x0C => Some(b'-'),
            0x0D => Some(b'='),
            0x10..=0x19 => Some(b"qwertyuiop"[c as usize - 0x10]),
            0x1A..=0x1B => Some(b"[]"[c as usize - 0x1A]),
            0x1E..=0x26 => Some(b"asdfghjkl"[c as usize - 0x1E]),
            0x27 => Some(b';'),
            0x28 => Some(b'\''),
            0x29 => Some(b'`'),
            0x2B => Some(b'\\'),
            0x2C..=0x32 => Some(b"zxcvbnm"[c as usize - 0x2C]),
            0x33 => Some(b','),
            0x34 => Some(b'.'),
            0x35 => Some(b'/'),
            0x39 => Some(b' '),
            0x0B => Some(b'0'),
            _ => None,
        }
    }

    /// The shifted ASCII.
    pub fn ascii_shifted(self) -> Option<u8> {
        let c = self.0;
        match c {
            0x02 => Some(b'!'),
            0x03 => Some(b'@'),
            0x04 => Some(b'#'),
            0x05 => Some(b'$'),
            0x06 => Some(b'%'),
            0x07 => Some(b'^'),
            0x08 => Some(b'&'),
            0x09 => Some(b'*'),
            0x0A => Some(b'('),
            0x0B => Some(b')'),
            0x0C => Some(b'_'),
            0x0D => Some(b'+'),
            0x1A => Some(b'{'),
            0x1B => Some(b'}'),
            0x27 => Some(b':'),
            0x28 => Some(b'"'),
            0x29 => Some(b'~'),
            0x2B => Some(b'|'),
            0x33 => Some(b'<'),
            0x34 => Some(b'>'),
            0x35 => Some(b'?'),
            _ => match self.ascii_unshifted() {
                Some(b'0'..=b'9') => None,
                Some(c) if c.is_ascii_lowercase() => Some(c - 32),
                other => other,
            },
        }
    }
}

/// F152: the Set 1 decoder. `0xE0` prefixes an extended (E0) code, and the top
/// bit of the *second* byte means release.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct ScancodeDecoder {
    extended: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ScancodeEvent {
    pub code: KeyCode,
    pub pressed: bool,
    pub extended: bool,
}

impl ScancodeDecoder {
    pub const fn new() -> ScancodeDecoder {
        ScancodeDecoder { extended: false }
    }

    /// Feed one byte from the controller. Returns `Some` when the byte
    /// completed an event.
    pub fn feed(&mut self, byte: u8) -> Option<ScancodeEvent> {
        if byte == SC_EXTENDED {
            self.extended = true;
            return None;
        }
        let released = byte & SC_RELEASE != 0;
        let code = byte & 0x7F;
        let event = ScancodeEvent {
            code: KeyCode(code),
            pressed: !released,
            extended: self.extended,
        };
        self.extended = false;
        Some(event)
    }

    pub fn in_escape_sequence(&self) -> bool {
        self.extended
    }
}

// ---------------------------------------------------------------------------
// F153/F154/F155 — layouts, planes, focus routing
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Modifiers(pub u8);

impl Modifiers {
    pub const SHIFT: u8 = 1 << 0;
    pub const CTRL: u8 = 1 << 1;
    pub const ALT: u8 = 1 << 2;
    pub const META: u8 = 1 << 3;

    pub fn has(self, m: u8) -> bool {
        self.0 & m == m
    }

    pub fn with(self, m: u8, on: bool) -> Modifiers {
        Modifiers(if on { self.0 | m } else { self.0 & !m })
    }

    /// The two modifiers that turn a key into a system shortcut.
    pub fn is_shortcut(self) -> bool {
        self.has(Self::CTRL) || self.has(Self::META) || (self.has(Self::ALT) && self.has(Self::SHIFT))
    }

    pub fn render(self, out: &mut [u8]) -> usize {
        let mut w = crate::cpu::Hud::new(out);
        let names = [
            (Self::CTRL, "ctrl"),
            (Self::SHIFT, "shift"),
            (Self::ALT, "alt"),
            (Self::META, "meta"),
        ];
        let mut first = true;
        for (bit, name) in names {
            if self.has(bit) {
                if !first {
                    w.str("+");
                }
                w.str(name);
                first = false;
            }
        }
        w.used()
    }
}

/// F154: the two key planes. The same physical keys mean different things
/// depending on which system has the user's attention.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum KeyPlane {
    /// Varix itself.
    #[default]
    Host,
    /// A guest system sharing the keyboard (W5).
    Guest,
}

impl KeyPlane {
    pub fn as_str(self) -> &'static str {
        match self {
            KeyPlane::Host => "host",
            KeyPlane::Guest => "guest",
        }
    }
}

/// A binding in one plane.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Binding {
    pub key: KeyCode,
    pub mods: Modifiers,
    pub action: &'static str,
}

/// F153: one layout = a plane's key table plus its shortcut table.
#[derive(Clone, Copy)]
pub struct Layout {
    pub name: &'static str,
    bindings: [Binding; 16],
    len: usize,
}

impl Layout {
    pub const fn new(name: &'static str) -> Layout {
        Layout {
            name,
            bindings: [Binding {
                key: KeyCode::UNKNOWN,
                mods: Modifiers(0),
                action: "",
            }; 16],
            len: 0,
        }
    }

    pub fn bind(&mut self, key: KeyCode, mods: Modifiers, action: &'static str) -> bool {
        if self.len >= 16 || !key.valid() {
            return false;
        }
        self.bindings[self.len] = Binding { key, mods, action };
        self.len += 1;
        true
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn get(&self, i: usize) -> Option<Binding> {
        if i < self.len {
            Some(self.bindings[i])
        } else {
            None
        }
    }

    /// Exact match on key + modifiers.
    pub fn lookup(&self, key: KeyCode, mods: Modifiers) -> Option<&'static str> {
        self.bindings[..self.len]
            .iter()
            .find(|b| b.key == key && b.mods == mods)
            .map(|b| b.action)
    }

    /// The printable character this key produces under `mods`.
    pub fn char_for(&self, key: KeyCode, mods: Modifiers) -> Option<u8> {
        if mods.has(Modifiers::SHIFT) {
            key.ascii_shifted()
        } else {
            key.ascii_unshifted()
        }
    }
}

impl Default for Layout {
    fn default() -> Layout {
        Layout::new("empty")
    }
}

/// The two tables, plus which one currently owns the keyboard.
pub struct KeyPlanes {
    host: Layout,
    guest: Layout,
    focused: KeyPlane,
    switches: u64,
}

impl KeyPlanes {
    pub const fn new() -> KeyPlanes {
        KeyPlanes {
            host: Layout::new("varix"),
            guest: Layout::new("guest"),
            focused: KeyPlane::Host,
            switches: 0,
        }
    }

    pub fn host_mut(&mut self) -> &mut Layout {
        &mut self.host
    }

    pub fn guest_mut(&mut self) -> &mut Layout {
        &mut self.guest
    }

    pub fn host(&self) -> &Layout {
        &self.host
    }

    pub fn guest(&self) -> &Layout {
        &self.guest
    }

    pub fn focused_plane(&self) -> KeyPlane {
        self.focused
    }

    pub fn switches(&self) -> u64 {
        self.switches
    }

    /// F155: focus decides the table. This is the single line that makes a
    /// dual-plane keyboard feel right instead of surprising.
    pub fn focus(&mut self, plane: KeyPlane) -> bool {
        if plane == self.focused {
            return false;
        }
        self.focused = plane;
        self.switches += 1;
        true
    }

    /// Route a key through the focused plane.
    pub fn route(&self, key: KeyCode, mods: Modifiers) -> Routing {
        let layout = match self.focused {
            KeyPlane::Host => &self.host,
            KeyPlane::Guest => &self.guest,
        };
        match layout.lookup(key, mods) {
            Some(action) => Routing {
                plane: self.focused,
                action,
                passthrough: false,
            },
            None => Routing {
                plane: self.focused,
                action: "",
                // No binding: the key still belongs to whoever has focus, so it
                // is delivered as text rather than forwarded to the other side.
                passthrough: false,
            },
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Routing {
    pub plane: KeyPlane,
    pub action: &'static str,
    /// True only when the key must reach the *other* system.
    pub passthrough: bool,
}

// ---------------------------------------------------------------------------
// F156/F157 — xHCI and USB enumeration
// ---------------------------------------------------------------------------

/// A 16-byte Transfer Request Block. The cycle bit is what tells the
/// controller (and the driver) whether a slot is valid, and forgetting to flip
/// it on wrap is the classic xHCI stall.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Trb {
    pub parameter: u64,
    pub status: u32,
    pub kind: u8,
    pub cycle: bool,
}

impl Trb {
    pub const LEN: usize = 16;
    pub const KIND_NORMAL: u8 = 1;
    pub const KIND_SETUP: u8 = 2;
    pub const KIND_DATA: u8 = 3;
    pub const KIND_STATUS: u8 = 4;
    pub const KIND_LINK: u8 = 6;

    pub fn encode(&self) -> [u8; Self::LEN] {
        let mut b = [0u8; Self::LEN];
        b[0..8].copy_from_slice(&self.parameter.to_le_bytes());
        b[8..12].copy_from_slice(&self.status.to_le_bytes());
        // Control dword: kind in bits 15:10, cycle in bit 0.
        let ctl = ((self.kind as u32) << 10) | (self.cycle as u32);
        b[12..16].copy_from_slice(&ctl.to_le_bytes());
        b
    }

    pub fn decode(b: &[u8; Self::LEN]) -> Trb {
        let ctl = u32::from_le_bytes([b[12], b[13], b[14], b[15]]);
        Trb {
            parameter: u64::from_le_bytes([
                b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
            ]),
            status: u32::from_le_bytes([b[8], b[9], b[10], b[11]]),
            kind: ((ctl >> 10) & 0x3F) as u8,
            cycle: ctl & 1 != 0,
        }
    }

    pub fn is_link(&self) -> bool {
        self.kind == Self::KIND_LINK
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct XhciRing {
    pub slots: u16,
    pub index: u16,
    pub cycle: bool,
    pub pending: u16,
}

impl XhciRing {
    pub fn new(slots: u16) -> XhciRing {
        XhciRing {
            slots,
            index: 0,
            cycle: true,
            pending: 0,
        }
    }

    /// Hand out the next TRB slot, flipping the cycle bit on wrap.
    pub fn push(&mut self) -> Option<(u16, bool)> {
        if self.pending >= self.slots {
            return None;
        }
        let slot = self.index;
        let cycle = self.cycle;
        self.index = (self.index + 1) % self.slots;
        if self.index == 0 {
            self.cycle = !self.cycle;
        }
        self.pending += 1;
        Some((slot, cycle))
    }

    pub fn pop(&mut self) -> bool {
        if self.pending == 0 {
            return false;
        }
        self.pending -= 1;
        true
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct DeviceDescriptor {
    pub usb_version: u16,
    pub class: u8,
    pub sub_class: u8,
    pub protocol: u8,
    pub max_packet0: u8,
    pub id_vendor: u16,
    pub id_product: u16,
    pub device_version: u16,
    pub configurations: u8,
}

impl DeviceDescriptor {
    pub fn parse(b: &[u8]) -> Result<DeviceDescriptor, &'static str> {
        if b.len() < 18 {
            return Err("device descriptor is truncated");
        }
        if b[1] != 1 {
            return Err("not a device descriptor");
        }
        if b[0] < 18 {
            return Err("descriptor length is too small");
        }
        Ok(DeviceDescriptor {
            usb_version: u16::from_le_bytes([b[2], b[3]]),
            class: b[4],
            sub_class: b[5],
            protocol: b[6],
            max_packet0: b[7],
            id_vendor: u16::from_le_bytes([b[8], b[9]]),
            id_product: u16::from_le_bytes([b[10], b[11]]),
            device_version: u16::from_le_bytes([b[12], b[13]]),
            configurations: b[17],
        })
    }

    /// F157: a keyboard or mouse with class 3 at the *device* level needs no
    /// interface parsing; one that reports class 0 hands off to the interface
    /// descriptor instead.
    pub fn is_hid(&self) -> bool {
        self.class == 3
    }

    pub fn vendor_product_str(&self) -> ([u8; 4], [u8; 4]) {
        let hex = |v: u16| {
            let d = b"0123456789ABCDEF";
            [
                d[(v >> 12) as usize & 0xF],
                d[(v >> 8) as usize & 0xF],
                d[(v >> 4) as usize & 0xF],
                d[v as usize & 0xF],
            ]
        };
        (hex(self.id_vendor), hex(self.id_product))
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct InterfaceDescriptor {
    pub number: u8,
    pub class: u8,
    pub sub_class: u8,
    pub protocol: u8,
    pub endpoints: u8,
}

impl InterfaceDescriptor {
    pub fn parse(b: &[u8]) -> Result<InterfaceDescriptor, &'static str> {
        if b.len() < 9 || b[1] != 4 {
            return Err("not an interface descriptor");
        }
        // bLength(0) type(1) number(2) alternate(3) endpoints(4)
        // class(5) sub_class(6) protocol(7) iInterface(8)
        Ok(InterfaceDescriptor {
            number: b[2],
            endpoints: b[4],
            class: b[5],
            sub_class: b[6],
            protocol: b[7],
        })
    }

    pub fn is_boot_keyboard(&self) -> bool {
        self.class == 3 && self.sub_class == 1 && self.protocol == 1
    }

    pub fn is_boot_mouse(&self) -> bool {
        self.class == 3 && self.sub_class == 1 && self.protocol == 2
    }
}

// ---------------------------------------------------------------------------
// F158/F159/F160/F161/F162/F168 — HID report descriptors and reports
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct ReportInfo {
    pub usage_page: u16,
    pub usage: u16,
    pub report_id: u8,
    pub report_size: u8,
    pub report_count: u16,
    pub logical_min: i32,
    pub logical_max: i32,
    /// The field was declared by an `Input` item.
    pub has_input: bool,
    /// Bits the device actually sends.
    pub input_bits: u32,
}

/// F158: a short-item HID report descriptor walker. Enough of the grammar to
/// size the reports and find the usage page — which is all a kernel needs
/// before it can read a boot-protocol report anyway.
pub fn parse_report_descriptor(bytes: &[u8]) -> Result<ReportInfo, &'static str> {
    let mut info = ReportInfo {
        logical_min: 0,
        logical_max: 0,
        ..ReportInfo::default()
    };
    let mut i = 0usize;
    let mut global_usage_page: u16 = 0;
    let mut local_usage: u16 = 0;
    let mut report_size: u8 = 0;
    let mut report_count: u16 = 0;
    let mut items = 0usize;

    while i < bytes.len() {
        let prefix = bytes[i];
        i += 1;
        if prefix == 0xFE {
            return Err("long items are not supported");
        }
        let size = match prefix & 0x03 {
            0 => 0usize,
            1 => 1,
            2 => 2,
            _ => 4,
        };
        if i + size > bytes.len() {
            return Err("report descriptor is truncated");
        }
        let tag = prefix >> 4;
        let kind = prefix & 0x0C;
        let mut value: u32 = 0;
        for k in 0..size {
            value |= (bytes[i + k] as u32) << (8 * k);
        }
        i += size;
        items += 1;

        match kind {
            0x00 => {
                // Main item.
                match tag {
                    0x08 => {
                        info.has_input = true;
                        info.input_bits = info
                            .input_bits
                            .saturating_add(report_size as u32 * report_count as u32);
                        if local_usage != 0 {
                            info.usage = local_usage;
                        }
                        info.usage_page = global_usage_page;
                        info.report_size = report_size;
                        info.report_count = report_count;
                        local_usage = 0;
                    }
                    _ => {}
                }
            }
            0x04 => {
                // Global item.
                match tag {
                    0x0 => global_usage_page = value as u16,
                    0x7 => report_size = value as u8,
                    0x9 => report_count = value as u16,
                    0x1 => info.logical_min = value as i32,
                    0x2 => info.logical_max = value as i32,
                    0x8 => info.report_id = value as u8,
                    _ => {}
                }
            }
            0x08 => {
                // Local item.
                if tag == 0x0 {
                    local_usage = value as u16;
                }
            }
            _ => {}
        }
    }
    if items == 0 {
        return Err("empty report descriptor");
    }
    if !info.has_input {
        return Err("no input report declared");
    }
    Ok(info)
}

/// F159: the 8-byte boot-protocol keyboard report.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct BootKeyboardReport {
    pub modifiers: Modifiers,
    pub keys: [KeyCode; 6],
}

impl BootKeyboardReport {
    pub fn parse(b: &[u8]) -> Result<BootKeyboardReport, &'static str> {
        if b.len() < 8 {
            return Err("keyboard report is truncated");
        }
        let mut keys = [KeyCode::UNKNOWN; 6];
        for (i, k) in b[2..8].iter().enumerate() {
            keys[i] = KeyCode(*k);
        }
        Ok(BootKeyboardReport {
            modifiers: Modifiers(b[0]),
            keys,
        })
    }

    pub fn key_count(&self) -> usize {
        self.keys.iter().filter(|k| k.valid()).count()
    }

    /// Keys present in `new` but not in `old` — the edge-triggered presses.
    pub fn pressed_since(&self, old: &BootKeyboardReport, out: &mut [KeyCode]) -> usize {
        let mut n = 0usize;
        for k in self.keys.iter() {
            if !k.valid() {
                continue;
            }
            if !old.keys.contains(k) {
                if n < out.len() {
                    out[n] = *k;
                    n += 1;
                }
            }
        }
        n
    }

    pub fn released_since(&self, old: &BootKeyboardReport, out: &mut [KeyCode]) -> usize {
        let mut n = 0usize;
        for k in old.keys.iter() {
            if !k.valid() {
                continue;
            }
            if !self.keys.contains(k) && n < out.len() {
                out[n] = *k;
                n += 1;
            }
        }
        n
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct BootMouseReport {
    pub buttons: u8,
    pub dx: i16,
    pub dy: i16,
    pub wheel: i8,
}

impl BootMouseReport {
    pub fn parse(b: &[u8]) -> Result<BootMouseReport, &'static str> {
        if b.len() < 3 {
            return Err("mouse report is truncated");
        }
        Ok(BootMouseReport {
            buttons: b[0],
            dx: b[1] as i8 as i16,
            dy: b[2] as i8 as i16,
            wheel: if b.len() > 3 { b[3] as i8 } else { 0 },
        })
    }

    pub fn left(&self) -> bool {
        self.buttons & 1 != 0
    }
    pub fn right(&self) -> bool {
        self.buttons & 2 != 0
    }
    pub fn middle(&self) -> bool {
        self.buttons & 4 != 0
    }
}

/// F162: a pen or touch point with pressure.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct PointerSample {
    pub x: i32,
    pub y: i32,
    /// 0..=32767, as HID reports it.
    pub pressure: u16,
    pub tilt_x: i8,
    pub tilt_y: i8,
    /// True while the pen tip is down (or the finger is on the glass).
    pub contact: bool,
}

impl PointerSample {
    pub const MAX_PRESSURE: u16 = 32767;

    pub fn is_pencil(&self) -> bool {
        self.tilt_x != 0 || self.tilt_y != 0 || self.pressure > 0
    }

    /// Pressure as a percentage — what the UI draws. Rounded, because a
    /// half-press reading as 49% looks like a bug to anyone watching.
    pub fn pressure_percent(&self) -> u64 {
        (self.pressure as u64 * 100 + Self::MAX_PRESSURE as u64 / 2)
            / Self::MAX_PRESSURE as u64
    }

    /// A pressure impulse is a "tap" when it is short and does not move; the
    /// gesture layer uses this to avoid firing a tap on a drag.
    pub fn is_tap_candidate(&self, moved: i32) -> bool {
        self.contact && moved < 8
    }
}

/// F160/F161: multi-touch gesture recognition from a stream of pointer samples.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Gesture {
    None,
    Tap,
    Drag,
    Scroll { dx: i32, dy: i32 },
    Pinch { scale_permille: i32 },
    Swipe { dx: i32, dy: i32 },
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct TouchTracker {
    touches: u8,
    start_x: i32,
    start_y: i32,
    last_x: i32,
    last_y: i32,
    span_start: i32,
    moved: i32,
    ticks: u32,
}

impl TouchTracker {
    pub const TAP_MAX_TICKS: u32 = 20;
    pub const SWIPE_MIN_MOVE: i32 = 40;

    pub fn begin(&mut self, x: i32, y: i32, touches: u8) {
        self.touches = touches;
        self.start_x = x;
        self.start_y = y;
        self.last_x = x;
        self.last_y = y;
        self.span_start = 0;
        self.moved = 0;
        self.ticks = 0;
    }

    /// Second finger down/up: the distance between the two fingers is the
    /// baseline the pinch is measured against.
    pub fn set_span(&mut self, span: i32) {
        self.span_start = span;
    }

    pub fn update(&mut self, x: i32, y: i32) {
        let step = (x - self.last_x).abs() + (y - self.last_y).abs();
        self.moved += step;
        self.last_x = x;
        self.last_y = y;
    }

    pub fn tick(&mut self) {
        self.ticks += 1;
    }

    /// Classify what the user just did.
    pub fn finish(&self, span_now: i32) -> Gesture {
        if self.touches >= 2 && self.span_start > 0 {
            let delta = ((span_now - self.span_start) * 1000) / self.span_start.max(1);
            if delta.abs() > 100 {
                return Gesture::Pinch {
                    scale_permille: 1000 + delta,
                };
            }
        }
        if self.moved < 8 {
            if self.ticks <= Self::TAP_MAX_TICKS {
                return Gesture::Tap;
            }
            return Gesture::None;
        }
        let dx = self.last_x - self.start_x;
        let dy = self.last_y - self.start_y;
        if self.touches >= 2 {
            return Gesture::Scroll { dx, dy };
        }
        if dx.abs() > Self::SWIPE_MIN_MOVE || dy.abs() > Self::SWIPE_MIN_MOVE {
            return Gesture::Swipe { dx, dy };
        }
        Gesture::Drag
    }
}

/// F168: a gamepad's axes and buttons, normalised.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct GamepadState {
    pub left_x: i16,
    pub left_y: i16,
    pub right_x: i16,
    pub right_y: i16,
    pub triggers: u16,
    pub buttons: u32,
}

impl GamepadState {
    /// A stick is only "moved" past the dead zone; a drifting stick must not
    /// scroll the desktop.
    pub const DEADZONE: i16 = 1200;

    pub fn deadzone(v: i16) -> i16 {
        if v.abs() < Self::DEADZONE {
            0
        } else {
            v
        }
    }

    pub fn normalised_left(&self) -> (i16, i16) {
        (Self::deadzone(self.left_x), Self::deadzone(self.left_y))
    }

    pub fn button(&self, index: u8) -> bool {
        index < 32 && self.buttons & (1 << index) != 0
    }
}

/// F169: a consumer-control (remote) usage.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RemoteKey {
    Power,
    VolumeUp,
    VolumeDown,
    Mute,
    PlayPause,
    Next,
    Previous,
}

impl RemoteKey {
    pub fn from_usage(usage: u16) -> Option<RemoteKey> {
        match usage {
            0x30 => Some(RemoteKey::Power),
            0xE9 => Some(RemoteKey::VolumeUp),
            0xEA => Some(RemoteKey::VolumeDown),
            0xE2 => Some(RemoteKey::Mute),
            0xCD => Some(RemoteKey::PlayPause),
            0xB5 => Some(RemoteKey::Next),
            0xB6 => Some(RemoteKey::Previous),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// F163/F164 — feel: inertial scroll and key repeat
// ---------------------------------------------------------------------------

/// F163: a scroll wheel that keeps moving after the wheel stops. Fixed point,
/// no floats, because this runs in the kernel's input path.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InertialScroll {
    /// Position in 1/256 of a notch.
    pub position: i32,
    /// Velocity in 1/256 notch per tick.
    pub velocity: i32,
    pub friction_num: i32,
    pub friction_den: i32,
    pub min_velocity: i32,
}

impl InertialScroll {
    pub const fn new() -> InertialScroll {
        InertialScroll {
            position: 0,
            velocity: 0,
            // 15/16 per tick: a long but finite tail — the curve that makes a
            // two-finger flick feel like it has weight instead of a stop.
            friction_num: 15,
            friction_den: 16,
            min_velocity: 8,
        }
    }

    /// A wheel detent. The decay sums to `v0 / (1 - friction)` = `v0 * 16`
    /// units, and there are 256 units per notch — so a base of 32 turns one
    /// detent into about two notches of travel, which is the amount that makes
    /// a trackpad flick feel like it has weight without running away.
    pub fn nudge(&mut self, notches: i32) {
        self.velocity += notches * Self::NOTCH_IMPULSE;
    }

    /// Initial velocity per wheel notch.
    pub const NOTCH_IMPULSE: i32 = 32;

    /// Advance one tick; returns the whole notches to deliver.
    pub fn step(&mut self) -> i32 {
        self.position += self.velocity;
        self.velocity = self.velocity * self.friction_num / self.friction_den;
        if self.velocity.abs() < self.min_velocity {
            self.velocity = 0;
        }
        let whole = self.position / 256;
        self.position -= whole * 256;
        whole
    }

    /// Motion is velocity. A leftover sub-notch position is not movement —
    /// otherwise the animation would never be considered finished and the
    /// compositor would keep redrawing.
    pub fn is_moving(&self) -> bool {
        self.velocity != 0
    }
}

impl Default for InertialScroll {
    fn default() -> InertialScroll {
        Self::new()
    }
}

/// F164: key repeat as a curve rather than two constants. `delay` is how long
/// the key must be held before repeating starts; the interval then tightens.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RepeatCurve {
    pub delay_ticks: u32,
    pub first_interval: u32,
    pub min_interval: u32,
    /// How many repeats it takes to reach `min_interval`.
    pub ramp_repeats: u32,
}

impl RepeatCurve {
    pub const fn standard() -> RepeatCurve {
        RepeatCurve {
            delay_ticks: 25,
            first_interval: 4,
            min_interval: 1,
            ramp_repeats: 12,
        }
    }

    pub const fn slow() -> RepeatCurve {
        RepeatCurve {
            delay_ticks: 40,
            first_interval: 8,
            min_interval: 4,
            ramp_repeats: 16,
        }
    }

    /// Interval before repeat `n` (1-based).
    pub fn interval_for(&self, n: u32) -> u32 {
        if n == 0 {
            return self.delay_ticks;
        }
        let ramp = self.ramp_repeats.max(1);
        if n >= ramp {
            return self.min_interval;
        }
        let span = self.first_interval.saturating_sub(self.min_interval);
        self.first_interval - span * n / ramp
    }

    /// Ticks until the next repeat, given how many repeats already happened.
    pub fn next_delay(&self, repeats: u32) -> u32 {
        if repeats == 0 {
            self.delay_ticks
        } else {
            self.interval_for(repeats)
        }
    }

    pub fn valid(&self) -> bool {
        self.min_interval > 0 && self.first_interval >= self.min_interval
    }
}

impl Default for RepeatCurve {
    fn default() -> RepeatCurve {
        RepeatCurve::standard()
    }
}

// ---------------------------------------------------------------------------
// F165/F166 — combo arbitration and conflict radar
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ComboVerdict {
    /// The combo fired; the plain key must not also be delivered.
    Swallowed(&'static str),
    /// No combo matched.
    Pass,
    /// The combo exists but is disabled, so the key goes through.
    Disabled(&'static str),
}

/// F165: system-level combo arbitration. Longer combos are checked first so
/// `Ctrl+Shift+T` is not stolen by `Ctrl+T`.
pub struct ComboArbiter {
    combos: [Binding; 16],
    len: usize,
    fired: u64,
    swallowed: u64,
}

impl ComboArbiter {
    pub const MAX_COMBO_KEYS: usize = 4;

    pub const fn new() -> ComboArbiter {
        ComboArbiter {
            combos: [Binding {
                key: KeyCode::UNKNOWN,
                mods: Modifiers(0),
                action: "",
            }; 16],
            len: 0,
            fired: 0,
            swallowed: 0,
        }
    }

    pub fn register(&mut self, key: KeyCode, mods: Modifiers, action: &'static str) -> bool {
        if self.len >= 16 {
            return false;
        }
        self.combos[self.len] = Binding { key, mods, action };
        self.len += 1;
        true
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn fired(&self) -> u64 {
        self.fired
    }

    pub fn swallowed(&self) -> u64 {
        self.swallowed
    }

    /// Resolve a key press. The most specific (most modifiers) match wins.
    pub fn resolve(&mut self, key: KeyCode, mods: Modifiers) -> ComboVerdict {
        let mut best: Option<usize> = None;
        for i in 0..self.len {
            let b = self.combos[i];
            // A system combo must involve Ctrl or Alt; requiring *both* would
            // silently drop every single-modifier shortcut.
            if b.key != key || b.mods.0 & (Modifiers::CTRL | Modifiers::ALT) == 0 {
                continue;
            }
            if !mods.has(b.mods.0) {
                continue;
            }
            let better = match best {
                None => true,
                Some(j) => b.mods.0.count_ones() > self.combos[j].mods.0.count_ones(),
            };
            if better {
                best = Some(i);
            }
        }
        match best {
            Some(i) => {
                self.fired += 1;
                self.swallowed += 1;
                ComboVerdict::Swallowed(self.combos[i].action)
            }
            None => ComboVerdict::Pass,
        }
    }

    /// F166: duplicate bindings — the reason a shortcut "sometimes does not
    /// work" is usually that two components claimed the same chord.
    pub fn conflicts(&self, out: &mut [(&'static str, &'static str)]) -> usize {
        let mut n = 0usize;
        for i in 0..self.len {
            for j in (i + 1)..self.len {
                if self.combos[i].key == self.combos[j].key
                    && self.combos[i].mods == self.combos[j].mods
                {
                    if n < out.len() {
                        out[n] = (self.combos[i].action, self.combos[j].action);
                        n += 1;
                    }
                }
            }
        }
        n
    }
}

impl Default for ComboArbiter {
    fn default() -> ComboArbiter {
        ComboArbiter::new()
    }
}

// ---------------------------------------------------------------------------
// F167/F171/F172 — latency meter, replay, macros
// ---------------------------------------------------------------------------

/// F167: key-to-pixel latency. The press timestamp travels with the event, so
/// the measurement spans the whole chain rather than just the ISR.
pub struct InputLatency {
    press_at: AtomicU64,
    last_ns: AtomicU64,
    worst_ns: AtomicU64,
    samples: AtomicU64,
    over_budget: AtomicU64,
    budget_ns: AtomicU64,
}

/// One frame at 60 Hz. Anything above this is visible.
pub const INPUT_LATENCY_BUDGET_NS: u64 = 16_667_000;

impl InputLatency {
    pub const fn new() -> InputLatency {
        InputLatency {
            press_at: AtomicU64::new(0),
            last_ns: AtomicU64::new(0),
            worst_ns: AtomicU64::new(0),
            samples: AtomicU64::new(0),
            over_budget: AtomicU64::new(0),
            budget_ns: AtomicU64::new(INPUT_LATENCY_BUDGET_NS),
        }
    }

    pub fn note_press(&self) {
        self.press_at
            .store(crate::cpu::clock::read_tsc(), Ordering::Release);
    }

    /// Called when the frame containing that key finally reaches the screen.
    pub fn note_present(&self) -> u64 {
        let at = self.press_at.load(Ordering::Acquire);
        if at == 0 {
            return 0;
        }
        let cal = crate::cpu::clock::calibration();
        let ns = if cal.usable() {
            cal.tsc_to_ns(crate::cpu::clock::read_tsc().wrapping_sub(at))
        } else {
            0
        };
        self.note_sample(ns);
        ns
    }

    pub fn note_sample(&self, ns: u64) {
        self.samples.fetch_add(1, Ordering::Relaxed);
        self.last_ns.store(ns, Ordering::Relaxed);
        self.worst_ns.fetch_max(ns, Ordering::Relaxed);
        if ns > self.budget_ns.load(Ordering::Relaxed) {
            self.over_budget.fetch_add(1, Ordering::Relaxed);
        }
    }

    pub fn last_ns(&self) -> u64 {
        self.last_ns.load(Ordering::Relaxed)
    }
    pub fn worst_ns(&self) -> u64 {
        self.worst_ns.load(Ordering::Relaxed)
    }
    pub fn samples(&self) -> u64 {
        self.samples.load(Ordering::Relaxed)
    }
    pub fn over_budget(&self) -> u64 {
        self.over_budget.load(Ordering::Relaxed)
    }
}

impl Default for InputLatency {
    fn default() -> InputLatency {
        InputLatency::new()
    }
}

static LATENCY: InputLatency = InputLatency::new();

pub fn latency() -> &'static InputLatency {
    &LATENCY
}

pub const MAX_RECORDED_EVENTS: usize = 64;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct RecordedEvent {
    pub tick: u64,
    pub code: KeyCode,
    pub pressed: bool,
    pub mods: Modifiers,
}

/// F171: an input recorder. Input bugs are timing bugs, and the only way to
/// debug a timing bug is to replay it.
pub struct InputRecorder {
    events: [RecordedEvent; MAX_RECORDED_EVENTS],
    len: usize,
    recording: bool,
    replays: u64,
}

impl InputRecorder {
    pub const fn new() -> InputRecorder {
        InputRecorder {
            events: [RecordedEvent {
                tick: 0,
                code: KeyCode::UNKNOWN,
                pressed: false,
                mods: Modifiers(0),
            }; MAX_RECORDED_EVENTS],
            len: 0,
            recording: false,
            replays: 0,
        }
    }

    pub fn start(&mut self) {
        self.len = 0;
        self.recording = true;
    }

    pub fn stop(&mut self) -> usize {
        self.recording = false;
        self.len
    }

    pub fn is_recording(&self) -> bool {
        self.recording
    }

    pub fn record(&mut self, code: KeyCode, pressed: bool, mods: Modifiers) -> bool {
        if !self.recording || self.len >= MAX_RECORDED_EVENTS {
            return false;
        }
        self.events[self.len] = RecordedEvent {
            tick: crate::cpu::clock::ticks(),
            code,
            pressed,
            mods,
        };
        self.len += 1;
        true
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn get(&self, i: usize) -> Option<RecordedEvent> {
        if i < self.len {
            Some(self.events[i])
        } else {
            None
        }
    }

    pub fn replays(&self) -> u64 {
        self.replays
    }

    /// Replay into a sink. Timing is deliberately *not* reproduced: the point
    /// is the sequence, and a replay that tried to be real-time would be
    /// flaky in exactly the tests it is meant to stabilise.
    pub fn replay(&mut self, mut sink: impl FnMut(RecordedEvent)) -> usize {
        for e in self.events[..self.len].iter() {
            sink(*e);
        }
        self.replays += 1;
        self.len
    }

    /// How long the recorded session lasted, in ticks.
    pub fn duration_ticks(&self) -> u64 {
        match (self.events.first(), self.events[..self.len].last()) {
            (Some(a), Some(b)) => b.tick.saturating_sub(a.tick),
            _ => 0,
        }
    }
}

impl Default for InputRecorder {
    fn default() -> InputRecorder {
        InputRecorder::new()
    }
}

/// F172: a named macro. Fixed capacity like everything else in the input path;
/// a macro that could grow without bound is a macro that can wedge the machine.
pub const MAX_MACRO_EVENTS: usize = 16;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Macro {
    pub name: &'static str,
    pub events: [RecordedEvent; MAX_MACRO_EVENTS],
    pub len: usize,
    pub plays: u64,
}

impl Macro {
    pub const fn new(name: &'static str) -> Macro {
        Macro {
            name,
            events: [RecordedEvent {
                tick: 0,
                code: KeyCode::UNKNOWN,
                pressed: false,
                mods: Modifiers(0),
            }; MAX_MACRO_EVENTS],
            len: 0,
            plays: 0,
        }
    }

    pub fn push(&mut self, code: KeyCode, pressed: bool, mods: Modifiers) -> bool {
        if self.len >= MAX_MACRO_EVENTS {
            return false;
        }
        self.events[self.len] = RecordedEvent {
            tick: 0,
            code,
            pressed,
            mods,
        };
        self.len += 1;
        true
    }

    pub fn replay(&mut self, mut sink: impl FnMut(RecordedEvent)) -> usize {
        for e in self.events[..self.len].iter() {
            sink(*e);
        }
        self.plays += 1;
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

// ---------------------------------------------------------------------------
// F173/F174 — degradation and key lighting
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug, PartialOrd, Ord)]
pub enum InputPath {
    /// Nothing at all — the machine is unusable and must say so.
    None = 0,
    /// PS/2, which works before USB is up.
    Ps2 = 1,
    /// USB HID.
    Usb = 2,
}

impl InputPath {
    pub fn as_str(self) -> &'static str {
        match self {
            InputPath::None => "none",
            InputPath::Ps2 => "ps2",
            InputPath::Usb => "usb",
        }
    }

    /// F173: pick the best path available. PS/2 is the fallback that keeps a
    /// machine with a broken xHCI findable.
    pub fn best(usb_present: bool, ps2_present: bool) -> InputPath {
        if usb_present {
            InputPath::Usb
        } else if ps2_present {
            InputPath::Ps2
        } else {
            InputPath::None
        }
    }

    pub fn degraded(self) -> bool {
        self == InputPath::Ps2
    }

    pub fn usable(self) -> bool {
        self != InputPath::None
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct LedState {
    pub num_lock: bool,
    pub caps_lock: bool,
    pub scroll_lock: bool,
    pub compose: bool,
}

impl LedState {
    /// The HID output report byte.
    pub fn to_report(self) -> u8 {
        let mut v = 0u8;
        if self.num_lock {
            v |= 1;
        }
        if self.caps_lock {
            v |= 2;
        }
        if self.scroll_lock {
            v |= 4;
        }
        if self.compose {
            v |= 8;
        }
        v
    }

    pub fn from_report(v: u8) -> LedState {
        LedState {
            num_lock: v & 1 != 0,
            caps_lock: v & 2 != 0,
            scroll_lock: v & 4 != 0,
            compose: v & 8 != 0,
        }
    }

    /// Caps lock changes the effective shift state for character keys.
    pub fn effective_shift(self, mods: Modifiers) -> Modifiers {
        mods.with(Modifiers::SHIFT, mods.has(Modifiers::SHIFT) ^ self.caps_lock)
    }
}

/// F170: a 2.4 GHz dongle presents itself as a plain HID device, so the only
/// thing the kernel tracks is its wakeup capability and battery report.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct WirelessReceiver {
    pub vendor: u16,
    pub product: u16,
    pub devices_paired: u8,
    pub battery_percent: u8,
    pub can_wake: bool,
}

impl WirelessReceiver {
    pub fn battery_ok(&self) -> bool {
        self.battery_percent > 5
    }
}

// ---------------------------------------------------------------------------
// Domain state and self-test
// ---------------------------------------------------------------------------

pub struct InputDomainState {
    pub path: InputPath,
    pub plane: KeyPlane,
    pub combos: usize,
    pub conflicts: usize,
    pub latency_worst_ms: u64,
    pub hids: usize,
    pub self_test: (usize, usize),
}

impl InputDomainState {
    pub fn ok(&self) -> bool {
        self.self_test.1 == 0
    }

    pub fn render(&self, out: &mut [u8]) -> usize {
        let mut w = crate::cpu::Hud::new(out);
        w.str("input path=");
        w.str(self.path.as_str());
        w.str(" plane=");
        w.str(self.plane.as_str());
        w.str(" combos=");
        w.num(self.combos as u64);
        w.str(" conflicts=");
        w.num(self.conflicts as u64);
        w.str(" latency=");
        w.num(self.latency_worst_ms);
        w.str("ms [");
        w.num(self.self_test.0 as u64);
        w.str("/");
        w.num((self.self_test.0 + self.self_test.1) as u64);
        w.str("]\n");
        w.used()
    }
}

static INPUT_SELFTEST: crate::selftest::SelfTest = crate::selftest::SelfTest::new();
static SCROLL: crate::cpu::sync::SpinProtected<InertialScroll> =
    crate::cpu::sync::SpinProtected::new(InertialScroll::new());
static PLANES: crate::cpu::sync::SpinProtected<KeyPlanes> =
    crate::cpu::sync::SpinProtected::new(KeyPlanes::new());
static ARBITER: crate::cpu::sync::SpinProtected<ComboArbiter> =
    crate::cpu::sync::SpinProtected::new(ComboArbiter::new());

pub fn input_selftest() -> &'static crate::selftest::SelfTest {
    &INPUT_SELFTEST
}

pub fn planes() -> &'static crate::cpu::sync::SpinProtected<KeyPlanes> {
    &PLANES
}

pub fn arbiter() -> &'static crate::cpu::sync::SpinProtected<ComboArbiter> {
    &ARBITER
}

pub fn scroll() -> &'static crate::cpu::sync::SpinProtected<InertialScroll> {
    &SCROLL
}

/// F151~F175 bring-up.
pub fn init() -> InputDomainState {
    // The host plane gets the shortcuts the desktop advertises; the guest plane
    // starts empty because it belongs to whatever runs in the guest (W5).
    {
        let mut p = PLANES.lock();
        let host = p.host_mut();
        let _ = host.bind(KeyCode(0x1F), Modifiers(0), "launcher");
        let _ = host.bind(KeyCode(0x0F), Modifiers(Modifiers::ALT), "window-switch");
        let _ = host.bind(KeyCode(0x20), Modifiers(Modifiers::META), "settings");
        let _ = host.bind(KeyCode(0x2E), Modifiers(Modifiers::CTRL), "interrupt");
    }
    {
        let mut a = ARBITER.lock();
        let _ = a.register(KeyCode(0x2E), Modifiers(Modifiers::CTRL), "signal-interrupt");
        let _ = a.register(KeyCode(0x20), Modifiers(Modifiers::META), "open-settings");
        let _ = a.register(
            KeyCode(0x14),
            Modifiers(Modifiers::CTRL | Modifiers::SHIFT),
            "close-window",
        );
    }

    let (passed, failed) = run_input_checks();
    let conflicts = {
        let a = ARBITER.lock();
        let mut buf = [("", ""); 8];
        a.conflicts(&mut buf)
    };
    let state = InputDomainState {
        // No transport is probed yet; PS/2 is what the firmware guarantees.
        path: InputPath::best(false, true),
        plane: PLANES.lock().focused_plane(),
        combos: ARBITER.lock().len(),
        conflicts,
        latency_worst_ms: LATENCY.worst_ns() / 1_000_000,
        hids: 0,
        self_test: (passed, failed),
    };

    crate::kinfo!(
        "input: path={} plane={} combos={} conflicts={} self-test {}/{}",
        state.path.as_str(),
        state.plane.as_str(),
        state.combos,
        state.conflicts,
        state.self_test.0,
        state.self_test.0 + state.self_test.1
    );
    state
}

pub fn render_to_console(st: &InputDomainState) {
    let mut buf = [0u8; 256];
    let n = st.render(&mut buf);
    if let Some(c) = crate::console::installed_ref() {
        for &b in &buf[..n] {
            c.put_byte(b);
        }
    }
    let mut detail = [0u8; 512];
    let m = INPUT_SELFTEST.render(&mut detail);
    if let Some(c) = crate::console::installed_ref() {
        for &b in &detail[..m] {
            c.put_byte(b);
        }
    }
}

/// F175: the input-chain checks.
pub fn run_input_checks() -> (usize, usize) {
    let r = &INPUT_SELFTEST;

    // F152 — the Set-1 decoder handles make, break and the E0 prefix.
    let mut d = ScancodeDecoder::new();
    let press = d.feed(0x1E).map(|e| e.pressed && e.code == KeyCode::A);
    let release = d.feed(0x9E).map(|e| !e.pressed && e.code == KeyCode::A);
    let extended = {
        let _ = d.feed(SC_EXTENDED);
        d.feed(0x4B).map(|e| e.extended).unwrap_or(false)
    };
    r.check(
        "scancode-set1",
        press == Some(true) && release == Some(true) && extended,
        "Set-1 decoding is wrong",
    );

    // F164 — the repeat curve tightens and then holds.
    let curve = RepeatCurve::standard();
    let fast = curve.interval_for(curve.ramp_repeats) == curve.min_interval;
    let slow = curve.next_delay(0) == curve.delay_ticks;
    r.check("repeat-curve", fast && slow && curve.valid(), "repeat curve is wrong");

    // F155 — focus selects the plane, and routing follows it.
    let mut p = KeyPlanes::new();
    let _ = p.host_mut().bind(KeyCode::SPACE, Modifiers(0), "host-space");
    let _ = p.guest_mut().bind(KeyCode::SPACE, Modifiers(0), "guest-space");
    let host = p.route(KeyCode::SPACE, Modifiers(0));
    p.focus(KeyPlane::Guest);
    let guest = p.route(KeyCode::SPACE, Modifiers(0));
    r.check(
        "focus-routing",
        host.action == "host-space" && guest.action == "guest-space",
        "focus did not switch the key table",
    );

    // F165 — the more specific combo wins.
    let mut a = ComboArbiter::new();
    let _ = a.register(KeyCode(0x14), Modifiers(Modifiers::CTRL), "close");
    let _ = a.register(
        KeyCode(0x14),
        Modifiers(Modifiers::CTRL | Modifiers::SHIFT),
        "close-all",
    );
    let plain = a.resolve(KeyCode(0x14), Modifiers(Modifiers::CTRL));
    let both = a.resolve(KeyCode(0x14), Modifiers(Modifiers::CTRL | Modifiers::SHIFT));
    r.check(
        "combo-specificity",
        plain == ComboVerdict::Swallowed("close")
            && both == ComboVerdict::Swallowed("close-all"),
        "combo arbitration picked the wrong binding",
    );

    // F166 — duplicate bindings are reported, not silently ignored.
    let mut dup = ComboArbiter::new();
    let _ = dup.register(KeyCode(0x14), Modifiers(Modifiers::CTRL), "one");
    let _ = dup.register(KeyCode(0x14), Modifiers(Modifiers::CTRL), "two");
    let mut pairs = [("", ""); 4];
    r.check(
        "conflict-radar",
        dup.conflicts(&mut pairs) == 1 && pairs[0] == ("one", "two"),
        "a duplicate shortcut was not reported",
    );

    // F158 — a real boot-keyboard report descriptor parses.
    let desc = [
        0x05, 0x01, // Usage Page (Generic Desktop)
        0x09, 0x06, // Usage (Keyboard)
        0xA1, 0x01, // Collection (Application)
        0x95, 0x08, // Report Count (8)
        0x75, 0x01, // Report Size (1)
        0x05, 0x07, // Usage Page (Keyboard)
        0x19, 0xE0, // Usage Minimum (224)
        0x29, 0xE7, // Usage Maximum (231)
        0x81, 0x02, // Input (Data,Var,Abs)
        0xC0, // End Collection
    ];
    let info = parse_report_descriptor(&desc);
    r.check(
        "hid-descriptor",
        info.map(|i| i.has_input && i.usage_page == 0x07 && i.report_count == 8)
            .unwrap_or(false),
        "report descriptor parsing failed",
    );
    r.check(
        "hid-descriptor-rejects",
        parse_report_descriptor(&[0x05]).is_err()
            && parse_report_descriptor(&[0x05, 0x01]).is_err(),
        "a truncated descriptor was accepted",
    );

    // F173 — the input path degrades rather than disappearing.
    r.check(
        "input-degradation",
        InputPath::best(false, true) == InputPath::Ps2
            && InputPath::best(true, false) == InputPath::Usb
            && !InputPath::best(false, false).usable(),
        "input path selection is wrong",
    );

    // F174 — the LED report round-trips.
    let leds = LedState {
        num_lock: true,
        caps_lock: false,
        scroll_lock: true,
        compose: false,
    };
    r.check(
        "led-report",
        LedState::from_report(leds.to_report()) == leds && leds.to_report() == 5,
        "LED report does not round-trip",
    );

    let (passed, failed) = r.tally();
    if failed == 0 {
        crate::kinfo!("input self-test: {}/{} pass", passed, passed);
    } else {
        crate::kwarn!(
            "input self-test: {}/{} pass ({} FAIL)",
            passed,
            passed + failed,
            failed
        );
    }
    (passed, failed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set1_scancodes_decode_make_break_and_extended() {
        let mut d = ScancodeDecoder::new();
        let a = d.feed(0x1E).unwrap();
        assert_eq!(a.code, KeyCode::A);
        assert!(a.pressed);
        assert!(!a.extended);
        let release = d.feed(0x9E).unwrap();
        assert_eq!(release.code, KeyCode::A);
        assert!(!release.pressed, "the high bit means release");

        // E0 prefix: the next byte is an extended code.
        assert!(d.feed(SC_EXTENDED).is_none());
        assert!(d.in_escape_sequence());
        let right_arrow = d.feed(0x4D).unwrap();
        assert!(right_arrow.extended);
        assert!(right_arrow.pressed);
        assert!(!d.in_escape_sequence(), "the prefix is consumed once");
        // A second E0 in a row is just another prefix.
        let _ = d.feed(SC_EXTENDED);
        let _ = d.feed(SC_EXTENDED);
        assert!(d.in_escape_sequence());
    }

    #[test]
    fn keycodes_map_to_ascii_shifted_and_unshifted() {
        assert_eq!(KeyCode::A.ascii_unshifted(), Some(b'a'));
        assert_eq!(KeyCode::A.ascii_shifted(), Some(b'A'));
        assert_eq!(KeyCode::N1.ascii_unshifted(), Some(b'1'));
        assert_eq!(KeyCode::N1.ascii_shifted(), Some(b'!'));
        assert_eq!(KeyCode::SPACE.ascii_unshifted(), Some(b' '));
        assert_eq!(KeyCode::SPACE.ascii_shifted(), Some(b' '));
        assert_eq!(KeyCode::ENTER.ascii_unshifted(), None);
        assert_eq!(KeyCode(0x35).ascii_shifted(), Some(b'?'));
        assert!(!KeyCode::A.valid() == false);
        assert!(!KeyCode::UNKNOWN.valid());
    }

    #[test]
    fn layouts_and_focus_route_keys_to_the_right_plane() {
        let mut p = KeyPlanes::new();
        let _ = p.host_mut().bind(KeyCode(0x1F), Modifiers(Modifiers::ALT), "launcher");
        let _ = p.guest_mut().bind(KeyCode(0x1F), Modifiers(Modifiers::ALT), "guest-menu");
        assert_eq!(p.focused_plane(), KeyPlane::Host);
        assert_eq!(p.route(KeyCode(0x1F), Modifiers(Modifiers::ALT)).action, "launcher");
        assert!(p.focus(KeyPlane::Guest));
        assert!(!p.focus(KeyPlane::Guest), "focusing the same plane is a no-op");
        assert_eq!(p.switches(), 1);
        let routed = p.route(KeyCode(0x1F), Modifiers(Modifiers::ALT));
        assert_eq!(routed.action, "guest-menu");
        assert_eq!(routed.plane, KeyPlane::Guest);
        assert!(!routed.passthrough);
        // An unbound key is still delivered, just with no action.
        assert_eq!(p.route(KeyCode(0x2C), Modifiers(0)).action, "");
        // The layout also resolves characters.
        assert_eq!(p.host().char_for(KeyCode::A, Modifiers(0)), Some(b'a'));
        assert_eq!(p.host().char_for(KeyCode::A, Modifiers(Modifiers::SHIFT)), Some(b'A'));
        assert_eq!(KeyPlane::Host.as_str(), "host");
    }

    #[test]
    fn xhci_trbs_and_rings_flip_the_cycle_bit() {
        let trb = Trb {
            parameter: 0xDEAD_BEEF_CAFE_0000,
            status: 8,
            kind: Trb::KIND_NORMAL,
            cycle: true,
        };
        let wire = trb.encode();
        assert_eq!(wire.len(), Trb::LEN);
        let back = Trb::decode(&wire);
        assert_eq!(back, trb);
        assert_eq!(back.parameter, 0xDEAD_BEEF_CAFE_0000);

        let mut ring = XhciRing::new(4);
        let first = ring.push().unwrap();
        assert_eq!(first, (0, true));
        for _ in 0..3 {
            assert!(ring.push().is_some());
        }
        assert!(ring.push().is_none(), "a full ring refuses");
        assert!(ring.pop());
        let wrap = ring.push().unwrap();
        assert_eq!(wrap.0, 0, "the index wrapped");
        assert!(!wrap.1, "and the cycle bit flipped");
        for _ in 0..4 {
            assert!(ring.pop());
        }
        assert_eq!(ring.pending, 0, "the ring drained");
        assert!(!ring.pop(), "nothing left to complete");
    }

    #[test]
    fn usb_descriptors_parse_and_classify() {
        let mut b = [0u8; 18];
        b[0] = 18;
        b[1] = 1;
        b[2] = 0x00;
        b[3] = 0x02; // USB 2.0
        b[4] = 3; // HID
        b[8] = 0x6B;
        b[9] = 0x04; // vendor 0x046B
        b[10] = 0x01;
        b[11] = 0xA0; // product 0xA001
        b[17] = 1;
        let d = DeviceDescriptor::parse(&b).unwrap();
        assert_eq!(d.usb_version, 0x0200);
        assert!(d.is_hid());
        assert_eq!(d.configurations, 1);
        let (v, p) = d.vendor_product_str();
        assert_eq!(&v, b"046B");
        assert_eq!(&p, b"A001");
        assert_eq!(DeviceDescriptor::parse(&b[..10]).unwrap_err(), "device descriptor is truncated");
        let mut wrong = b;
        wrong[1] = 2;
        assert_eq!(DeviceDescriptor::parse(&wrong).unwrap_err(), "not a device descriptor");

        let mut i = [0u8; 9];
        i[0] = 9;
        i[1] = 4;
        i[2] = 1; // interface number
        i[3] = 0; // alternate setting
        i[4] = 1; // one endpoint
        i[5] = 3; // HID class
        i[6] = 1; // boot subclass
        i[7] = 1; // keyboard protocol
        let iface = InterfaceDescriptor::parse(&i).unwrap();
        assert_eq!(iface.endpoints, 1);
        assert!(iface.is_boot_keyboard());
        assert!(!iface.is_boot_mouse());
        i[7] = 2;
        assert!(InterfaceDescriptor::parse(&i).unwrap().is_boot_mouse());
        assert_eq!(InterfaceDescriptor::parse(&[]).unwrap_err(), "not an interface descriptor");
    }

    #[test]
    fn hid_descriptor_and_boot_reports() {
        let desc = [
            0x05, 0x01, 0x09, 0x06, 0xA1, 0x01, 0x95, 0x08, 0x75, 0x01, 0x05, 0x07, 0x19, 0xE0,
            0x29, 0xE7, 0x81, 0x02, 0x95, 0x06, 0x75, 0x08, 0x81, 0x00, 0xC0,
        ];
        let info = parse_report_descriptor(&desc).unwrap();
        assert!(info.has_input);
        assert_eq!(info.usage_page, 0x07);
        assert_eq!(info.report_size, 8);
        assert_eq!(info.report_count, 6);
        assert_eq!(info.input_bits, 8 + 48);
        assert_eq!(parse_report_descriptor(&[]).unwrap_err(), "empty report descriptor");
        assert_eq!(
            parse_report_descriptor(&[0xFE, 0x00]).unwrap_err(),
            "long items are not supported"
        );

        let mut rep = [0u8; 8];
        rep[0] = Modifiers::CTRL | Modifiers::SHIFT;
        rep[2] = 0x1E; // A
        rep[3] = 0x1F; // S
        let kb = BootKeyboardReport::parse(&rep).unwrap();
        assert!(kb.modifiers.has(Modifiers::CTRL));
        assert_eq!(kb.key_count(), 2);
        assert!(kb.keys.contains(&KeyCode(0x1E)));
        assert_eq!(BootKeyboardReport::parse(&[0u8; 4]).unwrap_err(), "keyboard report is truncated");

        // Edge detection: only the newly pressed key is reported.
        let mut next = rep;
        next[2] = 0x1E;
        next[3] = 0x20; // D replaces S
        let kb2 = BootKeyboardReport::parse(&next).unwrap();
        let mut out = [KeyCode::UNKNOWN; 6];
        let pressed = kb2.pressed_since(&kb, &mut out);
        assert_eq!(pressed, 1);
        assert_eq!(out[0], KeyCode(0x20));
        let released = kb2.released_since(&kb, &mut out);
        assert_eq!(released, 1);
        assert_eq!(out[0], KeyCode(0x1F));

        let mouse = BootMouseReport::parse(&[0x03, 0xFF, 0x01, 0xFE]).unwrap();
        assert!(mouse.left() && mouse.right() && !mouse.middle());
        assert_eq!(mouse.dx, -1, "dx is signed");
        assert_eq!(mouse.dy, 1);
        assert_eq!(mouse.wheel, -2);
    }

    #[test]
    fn gestures_taps_and_pen_pressure() {
        let mut t = TouchTracker::default();
        t.begin(100, 100, 1);
        t.update(101, 101);
        t.tick();
        assert_eq!(t.finish(0), Gesture::Tap);

        let mut swipe = TouchTracker::default();
        swipe.begin(0, 0, 1);
        swipe.update(60, 5);
        for _ in 0..5 {
            swipe.tick();
        }
        match swipe.finish(0) {
            Gesture::Swipe { dx, dy } => {
                assert_eq!(dx, 60);
                assert_eq!(dy, 5);
            }
            other => panic!("expected a swipe, got {other:?}"),
        }

        let mut pinch = TouchTracker::default();
        pinch.begin(0, 0, 2);
        pinch.set_span(100);
        pinch.update(20, 0);
        assert_eq!(pinch.finish(150), Gesture::Pinch { scale_permille: 1500 });

        let mut scroll = TouchTracker::default();
        scroll.begin(0, 0, 2);
        scroll.set_span(80);
        scroll.update(0, 30);
        match scroll.finish(80) {
            Gesture::Scroll { dy, .. } => assert_eq!(dy, 30),
            other => panic!("expected a scroll, got {other:?}"),
        }

        // A long still press is not a tap.
        let mut hold = TouchTracker::default();
        hold.begin(0, 0, 1);
        for _ in 0..(TouchTracker::TAP_MAX_TICKS + 1) {
            hold.tick();
        }
        assert_eq!(hold.finish(0), Gesture::None);

        let pen = PointerSample {
            x: 10,
            y: 20,
            pressure: PointerSample::MAX_PRESSURE / 2,
            tilt_x: 5,
            tilt_y: 0,
            contact: true,
        };
        assert!(pen.is_pencil());
        assert_eq!(pen.pressure_percent(), 50);
        assert!(pen.is_tap_candidate(4));
        assert!(!pen.is_tap_candidate(40));
        assert!(!PointerSample::default().is_pencil());
    }

    #[test]
    fn inertial_scroll_physics_and_repeat_curve() {
        let mut s = InertialScroll::new();
        assert!(!s.is_moving());
        s.nudge(1);
        assert!(s.is_moving());
        let mut total = 0;
        let mut ticks = 0;
        while s.is_moving() && ticks < 200 {
            total += s.step();
            ticks += 1;
        }
        assert!(ticks < 200, "the scroll must settle");
        // A one-notch flick delivers at least one notch and then decays.
        assert!(total >= 1);
        let mut back = InertialScroll::new();
        back.nudge(-3);
        let mut neg = 0;
        for _ in 0..200 {
            neg += back.step();
            if !back.is_moving() {
                break;
            }
        }
        assert!(neg <= -1, "a negative flick scrolls the other way");

        let curve = RepeatCurve::standard();
        assert!(curve.valid());
        assert_eq!(curve.next_delay(0), curve.delay_ticks);
        assert!(curve.interval_for(1) > curve.interval_for(curve.ramp_repeats));
        assert_eq!(curve.interval_for(curve.ramp_repeats * 2), curve.min_interval);
        assert!(RepeatCurve::slow().min_interval > curve.min_interval);
        assert!(!RepeatCurve {
            delay_ticks: 10,
            first_interval: 1,
            min_interval: 4,
            ramp_repeats: 4
        }
        .valid());
    }

    #[test]
    fn combos_conflicts_recorder_and_macro() {
        let mut a = ComboArbiter::new();
        assert!(a.is_empty());
        let _ = a.register(KeyCode(0x14), Modifiers(Modifiers::CTRL), "close");
        let _ = a.register(
            KeyCode(0x14),
            Modifiers(Modifiers::CTRL | Modifiers::SHIFT),
            "close-all",
        );
        let _ = a.register(KeyCode(0x2E), Modifiers(Modifiers::CTRL), "sigint");
        assert_eq!(a.len(), 3);
        assert_eq!(
            a.resolve(KeyCode(0x14), Modifiers(Modifiers::CTRL)),
            ComboVerdict::Swallowed("close")
        );
        assert_eq!(
            a.resolve(
                KeyCode(0x14),
                Modifiers(Modifiers::CTRL | Modifiers::SHIFT)
            ),
            ComboVerdict::Swallowed("close-all")
        );
        // A key with no modifiers is never a system combo.
        assert_eq!(a.resolve(KeyCode(0x2E), Modifiers(0)), ComboVerdict::Pass);
        assert_eq!(a.resolve(KeyCode(0x2C), Modifiers(Modifiers::CTRL)), ComboVerdict::Pass);
        assert_eq!(a.fired(), 2);

        let mut dup = ComboArbiter::new();
        let _ = dup.register(KeyCode(1), Modifiers(Modifiers::CTRL), "x");
        let _ = dup.register(KeyCode(1), Modifiers(Modifiers::CTRL), "y");
        let _ = dup.register(KeyCode(2), Modifiers(Modifiers::CTRL), "z");
        let mut pairs = [("", ""); 4];
        assert_eq!(dup.conflicts(&mut pairs), 1);
        assert_eq!(pairs[0], ("x", "y"));

        let mut rec = InputRecorder::new();
        assert!(!rec.record(KeyCode::A, true, Modifiers(0)), "not recording yet");
        rec.start();
        assert!(rec.is_recording());
        assert!(rec.record(KeyCode::A, true, Modifiers(0)));
        assert!(rec.record(KeyCode::A, false, Modifiers(0)));
        assert_eq!(rec.stop(), 2);
        assert!(!rec.is_recording());
        assert_eq!(rec.len(), 2);
        assert_eq!(rec.get(0).unwrap().code, KeyCode::A);
        assert!(rec.get(5).is_none());
        let mut replayed = 0;
        assert_eq!(rec.replay(|_| replayed += 1), 2);
        assert_eq!(replayed, 2);
        assert_eq!(rec.replays(), 1);

        let mut m = Macro::new("copy-paste");
        assert!(m.is_empty());
        assert!(m.push(KeyCode(0x2E), true, Modifiers(Modifiers::CTRL)));
        assert!(m.push(KeyCode(0x2E), false, Modifiers(Modifiers::CTRL)));
        let mut seen = 0;
        assert_eq!(m.replay(|_| seen += 1), 2);
        assert_eq!(seen, 2);
        assert_eq!(m.plays, 1);
        for _ in 0..MAX_MACRO_EVENTS {
            let _ = m.push(KeyCode::A, true, Modifiers(0));
        }
        assert!(!m.push(KeyCode::A, true, Modifiers(0)), "macros are bounded");
    }

    #[test]
    fn paths_leds_remote_gamepad_and_wireless() {
        assert_eq!(InputPath::best(true, true), InputPath::Usb);
        assert_eq!(InputPath::best(false, true), InputPath::Ps2);
        assert!(!InputPath::best(false, true).degraded() == false);
        assert!(InputPath::Ps2.degraded(), "PS/2 is the documented fallback");
        assert!(!InputPath::best(false, false).usable());
        assert!(InputPath::Usb > InputPath::Ps2);
        assert!(!InputPath::None.as_str().is_empty());

        let mut leds = LedState::default();
        assert_eq!(leds.to_report(), 0);
        leds.caps_lock = true;
        assert_eq!(leds.to_report(), 2);
        assert_eq!(LedState::from_report(2), leds);
        // Caps lock inverts the effective shift for character keys.
        assert!(leds.effective_shift(Modifiers(0)).has(Modifiers::SHIFT));
        assert!(!leds.effective_shift(Modifiers(Modifiers::SHIFT)).has(Modifiers::SHIFT));
        assert!(Modifiers(Modifiers::CTRL).is_shortcut());
        assert!(!Modifiers(0).is_shortcut());
        assert_eq!(Modifiers(Modifiers::CTRL).render(&mut [0u8; 16]), 4);

        assert_eq!(RemoteKey::from_usage(0xE9), Some(RemoteKey::VolumeUp));
        assert_eq!(RemoteKey::from_usage(0xE2), Some(RemoteKey::Mute));
        assert_eq!(RemoteKey::from_usage(0x1234), None);

        let pad = GamepadState {
            left_x: 100,
            left_y: 20000,
            ..GamepadState::default()
        };
        assert_eq!(GamepadState::deadzone(100), 0, "inside the dead zone");
        assert_eq!(GamepadState::deadzone(5000), 5000, "past the dead zone");
        assert_eq!(pad.normalised_left(), (0, 20000));
        assert!(!pad.button(0));
        assert!(GamepadState {
            buttons: 1,
            ..pad
        }
        .button(0));

        let dongle = WirelessReceiver {
            vendor: 0x046D,
            product: 0xC534,
            devices_paired: 2,
            battery_percent: 80,
            can_wake: true,
        };
        assert!(dongle.battery_ok());
        assert!(!WirelessReceiver {
            battery_percent: 2,
            ..dongle
        }
        .battery_ok());

        let lat = InputLatency::new();
        lat.note_sample(5_000_000);
        assert_eq!(lat.samples(), 1);
        assert_eq!(lat.worst_ns(), 5_000_000);
        assert_eq!(lat.over_budget(), 0);
        lat.note_sample(INPUT_LATENCY_BUDGET_NS + 1);
        assert_eq!(lat.over_budget(), 1);
        lat.note_press();
        // Presenting without a recorded press reports zero rather than lying.
        assert_eq!(InputLatency::new().note_present(), 0);
    }

    #[test]
    fn self_test_passes() {
        let (passed, failed) = run_input_checks();
        assert_eq!(failed, 0, "{passed} passed, {failed} failed");
        assert!(passed >= 8);
    }
}
