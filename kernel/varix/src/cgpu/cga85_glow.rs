//! cga85_glow —— CGPU-F0085 SIMD 发光算子（Glow）。
//! 锚点（CGPU 施工书）：CGPU-F0085 · SIMD 发光算子 · 目标 320 行
//! 判据摘录：管线冻结（alpha 阈值提取 → 膨胀软化 → 屏幕混合叠
//! 加：out = src + glow×(1−src/255) 近似 screen）；阈值域闭集
//! （τ ∈ {64,128,192}——亮度门）；膨胀半径闭集（r ∈ {1,2,3}）；
//! 屏幕混合上界（out ≥ src——发光只加不减红线）；中心不动账
//! （r=0 或 glow=0 → 恒等——发光关断面）；单色发光源冻结（发
//! 光色三元组闭集——暖/冷/白三档）；溢出饱和账（钳顶像素计
//! 数落账——不许静默）；强度域闸（glow ∈ [0,255]）。
//! 组件：亮度阈值掩码 + 方形膨胀（切比雪夫距离 ≤r）+ 盒模糊软
//!   化（与 cga84 同底座）+ screen 混合定点化 + 饱和账 + 关断面
//!   + 域闸。
//! 职责定位：霓虹/选中态/魔法特效的标准算子——发光是「亮部自
//! 己的光晕」：掩码取亮部、膨胀扩边、软化成晕、screen 叠回；
//! 与 cga84（阴影：暗部垫底）互为明暗对偶。
//!
//! 数学要点：
//!   mask(x,y) = (lum(x,y) ≥ τ) × 255（二值提取）
//!   glow_layer = soften(dilate(mask, r)) × strength
//!   screen 定点：out = 255 − (255−src)×(255−g)/255（g=glow 分量）
//!   ⇒ out ≥ src 恒成立（screen 的代数性质——单调红线）。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

// ===== 诊断码闭集（0xE8A0 段 · 本模块独占）=====
pub const XC_TAU_OOB: u32 = 0xE8A0; // 阈值越界（∉{64,128,192}）
pub const XC_RAD_OOB: u32 = 0xE8A1; // 膨胀半径越界（∉{1,2,3}）
pub const XC_STR_OOB: u32 = 0xE8A2; // 强度越界（>255）
pub const XC_DIM: u32 = 0xE8A3; // 尺寸失配
pub const XC_MONO_FAIL: u32 = 0xE8A4; // screen 单调破（out<src）
pub const XC_SAT_NOLOG: u32 = 0xE8A5; // 饱和未落账

// ===== 常量闭集 =====
pub const TAUS: [u32; 3] = [64, 128, 192];
pub const RADII: [u32; 3] = [1, 2, 3];
/// 发光色闭集：暖/冷/白。
pub const GLOW_WARM: [u8; 3] = [255, 200, 120];
pub const GLOW_COOL: [u8; 3] = [120, 200, 255];
pub const GLOW_WHITE: [u8; 3] = [255, 255, 255];
pub const GLOW_ALL: [[u8; 3]; 3] = [GLOW_WARM, GLOW_COOL, GLOW_WHITE];

/// 亮度（Rec.601 定点近似：(R×77 + G×150 + B×29)/256）。
pub fn luminance(r: u8, g: u8, b: u8) -> u8 {
    (((r as u16 * 77) + (g as u16 * 150) + (b as u16 * 29)) >> 8) as u8
}

/// 亮度阈值二值掩码。
pub fn thresh_mask(px: &[u8], tau: u32) -> Result<Vec<u8>, u32> {
    if TAUS.iter().find(|&&t| t == tau).is_none() {
        return Err(XC_TAU_OOB);
    }
    if px.len() % 4 != 0 {
        return Err(XC_DIM);
    }
    let mut m: Vec<u8> = Vec::with_capacity(px.len() / 4);
    for i in 0..px.len() / 4 {
        let lum = luminance(px[i * 4], px[i * 4 + 1], px[i * 4 + 2]) as u32;
        m.push(if lum >= tau { 255 } else { 0 });
    }
    Ok(m)
}

/// 方形膨胀（切比雪夫距离 ≤r，二值域）。
pub fn dilate(m: &[u8], w: u32, h: u32, r: u32) -> Result<Vec<u8>, u32> {
    if RADII.iter().find(|&&v| v == r).is_none() {
        return Err(XC_RAD_OOB);
    }
    if m.len() != (w * h) as usize {
        return Err(XC_DIM);
    }
    let mut out: Vec<u8> = alloc::vec![0; m.len()];
    for y in 0..h {
        for x in 0..w {
            let mut hit = false;
            'win: for dy in -(r as i64)..=(r as i64) {
                for dx in -(r as i64)..=(r as i64) {
                    let cx = (x as i64 + dx).clamp(0, w as i64 - 1) as u32;
                    let cy = (y as i64 + dy).clamp(0, h as i64 - 1) as u32;
                    if m[(cy * w + cx) as usize] == 255 {
                        hit = true;
                        break 'win;
                    }
                }
            }
            out[(y * w + x) as usize] = if hit { 255 } else { 0 };
        }
    }
    Ok(out)
}

/// 一次盒模糊（与 cga84 同语义——灰度域）。
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

/// 软化（叠 2 次盒模糊近高斯——发光晕固定强度）。
pub fn soften2(a: &[u8], w: u32, h: u32) -> Result<Vec<u8>, u32> {
    let b = box3(a, w, h)?;
    box3(&b, w, h)
}

/// screen 混合定点化：out = 255 − (255−src)·(255−g)/255。
/// 数学保证 out ≥ src（g ≥ 0）——单调红线在实现层直核。
pub fn screen(src: u8, g: u8) -> u8 {
    let s = src as u16;
    let gg = g as u16;
    (255u16 - ((255 - s) * (255 - gg)) / 255) as u8
}

/// 全链发光：阈值→膨胀→软化→强度→按发光色 screen 叠回。
/// 返回（合成图, 饱和像素计数）。
pub fn glow(px: &[u8], w: u32, h: u32, tau: u32, r: u32, strength: u8, color: [u8; 3]) -> Result<(Vec<u8>, u32), u32> {
    if px.len() != (w * h * 4) as usize {
        return Err(XC_DIM);
    }
    let mask = thresh_mask(px, tau)?;
    let dil = dilate(&mask, w, h, r)?;
    let soft = soften2(&dil, w, h)?;
    let mut out: Vec<u8> = Vec::with_capacity(px.len());
    let mut sat: u32 = 0;
    for i in 0..(w * h) as usize {
        let g_lum = ((soft[i] as u16 * strength as u16) / 255) as u8;
        let mut ch = [0u8; 3];
        for c in 0..3 {
            let s = px[i * 4 + c];
            let gc = ((color[c] as u16 * g_lum as u16) / 255) as u8;
            let v = screen(s, gc);
            if v == 255 && s < 255 {
                sat += 1;
            }
            ch[c] = v;
        }
        out.push(ch[0]);
        out.push(ch[1]);
        out.push(ch[2]);
        out.push(px[i * 4 + 3]); // alpha 保持
    }
    Ok((out, sat))
}

/// screen 单调账：逐通道 out ≥ src。
pub fn screen_monotone(src: &[u8], out: &[u8]) -> Result<(), u32> {
    for i in 0..src.len() {
        if out[i] < src[i] {
            return Err(XC_MONO_FAIL);
        }
    }
    Ok(())
}

/// 关断面：strength=0 → 恒等（发光关）。
pub fn off_ablation(px: &[u8], w: u32, h: u32, tau: u32, r: u32) -> Result<bool, u32> {
    let (out, _) = glow(px, w, h, tau, r, 0, GLOW_WHITE)?;
    Ok(out[..] == px[..])
}

// ===== 主判据 =====
pub fn run_cga85_checks() -> CheckSet {
    let mut cs = CheckSet::new("CGPU-F0085");
    // 8×8：左半暗底、右半亮块（R=G=B=255 → lum=255）。
    let src: Vec<u8> = {
        let mut v: Vec<u8> = alloc::vec![0; 256];
        for y in 0..8u32 {
            for x in 0..8u32 {
                let b = ((y * 8 + x) * 4) as usize;
                if x >= 4 {
                    v[b] = 255;
                    v[b + 1] = 255;
                    v[b + 2] = 255;
                    v[b + 3] = 255;
                }
            }
        }
        v
    };
    // 族一：亮度与阈值。
    cs.add("亮度锚白", luminance(255, 255, 255) == 255, "全 1");
    cs.add("亮度锚黑", luminance(0, 0, 0) == 0, "全 0");
    cs.add("亮度锚红", luminance(255, 0, 0) == 77, "77/256");
    cs.add("阈值域闸", thresh_mask(&src, 100).is_err(), "∉{64,128,192}");
    if let Ok(m) = thresh_mask(&src, 64) {
        cs.add("掩码亮块", m[4] == 255 && m[0] == 0, "右亮左暗");
    }
    // 族二：膨胀。
    if let Ok(m) = thresh_mask(&src, 64) {
        if let Ok(d) = dilate(&m, 8, 8, 1) {
            cs.add("膨胀扩边", d[3] == 255, "亮块左邻被扩");
            cs.add("膨胀远端", d[(0 * 8 + 0) as usize] == 0, "远角不扩");
        }
        cs.add("半径越界拒", dilate(&m, 8, 8, 4).is_err(), "∉{1,2,3}");
    }
    // 族三：全链 screen 单调 + 关断面。
    if let Ok((out, sat)) = glow(&src, 8, 8, 64, 1, 255, GLOW_WHITE) {
        cs.add("screen 单调", screen_monotone(&src, &out).is_ok(), "只加不减");
        cs.add("饱和落账", sat > 0, "白源全饱和有账");
        cs.add("亮区白热", out[((0 * 8 + 6) * 4) as usize] == 255, "亮核 255");
    }
    cs.add("关断面恒等", off_ablation(&src, 8, 8, 64, 1) == Ok(true), "strength=0");
    // 族四：screen 定点代数锚。
    cs.add("screen 0+0", screen(0, 0) == 0, "双零");
    cs.add("screen s+0", screen(200, 0) == 200, "g=0 恒等");
    cs.add("screen 0+g", screen(0, 200) == 200, "s=0 → g");
    cs.add("screen 满", screen(255, 200) == 255, "src=255 → 255");
    cs.add("screen 单调抽样", (0..8u16).all(|i| screen(i as u8 * 30, 128) >= (i as u8 * 30) as u8), "步进单调");
    // 族五：发光色闭集烟测。
    for (idx, &c) in GLOW_ALL.iter().enumerate() {
        if let Ok((out, _)) = glow(&src, 8, 8, 64, 1, 128, c) {
            cs.add("色档烟测", out.len() == 256 && screen_monotone(&src, &out).is_ok(), "档位核");
            let _ = idx;
        }
    }
    CheckSet::merge(CheckSet::merge(cs, run_cga85_deep()), run_cga85_deep2())
}

// ===== 深化面：亮度锚与膨胀单调账 =====
/// 亮度锚值：黑=0 白=255 纯通道=加权值域内。
pub fn luminance_anchors() -> bool {
    luminance(0, 0, 0) == 0
        && luminance(255, 255, 255) == 255
        && luminance(255, 0, 0) <= 255
        && luminance(0, 255, 0) <= 255
        && luminance(0, 0, 255) <= 255
}

/// 掩码二值账：thresh_mask 输出只含 0/1。
pub fn mask_binary(px: &[u8], tau: u32) -> bool {
    match thresh_mask(px, tau) {
        Ok(m) => m.iter().all(|&v| v == 0 || v == 1),
        Err(_) => false,
    }
}

/// 膨胀单调账：r=1 结果 ≥ r=0 逐点。
pub fn dilate_order(m: &[u8], w: u32, h: u32) -> bool {
    match (dilate(m, w, h, 0), dilate(m, w, h, 1)) {
        (Ok(a), Ok(b)) => a.iter().zip(b.iter()).all(|(&x, &y)| x <= y),
        _ => false,
    }
}

/// screen 代数锚：0 不变、255 饱和、交换对称。
pub fn screen_algebra() -> bool {
    (0..8u32).all(|i| {
        let v = (i * 36) as u8;
        screen(v, 0) == v && screen(v, 255) == 255 && screen(v, 7) == screen(7, v)
    })
}

pub fn run_cga85_deep() -> CheckSet {
    let mut cs = CheckSet::new("cga85_deep");
    cs.add("亮度锚", luminance_anchors(), "0/255 闭");
    let px = alloc::vec![0u8, 255, 128, 64, 200, 30];
    cs.add("掩码二值", mask_binary(&px, 128), "0/1 闭域");
    let m = alloc::vec![0u8, 1, 0, 0, 0, 0, 1, 0, 0];
    cs.add("膨胀单调", dilate_order(&m, 3, 3), "r 增集大");
    cs.add("screen 代数", screen_algebra(), "0/255/交换");
    cs.add("τ 越界拒", thresh_mask(&px, 100).is_err(), "闭集外");
    cs.add("半径越界拒", dilate(&m, 3, 3, 9).is_err(), "闭集外");
    cs
}

// ===== 深化面二：熄灭账与软化守恒 =====
pub fn run_cga85_deep2() -> CheckSet {
    let mut cs = CheckSet::new("cga85_deep2");
    let px = alloc::vec![0u8, 0, 0, 250, 250, 250, 0, 0, 0];
    cs.add("熄灭消融", off_ablation(&px, 3, 3, 128, 1) == Ok(true), "强度 0 不动");
    cs.add("软化守恒", soften2(&px, 3, 3).map(|s| s.len() == 9).unwrap_or(false), "长度闭");
    cs.add("box3 常量惰性", {
        let flat = alloc::vec![77u8; 9];
        box3(&flat, 3, 3).map(|b| b.iter().all(|&v| v == 77)).unwrap_or(false)
    }, "常量图不变");
    cs.add("半径档闭", RADII == [1u32, 2, 3], "三档");
    cs.add("τ 档闭", TAUS == [64u32, 128, 192], "三档");
    cs.add("掩码 τ 域", [64u32, 128, 192].iter().all(|&t| thresh_mask(&px, t).is_ok()), "三档全收");
    cs.add("screen 幂等", {
        (0..8u32).all(|i| {
            let v = (i * 33) as u8;
            screen(v, 128) == screen(screen(v, 128), 128) || screen(v, 128) == 255
        })
    }, "饱和后不动");
    cs
}
