//! VE-F0784 发光滤镜
//! 职责：外发光（alpha 提取 → 膨胀 → 模糊 → 着色合成）、发光半径约束、
//!       膨胀（圆形结构元）、发光能量参数。
//! 契约：发光只在内容外围；半径 0 等价于描边不膨胀；纯色图发光圈完整。

use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::vec381_fbase::{FBitmap, Params, Pixel};
use super::vec383_fshadow::content_mask;

/// 圆形膨胀（半径 r 的结构元）
pub fn dilate(mask: &[bool], w: usize, h: usize, r: usize) -> Vec<bool> {
    let mut out = alloc::vec![false; mask.len()];
    let r2 = (r * r) as i64;
    for y in 0..h {
        for x in 0..w {
            let mut on = false;
            let mut dy = -(r as i64);
            while dy <= r as i64 && !on {
                let mut dx = -(r as i64);
                while dx <= r as i64 {
                    if dx * dx + dy * dy <= r2 {
                        let sx = x as i64 + dx;
                        let sy = y as i64 + dy;
                        if sx >= 0 && sx < w as i64 && sy >= 0 && sy < h as i64 && mask[sy as usize * w + sx as usize] {
                            on = true;
                            break;
                        }
                    }
                    dx += 1;
                }
                dy += 1;
            }
            out[y * w + x] = on;
        }
    }
    out
}

/// 外发光主入口
pub fn op_glow(content: &FBitmap, p: &Params) -> Result<FBitmap, &'static str> {
    let r = p.get_or("radius", 2.0);
    let alpha = p.get_or("alpha", 128.0) as u8;
    if r < 0.0 {
        return Err("negative radius");
    }
    let color: Pixel = [p.get_or("r", 255.0) as u8, p.get_or("g", 255.0) as u8, p.get_or("b", 255.0) as u8, alpha];
    let mask = content_mask(content);
    let ri = r.round() as usize;
    let grown = if ri == 0 { mask.clone() } else { dilate(&mask, content.w, content.h, ri) };
    let mut out = content.clone();
    let glow: Pixel = [color[0], color[1], color[2], color[3]];
    for y in 0..content.h {
        for x in 0..content.w {
            if grown[y * content.w + x] && !mask[y * content.w + x] {
                out.set(x, y, glow);
            }
        }
    }
    Ok(out)
}

/// 发光圈像素计数
pub fn glow_count(content: &FBitmap, out: &FBitmap) -> usize {
    let m = content_mask(content);
    out.px.iter().zip(m.iter()).filter(|(p, cm)| p[3] > 0 && !**cm).count()
}

pub fn run_vec384_checks() -> crate::checks::CheckSet {
    let mut s = CheckSet::new("svstar2-vec384");
    let mut content = FBitmap::solid(16, 16, [0, 0, 0, 0]);
    for y in 6..10 {
        for x in 6..10 {
            content.set(x, y, [255, 0, 0, 255]);
        }
    }
    // 族一：膨胀
    let mask = content_mask(&content);
    s.add("膨胀-原计数", mask_count(&mask) == 16, "16 base");
    let d1 = dilate(&mask, 16, 16, 1);
    s.add("膨胀-r1更大", mask_count(&d1) > 16, "grown");
    s.add("膨胀-r1含原", mask.iter().zip(d1.iter()).all(|(a, b)| !*a || *b), "base ⊆ grown");
    let d2 = dilate(&mask, 16, 16, 2);
    s.add("膨胀-单调", mask_count(&d2) >= mask_count(&d1), "monotone");
    // r=0 不变
    s.add("膨胀-零不变", dilate(&mask, 16, 16, 0) == mask, "r=0 same");
    // 膨胀界（方形界）：r=1 → 16 + 边界 20 = 36（十字形圆判 r²≤1 剔除角 → 恰 36）
    s.add("膨胀-r1计数", mask_count(&d1) == 36, "plus shape exact 36");
    // 出界裁剪：内容贴边膨胀不出画布
    let mut edge = FBitmap::solid(16, 16, [0, 0, 0, 0]);
    edge.set(0, 0, [255, 255, 255, 255]);
    let em = dilate(&content_mask(&edge), 16, 16, 2);
    s.add("膨胀-出界安全", em.len() == 256, "no panic");
    // 族二：发光主入口
    let mut p2 = Params::new();
    p2.set("radius", 1.0);
    p2.set("alpha", 200.0);
    let glow = op_glow(&content, &p2).expect("g");
    s.add("发光-尺寸不变", glow.same_size(&content), "same size");
    // 内容保持
    s.add("发光-内容保持", glow.get(6, 6) == [255, 0, 0, 255], "content kept");
    // 发光圈出现（不覆盖内容）
    s.add("发光-圈出现", glow_count(&content, &glow) == 20, "ring count 36-16");
    // 发光像素为配置色
    s.add("发光-颜色", glow.get(5, 7) == [255, 255, 255, 200], "glow color");
    // 远处仍透明
    s.add("发光-远处透明", glow.get(0, 0) == [0, 0, 0, 0], "far transparent");
    // 族三：半径 0 = 无圈
    let mut p0 = Params::new();
    p0.set("radius", 0.0);
    let g0 = op_glow(&content, &p0).expect("g0");
    s.add("发光-零半径无圈", glow_count(&content, &g0) == 0, "no ring");
    s.add("发光-零半径恒等", g0 == content, "identity");
    // 族四：负半径拒绝
    let mut pn = Params::new();
    pn.set("radius", -1.0);
    s.add("发光-负半径拒", op_glow(&content, &pn) == Err("negative radius"), "negative");
    // 族五：默认参数可用
    let gd = op_glow(&content, &Params::new()).expect("gd");
    s.add("发光-默认可用", gd.same_size(&content), "defaults ok");
    // 族六：发光圈连通（环绕内容一圈）
    // 内容 6..10，r=1 圈应覆盖 (5..11) 边框
    s.add("圈-左", glow.get(5, 7)[3] == 200, "left ring");
    s.add("圈-右", glow.get(10, 7)[3] == 200, "right ring");
    s.add("圈-上", glow.get(7, 5)[3] == 200, "top ring");
    s.add("圈-下", glow.get(7, 10)[3] == 200, "bottom ring");
    // 圈不进入内容
    s.add("圈-不进内容", glow.get(7, 7) == [255, 0, 0, 255], "inside is content");
    s
}

fn mask_count(m: &[bool]) -> usize {
    m.iter().filter(|b| **b).count()
}
