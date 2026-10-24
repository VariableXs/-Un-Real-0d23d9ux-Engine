//! VE-F0785 色彩调整滤镜
//! 职责：亮度/对比度/饱和度三参数调整、参数域校验（钳制）、
//!       中性输入不变性（灰图三参数不动）、通道独立性。
//! 契约：参数钳制到合法域；灰图在任何参数下不变（亮度除外按域）；能量不越 [0,255]。

use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::vec381_fbase::{FBitmap, Params, Pixel};

/// 亮度：加法偏移（-255..255）
pub fn apply_brightness(px: Pixel, v: f64) -> Pixel {
    let add = |c: u8| ((c as f64 + v).round().clamp(0.0, 255.0)) as u8;
    [add(px[0]), add(px[1]), add(px[2]), px[3]]
}

/// 对比度：围绕 128 缩放（因子 0..∞，钳 [0,4]）
pub fn apply_contrast(px: Pixel, k: f64) -> Pixel {
    let k = k.clamp(0.0, 4.0);
    let f = |c: u8| (((c as f64 - 128.0) * k + 128.0).round().clamp(0.0, 255.0)) as u8;
    [f(px[0]), f(px[1]), f(px[2]), px[3]]
}

/// 饱和度：向灰度锚点插值（因子 0..∞，钳 [0,4]；1 恒等）
pub fn apply_saturation(px: Pixel, k: f64) -> Pixel {
    let k = k.clamp(0.0, 4.0);
    let luma = 0.299 * px[0] as f64 + 0.587 * px[1] as f64 + 0.114 * px[2] as f64;
    let f = |c: u8| ((luma + (c as f64 - luma) * k).round().clamp(0.0, 255.0)) as u8;
    [f(px[0]), f(px[1]), f(px[2]), px[3]]
}

/// 三段主入口
pub fn op_color(b: &FBitmap, p: &Params) -> Result<FBitmap, &'static str> {
    let bright = p.get_or("brightness", 0.0);
    let contrast = p.get_or("contrast", 1.0);
    let sat = p.get_or("saturation", 1.0);
    let mut out = FBitmap::new(b.w, b.h);
    for y in 0..b.h {
        for x in 0..b.w {
            let mut px = b.get(x, y);
            px = apply_brightness(px, bright);
            px = apply_contrast(px, contrast);
            px = apply_saturation(px, sat);
            out.set(x, y, px);
        }
    }
    Ok(out)
}

pub fn run_vec385_checks() -> crate::checks::CheckSet {
    let mut s = CheckSet::new("svstar2-vec385");
    let red: Pixel = [200, 50, 30, 255];
    // 族一：亮度
    s.add("亮度-加法", apply_brightness(red, 50.0)[0] == 250, "200+50 clamp 250");
    s.add("亮度-上钳", apply_brightness(red, 200.0)[0] == 255, "clamp 255");
    s.add("亮度-下钳", apply_brightness(red, -200.0)[0] == 0, "clamp 0");
    s.add("亮度-零恒等", apply_brightness(red, 0.0) == red, "identity");
    s.add("亮度-alpha不动", apply_brightness(red, 100.0)[3] == 255, "alpha kept");
    // 负值像素
    s.add("亮度-负域", apply_brightness([10, 10, 10, 255], -50.0) == [0, 0, 0, 255], "floor");
    // 族二：对比度
    s.add("对比-零全灰", apply_contrast(red, 0.0) == [128, 128, 128, 255], "k=0 → 128");
    s.add("对比-一恒等", apply_contrast(red, 1.0) == red, "k=1 identity");
    s.add("对比-倍增", apply_contrast([178, 178, 178, 255], 2.0) == [228, 228, 228, 255], "50 over mid ×2");
    s.add("对比-上钳", apply_contrast([255, 255, 255, 255], 4.0)[0] == 255, "clamp high");
    s.add("对比-负钳", apply_contrast(red, -1.0) == [128, 128, 128, 255], "k<0 clamped to 0");
    s.add("对比-中点不动", apply_contrast([128, 128, 128, 255], 3.0) == [128, 128, 128, 255], "mid invariant");
    // 族三：饱和度
    s.add("饱和-一恒等", apply_saturation(red, 1.0) == red, "k=1 identity");
    s.add("饱和-零全灰", {
        let g = apply_saturation(red, 0.0);
        g[0] == g[1] && g[1] == g[2]
    }, "k=0 grayscale");
    // 灰度图饱和不变（任何 k）
    let gray: Pixel = [128, 128, 128, 255];
    s.add("饱和-灰不动", apply_saturation(gray, 2.0) == gray, "gray invariant");
    // 饱和度增强拉大与灰度距离
    let s2 = apply_saturation(red, 2.0);
    s.add("饱和-增强", (s2[0] as i32 - s2[1] as i32) > (red[0] as i32 - red[1] as i32), "amplified");
    // 上钳
    s.add("饱和-上钳", apply_saturation(red, 10.0)[0] <= 255, "clamp");
    // 族四：三段管线
    let bm = FBitmap::solid(4, 4, red);
    // 全默认 = 恒等
    let out0 = op_color(&bm, &Params::new()).expect("o0");
    s.add("管线-默认恒等", out0 == bm, "defaults identity");
    // 亮度管线
    let mut p50 = Params::new();
    p50.set("brightness", 50.0);
    let out1 = op_color(&bm, &p50).expect("o1");
    s.add("管线-亮度", out1.get(0, 0)[0] == 250, "applied");
    // 尺寸不变
    s.add("管线-尺寸", out1.same_size(&bm), "same size");
    // 组合：亮度+对比+饱和
    let mut pcombo = Params::new();
    pcombo.set("brightness", 10.0);
    pcombo.set("contrast", 1.5);
    pcombo.set("saturation", 1.2);
    let out2 = op_color(&bm, &pcombo).expect("o2");
    // 手工重算一致性
    let expect = apply_saturation(apply_contrast(apply_brightness(red, 10.0), 1.5), 1.2);
    s.add("管线-组合一致", out2.get(0, 0) == expect, "composition matches");
    // 族五：能量不越界
    s.add("能量-域内", out2.px.iter().all(|p| p.iter().all(|&c| c <= 255)), "all ≤ 255");
    // 族六：灰图亮度仍然变化（对比度例外已验）
    let gbm = FBitmap::solid(2, 2, gray);
    let mut pb = Params::new();
    pb.set("brightness", 100.0);
    let go = op_color(&gbm, &pb).expect("go");
    s.add("灰图-亮度生效", go.get(0, 0) == [228, 228, 228, 255], "gray brightened");
    s
}
