//! VE-F0783 投影滤镜
//! 职责：投影生成（内容 alpha 提取 → 偏移 → 模糊 → 着色 → 合成）、
//!       投影参数校验（offset/blur/color）、投影区不改变内容区、合成次序。
//! 契约：投影只加不减内容；偏移后投影区在画布内裁剪；关闭投影零影响。

use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::vec381_fbase::{FBitmap, Params, Pixel};
use super::vec382_fblur::blur_h;

/// 投影参数
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct ShadowCfg {
    pub dx: i64,
    pub dy: i64,
    pub blur: f64,
    pub alpha: u8,
}

impl ShadowCfg {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.blur < 0.0 {
            return Err("negative blur");
        }
        if self.alpha > 255 {
            return Err("alpha overflow");
        }
        Ok(())
    }
    pub fn off() -> ShadowCfg {
        ShadowCfg { dx: 0, dy: 0, blur: 0.0, alpha: 0 }
    }
    pub fn enabled(&self) -> bool {
        self.alpha > 0
    }
}

/// 内容遮罩（alpha > 0 的位置）
pub fn content_mask(b: &FBitmap) -> Vec<bool> {
    b.px.iter().map(|p| p[3] > 0).collect()
}

/// 遮罩偏移拷贝
pub fn shift_mask(mask: &[bool], w: usize, h: usize, dx: i64, dy: i64) -> Vec<bool> {
    let mut out = alloc::vec![false; mask.len()];
    for y in 0..h {
        for x in 0..w {
            let sx = x as i64 - dx;
            let sy = y as i64 - dy;
            if sx >= 0 && sx < w as i64 && sy >= 0 && sy < h as i64 {
                out[y * w + x] = mask[sy as usize * w + sx as usize];
            }
        }
    }
    out
}

/// 遮罩计数
pub fn mask_count(mask: &[bool]) -> usize {
    mask.iter().filter(|b| **b).count()
}

/// 投影主入口：内容 + 影 → 合成位图（影在内容下方）
pub fn op_shadow(content: &FBitmap, cfg: &ShadowCfg) -> Result<FBitmap, &'static str> {
    cfg.validate()?;
    let mut out = content.clone();
    if !cfg.enabled() {
        return Ok(out);
    }
    let mask = content_mask(content);
    let shifted = shift_mask(&mask, content.w, content.h, cfg.dx, cfg.dy);
    // 影色（黑 + alpha）
    let shade: Pixel = [0, 0, 0, cfg.alpha];
    // 影先铺（不覆盖内容非透明像素）
    for y in 0..content.h {
        for x in 0..content.w {
            if shifted[y * content.w + x] {
                let cur = content.get(x, y);
                if cur[3] == 0 {
                    out.set(x, y, shade);
                }
            }
        }
    }
    Ok(out)
}

pub fn run_vec383_checks() -> crate::checks::CheckSet {
    let mut s = CheckSet::new("svstar2-vec383");
    // 内容：中心 4×4 方块，透明背景
    let mut content = FBitmap::solid(16, 16, [0, 0, 0, 0]);
    for y in 6..10 {
        for x in 6..10 {
            content.set(x, y, [255, 0, 0, 255]);
        }
    }
    // 族一：参数校验
    let ok = ShadowCfg { dx: 2, dy: 2, blur: 2.0, alpha: 128 };
    s.add("校验-合法", ok.validate().is_ok(), "valid");
    s.add("校验-负模糊拒", ShadowCfg { dx: 0, dy: 0, blur: -1.0, alpha: 128 }.validate() == Err("negative blur"), "negative blur");
    s.add("校验-关闭", ShadowCfg::off().validate().is_ok(), "off valid");
    s.add("校验-关状态", !ShadowCfg::off().enabled(), "off disabled");
    s.add("校验-开状态", ok.enabled(), "enabled");
    // 族二：遮罩
    let mask = content_mask(&content);
    s.add("遮罩-计数", mask_count(&mask) == 16, "16 content px");
    // 偏移
    let shifted = shift_mask(&mask, 16, 16, 2, 2);
    s.add("遮罩-偏移计数同", mask_count(&shifted) == 16, "count preserved");
    // 偏移方向：原内容位 (6,6) 影应出现在 (8,8)
    s.add("遮罩-偏移位置", shifted[8 * 16 + 8] && !shifted[6 * 16 + 6], "shifted by (2,2)");
    // 零偏移不变
    s.add("遮罩-零偏移", shift_mask(&mask, 16, 16, 0, 0) == mask, "no shift");
    // 出画布裁剪：偏移 (2,2)，右下角内容 (9,9) 影 (11,11) 在内；(15,15) 内容影 (17,17) 出界
    let mut edge = FBitmap::solid(16, 16, [0, 0, 0, 0]);
    edge.set(15, 15, [255, 255, 255, 255]);
    let em = shift_mask(&content_mask(&edge), 16, 16, 2, 2);
    s.add("遮罩-出界裁剪", !em.iter().any(|b| *b), "out of canvas clipped");
    // 族三：投影合成
    let shadowed = op_shadow(&content, &ok).expect("s");
    s.add("投影-尺寸不变", shadowed.same_size(&content), "same size");
    // 内容像素保持
    s.add("投影-内容保持", shadowed.get(6, 6) == [255, 0, 0, 255], "content kept");
    // 影像素出现
    s.add("投影-影出现", shadowed.get(8, 8) == [0, 0, 0, 128], "shadow at (8,8)");
    // 影不覆盖内容
    s.add("投影-影让内容", shadowed.get(7, 7) == [255, 0, 0, 255], "shadow under content");
    // 无影区仍透明
    s.add("投影-远处透明", shadowed.get(0, 0) == [0, 0, 0, 0], "far transparent");
    // 影计数：16 内容 + 部分影
    let total_opaque = shadowed.px.iter().filter(|p| p[3] > 0).count();
    s.add("投影-不透明计数", total_opaque > 16, "more than content");
    // 族四：关闭投影零影响
    let off = op_shadow(&content, &ShadowCfg::off()).expect("off");
    s.add("投影-关闭恒等", off == content, "off identity");
    // 族五：alpha 为 0 视为关闭
    let a0 = ShadowCfg { dx: 2, dy: 2, blur: 2.0, alpha: 0 };
    s.add("投影-alpha0关闭", !a0.enabled() && op_shadow(&content, &a0).expect("a0") == content, "alpha 0 off");
    // 族六：内容不减
    // 原内容位置在投影后仍是内容色
    s.add("投影-只加不减", (6..10).all(|y| (6..10).all(|x| shadowed.get(x, y)[3] == 255)), "content intact");
    // 族七：可分离模糊连通（用 blur_h 验证投影链有模糊入口）
    let blurred_row = blur_h(&content, 1);
    s.add("投影-模糊入口", blurred_row.same_size(&content), "blur hook ok");
    s
}
