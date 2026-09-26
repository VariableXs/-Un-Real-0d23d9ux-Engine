//! 深化层五 · F148 社区规则与治理（2026-09-27 深化批次五 · h 层装配）。
//!
//! 装配与跨域接口件：举报 → 治理立案的移交数据（F134 联动深链）、
//! 案例公示页行（脱敏口径：当事人代号化）、治理指标卡装配。

use super::f148g::{median_days, CaseStage};
use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 移交数据：举报流水线结案（恶意）→ 治理立案参数
// ---------------------------------------------------------------------------

pub struct HandoffCase {
    pub report_id: u32,
    pub evidence_refs: alloc::vec::Vec<u32>,
    pub rule_cited: &'static str,
}

/// 移交门：至少一条证据 + 引用具体规则条款（不引条款不立案）。
pub fn handoff_valid(h: &HandoffCase) -> Result<(), &'static str> {
    if h.evidence_refs.is_empty() {
        return Err("无证据不立案：恶意指控必须带证");
    }
    if h.rule_cited.trim().is_empty() {
        return Err("未引用条款不立案：规则先于裁决");
    }
    Ok(())
}

/// 深链：举报 → 治理案件页参数。
pub fn handoff_link(report_id: u32, case_id: u32) -> [u8; 32] {
    let mut out = [0u8; 32];
    let prefix = b"gov://case/";
    out[..11].copy_from_slice(prefix);
    let s = alloc::format!("{}/{}", report_id, case_id);
    let b = s.as_bytes();
    let take = b.len().min(32 - 11);
    out[11..11 + take].copy_from_slice(&b[..take]);
    out
}

// ---------------------------------------------------------------------------
// 公示页行：案例 → 公示行（当事人代号化脱敏：user-A 形态）
// ---------------------------------------------------------------------------

pub struct PublicCaseRow {
    pub case_id: u32,
    pub stage_label: &'static str,
    pub party_label: &'static str,
}

/// 脱敏：真实名不出现在公示页（公开的是案件不是人）。
pub fn public_rows(
    cases: &[(u32, CaseStage, &'static str)],
) -> alloc::vec::Vec<PublicCaseRow> {
    cases
        .iter()
        .enumerate()
        .map(|(i, (id, stage, _real_name))| PublicCaseRow {
            case_id: *id,
            stage_label: match stage {
                CaseStage::Opened => "已立案",
                CaseStage::Reviewing => "复核中",
                CaseStage::Ruled => "已裁决",
                CaseStage::Executing => "执行中",
                CaseStage::Closed => "已结案",
            },
            party_label: alloc::format!("user-{}", char::from(b'A' + i as u8 % 26))
                .leak() as &'static str,
        })
        .collect()
}

// ---------------------------------------------------------------------------
// 治理指标卡：结案天数序列 + 总案数 → 三张卡（中位/结案率/逾期）
// ---------------------------------------------------------------------------

pub struct GovMetricCards {
    pub median_close_days: u32,
    pub close_rate_per_mille: u32,
    pub overdue_count: usize,
}

pub fn metric_cards(
    close_days: alloc::vec::Vec<u32>,
    total: usize,
    closed: usize,
    overdue: usize,
) -> GovMetricCards {
    GovMetricCards {
        median_close_days: median_days(close_days),
        close_rate_per_mille: if total == 0 {
            0
        } else {
            closed as u32 * 1000 / total as u32
        },
        overdue_count: overdue,
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub const F148H_TAG: &str = "stareco-F148-deep5";

pub fn run_f148_deep5_checks() -> CheckSet {
    let mut set = CheckSet::new(F148H_TAG);

    // 移交门
    let ok = HandoffCase {
        report_id: 7,
        evidence_refs: alloc::vec![101, 102],
        rule_cited: "规则 3.2 恶意内容",
    };
    set.add("f148h handoff ok", handoff_valid(&ok).is_ok(), "证据+条款齐放行");
    let no_evidence = HandoffCase { report_id: 8, evidence_refs: alloc::vec![], rule_cited: "3.2" };
    set.add("f148h no evidence", handoff_valid(&no_evidence).is_err(), "无证据拒绝");
    let no_rule = HandoffCase { report_id: 9, evidence_refs: alloc::vec![1], rule_cited: "" };
    set.add("f148h no rule", handoff_valid(&no_rule).is_err(), "未引条款拒绝");

    // 深链
    let link = handoff_link(7, 42);
    set.add(
        "f148h link",
        &link[..11] == b"gov://case/" && link[11..13] == *b"7/",
        "深链形制",
    );

    // 公示行脱敏
    let cases = [
        (1u32, CaseStage::Reviewing, "真实姓名甲"),
        (2, CaseStage::Closed, "真实姓名乙"),
    ];
    let rows = public_rows(&cases);
    set.add(
        "f148h anonymized",
        rows[0].party_label == "user-A" && rows[1].party_label == "user-B",
        "当事人代号化",
    );
    set.add(
        "f148h stages",
        rows[0].stage_label == "复核中" && rows[1].stage_label == "已结案",
        "阶段人话",
    );

    // 指标卡
    let cards = metric_cards(alloc::vec![9, 3, 7], 10, 8, 1);
    set.add(
        "f148h cards",
        cards.median_close_days == 7 && cards.close_rate_per_mille == 800 && cards.overdue_count == 1,
        "三卡装配",
    );

    set
}

#[cfg(test)]
mod deep5_tests {
    use super::*;

    #[test]
    fn party_labels_wrap() {
        // 27 个案子：代号在 A-Z 内循环（mod 26——不会越界）。
        let cases: alloc::vec::Vec<(u32, CaseStage, &'static str)> =
            (0..27u32).map(|i| (i, CaseStage::Opened, "x")).collect();
        let rows = public_rows(&cases);
        assert_eq!(rows[26].party_label, "user-A");
    }
}
