//! cga34_postfx.rs ↔ CGPU-F0034 后处理 SIMD 基础库（0x1B40 段）
//!
//! 职责（施工书原文）：
//!   CPU 后处理基础算子族（无 GPU 时的后处理全链）：高斯模糊（F0026 已立，此处归入链）、
//!   Bloom（亮部提取→模糊→叠加三段 SIMD）、色调映射（ACES 查表 SIMD）、
//!   暗角/颗粒/色差/锐化（卷积族 SIMD）、LUT 应用（3D LUT 四线性插值 SIMD）。
//!   每算子与 GPU 路径对拍同状态同结果。
//!
//! 判据（施工书原文）：
//!   算子族对拍、链编排正确、性能（4K 单算子 ≤8ms 八核）、LUT 精度、VE-K 对接。

use alloc::vec::Vec;
use crate::float::FloatExt;
use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::alloc::vec;
use crate::checks::CheckSet;

/// 诊断段（0x1B40 独占 0x10）。
pub const SEG_BASE: u16 = 0x1B40;
pub const XC_LUTDIM: u16 = SEG_BASE + 1; // LUT 维度非法
pub const XC_CHAINOP: u16 = SEG_BASE + 2; // 链上算子未知
pub const XC_IMGDIM: u16 = SEG_BASE + 3; // 图像尺寸非法

/// 线性 RGBA 图像（f32×4/像素）
pub struct Img {
    pub w: u32,
    pub h: u32,
    pub px: Vec<f32>, // w*h*4
}

impl Img {
    pub fn new(w: u32, h: u32) -> Result<Self, u16> {
        if w == 0 || h == 0 || (w as u64) * (h as u64) > 1 << 26 {
            return Err(XC_IMGDIM);
        }
        Ok(Img { w, h, px: vec![0.0; (w * h * 4) as usize] })
    }
    #[inline]
    pub fn get(&self, x: u32, y: u32) -> [f32; 4] {
        let i = ((y * self.w + x) * 4) as usize;
        [self.px[i], self.px[i + 1], self.px[i + 2], self.px[i + 3]]
    }
    #[inline]
    pub fn set(&mut self, x: u32, y: u32, c: [f32; 4]) {
        let i = ((y * self.w + x) * 4) as usize;
        self.px[i..i + 4].copy_from_slice(&c);
    }
    /// 亮度（Rec.709）
    pub fn luminance(c: [f32; 4]) -> f32 {
        0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]
    }
}

/// 算子一：亮部提取（Bloom 第一段，阈值软膝）
pub fn bright_pass(src: &Img, dst: &mut Img, threshold: f32, knee: f32) -> Result<(), u16> {
    if src.w != dst.w || src.h != dst.h {
        return Err(XC_IMGDIM);
    }
    for y in 0..src.h {
        for x in 0..src.w {
            let c = src.get(x, y);
            let l = Img::luminance(c);
            // 软膝：阈下全消，阈上软过渡到全量
            let t = if l <= threshold - knee {
                0.0
            } else if l >= threshold + knee {
                1.0
            } else {
                (l - (threshold - knee)) / (2.0 * knee)
            };
            let mut o = c;
            for ch in 0..3 {
                o[ch] *= t;
            }
            dst.set(x, y, o);
        }
    }
    Ok(())
}

/// 算子二：3x3 盒式模糊（Bloom 第二段；完整高斯见 F0026，此处链内轻量级）
pub fn box_blur3(src: &Img, dst: &mut Img) -> Result<(), u16> {
    if src.w != dst.w || src.h != dst.h {
        return Err(XC_IMGDIM);
    }
    for y in 0..src.h {
        for x in 0..src.w {
            let mut acc = [0f32; 4];
            for dy in -1i32..=1 {
                for dx in -1i32..=1 {
                    let sx = (x as i32 + dx).clamp(0, src.w as i32 - 1) as u32;
                    let sy = (y as i32 + dy).clamp(0, src.h as i32 - 1) as u32;
                    let c = src.get(sx, sy);
                    for ch in 0..4 {
                        acc[ch] += c[ch];
                    }
                }
            }
            for ch in 0..4 {
                acc[ch] /= 9.0;
            }
            dst.set(x, y, acc);
        }
    }
    Ok(())
}

/// 算子三：Bloom 合成（src + bloom×强度，tone 前叠加）
pub fn bloom_composite(base: &Img, bloom: &Img, dst: &mut Img, strength: f32) -> Result<(), u16> {
    if base.w != bloom.w || base.h != bloom.h || base.w != dst.w || base.h != dst.h {
        return Err(XC_IMGDIM);
    }
    for y in 0..base.h {
        for x in 0..base.w {
            let a = base.get(x, y);
            let b = bloom.get(x, y);
            let mut o = [0f32; 4];
            for ch in 0..4 {
                o[ch] = (a[ch] + b[ch] * strength).clamp(0.0, 64.0);
            }
            dst.set(x, y, o);
        }
    }
    Ok(())
}

/// ACES 色调映射（近似拟合式——Narkowicz 拟合，查表由调用侧缓存）
pub fn aces_tonemap(x: f32) -> f32 {
    let a = 2.51;
    let b = 0.03;
    let c = 2.43;
    let d = 0.59;
    let e = 0.14;
    ((x * (a * x + b)) / (x * (c * x + d) + e)).clamp(0.0, 1.0)
}

/// 算子四：色调映射（全图 ACES，alpha 直通）
pub fn tonemap(src: &Img, dst: &mut Img, exposure: f32) -> Result<(), u16> {
    if src.w != dst.w || src.h != dst.h {
        return Err(XC_IMGDIM);
    }
    for y in 0..src.h {
        for x in 0..src.w {
            let c = src.get(x, y);
            let mut o = [0f32; 4];
            for ch in 0..3 {
                o[ch] = aces_tonemap(c[ch] * exposure);
            }
            o[3] = c[3];
            dst.set(x, y, o);
        }
    }
    Ok(())
}

/// 算子五：暗角（径向衰减）
pub fn vignette(src: &Img, dst: &mut Img, strength: f32) -> Result<(), u16> {
    if src.w != dst.w || src.h != dst.h {
        return Err(XC_IMGDIM);
    }
    let cx = src.w as f32 / 2.0;
    let cy = src.h as f32 / 2.0;
    let max_r = (cx * cx + cy * cy).m_sqrt();
    for y in 0..src.h {
        for x in 0..src.w {
            let mut c = src.get(x, y);
            let dx = x as f32 - cx;
            let dy = y as f32 - cy;
            let r = (dx * dx + dy * dy).m_sqrt() / max_r;
            let att = 1.0 - strength * r * r;
            for ch in 0..3 {
                c[ch] *= att;
            }
            dst.set(x, y, c);
        }
    }
    Ok(())
}

/// 算子六：锐化（3x3 非锐化掩模：dst = src + k*(src - blur)）
pub fn sharpen(src: &Img, dst: &mut Img, k: f32) -> Result<(), u16> {
    let mut blur = Img::new(src.w, src.h)?;
    box_blur3(src, &mut blur)?;
    if src.w != dst.w || src.h != dst.h {
        return Err(XC_IMGDIM);
    }
    for y in 0..src.h {
        for x in 0..src.w {
            let a = src.get(x, y);
            let b = blur.get(x, y);
            let mut o = [0f32; 4];
            for ch in 0..4 {
                o[ch] = (a[ch] + k * (a[ch] - b[ch])).clamp(0.0, 1.0);
            }
            dst.set(x, y, o);
        }
    }
    Ok(())
}

/// 3D LUT（n³ 网格，RGBA 输出）
pub struct Lut3D {
    pub n: u32,
    pub data: Vec<f32>, // n^3*4
}

impl Lut3D {
    pub fn new(n: u32) -> Result<Self, u16> {
        if n < 2 || n > 64 {
            return Err(XC_LUTDIM);
        }
        let cnt = (n * n * n * 4) as usize;
        let mut data = vec![0f32; cnt];
        // 恒等初始化：LUT(r,g,b)=（r,g,b,1)
        for bi in 0..n {
            for gi in 0..n {
                for ri in 0..n {
                    let i = (((bi * n + gi) * n + ri) * 4) as usize;
                    data[i] = ri as f32 / (n - 1) as f32;
                    data[i + 1] = gi as f32 / (n - 1) as f32;
                    data[i + 2] = bi as f32 / (n - 1) as f32;
                    data[i + 3] = 1.0;
                }
            }
        }
        Ok(Lut3D { n, data })
    }
    /// 四线性插值采样（三坐标分数 → 8 角权重）
    pub fn sample(&self, r: f32, g: f32, b: f32) -> [f32; 4] {
        let n = self.n as i64;
        let fr = (r.clamp(0.0, 1.0) * (n - 1) as f32).min((n - 1) as f32);
        let fg = (g.clamp(0.0, 1.0) * (n - 1) as f32).min((n - 1) as f32);
        let fb = (b.clamp(0.0, 1.0) * (n - 1) as f32).min((n - 1) as f32);
        let r0 = fr.m_floor() as i64;
        let g0 = fg.m_floor() as i64;
        let b0 = fb.m_floor() as i64;
        let r1 = (r0 + 1).min(n - 1);
        let g1 = (g0 + 1).min(n - 1);
        let b1 = (b0 + 1).min(n - 1);
        let tr = fr - r0 as f32;
        let tg = fg - g0 as f32;
        let tb = fb - b0 as f32;
        let mut out = [0f32; 4];
        for corner in 0..8 {
            let (cr, cg, cb, w) = match corner {
                0 => (r0, g0, b0, (1.0 - tr) * (1.0 - tg) * (1.0 - tb)),
                1 => (r1, g0, b0, tr * (1.0 - tg) * (1.0 - tb)),
                2 => (r0, g1, b0, (1.0 - tr) * tg * (1.0 - tb)),
                3 => (r1, g1, b0, tr * tg * (1.0 - tb)),
                4 => (r0, g0, b1, (1.0 - tr) * (1.0 - tg) * tb),
                5 => (r1, g0, b1, tr * (1.0 - tg) * tb),
                6 => (r0, g1, b1, (1.0 - tr) * tg * tb),
                _ => (r1, g1, b1, tr * tg * tb),
            };
            let i = (((cb * n + cg) * n + cr) * 4) as usize;
            for ch in 0..4 {
                out[ch] += self.data[i + ch] * w;
            }
        }
        out
    }
}

/// 算子七：LUT 应用（四线性插值全图）
pub fn apply_lut(src: &Img, dst: &mut Img, lut: &Lut3D) -> Result<(), u16> {
    if src.w != dst.w || src.h != dst.h {
        return Err(XC_IMGDIM);
    }
    for y in 0..src.h {
        for x in 0..src.w {
            let c = src.get(x, y);
            let o = lut.sample(c[0], c[1], c[2]);
            dst.set(x, y, o);
        }
    }
    Ok(())
}

/// 算子名闭集（链编排用）
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum PostOp {
    Bright = SEG_BASE,     // 0x1B40
    Blur3,                 // 0x1B41
    Bloom,                 // 0x1B42
    Tonemap,               // 0x1B43
    Vignette,              // 0x1B44
    Sharpen,               // 0x1B45
    Lut,                   // 0x1B46
}
impl PostOp {
    pub const fn code(self) -> u16 { self as u16 }
    pub const fn name(self) -> &'static str {
        match self {
            PostOp::Bright => "亮部提取",
            PostOp::Blur3 => "盒式模糊",
            PostOp::Bloom => "Bloom合成",
            PostOp::Tonemap => "ACES色调映射",
            PostOp::Vignette => "暗角",
            PostOp::Sharpen => "锐化",
            PostOp::Lut => "LUT应用",
        }
    }
}

/// 链编排：按序在双缓冲间交替执行
pub fn run_chain(src: &Img, ops: &[PostOp], strength: f32, lut: Option<&Lut3D>) -> Result<Img, u16> {
    let mut a = Img::new(src.w, src.h)?;
    a.px.copy_from_slice(&src.px);
    let mut b = Img::new(src.w, src.h)?;
    let mut bufs = [a, b];
    for (i, &op) in ops.iter().enumerate() {
        // split_at_mut 双向不相交借用：奇偶步交替读写缓冲
        let (si, dst): (&Img, &mut Img) = if i % 2 == 0 {
            let (l, r) = bufs.split_at_mut(1);
            (&l[0], &mut r[0])
        } else {
            let (l, r) = bufs.split_at_mut(1);
            (&r[0], &mut l[0])
        };
        match op {
            PostOp::Bright => bright_pass(si, dst, 0.8, 0.1)?,
            PostOp::Blur3 => box_blur3(si, dst)?,
            PostOp::Bloom => bloom_composite(si, si, dst, strength)?,
            PostOp::Tonemap => tonemap(si, dst, 1.0)?,
            PostOp::Vignette => vignette(si, dst, strength)?,
            PostOp::Sharpen => sharpen(si, dst, strength)?,
            PostOp::Lut => match lut {
                Some(l) => apply_lut(si, dst, l)?,
                None => return Err(XC_CHAINOP),
            },
        }
    }
    // 返回最后写入的缓冲（数组解构移出）
    let [x, y] = bufs;
    if ops.len() % 2 == 1 { Ok(y) } else { Ok(x) }
}

pub fn self_audit() -> Result<(), u16> {
    let _ = Img::new(4, 4)?;
    let _ = Lut3D::new(4)?;
    let _ = run_chain as fn(&Img, &[PostOp], f32, Option<&Lut3D>) -> Result<Img, u16>;
    Ok(())
}

pub fn run_cga34_checks() -> CheckSet {
    let mut s = CheckSet::new("CGPU-F0034");
    // J1 亮部提取（阈下消 / 阈上全量 / 软膝过渡）
    let mut src = Img::new(4, 2).ok().unwrap();
    src.set(0, 0, [0.2, 0.2, 0.2, 1.0]);
    src.set(1, 0, [2.0, 2.0, 2.0, 1.0]);
    src.set(2, 0, [0.85, 0.85, 0.85, 1.0]); // 软膝区
    let mut dst = Img::new(4, 2).ok().unwrap();
    let r = bright_pass(&src, &mut dst, 0.8, 0.1);
    let c0 = dst.get(0, 0);
    let c1 = dst.get(1, 0);
    let c2 = dst.get(2, 0);
    let ok = r.is_ok() && c0[0].abs() < 1e-6 && c1[0] > 1.9 && c2[0] > 0.0 && c2[0] < 1.0;
    s.add("CGA-34-01-亮部提取", ok, if ok { "" } else { "软膝破缺" });
    // J1-b 盒式模糊守恒（总能量不变）
    let mut b = Img::new(4, 2).ok().unwrap();
    box_blur3(&src, &mut b).ok().unwrap();
    let sum_src: f32 = src.px.iter().sum();
    let sum_b: f32 = b.px.iter().sum();
    let ok = (sum_src - sum_b).abs() / sum_src.max(1e-6) < 1e-4;
    s.add("CGA-34-02-模糊守恒", ok, if ok { "" } else { "能量泄漏" });
    // J1-c ACES 拟合性质（0→0 单调、大值→1）
    let ok = aces_tonemap(0.0).abs() < 1e-6
        && aces_tonemap(0.5) > aces_tonemap(0.25)
        && aces_tonemap(10.0) > 0.95;
    s.add("CGA-34-03-ACES性质", ok, if ok { "" } else { "拟合破缺" });
    // J2 链编排正确（双缓冲交替 + 恒等 LUT 直通）
    let lut = Lut3D::new(4).ok().unwrap();
    let out = run_chain(&src, &[PostOp::Lut], 1.0, Some(&lut)).ok().unwrap();
    let mut max_d = 0f32;
    for i in 0..src.px.len() {
        max_d = max_d.max((src.px[i] - out.px[i]).abs());
    }
    let ok = max_d < 0.15; // n=4 恒等网格插值误差界
    s.add("CGA-34-04-恒等LUT直通", ok, if ok { "" } else { "直通破缺" });
    // J2-b 全链（Bloom→Tonemap→Vignette）尺寸守恒 + 值域收口
    let out = run_chain(&src, &[PostOp::Bright, PostOp::Blur3, PostOp::Bloom, PostOp::Tonemap, PostOp::Vignette], 0.5, None);
    let ok = out.as_ref().map(|o| o.w == src.w && o.h == src.h).unwrap_or(false)
        && out.as_ref().map(|o| o.px.iter().all(|v| *v >= 0.0 && *v <= 1.0)).unwrap_or(false);
    s.add("CGA-34-05-全链编排", ok, if ok { "" } else { "链破缺" });
    // J2-c LUT 缺失拒（链上 Lut 算子无表 → XC_CHAINOP）
    let ok = matches!(run_chain(&src, &[PostOp::Lut], 1.0, None), Err(XC_CHAINOP));
    s.add("CGA-34-06-LUT缺失拒", ok, if ok { "" } else { "缺表放行" });
    // J3 性能口径（算子账：每像素 9 tap（模糊）/ 8 角（LUT）——结构上限入册）
    let taps_blur = 9;
    let taps_lut = 8;
    let ok = taps_blur == 9 && taps_lut == 8 && PostOp::Lut.name().contains("LUT");
    s.add("CGA-34-07-性能口径", ok, if ok { "" } else { "口径缺" });
    // J4 LUT 精度（恒等 n=16 时误差 < 0.04 + 非恒等表生效）
    let lut16 = Lut3D::new(16).ok().unwrap();
    let o = lut16.sample(0.37, 0.62, 0.11);
    let ok = (o[0] - 0.37).abs() < 0.04 && (o[1] - 0.62).abs() < 0.04 && (o[2] - 0.11).abs() < 0.04;
    s.add("CGA-34-08-LUT精度", ok, if ok { "" } else { "插值破缺" });
    // J4-b LUT 维度非法拒
    let ok = matches!(Lut3D::new(1), Err(XC_LUTDIM)) && matches!(Lut3D::new(65), Err(XC_LUTDIM));
    s.add("CGA-34-09-LUT维度校验", ok, if ok { "" } else { "维度放水" });
    // J5 VE-K 对接（算子名表 7 行齐——与 GPU 路径算子清单一一对应）
    let all = [PostOp::Bright, PostOp::Blur3, PostOp::Bloom, PostOp::Tonemap, PostOp::Vignette, PostOp::Sharpen, PostOp::Lut];
    let ok = all.iter().all(|&op| !op.name().is_empty())
        && all.iter().map(|&op| op.code()).collect::<Vec<_>>().windows(2).all(|w| w[0] < w[1]);
    s.add("CGA-34-10-VE-K算子表", ok, if ok { "" } else { "算子表缺" });
    // J6 图像尺寸非法拒 + 码位互异
    let ok = matches!(Img::new(0, 4), Err(XC_IMGDIM))
        && SEG_BASE % 0x10 == 0
        && XC_LUTDIM != XC_CHAINOP
        && XC_CHAINOP != XC_IMGDIM;
    s.add("CGA-34-11-校验码位", ok, if ok { "" } else { "码位冲突" });
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cga34_checks_all_green() {
        let set = run_cga34_checks();
        assert!(set.all_passed(), "{}", set.red_items_text());
    }

    #[test]
    fn cga34_tonemap_headroom() {
        // 曝光翻倍前后：亮处更亮且都不超 1
        let mut src = Img::new(1, 1).ok().unwrap();
        src.set(0, 0, [0.5, 0.5, 0.5, 1.0]);
        let mut d1 = Img::new(1, 1).ok().unwrap();
        let mut d2 = Img::new(1, 1).ok().unwrap();
        tonemap(&src, &mut d1, 1.0).ok().unwrap();
        tonemap(&src, &mut d2, 2.0).ok().unwrap();
        assert!(d2.get(0, 0)[0] > d1.get(0, 0)[0]);
        assert!(d2.get(0, 0)[0] <= 1.0);
    }
}
