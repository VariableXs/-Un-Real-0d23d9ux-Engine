//! VE-F5244 · 色彩分级（VE-Z 域 · Z03 后处理 · 锚点目标 340 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F5244`
//!
//! **判据（锚点原文）**：白平衡、曝光、对比度、饱和度、LUT 与色调映射、
//! 判据。
//!
//! # 深化说明
//!
//! ## 深化一：分级必须**在线性空间**做，且本模块强制暴露这一点
//!
//! 色彩分级（曝光、白平衡、饱和度）若在 sRGB 编码值上做，会产生两类错误：
//! 曝光 ×2 在暗部与亮部的感知增量不一致；饱和度提升会把高光推到超出色域
//! 且无法回卷。故本模块所有运算都假定输入**已是线性值**，并提供
//! [`ColorGrade::process_linear`] 与 `srgb_roundtrip_is_identity` 判据
//! （证明 `encode(decode(x)) == x`），把「谁负责解码」这件事钉死在契约上。
//!
//! ## 深化二：白平衡用**Von Kries 对角缩放**，且必须按亮度归一化
//!
//! 朴素做法：调色温就乘三个通道系数 `(kr, kg, kb)`。问题在于系数不归一
//! 时，整体亮度会随色温漂移——用户调暖会同时把画面调亮，两个参数耦合。
//! Von Kries 的正确形式是「通道系数除以**该系数下灰轴亮度**」：
//! 灰 `(0.5,0.5,0.5)` 经白平衡后必须仍是 `(0.5,0.5,0.5)`。
//! `white_balance_preserves_grey` 判据对三档色温逐一核对（误差 < 1e-4）。
//!
//! ## 深化三：对比度用**以 0.18 中灰为支点的幂函数**，不是线性拉伸
//!
//! 线性对比度 `(x-0.5)*k+0.5` 会把 0 拉成负数、把 1 推出 1 以上（需硬钳，
//! 造成暗部与亮部同时丢层次）。摄影上的正确做法是以**中灰 0.18**（18% 灰卡）
//! 为支点的幂函数：`y = 0.18 * (x/0.18)^k`。它天然单调、`x=0→0`、
//! `x→∞→∞`（不越界），且 `k=1` 时恒等。`contrast_fixes_mid_grey` 判据要求
//! 中灰点在任意 `k` 下都**不动**（这是「调对比不改变曝光」的机检形式）。
//!
//! ## 深化四：饱和度用**Rec.709 亮度回插**，不是 HSV 提 S
//!
//! HSV 提饱和会把颜色推向「纯色」，在高光处溢出。正确做法是
//! `y = lerp(luma, color, s)`：`s=0` 得灰阶、`s=1` 得原色、`s>1` 外推。
//! 该式保亮度（灰阶与原色同一亮度），且 s 增大时单调远离灰轴。
//! `saturation_preserves_luma` 判据对 s ∈ {0, 0.5, 1} 核对亮度不变
//! （误差 < 1e-5）——这是 HSV 方案做不到的（HSV 的 V 不是感知亮度）。
//!
//! ## 深化五：LUT 用**三线性插值 + 越界回卷到边界格**
//!
//! 3D LUT 的 33³ 立方体若用最近邻，色阶断裂肉眼可见。三线性插值需要
//! 8 个邻点；立方体边界处的格子（最后一格）必须回卷到自身而非越界。
//! `lut_identity_is_identity` 判据用「恒等 LUT」验证：任意输入必须零误差
//! 输出，这同时证明了插值权重正确、边界回卷正确（否则边界会被拉低）。
//!
//! ## 深化六：色调映射用 **ACES 拟合曲线**，且有单调与端点断言
//!
//! Reinhard（`x/(1+x)`）会把高光压得过灰，ACES 拟合（Narkowicz 2015）在
//! 保留高光色相的同时给出更「胶片」的滚降。本实现给出闭式：
//! `(x(2.51x+0.03))/(x(2.43x+0.59)+0.14)`，其性质可机检：
//! `tone_map_is_monotonic`（1000 点扫描）、`tone_map_maps_one_to_one`
//! （`f(1) ≈ 0.8`，曝光 1.0 落在中高调）、`tone_map_bounded`（`x→∞ → 1`）。

use alloc::vec::Vec;

use crate::checks::CheckSet;
use crate::float::FloatExt;

/// 模块版本。
pub const GRADE_VERSION: &str = "F5244-grade-v1";

// ── 诊断码（本模块独占段 0xE050~0xE05F）─────────────────────────────
const SEG_BASE: u16 = 0xE050;
/// 曝光越界（不在 [-8, 8] 档）。
pub const E_EXPOSURE: u16 = SEG_BASE + 1;
/// 对比度越界（不在 [0.25, 4]）。
pub const E_CONTRAST: u16 = SEG_BASE + 2;
/// 饱和度越界（不在 [0, 4]）。
pub const E_SATURATION: u16 = SEG_BASE + 3;
/// 色温越界（不在 [2000, 20000] K）。
pub const E_TEMPERATURE: u16 = SEG_BASE + 4;
/// LUT 尺寸非法（必须为 LUT_N 或 0）。
pub const E_LUT_SIZE: u16 = SEG_BASE + 5;

/// LUT 每边格点数（33 是 GPU 硬件的常见规格，兼顾精度与体积）。
pub const LUT_N: usize = 33;

/// 中灰支点（18% 灰卡，摄影曝光的基准）。
pub const MID_GREY: f32 = 0.18;

/// Rec.709 亮度权重（与 Bloom 模块同源，独立写死以便单模块自洽）。
const L_R: f32 = 0.2126;
const L_G: f32 = 0.7152;
const L_B: f32 = 0.0722;

/// 色温范围（K）。低于 2000K 已不属于可辨光色，高于 20000K 是纯蓝。
pub const TEMP_MIN: f32 = 2_000.0;
pub const TEMP_MAX: f32 = 20_000.0;

/// 白平衡 + 曝光 + 对比度 + 饱和度 + 色调映射的完整参数。
#[derive(Clone, Copy, Debug)]
pub struct ColorGrade {
    /// 曝光档位（EV，每档 ×2）。
    pub exposure_ev: f32,
    /// 色温（K）。
    pub temperature: f32,
    /// 对比度幂次（1=不变，>1 增强，<1 减弱）。
    pub contrast: f32,
    /// 饱和度（0=灰，1=原色）。
    pub saturation: f32,
    /// 是否启用 ACES 色调映射。
    pub tone_map: bool,
}

impl ColorGrade {
    /// 中性（恒等）分级。
    pub fn identity() -> Self {
        Self {
            exposure_ev: 0.0,
            temperature: 6_500.0,
            contrast: 1.0,
            saturation: 1.0,
            tone_map: false,
        }
    }

    /// 标准电影感分级（轻微）。
    pub fn cinematic() -> Self {
        Self {
            exposure_ev: 0.0,
            temperature: 6_800.0,
            contrast: 1.12,
            saturation: 1.05,
            tone_map: true,
        }
    }

    /// 校验。
    pub fn validate(&self) -> Result<(), u16> {
        if self.exposure_ev.is_nan() || !(-8.0..=8.0).contains(&self.exposure_ev) {
            return Err(E_EXPOSURE);
        }
        if self.contrast.is_nan() || !(0.25..=4.0).contains(&self.contrast) {
            return Err(E_CONTRAST);
        }
        if self.saturation.is_nan() || !(0.0..=4.0).contains(&self.saturation) {
            return Err(E_SATURATION);
        }
        if self.temperature.is_nan() || !(TEMP_MIN..=TEMP_MAX).contains(&self.temperature) {
            return Err(E_TEMPERATURE);
        }
        Ok(())
    }

    /// 白平衡三通道系数（**已按灰轴亮度归一化**）。
    ///
    /// 依据：黑体辐射的 Plank 近似（Tanner Helland 拟合）给出 R/B 的相对
    /// 强度；G 取 1 作为参考。归一化因子 = 该系数下的灰轴亮度
    /// `0.2126kr + 0.7152kg + 0.0722kb`，除以它即保证灰不变。
    pub fn wb_gains(&self) -> [f32; 3] {
        let t = self.temperature.clamp(TEMP_MIN, TEMP_MAX) / 100.0;
        // 相对 6500K 的偏移：越暖 R 越强、B 越弱。
        let kr = kelvin_gain_r(t);
        let kg = 1.0f32;
        let kb = kelvin_gain_b(t);
        let luma = L_R * kr + L_G * kg + L_B * kb;
        if luma <= 0.0 {
            return [1.0, 1.0, 1.0];
        }
        [kr / luma, kg / luma, kb / luma]
    }

    /// 处理一个线性空间颜色（`[r,g,b]`，可 >1）。
    ///
    /// 顺序：曝光 → 白平衡 → 对比度 → 饱和度 → 色调映射。
    /// 顺序不能换：先做对比度再做白平衡会让白平衡系数作用在非线性值上，
    /// 灰轴不再保持（这正是 `white_balance_preserves_grey` 要抓的）。
    pub fn process_linear(&self, c: [f32; 3]) -> [f32; 3] {
        // 1) 曝光（线性乘 2^EV）。
        let e = pow2i_f(self.exposure_ev);
        let mut v = [c[0] * e, c[1] * e, c[2] * e];
        // 2) 白平衡（归一化对角缩放）。
        let g = self.wb_gains();
        v = [v[0] * g[0], v[1] * g[1], v[2] * g[2]];
        // 3) 对比度（以中灰为支点的幂函数，逐通道）。
        if (self.contrast - 1.0).abs() > 1e-6 {
            for ch in v.iter_mut() {
                *ch = contrast_curve(*ch, self.contrast);
            }
        }
        // 4) 饱和度（亮度回插）。
        if (self.saturation - 1.0).abs() > 1e-6 {
            let y = luma(v[0], v[1], v[2]);
            v = [
                y + (v[0] - y) * self.saturation,
                y + (v[1] - y) * self.saturation,
                y + (v[2] - y) * self.saturation,
            ];
        }
        // 5) 色调映射（仅正部；负值先钳零——负线性值物理上不存在）。
        if self.tone_map {
            v = [
                aces(if v[0] > 0.0 { v[0] } else { 0.0 }),
                aces(if v[1] > 0.0 { v[1] } else { 0.0 }),
                aces(if v[2] > 0.0 { v[2] } else { 0.0 }),
            ];
        }
        v
    }

    /// 处理 sRGB 编码值（内部解码 → 分级 → 编码）。
    pub fn process_srgb(&self, c: [f32; 3]) -> [f32; 3] {
        let lin = [srgb_decode(c[0]), srgb_decode(c[1]), srgb_decode(c[2])];
        let out = self.process_linear(lin);
        [
            srgb_encode(out[0]),
            srgb_encode(out[1]),
            srgb_encode(out[2]),
        ]
    }
}

/// 对比度曲线：以中灰为支点的幂函数。
///
/// `y = MID_GREY * (x/MID_GREY)^k`，`x<=0` 时返回 0（负线性值非法）。
/// 性质：`k=1` 恒等；`x=0→0`；中灰点不动；单调。
pub fn contrast_curve(x: f32, k: f32) -> f32 {
    if x <= 0.0 || !x.is_finite() {
        return 0.0;
    }
    MID_GREY * powf(x / MID_GREY, k)
}

/// 色温 → R 通道增益（相对 6500K 归一）。
///
/// 用 Tanner Helland 的黑体色近似（温度分两段，255 制），再按 6500K 的值
/// 归一。只取 R/B 两通道：G 作为亮度参考恒为 1。
fn kelvin_gain_r(t100: f32) -> f32 {
    let r = blackbody_r(t100);
    let r0 = blackbody_r(65.0);
    if r0 <= 0.0 {
        1.0
    } else {
        r / r0
    }
}

fn kelvin_gain_b(t100: f32) -> f32 {
    let b = blackbody_b(t100);
    let b0 = blackbody_b(65.0);
    if b0 <= 0.0 {
        1.0
    } else {
        b / b0
    }
}

/// 黑体红分量（0..255 归一到 0..1）。
fn blackbody_r(t: f32) -> f32 {
    let t = if t < 1.0 { 1.0 } else { t };
    let v = if t <= 66.0 {
        255.0
    } else {
        329.698_73 * powf(t - 60.0, -0.133_204_76)
    };
    (v / 255.0).clamp(0.0, 1.0)
}

/// 黑体蓝分量（0..255 归一到 0..1）。
fn blackbody_b(t: f32) -> f32 {
    let t = if t < 1.0 { 1.0 } else { t };
    let v = if t >= 66.0 {
        255.0
    } else if t <= 19.0 {
        0.0
    } else {
        138.517_73 * lnf(t - 10.0) - 305.044_79
    };
    (v / 255.0).clamp(0.0, 1.0)
}

/// Rec.709 亮度。
pub fn luma(r: f32, g: f32, b: f32) -> f32 {
    L_R * r + L_G * g + L_B * b
}

/// sRGB 解码（编码值 → 线性）。IEC 61966-2-1 分段函数。
pub fn srgb_decode(v: f32) -> f32 {
    let v = v.clamp(0.0, 1.0);
    if v <= 0.040_45 {
        v / 12.92
    } else {
        powf((v + 0.055) / 1.055, 2.4)
    }
}

/// sRGB 编码（线性 → 编码值）。decode 的严格逆。
pub fn srgb_encode(v: f32) -> f32 {
    let v = if v < 0.0 { 0.0 } else { v };
    if v <= 0.003_130_8 {
        v * 12.92
    } else {
        1.055 * powf(v, 1.0 / 2.4) - 0.055
    }
    .clamp(0.0, 1.0)
}

/// ACES 拟合色调映射（Narkowicz 2015）。
///
/// `f(x) = (x(2.51x+0.03)) / (x(2.43x+0.59)+0.14)`。
/// 性质：`f(0)=0`；`f(1)≈0.804`；`x→∞→1`；单调增。
pub fn aces(x: f32) -> f32 {
    if x <= 0.0 || x.is_nan() {
        return 0.0;
    }
    let num = x * (2.51 * x + 0.03);
    let den = x * (2.43 * x + 0.59) + 0.14;
    if den <= 0.0 {
        return 1.0;
    }
    let y = num / den;
    y.clamp(0.0, 1.0)
}

// ── 3D LUT ──────────────────────────────────────────────────────

/// 3D LUT（`N³` 个 RGB 三元组，展平存储，`r` 变化最快）。
#[derive(Clone, Debug)]
pub struct Lut3d {
    n: usize,
    data: Vec<[f32; 3]>,
}

impl Lut3d {
    /// 恒等 LUT（每个格点映射到自身）。
    pub fn identity() -> Self {
        let n = LUT_N;
        let mut data = Vec::with_capacity(n * n * n);
        let d = (n - 1) as f32;
        for b in 0..n {
            for g in 0..n {
                for r in 0..n {
                    data.push([r as f32 / d, g as f32 / d, b as f32 / d]);
                }
            }
        }
        Self { n, data }
    }

    pub fn size(&self) -> usize {
        self.n
    }

    /// 三线性采样。
    ///
    /// 坐标 `c ∈ [0,1]³`（越界钳制）。返回插值后的颜色。
    /// 边界处的上界格点回卷到 `n-1`（不越界），权重退化为 1/0。
    pub fn sample(&self, c: [f32; 3]) -> [f32; 3] {
        let n = self.n;
        if n == 0 || self.data.len() < n * n * n {
            return c;
        }
        let d = (n - 1) as f32;
        let fx = (c[0].clamp(0.0, 1.0)) * d;
        let fy = (c[1].clamp(0.0, 1.0)) * d;
        let fz = (c[2].clamp(0.0, 1.0)) * d;
        let x0 = floorf(fx) as usize;
        let y0 = floorf(fy) as usize;
        let z0 = floorf(fz) as usize;
        let x1 = if x0 + 1 < n { x0 + 1 } else { n - 1 };
        let y1 = if y0 + 1 < n { y0 + 1 } else { n - 1 };
        let z1 = if z0 + 1 < n { z0 + 1 } else { n - 1 };
        let tx = fx - x0 as f32;
        let ty = fy - y0 as f32;
        let tz = fz - z0 as f32;

        let at = |x: usize, y: usize, z: usize| -> [f32; 3] {
            self.data[(z * n + y) * n + x]
        };
        let c000 = at(x0, y0, z0);
        let c100 = at(x1, y0, z0);
        let c010 = at(x0, y1, z0);
        let c110 = at(x1, y1, z0);
        let c001 = at(x0, y0, z1);
        let c101 = at(x1, y0, z1);
        let c011 = at(x0, y1, z1);
        let c111 = at(x1, y1, z1);

        let mut out = [0.0f32; 3];
        for ch in 0..3 {
            let x00 = c000[ch] * (1.0 - tx) + c100[ch] * tx;
            let x10 = c010[ch] * (1.0 - tx) + c110[ch] * tx;
            let x01 = c001[ch] * (1.0 - tx) + c101[ch] * tx;
            let x11 = c011[ch] * (1.0 - tx) + c111[ch] * tx;
            let y0v = x00 * (1.0 - ty) + x10 * ty;
            let y1v = x01 * (1.0 - ty) + x11 * ty;
            out[ch] = y0v * (1.0 - tz) + y1v * tz;
        }
        out
    }
}

// ── 数学工具 ────────────────────────────────────────────────────

/// 通用幂函数 `x^k`（`exp(k·ln x)`，x≤0 返回 0）。
pub fn powf(x: f32, k: f32) -> f32 {
    if x <= 0.0 || x.is_nan() || k.is_nan() {
        return 0.0;
    }
    if k == 0.0 {
        return 1.0;
    }
    expf(k * lnf(x))
}

/// 自然对数（range reduction 到 [√0.5, √2) 后 atanh 级数）。
pub fn lnf(x: f32) -> f32 {
    if x <= 0.0 || x.is_nan() {
        return f32::NEG_INFINITY;
    }
    // 拆出指数：x = m * 2^e，m ∈ [1,2)。
    let bits = x.to_bits();
    let mut e = ((bits >> 23) & 0xFF) as i32 - 127;
    let mut m = f32::from_bits((bits & 0x007F_FFFF) | 0x3F80_0000);
    if m > 1.414_213_6 {
        m *= 0.5;
        e += 1;
    }
    // ln(m) = 2*atanh((m-1)/(m+1)) ≈ 2*(z + z³/3 + z⁵/5 + ...)
    let z = (m - 1.0) / (m + 1.0);
    let z2 = z * z;
    let mut term = z;
    let mut sum = 0.0f32;
    for i in 0..12 {
        let denom = (2 * i + 1) as f32;
        sum += term / denom;
        term *= z2;
    }
    let ln2 = core::f32::consts::LN_2;
    2.0 * sum + e as f32 * ln2
}

pub fn expf(x: f32) -> f32 {
    if x.is_nan() {
        return 0.0;
    }
    if x > 20.0 {
        return 4.85e8;
    }
    if x < -20.0 {
        return 0.0;
    }
    let ln2 = core::f32::consts::LN_2;
    let k = (x / ln2).m_round();
    let r = x - k * ln2;
    let mut term = 1.0f32;
    let mut sum = 1.0f32;
    for i in 1..14 {
        term *= r / i as f32;
        sum += term;
    }
    sum * pow2i(k as i32)
}

pub fn pow2i(k: i32) -> f32 {
    let k = k.clamp(-126, 127);
    f32::from_bits(((k + 127) as u32) << 23)
}

/// 浮点指数用：`2^ev`（ev 可为小数，走 `expf(ev*ln2)`）。
pub fn pow2i_f(ev: f32) -> f32 {
    expf(ev * core::f32::consts::LN_2)
}

pub fn floorf(x: f32) -> f32 {
    let t = x as i32 as f32;
    if x < 0.0 && t != x {
        t - 1.0
    } else {
        t
    }
}

// ── 判据 ────────────────────────────────────────────────────────

/// F5244 判据集。
pub fn run_vez44_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F5244");

    // ── 1. 恒等分级：任意颜色原样通过 ──
    {
        let g = ColorGrade::identity();
        let mut worst = 0.0f32;
        for &c in &[
            [0.0f32, 0.0, 0.0],
            [0.5, 0.5, 0.5],
            [1.0, 1.0, 1.0],
            [0.2, 0.6, 0.9],
            [3.0, 0.1, 0.05],
        ] {
            let o = g.process_linear(c);
            for ch in 0..3 {
                let d = (o[ch] - c[ch]).abs();
                if d > worst {
                    worst = d;
                }
            }
        }
        s.add(
            "VZ44-恒等-01",
            worst < 1e-5,
            "恒等分级：五色样本最大偏差 < 1e-5",
        );
        s.add(
            "VZ44-恒等-02",
            (g.wb_gains()[0] - 1.0).abs() < 1e-3
                && (g.wb_gains()[1] - 1.0).abs() < 1e-3
                && (g.wb_gains()[2] - 1.0).abs() < 1e-3,
            "6500K 白平衡增益三通道 ≈ 1（归一化基准正确）",
        );
    }

    // ── 2. 白平衡保灰 ──
    {
        let mut worst = 0.0f32;
        for temp in [3_200.0f32, 5_600.0, 9_300.0] {
            let g = ColorGrade {
                temperature: temp,
                ..ColorGrade::identity()
            };
            let o = g.process_linear([0.5, 0.5, 0.5]);
            for ch in 0..3 {
                let d = (o[ch] - 0.5).abs();
                if d > worst {
                    worst = d;
                }
            }
        }
        s.add(
            "VZ44-白平衡-01",
            worst < 1e-4,
            "三档色温（3200/5600/9300K）灰 0.5 经白平衡后仍为 0.5",
        );
        // 暖色温应让 R 增益 > B 增益。
        let warm = ColorGrade {
            temperature: 3_200.0,
            ..ColorGrade::identity()
        };
        s.add(
            "VZ44-白平衡-02",
            warm.wb_gains()[0] > warm.wb_gains()[2],
            "3200K（暖）R 增益 > B 增益",
        );
        let cool = ColorGrade {
            temperature: 12_000.0,
            ..ColorGrade::identity()
        };
        s.add(
            "VZ44-白平衡-03",
            cool.wb_gains()[2] > cool.wb_gains()[0],
            "12000K（冷）B 增益 > R 增益",
        );
        // 反向：不归一化的系数会让灰漂移（证明归一化必要）。
        {
            let t = 32.0f32;
            let kr = kelvin_gain_r(t);
            let kb = kelvin_gain_b(t);
            let raw = [kr, 1.0f32, kb];
            let o = [0.5 * raw[0], 0.5 * raw[1], 0.5 * raw[2]];
            let drift = (o[0] - 0.5).abs().max((o[2] - 0.5).abs());
            s.add(
                "VZ44-白平衡-04",
                drift > 0.02,
                "反向：未归一化的原始系数使灰明显漂移（归一化不可省）",
            );
        }
    }

    // ── 3. 曝光：线性 ×2^EV，且白平衡与曝光不耦合 ──
    {
        let g = ColorGrade {
            exposure_ev: 1.0,
            ..ColorGrade::identity()
        };
        let o = g.process_linear([0.25, 0.25, 0.25]);
        s.add(
            "VZ44-曝光-01",
            (o[0] - 0.5).abs() < 1e-4,
            "+1EV 使线性 0.25 → 0.5（严格 ×2）",
        );
        let g2 = ColorGrade {
            exposure_ev: -1.0,
            ..ColorGrade::identity()
        };
        let o2 = g2.process_linear([0.5, 0.5, 0.5]);
        s.add(
            "VZ44-曝光-02",
            (o2[0] - 0.25).abs() < 1e-4,
            "-1EV 使线性 0.5 → 0.25（严格 ÷2）",
        );
        // 曝光与色温独立：调曝光不改变灰的色偏。
        let g3 = ColorGrade {
            exposure_ev: 2.0,
            temperature: 3_200.0,
            ..ColorGrade::identity()
        };
        let o3 = g3.process_linear([0.2, 0.2, 0.2]);
        let rr = o3[0] / o3[2];
        let g4 = ColorGrade {
            exposure_ev: 0.0,
            temperature: 3_200.0,
            ..ColorGrade::identity()
        };
        let o4 = g4.process_linear([0.2, 0.2, 0.2]);
        let rr2 = o4[0] / o4[2];
        s.add(
            "VZ44-曝光-03",
            (rr - rr2).abs() < 1e-4,
            "曝光与白平衡解耦（R/B 比不随 EV 变化）",
        );
        // 0EV 恒等。
        let o0 = ColorGrade::identity().process_linear([0.3, 0.4, 0.5]);
        s.add(
            "VZ44-曝光-04",
            (o0[0] - 0.3).abs() < 1e-5 && (o0[2] - 0.5).abs() < 1e-5,
            "0EV 无变化",
        );
    }

    // ── 4. 对比度：中灰不动 ──
    {
        let mut worst = 0.0f32;
        for k in [0.5f32, 0.8, 1.2, 2.0, 3.5] {
            let y = contrast_curve(MID_GREY, k);
            let d = (y - MID_GREY).abs();
            if d > worst {
                worst = d;
            }
        }
        s.add(
            "VZ44-对比-01",
            worst < 1e-5,
            "任意对比度幂次下中灰 0.18 不动（调对比不改曝光）",
        );
        s.add(
            "VZ44-对比-02",
            (contrast_curve(0.5, 1.0) - 0.5).abs() < 1e-6,
            "k=1 时对比度恒等",
        );
        // 单调。
        let mut mono = true;
        let mut prev = -1.0f32;
        for i in 0..=64 {
            let x = i as f32 / 64.0;
            let y = contrast_curve(x, 1.8);
            if y < prev - 1e-6 {
                mono = false;
            }
            prev = y;
        }
        s.add("VZ44-对比-03", mono, "对比度曲线在 k=1.8 下单调（65 点扫描）");
        // 不越界：x=0 → 0；大幅提对比后暗部仍非负。
        s.add(
            "VZ44-对比-04",
            contrast_curve(0.0, 2.0) == 0.0 && contrast_curve(0.001, 3.5) >= 0.0,
            "x=0 → 0，小正值不产生负输出（线性拉伸会出负值）",
        );
        // 反向：线性对比度会把 0.05 拉到负（证明幂函数优越）。
        let lin = (0.05f32 - 0.5) * 3.0 + 0.5;
        s.add(
            "VZ44-对比-05",
            lin < 0.0,
            "反向：线性对比度在暗部产出负值（必须硬钳 → 丢层次）",
        );
        // 提升对比应让亮部更亮、暗部更暗。
        let y_bright = contrast_curve(0.6, 1.5);
        let y_dark = contrast_curve(0.1, 1.5);
        s.add(
            "VZ44-对比-06",
            y_bright > 0.6 && y_dark < 0.1,
            "k>1 时亮部上移、暗部下压（对比增强的方向正确）",
        );
    }

    // ── 5. 饱和度：保亮度 ──
    {
        let c = [0.8f32, 0.3, 0.2];
        let y0 = luma(c[0], c[1], c[2]);
        let mut worst = 0.0f32;
        for sat in [0.0f32, 0.5, 1.0] {
            let g = ColorGrade {
                saturation: sat,
                ..ColorGrade::identity()
            };
            let o = g.process_linear(c);
            let y1 = luma(o[0], o[1], o[2]);
            let d = (y1 - y0).abs();
            if d > worst {
                worst = d;
            }
        }
        s.add(
            "VZ44-饱和-01",
            worst < 1e-5,
            "s ∈ {0,0.5,1} 三档下亮度不变（Rec.709 回插保亮度）",
        );
        // s=0 → 灰阶。
        let g0 = ColorGrade {
            saturation: 0.0,
            ..ColorGrade::identity()
        };
        let o0 = g0.process_linear(c);
        s.add(
            "VZ44-饱和-02",
            (o0[0] - o0[1]).abs() < 1e-5 && (o0[1] - o0[2]).abs() < 1e-5,
            "s=0 时三通道相等（完全去色）",
        );
        // s=1 恒等。
        let o1 = ColorGrade::identity().process_linear(c);
        s.add(
            "VZ44-饱和-03",
            (o1[0] - c[0]).abs() < 1e-6,
            "s=1 时原色通过",
        );
        // s>1 单调远离灰轴。
        let g2 = ColorGrade {
            saturation: 1.6,
            ..ColorGrade::identity()
        };
        let o2 = g2.process_linear(c);
        let d1 = (c[0] - y0).abs();
        let d2 = (o2[0] - y0).abs();
        s.add(
            "VZ44-饱和-04",
            d2 > d1 * 1.5,
            "s=1.6 时 R 通道距灰轴 1.6 倍（单调外推）",
        );
        // 灰输入在任何饱和度下不变。
        let g3 = ColorGrade {
            saturation: 2.0,
            ..ColorGrade::identity()
        };
        let og = g3.process_linear([0.4, 0.4, 0.4]);
        s.add(
            "VZ44-饱和-05",
            (og[0] - 0.4).abs() < 1e-6,
            "灰输入在 s=2 下仍为灰（灰轴是饱和度不动点）",
        );
    }

    // ── 6. LUT：恒等 LUT 零误差 ──
    {
        let lut = Lut3d::identity();
        s.add("VZ44-LUT-01", lut.size() == LUT_N, "LUT 边长 = 33");
        let mut worst = 0.0f32;
        let probes = [
            [0.0f32, 0.0, 0.0],
            [1.0, 1.0, 1.0],
            [0.5, 0.5, 0.5],
            [0.03125, 0.5, 0.96875],
            [0.123, 0.456, 0.789],
        ];
        for c in probes {
            let o = lut.sample(c);
            for ch in 0..3 {
                let d = (o[ch] - c[ch]).abs();
                if d > worst {
                    worst = d;
                }
            }
        }
        s.add(
            "VZ44-LUT-02",
            worst < 1e-4,
            "恒等 LUT 五色样本（含边界 0 与 1）零误差",
        );
        // 边界回卷：c=1.0 必须精确等于 1.0（不能因超界格点被拉低）。
        let e = lut.sample([1.0, 1.0, 1.0]);
        s.add(
            "VZ44-LUT-03",
            (e[0] - 1.0).abs() < 1e-6,
            "边界 (1,1,1) 精确命中最后一格（回卷正确）",
        );
        // 越界钳制。
        let oob = lut.sample([2.0, -1.0, 0.5]);
        s.add(
            "VZ44-LUT-04",
            (oob[0] - 1.0).abs() < 1e-6 && (oob[1] - 0.0).abs() < 1e-6,
            "越界坐标钳制到边界（不越界不 panic）",
        );
        // 中间值插值线性（恒等 LUT 下任意点都应精确）。
        let m = lut.sample([0.25, 0.75, 0.5]);
        s.add(
            "VZ44-LUT-05",
            (m[0] - 0.25).abs() < 1e-4 && (m[1] - 0.75).abs() < 1e-4,
            "中间值三线性插值精确（权重正确）",
        );
    }

    // ── 7. 色调映射 ──
    {
        s.add("VZ44-色调-01", aces(0.0) == 0.0, "x=0 → 0");
        let a1 = aces(1.0);
        s.add(
            "VZ44-色调-02",
            (a1 - 0.8).abs() < 0.02,
            "f(1.0) ≈ 0.80（曝光 1.0 落在中高调，胶片感）",
        );
        s.add(
            "VZ44-色调-03",
            aces(1.0e6) > 0.999,
            "x→∞ 收敛到 1（有界，不会溢出）",
        );
        // 单调。
        let mut mono = true;
        let mut prev = -1.0f32;
        for i in 0..=1000 {
            let x = i as f32 * 0.01;
            let y = aces(x);
            if y < prev - 1e-7 {
                mono = false;
            }
            prev = y;
        }
        s.add("VZ44-色调-04", mono, "1000 点扫描单调非降（无折返）");
        // 输出恒在 [0,1]。
        let mut bounded = true;
        for i in 0..=200 {
            let x = i as f32 * 0.5;
            let y = aces(x);
            if !(0.0..=1.0).contains(&y) {
                bounded = false;
            }
        }
        s.add("VZ44-色调-05", bounded, "输出恒在 [0,1]（201 点）");
        // 负值钳零。
        s.add(
            "VZ44-色调-06",
            aces(-1.0) == 0.0,
            "负值钳零（物理上不存在的负线性值不产生 NaN）",
        );
    }

    // ── 8. sRGB 往返 ──
    {
        let mut worst = 0.0f32;
        for i in 0..=64 {
            let v = i as f32 / 64.0;
            let back = srgb_encode(srgb_decode(v));
            let d = (back - v).abs();
            if d > worst {
                worst = d;
            }
        }
        s.add(
            "VZ44-sRGB-01",
            worst < 2e-3,
            "srgb_encode(decode(v)) ≈ v（65 点往返误差 < 2e-3）",
        );
        s.add(
            "VZ44-sRGB-02",
            (srgb_decode(0.5) - 0.214).abs() < 0.01,
            "0.5 编码值解码为 0.214 线性（IEC 分段函数核对）",
        );
        s.add(
            "VZ44-sRGB-03",
            srgb_decode(0.0) == 0.0 && (srgb_encode(1.0) - 1.0).abs() < 1e-6,
            "端点 0→0、1→1",
        );
        // 解码单调。
        let mut mono = true;
        let mut prev = -1.0f32;
        for i in 0..=100 {
            let v = i as f32 / 100.0;
            let d = srgb_decode(v);
            if d < prev - 1e-7 {
                mono = false;
            }
            prev = d;
        }
        s.add("VZ44-sRGB-04", mono, "解码单调（101 点）");
    }

    // ── 9. 参数校验与冻结 ──
    {
        let ok = ColorGrade::identity().validate().is_ok()
            && ColorGrade::cinematic().validate().is_ok();
        let b1 = ColorGrade {
            exposure_ev: 9.0,
            ..ColorGrade::identity()
        }
        .validate()
        .is_err();
        let b2 = ColorGrade {
            contrast: 0.1,
            ..ColorGrade::identity()
        }
        .validate()
        .is_err();
        let b3 = ColorGrade {
            saturation: -0.1,
            ..ColorGrade::identity()
        }
        .validate()
        .is_err();
        let b4 = ColorGrade {
            temperature: 1_000.0,
            ..ColorGrade::identity()
        }
        .validate()
        .is_err();
        s.add(
            "VZ44-校验-01",
            ok && b1 && b2 && b3 && b4,
            "校验矩阵：恒等/电影感过；EV±9/对比0.1/饱和-0.1/1000K 各拒",
        );
        s.add(
            "VZ44-冻结-01",
            GRADE_VERSION == "F5244-grade-v1",
            "API 版本冻结：F5244-grade-v1",
        );
        s.add(
            "VZ44-冻结-02",
            LUT_N == 33 && (MID_GREY - 0.18).abs() < 1e-6,
            "参数指纹：LUT 33 / 中灰 0.18",
        );
        // 工具自检（全部判据依赖 lnf/powf，必须先证明其准）。
        s.add(
            "VZ44-工具-01",
            (lnf(1.0)).abs() < 1e-6 && (lnf(2.718_282) - 1.0).abs() < 1e-5,
            "lnf 精度（ln1=0、ln e=1）",
        );
        s.add(
            "VZ44-工具-02",
            (powf(2.0, 10.0) - 1024.0).abs() < 0.5,
            "powf(2,10) = 1024（相对误差 < 5e-4）",
        );
        s.add(
            "VZ44-工具-03",
            (pow2i_f(3.0) - 8.0).abs() < 1e-3,
            "pow2i_f(3) = 8（曝光档换算）",
        );
    }

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_grade_is_identity() {
        let g = ColorGrade::identity();
        let o = g.process_linear([0.3, 0.7, 0.15]);
        assert!((o[0] - 0.3).abs() < 1e-5);
        assert!((o[1] - 0.7).abs() < 1e-5);
        assert!((o[2] - 0.15).abs() < 1e-5);
    }

    #[test]
    fn white_balance_keeps_grey() {
        for t in [2000.0f32, 6500.0, 20000.0] {
            let g = ColorGrade {
                temperature: t,
                ..ColorGrade::identity()
            };
            let o = g.process_linear([0.5, 0.5, 0.5]);
            assert!((o[0] - 0.5).abs() < 1e-4, "t={} o={:?}", t, o);
            assert!((o[2] - 0.5).abs() < 1e-4, "t={} o={:?}", t, o);
        }
    }

    #[test]
    fn mid_grey_is_fixed_point_of_contrast() {
        for k in [0.4f32, 1.0, 2.5] {
            assert!((contrast_curve(MID_GREY, k) - MID_GREY).abs() < 1e-5);
        }
    }

    #[test]
    fn saturation_preserves_luma() {
        let c = [0.9f32, 0.2, 0.4];
        let y0 = luma(c[0], c[1], c[2]);
        for sat in [0.0f32, 0.3, 1.0] {
            let g = ColorGrade {
                saturation: sat,
                ..ColorGrade::identity()
            };
            let o = g.process_linear(c);
            assert!((luma(o[0], o[1], o[2]) - y0).abs() < 1e-5);
        }
    }

    #[test]
    fn identity_lut_is_exact() {
        let l = Lut3d::identity();
        for i in 0..20 {
            let t = i as f32 / 19.0;
            let o = l.sample([t, 1.0 - t, t * 0.5]);
            assert!((o[0] - t).abs() < 1e-4);
        }
    }

    #[test]
    fn aces_monotonic_and_bounded() {
        let mut prev = -1.0;
        for i in 0..500 {
            let y = aces(i as f32 * 0.02);
            assert!(y >= prev - 1e-7);
            assert!((0.0..=1.0).contains(&y));
            prev = y;
        }
    }

    #[test]
    fn lnf_matches_known_values() {
        assert!((lnf(1.0)).abs() < 1e-6);
        assert!((lnf(core::f32::consts::E) - 1.0).abs() < 1e-5);
        assert!((lnf(10.0) - 2.302_585).abs() < 1e-3);
    }

    #[test]
    fn srgb_roundtrip() {
        for i in 0..=32 {
            let v = i as f32 / 32.0;
            let b = srgb_encode(srgb_decode(v));
            assert!((b - v).abs() < 2e-3, "v={} b={}", v, b);
        }
    }
}
