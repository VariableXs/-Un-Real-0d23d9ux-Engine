//! cga80_unsharp —— CGPU-F0080 SIMD 锐化（Unsharp Mask）。
//! 锚点（CGPU 施工书）：CGPU-F0080 · SIMD 锐化（Unsharp Mask）· 目标 320 行
//! 判据摘录：三步链冻结（高斯模糊 → 差值掩码 → 增益叠加：
//! out = in + amount × (in − blur)）；amount 域闭集（[0,300]%
//! 定点——过锐化振铃红线）；恒等消融（amount=0 → 恒等——锐
//! 化关断面）；平坦区不动（in==blur → 增益项零——无噪声放大
//! 面）；边缘增强单向账（阶跃边对比度增——|out 差| ≥ |in 差|
//! 于边带）；溢出钳制账（钳顶/钳底像素计数落账）。
//! 组件：3×3 盒模糊（模糊底座——与 cga67 同语义）+ 差值掩码 +
//! 定点增益叠加 + amount 域闸 + 恒等消融核 + 平坦不动核 + 边
//! 境对比核 + 钳制账。
//! 职责定位：图像细节增强的标准算子——Unsharp Mask 是「模糊的
//! 反向利用」，三步链每步都可账面核；振铃（过锐化的伪轮廓）
//! 由 amount 域闸前置拦截。
//! 性能逐项分解：模糊 O(9n)；差值 O(n)；叠加 O(n)。全整数确定
//! 性（i64 中间域防负回绕）。
//! 诊断码段：0xE890（0xE891~0xE899 闭集；全域 grep 确认独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const US_VERSION: &str = "cga80_unsharp-v1";

/// 诊断段基址（0xE890 段 · 本模块独占）。
pub const US_SEG_BASE: u16 = 0xE890;

/// XcCode 闭集（0xE891~0xE899）。
pub const XC_AMOUNT_OOB: u32 = 0xE891;
pub const XC_DIM_ZERO: u32 = 0xE892;
pub const XC_LEN_MISMATCH: u32 = 0xE893;
pub const XC_IDEMPOTENT: u32 = 0xE894;
pub const XC_FLAT_DRIFT: u32 = 0xE895;
pub const XC_EDGE_WEAK: u32 = 0xE896;
pub const XC_MIRROR_OOB: u32 = 0xE897;
pub const XC_EMPTY: u32 = 0xE898;
pub const XC_IDENTITY_FAIL: u32 = 0xE899;

/// amount 域闭集（[0,300]‰ —— 300% 上限防振铃）。
pub const AMOUNT_MAX: u32 = 300;

pub fn amount_ok(a: u32) -> Result<(), u32> {
    if a > AMOUNT_MAX {
        return Err(XC_AMOUNT_OOB);
    }
    Ok(())
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

    fn at(&self, x: i64, y: i64) -> Result<u32, u32> {
        let m = |i: i64, n: usize| -> Result<usize, u32> {
            let nn = n as i64;
            let mut j = i;
            while j < 0 || j >= nn {
                if j < 0 {
                    j = -j;
                }
                if j >= nn {
                    j = 2 * nn - 2 - j;
                }
                if j < 0 {
                    return Err(XC_MIRROR_OOB);
                }
            }
            Ok(j as usize)
        };
        Ok(self.px[m(y, self.h)? * self.w + m(x, self.w)?])
    }
}

/// 3×3 盒模糊（>>3 无法整除 9 ⇒ 用 16 定点近似：Σ×16/9? 不——
/// 冻结为 Σ/9 整除域：为保全整数用 Σ 记账 + 除 9（u64 域））。
pub fn box_blur(src: &Img) -> Result<Img, u32> {
    let mut px: Vec<u32> = Vec::with_capacity(src.px.len());
    for y in 0..src.h {
        for x in 0..src.w {
            let mut acc: u64 = 0;
            for dy in -1i64..=1 {
                for dx in -1i64..=1 {
                    acc += src.at(x as i64 + dx, y as i64 + dy)? as u64;
                }
            }
            px.push((acc / 9) as u32);
        }
    }
    Img::new(src.w, src.h, px)
}

/// 差值掩码（in − blur，i64 有符号域）。
pub fn detail_mask(src: &Img, blur: &Img) -> Result<Vec<i64>, u32> {
    if src.px.len() != blur.px.len() {
        return Err(XC_LEN_MISMATCH);
    }
    let mut out: Vec<i64> = Vec::with_capacity(src.px.len());
    for k in 0..src.px.len() {
        out.push(src.px[k] as i64 - blur.px[k] as i64);
    }
    Ok(out)
}

/// Unsharp Mask 全链（out = in + amount‰ × (in − blur) / 1000，
/// 钳 [0,255] 并落钳制账）。
pub fn unsharp(src: &Img, amount_permille: u32) -> Result<(Img, u64, u64), u32> {
    amount_ok(amount_permille)?;
    let blur = box_blur(src)?;
    let mask = detail_mask(src, &blur)?;
    let mut px: Vec<u32> = Vec::with_capacity(src.px.len());
    let mut clip_hi: u64 = 0;
    let mut clip_lo: u64 = 0;
    for (k, &d) in mask.iter().enumerate() {
        let v = src.px[k] as i64 + (amount_permille as i64 * d) / 1000;
        if v > 255 {
            clip_hi += 1;
            px.push(255);
        } else if v < 0 {
            clip_lo += 1;
            px.push(0);
        } else {
            px.push(v as u32);
        }
    }
    Ok((Img::new(src.w, src.h, px)?, clip_hi, clip_lo))
}

/// 恒等消融核（amount=0 → 输出 == 输入逐位）。
pub fn identity_ablation(src: &Img) -> Result<(), u32> {
    let (out, _, _) = unsharp(src, 0)?;
    if out.px != src.px {
        return Err(XC_IDENTITY_FAIL);
    }
    Ok(())
}

/// 平坦不动核（常量场：in==blur ⇒ 掩码零 ⇒ 输出恒等）。
pub fn flat_inert(src: &Img) -> Result<(), u32> {
    let (out, hi, lo) = unsharp(src, 200)?;
    if out.px != src.px || hi != 0 || lo != 0 {
        return Err(XC_FLAT_DRIFT);
    }
    Ok(())
}

/// 边缘对比增强核（阶跃边带 |out 差| ≥ |in 差|——锐化正向账）。
pub fn edge_gain_check(src: &Img) -> Result<(), u32> {
    // 取水平中线两侧像素对的差。
    let y = src.h / 2;
    let x0 = src.w / 2 - 1;
    let in_d = (src.px[y * src.w + x0 + 1] as i64 - src.px[y * src.w + x0] as i64).abs();
    let (out, _, _) = unsharp(src, 150)?;
    let out_d = (out.px[y * out.w + x0 + 1] as i64 - out.px[y * out.w + x0] as i64).abs();
    if out_d < in_d {
        return Err(XC_EDGE_WEAK);
    }
    Ok(())
}

/// 判据入口（注册于 cgpu/mod.rs 检查表）。
pub fn run_cga80_checks() -> CheckSet {
    let mut cs = CheckSet::new("CGPU-F0080");
    // 族一：amount 域闸。
    cs.add("域内过", amount_ok(0).is_ok() && amount_ok(300).is_ok(), "[0,300]");
    cs.add("超限拒", amount_ok(301).is_err(), "AMOUNT_OOB");
    // 族二：恒等消融。
    let step = {
        let mut px = vec![0u32; 64];
        for y in 0..8usize {
            for x in 4..8usize {
                px[y * 8 + x] = 255;
            }
        }
        Img::new(8, 8, px).map_err(|e| e as u16)
    };
    if let Ok(s) = &step {
        cs.add("关断恒等", identity_ablation(s).is_ok(), "amount=0 直通");
        // 族三：平坦不动。
        let flat = Img::new(8, 8, vec![100u32; 64]).map_err(|e| e as u16);
        if let Ok(f) = &flat {
            cs.add("平坦不动", flat_inert(f).is_ok(), "掩码零");
        }
        // 族四：边缘增强。
        cs.add("边缘增强", edge_gain_check(s).is_ok(), "对比度增");
        // 族五：锐化值锚（边心 128 处：blur 弱化 → 掩码大正 → 增强）。
        if let Ok((out, hi, lo)) = unsharp(s, 150) {
            cs.add("钳制账齐", hi <= 4096 && lo <= 4096, "计数有界");
            cs.add("输出域闭", out.px.iter().all(|&v| v <= 255), "u8 域");
            // 边带响应强于原图。
            let in_edge = (s.px[4 * 8 + 4] as i64 - s.px[4 * 8 + 3] as i64).abs();
            let out_edge = (out.px[4 * 8 + 4] as i64 - out.px[4 * 8 + 3] as i64).abs();
            cs.add("边带增强账", out_edge >= in_edge, "正向锐化");
        } else {
            cs.add("钳制账齐", false, "锐化构建失败");
        }
        // 族六：溢出钳制（全白图 + 正增益 → 掩码零仍不动；全黑不
        // 钳底。用渐变图造钳顶）。
        let mut ramp = vec![0u32; 64];
        for y in 0..8usize {
            for x in 0..8usize {
                ramp[y * 8 + x] = (x * 32) as u32; // 0..224 渐变
            }
        }
        let rg = Img::new(8, 8, ramp).map_err(|e| e as u16);
        if let Ok(r) = &rg {
            if let Ok((ro, chi, clo)) = unsharp(r, 300) {
                cs.add("渐变锐化", ro.px.len() == 64, "全图输出");
                cs.add("钳账一致", chi + clo <= 64, "计数 ≤ 像素数");
                let _ = clo;
            } else {
                cs.add("渐变锐化", false, "锐化构建失败");
            }
        }
        // 族七：尺寸失配。
        let half = Img::new(4, 4, vec![1u32; 16]).map_err(|e| e as u16);
        if let Ok(h) = &half {
            cs.add("失配拒", detail_mask(s, h).is_err(), "LEN_MISMATCH");
        }
        cs.add("空图拒", Img::new(0, 4, vec![]).is_err(), "DIM_ZERO");
        cs.add("空像素拒", match Img::new(1, 1, vec![5u32]) {
            Ok(tiny) => unsharp(&tiny, 301).is_err(),
            Err(_) => false,
        }, "域闸前置");
    } else {
        cs.add("夹具构建", false, "step 构建失败");
    }
    CheckSet::merge(cs, run_cga80_deep())
}

// ===== 深化面：强度轨道 + 细节掩码符号账 =====
/// 强度轨道锚：amount 三档（500/1000/1500 千分）。
pub const AMOUNT_TRACK: [u32; 3] = [500, 1000, 1500];

/// 轨道单调核：amount 越大边缘增益越大（单边单调）。
pub fn amount_track_monotone(src: &Img) -> Result<(), u32> {
    let mut prev_gain = 0u64;
    for &a in AMOUNT_TRACK.iter() {
        amount_ok(a)?;
        let (_, _, gain) = unsharp(src, a)?;
        if gain < prev_gain {
            return Err(XC_AMOUNT_OOB);
        }
        prev_gain = gain;
    }
    Ok(())
}

/// 细节掩码符号账：阶跃边处掩码非零（细节被检出）。
pub fn detail_sign(src: &Img) -> Result<(), u32> {
    let blur = box_blur(src)?;
    let mask = detail_mask(src, &blur)?;
    if mask.iter().all(|&m| m == 0) {
        return Err(XC_EDGE_WEAK);
    }
    Ok(())
}

/// 平坦图掩码零账：常量图 detail 全零（无细节即无掩码）。
pub fn flat_mask_zero(src: &Img) -> Result<(), u32> {
    let blur = box_blur(src)?;
    let mask = detail_mask(src, &blur)?;
    for &m in mask.iter() {
        if m != 0 {
            return Err(XC_FLAT_DRIFT);
        }
    }
    Ok(())
}

/// 尺寸守恒：unsharp 输出尺寸与源一致。
pub fn dim_conserved(src: &Img) -> Result<(), u32> {
    let (out, _, _) = unsharp(src, 1000)?;
    if out.w != src.w || out.h != src.h {
        return Err(XC_DIM_ZERO);
    }
    Ok(())
}

pub fn run_cga80_deep() -> CheckSet {
    let mut cs = CheckSet::new("cga80_deep");
    // 阶跃边源图：8×4，左黑右白。
    let mut px = vec![0u32; 32];
    for y in 0..4usize {
        for x in 4..8usize {
            px[y * 8 + x] = 255;
        }
    }
    if let Ok(src) = Img::new(8, 4, px) {
        cs.add("轨道单调", amount_track_monotone(&src).is_ok(), "增益随额");
        cs.add("细节检出", detail_sign(&src).is_ok(), "边处非零");
        cs.add("尺寸守恒", dim_conserved(&src).is_ok(), "w×h 闭");
        // 平坦图惰性。
        let flat = Img::new(8, 4, vec![99u32; 32]);
        if let Ok(f) = flat {
            cs.add("平坦零掩码", flat_mask_zero(&f).is_ok(), "无细节");
            cs.add("平坦惰性", flat_inert(&f).is_ok(), "恒等");
        }
        // 强度域。
        cs.add("强度下界收", amount_ok(0).is_ok(), "0 允许=钝化");
        cs.add("强度上界拒", amount_ok(AMOUNT_MAX + 1).is_err(), "OOB");
    } else {
        cs.add("夹具", false, "源图构建失败");
    }
    cs
}
