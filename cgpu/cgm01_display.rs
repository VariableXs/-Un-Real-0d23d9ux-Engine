//! CGPU-F1921 · M 域开工与显示输出总架构（CGPU-M 域 · 显示输出 · 批次 M01 · 开山单）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F1921`
//!
//! 域使命：显示输出——渲染的"最后一厘米"：GPU 产出的一切都要正确、准时、
//! 好看地出现在物理屏上——**最后一厘米声明**。L 域移交包签收记录（F1917
//! 签收——签收兑现起点）；官方主题映射（枚举热插拔/模式时序/色彩 HDR/多显/
//! 安全功耗——十组映射表）；架构五段（枚举→模式→呈现→色彩→观测）；与
//! F1768 衔接（虚拟显示→物理显示贯通——贯通声明）；与 V 域（色彩管理跨卷
//! ——V 域协同声明）；不变量（呈现不破坏 80 帧合同——呈现段预算，合同终
//! 端声明）；风险四条（热插拔竞态/HDR 兼容/多屏同步/驱动差异——各配预案）。
//!
//! ## 要点一：最后一厘米是可机检的账
//!
//! "正确、准时、好看"三词不是口号而是三条款字面量冻结：正确=像素按
//! 确定性纪律到达（与 F0002 归并同源）、准时=呈现段预算受 80 帧合同
//! 约束（合同终端）、好看=色彩/HDR 档位显性登记。
//!
//! ## 要点二：签收是兑现起点，不是过场
//!
//! L 域移交包（F1917）签收为结构化记录（来源/内容项数/状态）——七件
//! 内容逐项可数；签收缺项即域未开工。
//!
//! ## 要点三：十组映射取官方批次标题逐字
//!
//! M 域十组（M01 显示输出总架构与枚举 … M10 收口）区间连续无缺口守恒
//! 160 项；官方五主题在映射表中全覆盖（判据侧重算）。
//!
//! ## 要点四：五段流水线单向
//!
//! 枚举→模式→呈现→色彩→观测；跳段（未枚举就选模式）与回退（观测逆向
//! 驱动色彩）均违约。
//!
//! ## 要点五：贯通不另起炉灶
//!
//! 虚拟显示（F1768）→物理显示（本域）是贯通而非重建：跨卷显示契约
//! 以 F1768 路径为上游——另起炉灶即分叉，分叉即漂移。
//!
//! ## 要点六：协同是显性声明
//!
//! 色彩管理跨卷对接 V 域（色彩管理域）：M 域呈现色彩档位引用 V 域声明，
//! 本域不私设色彩语义——私设即跨卷漂移。
//!
//! ## 要点七：不变量是合同终端
//!
//! 呈现段是 80 帧合同的最后一站：呈现预算登记受合同约束，呈现违约即
//! 合同终端违约——不允许"渲染准时、呈现掉链子"的豁免。
//!
//! ## 要点八：风险四条各配预案
//!
//! 热插拔竞态/HDR 兼容/多屏同步/驱动差异——预案互异且非空；无预案的
//! 风险等于黑屏邀请函。
//!
//! ## 要点九：诊断码独占 0x54xx 段
//!
//! 与 F1121（0x50xx）/cga02（0x51xx 实测）/F1441（0x52xx）/L01（0x53xx）/
//! F0001（0x39xx 细分）互不重叠。

// ---------------------------------------------------------------------------
// 导入（no_std 三件套）
// ---------------------------------------------------------------------------

use alloc::string::{String, ToString};

// ---------------------------------------------------------------------------
// 一、域使命与最后一厘米声明（字面量冻结）
// ---------------------------------------------------------------------------

/// M 域域使命三条款（字面量冻结——判据独立对拍）。
pub const MISSION_CLAUSES: [&str; 3] = [
    "GPU 产出的一切都要正确、准时、好看地出现在物理屏上",
    "渲染的最后一厘米——显示输出域使命声明",
    "呈现不破坏 80 帧合同——合同终端声明",
];

/// M 域起始任务号（本单）。
pub const M_DOMAIN_FIRST: u32 = 1921;
/// M 域终止任务号（含）。
pub const M_DOMAIN_LAST: u32 = 2080;
/// 域任务总数守恒（10 组 × 16 项）。
pub const M_DOMAIN_TOTAL: u32 = 160;

// ---------------------------------------------------------------------------
// 二、官方主题与十组映射
// ---------------------------------------------------------------------------

/// 官方主题五字面量（判据独立对拍）。
pub const OFFICIAL_THEMES: [&str; 5] = [
    "枚举热插拔",
    "模式时序",
    "色彩 HDR",
    "多显",
    "安全功耗",
];

/// 十组规划条目（官方批次标题逐字 + 主题覆盖标注）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GroupPlan {
    /// 组号（M01..M10）。
    pub group: u8,
    /// 官方组名（批次标题逐字）。
    pub name: &'static str,
    /// 本组覆盖的官方主题下标（流程/支撑组为空）。
    pub themes: &'static [usize],
    /// 起始任务号（含）。
    pub first: u32,
    /// 终止任务号（含）。
    pub last: u32,
}

/// 官方十组规划表（组名取施工书批次标题逐字；五主题全覆盖判据侧重算）。
pub const GROUP_PLANS: [GroupPlan; 10] = [
    GroupPlan { group: 1, name: "显示输出总架构与枚举组", themes: &[0], first: 1921, last: 1936 },
    GroupPlan { group: 2, name: "显示模式与时序组", themes: &[1], first: 1937, last: 1952 },
    GroupPlan { group: 3, name: "色彩与 HDR 输出组", themes: &[2], first: 1953, last: 1968 },
    GroupPlan { group: 4, name: "显示体验与无障碍组", themes: &[], first: 1969, last: 1984 },
    GroupPlan { group: 5, name: "多显示器与拼接组", themes: &[3], first: 1985, last: 2000 },
    GroupPlan { group: 6, name: "显示功耗与电源组", themes: &[4], first: 2001, last: 2016 },
    GroupPlan { group: 7, name: "显示遥测与调试组", themes: &[], first: 2017, last: 2032 },
    GroupPlan { group: 8, name: "显示生态与认证组", themes: &[], first: 2033, last: 2048 },
    GroupPlan { group: 9, name: "M 域预备与自查组", themes: &[], first: 2049, last: 2064 },
    GroupPlan { group: 10, name: "M 域收口组", themes: &[], first: 2065, last: 2080 },
];

// ---------------------------------------------------------------------------
// 三、L 域移交包签收（F1917 签收——签收兑现起点）
// ---------------------------------------------------------------------------

/// 交付记录状态（闭集）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordStatus {
    /// 已签收。
    Settled,
    /// 待兑现。
    Pending,
}

/// L 域移交包签收记录（F1917 签收——签收兑现起点）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HandoverReceipt {
    /// 来源任务号（L 域移交包）。
    pub source: u32,
    /// 移交包内容项数（七件）。
    pub items: u32,
    /// 状态。
    pub status: RecordStatus,
}

/// L 域移交包七件内容（逐字——判据独立写死）。
pub const HANDOVER_ITEMS: [&str; 7] = [
    "接口冻结清单（73 函数+四资产）",
    "契约清单（十域契约）",
    "资产清单（五账）",
    "基线快照（总册+损耗表+精度表）",
    "遗留移交清单（SR-IOV 迁移/机密计算/8K）",
    "显示输出衔接包（虚拟显示路径→M 域物理显示输出接口草案）",
    "经验教训十条",
];

/// L 域移交包签收（字面量钉死——判据对来源与项数独立写死）。
pub const L_HANDOVER: HandoverReceipt = HandoverReceipt {
    source: 1917,
    items: 7,
    status: RecordStatus::Settled,
};

// ---------------------------------------------------------------------------
// 四、架构五段（单向流水线）
// ---------------------------------------------------------------------------

/// 显示输出架构五段（官方闭集）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PipelineStage {
    /// 枚举（热插拔/设备发现）。
    Enumerate,
    /// 模式（分辨率/刷新率时序）。
    Mode,
    /// 呈现（交换链/上屏）。
    Present,
    /// 色彩（HDR/色域映射）。
    Color,
    /// 观测（遥测/调试）。
    Observe,
}

impl PipelineStage {
    /// 全部五段（单向序）。
    pub const ALL: [PipelineStage; 5] = [
        PipelineStage::Enumerate,
        PipelineStage::Mode,
        PipelineStage::Present,
        PipelineStage::Color,
        PipelineStage::Observe,
    ];

    /// 段下标。
    pub const fn index(self) -> usize {
        match self {
            PipelineStage::Enumerate => 0,
            PipelineStage::Mode => 1,
            PipelineStage::Present => 2,
            PipelineStage::Color => 3,
            PipelineStage::Observe => 4,
        }
    }

    /// 人话标签。
    pub fn label(self) -> String {
        match self {
            PipelineStage::Enumerate => "枚举",
            PipelineStage::Mode => "模式",
            PipelineStage::Present => "呈现",
            PipelineStage::Color => "色彩",
            PipelineStage::Observe => "观测",
        }
        .to_string()
    }
}

/// 相邻段迁移合法性（只许 i → i+1；跳段/回退均非法）。
pub fn stage_transition(from: PipelineStage, to: PipelineStage) -> bool {
    to.index() == from.index() + 1
}

// ---------------------------------------------------------------------------
// 五、贯通声明（F1768）与 V 域协同声明（跨卷显性化）
// ---------------------------------------------------------------------------

/// 跨卷贯通声明（上游来源+复用物+兑现任务）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BridgingDeclaration {
    /// 上游任务号。
    pub upstream: u32,
    /// 贯通内容。
    pub what: &'static str,
    /// 状态（衔接任务在本域后续批次兑现）。
    pub status: RecordStatus,
}

/// F1768 虚拟显示→物理显示贯通声明（字面量钉死——判据独立对拍）。
pub const VDISPLAY_BRIDGE: BridgingDeclaration = BridgingDeclaration {
    upstream: 1768,
    what: "虚拟显示→物理显示贯通（跨卷显示契约）",
    status: RecordStatus::Pending,
};

/// V 域协同声明（色彩管理跨卷——本域不私设色彩语义）。
pub const VDOMAIN_SYNERGY: BridgingDeclaration = BridgingDeclaration {
    upstream: 0,
    what: "色彩管理跨卷——M 域呈现色彩档位引用 V 域声明",
    status: RecordStatus::Pending,
};

// ---------------------------------------------------------------------------
// 六、不变量：合同终端（呈现段预算）
// ---------------------------------------------------------------------------

/// 呈现段预算登记（80 帧合同 12.5ms 帧内的呈现份额——合同终端声明）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PresentBudget {
    /// 呈现份额上限（微秒/帧）。
    pub budget_us: u32,
    /// 违约处置（显性登记，不静默吞帧）。
    pub on_breach: &'static str,
}

/// 呈现预算（字面量钉死——呈现份额 1.5ms/帧，占 12.5ms 的 12%）。
pub const PRESENT_BUDGET: PresentBudget = PresentBudget {
    budget_us: 1_500,
    on_breach: "呈现超预算即合同终端违约：显性入帧日志并联动降质链（F0008），不静默吞帧",
};

// ---------------------------------------------------------------------------
// 七、风险四条（各配预案，互异非空）
// ---------------------------------------------------------------------------

/// M 域风险四条（官方闭集）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RiskKind {
    /// 热插拔竞态。
    HotplugRace,
    /// HDR 兼容。
    HdrCompat,
    /// 多屏同步。
    MultiSync,
    /// 驱动差异。
    DriverVariance,
}

/// 风险总数。
pub const RISK_COUNT: usize = 4;

impl RiskKind {
    /// 全部风险（官方序）。
    pub const ALL: [RiskKind; RISK_COUNT] = [
        RiskKind::HotplugRace,
        RiskKind::HdrCompat,
        RiskKind::MultiSync,
        RiskKind::DriverVariance,
    ];

    /// 配置的预案（人话——判据断非空且逐条互异）。
    pub fn fallback(self) -> String {
        match self {
            RiskKind::HotplugRace => "预案：热插拔事件去抖+枚举快照原子替换，竞态窗口入遥测".to_string(),
            RiskKind::HdrCompat => "预案：HDR 能力协商表驱动，不支持档位显性回退 SDR 并登记".to_string(),
            RiskKind::MultiSync => "预案：主屏锚定相位对齐+从屏容差登记，失步显性立案".to_string(),
            RiskKind::DriverVariance => "预案：驱动差异表驱动，未测组合走保守时序并标注".to_string(),
        }
    }
}

// ---------------------------------------------------------------------------
// 八、错误契约（独占 0x54xx 段）
// ---------------------------------------------------------------------------

/// M01 诊断码。独占 `0x54xx` 段。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VmCode(pub u16);

impl VmCode {
    /// 主题越界（五主题表外）。
    pub const THEME_OUT_OF_TABLE: VmCode = VmCode(0x5401);
    /// 组越界（十组表外）。
    pub const GROUP_OUT_OF_TABLE: VmCode = VmCode(0x5402);
    /// 签收缺项（移交包未签收即域未开工）。
    pub const RECEIPT_INCOMPLETE: VmCode = VmCode(0x5403);
    /// 流水线违约（跳段/回退）。
    pub const STAGE_VIOLATION: VmCode = VmCode(0x5404);
    /// 贯通漂移（脱离 F1768 上游另起炉灶）。
    pub const BRIDGE_DRIFT: VmCode = VmCode(0x5405);
    /// 合同终端违约（呈现段破坏 80 帧合同）。
    pub const TERMINAL_VIOLATION: VmCode = VmCode(0x5406);
    /// 风险无预案。
    pub const RISK_NO_PLAN: VmCode = VmCode(0x5407);

    /// wire 码。
    pub const fn code(self) -> u16 {
        self.0
    }

    /// 人话原因。
    pub fn reason(self) -> String {
        match self {
            VmCode::THEME_OUT_OF_TABLE => "显示输出主题越界：五主题表外不得立题".into(),
            VmCode::GROUP_OUT_OF_TABLE => "组越界：M01..M10 表外不得立组".into(),
            VmCode::RECEIPT_INCOMPLETE => "移交包签收缺项：域未开工".into(),
            VmCode::STAGE_VIOLATION => "流水线违约：五段只许单向相邻迁移".into(),
            VmCode::BRIDGE_DRIFT => "贯通漂移：脱离 F1768 上游另起炉灶".into(),
            VmCode::TERMINAL_VIOLATION => "合同终端违约：呈现段破坏 80 帧合同".into(),
            VmCode::RISK_NO_PLAN => "风险无预案：无预案的风险等于黑屏邀请函".into(),
            VmCode(_) => "未知 M01 显示输出域诊断码".into(),
        }
    }
}

// ---------------------------------------------------------------------------
// 九、测试支撑（回归用例与断言）
// ---------------------------------------------------------------------------

#[cfg(test)]
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
        assert_eq!(total, M_DOMAIN_TOTAL);
    }

    #[test]
    fn 签收已定且七件齐() {
        assert_eq!(L_HANDOVER.status, RecordStatus::Settled);
        assert_eq!(L_HANDOVER.items, HANDOVER_ITEMS.len() as u32);
        assert_eq!(L_HANDOVER.source, 1917);
    }

    #[test]
    fn 流水线只许单向一步() {
        for i in 0..4 {
            assert!(stage_transition(PipelineStage::ALL[i], PipelineStage::ALL[i + 1]));
        }
        assert!(!stage_transition(PipelineStage::Mode, PipelineStage::Color));
    }

    #[test]
    fn 贯通上游是虚拟显示() {
        assert_eq!(VDISPLAY_BRIDGE.upstream, 1768);
        assert_eq!(VDISPLAY_BRIDGE.status, RecordStatus::Pending);
    }

    #[test]
    fn 呈现预算是合同终端() {
        assert_eq!(PRESENT_BUDGET.budget_us, 1_500);
        assert!(PRESENT_BUDGET.on_breach.contains("合同终端"));
    }
}
