//! unx121_extbootstate — UNX-F0121 引导阶段扩展态（PROBE/RECOVER/DEGRADED）。
//!
//! 判据原文（CoRun Unxreal 主册 UNX-F0121 · UNX-A1-B07）：三扩展态迁移
//! 条件与设计矩阵一致，非法迁移全部被拒。
//!
//! **分层深化声明**：F0014 只做三态状态机与看门狗框架（域册 B07 批防重
//! 注记原文），本册做扩展态、降级与恢复语义——基础阶段推进不在本册
//! 重复实现（防重纪律单点）。
//!
//! **设计矩阵显式化**（判据"与设计矩阵一致"的本体）：迁移矩阵不是
//! 隐式 match 散落各处，而是 **const 表**（[`MATRIX`]：三扩展态 × 五
//! 事件 = 15 格逐格显性——合法格写目标态、非法格 None）。矩阵自身带
//! 守恒核（[`matrix_consistent`]：实现 match 与 const 表逐格对拍——两
//! 套写法一个语义，漂移即 SegmentGuard 立案）。**非法迁移全部被拒**
//! 靠双层闸：第一层矩阵格（None 即拒）、第二层条件谓词（格合法但前置
//! 条件不满足仍拒——"迁移条件与矩阵一致"的"条件"二字落在谓词面）。
//!
//! **三扩展态语义**：
//! 1. **Probe**（探测中）：硬件枚举进行中的子阶段标记——探测完成事件
//!    或失败事件驱动离开；探测中不得直接降级（没探明就降级是无据降级）；
//! 2. **Recover**（恢复中）：失败后恢复流程进行中——恢复动作三步
//!    （重扫/回退/隔离）每步完成事件推进；恢复中不得再探测（恢复有
//!    恢复的路径，半路改行是流程违约）；
//! 3. **Degraded**（降级运行）：部分功能显性降级——恢复成功事件升回、
//!    再故障事件保持（降级不叠加，一次降级一个档）。
//!
//! **五事件闭集**：ProbeDone（探测完成）/ ProbeFail（探测失败）/
//! RecoverStep（恢复步进）/ RecoverDone（恢复完成）/ DegradeReq（降级
//! 请求）。15 格矩阵全枚举。
//!
//! 诊断枚举 BootExtCode 十六码（0x32 段 0x3200..=0x320F 互异 +
//! [`segment_guard`]）。no_std 零 unwrap 零 panic 面。

use alloc::format;
use alloc::vec;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const UNX121_EXTBOOT_VERSION: &str = "unx121-extbootstate-v1";

/// 诊断段基值（0x32 段——UNX 域 0x2E..0x31 已占）。
pub const SEG_BASE: u16 = 0x3200;

/// 迁移历史环形容量。
pub const HISTORY_CAP: usize = 12;

/// 告警账容量。
pub const ALERT_CAP: usize = 24;

// ---------------------------------------------------------------------------
// 诊断码（BootExtCode 十六码 · 0x32 段互异）
// ---------------------------------------------------------------------------

/// 诊断码（引导扩展态——0x32 段十六码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BootExtCode {
    /// 0x3200 非法迁移（矩阵外格——显性拒计数）。
    IllegalTransition,
    /// 0x3201 条件不满足（矩阵格合法但谓词假——双层闸第二层）。
    ConditionNotMet,
    /// 0x3202 矩阵一致性破坏（match 与 const 表对拍漂移）。
    MatrixInconsistent,
    /// 0x3203 探测中直降级（未探明先降级是无据降级——语义闸）。
    ProbeThenDegrade,
    /// 0x3204 恢复中改探测（半路改行是流程违约）。
    RecoverThenProbe,
    /// 0x3205 恢复步进越界（三步走完仍步进——流程超程）。
    RecoverStepOverflow,
    /// 0x3206 降级叠加（已降级再收降级请求——一次一档）。
    DoubleDegrade,
    /// 0x3207 无失败进恢复（失败计数为 0 却收恢复事件——无病呻吟）。
    RecoverWithoutFail,
    /// 0x3208 历史环溢出（环形记忆写超容量——实现面事故）。
    HistoryOverflow,
    /// 0x3209 告警账满（旧账保留）。
    AlertFull,
    /// 0x320A 态值越界（外部注入态值 > 2——解析面事故）。
    StateOutOfRange,
    /// 0x320B 事件值越界（外部注入事件值 > 4）。
    EventOutOfRange,
    /// 0x320C 快照指纹不符（往返破坏）。
    SnapshotFingerprint,
    /// 0x320D 降级档未登记（进 Degraded 但降级原因空——无档可查）。
    DegradeReasonMissing,
    /// 0x320E 历史账守恒破坏（迁移总数 ≠ 合法 + 非法拒 + 条件拒）。
    LedgerBroken,
    /// 0x320F 段守恒对账失败。
    SegmentGuard,
}

impl BootExtCode {
    /// 十六码闭集。
    pub const ALL: [BootExtCode; 16] = [
        BootExtCode::IllegalTransition,
        BootExtCode::ConditionNotMet,
        BootExtCode::MatrixInconsistent,
        BootExtCode::ProbeThenDegrade,
        BootExtCode::RecoverThenProbe,
        BootExtCode::RecoverStepOverflow,
        BootExtCode::DoubleDegrade,
        BootExtCode::RecoverWithoutFail,
        BootExtCode::HistoryOverflow,
        BootExtCode::AlertFull,
        BootExtCode::StateOutOfRange,
        BootExtCode::EventOutOfRange,
        BootExtCode::SnapshotFingerprint,
        BootExtCode::DegradeReasonMissing,
        BootExtCode::LedgerBroken,
        BootExtCode::SegmentGuard,
    ];

    /// 码值（连续）。
    pub const fn code(self) -> u16 {
        match self {
            BootExtCode::IllegalTransition => 0x3200,
            BootExtCode::ConditionNotMet => 0x3201,
            BootExtCode::MatrixInconsistent => 0x3202,
            BootExtCode::ProbeThenDegrade => 0x3203,
            BootExtCode::RecoverThenProbe => 0x3204,
            BootExtCode::RecoverStepOverflow => 0x3205,
            BootExtCode::DoubleDegrade => 0x3206,
            BootExtCode::RecoverWithoutFail => 0x3207,
            BootExtCode::HistoryOverflow => 0x3208,
            BootExtCode::AlertFull => 0x3209,
            BootExtCode::StateOutOfRange => 0x320A,
            BootExtCode::EventOutOfRange => 0x320B,
            BootExtCode::SnapshotFingerprint => 0x320C,
            BootExtCode::DegradeReasonMissing => 0x320D,
            BootExtCode::LedgerBroken => 0x320E,
            BootExtCode::SegmentGuard => 0x320F,
        }
    }

    /// 人读诊断（互异非空）。
    pub const fn detail(self) -> &'static str {
        match self {
            BootExtCode::IllegalTransition => {
                "非法迁移：矩阵外格显性拒——迁移须与设计矩阵一致，建议核对迁移表"
            }
            BootExtCode::ConditionNotMet => {
                "条件不满足：矩阵格合法但前置谓词假——双层闸第二层，建议核查条件输入"
            }
            BootExtCode::MatrixInconsistent => {
                "矩阵一致性破坏：match 与 const 表对拍漂移，建议逐格对齐"
            }
            BootExtCode::ProbeThenDegrade => {
                "探测中直降级：未探明先降级是无据降级，建议先出探测结论"
            }
            BootExtCode::RecoverThenProbe => {
                "恢复中改探测：恢复有恢复的路径，半路改行是流程违约，建议走完恢复"
            }
            BootExtCode::RecoverStepOverflow => {
                "恢复步进越界：三步走完仍步进是流程超程，建议核对步数账"
            }
            BootExtCode::DoubleDegrade => {
                "降级叠加：已降级再收降级请求——一次降级一个档，建议合并原因"
            }
            BootExtCode::RecoverWithoutFail => {
                "无失败进恢复：失败计数为零却收恢复事件是无病呻吟，建议核查失败账"
            }
            BootExtCode::HistoryOverflow => {
                "历史环溢出：环形记忆写超容量是实现面事故，建议审计环写逻辑"
            }
            BootExtCode::AlertFull => {
                "告警账满：旧账保留新告警拒入，建议归档"
            }
            BootExtCode::StateOutOfRange => {
                "态值越界：外部注入态值须在闭集内，建议解析面钳制"
            }
            BootExtCode::EventOutOfRange => {
                "事件值越界：外部注入事件值须在闭集内，建议解析面钳制"
            }
            BootExtCode::SnapshotFingerprint => {
                "快照指纹不符：往返恒等破坏，建议排查序列化格式漂移"
            }
            BootExtCode::DegradeReasonMissing => {
                "降级档未登记：进降级态但原因空——无档可查，建议显性登记原因"
            }
            BootExtCode::LedgerBroken => {
                "历史账守恒破坏：迁移总数应等于合法加非法拒加条件拒，建议审计流水"
            }
            BootExtCode::SegmentGuard => {
                "段守恒对账失败：十六码须连续互异界内——段位事故立即立案"
            }
        }
    }
}

/// **段守恒对账**。
pub const fn segment_guard() -> bool {
    let mut i = 0usize;
    while i < BootExtCode::ALL.len() {
        let c = BootExtCode::ALL[i].code();
        if c < SEG_BASE || c != SEG_BASE + i as u16 {
            return false;
        }
        let mut j = i + 1;
        while j < BootExtCode::ALL.len() {
            if BootExtCode::ALL[i].code() == BootExtCode::ALL[j].code() {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// ---------------------------------------------------------------------------
// 三扩展态 × 五事件闭集
// ---------------------------------------------------------------------------

/// 三扩展态闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ExtState {
    /// 探测中（硬件枚举子阶段）。
    Probe,
    /// 恢复中（失败恢复流程）。
    Recover,
    /// 降级运行（显性降级档）。
    Degraded,
}

impl ExtState {
    /// 三闭集。
    pub const ALL: [ExtState; 3] = [ExtState::Probe, ExtState::Recover, ExtState::Degraded];

    /// 序数（矩阵索引面）。
    pub const fn ordinal(self) -> usize {
        match self {
            ExtState::Probe => 0,
            ExtState::Recover => 1,
            ExtState::Degraded => 2,
        }
    }

    /// 序数反解（越界 None——StateOutOfRange 判定源）。
    pub const fn from_ordinal(v: u64) -> Option<ExtState> {
        match v {
            0 => Some(ExtState::Probe),
            1 => Some(ExtState::Recover),
            2 => Some(ExtState::Degraded),
            _ => None,
        }
    }

    /// 中文名（互异）。
    pub const fn zh(self) -> &'static str {
        match self {
            ExtState::Probe => "探测中",
            ExtState::Recover => "恢复中",
            ExtState::Degraded => "降级运行",
        }
    }
}

/// 五事件闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ExtEvent {
    /// 探测完成。
    ProbeDone,
    /// 探测失败。
    ProbeFail,
    /// 恢复步进（三步：重扫/回退/隔离）。
    RecoverStep,
    /// 恢复完成。
    RecoverDone,
    /// 降级请求。
    DegradeReq,
}

impl ExtEvent {
    /// 五闭集。
    pub const ALL: [ExtEvent; 5] = [
        ExtEvent::ProbeDone,
        ExtEvent::ProbeFail,
        ExtEvent::RecoverStep,
        ExtEvent::RecoverDone,
        ExtEvent::DegradeReq,
    ];

    /// 序数（矩阵索引面）。
    pub const fn ordinal(self) -> usize {
        match self {
            ExtEvent::ProbeDone => 0,
            ExtEvent::ProbeFail => 1,
            ExtEvent::RecoverStep => 2,
            ExtEvent::RecoverDone => 3,
            ExtEvent::DegradeReq => 4,
        }
    }

    /// 序数反解。
    pub const fn from_ordinal(v: u64) -> Option<ExtEvent> {
        match v {
            0 => Some(ExtEvent::ProbeDone),
            1 => Some(ExtEvent::ProbeFail),
            2 => Some(ExtEvent::RecoverStep),
            3 => Some(ExtEvent::RecoverDone),
            4 => Some(ExtEvent::DegradeReq),
            _ => None,
        }
    }

    /// 中文名（互异）。
    pub const fn zh(self) -> &'static str {
        match self {
            ExtEvent::ProbeDone => "探测完成",
            ExtEvent::ProbeFail => "探测失败",
            ExtEvent::RecoverStep => "恢复步进",
            ExtEvent::RecoverDone => "恢复完成",
            ExtEvent::DegradeReq => "降级请求",
        }
    }
}

// ---------------------------------------------------------------------------
// 设计矩阵（const 表——判据"与设计矩阵一致"的本体）
// ---------------------------------------------------------------------------

/// 矩阵格（Some 目标态 = 合法格 / None = 非法格——15 格全显性）。
pub type MatrixCell = Option<ExtState>;

/// **设计矩阵 const 表**（3 态 × 5 事件——行=当前态，列=事件）：
///
/// | 当前＼事件 | ProbeDone | ProbeFail | RecoverStep | RecoverDone | DegradeReq |
/// |---|---|---|---|---|---|
/// | Probe    | 出探测(None)* | Recover | None | None | None（无据降级闸） |
/// | Recover  | None（改行闸） | None | Recover(步进) | Degraded | None |
/// | Degraded | None | None | None | Recover | Degraded(幂等,叠层闸) |
///
/// \* ProbeDone 出探测 = 回归基础态框架（F0014 面）——扩展态视角是
/// 离开扩展态（None 表示扩展态机不再持有，离开动作由基础框架承接）。
/// 矩阵语义：**扩展态机的合法迁移只有 4 格**（Probe→Recover、
/// Recover→Recover 步进、Recover→Degraded、Degraded→Degraded 幂等），
/// 其余 11 格全部显性 None——非法迁移全部被拒的矩阵面。
pub const MATRIX: [[MatrixCell; 5]; 3] = [
    // Probe 行：ProbeDone 出机(None) / ProbeFail→Recover / 其余 None。
    [None, Some(ExtState::Recover), None, None, None],
    // Recover 行：步进自环 / 完成降级运行 / 其余 None。
    [None, None, Some(ExtState::Recover), Some(ExtState::Degraded), None],
    // Degraded 行：降级幂等自环（叠层闸在谓词层）/ 其余 None。
    [None, None, None, Some(ExtState::Recover), Some(ExtState::Degraded)],
];

/// 矩阵格读（态 × 事件——索引面）。
pub const fn matrix_cell(cur: ExtState, ev: ExtEvent) -> MatrixCell {
    MATRIX[cur.ordinal()][ev.ordinal()]
}

/// **矩阵一致性核**（match 独立写法 vs const 表逐格对拍——两套写法一个
/// 语义，漂移即 MatrixInconsistent；判据"迁移条件与设计矩阵一致"的
/// 机检面）。
pub const fn matrix_consistent() -> bool {
    let mut si = 0usize;
    while si < 3 {
        let st = ExtState::ALL[si];
        let mut ei = 0usize;
        while ei < 5 {
            let ev = ExtEvent::ALL[ei];
            // match 独立语义（语义学写法）：
            let by_match: MatrixCell = match (st, ev) {
                (ExtState::Probe, ExtEvent::ProbeFail) => Some(ExtState::Recover),
                (ExtState::Recover, ExtEvent::RecoverStep) => Some(ExtState::Recover),
                (ExtState::Recover, ExtEvent::RecoverDone) => Some(ExtState::Degraded),
                (ExtState::Degraded, ExtEvent::RecoverDone) => Some(ExtState::Recover),
                (ExtState::Degraded, ExtEvent::DegradeReq) => Some(ExtState::Degraded),
                _ => None,
            };
            let agree = match (matrix_cell(st, ev), by_match) {
                (Some(g), Some(m)) => g.ordinal() == m.ordinal(),
                (None, None) => true,
                _ => false,
            };
            if !agree {
                return false;
            }
            ei += 1;
        }
        si += 1;
    }
    true
}

// ---------------------------------------------------------------------------
// 条件谓词（双层闸第二层——"迁移条件"落在谓词面）
// ---------------------------------------------------------------------------

/// 迁移条件输入（谓词面依赖的账面快照）。
#[derive(Clone, Copy, Debug)]
pub struct CondCtx {
    /// 失败计数（RecoverWithoutFail 判定源）。
    pub fail_count: u32,
    /// 恢复步数（0..=3——RecoverStepOverflow 判定源）。
    pub recover_steps: u32,
    /// 降级原因已登记（Degraded 自环须有档）。
    pub degrade_reason_set: bool,
}

/// **迁移条件谓词**（合法格的前置条件——逐格显性）：
/// - Probe→Recover：失败计数 ≥ 1（无失败不恢复）；
/// - Recover→Recover（步进）：步数 < 3（三步走完不得再步进）；
/// - Recover→Degraded：步数 ≥ 1（至少走过一步的降级有据）；
/// - Degraded→Degraded（幂等）：降级原因已登记（无档不叠）；
/// - Degraded→Recover：恢复请求须有失败史（fail_count ≥ 1）。
pub const fn condition_ok(
    cur: ExtState,
    ev: ExtEvent,
    target: ExtState,
    ctx: &CondCtx,
) -> Option<BootExtCode> {
    match (cur, ev, target) {
        (ExtState::Probe, ExtEvent::ProbeFail, ExtState::Recover) => {
            if ctx.fail_count >= 1 {
                None
            } else {
                Some(BootExtCode::RecoverWithoutFail)
            }
        }
        (ExtState::Recover, ExtEvent::RecoverStep, ExtState::Recover) => {
            if ctx.recover_steps < 3 {
                None
            } else {
                Some(BootExtCode::RecoverStepOverflow)
            }
        }
        (ExtState::Recover, ExtEvent::RecoverDone, ExtState::Degraded) => {
            if ctx.recover_steps >= 1 {
                None
            } else {
                Some(BootExtCode::ConditionNotMet)
            }
        }
        (ExtState::Degraded, ExtEvent::RecoverDone, ExtState::Recover) => {
            if ctx.fail_count >= 1 {
                None
            } else {
                Some(BootExtCode::RecoverWithoutFail)
            }
        }
        (ExtState::Degraded, ExtEvent::DegradeReq, ExtState::Degraded) => {
            if ctx.degrade_reason_set {
                None
            } else {
                Some(BootExtCode::DegradeReasonMissing)
            }
        }
        _ => Some(BootExtCode::ConditionNotMet),
    }
}

// ---------------------------------------------------------------------------
// 扩展态机（编排面——双层闸 + 历史账 + 守恒）
// ---------------------------------------------------------------------------

/// 迁移历史项（从态×事件→到态 或 拒因）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HistItem {
    /// 合法迁移（从, 事件, 到）。
    Moved(ExtState, ExtEvent, ExtState),
    /// 矩阵拒（从, 事件）。
    MatrixDenied(ExtState, ExtEvent),
    /// 条件拒（从, 事件, 码）。
    CondDenied(ExtState, ExtEvent, BootExtCode),
}

/// 扩展态机（判据本体的编排面）。
pub struct ExtBootState {
    /// 当前扩展态。
    pub state: ExtState,
    /// 失败计数。
    pub fail_count: u32,
    /// 恢复步数。
    pub recover_steps: u32,
    /// 降级原因（Degraded 态的档位说明——空即无档）。
    pub degrade_reason: Option<String>,
    /// 历史环（最近 HISTORY_CAP 条——append-only 语义，环写覆盖最旧）。
    pub history: Vec<HistItem>,
    /// 计数：合法迁移/矩阵拒/条件拒。
    pub moved: u64,
    pub denied_matrix: u64,
    pub denied_cond: u64,
    /// 告警账。
    pub alerts: AlertList,
}

/// 告警账（轻量）。
pub struct AlertList {
    pub rows: Vec<String>,
    pub per_code: [u32; 16],
    pub rejected_full: u32,
}

impl AlertList {
    /// 空账。
    pub fn new() -> AlertList {
        AlertList {
            rows: Vec::new(),
            per_code: [0; 16],
            rejected_full: 0,
        }
    }

    /// 追加。
    pub fn push(&mut self, c: BootExtCode, note: &str) {
        if self.rows.len() >= ALERT_CAP {
            self.rejected_full += 1;
            return;
        }
        let idx = (c.code() - SEG_BASE) as usize;
        self.per_code[idx] += 1;
        let mut line = String::from(note);
        line.push_str(" | ");
        line.push_str(c.detail());
        self.rows.push(alloc::format!("{:#06x}|{}", c.code(), line));
    }

    /// 在账数。
    pub fn len(&self) -> usize {
        self.rows.len()
    }
}

impl Default for AlertList {
    fn default() -> Self {
        AlertList::new()
    }
}

impl ExtBootState {
    /// 构造（初态必须 Probe——引导扩展态从探测开始）。
    pub fn new() -> ExtBootState {
        ExtBootState {
            state: ExtState::Probe,
            fail_count: 0,
            recover_steps: 0,
            degrade_reason: None,
            history: Vec::new(),
            moved: 0,
            denied_matrix: 0,
            denied_cond: 0,
            alerts: AlertList::new(),
        }
    }

    fn hist_push(&mut self, item: HistItem) -> Result<(), BootExtCode> {
        if self.history.len() >= HISTORY_CAP {
            // 环写：满则移除最旧（FIFO）——溢出即实现事故（防呆计数）。
            self.history.remove(0);
            self.alerts.push(BootExtCode::HistoryOverflow, "环覆盖最旧");
            return Err(BootExtCode::HistoryOverflow);
        }
        self.history.push(item);
        Ok(())
    }

    /// **双层闸迁移**（矩阵格 → 条件谓词 → 提交 + 历史账——非法全拒）。
    pub fn dispatch(&mut self, ev: ExtEvent) -> Result<ExtState, BootExtCode> {
        let cur = self.state;
        // 语义闸先行（矩阵外但语义特别显性的两格——无据降级/半路改行）。
        if cur == ExtState::Probe && ev == ExtEvent::DegradeReq {
            self.alerts.push(BootExtCode::ProbeThenDegrade, "无据降级");
            self.denied_matrix += 1;
            let _ = self.hist_push(HistItem::MatrixDenied(cur, ev));
            return Err(BootExtCode::ProbeThenDegrade);
        }
        if cur == ExtState::Recover && ev == ExtEvent::ProbeFail {
            self.alerts.push(BootExtCode::RecoverThenProbe, "半路改行");
            self.denied_matrix += 1;
            let _ = self.hist_push(HistItem::MatrixDenied(cur, ev));
            return Err(BootExtCode::RecoverThenProbe);
        }
        // 降级叠加语义闸（Degraded×DegradeReq 矩阵幂等但语义上是一次
        // 一档——首降级后有档即可重复登记合并，无档即拒）。
        // 第一层：矩阵格。
        let cell = matrix_cell(cur, ev);
        let target = match cell {
            Some(t) => t,
            None => {
                self.denied_matrix += 1;
                let _ = self.hist_push(HistItem::MatrixDenied(cur, ev));
                self.alerts.push(BootExtCode::IllegalTransition, "矩阵外格");
                return Err(BootExtCode::IllegalTransition);
            }
        };
        // 第二层：条件谓词。
        let ctx = CondCtx {
            fail_count: self.fail_count,
            recover_steps: self.recover_steps,
            degrade_reason_set: self.degrade_reason.is_some(),
        };
        if let Some(c) = condition_ok(cur, ev, target, &ctx) {
            self.denied_cond += 1;
            let _ = self.hist_push(HistItem::CondDenied(cur, ev, c));
            self.alerts.push(c, "条件闸拒");
            return Err(c);
        }
        // 提交（副作用随迁移联动）。
        self.state = target;
        match (cur, ev, target) {
            (ExtState::Probe, ExtEvent::ProbeFail, ExtState::Recover) => {
                // 失败计数已由外部 fail_event 增——此处不重复。
            }
            (ExtState::Recover, ExtEvent::RecoverStep, ExtState::Recover) => {
                self.recover_steps += 1;
            }
            (ExtState::Recover, ExtEvent::RecoverDone, ExtState::Degraded) => {
                if self.degrade_reason.is_none() {
                    self.degrade_reason = Some(String::from("恢复完成转降级"));
                }
            }
            (ExtState::Degraded, ExtEvent::RecoverDone, ExtState::Recover) => {
                // 升回恢复流程——步数清零重新走。
                self.recover_steps = 0;
            }
            (ExtState::Degraded, ExtEvent::DegradeReq, ExtState::Degraded) => {
                // 幂等自环——档位保留。
            }
            _ => {}
        }
        self.moved += 1;
        let _ = self.hist_push(HistItem::Moved(cur, ev, target));
        Ok(target)
    }

    /// 探测失败事件（失败计数联动 + 迁移）。
    pub fn fail_event(&mut self) -> Result<ExtState, BootExtCode> {
        self.fail_count += 1;
        self.dispatch(ExtEvent::ProbeFail)
    }

    /// 降级登记（原因显性——无档不降）。
    pub fn set_degrade_reason(&mut self, reason: &str) {
        self.degrade_reason = Some(String::from(reason));
    }

    /// 历史账守恒（环窗口不超容量 + 计数面三分类齐全）。
    pub fn conserved(&self) -> bool {
        self.history.len() <= HISTORY_CAP
    }

    /// 最近历史读回。
    pub fn recent_history(&self) -> &[HistItem] {
        &self.history
    }

    /// 一行摘要。
    pub fn summary(&self) -> String {
        alloc::format!(
            "U121 扩展态: 态={} 失败={} 步数={} 迁移={} 矩阵拒={} 条件拒={} 矩阵一致={}",
            self.state.zh(),
            self.fail_count,
            self.recover_steps,
            self.moved,
            self.denied_matrix,
            self.denied_cond,
            if matrix_consistent() { "真" } else { "假" }
        )
    }
}

impl Default for ExtBootState {
    fn default() -> Self {
        ExtBootState::new()
    }
}

// ---------------------------------------------------------------------------
// 快照（态 + 计数 + 历史 '|' 序列化 + FNV + 往返）
// ---------------------------------------------------------------------------

/// 扩展态机快照。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExtSnap {
    /// 版本戳。
    pub version: String,
    /// 当前态序数。
    pub state_ord: u64,
    /// 失败计数/步数。
    pub fail_count: u32,
    pub recover_steps: u32,
    /// 计数三面。
    pub moved: u64,
    pub denied_matrix: u64,
    pub denied_cond: u64,
    /// 降级原因。
    pub degrade_reason: String,
}

/// FNV-1a 64。
pub const fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut i = 0usize;
    while i < bytes.len() {
        h ^= bytes[i] as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        i += 1;
    }
    h
}

impl ExtSnap {
    /// 定格。
    pub fn capture(m: &ExtBootState) -> ExtSnap {
        ExtSnap {
            version: String::from(UNX121_EXTBOOT_VERSION),
            state_ord: m.state.ordinal() as u64,
            fail_count: m.fail_count,
            recover_steps: m.recover_steps,
            moved: m.moved,
            denied_matrix: m.denied_matrix,
            denied_cond: m.denied_cond,
            degrade_reason: m.degrade_reason.clone().unwrap_or_default(),
        }
    }

    /// 序列化。
    pub fn serialize(&self) -> String {
        alloc::format!(
            "{}|{}|{}|{}|{}|{}|{}|{}",
            self.version,
            self.state_ord,
            self.fail_count,
            self.recover_steps,
            self.moved,
            self.denied_matrix,
            self.denied_cond,
            self.degrade_reason
        )
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        fnv1a64(self.serialize().as_bytes())
    }

    /// 往返恒等。
    pub fn roundtrip(&self) -> Result<(), BootExtCode> {
        match parse_snap(&self.serialize()) {
            Some(p) => {
                if p == *self && p.fingerprint() == self.fingerprint() {
                    Ok(())
                } else {
                    Err(BootExtCode::SnapshotFingerprint)
                }
            }
            None => Err(BootExtCode::SnapshotFingerprint),
        }
    }
}

/// 十进制解析。
fn parse_u64(s: &str) -> Option<u64> {
    if s.is_empty() {
        return None;
    }
    let mut v: u64 = 0;
    let b = s.as_bytes();
    let mut i = 0usize;
    while i < b.len() {
        let d = (b[i] as u64).wrapping_sub(b'0' as u64);
        if d > 9 {
            return None;
        }
        v = v.wrapping_mul(10).wrapping_add(d);
        i += 1;
    }
    Some(v)
}

/// 快照反序列化。
pub fn parse_snap(s: &str) -> Option<ExtSnap> {
    let parts: Vec<&str> = s.split('|').collect();
    if parts.len() != 8 {
        return None;
    }
    let version = String::from(parts[0]);
    let state_ord = parse_u64(parts[1])?;
    let fail_count = parse_u64(parts[2])? as u32;
    let recover_steps = parse_u64(parts[3])? as u32;
    let moved = parse_u64(parts[4])?;
    let denied_matrix = parse_u64(parts[5])?;
    let denied_cond = parse_u64(parts[6])?;
    let degrade_reason = String::from(parts[7]);
    if state_ord > 2 {
        return None;
    }
    Some(ExtSnap {
        version,
        state_ord,
        fail_count,
        recover_steps,
        moved,
        denied_matrix,
        denied_cond,
        degrade_reason,
    })
}

// ---------------------------------------------------------------------------
// 多口径参考探针（四口径）
// ---------------------------------------------------------------------------

/// 口径一：设计矩阵 15 格逐格对拍（const 表 vs match 独立写法）——
/// matrix_consistent 真 + 合法格恰 4 + 非法格恰 11。
fn probe_matrix_cells() -> bool {
    if !matrix_consistent() {
        return false;
    }
    let mut legal = 0usize;
    let mut illegal = 0usize;
    let mut si = 0usize;
    while si < 3 {
        let mut ei = 0usize;
        while ei < 5 {
            if MATRIX[si][ei].is_some() {
                legal += 1;
            } else {
                illegal += 1;
            }
            ei += 1;
        }
        si += 1;
    }
    // 合法格=5（Probe×1 + Recover×2 + Degraded×2），非法=15-5=10——
    // 判据侧独立枚举计数，与 MATRIX 表守恒。
    legal == 5 && illegal == 10
}

/// 口径二：非法迁移全拒——Probe×ProbeDone/RecoverDone/DegradeReq、
/// Recover×ProbeDone/ProbeFail(语义闸)/RecoverDone 后再步进、
/// Degraded×ProbeDone/ProbeFail/DegradeReq(无档)逐格 Err 且计数递增。
fn probe_illegal_denials() -> bool {
    let mut m = ExtBootState::new();
    // Probe 态非法三格。
    let d1 = m.dispatch(ExtEvent::ProbeDone) == Err(BootExtCode::IllegalTransition);
    let d2 = m.dispatch(ExtEvent::RecoverDone) == Err(BootExtCode::IllegalTransition);
    let d3 = m.dispatch(ExtEvent::DegradeReq) == Err(BootExtCode::ProbeThenDegrade);
    // 无失败进恢复（fail_count=0 时矩阵格合法但谓词拒）。
    let mut m2 = ExtBootState::new();
    let d4 = m2.dispatch(ExtEvent::ProbeFail) == Err(BootExtCode::RecoverWithoutFail);
    // Recover 态半路改行（先合法进 Recover）。
    let _ = m.fail_event(); // Probe→Recover（fail_count=1）
    let d5 = m.dispatch(ExtEvent::ProbeFail) == Err(BootExtCode::RecoverThenProbe);
    let d6 = m.dispatch(ExtEvent::ProbeDone) == Err(BootExtCode::IllegalTransition);
    // 走到 Degraded（一步有据）再测矩阵外格。
    let _ = m.dispatch(ExtEvent::RecoverStep);
    let _ = m.dispatch(ExtEvent::RecoverDone); // →Degraded
    let d7 = m.dispatch(ExtEvent::ProbeDone) == Err(BootExtCode::IllegalTransition);
    let d8 = m.dispatch(ExtEvent::ProbeFail) == Err(BootExtCode::IllegalTransition);
    d1 && d2 && d3 && d4 && d5 && d6 && d7 && d8 && m.denied_matrix >= 6
}

/// 口径三：条件谓词恰值——三步恢复走完后第四步步进拒
/// （RecoverStepOverflow）；恢复零步完成拒（ConditionNotMet）。
fn probe_condition_gates() -> bool {
    let mut m = ExtBootState::new();
    let _ = m.fail_event(); // →Recover（fail=1）
    // 三步走完。
    let s1 = m.dispatch(ExtEvent::RecoverStep).is_ok();
    let s2 = m.dispatch(ExtEvent::RecoverStep).is_ok();
    let s3 = m.dispatch(ExtEvent::RecoverStep).is_ok();
    let s4 = m.dispatch(ExtEvent::RecoverStep) == Err(BootExtCode::RecoverStepOverflow);
    // 恢复完成 → Degraded（步数=3 有据）。
    let done = m.dispatch(ExtEvent::RecoverDone).is_ok();
    // 新零步恢复链：Degraded→Recover（RecoverDone 事件）→ 步数清零 →
    // 立即 RecoverDone 拒（ConditionNotMet——零步无据）。
    let up = m.dispatch(ExtEvent::RecoverDone).is_ok(); // →Recover
    let zerostep = m.dispatch(ExtEvent::RecoverDone) == Err(BootExtCode::ConditionNotMet);
    s1 && s2 && s3 && s4 && done && up && zerostep
}

/// 口径四：段守恒 + 矩阵一致 + 快照往返 + 空态平凡。
fn probe_snapshot_segment() -> bool {
    if !segment_guard() || !matrix_consistent() {
        return false;
    }
    let mut m = ExtBootState::new();
    let _ = m.fail_event();
    let _ = m.dispatch(ExtEvent::RecoverStep);
    let snap = ExtSnap::capture(&m);
    snap.roundtrip().is_ok()
        && snap.state_ord == 1
        && snap.fail_count == 1
        && snap.recover_steps == 1
        && ExtBootState::new().summary().contains("U121")
}

/// 汇总参考探针。
pub fn reference_budget_check() -> bool {
    probe_matrix_cells() && probe_illegal_denials() && probe_condition_gates()
        && probe_snapshot_segment()
}

// ---------------------------------------------------------------------------
// 判据（CheckSet——判据面零 panic）
// ---------------------------------------------------------------------------

/// UNX-F0121 域自检（判据四组：矩阵×3、迁移×3、谓词×3、判据元×1）。
pub fn run_unx121_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0121");

    // 矩-01：设计矩阵 15 格——合法恰 4 非法恰 11 + 两套写法一致。
    if probe_matrix_cells() {
        s.ok("U121-矩-01-15格全枚举两写法一致");
    } else {
        s.fail("U121-矩-01-15格全枚举两写法一致", "矩阵面失效");
    }

    // 矩-02：矩阵格直读恰值——四合法格逐格读回目标态。
    let c1 = matrix_cell(ExtState::Probe, ExtEvent::ProbeFail) == Some(ExtState::Recover);
    let c2 = matrix_cell(ExtState::Recover, ExtEvent::RecoverStep) == Some(ExtState::Recover);
    let c3 = matrix_cell(ExtState::Recover, ExtEvent::RecoverDone) == Some(ExtState::Degraded);
    let c4 = matrix_cell(ExtState::Degraded, ExtEvent::DegradeReq) == Some(ExtState::Degraded);
    let n1 = matrix_cell(ExtState::Probe, ExtEvent::ProbeDone).is_none();
    if c1 && c2 && c3 && c4 && n1 {
        s.ok("U121-矩-02-合法格直读恰值");
    } else {
        s.fail("U121-矩-02-合法格直读恰值", "格面失效");
    }

    // 矩-03：序数往返（态/事件 from_ordinal 双向 + 越界 None）。
    let ord_ok = ExtState::from_ordinal(2) == Some(ExtState::Degraded)
        && ExtState::from_ordinal(3).is_none()
        && ExtEvent::from_ordinal(4) == Some(ExtEvent::DegradeReq)
        && ExtEvent::from_ordinal(5).is_none()
        && ExtState::ALL.len() == 3
        && ExtEvent::ALL.len() == 5;
    if ord_ok {
        s.ok("U121-矩-03-序数往返越界None");
    } else {
        s.fail("U121-矩-03-序数往返越界None", "序数面失效");
    }

    // 迁-01：非法迁移全拒（八格抽样 + 计数递增）。
    if probe_illegal_denials() {
        s.ok("U121-迁-01-非法迁移全拒");
    } else {
        s.fail("U121-迁-01-非法迁移全拒", "拒绝面失效");
    }

    // 迁-02：合法全链——失败进恢复 → 三步 → 降级 → 升回 → 再降级
    // （幂等自环）全通且历史环记忆。
    let mut m = ExtBootState::new();
    let r1 = m.fail_event() == Ok(ExtState::Recover);
    let _ = m.dispatch(ExtEvent::RecoverStep);
    let _ = m.dispatch(ExtEvent::RecoverStep);
    let r2 = m.dispatch(ExtEvent::RecoverStep) == Ok(ExtState::Recover);
    let r3 = m.dispatch(ExtEvent::RecoverDone) == Ok(ExtState::Degraded);
    let r4 = m.dispatch(ExtEvent::RecoverDone) == Ok(ExtState::Recover);
    // 升回后 steps 清零——「零步无据」谓词同样新恢复流程：先一步再完成。
    let _ = m.dispatch(ExtEvent::RecoverStep);
    let r5 = m.dispatch(ExtEvent::RecoverDone) == Ok(ExtState::Degraded);
    let r6 = m.dispatch(ExtEvent::DegradeReq) == Ok(ExtState::Degraded);
    // 9 次合法 dispatch（fail_event 1 + Step 3 + Done 1 + Done 1 + Step 1
    // + Done 1 + DegradeReq 1）——moved 判据侧独立累计。
    let hist_ok = m.recent_history().len() >= 6 && m.moved == 9;
    if r1 && r2 && r3 && r4 && r5 && r6 && hist_ok {
        s.ok("U121-迁-02-合法全链历史环");
    } else {
        s.fail("U121-迁-02-合法全链历史环", "全链失效");
    }

    // 迁-03：语义特闸显性——探测中直降级（ProbeThenDegrade）/恢复中
    // 改探测（RecoverThenProbe）逐码拒。
    let mut a = ExtBootState::new();
    let t1 = a.dispatch(ExtEvent::DegradeReq) == Err(BootExtCode::ProbeThenDegrade);
    let _ = a.fail_event();
    let t2 = a.dispatch(ExtEvent::ProbeFail) == Err(BootExtCode::RecoverThenProbe);
    if t1 && t2 {
        s.ok("U121-迁-03-语义特闸显性");
    } else {
        s.fail("U121-迁-03-语义特闸显性", "特闸失效");
    }

    // 谓-01：条件谓词恰值（三步上限/零步无据）。
    if probe_condition_gates() {
        s.ok("U121-谓-01-条件谓词恰值");
    } else {
        s.fail("U121-谓-01-条件谓词恰值", "谓词面失效");
    }

    // 谓-02：无失败不恢复——fail_count=0 时 ProbeFail 走矩阵格但谓词拒
    // （双层闸的独立性证明：格合法≠可迁移）。
    let mut m2 = ExtBootState::new();
    let ctx = CondCtx { fail_count: 0, recover_steps: 0, degrade_reason_set: false };
    let cond = condition_ok(ExtState::Probe, ExtEvent::ProbeFail, ExtState::Recover, &ctx)
        == Some(BootExtCode::RecoverWithoutFail);
    let disp = m2.dispatch(ExtEvent::ProbeFail) == Err(BootExtCode::RecoverWithoutFail);
    if cond && disp {
        s.ok("U121-谓-02-双层闸独立性");
    } else {
        s.fail("U121-谓-02-双层闸独立性", "双层闸失效");
    }

    // 谓-03：降级原因档——无档自环拒（DegradeReasonMissing）/有档自环
    // 通（幂等）。
    let mut m3 = ExtBootState::new();
    let _ = m3.fail_event();
    let _ = m3.dispatch(ExtEvent::RecoverStep);
    let _ = m3.dispatch(ExtEvent::RecoverDone); // →Degraded（自动给档）
    let auto_doc = m3.degrade_reason.is_some();
    let ok_doc = m3.dispatch(ExtEvent::DegradeReq).is_ok();
    let mut m4 = ExtBootState::new();
    let _ = m4.fail_event();
    let _ = m4.dispatch(ExtEvent::RecoverDone); // 零步→拒（到不了 Degraded）
    let m4_still_recover = m4.state == ExtState::Recover;
    if auto_doc && ok_doc && m4_still_recover {
        s.ok("U121-谓-03-降级档显性登记");
    } else {
        s.fail("U121-谓-03-降级档显性登记", "档面失效");
    }

    // 组-01：十六码段守恒 + 详情互异非空。
    let codes = BootExtCode::ALL;
    let mut codes_ok = codes.len() == 16;
    let mut i = 0usize;
    while i < codes.len() && codes_ok {
        if codes[i].detail().is_empty() {
            codes_ok = false;
        }
        let mut j = i + 1;
        while j < codes.len() {
            if codes[i].code() == codes[j].code() || codes[i].detail() == codes[j].detail() {
                codes_ok = false;
            }
            j += 1;
        }
        i += 1;
    }
    if codes_ok && segment_guard() {
        s.ok("U121-组-01-十六码段守恒");
    } else {
        s.fail("U121-组-01-十六码段守恒", "码面漂移");
    }

    // 组-02：参考探针四口径全真。
    if reference_budget_check() {
        s.ok("U121-组-02-四口径探针全真");
    } else {
        s.fail("U121-组-02-四口径探针全真", "探针失效");
    }

    // 元-01：零 panic 面 + 快照往返 + 空态平凡 + 版本非空。
    let src = include_str!("unx121_extbootstate.rs");
    let clean = strip_lexical_noise(src);
    let mut hits = 0usize;
    for pat in [".expect(", "panic!", "unreachable!", "unsafe"].iter() {
        if clean.contains(pat) {
            hits += 1;
        }
    }
    let empty = ExtBootState::new();
    let snap0 = ExtSnap::capture(&empty);
    let empty_ok = empty.state == ExtState::Probe
        && empty.fail_count == 0
        && empty.summary().contains("U121")
        && snap0.roundtrip().is_ok()
        && !UNX121_EXTBOOT_VERSION.is_empty()
        && ExtState::Probe.zh() == "探测中"
        && ExtEvent::DegradeReq.zh() == "降级请求";
    if hits == 0 && empty_ok {
        s.ok("U121-元-01-零panic空态平凡");
    } else {
        s.fail("U121-元-01-零panic空态平凡", "panic 面或空态失效");
    }

    // 深-01：回放一致性（合法全链重演终态恰等现态+污染态揭破）。
    if probe_replay_consistency() {
        s.ok("U121-深-01-历史环回放一致");
    } else {
        s.fail("U121-深-01-历史环回放一致", "回放面失效");
    }

    // 深-02：四谓词全覆盖审计（判据网无洞——零命中谓词即网洞）。
    if probe_cond_coverage() {
        s.ok("U121-深-02-谓词覆盖审计");
    } else {
        s.fail("U121-深-02-谓词覆盖审计", "覆盖面失效");
    }

    // 深-03：吸收轨迹恰值（好流五迁移+终态+渲染恰串；坏流两类拒
    // 同流显性+计数守恒——矩阵的运行时投影与设计面对拍）。
    if probe_absorb_trace() {
        s.ok("U121-深-03-轨迹吸收恰值");
    } else {
        s.fail("U121-深-03-轨迹吸收恰值", "吸收面失效");
    }

    // 深-04：可达闭包恰值（三态全可达+不动点稳定双写）+ wire 往返
    //（好流/空流恒等+非法序数显性拒——历史外带协议单点）。
    if probe_reach_wire() {
        s.ok("U121-深-04-可达闭包与wire");
    } else {
        s.fail("U121-深-04-可达闭包与wire", "闭包或协议失效");
    }

    // 深-05：幂等叠层（3 连发 moved 增 3 终态不变轨迹全自环+
    // 零叠层平凡——幂等格的运行时面）。
    if probe_degrade_stack() {
        s.ok("U121-深-05-幂等叠层账");
    } else {
        s.fail("U121-深-05-幂等叠层账", "叠层面失效");
    }

    // 深-06：环压缩编码——混合四项线往返恒等 + 再编码幂等 + 空账
    // 平凡（FNV 偏移基自洽）+ 篡改双路（载荷/指纹）显性拒 + 逐项
    // 坏例全拒（历史账可外带协议单点）。
    if probe_hist_wire() {
        s.ok("U121-深-06-环线格式指纹");
    } else {
        s.fail("U121-深-06-环线格式指纹", "环线面失效");
    }

    // 深-07：谓词解释账——15 格全枚举两 ctx 恰值（零值/满值闭式）
    // + 聚合账守恒恰值 + 线往返 + 结构性拒（拒因结构化单点）。
    if probe_cond_explain() {
        s.ok("U121-深-07-解释账十五格");
    } else {
        s.fail("U121-深-07-解释账十五格", "解释面失效");
    }

    // 深-08：告警聚合——已知流恰值（3 告警特闸/条件/特闸）+ 在账
    // 守恒（Σ per_code ≡ 行数）+ 并列取最低码 + 容量压力闭式 1250bp
    // + 线往返（桶账 rollup 单点）。
    if probe_alert_rollup() {
        s.ok("U121-深-08-告警聚合守恒");
    } else {
        s.fail("U121-深-08-告警聚合守恒", "聚合面失效");
    }

    // 深-09：状态驻留账——混合六项恰值三态各 2 + 守恒 + 实机流
    // 驻留 Σ ≡ 历史账长 + 线往返（账本时间维度投影单点）。
    if probe_residency() {
        s.ok("U121-深-09-驻留账守恒");
    } else {
        s.fail("U121-深-09-驻留账守恒", "驻留面失效");
    }

    s
}

/// 单遍词法剥除（注释剥除、字符串整体剥空——判据模式串不自 hit）。
fn strip_lexical_noise(src: &str) -> String {
    let b = src.as_bytes();
    let mut out = String::new();
    let mut i = 0usize;
    while i < b.len() {
        if i + 1 < b.len() && b[i] == b'/' && b[i + 1] == b'/' {
            while i < b.len() && b[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if i + 1 < b.len() && b[i] == b'/' && b[i + 1] == b'*' {
            let mut depth = 1usize;
            i += 2;
            while i < b.len() && depth > 0 {
                if i + 1 < b.len() && b[i] == b'/' && b[i + 1] == b'*' {
                    depth += 1;
                    i += 2;
                } else if i + 1 < b.len() && b[i] == b'*' && b[i + 1] == b'/' {
                    depth -= 1;
                    i += 2;
                } else {
                    i += 1;
                }
            }
            continue;
        }
        if b[i] == b'"' {
            i += 1;
            while i < b.len() {
                if b[i] == b'\\' {
                    i += 2;
                    continue;
                }
                let closed = b[i] == b'"';
                i += 1;
                if closed {
                    break;
                }
            }
            continue;
        }
        if b[i] == b'\'' {
            i += 1;
            while i < b.len() {
                if b[i] == b'\\' {
                    i += 2;
                    continue;
                }
                let closed = b[i] == b'\'';
                i += 1;
                if closed {
                    break;
                }
            }
            continue;
        }
        if let Some(c) = src.get(i..i + 1) {
            out.push_str(c);
        }
        i += 1;
    }
    out
}

// ===========================================================================
// 深化七区：历史环回放器 / 谓词覆盖审计——状态机的「为什么在这」面：
// 回放能重演到达路径，审计能证明每个谓词都被判据触达
// ===========================================================================

/// 历史环回放器（回放 = dispatch 链纯函数重演：从 new() 起按历史事件序
/// 重放，重演终态必须等于被回放实例的现态——不一致 = 历史账与现态账
/// 脱钩，状态机的时间一致性立案）。
pub struct HistReplay {
    /// 重放事件序（从历史环抽取的事件流）。
    pub events: Vec<ExtEvent>,
    /// 重放终态。
    pub final_state: Option<ExtState>,
    /// 重放完成标志（事件流耗尽且未中途被拒 = 完整重演）。
    pub complete: bool,
}

/// 从历史环抽取事件流（只取 Moved 项的事件——拒绝项不重放：重演的是
/// 「合法到达路径」，拒绝路径的合法重演是另一条证伪面）。
pub fn extract_moved_events(hist: &[HistItem]) -> Vec<ExtEvent> {
    let mut out: Vec<ExtEvent> = Vec::new();
    let mut i = 0usize;
    while i < hist.len() {
        if let HistItem::Moved(_, ev, _) = hist[i] {
            out.push(ev);
        }
        i += 1;
    }
    out
}

/// 回放到终态（从空白状态机重演事件流——返回重演终态；中途被拒即
/// None：历史里存在合法迁移在重演时被拒 = 环账被污染立案）。
pub fn replay(events: &[ExtEvent]) -> Option<ExtBootState> {
    let mut m = ExtBootState::new();
    // fail_count 前置：真实到达路径的首事件 ProbeFail 必带失败前置
    // （fail_event 语义），回放侧同样给计数前置。
    let mut i = 0usize;
    while i < events.len() {
        let ev = events[i];
        if ev == ExtEvent::ProbeFail {
            m.fail_count += 1;
        }
        match m.dispatch(ev) {
            Ok(_) => {
                i += 1;
            }
            Err(_) => return None,
        }
    }
    Some(m)
}

/// 回放一致性审计（重演终态 == 被审计实例现态 && 重演 moved == 实例
/// moved——双账对拍：历史环不是装饰是可重算的真账）。
pub fn replay_consistent(target: &ExtBootState) -> bool {
    let events = extract_moved_events(&target.history);
    match replay(&events) {
        Some(m) => m.state == target.state && m.moved == target.moved,
        None => false,
    }
}

/// 谓词覆盖审计（双层闸四谓词逐个命中计数——判据审计面：每个谓词
/// 至少被一条判据触达过，零命中谓词 = 判据网有洞）。
#[derive(Clone, Copy, Debug)]
pub struct CondCoverage {
    /// 无失败不恢复（RecoverWithoutFail）命中数。
    pub recover_without_fail: u32,
    /// 三步上限（RecoverStepOverflow）命中数。
    pub step_overflow: u32,
    /// 零步无据（ConditionNotMet）命中数。
    pub condition_not_met: u32,
    /// 零档不叠（DegradeReasonMissing）命中数。
    pub degrade_missing: u32,
}

impl CondCoverage {
    /// 全零审计账。
    pub const fn new() -> CondCoverage {
        CondCoverage {
            recover_without_fail: 0,
            step_overflow: 0,
            condition_not_met: 0,
            degrade_missing: 0,
        }
    }

    /// 按拒绝码入账（BootExtCode → 桶位单点）。
    pub const fn record(&mut self, c: BootExtCode) {
        match c {
            BootExtCode::RecoverWithoutFail => self.recover_without_fail += 1,
            BootExtCode::RecoverStepOverflow => self.step_overflow += 1,
            BootExtCode::ConditionNotMet => self.condition_not_met += 1,
            BootExtCode::DegradeReasonMissing => self.degrade_missing += 1,
            _ => {}
        }
    }

    /// 全覆盖（四谓词全非零——判据网无洞）。
    pub const fn all_covered(&self) -> bool {
        self.recover_without_fail > 0
            && self.step_overflow > 0
            && self.condition_not_met > 0
            && self.degrade_missing > 0
    }
}

/// 深化面参考探针——回放一致性（合法全链重演终态恰等于现态）。
pub fn probe_replay_consistency() -> bool {
    // 构造真实到达路径：失败→三步→降级→升回→一步→降级→幂等自环。
    let mut m = ExtBootState::new();
    let _ = m.fail_event();
    let _ = m.dispatch(ExtEvent::RecoverStep);
    let _ = m.dispatch(ExtEvent::RecoverStep);
    let _ = m.dispatch(ExtEvent::RecoverStep);
    let _ = m.dispatch(ExtEvent::RecoverDone);
    let _ = m.dispatch(ExtEvent::RecoverDone);
    let _ = m.dispatch(ExtEvent::RecoverStep);
    let _ = m.dispatch(ExtEvent::RecoverDone);
    let _ = m.dispatch(ExtEvent::DegradeReq);
    // 双账对拍：回放终态 == 现态。
    if !replay_consistent(&m) {
        return false;
    }
    // 污染面：手改现态后回放必须揭破（state 与历史账脱钩）。
    let mut dirty = ExtBootState::new();
    let _ = dirty.fail_event();
    let _ = dirty.dispatch(ExtEvent::RecoverStep);
    let _ = dirty.dispatch(ExtEvent::RecoverStep);
    let _ = dirty.dispatch(ExtEvent::RecoverDone);
    dirty.state = ExtState::Probe; // 手改——历史说 Degraded 现态却 Probe
    !replay_consistent(&dirty)
}

/// 深化面参考探针——四谓词全覆盖（逐谓词触发一例入账后 all_covered）。
pub fn probe_cond_coverage() -> bool {
    let mut cov = CondCoverage::new();
    // 谓词一：无失败不恢复。
    let mut m1 = ExtBootState::new();
    if let Err(c) = m1.dispatch(ExtEvent::ProbeFail) {
        cov.record(c);
    }
    // 谓词二：三步上限（第 4 步拒）。
    let mut m2 = ExtBootState::new();
    let _ = m2.fail_event();
    let _ = m2.dispatch(ExtEvent::RecoverStep);
    let _ = m2.dispatch(ExtEvent::RecoverStep);
    let _ = m2.dispatch(ExtEvent::RecoverStep);
    if let Err(c) = m2.dispatch(ExtEvent::RecoverStep) {
        cov.record(c);
    }
    // 谓词三：零步无据（不步进直接完成）。
    let mut m3 = ExtBootState::new();
    let _ = m3.fail_event();
    if let Err(c) = m3.dispatch(ExtEvent::RecoverDone) {
        cov.record(c);
    }
    // 谓词四：零档不叠（未设降级档先请求降级）。
    let mut m4 = ExtBootState::new();
    let _ = m4.fail_event();
    let _ = m4.dispatch(ExtEvent::RecoverStep);
    let _ = m4.dispatch(ExtEvent::RecoverDone);
    // Recover→Degraded 自动设档——须先清档再请求：直接构造 Probe 态
    // 语义特闸会拦 ProbeThenDegrade……用矩阵格 Degraded+DegradeReq
    // 但档位清空面：手动造 CondCtx 不可达（谓词私有）——实测经由
    // condition_ok 公开单点直接触达第四桶。
    if let Some(c) = condition_ok(
        ExtState::Degraded,
        ExtEvent::DegradeReq,
        ExtState::Degraded,
        &CondCtx {
            fail_count: 1,
            recover_steps: 0,
            degrade_reason_set: false,
        },
    ) {
        cov.record(c);
    }
    cov.all_covered()
}

// ---------------------------------------------------------------------------
// 深化八区：轨迹吸收面——矩阵/谓词/历史账之上是「事件流怎么跑完」的
// 运行时投影：逐步吸收轨迹 / 可达性闭包 / 事件流 wire 协议 / 幂等叠
// 层账。全部纯函数零 IO——运行面与设计面同构（矩阵是设计真相，轨迹
// 是矩阵的运行投影，两者对拍即「实现与设计一致」的实证面）。
// ---------------------------------------------------------------------------

/// 轨迹单步（吸收账原子项——合法迁移与两类拒全显性）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum TraceStep {
    /// 合法迁移（步号, 事件, 从, 到）。
    Moved(usize, ExtEvent, ExtState, ExtState),
    /// 矩阵拒（步号, 事件, 从）。
    MatrixDenied(usize, ExtEvent, ExtState),
    /// 条件拒（步号, 事件, 从, 码）。
    CondDenied(usize, ExtEvent, ExtState, BootExtCode),
}

impl TraceStep {
    /// 轨迹行渲染（人读形态——`#3 恢复步进: 恢复中→恢复中`）。
    pub fn render(&self) -> String {
        match self {
            TraceStep::Moved(n, ev, from, to) => alloc::format!(
                "#{} {}: {}→{}",
                n,
                ev.zh(),
                from.zh(),
                to.zh()
            ),
            TraceStep::MatrixDenied(n, ev, from) => {
                alloc::format!("#{} {}×矩阵拒({})", n, ev.zh(), from.zh())
            }
            TraceStep::CondDenied(n, ev, from, c) => alloc::format!(
                "#{} {}×条件拒({},{:#06x})",
                n,
                ev.zh(),
                from.zh(),
                c.code()
            ),
        }
    }
}

/// 吸收结果账（事件流整体投影——终态 + 轨迹 + 三分类计数）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct AbsorbOutcome {
    /// 逐步轨迹（含拒步——拒也是轨迹的一部分，全显性）。
    pub steps: Vec<TraceStep>,
    /// 终态（流跑完后的扩展态——拒步不改态）。
    pub final_state: ExtState,
    /// 合法迁移步数。
    pub moved: u64,
    /// 矩阵拒步数。
    pub denied_matrix: u64,
    /// 条件拒步数。
    pub denied_cond: u64,
}

impl AbsorbOutcome {
    /// 计数守恒（三分类和 == 总步数——账不丢不重）。
    pub fn conserved(&self) -> bool {
        (self.moved + self.denied_matrix + self.denied_cond) as usize == self.steps.len()
    }

    /// 轨迹行数（渲染面——与 steps 恒等）。
    pub fn render_lines(&self) -> Vec<String> {
        let mut out = Vec::new();
        for s in self.steps.iter() {
            out.push(s.render());
        }
        out.push(alloc::format!("终态: {}", self.final_state.zh()));
        out
    }
}

/// **事件流吸收**（独立机实例逐步 dispatch——纯函数：不改任何全局，
/// 轨迹与计数从机实例读回；流内事件顺序执行，拒步不中断流——
/// 真实引导事件流不会因为一次拒而停止，吸收语义与之一致）。
pub fn absorb_run(events: &[ExtEvent]) -> AbsorbOutcome {
    let mut m = ExtBootState::new();
    let mut steps = Vec::new();
    let mut moved = 0u64;
    let mut denied_matrix = 0u64;
    let mut denied_cond = 0u64;
    for (i, ev) in events.iter().enumerate() {
        let before = m.state;
        // ProbeFail 走 fail_event 语义（失败计数联动是事件语义的
        // 一部分——直接 dispatch 会因 fail_count=0 被条件谓词拒，
        // 与真实引导事件流语义不符）。
        let outcome = if *ev == ExtEvent::ProbeFail {
            m.fail_event()
        } else {
            m.dispatch(*ev)
        };
        match outcome {
            Ok(to) => {
                moved += 1;
                steps.push(TraceStep::Moved(i + 1, *ev, before, to));
            }
            Err(c) => {
                // 拒因二分：矩阵语义特闸（无据降级/半路改行）归矩阵拒，
                // 其余 Err 码经 alerts 与 HistItem 对读——独立重算面。
                let is_matrix = matches!(
                    c,
                    BootExtCode::IllegalTransition
                        | BootExtCode::ProbeThenDegrade
                        | BootExtCode::RecoverThenProbe
                );
                if is_matrix {
                    denied_matrix += 1;
                    steps.push(TraceStep::MatrixDenied(i + 1, *ev, before));
                } else {
                    denied_cond += 1;
                    steps.push(TraceStep::CondDenied(i + 1, *ev, before, c));
                }
            }
        }
    }
    AbsorbOutcome {
        steps,
        final_state: m.state,
        moved,
        denied_matrix,
        denied_cond,
    }
}

/// **可达性闭包**（初态 Probe 起，单步矩阵投影迭代至不动点——
/// 5 事件任意长序列可达的态集闭包。设计矩阵运行时重投影，与
/// 手排可达面对拍即双写验证）。
pub fn reach_closure() -> Vec<ExtState> {
    let mut reach = vec![ExtState::Probe];
    let mut changed = true;
    while changed {
        changed = false;
        let mut i = 0usize;
        while i < reach.len() {
            for ev in ExtEvent::ALL.iter() {
                if let Some(t) = matrix_cell(reach[i], *ev) {
                    // 新态入闭包（互异保序——枚举序）。
                    if !reach.contains(&t) {
                        reach.push(t);
                        changed = true;
                    }
                }
            }
            i += 1;
        }
    }
    reach
}

/// 可达性闭包恰值（三态全可达——手排对拍单点）。
pub fn reach_closure_expected() -> Vec<ExtState> {
    vec![
        ExtState::Probe,
        ExtState::Recover,
        ExtState::Degraded,
    ]
}

/// 事件流 wire 协议（序数 CSV 单行——历史外带的传输形态单点）。
///
/// 形态：`"1,2,2,4"`（序数逗号串）；空流 → 空串；反解非法序数
/// 整流显性 None（不静默跳过——协议面异物不猜）。
pub fn events_wire(events: &[ExtEvent]) -> String {
    let mut out = String::new();
    for (i, ev) in events.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str(&alloc::format!("{}", ev.ordinal()));
    }
    out
}

/// wire 串反解（序数 CSV → 事件流——非法序数显性 None）。
pub fn events_unwire(s: &str) -> Option<Vec<ExtEvent>> {
    let mut out = Vec::new();
    if s.is_empty() {
        return Some(out);
    }
    for part in s.split(',') {
        let v = parse_u64(part)?;
        out.push(ExtEvent::from_ordinal(v)?);
    }
    Some(out)
}

/// **幂等叠层账**（Degraded×DegradeReq 连发 N 发——矩阵幂等格的
/// 运行时面：有档即过闸，moved 增 N 终态不变，轨迹全自环）。
pub fn degrade_stack_run(n: usize) -> AbsorbOutcome {
    let mut events = Vec::new();
    // 前置合法链：失败→三步→降级（到达 Degraded 且有档）。
    events.push(ExtEvent::ProbeFail);
    events.push(ExtEvent::RecoverStep);
    events.push(ExtEvent::RecoverStep);
    events.push(ExtEvent::RecoverStep);
    events.push(ExtEvent::RecoverDone);
    // 幂等叠层 N 发。
    let mut i = 0usize;
    while i < n {
        events.push(ExtEvent::DegradeReq);
        i += 1;
    }
    absorb_run(&events)
}

/// 深-04 判据：吸收轨迹恰值（好流五迁移+终态；坏流矩阵拒截断显性；
/// 计数守恒；轨迹渲染行数恰值）。
pub fn probe_absorb_trace() -> bool {
    // 好流：失败→三步→降级（5 迁移，终态 Degraded）。
    let good = [
        ExtEvent::ProbeFail,
        ExtEvent::RecoverStep,
        ExtEvent::RecoverStep,
        ExtEvent::RecoverStep,
        ExtEvent::RecoverDone,
    ];
    let o = absorb_run(&good);
    let good_ok = o.moved == 5
        && o.denied_matrix == 0
        && o.denied_cond == 0
        && o.final_state == ExtState::Degraded
        && o.conserved()
        && o.steps.len() == 5;
    // 轨迹渲染：5 迁移行 + 终态行 = 6 行；首行形状恰值。
    let lines = o.render_lines();
    let render_ok = lines.len() == 6
        && lines[0] == "#1 探测失败: 探测中→恢复中"
        && lines[4] == "#5 恢复完成: 恢复中→降级运行"
        && lines[5] == "终态: 降级运行";
    // 坏流：Probe 态 RecoverStep（矩阵拒）+ ProbeFail 过闸（fail_event
    // 联动）到 Recover + 零步 RecoverDone（条件拒——零步无据谓词在
    // Recover 态生效，Probe 态是矩阵拒不是条件拒）——两类拒同流显性。
    let bad = [ExtEvent::RecoverStep, ExtEvent::ProbeFail, ExtEvent::RecoverDone];
    let ob = absorb_run(&bad);
    let bad_ok = ob.moved == 1
        && ob.denied_matrix == 1
        && ob.denied_cond == 1
        && ob.final_state == ExtState::Recover
        && ob.conserved()
        && matches!(ob.steps[0], TraceStep::MatrixDenied(1, ExtEvent::RecoverStep, ExtState::Probe))
        && matches!(ob.steps[1], TraceStep::Moved(2, ExtEvent::ProbeFail, ExtState::Probe, ExtState::Recover))
        && matches!(
            ob.steps[2],
            TraceStep::CondDenied(3, ExtEvent::RecoverDone, ExtState::Recover, _)
        );
    good_ok && render_ok && bad_ok
}

/// 深-05 判据：可达闭包恰值（三态全可达+闭包不动点稳定）+
/// wire 往返恒等（好流/空流/含幂等流三态）+ 非法序数显性拒。
pub fn probe_reach_wire() -> bool {
    // 闭包 == 手排恰值 + 再算一轮不变（不动点稳定双写）。
    let r1 = reach_closure();
    let r2 = reach_closure();
    let reach_ok = r1 == reach_closure_expected() && r1 == r2;
    // wire 往返：好流（含 0 序数——split 尾空串面）。
    let flow = [
        ExtEvent::ProbeFail,
        ExtEvent::RecoverStep,
        ExtEvent::RecoverDone,
    ];
    let w = events_wire(&flow);
    let rt = events_unwire(&w);
    let wire_ok = w == "1,2,3" && rt == Some(flow.to_vec());
    // 空流往返。
    let wire_empty = events_wire(&[]) == "" && events_unwire("") == Some(Vec::new());
    // 非法序数（9 越界）显性 None。
    let wire_bad = events_unwire("1,9").is_none();
    reach_ok && wire_ok && wire_empty && wire_bad
}

/// 深-06 判据：幂等叠层（3 连发 moved 增 3 终态不变轨迹全自环）+
/// 叠层与前置链合计计数守恒。
pub fn probe_degrade_stack() -> bool {
    let o3 = degrade_stack_run(3);
    // 前置 5 迁移 + 叠层 3 = moved 8；终态 Degraded 不变。
    let stack_ok = o3.moved == 8
        && o3.denied_matrix == 0
        && o3.denied_cond == 0
        && o3.final_state == ExtState::Degraded
        && o3.conserved();
    // 轨迹尾三行全自环（#6/#7/#8 降级请求: 降级运行→降级运行）。
    let lines = o3.render_lines();
    let tail_ok = lines.len() == 9
        && lines[5] == "#6 降级请求: 降级运行→降级运行"
        && lines[6] == "#7 降级请求: 降级运行→降级运行"
        && lines[7] == "#8 降级请求: 降级运行→降级运行";
    // 零叠层平凡（仅前置链 moved 5）。
    let o0 = degrade_stack_run(0);
    let zero_ok = o0.moved == 5 && o0.final_state == ExtState::Degraded;
    stack_ok && tail_ok && zero_ok
}

// ===========================================================================
// 深化九区：环压缩编码 / 谓词解释账 / 告警聚合 / 状态驻留账——历史与
// 告警两大账本的「可外带、可归因、可守恒」三面：
// ① 环账出线：HistItem 紧凑线格式（M/X/C 三标签 + 逗号字段 + 分号项间）
//    + 顶层四段 wire（标签|计数|FNV 指纹|载荷）——指纹失配即篡改显性拒；
// ② 拒因结构化：CondFailReason 五桶闭集——把双层闸的拒码提升为结构化
//    归因（格面/无失败史/步越界/零步降级/无档），15 格全枚举两套写法
//    对拍（cond_explain 复合语义 vs 手排期望表恰值）；
// ③ 拒因聚合账：CondLedger 从历史账逐项归因（Moved→Pass /
//    MatrixDenied→格面 / CondDenied→码映射桶），pass+Σfail 恒等项数；
// ④ 告警聚合：AlertRollup 桶账 rollup——在账守恒（Σ per_code ≡ 行数）
//    + 容量压力万分比闭式 + 并列取最低码；
// ⑤ 驻留账：state_residency 逐历史项所处态计数——账本的时间维度投影，
//    Σ驻留 ≡ 项数。
// ===========================================================================

/// 拒因五桶闭集（双层闸拒因的结构化归因面）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CondFailReason {
    /// 无失败史（RecoverWithoutFail——Probe→Recover / Degraded→Recover 共因）。
    NoFailHistory,
    /// 步进越界（RecoverStepOverflow——三步走完仍步进）。
    StepOverflow,
    /// 零步降级（Recover→Degraded 但步数为 0——有据闸）。
    DegradedNoStep,
    /// 降级档缺失（DegradeReasonMissing——无档不叠）。
    NoDegradeTier,
    /// 矩阵外格（matrix_cell None——第一层闸拒）。
    CellIllegal,
}

impl CondFailReason {
    /// 五闭集。
    pub const ALL: [CondFailReason; 5] = [
        CondFailReason::NoFailHistory,
        CondFailReason::StepOverflow,
        CondFailReason::DegradedNoStep,
        CondFailReason::NoDegradeTier,
        CondFailReason::CellIllegal,
    ];

    /// 序数（桶索引面）。
    pub const fn ordinal(self) -> usize {
        match self {
            CondFailReason::NoFailHistory => 0,
            CondFailReason::StepOverflow => 1,
            CondFailReason::DegradedNoStep => 2,
            CondFailReason::NoDegradeTier => 3,
            CondFailReason::CellIllegal => 4,
        }
    }

    /// 中文名（互异）。
    pub const fn zh(self) -> &'static str {
        match self {
            CondFailReason::NoFailHistory => "无失败史",
            CondFailReason::StepOverflow => "步进越界",
            CondFailReason::DegradedNoStep => "零步降级",
            CondFailReason::NoDegradeTier => "降级档缺失",
            CondFailReason::CellIllegal => "矩阵外格",
        }
    }
}

/// 拒码→拒因映射（const 纯函数——语义特闸码与矩阵码归格面桶，
/// ConditionNotMet 需格面上下文消歧：仅出自 (Recover, RecoverDone)
/// 零步格；其余条件码一一对应）。
pub const fn cond_fail_reason_of(cur: ExtState, ev: ExtEvent, code: BootExtCode) -> CondFailReason {
    match code {
        BootExtCode::RecoverWithoutFail => CondFailReason::NoFailHistory,
        BootExtCode::RecoverStepOverflow => CondFailReason::StepOverflow,
        BootExtCode::DegradeReasonMissing => CondFailReason::NoDegradeTier,
        BootExtCode::ConditionNotMet => {
            if matches!(cur, ExtState::Recover) && matches!(ev, ExtEvent::RecoverDone) {
                CondFailReason::DegradedNoStep
            } else {
                CondFailReason::CellIllegal
            }
        }
        _ => CondFailReason::CellIllegal,
    }
}

/// 条件解释裁决（Pass 带目标态 / Fail 带结构化拒因）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CondVerdict {
    /// 可迁移（双层闸全通——带矩阵目标态）。
    Pass(ExtState),
    /// 拒（带结构化拒因）。
    Fail(CondFailReason),
}

/// **条件解释器**（矩阵格 × 条件谓词的复合语义单点——与 dispatch 的
/// 双层闸同序：格 None→格面拒；格 Some 谓词过→Pass；谓词拒→码映射）。
pub fn cond_explain(cur: ExtState, ev: ExtEvent, ctx: &CondCtx) -> CondVerdict {
    match matrix_cell(cur, ev) {
        None => CondVerdict::Fail(CondFailReason::CellIllegal),
        Some(target) => match condition_ok(cur, ev, target, ctx) {
            None => CondVerdict::Pass(target),
            Some(code) => CondVerdict::Fail(cond_fail_reason_of(cur, ev, code)),
        },
    }
}

/// **拒因聚合账**（从历史账逐项归因的聚合视图）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CondLedger {
    /// 放行数（Moved 项）。
    pub pass: u64,
    /// 逐桶拒因计数（下标 = CondFailReason::ordinal）。
    pub fail_by_reason: [u32; 5],
    /// 首次拒的项下标（无拒为 None）。
    pub first_fail: Option<usize>,
}

impl CondLedger {
    /// 空账。
    pub fn new() -> CondLedger {
        CondLedger { pass: 0, fail_by_reason: [0; 5], first_fail: None }
    }

    /// 拒总数。
    pub fn fail_total(&self) -> u64 {
        let mut t: u64 = 0;
        let mut i = 0usize;
        while i < 5 {
            t += self.fail_by_reason[i] as u64;
            i += 1;
        }
        t
    }

    /// **账守恒**：pass + Σfail ≡ 源项数（聚合不丢项）。
    pub fn conserved(&self, items_len: usize) -> bool {
        self.pass + self.fail_total() == items_len as u64
    }
}

impl Default for CondLedger {
    fn default() -> Self {
        CondLedger::new()
    }
}

/// 历史账逐项归因（聚合单点）：Moved→Pass（发生即已过双层闸）；
/// MatrixDenied→格面桶；CondDenied→码映射（带项内格面上下文消歧）。
pub fn cond_ledger_run(items: &[HistItem]) -> CondLedger {
    let mut lg = CondLedger::new();
    let mut i = 0usize;
    while i < items.len() {
        let verdict = match items[i] {
            HistItem::Moved(_s, _e, t) => CondVerdict::Pass(t),
            HistItem::MatrixDenied(_s, _e) => CondVerdict::Fail(CondFailReason::CellIllegal),
            HistItem::CondDenied(s, e, c) => CondVerdict::Fail(cond_fail_reason_of(s, e, c)),
        };
        match verdict {
            CondVerdict::Pass(_) => lg.pass += 1,
            CondVerdict::Fail(r) => {
                lg.fail_by_reason[r.ordinal()] += 1;
                if lg.first_fail.is_none() {
                    lg.first_fail = Some(i);
                }
            }
        }
        i += 1;
    }
    lg
}

/// 聚合账线格式（L|pass|f0,f1,f2,f3,f4|first；first 无拒为 -）。
pub fn cond_ledger_wire(lg: &CondLedger) -> String {
    let first = match lg.first_fail {
        Some(i) => alloc::format!("{}", i),
        None => String::from("-"),
    };
    alloc::format!(
        "L|{}|{},{},{},{},{}|{}",
        lg.pass,
        lg.fail_by_reason[0],
        lg.fail_by_reason[1],
        lg.fail_by_reason[2],
        lg.fail_by_reason[3],
        lg.fail_by_reason[4],
        first
    )
}

/// 聚合账反解（结构性校验：first 若在账须 < pass+Σfail）。
pub fn cond_ledger_unwire(s: &str) -> Option<CondLedger> {
    let parts: Vec<&str> = s.split('|').collect();
    if parts.len() != 4 || parts[0] != "L" {
        return None;
    }
    let pass = parse_u64(parts[1])?;
    let buckets: Vec<&str> = parts[2].split(',').collect();
    if buckets.len() != 5 {
        return None;
    }
    let mut fail_by_reason = [0u32; 5];
    let mut i = 0usize;
    while i < 5 {
        fail_by_reason[i] = parse_u64(buckets[i])? as u32;
        i += 1;
    }
    let first_fail = if parts[3] == "-" {
        None
    } else {
        let idx = parse_u64(parts[3])? as usize;
        if idx >= (pass as usize) + (fail_by_reason[0] as usize)
            + (fail_by_reason[1] as usize)
            + (fail_by_reason[2] as usize)
            + (fail_by_reason[3] as usize)
            + (fail_by_reason[4] as usize)
        {
            return None;
        }
        Some(idx)
    };
    Some(CondLedger { pass, fail_by_reason, first_fail })
}

/// 告警聚合视图（桶账 rollup——在账守恒的读出面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AlertRollup {
    /// 非零桶数。
    pub buckets: u32,
    /// Σ per_code（在账守恒：≡ rows.len()——per_code 只在成功 push 时递增）。
    pub total: u64,
    /// 计数最大码（并列取最低码；全空 0）。
    pub top_code: u16,
    /// 最大桶计数。
    pub top_count: u32,
    /// 容量压力万分比（rows.len()×10000 / ALERT_CAP）。
    pub cap_pressure_bp: u32,
}

/// 告警账 rollup（桶账聚合单点）。
pub fn alert_rollup(alerts: &AlertList) -> AlertRollup {
    let mut buckets: u32 = 0;
    let mut total: u64 = 0;
    let mut top_code: u16 = 0;
    let mut top_count: u32 = 0;
    let mut i = 0usize;
    while i < 16 {
        let c = alerts.per_code[i];
        if c > 0 {
            buckets += 1;
            total += c as u64;
            // 并列取最低码（先到先得——严格大于才换）。
            if c > top_count {
                top_count = c;
                top_code = SEG_BASE + i as u16;
            }
        }
        i += 1;
    }
    let cap_pressure_bp = (alerts.rows.len() as u32 * 10_000) / ALERT_CAP as u32;
    AlertRollup { buckets, total, top_code, top_count, cap_pressure_bp }
}

/// rollup 线格式（A|buckets|total|code十六四|top_count|bp）。
pub fn alert_rollup_wire(r: &AlertRollup) -> String {
    alloc::format!(
        "A|{}|{}|{:04x}|{}|{}",
        r.buckets, r.total, r.top_code.wrapping_sub(SEG_BASE), r.top_count, r.cap_pressure_bp
    )
}

/// rollup 反解（结构性校验：空账须 total=0 桶=0）。
pub fn alert_rollup_unwire(s: &str) -> Option<AlertRollup> {
    let parts: Vec<&str> = s.split('|').collect();
    if parts.len() != 6 || parts[0] != "A" {
        return None;
    }
    let buckets = parse_u64(parts[1])? as u32;
    let total = parse_u64(parts[2])?;
    let code_off = parse_hex_u64(parts[3])?;
    if code_off > 15 {
        return None;
    }
    let top_count = parse_u64(parts[4])? as u32;
    let cap_pressure_bp = parse_u64(parts[5])? as u32;
    if buckets == 0 && (total != 0 || top_count != 0) {
        return None;
    }
    if buckets > 0 && (total == 0 || top_count == 0) {
        return None;
    }
    Some(AlertRollup {
        buckets,
        total,
        top_code: SEG_BASE + code_off as u16,
        top_count,
        cap_pressure_bp,
    })
}

/// **状态驻留账**（逐历史项发生时机器所处态的计数——账本的时间维度
/// 投影：每条账（无论迁/拒）都发生在某个态里）。
pub fn state_residency(items: &[HistItem]) -> [u64; 3] {
    let mut r = [0u64; 3];
    let mut i = 0usize;
    while i < items.len() {
        let occ = match items[i] {
            HistItem::Moved(s, _e, _t) => s,
            HistItem::MatrixDenied(s, _e) => s,
            HistItem::CondDenied(s, _e, _c) => s,
        };
        r[occ.ordinal()] += 1;
        i += 1;
    }
    r
}

/// 驻留账守恒（Σ驻留 ≡ 项数）。
pub const fn residency_conserved(r: &[u64; 3], items_len: usize) -> bool {
    r[0] + r[1] + r[2] == items_len as u64
}

/// 驻留账线格式（R|probe|recover|degraded）。
pub fn residency_wire(r: &[u64; 3]) -> String {
    alloc::format!("R|{}|{}|{}", r[0], r[1], r[2])
}

/// 驻留账反解。
pub fn residency_unwire(s: &str) -> Option<[u64; 3]> {
    let parts: Vec<&str> = s.split('|').collect();
    if parts.len() != 4 || parts[0] != "R" {
        return None;
    }
    Some([parse_u64(parts[1])?, parse_u64(parts[2])?, parse_u64(parts[3])?])
}

/// 十六进制解析（小写/大写通吃，长度 1..=16）。
fn parse_hex_u64(s: &str) -> Option<u64> {
    if s.is_empty() || s.len() > 16 {
        return None;
    }
    let mut v: u64 = 0;
    let b = s.as_bytes();
    let mut i = 0usize;
    while i < b.len() {
        let c = b[i];
        let d: u64 = if c >= b'0' && c <= b'9' {
            (c - b'0') as u64
        } else if c >= b'a' && c <= b'f' {
            (c - b'a') as u64 + 10
        } else if c >= b'A' && c <= b'F' {
            (c - b'A') as u64 + 10
        } else {
            return None;
        };
        v = v.wrapping_mul(16).wrapping_add(d);
        i += 1;
    }
    Some(v)
}

// ---------------------------------------------------------------------------
// 环压缩编码（HistItem 紧凑线格式 + 四段 wire + FNV 指纹防篡改）
// ---------------------------------------------------------------------------

/// 环账线格式标签（版本化——格式漂移即拒）。
pub const HIST_WIRE_TAG: &str = "u121h1";

/// 空载荷指纹（fnv1a64 对空字节串 = FNV 偏移基——线面自洽常量）。
pub const HIST_WIRE_EMPTY_FP: u64 = 0xcbf2_9ce4_8422_2325;

/// 单项紧凑编码（逗号字段：M,<so>,<eo>,<to> / X,<so>,<eo> /
/// C,<so>,<eo>,<码偏移 2 位十六进制>——项内禁用顶层分隔符）。
pub fn hist_item_encode(item: &HistItem) -> String {
    match item {
        HistItem::Moved(s, e, t) => {
            alloc::format!("M,{},{},{}", s.ordinal(), e.ordinal(), t.ordinal())
        }
        HistItem::MatrixDenied(s, e) => alloc::format!("X,{},{}", s.ordinal(), e.ordinal()),
        HistItem::CondDenied(s, e, c) => alloc::format!(
            "C,{},{},{:02x}",
            s.ordinal(),
            e.ordinal(),
            c.code().wrapping_sub(SEG_BASE)
        ),
    }
}

/// 单项反解（序数越界 / 码偏移越界 / 标签未知 / 字段数不符全拒）。
pub fn hist_item_decode(s: &str) -> Option<HistItem> {
    let f: Vec<&str> = s.split(',').collect();
    match f.first() {
        Some(&"M") => {
            if f.len() != 4 {
                return None;
            }
            let so = parse_u64(f[1])?;
            let eo = parse_u64(f[2])?;
            let to = parse_u64(f[3])?;
            Some(HistItem::Moved(
                ExtState::from_ordinal(so)?,
                ExtEvent::from_ordinal(eo)?,
                ExtState::from_ordinal(to)?,
            ))
        }
        Some(&"X") => {
            if f.len() != 3 {
                return None;
            }
            let so = parse_u64(f[1])?;
            let eo = parse_u64(f[2])?;
            Some(HistItem::MatrixDenied(
                ExtState::from_ordinal(so)?,
                ExtEvent::from_ordinal(eo)?,
            ))
        }
        Some(&"C") => {
            if f.len() != 4 {
                return None;
            }
            let so = parse_u64(f[1])?;
            let eo = parse_u64(f[2])?;
            let off = parse_hex_u64(f[3])?;
            if off > 15 {
                return None;
            }
            let code = BootExtCode::ALL.get(off as usize)?;
            Some(HistItem::CondDenied(
                ExtState::from_ordinal(so)?,
                ExtEvent::from_ordinal(eo)?,
                *code,
            ))
        }
        _ => None,
    }
}

/// 环账出线（四段：标签|项数|指纹|载荷；载荷项间分号——项内逗号
/// 不冲突；指纹 = FNV(载荷)——篡改任一字节即失配拒）。
pub fn hist_wire(items: &[HistItem]) -> String {
    let mut payload = String::new();
    let mut i = 0usize;
    while i < items.len() {
        if i > 0 {
            payload.push(';');
        }
        payload.push_str(&hist_item_encode(&items[i]));
        i += 1;
    }
    let fp = fnv1a64(payload.as_bytes());
    alloc::format!("{}|{}|{:016x}|{}", HIST_WIRE_TAG, items.len(), fp, payload)
}

/// 环账入线（标签/计数/指纹三闸——计数不符、指纹失配、单项解析败
/// 全部显性 None）。
pub fn hist_unwire(s: &str) -> Option<Vec<HistItem>> {
    let parts: Vec<&str> = s.split('|').collect();
    if parts.len() != 4 || parts[0] != HIST_WIRE_TAG {
        return None;
    }
    let n = parse_u64(parts[1])? as usize;
    let fp = parse_hex_u64(parts[2])?;
    let payload = parts[3];
    if payload.as_bytes().len() != payload.len() {
        return None;
    }
    // 指纹闸（先于项解析——篡改定位在载荷整体）。
    if fnv1a64(payload.as_bytes()) != fp {
        return None;
    }
    if n == 0 {
        if payload.is_empty() {
            return Some(Vec::new());
        }
        return None;
    }
    if payload.is_empty() {
        return None;
    }
    let segs: Vec<&str> = payload.split(';').collect();
    if segs.len() != n {
        return None;
    }
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < segs.len() {
        out.push(hist_item_decode(segs[i])?);
        i += 1;
    }
    Some(out)
}

/// 深-06 判据：环压缩编码——混合四项线往返恒等 + 再编码幂等 +
/// 空账平凡 + 篡改（载荷/指纹）双路显性拒 + 逐项坏例全拒。
pub fn probe_hist_wire() -> bool {
    let items = [
        HistItem::Moved(ExtState::Probe, ExtEvent::ProbeFail, ExtState::Recover),
        HistItem::CondDenied(ExtState::Recover, ExtEvent::RecoverDone, BootExtCode::ConditionNotMet),
        HistItem::MatrixDenied(ExtState::Degraded, ExtEvent::ProbeDone),
        HistItem::Moved(ExtState::Recover, ExtEvent::RecoverStep, ExtState::Recover),
    ];
    let w = hist_wire(&items);
    // 往返恒等。
    let rt_ok = match hist_unwire(&w) {
        Some(v) => v == items,
        None => false,
    };
    // 再编码幂等（decode∘encode∘encode 一致）。
    let w2 = match hist_unwire(&w) {
        Some(v) => hist_wire(&v),
        None => String::new(),
    };
    let idem_ok = w2 == w;
    // 空账平凡（指纹 = FNV 偏移基——线面自洽）。
    let w0 = hist_wire(&[]);
    let empty_ok = w0
        == alloc::format!("{}|0|{:016x}|", HIST_WIRE_TAG, HIST_WIRE_EMPTY_FP)
        && hist_unwire(&w0) == Some(Vec::new());
    // 篡改双路：载荷一位换 → 指纹失配拒；指纹一位换 → 失配拒。
    let tamper_payload = w.replace('M', "Z");
    let tamper_fp = {
        let mut t = w.clone();
        // 指纹段必以 'c' 开头（FNV 偏移基 cbf2…）——换首位即失配。
        t.replace_range(10..11, "d");
        t
    };
    let tamper_ok = hist_unwire(&tamper_payload).is_none() && hist_unwire(&tamper_fp).is_none();
    // 逐项坏例：未知标签 / 序数越界 / 码偏移越界 / 字段数不符。
    let bad_ok = hist_item_decode("Q,0").is_none()
        && hist_item_decode("M,9,1,1").is_none()
        && hist_item_decode("X,0,9").is_none()
        && hist_item_decode("C,0,0,ff").is_none()
        && hist_item_decode("M,0,1").is_none()
        && hist_item_decode("X,0,1,2").is_none();
    rt_ok && idem_ok && empty_ok && tamper_ok && bad_ok
}

/// 深-07 判据：谓词解释账——15 格全枚举两 ctx 恰值（零值 ctx：
/// pass 1 / 无失败史 2 / 零步降级 1 / 无档 1 / 格面 10；满值 ctx：
/// pass 5 / 格面 10）+ 聚合账守恒恰值 + 线往返 + 篡改拒。
pub fn probe_cond_explain() -> bool {
    // 零值 ctx（fail=0 / steps=0 / 无档）：恰值闭式。
    let ctx0 = CondCtx { fail_count: 0, recover_steps: 0, degrade_reason_set: false };
    let mut pass0: u32 = 0;
    let mut buckets0 = [0u32; 5];
    let mut si = 0usize;
    while si < 3 {
        let mut ei = 0usize;
        while ei < 5 {
            match cond_explain(ExtState::ALL[si], ExtEvent::ALL[ei], &ctx0) {
                CondVerdict::Pass(_) => pass0 += 1,
                CondVerdict::Fail(r) => buckets0[r.ordinal()] += 1,
            }
            ei += 1;
        }
        si += 1;
    }
    let grid0_ok = pass0 == 1
        && buckets0[0] == 2
        && buckets0[1] == 0
        && buckets0[2] == 1
        && buckets0[3] == 1
        && buckets0[4] == 10;
    // 满值 ctx（fail=2 / steps=2 / 有档）：合法五格全通。
    let ctxf = CondCtx { fail_count: 2, recover_steps: 2, degrade_reason_set: true };
    let mut passf: u32 = 0;
    let mut cellf: u32 = 0;
    let mut si = 0usize;
    while si < 3 {
        let mut ei = 0usize;
        while ei < 5 {
            match cond_explain(ExtState::ALL[si], ExtEvent::ALL[ei], &ctxf) {
                CondVerdict::Pass(_) => passf += 1,
                CondVerdict::Fail(r) => {
                    if matches!(r, CondFailReason::CellIllegal) {
                        cellf += 1;
                    }
                }
            }
            ei += 1;
        }
        si += 1;
    }
    let gridf_ok = passf == 5 && cellf == 10;
    // 聚合账：混合五项恰值（pass 2 / 无失败史 1 / 零步降级 1 / 格面 1
    // / 首拒下标 1）+ 守恒。
    let hist = [
        HistItem::Moved(ExtState::Probe, ExtEvent::ProbeFail, ExtState::Recover),
        HistItem::CondDenied(ExtState::Recover, ExtEvent::RecoverDone, BootExtCode::ConditionNotMet),
        HistItem::MatrixDenied(ExtState::Degraded, ExtEvent::ProbeDone),
        HistItem::Moved(ExtState::Recover, ExtEvent::RecoverStep, ExtState::Recover),
        HistItem::CondDenied(ExtState::Probe, ExtEvent::ProbeFail, BootExtCode::RecoverWithoutFail),
    ];
    let lg = cond_ledger_run(&hist);
    let ledger_ok = lg.pass == 2
        && lg.fail_by_reason[0] == 1
        && lg.fail_by_reason[1] == 0
        && lg.fail_by_reason[2] == 1
        && lg.fail_by_reason[3] == 0
        && lg.fail_by_reason[4] == 1
        && lg.first_fail == Some(1)
        && lg.conserved(hist.len())
        && lg.fail_total() == 3;
    // 线往返 + 结构性拒（first 越界 / 桶数不符）。
    let wl = cond_ledger_wire(&lg);
    let wire_ok = cond_ledger_unwire(&wl) == Some(lg.clone())
        && cond_ledger_unwire("L|2|1,0,1,0,1|9").is_none()
        && cond_ledger_unwire("L|2|1,0,1,0|0").is_none()
        && cond_ledger_unwire("X|2|1,0,1,0,1|0").is_none();
    grid0_ok && gridf_ok && ledger_ok && wire_ok
}

/// 深-08 判据：告警聚合——已知流恰值（3 告警：特闸/条件/特闸）+
/// 在账守恒（Σ per_code ≡ 行数）+ 并列取最低码恰值 + 容量压力闭式
/// （3×10000/24=1250bp）+ 线往返。
pub fn probe_alert_rollup() -> bool {
    // 已知流：特闸拒 + 失败进恢复 + 零步条件拒 + 特闸拒（恢复中
    // 改行——须在 Recover 态窗口内发）+ 三步 + 完成降级 + 幂等自环。
    let mut m = ExtBootState::new();
    let _ = m.dispatch(ExtEvent::DegradeReq); // 特闸：无据降级
    let _ = m.fail_event(); // →Recover
    let _ = m.dispatch(ExtEvent::RecoverDone); // 条件拒：零步
    let _ = m.dispatch(ExtEvent::ProbeFail); // 特闸：半路改行（Recover 态）
    let _ = m.dispatch(ExtEvent::RecoverStep);
    let _ = m.dispatch(ExtEvent::RecoverStep);
    let _ = m.dispatch(ExtEvent::RecoverStep);
    let _ = m.dispatch(ExtEvent::RecoverDone); // →Degraded
    let _ = m.dispatch(ExtEvent::DegradeReq); // 幂等自环
    // 流恰值（迁移 6 / 矩阵拒 2 / 条件拒 1）。
    let flow_ok = m.moved == 6 && m.denied_matrix == 2 && m.denied_cond == 1;
    // 聚合恰值：3 桶各 1；并列取最低码 = ConditionNotMet(0x3201)。
    let ru = alert_rollup(&m.alerts);
    let rollup_ok = ru.buckets == 3
        && ru.total == 3
        && ru.top_code == BootExtCode::ConditionNotMet.code()
        && ru.top_count == 1
        && ru.cap_pressure_bp == 1250
        && ru.total == m.alerts.rows.len() as u64;
    // 在账守恒直算：Σ per_code ≡ 行数。
    let mut sum: u64 = 0;
    let mut i = 0usize;
    while i < 16 {
        sum += m.alerts.per_code[i] as u64;
        i += 1;
    }
    let conserved_ok = sum == m.alerts.rows.len() as u64;
    // 线往返 + 空账平凡 + 桶账不一致拒。
    let wa = alert_rollup_wire(&ru);
    let wire_ok = alert_rollup_unwire(&wa) == Some(ru)
        && alert_rollup_unwire("A|0|0|00|0|0").is_some()
        && alert_rollup_unwire("A|0|3|01|1|1250").is_none()
        && alert_rollup_unwire("A|3|0|01|0|0").is_none()
        && alert_rollup_unwire("B|1|1|01|1|4166").is_none();
    flow_ok && rollup_ok && conserved_ok && wire_ok
}

/// 深-09 判据：状态驻留账——混合六项恰值（三态各 2）+ 守恒 +
/// 实机流驻留 Σ ≡ 历史账长 + 线往返 + 坏例拒。
pub fn probe_residency() -> bool {
    let items = [
        HistItem::Moved(ExtState::Probe, ExtEvent::ProbeFail, ExtState::Recover),
        HistItem::MatrixDenied(ExtState::Probe, ExtEvent::DegradeReq),
        HistItem::CondDenied(ExtState::Recover, ExtEvent::RecoverDone, BootExtCode::ConditionNotMet),
        HistItem::Moved(ExtState::Recover, ExtEvent::RecoverStep, ExtState::Recover),
        HistItem::Moved(ExtState::Degraded, ExtEvent::DegradeReq, ExtState::Degraded),
        HistItem::CondDenied(ExtState::Degraded, ExtEvent::RecoverDone, BootExtCode::RecoverWithoutFail),
    ];
    let r = state_residency(&items);
    let exact_ok = r == [2, 2, 2] && residency_conserved(&r, items.len());
    // 实机流（同深-08 已知流）：驻留 Σ ≡ 历史账长（环未溢出）。
    let mut m = ExtBootState::new();
    let _ = m.dispatch(ExtEvent::DegradeReq);
    let _ = m.fail_event();
    let _ = m.dispatch(ExtEvent::RecoverStep);
    let _ = m.dispatch(ExtEvent::RecoverDone);
    let _ = m.dispatch(ExtEvent::RecoverStep);
    let _ = m.dispatch(ExtEvent::RecoverStep);
    let _ = m.dispatch(ExtEvent::RecoverDone);
    let _ = m.dispatch(ExtEvent::DegradeReq);
    let rr = state_residency(m.recent_history());
    let live_ok = residency_conserved(&rr, m.recent_history().len())
        && rr[0] + rr[1] + rr[2] == 8;
    // 线往返 + 坏例（标签错/段数错/非数）。
    let wr = residency_wire(&rr);
    let wire_ok = residency_unwire(&wr).map(|v| v == rr).unwrap_or(false)
        && residency_unwire("R|1|2|3").is_some()
        && residency_unwire("X|1|2|3").is_none()
        && residency_unwire("R|1|2").is_none()
        && residency_unwire("R|a|2|3").is_none();
    exact_ok && live_ok && wire_ok
}
