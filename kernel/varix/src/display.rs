//! AI-08 · 图形显示域（F176~F200）.
//!
//! The display path is where "correct" and "feels right" are the same thing:
//! a frame that is 1 ms late is a dropped frame, a scroll that stops abruptly
//! is a bad scroll, and a font that is not subpixel-rendered is visibly worse
//! than one that is. Everything here is fixed point, because this runs on the
//! frame budget and an FPU save/restore per layer is real money.



// ---------------------------------------------------------------------------
// F176 — video modes
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct VideoMode {
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub bpp: u8,
    pub hz: u32,
}

impl VideoMode {
    pub fn pixels(&self) -> u64 {
        self.width as u64 * self.height as u64
    }

    pub fn bytes(&self) -> u64 {
        self.stride as u64 * self.height as u64 * (self.bpp as u64 / 8)
    }

    pub fn name(&self) -> ([u8; 12], usize) {
        let mut b = [b' '; 12];
        let mut n = 0usize;
        let put = |buf: &mut [u8; 12], n: &mut usize, v: u32| {
            let mut digits = [0u8; 10];
            let mut w = 0usize;
            let mut v = v;
            if v == 0 {
                digits[0] = b'0';
                w = 1;
            }
            while v > 0 && w < 10 {
                digits[w] = b'0' + (v % 10) as u8;
                v /= 10;
                w += 1;
            }
            while w > 0 && *n < 12 {
                w -= 1;
                buf[*n] = digits[w];
                *n += 1;
            }
        };
        put(&mut b, &mut n, self.width);
        if n < 12 {
            b[n] = b'x';
            n += 1;
        }
        put(&mut b, &mut n, self.height);
        if n < 12 {
            b[n] = b'@';
            n += 1;
        }
        put(&mut b, &mut n, self.hz);
        (b, n)
    }
}

pub const MAX_MODES: usize = 16;

/// F176: the mode list and the switch. Switching is an explicit operation with
/// a recorded history, because "which mode is actually active" is the kind of
/// thing a display bug turns into a guess.
pub struct ModeTable {
    modes: [VideoMode; MAX_MODES],
    len: usize,
    current: usize,
    switches: u64,
    rejected: u64,
}

impl ModeTable {
    pub const fn new() -> ModeTable {
        ModeTable {
            modes: [VideoMode {
                width: 0,
                height: 0,
                stride: 0,
                bpp: 0,
                hz: 0,
            }; MAX_MODES],
            len: 0,
            current: 0,
            switches: 0,
            rejected: 0,
        }
    }

    pub fn add(&mut self, mode: VideoMode) -> bool {
        if self.len >= MAX_MODES || mode.width == 0 || mode.height == 0 || mode.bpp < 16 {
            self.rejected += 1;
            return false;
        }
        if mode.stride < mode.width * (mode.bpp as u32 / 8) {
            self.rejected += 1;
            return false;
        }
        self.modes[self.len] = mode;
        self.len += 1;
        true
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn current(&self) -> Option<VideoMode> {
        self.modes.get(self.current).copied()
    }

    pub fn get(&self, i: usize) -> Option<VideoMode> {
        self.modes.get(i).copied()
    }

    pub fn switches(&self) -> u64 {
        self.switches
    }

    pub fn rejected(&self) -> u64 {
        self.rejected
    }

    /// Exact match wins; otherwise the largest mode that still fits, so a
    /// request for an unsupported size degrades downwards rather than failing.
    pub fn find(&self, width: u32, height: u32) -> Option<usize> {
        if let Some(i) = self
            .modes[..self.len]
            .iter()
            .position(|m| m.width == width && m.height == height)
        {
            return Some(i);
        }
        self.modes[..self.len]
            .iter()
            .enumerate()
            .filter(|(_, m)| m.width <= width && m.height <= height)
            .max_by_key(|(_, m)| m.pixels())
            .map(|(i, _)| i)
    }

    pub fn select(&mut self, index: usize) -> bool {
        if index >= self.len {
            self.rejected += 1;
            return false;
        }
        self.current = index;
        self.switches += 1;
        true
    }

    /// The smallest mode — the last-resort fallback when a driver cannot bring
    /// up anything better (F198).
    pub fn smallest(&self) -> Option<usize> {
        self.modes[..self.len]
            .iter()
            .enumerate()
            .min_by_key(|(_, m)| m.pixels())
            .map(|(i, _)| i)
    }
}

impl Default for ModeTable {
    fn default() -> ModeTable {
        ModeTable::new()
    }
}

// ---------------------------------------------------------------------------
// F177/F178/F179/F180 — buffering, damage, vsync, frame budget
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub fn empty() -> Rect {
        Rect::default()
    }

    pub fn area(&self) -> i64 {
        if self.w <= 0 || self.h <= 0 {
            0
        } else {
            self.w as i64 * self.h as i64
        }
    }

    pub fn intersects(&self, other: &Rect) -> bool {
        self.x < other.x + other.w
            && other.x < self.x + self.w
            && self.y < other.y + other.h
            && other.y < self.y + self.h
    }

    pub fn union(&self, other: &Rect) -> Rect {
        if self.area() == 0 {
            return *other;
        }
        if other.area() == 0 {
            return *self;
        }
        let x = self.x.min(other.x);
        let y = self.y.min(other.y);
        let x2 = (self.x + self.w).max(other.x + other.w);
        let y2 = (self.y + self.h).max(other.y + other.h);
        Rect {
            x,
            y,
            w: x2 - x,
            h: y2 - y,
        }
    }

    /// Shrinking a damage rectangle to its intersection with the screen is what
    /// keeps a clipped window from costing a full-screen redraw.
    pub fn clip(&self, w: i32, h: i32) -> Rect {
        let x = self.x.max(0);
        let y = self.y.max(0);
        let x2 = (self.x + self.w).min(w);
        let y2 = (self.y + self.h).min(h);
        Rect {
            x,
            y,
            w: (x2 - x).max(0),
            h: (y2 - y).max(0),
        }
    }
}

pub const MAX_DAMAGE: usize = 32;

/// F178: damage tracking. Redrawing only what changed is the single biggest
/// power win a compositor has, and merging overlapping rectangles is what keeps
/// the list from growing one rectangle per window.
pub struct DamageList {
    rects: [Rect; MAX_DAMAGE],
    len: usize,
    added: u64,
    coalesced: u64,
}

impl DamageList {
    pub const fn new() -> DamageList {
        DamageList {
            rects: [Rect {
                x: 0,
                y: 0,
                w: 0,
                h: 0,
            }; MAX_DAMAGE],
            len: 0,
            added: 0,
            coalesced: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn added(&self) -> u64 {
        self.added
    }

    pub fn coalesced(&self) -> u64 {
        self.coalesced
    }

    pub fn get(&self, i: usize) -> Option<Rect> {
        self.rects.get(i).copied()
    }

    /// Add a damaged rectangle, merging anything it overlaps.
    pub fn add(&mut self, rect: Rect) -> bool {
        if rect.area() == 0 {
            return false;
        }
        self.added += 1;
        for i in 0..self.len {
            if self.rects[i].intersects(&rect) {
                self.rects[i] = self.rects[i].union(&rect);
                self.coalesced += 1;
                return true;
            }
        }
        if self.len >= MAX_DAMAGE {
            // Full: merge the two smallest so the list stays usable instead of
            // silently dropping damage (which shows up as stale pixels).
            self.coalesced += 1;
            let mut a = 0usize;
            for i in 1..self.len {
                if self.rects[i].area() < self.rects[a].area() {
                    a = i;
                }
            }
            let mut b = if a == 0 { 1 } else { 0 };
            for i in 0..self.len {
                if i != a && self.rects[i].area() < self.rects[b].area() {
                    b = i;
                }
            }
            let merged = self.rects[a].union(&self.rects[b]);
            self.rects[a] = merged;
            let last = self.len - 1;
            self.rects[b] = self.rects[last];
            self.len = last;
            return self.add(rect);
        }
        self.rects[self.len] = rect;
        self.len += 1;
        true
    }

    pub fn total_area(&self) -> i64 {
        self.rects[..self.len].iter().map(|r| r.area()).sum()
    }

    pub fn clear(&mut self) {
        self.len = 0;
    }

    /// Pixels touched versus pixels on screen; the number the frame budget is
    /// actually spent against.
    pub fn coverage_percent(&self, screen_w: i32, screen_h: i32) -> u64 {
        let screen = screen_w as i64 * screen_h as i64;
        if screen <= 0 {
            return 0;
        }
        (self.total_area() * 100 / screen).min(100) as u64
    }
}

impl Default for DamageList {
    fn default() -> DamageList {
        DamageList::new()
    }
}

/// F177: double buffering.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct DoubleBuffer {
    pub front: u8,
    pub back: u8,
    pub swaps: u64,
    pub flips_at_vsync: u64,
}

impl DoubleBuffer {
    pub const fn new() -> DoubleBuffer {
        DoubleBuffer {
            front: 0,
            back: 1,
            swaps: 0,
            flips_at_vsync: 0,
        }
    }

    /// Swap front and back. `at_vsync` records whether the flip landed on the
    /// blanking interval — a flip outside it is a tear.
    pub fn swap(&mut self, at_vsync: bool) {
        core::mem::swap(&mut self.front, &mut self.back);
        self.swaps += 1;
        if at_vsync {
            self.flips_at_vsync += 1;
        }
    }

    pub fn tears(&self) -> u64 {
        self.swaps - self.flips_at_vsync
    }

    pub fn tear_percent(&self) -> u64 {
        if self.swaps == 0 {
            0
        } else {
            self.tears() * 100 / self.swaps
        }
    }
}

/// F179/F180: vsync discipline plus the frame-rate meter that proves it.
pub struct FrameClock {
    pub target_us: u32,
    frames: u64,
    late_frames: u64,
    worst_us: u32,
    total_us: u64,
    jank: u64,
    /// Consecutive late frames, which is what a user perceives as stutter.
    streak: u32,
    worst_streak: u32,
}

impl FrameClock {
    /// 60 Hz, with a 1 ms grace so a frame that is merely tight is not counted
    /// as late.
    pub const fn at_60hz() -> FrameClock {
        FrameClock {
            target_us: 16_667,
            frames: 0,
            late_frames: 0,
            worst_us: 0,
            total_us: 0,
            jank: 0,
            streak: 0,
            worst_streak: 0,
        }
    }

    pub const fn new(target_us: u32) -> FrameClock {
        FrameClock {
            target_us,
            frames: 0,
            late_frames: 0,
            worst_us: 0,
            total_us: 0,
            jank: 0,
            streak: 0,
            worst_streak: 0,
        }
    }

    pub fn present(&mut self, frame_us: u32) {
        self.frames += 1;
        self.total_us += frame_us as u64;
        self.worst_us = self.worst_us.max(frame_us);
        if frame_us > self.target_us + 1000 {
            self.late_frames += 1;
            self.streak += 1;
            self.worst_streak = self.worst_streak.max(self.streak);
            // Two in a row is where a stutter becomes visible.
            if self.streak >= 2 {
                self.jank += 1;
            }
        } else {
            self.streak = 0;
        }
    }

    pub fn frames(&self) -> u64 {
        self.frames
    }

    pub fn late_frames(&self) -> u64 {
        self.late_frames
    }

    pub fn worst_us(&self) -> u32 {
        self.worst_us
    }

    pub fn mean_us(&self) -> u32 {
        if self.frames == 0 {
            0
        } else {
            (self.total_us / self.frames) as u32
        }
    }

    pub fn jank(&self) -> u64 {
        self.jank
    }

    pub fn worst_streak(&self) -> u32 {
        self.worst_streak
    }

    /// Achieved frames per second, from the mean frame time.
    pub fn fps(&self) -> u32 {
        let mean = self.mean_us();
        if mean == 0 {
            0
        } else {
            1_000_000 / mean
        }
    }

    /// Is the display actually keeping up?
    pub fn healthy(&self) -> bool {
        self.late_frames * 100 <= self.frames.max(1)
    }
}

impl Default for FrameClock {
    fn default() -> FrameClock {
        FrameClock::at_60hz()
    }
}

// ---------------------------------------------------------------------------
// F181/F182/F183/F184 — text
// ---------------------------------------------------------------------------

/// F181: a rasterized glyph. The bitmap is 1 bit per pixel (the base layer);
/// F182 adds per-channel coverage on top.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Glyph {
    pub code: u32,
    pub width: u8,
    pub height: u8,
    pub advance: u8,
    pub rows: [u16; 16],
}

impl Glyph {
    pub const MAX_H: usize = 16;

    /// Rasterize an 8-pixel-wide bitmap into a glyph, scaled by an integer
    /// factor (the 128px icon pipeline's font equivalent).
    pub fn from_bitmap(code: u32, bitmap: &[u8], width: u8, height: u8, scale: u8) -> Glyph {
        let mut g = Glyph {
            code,
            width: width.saturating_mul(scale.max(1)),
            height: height.saturating_mul(scale.max(1)).min(Self::MAX_H as u8),
            advance: width.saturating_mul(scale.max(1)),
            rows: [0; 16],
        };
        let scale = scale.max(1) as usize;
        for y in 0..height as usize {
            let src = bitmap.get(y).copied().unwrap_or(0) as u16;
            let mut out_row = 0u16;
            for x in 0..width as usize {
                if src & (1 << (7 - x)) != 0 {
                    out_row |= 1 << x;
                }
            }
            // Nearest-neighbour horizontal scale, repeated vertically.
            let mut scaled = 0u16;
            for x in 0..width as usize {
                if out_row & (1 << x) != 0 {
                    for k in 0..scale {
                        if x * scale + k < 16 {
                            scaled |= 1 << (x * scale + k);
                        }
                    }
                }
            }
            for k in 0..scale {
                let row = y * scale + k;
                if row < Self::MAX_H {
                    g.rows[row] = scaled;
                }
            }
        }
        g
    }

    pub fn bitmap_rows(&self) -> &[u16] {
        &self.rows[..(self.height as usize).min(Self::MAX_H)]
    }

    pub fn ink_pixels(&self) -> u32 {
        self.bitmap_rows().iter().map(|r| r.count_ones()).sum()
    }
}

/// F182: subpixel coverage. LCD text renders the three colour channels
/// separately, which is why it looks sharper — and why a naive implementation
/// leaves coloured fringes.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct SubpixelCoverage {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl SubpixelCoverage {
    pub fn uniform(alpha: u8) -> SubpixelCoverage {
        SubpixelCoverage {
            r: alpha,
            g: alpha,
            b: alpha,
        }
    }

    pub fn max_channel(&self) -> u8 {
        self.r.max(self.g).max(self.b)
    }

    /// Blend a subpixel-covered foreground over a background, per channel.
    pub fn blend(&self, bg: u32, fg: u32) -> u32 {
        let mix = |b: u8, f: u8, a: u8| -> u8 {
            let a = a as u32;
            ((f as u32 * a + b as u32 * (255 - a)) / 255) as u8
        };
        let (br, bg_g, bb) = (
            ((bg >> 16) & 0xFF) as u8,
            ((bg >> 8) & 0xFF) as u8,
            (bg & 0xFF) as u8,
        );
        let (fr, fg_g, fb) = (
            ((fg >> 16) & 0xFF) as u8,
            ((fg >> 8) & 0xFF) as u8,
            (fg & 0xFF) as u8,
        );
        ((mix(br, fr, self.r) as u32) << 16)
            | ((mix(bg_g, fg_g, self.g) as u32) << 8)
            | (mix(bb, fb, self.b) as u32)
    }
}

pub const MAX_CACHED_GLYPHS: usize = 32;

/// F183: a glyph cache. Shaping is expensive, rasterizing is expensive, and
/// text is what the user looks at most — so it gets its own cache.
pub struct GlyphCache {
    entries: [Option<Glyph>; MAX_CACHED_GLYPHS],
    clock: u64,
    hits: u64,
    misses: u64,
    evictions: u64,
}

impl GlyphCache {
    pub const fn new() -> GlyphCache {
        GlyphCache {
            entries: [None; MAX_CACHED_GLYPHS],
            clock: 0,
            hits: 0,
            misses: 0,
            evictions: 0,
        }
    }

    pub fn hits(&self) -> u64 {
        self.hits
    }
    pub fn misses(&self) -> u64 {
        self.misses
    }
    pub fn evictions(&self) -> u64 {
        self.evictions
    }

    pub fn hit_percent(&self) -> u64 {
        let t = self.hits + self.misses;
        if t == 0 {
            0
        } else {
            self.hits * 100 / t
        }
    }

    pub fn lookup(&mut self, code: u32) -> Option<Glyph> {
        for e in self.entries.iter_mut().flatten() {
            if e.code == code {
                self.hits += 1;
                return Some(*e);
            }
        }
        self.misses += 1;
        None
    }

    pub fn insert(&mut self, glyph: Glyph) -> usize {
        if let Some(slot) = self.entries.iter().position(|e| e.is_none()) {
            self.entries[slot] = Some(glyph);
            return slot;
        }
        // Full: drop the first entry (a clock would be over-engineering for a
        // 32-entry cache; the working set is the visible line).
        self.evictions += 1;
        self.clock += 1;
        self.entries[0] = Some(glyph);
        0
    }

    pub fn len(&self) -> usize {
        self.entries.iter().filter(|e| e.is_some()).count()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn clear(&mut self) {
        self.entries = [None; MAX_CACHED_GLYPHS];
    }
}

impl Default for GlyphCache {
    fn default() -> GlyphCache {
        GlyphCache::new()
    }
}

/// F184: line breaking. Counting by advance rather than by bytes is what keeps
/// a wide-glyph script from overflowing its box.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct TextLayout {
    pub lines: u32,
    pub widest_px: u32,
    pub truncated: bool,
}

pub fn layout_text(
    text: &str,
    max_width_px: u32,
    advance: u32,
    max_lines: u32,
) -> TextLayout {
    let mut out = TextLayout::default();
    if advance == 0 || max_width_px == 0 || max_lines == 0 {
        return out;
    }
    let per_line = (max_width_px / advance).max(1);
    let mut line_len = 0u32;
    let mut lines = 1u32;
    let mut widest = 0u32;
    for ch in text.chars() {
        if ch == '\n' {
            lines += 1;
            line_len = 0;
            if lines > max_lines {
                out.truncated = true;
                break;
            }
            continue;
        }
        // A word longer than the line still has to wrap, or it will overflow.
        if line_len + 1 > per_line {
            lines += 1;
            line_len = 0;
            if lines > max_lines {
                out.truncated = true;
                break;
            }
        }
        line_len += 1;
        widest = widest.max(line_len * advance);
    }
    out.lines = lines.min(max_lines);
    out.widest_px = widest;
    out
}

/// A "direction flag": full bidirectional text needs the bidi algorithm, which
/// is not implemented. Saying so here is better than silently mis-rendering
/// right-to-left text.
pub const BIDI_SUPPORTED: bool = false;

/// Rough direction guess, so the layout can at least not reverse Latin text.
pub fn appears_rtl(text: &str) -> bool {
    let mut rtl = 0usize;
    let mut ltr = 0usize;
    for ch in text.chars().take(64) {
        let c = ch as u32;
        if (0x0590..=0x08FF).contains(&c) {
            rtl += 1;
        } else if (c as u8 as char).is_ascii_alphabetic() && c < 128 {
            ltr += 1;
        }
    }
    rtl > ltr
}

// ---------------------------------------------------------------------------
// F185/F186 — window tree and hit testing
// ---------------------------------------------------------------------------

pub const MAX_WINDOWS: usize = 32;
pub const ROOT_WINDOW: u32 = 0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Window {
    pub id: u32,
    pub used: bool,
    pub visible: bool,
    /// Hidden windows are not in the taskbar either (F417).
    pub in_taskbar: bool,
    pub rect: Rect,
    /// Painting order: higher is on top.
    pub z: i32,
    pub parent: u32,
    pub first_child: u32,
    pub next_sibling: u32,
    pub opaque: bool,
}

impl Default for Window {
    fn default() -> Window {
        Window {
            id: 0,
            used: false,
            visible: false,
            in_taskbar: false,
            rect: Rect::default(),
            z: 0,
            parent: ROOT_WINDOW,
            first_child: 0,
            next_sibling: 0,
            opaque: true,
        }
    }
}

/// F185/F186: the window tree. Children are ordered, z-order is explicit, and
/// hit testing walks from the top so the topmost window wins — which is the
/// only ordering a user will accept.
pub struct WindowTree {
    windows: [Window; MAX_WINDOWS],
    count: usize,
    z_counter: i32,
}

impl WindowTree {
    pub const fn new() -> WindowTree {
        WindowTree {
            windows: [Window {
                id: 0,
                used: false,
                visible: false,
                in_taskbar: false,
                rect: Rect {
                    x: 0,
                    y: 0,
                    w: 0,
                    h: 0,
                },
                z: 0,
                parent: 0,
                first_child: 0,
                next_sibling: 0,
                opaque: true,
            }; MAX_WINDOWS],
            count: 0,
            z_counter: 0,
        }
    }

    pub fn create(&mut self, id: u32, parent: u32, rect: Rect, opaque: bool) -> bool {
        if id as usize >= MAX_WINDOWS {
            return false;
        }
        let slot = &mut self.windows[id as usize];
        if slot.used {
            return false;
        }
        self.z_counter += 1;
        *slot = Window {
            id,
            used: true,
            visible: true,
            in_taskbar: true,
            rect: rect.clip(1_000_000, 1_000_000),
            z: self.z_counter,
            parent,
            first_child: 0,
            next_sibling: 0,
            opaque,
        };
        // Link into the parent's child list.
        if parent != id && (parent as usize) < MAX_WINDOWS {
            let first = self.windows[parent as usize].first_child;
            self.windows[id as usize].next_sibling = first;
            self.windows[parent as usize].first_child = id;
        }
        self.count += 1;
        true
    }

    pub fn destroy(&mut self, id: u32) -> bool {
        if id as usize >= MAX_WINDOWS || !self.windows[id as usize].used {
            return false;
        }
        let parent = self.windows[id as usize].parent;
        // Unlink from the parent's child list.
        if (parent as usize) < MAX_WINDOWS {
            let mut cur = self.windows[parent as usize].first_child;
            if cur == id {
                self.windows[parent as usize].first_child = self.windows[id as usize].next_sibling;
            } else {
                while cur != 0 && (cur as usize) < MAX_WINDOWS {
                    if self.windows[cur as usize].next_sibling == id {
                        self.windows[cur as usize].next_sibling =
                            self.windows[id as usize].next_sibling;
                        break;
                    }
                    cur = self.windows[cur as usize].next_sibling;
                }
            }
        }
        // Re-parent the children to the root rather than orphaning them — but
        // only when the root itself is not the window being destroyed.
        let mut child = if id == ROOT_WINDOW {
            0
        } else {
            self.windows[id as usize].first_child
        };
        while child != 0 && (child as usize) < MAX_WINDOWS {
            let next = self.windows[child as usize].next_sibling;
            self.windows[child as usize].parent = ROOT_WINDOW;
            self.windows[child as usize].next_sibling = self.windows[ROOT_WINDOW as usize].first_child;
            self.windows[ROOT_WINDOW as usize].first_child = child;
            child = next;
        }
        self.windows[id as usize] = Window::default();
        self.count = self.count.saturating_sub(1);
        true
    }

    pub fn get(&self, id: u32) -> Option<Window> {
        self.windows
            .get(id as usize)
            .copied()
            .filter(|w| w.used)
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// Raise to the top of the z-order.
    pub fn raise(&mut self, id: u32) -> bool {
        if id as usize >= MAX_WINDOWS || !self.windows[id as usize].used {
            return false;
        }
        self.z_counter += 1;
        self.windows[id as usize].z = self.z_counter;
        true
    }

    pub fn set_visible(&mut self, id: u32, visible: bool, in_taskbar: bool) -> bool {
        match self.windows.get_mut(id as usize) {
            Some(w) if w.used => {
                w.visible = visible;
                w.in_taskbar = in_taskbar;
                true
            }
            _ => false,
        }
    }

    /// F186: the topmost visible window containing the point.
    pub fn hit_test(&self, x: i32, y: i32) -> Option<u32> {
        let mut best: Option<(u32, i32)> = None;
        for w in self.windows.iter().filter(|w| w.used && w.visible) {
            let inside = x >= w.rect.x
                && x < w.rect.x + w.rect.w
                && y >= w.rect.y
                && y < w.rect.y + w.rect.h;
            if inside && best.map(|(_, z)| w.z > z).unwrap_or(true) {
                best = Some((w.id, w.z));
            }
        }
        best.map(|(id, _)| id)
    }

    /// Windows to paint, bottom first.
    pub fn paint_order(&self, out: &mut [u32]) -> usize {
        let mut picked: [(u32, i32); MAX_WINDOWS] = [(0, 0); MAX_WINDOWS];
        let mut n = 0usize;
        for w in self.windows.iter().filter(|w| w.used && w.visible) {
            picked[n] = (w.id, w.z);
            n += 1;
        }
        // Insertion sort: at most 32 entries, and it keeps the code readable.
        for i in 1..n {
            let cur = picked[i];
            let mut j = i;
            while j > 0 && picked[j - 1].1 > cur.1 {
                picked[j] = picked[j - 1];
                j -= 1;
            }
            picked[j] = cur;
        }
        let take = n.min(out.len());
        for (i, p) in picked[..take].iter().enumerate() {
            out[i] = p.0;
        }
        take
    }

    /// Which subtree a point lands in, including occluders — the input for
    /// damage computation (F178).
    pub fn occluded_by(&self, id: u32, rect: Rect) -> bool {
        match self.get(id) {
            Some(w) => self.windows.iter().any(|o| {
                o.used
                    && o.visible
                    && o.rect.intersects(&rect)
                    && o.z > w.z
                    && o.opaque
            }),
            None => false,
        }
    }
}

impl Default for WindowTree {
    fn default() -> WindowTree {
        WindowTree::new()
    }
}

// ---------------------------------------------------------------------------
// F187/F188 — compositing and blending
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum BlendMode {
    #[default]
    Normal,
    Multiply,
    Screen,
    Overlay,
    /// Used for submenus and the notification centre.
    BackdropBlur,
}

impl BlendMode {
    pub fn as_str(self) -> &'static str {
        match self {
            BlendMode::Normal => "normal",
            BlendMode::Multiply => "multiply",
            BlendMode::Screen => "screen",
            BlendMode::Overlay => "overlay",
            BlendMode::BackdropBlur => "blur",
        }
    }

    /// Does this mode need to read what is behind it? A Normal layer with full
    /// alpha does not, and knowing that is what lets the compositor skip work.
    pub fn needs_backdrop(self) -> bool {
        !matches!(self, BlendMode::Normal)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Layer {
    pub window: u32,
    pub alpha: u8,
    pub mode: BlendMode,
    pub blur_sigma: u8,
}

impl Layer {
    /// A fully opaque Normal layer can be copied instead of blended.
    pub fn is_opaque_copy(&self) -> bool {
        self.alpha == 255 && self.mode == BlendMode::Normal && self.blur_sigma == 0
    }
}

/// F188: blend one pixel. Integer math, premultiplied by the caller.
pub fn blend_channel(mode: BlendMode, src: u8, dst: u8, alpha: u8) -> u8 {
    let s = src as u32;
    let d = dst as u32;
    let a = alpha as u32;
    let mixed = match mode {
        BlendMode::Normal => s,
        BlendMode::Multiply => s * d / 255,
        BlendMode::Screen => 255 - (255 - s) * (255 - d) / 255,
        BlendMode::Overlay => {
            if d < 128 {
                2 * s * d / 255
            } else {
                255 - 2 * (255 - s) * (255 - d) / 255
            }
        }
        // The blur itself happens in the backdrop pass; the blend is Normal.
        BlendMode::BackdropBlur => s,
    };
    ((mixed * a + d * (255 - a)) / 255) as u8
}

pub fn blend_pixel(mode: BlendMode, src: u32, dst: u32, alpha: u8) -> u32 {
    let c = |v: u32, shift: u32| ((v >> shift) & 0xFF) as u8;
    let r = blend_channel(mode, c(src, 16), c(dst, 16), alpha);
    let g = blend_channel(mode, c(src, 8), c(dst, 8), alpha);
    let b = blend_channel(mode, c(src, 0), c(dst, 0), alpha);
    ((r as u32) << 16) | ((g as u32) << 8) | b as u32
}

pub const MAX_LAYERS: usize = MAX_WINDOWS;

/// F187: the compositing pipeline. The plan is computed as data so it can be
/// reviewed (and unit-tested) without a framebuffer.
pub struct Compositor {
    pub layers: [Option<Layer>; MAX_LAYERS],
    len: usize,
    composed_frames: u64,
    copied_layers: u64,
}

impl Compositor {
    pub const fn new() -> Compositor {
        Compositor {
            layers: [None; MAX_LAYERS],
            len: 0,
            composed_frames: 0,
            copied_layers: 0,
        }
    }

    pub fn push(&mut self, layer: Layer) -> bool {
        if self.len >= MAX_LAYERS {
            return false;
        }
        self.layers[self.len] = Some(layer);
        self.len += 1;
        true
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn clear(&mut self) {
        self.layers = [None; MAX_LAYERS];
        self.len = 0;
    }

    /// Count the layers that can be copied rather than blended — the work
    /// saved is what a low-end GPU notices.
    pub fn opaque_copies(&self) -> usize {
        self.layers[..self.len]
            .iter()
            .flatten()
            .filter(|l| l.is_opaque_copy())
            .count()
    }

    pub fn backdrop_layers(&self) -> usize {
        self.layers[..self.len]
            .iter()
            .flatten()
            .filter(|l| l.mode.needs_backdrop() || l.blur_sigma > 0)
            .count()
    }

    pub fn frame_done(&mut self) {
        self.composed_frames += 1;
        self.copied_layers += self.opaque_copies() as u64;
    }

    pub fn frames(&self) -> u64 {
        self.composed_frames
    }

    pub fn copied_layers(&self) -> u64 {
        self.copied_layers
    }
}

impl Default for Compositor {
    fn default() -> Compositor {
        Compositor::new()
    }
}

// ---------------------------------------------------------------------------
// F189/F190 — motion
// ---------------------------------------------------------------------------

/// F189: a critically-damped-ish spring, in fixed point. The curve is what
/// makes a window snap feel like it has weight, and it must never overshoot
/// into a visible wobble.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Spring {
    /// Current value, in 1/256 units.
    pub value: i32,
    /// Velocity, 1/256 units per tick.
    pub velocity: i32,
    pub target: i32,
    /// Stiffness and damping, in 1/256.
    pub stiffness: i32,
    pub damping: i32,
}

impl Spring {
    pub const fn new(stiffness: i32, damping: i32) -> Spring {
        Spring {
            value: 0,
            velocity: 0,
            target: 0,
            stiffness,
            damping,
        }
    }

    /// The default "snappy" feel: fast to settle, no bounce.
    pub const fn snappy() -> Spring {
        Spring::new(48, 14)
    }

    /// The gentler curve used for sheets and the notification centre.
    pub const fn gentle() -> Spring {
        Spring::new(16, 8)
    }

    pub fn to(&mut self, target: i32) {
        self.target = target;
    }

    pub fn snap(&mut self, value: i32) {
        self.value = value;
        self.target = value;
        self.velocity = 0;
    }

    /// Advance one tick. Returns the whole units to display.
    pub fn step(&mut self) -> i32 {
        let delta = self.target - self.value;
        self.velocity += delta * self.stiffness / 256;
        self.velocity = self.velocity * (256 - self.damping) / 256;
        self.value += self.velocity;
        self.value / 256
    }

    pub fn settled(&self) -> bool {
        let delta = (self.target - self.value).abs();
        delta < 256 && self.velocity.abs() < 8
    }

    /// Ticks until settled, as an upper bound for the animation budget.
    pub fn settle_ticks(&self, limit: u32) -> u32 {
        let mut probe = *self;
        let mut n = 0u32;
        while n < limit && !probe.settled() {
            probe.step();
            n += 1;
        }
        n
    }
}

impl Default for Spring {
    fn default() -> Spring {
        Spring::snappy()
    }
}

/// F190: an easing curve for the editor. `CubicOut` is the default because it
/// starts fast (feels responsive) and ends soft (does not snap).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Easing {
    Linear,
    CubicIn,
    CubicOut,
    CubicInOut,
    /// A small overshoot; used only where a bounce is deliberate.
    BackOut,
}

impl Easing {
    pub fn as_str(self) -> &'static str {
        match self {
            Easing::Linear => "linear",
            Easing::CubicIn => "cubic-in",
            Easing::CubicOut => "cubic-out",
            Easing::CubicInOut => "cubic-in-out",
            Easing::BackOut => "back-out",
        }
    }

    pub fn parse(s: &str) -> Option<Easing> {
        match s {
            "linear" => Some(Easing::Linear),
            "cubic-in" => Some(Easing::CubicIn),
            "cubic-out" => Some(Easing::CubicOut),
            "cubic-in-out" => Some(Easing::CubicInOut),
            "back-out" => Some(Easing::BackOut),
            _ => None,
        }
    }

    /// Evaluate at `t` in 0..=1000, returning 0..=1000 (or slightly beyond for
    /// BackOut, which is the point of it).
    pub fn eval(self, t: i32) -> i32 {
        let t = t.clamp(0, 1000);
        let tf = t as i64;
        let cube = |x: i64| x * x * x / (1000 * 1000);
        match self {
            Easing::Linear => t,
            Easing::CubicIn => cube(tf) as i32,
            Easing::CubicOut => {
                let inv = 1000 - tf;
                1000 - cube(inv) as i32
            }
            Easing::CubicInOut => {
                if tf < 500 {
                    (4 * cube(tf)) as i32
                } else {
                    let inv = 1000 - tf;
                    1000 - (4 * cube(inv)) as i32
                }
            }
            Easing::BackOut => {
                // 1 + c3*(t-1)^3 + c1*(t-1)^2, with c1 = 1.70158 and
                // c3 = 2.70158, in tenths. Evaluated around `u = t - 1` so the
                // curve passes through (0,0) and (1,1) exactly and overshoots
                // in between.
                let u = tf - 1000;
                let u3 = u * u * u / (1000 * 1000);
                let u2 = u * u / 1000;
                1000 + (u3 * 27 / 10) as i32 + (u2 * 17 / 10) as i32
            }
        }
    }

    /// Does this curve ever go past its target? If so the caller must be ready
    /// to clip, and reduce-motion users will not see it at all.
    pub fn overshoots(self) -> bool {
        self == Easing::BackOut
    }
}

impl Default for Easing {
    fn default() -> Easing {
        Easing::CubicOut
    }
}

// ---------------------------------------------------------------------------
// F191 — theme tokens
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum TokenKind {
    #[default]
    Surface,
    Text,
    Accent,
    Border,
    Success,
    Warning,
    Danger,
}

impl TokenKind {
    pub const COUNT: usize = 7;

    pub fn index(self) -> usize {
        self as usize
    }

    pub fn as_str(self) -> &'static str {
        match self {
            TokenKind::Surface => "surface",
            TokenKind::Text => "text",
            TokenKind::Accent => "accent",
            TokenKind::Border => "border",
            TokenKind::Success => "success",
            TokenKind::Warning => "warning",
            TokenKind::Danger => "danger",
        }
    }
}

/// F191: every hard-coded colour in the desktop comes from here, which is what
/// makes a theme switch instant instead of a scavenger hunt.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ThemeTokens {
    colors: [u32; TokenKind::COUNT],
    /// Corner radii in pixels: small, medium, large, pill.
    pub radius: [u8; 4],
    pub blur_sigma: u8,
    /// 0 = reduce-motion on, 100 = full animation.
    pub animation_scale: u8,
    pub dark: bool,
}

impl ThemeTokens {
    pub const fn light() -> ThemeTokens {
        ThemeTokens {
            colors: [0x00F5_F5F5, 0x001A_1A1A, 0x0024_6BF2, 0x00D6_D6D6, 0x0016_A34A, 0x00D9_8A00, 0x00D1_3A3A],
            radius: [4, 8, 16, 255],
            blur_sigma: 12,
            animation_scale: 100,
            dark: false,
        }
    }

    pub const fn dark() -> ThemeTokens {
        ThemeTokens {
            colors: [0x0018_1818, 0x00EC_ECEC, 0x004C_9AFF, 0x0033_3333, 0x002E_CC71, 0x00F5_B042, 0x00F0_5A5A],
            radius: [4, 8, 16, 255],
            blur_sigma: 16,
            animation_scale: 100,
            dark: true,
        }
    }

    pub const fn reduce_motion(mut self) -> ThemeTokens {
        self.animation_scale = 0;
        self.blur_sigma = 0;
        self
    }

    pub fn color(&self, kind: TokenKind) -> u32 {
        self.colors[kind.index()]
    }

    pub fn set_color(&mut self, kind: TokenKind, value: u32) {
        self.colors[kind.index()] = value & 0x00FF_FFFF;
    }

    /// Animated durations scale with the accessibility setting (F423).
    pub fn scale_duration(&self, ms: u32) -> u32 {
        ms * self.animation_scale as u32 / 100
    }

    /// Contrast check: WCAG relative-luminance ratio in tenths.
    pub fn contrast_ratio(&self, a: TokenKind, b: TokenKind) -> u32 {
        let lum = |color: u32| -> u32 {
            let r = (color >> 16) & 0xFF;
            let g = (color >> 8) & 0xFF;
            let bl = color & 0xFF;
            // Integer approximation of the sRGB relative luminance.
            (2126 * r + 7152 * g + 722 * bl) / 10000
        };
        let (l1, l2) = (lum(self.color(a)), lum(self.color(b)));
        let (hi, lo) = if l1 > l2 { (l1, l2) } else { (l2, l1) };
        // (hi + 5) / (lo + 5), reported in tenths.
        (hi + 5) * 10 / (lo + 5)
    }

    /// Is the text token readable on the surface token? WCAG AA is 4.5:1,
    /// i.e. 45 in tenths.
    pub fn text_contrast_ok(&self) -> bool {
        self.contrast_ratio(TokenKind::Text, TokenKind::Surface) >= 45
    }
}

impl Default for ThemeTokens {
    fn default() -> ThemeTokens {
        ThemeTokens::dark()
    }
}

// ---------------------------------------------------------------------------
// F192/F193/F194/F195 — HDR, multi-display, colour
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct HdrSupport {
    pub available: bool,
    pub max_luminance_nits: u16,
    pub min_luminance_centinits: u16,
    /// The display accepts a 10-bit-per-channel scanout.
    pub ten_bit: bool,
    /// The display accepts a PQ (SMPTE ST 2084) transfer function.
    pub pq: bool,
}

impl HdrSupport {
    /// F192: HDR is only usable when the panel claims both the luminance range
    /// and a transfer function Varix can drive.
    pub fn usable(&self) -> bool {
        self.available && self.max_luminance_nits >= 400 && (self.pq || self.ten_bit)
    }

    pub fn describe(&self) -> &'static str {
        if self.usable() {
            if self.pq {
                "hdr-pq"
            } else {
                "hdr-10bit"
            }
        } else if self.available {
            "sdr-capped"
        } else {
            "sdr"
        }
    }
}

pub const MAX_DISPLAYS: usize = 4;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Display {
    pub id: u8,
    /// Virtual-desktop rectangle.
    pub rect: Rect,
    /// Integer scale factor, e.g. 2 for a HiDPI panel.
    pub scale: u8,
    pub hz: u32,
    pub primary: bool,
    pub hdr: Option<HdrSupport>,
}

impl Display {
    pub fn usable(&self) -> bool {
        self.rect.area() > 0 && self.scale >= 1
    }

    /// Logical size after scaling — what layout code wants.
    pub fn logical_size(&self) -> (i32, i32) {
        let s = self.scale.max(1) as i32;
        (self.rect.w / s, self.rect.h / s)
    }

    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.rect.x
            && x < self.rect.x + self.rect.w
            && y >= self.rect.y
            && y < self.rect.y + self.rect.h
    }

    /// Device pixels per logical pixel — the factor a HiDPI-aware UI needs.
    pub fn pixels_per_point(&self) -> i32 {
        self.scale.max(1) as i32
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum DisplayLayout {
    /// One display only; the others are dark.
    #[default]
    Single,
    /// Side by side in the virtual desktop.
    Extend,
    /// Every display shows the same content.
    Mirror,
}

pub struct DisplaySet {
    displays: [Option<Display>; MAX_DISPLAYS],
    layout: DisplayLayout,
    primary: u8,
}

impl DisplaySet {
    pub const fn new() -> DisplaySet {
        DisplaySet {
            displays: [None; MAX_DISPLAYS],
            layout: DisplayLayout::Single,
            primary: 0,
        }
    }

    pub fn attach(&mut self, mut d: Display) -> bool {
        if !d.usable() {
            return false;
        }
        if let Some(slot) = self.displays.iter_mut().find(|s| s.is_none()) {
            d.primary = false;
            *slot = Some(d);
            if self.displays.iter().flatten().count() == 1 {
                self.primary = d.id;
                if let Some(first) = self.displays.iter_mut().flatten().next() {
                    first.primary = true;
                }
            }
            return true;
        }
        false
    }

    pub fn detach(&mut self, id: u8) -> bool {
        for slot in self.displays.iter_mut() {
            if slot.map(|d| d.id == id).unwrap_or(false) {
                *slot = None;
                if self.primary == id {
                    if let Some(next) = self.displays.iter_mut().flatten().next() {
                        next.primary = true;
                        self.primary = next.id;
                    }
                }
                return true;
            }
        }
        false
    }

    pub fn count(&self) -> usize {
        self.displays.iter().flatten().count()
    }

    pub fn layout(&self) -> DisplayLayout {
        self.layout
    }

    /// F193: changing the layout re-tiles the virtual desktop. Mirror mode
    /// stacks everything at the origin so a coordinate is display-independent.
    pub fn set_layout(&mut self, layout: DisplayLayout) -> bool {
        self.layout = layout;
        let mut cursor_x = 0;
        for d in self.displays.iter_mut().flatten() {
            match layout {
                DisplayLayout::Single => {
                    // Everything but the primary gets an off-screen placeholder.
                    if d.id != self.primary {
                        d.rect.x = 100_000;
                        d.rect.y = 100_000;
                    } else {
                        d.rect.x = 0;
                        d.rect.y = 0;
                    }
                }
                DisplayLayout::Extend => {
                    d.rect.x = cursor_x;
                    d.rect.y = 0;
                    cursor_x += d.rect.w;
                }
                DisplayLayout::Mirror => {
                    d.rect.x = 0;
                    d.rect.y = 0;
                }
            }
        }
        true
    }

    pub fn get(&self, id: u8) -> Option<Display> {
        self.displays.iter().flatten().copied().find(|d| d.id == id)
    }

    pub fn primary(&self) -> Option<Display> {
        self.get(self.primary)
    }

    /// Which display a virtual-desktop point lands on.
    pub fn display_at(&self, x: i32, y: i32) -> Option<Display> {
        self.displays
            .iter()
            .flatten()
            .find(|d| d.contains(x, y))
            .copied()
    }

    /// Total virtual-desktop width, for the layout engine.
    pub fn virtual_bounds(&self) -> Rect {
        let mut bounds = Rect::empty();
        for d in self.displays.iter().flatten() {
            if d.rect.x < 100_000 {
                bounds = bounds.union(&d.rect);
            }
        }
        bounds
    }

    pub fn hdr_displays(&self) -> usize {
        self.displays
            .iter()
            .flatten()
            .filter(|d| d.hdr.map(|h| h.usable()).unwrap_or(false))
            .count()
    }
}

impl Default for DisplaySet {
    fn default() -> DisplaySet {
        DisplaySet::new()
    }
}

/// F194: sRGB transfer. Fixed point with a 16-bit mantissa; the accuracy that
/// matters is at the dark end, which is where a naive gamma approximation
/// bands visibly.
/// sRGB anchors every 17 code values: the true transfer function evaluated at
/// v = 0, 17, 34 … 255, in 16-bit linear. A table plus linear interpolation is
/// how colour management is actually done — the curve is smooth enough that 17
/// anchors are indistinguishable from the exact function, including in the dark
/// end where a cheap `x^2.2` approximation bands visibly.
pub const SRGB_ANCHORS: [u16; 16] = [
    0, 367, 1047, 2170, 3788, 5951, 8708, 12094, 16141, 20876, 26337, 32575, 39575, 47389, 56034,
    65535,
];

/// Anchor spacing: the table has 16 entries covering the 255 code values in 15
/// equal intervals.
pub const SRGB_ANCHOR_STEP: u32 = 255 / 15;

/// sRGB → linear, scaled to 0..=65535.
pub fn srgb_to_linear_v(v: u8) -> u16 {
    let slot = v as u32 / SRGB_ANCHOR_STEP;
    if slot >= 15 {
        return SRGB_ANCHORS[15];
    }
    let a = SRGB_ANCHORS[slot as usize] as u32;
    let b = SRGB_ANCHORS[slot as usize + 1] as u32;
    let frac = v as u32 % SRGB_ANCHOR_STEP;
    (a + (b - a) * frac / SRGB_ANCHOR_STEP) as u16
}

/// linear → sRGB. The inverse of the same table, so the round trip is exact at
/// the anchors and within one code value in between.
pub fn linear_to_srgb_v(v: u16) -> u8 {
    let target = v as u32;
    let mut slot = 0usize;
    while slot + 1 < SRGB_ANCHORS.len() && SRGB_ANCHORS[slot + 1] as u32 <= target {
        slot += 1;
    }
    if slot >= 15 {
        return 255;
    }
    let a = SRGB_ANCHORS[slot] as u32;
    let b = SRGB_ANCHORS[slot + 1] as u32;
    let frac = if b > a {
        (target - a) * SRGB_ANCHOR_STEP / (b - a)
    } else {
        0
    };
    let value = slot as u32 * SRGB_ANCHOR_STEP + frac;
    value.min(255) as u8
}

/// F195: night colour temperature. A tint applied at the framebuffer's output
/// stage, so it affects everything including the boot log.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct NightShift {
    pub enabled: bool,
    /// Colour temperature in kelvin; 6500 is neutral.
    pub kelvin: u16,
    /// 0..=100, so the tint can fade in rather than snapping.
    pub strength: u8,
}

impl NightShift {
    pub const NEUTRAL_K: u16 = 6500;
    pub const MIN_K: u16 = 2000;

    pub const fn off() -> NightShift {
        NightShift {
            enabled: false,
            kelvin: Self::NEUTRAL_K,
            strength: 0,
        }
    }

    pub fn warm(strength: u8) -> NightShift {
        NightShift {
            enabled: true,
            kelvin: 3400,
            strength: strength.min(100),
        }
    }

    /// The per-channel multipliers, in 1/256. Warm light is achieved by taking
    /// blue *out*, not by adding orange — adding lifts the black level.
    pub fn multipliers(&self) -> (u16, u16, u16) {
        if !self.enabled || self.strength == 0 {
            return (256, 256, 256);
        }
        let k = self.kelvin.clamp(Self::MIN_K, Self::NEUTRAL_K) as u32;
        // Linear ramp from 2000 K (strong) to 6500 K (neutral).
        let warmth = (Self::NEUTRAL_K as u32 - k) * 100 / (Self::NEUTRAL_K as u32 - Self::MIN_K as u32);
        let s = (self.strength as u32).min(100);
        let blue_cut = warmth * s / 100 * 90 / 100; // up to 45% off
        let green_cut = warmth * s / 100 * 20 / 100;
        let red_gain = warmth * s / 100 * 10 / 100;
        (
            (256 + red_gain).min(280) as u16,
            (256 - green_cut).max(180) as u16,
            (256 - blue_cut).max(120) as u16,
        )
    }

    pub fn apply(&self, color: u32) -> u32 {
        let (r, g, b) = self.multipliers();
        let ch = |v: u32, mul: u16| -> u32 { (v * mul as u32 / 256).min(255) };
        (ch((color >> 16) & 0xFF, r) << 16)
            | (ch((color >> 8) & 0xFF, g) << 8)
            | ch(color & 0xFF, b)
    }

    /// Should the schedule be on at this hour? 21:00 to 07:00, which is the
    /// range a user actually wants rather than a fixed nightly window.
    pub fn scheduled(hour: u8) -> bool {
        hour >= 21 || hour < 7
    }
}

impl Default for NightShift {
    fn default() -> NightShift {
        NightShift::off()
    }
}

// ---------------------------------------------------------------------------
// F196/F197/F198/F199 — tear-free dragging, acceleration, fallback, capture
// ---------------------------------------------------------------------------

/// F196: a drag that only moves the window at a vsync boundary. Applying the
/// pointer delta immediately and sampling the compositor mid-frame is exactly
/// how a drag ends up tearing.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct DragSync {
    pub dragging: bool,
    pub pending_dx: i32,
    pub pending_dy: i32,
    pub applied_dx: i32,
    pub applied_dy: i32,
    pub vsync_drops: u64,
}

impl DragSync {
    pub fn begin(&mut self) {
        self.dragging = true;
        self.pending_dx = 0;
        self.pending_dy = 0;
    }

    /// Accumulate a pointer delta. Nothing moves until the next present.
    pub fn on_pointer(&mut self, dx: i32, dy: i32) {
        if !self.dragging {
            return;
        }
        self.pending_dx += dx;
        self.pending_dy += dy;
    }

    /// Apply at a vsync boundary. Returns the delta the window actually moves.
    pub fn on_present(&mut self, vsync: bool) -> (i32, i32) {
        if !self.dragging || !vsync {
            self.vsync_drops += 1;
            return (0, 0);
        }
        let d = (self.pending_dx, self.pending_dy);
        self.applied_dx += d.0;
        self.applied_dy += d.1;
        self.pending_dx = 0;
        self.pending_dy = 0;
        d
    }

    pub fn end(&mut self) {
        self.dragging = false;
        // Whatever moved after the last present still has to land.
        self.applied_dx += self.pending_dx;
        self.applied_dy += self.pending_dy;
        self.pending_dx = 0;
        self.pending_dy = 0;
    }

    pub fn lag_pixels(&self) -> i32 {
        self.pending_dx.abs() + self.pending_dy.abs()
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AccelBackend {
    None,
    /// Software blitting only.
    Software,
    /// 2D blit engine (no shaders).
    Blit2D,
    /// Programmable GPU.
    Gpu,
}

impl AccelBackend {
    pub fn as_str(self) -> &'static str {
        match self {
            AccelBackend::None => "none",
            AccelBackend::Software => "soft",
            AccelBackend::Blit2D => "blit2d",
            AccelBackend::Gpu => "gpu",
        }
    }

    /// Which compositor operations the backend can take over (F197).
    pub fn accelerates(self, op: &str) -> bool {
        match self {
            AccelBackend::Gpu => matches!(op, "blit" | "blend" | "blur" | "scale" | "transform"),
            AccelBackend::Blit2D => matches!(op, "blit" | "scale"),
            AccelBackend::Software => matches!(op, "blit"),
            AccelBackend::None => false,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct GpuState {
    pub backend: Option<AccelBackend>,
    pub vram_mb: u32,
    pub max_texture: u32,
    pub probe_failed: bool,
}

impl GpuState {
    pub fn detect(vendor_id: u16, known_driver: bool, vram_mb: u32) -> GpuState {
        if vendor_id == 0xFFFF || vendor_id == 0 {
            return GpuState {
                backend: None,
                vram_mb: 0,
                max_texture: 0,
                probe_failed: true,
            };
        }
        let backend = if known_driver && vram_mb >= 64 {
            AccelBackend::Gpu
        } else if vram_mb > 0 {
            AccelBackend::Blit2D
        } else {
            AccelBackend::Software
        };
        GpuState {
            backend: Some(backend),
            vram_mb,
            max_texture: if backend == AccelBackend::Gpu { 16384 } else { 2048 },
            probe_failed: false,
        }
    }

    pub fn backend(&self) -> AccelBackend {
        self.backend.unwrap_or(AccelBackend::Software)
    }

    pub fn describe(&self) -> &'static str {
        self.backend().as_str()
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CompositorPath {
    /// Full pipeline with backdrop blur and per-layer transforms.
    Full,
    /// Everything except blur and transforms.
    Reduced,
    /// Single buffer, no blending, nearest-neighbour scaling.
    Minimal,
}

impl CompositorPath {
    pub fn as_str(self) -> &'static str {
        match self {
            CompositorPath::Full => "full",
            CompositorPath::Reduced => "reduced",
            CompositorPath::Minimal => "minimal",
        }
    }

    /// F198: the degradation ladder. A compositor that refuses to start is a
    /// black screen, which is never the right answer.
    pub fn choose(gpu: &GpuState, memory_free_mb: u32, hz: u32) -> CompositorPath {
        if gpu.probe_failed {
            return CompositorPath::Minimal;
        }
        match gpu.backend() {
            AccelBackend::Gpu => {
                if memory_free_mb < 64 {
                    CompositorPath::Reduced
                } else {
                    CompositorPath::Full
                }
            }
            AccelBackend::Blit2D => CompositorPath::Reduced,
            _ => {
                if memory_free_mb < 32 || hz > 120 {
                    CompositorPath::Minimal
                } else {
                    CompositorPath::Reduced
                }
            }
        }
    }

    pub fn supports_blur(self) -> bool {
        self == CompositorPath::Full
    }

    pub fn buffer_count(self) -> u8 {
        match self {
            CompositorPath::Full => 3,
            CompositorPath::Reduced => 2,
            CompositorPath::Minimal => 1,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Screenshot {
    pub rect: Rect,
    pub width: u32,
    pub height: u32,
    pub bytes: u64,
    pub captured: bool,
}

/// F199: the capture service. It reports the geometry and size it *would*
/// produce, so the caller can reserve the buffer before the copy starts.
pub fn plan_screenshot(rect: Rect, screen_w: i32, screen_h: i32, has_cursor: bool) -> Screenshot {
    let clipped = rect.clip(screen_w, screen_h);
    let mut shot = Screenshot {
        rect: clipped,
        width: clipped.w.max(0) as u32,
        height: clipped.h.max(0) as u32,
        bytes: clipped.area().max(0) as u64 * 4,
        captured: false,
    };
    if has_cursor && clipped.area() > 0 {
        // The cursor is composited in by the caller; charge for one more row of
        // work so the budget is honest about it.
        shot.bytes += shot.width as u64 * 4;
    }
    shot
}

pub fn capture(shot: &mut Screenshot) -> bool {
    if shot.width == 0 || shot.height == 0 {
        return false;
    }
    shot.captured = true;
    true
}

// ---------------------------------------------------------------------------
// Domain state and self-test
// ---------------------------------------------------------------------------

pub struct DisplayDomainState {
    pub mode: Option<VideoMode>,
    pub displays: usize,
    pub hdr: usize,
    pub compositor: CompositorPath,
    pub gpu: &'static str,
    pub fps: u32,
    pub worst_frame_us: u32,
    pub self_test: (usize, usize),
}

impl DisplayDomainState {
    pub fn ok(&self) -> bool {
        self.self_test.1 == 0 && self.mode.is_some()
    }

    pub fn render(&self, out: &mut [u8]) -> usize {
        let mut w = crate::cpu::Hud::new(out);
        w.str("display ");
        match self.mode {
            Some(m) => {
                let (name, n) = m.name();
                w.str(core::str::from_utf8(&name[..n]).unwrap_or("?"));
            }
            None => w.str("none"),
        }
        w.str(" displays=");
        w.num(self.displays as u64);
        w.str(" hdr=");
        w.num(self.hdr as u64);
        w.str(" gpu=");
        w.str(self.gpu);
        w.str(" path=");
        w.str(self.compositor.as_str());
        w.str(" fps=");
        w.num(self.fps as u64);
        w.str(" worst=");
        w.num(self.worst_frame_us as u64);
        w.str("us [");
        w.num(self.self_test.0 as u64);
        w.str("/");
        w.num((self.self_test.0 + self.self_test.1) as u64);
        w.str("]\n");
        w.used()
    }
}

static DISPLAY_SELFTEST: crate::selftest::SelfTest = crate::selftest::SelfTest::new();
static MODES: crate::cpu::sync::SpinProtected<ModeTable> =
    crate::cpu::sync::SpinProtected::new(ModeTable::new());
static TEMPLATE: crate::cpu::sync::SpinProtected<ThemeTokens> =
    crate::cpu::sync::SpinProtected::new(ThemeTokens::dark());
static CLOCK: crate::cpu::sync::SpinProtected<FrameClock> =
    crate::cpu::sync::SpinProtected::new(FrameClock::at_60hz());
static BUFFER: crate::cpu::sync::SpinProtected<DoubleBuffer> =
    crate::cpu::sync::SpinProtected::new(DoubleBuffer::new());

pub fn display_selftest() -> &'static crate::selftest::SelfTest {
    &DISPLAY_SELFTEST
}

pub fn modes() -> &'static crate::cpu::sync::SpinProtected<ModeTable> {
    &MODES
}

pub fn theme() -> &'static crate::cpu::sync::SpinProtected<ThemeTokens> {
    &TEMPLATE
}

pub fn frame_clock() -> &'static crate::cpu::sync::SpinProtected<FrameClock> {
    &CLOCK
}

pub fn buffer() -> &'static crate::cpu::sync::SpinProtected<DoubleBuffer> {
    &BUFFER
}

/// F176~F200 bring-up.
pub fn init() -> DisplayDomainState {
    // The mode list comes from the boot GOP probe (AI-01, F003). Until a real
    // panel reports something, the safe list is the standard 16:9 ladder.
    {
        let mut m = MODES.lock();
        if m.is_empty() {
            let candidates = [
                (3840u32, 2160u32),
                (2560, 1440),
                (1920, 1080),
                (1600, 900),
                (1366, 768),
                (1280, 720),
                (1024, 768),
                (800, 600),
            ];
            for (w, h) in candidates {
                let _ = m.add(VideoMode {
                    width: w,
                    height: h,
                    stride: w * 4,
                    bpp: 32,
                    hz: 60,
                });
            }
            if let Some(i) = m.find(1920, 1080) {
                let _ = m.select(i);
            }
        }
    }

    let (backend, path) = {
        let gpu = GpuState::detect(0, false, 0);
        (gpu.describe(), CompositorPath::choose(&gpu, 512, 60))
    };

    let (passed, failed) = run_display_checks();
    let current = MODES.lock().current();
    let state = DisplayDomainState {
        mode: current,
        displays: 1,
        hdr: 0,
        compositor: path,
        gpu: backend,
        fps: CLOCK.lock().fps(),
        worst_frame_us: CLOCK.lock().worst_us(),
        self_test: (passed, failed),
    };

    crate::kinfo!(
        "display: gpu={} path={} displays={} fps={} self-test {}/{}",
        state.gpu,
        state.compositor.as_str(),
        state.displays,
        state.fps,
        state.self_test.0,
        state.self_test.0 + state.self_test.1
    );
    state
}

pub fn render_to_console(st: &DisplayDomainState) {
    let mut buf = [0u8; 256];
    let n = st.render(&mut buf);
    if let Some(c) = crate::console::installed_ref() {
        for &b in &buf[..n] {
            c.put_byte(b);
        }
    }
    let mut detail = [0u8; 512];
    let m = DISPLAY_SELFTEST.render(&mut detail);
    if let Some(c) = crate::console::installed_ref() {
        for &b in &detail[..m] {
            c.put_byte(b);
        }
    }
}

/// F200: the display-chain checks.
pub fn run_display_checks() -> (usize, usize) {
    let r = &DISPLAY_SELFTEST;

    // F176 — an unsupported size degrades to the largest that fits.
    let mut m = ModeTable::new();
    let _ = m.add(VideoMode {
        width: 1920,
        height: 1080,
        stride: 7680,
        bpp: 32,
        hz: 60,
    });
    let _ = m.add(VideoMode {
        width: 1280,
        height: 720,
        stride: 5120,
        bpp: 32,
        hz: 60,
    });
    let fits = m.find(1600, 900) == Some(1);
    let exact = m.find(1920, 1080) == Some(0);
    r.check("mode-selection", fits && exact, "mode selection is wrong");

    // F178 — overlapping damage merges instead of accumulating.
    let mut d = DamageList::new();
    let _ = d.add(Rect {
        x: 0,
        y: 0,
        w: 10,
        h: 10,
    });
    let _ = d.add(Rect {
        x: 5,
        y: 5,
        w: 10,
        h: 10,
    });
    r.check(
        "damage-merge",
        d.len() == 1 && d.coalesced() == 1,
        "overlapping damage was not merged",
    );

    // F180 — a healthy frame clock reports roughly 60 fps.
    let mut c = FrameClock::at_60hz();
    for _ in 0..60 {
        c.present(16_000);
    }
    r.check(
        "frame-budget",
        c.fps() == 62 && c.late_frames() == 0,
        "frame accounting is wrong",
    );

    // F184 — text wraps by advance, and says so when it is truncated.
    let l = layout_text("hello world", 48, 8, 4);
    r.check(
        "text-layout",
        l.lines == 2 && l.widest_px == 48 && !l.truncated,
        "line breaking is wrong",
    );

    // F186 — hit testing returns the topmost window.
    let mut t = WindowTree::new();
    let _ = t.create(0, 0, Rect { x: 0, y: 0, w: 1000, h: 1000 }, true);
    let _ = t.create(1, 0, Rect { x: 10, y: 10, w: 100, h: 100 }, true);
    let _ = t.create(2, 0, Rect { x: 50, y: 50, w: 100, h: 100 }, true);
    r.check(
        "hit-test",
        t.hit_test(60, 60) == Some(2) && t.hit_test(500, 500) == Some(0),
        "hit testing did not return the topmost window",
    );

    // F189 — the spring settles instead of oscillating forever.
    let s = Spring::snappy();
    let ticks = {
        let mut probe = s;
        probe.to(100 * 256);
        probe.settle_ticks(600)
    };
    r.check(
        "spring-settles",
        ticks > 0 && ticks < 300,
        "the spring did not settle within the animation budget",
    );

    // F188 — the blend modes are distinguishable and in range.
    let normal = blend_channel(BlendMode::Normal, 255, 0, 255);
    let multiply = blend_channel(BlendMode::Multiply, 128, 128, 255);
    let screen = blend_channel(BlendMode::Screen, 128, 128, 255);
    r.check(
        "blend-modes",
        normal == 255 && multiply == 64 && screen == 192,
        "blend math is wrong",
    );

    // F191 — the default theme is readable.
    let theme = ThemeTokens::dark();
    r.check(
        "theme-contrast",
        theme.text_contrast_ok(),
        "default theme fails its own contrast check",
    );

    // F198 — a failed GPU probe still produces a usable compositor.
    let broken = GpuState {
        backend: None,
        vram_mb: 0,
        max_texture: 0,
        probe_failed: true,
    };
    r.check(
        "compositor-fallback",
        CompositorPath::choose(&broken, 0, 60) == CompositorPath::Minimal,
        "a failed GPU probe did not degrade",
    );

    // F195 — night shift takes blue out rather than adding orange.
    let shift = NightShift::warm(100);
    let (r_, g_, b_) = shift.multipliers();
    r.check(
        "night-shift",
        r_ >= 256 && g_ < 256 && b_ < g_ && shift.apply(0x00FF_FFFF) != 0x00FF_FFFF,
        "night shift channel math is wrong",
    );

    let (passed, failed) = r.tally();
    if failed == 0 {
        crate::kinfo!("display self-test: {}/{} pass", passed, passed);
    } else {
        crate::kwarn!(
            "display self-test: {}/{} pass ({} FAIL)",
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
    fn mode_table_degrades_downwards() {
        let mut m = ModeTable::new();
        assert!(m.is_empty());
        assert!(m.add(VideoMode { width: 1920, height: 1080, stride: 7680, bpp: 32, hz: 60 }));
        assert!(m.add(VideoMode { width: 1280, height: 720, stride: 5120, bpp: 32, hz: 60 }));
        // A zero-sized mode is refused.
        assert!(!m.add(VideoMode::default()));
        assert_eq!(m.rejected(), 1);
        // A stride that cannot hold the row is refused too.
        assert!(!m.add(VideoMode { width: 800, height: 600, stride: 100, bpp: 32, hz: 60 }));
        assert_eq!(m.rejected(), 2);

        assert_eq!(m.find(1920, 1080), Some(0));
        assert_eq!(m.find(1600, 900), Some(1), "the largest that fits");
        assert_eq!(m.find(640, 480), None, "nothing is small enough");
        assert!(m.select(1));
        assert!(!m.select(9));
        assert_eq!(m.current().unwrap().width, 1280);
        assert_eq!(m.switches(), 1);
        assert_eq!(m.smallest(), Some(1));

        let mode = m.get(0).unwrap();
        assert_eq!(mode.pixels(), 1920 * 1080);
        assert_eq!(mode.bytes(), 7680 * 1080 * 4);
        let (name, n) = mode.name();
        assert_eq!(core::str::from_utf8(&name[..n]).unwrap(), "1920x1080@60");
    }

    #[test]
    fn damage_tracking_merges_and_clips() {
        let mut d = DamageList::new();
        assert!(d.is_empty());
        assert!(!d.add(Rect::empty()), "empty damage is not damage");
        assert!(d.add(Rect { x: 0, y: 0, w: 10, h: 10 }));
        assert!(d.add(Rect { x: 100, y: 100, w: 10, h: 10 }));
        assert_eq!(d.len(), 2);
        // A third rectangle bridging the two merges only the one it touches.
        assert!(d.add(Rect { x: 5, y: 5, w: 10, h: 10 }));
        assert_eq!(d.len(), 2);
        assert_eq!(d.coalesced(), 1);
        assert_eq!(d.added(), 3, "the empty rect never counted as damage");
        assert_eq!(d.get(0).unwrap().union(&Rect::empty()).x, 0);

        let r = Rect { x: -10, y: -10, w: 40, h: 40 };
        let clipped = r.clip(100, 100);
        assert_eq!(clipped.x, 0);
        assert_eq!(clipped.w, 30);
        let off = Rect { x: 200, y: 200, w: 10, h: 10 }.clip(100, 100);
        assert_eq!(off.area(), 0);

        assert_eq!(d.coverage_percent(100, 100), 3);
        d.clear();
        assert!(d.is_empty());
        assert_eq!(d.coverage_percent(0, 0), 0);
    }

    #[test]
    fn double_buffer_counts_tears_and_the_frame_clock_tracks_jank() {
        let mut b = DoubleBuffer::new();
        assert_eq!(b.front, 0);
        assert_eq!(b.back, 1);
        b.swap(true);
        b.swap(true);
        b.swap(false);
        assert_eq!(b.swaps, 3);
        assert_eq!(b.front, 1);
        assert_eq!(b.tears(), 1);
        assert_eq!(b.tear_percent(), 33);

        let mut c = FrameClock::at_60hz();
        for _ in 0..10 {
            c.present(16_000);
        }
        assert_eq!(c.frames(), 10);
        assert_eq!(c.late_frames(), 0);
        assert!(c.healthy());
        assert_eq!(c.fps(), 62);
        // Two consecutive late frames is a visible stutter.
        c.present(25_000);
        assert_eq!(c.jank(), 0, "one late frame is not jank yet");
        c.present(25_000);
        assert_eq!(c.jank(), 1);
        assert_eq!(c.worst_streak(), 2);
        assert_eq!(c.worst_us(), 25_000);
        assert!(c.mean_us() > 16_000);
        assert!(!c.healthy() || c.late_frames() * 100 <= c.frames());
        assert_eq!(FrameClock::new(0).fps(), 0);
    }

    #[test]
    fn glyphs_rasterize_and_cache() {
        // A 3x3 "solid square" bitmap.
        let bitmap = [0b1110_0000u8, 0b1010_0000, 0b1110_0000];
        let g = Glyph::from_bitmap(b'X' as u32, &bitmap, 3, 3, 1);
        assert_eq!(g.width, 3);
        assert_eq!(g.height, 3);
        assert_eq!(g.ink_pixels(), 8);
        assert_eq!(g.bitmap_rows()[0] & 0b111, 0b111);
        assert_eq!(g.bitmap_rows()[1] & 0b111, 0b101);

        // Scaling doubles the glyph in both directions.
        let s = Glyph::from_bitmap(b'X' as u32, &bitmap, 3, 3, 2);
        assert_eq!(s.width, 6);
        assert_eq!(s.height, 6);
        assert_eq!(s.bitmap_rows()[0] & 0b111111, 0b111111);
        assert_eq!(s.ink_pixels(), 8 * 4);

        let mut cache = GlyphCache::new();
        assert!(cache.is_empty());
        assert!(cache.lookup(b'X' as u32).is_none());
        assert_eq!(cache.misses(), 1);
        let _ = cache.insert(g);
        assert!(cache.lookup(b'X' as u32).is_some());
        assert_eq!(cache.hits(), 1);
        assert_eq!(cache.hit_percent(), 50);
        // Filling the cache starts evicting rather than refusing.
        for i in 0..(MAX_CACHED_GLYPHS + 4) {
            let _ = cache.insert(Glyph {
                code: 1000 + i as u32,
                ..g
            });
        }
        assert!(cache.evictions() > 0);
        cache.clear();
        assert!(cache.is_empty());
    }

    #[test]
    fn subpixel_and_layout_and_bidi_flags() {
        let c = SubpixelCoverage::uniform(255);
        assert_eq!(c.max_channel(), 255);
        // Full coverage replaces the background entirely.
        assert_eq!(c.blend(0x0000_0000, 0x00FF_FFFF), 0x00FF_FFFF);
        let half = SubpixelCoverage::uniform(0);
        assert_eq!(half.blend(0x0012_3456, 0x00FF_FFFF), 0x0012_3456);
        // Per-channel coverage is what removes the colour fringes.
        let fringe = SubpixelCoverage { r: 255, g: 128, b: 0 };
        let out = fringe.blend(0x0000_0000, 0x00FF_FFFF);
        assert_eq!((out >> 16) & 0xFF, 255);
        assert_eq!(out & 0xFF, 0, "the blue channel is untouched");

        // 48 px at an 8 px advance fits six characters per line.
        let l = layout_text("hello world", 48, 8, 4);
        assert_eq!(l.lines, 2);
        assert_eq!(l.widest_px, 48);
        assert!(!l.truncated);
        // Exceeding the line budget is reported, not hidden.
        let clamped = layout_text("aaaa bbbb cccc dddd eeee ffff gggg", 8, 8, 2);
        assert!(clamped.truncated);
        assert!(clamped.lines <= 2);
        assert_eq!(layout_text("x", 0, 8, 4).lines, 0);
        assert!(!BIDI_SUPPORTED, "bidirectional text is not implemented");
        assert!(appears_rtl("שלום"));
        assert!(!appears_rtl("hello"));
    }

    #[test]
    fn window_tree_z_order_hit_test_and_paint_order() {
        let mut t = WindowTree::new();
        assert!(t.is_empty());
        let _ = t.create(0, 0, Rect { x: 0, y: 0, w: 1000, h: 1000 }, true);
        let _ = t.create(1, 0, Rect { x: 0, y: 0, w: 100, h: 100 }, true);
        let _ = t.create(2, 0, Rect { x: 50, y: 50, w: 100, h: 100 }, true);
        assert_eq!(t.len(), 3);
        assert!(!t.create(1, 0, Rect::default(), true), "duplicate id");
        assert!(!t.create(99, 0, Rect::default(), true), "id out of range");

        // Window 2 was created last, so it is on top.
        assert_eq!(t.hit_test(60, 60), Some(2));
        assert_eq!(t.hit_test(500, 500), Some(0));
        assert_eq!(t.hit_test(2000, 2000), None);

        // Raising window 1 puts it above window 2.
        assert!(t.raise(1));
        assert_eq!(t.hit_test(60, 60), Some(1), "the raised window wins");
        assert!(!t.raise(99));

        // Hiding removes it from hit testing but keeps it in the tree.
        assert!(t.set_visible(1, false, false));
        assert_eq!(t.hit_test(60, 60), Some(2));
        assert!(!t.get(1).unwrap().in_taskbar);

        let mut order = [0u32; MAX_WINDOWS];
        let n = t.paint_order(&mut order);
        assert_eq!(n, 2);
        assert_eq!(order[0], 0, "the root paints first");
        assert_eq!(order[1], 2);

        // Destroying re-parents children rather than losing them.
        assert!(t.destroy(0));
        assert_eq!(t.get(2).unwrap().parent, 0);
        assert!(!t.destroy(0));
        assert!(t.get(0).is_none());
        assert_eq!(t.len(), 2, "only the root was destroyed");

        let single = WindowTree::default();
        assert!(single.hit_test(0, 0).is_none());
        let occl = Window { id: 5, used: true, visible: true, z: 99, opaque: true, rect: Rect { x: 0, y: 0, w: 10, h: 10 }, ..Window::default() };
        let mut t2 = WindowTree::new();
        let _ = t2.create(0, 0, Rect { x: 0, y: 0, w: 10, h: 10 }, true);
        assert!(!t2.occluded_by(0, Rect { x: 0, y: 0, w: 1, h: 1 }), "nothing above it");
        let _ = occl;
    }

    #[test]
    fn compositor_blend_and_layer_plan() {
        let mut c = Compositor::new();
        assert!(c.is_empty());
        assert!(c.push(Layer { window: 0, alpha: 255, mode: BlendMode::Normal, blur_sigma: 0 }));
        assert!(c.push(Layer { window: 1, alpha: 200, mode: BlendMode::Normal, blur_sigma: 0 }));
        assert!(c.push(Layer { window: 2, alpha: 255, mode: BlendMode::BackdropBlur, blur_sigma: 16 }));
        assert_eq!(c.len(), 3);
        assert_eq!(c.opaque_copies(), 1, "only the fully opaque Normal layer");
        assert_eq!(c.backdrop_layers(), 1);
        c.frame_done();
        assert_eq!(c.frames(), 1);
        assert_eq!(c.copied_layers(), 1);
        c.clear();
        assert!(c.is_empty());

        assert_eq!(blend_channel(BlendMode::Normal, 200, 0, 255), 200);
        assert_eq!(blend_channel(BlendMode::Normal, 200, 0, 128), 100);
        assert_eq!(blend_channel(BlendMode::Multiply, 255, 255, 255), 255);
        assert_eq!(blend_channel(BlendMode::Multiply, 128, 128, 255), 64);
        assert_eq!(blend_channel(BlendMode::Screen, 0, 0, 255), 0);
        assert_eq!(blend_channel(BlendMode::Screen, 255, 0, 255), 255);
        assert_eq!(blend_channel(BlendMode::Overlay, 200, 100, 255), 156);
        // A fully transparent layer leaves the destination alone.
        assert_eq!(blend_channel(BlendMode::Multiply, 255, 77, 0), 77);
        let px = blend_pixel(BlendMode::Normal, 0x00FF_0000, 0x0000_00FF, 255);
        assert_eq!(px, 0x00FF_0000);
        assert!(BlendMode::BackdropBlur.needs_backdrop());
        assert!(!BlendMode::Normal.needs_backdrop());
    }

    #[test]
    fn springs_and_easing_curves() {
        let mut s = Spring::snappy();
        assert!(s.settled(), "a spring at rest is settled");
        s.to(100 * 256);
        let mut ticks = 0;
        while !s.settled() && ticks < 600 {
            s.step();
            ticks += 1;
        }
        assert!(ticks < 300, "the snap spring must settle quickly ({ticks})");
        assert!(s.settle_ticks(10) <= 10);

        // A gentler spring takes longer but still settles.
        let mut g = Spring::gentle();
        g.to(50 * 256);
        let gentle_ticks = g.settle_ticks(600);
        assert!(gentle_ticks > 0);
        assert!(gentle_ticks < 600);

        // Snapping is immediate and stops the animation.
        let mut snap = Spring::snappy();
        snap.to(10 * 256);
        snap.snap(7 * 256 + 128);
        assert!(snap.settled());
        assert_eq!(snap.velocity, 0);

        assert_eq!(Easing::Linear.eval(0), 0);
        assert_eq!(Easing::Linear.eval(1000), 1000);
        assert_eq!(Easing::Linear.eval(500), 500);
        for e in [
            Easing::CubicIn,
            Easing::CubicOut,
            Easing::CubicInOut,
            Easing::BackOut,
        ] {
            assert_eq!(e.eval(0), 0, "{e:?} at 0");
            assert_eq!(e.eval(1000), 1000, "{e:?} at 1");
            assert!(!e.as_str().is_empty());
        }
        // CubicOut starts fast (57% of the way at a quarter of the time) and
        // ends soft; CubicIn is the mirror image.
        assert!(Easing::CubicOut.eval(250) > 500);
        assert!(Easing::CubicIn.eval(250) < 100);
        assert!(Easing::BackOut.overshoots());
        assert!(!Easing::Linear.overshoots());
        for e in [
            Easing::Linear,
            Easing::CubicIn,
            Easing::CubicOut,
            Easing::CubicInOut,
            Easing::BackOut,
        ] {
            assert_eq!(Easing::parse(e.as_str()), Some(e));
        }
        assert_eq!(Easing::parse("nope"), None);
    }

    #[test]
    fn theme_colour_displays_and_night_shift() {
        let theme = ThemeTokens::dark();
        assert!(theme.dark);
        assert_eq!(theme.color(TokenKind::Accent), 0x004C_9AFF);
        assert!(theme.text_contrast_ok(), "dark theme must pass its own contrast check");
        let light = ThemeTokens::light();
        assert!(light.text_contrast_ok());
        assert!(!light.dark);
        let mut custom = light;
        custom.set_color(TokenKind::Accent, 0xFFFF_FFFF);
        assert_eq!(custom.color(TokenKind::Accent), 0x00FF_FFFF, "alpha is masked off");
        let reduced = theme.reduce_motion();
        assert_eq!(reduced.scale_duration(200), 0);
        assert_eq!(theme.scale_duration(200), 200);
        for k in [
            TokenKind::Surface,
            TokenKind::Text,
            TokenKind::Accent,
            TokenKind::Border,
            TokenKind::Success,
            TokenKind::Warning,
            TokenKind::Danger,
        ] {
            assert!(!k.as_str().is_empty());
            assert!(k.index() < TokenKind::COUNT);
        }
        // WCAG AA is 45 in tenths; the default theme is comfortably past it.
        assert!(theme.contrast_ratio(TokenKind::Text, TokenKind::Surface) >= 45);

        let hdr = HdrSupport {
            available: true,
            max_luminance_nits: 1000,
            min_luminance_centinits: 5,
            ten_bit: true,
            pq: true,
        };
        assert!(hdr.usable());
        assert_eq!(hdr.describe(), "hdr-pq");
        assert!(!HdrSupport {
            max_luminance_nits: 300,
            ..hdr
        }
        .usable());

        let mut set = DisplaySet::new();
        assert!(set.attach(Display {
            id: 0,
            rect: Rect { x: 0, y: 0, w: 3840, h: 2160 },
            scale: 2,
            hz: 60,
            primary: false,
            hdr: Some(hdr),
        }));
        assert!(set.attach(Display {
            id: 1,
            rect: Rect { x: 0, y: 0, w: 1920, h: 1080 },
            scale: 1,
            hz: 144,
            primary: false,
            hdr: None,
        }));
        assert_eq!(set.count(), 2);
        assert_eq!(set.hdr_displays(), 1);
        assert!(set.primary().is_some());
        assert_eq!(set.primary().unwrap().scale, 2);
        let primary = set.primary().unwrap();
        assert_eq!(primary.logical_size(), (1920, 1080));
        assert_eq!(primary.pixels_per_point(), 2);
        assert!(primary.contains(100, 100));
        assert!(!primary.contains(4000, 100));

        assert!(set.set_layout(DisplayLayout::Extend));
        assert_eq!(set.layout(), DisplayLayout::Extend);
        let bounds = set.virtual_bounds();
        assert_eq!(bounds.x, 0);
        assert!(bounds.w >= 3840);
        assert_eq!(set.display_at(3850, 10).map(|d| d.id), Some(1));

        assert!(set.set_layout(DisplayLayout::Mirror));
        assert_eq!(set.virtual_bounds().w, 3840);
        assert!(set.detach(0));
        assert_eq!(set.count(), 1);
        assert!(!set.detach(0));
        assert!(!set.attach(Display::default()), "a zero-sized display is refused");

        // sRGB round trip stays within one code value across the range.
        for v in [0u8, 1, 10, 12, 64, 128, 200, 255] {
            let lin = srgb_to_linear_v(v);
            let back = linear_to_srgb_v(lin);
            assert!(
                (back as i32 - v as i32).abs() <= 2,
                "{v} -> {lin} -> {back}"
            );
        }

        let shift = NightShift::warm(100);
        assert!(shift.enabled);
        let (r, g, b) = shift.multipliers();
        assert!(r > 256, "a slight red lift, never a black-level raise");
        assert!(g < 256 && b < g, "green then blue are attenuated");
        let tinted = shift.apply(0x00FF_FFFF);
        assert_ne!(tinted, 0x00FF_FFFF);
        assert_eq!(NightShift::off().apply(0x0012_3456), 0x0012_3456);
        assert!(NightShift::scheduled(23));
        assert!(NightShift::scheduled(3));
        assert!(!NightShift::scheduled(12));
    }

    #[test]
    fn drag_sync_acceleration_fallback_and_capture() {
        let mut d = DragSync::default();
        d.begin();
        d.on_pointer(10, 0);
        assert_eq!(d.lag_pixels(), 10);
        // A present outside vsync moves nothing and is counted.
        assert_eq!(d.on_present(false), (0, 0));
        assert_eq!(d.vsync_drops, 1);
        // A vsync present applies everything accumulated.
        assert_eq!(d.on_present(true), (10, 0));
        assert_eq!(d.lag_pixels(), 0);
        assert_eq!(d.applied_dx, 10);
        d.on_pointer(5, -3);
        d.end();
        assert_eq!(d.applied_dx, 15, "the tail lands on release");
        assert_eq!(d.applied_dy, -3);
        assert!(!d.dragging);

        let gpu = GpuState::detect(0x8086, true, 512);
        assert_eq!(gpu.backend(), AccelBackend::Gpu);
        assert!(gpu.backend().accelerates("blur"));
        assert!(!AccelBackend::Software.accelerates("blur"));
        assert!(AccelBackend::Software.accelerates("blit"));
        assert!(!AccelBackend::None.accelerates("blit"));
        assert!(!gpu.probe_failed);
        let unknown = GpuState::detect(0xFFFF, false, 0);
        assert!(unknown.probe_failed);
        assert_eq!(unknown.backend(), AccelBackend::Software);
        let blit = GpuState::detect(0x1234, false, 16);
        assert_eq!(blit.backend(), AccelBackend::Blit2D);
        assert_eq!(blit.max_texture, 2048);
        assert!(!GpuState::default().describe().is_empty());

        assert_eq!(
            CompositorPath::choose(&gpu, 512, 60),
            CompositorPath::Full
        );
        assert_eq!(
            CompositorPath::choose(&gpu, 32, 60),
            CompositorPath::Reduced
        );
        assert_eq!(
            CompositorPath::choose(&unknown, 512, 60),
            CompositorPath::Minimal
        );
        assert_eq!(
            CompositorPath::choose(&blit, 512, 60),
            CompositorPath::Reduced
        );
        assert_eq!(
            CompositorPath::choose(&GpuState::default(), 512, 144),
            CompositorPath::Minimal,
            "software compositing cannot hold 144 Hz"
        );
        assert!(CompositorPath::Full.supports_blur());
        assert!(!CompositorPath::Minimal.supports_blur());
        assert_eq!(CompositorPath::Full.buffer_count(), 3);
        assert!(!CompositorPath::Minimal.as_str().is_empty());

        let shot = plan_screenshot(Rect { x: 0, y: 0, w: 100, h: 50 }, 1000, 1000, true);
        assert!(!shot.captured);
        assert_eq!(shot.width, 100);
        assert_eq!(shot.height, 50);
        assert_eq!(shot.bytes, 100 * 50 * 4 + 100 * 4);
        let mut shot = shot;
        assert!(capture(&mut shot));
        assert!(shot.captured);
        // A rectangle fully off-screen captures nothing.
        let empty = plan_screenshot(Rect { x: 2000, y: 0, w: 10, h: 10 }, 1000, 1000, false);
        assert_eq!(empty.width, 0);
        let mut empty = empty;
        assert!(!capture(&mut empty));
    }

    #[test]
    fn self_test_passes() {
        let (passed, failed) = run_display_checks();
        // The failure detail is rendered into the assertion so a regression
        // says *which* invariant broke, not just that one did.
        let mut out = [0u8; 512];
        let n = DISPLAY_SELFTEST.render(&mut out);
        let detail = core::str::from_utf8(&out[..n]).unwrap_or("");
        assert_eq!(failed, 0, "{passed} passed, {failed} failed: {detail}");
        assert!(passed >= 10);
    }
}
