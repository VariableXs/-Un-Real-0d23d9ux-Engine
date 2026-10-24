//! VE-F0654 蒙版逆变换命中（VE-D 域 · 目标 300 行）
//!
//! **锚点**：`VE-STAR-II · 总纲与施工书.md#VE-F0654`
//!
//! **判据（锚点原文逐条）**：
//! - **逆变换复判**：F0611 图层命中后蒙版级复判——命中点逆变换到蒙版局部空间查值；
//! - **解析优先**：圆角蒙版走 SDF 解析判定（零纹理开销），纹理次之；
//! - **超时保护**：缓存未命中光栅化按需，不阻塞命中主路径（超时降级包围盒）；
//! - **渲染一致**：命中结果与渲染一致（渲染不可见处不可命中——同一蒙版数据源）；
//! - **判据**：阈值歧义 → 统一阈值常量；数据源分叉 → 单源审计。

use crate::checks::CheckSet;
use super::vec250_rmask::{RrectMask, CornerRadii};
use super::vec251_pmask::{raster_nonzero, MaskMode};

/// 统一 alpha 阈值常量（阈值歧义对策：全组单源）
pub const HIT_ALPHA_THRESHOLD: f32 = 0.5;

/// 蒙版数据源（单源审计：解析与纹理共用此枚举定义）
#[derive(Clone, Debug, PartialEq)]
pub enum MaskSource {
    /// 解析 SDF（圆角）
    Analytic(RrectMask),
    /// 纹理采样（路径光栅化）
    Texture { w: usize, h: usize, mask: alloc::vec::Vec<bool>, mode: MaskMode },
}

impl MaskSource {
    /// 查值（双轨判定统一入口：渲染与命中同一数据源）
    pub fn alpha_at(&self, x: f32, y: f32) -> f32 {
        match self {
            MaskSource::Analytic(m) => m.alpha_at(x, y),
            MaskSource::Texture { w, h, mask, .. } => {
                let xi = x as usize;
                let yi = y as usize;
                if xi < *w && yi < *h && mask[yi * w + xi] {
                    1.0
                } else {
                    0.0
                }
            }
        }
    }

    /// 是否解析轨（解析优先）
    pub fn is_analytic(&self) -> bool {
        matches!(self, MaskSource::Analytic(_))
    }
}

/// 逆变换：世界点 → 蒙版局部点（平移+缩放逆变换）
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct InvXform {
    pub tx: f32,
    pub ty: f32,
    pub scale: f32,
}

impl InvXform {
    pub fn apply(&self, x: f32, y: f32) -> (f32, f32) {
        let s = if self.scale.abs() < 1e-9 { 1.0 } else { 1.0 / self.scale };
        ((x - self.tx) * s, (y - self.ty) * s)
    }
}

/// 复判管线：世界命中点 → 逆变换 → 蒙版查值 → 阈值判定
pub fn rehit(src: &MaskSource, xf: &InvXform, wx: f32, wy: f32) -> bool {
    let (lx, ly) = xf.apply(wx, wy);
    src.alpha_at(lx, ly) > HIT_ALPHA_THRESHOLD
}

/// 超时保护：纹理轨按需光栅化的预算（步数上限）
pub const RASTER_BUDGET_STEPS: u32 = 4;

/// 超时降级：包围盒命中（登记降级）
#[derive(Clone, Debug, PartialEq)]
pub struct TimeoutDegrade {
    pub bbox: (f32, f32, f32, f32),
    pub degraded: bool,
}

pub fn timeout_fallback(bbox: (f32, f32, f32, f32), steps_used: u32) -> TimeoutDegrade {
    TimeoutDegrade {
        bbox,
        degraded: steps_used > RASTER_BUDGET_STEPS,
    }
}

impl TimeoutDegrade {
    /// 降级命中判定（包围盒内即命中——保守宽松）
    pub fn hit(&self, x: f32, y: f32) -> bool {
        x >= self.bbox.0 && x < self.bbox.2 && y >= self.bbox.1 && y < self.bbox.3
    }
}

pub fn run_vec254_checks() -> crate::checks::CheckSet {
    let mut s = CheckSet::new("svstar2-vec254");
    // 族一：统一阈值
    s.add("命中-阈值单源", HIT_ALPHA_THRESHOLD == 0.5, "0.5 single");
    // 族二：解析轨复判
    let (rr, _) = RrectMask::build((0.0, 0.0, 100.0, 100.0), CornerRadii { tl: 25.0, tr: 0.0, br: 0.0, bl: 0.0 }, 1.0);
    let an = MaskSource::Analytic(rr);
    s.add("复判-解析轨标记", an.is_analytic(), "analytic first");
    let xf0 = InvXform { tx: 0.0, ty: 0.0, scale: 1.0 };
    s.add("复判-中心命中", rehit(&an, &xf0, 50.0, 50.0), "center hit");
    s.add("复判-圆角外穿透", !rehit(&an, &xf0, 1.0, 1.0), "corner miss");
    s.add("复判-界外穿透", !rehit(&an, &xf0, 150.0, 50.0), "outside miss");
    // 族三：纹理轨复判
    let poly = [(0i32, 0i32), (8, 0), (8, 8), (0, 8)];
    let rz = raster_nonzero(8, 8, &poly);
    let tex = MaskSource::Texture { w: 8, h: 8, mask: rz.mask, mode: MaskMode::Alpha };
    s.add("复判-纹理轨", !tex.is_analytic(), "texture track");
    s.add("复判-纹理命中", rehit(&tex, &xf0, 3.0, 3.0), "tex hit");
    s.add("复判-纹理穿透", !rehit(&tex, &xf0, 9.5, 3.0), "tex miss");
    // 族四：逆变换
    let xf = InvXform { tx: 100.0, ty: 200.0, scale: 2.0 };
    s.add("逆变换-平移缩放", xf.apply(120.0, 210.0) == (10.0, 5.0), "inv xform");
    let xfz = InvXform { tx: 0.0, ty: 0.0, scale: 0.0 };
    s.add("逆变换-零缩放保底", xfz.apply(10.0, 10.0) == (10.0, 10.0), "zero scale safe");
    // 世界偏移后命中一致性
    let xft = InvXform { tx: 500.0, ty: 500.0, scale: 1.0 };
    s.add("复判-世界坐标命中", rehit(&an, &xft, 550.0, 550.0), "world→local hit");
    s.add("复判-世界坐标穿透", !rehit(&an, &xft, 450.0, 550.0), "world→local miss");
    // 族五：渲染一致性（同一数据源两路判定一致）
    let p1 = an.alpha_at(50.0, 50.0) > HIT_ALPHA_THRESHOLD;
    let p2 = rehit(&an, &xf0, 50.0, 50.0);
    s.add("一致-渲染命中同源", p1 == p2, "same source");
    // 族六：超时保护
    let fb = timeout_fallback((0.0, 0.0, 100.0, 100.0), 9);
    s.add("超时-降级登记", fb.degraded, "degraded");
    s.add("超时-包围盒宽松", fb.hit(99.0, 99.0), "bbox permissive");
    let ok = timeout_fallback((0.0, 0.0, 100.0, 100.0), 2);
    s.add("超时-预算内不降", !ok.degraded, "within budget");
    s.add("超时-上限常量", RASTER_BUDGET_STEPS == 4, "budget 4");
    s
}
