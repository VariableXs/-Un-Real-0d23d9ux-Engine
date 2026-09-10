//! F013 引导横幅艺术化 — the Variable-quality boot screen (kernel edition).
//!
//! Composition (all procedural, no assets):
//!  1. vertical gradient wash  — deep navy → near-black
//!  2. CRT scanline overlay   — every other row dimmed ~12%
//!  3. VARIX wordmark         — font8x8 scaled 4x, blue-white core
//!  4. glow halo              — wordmark redrawn at ±1px offsets in dim cyan
//!  5. tagline + firmware line
//!  6. film vignette           — corner darkening bands
//!
//! Also provides the epoch → date rendering used by the boot headline.

/// Civil-from-days algorithm (Howard Hinnant) — pure, no std.
pub fn epoch_to_utc(epoch: i64) -> (i64, u32, u32, u32, u32, u32) {
    let days = epoch.div_euclid(86_400);
    let secs = epoch.rem_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = if m <= 2 { y + 1 } else { y };
    (
        y,
        m,
        d,
        (secs / 3600) as u32,
        ((secs % 3600) / 60) as u32,
        (secs % 60) as u32,
    )
}

use crate::fb::{Color, Surface};
use crate::font;

/// Boot screen palette.
pub const BG_TOP: Color = Color::rgb(0x0B, 0x10, 0x24);
pub const BG_BOTTOM: Color = Color::rgb(0x03, 0x04, 0x08);
pub const WORDMARK: Color = Color::rgb(0xD8, 0xE6, 0xF8);
pub const GLOW: Color = Color::rgb(0x2C, 0x5A, 0x8C);
pub const TAGLINE: Color = Color::rgb(0x6E, 0x82, 0x9E);
pub const HAIRLINE: Color = Color::rgb(0x3A, 0x6A, 0xA8);

/// Vertical gradient wash + scanline overlay + vignette.
pub fn paint_backdrop(surf: &Surface) {
    let h = surf.height() as i64;
    let w = surf.width() as i64;
    for y in 0..h {
        let t = (y.max(0) * 255 / h.max(1)) as u8;
        let row_color = BG_TOP.lerp(BG_BOTTOM, t);
        // CRT scanline: odd rows dimmed.
        let row_color = if y % 2 == 1 { row_color.scale(224) } else { row_color };
        surf.hline(0, w - 1, y, row_color);
    }
    // Film vignette: darken the outermost 3% of rows/columns progressively.
    let band = (h / 32).max(4);
    for i in 0..band {
        let s = (255 - (i * 255 / band) / 3) as u8;
        let c = BG_BOTTOM.scale(s);
        surf.hline(0, w - 1, i, c);
        surf.hline(0, w - 1, h - 1 - i, c);
    }
}

/// Draw the VARIX wordmark with glow halo, tagline and firmware line.
/// Returns the y coordinate just below the composition (for the HUD).
/// Full boot banner: vector logo (F023) + wordmark block, stacked.
pub fn draw_boot_banner(surf: &Surface, firmware: &str, bootloader: &str) -> i64 {
    let (lcx, lcy, lsize) = crate::logo::metrics_for(surf);
    let top = crate::logo::draw(surf, lcx, lcy, lsize) + 12;
    draw_boot_banner_at(surf, firmware, bootloader, top)
}

/// Wordmark + tagline + meta block starting at `y0` (below an already-drawn
/// logo). Returns the y coordinate just below the composition (for the HUD).
pub fn draw_boot_banner_at(
    surf: &Surface,
    firmware: &str,
    bootloader: &str,
    y0: i64,
) -> i64 {
    let w = surf.width() as i64;
    let scale = if w >= 1024 { 4 } else { 2 };
    let word = "VARIX";
    let text_w = font::text_width_scaled(word, scale);
    let x0 = (w - text_w) / 2;

    // Glow halo: dim cyan offsets.
    for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
        font::draw_text_scaled(surf, x0 + dx * 2, y0 + dy * 2, word, GLOW, scale);
    }
    // Wordmark core.
    font::draw_text_scaled(surf, x0, y0, word, WORDMARK, scale);

    // Tagline + provenance, centered.
    let tag = "INDEPENDENT KERNEL / BOOTSTRAP";
    let tag_w = font::text_width_scaled(tag, 1);
    let ty = y0 + 8 * scale * 2 + 6;
    font::draw_text_scaled(surf, (w - tag_w) / 2, ty, tag, TAGLINE, 1);

    let meta = firmware; // e.g. "UEFI 64"
    let mut line_buf = [0u8; 96];
    let bl = bootloader.as_bytes();
    let mut n = 0usize;
    for &b in meta.as_bytes() {
        if n >= 70 {
            break;
        }
        line_buf[n] = b;
        n += 1;
    }
    if !bl.is_empty() && n + 3 < line_buf.len() {
        line_buf[n] = b' ';
        line_buf[n + 1] = b'/';
        line_buf[n + 2] = b' ';
        n += 3;
        for &b in bl {
            if n >= line_buf.len() {
                break;
            }
            line_buf[n] = b;
            n += 1;
        }
    }
    if let Ok(s) = core::str::from_utf8(&line_buf[..n]) {
        let mw = font::text_width_scaled(s, 1);
        font::draw_text_scaled(surf, (w - mw) / 2, ty + 22, s, TAGLINE, 1);
    }

    // Hairline separator under the banner block.
    let hy = ty + 44;
    let inset = w / 12;
    surf.hline(inset, w - 1 - inset, hy, HAIRLINE);
    hy + 8
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fb::PixelFormat;
    use std::vec::Vec;

    fn surface(w: u32, h: u32) -> (Surface, Vec<u8>) {
        let mut v: Vec<u8> = std::vec![0u8; (w * h * 4) as usize];
        let s = unsafe { Surface::from_raw(v.as_mut_ptr(), w, h, w * 4, PixelFormat::Bgr32) };
        (s, v)
    }

    #[test]
    fn epoch_conversion_known_values() {
        assert_eq!(epoch_to_utc(0), (1970, 1, 1, 0, 0, 0));
        // 2026-01-01 00:00:00 UTC = 1767225600
        assert_eq!(epoch_to_utc(1_767_225_600), (2026, 1, 1, 0, 0, 0));
        // 2000-02-29 12:30:45 UTC (leap day) = 951827445
        assert_eq!(epoch_to_utc(951_827_445), (2000, 2, 29, 12, 30, 45));
        // Negative pre-epoch time: 1969-12-31 23:59:59
        assert_eq!(epoch_to_utc(-1), (1969, 12, 31, 23, 59, 59));
    }

    #[test]
    fn backdrop_gradient_and_scanlines() {
        let (s, _b) = surface(64, 256);
        paint_backdrop(&s);
        // Vignette band: outermost rows are pulled toward BG_BOTTOM (the
        // strongest vignette step is the identity scale at the frame edge).
        let bottom_pack = PixelFormat::Bgr32.pack(BG_BOTTOM);
        assert_eq!(s.get_px(32, 0).unwrap(), bottom_pack);
        assert_eq!(s.get_px(32, 255).unwrap(), bottom_pack);
        // Gradient: an even row below the top vignette band stays near
        // BG_TOP — brighter than the (dark) bottom band.
        let top_area = s.get_px(32, 12).unwrap();
        let top_c = ((top_area >> 8) & 0xFF) as u16; // green channel as proxy
        let bot_c = ((bottom_pack >> 8) & 0xFF) as u16;
        assert!(top_c > bot_c, "gradient row must be brighter than the vignette");
        // Scanlines: odd row must be dimmer than the even row above it.
        let even = s.get_px(0, 100).unwrap();
        let odd = s.get_px(0, 101).unwrap();
        let even_c = ((even >> 8) & 0xFF) as u16; // green channel as proxy
        let odd_c = ((odd >> 8) & 0xFF) as u16;
        assert!(odd_c < even_c, "scanline row must be dimmer");
    }

    #[test]
    fn banner_draws_wordmark_and_returns_cursor() {
        let (s, _b) = surface(1024, 640);
        let cursor_y = draw_boot_banner(&s, "UEFI 64", "Limine 8.x");
        assert!(cursor_y > 0 && cursor_y < 640);
        // Wordmark band must contain bright pixels (blue-ish white core).
        let mut bright = 0;
        for y in (cursor_y / 3)..(cursor_y / 2) {
            for x in 300..700 {
                let p = s.get_px(x as i64, y as i64).unwrap_or(0);
                let b = ((p >> 16) & 0xFF) as u16; // red channel (BGR32: r at 16)
                let g = ((p >> 8) & 0xFF) as u16;
                if b > 120 && g > 120 {
                    bright += 1;
                }
            }
        }
        assert!(bright > 50, "wordmark not visible, bright={bright}");
    }

    #[test]
    fn banner_meta_line_renders() {
        let (s, _b) = surface(1024, 640);
        let cursor_y = draw_boot_banner(&s, "UEFI 64", "Limine");
        // The tagline/meta band sits between the wordmark (upper quarter of
        // the composition) and the returned cursor.
        let mut non_bg = 0;
        for y in (cursor_y / 2)..cursor_y {
            for x in 256..768 {
                let p = s.get_px(x as i64, y as i64).unwrap_or(0);
                let g = ((p >> 8) & 0xFF) as u16;
                if g > 90 {
                    non_bg += 1;
                }
            }
        }
        assert!(non_bg > 10, "tagline not rendered, non_bg={non_bg}");
    }
}
