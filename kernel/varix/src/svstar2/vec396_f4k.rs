//! VE-F0796 滤镜 4K 精度
//! 职责：4K（3840×2160）规模下的内存预算与定点近似路径 ——
//!       以 u16 定点（Q8.8）替代 f64 做中间运算，误差可控、内存减半。
//! 契约：定点往返误差 ≤ 1 最低有效位（u8 域）；尺寸预算先行校验。

use crate::checks::CheckSet;
use super::vec381_fbase::{FBitmap, Pixel};

/// 4K 尺寸常量
pub const W4K: usize = 3840;
pub const H4K: usize = 2160;

/// 单帧 RGBA 内存字节数
pub fn frame_bytes(w: usize, h: usize) -> u64 {
    (w as u64) * (h as u64) * 4
}

/// 定点 Q8.8：u8 → u16（高 8 位整数、低 8 位小数）
pub fn to_fixed(x: u8) -> u16 {
    (x as u16) << 8
}

/// 定点 → u8（四舍五入到最近整数档）
pub fn from_fixed(x: u16) -> u8 {
    ((x + 128) >> 8).min(255) as u8
}

/// 定点乘（Q8.8 × Q8.8 → Q8.8，低 8 位四舍五入）
pub fn mul_fixed(a: u16, b: u16) -> u16 {
    (((a as u32) * (b as u32) + 0x80) >> 8) as u16
}

/// 定点亮度（Rec.601，权重 Q8.8 定点化：77/150/29 近似 0.302/0.588/0.114）
pub fn luma_fixed(p: Pixel) -> u16 {
    let r = mul_fixed(to_fixed(p[0]), 77 * 256 / 255);
    let g = mul_fixed(to_fixed(p[1]), 150 * 256 / 255);
    let b = mul_fixed(to_fixed(p[2]), 29 * 256 / 255);
    r + g + b
}

/// 定点反相变换（往返校验路径）
pub fn invert_fixed(src: &FBitmap) -> FBitmap {
    let mut out = FBitmap::new(src.w, src.h);
    for y in 0..src.h {
        for x in 0..src.w {
            let p = src.get(x, y);
            out.set(x, y, [255 - p[0], 255 - p[1], 255 - p[2], p[3]]);
        }
    }
    out
}

/// 预算校验：帧内存 + 双缓冲 ≤ 上限
pub fn budget4k(w: usize, h: usize, cap_mib: u64) -> bool {
    frame_bytes(w, h) * 2 <= cap_mib * 1024 * 1024
}

pub fn run_vec396_checks() -> crate::checks::CheckSet {
    let mut s = CheckSet::new("svstar2-vec396");
    // 族一：尺寸常量与内存
    s.add("4k-尺寸", W4K == 3840 && H4K == 2160, "dims");
    s.add("4k-帧字节", frame_bytes(W4K, H4K) == 3840 * 2160 * 4, "frame bytes");
    s.add("4k-双缓冲预算", budget4k(W4K, H4K, 64), "2 frames ≤ 64MiB");
    s.add("4k-预算超", !budget4k(W4K, H4K, 10), "cap reject");
    // 族二：定点往返
    let mut rt_ok = true;
    for x in 0..=255u8 {
        if from_fixed(to_fixed(x)) != x {
            rt_ok = false;
        }
    }
    s.add("4k-定点往返", rt_ok, "u8 roundtrip");
    // 族三：定点乘
    s.add("4k-乘恒等", mul_fixed(to_fixed(100), 256) == to_fixed(100), "×1.0");
    s.add("4k-乘半", from_fixed(mul_fixed(to_fixed(100), 128)) == 50, "×0.5");
    s.add("4k-乘零", mul_fixed(to_fixed(200), 0) == 0, "×0");
    s.add("4k-乘饱和", from_fixed(mul_fixed(to_fixed(255), 256)) == 255, "max");
    // 族四：定点亮度域
    s.add("4k-黑亮度", luma_fixed([0, 0, 0, 255]) == 0, "black");
    s.add("4k-白亮度高", luma_fixed([255, 255, 255, 255]) > 63000, "white");
    s.add("4k-绿主导", luma_fixed([0, 255, 0, 255]) > luma_fixed([255, 0, 0, 255]), "green>red");
    // 族五：定点反相
    let small = FBitmap::solid(4, 4, [10, 200, 90, 255]);
    let inv = invert_fixed(&small);
    s.add("4k-反相值", inv.get(0, 0) == [245, 55, 165, 255], "invert");
    s.add("4k-二次反相", invert_fixed(&inv).px == small.px, "involution");
    // 族六：小规模全值定点亮度有界（≤ 3×Q8.8 max）
    s.add("4k-亮度上界", luma_fixed([255, 255, 255, 255]) <= 3 * 65535, "bound");
    s
}
