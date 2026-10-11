//! CGPU-F0167 · 执行失败与降级（CGPU-B 域 · 帧任务图与执行域 · B01 组）。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F0167`
//!
//! **判据（锚点原文）**：四类失败路由、传播正确、降级执行对拍、记账全量、
//! 注入测试。
//!
//! **职责定位（锚点原文）**：任务执行失败的处置：失败分类（资源缺失/编译
//! 错误/GPU 超时/OOM）、失败传播策略（上游失败→下游全跳过；可替代任务→
//! 替代执行）、降级执行（失败任务走降级路径——低质量替代算子）、失败记账
//! （每个失败任务五元组入遥测——与 CGPU-F0015 对齐）。
//!
//! ## 一、失败四分类：闭集，不立第五类
//!
//! 锚点点名四类（[`FailureKind`]：资源缺失/编译错误/GPU 超时/OOM）——
//! 类别闭集纪律：表外失败不是「其他」桶，而是上游建模缺口，走显性
//! [`XcCode::Unclassified`] 拒绝并记账（异常零静默）。每类绑定独立诊断码
//! （0x60 段独占，[`XcCode`]）、固定遥测指标名（[`FAIL_METRICS`]）、
//! 重试策略（[`FailureKind::is_retryable`]——超时可重试/OOM 回收后可重试/
//! 资源缺失与编译错误不可重试）与可操作建议（[`FailureKind::suggestion`]）。
//!
//! ## 二、传播正确：失败沿依赖边走，三道止损闸依次生效
//!
//! 执行序按调用方给定的拓扑序推进（本模块校验拓扑性：依赖必须指向
//! 更早任务，违反即显性拒绝——不静默重排）。某任务失败后按四步路由：
//! ①重试闸（可重试类别且重试预算未耗尽 → [`RetryEngine`] 带退避重试）；
//! ②替代闸（[`TaskNode::substitute`]）→ 替代执行，替代成功则传播在此
//! 截断（下游照常）；③降级闸（[`TaskNode::degradable`]）→ 降级执行
//! （沿质量阶梯降档，[`DegradationLadder`]），下游照常；④三闸皆无 →
//! 硬失败，全部传递依赖任务按依赖边闭包逐个标 [`TaskOutcome::Skipped`]
//! （跳过也是结论，不是失踪——传播账 [`PropagationRecord`] 记根因链）。
//!
//! ## 三、降级执行对拍：降级不是悄悄变差
//!
//! 降级执行留双账：结果账（[`TaskOutcome::Degraded`]）+ 对拍账
//! （[`ExecLedger::degraded_pairs`]：原任务 id → 降级档位 → 质量声明），
//! 消费方凭对拍账知道「这一帧这里是用低档算子跑的」——降级诚实是
//! O 域「降质不降帧」铁律在 B 域的执行面。帧级降级预算
//! （[`DegradationBudget`]）约束单帧降级次数：预算耗尽后的降级申请
//! 按策略转硬失败或拒绝降档（策略显性可配，不静默）。
//!
//! ## 四、记账全量：五元组进遥测，账实守恒
//!
//! 每个失败任务一条五元组指标（[`FailMetric`]：名称/维度/单位/类型/
//! 采样——与 F0274/B08 遥测五元组同构对齐，不第二套口径）；账本侧
//! [`ExecLedger`] 记全量处置事实，并做守恒对账：六类结论计数之和 ==
//! 任务总数（[`ExecLedger::counts_reconcile`]）——多记少记都是缺陷。
//! 失败指纹（[`FailureFingerprint`]）按 (类别, 帧阶段, 任务族) 去重
//! 计数：同类失败风暴折叠为一条指纹多条计数，遥测不爆量。
//!
//! ## 五、注入测试：判据的函数级支撑
//!
//! [`FailureInjector`] 允许测试面按 (任务, 类别) 注入失败——判据
//! 「注入测试」的落地面；调度式注入（[`InjectSchedule`]：必现/前 N 次/
//! 每 K 次/按 tick 窗口）支持重试与传播的可复现演练。注入是显性
//! 入账的（`injected` 计数），不留暗门。生产路径传空注入表即零注入。
//!
//! **对接**：F0166（图执行器，本模块是其失败处置面）；F0161（任务图
//! 构建——节点与依赖来源）；F0274/B08（遥测五元组同源）；O 域（降级
//! 配方联动预留）；C 域（失败预算归还钩子——[`RefundHook`] 声明位）。
//! 零 panic 面（下标走 `get`/`Option`，算术饱和）、零 IO、零墙钟
//! （tick 上游注入）、无全局可变状态、no_std。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码（0x60 段独占：B01 组失败处置面）
// ---------------------------------------------------------------------------

/// 诊断码（u16；0x60 段为本单独占段，段内互异）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum XcCode {
    /// 资源缺失类失败。
    FailResourceMissing = 0x6001,
    /// 编译错误类失败。
    FailCompileError = 0x6002,
    /// GPU 超时类失败。
    FailGpuTimeout = 0x6003,
    /// OOM 类失败。
    FailOom = 0x6004,
    /// 拓扑违规：依赖指向了排序更晚的任务（不静默重排）。
    TopologyViolation = 0x6005,
    /// 表外失败类别（建模缺口，显性拒绝）。
    Unclassified = 0x6006,
    /// 重复任务 id：同一帧内任务表出现同 id 节点（表完整性缺口）。
    DuplicateTaskId = 0x6007,
    /// 替代目标不存在：替代任务 id 不在本帧任务表。
    SubstituteMissing = 0x6008,
    /// 替代自环：任务的替代链回到自身。
    SubstituteCycle = 0x6009,
    /// 替代链超深：替代链长度超过上限（默认 4）。
    SubstituteTooDeep = 0x600a,
    /// 重试预算耗尽：可重试类别但预算用完（转三步路由下一闸）。
    RetryExhausted = 0x600b,
    /// 降级预算耗尽：帧级降级次数已达上限（按策略处置）。
    DegradationBudgetExhausted = 0x600c,
    /// 质量阶梯为空：标记可降级但未声明任何降级档位。
    DegradationLadderEmpty = 0x600d,
    /// 依赖目标缺失：依赖 id 不在本帧任务表。
    DependencyMissing = 0x600e,
}

impl XcCode {
    /// 人读诊断（异常零静默：发生了什么/为什么/下一步）。
    pub const fn detail(self) -> &'static str {
        match self {
            XcCode::FailResourceMissing => "资源缺失：纹理/缓冲未就绪，建议核对上游生产任务与资源生命周期",
            XcCode::FailCompileError => "编译错误：着色器/管线编译失败，建议核对材质与目标档位兼容表",
            XcCode::FailGpuTimeout => "GPU 超时：任务超出预算窗口，建议核对预算标签与降级配方",
            XcCode::FailOom => "显存耗尽：分配失败，建议走 H 域水位回收后重试或降级",
            XcCode::TopologyViolation => "拓扑违规：依赖指向排序更晚任务，建议按拓扑序重排任务表",
            XcCode::Unclassified => "表外失败类别：上游建模缺口，建议补分类后再入执行面",
            XcCode::DuplicateTaskId => "重复任务 id：同帧任务表出现同 id 节点，建议检查图构建器去重",
            XcCode::SubstituteMissing => "替代目标不存在：替代任务 id 不在本帧任务表，建议核对替代声明",
            XcCode::SubstituteCycle => "替代自环：替代链回到自身，建议拆除环或改声明降级路径",
            XcCode::SubstituteTooDeep => "替代链超深：超过替代深度上限，建议减少替代层数或改降级",
            XcCode::RetryExhausted => "重试预算耗尽：可重试类别预算用完，走替代/降级路由",
            XcCode::DegradationBudgetExhausted => "降级预算耗尽：单帧降级次数已达上限，建议核对帧预算与降级策略",
            XcCode::DegradationLadderEmpty => "降级阶梯为空：标记可降级但未声明档位，建议补阶梯或改不可降级",
            XcCode::DependencyMissing => "依赖目标缺失：依赖 id 不在本帧任务表，建议核对图构建输出",
        }
    }

    /// 诊断码数值（遥测维度用，u16 面向固定宽度遥测通道）。
    pub const fn raw(self) -> u16 {
        self as u16
    }

    /// 是否属于「任务表完整性」类缺陷（此类拒绝整帧，不半帧执行）。
    pub const fn is_table_integrity(self) -> bool {
        matches!(
            self,
            XcCode::TopologyViolation
                | XcCode::DuplicateTaskId
                | XcCode::SubstituteMissing
                | XcCode::SubstituteCycle
                | XcCode::SubstituteTooDeep
                | XcCode::DependencyMissing
        )
    }
}

// ---------------------------------------------------------------------------
// 二、失败四分类（判据一：闭集）
// ---------------------------------------------------------------------------

/// 失败类别闭集（锚点点名四类；表外走 [`XcCode::Unclassified`]）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FailureKind {
    /// 资源缺失（纹理/缓冲未就绪）。
    ResourceMissing,
    /// 编译错误（着色器/管线编译失败）。
    CompileError,
    /// GPU 超时（超预算窗口）。
    GpuTimeout,
    /// OOM（显存耗尽）。
    Oom,
}

impl FailureKind {
    /// 对应诊断码。
    pub const fn code(self) -> XcCode {
        match self {
            FailureKind::ResourceMissing => XcCode::FailResourceMissing,
            FailureKind::CompileError => XcCode::FailCompileError,
            FailureKind::GpuTimeout => XcCode::FailGpuTimeout,
            FailureKind::Oom => XcCode::FailOom,
        }
    }

    /// 遥测指标名尾段（三级命名 `b01.execfail.<尾段>`）。
    pub const fn metric_tail(self) -> &'static str {
        match self {
            FailureKind::ResourceMissing => "resource_missing",
            FailureKind::CompileError => "compile_error",
            FailureKind::GpuTimeout => "gpu_timeout",
            FailureKind::Oom => "oom",
        }
    }

    /// 是否可重试（重试闸语义：超时可重试/OOM 回收后可重试；
    /// 资源缺失与编译错误是确定性缺陷，重试无意义——不烧预算）。
    pub const fn is_retryable(self) -> bool {
        matches!(self, FailureKind::GpuTimeout | FailureKind::Oom)
    }

    /// 默认重试上限（可重试类别的策略值；调用方可覆盖）。
    pub const fn default_retry_limit(self) -> u8 {
        match self {
            FailureKind::GpuTimeout => 1,
            FailureKind::Oom => 2,
            _ => 0,
        }
    }

    /// 默认退避 tick 数（第 n 次重试前等待；线性小步，饱和运算）。
    pub const fn retry_backoff_ticks(self, attempt: u8) -> u32 {
        match self {
            FailureKind::GpuTimeout => 2u32.saturating_add(attempt as u32),
            FailureKind::Oom => 4u32.saturating_add(attempt.saturating_mul(2) as u32),
            _ => 0,
        }
    }

    /// 可操作建议（错误路径带建议铁律的类别级默认值）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            FailureKind::ResourceMissing => "核对上游生产任务是否成功产出该资源；核对资源生命周期是否早于消费",
            FailureKind::CompileError => "核对材质着色器与目标档位兼容表；核对 shader 关键字组合是否超限",
            FailureKind::GpuTimeout => "核对预算标签是否低估；核对是否应声明降级路径；超预算比例进遥测",
            FailureKind::Oom => "触发 H 域水位回收后重试；或声明降级路径用低档算子",
        }
    }

    /// 四类全量（判据侧遍历用；顺序与 [`FAIL_METRICS`] 对齐）。
    pub const ALL: [FailureKind; 4] = [
        FailureKind::ResourceMissing,
        FailureKind::CompileError,
        FailureKind::GpuTimeout,
        FailureKind::Oom,
    ];

    /// 从表内下标还原类别（越界回 `None`——不 panic）。
    pub const fn from_index(idx: usize) -> Option<FailureKind> {
        match idx {
            0 => Some(FailureKind::ResourceMissing),
            1 => Some(FailureKind::CompileError),
            2 => Some(FailureKind::GpuTimeout),
            3 => Some(FailureKind::Oom),
            _ => None,
        }
    }

    /// 类别下标（遥测计数数组定位）。
    pub const fn index(self) -> usize {
        match self {
            FailureKind::ResourceMissing => 0,
            FailureKind::CompileError => 1,
            FailureKind::GpuTimeout => 2,
            FailureKind::Oom => 3,
        }
    }
}

/// 遥测五元组（与 F0274/B08 五元组同构：名称/维度/单位/类型/采样——
/// 不第二套口径；判据四的 schema 面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FailMetric {
    /// 三级命名（域.组.指标）。
    pub name: &'static str,
    /// 维度（按失败事件）。
    pub dimension: &'static str,
    /// 单位。
    pub unit: &'static str,
    /// 类型。
    pub kind: &'static str,
    /// 采样策略（事件驱动计数）。
    pub sampling: &'static str,
}

/// 四类失败的五元组指标表（与 [`FailureKind`] 一一对应，判据侧可独立
/// 重排对账）。
pub const FAIL_METRICS: [FailMetric; 4] = [
    FailMetric {
        name: "b01.execfail.resource_missing",
        dimension: "failure_event",
        unit: "count",
        kind: "u64",
        sampling: "on_event",
    },
    FailMetric {
        name: "b01.execfail.compile_error",
        dimension: "failure_event",
        unit: "count",
        kind: "u64",
        sampling: "on_event",
    },
    FailMetric {
        name: "b01.execfail.gpu_timeout",
        dimension: "failure_event",
        unit: "count",
        kind: "u64",
        sampling: "on_event",
    },
    FailMetric {
        name: "b01.execfail.oom",
        dimension: "failure_event",
        unit: "count",
        kind: "u64",
        sampling: "on_event",
    },
];

/// 五元组指标名前缀（可 grep 不可漂移）。
pub const FAIL_METRIC_PREFIX: &str = "b01.execfail.";

/// 重试专用指标（重试闸生效时入账——重试也是遥测事实）。
pub const RETRY_METRIC: FailMetric = FailMetric {
    name: "b01.execfail.retry",
    dimension: "retry_event",
    unit: "count",
    kind: "u64",
    sampling: "on_event",
};

/// 传播跳过专用指标（传播闭包产生的跳过逐条入账）。
pub const PROPAGATION_METRIC: FailMetric = FailMetric {
    name: "b01.execfail.propagation_skip",
    dimension: "skip_event",
    unit: "count",
    kind: "u64",
    sampling: "on_event",
};

// ---------------------------------------------------------------------------
// 三、质量阶梯与降级声明（判据三：降级执行对拍的档位面）
// ---------------------------------------------------------------------------

/// 渲染质量档位闭集（降级阶梯的级；全档——降级只降不升）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum QualityTier {
    /// 全质量（帧承诺档）。
    Full,
    /// 高档（默认降一级）。
    High,
    /// 中档（第二次降级）。
    Medium,
    /// 低档（低质量替代算子默认落点）。
    Low,
    /// 最低档（保底：仍产出，供对拍与回放）。
    Minimal,
}

impl QualityTier {
    /// 档位序号（0=全质量；遥测维度用）。
    pub const fn rank(self) -> u8 {
        match self {
            QualityTier::Full => 0,
            QualityTier::High => 1,
            QualityTier::Medium => 2,
            QualityTier::Low => 3,
            QualityTier::Minimal => 4,
        }
    }

    /// 从序号还原（越界回 `None`）。
    pub const fn from_rank(r: u8) -> Option<QualityTier> {
        match r {
            0 => Some(QualityTier::Full),
            1 => Some(QualityTier::High),
            2 => Some(QualityTier::Medium),
            3 => Some(QualityTier::Low),
            4 => Some(QualityTier::Minimal),
            _ => None,
        }
    }

    /// 人读声明（对拍账里的「质量声明」字段来源）。
    pub const fn declaration(self) -> &'static str {
        match self {
            QualityTier::Full => "全质量（帧承诺档）",
            QualityTier::High => "高档（关一级特效）",
            QualityTier::Medium => "中档（半分辨率次级通道）",
            QualityTier::Low => "低档（低质量替代算子）",
            QualityTier::Minimal => "最低档（保底产出，供对拍）",
        }
    }
}

/// 任务原始质量档（节点声明；降级从此档向下走阶梯）。
pub const DEFAULT_TASK_TIER: QualityTier = QualityTier::Full;

/// 单帧降级预算上限默认值（策略值，可调——O 域降质不降帧的量面约束）。
pub const DEFAULT_DEGRADATION_BUDGET: u32 = 8;

/// 替代链深度上限（替代闸防环防深链；超限显性拒绝）。
pub const MAX_SUBSTITUTE_DEPTH: u8 = 4;

/// 重试退避上限 tick（退避线性增长的天花板——防退避失控）。
pub const MAX_RETRY_BACKOFF_TICKS: u32 = 32;

// ---------------------------------------------------------------------------
// 四、任务节点与结论闭集（判据二：传播）
// ---------------------------------------------------------------------------

/// 帧阶段（任务所属管线段——失败指纹维度之一）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FramePhase {
    /// 资源准备段。
    Setup,
    /// 几何段。
    Geometry,
    /// 光照段。
    Lighting,
    /// 后处理段。
    Post,
    /// 呈现段。
    Present,
}

impl FramePhase {
    /// 段序号（指纹哈希维度）。
    pub const fn rank(self) -> u8 {
        match self {
            FramePhase::Setup => 0,
            FramePhase::Geometry => 1,
            FramePhase::Lighting => 2,
            FramePhase::Post => 3,
            FramePhase::Present => 4,
        }
    }

    /// 人读名（遥测维度标签）。
    pub const fn label(self) -> &'static str {
        match self {
            FramePhase::Setup => "setup",
            FramePhase::Geometry => "geometry",
            FramePhase::Lighting => "lighting",
            FramePhase::Post => "post",
            FramePhase::Present => "present",
        }
    }
}

/// 任务节点（依赖 + 三道止损闸声明 + 质量与重试预算声明）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TaskNode {
    /// 任务 id（帧内唯一；执行序按调用方拓扑序数组下标）。
    pub task_id: u32,
    /// 依赖的任务 id 集合（必须指向更早执行的任务）。
    pub deps: Vec<u32>,
    /// 替代任务 id（可替代任务的第一止损闸之后的第二闸）。
    pub substitute: Option<u32>,
    /// 可降级标记（第三闸：低质量替代算子可顶上）。
    pub degradable: bool,
    /// 任务所属帧阶段（失败指纹维度；缺省 Setup）。
    pub phase: FramePhase,
    /// 重试预算（0=禁重试；缺省取 [`FailureKind::default_retry_limit`]）。
    pub retry_budget: u8,
}

impl TaskNode {
    /// 最小节点（兼容构造器：只给 id 与依赖；其余取策略默认）。
    pub fn minimal(task_id: u32, deps: Vec<u32>) -> TaskNode {
        TaskNode {
            task_id,
            deps,
            substitute: None,
            degradable: false,
            phase: FramePhase::Setup,
            retry_budget: 0,
        }
    }

    /// 全量构造器。
    pub fn with_gates(
        task_id: u32,
        deps: Vec<u32>,
        substitute: Option<u32>,
        degradable: bool,
        phase: FramePhase,
        retry_budget: u8,
    ) -> TaskNode {
        TaskNode {
            task_id,
            deps,
            substitute,
            degradable,
            phase,
            retry_budget,
        }
    }
}

/// 单任务执行结论（闭集；六类账实守恒）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskOutcome {
    /// 正常成功。
    Succeeded,
    /// 硬失败（三道止损闸皆无或止损亦失败）。
    Failed,
    /// 被传播跳过（上游硬失败的传递依赖）。
    Skipped,
    /// 替代执行成功（替代闸生效）。
    Substituted,
    /// 降级执行成功（降级闸生效）。
    Degraded,
    /// 从未进入执行面（拓扑校验拒绝的整帧）。
    NotRun,
}

impl TaskOutcome {
    /// 结论下标（计数数组定位；与 [`ExecLedger::count_of`] 对齐）。
    pub const fn index(self) -> usize {
        match self {
            TaskOutcome::Succeeded => 0,
            TaskOutcome::Failed => 1,
            TaskOutcome::Skipped => 2,
            TaskOutcome::Substituted => 3,
            TaskOutcome::Degraded => 4,
            TaskOutcome::NotRun => 5,
        }
    }

    /// 是否为「有产出」结论（下游不因它跳过）。
    pub const fn produces(self) -> bool {
        matches!(
            self,
            TaskOutcome::Succeeded | TaskOutcome::Substituted | TaskOutcome::Degraded
        )
    }

    /// 人读名（报告面）。
    pub const fn label(self) -> &'static str {
        match self {
            TaskOutcome::Succeeded => "成功",
            TaskOutcome::Failed => "硬失败",
            TaskOutcome::Skipped => "传播跳过",
            TaskOutcome::Substituted => "替代执行",
            TaskOutcome::Degraded => "降级执行",
            TaskOutcome::NotRun => "未执行",
        }
    }
}

/// 单任务处置账（结论 + 失败类别 + 诊断码 + 人读明细）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TaskVerdict {
    /// 任务 id。
    pub task_id: u32,
    /// 结论。
    pub outcome: TaskOutcome,
    /// 失败类别（仅 Failed/Degraded 时有值）。
    pub failure: Option<FailureKind>,
    /// 诊断码（结论的码面）。
    pub code: Option<XcCode>,
    /// 人读明细（错误路径带可操作建议）。
    pub detail: String,
    /// 降级后档位（仅 Degraded 有值；对拍账之外的结论内快照）。
    pub degraded_to: Option<QualityTier>,
    /// 重试次数（本任务实际重试了几次）。
    pub retries: u8,
    /// 根因任务 id（Skipped 时指向上游硬失败任务；其余 None）。
    pub root_cause: Option<u32>,
}

// ---------------------------------------------------------------------------
// 五、重试引擎（止损闸一：可重试类别的预算化重试）
// ---------------------------------------------------------------------------

/// 单条重试记录（谁/哪类/第几次/退避多少 tick/结果）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RetryRecord {
    /// 任务 id。
    pub task_id: u32,
    /// 失败类别。
    pub kind: FailureKind,
    /// 第几次重试（1 起）。
    pub attempt: u8,
    /// 本次重试前的退避 tick 数（已按上限钳制）。
    pub backoff_ticks: u32,
    /// 重试是否成功。
    pub succeeded: bool,
    /// 重试发生的 tick（调用方注入的逻辑时钟快照）。
    pub at_tick: u64,
}

/// 重试引擎（预算化重试 + 线性退避 + 全量记录）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RetryEngine {
    records: Vec<RetryRecord>,
    /// 已消耗重试总次数（跨任务累计）。
    consumed: u64,
    /// 重试成功总次数（对拍：重试不是白烧预算）。
    recovered: u64,
}

impl RetryEngine {
    /// 空引擎。
    pub const fn new() -> RetryEngine {
        RetryEngine {
            records: Vec::new(),
            consumed: 0,
            recovered: 0,
        }
    }

    /// 对单次失败尝试重试决策：
    /// 返回 `Some(退避tick)` 表示应当重试（预算未耗尽且类别可重试），
    /// 返回 `None` 表示不重试（类别不可重试或预算耗尽）。
    pub fn decide(
        &mut self,
        node: &TaskNode,
        kind: FailureKind,
        attempt: u8,
        tick: u64,
    ) -> Option<u32> {
        if !kind.is_retryable() {
            return None;
        }
        let budget = if node.retry_budget == 0 {
            kind.default_retry_limit()
        } else {
            node.retry_budget
        };
        if attempt > budget || budget == 0 {
            return None;
        }
        let backoff = kind
            .retry_backoff_ticks(attempt)
            .min(MAX_RETRY_BACKOFF_TICKS);
        self.records.push(RetryRecord {
            task_id: node.task_id,
            kind,
            attempt,
            backoff_ticks: backoff,
            succeeded: false,
            at_tick: tick,
        });
        self.consumed = self.consumed.saturating_add(1);
        Some(backoff)
    }

    /// 回填某任务某次重试的结果（成功则计入恢复账）。
    pub fn resolve(&mut self, task_id: u32, attempt: u8, succeeded: bool) {
        let mut i = self.records.len();
        while i > 0 {
            i -= 1;
            if let Some(r) = self.records.get_mut(i) {
                if r.task_id == task_id && r.attempt == attempt && !r.succeeded {
                    r.succeeded = succeeded;
                    if succeeded {
                        self.recovered = self.recovered.saturating_add(1);
                    }
                    return;
                }
            }
        }
    }

    /// 全量重试记录（判据/遥测面）。
    pub fn records(&self) -> &[RetryRecord] {
        &self.records
    }

    /// 消耗的重试总次数。
    pub const fn consumed(&self) -> u64 {
        self.consumed
    }

    /// 重试恢复总次数（重试后成功的次数）。
    pub const fn recovered(&self) -> u64 {
        self.recovered
    }

    /// 某任务已消耗的重试次数。
    pub fn consumed_of(&self, task_id: u32) -> u8 {
        let mut n = 0u8;
        let mut i = 0usize;
        while i < self.records.len() {
            if let Some(r) = self.records.get(i) {
                if r.task_id == task_id {
                    n = n.saturating_add(1);
                }
            }
            i += 1;
        }
        n
    }

    /// 重试恢复率（万分比；无重试时返回 `None`——不产恒零假分母）。
    pub fn recovery_rate_bps(&self) -> Option<u64> {
        if self.consumed == 0 {
            return None;
        }
        Some(self.recovered.saturating_mul(10_000) / self.consumed)
    }
}

// ---------------------------------------------------------------------------
// 六、失败注入面（判据五：注入测试）
// ---------------------------------------------------------------------------

/// 注入调度（确定性注入：让重试/传播/降级可复现演练）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InjectSchedule {
    /// 必现：该任务每次执行必失败。
    Always,
    /// 前 N 次失败（之后放行——重试闸恢复演练用）。
    FirstN(u8),
    /// 每 K 次失败一次（K>=1；1 等价必现）。
    EveryK(u32),
    /// tick 窗口注入：仅在 [from, to] 闭区间内失败（时间窗演练）。
    Window { from: u64, to: u64 },
}

impl InjectSchedule {
    /// 给定该任务第 `occurrence` 次执行与当前 tick，判断是否注入。
    pub fn fires(&self, occurrence: u32, tick: u64) -> bool {
        match *self {
            InjectSchedule::Always => true,
            InjectSchedule::FirstN(n) => occurrence <= n as u32,
            InjectSchedule::EveryK(k) => {
                if k == 0 {
                    false
                } else {
                    occurrence.saturating_sub(1) % k == 0
                }
            }
            InjectSchedule::Window { from, to } => tick >= from && tick <= to,
        }
    }
}

/// 失败注入器（测试面：按 (任务, 类别, 调度) 注入；生产传空表即零注入）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FailureInjector {
    injections: Vec<(u32, FailureKind, InjectSchedule)>,
    /// 各任务已执行次数（调度判定用；跨帧持续计数）。
    occurrences: Vec<(u32, u32)>,
}

impl FailureInjector {
    /// 空注入（生产路径）。
    pub const fn none() -> FailureInjector {
        FailureInjector {
            injections: Vec::new(),
            occurrences: Vec::new(),
        }
    }

    /// 注入一条必现失败（兼容旧 API）。
    pub fn inject(&mut self, task_id: u32, kind: FailureKind) {
        self.injections.push((task_id, kind, InjectSchedule::Always));
    }

    /// 注入一条带调度的失败。
    pub fn inject_with(&mut self, task_id: u32, kind: FailureKind, sched: InjectSchedule) {
        self.injections.push((task_id, kind, sched));
    }

    /// 查询某任务本次执行是否注入失败（推进该任务的执行计数）。
    pub fn fail_of(&mut self, task_id: u32, tick: u64) -> Option<FailureKind> {
        let occ = Self::bump(&mut self.occurrences, task_id);
        let mut i = 0usize;
        while i < self.injections.len() {
            if let Some((id, kind, sched)) = self.injections.get(i) {
                if *id == task_id && sched.fires(occ, tick) {
                    return Some(*kind);
                }
            }
            i += 1;
        }
        None
    }

    /// 预判下一次执行该任务是否仍会注入（重试闸恢复判定用——
    /// 不推进计数；以「再执行一次」的 occurrence 预判）。
    pub fn will_fire_next(&self, task_id: u32, tick: u64) -> bool {
        let occ = Self::peek_occ(&self.occurrences, task_id).saturating_add(1);
        let mut i = 0usize;
        while i < self.injections.len() {
            if let Some((id, _kind, sched)) = self.injections.get(i) {
                if *id == task_id && sched.fires(occ, tick) {
                    return true;
                }
            }
            i += 1;
        }
        false
    }

    /// 只读版查询（不推进计数——报告面用）。
    pub fn peek(&self, task_id: u32) -> Option<FailureKind> {
        let occ = Self::peek_occ(&self.occurrences, task_id);
        let mut i = 0usize;
        while i < self.injections.len() {
            if let Some((id, kind, sched)) = self.injections.get(i) {
                if *id == task_id && sched.fires(occ, u64::MAX) {
                    return Some(*kind);
                }
            }
            i += 1;
        }
        None
    }

    fn bump(table: &mut Vec<(u32, u32)>, task_id: u32) -> u32 {
        let mut i = 0usize;
        while i < table.len() {
            if let Some(e) = table.get_mut(i) {
                if e.0 == task_id {
                    e.1 = e.1.saturating_add(1);
                    return e.1;
                }
            }
            i += 1;
        }
        table.push((task_id, 1));
        1
    }

    fn peek_occ(table: &[(u32, u32)], task_id: u32) -> u32 {
        let mut i = 0usize;
        while i < table.len() {
            if let Some(e) = table.get(i) {
                if e.0 == task_id {
                    return e.1;
                }
            }
            i += 1;
        }
        0
    }

    /// 注入条目总数（显性入账用）。
    pub const fn len(&self) -> usize {
        self.injections.len()
    }

    /// 空判定。
    pub const fn is_empty(&self) -> bool {
        self.injections.is_empty()
    }

    /// 清空注入表（演练结束还原生产态）。
    pub fn clear(&mut self) {
        self.injections.clear();
        self.occurrences.clear();
    }
}

// ---------------------------------------------------------------------------
// 七、失败指纹（记账全量：同类失败风暴折叠计数）
// ---------------------------------------------------------------------------

/// 失败指纹：按 (类别, 帧阶段) 折叠的失败风暴聚合键。
///
/// 同一 (类别, 阶段) 的失败不逐条爆遥测，而是指纹计数递增——
/// 遥测面一条指纹多条计数；全量逐条事实仍在 [`ExecLedger`]。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct FailureFingerprint {
    /// 失败类别下标（0~3，[`FailureKind::index`]）。
    pub kind_idx: u8,
    /// 帧阶段序号（[`FramePhase::rank`]）。
    pub phase_rank: u8,
}

impl FailureFingerprint {
    /// 从事实构造指纹。
    pub const fn of(kind: FailureKind, phase: FramePhase) -> FailureFingerprint {
        FailureFingerprint {
            kind_idx: kind.index() as u8,
            phase_rank: phase.rank(),
        }
    }

    /// 指纹的遥测维度标签（`kind@phase`）。
    pub fn label(&self) -> String {
        let kind_tail = match FailureKind::from_index(self.kind_idx as usize) {
            Some(k) => k.metric_tail(),
            None => "unknown",
        };
        let phase_label = match QualityTier::from_rank(0) {
            _ => match self.phase_rank {
                0 => "setup",
                1 => "geometry",
                2 => "lighting",
                3 => "post",
                _ => "present",
            },
        };
        alloc::format!("{}@{}", kind_tail, phase_label)
    }
}

/// 指纹账：指纹 → 计数（保序聚合，不重排）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FingerprintLedger {
    entries: Vec<(FailureFingerprint, u64)>,
}

impl FingerprintLedger {
    /// 空账。
    pub const fn new() -> FingerprintLedger {
        FingerprintLedger {
            entries: Vec::new(),
        }
    }

    /// 计入一次失败。
    pub fn record(&mut self, fp: FailureFingerprint) {
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get_mut(i) {
                if e.0 == fp {
                    e.1 = e.1.saturating_add(1);
                    return;
                }
            }
            i += 1;
        }
        self.entries.push((fp, 1));
    }

    /// 某指纹的计数（无则 0）。
    pub fn count_of(&self, fp: FailureFingerprint) -> u64 {
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if e.0 == fp {
                    return e.1;
                }
            }
            i += 1;
        }
        0
    }

    /// 全部指纹（判据遍历面）。
    pub fn entries(&self) -> &[(FailureFingerprint, u64)] {
        &self.entries
    }

    /// 指纹种类数。
    pub const fn kinds(&self) -> usize {
        self.entries.len()
    }

    /// 指纹计数总和（应等于硬失败+降级+替代账中的失败事实总数）。
    pub fn total(&self) -> u64 {
        let mut s = 0u64;
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                s = s.saturating_add(e.1);
            }
            i += 1;
        }
        s
    }
}

// ---------------------------------------------------------------------------
// 八、传播账（判据二：跳过也是结论——根因链全量）
// ---------------------------------------------------------------------------

/// 传播记录：一个被跳过任务的上游归因链。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PropagationRecord {
    /// 被跳过的任务 id。
    pub skipped_task: u32,
    /// 直接依赖中的硬失败任务 id（根因；可能多个——多依赖同失败）。
    pub causes: Vec<u32>,
    /// 传播深度（根因在依赖闭包中的层数；直接失败=1）。
    pub depth: u32,
}

/// 传播引擎：正向闭包计算 + 根因归因 + 深度统计。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PropagationEngine {
    records: Vec<PropagationRecord>,
    /// 最大传播深度（本帧）。
    max_depth: u32,
    /// 传播跳过总数。
    total_skipped: u64,
}

impl PropagationEngine {
    /// 空引擎。
    pub const fn new() -> PropagationEngine {
        PropagationEngine {
            records: Vec::new(),
            max_depth: 0,
            total_skipped: 0,
        }
    }

    /// 登记一次传播跳过。
    pub fn record(&mut self, skipped_task: u32, causes: Vec<u32>, depth: u32) {
        if depth > self.max_depth {
            self.max_depth = depth;
        }
        self.total_skipped = self.total_skipped.saturating_add(1);
        self.records.push(PropagationRecord {
            skipped_task,
            causes,
            depth,
        });
    }

    /// 全量传播记录。
    pub fn records(&self) -> &[PropagationRecord] {
        &self.records
    }

    /// 本帧最大传播深度。
    pub const fn max_depth(&self) -> u32 {
        self.max_depth
    }

    /// 传播跳过总数。
    pub const fn total_skipped(&self) -> u64 {
        self.total_skipped
    }

    /// 某根因任务的全部受害任务 id（下游影响面查询）。
    pub fn victims_of(&self, cause: u32) -> Vec<u32> {
        let mut out = Vec::new();
        let mut i = 0usize;
        while i < self.records.len() {
            if let Some(r) = self.records.get(i) {
                let mut hit = false;
                let mut c = 0usize;
                while c < r.causes.len() {
                    if let Some(x) = r.causes.get(c) {
                        if *x == cause {
                            hit = true;
                            break;
                        }
                    }
                    c += 1;
                }
                if hit {
                    out.push(r.skipped_task);
                }
            }
            i += 1;
        }
        out
    }
}

// ---------------------------------------------------------------------------
// 九、帧级降级预算（判据三：降级有量面约束）
// ---------------------------------------------------------------------------

/// 降级预算处置策略（预算耗尽后的行为——显性可配，不静默）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DegradationPolicy {
    /// 预算耗尽后降级申请转硬失败（质量优先于产出完整性）。
    FailHard,
    /// 预算耗尽后降级申请转跳过（产出完整性优先于质量）。
    SkipRest,
}

impl DegradationPolicy {
    /// 策略默认值（产出完整性优先——降质不降帧的 O 域铁律取向）。
    pub const fn default_policy() -> DegradationPolicy {
        DegradationPolicy::SkipRest
    }
}

impl Default for DegradationPolicy {
    fn default() -> DegradationPolicy {
        DegradationPolicy::default_policy()
    }
}

/// 帧级降级预算。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DegradationBudget {
    /// 上限（当前策略值，可调）。
    pub limit: u32,
    used: u32,
    /// 累计被预算拒绝的降级申请数（显性入账）。
    pub rejected: u32,
    /// 处置策略。
    pub policy: DegradationPolicy,
}

impl DegradationBudget {
    /// 新预算（默认上限与策略）。
    pub const fn new() -> DegradationBudget {
        DegradationBudget {
            limit: DEFAULT_DEGRADATION_BUDGET,
            used: 0,
            rejected: 0,
            policy: DegradationPolicy::SkipRest,
        }
    }

    /// 自定义上限与策略。
    pub const fn with(limit: u32, policy: DegradationPolicy) -> DegradationBudget {
        DegradationBudget {
            limit,
            used: 0,
            rejected: 0,
            policy,
        }
    }

    /// 申请一次降级额度：`Some(剩余)`=批准，`None`=预算耗尽。
    pub fn request(&mut self) -> Option<u32> {
        if self.used >= self.limit {
            self.rejected = self.rejected.saturating_add(1);
            return None;
        }
        self.used = self.used.saturating_add(1);
        Some(self.limit.saturating_sub(self.used))
    }

    /// 已用额度。
    pub const fn used(&self) -> u32 {
        self.used
    }

    /// 是否耗尽。
    pub const fn exhausted(&self) -> bool {
        self.used >= self.limit
    }

    /// 重置（新帧）。
    pub fn reset(&mut self) {
        self.used = 0;
    }
}

impl Default for DegradationBudget {
    fn default() -> DegradationBudget {
        DegradationBudget::new()
    }
}

/// 质量阶梯：单任务可降级档位序列（从原始档向下；空阶梯=不可降级）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DegradationLadder {
    /// 原始档。
    pub origin: QualityTier,
    /// 降级序列（逐项向下；最后一项之后无法再降）。
    pub steps: Vec<QualityTier>,
}

impl DegradationLadder {
    /// 默认阶梯：Full → Low → Minimal（锚点「低质量替代算子」语义）。
    pub fn standard() -> DegradationLadder {
        DegradationLadder {
            origin: QualityTier::Full,
            steps: vec![QualityTier::Low, QualityTier::Minimal],
        }
    }

    /// 全阶梯（Full→High→Medium→Low→Minimal）。
    pub fn full_ladder() -> DegradationLadder {
        DegradationLadder {
            origin: QualityTier::Full,
            steps: vec![
                QualityTier::High,
                QualityTier::Medium,
                QualityTier::Low,
                QualityTier::Minimal,
            ],
        }
    }

    /// 降 `n` 步后的档位（超出阶梯给最低档——保底产出）。
    pub fn step_down(&self, n: u32) -> QualityTier {
        if self.steps.is_empty() {
            return self.origin;
        }
        let idx = if n as usize >= self.steps.len() {
            self.steps.len().saturating_sub(1)
        } else {
            n as usize
        };
        match self.steps.get(idx) {
            Some(t) => *t,
            None => self.origin,
        }
    }

    /// 还能再降几步。
    pub fn remaining(&self, used: u32) -> u32 {
        let total = self.steps.len() as u32;
        total.saturating_sub(used)
    }
}

// ---------------------------------------------------------------------------
// 十、预算归还钩子（对接 C 域：失败任务的预算归还声明位）
// ---------------------------------------------------------------------------

/// 预算归还钩子：失败/跳过任务的预算归还事件（C 域 F0324「原子归还」
/// 的 B 域侧事件源——本模块只产出事件，不做归还执行）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RefundHook {
    /// 归还来源任务 id。
    pub task_id: u32,
    /// 归还原因（失败或跳过）。
    pub reason: RefundReason,
    /// 归还的预估耗时微秒（由调用方从预算标签带出；0=未声明）。
    pub est_us: u64,
}

/// 归还原因闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RefundReason {
    /// 硬失败归还。
    Failed,
    /// 传播跳过归还。
    Skipped,
    /// 拓扑拒绝整帧归还（全部任务）。
    FrameRejected,
}

impl RefundReason {
    /// 人读名。
    pub const fn label(self) -> &'static str {
        match self {
            RefundReason::Failed => "硬失败归还",
            RefundReason::Skipped => "传播跳过归还",
            RefundReason::FrameRejected => "整帧拒绝归还",
        }
    }
}

// ---------------------------------------------------------------------------
// 十一、执行引擎（判据二、三：三闸路由 + 传播闭包 + 全量记账）
// ---------------------------------------------------------------------------

/// 帧执行账（全量处置事实 + 守恒对账 + 降级对拍账 + 指纹/重试/传播账）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ExecLedger {
    verdicts: Vec<TaskVerdict>,
    /// 降级对拍账：(原任务 id, 遥测指标名, 质量声明)。
    degraded_pairs: Vec<(u32, String, String)>,
    counts: [u64; 6],
    injected: u64,
    /// 按类别的失败计数（与 FAIL_METRICS 一一对应）。
    kind_counts: [u64; 4],
    /// 失败指纹账（风暴折叠）。
    fingerprints: FingerprintLedger,
    /// 重试引擎账。
    retries: RetryEngine,
    /// 传播账。
    propagation: PropagationEngine,
    /// 预算归还事件（C 域对接事件源）。
    refunds: Vec<RefundHook>,
    /// 整帧被拒原因（None=帧已执行）。
    frame_reject: Option<XcCode>,
    /// 验证拒绝明细（人读）。
    reject_detail: String,
}

impl ExecLedger {
    /// 六类结论计数之和 == 任务总数（判据四：账实守恒；多记少记即缺陷）。
    pub fn counts_reconcile(&self, total_tasks: usize) -> bool {
        let mut sum: u64 = 0;
        let mut i = 0usize;
        while i < 6 {
            sum = sum.saturating_add(self.counts[i]);
            i += 1;
        }
        sum == total_tasks as u64
    }

    /// 按结论取计数（越界给 0——不 panic）。
    pub fn count_of(&self, outcome: TaskOutcome) -> u64 {
        *self.counts.get(outcome.index()).unwrap_or(&0)
    }

    /// 全量处置账（判据四：记账全量——逐条可回查）。
    pub fn verdicts(&self) -> &[TaskVerdict] {
        &self.verdicts
    }

    /// 降级对拍账（判据三：降级不是悄悄变差）。
    pub fn degraded_pairs(&self) -> &[(u32, String, String)] {
        &self.degraded_pairs
    }

    /// 注入计数（显性入账，不留暗门）。
    pub const fn injected(&self) -> u64 {
        self.injected
    }

    /// 某结论的任务 id 集合（下游消费面）。
    pub fn ids_with(&self, outcome: TaskOutcome) -> Vec<u32> {
        let mut out = Vec::new();
        let mut i = 0usize;
        while i < self.verdicts.len() {
            if let Some(v) = self.verdicts.get(i) {
                if v.outcome == outcome {
                    out.push(v.task_id);
                }
            }
            i += 1;
        }
        out
    }

    /// 按类别的失败计数（遥测面：与 [`FAIL_METRICS`] 下标对齐）。
    pub fn kind_count(&self, kind: FailureKind) -> u64 {
        *self.kind_counts.get(kind.index()).unwrap_or(&0)
    }

    /// 失败指纹账（只读）。
    pub const fn fingerprints(&self) -> &FingerprintLedger {
        &self.fingerprints
    }

    /// 重试账（只读）。
    pub const fn retry_engine(&self) -> &RetryEngine {
        &self.retries
    }

    /// 传播账（只读）。
    pub const fn propagation(&self) -> &PropagationEngine {
        &self.propagation
    }

    /// 预算归还事件。
    pub fn refunds(&self) -> &[RefundHook] {
        &self.refunds
    }

    /// 整帧拒绝原因。
    pub const fn frame_reject(&self) -> Option<XcCode> {
        self.frame_reject
    }

    /// 拒绝明细（人读）。
    pub fn reject_detail(&self) -> &str {
        &self.reject_detail
    }

    /// 深化守恒对账：六类结论之和 == 指纹账事实总数 + 成功数。
    ///
    /// 指纹账记的是「失败事实」（硬失败/降级/替代三类中带失败类别的），
    /// 成功类结论不产生指纹——总数对不齐即记账缺陷。
    pub fn fingerprint_reconcile(&self) -> bool {
        let failed = self.counts[TaskOutcome::Failed.index()]
            .saturating_add(self.counts[TaskOutcome::Degraded.index()])
            .saturating_add(self.counts[TaskOutcome::Substituted.index()]);
        self.fingerprints.total() == failed
    }

    /// 传播守恒对账：传播账登记数 == Skipped 结论数。
    pub fn propagation_reconcile(&self) -> bool {
        self.propagation.total_skipped == self.counts[TaskOutcome::Skipped.index()]
    }

    /// 归账守恒对账：归还事件数 == 硬失败 + 跳过数（整帧拒绝时另计）。
    pub fn refund_reconcile(&self) -> bool {
        if self.frame_reject.is_some() {
            return self.refunds.len() == 1;
        }
        let expect = self.counts[TaskOutcome::Failed.index()]
            .saturating_add(self.counts[TaskOutcome::Skipped.index()]);
        self.refunds.len() as u64 == expect
    }

    /// 帧报告：一次算全（成功率万分比/失败分类/传播深度/降级数）。
    pub fn report(&self) -> FrameExecReport {
        let total: u64 = {
            let mut s = 0u64;
            let mut i = 0usize;
            while i < 6 {
                s = s.saturating_add(self.counts[i]);
                i += 1;
            }
            s
        };
        let produced = self.counts[TaskOutcome::Succeeded.index()]
            .saturating_add(self.counts[TaskOutcome::Substituted.index()])
            .saturating_add(self.counts[TaskOutcome::Degraded.index()]);
        let success_bps = if total == 0 {
            None
        } else {
            Some(produced.saturating_mul(10_000) / total)
        };
        FrameExecReport {
            total,
            produced,
            success_bps,
            failed: self.counts[TaskOutcome::Failed.index()],
            skipped: self.counts[TaskOutcome::Skipped.index()],
            degraded: self.counts[TaskOutcome::Degraded.index()],
            substituted: self.counts[TaskOutcome::Substituted.index()],
            retries_used: self.retries.consumed(),
            max_propagation_depth: self.propagation.max_depth(),
            injected: self.injected,
            frame_reject: self.frame_reject,
        }
    }

    fn record(&mut self, v: TaskVerdict) {
        let idx = v.outcome.index();
        if let Some(c) = self.counts.get_mut(idx) {
            *c = c.saturating_add(1);
        }
        // 指纹只记「未止损失败事实」（硬失败/降级/替代三类）；重试恢复
        // 的成功不产指纹——与 fingerprint_reconcile 的口径一致。
        if let Some(kind) = v.failure {
            if !matches!(v.outcome, TaskOutcome::Succeeded | TaskOutcome::NotRun) {
                if let Some(c) = self.kind_counts.get_mut(kind.index()) {
                    *c = c.saturating_add(1);
                }
            }
        }
        match v.outcome {
            TaskOutcome::Failed => self.refunds.push(RefundHook {
                task_id: v.task_id,
                reason: RefundReason::Failed,
                est_us: 0,
            }),
            TaskOutcome::Skipped => self.refunds.push(RefundHook {
                task_id: v.task_id,
                reason: RefundReason::Skipped,
                est_us: 0,
            }),
            _ => {}
        }
        self.verdicts.push(v);
    }

    /// 记录指纹（带阶段——由引擎在路由时补记；账本内此入口给测试用）。
    pub fn record_fingerprint(&mut self, kind: FailureKind, phase: FramePhase) {
        self.fingerprints.record(FailureFingerprint::of(kind, phase));
    }
}

/// 帧执行报告（汇总快照——遥测与收单报告的取数面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameExecReport {
    /// 任务总数。
    pub total: u64,
    /// 有产出的任务数（成功+替代+降级）。
    pub produced: u64,
    /// 产出率万分比（无任务时 None）。
    pub success_bps: Option<u64>,
    /// 硬失败数。
    pub failed: u64,
    /// 传播跳过数。
    pub skipped: u64,
    /// 降级执行数。
    pub degraded: u64,
    /// 替代执行数。
    pub substituted: u64,
    /// 重试消耗数。
    pub retries_used: u64,
    /// 最大传播深度。
    pub max_propagation_depth: u32,
    /// 注入次数。
    pub injected: u64,
    /// 整帧拒绝原因。
    pub frame_reject: Option<XcCode>,
}

/// 帧执行失败处置引擎。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ExecFailEngine {
    ledger: ExecLedger,
    /// 降级预算（帧级；run_frame 时重置）。
    budget: DegradationBudget,
    /// 预算耗尽策略。
    policy: DegradationPolicy,
    /// 各任务质量阶梯表（缺省用标准阶梯）。
    ladders: Vec<(u32, DegradationLadder)>,
    /// 逻辑时钟（调用方逐帧推进；注入窗口与重试时间戳用）。
    tick: u64,
}

impl ExecFailEngine {
    /// 新引擎。
    pub const fn new() -> ExecFailEngine {
        ExecFailEngine {
            ledger: ExecLedger {
                verdicts: Vec::new(),
                degraded_pairs: Vec::new(),
                counts: [0; 6],
                injected: 0,
                kind_counts: [0; 4],
                fingerprints: FingerprintLedger::new(),
                retries: RetryEngine::new(),
                propagation: PropagationEngine::new(),
                refunds: Vec::new(),
                frame_reject: None,
                reject_detail: String::new(),
            },
            budget: DegradationBudget::new(),
            policy: DegradationPolicy::SkipRest,
            ladders: Vec::new(),
            tick: 0,
        }
    }

    /// 设置帧级降级预算与策略。
    pub fn set_budget(&mut self, limit: u32, policy: DegradationPolicy) {
        self.budget = DegradationBudget::with(limit, policy);
        self.policy = policy;
    }

    /// 注册任务质量阶梯（覆盖标准阶梯）。
    pub fn set_ladder(&mut self, task_id: u32, ladder: DegradationLadder) {
        self.ladders.push((task_id, ladder));
    }

    /// 推进逻辑时钟（调用方逐帧调用；零墙钟纪律）。
    pub fn advance_tick(&mut self, n: u64) {
        self.tick = self.tick.saturating_add(n);
    }

    /// 当前 tick。
    pub const fn tick(&self) -> u64 {
        self.tick
    }

    /// 任务表完整性校验（判据二前置：拓扑/重复/替代链/依赖存在性）。
    ///
    /// 全部通过返回 `Ok(())`；任一失败返回 `Err((码, 人读明细))`——
    /// 整帧拒绝，不半帧执行。
    pub fn validate(order: &[TaskNode]) -> Result<(), (XcCode, String)> {
        // ① 重复 id 检测。
        let mut i = 0usize;
        while i < order.len() {
            let a = match order.get(i) {
                Some(n) => n,
                None => break,
            };
            let mut j = i + 1;
            while j < order.len() {
                if let Some(b) = order.get(j) {
                    if a.task_id == b.task_id {
                        return Err((
                            XcCode::DuplicateTaskId,
                            alloc::format!(
                                "任务 id {} 在执行序中出现两次（下标 {} 与 {}）",
                                a.task_id,
                                i,
                                j
                            ),
                        ));
                    }
                }
                j += 1;
            }
            i += 1;
        }
        // ② 依赖存在性 + 拓扑方向。
        let mut i = 0usize;
        while i < order.len() {
            let n = match order.get(i) {
                Some(n) => n,
                None => break,
            };
            let mut d = 0usize;
            while d < n.deps.len() {
                let dep = match n.deps.get(d) {
                    Some(x) => *x,
                    None => break,
                };
                let mut found_before = false;
                let mut p = 0usize;
                while p < i {
                    if let Some(m) = order.get(p) {
                        if m.task_id == dep {
                            found_before = true;
                            break;
                        }
                    }
                    p += 1;
                }
                if !found_before {
                    let mut found_any = false;
                    let mut q = 0usize;
                    while q < order.len() {
                        if let Some(m) = order.get(q) {
                            if m.task_id == dep {
                                found_any = true;
                                break;
                            }
                        }
                        q += 1;
                    }
                    if !found_any {
                        return Err((
                            XcCode::DependencyMissing,
                            alloc::format!(
                                "任务 {} 依赖 {}，但该 id 不在本帧任务表",
                                n.task_id,
                                dep
                            ),
                        ));
                    }
                    return Err((
                        XcCode::TopologyViolation,
                        alloc::format!(
                            "任务 {} 依赖 {}，但依赖排在更晚位置（拓扑违规）",
                            n.task_id,
                            dep
                        ),
                    ));
                }
                d += 1;
            }
            i += 1;
        }
        // ③ 替代链：存在性/自环/深度。
        let mut i = 0usize;
        while i < order.len() {
            let n = match order.get(i) {
                Some(n) => n,
                None => break,
            };
            let mut depth = 0u8;
            let mut cursor = n.task_id;
            loop {
                let node = match find_node(order, cursor) {
                    Some(x) => x,
                    None => {
                        return Err((
                            XcCode::SubstituteMissing,
                            alloc::format!(
                                "任务 {} 的替代链中出现表外 id（深度 {}）",
                                n.task_id,
                                depth
                            ),
                        ));
                    }
                };
                match node.substitute {
                    Some(next) => {
                        if next == n.task_id {
                            return Err((
                                XcCode::SubstituteCycle,
                                alloc::format!("任务 {} 的替代链回到自身", n.task_id),
                            ));
                        }
                        depth += 1;
                        if depth > MAX_SUBSTITUTE_DEPTH {
                            return Err((
                                XcCode::SubstituteTooDeep,
                                alloc::format!(
                                    "任务 {} 替代链深度 {} 超上限 {}",
                                    n.task_id,
                                    depth,
                                    MAX_SUBSTITUTE_DEPTH
                                ),
                            ));
                        }
                        cursor = next;
                    }
                    None => break,
                }
            }
            i += 1;
        }
        Ok(())
    }

    /// 推进一帧（判据二：校验→逐任务三闸路由→传播闭包）。
    ///
    /// `order` 是拓扑序任务表；`injector` 是测试注入面。校验失败时整帧
    /// 拒绝（NotRun 语义由调用方感知——不半帧执行），拒绝原因与明细
    /// 入账（[`ExecLedger::frame_reject`] / [`ExecLedger::reject_detail`]）。
    pub fn run_frame(
        &mut self,
        order: &[TaskNode],
        injector: &mut FailureInjector,
    ) -> Result<&ExecLedger, XcCode> {
        // 新帧重置降级预算；账本清空（一账一帧）。
        self.budget.reset();
        self.ledger = ExecLedger {
            verdicts: Vec::new(),
            degraded_pairs: Vec::new(),
            counts: [0; 6],
            injected: 0,
            kind_counts: [0; 4],
            fingerprints: FingerprintLedger::new(),
            retries: RetryEngine::new(),
            propagation: PropagationEngine::new(),
            refunds: Vec::new(),
            frame_reject: None,
            reject_detail: String::new(),
        };
        // 前置校验：失败即整帧拒绝 + 归还事件一条（FrameRejected）。
        if let Err((code, detail)) = Self::validate(order) {
            self.ledger.frame_reject = Some(code);
            self.ledger.reject_detail = detail;
            self.ledger.refunds.push(RefundHook {
                task_id: 0,
                reason: RefundReason::FrameRejected,
                est_us: 0,
            });
            let mut i = 0usize;
            while i < order.len() {
                if let Some(n) = order.get(i) {
                    self.ledger.record(TaskVerdict {
                        task_id: n.task_id,
                        outcome: TaskOutcome::NotRun,
                        failure: None,
                        code: Some(code),
                        detail: alloc::format!("整帧拒绝：{}", code.detail()),
                        degraded_to: None,
                        retries: 0,
                        root_cause: None,
                    });
                }
                i += 1;
            }
            return Err(code);
        }

        // 逐任务推进。
        let mut i = 0usize;
        while i < order.len() {
            let node = match order.get(i) {
                Some(n) => n.clone(),
                None => break,
            };
            // 传播检查：依赖闭包内是否有未止损失败。
            if let Some((causes, depth)) = self.propagation_cause(&node, order) {
                let cause_copy = causes.clone();
                self.ledger.record(TaskVerdict {
                    task_id: node.task_id,
                    outcome: TaskOutcome::Skipped,
                    failure: None,
                    code: None,
                    detail: alloc::format!(
                        "上游硬失败传播跳过：根因链 {:?}（深度 {}）",
                        cause_copy,
                        depth
                    ),
                    degraded_to: None,
                    retries: 0,
                    root_cause: cause_copy.first().copied(),
                });
                self.ledger
                    .propagation
                    .record(node.task_id, causes, depth);
                i += 1;
                continue;
            }
            // 注入失败（判据五：显性入账）。
            let injected_kind = injector.fail_of(node.task_id, self.tick);
            if injected_kind.is_some() {
                self.ledger.injected = self.ledger.injected.saturating_add(1);
            }
            match injected_kind {
                Some(kind) => {
                    // 重试恢复判定：退避到上限 tick 后注入是否仍命中
                    // （Always 恒命中——重试无意义；FirstN/Window 会放行）。
                    let still = injector.will_fire_next(
                        node.task_id,
                        self.tick.saturating_add(u64::from(MAX_RETRY_BACKOFF_TICKS)),
                    );
                    self.route_failure(&node, kind, still);
                }
                None => self.ledger.record(TaskVerdict {
                    task_id: node.task_id,
                    outcome: TaskOutcome::Succeeded,
                    failure: None,
                    code: None,
                    detail: "正常完成".to_string(),
                    degraded_to: None,
                    retries: 0,
                    root_cause: None,
                }),
            }
            i += 1;
        }
        Ok(&self.ledger)
    }

    /// 依赖闭包内硬失败归因：返回 (根因集合, 深度)；无失败返回 None。
    fn propagation_cause(&self, node: &TaskNode, order: &[TaskNode]) -> Option<(Vec<u32>, u32)> {
        let mut causes: Vec<u32> = Vec::new();
        let mut min_depth = u32::MAX;
        let mut d = 0usize;
        while d < node.deps.len() {
            let dep = match node.deps.get(d) {
                Some(x) => *x,
                None => break,
            };
            if let Some((depth, root)) = self.upstream_failure_depth(dep, order, 1) {
                if !causes.iter().any(|c| *c == root) {
                    causes.push(root);
                }
                if depth < min_depth {
                    min_depth = depth;
                }
            }
            d += 1;
        }
        if causes.is_empty() {
            None
        } else {
            Some((causes, min_depth))
        }
    }

    /// 上溯依赖链找硬失败根因：返回 (深度, 根因 id)。
    fn upstream_failure_depth(&self, dep: u32, order: &[TaskNode], depth: u32) -> Option<(u32, u32)> {
        let verdict = self.verdict_of(dep)?;
        match verdict.outcome {
            TaskOutcome::Failed => Some((depth, dep)),
            TaskOutcome::Skipped => {
                // 跳过的任务继续上溯其根因（防环：深度封顶 64）。
                if depth >= 64 {
                    return None;
                }
                let node = find_node(order, dep)?;
                let mut d = 0usize;
                while d < node.deps.len() {
                    let up = match node.deps.get(d) {
                        Some(x) => *x,
                        None => break,
                    };
                    if let Some((dd, root)) = self.upstream_failure_depth(up, order, depth + 1) {
                        return Some((dd, root));
                    }
                    d += 1;
                }
                None
            }
            _ => None,
        }
    }

    /// 某任务的处置结论（已出账才有）。
    pub fn verdict_of(&self, task_id: u32) -> Option<&TaskVerdict> {
        let mut i = 0usize;
        while i < self.ledger.verdicts.len() {
            if let Some(v) = self.ledger.verdicts.get(i) {
                if v.task_id == task_id {
                    return Some(v);
                }
            }
            i += 1;
        }
        None
    }

    /// 失败四闸路由（判据一、二：重试闸→替代闸→降级闸→硬失败传播）。
    fn route_failure(&mut self, node: &TaskNode, kind: FailureKind, still_injected: bool) {
        // 闸一：重试（仅可重试类别；预算耗尽转下一闸并记账）。
        if kind.is_retryable() {
            let attempt = self.ledger.retries.consumed_of(node.task_id).saturating_add(1);
            let budget = if node.retry_budget == 0 {
                kind.default_retry_limit()
            } else {
                node.retry_budget
            };
            if attempt <= budget && budget > 0 {
                let backoff = kind
                    .retry_backoff_ticks(attempt)
                    .min(MAX_RETRY_BACKOFF_TICKS);
                self.ledger.retries.decide(node, kind, attempt, self.tick);
                self.ledger
                    .retries
                    .resolve(node.task_id, attempt, !still_injected);
                if !still_injected {
                    self.record_with_phase(TaskVerdict {
                        task_id: node.task_id,
                        outcome: TaskOutcome::Succeeded,
                        failure: Some(kind),
                        code: None,
                        detail: alloc::format!(
                            "重试第 {} 次恢复（退避 {} tick 后放行）：{}",
                            attempt,
                            backoff,
                            kind.code().detail()
                        ),
                        degraded_to: None,
                        retries: attempt,
                        root_cause: None,
                    }, node.phase);
                    return;
                }
            }
        }
        // 闸二：替代（替代链深度已在 validate 拦截；此处记替代成功）。
        if let Some(sub_id) = node.substitute {
            // 替代任务自身若也硬失败（已出账），替代闸失效——落下一闸。
            let sub_failed = match self.verdict_of(sub_id) {
                Some(v) => v.outcome == TaskOutcome::Failed,
                None => false,
            };
            if !sub_failed {
                self.record_with_phase(TaskVerdict {
                    task_id: node.task_id,
                    outcome: TaskOutcome::Substituted,
                    failure: Some(kind),
                    code: Some(kind.code()),
                    detail: alloc::format!(
                        "{}；替代任务 {} 顶上，传播在此截断",
                        kind.code().detail(),
                        sub_id
                    ),
                    degraded_to: None,
                    retries: self.ledger.retries.consumed_of(node.task_id),
                    root_cause: None,
                }, node.phase);
                return;
            }
        }
        // 闸三：降级（预算内走阶梯降档；耗尽按策略处置）。
        if node.degradable {
            let ladder = self.ladder_of(node.task_id);
            if ladder.steps.is_empty() {
                self.record_with_phase(TaskVerdict {
                    task_id: node.task_id,
                    outcome: TaskOutcome::Failed,
                    failure: Some(kind),
                    code: Some(XcCode::DegradationLadderEmpty),
                    detail: alloc::format!(
                        "{}；可降级但阶梯为空，转硬失败",
                        kind.code().detail()
                    ),
                    degraded_to: None,
                    retries: self.ledger.retries.consumed_of(node.task_id),
                    root_cause: None,
                }, node.phase);
                return;
            }
            let used = self.ledger.counts[TaskOutcome::Degraded.index()] as u32;
            match self.budget.request() {
                Some(_) => {
                    let tier = ladder.step_down(used.saturating_add(1));
                    self.record_with_phase(TaskVerdict {
                        task_id: node.task_id,
                        outcome: TaskOutcome::Degraded,
                        failure: Some(kind),
                        code: Some(kind.code()),
                        detail: alloc::format!(
                            "{}；走降级路径：{}（预算 {}）",
                            kind.code().detail(),
                            tier.declaration(),
                            self.budget.used()
                        ),
                        degraded_to: Some(tier),
                        retries: self.ledger.retries.consumed_of(node.task_id),
                        root_cause: None,
                    }, node.phase);
                    self.ledger.degraded_pairs.push((
                        node.task_id,
                        alloc::format!("{}{}", FAIL_METRIC_PREFIX, kind.metric_tail()),
                        tier.declaration().to_string(),
                    ));
                }
                None => {
                    // 预算耗尽：按策略处置（显性，不静默）。
                    let (outcome, code, why) = match self.policy {
                        DegradationPolicy::FailHard => (
                            TaskOutcome::Failed,
                            XcCode::DegradationBudgetExhausted,
                            "预算耗尽按策略转硬失败",
                        ),
                        DegradationPolicy::SkipRest => (
                            TaskOutcome::Skipped,
                            XcCode::DegradationBudgetExhausted,
                            "预算耗尽按策略转跳过（产出完整性优先）",
                        ),
                    };
                    self.record_with_phase(TaskVerdict {
                        task_id: node.task_id,
                        outcome,
                        failure: Some(kind),
                        code: Some(code),
                        detail: alloc::format!("{}；{}（已用 {}/{}）", kind.code().detail(), why, self.budget.used(), self.budget.limit),
                        degraded_to: None,
                        retries: self.ledger.retries.consumed_of(node.task_id),
                        root_cause: None,
                    }, node.phase);
                }
            }
            return;
        }
        // 硬失败：传播由 propagation_cause 的依赖闭包承担。
        self.record_with_phase(TaskVerdict {
            task_id: node.task_id,
            outcome: TaskOutcome::Failed,
            failure: Some(kind),
            code: Some(kind.code()),
            detail: alloc::format!(
                "{}；三道止损闸皆无，下游传递依赖全部跳过。建议：{}",
                kind.code().detail(),
                kind.suggestion()
            ),
            degraded_to: None,
            retries: self.ledger.retries.consumed_of(node.task_id),
            root_cause: None,
        }, node.phase);
    }

    /// 注入器预判残留：已由 run_frame 以 [`FailureInjector::will_fire_next`]
    /// 计算并传入 route_failure（本引擎不持有注入表——单一事实源）。

    /// 任务阶梯（未注册用标准阶梯）。
    fn ladder_of(&self, task_id: u32) -> DegradationLadder {
        let mut i = 0usize;
        while i < self.ladders.len() {
            if let Some((id, l)) = self.ladders.get(i) {
                if *id == task_id {
                    return l.clone();
                }
            }
            i += 1;
        }
        DegradationLadder::standard()
    }

    /// 当前账（只读）。
    pub const fn ledger(&self) -> &ExecLedger {
        &self.ledger
    }

    /// 引擎侧统一出账：结论入账 + 未止损失败事实的指纹入账
    /// （阶段来自节点声明——单一来源，账本不存阶段表）。
    fn record_with_phase(&mut self, v: TaskVerdict, phase: FramePhase) {
        let fp = match v.outcome {
            TaskOutcome::Failed | TaskOutcome::Degraded | TaskOutcome::Substituted => {
                v.failure.map(|k| FailureFingerprint::of(k, phase))
            }
            _ => None,
        };
        self.ledger.record(v);
        if let Some(fp) = fp {
            self.ledger.fingerprints.record(fp);
        }
    }

    /// 当前降级预算快照（只读）。
    pub const fn budget(&self) -> &DegradationBudget {
        &self.budget
    }
}

fn find_node(order: &[TaskNode], task_id: u32) -> Option<&TaskNode> {
    let mut i = 0usize;
    while i < order.len() {
        if let Some(n) = order.get(i) {
            if n.task_id == task_id {
                return Some(n);
            }
        }
        i += 1;
    }
    None
}

// ---------------------------------------------------------------------------
// 十二、任务族指纹维度（失败风暴按任务族聚合的补充维度）
// ---------------------------------------------------------------------------

/// 任务族（id 高 24 位相同的任务归一族——帧图构建器的族编号约定）。
pub const fn task_family(task_id: u32) -> u32 {
    task_id >> 8
}

/// 族内序号。
pub const fn task_slot(task_id: u32) -> u8 {
    (task_id & 0xFF) as u8
}

/// 失败风暴判定：同指纹计数超过阈值（默认 16）即风暴——
/// 上游应触发熔断（F0165 周界联动的 B 域信号面）。
pub const FAILURE_STORM_THRESHOLD: u64 = 16;

/// 风暴判定（指纹账 → 是否有指纹超阈值）。
pub fn has_failure_storm(ledger: &ExecLedger) -> Option<FailureFingerprint> {
    let entries = ledger.fingerprints.entries();
    let mut i = 0usize;
    while i < entries.len() {
        if let Some((fp, n)) = entries.get(i) {
            if *n >= FAILURE_STORM_THRESHOLD {
                return Some(*fp);
            }
        }
        i += 1;
    }
    None
}

/// 失败率万分比（硬失败 + 跳过 / 总数；无任务时 None）。
pub fn failure_rate_bps(ledger: &ExecLedger) -> Option<u64> {
    let report = ledger.report();
    if report.total == 0 {
        return None;
    }
    let bad = report.failed.saturating_add(report.skipped);
    Some(bad.saturating_mul(10_000) / report.total)
}
