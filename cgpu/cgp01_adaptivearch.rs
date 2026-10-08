//! CGPU-F2401 · P 域开工与自适应遥测总架构（CGPU-P 域 · 自适应遥测域 · P01 组 · 目标 340 行）。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F2401`
//!
//! **判据（锚点原文）**：自适应声明、签收、兑现起点、映射、五段、基座、
//! 红线、风险、八组、判据。
//!
//! **职责定位（锚点原文）**：P 域开工：域使命声明（自适应遥测——让遥测
//! 本身成为自适应系统：按需采集、按价值存储、按洞察呈现——遥测自适应
//! 声明；O 域移交包签收记录（F2397 签收）；F1453/F1553/F2216 遥测预留
//! 三处兑现起点声明（预留兑现起点）；官方主题映射（统一模型/采样策略/
//! 数据管道/分析洞察/看板——十组映射表）；架构五段（统一模型→自适应
//! 采集→管道→分析→消费）；与 J08/K07/O04 关系（J08=功耗遥测/K07=异构
//! 遥测/O04=降级遥测——三域遥测收编统一——统一收编声明：P 域=全遥测
//! 基座、各域=遥测生产者——基座声明）；隐私红线（本地默认/匿名/用户
//! 控制——红线复用家族——红线汇总；风险四条（数据量/口径漂移/隐私/
//! 性能——各配预案）；测试（使命/签收/兑现/映射/五段/收编/红线/风险
//! 八组）。
//!
//! ## 一、使命：让遥测本身成为自适应系统
//!
//! 传统遥测是死管道：全量采集→全量存储→人肉翻看。P 域使命（
//! [`MISSION_DOC`]）三条按价值重排：**按需采集**（价值低的信号降低
//! 采样率）、**按价值存储**（滚动聚合替逐帧全存）、**按洞察呈现**
//! （看板给结论不给原始流）——遥测本身要有预算意识（与帧预算同纪律：
//! 遥测开销有账面）。
//!
//! ## 二、签收与兑现起点：接过的单要认，许过的愿要认领
//!
//! O 域移交包（F2397）结构化签收（[`HandoverReceipt`]：来源/项数/
//! 状态）——签收是记录不是口号；F1453/F1553/F2216 三处他域遥测预留
//! 逐条认领（[`RESERVATIONS`]：兑现起点 Pending，兑现动作在 P 域内
//! 完成）——预留不认领就是悬空承诺。
//!
//! ## 三、基座声明：各域生产遥测，P 域统一收编
//!
//! J08（功耗遥测）/K07（异构遥测）/O04（降级遥测）各自产出遥测，
//! 统一进 P 域基座（[`BASE_DOC`]）——基座不生产业务数据、只定统一
//! 模型与管道；各域是生产者不是孤岛。隐私红线（[`PRIVACY_DOC`]）随
//! 基座走：本地默认/匿名化/用户控制三条是全遥测面的硬约束。
//!
//! ## 四、五段单向流水线：跳段与回退都拒绝
//!
//! 统一模型→自适应采集→管道→分析→消费（[`PipelineStage`]）单向
//! 流转：跳段（直接从模型跳到分析）与回退（分析倒回采集）都被
//! [`advance`] 显性拒绝——阶段越权在类型面拦截。
//!
//! **对接**：F0274（指标 schema 同源）；F0482/F0483（帧计时/账本——
//! D 域生产者）；J08/K07/O04（三域收编）。零 panic 面（下标走
//! `get`/`Option`）、零 IO、零墙钟、无全局可变状态、no_std 零 std
//! 依赖。

use alloc::format;
use alloc::string::String;

// ---------------------------------------------------------------------------
// 一、域使命与官方主题（判据一、判据四）
// ---------------------------------------------------------------------------

/// 本项版本。
pub const P_DOMAIN_VERSION: &str = "P01-adaptivearch-v1";

/// 域使命声明（判据一：自适应遥测三条按价值重排——字面量冻结）。
pub const MISSION_DOC: &str = "\
P 域使命（自适应遥测）：让遥测本身成为自适应系统——按需采集（价值\
低的信号降低采样率）、按价值存储（滚动聚合替逐帧全存）、按洞察呈现\
（看板给结论不给原始流）。遥测本身有预算意识：采集、存储、呈现三段\
各自有开销账面，遥测不得成为帧预算的隐形税。";

/// 官方五主题闭集（锚点原文：统一模型/采样策略/数据管道/分析洞察/看板）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PTheme {
    /// 统一模型。
    UnifiedModel,
    /// 采样策略。
    SamplingPolicy,
    /// 数据管道。
    DataPipeline,
    /// 分析洞察。
    AnalysisInsight,
    /// 看板。
    Dashboard,
}

impl PTheme {
    /// 人话标签。
    pub const fn label(self) -> &'static str {
        match self {
            PTheme::UnifiedModel => "统一模型",
            PTheme::SamplingPolicy => "采样策略",
            PTheme::DataPipeline => "数据管道",
            PTheme::AnalysisInsight => "分析洞察",
            PTheme::Dashboard => "看板",
        }
    }
}

/// 五主题齐备（判据四的结构断言源）。
pub const fn themes_present() -> [PTheme; 5] {
    [
        PTheme::UnifiedModel,
        PTheme::SamplingPolicy,
        PTheme::DataPipeline,
        PTheme::AnalysisInsight,
        PTheme::Dashboard,
    ]
}

/// 十组规划（判据四：组名+起止单号逐字取自总纲批次锚点；F2401-F2560）。
pub const P_GROUPS: [(&str, u32, u32); 10] = [
    ("域开工与自适应遥测总架构组", 2401, 2416),
    ("遥测采集与分级组", 2417, 2432),
    ("遥测分析与洞察组", 2433, 2448),
    ("遥测看板与消费组", 2449, 2464),
    ("遥测预测与智能组", 2465, 2480),
    ("遥测生态与开放组", 2481, 2496),
    ("遥测性能与预算组", 2497, 2512),
    ("遥测测试与资产组", 2513, 2528),
    ("P 域预备与自查组", 2529, 2544),
    ("P 域收口组", 2545, 2560),
];

// ---------------------------------------------------------------------------
// 二、签收与兑现起点（判据二、判据三）
// ---------------------------------------------------------------------------

/// 签收状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReceiptStatus {
    /// 已签收（项数逐项核对）。
    Settled,
}

/// O 域移交包签收记录（判据二：F2397 签收——结构化记录不是口号）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HandoverReceipt {
    /// 移交来源（O 域）。
    pub from_domain: &'static str,
    /// 移交单号。
    pub source_task: u32,
    /// 签收项数。
    pub items: u32,
    /// 状态。
    pub status: ReceiptStatus,
}

/// O 域移交签收（F2397：来源 O 域、移交包 4 项）。
pub const O_HANDOVER: HandoverReceipt = HandoverReceipt {
    from_domain: "O域",
    source_task: 2397,
    items: 4,
    status: ReceiptStatus::Settled,
};

/// 兑现状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClaimStatus {
    /// 兑现起点（本单认领，兑现动作在 P 域内完成）。
    Pending,
}

/// 一处预留兑现声明（判据三：兑现起点——预留不认领就是悬空承诺）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReservationClaim {
    /// 声明编号（P-R01/P-R02/P-R03）。
    pub claim_id: &'static str,
    /// 预留来源单号。
    pub source_task: u32,
    /// 预留主题。
    pub subject: &'static str,
    /// 状态。
    pub status: ClaimStatus,
}

/// 三处遥测预留兑现声明（F1453/F1553/F2216——锚点原文逐条认领）。
pub const RESERVATIONS: [ReservationClaim; 3] = [
    ReservationClaim {
        claim_id: "P-R01",
        source_task: 1453,
        subject: "功耗遥测与 P 域对接预留",
        status: ClaimStatus::Pending,
    },
    ReservationClaim {
        claim_id: "P-R02",
        source_task: 1553,
        subject: "J08 功耗遥测与报告组遥测总架构",
        status: ClaimStatus::Pending,
    },
    ReservationClaim {
        claim_id: "P-R03",
        source_task: 2216,
        subject: "性能与精度资产汇总",
        status: ClaimStatus::Pending,
    },
];

/// 按来源单号查兑现声明（O(1) 线性闭集查表；表外 None）。
pub fn reservation_by_task(task: u32) -> Option<&'static ReservationClaim> {
    let mut i = 0usize;
    while i < RESERVATIONS.len() {
        if let Some(r) = RESERVATIONS.get(i) {
            if r.source_task == task {
                return Some(r);
            }
        }
        i += 1;
    }
    None
}

// ---------------------------------------------------------------------------
// 三、基座声明与隐私红线（判据六、判据七）
// ---------------------------------------------------------------------------

/// 统一收编基座声明（判据六：P=全遥测基座、各域=遥测生产者）。
pub const BASE_DOC: &str = "\
统一收编声明（CGPU-F2401）：J08=功耗遥测、K07=异构遥测、O04=降级\
遥测——三域遥测收编统一。P 域=全遥测基座（统一模型+管道+分析+看板\
的承载面），各域=遥测生产者（只生产不建管道）。基座不生产业务数据、\
各域不私搭第二套管道——收编是单向的：生产者对接基座，基座不反向\
依赖任何生产者。";

/// 隐私红线（判据七：红线复用家族——本地默认/匿名/用户控制）。
pub const PRIVACY_DOC: &str = "\
隐私红线（全遥测面硬约束）：① 本地默认——遥测数据默认留在本地，\
不做未经确认的外发；② 匿名化——任何出口数据先匿名化，不携带用户\
可识别信息；③ 用户控制——用户可查可关可删，关闭后采集即停。红线\
复用家族口径，任何遥测生产者对接基座即受此约束。";

/// 三条红线逐条可 grep 的闭集（判据七结构面）。
pub const PRIVACY_LINES: [&str; 3] = [
    "本地默认——遥测数据默认留在本地",
    "匿名化——任何出口数据先匿名化",
    "用户控制——用户可查可关可删",
];

// ---------------------------------------------------------------------------
// 四、架构五段单向流水线（判据五）
// ---------------------------------------------------------------------------

/// 架构五段闭集（锚点原文：统一模型→自适应采集→管道→分析→消费）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PipelineStage {
    /// 统一模型。
    UnifiedModel,
    /// 自适应采集。
    AdaptiveCollect,
    /// 管道。
    Pipeline,
    /// 分析。
    Analysis,
    /// 消费。
    Consume,
}

impl PipelineStage {
    /// 序号（单向流转的次序基准）。
    pub const fn ordinal(self) -> usize {
        match self {
            PipelineStage::UnifiedModel => 0,
            PipelineStage::AdaptiveCollect => 1,
            PipelineStage::Pipeline => 2,
            PipelineStage::Analysis => 3,
            PipelineStage::Consume => 4,
        }
    }

    /// 人话标签。
    pub const fn label(self) -> &'static str {
        match self {
            PipelineStage::UnifiedModel => "统一模型",
            PipelineStage::AdaptiveCollect => "自适应采集",
            PipelineStage::Pipeline => "管道",
            PipelineStage::Analysis => "分析",
            PipelineStage::Consume => "消费",
        }
    }
}

/// 五段齐备（判据五的结构断言源）。
pub const PIPELINE_STAGES: [PipelineStage; 5] = [
    PipelineStage::UnifiedModel,
    PipelineStage::AdaptiveCollect,
    PipelineStage::Pipeline,
    PipelineStage::Analysis,
    PipelineStage::Consume,
];

/// 流转结论。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AdvanceOutcome {
    /// 前进到下一段。
    Advanced(PipelineStage),
    /// 拒绝：已在末段（消费之后无段）。
    AtEnd,
}

/// 五段单向流转（跳段与回退双向拒绝——阶段越权在类型面拦截）。
///
/// 合法流转：恰前进一段；`next == cur`（原地重复）拒绝；`next` 序号
/// 小于等于当前序号（回退/跳到非邻段）拒绝；跳段（next > cur+1）拒绝。
pub fn advance(cur: PipelineStage, next: PipelineStage) -> Option<AdvanceOutcome> {
    let co = cur.ordinal();
    let no = next.ordinal();
    if no == co.saturating_add(1) && no > co {
        Some(AdvanceOutcome::Advanced(next))
    } else if no >= PIPELINE_STAGES.len() {
        Some(AdvanceOutcome::AtEnd)
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// 五、风险预案（判据八：四条各配预案逐条互异）
// ---------------------------------------------------------------------------

/// 风险与预案。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RiskItem {
    /// 风险。
    pub what: &'static str,
    /// 预案（逐条互异——同一句预案糊四条视为没预案）。
    pub mitigation: &'static str,
}

/// 四条风险（锚点原文：数据量/口径漂移/隐私/性能——各配预案）。
pub const RISKS: [RiskItem; 4] = [
    RiskItem {
        what: "数据量",
        mitigation: "滚动聚合+分级存储，逐帧全存仅近期窗口",
    },
    RiskItem {
        what: "口径漂移",
        mitigation: "schema 版本化+F0274 五元组单源，指标命名三级锁死",
    },
    RiskItem {
        what: "隐私",
        mitigation: "三条红线硬约束（本地默认/匿名化/用户控制）随基座走",
    },
    RiskItem {
        what: "性能",
        mitigation: "遥测开销账面化，预算红线 0.1% 帧预算同 F0482 口径",
    },
];

/// 风险预案完整性（判据八：四条齐+预案两两互异）。
pub fn risks_complete() -> bool {
    if RISKS.len() != 4 {
        return false;
    }
    let mut i = 0usize;
    while i < RISKS.len() {
        let a = match RISKS.get(i) {
            Some(r) => r.mitigation,
            None => return false,
        };
        let mut j = i + 1;
        while j < RISKS.len() {
            let b = match RISKS.get(j) {
                Some(r) => r.mitigation,
                None => return false,
            };
            if a == b {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// 十组宣告单行人话（读屏可查——域开工宣告的播报行）。
pub fn ten_group_line() -> String {
    let mut s = String::from("P 域十组规划：");
    let mut i = 0usize;
    while i < P_GROUPS.len() {
        if let Some((name, from, to)) = P_GROUPS.get(i) {
            if i > 0 {
                s.push_str("；");
            }
            s.push_str(&format!("P{:02} {}(F{}-F{})", i + 1, name, from, to));
        }
        i += 1;
    }
    s
}
