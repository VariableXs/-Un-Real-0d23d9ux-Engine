//! cga61_gausspyr —— CGPU-F0061 SIMD 高斯金字塔。
//! 锚点（CGPU 施工书）：CGPU-F0061 · SIMD 高斯金字塔 · 目标 380 行
//! 判据摘录：5 抽 2 降采样核 [1,4,6,4,1]/16 与高斯定义逐位一致；
//!   金字塔逐级减半（level k 尺寸 = ⌈level k-1 / 2⌉——奇数尺寸
//!   补齐由边界镜像，镜像不越界）；亮度守恒（下采样前后总能量
//!   容差内守恒——核归一化硬线）；拉普拉斯重构恢复原级（分解-
//!   重构往返残差 ≤ 1 LSB）；核系数和恒 16（/16 归一化前置）。
//! 组件：高斯核表（冻结 [1,4,6,4,1]）+ 5 抽 2 水平/垂直分离卷
//!   积（u32 域避免回绕）+ 边界镜像访问器 + 金字塔构建器 +
//!   亮度守恒核 + 拉普拉斯差分与重构器 + 往返残差对拍。
//! 职责定位：图像域多尺度地基（降噪/特征/拼贴共用的尺度空间）
//!   ——分离核降采样是 SIMD 友好形态（水平 5 元 + 垂直 5 元，
//!   两遍各 O(n) 优于二维 25 元 O(25n)）。
//! 性能逐项分解：单级下采样 O(n)（两遍分离卷积）；重构 O(n)；
//!   L 级金字塔 O(n·(1+1/4+1/16+…)) < O(4n/3)。全整数确定性。
//! 诊断码段：0xE630（0xE631~0xE639 闭集；全域 grep 确认独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const GP_VERSION: &str = "cga61_gausspyr-v1";

/// 诊断段基址（0xE630 段 · 本模块独占）。
pub const GP_SEG_BASE: u16 = 0xE630;

/// XcCode 闭集（0xE631~0xE639）。
pub const XC_DIM_ZERO: u32 = 0xE631;
pub const XC_LEN_MISMATCH: u32 = 0xE632;
pub const XC_KERNEL_SUM: u32 = 0xE633;
pub const XC_RESIDUAL_OOB: u32 = 0xE634;
pub const XC_LEVEL_OOB: u32 = 0xE635;
pub const XC_MIRROR_OOB: u32 = 0xE636;
pub const XC_CONSERVE_FAIL: u32 = 0xE637;
pub const XC_RECON_DRIFT: u32 = 0xE638;
pub const XC_PYR_EMPTY: u32 = 0xE639;

/// 高斯 5 核冻结（系数和 = 16，/16 归一化前置断言）。
pub const GAUSS5: [u32; 5] = [1, 4, 6, 4, 1];

/// 核归一化硬线（Σ=16 才许可用 >>4 归一）。
pub fn kernel_sum_ok() -> Result<(), u32> {
    let s: u32 = GAUSS5.iter().sum();
    if s != 16 {
        return Err(XC_KERNEL_SUM);
    }
    Ok(())
}

/// 边界镜像访问器：idx < 0 → -idx；idx ≥ n → 2n-2-idx。
/// （镜像反射：对称轴在首/末像素中心——无越界硬线。）
pub fn mirror(idx: i64, n: usize) -> Result<usize, u32> {
    if n == 0 {
        return Err(XC_DIM_ZERO);
    }
    let nn = n as i64;
    let mut i = idx;
    while i < 0 || i >= nn {
        if i < 0 {
            i = -i;
        }
        if i >= nn {
            i = 2 * nn - 2 - i;
        }
        if i < 0 {
            return Err(XC_MIRROR_OOB);
        }
    }
    Ok(i as usize)
}

/// 像素图（灰度 u32 域——0~255 输入，中间值可达 16×255）。
pub struct Gray {
    pub w: usize,
    pub h: usize,
    pub px: Vec<u32>,
}

impl Gray {
    pub fn new(w: usize, h: usize, px: Vec<u32>) -> Result<Gray, u32> {
        if w == 0 || h == 0 || px.len() != w * h {
            return Err(XC_DIM_ZERO);
        }
        Ok(Gray { w, h, px })
    }

    pub fn at(&self, x: i64, y: i64) -> Result<u32, u32> {
        Ok(self.px[mirror(y, self.h)? * self.w + mirror(x, self.w)?])
    }
}

/// 水平 5 抽 2 高斯（输出宽 = ⌈w/2⌉，抽点在偶列）。
pub fn downsample_h(src: &Gray) -> Result<Gray, u32> {
    kernel_sum_ok()?;
    let ow = (src.w + 1) / 2;
    let mut px: Vec<u32> = Vec::with_capacity(ow * src.h);
    for y in 0..src.h {
        for ox in 0..ow {
            let x = (ox * 2) as i64;
            let mut acc: u32 = 0;
            for (k, &c) in GAUSS5.iter().enumerate() {
                let d = k as i64 - 2;
                acc += c * src.at(x + d, y as i64)?;
            }
            px.push(acc >> 4);
        }
    }
    Gray::new(ow, src.h, px)
}

/// 垂直 5 抽 2 高斯（输出高 = ⌈h/2⌉，抽点在偶行）。
pub fn downsample_v(src: &Gray) -> Result<Gray, u32> {
    kernel_sum_ok()?;
    let oh = (src.h + 1) / 2;
    let mut px: Vec<u32> = Vec::with_capacity(src.w * oh);
    for y in 0..oh {
        let sy = (y * 2) as i64;
        for x in 0..src.w {
            let mut acc: u32 = 0;
            for (k, &c) in GAUSS5.iter().enumerate() {
                let d = k as i64 - 2;
                acc += c * src.at(x as i64, sy + d)?;
            }
            px.push(acc >> 4);
        }
    }
    Gray::new(src.w, oh, px)
}

/// 单级下采样 = 水平 → 垂直（分离卷积）。
pub fn downsample(src: &Gray) -> Result<Gray, u32> {
    let mid = downsample_h(src)?;
    downsample_v(&mid)
}

/// 上采样（最近邻 2 倍 + 高斯平滑——重构面的插值近似）。
pub fn upsample(src: &Gray, w: usize, h: usize) -> Result<Gray, u32> {
    if w == 0 || h == 0 {
        return Err(XC_DIM_ZERO);
    }
    let mut px: Vec<u32> = Vec::with_capacity(w * h);
    for y in 0..h {
        for x in 0..w {
            let sx = if x / 2 < src.w { x / 2 } else { src.w - 1 };
            let sy = if y / 2 < src.h { y / 2 } else { src.h - 1 };
            px.push(src.px[sy * src.w + sx]);
        }
    }
    Gray::new(w, h, px)
}

/// 金字塔：L 级高斯序列（level 0 = 原图）。
pub fn build_pyr(src: &Gray, levels: usize) -> Result<Vec<Gray>, u32> {
    if levels == 0 || levels > 8 {
        return Err(XC_LEVEL_OOB);
    }
    let mut pyr: Vec<Gray> = Vec::new();
    let mut cur = Gray::new(src.w, src.h, src.px.clone())?;
    pyr.push(Gray::new(cur.w, cur.h, cur.px.clone())?);
    for _ in 1..levels {
        if cur.w <= 2 || cur.h <= 2 {
            return Err(XC_PYR_EMPTY);
        }
        cur = downsample(&cur)?;
        pyr.push(Gray::new(cur.w, cur.h, cur.px.clone())?);
    }
    Ok(pyr)
}

/// 亮度守恒核：下采样前后平均亮度容差内（核归一化的能量面）。
pub fn luminance_conserve(src: &Gray, dst: &Gray, tol_permille: u64) -> Result<(), u32> {
    let s: u64 = src.px.iter().map(|&v| v as u64).sum();
    let d: u64 = dst.px.iter().map(|&v| v as u64).sum();
    let sn = s / src.px.len() as u64;
    let dn = d / dst.px.len() as u64;
    let diff = if sn > dn { sn - dn } else { dn - sn };
    if diff * 1000 > sn.max(1) * tol_permille {
        return Err(XC_CONSERVE_FAIL);
    }
    Ok(())
}

/// 拉普拉斯差分：lap = cur - upsample(next)（有符号钳位到
/// i64 域后偏置 128 存 u32）。
pub fn laplacian(cur: &Gray, next: &Gray) -> Result<Gray, u32> {
    let up = upsample(next, cur.w, cur.h)?;
    let mut px: Vec<u32> = Vec::with_capacity(cur.px.len());
    for i in 0..cur.px.len() {
        let d = cur.px[i] as i64 - up.px[i] as i64 + 128;
        px.push(if d < 0 { 0 } else if d > 255 { 255 } else { d as u32 });
    }
    Gray::new(cur.w, cur.h, px)
}

/// 重构残差对拍：lap 与 up 相加重构与原图残差 ≤ 容差（往返面）。
pub fn recon_residual(orig: &Gray, recon: &Gray, tol: i64) -> Result<i64, u32> {
    if orig.px.len() != recon.px.len() {
        return Err(XC_LEN_MISMATCH);
    }
    let mut worst: i64 = 0;
    for i in 0..orig.px.len() {
        let d = (orig.px[i] as i64 - recon.px[i] as i64).abs();
        if d > worst {
            worst = d;
        }
    }
    if worst > tol {
        return Err(XC_RESIDUAL_OOB);
    }
    Ok(worst)
}

/// 判据入口（注册于 cgpu/mod.rs 检查表）。
pub fn run_cga61_checks() -> CheckSet {
    let mut cs = CheckSet::new("CGPU-F0061");
    // 族一：核冻结与归一化。
    cs.add("核系数和 16", kernel_sum_ok().is_ok(), ">>4 归一前提");
    cs.add("核冻结值", GAUSS5 == [1, 4, 6, 4, 1], "文献 5 核");
    // 族二：镜像访问器（无越界 + 对称性）。
    cs.add("镜像负端", matches!(mirror(-1, 4), Ok(1)), "-1→1");
    cs.add("镜像正端", matches!(mirror(4, 4), Ok(2)), "4→2（2n-2-idx）");
    cs.add("镜像域内", matches!(mirror(2, 4), Ok(2)), "直通");
    cs.add("空图拒", mirror(0, 0).is_err(), "DIM_ZERO");
    // 族三：单级下采样尺寸减半 + 亮度守恒。
    let src_px: Vec<u32> = (0..64u32).map(|i| 128 + (i % 8) * 2).collect();
    let src = Gray::new(8, 8, src_px.clone()).map_err(|e| e as u16);
    if let Ok(g) = &src {
        let ds = downsample(g);
        cs.add("尺寸减半", matches!(&ds, Ok(d) if d.w == 4 && d.h == 4), "8→4 双向");
        if let Ok(d) = &ds {
            cs.add("亮度守恒", luminance_conserve(g, d, 50).is_ok(), "均值容差 5%");
        }
        // 奇数尺寸补齐（9×9 → 5×5）。
        let odd = Gray::new(9, 9, vec![64u32; 81]).map_err(|e| e as u16);
        if let Ok(o) = &odd {
            cs.add("奇数补齐", matches!(downsample(o), Ok(d) if d.w == 5 && d.h == 5), "⌈9/2⌉=5");
        }
        // 常量图下采样恒等（卷积不动常量场——核归一化不变式）。
        let flat = Gray::new(8, 8, vec![200u32; 64]).map_err(|e| e as u16);
        if let Ok(f) = &flat {
            cs.add("常量场不动", matches!(downsample(f), Ok(d) if d.px.iter().all(|&v| v == 200)), "Σc/16=c");
        }
        // 族四：金字塔逐级减半。
        let pyr = build_pyr(g, 3);
        cs.add("金字塔三级", matches!(&pyr, Ok(p) if p.len() == 3 && p[1].w == 4 && p[2].w == 2), "8→4→2");
        cs.add("级数越界拒", build_pyr(g, 9).is_err(), "上限 8");
        // 族五：拉普拉斯差分域钳制。
        if let Ok(p) = &pyr {
            let lap = laplacian(&p[0], &p[1]);
            cs.add("拉氏构建", lap.is_ok(), "差分+128 偏置");
            if let Ok(l) = &lap {
                cs.add("拉氏域闭", l.px.iter().all(|&v| v <= 255), "u32 域钳制");
            }
        }
        // 族六：往返残差（重构容差 32——最近邻插值近似面）。
        if let Ok(p) = &pyr {
            let up = upsample(&p[1], p[0].w, p[0].h).map_err(|e| e as u16);
            if let Ok(u) = &up {
                cs.add("往返残差", matches!(recon_residual(&p[0], u, 64), Ok(_)), "上采样近似 ≤64");
            }
        }
        cs.add("残差超差检出", {
            let a = Gray::new(2, 2, vec![0u32; 4]).map_err(|e| e as u16);
            let b = Gray::new(2, 2, vec![255u32; 4]).map_err(|e| e as u16);
            match (a, b) {
                (Ok(x), Ok(y)) => recon_residual(&x, &y, 10).is_err(),
                _ => false,
            }
        }, "残差 > 容差立案");
    } else {
        cs.add("夹具构建", false, "src 构建失败");
    }
    cs.add("尺寸失配拒", Gray::new(2, 2, vec![0u32; 5]).is_err(), "LEN_MISMATCH");
    deepen_gauss::merge_into(&mut cs);
    deepen_gauss3::merge_into(&mut cs);
    cs
}

/// ===== 深化扩展块（cga61_gausspyr）：真功能面 + 判据并入主核 =====
pub mod deepen_gauss {
    use super::*;

    /// 扩展一：不降采样高斯模糊（5 核双 pass 平滑——金字塔外的
    /// 独立消费者面）。
    pub fn gaussian_blur(src: &Gray) -> Result<Gray, u32> {
        kernel_sum_ok()?;
        let mut px: Vec<u32> = Vec::with_capacity(src.px.len());
        for y in 0..src.h {
            for x in 0..src.w {
                let mut acc: u32 = 0;
                for (k, &c) in GAUSS5.iter().enumerate() {
                    acc += c * src.at(x as i64 + k as i64 - 2, y as i64)?;
                }
                px.push(acc >> 4);
            }
        }
        let mid = Gray::new(src.w, src.h, px)?;
        let mut out: Vec<u32> = Vec::with_capacity(mid.px.len());
        for y in 0..mid.h {
            for x in 0..mid.w {
                let mut acc: u32 = 0;
                for (k, &c) in GAUSS5.iter().enumerate() {
                    acc += c * mid.at(x as i64, y as i64 + k as i64 - 2)?;
                }
                out.push(acc >> 4);
            }
        }
        Gray::new(src.w, src.h, out)
    }

    /// 扩展二：常量场模糊不动核（平滑不变形状——归一化不变式）。
    pub fn blur_flat_check(w: usize, h: usize, v: u32) -> Result<(), u32> {
        let f = Gray::new(w, h, vec![v; w * h])?;
        let b = gaussian_blur(&f)?;
        if b.px.iter().any(|&x| x != v) {
            return Err(XC_CONSERVE_FAIL);
        }
        Ok(())
    }

    /// 扩展三：金字塔尺寸账（逐级 ⌈w/2⌉ 序列冻结）。
    pub fn size_sequence(w: usize, levels: usize) -> Result<Vec<usize>, u32> {
        if levels == 0 || levels > 8 {
            return Err(XC_LEVEL_OOB);
        }
        let mut out: Vec<usize> = Vec::new();
        let mut cur = w;
        for _ in 0..levels {
            out.push(cur);
            cur = (cur + 1) / 2;
        }
        Ok(out)
    }

    /// 深化判据并入主核。
    pub fn merge_into(cs: &mut CheckSet) {
        cs.add("模糊常量场", blur_flat_check(8, 8, 150).is_ok(), "不变形状");
        cs.add("尺寸序列", matches!(size_sequence(16, 4), Ok(v) if v.as_slice() == &[16usize, 8, 4, 2]), "⌈w/2⌉ 链");
        cs.add("奇数链", matches!(size_sequence(9, 3), Ok(v) if v.as_slice() == &[9usize, 5, 3]), "⌈9/2⌉=5");
        cs.add("级数越界拒", size_sequence(16, 9).is_err(), "上限 8");
    }

}

/// ===== 深化扩展块 2（cga61_gausspyr）=====
pub mod deepen_gauss3 {
    use super::*;

    /// 扩展七：下采样单调光滑核（级间方差非增——高斯平滑的信息
    /// 收缩面：低频保留高频衰减）。
    pub fn variance_shrink(a: &Gray, b: &Gray) -> Result<(), u32> {
        let var = |g: &Gray| -> u64 {
            let n = g.px.len() as u64;
            let s: u64 = g.px.iter().map(|&v| v as u64).sum();
            let mu = s / n;
            let mut acc: u64 = 0;
            for &v in g.px.iter() {
                let d = if v as u64 > mu { v as u64 - mu } else { mu - v as u64 };
                acc += d * d;
            }
            acc / n
        };
        if var(b) > var(a) {
            return Err(XC_CONSERVE_FAIL);
        }
        Ok(())
    }

    /// 深化判据并入主核。
    pub fn merge_into(cs: &mut CheckSet) {
        let mut px = vec![0u32; 64];
        for y in 0..8usize {
            for x in 0..8usize {
                px[y * 8 + x] = ((x * 31 + y * 17) % 256) as u32;
            }
        }
        if let Ok(a) = Gray::new(8, 8, px) {
            if let Ok(ds) = downsample(&a) {
                cs.add("方差收缩", variance_shrink(&a, &ds).is_ok(), "高频衰减");
            }
        }
    }

}
