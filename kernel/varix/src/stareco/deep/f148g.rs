//! 深化层四 · F148 社区规则与治理（2026-09-27 深化批次四 · g 层）。
//!
//! 案例全生命周期视图（阶段×时限×参与方）、裁决执行追踪、社区选举
//! 核（提名→计票→就任）、调解步骤模板、治理指标（中位结案天数）。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 案例生命周期视图：开案→复核→裁决→执行→结案；逐段时限点名逾期
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CaseStage {
    Opened,
    Reviewing,
    Ruled,
    Executing,
    Closed,
}

pub struct CaseView {
    pub id: u32,
    pub stage: CaseStage,
    pub stage_since: u32,
}

/// 各段时限：复核 7 / 裁决 14 / 执行 30。
pub fn case_overdue(c: &CaseView, today: u32) -> Option<&'static str> {
    let limit = match c.stage {
        CaseStage::Opened => return None,
        CaseStage::Reviewing => 7,
        CaseStage::Ruled => 14,
        CaseStage::Executing => 30,
        CaseStage::Closed => return None,
    };
    if today > c.stage_since + limit {
        Some("阶段逾期")
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// 裁决执行追踪：履行/未履行/逾期三态
// ---------------------------------------------------------------------------

pub struct Ruling {
    pub case_id: u32,
    pub due_day: u32,
    pub fulfilled: Option<bool>,
}

pub fn ruling_state(r: &Ruling, today: u32) -> u8 {
    match r.fulfilled {
        Some(true) => 0,    // 履行
        Some(false) => 2,   // 违约（明确未履行）
        None if today > r.due_day => 2, // 逾期未履行
        None => 1,          // 履行中
    }
}

// ---------------------------------------------------------------------------
// 社区选举核：提名 → 计票（多票无效）→ 席位
// ---------------------------------------------------------------------------

pub struct Election {
    pub seats: usize,
    pub candidates: alloc::vec::Vec<&'static str>,
    /// 逐票（候选人序号）；对同一候选人重复投票的选票作废。
    pub ballots: alloc::vec::Vec<alloc::vec::Vec<usize>>,
}

impl Election {
    /// 计票：越界/重复候选人票作废（审计计数如实返回）。
    pub fn tally(&self) -> (alloc::vec::Vec<(usize, u32)>, usize) {
        let mut counts: alloc::vec::Vec<(usize, u32)> =
            (0..self.candidates.len()).map(|i| (i, 0)).collect();
        let mut invalid = 0usize;
        for b in &self.ballots {
            let mut seen: alloc::vec::Vec<usize> = alloc::vec::Vec::new();
            let mut ok = true;
            for c in b {
                if *c >= self.candidates.len() || seen.contains(c) {
                    ok = false;
                    break;
                }
                seen.push(*c);
            }
            if !ok {
                invalid += 1;
                continue;
            }
            // 连选制：每张有效票给所选候选人各 +1。
            for c in seen {
                counts[c].1 += 1;
            }
        }
        counts.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        (counts, invalid)
    }

    /// 就任名单：票数降序前 seats 席（平局序号小者）。
    pub fn winners(&self) -> alloc::vec::Vec<&'static str> {
        let (counts, _) = self.tally();
        counts
            .into_iter()
            .take(self.seats)
            .map(|(i, _)| self.candidates[i])
            .collect()
    }
}

// ---------------------------------------------------------------------------
// 调解步骤模板：按争端类型给步骤序列（确定性）
// ---------------------------------------------------------------------------

pub fn mediation_steps(kind: &str) -> alloc::vec::Vec<&'static str> {
    match kind {
        "credit" => alloc::vec![
            "双方陈述",
            "证据核对",
            "信用修复方案",
            "公示 7 天",
        ],
        "conduct" => alloc::vec![
            "双方陈述",
            "证人问询",
            "行为裁定",
            "申诉告知",
        ],
        _ => alloc::vec!["双方陈述", "证据核对", "裁定建议", "申诉告知"],
    }
}

// ---------------------------------------------------------------------------
// 治理指标：中位结案天数（奇偶两口径）+ 结案率
// ---------------------------------------------------------------------------

pub fn median_days(mut days: alloc::vec::Vec<u32>) -> u32 {
    days.sort_unstable();
    let n = days.len();
    if n == 0 {
        return 0;
    }
    if n % 2 == 1 {
        days[n / 2]
    } else {
        (days[n / 2 - 1] + days[n / 2]) / 2
    }
}

pub fn close_rate(total: usize, closed: usize) -> u32 {
    if total == 0 {
        return 0;
    }
    closed as u32 * 1000 / total as u32
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub const F148G_TAG: &str = "stareco-F148-deep4";

pub fn run_f148_deep4_checks() -> CheckSet {
    let mut set = CheckSet::new(F148G_TAG);

    // 案例逾期
    let reviewing = CaseView { id: 1, stage: CaseStage::Reviewing, stage_since: 100 };
    set.add(
        "f148g case overdue",
        case_overdue(&reviewing, 107).is_none() && case_overdue(&reviewing, 108).is_some(),
        "复核 7 天线",
    );
    let closed = CaseView { id: 2, stage: CaseStage::Closed, stage_since: 0 };
    set.add("f148g closed never overdue", case_overdue(&closed, 9999).is_none(), "结案不判逾期");

    // 裁决执行
    let r1 = Ruling { case_id: 1, due_day: 100, fulfilled: Some(true) };
    let r2 = Ruling { case_id: 2, due_day: 100, fulfilled: None };
    let r3 = Ruling { case_id: 3, due_day: 100, fulfilled: Some(false) };
    set.add(
        "f148g ruling states",
        ruling_state(&r1, 101) == 0 && ruling_state(&r2, 100) == 1 && ruling_state(&r3, 101) == 2,
        "履行中/违约三态",
    );
    set.add("f148g ruling overdue", ruling_state(&r2, 200) == 2, "逾期转违约");

    // 选举
    let e = Election {
        seats: 2,
        candidates: alloc::vec!["甲", "乙", "丙"],
        ballots: alloc::vec![
            alloc::vec![0, 1],
            alloc::vec![1, 0],
            alloc::vec![2],
            alloc::vec![0, 0], // 重复作废
            alloc::vec![9],    // 越界作废
        ],
    };
    let (counts, invalid) = e.tally();
    set.add("f148g invalid", invalid == 2, "两票作废");
    set.add(
        "f148g tally top",
        counts[0] == (0, 2) && counts[1] == (1, 2) && counts[2] == (2, 1),
        "甲乙各 2 票平局序号定序",
    );
    set.add("f148g winners", e.winners() == alloc::vec!["甲", "乙"], "两席就任");

    // 调解模板
    set.add(
        "f148g mediation",
        mediation_steps("conduct")[2] == "行为裁定" && mediation_steps("??")[0] == "双方陈述",
        "分型模板+缺省模板",
    );

    // 治理指标
    set.add("f148g median odd", median_days(alloc::vec![9, 3, 7]) == 7, "奇数中位");
    set.add("f148g median even", median_days(alloc::vec![8, 2, 4, 6]) == 5, "偶数取中均");
    set.add("f148g median empty", median_days(alloc::vec![]) == 0, "空哨兵");
    set.add("f148g close rate", close_rate(8, 6) == 750, "结案率 750‰");

    set
}

#[cfg(test)]
mod deep4_tests {
    use super::*;

    #[test]
    fn election_seat_cap() {
        let e = Election {
            seats: 1,
            candidates: alloc::vec!["a", "b"],
            ballots: alloc::vec![alloc::vec![1], alloc::vec![0]],
        };
        // 平局票数按序号定序 → "a" 先就任。
        assert_eq!(e.winners(), alloc::vec!["a"]);
    }

    #[test]
    fn case_executing_deadline() {
        let c = CaseView { id: 9, stage: CaseStage::Executing, stage_since: 0 };
        assert!(case_overdue(&c, 30).is_none());
        assert!(case_overdue(&c, 31).is_some());
    }
}
