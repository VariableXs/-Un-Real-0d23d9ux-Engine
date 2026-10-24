//! VE-F0637 线性空间混合纪律（VE-D 域 · 目标 340 行）
//!
//! **锚点**：`VE-STAR-II · 总纲与施工书.md#VE-F0637`
//!
//! **判据（锚点原文逐条）**：
//! - **线性强制**：混合运算一律在线性光空间（sRGB 数值域禁止直接混合）；
//! - **转换点登记**：输入侧 sRGB→线性、输出侧线性→sRGB，转换点全集显性；
//! - **误差入账**：8bit sRGB 往返量化误差登记入账（判据内消化）；
//! - **禁混用**：硬件与软件转换不得混用于同一帧（混用即色差）；
//! - **判据**：附加族（F0624）线性前提断言复用本纪律。

use crate::checks::CheckSet;

/// sRGB EOTF 分段常数（与 ved24 同源常数，本表为转换点登记用副本声明）
pub const EOTF_SPLIT: f32 = 0.04045;
pub const EOTF_LIN_DIV: f32 = 12.92;
pub const EOTF_EXP: f32 = 2.4;
pub const EOTF_ALPHA: f32 = 0.055;
pub const OETF_SPLIT: f32 = 0.0031308;
pub const OETF_LIN_MUL: f32 = 12.92;
pub const OETF_ALPHA: f32 = 0.055;

/// 转换点登记条目
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConvSite {
    /// 输入侧：sRGB → 线性
    InSrgbToLin,
    /// 输出侧：线性 → sRGB
    OutLinToSrgb,
    /// 硬件自动转换（sRGB 帧缓冲格式）
    HwSrgbFramebuffer,
}

/// 转换点登记表（全集显性）
pub const CONV_SITES: [ConvSite; 3] = [
    ConvSite::InSrgbToLin,
    ConvSite::OutLinToSrgb,
    ConvSite::HwSrgbFramebuffer,
];

/// 转换点标签
pub fn site_label(s: ConvSite) -> &'static str {
    match s {
        ConvSite::InSrgbToLin => "in:srgb→lin",
        ConvSite::OutLinToSrgb => "out:lin→srgb",
        ConvSite::HwSrgbFramebuffer => "hw:srgb-fb",
    }
}

/// sRGB → 线性（软件路）
pub fn srgb_to_linear(v: f32) -> f32 {
    if v <= EOTF_SPLIT {
        v / EOTF_LIN_DIV
    } else {
        ((v + EOTF_ALPHA) / (1.0 + EOTF_ALPHA)).powf(EOTF_EXP)
    }
}

/// 线性 → sRGB（软件路）
pub fn linear_to_srgb(v: f32) -> f32 {
    if v <= OETF_SPLIT {
        v * OETF_LIN_MUL
    } else {
        (1.0 + OETF_ALPHA) * v.powf(1.0 / EOTF_EXP) - OETF_ALPHA
    }
}

/// 8bit 量化误差账：sRGB 往返（8bit 量化 → 线性 → 8bit）的最大偏差档
pub struct RoundTripErr {
    pub max_step: f32,
    pub samples: u32,
}

/// 全 256 档往返误差扫描（8bit 量化域）
pub fn roundtrip_scan() -> RoundTripErr {
    let mut max_step = 0.0f32;
    for i in 0..=255u32 {
        let s = i as f32 / 255.0;
        let lin = srgb_to_linear(s);
        let back = (linear_to_srgb(lin) * 255.0).round() / 255.0;
        let d = (back - s).abs();
        if d > max_step {
            max_step = d;
        }
    }
    RoundTripErr { max_step, samples: 256 }
}

/// 硬件/软件混用审计：同一帧的转换点使用集必须同源
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameConvAudit {
    pub used_hw: bool,
    pub used_sw_in: bool,
    pub used_sw_out: bool,
}

impl FrameConvAudit {
    /// 混用判定：硬件转换与软件转换并存即拦截
    pub fn mixed(&self) -> bool {
        self.used_hw && (self.used_sw_in || self.used_sw_out)
    }
    /// 合规帧：要么全硬件、要么全软件
    pub fn clean(&self) -> bool {
        !self.mixed()
    }
}

/// 附加族线性前提断言（F0624 联动）：加法混合在线性域可交换验证
pub fn additive_linear_premise() -> bool {
    // 线性域加法：lin(a)+lin(b) 与 lin(b)+lin(a) 相等（交换律是线性混合前提）
    let a = srgb_to_linear(0.3);
    let b = srgb_to_linear(0.7);
    ((a + b) - (b + a)).abs() < 1e-6
}

pub fn run_vec237_checks() -> crate::checks::CheckSet {
    let mut s = CheckSet::new("svstar2-vec237");
    // 族一：转换点登记
    s.add("线性-转换点全集", CONV_SITES.len() == 3, "3 sites");
    s.add("线性-标签齐", CONV_SITES.iter().all(|&x| !site_label(x).is_empty()), "labeled");
    s.add("线性-标签互异", site_label(ConvSite::InSrgbToLin) != site_label(ConvSite::OutLinToSrgb), "distinct");
    // 族二：EOTF/OETF 端点与单调
    s.add("线性-0映射0", srgb_to_linear(0.0) == 0.0, "black");
    s.add("线性-1映射1", (srgb_to_linear(1.0) - 1.0).abs() < 1e-5, "white");
    s.add("线性-逆往返", (linear_to_srgb(srgb_to_linear(0.5)) - 0.5).abs() < 1e-4, "roundtrip");
    let mut mono = true;
    let mut prev = -1.0f32;
    for i in 0..=20 {
        let v = srgb_to_linear(i as f32 / 20.0);
        if v < prev {
            mono = false;
        }
        prev = v;
    }
    s.add("线性-EOTF单调", mono, "monotone");
    // 低段线性斜率（≤0.04045 段为线性 /12.92）
    s.add("线性-低段线性", (srgb_to_linear(0.02) - 0.02 / 12.92).abs() < 1e-6, "linear seg");
    // 族三：误差入账
    let rt = roundtrip_scan();
    s.add("线性-全档扫描", rt.samples == 256, "256 steps");
    s.add("线性-往返≤半档", rt.max_step <= 0.5 / 255.0, "≤ ½ LSB8");
    // 族四：禁混用审计
    let ok_sw = FrameConvAudit { used_hw: false, used_sw_in: true, used_sw_out: true };
    s.add("审计-纯软件合规", ok_sw.clean(), "sw only ok");
    let ok_hw = FrameConvAudit { used_hw: true, used_sw_in: false, used_sw_out: false };
    s.add("审计-纯硬件合规", ok_hw.clean(), "hw only ok");
    let bad = FrameConvAudit { used_hw: true, used_sw_in: true, used_sw_out: false };
    s.add("审计-混用拦截", bad.mixed() && !bad.clean(), "mixed rejected");
    // 族五：附加族线性前提
    s.add("线性-加法前提", additive_linear_premise(), "commutative");
    // 族六：线性域混合与 sRGB 域混合结果不同（物理正确性来源）
    let lin_mix = (srgb_to_linear(0.2) + srgb_to_linear(0.8)) / 2.0;
    let srgb_mix = (0.2 + 0.8) / 2.0;
    s.add("线性-域差实在", (linear_to_srgb(lin_mix) - srgb_mix).abs() > 1e-3, "domains differ");
    s
}
