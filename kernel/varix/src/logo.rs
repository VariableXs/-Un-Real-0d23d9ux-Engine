//! F023 图形 Logo 层 — high-resolution boot mark.
//!
//! The mark is *not* a bitmap: it is a signed-distance field evaluated per
//! pixel, so it stays crisp at any resolution the firmware hands us (720p →
//! 4K) with no asset and no upscaling. Anti-aliasing comes from supersampling
//! the coverage, and the edge is alpha-blended against the backdrop instead of
//! being stamped over it — no downsampling, no quality loss.
//!
//! Geometry (normalized, y grows downward, mark fits inside [-1, 1]):
//!   * an outer ring of radius `RING_R` with a gap cut at the top, and
//!   * an inner "V" stroke pair — the Varix wordmark reduced to two strokes.

use crate::fb::{Color, Surface};

/// Top of the mark's vertical gradient (near-white).
pub const MARK_TOP: Color = Color::rgb(0xE6, 0xF2, 0xFF);
/// Bottom of the gradient (deep cyan-blue).
pub const MARK_BOTTOM: Color = Color::rgb(0x3E, 0x7E, 0xC8);

const RING_R: f32 = 0.80;
const RING_T: f32 = 0.085;
/// Cosine of the ring gap half-angle around "up" (~26°).
const GAP_COS: f32 = 0.90;

const V_LEFT: (f32, f32) = (-0.30, -0.30);
const V_BOTTOM: (f32, f32) = (0.00, 0.34);
const V_RIGHT: (f32, f32) = (0.30, -0.30);
const V_T: f32 = 0.085;

/// sqrt without linking libm — `f32::sqrt` is std-only, and the kernel has no
/// math library. Bit-halving seed plus Newton refinement; exact enough for
/// distance fields (see `tests::sqrt_matches_reference`).
fn fsqrt(v: f32) -> f32 {
    if !(v > 0.0) {
        return 0.0;
    }
    // Exponent halving gives a seed within ~2x of the true root.
    let mut x = f32::from_bits((v.to_bits() >> 1) + 0x1FC0_0000);
    if !(x > 0.0) {
        x = 1.0;
    }
    let mut i = 0;
    while i < 4 {
        x = 0.5 * (x + v / x);
        i += 1;
    }
    x
}

fn clamp01(v: f32) -> f32 {
    if v < 0.0 {
        0.0
    } else if v > 1.0 {
        1.0
    } else {
        v
    }
}

/// Hermite ramp from 0 (below `e0`) to 1 (above `e1`).
fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = if e1 == e0 {
        0.0
    } else {
        clamp01((x - e0) / (e1 - e0))
    };
    t * t * (3.0 - 2.0 * t)
}

/// Distance to the segment `a`→`b`.
fn sd_segment(px: f32, py: f32, ax: f32, ay: f32, bx: f32, by: f32) -> f32 {
    let pax = px - ax;
    let pay = py - ay;
    let bax = bx - ax;
    let bay = by - ay;
    let denom = bax * bax + bay * bay;
    let mut h = if denom > 0.0 {
        (pax * bax + pay * bay) / denom
    } else {
        0.0
    };
    h = clamp01(h);
    let dx = pax - bax * h;
    let dy = pay - bay * h;
    fsqrt(dx * dx + dy * dy)
}

/// Signed distance to the Varix mark: negative inside, positive outside.
pub fn mark_sdf(px: f32, py: f32) -> f32 {
    let len = fsqrt(px * px + py * py);
    let ring = if len >= RING_R {
        len - RING_R
    } else {
        RING_R - len
    } - RING_T;
    let stroke = {
        let a = sd_segment(px, py, V_LEFT.0, V_LEFT.1, V_BOTTOM.0, V_BOTTOM.1) - V_T;
        let b = sd_segment(px, py, V_BOTTOM.0, V_BOTTOM.1, V_RIGHT.0, V_RIGHT.1) - V_T;
        if a < b {
            a
        } else {
            b
        }
    };
    if ring < stroke {
        ring
    } else {
        stroke
    }
}

/// Coverage of one sample point: 1 inside the mark, 0 outside, ramped across
/// one pixel (`aa`) so edges never stair-step.
fn coverage_at(fx: f32, fy: f32, aa: f32) -> f32 {
    let d = mark_sdf(fx, fy);
    let mut cov = 0.5 - d / aa;
    cov = clamp01(cov);
    if cov <= 0.0 {
        return 0.0;
    }
    // Cut the top gap out of the ring. `upness` is cos(angle to "up"); the
    // softening width is one pixel expressed in cosine space, so the gap
    // edges are anti-aliased at the same rate as the geometry itself.
    let len = fsqrt(fx * fx + fy * fy);
    if len > 1e-6 {
        let upness = -fy / len;
        let soft = aa / (if len < 0.2 { 0.2 } else { len });
        let in_gap = smoothstep(GAP_COS - soft, GAP_COS + soft, upness);
        cov *= 1.0 - in_gap;
    }
    cov
}

/// Boot logo placement for a surface: `(center_x, center_y, size)`.
///
/// Size tracks the short edge (14%), clamped so it neither disappears on tiny
/// panels nor dominates a 4K frame.
pub fn metrics_for(surf: &Surface) -> (i64, i64, i64) {
    let w = surf.width() as i64;
    let h = surf.height() as i64;
    let short = if w < h { w } else { h };
    let mut size = short * 14 / 100;
    if size < 40 {
        size = 40;
    }
    if size > 320 {
        size = 320;
    }
    let cx = w / 2;
    let cy = (h / 12).max(8) + size / 2;
    (cx, cy, size)
}

/// Rasterize the mark centered at `(cx, cy)` with side length `size`.
/// Returns the y coordinate just below the mark (for layout stacking).
///
/// Every write is clipped to the surface, so an oversized or off-center mark
/// degrades to a partial draw rather than a fault.
pub fn draw(surf: &Surface, cx: i64, cy: i64, size: i64) -> i64 {
    if size <= 1 {
        return cy;
    }
    let half = size / 2;
    let half_f = (size as f32) * 0.5;
    let aa = 1.0 / half_f;
    // Large marks drop to 2x2 supersampling: same perceived edge quality at
    // high DPI, a third less work. Keeps the boot frame inside its budget.
    let steps = if size > 160 { 2 } else { 3 };
    let inv_steps = 1.0 / (steps as f32);
    let total = (steps * steps) as f32;

    let fmt = surf.format();
    for dy in -half..=half {
        for dx in -half..=half {
            let mut cov = 0.0f32;
            let mut sy = 0;
            while sy < steps {
                let mut sx = 0;
                while sx < steps {
                    let fx = (dx as f32 + (sx as f32 + 0.5) * inv_steps - 0.5) / half_f;
                    let fy = (dy as f32 + (sy as f32 + 0.5) * inv_steps - 0.5) / half_f;
                    cov += coverage_at(fx, fy, aa);
                    sx += 1;
                }
                sy += 1;
            }
            cov /= total;
            if cov <= 0.002 {
                continue;
            }
            let x = cx + dx;
            let y = cy + dy;
            if x < 0 || y < 0 || x >= surf.width() as i64 || y >= surf.height() as i64 {
                continue;
            }
            // Vertical gradient across the mark.
            let t = clamp01((dy as f32 / half_f + 1.0) * 0.5);
            let col = MARK_TOP.lerp(MARK_BOTTOM, (t * 255.0) as u8);
            // Blend against the backdrop instead of stamping: the edge keeps
            // the scanline/vignette detail underneath.
            let dst = match surf.get_px(x, y) {
                Some(p) => fmt.unpack(p),
                None => continue,
            };
            surf.set_px(x, y, dst.lerp(col, (cov * 255.0) as u8));
        }
    }
    cy + half
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
    fn sqrt_matches_reference() {
        // Bit-halving seed + 4 Newton steps is within 1e-5 relative over the
        // whole range the distance field can produce.
        let mut worst = 0.0f32;
        let mut x = 0.001f32;
        while x < 12.0 {
            let got = fsqrt(x);
            let want = x.sqrt();
            let rel = ((got - want) / want).abs();
            if rel > worst {
                worst = rel;
            }
            x += 0.017;
        }
        assert!(worst < 1e-5, "worst relative error {worst}");
        assert_eq!(fsqrt(0.0), 0.0);
        assert_eq!(fsqrt(-3.0), 0.0);
        assert!((fsqrt(4.0) - 2.0).abs() < 1e-5);
    }

    #[test]
    fn sdf_signs_are_correct() {
        // Inside the V stroke.
        assert!(mark_sdf(0.0, 0.2) < 0.0, "V bottom must be inside");
        // On the ring (left flank, away from the top gap).
        assert!(mark_sdf(-0.8, 0.0) < 0.0, "ring band must be inside");
        // Far outside.
        assert!(mark_sdf(1.6, 1.6) > 0.0, "corner must be outside");
        assert!(mark_sdf(0.0, 0.0) > 0.0, "hollow centre must be outside");
        // Ring interior (between centre and the band) is outside the stroke.
        assert!(mark_sdf(0.0, 0.55) > 0.0, "gap between ring and V is outside");
    }

    #[test]
    fn ring_gap_is_cut_at_the_top() {
        // A point on the ring band at the top is in the gap → no coverage.
        let aa = 1.0 / 64.0;
        let top = coverage_at(0.0, -0.8, aa);
        assert!(top < 0.02, "top of the ring must be open, cov={top}");
        // The same radius on the flank still draws.
        let flank = coverage_at(-0.8, 0.0, aa);
        assert!(flank > 0.9, "flank must draw, cov={flank}");
    }

    #[test]
    fn metrics_scale_with_the_short_edge() {
        let (s, _b) = surface(1920, 1080);
        let (_cx, _cy, size) = metrics_for(&s);
        assert_eq!(size, 151); // 1080 * 14%
        let (small, _b) = surface(320, 240);
        let (_, _, size_small) = metrics_for(&small);
        assert_eq!(size_small, 40); // clamped floor
        let (big, _b) = surface(3840, 2160);
        let (_, _, size_big) = metrics_for(&big);
        assert_eq!(size_big, 302);
    }

    #[test]
    fn draw_produces_a_centered_mark() {
        let (s, _b) = surface(1024, 768);
        let (cx, cy, size) = metrics_for(&s);
        let bottom = draw(&s, cx, cy, size);
        assert_eq!(bottom, cy + size / 2);

        // Pixels are written inside the mark box...
        let mut inside = 0;
        for y in (cy - size / 2)..=(cy + size / 2) {
            for x in (cx - size / 2)..=(cx + size / 2) {
                if s.get_px(x, y).unwrap_or(0) != 0 {
                    inside += 1;
                }
            }
        }
        assert!(inside > 100, "mark barely drew, inside={inside}");

        // ...and nowhere outside it (the layer must not bleed).
        let mut outside = 0;
        for y in 0..768 {
            for x in 0..1024 {
                if x >= cx - size / 2 && x <= cx + size / 2 && y >= cy - size / 2 && y <= cy + size / 2
                {
                    continue;
                }
                if s.get_px(x, y).unwrap_or(0) != 0 {
                    outside += 1;
                }
            }
        }
        assert_eq!(outside, 0, "logo bled outside its box");
    }

    #[test]
    fn mark_is_resolution_independent() {
        // The same normalized point (the V's bottom vertex) is inked at every
        // size — proof the mark is vector, not bitmap.
        for &size in &[40i64, 96, 160, 320] {
            let (s, _b) = surface(1024, 768);
            let cx = 512;
            let cy = 384;
            draw(&s, cx, cy, size);
            // V vertex sits at normalized (0, 0.34) → below centre.
            let px = cx;
            let py = cy + ((0.34 * (size as f32) * 0.5) as i64);
            assert!(
                s.get_px(px, py).unwrap_or(0) != 0,
                "V vertex missing at size {size}"
            );
        }
    }

    #[test]
    fn oversized_mark_clips_instead_of_faulting() {
        let (s, _b) = surface(128, 128);
        // Way bigger than the surface and off-centre: must not panic, must not
        // write out of bounds.
        draw(&s, 4, 4, 512);
    }
}
