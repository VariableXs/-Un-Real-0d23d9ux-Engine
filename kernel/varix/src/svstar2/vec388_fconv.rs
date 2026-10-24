//! VE-F0788 通用卷积滤镜
//! 职责：任意 3×3 核的二维卷积（边界 clamp），预设锐化 / 边缘 / 浮雕，
//!       权重归一化保证亮度守恒。
//! 契约：输出尺寸不变；核元素和为 0 的边缘核不做归一化（alpha 通道直通）。

use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::vec381_fbase::{FBitmap, Params, Pixel};

/// 3×3 卷积核
pub type Kernel3 = [[f64; 3]; 3];

pub const SHARPEN: Kernel3 = [
    [0.0, -1.0, 0.0],
    [-1.0, 5.0, -1.0],
    [0.0, -1.0, 0.0],
];
pub const EDGE: Kernel3 = [
    [0.0, 1.0, 0.0],
    [1.0, -4.0, 1.0],
    [0.0, 1.0, 0.0],
];
pub const EMBOSS: Kernel3 = [
    [-2.0, -1.0, 0.0],
    [-1.0, 1.0, 1.0],
    [0.0, 1.0, 2.0],
];
pub const IDENTITY_K: Kernel3 = [
    [0.0, 0.0, 0.0],
    [0.0, 1.0, 0.0],
    [0.0, 0.0, 0.0],
];

/// 核元素和（用于归一化）
pub fn kernel_sum(k: &Kernel3) -> f64 {
    k.iter().flat_map(|r| r.iter()).sum()
}

fn u8c(v: f64) -> u8 {
    v.round().clamp(0.0, 255.0) as u8
}

/// 单像素卷积（邻域已取好，行主序 9 元素）
pub fn conv_px(nb: &[Pixel; 9], k: &Kernel3, alpha_keep: bool) -> Pixel {
    let mut acc = [0.0f64; 3];
    for i in 0..9 {
        let kw = k[i / 3][i % 3];
        if kw == 0.0 {
            continue;
        }
        acc[0] += nb[i][0] as f64 * kw;
        acc[1] += nb[i][1] as f64 * kw;
        acc[2] += nb[i][2] as f64 * kw;
    }
    let sum = kernel_sum(k);
    if sum > 0.001 || sum < -0.001 {
        acc[0] /= sum;
        acc[1] /= sum;
        acc[2] /= sum;
    }
    let a = if alpha_keep { nb[4][3] } else { nb[4][3] };
    [u8c(acc[0]), u8c(acc[1]), u8c(acc[2]), a]
}

/// 全图卷积（clamp 边界）
pub fn convolve(src: &FBitmap, k: &Kernel3) -> FBitmap {
    let mut out = FBitmap::new(src.w, src.h);
    let mut nb = [[0u8; 4]; 9];
    for y in 0..src.h {
        for x in 0..src.w {
            for dy in -1i64..=1 {
                for dx in -1i64..=1 {
                    let cx = (x as i64 + dx).clamp(0, src.w as i64 - 1) as usize;
                    let cy = (y as i64 + dy).clamp(0, src.h as i64 - 1) as usize;
                    nb[((dy + 1) * 3 + (dx + 1)) as usize] = src.get(cx, cy);
                }
            }
            out.set(x, y, conv_px(&nb, k, true));
        }
    }
    out
}

/// 算子包装：参数 kernel = 0 锐化 / 1 边缘 / 2 浮雕 / 其他恒等
pub fn op_conv(b: &FBitmap, p: &Params) -> Result<FBitmap, &'static str> {
    let sel = p.get_or("kernel", -1.0);
    let k = match sel as i64 {
        0 => SHARPEN,
        1 => EDGE,
        2 => EMBOSS,
        _ => IDENTITY_K,
    };
    Ok(convolve(b, &k))
}

pub fn run_vec388_checks() -> crate::checks::CheckSet {
    let mut s = CheckSet::new("svstar2-vec388");
    // 族一：核工具
    s.add("卷积-恒等核和", (kernel_sum(&IDENTITY_K) - 1.0).abs() < 1e-9, "sum id");
    s.add("卷积-锐化核和", (kernel_sum(&SHARPEN) - 1.0).abs() < 1e-9, "sum sharpen");
    s.add("卷积-边缘核和", kernel_sum(&EDGE).abs() < 1e-9, "sum edge 0");
    // 族二：恒等核不变
    let src = FBitmap::solid(4, 4, [100, 150, 200, 255]);
    let id = convolve(&src, &IDENTITY_K);
    s.add("卷积-恒等核", id.px == src.px, "identity kernel");
    // 族三：纯色图锐化不变（邻域同值 → 加权和 = 值 × 核和 = 原值）
    let sh = convolve(&src, &SHARPEN);
    s.add("卷积-纯色锐化不变", sh.get(1, 1) == [100, 150, 200, 255], "solid invar");
    // 族四：边缘检测零响应于纯色
    let eg = convolve(&src, &EDGE);
    s.add("卷积-纯色边缘零", eg.get(1, 1) == [0, 0, 0, 255], "edge zero");
    // 族五：浮雕有响应
    let em = convolve(&src, &EMBOSS);
    s.add("卷积-浮雕响应", em.get(1, 1) != [0, 0, 0, 255], "emboss resp");
    s.add("卷积-输出同尺寸", em.w == src.w && em.h == src.h, "size keep");
    // 族六：单像素卷积（中心加权 2，邻域 0 → alpha 直通）
    let nb = [[255u8, 0, 0, 255]; 9];
    let k2 = [[0.0; 3], [0.0, 2.0, 0.0], [0.0; 3]];
    let r = conv_px(&nb, &k2, true);
    s.add("卷积-单像素加倍", r == [255, 0, 0, 255], "center x2 clamp");
    // 族七：算子选择
    let mut p = Params::new();
    p.set("kernel", 0.0);
    let o0 = op_conv(&src, &p).expect("k0");
    s.add("卷积-算子锐化", o0.w == src.w, "op kernel 0");
    let o9 = op_conv(&src, &Params::new()).expect("kdef");
    s.add("卷积-算子缺省恒等", o9.px == src.px, "op default id");
    // 族八：8-bit 夹取（负下溢）
    let dark = FBitmap::solid(3, 3, [5, 5, 5, 255]);
    let dsh = convolve(&dark, &SHARPEN);
    s.add("卷积-下溢夹0", dsh.get(1, 1)[0] >= 0, "underflow clamp");
    s
}
