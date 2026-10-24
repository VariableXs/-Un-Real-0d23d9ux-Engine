//! VE-F0711 · 嵌套裁剪与路径栅格化
//!
//! 职责：路径内容 + 嵌套裁剪的栅格化组合——
//! 路径在多层裁剪内的正确栅格化（裁剪链预计算——裁剪区域与路径 bbox 相交预筛、
//! 逐瓦片裁剪掩码应用）、
//! 裁剪与 AA 的交互（裁剪边的 AA 与路径边的 AA 一致性——两层 AA 叠加的边缘质量）。

use alloc::vec;
use alloc::vec::Vec;

/// 裁剪区域：矩形（简化模型——多层矩形裁剪链）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ClipRect {
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
}

impl ClipRect {
    pub fn intersects(&self, o: &ClipRect) -> bool {
        self.x0 < o.x1 && o.x0 < self.x1 && self.y0 < o.y1 && o.y0 < self.y1
    }
    /// 与另一裁剪求交（嵌套裁剪的语义合成）。
    pub fn intersect(&self, o: &ClipRect) -> ClipRect {
        ClipRect {
            x0: self.x0.max(o.x0),
            y0: self.y0.max(o.y0),
            x1: self.x1.min(o.x1),
            y1: self.y1.min(o.y1),
        }
    }
    pub fn valid(&self) -> bool {
        self.x0 < self.x1 && self.y0 < self.y1
    }
}

/// 裁剪链预计算：多层裁剪折叠为单一有效裁剪（嵌套语义 = 逐层求交）。
/// 预筛：折叠结果与路径 bbox 不相交 → 整条路径可跳过（零工作）。
pub fn clip_chain_fold(chain: &[ClipRect], path_bbox: &ClipRect) -> Result<ClipRect, &'static str> {
    let mut acc = ClipRect { x0: f32::NEG_INFINITY, y0: f32::NEG_INFINITY, x1: f32::INFINITY, y1: f32::INFINITY };
    for c in chain {
        if !c.valid() {
            return Err("裁剪矩形退化（空区域）");
        }
        acc = acc.intersect(c);
    }
    if !acc.valid() || !acc.intersects(path_bbox) {
        return Err("路径与裁剪链不相交：整条路径预筛剔除");
    }
    Ok(acc)
}

/// 裁剪掩码应用：逐像素判定是否在有效裁剪内（逐瓦片掩码的像素级模型）。
pub fn clip_mask(folded: &ClipRect, x: u32, y: u32) -> bool {
    let px = x as f32;
    let py = y as f32;
    px >= folded.x0 && px < folded.x1 && py >= folded.y0 && py < folded.y1
}

/// 两层 AA 叠加的边缘质量：路径覆盖 alpha × 裁剪边缘 alpha（预乘合成）。
/// 一致性要求：裁剪边 AA 与路径边 AA 用同一覆盖度口径（千分位）。
pub fn aa_compose(path_alpha: u32, clip_alpha: u32) -> u32 {
    (path_alpha as u64 * clip_alpha as u64 / 1000) as u32
}

/// 裁剪 AA 一致性核验：裁剪边缘的覆盖度渐变带与路径边缘同宽（都是半像素带）。
pub fn aa_consistent(path_edge_cov: u32, clip_edge_cov: u32) -> bool {
    // 两条边缘在正交穿越时的半覆盖都应是 500（同一口径）。
    (path_edge_cov == 500) == (clip_edge_cov == 500)
}

/// 路径 bbox 预筛收益模型：与裁剪不相交的路径直接跳过。
pub fn cull_count(paths: &[ClipRect], folded: &ClipRect) -> u32 {
    paths.iter().filter(|p| !folded.intersects(p)).count() as u32
}

pub fn run_vec311_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("svstar2-vec311");
    // 族一：裁剪矩形语义
    let a = ClipRect { x0: 0.0, y0: 0.0, x1: 10.0, y1: 10.0 };
    let b = ClipRect { x0: 5.0, y0: 5.0, x1: 20.0, y1: 20.0 };
    let c = ClipRect { x0: 20.0, y0: 0.0, x1: 30.0, y1: 10.0 };
    s.add("相交判定", a.intersects(&b) && !a.intersects(&c), "intersects");
    let ab = a.intersect(&b);
    s.add("求交区域正确", ab == ClipRect { x0: 5.0, y0: 5.0, x1: 10.0, y1: 10.0 }, "intersect");
    s.add("求交保持有效", ab.valid(), "still-valid");
    let ac = a.intersect(&c);
    s.add("不相交求积退化", !ac.valid(), "degenerate");
    // 族二：裁剪链折叠与预筛
    let chain = vec![a, b];
    let path_in = ClipRect { x0: 6.0, y0: 6.0, x1: 8.0, y1: 8.0 };
    let path_out = ClipRect { x0: 15.0, y0: 0.0, x1: 18.0, y1: 5.0 };
    let folded = clip_chain_fold(&chain, &path_in);
    s.add("折叠结果=逐层交", folded == Ok(ClipRect { x0: 5.0, y0: 5.0, x1: 10.0, y1: 10.0 }), "fold");
    s.add("相交路径通过预筛", folded.is_ok(), "keep");
    s.add("不相交路径预筛剔除", clip_chain_fold(&chain, &path_out).is_err(), "cull");
    let degen = vec![ClipRect { x0: 5.0, y0: 5.0, x1: 5.0, y1: 9.0 }];
    s.add("退化裁剪拒绝", clip_chain_fold(&degen, &path_in).is_err(), "degen-chain");
    // 族三：掩码应用
    let f = folded.unwrap();
    s.add("掩码内真", clip_mask(&f, 6, 6), "in-mask");
    s.add("掩码外假", !clip_mask(&f, 3, 3) && !clip_mask(&f, 12, 12), "out-mask");
    s.add("边界含下不含上", clip_mask(&f, 5, 5) && !clip_mask(&f, 10, 10), "half-open");
    // 族四：AA 一致性
    s.add("两层 AA 预乘合成", aa_compose(500, 500) == 250, "compose");
    s.add("全覆盖透传", aa_compose(1000, 800) == 800, "full-pass");
    s.add("零覆盖归零", aa_compose(0, 800) == 0, "zero");
    s.add("同口径半覆盖一致", aa_consistent(500, 500), "consistent");
    s.add("异口径判不一致", !aa_consistent(500, 700), "inconsistent");
    // 族五：预筛收益
    let paths = vec![path_in, path_out, ClipRect { x0: 30.0, y0: 30.0, x1: 40.0, y1: 40.0 }];
    s.add("预筛剔除计数", cull_count(&paths, &f) == 2, "cull-count");
    s.add("嵌套正确性：折叠语义==逐层掩码", {
        // 逐层掩码与折叠掩码等价：点 6,6 在 a 且在 b 且在折叠内。
        let in_a = clip_mask(&a, 6, 6);
        let in_b = clip_mask(&b, 6, 6);
        in_a && in_b && clip_mask(&f, 6, 6)
    }, "nested-eq");
    s
}
