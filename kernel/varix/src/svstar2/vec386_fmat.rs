//! VE-F0786 色彩矩阵滤镜
//! 职责：4×5 色彩矩阵乘法（RGBA + 偏置行）、常用矩阵（灰度/反相/通道交换）、
//!       单位矩阵恒等、行和不变性（alpha 行约束）。
//! 契约：单位矩阵恒等；灰度矩阵输出 R==G==B；偏置钳制 [0,255]。

use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::vec381_fbase::{FBitmap, Params, Pixel};

/// 4×5 矩阵（四行 RGBA 系数 + 每行末列为偏置 0..255）
pub type ColorMat = [[f64; 5]; 4];

/// 单位矩阵
pub const IDENT: ColorMat = [
    [1.0, 0.0, 0.0, 0.0, 0.0],
    [0.0, 1.0, 0.0, 0.0, 0.0],
    [0.0, 0.0, 1.0, 0.0, 0.0],
    [0.0, 0.0, 0.0, 1.0, 0.0],
];

/// 灰度矩阵（Rec.601 系数）
pub const GRAY: ColorMat = [
    [0.299, 0.587, 0.114, 0.0, 0.0],
    [0.299, 0.587, 0.114, 0.0, 0.0],
    [0.299, 0.587, 0.114, 0.0, 0.0],
    [0.0, 0.0, 0.0, 1.0, 0.0],
];

/// 反相矩阵
pub const INVERT: ColorMat = [
    [-1.0, 0.0, 0.0, 0.0, 255.0],
    [0.0, -1.0, 0.0, 0.0, 255.0],
    [0.0, 0.0, -1.0, 0.0, 255.0],
    [0.0, 0.0, 0.0, 1.0, 0.0],
];

/// 单像素矩阵乘
pub fn apply_mat(px: Pixel, m: &ColorMat) -> Pixel {
    let src = [px[0] as f64, px[1] as f64, px[2] as f64, px[3] as f64];
    let mut out = [0u8; 4];
    for r in 0..4 {
        let mut v = m[r][4]; // 偏置
        for c in 0..4 {
            v += m[r][c] * src[c];
        }
        out[r] = v.round().clamp(0.0, 255.0) as u8;
    }
    out
}

/// 全图矩阵滤镜
pub fn op_colormat(b: &FBitmap, p: &Params) -> Result<FBitmap, &'static str> {
    // 矩阵以 20 个参数键传入：m00..m34
    let mut m = IDENT;
    for r in 0..4 {
        for c in 0..5 {
            let key = alloc::format!("m{}{}", r, c);
            if let Some(v) = p.get(&key) {
                m[r][c] = v;
            }
        }
    }
    let mut out = FBitmap::new(b.w, b.h);
    for y in 0..b.h {
        for x in 0..b.w {
            out.set(x, y, apply_mat(b.get(x, y), &m));
        }
    }
    Ok(out)
}

pub fn run_vec386_checks() -> crate::checks::CheckSet {
    let mut s = CheckSet::new("svstar2-vec386");
    let red: Pixel = [200, 50, 30, 255];
    // 族一：单位矩阵恒等
    s.add("单位-恒等", apply_mat(red, &IDENT) == red, "identity");
    // 族二：灰度矩阵
    let g = apply_mat(red, &GRAY);
    s.add("灰度-三通道等", g[0] == g[1] && g[1] == g[2], "R==G==B");
    // Rec.601 值：0.299*200 + 0.587*50 + 0.114*30 = 59.8+29.35+3.42 = 92.57 → 93
    s.add("灰度-系数值", g[0] == 93, "luma 93");
    s.add("灰度-alpha不动", g[3] == 255, "alpha kept");
    // 白色 → 255
    s.add("灰度-白保持", apply_mat([255, 255, 255, 255], &GRAY)[0] == 255, "white 255");
    // 黑色 → 0
    s.add("灰度-黑保持", apply_mat([0, 0, 0, 255], &GRAY)[0] == 0, "black 0");
    // 族三：反相矩阵
    let inv = apply_mat(red, &INVERT);
    s.add("反相-红变青", inv == [55, 205, 225, 255], "200→55 etc");
    s.add("反相-双反还原", apply_mat(inv, &INVERT) == red, "double invert");
    // 反相白变黑
    s.add("反相-白变黑", apply_mat([255, 255, 255, 255], &INVERT) == [0, 0, 0, 255], "white→black");
    // 族四：偏置与钳制
    // 纯加 50 亮度
    let mut add50 = IDENT;
    add50[0][4] = 50.0;
    add50[1][4] = 50.0;
    add50[2][4] = 50.0;
    let a = apply_mat(red, &add50);
    s.add("偏置-加50", a[0] == 250 && a[1] == 100, "bias added");
    // 上钳
    add50[0][4] = 200.0;
    s.add("偏置-上钳", apply_mat(red, &add50)[0] == 255, "clamp 255");
    // 负下钳
    let mut sub = IDENT;
    sub[0][4] = -300.0;
    s.add("偏置-下钳", apply_mat(red, &sub)[0] == 0, "clamp 0");
    // 族五：通道交换
    let mut swap = IDENT;
    let (sa, sb) = (swap[0][1], swap[1][0]);
    swap[0][1] = sb;
    swap[1][0] = sa;
    let sw = apply_mat(red, &swap);
    s.add("交换-RG", sw == [50, 200, 30, 255], "R↔G");
    // 族六：全图管线
    let bm = FBitmap::solid(4, 4, red);
    // 默认（无参数）= 单位矩阵恒等
    let out0 = op_colormat(&bm, &Params::new()).expect("o0");
    s.add("管线-默认恒等", out0 == bm, "defaults identity");
    // 灰度管线（20 参数全传）
    let mut pg = Params::new();
    for r in 0..4 {
        for c in 0..5 {
            pg.set(&leak_key(r, c), GRAY[r][c]);
        }
    }
    let out1 = op_colormat(&bm, &pg).expect("o1");
    s.add("管线-灰度", out1.get(0, 0) == g, "gray applied");
    s.add("管线-尺寸", out1.same_size(&bm), "same size");
    // 部分参数（只覆盖一格）
    let mut ppartial = Params::new();
    ppartial.set("m05", 0.5); // 不存在（键是 m1 0？行1列0）——验证非法键被忽略
    let out2 = op_colormat(&bm, &ppartial).expect("o2");
    s.add("管线-非法键忽略", out2 == bm, "unknown keys ignored");
    // 族七：alpha 行不被污染
    s.add("alpha-保持", apply_mat([10, 20, 30, 77], &GRAY)[3] == 77, "alpha passthrough");
    s
}

fn leak_key(r: usize, c: usize) -> &'static str {
    // 泄漏为 'static 的键名（no_std 环境用 Box::leak）
    let s: &'static str = alloc::format!("m{}{}", r, c).leak();
    s
}
