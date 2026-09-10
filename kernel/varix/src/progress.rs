//! F014 引导进度真实性 — boot progress bar that maps 1:1 to actually
//! completed boot stages (no fake animation): the bar only advances when a
//! stage of the boot timeline finishes. Game-HUD style per the Variable
//! boot-line spec: full banner width, 24px segment ticks, right-aligned
//! percentage, sheen sweep across the filled span.

use crate::fb::{Color, Surface};
use crate::font;

/// Bar geometry (px).
pub const BAR_H: i64 = 10;
/// Segment tick spacing (px) — the 24px rhythm of the boot line spec.
pub const TICK_SPACING: i64 = 24;
/// Ticks 1px wide, inset from top/bottom.
const TICK_INSET: i64 = 2;

/// Progress palette.
pub const TRACK: Color = Color::rgb(0x14, 0x1C, 0x30);
pub const TRACK_EDGE: Color = Color::rgb(0x2A, 0x3A, 0x58);
pub const FILL: Color = Color::rgb(0x3A, 0x6A, 0xA8);
pub const FILL_BRIGHT: Color = Color::rgb(0x6E, 0xA8, 0xDC);
pub const SHEEN: Color = Color::rgb(0xC8, 0xE2, 0xFF);
pub const PCT_TEXT: Color = Color::rgb(0x9E, 0xB2, 0xCC);

/// Map `completed` of `total` stages to a percentage 0..=100.
/// Never shows 100% until `total` stages completed — the truth rule of F014.
pub fn percent(completed: usize, total: usize) -> u32 {
    if total == 0 || completed == 0 {
        return 0;
    }
    if completed >= total {
        return 100;
    }
    ((completed * 100) / total) as u32
}

/// Draw the progress bar into `surf` at (x, y) with width `w`.
/// `completed`/`total` drive the fill; the percentage is drawn to the right
/// of the bar (needs ~5 glyph cells).
pub fn draw(surf: &Surface, x: i64, y: i64, w: i64, completed: usize, total: usize) {
    if w <= 0 {
        return;
    }
    // Track.
    surf.fill_rect(x, y, w, BAR_H, TRACK);
    surf.rect_outline(x, y, w, BAR_H, TRACK_EDGE);

    // Fill span: proportional to completed/total.
    let pct = percent(completed, total);
    let fill_w = (w as u64 * pct as u64 / 100) as i64;
    if fill_w > 0 {
        surf.fill_rect(x + 1, y + 1, fill_w.min(w - 2), BAR_H - 2, FILL);
        // Top highlight line: brighter fill edge.
        surf.hline(x + 1, x + fill_w, y + 1, FILL_BRIGHT);
        // Sheen sweep: a 6px bright band just inside the leading edge.
        let sweep_x = x + 1 + fill_w;
        let s0 = sweep_x - 6;
        for i in 0..6i64 {
            let alpha = (i + 1) * 255 / 6; // brighter towards the edge
            let c = FILL.lerp(SHEEN, alpha as u8);
            let px = s0 + i;
            if px >= x + 1 && px < x + w - 1 {
                surf.vline(px, y + 2, y + BAR_H - 3, c);
            }
        }
    }

    // Segment ticks: every TICK_SPACING px, top and bottom inset bands.
    let mut tx = x + TICK_SPACING;
    while tx < x + w - 1 {
        surf.vline(tx, y + TICK_INSET, y + TICK_INSET, TRACK_EDGE);
        surf.vline(tx, y + BAR_H - 1 - TICK_INSET, y + BAR_H - 1 - TICK_INSET, TRACK_EDGE);
        tx += TICK_SPACING;
    }

    // Percentage text right of the bar, baseline-aligned with the bar.
    let mut num = [0u8; 4];
    let mut n = 0usize;
    let mut v = pct;
    if v == 0 {
        num[0] = b'0';
        n = 1;
    } else {
        while v > 0 && n < 3 {
            num[n] = b'0' + (v % 10) as u8;
            v /= 10;
            n += 1;
        }
    }
    let mut pct_buf = [0u8; 4];
    let mut w2 = 0usize;
    while w2 < n {
        pct_buf[w2] = num[n - 1 - w2];
        w2 += 1;
    }
    pct_buf[w2] = b'%';
    let text_len = w2 + 1;
    if let Ok(s) = core::str::from_utf8(&pct_buf[..text_len]) {
        let ty = y + (BAR_H - 16) / 2; // vertically center on 8px glyphs
        font::draw_text_scaled(surf, x + w + 8, ty, s, PCT_TEXT, 1);
    }
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
    fn percent_mapping_is_honest() {
        assert_eq!(percent(0, 13), 0);
        assert_eq!(percent(1, 13), 7); // floor
        assert_eq!(percent(12, 13), 92);
        assert_eq!(percent(13, 13), 100);
        assert_eq!(percent(20, 13), 100); // saturated
        // degenerate totals never fake progress
        assert_eq!(percent(1, 0), 0);
    }

    #[test]
    fn bar_draws_fill_and_track() {
        let (s, _b) = surface(640, 64);
        let x = 80;
        let y = 24;
        let w = 480i64;
        draw(&s, x, y, w, 0, 12);
        // empty: only the track, no fill color anywhere.
        let fill = PixelFormat::Bgr32.pack(FILL);
        let mut found_fill = false;
        for px in x..x + w {
            if s.get_px(px, y + 5) == Some(fill) {
                found_fill = true;
            }
        }
        assert!(!found_fill, "empty progress must not paint fill");

        draw(&s, x, y, w, 6, 12); // 50%
        // x+120 sits mid-fill, clear of the 6px sheen band hugging the
        // leading edge (x+235..x+240).
        let half = s.get_px(x + 120, y + 5).unwrap();
        let beyond = s.get_px(x + 400, y + 5).unwrap();
        assert_eq!(half, fill);
        assert_ne!(beyond, fill);
        assert_eq!(beyond, PixelFormat::Bgr32.pack(TRACK));
    }

    #[test]
    fn full_bar_fills_completely() {
        let (s, _b) = surface(640, 64);
        draw(&s, 80, 24, 480, 12, 12);
        let fill = PixelFormat::Bgr32.pack(FILL);
        // Interior columns well inside the sheen band at the leading edge
        // (x+475..x+478) must be pure fill.
        assert_eq!(s.get_px(80 + 400, 24 + 5), Some(fill));
        assert_eq!(s.get_px(80 + 474, 24 + 5), Some(fill));
        // The sheen band itself sits just inside the leading edge — it must
        // be neither track nor background.
        assert_ne!(s.get_px(80 + 478, 24 + 5), Some(PixelFormat::Bgr32.pack(TRACK)));
    }

    #[test]
    fn ticks_appear_every_24_px() {
        let (s, _b) = surface(640, 64);
        let x = 80;
        let y = 24;
        draw(&s, x, y, 480, 0, 12);
        let edge = PixelFormat::Bgr32.pack(TRACK_EDGE);
        // First tick at x + 24 (top inset row).
        assert_eq!(s.get_px(x + 24, y + TICK_INSET), Some(edge));
        // ...and the next one at x + 48.
        assert_eq!(s.get_px(x + 48, y + TICK_INSET), Some(edge));
        // No tick before the first.
        assert_ne!(s.get_px(x + 12, y + TICK_INSET), Some(edge));
    }

    #[test]
    fn zero_width_bar_is_noop() {
        let (s, _b) = surface(64, 32);
        draw(&s, 8, 8, 0, 5, 10);
        // no pixels written, no panic
        assert_eq!(s.get_px(8, 8), Some(0));
    }
}
