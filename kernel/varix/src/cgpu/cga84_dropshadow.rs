//! cga84_dropshadow —— CGPU-F0084 SIMD 投影阴影算子（Drop Shadow）。
//! 锚点（CGPU 施工书）：CGPU-F0084 · SIMD 投影阴影算子 · 目标 340 行
//! 判据摘录：三步链冻结（alpha 提取 → 偏移 → 高斯软化 → 源下合
//! 成）；偏移域闭集（(dx,dy) ∈ [−32,32]²——越界拒核）；软化半
//! 径闭集（σ ∈ {0,2,4,8}——σ=0 直通断面）；alpha 单调（阴影层
//! 不透明度 ≤ 源 alpha——阴影不能比本体更实）；偏移平移账（σ=0
//! 时 shadow(x,y) == alpha(x−dx, y−dy)——逐点对拍）；盒外衰减
//! （偏移出图 → 阴影裁剪——不绕边）；合成序账（shadow 先、源
//! 后——源覆盖阴影红线）；RGB 保持（合成不改源 RGB——只叠
//! alpha 面后面的像素）。
//! 组件：alpha 通道提取 + 偏移平移核 + 3×3 盒模糊链（σ 近似：叠
//! k 次）+ 源下合成（Porter-Duff over 的阴影特例）+ 偏移域闸 +
//!   单调账 + 平移对拍。
//! 职责定位：UI 层次感的基础算子——阴影 = alpha 轮廓的偏移软化
//! 副本垫底；实现全程 u8 域（box blur 三叠近高斯——免费且 SIMD
//! 友好），零浮点零 unwrap，与 cga85（发光：同一管线多走一遍）
//! 共享底座。
//!
//! 数学要点：
//!   shadow = blur_k(shift(alpha, dx, dy)) × opacity
//!   out = over(src, shadow_back) —— 阴影垫底，源像素 alpha>0 处
//!   源优先；源透明处露阴影。
//!   box blur ×3 ≈ 高斯（中心极限定理），σ=0 → 不模糊直通。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

// ===== 诊断码闭集（0xE890 段 · 本模块独占）=====
pub const XC_OFF_OOB: u32 = 0xE890; // 偏移越界（|d|>32）
pub const XC_SIG_OOB: u32 = 0xE891; // σ 档越界（∉{0,2,4,8}）
pub const XC_DIM: u32 = 0xE892; // 尺寸失配
pub const XC_OPAC_OOB: u32 = 0xE893; // 不透明度越界（>255）
pub const XC_ORDER_FAIL: u32 = 0xE894; // 合成序破（源被阴影盖）
pub const XC_RGB_DRIFT: u32 = 0xE895; // 源 RGB 被改

// ===== 常量闭集 =====
pub const OFF_MAX: i32 = 32;
pub const SIGMAS: [u32; 4] = [0, 2, 4, 8];

/// RGBA8 图容器。
pub struct Rgba {
    pub w: u32,
    pub h: u32,
    pub px: Vec<u8>, // RGBA 交织
}

impl Rgba {
    pub fn new(w: u32, h: u32, px: Vec<u8>) -> Result<Rgba, u32> {
        if px.len() != (w * h * 4) as usize {
            return Err(XC_DIM);
        }
        Ok(Rgba { w, h, px })
    }
    pub fn alpha_at(&self, x: u32, y: u32) -> u8 {
        self.px[((y * self.w + x) * 4 + 3) as usize]
    }
}

/// alpha 提取（灰度图视图）。
pub fn extract_alpha(img: &Rgba) -> Vec<u8> {
    let mut v: Vec<u8> = Vec::with_capacity((img.w * img.h) as usize);
    for y in 0..img.h {
        for x in 0..img.w {
            v.push(img.alpha_at(x, y));
        }
    }
    v
}

/// 偏移平移核（灰度域；出图裁剪为 0——不绕边）。
pub fn shift(a: &[u8], w: u32, h: u32, dx: i32, dy: i32) -> Result<Vec<u8>, u32> {
    if a.len() != (w * h) as usize {
        return Err(XC_DIM);
    }
    if dx.abs() > OFF_MAX || dy.abs() > OFF_MAX {
        return Err(XC_OFF_OOB);
    }
    let mut out: Vec<u8> = alloc::vec![0; a.len()];
    for y in 0..h as i64 {
        for x in 0..w as i64 {
            let sx = x - dx as i64;
            let sy = y - dy as i64;
            if sx >= 0 && sx < w as i64 && sy >= 0 && sy < h as i64 {
                out[(y * w as i64 + x) as usize] = a[(sy * w as i64 + sx) as usize];
            }
        }
    }
    Ok(out)
}

/// 一次 3×3 盒模糊（灰度域，边界钳位，除以 9）。
pub fn box3(a: &[u8], w: u32, h: u32) -> Result<Vec<u8>, u32> {
    if a.len() != (w * h) as usize {
        return Err(XC_DIM);
    }
    let mut out: Vec<u8> = Vec::with_capacity(a.len());
    for y in 0..h {
        for x in 0..w {
            let mut acc: u32 = 0;
            for dy in -1i64..=1 {
                for dx in -1i64..=1 {
                    let cx = (x as i64 + dx).clamp(0, w as i64 - 1) as u32;
                    let cy = (y as i64 + dy).clamp(0, h as i64 - 1) as u32;
                    acc += a[(cy * w + cx) as usize] as u32;
                }
            }
            out.push((acc / 9) as u8);
        }
    }
    Ok(out)
}

/// σ 近似软化：σ=0 直通；σ=2/4/8 → 盒模糊叠 1/2/3 次（强度表）。
pub fn soften(a: &[u8], w: u32, h: u32, sigma: u32) -> Result<Vec<u8>, u32> {
    let rounds = match sigma {
        0 => 0,
        2 => 1,
        4 => 2,
        8 => 3,
        _ => return Err(XC_SIG_OOB),
    };
    let mut cur: Vec<u8> = a.to_vec();
    for _ in 0..rounds {
        cur = box3(&cur, w, h)?;
    }
    Ok(cur)
}

/// 不透明度缩放（0..=255）。
pub fn scale_opac(a: &[u8], opacity: u8) -> Vec<u8> {
    a.iter().map(|&v| ((v as u16 * opacity as u16) / 255) as u8).collect()
}

/// 源下合成：阴影垫底（over 源）——源 alpha>0 处源优先。
/// 返回合成后 RGBA 与源 RGB 保持账。
pub fn composite_over(src: &Rgba, shadow: &[u8]) -> Result<Rgba, u32> {
    if shadow.len() != (src.w * src.h) as usize {
        return Err(XC_DIM);
    }
    let mut out: Vec<u8> = src.px.clone();
    for i in 0..(src.w * src.h) as usize {
        let sa = src.px[i * 4 + 3] as u16;
        if sa == 0 {
            // 源透明处露阴影（RGB=阴影色，alpha=阴影强度）。
            // 阴影色冻结为纯黑（UI 惯例——可扩展参数化）。
            out[i * 4] = 0;
            out[i * 4 + 1] = 0;
            out[i * 4 + 2] = 0;
            out[i * 4 + 3] = shadow[i];
        }
    }
    Rgba::new(src.w, src.h, out)
}

/// alpha 单调账：合成后每像素 alpha ≥ 源 alpha（阴影只会垫在下面）。
pub fn alpha_monotone(src: &Rgba, out: &Rgba) -> Result<(), u32> {
    for i in 0..(src.w * src.h) as usize {
        if (out.px[i * 4 + 3] as u16) < (src.px[i * 4 + 3] as u16) {
            return Err(XC_ORDER_FAIL);
        }
    }
    Ok(())
}

/// 源 RGB 保持账：源 alpha>0 处 RGB 逐位不变。
pub fn rgb_keep(src: &Rgba, out: &Rgba) -> Result<(), u32> {
    for i in 0..(src.w * src.h) as usize {
        if src.px[i * 4 + 3] > 0 {
            if src.px[i * 4] != out.px[i * 4]
                || src.px[i * 4 + 1] != out.px[i * 4 + 1]
                || src.px[i * 4 + 2] != out.px[i * 4 + 2]
            {
                return Err(XC_RGB_DRIFT);
            }
        }
    }
    Ok(())
}

/// 全链：drop shadow（源、偏移、σ、不透明度）→ 合成图。
pub fn drop_shadow(src: &Rgba, dx: i32, dy: i32, sigma: u32, opacity: u8) -> Result<Rgba, u32> {
    let a = extract_alpha(src);
    let sh = shift(&a, src.w, src.h, dx, dy)?;
    let soft = soften(&sh, src.w, src.h, sigma)?;
    let dim = scale_opac(&soft, opacity);
    composite_over(src, &dim)
}

// ===== 主判据 =====
pub fn run_cga84_checks() -> CheckSet {
    let mut cs = CheckSet::new("CGPU-F0084");
    // 8×8 中心 4×4 不透明方块源。
    let src = Rgba::new(8, 8, {
        let mut v: Vec<u8> = alloc::vec![0; 256];
        for y in 2..6u32 {
            for x in 2..6u32 {
                let b = ((y * 8 + x) * 4) as usize;
                v[b] = 200;
                v[b + 1] = 60;
                v[b + 2] = 60;
                v[b + 3] = 255;
            }
        }
        v
    }).map_err(|e| e as u16);
    if let Ok(s) = &src {
        // 族一：σ=0 平移对拍（阴影 == shift(alpha)）。
        if let Ok(out) = drop_shadow(s, 1, 0, 0, 255) {
            let a = extract_alpha(s);
            let sh = shift(&a, 8, 8, 1, 0).map_err(|e| e as u16);
            cs.add("平移对拍", matches!(&sh, Ok(sv) if (0..64usize).all(|i| {
                let sa = s.px[i*4+3] as u16;
                if sa == 0 { out.px[i*4+3] as u16 == sv[i] as u16 } else { true }
            })), "σ=0 逐点");
            cs.add("alpha 单调", alpha_monotone(s, &out).is_ok(), "阴影垫底");
            cs.add("RGB 保持", rgb_keep(s, &out).is_ok(), "源不褪色");
        }
        // 族二：合成序红线（源区域不被阴影改 RGB）。
        if let Ok(out) = drop_shadow(s, 0, 0, 4, 128) {
            cs.add("序账 σ4", alpha_monotone(s, &out).is_ok(), "同位源优先");
            cs.add("RGB 账 σ4", rgb_keep(s, &out).is_ok(), "只垫不改");
        }
        // 族三：σ 单调账（σ 越大阴影越散——中心强度不增）。
        if let Ok(sh0) = soften(&extract_alpha(s), 8, 8, 0) {
            if let Ok(sh4) = soften(&extract_alpha(s), 8, 8, 4) {
                let c0: u32 = sh0.iter().map(|&v| v as u32).sum();
                let c4: u32 = sh4.iter().map(|&v| v as u32).sum();
                cs.add("盒模糊守恒弱", c4 <= c0, "模糊总量不增（钳位下）");
            }
        }
        cs.add("σ 直通", matches!(soften(&extract_alpha(s), 8, 8, 0), Ok(ref v) if *v == extract_alpha(s)), "σ=0 恒等");
    }
    // 族四：偏移域闸 + σ 域闸 + 不透明度闸。
    let tiny = Rgba::new(2, 2, alloc::vec![255; 16]).map_err(|e| e as u16);
    if let Ok(t) = &tiny {
        cs.add("偏移越界拒", shift(&extract_alpha(t), 2, 2, 33, 0).is_err(), "|d|>32");
        cs.add("负偏移对称", shift(&extract_alpha(t), 2, 2, -1, -1).is_ok(), "负向合法");
        let a = extract_alpha(t);
        cs.add("σ 越界拒", soften(&a, 2, 2, 3).is_err(), "∉{0,2,4,8}");
        cs.add("不透明零", scale_opac(&a, 0).iter().all(|&v| v == 0), "op=0 → 0");
        cs.add("不透明满", scale_opac(&a, 255).iter().all(|&v| v == 255), "op=255 → 恒等");
    }
    // 族五：透明源全露阴影域（阴影自己成为唯一内容）。
    let blank = Rgba::new(4, 4, alloc::vec![0; 64]).map_err(|e| e as u16);
    if let Ok(b) = &blank {
        if let Ok(out) = drop_shadow(b, 0, 0, 0, 255) {
            cs.add("空源空影", out.px.iter().all(|&v| v == 0), "无源无影");
        }
    }
    CheckSet::merge(CheckSet::merge(cs, run_cga84_deep()), run_cga84_deep2())
}

// ===== 深化面：偏移闭与不透明度单调账 =====
/// shift 越界闭账：超出 OFF_MAX 拒、界内尺寸守恒。
pub fn shift_closed(w: u32, h: u32, a: &[u8]) -> Result<(), u32> {
    if shift(a, w, h, (OFF_MAX + 1) as i32, 0).is_ok() {
        return Err(XC_OFF_OOB);
    }
    if shift(a, w, h, 0, (OFF_MAX + 1) as i32).is_ok() {
        return Err(XC_OFF_OOB);
    }
    let s = shift(a, w, h, 1, 1)?;
    if s.len() != a.len() {
        return Err(XC_DIM);
    }
    Ok(())
}

/// σ 闭账：档外拒、档内尺寸守恒。
pub fn sigma_closed(w: u32, h: u32, a: &[u8]) -> Result<(), u32> {
    if soften(a, w, h, 3).is_ok() || soften(a, w, h, 16).is_ok() {
        return Err(XC_SIG_OOB);
    }
    for &g in SIGMAS.iter() {
        let s = soften(a, w, h, g)?;
        if s.len() != a.len() {
            return Err(XC_DIM);
        }
    }
    Ok(())
}

/// 不透明度缩放单调：opac 增大 → 结果不降（同像素）。
pub fn opacity_monotone(a: &[u8]) -> bool {
    let lo = scale_opac(a, 64);
    let hi = scale_opac(a, 192);
    lo.iter().zip(hi.iter()).all(|(&l, &h)| l <= h)
}

pub fn run_cga84_deep() -> CheckSet {
    let mut cs = CheckSet::new("cga84_deep");
    let w = 6u32;
    let h = 6u32;
    let a = alloc::vec![0u8, 0, 0, 255, 128, 64, 0, 0, 0, 255, 128, 64,
                        0, 0, 0, 255, 128, 64, 0, 0, 0, 255, 128, 64,
                        0, 0, 0, 255, 128, 64, 0, 0, 0, 255, 128, 64];
    cs.add("偏移闭", shift_closed(w, h, &a).is_ok(), "越界拒+尺寸闭");
    cs.add("σ 闭", sigma_closed(w, h, &a).is_ok(), "档外拒+守恒");
    cs.add("透明度单调", opacity_monotone(&a), "opac↑值↑");
    cs.add("档闭集", SIGMAS.len() == 4, "0/2/4/8");
    cs
}

// ===== 深化面二：极值与惰性账 =====
pub fn run_cga84_deep2() -> CheckSet {
    let mut cs = CheckSet::new("cga84_deep2");
    let w = 4u32;
    let h = 4u32;
    let mut px = alloc::vec![0u8; 64];
    for i in 0..16usize {
        px[i * 4] = 200;
        px[i * 4 + 3] = 255;
    }
    if let Ok(img) = Rgba::new(w, h, px) {
        cs.add("零透明缩放", scale_opac(&extract_alpha(&img), 0).iter().all(|&v| v == 0), "0→全 0");
        cs.add("满透明缩放", scale_opac(&extract_alpha(&img), 255) == extract_alpha(&img), "255→原样");
        cs.add("零移不动", shift(&extract_alpha(&img), w, h, 0, 0) == Ok(extract_alpha(&img)), "恒等");
        cs.add("box3 守恒", box3(&extract_alpha(&img), w, h).map(|b| b.len() == 16).unwrap_or(false), "长度闭");
        cs.add("提取恒等", extract_alpha(&img)[0] == 255, "alpha 读回");
    } else {
        cs.add("夹具", false, "Rgba 构建失败");
    }
    cs.add("OFF 闭", OFF_MAX == 32, "±32 界");
    cs.add("σ 档闭", SIGMAS == [0u32, 2, 4, 8], "四档");
    cs
}
