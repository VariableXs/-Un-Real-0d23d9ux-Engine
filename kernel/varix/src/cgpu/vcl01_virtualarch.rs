//! CGPU-F1761 · L 域开工与虚拟化总架构（CGPU-L 域 · 虚拟化 GPU · 批次 L01 · 开山单）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F1761`
//!
//! 域使命：虚拟化 GPU——让 GPU 能力在虚拟机/容器/云场景下被安全高效地
//! 共享与使用：**虚拟化不是二等公民——虚拟化平等声明**。K 域移交包签收
//! 记录（F1757 签收——签收兑现起点）；I09 虚拟化带宽预留兑现声明（预留
//! 兑现起点）；官方主题映射（虚拟化模式/vGPU 抽象/直通/分区/半虚拟化/
//! 显示/迁移/遥测——十组映射表）；架构五段（环境检测→模式选择→抽象接
//! 入→性能保障→观测调试）；与 K01 关系（ComputeDevice trait 复用为
//! vGPU 基础——复用声明，F1763 扩展）；场景族（桌面虚拟化/云游戏/容器
//! GPU/AI 多租户）；不变量（虚拟化不破坏能达成的合同——模式对应合同
//! 等级表，合同适配复用 F1466 模式）；风险四条（性能损耗/兼容碎片/
//! 隔离逃逸/调度公平）各配预案。
//!
//! ## 要点一：平等声明是可机检的账
//!
//! 「虚拟化不是二等公民」落为模式×合同等级表：每种虚拟化模式对应的
//! 合同等级由表声明，表外无暗降——等级与裸机不一致处必须显性登记
//! 而不是默默缩水。
//!
//! ## 要点二：签收与预留是兑现起点，不是过场
//!
//! K 域移交包（F1757）签收记录与 I09 带宽预留兑现声明各为结构化记录
//! （来源/内容/状态/指纹）——签收缺项或预留悬空即域未开工。
//!
//! ## 要点三：十组映射取官方批次标题逐字
//!
//! L 域十组（L01 虚拟化总架构与抽象 … L10 收口）区间连续无缺口守恒
//! 160 项；八官方主题在映射表中全覆盖（判据侧重算）。
//!
//! ## 要点四：五段流水线单向
//!
//! 环境检测→模式选择→抽象接入→性能保障→观测调试；跳段（未检测就选
//! 模式）与回退（观测逆向驱动接入）均违约。
//!
//! ## 要点五：复用不重建
//!
//! vGPU 抽象以 K01 ComputeDevice trait 为基础（复用声明，F1763 扩展
//! 兑现）；合同适配复用 F1466 模式——重建即分叉，分叉即漂移。
//!
//! ## 要点六：风险四条各配预案
//!
//! 性能损耗/兼容碎片/隔离逃逸/调度公平——预案互异且非空；无预案的
//! 风险等于事故邀请函。
//!
//! ## 要点七：诊断码独占 0x53xx 段
//!
//! 与 F1121（0x50xx）/cga02（0x51xx）/F1441（0x52xx）/F0001（0x39xx
//! 细分）互不重叠。

// ---------------------------------------------------------------------------
// 导入（no_std 三件套）
// ---------------------------------------------------------------------------

use alloc::string::{String, ToString};

// ---------------------------------------------------------------------------
// 一、域使命与平等声明（字面量冻结）
// ---------------------------------------------------------------------------

/// L 域域使命三条款（字面量冻结——判据独立对拍）。
pub const MISSION_CLAUSES: [&str; 3] = [
    "让 GPU 能力在虚拟机/容器/云场景下被安全高效地共享与使用",
    "虚拟化不是二等公民——虚拟化平等声明",
    "虚拟化不破坏能达成的合同",
];

/// L 域起始任务号（本单）。
pub const L_DOMAIN_FIRST: u32 = 1761;
/// L 域终止任务号（含）。
pub const L_DOMAIN_LAST: u32 = 1920;
/// 域任务总数守恒（10 组 × 16 项）。
pub const L_DOMAIN_TOTAL: u32 = 160;

// ---------------------------------------------------------------------------
// 二、官方主题与十组映射
// ---------------------------------------------------------------------------

/// 官方主题八字面量（判据独立对拍）。
pub const OFFICIAL_THEMES: [&str; 8] = [
    "虚拟化模式",
    "vGPU 抽象",
    "直通",
    "分区",
    "半虚拟化",
    "显示",
    "迁移",
    "遥测",
];

/// 十组规划条目（官方批次标题逐字 + 主题覆盖标注）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GroupPlan {
    /// 组号（L01..L10）。
    pub group: u8,
    /// 官方组名（批次标题逐字）。
    pub name: &'static str,
    /// 本组覆盖的官方主题下标（跨主题/流程组为空）。
    pub themes: &'static [usize],
    /// 起始任务号（含）。
    pub first: u32,
    /// 终止任务号（含）。
    pub last: u32,
}

/// 官方十组规划表（组名取施工书批次标题逐字；八主题全覆盖判据侧重算）。
pub const GROUP_PLANS: [GroupPlan; 10] = [
    GroupPlan { group: 1, name: "虚拟化总架构与抽象组", themes: &[0, 1], first: 1761, last: 1776 },
    GroupPlan { group: 2, name: "GPU 直通与分区组", themes: &[2, 3], first: 1777, last: 1792 },
    GroupPlan { group: 3, name: "virtio-gpu 半虚拟化组", themes: &[4], first: 1793, last: 1808 },
    GroupPlan { group: 4, name: "虚拟化场景与优化组", themes: &[], first: 1809, last: 1824 },
    GroupPlan { group: 5, name: "虚拟化显示与多媒体组", themes: &[5], first: 1825, last: 1840 },
    GroupPlan { group: 6, name: "虚拟化安全与多租户组", themes: &[], first: 1841, last: 1856 },
    GroupPlan { group: 7, name: "虚拟化遥测与调试组", themes: &[7], first: 1857, last: 1872 },
    GroupPlan { group: 8, name: "虚拟化场景生态组", themes: &[6], first: 1873, last: 1888 },
    GroupPlan { group: 9, name: "L 域预备与自查组", themes: &[], first: 1889, last: 1904 },
    GroupPlan { group: 10, name: "L 域收口组", themes: &[], first: 1905, last: 1920 },
];

// ---------------------------------------------------------------------------
// 三、K 域移交签收与 I09 预留兑现（结构化记录，兑现起点）
// ---------------------------------------------------------------------------

/// 交付记录状态（闭集）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordStatus {
    /// 已签收/已兑现。
    Settled,
    /// 待兑现（预留已声明、任务未完成）。
    Pending,
}

/// K 域移交包签收记录（F1757 签收——签收兑现起点）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HandoverReceipt {
    /// 来源任务号（K 域收口）。
    pub source: u32,
    /// 移交包内容项数。
    pub items: u32,
    /// 状态。
    pub status: RecordStatus,
}

/// I09 虚拟化带宽预留兑现声明（预留兑现起点）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReservePromise {
    /// 来源组（I09）。
    pub from: &'static str,
    /// 预留内容。
    pub what: &'static str,
    /// 状态（兑现任务在本域后续批次完成）。
    pub status: RecordStatus,
}

/// K 域移交包签收（字面量钉死——判据对来源与项数独立写死）。
pub const K_HANDOVER: HandoverReceipt = HandoverReceipt {
    source: 1757,
    items: 4,
    status: RecordStatus::Settled,
};

/// I09 带宽预留兑现声明（字面量钉死）。
pub const I_RESERVE: ReservePromise = ReservePromise {
    from: "I09",
    what: "虚拟化带宽预留→本域对接",
    status: RecordStatus::Pending,
};

// ---------------------------------------------------------------------------
// 四、架构五段（单向流水线）
// ---------------------------------------------------------------------------

/// 虚拟化架构五段（官方闭集）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PipelineStage {
    /// 环境检测。
    EnvDetect,
    /// 模式选择。
    ModeSelect,
    /// 抽象接入。
    AbstractionAttach,
    /// 性能保障。
    PerfAssure,
    /// 观测调试。
    ObserveDebug,
}

impl PipelineStage {
    /// 全部五段（单向序）。
    pub const ALL: [PipelineStage; 5] = [
        PipelineStage::EnvDetect,
        PipelineStage::ModeSelect,
        PipelineStage::AbstractionAttach,
        PipelineStage::PerfAssure,
        PipelineStage::ObserveDebug,
    ];

    /// 段下标。
    pub const fn index(self) -> usize {
        match self {
            PipelineStage::EnvDetect => 0,
            PipelineStage::ModeSelect => 1,
            PipelineStage::AbstractionAttach => 2,
            PipelineStage::PerfAssure => 3,
            PipelineStage::ObserveDebug => 4,
        }
    }

    /// 人话标签。
    pub fn label(self) -> String {
        match self {
            PipelineStage::EnvDetect => "环境检测",
            PipelineStage::ModeSelect => "模式选择",
            PipelineStage::AbstractionAttach => "抽象接入",
            PipelineStage::PerfAssure => "性能保障",
            PipelineStage::ObserveDebug => "观测调试",
        }
        .to_string()
    }
}

/// 相邻段迁移合法性（只许 i → i+1；跳段/回退均非法）。
pub fn stage_transition(from: PipelineStage, to: PipelineStage) -> bool {
    to.index() == from.index() + 1
}

// ---------------------------------------------------------------------------
// 五、复用声明与场景族（复用不重建；四场景闭集）
// ---------------------------------------------------------------------------

/// 复用声明（K01 ComputeDevice trait 复用为 vGPU 基础；F1763 扩展兑现）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReuseDeclaration {
    /// 上游来源（K01）。
    pub upstream: u32,
    /// 复用物。
    pub what: &'static str,
    /// 扩展兑现任务号。
    pub extend_at: u32,
}

/// vGPU 复用声明（字面量钉死——判据独立对拍）。
pub const VGPU_REUSE: ReuseDeclaration = ReuseDeclaration {
    upstream: 1601,
    what: "ComputeDevice trait 复用为 vGPU 基础",
    extend_at: 1763,
};

/// 合同适配复用声明（F1466 模式）。
pub const CONTRACT_ADAPTER_REUSE: ReuseDeclaration = ReuseDeclaration {
    upstream: 1466,
    what: "合同适配复用 F1466 模式",
    extend_at: 1776,
};

/// 虚拟化场景族（官方四场景闭集）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScenarioFamily {
    /// 桌面虚拟化。
    DesktopVdi,
    /// 云游戏。
    CloudGaming,
    /// 容器 GPU。
    ContainerGpu,
    /// AI 多租户。
    AiMultiTenant,
}

/// 场景族总数。
pub const SCENARIO_COUNT: usize = 4;

impl ScenarioFamily {
    /// 全部场景（官方序）。
    pub const ALL: [ScenarioFamily; SCENARIO_COUNT] = [
        ScenarioFamily::DesktopVdi,
        ScenarioFamily::CloudGaming,
        ScenarioFamily::ContainerGpu,
        ScenarioFamily::AiMultiTenant,
    ];

    /// 人话标签。
    pub fn label(self) -> String {
        match self {
            ScenarioFamily::DesktopVdi => "桌面虚拟化",
            ScenarioFamily::CloudGaming => "云游戏",
            ScenarioFamily::ContainerGpu => "容器 GPU",
            ScenarioFamily::AiMultiTenant => "AI 多租户",
        }
        .to_string()
    }
}

// ---------------------------------------------------------------------------
// 六、模式×合同等级表（平等声明的可机检面；合同适配复用 F1466 模式）
// ---------------------------------------------------------------------------

/// 合同等级（三档闭集——复用 F1466 模式的等级语义）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContractLevel {
    /// 全量合同（与裸机一致）。
    Full,
    /// 标准合同（保证帧率下限与降质通知）。
    Standard,
    /// 尽力合同（无硬承诺，显性标注）。
    BestEffort,
}

/// 虚拟化模式（官方闭集）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VirtMode {
    /// 直通（独占，等价裸机）。
    Passthrough,
    /// 分区（SR-IOV 类）。
    Partitioned,
    /// 半虚拟化（virtio-gpu 类）。
    Paravirtual,
}

/// 模式→合同等级表（平等声明的核心账——表外无暗降）。
pub fn contract_level_of(mode: VirtMode) -> ContractLevel {
    match mode {
        VirtMode::Passthrough => ContractLevel::Full,
        VirtMode::Partitioned => ContractLevel::Standard,
        VirtMode::Paravirtual => ContractLevel::Standard,
    }
}

/// 模式总数（表完备性判据用）。
pub const VIRT_MODE_COUNT: usize = 3;

// ---------------------------------------------------------------------------
// 七、风险四条（各配预案，互异非空）
// ---------------------------------------------------------------------------

/// L 域风险四条（官方闭集）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RiskKind {
    /// 性能损耗。
    PerfLoss,
    /// 兼容碎片。
    CompatFragmentation,
    /// 隔离逃逸。
    IsolationEscape,
    /// 调度公平。
    SchedulingFairness,
}

/// 风险总数。
pub const RISK_COUNT: usize = 4;

impl RiskKind {
    /// 全部风险（官方序）。
    pub const ALL: [RiskKind; RISK_COUNT] = [
        RiskKind::PerfLoss,
        RiskKind::CompatFragmentation,
        RiskKind::IsolationEscape,
        RiskKind::SchedulingFairness,
    ];

    /// 配置的预案（人话——判据断非空且逐条互异）。
    pub fn fallback(self) -> String {
        match self {
            RiskKind::PerfLoss => "预案：模式×场景性能基线实测入册，损耗超阈显性标注并降合同".to_string(),
            RiskKind::CompatFragmentation => "预案：驱动/内核版本差异表驱动，未测组合走保守路径".to_string(),
            RiskKind::IsolationEscape => "预案：直写路径类型上封闭+越权访问立案，隔离红线随视图走".to_string(),
            RiskKind::SchedulingFairness => "预案：租户配额记账+饥饿检测，公平性指标入遥测".to_string(),
        }
    }
}

// ---------------------------------------------------------------------------
// 八、错误契约（独占 0x53xx 段）
// ---------------------------------------------------------------------------

/// L01 诊断码。独占 `0x53xx` 段。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VlCode(pub u16);

impl VlCode {
    /// 模式越界（合同表外模式）。
    pub const MODE_OUT_OF_TABLE: VlCode = VlCode(0x5301);
    /// 合同暗降（等级与模式表不符——平等声明违约）。
    pub const CONTRACT_DOWNGRADE: VlCode = VlCode(0x5302);
    /// 签收/预留记录缺项（域未开工）。
    pub const RECORDS_INCOMPLETE: VlCode = VlCode(0x5303);
    /// 复用漂移（复用物被重建而非扩展）。
    pub const REUSE_DRIFT: VlCode = VlCode(0x5304);
    /// 场景越界（四场景族外）。
    pub const SCENARIO_OUT_OF_TABLE: VlCode = VlCode(0x5305);

    /// wire 码。
    pub const fn code(self) -> u16 {
        self.0
    }

    /// 人话原因。
    pub fn reason(self) -> String {
        match self {
            VlCode::MODE_OUT_OF_TABLE => "虚拟化模式越界：模式×合同表外不得立模式".into(),
            VlCode::CONTRACT_DOWNGRADE => "合同暗降：等级与模式表不符，平等声明违约".into(),
            VlCode::RECORDS_INCOMPLETE => "签收/预留记录缺项：域未开工".into(),
            VlCode::REUSE_DRIFT => "复用漂移：复用物被重建而非扩展".into(),
            VlCode::SCENARIO_OUT_OF_TABLE => "场景越界：四场景族外不得立场景".into(),
            VlCode(_) => "未知 L01 虚拟化域诊断码".into(),
        }
    }
}

// ---------------------------------------------------------------------------
// 九、测试支撑（回归用例与断言）
// ---------------------------------------------------------------------------

#[cfg(all(test, not(no_std)))]
mod tests {
    use super::*;

    #[test]
    fn 十组映射连续守恒() {
        let mut total = 0;
        for i in 0..GROUP_PLANS.len() {
            let g = &GROUP_PLANS[i];
            total += g.last - g.first + 1;
            if i > 0 {
                assert_eq!(g.first, GROUP_PLANS[i - 1].last + 1);
            }
        }
        assert_eq!(total, L_DOMAIN_TOTAL);
    }

    #[test]
    fn 直通模式全量合同() {
        assert_eq!(contract_level_of(VirtMode::Passthrough), ContractLevel::Full);
        assert_ne!(contract_level_of(VirtMode::Paravirtual), ContractLevel::Full);
    }

    #[test]
    fn 流水线只许单向一步() {
        for i in 0..4 {
            assert!(stage_transition(PipelineStage::ALL[i], PipelineStage::ALL[i + 1]));
        }
        assert!(!stage_transition(PipelineStage::ModeSelect, PipelineStage::PerfAssure));
    }

    #[test]
    fn 四场景族闭集() {
        assert_eq!(ScenarioFamily::ALL.len(), SCENARIO_COUNT);
        assert_eq!(ScenarioFamily::ALL[0].label(), "桌面虚拟化");
    }

    #[test]
    fn 签收已定预留待兑现() {
        assert_eq!(K_HANDOVER.status, RecordStatus::Settled);
        assert_eq!(I_RESERVE.status, RecordStatus::Pending);
    }
}
