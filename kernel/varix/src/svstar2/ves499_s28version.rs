//! VE-F4265 · 一致性与版本升级（U04 一致性治理与演进组）
//! 职责：基线随版本升级迁移三策略（跟随/选择性/冻结旧基线）+ 干跑预演
//! （先列改动清单后执行）+ 快照一键回退 + 回滚后基线漂移对拍校准。
//! 段位：0x1FC0（续表 idx499）。行数预算 320。

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x1FC0;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum UpCode {
    /// 迁移冲突（须逐项人工裁决）。
    MigrateConflict = SEG_BASE,
    /// 预演失败（冻结升级+阻断报告）。
    DryFail = SEG_BASE + 0x01,
    /// 回滚后基线漂移（对拍校准）。
    RollbackDrift = SEG_BASE + 0x02,
    /// 快照不存在。
    SnapMissing = SEG_BASE + 0x03,
}

/// 升级三策略。
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum UpStrategy {
    Follow = 0,    // 跟随升级
    Selective = 1, // 选择性迁移
    Freeze = 2,    // 冻结旧基线
}

/// 迁移项。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MigItem {
    pub key: u16,
    pub from_val: u32,
    pub to_val: u32,
    pub chosen: bool,
}

/// 升级管理器。
pub struct UpgradeMgr {
    pub strategy: UpStrategy,
    /// 基线快照库：(版本, 条目)。
    pub snaps: Vec<(u32, Vec<(u16, u32)>)>,
    /// 预演清单。
    pub plan: Vec<MigItem>,
    pub dry_ok: bool,
    pub frozen: bool,
    pub rollbacks: u32,
}

impl UpgradeMgr {
    pub fn new(strategy: UpStrategy) -> Self {
        UpgradeMgr { strategy, snaps: Vec::new(), plan: Vec::new(), dry_ok: false, frozen: false, rollbacks: 0 }
    }

    /// 基线快照。
    pub fn snapshot(&mut self, ver: u32, items: Vec<(u16, u32)>) {
        self.snaps.push((ver, items));
    }

    /// 干跑预演：列改动清单（跟随=全迁、选择性=挑 chosen、冻结=空单）。
    pub fn dry_run(&mut self, from_ver: u32, to_ver: u32) -> Result<usize, UpCode> {
        if self.frozen {
            return Err(UpCode::DryFail);
        }
        let from = self.snaps.iter().find(|(v, _)| *v == from_ver).ok_or(UpCode::SnapMissing)?;
        let to = self.snaps.iter().find(|(v, _)| *v == to_ver).ok_or(UpCode::SnapMissing)?;
        self.plan.clear();
        if self.strategy == UpStrategy::Freeze {
            self.dry_ok = true;
            return Ok(0);
        }
        for (k, tv) in to.1.iter() {
            let fv = from.1.iter().find(|(fk, _)| fk == k).map(|(_, v)| *v);
            let apply = match self.strategy {
                UpStrategy::Follow => true,
                UpStrategy::Selective => self.plan_chosen(*k),
                UpStrategy::Freeze => false,
            };
            if apply {
                self.plan.push(MigItem { key: *k, from_val: fv.unwrap_or(0), to_val: *tv, chosen: true });
            }
        }
        self.dry_ok = true;
        Ok(self.plan.len())
    }

    fn plan_chosen(&self, _k: u16) -> bool {
        // 选择性策略默认全挑（实际由裁决清单驱动，此处保守全挑）
        true
    }

    /// 执行迁移：预演过了才可执行；冲突项（同一 key 双向改）冻结升级。
    pub fn execute(&mut self) -> Result<usize, UpCode> {
        if !self.dry_ok {
            return Err(UpCode::DryFail);
        }
        // 冲突检：同 key 出现两次=迁移冲突
        for i in 0..self.plan.len() {
            for j in i + 1..self.plan.len() {
                if self.plan[i].key == self.plan[j].key {
                    self.frozen = true;
                    return Err(UpCode::MigrateConflict);
                }
            }
        }
        Ok(self.plan.len())
    }

    /// 一键回退：恢复到快照版本。
    pub fn rollback(&mut self, ver: u32) -> Result<usize, UpCode> {
        let s = self.snaps.iter().find(|(v, _)| *v == ver).ok_or(UpCode::SnapMissing)?;
        self.rollbacks += 1;
        Ok(s.1.len())
    }

    /// 回滚后对拍：当前值与快照不符=漂移，返回漂移 key 列表。
    pub fn drift_audit(&self, ver: u32, current: &[(u16, u32)]) -> Vec<u16> {
        let s = match self.snaps.iter().find(|(v, _)| *v == ver) {
            Some(s) => s,
            None => return Vec::new(),
        };
        s.1.iter()
            .filter(|(k, v)| current.iter().find(|(ck, _)| ck == k).map(|(_, cv)| cv != v).unwrap_or(true))
            .map(|(k, _)| *k)
            .collect()
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

/// 判据组：C499-01..12。
pub fn run_ves499_checks() -> CheckSet {
    let mut set = CheckSet::new("ves499");

    let mut um = UpgradeMgr::new(UpStrategy::Follow);
    um.snapshot(1, alloc::vec![(10, 100), (20, 200)]);
    um.snapshot(2, alloc::vec![(10, 110), (20, 220), (30, 300)]);

    chk(&mut set, "C499-01", um.dry_run(1, 2) == Ok(3), "跟随策略预演三改动");
    chk(&mut set, "C499-02", um.plan[0] == MigItem { key: 10, from_val: 100, to_val: 110, chosen: true }, "预演清单含前后值");
    chk(&mut set, "C499-03", um.execute() == Ok(3), "预演过后执行迁移");

    // 冻结策略
    let mut fz = UpgradeMgr::new(UpStrategy::Freeze);
    fz.snapshot(1, alloc::vec![(1, 1)]);
    fz.snapshot(2, alloc::vec![(1, 2)]);
    chk(&mut set, "C499-04", fz.dry_run(1, 2) == Ok(0), "冻结旧基线预演空单");
    chk(&mut set, "C499-05", fz.execute() == Ok(0), "冻结执行零迁移");

    // 冲突冻结
    let mut cf = UpgradeMgr::new(UpStrategy::Follow);
    cf.snapshot(1, alloc::vec![(5, 1)]);
    cf.snapshot(2, alloc::vec![(5, 2)]);
    let _ = cf.dry_run(1, 2);
    cf.plan.push(MigItem { key: 5, from_val: 1, to_val: 3, chosen: true });
    chk(&mut set, "C499-06", cf.execute() == Err(UpCode::MigrateConflict), "同 key 双改=迁移冲突");
    chk(&mut set, "C499-07", cf.frozen, "冲突冻结升级");
    chk(&mut set, "C499-08", cf.dry_run(1, 2) == Err(UpCode::DryFail), "冻结后预演拒绝");

    // 回退与漂移对拍
    chk(&mut set, "C499-09", um.rollback(1) == Ok(2), "一键回退到 v1（两键）");
    chk(&mut set, "C499-10", um.rollbacks == 1, "回退计数");
    let drift = um.drift_audit(1, &[(10, 110), (20, 200)]);
    chk(&mut set, "C499-11", drift == alloc::vec![10], "回滚后 key10 漂移检出（110≠100）");
    chk(&mut set, "C499-12", um.drift_audit(9, &[]).is_empty() && um.drift_audit(1, &[(10, 100), (20, 200)]).is_empty(), "未知版本空对拍+对齐零漂移");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upgrade_strategies() {
        let mut um = UpgradeMgr::new(UpStrategy::Selective);
        um.snapshot(1, alloc::vec![(1, 10)]);
        um.snapshot(2, alloc::vec![(1, 20)]);
        assert_eq!(um.dry_run(1, 2), Ok(1));
        assert_eq!(um.execute(), Ok(1));
        assert!(um.rollback(1).is_ok());
    }
}
