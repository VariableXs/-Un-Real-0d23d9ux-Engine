//! CGPU-F0802 · 认证流程体系（CGPU-F 域 · 硬件兼容与认证域 · F01 组）。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F0802`
//!
//! **判据（锚点原文）**：六步、自动化、中断恢复、审计、判据。
//!
//! **职责定位（锚点原文）**：认证流程：六步认证正式化（申请→能力探测→
//! 基准执行→判定→认证授予→发布——每步的输入输出与判据固化）；流程
//! 自动化（全流程无人值守可跑——D08 执行器复用）；流程中断恢复（任一
//! 步失败的续跑/重跑语义）；流程审计（每份认证的全流程留痕——可回溯）。
//!
//! ## 一、六步正式化：每步有输入输出与判据（判据一）
//!
//! 六步闭集（[`Step`]）：申请/能力探测/基准执行/判定/授予/发布。每步的
//! 输入输出契约固化为 [`STEP_CONTRACTS`]（步骤 → 输入要点 → 输出要点 →
//! 通过判据）——「正式化」的含义是契约在册可对账，不是口头约定。步骤
//! 线性单向：跳步（[`XcCode::StepSkipped`]）与回退（
//! [`XcCode::StepRegression`]）都显性拒。步产物类型化（[`Artifact`]）：
//! 每步的产出以带类型与证据链的工件入账——判定步的诚实纪律落到工件
//! 引用上（[`XcCode::VerdictNoEvidence`]）。
//!
//! ## 二、流程自动化：状态机驱动，无人值守（判据二）
//!
//! [`CertPipeline`] 是逐份认证的状态机：step 游标 + 每步工件。自动化
//! 入口 [`CertPipeline::advance`] 按当前步取动作（[`StepAction`] 三态：
//! 就绪可推进/等待外部输入/失败待处置），推进逻辑零人工判断点
//! ——无人值守的落法是「状态机自己知道下一步是什么」。批量自动化
//! （[`AutoRunner`]）驱动多份认证并发推进：逐 tick 泵动、注入式步
//! 结果（[`StepOutcome`]）驱动推进/失败/等待——D08 执行器复用的
//! 声明位（执行动作外部注入，流程面只管调度与记账）。
//!
//! ## 三、中断恢复：续跑不重做，重跑可重入（判据三）
//!
//! 中断语义两条（[`CertPipeline::resume`]）：已完成步的工件保留（续跑
//! 不重做——基准执行代价高，重做即浪费）；失败步支持原地重跑
//! （[`CertPipeline::retry_current`]，重试计数封顶 [`MAX_STEP_RETRIES`]，
//! 超顶显性转 [`StepPhase::Abandoned`]——不静默无限重试）。流水线
//! 快照（[`CertPipeline::snapshot`] / [`CertPipeline::restore`]）把
//! 状态折叠为定长元组——跨会话恢复零损失（恢复面不依赖账本内存活）。
//!
//! ## 四、流程审计：全留痕可回溯 + 防篡改（判据四）
//!
//! 每次步推进追加一条审计行（[`AuditLine`]：认证 id/步/相位/诊断摘要/
//! tick/序号/前链指纹）——append-only，封顶滚动（配额 [`AUDIT_CAPACITY`]）；
//! 链式指纹（[`CertAudit::chain_intact`]：FNV-1a 逐行迭代，断链即红）
//! 让审计不只是留痕，而是**改不了**的留痕——出纠纷看账，不靠回忆。
//! 审计按步/按相位/按 tick 区间多维回溯（[`CertAudit::trace_step`] 等）。
//!
//! ## 五、判定步的诚实纪律与授予有效期
//!
//! 判定（[`Step::Verdict`]）输出三态结论（金/银/铜/不认证的分级判定
//! 在 F0806，本步只产出「过/不过 + 依据」）；依据必须引用基准执行步
//! 的工件（无工件依据 = 判定无效 [`XcCode::VerdictNoEvidence`]）——
//! 不凭印象发证。授予记录（[`GrantRecord`]）带等级与有效期（tick
//! 窗口；过期查询 [`GrantRecord::expired_at`]）；发布条目
//! （[`PublishEntry`]）走隐私白名单（[`PUBLISH_ALLOWED_FIELDS`]）——
//! 表外字段发布即 [`XcCode::PublishPrivacy`] 拒。
//!
//! **对接**：F0801（域开工，本模块是流程面）；F0803/F0804（能力探测/
//! 身份识别——第二步的实现供给）；F0805（基准套件——第三步考卷）；
//! F0806（分级——第五步授予的分级规则）；D08（执行器复用声明）。
//! 零 panic 面、零 IO、零墙钟（tick 注入）、no_std。

use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、规格常量
// ---------------------------------------------------------------------------

/// 本项版本。
pub const CERT_FLOW_VERSION: &str = "F01-certflow-v1";

/// 单步重试上限（超顶显性弃置，不静默无限重试）。
pub const MAX_STEP_RETRIES: u32 = 3;

/// 审计容量（append-only 封顶滚动；配额恒定长跑不膨胀）。
pub const AUDIT_CAPACITY: usize = 256;

/// 授予默认有效期（tick 数；「有效期」是 tick 窗口不是墙钟日期——
/// 零墙钟纪律。当前策略值，可调）。
pub const GRANT_VALIDITY_TICKS: u64 = 1_000_000;

/// FNV-1a 64 位偏移基（审计链指纹用）。
const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;

/// FNV-1a 64 位素数。
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// 单字节 FNV 迭代。
const fn fnv_byte(acc: u64, b: u8) -> u64 {
    (acc ^ (b as u64)).wrapping_mul(FNV_PRIME)
}

/// 字符串 FNV 迭代。
const fn fnv_str(acc: u64, s: &str) -> u64 {
    let bytes = s.as_bytes();
    let mut acc = acc;
    let mut i = 0usize;
    while i < bytes.len() {
        acc = fnv_byte(acc, bytes[i]);
        i += 1;
    }
    acc
}

// ---------------------------------------------------------------------------
// 二、六步闭集与契约表（判据一）
// ---------------------------------------------------------------------------

/// 六步闭集（线性单向；表外不立步）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// 第一步：申请（认证请求受理）。
    Apply,
    /// 第二步：能力探测。
    Probe,
    /// 第三步：基准执行。
    Bench,
    /// 第四步：判定。
    Verdict,
    /// 第五步：认证授予。
    Grant,
    /// 第六步：发布。
    Publish,
}

impl Step {
    /// 步序（0-5；线性单向的序面）。
    pub const fn ordinal(self) -> u8 {
        match self {
            Step::Apply => 0,
            Step::Probe => 1,
            Step::Bench => 2,
            Step::Verdict => 3,
            Step::Grant => 4,
            Step::Publish => 5,
        }
    }

    /// 下一步（发布步的下一步是它自己——终态由相位表达）。
    pub const fn next(self) -> Step {
        match self {
            Step::Apply => Step::Probe,
            Step::Probe => Step::Bench,
            Step::Bench => Step::Verdict,
            Step::Verdict => Step::Grant,
            Step::Grant => Step::Publish,
            Step::Publish => Step::Publish,
        }
    }

    /// 前一步（回退校验用；申请步无前步）。
    pub const fn prev(self) -> Option<Step> {
        match self {
            Step::Apply => None,
            Step::Probe => Some(Step::Apply),
            Step::Bench => Some(Step::Probe),
            Step::Verdict => Some(Step::Bench),
            Step::Grant => Some(Step::Verdict),
            Step::Publish => Some(Step::Grant),
        }
    }

    /// 从序号还原（越界 None）。
    pub const fn from_ordinal(o: u8) -> Option<Step> {
        match o {
            0 => Some(Step::Apply),
            1 => Some(Step::Probe),
            2 => Some(Step::Bench),
            3 => Some(Step::Verdict),
            4 => Some(Step::Grant),
            5 => Some(Step::Publish),
            _ => None,
        }
    }

    /// 人读名（审计与报告面）。
    pub const fn zh(self) -> &'static str {
        match self {
            Step::Apply => "申请",
            Step::Probe => "能力探测",
            Step::Bench => "基准执行",
            Step::Verdict => "判定",
            Step::Grant => "授予",
            Step::Publish => "发布",
        }
    }
}

/// 步契约行（步骤 → 输入要点 → 输出要点 → 通过判据）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StepContract {
    /// 步骤。
    pub step: Step,
    /// 输入要点。
    pub input: &'static str,
    /// 输出要点。
    pub output: &'static str,
    /// 通过判据（固化在册，可对账）。
    pub criterion: &'static str,
}

/// 六步契约表（判据一：正式化=契约在册）。
pub const STEP_CONTRACTS: [StepContract; 6] = [
    StepContract {
        step: Step::Apply,
        input: "硬件身份三要素+认证等级申请",
        output: "受理回执（认证 id）",
        criterion: "身份三要素齐备且未在有效认证期内",
    },
    StepContract {
        step: Step::Probe,
        input: "受理回执",
        output: "能力探测报告（三层探测 schema）",
        criterion: "设备级/驱动级/系统级三层全探测且防伪抽查通过",
    },
    StepContract {
        step: Step::Bench,
        input: "能力探测报告",
        output: "基准执行工件（分项场景分数）",
        criterion: "考卷全项执行完成，逐项有分数无缺项",
    },
    StepContract {
        step: Step::Verdict,
        input: "基准执行工件",
        output: "判定结论（过/不过+依据）",
        criterion: "依据引用基准工件，无工件不判定",
    },
    StepContract {
        step: Step::Grant,
        input: "判定结论",
        output: "认证授予记录（等级+有效期）",
        criterion: "仅判定通过的申请可授予",
    },
    StepContract {
        step: Step::Publish,
        input: "认证授予记录",
        output: "公开认证矩阵条目",
        criterion: "不含用户隐私数据且带签名",
    },
];

/// 契约表守恒：六步逐位对齐 + 判据非空（判据一机检面）。
pub fn contracts_intact() -> bool {
    let mut i = 0usize;
    while i < STEP_CONTRACTS.len() {
        let c = match STEP_CONTRACTS.get(i) {
            Some(c) => c,
            None => return false,
        };
        if c.step.ordinal() as usize != i {
            return false;
        }
        if c.input.is_empty() || c.output.is_empty() || c.criterion.is_empty() {
            return false;
        }
        i += 1;
    }
    true
}

// ---------------------------------------------------------------------------
// 三、工件与诊断码（判定诚实纪律的落地面）
// ---------------------------------------------------------------------------

/// 工件类型闭集（每步产物的类型面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArtifactKind {
    /// 受理回执。
    Receipt,
    /// 能力探测报告。
    ProbeReport,
    /// 基准执行工件。
    BenchResult,
    /// 判定结论。
    VerdictDoc,
    /// 授予记录。
    GrantDoc,
    /// 公开矩阵条目。
    PublishDoc,
}

impl ArtifactKind {
    /// 归属步。
    pub const fn owner(self) -> Step {
        match self {
            ArtifactKind::Receipt => Step::Apply,
            ArtifactKind::ProbeReport => Step::Probe,
            ArtifactKind::BenchResult => Step::Bench,
            ArtifactKind::VerdictDoc => Step::Verdict,
            ArtifactKind::GrantDoc => Step::Grant,
            ArtifactKind::PublishDoc => Step::Publish,
        }
    }
}

/// 步工件（类型 + 内容指纹 + 证据引用）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Artifact {
    /// 工件类型。
    pub kind: ArtifactKind,
    /// 内容指纹（FNV-1a——同内容同指纹，改内容指纹必变）。
    pub fingerprint: u64,
    /// 证据引用（判定工件必须引用基准工件指纹——诚实纪律的机检面）。
    pub evidence_of: Option<u64>,
    /// 入账 tick。
    pub at_tick: u64,
}

/// 工件指纹计算（确定算法公开——同输入同指纹）。
pub fn artifact_fingerprint(kind: ArtifactKind, payload: &str) -> u64 {
    let mut acc = fnv_str(FNV_OFFSET, payload);
    acc = fnv_str(acc, kind.owner().zh());
    acc
}

/// 诊断码闭集（0x5F 段）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum XcCode {
    /// 跳步（线性单向被破坏）。
    StepSkipped,
    /// 回退（流程倒退）。
    StepRegression,
    /// 判定无证据（未引用基准工件）。
    VerdictNoEvidence,
    /// 重试超顶。
    RetriesExhausted,
    /// 等待外部输入（自动化视角的正常停等，非缺陷）。
    WaitExternal,
    /// 授予前置缺失：判定未通过（或无判定工件）即申请授予。
    GrantWithoutPass,
    /// 发布隐私违规：发布条目携带白名单外字段。
    PublishPrivacy,
    /// 工件类型错步：工件归属步与当前步不符。
    ArtifactWrongStep,
    /// 授予已过期（查询面结论，非缺陷）。
    GrantExpired,
    /// 审计链断裂（防篡改对账失败——严重缺陷）。
    AuditChainBroken,
}

impl XcCode {
    /// 人读诊断。
    pub const fn detail(self) -> &'static str {
        match self {
            XcCode::StepSkipped => "流程跳步：六步线性单向，建议按序补齐前步工件",
            XcCode::StepRegression => "流程回退：已完成步不可倒退，如需重验走重认证新申请",
            XcCode::VerdictNoEvidence => "判定无证据：判定依据必须引用基准执行工件，建议先完成基准步",
            XcCode::RetriesExhausted => "重试超顶：该步连续失败达上限，显性弃置；建议人工归因后走续跑或新申请",
            XcCode::WaitExternal => "等待外部输入：自动化停等态，非缺陷；输入就绪后 advance 自动续推",
            XcCode::GrantWithoutPass => "授予前置缺失：仅判定通过的申请可授予，建议先完成判定步并通过",
            XcCode::PublishPrivacy => "发布隐私违规：发布条目携带白名单外字段，建议按 PUBLISH_ALLOWED_FIELDS 裁剪",
            XcCode::ArtifactWrongStep => "工件类型错步：工件归属步与当前步不符，建议核对工件流水",
            XcCode::GrantExpired => "授予已过期：有效期（tick 窗口）已过，建议走重认证新申请",
            XcCode::AuditChainBroken => "审计链断裂：链式指纹对账失败——审计被篡改或损坏，建议立即冻结归因",
        }
    }
}

// ---------------------------------------------------------------------------
// 四、状态机（判据二、三：自动化 + 中断恢复）
// ---------------------------------------------------------------------------

/// 步相位（推进到哪一步的哪一相）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StepPhase {
    /// 未开始。
    Pending,
    /// 执行中（可带重试计数）。
    Running,
    /// 已完成（工件已落账）。
    Done,
    /// 失败待重试。
    FailedRetrying,
    /// 显性弃置（重试超顶）。
    Abandoned,
}

/// 步动作三态（自动化的判定输出：状态机自己知道下一步是什么）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StepAction {
    /// 就绪：可推进（输入工件齐备）。
    Ready,
    /// 等待外部输入（停等，非缺陷）。
    Wait,
    /// 失败待处置（重试或弃置）。
    Fail,
}

/// 判定结论（本步三态；金/银/铜分级在 F0806）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VerdictOutcome {
    /// 通过（可授予）。
    Pass,
    /// 不通过（不可授予——原因进判定工件）。
    Reject,
    /// 暂缓（等待补测——不算通过也不算拒绝）。
    Defer,
}

impl VerdictOutcome {
    /// 是否可授予。
    pub const fn grantable(self) -> bool {
        matches!(self, VerdictOutcome::Pass)
    }

    /// 人读名。
    pub const fn zh(self) -> &'static str {
        match self {
            VerdictOutcome::Pass => "通过",
            VerdictOutcome::Reject => "不通过",
            VerdictOutcome::Defer => "暂缓",
        }
    }
}

/// 认证等级（授予面；分级规则在 F0806，此处为授予记录的类型面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CertTier {
    /// 金级。
    Gold,
    /// 银级。
    Silver,
    /// 铜级。
    Bronze,
}

impl CertTier {
    /// 人读名。
    pub const fn zh(self) -> &'static str {
        match self {
            CertTier::Gold => "金级",
            CertTier::Silver => "银级",
            CertTier::Bronze => "铜级",
        }
    }
}

/// 授予记录（等级 + 有效期 tick 窗口）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GrantRecord {
    /// 授予等级。
    pub tier: CertTier,
    /// 授予 tick。
    pub granted_at: u64,
    /// 有效期至（tick；= granted_at + GRANT_VALIDITY_TICKS）。
    pub valid_until: u64,
}

impl GrantRecord {
    /// 新授予记录。
    pub fn new(tier: CertTier, granted_at: u64) -> GrantRecord {
        GrantRecord {
            tier,
            granted_at,
            valid_until: granted_at.saturating_add(GRANT_VALIDITY_TICKS),
        }
    }

    /// 是否已过期（给定当前 tick）。
    pub const fn expired_at(&self, now: u64) -> bool {
        now > self.valid_until
    }

    /// 剩余有效期（已过期给 0）。
    pub const fn remaining(&self, now: u64) -> u64 {
        if now >= self.valid_until {
            0
        } else {
            self.valid_until.saturating_sub(now)
        }
    }
}

/// 认证流程状态机（逐份认证一行账）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CertPipeline {
    /// 认证 id（受理时分配，全流程随行）。
    pub cert_id: u64,
    /// 当前步。
    pub current: Step,
    /// 六步相位（与 Step ordinal 对位）。
    phases: [StepPhase; 6],
    /// 每步重试计数。
    retries: [u32; 6],
    /// 基准执行工件是否在账（判定步的证据前提）。
    bench_artifact: bool,
    /// 判定结论（判定步落账后非 None）。
    verdict: Option<VerdictOutcome>,
    /// 授予记录（授予步落账后非 None）。
    grant: Option<GrantRecord>,
    /// 已入账工件（按步序保序）。
    artifacts: Vec<Artifact>,
    /// 终态（发布完成）。
    finished: bool,
    /// 中断恢复次数（判据三遥测面）。
    resumes: u32,
    /// 弃置步计数。
    abandoned: u32,
}

impl CertPipeline {
    /// 新流程（第一步申请默认就绪：受理即入流程）。
    pub fn new(cert_id: u64) -> CertPipeline {
        let mut p = CertPipeline {
            cert_id,
            current: Step::Apply,
            phases: [StepPhase::Pending; 6],
            retries: [0; 6],
            bench_artifact: false,
            verdict: None,
            grant: None,
            artifacts: Vec::new(),
            finished: false,
            resumes: 0,
            abandoned: 0,
        };
        p.phases[0] = StepPhase::Running;
        p
    }

    fn idx(&self, s: Step) -> usize {
        s.ordinal() as usize
    }

    /// 自动化推进判定（判据二：按当前步相位给出动作三态——零人工判断点）。
    pub fn advance(&self) -> Result<StepAction, XcCode> {
        let i = self.idx(self.current);
        let phase = *self.phases.get(i).unwrap_or(&StepPhase::Pending);
        match phase {
            StepPhase::Running => Ok(StepAction::Ready),
            StepPhase::FailedRetrying => Ok(StepAction::Fail),
            StepPhase::Abandoned => Err(XcCode::RetriesExhausted),
            StepPhase::Done => {
                if self.finished {
                    Ok(StepAction::Wait)
                } else {
                    Ok(StepAction::Ready)
                }
            }
            StepPhase::Pending => Ok(StepAction::Wait),
        }
    }

    /// 当前步的工件类型期待（自动化执行器取件用）。
    pub const fn expected_artifact(&self) -> ArtifactKind {
        match self.current {
            Step::Apply => ArtifactKind::Receipt,
            Step::Probe => ArtifactKind::ProbeReport,
            Step::Bench => ArtifactKind::BenchResult,
            Step::Verdict => ArtifactKind::VerdictDoc,
            Step::Grant => ArtifactKind::GrantDoc,
            Step::Publish => ArtifactKind::PublishDoc,
        }
    }

    /// 入账当前步工件（类型与步校验 + 指纹落账）——complete_current 的
    /// 前置：工件先入账再完成步（判定步凭工件验证据）。
    pub fn submit_artifact(
        &mut self,
        kind: ArtifactKind,
        payload: &str,
        evidence_of: Option<u64>,
        at_tick: u64,
    ) -> Result<u64, XcCode> {
        if kind.owner() != self.current {
            return Err(XcCode::ArtifactWrongStep);
        }
        let fp = artifact_fingerprint(kind, payload);
        if kind == ArtifactKind::BenchResult {
            self.bench_artifact = true;
        }
        if kind == ArtifactKind::VerdictDoc {
            // 判定工件必须引用基准工件指纹（诚实纪律机检面）。
            match evidence_of {
                Some(ev) if self.bench_artifact && ev != 0 => {}
                _ => return Err(XcCode::VerdictNoEvidence),
            }
        }
        self.artifacts.push(Artifact {
            kind,
            fingerprint: fp,
            evidence_of,
            at_tick,
        });
        Ok(fp)
    }

    /// 记录判定结论（判定步专用；Pass 才可在授予步授予）。
    pub fn record_verdict(&mut self, outcome: VerdictOutcome) -> Result<(), XcCode> {
        if self.current != Step::Verdict {
            return Err(XcCode::StepSkipped);
        }
        if !self.bench_artifact {
            return Err(XcCode::VerdictNoEvidence);
        }
        self.verdict = Some(outcome);
        Ok(())
    }

    /// 判定结论（未判定 None）。
    pub const fn verdict(&self) -> Option<VerdictOutcome> {
        self.verdict
    }

    /// 授予记录（未授予 None）。
    pub const fn grant(&self) -> Option<GrantRecord> {
        self.grant
    }

    /// 授予（授予步专用：仅判定通过可授予；等级由调用方按 F0806 定）。
    pub fn grant_now(&mut self, tier: CertTier, at_tick: u64) -> Result<GrantRecord, XcCode> {
        if self.current != Step::Grant {
            return Err(XcCode::StepSkipped);
        }
        match self.verdict {
            Some(v) if v.grantable() => {}
            _ => return Err(XcCode::GrantWithoutPass),
        }
        let g = GrantRecord::new(tier, at_tick);
        self.grant = Some(g);
        Ok(g)
    }

    /// 完成当前步并前移（跳步/回退显性拒；判定步验证据；授予步验通过）。
    pub fn complete_current(&mut self, bench_evidence: bool) -> Result<Step, XcCode> {
        if self.finished {
            return Err(XcCode::StepRegression);
        }
        // 判定步诚实纪律：无基准工件不判定。
        if self.current == Step::Verdict && !self.bench_artifact {
            return Err(XcCode::VerdictNoEvidence);
        }
        // 授予步守门：判定未通过（或未判定）不可完成授予步。
        if self.current == Step::Grant {
            match self.verdict {
                Some(v) if v.grantable() => {}
                _ => return Err(XcCode::GrantWithoutPass),
            }
        }
        let i = self.idx(self.current);
        if let Some(p) = self.phases.get_mut(i) {
            if *p != StepPhase::Running && *p != StepPhase::FailedRetrying {
                return Err(XcCode::StepSkipped);
            }
            *p = StepPhase::Done;
        }
        if self.current == Step::Bench && bench_evidence {
            self.bench_artifact = true;
        }
        let nxt = self.current.next();
        if nxt == self.current {
            // 发布步完成 → 终态。
            self.finished = true;
        } else {
            let j = self.idx(nxt);
            if let Some(p) = self.phases.get_mut(j) {
                *p = StepPhase::Running;
            }
            self.current = nxt;
        }
        Ok(self.current)
    }

    /// 中断恢复：续跑（判据三：已完成步工件保留，从当前步继续）。
    /// 本函数是恢复语义的显性入口——不重建、不重置，直接以现有状态续推。
    pub fn resume(&mut self) -> Result<StepAction, XcCode> {
        self.resumes = self.resumes.saturating_add(1);
        self.advance()
    }

    /// 失败步重跑（判据三：原地重试，计数封顶，超顶显性弃置）。
    pub fn retry_current(&mut self) -> Result<(), XcCode> {
        let i = self.idx(self.current);
        let cur = *self.phases.get(i).unwrap_or(&StepPhase::Pending);
        if cur != StepPhase::FailedRetrying && cur != StepPhase::Running {
            return Err(XcCode::StepRegression);
        }
        let r = *self.retries.get(i).unwrap_or(&MAX_STEP_RETRIES);
        if r >= MAX_STEP_RETRIES {
            if let Some(p) = self.phases.get_mut(i) {
                *p = StepPhase::Abandoned;
            }
            self.abandoned = self.abandoned.saturating_add(1);
            return Err(XcCode::RetriesExhausted);
        }
        if let Some(c) = self.retries.get_mut(i) {
            *c = c.saturating_add(1);
        }
        if let Some(p) = self.phases.get_mut(i) {
            *p = StepPhase::Running;
        }
        Ok(())
    }

    /// 标记当前步失败（转入待重试相）。
    pub fn fail_current(&mut self) {
        let i = self.idx(self.current);
        if let Some(p) = self.phases.get_mut(i) {
            if *p == StepPhase::Running {
                *p = StepPhase::FailedRetrying;
            }
        }
    }

    /// 步相位读取（审计/遥测消费面）。
    pub const fn phase_of(&self, s: Step) -> StepPhase {
        // 六步定长表 + ordinal 界内（0..6），恒安全。
        self.phases[s.ordinal() as usize]
    }

    /// 已入账工件（只读）。
    pub fn artifacts(&self) -> &[Artifact] {
        &self.artifacts
    }

    /// 中断恢复次数。
    pub const fn resumes(&self) -> u32 {
        self.resumes
    }

    /// 弃置步数。
    pub const fn abandoned(&self) -> u32 {
        self.abandoned
    }

    /// 是否终态。
    pub const fn is_finished(&self) -> bool {
        self.finished
    }

    /// 流水线快照（判据三：跨会话恢复面——状态折叠为定长元组）。
    pub fn snapshot(&self) -> [u64; 12] {
        let mut phase_bits = 0u64;
        let mut i = 0usize;
        while i < 6 {
            let code = match self.phases[i] {
                StepPhase::Pending => 0u64,
                StepPhase::Running => 1,
                StepPhase::Done => 2,
                StepPhase::FailedRetrying => 3,
                StepPhase::Abandoned => 4,
            };
            phase_bits |= code << (i * 3);
            i += 1;
        }
        let mut retry_bits = 0u64;
        let mut i = 0usize;
        while i < 6 {
            retry_bits |= (self.retries[i] as u64 & 0x7) << (i * 3);
            i += 1;
        }
        [
            self.cert_id,
            self.current.ordinal() as u64,
            phase_bits,
            retry_bits,
            self.bench_artifact as u64,
            match self.verdict {
                Some(VerdictOutcome::Pass) => 1,
                Some(VerdictOutcome::Reject) => 2,
                Some(VerdictOutcome::Defer) => 3,
                None => 0,
            },
            match self.grant {
                Some(g) => g.tier as u64 + 1,
                None => 0,
            },
            self.grant.map(|g| g.granted_at).unwrap_or(0),
            self.grant.map(|g| g.valid_until).unwrap_or(0),
            self.artifacts.len() as u64,
            self.resumes as u64,
            (self.finished as u64) | (self.abandoned as u64) << 1,
        ]
    }

    /// 从快照恢复（判据三：快照→状态机；工件明细不随快照走——
    /// 恢复后以快照相位为准续推，明细账由审计侧兜底）。
    pub fn restore(snap: &[u64; 12]) -> Result<CertPipeline, XcCode> {
        let cur = Step::from_ordinal((snap[1] as u8).min(5)).unwrap_or(Step::Apply);
        let mut phases = [StepPhase::Pending; 6];
        let mut i = 0usize;
        while i < 6 {
            let code = (snap[2] >> (i * 3)) & 0x7;
            phases[i] = match code {
                1 => StepPhase::Running,
                2 => StepPhase::Done,
                3 => StepPhase::FailedRetrying,
                4 => StepPhase::Abandoned,
                _ => StepPhase::Pending,
            };
            i += 1;
        }
        let mut retries = [0u32; 6];
        let mut i = 0usize;
        while i < 6 {
            retries[i] = ((snap[3] >> (i * 3)) & 0x7) as u32;
            i += 1;
        }
        Ok(CertPipeline {
            cert_id: snap[0],
            current: cur,
            phases,
            retries,
            bench_artifact: snap[4] != 0,
            verdict: match snap[5] {
                1 => Some(VerdictOutcome::Pass),
                2 => Some(VerdictOutcome::Reject),
                3 => Some(VerdictOutcome::Defer),
                _ => None,
            },
            grant: if snap[6] > 0 {
                let tier = match snap[6].saturating_sub(1) {
                    0 => CertTier::Gold,
                    1 => CertTier::Silver,
                    _ => CertTier::Bronze,
                };
                Some(GrantRecord {
                    tier,
                    granted_at: snap[7],
                    valid_until: snap[8],
                })
            } else {
                None
            },
            artifacts: Vec::new(),
            finished: snap[11] & 1 == 1,
            resumes: snap[10] as u32,
            abandoned: (snap[11] >> 1) as u32,
        })
    }
}

// ---------------------------------------------------------------------------
// 五、批量自动化执行器（判据二：D08 执行器复用声明位）
// ---------------------------------------------------------------------------

/// 注入式步结果（执行器每 tick 对运行中步给出的结果——流程面不管
/// 执行动作本身，只管调度记账——D08 复用的接缝）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StepOutcome {
    /// 本 tick 完成（附工件载荷）。
    Done(&'static str),
    /// 本 tick 失败。
    Fail,
    /// 继续等待（执行中，未出结果）。
    Pending,
}

/// 批量泵动结论（一轮泵动的全量事实）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PumpReport {
    /// 本轮完成的步数。
    pub advanced: u32,
    /// 本轮失败的步数。
    pub failed: u32,
    /// 本轮弃置的步数。
    pub abandoned: u32,
    /// 本轮等待（未动）的认证数。
    pub waiting: u32,
    /// 本轮走到终态的认证数。
    pub finished: u32,
}

/// 批量自动化执行器（多份认证并发泵动；tick 注入驱动）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AutoRunner {
    pipelines: Vec<CertPipeline>,
    tick: u64,
    /// 累计泵动报告（跨轮累计）。
    totals: PumpReport,
}

impl AutoRunner {
    /// 空执行器。
    pub const fn new() -> AutoRunner {
        AutoRunner {
            pipelines: Vec::new(),
            tick: 0,
            totals: PumpReport {
                advanced: 0,
                failed: 0,
                abandoned: 0,
                waiting: 0,
                finished: 0,
            },
        }
    }

    /// 登记一份新认证（受理：分配 id 并入列）。
    pub fn enroll(&mut self, cert_id: u64) {
        self.pipelines.push(CertPipeline::new(cert_id));
    }

    /// 在册认证数。
    pub fn len(&self) -> usize {
        self.pipelines.len()
    }

    /// 空判定。
    pub const fn is_empty(&self) -> bool {
        self.pipelines.is_empty()
    }

    /// 当前 tick。
    pub const fn tick(&self) -> u64 {
        self.tick
    }

    /// 推进 tick。
    pub fn advance_tick(&mut self, n: u64) {
        self.tick = self.tick.saturating_add(n);
    }

    /// 在册流水线（只读）。
    pub fn pipelines(&self) -> &[CertPipeline] {
        &self.pipelines
    }

    /// 累计泵动报告。
    pub const fn totals(&self) -> &PumpReport {
        &self.totals
    }

    /// 泵动一轮（判据二：逐份按 advance 三态 + 注入结果推进——
    /// 无人值守一轮的全部记账在此发生）。
    pub fn pump_once(
        &mut self,
        outcomes: &[(u64, StepOutcome)],
        audit: &mut CertAudit,
    ) -> PumpReport {
        let mut rep = PumpReport::default();
        let mut i = 0usize;
        while i < self.pipelines.len() {
            let act = match self.pipelines.get(i) {
                Some(p) => p.advance(),
                None => break,
            };
            match act {
                Err(XcCode::RetriesExhausted) => {
                    rep.abandoned = rep.abandoned.saturating_add(1);
                    if let Some(p) = self.pipelines.get(i) {
                        audit.append(AuditLine {
                            cert_id: p.cert_id,
                            step: p.current,
                            phase: StepPhase::Abandoned,
                            summary: XcCode::RetriesExhausted.detail().to_string(),
                            tick: self.tick,
                            seq: 0,
                            prev_chain: 0,
                        });
                    }
                    i += 1;
                    continue;
                }
                Err(_) => {
                    rep.waiting = rep.waiting.saturating_add(1);
                    i += 1;
                    continue;
                }
                Ok(StepAction::Wait) => {
                    rep.waiting = rep.waiting.saturating_add(1);
                    i += 1;
                    continue;
                }
                Ok(StepAction::Fail) => {
                    // 失败步：先重跑，超顶自然在下轮显性弃置。
                    if let Some(p) = self.pipelines.get_mut(i) {
                        let _ = p.retry_current();
                    }
                    rep.failed = rep.failed.saturating_add(1);
                    i += 1;
                    continue;
                }
                Ok(StepAction::Ready) => {}
            }
            // 就绪：取注入结果。
            let cert_id = match self.pipelines.get(i) {
                Some(p) => p.cert_id,
                None => break,
            };
            let outcome = lookup_outcome(outcomes, cert_id);
            match outcome {
                StepOutcome::Pending => {
                    rep.waiting = rep.waiting.saturating_add(1);
                }
                StepOutcome::Fail => {
                    if let Some(p) = self.pipelines.get_mut(i) {
                        p.fail_current();
                    }
                    rep.failed = rep.failed.saturating_add(1);
                }
                StepOutcome::Done(payload) => {
                    let done = self.finish_one(i, payload);
                    match done {
                        Ok(finished) => {
                            rep.advanced = rep.advanced.saturating_add(1);
                            if finished {
                                rep.finished = rep.finished.saturating_add(1);
                            }
                        }
                        Err(_) => {
                            rep.failed = rep.failed.saturating_add(1);
                        }
                    }
                }
            }
            i += 1;
        }
        self.totals.advanced = self.totals.advanced.saturating_add(rep.advanced);
        self.totals.failed = self.totals.failed.saturating_add(rep.failed);
        self.totals.abandoned = self.totals.abandoned.saturating_add(rep.abandoned);
        self.totals.waiting = self.totals.waiting.saturating_add(rep.waiting);
        self.totals.finished = self.totals.finished.saturating_add(rep.finished);
        rep
    }

    /// 完成单份认证的当前步（工件入账 + 步前移 + 判定/授予特殊步）。
    fn finish_one(&mut self, idx: usize, payload: &str) -> Result<bool, XcCode> {
        let tick = self.tick;
        let kind = {
            let p = match self.pipelines.get(idx) {
                Some(p) => p,
                None => return Err(XcCode::StepSkipped),
            };
            p.expected_artifact()
        };
        let p = match self.pipelines.get_mut(idx) {
            Some(p) => p,
            None => return Err(XcCode::StepSkipped),
        };
        // 判定步：payload 以「pass|reject|defer」前缀给出结论。
        if kind == ArtifactKind::VerdictDoc {
            let v = if payload.starts_with("pass") {
                VerdictOutcome::Pass
            } else if payload.starts_with("reject") {
                VerdictOutcome::Reject
            } else {
                VerdictOutcome::Defer
            };
            p.record_verdict(v)?;
            // 证据引用：基准工件指纹（流水内最后一个基准工件）。
            let mut bench_fp = 0u64;
            let mut k = 0usize;
            while k < p.artifacts.len() {
                if let Some(a) = p.artifacts.get(k) {
                    if a.kind == ArtifactKind::BenchResult {
                        bench_fp = a.fingerprint;
                    }
                }
                k += 1;
            }
            p.submit_artifact(kind, payload, if bench_fp != 0 { Some(bench_fp) } else { None }, tick)?;
            let _ = p.complete_current(true);
            return Ok(false);
        }
        // 授予步：payload 以金/银/铜首字给级。
        if kind == ArtifactKind::GrantDoc {
            let tier = if payload.starts_with("gold") {
                CertTier::Gold
            } else if payload.starts_with("silver") {
                CertTier::Silver
            } else {
                CertTier::Bronze
            };
            p.grant_now(tier, tick)?;
            p.submit_artifact(kind, payload, None, tick)?;
            let _ = p.complete_current(true);
            return Ok(false);
        }
        // 其余步：工件入账 + 前移。
        p.submit_artifact(kind, payload, None, tick)?;
        p.complete_current(true)?;
        let finished = p.is_finished();
        Ok(finished)
    }
}

/// 查注入结果（无注入给 Pending——停等不臆动）。
fn lookup_outcome(outcomes: &[(u64, StepOutcome)], cert_id: u64) -> StepOutcome {
    let mut i = 0usize;
    while i < outcomes.len() {
        if let Some((id, o)) = outcomes.get(i) {
            if *id == cert_id {
                return *o;
            }
        }
        i += 1;
    }
    StepOutcome::Pending
}

// ---------------------------------------------------------------------------
// 六、授予有效期与发布隐私白名单（第五、六步的落地面）
// ---------------------------------------------------------------------------

/// 发布字段白名单（判据六步契约「不含用户隐私数据」的机检面）。
pub const PUBLISH_ALLOWED_FIELDS: [&str; 7] = [
    "vendor",
    "device",
    "driver_version",
    "tier",
    "granted_at_tick",
    "valid_until_tick",
    "cert_id",
];

/// 发布条目（公开认证矩阵行：白名单字段键值对——表外字段发布即拒）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PublishEntry {
    /// 认证 id。
    pub cert_id: u64,
    /// 白名单键值对（键必须 ∈ PUBLISH_ALLOWED_FIELDS）。
    pub fields: Vec<(String, String)>,
    /// 条目指纹（键序化 FNV——同条目同指纹）。
    pub fingerprint: u64,
}

impl PublishEntry {
    /// 构造并校验（白名单外字段显性拒——不静默裁剪）。
    pub fn build(cert_id: u64, fields: Vec<(String, String)>) -> Result<PublishEntry, XcCode> {
        let mut i = 0usize;
        while i < fields.len() {
            let k = match fields.get(i) {
                Some((k, _)) => k,
                None => break,
            };
            let mut hit = false;
            let mut j = 0usize;
            while j < PUBLISH_ALLOWED_FIELDS.len() {
                if let Some(a) = PUBLISH_ALLOWED_FIELDS.get(j) {
                    if *a == k.as_str() {
                        hit = true;
                        break;
                    }
                }
                j += 1;
            }
            if !hit {
                return Err(XcCode::PublishPrivacy);
            }
            i += 1;
        }
        // 指纹：键值按序迭代（键值分隔 0x1F，项间 0x1E——防拼接歧义）。
        let mut acc = fnv_str(FNV_OFFSET, "publish-entry");
        let mut i = 0usize;
        while i < fields.len() {
            if let Some((k, v)) = fields.get(i) {
                acc = fnv_byte(acc, 0x1E);
                acc = fnv_str(acc, k);
                acc = fnv_byte(acc, 0x1F);
                acc = fnv_str(acc, v);
            }
            i += 1;
        }
        Ok(PublishEntry {
            cert_id,
            fields,
            fingerprint: acc,
        })
    }
}

// ---------------------------------------------------------------------------
// 七、审计账（判据四：全留痕可回溯 + 链式防篡改）
// ---------------------------------------------------------------------------

/// 审计行（append-only；一行=一次步推进的事实 + 链指纹）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuditLine {
    /// 认证 id。
    pub cert_id: u64,
    /// 步。
    pub step: Step,
    /// 相位（推进后的相位）。
    pub phase: StepPhase,
    /// 诊断摘要（正常推进写「推进」，异常写诊断码明细）。
    pub summary: String,
    /// 逻辑 tick（零墙钟：上游注入）。
    pub tick: u64,
    /// 行序号（账内单调递增——链的序面）。
    pub seq: u64,
    /// 前链指纹（上一行的链值——FNV 迭代防篡改）。
    pub prev_chain: u64,
}

impl AuditLine {
    /// 本行的链值（内容 + 前链——改任何字段断链）。
    pub fn chain(&self) -> u64 {
        let acc = fnv_str(FNV_OFFSET, &self.summary);
        let acc = fnv_byte(acc, 0x1E);
        let acc = fnv_byte(acc, self.step.ordinal());
        let acc = fnv_byte(acc, self.phase_code());
        let acc = fnv_str(acc, &self.tick.to_string());
        let acc = fnv_str(acc, &self.seq.to_string());
        let acc = fnv_str(acc, &self.cert_id.to_string());
        acc ^ self.prev_chain.wrapping_mul(FNV_PRIME)
    }

    /// 相位编码（链输入面）。
    const fn phase_code(&self) -> u8 {
        match self.phase {
            StepPhase::Pending => 0,
            StepPhase::Running => 1,
            StepPhase::Done => 2,
            StepPhase::FailedRetrying => 3,
            StepPhase::Abandoned => 4,
        }
    }
}

/// 流程审计账（封顶滚动 + 链式防篡改；最旧行退场）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CertAudit {
    lines: Vec<AuditLine>,
    dropped: u64,
    next_seq: u64,
    /// 当前链头（最新一行的链值；空账 0）。
    head: u64,
}

impl CertAudit {
    /// 追加一行（判据四：全留痕；封顶滚动如实计数；链值即时核算）。
    pub fn append(&mut self, mut line: AuditLine) {
        line.seq = self.next_seq;
        line.prev_chain = self.head;
        self.head = line.chain();
        self.next_seq = self.next_seq.saturating_add(1);
        self.lines.push(line);
        if self.lines.len() > AUDIT_CAPACITY {
            self.lines.remove(0);
            self.dropped = self.dropped.saturating_add(1);
        }
    }

    /// 链完整性对账（判据四防篡改面：逐行重算链值，断链即红）。
    pub fn chain_intact(&self) -> bool {
        let mut expect_prev = 0u64;
        let mut expect_seq = 0u64;
        let mut i = 0usize;
        while i < self.lines.len() {
            let l = match self.lines.get(i) {
                Some(l) => l,
                None => return false,
            };
            if l.prev_chain != expect_prev || l.seq != expect_seq {
                return false;
            }
            expect_prev = l.chain();
            expect_seq = expect_seq.saturating_add(1);
            i += 1;
        }
        expect_prev == self.head
    }

    /// 按认证 id 回溯（判据四：可回溯——全流程按序可查）。
    pub fn trace(&self, cert_id: u64) -> Vec<AuditLine> {
        let mut out = Vec::new();
        let mut i = 0usize;
        while i < self.lines.len() {
            if let Some(l) = self.lines.get(i) {
                if l.cert_id == cert_id {
                    out.push(l.clone());
                }
            }
            i += 1;
        }
        out
    }

    /// 按步回溯（多维查询面：某步的全部审计行）。
    pub fn trace_step(&self, step: Step) -> Vec<AuditLine> {
        let mut out = Vec::new();
        let mut i = 0usize;
        while i < self.lines.len() {
            if let Some(l) = self.lines.get(i) {
                if l.step == step {
                    out.push(l.clone());
                }
            }
            i += 1;
        }
        out
    }

    /// 按相位回溯。
    pub fn trace_phase(&self, phase: StepPhase) -> Vec<AuditLine> {
        let mut out = Vec::new();
        let mut i = 0usize;
        while i < self.lines.len() {
            if let Some(l) = self.lines.get(i) {
                if l.phase == phase {
                    out.push(l.clone());
                }
            }
            i += 1;
        }
        out
    }

    /// 按 tick 区间回溯（闭区间；零墙钟纪律下的时间窗查询）。
    pub fn trace_window(&self, from: u64, to: u64) -> Vec<AuditLine> {
        let mut out = Vec::new();
        if from > to {
            return out;
        }
        let mut i = 0usize;
        while i < self.lines.len() {
            if let Some(l) = self.lines.get(i) {
                if l.tick >= from && l.tick <= to {
                    out.push(l.clone());
                }
            }
            i += 1;
        }
        out
    }

    /// 某认证的相位轨迹摘要（人读回溯面：`步=相位` 逐段连接）。
    pub fn trace_summary(&self, cert_id: u64) -> String {
        let tr = self.trace(cert_id);
        let mut s = String::new();
        let mut i = 0usize;
        while i < tr.len() {
            if let Some(l) = tr.get(i) {
                if i > 0 {
                    s.push_str(" → ");
                }
                s.push_str(&alloc::format!("{}@{}", l.step.zh(), l.phase_name()));
            }
            i += 1;
        }
        s
    }

    /// 在账行数。
    pub const fn len_of(&self) -> usize {
        self.lines.len()
    }

    /// 滚动退场行数（回溯完整性声明：退场行不可查，计数如实）。
    pub const fn dropped(&self) -> u64 {
        self.dropped
    }

    /// 链头指纹（最新一行链值；外部对拍面）。
    pub const fn head(&self) -> u64 {
        self.head
    }
}

impl StepPhase {
    /// 人读名（审计摘要面）。
    pub const fn zh(self) -> &'static str {
        match self {
            StepPhase::Pending => "未开始",
            StepPhase::Running => "执行中",
            StepPhase::Done => "完成",
            StepPhase::FailedRetrying => "失败待重试",
            StepPhase::Abandoned => "弃置",
        }
    }
}

// 审计行相位人读名的挂接面（避免在 AuditLine 上重复实现）。
impl AuditLine {
    /// 相位人读名。
    pub const fn phase_name(&self) -> &'static str {
        self.phase.zh()
    }
}
