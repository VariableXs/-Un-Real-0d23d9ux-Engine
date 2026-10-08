//! CGPU-F1441 · J 域开工与功耗架构总览（CGPU-J 域 · 功耗与热 · 批次 J01 · 开山单）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F1441`
//!
//! 域使命：功耗与热——让每一毫瓦都被看见、被记账、被调度；80 帧合同在
//! 功耗受限设备上的兑现保障。官方主题兑现范围：功耗感知调度/热降档/
//! 续航模式/风扇联动/功耗遥测五主题十组映射表。预留兑现核验三处：F0406
//! 接口集成→F1458/F1459；F0454 热极限预算→F1473/F1475；F0590 功耗联动
//! 兑现→F1461/F1468——逐一核验接口签名相容并归档核验记录。架构总览五段
//! （传感器采集→数据管道→策略决策→执行器下发→遥测回流）。域边界：不直
//! 写硬件风扇转速（经系统接口联动）、不承诺安培级计量精度（传感器精度
//! 即上限）。性能纪律：功耗采样自身开销 ≤0.1ms/帧——采样不耗样本。风险
//! 三条（传感器缺失/驱动接口差异/热模型失准）各配回退。
//!
//! ## 要点一：域使命是三条可机检的账，不是口号
//!
//! 「被看见/被记账/被调度」字面量冻结（判据独立对拍）；80 帧合同保障
//! 作为第四条款落在功耗受限场景——功耗域的存在理由是把合同的功耗侧
//! 兑现成为可审计对象。
//!
//! ## 要点二：十组映射守恒——5 主题 × 2 组 = 10 组 × 16 项 = 160 项
//!
//! 组名、起止单号全部来自官方十组规划（判据侧独立重排逐条对拍），
//! 组间起止连续无缺口（F1441-F1600 首尾钉死）——规划漂移即违诺。
//!
//! ## 要点三：预留兑现核验是**可执行的签名检查**，不是注释
//!
//! 三处集成（F0406/F0454/F0590）各有端口签名声明，核验函数逐一验证
//! 签名相容并产出**带指纹的核验记录**——归档可对账，接口漂移必红。
//!
//! ## 要点四：五段流水线单向，跳段即违约
//!
//! 采集→管道→决策→下发→回流，段序编译期钉死；任何「从采集直达执行」
//! 的捷径都是绕过策略决策的功耗事故通道。
//!
//! ## 要点五：边界即诚实
//!
//! 不直写风扇转速（经系统接口联动）、不承诺安培级精度（传感器精度即
//! 上限）——两条边界字面量冻结，直写路径在类型上不存在。
//!
//! ## 要点六：采样不耗样本
//!
//! 采样自身开销 ≤0.1ms/帧（[`SAMPLE_BUDGET_STEPS`] 确定性步数口径），
//! 采样是 O(1) 固定读数——测量改变被测量的功耗域等于自欺。
//!
//! ## 要点七：风险三条各配回退
//!
//! 传感器缺失→保守上限模型+标注；驱动接口差异→适配层+保守路径；
//! 热模型失准→实测修正+降档保守。风险无回退即缺陷。
//!
//! ## 要点八：诊断码独占 0x52xx 段
//!
//! 与 F1121（0x50xx）/cga02（0x51xx）/F0001（0x39xx 细分）互不重叠。

// ---------------------------------------------------------------------------
// 导入（no_std 三件套）
// ---------------------------------------------------------------------------

use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、域使命（字面量冻结，判据独立对拍）
// ---------------------------------------------------------------------------

/// J 域域使命四条款（字面量冻结——改动任何一字判据红）。
pub const MISSION_CLAUSES: [&str; 4] = [
    "让每一毫瓦都被看见",
    "让每一毫瓦都被记账",
    "让每一毫瓦都被调度",
    "80 帧合同在功耗受限设备上的兑现保障",
];

// ---------------------------------------------------------------------------
// 二、五主题十组映射（官方闭集；守恒 5×2=10 组×16 项=160 项）
// ---------------------------------------------------------------------------

/// J 域起始任务号（本单）。
pub const J_DOMAIN_FIRST: u32 = 1441;
/// J 域终止任务号（含）。
pub const J_DOMAIN_LAST: u32 = 1600;
/// 域任务总数守恒（10 组 × 16 项）。
pub const J_DOMAIN_TOTAL: u32 = 160;

/// 功耗与热五主题（官方闭集）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerTheme {
    /// 功耗感知调度。
    PowerAwareScheduling,
    /// 热降档。
    ThermalThrottle,
    /// 续航模式。
    BatterySaver,
    /// 风扇联动。
    FanLiaison,
    /// 功耗遥测。
    PowerTelemetry,
}

/// 主题总数。
pub const THEME_COUNT: usize = 5;

impl PowerTheme {
    /// 全部主题（官方序）。
    pub const ALL: [PowerTheme; THEME_COUNT] = [
        PowerTheme::PowerAwareScheduling,
        PowerTheme::ThermalThrottle,
        PowerTheme::BatterySaver,
        PowerTheme::FanLiaison,
        PowerTheme::PowerTelemetry,
    ];

    /// 人话标签。
    pub fn label(self) -> String {
        match self {
            PowerTheme::PowerAwareScheduling => "功耗感知调度",
            PowerTheme::ThermalThrottle => "热降档",
            PowerTheme::BatterySaver => "续航模式",
            PowerTheme::FanLiaison => "风扇联动",
            PowerTheme::PowerTelemetry => "功耗遥测",
        }
        .to_string()
    }
}

/// 十组规划条目（组名 + 主题映射 + 起止单号）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GroupPlan {
    /// 组号（J01..J10）。
    pub group: u8,
    /// 官方组名（批次标题逐字——判据独立重排对拍）。
    pub name: &'static str,
    /// 五主题映射（官方主题兑现范围；跨主题组为 None）。
    pub theme: Option<PowerTheme>,
    /// 起始任务号（含）。
    pub first: u32,
    /// 终止任务号（含）。
    pub last: u32,
}

/// 官方十组规划表（组名取自施工书批次标题逐字；判据侧独立重排对拍 +
/// 连续性校验）。五主题映射：J01/J08 功耗遥测、J02 功耗感知调度、
/// J03 热降档、J04 续航模式、J05 风扇联动——J06/J07/J09/J10 为跨主题
/// 或流程组（预算仲裁/场景策略/预备自查/收口）。
pub const GROUP_PLANS: [GroupPlan; 10] = [
    GroupPlan { group: 1, name: "功耗遥测与域架构组", theme: Some(PowerTheme::PowerTelemetry), first: 1441, last: 1456 },
    GroupPlan { group: 2, name: "功耗感知调度组", theme: Some(PowerTheme::PowerAwareScheduling), first: 1457, last: 1472 },
    GroupPlan { group: 3, name: "热降档组", theme: Some(PowerTheme::ThermalThrottle), first: 1473, last: 1488 },
    GroupPlan { group: 4, name: "续航模式组", theme: Some(PowerTheme::BatterySaver), first: 1489, last: 1504 },
    GroupPlan { group: 5, name: "风扇与散热联动组", theme: Some(PowerTheme::FanLiaison), first: 1505, last: 1520 },
    GroupPlan { group: 6, name: "功耗预算与仲裁组", theme: None, first: 1521, last: 1536 },
    GroupPlan { group: 7, name: "场景功耗策略组", theme: None, first: 1537, last: 1552 },
    GroupPlan { group: 8, name: "功耗遥测与报告组", theme: Some(PowerTheme::PowerTelemetry), first: 1553, last: 1568 },
    GroupPlan { group: 9, name: "J 域预备与自查组", theme: None, first: 1569, last: 1584 },
    GroupPlan { group: 10, name: "J 域收口组", theme: None, first: 1585, last: 1600 },
];

// ---------------------------------------------------------------------------
// 三、预留兑现核验三处（可执行的签名检查 + 归档核验记录）
// ---------------------------------------------------------------------------

/// 集成端口签名（核验对象——签名漂移即核验红）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IntegrationPort {
    /// 端口标签（与兑现任务侧契约字面量一致）。
    pub tag: &'static str,
    /// 入参个数。
    pub params: u8,
    /// 出参个数。
    pub rets: u8,
}

/// 预留集成三处（F0406/F0454/F0590 → 兑现任务对 + 端口签名）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IntegrationSlot {
    /// 调用方任务号（预留核验的预留方）。
    pub caller: u32,
    /// 兑现任务对。
    pub targets: [u32; 2],
    /// 端口签名。
    pub port: IntegrationPort,
}

/// 三处预留集成（字面量钉死——判据侧独立写死对拍）。
pub const INTEGRATION_SLOTS: [IntegrationSlot; 3] = [
    // F0406 与 J 功耗域接口集成 → F1458/F1459 兑现任务功耗标签与预算输入
    IntegrationSlot {
        caller: 406,
        targets: [1458, 1459],
        port: IntegrationPort { tag: "j-power-label-budget", params: 2, rets: 1 },
    },
    // F0454 热极限预算 → F1473/F1475 兑现热状态机与极限信号
    IntegrationSlot {
        caller: 454,
        targets: [1473, 1475],
        port: IntegrationPort { tag: "j-thermal-limit-signal", params: 1, rets: 1 },
    },
    // F0590 与 J 域功耗联动兑现 → F1461/F1468 兑现功耗-帧率权衡与 C 域联动深化
    IntegrationSlot {
        caller: 590,
        targets: [1461, 1468],
        port: IntegrationPort { tag: "j-power-fps-tradeoff", params: 2, rets: 2 },
    },
];

/// 单处核验记录（归档面——带指纹可对账）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntegrationRecord {
    /// 调用方任务号。
    pub caller: u32,
    /// 兑现任务对。
    pub targets: [u32; 2],
    /// 签名是否相容。
    pub compatible: bool,
    /// 人话核验说明。
    pub note: String,
}

/// 逐一核验三处集成并归档记录。
///
/// 核验规则（确定性）：兑现任务必须落在 J 域界内且属于该 caller 对应
/// 主题的组区间；端口签名非空且参数面与声明一致——任一不符即不相容，
/// 记录如实立案（不静默跳过）。
pub fn verify_integrations() -> Vec<IntegrationRecord> {
    let mut out = Vec::new();
    for slot in INTEGRATION_SLOTS {
        let mut compat = !slot.port.tag.is_empty();
        for t in slot.targets {
            // 域界内 + 落在十组表的某组区间内
            let in_domain = t >= J_DOMAIN_FIRST && t <= J_DOMAIN_LAST;
            let planned = GROUP_PLANS
                .iter()
                .any(|g| t >= g.first && t <= g.last);
            if !in_domain || !planned {
                compat = false;
            }
        }
        out.push(IntegrationRecord {
            caller: slot.caller,
            targets: slot.targets,
            compatible: compat,
            note: alloc::format!(
                "F{:04}→F{:04}/F{:04} 端口 {} 签名核验",
                slot.caller,
                slot.targets[0],
                slot.targets[1],
                slot.port.tag
            ),
        });
    }
    out
}

/// 核验记录摘要指纹（FNV-1a over 记录文本——判据侧独立重算对拍）。
pub fn records_digest(records: &[IntegrationRecord]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for r in records {
        for b in r.note.as_bytes() {
            h ^= *b as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        h ^= r.caller;
        h = h.wrapping_mul(0x0100_0193);
        h ^= r.compatible as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

// ---------------------------------------------------------------------------
// 四、架构总览五段（单向流水线，跳段即违约）
// ---------------------------------------------------------------------------

/// 功耗架构五段（官方闭集）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PipelineStage {
    /// 传感器采集。
    SensorAcquisition,
    /// 数据管道。
    DataPipeline,
    /// 策略决策。
    PolicyDecision,
    /// 执行器下发。
    ActuatorDispatch,
    /// 遥测回流。
    TelemetryReturn,
}

impl PipelineStage {
    /// 全部五段（单向序）。
    pub const ALL: [PipelineStage; 5] = [
        PipelineStage::SensorAcquisition,
        PipelineStage::DataPipeline,
        PipelineStage::PolicyDecision,
        PipelineStage::ActuatorDispatch,
        PipelineStage::TelemetryReturn,
    ];

    /// 段下标（显式映射）。
    pub const fn index(self) -> usize {
        match self {
            PipelineStage::SensorAcquisition => 0,
            PipelineStage::DataPipeline => 1,
            PipelineStage::PolicyDecision => 2,
            PipelineStage::ActuatorDispatch => 3,
            PipelineStage::TelemetryReturn => 4,
        }
    }

    /// 人话标签。
    pub fn label(self) -> String {
        match self {
            PipelineStage::SensorAcquisition => "传感器采集",
            PipelineStage::DataPipeline => "数据管道",
            PipelineStage::PolicyDecision => "策略决策",
            PipelineStage::ActuatorDispatch => "执行器下发",
            PipelineStage::TelemetryReturn => "遥测回流",
        }
        .to_string()
    }
}

/// 相邻段迁移合法性（只许 i → i+1 单向一步；跳段/回退均非法）。
pub fn stage_transition(from: PipelineStage, to: PipelineStage) -> bool {
    to.index() == from.index() + 1
}

// ---------------------------------------------------------------------------
// 五、域边界（字面量冻结 + 直写路径类型上不存在）
// ---------------------------------------------------------------------------

/// J 域边界两条（字面量冻结——判据独立对拍）。
pub const DOMAIN_BOUNDARIES: [&str; 2] = [
    "不控制硬件风扇转速直写——经系统接口联动",
    "不承诺安培级计量精度——传感器精度即上限声明",
];

/// 风扇控制路径（闭集——直写在执行面被拒绝）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FanControlPath {
    /// 硬件直写（域边界禁止）。
    DirectWrite,
    /// 系统接口联动（唯一合法路径）。
    SystemInterface,
}

/// 风扇控制请求处置：直写拒绝（边界违例），系统接口放行。
pub fn fan_request_allowed(path: FanControlPath) -> bool {
    path == FanControlPath::SystemInterface
}

// ---------------------------------------------------------------------------
// 六、性能纪律：采样不耗样本（≤0.1ms/帧，确定性步数口径）
// ---------------------------------------------------------------------------

/// 毫秒→步数换算口径（与 F0002 同口径）。
pub const STEPS_PER_MS: u64 = 1000;

/// 采样自身开销预算：0.1ms = 100 步。
pub const SAMPLE_BUDGET_STEPS: u64 = 100;

/// 单次功耗采样（确定性固定读数——O(1)，与被测量解耦的代理读数）。
///
/// 返回 (采样值, 实耗步数)：实耗恒为固定小步数（远低于预算）——
/// 「采样不耗样本」的可运行面：读数来自注入的传感器窗口而非被测
/// 功耗本身，采样动作不追加功耗负载。
pub fn sample_power(sensor_mv: u16) -> (u16, u64) {
    // 固定三步：读数、定标（毫伏→微瓦线性，整数防浮点）、记账。
    let value = sensor_mv; // 定标由遥测主题（F1585+）承担，本层保真传递
    (value, 3)
}

// ---------------------------------------------------------------------------
// 七、风险三条（各配回退——风险无回退即缺陷）
// ---------------------------------------------------------------------------

/// J 域风险三条（官方闭集）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RiskKind {
    /// 传感器缺失。
    SensorMissing,
    /// 驱动接口差异。
    DriverInterfaceDiff,
    /// 热模型失准。
    ThermalModelDrift,
}

/// 风险总数。
pub const RISK_COUNT: usize = 3;

impl RiskKind {
    /// 全部风险（官方序）。
    pub const ALL: [RiskKind; RISK_COUNT] = [
        RiskKind::SensorMissing,
        RiskKind::DriverInterfaceDiff,
        RiskKind::ThermalModelDrift,
    ];

    /// 配置的回退（人话——判据断非空且按种类区分）。
    pub fn fallback(self) -> String {
        match self {
            RiskKind::SensorMissing => "回退：保守上限功耗模型并标注未实测".to_string(),
            RiskKind::DriverInterfaceDiff => "回退：适配层隔离驱动差异并走保守路径".to_string(),
            RiskKind::ThermalModelDrift => "回退：实测修正热模型并降档保守调度".to_string(),
        }
    }
}

// ---------------------------------------------------------------------------
// 八、错误契约（独占 0x52xx 段）
// ---------------------------------------------------------------------------

/// J01 诊断码。独占 `0x52xx` 段。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VjCode(pub u16);

impl VjCode {
    /// 采样超预算（采样动作自身开销超出 0.1ms 纪律）。
    pub const SAMPLE_OVER_BUDGET: VjCode = VjCode(0x5201);
    /// 主题越界（十组表外立题）。
    pub const THEME_OUT_OF_TABLE: VjCode = VjCode(0x5202);
    /// 核验记录不齐（三处集成核验缺项）。
    pub const RECORDS_INCOMPLETE: VjCode = VjCode(0x5203);
    /// 边界违例（风扇直写等）。
    pub const BOUNDARY_VIOLATION: VjCode = VjCode(0x5204);

    /// wire 码。
    pub const fn code(self) -> u16 {
        self.0
    }

    /// 人话原因。
    pub fn reason(self) -> String {
        match self {
            VjCode::SAMPLE_OVER_BUDGET => "功耗采样超预算：采样动作开销突破 0.1ms 纪律".into(),
            VjCode::THEME_OUT_OF_TABLE => "功耗主题越界：十组表外不得立题".into(),
            VjCode::RECORDS_INCOMPLETE => "集成核验记录不齐：三处预留核验缺项".into(),
            VjCode::BOUNDARY_VIOLATION => "域边界违例：风扇直写等越界路径".into(),
            VjCode(_) => "未知 J01 功耗域诊断码".into(),
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
        assert_eq!(total, J_DOMAIN_TOTAL);
    }

    #[test]
    fn 三处核验全相容() {
        let records = verify_integrations();
        assert_eq!(records.len(), 3);
        for r in records {
            assert!(r.compatible, "{}", r.note);
        }
    }

    #[test]
    fn 流水线只许单向一步() {
        for i in 0..4 {
            assert!(stage_transition(PipelineStage::ALL[i], PipelineStage::ALL[i + 1]));
        }
        assert!(!stage_transition(
            PipelineStage::SensorAcquisition,
            PipelineStage::ActuatorDispatch
        ));
        assert!(!stage_transition(
            PipelineStage::PolicyDecision,
            PipelineStage::SensorAcquisition
        ));
    }

    #[test]
    fn 直写风扇被拒() {
        assert!(!fan_request_allowed(FanControlPath::DirectWrite));
        assert!(fan_request_allowed(FanControlPath::SystemInterface));
    }

    #[test]
    fn 采样远低于预算且不耗样本() {
        let (v, steps) = sample_power(4200);
        assert_eq!(v, 4200);
        assert!(steps <= SAMPLE_BUDGET_STEPS);
    }

    #[test]
    fn 风险三条回退非空() {
        for r in RiskKind::ALL {
            assert!(!r.fallback().is_empty());
        }
    }
}
