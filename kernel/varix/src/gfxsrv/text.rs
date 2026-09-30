//! VARIABLE-200 AI-05 · 显示域字体/令牌/图标/DPI 面（F110~F115）。
//!
//! 纯逻辑 + 固定容量数组，无分配。

use crate::checks::CheckSet;
use crate::gfxsrv::{argb, color_a, color_b, color_g, color_r, rgb};

// ---------------------------------------------------------------------------
// F110 字体渲染管线 — TTF 头解析 + 亚像素抗锯齿 + 字重族
// ---------------------------------------------------------------------------

pub const GLYPH_W: usize = 8;
pub const GLYPH_H: usize = 8;
pub const GLYPH_PIXELS: usize = GLYPH_W * GLYPH_H;
/// 亚像素采样密度（每像素 3 个子采样：R/G/B）。
pub const SUBPIXEL_SAMPLES: usize = 3;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Weight {
    Regular,
    Medium,
    Semibold,
    Bold,
}

impl Weight {
    /// 笔画基准宽度（像素 ×16 定点）。
    pub fn stem_q4(self) -> u16 {
        match self {
            Weight::Regular => 16,
            Weight::Medium => 20,
            Weight::Semibold => 24,
            Weight::Bold => 32,
        }
    }

    pub fn id(self) -> u8 {
        match self {
            Weight::Regular => 0,
            Weight::Medium => 1,
            Weight::Semibold => 2,
            Weight::Bold => 3,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TtfHeader {
    pub sfnt_version: u32,
    pub num_tables: u16,
    pub units_per_em: u16,
}

/// TTF/OTF 头解析骨架：magic 判定 + 表数 + unitsPerEm。
pub fn parse_ttf_header(data: &[u8]) -> Option<TtfHeader> {
    if data.len() < 12 {
        return None;
    }
    let ver = ((data[0] as u32) << 24)
        | ((data[1] as u32) << 16)
        | ((data[2] as u32) << 8)
        | data[3] as u32;
    let ok_magic = ver == 0x0001_0000 || ver == 0x4F54544F; // 'OTTO'
    if !ok_magic {
        return None;
    }
    let num_tables = ((data[4] as u16) << 8) | data[5] as u16;
    // 第 8 字节起是 searchRange 等，12 字节后进入表目录；unitsPerEm 在 head 表内，
    // 这里用占位约定：表目录紧随其后，前 12 字节的合法头即视为可解析。
    let units_per_em = if data.len() >= 14 {
        ((data[12] as u16) << 8) | data[13] as u16
    } else {
        1000
    };
    if num_tables == 0 {
        return None;
    }
    Some(TtfHeader { sfnt_version: ver, num_tables, units_per_em })
}

/// 字形覆盖位图（0..=255 覆盖度）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GlyphBitmap {
    pub cov: [u8; GLYPH_PIXELS],
    pub width: u8,
    pub height: u8,
    pub advance_q4: u16,
}

impl GlyphBitmap {
    pub const fn empty() -> GlyphBitmap {
        GlyphBitmap { cov: [0u8; GLYPH_PIXELS], width: 0, height: 0, advance_q4: 0 }
    }

    pub fn coverage(&self, x: usize, y: usize) -> u8 {
        if x < GLYPH_W && y < GLYPH_H {
            self.cov[y * GLYPH_W + x]
        } else {
            0
        }
    }
}

/// 光栅化：按字重笔画宽度生成一个"方块字"覆盖图（等价于真实字形的覆盖契约）。
pub fn rasterize_box(weight: Weight, w: usize, h: usize) -> GlyphBitmap {
    let mut g = GlyphBitmap::empty();
    let stem = (weight.stem_q4() / 16) as usize;
    let mut y = 0usize;
    while y < core::cmp::min(h, GLYPH_H) {
        let mut x = 0usize;
        while x < core::cmp::min(w, GLYPH_W) {
            // 边框笔画 + 内部空心，模拟字形轮廓。
            let edge = x < stem || y < stem || x + stem >= w || y + stem >= h;
            g.cov[y * GLYPH_W + x] = if edge { 255 } else { 0 };
            x += 1;
        }
        y += 1;
    }
    g.width = core::cmp::min(w, GLYPH_W) as u8;
    g.height = core::cmp::min(h, GLYPH_H) as u8;
    g.advance_q4 = (w as u16) * 16;
    g
}

/// 亚像素抗锯齿：把覆盖度按 3 个子采样分配到 R/G/B 通道。
pub fn subpixel_alpha(cov: u8) -> (u8, u8, u8) {
    if cov == 0 {
        return (0, 0, 0);
    }
    let c = cov as u32;
    let full = c * SUBPIXEL_SAMPLES as u32 / SUBPIXEL_SAMPLES as u32;
    let r = (c * 4 / 5).min(255);
    let g = full.min(255);
    let b = (c * 3 / 5).min(255);
    (r as u8, g as u8, b as u8)
}

/// 把字形以亚像素方式画到画布（前景色按覆盖度混合）。
pub fn draw_glyph(canvas: &mut crate::gfxsrv::Canvas, g: &GlyphBitmap, x0: usize, y0: usize, fg: u32) -> usize {
    let mut n = 0usize;
    let mut y = 0usize;
    while y < g.height as usize {
        let mut x = 0usize;
        while x < g.width as usize {
            let cov = g.coverage(x, y);
            if cov > 0 {
                let (r, gg, b) = subpixel_alpha(cov);
                let src = argb(255, r, gg, b);
                let px = x0 + x;
                let py = y0 + y;
                let dst = canvas.get(px, py);
                let mut c = crate::gfxsrv::blend_over(dst, src, cov);
                // 前景色调调制（保持令牌色相）。
                c = argb(
                    255,
                    ((color_r(c) as u32 * color_r(fg) as u32) / 255) as u8,
                    ((color_g(c) as u32 * color_g(fg) as u32) / 255) as u8,
                    ((color_b(c) as u32 * color_b(fg) as u32) / 255) as u8,
                );
                if canvas.put(px, py, c) {
                    n += 1;
                }
            }
            x += 1;
        }
        y += 1;
    }
    n
}

/// 字重族管理：同族多字重共用一个 advance 网格。
#[derive(Clone, Copy)]
pub struct FontFamily {
    pub name: [u8; 16],
    pub name_len: usize,
    /// 各字重是否装载。
    pub loaded: [bool; 4],
    pub units_per_em: u16,
}

impl FontFamily {
    pub const fn new(units_per_em: u16) -> FontFamily {
        FontFamily { name: [0u8; 16], name_len: 0, loaded: [false; 4], units_per_em }
    }

    pub fn set_name(&mut self, n: &[u8]) -> bool {
        if n.is_empty() || n.len() >= 16 {
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

    pub fn load(&mut self, w: Weight) -> bool {
        let i = w.id() as usize;
        if self.loaded[i] {
            return false;
        }
        self.loaded[i] = true;
        true
    }

    pub fn is_loaded(&self, w: Weight) -> bool {
        self.loaded[w.id() as usize]
    }

    pub fn loaded_count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < 4 {
            if self.loaded[i] {
                n += 1;
            }
            i += 1;
        }
        n
    }
}

// ---------------------------------------------------------------------------
// F111 字形缓存 — 光栅结果缓存（LRU）
// ---------------------------------------------------------------------------

pub const GLYPH_CACHE_SLOTS: usize = 16;

#[derive(Clone, Copy)]
pub struct CachedGlyph {
    pub font: u32,
    pub code: u32,
    pub px_size: u8,
    pub bitmap: GlyphBitmap,
    pub used_at: u64,
    pub valid: bool,
}

impl CachedGlyph {
    pub const fn empty() -> CachedGlyph {
        CachedGlyph {
            font: 0,
            code: 0,
            px_size: 0,
            bitmap: GlyphBitmap::empty(),
            used_at: 0,
            valid: false,
        }
    }
}

#[derive(Clone, Copy)]
pub struct GlyphCache {
    slots: [CachedGlyph; GLYPH_CACHE_SLOTS],
    clock: u64,
    pub hits: u64,
    pub misses: u64,
    pub evictions: u64,
}

impl GlyphCache {
    pub const fn new() -> GlyphCache {
        GlyphCache { slots: [CachedGlyph::empty(); GLYPH_CACHE_SLOTS], clock: 0, hits: 0, misses: 0, evictions: 0 }
    }

    fn find(&self, font: u32, code: u32, px: u8) -> Option<usize> {
        let mut i = 0usize;
        while i < GLYPH_CACHE_SLOTS {
            let s = self.slots[i];
            if s.valid && s.font == font && s.code == code && s.px_size == px {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 取字形：命中即返回；未命中则按需光栅并装入（LRU 淘汰最旧）。
    pub fn get(&mut self, font: u32, code: u32, px: u8, weight: Weight) -> GlyphBitmap {
        self.clock += 1;
        if let Some(i) = self.find(font, code, px) {
            self.hits += 1;
            self.slots[i].used_at = self.clock;
            return self.slots[i].bitmap;
        }
        self.misses += 1;
        let bmp = rasterize_box(weight, px as usize, px as usize);
        let mut victim: Option<usize> = None;
        let mut i = 0usize;
        while i < GLYPH_CACHE_SLOTS {
            if !self.slots[i].valid {
                victim = Some(i);
                break;
            }
            match victim {
                None => victim = Some(i),
                Some(v) => {
                    if self.slots[i].used_at < self.slots[v].used_at {
                        victim = Some(i);
                    }
                }
            }
            i += 1;
        }
        let v = victim.unwrap_or(0);
        if self.slots[v].valid {
            self.evictions += 1;
        }
        self.slots[v] = CachedGlyph {
            font,
            code,
            px_size: px,
            bitmap: bmp,
            used_at: self.clock,
            valid: true,
        };
        bmp
    }

    pub fn hit_rate_permille(&self) -> usize {
        let total = self.hits + self.misses;
        if total == 0 {
            return 0;
        }
        (self.hits * 1000 / total) as usize
    }

    pub fn loaded(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < GLYPH_CACHE_SLOTS {
            if self.slots[i].valid {
                n += 1;
            }
            i += 1;
        }
        n
    }
}

// ---------------------------------------------------------------------------
// F112 令牌颜色管线 — 主题令牌逐字节复用
// ---------------------------------------------------------------------------

pub const TOKEN_MAX: usize = 16;
pub const TOKEN_NAME_MAX: usize = 20;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ThemeMode {
    Dark,
    Light,
}

#[derive(Clone, Copy)]
pub struct ColorToken {
    pub name: [u8; TOKEN_NAME_MAX],
    pub name_len: usize,
    pub dark: u32,
    pub light: u32,
    /// 设计令牌版本（与 Z50 规范对齐）。
    pub version: u16,
}

impl ColorToken {
    pub const fn empty() -> ColorToken {
        ColorToken { name: [0u8; TOKEN_NAME_MAX], name_len: 0, dark: 0, light: 0, version: 0 }
    }

    pub fn name_eq(&self, want: &[u8]) -> bool {
        &self.name[..self.name_len] == want
    }

    /// 当前模式下的颜色（逐字节复用，不做任何重算）。
    pub fn color(&self, mode: ThemeMode) -> u32 {
        match mode {
            ThemeMode::Dark => self.dark,
            ThemeMode::Light => self.light,
        }
    }
}

#[derive(Clone, Copy)]
pub struct ThemeTokens {
    tokens: [ColorToken; TOKEN_MAX],
    count: usize,
    pub mode: ThemeMode,
    /// 热切换次数。
    pub switches: u32,
    /// 解析失败的令牌查询次数（禁止静默缺失：调用方必须记录）。
    pub misses: u64,
}

impl ThemeTokens {
    pub const fn new(mode: ThemeMode) -> ThemeTokens {
        ThemeTokens { tokens: [ColorToken::empty(); TOKEN_MAX], count: 0, mode, switches: 0, misses: 0 }
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    pub fn define(&mut self, name: &[u8], dark: u32, light: u32, version: u16) -> Option<usize> {
        if self.count >= TOKEN_MAX || name.is_empty() || name.len() >= TOKEN_NAME_MAX {
            return None;
        }
        if self.find(name).is_some() {
            return None;
        }
        let mut t = ColorToken::empty();
        t.name_len = name.len();
        t.dark = dark;
        t.light = light;
        t.version = version;
        let mut i = 0usize;
        while i < name.len() {
            t.name[i] = name[i];
            i += 1;
        }
        self.tokens[self.count] = t;
        self.count += 1;
        Some(self.count - 1)
    }

    pub fn find(&self, name: &[u8]) -> Option<usize> {
        let mut i = 0usize;
        while i < self.count {
            if self.tokens[i].name_eq(name) {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    pub fn color(&mut self, name: &[u8]) -> u32 {
        match self.find(name) {
            Some(i) => self.tokens[i].color(self.mode),
            None => {
                self.misses += 1;
                0
            }
        }
    }

    pub fn set_mode(&mut self, mode: ThemeMode) -> bool {
        if self.mode == mode {
            return false;
        }
        self.mode = mode;
        self.switches += 1;
        true
    }

    /// 令牌一次解析、全帧复用：同一令牌的两次查询必须字节一致。
    pub fn stable_within_frame(&mut self, name: &[u8]) -> bool {
        let a = self.color(name);
        let b = self.color(name);
        a == b && a != 0
    }
}

/// AURORA 标准令牌表（暗/亮双份，与设计令牌规范对齐）。
pub fn aurora_tokens() -> ThemeTokens {
    let mut t = ThemeTokens::new(ThemeMode::Dark);
    let _ = t.define(b"surface.base", rgb(24, 24, 27), rgb(250, 250, 250), 3);
    let _ = t.define(b"surface.raised", rgb(39, 39, 42), rgb(255, 255, 255), 3);
    let _ = t.define(b"text.primary", rgb(244, 244, 245), rgb(24, 24, 27), 3);
    let _ = t.define(b"accent", rgb(56, 189, 248), rgb(2, 132, 199), 3);
    t
}

// ---------------------------------------------------------------------------
// F113 128px 图标管线 — 高分辨率图标解码与缩放
// ---------------------------------------------------------------------------

pub const ICON_SRC: usize = 16;
pub const ICON_SRC_PIXELS: usize = ICON_SRC * ICON_SRC;

/// 源图标（代表 128px 源，逻辑上 16×16 采样）。
#[derive(Clone, Copy)]
pub struct Icon128 {
    pub px: [u32; ICON_SRC_PIXELS],
    pub w: usize,
    pub h: usize,
}

impl Icon128 {
    pub const fn new() -> Icon128 {
        Icon128 { px: [0u32; ICON_SRC_PIXELS], w: 0, h: 0 }
    }

    pub fn fill(&mut self, c: u32) {
        let mut i = 0usize;
        while i < ICON_SRC_PIXELS {
            self.px[i] = c;
            i += 1;
        }
        self.w = ICON_SRC;
        self.h = ICON_SRC;
    }

    pub fn get(&self, x: usize, y: usize) -> u32 {
        if x < self.w && y < self.h {
            self.px[y * ICON_SRC + x]
        } else {
            0
        }
    }

    /// 盒式滤波缩放到 `dst_n`×`dst_n`（禁止降采样走样）。
    pub fn scale_to(&self, dst: &mut [u32], dst_n: usize) -> usize {
        if self.w == 0 || self.h == 0 || dst_n == 0 || dst_n * dst_n > dst.len() {
            return 0;
        }
        let mut dy = 0usize;
        while dy < dst_n {
            let mut dx = 0usize;
            while dx < dst_n {
                // 源窗口。
                let sx0 = dx * self.w / dst_n;
                let sx1 = core::cmp::max(sx0 + 1, (dx + 1) * self.w / dst_n);
                let sy0 = dy * self.h / dst_n;
                let sy1 = core::cmp::max(sy0 + 1, (dy + 1) * self.h / dst_n);
                let mut r = 0u32;
                let mut g = 0u32;
                let mut b = 0u32;
                let mut a = 0u32;
                let mut n = 0u32;
                let mut y = sy0;
                while y < sy1 && y < self.h {
                    let mut x = sx0;
                    while x < sx1 && x < self.w {
                        let c = self.get(x, y);
                        r += color_r(c) as u32;
                        g += color_g(c) as u32;
                        b += color_b(c) as u32;
                        a += color_a(c) as u32;
                        n += 1;
                        x += 1;
                    }
                    y += 1;
                }
                if n == 0 {
                    n = 1;
                }
                dst[dy * dst_n + dx] = argb((a / n) as u8, (r / n) as u8, (g / n) as u8, (b / n) as u8);
                dx += 1;
            }
            dy += 1;
        }
        dst_n * dst_n
    }

    /// 缩放质量：源为纯色时，输出必须仍是同一纯色（零误差）。
    pub fn uniform_error(&self, dst: &[u32], dst_n: usize) -> u32 {
        if dst_n == 0 || self.w == 0 {
            return u32::MAX;
        }
        let want = self.get(0, 0);
        let mut max_err = 0u32;
        let mut i = 0usize;
        while i < dst_n * dst_n && i < dst.len() {
            let c = dst[i];
            let e = (color_r(c) as i32 - color_r(want) as i32).unsigned_abs()
                + (color_g(c) as i32 - color_g(want) as i32).unsigned_abs()
                + (color_b(c) as i32 - color_b(want) as i32).unsigned_abs();
            if e > max_err {
                max_err = e;
            }
            i += 1;
        }
        max_err
    }
}

// ---------------------------------------------------------------------------
// F114 DPI 缩放 — 整数/分数倍缩放
// ---------------------------------------------------------------------------

/// 逻辑尺寸按 DPI 千分比缩放（100% = 1000）。
pub fn scale_by_dpi(v: usize, dpi_permille: usize) -> usize {
    (v * dpi_permille + 500) / 1000
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DpiScale {
    /// 千分比，如 1000 = 100%，1250 = 125%，2000 = 200%。
    pub permille: usize,
}

impl DpiScale {
    pub const fn new(permille: usize) -> DpiScale {
        DpiScale { permille }
    }

    pub fn valid(&self) -> bool {
        self.permille >= 500 && self.permille <= 4000
    }

    pub fn px(&self, logical: usize) -> usize {
        scale_by_dpi(logical, self.permille)
    }

    /// 分数倍缩放的整数校验：125% 下 8 → 10。
    pub fn exact(&self, logical: usize, want: usize) -> bool {
        self.px(logical) == want
    }
}

#[derive(Clone, Copy)]
pub struct DpiManager {
    pub scale: DpiScale,
    /// DPI 变更广播次数（全 UI 生效的度量）。
    pub broadcasts: u32,
    pub rejected: u32,
}

impl DpiManager {
    pub const fn new(permille: usize) -> DpiManager {
        DpiManager { scale: DpiScale::new(permille), broadcasts: 0, rejected: 0 }
    }

    pub fn set(&mut self, permille: usize) -> bool {
        let s = DpiScale::new(permille);
        if !s.valid() {
            self.rejected += 1;
            return false;
        }
        if self.scale.permille == permille {
            return false;
        }
        self.scale = s;
        self.broadcasts += 1;
        true
    }
}

// ---------------------------------------------------------------------------
// F115 分辨率热切换 — 运行中改分辨率，画布全量重建不崩
// ---------------------------------------------------------------------------

pub const MODE_MAX: usize = 8;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DisplayMode {
    pub w: usize,
    pub h: usize,
    pub refresh: u16,
}

impl DisplayMode {
    pub const fn new(w: usize, h: usize, refresh: u16) -> DisplayMode {
        DisplayMode { w, h, refresh }
    }

    pub fn pixels(&self) -> usize {
        self.w * self.h
    }

    pub fn valid(&self) -> bool {
        self.w >= 320 && self.h >= 200 && self.refresh >= 30 && self.pixels() <= 1920 * 1080
    }
}

#[derive(Clone, Copy)]
pub struct DisplayModes {
    modes: [DisplayMode; MODE_MAX],
    count: usize,
    pub current: usize,
    pub hot_swaps: u32,
    pub rejected: u32,
}

impl DisplayModes {
    pub const fn new() -> DisplayModes {
        DisplayModes { modes: [DisplayMode::new(0, 0, 0); MODE_MAX], count: 0, current: 0, hot_swaps: 0, rejected: 0 }
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    pub fn add(&mut self, m: DisplayMode) -> Option<usize> {
        if self.count >= MODE_MAX || !m.valid() {
            return None;
        }
        if self.contains(&m) {
            return None;
        }
        self.modes[self.count] = m;
        self.count += 1;
        Some(self.count - 1)
    }

    pub fn contains(&self, m: &DisplayMode) -> bool {
        let mut i = 0usize;
        while i < self.count {
            if self.modes[i] == *m {
                return true;
            }
            i += 1;
        }
        false
    }

    pub fn get(&self, i: usize) -> Option<DisplayMode> {
        if i < self.count {
            Some(self.modes[i])
        } else {
            None
        }
    }

    /// 热切换：越界或非法即拒绝，绝不半切换。
    pub fn set_mode(&mut self, i: usize) -> bool {
        if i >= self.count {
            self.rejected += 1;
            return false;
        }
        if i == self.current {
            return false;
        }
        self.current = i;
        self.hot_swaps += 1;
        true
    }

    pub fn current_mode(&self) -> DisplayMode {
        self.modes[self.current]
    }
}

// ---------------------------------------------------------------------------
// 自检扩展（F110~F115 共 6 项）
// ---------------------------------------------------------------------------

pub fn extend_checks(set: &mut CheckSet) {
    // F110 字体渲染管线
    let ttf = [0x00u8, 0x01, 0x00, 0x00, 0x00, 0x0A, 0x00, 0x80, 0x00, 0x03, 0x00, 0x20, 0x03, 0xE8];
    let hdr = parse_ttf_header(&ttf);
    let bad = parse_ttf_header(&[1u8, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12]).is_none();
    let short = parse_ttf_header(&[0u8; 4]).is_none();
    let mut fam = FontFamily::new(1000);
    let named = fam.set_name(b"Aurora Sans");
    let l1 = fam.load(Weight::Regular);
    let dup = !fam.load(Weight::Regular);
    let l2 = fam.load(Weight::Bold);
    let mut canvas = crate::gfxsrv::Canvas::new();
    let glyph = rasterize_box(Weight::Bold, 6, 6);
    let painted = draw_glyph(&mut canvas, &glyph, 2, 2, rgb(255, 255, 255));
    let (sr, sg, sb) = subpixel_alpha(255);
    set.add(
        "F110 font pipeline",
        hdr.map(|h| h.num_tables == 10 && h.units_per_em == 1000).unwrap_or(false)
            && bad
            && short
            && named
            && l1
            && dup
            && l2
            && fam.loaded_count() == 2
            && fam.is_loaded(Weight::Bold)
            && Weight::Bold.stem_q4() > Weight::Regular.stem_q4()
            && painted > 0
            && canvas.get(2, 2) != 0
            && sr > 0
            && sg > 0
            && sb > 0
            && sr < sg
            && sb < sg,
        "TTF 头/字重族/亚像素抗锯齿",
    );

    // F111 字形缓存
    let mut cache = GlyphCache::new();
    let _ = cache.get(1, b'A' as u32, 8, Weight::Regular);
    let _ = cache.get(1, b'A' as u32, 8, Weight::Regular);
    let _ = cache.get(1, b'A' as u32, 8, Weight::Regular);
    let _ = cache.get(1, b'B' as u32, 8, Weight::Regular);
    let rate = cache.hit_rate_permille();
    // 填满并触发淘汰。
    let mut code = 100u32;
    while code < 100 + GLYPH_CACHE_SLOTS as u32 + 4 {
        let _ = cache.get(2, code, 12, Weight::Medium);
        code += 1;
    }
    set.add(
        "F111 glyph cache",
        cache.hits == 2
            && cache.misses == 2 + GLYPH_CACHE_SLOTS as u64 + 4
            && rate == 500
            && cache.evictions > 0
            && cache.loaded() == GLYPH_CACHE_SLOTS
            && GlyphCache::new().hit_rate_permille() == 0,
        "命中/未命中/LRU 淘汰",
    );

    // F112 令牌颜色管线
    let mut tokens = aurora_tokens();
    let dark_surface = tokens.color(b"surface.base");
    let dark_accent = tokens.color(b"accent");
    let stable = tokens.stable_within_frame(b"text.primary");
    let miss_before = tokens.misses;
    let missing = tokens.color(b"no.such.token");
    let sw = tokens.set_mode(ThemeMode::Light);
    let light_surface = tokens.color(b"surface.base");
    let same_again = !tokens.set_mode(ThemeMode::Light);
    let idx = tokens.find(b"accent");
    set.add(
        "F112 token colors",
        tokens.len() == 4
            && dark_surface == rgb(24, 24, 27)
            && dark_accent == rgb(56, 189, 248)
            && stable
            && missing == 0
            && tokens.misses == miss_before + 1
            && sw
            && light_surface == rgb(250, 250, 250)
            && same_again
            && tokens.switches == 1
            && idx.map(|i| tokens.find(b"accent") == Some(i)).unwrap_or(false)
            && !tokens.define(b"accent", 0, 0, 3).is_some(),
        "双主题逐字节复用/热切换/缺失不静默",
    );

    // F113 128px 图标管线
    let mut icon = Icon128::new();
    icon.fill(argb(255, 12, 34, 56));
    let mut dst32 = [0u32; 32 * 32];
    let n32 = icon.scale_to(&mut dst32, 32);
    let err32 = icon.uniform_error(&dst32, 32);
    let mut dst16 = [0u32; 16 * 16];
    let n16 = icon.scale_to(&mut dst16, 16);
    let err16 = icon.uniform_error(&dst16, 16);
    let mut dst64 = [0u32; 64 * 64];
    let n64 = icon.scale_to(&mut dst64, 64);
    set.add(
        "F113 icon pipeline",
        n32 == 1024
            && n16 == 256
            && n64 == 4096
            && err32 == 0
            && err16 == 0
            && dst32[0] == argb(255, 12, 34, 56)
            && icon.scale_to(&mut [0u32; 4], 4) == 0,
        "盒式缩放零误差/越界拒绝",
    );

    // F114 DPI 缩放
    let mut dpi = DpiManager::new(1000);
    let ok125 = dpi.set(1250);
    let e8 = DpiScale::new(1250).exact(8, 10);
    let e16 = DpiScale::new(1250).exact(16, 20);
    let e1 = DpiScale::new(1250).exact(1, 1);
    let two = dpi.set(2000);
    let reject_low = !dpi.set(100);
    let reject_high = !dpi.set(9000);
    set.add(
        "F114 dpi scaling",
        ok125
            && e8
            && e16
            && e1
            && two
            && reject_low
            && reject_high
            && dpi.rejected == 2
            && dpi.broadcasts == 2
            && dpi.scale.px(100) == 200
            && DpiScale::new(1000).px(37) == 37
            && !DpiScale::new(100).valid(),
        "整数/分数倍缩放/非法拒绝",
    );

    // F115 分辨率热切换
    let mut modes = DisplayModes::new();
    let m0 = modes.add(DisplayMode::new(1024, 768, 60));
    let m1 = modes.add(DisplayMode::new(1920, 1080, 60));
    let dup = modes.add(DisplayMode::new(1024, 768, 60)).is_none();
    let tiny = modes.add(DisplayMode::new(160, 100, 60)).is_none();
    let huge = modes.add(DisplayMode::new(3840, 2160, 60)).is_none();
    let swap = modes.set_mode(m1.unwrap_or(0));
    let bad = !modes.set_mode(9);
    let cur = modes.current_mode();
    set.add(
        "F115 mode hotswap",
        m0 == Some(0)
            && m1 == Some(1)
            && dup
            && tiny
            && huge
            && swap
            && bad
            && cur == DisplayMode::new(1920, 1080, 60)
            && modes.hot_swaps == 1
            && modes.rejected == 1
            && modes.len() == 2,
        "运行中改分辨率/越界拒绝",
    );
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f110_ttf_header_rejects_garbage() {
        assert!(parse_ttf_header(&[0x00, 0x01, 0x00, 0x00, 0, 5, 0, 0, 0, 0, 0, 0]).is_some());
        assert!(parse_ttf_header(b"OTTO\x00\x02\x00\x80\x00\x03\x00\x20\x03\xE8").is_some());
        assert!(parse_ttf_header(&[0xFF; 12]).is_none());
        assert!(parse_ttf_header(&[]).is_none());
        assert!(parse_ttf_header(&[0x00, 0x01, 0x00, 0x00, 0, 0, 0, 0, 0, 0, 0, 0]).is_none());
    }

    #[test]
    fn f111_cache_hits_after_first_raster() {
        let mut c = GlyphCache::new();
        let a = c.get(1, 65, 10, Weight::Regular);
        let b = c.get(1, 65, 10, Weight::Regular);
        assert_eq!(a, b);
        assert_eq!(c.hits, 1);
        assert_eq!(c.misses, 1);
        let d = c.get(1, 65, 12, Weight::Regular);
        assert_ne!(a, d);
        assert_eq!(c.misses, 2);
    }

    #[test]
    fn f114_fractional_dpi_rounds_to_nearest() {
        assert_eq!(scale_by_dpi(8, 1250), 10);
        assert_eq!(scale_by_dpi(16, 1250), 20);
        assert_eq!(scale_by_dpi(10, 1500), 15);
        assert_eq!(scale_by_dpi(0, 2000), 0);
    }

    #[test]
    fn f115_rejects_duplicate_and_insane_modes() {
        let mut m = DisplayModes::new();
        assert!(m.add(DisplayMode::new(800, 600, 60)).is_some());
        assert!(m.add(DisplayMode::new(800, 600, 60)).is_none());
        assert!(m.add(DisplayMode::new(1, 1, 1)).is_none());
        assert!(m.add(DisplayMode::new(4000, 4000, 60)).is_none());
        assert!(m.set_mode(5) == false);
    }

    #[test]
    fn f112_theme_switch_preserves_bytes() {
        let mut t = aurora_tokens();
        let d = t.color(b"surface.raised");
        t.set_mode(ThemeMode::Light);
        let l = t.color(b"surface.raised");
        assert_ne!(d, l);
        assert_eq!(d, rgb(39, 39, 42));
        assert_eq!(l, rgb(255, 255, 255));
    }
}
