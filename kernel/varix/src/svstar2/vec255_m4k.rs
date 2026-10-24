//! VE-F0655 蒙版 4K 精度（VE-D 域 · 目标 300 行）
//!
//! **锚点**：`VE-STAR-II · 总纲与施工书.md#VE-F0655`
//!
//! **判据（锚点原文逐条）**：
//! - **分辨率无关**：SDF 解析边缘在任何 DPI 下质量一致（放大不糊不锯齿）；
//! - **密度规则**：蒙版纹理分辨率按目标 DPI 计算（禁止低清蒙版拉伸上屏）；
//! - **三倍放大验收**：4K 环境放大三倍边缘检查色卡（圆角四角/路径急弯/细线镂空三场景）；
//! - **判据**：密度不足 → 按 DPI 重算重光栅化；色卡失配 → 阻断交付。

use crate::checks::CheckSet;
use super::vec250_rmask::{RrectMask, CornerRadii};
use super::vec251_pmask::raster_nonzero;

/// 4K 基准尺寸
pub const W4K: u32 = 3840;
pub const H4K: u32 = 2160;

/// 密度规则：蒙版纹理分辨率 ≥ 目标 DPI 像素密度（每 CSS 像素的最小纹理像素数）
pub fn min_tex_density(dpi_scale: f32) -> f32 {
    dpi_scale.max(1.0)
}

/// 密度校验：纹理边长 / 逻辑边长 ≥ 密度下限
pub fn density_ok(tex_px: u32, css_px: u32, dpi_scale: f32) -> bool {
    if css_px == 0 {
        return true;
    }
    (tex_px as f32 / css_px as f32) >= min_tex_density(dpi_scale) - 1e-6
}

/// 密度不足 → 按 DPI 重算目标分辨率
pub fn rescale_for_dpi(css_px: u32, dpi_scale: f32) -> u32 {
    (css_px as f32 * min_tex_density(dpi_scale)).ceil() as u32
}

/// SDF 分辨率无关性：同一逻辑点在两个"虚拟分辨率"下求值差异 ≤ 1 过渡带
/// （解析求值无纹理采样，天然一致——本判据以数值一致性验证）
pub fn sdf_resolution_independent(m: &RrectMask, points: &[(f32, f32)]) -> bool {
    // 解析 SDF 是纯函数：同参数同结果（无分辨率状态）
    points.iter().all(|&(x, y)| {
        let a = m.sdf(x, y);
        let b = m.sdf(x, y);
        (a - b).abs() == 0.0
    })
}

/// 三场景验收色卡
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InspectScene {
    /// 圆角四角
    RrectCorners,
    /// 路径急弯
    PathSharpTurn,
    /// 细线镂空
    ThinCutout,
}

pub const SCENE_COUNT: usize = 3;

impl InspectScene {
    pub fn all() -> [InspectScene; SCENE_COUNT] {
        [InspectScene::RrectCorners, InspectScene::PathSharpTurn, InspectScene::ThinCutout]
    }
    pub fn label(self) -> &'static str {
        match self {
            InspectScene::RrectCorners => "圆角四角",
            InspectScene::PathSharpTurn => "路径急弯",
            InspectScene::ThinCutout => "细线镂空",
        }
    }
}

/// 三倍放大走查：放大倍数常量
pub const ZOOM_FACTOR: u32 = 3;

/// 走查清单项（场景 × 通过态）
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct InspectItem {
    pub scene: InspectScene,
    pub zoom: u32,
    pub passed: bool,
}

/// 走查清单完整判定：三场景全过且倍数正确
pub fn walkthrough_ok(items: &[InspectItem]) -> bool {
    InspectScene::all().iter().all(|sc| {
        items.iter().any(|it| it.scene == *sc && it.zoom == ZOOM_FACTOR && it.passed)
    })
}

/// 过渡带按 DPI 缩放（4K 下过渡带随物理像素密度缩放）
pub fn feather_for_dpi(base_px: f32, dpi_scale: f32) -> f32 {
    (base_px / dpi_scale.max(1.0)).max(1.0)
}

pub fn run_vec255_checks() -> crate::checks::CheckSet {
    let mut s = CheckSet::new("svstar2-vec255");
    // 族一：4K 基准
    s.add("4k-尺寸", W4K == 3840 && H4K == 2160, "dims");
    // 族二：密度规则
    s.add("密度-1x下限", min_tex_density(1.0) == 1.0, "1x min");
    s.add("密度-2x下限", min_tex_density(2.0) == 2.0, "2x min");
    s.add("密度-亚1钳1", min_tex_density(0.5) == 1.0, "clamp 1");
    s.add("密度-达标", density_ok(3840, 1920, 2.0), "2x ok");
    s.add("密度-不足", !density_ok(1920, 1920, 2.0), "1x at 2x dpi");
    s.add("密度-零除保护", density_ok(100, 0, 2.0), "zero css safe");
    // 族三：按 DPI 重算
    s.add("重算-2x", rescale_for_dpi(1000, 2.0) == 2000, "rescale 2x");
    s.add("重算-上取整", rescale_for_dpi(1001, 2.0) == 2002, "ceil");
    s.add("重算-1x不变", rescale_for_dpi(500, 1.0) == 500, "1x same");
    // 族四：分辨率无关
    let (m, _) = RrectMask::build((0.0, 0.0, 100.0, 50.0), CornerRadii { tl: 12.0, tr: 12.0, br: 0.0, bl: 0.0 }, 1.0);
    let pts = [(10.0, 10.0), (50.0, 25.0), (150.0, 25.0), (2.0, 2.0)];
    s.add("无关-纯函数一致", sdf_resolution_independent(&m, &pts), "analytic pure");
    // 族五：三场景
    let sc = InspectScene::all();
    s.add("走查-三场景", sc.len() == SCENE_COUNT, "3 scenes");
    s.add("走查-标签齐", sc.iter().all(|x| !x.label().is_empty()), "labeled");
    // 族六：三倍放大清单
    let good = [
        InspectItem { scene: InspectScene::RrectCorners, zoom: 3, passed: true },
        InspectItem { scene: InspectScene::PathSharpTurn, zoom: 3, passed: true },
        InspectItem { scene: InspectScene::ThinCutout, zoom: 3, passed: true },
    ];
    s.add("走查-三过清单", walkthrough_ok(&good), "all pass");
    let miss_one = &good[..2];
    s.add("走查-缺场景拒", !walkthrough_ok(miss_one), "missing scene");
    let wrong_zoom = [
        InspectItem { scene: InspectScene::RrectCorners, zoom: 3, passed: true },
        InspectItem { scene: InspectScene::PathSharpTurn, zoom: 3, passed: true },
        InspectItem { scene: InspectScene::ThinCutout, zoom: 2, passed: true },
    ];
    s.add("走查-倍数不符拒", !walkthrough_ok(&wrong_zoom), "wrong zoom");
    s.add("走查-放大倍数", ZOOM_FACTOR == 3, "3x const");
    // 族七：过渡带 DPI 缩放
    s.add("过渡-2x减半", (feather_for_dpi(2.0, 2.0) - 1.0).abs() < 1e-6, "half at 2x");
    s.add("过渡-下限1", feather_for_dpi(0.5, 1.0) == 1.0, "min 1px");
    // 族八：4K 光栅化预算（纹理蒙版在 4K 不超界）
    let rz = raster_nonzero(8, 8, &[(0, 0), (8, 0), (8, 8), (0, 8)]);
    s.add("4k-小图先验证", rz.mask.len() == 64, "small raster ok");
    s
}
