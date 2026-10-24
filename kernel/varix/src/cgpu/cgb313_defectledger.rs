// CGPU-F0313 域级缺陷账本清账 · cgb313_defectledger.rs
// AI-MIMO-1022 · CGPU-B 收口
// 判据：缺陷全生命周期（发现→分诊→修复→验证→关闭）账本完备；
//       超期缺陷告警；清账率报告；根因分类统计。
#![allow(dead_code)]

extern crate alloc;
use alloc::vec::Vec;
use alloc::string::String;

use super::super::checks::CheckSet;

/// 缺陷状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DefectState {
    New,
    Triaged,
    Assigned,
    Fixing,
    Fixed,
    Verified,
    Closed,
    Deferred,
}

/// 根因分类。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RootCause {
    LogicError,
    BoundaryError,
    Concurrency,
    Memory,
    ApiMisuse,
    SpecGap,
    Tooling,
    Unknown,
}

/// 缺陷条目。
#[derive(Clone, Debug)]
pub struct Defect {
    pub id: u32,
    pub state: DefectState,
    pub root_cause: RootCause,
    pub severity: u8,
    pub opened_at: u64,
    pub closed_at: Option<u64>,
    pub owner: u32,
    pub evidence_id: u32,
}

/// 清账报告。
#[derive(Clone, Copy, Default, Debug)]
pub struct ClearanceReport {
    pub total: u32,
    pub closed: u32,
    pub open: u32,
    pub overdue: u32,
    pub clearance_rate_q8: u16,
    pub mean_age_hours: u32,
}

/// 缺陷账本。
pub struct DefectLedger {
    pub defects: Vec<Defect>,
    pub report: ClearanceReport,
    pub sla_hours: u64,
    pub now: u64,
}

impl DefectLedger {
    pub fn new(sla_hours: u64, now: u64) -> Self {
        Self {
            defects: Vec::new(),
            report: ClearanceReport::default(),
            sla_hours,
            now,
        }
    }

    pub fn open_defect(&mut self, root_cause: RootCause, severity: u8, owner: u32) -> u32 {
        let id = self.defects.len() as u32 + 1;
        self.defects.push(Defect {
            id,
            state: DefectState::New,
            root_cause,
            severity,
            opened_at: self.now,
            closed_at: None,
            owner,
            evidence_id: 0,
        });
        id
    }

    /// 状态迁移（合法链）。
    pub fn transition(&mut self, id: u32, new_state: DefectState) -> bool {
        let legal = |from: DefectState, to: DefectState| -> bool {
            matches!(
                (from, to),
                (DefectState::New, DefectState::Triaged)
                    | (DefectState::Triaged, DefectState::Assigned)
                    | (DefectState::Assigned, DefectState::Fixing)
                    | (DefectState::Fixing, DefectState::Fixed)
                    | (DefectState::Fixed, DefectState::Verified)
                    | (DefectState::Verified, DefectState::Closed)
                    | (DefectState::Triaged, DefectState::Deferred)
                    | (DefectState::Deferred, DefectState::Triaged)
                    | (DefectState::New, DefectState::Deferred)
            )
        };
        if let Some(d) = self.defects.iter_mut().find(|d| d.id == id) {
            if !legal(d.state, new_state) {
                return false;
            }
            d.state = new_state;
            if new_state == DefectState::Closed {
                d.closed_at = Some(self.now);
            }
            true
        } else {
            false
        }
    }

    /// 生成清账报告。
    pub fn finalize(&mut self) -> ClearanceReport {
        let total = self.defects.len() as u32;
        let closed = self.defects.iter().filter(|d| matches!(d.state, DefectState::Closed)).count() as u32;
        let open = total - closed;
        let overdue = self
            .defects
            .iter()
            .filter(|d| d.closed_at.is_none() && self.now.saturating_sub(d.opened_at) > self.sla_hours * 3600)
            .count() as u32;
        let rate = if total == 0 { 256 } else { ((closed as u64 * 256) / total as u64) as u16 };
        let ages: u64 = self
            .defects
            .iter()
            .map(|d| {
                let end = d.closed_at.unwrap_or(self.now);
                end.saturating_sub(d.opened_at)
            })
            .sum();
        self.report = ClearanceReport {
            total,
            closed,
            open,
            overdue,
            clearance_rate_q8: rate,
            mean_age_hours: if total == 0 { 0 } else { (ages / total as u64 / 3600) as u32 },
        };
        self.report
    }

    /// 根因统计。
    pub fn root_cause_counts(&self) -> Vec<(RootCause, u32)> {
        let mut out = Vec::new();
        let causes = [
            RootCause::LogicError,
            RootCause::BoundaryError,
            RootCause::Concurrency,
            RootCause::Memory,
            RootCause::ApiMisuse,
            RootCause::SpecGap,
            RootCause::Tooling,
            RootCause::Unknown,
        ];
        for c in causes {
            let n = self.defects.iter().filter(|d| d.root_cause == c).count() as u32;
            if n > 0 {
                out.push((c, n));
            }
        }
        out
    }

    pub fn run_checks(&self) -> CheckSet {
        let mut cs = CheckSet::new("CGPU-F0313 缺陷账本清账");
        cs.add("缺陷 id 唯一", {
            let mut ids: Vec<u32> = self.defects.iter().map(|d| d.id).collect();
            ids.sort_unstable();
            ids.dedup();
            ids.len() == self.defects.len()
        }, "unique");
        cs.add("关闭的有时间戳", {
            self.defects.iter().filter(|d| d.state == DefectState::Closed).all(|d| d.closed_at.is_some())
        }, "timestamp");
        cs.add("清账率 ≤ 256", self.report.clearance_rate_q8 <= 256, "q8");
        cs.add("关闭 ≤ 总数", self.report.closed <= self.report.total, "closed ≤ total");
        cs.add("超期可数", self.report.overdue <= self.report.open, "overdue ≤ open");
        cs.add("根因统计总和 = 总数", {
            let sum: u32 = self.root_cause_counts().iter().map(|(_, n)| n).sum();
            sum == self.defects.len() as u32
        }, "root cause sum");
        cs
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lifecycle() {
        let mut l = DefectLedger::new(24, 0);
        let id = l.open_defect(RootCause::LogicError, 2, 1);
        assert!(l.transition(id, DefectState::Triaged));
        assert!(l.transition(id, DefectState::Assigned));
        assert!(l.transition(id, DefectState::Fixing));
        assert!(l.transition(id, DefectState::Fixed));
        assert!(l.transition(id, DefectState::Verified));
        assert!(l.transition(id, DefectState::Closed));
        l.finalize();
        assert_eq!(l.report.closed, 1);
    }

    #[test]
    fn illegal_transition() {
        let mut l = DefectLedger::new(24, 0);
        let id = l.open_defect(RootCause::Memory, 1, 1);
        assert!(!l.transition(id, DefectState::Closed)); // 不能从 New 直接 Closed
    }
}
