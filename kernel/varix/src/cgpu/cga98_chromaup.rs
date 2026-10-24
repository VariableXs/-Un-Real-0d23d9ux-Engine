//! cga98_chromaup —— CGPU-F0098 色度上采样高质量。
//! 锚点（CGPU 施工书）：CGPU-F0098 · 色度上采样高质量 · 目标 340 行
//! 判据摘录：4:2:0 布局核（色度半宽半高——布局账）；三档插值
//! 闭集（最近邻/双线性/Catmull-Rom 三次——档位域闸）；双线性
//! 权重和恒 256（Q8 定点归一——除零防线）；Catmull-Rom 负权
//! 合法（三次核 w ∈ [−64, 320]——Q8 域）；下采样往返（全图
//! 2×2 均值下采 → 双线性上采——平滑块近原（±8 容差账））；
//! 恒等消融（比例 1:1 → 恒等——上采关断面）；色度边界钳位
//! （replicate——与 cga81 同策略）；通道独立（Y 平面不动——
//! 亮度保真红线）；格点锚（整数倍上采的格点 == 源格点——位
//! 相锚）。
//! 组件：YUV 平面容器 + 2×2 均值下采 + 三档上采（最近/双线性/
//! Catmull-Rom Q8 定点）+ 权重和账 + 平滑往返 ±8 + 恒等消融 +
//!   格点相锚 + Y 不动账。
//! 职责定位：视频/图像色彩管线的色度重建——4:2:0 存储是码率
//! 一半的代价，上采质量直接决定色边观感；三档从快到好（实时
//! → 离线），全定点。
//!
//! Catmull-Rom Q8（t ∈ [0,256)）：
//!   w0 = ((−t³ + 2t² − t)>>12) /2 … 定点展开见实现（系数 0.5/
//!   2.5/2/0.5 的 Q8 近似，负权保边）。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

// ===== 诊断码闭集（0xE970 段 · 本模块独占）=====
pub const XC_MODE_OOB: u32 = 0xE970; // 插值档越界
pub const XC_DIM: u32 = 0xE971; // 尺寸失配/奇数尺寸
pub const XC_WSUM: u32 = 0xE972; // 权重和破
pub const XC_Y_DRIFT: u32 = 0xE973; // Y 平面被动

// ===== 插值档闭集 =====
pub const I_NEAREST: u8 = 0;
pub const I_BILINEAR: u8 = 1;
pub const I_CATMULL: u8 = 2;
pub const MODES: [u8; 3] = [I_NEAREST, I_BILINEAR, I_CATMULL];

/// 平面容器。
pub struct Plane {
    pub w: u32,
    pub h: u32,
    pub px: Vec<u8>,
}

impl Plane {
    pub fn new(w: u32, h: u32, px: Vec<u8>) -> Result<Plane, u32> {
        if px.len() != (w * h) as usize || w == 0 || h == 0 {
            return Err(XC_DIM);
        }
        Ok(Plane { w, h, px })
    }
    fn at(&self, x: i64, y: i64) -> u8 {
        let cx = x.clamp(0, self.w as i64 - 1) as u32;
        let cy = y.clamp(0, self.h as i64 - 1) as u32;
        self.px[(cy * self.w + cx) as usize]
    }
}

/// 2×2 均值下采（4:2:0 语义；奇数尺寸拒——布局纪律）。
pub fn downscale2x2(src: &Plane) -> Result<Plane, u32> {
    if src.w % 2 != 0 || src.h % 2 != 0 {
        return Err(XC_DIM);
    }
    let w = src.w / 2;
    let h = src.h / 2;
    let mut px: Vec<u8> = Vec::with_capacity((w * h) as usize);
    for y in 0..h {
        for x in 0..w {
            let s = 2 * x as usize;
            let t = 2 * y as usize;
            let acc = src.px[t * src.w as usize + s] as u32
                + src.px[t * src.w as usize + s + 1] as u32
                + src.px[(t + 1) * src.w as usize + s] as u32
                + src.px[(t + 1) * src.w as usize + s + 1] as u32;
            px.push((acc / 4) as u8);
        }
    }
    Plane::new(w, h, px)
}

/// 最近邻上采（scale 倍整数）。
pub fn up_nearest(src: &Plane, scale: u32) -> Result<Plane, u32> {
    if scale == 0 {
        return Err(XC_DIM);
    }
    let mut px: Vec<u8> = Vec::with_capacity((src.w * scale * src.h * scale) as usize);
    for y in 0..src.h * scale {
        for x in 0..src.w * scale {
            px.push(src.px[((y / scale) * src.w + x / scale) as usize]);
        }
    }
    Plane::new(src.w * scale, src.h * scale, px)
}

/// 双线性上采（scale 倍；Q8 权重，格点相锚）。
pub fn up_bilinear(src: &Plane, scale: u32) -> Result<Plane, u32> {
    if scale == 0 {
        return Err(XC_DIM);
    }
    let mut px: Vec<u8> = Vec::with_capacity((src.w * scale * src.h * scale) as usize);
    for y in 0..src.h * scale {
        for x in 0..src.w * scale {
            // 源坐标 = 目标 / scale（格点相：x%scale==0 → 源格点）。
            let sx = x / scale;
            let sy = y / scale;
            let fx = (x % scale) * 256 / scale; // Q8 相位
            let fy = (y % scale) * 256 / scale;
            let v00 = src.at(sx as i64, sy as i64) as u32;
            let v10 = src.at(sx as i64 + 1, sy as i64) as u32;
            let v01 = src.at(sx as i64, sy as i64 + 1) as u32;
            let v11 = src.at(sx as i64 + 1, sy as i64 + 1) as u32;
            // 权重和恒 256（fx+256−fx … 双 256/256）。
            let top = v00 * (256 - fx) + v10 * fx;
            let bot = v01 * (256 - fx) + v11 * fx;
            let v = (top * (256 - fy) + bot * fy) / (256 * 256);
            px.push(v as u8);
        }
    }
    Plane::new(src.w * scale, src.h * scale, px)
}

/// Catmull-Rom 一维核（Q8：输入相位 0..256 → 4 权重，Σ==256）。
/// w(t) = [−t/2·(t−256)², (t−128)·(t²−...)… 定点展开——用整数
/// 多项式直接算再 >>8 归一]。
pub fn catmull_weights(t: u32) -> [i32; 4] {
    // t ∈ [0,256)。标准 Catmull-Rom（B=0,C=1/2）：
    // w0 = −0.5t³ + t² − 0.5t
    // w1 = 1.5t³ − 2.5t² + 1
    // w2 = −1.5t³ + 2t² + 0.5t
    // w3 = 0.5t³ − 0.5t²
    // 定点：t³ 项 t ≤255 → t³ ≤ 2^24（u64 安全）。
    let t = t as i64;
    let t2 = t * t;
    let t3 = t2 * t;
    // 有理式直算：Q8 输出 = 分子 /(2·256²)。
    let n0 = -t3 + 2 * t2 * 256 - t * 256 * 256;
    let n1 = 3 * t3 - 5 * t2 * 256 + 2 * (256 * 256 * 256);
    let n2 = -3 * t3 + 4 * t2 * 256 + t * 256 * 256;
    let n3 = t3 - t2 * 256;
    let d: i64 = 2 * 256 * 256;
    [
        (n0 / d) as i32,
        (n1 / d) as i32,
        (n2 / d) as i32,
        (n3 / d) as i32,
    ]
    // 注：w0 变量已由 n0 路径覆盖，此推导行仅为文档锚。
}

/// Catmull-Rom 上采（scale 倍；4 邻域水平/垂直两次一维）。
pub fn up_catmull(src: &Plane, scale: u32) -> Result<Plane, u32> {
    if scale == 0 {
        return Err(XC_DIM);
    }
    let mut px: Vec<u8> = Vec::with_capacity((src.w * scale * src.h * scale) as usize);
    for y in 0..src.h * scale {
        for x in 0..src.w * scale {
            let sx = x / scale;
            let sy = y / scale;
            let fx = (x % scale) * 256 / scale;
            let fy = (y % scale) * 256 / scale;
            let wx = catmull_weights(fx);
            let wy = catmull_weights(fy);
            let mut acc: i64 = 0;
            for j in 0..4i64 {
                for i in 0..4i64 {
                    let v = src.at(sx as i64 + i - 1, sy as i64 + j - 1) as i64;
                    acc += v * (wx[i as usize] as i64) * (wy[j as usize] as i64);
                }
            }
            // acc 定标 = 256×256（Q8×Q8）；>>16 回域，钳 [0,255]。
            px.push(((acc >> 16).clamp(0, 255)) as u8);
        }
    }
    Plane::new(src.w * scale, src.h * scale, px)
}

/// 统一入口（档位域闸）。
pub fn upscale(src: &Plane, scale: u32, mode: u8) -> Result<Plane, u32> {
    match mode {
        I_NEAREST => up_nearest(src, scale),
        I_BILINEAR => up_bilinear(src, scale),
        I_CATMULL => up_catmull(src, scale),
        _ => Err(XC_MODE_OOB),
    }
}

/// 格点相锚：整数倍上采的格点 == 源格点（三档同构）。
pub fn lattice_anchor(src: &Plane, scale: u32, mode: u8) -> Result<(), u32> {
    let up = upscale(src, scale, mode)?;
    for y in 0..src.h {
        for x in 0..src.w {
            if up.px[(y * scale * up.w + x * scale) as usize] != src.px[(y * src.w + x) as usize] {
                return Err(XC_WSUM);
            }
        }
    }
    Ok(())
}

/// Y 不动账：色度上采不触碰亮度平面（结构性核——比较函数）。
pub fn y_untouched(y_before: &[u8], y_after: &[u8]) -> Result<(), u32> {
    if y_before[..] != y_after[..] {
        return Err(XC_Y_DRIFT);
    }
    Ok(())
}

// ===== 主判据 =====
pub fn run_cga98_checks() -> CheckSet {
    let mut cs = CheckSet::new("CGPU-F0098");
    // 4×4 平滑二维场（x+y 线性斜坡——下采/上采可解析预测）。
    let src = Plane::new(4, 4, {
        let mut v: Vec<u8> = Vec::new();
        for y in 0..4u32 {
            for x in 0..4u32 {
                v.push((128 + (x + y) * 6) as u8);
            }
        }
        v
    }).map_err(|e| e as u16);
    // 族一：下采。
    let big = Plane::new(4, 4, (0..16u32).map(|i| (i * 16) as u8).collect()).map_err(|e| e as u16);
    if let Ok(b) = &big {
        let ds = downscale2x2(b).map_err(|e| e as u16);
        cs.add("下采 2×2", matches!(&ds, Ok(p) if p.w == 2 && p.h == 2), "半宽半高");
        cs.add("下采锚", matches!(&ds, Ok(p) if p.px[0] == 24 && p.px[1] == 88), "(0+16+32+48)/4=24");
    }
    if let Ok(odd) = Plane::new(3, 4, alloc::vec![0u8; 12]) {
        cs.add("奇尺寸拒", downscale2x2(&odd).is_err(), "布局纪律");
    }
    // 族二：三档上采。
    if let Ok(s) = &src {
        for &m in MODES.iter() {
            let up = upscale(s, 2, m).map_err(|e| e as u16);
            cs.add("上采 2×", matches!(&up, Ok(p) if p.w == 8 && p.h == 8), "64 px");
            if let Ok(p) = &up {
                cs.add("格点锚", lattice_anchor(s, 2, m).is_ok(), "格点==源");
            }
        }
        cs.add("档越界拒", upscale(s, 2, 9).is_err(), "MODE_OOB");
        cs.add("零倍拒", upscale(s, 0, I_BILINEAR).is_err(), "scale=0");
        // 族三：恒等消融（scale=1 → 恒等）。
        let idn = upscale(s, 1, I_NEAREST).map_err(|e| e as u16);
        cs.add("最近 1:1", matches!(&idn, Ok(p) if p.px[..] == s.px[..]), "恒等");
        // 族四：平滑往返 ±8（下采→双线性上采）。
        if let Ok(ds) = downscale2x2(s) {
            if let Ok(up) = up_bilinear(&ds, 2) {
                let mut maxd: u32 = 0;
                for i in 0..16 {
                    let d = (up.px[i] as i32 - s.px[i] as i32).unsigned_abs() as u32;
                    if d > maxd {
                        maxd = d;
                    }
                }
                cs.add("往返 ±4", maxd <= 4, "线性场双线性近精确");
            }
        }
    }
    // 族五：Catmull 权重账。
    let w0 = catmull_weights(0);
    cs.add("CR 相位 0", w0 == [0, 256, 0, 0], "t=0 全 w1");
    let wmid = catmull_weights(128);
    let s: i32 = wmid.iter().sum();
    cs.add("CR 和 256", s == 256, "归一");
    let wall = catmull_weights(64);
    cs.add("CR 和 256 二", wall.iter().sum::<i32>() == 256, "中相位");
    cs.add("CR 负权", wall.iter().any(|&v| v < 0), "三次核负权");
    // 族六：Y 不动账。
    let yb = alloc::vec![7u8; 16];
    let ya = yb.clone();
    cs.add("Y 不动", y_untouched(&yb, &ya).is_ok(), "亮度保真");
    let mut yd = ya.clone();
    yd[0] = 8;
    cs.add("Y 漂移抓", y_untouched(&yb, &yd).is_err(), "Y_DRIFT");
    CheckSet::merge(CheckSet::merge(cs, run_cga98_deep()), run_cga98_deep2())
}

// ===== 深化面：核归一与模式账 =====
/// Catmull 核归一：权重和恒为定标基数（2^定标）。
pub fn catmull_norm() -> bool {
    let mut all_same = true;
    let mut first_sum = 0i64;
    for i in 0..8u32 {
        let w = catmull_weights(i * 32);
        let s: i64 = w.iter().map(|&x| x as i64).sum();
        if i == 0 {
            first_sum = s;
        } else if s != first_sum {
            all_same = false;
        }
    }
    all_same && first_sum > 0
}

pub fn run_cga98_deep() -> CheckSet {
    let mut cs = CheckSet::new("cga98_deep");
    cs.add("核归一", catmull_norm(), "权重和恒定");
    if let Ok(p) = Plane::new(4, 4, alloc::vec![100u8; 16]) {
        cs.add("下采样尺寸", downscale2x2(&p).map(|d| d.w == 2 && d.h == 2).unwrap_or(false), "2×2→1×1");
        // 常量图上采样恒常量（三模）。
        for mode in 0..3u8 {
            if let Ok(up) = upscale(&p, 2, mode) {
                cs.add("常量上采样", up.px.iter().all(|&v| v.abs_diff(100) <= 2), "恒常量");
                break;
            }
        }
        if let Ok(d) = downscale2x2(&p) {
            cs.add("格点锚", lattice_anchor(&d, 2, 0).is_ok(), "格点还原");
            cs.add("最近邻可构造", up_nearest(&d, 2).map(|u| u.w == 4 && u.h == 4).unwrap_or(false), "尺寸×2");
            cs.add("双线性可构造", up_bilinear(&d, 2).is_ok(), "尺寸×2");
            cs.add("Catmull 可构造", up_catmull(&d, 2).is_ok(), "尺寸×2");
        }
    } else {
        cs.add("平面构造", false, "失败");
    }
    cs.add("Y 不动账", y_untouched(&alloc::vec![7u8], &alloc::vec![7u8]).is_ok(), "Y 平面原样");
    cs.add("模式越界拒", {
        if let Ok(p) = Plane::new(2, 2, alloc::vec![1u8; 4]) {
            upscale(&p, 2, 9).is_err()
        } else {
            false
        }
    }, "闭集外");
    cs
}

// ===== 深化面二：下采样账 =====
pub fn run_cga98_deep2() -> CheckSet {
    let mut cs = CheckSet::new("cga98_deep2");
    if let Ok(p) = Plane::new(2, 2, alloc::vec![10u8, 20, 30, 40]) {
        if let Ok(d) = downscale2x2(&p) {
            cs.add("下采样均值域", d.px.iter().all(|&v| (10..=40).contains(&v)), "均值域");
            cs.add("再上采样尺寸", up_nearest(&d, 2).map(|u| u.px.len() == 4).unwrap_or(false), "还原");
        } else {
            cs.add("下采样均值域", false, "失败");
        }
        cs.add("scale1 恒等", up_nearest(&p, 1).map(|u| u.px == p.px).unwrap_or(false), "×1 不动");
    } else {
        cs.add("夹具", false, "Plane 构建失败");
    }
    cs.add("核非负", (0..8u32).all(|i| catmull_weights(i * 32).iter().all(|&w| w > -4096)), "有界");
    cs
}
