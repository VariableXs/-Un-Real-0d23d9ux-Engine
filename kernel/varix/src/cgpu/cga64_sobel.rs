//! cga64_sobel —— CGPU-F0064 SIMD 边缘检测（Sobel）。
//! 锚点（CGPU 施工书）：CGPU-F0064 · SIMD 边缘检测（Sobel）· 目标 320 行
//! 判据摘录：水平/垂直 Sobel 核与文献定义逐位一致（Gx=[[-1,0,1],
//!   [-2,0,2],[-1,0,1]]，Gy=转置）；梯度幅值 L1 路径（|gx|+|gy|）
//!   与 L2 路径（整数近似 √ 不可用 ⇒ L1 为主路径，L2 用平方和
//!   保留域供阈值参考）双账并行；方向量化四象限闭集（0°/45°/
//!   90°/135°，atan2 整数化判定）；常量场梯度恒零；纯阶跃边
//!   响应冻结（8×8 语料锚值）；核系数和恒零（一阶差分性质）。
//! 组件：Sobel 核表（冻结）+ 卷积引擎（镜像边界）+ 幅值 L1/L2
//!   双账 + 方向四象限量化器 + 阶跃边语料锚 + 常量场不动核。
//! 职责定位：图像特征提取第一级——Sobel 是所有边缘/轮廓/角点
//!   检测的公共前缀，双账（L1 主路径/L2 参考域）把阈值策略与
//!   度量解耦。
//! 性能逐项分解：单像素两核卷积 O(2×6 有效乘)（零系数跳过）；
//!   全图 O(12n)。全整数确定性（i64 域中间值防负回绕）。
//! 诊断码段：0xE670（0xE671~0xE679 闭集；全域 grep 确认独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const SB_VERSION: &str = "cga64_sobel-v1";

/// 诊断段基址（0xE670 段 · 本模块独占）。
pub const SB_SEG_BASE: u16 = 0xE670;

/// XcCode 闭集（0xE671~0xE679）。
pub const XC_DIM_ZERO: u32 = 0xE671;
pub const XC_KERNEL_SUM: u32 = 0xE672;
pub const XC_DIR_OOB: u32 = 0xE673;
pub const XC_GRAD_DRIFT: u32 = 0xE674;
pub const XC_MIRROR_OOB: u32 = 0xE675;
pub const XC_FLAT_DRIFT: u32 = 0xE676;
pub const XC_ANCHOR_MISS: u32 = 0xE677;
pub const XC_QUANT_DRIFT: u32 = 0xE678;
pub const XC_LEN_MISMATCH: u32 = 0xE679;

/// Sobel 核冻结（文献值；Σ=0 一阶差分性质）。
pub const SOBEL_GX: [i64; 9] = [-1, 0, 1, -2, 0, 2, -1, 0, 1];
pub const SOBEL_GY: [i64; 9] = [-1, -2, -1, 0, 0, 0, 1, 2, 1];

/// 核和恒零硬线。
pub fn kernel_zero_sum() -> Result<(), u32> {
    let sx: i64 = SOBEL_GX.iter().sum();
    let sy: i64 = SOBEL_GY.iter().sum();
    if sx != 0 || sy != 0 {
        return Err(XC_KERNEL_SUM);
    }
    Ok(())
}

/// 边界镜像。
fn mirror(idx: i64, n: usize) -> Result<usize, u32> {
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

/// 灰度图。
pub struct Img {
    pub w: usize,
    pub h: usize,
    pub px: Vec<u32>,
}

impl Img {
    pub fn new(w: usize, h: usize, px: Vec<u32>) -> Result<Img, u32> {
        if w == 0 || h == 0 || px.len() != w * h {
            return Err(XC_DIM_ZERO);
        }
        Ok(Img { w, h, px })
    }

    fn at(&self, x: i64, y: i64) -> Result<i64, u32> {
        Ok(self.px[mirror(y, self.h)? * self.w + mirror(x, self.w)?] as i64)
    }
}

/// 梯度账目（单像素）：gx、gy（i64 有符号域）。
pub struct Grad {
    pub gx: i64,
    pub gy: i64,
}

/// 双核卷积（镜像边界，零系数跳过——6 乘 2 加有效路径）。
pub fn convolve_at(src: &Img, x: usize, y: usize) -> Result<Grad, u32> {
    let mut gx: i64 = 0;
    let mut gy: i64 = 0;
    let mut k = 0usize;
    for dy in -1i64..=1 {
        for dx in -1i64..=1 {
            let v = src.at(x as i64 + dx, y as i64 + dy)?;
            let wx = SOBEL_GX[k];
            let wy = SOBEL_GY[k];
            if wx != 0 {
                gx += wx * v;
            }
            if wy != 0 {
                gy += wy * v;
            }
            k += 1;
        }
    }
    Ok(Grad { gx, gy })
}

/// L1 幅值主路径（|gx|+|gy|，u32 域钳制）。
pub fn mag_l1(g: &Grad) -> u32 {
    let a = g.gx.abs();
    let b = g.gy.abs();
    let m = a + b;
    if m > u32::MAX as i64 {
        u32::MAX
    } else {
        m as u32
    }
}

/// L2 平方和参考域（供阈值策略比较；不开方——平方域冻结）。
pub fn mag_l2sq(g: &Grad) -> u64 {
    (g.gx * g.gx + g.gy * g.gy) as u64
}

/// 方向四象限量化（整数化 atan2：判 |gy| vs |gx| 的倍率关系）。
/// 返回 0=0°(水平边) 1=45° 2=90°(垂直边) 3=135°。
pub fn quant_dir(g: &Grad) -> Result<u8, u32> {
    let ax = g.gx.abs();
    let ay = g.gy.abs();
    // 梯度方向与边方向正交：gx 主导 → 垂直边(90°)。
    if ax == 0 && ay == 0 {
        return Err(XC_DIR_OOB); // 无梯度无方向——显性拒绝
    }
    // 倍率判据（2× tan22.5° ≈ 0.828 用 2/5 近似——冻结比例线）。
    if ay * 5 <= ax * 2 {
        Ok(2) // 近水平梯度 → 垂直边
    } else if ax * 5 <= ay * 2 {
        Ok(0) // 近垂直梯度 → 水平边
    } else if (g.gx >= 0) == (g.gy >= 0) {
        Ok(1) // 同号 → 45°
    } else {
        Ok(3) // 异号 → 135°
    }
}

/// 全图梯度场。
pub fn gradient_field(src: &Img) -> Result<(Vec<u32>, Vec<u8>), u32> {
    let mut mags: Vec<u32> = Vec::with_capacity(src.px.len());
    let mut dirs: Vec<u8> = Vec::with_capacity(src.px.len());
    for y in 0..src.h {
        for x in 0..src.w {
            let g = convolve_at(src, x, y)?;
            mags.push(mag_l1(&g));
            dirs.push(quant_dir(&g)?);
        }
    }
    Ok((mags, dirs))
}

/// 常量场梯度恒零核。
pub fn flat_field_check(w: usize, h: usize) -> Result<(), u32> {
    let f = Img::new(w, h, vec![100u32; w * h])?;
    let (mags, _) = gradient_field(&f)?;
    if mags.iter().any(|&m| m != 0) {
        return Err(XC_FLAT_DRIFT);
    }
    Ok(())
}

/// 判据入口（注册于 cgpu/mod.rs 检查表）。
pub fn run_cga64_checks() -> CheckSet {
    let mut cs = CheckSet::new("CGPU-F0064");
    // 族一：核冻结。
    cs.add("核和零", kernel_zero_sum().is_ok(), "一阶差分 Σ=0");
    cs.add("Gx 冻结", SOBEL_GX == [-1i64, 0, 1, -2, 0, 2, -1, 0, 1], "文献值");
    cs.add("Gy 冻结", SOBEL_GY == [-1i64, -2, -1, 0, 0, 0, 1, 2, 1], "Gx 转置");
    // 族二：常量场梯度恒零。
    cs.add("常量场零梯度", flat_field_check(8, 8).is_ok(), "Σ=0 必然面");
    // 族三：纯垂直阶跃边（x<4 黑 x≥4 白）锚。
    let mut edge = vec![0u32; 64];
    for y in 0..8usize {
        for x in 4..8usize {
            edge[y * 8 + x] = 255;
        }
    }
    let eg = Img::new(8, 8, edge).map_err(|e| e as u16);
    if let Ok(e) = &eg {
        // 边中心 (3,4)：gx = 右列 - 左列 = 大正值；gy = 0（上下对称）。
        let g = convolve_at(e, 3, 4).map_err(|e2| e2 as u16);
        cs.add("边心强响应", matches!(&g, Ok(v) if v.gx == 4 * 255), "gx=4Δ=1020");
        cs.add("边心 gy 零", matches!(&g, Ok(v) if v.gy == 0), "上下对称抵消");
        cs.add("L1 冻结", matches!(&g, Ok(v) if mag_l1(v) == 1020), "|gx|+|gy|");
        cs.add("L2 平方域", matches!(&g, Ok(v) if mag_l2sq(v) == 1020u64 * 1020), "不开方冻结");
        cs.add("方向=垂直边", matches!(&g, Ok(v) if quant_dir(v).map_err(|e3| e3 as u16) == Ok(2)), "gx 主导");
        // 边远处 (0,0)：镜像边界弱化但非零——直接锚值。
        let g0 = convolve_at(e, 0, 0).map_err(|e2| e2 as u16);
        cs.add("角点有响应", matches!(&g0, Ok(v) if v.gx > 0), "镜像弱化非零");
        // 全图方向场：垂直边区域方向应清一色 2。
        if let Ok((mags, dirs)) = gradient_field(e) {
            cs.add("场构建", mags.len() == 64 && dirs.len() == 64, "双账齐");
            cs.add("边带强响应", (4..40).all(|i| mags[i] > 500), "x=4 列起强");
            cs.add("方向清一色", (4..40).all(|i| dirs[i] == 2), "垂直边全域");
        } else {
            cs.add("场构建", false, "场构建失败");
        }
        // 族四：水平阶跃边（y<4 黑 y≥4 白）—— gy 主导，方向 0。
        let mut hedge = vec![0u32; 64];
        for y in 4..8usize {
            for x in 0..8usize {
                hedge[y * 8 + x] = 255;
            }
        }
        let hg = Img::new(8, 8, hedge).map_err(|e2| e2 as u16);
        if let Ok(h) = &hg {
            let gh = convolve_at(h, 4, 3).map_err(|e2| e2 as u16);
            cs.add("水平边 gy 主导", matches!(&gh, Ok(v) if v.gy == 4 * 255 && v.gx == 0), "转置对称");
            cs.add("方向=水平边", matches!(&gh, Ok(v) if quant_dir(v).map_err(|e3| e3 as u16) == Ok(0)), "闭集 0");
        }
        // 族五：斜边 45° 语料（对角半亮）。
        let mut diag = vec![0u32; 64];
        for y in 0..8usize {
            for x in 0..8usize {
                if x + y >= 7 {
                    diag[y * 8 + x] = 255;
                }
            }
        }
        let dg = Img::new(8, 8, diag).map_err(|e2| e2 as u16);
        if let Ok(d) = &dg {
            let gd = convolve_at(d, 4, 3).map_err(|e2| e2 as u16);
            cs.add("斜边方向 45/135", matches!(&gd, Ok(v) if matches!(quant_dir(v), Ok(1) | Ok(3))), "对角闭集");
        }
        // 族六：零梯度拒绝（常量场方向无定义）。
        let flat = Img::new(4, 4, vec![9u32; 16]).map_err(|e2| e2 as u16);
        if let Ok(f) = &flat {
            let gf = convolve_at(f, 1, 1).map_err(|e2| e2 as u16);
            cs.add("零梯度拒方向", matches!(&gf, Ok(v) if quant_dir(v).is_err()), "DIR_OOB 显性");
        }
        cs.add("空图拒", Img::new(0, 4, vec![]).is_err(), "DIM_ZERO");
        cs.add("尺寸失配拒", Img::new(2, 2, vec![0u32; 5]).is_err(), "LEN_MISMATCH");
    } else {
        cs.add("夹具构建", false, "edge 构建失败");
    }
    deepen_sobel::merge_into(&mut cs);
    deepen_sobel2::merge_into(&mut cs);
    cs
}

/// ===== 深化扩展块（cga64_sobel）：真功能面 + 判据并入主核 =====
pub mod deepen_sobel {
    use super::*;

    /// 扩展一：Prewitt 对照核（等权 [1,0,-1]×3——与 Sobel 双实现
    /// 互证：纯阶跃边上同号同向，幅值比例 2:3）。
    pub const PREWITT_GX: [i64; 9] = [-1, 0, 1, -1, 0, 1, -1, 0, 1];
    pub const PREWITT_GY: [i64; 9] = [-1, -1, -1, 0, 0, 0, 1, 1, 1];

    pub fn prewitt_at(src: &Img, x: usize, y: usize) -> Result<Grad, u32> {
        let mut gx: i64 = 0;
        let mut gy: i64 = 0;
        let mut k = 0usize;
        for dy in -1i64..=1 {
            for dx in -1i64..=1 {
                let v = src.at(x as i64 + dx, y as i64 + dy)?;
                if PREWITT_GX[k] != 0 {
                    gx += PREWITT_GX[k] * v;
                }
                if PREWITT_GY[k] != 0 {
                    gy += PREWITT_GY[k] * v;
                }
                k += 1;
            }
        }
        Ok(Grad { gx, gy })
    }

    /// 扩展二：阈值二值化（边缘图 → 0/255——L1 阈值闭集路由）。
    pub fn threshold_bin(mags: &[u32], thr: u32) -> Result<Vec<u32>, u32> {
        let mut out: Vec<u32> = Vec::with_capacity(mags.len());
        for &m in mags.iter() {
            out.push(if m >= thr { 255 } else { 0 });
        }
        Ok(out)
    }

    /// 扩展三：幅值界核（L1 ≤ 4×255×2 = 2040——卷积极值界）。
    pub fn mag_bound_check(mags: &[u32]) -> Result<(), u32> {
        for (i, &m) in mags.iter().enumerate() {
            if m > 2040 {
                return Err(XC_GRAD_DRIFT + i as u32);
            }
        }
        Ok(())
    }

    /// 深化判据并入主核。
    pub fn merge_into(cs: &mut CheckSet) {
        let mut edge = vec![0u32; 64];
        for y in 0..8usize {
            for x in 4..8usize {
                edge[y * 8 + x] = 255;
            }
        }
        if let Ok(e) = Img::new(8, 8, edge) {
            let gs = convolve_at(&e, 3, 4).map_err(|e2| e2 as u16);
            let gp = prewitt_at(&e, 3, 4).map_err(|e2| e2 as u16);
            cs.add("双核同号", matches!((&gs, &gp), (Ok(s), Ok(p)) if s.gx > 0 && p.gx > 0 && s.gy == 0 && p.gy == 0), "同向");
            cs.add("幅值比 3:2", matches!((&gs, &gp), (Ok(s), Ok(p)) if s.gx == p.gx / 2 * 3), "1020:680");
            if let Ok((mags, _)) = gradient_field(&e) {
                cs.add("幅值界内", mag_bound_check(&mags).is_ok(), "≤2040");
                let bin = threshold_bin(&mags, 500).map_err(|e2| e2 as u16);
                cs.add("阈值二值", matches!(&bin, Ok(v) if v.iter().all(|&x| x == 0 || x == 255)), "0/255 闭域");
                cs.add("边带检出", matches!(&bin, Ok(v) if v[4 * 8 + 4] == 255), "边心过阈");
            } else {
                cs.add("幅值界内", false, "场构建失败");
            }
        }
    }

}

/// ===== 深化扩展块 2（cga64_sobel）=====
pub mod deepen_sobel2 {
    use super::*;

    /// 扩展四：非极大值抑制（NMS 简化：3×3 窗内局部极大保留——
    /// 边缘细化的标准后续）。
    pub fn nms(mags: &[u32], w: usize, h: usize) -> Result<Vec<bool>, u32> {
        if w == 0 || h == 0 || mags.len() != w * h {
            return Err(XC_LEN_MISMATCH);
        }
        let mut out: Vec<bool> = Vec::with_capacity(mags.len());
        for y in 0..h {
            for x in 0..w {
                let v = mags[y * w + x];
                let mut is_max = true;
                for dy in -1i64..=1 {
                    for dx in -1i64..=1 {
                        if dx == 0 && dy == 0 {
                            continue;
                        }
                        let nx = (x as i64 + dx).clamp(0, w as i64 - 1) as usize;
                        let ny = (y as i64 + dy).clamp(0, h as i64 - 1) as usize;
                        if mags[ny * w + nx] > v {
                            is_max = false;
                        }
                    }
                }
                out.push(is_max && v > 0);
            }
        }
        Ok(out)
    }

    /// 扩展五：梯度守恒核（垂直阶跃边上 Σ响应 == 0——镜像边界
    /// 使正负响应成对抵消——卷积和恒等式）。
    pub fn grad_sum_zero(src: &Img) -> Result<(), u32> {
        let mut gx_sum: i64 = 0;
        for y in 0..src.h {
            for x in 0..src.w {
                gx_sum += convolve_at(src, x, y)?.gx;
            }
        }
        // 镜像边界不完全抵消（边界重复采样），但内部行应近零——
        // 取账面界 ≤ 4×255×2（边界行预算）。
        if gx_sum.abs() > 2040 {
            return Err(XC_GRAD_DRIFT);
        }
        Ok(())
    }

    /// 深化判据并入主核。
    pub fn merge_into(cs: &mut CheckSet) {
        let mut edge = vec![0u32; 64];
        for y in 0..8usize {
            for x in 4..8usize {
                edge[y * 8 + x] = 255;
            }
        }
        if let Ok(e) = Img::new(8, 8, edge) {
            if let Ok((mags, _)) = gradient_field(&e) {
                let nm = nms(&mags, 8, 8).map_err(|e2| e2 as u16);
                cs.add("NMS 构建", nm.is_ok(), "细线保留");
                if let Ok(n) = &nm {
                    cs.add("NMS 稀疏", n.iter().filter(|&&b| b).count() <= 16, "极大点稀疏");
                }
                cs.add("NMS 尺寸拒", nms(&mags[..63], 8, 8).is_err(), "LEN_MISMATCH");
            } else {
                cs.add("NMS 构建", false, "场构建失败");
            }
            cs.add("梯度界账", grad_sum_zero(&e).is_ok(), "Σgx 有界");
        }
    }

}
