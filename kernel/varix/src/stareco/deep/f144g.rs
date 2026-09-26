//! 深化层四 · F144 「Crafted for VARIX」徽标（2026-09-27 深化批次四 · g 层）。
//!
//! 申请表校验器、授予/撤销批量审计（计数对账）、展示合规快照模型、
//! 年度复核聚合、争端处理流程状态机。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 申请表校验：应用名/联系方式/证据三件（自测截图说明+判据对照）
// ---------------------------------------------------------------------------

pub struct Application {
    pub app: &'static str,
    pub contact: &'static str,
    pub evidence_screens: u32,
    pub criteria_refs: u32,
}

impl Application {
    /// 校验：名称非空 ≤32 字节、联系方式含 @、证据 ≥2 张、判据对照 ≥4 条。
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.app.is_empty() || self.app.len() > 32 {
            return Err("应用名空或超 32 字节");
        }
        if !self.contact.contains('@') {
            return Err("联系方式须为邮箱形态");
        }
        if self.evidence_screens < 2 {
            return Err("自测截图不足 2 张");
        }
        if self.criteria_refs < 4 {
            return Err("判据对照不足 4 条（四查各一）");
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 授予/撤销批量审计：事件账与在册数对账（漂移即红）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum BadgeEvent {
    Granted,
    Revoked,
}

/// 审计：active = granted - revoked（撤销不可超授予）。
pub fn badge_audit(events: &[(u32, BadgeEvent)]) -> Result<usize, &'static str> {
    let mut active: alloc::vec::Vec<(u32, i32)> = alloc::vec::Vec::new();
    for (id, ev) in events {
        let slot = active.iter_mut().find(|e| e.0 == *id);
        match ev {
            BadgeEvent::Granted => match slot {
                Some(e) => e.1 += 1,
                None => active.push((*id, 1)),
            },
            BadgeEvent::Revoked => match slot {
                Some(e) if e.1 > 0 => e.1 -= 1,
                _ => return Err("撤销多于授予：账目矛盾"),
            },
        }
    }
    Ok(active.iter().filter(|e| e.1 > 0).count())
}

// ---------------------------------------------------------------------------
// 展示合规快照：截图说明必须含位置/尺寸/底色三字段
// ---------------------------------------------------------------------------

pub struct ShowcaseSnapshot {
    pub app: &'static str,
    pub has_position: bool,
    pub has_size: bool,
    pub has_bg: bool,
}

pub fn snapshot_gaps(snaps: &[ShowcaseSnapshot]) -> alloc::vec::Vec<&'static str> {
    snaps
        .iter()
        .filter(|s| !(s.has_position && s.has_size && s.has_bg))
        .map(|s| s.app)
        .collect()
}

// ---------------------------------------------------------------------------
// 年度复核聚合：active/grace/revoked 三态计数
// ---------------------------------------------------------------------------

pub struct YearlyRow {
    pub app: &'static str,
    pub state: u8, // 0=active 1=grace 2=revoked
}

pub struct YearlySummary {
    pub active: usize,
    pub grace: usize,
    pub revoked: usize,
}

pub fn yearly_summary(rows: &[YearlyRow]) -> YearlySummary {
    YearlySummary {
        active: rows.iter().filter(|r| r.state == 0).count(),
        grace: rows.iter().filter(|r| r.state == 1).count(),
        revoked: rows.iter().filter(|r| r.state == 2).count(),
    }
}

// ---------------------------------------------------------------------------
// 争端流程：Dispute→Mediation→{Resolved|Escalated(转 F148 治理)}
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DisputeStage {
    Dispute,
    Mediation,
    Resolved,
    Escalated,
}

pub fn dispute_transition(from: DisputeStage, to: DisputeStage) -> Result<DisputeStage, &'static str> {
    let legal = match (from, to) {
        (DisputeStage::Dispute, DisputeStage::Mediation)
        | (DisputeStage::Mediation, DisputeStage::Resolved)
        | (DisputeStage::Mediation, DisputeStage::Escalated) => true,
        _ => false,
    };
    if legal {
        Ok(to)
    } else {
        Err("非法争端迁移")
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub const F144G_TAG: &str = "stareco-F144-deep4";

pub fn run_f144_deep4_checks() -> CheckSet {
    let mut set = CheckSet::new(F144G_TAG);

    // 申请表
    let ok = Application { app: "files-app", contact: "a@b", evidence_screens: 3, criteria_refs: 4 };
    set.add("f144g apply ok", ok.validate().is_ok(), "四件齐放行");
    let long_name = Application { app: "a-very-long-application-name-exceeding", contact: "a@b", evidence_screens: 3, criteria_refs: 4 };
    set.add("f144g name long", long_name.validate().is_err(), "名称超长拒绝");
    let thin = Application { app: "x", contact: "a@b", evidence_screens: 1, criteria_refs: 4 };
    set.add("f144g evidence thin", thin.validate().is_err(), "截图不足拒绝");
    set.add(
        "f144g criteria thin",
        Application { app: "x", contact: "a@b", evidence_screens: 2, criteria_refs: 3 }.validate().is_err(),
        "判据对照不足拒绝",
    );

    // 批量审计
    let events = [
        (1u32, BadgeEvent::Granted),
        (2, BadgeEvent::Granted),
        (2, BadgeEvent::Revoked),
        (3, BadgeEvent::Granted),
    ];
    set.add("f144g audit", badge_audit(&events) == Ok(2), "授予 3 撤 1 → 在册 2");
    let bad = [(1u32, BadgeEvent::Revoked)];
    set.add("f144g audit ghost revoke", badge_audit(&bad).is_err(), "撤销未授予项拒绝");

    // 展示快照
    let snaps = [
        ShowcaseSnapshot { app: "ok-app", has_position: true, has_size: true, has_bg: true },
        ShowcaseSnapshot { app: "lazy-app", has_position: true, has_size: false, has_bg: true },
    ];
    set.add("f144g snapshot", snapshot_gaps(&snaps) == alloc::vec!["lazy-app"], "缺字段点名");

    // 年度聚合
    let rows = [
        YearlyRow { app: "a", state: 0 },
        YearlyRow { app: "b", state: 0 },
        YearlyRow { app: "c", state: 1 },
        YearlyRow { app: "d", state: 2 },
    ];
    let s = yearly_summary(&rows);
    set.add(
        "f144g yearly",
        s.active == 2 && s.grace == 1 && s.revoked == 1,
        "三态计数",
    );

    // 争端流程
    set.add(
        "f144g dispute path",
        dispute_transition(DisputeStage::Dispute, DisputeStage::Mediation).is_ok()
            && dispute_transition(DisputeStage::Mediation, DisputeStage::Resolved).is_ok(),
        "调解-解决走通",
    );
    set.add(
        "f144g dispute escalate",
        dispute_transition(DisputeStage::Mediation, DisputeStage::Escalated).is_ok(),
        "调解不成转治理",
    );
    set.add(
        "f144g dispute skip",
        dispute_transition(DisputeStage::Dispute, DisputeStage::Resolved).is_err(),
        "跳过调解拒绝",
    );

    set
}

#[cfg(test)]
mod deep4_tests {
    use super::*;

    #[test]
    fn audit_multiple_grants() {
        let events = [
            (1u32, BadgeEvent::Granted),
            (1, BadgeEvent::Granted),
            (1, BadgeEvent::Revoked),
        ];
        assert_eq!(badge_audit(&events), Ok(1)); // 两授一撤仍有一枚
    }

    #[test]
    fn dispute_escalate_only_from_mediation() {
        assert!(dispute_transition(DisputeStage::Dispute, DisputeStage::Escalated).is_err());
    }
}
