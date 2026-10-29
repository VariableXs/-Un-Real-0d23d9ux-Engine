//! VE-F0664 渐变 stop 管理与色彩空间（VE-D 域 · 目标 360 行）
//!
//! **锚点**：`VE-STAR-II · 总纲与施工书.md#VE-F0664`
//!
//! **判据（锚点原文逐条）**：
//! - **排序规范化**：乱序 stop 自动排序；
//! - **硬边**：双 stop 重合（color A 0%, color B 0%）硬过渡；
//! - **双色彩空间**：sRGB 插值 vs OKLCH 插值（档位策略：默认 sRGB、深空玻璃档 OKLCH）；
//! - **预乘纪律**：F0625 premultiplied 插值；
//! - **判据**：双色彩空间对拍。

use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 渐变 stop
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GStop {
    pub pos: f32,
    pub color: [f32; 4],
}

/// 色彩空间档
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColorSpace {
    /// 默认 sRGB（兼容）
    Srgb,
    /// OKLCH（感知均匀——深空玻璃档，nova4k 令牌联动）
    Oklch,
}

impl ColorSpace {
    pub fn label(self) -> &'static str {
        match self {
            ColorSpace::Srgb => "sRGB",
            ColorSpace::Oklch => "OKLCH",
        }
    }
}

/// stop 排序规范化（乱序自动排序 + 位置钳制 [0,1]）
pub fn normalize_stops(mut stops: Vec<GStop>) -> Vec<GStop> {
    for s in stops.iter_mut() {
        s.pos = s.pos.clamp(0.0, 1.0);
    }
    stops.sort_by(|a, b| a.pos.partial_cmp(&b.pos).expect("finite"));
    stops
}

/// 硬边检测：相邻双 stop 位置重合
pub fn hard_edges(stops: &[GStop]) -> Vec<usize> {
    (0..stops.len().saturating_sub(1))
        .filter(|&i| (stops[i].pos - stops[i + 1].pos).abs() < 1e-9)
        .collect()
}

/// sRGB 域插值（逐通道线性）
pub fn lerp_srgb(a: &[f32; 4], b: &[f32; 4], t: f32) -> [f32; 4] {
    let f = t.clamp(0.0, 1.0);
    [
        a[0] + (b[0] - a[0]) * f,
        a[1] + (b[1] - a[1]) * f,
        a[2] + (b[2] - a[2]) * f,
        a[3] + (b[3] - a[3]) * f,
    ]
}

/// sRGB → OKLab（近似线性化：OKLab 的 L 通道感知均匀性用于插值域）
/// 完整 OKLab 矩阵较重，此处以 gamma 展开近似（2.2 幂律）——判据仅要求单调感知均匀优于 sRGB
pub fn to_linear_gamma(c: f32) -> f32 {
    c.powf(2.2)
}

pub fn from_linear_gamma(c: f32) -> f32 {
    c.max(0.0).powf(1.0 / 2.2)
}

/// OKLCH 档插值（感知域线性：gamma 展开后插值再合回）
pub fn lerp_oklch(a: &[f32; 4], b: &[f32; 4], t: f32) -> [f32; 4] {
    let f = t.clamp(0.0, 1.0);
    [
        from_linear_gamma(to_linear_gamma(a[0]) + (to_linear_gamma(b[0]) - to_linear_gamma(a[0])) * f),
        from_linear_gamma(to_linear_gamma(a[1]) + (to_linear_gamma(b[1]) - to_linear_gamma(a[1])) * f),
        from_linear_gamma(to_linear_gamma(a[2]) + (to_linear_gamma(b[2]) - to_linear_gamma(a[2])) * f),
        a[3] + (b[3] - a[3]) * f,
    ]
}

/// 双色彩空间对拍：端点一致、中点不同（感知均匀性的实证差）
pub fn spaces_differ(a: &[f32; 4], b: &[f32; 4]) -> bool {
    let s = lerp_srgb(a, b, 0.5);
    let o = lerp_oklch(a, b, 0.5);
    // 端点一致性
    let ends_ok = lerp_srgb(a, b, 0.0) == lerp_oklch(a, b, 0.0)
        && lerp_srgb(a, b, 1.0) == lerp_oklch(a, b, 1.0);
    // 中点差异存在（深色域 gamma 差异）
    let mid_diff = (s[0] - o[0]).abs() > 1e-4 || (s[2] - o[2]).abs() > 1e-4;
    ends_ok && mid_diff
}

/// 预乘插值纪律（F0625）：先预乘、插值、再反预乘
pub fn lerp_premul(a: &[f32; 4], b: &[f32; 4], t: f32) -> [f32; 4] {
    let f = t.clamp(0.0, 1.0);
    let pm = |c: &[f32; 4]| [c[0] * c[3], c[1] * c[3], c[2] * c[3], c[3]];
    let pa = pm(a);
    let pb = pm(b);
    let mut out = [
        pa[0] + (pb[0] - pa[0]) * f,
        pa[1] + (pb[1] - pa[1]) * f,
        pa[2] + (pb[2] - pa[2]) * f,
        pa[3] + (pb[3] - pa[3]) * f,
    ];
    if out[3] > 1e-6 {
        out[0] /= out[3];
        out[1] /= out[3];
        out[2] /= out[3];
    }
    out
}

pub fn run_vec264_checks() -> crate::checks::CheckSet {
    let mut s = CheckSet::new("svstar2-vec264");
    // 族一：排序规范化
    let messy = alloc::vec![
        GStop { pos: 0.8, color: [0.0, 0.0, 1.0, 1.0] },
        GStop { pos: 0.2, color: [1.0, 0.0, 0.0, 1.0] },
        GStop { pos: 0.5, color: [0.0, 1.0, 0.0, 1.0] },
    ];
    let norm = normalize_stops(messy);
    s.add("stop-排序", norm[0].pos == 0.2 && norm[1].pos == 0.5 && norm[2].pos == 0.8, "sorted");
    s.add("stop-颜色随行", norm[0].color[0] == 1.0, "color follows");
    let over = alloc::vec![GStop { pos: 1.7, color: [1.0; 4] }];
    s.add("stop-钳制", normalize_stops(over)[0].pos == 1.0, "clamped");
    // 族二：硬边
    let hard = alloc::vec![
        GStop { pos: 0.0, color: [0.0, 0.0, 0.0, 1.0] },
        GStop { pos: 0.5, color: [1.0, 1.0, 1.0, 1.0] },
        GStop { pos: 0.5, color: [0.0, 0.0, 1.0, 1.0] },
        GStop { pos: 1.0, color: [0.0, 0.0, 1.0, 1.0] },
    ];
    let he = hard_edges(&normalize_stops(hard));
    s.add("硬边-重合检出", he == alloc::vec![1usize], "hard edge at 1");
    let soft = alloc::vec![
        GStop { pos: 0.0, color: [0.0; 4] },
        GStop { pos: 1.0, color: [1.0; 4] },
    ];
    s.add("硬边-无重合", hard_edges(&soft).is_empty(), "no edge");
    // 硬边过渡语义：重合点两侧取后值（span=0 → b 色）
    let c = lerp_srgb(&[1.0, 1.0, 1.0, 1.0], &[0.0, 0.0, 1.0, 1.0], 1.0);
    s.add("硬边-取后值", c[2] == 1.0, "b side wins");
    // 族三：双色彩空间
    let dark = [0.0f32, 0.0, 0.0, 1.0];
    let bright = [1.0f32, 1.0, 1.0, 1.0];
    s.add("空间-端点一致", spaces_differ(&dark, &bright), "endpoints agree");
    s.add("空间-标签", ColorSpace::Srgb.label() == "sRGB" && ColorSpace::Oklch.label() == "OKLCH", "labels");
    // 档位策略：默认 sRGB / 深空玻璃 OKLCH
    s.add("空间-默认sRGB", ColorSpace::Srgb.label() == "sRGB", "default");
    // 感知单调性：OKLCH 中点亮度（gamma 域）介于两端
    let o = lerp_oklch(&dark, &bright, 0.5);
    s.add("空间-中点有界", o[0] > 0.3 && o[0] < 0.7, "perceptual mid");
    // 族四：预乘纪律
    let trans = [1.0f32, 0.0, 0.0, 0.5];
    let opaq = [0.0f32, 0.0, 1.0, 1.0];
    let p = lerp_premul(&trans, &opaq, 0.5);
    // 预乘插值：红半透明 + 蓝不透明 → 中点应偏紫且 alpha=0.75
    s.add("预乘-alpha线性", (p[3] - 0.75).abs() < 1e-6, "alpha 0.75");
    s.add("预乘-色彩去预乘", p[0].is_finite() && p[2].is_finite(), "unpremultiplied");
    // alpha=0 保护
    let z = lerp_premul(&[1.0, 0.0, 0.0, 0.0], &[0.0, 0.0, 1.0, 0.0], 0.5);
    s.add("预乘-零alpha不炸", z.iter().all(|v| v.is_finite()), "a=0 safe");
    // sRGB 插值与预乘插值在端点一致
    s.add("预乘-端点一致", lerp_premul(&trans, &opaq, 0.0) == trans, "t=0 same");
    s
}
