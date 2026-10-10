//! CGPU-F0802 · 认证流程体系（CGPU-F 域 · 硬件兼容与认证域 · F01 组 · 目标 400 行）。
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
//! [`XcCode::StepRegression`]）都显性拒。
//!
//! ## 二、流程自动化：状态机驱动，无人值守（判据二）
//!
//! [`CertPipeline`] 是逐份认证的状态机：step 游标 + 每步工件（artifact）。
//! 自动化入口 [`CertPipeline::advance`] 按当前步取动作（[`StepAction`]
//! 三态：就绪可推进/等待外部输入/失败待处置），推进逻辑零人工判断点
//! ——无人值守的落法是「状态机自己知道下一步是什么」。
//!
//! ## 三、中断恢复：续跑不重做，重跑可重入（判据三）
//!
//! 中断语义两条（[`CertPipeline::resume`]）：已完成步的工件保留（续跑
//! 不重做——基准执行代价高，重做即浪费）；失败步支持原地重跑
//! （[`CertPipeline::retry_current`]，重试计数封顶 [`MAX_STEP_RETRIES`]，
//! 超顶显性转 [`StepPhase::Abandoned`]——不静默无限重试）。
//!
//! ## 四、流程审计：全留痕可回溯（判据四）
//!
//! 每次步推进追加一条审计行（[`AuditLine`]：认证 id/步/相位/诊断摘要/
//! tick）——append-only，封顶滚动（配额 [`AUDIT_CAPACITY`]）；审计是
//! 回溯的唯一依据：出纠纷看账，不靠回忆。
//!
//! ## 五、判定步的诚实纪律
//!
//! 判定（[`Step::Verdict`]）输出三态结论（金/银/铜/不认证的分级判定
//! 在 F0806，本步只产出「过/不过 + 依据」）；依据必须引用基准执行步
//! 的工件（无工件依据 = 判定无效 [`XcCode::VerdictNoEvidence`]）——
//! 不凭印象发证。
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

// ---------------------------------------------------------------------------
// 三、诊断码（0x5F 段独占：F01 认证流程面）
// ---------------------------------------------------------------------------

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
    /// 终态（发布完成）。
    finished: bool,
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
            finished: false,
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

    /// 完成当前步并前移（跳步/回退显性拒；判定步验证据）。
    pub fn complete_current(&mut self, bench_evidence: bool) -> Result<Step, XcCode> {
        if self.finished {
            return Err(XcCode::StepRegression);
        }
        // 判定步诚实纪律：无基准工件不判定。
        if self.current == Step::Verdict && !self.bench_artifact {
            return Err(XcCode::VerdictNoEvidence);
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
    pub fn resume(&self) -> Result<StepAction, XcCode> {
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
        self.phases[s.ordinal() as usize]
    }

    /// 是否终态。
    pub const fn is_finished(&self) -> bool {
        self.finished
    }
}

// ---------------------------------------------------------------------------
// 五、审计账（判据四：全留痕可回溯）
// ---------------------------------------------------------------------------

/// 审计行（append-only；一行=一次步推进的事实）。
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
}

/// 流程审计账（封顶滚动；最旧行退场）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CertAudit {
    lines: Vec<AuditLine>,
    dropped: u64,
}

impl CertAudit {
    /// 追加一行（判据四：全留痕；封顶滚动如实计数）。
    pub fn append(&mut self, line: AuditLine) {
        self.lines.push(line);
        if self.lines.len() > AUDIT_CAPACITY {
            self.lines.remove(0);
            self.dropped = self.dropped.saturating_add(1);
        }
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

    /// 在账行数。
    pub const fn len_of(&self) -> usize {
        self.lines.len()
    }

    /// 滚动退场行数（回溯完整性声明：退场行不可查，计数如实）。
    pub const fn dropped(&self) -> u64 {
        self.dropped
    }
}
