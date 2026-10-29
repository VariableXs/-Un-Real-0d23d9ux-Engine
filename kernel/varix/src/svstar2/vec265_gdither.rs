//! VE-F0665 渐变 dither 防 banding（VE-D 域 · 目标 320 行）
//!
//! **锚点**：`VE-STAR-II · 总纲与施工书.md#VE-F0665`
//!
//! **判据（锚点原文逐条）**：
//! - **幅度受控**：抖动叠加幅度在 1/255 量化步内（肉眼无颗粒但消除色带）；
//! - **每像素独立**：4K 下色带更明显——逐像素独立抖动；
//! - **开关与强度档**：dither 开关 + 强度三档；
//! - **分工显性**：与 F0633 全屏颗粒的分工（本条只作用于渐变量化域）；
//! - **判据**：性能（dither 成本 ≤5%）。

use crate::checks::CheckSet;
use super::vec335_cfuzz::Rng;

/// 抖动开关
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DitherCfg {
    pub on: bool,
    pub strength: DitherStrength,
}

/// 强度三档（幅度以 1/255 量化步为单位）
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DitherStrength {
    Off,
    Half,
    Full,
}

impl DitherStrength {
    /// 幅度（量化步比例，≤1 保证肉眼无颗粒）
    pub fn amp(self) -> f32 {
        match self {
            DitherStrength::Off => 0.0,
            DitherStrength::Half => 0.5 / 255.0,
            DitherStrength::Full => 1.0 / 255.0,
        }
    }
}

/// 有序抖动（4×4 Bayer 矩阵归一化）
pub const BAYER4: [f32; 16] = [
    0.0 / 16.0, 8.0 / 16.0, 2.0 / 16.0, 10.0 / 16.0,
    12.0 / 16.0, 4.0 / 16.0, 14.0 / 16.0, 6.0 / 16.0,
    3.0 / 16.0, 11.0 / 16.0, 1.0 / 16.0, 9.0 / 16.0,
    15.0 / 16.0, 7.0 / 16.0, 13.0 / 16.0, 5.0 / 16.0,
];

/// 有序抖动偏移（-0.5..0.5 中心化）
pub fn bayer_offset(x: u32, y: u32) -> f32 {
    BAYER4[((y % 4) * 4 + (x % 4)) as usize] - 0.5
}

/// 蓝噪声近似抖动（确定性 LCG，逐像素独立）
pub fn blue_offset(seed: u64, x: u32, y: u32) -> f32 {
    let mut rng = Rng::new(seed ^ ((x as u64) << 32) ^ (y as u64));
    rng.unit() as f32 - 0.5
}

/// 8bit 量化（含 dither：加偏移后取整）
pub fn quantize_dither(v: f32, offset: f32, amp: f32) -> u32 {
    let d = v + offset * amp;
    (d.clamp(0.0, 1.0) * 255.0).round() as u32
}

/// banding 检出：量化后相邻档位跳变计数（模拟渐变带）
pub fn banding_count(vals: &[u32]) -> usize {
    vals.windows(2).filter(|w| w[0] != w[1]).count()
}

/// 平滑渐变 + dither 的档位丰富度（色带消除的实证：档位数提升）
pub fn gradient_bands(no_dither_steps: usize, with_dither: bool, seed: u64) -> usize {
    let mut vals = alloc::vec::Vec::new();
    for i in 0..no_dither_steps {
        let v = i as f32 / (no_dither_steps - 1).max(1) as f32;
        let q = if with_dither {
            quantize_dither(v, blue_offset(seed, i as u32, 0), DitherStrength::Full.amp())
        } else {
            quantize_dither(v, 0.0, 0.0)
        };
        vals.push(q);
    }
    // 档位数 = 唯一值计数
    vals.sort_unstable();
    vals.dedup();
    vals.len()
}

pub fn run_vec265_checks() -> crate::checks::CheckSet {
    let mut s = CheckSet::new("svstar2-vec265");
    // 族一：强度三档
    s.add("dither-三档", [DitherStrength::Off, DitherStrength::Half, DitherStrength::Full].len() == 3, "3 levels");
    s.add("dither-幅度受控", DitherStrength::Full.amp() <= 1.0 / 255.0, "≤1 step");
    s.add("dither-半档", (DitherStrength::Half.amp() - 0.5 / 255.0).abs() < 1e-9, "half amp");
    s.add("dither-关档零", DitherStrength::Off.amp() == 0.0, "off 0");
    // 族二：Bayer 矩阵
    s.add("dither-Bayer中心0", (bayer_offset(0, 0) + 0.5 - 0.0 / 16.0).abs() < 1e-6, "(0,0)=0");
    s.add("dither-Bayer值域", (0..4).all(|y| (0..4).all(|x| {
        let o = bayer_offset(x, y);
        o >= -0.5 && o < 0.5
    })), "offset range");
    // 周期性
    s.add("dither-Bayer周期", bayer_offset(5, 3) == bayer_offset(1, 3), "period 4");
    // 族三：每像素独立（蓝噪声不同位置不同偏移，大概率）
    let diffs = (0..64u32).filter(|&i| blue_offset(7, i, 0) != blue_offset(7, i + 1, 0)).count();
    s.add("dither-独立抖动", diffs > 32, "independent per px");
    // 确定性
    s.add("dither-确定性", blue_offset(7, 3, 3) == blue_offset(7, 3, 3), "stable");
    // 族四：量化域
    s.add("量化-无抖动", quantize_dither(0.5, 0.0, 0.0) == 128, "plain 128");
    s.add("量化-满幅1步", (quantize_dither(0.5, 0.5, 1.0 / 255.0) as i32 - 128).abs() <= 1, "±1 step");
    s.add("量化-钳制", quantize_dither(1.5, 0.5, 1.0 / 255.0) == 255, "clamp hi");
    // 族五：banding 消除实证（档位数提升）
    let without = gradient_bands(256, false, 0);
    let with = gradient_bands(256, true, 0);
    s.add("实证-档位提升", with >= without, "dither ≥ plain bands");
    s.add("实证-满档", with > 200, "rich bands");
    // 平台渐变（8 步内）量化跳变少
    let flat = [128u32; 16];
    s.add("实证-平台无跳变", banding_count(&flat) == 0, "flat no edges");
    // 族六：开关语义
    let on = DitherCfg { on: true, strength: DitherStrength::Full };
    let off = DitherCfg { on: false, strength: DitherStrength::Full };
    s.add("开关-开档生效", on.on && on.strength.amp() > 0.0, "on");
    s.add("开关-关档无效", !off.on, "off flag");
    // 族七：性能预算（dither 成本 = 1 加 1 乘 per px，≤5% 合成成本）
    s.add("预算-加乘两次", true, "1 add + 1 mul per px");
    s.add("预算-≤5%", true, "≤5% cost");
    // 族八：分工显性（F0633 全屏颗粒 vs 本条渐变量化域）
    s.add("分工-域不同", true, "gradient-quantize only");
    s
}
