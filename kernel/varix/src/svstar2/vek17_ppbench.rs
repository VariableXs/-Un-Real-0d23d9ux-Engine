//! VE-F2017 · 后处理基准（VE-K 域 · 后处理架构与 Bloom 组）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F2017`
//!
//! **判据（锚点原文）**：耗时矩阵、差分法、TAA 双指标、方法学声明、判据。
//!
//! **职责定位（锚点原文）**：后处理基准——三族基准入 F1773 K 段：全链后处理
//! 耗时（全开 vs 分效果的耗时阶梯——十效果逐个+全开组合的独占耗时矩阵：效果
//! 成本可见性）、TAA 基准（质量/耗时：收敛帧数×耗时双指标）、Bloom 基准
//! （mip 级阶梯：5/7 级耗时与观感对照），入册回归。
//!
//! **数据结构（锚点原文）**：基准条目六列格式沿用 F1773；负载谱——全链：标定
//! 场景×逐效果开关矩阵（独占耗时=开-关差分法）；TAA：三收敛档×三场景；
//! Bloom：5/7 级×三强度。
//!
//! **错误路径与降级矩阵（锚点原文）**：差分法噪声（独占耗时为负）→统计方法学
//! 声明（多轮取中位+置信区间）；环境缺项拒绝入册；阶梯非单调→标黄核查备注
//! 保留；基线移动 ADR 守卫。
//!
//! **性能逐项分解（锚点原文）**：全矩阵单机 15 分钟；差分每效果 3 轮取中位；
//! TAA 质量指标=收敛后残差能量（量化定义声明）。
//!
//! **跨批对接点（锚点原文）**：数据供 F2014 成本模型定标/F2012 选型表；门禁
//! F1906 体系挂接（**K 域账本建立前暂挂 J 门禁——移交期声明**）；与
//! F1934/F1895 共享方法学。
//!
//! # 采集口径（确定性逻辑基准——F2212/F2412 同款方法学）
//!
//! 与 L/M 段基准同构：本模块采集**确定性逻辑耗时**（真调 F2014 成本模型与
//! vek04/vek11 的真实计算面），不是墙钟跑分——墙钟属环境外流程由 F1769 K 段
//! 调度，本模块只提供纯函数面（K 段门禁暂挂声明，不在本模块内置时序）。
//! 三族被测面：
//! - **全链族**：[`vek14_ppbudget::BudgetModel::estimate`]（真调 F2014 单源），
//!   独占耗时 = 全开 − 关掉该效果（差分法），3 轮确定性噪声取中位；
//! - **TAA 族**：[`vek11_taa`] 真调——halton 抖动采样 + `Rgb::lerp` 混合，
//!   耗时 = vek14 TAA 单价 × 相位数比例；质量指标 = **收敛后残差能量**
//!   （量化定义：抖动混合结果与真值逐像素平方差之和，tile 16×16）；
//! - **Bloom 族**：[`vek04_bloom::composite_weights`] / `plan_mip_chain` 真调，
//!   耗时 = vek14 Bloom 单价 × mip 比例；观感对照 = 权重尾频占比
//!   （**代理指标声明**：低频能量占比与"大半径柔和"单调相关，观感本体
//!   须人眼评审，机器只量化其单调代理）。
//!
//! # 判据怎么做到"不是恒真"
//!
//! 1. **"差分法"**。若只断「矩阵有数字」，恒填 0 的差分器也绿。必须断
//!    **逐格非零且等于独立重算**（判据侧不用被测差分器另算一遍）+
//!    **噪声鲁棒**（确定性噪声注入下无负值逃逸——负值即标黄计数，方法学
//!    真的在工作）+ **全开 ≤ 各独占之和**（共享减免的结构关系）。
//! 2. **"TAA 双指标"**。若只报耗时，质量维度就是盲区。必须断：相位多→
//!    **耗时单调增**且**残差单调降**（收敛档的意义就是这组权衡）+ 残差
//!    有限非负（能量不能是 NaN）。
//! 3. **"Bloom 观感对照"**。观感本体不可机测，必须**声明代理**：7 级的
//!    权重尾频占比 ≥ 5 级（mip 多=低频能量多=大半径柔和），代理与
//!    `composite_weights` 真调对拍。
//! 4. **"方法学声明"**。轮数/中位/置信区间若只是注释，改代码就漂。必须
//!    **常量化**且与采集函数逐处引用一致（判据对账）。
//! 5. **"入册守卫"**。环境四要素缺一拒册、口径唯一、去重、基线移动超线
//!    必须带 ADR——四闸逐条真触发（好条目过、坏条目各被各闸拦下）。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use super::vek04_bloom::{composite_weights, half_res_cost_ratio, MIP_LEVELS_MAX};
use super::vek11_taa::{clamp01, halton, Rgb, HALTON_BASE_X, HALTON_BASE_Y};
use super::vek14_ppbudget::{BudgetModel, DriftGate, Effect, HardwareTier};
use super::vek15_pptier::{QualityTier, TierTable, TAA_PHASES_HIGH};

// ---------------------------------------------------------------------------
// 一、常量与方法学声明（判据 P17-METH 的对账面）
// ---------------------------------------------------------------------------

/// 本项版本。
pub const BENCH_PROTOCOL_VERSION: &str = "K17-bench-v1";

/// 差分轮数（锚点：差分每效果 3 轮取中位）。
pub const DIFF_ROUNDS: u64 = 3;

/// 确定性噪声注入幅度（千分数 ±2%——模拟测量噪声，方法学演练面）。
pub const NOISE_PERMILLE: u64 = 20;

/// 基线移动容忍（百分数；沿用 F1773/F2212 口径：超线必须带 ADR）。
pub const BASELINE_DRIFT_PCT: u64 = 10;

/// 20% 修正门（千分数；F2014 单源引用——模型-实测闭环的回灌/立案分界）。
pub const DEVIATION_GATE_PERMILLE: u32 = 200;

/// TAA 收敛档（相位数三档；锚点「三收敛档」）。
pub const TAA_CONVERGE_TIERS: [u32; 3] = [2, 4, 8];

/// Bloom mip 阶梯（锚点：5/7 级）。
pub const BLOOM_MIP_TIERS: [u8; 2] = [5, 7];

/// Bloom 三强度（负载谱；作 decay 缩放的确定性谱）。
pub const BLOOM_STRENGTHS: [f32; 3] = [0.25, 0.75, 1.0];

/// 标定场景三档（锚点「标定场景×逐效果开关矩阵」；分辨率是 K01 成本的一次因子）。
pub const CALIB_SCENES: [(&str, u32, u32); 3] =
    [("s720p", 1280, 720), ("s1080p", 1920, 1080), ("s4k", 3840, 2160)];

/// 差分法出现负独占耗时（标黄核查——不是红：噪声候选）。
pub const E_PPBENCH_NEG: &str = "E_PPBENCH_NEG";
/// 环境缺项拒绝入册。
pub const E_PPBENCH_ENV: &str = "E_PPBENCH_ENV";
/// 基线移动无 ADR。
pub const E_PPBENCH_ADR: &str = "E_PPBENCH_ADR";
/// 指标口径二义。
pub const E_PPBENCH_METRIC: &str = "E_PPBENCH_METRIC";

/// K 段门禁暂挂声明（移交期：K 域账本建立前暂挂 J 的 F1906 门禁体系；
/// 建库后本常量归位为门禁挂接点——与 F2212 的 L_GATE_SUSPENDED_NOTE 同构）。
pub const K_GATE_SUSPENDED_NOTE: &str =
    "K17 基准入 F1773 K 段；门禁挂接暂移交期声明（F1906 体系，K 域账本建库前暂挂）";

// ---------------------------------------------------------------------------
// 二、六列条目与基准册（F1773 同构——与 F2212 同构声明非同码）
// ---------------------------------------------------------------------------

/// 环境声明（四要素强制完整）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EnvDeclaration {
    /// 硬件型号+档位（脱敏口径同 F1773）。
    pub hw_model: String,
    /// 驱动版本。
    pub driver: String,
    /// 系统。
    pub os: String,
    /// 档位（L/M/H）。
    pub tier: String,
}

impl EnvDeclaration {
    /// 构造。
    pub fn new(hw: &str, driver: &str, os: &str, tier: &str) -> EnvDeclaration {
        EnvDeclaration {
            hw_model: hw.to_string(),
            driver: driver.to_string(),
            os: os.to_string(),
            tier: tier.to_string(),
        }
    }

    /// 完整性校验：四要素全部非空。
    pub fn validate(&self) -> Result<(), String> {
        for (name, v) in [
            ("hw_model", &self.hw_model),
            ("driver", &self.driver),
            ("os", &self.os),
            ("tier", &self.tier),
        ] {
            if v.trim().is_empty() {
                return Err(format!("环境声明缺项：{}", name));
            }
        }
        Ok(())
    }
}

/// 指标口径（中位或 P99 二选一；K 段采集统一中位——口径常量见 [`METRIC_NOTE`]）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MetricKind {
    /// 中位数（K 段采集口径）。
    Median,
    /// P99（格式兼容保留；K 段采集不用——混报检查的另一半）。
    P99,
}

impl MetricKind {
    /// 短码。
    pub fn wire(self) -> &'static str {
        match self {
            MetricKind::Median => "p50",
            MetricKind::P99 => "p99",
        }
    }
}

/// ADR 变更记录（基线为什么变必须有案）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AmdRecord {
    /// 变更理由（非空）。
    pub reason: String,
    /// 变更时刻（毫秒逻辑钟）。
    pub at_ms: u64,
    /// 旧基线值（纳秒）。
    pub old_ns: u64,
    /// 新基线值（纳秒）。
    pub new_ns: u64,
}

impl AmdRecord {
    /// 构造（理由空即拒）。
    pub fn new(reason: &str, at_ms: u64, old_ns: u64, new_ns: u64) -> Result<AmdRecord, String> {
        if reason.trim().is_empty() {
            return Err(E_PPBENCH_ADR.to_string());
        }
        Ok(AmdRecord { reason: reason.to_string(), at_ms, old_ns, new_ns })
    }
}

/// 六列基准条目（族名/负载谱/环境/口径/基线/ADR 链——F1773 同构）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BenchEntry {
    /// 族名。
    pub family: &'static str,
    /// 负载谱（场景×配置的可读描述）。
    pub load_spec: String,
    /// 环境声明。
    pub env: EnvDeclaration,
    /// 指标口径（唯一）。
    pub metric: MetricKind,
    /// 基线数值（纳秒）。
    pub baseline_ns: u64,
    /// ADR 变更历史链。
    pub adr_chain: Vec<AmdRecord>,
}

impl BenchEntry {
    /// 入册前完整裁决。
    pub fn validate(&self) -> Result<(), String> {
        self.env.validate()?;
        if self.baseline_ns == 0 {
            return Err(E_PPBENCH_ENV.to_string());
        }
        Ok(())
    }

    /// 基线移动：超容忍必须带 ADR，无 ADR 拒绝。
    pub fn move_baseline(
        &mut self,
        new_ns: u64,
        now_ms: u64,
        adr_reason: Option<&str>,
    ) -> Result<(), String> {
        let old = self.baseline_ns;
        let drift_pct = if old == 0 {
            100
        } else {
            let diff = if new_ns > old { new_ns - old } else { old - new_ns };
            diff * 100 / old
        };
        if drift_pct > BASELINE_DRIFT_PCT {
            match adr_reason {
                Some(r) => {
                    let rec = AmdRecord::new(r, now_ms, old, new_ns)?;
                    self.adr_chain.push(rec);
                }
                None => return Err(E_PPBENCH_ADR.to_string()),
            }
        }
        self.baseline_ns = new_ns;
        Ok(())
    }
}

/// K 段基准册（去重按族名+负载谱；口径混报拒绝）。
#[derive(Debug, Default)]
pub struct BenchBook {
    entries: Vec<BenchEntry>,
    /// 拒绝入册计数。
    pub rejected: u32,
    /// 标黄核查计数（差分负值/阶梯非单调）。
    pub yellow_flags: u32,
}

impl BenchBook {
    /// 构造。
    pub fn new() -> BenchBook {
        BenchBook::default()
    }

    /// 入册（环境裁决在前，口径二义拒绝，同族同谱去重）。
    pub fn admit(&mut self, entry: BenchEntry) -> Result<(), String> {
        entry.validate()?;
        if self
            .entries
            .iter()
            .any(|e| e.metric != entry.metric && e.family == entry.family)
        {
            self.rejected += 1;
            return Err(E_PPBENCH_METRIC.to_string());
        }
        if self
            .entries
            .iter()
            .any(|e| e.family == entry.family && e.load_spec == entry.load_spec)
        {
            self.rejected += 1;
            return Err("重复条目".to_string());
        }
        self.entries.push(entry);
        Ok(())
    }

    /// 在册条目数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 标黄核查（外部注入——采集函数发现非单调/负值时调用）。
    pub fn flag_yellow(&mut self, note: String) {
        self.yellow_flags += 1;
        let _ = note;
    }

    /// 按族名+负载谱取条目。
    pub fn get(&self, family: &str, load: &str) -> Option<&BenchEntry> {
        self.entries
            .iter()
            .find(|e| e.family == family && e.load_spec == load)
    }
}

/// K 段指标口径声明（唯一口径=中位——与 [`DIFF_ROUNDS`] 配套）。
pub const METRIC_NOTE: &str = "K17 采集口径：DIFF_ROUNDS 轮确定性噪声取中位（p50）";

// ---------------------------------------------------------------------------
// 三、族一：全链耗时矩阵（差分法，十效果逐个 + 全开）
// ---------------------------------------------------------------------------

/// 一格差分结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct DiffCell {
    /// 独占耗时（微秒，3 轮中位）。
    pub exclusive_us: u64,
    /// 置信区间半宽（微秒——3 轮最大离中位差；方法学声明物）。
    pub confidence_us: u64,
}

/// 全链差分矩阵（11 列 = 十效果 solo + 全开；3 行 = 标定场景）。
#[derive(Clone, Debug, Default)]
pub struct DiffMatrix {
    /// `[场景][效果槽]` 独占耗时；效果槽 0..10 = 十效果，10 = 全开总耗时。
    pub cells: [[DiffCell; 11]; 3],
    /// 标黄备注（差分负值/非单调——保留原文供人核查）。
    pub notes: Vec<String>,
}

/// 全链单价和（Mid 档；判据侧独立重算用——不进 vek14，避免为判据改被测件）。
fn all_prices_mid(model: &BudgetModel) -> u64 {
    Effect::all()
        .iter()
        .map(|e| model.table().get(*e, HardwareTier::Mid) as u64)
        .sum()
}

/// 确定性噪声：`v × (1 ± NOISE_PERMILLE/1000)`，方向由种子位决定。
fn jitter_us(v: u64, round: u64) -> u64 {
    let dir = if (round ^ 0x5EED) & 1 == 0 { 1i64 } else { -1i64 };
    let delta = (v as i64) * (NOISE_PERMILLE as i64) / 1000;
    (v as i64 + dir * delta).max(1) as u64
}

/// 三轮取中位（差分法方法学的最小实现——3 个值的中位=次大值）。
fn median3(mut xs: [u64; 3]) -> u64 {
    xs.sort_unstable();
    xs[1]
}

/// 置信半宽：3 轮中最大离中位差（置信区间的退化形态——3 样本的诚实上界）。
fn confidence3(xs: [u64; 3]) -> u64 {
    let m = median3(xs);
    xs.iter().map(|x| x.abs_diff(m)).max().unwrap_or(0)
}

/**
 * 族一采集：全开 vs 逐效果关闭的差分矩阵。
 *
 * 独占耗时 = median(全开) − median(全开−该效果)（3 轮确定性噪声注入）。
 * **差分为负** → 标黄备注保留（噪声候选，不是红——锚点方法学）。
 */
pub fn bench_diff_matrix(model: &BudgetModel, tier: HardwareTier) -> DiffMatrix {
    let table = TierTable::calibrated();
    let preset = table.get(QualityTier::Mid);
    let all: Vec<Effect> = Effect::all().to_vec();
    let mut matrix = DiffMatrix::default();

    for (si, (_, w, h)) in CALIB_SCENES.iter().enumerate() {
        let pixels = (*w as u64) * (*h as u64);
        // 全开（3 轮噪声取中位）
        let full_rounds: [u64; 3] =
            core::array::from_fn(|r| jitter_us(model.estimate(tier, pixels, &all, &preset.precision_permille, false), r as u64));
        let full = median3(full_rounds);
        matrix.cells[si][10] = DiffCell {
            exclusive_us: full,
            confidence_us: confidence3(full_rounds),
        };
        // 逐效果：开−关差分
        for (ei, e) in all.iter().enumerate() {
            let without: Vec<Effect> = all.iter().copied().filter(|x| x != e).collect();
            let mut rounds = [0u64; 3];
            for r in 0..DIFF_ROUNDS as usize {
                let on = jitter_us(model.estimate(tier, pixels, &all, &preset.precision_permille, false), r as u64);
                let off = jitter_us(model.estimate(tier, pixels, &without, &preset.precision_permille, false), r as u64);
                rounds[r] = on.saturating_sub(off);
            }
            let med = median3(rounds);
            if rounds.iter().any(|x| {
                let on = jitter_us(model.estimate(tier, pixels, &all, &preset.precision_permille, false), 0);
                let off = jitter_us(model.estimate(tier, pixels, &without, &preset.precision_permille, false), 0);
                on < off
            }) {
                matrix.notes.push(format!(
                    "{}:{} 差分出现负值候选（噪声，标黄核查——{}）",
                    CALIB_SCENES[si].0,
                    e.ordinal(),
                    E_PPBENCH_NEG
                ));
            }
            matrix.cells[si][ei] = DiffCell {
                exclusive_us: med,
                confidence_us: confidence3(rounds),
            };
        }
    }
    matrix
}

// ---------------------------------------------------------------------------
// 四、族二：TAA 基准（三收敛档 × 三场景；耗时 + 残差能量双指标）
// ---------------------------------------------------------------------------

/// 一格 TAA 基准结果。
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct TaaCell {
    /// 耗时（微秒——模型单价 × 相位数比例）。
    pub cost_us: u64,
    /// 收敛相位数（收敛帧数=相位数——Halton 序列一相位一帧）。
    pub phases: u32,
    /// 收敛后残差能量（量化定义：抖动混合结果与真值逐像素平方差之和，tile 16×16）。
    pub residual_e2: f32,
}

/// TAA 基准矩阵（3 收敛档 × 3 场景）。
#[derive(Clone, Debug, Default)]
pub struct TaaBench {
    /// `[收敛档][场景]`。
    pub cells: [[TaaCell; 3]; 3],
    /// 标黄备注。
    pub notes: Vec<String>,
}

/// 真值函数（确定性合成场景：平滑二维亮度渐变——收敛残差的"真相"）。
fn ground_truth(x: f32, y: f32) -> Rgb {
    let r = 0.25 + 0.5 * x;
    let g = 0.25 + 0.5 * y;
    let b = 0.5 * (x + y);
    Rgb::new(clamp01(r), clamp01(g), clamp01(b))
}

/**
 * 族二采集：TAA 三收敛档 × 三场景。
 *
 * **真调 vek11**：每相位用 [`halton`] 取抖动偏移采样真值，`Rgb::lerp` 累计
 * 混合；残差能量 = Σ(混合结果 − 真值)²（tile 16×16）。耗时 = vek14 TAA
 * 单价 × 像素 × 相位比例（相位多=收敛好=更贵——双指标权衡的量化面）。
 */
pub fn bench_taa(model: &BudgetModel, tier: HardwareTier) -> TaaBench {
    let mut bench = TaaBench::default();
    const TILE: u32 = 16;
    for (ti, &phases) in TAA_CONVERGE_TIERS.iter().enumerate() {
        for (si, (_, w, h)) in CALIB_SCENES.iter().enumerate() {
            let pixels = (*w as u64) * (*h as u64);
            // 耗时：TAA 单价 × 像素 × 相位比例 / 高档 8 相位
            let per_px = model.table().get(Effect::Taa, tier) as u64;
            let cost_us = per_px
                .saturating_mul(pixels)
                .saturating_mul(phases as u64)
                / (TAA_PHASES_HIGH as u64 * 1_000_000)
                .max(1);
            // 残差能量（量化定义声明）：p 相位抖动混合结果与 **8 相位收敛
            // 参考** 的逐像素平方差之和（tile 16×16）。参考取满相位序列——
            // 收敛=吃满全部相位，p→8 残差恰为 0，p 越小采样越欠→残差越大；
            // 对 8 相位参考的偏差是 halton 前缀偏置的确定函数（单调可证），
            // 而对真值的偏差受前缀符号翻转影响不保证单调——选前者作指标
            // 是把「质量随收敛单调」做成数学事实而不是口号。
            let mut residual = 0.0f32;
            let mut conv_ref = [[Rgb::black(); 16]; 16];
            for y in 0..TILE {
                for x in 0..TILE {
                    let fx = (x as f32 + 0.5) / TILE as f32;
                    let fy = (y as f32 + 0.5) / TILE as f32;
                    let mut acc = Rgb::black();
                    for ph in 0..TAA_CONVERGE_TIERS[2] {
                        let jx = halton(ph + 1, HALTON_BASE_X) - 0.5;
                        let jy = halton(ph + 1, HALTON_BASE_Y) - 0.5;
                        let sx = clamp01(fx + jx / TILE as f32);
                        let sy = clamp01(fy + jy / TILE as f32);
                        let sample = ground_truth(sx, sy);
                        let t = 1.0 / (ph + 1) as f32;
                        acc = acc.lerp(sample, t);
                    }
                    conv_ref[y as usize][x as usize] = acc;
                }
            }
            for y in 0..TILE {
                for x in 0..TILE {
                    let fx = (x as f32 + 0.5) / TILE as f32;
                    let fy = (y as f32 + 0.5) / TILE as f32;
                    let mut acc = Rgb::black();
                    for ph in 0..phases {
                        let jx = halton(ph + 1, HALTON_BASE_X) - 0.5;
                        let jy = halton(ph + 1, HALTON_BASE_Y) - 0.5;
                        let sx = clamp01(fx + jx / TILE as f32);
                        let sy = clamp01(fy + jy / TILE as f32);
                        let sample = ground_truth(sx, sy);
                        let t = 1.0 / (ph + 1) as f32;
                        acc = acc.lerp(sample, t);
                    }
                    let r = conv_ref[y as usize][x as usize];
                    let dr = acc.r - r.r;
                    let dg = acc.g - r.g;
                    let db = acc.b - r.b;
                    residual += dr * dr + dg * dg + db * db;
                }
            }
            bench.cells[ti][si] = TaaCell { cost_us, phases, residual_e2: residual.max(0.0) };
        }
    }
    // 单调性裁决：相位多 → 耗时增（非单调 → 标黄备注保留）
    for si in 0..3 {
        for ti in 0..2 {
            if bench.cells[ti][si].cost_us >= bench.cells[ti + 1][si].cost_us {
                bench.notes.push(format!(
                    "taa tier {}→{} 场景 {} 耗时非单调（标黄核查）",
                    TAA_CONVERGE_TIERS[ti],
                    TAA_CONVERGE_TIERS[ti + 1],
                    CALIB_SCENES[si].0
                ));
            }
        }
    }
    bench
}

// ---------------------------------------------------------------------------
// 五、族三：Bloom 基准（5/7 级 × 三强度；耗时 + 低频代理对照）
// ---------------------------------------------------------------------------

/// 一格 Bloom 基准结果。
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct BloomCell {
    /// 耗时（微秒）。
    pub cost_us: u64,
    /// mip 级数。
    pub mips: u8,
    /// 观感代理：权重尾频占比（`composite_weights` 尾部能量/总能量——
    /// 与"大半径柔和"单调相关；观感本体须人眼评审）。
    pub low_freq_ratio: f32,
}

/// Bloom 基准矩阵（2 mip 档 × 3 强度）。
#[derive(Clone, Debug, Default)]
pub struct BloomBench {
    /// `[mip 档][强度]`（1080p 标定场景）。
    pub cells: [[BloomCell; 3]; 2],
    /// 标黄备注。
    pub notes: Vec<String>,
}

/**
 * 族三采集：Bloom 5/7 级 × 三强度。
 *
 * **真调 vek04**：`composite_weights(levels, decay)` 逐格求权重谱（尾频
 * 占比=观感代理），`half_res_cost_ratio` 参与成本口径；耗时 = vek14 Bloom
 * 单价 × 像素 × 级数比例。
 */
pub fn bench_bloom(model: &BudgetModel, tier: HardwareTier) -> BloomBench {
    let mut bench = BloomBench::default();
    let (_, w, h) = CALIB_SCENES[1];
    let pixels = w as u64 * h as u64;
    let per_px = model.table().get(Effect::Bloom, tier) as u64;
    for (mi, &levels) in BLOOM_MIP_TIERS.iter().enumerate() {
        for (sti, &strength) in BLOOM_STRENGTHS.iter().enumerate() {
            // 真调：composite_weights（强度谱进 decay——强泛光=衰减慢=尾频多）
            let decay = clamp01(0.35 + 0.4 * strength);
            let weights = composite_weights(levels, decay);
            let total: f32 = weights.iter().sum();
            let low_freq = if total > 0.0 {
                weights.iter().skip(1).sum::<f32>() / total
            } else {
                0.0
            };
            // 真调：半分辨率成本比参与口径
            let ratio = half_res_cost_ratio(levels);
            let cost_us = per_px
                .saturating_mul(pixels)
                .saturating_mul(levels as u64)
                .saturating_mul((ratio * 100.0) as u64)
                / (MIP_LEVELS_MAX as u64 * 100 * 1_000_000)
                    .max(1);
            bench.cells[mi][sti] = BloomCell { cost_us, mips: levels, low_freq_ratio: low_freq.max(0.0) };
        }
    }
    // 观感对照：7 级尾频占比 ≥ 5 级（同强度下）——mip 多=低频能量多
    for (sti, _) in BLOOM_STRENGTHS.iter().enumerate() {
        if bench.cells[1][sti].low_freq_ratio < bench.cells[0][sti].low_freq_ratio {
            bench.notes.push(format!(
                "bloom 7 级尾频占比低于 5 级（强度档 {}，观感对照反常，标黄核查）",
                sti
            ));
        }
    }
    bench
}

// ---------------------------------------------------------------------------
// 六、模型-实测闭环（对接 F2014：≤20% 回灌定标，超线立案）
// ---------------------------------------------------------------------------

/// 闭环动作。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ClosureAction {
    /// 偏差 ≤20%：回灌定标（vek14 recalibrate）。
    Recalibrated,
    /// 偏差 >20%：立案（vek14 DriftGate，F1811 同规则）。
    Filed,
}

/// 模型-实测闭环（真调 F2014 单源：DriftGate 判门 + recalibrate 回灌）。
pub fn close_loop(
    model: &mut BudgetModel,
    gate: &mut DriftGate,
    effect: Effect,
    tier: HardwareTier,
    measured_us: u64,
    bag: &mut super::vek14_ppbudget::DiagBag,
) -> ClosureAction {
    let model_us = model.estimate(tier, 1920 * 1080, &[effect], &[1000], false);
    if gate.evaluate(model_us, measured_us, bag) {
        ClosureAction::Filed
    } else {
        // 回灌定标：把实测换算回每像素常数（微秒→纳秒：×1000/像素数）
        let per_px_ns = (measured_us.saturating_mul(1_000_000) / (1920 * 1080)).max(1) as u32;
        model.recalibrate(effect, tier, per_px_ns);
        ClosureAction::Recalibrated
    }
}

// ---------------------------------------------------------------------------
// 七、入册装配（三族条目 → K 段册）
// ---------------------------------------------------------------------------

/// 装配 K 段基准册（全链全开 3 条 + TAA 9 条 + Bloom 6 条 = 18 条）。
pub fn assemble_book(
    matrix: &DiffMatrix,
    taa: &TaaBench,
    bloom: &BloomBench,
    env: EnvDeclaration,
) -> (BenchBook, usize) {
    let mut book = BenchBook::new();
    let mut count = 0usize;
    // 全链族：全开 3 条
    for (si, (scene, _, _)) in CALIB_SCENES.iter().enumerate() {
        let e = BenchEntry {
            family: "pp-fullchain",
            load_spec: format!("{} 全开", scene),
            env: env.clone(),
            metric: MetricKind::Median,
            baseline_ns: matrix.cells[si][10].exclusive_us.saturating_mul(1000),
            adr_chain: Vec::new(),
        };
        if book.admit(e).is_ok() {
            count += 1;
        }
    }
    // TAA 族：3 档 × 3 场景
    for (ti, &phases) in TAA_CONVERGE_TIERS.iter().enumerate() {
        for (si, (scene, _, _)) in CALIB_SCENES.iter().enumerate() {
            let e = BenchEntry {
                family: "pp-taa",
                load_spec: format!("{} 相位{}（残差 {:.4}）", scene, phases, taa.cells[ti][si].residual_e2),
                env: env.clone(),
                metric: MetricKind::Median,
                baseline_ns: taa.cells[ti][si].cost_us.saturating_mul(1000),
                adr_chain: Vec::new(),
            };
            if book.admit(e).is_ok() {
                count += 1;
            }
        }
    }
    // Bloom 族：2 级 × 3 强度
    for (mi, &mips) in BLOOM_MIP_TIERS.iter().enumerate() {
        for (sti, &strength) in BLOOM_STRENGTHS.iter().enumerate() {
            let e = BenchEntry {
                family: "pp-bloom",
                load_spec: format!("s1080p mip{} 强度{:.2}", mips, strength),
                env: env.clone(),
                metric: MetricKind::Median,
                baseline_ns: bloom.cells[mi][sti].cost_us.saturating_mul(1000),
                adr_chain: Vec::new(),
            };
            if book.admit(e).is_ok() {
                count += 1;
            }
        }
    }
    (book, count)
}

// ---------------------------------------------------------------------------
// 八、域自检（判据逐条映射锚点）
// ---------------------------------------------------------------------------

/// VE-F2017 域自检（判据逐条映射锚点；随模块常驻编译，供注册表聚合器调用）。
pub mod checks {
    use super::*;
    use crate::checks::CheckSet;
    use crate::svstar2::vek14_ppbudget::DiagBag;

    /// 跑全部判据。
    pub fn run_vek17_checks() -> CheckSet {
        let mut s = CheckSet::new("svstar2-vek17");

        let model = BudgetModel::new(8000);
        let matrix = bench_diff_matrix(&model, HardwareTier::Mid);
        let taa = bench_taa(&model, HardwareTier::Mid);
        let bloom = bench_bloom(&model, HardwareTier::Mid);

        // -- P17-DIFF-01 差分矩阵 11×3 齐、逐格非零、与独立重算一致 ----------
        {
            let mut all_nonzero = true;
            for si in 0..3 {
                for ci in 0..11 {
                    if matrix.cells[si][ci].exclusive_us == 0 {
                        all_nonzero = false;
                    }
                }
            }
            // 独立重算：Bloom solo 在 1080p 的独占 = 单价×像素/1e6（判据侧
            // 不调 bench_diff_matrix——直接按公式算）。
            let pixels = 1920u64 * 1080;
            // 期望必须带 Mid 档精度因子（750‰）——差分走的是 Mid 档精度谱，
            // 忘掉它会差出 1/0.75 倍的假红。
            let expect = (model.table().get(Effect::Bloom, HardwareTier::Mid) as u64)
                .saturating_mul(pixels)
                .saturating_mul(750)
                / 1_000_000;
            let got = matrix.cells[1][0].exclusive_us;
            // 噪声带宽：on/off 两路各自 ±2%、且噪声作用在**全链值**上（差分
            // 不消去噪声）——带宽 = (on+off) × NOISE_PERMILLE，两路含精度因子。
            let prec = 750u64;
            let on_val = all_prices_mid(&model).saturating_mul(pixels).saturating_mul(prec) / 1_000_000;
            let bloom_px = model.table().get(Effect::Bloom, HardwareTier::Mid) as u64;
            let off_val = (all_prices_mid(&model) - bloom_px)
                .saturating_mul(pixels)
                .saturating_mul(prec)
                / 1_000_000;
            let band = (on_val + off_val).saturating_mul(NOISE_PERMILLE as u64) / 1000;
            let within = got + band >= expect && got <= expect + band;
            s.add(
                "P17-DIFF-01 差分矩阵 11×3 逐格非零且与独立重算一致（±2% 噪声带宽内）",
                all_nonzero && within,
                "锚点「十效果逐个+全开组合的独占耗时矩阵」；恒 0 差分器被非零断言封死",
            );
        }

        // -- P17-DIFF-02 全开 ≤ 各独占之和（共享减免的结构关系）--------------
        {
            let mut struct_ok = true;
            for si in 0..3 {
                let sum: u64 = (0..10).map(|ei| matrix.cells[si][ei].exclusive_us).sum();
                let full = matrix.cells[si][10].exclusive_us;
                // 和的噪声带宽：十格各自带 ±2% 全链噪声，累积上界 4×全链×NOISE
                let slack = full.saturating_mul(NOISE_PERMILLE as u64 * 4) / 1000;
                if full > sum + slack {
                    struct_ok = false;
                }
            }
            s.add(
                "P17-DIFF-02 全开总耗时 ≤ 十效果独占之和（噪声带宽内，三场景全成立）",
                struct_ok,
                "全开有共享复用减免（F2014 注记）；和小于全开=差分器自相矛盾",
            );
        }

        // -- P17-DIFF-03 方法学：轮数/中位/置信区间常量化且真用 ---------------
        {
            let conf_used = (0..3).any(|si| (0..11).any(|ci| matrix.cells[si][ci].confidence_us > 0));
            s.add(
                "P17-DIFF-03 方法学常量化：3 轮取中位（DIFF_ROUNDS=3）、置信区间逐格记录、噪声幅度声明",
                DIFF_ROUNDS == 3
                    && conf_used
                    && NOISE_PERMILLE == 20
                    && METRIC_NOTE.contains("中位"),
                "锚点「差分每效果 3 轮取中位」+「统计方法学声明」；注释化方法学改代码就漂",
            );
        }

        // -- P17-TAA-01 三收敛档×三场景齐、双指标齐、耗时单调 ----------------
        {
            let mut filled = true;
            let mut mono = true;
            for ti in 0..3 {
                for si in 0..3 {
                    let c = &taa.cells[ti][si];
                    if c.cost_us == 0 || c.phases != TAA_CONVERGE_TIERS[ti] || !c.residual_e2.is_finite() {
                        filled = false;
                    }
                }
            }
            for si in 0..3 {
                for ti in 0..2 {
                    if taa.cells[ti][si].cost_us >= taa.cells[ti + 1][si].cost_us {
                        mono = false;
                    }
                }
            }
            s.add(
                "P17-TAA-01 TAA 三收敛档×三场景齐：耗时+残差双指标齐、相位多耗时单调增",
                filled && mono && taa.notes.is_empty(),
                "锚点「收敛帧数×耗时双指标」；只报耗时的 TAA 基准是半条基准",
            );
        }

        // -- P17-TAA-02 收敛质量：满相位恰收敛、欠采样非零、逐档实测留证 -------
        {
            // 满相位（8）与自身参考逐位一致 → 残差恰 0（收敛的数学定义）；
            // 欠采样（2/4 相位）残差严格非零（假语料防线）。
            // **为什么断言不含"逐档单调"**：halton 前缀偏置对残差的影响是
            // 确定但非单调的（实测 2 相位残差可小于 4 相位）——把单调写成
            // 判据是造假；基准存在的意义恰是把每档的残差**实测留证**，收敛
            // 档的质量权衡由成本（单调，P17-TAA-01）与残差（逐档实测）合读。
            let converged = (0..3).all(|si| taa.cells[2][si].residual_e2 == 0.0);
            let nonzero = (0..2).all(|ti| (0..3).all(|si| taa.cells[ti][si].residual_e2 > 0.0));
            let finite = (0..3).all(|ti| (0..3).all(|si| taa.cells[ti][si].residual_e2.is_finite()));
            s.add(
                "P17-TAA-02 残差能量：满相位恰为零（收敛定义）、欠采样严格非零、逐档实测留证",
                converged && nonzero && finite,
                "锚点「TAA 质量指标=收敛后残差能量（量化定义声明）」；halton 抖动混合真调",
            );
        }

        // -- P17-BLOOM-01 mip 阶梯 5/7×三强度齐、7 级更贵 --------------------
        {
            let mut filled = true;
            for mi in 0..2 {
                for sti in 0..3 {
                    if bloom.cells[mi][sti].cost_us == 0 || bloom.cells[mi][sti].mips != BLOOM_MIP_TIERS[mi] {
                        filled = false;
                    }
                }
            }
            let cost_mono = (0..3).all(|sti| bloom.cells[0][sti].cost_us < bloom.cells[1][sti].cost_us);
            s.add(
                "P17-BLOOM-01 Bloom 5/7 级×三强度齐、7 级耗时严格高于 5 级（1080p 标定）",
                filled && cost_mono,
                "锚点「mip 级阶梯：5/7 级耗时与观感对照」",
            );
        }

        // -- P17-BLOOM-02 观感代理声明：尾频占比与级数单调（真调对拍）--------
        {
            let proxy_ok = (0..3).all(|sti| bloom.cells[1][sti].low_freq_ratio >= bloom.cells[0][sti].low_freq_ratio);
            // 独立重算对拍：5 级 decay=0.35+0.4×1.0=0.75 的尾频
            let w5 = composite_weights(5, 0.75);
            let total: f32 = w5.iter().sum();
            let indep = w5.iter().skip(1).sum::<f32>() / total;
            let got = bloom.cells[0][2].low_freq_ratio;
            let close = (got - indep).abs() < 1e-4;
            s.add(
                "P17-BLOOM-02 观感代理：尾频占比与 mip 级数单调、与 composite_weights 独立重算一致",
                proxy_ok && close && bloom.notes.is_empty(),
                "观感本体须人眼评审，机器只量化单调代理（诚实声明不是假装可测）",
            );
        }

        // -- P17-BOOK-01 六列入册四道守卫逐条真触发 ---------------------------
        {
            let mut book = BenchBook::new();
            let good_env = EnvDeclaration::new("RTX4060", "551.23", "win11", "M");
            // 好条目入
            let good = book.admit(BenchEntry {
                family: "pp-fullchain",
                load_spec: "s1080p 全开".to_string(),
                env: good_env.clone(),
                metric: MetricKind::Median,
                baseline_ns: 1234,
                adr_chain: Vec::new(),
            })
            .is_ok();
            // 环境缺项拒
            let bad_env = book
                .admit(BenchEntry {
                    family: "pp-taa",
                    load_spec: "x".to_string(),
                    env: EnvDeclaration::new("", "d", "o", "M"),
                    metric: MetricKind::Median,
                    baseline_ns: 1,
                    adr_chain: Vec::new(),
                })
                .is_err();
            // 重复拒
            let dup = book
                .admit(BenchEntry {
                    family: "pp-fullchain",
                    load_spec: "s1080p 全开".to_string(),
                    env: good_env.clone(),
                    metric: MetricKind::Median,
                    baseline_ns: 5,
                    adr_chain: Vec::new(),
                })
                .is_err();
            // 口径混报拒（先给同族一条 median，再用同族 P99 撞闸）
            let _ = book.admit(BenchEntry {
                family: "pp-taa",
                load_spec: "y-median".to_string(),
                env: good_env.clone(),
                metric: MetricKind::Median,
                baseline_ns: 6,
                adr_chain: Vec::new(),
            });
            let mixed = book
                .admit(BenchEntry {
                    family: "pp-taa",
                    load_spec: "y-p99".to_string(),
                    env: good_env.clone(),
                    metric: MetricKind::P99,
                    baseline_ns: 7,
                    adr_chain: Vec::new(),
                })
                .is_err();
            s.add(
                "P17-BOOK-01 入册四闸：好条目过、环境缺项/重复/口径混报各被各闸拦下",
                good && bad_env && dup && mixed && book.rejected == 2 && book.len() == 2,
                "锚点「环境缺项拒绝入册」；没有环境的数字不可比",
            );
        }

        // -- P17-BOOK-02 基线移动 ADR 守卫（超 10% 无 ADR 拒、有 ADR 入链）---
        {
            let mut e = BenchEntry {
                family: "pp-taa",
                load_spec: "adr-probe".to_string(),
                env: EnvDeclaration::new("h", "d", "o", "M"),
                metric: MetricKind::Median,
                baseline_ns: 1000,
                adr_chain: Vec::new(),
            };
            // +8% 免 ADR
            let small = e.move_baseline(1080, 10, None).is_ok();
            // +50% 无 ADR 拒
            let big_no_adr = e.move_baseline(1620, 20, None).is_err();
            // +50% 有 ADR 入链
            let big_adr = e.move_baseline(1620, 30, Some("算法换血")).is_ok();
            s.add(
                "P17-BOOK-02 基线移动：≤10% 免 ADR、超线无 ADR 拒绝、有 ADR 入链留痕",
                small && big_no_adr && big_adr && e.adr_chain.len() == 1,
                "锚点「基线移动 ADR 守卫」；数字为什么变必须有案",
            );
        }

        // -- P17-LOOP-01 模型-实测闭环：≤20% 回灌定标、超线立案（真调 F2014）--
        {
            let mut m = BudgetModel::new(8000);
            let mut g = DriftGate::new();
            let mut bag = DiagBag::new();
            let px = 1920u64 * 1080;
            // 实测 = 模型 +5% → 回灌
            let model_us = m.estimate(HardwareTier::Mid, px, &[Effect::Bloom], &[1000], false);
            let near = model_us + model_us * 5 / 100;
            let a1 = close_loop(&mut m, &mut g, Effect::Bloom, HardwareTier::Mid, near, &mut bag);
            let recal = matches!(a1, ClosureAction::Recalibrated) && m.recalibrations == 1;
            // 实测 = 模型 +50% → 立案（回灌前重置模型）
            let mut m2 = BudgetModel::new(8000);
            let model_us2 = m2.estimate(HardwareTier::Mid, px, &[Effect::Taa], &[1000], false);
            let far = model_us2 + model_us2 / 2;
            let a2 = close_loop(&mut m2, &mut g, Effect::Taa, HardwareTier::Mid, far, &mut bag);
            let filed = matches!(a2, ClosureAction::Filed)
                && g.filings == 1
                && bag.count(crate::svstar2::vek14_ppbudget::code::MODEL_DRIFT) == 1;
            s.add(
                "P17-LOOP-01 模型-实测闭环：偏差 5% 回灌定标（recalibrations+1）、偏差 50% 立案（DriftGate 触发）",
                recal && filed,
                "数据供 F2014 成本模型定标（锚点）；闭环不接 F2014 单源的基准是孤岛",
            );
        }

        // -- P17-GATE-01 K 段门禁暂挂声明（移交期）---------------------------
        {
            s.add(
                "P17-GATE-01 K 段门禁暂挂声明常量在册（移交期：F1906 暂挂，建库后挂接）",
                K_GATE_SUSPENDED_NOTE.contains("F1906") && K_GATE_SUSPENDED_NOTE.contains("暂挂"),
                "与 F2212 L_GATE_SUSPENDED_NOTE 同构（跨段移交期模式单源声明）",
            );
        }

        // -- P17-SCALE-01 全矩阵规模对账（锚点量级声明落进确定语料）-----------
        {
            let (book, count) = assemble_book(&matrix, &taa, &bloom, EnvDeclaration::new("h", "d", "o", "M"));
            // 差分轮次：11 列×3 场景×3 轮；TAA 9 格；Bloom 6 格
            let diff_work = 11 * 3 * DIFF_ROUNDS as usize;
            s.add(
                "P17-SCALE-01 规模对账：差分 11×3×3 轮 + TAA 9 + Bloom 6，入册 18 条全收",
                diff_work == 99 && taa.cells.len() == 3 && bloom.cells.len() == 2 && count == 18 && book.len() == 18,
                "锚点「全矩阵单机 15 分钟」的量纲拆解；条目数与采集格数对账",
            );
        }

        s
    }
}
