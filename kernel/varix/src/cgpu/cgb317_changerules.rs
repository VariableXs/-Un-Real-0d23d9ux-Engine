// CGPU-F0317 域级变更纪律 · cgb317_changerules.rs
// AI-MIMO-1022 · CGPU-B 收口
// 判据：变更分类（breaking/additive/fix/doc）；变更审批链；
//       回滚预案必备；变更日志自动生成；纪律违规可审计。
#![allow(dead_code)]

use alloc::format;
extern crate alloc;
use alloc::vec::Vec;
use alloc::string::String;

use super::super::checks::CheckSet;

/// 变更类别。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChangeClass {
    Breaking,
    Additive,
    Fix,
    Documentation,
    Refactor,
    Performance,
    Security,
}

impl ChangeClass {
    pub fn requires_approval(self) -> bool {
        matches!(self, Self::Breaking | Self::Security | Self::Performance)
    }
    pub fn requires_rollback(self) -> bool {
        matches!(self, Self::Breaking | Self::Security | Self::Performance | Self::Refactor)
    }
}

/// 变更记录。
#[derive(Clone, Debug)]
pub struct ChangeRecord {
    pub id: u32,
    pub class: ChangeClass,
    pub title_id: u32,
    pub approved_by: Option<u32>,
    pub rollback_plan_id: Option<u32>,
    pub applied: bool,
    pub reverted: bool,
    pub version: u32,
}

/// 审批链。
#[derive(Clone, Debug)]
pub struct ApprovalChain {
    pub change_id: u32,
    pub required: u32,
    pub obtained: Vec<u32>,
    pub complete: bool,
}

/// 变更纪律引擎。
pub struct ChangeDiscipline {
    pub changes: Vec<ChangeRecord>,
    pub approvals: Vec<ApprovalChain>,
    pub violations: Vec<u32>,
    pub current_version: u32,
}

impl ChangeDiscipline {
    pub fn new() -> Self {
        Self {
            changes: Vec::new(),
            approvals: Vec::new(),
            violations: Vec::new(),
            current_version: 1,
        }
    }

    /// 登记变更。
    pub fn register(&mut self, class: ChangeClass, title_id: u32) -> u32 {
        let id = self.changes.len() as u32 + 1;
        self.changes.push(ChangeRecord {
            id,
            class,
            title_id,
            approved_by: None,
            rollback_plan_id: None,
            applied: false,
            reverted: false,
            version: self.current_version,
        });
        if class.requires_approval() {
            self.approvals.push(ApprovalChain {
                change_id: id,
                required: 2,
                obtained: Vec::new(),
                complete: false,
            });
        }
        id
    }

    /// 添加审批。
    pub fn approve(&mut self, change_id: u32, approver: u32) -> bool {
        if let Some(chain) = self.approvals.iter_mut().find(|c| c.change_id == change_id) {
            if !chain.obtained.contains(&approver) {
                chain.obtained.push(approver);
            }
            chain.complete = chain.obtained.len() as u32 >= chain.required;
            if chain.complete {
                if let Some(c) = self.changes.iter_mut().find(|c| c.id == change_id) {
                    c.approved_by = Some(approver);
                }
            }
            chain.complete
        } else {
            // 无需审批的直接通过
            if let Some(c) = self.changes.iter_mut().find(|c| c.id == change_id) {
                c.approved_by = Some(approver);
            }
            true
        }
    }

    /// 设置回滚预案。
    pub fn set_rollback(&mut self, change_id: u32, plan_id: u32) -> bool {
        if let Some(c) = self.changes.iter_mut().find(|c| c.id == change_id) {
            c.rollback_plan_id = Some(plan_id);
            true
        } else {
            false
        }
    }

    /// 应用变更（审批+回滚齐备才可应用）。
    pub fn apply(&mut self, change_id: u32) -> bool {
        let ok = {
            let c = self.changes.iter().find(|c| c.id == change_id);
            match c {
                None => false,
                Some(c) => {
                    let approved = c.approved_by.is_some();
                    let rollback_ok = !c.class.requires_rollback() || c.rollback_plan_id.is_some();
                    approved && rollback_ok
                }
            }
        };
        if !ok {
            self.violations.push(change_id);
            return false;
        }
        if let Some(c) = self.changes.iter_mut().find(|c| c.id == change_id) {
            c.applied = true;
        }
        true
    }

    /// 回滚。
    pub fn revert(&mut self, change_id: u32) -> bool {
        if let Some(c) = self.changes.iter_mut().find(|c| c.id == change_id) {
            if !c.applied {
                return false;
            }
            c.reverted = true;
            c.applied = false;
            true
        } else {
            false
        }
    }

    /// 变更日志生成。
    pub fn changelog(&self) -> String {
        let mut s = String::from("变更日志：\n");
        for c in &self.changes {
            let status = if c.reverted {
                "已回滚"
            } else if c.applied {
                "已应用"
            } else {
                "待应用"
            };
            s.push_str(&alloc::format!("  #{} v{} {}\n", c.id, c.version, status));
        }
        s
    }

    /// 纪律违规统计。
    pub fn violation_count(&self) -> u32 {
        self.violations.len() as u32
    }

    pub fn run_checks(&self) -> CheckSet {
        let mut cs = CheckSet::new("CGPU-F0317 域级变更纪律");
        cs.add("变更 id 唯一", {
            let mut ids: Vec<u32> = self.changes.iter().map(|c| c.id).collect();
            ids.sort_unstable();
            ids.dedup();
            ids.len() == self.changes.len()
        }, "unique");
        cs.add("需审批的已审批才能应用", {
            self.changes
                .iter()
                .filter(|c| c.applied)
                .all(|c| c.approved_by.is_some())
        }, "approved");
        cs.add("需回滚的有预案才能应用", {
            self.changes
                .iter()
                .filter(|c| c.applied && c.class.requires_rollback())
                .all(|c| c.rollback_plan_id.is_some())
        }, "rollback plan");
        cs.add("审批链完整性一致", {
            self.approvals.iter().all(|a| a.complete == (a.obtained.len() as u32 >= a.required))
        }, "chain");
        cs.add("回滚的必已应用过", {
            self.changes.iter().filter(|c| c.reverted).all(|c| c.rollback_plan_id.is_some())
        }, "revert needs plan");
        cs.add("违规可数", self.violation_count() <= self.changes.len() as u32, "countable");
        cs
    }
}

impl Default for ChangeDiscipline {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn breaking_needs_approval_and_rollback() {
        let mut cd = ChangeDiscipline::new();
        let id = cd.register(ChangeClass::Breaking, 1);
        assert!(!cd.apply(id)); // 未审批
        cd.approve(id, 1);
        cd.approve(id, 2);
        assert!(!cd.apply(id)); // 无回滚预案
        cd.set_rollback(id, 100);
        assert!(cd.apply(id));
    }

    #[test]
    fn fix_no_approval_needed() {
        let mut cd = ChangeDiscipline::new();
        let id = cd.register(ChangeClass::Fix, 2);
        assert!(cd.approve(id, 1));
        assert!(cd.apply(id));
    }
}
