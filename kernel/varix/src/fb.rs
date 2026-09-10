//! F003 GOP 帧缓冲初始化 — resolution / pixel format / stride handling.
//! F004 像素格式抽象 — RGB/BGR unified drawing layer.
//!
//! `Surface` is the single drawing primitive of the boot phase: a raw
//! framebuffer region plus geometry and pixel format. All higher layers
//! (console, banner, logo, progress) draw through it, which keeps the
//! pixel-format logic in exactly one place and makes every renderer
//! testable on the host against a `Vec`-backed surface.

/// 24-bit RGB color value used across the boot renderers.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Color {
    pub const fn rgb(r: u8, g: u8, b: u8) -> Color {
        Color { r, g, b }
    }

    /// Linear interpolation between two colors (`t` in 0..=255).
    pub const fn lerp(self, other: Color, t: u8) -> Color {
        let t = t as u32;
        let inv = 255 - t;
        Color {
            r: ((self.r as u32 * inv + other.r as u32 * t) / 255) as u8,
            g: ((self.g as u32 * inv + other.g as u32 * t) / 255) as u8,
            b: ((self.b as u32 * inv + other.b as u32 * t) / 255) as u8,
        }
    }

    /// Uniform brightness scale (`s` in 0..=255, 255 = unchanged).
    pub const fn scale(self, s: u8) -> Color {
        Color {
            r: ((self.r as u32 * s as u32) / 255) as u8,
            g: ((self.g as u32 * s as u32) / 255) as u8,
            b: ((self.b as u32 * s as u32) / 255) as u8,
        }
    }

    /// Perceptual luminance in 0..=255.
    pub const fn luminance(self) -> u8 {
        ((self.r as u32 * 299 + self.g as u32 * 587 + self.b as u32 * 114) / 1000) as u8
    }
}

/// Pixel layouts the boot phase knows how to drive (F004).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PixelFormat {
    /// 32bpp, byte order in memory: R G B x (red mask shift 24)
    Rgb32,
    /// 32bpp, byte order in memory: B G R x (blue mask shift 0) — QEMU/OVMF default
    Bgr32,
    /// 24bpp, byte order in memory: R G B
    Rgb24,
    /// 24bpp, byte order in memory: B G R
    Bgr24,
}

impl PixelFormat {
    /// Map Limine GOP mask metadata onto a known layout.
    pub fn from_masks(
        bpp: u16,
        red_size: u8,
        red_shift: u8,
        green_size: u8,
        green_shift: u8,
        blue_size: u8,
        blue_shift: u8,
    ) -> Option<PixelFormat> {
        if red_size != 8 || green_size != 8 || blue_size != 8 {
            return None; // non-8-bit channels unsupported at boot
        }
        match (bpp, red_shift, green_shift, blue_shift) {
            (32, 24, 16, 8) => Some(PixelFormat::Rgb32),
            (32, 16, 8, 0) => Some(PixelFormat::Bgr32),
            (24, 24, 16, 8) => Some(PixelFormat::Rgb24),
            (24, 16, 8, 0) => Some(PixelFormat::Bgr24),
            _ => None,
        }
    }

    pub const fn bytes_per_pixel(self) -> u32 {
        match self {
            PixelFormat::Rgb32 | PixelFormat::Bgr32 => 4,
            PixelFormat::Rgb24 | PixelFormat::Bgr24 => 3,
        }
    }

    /// Inverse of `pack` — read a packed pixel back into a `Color` (F004).
    /// Needed by any layer that alpha-blends against what is already on
    /// screen (e.g. the anti-aliased logo edge).
    pub const fn unpack(self, v: u32) -> Color {
        match self {
            PixelFormat::Rgb32 => Color::rgb((v >> 24) as u8, (v >> 16) as u8, (v >> 8) as u8),
            PixelFormat::Bgr32 => Color::rgb(v as u8, (v >> 8) as u8, (v >> 16) as u8),
            PixelFormat::Rgb24 | PixelFormat::Bgr24 => {
                Color::rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
            }
        }
    }

    /// Pack a color into the native u32 of this format (F004 unified layer).
    pub const fn pack(self, c: Color) -> u32 {
        match self {
            PixelFormat::Rgb32 => (c.r as u32) << 24 | (c.g as u32) << 16 | (c.b as u32) << 8,
            PixelFormat::Bgr32 => (c.b as u32) << 16 | (c.g as u32) << 8 | (c.r as u32),
            PixelFormat::Rgb24 | PixelFormat::Bgr24 => {
                (c.r as u32) << 16 | (c.g as u32) << 8 | (c.b as u32)
            }
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FbError {
    Missing,
    UnsupportedBpp(u16),
    UnsupportedMasks,
    InvalidGeometry,
}

impl FbError {
    pub fn as_str(self) -> &'static str {
        match self {
            FbError::Missing => "framebuffer request missing",
            FbError::UnsupportedBpp(_) => "unsupported bpp",
            FbError::UnsupportedMasks => "unsupported pixel masks",
            FbError::InvalidGeometry => "invalid framebuffer geometry",
        }
    }
}

/// A raw framebuffer drawing surface (F003).
///
/// Safety: `base` must point to `height * stride` writable bytes for the
/// lifetime of the surface. Host tests point it at a stack array; on target
/// it is the GOP framebuffer mapped by the bootloader.
#[derive(Clone, Copy)]
pub struct Surface {
    base: *mut u8,
    width: u32,
    height: u32,
    stride: u32,
    fmt: PixelFormat,
}

impl Surface {
    /// Wrap an already-initialized Limine framebuffer (F003).
    pub fn from_limine(fb: &crate::limine::Framebuffer) -> Result<Surface, FbError> {
        let fmt = PixelFormat::from_masks(
            fb.bpp,
            fb.red_mask_size,
            fb.red_mask_shift,
            fb.green_mask_size,
            fb.green_mask_shift,
            fb.blue_mask_size,
            fb.blue_mask_shift,
        )
        .ok_or(FbError::UnsupportedMasks)?;
        if fb.memory_model != 1 {
            // 1 = RGB memory model per Limine/GOP; anything else at 8bpc is
            // already rejected by the mask matcher above.
            return Err(FbError::UnsupportedMasks);
        }
        if fb.width == 0 || fb.height == 0 || fb.width > 16384 || fb.height > 16384 {
            return Err(FbError::InvalidGeometry);
        }
        let min_stride = fb.width as u64 * fmt.bytes_per_pixel() as u64;
        if (fb.pitch as u64) < min_stride {
            return Err(FbError::InvalidGeometry);
        }
        Ok(Surface {
            base: fb.address,
            width: fb.width as u32,
            height: fb.height as u32,
            stride: fb.pitch as u32,
            fmt,
        })
    }

    /// # Safety
    /// `base` must remain valid and writable for `height * stride` bytes.
    pub unsafe fn from_raw(
        base: *mut u8,
        width: u32,
        height: u32,
        stride: u32,
        fmt: PixelFormat,
    ) -> Surface {
        Surface { base, width, height, stride, fmt }
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn stride(&self) -> u32 {
        self.stride
    }

    pub fn format(&self) -> PixelFormat {
        self.fmt
    }

    /// Write one pixel. Out-of-bounds coordinates are ignored (drawing
    /// layers stay branchy-free and OOB-safe).
    pub fn set_px(&self, x: i64, y: i64, c: Color) {
        if x < 0 || y < 0 || x >= self.width as i64 || y >= self.height as i64 {
            return;
        }
        let off = y as usize * self.stride as usize + x as usize * self.fmt.bytes_per_pixel() as usize;
        unsafe {
            match self.fmt {
                PixelFormat::Rgb32 | PixelFormat::Bgr32 => {
                    let v = self.fmt.pack(c);
                    core::ptr::write_unaligned(self.base.add(off) as *mut u32, v);
                }
                PixelFormat::Rgb24 => {
                    let p = self.base.add(off);
                    *p = c.r;
                    *p.add(1) = c.g;
                    *p.add(2) = c.b;
                }
                PixelFormat::Bgr24 => {
                    let p = self.base.add(off);
                    *p = c.b;
                    *p.add(1) = c.g;
                    *p.add(2) = c.r;
                }
            }
        }
    }

    /// Read one packed pixel back (verification / tests / debugging).
    pub fn get_px(&self, x: i64, y: i64) -> Option<u32> {
        if x < 0 || y < 0 || x >= self.width as i64 || y >= self.height as i64 {
            return None;
        }
        let off = y as usize * self.stride as usize + x as usize * self.fmt.bytes_per_pixel() as usize;
        unsafe {
            Some(match self.fmt {
                PixelFormat::Rgb32 | PixelFormat::Bgr32 => {
                    core::ptr::read_unaligned(self.base.add(off) as *const u32)
                }
                PixelFormat::Rgb24 | PixelFormat::Bgr24 => {
                    let p = self.base.add(off);
                    ((*p) as u32) << 16 | ((*p.add(1)) as u32) << 8 | (*p.add(2)) as u32
                }
            })
        }
    }

    pub fn fill(&self, c: Color) {
        self.fill_rect(0, 0, self.width as i64, self.height as i64, c);
    }

    pub fn fill_rect(&self, x0: i64, y0: i64, w: i64, h: i64, c: Color) {
        for y in y0..y0 + h {
            for x in x0..x0 + w {
                self.set_px(x, y, c);
            }
        }
    }

    /// Horizontal span fill (fast path for gradients / scanline effects).
    pub fn hline(&self, x0: i64, x1_inclusive: i64, y: i64, c: Color) {
        for x in x0..=x1_inclusive {
            self.set_px(x, y, c);
        }
    }

    /// Vertical span fill.
    pub fn vline(&self, x: i64, y0: i64, y1_inclusive: i64, c: Color) {
        for y in y0..=y1_inclusive {
            self.set_px(x, y, c);
        }
    }

    /// Move `rows` scanlines starting at `src_y` up/down to `dst_y`
    /// (memmove semantics; used by the console scroller).
    pub fn copy_rows(&self, dst_y: i64, src_y: i64, rows: i64) {
        if rows <= 0 || dst_y == src_y {
            return;
        }
        let bytes_per_row = self.width as usize * self.fmt.bytes_per_pixel() as usize;
        // Rows within the surface; clip.
        let max_rows = (self.height as i64 - dst_y.max(src_y)).max(0);
        let rows = rows.min(max_rows);
        if rows <= 0 {
            return;
        }
        unsafe {
            for i in 0..rows {
                let d = self.base.add(((dst_y + i) as usize) * self.stride as usize);
                let s = self.base.add(((src_y + i) as usize) * self.stride as usize);
                core::ptr::copy(s, d, bytes_per_row);
            }
        }
    }

    /// 1-pixel rectangle outline.
    pub fn rect_outline(&self, x0: i64, y0: i64, w: i64, h: i64, c: Color) {
        if w <= 0 || h <= 0 {
            return;
        }
        self.hline(x0, x0 + w - 1, y0, c);
        self.hline(x0, x0 + w - 1, y0 + h - 1, c);
        self.vline(x0, y0, y0 + h - 1, c);
        self.vline(x0 + w - 1, y0, y0 + h - 1, c);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::vec::Vec;

    fn test_surface(w: u32, h: u32, fmt: PixelFormat) -> (Surface, Vec<u8>) {
        let bpp = fmt.bytes_per_pixel();
        let stride = w * bpp;
        let mut v: Vec<u8> = std::vec![0u8; (h * stride) as usize];
        let s = unsafe { Surface::from_raw(v.as_mut_ptr(), w, h, stride, fmt) };
        (s, v)
    }

    #[test]
    fn format_packing() {
        let c = Color::rgb(0x11, 0x22, 0x33);
        assert_eq!(PixelFormat::Rgb32.pack(c), 0x11223300);
        assert_eq!(PixelFormat::Bgr32.pack(c), 0x332211);
    }

    #[test]
    fn format_detection_from_masks() {
        // RGB32: red at shift 24
        assert_eq!(
            PixelFormat::from_masks(32, 8, 24, 8, 16, 8, 8),
            Some(PixelFormat::Rgb32)
        );
        // BGR32: blue at shift 0 — the QEMU/OVMF layout
        assert_eq!(
            PixelFormat::from_masks(32, 8, 16, 8, 8, 8, 0),
            Some(PixelFormat::Bgr32)
        );
        assert_eq!(PixelFormat::from_masks(24, 8, 24, 8, 16, 8, 8), Some(PixelFormat::Rgb24));
        assert_eq!(PixelFormat::from_masks(24, 8, 16, 8, 8, 8, 0), Some(PixelFormat::Bgr24));
        // 5:6:5 layouts are rejected at boot
        assert_eq!(PixelFormat::from_masks(16, 5, 11, 6, 5, 5, 0), None);
        // weird shift orders rejected
        assert_eq!(PixelFormat::from_masks(32, 8, 8, 8, 16, 8, 24), None);
    }

    #[test]
    fn set_get_pixel_bgr32() {
        let (s, _backing) = test_surface(8, 4, PixelFormat::Bgr32);
        s.set_px(3, 2, Color::rgb(0xAA, 0xBB, 0xCC));
        assert_eq!(s.get_px(3, 2), Some(0xCCBBAA));
    }

    #[test]
    fn set_get_pixel_rgb24() {
        let (s, _backing) = test_surface(8, 4, PixelFormat::Rgb24);
        s.set_px(1, 1, Color::rgb(0xAA, 0xBB, 0xCC));
        assert_eq!(s.get_px(1, 1), Some(0xAABBCC));
    }

    #[test]
    fn out_of_bounds_is_ignored() {
        let (s, _b) = test_surface(8, 4, PixelFormat::Bgr32);
        s.set_px(-1, 0, Color::rgb(255, 255, 255));
        s.set_px(8, 0, Color::rgb(255, 255, 255));
        s.set_px(0, 4, Color::rgb(255, 255, 255));
        assert_eq!(s.get_px(-1, 0), None);
        assert_eq!(s.get_px(8, 0), None);
        assert_eq!(s.get_px(0, 4), None);
    }

    #[test]
    fn fill_and_rect() {
        let (s, _b) = test_surface(16, 8, PixelFormat::Bgr32);
        s.fill(Color::rgb(1, 2, 3));
        for y in 0..8 {
            for x in 0..16 {
                assert_eq!(s.get_px(x, y), Some(0x030201));
            }
        }
        s.fill_rect(4, 2, 2, 2, Color::rgb(9, 9, 9));
        assert_eq!(s.get_px(4, 2), Some(0x090909));
        assert_eq!(s.get_px(3, 2), Some(0x030201)); // outside the rect
        assert_eq!(s.get_px(6, 2), Some(0x030201));
    }

    #[test]
    fn copy_rows_scrolls_up() {
        let (s, _b) = test_surface(4, 4, PixelFormat::Bgr32);
        // row 2 is white, row 0 is black
        s.hline(0, 3, 2, Color::rgb(255, 255, 255));
        s.copy_rows(0, 1, 3); // shift rows 1..4 up to 0..3
        assert_eq!(s.get_px(0, 1), Some(0xFFFFFF));
        assert_eq!(s.get_px(0, 0), Some(0));
    }

    #[test]
    fn color_math() {
        let black = Color::rgb(0, 0, 0);
        let white = Color::rgb(255, 255, 255);
        assert_eq!(black.lerp(white, 128), Color::rgb(128, 128, 128));
        assert_eq!(white.scale(128), Color::rgb(128, 128, 128));
        assert_eq!(white.lerp(black, 0), white);
        assert_eq!(Color::rgb(255, 255, 255).luminance(), 255);
        assert_eq!(Color::rgb(0, 255, 0).luminance(), 149);
    }

    #[test]
    fn from_limine_rejects_bad_geometry() {
        let mut fb: crate::limine::Framebuffer = unsafe { core::mem::zeroed() };
        fb.address = core::ptr::null_mut();
        // BGR32 layout
        fb.bpp = 32;
        fb.memory_model = 1;
        fb.red_mask_size = 8;
        fb.red_mask_shift = 16;
        fb.green_mask_size = 8;
        fb.green_mask_shift = 8;
        fb.blue_mask_size = 8;
        fb.blue_mask_shift = 0;
        fb.width = 640;
        fb.height = 400;
        fb.pitch = 640 * 4;
        assert!(Surface::from_limine(&fb).is_ok());

        // pitch too small
        fb.pitch = 639 * 4;
        assert!(matches!(
            Surface::from_limine(&fb),
            Err(FbError::InvalidGeometry)
        ));

        // unsupported masks (565)
        fb.pitch = 640 * 4;
        fb.red_mask_size = 5;
        fb.red_mask_shift = 11;
        fb.green_mask_size = 6;
        fb.green_mask_shift = 5;
        fb.blue_mask_size = 5;
        fb.blue_mask_shift = 0;
        assert!(matches!(
            Surface::from_limine(&fb),
            Err(FbError::UnsupportedMasks)
        ));
    }
}
