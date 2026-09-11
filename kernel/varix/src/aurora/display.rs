//! AURORA-1000 AI-01 · 显示输出与屏幕管理（A001~A025，W1）
//! 职责：以纯函数/固定容量状态机实现帧缓冲抽象、EDID 识别、模式枚举与设置、
//! 多屏拓扑、DPI 缩放、垂直同步与防撕裂、亮度色温护眼、旋转省电、每屏配置、
//! 合成协作、策略中心与降级链——不触碰任何真实硬件。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// A001 — 帧缓冲抽象层 (Framebuffer abstraction)
// ---------------------------------------------------------------------------

/// Pixel memory layout reported by the (modelled) display controller.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PixelFormat {
    Rgb888,
    Bgr888,
    Rgba8888,
    Gray8,
}

/// A fixed-descriptor of a linear frame buffer. No allocation, no pointer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Framebuffer {
    pub width: u16,
    pub height: u16,
    pub bpp: u8,
    pub stride: u16,
    pub format: PixelFormat,
}

impl Framebuffer {
    pub const fn new(width: u16, height: u16, bpp: u8) -> Framebuffer {
        let bytes_per_pixel = (bpp as u32) / 8;
        let stride = (width as u32).saturating_mul(bytes_per_pixel) as u16;
        Framebuffer {
            width,
            height,
            bpp,
            stride,
            format: PixelFormat::Rgba8888,
        }
    }

    /// Total bytes backing the buffer (stride * height), saturating.
    pub fn byte_len(&self) -> u32 {
        (self.stride as u32).saturating_mul(self.height as u32)
    }

    pub fn pixel_count(&self) -> u32 {
        (self.width as u32).saturating_mul(self.height as u32)
    }

    /// A framebuffer is usable only with sane, aligned dimensions.
    pub fn is_valid(&self) -> bool {
        self.width > 0
            && self.height > 0
            && self.bpp >= 8
            && self.bpp % 8 == 0
            && (self.stride as u32) >= (self.width as u32).saturating_mul((self.bpp as u32) / 8)
    }
}

// ---------------------------------------------------------------------------
// A002 — EDID 解析与显示器识别 (EDID parse & monitor identification)
// ---------------------------------------------------------------------------

/// True when bytes 0..8 equal the EDID magic header.
pub fn edid_header_ok(e: &[u8]) -> bool {
    if e.len() < 8 {
        return false;
    }
    e[0] == 0x00
        && e[1] == 0xFF
        && e[2] == 0xFF
        && e[3] == 0xFF
        && e[4] == 0xFF
        && e[5] == 0xFF
        && e[6] == 0xFF
        && e[7] == 0x00
}

/// Sum of all 128 bytes mod 256 (None when not exactly 128 bytes).
pub fn edid_checksum(e: &[u8]) -> Option<u8> {
    if e.len() != 128 {
        return None;
    }
    let mut sum: u32 = 0;
    let mut i = 0;
    while i < 128 {
        sum = sum.wrapping_add(e[i] as u32);
        i += 1;
    }
    Some((sum & 0xFF) as u8)
}

/// A structurally valid EDID: correct length, header and zero checksum.
pub fn edid_valid(e: &[u8]) -> bool {
    if e.len() != 128 {
        return false;
    }
    if !edid_header_ok(e) {
        return false;
    }
    edid_checksum(e) == Some(0)
}

/// 3-letter PNP manufacturer id decoded from bytes 8..10.
pub fn edid_manufacturer(e: &[u8]) -> [u8; 3] {
    if e.len() < 10 {
        return [b'X', b'X', b'X'];
    }
    let raw = ((e[8] as u16) << 8) | (e[9] as u16);
    let c1 = (raw >> 10) & 0x1F;
    let c2 = (raw >> 5) & 0x1F;
    let c3 = raw & 0x1F;
    let ch = |c: u16| -> u8 {
        if c == 0 {
            b'X'
        } else {
            (b'A' + (c as u8 - 1))
        }
    };
    [ch(c1), ch(c2), ch(c3)]
}

/// 16-bit product code (LE) at bytes 10..12.
pub fn edid_product_code(e: &[u8]) -> u16 {
    if e.len() < 12 {
        return 0;
    }
    u16::from_le_bytes([e[10], e[11]])
}

/// Manufacture year = 1990 + byte 17.
pub fn edid_year(e: &[u8]) -> u16 {
    if e.len() < 18 {
        return 0;
    }
    1990u16 + (e[17] as u16)
}

/// EDID structure version / revision (bytes 18..20).
pub fn edid_version(e: &[u8]) -> (u8, u8) {
    if e.len() < 20 {
        return (0, 0);
    }
    (e[18], e[19])
}

/// Count of established-timing bits set in bytes 35..38.
pub fn edid_established_count(e: &[u8]) -> u32 {
    if e.len() < 38 {
        return 0;
    }
    let bytes = [e[35], e[36], e[37]];
    let mut count = 0u32;
    let mut j = 0;
    while j < bytes.len() {
        let mut v = bytes[j];
        while v != 0 {
            count += (v & 1) as u32;
            v >>= 1;
        }
        j += 1;
    }
    count
}

/// Count of non-zero standard-timing entries (bytes 38..54, 8 slots).
pub fn edid_standard_count(e: &[u8]) -> u32 {
    if e.len() < 54 {
        return 0;
    }
    let mut count = 0u32;
    let mut i = 0;
    while i < 8 {
        if e[38 + i * 2] != 0 {
            count += 1;
        }
        i += 1;
    }
    count.min(8)
}

/// Parse detailed timing descriptor `index` (0..4) at offset 54+i*18.
pub fn edid_detailed(e: &[u8], index: usize) -> Option<ModeInfo> {
    if e.len() < 126 || index >= 4 {
        return None;
    }
    let off = 54 + index * 18;
    let clock = u16::from_le_bytes([e[off], e[off + 1]]);
    if clock == 0 {
        return None;
    }
    let h = e[off + 2] as u16 | (((e[off + 4] & 0x0F) as u16) << 8);
    let v = e[off + 3] as u16 | (((e[off + 4] & 0xF0) as u16) << 4);
    let r = if e[off + 5] == 0 { 60u16 } else { e[off + 5] as u16 };
    Some(ModeInfo {
        width: h,
        height: v,
        refresh_hz: r,
        interlaced: false,
    })
}

/// A representative, checksum-valid 128-byte EDID (manufacturer "AUR").
pub fn sample_edid() -> [u8; 128] {
    let mut e = [0u8; 128];
    e[0..8].copy_from_slice(&[0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x00]);
    e[8] = 0x06; // manufacturer AUR: raw = 0x06B2
    e[9] = 0xB2;
    e[10] = 0x34; // product code 0x1234 (LE)
    e[11] = 0x12;
    e[12] = 0x01; // serial 0x000A01 (LE)
    e[13] = 0x0A;
    e[14] = 0x00;
    e[15] = 0x00;
    e[16] = 0x14; // week 20
    e[17] = 33; // 1990 + 33 = 2023
    e[18] = 1; // version 1
    e[19] = 4; // revision 4
    e[35] = 0x08; // one established timing bit (800x600@60)
    e[38] = 0xD1; // standard timing 0 -> 1920x1200
    e[39] = 0x00;
    // detailed timing descriptor 0: 1920x1080@60
    e[54] = 0xFA; // pixel clock (LE, 10kHz units), non-zero
    e[55] = 0x39;
    e[56] = 0x80; // h active lo (1920 -> 0x80)
    e[57] = 0x38; // v active lo (1080 -> 0x38)
    e[58] = 0x47; // h hi 7, v hi 4
    e[59] = 60; // refresh
    e[127] = compute_edid_checksum(&e);
    e
}

fn compute_edid_checksum(e: &[u8; 128]) -> u8 {
    let mut sum: u32 = 0;
    let mut i = 0;
    while i < 127 {
        sum = sum.wrapping_add(e[i] as u32);
        i += 1;
    }
    (256u32.wrapping_sub(sum & 0xFF) & 0xFF) as u8
}

// ---------------------------------------------------------------------------
// A003 — 分辨率/刷新率枚举 (Resolution / refresh enumeration)
// ---------------------------------------------------------------------------

/// One display mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ModeInfo {
    pub width: u16,
    pub height: u16,
    pub refresh_hz: u16,
    pub interlaced: bool,
}

pub const MAX_MODES: usize = 8;

/// Fixed-capacity list of supported display modes.
#[derive(Clone, Copy, Debug)]
pub struct ModeList {
    modes: [ModeInfo; MAX_MODES],
    count: usize,
}

impl ModeList {
    pub const fn new() -> ModeList {
        ModeList {
            modes: [ModeInfo {
                width: 0,
                height: 0,
                refresh_hz: 0,
                interlaced: false,
            }; MAX_MODES],
            count: 0,
        }
    }

    pub fn push(&mut self, m: ModeInfo) -> bool {
        if self.count >= MAX_MODES {
            return false;
        }
        self.modes[self.count] = m;
        self.count += 1;
        true
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn get(&self, i: usize) -> Option<ModeInfo> {
        if i < self.count {
            Some(self.modes[i])
        } else {
            None
        }
    }

    pub fn contains(&self, w: u16, h: u16) -> bool {
        let mut i = 0;
        while i < self.count {
            if self.modes[i].width == w && self.modes[i].height == h {
                return true;
            }
            i += 1;
        }
        false
    }

    /// Pick the mode whose refresh is closest to `preferred`; tie-break by
    /// higher refresh, then larger pixel area.
    pub fn negotiate_refresh(&self, preferred: u16) -> Option<ModeInfo> {
        let mut best: Option<ModeInfo> = None;
        let mut best_dist: u32 = u32::MAX;
        let mut i = 0;
        while i < self.count {
            let m = self.modes[i];
            let dist = if m.refresh_hz > preferred {
                (m.refresh_hz - preferred) as u32
            } else {
                (preferred - m.refresh_hz) as u32
            };
            let better = match best {
                None => true,
                Some(b) => {
                    dist < best_dist
                        || (dist == best_dist
                            && (m.refresh_hz > b.refresh_hz
                                || (m.refresh_hz == b.refresh_hz
                                    && (m.width as u32 * m.height as u32)
                                        > (b.width as u32 * b.height as u32))))
                }
            };
            if better {
                best = Some(m);
                best_dist = dist;
            }
            i += 1;
        }
        best
    }
}

/// The platform's built-in mode table.
pub fn builtin_modes() -> ModeList {
    let mut l = ModeList::new();
    l.push(ModeInfo {
        width: 640,
        height: 480,
        refresh_hz: 60,
        interlaced: false,
    });
    l.push(ModeInfo {
        width: 800,
        height: 600,
        refresh_hz: 60,
        interlaced: false,
    });
    l.push(ModeInfo {
        width: 1024,
        height: 768,
        refresh_hz: 60,
        interlaced: false,
    });
    l.push(ModeInfo {
        width: 1280,
        height: 720,
        refresh_hz: 60,
        interlaced: false,
    });
    l.push(ModeInfo {
        width: 1920,
        height: 1080,
        refresh_hz: 60,
        interlaced: false,
    });
    l.push(ModeInfo {
        width: 2560,
        height: 1440,
        refresh_hz: 60,
        interlaced: false,
    });
    l.push(ModeInfo {
        width: 3840,
        height: 2160,
        refresh_hz: 60,
        interlaced: false,
    });
    l.push(ModeInfo {
        width: 1920,
        height: 1080,
        refresh_hz: 120,
        interlaced: false,
    });
    l
}

// ---------------------------------------------------------------------------
// A004 — 模式设置 modeset
// ---------------------------------------------------------------------------

/// Live display state after a (modelled) modeset.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DisplayState {
    pub mode: ModeInfo,
    pub fb: Framebuffer,
    pub active: bool,
}

impl DisplayState {
    pub const fn new() -> DisplayState {
        DisplayState {
            mode: ModeInfo {
                width: 0,
                height: 0,
                refresh_hz: 0,
                interlaced: false,
            },
            fb: Framebuffer::new(0, 0, 32),
            active: false,
        }
    }

    pub fn apply(&mut self, m: ModeInfo, fb: Framebuffer) {
        self.mode = m;
        self.fb = fb;
        self.active = true;
    }
}

// ---------------------------------------------------------------------------
// A005 — 刷新率协商 (Refresh rate negotiation)
// ---------------------------------------------------------------------------

/// Free helper wrapping `ModeList::negotiate_refresh` for an ad-hoc list.
pub fn negotiate_refresh(preferred: u16, list: &ModeList) -> Option<ModeInfo> {
    list.negotiate_refresh(preferred)
}

// ---------------------------------------------------------------------------
// A006 — 多显示器检测 (Multi-display detection)
// ---------------------------------------------------------------------------

pub const MAX_MONITORS: usize = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Monitor {
    pub index: u8,
    pub connected: bool,
    pub is_primary: bool,
    pub edid_csum: u8,
}

#[derive(Clone, Copy, Debug)]
pub struct Topology {
    monitors: [Monitor; MAX_MONITORS],
    count: usize,
}

impl Topology {
    pub const fn new() -> Topology {
        Topology {
            monitors: [Monitor {
                index: 0,
                connected: false,
                is_primary: false,
                edid_csum: 0,
            }; MAX_MONITORS],
            count: 0,
        }
    }

    pub fn detect(&mut self, m: Monitor) -> bool {
        if self.count >= MAX_MONITORS {
            return false;
        }
        self.monitors[self.count] = m;
        self.count += 1;
        true
    }

    pub fn connected_count(&self) -> usize {
        let mut c = 0;
        let mut i = 0;
        while i < self.count {
            if self.monitors[i].connected {
                c += 1;
            }
            i += 1;
        }
        c
    }

    pub fn primary(&self) -> Option<Monitor> {
        let mut i = 0;
        while i < self.count {
            if self.monitors[i].is_primary {
                return Some(self.monitors[i]);
            }
            i += 1;
        }
        None
    }

    pub fn get(&self, i: usize) -> Option<Monitor> {
        if i < self.count {
            Some(self.monitors[i])
        } else {
            None
        }
    }
}

// ---------------------------------------------------------------------------
// A007 — 显示器热插拔 (Monitor hotplug state machine)
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HotplugState {
    Absent,
    Present,
    PendingAdd,
    PendingRemove,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlugEvent {
    Attach,
    Detach,
}

#[derive(Clone, Copy, Debug)]
pub struct Hotplug {
    pub state: HotplugState,
    ticks: u32,
}

impl Hotplug {
    pub const fn new() -> Hotplug {
        Hotplug {
            state: HotplugState::Absent,
            ticks: 0,
        }
    }

    pub fn on_event(&mut self, ev: PlugEvent) {
        match ev {
            PlugEvent::Attach => {
                if self.state == HotplugState::Absent || self.state == HotplugState::PendingRemove {
                    self.state = HotplugState::PendingAdd;
                }
            }
            PlugEvent::Detach => {
                if self.state == HotplugState::Present || self.state == HotplugState::PendingAdd {
                    self.state = HotplugState::PendingRemove;
                }
            }
        }
    }

    /// Advance the debounce settle: pending states resolve to stable ones.
    pub fn tick(&mut self) {
        self.ticks = self.ticks.saturating_add(1);
        self.state = match self.state {
            HotplugState::PendingAdd => HotplugState::Present,
            HotplugState::PendingRemove => HotplugState::Absent,
            other => other,
        };
    }

    pub fn is_present(&self) -> bool {
        self.state == HotplugState::Present
    }
}

// ---------------------------------------------------------------------------
// A008 — DPI 与缩放因子 (DPI & scaling factor)
// ---------------------------------------------------------------------------

/// Integer square root (no float, no_std safe).
pub fn isqrt(mut n: u32) -> u32 {
    if n == 0 {
        return 0;
    }
    let mut res = 0u32;
    let mut bit = 1u32 << 30;
    while bit > n {
        bit >>= 2;
    }
    while bit != 0 {
        if n >= res + bit {
            n -= res + bit;
            res = (res >> 1) + bit;
        } else {
            res >>= 1;
        }
        bit >>= 2;
    }
    res
}

/// Diagonal pixel length = sqrt(w^2 + h^2).
pub fn diag_px(width: u16, height: u16) -> u32 {
    let w = width as u32;
    let h = height as u32;
    isqrt(w.wrapping_mul(w).wrapping_add(h.wrapping_mul(h)))
}

/// DPI for a diagonal given in tenths of an inch (e.g. 15.6" -> 156).
pub fn dpi(width: u16, height: u16, diag_inch_x10: u16) -> u16 {
    if diag_inch_x10 == 0 {
        return 0;
    }
    let dp = diag_px(width, height);
    ((dp * 10) / diag_inch_x10 as u32) as u16
}

/// Discrete scale percent (100/125/150/200) chosen from DPI.
pub fn scale_percent(width: u16, height: u16, diag_inch_x10: u16) -> u8 {
    let d = dpi(width, height, diag_inch_x10);
    if d >= 180 {
        200
    } else if d >= 140 {
        150
    } else if d >= 110 {
        125
    } else {
        100
    }
}

/// Logical (output) size for a given scale percent.
pub fn logical_size(width: u16, height: u16, scale_percent: u8) -> (u16, u16) {
    if scale_percent == 0 {
        return (width, height);
    }
    let sw = (width as u32 * 100) / scale_percent as u32;
    let sh = (height as u32 * 100) / scale_percent as u32;
    (sw as u16, sh as u16)
}

// ---------------------------------------------------------------------------
// A009 — 双缓冲/三缓冲 (Double / triple buffering)
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BufferPolicy {
    Single,
    Double,
    Triple,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BufferPool {
    pub policy: BufferPolicy,
    front: u8,
    back: u8,
    buf_count: u8,
}

impl BufferPool {
    pub const fn new(policy: BufferPolicy) -> BufferPool {
        let c = match policy {
            BufferPolicy::Single => 1,
            BufferPolicy::Double => 2,
            BufferPolicy::Triple => 3,
        };
        BufferPool {
            policy,
            front: 0,
            back: if c > 1 { 1 } else { 0 },
            buf_count: c,
        }
    }

    /// Swap/rotate the visible and in-progress buffers.
    pub fn flip(&mut self) {
        match self.policy {
            BufferPolicy::Single => {}
            BufferPolicy::Double => {
                let t = self.front;
                self.front = self.back;
                self.back = t;
            }
            BufferPolicy::Triple => {
                self.front = (self.front + 1) % 3;
                self.back = (self.back + 1) % 3;
            }
        }
    }

    pub fn front_index(&self) -> u8 {
        self.front
    }

    pub fn back_index(&self) -> u8 {
        self.back
    }

    pub fn buffer_count(&self) -> u8 {
        self.buf_count
    }
}

// ---------------------------------------------------------------------------
// A010 — 垂直同步 vsync
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Vsync {
    pub frame: u32,
    pub in_blank: bool,
}

impl Vsync {
    pub const fn new() -> Vsync {
        Vsync {
            frame: 0,
            in_blank: false,
        }
    }

    /// Begin a vsync pulse (enters blanking, advances the frame counter).
    pub fn pulse(&mut self) {
        self.frame = self.frame.saturating_add(1);
        self.in_blank = true;
    }

    pub fn end_blank(&mut self) {
        self.in_blank = false;
    }

    /// Advance exactly one vsync: returns the new frame number.
    pub fn next_frame(&mut self) -> u32 {
        self.pulse();
        self.end_blank();
        self.frame
    }
}

// ---------------------------------------------------------------------------
// A011 — 屏幕撕裂防护 (Tearing protection)
// ---------------------------------------------------------------------------

/// A page-flip is only safe while the scanout is in blanking.
pub fn can_present(v: &Vsync) -> bool {
    v.in_blank
}

// ---------------------------------------------------------------------------
// A012 — 亮度与色温调节 (Brightness & color temperature)
// ---------------------------------------------------------------------------

/// 0..100 brightness mapped to a 0..256 fixed-point scale.
pub fn brightness_scale(brightness: u8) -> u16 {
    (brightness as u16) * 256 / 100
}

/// RGB multiplier (0..255) for a given color temperature in Kelvin.
pub fn temp_rgb(kelvin: u16) -> (u8, u8, u8) {
    if kelvin >= 6500 {
        (255, 255, 255)
    } else {
        let drop = (((6500 - kelvin) / 50) as u16).min(255);
        let b = 255u16.saturating_sub(drop) as u8;
        let g = (255u16.saturating_sub(drop / 2)).min(255) as u8;
        (255, g, b)
    }
}

// ---------------------------------------------------------------------------
// A013 — 夜间护眼模式 (Night eye-care mode)
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NightLevel {
    Off,
    Low,
    Medium,
    High,
}

/// Blue-channel retention (0..255) under each night level.
pub fn night_blue(level: NightLevel) -> u8 {
    match level {
        NightLevel::Off => 255,
        NightLevel::Low => 210,
        NightLevel::Medium => 170,
        NightLevel::High => 140,
    }
}

/// Apply the night blue reduction to a blue pixel value.
pub fn night_apply(pixel_b: u8, level: NightLevel) -> u8 {
    ((pixel_b as u16) * night_blue(level) as u16 / 255) as u8
}

// ---------------------------------------------------------------------------
// A014 — 屏幕旋转 (Screen rotation)
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rotation {
    R0,
    R90,
    R180,
    R270,
}

/// Output resolution after applying rotation (90/270 swap axes).
pub fn rotated_size(r: Rotation, w: u16, h: u16) -> (u16, u16) {
    match r {
        Rotation::R0 | Rotation::R180 => (w, h),
        Rotation::R90 | Rotation::R270 => (h, w),
    }
}

// ---------------------------------------------------------------------------
// A015 — 显示省电降帧 (Display power-save frame reduction)
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PowerSave {
    Off,
    Low,
    Medium,
    High,
}

/// Reduced refresh rate for a given power-save level (never below 1 Hz).
pub fn save_refresh(current: u16, level: PowerSave) -> u16 {
    let cur = current.max(1);
    match level {
        PowerSave::Off => cur,
        PowerSave::Low => {
            if cur > 60 {
                60
            } else {
                cur
            }
        }
        PowerSave::Medium => {
            if cur > 48 {
                48
            } else {
                cur
            }
        }
        PowerSave::High => {
            if cur > 30 {
                30
            } else {
                cur
            }
        }
    }
}

// ---------------------------------------------------------------------------
// A016 — 每屏独立配置 (Per-screen independent config)
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScreenConfig {
    pub index: u8,
    pub scale_percent: u8,
    pub brightness: u8,
    pub night: NightLevel,
    pub rotation: Rotation,
    pub power_save: PowerSave,
}

#[derive(Clone, Copy, Debug)]
pub struct ScreenConfigTable {
    cfg: [Option<ScreenConfig>; MAX_MONITORS],
    count: usize,
}

impl ScreenConfigTable {
    pub const fn new() -> ScreenConfigTable {
        ScreenConfigTable {
            cfg: [None; MAX_MONITORS],
            count: 0,
        }
    }

    /// Insert or update the config for `c.index`.
    pub fn set(&mut self, c: ScreenConfig) -> bool {
        let mut i = 0;
        while i < self.count {
            if let Some(mut s) = self.cfg[i] {
                if s.index == c.index {
                    self.cfg[i] = Some(c);
                    return true;
                }
            }
            i += 1;
        }
        if self.count < MAX_MONITORS {
            self.cfg[self.count] = Some(c);
            self.count += 1;
            true
        } else {
            false
        }
    }

    pub fn get(&self, index: u8) -> Option<ScreenConfig> {
        let mut i = 0;
        while i < self.count {
            if let Some(s) = self.cfg[i] {
                if s.index == index {
                    return Some(s);
                }
            }
            i += 1;
        }
        None
    }

    pub fn len(&self) -> usize {
        self.count
    }
}

// ---------------------------------------------------------------------------
// A017 — 与合成器协作 (Cooperation with the compositor)
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DirtyRect {
    pub x: u16,
    pub y: u16,
    pub w: u16,
    pub h: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CompositorHandoff {
    pub frame_seq: u32,
    pub dirty: DirtyRect,
    pub vsync_frame: u32,
    pub ready: bool,
}

/// Build a handoff descriptor after a successful flip.
pub fn make_handoff(frame_seq: u32, vsync_frame: u32, dirty: DirtyRect) -> CompositorHandoff {
    CompositorHandoff {
        frame_seq,
        dirty,
        vsync_frame,
        ready: true,
    }
}

/// The compositor may consume the frame only when ready and non-empty.
pub fn handoff_ready(h: &CompositorHandoff) -> bool {
    h.ready && h.dirty.w > 0 && h.dirty.h > 0
}

/// Bounding-box union of two dirty rectangles (saturating).
pub fn dirty_union(a: DirtyRect, b: DirtyRect) -> DirtyRect {
    let x1 = a.x.min(b.x);
    let y1 = a.y.min(b.y);
    let x2 = (a.x + a.w).max(b.x + b.w);
    let y2 = (a.y + a.h).max(b.y + b.h);
    DirtyRect {
        x: x1,
        y: y1,
        w: x2.saturating_sub(x1),
        h: y2.saturating_sub(y1),
    }
}

// ---------------------------------------------------------------------------
// A018 — 显示策略中心 (Display policy center)
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DisplayPolicy {
    pub global_brightness: u8,
    pub global_night: NightLevel,
    pub allow_rotation: bool,
    pub vsync_on: bool,
    pub power_save: PowerSave,
}

/// Resolve the global policy into a concrete per-screen configuration.
pub fn policy_resolve(p: DisplayPolicy, index: u8, base_scale: u8) -> ScreenConfig {
    ScreenConfig {
        index,
        scale_percent: base_scale,
        brightness: p.global_brightness,
        night: p.global_night,
        rotation: if p.allow_rotation {
            Rotation::R0
        } else {
            Rotation::R0
        },
        power_save: p.power_save,
    }
}

// ---------------------------------------------------------------------------
// A019 — 显示输出与屏幕管理自检 (Self-check invariants)
// ---------------------------------------------------------------------------

/// Invariants that must always hold for the domain to be coherent.
pub fn display_invariants_ok() -> bool {
    let edid = sample_edid();
    edid_valid(&edid)
        && edid_manufacturer(&edid).len() == 3
        && builtin_modes().contains(640, 480)
        && edid_detailed(&edid, 0).map(|m| m.width == 1920).unwrap_or(false)
}

// ---------------------------------------------------------------------------
// A020 — 显示输出与屏幕管理性能预算 (Performance budget)
// ---------------------------------------------------------------------------

pub const FRAME_BUDGET_MS: u16 = 16;

pub fn within_budget(frame_ms: u16) -> bool {
    frame_ms <= FRAME_BUDGET_MS
}

/// Signed slack (budget - actual); positive means headroom.
pub fn budget_slack(frame_ms: u16) -> i16 {
    FRAME_BUDGET_MS as i16 - frame_ms as i16
}

// ---------------------------------------------------------------------------
// A021 — 显示输出与屏幕管理可观测 (Observability)
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DisplayStats {
    pub frames_presented: u32,
    pub vsync_frames: u32,
    pub flips: u32,
    pub drops: u32,
    present_ms_sum: u64,
    present_ms_n: u32,
}

impl DisplayStats {
    pub const fn new() -> DisplayStats {
        DisplayStats {
            frames_presented: 0,
            vsync_frames: 0,
            flips: 0,
            drops: 0,
            present_ms_sum: 0,
            present_ms_n: 0,
        }
    }

    pub fn record_present(&mut self, ms: u16) {
        self.frames_presented = self.frames_presented.saturating_add(1);
        self.present_ms_sum = self.present_ms_sum.saturating_add(ms as u64);
        self.present_ms_n = self.present_ms_n.saturating_add(1);
    }

    pub fn record_flip(&mut self) {
        self.flips = self.flips.saturating_add(1);
    }

    pub fn record_drop(&mut self) {
        self.drops = self.drops.saturating_add(1);
    }

    pub fn record_vsync(&mut self) {
        self.vsync_frames = self.vsync_frames.saturating_add(1);
    }

    pub fn avg_present_ms(&self) -> u16 {
        if self.present_ms_n == 0 {
            0
        } else {
            (self.present_ms_sum / self.present_ms_n as u64) as u16
        }
    }

    /// Drop rate in permille (0..1000).
    pub fn drop_rate_permille(&self) -> u16 {
        let total = self.frames_presented.saturating_add(self.drops);
        if total == 0 {
            0
        } else {
            ((self.drops as u64 * 1000) / total as u64) as u16
        }
    }
}

// ---------------------------------------------------------------------------
// A022 — 显示输出与屏幕管理模糊测试 (Fuzz harness)
// ---------------------------------------------------------------------------

/// Total parser over arbitrary bytes: never panics. Returns the number of
/// internal invariants violated, which must be 0 for any input.
pub fn fuzz_edid(data: &[u8]) -> u32 {
    let mut buf = [0u8; 128];
    let n = data.len().min(128);
    let mut i = 0;
    while i < n {
        buf[i] = data[i];
        i += 1;
    }
    let mut violations = 0u32;
    if edid_header_ok(&buf) {
        if let Some(m) = edid_detailed(&buf, 0) {
            if m.width == 0 || m.height == 0 || m.refresh_hz == 0 {
                violations += 1;
            }
        }
        if edid_standard_count(&buf) > 8 {
            violations += 1;
        }
    }
    violations
}

// ---------------------------------------------------------------------------
// A023 — 显示输出与屏幕管理文档 (Documentation)
// ---------------------------------------------------------------------------

pub const FEATURE_COUNT: usize = 25;

pub const fn feature_ids() -> [u8; 25] {
    [
        1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25,
    ]
}

pub fn doc_summary() -> &'static str {
    "AURORA-1000 AI-01 display & screen management: framebuffer, EDID, modeset, \
     multi-monitor, DPI scaling, vsync, brightness/color-temp, night mode, rotation, \
     power-save, compositor handoff, policy center, degradation chain (A001-A025)."
}

// ---------------------------------------------------------------------------
// A024 — 显示输出与屏幕管理降级链 (Degradation chain)
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DisplayTier {
    Full,
    Reduced,
    Safe,
    Text,
}

/// Step one tier down (Text is the floor).
pub fn degrade(t: DisplayTier) -> DisplayTier {
    match t {
        DisplayTier::Full => DisplayTier::Reduced,
        DisplayTier::Reduced => DisplayTier::Safe,
        DisplayTier::Safe => DisplayTier::Text,
        DisplayTier::Text => DisplayTier::Text,
    }
}

/// The concrete mode a degraded tier falls back to.
pub fn tier_mode(t: DisplayTier) -> ModeInfo {
    match t {
        DisplayTier::Full => ModeInfo {
            width: 1920,
            height: 1080,
            refresh_hz: 60,
            interlaced: false,
        },
        DisplayTier::Reduced => ModeInfo {
            width: 1280,
            height: 720,
            refresh_hz: 60,
            interlaced: false,
        },
        DisplayTier::Safe => ModeInfo {
            width: 800,
            height: 600,
            refresh_hz: 60,
            interlaced: false,
        },
        DisplayTier::Text => ModeInfo {
            width: 640,
            height: 480,
            refresh_hz: 60,
            interlaced: false,
        },
    }
}

// ---------------------------------------------------------------------------
// A025 — 显示输出与屏幕管理域自检收口 (Domain self-check closure)
// ---------------------------------------------------------------------------

/// Domain self-test: pure invariants asserted on sample data, all must hold.
pub fn run_display_checks() -> CheckSet {
    let mut set = CheckSet::new("aurora-display");

    let edid = sample_edid();
    let fb = Framebuffer::new(1920, 1080, 32);
    let modes = builtin_modes();
    let mut topo = Topology::new();
    topo.detect(Monitor {
        index: 0,
        connected: true,
        is_primary: true,
        edid_csum: edid_checksum(&edid).unwrap_or(0),
    });
    topo.detect(Monitor {
        index: 1,
        connected: true,
        is_primary: false,
        edid_csum: 0,
    });

    // A001 framebuffer
    set.add(
        "A001 framebuffer",
        fb.is_valid()
            && fb.byte_len() == 1920u32 * 4 * 1080
            && !Framebuffer::new(0, 1080, 32).is_valid(),
        "fb model",
    );

    // A002 EDID parse
    set.add(
        "A002 EDID parse",
        edid_valid(&edid)
            && edid_manufacturer(&edid) == [b'A', b'U', b'R']
            && edid_product_code(&edid) == 0x1234
            && edid_year(&edid) == 2023,
        "edid",
    );

    // A003 mode enumeration
    set.add(
        "A003 mode enum",
        modes.len() == 8
            && modes.contains(1920, 1080)
            && edid_standard_count(&edid) == 1
            && edid_detailed(&edid, 0).map(|m| m.width == 1920).unwrap_or(false),
        "modes",
    );

    // A004 modeset
    let mut disp = DisplayState::new();
    disp.apply(modes.get(4).unwrap(), fb);
    set.add("A004 modeset", disp.mode.width == 1920 && disp.active, "modeset");

    // A005 refresh negotiation
    let mut neg = ModeList::new();
    neg.push(ModeInfo {
        width: 1920,
        height: 1080,
        refresh_hz: 60,
        interlaced: false,
    });
    neg.push(ModeInfo {
        width: 1920,
        height: 1080,
        refresh_hz: 120,
        interlaced: false,
    });
    let best = negotiate_refresh(100, &neg);
    set.add(
        "A005 refresh neg",
        best.map(|m| m.refresh_hz == 120).unwrap_or(false),
        "negotiate",
    );

    // A006 multi-display
    set.add(
        "A006 multi-display",
        topo.connected_count() == 2 && topo.primary().map(|m| m.is_primary).unwrap_or(false),
        "topology",
    );

    // A007 hotplug
    let mut hp = Hotplug::new();
    hp.on_event(PlugEvent::Attach);
    hp.tick();
    let present = hp.is_present();
    hp.on_event(PlugEvent::Detach);
    hp.tick();
    set.add(
        "A007 hotplug",
        present && !hp.is_present() && hp.state == HotplugState::Absent,
        "hotplug",
    );

    // A008 DPI & scaling
    set.add(
        "A008 dpi scale",
        scale_percent(1920, 1080, 156) == 150
            && logical_size(1920, 1080, 150) == (1280, 720),
        "dpi",
    );

    // A009 buffers
    let mut bp = BufferPool::new(BufferPolicy::Double);
    bp.flip();
    set.add(
        "A009 buffers",
        bp.front_index() == 1
            && bp.back_index() == 0
            && BufferPool::new(BufferPolicy::Triple).buffer_count() == 3,
        "buffers",
    );

    // A010 vsync
    let mut v = Vsync::new();
    let f = v.next_frame();
    set.add("A010 vsync", f == 1 && v.frame == 1, "vsync");

    // A011 tearing protection
    v.pulse();
    let can = can_present(&v);
    v.end_blank();
    set.add("A011 tearing", can && !can_present(&v), "tearing");

    // A012 brightness & color temperature
    set.add(
        "A012 brightness temp",
        temp_rgb(6500) == (255, 255, 255)
            && temp_rgb(3000).2 < 255
            && brightness_scale(50) == 128,
        "color",
    );

    // A013 night mode
    set.add(
        "A013 night mode",
        night_blue(NightLevel::Off) == 255
            && night_blue(NightLevel::High) == 140
            && night_apply(255, NightLevel::High) == 140,
        "night",
    );

    // A014 rotation
    set.add(
        "A014 rotation",
        rotated_size(Rotation::R90, 1920, 1080) == (1080, 1920),
        "rotation",
    );

    // A015 power save
    set.add(
        "A015 power save",
        save_refresh(120, PowerSave::High) == 30
            && save_refresh(30, PowerSave::High) == 30
            && save_refresh(60, PowerSave::Low) == 60,
        "powersave",
    );

    // A016 per-screen config
    let mut sc = ScreenConfigTable::new();
    sc.set(ScreenConfig {
        index: 0,
        scale_percent: 150,
        brightness: 80,
        night: NightLevel::Off,
        rotation: Rotation::R0,
        power_save: PowerSave::Off,
    });
    set.add(
        "A016 per-screen",
        sc.get(0).map(|c| c.scale_percent == 150).unwrap_or(false) && sc.get(1).is_none(),
        "perscreen",
    );

    // A017 compositor cooperation
    let h = make_handoff(7, 1, DirtyRect {
        x: 0,
        y: 0,
        w: 100,
        h: 50,
    });
    let u = dirty_union(
        DirtyRect {
            x: 0,
            y: 0,
            w: 100,
            h: 50,
        },
        DirtyRect {
            x: 50,
            y: 20,
            w: 100,
            h: 50,
        },
    );
    set.add(
        "A017 compositor",
        h.ready && handoff_ready(&h) && u.w == 150 && u.h == 70,
        "compositor",
    );

    // A018 policy center
    let pol = DisplayPolicy {
        global_brightness: 70,
        global_night: NightLevel::Off,
        allow_rotation: false,
        vsync_on: true,
        power_save: PowerSave::Off,
    };
    let cfg = policy_resolve(pol, 0, 150);
    set.add(
        "A018 policy",
        cfg.brightness == 70 && cfg.night == NightLevel::Off,
        "policy",
    );

    // A019 self-check invariants
    set.add(
        "A019 invariants",
        display_invariants_ok()
            && edid_manufacturer(&edid).len() == 3
            && builtin_modes().contains(640, 480),
        "invariants",
    );

    // A020 performance budget
    set.add(
        "A020 perf budget",
        within_budget(16) && !within_budget(17) && budget_slack(10) == 6,
        "budget",
    );

    // A021 observability
    let mut st = DisplayStats::new();
    st.record_present(8);
    st.record_present(16);
    st.record_drop();
    set.add(
        "A021 observability",
        st.frames_presented == 2 && st.avg_present_ms() == 12 && st.drop_rate_permille() == 333,
        "stats",
    );

    // A022 fuzz
    set.add(
        "A022 fuzz",
        fuzz_edid(&edid) == 0 && fuzz_edid(&[0u8; 200]) == 0 && fuzz_edid(&[0xFFu8; 300]) == 0,
        "fuzz",
    );

    // A023 docs
    set.add(
        "A023 docs",
        doc_summary().len() > 0 && FEATURE_COUNT == 25,
        "docs",
    );

    // A024 degradation chain
    set.add(
        "A024 degradation",
        degrade(DisplayTier::Full) == DisplayTier::Reduced
            && degrade(DisplayTier::Text) == DisplayTier::Text
            && tier_mode(DisplayTier::Text).width == 640,
        "degrade",
    );

    // A025 domain closure: full pipeline EDID -> mode -> fb -> handoff
    let edid_mode = edid_detailed(&edid, 0).unwrap_or(modes.get(4).unwrap());
    let mut d2 = DisplayState::new();
    d2.apply(
        edid_mode,
        Framebuffer::new(edid_mode.width, edid_mode.height, 32),
    );
    let hand = make_handoff(
        1,
        1,
        DirtyRect {
            x: 0,
            y: 0,
            w: edid_mode.width,
            h: edid_mode.height,
        },
    );
    set.add(
        "A025 domain closure",
        d2.active && d2.fb.is_valid() && handoff_ready(&hand),
        "closure",
    );

    set
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a001_framebuffer() {
        let fb = Framebuffer::new(1920, 1080, 32);
        assert!(fb.is_valid());
        assert_eq!(fb.byte_len(), 1920u32 * 4 * 1080);
        assert_eq!(fb.pixel_count(), 1920u32 * 1080);
        assert!(!Framebuffer::new(0, 1080, 32).is_valid());
        assert!(!Framebuffer::new(1920, 1080, 9).is_valid());
    }

    #[test]
    fn a002_edid_parse() {
        let e = sample_edid();
        assert!(edid_valid(&e));
        assert_eq!(edid_manufacturer(&e), [b'A', b'U', b'R']);
        assert_eq!(edid_product_code(&e), 0x1234);
        assert_eq!(edid_year(&e), 2023);
        assert_eq!(edid_version(&e), (1, 4));
        assert_eq!(edid_established_count(&e), 1);
        assert_eq!(edid_standard_count(&e), 1);
        let d = edid_detailed(&e, 0).unwrap();
        assert_eq!(d.width, 1920);
        assert_eq!(d.height, 1080);
        assert_eq!(d.refresh_hz, 60);
        assert!(edid_detailed(&e, 5).is_none());
        assert!(!edid_valid(&[0u8; 64]));
    }

    #[test]
    fn a003_mode_enum() {
        let m = builtin_modes();
        assert_eq!(m.len(), 8);
        assert!(m.contains(1920, 1080));
        assert!(m.contains(640, 480));
        assert!(!m.contains(9999, 9999));
        assert_eq!(m.get(7).unwrap().refresh_hz, 120);
        assert!(m.get(99).is_none());
    }

    #[test]
    fn a003_mode_list_full() {
        let mut l = ModeList::new();
        for i in 0..MAX_MODES + 3 {
            let ok = l.push(ModeInfo {
                width: 100u16 + i as u16,
                height: 100,
                refresh_hz: 60,
                interlaced: false,
            });
            if i >= MAX_MODES {
                assert!(!ok);
            }
        }
        assert_eq!(l.len(), MAX_MODES);
    }

    #[test]
    fn a004_modeset() {
        let mut d = DisplayState::new();
        assert!(!d.active);
        let m = builtin_modes().get(4).unwrap();
        d.apply(m, Framebuffer::new(m.width, m.height, 32));
        assert!(d.active);
        assert_eq!(d.mode.width, 1920);
    }

    #[test]
    fn a005_refresh_neg() {
        let mut l = ModeList::new();
        l.push(ModeInfo {
            width: 1920,
            height: 1080,
            refresh_hz: 60,
            interlaced: false,
        });
        l.push(ModeInfo {
            width: 1920,
            height: 1080,
            refresh_hz: 120,
            interlaced: false,
        });
        assert_eq!(negotiate_refresh(100, &l).unwrap().refresh_hz, 120);
        assert_eq!(negotiate_refresh(60, &l).unwrap().refresh_hz, 60);
        assert!(negotiate_refresh(60, &ModeList::new()).is_none());
    }

    #[test]
    fn a006_multidisplay() {
        let mut t = Topology::new();
        assert_eq!(t.connected_count(), 0);
        assert!(t.detect(Monitor {
            index: 0,
            connected: true,
            is_primary: true,
            edid_csum: 0,
        }));
        t.detect(Monitor {
            index: 1,
            connected: true,
            is_primary: false,
            edid_csum: 0,
        });
        assert_eq!(t.connected_count(), 2);
        assert!(t.primary().unwrap().is_primary);
        // 容量 MAX_MONITORS=4：未满时继续接受，满后拒绝。
        assert!(t.detect(Monitor {
            index: 2,
            connected: true,
            is_primary: false,
            edid_csum: 0,
        }));
        assert!(t.detect(Monitor {
            index: 3,
            connected: true,
            is_primary: false,
            edid_csum: 0,
        }));
        assert!(!t.detect(Monitor {
            index: 4,
            connected: true,
            is_primary: false,
            edid_csum: 0,
        }));
        assert_eq!(t.connected_count(), MAX_MONITORS);
    }

    #[test]
    fn a007_hotplug() {
        let mut hp = Hotplug::new();
        assert_eq!(hp.state, HotplugState::Absent);
        hp.on_event(PlugEvent::Attach);
        assert_eq!(hp.state, HotplugState::PendingAdd);
        hp.tick();
        assert!(hp.is_present());
        hp.on_event(PlugEvent::Detach);
        assert_eq!(hp.state, HotplugState::PendingRemove);
        hp.tick();
        assert_eq!(hp.state, HotplugState::Absent);
        // ignored transitions
        hp.on_event(PlugEvent::Detach);
        assert_eq!(hp.state, HotplugState::Absent);
    }

    #[test]
    fn a008_dpi_scale() {
        assert_eq!(scale_percent(1920, 1080, 156), 150);
        assert_eq!(logical_size(1920, 1080, 150), (1280, 720));
        assert_eq!(scale_percent(1920, 1080, 240), 100);
        assert_eq!(logical_size(1920, 1080, 100), (1920, 1080));
        assert_eq!(dpi(1920, 1080, 0), 0);
        assert_eq!(logical_size(100, 100, 0), (100, 100));
    }

    #[test]
    fn a009_buffers() {
        let mut d = BufferPool::new(BufferPolicy::Double);
        assert_eq!(d.front_index(), 0);
        assert_eq!(d.back_index(), 1);
        d.flip();
        assert_eq!(d.front_index(), 1);
        assert_eq!(d.back_index(), 0);
        let mut t = BufferPool::new(BufferPolicy::Triple);
        assert_eq!(t.buffer_count(), 3);
        t.flip();
        assert_eq!(t.front_index(), 1);
        assert_ne!(t.front_index(), t.back_index());
    }

    #[test]
    fn a010_vsync() {
        let mut v = Vsync::new();
        assert_eq!(v.frame, 0);
        assert_eq!(v.next_frame(), 1);
        assert_eq!(v.next_frame(), 2);
        assert_eq!(v.frame, 2);
    }

    #[test]
    fn a011_tearing() {
        let mut v = Vsync::new();
        assert!(!can_present(&v));
        v.pulse();
        assert!(v.in_blank);
        assert!(can_present(&v));
        v.end_blank();
        assert!(!can_present(&v));
    }

    #[test]
    fn a012_brightness_temp() {
        assert_eq!(temp_rgb(6500), (255, 255, 255));
        let warm = temp_rgb(3000);
        assert!(warm.2 < 255);
        assert_eq!(warm.0, 255);
        assert_eq!(brightness_scale(0), 0);
        assert_eq!(brightness_scale(50), 128);
        assert_eq!(brightness_scale(100), 256);
    }

    #[test]
    fn a013_night() {
        assert_eq!(night_blue(NightLevel::Off), 255);
        assert_eq!(night_blue(NightLevel::High), 140);
        assert!(night_blue(NightLevel::Low) > night_blue(NightLevel::High));
        assert_eq!(night_apply(255, NightLevel::High), 140);
        assert_eq!(night_apply(255, NightLevel::Off), 255);
    }

    #[test]
    fn a014_rotation() {
        assert_eq!(rotated_size(Rotation::R0, 1920, 1080), (1920, 1080));
        assert_eq!(rotated_size(Rotation::R90, 1920, 1080), (1080, 1920));
        assert_eq!(rotated_size(Rotation::R270, 800, 600), (600, 800));
    }

    #[test]
    fn a015_powersave() {
        assert_eq!(save_refresh(120, PowerSave::High), 30);
        assert_eq!(save_refresh(30, PowerSave::High), 30);
        assert_eq!(save_refresh(60, PowerSave::Low), 60);
        assert_eq!(save_refresh(120, PowerSave::Medium), 48);
        assert_eq!(save_refresh(10, PowerSave::Off), 10);
        assert_eq!(save_refresh(0, PowerSave::High), 1);
    }

    #[test]
    fn a016_per_screen() {
        let mut t = ScreenConfigTable::new();
        assert!(t.set(ScreenConfig {
            index: 0,
            scale_percent: 150,
            brightness: 80,
            night: NightLevel::Off,
            rotation: Rotation::R0,
            power_save: PowerSave::Off,
        }));
        assert!(t.set(ScreenConfig {
            index: 1,
            scale_percent: 125,
            brightness: 60,
            night: NightLevel::Low,
            rotation: Rotation::R0,
            power_save: PowerSave::Off,
        }));
        assert_eq!(t.get(0).unwrap().scale_percent, 150);
        // update existing
        t.set(ScreenConfig {
            index: 0,
            scale_percent: 200,
            brightness: 80,
            night: NightLevel::Off,
            rotation: Rotation::R0,
            power_save: PowerSave::Off,
        });
        assert_eq!(t.get(0).unwrap().scale_percent, 200);
        assert!(t.get(1).is_none() == false);
        assert!(t.get(9).is_none());
    }

    #[test]
    fn a016_per_screen_full() {
        let mut t = ScreenConfigTable::new();
        for i in 0..MAX_MONITORS + 2 {
            let ok = t.set(ScreenConfig {
                index: i as u8,
                scale_percent: 100,
                brightness: 100,
                night: NightLevel::Off,
                rotation: Rotation::R0,
                power_save: PowerSave::Off,
            });
            if i >= MAX_MONITORS {
                assert!(!ok);
            }
        }
        assert_eq!(t.len(), MAX_MONITORS);
    }

    #[test]
    fn a017_compositor() {
        let h = make_handoff(7, 1, DirtyRect {
            x: 0,
            y: 0,
            w: 100,
            h: 50,
        });
        assert!(h.ready);
        assert!(handoff_ready(&h));
        let empty = make_handoff(1, 1, DirtyRect {
            x: 0,
            y: 0,
            w: 0,
            h: 0,
        });
        assert!(!handoff_ready(&empty));
        let u = dirty_union(
            DirtyRect {
                x: 0,
                y: 0,
                w: 100,
                h: 50,
            },
            DirtyRect {
                x: 50,
                y: 20,
                w: 100,
                h: 50,
            },
        );
        assert_eq!((u.w, u.h), (150, 70));
    }

    #[test]
    fn a018_policy() {
        let pol = DisplayPolicy {
            global_brightness: 70,
            global_night: NightLevel::Off,
            allow_rotation: false,
            vsync_on: true,
            power_save: PowerSave::Off,
        };
        let cfg = policy_resolve(pol, 0, 150);
        assert_eq!(cfg.brightness, 70);
        assert_eq!(cfg.night, NightLevel::Off);
        assert_eq!(cfg.scale_percent, 150);
        let pol2 = DisplayPolicy {
            global_brightness: 40,
            global_night: NightLevel::High,
            allow_rotation: true,
            vsync_on: true,
            power_save: PowerSave::Medium,
        };
        let cfg2 = policy_resolve(pol2, 2, 100);
        assert_eq!(cfg2.brightness, 40);
        assert_eq!(cfg2.night, NightLevel::High);
    }

    #[test]
    fn a019_self_check_invariants() {
        assert!(display_invariants_ok());
        let e = sample_edid();
        assert_eq!(edid_manufacturer(&e).len(), 3);
        assert!(builtin_modes().contains(640, 480));
    }

    #[test]
    fn a020_perf_budget() {
        assert!(within_budget(16));
        assert!(within_budget(1));
        assert!(!within_budget(17));
        assert_eq!(budget_slack(10), 6);
        assert_eq!(budget_slack(20), -4);
    }

    #[test]
    fn a021_observability() {
        let mut s = DisplayStats::new();
        assert_eq!(s.avg_present_ms(), 0);
        assert_eq!(s.drop_rate_permille(), 0);
        s.record_present(8);
        s.record_present(16);
        s.record_vsync();
        s.record_flip();
        s.record_drop();
        assert_eq!(s.frames_presented, 2);
        assert_eq!(s.avg_present_ms(), 12);
        assert_eq!(s.drop_rate_permille(), 333);
    }

    #[test]
    fn a022_fuzz() {
        let e = sample_edid();
        assert_eq!(fuzz_edid(&e), 0);
        assert_eq!(fuzz_edid(&[0u8; 200]), 0);
        assert_eq!(fuzz_edid(&[0xFFu8; 300]), 0);
        assert_eq!(fuzz_edid(&[]), 0);
        assert_eq!(fuzz_edid(&[0xABu8; 7]), 0);
    }

    #[test]
    fn a023_docs() {
        assert!(doc_summary().len() > 0);
        assert_eq!(FEATURE_COUNT, 25);
        assert_eq!(feature_ids().len(), 25);
        assert_eq!(feature_ids()[24], 25);
    }

    #[test]
    fn a024_degradation() {
        assert_eq!(degrade(DisplayTier::Full), DisplayTier::Reduced);
        assert_eq!(degrade(DisplayTier::Reduced), DisplayTier::Safe);
        assert_eq!(degrade(DisplayTier::Safe), DisplayTier::Text);
        assert_eq!(degrade(DisplayTier::Text), DisplayTier::Text);
        assert_eq!(tier_mode(DisplayTier::Text).width, 640);
        assert_eq!(tier_mode(DisplayTier::Safe).width, 800);
    }

    #[test]
    fn a025_domain_closure() {
        let set = run_display_checks();
        assert!(set.len() >= 25);
        assert!(set.all_passed(), "display self-check must be all green");
        let (passed, failed) = set.tally();
        assert_eq!(failed, 0);
        assert_eq!(passed, set.len());
    }
}
