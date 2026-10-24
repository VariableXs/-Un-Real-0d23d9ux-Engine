//! VE-F0715 · 栅格化 CPU/GPU 一致性
//!
//! 职责：CPU 扫描线与 GPU（stencil-and-cover/分析式）三路径的一致性维护
//!（F0636 机制复用）：同路径三路渲染对拍（AA 实现差异的合法容差内一致）、
//! 语义变更三路同改、每夜对拍、差异仲裁。
//! 一致性 = 跨 GPU/软渲环境路径观感统一（十四章）的栅格面。

use alloc::format;
use alloc::vec;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 三条渲染路径：CPU 扫描线、GPU stencil-and-cover、GPU 分析式。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RenderPath {
    CpuScanline,
    GpuStencilCover,
    GpuAnalytic,
}

impl RenderPath {
    pub fn all() -> Vec<RenderPath> {
        vec![RenderPath::CpuScanline, RenderPath::GpuStencilCover, RenderPath::GpuAnalytic]
    }
    pub fn name(self) -> &'static str {
        match self {
            RenderPath::CpuScanline => "CPU扫描线",
            RenderPath::GpuStencilCover => "GPU-SC",
            RenderPath::GpuAnalytic => "GPU-解析",
        }
    }
}

/// 覆盖图对拍：两路径同几何渲染结果的逐像素差（千分位覆盖度）。
/// 返回最大差与超容差像素数。
pub fn coverage_compare(a: &[u32], b: &[u32], tolerance: u32) -> (u32, u32) {
    let max_diff = a
        .iter()
        .zip(b.iter())
        .map(|(x, y)| (*x as i32 - *y as i32).unsigned_abs())
        .max()
        .unwrap_or(0);
    let over = a
        .iter()
        .zip(b.iter())
        .filter(|(x, y)| (**x as i32 - **y as i32).unsigned_abs() > tolerance)
        .count() as u32;
    (max_diff, over)
}

/// 合法容差规范：AA 实现差异的容忍上限（千分位）——不同 AA 实现的
/// 采样策略差异在半像素渐变带内产生 ±80 的覆盖度抖动属合法。
pub const AA_TOLERANCE: u32 = 80;

/// 三路对拍：任两路组合都过容差 → 一致。
pub fn triple_compare(
    cpu: &[u32],
    sc: &[u32],
    ana: &[u32],
    tolerance: u32,
) -> Result<(), String> {
    let pairs = [("CPU-SC", cpu, sc), ("CPU-ANA", cpu, ana), ("SC-ANA", sc, ana)];
    let mut bad: Vec<String> = Vec::new();
    for (name, a, b) in pairs {
        let (max_d, over) = coverage_compare(a, b, tolerance);
        if over > 0 {
            bad.push(format!("{} 超容差像素 {}（最大差 {}）", name, over, max_d));
        }
    }
    if bad.is_empty() {
        Ok(())
    } else {
        Err(bad.join("；"))
    }
}

/// 差异仲裁：超容差差异的归因分类——实现 bug / 合法实现差异 / 参考基准错。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiffArbitration {
    ImplementationBug,
    LegalImplDiff,
    ReferenceWrong,
}

/// 仲裁规则：差异集中在渐变带边缘（半覆盖附近）→ 合法实现差异；
/// 差异在内区（全覆盖/零覆盖）→ 实现 bug；
/// 三路中两路一致一路孤立 → 孤立者可疑（先查孤立者实现）。
pub fn arbitrate(max_diff: u32, at_half_cover: bool, isolated_path: bool) -> DiffArbitration {
    if max_diff <= AA_TOLERANCE {
        return DiffArbitration::LegalImplDiff;
    }
    if at_half_cover {
        DiffArbitration::LegalImplDiff
    } else if isolated_path {
        DiffArbitration::ImplementationBug
    } else {
        DiffArbitration::ReferenceWrong
    }
}

/// 每夜对拍记录：日期 + 三路 + 结论。
#[derive(Clone, Debug, PartialEq)]
pub struct NightlyRecord {
    pub case_count: u32,
    pub pass_count: u32,
    /// 语义变更后三路是否同改（变更纪律）。
    pub synced_change: bool,
}

impl NightlyRecord {
    pub fn verdict(&self) -> Result<(), String> {
        if !self.synced_change {
            return Err("语义变更未三路同改：变更纪律违规".to_string());
        }
        if self.pass_count != self.case_count {
            return Err(format!("每夜对拍 {}/{} 未过：差异仲裁跟进", self.pass_count, self.case_count));
        }
        Ok(())
    }
}

/// 观感统一承诺：三路一致是跨 GPU/软渲环境路径观感统一的栅格面支撑。
pub fn visual_uniformity_supported(triple_ok: bool) -> bool {
    triple_ok
}

pub fn run_vec315_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("svstar2-vec315");
    // 族一：三路标识
    let paths = RenderPath::all();
    s.add("三路径齐备", paths.len() == 3, "three");
    s.add("名称互异", {
        let n: Vec<&str> = paths.iter().map(|p| p.name()).collect();
        n[0] != n[1] && n[1] != n[2] && n[0] != n[2]
    }, "names");
    // 族二：覆盖对拍与容差
    let cpu = vec![1000u32, 500, 0, 1000];
    let sc = vec![1000u32, 460, 20, 980];
    let ana = vec![990u32, 520, 0, 1000];
    s.add("容差内一致", coverage_compare(&cpu, &sc, AA_TOLERANCE) == (40, 0), "in-tol");
    s.add("最大差计算", coverage_compare(&cpu, &ana, AA_TOLERANCE).0 == 60, "max-diff");
    s.add("超容差计数", {
        let harsh = vec![1000u32, 100, 0, 0];
        coverage_compare(&cpu, &harsh, AA_TOLERANCE).1 == 2
    }, "over-count");
    // 三路对拍
    s.add("三路全过容差", triple_compare(&cpu, &sc, &ana, AA_TOLERANCE).is_ok(), "triple-ok");
    let bad_sc = vec![1000u32, 500, 900, 0];
    match triple_compare(&cpu, &bad_sc, &ana, AA_TOLERANCE) {
        Err(m) => s.add("超差点名路径对", m.contains("CPU-SC") && m.contains("SC-ANA"), "triple-red"),
        Ok(()) => s.add("超差点名路径对", false, "expected err"),
    }
    // 族三：差异仲裁
    s.add("容差内=合法差异", arbitrate(50, false, false) == DiffArbitration::LegalImplDiff, "legal");
    s.add("渐变带边缘=合法差异", arbitrate(300, true, false) == DiffArbitration::LegalImplDiff, "ramp");
    s.add("内区孤立=实现bug", arbitrate(400, false, true) == DiffArbitration::ImplementationBug, "bug");
    s.add("内区非孤立=基准错", arbitrate(400, false, false) == DiffArbitration::ReferenceWrong, "ref-wrong");
    // 族四：每夜对拍与变更纪律
    let ok_night = NightlyRecord { case_count: 50, pass_count: 50, synced_change: true };
    s.add("每夜全过通过", ok_night.verdict().is_ok(), "night-ok");
    let partial = NightlyRecord { case_count: 50, pass_count: 48, synced_change: true };
    match partial.verdict() {
        Err(m) => s.add("未过点名跟进", m.contains("48/50") && m.contains("仲裁"), "night-red"),
        Ok(()) => s.add("未过点名跟进", false, "expected err"),
    }
    let unsync = NightlyRecord { case_count: 50, pass_count: 50, synced_change: false };
    match unsync.verdict() {
        Err(m) => s.add("变更未同改拒绝", m.contains("三路同改") && m.contains("纪律"), "unsync"),
        Ok(()) => s.add("变更未同改拒绝", false, "expected err"),
    }
    // 族五：观感统一
    s.add("三路一致支撑观感统一", visual_uniformity_supported(true), "uniform");
    s.add("不一致即观感风险", !visual_uniformity_supported(false), "risk");
    s.add("容差规范显性", AA_TOLERANCE == 80, "tolerance");
    s
}
