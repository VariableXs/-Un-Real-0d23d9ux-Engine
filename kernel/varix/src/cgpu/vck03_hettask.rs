//! vck03_hettask — CGPU-F1603 异构任务模型与标签（B 域 Task 的异构扩展层）。
//!
//! 判据原文（施工书锚点 CGPU-F1603）：任务模型：异构任务扩展（B 域 Task 扩展
//! 异构字段（目标引擎/资源需求/引擎特定参数（模型 ID/任务粒度）——字段
//! schema 版本化——B 域结构扩展不改内核契约声明）；功耗标签复用（F1458 三维
//! 标签（功耗等级/可延迟/可合并）跨引擎适用——NPU 任务标签（NPU 功耗系数
//! 表）——标签跨引擎统一）；引擎亲和性（任务声明引擎偏好（NPU-only/prefer-
//! NPU/any——亲和级别（亲和语义：any 时调度器按能效选引擎）；批处理任务
//! （NPU 批推理任务模型（批维度参数）——批任务类型；实时约束（DSP 任务
//! deadline 字段——实时标签——deadline 语义（错过=失败（不静默）；任务校验
//! （引擎能力×任务需求匹配校验（提交前——校验规则表。判据：扩展不改核、标
//! 签统一、亲和三级、批模型、deadline、校验、六组。
//!
//! 职责定位：异构任务的结构扩展+三级亲和+三维功耗标签+批维度+实时约束+
//! 提交前校验规则表。能力位图与引擎类型声明式复用 vck02_hetres（F1602）：
//! FEAT_BATCH/FEAT_REALTIME/EngineKind——本模块不重定义引擎能力口径。扩展
//! 不改核：全部字段挂 HetTaskExt 侧车结构，B 域 Task 本体零改动（判据"扩展
//! 不改核"的结构化兑现）。能效选路表独立写死可对拍。诊断码段 0x41xx。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

// 引擎能力口径：声明式复用 vck02_hetres（F1602）——单一事实源。
use crate::cgpu::vck02_hetres::{EngineKind, FEAT_BATCH, FEAT_REALTIME};

/// 模块版本（schema 版本化标识——判据"字段 schema 版本化"）。
pub const HET_TASK_SCHEMA: &str = "K03-hettask-v1";

/// schema 支持集（任务声明更高版本即拒——版本协商闭集）。
pub const SUPPORTED_SCHEMA_MINOR: u32 = 1;

/// 功耗等级闭集（F1458 三维标签之一——跨引擎统一语义）。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum PwrLevel {
    /// 低功耗（能效优先）。
    Saver,
    /// 平衡。
    Balanced,
    /// 性能。
    Performance,
    /// 短时冲刺（热预算允许的窗口档）。
    Boost,
}

impl PwrLevel {
    pub const ALL: [PwrLevel; 4] = [
        PwrLevel::Saver,
        PwrLevel::Balanced,
        PwrLevel::Performance,
        PwrLevel::Boost,
    ];

    pub const fn ordinal(self) -> usize {
        match self {
            PwrLevel::Saver => 0,
            PwrLevel::Balanced => 1,
            PwrLevel::Performance => 2,
            PwrLevel::Boost => 3,
        }
    }

    pub const fn zh(self) -> &'static str {
        match self {
            PwrLevel::Saver => "低功耗",
            PwrLevel::Balanced => "平衡",
            PwrLevel::Performance => "性能",
            PwrLevel::Boost => "冲刺",
        }
    }
}

/// NPU 功耗系数表（mW/千算子，四档×静态写死——判据"NPU 功耗系数表"。
/// 数字为工程设定基线，跨版本改动走台账对账）。
pub const NPU_PWR_COEFF_MW: [u32; 4] = [180, 450, 900, 1500];

/// 亲和三级闭集（判据"NPU-only/prefer-NPU/any——亲和级别"）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Affinity {
    /// 仅 NPU（不可换引擎）。
    NpuOnly,
    /// 偏好 NPU（NPU 不可用时按能效序降级，降级留痕）。
    PreferNpu,
    /// 任意（调度器按能效选引擎——判据亲和语义）。
    Any,
}

impl Affinity {
    pub const fn zh(self) -> &'static str {
        match self {
            Affinity::NpuOnly => "仅NPU",
            Affinity::PreferNpu => "偏好NPU",
            Affinity::Any => "任意(按能效)",
        }
    }
}

/// 能效选路序（any 时的静态能效序：NPU>DSP>MEDIA>ISP。
/// 依据：NPU 专化算子密度最高，DSP 次之，MEDIA 固定功能流水能效中档，
/// ISP 面向像素流非推理负载——序为工程设定，跨版本改动走台账）。
pub const EFFICIENCY_ORDER: [EngineKind; 4] = [
    EngineKind::Npu,
    EngineKind::Dsp,
    EngineKind::Media,
    EngineKind::Isp,
];

/// 诊断码（0x41xx 段独占）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum XcCode {
    /// 0x4101 schema 版本不支持（协商闭集外）。
    HtSchemaUnsupported,
    /// 0x4102 亲和不可满足（NpuOnly 但目标引擎集无 NPU）。
    HtAffinityUnsat,
    /// 0x4103 批任务但引擎无批能力（FEAT_BATCH 缺失）。
    HtBatchCapMiss,
    /// 0x4104 实时任务但引擎无实时能力（FEAT_REALTIME 缺失）。
    HtRealtimeCapMiss,
    /// 0x4105 deadline 已过（错过=失败——不静默，显性失败回执）。
    HtDeadlinePassed,
    /// 0x4106 功耗标签非法（NPU 系数表外档位组合）。
    HtLabelInvalid,
    /// 0x4107 任务粒度为零（粒度=最小可切分算子数，零=不可调度）。
    HtGranZero,
    /// 0x4108 推理任务缺模型 ID（引擎特定参数必带项）。
    HtModelMissing,
}

impl XcCode {
    /// 诊断详情（闭集逐码中文说明）。
    pub const fn detail(self) -> &'static str {
        match self {
            XcCode::HtSchemaUnsupported => "任务 schema 版本不支持：协商闭集外，升级或降版重提",
            XcCode::HtAffinityUnsat => "亲和不可满足：NpuOnly 但引擎集无 NPU，任务拒绝入队",
            XcCode::HtBatchCapMiss => "批任务落到了无批能力的引擎：FEAT_BATCH 缺失",
            XcCode::HtRealtimeCapMiss => "实时任务落到了无实时能力的引擎：FEAT_REALTIME 缺失",
            XcCode::HtDeadlinePassed => "deadline 已过：错过即失败，显性回执不静默吞单",
            XcCode::HtLabelInvalid => "功耗标签非法：档位闭集外或系数表无此组合",
            XcCode::HtGranZero => "任务粒度为零：最小可切分算子数必须非零",
            XcCode::HtModelMissing => "推理任务缺模型 ID：引擎特定参数必带项",
        }
    }
}

/// 三维功耗标签（F1458 复用声明：功耗等级/可延迟/可合并——跨引擎统一）。
#[derive(Clone, Copy, Debug)]
pub struct PwrLabel {
    /// 功耗等级。
    pub level: PwrLevel,
    /// 可延迟（低峰再跑）。
    pub delayable: bool,
    /// 可合并（与同类任务合并执行）。
    pub mergeable: bool,
}

impl PwrLabel {
    /// NPU 口径功耗系数（mW/千算子——判据"NPU 功耗系数表"查询口）。
    pub const fn npu_coeff_mw(&self) -> u32 {
        NPU_PWR_COEFF_MW[self.level.ordinal()]
    }

    /// 标签合法性：Boost 档不可与可延迟同时声明（冲刺档语义即立刻跑）。
    /// const fn 禁 PartialEq 的 ==——matches! 表驱动比较。
    pub const fn valid(&self) -> bool {
        !matches!(self.level, PwrLevel::Boost) || !self.delayable
    }
}

/// 批维度参数（判据"NPU 批推理任务模型（批维度参数）"）。
#[derive(Clone, Copy, Debug)]
pub struct BatchDim {
    /// 批大小（一次提交的样本数）。
    pub batch: u32,
    /// 微批（引擎单次硬件执行的内层批）。
    pub micro_batch: u32,
}

impl BatchDim {
    /// 合法性：两者非零，且 micro_batch ≤ batch（微批大于整批=配置倒置）。
    pub const fn valid(&self) -> bool {
        self.batch > 0 && self.micro_batch > 0 && self.micro_batch <= self.batch
    }

    /// 外层步数（batch / micro_batch，向上取整——调度切分口径）。
    pub const fn outer_steps(&self) -> u32 {
        (self.batch + self.micro_batch - 1) / self.micro_batch
    }
}

/// 异构任务扩展（B 域 Task 侧车——判据"扩展不改核"的结构形态）。
#[derive(Clone, Debug)]
pub struct HetTaskExt {
    /// schema 次版本（主版本由模块版本承载）。
    pub schema_minor: u32,
    /// 亲和级别。
    pub affinity: Affinity,
    /// 功耗标签（三维）。
    pub label: PwrLabel,
    /// 资源需求（抽象算子数——引擎无关口径，调度器换算）。
    pub ops_required: u64,
    /// 引擎特定参数：模型 ID（推理任务必带）。
    pub model_id: Option<u32>,
    /// 引擎特定参数：任务粒度（最小可切分算子数）。
    pub granularity_ops: u64,
    /// 批维度（None=非批任务）。
    pub batch: Option<BatchDim>,
    /// 实时约束：deadline（µs 绝对时间轴；None=非实时）。
    pub deadline_us: Option<u64>,
}

impl HetTaskExt {
    /// 静态自检（不依赖引擎的合法性：schema/粒度/标签/批维度/模型 ID）。
    pub fn static_valid(&self) -> Result<(), XcCode> {
        if self.schema_minor > SUPPORTED_SCHEMA_MINOR {
            return Err(XcCode::HtSchemaUnsupported);
        }
        if self.granularity_ops == 0 {
            return Err(XcCode::HtGranZero);
        }
        if !self.label.valid() {
            return Err(XcCode::HtLabelInvalid);
        }
        if let Some(b) = &self.batch {
            if !b.valid() {
                return Err(XcCode::HtBatchCapMiss);
            }
        }
        if self.model_id.is_none() && self.ops_required > 0 {
            // 有算子量的任务按推理口径要求模型 ID（判据"引擎特定参数必带"）
            return Err(XcCode::HtModelMissing);
        }
        Ok(())
    }
}

/// 校验结果行（判据"引擎能力×任务需求匹配校验"的输出）。
#[derive(Clone, Copy, Debug)]
pub struct MatchRow {
    /// 引擎类型。
    pub kind: EngineKind,
    /// 该引擎是否可承接。
    pub eligible: bool,
    /// 拒因（不可承接时携带；可承接为 None 语义——用空诊断占位无、改用枚举）。
    pub deny: Option<XcCode>,
    /// 能效序位（数值越小越优先；any 选路排序键）。
    pub eff_rank: u32,
}

/// 提交前校验规则表（判据"校验规则表"本体：逐引擎过规则）。
pub struct TaskValidator;

impl TaskValidator {
    /// 规则一：schema 静态自检（引擎无关，先跑）。
    /// 规则二：亲和闭集匹配（NpuOnly 只认 NPU 引擎）。
    /// 规则三：批任务需 FEAT_BATCH。
    /// 规则四：实时任务需 FEAT_REALTIME。
    /// 规则五：deadline 已过直接失败回执（提交前拦——判据"错过=失败不静默"）。
    pub fn validate(candidates: &[(EngineKind, u32)], task: &HetTaskExt, now_us: u64) -> Result<Vec<MatchRow>, XcCode> {
        task.static_valid()?;
        // 实时 deadline 提交前拦：已过即失败回执
        if let Some(dl) = task.deadline_us {
            if now_us >= dl {
                return Err(XcCode::HtDeadlinePassed);
            }
        }
        let mut out = Vec::new();
        let mut any_npu = false;
        for &(kind, caps) in candidates {
            if kind == EngineKind::Npu {
                any_npu = true;
            }
            let mut deny: Option<XcCode> = None;
            // 规则二：亲和
            if task.affinity == Affinity::NpuOnly && kind != EngineKind::Npu {
                deny = Some(XcCode::HtAffinityUnsat);
            }
            // 规则三：批能力
            if deny.is_none() && task.batch.is_some() && (caps & FEAT_BATCH) == 0 {
                deny = Some(XcCode::HtBatchCapMiss);
            }
            // 规则四：实时能力
            if deny.is_none() && task.deadline_us.is_some() && (caps & FEAT_REALTIME) == 0 {
                deny = Some(XcCode::HtRealtimeCapMiss);
            }
            // 能效序位（any 选路排序键）
            let eff_rank = EFFICIENCY_ORDER
                .iter()
                .position(|k| *k == kind)
                .map(|p| p as u32)
                .unwrap_or(u32::MAX);
            out.push(MatchRow { kind, eligible: deny.is_none(), deny, eff_rank });
        }
        // 亲和终判：NpuOnly 但候选里根本没有 NPU → 显性不可满足
        if task.affinity == Affinity::NpuOnly && !any_npu {
            return Err(XcCode::HtAffinityUnsat);
        }
        if out.is_empty() {
            return Err(XcCode::HtAffinityUnsat); // 候选空=无从承接
        }
        Ok(out)
    }

    /// any/prefer-NPU 选路（判据"any 时调度器按能效选引擎"）：
    /// 只在可承接集合里按能效序取最小；prefer-NPU 时 NPU 可承接则必选 NPU。
    pub fn pick(rows: &[MatchRow], affinity: Affinity) -> Option<EngineKind> {
        let elig: Vec<&MatchRow> = rows.iter().filter(|r| r.eligible).collect();
        if elig.is_empty() {
            return None;
        }
        match affinity {
            Affinity::NpuOnly => elig
                .iter()
                .find(|r| r.kind == EngineKind::Npu)
                .map(|r| r.kind),
            Affinity::PreferNpu => {
                if let Some(r) = elig.iter().find(|r| r.kind == EngineKind::Npu) {
                    return Some(r.kind);
                }
                elig.iter().min_by_key(|r| r.eff_rank).map(|r| r.kind)
            }
            Affinity::Any => elig.iter().min_by_key(|r| r.eff_rank).map(|r| r.kind),
        }
    }
}

/// 调度回执（deadline 语义的显性失败形态——判据"错过=失败不静默"）。
#[derive(Clone, Copy, Debug)]
pub enum DispatchOutcome {
    /// 已调度（引擎，承诺完成时点 ≤ deadline）。
    Dispatched { engine: EngineKind, eta_us: u64 },
    /// 失败回执：deadline 错过（显性——消费方必须收到此态）。
    DeadlineMissed { at_us: u64 },
    /// 失败回执：无可承接引擎。
    NoEngine,
}

/// 异构任务调度面（校验+选路+回执的小编排——判据六组的集成出口）。
pub struct HetScheduler {
    /// 候选引擎集（kind, caps 位图）。
    candidates: Vec<(EngineKind, u32)>,
    /// 调度计数。
    pub dispatched: u32,
    /// 失败计数（分因：deadline 错过/无引擎）。
    pub missed: u32,
    pub no_engine: u32,
}

impl HetScheduler {
    /// 构造（候选集由调用方从 vck02 EngineRegistry 投影而来——本模块
    /// 不重复登记引擎，只消费候选位图）。
    pub fn new(candidates: Vec<(EngineKind, u32)>) -> Self {
        HetScheduler { candidates, dispatched: 0, missed: 0, no_engine: 0 }
    }

    /// 提交任务（判据主路径：校验→选路→回执）。
    pub fn submit(&mut self, task: &HetTaskExt, now_us: u64) -> Result<DispatchOutcome, XcCode> {
        let rows = TaskValidator::validate(&self.candidates, task, now_us)?;
        match TaskValidator::pick(&rows, task.affinity) {
            None => {
                self.no_engine = self.no_engine.saturating_add(1);
                Ok(DispatchOutcome::NoEngine)
            }
            Some(kind) => {
                // ETA 估算：算子量 / 粒度 × 粒度时延基线（µs）——静态能效档
                // 系数表简化为粒度×批次步数×10µs（工程基线，对账口径写死）。
                let steps = task.batch.as_ref().map(|b| b.outer_steps() as u64).unwrap_or(1);
                let ops_win = task.ops_required / task.granularity_ops.max(1);
                let eta = (ops_win.max(1)).saturating_mul(10).saturating_mul(steps.max(1));
                if let Some(dl) = task.deadline_us {
                    if now_us.saturating_add(eta) > dl {
                        self.missed = self.missed.saturating_add(1);
                        return Ok(DispatchOutcome::DeadlineMissed { at_us: now_us.saturating_add(eta) });
                    }
                }
                self.dispatched = self.dispatched.saturating_add(1);
                Ok(DispatchOutcome::Dispatched { engine: kind, eta_us: eta })
            }
        }
    }

    /// 单行摘要。
    pub fn summary(&self) -> alloc::string::String {
        alloc::format!(
            "异构调度{} 候选{} 已派{} 错期{} 无引擎{}",
            HET_TASK_SCHEMA,
            self.candidates.len(),
            self.dispatched,
            self.missed,
            self.no_engine,
        )
    }
}
