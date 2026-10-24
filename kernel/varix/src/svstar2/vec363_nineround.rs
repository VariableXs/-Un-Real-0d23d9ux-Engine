//! VE-F0763 九宫格与圆角阴影
//! 职责：圆角九宫格（角区带半径约束）、阴影九宫格（外扩 padding + 模糊边距）、
//!       圆角-边距冲突校验、阴影纹理尺寸推导。
//! 契约：角半径 ≤ min(角区尺寸)；阴影 padding ≥ 模糊半径。

use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::vec361_ninecore::{Insets, Slice};

/// 圆角九宫格
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct RoundedSlice {
    pub slice: Slice,
    pub radius: f64,
}

impl RoundedSlice {
    /// 构造：半径不得超过最小角区边
    pub fn build(tex_w: f64, tex_h: f64, inset: Insets, radius: f64) -> Option<RoundedSlice> {
        let slice = Slice::build(tex_w, tex_h, inset)?;
        let min_corner = inset.left.min(inset.top).min(inset.right).min(inset.bottom);
        if radius < 0.0 || radius > min_corner {
            return None;
        }
        Some(RoundedSlice { slice, radius })
    }
    /// 直角切分处的内缩量（圆角内侧留白）
    pub fn corner_inset(&self) -> f64 {
        self.radius * (1.0 - core::f64::consts::FRAC_1_SQRT_2)
    }
}

/// 阴影九宫格
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct ShadowSlice {
    pub slice: Slice,
    /// 内容区到阴影纹理的留白（两侧对称外扩）
    pub padding: f64,
    /// 模糊半径
    pub blur: f64,
}

impl ShadowSlice {
    /// 构造：padding ≥ blur（模糊不越出纹理）
    pub fn build(blur: f64, padding: f64) -> Option<ShadowSlice> {
        if blur < 0.0 || padding < blur {
            return None;
        }
        // 阴影纹理 = 内容 1×1 基准 + 2×padding；角区 = padding
        let tex = 1.0 + 2.0 * padding;
        let slice = Slice::build(tex, tex, Insets::uniform(padding))?;
        Some(ShadowSlice { slice, padding, blur })
    }
    /// 阴影绘制区域（内容矩形外扩 padding）
    pub fn draw_rect(&self, cx: f64, cy: f64, w: f64, h: f64) -> (f64, f64, f64, f64) {
        (cx - w * 0.5 - self.padding, cy - h * 0.5 - self.padding, w + 2.0 * self.padding, h + 2.0 * self.padding)
    }
    /// 有效模糊衰减（padding/blur 比越大边缘越干净）
    pub fn edge_quality(&self) -> f64 {
        if self.blur == 0.0 {
            return f64::INFINITY;
        }
        self.padding / self.blur
    }
}

pub fn run_vec363_checks() -> crate::checks::CheckSet {
    let mut s = CheckSet::new("svstar2-vec363");
    // 族一：圆角约束
    let rs = RoundedSlice::build(64.0, 64.0, Insets::uniform(12.0), 8.0).expect("rs");
    s.add("圆角-合法", rs.radius == 8.0, "r=8 ≤ 12");
    s.add("圆角-等于角区", RoundedSlice::build(64.0, 64.0, Insets::uniform(12.0), 12.0).is_some(), "r == inset ok");
    s.add("圆角-超界拒", RoundedSlice::build(64.0, 64.0, Insets::uniform(12.0), 13.0).is_none(), "r > inset rejected");
    s.add("圆角-负半径拒", RoundedSlice::build(64.0, 64.0, Insets::uniform(12.0), -1.0).is_none(), "negative");
    s.add("圆角-底纹拒", RoundedSlice::build(16.0, 64.0, Insets::uniform(12.0), 4.0).is_none(), "inset ≥ tex");
    // 内切量 r(1-√2/2)
    let ci = rs.corner_inset();
    s.add("圆角-内切量", (ci - 8.0 * (1.0 - 0.7071067811865476)).abs() < 1e-9, "r(1-√2/2)");
    s.add("圆角-内切小于半径", ci < rs.radius, "inset < r");
    // 零半径 = 直角
    let sharp = RoundedSlice::build(64.0, 64.0, Insets::uniform(8.0), 0.0).expect("sharp");
    s.add("直角-内切零", sharp.corner_inset() == 0.0, "no inset");
    // 族二：阴影构造
    let sh = ShadowSlice::build(6.0, 10.0).expect("shadow");
    s.add("阴影-模糊6", sh.blur == 6.0, "blur");
    s.add("阴影-留白10", sh.padding == 10.0, "padding");
    s.add("阴影-纹理尺寸", sh.slice.tex_w == 21.0, "1 + 2×10");
    s.add("阴影-角区", sh.slice.inset.left == 10.0, "corner = padding");
    // 构造拒绝
    s.add("阴影-留白不足拒", ShadowSlice::build(6.0, 5.0).is_none(), "padding < blur");
    s.add("阴影-恰好", ShadowSlice::build(6.0, 6.0).is_some(), "padding == blur ok");
    s.add("阴影-负模糊拒", ShadowSlice::build(-1.0, 5.0).is_none(), "negative blur");
    // 零模糊
    let none = ShadowSlice::build(0.0, 2.0).expect("none");
    s.add("阴影-零模糊质量", none.edge_quality().is_infinite(), "inf quality");
    // 族三：边缘质量
    s.add("质量-比值", (sh.edge_quality() - 10.0 / 6.0).abs() < 1e-12, "padding/blur");
    let soft = ShadowSlice::build(2.0, 20.0).expect("soft");
    s.add("质量-更大更净", soft.edge_quality() > sh.edge_quality(), "bigger ratio");
    // 族四：绘制区域
    let (dx, dy, dw, dh) = sh.draw_rect(50.0, 50.0, 20.0, 10.0);
    s.add("区域-x", (dx - (50.0 - 10.0 - 10.0)).abs() < 1e-12, "x = cx - w/2 - pad");
    s.add("区域-宽", (dw - 40.0).abs() < 1e-12, "w + 2pad");
    s.add("区域-高", (dh - 30.0).abs() < 1e-12, "h + 2pad");
    s.add("区域-y", (dy - (50.0 - 5.0 - 10.0)).abs() < 1e-12, "y");
    // 阴影区域 ⊇ 内容区域
    s.add("区域-包含内容", dx <= 50.0 - 10.0 && dx + dw >= 50.0 + 10.0, "covers content");
    // 族五：圆角 + 阴影联合（圆角半径不越出阴影角区）
    s.add("联合-圆角合法", RoundedSlice::build(sh.slice.tex_w, sh.slice.tex_h, sh.slice.inset, 10.0).is_some(), "r == padding ok");
    s.add("联合-圆角超界", RoundedSlice::build(sh.slice.tex_w, sh.slice.tex_h, sh.slice.inset, 11.0).is_none(), "r > padding rejected");
    s
}
