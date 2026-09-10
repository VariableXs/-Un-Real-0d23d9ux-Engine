//! TRINITY-500 · AI-05 · F105 模糊与玻璃材质
//!
//! 无 GPU 时的软合成路径：可分离盒式模糊 + 玻璃材质合成。
//! 纪律：**禁止降采样换取速度**（画质无损军规），半径越大越慢，但不糊。

/// 模糊参数。半径以像素为单位，passes 为迭代次数（近似高斯）。
#[derive(Clone, Copy, Debug)]
pub struct BlurKernel {
    pub radius: u32,
    pub passes: u32,
}

impl BlurKernel {
    pub const fn new(radius: u32, passes: u32) -> BlurKernel {
        BlurKernel { radius, passes }
    }

    /// 单次水平/垂直扫描的窗口宽度。
    pub fn window(&self) -> u32 {
        self.radius * 2 + 1
    }

    /// 成本估算（每像素读写次数），用于性能预算门禁。
    pub fn cost_per_pixel(&self) -> u32 {
        self.window() * 2 * self.passes.max(1)
    }
}

/// 可分离盒式模糊的水平一遍（不产生分配；`tmp` 由调用方提供）。
///
/// 越界像素按边缘钳制（clamp-to-edge），保证边界不发黑。
pub fn blur_horizontal(src: &[u32], tmp: &mut [u32], w: usize, h: usize, radius: u32) -> bool {
    if src.len() < w * h || tmp.len() < w * h {
        return false;
    }
    let r = radius as i64;
    for y in 0..h {
        let row = y * w;
        for x in 0..w {
            let mut acc = [0u32; 3];
            let mut n = 0u32;
            let mut d = -r;
            while d <= r {
                let sx = (x as i64 + d).clamp(0, w as i64 - 1) as usize;
                let p = src[row + sx];
                acc[0] += (p >> 16) & 0xFF;
                acc[1] += (p >> 8) & 0xFF;
                acc[2] += p & 0xFF;
                n += 1;
                d += 1;
            }
            let r8 = acc[0] / n;
            let g8 = acc[1] / n;
            let b8 = acc[2] / n;
            tmp[row + x] = 0xFF00_0000 | (r8 << 16) | (g8 << 8) | b8;
        }
    }
    true
}

/// 可分离盒式模糊的垂直一遍（读 `tmp`，写 `dst`）。
pub fn blur_vertical(tmp: &[u32], dst: &mut [u32], w: usize, h: usize, radius: u32) -> bool {
    if tmp.len() < w * h || dst.len() < w * h {
        return false;
    }
    let r = radius as i64;
    for y in 0..h {
        let row = y * w;
        for x in 0..w {
            let mut acc = [0u32; 3];
            let mut n = 0u32;
            let mut d = -r;
            while d <= r {
                let sy = (y as i64 + d).clamp(0, h as i64 - 1) as usize;
                let p = tmp[sy * w + x];
                acc[0] += (p >> 16) & 0xFF;
                acc[1] += (p >> 8) & 0xFF;
                acc[2] += p & 0xFF;
                n += 1;
                d += 1;
            }
            dst[row + x] = 0xFF00_0000 | ((acc[0] / n) << 16) | ((acc[1] / n) << 8) | (acc[2] / n);
        }
    }
    true
}

/// 完整一遍（水平 + 垂直）。
pub fn blur_once(src: &[u32], dst: &mut [u32], tmp: &mut [u32], w: usize, h: usize, radius: u32) -> bool {
    if !blur_horizontal(src, tmp, w, h, radius) {
        return false;
    }
    blur_vertical(tmp, dst, w, h, radius)
}

// ---------------------------------------------------------------------------
// 玻璃材质
// ---------------------------------------------------------------------------

/// 玻璃材质参数：模糊 + 色调 + 白噪声颗粒（避免大面积色带）。
#[derive(Clone, Copy, Debug)]
pub struct Glass {
    pub blur: BlurKernel,
    /// 底色叠加（0x00RRGGBB）。
    pub tint: u32,
    /// 色调强度 0..255。
    pub tint_alpha: u8,
    /// 颗粒强度 0..255。
    pub grain: u8,
    /// 高光边强度 0..255。
    pub rim: u8,
}

impl Glass {
    pub const fn frosted() -> Glass {
        Glass {
            blur: BlurKernel::new(8, 2),
            tint: 0x0016_1A22,
            tint_alpha: 96,
            grain: 8,
            rim: 40,
        }
    }
}

/// 在已有模糊结果上叠加玻璃色调。
pub fn glass_over(blurred: u32, g: &Glass) -> u32 {
    let tinted = crate::gfx::surface::blend(g.tint, blurred, g.tint_alpha);
    let with_grain = if g.grain > 0 {
        // 确定性哈希噪声：同一像素每帧一致，避免闪烁。
        let h = (blurred ^ (blurred >> 7) ^ 0x9E37_79B9) & 0xFF;
        let n = ((h as u32 * g.grain as u32) / 255) as u32;
        let add = |c: u32| -> u32 { (c + n).min(255) };
        let r = add((tinted >> 16) & 0xFF);
        let gg = add((tinted >> 8) & 0xFF);
        let b = add(tinted & 0xFF);
        0xFF00_0000 | (r << 16) | (gg << 8) | b
    } else {
        tinted
    };
    crate::gfx::surface::apply_highlight(with_grain, g.rim / 4)
}

/// 玻璃区域是否值得开模糊（小面积直接纯色，省算力）。
pub fn blur_worth_it(area: i64, kernel: &BlurKernel) -> bool {
    area >= 64 && kernel.cost_per_pixel() <= 512
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f105_kernel_window() {
        let k = BlurKernel::new(4, 2);
        assert_eq!(k.window(), 9);
        assert_eq!(k.cost_per_pixel(), 9 * 2 * 2);
    }

    #[test]
    fn f105_box_blur_smears() {
        let w = 4usize;
        let h = 1usize;
        let src = [0x0000_0000u32, 0x0000_0000, 0x00FF_FFFF, 0x00FF_FFFF];
        let mut tmp = [0u32; 4];
        let mut dst = [0u32; 4];
        assert!(blur_horizontal(&src, &mut tmp, w, h, 1));
        assert!(tmp[1] & 0x00FF_FFFF > 0, "edge picks up neighbour");
        assert!(blur_vertical(&tmp, &mut dst, w, h, 0));
        assert_eq!(dst, tmp);
    }

    #[test]
    fn f105_blur_once_is_bounded() {
        let w = 3usize;
        let h = 3usize;
        let src = [0x0012_3456u32; 9];
        let mut tmp = [0u32; 9];
        let mut dst = [0u32; 9];
        assert!(blur_once(&src, &mut dst, &mut tmp, w, h, 1));
        for p in dst.iter() {
            assert_eq!(p & 0xFF00_0000, 0xFF00_0000, "alpha stays opaque");
        }
        let mut small = [0u32; 2];
        assert!(!blur_once(&src, &mut small, &mut tmp, w, h, 1));
    }

    #[test]
    fn f105_glass_material() {
        let g = Glass::frosted();
        let out = glass_over(0xFF80_8080, &g);
        let r = (out >> 16) & 0xFF;
        assert!(r < 0x80 + 64, "tint darkens the backdrop");
        assert_eq!(out >> 24, 0xFF);
        assert!(blur_worth_it(128, &g.blur));
        assert!(!blur_worth_it(4, &g.blur));
    }
}
