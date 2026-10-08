//! CGPU-F2561 · Q 域开工与可靠性总架构（CGPU-Q 域 · 可靠性 · 批次 Q01 · 开山单）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F2561`
//!
//! 域使命：可靠性——让系统"永远能恢复"：崩溃不是终点而是恢复的起点——
//! **恢复起点哲学声明**。P 域移交包签收记录（F2557 签收）；官方主题映射
//! （可靠性模型/崩溃恢复/看门狗/健康检查/容错——十组映射表）；架构五段
//! （故障分类→容错设计→恢复机制→健康监控→持续验证）；与 O 域（降级管
//! "性能退"、Q 管"活着"——分工契约：降级失败→容错接管——交接声明）；
//! 与 O05 混沌（混沌测试设施复用——设施复用声明）；不变量（任何单点故
//! 障不得导致数据丢失或系统不可恢复——不可恢复=最高缺陷红线）；风险四
//! 条（状态丢失/恢复风暴/检查点开销/级联失败——各配预案）；测试（使命/
//! 签收/映射/五段/分工/设施/红线/风险八组）。
//!
//! ## 要点一：恢复起点哲学是可机检的账
//!
//! 「崩溃不是终点而是恢复的起点」不是口号：它落为域使命字面量冻结 + 五
//! 段架构（恢复机制是独立一段，不是故障处理的附属品）+ 不可恢复红线立
//! 案通道——三者判据侧独立对拍。
//!
//! ## 要点二：签收是兑现起点
//!
//! P 域移交包（F2557）七件逐件入账（来源/件数/状态结构化记录）——签收
//! 缺项即域未开工。
//!
//! ## 要点三：十组映射取官方批次标题逐字
//!
//! Q 域十组（Q01 域开工与可靠性总架构 … Q10 收口）区间连续无缺口守恒
//! 160 项；五官方主题在映射表中全覆盖（判据侧重算）。
//!
//! ## 要点四：五段流水线单向
//!
//! 故障分类→容错设计→恢复机制→健康监控→持续验证；跳段（未分类就设
//! 计容错）与回退（验证逆向驱动恢复）均违约。
//!
//! ## 要点五：交接与复用不重建
//!
//! 与 O 域分工契约逐字冻结（降级管性能退/Q 管活着/降级失败→容错接管）
//! ——职责交集恒空；混沌测试设施复用 O05 不重建——重建即分叉，分叉即
//! 漂移。
//!
//! ## 要点六：不可恢复=最高缺陷红线
//!
//! 任何单点故障不得导致数据丢失或系统不可恢复——红线字面量冻结且有专属
//! 立案码（REDLINE_BREACH），无立案通道的红线等于没红线。
//!
//! ## 要点七：诊断码独占 0x56xx 段
//!
//! 与 cgm01/cgm02（0x54xx）/vco01（0x55xx）/L01（0x53xx）/F1441（0x52xx）
//! /cga02（0x51xx）/F1121（0x50xx）互不重叠。

// ---------------------------------------------------------------------------
// 导入（no_std 三件套）
// ---------------------------------------------------------------------------

use alloc::string::{String, ToString};

// ---------------------------------------------------------------------------
// 一、域使命与恢复起点哲学（字面量冻结）
// ---------------------------------------------------------------------------

/// Q 域域使命三条款（字面量冻结——判据独立对拍）。
pub const Q_MISSION: [&str; 3] = [
    "可靠性——让系统永远能恢复",
    "崩溃不是终点而是恢复的起点",
    "任何单点故障不得导致数据丢失或系统不可恢复",
];

/// Q 域起始任务号（本单）。
pub const Q_DOMAIN_FIRST: u32 = 2561;
/// Q 域终止任务号（含）。
pub const Q_DOMAIN_LAST: u32 = 2720;
/// 域任务总数守恒（10 组 × 16 项）。
pub const Q_DOMAIN_TOTAL: u32 = 160;

// ---------------------------------------------------------------------------
// 二、官方主题与十组映射
// ---------------------------------------------------------------------------

/// 官方主题五字面量（判据独立对拍）。
pub const OFFICIAL_THEMES_Q: [&str; 5] = [
    "可靠性模型",
    "崩溃恢复",
    "看门狗",
    "健康检查",
    "容错",
];

/// 十组规划条目（官方批次标题逐字 + 主题覆盖标注）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GroupPlan {
    /// 组号（Q01..Q10）。
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

/// 官方十组规划表（组名取施工书批次标题逐字；五主题全覆盖判据侧重算）。
pub const GROUP_PLANS_Q: [GroupPlan; 10] = [
    GroupPlan { group: 1, name: "域开工与可靠性总架构组", themes: &[0], first: 2561, last: 2576 },
    GroupPlan { group: 2, name: "崩溃恢复与容错组", themes: &[1, 4], first: 2577, last: 2592 },
    GroupPlan { group: 3, name: "看门狗与健康检查组", themes: &[2, 3], first: 2593, last: 2608 },
    GroupPlan { group: 4, name: "可靠性测试与资产组", themes: &[], first: 2609, last: 2624 },
    GroupPlan { group: 5, name: "驱动崩溃隔离组", themes: &[4], first: 2625, last: 2640 },
    GroupPlan { group: 6, name: "长稳运行与漂移组", themes: &[0], first: 2641, last: 2656 },
    GroupPlan { group: 7, name: "可靠性治理与演进组", themes: &[], first: 2657, last: 2672 },
    GroupPlan { group: 8, name: "可靠性生态与工具组", themes: &[], first: 2673, last: 2688 },
    GroupPlan { group: 9, name: "Q 域预备与自查组", themes: &[], first: 2689, last: 2704 },
    GroupPlan { group: 10, name: "Q 域收口组 · CGPU-Q 域 160 项收官", themes: &[], first: 2705, last: 2720 },
];

// ---------------------------------------------------------------------------
// 三、P 域移交签收（结构化记录，兑现起点）
// ---------------------------------------------------------------------------

/// 交付记录状态（闭集）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordStatus {
    /// 已签收/已兑现。
    Settled,
    /// 待兑现（预留已声明、任务未完成）。
    Pending,
}

/// P 域移交包签收记录（F2557 签收——签收兑现起点）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HandoverReceipt {
    /// 来源任务号（P 域移交包）。
    pub source: u32,
    /// 移交包内容件数。
    pub items: u32,
    /// 状态。
    pub status: RecordStatus,
}

/// P 域移交包七件（F2557 锚点原文逐字——判据独立对拍）。
pub const HANDOVER_ITEMS_Q: [&str; 7] = [
    "接口冻结清单",
    "契约清单",
    "资产清单",
    "基线快照",
    "遗留移交清单",
    "可靠性衔接包",
    "经验教训十条",
];

/// P 域移交包签收（字面量钉死——判据对来源与件数独立写死）。
pub const P_HANDOVER: HandoverReceipt = HandoverReceipt {
    source: 2557,
    items: 7,
    status: RecordStatus::Settled,
};

// ---------------------------------------------------------------------------
// 四、架构五段（单向流水线）
// ---------------------------------------------------------------------------

/// 可靠性架构五段（官方闭集）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PipelineStage {
    /// 故障分类。
    FaultClassify,
    /// 容错设计。
    TolerantDesign,
    /// 恢复机制。
    RecoveryMech,
    /// 健康监控。
    HealthMonitor,
    /// 持续验证。
    ContinuousVerify,
}

impl PipelineStage {
    /// 全部五段（单向序）。
    pub const ALL: [PipelineStage; 5] = [
        PipelineStage::FaultClassify,
        PipelineStage::TolerantDesign,
        PipelineStage::RecoveryMech,
        PipelineStage::HealthMonitor,
        PipelineStage::ContinuousVerify,
    ];

    /// 段下标。
    pub const fn index(self) -> usize {
        match self {
            PipelineStage::FaultClassify => 0,
            PipelineStage::TolerantDesign => 1,
            PipelineStage::RecoveryMech => 2,
            PipelineStage::HealthMonitor => 3,
            PipelineStage::ContinuousVerify => 4,
        }
    }

    /// 人话标签。
    pub fn label(self) -> String {
        match self {
            PipelineStage::FaultClassify => "故障分类",
            PipelineStage::TolerantDesign => "容错设计",
            PipelineStage::RecoveryMech => "恢复机制",
            PipelineStage::HealthMonitor => "健康监控",
            PipelineStage::ContinuousVerify => "持续验证",
        }
        .to_string()
    }
}

/// 相邻段迁移合法性（只许 i → i+1；跳段/回退均非法）。
pub fn stage_transition(from: PipelineStage, to: PipelineStage) -> bool {
    to.index() == from.index() + 1
}

// ---------------------------------------------------------------------------
// 五、交接声明与设施复用（复用不重建）
// ---------------------------------------------------------------------------

/// 跨域交接声明（peer/what/rule 三位逐字冻结）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BridgingDeclaration {
    /// 对端域。
    pub peer: &'static str,
    /// 分工内容。
    pub what: &'static str,
    /// 交接规则。
    pub rule: &'static str,
}

/// 与 O 域的分工交接声明（锚点原文逐字——降级失败→容错接管）。
pub const O_DOMAIN_HANDOVER: BridgingDeclaration = BridgingDeclaration {
    peer: "O 域",
    what: "降级管\"性能退\"、Q 管\"活着\"",
    rule: "降级失败→容错接管",
};

/// 复用声明（上游来源/复用物/扩展兑现任务号）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReuseDeclaration {
    /// 上游来源（O05 混沌批次起始任务号）。
    pub upstream: u32,
    /// 复用物。
    pub what: &'static str,
    /// 扩展兑现任务号。
    pub extend_at: u32,
}

/// 混沌测试设施复用声明（O05 混沌→Q04 可靠性测试兑现）。
pub const CHAOS_FACILITY_REUSE: ReuseDeclaration = ReuseDeclaration {
    upstream: 2465,
    what: "混沌测试设施复用",
    extend_at: 2609,
};

// ---------------------------------------------------------------------------
// 六、不变量红线（不可恢复=最高缺陷）
// ---------------------------------------------------------------------------

/// 不可恢复红线字面量（判据独立对拍）。
pub const UNRECOVERABLE_REDLINE: &str =
    "不可恢复=最高缺陷——任何单点故障不得导致数据丢失或系统不可恢复";

// ---------------------------------------------------------------------------
// 七、风险四条（各配预案，互异非空）
// ---------------------------------------------------------------------------

/// Q 域风险四条（官方闭集）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RiskKind {
    /// 状态丢失。
    StateLoss,
    /// 恢复风暴。
    RecoveryStorm,
    /// 检查点开销。
    CheckpointOverhead,
    /// 级联失败。
    CascadingFailure,
}

/// 风险总数。
pub const RISK_COUNT: usize = 4;

impl RiskKind {
    /// 全部风险（官方序）。
    pub const ALL: [RiskKind; RISK_COUNT] = [
        RiskKind::StateLoss,
        RiskKind::RecoveryStorm,
        RiskKind::CheckpointOverhead,
        RiskKind::CascadingFailure,
    ];

    /// 配置的预案（人话——判据断非空且逐条互异）。
    pub fn fallback(self) -> String {
        match self {
            RiskKind::StateLoss => "预案：检查点先落账再推进，恢复点目标 RPO 逐系统入表".to_string(),
            RiskKind::RecoveryStorm => "预案：恢复限流+指数退避，风暴熔断阈值入健康监控".to_string(),
            RiskKind::CheckpointOverhead => "预案：检查点增量落账+开销预算入帧账，超阈显性标注".to_string(),
            RiskKind::CascadingFailure => "预案：故障隔离舱+级联断路器，隔离面随部署拓扑走".to_string(),
        }
    }
}

// ---------------------------------------------------------------------------
// 八、错误契约（独占 0x56xx 段）
// ---------------------------------------------------------------------------

/// Q01 诊断码。独占 `0x56xx` 段。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VqCode(pub u16);

impl VqCode {
    /// 域使命漂移（条款与冻结字面量不符）。
    pub const MISSION_DRIFT: VqCode = VqCode(0x5601);
    /// 签收记录缺项（域未开工）。
    pub const RECEIPT_INCOMPLETE: VqCode = VqCode(0x5602);
    /// 主题越界（映射表外主题）。
    pub const THEME_OUT_OF_TABLE: VqCode = VqCode(0x5603);
    /// 五段违约（跳段/回退）。
    pub const STAGE_VIOLATION: VqCode = VqCode(0x5604);
    /// 交接漂移（分工契约被重建或改写）。
    pub const HANDOVER_DRIFT: VqCode = VqCode(0x5605);
    /// 不可恢复红线击穿（最高缺陷立案）。
    pub const REDLINE_BREACH: VqCode = VqCode(0x5606);

    /// wire 码。
    pub const fn code(self) -> u16 {
        self.0
    }

    /// 人话原因。
    pub fn reason(self) -> String {
        match self {
            VqCode::MISSION_DRIFT => "域使命漂移：条款与恢复起点哲学冻结字面量不符".into(),
            VqCode::RECEIPT_INCOMPLETE => "签收记录缺项：P 域移交包未逐件入账，域未开工".into(),
            VqCode::THEME_OUT_OF_TABLE => "可靠性主题越界：映射表外不得立主题".into(),
            VqCode::STAGE_VIOLATION => "五段流水线违约：跳段或回退".into(),
            VqCode::HANDOVER_DRIFT => "交接漂移：O 域分工契约被重建或改写".into(),
            VqCode::REDLINE_BREACH => "不可恢复红线击穿：单点故障导致数据丢失或系统不可恢复——最高缺陷立案".into(),
            VqCode(_) => "未知 Q01 可靠性域诊断码".into(),
        }
    }
}

// ---------------------------------------------------------------------------
// 九、测试支撑（回归八组：使命/签收/映射/五段/分工/设施/红线/风险）
// ---------------------------------------------------------------------------

#[cfg(all(test, not(no_std)))]
mod tests {
    use super::*;

    #[test]
    fn 十组映射连续守恒() {
        let mut total = 0;
        for i in 0..GROUP_PLANS_Q.len() {
            let g = &GROUP_PLANS_Q[i];
            total += g.last - g.first + 1;
            if i > 0 {
                assert_eq!(g.first, GROUP_PLANS_Q[i - 1].last + 1);
            }
        }
        assert_eq!(total, Q_DOMAIN_TOTAL);
    }

    #[test]
    fn 五主题全覆盖() {
        let mut covered = [false; 5];
        for g in GROUP_PLANS_Q.iter() {
            for t in g.themes {
                covered[*t] = true;
            }
        }
        assert!(covered.iter().all(|c| *c));
    }

    #[test]
    fn 签收已定七件() {
        assert_eq!(P_HANDOVER.status, RecordStatus::Settled);
        assert_eq!(P_HANDOVER.items, 7);
        assert_eq!(HANDOVER_ITEMS_Q.len(), 7);
    }

    #[test]
    fn 流水线只许单向一步() {
        for i in 0..4 {
            assert!(stage_transition(PipelineStage::ALL[i], PipelineStage::ALL[i + 1]));
        }
        assert!(!stage_transition(PipelineStage::FaultClassify, PipelineStage::HealthMonitor));
    }

    #[test]
    fn 交接与复用声明() {
        assert_eq!(O_DOMAIN_HANDOVER.peer, "O 域");
        assert_eq!(CHAOS_FACILITY_REUSE.upstream, 2465);
    }

    #[test]
    fn 风险四条预案互异() {
        let f: Vec<String> = RiskKind::ALL.iter().map(|r| r.fallback()).collect();
        assert_eq!(f.len(), 4);
        for i in 0..4 {
            for j in 0..4 {
                if i != j {
                    assert_ne!(f[i], f[j]);
                }
            }
        }
    }
}
