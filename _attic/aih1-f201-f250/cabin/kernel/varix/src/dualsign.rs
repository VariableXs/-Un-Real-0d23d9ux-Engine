//! 评级复核双签制度（WP-305 · B-4302 升级双签——流程审计通过）。
//!
//! MD2 篇 43.2：复核通过则生成候选星卡（评级建议由数据推，人工可改，改动
//! 必须留理由）→ 双人复核（**评级调升类必须第二人复核——降级单签、升级
//! 双签**，防止乐观偏差）→ 目录合入。争议处理：提交者对复核结果有异议走
//! 申诉（附补充证据），申诉记录与处理结论公开——星卡的公信力来自流程的
//! 可见，不只是结论的正确。
//!
//! 零堆纪律：无 Vec/String/Box/format!，定长数组 + 字节串。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 复核动作与签核规则
// ---------------------------------------------------------------------------

/// 复核动作（穷举三态——调升/降级/维持）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ReviewOp {
    /// 评级调升（乐观偏差风险面——必须双签）。
    Upgrade,
    /// 评级降级（保守方向——单签足够）。
    Downgrade,
    /// 维持原级。
    Keep,
}

/// 签核数规则（**B-4302 达标线：降级单签、升级双签**）——调升必须第二人
/// 复核，规则是查表不是裁量。
pub fn signoffs_required(op: ReviewOp) -> u8 {
    match op {
        ReviewOp::Upgrade => 2,
        ReviewOp::Downgrade => 1,
        ReviewOp::Keep => 1,
    }
}

/// 候选星卡（评级建议由数据推，人工可改——改动必须留理由）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Candidate {
    /// 复核动作。
    pub op: ReviewOp,
    /// 人工改动发生（数据推的建议被复核员推翻）。
    pub manual_override: bool,
    /// 改动理由在册（没理由的改动是任性不是复核）。
    pub manual_reason: bool,
}

/// 合入许可判：签核数达规则要求，且人工改动必须带理由——缺一不许合入。
pub fn merge_ok(signs: u8, c: &Candidate) -> bool {
    signs >= signoffs_required(c.op) && (!c.manual_override || c.manual_reason)
}

// ---------------------------------------------------------------------------
// 申诉链
// ---------------------------------------------------------------------------

/// 申诉（提交者对复核结果有异议——附补充证据，记录与结论公开）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Appeal {
    /// 补充证据已附（空口申诉不受理）。
    pub evidence_attached: bool,
    /// 申诉记录与处理结论公开（公信力来自流程可见）。
    pub record_public: bool,
}

impl Appeal {
    /// 申诉受理判：证据与公开缺一不可。
    pub fn handled(&self) -> bool {
        self.evidence_attached && self.record_public
    }
}

// ---------------------------------------------------------------------------
// 流程审计（全链对账）
// ---------------------------------------------------------------------------

/// 审计账行（一笔复核一条账）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AuditLine {
    /// 复核动作。
    pub op: ReviewOp,
    /// 实际签核数。
    pub signs: u8,
    /// 人工改动发生。
    pub manual_override: bool,
    /// 改动理由在册。
    pub manual_reason: bool,
}

/// 流程审计（**B-4302 达标线的账面：流程审计通过**）——逐行对账：每笔签
/// 核数达标、每笔改动留理由，一行违规全账判红。
pub fn audit_ok(lines: &[AuditLine]) -> bool {
    let mut i = 0;
    while i < lines.len() {
        let l = &lines[i];
        if l.signs < signoffs_required(l.op) {
            return false;
        }
        if l.manual_override && !l.manual_reason {
            return false;
        }
        i += 1;
    }
    true
}

// ---------------------------------------------------------------------------
// CheckSet（B-4302 · 4 项）
// ---------------------------------------------------------------------------

pub fn run_dualsign_checks() -> CheckSet {
    let mut set = CheckSet::new("B-4302 升级双签");
    // 1. 降级单签升级双签：签核规则查表——调升无第二签不合入。
    let up = Candidate { op: ReviewOp::Upgrade, manual_override: false, manual_reason: false };
    let down = Candidate { op: ReviewOp::Downgrade, manual_override: false, manual_reason: false };
    let keep = Candidate { op: ReviewOp::Keep, manual_override: false, manual_reason: false };
    set.add(
        "B-4302 降级单签升级双签",
        signoffs_required(ReviewOp::Upgrade) == 2
            && signoffs_required(ReviewOp::Downgrade) == 1
            && signoffs_required(ReviewOp::Keep) == 1
            && !merge_ok(1, &up)
            && merge_ok(2, &up)
            && merge_ok(1, &down)
            && merge_ok(1, &keep),
        "评级调升必须第二人复核——防乐观偏差是查表规则不是裁量（B-4302 达标线）",
    );
    // 2. 改动必留理由：数据推人工可改，无理由改动拒。
    let no_reason = Candidate { op: ReviewOp::Downgrade, manual_override: true, manual_reason: false };
    let with_reason = Candidate { op: ReviewOp::Downgrade, manual_override: true, manual_reason: true };
    set.add(
        "B-4302 改动留理由",
        !merge_ok(1, &no_reason) && merge_ok(1, &with_reason),
        "评级建议由数据推，人工可改但必须留理由——没理由的改动是任性",
    );
    // 3. 申诉链：证据与公开缺一不可。
    let ok = Appeal { evidence_attached: true, record_public: true };
    let no_ev = Appeal { evidence_attached: false, record_public: true };
    let hidden = Appeal { evidence_attached: true, record_public: false };
    set.add(
        "B-4302 申诉链",
        ok.handled() && !no_ev.handled() && !hidden.handled(),
        "附补充证据+记录结论公开——公信力来自流程的可见，不只是结论的正确",
    );
    // 4. 流程审计：全账逐行对账，一行违规全账红。
    let lines = [
        AuditLine { op: ReviewOp::Upgrade, signs: 2, manual_override: true, manual_reason: true },
        AuditLine { op: ReviewOp::Downgrade, signs: 1, manual_override: false, manual_reason: false },
    ];
    let bad_signs = [
        AuditLine { op: ReviewOp::Upgrade, signs: 1, manual_override: false, manual_reason: false },
        lines[1],
    ];
    let bad_reason = [
        AuditLine { op: ReviewOp::Keep, signs: 1, manual_override: true, manual_reason: false },
        lines[1],
    ];
    set.add(
        "B-4302 流程审计",
        audit_ok(&lines) && !audit_ok(&bad_signs) && !audit_ok(&bad_reason),
        "每笔签核达标+每笔改动留理由——一行违规全账判红（流程审计通过）",
    );
    set
}

// ---------------------------------------------------------------------------
// 单元测试（fe18 · 4 项）
// ---------------------------------------------------------------------------

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn fe18_dual_sign_rules() {
        // 查表语义：Upgrade=2 / Downgrade=1 / Keep=1。
        assert_eq!(signoffs_required(ReviewOp::Upgrade), 2);
        assert_eq!(signoffs_required(ReviewOp::Downgrade), 1);
        assert_eq!(signoffs_required(ReviewOp::Keep), 1);
        // 边界：恰好达标合入，差一签拒。
        let up = Candidate { op: ReviewOp::Upgrade, manual_override: false, manual_reason: false };
        assert!(!merge_ok(0, &up));
        assert!(!merge_ok(1, &up));
        assert!(merge_ok(2, &up));
        assert!(merge_ok(3, &up)); // 超额签核只多不少，不拒。
    }

    #[test]
    fn fe18_reason_required() {
        // 改动无理由拒——单签双签都救不回（理由是独立条件不是签核的影子）。
        let bad = Candidate { op: ReviewOp::Downgrade, manual_override: true, manual_reason: false };
        assert!(!merge_ok(1, &bad));
        assert!(!merge_ok(2, &bad));
        // 未改动不需要理由。
        let untouched = Candidate { op: ReviewOp::Keep, manual_override: false, manual_reason: false };
        assert!(merge_ok(1, &untouched));
    }

    #[test]
    fn fe18_appeal_chain() {
        // 证据与公开四象限：只有双全才受理。
        assert!(Appeal { evidence_attached: true, record_public: true }.handled());
        assert!(!Appeal { evidence_attached: false, record_public: true }.handled());
        assert!(!Appeal { evidence_attached: true, record_public: false }.handled());
        assert!(!Appeal { evidence_attached: false, record_public: false }.handled());
    }

    #[test]
    fn fe18_audit_ledger() {
        // 全合规账绿；单行违规（少签/无理由改动）全账红。
        let ok_line = AuditLine { op: ReviewOp::Keep, signs: 1, manual_override: false, manual_reason: false };
        let good = [
            ok_line,
            AuditLine { op: ReviewOp::Upgrade, signs: 2, manual_override: false, manual_reason: false },
        ];
        assert!(audit_ok(&good));
        let less_signs = [AuditLine { op: ReviewOp::Upgrade, signs: 1, manual_override: false, manual_reason: false }, ok_line];
        assert!(!audit_ok(&less_signs));
        let no_reason = [AuditLine { op: ReviewOp::Upgrade, signs: 2, manual_override: true, manual_reason: false }, ok_line];
        assert!(!audit_ok(&no_reason));
        // 空账无违规——审计通过（没有复核发生也是合规状态）。
        let empty: [AuditLine; 0] = [];
        assert!(audit_ok(&empty));
    }
}
