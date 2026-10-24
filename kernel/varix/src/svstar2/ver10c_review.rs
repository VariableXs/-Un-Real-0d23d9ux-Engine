//! VE-F3543 · persona 审核标准 —— 三查标准 + 三要素驳回 + 时限承诺 + 标准版本化。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3543`
//!
//! # 职责（锚点原文拆解）
//!
//! - **三查标准**（[`ReviewChecklist`]）：档案完整性 / 无障碍硬线 /
//!   性能预算三查——上架审核不是玄学，是三条机器可验的门。
//! - **三要素驳回**（[`RejectReason`]）：驳回必须带「哪条没过 +
//!   差多少 + 怎么改」三要素，空洞驳回（"不符合规范"）非法。
//! - **时限承诺**（[`ReviewSla`]）：审核有时限，超时升级
//!   （[`Escalation`]）——不让人等无限期。
//! - **标准版本化**（[`REVIEW_STANDARD_VERSION`]）：审核标准自己
//!   带版本号，标准漂移可追溯（包过审时记录当时标准版本）。
//!
//! # 降级矩阵（锚点）
//!
//! | 故障 | 处置 |
//! |---|---|
//! | 未过三查 | 驳回 + 三要素理由 |
//! | 审核超时 | 升级（提级处理） |
//! | 标准漂移 | 版本化追溯 |
//!
//! # 复杂度
//!
//! O(包)（三查线性）。
//!
//! # 零 panic 面
//!
//! 无索引、无 `unwrap`、无浮点。

use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码（E19 段续：6040 起）
// ---------------------------------------------------------------------------

/// 审核标准诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ReviewCode {
    /// 档案不完整。
    ProfileIncomplete = 6040,
    /// 无障碍硬线未过。
    A11yHardline = 6041,
    /// 性能预算超支。
    BudgetExceeded = 6042,
    /// 驳回理由缺要素。
    RejectMalformed = 6043,
    /// 审核超时（已升级）。
    SlaExceeded = 6044,
    /// 标准版本漂移。
    StandardDrift = 6045,
}

impl ReviewCode {
    /// 稳定诊断号。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 中文说明。
    pub fn text(self) -> &'static str {
        match self {
            ReviewCode::ProfileIncomplete => "档案完整性未过",
            ReviewCode::A11yHardline => "无障碍硬线未过",
            ReviewCode::BudgetExceeded => "性能预算超支",
            ReviewCode::RejectMalformed => "驳回理由缺三要素",
            ReviewCode::SlaExceeded => "审核超时已升级",
            ReviewCode::StandardDrift => "审核标准版本漂移",
        }
    }

    /// 是否阻断级。
    pub fn blocking(self) -> bool {
        matches!(
            self,
            ReviewCode::ProfileIncomplete | ReviewCode::A11yHardline | ReviewCode::BudgetExceeded
        )
    }
}

// ---------------------------------------------------------------------------
// 二、标准版本
// ---------------------------------------------------------------------------

/// 当前审核标准版本。
pub const REVIEW_STANDARD_VERSION: u32 = 1;

/// 审核时限（分钟；超时升级）。
pub const SLA_MINUTES: u32 = 24 * 60;

// ---------------------------------------------------------------------------
// 三、三查
// ---------------------------------------------------------------------------

/// 单查结果。
#[derive(Clone, Copy, Debug)]
pub struct CheckOutcome {
    /// 是否通过。
    pub passed: bool,
    /// 实测值（千分比或计数，量纲由查项自定）。
    pub measured: u32,
    /// 阈值。
    pub threshold: u32,
}

/// 三查结果集。
#[derive(Clone, Copy, Debug)]
pub struct ReviewChecklist {
    /// 档案完整性（必填域齐备率，千分比 1000=满）。
    pub completeness: CheckOutcome,
    /// 无障碍硬线（对比度最低值，千分比；阈值 450 = 4.5:1）。
    pub a11y_contrast: CheckOutcome,
    /// 性能预算（加载耗时上界，毫秒；阈值 2000）。
    pub budget_ms: CheckOutcome,
    /// 过审时绑定的标准版本。
    pub standard_version: u32,
}

impl ReviewChecklist {
    /// 三查是否全过。
    pub fn all_passed(&self) -> bool {
        self.completeness.passed && self.a11y_contrast.passed && self.budget_ms.passed
    }

    /// 首个未过查项（None = 全过）。
    pub fn first_fail(&self) -> Option<ReviewCode> {
        if !self.completeness.passed {
            Some(ReviewCode::ProfileIncomplete)
        } else if !self.a11y_contrast.passed {
            Some(ReviewCode::A11yHardline)
        } else if !self.budget_ms.passed {
            Some(ReviewCode::BudgetExceeded)
        } else {
            None
        }
    }

    /// 标准版本漂移判定（过审记录 vs 当前标准）。
    pub fn drifted(&self) -> bool {
        self.standard_version != REVIEW_STANDARD_VERSION
    }
}

// ---------------------------------------------------------------------------
// 四、三要素驳回
// ---------------------------------------------------------------------------

/// 驳回三要素（缺一非法）。
#[derive(Clone, Debug)]
pub struct RejectReason {
    /// 要素一：哪条没过。
    pub which: ReviewCode,
    /// 要素二：差多少（实测 - 阈值，带符号量纲由查项自定）。
    pub gap_text: &'static str,
    /// 要素三：怎么改（可执行建议）。
    pub howto: &'static str,
}

impl RejectReason {
    /// 三要素是否齐备。
    pub fn complete(&self) -> bool {
        !self.gap_text.is_empty() && !self.howto.is_empty()
    }

    /// 合法驳回 → 合并文案。
    pub fn render(&self) -> &'static str {
        if self.complete() {
            self.howto
        } else {
            "驳回理由不完整"
        }
    }
}

// ---------------------------------------------------------------------------
// 五、时限与升级
// ---------------------------------------------------------------------------

/// 升级档。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Escalation {
    /// 无升级。
    None,
    /// 提级给组长。
    ToLead,
    /// 提级给域主。
    ToOwner,
}

impl Escalation {
    /// 档位短名。
    pub const fn label(self) -> &'static str {
        match self {
            Escalation::None => "无",
            Escalation::ToLead => "升级组长",
            Escalation::ToOwner => "升级域主",
        }
    }
}

/// 审核时限追踪。
#[derive(Clone, Copy, Debug)]
pub struct ReviewSla {
    /// 已耗时（分钟）。
    pub elapsed: u32,
}

impl ReviewSla {
    /// 新建。
    pub const fn new() -> Self {
        Self { elapsed: 0 }
    }

    /// 推进时钟。
    pub fn tick(&mut self, minutes: u32) {
        self.elapsed = self.elapsed.saturating_add(minutes);
    }

    /// 是否超时。
    pub fn exceeded(&self) -> bool {
        self.elapsed > SLA_MINUTES
    }

    /// 当前升级档（超时一倍 → 组长；两倍 → 域主）。
    pub fn escalation(&self) -> Escalation {
        if self.elapsed > SLA_MINUTES.saturating_mul(2) {
            Escalation::ToOwner
        } else if self.elapsed > SLA_MINUTES {
            Escalation::ToLead
        } else {
            Escalation::None
        }
    }
}

// ---------------------------------------------------------------------------
// 六、审核记录与审核器
// ---------------------------------------------------------------------------

/// 审核单。
#[derive(Clone, Debug)]
pub struct ReviewTicket {
    /// 包 id。
    pub pkg: u32,
    /// 三查结果。
    pub checklist: ReviewChecklist,
    /// 驳回理由（全过时 None）。
    pub reject: Option<RejectReason>,
    /// 时限追踪。
    pub sla: ReviewSla,
}

/// 审核器。
pub struct Reviewer {
    tickets: Vec<ReviewTicket>,
}

impl Reviewer {
    /// 新建。
    pub fn new() -> Self {
        Self {
            tickets: Vec::new(),
        }
    }

    /// 提交审核单：校验驳回合法性后入册。
    pub fn submit(&mut self, t: ReviewTicket) -> Result<(), ReviewCode> {
        if t.checklist.drifted() {
            return Err(ReviewCode::StandardDrift);
        }
        if let Some(r) = &t.reject {
            if !r.complete() {
                return Err(ReviewCode::RejectMalformed);
            }
            // 有驳回理由时必须确有未过项。
            if t.checklist.first_fail().is_none() {
                return Err(ReviewCode::RejectMalformed);
            }
        }
        self.tickets.push(t);
        Ok(())
    }

    /// 全过票数。
    pub fn approved_count(&self) -> usize {
        self.tickets
            .iter()
            .filter(|t| t.checklist.all_passed() && t.reject.is_none())
            .count()
    }

    /// 超时票的升级档列表。
    pub fn overdue_escalations(&self) -> Vec<(u32, Escalation)> {
        self.tickets
            .iter()
            .filter(|t| t.sla.exceeded())
            .map(|t| (t.pkg, t.sla.escalation()))
            .collect()
    }

    /// 在册票数。
    pub fn len(&self) -> usize {
        self.tickets.len()
    }

    /// 空判定。
    pub fn is_empty(&self) -> bool {
        self.tickets.is_empty()
    }

    /// 按包查票。
    pub fn ticket_for(&self, pkg: u32) -> Option<&ReviewTicket> {
        self.tickets.iter().find(|t| t.pkg == pkg)
    }
}

impl Default for Reviewer {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 七、判据
// ---------------------------------------------------------------------------

/// 审核标准判据集。
pub fn run_review_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("svstar2-ver10c_review");

    let pass_all = ReviewChecklist {
        completeness: CheckOutcome {
            passed: true,
            measured: 1000,
            threshold: 1000,
        },
        a11y_contrast: CheckOutcome {
            passed: true,
            measured: 700,
            threshold: 450,
        },
        budget_ms: CheckOutcome {
            passed: true,
            measured: 1200,
            threshold: 2000,
        },
        standard_version: REVIEW_STANDARD_VERSION,
    };

    // 1. 三查全过可过审。
    set.add("6040-三查全过", pass_all.all_passed(), "");

    // 2. 单查不过即首败定位。
    let mut fail_a11y = pass_all;
    fail_a11y.a11y_contrast = CheckOutcome {
        passed: false,
        measured: 300,
        threshold: 450,
    };
    set.add(
            "6041-首败定位无障碍",
            fail_a11y.first_fail() == Some(ReviewCode::A11yHardline),
            "",
        );

    // 3. 三要素驳回合法。
    let reason = RejectReason {
        which: ReviewCode::BudgetExceeded,
        gap_text: "超 300ms",
        howto: "削减首屏资产或启用懒加载",
    };
    set.add("6042-三要素齐备", reason.complete() && reason.render() != "驳回理由不完整", "");

    // 4. 空洞驳回被拒。
    let hollow = RejectReason {
        which: ReviewCode::A11yHardline,
        gap_text: "",
        howto: "",
    };
    let mut rv = Reviewer::new();
    let mut bad_ticket = ReviewTicket {
        pkg: 1,
        checklist: fail_a11y,
        reject: Some(hollow),
        sla: ReviewSla::new(),
    };
    // fail_a11y 有首败，但理由空洞 → 拒。
    let hollow_rejected = rv.submit(bad_ticket).err() == Some(ReviewCode::RejectMalformed);
    set.add("6043-空洞驳回拒绝", hollow_rejected, "");

    // 5. 标准漂移拦截。
    let mut drifted = pass_all;
    drifted.standard_version = REVIEW_STANDARD_VERSION + 9;
    bad_ticket = ReviewTicket {
        pkg: 2,
        checklist: drifted,
        reject: None,
        sla: ReviewSla::new(),
    };
    let drift_rejected = rv.submit(bad_ticket).err() == Some(ReviewCode::StandardDrift);
    set.add("6044-标准漂移拦截", drift_rejected, "");

    // 6. 超时升级档。
    let mut sla = ReviewSla::new();
    sla.tick(SLA_MINUTES + 1);
    let escalated = sla.escalation() == Escalation::ToLead;
    sla.tick(SLA_MINUTES + 1);
    let double = sla.escalation() == Escalation::ToOwner;
    set.add("6045-超时升级", escalated && double, "");

    // 7. 合法过审单入册。
    let good_ticket = ReviewTicket {
        pkg: 3,
        checklist: pass_all,
        reject: None,
        sla: ReviewSla::new(),
    };
    let accepted = rv.submit(good_ticket).is_ok() && rv.approved_count() == 1;
    set.add("6046-合法过审入册", accepted, "");

    set
}
