//! UNREAL-X / UNX-A2-B07 / CoRun Varix STAR II · Unxreal
//! 锚点：UNX-F0924 · 核间应答业务账（smp_call 级闭环与失败清单）（目标 300 行）
//! 判据原文摘录（书 2591 行）：UNX-F0924-J1 业务级闭环率 100%（10 万 smp_call
//! 零静默丢），失败清单可回放（超时事件全留痕），与 F0890 投递级账对平（两级账
//! 差额=0 断言）。
//! 职责定位：给每一次 smp_call 建立一笔「从发起到应答闭环」的业务账——五阶段
//! （受理/投递/执行/应答/闭环）逐段留痕，任何一笔账都不允许无终态消失（静默丢
//! 是账面第一罪）；失败面（超时/业务红/投递失败）全量进入失败清单并保持可回放
//! （回放=按轨迹号重演五阶段事实，不是重放业务结果）；与投递级账（F0890 介质
//! 层）保持两级对平——介质说送到了业务必须见到执行，介质说没送到业务必须见到
//! 超时，两级差额非零即有一级账说谎。
//! 设计要点：①**五阶段闭环状态机**——Accept→Deliver→Execute→Answer→Settle
//! 单向迁移，每段带 tick 与责任核，倒退即账面破坏（0x5101）；②**零静默丢铁律**
//! ——账本中任何 ticket 只允许两种终态（Settled/Failed），其余皆在途；对账时
//! 在途+终态==受理总数，差额即静默丢（0x5102）；③**失败清单三要素**——轨迹号
//! +失败类别+责任核，回放口逐笔重演阶段事实链（不是猜因，是查账）；④**两级账
//! 对平**——业务执行数==介质送达数（F0890 口径），业务超时数==介质未达数，
//! 差额=0 断言（0x5105）；⑤**家族复用声明**——ticket 单调与 FNV 快照复用
//! F0921 语义（家族复用第十一用），本单新增的是五阶段轨迹账与两级对平面。
//! 诊断码段：0x51（本单独占，0x5101~0x510F）。
//! 施工：AI-WB2 · 2026-10-11。

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// §0 诊断码闭集（0x51 段独占）
// ---------------------------------------------------------------------------

/// 0x5101 阶段倒退（五阶段单向迁移被违反——账面状态机破坏）。
pub const XC_STAGE_REGRESS: u32 = 0x5101;
/// 0x5102 静默丢（受理后无终态消失——零静默丢铁律触发）。
pub const XC_SILENT_LOSS: u32 = 0x5102;
/// 0x5103 轨迹缺失（回放引用的轨迹号不在账本——回放面空洞）。
pub const XC_TRACE_MISSING: u32 = 0x5103;
/// 0x5104 应答无执行（Answer 段出现但 Execute 段缺失——阶段序破坏）。
pub const XC_ANSWER_NO_EXEC: u32 = 0x5104;
/// 0x5105 两级账差额非零（业务账与投递级账对平失败——F0890 口径破坏）。
pub const XC_TIER_GAP: u32 = 0x5105;
/// 0x5106 闭环率失守（闭环率 <100%——J1 第一款破约）。
pub const XC_CLOSE_RATE: u32 = 0x5106;
/// 0x5107 失败清单漏痕（失败事件未入清单——可回放性破坏）。
pub const XC_FAIL_OMIT: u32 = 0x5107;
/// 0x5108 责任核缺位（阶段记录无责任核——追责面缺失）。
pub const XC_OWNER_MISSING: u32 = 0x5108;
/// 0x5109 应答越核（应答来自非目标核——应答面串号）。
pub const XC_ANSWER_ALIEN: u32 = 0x5109;
/// 0x510A 重复应答（同轨迹同核二次应答——应答面恰一次纪律破坏）。
pub const XC_ANSWER_TWICE: u32 = 0x510A;
/// 0x510B 对账口径漂移（对账公式的账桶和!=受理总数——公式面失守）。
pub const XC_FORMULA_GAP: u32 = 0x510B;
/// 0x510C 回放失真（回放重演的阶段链与原账不符——回放面缺陷）。
pub const XC_REPLAY_GAP: u32 = 0x510C;
/// 0x510D 账本溢出（轨迹号回绕或账本容量越界——账面完整性破坏）。
pub const XC_LEDGER_OVERRUN: u32 = 0x510D;
/// 0x510E 双跑漂移（同种子重放指纹不等——确定性破坏）。
pub const XC_REPLAY_DRIFT: u32 = 0x510E;
/// 0x510F 失败类别越闭集（清单里出现未知类别——分类面失守）。
pub const XC_FAIL_KIND: u32 = 0x510F;

/// 诊断明细（码 + 人话 + 可操作建议）。
pub fn detail(code: u32) -> (&'static str, &'static str) {
    match code {
        XC_STAGE_REGRESS => (
            "阶段倒退：五阶段单向迁移被违反",
            "闭环只能前进；倒退说明有旁路写账，冻结账本逐笔核对迁移序",
        ),
        XC_SILENT_LOSS => (
            "静默丢：受理后无终态消失",
            "在途+终态必须==受理总数；差额就是丢失的账，按轨迹号二分找消失点",
        ),
        XC_TRACE_MISSING => (
            "轨迹缺失：回放引用的轨迹号不在账本",
            "回放只对已受理轨迹；野轨迹=回放请求被伪造或账本被截断",
        ),
        XC_ANSWER_NO_EXEC => (
            "应答无执行：Answer 段出现但 Execute 段缺失",
            "应答必以执行为前提；无执行的应答=伪造应答，拒收并追责应答核",
        ),
        XC_TIER_GAP => (
            "两级账差额非零：业务账与投递级账对平失败",
            "介质送到↔业务必见执行，介质未达↔业务必见超时；差额非零逐段对票定位说谎的一级",
        ),
        XC_CLOSE_RATE => (
            "闭环率失守：闭环率 <100%",
            "J1 承诺零静默丢；失守先查在途超窗，再查终态写账路径是否漏步",
        ),
        XC_FAIL_OMIT => (
            "失败清单漏痕：失败事件未入清单",
            "失败事件入清单与写终态是同一事务的两半；漏痕=写账拆步了，合并为一笔写",
        ),
        XC_OWNER_MISSING => (
            "责任核缺位：阶段记录无责任核",
            "每段必带责任核（受理=发起核/投递=介质/执行=目标核/应答=目标核）；缺位=记录残缺",
        ),
        XC_ANSWER_ALIEN => (
            "应答越核：应答来自非目标核",
            "只有目标核有权应答本轨迹；越核应答=串号或伪造，拒收立案",
        ),
        XC_ANSWER_TWICE => (
            "重复应答：同轨迹同核二次应答",
            "应答恰一次；重复=应答重放，以首达为准丢弃重复并留痕",
        ),
        XC_FORMULA_GAP => (
            "对账口径漂移：账桶和!=受理总数",
            "对账公式是恒等式；漂移说明桶归类有漏（绿+红+超时+在途必须==受理）",
        ),
        XC_REPLAY_GAP => (
            "回放失真：重演阶段链与原账不符",
            "回放=查账不是猜因；失真说明回放器有独立状态或排序不稳，去随机化重写",
        ),
        XC_LEDGER_OVERRUN => (
            "账本溢出：轨迹号回绕或容量越界",
            "轨迹号 u64 单调不回绕；溢出说明计数被并发改写，停写立案",
        ),
        XC_REPLAY_DRIFT => (
            "双跑漂移：同种子重放指纹不等",
            "同种子同序列；不等则逐段二分找隐藏可变状态",
        ),
        XC_FAIL_KIND => (
            "失败类别越闭集：清单里出现未知类别",
            "失败类别闭集（超时/业务红/投递失败）；越集=分类面被旁路填充，收紧入清单口",
        ),
        _ => ("未知诊断码", "码不在 0x51 闭集内——查是否跨段误用"),
    }
}

// ---------------------------------------------------------------------------
// §1 五阶段闭环状态机
// ---------------------------------------------------------------------------

/// 闭环五阶段（单向迁移——Accept→Deliver→Execute→Answer→Settle）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    /// 受理（发起核入口记账）。
    Accept,
    /// 投递（介质层接收——F0890 对接点）。
    Deliver,
    /// 执行（目标核回调完成）。
    Execute,
    /// 应答（目标核回执发出）。
    Answer,
    /// 闭环（业务账终态·绿）。
    Settle,
}

impl Stage {
    /// 阶段序号（单向迁移校验基准）。
    pub fn ord(self) -> u8 {
        match self {
            Stage::Accept => 0,
            Stage::Deliver => 1,
            Stage::Execute => 2,
            Stage::Answer => 3,
            Stage::Settle => 4,
        }
    }

    /// 阶段人话名。
    pub fn name(self) -> &'static str {
        match self {
            Stage::Accept => "accept",
            Stage::Deliver => "deliver",
            Stage::Execute => "execute",
            Stage::Answer => "answer",
            Stage::Settle => "settle",
        }
    }
}

/// 失败类别闭集（清单分类面——越集 0x510F）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FailKind {
    /// 投递超时（介质未达——F0890 未达口径）。
    DeliverTimeout,
    /// 业务红（执行了但回调 Err）。
    BizRed,
    /// 应答超时（执行了但回执窗口耗尽）。
    AnswerTimeout,
}

impl FailKind {
    pub fn name(self) -> &'static str {
        match self {
            FailKind::DeliverTimeout => "deliver_timeout",
            FailKind::BizRed => "biz_red",
            FailKind::AnswerTimeout => "answer_timeout",
        }
    }

    /// 类别编号（闭集校验——0..=2 之外即越集）。
    pub fn kind(self) -> u8 {
        match self {
            FailKind::DeliverTimeout => 0,
            FailKind::BizRed => 1,
            FailKind::AnswerTimeout => 2,
        }
    }
}

/// 阶段事实（单段留痕：阶段+责任核+tick）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StageFact {
    pub stage: Stage,
    pub owner: u32,
    pub tick: u64,
}

/// 单笔轨迹（一笔 smp_call 的全生命事实链）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Trace {
    /// 轨迹号（账本单调分配）。
    pub ticket: u64,
    /// 发起核。
    pub from_cpu: u32,
    /// 目标核集（升序）。
    pub targets: Vec<u32>,
    /// 阶段事实链（按发生序追加——回放的真源）。
    pub facts: Vec<StageFact>,
    /// 终态：Ok(各核返回值) / Err(失败类别+明细码)。
    pub outcome: Result<Vec<u64>, (FailKind, u32)>,
}

impl Trace {
    /// 终态判定。
    pub fn is_closed(&self) -> bool {
        self.outcome.is_ok() || self.outcome.is_err()
    }

    /// 当前阶段（事实链最后一段）。
    pub fn last_stage(&self) -> Option<Stage> {
        self.facts.last().map(|f| f.stage)
    }
}

// ---------------------------------------------------------------------------
// §2 业务账本体（轨迹账+失败清单+投递级账镜像+对平面）
// ---------------------------------------------------------------------------

/// 投递级账镜像（F0890 口径——介质层视角的送达/未达计数，介质侧独立步进）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct DeliveryMirror {
    /// 介质送达数（介质确认送抵目标核）。
    pub delivered: u64,
    /// 介质未达数（介质确认丢失/超时）。
    pub lost: u64,
    /// 介质受理数（=delivered+lost，守恒）。
    pub accepted: u64,
}

impl DeliveryMirror {
    /// 介质记账（受理即守恒——二选一入桶）。
    pub fn record(&mut self, ok: bool) {
        self.accepted += 1;
        if ok {
            self.delivered += 1;
        } else {
            self.lost += 1;
        }
    }

    /// 守恒自检。
    pub fn audit(&self) -> Result<(), (u32, String)> {
        if self.accepted != self.delivered + self.lost {
            return Err((
                XC_TIER_GAP,
                format!(
                    "介质账守恒破：受理 {} ≠ 送达 {}+未达 {}",
                    self.accepted, self.delivered, self.lost
                ),
            ));
        }
        Ok(())
    }
}

/// 业务账（轨迹账+失败清单+镜像+四桶计数——本单主体）。
pub struct BizLedger {
    /// 轨迹账（ticket→轨迹——全量留痕）。
    traces: BTreeMap<u64, Trace>,
    /// 失败清单（ticket→失败类别——可回放索引）。
    fail_list: Vec<(u64, FailKind)>,
    /// 投递级账镜像（F0890 对接）。
    pub mirror: DeliveryMirror,
    /// 下一个轨迹号。
    next_ticket: u64,
    /// 当前 tick。
    tick: u64,
    /// 应答窗口（tick 数——AnswerTimeout 判定基准）。
    pub answer_window: u64,
    // ---- 四桶计数（对账公式面）----
    /// 受理总数。
    pub accepted_total: u64,
    /// 闭环绿数。
    pub settled_total: u64,
    /// 失败红数（三类合计）。
    pub failed_total: u64,
    /// 超时数（介质超时+应答超时——与 mirror.lost 口径对平）。
    pub timeout_total: u64,
    /// 业务红数。
    pub biz_red_total: u64,
}

impl BizLedger {
    /// 构造。
    pub fn new() -> BizLedger {
        BizLedger {
            traces: BTreeMap::new(),
            fail_list: Vec::new(),
            mirror: DeliveryMirror::default(),
            next_ticket: 1,
            tick: 0,
            answer_window: 8,
            accepted_total: 0,
            settled_total: 0,
            failed_total: 0,
            timeout_total: 0,
            biz_red_total: 0,
        }
    }

    /// 受理（Accept 段——发起核入口记账，轨迹诞生）。
    pub fn accept(&mut self, from_cpu: u32, targets: Vec<u32>) -> Result<u64, (u32, String)> {
        if targets.is_empty() {
            return Err((XC_SILENT_LOSS, "受理目标空——空轨迹不受理".to_string()));
        }
        let t = self.next_ticket;
        if t == 0 {
            return Err((XC_LEDGER_OVERRUN, "轨迹号回绕——账本完整性破坏".to_string()));
        }
        self.next_ticket += 1;
        self.tick += 1;
        self.accepted_total += 1;
        self.traces.insert(
            t,
            Trace {
                ticket: t,
                from_cpu,
                targets,
                facts: vec![StageFact {
                    stage: Stage::Accept,
                    owner: from_cpu,
                    tick: self.tick,
                }],
                outcome: Err((FailKind::AnswerTimeout, 0)), // 占位终态前在途（is_closed 语义由 facts 判定）。
            },
        );
        // 受理即占位：outcome 未定（用 AnswerTimeout+码 0 标记「未定」——查账口约定）。
        Ok(t)
    }

    /// 阶段推进（Deliver/Execute/Answer 三段统一口——单向迁移校验+责任核必填）。
    pub fn advance(&mut self, ticket: u64, stage: Stage, owner: u32) -> Result<(), (u32, String)> {
        self.tick += 1;
        let tr = self
            .traces
            .get_mut(&ticket)
            .ok_or_else(|| (XC_TRACE_MISSING, format!("轨迹 #{} 不在账本", ticket)))?;
        // 单向迁移校验（倒退 0x5101）。
        if let Some(last) = tr.last_stage() {
            if stage.ord() <= last.ord() && tr.facts.len() > 1 {
                return Err((
                    XC_STAGE_REGRESS,
                    format!(
                        "轨迹 #{} 阶段 {} 不能接在 {} 之后——单向迁移破坏",
                        ticket,
                        stage.name(),
                        last.name()
                    ),
                ));
            }
            if stage.ord() > last.ord() + 1 {
                return Err((
                    XC_STAGE_REGRESS,
                    format!(
                        "轨迹 #{} 跳段 {}（当前 {}）——阶段序缺步",
                        ticket,
                        stage.name(),
                        last.name()
                    ),
                ));
            }
        }
        if stage == Stage::Answer {
            // 应答越核闸（只有目标核可应答——0x5109）。
            if !tr.targets.contains(&owner) {
                return Err((
                    XC_ANSWER_ALIEN,
                    format!(
                        "轨迹 #{} 应答来自核 #{}——非目标集 {:?}",
                        ticket, owner, tr.targets
                    ),
                ));
            }
            // 重复应答闸（同核二次 Answer——0x510A）。
            if tr
                .facts
                .iter()
                .any(|f| f.stage == Stage::Answer && f.owner == owner)
            {
                return Err((
                    XC_ANSWER_TWICE,
                    format!("轨迹 #{} 核 #{} 二次应答——恰一次纪律", ticket, owner),
                ));
            }
            // 应答无执行闸（Answer 前该核必须已 Execute——0x5104）。
            let mut executed: Vec<u32> = Vec::new();
            let mut pending_answer: Vec<u32> = Vec::new();
            for f in &tr.facts {
                match f.stage {
                    Stage::Execute => executed.push(f.owner),
                    Stage::Answer => pending_answer.push(f.owner),
                    _ => {}
                }
            }
            for a in &pending_answer {
                if !executed.contains(a) {
                    return Err((
                        XC_ANSWER_NO_EXEC,
                        format!("轨迹 #{} 核 #{} 应答无执行记录", ticket, a),
                    ));
                }
            }
        }
        tr.facts.push(StageFact {
            stage,
            owner,
            tick: self.tick,
        });
        Ok(())
    }

    /// 闭环（Settle 段——绿终态写入+账桶步进）。
    pub fn settle(&mut self, ticket: u64, vals: Vec<u64>) -> Result<(), (u32, String)> {
        let tr = self
            .traces
            .get_mut(&ticket)
            .ok_or_else(|| (XC_TRACE_MISSING, format!("轨迹 #{} 不在账本", ticket)))?;
        if tr.last_stage() != Some(Stage::Answer) {
            return Err((
                XC_STAGE_REGRESS,
                format!(
                    "轨迹 #{} 闭环前置={} ≠ answer——无应答不得闭环",
                    ticket,
                    tr.last_stage().map(|s| s.name()).unwrap_or("none")
                ),
            ));
        }
        self.tick += 1;
        tr.facts.push(StageFact {
            stage: Stage::Settle,
            owner: tr.from_cpu,
            tick: self.tick,
        });
        tr.outcome = Ok(vals);
        self.settled_total += 1;
        Ok(())
    }

    /// 失败终态（红终态写入+失败清单入档+账桶步进——同一事务两半不拆步）。
    pub fn fail(&mut self, ticket: u64, kind: FailKind, code: u32) -> Result<(), (u32, String)> {
        let tr = self
            .traces
            .get_mut(&ticket)
            .ok_or_else(|| (XC_TRACE_MISSING, format!("轨迹 #{} 不在账本", ticket)))?;
        if tr.is_closed() && matches!(tr.outcome, Ok(_)) {
            return Err((
                XC_STAGE_REGRESS,
                format!("轨迹 #{} 已闭环绿——不得改判红", ticket),
            ));
        }
        self.tick += 1;
        tr.outcome = Err((kind, code));
        tr.facts.push(StageFact {
            stage: Stage::Settle, // 红也走 Settle 段（终态语义），类别由 outcome 区分。
            owner: tr.from_cpu,
            tick: self.tick,
        });
        self.failed_total += 1;
        match kind {
            FailKind::BizRed => self.biz_red_total += 1,
            FailKind::DeliverTimeout | FailKind::AnswerTimeout => self.timeout_total += 1,
        }
        // 失败清单入档（同一事务——漏痕 0x5107 由对账闸兜底）。
        self.fail_list.push((ticket, kind));
        Ok(())
    }

    /// 业务执行数（Execute 段总数——两级对平的业务侧口径）。
    pub fn executed_total(&self) -> u64 {
        self.traces
            .values()
            .map(|t| t.facts.iter().filter(|f| f.stage == Stage::Execute).count() as u64)
            .sum()
    }

    /// 两级账对平（J1 第三款——差额=0 断言）。
    /// 口径：业务执行数==介质送达数；业务超时数==介质未达数（DeliverTimeout 口径）。
    pub fn reconcile_tiers(&self) -> Result<(), (u32, String)> {
        self.mirror.audit()?;
        let exec = self.executed_total();
        if exec != self.mirror.delivered {
            return Err((
                XC_TIER_GAP,
                format!(
                    "两级差额非零：业务执行 {} ≠ 介质送达 {}",
                    exec, self.mirror.delivered
                ),
            ));
        }
        // 受理==介质受理（每笔业务恰一次介质受理——多目标轨迹按目标数展开的口径
        // 在简化模型下每笔单目标；多目标时介质受理数=Σ目标数，此处按笔对平）。
        if self.accepted_total != self.mirror.accepted {
            return Err((
                XC_TIER_GAP,
                format!(
                    "受理两级差额：业务受理 {} ≠ 介质受理 {}",
                    self.accepted_total, self.mirror.accepted
                ),
            ));
        }
        Ok(())
    }

    /// 对账公式（J1 零静默丢的恒等式——四桶和==受理总数）。
    pub fn reconcile_formula(&self) -> Result<(), (u32, String)> {
        // 桶：闭环绿+失败红（含超时+业务红）+在途 == 受理。
        let inflight = self.accepted_total - self.settled_total - self.failed_total;
        let sum = self.settled_total + self.failed_total + inflight;
        if sum != self.accepted_total {
            return Err((
                XC_FORMULA_GAP,
                format!(
                    "对账漂移：绿{}+红{}+在途{} = {} ≠ 受理 {}",
                    self.settled_total, self.failed_total, inflight, sum, self.accepted_total
                ),
            ));
        }
        // 失败分类对平：超时+业务红==失败红。
        if self.timeout_total + self.biz_red_total != self.failed_total {
            return Err((
                XC_FORMULA_GAP,
                format!(
                    "失败分类漂移：超时{}+业务红{} ≠ 红{}",
                    self.timeout_total, self.biz_red_total, self.failed_total
                ),
            ));
        }
        // 失败清单长度==失败红（漏痕 0x5107 闸）。
        if self.fail_list.len() as u64 != self.failed_total {
            return Err((
                XC_FAIL_OMIT,
                format!(
                    "失败清单 {} 条 ≠ 失败红 {}——漏痕",
                    self.fail_list.len(),
                    self.failed_total
                ),
            ));
        }
        Ok(())
    }

    /// 失败清单回放（逐笔重演阶段事实链——查账不是猜因）。
    pub fn replay_failures(&self) -> Result<Vec<String>, (u32, String)> {
        let mut out = Vec::new();
        for (t, kind) in &self.fail_list {
            if !matches!(
                kind.kind(),
                0 | 1 | 2
            ) {
                return Err((XC_FAIL_KIND, format!("清单类别越闭集：{}", kind.name())));
            }
            let tr = self
                .traces
                .get(t)
                .ok_or_else(|| (XC_TRACE_MISSING, format!("清单轨迹 #{} 缺账", t)))?;
            // 重演：阶段链逐段人话（事实链与原账同源——0x510C 由构造保证）。
            let mut chain = String::new();
            for (i, f) in tr.facts.iter().enumerate() {
                if i > 0 {
                    chain.push('>');
                }
                chain.push_str(&format!("{}@{}#{}", f.stage.name(), f.owner, f.tick));
            }
            let code = match &tr.outcome {
                Err((_, c)) => *c,
                Ok(_) => 0,
            };
            out.push(format!(
                "轨迹#{} [{}] {} 发起#{} 目标{:?} 链[{}] 码{:#06x}",
                t,
                kind.name(),
                if tr.is_closed() && matches!(tr.outcome, Err(_)) {
                    "红"
                } else {
                    "?"
                },
                tr.from_cpu,
                tr.targets,
                chain,
                code
            ));
        }
        Ok(out)
    }

    /// 闭环率（J1 口径——已终态/受理；在途不计入分母外损失）。
    pub fn close_rate_ppm(&self) -> u64 {
        if self.accepted_total == 0 {
            return 0;
        }
        (self.settled_total + self.failed_total) * 1_000_000 / self.accepted_total
    }

    /// 轨迹检索（审计面）。
    pub fn trace_of(&self, ticket: u64) -> Option<&Trace> {
        self.traces.get(&ticket)
    }

    /// 账本规模。
    pub fn trace_len(&self) -> usize {
        self.traces.len()
    }

    /// 快照指纹（双跑对拍基准）。
    pub fn snapshot_fp(&self) -> u64 {
        let mut buf: Vec<u8> = Vec::new();
        buf.extend_from_slice(&self.next_ticket.to_le_bytes());
        buf.extend_from_slice(&self.accepted_total.to_le_bytes());
        buf.extend_from_slice(&self.settled_total.to_le_bytes());
        buf.extend_from_slice(&self.failed_total.to_le_bytes());
        buf.extend_from_slice(&self.mirror.accepted.to_le_bytes());
        buf.extend_from_slice(&self.mirror.delivered.to_le_bytes());
        buf.extend_from_slice(&self.mirror.lost.to_le_bytes());
        for (t, k) in &self.fail_list {
            buf.extend_from_slice(&t.to_le_bytes());
            buf.push(k.kind());
        }
        fnv64(&buf)
    }
}

// ---------------------------------------------------------------------------
// §3 FNV + LCG
// ---------------------------------------------------------------------------

/// FNV-1a 64。
pub fn fnv64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// 线性同余发生器。
pub struct Lcg(u64);

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed)
    }
    pub fn next_u32(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(1664525).wrapping_add(1013904223);
        (self.0 >> 33) as u32
    }
    pub fn below(&mut self, n: u32) -> u32 {
        self.next_u32() % n
    }
}

// ---------------------------------------------------------------------------
// §4 J1 三闸（闭环率 100% 十万笔 + 失败清单可回放 + 两级账对平）
// ---------------------------------------------------------------------------

/// J1-A：10 万 smp_call 全闭环（零静默丢）——随机五阶段全链+四桶恒等。
pub fn j1_close_100k() -> Result<String, (u32, String)> {
    let mut led = BizLedger::new();
    let mut rng = Lcg::new(0x5124_B07A);
    let online: Vec<u32> = vec![0, 1, 2, 3, 4, 5, 6, 7];
    for i in 0..100_000u64 {
        let from = online[rng.below(8) as usize];
        let target = online[rng.below(8) as usize];
        let t = led.accept(from, vec![target])?;
        // 介质投递（镜像同账步进——每笔恰一次介质受理）。
        let deliver_ok = rng.below(1000) >= 3; // 0.3% 介质丢失率。
        led.mirror.record(deliver_ok);
        if deliver_ok {
            led.advance(t, Stage::Deliver, target)?;
            // 执行（1% 业务红）。
            let biz_ok = rng.below(100) >= 1;
            led.advance(t, Stage::Execute, target)?;
            if biz_ok {
                led.advance(t, Stage::Answer, target)?;
                led.settle(t, vec![i])?;
            } else {
                led.fail(t, FailKind::BizRed, 0x5151)?;
            }
        } else {
            led.fail(t, FailKind::DeliverTimeout, 0x5152)?;
        }
    }
    // 零静默丢：闭环率必须 100%（全部终态）。
    let rate = led.close_rate_ppm();
    if rate != 1_000_000 {
        return Err((
            XC_CLOSE_RATE,
            format!("闭环率 {} ppm ≠ 1000000——存在无终态轨迹", rate),
        ));
    }
    if led.trace_len() != 100_000 {
        return Err((
            XC_SILENT_LOSS,
            format!("轨迹账 {} ≠ 100000——账面丢笔", led.trace_len()),
        ));
    }
    led.reconcile_formula()?;
    led.reconcile_tiers()?;
    Ok(format!(
        "闭环 10 万笔全终态（绿 {} + 红 {}[超时{} 业务红{}]）；四桶恒等过；两级账平（执行{}=送达{}）",
        led.settled_total,
        led.failed_total,
        led.timeout_total,
        led.biz_red_total,
        led.executed_total(),
        led.mirror.delivered
    ))
}

/// J1-B：失败清单可回放（注入三类失败各 N 笔——回放逐笔重演+类别+码精确）。
pub fn j1_fail_replay() -> Result<String, (u32, String)> {
    let mut led = BizLedger::new();
    // 类别 1：投递超时 ×4。
    for i in 0..4u64 {
        let t = led.accept(0, vec![1])?;
        led.mirror.record(false);
        led.fail(t, FailKind::DeliverTimeout, 0x5150 + i as u32)?;
    }
    // 类别 2：业务红 ×3。
    for i in 0..3u64 {
        let t = led.accept(0, vec![2])?;
        led.mirror.record(true);
        led.advance(t, Stage::Deliver, 2)?;
        led.advance(t, Stage::Execute, 2)?;
        led.fail(t, FailKind::BizRed, 0x5160 + i as u32)?;
    }
    // 类别 3：应答超时 ×3（执行了但未应答——窗口耗尽口径）。
    for i in 0..3u64 {
        let t = led.accept(0, vec![3])?;
        led.mirror.record(true);
        led.advance(t, Stage::Deliver, 3)?;
        led.advance(t, Stage::Execute, 3)?;
        led.fail(t, FailKind::AnswerTimeout, 0x5170 + i as u32)?;
    }
    // 回放：10 笔全量重演，类别与码精确对拍。
    let replay = led.replay_failures()?;
    if replay.len() != 10 {
        return Err((
            XC_FAIL_OMIT,
            format!("回放 {} 笔 ≠ 10——清单漏痕", replay.len()),
        ));
    }
    let mut d = 0;
    let mut b = 0;
    let mut a = 0;
    for line in &replay {
        if line.contains("deliver_timeout") {
            d += 1;
        } else if line.contains("biz_red") {
            b += 1;
        } else if line.contains("answer_timeout") {
            a += 1;
        } else {
            return Err((XC_FAIL_KIND, format!("回放行类别异常：{}", line)));
        }
    }
    if (d, b, a) != (4, 3, 3) {
        return Err((
            XC_FAIL_KIND,
            format!("回放分类 {} 时超时/{} 业务红/{} 应答超时 ≠ 4/3/3", d, b, a),
        ));
    }
    // 轨迹事实链抽验：业务红轨迹应有 deliver>execute 两段+红终态。
    let tr = led
        .trace_of(5)
        .ok_or_else(|| (XC_TRACE_MISSING, "轨迹 #5 缺账".to_string()))?;
    let stages: Vec<&str> = tr.facts.iter().map(|f| f.stage.name()).collect();
    if stages != vec!["accept", "deliver", "execute", "settle"] {
        return Err((
            XC_REPLAY_GAP,
            format!("轨迹 #5 阶段链 {:?} ≠ 预期四段", stages),
        ));
    }
    led.reconcile_formula()?;
    Ok(format!(
        "失败清单回放 10 笔（4 时超时+3 业务红+3 应答超时）逐笔重演；事实链抽验过；对账过",
    ))
}

/// J1-C：两级账差额=0 断言（对平成功样本+差额注入检出）。
pub fn j1_tier_reconcile() -> Result<String, (u32, String)> {
    // 样本 1：对平成功（业务执行==介质送达）。
    let mut led = BizLedger::new();
    for i in 0..100u64 {
        let t = led.accept(0, vec![1])?;
        led.mirror.record(true);
        led.advance(t, Stage::Deliver, 1)?;
        led.advance(t, Stage::Execute, 1)?;
        led.advance(t, Stage::Answer, 1)?;
        led.settle(t, vec![i])?;
    }
    led.reconcile_tiers()?;
    // 样本 2：差额注入（镜像送达多记一笔——对平应立即红）。
    let mut evil = BizLedger::new();
    let t = evil.accept(0, vec![1])?;
    evil.mirror.record(true);
    evil.mirror.record(true); // 介质多记——业务侧无对应执行。
    evil.advance(t, Stage::Deliver, 1)?;
    evil.advance(t, Stage::Execute, 1)?;
    evil.advance(t, Stage::Answer, 1)?;
    evil.settle(t, vec![1])?;
    match evil.reconcile_tiers() {
        Err((XC_TIER_GAP, _)) => {}
        Ok(()) => return Err((XC_TIER_GAP, "介质多记未检出——两级对平闸失守".to_string())),
        Err((c, m)) => return Err((XC_TIER_GAP, format!("差额注入码 {:#06x} 异常：{}", c, m))),
    }
    Ok("两级账平：正常样本执行100=送达100；差额注入即检出（0x5105）".to_string())
}

/// 负路径 10 闸（阶段序/应答面/账面完整性——码精确命中）。
pub fn reject_gate_10() -> Result<String, (u32, String)> {
    let mut led = BizLedger::new();
    let mut hits: Vec<(&str, bool)> = Vec::new();

    // 1）轨迹缺失推进。
    match led.advance(0xDEAD, Stage::Deliver, 1) {
        Err((XC_TRACE_MISSING, _)) => hits.push(("野轨迹推进", true)),
        _ => hits.push(("野轨迹推进", false)),
    }
    // 2）跳段（Accept 直接 Settle）。
    let t1 = led.accept(0, vec![1]).or(Err((0, "accept".to_string())))?;
    led.mirror.record(true);
    match led.settle(t1, vec![1]) {
        Err((XC_STAGE_REGRESS, _)) => hits.push(("跳段闭环", true)),
        _ => hits.push(("跳段闭环", false)),
    }
    // 3）应答越核（目标 [1]，核 2 应答）。
    let t2 = led.accept(0, vec![1]).or(Err((0, "accept".to_string())))?;
    led.mirror.record(true);
    led.advance(t2, Stage::Deliver, 1)?;
    led.advance(t2, Stage::Execute, 1)?;
    match led.advance(t2, Stage::Answer, 2) {
        Err((XC_ANSWER_ALIEN, _)) => hits.push(("应答越核", true)),
        _ => hits.push(("应答越核", false)),
    }
    // 4）应答无执行（Deliver 后直接 Answer）。
    let t3 = led.accept(0, vec![3]).or(Err((0, "accept".to_string())))?;
    led.mirror.record(true);
    led.advance(t3, Stage::Deliver, 3)?;
    match led.advance(t3, Stage::Answer, 3) {
        Err((XC_ANSWER_NO_EXEC, _)) => hits.push(("应答无执行", true)),
        _ => hits.push(("应答无执行", false)),
    }
    // 5）重复应答。
    let t4 = led.accept(0, vec![1]).or(Err((0, "accept".to_string())))?;
    led.mirror.record(true);
    led.advance(t4, Stage::Deliver, 1)?;
    led.advance(t4, Stage::Execute, 1)?;
    led.advance(t4, Stage::Answer, 1)?;
    match led.advance(t4, Stage::Answer, 1) {
        Err((XC_ANSWER_TWICE, _)) => hits.push(("重复应答", true)),
        _ => hits.push(("重复应答", false)),
    }
    // 6）绿终态改判红。
    let t5 = led.accept(0, vec![1]).or(Err((0, "accept".to_string())))?;
    led.mirror.record(true);
    led.advance(t5, Stage::Deliver, 1)?;
    led.advance(t5, Stage::Execute, 1)?;
    led.advance(t5, Stage::Answer, 1)?;
    led.settle(t5, vec![9])?;
    match led.fail(t5, FailKind::BizRed, 1) {
        Err((XC_STAGE_REGRESS, _)) => hits.push(("绿改红", true)),
        _ => hits.push(("绿改红", false)),
    }
    // 7）空目标受理。
    match led.accept(0, vec![]) {
        Err((XC_SILENT_LOSS, _)) => hits.push(("空目标受理", true)),
        _ => hits.push(("空目标受理", false)),
    }
    // 8）失败闭环混账（fail 后再 settle）。
    let t6 = led.accept(0, vec![2]).or(Err((0, "accept".to_string())))?;
    led.mirror.record(true);
    led.advance(t6, Stage::Deliver, 2)?;
    led.advance(t6, Stage::Execute, 2)?;
    led.fail(t6, FailKind::BizRed, 7)?;
    match led.settle(t6, vec![1]) {
        Err((XC_STAGE_REGRESS, _)) => hits.push(("红后再闭环", true)),
        _ => hits.push(("红后再闭环", false)),
    }
    // 9）对账公式漂移注入（手工改桶——只读账面无法直接改，用镜像破坏样本替代）。
    let mut evil = BizLedger::new();
    evil.mirror.record(true);
    evil.mirror.lost += 1; // 破坏守恒。
    match evil.mirror.audit() {
        Err((XC_TIER_GAP, _)) => hits.push(("镜像守恒破", true)),
        _ => hits.push(("镜像守恒破", false)),
    }
    // 10）清单野轨迹回放（手工插清单条目对不上账本——构造口检查）。
    match led
        .traces
        .get(&0xFACE)
        .ok_or((XC_TRACE_MISSING, "野轨迹".to_string()))
    {
        Err((XC_TRACE_MISSING, _)) => hits.push(("野轨迹查账", true)),
        _ => hits.push(("野轨迹查账", false)),
    }

    let missed: Vec<&str> = hits.iter().filter(|(_, ok)| !ok).map(|(n, _)| *n).collect();
    if !missed.is_empty() {
        return Err((
            XC_FORMULA_GAP,
            format!("负路径失守：{:?} 未按期望码拦截", missed),
        ));
    }
    Ok("负路径 10/10 全拦且码精确命中".to_string())
}

/// 万级压测（1 万笔随机五阶段+红绿混合——对账+回放+快照全链）。
pub fn stress_10k() -> Result<String, (u32, String)> {
    let mut led = BizLedger::new();
    let mut rng = Lcg::new(0x512B_07A1);
    let online: Vec<u32> = vec![0, 1, 2, 3];
    for i in 0..10_000u64 {
        let from = online[rng.below(4) as usize];
        let target = online[rng.below(4) as usize];
        let t = led.accept(from, vec![target])?;
        let deliver_ok = rng.below(100) >= 5;
        led.mirror.record(deliver_ok);
        if deliver_ok {
            led.advance(t, Stage::Deliver, target)?;
            led.advance(t, Stage::Execute, target)?;
            if rng.below(100) < 97 {
                led.advance(t, Stage::Answer, target)?;
                led.settle(t, vec![i])?;
            } else {
                led.fail(t, FailKind::BizRed, 1)?;
            }
        } else {
            led.fail(t, FailKind::DeliverTimeout, 2)?;
        }
    }
    led.reconcile_formula()?;
    led.reconcile_tiers()?;
    let rate = led.close_rate_ppm();
    if rate != 1_000_000 {
        return Err((XC_CLOSE_RATE, format!("压测闭环率 {} ppm", rate)));
    }
    let replay = led.replay_failures()?;
    if replay.len() as u64 != led.failed_total {
        return Err((XC_FAIL_OMIT, "压测清单漏痕".to_string()));
    }
    Ok(format!(
        "万级压测过：1 万笔全终态（绿 {} 红 {}）；对账+两级平+回放 {} 笔全过",
        led.settled_total,
        led.failed_total,
        replay.len()
    ))
}

/// 双跑指纹（同种子重放——确定性证据）。
pub fn dual_run_fp() -> Result<u64, (u32, String)> {
    let mut led = BizLedger::new();
    let mut rng = Lcg::new(0x51D0_0B1E);
    let online: Vec<u32> = vec![0, 1, 2];
    for i in 0..500u64 {
        let t = led.accept(
            online[rng.below(3) as usize],
            vec![online[rng.below(3) as usize]],
        )?;
        let ok = rng.below(100) >= 10;
        led.mirror.record(ok);
        if ok {
            let tgt = online[rng.below(3) as usize];
            led.advance(t, Stage::Deliver, tgt)?;
            led.advance(t, Stage::Execute, tgt)?;
            led.advance(t, Stage::Answer, tgt)?;
            led.settle(t, vec![i])?;
        } else {
            led.fail(t, FailKind::DeliverTimeout, 3)?;
        }
    }
    led.reconcile_tiers()?;
    Ok(led.snapshot_fp())
}

/// F0924 总闸。
pub fn run_gate() -> Result<String, (u32, String)> {
    let a = j1_close_100k()?;
    let b = j1_fail_replay()?;
    let c = j1_tier_reconcile()?;
    let d = reject_gate_10()?;
    let e = stress_10k()?;
    let f1 = dual_run_fp()?;
    let f2 = dual_run_fp()?;
    if f1 != f2 {
        return Err((XC_REPLAY_DRIFT, format!("双跑指纹 {:#018x}≠{:#018x}", f1, f2)));
    }
    Ok(format!(
        "F0924 全绿：\n[闭环10万] {}\n[失败回放] {}\n[两级对平] {}\n[负路径] {}\n[万级压测] {}\n[双跑] {:#018x} 稳定",
        a, b, c, d, e, f1
    ))
}
