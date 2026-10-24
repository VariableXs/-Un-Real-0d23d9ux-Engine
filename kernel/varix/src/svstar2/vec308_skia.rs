//! VE-F0708 · 栅格化质量基准（对拍 Skia 级）
//!
//! 职责：栅格化质量的业界对拍——标准测试路径集（圆/星形/细线/锐角/自交/
//! 文字轮廓——每类挑战一种栅格化弱点）在 VE 与 Skia（参考实现）渲染对拍——
//! 像素差 diff 图与质量评估（AA 平滑度/边缘干净度/形状保真）。
//! 质量分数量化（差异像素数/边缘锯齿能量）。对拍发现的差距逐项修复排期。

use alloc::vec;
use alloc::vec::Vec;

/// 标准测试路径集：六类挑战型几何。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TestShape {
    Circle,     // 圆：全角度平滑
    Star,       // 星形：锐角
    ThinLine,   // 细线：亚像素覆盖
    SharpAngle, // 锐角：尖角锯齿
    SelfCross,  // 自交：填充规则
    Glyph,      // 文字轮廓：曲线密度
}

impl TestShape {
    pub fn all() -> Vec<TestShape> {
        vec![
            TestShape::Circle, TestShape::Star, TestShape::ThinLine,
            TestShape::SharpAngle, TestShape::SelfCross, TestShape::Glyph,
        ]
    }
    pub fn name(self) -> &'static str {
        match self {
            TestShape::Circle => "圆",
            TestShape::Star => "星形",
            TestShape::ThinLine => "细线",
            TestShape::SharpAngle => "锐角",
            TestShape::SelfCross => "自交",
            TestShape::Glyph => "文字轮廓",
        }
    }
}

/// 像素差 diff：两张同尺寸覆盖图的逐像素差（千分位），返回 diff 图与差异像素数。
pub fn pixel_diff(a: &[u32], b: &[u32]) -> (Vec<u32>, u32) {
    let diff: Vec<u32> = a
        .iter()
        .zip(b.iter())
        .map(|(x, y)| (*x as i32 - *y as i32).unsigned_abs())
        .collect();
    let count = diff.iter().filter(|&&d| d > 0).count() as u32;
    (diff, count)
}

/// 质量分数（千分位，越高越好）：1000 - 差异像素占比 - 锯齿能量惩罚。
pub fn quality_score(diff_count: u32, total_px: u32, jag_energy: u32) -> u32 {
    let diff_penalty = diff_count * 1000 / total_px.max(1);
    let jag_penalty = jag_energy * 100 / 1000; // 锯齿能量（千分位梯度）按 1/10 权
    1000u32.saturating_sub(diff_penalty).saturating_sub(jag_penalty)
}

/// 边缘锯齿能量：相邻覆盖度梯度的均方（AA 越平滑梯度越缓）。
pub fn jag_energy(covers: &[u32], w: u32) -> u32 {
    if covers.len() < 2 || w < 2 {
        return 0;
    }
    let h = covers.len() as u32 / w;
    let mut sum = 0u64;
    let mut n = 0u64;
    for y in 0..h {
        for x in 0..w - 1 {
            let i = (y * w + x) as usize;
            let d = (covers[i] as i64 - covers[i + 1] as i64).unsigned_abs() as u64;
            sum += d * d;
            n += 1;
        }
    }
    if n == 0 {
        0
    } else {
        ((sum / n) as f64).sqrt() as u32
    }
}

/// 对拍结论：逐形状登记 VE 分数与参考分、差距、修复排期。
#[derive(Clone, Debug, PartialEq)]
pub struct CompareRow {
    pub shape: TestShape,
    pub ve_score: u32,
    pub ref_score: u32,
    /// 差距 = ref - ve（负数即 VE 反超）。
    pub gap: i32,
    /// 修复排期：差距超阈（50）入排期。
    pub scheduled: bool,
}

/// 对拍流程：六形状逐一对拍出结论表——差距超阈值入修复排期。
pub fn compare_run(ve_scores: &[u32], ref_scores: &[u32], gap_threshold: i32) -> Vec<CompareRow> {
    let shapes = TestShape::all();
    shapes
        .iter()
        .zip(ve_scores.iter().zip(ref_scores.iter()))
        .map(|(sh, (v, r))| {
            let gap = *r as i32 - *v as i32;
            CompareRow {
                shape: *sh,
                ve_score: *v,
                ref_score: *r,
                gap,
                scheduled: gap > gap_threshold,
            }
        })
        .collect()
}

/// 差距修复排期统计：待排期数与最大差距。
pub fn repair_backlog(rows: &[CompareRow]) -> (u32, i32) {
    let n = rows.iter().filter(|r| r.scheduled).count() as u32;
    let max = rows.iter().map(|r| r.gap).max().unwrap_or(0);
    (n, max)
}

pub fn run_vec308_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("svstar2-vec308");
    // 族一：测试路径集
    let shapes = TestShape::all();
    s.add("六类挑战齐备", shapes.len() == 6, "six");
    s.add("名称互异", {
        let mut names: Vec<&str> = shapes.iter().map(|x| x.name()).collect();
        names.sort_unstable();
        names.dedup();
        names.len() == 6
    }, "names");
    // 族二：像素 diff
    let a = vec![1000u32, 500, 0, 1000];
    let b = vec![1000u32, 250, 0, 500];
    let (d, n) = pixel_diff(&a, &b);
    s.add("diff 图逐像素差", d == vec![0u32, 250, 0, 500], "diff-map");
    s.add("差异像素计数", n == 2, "diff-count");
    let (d0, n0) = pixel_diff(&a, &a);
    s.add("同图零差", d0.iter().all(|&x| x == 0) && n0 == 0, "identical");
    // 族三：质量分数与锯齿能量
    s.add("满分无差", quality_score(0, 1000, 0) == 1000, "perfect");
    s.add("差异扣分", quality_score(100, 1000, 0) == 900, "diff-pen");
    s.add("锯齿扣分", quality_score(0, 1000, 200) == 980, "jag-pen");
    s.add("不低于零", quality_score(2000, 1000, 5000) == 0, "floor");
    // 锯齿能量：缓变 vs 突变
    let smooth = vec![0u32, 200, 400, 600, 800, 1000];
    let harsh = vec![0u32, 1000, 0, 1000, 0, 1000];
    let e_smooth = jag_energy(&smooth, 6);
    let e_harsh = jag_energy(&harsh, 6);
    s.add("锯齿能量平滑低", e_smooth < 250, "e-smooth");
    s.add("锯齿能量突变高", e_harsh > 500, "e-harsh");
    s.add("平滑先于突变", e_smooth < e_harsh, "order");
    let flat = vec![500u32; 36];
    s.add("常数场零能量", jag_energy(&flat, 6) == 0, "flat");
    // 族四：对拍流程与修复排期
    let rows = compare_run(&[920, 880, 950, 900, 870, 930], &[950, 950, 950, 950, 950, 950], 50);
    s.add("六形状逐一对拍", rows.len() == 6, "rows");
    s.add("差距计算正确", rows[0].gap == 30 && rows[1].gap == 70, "gap");
    s.add("差距超阈入排期", rows.iter().filter(|r| r.scheduled).count() == 2, "sched");
    s.add("反超不排期", {
        let better = compare_run(&[960, 950, 950, 950, 950, 950], &[950; 6], 50);
        !better[0].scheduled
    }, "no-sched");
    let (backlog, max_gap) = repair_backlog(&rows);
    s.add("待排期统计", backlog == 2, "backlog");
    s.add("最大差距正确", max_gap == 70, "max-gap");
    s.add("量化评分闭环", rows.iter().all(|r| r.gap == r.ref_score as i32 - r.ve_score as i32), "quantified");
    // 族五：版本对比
    let v1 = compare_run(&[900; 6], &[950; 6], 50);
    let v2 = compare_run(&[940; 6], &[950; 6], 50);
    let (_, max1) = repair_backlog(&v1);
    let (_, max2) = repair_backlog(&v2);
    s.add("版本迭代差距收窄", max2 < max1, "version");
    s
}
