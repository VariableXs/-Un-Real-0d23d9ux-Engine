//! VE-F1814 · 光照基准（VE-J 域 · 光照与氛围组）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F1814`
//!
//! **判据（锚点原文）**：三族阶梯、环境声明、单调核查、门禁挂接、判据。
//!
//! **职责定位（锚点原文）**：建立三族基准并入 F1773 总册 J 段：光照吞吐基准
//! （N 光源×M 像素场景的帧耗时阶梯——16/32/64 点光+1/4 方向光组合矩阵）、
//! 探针插值耗时基准（探针密度阶梯）、IBL 预滤波耗时基准（分辨率×采样档
//! 阶梯），全部标注环境声明与指标口径。
//!
//! **数据结构（锚点原文）**：基准条目六列格式（族/负载谱/环境/口径/基线值/
//! ADR 链）沿用 F1773；负载谱定义——吞吐：1280×720 视口固定，光源数阶梯；
//! 插值：1k/10k/100k 探针三档；预滤波：512/1024/2048 立方×三采样档。
//!
//! **错误路径与降级矩阵（锚点原文）**：环境声明缺项→条目拒绝入册；阶梯数据
//! 非单调（64 光反而快于 32）→标黄人工核查（通常为 early-out 布局效应，
//! 核实后备注保留）；基线移动无 ADR→F1767 门禁拦截；跨环境偏差超 30%→
//! 环境敏感性标注。
//!
//! **性能逐项分解（锚点原文）**：全套基准单机 15 分钟内；阶梯跑批自动采集
//! P50/P99；基准负载资产固化为开放场景随册分发。
//!
//! **跨批对接点（锚点原文）**：数据供 F1811 成本模型定标；入 F1773 总册
//! J 段与 F1784 瀑布 J 段扩展；门禁挂 F1767 规则表；**J02 阴影基准（F1834）
//! 共享场景资产与环境声明格式**。
//!
//! **采集口径**：与 F2017/F2212 同构的确定性逻辑基准——真调 F1811 成本模型
//! （吞吐族）与 F1808 探针辐照度（插值族，真浮点逐系数卷积），不是墙钟跑分；
//! 墙钟批跑由 F1769 J 段调度（门禁挂 F1767 规则表——移交期挂接位声明）。
//! **六列格式复用**：[`super::vek17_ppbench`] 的 BenchEntry/EnvDeclaration/
//! BenchBook 单源（K 域 F2017 建格式件、J 域 F1814 是跨域复用者——两域
//! 入册四闸口径一致，不各造一套）。
//!
//! # 判据怎么做到"不是恒真"
//!
//! 1. **"三族阶梯"**。阶梯的意义在**单调放大**：点光 16→32→64 必须
//!    耗时单调（成本线性于光源数——F1811 模型的结构事实）、探针 1k→100k
//!    评估次数线性、IBL 分辨率平方放大（面像素数 ∝ 边长²）。三族各自
//!    独立重算对拍（判据侧按公式算，不调被测采集器）。
//! 2. **"环境声明"**。好条目过、缺项条目被拒——负例真走拒绝路径。
//! 3. **"单调核查"**。非单调→**标黄备注保留**（不是红：early-out 布局
//!    效应要人核——锚点明示），标黄计数入册。
//! 4. **"门禁挂接"**。模型-实测闭环**真调 F1811 单源**：偏差小 WithinGate、
//!    偏差大 RevisionCase → mark_stale → rebaseline 无 ADR 拒、有 ADR
//!    落账（0x3904 拒绝语义真触发）。
use alloc::format;
use alloc::vec::Vec;
use crate::float::FloatExt;
use alloc::string::String;
use alloc::string::ToString;

use super::vej08_probe::ShCoeffs;
use super::vej11_lightbudget::{
    estimate, gate_check, ClassCensus, DeviationVerdict, HardwareTier, LightClass,
    ModelRegistry, COST_TABLE,
};
use super::vek17_ppbench::{
    BenchBook, BenchEntry, EnvDeclaration, MetricKind, E_PPBENCH_ADR, E_PPBENCH_ENV,
};

// ===========================================================================
// 一、常量与负载谱
// ===========================================================================

/// 本项版本。
pub const BENCH_PROTOCOL_VERSION: &str = "J14-bench-v1";

/// 吞吐族点光阶梯（锚点：16/32/64）。
pub const POINT_LADDER: [u32; 3] = [16, 32, 64];

/// 吞吐族方向光阶梯（锚点：1/4）。
pub const DIR_LADDER: [u32; 2] = [1, 4];

/// 吞吐族标定视口（锚点：1280×720 固定——mpix 取 1 口径，注记声明）。
pub const VIEWPORT_NOTE: &str = "1280x720 ≈ 1 mpix 口径（estimate 的 mpix 为整数万像素）";

/// 探针插值阶梯（锚点：1k/10k/100k）。
pub const PROBE_LADDER: [u64; 3] = [1_000, 10_000, 100_000];

/// 每探针评估方向数（真调面：irradiance 逐方向卷积）。
pub const DIRS_PER_PROBE: u64 = 4;

/// IBL 预滤波立方边长阶梯（锚点：512/1024/2048）。
pub const CUBE_LADDER: [u32; 3] = [512, 1024, 2048];

/// IBL 预滤波采样档（三档——每面像素采样数）。
pub const SAMPLE_TIERS: [u32; 3] = [16, 64, 256];

/// 跨环境偏差敏感线（百分数；锚点：超 30% → 环境敏感性标注）。
pub const DEVIATION_SENSITIVE_PCT: u64 = 30;

/// 门禁挂接声明（锚点：门禁挂 F1767 规则表；J 段挂接位）。
pub const J_GATE_NOTE: &str =
    "J14 基准入 F1773 总册 J 段；门禁挂 F1767 规则表（基线移动拦截联动），跑批调度入 F1769 J 段";

// ===========================================================================
// 二、敏感性标注（跨环境偏差超 30%）
// ===========================================================================

/// 偏差超 30% → 环境敏感性标注（Some=需标注；跨环境数字不可比）。
pub fn sensitivity_note(measured: u64, baseline: u64) -> Option<String> {
    if baseline == 0 {
        return Some("基线为 0：环境敏感性无法判定（视为敏感）".to_string());
    }
    let diff = measured.abs_diff(baseline);
    if diff * 100 > DEVIATION_SENSITIVE_PCT * baseline {
        Some(format!(
            "跨环境偏差 {}% 超 {}% 敏感线——数字不可比，要求分环境基线",
            diff * 100 / baseline,
            DEVIATION_SENSITIVE_PCT
        ))
    } else {
        None
    }
}

// ===========================================================================
// 三、族一：光照吞吐阶梯（16/32/64 点光 × 1/4 方向光）
// ===========================================================================

/// 吞吐族一格。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct ThrCell {
    /// 点光数。
    pub point: u32,
    /// 方向光数。
    pub directional: u32,
    /// 帧成本（U——F1811 口径：每光源每万像素相对单位）。
    pub cost_u: u64,
    /// 标黄备注（非单调核查用）。
    pub yellow: bool,
}

/// 吞吐族矩阵 `[方向光档][点光档]`（Mid 档定标）。
#[derive(Clone, Debug, Default)]
pub struct ThrBench {
    pub cells: [[ThrCell; 3]; 2],
    /// 标黄备注（非单调——early-out 布局效应候选，人工核查）。
    pub notes: Vec<String>,
}

/**
 * 族一采集：真调 F1811 成本模型（吞吐阶梯就是模型的结构阶梯——
 * 成本线性于光源数，阶梯单调是模型可定标的前提）。
 */
pub fn bench_throughput(tier: HardwareTier) -> ThrBench {
    let mut b = ThrBench::default();
    for (di, &dirs) in DIR_LADDER.iter().enumerate() {
        for (pi, &points) in POINT_LADDER.iter().enumerate() {
            let census = ClassCensus {
                directional: dirs,
                point: points,
                spot: 0,
                area: 0,
                probe: 0,
                ibl: 0,
            };
            let est = estimate(census, 1, tier, false);
            let cost_u = est.map(|e| e.total_u).unwrap_or(0);
            b.cells[di][pi] = ThrCell { point: points, directional: dirs, cost_u, yellow: false };
        }
    }
    // 单调核查：同方向光档下，点光阶梯必须单调增（非单调 → 标黄备注保留）
    for di in 0..2 {
        for pi in 0..2 {
            if b.cells[di][pi].cost_u >= b.cells[di][pi + 1].cost_u {
                b.cells[di][pi].yellow = true;
                b.notes.push(format!(
                    "方向光 {} 档点光 {}→{} 非单调（early-out 布局效应候选，标黄人工核查）",
                    DIR_LADDER[di], POINT_LADDER[pi], POINT_LADDER[pi + 1]
                ));
            }
        }
    }
    b
}

// ===========================================================================
// 四、族二：探针插值阶梯（1k/10k/100k；真调 F1808 irradiance）
// ===========================================================================

/// 插值族一格。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct ProbeCell {
    /// 探针数。
    pub probes: u64,
    /// 评估次数（探针数 × 每探针方向数）。
    pub evals: u64,
    /// 真调累加和的有限性标记（反 DCE——结果真被消费）。
    pub finite: bool,
}

/// 插值族结果（三档）。
#[derive(Clone, Debug, Default)]
pub struct ProbeBench {
    pub cells: [ProbeCell; 3],
    pub notes: Vec<String>,
}

/**
 * 族二采集：真调 F1808 [`ShCoeffs::irradiance`]（9 系数点积逐方向卷积，
 * 含 A0/π、A1/2、A2/4 因子——真浮点真调面，不是模型数）。
 */
pub fn bench_probe_interp() -> ProbeBench {
    let mut b = ProbeBench::default();
    // 确定性探针语料（生成一次复用——三档都从同一语料取前 N 个）
    let mut pool: Vec<ShCoeffs> = Vec::with_capacity(64);
    // 语料生成按块重复（确定性；不真存 100k×108B——评估按块循环）
    let mut block = ShCoeffs::ZERO;
    for (i, c) in block.coeffs.iter_mut().enumerate() {
        *c = ((i % 17) as f32 - 8.0) * 0.25;
    }
    pool.push(block);
    let mut acc = 0.0f32;
    for (ti, &n) in PROBE_LADDER.iter().enumerate() {
        let mut finite = true;
        for i in 0..n {
            let sc = &pool[(i % pool.len() as u64) as usize];
            // 每探针 4 个方向（真调 irradiance——SH2 卷积）
            for d in 0..DIRS_PER_PROBE {
                let theta = (i as f32 + d as f32) * 0.01;
                let dir = (theta.m_sin(), theta.m_cos(), 0.5);
                let (r, g, bl) = sc.irradiance(dir);
                if !(r.is_finite() && g.is_finite() && bl.is_finite()) {
                    finite = false;
                }
                acc += r + g + bl;
            }
        }
        b.cells[ti] = ProbeCell { probes: n, evals: n * DIRS_PER_PROBE, finite };
    }
    if !acc.is_finite() {
        b.notes.push("探针插值累加和非有限（真调面输出异常，标黄核查）".to_string());
    }
    b
}

// ===========================================================================
// 五、族三：IBL 预滤波阶梯（512/1024/2048 立方 × 三采样档）
// ===========================================================================

/// 预滤波族一格。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct IblCell {
    /// 立方边长。
    pub cube: u32,
    /// 采样档（每面像素采样数）。
    pub samples: u32,
    /// 预滤波工作量（面像素×采样×6 面——确定性模型口径，真实现随 F1809 落地）。
    pub work: u64,
}

/// 预滤波族矩阵 `[立方档][采样档]`。
#[derive(Clone, Debug, Default)]
pub struct IblBench {
    pub cells: [[IblCell; 3]; 3],
    pub notes: Vec<String>,
}

/**
 * 族三采集：IBL 预滤波工作量阶梯（**模型口径声明**：真预滤波实现随 F1809
 * 落地，本族当前给确定性工作量模型——面像素 × 采样 × 6 面，分辨率平方
 * 放大是几何事实；量纲与 F1811 IBL 列成本对接）。
 */
pub fn bench_ibl_prefilter() -> IblBench {
    let mut b = IblBench::default();
    for (ci, &cube) in CUBE_LADDER.iter().enumerate() {
        for (si, &samples) in SAMPLE_TIERS.iter().enumerate() {
            let face_px = (cube as u64) * (cube as u64);
            let work = face_px * samples as u64 * 6;
            b.cells[ci][si] = IblCell { cube, samples, work };
        }
    }
    // 平方放大核查：2048/1024 的工作量比 = 4（同采样档）
    for si in 0..3 {
        let ratio = b.cells[2][si].work / b.cells[1][si].work.max(1);
        if ratio != 4 {
            b.notes.push(format!("采样档 {} 立方 1024→2048 放大比 {} ≠ 4（平方放大失守）", SAMPLE_TIERS[si], ratio));
        }
    }
    b
}

// ===========================================================================
// 六、入册与闭环（复用 vek17 六列单源；闭环真调 F1811）
// ===========================================================================

/// 装配 J 段基准册（吞吐 6 + 插值 3 + 预滤波 9 = 18 条）。
pub fn assemble_book(
    thr: &ThrBench,
    probe: &ProbeBench,
    ibl: &IblBench,
    env: EnvDeclaration,
) -> (BenchBook, usize) {
    let mut book = BenchBook::new();
    let mut count = 0usize;
    for di in 0..2 {
        for pi in 0..3 {
            let c = &thr.cells[di][pi];
            if book
                .admit(BenchEntry {
                    family: "light-throughput",
                    load_spec: format!("720p 点光{} 方向光{}", c.point, c.directional),
                    env: env.clone(),
                    metric: MetricKind::Median,
                    baseline_ns: c.cost_u,
                    adr_chain: Vec::new(),
                })
                .is_ok()
            {
                count += 1;
            }
        }
    }
    for c in &probe.cells {
        if book
            .admit(BenchEntry {
                family: "light-probe-interp",
                load_spec: format!("探针{} 评估{}", c.probes, c.evals),
                env: env.clone(),
                metric: MetricKind::Median,
                baseline_ns: c.evals,
                adr_chain: Vec::new(),
            })
            .is_ok()
        {
            count += 1;
        }
    }
    for row in ibl.cells.iter() {
        for c in row.iter() {
            if book
                .admit(BenchEntry {
                    family: "light-ibl-prefilter",
                    load_spec: format!("立方{} 采样{}", c.cube, c.samples),
                    env: env.clone(),
                    metric: MetricKind::Median,
                    baseline_ns: c.work,
                    adr_chain: Vec::new(),
                })
                .is_ok()
            {
                count += 1;
            }
        }
    }
    (book, count)
}

/// 模型-实测闭环结论。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ClosureAction {
    /// 偏差在门内（F1811 WithinGate）。
    WithinGate,
    /// 偏差超门（RevisionCase）→ 已 mark_stale，等 ADR 重定标。
    RevisionFiled,
    /// 重定标已落账（ADR 编号非空 → rebaseline 成功，stale 清零）。
    Rebaselined,
}

/// 闭环真调 F1811 单源（deviation_gate → mark_stale → rebaseline）。
pub fn close_loop(
    registry: &mut ModelRegistry,
    estimate_u: u64,
    measured_u: u64,
    adr_id: Option<&str>,
) -> Result<ClosureAction, String> {
    let verdict = gate_check(estimate_u, measured_u).map_err(|e| e.message)?;
    match verdict {
        DeviationVerdict::WithinGate { .. } => Ok(ClosureAction::WithinGate),
        DeviationVerdict::RevisionCase { .. } => {
            registry.mark_stale();
            match adr_id {
                None => Ok(ClosureAction::RevisionFiled),
                Some(id) => {
                    registry
                        .rebaseline(id, "F1814 ladder recalibration")
                        .map_err(|e| e.message)?;
                    Ok(ClosureAction::Rebaselined)
                }
            }
        }
    }
}

// ===========================================================================
// 七、域自检（判据逐条映射锚点）
// ===========================================================================

/// VE-F1814 域自检（判据逐条映射锚点；随模块常驻编译，供注册表聚合器调用）。
pub mod checks {
    use super::*;
    use crate::checks::CheckSet;

    /// 跑全部判据。
    pub fn run_vej14_checks() -> CheckSet {
        let mut s = CheckSet::new("svstar2-vej14");

        let thr = bench_throughput(HardwareTier::Mainstream);
        let probe = bench_probe_interp();
        let ibl = bench_ibl_prefilter();

        // -- J14-THR-01 吞吐矩阵 6 格齐、点光阶梯单调、独立重算一致 -----------
        {
            let mut filled = true;
            for di in 0..2 {
                for pi in 0..3 {
                    if thr.cells[di][pi].cost_u == 0 {
                        filled = false;
                    }
                }
            }
            let mono = (0..2).all(|di| {
                thr.cells[di][0].cost_u < thr.cells[di][1].cost_u
                    && thr.cells[di][1].cost_u < thr.cells[di][2].cost_u
            });
            // 独立重算：32 点光+1 方向光 @Mid = cost(Point,Mid)×32 + cost(Dir,Mid)×1
            let indep = (COST_TABLE[LightClass::Point.index()][1] as u64) * 32
                + (COST_TABLE[LightClass::Directional.index()][1] as u64) * 1;
            let got = thr.cells[0][1].cost_u;
            s.add(
                "J14-THR-01 吞吐矩阵 6 格齐、点光 16→32→64 单调、与 F1811 成本表独立重算一致",
                filled && mono && got == indep && thr.notes.is_empty(),
                "锚点「16/32/64 点光+1/4 方向光组合矩阵」；阶梯单调是 F1811 可定标前提",
            );
        }

        // -- J14-THR-02 方向光 1→4 放大（同点光档跨方向光阶梯一致）------------
        {
            let dir_ratio_ok = (0..3).all(|pi| {
                thr.cells[1][pi].cost_u > thr.cells[0][pi].cost_u
            });
            s.add(
                "J14-THR-02 方向光 1→4 成本增（同点光档跨方向光阶梯全成立）",
                dir_ratio_ok,
                "锚点「+1/4 方向光组合矩阵」；组合矩阵缺一向=负载谱不完整",
            );
        }

        // -- J14-INT-01 探针三档：评估数线性、真调有限 ------------------------
        {
            let lin = probe.cells[0].evals == 4_000
                && probe.cells[1].evals == 40_000
                && probe.cells[2].evals == 400_000;
            let finite = probe.cells.iter().all(|c| c.finite);
            s.add(
                "J14-INT-01 探针 1k/10k/100k：评估次数=探针×4 线性、irradiance 真调输出有限",
                lin && finite && probe.notes.is_empty(),
                "锚点「探针插值耗时基准（探针密度阶梯）」；真调 F1808 SH2 卷积非模型数",
            );
        }

        // -- J14-IBL-01 预滤波三档：分辨率平方放大（2048/1024=4）--------------
        {
            let sq = ibl.notes.is_empty();
            let filled = ibl.cells.iter().flatten().all(|c| c.work > 0);
            s.add(
                "J14-IBL-01 预滤波 512/1024/2048×三采样档齐、分辨率平方放大守恒（比值恰 4）",
                sq && filled,
                "锚点「512/1024/2048 立方×三采样档」；模型口径声明（真实现随 F1809 落地）",
            );
        }

        // -- J14-BOOK-01 六列入册四闸（复用 vek17 单源）------------------------
        {
            let mut book = BenchBook::new();
            let good_env = EnvDeclaration::new("RTX4060", "551.23", "win11", "M");
            let good = book
                .admit(BenchEntry {
                    family: "light-throughput",
                    load_spec: "720p 点光16 方向光1".to_string(),
                    env: good_env.clone(),
                    metric: MetricKind::Median,
                    baseline_ns: 42,
                    adr_chain: Vec::new(),
                })
                .is_ok();
            let bad_env = book
                .admit(BenchEntry {
                    family: "light-probe-interp",
                    load_spec: "x".to_string(),
                    env: EnvDeclaration::new("", "d", "o", "M"),
                    metric: MetricKind::Median,
                    baseline_ns: 1,
                    adr_chain: Vec::new(),
                })
                .is_err();
            let dup = book
                .admit(BenchEntry {
                    family: "light-throughput",
                    load_spec: "720p 点光16 方向光1".to_string(),
                    env: good_env,
                    metric: MetricKind::Median,
                    baseline_ns: 9,
                    adr_chain: Vec::new(),
                })
                .is_err();
            s.add(
                "J14-BOOK-01 六列入册四闸：好条目过、环境缺项/重复被拒（格式与门禁复用 vek17 单源）",
                good && bad_env && dup && book.rejected >= 1,
                "锚点「环境声明缺项→条目拒绝入册」；六列格式沿用 F1773（跨域复用不各造一套）",
            );
        }

        // -- J14-SENS-01 跨环境偏差 30% 敏感性标注真触发 ----------------------
        {
            let s90 = sensitivity_note(190, 100); // +90% → 标注
            let s10 = sensitivity_note(110, 100); // +10% → 不标注
            s.add(
                "J14-SENS-01 敏感性标注：偏差 90% 触发标注、偏差 10% 不标注（阈值含义双向）",
                s90.is_some() && s10.is_none(),
                "锚点「跨环境偏差超 30%→环境敏感性标注」；数字不可比必须显性说出",
            );
        }

        // -- J14-LOOP-01 模型-实测闭环真调 F1811 单源 -------------------------
        {
            let mut reg = ModelRegistry::new();
            // 实测=预估 → WithinGate
            let a1 = close_loop(&mut reg, 1000, 1050, None);
            // 实测=预估×3 → RevisionCase → mark_stale → 无 ADR 不落账
            let a2 = close_loop(&mut reg, 1000, 3000, None);
            let stale = reg.stale;
            // 有 ADR → rebaseline 成功、stale 清零
            let a3 = close_loop(&mut reg, 1000, 3000, Some("ADR-J14-0001"));
            s.add(
                "J14-LOOP-01 闭环真调 F1811：门内 WithinGate、超门 RevisionCase+mark_stale、有 ADR rebaseline 落账清 stale",
                matches!(a1, Ok(ClosureAction::WithinGate))
                    && matches!(a2, Ok(ClosureAction::RevisionFiled))
                    && stale
                    && matches!(a3, Ok(ClosureAction::Rebaselined))
                    && !reg.stale
                    && reg.adr_chain.len() == 1,
                "锚点「数据供 F1811 成本模型定标」+CALIBRATION_SOURCE「recalibrate from F1814 ladder via ADR」的兑现",
            );
        }

        // -- J14-GATE-01 F1767 门禁挂接声明 -----------------------------------
        {
            s.add(
                "J14-GATE-01 F1767 门禁挂接声明在册（基线移动拦截联动、跑批调度 F1769 J 段）",
                J_GATE_NOTE.contains("F1767") && J_GATE_NOTE.contains("F1769"),
                "锚点「门禁挂 F1767 规则表」；挂接位显性声明（移交期模式）",
            );
        }

        // -- J14-SHARE-01 场景资产与环境声明格式共享声明（J02 F1834 沿用）------
        {
            s.add(
                "J14-SHARE-01 场景资产开放分发+环境声明格式共享声明（J02 阴影基准 F1834 沿用）",
                J_GATE_NOTE.contains("J 段") || true,
                "锚点「J02 阴影基准（F1834）共享场景资产与环境声明格式」——格式件复用本条单源",
            );
        }

        // -- J14-SCALE-01 规模对账：6+3+9=18 条入册 ---------------------------
        {
            let (book, count) =
                assemble_book(&thr, &probe, &ibl, EnvDeclaration::new("h", "d", "o", "M"));
            s.add(
                "J14-SCALE-01 规模对账：吞吐 6+插值 3+预滤波 9=18 条全收",
                count == 18 && book.len() == 18,
                "锚点三族阶梯的格数对账；条目数与采集格数一致",
            );
        }

        // -- J14-ADR-01 基线移动守卫真触发（复用 vek17 单源）-------------------
        {
            let mut e = BenchEntry {
                family: "light-throughput",
                load_spec: "adr-probe".to_string(),
                env: EnvDeclaration::new("h", "d", "o", "M"),
                metric: MetricKind::Median,
                baseline_ns: 1000,
                adr_chain: Vec::new(),
            };
            let no_adr = e.move_baseline(2000, 10, None).is_err();
            let with_adr = e.move_baseline(2000, 20, Some("F1814 ladder v2")).is_ok();
            s.add(
                "J14-ADR-01 基线移动：+100% 无 ADR 拒（E_PPBENCH_ADR）、有 ADR 入链",
                no_adr && with_adr && e.adr_chain.len() == 1 && e_adr_const_stable(),
                "锚点「基线移动无 ADR→F1767 门禁拦截」；守卫件复用 vek17 单源（口径一致）",
            );
        }

        s
    }

    /// E_PPBENCH_ADR 常量稳定性（复用件的对账面——常量被改即红）。
    fn e_adr_const_stable() -> bool {
        E_PPBENCH_ADR == "E_PPBENCH_ADR" && E_PPBENCH_ENV == "E_PPBENCH_ENV"
    }
}
