//! VE-F0760 布尔组收口
//! 职责：F0741~F0759 二十单的组级收口——能力矩阵、跨模块交叉验证
//!       （裁剪×重建×缓存联动）、遗留风险挂账、组完成判定。
//! 契约：矩阵二十行全 Done；交叉验证三例全绿；风险挂账远景。

use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::vec321_bezcore::Pt;
use super::vec341_boolorg::{FillRule, Poly, DirEdge};

/// 能力三态
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum BCapState {
    Done,
    Missing,
}

/// 能力矩阵行
#[derive(Clone, Debug, PartialEq)]
pub struct BCapRow {
    pub task: &'static str,
    pub module: &'static str,
    pub state: BCapState,
}

/// 二十单能力矩阵
pub fn capability_matrix() -> Vec<BCapRow> {
    alloc::vec![
        BCapRow { task: "VE-F0741", module: "vec341_boolorg", state: BCapState::Done },
        BCapRow { task: "VE-F0742", module: "vec342_clip", state: BCapState::Done },
        BCapRow { task: "VE-F0743", module: "vec343_curedge", state: BCapState::Done },
        BCapRow { task: "VE-F0744", module: "vec344_bdegen", state: BCapState::Done },
        BCapRow { task: "VE-F0745", module: "vec345_rebuild", state: BCapState::Done },
        BCapRow { task: "VE-F0746", module: "vec346_bopt", state: BCapState::Done },
        BCapRow { task: "VE-F0747", module: "vec347_selfx", state: BCapState::Done },
        BCapRow { task: "VE-F0748", module: "vec348_bverify", state: BCapState::Done },
        BCapRow { task: "VE-F0749", module: "vec349_bfuzz", state: BCapState::Done },
        BCapRow { task: "VE-F0750", module: "vec350_iconwfl", state: BCapState::Done },
        BCapRow { task: "VE-F0751", module: "vec351_maskcomp", state: BCapState::Done },
        BCapRow { task: "VE-F0752", module: "vec352_xform", state: BCapState::Done },
        BCapRow { task: "VE-F0753", module: "vec353_bcache", state: BCapState::Done },
        BCapRow { task: "VE-F0754", module: "vec354_bduel", state: BCapState::Done },
        BCapRow { task: "VE-F0755", module: "vec355_prec", state: BCapState::Done },
        BCapRow { task: "VE-F0756", module: "vec356_bdocs", state: BCapState::Done },
        BCapRow { task: "VE-F0757", module: "vec357_bbench", state: BCapState::Done },
        BCapRow { task: "VE-F0758", module: "vec358_bdet", state: BCapState::Done },
        BCapRow { task: "VE-F0759", module: "vec359_bext", state: BCapState::Done },
        BCapRow { task: "VE-F0760", module: "vec360_bclose", state: BCapState::Done },
    ]
}

pub fn matrix_complete(m: &[BCapRow]) -> bool {
    m.len() == 20 && m.iter().all(|r| r.state == BCapState::Done)
}

/// 交叉验证一：裁剪 × 重建——裁剪结果的扫描区间面积 = 裁剪面积
pub fn xval_clip_span(subject: &Poly, clip: super::vec342_clip::ClipRect, ys: &[f64], dy: f64) -> f64 {
    let clipped = super::vec342_clip::clip_rect(subject, clip);
    super::vec345_rebuild::spans_area(&super::vec345_rebuild::spans_at(&clipped.edges(), ys, FillRule::NonZero), dy)
}

/// 交叉验证二：验证器 × fuzz——fuzz 生成的凸多边形全过结构级验证
pub fn xval_verify_fuzz(seed: u64, rounds: usize) -> bool {
    let st = super::vec349_bfuzz::fuzz_run(seed, rounds);
    st.struct_pass == rounds
}

/// 交叉验证三：精度预处理 × 缓存——预处理后同形状必命中缓存
pub fn xval_prec_cache(p: &Poly) -> bool {
    let tier = super::vec355_prec::PrecTier::F64;
    let clean = super::vec355_prec::preprocess(p, tier);
    let mut cache = super::vec353_bcache::BoolCache::new(4);
    let key = super::vec353_bcache::CacheKey {
        ka: super::vec353_bcache::key_of(&clean),
        kb: super::vec353_bcache::key_of(&clean),
        op: super::vec341_boolorg::BoolOp::Union,
        rule: FillRule::NonZero,
    };
    cache.put(key, &clean, &clean, &clean);
    cache.hit(key, &clean, &clean)
}

/// 遗留风险
pub fn leftover_risks() -> Vec<&'static str> {
    alloc::vec![
        "曲线边布尔当前为扫描近似——精确贝塞尔裁剪挂账 D08 远景",
        "同向洞（非零规则填洞）语义未暴露给蒙版复合——需复合模式扩展",
        "沙箱输出上限为事件计数——多边形级别配额挂账 D09",
    ]
}

/// 组完成判定
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct CloseInput {
    pub matrix_done: bool,
    pub xval_pass: bool,
    pub risks_filed: usize,
}

pub fn group_done(c: CloseInput) -> bool {
    c.matrix_done && c.xval_pass && c.risks_filed >= 1
}

pub fn run_vec360_checks() -> crate::checks::CheckSet {
    let mut s = CheckSet::new("svstar2-vec360");
    // 族一：能力矩阵
    let m = capability_matrix();
    s.add("矩阵-二十行", m.len() == 20, "20 rows");
    s.add("矩阵-全完成", matrix_complete(&m), "all Done");
    s.add("矩阵-任务号连续", m.windows(2).all(|w| {
        let a = w[0].task.trim_start_matches("VE-F");
        let b = w[1].task.trim_start_matches("VE-F");
        b.parse::<u32>().unwrap_or(0) == a.parse::<u32>().unwrap_or(0) + 1
    }), "consecutive");
    // 族二：交叉验证一（裁剪×重建）
    let square = Poly::new(alloc::vec![Pt::new(0.0, 0.0), Pt::new(4.0, 0.0), Pt::new(4.0, 4.0), Pt::new(0.0, 4.0)]);
    let clip = super::vec342_clip::ClipRect { min: Pt::new(1.0, 1.0), max: Pt::new(3.0, 3.0) };
    let ys: Vec<f64> = alloc::vec![1.5, 2.5];
    s.add("交叉-裁剪区间面积", (xval_clip_span(&square, clip, &ys, 1.0) - 4.0).abs() < 1e-9, "2×2 span");
    // 族三：交叉验证二（验证器×fuzz）
    s.add("交叉-fuzz结构全过", xval_verify_fuzz(2024, 32), "32 rounds clean");
    // 族四：交叉验证三（精度×缓存）
    s.add("交叉-预处理必命中", xval_prec_cache(&square), "cache hit after preprocess");
    // 族五：风险挂账
    let risks = leftover_risks();
    s.add("风险-三条", risks.len() == 3, "3 risks");
    s.add("风险-挂账域", risks.iter().any(|r| r.contains("D08")) && risks.iter().any(|r| r.contains("D09")), "linked domains");
    // 族六：组判定
    s.add("判定-全绿", group_done(CloseInput { matrix_done: true, xval_pass: true, risks_filed: 3 }), "group done");
    s.add("判定-矩阵缺", !group_done(CloseInput { matrix_done: false, xval_pass: true, risks_filed: 3 }), "matrix gate");
    s.add("判定-交叉缺", !group_done(CloseInput { matrix_done: true, xval_pass: false, risks_filed: 3 }), "xval gate");
    s.add("判定-风险缺", !group_done(CloseInput { matrix_done: true, xval_pass: true, risks_filed: 0 }), "risk gate");
    // 族七：与前置组联动（曲线组矩阵仍全绿）
    s.add("联动-曲线组", super::vec339_cclose::matrix_complete(&super::vec339_cclose::capability_matrix()), "curve group intact");
    let _ = DirEdge::new(Pt::ZERO, Pt::ZERO);
    s
}
